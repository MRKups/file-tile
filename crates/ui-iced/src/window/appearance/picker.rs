//! Local Iced theme for picker-owned popup scrollbars, without changing app state.
use super::{Palette, scrollbars, theme_for};
use crate::Message;
use iced::{
    Element, Theme, theme,
    widget::{overlay::menu, pick_list, scrollable, themer},
};

pub(crate) struct PickerTheme {
    palette: Palette,
    base: Theme,
}

pub(crate) fn picker<'a>(
    palette: Palette,
    content: impl Into<Element<'a, Message, PickerTheme>>,
) -> Element<'a, Message> {
    themer(
        Some(PickerTheme {
            palette,
            base: theme_for(palette),
        }),
        content,
    )
    .into()
}

impl theme::Base for PickerTheme {
    fn default(preference: theme::Mode) -> Self {
        let system = super::system_appearance(preference);
        let preferences =
            crate::application::preferences::appearance::AppearancePreferences::default();
        let palette = super::resolve(preferences.resolve(system, &[])).boxes;
        Self {
            palette,
            base: theme_for(palette),
        }
    }
    fn mode(&self) -> theme::Mode {
        self.base.mode()
    }
    fn base(&self) -> theme::Style {
        self.base.base()
    }
    fn palette(&self) -> Option<theme::Palette> {
        Some(self.base.palette())
    }
    fn name(&self) -> &str {
        self.base.name()
    }
}

impl pick_list::Catalog for PickerTheme {
    type Class<'a> = pick_list::StyleFn<'a, Self>;
    fn default<'a>() -> <Self as pick_list::Catalog>::Class<'a> {
        Box::new(|theme, status| pick_list::default(&theme.base, status))
    }
    fn style(
        &self,
        class: &<Self as pick_list::Catalog>::Class<'_>,
        status: pick_list::Status,
    ) -> pick_list::Style {
        class(self, status)
    }
}

impl menu::Catalog for PickerTheme {
    type Class<'a> = menu::StyleFn<'a, Self>;
    fn default<'a>() -> <Self as menu::Catalog>::Class<'a> {
        Box::new(|theme| theme.palette.picker_menu())
    }
    fn style(&self, class: &<Self as menu::Catalog>::Class<'_>) -> menu::Style {
        class(self)
    }
}

impl scrollable::Catalog for PickerTheme {
    type Class<'a> = scrollable::StyleFn<'a, Self>;
    fn default<'a>() -> Self::Class<'a> {
        Box::new(|theme, status| {
            let mut style = scrollbars::style(&theme.base, theme.palette.text, status);
            // Iced owns this popup's 10-point geometry. An inset matching its
            // opaque surface leaves the same 5-point visible capsule as our rails.
            for rail in [&mut style.vertical_rail, &mut style.horizontal_rail] {
                rail.scroller.border.width = 2.5;
                rail.scroller.border.color = theme.palette.contrast_background;
            }
            style
        })
    }
    fn style(&self, class: &Self::Class<'_>, status: scrollable::Status) -> scrollable::Style {
        class(self, status)
    }
}
