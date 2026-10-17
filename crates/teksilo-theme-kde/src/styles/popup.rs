// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The frame every Breeze popup is drawn in: a menu, a combo box's list, a
//! popover and a tooltip.
//!
//! A Breeze popup is a window of its own, a rounded rectangle at the 5 px
//! frame radius, filled and stroked with a 1 px pen on the rectangle inset
//! by half the pen, so the outline lies inside it at radius 4.5. A menu or
//! any other popup panel is filled with the frame background (the window
//! colour 30 % of the way to the view's, `surface_raised`) and outlined in
//! the frame outline (`border`); a tooltip is filled with the tooltip
//! background and outlined in the tooltip background moved toward the
//! tooltip text by the frame contrast (`tooltip_bg`, `tooltip_border`).
//! Both colours are resolved from the theme at paint time, so the inactive
//! and high-contrast projections reach them; the tooltip tokens are the
//! active state's in every state, as Qt's palette has them.
//!
//! Under the frame lies Breeze's window shadow at its default size, Large:
//! two layers of black, one 5 px down with a 20 px blur at 22 %, one 2 px
//! down with a 10 px blur at 12 %, both on the window's 5 px rounded
//! rectangle (`shadow_lg` and `shadow_inner_lg`, spec k.6).
//!
//! ## Where this departs from Breeze
//!
//! - **The panel is always rounded and opaque.** Breeze rounds a popup only
//!   on a compositing desktop, and makes a menu translucent when
//!   `breezerc`'s `MenuOpacity` is below 100; a Teksilo popup is drawn in
//!   its window's own pass, so it is the composited form at Breeze's
//!   default opacity.
//! - **The shadow is drawn on every side.** The framework's popover cuts
//!   the shadow on the side that meets the control that opened it; a Breeze
//!   popup is a window, and its shadow surrounds it.
//! - **The frame outline is not lifted** (D6): it is the outline Breeze
//!   draws around a passive frame, and the shadow already separates the
//!   panel from what lies under it.

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::styles::Theme;
use teksilo_core::widget::{
    LayoutContext, LayoutResponse, PaintContext, PendingChild, Widget, WidgetPlacement,
};
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Color, CornerRadius};

use crate::styles::chrome::frame_radius;

/// Which Breeze popup a frame is painted as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreezePopupKind {
    /// A menu, a combo box's list or any other popup panel.
    Panel,
    /// A tooltip.
    Tooltip,
}

/// The colours of a popup frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezePopupColors {
    /// The frame's fill.
    pub fill: Color,
    /// The frame's 1 px outline.
    pub outline: Color,
}

impl BreezePopupColors {
    /// `[Hp]` The colours of a `kind` popup in `theme`, the theme the paint
    /// walker hands a widget.
    pub fn resolve(theme: &Theme, kind: BreezePopupKind) -> BreezePopupColors {
        let c = &theme.colors;
        match kind {
            BreezePopupKind::Panel => BreezePopupColors {
                fill: c.surface_raised,
                outline: c.border,
            },
            BreezePopupKind::Tooltip => BreezePopupColors {
                fill: c.tooltip_bg,
                outline: c.tooltip_border,
            },
        }
    }
}

/// Paint a `kind` popup frame on `bounds`: both shadow layers on the
/// window's rounded rectangle, then the fill and the outline on the
/// rectangle inset by half the pen.
pub fn paint_popup(canvas: &mut Canvas, bounds: Rect, kind: BreezePopupKind, theme: &Theme) {
    let shape = &theme.shape;
    let window = CornerRadius::uniform(shape.radius_popup);
    for shadow in [&shape.shadow_lg, &shape.shadow_inner_lg] {
        if shadow.color.a() > 0.0 {
            canvas.draw_shadow(bounds, window, shadow);
        }
    }
    let colors = BreezePopupColors::resolve(theme, kind);
    let pen = shape.border_width;
    let d = pen * 0.5;
    let rect = Rect::new(
        bounds.x + d,
        bounds.y + d,
        (bounds.width - 2.0 * d).max(0.0),
        (bounds.height - 2.0 * d).max(0.0),
    );
    let corner = CornerRadius::uniform(frame_radius(shape));
    if colors.fill.a() > 0.0 {
        canvas.fill_rounded_rect(rect, corner, colors.fill);
    }
    if colors.outline.a() > 0.0 && pen > 0.0 {
        canvas.stroke_rounded_rect(rect, corner, colors.outline, pen);
    }
}

/// The accessibility node a popup frame emits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PopupNode {
    /// Nothing of its own: a bare `GenericContainer`, which the adapters
    /// drop while keeping its children. For a menu, whose owner carries the
    /// container role.
    Container,
    /// A non-modal dialog named after what opened it, as the framework's
    /// popover surface is.
    Dialog(String),
    /// Hidden: the tooltip widget emits the tooltip node.
    Hidden,
}

/// A popup frame around one content subtree, `padding` inside it on every
/// side.
pub(crate) struct BreezePopupFrame {
    content: Option<WidgetId>,
    pending: Option<PendingChild>,
    kind: BreezePopupKind,
    padding: f32,
    /// The widest the frame lays its content out, even when nothing bounds
    /// it.
    max_width: Option<f32>,
    node: PopupNode,
}

impl BreezePopupFrame {
    pub(crate) fn new(
        content: WidgetId,
        kind: BreezePopupKind,
        padding: f32,
        max_width: Option<f32>,
        node: PopupNode,
    ) -> Self {
        Self {
            content: None,
            pending: Some(PendingChild::Id(content)),
            kind,
            padding,
            max_width,
            node,
        }
    }
}

impl std::fmt::Debug for BreezePopupFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezePopupFrame")
            .field("kind", &self.kind)
            .finish()
    }
}

impl Widget for BreezePopupFrame {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending.take() {
            self.content = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }
        self.children()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let inset = 2.0 * self.padding;
        let width = match self.max_width {
            Some(max) => Some(proposal.width.map_or(max, |w| w.min(max))),
            None => proposal.width,
        };
        let inner = SizeProposal {
            width: width.map(|w| (w - inset).max(0.0)),
            height: proposal.height.map(|h| (h - inset).max(0.0)),
        };
        self.content
            .and_then(|id| ctx.child_size(id, inner))
            .map(|s| Size::new(s.width + inset, s.height + inset))
            .unwrap_or_else(|| proposal.resolve(inset, inset))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        let p = self.padding;
        for child in children.iter_mut() {
            child.origin = Point::new(bounds.x + p, bounds.y + p);
            child.size = Size::new(
                (bounds.width - 2.0 * p).max(0.0),
                (bounds.height - 2.0 * p).max(0.0),
            );
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        paint_popup(canvas, bounds, self.kind, ctx.theme);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        match &self.node {
            PopupNode::Container => {
                builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
            }
            PopupNode::Dialog(name) => {
                builder.set_role(teksilo_core::accesskit::Role::Dialog);
                // An empty name would hide the `labelled_by` relation the
                // popover names its surface with.
                if !name.is_empty() {
                    builder.set_name(name);
                }
            }
            PopupNode::Hidden => builder.set_hidden(),
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.content.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_kde_scheme::KdeColor;

    fn rgba8(c: Color) -> [u8; 4] {
        KdeColor::from_color(c).to_rgba8()
    }

    /// Plasma's menu panel and tooltip, in Breeze Light and Dark.
    #[test]
    fn the_frames_are_breezes() {
        let light = crate::light();
        let panel = BreezePopupColors::resolve(&light, BreezePopupKind::Panel);
        assert_eq!(rgba8(panel.fill), [244, 245, 245, 255]);
        assert_eq!(rgba8(panel.outline), [198, 200, 201, 255]);
        let tip = BreezePopupColors::resolve(&light, BreezePopupKind::Tooltip);
        assert_eq!(rgba8(tip.fill), [247, 247, 247, 255]);
        assert_eq!(rgba8(tip.outline), [205, 205, 206, 255]);
        let dark = crate::dark();
        let panel = BreezePopupColors::resolve(&dark, BreezePopupKind::Panel);
        assert_eq!(rgba8(panel.fill), [28, 31, 34, 255]);
        assert_eq!(rgba8(panel.outline), [76, 78, 81, 255]);
        let tip = BreezePopupColors::resolve(&dark, BreezePopupKind::Tooltip);
        assert_eq!(rgba8(tip.fill), [41, 44, 48, 255]);
        assert_eq!(rgba8(tip.outline), [83, 86, 89, 255]);
    }

    /// The frame is painted from the projected tokens, so the high-contrast
    /// projection reaches its outline.
    #[test]
    fn the_frame_follows_the_high_contrast_projection() {
        let t = crate::light().for_paint_state(teksilo_core::styles::PaintState {
            window_active: true,
            high_contrast: true,
        });
        let panel = BreezePopupColors::resolve(&t, BreezePopupKind::Panel);
        assert_eq!(panel.outline, t.colors.border);
        assert_ne!(panel.outline, crate::light().colors.border);
    }
}
