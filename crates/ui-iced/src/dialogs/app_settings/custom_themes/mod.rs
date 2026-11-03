//! Draft state, validation, and HSV color editing for custom themes.

pub(super) mod view;

use iced::Task;

use crate::application::preferences::appearance::{
    CustomThemeId, Rgb8, SystemAppearance, ThemeChoice, ThemeSpec, UserTheme,
};

use super::{appearance, focus};
use crate::Message;

pub(super) const NAME_ID: &str = "custom-theme-name";
pub(super) const NEW_ID: &str = "custom-themes-new";
pub(super) const CANCEL_ID: &str = "custom-themes-cancel";
pub(super) const DONE_ID: &str = "custom-themes-done";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColorSlot {
    Window,
    Boxes,
    Configure,
    Text,
    Muted,
    Border,
    Accent,
    Error,
}

impl ColorSlot {
    pub(super) const ALL: [Self; 8] = [
        Self::Window,
        Self::Boxes,
        Self::Configure,
        Self::Text,
        Self::Muted,
        Self::Border,
        Self::Accent,
        Self::Error,
    ];

    pub(super) const fn index(self) -> usize {
        self as usize
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Window => "Window",
            Self::Boxes => "Boxes",
            Self::Configure => "Configure",
            Self::Text => "Main text",
            Self::Muted => "Muted text",
            Self::Border => "Border",
            Self::Accent => "Accent",
            Self::Error => "Error",
        }
    }

    pub(super) const fn input_id(self) -> &'static str {
        match self {
            Self::Window => "custom-theme-hex-window",
            Self::Boxes => "custom-theme-hex-boxes",
            Self::Configure => "custom-theme-hex-configure",
            Self::Text => "custom-theme-hex-text",
            Self::Muted => "custom-theme-hex-muted",
            Self::Border => "custom-theme-hex-border",
            Self::Accent => "custom-theme-hex-accent",
            Self::Error => "custom-theme-hex-error",
        }
    }

    pub(super) const fn swatch_id(self) -> &'static str {
        match self {
            Self::Window => "custom-theme-swatch-window",
            Self::Boxes => "custom-theme-swatch-boxes",
            Self::Configure => "custom-theme-swatch-configure",
            Self::Text => "custom-theme-swatch-text",
            Self::Muted => "custom-theme-swatch-muted",
            Self::Border => "custom-theme-swatch-border",
            Self::Accent => "custom-theme-swatch-accent",
            Self::Error => "custom-theme-swatch-error",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Hsv {
    pub(super) hue: u16,
    pub(super) saturation: u8,
    pub(super) brightness: u8,
}

impl Hsv {
    pub(super) fn from_rgb(color: Rgb8) -> Self {
        let r = f32::from(color.red) / 255.0;
        let g = f32::from(color.green) / 255.0;
        let b = f32::from(color.blue) / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        let hue = if delta <= f32::EPSILON {
            0.0
        } else if (max - r).abs() <= f32::EPSILON {
            60.0 * (((g - b) / delta).rem_euclid(6.0))
        } else if (max - g).abs() <= f32::EPSILON {
            60.0 * (((b - r) / delta) + 2.0)
        } else {
            60.0 * (((r - g) / delta) + 4.0)
        };
        let saturation = if max <= f32::EPSILON {
            0.0
        } else {
            (delta / max) * 100.0
        };
        Self {
            hue: (hue.round().clamp(0.0, 360.0) as u16) % 360,
            saturation: saturation.round().clamp(0.0, 100.0) as u8,
            brightness: (max * 100.0).round().clamp(0.0, 100.0) as u8,
        }
    }

    pub(super) fn to_rgb(self) -> Rgb8 {
        let h = f32::from(self.hue % 360);
        let s = f32::from(self.saturation.min(100)) / 100.0;
        let v = f32::from(self.brightness.min(100)) / 100.0;
        let c = v * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = v - c;
        let (r, g, b) = match (h / 60.0).floor() as u8 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let byte = |channel: f32| ((channel + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Rgb8::new(byte(r), byte(g), byte(b))
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Edit {
    Select(usize),
    Create,
    Duplicate,
    Delete,
    Name(String),
    Hex(ColorSlot, String),
    TogglePicker(ColorSlot),
    Hsv(ColorSlot, Hsv),
    Cancel,
    Done,
}

#[derive(Clone)]
pub(super) struct ColorField {
    pub(super) color: Rgb8,
    pub(super) hex: String,
}

impl ColorField {
    fn new(color: Rgb8) -> Self {
        Self {
            color,
            hex: color.to_hex(),
        }
    }

    pub(super) fn is_valid(&self) -> bool {
        Rgb8::parse_hex(&self.hex).is_some()
    }
}

#[derive(Clone)]
pub(super) struct EntryDraft {
    pub(super) id: CustomThemeId,
    pub(super) name: String,
    pub(super) colors: [ColorField; 8],
    pub(super) name_error: Option<&'static str>,
    pub(super) color_error: Option<&'static str>,
}

impl EntryDraft {
    fn from_theme(theme: &UserTheme) -> Self {
        Self::from_spec(theme.id, theme.name.clone(), theme.spec)
    }

    fn from_spec(id: CustomThemeId, name: String, spec: ThemeSpec) -> Self {
        Self {
            id,
            name,
            colors: [
                ColorField::new(spec.window),
                ColorField::new(spec.boxes),
                ColorField::new(spec.configure),
                ColorField::new(spec.text),
                ColorField::new(spec.muted),
                ColorField::new(spec.border),
                ColorField::new(spec.accent),
                ColorField::new(spec.error),
            ],
            name_error: None,
            color_error: None,
        }
    }

    pub(super) fn spec(&self) -> ThemeSpec {
        ThemeSpec {
            window: self.colors[0].color,
            boxes: self.colors[1].color,
            configure: self.colors[2].color,
            text: self.colors[3].color,
            muted: self.colors[4].color,
            border: self.colors[5].color,
            accent: self.colors[6].color,
            error: self.colors[7].color,
        }
    }
}

pub(super) struct EditorDraft {
    slot: SystemAppearance,
    pub(super) entries: Vec<EntryDraft>,
    pub(super) selected: Option<usize>,
    pub(super) active_picker: Option<(ColorSlot, Hsv)>,
    seed_name: String,
    seed_spec: ThemeSpec,
}

impl EditorDraft {
    pub(super) fn new(
        slot: SystemAppearance,
        active: ThemeChoice,
        themes: &[UserTheme],
        active_spec: ThemeSpec,
    ) -> Self {
        let entries: Vec<EntryDraft> = themes.iter().map(EntryDraft::from_theme).collect();
        let selected = match active {
            ThemeChoice::Custom(id) => entries
                .iter()
                .position(|entry| entry.id == id)
                .or((!entries.is_empty()).then_some(0)),
            ThemeChoice::Preset(_) => (!entries.is_empty()).then_some(0),
        };
        let seed_name = match active {
            ThemeChoice::Preset(preset) => format!("{} Custom", preset.label()),
            ThemeChoice::Custom(id) => themes
                .iter()
                .find(|theme| theme.id == id)
                .map_or_else(|| "Custom Theme".into(), |t| format!("{} Copy", t.name)),
        };
        Self {
            slot,
            entries,
            selected,
            active_picker: None,
            seed_name,
            seed_spec: active_spec,
        }
    }

    pub(super) fn initial_focus(&self) -> &'static str {
        if self.selected.is_some() {
            NAME_ID
        } else {
            NEW_ID
        }
    }

    pub(super) fn preview_spec(&self) -> Option<ThemeSpec> {
        self.selected
            .and_then(|index| self.entries.get(index))
            .map(EntryDraft::spec)
    }

    pub(super) fn apply(&mut self, edit: Edit) -> Outcome {
        match edit {
            Edit::Select(index) => {
                if index < self.entries.len() {
                    self.selected = Some(index);
                    self.active_picker = None;
                }
                Outcome::Stay(Task::none())
            }
            Edit::Create => self.push_entry(&self.seed_name.clone(), self.seed_spec),
            Edit::Duplicate => {
                let Some(current) = self.selected.and_then(|i| self.entries.get(i)) else {
                    return Outcome::Stay(Task::none());
                };
                let base = format!("{} Copy", current.name.trim_end_matches(' '));
                let spec = current.spec();
                self.push_entry(&base, spec)
            }
            Edit::Delete => {
                if let Some(index) = self.selected
                    && index < self.entries.len()
                {
                    self.entries.remove(index);
                    self.active_picker = None;
                    self.selected = if self.entries.is_empty() {
                        None
                    } else {
                        Some(index.min(self.entries.len() - 1))
                    };
                }
                Outcome::Stay(focus::field(self.initial_focus()))
            }
            Edit::Name(name) => {
                if let Some(entry) = self.current_mut() {
                    entry.name = crate::domain::validation::sanitize_config_name(&name);
                    entry.name_error = None;
                }
                Outcome::Stay(Task::none())
            }
            Edit::Hex(slot, value) => {
                let parsed = Rgb8::parse_hex(&value);
                if let Some(entry) = self.current_mut() {
                    let field = &mut entry.colors[slot.index()];
                    field.hex = value;
                    if let Some(color) = parsed {
                        field.color = color;
                        entry.color_error = None;
                    }
                }
                if let (Some(color), Some((open_slot, hsv))) = (parsed, &mut self.active_picker)
                    && *open_slot == slot
                {
                    *hsv = Hsv::from_rgb(color);
                }
                Outcome::Stay(Task::none())
            }
            Edit::TogglePicker(slot) => {
                if self.active_picker.is_some_and(|(open, _)| open == slot) {
                    self.active_picker = None;
                } else if let Some(entry) = self.selected.and_then(|i| self.entries.get(i)) {
                    let color = entry.colors[slot.index()].color;
                    self.active_picker = Some((slot, Hsv::from_rgb(color)));
                }
                Outcome::Stay(Task::none())
            }
            Edit::Hsv(slot, hsv) => {
                let color = hsv.to_rgb();
                self.active_picker = Some((slot, hsv));
                if let Some(entry) = self.current_mut() {
                    let field = &mut entry.colors[slot.index()];
                    field.color = color;
                    field.hex = color.to_hex();
                    entry.color_error = None;
                }
                Outcome::Stay(Task::none())
            }
            Edit::Cancel => Outcome::Cancel(focus::field(appearance::custom_themes_id(self.slot))),
            Edit::Done => self.finish(),
        }
    }

    fn current_mut(&mut self) -> Option<&mut EntryDraft> {
        self.selected
            .and_then(move |index| self.entries.get_mut(index))
    }

    fn push_entry(&mut self, base: &str, spec: ThemeSpec) -> Outcome {
        if self.entries.len() >= UserTheme::MAX_THEMES {
            return Outcome::Stay(Task::none());
        }
        let name = unique_name(base, &self.entries);
        self.entries
            .push(EntryDraft::from_spec(CustomThemeId::random(), name, spec));
        self.selected = Some(self.entries.len() - 1);
        self.active_picker = None;
        Outcome::Stay(focus::field(NAME_ID))
    }

    fn finish(&mut self) -> Outcome {
        for index in 0..self.entries.len() {
            self.entries[index].name =
                crate::domain::validation::finalize_config_name(&self.entries[index].name);
            self.entries[index].name_error = None;
            self.entries[index].color_error = None;
            if let Some(err) = UserTheme::name_error(&self.entries[index].name) {
                self.selected = Some(index);
                self.entries[index].name_error = Some(err);
                return Outcome::Stay(focus::field(NAME_ID));
            }
            let duplicate = self.entries[..index]
                .iter()
                .any(|earlier| earlier.name.eq_ignore_ascii_case(&self.entries[index].name));
            if duplicate {
                self.selected = Some(index);
                self.entries[index].name_error = Some("Enter a unique theme name.");
                return Outcome::Stay(focus::field(NAME_ID));
            }
            for slot in ColorSlot::ALL {
                if !self.entries[index].colors[slot.index()].is_valid() {
                    self.selected = Some(index);
                    self.entries[index].color_error = Some("Enter 6 hex digits (#RRGGBB).");
                    return Outcome::Stay(focus::field(slot.input_id()));
                }
            }
        }
        let themes: Vec<UserTheme> = self
            .entries
            .iter()
            .map(|entry| UserTheme {
                id: entry.id,
                name: entry.name.clone(),
                spec: entry.spec(),
            })
            .collect();
        let chosen = self
            .selected
            .and_then(|index| themes.get(index))
            .map(|theme| theme.id);
        Outcome::Commit {
            slot: self.slot,
            themes,
            chosen,
            focus: focus::field(appearance::theme_id(self.slot)),
        }
    }
}

pub(super) enum Outcome {
    Stay(Task<Message>),
    Cancel(Task<Message>),
    Commit {
        slot: SystemAppearance,
        themes: Vec<UserTheme>,
        chosen: Option<CustomThemeId>,
        focus: Task<Message>,
    },
}

fn unique_name(base: &str, entries: &[EntryDraft]) -> String {
    let finalized = crate::domain::validation::finalize_config_name(base);
    let stem = if finalized.is_empty() {
        "Custom Theme"
    } else {
        finalized.as_str()
    };
    let taken = |candidate: &str| {
        UserTheme::name_error(candidate).is_some()
            || entries
                .iter()
                .any(|e| e.name.trim_end_matches(' ').eq_ignore_ascii_case(candidate))
    };
    if !taken(stem) {
        return stem.to_owned();
    }
    let prefix_raw: String = stem
        .chars()
        .take(UserTheme::MAX_NAME_CHARS.saturating_sub(4))
        .collect();
    let prefix = prefix_raw.trim_end_matches(' ');
    let prefix = if prefix.is_empty() { "Custom" } else { prefix };
    for number in 2..=999 {
        let candidate = format!("{prefix} {number}");
        if !taken(&candidate) {
            return candidate;
        }
    }
    format!("Custom {}", entries.len() + 1)
}
