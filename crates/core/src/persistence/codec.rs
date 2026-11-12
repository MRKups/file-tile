use std::collections::{HashSet, VecDeque};
use std::io::{self, Write};
use std::path::PathBuf;

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::application::contracts::configuration::{
    AppPreferences, ConfigurationDocument, WorkspaceDocument, WorkspaceId, WorkspaceIndex,
    WorkspaceSummary,
};
use crate::domain::{
    BoxId, BoxSettings, Cell, FolderBox, FolderView, GridRect, Navigation, Tabs, ViewId, Workspace,
};

use super::path::{PathRecord, PathRef};
use super::unique_value::UniqueValue;
use super::{ConfigError, ConfigKey, Decoded, FORMAT, RecoveryIssue, SCHEMA_VERSION};

pub(super) fn decode_header(
    bytes: &[u8],
    limit: usize,
    expected_kind: &str,
) -> Result<Value, ConfigError> {
    if bytes.len() > limit {
        return Err(ConfigError::TooLarge);
    }
    let UniqueValue(value) = serde_json::from_slice(bytes).map_err(|_| ConfigError::InvalidJson)?;
    let Value::Object(mut fields) = value else {
        return Err(ConfigError::InvalidHeader);
    };
    let format = fields.remove("format").ok_or(ConfigError::InvalidHeader)?;
    if format.as_str() != Some(FORMAT) {
        return Err(if format.is_string() {
            ConfigError::UnsupportedFormat
        } else {
            ConfigError::InvalidHeader
        });
    }
    let version = fields
        .remove("schema_version")
        .and_then(|value| value.as_u64())
        .ok_or(ConfigError::InvalidHeader)?;
    if version == 0 || version > u64::from(SCHEMA_VERSION) {
        return Err(ConfigError::UnsupportedVersion(version));
    }
    let kind = fields.remove("kind").ok_or(ConfigError::InvalidHeader)?;
    if kind.as_str() != Some(expected_kind) {
        return Err(if kind.is_string() {
            ConfigError::WrongDocument
        } else {
            ConfigError::InvalidHeader
        });
    }
    super::migrations::upgrade(version, expected_kind, Value::Object(fields))
}

pub(super) fn decode(bytes: &[u8], key: ConfigKey) -> Result<Decoded, ConfigError> {
    let payload = decode_header(bytes, key.byte_limit(), key.kind())?;
    let mut issues = Vec::new();
    let document = match key {
        ConfigKey::Preferences => preferences(payload, &mut issues)?,
        ConfigKey::WorkspaceIndex => index(payload, &mut issues)?,
        ConfigKey::Workspace(id) => workspace(payload, id, &mut issues)?,
    };
    Ok(Decoded { document, issues })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreferencesRecord {
    last_workspace_id: Option<String>,
    window: Option<Value>,
    new_box_folder: Option<Value>,
    #[serde(default, deserialize_with = "deserialize_present_value")]
    pinned_folders: Option<Value>,
    appearance: Value,
    typography: Value,
    safety: Value,
    #[serde(default)]
    capabilities: Option<Value>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CapabilitiesRecord {
    git: bool,
    languages: bool,
    env_files: bool,
}

impl From<crate::domain::CapabilityPreferences> for CapabilitiesRecord {
    fn from(capabilities: crate::domain::CapabilityPreferences) -> Self {
        Self {
            git: capabilities.git,
            languages: capabilities.languages,
            env_files: capabilities.env_files,
        }
    }
}

#[derive(Serialize)]
struct SafetyRecord<'a> {
    desktop_access: bool,
    documents_access: bool,
    downloads_access: bool,
    full_disk_access: bool,
    external_drives_access: bool,
    network_shares_access: bool,
    #[serde(skip_serializing_if = "PathSet::is_empty")]
    disabled_locations: PathSet<'a>,
    system_folders: bool,
    applications: bool,
    credentials: bool,
}

impl<'a> From<&'a crate::domain::SafetyPreferences> for SafetyRecord<'a> {
    fn from(safety: &'a crate::domain::SafetyPreferences) -> Self {
        Self {
            desktop_access: safety.desktop_access,
            documents_access: safety.documents_access,
            downloads_access: safety.downloads_access,
            full_disk_access: safety.full_disk_access,
            external_drives_access: safety.external_drives_access,
            network_shares_access: safety.network_shares_access,
            disabled_locations: PathSet(&safety.disabled_locations),
            system_folders: safety.system_folders,
            applications: safety.applications,
            credentials: safety.credentials,
        }
    }
}

struct PathSet<'a>(&'a std::collections::BTreeSet<PathBuf>);

impl PathSet<'_> {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for PathSet<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|path| PathRef(path)))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WindowRecord {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn restore_window_placement(
    window: Option<Value>,
    issues: &mut Vec<RecoveryIssue>,
) -> Option<crate::domain::WindowPlacement> {
    window.and_then(|value| {
        let placement = serde_json::from_value::<WindowRecord>(value)
            .ok()
            .map(|record| crate::domain::WindowPlacement {
                x: record.x,
                y: record.y,
                width: record.width,
                height: record.height,
            })
            .filter(|placement| placement.valid());
        if placement.is_none() {
            issue(
                issues,
                0,
                "invalid window bounds; using default placement for viewing only",
            );
        }
        placement
    })
}

fn restore_new_box_folder(
    new_box_folder: Option<Value>,
    issues: &mut Vec<RecoveryIssue>,
) -> Option<PathBuf> {
    new_box_folder.and_then(|value| {
        let path = serde_json::from_value::<PathRecord>(value)
            .ok()
            .and_then(|record| record.into_path().ok());
        if path.is_none() {
            issue(
                issues,
                0,
                "invalid starting folder; using Home for viewing only",
            );
        }
        path
    })
}

fn preferences(
    payload: Value,
    issues: &mut Vec<RecoveryIssue>,
) -> Result<ConfigurationDocument, ConfigError> {
    // Serde's Option accepts missing fields. Require explicit keys on every
    // preferences document.
    if [
        "last_workspace_id",
        "window",
        "new_box_folder",
        "appearance",
        "typography",
        "safety",
    ]
    .iter()
    .any(|key| payload.get(key).is_none())
    {
        return Err(ConfigError::InvalidPayload);
    }
    let record: PreferencesRecord = deserialize(payload)?;
    let last_workspace_id = record
        .last_workspace_id
        .map(|value| WorkspaceId::parse(&value).map_err(|_| ConfigError::InvalidPayload))
        .transpose()?;
    let window = restore_window_placement(record.window, issues);
    let new_box_folder = restore_new_box_folder(record.new_box_folder, issues);
    let (pinned_folders, _pinned_healed) =
        super::healing::preferences_fields::restore_pinned_folders(record.pinned_folders);
    let appearance = super::appearance::restore(record.appearance).unwrap_or_else(|| {
        issue(
            issues,
            0,
            "invalid appearance; using default opaque backgrounds for viewing only",
        );
        crate::application::preferences::appearance::AppearancePreferences {
            transparency: crate::application::preferences::appearance::Percent::OPAQUE,
            box_transparency: crate::application::preferences::appearance::BoxPercent::OPAQUE,
            ..crate::application::preferences::appearance::AppearancePreferences::default()
        }
    });
    let typography = super::typography::restore(record.typography).unwrap_or_else(|| {
        issue(
            issues,
            0,
            "invalid typography; using default text for viewing only",
        );
        crate::application::preferences::typography::TypographyPreferences::default()
    });
    let (safety, _safety_healed) =
        super::healing::preferences_fields::restore_safety(record.safety);
    let (capabilities, _capabilities_healed) =
        super::healing::preferences_fields::restore_capabilities(record.capabilities);
    Ok(ConfigurationDocument::Preferences(AppPreferences {
        last_workspace_id,
        window,
        new_box_folder,
        pinned_folders,
        appearance,
        typography,
        safety,
        capabilities,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexRecord {
    workspaces: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SummaryRecord {
    id: String,
    name: String,
}

fn index(
    payload: Value,
    issues: &mut Vec<RecoveryIssue>,
) -> Result<ConfigurationDocument, ConfigError> {
    let record: IndexRecord = deserialize(payload)?;
    if record.workspaces.len() > WorkspaceIndex::MAX_WORKSPACES {
        return Err(ConfigError::LimitExceeded);
    }
    let mut workspaces = Vec::new();
    let mut ids = HashSet::new();
    for (position, value) in record.workspaces.into_iter().enumerate() {
        match summary(value) {
            Ok(summary) if ids.insert(summary.id) => workspaces.push(summary),
            Ok(_) => issue(issues, position, "duplicate workspace UUID"),
            Err(reason) => issue(issues, position, reason),
        }
    }
    Ok(ConfigurationDocument::WorkspaceIndex(WorkspaceIndex {
        workspaces,
    }))
}

fn summary(value: Value) -> Result<WorkspaceSummary, &'static str> {
    let record: SummaryRecord = serde_json::from_value(value)
        .map_err(|_| "workspace index record has invalid or unknown fields")?;
    let id = WorkspaceId::parse(&record.id).map_err(|_| "invalid workspace UUID")?;
    if !WorkspaceIndex::valid_name(&record.name) {
        return Err(
            "workspace name must be 1 to 32 letters, numbers, single spaces, hyphens, or underscores without leading or trailing spaces",
        );
    }
    Ok(WorkspaceSummary {
        id,
        name: record.name,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceRecord {
    id: String,
    boxes: Vec<Value>,
    launchers: Vec<Value>,
}

fn workspace(
    payload: Value,
    expected: WorkspaceId,
    issues: &mut Vec<RecoveryIssue>,
) -> Result<ConfigurationDocument, ConfigError> {
    let record: WorkspaceRecord = deserialize(payload)?;
    let id = WorkspaceId::parse(&record.id).map_err(|_| ConfigError::InvalidPayload)?;
    if id != expected {
        return Err(ConfigError::WrongDocument);
    }
    if record.boxes.len() + record.launchers.len() > Workspace::MAX_BOXES {
        return Err(ConfigError::LimitExceeded);
    }
    let mut workspace = Workspace::default();
    let box_count = record.boxes.len();
    for (position, value) in record.boxes.into_iter().enumerate() {
        match restore_box(value, position, issues) {
            Ok(item) => {
                if let Err(reason) = workspace.restore_box(item) {
                    issue(issues, position, reason);
                }
            }
            Err(reason) => issue(issues, position, reason),
        }
    }
    for (offset, value) in record.launchers.into_iter().enumerate() {
        let result =
            super::launchers::restore(value).and_then(|item| workspace.restore_launcher(item));
        if let Err(reason) = result {
            issue(issues, box_count + offset, reason);
        }
    }
    Ok(ConfigurationDocument::Workspace(WorkspaceDocument {
        id,
        workspace,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoxRecord {
    id: u64,
    bounds: BoundsRecord,
    standalone_view: ViewRecord,
    settings: SettingsDecodeRecord,
    tabs: Option<TabsRecord>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TabsRecord {
    visible: bool,
    active_view: u64,
    views: Vec<ViewRecord>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsDecodeRecord {
    sort: super::sorting::SortRecord,
    show_forward: bool,
    show_up: bool,
    show_start: bool,
    show_hidden: bool,
    show_sizes: bool,
    show_dates: bool,
    prohibit_delete: bool,
    file_view: FileViewRecord,
    #[serde(default, deserialize_with = "deserialize_present_value")]
    sidebar: Option<Value>,
}

#[derive(Serialize)]
struct SettingsRecord {
    sort: super::sorting::SortRecord,
    show_forward: bool,
    show_up: bool,
    show_start: bool,
    show_hidden: bool,
    show_sizes: bool,
    show_dates: bool,
    prohibit_delete: bool,
    file_view: FileViewRecord,
    sidebar: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum FileViewRecord {
    List,
    Grid,
    Columns,
}

impl From<BoxSettings> for SettingsRecord {
    fn from(settings: BoxSettings) -> Self {
        Self {
            sort: settings.sort.into(),
            show_forward: settings.show_forward,
            show_up: settings.show_up,
            show_start: settings.show_start,
            show_hidden: settings.show_hidden,
            show_sizes: settings.show_sizes,
            show_dates: settings.show_dates,
            prohibit_delete: settings.prohibit_delete,
            file_view: match settings.file_view {
                crate::domain::FileView::List => FileViewRecord::List,
                crate::domain::FileView::Grid => FileViewRecord::Grid,
                crate::domain::FileView::Columns => FileViewRecord::Columns,
            },
            sidebar: settings.sidebar,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BoundsRecord {
    column: u16,
    row: u16,
    width: u16,
    height: u16,
}

impl From<GridRect> for BoundsRecord {
    fn from(rect: GridRect) -> Self {
        Self {
            column: rect.origin.column,
            row: rect.origin.row,
            width: rect.width,
            height: rect.height,
        }
    }
}

impl From<BoundsRecord> for GridRect {
    fn from(bounds: BoundsRecord) -> Self {
        Self {
            origin: Cell::new(bounds.column, bounds.row),
            width: bounds.width,
            height: bounds.height,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewRecord {
    id: u64,
    starting_folder: PathRecord,
    current_folder: PathRecord,
    scope: Value,
    history: HistoryRecord,
    #[serde(default, deserialize_with = "deserialize_present_value")]
    top_item: Option<Value>,
}

fn deserialize_present_value<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Value::deserialize(deserializer).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryRecord {
    back: Vec<Value>,
    forward: Vec<Value>,
}

fn restore_box(
    value: Value,
    position: usize,
    issues: &mut Vec<RecoveryIssue>,
) -> Result<FolderBox, &'static str> {
    let fields = value.as_object().ok_or("box record is not an object")?;
    if !fields.contains_key("tabs") {
        return Err("box record is missing tab state");
    }
    let record: BoxRecord = serde_json::from_value(value)
        .map_err(|_| "box record has invalid, unknown, or unsupported fields")?;
    let standalone = restore_view(record.standalone_view, position, issues)?;
    let tabs = record
        .tabs
        .map(|tabs| {
            if tabs.views.len() > Workspace::MAX_TABS {
                return Err("box tab limit exceeded");
            }
            let views = tabs
                .views
                .into_iter()
                .map(|view| restore_view(view, position, issues))
                .collect::<Result<Vec<_>, _>>()?;
            Tabs::restore(views, ViewId(tabs.active_view), tabs.visible)
        })
        .transpose()?;
    let tabs_visible = tabs.as_ref().is_some_and(Tabs::visible);
    let (sidebar, _sidebar_healed) =
        super::healing::view_fields::restore_box_sidebar(record.settings.sidebar, tabs_visible);
    Ok(FolderBox {
        id: BoxId(record.id),
        rect: record.bounds.into(),
        standalone,
        tabs,
        settings: BoxSettings {
            sort: record.settings.sort.into(),
            show_forward: record.settings.show_forward,
            show_up: record.settings.show_up,
            show_start: record.settings.show_start,
            show_hidden: record.settings.show_hidden,
            show_sizes: record.settings.show_sizes,
            show_dates: record.settings.show_dates,
            prohibit_delete: record.settings.prohibit_delete,
            file_view: match record.settings.file_view {
                FileViewRecord::List => crate::domain::FileView::List,
                FileViewRecord::Grid => crate::domain::FileView::Grid,
                FileViewRecord::Columns => crate::domain::FileView::Columns,
            },
            sidebar,
        },
    })
}

fn restore_view(
    view: ViewRecord,
    position: usize,
    issues: &mut Vec<RecoveryIssue>,
) -> Result<FolderView, &'static str> {
    let scope = super::scope::restore(view.scope)?;
    let start = view.starting_folder.into_path()?;
    let current = view.current_folder.into_path()?;
    if matches!(
        scope.kind(),
        crate::domain::ScopeKind::Pinned | crate::domain::ScopeKind::Branch
    ) && !scope.allows(&start)
    {
        return Err("stored starting folder is outside its fixed scope");
    }
    if !scope.allows(&current) {
        return Err("stored current folder is outside its restrictive scope");
    }
    let mut back = history(view.history.back, position, "Back history", issues);
    let mut forward = history(view.history.forward, position, "Forward history", issues);
    for paths in [&mut back, &mut forward] {
        if paths.iter().any(|path| !scope.allows(path)) {
            issue(
                issues,
                position,
                "history contains folders outside the restrictive scope",
            );
            paths.retain(|path| scope.allows(path));
        }
    }
    let navigation = Navigation::restore(start, current, back, forward)
        .ok_or("navigation history exceeds its limit")?
        .with_scope(scope);
    let (top_item, _top_item_healed) = super::healing::view_fields::restore_top_item(view.top_item);
    Ok(FolderView {
        view: ViewId(view.id),
        navigation,
        top_item,
    })
}

fn history(
    values: Vec<Value>,
    position: usize,
    direction: &str,
    issues: &mut Vec<RecoveryIssue>,
) -> VecDeque<PathBuf> {
    let skip = values.len().saturating_sub(Navigation::HISTORY_LIMIT);
    if skip > 0 {
        issue(
            issues,
            position,
            &format!("{direction} exceeds its limit; viewing copy keeps the most recent entries"),
        );
    }
    let mut paths = VecDeque::new();
    for value in values.into_iter().skip(skip) {
        let path = serde_json::from_value::<PathRecord>(value)
            .map_err(|_| "history entry has invalid or unknown fields")
            .and_then(PathRecord::into_path);
        match path {
            Ok(path) => paths.push_back(path),
            Err(reason) => issue(issues, position, &format!("{direction}: {reason}")),
        }
    }
    paths
}

fn deserialize<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, ConfigError> {
    serde_json::from_value(value).map_err(|_| ConfigError::InvalidPayload)
}

fn issue(issues: &mut Vec<RecoveryIssue>, record_index: usize, reason: &str) {
    issues.push(RecoveryIssue {
        record_index,
        reason: reason.into(),
    });
}

pub fn encode(document: &ConfigurationDocument) -> Result<Vec<u8>, ConfigError> {
    validate_document(document)?;
    let key = match document {
        ConfigurationDocument::Preferences(_) => ConfigKey::Preferences,
        ConfigurationDocument::WorkspaceIndex(_) => ConfigKey::WorkspaceIndex,
        ConfigurationDocument::Workspace(record) => ConfigKey::Workspace(record.id),
    };
    let mut writer = LimitedWriter {
        bytes: Vec::new(),
        limit: key.byte_limit(),
        exceeded: false,
    };
    let result = serde_json::to_writer_pretty(&mut writer, &DocumentRef { document, key });
    if writer.exceeded {
        return Err(ConfigError::TooLarge);
    }
    result.map_err(|_| ConfigError::InvalidPayload)?;
    writer.write_all(b"\n").map_err(|_| ConfigError::TooLarge)?;
    Ok(writer.bytes)
}

fn validate_document(document: &ConfigurationDocument) -> Result<(), ConfigError> {
    match document {
        ConfigurationDocument::Preferences(preferences) => {
            if !preferences.typography.is_valid() {
                return Err(ConfigError::InvalidState(
                    "font weight is not supported by the selected family".into(),
                ));
            }
            if preferences
                .window
                .is_some_and(|placement| !placement.valid())
            {
                return Err(ConfigError::InvalidState(
                    "window dimensions must be positive".into(),
                ));
            }
            if preferences.pinned_folders.as_ref().is_some_and(|folders| {
                folders.len() > crate::application::contracts::configuration::MAX_PINNED_FOLDERS
            }) || preferences.safety.disabled_locations.len()
                > crate::domain::MAX_DISABLED_LOCATIONS
            {
                return Err(ConfigError::LimitExceeded);
            }
        }
        ConfigurationDocument::WorkspaceIndex(index) => {
            if index.workspaces.len() > WorkspaceIndex::MAX_WORKSPACES {
                return Err(ConfigError::LimitExceeded);
            }
            let mut ids = HashSet::new();
            for workspace in &index.workspaces {
                if !WorkspaceIndex::valid_name(&workspace.name) || !ids.insert(workspace.id) {
                    return Err(ConfigError::InvalidState(
                        "workspace names or identities are invalid".into(),
                    ));
                }
            }
        }
        ConfigurationDocument::Workspace(record) => {
            record
                .workspace
                .validate_saved()
                .map_err(|reason| ConfigError::InvalidState(reason.into()))?;
        }
    }
    // The domain guards identities, geometry, and count. Paths are
    // validated as they stream into the bounded writer, without cloning histories.
    Ok(())
}

struct DocumentRef<'a> {
    document: &'a ConfigurationDocument,
    key: ConfigKey,
}

impl Serialize for DocumentRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("format", FORMAT)?;
        map.serialize_entry("schema_version", &SCHEMA_VERSION)?;
        map.serialize_entry("kind", self.key.kind())?;
        match self.document {
            ConfigurationDocument::Preferences(preferences) => {
                map.serialize_entry(
                    "last_workspace_id",
                    &preferences.last_workspace_id.map(|id| id.to_string()),
                )?;
                map.serialize_entry(
                    "window",
                    &preferences.window.map(|placement| WindowRecord {
                        x: placement.x,
                        y: placement.y,
                        width: placement.width,
                        height: placement.height,
                    }),
                )?;
                map.serialize_entry(
                    "new_box_folder",
                    &preferences.new_box_folder.as_deref().map(PathRef),
                )?;
                map.serialize_entry(
                    "pinned_folders",
                    &preferences.pinned_folders.as_deref().map(PathSlice),
                )?;
                map.serialize_entry(
                    "appearance",
                    &super::appearance::AppearanceRecord::from(preferences.appearance),
                )?;
                map.serialize_entry(
                    "typography",
                    &super::typography::TypographyRecord::from(preferences.typography),
                )?;
                map.serialize_entry("safety", &SafetyRecord::from(&preferences.safety))?;
                map.serialize_entry(
                    "capabilities",
                    &CapabilitiesRecord::from(preferences.capabilities),
                )?;
            }
            ConfigurationDocument::WorkspaceIndex(index) => {
                map.serialize_entry("workspaces", &Summaries(&index.workspaces))?;
            }
            ConfigurationDocument::Workspace(record) => {
                map.serialize_entry("id", &record.id.to_string())?;
                map.serialize_entry("boxes", &Boxes(record.workspace.boxes()))?;
                map.serialize_entry(
                    "launchers",
                    &super::launchers::Launchers(record.workspace.launchers()),
                )?;
            }
        }
        map.end()
    }
}

struct Summaries<'a>(&'a [WorkspaceSummary]);

impl Serialize for Summaries<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Summary<'a> {
            id: String,
            name: &'a str,
        }
        serializer.collect_seq(self.0.iter().map(|summary| Summary {
            id: summary.id.to_string(),
            name: &summary.name,
        }))
    }
}

struct Boxes<'a>(&'a [FolderBox]);

impl Serialize for Boxes<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(BoxRef))
    }
}

struct BoxRef<'a>(&'a FolderBox);

impl Serialize for BoxRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct BoxFields<'a> {
            id: u64,
            bounds: BoundsRecord,
            standalone_view: ViewRef<'a>,
            settings: SettingsRecord,
            tabs: Option<TabsRef<'a>>,
        }
        let item = self.0;
        BoxFields {
            id: item.id.0,
            settings: item.settings.into(),
            tabs: item.tabs.as_ref().map(TabsRef),
            bounds: item.rect.into(),
            standalone_view: ViewRef(&item.standalone),
        }
        .serialize(serializer)
    }
}

struct ViewRef<'a>(&'a FolderView);
impl Serialize for ViewRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct History<'a> {
            back: Paths<'a>,
            forward: Paths<'a>,
        }
        #[derive(Serialize)]
        struct View<'a> {
            id: u64,
            starting_folder: PathRef<'a>,
            current_folder: PathRef<'a>,
            scope: super::scope::ScopeRef<'a>,
            history: History<'a>,
            top_item: u32,
        }
        let item = self.0;
        View {
            id: item.view.0,
            starting_folder: PathRef(item.navigation.start()),
            current_folder: PathRef(item.navigation.current()),
            scope: super::scope::ScopeRef(item.navigation.scope()),
            history: History {
                back: Paths(item.navigation.back_history()),
                forward: Paths(item.navigation.forward_history()),
            },
            top_item: item.top_item,
        }
        .serialize(serializer)
    }
}

struct TabsRef<'a>(&'a Tabs);
impl Serialize for TabsRef<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("visible", &self.0.visible())?;
        map.serialize_entry("active_view", &self.0.active_view().view.0)?;
        map.serialize_entry("views", &Views(self.0.views()))?;
        map.end()
    }
}
struct Views<'a>(&'a [FolderView]);
impl Serialize for Views<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(ViewRef))
    }
}

struct Paths<'a>(&'a VecDeque<PathBuf>);

impl Serialize for Paths<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|path| PathRef(path)))
    }
}

struct PathSlice<'a>(&'a [PathBuf]);

impl Serialize for PathSlice<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(|path| PathRef(path)))
    }
}

struct LimitedWriter {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "configuration byte limit exceeded",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
