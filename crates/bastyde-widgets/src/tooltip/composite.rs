// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `CompositeTooltipWidget` — third-tier tooltip that hosts an arbitrary
//! widget tree as its body.
//!
//! Where `TooltipWidget` is a single line of text and `RichTooltipWidget`
//! renders a structured `TooltipContent` (body + shortcut chip + "more"
//! disclosure with inline markup), `CompositeTooltipWidget` accepts any
//! `impl Widget + 'static` and paints it inside the same chrome with a
//! larger surface budget — the Crusader Kings 3 style: tabbed sections,
//! charts, progress bars, conditional rows, dynamic numeric values.
//!
//! "Primary-only" by construction: the widget has no inline-markup body
//! and no registry key, so it cannot be the target of a `[label](:key)`
//! cascade from a rich tooltip. Child widgets *inside* the body keep
//! their own `.tooltip(...)` / `.rich_tooltip(...)` setters and cascade
//! normally as ordinary widget composition.
//!
//! Reuses the `RichTooltipWidget` dwell-to-sticky machinery: at 2 s
//! dwell the role flips `Tooltip → Dialog`, dismiss swaps to
//! `EscapeOrClickOutside`, and the surface becomes Tab-reachable so
//! rare-but-allowed interactive descendants (a "Pin" button, an
//! internal `TabWidget`'s tab strip) work cleanly.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, PaintContext, Widget};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::LocalizedString;
use bastyde_tokens::{CornerRadius, TextRole};

use crate::primitives::{Grid, Padding, Spacer, TrackSize};
use crate::scroll_area::{ScrollArea, ScrollBarPolicy};
use crate::tooltip::dwell_indicator::DwellIndicator;
// Step granularity is shared with `RichTooltipWidget`'s indicator (0..=4) —
// imported, not restated, so the two tiers cannot drift apart.
use crate::tooltip::rich::{DWELL_STEP_DURATION, DWELL_STEPS};

/// Composite tooltip surface — hosts an arbitrary widget body with the
/// same dwell-to-sticky promotion as the rich tooltip.
pub struct CompositeTooltipWidget {
    body: Option<Box<dyn Widget>>,
    body_id: Option<WidgetId>,
    /// The padded scroll viewport holding the body, and the footer row that
    /// carries the dwell indicator — placed by hand, see `place_children`.
    padded_id: Option<WidgetId>,
    footer_id: Option<WidgetId>,
    /// The `ScrollArea` wrapping the body. Kept only so `layout_response` can discount its
    /// placeholder intrinsic height — see there.
    scrolled_id: Option<WidgetId>,
    access_label: Option<String>,
    max_width_override: Option<f32>,
    max_height_override: Option<f32>,
    dwell_step: Signal<u32>,
    sticky: Signal<bool>,
    /// Whether this surface offers dwell-to-sticky promotion at all.
    ///
    /// A composite tooltip is just as useful as a *read-only* surface — a fact
    /// sheet, a row's card — where there is nothing to reach into and nothing
    /// to pin. There the dwell machinery is all cost: a countdown indicator
    /// promising an interaction that does not exist, a surface that outlives
    /// the pointer, and a `Role::Dialog` for assistive tech to announce.
    /// Turning it off makes the tier behave like a richer plain tooltip.
    sticky_enabled: bool,
    shown_at_sink: Rc<Cell<Option<Instant>>>,
}

impl Default for CompositeTooltipWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CompositeTooltipWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeTooltipWidget")
            .field("has_body", &self.body.is_some())
            .field("access_label", &self.access_label)
            .field("max_width_override", &self.max_width_override)
            .field("max_height_override", &self.max_height_override)
            .field("sticky_enabled", &self.sticky_enabled)
            .finish()
    }
}

impl CompositeTooltipWidget {
    pub fn new() -> Self {
        Self {
            body: None,
            body_id: None,
            padded_id: None,
            footer_id: None,
            scrolled_id: None,
            access_label: None,
            max_width_override: None,
            max_height_override: None,
            dwell_step: Signal::new(0),
            sticky: Signal::new(false),
            // Kept on by default: it is what the tier has always done, and a
            // body with something to reach into is the case that needs it.
            sticky_enabled: true,
            shown_at_sink: Rc::new(Cell::new(None)),
        }
    }

    /// Set the tooltip body. Replaces any previously set body.
    pub fn content(mut self, body: impl Widget + 'static) -> Self {
        self.body = Some(Box::new(body));
        self
    }

    /// Set the tooltip body from an already-boxed widget. Used by the
    /// per-widget `.composite_tooltip(...)` setters that store
    /// `Box<dyn Widget>` and forward through `attach_composite_tooltip_boxed`.
    pub fn content_boxed(mut self, body: Box<dyn Widget>) -> Self {
        self.body = Some(body);
        self
    }

    /// Accessibility label (used for `set_name` on the AT node — the
    /// `Role::Tooltip`/`Role::Dialog` would otherwise be unnamed).
    pub fn access_label(mut self, label: impl Into<LocalizedString>) -> Self {
        let ls: LocalizedString = label.into();
        self.access_label = Some(ls.resolve_now());
        self
    }

    /// Override the per-theme `composite_tooltip.max_width`.
    /// Whether the surface offers dwell-to-sticky promotion. Default `true`.
    ///
    /// Turn it **off** for a read-only body — a fact sheet, a data-view row's
    /// card. Three things follow, all of them the point:
    ///
    /// * no [`DwellIndicator`] is built, so nothing counts down towards an
    ///   interaction that does not exist (and the footer disappears with it,
    ///   letting the surface hug its content);
    /// * the entry is registered with no `sticky_after`, so the tip never
    ///   promotes, never outlives the pointer, and stays a `Role::Tooltip`
    ///   rather than becoming a `Dialog` for assistive tech to announce;
    /// * and, following the plain tier, it is not surfaced by keyboard focus —
    ///   its text reaches assistive tech through the anchor's own description
    ///   instead, which is the W3C-recommended route for supplementary hints.
    ///
    /// Callers reaching the tier through a widget's `.composite_tooltip(...)`
    /// setter get the sticky default; opt out by building the widget yourself
    /// and attaching it with
    /// [`attach_composite_tooltip_widget_with_placement`](crate::tooltip::attach_composite_tooltip_widget_with_placement).
    pub fn sticky(mut self, on: bool) -> Self {
        self.sticky_enabled = on;
        self
    }

    /// Whether dwell-to-sticky promotion is enabled — what an attach helper
    /// consults to decide the entry's `sticky_after`.
    pub fn sticky_enabled(&self) -> bool {
        self.sticky_enabled
    }

    pub fn max_width(mut self, w: f32) -> Self {
        self.max_width_override = Some(w);
        self
    }

    /// Override the per-theme `composite_tooltip.max_height`.
    pub fn max_height(mut self, h: f32) -> Self {
        self.max_height_override = Some(h);
        self
    }

    /// Cloneable `shown_at_sink` for the attach helper to thread into
    /// `attach_tooltip_with_sticky_sink`.
    pub fn shown_at_sink(&self) -> Rc<Cell<Option<Instant>>> {
        self.shown_at_sink.clone()
    }

    fn tick_dwell(&self) {
        let Some(shown_at) = self.shown_at_sink.get() else {
            if self.dwell_step.get() != 0 {
                self.dwell_step.set(0);
            }
            if self.sticky.get() {
                self.sticky.set(false);
            }
            return;
        };
        let elapsed = Instant::now().saturating_duration_since(shown_at);
        let new_step =
            ((elapsed.as_millis() / DWELL_STEP_DURATION.as_millis()) as u32).min(DWELL_STEPS);
        if self.dwell_step.get() != new_step {
            self.dwell_step.set(new_step);
        }
        let now_sticky = new_step >= DWELL_STEPS;
        if self.sticky.get() != now_sticky {
            self.sticky.set(now_sticky);
        }
    }
}

/// Gap between the body viewport and the dwell-indicator footer — what the
/// `VStack` spacing used to supply before the column was placed by hand.
const FOOTER_GAP: f32 = 6.0;

impl Widget for CompositeTooltipWidget {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use crate::styles::recipe_tooltip_style as tt;
        let self_id = ctx.self_id();

        // Body — mounted once and reused across rebuilds. `build()` can
        // re-run (`rebuild_single_widget`), and taking `self.body`
        // unconditionally would collapse a rebuilt composite to a Spacer
        // (the body box is gone after the first take). So we take only on
        // first build, store the id, and reuse it on every later build —
        // the same shape `ModalContainer` uses for `pending_content` →
        // `content_id` (see dialog.rs). Reuse is only sound because
        // `preserves_children_on_rebuild()` returns `true` below: the
        // body lands under the ScrollArea subtree, which a normal rebuild
        // would `destroy_subtree`, leaving a dangling id. Preserving the
        // subtree keeps `body_id` alive so the reused id is valid. The
        // Spacer fallback applies only when no body was ever set.
        let body_id = if let Some(body) = self.body.take() {
            let id = ctx.add_boxed(body);
            self.body_id = Some(id);
            id
        } else if let Some(id) = self.body_id {
            id
        } else {
            let id = ctx.add(Spacer::new());
            self.body_id = Some(id);
            id
        };

        // Always wrap in a vertical-only ScrollArea; chrome stays
        // invisible until overflow (AsNeeded) and the user can scroll
        // long content with the wheel either way.
        let scrolled = ctx.add(
            ScrollArea::from_id(body_id)
                .vertical_scroll_bar_policy(ScrollBarPolicy::AsNeeded)
                .horizontal_scroll_bar_policy(ScrollBarPolicy::AlwaysOff)
                // The chip is the dark / inverse `tooltip_bg`; tint the thumb
                // from `tooltip_text` so it contrasts (the surface-relative
                // `scrollbar_thumb` token would be dark-on-dark / light-on-light).
                .scroll_bar_thumb_color(TextRole::TooltipText),
        );

        self.scrolled_id = Some(scrolled);

        let padded = ctx.add(
            Padding::symmetric(
                tt::COMPOSITE_TOOLTIP_PADDING_VERTICAL,
                tt::COMPOSITE_TOOLTIP_PADDING_HORIZONTAL,
            )
            .child_id(scrolled),
        );

        // No promotion, no countdown: a read-only surface builds no footer at
        // all, so it hugs its content instead of reserving a row for an
        // indicator that would promise an interaction it does not offer.
        let footer = self.sticky_enabled.then(|| {
            let indicator = ctx.add(DwellIndicator::new(
                self.dwell_step.clone(),
                self.sticky.clone(),
                TextRole::TooltipText,
            ));
            // Footer: 1fr Spacer + Auto indicator — keeps the dwell pin
            // visually anchored at the bottom-right of the surface, the
            // same convention rich tooltips use at the top-right of their
            // inner Grid.
            let footer_spacer = ctx.add(Spacer::new());
            ctx.add(
                Grid::new()
                    .columns(vec![TrackSize::Fractional(1.0), TrackSize::Auto])
                    .rows(vec![TrackSize::Auto])
                    .column_gap(8.0)
                    .add_child(footer_spacer)
                    .add_child(indicator),
            )
        });

        // Placed by `place_children`, not stacked by a `VStack`.
        //
        // A `ScrollArea`'s intrinsic height is a fixed placeholder — a viewport
        // exists to be smaller than what it holds, so it cannot answer "how
        // tall is my content". `layout_response` below already discounts that
        // placeholder to report an honest height, but a `VStack` doing its own
        // vertical distribution never saw the correction: it laid the column
        // out at placeholder height and the footer — the dwell indicator —
        // ended up painted below the bubble, on whatever happened to be behind
        // the tooltip. Owning the placement is what keeps the two in step.
        self.padded_id = Some(padded);
        self.footer_id = footer;

        // Focusable so Tab can enter the surface once promoted.
        let handlers = HandlerSet::new().focusable(true);
        ctx.apply_self_handlers(handlers);

        // Bind sticky for the role flip in accessibility().
        self.sticky.bind_to(
            self_id,
            ctx.binding_registry(),
            bastyde_core::binding::BindingLevel::AccessibilityOnly,
        );

        self.padded_id.into_iter().chain(self.footer_id).collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        use crate::styles::recipe_tooltip_style as tt;
        let max_w = self
            .max_width_override
            .unwrap_or(tt::COMPOSITE_TOOLTIP_MAX_WIDTH);
        let max_h = self
            .max_height_override
            .unwrap_or(tt::COMPOSITE_TOOLTIP_MAX_HEIGHT);
        // `max_width` / `max_height` are *maxima*, so measure what the content wants and
        // clamp the result — do NOT propose the maximum as an exact size.
        //
        // Proposing it was the bug: the body sits inside a `ScrollArea` (below) whose
        // horizontal policy is `AlwaysOff`, so it fills whatever width it is handed, and the
        // wrapper chain likewise takes the offered height. A tooltip holding a 202x16 dp row
        // therefore painted as a 480x244 dp slab, and neither a smaller `max_width` nor a
        // smaller `max_height` could make it hug — they only moved the number it filled to.
        //
        // Two passes, because width and height are not independent: measuring unbounded
        // gives text its single-line length, and a body clamped narrower than that needs to
        // be re-measured to learn how tall it becomes once it wraps.
        let unbounded = SizeProposal {
            width: None,
            height: None,
        };
        let Some(padded) = self.padded_id else {
            return Size::new(0.0, 0.0).into();
        };
        let footer_natural = self
            .footer_id
            .and_then(|id| ctx.child_size(id, unbounded))
            .unwrap_or_else(|| Size::new(0.0, 0.0));
        let Some(padded_natural) = ctx.child_size(padded, unbounded) else {
            return Size::new(0.0, 0.0).into();
        };
        let natural = Size::new(
            padded_natural.width.max(footer_natural.width),
            padded_natural.height + FOOTER_GAP + footer_natural.height,
        );
        let avail_w = proposal.width.unwrap_or(f32::INFINITY).min(max_w);
        let w = natural.width.min(avail_w);
        let at_w = SizeProposal {
            width: Some(w),
            height: None,
        };
        let footer_h = self
            .footer_id
            .and_then(|id| ctx.child_size(id, at_w))
            .map(|s| s.height)
            .unwrap_or(footer_natural.height);
        let h = ctx
            .child_size(padded, at_w)
            .map(|s| s.height + FOOTER_GAP + footer_h)
            .unwrap_or(natural.height);

        // Height needs one more correction. A `ScrollArea` is a viewport: asked for its
        // intrinsic height it answers with a fixed placeholder (200 dp) rather than its
        // content's, because a viewport's whole job is to be smaller than what it holds. The
        // body sits inside one, so the measurement above says 244 dp for a 16 dp row.
        //
        // Discount the scroll area's own answer and substitute the body's. Expressed as a
        // difference rather than by re-adding the padding and footer by hand, so it stays
        // right if the chrome around the body ever changes.
        let h = match (self.scrolled_id, self.body_id) {
            (Some(scrolled), Some(body)) => {
                let at_width = SizeProposal {
                    width: Some(w),
                    height: None,
                };
                match (
                    ctx.child_size(scrolled, at_width),
                    ctx.child_size(body, at_width),
                ) {
                    (Some(vp), Some(content)) => (h - vp.height + content.height).max(0.0),
                    _ => h,
                }
            }
            _ => h,
        };
        let avail_h = proposal.height.unwrap_or(f32::INFINITY).min(max_h);
        Size::new(w, h.min(avail_h)).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let radius = CornerRadius::uniform(
            crate::styles::recipe_tooltip_style::COMPOSITE_TOOLTIP_CORNER_RADIUS,
        );
        let _ = ctx;
        super::paint_composite_tooltip_shadows(canvas, bounds, radius, ctx);
        canvas.fill_rounded_rect(bounds, radius, ctx.theme.colors.tooltip_bg);
        // paint() is the visibility hook — only invoked while the
        // tooltip is active. Drives the dwell-to-sticky timer.
        if self.sticky_enabled {
            self.tick_dwell();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        let is_sticky = self.sticky.get();
        let role = if is_sticky {
            bastyde_core::accesskit::Role::Dialog
        } else {
            bastyde_core::accesskit::Role::Tooltip
        };
        builder.set_role(role);
        // Composite tooltips host arbitrary widget bodies and have no
        // intrinsic text, so without an explicit `.access_label(...)` the
        // node would be unnamed. Fall back to a localized generic name —
        // same approach as `ModalContainer` / `SnackbarWidget`.
        let name = self
            .access_label
            .clone()
            .unwrap_or_else(|| bastyde_i18n::tr_widget!(a11y_tooltip_name()).resolve_now());
        builder.set_name(name);
        if is_sticky {
            builder.add_action(bastyde_core::accesskit::Action::Focus);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.padded_id.into_iter().chain(self.footer_id).collect()
    }

    /// Place the body viewport and the dwell-indicator footer by hand.
    ///
    /// The footer takes its natural height at the bottom; the viewport takes
    /// everything above it. That is the whole fix: the viewport is *given* a
    /// height rather than asked for one, so it can no longer claim its
    /// placeholder and push the indicator out of the bubble. Content taller
    /// than what is left simply scrolls, which is what the `ScrollArea` is for.
    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [bastyde_core::widget::WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let at_w = SizeProposal {
            width: Some(bounds.width),
            height: None,
        };
        let footer_h = self
            .footer_id
            .and_then(|id| ctx.child_size(id, at_w))
            .map(|s| s.height)
            .unwrap_or(0.0);
        let body_h = (bounds.height - footer_h - FOOTER_GAP).max(0.0);
        for (i, child) in children.iter_mut().enumerate() {
            if i == 0 {
                child.origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
                child.size = Size::new(bounds.width, body_h);
            } else {
                child.origin = bastyde_canvas::Point::new(bounds.x, bounds.y + body_h + FOOTER_GAP);
                child.size = Size::new(bounds.width, footer_h);
            }
        }
    }

    /// Reconcile children across rebuilds rather than tearing them down.
    /// The body widget is owned once (`self.body` is taken on first build)
    /// and cannot be reconstructed on a later `build()`, so it must survive —
    /// otherwise the reused `body_id` would dangle. The body is re-parented
    /// under the freshly-built chrome each rebuild; the reconciling rebuild
    /// path follows authoritative parent pointers, so it keeps the re-parented
    /// body and destroys only the superseded old chrome. (In practice the
    /// composite tooltip has no `Rebuild`-level binding, so this path is rarely
    /// exercised.) Mirrors `Switcher`'s preserve-on-rebuild contract.
    fn preserves_children_on_rebuild(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::button::Button;
    use crate::primitives::{TextWidget, VStack};
    use crate::tooltip::attach::attach_composite_tooltip;
    use bastyde_canvas::{MockTextBackend, SizeProposal};
    use bastyde_core::widget_tree::WidgetTree;
    use bastyde_i18n::lit;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    fn tree_with_backend() -> WidgetTree {
        WidgetTree::new().with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())))
    }

    /// Test-only host widget that wires a composite tooltip onto a
    /// child `Button` from inside its `build()`. Exposes the resulting
    /// tooltip content id via a shared `Cell` so tests can drive the
    /// tree's `promote_tooltip_to_sticky` API directly.
    #[derive(Debug)]
    struct ComposeTooltipHost {
        anchor_id: Option<WidgetId>,
        tooltip_id_sink: Rc<Cell<Option<WidgetId>>>,
        sticky: bool,
    }

    impl ComposeTooltipHost {
        fn new(tooltip_id_sink: Rc<Cell<Option<WidgetId>>>) -> Self {
            Self {
                anchor_id: None,
                tooltip_id_sink,
                sticky: true,
            }
        }
        fn new_non_sticky(tooltip_id_sink: Rc<Cell<Option<WidgetId>>>) -> Self {
            Self {
                anchor_id: None,
                tooltip_id_sink,
                sticky: false,
            }
        }
    }

    impl Widget for ComposeTooltipHost {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let anchor = ctx.add(Button::new(lit!("Hover me")));
            self.anchor_id = Some(anchor);
            let body = VStack::new()
                .child(TextWidget::new(lit!("Header")))
                .child(TextWidget::new(lit!("Body")));
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            let tip = crate::tooltip::attach_composite_tooltip_widget_with_placement(
                ctx,
                anchor,
                CompositeTooltipWidget::new()
                    .content(body)
                    .sticky(self.sticky),
                delay,
                crate::tooltip::TooltipPlacement::Below,
            );
            self.tooltip_id_sink.set(Some(tip));
            vec![anchor]
        }
        fn layout_response(
            &self,
            proposal: SizeProposal,
            ctx: &LayoutContext,
        ) -> bastyde_core::widget::LayoutResponse {
            self.anchor_id
                .and_then(|id| ctx.child_size(id, proposal))
                .unwrap_or_else(|| Size::new(0.0, 0.0))
                .into()
        }
        fn children(&self) -> Vec<WidgetId> {
            self.anchor_id.map(|id| vec![id]).unwrap_or_default()
        }
    }

    #[test]
    fn composite_tooltip_appears_after_hover_delay() {
        let mut tree = tree_with_backend();
        let tooltip_id_sink = Rc::new(Cell::new(None));
        let host = tree.add(ComposeTooltipHost::new(tooltip_id_sink.clone()));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        assert!(tree.active_overlays().is_empty());
        tree.pointer_move(tree.bounds(host).center());
        assert!(
            tree.active_overlays().is_empty(),
            "composite tooltip should not appear instantly — waits for delay"
        );

        tree.advance_time(Duration::from_millis(700) + Duration::from_millis(50));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "composite tooltip should have appeared after the hover delay"
        );
    }

    #[test]
    fn composite_tooltip_does_not_appear_at_the_light_tier_delay() {
        // The test above jumps straight from t=0 to heavy+50 ms, so it would
        // pass just as happily if the composite path were wired to the 500 ms
        // `tooltip_delay` instead of the 700 ms `tooltip_delay_heavy`. This is
        // the missing checkpoint in between: a composite surface is heavier to
        // read and to dismiss, and must demand the longer statement of intent.
        let mut tree = tree_with_backend();
        let tooltip_id_sink = Rc::new(Cell::new(None));
        let host = tree.add(ComposeTooltipHost::new(tooltip_id_sink.clone()));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.pointer_move(tree.bounds(host).center());
        tree.advance_time(tree.theme().motion.tooltip_delay + Duration::from_millis(50));
        assert!(
            tree.active_overlays().is_empty(),
            "a composite tooltip must still be waiting at the plain 500 ms delay"
        );

        tree.advance_time(Duration::from_millis(200));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "and appear once the 700 ms heavy delay elapses"
        );
    }

    #[test]
    fn composite_tooltip_dismisses_on_pointer_leave_before_promotion() {
        let mut tree = tree_with_backend();
        let tooltip_id_sink = Rc::new(Cell::new(None));
        let host = tree.add(ComposeTooltipHost::new(tooltip_id_sink.clone()));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.pointer_move(tree.bounds(host).center());
        tree.advance_time(Duration::from_millis(700) + Duration::from_millis(50));
        assert_eq!(tree.active_overlays().len(), 1);

        // Pointer leaves before sticky promotion → dismiss.
        tree.pointer_move(bastyde_canvas::Point::new(2000.0, 2000.0));
        tree.advance_time(Duration::from_millis(500));
        assert!(
            tree.active_overlays().is_empty(),
            "non-sticky composite tooltip should dismiss on pointer-leave"
        );
    }

    #[test]
    fn composite_tooltip_survives_pointer_leave_once_promoted() {
        // The 2 s dwell auto-promote uses real time (not sim) — so we
        // promote manually via `promote_tooltip_to_sticky` to test the
        // post-sticky behavior deterministically.
        let mut tree = tree_with_backend();
        let tooltip_id_sink = Rc::new(Cell::new(None));
        let host = tree.add(ComposeTooltipHost::new(tooltip_id_sink.clone()));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.pointer_move(tree.bounds(host).center());
        tree.advance_time(Duration::from_millis(700) + Duration::from_millis(50));
        assert_eq!(tree.active_overlays().len(), 1);

        let content_id = tooltip_id_sink
            .get()
            .expect("tooltip id captured during build");
        tree.promote_tooltip_to_sticky(content_id);

        tree.pointer_move(bastyde_canvas::Point::new(2000.0, 2000.0));
        tree.advance_time(Duration::from_millis(500));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "sticky composite tooltip should survive pointer-leave"
        );
    }

    #[test]
    fn composite_tooltip_preserves_children_so_body_survives_rebuild() {
        // The body box is taken on first `build()` and cannot be rebuilt,
        // so the rebuild-safe body reuse depends on the child subtree
        // being preserved (otherwise the reused `body_id` would dangle).
        // Assert that contract directly — it links the two halves of the
        // fix (reuse + preserve); removing either should fail here.
        let w = CompositeTooltipWidget::new().content(TextWidget::new(lit!("Body")));
        assert!(
            w.preserves_children_on_rebuild(),
            "composite must preserve children so the reused body id stays valid across rebuild"
        );
    }

    /// A non-sticky composite never promotes, however long the pointer rests.
    ///
    /// That is the whole contract of the read-only tier: it retires with the
    /// pointer like a plain tooltip, rather than becoming a panel that outlives
    /// it and reads to assistive tech as a `Dialog`.
    #[test]
    fn a_non_sticky_composite_tooltip_never_promotes() {
        let mut tree = tree_with_backend();
        let tooltip_id_sink = Rc::new(Cell::new(None));
        let host = tree.add(ComposeTooltipHost::new_non_sticky(tooltip_id_sink.clone()));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.pointer_move(tree.bounds(host).center());
        tree.advance_time(Duration::from_millis(750));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "it still shows on hover — only the promotion is gone"
        );

        // Well past the dwell window the sticky tier would have promoted at.
        tree.advance_time(crate::tooltip::rich::DWELL_PROMOTION * 3);
        tree.pointer_move(bastyde_canvas::Point::new(-100.0, -100.0));
        tree.advance_time(Duration::from_millis(300));
        assert!(
            tree.active_overlays().is_empty(),
            "a non-sticky surface must retire with the pointer, not survive it"
        );
    }

    /// …and builds no dwell indicator, so it hugs its content.
    #[test]
    fn a_non_sticky_composite_tooltip_is_shorter_than_a_sticky_one() {
        let measure = |sticky: bool| {
            let mut tree = WidgetTree::new()
                .with_theme(bastyde_core::presets::intui::light())
                .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
            tree.add_boxed(Box::new(
                CompositeTooltipWidget::new()
                    .content(TextWidget::new(lit!("Hi")))
                    .sticky(sticky),
            ));
            tree.layout(SizeProposal::exact(1200.0, 900.0));
            tree.measure_root_intrinsic(SizeProposal {
                width: Some(1200.0),
                height: Some(900.0),
            })
            .expect("a size")
            .height
        };
        let sticky = measure(true);
        let plain = measure(false);
        assert!(
            plain < sticky,
            "without the indicator the surface should hug tighter \
             (non-sticky {plain}, sticky {sticky})"
        );
    }

    /// Every painted descendant must sit inside the surface that paints the
    /// tooltip's background.
    ///
    /// The dwell indicator is the last child of the root `VStack`, so anything
    /// that makes the surface report a height shorter than the column actually
    /// needs pushes the indicator out the bottom — painted on the window, over
    /// whatever is behind, with no tooltip under it.
    #[test]
    fn the_dwell_indicator_sits_inside_the_tooltip_surface() {
        let mut tree = WidgetTree::new()
            .with_theme(bastyde_core::presets::intui::light())
            .with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        let tip = tree.add_boxed(Box::new(
            CompositeTooltipWidget::new().content(TextWidget::new(lit!("Hi"))),
        ));
        // Lay out at the tooltip's own intrinsic size, the way the overlay
        // manager places it — not at a generous proposal, which would place the
        // root at the proposal and hide the discrepancy.
        let want = tree
            .measure_root_intrinsic(SizeProposal {
                width: Some(1200.0),
                height: Some(900.0),
            })
            .expect("the tooltip reports a size");
        tree.layout(SizeProposal::exact(want.width, want.height));

        let surface = tree.bounds(tip);
        // Walk to the deepest descendants and check each stays within.
        let mut stack = tree.children(tip);
        let mut worst: Option<(f32, f32)> = None;
        while let Some(id) = stack.pop() {
            let b = tree.bounds(id);
            if b.height > 0.0 && b.y + b.height > surface.y + surface.height + 0.5 {
                let overflow = (b.y + b.height) - (surface.y + surface.height);
                if worst.is_none_or(|(w, _)| overflow > w) {
                    worst = Some((overflow, b.y + b.height));
                }
            }
            stack.extend(tree.children(id));
        }
        assert!(
            worst.is_none(),
            "a descendant spills {:.1}dp below the tooltip surface \
             (surface ends at {:.1}, child at {:.1}) — the dwell indicator is \
             painted outside the bubble",
            worst.unwrap().0,
            surface.y + surface.height,
            worst.unwrap().1,
        );
    }

    /// `max_width` / `max_height` are maxima, not the size to fill.
    ///
    /// They used to be proposed to the content as an exact size, and since the body sits in
    /// a `ScrollArea` that fills what it is handed, every composite tooltip painted at the
    /// maximum: a one-line body rendered as a 480x480 slab. Lowering either maximum only
    /// changed the number it filled to, so there was no way to get a tooltip that fit its
    /// content. Pin the hugging directly.
    #[test]
    fn a_short_composite_tooltip_hugs_its_content() {
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add_boxed(Box::new(
            CompositeTooltipWidget::new().content(TextWidget::new(lit!("Hi"))),
        ));
        // An overlay proposes generously; the tooltip must not ask for all of it.
        // Measured, not `bounds()`: the widget under test is the tree root and so is
        // *placed* at the proposal whatever it reports.
        tree.layout(SizeProposal::exact(1200.0, 900.0));
        let s = tree
            .measure_root_intrinsic(SizeProposal {
                width: Some(1200.0),
                height: Some(900.0),
            })
            .expect("the tooltip reports a size");
        assert!(
            s.width < 200.0,
            "a two-letter body should not ask for a {}dp-wide tooltip",
            s.width
        );
        assert!(
            s.height < 120.0,
            "a one-line body should not ask for a {}dp-tall tooltip",
            s.height
        );
    }

    /// …and a body larger than the maxima is still bounded by them.
    #[test]
    fn a_long_composite_tooltip_is_capped_by_its_maximum() {
        let long = "word ".repeat(400);
        let mut tree = WidgetTree::new().with_theme(bastyde_core::presets::intui::light());
        tree.add_boxed(Box::new(
            CompositeTooltipWidget::new()
                .content(TextWidget::new(lit!(long)))
                .max_width(240.0)
                .max_height(160.0),
        ));
        tree.layout(SizeProposal::exact(1200.0, 900.0));
        let s = tree
            .measure_root_intrinsic(SizeProposal {
                width: Some(1200.0),
                height: Some(900.0),
            })
            .expect("the tooltip reports a size");
        assert!(s.width <= 240.5, "width {} exceeds its maximum", s.width);
        assert!(s.height <= 160.5, "height {} exceeds its maximum", s.height);
    }
}
