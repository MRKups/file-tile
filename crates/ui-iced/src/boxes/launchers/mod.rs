//! Shortcut drafts and bounded opening lifetime, separate from workspace values.
mod icon_picker;
mod view;
use crate::{Desktop, Message};
use crate::{
    application::{
        Action,
        workspace::launchers::{Request, Status},
    },
    domain::{BoxId, LauncherConfig, LauncherIcon, LauncherTarget, Mode},
};
use iced::{Task, widget::operation};
use std::path::PathBuf;
pub(crate) use view::{editor, tile};

pub(crate) const EDITOR_ID: &str = "launcher-editor";
pub(crate) const CHOOSE_ID: &str = "launcher-choose";

#[derive(Debug, Clone)]
pub(crate) enum Event {
    FocusChecked(u64),
    Edit(BoxId),
    Name(u64, String),
    Target(u64, String),
    Icon(u64, LauncherIcon),
    Apply(u64),
    Cancel(u64),
    Choose(u64),
    Chosen(u64, Result<Option<PathBuf>, String>),
}
pub(crate) struct Draft {
    pub id: BoxId,
    pub generation: u64,
    pub name: String,
    pub target: String,
    pub icon: LauncherIcon,
    pub error: Option<String>,
    baseline: LauncherConfig,
    target_edited: bool,
    chosen: Option<PathBuf>,
    picker: Option<u64>,
}
impl Draft {
    pub fn config(&self, home: &std::path::Path) -> Result<LauncherConfig, &'static str> {
        let config = LauncherConfig {
            name: self.name.clone(),
            target: self.target_value(home)?,
            icon: self.icon,
        };
        config.validate()?;
        Ok(config)
    }
    pub fn target_value(
        &self,
        home: &std::path::Path,
    ) -> Result<Option<LauncherTarget>, &'static str> {
        let target = if let Some(path) = &self.chosen {
            Some(LauncherTarget::Path(path.clone()))
        } else if !self.target_edited {
            self.baseline.target.clone()
        } else if self.target.is_empty() {
            None
        } else {
            Some(LauncherTarget::from_input(&self.target, home)?)
        };
        if let Some(target) = &target {
            target.validate()?;
        }
        Ok(target)
    }
    pub fn changed(&self, home: &std::path::Path) -> bool {
        self.config(home)
            .is_ok_and(|config| config != self.baseline)
    }
    pub(crate) fn picker(&self) -> Option<u64> {
        self.picker
    }
    pub(crate) fn clear_picker(&mut self) {
        self.picker = None;
    }
}
struct Pending {
    request: Request,
    cancellation: crate::application::contracts::directory::Cancellation,
    handle: iced::task::Handle,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.handle.abort();
    }
}
#[derive(Default)]
pub(crate) struct Runtime {
    pub draft: Option<Draft>,
    generation: u64,
    pending: Option<Pending>,
}

impl Desktop {
    pub(crate) fn sync_launchers(&mut self) {
        if self.launchers.pending.as_ref().is_some_and(|pending| {
            !self.application.launcher_opening().is_some_and(|opening| {
                opening.request == pending.request && opening.status == Status::Pending
            })
        }) {
            self.launchers.pending = None;
        }
        let item = self
            .application
            .workspace()
            .launchers()
            .iter()
            .find(|item| self.application.workspace().configuring(item.id))
            .filter(|_| self.application.accepts_edits());
        let Some(item) = item else {
            self.launchers.draft = None;
            return;
        };
        let blocked = self.modal_open() || !self.application.accepts_workspace_input();
        if let Some(draft) = &mut self.launchers.draft
            && draft.id == item.id
        {
            if blocked {
                draft.picker = None;
            }
            if draft.baseline != item.config {
                draft.baseline = item.config.clone();
                draft.target_edited = false;
                draft.chosen = None;
                draft.error = None;
            }
            return;
        }
        let Some(generation) = self.launchers.generation.checked_add(1) else {
            return;
        };
        self.launchers.generation = generation;
        self.launchers.draft = Some(Draft {
            id: item.id,
            generation,
            name: item.config.name.clone(),
            target: target_text(item.config.target.as_ref()),
            icon: item.config.icon,
            baseline: item.config.clone(),
            error: None,
            target_edited: false,
            chosen: None,
            picker: None,
        });
        self.focused_path = None;
        self.deactivate_all_views();
        self.focus_version = self.focus_version.saturating_add(1);
    }

    pub(crate) fn launcher_event(&mut self, event: Event) -> Task<Message> {
        if let Event::FocusChecked(version) = event {
            if version == self.focus_version {
                self.deactivate_all_views();
                self.focused_path = None;
            }
            return Task::none();
        }
        if let Event::Chosen(generation, result) = event {
            return self.launcher_target_chosen(generation, result);
        }
        if !self.workspace_input_enabled() {
            return Task::none();
        }
        if let Event::Edit(id) = event {
            if self.application.workspace().mode() != Mode::Browse
                || self.application.workspace().launcher(id).is_none()
            {
                return Task::none();
            }
            let mode = self.dispatch(Action::SelectMode(Mode::Configure));
            return Task::batch([
                mode,
                self.dispatch(Action::ConfigureBox(Some(id))),
                operation::focus("launcher-name"),
            ]);
        }
        let generation = match &event {
            Event::Name(id, _)
            | Event::Target(id, _)
            | Event::Icon(id, _)
            | Event::Apply(id)
            | Event::Cancel(id)
            | Event::Choose(id) => *id,
            _ => return Task::none(),
        };
        let Some(draft) = self
            .launchers
            .draft
            .as_mut()
            .filter(|draft| draft.generation == generation)
        else {
            return Task::none();
        };
        match event {
            Event::Name(_, name) => {
                draft.name = name;
                draft.error = None;
            }
            Event::Target(_, target) => {
                draft.target = target;
                draft.target_edited = true;
                draft.chosen = None;
                draft.picker = None;
                draft.error = None;
            }
            Event::Icon(_, icon) => draft.icon = icon,
            Event::Cancel(_) => return self.dispatch(Action::ConfigureBox(None)),
            Event::Apply(_) => match draft.config(self.application.home()) {
                Ok(config) => {
                    let id = draft.id;
                    return self.dispatch(Action::ConfigureLauncher(id, config));
                }
                Err(error) => draft.error = Some(error.into()),
            },
            Event::Choose(_) => {
                let home = self.application.home();
                let directory = match draft.target_value(home) {
                    Ok(Some(LauncherTarget::Path(path))) => {
                        path.parent().unwrap_or(&path).to_path_buf()
                    }
                    _ => home.to_path_buf(),
                };
                let receipt =
                    |generation, result| Message::Launcher(Event::Chosen(generation, result));
                let choice = self.settings_picker.start_target(directory, receipt);
                if let Some((id, task)) = choice {
                    draft.picker = Some(id);
                    return task;
                }
            }
            Event::Edit(_) | Event::Chosen(..) | Event::FocusChecked(_) => {}
        }
        Task::none()
    }

    fn launcher_target_chosen(
        &mut self,
        generation: u64,
        result: Result<Option<PathBuf>, String>,
    ) -> Task<Message> {
        if !self.settings_picker.finish(generation) || !self.workspace_input_enabled() {
            return Task::none();
        }
        if let Some(draft) = &mut self.launchers.draft
            && draft.picker == Some(generation)
        {
            draft.picker = None;
            match result {
                Ok(Some(path)) => {
                    draft.target = path.to_string_lossy().into_owned();
                    draft.chosen = Some(path);
                    draft.target_edited = true;
                    draft.error = None;
                }
                Ok(None) => {}
                Err(error) => draft.error = Some(error),
            }
            return operation::focus(CHOOSE_ID);
        }
        Task::none()
    }

    pub(crate) fn open_launcher_task(&mut self, request: Request) -> Task<Message> {
        let Some(workers) = &self.open_workers else {
            return Task::done(Message::Action(Action::LauncherOpened(crate::application::workspace::launchers::Completion {
            request, result: Err("Cannot open shortcut: the opening worker is unavailable. Relaunch and retry.".into()),
        })));
        };
        let future = workers.open_launcher(request.clone());
        let cancellation = future.cancellation();
        let (task, handle) = Task::perform(future, |result| {
            Message::Action(Action::LauncherOpened(result))
        })
        .abortable();
        self.launchers.pending = Some(Pending {
            request,
            cancellation,
            handle,
        });
        task
    }
}

pub(crate) fn target_text(target: Option<&LauncherTarget>) -> String {
    match target {
        Some(LauncherTarget::Path(path)) => path.to_string_lossy().into_owned(),
        Some(LauncherTarget::Link(link)) => link.clone(),
        None => String::new(),
    }
}
