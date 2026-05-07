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
use std::time::{Duration, Instant};

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, PaintContext, Widget};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_i18n::LocalizedString;
use fern_tokens::CornerRadius;

use crate::primitives::{Grid, Padding, Spacer, TrackSize, VStack};
use crate::scroll_area::{ScrollArea, ScrollBarPolicy};
use crate::tooltip::dwell_indicator::DwellIndicator;
use crate::tooltip::rich::DWELL_PROMOTION;

/// Step granularity matches `RichTooltipWidget`'s indicator (0..=4).
const DWELL_STEPS: u32 = 4;
const DWELL_STEP_DURATION: Duration =
    Duration::from_millis((DWELL_PROMOTION.as_millis() / DWELL_STEPS as u128) as u64);

/// Composite tooltip surface — hosts an arbitrary widget body with the
/// same dwell-to-sticky promotion as the rich tooltip.
pub struct CompositeTooltipWidget {
    body: Option<Box<dyn Widget>>,
    body_id: Option<WidgetId>,
    root_child_id: Option<WidgetId>,
    access_label: Option<String>,
    max_width_override: Option<f32>,
    max_height_override: Option<f32>,
    dwell_step: Signal<u32>,
    sticky: Signal<bool>,
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
            .finish()
    }
}

impl CompositeTooltipWidget {
    pub fn new() -> Self {
        Self {
            body: None,
            body_id: None,
            root_child_id: None,
            access_label: None,
            max_width_override: None,
            max_height_override: None,
            dwell_step: Signal::new(0),
            sticky: Signal::new(false),
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

    #[doc(hidden)]
    pub fn access_label_literal(mut self, label: impl Into<String>) -> Self {
        self.access_label = Some(label.into());
        self
    }

    /// Override the per-theme `composite_tooltip.max_width`.
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

impl Widget for CompositeTooltipWidget {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme_signal().get();
        let style = theme.components.composite_tooltip;
        let self_id = ctx.self_id();

        // Body — taken out of self once and laid into the arena. If
        // unset, fall back to a Spacer so layout remains well-formed.
        let body_id = if let Some(body) = self.body.take() {
            ctx.add_boxed(body)
        } else {
            ctx.add(Spacer::new())
        };
        self.body_id = Some(body_id);

        // Always wrap in a vertical-only ScrollArea; chrome stays
        // invisible until overflow (AsNeeded) and the user can scroll
        // long content with the wheel either way.
        let scrolled = ctx.add(
            ScrollArea::from_id(body_id)
                .vertical_scroll_bar_policy(ScrollBarPolicy::AsNeeded)
                .horizontal_scroll_bar_policy(ScrollBarPolicy::AlwaysOff),
        );

        let padded = ctx.add(
            Padding::symmetric(style.padding_vertical, style.padding_horizontal).child_id(scrolled),
        );

        let indicator = ctx.add(DwellIndicator::new(
            self.dwell_step.clone(),
            self.sticky.clone(),
            theme.colors.tooltip_text,
        ));

        // Footer: 1fr Spacer + Auto indicator — keeps the dwell pin
        // visually anchored at the bottom-right of the surface, the
        // same convention rich tooltips use at the top-right of their
        // inner Grid.
        let footer_spacer = ctx.add(Spacer::new());
        let footer = ctx.add(
            Grid::new()
                .columns(vec![TrackSize::Fractional(1.0), TrackSize::Auto])
                .rows(vec![TrackSize::Auto])
                .column_gap(8.0)
                .add_child(footer_spacer)
                .add_child(indicator),
        );

        let root = ctx.add(
            VStack::new()
                .spacing(6.0)
                .add_child(padded)
                .add_child(footer),
        );
        self.root_child_id = Some(root);

        // Focusable so Tab can enter the surface once promoted.
        let handlers = HandlerSet::new().focusable(true);
        ctx.apply_self_handlers(handlers);

        // Bind sticky for the role flip in accessibility().
        self.sticky.bind_to(
            self_id,
            ctx.binding_registry(),
            fern_core::binding::BindingLevel::AccessibilityOnly,
        );

        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> fern_core::widget::LayoutResponse {
        let max_w = self
            .max_width_override
            .unwrap_or(ctx.theme.components.composite_tooltip.max_width);
        let max_h = self
            .max_height_override
            .unwrap_or(ctx.theme.components.composite_tooltip.max_height);
        let clamped = SizeProposal {
            width: Some(proposal.width.map(|w| w.min(max_w)).unwrap_or(max_w)),
            height: Some(proposal.height.map(|h| h.min(max_h)).unwrap_or(max_h)),
        };
        self.root_child_id
            .and_then(|id| ctx.child_size(id, clamped))
            .unwrap_or_else(|| Size::new(0.0, 0.0))
            .into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let style = ctx.theme.components.composite_tooltip;
        let radius = CornerRadius::uniform(style.corner_radius);
        super::paint_composite_tooltip_shadows(canvas, bounds, radius, ctx);
        canvas.fill_rounded_rect(bounds, radius, ctx.theme.colors.tooltip_bg);
        // paint() is the visibility hook — only invoked while the
        // tooltip is active. Drives the dwell-to-sticky timer.
        self.tick_dwell();
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        let is_sticky = self.sticky.get();
        let role = if is_sticky {
            fern_core::accesskit::Role::Dialog
        } else {
            fern_core::accesskit::Role::Tooltip
        };
        builder.set_role(role);
        if let Some(label) = self.access_label.as_deref() {
            builder.set_name(label);
        }
        if is_sticky {
            builder.add_action(fern_core::accesskit::Action::Focus);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.map(|id| vec![id]).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::button::Button;
    use crate::primitives::{TextWidget, VStack};
    use crate::tooltip::attach::{DEFAULT_COMPOSITE_TOOLTIP_DELAY, attach_composite_tooltip};
    use fern_canvas::{MockTextBackend, SizeProposal};
    use fern_core::widget_tree::WidgetTree;
    use std::cell::RefCell;
    use std::rc::Rc;

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
    }

    impl ComposeTooltipHost {
        fn new(tooltip_id_sink: Rc<Cell<Option<WidgetId>>>) -> Self {
            Self {
                anchor_id: None,
                tooltip_id_sink,
            }
        }
    }

    impl Widget for ComposeTooltipHost {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let anchor = ctx.add(Button::new_literal("Hover me"));
            self.anchor_id = Some(anchor);
            let body = VStack::new()
                .child(TextWidget::new_literal("Header"))
                .child(TextWidget::new_literal("Body"));
            let tip = attach_composite_tooltip(ctx, anchor, body, DEFAULT_COMPOSITE_TOOLTIP_DELAY);
            self.tooltip_id_sink.set(Some(tip));
            vec![anchor]
        }
        fn layout_response(
            &self,
            proposal: SizeProposal,
            ctx: &LayoutContext,
        ) -> fern_core::widget::LayoutResponse {
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

        tree.advance_time(DEFAULT_COMPOSITE_TOOLTIP_DELAY + Duration::from_millis(50));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "composite tooltip should have appeared after the hover delay"
        );
    }

    #[test]
    fn composite_tooltip_dismisses_on_pointer_leave_before_promotion() {
        let mut tree = tree_with_backend();
        let tooltip_id_sink = Rc::new(Cell::new(None));
        let host = tree.add(ComposeTooltipHost::new(tooltip_id_sink.clone()));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.pointer_move(tree.bounds(host).center());
        tree.advance_time(DEFAULT_COMPOSITE_TOOLTIP_DELAY + Duration::from_millis(50));
        assert_eq!(tree.active_overlays().len(), 1);

        // Pointer leaves before sticky promotion → dismiss.
        tree.pointer_move(fern_canvas::Point::new(2000.0, 2000.0));
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
        tree.advance_time(DEFAULT_COMPOSITE_TOOLTIP_DELAY + Duration::from_millis(50));
        assert_eq!(tree.active_overlays().len(), 1);

        let content_id = tooltip_id_sink
            .get()
            .expect("tooltip id captured during build");
        tree.promote_tooltip_to_sticky(content_id);

        tree.pointer_move(fern_canvas::Point::new(2000.0, 2000.0));
        tree.advance_time(Duration::from_millis(500));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "sticky composite tooltip should survive pointer-leave"
        );
    }
}
