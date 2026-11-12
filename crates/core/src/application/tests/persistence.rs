use std::collections::VecDeque;

use crate::application::contracts::configuration::{
    WorkspaceDocument, WorkspaceId, WorkspaceSummary,
};
use crate::application::contracts::directory::{DirectoryError, DirectoryErrorKind, ListingOrigin};
use crate::application::contracts::storage::{
    LoadedConfiguration, SaveOutcome, SaveTag, StorageFailure,
};
use crate::application::*;
use crate::domain::{BoxId, FolderBox, GridRect, Navigation};

pub(in crate::application) fn saved_workspace(count: usize) -> Workspace {
    let mut workspace = Workspace::default();
    for index in 0..count {
        let identity = index as u64 + 1;
        let item = FolderBox {
            id: BoxId(identity),
            rect: GridRect {
                origin: Cell::new((index % 200) as u16, (index / 200) as u16),
                width: 1,
                height: 1,
            },
            standalone: crate::domain::FolderView::new(
                ViewId(identity),
                Navigation::restore(
                    "/fixture/start".into(),
                    "/fixture/current".into(),
                    VecDeque::from(["/fixture/previous".into()]),
                    VecDeque::from(["/fixture/next".into()]),
                )
                .unwrap_or_else(|| Navigation::new("/fixture".into())),
            ),
            tabs: None,
            settings: crate::domain::BoxSettings::default(),
        };
        assert!(workspace.restore_box(item).is_ok());
    }
    workspace
}

pub(in crate::application) fn boot(
    workspace: Workspace,
    issues: usize,
) -> (Application, Vec<Effect>) {
    let (mut app, effects) =
        Application::with_storage(Some("/fixture/new-box".into()), "/fixture".into());
    let Some(Effect::LoadConfiguration(session)) = effects.first() else {
        return (app, effects);
    };
    let id = WorkspaceId::random();
    let effects = app.update(Action::StorageCompleted(StorageCompletion::Loaded {
        session: *session,
        result: Ok(Box::new(LoadedConfiguration {
            preferences: AppPreferences {
                last_workspace_id: Some(id),
                window: None,
                new_box_folder: None,
                pinned_folders: None,
                appearance: Default::default(),
                typography: Default::default(),
                safety: Default::default(),
                capabilities: Default::default(),
            },
            index: WorkspaceIndex {
                workspaces: vec![WorkspaceSummary {
                    id,
                    name: "Main".into(),
                }],
            },
            active: WorkspaceDocument { id, workspace },
            user_themes: Default::default(),
            user_themes_notice: None,
            recovery_issues: (0..issues)
                .map(
                    |record_index| crate::application::contracts::storage::RecoveryNotice {
                        document: "workspace",
                        record_index,
                        reason: "invalid record omitted".into(),
                    },
                )
                .collect(),
            backup: None,
            notice: None,
        })),
    }));
    (app, effects)
}

pub(in crate::application) fn listing(effects: &[Effect]) -> Option<ListingRequest> {
    effects.iter().find_map(|effect| match effect {
        Effect::List(request) => Some(request.clone()),
        _ => None,
    })
}

pub(in crate::application) fn save(effects: &[Effect]) -> Option<SaveRequest> {
    effects.iter().find_map(|effect| match effect {
        Effect::SaveWorkspace(request) => Some(request.clone()),
        _ => None,
    })
}

fn close_after_capture(app: &mut Application) -> Result<Vec<Effect>, &'static str> {
    let effects = app.update(Action::RequestClose);
    assert!(!effects.iter().any(|effect| matches!(effect, Effect::Close)));
    assert!(save(&effects).is_none());
    complete_capture(app, &effects)
}

fn complete_capture(
    app: &mut Application,
    effects: &[Effect],
) -> Result<Vec<Effect>, &'static str> {
    let tag = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::CaptureWindowForClose(tag) => Some(*tag),
            _ => None,
        })
        .ok_or("Close must capture the window before flushing")?;
    Ok(app.update(Action::WindowCapturedForClose {
        tag,
        placement: None,
    }))
}

pub(in crate::application) fn complete(
    app: &mut Application,
    request: ListingRequest,
) -> Vec<Effect> {
    let result = Ok(DirectoryListing::new(
        request.directory.clone(),
        ListingOrigin::Controlled,
        vec![],
        vec![],
    ));
    app.update(Action::ListingCompleted(ListingCompletion {
        request,
        result,
    }))
}

#[test]
fn restore_preserves_starts_and_histories_and_admits_two_reads_at_a_time() {
    let workspace = saved_workspace(256);
    let before = workspace.boxes().to_vec();
    let (mut app, mut effects) = boot(workspace, 0);
    assert_eq!(
        effects
            .iter()
            .filter(|effect| matches!(effect, Effect::List(_)))
            .count(),
        2
    );
    assert_eq!(app.workspace().mode(), Mode::Browse);
    assert_eq!(app.workspace().selected(), None);
    let mut completed = 0;
    while let Some(request) = listing(&effects) {
        effects.retain(|effect| !matches!(effect, Effect::List(old) if old == &request));
        effects.extend(complete(&mut app, request));
        assert!(
            effects
                .iter()
                .filter(|effect| matches!(effect, Effect::List(_)))
                .count()
                <= 2
        );
        completed += 1;
    }
    assert_eq!(completed, 256);
    assert_eq!(app.workspace().boxes(), before);
    assert!(matches!(
        app.persistence(),
        PersistenceState::Ready {
            revision: 0,
            saved_revision: 0,
            ..
        }
    ));
}

#[test]
fn recovery_and_unavailable_folders_remain_visible_through_mode_changes() {
    let (mut app, effects) = boot(saved_workspace(1), 1);
    let before = app.workspace().boxes().to_vec();
    if let Some(request) = listing(&effects) {
        let error = DirectoryError::new(request.directory.clone(), DirectoryErrorKind::Unavailable);
        assert!(
            app.update(Action::ListingCompleted(ListingCompletion {
                request,
                result: Err(error)
            }))
            .is_empty()
        );
    }
    app.update(Action::SelectMode(Mode::Layout));
    app.update(Action::BeginDraw(Cell::new(2, 0), GridExtent::new(12, 8)));
    assert!(save(&app.update(Action::FinishDraw)).is_none());
    app.update(Action::Escape);
    assert!(matches!(
        app.persistence(),
        PersistenceState::Recovery { .. }
    ));
    assert!(app.storage_detail().is_some());
    assert_eq!(&app.workspace().boxes()[0], &before[0]);
    assert!(app.update(Action::RequestClose).iter().all(|effect| matches!(effect, Effect::ObserveDirectories(observations) if observations.is_empty())));
    assert!(app.close_failed());
    assert!(matches!(
        app.update(Action::CloseWithoutSaving).first(),
        Some(Effect::Close)
    ));
    app.update(Action::CancelClose);
    assert!(app.accepts_edits());
}

#[test]
fn close_freezes_commits_and_failure_offers_retry_cancel_and_explicit_discard()
-> Result<(), &'static str> {
    let (mut app, effects) = boot(saved_workspace(1), 0);
    let before = app.workspace().boxes().to_vec();
    let sampling = app.update(Action::RequestClose);
    assert!(
        !sampling
            .iter()
            .any(|effect| matches!(effect, Effect::Close))
    );
    assert!(save(&sampling).is_none());
    let closing = complete_capture(&mut app, &sampling)?;
    assert!(!closing.iter().any(|effect| matches!(effect, Effect::Close)));
    let pending = save(&closing).ok_or("Closing must flush the current workspace")?;
    assert!(!app.accepts_edits());
    let late = listing(&effects).ok_or("Restore must have a pending read")?;
    assert!(complete(&mut app, late).is_empty());
    assert_eq!(app.workspace().boxes(), before);
    let superseded = app.update(Action::StorageCompleted(StorageCompletion::Saved {
        tag: pending.tag,
        result: Ok(SaveOutcome::Superseded),
    }));
    assert!(
        !superseded
            .iter()
            .any(|effect| matches!(effect, Effect::Close))
    );
    assert!(!app.accepts_edits());
    let failed = app.update(Action::StorageCompleted(StorageCompletion::Saved {
        tag: pending.tag,
        result: Err(StorageFailure::Io(std::io::ErrorKind::PermissionDenied)),
    }));
    assert!(!failed.iter().any(|effect| matches!(effect, Effect::Close)));
    assert!(app.close_failed());
    assert!(matches!(
        app.update(Action::CloseWithoutSaving).first(),
        Some(Effect::Close)
    ));
    let retry = app.update(Action::RetrySave);
    assert!(!retry.iter().any(|effect| matches!(effect, Effect::Close)));
    assert!(save(&retry).is_none());
    let retry = complete_capture(&mut app, &retry)?;
    assert!(!retry.iter().any(|effect| matches!(effect, Effect::Close)));
    let retry = save(&retry).ok_or("Retry must flush after window capture")?;
    assert_ne!(retry.tag, pending.tag);
    let obsolete = app.update(Action::StorageCompleted(StorageCompletion::Saved {
        tag: pending.tag,
        result: Ok(SaveOutcome::Saved),
    }));
    assert!(
        !obsolete
            .iter()
            .any(|effect| matches!(effect, Effect::Close))
    );
    assert!(!app.accepts_edits());
    app.update(Action::CancelClose);
    assert!(app.accepts_edits());
    assert!(app.save_failed());
    app.update(Action::Escape);
    assert!(app.save_failed());
    let closing = close_after_capture(&mut app)?;
    assert!(!closing.iter().any(|effect| matches!(effect, Effect::Close)));
    let pending = save(&closing).ok_or("Final close must flush the current workspace")?;
    assert!(matches!(
        app.update(Action::StorageCompleted(StorageCompletion::Saved {
            tag: pending.tag,
            result: Ok(SaveOutcome::Saved)
        }))
        .first(),
        Some(Effect::Close)
    ));
    Ok(())
}

#[test]
fn retry_uses_latest_state_not_an_old_failed_snapshot_and_rejects_wrong_tags() {
    let (mut app, _) = boot(Workspace::default(), 0);
    app.update(Action::SelectMode(Mode::Layout));
    let mut previous: Option<SaveTag> = None;
    for column in [0, 2] {
        app.update(Action::BeginDraw(
            Cell::new(column, 0),
            GridExtent::new(12, 8),
        ));
        let effects = app.update(Action::FinishDraw);
        if let Some(request) = save(&effects) {
            previous = Some(request.tag);
            app.update(Action::StorageCompleted(StorageCompletion::Saved {
                tag: request.tag,
                result: Err(StorageFailure::Unavailable),
            }));
        }
    }
    let retry = save(&app.update(Action::RetrySave));
    assert!(
        retry
            .as_ref()
            .is_some_and(|request| request.document.workspace.boxes().len() == 2)
    );
    if let Some(tag) = previous {
        app.update(Action::StorageCompleted(StorageCompletion::Saved {
            tag,
            result: Ok(SaveOutcome::Saved),
        }));
    }
    assert!(app.save_failed());
    if let Some(request) = retry {
        let mut wrong = request.tag;
        wrong.workspace = WorkspaceId::random();
        app.update(Action::StorageCompleted(StorageCompletion::Saved {
            tag: wrong,
            result: Ok(SaveOutcome::Saved),
        }));
        assert!(app.save_failed());
        app.update(Action::StorageCompleted(StorageCompletion::Saved {
            tag: request.tag,
            result: Ok(SaveOutcome::Saved),
        }));
    }
    assert!(!app.save_failed());
}

#[test]
fn startup_retry_requires_confirmation_for_unsaved_viewing_changes_and_rejects_old_loads() {
    use crate::application::contracts::storage::StartupOperation;
    let (mut app, _) = boot(saved_workspace(1), 1);
    app.update(Action::SelectMode(Mode::Layout));
    app.update(Action::BeginDraw(Cell::new(2, 0), GridExtent::new(12, 8)));
    app.update(Action::FinishDraw);
    assert!(app.startup_has_unsaved_edits());
    assert!(
        app.update(Action::RestartStartup {
            operation: StartupOperation::Retry,
            confirmed: false
        })
        .is_empty()
    );
    assert!(app.can_retry_startup());
    let effects = app.update(Action::RestartStartup {
        operation: StartupOperation::Retry,
        confirmed: true,
    });
    assert!(matches!(
        effects.first(),
        Some(Effect::LoadConfiguration(_))
    ));
    assert!(!app.accepts_edits());
    let current = match app.persistence() {
        PersistenceState::Loading(current) => *current,
        _ => SessionId(WorkspaceId::random()),
    };
    app.update(Action::StorageCompleted(StorageCompletion::Loaded {
        session: SessionId(WorkspaceId::random()),
        result: Err(StorageFailure::Busy),
    }));
    assert!(matches!(app.persistence(), PersistenceState::Loading(session) if *session == current));
    assert!(
        app.update(Action::RestartStartup {
            operation: StartupOperation::Retry,
            confirmed: true
        })
        .is_empty()
    );
}

#[test]
fn restoring_a_backup_requires_a_current_offer_and_explicit_confirmation() {
    use crate::application::contracts::storage::{BackupOffer, StartupOperation};
    let (mut app, effects) = Application::with_storage(Some("/fixture".into()), "/fixture".into());
    let session = match effects.first() {
        Some(Effect::LoadConfiguration(session)) => *session,
        _ => SessionId(WorkspaceId::random()),
    };
    let offer = BackupOffer {
        token: WorkspaceId::random(),
        documents: vec!["preferences.json".into()],
    };
    app.update(Action::StorageCompleted(StorageCompletion::Loaded {
        session,
        result: Err(StorageFailure::BackupAvailable {
            cause: Box::new(StorageFailure::ReadOnly),
            offer: offer.clone(),
        }),
    }));
    assert_eq!(app.backup_offer(), Some(&offer));
    for (token, confirmed) in [(offer.token, false), (WorkspaceId::random(), true)] {
        assert!(
            app.update(Action::RestartStartup {
                operation: StartupOperation::RestoreBackup(token),
                confirmed
            })
            .is_empty()
        );
        assert_eq!(app.backup_offer(), Some(&offer));
    }
    let effects = app.update(Action::RestartStartup {
        operation: StartupOperation::RestoreBackup(offer.token),
        confirmed: true,
    });
    assert!(
        matches!(effects.first(), Some(Effect::RestoreBackup { token, .. }) if *token == offer.token)
    );
    assert!(!app.accepts_edits());
    assert!(app.backup_offer().is_none());
}

#[test]
fn window_preferences_do_not_change_boxes_and_old_receipts_cannot_close_before_capture() {
    use crate::domain::WindowPlacement;
    let (mut app, _) = boot(saved_workspace(1), 0);
    let boxes = app.workspace().boxes().to_vec();
    let placement = WindowPlacement {
        x: 100,
        y: 80,
        width: 960,
        height: 640,
    };
    let saved = save(&app.update(Action::WindowPlacementObserved(placement)));
    assert!(
        saved
            .as_ref()
            .is_some_and(|request| request.preferences.window == Some(placement))
    );
    assert_eq!(app.workspace().boxes(), boxes);
    assert!(
        app.update(Action::WindowPlacementObserved(placement))
            .is_empty()
    );
    let closing = app.update(Action::RequestClose);
    let tag = closing.iter().find_map(|effect| match effect {
        Effect::CaptureWindowForClose(tag) => Some(*tag),
        _ => None,
    });
    assert!(tag.is_some());
    if let Some(saved) = saved {
        assert!(
            app.update(Action::StorageCompleted(StorageCompletion::Saved {
                tag: saved.tag,
                result: Ok(SaveOutcome::Saved)
            }))
            .is_empty()
        );
    }
    assert!(!app.accepts_edits());
    assert!(app.update(Action::RetrySave).is_empty());
    if let Some(tag) = tag {
        let current = WindowPlacement {
            x: 200,
            y: 120,
            width: 1080,
            height: 700,
        };
        let effects = app.update(Action::WindowCapturedForClose {
            tag,
            placement: Some(current),
        });
        let saved = save(&effects);
        assert!(
            saved
                .as_ref()
                .is_some_and(|request| request.preferences.window == Some(current))
        );
        assert_eq!(app.window_placement(), Some(current));
        assert!(
            app.update(Action::WindowCapturedForClose {
                tag,
                placement: Some(placement)
            })
            .is_empty()
        );
        if let Some(saved) = saved {
            assert!(matches!(
                app.update(Action::StorageCompleted(StorageCompletion::Saved {
                    tag: saved.tag,
                    result: Ok(SaveOutcome::Saved)
                }))
                .first(),
                Some(Effect::Close)
            ));
        }
    }
}
