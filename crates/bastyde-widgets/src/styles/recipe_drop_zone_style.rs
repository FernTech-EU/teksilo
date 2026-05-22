//! Default `DropZoneStyle` impl driven by paint-recipe data.
//!
//! `RecipeDropZoneStyle` ships the IntUI drop-zone chrome: a rounded,
//! bordered region whose surface tint and border color track the
//! interaction state (idle / accepting / rejecting) via the config's
//! reactive `Signal<DropZoneVisualState>`. The content column (prompt,
//! subtitle, status line, Browse button) is centered and inset by the
//! zone padding.
//!
//! Apps wanting a different look (dashed border, full-bleed dropzone,
//! image-backed) write their own `impl DropZoneStyle` block and install it
//! per-call (`DropZone::style(...)`) or theme-wide
//! (`theme.style_slots.drop_zone = Some(Rc::new(...))`).

use bastyde_core::build_context::BuildContext;
use bastyde_core::styles::{DropZoneStyle, DropZoneStyleConfig};
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::CornerRadius;

use crate::primitives::{Center, Padding, RectWidget, ZStack};

/// Corner radius of the zone's rounded rectangle.
pub const DROP_ZONE_CORNER_RADIUS: f32 = 12.0;
/// Border thickness in logical pixels.
pub const DROP_ZONE_BORDER_WIDTH: f32 = 2.0;
/// Inner padding between the border and the content column.
pub const DROP_ZONE_PADDING: f32 = 20.0;

/// Default `DropZoneStyle` shipped with Bastyde.
#[derive(Debug, Default, Clone, Copy)]
pub struct RecipeDropZoneStyle;

impl DropZoneStyle for RecipeDropZoneStyle {
    fn make_body(&self, cfg: &DropZoneStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // Surface + border colors track the reactive interaction state.
        let bg = cfg.state.map(|s| s.surface_role());
        let border = cfg.state.map(|s| s.border_role());

        let rect = ctx.add(
            RectWidget::new()
                .background(bg)
                .border_color(border)
                .border_width(DROP_ZONE_BORDER_WIDTH)
                .corner_radius(CornerRadius::uniform(DROP_ZONE_CORNER_RADIUS)),
        );

        let centered =
            Center::new().child(Padding::uniform(DROP_ZONE_PADDING).child_id(cfg.content));

        ctx.add(ZStack::new().add_child(rect).child(centered))
    }
}
