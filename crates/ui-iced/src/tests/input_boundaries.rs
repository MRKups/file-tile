use super::*;
use iced::advanced::widget::{
    Id, Operation,
    operation::{Focusable, focusable, text_input},
};
use std::cell::Cell;

#[derive(Default)]
struct FocusProbe {
    ids: Vec<Id>,
    focused: Option<Id>,
}
impl Operation for FocusProbe {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn focusable(&mut self, id: Option<&Id>, _: Rectangle, state: &mut dyn Focusable) {
        if let Some(id) = id {
            self.ids.push(id.clone());
            if state.is_focused() {
                self.focused = Some(id.clone());
            }
        }
    }
}

fn character_key(character: &str, modifiers: keyboard::Modifiers, text: Option<&str>) -> Event {
    Event::Keyboard(keyboard::Event::KeyPressed {
        key: Key::Character(character.into()),
        modified_key: Key::Character(character.into()),
        physical_key: key::Physical::Code(key::Code::KeyC),
        location: keyboard::Location::Standard,
        modifiers,
        text: text.map(Into::into),
        repeat: false,
    })
}

#[derive(Default)]
struct ClipboardSpy {
    reads: Cell<usize>,
    writes: Vec<String>,
}

impl Clipboard for ClipboardSpy {
    fn read(&self, _: clipboard::Kind) -> Option<String> {
        self.reads.set(self.reads.get() + 1);
        Some("synthetic clipboard".into())
    }
    fn write(&mut self, _: clipboard::Kind, contents: String) {
        self.writes.push(contents);
    }
}

fn retained_editor(blocked: bool) -> Element<'static, Message> {
    input_boundary::view(
        blocked,
        iced::widget::text_input("Draft", "synthetic draft")
            .id("boundary-editor")
            .on_input(|text| Message::PathChanged(ViewId(1), text)),
    )
}

fn clipboard_frame(
    harness: &mut WidgetHarness,
    mut element: Element<'_, Message>,
    events: &[Event],
    clipboard: &mut ClipboardSpy,
) -> (Vec<Message>, bool) {
    let widget = element.as_widget_mut();
    harness.tree.diff(&*widget);
    let size = Size::new(300.0, 100.0);
    let node = widget.layout(
        &mut harness.tree,
        &harness.renderer,
        &layout::Limits::new(Size::ZERO, size),
    );
    let mut messages = Vec::new();
    let mut input_method = false;
    for event in events {
        let mut shell = Shell::new(&mut messages);
        widget.update(
            &mut harness.tree,
            event,
            Layout::new(&node),
            mouse::Cursor::Available(Point::new(5.0, 5.0)),
            &harness.renderer,
            clipboard,
            &mut shell,
            &Rectangle::with_size(size),
        );
        input_method |= shell.input_method().is_enabled();
    }
    (messages, input_method)
}

#[test]
fn blocked_editor_cannot_touch_clipboard_focus_or_selection_and_its_child_state_is_retained()
-> Result<(), Box<dyn std::error::Error>> {
    let mut harness = WidgetHarness::new()?;
    let size = Size::new(300.0, 100.0);
    harness.operate(
        retained_editor(false),
        size,
        &mut focusable::focus::<()>("boundary-editor".into()),
    );
    harness.operate(
        retained_editor(false),
        size,
        &mut text_input::select_all::<()>("boundary-editor".into()),
    );
    let mut clipboard = ClipboardSpy::default();
    let commands = [
        Event::Keyboard(keyboard::Event::ModifiersChanged(
            keyboard::Modifiers::COMMAND,
        )),
        character_key("c", keyboard::Modifiers::COMMAND, None),
        character_key("x", keyboard::Modifiers::COMMAND, None),
        character_key("v", keyboard::Modifiers::COMMAND, None),
        Event::InputMethod(iced::advanced::input_method::Event::Commit(
            "hidden input".into(),
        )),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Window(window::Event::RedrawRequested(std::time::Instant::now())),
    ];
    let (messages, input_method) = clipboard_frame(
        &mut harness,
        retained_editor(true),
        &commands,
        &mut clipboard,
    );
    assert!(messages.is_empty());
    assert!(
        !input_method,
        "The hidden editor requested the native input method"
    );
    assert_eq!(clipboard.reads.get(), 0);
    assert!(clipboard.writes.is_empty());
    harness.operate(retained_editor(true), size, &mut focusable::unfocus::<()>());
    harness.operate(
        retained_editor(true),
        size,
        &mut text_input::move_cursor_to_end::<()>("boundary-editor".into()),
    );
    let mut blocked = FocusProbe::default();
    harness.operate(retained_editor(true), size, &mut blocked);
    assert!(blocked.ids.is_empty());
    let mut restored = FocusProbe::default();
    harness.operate(retained_editor(false), size, &mut restored);
    assert_eq!(restored.focused, Some(Id::new("boundary-editor")));
    let (messages, _) = clipboard_frame(
        &mut harness,
        retained_editor(false),
        &[
            Event::Keyboard(keyboard::Event::ModifiersChanged(
                keyboard::Modifiers::empty(),
            )),
            character_key("X", keyboard::Modifiers::empty(), Some("X")),
        ],
        &mut clipboard,
    );
    assert!(matches!(messages.as_slice(), [Message::PathChanged(ViewId(1), text)] if text == "X"));
    Ok(())
}

#[test]
fn both_range_editors_block_uncaptured_workspace_undo_and_reload_shortcuts()
-> Result<(), Box<dyn std::error::Error>> {
    let mut desktop = scopes::fixture()?;
    let view = ViewId(1);
    let _ = desktop.dispatch(Action::EditScope(
        view,
        crate::application::workspace::scopes::ScopeEdit::Start("/Users/demo/Projects".into()),
    ));
    scopes::finish(&mut desktop)?;
    let _ = desktop.update(Message::ChooseBoxFolder(view));
    assert!(desktop.application.workspace().can_undo());
    assert!(matches!(
        desktop.application.load(view),
        Some(LoadState::Ready(_))
    ));
    let before = desktop.application.workspace().snapshot();
    for field in [range_editor::Field::Start, range_editor::Field::Stops] {
        let _ = desktop.update(Message::PathFocusChecked {
            focused: None,
            box_folder: false,
            boundary: Some(field),
            version: desktop.focus_version,
        });
        for modifiers in [keyboard::Modifiers::CTRL, keyboard::Modifiers::LOGO] {
            for key in ["z", "r"] {
                let task = desktop.key(Key::Character(key.into()), modifiers, Status::Ignored);
                assert_eq!(task.units(), 0, "{field:?} leaked {modifiers:?}+{key}");
                assert!(desktop.application.workspace().same_durable_state(&before));
                assert!(matches!(
                    desktop.application.load(view),
                    Some(LoadState::Ready(_))
                ));
            }
        }
    }
    Ok(())
}
