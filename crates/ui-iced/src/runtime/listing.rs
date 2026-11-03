//! Directory task ownership and accepted-completion presentation updates.

#[cfg(test)]
use crate::application::contracts::directory::{DirectoryReader, ListingCompletion};
use crate::application::{Action, LoadState, contracts::directory::ListingRequest};
use crate::domain::{NavigationIntent, ViewId};
use crate::{Desktop, DirectoryBackend, Message, PendingRead, boxes::file_list};
use iced::{Task, widget::operation};

pub(crate) struct ListingCompletionContext {
    view: ViewId,
    generation: u64,
    intent: NavigationIntent,
    succeeded: bool,
    previous_reload: Option<(
        std::path::PathBuf,
        Vec<crate::application::contracts::directory::DirectoryEntry>,
    )>,
}

impl Desktop {
    pub(crate) fn complete_listing_task(
        &mut self,
        context: Option<ListingCompletionContext>,
    ) -> Task<Message> {
        context.map_or_else(Task::none, |ctx| {
            let succeeded = ctx.succeeded
                && !(ctx.intent == crate::domain::NavigationIntent::Configure
                    && self.application.box_configuration_error(ctx.view).is_some());
            self.completed_listing(
                ctx.view,
                ctx.generation,
                ctx.intent,
                succeeded,
                ctx.previous_reload,
            )
        })
    }
    pub(crate) fn listing_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::FolderFocused(view) => {
                if !self.file_view_input_enabled(view) {
                    return Task::none();
                }
                let state = self.views.entry(view).or_default();
                state.selected = None;
                state.selection.clear();
                state.pending_selection.clear();
                state.reveal_selected = false;
                state.selection_anchor = None;
                self.active_view = Some(view);
                self.focus_canvas()
            }
            Message::FileViewportScrolled(view, offset, height) => {
                self.record_file_scroll(view, offset, height)
            }
            Message::ListingScrollObserved {
                view,
                generation,
                window,
            } => self.observe_listing_scroll(view, generation, window),
            Message::EntrySelected(view, path) => self.select_entry(view, path),
            Message::EntryOpened(view, path) => self.open_entry(view, path),
            Message::EntryHovered(view, path, hovered) => self.hover_entry(view, path, hovered),
            _ => Task::none(),
        }
    }
    pub(crate) fn observe_listing_scroll(
        &mut self,
        view: ViewId,
        generation: u64,
        window: file_list::ScrollWindow,
    ) -> Task<Message> {
        if let Some(presentation) = self.views.get_mut(&view)
            && presentation.listing_revision == generation
            && self.application.workspace().is_active(view)
        {
            presentation.scroll = window;
        }
        Task::none()
    }

    pub(crate) fn record_file_scroll(
        &mut self,
        view: ViewId,
        offset: iced::widget::scrollable::AbsoluteOffset,
        height: f32,
    ) -> Task<Message> {
        if self.workspace_input_enabled() && self.application.workspace().can_browse(view) {
            let state = self.views.entry(view).or_default();
            state.scroll_x = offset.x;
            state.scroll = file_list::ScrollWindow {
                offset: offset.y,
                height,
            };
        }
        Task::none()
    }
    pub(crate) fn listing_completion_context(
        &self,
        action: &Action,
    ) -> Option<ListingCompletionContext> {
        let Action::ListingCompleted(completion) = action else {
            return None;
        };
        if !self.application.accepts_edits() {
            return None;
        }
        match self.application.load(completion.request.view) {
            Some(LoadState::Loading {
                request, intent, ..
            }) if request == &completion.request => {
                let succeeded = completion.result.is_ok();
                let previous_reload = (*intent == NavigationIntent::Reload && succeeded)
                    .then(|| {
                        let prev_listing = self
                            .application
                            .load(completion.request.view)
                            .and_then(LoadState::listing)?;
                        Some((
                            prev_listing.directory.clone(),
                            self.application
                                .ordered_entries(completion.request.view)
                                .cloned()
                                .collect(),
                        ))
                    })
                    .flatten();
                Some(ListingCompletionContext {
                    view: completion.request.view,
                    generation: completion.request.generation,
                    intent: *intent,
                    succeeded,
                    previous_reload,
                })
            }
            _ => None,
        }
    }
    pub(crate) fn list_directory(&mut self, request: ListingRequest) -> Task<Message> {
        if let Err(reason) = self
            .application
            .safety_policy(Some(request.view))
            .check_read_directory(&request.directory)
        {
            return Task::done(Message::Action(Action::ListingCompleted(
                crate::application::contracts::directory::ListingCompletion {
                    result: Err(crate::application::contracts::directory::DirectoryError {
                        path: request.directory.clone(),
                        kind:
                            crate::application::contracts::directory::DirectoryErrorKind::PermissionDenied,
                        detail: Some(reason.to_owned()),
                    }),
                    request,
                },
            )));
        }
        let scope = self.application.read_scope(request.view);
        match &self.directory {
            #[cfg(test)]
            DirectoryBackend::Controlled(reader) => {
                let result = reader.list_scoped(
                    &request.directory,
                    &scope,
                    &crate::application::contracts::directory::Cancellation::default(),
                );
                Task::done(Message::Action(Action::ListingCompleted(
                    ListingCompletion { request, result },
                )))
            }
            DirectoryBackend::Native(workers) => {
                let future = workers.submit_scoped(request.clone(), scope);
                let cancellation = future.cancellation();
                let (task, handle) = Task::perform(future, |completion| {
                    Message::Action(Action::ListingCompleted(completion))
                })
                .abortable();
                self.pending_reads.insert(
                    request.view,
                    PendingRead {
                        request,
                        cancellation,
                        handle,
                    },
                );
                task
            }
        }
    }

    pub(crate) fn completed_listing(
        &mut self,
        view: ViewId,
        generation: u64,
        intent: NavigationIntent,
        succeeded: bool,
        previous_reload: Option<(
            std::path::PathBuf,
            Vec<crate::application::contracts::directory::DirectoryEntry>,
        )>,
    ) -> Task<Message> {
        if intent == NavigationIntent::Configure {
            self.completed_box_folder(view, generation, succeeded);
        }
        let show_hidden = self
            .application
            .workspace()
            .view(view)
            .is_some_and(|item| item.settings.show_hidden);
        let row_height = crate::window::typography::Style(self.application.typography())
            .row(file_list::ROW_HEIGHT);
        let list_mode = self
            .application
            .workspace()
            .view(view)
            .is_some_and(|item| item.settings.file_view == crate::domain::FileView::List);
        let selected_index = self.views.get(&view).and_then(|state| {
            let selected = state.selected.as_ref()?;
            self.application
                .ordered_entries(view)
                .position(|entry| &entry.path == selected)
        });
        let presentation = self.views.entry(view).or_default();
        let unchanged_draft =
            presentation.submitted_revision == Some((generation, presentation.draft_revision));
        let mut task = Task::none();
        let mut reloaded_dir = None;
        // Application admission may already have started the pending catch-up.
        // Its retained listing is still the result we must reconcile now.
        match self.application.load(view).and_then(LoadState::listing) {
            Some(listing) if succeeded => {
                record_reload_motion(
                    intent,
                    previous_reload,
                    listing,
                    self.application.sort_indices(view),
                    show_hidden,
                    presentation,
                );
                if matches!(
                    intent,
                    NavigationIntent::Reload | NavigationIntent::Activate
                ) {
                    if intent == NavigationIntent::Reload {
                        reloaded_dir = Some(listing.directory.clone());
                    }
                    let reveal_target = selected_index
                        .filter(|_| list_mode)
                        .map(|index| (index as f32 * row_height, (index + 1) as f32 * row_height));
                    task = reconcile_reload_listing(
                        view,
                        generation,
                        intent,
                        show_hidden,
                        reveal_target,
                        listing,
                        presentation,
                    );
                } else {
                    task = reset_navigated_presentation(
                        view,
                        intent,
                        unchanged_draft,
                        show_hidden,
                        listing,
                        presentation,
                    );
                }
                presentation.listing_revision = generation;
            }
            _ if !succeeded
                && !matches!(
                    intent,
                    NavigationIntent::Reload | NavigationIntent::Activate
                )
                && intent != NavigationIntent::Configure =>
            {
                presentation.invalid = unchanged_draft && presentation.draft.is_some();
            }
            _ => {}
        }
        presentation.submitted_revision = None;
        if let Some(dir) = reloaded_dir {
            self.column_previews.invalidate_directory(&dir);
        }
        task
    }
}

fn reset_navigated_presentation(
    view: ViewId,
    intent: NavigationIntent,
    unchanged_draft: bool,
    show_hidden: bool,
    listing: &crate::application::contracts::directory::DirectoryListing,
    presentation: &mut crate::ViewPresentation,
) -> Task<Message> {
    let column_focus = presentation.column_focus.take();
    if unchanged_draft || intent == NavigationIntent::Configure {
        presentation.draft = None;
    }
    presentation.invalid = false;
    presentation.selected = None;
    presentation.selection.clear();
    presentation.pending_selection.clear();
    presentation.reveal_selected = false;
    presentation.selection_anchor = None;
    presentation.name_draft = None;
    presentation.hovered = None;
    presentation.hint = None;
    presentation.earlier_ancestors = false;
    presentation.scroll = file_list::ScrollWindow::default();
    presentation.scroll_x = 0.0;
    if let Some(focus) = column_focus
        && focus.directory == listing.directory
        && listing
            .entry(&focus.target)
            .is_some_and(|entry| show_hidden || !entry.hidden)
    {
        presentation.selected = Some(focus.target.clone());
        presentation.selection.insert(focus.target.clone());
        presentation.selection_anchor = Some(focus.target);
    }
    operation::scroll_to(
        file_list::scroll_id(view),
        operation::AbsoluteOffset { x: 0.0, y: 0.0 },
    )
}

fn record_reload_motion(
    intent: NavigationIntent,
    previous_reload: Option<(
        std::path::PathBuf,
        Vec<crate::application::contracts::directory::DirectoryEntry>,
    )>,
    listing: &crate::application::contracts::directory::DirectoryListing,
    current_order: Option<&[usize]>,
    show_hidden: bool,
    presentation: &mut crate::ViewPresentation,
) {
    if intent == NavigationIntent::Reload {
        if listing.origin == crate::application::contracts::directory::ListingOrigin::Local
            && let Some((prev_dir, prev_ordered)) = previous_reload
        {
            let diff = crate::application::files::listing_diff::compute(
                &prev_dir,
                &prev_ordered,
                listing,
                current_order,
                show_hidden,
            );
            crate::boxes::entry_motion::ViewMotion::apply_diff(
                &mut presentation.motion,
                diff,
                listing,
                show_hidden,
                presentation.scroll.offset,
                std::time::Instant::now(),
            );
        }
    } else {
        presentation.motion = None;
    }
}

fn reconcile_reload_listing(
    view: ViewId,
    generation: u64,
    intent: NavigationIntent,
    show_hidden: bool,
    reveal_target: Option<(f32, f32)>,
    listing: &crate::application::contracts::directory::DirectoryListing,
    presentation: &mut crate::ViewPresentation,
) -> Task<Message> {
    let visible = |path: &std::path::Path| {
        listing
            .entry(path)
            .is_some_and(|entry| show_hidden || !entry.hidden)
    };
    presentation.pending_selection.retain(|path| !visible(path));
    presentation
        .selection
        .retain(|path| visible(path) || presentation.pending_selection.contains(path));
    if presentation
        .selected
        .as_ref()
        .is_some_and(|path| !visible(path) && !presentation.pending_selection.contains(path))
    {
        presentation.selected = None;
    }
    if presentation
        .hovered
        .as_ref()
        .is_some_and(|path| !visible(path))
    {
        presentation.hovered = None;
    }
    let reveal_y = if presentation.reveal_selected
        && presentation
            .selected
            .as_ref()
            .is_some_and(|path| visible(path))
    {
        presentation.reveal_selected = false;
        reveal_target
            .filter(|_| presentation.scroll.height > 0.0)
            .and_then(|(top, bottom)| {
                if top < presentation.scroll.offset {
                    Some(top)
                } else if bottom > presentation.scroll.offset + presentation.scroll.height {
                    Some((bottom - presentation.scroll.height).max(0.0))
                } else {
                    None
                }
            })
    } else {
        None
    };
    if let Some(y) = reveal_y {
        presentation.scroll.offset = y;
        operation::scroll_to(
            file_list::scroll_id(view),
            operation::AbsoluteOffset {
                x: presentation.scroll_x,
                y,
            },
        )
        .chain(file_list::observe_scroll(view, generation))
    } else if intent == NavigationIntent::Activate {
        operation::scroll_to(
            file_list::scroll_id(view),
            operation::AbsoluteOffset {
                x: 0.0,
                y: presentation.scroll.offset,
            },
        )
        .chain(file_list::observe_scroll(view, generation))
    } else {
        file_list::observe_scroll(view, generation)
    }
}
