use crate::domain::*;

fn rect(x: u16, y: u16, width: u16, height: u16) -> GridRect {
    GridRect {
        origin: Cell::new(x, y),
        width,
        height,
    }
}

fn workspace(rectangles: &[GridRect]) -> Workspace {
    let mut workspace = Workspace::default();
    for (index, rect) in rectangles.iter().enumerate() {
        let id = index as u64 + 1;
        assert!(
            workspace
                .restore_box(FolderBox {
                    id: BoxId(id),
                    rect: *rect,
                    standalone: FolderView::new(
                        ViewId(id),
                        Navigation::new(format!("/fixture/{id}").into()),
                    ),
                    tabs: None,
                    settings: BoxSettings::default(),
                })
                .is_ok()
        );
    }
    workspace.select_mode(Mode::Layout);
    workspace
}

#[test]
fn removal_undo_restores_complete_state_and_keeps_identity_allocation_monotonic()
-> Result<(), Box<dyn std::error::Error>> {
    let mut boxes = workspace(&[rect(0, 0, 2, 2), rect(5, 0, 2, 2)])
        .boxes()
        .to_vec();
    let removed = &mut boxes[0];
    removed.settings = BoxSettings {
        sort: crate::domain::sorting::SortSettings::default(),
        show_forward: true,
        show_up: true,
        show_start: false,
        show_hidden: true,
        show_sizes: true,
        show_dates: true,
        prohibit_delete: true,
        file_view: FileView::Columns,
    };
    removed.standalone.navigation = Navigation::restore(
        "/fixture/standalone/start".into(),
        "/fixture/standalone/current".into(),
        ["/fixture/standalone/back".into()].into(),
        ["/fixture/standalone/forward".into()].into(),
    )
    .ok_or("Invalid standalone history fixture")?;
    let mut tabs = Vec::new();
    for (id, name) in [(3, "first"), (4, "second")] {
        tabs.push(FolderView::new(
            ViewId(id),
            Navigation::restore(
                format!("/fixture/{name}/start").into(),
                format!("/fixture/{name}/current").into(),
                [format!("/fixture/{name}/back").into()].into(),
                [format!("/fixture/{name}/forward").into()].into(),
            )
            .ok_or("Invalid retained-tab history fixture")?,
        ));
    }
    removed.tabs = Some(Tabs::restore(tabs, ViewId(4), false)?);
    let mut workspace = Workspace::default();
    for item in boxes {
        workspace.restore_box(item)?;
    }
    workspace.select_mode(Mode::Layout);
    let before = workspace.boxes().to_vec();
    assert!(workspace.remove_box(BoxId(1)));
    assert_eq!(workspace.boxes(), &before[1..]);
    assert_eq!(workspace.undo(), UndoOutcome::Restored);
    assert_eq!(workspace.boxes(), before);
    workspace.begin_draw(Cell::new(10, 0), GridExtent::new(20, 20));
    assert_eq!(
        workspace.finish_draw("/fixture/new".into()),
        DrawOutcome::Created(ViewId(5))
    );
    assert_eq!(workspace.undo(), UndoOutcome::Restored);
    workspace.begin_draw(Cell::new(10, 0), GridExtent::new(20, 20));
    assert_eq!(
        workspace.finish_draw("/fixture/new".into()),
        DrawOutcome::Created(ViewId(6))
    );
    Ok(())
}
