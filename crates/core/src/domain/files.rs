//! Pure filename and operation choices, separate from workspace snapshots.
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateKind {
    Folder,
    Text,
    Markdown,
    Json,
}

impl CreateKind {
    pub const ALL: [Self; 4] = [Self::Folder, Self::Text, Self::Markdown, Self::Json];
    pub fn label(self) -> &'static str {
        match self {
            Self::Folder => "Folder",
            Self::Text => "Text",
            Self::Markdown => "Markdown",
            Self::Json => "JSON",
        }
    }
    pub fn default_name(self) -> &'static str {
        match self {
            Self::Folder => "New folder",
            Self::Text => "Untitled.txt",
            Self::Markdown => "Untitled.md",
            Self::Json => "Untitled.json",
        }
    }
    pub fn filename(self, input: &str) -> Result<String, String> {
        validate_name(input)?;
        let extension = match self {
            Self::Folder => return Ok(input.into()),
            Self::Text => "txt",
            Self::Markdown => "md",
            Self::Json => "json",
        };
        let name = if Path::new(input).extension().is_none() {
            format!("{input}.{extension}")
        } else {
            input.into()
        };
        validate_name(&name)?;
        Ok(name)
    }
    pub fn contents(self) -> &'static [u8] {
        if self == Self::Json { b"{}\n" } else { b"" }
    }
}

pub fn validate_name(name: &str) -> Result<(), String> {
    crate::domain::validation::validate_entry_name(name).map_err(str::to_owned)
}

pub fn source_name(path: &Path) -> Result<String, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Cannot copy this native filename exactly. Use Finder for this entry.")?;
    validate_name(name).map_err(|_| "Cannot copy this filename within the supported name limits. Use Finder for this entry.")?;
    Ok(name.into())
}

pub fn keep_both_name(name: &str) -> String {
    let path = Path::new(name);
    let stem = path.file_stem().and_then(|v| v.to_str()).unwrap_or(name);
    let extension = path.extension().and_then(|v| v.to_str());
    let (stem, number) = stem
        .rsplit_once(" (")
        .and_then(|(prefix, suffix)| {
            suffix
                .strip_suffix(')')
                .and_then(|v| v.parse::<u32>().ok())
                .and_then(|n| n.checked_add(1))
                .map(|n| (prefix, n))
        })
        .unwrap_or((stem, 2));
    match extension {
        Some(ext) => format!("{stem} ({number}).{ext}"),
        None => format!("{stem} ({number})"),
    }
}
