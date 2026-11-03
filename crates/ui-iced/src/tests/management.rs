use super::*;
use crate::application::contracts::configuration::{WorkspaceDocument, WorkspaceSummary};
use crate::application::contracts::storage::{
    LoadedConfiguration, ManagementResult, StorageCompletion,
};
use iced::advanced::widget::Id;

fn click(
    harness: &mut WidgetHarness,
    desktop: &mut Desktop,
    id: impl Into<Id>,
) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
    let point = harness.bounds(desktop, id)?.center();
    Ok(harness.desktop_frame(
        desktop,
        mouse::Cursor::Available(point),
        &[
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ],
    ))
}

fn created_result(
    desktop: &Desktop,
    request: &crate::application::contracts::storage::ManagementRequest,
    workspace: crate::domain::Workspace,
) -> Result<ManagementResult, Box<dyn std::error::Error>> {
    let crate::application::contracts::storage::WorkspaceCommand::Create { id, name } =
        &request.command
    else {
        return Err("Expected creation".into());
    };
    let (mut preferences, mut index) = desktop
        .application
        .configuration()
        .ok_or("Missing config")?
        .clone();
    index.workspaces.push(WorkspaceSummary {
        id: *id,
        name: name.clone(),
    });
    preferences.last_workspace_id = Some(*id);
    Ok(ManagementResult {
        index: index.clone(),
        selected: Some(Box::new(LoadedConfiguration {
            preferences,
            index,
            active: WorkspaceDocument { id: *id, workspace },
            user_themes: Default::default(),
            user_themes_notice: None,
            recovery_issues: vec![],
            backup: None,
            notice: None,
        })),
        source_saved: true,
        source_error: None,
        cancelled: false,
    })
}

#[test]
fn management_widget_removal_requires_confirmation_and_targets_only_inactive_identity()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::application::contracts::storage::WorkspaceCommand;
    let mut desktop = app_settings::tests::ready_desktop(None);
    let source = desktop
        .application
        .configuration()
        .ok_or("Missing config")?
        .0
        .last_workspace_id
        .ok_or("Missing source")?;
    desktop.application.update(Action::SelectMode(Mode::Layout));
    let _ = desktop.update(Message::OpenWorkspaceManagement);
    let _ = desktop.update(Message::ManagementNameChanged("Main".into()));
    let _ = desktop.update(Message::CreateWorkspace);
    let creation = desktop
        .application
        .management_request()
        .ok_or("Missing creation")?;
    let WorkspaceCommand::Create { id, .. } = creation.command else {
        return Err("Expected creation".into());
    };
    let result = created_result(&desktop, &creation, crate::domain::Workspace::default())?;
    let _ = desktop.dispatch(Action::StorageCompleted(StorageCompletion::Managed {
        operation: creation.receipt_tag(),
        result: Ok(result),
    }));
    desktop.size = Size::new(480.0, 360.0);
    desktop.application.update(Action::SelectMode(Mode::Layout));
    let mut harness = WidgetHarness::new()?;
    click(&mut harness, &mut desktop, "workspace-management")?;
    assert!(
        harness
            .bounds(&desktop, format!("workspace-remove-{id}"))
            .is_err()
    );
    click(
        &mut harness,
        &mut desktop,
        format!("workspace-remove-{source}"),
    )?;
    assert!(!desktop.application.workspace_management_pending());
    assert!(harness.bounds(&desktop, "workspace-remove-confirm").is_ok());
    click(&mut harness, &mut desktop, "workspace-remove-cancel")?;
    assert!(!desktop.application.workspace_management_pending());
    assert!(
        harness
            .bounds(&desktop, "workspace-remove-confirm")
            .is_err()
    );
    click(
        &mut harness,
        &mut desktop,
        format!("workspace-remove-{source}"),
    )?;
    click(&mut harness, &mut desktop, "workspace-remove-confirm")?;
    let removal = desktop
        .application
        .management_request()
        .ok_or("Missing removal")?;
    assert_eq!(removal.command, WorkspaceCommand::Remove(source));
    let mut index = desktop
        .application
        .configuration()
        .ok_or("Missing index")?
        .1
        .clone();
    index.workspaces.retain(|entry| entry.id != source);
    let _ = desktop.dispatch(Action::StorageCompleted(StorageCompletion::Managed {
        operation: removal.receipt_tag(),
        result: Ok(ManagementResult {
            index,
            selected: None,
            source_saved: true,
            source_error: None,
            cancelled: false,
        }),
    }));
    assert!(desktop.workspace_management.is_none());
    assert_eq!(
        desktop
            .application
            .configuration()
            .ok_or("Missing config")?
            .0
            .last_workspace_id,
        Some(id)
    );
    assert_eq!(desktop.application.workspace().mode(), Mode::Layout);
    Ok(())
}
