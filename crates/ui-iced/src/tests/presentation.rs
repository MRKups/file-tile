use super::*;
use crate::application::files::metadata::{FileMetadata, MetadataCompletion};
use crate::domain::{BoxId, BoxSetting, FileView};
use iced::advanced::widget::operation::focusable;

fn choose(desktop: &mut Desktop, view: FileView) {
    let _ = desktop.dispatch(Action::SelectMode(Mode::Configure));
    let _ = desktop.dispatch(Action::ConfigureBox(Some(BoxId(1))));
    let _ = desktop.dispatch(Action::SetBoxSetting(BoxId(1), BoxSetting::FileView(view)));
}
fn metadata(desktop: &mut Desktop) {
    let requests: Vec<_> = desktop.application.metadata_requests().cloned().collect();
    for request in requests {
        let values = request
            .paths
            .iter()
            .map(|path| {
                (
                    path.clone(),
                    Ok(FileMetadata {
                        bytes: Some(1234),
                        modified: Ok(std::time::UNIX_EPOCH),
                        created: Some(std::time::UNIX_EPOCH),
                        item_count: None,
                        dimensions: None,
                    }),
                )
            })
            .collect();
        let _ = desktop.dispatch(Action::MetadataCompleted(MetadataCompletion {
            request,
            result: Ok(values),
        }));
    }
}
fn redraw() -> Event {
    Event::Window(window::Event::RedrawRequested(std::time::Instant::now()))
}

fn files_fixture() -> Result<Desktop, Box<dyn std::error::Error>> {
    let mut desktop = tabs::fixture(0)?;
    let request = desktop
        .application
        .update(Action::Navigate(ViewId(1), Navigate::Refresh))
        .into_iter()
        .find_map(|effect| {
            if let Effect::List(request) = effect {
                Some(request)
            } else {
                None
            }
        })
        .ok_or("Missing synthetic read")?;
    let entries = (0..80)
        .map(|index| {
            let name = format!("file-{index:03}.txt");
            DirectoryEntry {
                name: name.clone().into(),
                path: request.directory.join(name),
                kind: EntryKind::File,
                hidden: false,
                symlink: false,
                dataless: false,
                inode: None,
            }
        })
        .collect();
    let data = DirectoryListing::new(
        request.directory.clone(),
        ListingOrigin::Controlled,
        entries,
        vec![],
    );
    let _ = desktop.dispatch(Action::ListingCompleted(ListingCompletion {
        request,
        result: Ok(data),
    }));
    Ok(desktop)
}

fn enter() -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Named(key::Named::Enter),
        modified_key: Key::Named(key::Named::Enter),
        physical_key: key::Physical::Code(key::Code::Enter),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::empty(),
        text: None,
        repeat: false,
    })
}

#[test]
fn columns_selects_files_for_details_and_scoped_parents_and_hides_secondary_panes()
-> Result<(), Box<dyn std::error::Error>> {
    let mut desktop = files_fixture()?;
    choose(&mut desktop, FileView::Columns);
    let _ = desktop.dispatch(Action::SelectMode(Mode::Browse));
    let view = ViewId(1);
    let path = desktop
        .application
        .workspace()
        .view(view)
        .ok_or("Missing columns view")?
        .navigation
        .current()
        .join("file-000.txt");
    let before = desktop.application.workspace().snapshot();
    let mut harness = WidgetHarness::new()?;
    let row = harness.bounds(&desktop, file_entry::focus_id(view, &path))?;
    let messages = harness.desktop_frame(
        &mut desktop,
        mouse::Cursor::Available(row.center()),
        &[
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ],
    );
    assert!(messages.iter().any(
        |message| matches!(message, Message::EntrySelected(selected_view, selected) if *selected_view == view && selected == &path)
    ));
    assert_eq!(
        desktop
            .views
            .get(&view)
            .and_then(|state| state.selected.as_ref()),
        Some(&path)
    );
    assert!(desktop.application.workspace().same_durable_state(&before));
    assert!(
        desktop.application.external_opening().is_none(),
        "Selection alone must not launch a handler"
    );
    harness.desktop_frame(&mut desktop, mouse::Cursor::Unavailable, &[redraw()]);
    while desktop.application.metadata_requests().next().is_some() {
        metadata(&mut desktop);
    }
    let details = harness.bounds(&desktop, "column-details-1")?;
    assert!(details.width >= 140.0);
    let mut probe = BoundsProbe::default();
    harness.operate(desktop.view(), desktop.size, &mut probe);
    assert!(probe.labels.iter().any(|(label, _)| label == "Open file"));
    assert!(
        probe.labels.iter().any(|(label, _)| label == "1.2 kB"),
        "Details ignore the disabled list metadata switches"
    );
    let parent = desktop
        .application
        .workspace()
        .view(view)
        .ok_or("Missing columns navigation")?
        .navigation
        .allowed_ancestors()
        .next()
        .ok_or("Missing parent")?
        .to_path_buf();
    harness.operate(
        desktop.view(),
        desktop.size,
        &mut focusable::focus::<()>(format!("column-parent-{view:?}-{parent:?}").into()),
    );
    let messages = harness.desktop_frame(&mut desktop, mouse::Cursor::Unavailable, &[enter()]);
    assert!(messages.iter().any(
        |message| matches!(message, Message::ColumnNavigate(ViewId(1), path) if path == &parent)
    ));
    assert!(
        matches!(desktop.application.load(view), Some(LoadState::Loading { request, .. }) if request.directory == parent),
        "Parent navigation uses the atomic directory path"
    );
    let _ = desktop.dispatch(Action::OpenAppSettings);
    let before = desktop.application.workspace().snapshot();
    let _ = desktop.update(Message::ColumnNavigate(view, "/".into()));
    assert!(desktop.application.workspace().same_durable_state(&before));
    Ok(())
}
