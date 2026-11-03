//! Starting-folder draft integration with the shared native chooser and path pill.
use super::{Draft, FOLDER_FIELD, focus};
use crate::{Desktop, Message, widgets::folder_field, window::appearance as a};
use iced::widget::text_input;
use iced::{Element, Fill, Task};
use std::path::PathBuf;

pub(super) const CHOOSE_ID: &str = "app-settings-choose-folder";

impl Desktop {
    pub(super) fn choose_settings_folder(&mut self) -> Task<Message> {
        let Some(draft) = &mut self.app_settings else {
            return Task::none();
        };
        if !self.application.can_configure_application() {
            return Task::none();
        }
        let directory = draft
            .folder_value(self.application.home())
            .ok()
            .flatten()
            .unwrap_or_else(|| self.application.home().into());
        let Some((generation, task)) = self
            .settings_picker
            .start(directory, Message::AppFolderChosen)
        else {
            return Task::none();
        };
        draft.font_picker_open = false;
        draft.font_picker_cursor = None;
        draft.picker = Some(generation);
        task
    }

    pub(super) fn settings_folder_chosen(
        &mut self,
        generation: u64,
        result: Result<Option<PathBuf>, String>,
    ) -> Task<Message> {
        if !self.settings_picker.finish(generation) {
            return Task::none();
        }
        let Some(draft) = &mut self.app_settings else {
            return Task::none();
        };
        if draft.picker != Some(generation) {
            return Task::none();
        }
        draft.picker = None;
        if !self.application.app_settings_open()
            || !self.application.can_configure_application()
            || draft.appearance.custom_themes_open()
            || draft.font_explorer.is_some()
        {
            return Task::none();
        }
        match result {
            Ok(Some(path)) => {
                draft.folder = path.to_string_lossy().into_owned();
                draft.chosen = Some(path);
                draft.edited = true;
                draft.error = None;
            }
            Ok(None) => {}
            Err(error) => draft.error = Some(error),
        }
        focus::field(CHOOSE_ID)
    }
}

pub(super) fn view(
    draft: &Draft,
    home: &std::path::Path,
    busy: bool,
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let invalid = draft.error.is_some();
    let field = text_input(&home.to_string_lossy(), &draft.folder)
        .id(FOLDER_FIELD)
        .padding([10, 14])
        .size(ty.size(a::CONTROL_SIZE))
        .font(ty.font())
        .width(Fill)
        .on_input_maybe((!busy).then_some(Message::AppSettingsChanged))
        .on_submit_maybe((!busy).then_some(Message::ApplyAppSettings))
        .style(move |_, status| folder_field::field_style(palette, status, invalid));
    folder_field::view(
        field.into(),
        CHOOSE_ID,
        (!busy).then_some(Message::ChooseAppFolder),
        palette,
    )
}
