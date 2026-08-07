// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Default `RichTextEditorStyle` impl.
//!
//! Frames the editor viewport in a TextInput-like border + padding +
//! corner-radius surface. The `RichTextEditor` widget routes its
//! body through this trait — apps installing a custom
//! `RichTextEditorStyle` (per-call `.style(...)` or theme-wide via
//! `style_slots.rich_text_editor`) swap the chrome wholesale
//! without touching the editor's handlers, focus, or paint.

use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::styles::{RichTextEditorStyle, RichTextEditorStyleConfig};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{BorderRole, CornerRadius, SurfaceRole};

use crate::primitives::{Padding, RectWidget, ZStack};
use crate::styles::recipe_text_input_style as field_dims;

/// Default `RichTextEditorStyle` shipped with Teksilo. Wraps the
/// viewport in a TextInput-like border + padding + corner-radius
/// frame, with a focus-aware border. Returns the viewport id directly
/// when read-only — viewers shouldn't carry an editable-field frame.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeRichTextEditorStyle;

impl RichTextEditorStyle for RecipeRichTextEditorStyle {
    fn make_body(&self, cfg: &RichTextEditorStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // Read-only viewers stay frameless — they're typically rendered
        // inside an outer surface (Card, Panel) that owns the chrome.
        // The widget-level `content_padding` knob is still honoured here:
        // a user-set inset wraps the viewport in a Padding so the text
        // gets the requested gutter against whatever surface the viewer
        // is mounted in.
        if cfg.is_read_only {
            let content = match cfg.content_padding {
                Some((t, r, b, l)) => ctx.add(Padding::new(t, r, b, l).child_id(cfg.viewport)),
                None => cfg.viewport,
            };
            return match cfg.background.clone() {
                Some(bg) => {
                    let bg_id = ctx.add(RectWidget::new().background(bg));
                    ctx.add(ZStack::new().add_child(bg_id).add_child(content))
                }
                None => content,
            };
        }

        // Editable: TextInput-style focus-aware frame. The widget-level
        // `content_padding` replaces the default field insets when set.
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
            .background(
                cfg.background
                    .clone()
                    .unwrap_or_else(|| SurfaceRole::Content.into()),
            )
            .border_color(ColorProp::DynamicBorderRole(border_role))
            .border_width(border_width_signal)
            .corner_radius(CornerRadius::uniform(field_dims::TEXT_FIELD_CORNER_RADIUS));
        let bg_id = ctx.add(bg);
        let (pt, pr, pb, pl) = cfg.content_padding.unwrap_or((
            field_dims::TEXT_FIELD_PADDING_VERTICAL,
            field_dims::TEXT_FIELD_PADDING_HORIZONTAL,
            field_dims::TEXT_FIELD_PADDING_VERTICAL,
            field_dims::TEXT_FIELD_PADDING_HORIZONTAL,
        ));
        let padded = ctx.add(Padding::new(pt, pr, pb, pl).child_id(cfg.viewport));
        ctx.add(ZStack::new().add_child(bg_id).add_child(padded))
    }
}
