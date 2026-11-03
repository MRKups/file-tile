use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(in crate::persistence) use crate::filesystem::tests::wait;

use crate::application::contracts::configuration::{
    ConfigurationDocument, WorkspaceDocument, WorkspaceId,
};
use crate::application::contracts::storage::{
    LoadedConfiguration, SaveRequest, SaveTag, SessionId, StorageCompletion, StorageFailure,
};
use crate::domain::{Cell, GridExtent, Mode, NavigationIntent};
use crate::persistence::store::{Stage, Store};
use crate::persistence::*;

pub(crate) struct Fixture(PathBuf);

impl Fixture {
    pub(crate) fn new() -> std::io::Result<Self> {
        let root =
            std::env::temp_dir().join(format!("file-tile-storage-{}", WorkspaceId::random()));
        fs::create_dir(&root)?;
        Ok(Self(root))
    }
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    pub(in crate::persistence) fn config_bytes(
        &self,
    ) -> std::io::Result<std::collections::BTreeMap<std::ffi::OsString, Vec<u8>>> {
        fs::read_dir(self.path().join("config"))?
            .map(|entry| {
                let entry = entry?;
                Ok((entry.file_name(), fs::read(entry.path())?))
            })
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("Cannot remove owned storage fixture ({:?}).", error.kind());
        }
    }
}

fn session() -> SessionId {
    SessionId(WorkspaceId::random())
}

pub(in crate::persistence) fn request(
    loaded: &LoadedConfiguration,
    session: SessionId,
    revision: u64,
) -> SaveRequest {
    let mut workspace = loaded.active.workspace.snapshot();
    workspace.select_mode(Mode::Layout);
    workspace.begin_draw(Cell::new(0, 0), GridExtent::new(12, 8));
    workspace.draw_to(Cell::new(2, 2));
    workspace.finish_draw("/fixture/start".into());
    if let Some(item) = workspace
        .boxes()
        .first()
        .map(|item| item.active_view().view)
        .and_then(|view| workspace.view_mut(view))
    {
        item.navigation.commit(
            format!("/fixture/next-{revision}").into(),
            NavigationIntent::Open,
        );
    }
    SaveRequest {
        tag: SaveTag {
            session,
            workspace: loaded.active.id,
            revision,
            attempt: revision,
        },
        document: Arc::new(WorkspaceDocument {
            id: loaded.active.id,
            workspace,
        }),
        preferences: loaded.preferences.clone(),
        workspace_name: loaded
            .index
            .workspaces
            .iter()
            .find(|item| item.id == loaded.active.id)
            .map(|item| item.name.clone())
            .unwrap_or_default(),
        user_themes: None,
    }
}

#[test]
fn rename_survives_restart_preserves_other_workspaces_and_retries_index_failure()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::application::contracts::configuration::{WorkspaceIndex, WorkspaceSummary};
    let fixture = Fixture::new()?;
    let mut store = Store::open(fixture.path())?;
    let first = store.load(session())?;
    let other = WorkspaceId::random();
    let config = fixture.path().join("config");
    let mut index = first.index;
    index.workspaces.push(WorkspaceSummary {
        id: other,
        name: "Other".into(),
    });
    let index_bytes = encode(&ConfigurationDocument::WorkspaceIndex(index))?;
    let inactive = encode(&ConfigurationDocument::Workspace(WorkspaceDocument {
        id: other,
        workspace: crate::domain::Workspace::default(),
    }))?;
    fs::write(config.join("workspaces.json"), &index_bytes)?;
    fs::write(
        config.join(ConfigKey::Workspace(other).filename()),
        &inactive,
    )?;
    let current_session = session();
    let loaded = store.load(current_session)?;
    let mut rename = request(&loaded, current_session, 1);
    let workspace_path = config.join(ConfigKey::Workspace(loaded.active.id).filename());
    let original_workspace = fs::read(&workspace_path)?;
    rename.workspace_name = "x".repeat(WorkspaceIndex::MAX_NAME_CHARACTERS + 1);
    assert!(matches!(
        store.save(&rename),
        Err(StorageFailure::InvalidConfiguration(_))
    ));
    rename.workspace_name = "Projects 📁".into();
    assert!(matches!(
        store.save(&rename),
        Err(StorageFailure::InvalidConfiguration(_))
    ));
    rename.workspace_name = "Double  space".into();
    assert!(matches!(
        store.save(&rename),
        Err(StorageFailure::InvalidConfiguration(_))
    ));
    assert_eq!(fs::read(&workspace_path)?, original_workspace);
    rename.workspace_name = "Projekty Główne".into();
    // Index publication fails after the workspace has been published. No complete
    // save may be acknowledged; the latest snapshot must be safe to retry.
    let blocked_backup = config.join("workspaces.json.bak");
    fs::create_dir(&blocked_backup)?;
    assert_eq!(store.save(&rename), Err(StorageFailure::UnsafePath));
    assert_ne!(fs::read(&workspace_path)?, original_workspace);
    assert_eq!(fs::read(config.join("workspaces.json"))?, index_bytes);
    fs::remove_dir(&blocked_backup)?;
    store.save(&rename)?;
    assert_eq!(fs::read(&blocked_backup)?, index_bytes);
    store.save(&rename)?;
    assert_eq!(
        fs::read(&blocked_backup)?,
        index_bytes,
        "Unchanged close save keeps useful backup"
    );
    assert_eq!(
        fs::read(config.join(ConfigKey::Workspace(other).filename()))?,
        inactive
    );
    drop(store);
    let mut restarted = Store::open(fixture.path())?;
    let restored = restarted.load(session())?;
    assert_eq!(restored.index.workspaces[0].id, loaded.active.id);
    assert_eq!(restored.index.workspaces[0].name, "Projekty Główne");
    assert_eq!(
        restored.index.workspaces[1],
        WorkspaceSummary {
            id: other,
            name: "Other".into()
        }
    );
    assert_eq!(
        restored.active.workspace.boxes(),
        rename.document.workspace.boxes()
    );
    assert!(restored.recovery_issues.is_empty());
    Ok(())
}

#[test]
fn rename_refuses_externally_changed_index_without_publishing()
-> Result<(), Box<dyn std::error::Error>> {
    for edit_workspace in [false, true] {
        let fixture = Fixture::new()?;
        let mut store = Store::open(fixture.path())?;
        let current_session = session();
        let loaded = store.load(current_session)?;
        let mut rename = request(&loaded, current_session, 1);
        rename.workspace_name = "New_name".into();
        let config = fixture.path().join("config");
        let workspace_path = config.join(ConfigKey::Workspace(loaded.active.id).filename());
        let index_path = config.join("workspaces.json");
        let preferences_path = config.join("preferences.json");
        let originals = [
            (&workspace_path, fs::read(&workspace_path)?),
            (&index_path, fs::read(&index_path)?),
            (&preferences_path, fs::read(&preferences_path)?),
        ];
        let edited_path = if edit_workspace {
            &workspace_path
        } else {
            &index_path
        };
        let changed = if edit_workspace {
            b"preserve externally changed workspace".as_slice()
        } else {
            b"preserve externally changed index".as_slice()
        };
        fs::write(edited_path, changed)?;
        assert_eq!(store.save(&rename), Err(StorageFailure::ChangedExternally));
        for (path, original) in &originals {
            assert_eq!(
                fs::read(path)?,
                if *path == edited_path {
                    changed
                } else {
                    original.as_slice()
                }
            );
        }
        let baseline = originals
            .iter()
            .find(|(path, _)| *path == edited_path)
            .ok_or("Missing edited baseline")?;
        fs::write(edited_path, &baseline.1)?;
        assert_eq!(store.save(&rename), Err(StorageFailure::ReadOnly));
        for (path, original) in &originals {
            assert_eq!(fs::read(path)?, *original);
        }
    }
    Ok(())
}

#[test]
fn partial_initialization_is_preserved_and_cannot_be_reset()
-> Result<(), Box<dyn std::error::Error>> {
    for footprint in [
        "workspace-orphan.json",
        "workspaces.json.bak",
        ".file-tile-interrupted.tmp",
        "preferences.json",
    ] {
        let fixture = Fixture::new()?;
        let mut store = Store::open(fixture.path())?;
        let path = fixture.path().join("config").join(footprint);
        fs::write(&path, b"preserve")?;
        assert!(store.load(session()).is_err());
        assert_eq!(fs::read(path)?, b"preserve");
        assert_eq!(fs::read_dir(fixture.path().join("config"))?.count(), 2);
    }
    Ok(())
}

#[test]
fn publication_failures_preserve_a_valid_primary_and_backup_and_allow_retry()
-> Result<(), Box<dyn std::error::Error>> {
    for stage in [
        Stage::TemporarySynced,
        Stage::BackupPublished,
        Stage::PrimaryPublished,
    ] {
        let fixture = Fixture::new()?;
        let session = session();
        let mut store = Store::open(fixture.path())?;
        let loaded = store.load(session)?;
        let request = request(&loaded, session, 1);
        let config = fixture.path().join("config");
        let key = ConfigKey::Workspace(loaded.active.id);
        let path = config.join(key.filename());
        let backup_path = config.join(format!("{}.bak", key.filename()));
        let original = fs::read(&path)?;
        let expected = encode(&ConfigurationDocument::Workspace(
            (*request.document).clone(),
        ))?;
        assert_ne!(
            expected, original,
            "The requested edit must change saved bytes"
        );
        let preferences = fs::read(config.join("preferences.json"))?;
        let index = fs::read(config.join("workspaces.json"))?;
        store.fault = Some(stage);
        assert_eq!(
            store.save(&request),
            Err(StorageFailure::Io(std::io::ErrorKind::Other))
        );
        let interrupted_primary = if stage == Stage::PrimaryPublished {
            &expected
        } else {
            &original
        };
        assert_eq!(fs::read(&path)?, *interrupted_primary);
        if stage == Stage::TemporarySynced {
            assert!(!backup_path.try_exists()?);
        } else {
            assert_eq!(fs::read(&backup_path)?, original);
        }
        assert_eq!(fs::read(config.join("preferences.json"))?, preferences);
        assert_eq!(fs::read(config.join("workspaces.json"))?, index);
        store.fault = None;
        store.save(&request)?;
        assert_eq!(fs::read(&path)?, expected);
        assert_eq!(fs::read(&backup_path)?, original);
        assert_eq!(fs::read(config.join("preferences.json"))?, preferences);
        assert_eq!(fs::read(config.join("workspaces.json"))?, index);
        let previous = fs::read(&backup_path)?;
        store.save(&request)?;
        assert_eq!(
            fs::read(&backup_path)?,
            previous,
            "Unchanged close save keeps useful backup"
        );
        assert_eq!(fs::read(&path)?, expected);
        assert_eq!(fs::read(config.join("preferences.json"))?, preferences);
        assert_eq!(fs::read(config.join("workspaces.json"))?, index);
    }
    Ok(())
}

#[test]
fn worker_loads_and_serializes_latest_admitted_revisions() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new()?;
    let session = session();
    let mut worker = StorageWorker::new(fixture.path().into())?;
    let loaded = match wait(worker.load(session)) {
        StorageCompletion::Loaded {
            result: Ok(loaded), ..
        } => loaded,
        other => return Err(format!("Expected successful load: {other:?}").into()),
    };
    let mut futures = Vec::new();
    for revision in 1..=64 {
        futures.push(worker.save(request(&loaded, session, revision)));
    }
    for future in futures {
        assert!(matches!(
            wait(future),
            StorageCompletion::Saved { result: Ok(_), .. }
        ));
    }
    assert!(matches!(
        wait(worker.save(request(&loaded, session, 63))),
        StorageCompletion::Saved {
            result: Err(StorageFailure::StaleRequest),
            ..
        }
    ));
    worker.shutdown()?;
    let mut store = Store::open(fixture.path())?;
    let reopened = store.load(self::session())?;
    assert_eq!(
        reopened.active.workspace.boxes(),
        request(&loaded, session, 64).document.workspace.boxes()
    );
    Ok(())
}

#[test]
fn writer_lock_coordinates_real_processes_and_releases_on_exit()
-> Result<(), Box<dyn std::error::Error>> {
    if let Some(root) = std::env::var_os("FILE_TILE_LOCK_FIXTURE") {
        let result = Store::open(Path::new(&root));
        if std::env::var_os("FILE_TILE_LOCK_EXPECT_BUSY").is_some() {
            assert!(matches!(result, Err(StorageFailure::Busy)));
        } else {
            assert!(result.is_ok());
        }
        return Ok(());
    }
    let fixture = Fixture::new()?;
    let store = Store::open(fixture.path())?;
    let output = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", "persistence::tests::storage::writer_lock_coordinates_real_processes_and_releases_on_exit"])
        .env("FILE_TILE_LOCK_FIXTURE", fixture.path()).env("FILE_TILE_LOCK_EXPECT_BUSY", "1").output()?;
    assert!(
        output.status.success(),
        "Lock contender failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    drop(store);
    let output = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", "persistence::tests::storage::writer_lock_coordinates_real_processes_and_releases_on_exit"])
        .env("FILE_TILE_LOCK_FIXTURE", fixture.path()).env_remove("FILE_TILE_LOCK_EXPECT_BUSY").output()?;
    assert!(output.status.success());
    Ok(())
}

#[test]
fn managed_symlinks_oversized_input_and_bad_references_are_preserved()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut store = Store::open(fixture.path())?;
    let loaded = store.load(session())?;
    drop(store);
    let config = fixture.path().join("config");
    let preferences = config.join("preferences.json");
    let original = fs::read(&preferences)?;
    let mut value: serde_json::Value = serde_json::from_slice(&original)?;
    for modification in [
        serde_json::json!(null),
        serde_json::json!(WorkspaceId::random().to_string()),
    ] {
        value["last_workspace_id"] = modification;
        let bytes = serde_json::to_vec(&value)?;
        fs::write(&preferences, &bytes)?;
        let mut reopened = Store::open(fixture.path())?;
        assert!(reopened.load(session()).is_err());
        assert_eq!(fs::read(&preferences)?, bytes);
    }
    fs::write(&preferences, &original)?;
    let path = config.join(ConfigKey::Workspace(loaded.active.id).filename());
    let bytes = fs::read(&path)?;
    let file = fs::OpenOptions::new().write(true).open(&path)?;
    file.set_len(ConfigKey::Workspace(loaded.active.id).byte_limit() as u64 + 1)?;
    drop(file);
    let mut reopened = Store::open(fixture.path())?;
    assert!(matches!(
        reopened.load(session()),
        Err(StorageFailure::InvalidConfiguration(_))
    ));
    drop(reopened);
    #[cfg(unix)]
    {
        let preserved = fixture.path().join("preserved-workspace.json");
        fs::write(&preserved, &bytes)?;
        fs::remove_file(&path)?;
        std::os::unix::fs::symlink(&preserved, &path)?;
        let mut reopened = Store::open(fixture.path())?;
        assert!(matches!(
            reopened.load(session()),
            Err(StorageFailure::UnsafePath)
        ));
        assert_eq!(fs::read(preserved)?, bytes);
    }
    Ok(())
}

#[test]
fn diagnostics_failure_cannot_change_successful_load_or_save_results()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let logs = fixture.path().join("logs");
    fs::write(&logs, b"preserve unrelated log-path data")?;
    let session = session();
    let mut store = Store::open(fixture.path())?;
    let loaded = store.load(session)?;
    crate::diagnostics::session().record(
        crate::diagnostics::Subsystem::Storage,
        crate::diagnostics::Activity::Load,
        crate::diagnostics::Outcome::Complete,
        0,
        0,
        None,
    );
    let request = request(&loaded, session, 1);
    store.save(&request)?;
    crate::diagnostics::session().record(
        crate::diagnostics::Subsystem::Storage,
        crate::diagnostics::Activity::Save,
        crate::diagnostics::Outcome::Complete,
        1,
        0,
        None,
    );
    assert_eq!(fs::read(logs)?, b"preserve unrelated log-path data");
    let key = ConfigKey::Workspace(loaded.active.id);
    let decoded = decode(
        &fs::read(fixture.path().join("config").join(key.filename()))?,
        key,
    )?;
    match decoded.document {
        ConfigurationDocument::Workspace(active) => {
            assert_eq!(active.workspace.boxes(), request.document.workspace.boxes())
        }
        _ => return Err("Expected saved workspace".into()),
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn new_managed_permissions_are_private_and_failed_backup_keeps_primary()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new()?;
    let original_mode = fs::metadata(fixture.path())?.permissions().mode() & 0o777;
    let session = session();
    let mut store = Store::open(fixture.path())?;
    let loaded = store.load(session)?;
    assert_eq!(
        fs::metadata(fixture.path())?.permissions().mode() & 0o777,
        original_mode
    );
    // The configuration store does not own diagnostic files. Keep its permission
    // assertions and require it to leave the separate log path absent.
    assert!(!fixture.path().join("logs").try_exists()?);
    assert_eq!(
        fs::metadata(fixture.path().join("config"))?
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    for entry in fs::read_dir(fixture.path().join("config"))? {
        assert_eq!(entry?.metadata()?.permissions().mode() & 0o777, 0o600);
    }
    let key = ConfigKey::Workspace(loaded.active.id);
    let path = fixture.path().join("config").join(key.filename());
    let original = fs::read(&path)?;
    fs::create_dir(
        fixture
            .path()
            .join("config")
            .join(format!("{}.bak", key.filename())),
    )?;
    assert_eq!(
        store.save(&request(&loaded, session, 1)),
        Err(StorageFailure::UnsafePath)
    );
    assert_eq!(fs::read(path)?, original);
    Ok(())
}

#[test]
fn preferences_publication_failure_does_not_acknowledge_a_partial_session_save()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let session = session();
    let mut store = Store::open(fixture.path())?;
    let loaded = store.load(session)?;
    let config = fixture.path().join("config");
    let preferences = fs::read(config.join("preferences.json"))?;
    fs::create_dir(config.join("preferences.json.bak"))?;
    let mut request = request(&loaded, session, 1);
    request.preferences.window = Some(crate::domain::WindowPlacement {
        x: 120,
        y: 90,
        width: 960,
        height: 640,
    });
    assert_eq!(store.save(&request), Err(StorageFailure::UnsafePath));
    assert_eq!(fs::read(config.join("preferences.json"))?, preferences);
    assert!(
        decode(
            &fs::read(config.join(ConfigKey::Workspace(loaded.active.id).filename()))?,
            ConfigKey::Workspace(loaded.active.id)
        )?
        .can_save()
    );
    fs::remove_dir(config.join("preferences.json.bak"))?;
    store.save(&request)?;
    let restored = store.load(self::session())?;
    assert_eq!(restored.preferences.window, request.preferences.window);
    assert_eq!(
        restored.active.workspace.boxes(),
        request.document.workspace.boxes()
    );
    Ok(())
}
