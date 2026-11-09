//! Atomic scope resolution and budget admission. No filesystem or JSON work here.

use crate::application::contracts::configuration::{
    WorkspaceDocument, WorkspaceId, validate_saved_path,
};
use crate::application::contracts::directory::{ListingCompletion, ListingRequest};
use crate::application::contracts::storage::{BudgetRequest, StorageFailure};
use crate::application::{Application, Effect};
use crate::domain::{
    NavigationIntent, NavigationRange, Scope, ScopeKind, ViewId, Workspace, expand_path,
};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum ScopeEdit {
    NativeBoundaries {
        start: PathBuf,
        stops: Vec<PathBuf>,
    },
    Select(ScopeKind),
    Boundaries {
        start: String,
        stops: String,
    },
    Draft {
        start: Option<String>,
        stops: Option<String>,
    },
    Start(PathBuf),
    ToggleStop(PathBuf),
}

#[derive(Debug)]
pub(in crate::application) struct PendingScope {
    view: ViewId,
    baseline: Workspace,
    kind: ScopeKind,
    paths: VecDeque<PathBuf>,
    resolved: Vec<PathBuf>,
    strict: bool,
    pub request: Option<ListingRequest>,
    pub budget: Option<BudgetRequest>,
}

impl Application {
    pub fn cancel_scope_edit(&mut self) {
        self.pending_scope = None;
        self.scope_error = None;
    }
    pub fn scope_error(&self, view: ViewId) -> Option<&str> {
        self.scope_error
            .as_ref()
            .filter(|(id, _)| *id == view)
            .map(|(_, error)| error.as_str())
    }
    pub fn scope_pending(&self, view: ViewId) -> bool {
        self.pending_scope
            .as_ref()
            .is_some_and(|pending| pending.view == view)
    }
    pub fn scope_budget(&self) -> Option<BudgetRequest> {
        self.pending_scope.as_ref()?.budget.clone()
    }
    pub fn scope_request(&self) -> Option<&ListingRequest> {
        self.pending_scope.as_ref()?.request.as_ref()
    }
    pub fn read_scope(&self, view: ViewId) -> Scope {
        let scope = self
            .workspace
            .view(view)
            .map(|item| item.navigation.scope().clone())
            .unwrap_or_default();
        let configuring_tabbed_box = self
            .workspace
            .box_for_view(view)
            .is_some_and(crate::domain::FolderBox::has_visible_tabs);
        if self
            .pending_configuration
            .as_ref()
            .is_some_and(|pending| pending.view == view)
            && (configuring_tabbed_box || scope.kind() != ScopeKind::Range)
        {
            Scope::default()
        } else {
            scope
        }
    }
    pub(in crate::application) fn cancel_invalid_scope_context(&mut self) {
        if self.pending_scope.as_ref().is_some_and(|pending| {
            !self.accepts_workspace_input()
                || !self
                    .workspace
                    .view(pending.view)
                    .is_some_and(|item| self.workspace.configuring(item.id))
        }) {
            self.pending_scope = None;
        }
    }
    pub(in crate::application) fn edit_scope(
        &mut self,
        view: ViewId,
        edit: ScopeEdit,
    ) -> Vec<Effect> {
        if !self
            .workspace
            .view(view)
            .is_some_and(|item| self.workspace.configuring(item.id))
        {
            return vec![];
        }
        let mut effects = self.cancel_folder_configuration();
        self.pending_tabs = None;
        self.pending_scope = None;
        self.scope_error = None;
        let proposal = self.scope_paths(view, edit);
        match proposal {
            Ok((kind, paths, strict)) => {
                self.pending_scope = Some(PendingScope {
                    view,
                    baseline: self.workspace.snapshot(),
                    kind,
                    paths: paths.into(),
                    resolved: vec![],
                    strict,
                    request: None,
                    budget: None,
                });
                effects.extend(self.next_scope_path());
            }
            Err(error) => {
                self.scope_error = Some((
                    view,
                    format!("Cannot configure scope: {error}. Correct the boundaries and retry."),
                ))
            }
        }
        effects
    }
    fn scope_paths(
        &self,
        view: ViewId,
        edit: ScopeEdit,
    ) -> Result<(ScopeKind, Vec<PathBuf>, bool), String> {
        let nav = self
            .workspace
            .view(view)
            .ok_or("View no longer exists")?
            .navigation;
        let expand = |text: &str| {
            expand_path(text, nav.current(), &self.home).map_err(|error| error.to_string())
        };
        let result = match edit {
            ScopeEdit::NativeBoundaries { start, stops } => {
                if stops.is_empty() || stops.len() > NavigationRange::MAX_STOPS {
                    return Err("Choose 1 to 64 range ends".into());
                }
                (
                    ScopeKind::Range,
                    std::iter::once(start).chain(stops).collect(),
                    false,
                )
            }
            ScopeEdit::Select(ScopeKind::Free) => (ScopeKind::Free, vec![], false),
            ScopeEdit::Select(ScopeKind::Range) if nav.scope().range().is_some() => {
                let scope = nav
                    .scope()
                    .select(ScopeKind::Range, nav.current().into())
                    .map_err(str::to_owned)?;
                (ScopeKind::Range, scope.boundary_paths(), true)
            }
            ScopeEdit::Select(kind) => (
                kind,
                if kind == ScopeKind::Range {
                    vec![nav.current().into(), nav.current().into()]
                } else {
                    vec![nav.current().into()]
                },
                false,
            ),
            ScopeEdit::Start(path) => (ScopeKind::Range, vec![path.clone(), path], false),
            ScopeEdit::Boundaries { start, stops } => {
                let mut paths = vec![expand(&start)?];
                append_stop_lines(&stops, &mut paths, &expand)?;
                (ScopeKind::Range, paths, false)
            }
            ScopeEdit::Draft { start, stops } => {
                let range = nav.scope().range().ok_or("Choose a Range Start first")?;
                let mut paths =
                    vec![start.map_or_else(|| Ok(range.start().into()), |start| expand(&start))?];
                if let Some(stops) = stops {
                    append_stop_lines(&stops, &mut paths, &expand)?;
                } else {
                    paths.extend_from_slice(range.stops());
                }
                (ScopeKind::Range, paths, false)
            }
            ScopeEdit::ToggleStop(path) => {
                let range = nav
                    .scope()
                    .range()
                    .ok_or("Choose a Range Start first")?
                    .toggle(path)
                    .map_err(str::to_owned)?;
                (
                    ScopeKind::Range,
                    std::iter::once(range.start().into())
                        .chain(range.stops().iter().cloned())
                        .collect(),
                    true,
                )
            }
        };
        for path in &result.1 {
            validate_saved_path(path)?;
        }
        Ok(result)
    }
    fn next_scope_path(&mut self) -> Vec<Effect> {
        let Some(pending) = &mut self.pending_scope else {
            return vec![];
        };
        let Some(directory) = pending.paths.pop_front() else {
            return self.scope_proposal();
        };
        let Some(generation) = self.next_generation.checked_add(1) else {
            self.pending_scope = None;
            return vec![];
        };
        self.next_generation = generation;
        let request = ListingRequest {
            view: pending.view,
            generation,
            directory,
        };
        pending.request = Some(request.clone());
        vec![Effect::ResolveScope(request)]
    }
    pub(in crate::application) fn scope_resolved(
        &mut self,
        completion: ListingCompletion,
    ) -> Vec<Effect> {
        let Some(pending) = &mut self.pending_scope else {
            return vec![];
        };
        if pending.request.as_ref() != Some(&completion.request) {
            return vec![];
        }
        let view = pending.view;
        match completion.result {
            Ok(listing) if !pending.strict || listing.directory == completion.request.directory => {
                if let Err(error) = validate_saved_path(&listing.directory) {
                    self.scope_error = Some((view, error.into()));
                    self.pending_scope = None;
                    return vec![];
                }
                pending.resolved.push(listing.directory);
                pending.request = None;
                self.next_scope_path()
            }
            result => {
                self.scope_error = Some((view, match result { Err(error) => error.describe("resolve scope boundary"), Ok(_) => "Scope boundary now resolves elsewhere. Keep existing restrictions or explicitly configure replacement boundaries.".into() }));
                self.pending_scope = None;
                vec![]
            }
        }
    }
    fn scope_proposal(&mut self) -> Vec<Effect> {
        let Some(pending) = &self.pending_scope else {
            return vec![];
        };
        let view = pending.view;
        let nav = pending.baseline.view(view).map(|item| item.navigation);
        let Some(nav) = nav else {
            return vec![];
        };
        let scope = if pending.kind == ScopeKind::Range {
            pending
                .resolved
                .first()
                .ok_or("Missing Range Start")
                .and_then(|start| {
                    NavigationRange::new(start.clone(), pending.resolved[1..].to_vec())
                })
                .map(|range| nav.scope().with_range(range))
        } else {
            nav.scope().select(
                pending.kind,
                pending
                    .resolved
                    .first()
                    .cloned()
                    .unwrap_or_else(|| nav.current().into()),
            )
        };
        let scope = match scope {
            Ok(scope) => scope,
            Err(error) => {
                self.scope_error = Some((
                    view,
                    format!("Cannot configure scope: {error}. Correct the boundaries and retry."),
                ));
                self.pending_scope = None;
                return vec![];
            }
        };
        let Some(workspace) = self.workspace.propose_scope(view, scope) else {
            self.pending_scope = None;
            return vec![];
        };
        let Some(generation) = self.next_tab_generation.checked_add(1) else {
            self.pending_scope = None;
            return vec![];
        };
        self.next_tab_generation = generation;
        let id = self
            .configuration
            .as_ref()
            .and_then(|(preferences, _)| preferences.last_workspace_id)
            .unwrap_or_else(WorkspaceId::random);
        let request = BudgetRequest {
            generation,
            document: Arc::new(WorkspaceDocument { id, workspace }),
        };
        if let Some(pending) = &mut self.pending_scope {
            pending.budget = Some(request.clone());
        }
        vec![Effect::CheckWorkspaceBudget(request)]
    }
    pub(in crate::application) fn scope_budget_checked(
        &mut self,
        generation: u64,
        result: Result<(), StorageFailure>,
    ) -> Option<Vec<Effect>> {
        if !self.pending_scope.as_ref().is_some_and(|pending| {
            pending
                .budget
                .as_ref()
                .is_some_and(|request| request.generation == generation)
        }) {
            return None;
        }
        let pending = self.pending_scope.take()?;
        if !self.accepts_workspace_input()
            || !self
                .workspace
                .view(pending.view)
                .is_some_and(|item| self.workspace.configuring(item.id))
        {
            return Some(vec![]);
        }
        if result.is_err() || !self.workspace.same_durable_state(&pending.baseline) {
            self.scope_error = Some((pending.view, "Cannot accept scope change: validation failed, the saved-size limit was exceeded, or the workspace changed. Keep the current scope and retry.".into()));
            return Some(vec![]);
        }
        let request = pending.budget?;
        self.workspace
            .accept_tab_proposal(request.document.workspace.snapshot());
        crate::diagnostics::configuration_changed(crate::diagnostics::Change::BoxScope);
        self.loads.remove(&pending.view);
        let path = self
            .workspace
            .view(pending.view)?
            .navigation
            .current()
            .into();
        let mut effects: Vec<_> = self
            .request(pending.view, path, NavigationIntent::Activate)
            .into_iter()
            .collect();
        effects.extend(self.save_effect(true));
        Some(effects)
    }
}

fn append_stop_lines(
    stops: &str,
    paths: &mut Vec<PathBuf>,
    expand: &dyn Fn(&str) -> Result<PathBuf, String>,
) -> Result<(), String> {
    let lines: Vec<_> = stops
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.is_empty() || lines.len() > NavigationRange::MAX_STOPS {
        return Err("Enter 1 to 64 Stop paths, one per line".into());
    }
    for line in lines {
        paths.push(expand(line)?);
    }
    Ok(())
}
