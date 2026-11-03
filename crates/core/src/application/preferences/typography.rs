//! App-wide text preferences, independent of fonts, widgets, and storage adapters.
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

struct CatalogEntry {
    name: Box<str>,
    weights: &'static [TextWeight],
}

static CATALOG: OnceLock<Box<[CatalogEntry]>> = OnceLock::new();

#[derive(Debug, Clone, Copy)]
pub struct CustomFont {
    name: &'static str,
    weights: &'static [TextWeight],
}

impl PartialEq for CustomFont {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for CustomFont {}

impl PartialOrd for CustomFont {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CustomFont {
    fn cmp(&self, other: &Self) -> Ordering {
        self.name.cmp(other.name)
    }
}

impl Hash for CustomFont {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl CustomFont {
    pub const PREFIX: &'static str = "custom:";
    pub const MAX_LEN: usize = 128;

    pub fn is_valid_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= Self::MAX_LEN
            && name.trim() == name
            && !name.starts_with('.')
            && !name.chars().any(char::is_control)
    }

    pub fn register_catalog(mut entries: Vec<(String, &'static [TextWeight])>) {
        entries.retain(|(name, weights)| Self::is_valid_name(name) && !weights.is_empty());
        entries.sort_by(|left, right| left.0.cmp(&right.0));
        entries.dedup_by(|left, right| left.0 == right.0);
        let boxed: Box<[CatalogEntry]> = entries
            .into_iter()
            .map(|(name, weights)| CatalogEntry {
                name: name.into_boxed_str(),
                weights,
            })
            .collect();
        let _ = CATALOG.set(boxed);
    }

    pub fn parse(raw: &str) -> Option<Self> {
        if !Self::is_valid_name(raw) {
            return None;
        }
        let catalog = CATALOG.get()?;
        let index = catalog
            .binary_search_by(|entry| entry.name.as_ref().cmp(raw))
            .ok()?;
        let entry = &catalog[index];
        Some(Self {
            name: &entry.name,
            weights: entry.weights,
        })
    }

    pub const fn name(self) -> &'static str {
        self.name
    }

    pub const fn weights(self) -> &'static [TextWeight] {
        self.weights
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Typeface {
    #[default]
    SystemDefault,
    AvenirNext,
    MonaspaceArgon,
    Cantarell,
    Custom(CustomFont),
}

impl Typeface {
    pub const STANDARD: [Self; 4] = [
        Self::SystemDefault,
        Self::AvenirNext,
        Self::MonaspaceArgon,
        Self::Cantarell,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::SystemDefault => "System Default",
            Self::AvenirNext => "Avenir Next",
            Self::MonaspaceArgon => "Monaspace Argon",
            Self::Cantarell => "Cantarell",
            Self::Custom(font) => font.name(),
        }
    }

    pub fn encode(self) -> String {
        match self {
            Self::SystemDefault => "system_default".into(),
            Self::AvenirNext => "avenir_next".into(),
            Self::MonaspaceArgon => "monaspace_argon".into(),
            Self::Cantarell => "cantarell".into(),
            Self::Custom(font) => format!("{}{}", CustomFont::PREFIX, font.name()),
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        if let Some(name) = key.strip_prefix(CustomFont::PREFIX) {
            return CustomFont::parse(name).map(Self::Custom);
        }
        match key {
            "system_default" => Some(Self::SystemDefault),
            "avenir_next" => Some(Self::AvenirNext),
            "monaspace_argon" => Some(Self::MonaspaceArgon),
            "cantarell" => Some(Self::Cantarell),
            _ => None,
        }
    }

    pub const fn weights(self) -> &'static [TextWeight] {
        match self {
            Self::Cantarell => &[TextWeight::Regular, TextWeight::Bold],
            Self::MonaspaceArgon => &[TextWeight::Regular, TextWeight::Medium],
            Self::SystemDefault | Self::AvenirNext => {
                &[TextWeight::Regular, TextWeight::Medium, TextWeight::Bold]
            }
            Self::Custom(font) => font.weights(),
        }
    }
}

impl std::fmt::Display for Typeface {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextWeight {
    Regular,
    #[default]
    Medium,
    Bold,
}

impl TextWeight {
    pub const ALL: [Self; 3] = [Self::Regular, Self::Medium, Self::Bold];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Medium => "medium",
            Self::Bold => "bold",
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|weight| weight.key() == key)
    }

    pub const fn slice(regular: bool, medium: bool, bold: bool) -> Option<&'static [Self]> {
        match (regular, medium, bold) {
            (false, false, false) => None,
            (true, false, false) => Some(&[Self::Regular]),
            (false, true, false) => Some(&[Self::Medium]),
            (false, false, true) => Some(&[Self::Bold]),
            (true, true, false) => Some(&[Self::Regular, Self::Medium]),
            (true, false, true) => Some(&[Self::Regular, Self::Bold]),
            (false, true, true) => Some(&[Self::Medium, Self::Bold]),
            (true, true, true) => Some(&[Self::Regular, Self::Medium, Self::Bold]),
        }
    }
}

impl std::fmt::Display for TextWeight {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Regular => "Regular",
            Self::Medium => "Medium",
            Self::Bold => "Bold",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSize(u8);

impl TextSize {
    pub const MIN: u8 = 12;
    pub const MAX: u8 = 16;
    pub fn new(value: u8) -> Option<Self> {
        (Self::MIN..=Self::MAX)
            .contains(&value)
            .then_some(Self(value))
    }
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl Default for TextSize {
    fn default() -> Self {
        Self(14)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TypographyPreferences {
    pub typeface: Typeface,
    pub size: TextSize,
    pub weight: TextWeight,
}

impl TypographyPreferences {
    pub fn is_valid(self) -> bool {
        self.typeface.weights().contains(&self.weight)
    }

    pub fn with_typeface(self, typeface: Typeface) -> Self {
        let weights = typeface.weights();
        let weight = if weights.contains(&self.weight) {
            self.weight
        } else if self.weight == TextWeight::Regular {
            weights.first().copied().unwrap_or(TextWeight::Regular)
        } else {
            weights.last().copied().unwrap_or(TextWeight::Regular)
        };
        Self {
            typeface,
            weight,
            ..self
        }
    }
}

/// Resolved at launch, never persisted as a machine-specific font choice.
pub struct SystemFontInfo {
    pub name: String,
    pub family: String,
}
