//! Validated configured-folder commits through the existing tagged listing owner.

use crate::application::{Action, Application, Effect, LoadState};
use crate::diagnostics::{Change, configuration_changed};
use crate::domain::{Mode, NavigationIntent, ViewId, expand_path};

#[derive(Debug)]
pub(in crate::application) struct PendingFolder {
    pub view: ViewId,
    baseline: Option<LoadState>,
}

impl Application {
    pub(in crate::application) fn commit_listed_navigation(
        &mut self,
        view: ViewId,
        directory: std::path::PathBuf,
        intent: NavigationIntent,
    ) -> bool {
        let changed = intent != NavigationIntent::Reload
            && self.workspace.commit_navigation(view, directory, intent);
        if changed && intent == NavigationIntent::Configure {
            configuration_changed(Change::BoxFolder);
        }
        changed
    }

    pub fn suspend_box_configuration(&mut self) -> Vec<Effect> {
        self.pending_tabs = None;
        self.folder_tree = None;
        self.cancel_folder_configuration()
    }
    pub(in crate::application) fn workspace_mode_action(&mut self, action: Action) -> Vec<Effect> {
        self.pending_tabs = None;
        self.tab_error = None;
        self.scope_error = None;
        let effects = self.cancel_folder_configuration();
        self.box_configuration_error = None;
        match action {
            Action::SelectMode(mode) => self.workspace.select_mode(mode),
            Action::Escape => self.workspace.escape(),
            _ => {}
        }
        self.status = None;
        effects
    }
    pub(in crate::application) fn box_configuration_action(
        &mut self,
        action: Action,
    ) -> Vec<Effect> {
        match action {
            Action::ConfigureBox(id) if self.workspace.mode() == Mode::Configure => {
                self.scope_error = None;
                self.pending_tabs = None;
                self.tab_error = None;
                let effects = self.cancel_folder_configuration();
                self.box_configuration_error = None;
                self.workspace.configure_box(id);
                effects
            }
            Action::SetBoxSetting(id, setting) => {
                if self.workspace.change_box_setting(id, setting) {
                    configuration_changed(Change::BoxSettings);
                    self.save_effect(true)
                } else {
                    Vec::new()
                }
            }
            Action::ConfigureFolder(view, input) => self.configure_folder(view, input),
            Action::ConfigurePath(view, path) => self.request_configured_folder(view, Ok(path)),
            Action::ConfigureCurrentFolder(view) => {
                let target = self.workspace.box_for_view(view).and_then(|box_item| {
                    if box_item.has_visible_tabs() {
                        Some(box_item.default_folder().to_path_buf())
                    } else {
                        self.workspace
                            .view(view)
                            .map(|item| item.navigation.current().to_path_buf())
                    }
                });
                target.map_or_else(Vec::new, |path| {
                    self.request_configured_folder(view, Ok(path))
                })
            }
            Action::ConfiguredFolderEdited(view)
                if self
                    .workspace
                    .view(view)
                    .is_some_and(|item| self.workspace.configuring(item.id)) =>
            {
                let effects = self.cancel_folder_configuration();
                self.box_configuration_error = None;
                effects
            }
            _ => Vec::new(),
        }
    }
    pub fn box_configuration_error(&self, view: ViewId) -> Option<&str> {
        self.box_configuration_error
            .as_ref()
            .filter(|(id, _)| *id == view)
            .map(|(_, error)| error.as_str())
    }

    pub(in crate::application) fn configure_folder(
        &mut self,
        view: ViewId,
        input: String,
    ) -> Vec<Effect> {
        let Some(box_item) = self.workspace.box_for_view(view) else {
            return Vec::new();
        };
        if !self.workspace.configuring(box_item.id) {
            return Vec::new();
        }
        let base = if box_item.has_visible_tabs() {
            box_item.default_folder()
        } else {
            let Some(item) = self.workspace.view(view) else {
                return Vec::new();
            };
            item.navigation.current()
        };
        let target = expand_path(&input, base, &self.home).map_err(|error| error.to_string());
        self.request_configured_folder(view, target)
    }

    pub(in crate::application) fn request_configured_folder(
        &mut self,
        view: ViewId,
        target: Result<std::path::PathBuf, String>,
    ) -> Vec<Effect> {
        if !self
            .workspace
            .view(view)
            .is_some_and(|item| self.workspace.configuring(item.id))
        {
            return Vec::new();
        }
        let target = target.and_then(|path| {
            crate::application::contracts::configuration::validate_saved_path(&path)?;
            Ok(path)
        });
        self.box_configuration_error = None;
        let mut effects = self.cancel_folder_configuration();
        match target {
            Ok(path) => {
                let baseline = self.loads.get(&view).cloned();
                if let Some(effect) = self.request(view, path, NavigationIntent::Configure) {
                    self.pending_configuration = Some(PendingFolder { view, baseline });
                    // A new request supersedes a restarted baseline read in the same update.
                    effects.retain(
                        |effect| !matches!(effect, Effect::List(request) if request.view == view),
                    );
                    effects.push(effect);
                }
            }
            Err(error) => self.box_configuration_error = Some((view, error)),
        }
        effects
    }

    pub(in crate::application) fn cancel_folder_configuration(&mut self) -> Vec<Effect> {
        self.pending_scope = None;
        let Some(pending) = self.pending_configuration.take() else {
            return Vec::new();
        };
        self.loads.remove(&pending.view);
        match pending.baseline {
            Some(LoadState::Loading {
                intent, previous, ..
            }) => {
                if let Some(previous) = previous {
                    self.loads.insert(pending.view, LoadState::Ready(previous));
                } else if let Some(item) = self.workspace.view(pending.view) {
                    let path = item.navigation.current().to_path_buf();
                    return self
                        .request(
                            pending.view,
                            path,
                            if intent == NavigationIntent::Initialize {
                                NavigationIntent::Initialize
                            } else {
                                NavigationIntent::Restore
                            },
                        )
                        .into_iter()
                        .collect();
                }
            }
            Some(baseline) => {
                self.loads.insert(pending.view, baseline);
            }
            None => {}
        }
        Vec::new()
    }
}
