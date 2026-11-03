//! Compact shortcut surface and a responsive Configure card.
use super::{CHOOSE_ID, Draft, EDITOR_ID, Event, target_text};
use crate::{
    Desktop, Message,
    application::{Action, Application},
    dialogs::app_settings::style,
    domain::{LauncherIcon, LauncherTarget, LauncherTile, Mode},
    widgets::{focus_control, folder_field},
    window::{
        appearance::{self as a, Palette},
        icons::Icon,
    },
};
use iced::{
    Center, Element, Fill, Padding, Size,
    widget::{
        button, column, container, opaque, pin, row, rule, space, stack, text_input, tooltip,
    },
};

const LABEL_WIDTH: f32 = 68.0;
const COLUMN_GAP: f32 = 16.0;

fn icon(tile: &LauncherTile) -> Icon {
    match tile.config.icon {
        LauncherIcon::Folder => Icon::Folder,
        LauncherIcon::File => Icon::Document,
        LauncherIcon::Application => Icon::Application,
        LauncherIcon::Link => Icon::Link,
        LauncherIcon::Automatic => match &tile.config.target {
            Some(LauncherTarget::Link(_)) => Icon::Link,
            Some(LauncherTarget::Path(path))
                if path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("app")) =>
            {
                Icon::Application
            }
            _ => Icon::External,
        },
    }
}

pub(crate) fn tile<'a>(
    app: &'a Application,
    item: &'a LauncherTile,
    size: Size,
    palette: Palette,
    window_palette: Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    let failed = app.launcher_failed(item.id);
    let pending = app.launcher_opening().is_some_and(|opening| {
        opening.status == crate::application::workspace::launchers::Status::Pending
    });
    let action = (app.accepts_workspace_input()
        && app.workspace().mode() == Mode::Browse
        && !pending
        && (item.config.target.is_some() || app.file_operation().is_none()))
    .then(|| {
        if item.config.target.is_some() {
            Message::Action(Action::ActivateLauncher(item.id))
        } else {
            Message::Launcher(Event::Edit(item.id))
        }
    });
    let dimmed = app.workspace().mode() == Mode::Configure
        && app.workspace().selected().is_some()
        && !app.workspace().configuring(item.id);
    let color = palette.text;
    let face = button(tile_content(item, size, palette, color, failed))
        .padding(6)
        .width(size.width)
        .height(size.height)
        .on_press_maybe(action.clone())
        .style(move |_, status| iced::widget::button::Style {
            background: (status == button::Status::Hovered).then_some(palette.row_hover.into()),
            text_color: color,
            border: iced::Border {
                radius: 10.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });
    let control =
        focus_control::themed_optional(format!("launcher-{}", item.id.0), action, face, palette);
    let mut layers = stack![control];
    if app.launcher_status(item.id).is_some() {
        layers = layers.push(
            pin(ty.control(if failed { "!" } else { "…" }).color(if failed {
                palette.error
            } else {
                palette.muted
            }))
            .x(size.width - 18.0)
            .y(4.0),
        );
    }
    if dimmed {
        layers = layers.push(
            container(space())
                .width(Fill)
                .height(Fill)
                .style(move |_| window_palette.configure_scrim()),
        );
    }
    let card = container(layers.width(Fill).height(Fill))
        .width(size.width)
        .height(size.height)
        .clip(true)
        .style(move |_| palette.panel());
    let mut details = format!(
        "{}\n{}",
        item.config.name,
        if item.config.target.is_some() {
            target_text(item.config.target.as_ref())
        } else {
            "Choose a target in Configure.".into()
        }
    );
    if let Some(status) = app.launcher_status(item.id) {
        details.push('\n');
        details.push_str(status);
    } else if pending {
        details.push_str("\nAnother shortcut is opening. Wait for it to finish.");
    } else if item.config.target.is_none() && app.file_operation().is_some() {
        details
            .push_str("\nFinish or dismiss the file operation before configuring this shortcut.");
    }
    tooltip(
        card,
        container(
            ty.small(details)
                .wrapping(iced::widget::text::Wrapping::WordOrGlyph),
        )
        .max_width(420)
        .padding(8),
        tooltip::Position::FollowCursor,
    )
    .style(move |_| palette.tooltip())
    .into()
}

fn tile_content(
    item: &LauncherTile,
    size: Size,
    palette: Palette,
    color: iced::Color,
    failed: bool,
) -> Element<'_, Message> {
    let ty = palette.typography;
    let glyph = crate::window::icons::view(
        icon(item),
        if failed {
            14.0
        } else if size.height > 100.0 {
            38.0
        } else {
            24.0
        },
        Some(color),
        1.0,
    );
    let label = container(
        ty.text(&item.config.name)
            .color(color)
            .width(iced::Shrink)
            .height(ty.row(if failed { 16.0 } else { 18.0 }))
            .wrapping(iced::widget::text::Wrapping::None),
    )
    .width(Fill)
    .center_x(Fill)
    .clip(true);
    let mut body = column![glyph, label]
        .align_x(Center)
        .spacing(if failed { 3 } else { 4 });
    if failed {
        body = body.push(ty.caption("Check target").color(palette.error));
    }
    container(body)
        .clip(true)
        .width(Fill)
        .height(Fill)
        .center_x(Fill)
        .center_y(Fill)
        .into()
}

fn action_button<'a>(
    label: &'a str,
    id: &'static str,
    action: Option<Message>,
    primary: bool,
    palette: Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    focus_control::themed_optional(
        id,
        action.clone(),
        button(ty.control(label))
            .on_press_maybe(action)
            .padding([8, 16])
            .style(move |_, status| style::button_style(palette, status, primary, false)),
        palette,
    )
}

fn form_row<'a>(
    label: &'static str,
    control: Element<'a, Message>,
    palette: Palette,
) -> Element<'a, Message> {
    let ty = palette.typography;
    row![
        container(ty.control(label).color(palette.text)).width(ty.row(LABEL_WIDTH)),
        container(control).width(Fill),
    ]
    .spacing(COLUMN_GAP)
    .align_y(Center)
    .into()
}

fn fields<'a>(desktop: &'a Desktop, draft: &'a Draft, palette: Palette) -> Element<'a, Message> {
    let ty = palette.typography;
    let generation = draft.generation;
    let busy = desktop.settings_picker.pending();
    let config = draft.config(desktop.application.home());
    let name_invalid = crate::domain::LauncherConfig::validate_name(&draft.name).is_err();
    let target_invalid = draft.target_value(desktop.application.home()).is_err();
    let error = draft
        .error
        .as_deref()
        .or_else(|| config.as_ref().err().copied());
    let name = text_input("Shortcut name", &draft.name)
        .id("launcher-name")
        .size(ty.size(a::CONTROL_SIZE))
        .font(ty.font())
        .padding([10, 14])
        .width(Fill)
        .on_input(move |value| Message::Launcher(Event::Name(generation, value)))
        .on_submit_maybe((!busy).then_some(Message::Launcher(Event::Apply(generation))))
        .style(move |_, status| style::field(palette, status, name_invalid));
    let target_input = text_input("/path, ~/path, or https://…", &draft.target)
        .id("launcher-target")
        .size(ty.size(a::CONTROL_SIZE))
        .font(ty.font())
        .padding([10, 14])
        .width(Fill)
        .on_input_maybe(
            (!busy).then_some(move |value| Message::Launcher(Event::Target(generation, value))),
        )
        .on_submit_maybe((!busy).then_some(Message::Launcher(Event::Apply(generation))))
        .style(move |_, status| folder_field::field_style(palette, status, target_invalid));
    let target = folder_field::view(
        target_input.into(),
        CHOOSE_ID,
        (!busy).then_some(Message::Launcher(Event::Choose(generation))),
        palette,
    );
    let mut form = column![
        form_row("Name", name.into(), palette),
        form_row("Target", target, palette),
        form_row(
            "Icon",
            super::icon_picker::view(generation, draft.icon, palette),
            palette,
        ),
    ]
    .spacing(14);
    if let Some(error) = error {
        form = form.push(
            row![
                space().width(ty.row(LABEL_WIDTH)),
                ty.caption(error).color(palette.error).width(Fill),
            ]
            .spacing(COLUMN_GAP),
        );
    }
    container(form).padding([2, 0]).width(Fill).into()
}

pub(crate) fn editor<'a>(desktop: &'a Desktop, draft: &'a Draft) -> Element<'a, Message> {
    let palette = desktop.configure_palette();
    let ty = palette.typography;
    let generation = draft.generation;
    let number = desktop
        .application
        .workspace()
        .launchers()
        .iter()
        .position(|item| item.id == draft.id)
        .map_or(1, |index| index + 1);
    let can_apply = !desktop.settings_picker.pending() && draft.changed(desktop.application.home());
    let actions = row![
        space().width(Fill),
        action_button(
            "Cancel",
            "launcher-cancel",
            Some(Message::Launcher(Event::Cancel(generation))),
            false,
            palette,
        ),
        action_button(
            "Apply",
            "launcher-apply",
            can_apply.then_some(Message::Launcher(Event::Apply(generation))),
            true,
            palette,
        ),
    ]
    .spacing(10)
    .align_y(Center);
    let header_height = desktop.geometry().header_height;
    let top_pad = header_height + 12.0;
    let bottom_pad = (desktop.size.height - 380.0 - top_pad).clamp(12.0, top_pad);
    let width = (desktop.size.width - 24.0).clamp(1.0, 540.0);
    let available_height = (desktop.size.height - top_pad - bottom_pad).max(120.0);
    let body_max_height = (available_height - ty.row(152.0)).max(ty.row(56.0));
    let body =
        container(a::scroll_area(fields(desktop, draft, palette)).style(palette.scrollable()))
            .max_height(body_max_height);
    let card = container(
        column![
            header(number, generation, palette),
            body,
            rule::horizontal(1).style(move |_| palette.rule()),
            actions,
        ]
        .spacing(18),
    )
    .id(EDITOR_ID)
    .padding([22, 24])
    .width(width)
    .max_height(available_height)
    .style(move |_| palette.inspector_panel());
    container(opaque(card))
        .width(Fill)
        .height(Fill)
        .padding(Padding {
            top: top_pad,
            right: 12.0,
            bottom: bottom_pad,
            left: 12.0,
        })
        .center_x(Fill)
        .center_y(Fill)
        .into()
}

fn header(number: usize, generation: u64, palette: Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    row![
        column![
            ty.caption("CONFIGURE")
                .font(ty.heading_font())
                .color(palette.muted),
            ty.heading(format!("Shortcut {number:02}"))
                .color(palette.text),
        ]
        .spacing(4)
        .width(Fill),
        close(generation, palette),
    ]
    .align_y(iced::Top)
    .spacing(12)
    .into()
}

fn close(generation: u64, palette: Palette) -> Element<'static, Message> {
    let ty = palette.typography;
    let action = Message::Launcher(Event::Cancel(generation));
    let size = ty.row(26.0);
    let face = button(
        container(ty.text("×").size(ty.size(a::TITLE_SIZE)))
            .center_x(size)
            .center_y(size),
    )
    .padding(0)
    .width(size)
    .height(size)
    .style(move |_, status| palette.text_button(status))
    .on_press(action.clone());
    tooltip(
        focus_control::themed("launcher-close", action, face, palette),
        ty.small("Close"),
        tooltip::Position::Bottom,
    )
    .style(move |_| palette.tooltip())
    .into()
}
