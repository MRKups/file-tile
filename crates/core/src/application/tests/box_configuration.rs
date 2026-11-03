use crate::application::contracts::directory::{DirectoryError, DirectoryErrorKind, ListingOrigin};
use crate::application::*;
use crate::domain::BoxId;

fn setup() -> Application {
    let mut app = Application::new("/fixture".into(), "/home".into());
    app.update(Action::SelectMode(Mode::Layout));
    for column in [0, 4] {
        app.update(Action::BeginDraw(
            Cell::new(column, 0),
            GridExtent::new(12, 8),
        ));
        let effects = app.update(Action::FinishDraw);
        for effect in effects {
            if let Effect::List(request) = effect {
                app.update(Action::ListingCompleted(ListingCompletion {
                    result: Ok(DirectoryListing::new(
                        request.directory.clone(),
                        ListingOrigin::Controlled,
                        vec![],
                        vec![],
                    )),
                    request,
                }));
            }
        }
    }
    app.update(Action::SelectMode(Mode::Configure));
    app.update(Action::ConfigureBox(Some(BoxId(1))));
    app
}

fn request(effects: Vec<Effect>) -> Result<ListingRequest, Box<dyn std::error::Error>> {
    effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::List(request) => Some(request),
            _ => None,
        })
        .ok_or_else(|| "Missing configuration read".into())
}

fn completion(request: ListingRequest, path: &str) -> Action {
    Action::ListingCompleted(ListingCompletion {
        request,
        result: Ok(DirectoryListing::new(
            path.into(),
            ListingOrigin::Controlled,
            vec![],
            vec![],
        )),
    })
}

#[test]
fn invalid_failed_cancelled_and_superseded_configuration_never_replace_valid_state()
-> Result<(), Box<dyn std::error::Error>> {
    let mut app = setup();
    let before = app.workspace().boxes().to_vec();
    assert!(
        app.update(Action::ConfigureFolder(ViewId(1), "~someone".into()))
            .is_empty()
    );
    assert!(app.box_configuration_error(ViewId(1)).is_some());
    let failed = request(app.update(Action::ConfigureFolder(ViewId(1), "/missing".into())))?;
    app.update(Action::ListingCompleted(ListingCompletion {
        request: failed,
        result: Err(DirectoryError::new(
            "/missing".into(),
            DirectoryErrorKind::NotFound,
        )),
    }));
    assert!(app.box_configuration_error(ViewId(1)).is_some());
    assert!(matches!(app.load(ViewId(1)), Some(LoadState::Ready(_))));
    let stale = request(app.update(Action::ConfigureFolder(ViewId(1), "/stale".into())))?;
    let latest = request(app.update(Action::ConfigureFolder(ViewId(1), "/latest".into())))?;
    app.update(completion(stale, "/stale"));
    assert_eq!(app.workspace().boxes(), before);
    app.update(Action::Escape);
    app.update(completion(latest, "/latest"));
    assert_eq!(app.workspace().boxes(), before);
    assert_eq!(app.workspace().mode(), Mode::Configure);
    assert!(app.box_configuration_error(ViewId(1)).is_none());
    Ok(())
}
