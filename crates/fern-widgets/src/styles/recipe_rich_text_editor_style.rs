//! Default `RichTextEditorStyle` impl.
//!
//! Frames the editor viewport in a TextInput-like border + padding +
//! corner-radius surface. The recipe is *available* but the widget
//! itself is not yet routed through `make_body` — that requires the
//! leaf-vs-composing split documented on `RichTextEditorStyle` and
//! is deferred. The default impl exists so apps installing a custom
//! `RichTextEditorStyle` via the theme slot have a reference shape
//! to compare against.

use fern_core::build_context::BuildContext;
use fern_core::color_prop::ColorProp;
use fern_core::styles::{RichTextEditorStyle, RichTextEditorStyleConfig};
use fern_core::widget_id::WidgetId;
use fern_tokens::{BorderRole, CornerRadius, SurfaceRole};

use crate::primitives::{Padding, RectWidget, ZStack};
use crate::styles::recipe_text_input_style as field_dims;

/// Default `RichTextEditorStyle` shipped with FernUI. Wraps the
/// viewport in a TextInput-like border + padding + corner-radius
/// frame, with a focus-aware border. Returns the viewport id directly
/// when read-only — viewers shouldn't carry an editable-field frame.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeRichTextEditorStyle;

impl RichTextEditorStyle for RecipeRichTextEditorStyle {
    fn make_body(
        &self,
        cfg: &RichTextEditorStyleConfig,
        ctx: &mut BuildContext,
    ) -> WidgetId {
        // Read-only viewers stay frameless — they're typically rendered
        // inside an outer surface (Card, Panel) that owns the chrome.
        if cfg.is_read_only {
            return cfg.viewport;
        }

        // Editable: TextInput-style focus-aware frame.
        let theme = ctx.theme_signal().get();
        let focus_ring_width = theme.shape.focus_ring_width;
        let field_border_width = field_dims::TEXT_FIELD_BORDER_WIDTH;
        let border_role = cfg.is_focused.map(|f| {
            if *f {
                BorderRole::Focused
            } else {
                BorderRole::Default
            }
        });
        let border_width_signal = cfg.is_focused.map(move |f| {
            if *f {
                focus_ring_width
            } else {
                field_border_width
            }
        });
        let bg = RectWidget::new()
            .background(SurfaceRole::Content)
            .border_color(ColorProp::DynamicBorderRole(border_role))
            .border_width(border_width_signal)
            .corner_radius(CornerRadius::uniform(field_dims::TEXT_FIELD_CORNER_RADIUS));
        let bg_id = ctx.add(bg);
        let padded = ctx.add(
            Padding::new(
                field_dims::TEXT_FIELD_PADDING_VERTICAL,
                field_dims::TEXT_FIELD_PADDING_HORIZONTAL,
                field_dims::TEXT_FIELD_PADDING_VERTICAL,
                field_dims::TEXT_FIELD_PADDING_HORIZONTAL,
            )
            .child_id(cfg.viewport),
        );
        ctx.add(ZStack::new().add_child(bg_id).add_child(padded))
    }
}
