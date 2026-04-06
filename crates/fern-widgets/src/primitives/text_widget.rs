use std::cell::RefCell;
use std::rc::Rc;

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_tokens::{Color, TextStyle};

use fern_core::accessibility::AccessNodeBuilder;
use fern_core::signal::Prop;
use fern_core::widget::{LayoutContext, PaintContext, Widget};

/// A leaf widget that renders a single line of text via the TextBackend.
/// Text and color can be static or bound to reactive state.
pub struct TextWidget {
    text: Prop<String>,
    color: Prop<Color>,
    style: TextStyle,
    text_backend: Option<Rc<RefCell<dyn fern_canvas::TextBackend>>>,
}

impl std::fmt::Debug for TextWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextWidget").finish()
    }
}

impl TextWidget {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: Prop::Static(text.into()),
            color: Prop::Static(Color::BLACK),
            style: TextStyle::default(),
            text_backend: None,
        }
    }

    pub fn color(mut self, color: Color) -> Self {
        self.color = Prop::Static(color);
        self
    }

    pub fn style(mut self, style: TextStyle) -> Self {
        self.style = style;
        self
    }

    pub fn text_backend(mut self, backend: Rc<RefCell<dyn fern_canvas::TextBackend>>) -> Self {
        self.text_backend = Some(backend);
        self
    }

    /// Bind the text content to a reactive state.
    pub fn bind_text(mut self, state: impl Into<Prop<String>>) -> Self {
        self.text = state.into();
        self
    }

    /// Bind the text color to a reactive state.
    pub fn bind_color(mut self, state: impl Into<Prop<Color>>) -> Self {
        self.color = state.into();
        self
    }

    /// Get the current text value (resolves from state if bound).
    pub fn text(&self) -> String {
        self.text.get()
    }
}

impl Widget for TextWidget {
    fn build(
        &mut self,
        ctx: &mut fern_core::build_context::BuildContext,
    ) -> Vec<fern_core::widget_id::WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.text
            .register_if_bound(self_id, registry, fern_core::state::BindingLevel::Relayout);
        self.color.register_if_bound(
            self_id,
            registry,
            fern_core::state::BindingLevel::RepaintOnly,
        );
        Vec::new()
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let text = self.text.get();
        // Use widget's own backend, or fall back to the context's shared backend.
        if let Some(backend) = self.text_backend.as_ref().or(ctx.text_backend) {
            let mut backend = backend.borrow_mut();
            let layout = backend.layout_single_line(&text, &self.style, proposal.width);
            Size::new(layout.width, layout.height)
        } else {
            let width = text.len() as f32 * 8.0;
            let height = 16.0;
            let w = match proposal.width {
                Some(max) => width.min(max),
                None => width,
            };
            Size::new(w, height)
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        let text = self.text.get();
        let color = self.color.get();
        canvas.draw_text(&text, bounds, &self.style, color);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        let text = self.text.get();
        builder.set_role(fern_core::accesskit::Role::Label);
        builder.set_name(&text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::state::State;
    use fern_core::widget_tree::WidgetTree;

    #[test]
    fn bind_text_renders_state_value() {
        let text = State::new("Hello".to_string());
        let mut tree = WidgetTree::new();
        let w = tree.add(TextWidget::new("").bind_text(text.clone()));
        text.bind_to(
            w,
            tree.binding_registry(),
            fern_core::state::BindingLevel::Relayout,
        );
        tree.layout(SizeProposal::exact(200.0, 40.0));

        assert_eq!(tree.text_content(w), Some("Hello".to_string()));
    }

    #[test]
    fn bind_text_updates_on_state_change() {
        let text = State::new("Hello".to_string());
        let mut tree = WidgetTree::new();
        let w = tree.add(TextWidget::new("").bind_text(text.clone()));
        text.bind_to(
            w,
            tree.binding_registry(),
            fern_core::state::BindingLevel::Relayout,
        );

        tree.layout(SizeProposal::exact(200.0, 40.0));
        assert_eq!(tree.text_content(w), Some("Hello".to_string()));

        text.set("World".to_string());
        tree.layout(SizeProposal::exact(200.0, 40.0));
        assert_eq!(tree.text_content(w), Some("World".to_string()));
    }
}
