use crate::application::workspace::scopes::ScopeEdit;
use crate::application::*;
use crate::domain::{BoxId, ScopeKind};
use crate::filesystem::tests::{Fixture, wait};
use crate::filesystem::{DirectoryWorkers, NativeDirectory};

fn drive(
    app: &mut Application,
    workers: &DirectoryWorkers,
    effects: Vec<Effect>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut queue = VecDeque::from(effects);
    while let Some(effect) = queue.pop_front() {
        let action = match effect {
            Effect::List(request) => {
                let scope = app.read_scope(request.view);
                Action::ListingCompleted(wait(workers.submit_scoped(request, scope)))
            }
            Effect::ListTree(request) => Action::TreeCompleted(wait(workers.submit(request))),
            Effect::ObserveDirectories(_) => continue,
            Effect::MeasureFolderUsage(request) => {
                let workers = crate::filesystem::folder_usage::UsageWorkers::new()?;
                let (_, future) = workers.submit(request);
                Action::FolderUsageCompleted(wait(Box::pin(future)))
            }
            Effect::SortDirectory(request) => {
                let result = crate::filesystem::sorting::prepare(
                    &request,
                    &NativeDirectory,
                    &directory::Cancellation::default(),
                );
                Action::SortCompleted(sorting::SortCompletion { request, result })
            }
            _ => return Err("Unexpected persistence operation in native scope fixture".into()),
        };
        queue.extend(app.update(action));
    }
    Ok(())
}
fn fixture() -> Result<(Fixture, Application, DirectoryWorkers), Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    std::fs::create_dir(fixture.path().join("Root"))?;
    let root = std::fs::canonicalize(fixture.path())?;
    let mut app = Application::new(root.join("Root"), root);
    let workers = DirectoryWorkers::new(Arc::new(NativeDirectory), 2, 8)?;
    app.update(Action::SelectMode(Mode::Layout));
    app.update(Action::BeginDraw(Cell::new(0, 0), GridExtent::new(15, 10)));
    let effects = app.update(Action::FinishDraw);
    drive(&mut app, &workers, effects)?;
    app.update(Action::SelectMode(Mode::Configure));
    let effects = app.update(Action::ConfigureBox(Some(BoxId(1))));
    drive(&mut app, &workers, effects)?;
    Ok((fixture, app, workers))
}

#[test]
fn scope_budget_rejection_and_changed_baseline_do_not_add_undo_or_replace_scope()
-> Result<(), Box<dyn std::error::Error>> {
    let (_fixture, mut app, mut workers) = fixture()?;
    for reject in [true, false] {
        let effects = app.update(Action::EditScope(
            ViewId(1),
            ScopeEdit::Select(ScopeKind::Branch),
        ));
        let request = effects
            .into_iter()
            .find_map(|effect| {
                if let Effect::ResolveScope(request) = effect {
                    Some(request)
                } else {
                    None
                }
            })
            .ok_or("Missing scope resolution")?;
        let effects = app.update(Action::ScopeResolved(wait(workers.resolve(request))));
        let budget = effects
            .into_iter()
            .find_map(|effect| {
                if let Effect::CheckWorkspaceBudget(request) = effect {
                    Some(request)
                } else {
                    None
                }
            })
            .ok_or("Missing scope budget")?;
        if !reject {
            app.update(Action::SetBoxSetting(
                BoxId(1),
                crate::domain::BoxSetting::HiddenFiles(true),
            ));
        }
        let before = app.workspace.snapshot();
        let undo = app.workspace.can_undo();
        let result = if reject {
            Err(storage::StorageFailure::InvalidConfiguration(
                "Synthetic encoded-budget rejection".into(),
            ))
        } else {
            Ok(())
        };
        let effects = app.update(Action::StorageCompleted(StorageCompletion::BudgetChecked {
            generation: budget.generation,
            result,
        }));
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::SaveWorkspace(_)))
        );
        assert!(app.workspace.same_durable_state(&before));
        assert_eq!(app.workspace.can_undo(), undo);
        assert!(app.scope_error(ViewId(1)).is_some());
    }
    workers.shutdown()?;
    Ok(())
}
