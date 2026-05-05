//! SearchField — a [`TextInput`](crate::text_input::TextInput) preset
//! configured for search workflows: leading magnifier glyph, a clear-X
//! button enabled by default, and a placeholder defaulting to the
//! framework-translated "Search" label.
//!
//! ```ignore
//! let query = ctx.signal(String::new());
//! SearchField::new(query.clone())
//!     .placeholder("Search documents")
//!     .on_submit_fn(|ctx| ctx.send_intent(AppIntent::Search))
//! ```
//!
//! The wrapper is intentionally thin — every option that
//! [`TextInput`] exposes (label, max length, validation, on_submit,
//! on_blur, leading/trailing slots, etc.) can be reached by calling
//! [`SearchField::into_input`] to drop down to the underlying
//! `TextInput` builder.

use fern_canvas::{Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{EventContext, LayoutContext, Widget, WidgetPlacement};
use fern_core::widget_id::WidgetId;
use fern_tokens::TextRole;

use crate::built_in_button::BuiltInIcons;
use crate::primitives::Center;
use crate::primitives::icon_widget::IconWidget;
use crate::text_input::TextInput;

const SEARCH_GLYPH_SIZE: f32 = 14.0;

fn search_glyph() -> IconWidget {
    (BuiltInIcons::global().search)()
        .icon_size(SEARCH_GLYPH_SIZE)
        .color(TextRole::Secondary)
}

/// Convenience wrapper around [`TextInput`] preset for search.
pub struct SearchField {
    input: Option<TextInput>,
    root_child_id: Option<WidgetId>,
}

impl SearchField {
    /// Construct a `SearchField` bound to `text`. Placeholder defaults
    /// to the localized "Search" string; override with [`Self::placeholder`].
    pub fn new(text: Signal<String>) -> Self {
        // Reuse the existing `a11y_builtin_search` translation as the
        // default placeholder — apps can override per-instance.
        let placeholder = fern_i18n::tr_widget!(a11y_builtin_search()).resolve_now();
        let input = TextInput::new(text)
            .placeholder(placeholder)
            .show_clear_button(true)
            .leading_slot(Center::new().child(search_glyph()));
        Self {
            input: Some(input),
            root_child_id: None,
        }
    }

    fn map_input(mut self, f: impl FnOnce(TextInput) -> TextInput) -> Self {
        let inner = self
            .input
            .take()
            .expect("SearchField input is consumed only once during build");
        self.input = Some(f(inner));
        self
    }

    /// Override the placeholder text shown when the field is empty.
    pub fn placeholder(self, text: impl Into<String>) -> Self {
        let text = text.into();
        self.map_input(|i| i.placeholder(text))
    }

    /// Accessible name for the search field.
    pub fn label(self, label: impl Into<String>) -> Self {
        let label = label.into();
        self.map_input(|i| i.label(label))
    }

    /// Disable / re-enable the field.
    pub fn enabled(self, on: bool) -> Self {
        self.map_input(|i| i.enabled(on))
    }

    /// Clamp the maximum length the user can type.
    pub fn max_length(self, n: usize) -> Self {
        self.map_input(|i| i.max_length(n))
    }

    /// Closure invoked on Enter — typical search-trigger hook.
    pub fn on_submit_fn(self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.map_input(|i| i.on_submit_fn(f))
    }

    /// Closure invoked on focus loss.
    pub fn on_blur_fn(self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.map_input(|i| i.on_blur_fn(f))
    }

    /// Drop down to the underlying [`TextInput`] builder for any option
    /// the wrapper doesn't surface.
    pub fn into_input(mut self) -> TextInput {
        self.input.take().expect("SearchField inner input is set")
    }
}

impl std::fmt::Debug for SearchField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchField").finish_non_exhaustive()
    }
}

impl Widget for SearchField {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Take the inner TextInput on first build only; subsequent
        // rebuilds reuse the cached id.
        if let Some(input) = self.input.take() {
            self.root_child_id = Some(ctx.add(input));
        }
        self.children()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // SearchField itself is a layout shell — the inner TextInput
        // carries the text-edit role and value. Hiding the outer node
        // keeps the AT tree from announcing a redundant container.
        builder.set_role(fern_core::accesskit::Role::SearchInput);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_core::widget_tree::WidgetTree;
    use fern_tokens::Theme;

    #[test]
    fn search_field_builds() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let q = Signal::new(String::new());
        let id = tree.add(SearchField::new(q.clone()).placeholder("Search docs"));
        tree.layout(SizeProposal {
            width: Some(320.0),
            height: None,
        });
        let b = tree.bounds(id);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn search_field_a11y_role() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let id = tree.add(SearchField::new(Signal::new(String::new())));
        tree.layout(SizeProposal {
            width: Some(280.0),
            height: None,
        });
        let info = tree.accessibility_node(id);
        assert_eq!(info.role(), fern_core::accesskit::Role::SearchInput);
    }
}
