//! Shortcut icon dropdown displaying each option's vector glyph beside its label.
use super::Event;
use crate::{
    Message,
    dialogs::app_settings::style,
    domain::LauncherIcon,
    widgets::focus_control,
    window::{
        appearance::Palette,
        icons::{self, Icon},
    },
};
use iced::advanced::Renderer as _;
use iced::advanced::{
    Clipboard, Layout, Overlay, Shell, Widget, layout, overlay, renderer,
    widget::{Operation, Tree, tree},
};
use iced::{
    Border, Center, Color, Element, Event as IcedEvent, Fill, Length, Point, Rectangle, Renderer,
    Shadow, Size, Theme, Vector, keyboard, mouse,
    widget::{button, column, container, pick_list, row},
};

pub(super) fn choice_icon(choice: LauncherIcon) -> Icon {
    match choice {
        LauncherIcon::Automatic => Icon::External,
        LauncherIcon::Application => Icon::Application,
        LauncherIcon::File => Icon::Document,
        LauncherIcon::Folder => Icon::Folder,
        LauncherIcon::Link => Icon::Link,
    }
}

pub(super) fn view(
    generation: u64,
    current: LauncherIcon,
    palette: Palette,
) -> Element<'static, Message> {
    let index = LauncherIcon::ALL
        .iter()
        .position(|icon| *icon == current)
        .unwrap_or(0);
    let last = LauncherIcon::ALL.len() - 1;
    let keys = [
        (keyboard::key::Named::ArrowUp, index.saturating_sub(1)),
        (keyboard::key::Named::ArrowDown, (index + 1).min(last)),
        (keyboard::key::Named::Home, 0),
        (keyboard::key::Named::End, last),
    ]
    .into_iter()
    .map(|(key, idx)| {
        (
            key,
            keyboard::Modifiers::empty(),
            Message::Launcher(Event::Icon(generation, LauncherIcon::ALL[idx])),
        )
    })
    .collect();
    let next = LauncherIcon::ALL[(index + 1) % LauncherIcon::ALL.len()];
    let picker = Element::new(Picker {
        generation,
        palette,
        trigger: trigger_content(current, palette),
        menu: menu_content(generation, current, palette),
    });
    focus_control::exclusive_keys(
        "launcher-icon",
        Message::Launcher(Event::Icon(generation, next)),
        picker,
        keys,
        palette,
    )
}

fn trigger_content(current: LauncherIcon, palette: Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    container(
        row![
            icons::view(choice_icon(current), ty.icon(15.0), Some(palette.text), 1.0),
            ty.control(current.to_string())
                .color(palette.text)
                .width(Fill),
            ty.text("▾").color(palette.muted),
        ]
        .spacing(10)
        .align_y(Center),
    )
    .padding([10, 14])
    .width(Fill)
    .into()
}

fn menu_content(
    generation: u64,
    current: LauncherIcon,
    palette: Palette,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let mut list = column![].spacing(2).width(Fill);
    for option in LauncherIcon::ALL {
        let selected = option == current;
        let item = button(
            row![
                icons::view(choice_icon(option), ty.icon(15.0), Some(palette.text), 1.0),
                ty.control(option.to_string())
                    .color(palette.text)
                    .width(Fill),
            ]
            .spacing(10)
            .align_y(Center),
        )
        .padding([8, 10])
        .width(Fill)
        .on_press(Message::Launcher(Event::Icon(generation, option)))
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
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
        });
        list = list.push(item);
    }
    let menu_style = style::picker_menu(palette);
    container(list)
        .padding(4)
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(menu_style.background),
            text_color: Some(menu_style.text_color),
            border: menu_style.border,
            shadow: Shadow {
                color: Color::BLACK.scale_alpha(0.14),
                offset: Vector::new(0.0, 6.0),
                blur_radius: 16.0,
            },
            snap: false,
        })
        .into()
}

#[derive(Default)]
struct State {
    generation: u64,
    open: bool,
    hovered: bool,
}

struct Picker<'a> {
    generation: u64,
    palette: Palette,
    trigger: Element<'a, Message>,
    menu: Element<'a, Message>,
}

impl Widget<Message, Theme, Renderer> for Picker<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State {
            generation: self.generation,
            open: false,
            hovered: false,
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.trigger), Tree::new(&self.menu)]
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.generation != self.generation {
            state.generation = self.generation;
            state.open = false;
        }
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
        event: &IcedEvent,
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
        if let IcedEvent::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && over
        {
            state.open = !state.open;
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
        let state = tree.state.downcast_ref::<State>();
        let is_hovered = cursor.is_over(layout.bounds());
        let status = if state.open {
            pick_list::Status::Opened { is_hovered }
        } else if is_hovered {
            pick_list::Status::Hovered
        } else {
            pick_list::Status::Active
        };
        let visual = style::picker(self.palette, status);
        renderer.fill_quad(
            renderer::Quad {
                bounds: layout.bounds(),
                border: visual.border,
                ..renderer::Quad::default()
            },
            visual.background,
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
        let state = tree.state.downcast_mut::<State>();
        if !state.open {
            return None;
        }
        let bounds = layout.bounds();
        let (_, menu_tree) = tree.children.split_at_mut(1);
        Some(overlay::Element::new(Box::new(MenuOverlay {
            open: &mut state.open,
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
    open: &'b mut bool,
    menu: &'b mut Element<'a, Message>,
    tree: &'b mut Tree,
    anchor: Rectangle,
}

impl Overlay<Message, Theme, Renderer> for MenuOverlay<'_, '_> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let limits = layout::Limits::new(
            Size::new(self.anchor.width, 0.0),
            Size::new(self.anchor.width, (bounds.height - 16.0).max(1.0)),
        );
        let node = self
            .menu
            .as_widget_mut()
            .layout(self.tree, renderer, &limits);
        let size = node.size();
        let below = self.anchor.y + self.anchor.height + 4.0;
        let y = if below + size.height <= bounds.height - 8.0 {
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
    fn update(
        &mut self,
        event: &IcedEvent,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        if let IcedEvent::Keyboard(keyboard::Event::KeyPressed {
            key:
                keyboard::Key::Named(
                    keyboard::key::Named::Escape
                    | keyboard::key::Named::Enter
                    | keyboard::key::Named::Space,
                ),
            repeat: false,
            ..
        }) = event
        {
            *self.open = false;
            shell.capture_event();
            shell.request_redraw();
            return;
        }
        if matches!(
            event,
            IcedEvent::Mouse(mouse::Event::ButtonPressed(_) | mouse::Event::WheelScrolled { .. })
        ) && !cursor.is_over(layout.bounds())
        {
            *self.open = false;
            shell.capture_event();
            shell.request_redraw();
            return;
        }
        let mut published = Vec::new();
        let mut local = Shell::new(&mut published);
        self.menu.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            &mut local,
            &layout.bounds(),
        );
        let selected = std::cell::Cell::new(false);
        shell.merge(local, |message| {
            selected.set(true);
            message
        });
        if selected.get() {
            *self.open = false;
            shell.request_redraw();
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
