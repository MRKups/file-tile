//! Standard-font dropdown with a trailing Font Explorer action.
use super::style;
use crate::application::preferences::typography::Typeface;
use crate::{
    Message,
    widgets::focus_control,
    window::{appearance as a, system_font::SystemFont},
};
use iced::advanced::Renderer as _;
use iced::advanced::text::Renderer as _;
use iced::advanced::{
    Clipboard, Layout, Overlay, Shell, Widget, layout, overlay, renderer,
    widget::{Operation, Tree, tree},
};
use iced::widget::{
    button, column, container, pick_list, row, rule, space,
    text::{self, Wrapping},
};
use iced::{
    Border, Center, Color, Element, Event, Fill, Length, Point, Rectangle, Renderer, Shadow, Size,
    Theme, Vector, keyboard, mouse,
};

pub(crate) const TRIGGER_ID: &str = "app-settings-typeface";
pub(super) const EXPLORER_INDEX: usize = Typeface::STANDARD.len();
pub(super) const MENU_COUNT: usize = EXPLORER_INDEX + 1;

#[derive(Debug, Clone)]
pub(crate) enum FontPickerEdit {
    Toggle,
    Close,
    StepStandard(isize),
    MoveCursor(isize),
    ActivateCursor,
    Select(Typeface),
    OpenExplorer,
}

pub(super) fn display_label(typeface: Typeface, system_font: &SystemFont) -> String {
    if typeface == Typeface::SystemDefault {
        system_font.label()
    } else {
        typeface.to_string()
    }
}

pub(super) fn view(
    current: Typeface,
    open: bool,
    cursor_index: Option<usize>,
    palette: a::Palette,
    system_font: &SystemFont,
) -> Element<'static, Message> {
    let label = display_label(current, system_font);
    let menu = if open {
        menu_content(current, cursor_index, palette, system_font)
    } else {
        space().into()
    };
    let picker = Element::new(Picker {
        open,
        palette,
        trigger: trigger_content(label, palette),
        menu,
    });
    let (action, up, down) = if open {
        (
            FontPickerEdit::ActivateCursor,
            FontPickerEdit::MoveCursor(-1),
            FontPickerEdit::MoveCursor(1),
        )
    } else {
        (
            FontPickerEdit::Toggle,
            FontPickerEdit::StepStandard(-1),
            FontPickerEdit::StepStandard(1),
        )
    };
    let keys = [
        (
            keyboard::key::Named::ArrowUp,
            keyboard::Modifiers::empty(),
            Message::AppFontPicker(up),
        ),
        (
            keyboard::key::Named::ArrowDown,
            keyboard::Modifiers::empty(),
            Message::AppFontPicker(down),
        ),
    ]
    .into();
    focus_control::themed_keys(
        TRIGGER_ID,
        Message::AppFontPicker(action),
        picker,
        keys,
        palette,
    )
}

fn trigger_content(label: String, palette: a::Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    let arrow = iced::widget::text(Renderer::ARROW_DOWN_ICON)
        .font(Renderer::ICON_FONT)
        .size(ty.size(a::CONTROL_SIZE) * 0.7)
        .line_height(text::LineHeight::Relative(1.0))
        .shaping(text::Shaping::Basic)
        .color(palette.muted);
    container(
        row![
            ty.control(label)
                .color(palette.text)
                .wrapping(Wrapping::None)
                .width(Fill),
            arrow,
        ]
        .spacing(10)
        .align_y(Center),
    )
    .padding([9, 12])
    .width(Fill)
    .clip(true)
    .into()
}

fn menu_content(
    current: Typeface,
    cursor_index: Option<usize>,
    palette: a::Palette,
    system_font: &SystemFont,
) -> Element<'static, Message> {
    let mut list = column![].spacing(2).width(Fill);
    for (index, face) in Typeface::STANDARD.into_iter().enumerate() {
        list = list.push(menu_row(
            display_label(face, system_font),
            face == current,
            cursor_index == Some(index),
            Message::AppFontPicker(FontPickerEdit::Select(face)),
            palette,
        ));
    }
    list = list.push(container(rule::horizontal(1).style(move |_| palette.rule())).padding([4, 6]));
    list = list.push(menu_row(
        "Font Explorer".into(),
        false,
        cursor_index == Some(EXPLORER_INDEX),
        Message::AppFontPicker(FontPickerEdit::OpenExplorer),
        palette,
    ));
    let menu_style = style::picker_menu(palette);
    container(list)
        .padding(4)
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(menu_style.background),
            text_color: Some(menu_style.text_color),
            border: menu_style.border,
            shadow: Shadow {
                color: Color::BLACK.scale_alpha(0.16),
                offset: Vector::new(0.0, 6.0),
                blur_radius: 16.0,
            },
            snap: false,
        })
        .into()
}

fn menu_row(
    label: String,
    selected: bool,
    keyboard_cursor: bool,
    message: Message,
    palette: a::Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    button(
        ty.control(label)
            .color(palette.text)
            .wrapping(Wrapping::None)
            .width(Fill),
    )
    .padding([8, 10])
    .width(Fill)
    .on_press(message)
    .style(move |_, status| {
        let hovered =
            keyboard_cursor || matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: if selected {
                Some(palette.row_selected.into())
            } else if hovered {
                Some(palette.row_hover.into())
            } else {
                None
            },
            text_color: palette.text,
            border: Border {
                radius: 6.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    })
    .into()
}

#[derive(Default)]
struct State {
    hovered: bool,
}

struct Picker<'a> {
    open: bool,
    palette: a::Palette,
    trigger: Element<'a, Message>,
    menu: Element<'a, Message>,
}

impl Widget<Message, Theme, Renderer> for Picker<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.trigger), Tree::new(&self.menu)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.trigger, &self.menu]);
    }

    fn size(&self) -> Size<Length> {
        self.trigger.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.trigger
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.trigger
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _: &Renderer,
        _: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let over = cursor.is_over(layout.bounds());
        if state.hovered != over {
            state.hovered = over;
            shell.request_redraw();
        }
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && over
        {
            shell.publish(Message::AppFontPicker(FontPickerEdit::Toggle));
            shell.capture_event();
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        renderer_style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let is_hovered = cursor.is_over(layout.bounds());
        let status = if self.open {
            pick_list::Status::Opened { is_hovered }
        } else if is_hovered {
            pick_list::Status::Hovered
        } else {
            pick_list::Status::Active
        };
        let visual = style::picker(self.palette, status);
        let background = if matches!(status, pick_list::Status::Active) {
            Color::TRANSPARENT.into()
        } else {
            visual.background
        };
        renderer.fill_quad(
            renderer::Quad {
                bounds: layout.bounds(),
                border: visual.border,
                ..renderer::Quad::default()
            },
            background,
        );
        self.trigger.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            renderer_style,
            layout,
            cursor,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        _: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _: &Rectangle,
        _: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _: &Renderer,
        _: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        if !self.open {
            return None;
        }
        let bounds = layout.bounds();
        let (_, menu_tree) = tree.children.split_at_mut(1);
        Some(overlay::Element::new(Box::new(MenuOverlay {
            menu: &mut self.menu,
            tree: &mut menu_tree[0],
            anchor: Rectangle {
                x: bounds.x + translation.x,
                y: bounds.y + translation.y,
                width: bounds.width,
                height: bounds.height,
            },
        })))
    }
}

struct MenuOverlay<'a, 'b> {
    menu: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    anchor: Rectangle,
}

impl Overlay<Message, Theme, Renderer> for MenuOverlay<'_, '_> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let below = self.anchor.y + self.anchor.height + 4.0;
        let room_below = (bounds.height - below - 8.0).max(0.0);
        let room_above = (self.anchor.y - 12.0).max(0.0);
        let open_below = room_below >= 260.0 || room_below >= room_above;
        let max_height = if open_below { room_below } else { room_above }.max(120.0);
        let limits = layout::Limits::new(
            Size::new(self.anchor.width, 0.0),
            Size::new(self.anchor.width, max_height),
        );
        let node = self
            .menu
            .as_widget_mut()
            .layout(self.tree, renderer, &limits);
        let size = node.size();
        let y = if open_below {
            below
        } else {
            (self.anchor.y - size.height - 4.0).max(8.0)
        };
        let x = self
            .anchor
            .x
            .clamp(8.0, (bounds.width - size.width - 8.0).max(8.0));
        node.move_to(Point::new(x, y))
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.menu
            .as_widget_mut()
            .operate(self.tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let over_menu = cursor.is_over(layout.bounds());
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(_) | mouse::Event::WheelScrolled { .. })
        ) && !over_menu
        {
            shell.publish(Message::AppFontPicker(FontPickerEdit::Close));
            shell.capture_event();
            shell.request_redraw();
            return;
        }
        self.menu.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            &layout.bounds(),
        );
        if over_menu && matches!(event, Event::Mouse(_)) {
            shell.capture_event();
        }
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.menu.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &layout.bounds(),
        );
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.menu.as_widget().mouse_interaction(
            self.tree,
            layout,
            cursor,
            &layout.bounds(),
            renderer,
        )
    }
}
