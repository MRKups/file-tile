use crate::application::contracts::directory::DirectoryReader;
use crate::application::files::opening::{OpenCompletion, Opening};
use crate::application::*;
use crate::filesystem::ControlledDirectory;
fn fixture() -> Result<Application, Box<dyn std::error::Error>> {
    let (mut app, _) = crate::application::tests::persistence::boot(
        crate::application::tests::persistence::saved_workspace(0),
        0,
    );
    app.directory_override = Some("/Users/demo/Documents".into());
    app.home = "/Users/demo".into();
    app.update(Action::SelectMode(Mode::Layout));
    app.update(Action::BeginDraw(Cell::new(0, 0), GridExtent::new(12, 8)));
    for effect in app.update(Action::FinishDraw) {
        if let Effect::List(request) = effect {
            let data = ControlledDirectory::default().list(&request.directory)?;
            let mut entries = data.entries().to_vec();
            entries.push(directory::DirectoryEntry {
                name: "image.png".into(),
                path: request.directory.join("image.png"),
                kind: directory::EntryKind::File,
                hidden: false,
                symlink: false,
                inode: None,
            });
            let result = Ok(directory::DirectoryListing::new(
                request.directory.clone(),
                data.origin,
                entries,
                vec![],
            ));
            app.update(Action::ListingCompleted(ListingCompletion {
                request,
                result,
            }));
        }
    }
    app.update(Action::SelectMode(Mode::Browse));
    Ok(app)
}

#[test]
fn image_activation_requires_explicit_input_and_rejects_duplicate_and_stale_handoffs()
-> Result<(), Box<dyn std::error::Error>> {
    let mut app = fixture()?;
    let path = PathBuf::from("/Users/demo/Documents/image.png");
    let before = app.workspace.snapshot();
    let effects = app.update(Action::OpenFile(ViewId(1), path.clone()));
    assert_eq!(effects.len(), 1);
    let request = effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::OpenExternal(request) => Some(request),
            _ => None,
        })
        .ok_or("Image activation must hand only a file reference to the OS")?;
    assert_eq!(request.path, path);
    assert!(
        app.update(Action::OpenFile(ViewId(1), path.clone()))
            .is_empty()
    );
    assert!(app.workspace.same_durable_state(&before));
    app.update(Action::OpenAppSettings);
    for action in [
        Action::Navigate(ViewId(1), Navigate::Folder("/Users/demo".into())),
        Action::OpenFile(ViewId(1), path.clone()),
    ] {
        assert!(app.update(action).is_empty());
        assert!(app.workspace.same_durable_state(&before));
    }
    let effects = app.update(Action::ExternalOpened(OpenCompletion {
        request: request.clone(),
        result: Err("Controlled image handoff failure".into()),
    }));
    assert!(
        effects.is_empty(),
        "A receipt must never launch an application"
    );
    assert_eq!(
        app.external_open_status(ViewId(1)),
        Some("Controlled image handoff failure")
    );
    app.update(Action::CancelAppSettings);
    let retry = app
        .update(Action::OpenFile(ViewId(1), path))
        .into_iter()
        .find_map(|effect| match effect {
            Effect::OpenExternal(request) => Some(request),
            _ => None,
        })
        .ok_or("Explicit retry must issue a fresh handoff")?;
    assert_ne!(retry.generation, request.generation);
    assert!(
        app.update(Action::ExternalOpened(OpenCompletion {
            request,
            result: Ok(()),
        }))
        .is_empty()
    );
    assert!(matches!(
        app.external_opening().map(|opening| &opening.phase),
        Some(Opening::Pending(_))
    ));
    assert!(
        app.update(Action::ExternalOpened(OpenCompletion {
            request: retry,
            result: Ok(()),
        }))
        .is_empty()
    );
    assert_eq!(
        app.external_opening().map(|opening| &opening.phase),
        Some(&Opening::HandedOff)
    );
    assert!(app.workspace.same_durable_state(&before));
    Ok(())
}
#[test]
fn nonimage_open_hands_only_a_reference_to_os_without_preview_and_rejects_stale_receipts()
-> Result<(), Box<dyn std::error::Error>> {
    let mut app = fixture()?;
    let path = PathBuf::from("/Users/demo/Documents/Readme.md");
    let before = app.workspace.snapshot();
    let effects = app.update(Action::OpenFile(ViewId(1), path.clone()));
    assert_eq!(effects.len(), 1);
    let first = match &effects[0] {
        Effect::OpenExternal(request) => request.clone(),
        _ => return Err("Nonimage activation must only hand a file reference to the OS".into()),
    };
    assert_eq!(first.path, path);
    assert!(app.workspace.same_durable_state(&before));
    assert!(
        app.update(Action::OpenFile(ViewId(1), path.clone()))
            .is_empty()
    );
    app.update(Action::OpenAppSettings);
    assert!(app.app_settings_open());
    app.update(Action::ExternalOpened(OpenCompletion {
        request: first.clone(),
        result: Err("Controlled handler failure".into()),
    }));
    assert_eq!(
        app.external_open_status(ViewId(1)),
        Some("Controlled handler failure")
    );
    assert!(
        app.update(Action::OpenFile(ViewId(1), path.clone()))
            .is_empty()
    );
    app.update(Action::CancelAppSettings);
    let second = app
        .update(Action::OpenFile(ViewId(1), path))
        .into_iter()
        .find_map(|effect| {
            if let Effect::OpenExternal(request) = effect {
                Some(request)
            } else {
                None
            }
        })
        .ok_or("Missing explicit retry")?;
    assert_ne!(first.generation, second.generation);
    app.update(Action::ExternalOpened(OpenCompletion {
        request: first,
        result: Ok(()),
    }));
    assert!(matches!(
        app.external_opening().map(|opening| &opening.phase),
        Some(Opening::Pending(_))
    ));
    app.update(Action::ExternalOpened(OpenCompletion {
        request: second.clone(),
        result: Ok(()),
    }));
    assert!(app.external_open_status(ViewId(1)).is_none());
    assert!(app.workspace.same_durable_state(&before));
    app.update(Action::SelectMode(Mode::Layout));
    assert!(
        app.update(Action::OpenFile(ViewId(1), second.path.clone()))
            .is_empty()
    );
    app.update(Action::SelectMode(Mode::Browse));
    assert!(
        app.update(Action::OpenFile(ViewId(1), "/unlisted.txt".into()))
            .is_empty()
    );
    app.update(Action::Navigate(
        ViewId(1),
        Navigate::Folder("/Users/demo".into()),
    ));
    let request = match app.load(ViewId(1)) {
        Some(LoadState::Loading { request, .. }) => request.clone(),
        _ => return Err("Missing navigation request".into()),
    };
    app.update(Action::ListingCompleted(ListingCompletion {
        result: ControlledDirectory::default().list(&request.directory),
        request,
    }));
    assert!(app.external_opening().is_none());
    app.update(Action::ExternalOpened(OpenCompletion {
        request: second,
        result: Err("Late error".into()),
    }));
    assert!(app.external_opening().is_none());
    Ok(())
}
