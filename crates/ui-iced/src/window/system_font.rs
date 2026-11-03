//! Resolve renderer resources once at launch. Saved preferences remain application-owned.
use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

#[cfg(target_os = "macos")]
use crate::application::preferences::typography::SystemFontInfo;
use crate::application::preferences::typography::{
    CustomFont, TextWeight, Typeface, TypographyPreferences,
};
use iced::advanced::graphics::text::{
    cosmic_text::{
        fontdb,
        skrifa::{self, GlyphId, MetadataProvider, Tag},
    },
    font_system,
};
use iced::{Font, font};

const REQUIRED_ASCII: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const VARIABLE_STOPS: [u16; 4] = [400, 500, 600, 700];

const EXCLUDED_FAMILIES: &[&str] = &[
    "Apple Braille",
    "Apple Color Emoji",
    "Apple Symbols",
    "Avenir Next",
    "Bodoni Ornaments",
    "Bookshelf Symbol 7",
    "Cantarell",
    "Hoefler Text Ornaments",
    "Iced-Icons",
    "LastResort",
    "MS Reference Specialty",
    "MT Extra",
    "Marlett",
    "Monaspace Argon",
    "Symbol",
    "System Font",
    "Webdings",
    "Wingdings",
    "Wingdings 2",
    "Wingdings 3",
    "Zapf Dingbats",
    "iced-icons",
];

#[derive(Clone, Copy)]
struct FaceCut {
    raw: u16,
    weight: font::Weight,
    stretch: font::Stretch,
    style: font::Style,
}

#[derive(Clone, Copy)]
struct ResolvedPair {
    body: FaceCut,
    heading: FaceCut,
}

struct ResolvedEntry {
    font: CustomFont,
    regular: Option<ResolvedPair>,
    medium: Option<ResolvedPair>,
    bold: Option<ResolvedPair>,
}

#[derive(Clone, Copy)]
struct FaceInspection {
    scalable_outline: bool,
    ascii_complete: bool,
    variable_wght: Option<(f32, f32)>,
}

static RESOLVED: OnceLock<Box<[ResolvedEntry]>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstalledFont {
    pub font: CustomFont,
}

#[derive(Debug, Clone)]
pub(crate) struct SystemFont {
    pub name: Option<String>,
    pub error: Option<String>,
    pub installed: Vec<InstalledFont>,
}

impl Default for SystemFont {
    fn default() -> Self {
        Self {
            name: None,
            error: Some("System font lookup is unavailable. System Default uses the renderer's sans-serif fallback; choose an explicit font if needed.".into()),
            installed: Vec::new(),
        }
    }
}

impl SystemFont {
    pub fn load() -> Self {
        let installed = discover_installed();
        #[cfg(target_os = "macos")]
        {
            match crate::platform::system_font() {
                Ok((name_str, family)) => {
                    let info = SystemFontInfo {
                        name: name_str.clone(),
                        family,
                    };
                    let name = Some(name_str);
                    let error = register(info).err().map(|error| format!("Cannot use the macOS system font: {error} System Default uses the renderer's sans-serif fallback. Choose an explicit font if needed."));
                    Self {
                        name,
                        error,
                        installed,
                    }
                }
                Err(error) => Self {
                    name: None,
                    error: Some(format!(
                        "Cannot read the macOS system font: {error} System Default uses the renderer's sans-serif fallback. Choose an explicit font if needed."
                    )),
                    installed,
                },
            }
        }
        #[cfg(not(target_os = "macos"))]
        Self {
            installed,
            ..Self::default()
        }
    }

    pub fn label(&self) -> String {
        match &self.name {
            Some(name) => format!("{name} (System Default)"),
            None => "System Default (unavailable)".into(),
        }
    }

    pub fn weights_for(&self, typeface: Typeface) -> Vec<TextWeight> {
        typeface.weights().to_vec()
    }

    pub fn select_typeface(
        &self,
        preferences: TypographyPreferences,
        typeface: Typeface,
    ) -> TypographyPreferences {
        preferences.with_typeface(typeface)
    }
}

pub(super) fn resolve_custom(custom: CustomFont, weight: TextWeight) -> (Font, Font) {
    if let Some(entries) = RESOLVED.get()
        && let Ok(index) = entries.binary_search_by(|entry| entry.font.cmp(&custom))
    {
        let entry = &entries[index];
        let pair = match weight {
            TextWeight::Regular => entry.regular.or(entry.medium).or(entry.bold),
            TextWeight::Medium => entry.medium.or(entry.bold).or(entry.regular),
            TextWeight::Bold => entry.bold.or(entry.medium).or(entry.regular),
        };
        if let Some(pair) = pair {
            return (
                cut_to_font(custom.name(), pair.body),
                cut_to_font(custom.name(), pair.heading),
            );
        }
    }
    let base = Font::with_name(custom.name());
    let body = Font {
        weight: match weight {
            TextWeight::Regular => font::Weight::Normal,
            TextWeight::Medium => font::Weight::Medium,
            TextWeight::Bold => font::Weight::Bold,
        },
        ..base
    };
    let heading = Font {
        weight: match weight {
            TextWeight::Regular => font::Weight::Semibold,
            TextWeight::Medium | TextWeight::Bold => font::Weight::Bold,
        },
        ..base
    };
    (body, heading)
}

fn cut_to_font(name: &'static str, cut: FaceCut) -> Font {
    Font {
        family: font::Family::Name(name),
        weight: cut.weight,
        stretch: cut.stretch,
        style: cut.style,
    }
}

fn is_selectable_system_family(name: &str) -> bool {
    CustomFont::is_valid_name(name)
        && !EXCLUDED_FAMILIES
            .iter()
            .any(|excluded| excluded.eq_ignore_ascii_case(name))
}

fn inspect_face(data: &[u8], index: u32) -> Option<FaceInspection> {
    let font_ref = skrifa::FontRef::from_index(data, index).ok()?;
    let scalable_outline = font_ref.table_data(Tag::new(b"glyf")).is_some()
        || font_ref.table_data(Tag::new(b"CFF ")).is_some()
        || font_ref.table_data(Tag::new(b"CFF2")).is_some();
    let charmap = font_ref.charmap();
    let ascii_complete = REQUIRED_ASCII.chars().all(|ch| {
        charmap
            .map(ch)
            .is_some_and(|glyph_id| glyph_id != GlyphId::NOTDEF)
    });
    let variable_wght = font_ref
        .axes()
        .iter()
        .find(|axis| axis.tag() == Tag::new(b"wght"))
        .map(|axis| (axis.min_value(), axis.max_value()));
    Some(FaceInspection {
        scalable_outline,
        ascii_complete,
        variable_wght,
    })
}

fn discover_installed() -> Vec<InstalledFont> {
    let entries = RESOLVED.get_or_init(init_catalog);
    let mut installed: Vec<InstalledFont> = entries
        .iter()
        .map(|entry| InstalledFont { font: entry.font })
        .collect();
    installed.sort_by(|left, right| {
        left.font
            .name()
            .to_lowercase()
            .cmp(&right.font.name().to_lowercase())
            .then_with(|| left.font.name().cmp(right.font.name()))
    });
    installed
}

fn init_catalog() -> Box<[ResolvedEntry]> {
    let Some(families) = scan_and_expand_database() else {
        return Box::new([]);
    };
    let mut raw_resolved = Vec::with_capacity(families.len());
    let mut catalog = Vec::with_capacity(families.len());
    for (family, mut cuts) in families {
        cuts.sort_by_key(|cut| (cut.raw, style_rank(cut.style), stretch_rank(cut.stretch)));
        cuts.dedup_by_key(|cut| cut.raw);
        let Some((weights, regular, medium, bold)) = resolve_family_cuts(&cuts) else {
            continue;
        };
        catalog.push((family.clone(), weights));
        raw_resolved.push((family, regular, medium, bold));
    }
    CustomFont::register_catalog(catalog);
    let mut entries: Vec<ResolvedEntry> = raw_resolved
        .into_iter()
        .filter_map(|(family, regular, medium, bold)| {
            Some(ResolvedEntry {
                font: CustomFont::parse(&family)?,
                regular,
                medium,
                bold,
            })
        })
        .collect();
    entries.sort_by_key(|entry| entry.font);
    entries.into_boxed_slice()
}

fn scan_and_expand_database() -> Option<BTreeMap<String, Vec<FaceCut>>> {
    let mut fonts = font_system().write().ok()?;
    let db = fonts.raw().db();
    let mut existing: HashSet<(String, fontdb::Style, fontdb::Stretch, u16)> = db
        .faces()
        .filter_map(|face| {
            let (family, _) = face.families.first()?;
            Some((family.clone(), face.style, face.stretch, face.weight.0))
        })
        .collect();
    let mut synthetic_faces = Vec::new();
    let mut families: BTreeMap<String, Vec<FaceCut>> = BTreeMap::new();
    for face in db.faces() {
        let Some((family, _)) = face.families.first() else {
            continue;
        };
        let Some(inspection) = db.with_face_data(face.id, inspect_face).flatten() else {
            continue;
        };
        if !inspection.scalable_outline || !inspection.ascii_complete {
            continue;
        }
        let stretch = iced_stretch(face.stretch);
        let style = iced_style(face.style);
        let selectable = is_selectable_system_family(family);
        if selectable && let Some(weight) = iced_weight(face.weight.0) {
            families.entry(family.clone()).or_default().push(FaceCut {
                raw: face.weight.0,
                weight,
                stretch,
                style,
            });
        }
        if let Some((min_w, max_w)) = inspection.variable_wght {
            for stop in VARIABLE_STOPS {
                if !(min_w..=max_w).contains(&f32::from(stop)) {
                    continue;
                }
                if selectable && let Some(weight) = iced_weight(stop) {
                    families.entry(family.clone()).or_default().push(FaceCut {
                        raw: stop,
                        weight,
                        stretch,
                        style,
                    });
                }
                if existing.insert((family.clone(), face.style, face.stretch, stop)) {
                    let mut synthetic = face.clone();
                    synthetic.id = fontdb::ID::dummy();
                    synthetic.weight = fontdb::Weight(stop);
                    synthetic_faces.push(synthetic);
                }
            }
        }
    }
    let db_mut = fonts.raw().db_mut();
    for synthetic in synthetic_faces {
        db_mut.push_face_info(synthetic);
    }
    Some(families)
}

type FamilyResolution = (
    &'static [TextWeight],
    Option<ResolvedPair>,
    Option<ResolvedPair>,
    Option<ResolvedPair>,
);

fn resolve_family_cuts(cuts: &[FaceCut]) -> Option<FamilyResolution> {
    let regular_body = cuts
        .iter()
        .copied()
        .filter(|cut| cut.raw < 450)
        .min_by_key(|cut| (cut.raw.abs_diff(400), std::cmp::Reverse(cut.raw)));
    let medium_body = cuts
        .iter()
        .copied()
        .find(|cut| (450..600).contains(&cut.raw));
    let bold_body = cuts
        .iter()
        .copied()
        .filter(|cut| cut.raw >= 600)
        .min_by_key(|cut| (cut.raw.abs_diff(700), cut.raw));
    let weights = TextWeight::slice(
        regular_body.is_some(),
        medium_body.is_some(),
        bold_body.is_some(),
    )?;
    let pair = |slot: TextWeight, body: FaceCut| ResolvedPair {
        body,
        heading: heading_cut(cuts, slot, body),
    };
    Some((
        weights,
        regular_body.map(|body| pair(TextWeight::Regular, body)),
        medium_body.map(|body| pair(TextWeight::Medium, body)),
        bold_body.map(|body| pair(TextWeight::Bold, body)),
    ))
}

fn heading_cut(cuts: &[FaceCut], slot: TextWeight, body: FaceCut) -> FaceCut {
    match slot {
        TextWeight::Regular => cuts
            .iter()
            .copied()
            .filter(|cut| cut.raw > body.raw)
            .min_by_key(|cut| (cut.raw > 700, cut.raw.abs_diff(600), cut.raw))
            .unwrap_or(body),
        TextWeight::Medium => cuts
            .iter()
            .copied()
            .filter(|cut| cut.raw > body.raw)
            .min_by_key(|cut| (cut.raw.abs_diff(700), cut.raw))
            .unwrap_or(body),
        TextWeight::Bold => body,
    }
}

const fn iced_weight(raw: u16) -> Option<font::Weight> {
    match raw {
        100 => Some(font::Weight::Thin),
        200 => Some(font::Weight::ExtraLight),
        300 => Some(font::Weight::Light),
        400 => Some(font::Weight::Normal),
        500 => Some(font::Weight::Medium),
        600 => Some(font::Weight::Semibold),
        700 => Some(font::Weight::Bold),
        800 => Some(font::Weight::ExtraBold),
        900 => Some(font::Weight::Black),
        _ => None,
    }
}

const fn iced_stretch(stretch: fontdb::Stretch) -> font::Stretch {
    match stretch {
        fontdb::Stretch::UltraCondensed => font::Stretch::UltraCondensed,
        fontdb::Stretch::ExtraCondensed => font::Stretch::ExtraCondensed,
        fontdb::Stretch::Condensed => font::Stretch::Condensed,
        fontdb::Stretch::SemiCondensed => font::Stretch::SemiCondensed,
        fontdb::Stretch::Normal => font::Stretch::Normal,
        fontdb::Stretch::SemiExpanded => font::Stretch::SemiExpanded,
        fontdb::Stretch::Expanded => font::Stretch::Expanded,
        fontdb::Stretch::ExtraExpanded => font::Stretch::ExtraExpanded,
        fontdb::Stretch::UltraExpanded => font::Stretch::UltraExpanded,
    }
}

const fn iced_style(style: fontdb::Style) -> font::Style {
    match style {
        fontdb::Style::Normal => font::Style::Normal,
        fontdb::Style::Italic => font::Style::Italic,
        fontdb::Style::Oblique => font::Style::Oblique,
    }
}

const fn style_rank(style: font::Style) -> u8 {
    match style {
        font::Style::Normal => 0,
        font::Style::Oblique => 1,
        font::Style::Italic => 2,
    }
}

const fn stretch_rank(stretch: font::Stretch) -> u8 {
    match stretch {
        font::Stretch::Normal => 0,
        font::Stretch::SemiCondensed | font::Stretch::SemiExpanded => 1,
        font::Stretch::Condensed | font::Stretch::Expanded => 2,
        font::Stretch::ExtraCondensed | font::Stretch::ExtraExpanded => 3,
        font::Stretch::UltraCondensed | font::Stretch::UltraExpanded => 4,
    }
}

#[cfg(target_os = "macos")]
fn register(info: SystemFontInfo) -> Result<(), &'static str> {
    let mut fonts = font_system()
        .write()
        .map_err(|_| "the renderer font database is unavailable.")?;
    // AppKit reports an opaque UI alias on current macOS. The same system face
    // is exposed to file-based renderers under its SFNS family, "System Font".
    // Verify it exists rather than silently treating a missing family as resolved.
    let family = if info.family == ".AppleSystemUIFont" {
        "System Font"
    } else {
        &info.family
    };
    let query = fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        ..Default::default()
    };
    if fonts.raw().db().query(&query).is_none() {
        return Err("the OS family is not available to the renderer.");
    }
    fonts.raw().db_mut().set_sans_serif_family(family);
    Ok(())
}
