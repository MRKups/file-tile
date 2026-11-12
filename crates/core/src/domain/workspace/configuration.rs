//! Box settings and editing permissions, without filesystem or UI state.

use super::{BoxId, Mode, ViewId, Workspace};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoxSettings {
    pub sort: crate::domain::sorting::SortSettings,
    pub show_forward: bool,
    pub show_up: bool,
    pub show_start: bool,
    pub show_hidden: bool,
    pub show_sizes: bool,
    pub show_dates: bool,
    pub prohibit_delete: bool,
    pub file_view: FileView,
    pub sidebar: bool,
}

impl Default for BoxSettings {
    fn default() -> Self {
        Self {
            sort: crate::domain::sorting::SortSettings::default(),
            show_forward: false,
            show_up: false,
            show_start: true,
            show_hidden: false,
            show_sizes: false,
            show_dates: false,
            prohibit_delete: false,
            file_view: FileView::List,
            sidebar: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LeftPanel {
    #[default]
    None,
    Sidebar,
    VerticalTabs,
}
impl LeftPanel {
    pub const ALL: [Self; 3] = [Self::None, Self::Sidebar, Self::VerticalTabs];
}
impl std::fmt::Display for LeftPanel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::None => "None",
            Self::Sidebar => "Sidebar",
            Self::VerticalTabs => "Vertical tabs",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxSetting {
    Forward(bool),
    Up(bool),
    ReturnToStart(bool),
    HiddenFiles(bool),
    FileSizes(bool),
    ModifiedDates(bool),
    ProhibitDelete(bool),
    FileView(FileView),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FileView {
    #[default]
    List,
    Grid,
    Columns,
}
impl FileView {
    pub const ALL: [Self; 3] = [Self::List, Self::Grid, Self::Columns];
}
impl std::fmt::Display for FileView {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::List => "List",
            Self::Grid => "Grid",
            Self::Columns => "Columns",
        })
    }
}

impl Workspace {
    pub fn change_sort(
        &mut self,
        view: ViewId,
        sort: crate::domain::sorting::SortSettings,
    ) -> bool {
        if !self.can_browse(view) {
            return false;
        }
        let Some(index) = self
            .boxes
            .iter()
            .position(|item| item.active_view().view == view)
        else {
            return false;
        };
        if self.boxes[index].settings.sort == sort {
            return false;
        }
        self.remember();
        self.boxes[index].settings.sort = sort;
        true
    }
    pub fn configuring(&self, id: BoxId) -> bool {
        self.mode == Mode::Configure && self.selected == Some(id)
    }

    pub fn can_browse(&self, view: ViewId) -> bool {
        self.view(view).is_some_and(|item| {
            self.is_active(view) && (self.mode == Mode::Browse || self.configuring(item.id))
        })
    }

    /// Outside clicks leave editing without choosing another box in the same click.
    pub fn configure_box(&mut self, id: Option<BoxId>) {
        if self.mode != Mode::Configure {
            return;
        }
        self.selected = if self.selected.is_some() {
            None
        } else {
            id.filter(|id| self.item_rect(*id).is_some())
        };
    }

    pub fn change_box_setting(&mut self, id: BoxId, setting: BoxSetting) -> bool {
        let permitted = if matches!(setting, BoxSetting::HiddenFiles(_)) {
            self.boxes
                .iter()
                .find(|item| item.id == id)
                .is_some_and(|item| self.can_browse(item.active_view().view))
        } else {
            self.configuring(id)
        };
        if !permitted {
            return false;
        }
        let Some(index) = self.boxes.iter().position(|item| item.id == id) else {
            return false;
        };
        let mut settings = self.boxes[index].settings;
        if settings.file_view == FileView::Grid
            && matches!(
                setting,
                BoxSetting::FileSizes(_) | BoxSetting::ModifiedDates(_)
            )
        {
            return false;
        }
        match setting {
            BoxSetting::Forward(value) => settings.show_forward = value,
            BoxSetting::Up(value) => settings.show_up = value,
            BoxSetting::ReturnToStart(value) => settings.show_start = value,
            BoxSetting::HiddenFiles(value) => settings.show_hidden = value,
            BoxSetting::FileSizes(value) => settings.show_sizes = value,
            BoxSetting::ModifiedDates(value) => settings.show_dates = value,
            BoxSetting::ProhibitDelete(value) => settings.prohibit_delete = value,
            BoxSetting::FileView(value) => settings.file_view = value,
        }
        if settings == self.boxes[index].settings {
            return false;
        }
        self.remember();
        self.boxes[index].settings = settings;
        true
    }
}
