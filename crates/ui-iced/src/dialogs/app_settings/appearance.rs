//! Local theme, custom-theme list, and transparency draft for live Settings preview.

use iced::Task;

use crate::application::preferences::appearance::{
    AppearancePreferences, BoxPercent, Percent, Preset, ResolvedAppearance, SurfaceTexture,
    SystemAppearance, ThemeChoice, UserTheme,
};

use super::custom_themes::{self, EditorDraft, Outcome};
use crate::Message;

pub(super) const LIGHT_THEME_ID: &str = "app-settings-light-theme";
pub(super) const DARK_THEME_ID: &str = "app-settings-dark-theme";
pub(super) const LIGHT_CUSTOM_THEMES_ID: &str = "app-settings-light-custom-themes";
pub(super) const DARK_CUSTOM_THEMES_ID: &str = "app-settings-dark-custom-themes";
pub(super) const TRANSPARENCY_ID: &str = "app-settings-window-transparency";
pub(super) const BOX_TRANSPARENCY_ID: &str = "app-settings-box-transparency";
pub(super) const SURFACE_TEXTURE_ID: &str = "app-settings-surface-texture";

pub(super) const fn theme_id(slot: SystemAppearance) -> &'static str {
    match slot {
        SystemAppearance::Light => LIGHT_THEME_ID,
        SystemAppearance::Dark => DARK_THEME_ID,
    }
}

pub(super) const fn custom_themes_id(slot: SystemAppearance) -> &'static str {
    match slot {
        SystemAppearance::Light => LIGHT_CUSTOM_THEMES_ID,
        SystemAppearance::Dark => DARK_CUSTOM_THEMES_ID,
    }
}

#[derive(Clone, Debug)]
pub(crate) enum AppearanceEdit {
    Theme(SystemAppearance, ThemeChoice),
    Transparency(u8),
    BoxTransparency(u8),
    SurfaceTexture(SurfaceTexture),
    OpenCustomThemes(SystemAppearance),
    Custom(custom_themes::Edit),
}

pub(super) struct Draft {
    pub(super) light_theme: ThemeChoice,
    pub(super) dark_theme: ThemeChoice,
    preview_slot: SystemAppearance,
    pub(super) transparency: u8,
    pub(super) box_transparency: u8,
    pub(super) surface_texture: SurfaceTexture,
    pub(super) user_themes: Vec<UserTheme>,
    pub(super) editor: Option<EditorDraft>,
}

impl Draft {
    pub(super) fn new(
        preferences: AppearancePreferences,
        system: SystemAppearance,
        user_themes: &[UserTheme],
    ) -> Self {
        Self {
            light_theme: sanitize_choice(preferences.light_theme, Preset::LiquidLight, user_themes),
            dark_theme: sanitize_choice(preferences.dark_theme, Preset::LiquidDark, user_themes),
            preview_slot: system,
            transparency: preferences.transparency.get(),
            box_transparency: preferences.box_transparency.get(),
            surface_texture: preferences.surface_texture,
            user_themes: user_themes.to_vec(),
            editor: None,
        }
    }

    pub(super) fn observe_system(&mut self, system: SystemAppearance) {
        self.preview_slot = system;
    }

    pub(super) const fn theme_for(&self, slot: SystemAppearance) -> ThemeChoice {
        match slot {
            SystemAppearance::Light => self.light_theme,
            SystemAppearance::Dark => self.dark_theme,
        }
    }

    pub(super) fn custom_themes_open(&self) -> bool {
        self.editor.is_some()
    }

    pub(super) fn preview(&self) -> ResolvedAppearance {
        let transparency = Percent::new(self.transparency.min(100)).unwrap_or_default();
        let box_transparency =
            BoxPercent::new(self.box_transparency.min(BoxPercent::MAX)).unwrap_or_default();
        if let Some(spec) = self.editor.as_ref().and_then(EditorDraft::preview_spec) {
            return ResolvedAppearance {
                spec,
                liquid_accent: false,
                transparency,
                box_transparency,
                surface_texture: self.surface_texture,
            };
        }
        AppearancePreferences {
            light_theme: self.light_theme,
            dark_theme: self.dark_theme,
            transparency,
            box_transparency,
            surface_texture: self.surface_texture,
        }
        .resolve(self.preview_slot, &self.user_themes)
    }

    pub(super) fn edit(&mut self, edit: AppearanceEdit) -> Task<Message> {
        match edit {
            AppearanceEdit::Theme(slot, choice) => {
                let changed = self.theme_for(slot) != choice;
                match slot {
                    SystemAppearance::Light => self.light_theme = choice,
                    SystemAppearance::Dark => self.dark_theme = choice,
                }
                self.preview_slot = slot;
                if changed {
                    self.transparency = choice.default_transparency();
                }
                Task::none()
            }
            AppearanceEdit::Transparency(value) => {
                self.transparency = value.min(100);
                Task::none()
            }
            AppearanceEdit::BoxTransparency(value) => {
                self.box_transparency = value.min(BoxPercent::MAX);
                Task::none()
            }
            AppearanceEdit::SurfaceTexture(texture) => {
                self.surface_texture = texture;
                Task::none()
            }
            AppearanceEdit::OpenCustomThemes(slot) => {
                self.preview_slot = slot;
                let mut resolved = AppearancePreferences {
                    light_theme: self.light_theme,
                    dark_theme: self.dark_theme,
                    transparency: Percent::new(self.transparency.min(100)).unwrap_or_default(),
                    box_transparency: BoxPercent::new(self.box_transparency.min(BoxPercent::MAX))
                        .unwrap_or_default(),
                    surface_texture: self.surface_texture,
                }
                .resolve(slot, &self.user_themes);
                if resolved.liquid_accent
                    && let Some((r, g, b)) = crate::platform::system_accent_color()
                {
                    resolved.spec.accent =
                        crate::application::preferences::appearance::Rgb8::new(r, g, b);
                }
                let draft =
                    EditorDraft::new(slot, self.theme_for(slot), &self.user_themes, resolved.spec);
                let initial = draft.initial_focus();
                self.editor = Some(draft);
                super::focus::field(initial)
            }
            AppearanceEdit::Custom(custom_edit) => {
                let Some(mut editor) = self.editor.take() else {
                    return Task::none();
                };
                match editor.apply(custom_edit) {
                    Outcome::Stay(task) => {
                        self.editor = Some(editor);
                        task
                    }
                    Outcome::Cancel(task) => task,
                    Outcome::Commit {
                        slot,
                        themes,
                        chosen,
                        focus,
                    } => {
                        if let Some(id) = chosen {
                            let choice = ThemeChoice::Custom(id);
                            let changed = self.theme_for(slot) != choice;
                            match slot {
                                SystemAppearance::Light => {
                                    self.light_theme = choice;
                                }
                                SystemAppearance::Dark => {
                                    self.dark_theme = choice;
                                }
                            }
                            self.preview_slot = slot;
                            if changed {
                                self.transparency = choice.default_transparency();
                            }
                        }
                        self.light_theme =
                            sanitize_choice(self.light_theme, Preset::LiquidLight, &themes);
                        self.dark_theme =
                            sanitize_choice(self.dark_theme, Preset::LiquidDark, &themes);
                        self.user_themes = themes;
                        focus
                    }
                }
            }
        }
    }

    pub(super) fn validate(&self) -> Option<(AppearancePreferences, Vec<UserTheme>)> {
        UserTheme::validate_collection(&self.user_themes).ok()?;
        for choice in [self.light_theme, self.dark_theme] {
            if let ThemeChoice::Custom(id) = choice
                && !self.user_themes.iter().any(|theme| theme.id == id)
            {
                return None;
            }
        }
        Some((
            AppearancePreferences {
                light_theme: self.light_theme,
                dark_theme: self.dark_theme,
                transparency: Percent::new(self.transparency).ok()?,
                box_transparency: BoxPercent::new(self.box_transparency).ok()?,
                surface_texture: self.surface_texture,
            },
            self.user_themes.clone(),
        ))
    }

    pub(super) fn matches(
        &self,
        preferences: AppearancePreferences,
        user_themes: &[UserTheme],
    ) -> bool {
        self.user_themes == user_themes
            && Percent::new(self.transparency)
                .ok()
                .zip(BoxPercent::new(self.box_transparency).ok())
                .map(|(transparency, box_transparency)| AppearancePreferences {
                    light_theme: self.light_theme,
                    dark_theme: self.dark_theme,
                    transparency,
                    box_transparency,
                    surface_texture: self.surface_texture,
                })
                == Some(preferences)
    }
}

fn sanitize_choice(
    choice: ThemeChoice,
    fallback: Preset,
    user_themes: &[UserTheme],
) -> ThemeChoice {
    match choice {
        ThemeChoice::Custom(id) if !user_themes.iter().any(|item| item.id == id) => {
            ThemeChoice::Preset(fallback)
        }
        valid => valid,
    }
}
