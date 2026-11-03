use crate::application::contracts::directory::DirectoryReader;
use crate::application::*;
use crate::domain::{BoxId, TabCommand};
use crate::filesystem::ControlledDirectory;

fn fixture() -> Application {
    let mut app = Application::new("/Users/demo/Documents".into(), "/Users/demo".into());
    app.update(Action::SelectMode(Mode::Layout));
    for column in [0, 5] {
        app.update(Action::BeginDraw(
            Cell::new(column, 0),
            GridExtent::new(15, 10),
        ));
        let effects = app.update(Action::FinishDraw);
        finish_reads(&mut app, effects);
    }
    app.update(Action::SelectMode(Mode::Configure));
    app.update(Action::ConfigureBox(Some(BoxId(1))));
    app
}

fn finish_reads(app: &mut Application, effects: Vec<Effect>) {
    for effect in effects {
        if let Effect::List(request) = effect {
            let result = ControlledDirectory::default().list(&request.directory);
            app.update(Action::ListingCompleted(ListingCompletion {
                request,
                result,
            }));
        }
    }
}

#[test]
fn failed_superseded_and_changed_baseline_checks_do_not_commit_or_consume_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let mut app = fixture();
    let before = app.workspace.snapshot();
    app.update(Action::ChangeTabs(BoxId(1), TabCommand::Visible(true)));
    let first = app
        .pending_tab_budget()
        .ok_or("Missing first budget check")?;
    app.update(Action::StorageCompleted(StorageCompletion::BudgetChecked {
        generation: first.generation,
        result: Err(storage::StorageFailure::InvalidConfiguration(
            "too large".into(),
        )),
    }));
    assert!(app.workspace.same_durable_state(&before));
    assert!(app.tab_error(BoxId(1)).is_some());
    app.update(Action::ChangeTabs(BoxId(1), TabCommand::Visible(true)));
    let second = app
        .pending_tab_budget()
        .ok_or("Missing second budget check")?;
    assert_eq!(
        first.document.workspace.boxes()[0].active_view().view,
        second.document.workspace.boxes()[0].active_view().view
    );
    app.update(Action::StorageCompleted(StorageCompletion::BudgetChecked {
        generation: first.generation,
        result: Ok(()),
    }));
    assert!(app.workspace.same_durable_state(&before));
    app.update(Action::SetBoxSetting(
        BoxId(1),
        crate::domain::BoxSetting::Up(true),
    ));
    let changed = app.workspace.snapshot();
    app.update(Action::StorageCompleted(StorageCompletion::BudgetChecked {
        generation: second.generation,
        result: Ok(()),
    }));
    assert!(app.workspace.same_durable_state(&changed));
    assert!(
        app.tab_error(BoxId(1))
            .is_some_and(|message| message.contains("workspace changed"))
    );
    Ok(())
}
