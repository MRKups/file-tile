use std::fs;
use std::sync::Arc;

use super::storage::{Fixture, wait};
use crate::application::contracts::configuration::{WorkspaceDocument, WorkspaceId};
use crate::application::contracts::storage::{
    BackupOffer, LoadedConfiguration, SaveRequest, SaveTag, SessionId, StorageCompletion,
    StorageFailure,
};
use crate::domain::{Cell, GridExtent, Mode};
use crate::persistence::recovery::RecoveryPlan;
use crate::persistence::store::Store;
use crate::persistence::*;

fn session() -> SessionId {
    SessionId(WorkspaceId::random())
}

fn prepare(
    fixture: &Fixture,
) -> Result<(Store, LoadedConfiguration, SessionId), Box<dyn std::error::Error>> {
    let session = session();
    let mut store = Store::open(fixture.path())?;
    let loaded = store.load(session)?;
    let mut workspace = loaded.active.workspace.snapshot();
    workspace.select_mode(Mode::Layout);
    workspace.begin_draw(Cell::new(0, 0), GridExtent::new(12, 8));
    workspace.draw_to(Cell::new(2, 2));
    workspace.finish_draw("/fixture/start".into());
    for revision in 1..=2 {
        if revision == 2 {
            workspace.begin_draw(Cell::new(4, 0), GridExtent::new(12, 8));
            workspace.finish_draw("/fixture/second".into());
        }
        store.save(&SaveRequest {
            preferences: loaded.preferences.clone(),
            workspace_name: "Main".into(),
            tag: SaveTag {
                session,
                workspace: loaded.active.id,
                revision,
                attempt: revision,
            },
            document: Arc::new(WorkspaceDocument {
                id: loaded.active.id,
                workspace: workspace.snapshot(),
            }),
            user_themes: None,
        })?;
    }
    Ok((store, loaded, session))
}

fn backup_offer(
    result: Result<LoadedConfiguration, StorageFailure>,
) -> Result<BackupOffer, Box<dyn std::error::Error>> {
    match result {
        Err(StorageFailure::BackupAvailable { offer, .. }) => Ok(offer),
        Ok(loaded) => loaded
            .backup
            .ok_or_else(|| "Expected backup offer for read-only viewing copy".into()),
        other => Err(format!("Expected backup offer: {other:?}").into()),
    }
}

#[test]
fn explicit_restore_preserves_corrupt_original_and_backup_then_allows_autosave()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let (mut store, loaded, _) = prepare(&fixture)?;
    let key = ConfigKey::Workspace(loaded.active.id);
    let path = fixture.path().join("config").join(key.filename());
    let backup_path = fixture
        .path()
        .join("config")
        .join(format!("{}.bak", key.filename()));
    let backup = fs::read(&backup_path)?;
    let corrupt = b"{preserve damaged workspace";
    fs::write(&path, corrupt)?;
    let offer = backup_offer(store.load(session()))?;
    assert_eq!(offer.documents, vec![key.filename()]);
    assert_eq!(
        fs::read(&path)?,
        corrupt,
        "Inspection must not modify primary"
    );
    assert!(matches!(
        store.restore(session(), WorkspaceId::random()),
        Err(StorageFailure::StaleRequest)
    ));
    let new_session = session();
    let restored = store.restore(new_session, offer.token)?;
    assert_eq!(restored.active.workspace.boxes().len(), 1);
    assert!(restored.recovery_issues.is_empty());
    assert!(restored.notice.is_some());
    assert_eq!(fs::read(&path)?, backup);
    assert_eq!(fs::read(&backup_path)?, backup);
    assert_eq!(
        fs::read(fixture.path().join("config").join(format!(
            "{}.recovery-{}",
            key.filename(),
            offer.token
        )))?,
        corrupt
    );
    let mut workspace = restored.active.workspace.snapshot();
    workspace.select_mode(Mode::Layout);
    workspace.begin_draw(Cell::new(4, 0), GridExtent::new(12, 8));
    workspace.finish_draw("/fixture/new".into());
    store.save(&SaveRequest {
        preferences: restored.preferences.clone(),
        workspace_name: "Main".into(),
        tag: SaveTag {
            session: new_session,
            workspace: restored.active.id,
            revision: 1,
            attempt: 1,
        },
        document: Arc::new(WorkspaceDocument {
            id: restored.active.id,
            workspace,
        }),
        user_themes: None,
    })?;
    assert_eq!(
        fs::read(backup_path)?,
        backup,
        "First save after recovery backs up the restored valid primary"
    );
    Ok(())
}

#[test]
fn unsupported_or_unsafe_state_and_invalid_backups_never_receive_a_restore_offer()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let (mut store, loaded, _) = prepare(&fixture)?;
    let key = ConfigKey::Workspace(loaded.active.id);
    let path = fixture.path().join("config").join(key.filename());
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    for field in ["schema_version", "format"] {
        let original = value[field].clone();
        value[field] = if field == "schema_version" {
            serde_json::json!(999)
        } else {
            serde_json::json!("other.app")
        };
        let bytes = serde_json::to_vec(&value)?;
        fs::write(&path, &bytes)?;
        assert!(matches!(
            store.load(session()),
            Err(StorageFailure::InvalidConfiguration(_))
        ));
        assert_eq!(fs::read(&path)?, bytes);
        value[field] = original;
    }
    fs::write(&path, b"{corrupt")?;
    let backup = fixture
        .path()
        .join("config")
        .join(format!("{}.bak", key.filename()));
    fs::write(&backup, b"{also corrupt")?;
    assert!(matches!(
        store.load(session()),
        Err(StorageFailure::InvalidConfiguration(_))
    ));
    fs::remove_file(&backup)?;
    fs::create_dir(&backup)?;
    assert!(matches!(
        store.load(session()),
        Err(StorageFailure::InvalidConfiguration(_))
    ));
    assert_eq!(fs::read(path)?, b"{corrupt");
    Ok(())
}

#[test]
fn changed_primary_or_backup_invalidates_confirmation_without_publication()
-> Result<(), Box<dyn std::error::Error>> {
    for change_backup in [false, true] {
        let fixture = Fixture::new()?;
        let (mut store, loaded, _) = prepare(&fixture)?;
        let config = fixture.path().join("config");
        let key = ConfigKey::Workspace(loaded.active.id);
        let path = config.join(key.filename());
        fs::write(&path, b"{damaged")?;
        let offer = backup_offer(store.load(session()))?;
        if change_backup {
            fs::write(
                config.join(format!("{}.bak", key.filename())),
                b"changed backup",
            )?;
        } else {
            fs::write(&path, b"changed primary")?;
        }
        let before = fs::read(&path)?;
        assert!(matches!(
            store.restore(session(), offer.token),
            Err(StorageFailure::ChangedExternally)
        ));
        assert_eq!(fs::read(path)?, before);
        assert!(!fs::read_dir(config)?.any(|entry| {
            entry.is_ok_and(|entry| entry.file_name().to_string_lossy().contains(".recovery-"))
        }));
    }
    Ok(())
}

#[test]
fn interrupted_multi_document_recovery_preserves_all_originals_and_can_be_retried()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let (_store, loaded, _) = prepare(&fixture)?;
    let config = fixture.path().join("config");
    let key = ConfigKey::Workspace(loaded.active.id);
    let index = fs::read(config.join("workspaces.json"))?;
    fs::write(config.join("workspaces.json.bak"), &index)?;
    let workspace_backup_path = config.join(format!("{}.bak", key.filename()));
    let workspace_backup = fs::read(&workspace_backup_path)?;
    let index_backup = fs::read(config.join("workspaces.json.bak"))?;
    fs::write(config.join("workspaces.json"), b"{damaged index")?;
    fs::write(config.join(key.filename()), b"{damaged workspace")?;
    let mut plan = RecoveryPlan::inspect(&config)?.ok_or("Expected multi-document plan")?;
    let token = plan.token;
    plan.fail_before = Some(ConfigKey::WorkspaceIndex);
    assert_eq!(
        plan.restore(&config),
        Err(StorageFailure::Io(std::io::ErrorKind::Other))
    );
    assert_eq!(fs::read(config.join(key.filename()))?, workspace_backup);
    assert_eq!(fs::read(&workspace_backup_path)?, workspace_backup);
    assert_eq!(fs::read(config.join("workspaces.json.bak"))?, index_backup);
    assert_eq!(fs::read(config.join("workspaces.json"))?, b"{damaged index");
    assert_eq!(
        fs::read(config.join(format!("{}.recovery-{token}", key.filename())))?,
        b"{damaged workspace"
    );
    assert_eq!(
        fs::read(config.join(format!("workspaces.json.recovery-{token}")))?,
        b"{damaged index"
    );
    let plan = RecoveryPlan::inspect(&config)?.ok_or("Expected remaining recovery plan")?;
    assert_eq!(plan.offer().documents, vec!["workspaces.json"]);
    plan.restore(&config)?;
    assert_eq!(fs::read(config.join("workspaces.json"))?, index_backup);
    assert_eq!(fs::read(config.join(key.filename()))?, workspace_backup);
    assert_eq!(fs::read(&workspace_backup_path)?, workspace_backup);
    assert_eq!(fs::read(config.join("workspaces.json.bak"))?, index_backup);
    assert_eq!(
        fs::read(config.join(format!("{}.recovery-{token}", key.filename())))?,
        b"{damaged workspace"
    );
    assert_eq!(
        fs::read(config.join(format!("workspaces.json.recovery-{token}")))?,
        b"{damaged index"
    );
    Ok(())
}

#[test]
fn read_only_subset_can_offer_backup_but_never_autosaves_it_without_confirmation()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let (mut store, loaded, old_session) = prepare(&fixture)?;
    let key = ConfigKey::Workspace(loaded.active.id);
    let path = fixture.path().join("config").join(key.filename());
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
    value["boxes"][1]["id"] = serde_json::json!(0);
    let original = serde_json::to_vec(&value)?;
    fs::write(&path, &original)?;
    let viewing = store.load(old_session)?;
    assert!(viewing.backup.is_some());
    assert_eq!(viewing.active.workspace.boxes().len(), 1);
    assert_eq!(
        store.save(&SaveRequest {
            preferences: viewing.preferences.clone(),
            workspace_name: "Main".into(),
            tag: SaveTag {
                session: old_session,
                workspace: loaded.active.id,
                revision: 1,
                attempt: 1
            },
            document: Arc::new(WorkspaceDocument {
                id: loaded.active.id,
                workspace: viewing.active.workspace
            }),
            user_themes: None,
        }),
        Err(StorageFailure::ReadOnly)
    );
    assert_eq!(fs::read(path)?, original);
    Ok(())
}

#[test]
fn startup_worker_restores_backups_without_releasing_writer_ownership()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let (store, loaded, _) = prepare(&fixture)?;
    let path = fixture
        .path()
        .join("config")
        .join(ConfigKey::Workspace(loaded.active.id).filename());
    fs::write(path, b"{damaged")?;
    drop(store);
    let mut worker = StorageWorker::new(fixture.path().into())?;
    let offer = match wait(worker.load(session())) {
        StorageCompletion::Loaded { result, .. } => backup_offer(result.map(|loaded| *loaded))?,
        _ => return Err("Expected startup result".into()),
    };
    assert!(matches!(
        Store::open(fixture.path()),
        Err(StorageFailure::Busy)
    ));
    let restored = match wait(worker.restore(session(), offer.token)) {
        StorageCompletion::Loaded {
            result: Ok(loaded), ..
        } => loaded,
        other => return Err(format!("Expected restored startup: {other:?}").into()),
    };
    assert_eq!(restored.active.workspace.boxes().len(), 1);
    assert!(restored.notice.is_some());
    assert!(matches!(
        Store::open(fixture.path()),
        Err(StorageFailure::Busy)
    ));
    worker.shutdown()?;
    assert!(Store::open(fixture.path()).is_ok());
    Ok(())
}
