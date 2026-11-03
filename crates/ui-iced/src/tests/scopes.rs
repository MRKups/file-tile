use super::*;
use crate::application::contracts::directory::DirectoryReader;
use crate::application::workspace::scopes::ScopeEdit;
use crate::domain::{BoxId, BoxSetting, ScopeKind};

pub(super) fn finish(desktop: &mut Desktop) -> Result<(), Box<dyn std::error::Error>> {
    for _ in 0..256 {
        let action = if let Some(request) = desktop.application.scope_request().cloned() {
            let result = desktop.directory.list(&request.directory);
            Some(Action::ScopeResolved(ListingCompletion { request, result }))
        } else if let Some(request) = desktop.application.scope_budget() {
            let result = crate::persistence::encode(
                &crate::application::contracts::configuration::ConfigurationDocument::Workspace(
                    request.document.as_ref().clone(),
                ),
            )
            .map(|_| ())
            .map_err(|error| {
                crate::application::contracts::storage::StorageFailure::InvalidConfiguration(
                    error.reason().into(),
                )
            });
            Some(Action::StorageCompleted(
                crate::application::contracts::storage::StorageCompletion::BudgetChecked {
                    generation: request.generation,
                    result,
                },
            ))
        } else if let Some(request) = desktop
            .application
            .folder_tree()
            .and_then(|tree| tree.requests().next())
            .cloned()
        {
            let result = desktop.directory.list(&request.directory);
            Some(Action::TreeCompleted(ListingCompletion { request, result }))
        } else if let Some(request) =
            desktop
                .application
                .workspace()
                .active_views()
                .find_map(|view| match desktop.application.load(view.view) {
                    Some(LoadState::Loading { request, .. }) => Some(request.clone()),
                    _ => None,
                })
        {
            let result = match &desktop.directory {
                DirectoryBackend::Controlled(reader) => reader.list_scoped(
                    &request.directory,
                    &desktop.application.read_scope(request.view),
                    &Cancellation::default(),
                ),
                _ => return Err("Expected controlled widget fixture".into()),
            };
            Some(Action::ListingCompleted(ListingCompletion {
                request,
                result,
            }))
        } else {
            None
        };
        let Some(action) = action else {
            return Ok(());
        };
        let _ = desktop.dispatch(action);
    }
    Err("Configuration fixture failed to settle within its bounded reads".into())
}
pub(super) fn fixture() -> Result<Desktop, Box<dyn std::error::Error>> {
    let mut desktop = tabs::fixture(0)?;
    let _ = desktop.dispatch(Action::SelectMode(Mode::Configure));
    let _ = desktop.dispatch(Action::ConfigureBox(Some(BoxId(1))));
    finish(&mut desktop)?;
    Ok(desktop)
}

#[test]
fn save_recovery_modal_cancels_budget_admission_and_blocks_queued_scope_tree_actions()
-> Result<(), Box<dyn std::error::Error>> {
    let mut desktop = fixture()?;
    let view = ViewId(1);
    let _ = desktop.dispatch(Action::EditScope(
        view,
        ScopeEdit::Select(ScopeKind::Branch),
    ));
    let request = desktop
        .application
        .scope_request()
        .cloned()
        .ok_or("Missing scope resolution")?;
    let result = desktop.directory.list(&request.directory);
    let _ = desktop.dispatch(Action::ScopeResolved(ListingCompletion { request, result }));
    let budget = desktop
        .application
        .scope_budget()
        .ok_or("Missing scope admission")?;
    let saved = desktop
        .application
        .update(Action::WindowPlacementObserved(
            crate::domain::WindowPlacement {
                x: 50,
                y: 50,
                width: 900,
                height: 700,
            },
        ))
        .into_iter()
        .find_map(|effect| {
            if let Effect::SaveWorkspace(save) = effect {
                Some(save)
            } else {
                None
            }
        })
        .ok_or("Missing controlled save")?;
    let _ = desktop.dispatch(Action::StorageCompleted(
        crate::application::contracts::storage::StorageCompletion::Saved {
            tag: saved.tag,
            result: Err(crate::application::contracts::storage::StorageFailure::Unavailable),
        },
    ));
    assert!(desktop.modal_open());
    let before = desktop.application.workspace().snapshot();
    assert_eq!(desktop.application.workspace().selected(), Some(BoxId(1)));
    assert!(desktop.application.workspace().can_undo());
    assert!(
        !desktop.application.workspace().boxes()[0]
            .settings
            .show_hidden
    );
    for action in [
        Action::SetBoxSetting(BoxId(1), BoxSetting::HiddenFiles(true)),
        Action::RemoveSelected,
        Action::Undo,
        Action::SelectMode(Mode::Browse),
        Action::EditScope(view, ScopeEdit::Select(ScopeKind::Pinned)),
        Action::ToggleTree(view, "/Users/demo".into()),
        Action::ConfigurePath(view, "/Users/demo/Projects".into()),
        Action::StorageCompleted(
            crate::application::contracts::storage::StorageCompletion::BudgetChecked {
                generation: budget.generation,
                result: Ok(()),
            },
        ),
    ] {
        let _ = desktop.dispatch(action);
        assert!(desktop.application.workspace().same_durable_state(&before));
        assert_eq!(desktop.application.workspace().mode(), Mode::Configure);
    }
    assert!(desktop.application.workspace().same_durable_state(&before));
    assert!(!desktop.application.scope_pending(view));
    assert!(desktop.application.folder_tree().is_none());
    let _ = desktop.update(Message::StorageDetails(false));
    assert!(
        desktop.application.folder_tree().is_some(),
        "Dismissing recovery resumes only the current Configure tree"
    );
    Ok(())
}
