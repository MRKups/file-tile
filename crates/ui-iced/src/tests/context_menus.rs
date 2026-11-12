use super::*;
use crate::application::files::opening::Opening;
use crate::domain::{BoxId, BoxSetting, FileView};
use entry_menu::Command;

fn fixture(layout: FileView) -> Result<Desktop, Box<dyn std::error::Error>> {
    let mut desktop = tabs::fixture(0)?;
    let _ = desktop.dispatch(Action::SelectMode(Mode::Configure));
    let _ = desktop.dispatch(Action::ConfigureBox(Some(BoxId(1))));
    let _ = desktop.dispatch(Action::SetBoxSetting(
        BoxId(1),
        BoxSetting::FileView(layout),
    ));
    let _ = desktop.dispatch(Action::SelectMode(Mode::Browse));
    let root = desktop
        .application
        .workspace()
        .view(ViewId(1))
        .ok_or("Missing view")?
        .navigation
        .current()
        .to_path_buf();
    let entries = [
        ("Folder", EntryKind::Directory),
        ("Image.PNG", EntryKind::File),
        ("Notes.txt", EntryKind::File),
        ("Special", EntryKind::Other),
    ]
    .into_iter()
    .map(|(name, kind)| DirectoryEntry {
        name: name.into(),
        path: root.join(name),
        kind,
        hidden: false,
        symlink: false,
        dataless: false,
        inode: None,
    })
    .collect();
    replace_listing(&mut desktop, entries)?;
    Ok(desktop)
}

fn replace_listing(
    desktop: &mut Desktop,
    entries: Vec<DirectoryEntry>,
) -> Result<(), Box<dyn std::error::Error>> {
    let request = desktop
        .application
        .update(Action::Navigate(ViewId(1), Navigate::Refresh))
        .into_iter()
        .find_map(|effect| match effect {
            Effect::List(request) => Some(request),
            _ => None,
        })
        .ok_or("Missing listing request")?;
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
    Ok(())
}

fn path(desktop: &Desktop, name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(desktop
        .application
        .workspace()
        .view(ViewId(1))
        .ok_or("Missing view")?
        .navigation
        .current()
        .join(name))
}

fn right_click(
    harness: &mut WidgetHarness,
    desktop: &mut Desktop,
    path: &std::path::Path,
) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
    let bounds = harness.bounds(desktop, file_entry::focus_id(ViewId(1), path))?;
    Ok(harness.desktop_frame(
        desktop,
        mouse::Cursor::Available(bounds.center()),
        &[
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)),
        ],
    ))
}

fn click(
    harness: &mut WidgetHarness,
    desktop: &mut Desktop,
    id: &'static str,
) -> Result<Vec<Message>, Box<dyn std::error::Error>> {
    let bounds = harness.bounds(desktop, id)?;
    Ok(harness.desktop_frame(
        desktop,
        mouse::Cursor::Available(bounds.center()),
        &[
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ],
    ))
}

fn menu_generation(desktop: &Desktop) -> Result<u64, Box<dyn std::error::Error>> {
    Ok(desktop
        .entry_menu
        .as_ref()
        .ok_or("Missing item menu")?
        .generation)
}

#[test]
fn file_menu_starts_with_open_without_preview_or_automatic_launch()
-> Result<(), Box<dyn std::error::Error>> {
    let mut harness = WidgetHarness::new()?;
    for name in ["Image.PNG", "Notes.txt"] {
        let mut desktop = fixture(FileView::List)?;
        let image = path(&desktop, name)?;
        let before = desktop.application.workspace().snapshot();
        right_click(&mut harness, &mut desktop, &image)?;
        assert!(desktop.application.external_opening().is_none());
        let mut probe = BoundsProbe::default();
        harness.operate(desktop.view(), desktop.size, &mut probe);
        for (id, icon_id, label) in [
            (entry_menu::OPEN_ID, "entry-context-open-icon", "Open"),
            (entry_menu::COPY_ID, "entry-context-copy-icon", "Copy path"),
        ] {
            let row = harness.bounds(&desktop, id)?;
            let icon = harness.bounds(&desktop, icon_id)?;
            let label = probe
                .labels
                .iter()
                .find(|(text, _)| text == label)
                .ok_or("Missing menu label")?
                .1;
            assert!((icon.x - row.x - 12.0).abs() < 0.5);
            assert!((label.x - icon.x - icon.width - 8.0).abs() < 0.5);
            assert!((label.center_y() - row.center_y()).abs() < 0.5);
        }
        assert!(!probe.labels.iter().any(|(text, _)| text == "Preview image"));
        assert!(
            harness.bounds(&desktop, entry_menu::OPEN_ID)?.y
                < harness.bounds(&desktop, entry_menu::COPY_ID)?.y
        );
        click(&mut harness, &mut desktop, entry_menu::OPEN_ID)?;
        assert!(desktop.entry_menu.is_none());
        assert!(desktop.application.workspace().same_durable_state(&before));
        assert!(
            desktop
                .application
                .external_opening()
                .is_some_and(|opening| opening.request.path == image
                    && matches!(opening.phase, Opening::Pending(_)))
        );
    }
    Ok(())
}

#[test]
fn item_menu_listing_resize_focus_and_close_invalidation_reject_queued_actions()
-> Result<(), Box<dyn std::error::Error>> {
    let mut harness = WidgetHarness::new()?;
    for invalidation in 0..4 {
        let mut desktop = fixture(FileView::List)?;
        let file = path(&desktop, "Notes.txt")?;
        right_click(&mut harness, &mut desktop, &file)?;
        let generation = desktop
            .entry_menu
            .as_ref()
            .ok_or("Missing menu")?
            .generation;
        let generation = if invalidation == 0 {
            let _ = desktop.update(Message::DismissEntryMenu(generation));
            right_click(&mut harness, &mut desktop, &file)?;
            let current = menu_generation(&desktop)?;
            assert_ne!(current, generation);
            let _ = desktop.update(Message::EntryMenuAction(generation, Command::Open));
            assert!(desktop.application.external_opening().is_none());
            assert_eq!(
                desktop.entry_menu.as_ref().map(|menu| menu.generation),
                Some(current)
            );
            let _ = desktop.update(Message::EntryOpened(ViewId(1), file.clone()));
            assert!(desktop.application.external_opening().is_none());
            assert_eq!(
                desktop.entry_menu.as_ref().map(|menu| menu.generation),
                Some(current)
            );
            current
        } else {
            generation
        };
        match invalidation {
            0 => {
                let entries = desktop
                    .application
                    .load(ViewId(1))
                    .and_then(LoadState::listing)
                    .ok_or("Missing listing")?
                    .entries()
                    .to_vec();
                replace_listing(&mut desktop, entries)?;
            }
            1 => {
                let _ = desktop.update(Message::ViewportLaidOut(Size::new(700.0, 600.0)));
            }
            2 => {
                let _ = desktop.input(Event::Window(window::Event::Unfocused), Status::Ignored);
            }
            _ => {
                let _ = desktop.dispatch(Action::RequestClose);
            }
        }
        assert!(desktop.entry_menu.is_none());
        assert_eq!(
            desktop
                .update(Message::EntryMenuAction(generation, Command::Open))
                .units(),
            0
        );
        assert!(desktop.application.external_opening().is_none());
    }
    Ok(())
}
