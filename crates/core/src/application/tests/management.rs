use crate::application::contracts::configuration::{WorkspaceId, WorkspaceSummary};
use crate::application::contracts::storage::{ManagementRequest, ManagementResult, StorageFailure};
use crate::application::*;

fn request(effects: Vec<Effect>) -> Result<ManagementRequest, &'static str> {
    effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::ManageWorkspace(request) => Some(request),
            _ => None,
        })
        .ok_or("Missing management effect")
}
#[test]
fn management_source_failure_retry_cancel_keeps_state_and_operation() -> Result<(), &'static str> {
    let (mut app, _) = super::persistence::boot(super::persistence::saved_workspace(1), 0);
    assert!(app.update(Action::OpenWorkspaceManagement).is_empty());
    assert!(!app.workspace_management_open());
    app.update(Action::SelectMode(Mode::Layout));
    app.update(Action::OpenWorkspaceManagement);
    let before = app.workspace().boxes().to_vec();
    app.configuration
        .as_mut()
        .ok_or("Missing config")?
        .1
        .workspaces[0]
        .name = "Unsaved name".into();
    let first = request(app.update(Action::CreateWorkspace("Main".into())))?;
    assert!(!app.accepts_edits());
    assert!(app.update(Action::Undo).is_empty());
    app.update(Action::StorageCompleted(StorageCompletion::Managed {
        operation: first.receipt_tag(),
        result: Err(StorageFailure::Io(std::io::ErrorKind::PermissionDenied)),
    }));
    assert_eq!(app.workspace().boxes(), before);
    assert_eq!(app.workspace_name(), Some("Unsaved name"));
    let retry = request(app.update(Action::RetryWorkspaceManagement))?;
    assert_eq!(retry.operation, first.operation);
    assert_eq!(retry.command, first.command);
    app.update(Action::StorageCompleted(StorageCompletion::Managed {
        operation: first.receipt_tag(),
        result: Err(StorageFailure::Unavailable),
    }));
    assert!(
        app.workspace_management_running(),
        "An old receipt must not finish the retry"
    );
    app.update(Action::StorageCompleted(StorageCompletion::Managed {
        operation: retry.receipt_tag(),
        result: Err(StorageFailure::Unavailable),
    }));
    let cancel = request(app.update(Action::CancelWorkspaceManagement))?;
    assert!(cancel.cancel);
    let mut index = app.configuration().ok_or("Missing config")?.1.clone();
    index.workspaces[0].name = "Main".into();
    app.update(Action::StorageCompleted(StorageCompletion::Managed {
        operation: cancel.receipt_tag(),
        result: Ok(ManagementResult {
            index,
            selected: None,
            source_saved: false,
            source_error: Some(StorageFailure::Unavailable),
            cancelled: true,
        }),
    }));
    assert!(app.accepts_edits());
    assert!(app.save_failed());
    assert_eq!(app.workspace().boxes(), before);
    assert_eq!(app.workspace_name(), Some("Unsaved name"));
    Ok(())
}

#[test]
fn management_close_failure_keeps_retry_cancel_and_explicit_discard_reachable()
-> Result<(), &'static str> {
    let (mut app, _) = super::persistence::boot(super::persistence::saved_workspace(1), 0);
    app.update(Action::SelectMode(Mode::Layout));
    app.update(Action::OpenWorkspaceManagement);
    let transition = request(app.update(Action::CreateWorkspace("Next".into())))?;
    let closing = app.update(Action::RequestClose);
    assert!(!closing.iter().any(|effect| matches!(effect, Effect::Close)));
    assert!(super::persistence::save(&closing).is_none());
    let failed = app.update(Action::StorageCompleted(StorageCompletion::Managed {
        operation: transition.receipt_tag(),
        result: Err(StorageFailure::Unavailable),
    }));
    assert!(!failed.iter().any(|effect| matches!(effect, Effect::Close)));
    assert!(super::persistence::save(&failed).is_none());
    assert!(app.close_failed());
    assert!(app.storage_detail().is_some());
    assert!(
        app.update(Action::CloseWithoutSaving)
            .iter()
            .any(|effect| matches!(effect, Effect::Close))
    );
    app.update(Action::CancelClose);
    assert!(!app.close_failed());
    assert!(app.workspace_management_error().is_some());
    let closing = app.update(Action::RequestClose);
    assert!(!closing.iter().any(|effect| matches!(effect, Effect::Close)));
    assert!(super::persistence::save(&closing).is_none());
    let retry = app.update(Action::RetrySave);
    assert!(!retry.iter().any(|effect| matches!(effect, Effect::Close)));
    assert!(super::persistence::save(&retry).is_none());
    let cancelling = request(retry)?;
    assert!(cancelling.cancel);
    assert!(!app.close_failed());
    let index = app.configuration().ok_or("Missing config")?.1.clone();
    let effects = app.update(Action::StorageCompleted(StorageCompletion::Managed {
        operation: cancelling.receipt_tag(),
        result: Ok(ManagementResult {
            index,
            selected: None,
            source_saved: true,
            source_error: None,
            cancelled: true,
        }),
    }));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::CaptureWindowForClose(tag)
                if tag.workspace == transition.source.tag.workspace))
    );
    assert!(!effects.iter().any(|effect| matches!(effect, Effect::Close)));
    assert!(super::persistence::save(&effects).is_none());
    Ok(())
}

struct EqualIdSwitch {
    app: Application,
    stale: ListingRequest,
    destination: configuration::WorkspaceDocument,
    completion: StorageCompletion,
}

fn equal_id_switch() -> Result<EqualIdSwitch, &'static str> {
    let (mut app, reads) = super::persistence::boot(super::persistence::saved_workspace(1), 0);
    let stale = super::persistence::listing(&reads).ok_or("Missing source read")?;
    let target = WorkspaceId::random();
    app.configuration
        .as_mut()
        .ok_or("Missing config")?
        .1
        .workspaces
        .push(WorkspaceSummary {
            id: target,
            name: "Destination".into(),
        });
    let mut workspace = super::persistence::saved_workspace(1);
    workspace
        .view_mut(ViewId(1))
        .ok_or("Missing destination view")?
        .navigation = crate::domain::Navigation::new("/fixture/destination".into());
    assert_eq!(workspace.boxes()[0].id, app.workspace().boxes()[0].id);
    assert_eq!(workspace.boxes()[0].standalone.view, stale.view);
    assert_ne!(workspace.boxes(), app.workspace().boxes());
    let destination = configuration::WorkspaceDocument {
        id: target,
        workspace,
    };
    app.update(Action::SelectMode(Mode::Layout));
    app.update(Action::OpenWorkspaceManagement);
    let transition = request(app.update(Action::SelectWorkspace(target)))?;
    let (mut preferences, index) = app.configuration().ok_or("Missing config")?.clone();
    preferences.last_workspace_id = Some(target);
    let completion = StorageCompletion::Managed {
        operation: transition.receipt_tag(),
        result: Ok(ManagementResult {
            index: index.clone(),
            selected: Some(Box::new(storage::LoadedConfiguration {
                preferences,
                index,
                active: destination.clone(),
                user_themes: Default::default(),
                user_themes_notice: None,
                recovery_issues: vec![],
                backup: None,
                notice: None,
            })),
            source_saved: true,
            source_error: None,
            cancelled: false,
        }),
    };
    Ok(EqualIdSwitch {
        app,
        stale,
        destination,
        completion,
    })
}

#[test]
fn management_switch_equal_ids_rejects_old_reads_and_close_waits_for_new_owner()
-> Result<(), &'static str> {
    for close_during_switch in [false, true] {
        let EqualIdSwitch {
            mut app,
            stale,
            destination,
            completion,
        } = equal_id_switch()?;
        let old_epoch = app.workspace_epoch();
        let target = destination.id;
        let before = destination.workspace.boxes();
        if close_during_switch {
            let closing = app.update(Action::RequestClose);
            assert!(!closing.iter().any(|effect| matches!(effect, Effect::Close)));
            assert!(super::persistence::save(&closing).is_none());
        }
        let effects = app.update(Action::StorageCompleted(completion));
        assert_ne!(app.workspace_epoch(), old_epoch);
        assert_eq!(app.workspace().mode(), Mode::Browse);
        assert!(!app.workspace().can_undo());
        assert_eq!(app.workspace().boxes(), before);
        assert!(!effects.iter().any(|effect| matches!(effect, Effect::Close)));
        assert!(super::persistence::save(&effects).is_none());
        if close_during_switch {
            let capture = effects
                .iter()
                .find_map(|effect| match effect {
                    Effect::CaptureWindowForClose(tag) => Some(*tag),
                    _ => None,
                })
                .ok_or("Switched owner must capture before close")?;
            assert_eq!(capture.workspace, target);
            assert_eq!(Some(capture.session), app.workspace_epoch());
            let flushing = app.update(Action::WindowCapturedForClose {
                tag: capture,
                placement: None,
            });
            assert!(
                !flushing
                    .iter()
                    .any(|effect| matches!(effect, Effect::Close))
            );
            let saved = super::persistence::save(&flushing).ok_or("Missing destination flush")?;
            assert_eq!(saved.tag.workspace, target);
            assert_eq!(saved.document.id, target);
            assert_eq!(saved.document.workspace.boxes(), before);
        } else {
            assert!(app.accepts_edits(), "Stale rejection must run before close");
            let fresh = super::persistence::listing(&effects).ok_or("Missing destination read")?;
            assert_eq!(fresh.view, stale.view);
            assert_ne!(fresh, stale);
            assert_eq!(
                fresh.directory,
                std::path::Path::new("/fixture/destination")
            );
            assert!(matches!(
                app.load(fresh.view),
                Some(LoadState::Loading { request, previous: None, .. }) if request == &fresh
            ));
            let ignored = super::persistence::complete(&mut app, stale.clone());
            assert!(super::persistence::save(&ignored).is_none());
            assert!(!ignored.iter().any(|effect| matches!(effect, Effect::Close)));
            assert_eq!(app.workspace().boxes(), before);
            assert!(matches!(
                app.load(fresh.view),
                Some(LoadState::Loading { request, previous: None, .. }) if request == &fresh
            ));
            let loaded = super::persistence::complete(&mut app, fresh.clone());
            assert!(super::persistence::save(&loaded).is_none());
            let destination_listing = app
                .load(fresh.view)
                .and_then(LoadState::listing)
                .cloned()
                .ok_or("Missing destination listing")?;
            assert_eq!(destination_listing.directory, fresh.directory);
            let ignored = super::persistence::complete(&mut app, stale);
            assert!(super::persistence::save(&ignored).is_none());
            assert_eq!(
                app.load(fresh.view).and_then(LoadState::listing),
                Some(&destination_listing)
            );
            assert_eq!(app.workspace().boxes(), before);
        }
    }
    Ok(())
}
