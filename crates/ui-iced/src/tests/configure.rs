use super::*;
use crate::domain::{BoxId, Cell, GridExtent, NavigationIntent};

fn fixture() -> Desktop {
    let mut desktop = app_settings::tests::ready_desktop(None);
    let _ = desktop.dispatch(Action::SelectMode(Mode::Layout));
    let _ = desktop.dispatch(Action::BeginDraw(Cell::new(5, 0), GridExtent::new(13, 9)));
    let _ = desktop.dispatch(Action::DrawTo(Cell::new(8, 2)));
    let _ = desktop.dispatch(Action::FinishDraw);
    for view in [ViewId(1), ViewId(2)] {
        finish_read(&mut desktop, view);
    }
    let _ = desktop.dispatch(Action::SelectMode(Mode::Browse));
    desktop
}

fn finish_read(desktop: &mut Desktop, view: ViewId) {
    if let Some(LoadState::Loading { request, .. }) = desktop.application.load(view) {
        let request = request.clone();
        let result = desktop.directory.list(&request.directory);
        let _ = desktop.dispatch(Action::ListingCompleted(ListingCompletion {
            request,
            result,
        }));
    }
}

#[test]
fn explicit_navigation_supersedes_the_same_clicks_configure_blur() {
    let mut desktop = fixture();
    let _ = desktop.dispatch(Action::SelectMode(Mode::Configure));
    let _ = desktop.dispatch(Action::ConfigureBox(Some(BoxId(1))));
    let _ = desktop.update(Message::BoxFolderChanged(ViewId(1), "~/Projects".into()));
    let _ = desktop.dispatch(Action::Navigate(ViewId(1), Navigate::Up));
    let _ = desktop.update(Message::PathFocusChecked {
        boundary: None,
        focused: None,
        box_folder: false,
        version: desktop.focus_version,
    });
    assert!(matches!(
        desktop.application.load(ViewId(1)),
        Some(LoadState::Loading {
            intent: NavigationIntent::Open,
            ..
        })
    ));
    finish_read(&mut desktop, ViewId(1));
    assert_eq!(
        desktop.application.workspace().boxes()[0]
            .standalone
            .navigation
            .current(),
        std::path::Path::new("/Users/demo")
    );
}
