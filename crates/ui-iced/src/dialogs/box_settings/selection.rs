//! Folder chooser targeting and transitions between Configure and its Range page.
use super::{CHOOSE_ID, Draft, FIELDS_ID, folder_id};
use crate::{Desktop, Message, dialogs::range_editor, widgets::folder_field};
use crate::{
    application::{Action, LoadState},
    domain::{ScopeKind, ViewId},
};
use iced::{
    Element, Fill, Task,
    widget::{column, operation, text_input},
};
use std::path::PathBuf;

impl Desktop {
    pub(crate) fn choose_box_folder(&mut self, view: ViewId) -> Task<Message> {
        if !self.workspace_input_enabled() {
            return Task::none();
        }
        let Some(draft) = self
            .box_settings
            .as_mut()
            .filter(|draft| draft.view == view && !draft.range_page)
        else {
            return Task::none();
        };
        let Some(item) = self.application.workspace().view(view) else {
            return Task::none();
        };
        draft.focused = false;
        draft.picker_error = None;
        self.focus_version = self.focus_version.saturating_add(1);
        if item.navigation.scope().kind() == ScopeKind::Range {
            draft.range = range_editor::Draft::new();
            draft
                .range
                .sync(item.navigation.scope().range(), false, false);
            draft.range_page = true;
            self.application.cancel_scope_edit();
            self.retain_configuration_reads();
            return operation::scroll_to(
                range_editor::SCROLL_ID,
                operation::AbsoluteOffset { x: 0.0, y: 0.0 },
            )
            .chain(operation::focus(range_editor::CANCEL_ID));
        }
        let directory = item.navigation.current().to_path_buf();
        let Some((generation, task)) = self
            .settings_picker
            .start(directory, Message::BoxFolderChosen)
        else {
            return Task::none();
        };
        draft.picker = Some(generation);
        task
    }

    pub(crate) fn box_folder_chosen(
        &mut self,
        generation: u64,
        result: Result<Option<PathBuf>, String>,
    ) -> Task<Message> {
        if !self.settings_picker.finish(generation) {
            return Task::none();
        }
        let enabled = self.workspace_input_enabled();
        let Some(draft) = self
            .box_settings
            .as_mut()
            .filter(|draft| draft.picker == Some(generation))
        else {
            return Task::none();
        };
        draft.picker = None;
        if !enabled || draft.range_page {
            return Task::none();
        }
        let view = draft.view;
        let task = match result {
            Ok(Some(path)) => self.dispatch(Action::ConfigurePath(view, path)),
            Ok(None) => Task::none(),
            Err(error) => {
                draft.picker_error = Some(error);
                Task::none()
            }
        };
        Task::batch([task, operation::focus(CHOOSE_ID)])
    }

    pub(crate) fn restore_box_form(&mut self) -> Task<Message> {
        let Some(draft) = &mut self.box_settings else {
            return Task::none();
        };
        if !std::mem::take(&mut draft.form_restore) {
            return Task::none();
        }
        self.focus_version = self.focus_version.saturating_add(1);
        draft.range.focused = None;
        operation::scroll_to(
            FIELDS_ID,
            operation::AbsoluteOffset {
                x: 0.0,
                y: draft.form_offset,
            },
        )
        .chain(operation::focus(CHOOSE_ID))
    }

    pub(crate) fn cancel_range_page(&mut self, view: ViewId) -> Task<Message> {
        let Some(draft) = self
            .box_settings
            .as_mut()
            .filter(|draft| draft.view == view && draft.range_page)
        else {
            return Task::none();
        };
        draft.range_page = false;
        draft.form_restore = true;
        draft.range = range_editor::Draft::new();
        self.application.cancel_scope_edit();
        self.retain_configuration_reads();
        self.restore_box_form()
    }
}

pub(super) fn folder_editor<'a>(
    desktop: &'a Desktop,
    item: crate::domain::ViewRef<'a>,
    draft: &'a Draft,
) -> (Element<'a, Message>, Element<'a, Message>) {
    let error = desktop.application.box_configuration_error(draft.view);
    let palette = desktop.configure_palette();
    let ty = palette.typography;
    let invalid = error.is_some() || draft.picker_error.is_some();
    let view = draft.view;
    let busy = desktop.settings_picker.pending();
    let field = text_input("Folder path", &draft.folder)
        .id(folder_id(view))
        .padding([10, 14])
        .size(ty.size(crate::window::appearance::CONTROL_SIZE))
        .font(ty.font())
        .width(Fill)
        .on_input_maybe((!busy).then_some(move |text| Message::BoxFolderChanged(view, text)))
        .on_submit_maybe((!busy).then_some(Message::CommitBoxFolder(view)))
        .style(move |_, status| folder_field::field_style(palette, status, invalid));
    let tab_number = tab_number(desktop, draft);
    let mut editor = column![
        ty.control(tab_number.map_or_else(
            || "Folder".into(),
            |number| format!("Folder for tab {number}")
        ))
        .color(palette.text),
        folder_field::view(
            field.into(),
            CHOOSE_ID,
            (!busy).then_some(Message::ChooseBoxFolder(view)),
            palette
        )
    ]
    .spacing(7);
    if item.navigation.scope().kind() == ScopeKind::Range
        && let Some(range) = item.navigation.scope().range()
    {
        let origin = crate::boxes::navigation_header::display_path(
            range.start(),
            desktop.application.home(),
        );
        editor = editor
            .push(
                ty.small(format!("● Origin: {origin}"))
                    .color(palette.accent),
            )
            .push(
                ty.small(format!("■ Range ends: {}", range.stops().len()))
                    .color(palette.text),
            );
    }
    let mut feedback = column![].spacing(6);
    for error in [
        error,
        draft.picker_error.as_deref(),
        desktop.application.scope_error(view),
    ]
    .into_iter()
    .flatten()
    {
        feedback = feedback.push(ty.caption(error).color(palette.error));
    }
    if desktop.application.scope_pending(view) {
        feedback = feedback.push(ty.caption("Checking scope…").color(palette.muted));
    }
    if matches!(
        desktop.application.load(view),
        Some(LoadState::Loading {
            intent: crate::domain::NavigationIntent::Configure,
            ..
        })
    ) {
        feedback = feedback.push(ty.caption("Checking folder…").color(palette.muted));
    }
    if item.navigation.current().to_str().is_none() && !draft.edited {
        feedback = feedback.push(
            ty.caption("This path cannot be represented exactly as text. Leave it unchanged to preserve it, or choose a replacement.")
                .color(palette.muted),
        );
    }
    (editor.into(), feedback.into())
}

fn tab_number(desktop: &Desktop, draft: &Draft) -> Option<usize> {
    desktop
        .application
        .workspace()
        .boxes()
        .iter()
        .find(|item| item.id == draft.id)
        .and_then(|item| item.tabs.as_ref())
        .filter(|tabs| tabs.visible())
        .and_then(|tabs| tabs.views().iter().position(|tab| tab.view == draft.view))
        .map(|index| index + 1)
}
