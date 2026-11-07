//! Local app-settings drafts and focus. Accepted preferences belong to the app.

use std::path::PathBuf;

mod access_safety;
mod appearance;
mod capabilities;
mod controls;
mod custom_themes;
mod focus;
mod folder;
pub(crate) mod font_explorer;
pub(crate) mod font_picker;
mod layout;
pub(crate) mod style;
mod typography;
mod view;

pub(crate) use access_safety::AccessSafetyEdit;
pub(crate) use appearance::AppearanceEdit;
pub(crate) use font_explorer::FontExplorerEdit;
pub(crate) use font_picker::FontPickerEdit;
pub(crate) use view::view;

use iced::widget::operation;
use iced::{Event, Task, keyboard};

use crate::application::Action;
use crate::application::contracts::configuration::parse_new_box_folder;
use crate::domain::ViewId;

use crate::{Desktop, Message, boxes::navigation_header};

pub(crate) const FOLDER_FIELD: &str = "app-settings-folder";

pub(crate) struct Draft {
    folder: String,
    original: Option<PathBuf>,
    edited: bool,
    chosen: Option<PathBuf>,
    picker: Option<u64>,
    error: Option<String>,
    appearance: appearance::Draft,
    typography: crate::application::preferences::typography::TypographyPreferences,
    font_picker_open: bool,
    font_picker_cursor: Option<usize>,
    font_explorer: Option<font_explorer::ExplorerDraft>,
    safety: crate::domain::SafetyPreferences,
    access_safety: access_safety::AccessSafetyDraft,
    capabilities: crate::domain::CapabilityPreferences,
    return_focus: Option<ViewId>,
}

impl Draft {
    fn folder_value(&self, home: &std::path::Path) -> Result<Option<PathBuf>, String> {
        if self.edited {
            if let Some(path) = &self.chosen {
                crate::application::contracts::configuration::validate_saved_path(path)
                    .map_err(|_| "Cannot apply starting folder: the path is invalid or exceeds 4,096 bytes. Shorten or edit it and retry.".to_owned())?;
                return Ok(Some(path.clone()));
            }
            parse_new_box_folder(&self.folder, home)
        } else {
            Ok(self.original.clone())
        }
    }

    fn is_dirty(&self, application: &crate::application::Application) -> bool {
        self.folder_value(application.home()).as_ref() != Ok(&self.original)
            || !self
                .appearance
                .matches(application.appearance(), application.user_themes())
            || self.typography != application.typography()
            || self.safety != application.safety()
            || self.capabilities != application.capabilities()
    }

    pub(crate) fn observe_system_appearance(
        &mut self,
        system: crate::application::preferences::appearance::SystemAppearance,
    ) {
        self.appearance.observe_system(system);
    }

    pub(crate) fn picker(&self) -> Option<u64> {
        (!self.appearance.custom_themes_open() && self.font_explorer.is_none())
            .then_some(self.picker)
            .flatten()
    }

    pub(crate) fn clear_picker(&mut self) {
        self.picker = None;
    }

    pub(crate) fn close_transient_menus(&mut self) {
        self.font_picker_open = false;
        self.font_picker_cursor = None;
    }

    pub(crate) fn restore_focus_after_overlay(&self) -> Task<Message> {
        if self.appearance.custom_themes_open() {
            operation::focus(custom_themes::CANCEL_ID)
        } else if self.font_explorer.is_some() {
            operation::focus(font_explorer::SEARCH_ID)
        } else {
            operation::focus("app-settings-cancel")
        }
    }
}

impl Desktop {
    /// Draft appearance is paint-only. Persistence and application decisions
    /// continue to read accepted preferences until Apply.
    pub(crate) fn preview_appearance(
        &self,
    ) -> crate::application::preferences::appearance::ResolvedAppearance {
        let accepted = self.application.resolved_appearance(self.system_appearance);
        if !self.application.app_settings_open()
            || self.native_menu.error().is_some()
            || self.storage_details && self.application.storage_detail().is_some()
        {
            return accepted;
        }
        self.app_settings
            .as_ref()
            .map_or(accepted, |draft| draft.appearance.preview())
    }

    pub(crate) fn modal_open(&self) -> bool {
        self.native_menu.error().is_some()
            || self.about.is_some()
            || self.app_settings.is_some()
            || self.workspace_management.is_some()
            || self.storage_details && self.application.storage_detail().is_some()
    }

    pub(crate) fn workspace_input_enabled(&self) -> bool {
        self.application.accepts_workspace_input()
            && !self.modal_open()
            && !self.context_menu_open()
    }

    pub(crate) fn permits_ui_action(&self, action: &Action) -> bool {
        // Widget messages arrive before the event subscription's modal guard.
        !(self.modal_open() || self.context_menu_open())
            || !matches!(
                action,
                Action::OpenFile(..)
                    | Action::ActivateLauncher(_)
                    | Action::ConfigureLauncher(..)
                    | Action::BeginItemDraw(..)
                    | Action::StartFileOperation { .. }
                    | Action::ResolveFileConflict(..)
                    | Action::OpenWorkspaceManagement
                    | Action::OpenAppSettings
                    | Action::SelectMode(_)
                    | Action::ChangeTabs(..)
                    | Action::EditScope(..)
                    | Action::ToggleTree(..)
                    | Action::ConfigurePath(..)
                    | Action::BeginDraw(..)
                    | Action::DrawTo(_)
                    | Action::FinishDraw
                    | Action::SelectBox(_)
                    | Action::BeginLayout { .. }
                    | Action::LayoutTo(_)
                    | Action::FinishLayout
                    | Action::RemoveBox(_)
                    | Action::RemoveSelected
                    | Action::Undo
                    | Action::Escape
                    | Action::Navigate(..)
                    | Action::ReloadAll
                    | Action::ConfigureBox(_)
                    | Action::SetBoxSetting(..)
                    | Action::SetSort(..)
                    | Action::ConfigureFolder(..)
                    | Action::ConfigureCurrentFolder(_)
                    | Action::ConfiguredFolderEdited(_)
            )
    }

    pub(crate) fn app_settings_message(&mut self, message: Message) -> Task<Message> {
        if self.about.is_some() && !matches!(message, Message::AppFolderChosen(..)) {
            return Task::none();
        }
        match message {
            Message::OpenAppSettings => self.open_app_settings(),
            Message::AppSettingsChanged(folder) => {
                if let Some(draft) = &mut self.app_settings {
                    if draft.picker.is_some() {
                        return Task::none();
                    }
                    draft.folder = folder;
                    draft.chosen = None;
                    draft.edited = true;
                    draft.error = None;
                }
                Task::none()
            }
            Message::ChooseAppFolder => self.choose_settings_folder(),
            Message::AppFolderChosen(generation, result) => {
                self.settings_folder_chosen(generation, result)
            }
            Message::AppAppearanceEdited(edit) => {
                if let Some(draft) = &mut self.app_settings {
                    draft.font_picker_open = false;
                    draft.font_picker_cursor = None;
                    return draft.appearance.edit(edit);
                }
                Task::none()
            }
            Message::AppFontPicker(edit) => self.edit_font_picker(edit),
            Message::AppFontExplorer(edit) => self.edit_font_explorer(edit),
            Message::AppTypographyChanged(typography) => {
                if typography.is_valid()
                    && let Some(draft) = &mut self.app_settings
                {
                    draft.typography = typography;
                }
                Task::none()
            }
            Message::AppAccessSafetyEdited(edit) => {
                let probe_os = matches!(self.directory, crate::DirectoryBackend::Native(_));
                let home = self.application.home().to_path_buf();
                if let Some(draft) = &mut self.app_settings {
                    draft
                        .access_safety
                        .apply(edit, &mut draft.safety, &home, probe_os);
                }
                Task::none()
            }
            Message::AppCapabilitiesChanged(capabilities) => {
                if let Some(draft) = &mut self.app_settings {
                    draft.capabilities = capabilities;
                }
                Task::none()
            }
            Message::ApplyAppSettings => self.apply_app_settings(),
            Message::CancelAppSettings => {
                let focus = self.take_settings_focus();
                let task = self.dispatch(Action::CancelAppSettings);
                Task::batch([task, self.restore_workspace_focus(focus)])
            }
            _ => Task::none(),
        }
    }

    fn open_app_settings(&mut self) -> Task<Message> {
        if self.modal_open() || !self.application.can_configure_application() {
            return Task::none();
        }
        let original = self
            .application
            .configuration()
            .and_then(|(preferences, _)| preferences.new_box_folder.clone());
        let probe_os = matches!(self.directory, crate::DirectoryBackend::Native(_));
        let draft = Draft {
            folder: original
                .as_deref()
                .map_or_else(String::new, |path| path.to_string_lossy().into_owned()),
            original,
            edited: false,
            chosen: None,
            picker: None,
            error: None,
            appearance: appearance::Draft::new(
                self.application.appearance(),
                self.system_appearance,
                self.application.user_themes(),
            ),
            typography: self.application.typography(),
            font_picker_open: false,
            font_picker_cursor: None,
            font_explorer: None,
            safety: self.application.safety(),
            access_safety: access_safety::AccessSafetyDraft::new(&self.application, probe_os),
            capabilities: self.application.capabilities(),
            return_focus: self.focused_path.take(),
        };
        self.focus_version = self.focus_version.saturating_add(1);
        self.pointer = None;
        for presentation in self.views.values_mut() {
            presentation.hovered = None;
        }
        let task = self.dispatch(Action::OpenAppSettings);
        if self.application.app_settings_open() {
            self.app_settings = Some(draft);
        }
        Task::batch([task, focus::field(appearance::LIGHT_THEME_ID)])
    }

    fn apply_app_settings(&mut self) -> Task<Message> {
        if self.settings_picker.pending() {
            return Task::none();
        }
        if self
            .app_settings
            .as_ref()
            .is_some_and(|draft| !draft.is_dirty(&self.application))
        {
            return Task::none();
        }
        let Some(draft) = &mut self.app_settings else {
            return Task::none();
        };
        let folder = draft.folder_value(self.application.home());
        draft.error = folder.as_ref().err().cloned();
        let validated = draft.appearance.validate();
        let (Ok(folder), Some((appearance, user_themes))) = (folder, validated) else {
            let field = if draft.error.is_some() {
                FOLDER_FIELD
            } else if crate::application::preferences::appearance::Percent::new(
                draft.appearance.transparency,
            )
            .is_err()
            {
                appearance::TRANSPARENCY_ID
            } else {
                appearance::BOX_TRANSPARENCY_ID
            };
            return focus::field(field);
        };
        let focus = draft.return_focus;
        let typography = draft.typography;
        let safety = draft.safety;
        let capabilities = draft.capabilities;
        let task = self.dispatch(Action::ApplyAppSettings {
            new_box_folder: folder,
            appearance,
            user_themes,
            typography,
            safety,
            capabilities,
        });
        if !self.application.app_settings_open() {
            return Task::batch([task, self.restore_workspace_focus(focus)]);
        }
        Task::batch([task, iced::advanced::widget::operate(focus::reveal())])
    }

    fn take_settings_focus(&mut self) -> Option<ViewId> {
        self.app_settings
            .take()
            .and_then(|draft| draft.return_focus)
    }

    pub(crate) fn restore_workspace_focus(&mut self, focus: Option<ViewId>) -> Task<Message> {
        self.focus_version = self.focus_version.saturating_add(1);
        self.focused_path = focus.filter(|view| {
            self.application.workspace().can_browse(*view)
                && self.application.workspace().view(*view).is_some()
        });
        self.focused_path.map_or_else(
            || operation::focus("workspace-canvas"),
            |view| operation::focus(navigation_header::path_id(view)),
        )
    }

    fn edit_font_picker(&mut self, edit: FontPickerEdit) -> Task<Message> {
        use crate::application::preferences::typography::Typeface;
        let Some(draft) = &mut self.app_settings else {
            return Task::none();
        };
        if draft.picker.is_some() || draft.font_explorer.is_some() {
            return Task::none();
        }
        match edit {
            FontPickerEdit::Toggle => {
                draft.font_picker_open = !draft.font_picker_open;
                draft.font_picker_cursor = None;
                focus::field(font_picker::TRIGGER_ID)
            }
            FontPickerEdit::Close => {
                draft.font_picker_open = false;
                draft.font_picker_cursor = None;
                Task::none()
            }
            FontPickerEdit::StepStandard(delta) => {
                if let Some(index) = Typeface::STANDARD
                    .iter()
                    .position(|face| *face == draft.typography.typeface)
                {
                    let next_index = if delta < 0 {
                        index.saturating_sub(1)
                    } else {
                        (index + 1).min(Typeface::STANDARD.len() - 1)
                    };
                    let next = self
                        .system_font
                        .select_typeface(draft.typography, Typeface::STANDARD[next_index]);
                    if next.is_valid() {
                        draft.typography = next;
                    }
                }
                Task::none()
            }
            FontPickerEdit::MoveCursor(delta) => {
                let initial = Typeface::STANDARD
                    .iter()
                    .position(|face| *face == draft.typography.typeface)
                    .unwrap_or(font_picker::EXPLORER_INDEX);
                let current = draft.font_picker_cursor.unwrap_or(initial);
                let next = if delta < 0 {
                    current.saturating_sub(1)
                } else {
                    (current + 1).min(font_picker::MENU_COUNT - 1)
                };
                draft.font_picker_cursor = Some(next);
                Task::none()
            }
            FontPickerEdit::ActivateCursor => {
                let target = draft.font_picker_cursor;
                draft.font_picker_open = false;
                draft.font_picker_cursor = None;
                match target {
                    Some(index) if index < Typeface::STANDARD.len() => {
                        let next = self
                            .system_font
                            .select_typeface(draft.typography, Typeface::STANDARD[index]);
                        if next.is_valid() {
                            draft.typography = next;
                        }
                        focus::field(font_picker::TRIGGER_ID)
                    }
                    Some(font_picker::EXPLORER_INDEX) => self.open_font_explorer(),
                    _ => focus::field(font_picker::TRIGGER_ID),
                }
            }
            FontPickerEdit::Select(face) => {
                draft.font_picker_open = false;
                draft.font_picker_cursor = None;
                let next = self.system_font.select_typeface(draft.typography, face);
                if next.is_valid() {
                    draft.typography = next;
                }
                focus::field(font_picker::TRIGGER_ID)
            }
            FontPickerEdit::OpenExplorer => self.open_font_explorer(),
        }
    }

    fn open_font_explorer(&mut self) -> Task<Message> {
        let Some(draft) = &mut self.app_settings else {
            return Task::none();
        };
        draft.font_picker_open = false;
        draft.font_picker_cursor = None;
        draft.font_explorer = Some(font_explorer::ExplorerDraft::new(draft.typography));
        Task::batch([
            operation::focus(font_explorer::SEARCH_ID),
            font_explorer::reveal_selected(),
        ])
    }

    fn edit_font_explorer(&mut self, edit: FontExplorerEdit) -> Task<Message> {
        let Some(draft) = &mut self.app_settings else {
            return Task::none();
        };
        let Some(explorer) = &mut draft.font_explorer else {
            return Task::none();
        };
        match edit {
            FontExplorerEdit::QueryChanged(query) => {
                explorer.query = query;
                font_explorer::reset_scroll()
            }
            FontExplorerEdit::Step(delta) => {
                if let Some(face) = font_explorer::step_match(
                    explorer.typography.typeface,
                    &explorer.query,
                    delta,
                    &self.system_font,
                ) {
                    let next = self.system_font.select_typeface(explorer.typography, face);
                    if next.is_valid() {
                        explorer.typography = next;
                    }
                    return font_explorer::reveal_selected();
                }
                Task::none()
            }
            FontExplorerEdit::SubmitSearch => {
                if !explorer.query.trim().is_empty()
                    && !font_explorer::contains_match(
                        explorer.typography.typeface,
                        &explorer.query,
                        &self.system_font,
                    )
                    && let Some(face) =
                        font_explorer::first_match(&explorer.query, &self.system_font)
                {
                    let next = self.system_font.select_typeface(explorer.typography, face);
                    if next.is_valid() {
                        explorer.typography = next;
                    }
                    return font_explorer::reveal_selected();
                }
                Task::none()
            }
            FontExplorerEdit::SelectTypeface(face) => {
                let next = self.system_font.select_typeface(explorer.typography, face);
                if next.is_valid() {
                    explorer.typography = next;
                }
                Task::none()
            }
            FontExplorerEdit::SizeChanged(size) => {
                explorer.typography.size = size;
                Task::none()
            }
            FontExplorerEdit::WeightChanged(weight) => {
                let next = crate::application::preferences::typography::TypographyPreferences {
                    weight,
                    ..explorer.typography
                };
                if next.is_valid() {
                    explorer.typography = next;
                }
                Task::none()
            }
            FontExplorerEdit::Cancel => {
                draft.font_explorer = None;
                focus::field(font_picker::TRIGGER_ID)
            }
            FontExplorerEdit::Done => {
                let chosen = explorer.typography;
                draft.font_explorer = None;
                if chosen.is_valid() {
                    draft.typography = chosen;
                }
                focus::field(font_picker::TRIGGER_ID)
            }
        }
    }

    pub(crate) fn app_settings_input(
        &mut self,
        event: Event,
        status: iced::event::Status,
    ) -> Task<Message> {
        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            repeat,
            ..
        }) = event
        {
            if status == iced::event::Status::Ignored
                && self
                    .app_settings
                    .as_ref()
                    .is_some_and(|draft| draft.font_explorer.is_some())
            {
                match key {
                    keyboard::Key::Named(keyboard::key::Named::ArrowUp) if modifiers.is_empty() => {
                        return self.edit_font_explorer(FontExplorerEdit::Step(-1));
                    }
                    keyboard::Key::Named(keyboard::key::Named::ArrowDown)
                        if modifiers.is_empty() =>
                    {
                        return self.edit_font_explorer(FontExplorerEdit::Step(1));
                    }
                    _ => {}
                }
            }
            if repeat {
                return Task::none();
            }
            match key {
                keyboard::Key::Named(keyboard::key::Named::Escape) => {
                    if let Some(draft) = &mut self.app_settings
                        && draft.font_picker_open
                    {
                        draft.font_picker_open = false;
                        draft.font_picker_cursor = None;
                        return focus::field(font_picker::TRIGGER_ID);
                    }
                    if self
                        .app_settings
                        .as_ref()
                        .is_some_and(|draft| draft.font_explorer.is_some())
                    {
                        return self.edit_font_explorer(FontExplorerEdit::Cancel);
                    }
                    if self
                        .app_settings
                        .as_ref()
                        .is_some_and(|draft| draft.appearance.custom_themes_open())
                    {
                        return self.app_settings_message(Message::AppAppearanceEdited(
                            AppearanceEdit::Custom(custom_themes::Edit::Cancel),
                        ));
                    }
                    return self.app_settings_message(Message::CancelAppSettings);
                }
                keyboard::Key::Named(keyboard::key::Named::Tab) => {
                    let advance = if modifiers.shift() {
                        operation::focus_previous()
                    } else {
                        operation::focus_next()
                    };
                    if let Some(draft) = &mut self.app_settings
                        && draft.font_picker_open
                    {
                        draft.font_picker_open = false;
                        draft.font_picker_cursor = None;
                        return operation::focus(font_picker::TRIGGER_ID)
                            .chain(advance)
                            .chain(iced::advanced::widget::operate(focus::reveal()));
                    }
                    return advance.chain(iced::advanced::widget::operate(focus::reveal()));
                }
                _ => {}
            }
        }
        Task::none()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::application::Application;
    use crate::application::contracts::configuration::{
        AppPreferences, WorkspaceDocument, WorkspaceId, WorkspaceIndex, WorkspaceSummary,
    };
    use crate::application::contracts::storage::{LoadedConfiguration, StorageCompletion};
    use crate::domain::{Cell, GridExtent, Mode, Workspace};

    pub(crate) fn ready_desktop(folder: Option<PathBuf>) -> Desktop {
        let (mut application, effects) = Application::with_storage(None, "/Users/demo".into());
        let mut workspace = Workspace::default();
        workspace.select_mode(Mode::Layout);
        workspace.begin_draw(Cell::new(0, 0), GridExtent::new(12, 8));
        workspace.draw_to(Cell::new(3, 2));
        workspace.finish_draw("/Users/demo/Documents".into());
        let id = WorkspaceId::random();
        if let Some(crate::application::Effect::LoadConfiguration(session)) = effects.first() {
            application.update(Action::StorageCompleted(StorageCompletion::Loaded {
                session: *session,
                result: Ok(Box::new(LoadedConfiguration {
                    preferences: AppPreferences {
                        last_workspace_id: Some(id),
                        window: None,
                        new_box_folder: folder,
                        appearance: Default::default(),
                        typography: Default::default(),
                        safety: Default::default(),
                        capabilities: Default::default(),
                    },
                    index: WorkspaceIndex {
                        workspaces: vec![WorkspaceSummary {
                            id,
                            name: "Main".into(),
                        }],
                    },
                    active: WorkspaceDocument { id, workspace },
                    user_themes: Default::default(),
                    user_themes_notice: None,
                    recovery_issues: vec![],
                    backup: None,
                    notice: None,
                })),
            }));
        }
        Desktop::new(
            application,
            crate::DirectoryBackend::Controlled(crate::filesystem::ControlledDirectory::default()),
        )
    }

    #[cfg(unix)]
    #[test]
    fn app_settings_keeps_opaque_paths_without_a_lossy_rewrite_until_edited() {
        use std::os::unix::ffi::OsStringExt;
        let path = PathBuf::from(std::ffi::OsString::from_vec(
            b"/fixture/opaque-\xff".to_vec(),
        ));
        let mut desktop = ready_desktop(Some(path.clone()));
        let _ = desktop.update(Message::OpenAppSettings);
        let _ = desktop.update(Message::ApplyAppSettings);
        assert_eq!(desktop.application.new_box_folder(), path);
        let _ = desktop.update(Message::OpenAppSettings);
        let _ = desktop.update(Message::AppSettingsChanged(String::new()));
        let _ = desktop.update(Message::ApplyAppSettings);
        assert_eq!(
            desktop.application.new_box_folder(),
            desktop.application.home()
        );
    }
}
