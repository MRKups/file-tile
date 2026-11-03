use iced::advanced::Renderer as _;
use iced::advanced::{
    Clipboard, Layout, Shell, clipboard, layout, renderer, renderer::Headless, widget::Tree,
};
use iced::keyboard::{Key, key};
use iced::{Rectangle, Renderer, mouse, window};

use super::*;
use crate::application::contracts::directory::{
    DirectoryEntry, DirectoryListing, EntryKind, ListingOrigin,
};
use crate::domain::Mode;
use crate::{
    boxes::{file_entry, tab_rail},
    canvas::layout_controls,
    widgets::input_boundary,
};
mod configure;
mod context_menus;
mod input_boundaries;
mod management;
mod presentation;
mod scopes;
mod tabs;

/// Drives the production widget tree, including the overlay pass.
/// This uses headless tiny-skia, not native window/input injection.
struct WidgetHarness {
    tree: Tree,
    renderer: Renderer,
    capture_sequence: u32,
}

impl WidgetHarness {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let renderer = iced::futures::executor::block_on(Renderer::new(
            typography::Style::default().font(),
            iced::Pixels(16.0),
            Some("tiny-skia"),
        ))
        .ok_or("Headless tiny-skia renderer is unavailable")?;
        Ok(Self {
            tree: Tree::empty(),
            renderer,
            capture_sequence: 0,
        })
    }

    fn frame(
        &mut self,
        mut element: Element<'_, Message>,
        size: Size,
        cursor: mouse::Cursor,
        events: &[Event],
    ) -> Vec<Message> {
        let widget = element.as_widget_mut();
        self.tree.diff(&*widget);
        let limits = layout::Limits::new(Size::ZERO, size);
        let mut node = widget.layout(&mut self.tree, &self.renderer, &limits);
        let viewport = Rectangle::with_size(size);
        // Iced queries overlays before dispatching the next event.
        drop(widget.overlay(
            &mut self.tree,
            Layout::new(&node),
            &self.renderer,
            &viewport,
            Vector::ZERO,
        ));
        let mut messages = Vec::new();
        let clipboard: &mut dyn Clipboard = &mut clipboard::Null;
        for event in events {
            let mut shell = Shell::new(&mut messages);
            widget.update(
                &mut self.tree,
                event,
                Layout::new(&node),
                cursor,
                &self.renderer,
                clipboard,
                &mut shell,
                &viewport,
            );
            if shell.is_layout_invalid() {
                node = widget.layout(&mut self.tree, &self.renderer, &limits);
            }
        }
        drop(widget.overlay(
            &mut self.tree,
            Layout::new(&node),
            &self.renderer,
            &viewport,
            Vector::ZERO,
        ));
        widget.draw(
            &self.tree,
            &mut self.renderer,
            &appearance::theme(),
            &renderer::Style::default(),
            Layout::new(&node),
            cursor,
            &viewport,
        );
        if let Some(root) = std::env::var_os("FILE_TILE_WIDGET_CAPTURE_DIR") {
            use std::io::Write as _;
            let path = PathBuf::from(root).join(format!(
                "frame-{:02}-{}x{}.rgba",
                self.capture_sequence, size.width as u32, size.height as u32
            ));
            let pixels = self.renderer.screenshot(
                Size::new(size.width as u32, size.height as u32),
                1.0,
                appearance::resolve(Default::default())
                    .window
                    .contrast_background,
            );
            let result = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .and_then(|mut file| file.write_all(&pixels));
            assert!(
                result.is_ok(),
                "Cannot capture the synthetic widget frame: {result:?}"
            );
            self.capture_sequence += 1;
        }
        self.renderer.reset(viewport);
        messages
    }

    fn desktop_frame(
        &mut self,
        desktop: &mut Desktop,
        cursor: mouse::Cursor,
        events: &[Event],
    ) -> Vec<Message> {
        let messages = self.frame(desktop.view(), desktop.size, cursor, events);
        for message in &messages {
            let _ = desktop.update(message.clone());
        }
        // Preserve production scoped delivery above; expose the interaction to
        // existing assertions without making every test match the routing envelope.
        messages
            .into_iter()
            .map(|message| match message {
                Message::WorkspaceEpoch(_, message) => *message,
                message => message,
            })
            .collect()
    }

    fn operate<T>(
        &mut self,
        mut element: Element<'_, Message>,
        size: Size,
        operation: &mut impl iced::advanced::widget::Operation<T>,
    ) -> iced::advanced::widget::operation::Outcome<T> {
        let widget = element.as_widget_mut();
        self.tree.diff(&*widget);
        let node = widget.layout(
            &mut self.tree,
            &self.renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        widget.operate(
            &mut self.tree,
            Layout::new(&node),
            &self.renderer,
            &mut iced::advanced::widget::operation::black_box(operation),
        );
        operation.finish()
    }

    fn bounds(
        &mut self,
        desktop: &Desktop,
        id: impl Into<iced::advanced::widget::Id>,
    ) -> Result<Rectangle, Box<dyn std::error::Error>> {
        let id = id.into();
        let mut probe = BoundsProbe::default();
        self.operate(desktop.view(), desktop.size, &mut probe);
        probe
            .controls
            .get(&id)
            .copied()
            .ok_or_else(|| format!("Missing control {id:?}").into())
    }
}

#[derive(Default)]
struct BoundsProbe {
    controls: HashMap<iced::advanced::widget::Id, Rectangle>,
    labels: Vec<(String, Rectangle)>,
}

impl iced::advanced::widget::Operation for BoundsProbe {
    fn focusable(
        &mut self,
        id: Option<&iced::advanced::widget::Id>,
        bounds: Rectangle,
        _: &mut dyn iced::advanced::widget::operation::Focusable,
    ) {
        if let Some(id) = id {
            self.controls.insert(id.clone(), bounds);
        }
    }
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation)) {
        operate(self);
    }
    fn container(&mut self, id: Option<&iced::advanced::widget::Id>, bounds: Rectangle) {
        if let Some(id) = id {
            self.controls.insert(id.clone(), bounds);
        }
    }
    fn text(&mut self, _: Option<&iced::advanced::widget::Id>, bounds: Rectangle, text: &str) {
        self.labels.push((text.into(), bounds));
    }
    fn scrollable(
        &mut self,
        id: Option<&iced::advanced::widget::Id>,
        bounds: Rectangle,
        _: Rectangle,
        _: Vector,
        _: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        if let Some(id) = id {
            self.controls.insert(id.clone(), bounds);
        }
    }
}

fn listing(directory: &str, count: usize) -> LoadState {
    LoadState::Ready(Arc::new(DirectoryListing::new(
        directory.into(),
        ListingOrigin::Controlled,
        (0..count)
            .map(|index| {
                let name = format!("folder-{index:03}");
                DirectoryEntry {
                    path: PathBuf::from(directory).join(&name),
                    name: name.into(),
                    kind: EntryKind::Directory,
                    hidden: false,
                    symlink: false,
                    inode: None,
                }
            })
            .collect(),
        vec![],
    )))
}

#[test]
fn minimum_window_external_resize_targets_take_priority_over_removal()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::domain::{BoxId, LayoutKind, ResizeHandle};
    let mut desktop = app_settings::tests::ready_desktop(None);
    desktop.size = window_shell::MIN_SIZE;
    let _ = desktop.update(Message::Action(Action::SelectMode(Mode::Layout)));
    let mut harness = WidgetHarness::new()?;
    let geometry = desktop.geometry();
    let world = geometry.rect(desktop.application.workspace().boxes()[0].rect);
    let bounds = Rectangle {
        x: geometry.viewport.x,
        y: geometry.viewport.y,
        ..world
    };
    let boxes = desktop.application.workspace().boxes().to_vec();
    let external = Point::new(bounds.center_x(), bounds.y - 4.0);
    assert!(!bounds.contains(external));
    assert!(layout_controls::handle_bounds(bounds, ResizeHandle::North).contains(external));
    let overlap = Point::new(bounds.x + bounds.width - 12.0, bounds.y + 14.0);
    assert!(layout_controls::remove_bounds(bounds).contains(overlap));
    for (point, handle) in [
        (external, ResizeHandle::North),
        (overlap, ResizeHandle::NorthEast),
    ] {
        let messages = harness.desktop_frame(
            &mut desktop,
            mouse::Cursor::Available(point),
            &[Event::Mouse(mouse::Event::ButtonPressed(
                mouse::Button::Left,
            ))],
        );
        assert!(messages.iter().any(|message| matches!(message, Message::LayoutPressed { id: BoxId(1), kind: LayoutKind::Resize(selected), .. } if *selected == handle)), "{handle:?}");
        assert!(!messages.iter().any(|message| matches!(
            message,
            Message::Action(Action::RemoveBox(_) | Action::RemoveSelected)
        )));
        let _ = desktop.dispatch(Action::CancelPointer);
        harness.desktop_frame(&mut desktop, mouse::Cursor::Unavailable, &[]);
        assert_eq!(desktop.application.workspace().boxes(), boxes);
        assert!(desktop.application.workspace().layout_preview().is_none());
    }
    Ok(())
}

#[test]
fn row_identity_preserves_hover_on_rebuild_but_resets_for_another_listing_or_view()
-> Result<(), Box<dyn std::error::Error>> {
    let mut harness = WidgetHarness::new()?;
    let first = listing("/fixture/first", 64);
    let second = listing("/fixture/second", 64);
    let cursor = Point::new(30.0, 17.0);
    let events = [
        Event::Mouse(mouse::Event::CursorMoved { position: cursor }),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
    ];
    let mut props = file_list::Props {
        order: None,
        order_revision: 0,
        usage: None,
        folder_usages: None,
        selection: None,
        thumbnails: None,
        palette: appearance::resolve(Default::default()).boxes,
        metadata_paths: None,
        clipped_left: 0.0,
        clipped_width: 400.0,
        metadata: None,
        settings: crate::domain::BoxSettings::default(),
        width: 400.0,
        scope: &crate::domain::Scope::default(),
        view: ViewId(1),
        load: Some(&first),
        selected: None,
        hovered: None,
        interactive: true,
        window: file_list::ScrollWindow {
            offset: 0.0,
            height: 340.0,
        },
        revision: 1,
        show_hidden: false,
        motion: None,
        dragging: None,
        column_preview: None,
    };
    let mut frame = |props, events: &[Event]| {
        harness.frame(
            file_list::content(props),
            Size::new(400.0, 3000.0),
            mouse::Cursor::Available(cursor),
            events,
        )
    };
    let messages = frame(props, &events);
    assert!(messages.iter().any(|message| matches!(message,
        Message::EntryHovered(ViewId(1), path, true) if path == &PathBuf::from("/fixture/first/folder-000")
    )));
    props.hovered = Some(std::path::Path::new("/fixture/first/folder-000"));
    assert!(frame(props, &events[..1]).is_empty());

    props.load = Some(&second);
    props.revision = 2;
    let messages = frame(props, &events);
    assert!(messages.iter().any(|message| matches!(message,
        Message::EntryHovered(ViewId(1), path, true) if path == &PathBuf::from("/fixture/second/folder-000")
    )));
    assert!(messages.iter().any(|message| matches!(message,
        Message::EntrySelected(ViewId(1), path) if path == &PathBuf::from("/fixture/second/folder-000")
    )));
    assert!(
        !messages
            .iter()
            .any(|message| matches!(message, Message::EntryOpened(..)))
    );

    props.view = ViewId(2);
    assert!(
        frame(props, &events[..1])
            .iter()
            .any(|message| matches!(message, Message::EntryHovered(ViewId(2), _, true)))
    );
    props.interactive = false;
    assert!(frame(props, &events).is_empty());
    props.interactive = true;
    assert!(
        frame(props, &events[..1])
            .iter()
            .any(|message| matches!(message, Message::EntryHovered(ViewId(2), _, true)))
    );
    Ok(())
}
