//! Pure navigation boundaries over already resolved native path components.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScopeKind {
    #[default]
    Free,
    Pinned,
    Branch,
    Range,
}
impl ScopeKind {
    pub const ALL: [Self; 4] = [Self::Free, Self::Pinned, Self::Branch, Self::Range];
    pub fn explanation(self) -> &'static str {
        match self {
            Self::Free => "Browse anywhere.",
            Self::Pinned => "Keep this view in one folder.",
            Self::Branch => "Stay in this folder and its subfolders.",
            Self::Range => "Browse only along the selected branches.",
        }
    }
}
impl std::fmt::Display for ScopeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Free => "Free",
            Self::Pinned => "Pinned",
            Self::Branch => "Branch",
            Self::Range => "Range",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Range {
    start: PathBuf,
    stops: Vec<PathBuf>,
}
impl Range {
    pub const MAX_STOPS: usize = 64;
    pub fn new(start: PathBuf, stops: Vec<PathBuf>) -> Result<Self, &'static str> {
        if stops.is_empty() || stops.len() > Self::MAX_STOPS {
            return Err("Range needs 1 to 64 Stops");
        }
        if !canonical_form(&start)
            || stops
                .iter()
                .any(|stop| !canonical_form(stop) || !stop.starts_with(&start))
        {
            return Err("Every Stop must be Start or a folder below Start");
        }
        let mut unique = Vec::new();
        for stop in stops {
            if !unique.contains(&stop) {
                unique.push(stop);
            }
        }
        let stops = unique
            .iter()
            .filter(|stop| {
                !unique
                    .iter()
                    .any(|other| other != *stop && other.starts_with(stop))
            })
            .cloned()
            .collect();
        Ok(Self { start, stops })
    }
    pub fn start(&self) -> &Path {
        &self.start
    }
    pub fn stops(&self) -> &[PathBuf] {
        &self.stops
    }
    pub fn allows(&self, path: &Path) -> bool {
        if !canonical_form(path) {
            return false;
        }
        path.starts_with(&self.start) && self.stops.iter().any(|stop| stop.starts_with(path))
    }
    pub fn toggle(&self, path: PathBuf) -> Result<Self, &'static str> {
        let mut stops = self.stops.clone();
        if let Some(index) = stops.iter().position(|stop| stop == &path) {
            stops.remove(index);
        } else {
            stops.push(path);
        }
        Self::new(self.start.clone(), stops)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    kind: ScopeKind,
    root: Option<PathBuf>,
    range: Option<Range>,
}
impl Scope {
    pub fn restore(
        kind: ScopeKind,
        root: Option<PathBuf>,
        range: Option<Range>,
    ) -> Result<Self, &'static str> {
        if root.as_ref().is_some_and(|path| !canonical_form(path)) {
            return Err("Scope root must be an absolute resolved path");
        }
        if matches!(kind, ScopeKind::Pinned | ScopeKind::Branch) != root.is_some()
            || (kind == ScopeKind::Range && range.is_none())
        {
            return Err("Scope boundaries are missing or inconsistent");
        }
        Ok(Self { kind, root, range })
    }
    pub fn kind(&self) -> ScopeKind {
        self.kind
    }
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }
    pub fn range(&self) -> Option<&Range> {
        self.range.as_ref()
    }
    pub fn allows(&self, path: &Path) -> bool {
        match self.kind {
            ScopeKind::Free => true,
            ScopeKind::Pinned => self.root.as_deref() == Some(path),
            ScopeKind::Branch => self
                .root
                .as_ref()
                .is_some_and(|root| canonical_form(path) && path.starts_with(root)),
            ScopeKind::Range => self.range.as_ref().is_some_and(|range| range.allows(path)),
        }
    }
    pub(crate) fn boundary_paths(&self) -> Vec<PathBuf> {
        match self.kind {
            ScopeKind::Free => vec![],
            ScopeKind::Pinned | ScopeKind::Branch => self.root.iter().cloned().collect(),
            ScopeKind::Range => self
                .range
                .iter()
                .flat_map(|range| std::iter::once(range.start.clone()).chain(range.stops.clone()))
                .collect(),
        }
    }
    pub(super) fn fallback(&self) -> Option<&Path> {
        if self.kind == ScopeKind::Range {
            self.range.as_ref().map(Range::start)
        } else {
            self.root()
        }
    }
    pub fn select(&self, kind: ScopeKind, current: PathBuf) -> Result<Self, &'static str> {
        let root = matches!(kind, ScopeKind::Pinned | ScopeKind::Branch).then_some(current.clone());
        let range = if kind == ScopeKind::Range && self.range.is_none() {
            Some(Range::new(current.clone(), vec![current])?)
        } else {
            self.range.clone()
        };
        Self::restore(kind, root, range)
    }
    pub(crate) fn with_range(&self, range: Range) -> Self {
        Self {
            kind: ScopeKind::Range,
            root: None,
            range: Some(range),
        }
    }
}

fn canonical_form(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
}
