//! Application-owned native menu commands and their presentation permissions.
use crate::domain::Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuCommand {
    About,
    CheckForUpdates,
    Settings,
    SelectMode(Mode),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuState {
    pub about_enabled: bool,
    pub check_updates_enabled: bool,
    pub settings_enabled: bool,
    pub modes_enabled: bool,
    pub mode: Mode,
}

impl MenuState {
    pub fn permits(self, command: MenuCommand) -> bool {
        match command {
            MenuCommand::About => self.about_enabled,
            MenuCommand::CheckForUpdates => self.check_updates_enabled,
            MenuCommand::Settings => self.settings_enabled,
            MenuCommand::SelectMode(_) => self.modes_enabled,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MenuRequest {
    pub command: MenuCommand,
    pub epoch: u64,
}
