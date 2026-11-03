//! Explicit shortcut handoffs. Workspace state owns actions; receipts never launch.
use crate::application::{Application, Effect};
use crate::domain::{BoxId, LauncherConfig, LauncherTarget, Mode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub id: BoxId,
    pub generation: u64,
    pub epoch: Option<crate::application::contracts::storage::SessionId>,
    pub target: LauncherTarget,
}
#[derive(Debug, Clone)]
pub struct Completion {
    pub request: Request,
    pub result: Result<(), String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Pending,
    HandedOff,
    Failed(String),
}
#[derive(Debug)]
pub struct Opening {
    pub request: Request,
    pub config: LauncherConfig,
    pub status: Status,
}
#[derive(Debug, Default)]
pub(in crate::application) struct OpeningBook {
    pub current: Option<Opening>,
    generation: u64,
    failures: std::collections::HashMap<BoxId, Failure>,
}
#[derive(Debug)]
struct Failure {
    config: LauncherConfig,
    epoch: Option<crate::application::contracts::storage::SessionId>,
    message: String,
}

impl Application {
    pub fn launcher_opening(&self) -> Option<&Opening> {
        self.launcher_opening.current.as_ref()
    }
    pub fn launcher_status(&self, id: BoxId) -> Option<&str> {
        let opening = self
            .launcher_opening
            .current
            .as_ref()
            .filter(|opening| opening.request.id == id && opening.status == Status::Pending);
        if opening.is_some() {
            Some("Opening…")
        } else {
            self.launcher_opening
                .failures
                .get(&id)
                .map(|failure| failure.message.as_str())
        }
    }
    pub fn launcher_failed(&self, id: BoxId) -> bool {
        self.launcher_opening.failures.contains_key(&id)
    }
    pub(in crate::application) fn activate_launcher(&mut self, id: BoxId) -> Vec<Effect> {
        if !self.accepts_workspace_input()
            || self.workspace.mode() != Mode::Browse
            || self
                .launcher_opening
                .current
                .as_ref()
                .is_some_and(|opening| opening.status == Status::Pending)
        {
            return vec![];
        }
        let Some(item) = self.workspace.launcher(id) else {
            return vec![];
        };
        let Some(target) = item
            .config
            .target
            .clone()
            .filter(|target| target.validate().is_ok())
        else {
            return vec![];
        };
        let Some(generation) = self.launcher_opening.generation.checked_add(1) else {
            return vec![];
        };
        self.launcher_opening.generation = generation;
        self.launcher_opening.failures.remove(&id);
        let request = Request {
            id,
            generation,
            epoch: self.workspace_epoch(),
            target,
        };
        self.launcher_opening.current = Some(Opening {
            request: request.clone(),
            config: item.config.clone(),
            status: Status::Pending,
        });
        vec![Effect::OpenLauncher(request)]
    }
    pub(in crate::application) fn launcher_opened(&mut self, completion: Completion) {
        if let Some(opening) = &mut self.launcher_opening.current
            && opening.request == completion.request
            && opening.status == Status::Pending
        {
            opening.status = match completion.result {
                Ok(()) => Status::HandedOff,
                Err(error) => {
                    self.launcher_opening.failures.insert(
                        opening.request.id,
                        Failure {
                            config: opening.config.clone(),
                            epoch: opening.request.epoch,
                            message: error.clone(),
                        },
                    );
                    Status::Failed(error)
                }
            };
        }
    }
    pub(in crate::application) fn sync_launcher_opening(&mut self) {
        let epoch = self.workspace_epoch();
        let workspace = &self.workspace;
        self.launcher_opening.failures.retain(|id, failure| {
            failure.epoch == epoch
                && workspace
                    .launcher(*id)
                    .is_some_and(|item| item.config == failure.config)
        });
        if self
            .launcher_opening
            .current
            .as_ref()
            .is_some_and(|opening| {
                !self.accepts_workspace_input()
                    || self.workspace.mode() != Mode::Browse
                    || opening.request.epoch != self.workspace_epoch()
                    || !self
                        .workspace
                        .launcher(opening.request.id)
                        .is_some_and(|item| item.config == opening.config)
            })
        {
            self.launcher_opening.current = None;
        }
    }
}
