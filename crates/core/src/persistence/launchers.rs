//! Strict launcher records.
use super::codec::BoundsRecord;
use super::path::{PathRecord, PathRef};
use crate::domain::{BoxId, LauncherConfig, LauncherIcon, LauncherTarget, LauncherTile};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Icon {
    Automatic,
    Application,
    File,
    Folder,
    Link,
}
impl From<LauncherIcon> for Icon {
    fn from(icon: LauncherIcon) -> Self {
        match icon {
            LauncherIcon::Automatic => Self::Automatic,
            LauncherIcon::Application => Self::Application,
            LauncherIcon::File => Self::File,
            LauncherIcon::Folder => Self::Folder,
            LauncherIcon::Link => Self::Link,
        }
    }
}
impl From<Icon> for LauncherIcon {
    fn from(icon: Icon) -> Self {
        match icon {
            Icon::Automatic => Self::Automatic,
            Icon::Application => Self::Application,
            Icon::File => Self::File,
            Icon::Folder => Self::Folder,
            Icon::Link => Self::Link,
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Target {
    Path { path: PathRecord },
    Link { url: String },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    id: u64,
    bounds: BoundsRecord,
    name: String,
    target: Option<Target>,
    icon: Icon,
}

pub(super) fn restore(value: Value) -> Result<LauncherTile, &'static str> {
    if value.get("target").is_none() {
        return Err("launcher target field is missing");
    }
    let record: Record = serde_json::from_value(value)
        .map_err(|_| "launcher record has invalid or unknown fields")?;
    let target = record
        .target
        .map(|target| match target {
            Target::Path { path } => path.into_path().map(LauncherTarget::Path),
            Target::Link { url } => Ok(LauncherTarget::Link(url)),
        })
        .transpose()?;
    let config = LauncherConfig {
        name: record.name,
        target,
        icon: record.icon.into(),
    };
    config.validate()?;
    Ok(LauncherTile {
        id: BoxId(record.id),
        rect: record.bounds.into(),
        config,
    })
}

pub(super) struct Launchers<'a>(pub &'a [LauncherTile]);
impl Serialize for Launchers<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(Reference))
    }
}
struct Reference<'a>(&'a LauncherTile);
impl Serialize for Reference<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(tag = "kind", rename_all = "snake_case")]
        enum TargetRef<'a> {
            Path { path: PathRef<'a> },
            Link { url: &'a str },
        }
        #[derive(Serialize)]
        struct Fields<'a> {
            id: u64,
            bounds: BoundsRecord,
            name: &'a str,
            target: Option<TargetRef<'a>>,
            icon: Icon,
        }
        let item = self.0;
        Fields {
            id: item.id.0,
            bounds: item.rect.into(),
            name: &item.config.name,
            target: item.config.target.as_ref().map(|target| match target {
                LauncherTarget::Path(path) => TargetRef::Path {
                    path: PathRef(path),
                },
                LauncherTarget::Link(url) => TargetRef::Link { url },
            }),
            icon: item.config.icon.into(),
        }
        .serialize(serializer)
    }
}
