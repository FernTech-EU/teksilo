//! Tier-3 style protocol for `RichTextEditor`. See `docs/styling-system.md`.
//!
//! Themes only the *frame* — border, padding, focus ring, background —
//! the same surface a `TextInput` has. `RichTextEditor::paint` renders
//! glyph runs, caret, and selection itself; that is the editor's
//! domain output and stays widget-owned (principle 6).
//!
//! ## Wiring status
//!
//! The trait surface and the `style_slots.rich_text_editor` slot are
//! in place. Wiring the `RichTextEditor` widget itself through
//! `make_body` requires splitting the editor between a composing
//! outer widget (state + handlers) and an inner leaf (paint only),
//! since today `RichTextEditor` is a single leaf widget owning both.
//! That refactor is intentionally deferred — see the follow-up entry
//! in `docs/plans/group-5-styling-migration.md`. Apps that want a
//! bordered surface around a `RichTextEditor` should continue
//! wrapping it in a `Panel` until the wiring lands.

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::widget_id::WidgetId;

pub struct RichTextEditorStyleConfig {
    /// Pre-built editor viewport (the leaf widget that paints text).
    pub viewport: WidgetId,
    /// Reactive focus signal — drives the focus-ring colour.
    pub is_focused: Signal<bool>,
    /// `true` when the editor is in read-only / viewer mode.
    pub is_read_only: bool,
}

pub trait RichTextEditorStyle: 'static {
    fn make_body(
        &self,
        cfg: &RichTextEditorStyleConfig,
        ctx: &mut BuildContext,
    ) -> WidgetId;
}

pub type SharedRichTextEditorStyle = Rc<dyn RichTextEditorStyle>;
