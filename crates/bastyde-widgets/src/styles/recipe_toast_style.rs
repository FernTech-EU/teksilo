//! Default `ToastStyle` impl driven by paint-recipe data.
//!
//! `RecipeToastStyle` ships the IntUI toast chrome: a per-severity
//! status-tinted surface (`StatusInfo` / `StatusSuccess` /
//! `StatusWarning` / `StatusError`) with rounded corners, padding,
//! a leading severity glyph, and an optional trailing close `IconButton`.
//!
//! Tokens are co-located here.
//! Apps that want a different look (full-bleed strip, frosted glass,
//! icon-free) write their own `impl ToastStyle` block. The widget
//! always builds the functional pieces (severity glyph, close button,
//! body content) — the recipe is pure chrome.

use bastyde_core::build_context::BuildContext;
use bastyde_core::color_prop::ColorProp;
use bastyde_core::styles::{ToastStyle, ToastStyleConfig};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{CornerRadius, VAlignment};

use crate::primitives::{Expand, HStack, Padding, RectWidget, ZStack};

/// Outer horizontal padding inside the toast surface.
pub const TOAST_PADDING_HORIZONTAL: f32 = 14.0;
/// Outer vertical padding inside the toast surface.
pub const TOAST_PADDING_VERTICAL: f32 = 12.0;
/// Rounded corner radius of the toast surface (matches IntUI
/// `radius_popup`).
pub const TOAST_CORNER_RADIUS: f32 = 8.0;
/// Diameter of the leading severity glyph (consumed by the widget, not
/// the recipe — exposed here so the widget pulls one constant).
pub const TOAST_GLYPH_SIZE: f32 = 16.0;
/// Horizontal gap between leading glyph, body column, and trailing
/// close button.
pub const TOAST_CONTENT_GAP: f32 = 12.0;
/// Vertical gap between title and body lines inside the body column.
pub const TOAST_TITLE_BODY_GAP: f32 = 2.0;
/// Vertical gap between body and action row (when actions are present).
pub const TOAST_BODY_ACTIONS_GAP: f32 = 8.0;

/// Default `ToastStyle` shipped with Bastyde. Surface tint comes from
/// the per-severity `SurfaceRole`. The recipe ignores
/// `cfg.priority` — High/Urgent toasts look identical to Normal at
/// this default styling tier (apps that want a heavier shadow on
/// Urgent provide their own `impl ToastStyle`).
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeToastStyle;

impl ToastStyle for RecipeToastStyle {
    fn make_body(&self, cfg: &ToastStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let radius = CornerRadius::uniform(TOAST_CORNER_RADIUS);

        // Background panel — status surface tint, no border (status
        // surface tokens already encode contrast with the page bg).
        let bg = ctx.add(
            RectWidget::new()
                .background(ColorProp::SurfaceRole(cfg.severity.surface()))
                .corner_radius(radius),
        );

        // Row layout: [glyph] [body (expands)] [close?].
        let body = ctx.add(Expand::horizontal().child_id(cfg.content));
        let mut row = HStack::new()
            .spacing(TOAST_CONTENT_GAP)
            .alignment(VAlignment::Top)
            .add_child(cfg.leading_glyph)
            .add_child(body);
        if let Some(close_id) = cfg.trailing_close {
            row = row.add_child(close_id);
        }
        let row_id = ctx.add(row);
        let padded = ctx.add(
            Padding::symmetric(TOAST_PADDING_VERTICAL, TOAST_PADDING_HORIZONTAL).child_id(row_id),
        );

        ctx.add(ZStack::new().add_child(bg).add_child(padded))
    }
}
