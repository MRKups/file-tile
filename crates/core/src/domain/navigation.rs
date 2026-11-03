use std::collections::VecDeque;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationIntent {
    Open,
    Back,
    Forward,
    Reload,
    Initialize,
    Restore,
    Activate,
    Configure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Navigation {
    current: PathBuf,
    start: PathBuf,
    back: VecDeque<PathBuf>,
    forward: VecDeque<PathBuf>,
    scope: super::Scope,
}

impl Navigation {
    pub const HISTORY_LIMIT: usize = 100;

    pub fn new(start: PathBuf) -> Self {
        Self {
            current: start.clone(),
            start,
            back: VecDeque::new(),
            forward: VecDeque::new(),
            scope: super::Scope::default(),
        }
    }

    pub fn current(&self) -> &Path {
        &self.current
    }
    pub fn start(&self) -> &Path {
        &self.start
    }

    pub fn back_history(&self) -> &VecDeque<PathBuf> {
        &self.back
    }

    pub fn forward_history(&self) -> &VecDeque<PathBuf> {
        &self.forward
    }

    pub(crate) fn restore(
        start: PathBuf,
        current: PathBuf,
        back: VecDeque<PathBuf>,
        forward: VecDeque<PathBuf>,
    ) -> Option<Self> {
        if back.len() > Self::HISTORY_LIMIT || forward.len() > Self::HISTORY_LIMIT {
            return None;
        }
        Some(Self {
            start,
            current,
            back,
            forward,
            scope: super::Scope::default(),
        })
    }
    pub fn back_target(&self) -> Option<&Path> {
        self.back.back().map(PathBuf::as_path)
    }
    pub fn forward_target(&self) -> Option<&Path> {
        self.forward.back().map(PathBuf::as_path)
    }
    pub fn up_target(&self) -> Option<&Path> {
        self.current.parent().filter(|path| self.scope.allows(path))
    }

    pub fn scope(&self) -> &super::Scope {
        &self.scope
    }
    /// Nearest first. Restrictive traversal stops at its first disallowed parent.
    pub fn allowed_ancestors(&self) -> impl Iterator<Item = &Path> {
        self.current
            .ancestors()
            .skip(1)
            .take_while(|path| self.scope.allows(path))
    }
    pub fn return_target(&self) -> &Path {
        if self.scope.allows(&self.start) {
            &self.start
        } else {
            self.scope.fallback().unwrap_or(&self.start)
        }
    }
    pub fn with_scope(mut self, scope: super::Scope) -> Self {
        self.scope = scope;
        self
    }
    pub fn set_scope(&mut self, scope: super::Scope) {
        if matches!(
            scope.kind(),
            super::ScopeKind::Pinned | super::ScopeKind::Branch
        ) && let Some(root) = scope.root()
        {
            self.start = root.to_path_buf();
        }
        if !scope.allows(&self.current)
            && let Some(root) = scope.fallback()
        {
            self.current = root.to_path_buf();
        }
        self.scope = scope;
        self.back.clear();
        self.forward.clear();
    }

    /// Commit only after the application accepts a successful current listing.
    pub(crate) fn commit(&mut self, target: PathBuf, intent: NavigationIntent) {
        if (intent != NavigationIntent::Configure || self.scope.kind() == super::ScopeKind::Range)
            && !self.scope.allows(&target)
        {
            return;
        }
        match intent {
            NavigationIntent::Configure => {
                let scope = if matches!(
                    self.scope.kind(),
                    super::ScopeKind::Pinned | super::ScopeKind::Branch
                ) {
                    self.scope
                        .select(self.scope.kind(), target.clone())
                        .unwrap_or_else(|_| self.scope.clone())
                } else {
                    self.scope.clone()
                };
                *self = Self::new(target).with_scope(scope);
            }
            NavigationIntent::Initialize => {
                self.current = target.clone();
                self.start = target;
            }
            NavigationIntent::Reload => {}
            NavigationIntent::Restore | NavigationIntent::Activate => {
                self.current = target;
            }
            NavigationIntent::Open => {
                if target != self.current {
                    push_history(&mut self.back, self.current.clone());
                    self.forward.clear();
                    self.current = target;
                }
            }
            NavigationIntent::Back => {
                if target != self.current {
                    self.back.pop_back();
                    push_history(&mut self.forward, self.current.clone());
                    self.current = target;
                }
            }
            NavigationIntent::Forward => {
                if target != self.current {
                    self.forward.pop_back();
                    push_history(&mut self.back, self.current.clone());
                    self.current = target;
                }
            }
        }
    }
}

fn push_history(history: &mut VecDeque<PathBuf>, path: PathBuf) {
    history.push_back(path);
    if history.len() > Navigation::HISTORY_LIMIT {
        history.pop_front();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathInputError {
    Empty,
    InvalidCharacters,
    UnsupportedHome,
}

impl std::fmt::Display for PathInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Empty => "Enter a folder path.",
            Self::InvalidCharacters => "Folder paths cannot contain line breaks or null characters. Edit the path and retry.",
            Self::UnsupportedHome => "Use ~ or ~/ for the home folder, not ~name.",
        })
    }
}

/// Validate and expand user input without changing filesystem-sensitive `..` semantics.
pub fn expand_path(input: &str, current: &Path, home: &Path) -> Result<PathBuf, PathInputError> {
    if input.contains(['\n', '\r', '\0']) {
        return Err(PathInputError::InvalidCharacters);
    }
    let input = input.trim();
    if input.is_empty() {
        return Err(PathInputError::Empty);
    }
    let path = if input == "~" {
        home.to_path_buf()
    } else if let Some(relative) = input.strip_prefix("~/") {
        home.join(relative)
    } else if input.starts_with('~') {
        return Err(PathInputError::UnsupportedHome);
    } else if Path::new(input).is_absolute() {
        PathBuf::from(input)
    } else {
        current.join(input)
    };
    Ok(path)
}

pub(crate) fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}
