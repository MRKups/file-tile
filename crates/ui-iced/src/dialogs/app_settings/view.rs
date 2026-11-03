//! A relaxed explanation/control form with retained children and fixed actions.

use iced::widget::{button, column, container, row, rule, scrollable, space};
use iced::{Center, Element, Fill};

use crate::application::Application;

use super::{Draft, FOLDER_FIELD, controls, focus, layout, style};
use crate::window::window_appearance::Capability;
use crate::{Desktop, Message, widgets::focus_control, window::appearance as a};

pub(crate) fn view<'a>(desktop: &'a Desktop, draft: &'a Draft) -> Element<'a, Message> {
    let application = &desktop.application;
    let size = desktop.size;
    let capability = desktop.window_appearance;
    let picker_busy = desktop.settings_picker.pending();
    let system_font = &desktop.system_font;
    let logs = &desktop.logs;
    let updates = &desktop.updates;
    let dimensions = layout::panel_size(size);
    let width = layout::form_width(dimensions);
    let palette = a::resolve(draft.appearance.preview())
        .with_typography(application.typography())
        .boxes;
    if let Some(editor) = &draft.appearance.editor {
        return super::custom_themes::view::view(editor, size, palette);
    }
    if let Some(explorer) = &draft.font_explorer {
        return super::font_explorer::view(explorer, size, palette, system_font);
    }
    let ty = palette.typography;
    let dirty = draft.is_dirty(application);
    let content = column![
        appearance_section(
            &draft.appearance,
            application.user_themes_notice(),
            capability,
            width,
            palette,
        ),
        super::typography::view(
            draft.typography,
            draft.font_picker_open,
            draft.font_picker_cursor,
            width,
            palette,
            system_font,
        ),
        general(
            application,
            draft,
            width,
            palette,
            picker_busy,
            updates,
            logs
        ),
        super::access_safety::view(draft.safety, &draft.access_safety, width, palette),
    ]
    .spacing(64)
    .width(width);
    let form = a::scroll_area(container(container(content).center_x(Fill)).padding(
        iced::Padding {
            top: 3.0,
            right: 14.0,
            bottom: 12.0,
            left: 3.0,
        },
    ))
    .id(focus::SCROLL_ID)
    .style(palette.scrollable())
    .direction(scrollable::Direction::Vertical(a::scrollbar()))
    .width(Fill)
    .height(Fill);
    let panel = column![
        ty.title("Settings").color(palette.text),
        form,
        rule::horizontal(1).style(move |_| palette.rule()),
        footer(dirty, picker_busy, palette),
    ]
    .spacing(20);
    container(
        container(panel)
            .padding(layout::padding(dimensions))
            .width(dimensions.width)
            .height(dimensions.height)
            .style(move |_| style::panel(palette)),
    )
    .padding(layout::MARGIN)
    .center_x(Fill)
    .center_y(Fill)
    .style(|_| container::Style {
        background: Some(a::MODAL_SCRIM.into()),
        ..container::Style::default()
    })
    .into()
}

fn footer(dirty: bool, picker_busy: bool, palette: a::Palette) -> Element<'static, Message> {
    row![
        space().width(Fill),
        action(
            "app-settings-cancel",
            if dirty { "Cancel" } else { "Close" },
            Some(Message::CancelAppSettings),
            false,
            palette,
        ),
        action(
            "app-settings-apply",
            "Apply",
            (dirty && !picker_busy).then_some(Message::ApplyAppSettings),
            true,
            palette,
        ),
    ]
    .spacing(10)
    .align_y(Center)
    .into()
}

fn action(
    id: &'static str,
    label: &'static str,
    message: Option<Message>,
    primary: bool,
    palette: a::Palette,
) -> Element<'static, Message> {
    focus_control::themed_optional(
        id,
        message.clone(),
        button(palette.typography.control(label))
            .padding([8, 16])
            .style(move |_, status| style::button_style(palette, status, primary, false))
            .on_press_maybe(message),
        palette,
    )
}

pub(super) fn setting<'a>(
    label: &'static str,
    description: &'static str,
    control: Element<'a, Message>,
    width: f32,
    palette: a::Palette,
) -> Element<'a, Message> {
    let (left, right) = layout::column_widths(width);
    let mut explanation = column![palette.typography.control(label).color(palette.text)].spacing(4);
    if !description.is_empty() {
        explanation = explanation.push(palette.typography.text(description).color(palette.muted));
    }
    // Wrapping preserves the same children and editor state across the breakpoint.
    row![
        container(explanation).width(left),
        container(control).width(right),
    ]
    .spacing(36)
    .wrap()
    .vertical_spacing(12)
    .into()
}

fn appearance_section<'a>(
    draft: &'a super::appearance::Draft,
    notice: Option<&str>,
    capability: Capability,
    width: f32,
    palette: a::Palette,
) -> Element<'a, Message> {
    use crate::application::preferences::appearance::SystemAppearance;
    let light = controls::theme(draft, SystemAppearance::Light, palette);
    let mut dark = column![controls::theme(draft, SystemAppearance::Dark, palette)].spacing(8);
    if let Some(notice) = notice {
        dark = dark.push(
            palette
                .typography
                .caption(notice.to_owned())
                .color(palette.error),
        );
    }
    let mut transparency = column![controls::transparency(draft.transparency, palette)].spacing(8);
    if draft.transparency > 0
        && let Some(notice) = capability.notice()
    {
        transparency = transparency.push(palette.typography.caption(notice).color(palette.text));
    }
    column![
        palette.typography.heading("Appearance").color(palette.text),
        setting(
            "Light theme",
            "Used when macOS is in Light mode.",
            light,
            width,
            palette,
        ),
        setting(
            "Dark theme",
            "Used when macOS is in Dark mode.",
            dark.into(),
            width,
            palette,
        ),
        setting(
            "Window transparency",
            "",
            transparency.into(),
            width,
            palette,
        ),
        setting(
            "Box transparency",
            "",
            controls::box_transparency(draft.box_transparency, palette),
            width,
            palette,
        ),
        setting(
            "Surface texture",
            "",
            controls::surface_texture(draft.surface_texture, palette),
            width,
            palette,
        ),
    ]
    .spacing(24)
    .into()
}

fn general(
    application: &Application,
    draft: &Draft,
    width: f32,
    palette: a::Palette,
    picker_busy: bool,
    updates: &crate::dialogs::updates::Updates,
    logs: &crate::dialogs::diagnostics::Logs,
) -> Element<'static, Message> {
    let ty = palette.typography;
    let mut field = column![super::folder::view(
        draft,
        application.home(),
        picker_busy,
        palette
    )]
    .spacing(8);
    if let Some(error) = &draft.error {
        field = field.push(ty.caption(error.clone()).color(palette.error));
    }
    let mut folder = column![container(field).id(FOLDER_FIELD)].spacing(8);
    if draft
        .chosen
        .as_deref()
        .or(draft.original.as_deref())
        .is_some_and(|path| path.to_str().is_none())
        && (!draft.edited || draft.chosen.is_some())
    {
        folder = folder.push(
            ty.caption("This saved path cannot be represented exactly as text. Leave it unchanged to preserve it, or enter a replacement.")
                .color(palette.text),
        );
    }
    if let Some(path) = application.directory_override() {
        folder = folder.push(
            ty.caption(format!(
                "This launch uses --directory {} for new boxes. The saved default applies without that override.",
                path.to_string_lossy()
            ))
            .color(palette.text),
        );
    }
    let mut content = column![
        ty.heading("General").color(palette.text),
        setting(
            "Updates",
            "",
            crate::dialogs::updates::controls(updates, palette),
            width,
            palette,
        ),
        setting(
            "Starting folder for new boxes",
            "",
            folder.into(),
            width,
            palette,
        ),
    ]
    .spacing(24);
    if application.save_failed()
        && let Some(detail) = application.storage_detail()
    {
        let recovery = column![
            ty.caption(detail).color(palette.error),
            focus_control::themed(
                "app-settings-recovery-details",
                Message::StorageDetails(true),
                button(ty.control("Workspace recovery"))
                    .style(move |_, status| palette.text_button(status))
                    .on_press(Message::StorageDetails(true)),
                palette,
            ),
        ]
        .spacing(8);
        content = content.push(setting(
            "Saving and recovery",
            "Discards unapplied settings.",
            recovery.into(),
            width,
            palette,
        ));
    }
    content
        .push(setting(
            "Diagnostic Logs",
            "",
            crate::dialogs::diagnostics::controls(logs, palette),
            width,
            palette,
        ))
        .into()
}
