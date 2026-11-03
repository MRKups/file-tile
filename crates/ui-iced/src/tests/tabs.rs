use super::*;
use crate::application::contracts::storage::{StorageCompletion, StorageFailure};
use crate::domain::{BoxId, Cell, GridDelta, GridExtent, LayoutKind, ResizeHandle, TabCommand};
use iced::advanced::widget::operation::{Outcome, focusable};

pub(super) fn fixture(count: usize) -> Result<Desktop, Box<dyn std::error::Error>> {
    let mut desktop = app_settings::tests::ready_desktop(None);
    let _ = desktop.dispatch(Action::SelectMode(Mode::Layout));
    for (handle, delta) in [
        (
            ResizeHandle::East,
            GridDelta {
                columns: 2,
                rows: 0,
            },
        ),
        (
            ResizeHandle::South,
            GridDelta {
                columns: 0,
                rows: 1,
            },
        ),
    ] {
        let _ = desktop.dispatch(Action::BeginLayout {
            id: BoxId(1),
            kind: LayoutKind::Resize(handle),
            extent: GridExtent::new(15, 10),
        });
        let _ = desktop.dispatch(Action::LayoutTo(delta));
        let _ = desktop.dispatch(Action::FinishLayout);
    }
    let _ = desktop.dispatch(Action::BeginDraw(Cell::new(7, 0), GridExtent::new(15, 10)));
    let _ = desktop.dispatch(Action::DrawTo(Cell::new(10, 3)));
    let _ = desktop.dispatch(Action::FinishDraw);
    finish_reads(&mut desktop, 80)?;
    let _ = desktop.dispatch(Action::SelectMode(Mode::Configure));
    let _ = desktop.dispatch(Action::ConfigureBox(Some(BoxId(1))));
    if count > 0 {
        change(&mut desktop, TabCommand::Visible(true))?;
        for _ in 1..count {
            change(&mut desktop, TabCommand::Add)?;
        }
    }
    let _ = desktop.dispatch(Action::SelectMode(Mode::Browse));
    Ok(desktop)
}

fn finish_reads(desktop: &mut Desktop, count: usize) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        let request = desktop
            .application
            .workspace()
            .active_views()
            .find_map(|view| match desktop.application.load(view.view) {
                Some(LoadState::Loading { request, .. }) => Some(request.clone()),
                _ => None,
            });
        let Some(request) = request else {
            break;
        };
        let LoadState::Ready(data) = listing(&request.directory.to_string_lossy(), count) else {
            return Err("Missing synthetic listing".into());
        };
        let _ = desktop.dispatch(Action::ListingCompleted(ListingCompletion {
            request,
            result: Ok((*data).clone()),
        }));
    }
    Ok(())
}

fn accept_budget(desktop: &mut Desktop) -> Result<(), Box<dyn std::error::Error>> {
    let request = desktop
        .application
        .pending_tab_budget()
        .ok_or("Missing tab budget request")?;
    let result = crate::persistence::encode(
        &crate::application::contracts::configuration::ConfigurationDocument::Workspace(
            request.document.as_ref().clone(),
        ),
    )
    .map(|_| ())
    .map_err(|error| StorageFailure::InvalidConfiguration(error.reason().into()));
    let _ = desktop.dispatch(Action::StorageCompleted(StorageCompletion::BudgetChecked {
        generation: request.generation,
        result,
    }));
    Ok(())
}

fn change(desktop: &mut Desktop, command: TabCommand) -> Result<(), Box<dyn std::error::Error>> {
    let _ = desktop.dispatch(Action::ChangeTabs(BoxId(1), command));
    accept_budget(desktop)?;
    finish_reads(desktop, 80)
}

fn views(desktop: &Desktop) -> Result<Vec<ViewId>, Box<dyn std::error::Error>> {
    Ok(desktop.application.workspace().boxes()[0]
        .tabs
        .as_ref()
        .ok_or("Missing tabs")?
        .views()
        .iter()
        .map(|view| view.view)
        .collect())
}

#[test]
fn tab_pointer_press_blur_resolution_release_cannot_commit_the_old_folder_draft()
-> Result<(), Box<dyn std::error::Error>> {
    let mut desktop = fixture(2)?;
    let tabs = views(&desktop)?;
    let old = tabs[1];
    let _ = desktop.dispatch(Action::SelectMode(Mode::Configure));
    let _ = desktop.dispatch(Action::ConfigureBox(Some(BoxId(1))));
    let before = desktop
        .application
        .workspace()
        .view(old)
        .ok_or("Missing original tab")?
        .navigation
        .clone();
    let _ = desktop.update(Message::BoxFolderChanged(
        old,
        "/Users/demo/Projects".into(),
    ));
    let mut harness = WidgetHarness::new()?;
    harness.operate(
        desktop.view(),
        desktop.size,
        &mut focusable::focus::<()>(box_settings::folder_id(old).into()),
    );
    let target = harness.bounds(&desktop, tab_rail::tab_id(tabs[0]))?;
    let point = Point::new(target.x + 40.0, target.center_y());
    let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    harness.desktop_frame(
        &mut desktop,
        mouse::Cursor::Available(point),
        std::slice::from_ref(&press),
    );
    // Deliver the subscription and its measured focus receipt separately from
    // release, so a fast read can complete while the pointer is held down.
    let probe_task = desktop.input(press, Status::Captured);
    let box_folder = matches!(
        harness.operate(
            desktop.view(),
            desktop.size,
            &mut focusable::is_focused(box_settings::folder_id(old).into()),
        ),
        Outcome::Some(true)
    );
    let _ = desktop.update(Message::PathFocusChecked {
        focused: None,
        box_folder,
        boundary: None,
        version: desktop.focus_version,
    });
    if let Some(LoadState::Loading {
        request,
        intent: crate::domain::NavigationIntent::Configure,
        ..
    }) = desktop.application.load(old)
    {
        let request = request.clone();
        let _ = desktop.dispatch(Action::ListingCompleted(ListingCompletion {
            result: ControlledDirectory::default().list(&request.directory),
            request,
        }));
    }
    harness.desktop_frame(
        &mut desktop,
        mouse::Cursor::Available(point),
        &[Event::Mouse(mouse::Event::ButtonReleased(
            mouse::Button::Left,
        ))],
    );
    accept_budget(&mut desktop)?;
    finish_reads(&mut desktop, 80)?;
    assert_eq!(
        desktop.application.workspace().boxes()[0]
            .active_view()
            .view,
        tabs[0],
    );
    assert_eq!(
        desktop
            .application
            .workspace()
            .view(old)
            .ok_or("Missing retained original tab")?
            .navigation,
        &before,
        "The tab click committed a blur from its old Configure context",
    );
    drop(probe_task);
    Ok(())
}

fn edit_range_field(desktop: &mut Desktop, view: ViewId, field: range_editor::Field) -> String {
    let _ = desktop.update(Message::ChooseBoxFolder(view));
    use iced::widget::text_editor;
    match field {
        range_editor::Field::Start => {
            let _ = desktop.update(Message::RangeStartChanged(view, "/Users/demo".into()));
            range_editor::start_id(view)
        }
        range_editor::Field::Stops => {
            let _ = desktop.update(Message::RangeStopsChanged(
                view,
                text_editor::Action::SelectAll,
            ));
            let _ = desktop.update(Message::RangeStopsChanged(
                view,
                text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(
                    "/Users/demo/Projects/App/Assets".into(),
                ))),
            ));
            range_editor::stops_id(view)
        }
    }
}

fn resolve_range_budget(
    desktop: &mut Desktop,
    view: ViewId,
) -> Result<crate::application::contracts::storage::BudgetRequest, Box<dyn std::error::Error>> {
    let _ = desktop.update(Message::CommitRange(view));
    while let Some(request) = desktop.application.scope_request().cloned() {
        let _ = desktop.dispatch(Action::ScopeResolved(ListingCompletion {
            result: ControlledDirectory::default().list(&request.directory),
            request,
        }));
    }
    desktop
        .application
        .scope_budget()
        .ok_or_else(|| "Missing old Range validation budget".into())
}

type PressedTab = (Point, Vec<Message>, Task<Message>);

fn press_tab_with_probe(
    harness: &mut WidgetHarness,
    desktop: &mut Desktop,
    old: ViewId,
    target: ViewId,
) -> Result<PressedTab, Box<dyn std::error::Error>> {
    let row = harness.bounds(desktop, tab_rail::tab_id(target))?;
    let point = Point::new(row.x + 40.0, row.center_y());
    let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
    let messages = harness.desktop_frame(
        desktop,
        mouse::Cursor::Available(point),
        std::slice::from_ref(&press),
    );
    let probe = desktop.input(press, Status::Captured);
    let focused = |harness: &mut WidgetHarness, id: String| {
        matches!(
            harness.operate(
                desktop.view(),
                desktop.size,
                &mut focusable::is_focused(id.into())
            ),
            Outcome::Some(true)
        )
    };
    let box_folder = focused(harness, box_settings::folder_id(old));
    let boundary = if focused(harness, range_editor::start_id(old)) {
        Some(range_editor::Field::Start)
    } else if focused(harness, range_editor::stops_id(old)) {
        Some(range_editor::Field::Stops)
    } else {
        None
    };
    let _ = desktop.update(Message::PathFocusChecked {
        focused: None,
        box_folder,
        boundary,
        version: desktop.focus_version,
    });
    Ok((point, messages, probe))
}

#[test]
fn tab_pointer_press_suppresses_both_range_blurs_and_rejects_old_validation_before_release()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::application::workspace::scopes::ScopeEdit;
    for field in [range_editor::Field::Start, range_editor::Field::Stops] {
        for submitted in [false, true] {
            let mut desktop = fixture(2)?;
            let tabs = views(&desktop)?;
            let old = tabs[1];
            let _ = desktop.dispatch(Action::SelectMode(Mode::Configure));
            let _ = desktop.dispatch(Action::ConfigureBox(Some(BoxId(1))));
            let _ = desktop.dispatch(Action::EditScope(
                old,
                ScopeEdit::Start("/Users/demo/Projects".into()),
            ));
            scopes::finish(&mut desktop)?;
            let before = desktop
                .application
                .workspace()
                .view(old)
                .ok_or("Missing configured Range tab")?
                .navigation
                .clone();
            let settings = desktop.application.workspace().boxes()[0].settings;
            let other = desktop.application.workspace().boxes()[1].clone();
            let editor = edit_range_field(&mut desktop, old, field);
            let old_budget = if submitted {
                Some(resolve_range_budget(&mut desktop, old)?)
            } else {
                None
            };
            let mut harness = WidgetHarness::new()?;
            harness.operate(
                desktop.view(),
                desktop.size,
                &mut focusable::focus::<()>(editor.into()),
            );
            let (point, messages, probe) =
                press_tab_with_probe(&mut harness, &mut desktop, old, tabs[0])?;
            assert_eq!(messages.iter().filter(|message| matches!(message,
                Message::Action(Action::ChangeTabs(_, TabCommand::Activate(view))) if *view == tabs[0]
            )).count(), 1);
            // Finish any erroneously admitted blur, then deliver validation
            // already queued before the tab press. Neither may change the old view.
            scopes::finish(&mut desktop)?;
            if let Some(budget) = old_budget {
                let _ =
                    desktop.dispatch(Action::StorageCompleted(StorageCompletion::BudgetChecked {
                        generation: budget.generation,
                        result: Ok(()),
                    }));
            }
            assert!(!desktop.application.scope_pending(old));
            assert!(desktop.application.tab_pending(BoxId(1)));
            let released = harness.desktop_frame(
                &mut desktop,
                mouse::Cursor::Available(point),
                &[Event::Mouse(mouse::Event::ButtonReleased(
                    mouse::Button::Left,
                ))],
            );
            assert!(!released.iter().any(|message| matches!(
                message,
                Message::Action(Action::ChangeTabs(_, TabCommand::Activate(_)))
            )));
            accept_budget(&mut desktop)?;
            finish_reads(&mut desktop, 80)?;
            assert_eq!(
                desktop.application.workspace().boxes()[0]
                    .active_view()
                    .view,
                tabs[0]
            );
            assert_eq!(
                desktop
                    .application
                    .workspace()
                    .view(old)
                    .ok_or("Missing retained Range tab")?
                    .navigation,
                &before,
                "{field:?}, submitted={submitted}: the old Range draft committed during a tab click"
            );
            assert_eq!(
                desktop.application.workspace().boxes()[0].settings,
                settings
            );
            assert_eq!(desktop.application.workspace().boxes()[1], other);
            drop(probe);
        }
    }
    Ok(())
}
