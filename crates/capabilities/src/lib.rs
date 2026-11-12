//! Modular, read-only folder capability detectors.
//!
//! Each capability lives in its own isolated module, inspects in-memory directory
//! snapshots and small marker files without spawning subprocesses, and emits
//! compact tokens for the folder box footer along with hover tooltip details.

pub mod cpp;
pub mod dart;
pub mod dotnet;
pub mod elixir;
pub mod env;
pub mod folder;
pub mod git;
pub mod go;
pub mod jvm;
pub mod node;
pub mod php;
pub mod python;
pub mod ruby;
pub mod rust;
pub mod scan;
pub mod swift;
pub mod terraform;
pub mod toolchains;
pub mod zig;

use std::path::{Path, PathBuf};

pub use folder::MountOrigin;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CapabilityKind {
    Git,
    Languages,
    EnvFiles,
}

impl CapabilityKind {
    pub const ALL: [Self; 3] = [Self::Git, Self::Languages, Self::EnvFiles];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Git => "Git",
            Self::Languages => "Languages and toolchains",
            Self::EnvFiles => "Environment and secrets",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Git => "Branch, commit, worktree, submodule, and stash count.",
            Self::Languages => "Runtimes, versions, and package managers.",
            Self::EnvFiles => ".env, .envrc, auth dotfiles, and key files.",
        }
    }

    pub const fn tooltip(self) -> Option<&'static str> {
        match self {
            Self::Git | Self::EnvFiles => None,
            Self::Languages => Some(
                "Supported: Python, Node.js, Deno, Rust, Go, Ruby, Java/JVM, C/C++, Swift, .NET, PHP, Zig, Elixir, Dart/Flutter, Terraform/OpenTofu, Nix, mise, and asdf.",
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityPreferences {
    pub git: bool,
    pub languages: bool,
    pub env_files: bool,
}

impl Default for CapabilityPreferences {
    fn default() -> Self {
        Self {
            git: true,
            languages: true,
            env_files: true,
        }
    }
}

impl CapabilityPreferences {
    pub const fn enabled(self, kind: CapabilityKind) -> bool {
        match kind {
            CapabilityKind::Git => self.git,
            CapabilityKind::Languages => self.languages,
            CapabilityKind::EnvFiles => self.env_files,
        }
    }

    pub const fn with_enabled(mut self, kind: CapabilityKind, enabled: bool) -> Self {
        match kind {
            CapabilityKind::Git => self.git = enabled,
            CapabilityKind::Languages => self.languages = enabled,
            CapabilityKind::EnvFiles => self.env_files = enabled,
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityItem {
    pub kind: Option<CapabilityKind>,
    pub prefix: String,
    pub value: Option<String>,
    pub detail: String,
}

impl CapabilityItem {
    pub fn new(
        kind: CapabilityKind,
        prefix: impl Into<String>,
        value: Option<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind: Some(kind),
            prefix: prefix.into(),
            value,
            detail: detail.into(),
        }
    }

    pub fn always(
        prefix: impl Into<String>,
        value: Option<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind: None,
            prefix: prefix.into(),
            value,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderCapabilities {
    items: Vec<CapabilityItem>,
}

impl FolderCapabilities {
    pub fn new(items: Vec<CapabilityItem>) -> Self {
        Self { items }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> &[CapabilityItem] {
        &self.items
    }

    pub fn active_items(
        &self,
        preferences: CapabilityPreferences,
    ) -> impl Iterator<Item = &CapabilityItem> {
        self.items
            .iter()
            .filter(move |item| item.kind.is_none_or(|kind| preferences.enabled(kind)))
    }
}

/// Inspect `directory` and its bounded ancestors across all capability modules.
pub fn detect(directory: &Path) -> FolderCapabilities {
    detect_resolved(directory, directory)
}

type Detector = fn(&[scan::DirSnapshot]) -> Option<CapabilityItem>;

const DETECTORS: [Detector; 16] = [
    git::detect,
    python::detect,
    node::detect,
    rust::detect,
    go::detect,
    ruby::detect,
    jvm::detect,
    cpp::detect,
    swift::detect,
    dotnet::detect,
    php::detect,
    zig::detect,
    elixir::detect,
    dart::detect,
    terraform::detect,
    toolchains::detect,
];

/// Inspect `directory` (the requested path) and `resolved` (its canonical target)
/// along with bounded ancestors across all capability modules.
pub fn detect_resolved(directory: &Path, resolved: &Path) -> FolderCapabilities {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    detect_in_context(directory, resolved, home.as_deref(), MountOrigin::Local)
}

/// Inspect `directory` and `resolved` with explicit home and mount context
/// along with bounded ancestors across all capability modules.
pub fn detect_in_context(
    directory: &Path,
    resolved: &Path,
    home: Option<&Path>,
    mount: MountOrigin,
) -> FolderCapabilities {
    let scan_contents = !matches!(mount, MountOrigin::Network { .. });
    detect_in_context_measurable(directory, resolved, home, mount, scan_contents)
}

/// Inspect `directory` and `resolved` with explicit home, mount, and content-scan gate.
pub fn detect_in_context_measurable(
    directory: &Path,
    resolved: &Path,
    home: Option<&Path>,
    mount: MountOrigin,
    scan_contents: bool,
) -> FolderCapabilities {
    let mut items = folder::detect_with_context(directory, resolved, home, &mount);
    if !scan_contents {
        return FolderCapabilities::new(items);
    }
    let ancestors = scan::ancestors(resolved);
    for detector in DETECTORS {
        if let Some(item) = detector(&ancestors) {
            items.push(item);
        }
    }
    if let Some(current) = ancestors.first() {
        items.extend(env::detect(current));
    }
    FolderCapabilities::new(items)
}
