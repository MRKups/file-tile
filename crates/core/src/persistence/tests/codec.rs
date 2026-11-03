use std::collections::VecDeque;

use serde_json::{Value, json};

use crate::application::contracts::configuration::{
    AppPreferences, ConfigurationDocument, WorkspaceDocument, WorkspaceId, WorkspaceIndex,
    WorkspaceSummary,
};
use crate::domain::{BoxId, Cell, FolderBox, GridRect, Navigation, ViewId, Workspace};
use crate::persistence::*;

const ID: &str = "11111111-1111-4111-8111-111111111111";
const OTHER_ID: &str = "22222222-2222-4222-8222-222222222222";
const EXAMPLE: &str = include_str!(
    "../../../../../schemas/examples/workspace-11111111-1111-4111-8111-111111111111.json"
);

fn id() -> Result<WorkspaceId, Box<dyn std::error::Error>> {
    Ok(WorkspaceId::parse(ID)?)
}

fn example() -> Result<Value, serde_json::Error> {
    serde_json::from_str(EXAMPLE)
}

fn from_value(value: &Value) -> Result<Decoded, ConfigError> {
    let bytes = serde_json::to_vec(value).map_err(|_| ConfigError::InvalidJson)?;
    let id = WorkspaceId::parse(ID).map_err(|_| ConfigError::InvalidPayload)?;
    decode(&bytes, ConfigKey::Workspace(id))
}

fn restored(decoded: &Decoded) -> Result<&Workspace, Box<dyn std::error::Error>> {
    match &decoded.document {
        ConfigurationDocument::Workspace(record) => Ok(&record.workspace),
        _ => Err("Expected a workspace document".into()),
    }
}

#[test]
fn window_preferences_round_trip_and_invalid_bounds_are_read_only()
-> Result<(), Box<dyn std::error::Error>> {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../../../../schemas/examples/preferences.json"
    ))?;
    value["window"] = json!({"x": -900, "y": 80, "width": 960, "height": 640});
    let decoded = decode(&serde_json::to_vec(&value)?, ConfigKey::Preferences)?;
    assert!(decoded.can_save());
    let encoded = encode(&decoded.document)?;
    assert_eq!(serde_json::from_slice::<Value>(&encoded)?, value);
    for invalid in [
        json!({"x": 0, "y": 0, "width": 0, "height": 640}),
        json!({"x": 0, "y": 0, "width": 960, "height": 640, "unknown": true}),
        json!("bad"),
    ] {
        value["window"] = invalid;
        let decoded = decode(&serde_json::to_vec(&value)?, ConfigKey::Preferences)?;
        assert!(!decoded.can_save());
        assert!(matches!(
            decoded.document,
            ConfigurationDocument::Preferences(AppPreferences { window: None, .. })
        ));
    }
    value
        .as_object_mut()
        .ok_or("Expected preferences object")?
        .remove("window");
    assert!(matches!(
        decode(&serde_json::to_vec(&value)?, ConfigKey::Preferences),
        Err(ConfigError::InvalidPayload)
    ));
    Ok(())
}

#[test]
fn app_settings_paths_round_trip_and_invalid_defaults_cannot_authorize_autosave()
-> Result<(), Box<dyn std::error::Error>> {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../../../../schemas/examples/preferences.json"
    ))?;
    value["new_box_folder"] = json!({ "encoding": "utf8", "value": "/fixture/link/../preferred" });
    let decoded = decode(&serde_json::to_vec(&value)?, ConfigKey::Preferences)?;
    assert!(decoded.can_save());
    assert_eq!(
        serde_json::from_slice::<Value>(&encode(&decoded.document)?)?,
        value
    );
    #[cfg(unix)]
    {
        value["new_box_folder"] = json!({ "encoding": "unix_bytes", "value": [47, 102, 255] });
        let decoded = decode(&serde_json::to_vec(&value)?, ConfigKey::Preferences)?;
        assert!(decoded.can_save());
        assert_eq!(
            serde_json::from_slice::<Value>(&encode(&decoded.document)?)?,
            value
        );
    }
    for invalid in [
        json!({"encoding": "utf8", "value": "relative"}),
        json!({"encoding": "utf8", "value": "/fixture", "unknown": true}),
        json!({"encoding": "utf8", "value": "/null\0"}),
        json!({"encoding": "utf8", "value": format!("/{}", "a".repeat(4096))}),
        json!(false),
    ] {
        value["new_box_folder"] = invalid;
        let decoded = decode(&serde_json::to_vec(&value)?, ConfigKey::Preferences)?;
        assert!(!decoded.can_save());
        assert!(matches!(
            decoded.document,
            ConfigurationDocument::Preferences(AppPreferences {
                new_box_folder: None,
                ..
            })
        ));
    }
    value
        .as_object_mut()
        .ok_or("Expected preferences object")?
        .remove("new_box_folder");
    assert!(matches!(
        decode(&serde_json::to_vec(&value)?, ConfigKey::Preferences),
        Err(ConfigError::InvalidPayload)
    ));
    Ok(())
}

#[test]
fn duplicate_identities_overlap_and_outside_bounds_cannot_replace_the_first_valid_box()
-> Result<(), Box<dyn std::error::Error>> {
    for field in ["duplicate", "overlap", "outside", "zero_id"] {
        let mut value = example()?;
        let first = value["boxes"][0].clone();
        let mut second = first.clone();
        second["id"] = json!(9);
        second["standalone_view"]["id"] = json!(9);
        second["bounds"]["column"] = json!(8);
        let mut later = first.clone();
        later["id"] = json!(10);
        later["standalone_view"]["id"] = json!(42);
        later["bounds"]["column"] = json!(12);
        value["boxes"] = json!([first, second, later]);
        let baseline = from_value(&value)?;
        assert!(baseline.can_save());
        assert_eq!(restored(&baseline)?.boxes().len(), 3);
        assert_eq!(
            serde_json::from_slice::<Value>(&encode(&baseline.document)?)?,
            value
        );
        match field {
            "duplicate" => value["boxes"][1]["id"] = json!(1),
            "overlap" => value["boxes"][1]["bounds"]["column"] = json!(0),
            "outside" => value["boxes"][1]["bounds"]["column"] = json!(199),
            "zero_id" => value["boxes"][1]["id"] = json!(0),
            _ => unreachable!(),
        }
        let decoded = from_value(&value)?;
        assert!(!decoded.can_save(), "{field}");
        assert_eq!(decoded.issues.len(), 1, "{field}");
        assert_eq!(decoded.issues[0].record_index, 1, "{field}");
        let expected = [
            restored(&baseline)?.boxes()[0].clone(),
            restored(&baseline)?.boxes()[2].clone(),
        ];
        assert_eq!(
            restored(&decoded)?.boxes(),
            expected,
            "{field} must preserve the complete first and later valid boxes"
        );
    }
    Ok(())
}

#[test]
fn unknown_top_level_fields_fail_and_unknown_box_fields_disable_autosave()
-> Result<(), Box<dyn std::error::Error>> {
    let baseline = example()?;
    let decoded = from_value(&baseline)?;
    assert!(decoded.can_save());
    assert_eq!(
        serde_json::from_slice::<Value>(&encode(&decoded.document)?)?,
        baseline
    );
    let mut value = baseline.clone();
    value["new_schema_field"] = json!(true);
    assert!(matches!(
        from_value(&value),
        Err(ConfigError::InvalidPayload)
    ));
    value
        .as_object_mut()
        .ok_or("Expected an object")?
        .remove("new_schema_field");
    value["boxes"][0]["future_setting"] = json!(true);
    let decoded = from_value(&value)?;
    assert!(!decoded.can_save());
    assert!(restored(&decoded)?.boxes().is_empty());
    for (field, invalid) in [
        ("show_forward", json!("false")),
        ("file_view", json!("future_layout")),
    ] {
        let mut damaged = baseline.clone();
        damaged["boxes"][0]["settings"][field] = invalid;
        let decoded = from_value(&damaged)?;
        assert!(
            !decoded.can_save(),
            "{field} must not become a writable default"
        );
        assert!(restored(&decoded)?.boxes().is_empty(), "{field}");
    }
    Ok(())
}

#[test]
fn invalid_and_excessive_histories_recover_for_viewing_without_altering_the_original()
-> Result<(), Box<dyn std::error::Error>> {
    let mut value = example()?;
    let histories = (0..105)
        .map(|index| json!({"encoding":"utf8", "value":format!("/fixture/{index}")}))
        .collect::<Vec<_>>();
    value["boxes"][0]["standalone_view"]["history"]["back"] = json!(histories);
    value["boxes"][0]["standalone_view"]["history"]["forward"] = json!([
        {"encoding":"utf8", "value":"relative"},
        {"encoding":"utf8", "value":"/fixture/next"}
    ]);
    let decoded = from_value(&value)?;
    assert!(!decoded.can_save());
    let nav = &restored(&decoded)?.boxes()[0].standalone.navigation;
    assert_eq!(nav.back_history().len(), 100);
    assert_eq!(
        nav.back_history().front(),
        Some(&std::path::PathBuf::from("/fixture/5"))
    );
    assert_eq!(
        nav.back_target(),
        Some(std::path::Path::new("/fixture/104"))
    );
    assert_eq!(
        nav.forward_target(),
        Some(std::path::Path::new("/fixture/next"))
    );
    assert_eq!(
        value["boxes"][0]["standalone_view"]["history"]["back"]
            .as_array()
            .map(Vec::len),
        Some(105)
    );
    Ok(())
}

#[test]
fn relative_null_and_oversized_paths_never_restore_a_writable_view()
-> Result<(), Box<dyn std::error::Error>> {
    for path in [
        "relative".to_owned(),
        "/bad\0path".into(),
        format!("/{}", "x".repeat(4096)),
        format!("/{}", "界".repeat(1400)),
    ] {
        let mut value = example()?;
        value["boxes"][0]["standalone_view"]["current_folder"]["value"] = json!(path);
        let decoded = from_value(&value)?;
        assert!(!decoded.can_save());
        assert!(restored(&decoded)?.boxes().is_empty());
    }
    Ok(())
}

#[test]
fn workspace_index_preserves_order_and_reports_invalid_or_duplicate_records()
-> Result<(), Box<dyn std::error::Error>> {
    let value = json!({"format":FORMAT,"schema_version":SCHEMA_VERSION,"kind":"workspace_index", "workspaces":[
        {"id":OTHER_ID,"name":"Second"}, {"id":OTHER_ID,"name":"Duplicate"},
        {"id":ID,"name":" "}, {"id":ID,"name":"First"}
    ]});
    let decoded = decode(&serde_json::to_vec(&value)?, ConfigKey::WorkspaceIndex)?;
    assert!(!decoded.can_save());
    assert_eq!(decoded.issues.len(), 2);
    let ConfigurationDocument::WorkspaceIndex(index) = decoded.document else {
        return Err("Expected a workspace index".into());
    };
    assert_eq!(
        index
            .workspaces
            .iter()
            .map(|record| record.name.as_str())
            .collect::<Vec<_>>(),
        ["Second", "First"]
    );
    for name in [
        "".into(),
        " ".into(),
        " Leading".into(),
        "Trailing ".into(),
        "Double  space".into(),
        "bad/char".into(),
        "control\nname".into(),
        "x".repeat(WorkspaceIndex::MAX_NAME_CHARACTERS + 1),
    ] {
        let document = ConfigurationDocument::WorkspaceIndex(WorkspaceIndex {
            workspaces: vec![WorkspaceSummary { id: id()?, name }],
        });
        assert!(encode(&document).is_err());
    }
    Ok(())
}

#[test]
fn encoder_rejects_invalid_domain_mutation_and_excessive_aggregate_output()
-> Result<(), Box<dyn std::error::Error>> {
    let decoded = from_value(&example()?)?;
    let ConfigurationDocument::Workspace(mut record) = decoded.document else {
        return Err("Expected a workspace document".into());
    };
    record
        .workspace
        .box_mut(BoxId(1))
        .ok_or("Missing view")?
        .rect
        .width = 0;
    assert!(matches!(
        encode(&ConfigurationDocument::Workspace(record)),
        Err(ConfigError::InvalidState(_))
    ));

    let mut workspace = Workspace::default();
    let path: std::path::PathBuf = format!("/{}", "x".repeat(4095)).into();
    for number in 1..=12_u64 {
        let nav = Navigation::restore(
            path.clone(),
            path.clone(),
            VecDeque::from(vec![path.clone(); 100]),
            VecDeque::from(vec![path.clone(); 100]),
        )
        .ok_or("Invalid history fixture")?;
        workspace.restore_box(FolderBox {
            id: BoxId(number),
            rect: GridRect {
                origin: Cell::new(number as u16, 0),
                width: 1,
                height: 1,
            },
            standalone: crate::domain::FolderView {
                view: ViewId(number),
                navigation: nav,
            },
            tabs: None,
            settings: crate::domain::BoxSettings::default(),
        })?;
    }
    let document = ConfigurationDocument::Workspace(WorkspaceDocument {
        id: id()?,
        workspace,
    });
    assert!(matches!(encode(&document), Err(ConfigError::TooLarge)));
    Ok(())
}

fn tabbed_value() -> Result<Value, Box<dyn std::error::Error>> {
    let mut value = example()?;
    let mut first = value["boxes"][0]["standalone_view"].clone();
    first["id"] = json!(9);
    let mut second = first.clone();
    second["id"] = json!(12);
    second["current_folder"]["value"] = json!("/fixture/Other");
    value["boxes"][0]["tabs"] =
        json!({"visible": true, "active_view": 12, "views": [first, second]});
    Ok(value)
}

#[test]
fn tabs_round_trip_order_active_identity_hidden_collection_and_standalone_history()
-> Result<(), Box<dyn std::error::Error>> {
    for visible in [true, false] {
        let mut value = tabbed_value()?;
        value["boxes"][0]["tabs"]["visible"] = json!(visible);
        let decoded = from_value(&value)?;
        assert!(decoded.can_save());
        let workspace = restored(&decoded)?;
        let item = &workspace.boxes()[0];
        let tabs = item.tabs.as_ref().ok_or("Missing restored tabs")?;
        assert_eq!(
            tabs.views()
                .iter()
                .map(|view| view.view)
                .collect::<Vec<_>>(),
            [ViewId(9), ViewId(12)]
        );
        assert_eq!(tabs.active_view().view, ViewId(12));
        assert_eq!(
            item.active_view().view,
            if visible { ViewId(12) } else { ViewId(1) }
        );
        assert_eq!(
            item.standalone.navigation.back_target(),
            Some(std::path::Path::new("/fixture"))
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&encode(&decoded.document)?)?,
            value
        );
        assert!(!workspace.can_undo());
    }
    Ok(())
}

#[test]
fn invalid_tabs_are_recovery_only_and_cannot_overwrite_or_replace_later_valid_boxes()
-> Result<(), Box<dyn std::error::Error>> {
    let original = tabbed_value()?;
    let mut cases = Vec::new();
    for tabs in [
        json!({}),
        json!({"visible": true, "active_view": 9, "views": []}),
        json!({"visible": "true", "active_view": 9, "views": []}),
    ] {
        let mut value = original.clone();
        value["boxes"][0]["tabs"] = tabs;
        cases.push(value);
    }
    let mut missing = original.clone();
    missing["boxes"][0]
        .as_object_mut()
        .ok_or("Missing box")?
        .remove("tabs");
    cases.push(missing);
    for (key, invalid) in [("active_view", json!(99)), ("extra", json!(true))] {
        let mut value = original.clone();
        value["boxes"][0]["tabs"][key] = invalid;
        cases.push(value);
    }
    for identity in [0, 1, 9] {
        let mut value = original.clone();
        value["boxes"][0]["tabs"]["views"][1]["id"] = json!(identity);
        cases.push(value);
    }
    let mut too_many = original.clone();
    too_many["boxes"][0]["tabs"]["views"] = json!(
        (0..=Workspace::MAX_TABS)
            .map(|index| {
                let mut view = original["boxes"][0]["tabs"]["views"][0].clone();
                view["id"] = json!(index + 9);
                view
            })
            .collect::<Vec<_>>()
    );
    cases.push(too_many);
    for mut value in cases {
        let mut later = example()?["boxes"][0].clone();
        later["id"] = json!(2);
        later["standalone_view"]["id"] = json!(20);
        later["bounds"]["column"] = json!(5);
        value["boxes"]
            .as_array_mut()
            .ok_or("Missing boxes")?
            .push(later);
        let bytes = serde_json::to_vec(&value)?;
        let decoded = decode(&bytes, ConfigKey::Workspace(id()?))?;
        assert!(!decoded.can_save());
        assert_eq!(restored(&decoded)?.boxes().len(), 1);
        assert_eq!(restored(&decoded)?.boxes()[0].id, BoxId(2));
    }
    let mut duplicate = original;
    let mut later = duplicate["boxes"][0].clone();
    later["id"] = json!(2);
    later["bounds"]["column"] = json!(5);
    later["standalone_view"]["id"] = json!(9);
    later["tabs"] = Value::Null;
    duplicate["boxes"]
        .as_array_mut()
        .ok_or("Missing boxes")?
        .push(later);
    let decoded = from_value(&duplicate)?;
    assert!(!decoded.can_save());
    assert_eq!(restored(&decoded)?.boxes().len(), 1);
    assert_eq!(restored(&decoded)?.boxes()[0].id, BoxId(1));
    Ok(())
}
