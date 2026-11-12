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
    directory: Option<std::path::PathBuf>,
    previous_folder: Option<std::path::PathBuf>,
    previous_reload: Option<(
        std::path::PathBuf,
        Vec<crate::application::contracts::directory::DirectoryEntry>,
    )>,
}

pub(crate) struct BoxSnapshot {
    active: std::collections::HashMap<crate::domain::BoxId, ViewId>,
    scrollers: std::collections::HashMap<
        crate::domain::BoxId,
        (
            crate::boxes::folder_box::ScrollerGeometry,
            crate::domain::BoxSettings,
        ),
    >,
    typography: crate::application::preferences::typography::TypographyPreferences,
}

impl Desktop {
    pub(crate) fn snapshot_boxes_before(&self) -> BoxSnapshot {
        let geometry = self.geometry();
        let mut active = std::collections::HashMap::new();
        let mut scrollers = std::collections::HashMap::new();
        for item in self.application.workspace().boxes() {
            active.insert(item.id, item.active_view().view);
            scrollers.insert(
                item.id,
                (
                    crate::boxes::folder_box::scroller_geometry(geometry, item),
                    item.settings,
                ),
            );
        }
        BoxSnapshot {
            active,
            scrollers,
            typography: self.application.typography(),
        }
    }

    pub(crate) fn reconcile_boxes_after(&mut self, before: BoxSnapshot) -> Task<Message> {
        Task::batch([
            self.reconcile_tab_activation(before.active),
            self.rescale_box_scrollers(&before.scrollers, before.typography),
        ])
    }

    pub(crate) fn reobserve_active_scrolls(&self) -> Task<Message> {
        let tasks: Vec<Task<Message>> = self
            .application
            .workspace()
            .boxes()
            .iter()
            .filter_map(|item| {
                let view = item.active_view().view;
                self.application.load(view).and_then(LoadState::listing)?;
                let state = self.views.get(&view)?;
                state
                    .pending_scroll_restore
                    .is_none()
                    .then(|| file_list::observe_scroll(view, state.listing_revision))
            })
            .collect();
        Task::batch(tasks)
    }

    fn configure_kept_other_folder(
        &self,
        view: ViewId,
        intent: NavigationIntent,
        configured_dir: Option<&std::path::Path>,
    ) -> bool {
        intent == NavigationIntent::Configure
            && configured_dir.is_some_and(|dir| {
                self.application
                    .workspace()
                    .view(view)
                    .is_some_and(|item| item.navigation.current() != dir)
            })
    }

    fn configure_preserved_same_tab_folder(
        &self,
        view: ViewId,
        intent: NavigationIntent,
        previous_folder: Option<&std::path::Path>,
    ) -> bool {
        intent == NavigationIntent::Configure
            && self
                .application
                .workspace()
                .box_for_view(view)
                .is_some_and(crate::domain::FolderBox::has_visible_tabs)
            && previous_folder.is_some_and(|prev| {
                self.application
                    .workspace()
                    .view(view)
                    .is_some_and(|item| item.navigation.current() == prev)
            })
    }

    pub(crate) fn will_reset_navigated_presentation(
        &self,
        context: Option<&ListingCompletionContext>,
    ) -> Option<ViewId> {
        let ctx = context?;
        let succeeded = ctx.succeeded
            && !(ctx.intent == NavigationIntent::Configure
                && self.application.box_configuration_error(ctx.view).is_some());
        (succeeded
            && !self.configure_kept_other_folder(ctx.view, ctx.intent, ctx.directory.as_deref())
            && !self.configure_preserved_same_tab_folder(
                ctx.view,
                ctx.intent,
                ctx.previous_folder.as_deref(),
            )
            && !matches!(
                ctx.intent,
                NavigationIntent::Reload | NavigationIntent::Activate
            )
            && self
                .application
                .load(ctx.view)
                .and_then(LoadState::listing)
                .is_some())
        .then_some(ctx.view)
    }

    pub(crate) fn apply_pending_scroll_restore(&mut self, view: ViewId) -> Task<Message> {
        if !self.application.workspace().is_active(view)
            || !self.application.is_sort_ready(view)
            || matches!(
                self.application.load(view),
                Some(LoadState::Loading { intent, .. }) if *intent != NavigationIntent::Reload
            )
        {
            return Task::none();
        }
        if !self
            .views
            .get(&view)
            .is_some_and(|state| state.pending_scroll_restore.is_some())
        {
            return Task::none();
        }
        let Some(item) = self.application.workspace().box_for_view(view) else {
            return Task::none();
        };
        let Some(view_ref) = self.application.workspace().view(view) else {
            return Task::none();
        };
        let Some(listing) = self.application.load(view).and_then(LoadState::listing) else {
            return Task::none();
        };
        let geometry = self.geometry();
        let scroller = crate::boxes::folder_box::scroller_geometry(geometry, item);
        let visible_count = listing.display_len(view_ref.settings.show_hidden);
        let Some(presentation) = self.views.get_mut(&view) else {
            return Task::none();
        };
        let Some(top_item) = presentation.pending_scroll_restore.take() else {
            return Task::none();
        };
        let y = crate::boxes::scroll_position::top_item_to_offset(
            top_item.get(),
            scroller,
            view_ref.settings.file_view,
            visible_count,
            geometry.typography,
        );
        presentation.scroll = file_list::ScrollWindow {
            offset: y,
            height: scroller.viewport_height,
        };
        presentation.scroll_x = 0.0;
        let generation = presentation.listing_revision;
        self.sync_view_top_item(view);
        operation::scroll_to(
            file_list::scroll_id(view),
            operation::AbsoluteOffset { x: 0.0, y },
        )
        .chain(file_list::observe_scroll(view, generation))
    }

    pub(crate) fn complete_listing_task(
        &mut self,
        context: Option<ListingCompletionContext>,
    ) -> Task<Message> {
        context.map_or_else(Task::none, |mut ctx| {
            ctx.succeeded = ctx.succeeded
                && !(ctx.intent == crate::domain::NavigationIntent::Configure
                    && self.application.box_configuration_error(ctx.view).is_some());
            self.completed_listing(ctx)
        })
    }
    pub(crate) fn listing_message(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::FolderFocused(view) => {
                if !self.file_view_input_enabled(view) {
                    return Task::none();
                }
                self.type_jump.dismiss_session();
                let state = self.views.entry(view).or_default();
                state.selected = None;
                state.selection.clear();
                state.pending_selection.clear();
                state.reveal_selected = false;
                state.reveal_after_motion = false;
                state.earlier_ancestors = false;
                state.selection_anchor = None;
                self.cancel_pending_scroll_restore(view);
                self.activate_view(view);
                self.focus_canvas()
            }
            Message::FileViewportScrolled(view, offset, height, user_scrolled) => {
                self.record_file_scroll(view, offset, height, user_scrolled)
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
        if self
            .application
            .load(view)
            .and_then(LoadState::listing)
            .is_none()
        {
            return Task::none();
        }
        let updated = if let Some(presentation) = self.views.get_mut(&view)
            && presentation.listing_revision == generation
            && self.application.workspace().is_active(view)
        {
            if presentation.pending_scroll_restore.is_some() {
                presentation.scroll.height = window.height;
                false
            } else {
                presentation.scroll = window;
                true
            }
        } else {
            false
        };
        if updated {
            self.sync_view_top_item(view);
        }
        Task::none()
    }

    pub(crate) fn record_file_scroll(
        &mut self,
        view: ViewId,
        offset: iced::widget::scrollable::AbsoluteOffset,
        height: f32,
        user_scrolled: bool,
    ) -> Task<Message> {
        if !self.workspace_input_enabled()
            || !self.application.workspace().can_browse(view)
            || self
                .application
                .load(view)
                .and_then(LoadState::listing)
                .is_none()
        {
            return Task::none();
        }
        let state = self.views.entry(view).or_default();
        if user_scrolled {
            state.pending_scroll_restore = None;
            state.scroll_x = offset.x;
            state.scroll = file_list::ScrollWindow {
                offset: offset.y,
                height,
            };
            self.sync_view_top_item(view);
        } else if state.pending_scroll_restore.is_some() {
            state.scroll.height = height;
        } else {
            state.scroll_x = offset.x;
            state.scroll = file_list::ScrollWindow {
                offset: offset.y,
                height,
            };
            self.sync_view_top_item(view);
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
                    directory: completion
                        .result
                        .as_ref()
                        .ok()
                        .map(|listing| listing.directory.clone()),
                    previous_folder: self
                        .application
                        .workspace()
                        .view(completion.request.view)
                        .map(|item| item.navigation.current().to_path_buf()),
                    previous_reload,
                })
            }
            _ => None,
        }
    }
    pub(crate) fn list_directory(&mut self, request: ListingRequest) -> Task<Message> {
        let safety = self.application.safety_policy(Some(request.view));
        if let Err(reason) = safety.check_read_directory(&request.directory) {
            return Task::done(Message::Action(Action::ListingCompleted(
                crate::application::contracts::directory::ListingCompletion {
                    result: Err(
                        crate::application::contracts::directory::DirectoryError::access_disabled(
                            request.directory.clone(),
                            reason,
                        ),
                    ),
                    request,
                },
            )));
        }
        let scope = self.application.read_scope(request.view);
        match &self.directory {
            #[cfg(test)]
            DirectoryBackend::Controlled(reader) => {
                let result = reader.list_permitted(
                    &request.directory,
                    &scope,
                    &safety,
                    &crate::application::contracts::directory::Cancellation::default(),
                );
                Task::done(Message::Action(Action::ListingCompleted(
                    ListingCompletion { request, result },
                )))
            }
            DirectoryBackend::Native(workers) => {
                let future = workers.submit_permitted(request.clone(), scope, safety);
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

    fn reload_reveal_target(&self, view: ViewId) -> Option<(f32, f32)> {
        let list_mode = self
            .application
            .workspace()
            .view(view)
            .is_some_and(|item| item.settings.file_view == crate::domain::FileView::List);
        if !list_mode {
            return None;
        }
        let selected = self.views.get(&view)?.selected.as_ref()?;
        let index = self
            .application
            .ordered_entries(view)
            .position(|entry| &entry.path == selected)?;
        let row_height = crate::window::typography::Style(self.application.typography())
            .row(file_list::ROW_HEIGHT);
        Some((index as f32 * row_height, (index + 1) as f32 * row_height))
    }

    fn finish_settled_reload_reveal(
        &mut self,
        view: ViewId,
        intent: NavigationIntent,
        succeeded: bool,
    ) -> Task<Message> {
        let should_consume = self.views.get(&view).is_some_and(|state| {
            state.reveal_after_motion
                && (!succeeded
                    || state
                        .motion
                        .as_ref()
                        .is_none_or(|motion| !motion.has_layout_motion()))
        });
        if !should_consume {
            return Task::none();
        }
        if let Some(state) = self.views.get_mut(&view) {
            state.reveal_after_motion = false;
        }
        if succeeded && intent == NavigationIntent::Reload {
            self.reveal_selected_entry(view)
        } else {
            Task::none()
        }
    }

    fn finish_configure_other_folder(
        &mut self,
        view: ViewId,
        generation: u64,
        intent: NavigationIntent,
        succeeded: bool,
        configured_dir: Option<&std::path::Path>,
    ) -> bool {
        if intent != NavigationIntent::Configure {
            return false;
        }
        self.completed_box_folder(view, generation, succeeded);
        if succeeded && self.configure_kept_other_folder(view, intent, configured_dir) {
            if let Some(presentation) = self.views.get_mut(&view) {
                presentation.submitted_revision = None;
            }
            return true;
        }
        false
    }

    pub(crate) fn completed_listing(&mut self, ctx: ListingCompletionContext) -> Task<Message> {
        let ListingCompletionContext {
            view,
            generation,
            intent,
            succeeded,
            directory: configured_dir,
            previous_folder,
            previous_reload,
        } = ctx;
        if self.finish_configure_other_folder(
            view,
            generation,
            intent,
            succeeded,
            configured_dir.as_deref(),
        ) {
            return Task::none();
        }
        let preserved_tab_folder =
            self.configure_preserved_same_tab_folder(view, intent, previous_folder.as_deref());
        let in_place = matches!(
            intent,
            NavigationIntent::Reload | NavigationIntent::Activate
        ) || preserved_tab_folder;
        let show_hidden = self
            .application
            .workspace()
            .view(view)
            .is_some_and(|item| item.settings.show_hidden);
        let reveal_target = self.reload_reveal_target(view);
        let sort_indices = self.application.sort_indices(view);
        let listing_after = self.application.load(view).and_then(LoadState::listing);
        let mut task = Task::none();
        let mut reloaded_dir = None;
        let mut did_reset_scroll = false;
        {
            let presentation = self.views.entry(view).or_default();
            let unchanged_draft =
                presentation.submitted_revision == Some((generation, presentation.draft_revision));
            // Application admission may already have started the pending catch-up.
            // Its retained listing is still the result we must reconcile now.
            match listing_after {
                Some(listing) if succeeded => {
                    record_reload_motion(
                        intent,
                        previous_reload,
                        listing,
                        sort_indices,
                        show_hidden,
                        presentation,
                    );
                    if in_place {
                        if intent == NavigationIntent::Reload || preserved_tab_folder {
                            reloaded_dir = Some(listing.directory.clone());
                        }
                        if preserved_tab_folder {
                            presentation.draft = None;
                            presentation.invalid = false;
                        }
                        task = reconcile_reload_listing(
                            view,
                            generation,
                            if preserved_tab_folder {
                                NavigationIntent::Reload
                            } else {
                                intent
                            },
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
                        did_reset_scroll = intent != NavigationIntent::Restore;
                    }
                    presentation.listing_revision = generation;
                }
                _ if !succeeded && !in_place && intent != NavigationIntent::Configure => {
                    presentation.invalid = unchanged_draft && presentation.draft.is_some();
                }
                _ => {}
            }
            presentation.submitted_revision = None;
        }
        if did_reset_scroll {
            self.sync_view_top_item(view);
        }
        if let Some(dir) = reloaded_dir {
            self.column_previews.invalidate_directory(&dir);
        }
        let settled_reveal = self.finish_settled_reload_reveal(view, intent, succeeded);
        Task::batch([task, settled_reveal])
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
    if intent != NavigationIntent::Restore {
        presentation.pending_scroll_restore = None;
    }
    presentation.invalid = false;
    presentation.selected = None;
    presentation.selection.clear();
    presentation.pending_selection.clear();
    presentation.reveal_selected = false;
    presentation.reveal_after_motion = false;
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
        presentation.pending_scroll_restore = None;
        presentation.scroll.offset = y;
        operation::scroll_to(
            file_list::scroll_id(view),
            operation::AbsoluteOffset {
                x: presentation.scroll_x,
                y,
            },
        )
        .chain(file_list::observe_scroll(view, generation))
    } else if presentation.pending_scroll_restore.is_some() {
        if intent == NavigationIntent::Activate {
            operation::scroll_to(
                file_list::scroll_id(view),
                operation::AbsoluteOffset { x: 0.0, y: 0.0 },
            )
        } else {
            Task::none()
        }
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
