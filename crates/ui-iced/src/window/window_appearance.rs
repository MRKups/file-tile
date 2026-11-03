//! Observe the existing Iced renderer without changing its window or compositor.

use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, overlay, renderer,
    widget::{Operation, Tree, tree},
};
use iced::{
    Color, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector, mouse, window,
};

/// Runtime evidence only. Never persist this instead of the requested preference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Capability {
    #[default]
    Unknown,
    Metal,
    TinySkia,
}

impl Capability {
    pub(crate) fn supports_transparency(self) -> bool {
        matches!(self, Self::Metal)
    }

    /// Apply only to the window background, not text, controls, or whole widgets.
    ///
    /// With `web-colors` enabled, Iced premultiplies quad colors directly in sRGB
    /// space on Metal's `Bgra8Unorm` swapchain, matching macOS WindowServer's
    /// `CAMetalLayer` compositing without linear-space pre-compensation.
    pub(crate) fn background(self, mut requested: Color) -> Color {
        if !self.supports_transparency() {
            requested.a = 1.0;
            return requested;
        }
        requested.a = requested.a.clamp(0.0, 1.0);
        if requested.a <= 0.0 {
            Color::TRANSPARENT
        } else {
            requested
        }
    }

    /// Clear color for the swapchain pass before painting the root surface quad.
    /// `wgpu::LoadOp::Clear` does not premultiply RGB by alpha, so transparent
    /// render targets must clear to zeroed RGBA rather than `(r, g, b, 0.0)`.
    pub(crate) fn clear_color(self, requested: Color) -> Color {
        if self.supports_transparency() {
            Color::TRANSPARENT
        } else {
            self.background(requested)
        }
    }

    /// Show only when desktop transparency was requested, including a local draft.
    pub(crate) fn notice(self) -> Option<&'static str> {
        match self {
            Self::Metal => None,
            Self::TinySkia => Some(
                "Desktop transparency is unavailable with the software renderer. The window stays opaque; your preference is kept.",
            ),
            Self::Unknown => Some(
                "Desktop transparency could not be confirmed. The window stays opaque; your preference is kept.",
            ),
        }
    }
}

fn capability(renderer: &Renderer) -> Capability {
    match renderer {
        // This is the actual window renderer, not an environment preference or
        // another headless instance. Metal is established only if it is also the
        // sole backend this wgpu build can select. Widened features fail closed.
        Renderer::Primary(_)
            if cfg!(target_os = "macos")
                && iced::wgpu::Instance::enabled_backend_features()
                    == iced::wgpu::Backends::METAL =>
        {
            Capability::Metal
        }
        Renderer::Primary(_) => Capability::Unknown,
        Renderer::Secondary(_) => Capability::TinySkia,
    }
}

/// Mount outside workspace epochs and modal input boundaries. Redraw observations
/// use the existing UI message port and do not capture input or poll a timer.
pub(crate) fn observe<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    on_observed: impl Fn(Capability) -> Message + 'a,
) -> Element<'a, Message> {
    Element::new(Observer {
        content: content.into(),
        on_observed: Box::new(on_observed),
    })
}

struct Observer<'a, Message> {
    content: Element<'a, Message>,
    on_observed: Box<dyn Fn(Capability) -> Message + 'a>,
}

impl<Message> Widget<Message, Theme, Renderer> for Observer<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Option<Capability>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(None::<Capability>)
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[self.content.as_widget()]);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
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
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if matches!(event, Event::Window(window::Event::RedrawRequested(_))) {
            let observed = capability(renderer);
            let reported = tree.state.downcast_mut::<Option<Capability>>();
            if *reported != Some(observed) {
                *reported = Some(observed);
                shell.publish((self.on_observed)(observed));
            }
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}
