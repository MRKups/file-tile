//! Shortcut values and shared item geometry, with no native objects or effects.
use super::*;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherTarget {
    Path(PathBuf),
    Link(String),
}
impl LauncherTarget {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Path(path) => {
                let bytes = path.as_os_str().as_encoded_bytes();
                if !path.is_absolute() || bytes.len() > 4096 || bytes.contains(&0) {
                    return Err("Use an absolute path or ~/path, within 4,096 bytes.");
                }
            }
            Self::Link(link) => {
                let (scheme, address) = link
                    .split_once(':')
                    .ok_or("Use a link with a scheme, such as https://example.com.")?;
                if link.len() > 4096
                    || address.is_empty()
                    || link.chars().any(|c| c.is_whitespace() || c.is_control())
                    || !scheme
                        .as_bytes()
                        .first()
                        .is_some_and(u8::is_ascii_alphabetic)
                    || !scheme
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"+.-".contains(&c))
                {
                    return Err(
                        "Use a complete link without spaces or control characters, within 4,096 bytes.",
                    );
                }
                if [
                    "javascript",
                    "vbscript",
                    "data",
                    "file",
                    "shell",
                    "exec",
                    "command",
                ]
                .iter()
                .any(|value| scheme.eq_ignore_ascii_case(value))
                {
                    return Err(
                        "This link type is not supported. Use a file path or an ordinary web/app link, not a script or command.",
                    );
                }
                if (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
                    && !address
                        .strip_prefix("//")
                        .is_some_and(|value| !value.is_empty() && !value.starts_with('/'))
                {
                    return Err("Use a complete web address, such as https://example.com.");
                }
            }
        }
        Ok(())
    }
    pub fn from_input(input: &str, home: &Path) -> Result<Self, &'static str> {
        let target = if input == "~" {
            Self::Path(home.into())
        } else if let Some(rest) = input.strip_prefix("~/") {
            Self::Path(home.join(rest))
        } else if input.starts_with('/') {
            Self::Path(input.into())
        } else {
            Self::Link(input.into())
        };
        target.validate()?;
        Ok(target)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LauncherIcon {
    #[default]
    Automatic,
    Application,
    File,
    Folder,
    Link,
}
impl LauncherIcon {
    pub const ALL: [Self; 5] = [
        Self::Automatic,
        Self::Application,
        Self::File,
        Self::Folder,
        Self::Link,
    ];
}
impl std::fmt::Display for LauncherIcon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Automatic => "Automatic",
            Self::Application => "Application",
            Self::File => "File",
            Self::Folder => "Folder",
            Self::Link => "Link",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherConfig {
    pub name: String,
    pub target: Option<LauncherTarget>,
    pub icon: LauncherIcon,
}
impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            name: "Shortcut".into(),
            target: None,
            icon: LauncherIcon::Automatic,
        }
    }
}
impl LauncherConfig {
    pub fn validate_name(name: &str) -> Result<(), &'static str> {
        crate::domain::validation::validate_launcher_name(name)
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        Self::validate_name(&self.name)?;
        if let Some(target) = &self.target {
            target.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherTile {
    pub id: BoxId,
    pub rect: GridRect,
    pub config: LauncherConfig,
}
#[derive(Debug, Clone, Copy)]
pub struct ItemPlacement {
    pub id: BoxId,
    pub rect: GridRect,
}

impl Workspace {
    pub fn launchers(&self) -> &[LauncherTile] {
        &self.launchers
    }
    pub fn launcher(&self, id: BoxId) -> Option<&LauncherTile> {
        self.launchers.iter().find(|item| item.id == id)
    }
    pub fn placements(&self) -> impl DoubleEndedIterator<Item = ItemPlacement> + use<'_> {
        self.boxes
            .iter()
            .map(|item| ItemPlacement {
                id: item.id,
                rect: item.rect,
            })
            .chain(self.launchers.iter().map(|item| ItemPlacement {
                id: item.id,
                rect: item.rect,
            }))
    }
    pub fn item_rect(&self, id: BoxId) -> Option<GridRect> {
        self.placements()
            .find(|item| item.id == id)
            .map(|item| item.rect)
    }
    pub fn item_kind(&self, id: BoxId) -> Option<ItemKind> {
        if self.boxes.iter().any(|item| item.id == id) {
            Some(ItemKind::FolderBox)
        } else if self.launchers.iter().any(|item| item.id == id) {
            Some(ItemKind::Launcher)
        } else {
            None
        }
    }
    pub fn item_count(&self) -> usize {
        self.boxes.len() + self.launchers.len()
    }
    pub(super) fn set_item_rect(&mut self, id: BoxId, rect: GridRect) {
        if let Some(item) = self.boxes.iter_mut().find(|item| item.id == id) {
            item.rect = rect;
        }
        if let Some(item) = self.launchers.iter_mut().find(|item| item.id == id) {
            item.rect = rect;
        }
    }
    pub fn configure_launcher(&mut self, id: BoxId, config: LauncherConfig) -> bool {
        let browse_unconfigured = self.mode == Mode::Browse
            && self
                .launcher(id)
                .is_some_and(|item| item.config.target.is_none());
        if (!self.configuring(id) && !browse_unconfigured)
            || config.validate().is_err()
            || !self.launcher(id).is_some_and(|item| item.config != config)
        {
            return false;
        }
        self.remember();
        if let Some(item) = self.launchers.iter_mut().find(|item| item.id == id) {
            item.config = config;
        }
        true
    }
    pub(crate) fn restore_launcher(&mut self, item: LauncherTile) -> Result<(), &'static str> {
        item.config.validate()?;
        if self.item_count() >= Self::MAX_BOXES
            || item.id.0 == 0
            || self.item_rect(item.id).is_some()
        {
            return Err(
                "launcher identity is invalid, duplicate, or exceeds the shared item limit",
            );
        }
        let rect = item.rect;
        if !shape::is_launcher_shape(rect)
            || u32::from(rect.origin.column) + u32::from(rect.width)
                > u32::from(GridExtent::MAX_DIMENSION)
            || u32::from(rect.origin.row) + u32::from(rect.height)
                > u32::from(GridExtent::MAX_DIMENSION)
            || self.placements().any(|other| other.rect.intersects(rect))
        {
            return Err("launcher bounds are invalid or overlap an earlier item");
        }
        self.next_identity = self.next_identity.max(item.id.0);
        self.launchers.push(item);
        Ok(())
    }
    pub(super) fn validate_launchers(&self) -> Result<(), &'static str> {
        let mut prior = Self {
            boxes: self.boxes.clone(),
            ..Self::default()
        };
        for item in &self.launchers {
            prior.restore_launcher(item.clone())?;
        }
        Ok(())
    }
}
