//! Tier-3 style protocol for `RichTextEditor`. See `docs/styling-system.md`.
//!
//! Themes only the *frame* — border, padding, focus ring, background —
//! the same surface a `TextInput` has. The body's `paint` renders
//! glyph runs, caret, and selection itself; that is the editor's
//! domain output and stays widget-owned (principle 6).
//!
//! The `RichTextEditor` widget is wired through `make_body` — the
//! composing outer widget owns state, handlers, focus, while the
//! inner leaf body owns layout, paint, and accessibility. Apps
//! install a chrome override via
//! `RichTextEditor::style(impl RichTextEditorStyle)` or the
//! theme-wide `style_slots.rich_text_editor` slot.

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
    /// Per-edge `(top, right, bottom, left)` padding between the text
    /// content and the chrome border. `None` means the style picks its
    /// own default (TextInput-style insets for editable, no padding for
    /// read-only). Set via `RichTextEditor::content_padding`.
    pub content_padding: Option<(f32, f32, f32, f32)>,
}

pub trait RichTextEditorStyle: 'static {
    fn make_body(&self, cfg: &RichTextEditorStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedRichTextEditorStyle = Rc<dyn RichTextEditorStyle>;
