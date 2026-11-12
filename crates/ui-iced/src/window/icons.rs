//! Embedded UI vector icons.

use std::sync::OnceLock;

use iced::Color;
use iced::widget::svg::{self, Handle, Svg};

use crate::application::contracts::directory::{DirectoryEntry, EntryKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Eye,
    Cog,
    Move,
    Back,
    Home,
    Folder,
    Document,
    Image,
    Sliders,
    Undo,
    Rename,
    Forward,
    Up,
    Close,
    External,
    Copy,
    Sort,
    Application,
    Link,
    Trash,
    Wrench,
    Github,
    Scissors,
    Info,
    Cloud,
    Drive,
    Network,
    Pin,
    Lock,
}

impl Icon {
    pub fn for_entry(entry: &DirectoryEntry) -> Self {
        if entry.kind == EntryKind::Directory {
            return Self::Folder;
        }
        let image = entry
            .path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                ["png", "jpg", "jpeg", "gif", "webp", "svg", "heic"]
                    .iter()
                    .any(|image| extension.eq_ignore_ascii_case(image))
            });
        if entry.kind == EntryKind::File && image {
            Self::Image
        } else {
            Self::Document
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            Self::Eye => include_bytes!("../../../../assets/icons/ui/eye.svg"),
            Self::Cog => include_bytes!("../../../../assets/icons/ui/cog.svg"),
            Self::Move => include_bytes!("../../../../assets/icons/ui/move.svg"),
            Self::Back => include_bytes!("../../../../assets/icons/ui/back.svg"),
            Self::Home => include_bytes!("../../../../assets/icons/ui/home.svg"),
            Self::Folder => include_bytes!("../../../../assets/icons/ui/folder.svg"),
            Self::Document => include_bytes!("../../../../assets/icons/ui/document.svg"),
            Self::Image => include_bytes!("../../../../assets/icons/ui/image.svg"),
            Self::Sliders => include_bytes!("../../../../assets/icons/ui/sliders.svg"),
            Self::Undo => include_bytes!("../../../../assets/icons/ui/undo.svg"),
            Self::Rename => include_bytes!("../../../../assets/icons/ui/rename.svg"),
            Self::Forward => include_bytes!("../../../../assets/icons/ui/forward.svg"),
            Self::Up => include_bytes!("../../../../assets/icons/ui/up.svg"),
            Self::Close => include_bytes!("../../../../assets/icons/ui/close.svg"),
            Self::External => include_bytes!("../../../../assets/icons/ui/external.svg"),
            Self::Copy => include_bytes!("../../../../assets/icons/ui/copy.svg"),
            Self::Sort => include_bytes!("../../../../assets/icons/ui/sort.svg"),
            Self::Application => include_bytes!("../../../../assets/icons/ui/application.svg"),
            Self::Link => include_bytes!("../../../../assets/icons/ui/link.svg"),
            Self::Trash => include_bytes!("../../../../assets/icons/ui/trash.svg"),
            Self::Wrench => include_bytes!("../../../../assets/icons/ui/wrench.svg"),
            Self::Github => include_bytes!("../../../../assets/icons/ui/github.svg"),
            Self::Scissors => include_bytes!("../../../../assets/icons/ui/scissors.svg"),
            Self::Info => include_bytes!("../../../../assets/icons/ui/info.svg"),
            Self::Cloud => include_bytes!("../../../../assets/icons/ui/cloud.svg"),
            Self::Drive => include_bytes!("../../../../assets/icons/ui/drive.svg"),
            Self::Network => include_bytes!("../../../../assets/icons/ui/network.svg"),
            Self::Pin => include_bytes!("../../../../assets/icons/ui/pin.svg"),
            Self::Lock => include_bytes!("../../../../assets/icons/ui/lock.svg"),
        }
    }

    fn handle(self) -> Handle {
        static HANDLES: OnceLock<[Handle; 29]> = OnceLock::new();
        HANDLES.get_or_init(|| {
            [
                Self::Eye,
                Self::Cog,
                Self::Move,
                Self::Back,
                Self::Home,
                Self::Folder,
                Self::Document,
                Self::Image,
                Self::Sliders,
                Self::Undo,
                Self::Rename,
                Self::Forward,
                Self::Up,
                Self::Close,
                Self::External,
                Self::Copy,
                Self::Sort,
                Self::Application,
                Self::Link,
                Self::Trash,
                Self::Wrench,
                Self::Github,
                Self::Scissors,
                Self::Info,
                Self::Cloud,
                Self::Drive,
                Self::Network,
                Self::Pin,
                Self::Lock,
            ]
            .map(|icon| Handle::from_memory(icon.bytes()))
        })[self as usize]
            .clone()
    }
}

pub fn view(icon: Icon, size: f32, color: Option<Color>, opacity: f32) -> Svg<'static> {
    svg::Svg::new(icon.handle())
        .width(size)
        .height(size)
        .opacity(opacity)
        .style(move |_, _| svg::Style { color })
}
