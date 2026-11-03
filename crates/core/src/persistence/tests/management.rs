use super::storage::Fixture;
use crate::application::contracts::configuration::WorkspaceSummary;
use crate::application::contracts::storage::{
    LoadedConfiguration, ManagementRequest, ManagementResult, SaveRequest, SaveTag, SessionId,
    StorageFailure, WorkspaceCommand,
};
use crate::persistence::store::{Stage, Store};
use crate::persistence::*;
use std::fs;
use std::sync::Arc;

fn request(
    loaded: &LoadedConfiguration,
    session: SessionId,
    command: WorkspaceCommand,
) -> ManagementRequest {
    ManagementRequest {
        operation: WorkspaceId::random(),
        attempt: 1,
        next_session: SessionId(WorkspaceId::random()),
        cancel: false,
        command,
        source: SaveRequest {
            tag: SaveTag {
                session,
                workspace: loaded.active.id,
                revision: 1,
                attempt: 1,
            },
            document: Arc::new(loaded.active.clone()),
            preferences: loaded.preferences.clone(),
            workspace_name: loaded
                .index
                .workspaces
                .iter()
                .find(|entry| entry.id == loaded.active.id)
                .map_or("Main".into(), |entry| entry.name.clone()),
            user_themes: None,
        },
    }
}
fn selected(result: ManagementResult) -> Result<LoadedConfiguration, Box<dyn std::error::Error>> {
    result
        .selected
        .map(|loaded| *loaded)
        .ok_or_else(|| "Missing selected workspace".into())
}

#[test]
fn management_create_switch_remove_retains_documents_backups_and_restart_reference()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let session = SessionId(WorkspaceId::random());
    let mut store = Store::open(fixture.path())?;
    let loaded = store.load(session)?;
    let old_id = loaded.active.id;
    let old_path = fixture
        .path()
        .join("config")
        .join(ConfigKey::Workspace(old_id).filename());
    let old_bytes = fs::read(&old_path)?;
    let id = WorkspaceId::random();
    let create = request(
        &loaded,
        session,
        WorkspaceCommand::Create {
            id,
            name: "Main".into(),
        },
    );
    let created = selected(store.manage(&create)?)?;
    assert_eq!(created.active.id, id);
    assert!(created.active.workspace.boxes().is_empty());
    assert_eq!(
        created
            .index
            .workspaces
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        [old_id, id]
    );
    assert_eq!(fs::read(&old_path)?, old_bytes);
    let back = request(
        &created,
        create.next_session,
        WorkspaceCommand::Select(old_id),
    );
    let restored = selected(store.manage(&back)?)?;
    let retained_path = fixture
        .path()
        .join("config")
        .join(ConfigKey::Workspace(id).filename());
    let retained = fs::read(&retained_path)?;
    let backup =
        retained_path.with_file_name(format!("{}.bak", ConfigKey::Workspace(id).filename()));
    fs::write(&backup, &retained)?;
    let remove = request(&restored, back.next_session, WorkspaceCommand::Remove(id));
    let removed = store.manage(&remove)?;
    assert!(removed.selected.is_none());
    assert_eq!(removed.index.workspaces.len(), 1);
    assert_eq!(fs::read(&retained_path)?, retained);
    assert_eq!(fs::read(&backup)?, retained);
    assert_eq!(fs::read(&old_path)?, old_bytes);
    let denied = request(
        &restored,
        back.next_session,
        WorkspaceCommand::Remove(old_id),
    );
    assert_eq!(
        store.manage(&denied).err(),
        Some(StorageFailure::StaleRequest)
    );
    drop(store);
    let mut reopened = Store::open(fixture.path())?;
    let loaded = reopened.load(SessionId(WorkspaceId::random()))?;
    assert_eq!(loaded.active.id, old_id);
    assert_eq!(loaded.index.workspaces.len(), 1);
    Ok(())
}

#[test]
fn management_creation_partial_stages_retry_same_identity_and_cancel_reconciles_source()
-> Result<(), Box<dyn std::error::Error>> {
    for stage in [
        Stage::ManagementWorkspace,
        Stage::ManagementIndex,
        Stage::ManagementPreferences,
    ] {
        for cancel in [false, true] {
            let fixture = Fixture::new()?;
            let session = SessionId(WorkspaceId::random());
            let mut store = Store::open(fixture.path())?;
            let loaded = store.load(session)?;
            let id = WorkspaceId::random();
            let mut transition = request(
                &loaded,
                session,
                WorkspaceCommand::Create {
                    id,
                    name: "Created".into(),
                },
            );
            store.fault = Some(stage);
            assert!(store.manage(&transition).is_err());
            let created_path = fixture
                .path()
                .join("config")
                .join(ConfigKey::Workspace(id).filename());
            let bytes = fs::read(&created_path)?;
            assert_eq!(store.active_owner(), Some((session, loaded.active.id)));
            assert_eq!(
                store.save(&transition.source),
                Err(StorageFailure::StaleRequest)
            );
            store.fault = None;
            transition.cancel = cancel;
            transition.attempt += 1;
            let result = store.manage(&transition)?;
            assert_eq!(fs::read(created_path)?, bytes);
            assert_eq!(result.cancelled, cancel);
            assert_eq!(
                result
                    .index
                    .workspaces
                    .iter()
                    .filter(|entry| entry.id == id)
                    .count(),
                usize::from(stage != Stage::ManagementWorkspace || !cancel)
            );
            drop(store);
            let mut reopened = Store::open(fixture.path())?;
            let loaded_after = reopened.load(SessionId(WorkspaceId::random()))?;
            assert_eq!(
                loaded_after.active.id,
                if cancel { loaded.active.id } else { id }
            );
        }
    }
    Ok(())
}

#[test]
fn management_bad_target_and_external_source_edit_do_not_replace_source_baselines()
-> Result<(), Box<dyn std::error::Error>> {
    for damaged in [
        b"broken".as_slice(),
        br#"{"format":"file-tile.config","schema_version":999,"kind":"workspace"}"#.as_slice(),
    ] {
        let fixture = Fixture::new()?;
        let session = SessionId(WorkspaceId::random());
        let mut store = Store::open(fixture.path())?;
        let loaded = store.load(session)?;
        drop(store);
        let target = WorkspaceId::random();
        let mut index = loaded.index.clone();
        index.workspaces.push(WorkspaceSummary {
            id: target,
            name: "Broken".into(),
        });
        fs::write(
            fixture.path().join("config/workspaces.json"),
            encode(&ConfigurationDocument::WorkspaceIndex(index))?,
        )?;
        let target_path = fixture
            .path()
            .join("config")
            .join(ConfigKey::Workspace(target).filename());
        fs::write(&target_path, damaged)?;
        let mut store = Store::open(fixture.path())?;
        let loaded = store.load(session)?;
        let mut transition = request(&loaded, session, WorkspaceCommand::Select(target));
        assert!(store.manage(&transition).is_err());
        assert_eq!(store.active_owner(), Some((session, loaded.active.id)));
        transition.cancel = true;
        transition.attempt += 1;
        assert!(store.manage(&transition)?.cancelled);
        let mut saving = transition.source.clone();
        saving.tag.attempt += 1;
        store.save(&saving)?;
        assert_eq!(fs::read(target_path)?, damaged);
        fs::write(
            fixture.path().join("config/preferences.json"),
            b"external edit",
        )?;
        let mut next = request(&loaded, session, WorkspaceCommand::Select(target));
        assert_eq!(
            store.manage(&next).err(),
            Some(StorageFailure::ChangedExternally)
        );
        next.cancel = true;
        next.attempt += 1;
        store.manage(&next)?;
        assert_eq!(store.save(&saving), Err(StorageFailure::ReadOnly));
        assert_eq!(
            fs::read(fixture.path().join("config/preferences.json"))?,
            b"external edit"
        );
    }
    Ok(())
}

#[test]
fn management_cancel_source_save_failure_and_partial_removal_keep_owned_data()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let session = SessionId(WorkspaceId::random());
    let mut store = Store::open(fixture.path())?;
    let loaded = store.load(session)?;
    let new_id = WorkspaceId::random();
    let mut transition = request(
        &loaded,
        session,
        WorkspaceCommand::Create {
            id: new_id,
            name: "New".into(),
        },
    );
    transition.source.workspace_name = "Working name".into();
    let blocker = fixture.path().join("config/workspaces.json.bak");
    fs::create_dir(&blocker)?;
    assert_eq!(
        store.manage(&transition).err(),
        Some(StorageFailure::UnsafePath)
    );
    transition.cancel = true;
    transition.attempt += 1;
    let cancelled = store.manage(&transition)?;
    assert!(!cancelled.source_saved);
    assert_eq!(cancelled.source_error, Some(StorageFailure::UnsafePath));
    assert!(
        !fixture
            .path()
            .join("config")
            .join(ConfigKey::Workspace(new_id).filename())
            .exists()
    );
    fs::remove_dir(blocker)?;
    let create = request(
        &loaded,
        session,
        WorkspaceCommand::Create {
            id: new_id,
            name: "New".into(),
        },
    );
    let created = selected(store.manage(&create)?)?;
    let old_path = fixture
        .path()
        .join("config")
        .join(ConfigKey::Workspace(loaded.active.id).filename());
    let old_bytes = fs::read(&old_path)?;
    let mut remove = request(
        &created,
        create.next_session,
        WorkspaceCommand::Remove(loaded.active.id),
    );
    store.fault = Some(Stage::ManagementIndex);
    assert!(store.manage(&remove).is_err());
    assert_eq!(fs::read(&old_path)?, old_bytes);
    store.fault = None;
    remove.attempt += 1;
    let result = store.manage(&remove)?;
    assert_eq!(result.index.workspaces.len(), 1);
    assert_eq!(fs::read(old_path)?, old_bytes);
    Ok(())
}
