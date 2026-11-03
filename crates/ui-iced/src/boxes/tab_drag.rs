//! Same-box tab drag anchors and feedback. Domain proposals own committed ordering.

use crate::application::Action;
use crate::domain::{BoxId, Mode, TabCommand, ViewId};
use crate::{Desktop, Message, boxes::tab_rail, window::appearance as a};
use iced::widget::{container, pin, space, stack, text};
use iced::{Border, Element, Point, Rectangle, Task};

pub(crate) struct Drag {
    pub id: BoxId,
    pub view: ViewId,
    pointer: Point,
    target: Option<usize>,
    marker: Option<Rectangle>,
}

impl Desktop {
    pub(crate) fn cancel_tab_drag_for(&mut self, action: &Action) -> bool {
        if self.cancel_file_drag_for(action) {
            return true;
        }
        if matches!(action, Action::Undo) && self.tab_drag.take().is_some() {
            return true;
        }
        if matches!(
            action,
            Action::SelectMode(_)
                | Action::Escape
                | Action::CancelPointer
                | Action::RemoveBox(_)
                | Action::RemoveSelected
                | Action::ChangeTabs(..)
        ) {
            self.tab_drag = None;
        }
        false
    }
    pub(crate) fn begin_tab_drag(
        &mut self,
        id: BoxId,
        view: ViewId,
        position: Point,
    ) -> Task<Message> {
        if !self.workspace_input_enabled()
            || self.application.workspace().mode() != Mode::Layout
            || !self.application.workspace().boxes().iter().any(|item| {
                item.id == id
                    && item.tabs.as_ref().is_some_and(|tabs| {
                        tabs.visible() && tabs.views().iter().any(|item| item.view == view)
                    })
            })
        {
            return Task::none();
        }
        let task = self.dispatch(Action::SelectBox(Some(id)));
        self.tab_drag = Some(Drag {
            id,
            view,
            pointer: position,
            target: None,
            marker: None,
        });
        Task::batch([task, iced::widget::operation::focus(tab_rail::tab_id(view))])
    }
    pub(crate) fn move_tab_drag(&mut self, point: Point) {
        let geometry = self.geometry();
        let Some(drag) = &mut self.tab_drag else {
            return;
        };
        drag.pointer = point;
        drag.target = None;
        drag.marker = None;
        let Some(item) = self
            .application
            .workspace()
            .boxes()
            .iter()
            .find(|item| item.id == drag.id)
        else {
            return;
        };
        let rect = geometry.rect(item.rect);
        let bounds = Rectangle {
            x: rect.x + geometry.viewport.x - geometry.offset.x,
            y: rect.y + geometry.viewport.y - geometry.offset.y,
            ..rect
        };
        if !geometry.viewport.contains(point) {
            return;
        }
        let offset = tab_rail::offset(
            item,
            bounds.height,
            self.tab_offsets.get(&item.id).copied().unwrap_or(0.0),
            geometry.typography,
        );
        let Some((_, index)) = tab_rail::row_at(item, bounds, offset, point, geometry.typography)
        else {
            return;
        };
        let Some(tabs) = &item.tabs else {
            return;
        };
        let Some(from) = tabs.views().iter().position(|view| view.view == drag.view) else {
            return;
        };
        let row_height = geometry.typography.row(tab_rail::ROW_HEIGHT);
        let row_top = bounds.y + tab_rail::END_INSET + index as f32 * row_height - offset;
        let insert = index + usize::from(point.y >= row_top + row_height / 2.0);
        drag.target = Some(insert - usize::from(from < insert));
        drag.marker = Some(Rectangle {
            x: bounds.x + 7.0,
            y: (bounds.y + tab_rail::END_INSET + insert as f32 * row_height - offset - 2.0).clamp(
                bounds.y + tab_rail::END_INSET,
                (bounds.y + bounds.height
                    - geometry.typography.row(tab_rail::ADD_HEIGHT)
                    - tab_rail::END_INSET
                    - 6.0)
                    .max(bounds.y + tab_rail::END_INSET),
            ),
            width: (tab_rail::width(item, bounds.width) - 14.0).max(1.0),
            height: 4.0,
        });
    }
    pub(crate) fn finish_tab_drag(&mut self) -> Task<Message> {
        let Some(drag) = self.tab_drag.take() else {
            return Task::none();
        };
        drag.target.map_or_else(Task::none, |position| {
            self.dispatch(Action::ChangeTabs(
                drag.id,
                TabCommand::Reorder {
                    view: drag.view,
                    position,
                },
            ))
        })
    }
}

pub(crate) fn view(desktop: &Desktop, palette: a::Palette) -> Element<'_, Message> {
    let Some(drag) = &desktop.tab_drag else {
        return space().into();
    };
    let Some(view) = desktop.application.workspace().view(drag.view) else {
        return space().into();
    };
    let name = view
        .navigation
        .current()
        .file_name()
        .unwrap_or_else(|| view.navigation.current().as_os_str())
        .to_string_lossy();
    let preview = container(
        palette
            .typography
            .small(name)
            .wrapping(text::Wrapping::None),
    )
    .padding([9, 14])
    .width(160)
    .clip(true)
    .style(move |_| container::Style {
        background: Some(palette.contrast_background.into()),
        text_color: Some(palette.tab_text),
        border: Border {
            color: palette.selected_border,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    });
    let preview = pin(preview)
        .x((drag.pointer.x + 18.0).min((desktop.size.width - 172.0).max(12.0)))
        .y((drag.pointer.y + 20.0).min((desktop.size.height - 50.0).max(12.0)));
    let mut layers = iced::widget::Stack::new();
    if let Some(rect) = drag.marker {
        let line = container(space())
            .width(rect.width)
            .height(4)
            .style(move |_| container::Style {
                background: Some(palette.tool_active_icon.into()),
                border: Border {
                    radius: 2.0.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            });
        layers = layers.push(pin(line).x(rect.x).y(rect.y));
        for x in [rect.x - 4.0, rect.x + rect.width - 4.0] {
            layers = layers.push(
                pin(container(space())
                    .width(8)
                    .height(8)
                    .style(move |_| container::Style {
                        background: Some(palette.tool_active_icon.into()),
                        border: Border {
                            color: palette.contrast_background,
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..container::Style::default()
                    }))
                .x(x)
                .y(rect.y - 2.0),
            );
        }
    }
    stack![layers, preview].into()
}
