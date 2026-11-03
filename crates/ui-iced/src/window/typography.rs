//! Explicit text styling and matching row metrics. Never changes workspace pitch.
use crate::application::preferences::typography::{TextWeight, Typeface, TypographyPreferences};
use iced::{Font, font};

pub(crate) const ARGON_REGULAR: &[u8] =
    include_bytes!("../../../../assets/fonts/monaspace-argon/MonaspaceArgon-Regular.otf");
pub(crate) const ARGON_MEDIUM: &[u8] =
    include_bytes!("../../../../assets/fonts/monaspace-argon/MonaspaceArgon-Medium.otf");
pub(crate) const CANTARELL_REGULAR: &[u8] =
    include_bytes!("../../../../assets/fonts/cantarell/Cantarell-Regular.ttf");
pub(crate) const CANTARELL_BOLD: &[u8] =
    include_bytes!("../../../../assets/fonts/cantarell/Cantarell-Bold.ttf");

pub(crate) const CAPTION_SIZE: f32 = 10.0;
pub(crate) const SMALL_SIZE: f32 = 11.0;
pub(crate) const BODY_SIZE: f32 = 12.0;
pub(crate) const CONTROL_SIZE: f32 = 13.0;
pub(crate) const HEADING_SIZE: f32 = 16.0;
pub(crate) const TITLE_SIZE: f32 = 22.0;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Style(pub TypographyPreferences);

impl Style {
    fn base_font(self) -> Font {
        if self.0.typeface == Typeface::SystemDefault {
            Font::DEFAULT
        } else {
            Font::with_name(self.0.typeface.name())
        }
    }

    pub fn font(self) -> Font {
        if let Typeface::Custom(custom) = self.0.typeface {
            return super::system_font::resolve_custom(custom, self.0.weight).0;
        }
        Font {
            weight: match self.0.weight {
                TextWeight::Regular => font::Weight::Normal,
                TextWeight::Medium => font::Weight::Medium,
                TextWeight::Bold => font::Weight::Bold,
            },
            ..self.base_font()
        }
    }

    pub fn heading_font(self) -> Font {
        if let Typeface::Custom(custom) = self.0.typeface {
            return super::system_font::resolve_custom(custom, self.0.weight).1;
        }
        Font {
            weight: match (self.0.typeface, self.0.weight) {
                (Typeface::MonaspaceArgon, _) => font::Weight::Medium,
                (Typeface::Cantarell, _) => font::Weight::Bold,
                (_, TextWeight::Regular) => font::Weight::Medium,
                (_, TextWeight::Medium | TextWeight::Bold) => font::Weight::Bold,
            },
            ..self.base_font()
        }
    }

    pub fn size(self, base: f32) -> f32 {
        (base.max(CAPTION_SIZE) * f32::from(self.0.size.get()) / BODY_SIZE).round()
    }

    pub fn icon(self, base: f32) -> f32 {
        base * f32::from(self.0.size.get()) / BODY_SIZE
    }

    pub fn row(self, base: f32) -> f32 {
        (base * f32::from(self.0.size.get()) / BODY_SIZE).ceil()
    }

    pub fn caption<'a>(
        self,
        value: impl iced::advanced::text::IntoFragment<'a>,
    ) -> iced::widget::Text<'a> {
        iced::widget::text(value)
            .font(self.font())
            .size(self.size(CAPTION_SIZE))
    }

    pub fn small<'a>(
        self,
        value: impl iced::advanced::text::IntoFragment<'a>,
    ) -> iced::widget::Text<'a> {
        iced::widget::text(value)
            .font(self.font())
            .size(self.size(SMALL_SIZE))
    }

    pub fn text<'a>(
        self,
        value: impl iced::advanced::text::IntoFragment<'a>,
    ) -> iced::widget::Text<'a> {
        iced::widget::text(value)
            .font(self.font())
            .size(self.size(BODY_SIZE))
    }

    pub fn control<'a>(
        self,
        value: impl iced::advanced::text::IntoFragment<'a>,
    ) -> iced::widget::Text<'a> {
        iced::widget::text(value)
            .font(self.font())
            .size(self.size(CONTROL_SIZE))
    }

    pub fn heading<'a>(
        self,
        value: impl iced::advanced::text::IntoFragment<'a>,
    ) -> iced::widget::Text<'a> {
        iced::widget::text(value)
            .font(self.heading_font())
            .size(self.size(HEADING_SIZE))
    }

    pub fn title<'a>(
        self,
        value: impl iced::advanced::text::IntoFragment<'a>,
    ) -> iced::widget::Text<'a> {
        iced::widget::text(value)
            .font(self.heading_font())
            .size(self.size(TITLE_SIZE))
    }
}
