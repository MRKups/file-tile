//! Modular, read-only folder capability detectors.
//!
//! Each capability lives in its own isolated module, inspects small marker files
//! without spawning subprocesses, and emits compact tokens for the folder box
//! footer along with human-readable details for hover tooltips.

pub mod env;
pub mod git;
pub mod go;
pub mod node;
pub mod python;
pub mod rust;
pub mod scan;

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CapabilityKind {
    Git,
    Python,
    Node,
    Rust,
    Go,
    EnvFiles,
}

impl CapabilityKind {
    pub const ALL: [Self; 6] = [
        Self::Git,
        Self::Python,
        Self::Node,
        Self::Rust,
        Self::Go,
        Self::EnvFiles,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Git => "Git",
            Self::Python => "Python",
            Self::Node => "Node.js & JavaScript",
            Self::Rust => "Rust",
            Self::Go => "Go",
            Self::EnvFiles => "Environment Files (.env)",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Git => "Show active Git branch, detached commit, worktree, or submodule state.",
            Self::Python => "Detect virtual environments (.venv), Python versions, uv, and Poetry.",
            Self::Node => {
                "Detect Node versions (.nvmrc, .node-version) and npm, pnpm, Bun, or Yarn."
            }
            Self::Rust => "Detect Cargo packages, workspaces, Rust editions, and toolchains.",
            Self::Go => "Detect Go modules (go.mod) and multi-module workspaces (go.work).",
            Self::EnvFiles => "Highlight when .env or .envrc override files exist in the folder.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityPreferences {
    pub git: bool,
    pub python: bool,
    pub node: bool,
    pub rust: bool,
    pub go: bool,
    pub env_files: bool,
}

impl Default for CapabilityPreferences {
    fn default() -> Self {
        Self {
            git: true,
            python: true,
            node: true,
            rust: true,
            go: true,
            env_files: true,
        }
    }
}

impl CapabilityPreferences {
    pub const fn enabled(self, kind: CapabilityKind) -> bool {
        match kind {
            CapabilityKind::Git => self.git,
            CapabilityKind::Python => self.python,
            CapabilityKind::Node => self.node,
            CapabilityKind::Rust => self.rust,
            CapabilityKind::Go => self.go,
            CapabilityKind::EnvFiles => self.env_files,
        }
    }

    pub const fn with_enabled(mut self, kind: CapabilityKind, enabled: bool) -> Self {
        match kind {
            CapabilityKind::Git => self.git = enabled,
            CapabilityKind::Python => self.python = enabled,
            CapabilityKind::Node => self.node = enabled,
            CapabilityKind::Rust => self.rust = enabled,
            CapabilityKind::Go => self.go = enabled,
            CapabilityKind::EnvFiles => self.env_files = enabled,
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityItem {
    pub kind: CapabilityKind,
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
            kind,
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
            .filter(move |item| preferences.enabled(item.kind))
    }
}

/// Inspect `directory` and its bounded ancestors across all capability modules.
pub fn detect(directory: &Path) -> FolderCapabilities {
    let ancestors = scan::ancestors(directory);
    let mut items = Vec::new();
    if let Some(item) = git::detect(&ancestors) {
        items.push(item);
    }
    if let Some(item) = python::detect(&ancestors) {
        items.push(item);
    }
    if let Some(item) = node::detect(&ancestors) {
        items.push(item);
    }
    if let Some(item) = rust::detect(&ancestors) {
        items.push(item);
    }
    if let Some(item) = go::detect(&ancestors) {
        items.push(item);
    }
    if let Some(item) = env::detect(directory) {
        items.push(item);
    }
    FolderCapabilities::new(items)
}
