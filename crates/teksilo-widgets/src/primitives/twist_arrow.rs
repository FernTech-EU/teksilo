// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! TwistArrow — a small chevron that indicates and toggles a tree node's expansion.
//!
//! Renders a right-pointing arrow when collapsed and a down-pointing arrow when
//! expanded; a leaf node (where `has_children` is false) paints nothing but
//! reserves its slot so the indent column stays aligned across all rows.
//! The glyph flips direction under right-to-left layout.
//! Accessibility-decorative: the chevron hides itself from the AT tree and
//! the parent row's node owns `set_expanded`.
//!
//! ```ignore
//! // TwistArrow is typically instantiated by TreeView row delegates and requires
//! // an EventContext to wire the tap callback. The snippet below shows the
//! // construction pattern used inside a custom tree-row build().
//! let arrow = TwistArrow::new(16.0, true, false)
//!     .on_click(|ctx| ctx.send_intent(teksilo_core::Intent::new("tree.toggle")));
//! ```
//!
//! ## Touch and pen
//!
//! A 12 dp chevron is half the 24 dp target floor and cannot grow — the indent
//! column is the tree's own geometry. It declares a `Widget::hit_outset`
//! instead, which is the one mechanism that can win here: the chevron's
//! neighbour is the row, the row takes presses, and a point inside the row is
//! at distance zero from it, so the miss-only slop pass could never reach the
//! chevron. Zero for a leaf chevron and for a decorative one, which take no
//! press.

use std::rc::Rc;

use teksilo_canvas::{Canvas, Path, Point, Rect, Size, SizeProposal};

use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::widget::{
    EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{SurfaceRole, TextRole};

use crate::primitives::rect_widget::RectWidget;

/// Small interactive chevron rendered in the leading indent column of a tree row.
pub struct TwistArrow {
    size: f32,
    has_children: bool,
    expanded: bool,
    /// Glyph colour. Defaults to [`TextRole::Secondary`] — the muted
    /// chevron every tree draws — but a row whose selection fills with a
    /// saturated colour has to move it with the label. See [`Self::color`].
    color: ColorProp,
    on_click: Option<Rc<dyn Fn(&mut EventContext)>>,
}

impl TwistArrow {
    /// Construct a chevron. `size` is the square side length in logical pixels;
    /// `has_children` determines whether the glyph is painted; `expanded`
    /// determines the glyph direction (down = expanded, right/left = collapsed).
    pub fn new(size: f32, has_children: bool, expanded: bool) -> Self {
        Self {
            size,
            has_children,
            expanded,
            color: ColorProp::TextRole(TextRole::Secondary),
            on_click: None,
        }
    }

    /// Override the glyph colour. Accepts a `Color`, a `TextRole`, or a
    /// `Signal` of either.
    ///
    /// The default `TextRole::Secondary` is a muted grey, which is right on
    /// every row that is not filled. It is *not* right on one that is: a
    /// design language whose selected row is a solid accent capsule flips
    /// its label to `TextRole::OnAccent` through
    /// `StandardItemStyle::selected_label_role`, and a chevron left behind
    /// at `Secondary` then sits on that capsule at roughly 2.5:1 — under
    /// WCAG SC 1.4.11's 3:1 floor, and visibly wrong beside a white label.
    /// `StandardTreeItem` passes the row's own label role here so the two
    /// always move together.
    pub fn color(mut self, color: impl Into<ColorProp>) -> Self {
        self.color = color.into();
        self
    }

    /// Install a tap handler. Receives the firing [`EventContext`]
    /// so consumers can dispatch intents (e.g. lazy-load children on
    /// expand) or open dialogs from the chevron toggle.
    pub fn on_click(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl std::fmt::Debug for TwistArrow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwistArrow")
            .field("size", &self.size)
            .field("has_children", &self.has_children)
            .field("expanded", &self.expanded)
            .field("color", &self.color)
            .finish()
    }
}

impl Widget for TwistArrow {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // A bound colour has to repaint the glyph when it changes — the
        // row's label role flips the moment the row becomes selected.
        self.color.register_if_bound(
            ctx.self_id(),
            ctx.binding_registry(),
            BindingLevel::RepaintOnly,
        );
        if let Some(cb) = self.on_click.clone() {
            let handlers = HandlerSet::new()
                .on_tap(move |_pos, ctx| {
                    cb(ctx);
                })
                .focusable(false)
                // The chevron lives inside a reorderable tree row that owns a drag
                // recognizer. Without this, a press here arms the row's ancestor
                // drag, and the few px of jitter a real click carries (especially
                // right after a drag) crosses the drag threshold and steals the
                // gesture — the toggle never fires and a row drag starts instead.
                // A gesture dead zone stops ancestor drag-arming at this boundary,
                // exactly as the docking accordion's trailing controls do.
                .gesture_dead_zone(true);
            ctx.apply_self_handlers(handlers);
        }
        let rect = ctx.add(RectWidget::new().background(SurfaceRole::Transparent));
        vec![rect]
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        Size::new(self.size, self.size).into()
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

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        if !self.has_children {
            return;
        }
        let color = self.color.resolve(ctx.theme, ctx.effective_enabled);
        let cx = bounds.x + bounds.width / 2.0;
        let cy = bounds.y + bounds.height / 2.0;
        let r = bounds.width.min(bounds.height) * 0.4;
        let mut path = Path::new();
        if self.expanded {
            path.move_to(Point::new(cx - r, cy - r * 0.4));
            path.line_to(Point::new(cx + r, cy - r * 0.4));
            path.line_to(Point::new(cx, cy + r * 0.6));
            path.close();
        } else if ctx.layout_direction == teksilo_core::environment::LayoutDirection::RightToLeft {
            // Collapsed glyph points toward the leading edge — left under
            // RTL — so it mirrors the direction the subtree expands into.
            path.move_to(Point::new(cx + r * 0.4, cy - r));
            path.line_to(Point::new(cx - r * 0.6, cy));
            path.line_to(Point::new(cx + r * 0.4, cy + r));
            path.close();
        } else {
            path.move_to(Point::new(cx - r * 0.4, cy - r));
            path.line_to(Point::new(cx + r * 0.6, cy));
            path.line_to(Point::new(cx - r * 0.4, cy + r));
            path.close();
        }
        canvas.fill_path(&path, color);
    }

    /// A 12 dp chevron is half the 24 dp conformance floor, and it cannot grow:
    /// the indent column it sits in is the tree's own geometry, and widening it
    /// at Compact would move every row's label.
    ///
    /// So the shortfall is made up between the pointer and the arena. This is
    /// the one place it *has* to be an outset rather than the miss-only slop
    /// pass: the chevron's neighbour is the row, the row takes presses, and a
    /// point inside the row is at distance zero from it — so the slop pass, which
    /// only re-attributes to a candidate strictly closer than the bubble owner,
    /// can never reach the chevron. An outset is tested inside the exact pass
    /// and wins.
    ///
    /// Zero for a chevron that would refuse the press anyway — no children, or
    /// no `on_click` — because a widened node that then ignores the press is a
    /// hole punched in the row behind it.
    fn hit_outset(
        &self,
        kind: teksilo_tokens::PointerKind,
        tokens: &teksilo_tokens::InputTokens,
    ) -> teksilo_canvas::EdgeInsets {
        if !self.has_children || self.on_click.is_none() {
            return teksilo_canvas::EdgeInsets::ZERO;
        }
        crate::button::target_outset(Size::new(self.size, self.size), kind, tokens)
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use teksilo_core::pointer::PointerPhase;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_tokens::{InputTokens, PointerKind};

    use crate::button::press_test_support::{finger, touch};
    use crate::primitives::{HStack, TextWidget};

    /// A 12 dp chevron beside a 200 dp row label, both inside a row that takes
    /// taps of its own: the row is the shape the census names as the reason the
    /// slop pass cannot serve the chevron.
    fn row_with_chevron(
        toggles: std::rc::Rc<Cell<u32>>,
        row_taps: std::rc::Rc<Cell<u32>>,
    ) -> (WidgetTree, WidgetId) {
        use teksilo_core::widget_builder::WidgetBuilder;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let arrow = tree.add(
            TwistArrow::new(12.0, true, false).on_click(move |_| toggles.set(toggles.get() + 1)),
        );
        let label = tree.add(TextWidget::new(teksilo_i18n::lit!("Documents")));
        let _row = tree.add(
            HStack::new()
                .child(arrow)
                .child(label)
                .on_tap(move |_e, _c| row_taps.set(row_taps.get() + 1)),
        );
        tree.layout(SizeProposal::exact(240.0, 28.0));
        (tree, arrow)
    }

    /// The outset takes a finger that landed beside the glyph, inside the row.
    #[test]
    fn a_finger_just_outside_the_chevron_still_toggles() {
        let toggles = std::rc::Rc::new(Cell::new(0));
        let row_taps = std::rc::Rc::new(Cell::new(0));
        let (mut tree, arrow) = row_with_chevron(toggles.clone(), row_taps.clone());
        let b = tree.bounds(arrow);
        // 4 dp past the glyph's trailing edge — outside the 12 dp box, inside
        // the 24 dp target the outset earns it.
        let at = teksilo_canvas::Point::new(b.x + b.width + 4.0, b.center().y);
        let id = finger();
        tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
        tree.dispatch_pointer(touch(id, PointerPhase::Up, at, 30));
        assert_eq!(
            toggles.get(),
            1,
            "the chevron's outset did not take the press"
        );
        assert_eq!(row_taps.get(), 0, "and the row must not have taken it too");
    }

    /// A mouse is exact: the same press lands on the row, exactly as it did
    /// before the touch programme.
    #[test]
    fn a_mouse_just_outside_the_chevron_lands_on_the_row() {
        let toggles = std::rc::Rc::new(Cell::new(0));
        let row_taps = std::rc::Rc::new(Cell::new(0));
        let (mut tree, arrow) = row_with_chevron(toggles.clone(), row_taps.clone());
        let b = tree.bounds(arrow);
        let at = teksilo_canvas::Point::new(b.x + b.width + 4.0, b.center().y);
        tree.dispatch_event(teksilo_core::event::WidgetEvent::pointer_down(
            at,
            teksilo_core::event::PointerButton::Primary,
            teksilo_core::event::Modifiers::NONE,
        ));
        tree.dispatch_event(teksilo_core::event::WidgetEvent::pointer_up(
            at,
            teksilo_core::event::PointerButton::Primary,
            teksilo_core::event::Modifiers::NONE,
        ));
        assert_eq!(
            toggles.get(),
            0,
            "a mouse must not be given the chevron's outset"
        );
        assert_eq!(row_taps.get(), 1);
    }

    /// A chevron that paints nothing and does nothing claims no space: a leaf
    /// row's indent slot must stay transparent to the row behind it.
    #[test]
    fn an_inert_chevron_declares_no_outset() {
        let tokens = InputTokens::default();
        let leaf = TwistArrow::new(12.0, false, false).on_click(|_| {});
        assert_eq!(
            leaf.hit_outset(PointerKind::Touch, &tokens),
            teksilo_canvas::EdgeInsets::ZERO,
        );
        let decorative = TwistArrow::new(12.0, true, false);
        assert_eq!(
            decorative.hit_outset(PointerKind::Touch, &tokens),
            teksilo_canvas::EdgeInsets::ZERO,
        );
    }

    /// The outset reaches exactly the conformance floor at Compact and the
    /// density's own target above it — and never applies to a mouse.
    #[test]
    fn the_outset_lifts_the_chevron_to_the_density_target() {
        let arrow = TwistArrow::new(12.0, true, false).on_click(|_| {});
        for density in [
            teksilo_tokens::TargetDensity::Compact,
            teksilo_tokens::TargetDensity::Comfortable,
            teksilo_tokens::TargetDensity::Touch,
        ] {
            let tokens = InputTokens::for_density(density);
            let out = arrow.hit_outset(PointerKind::Touch, &tokens);
            assert_eq!(
                12.0 + out.horizontal(),
                tokens.target_size,
                "{density:?} horizontal",
            );
            assert_eq!(
                12.0 + out.vertical(),
                tokens.target_size,
                "{density:?} vertical"
            );
            assert_eq!(
                arrow.hit_outset(PointerKind::Mouse, &tokens),
                teksilo_canvas::EdgeInsets::ZERO,
                "{density:?} mouse",
            );
        }
    }
}
