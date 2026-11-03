//! Editing-only hit targets and focus, using the same projection as box painting.

use iced::advanced::Renderer as _;
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, renderer,
    widget::{Id, Operation, Tree, operation::Focusable, tree},
};
use iced::{
    Border, Element, Event, Fill, Length, Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
};

use crate::application::{Action, Application};
use crate::domain::{BoxId, GridRect, LayoutKind, Mode, ResizeHandle, ViewId, Workspace};
use crate::{Message, boxes::tab_rail, canvas::geometry::CanvasGeometry, window::appearance as a};

const HANDLE_TARGET: f32 = 36.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Body(BoxId),
    Resize(BoxId, ResizeHandle),
    Remove(BoxId),
    Tab(BoxId, ViewId),
}

pub(crate) fn box_id(id: BoxId) -> Id {
    format!("layout-box-{}", id.0).into()
}
pub(crate) fn handle_id(id: BoxId, index: usize) -> Id {
    format!("layout-box-{}-handle-{index}", id.0).into()
}

fn bounds(geometry: CanvasGeometry, item: crate::domain::ItemPlacement) -> Rectangle {
    projected(geometry, item.rect)
}

fn projected(geometry: CanvasGeometry, rect: GridRect) -> Rectangle {
    let rect = geometry.rect(rect);
    Rectangle {
        x: rect.x + geometry.viewport.x - geometry.offset.x,
        y: rect.y + geometry.viewport.y - geometry.offset.y,
        ..rect
    }
}

pub(crate) fn handle_bounds(rect: Rectangle, handle: ResizeHandle) -> Rectangle {
    let x = if handle.west() {
        rect.x
    } else if handle.east() {
        rect.x + rect.width
    } else {
        rect.x + rect.width / 2.0
    };
    let y = if handle.north() {
        rect.y
    } else if handle.south() {
        rect.y + rect.height
    } else {
        rect.y + rect.height / 2.0
    };
    Rectangle {
        x: x - HANDLE_TARGET / 2.0,
        y: y - HANDLE_TARGET / 2.0,
        width: HANDLE_TARGET,
        height: HANDLE_TARGET,
    }
}

pub(crate) fn remove_bounds(rect: Rectangle) -> Rectangle {
    Rectangle {
        x: rect.x + rect.width - 32.0,
        y: rect.y + 8.0,
        width: 24.0,
        height: 24.0,
    }
}

pub(crate) fn hit(workspace: &Workspace, geometry: CanvasGeometry, point: Point) -> Option<Target> {
    hit_with_offsets(
        workspace,
        geometry,
        point,
        &std::collections::HashMap::new(),
    )
}
pub(crate) fn hit_with_offsets(
    workspace: &Workspace,
    geometry: CanvasGeometry,
    point: Point,
    offsets: &std::collections::HashMap<BoxId, f32>,
) -> Option<Target> {
    if workspace.mode() == Mode::Configure {
        return workspace.placements().rev().find_map(|item| {
            let rect = bounds(geometry, item);
            if workspace.configuring(item.id) && remove_bounds(rect).contains(point) {
                Some(Target::Remove(item.id))
            } else if rect.contains(point) {
                Some(Target::Body(item.id))
            } else {
                None
            }
        });
    }
    if workspace.mode() != Mode::Layout {
        return None;
    }
    let nearest = workspace
        .placements()
        .flat_map(|item| {
            ResizeHandle::ALL
                .into_iter()
                .map(move |handle| (item, handle))
        })
        .filter_map(|(item, handle)| {
            let target = handle_bounds(bounds(geometry, item), handle);
            target.contains(point).then_some((
                item.id,
                handle,
                (point.x - target.center_x()).powi(2) + (point.y - target.center_y()).powi(2),
            ))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2));
    if let Some((id, handle, _)) = nearest {
        return Some(Target::Resize(id, handle));
    }
    for item in workspace.placements().rev() {
        let rect = bounds(geometry, item);
        if remove_bounds(rect).contains(point) {
            return Some(Target::Remove(item.id));
        }
        if let Some(folder) = workspace.boxes().iter().find(|folder| folder.id == item.id)
            && let Some((view, _)) = tab_rail::row_at(
                folder,
                rect,
                offsets.get(&item.id).copied().unwrap_or(0.0),
                point,
                geometry.typography,
            )
        {
            return Some(Target::Tab(item.id, view));
        }
        if rect.contains(point) {
            return Some(Target::Body(item.id));
        }
    }
    None
}

fn empty_hover(workspace: &Workspace, geometry: CanvasGeometry, point: Point) -> Option<Rectangle> {
    if workspace.mode() != Mode::Layout
        || workspace.draw_preview().is_some()
        || workspace.layout_preview().is_some()
        || !geometry.can_start(point)
        || hit(workspace, geometry, point).is_some()
    {
        return None;
    }
    let cell = geometry.cell(point);
    if workspace.placements().any(|item| item.rect.contains(cell)) {
        return None;
    }
    Some(projected(
        geometry,
        GridRect {
            origin: cell,
            width: 1,
            height: 1,
        },
    ))
}

#[derive(Clone, Copy, Default)]
struct Focus(bool);
impl Focusable for Focus {
    fn is_focused(&self) -> bool {
        self.0
    }
    fn focus(&mut self) {
        self.0 = true;
    }
    fn unfocus(&mut self) {
        self.0 = false;
    }
}
struct BoxFocus {
    id: BoxId,
    controls: [Focus; 10],
}
#[derive(Default)]
struct State {
    boxes: Vec<BoxFocus>,
    mode: Option<Mode>,
}

struct Controls<'a> {
    palettes: a::Palettes,
    application: &'a Application,
    geometry: CanvasGeometry,
    enabled: bool,
    offsets: &'a std::collections::HashMap<BoxId, f32>,
    tab_dragging: bool,
}

pub(crate) fn view<'a>(
    application: &'a Application,
    geometry: CanvasGeometry,
    enabled: bool,
    offsets: &'a std::collections::HashMap<BoxId, f32>,
    tab_dragging: bool,
    palettes: a::Palettes,
) -> Element<'a, Message> {
    Element::new(Controls {
        palettes,
        application,
        geometry,
        enabled: enabled && application.workspace().mode() != Mode::Browse,
        offsets,
        tab_dragging,
    })
}

impl Widget<Message, Theme, Renderer> for Controls<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State {
            mode: Some(self.application.workspace().mode()),
            boxes: if self.enabled {
                self.application
                    .workspace()
                    .placements()
                    .map(|item| BoxFocus {
                        id: item.id,
                        controls: [Focus::default(); 10],
                    })
                    .collect()
            } else {
                Vec::new()
            },
        })
    }
    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.mode != Some(self.application.workspace().mode()) {
            state.boxes.clear();
            state.mode = Some(self.application.workspace().mode());
        }
        if !self.enabled {
            state.boxes.clear();
            return;
        }
        state.boxes.retain(|focus| {
            self.application
                .workspace()
                .placements()
                .any(|item| item.id == focus.id)
        });
        for item in self.application.workspace().placements() {
            if !state.boxes.iter().any(|focus| focus.id == item.id) {
                state.boxes.push(BoxFocus {
                    id: item.id,
                    controls: [Focus::default(); 10],
                });
            }
        }
        if self.application.workspace().mode() == Mode::Configure
            && self.application.workspace().selected().is_some()
        {
            for focus in &mut state.boxes {
                for control in &mut focus.controls[..9] {
                    control.unfocus();
                }
            }
        }
    }
    fn size(&self) -> Size<Length> {
        Size::new(Fill, Fill)
    }
    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.resolve(Fill, Fill, Size::ZERO))
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        _layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        if !self.enabled {
            return;
        }
        for focus in &mut tree.state.downcast_mut::<State>().boxes {
            let Some(item) = self
                .application
                .workspace()
                .placements()
                .find(|item| item.id == focus.id)
            else {
                continue;
            };
            let rect = bounds(self.geometry, item);
            if self.application.workspace().mode() == Mode::Configure {
                if self.application.workspace().selected().is_none() {
                    operation.focusable(Some(&box_id(item.id)), rect, &mut focus.controls[0]);
                } else if self.application.workspace().configuring(item.id) {
                    operation.focusable(
                        Some(&handle_id(item.id, 8)),
                        remove_bounds(rect),
                        &mut focus.controls[9],
                    );
                }
                continue;
            }
            operation.focusable(Some(&box_id(item.id)), rect, &mut focus.controls[0]);
            for (index, handle) in ResizeHandle::ALL.into_iter().enumerate() {
                operation.focusable(
                    Some(&handle_id(item.id, index)),
                    handle_bounds(rect, handle),
                    &mut focus.controls[index + 1],
                );
            }
            operation.focusable(
                Some(&handle_id(item.id, 8)),
                remove_bounds(rect),
                &mut focus.controls[9],
            );
        }
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if !self.enabled {
            return;
        }
        if self.application.workspace().mode() == Mode::Configure {
            self.update_configure(tree, event, layout, cursor, shell);
            return;
        }
        match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) if self.tab_dragging => {
                shell.publish(Message::MoveTabDrag(*position));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if self.tab_dragging =>
            {
                shell.publish(Message::FinishTabDrag);
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { position })
                if self.application.workspace().layout_preview().is_some() =>
            {
                shell.publish(Message::LayoutPointerMoved(*position));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::CursorMoved { position })
                if self.application.workspace().draw_preview().is_some() =>
            {
                shell.publish(Message::Action(Action::DrawTo(
                    self.geometry.cell(*position),
                )));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if self.application.workspace().layout_preview().is_some() =>
            {
                shell.publish(Message::FinishLayout);
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if self.application.workspace().draw_preview().is_some() =>
            {
                shell.publish(Message::Action(Action::FinishDraw));
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(point) = cursor
                    .position()
                    .filter(|point| layout.bounds().contains(*point))
                else {
                    return;
                };
                let target = hit_with_offsets(
                    self.application.workspace(),
                    self.geometry,
                    point,
                    self.offsets,
                );
                Self::focus_target(tree.state.downcast_mut::<State>(), target);
                match target {
                    Some(Target::Remove(id)) => {
                        shell.publish(Message::Action(Action::RemoveBox(id)))
                    }
                    Some(Target::Tab(id, view)) => {
                        shell.publish(Message::BeginTabDrag(id, view, point))
                    }
                    Some(Target::Resize(id, handle)) => shell.publish(Message::LayoutPressed {
                        id,
                        kind: LayoutKind::Resize(handle),
                        position: point,
                    }),
                    Some(Target::Body(id)) => shell.publish(Message::LayoutPressed {
                        id,
                        kind: LayoutKind::Move,
                        position: point,
                    }),
                    None if self.geometry.can_start(point) => shell.publish(Message::Action(
                        Action::BeginItemDraw(self.geometry.cell(point), self.geometry.extent),
                    )),
                    None => shell.publish(Message::Action(Action::SelectBox(None))),
                }
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                repeat: false,
                ..
            }) if modifiers.is_empty() => {
                if let Some(action) = control_key(tree.state.downcast_ref::<State>(), key) {
                    shell.publish(Message::Action(action));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) => {
                shell.request_redraw()
            }
            _ => {}
        }
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if !self.enabled {
            return;
        }
        let Some(clip) = layout.bounds().intersection(viewport) else {
            return;
        };
        let palette = self.palettes.boxes;
        let window_palette = self.palettes.window;
        renderer.with_layer(clip, |renderer| {
            self.draw_hover(renderer, cursor, clip);
            if let Some(point) = cursor.position().filter(|point| clip.contains(*point))
                && let Some(rect) = empty_hover(self.application.workspace(), self.geometry, point)
            {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: rect,
                        border: Border {
                            color: window_palette.selected_border,
                            width: 1.0,
                            radius: 10.0.into(),
                        },
                        ..renderer::Quad::default()
                    },
                    window_palette.grid_hover,
                );
            }
            for item in self.application.workspace().placements() {
                let rect = bounds(self.geometry, item);
                if rect.expand(18.0).intersection(&clip).is_none() {
                    continue;
                }
                let focus = tree
                    .state
                    .downcast_ref::<State>()
                    .boxes
                    .iter()
                    .find(|focus| focus.id == item.id);
                if self.application.workspace().mode() == Mode::Configure {
                    draw_configure(
                        renderer,
                        rect,
                        focus,
                        self.application.workspace().configuring(item.id),
                        cursor,
                        palette,
                    );
                    continue;
                }
                for (index, handle) in ResizeHandle::ALL.into_iter().enumerate() {
                    let target = handle_bounds(rect, handle);
                    if cursor.is_over(target)
                        || focus.is_some_and(|focus| focus.controls[index + 1].is_focused())
                    {
                        let grip = Rectangle {
                            x: target.center_x() - 7.0,
                            y: target.center_y() - 7.0,
                            width: 14.0,
                            height: 14.0,
                        };
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: grip,
                                border: Border {
                                    color: palette.tool_active_icon,
                                    width: 1.0,
                                    radius: 4.0.into(),
                                },
                                ..renderer::Quad::default()
                            },
                            palette.tool_active,
                        );
                    }
                }
                if cursor.is_over(rect)
                    || focus.is_some_and(|focus| focus.controls.iter().any(Focusable::is_focused))
                {
                    draw_remove(renderer, remove_bounds(rect), palette);
                }
            }
        });
    }
    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if !self.enabled {
            return mouse::Interaction::None;
        }
        if let Some(preview) = self.application.workspace().layout_preview() {
            return match preview.kind {
                LayoutKind::Move => mouse::Interaction::Grabbing,
                LayoutKind::Resize(handle) => resize_cursor(handle),
            };
        }
        let Some(point) = cursor
            .position()
            .filter(|point| layout.bounds().contains(*point))
        else {
            return mouse::Interaction::None;
        };
        if self.application.workspace().mode() == Mode::Configure {
            return match hit(self.application.workspace(), self.geometry, point) {
                Some(Target::Remove(_)) => mouse::Interaction::Pointer,
                Some(Target::Body(_)) if self.application.workspace().selected().is_none() => {
                    mouse::Interaction::Pointer
                }
                _ => mouse::Interaction::None,
            };
        }
        match hit(self.application.workspace(), self.geometry, point) {
            Some(Target::Resize(_, handle)) => resize_cursor(handle),
            Some(Target::Remove(_)) => mouse::Interaction::Pointer,
            Some(Target::Body(_) | Target::Tab(..)) => mouse::Interaction::Grab,
            None if self.geometry.can_start(point) => mouse::Interaction::Crosshair,
            _ => mouse::Interaction::None,
        }
    }
}

impl Controls<'_> {
    fn draw_hover(&self, renderer: &mut Renderer, cursor: mouse::Cursor, clip: Rectangle) {
        let workspace = self.application.workspace();
        if self.tab_dragging
            || workspace.draw_preview().is_some()
            || workspace.layout_preview().is_some()
        {
            return;
        }
        let Some(viewport_clip) = self.geometry.viewport.intersection(&clip) else {
            return;
        };
        let Some(point) = cursor
            .position()
            .filter(|point| viewport_clip.contains(*point))
        else {
            return;
        };
        let Some(item) = workspace
            .placements()
            .rev()
            .find(|item| bounds(self.geometry, *item).contains(point))
        else {
            return;
        };
        let glow_clip = self
            .geometry
            .viewport
            .expand(6.0)
            .intersection(&clip)
            .unwrap_or(viewport_clip);
        let rect = bounds(self.geometry, item);
        let accent = self.palettes.window.selected_border;
        renderer.with_layer(glow_clip, |renderer| {
            for (spread, alpha) in [(4.5, 0.06), (3.0, 0.12), (1.5, 0.22)] {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: rect.expand(spread),
                        border: Border {
                            color: accent.scale_alpha(alpha),
                            width: spread,
                            radius: (10.0 + spread).into(),
                        },
                        ..renderer::Quad::default()
                    },
                    iced::Color::TRANSPARENT,
                );
            }
            renderer.fill_quad(
                renderer::Quad {
                    bounds: rect,
                    border: Border {
                        color: accent,
                        width: 1.0,
                        radius: 10.0.into(),
                    },
                    ..renderer::Quad::default()
                },
                iced::Color::TRANSPARENT,
            );
        });
    }

    fn focus_target(state: &mut State, target: Option<Target>) {
        for focus in &mut state.boxes {
            for control in &mut focus.controls {
                control.unfocus();
            }
            match target {
                Some(Target::Resize(id, handle)) if id == focus.id => {
                    if let Some(index) = ResizeHandle::ALL
                        .iter()
                        .position(|candidate| *candidate == handle)
                    {
                        focus.controls[index + 1].focus();
                    }
                }
                Some(Target::Remove(id)) if id == focus.id => focus.controls[9].focus(),
                Some(Target::Body(id)) if id == focus.id => focus.controls[0].focus(),
                _ => {}
            }
        }
    }
    fn update_configure(
        &self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        shell: &mut Shell<'_, Message>,
    ) {
        let workspace = self.application.workspace();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(point) = cursor
                    .position()
                    .filter(|point| layout.bounds().contains(*point))
                else {
                    return;
                };
                let target = hit(workspace, self.geometry, point);
                if matches!(target, Some(Target::Body(id)) if workspace.configuring(id)) {
                    return;
                }
                for focus in &mut tree.state.downcast_mut::<State>().boxes {
                    for control in &mut focus.controls {
                        control.unfocus();
                    }
                    if target == Some(Target::Body(focus.id)) {
                        focus.controls[0].focus();
                    }
                }
                let action = match target {
                    Some(Target::Remove(id)) => Action::RemoveBox(id),
                    Some(Target::Body(id)) => Action::ConfigureBox(Some(id)),
                    _ => Action::ConfigureBox(None),
                };
                shell.publish(Message::Action(action));
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                repeat: false,
                ..
            }) if modifiers.is_empty() => {
                if let Some(action) = control_key(tree.state.downcast_ref::<State>(), key) {
                    shell.publish(Message::Action(match action {
                        Action::SelectBox(id) => Action::ConfigureBox(id),
                        action => action,
                    }));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) => {
                shell.request_redraw()
            }
            _ => {}
        }
    }
}

fn draw_configure(
    renderer: &mut Renderer,
    rect: Rectangle,
    focus: Option<&BoxFocus>,
    selected: bool,
    cursor: mouse::Cursor,
    palette: a::Palette,
) {
    if selected
        && (cursor.is_over(remove_bounds(rect))
            || focus.is_some_and(|focus| focus.controls[9].is_focused()))
    {
        draw_remove(renderer, remove_bounds(rect), palette);
    }
}

fn draw_remove(renderer: &mut Renderer, target: Rectangle, palette: a::Palette) {
    renderer.fill_quad(
        renderer::Quad {
            bounds: target,
            border: Border {
                radius: 6.0.into(),
                ..Border::default()
            },
            ..renderer::Quad::default()
        },
        palette.remove_fill,
    );
    draw_cross(renderer, target, palette);
}

fn draw_cross(renderer: &mut Renderer, target: Rectangle, palette: a::Palette) {
    // Embedded geometry rather than a font-dependent cross or external image.
    for index in 0..9 {
        for y in [index, 8 - index] {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle {
                        x: target.center_x() - 4.5 + index as f32,
                        y: target.center_y() - 4.5 + y as f32,
                        width: 1.5,
                        height: 1.5,
                    },
                    ..renderer::Quad::default()
                },
                palette.error,
            );
        }
    }
}

fn control_key(state: &State, key: &keyboard::Key) -> Option<Action> {
    let focus = state
        .boxes
        .iter()
        .find(|focus| focus.controls.iter().any(Focusable::is_focused))?;
    match key {
        keyboard::Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::Space)
            if focus.controls[9].is_focused() =>
        {
            Some(Action::RemoveBox(focus.id))
        }
        keyboard::Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::Space)
            if focus.controls[0].is_focused() =>
        {
            Some(Action::SelectBox(Some(focus.id)))
        }
        _ => None,
    }
}

fn resize_cursor(handle: ResizeHandle) -> mouse::Interaction {
    match handle {
        ResizeHandle::North | ResizeHandle::South => mouse::Interaction::ResizingVertically,
        ResizeHandle::East | ResizeHandle::West => mouse::Interaction::ResizingHorizontally,
        ResizeHandle::NorthEast | ResizeHandle::SouthWest => {
            mouse::Interaction::ResizingDiagonallyUp
        }
        _ => mouse::Interaction::ResizingDiagonallyDown,
    }
}
