//! Main-thread application menu. No native window, delegate, or run-loop replacement.
use super::{MenuSignal, QuitSignal};
use crate::application::contracts::menu::{MenuCommand, MenuState};
use crate::domain::Mode;
use crate::platform::menu::{CheckedMode, MenuAction, NativeMenu, NativeMenuState};
use std::sync::Arc;

pub struct ApplicationMenu {
    // NativeMenu is MainThreadOnly (!Send). Desktop owns it on the main thread.
    menu: Option<NativeMenu>,
    signal: Arc<QuitSignal>,
    commands: Arc<MenuSignal>,
    error: Option<String>,
}

impl ApplicationMenu {
    /// Call once from Iced's Opened event, after winit's default menu initialization.
    pub fn install() -> Self {
        let signal = Arc::new(QuitSignal::default());
        let commands = Arc::new(MenuSignal::default());
        let quit_callback = Arc::downgrade(&signal);
        let command_callback = Arc::downgrade(&commands);
        let result = NativeMenu::install(move |action| {
            let cmd = match action {
                MenuAction::Quit => {
                    if let Some(signal) = quit_callback.upgrade() {
                        signal.request();
                    }
                    return;
                }
                MenuAction::About => MenuCommand::About,
                MenuAction::CheckForUpdates => MenuCommand::CheckForUpdates,
                MenuAction::Settings => MenuCommand::Settings,
                MenuAction::SelectBrowse => MenuCommand::SelectMode(Mode::Browse),
                MenuAction::SelectConfigure => MenuCommand::SelectMode(Mode::Configure),
                MenuAction::SelectLayout => MenuCommand::SelectMode(Mode::Layout),
            };
            if let Some(signal) = command_callback.upgrade() {
                signal.request(cmd);
            }
        });
        let (menu, error) = match result {
            Ok(menu) => (Some(menu), None),
            Err(error) => (
                None,
                Some(format!(
                    "Cannot install application menus: {error}. Close with the red button, relaunch, and collect application error output if this persists."
                )),
            ),
        };
        Self {
            menu,
            signal,
            commands,
            error,
        }
    }

    pub fn signal(&self) -> Arc<QuitSignal> {
        Arc::clone(&self.signal)
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn commands(&self) -> Arc<MenuSignal> {
        Arc::clone(&self.commands)
    }

    pub fn update(&self, state: MenuState) {
        let Some(menu) = &self.menu else {
            return;
        };
        let checked_mode = match state.mode {
            Mode::Browse => CheckedMode::Browse,
            Mode::Configure => CheckedMode::Configure,
            Mode::Layout => CheckedMode::Layout,
        };
        menu.update(NativeMenuState {
            about_enabled: state.about_enabled,
            check_updates_enabled: state.check_updates_enabled,
            settings_enabled: state.settings_enabled,
            modes_enabled: state.modes_enabled,
            checked_mode,
        });
    }
}

impl Drop for ApplicationMenu {
    fn drop(&mut self) {
        self.signal.stop();
        self.commands.stop();
    }
}
