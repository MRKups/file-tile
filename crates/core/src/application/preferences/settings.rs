//! App-wide preferences and modal permissions, independent of widgets and disk.

use crate::diagnostics::{Change, configuration_changed};
use std::path::{Path, PathBuf};

use crate::application::contracts::configuration::validate_saved_path;
use crate::application::contracts::storage::{CloseStage, PersistenceState};
use crate::application::preferences::appearance::{
    AppearancePreferences, ResolvedAppearance, SystemAppearance, ThemeChoice, UserTheme,
};
use crate::application::{Action, Application, Effect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::application) enum ConfigurationEditor {
    ApplicationSettings,
    Workspaces,
}

impl Application {
    pub(in crate::application) fn configuration_blocks(
        &self,
        action: &crate::application::Action,
    ) -> bool {
        self.configuration_editor.is_some()
            && !matches!(
                action,
                crate::application::Action::ListingCompleted(_)
                    | crate::application::Action::WatchCompleted(_)
                    | crate::application::Action::LocationsLoaded { .. }
                    | crate::application::Action::CancelPointer
                    | crate::application::Action::MetadataNeeded(..)
                    | crate::application::Action::MetadataCompleted(_)
                    | crate::application::Action::ExternalOpened(_)
            )
    }
    pub(in crate::application) fn configuration_action(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::OpenAppSettings => self.open_app_settings(),
            Action::ApplyAppSettings {
                new_box_folder,
                appearance,
                user_themes,
                typography,
                safety,
                capabilities,
            } => self.apply_app_settings(
                new_box_folder,
                appearance,
                user_themes,
                typography,
                safety,
                capabilities,
            ),
            Action::CancelAppSettings => {
                if self.app_settings_open() {
                    self.configuration_editor = None;
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    pub fn can_configure_application(&self) -> bool {
        self.management.is_none()
            && !self.files_closing()
            && matches!(
                self.persistence,
                PersistenceState::Ready {
                    closing: CloseStage::Open,
                    ..
                }
            )
    }

    pub fn app_settings_open(&self) -> bool {
        self.configuration_editor == Some(ConfigurationEditor::ApplicationSettings)
    }

    pub fn accepts_workspace_input(&self) -> bool {
        self.accepts_edits() && self.configuration_editor.is_none()
    }

    pub fn directory_override(&self) -> Option<&Path> {
        self.directory_override.as_deref()
    }

    pub fn appearance(&self) -> AppearancePreferences {
        self.configuration
            .as_ref()
            .map(|(preferences, _)| preferences.appearance)
            .unwrap_or_default()
    }

    pub fn user_themes(&self) -> &[UserTheme] {
        &self.user_themes
    }

    pub fn user_themes_notice(&self) -> Option<&str> {
        self.user_themes_notice.as_deref()
    }

    pub fn resolved_appearance(&self, system: SystemAppearance) -> ResolvedAppearance {
        self.appearance().resolve(system, &self.user_themes)
    }

    pub fn typography(&self) -> crate::application::preferences::typography::TypographyPreferences {
        self.configuration
            .as_ref()
            .map(|(preferences, _)| preferences.typography)
            .unwrap_or_default()
    }

    pub fn safety(&self) -> crate::domain::SafetyPreferences {
        self.configuration
            .as_ref()
            .map(|(preferences, _)| preferences.safety.clone())
            .unwrap_or_default()
    }

    pub fn capabilities(&self) -> crate::domain::CapabilityPreferences {
        self.configuration
            .as_ref()
            .map(|(preferences, _)| preferences.capabilities)
            .unwrap_or_default()
    }

    pub fn new_box_folder(&self) -> &Path {
        self.directory_override
            .as_deref()
            .or_else(|| {
                self.configuration
                    .as_ref()
                    .and_then(|(preferences, _)| preferences.new_box_folder.as_deref())
            })
            .unwrap_or(&self.home)
    }

    pub(in crate::application) fn open_app_settings(&mut self) -> Vec<Effect> {
        if self.can_configure_application() && self.configuration_editor.is_none() {
            self.pending_tabs = None;
            let mut effects = self.cancel_folder_configuration();
            self.workspace.cancel_pointer();
            self.configuration_editor = Some(ConfigurationEditor::ApplicationSettings);
            effects.extend(self.refresh_locations());
            return effects;
        }
        Vec::new()
    }

    pub(in crate::application) fn apply_app_settings(
        &mut self,
        new_box_folder: Option<PathBuf>,
        appearance: AppearancePreferences,
        user_themes: Vec<UserTheme>,
        typography: crate::application::preferences::typography::TypographyPreferences,
        safety: crate::domain::SafetyPreferences,
        capabilities: crate::domain::CapabilityPreferences,
    ) -> Vec<Effect> {
        let valid_choice = |choice: ThemeChoice| match choice {
            ThemeChoice::Preset(_) => true,
            ThemeChoice::Custom(id) => user_themes.iter().any(|theme| theme.id == id),
        };
        let valid_custom =
            valid_choice(appearance.light_theme) && valid_choice(appearance.dark_theme);
        if !self.can_configure_application()
            || !self.app_settings_open()
            || !typography.is_valid()
            || !valid_custom
            || safety.disabled_locations.len() > crate::domain::MAX_DISABLED_LOCATIONS
            || UserTheme::validate_collection(&user_themes).is_err()
            || new_box_folder
                .as_deref()
                .is_some_and(|path| validate_saved_path(path).is_err())
        {
            return Vec::new();
        }
        let Some((preferences, _)) = &mut self.configuration else {
            return Vec::new();
        };
        let previous_safety = preferences.safety.clone();
        let themes_changed = self.user_themes.as_ref() != user_themes.as_slice();
        let changed = themes_changed
            || preferences.new_box_folder != new_box_folder
            || preferences.appearance != appearance
            || preferences.typography != typography
            || preferences.safety != safety
            || preferences.capabilities != capabilities;
        preferences.new_box_folder = new_box_folder;
        preferences.appearance = appearance;
        preferences.typography = typography;
        preferences.safety = safety;
        preferences.capabilities = capabilities;
        if themes_changed {
            self.user_themes = std::sync::Arc::from(user_themes);
            self.user_themes_edited = true;
            self.user_themes_notice = None;
        }
        self.configuration_editor = None;
        if changed {
            let safety_changed = previous_safety != self.safety();
            configuration_changed(Change::AppSettings);
            let mut effects = self.reconcile_safety_views(previous_safety);
            if safety_changed {
                effects.extend(self.refresh_locations());
            }
            effects.extend(self.save_effect(true));
            effects
        } else {
            Vec::new()
        }
    }
}
