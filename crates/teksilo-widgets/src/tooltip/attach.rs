// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Rich-tooltip attachment helpers.
//!
//! Widgets call [`attach_rich_tooltip`] (or [`attach_rich_tooltip_content`])
//! from their `build()` to wire a hover-triggered [`RichTooltipWidget`]
//! onto an anchor. The helper:
//!
//! 1. Creates a dormant `RichTooltipWidget` as a child of the current
//!    build context,
//! 2. Registers it with the widget tree's tooltip-attachment table
//!    via [`BuildContext::attach_tooltip`], so hover enter/leave +
//!    delay timing + overlay show/hide are handled by the same
//!    machinery the plain `TooltipWidget` already uses.
//!
//! This is a thin convenience layer — the full attach lifecycle lives
//! in `teksilo_core::widget_tree::overlay_impl::attach_tooltip`, which
//! takes any `content_id` and doesn't care whether it wraps plain text
//! or rich content. Rich tooltips drop into the existing hover plumbing
//! without a separate attachment path.

use std::time::Duration;

use teksilo_core::build_context::BuildContext;
use teksilo_core::overlay::TooltipPlacement;
use teksilo_core::widget::Widget;
use teksilo_core::widget_id::WidgetId;

use crate::tooltip::composite::CompositeTooltipWidget;
use crate::tooltip::registry::TooltipContent;
use crate::tooltip::rich::{DWELL_PROMOTION, RichTooltipWidget};

/// Source resolution for a rich tooltip — either a registry key (the
/// common path) or an inline [`TooltipContent`] entry (one-offs that
/// don't belong in the app-wide registry).
#[derive(Debug, Clone)]
pub enum RichTooltipSource {
    /// Resolve against the thread-local
    /// [`TooltipRegistry`](crate::tooltip::registry::TooltipRegistry)
    /// at build time using the given key.
    Key(String),
    /// Render the given content directly.
    Content(TooltipContent),
}

impl<T: Into<String>> From<T> for RichTooltipSource {
    fn from(value: T) -> Self {
        RichTooltipSource::Key(value.into())
    }
}

/// Attach a rich tooltip to `anchor_id`. Creates a `RichTooltipWidget`
/// resolving `key` from the registry and wires it into the existing
/// tooltip-hover machinery.
///
/// Typical use inside a widget's `build()`:
///
/// ```ignore
/// let root = ctx.add(/* visible subtree */);
/// let delay = ctx.theme().motion.tooltip_delay;
/// attach_rich_tooltip(ctx, root, "save-as-details", delay);
/// ```
pub fn attach_rich_tooltip(
    ctx: &mut BuildContext,
    anchor_id: WidgetId,
    key: impl Into<String>,
    delay: Duration,
) -> WidgetId {
    attach_rich_tooltip_with_placement(ctx, anchor_id, key, delay, TooltipPlacement::Below)
}

/// [`attach_rich_tooltip`] with an explicit [`TooltipPlacement`] — pass
/// `Side` for anchors stacked vertically (menu items, a vertical tab
/// strip, list/tree rows) so the tooltip opens beside the anchor.
pub fn attach_rich_tooltip_with_placement(
    ctx: &mut BuildContext,
    anchor_id: WidgetId,
    key: impl Into<String>,
    delay: Duration,
    placement: TooltipPlacement,
) -> WidgetId {
    let tooltip = RichTooltipWidget::from_key(key);
    // Grab the sink BEFORE handing the widget to the arena — after
    // `ctx.add(tooltip)` we can't borrow the widget back. The sink is
    // an Rc<Cell<..>> that the tree updates on show / dismiss and the
    // widget reads from `paint()` to drive its dwell indicator.
    let sink = tooltip.shown_at_sink();
    let tooltip_id = ctx.add(tooltip);
    ctx.attach_tooltip_with_sticky_sink_placement(
        anchor_id,
        tooltip_id,
        delay,
        Some(DWELL_PROMOTION),
        sink,
        placement,
    );
    tooltip_id
}

/// Attach a rich tooltip driven by an inline [`TooltipContent`] entry.
/// Use this for one-off tooltips that don't live in the central
/// registry (tests, dynamic content, per-row tips on data-driven
/// widgets).
pub fn attach_rich_tooltip_content(
    ctx: &mut BuildContext,
    anchor_id: WidgetId,
    content: TooltipContent,
    delay: Duration,
) -> WidgetId {
    attach_rich_tooltip_content_with_placement(
        ctx,
        anchor_id,
        content,
        delay,
        TooltipPlacement::Below,
    )
}

/// [`attach_rich_tooltip_content`] with an explicit [`TooltipPlacement`].
pub fn attach_rich_tooltip_content_with_placement(
    ctx: &mut BuildContext,
    anchor_id: WidgetId,
    content: TooltipContent,
    delay: Duration,
    placement: TooltipPlacement,
) -> WidgetId {
    let tooltip = RichTooltipWidget::new(content);
    let sink = tooltip.shown_at_sink();
    let tooltip_id = ctx.add(tooltip);
    ctx.attach_tooltip_with_sticky_sink_placement(
        anchor_id,
        tooltip_id,
        delay,
        Some(DWELL_PROMOTION),
        sink,
        placement,
    );
    tooltip_id
}

/// Attach a rich tooltip from a [`RichTooltipSource`]. Matches whether
/// the source is a registry key or inline content and forwards to the
/// appropriate helper. Convenient for builder methods that accept
/// `impl Into<RichTooltipSource>` so callers can pass either a bare
/// `&str` (resolved as a key) or a fully-built `TooltipContent`.
pub fn attach_rich_tooltip_source(
    ctx: &mut BuildContext,
    anchor_id: WidgetId,
    source: RichTooltipSource,
    delay: Duration,
) -> WidgetId {
    attach_rich_tooltip_source_with_placement(
        ctx,
        anchor_id,
        source,
        delay,
        TooltipPlacement::Below,
    )
}

/// [`attach_rich_tooltip_source`] with an explicit [`TooltipPlacement`] —
/// the placement-aware entry point used by widgets that live in a vertical
/// list (menu items, list/tree rows, activity-rail items) and want `Side`.
pub fn attach_rich_tooltip_source_with_placement(
    ctx: &mut BuildContext,
    anchor_id: WidgetId,
    source: RichTooltipSource,
    delay: Duration,
    placement: TooltipPlacement,
) -> WidgetId {
    match source {
        RichTooltipSource::Key(k) => {
            attach_rich_tooltip_with_placement(ctx, anchor_id, k, delay, placement)
        }
        RichTooltipSource::Content(c) => {
            attach_rich_tooltip_content_with_placement(ctx, anchor_id, c, delay, placement)
        }
    }
}

/// Attach a composite tooltip — third tier, hosting an arbitrary
/// `impl Widget + 'static` body. Wires the same dwell-to-sticky
/// machinery rich tooltips use, so the surface promotes to a
/// `Role::Dialog` after the user dwells for `DWELL_PROMOTION`.
pub fn attach_composite_tooltip(
    ctx: &mut BuildContext,
    anchor_id: WidgetId,
    content: impl Widget + 'static,
    delay: Duration,
) -> WidgetId {
    attach_composite_tooltip_boxed(ctx, anchor_id, Box::new(content), delay)
}

/// Variant of [`attach_composite_tooltip`] that takes an already-boxed
/// body. Used by per-widget `.composite_tooltip(...)` setters that
/// store `Box<dyn Widget>` so the user-supplied content can survive
/// across the borrow boundary into `build()`.
pub fn attach_composite_tooltip_boxed(
    ctx: &mut BuildContext,
    anchor_id: WidgetId,
    content: Box<dyn Widget>,
    delay: Duration,
) -> WidgetId {
    attach_composite_tooltip_boxed_with_placement(
        ctx,
        anchor_id,
        content,
        delay,
        TooltipPlacement::Below,
    )
}

/// [`attach_composite_tooltip_boxed`] with an explicit [`TooltipPlacement`].
/// Attach an already-built [`CompositeTooltipWidget`], honouring its own
/// [`sticky`](CompositeTooltipWidget::sticky) setting.
///
/// The general primitive the other composite helpers lower to. Reach for it
/// when the body is read-only and should not offer dwell promotion, or when
/// the surface needs an accessible label — both of which are settings on the
/// widget, and neither of which a helper taking a bare `Box<dyn Widget>` can
/// express.
pub fn attach_composite_tooltip_widget_with_placement(
    ctx: &mut BuildContext,
    anchor_id: WidgetId,
    tooltip: CompositeTooltipWidget,
    delay: Duration,
    placement: TooltipPlacement,
) -> WidgetId {
    // A surface with no promotion registers no dwell window. That is what makes
    // it behave like a plain tooltip: pointer-leave retires it, focus does not
    // surface it, and it never becomes a `Dialog`.
    let sticky_after = tooltip.sticky_enabled().then_some(DWELL_PROMOTION);
    let sink = tooltip.shown_at_sink();
    let tooltip_id = ctx.add(tooltip);
    ctx.attach_tooltip_with_sticky_sink_placement(
        anchor_id,
        tooltip_id,
        delay,
        sticky_after,
        sink,
        placement,
    );
    tooltip_id
}

pub fn attach_composite_tooltip_boxed_with_placement(
    ctx: &mut BuildContext,
    anchor_id: WidgetId,
    content: Box<dyn Widget>,
    delay: Duration,
    placement: TooltipPlacement,
) -> WidgetId {
    attach_composite_tooltip_widget_with_placement(
        ctx,
        anchor_id,
        CompositeTooltipWidget::new().content_boxed(content),
        delay,
        placement,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::button::Button;
    use crate::menu_item::MenuItem;
    use crate::menu_list::MenuList;
    use crate::primitives::VStack;
    use crate::tooltip::TooltipWidget;
    use crate::tooltip::registry::{
        _reset_tooltip_registry, TooltipContent, install_tooltip_registry,
    };
    use std::cell::RefCell;
    use std::rc::Rc;
    use teksilo_canvas::{MockTextBackend, SizeProposal};
    use teksilo_core::event::{Key, Modifiers};
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;

    fn tree_with_backend() -> WidgetTree {
        WidgetTree::new().with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())))
    }

    #[test]
    fn button_rich_tooltip_appears_after_hover_delay() {
        _reset_tooltip_registry();
        install_tooltip_registry(vec![TooltipContent::new(
            "save-as",
            lit!("Save the current file under a new name"),
        )]);

        let mut tree = tree_with_backend();
        let btn = tree.add(Button::new(lit!("Save As")).rich_tooltip("save-as"));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        // No tooltip visible before hover.
        assert!(tree.active_overlays().is_empty());

        tree.pointer_move(tree.bounds(btn).center());
        assert!(
            tree.active_overlays().is_empty(),
            "tooltip should not appear instantly — waits for delay"
        );

        tree.advance_time(Duration::from_millis(500) + Duration::from_millis(50));

        assert_eq!(
            tree.active_overlays().len(),
            1,
            "rich tooltip should have appeared after the hover delay"
        );

        _reset_tooltip_registry();
    }

    #[test]
    fn button_rich_tooltip_overrides_plain_tooltip() {
        _reset_tooltip_registry();
        install_tooltip_registry(vec![TooltipContent::new("help", lit!("Help body"))]);

        let mut tree = tree_with_backend();
        // Plain set first, then rich: rich should win (latest setter
        // clears the other field).
        let btn = tree.add(
            Button::new(lit!("Help"))
                .tooltip(lit!("stale plain text"))
                .rich_tooltip("help"),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));
        tree.pointer_move(tree.bounds(btn).center());
        tree.advance_time(Duration::from_millis(500) + Duration::from_millis(50));

        assert_eq!(tree.active_overlays().len(), 1);
        // The stale plain text must NOT be reachable — the rich tooltip
        // supplanted it entirely.
        assert!(
            tree.find_by_label("stale plain text").is_none(),
            "plain tooltip text should have been cleared by .rich_tooltip(...)"
        );

        _reset_tooltip_registry();
    }

    #[test]
    fn rich_tooltip_shows_on_keyboard_focus_once_focus_rests() {
        _reset_tooltip_registry();
        install_tooltip_registry(vec![TooltipContent::new(
            "focus-key",
            lit!("Focus-shown body"),
        )]);

        let mut tree = tree_with_backend();
        let btn = tree.add(Button::new(lit!("Focus me")).rich_tooltip("focus-key"));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        assert!(tree.active_overlays().is_empty());

        // Keyboard focus, no hover. Focus arms the same delay the pointer
        // arms — a tip that appeared on arrival strobed across a Tab sweep.
        tree.focus(btn);
        assert!(
            tree.active_overlays().is_empty(),
            "focus arriving arms the delay; it does not show on arrival"
        );
        tree.advance_time(tree.theme().motion.tooltip_delay + Duration::from_millis(50));

        assert_eq!(
            tree.active_overlays().len(),
            1,
            "rich tooltip appears once keyboard focus has rested for the delay"
        );

        _reset_tooltip_registry();
    }

    #[test]
    fn focus_promoted_tooltip_dismisses_when_focus_leaves_scope() {
        _reset_tooltip_registry();
        install_tooltip_registry(vec![TooltipContent::new("leave-key", lit!("Goes away"))]);

        let mut tree = tree_with_backend();
        let btn = tree.add(Button::new(lit!("Anchor")).rich_tooltip("leave-key"));
        let other = tree.add(Button::new(lit!("Elsewhere")));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.focus(btn);
        tree.advance_time(tree.theme().motion.tooltip_delay + Duration::from_millis(50));
        assert_eq!(tree.active_overlays().len(), 1);

        // Moving focus to an unrelated widget dismisses the
        // focus-promoted tooltip (prevents sticky accumulation as the
        // user Tabs through a form).
        tree.focus(other);
        assert!(
            tree.active_overlays().is_empty(),
            "focus-promoted sticky tooltip should dismiss when focus moves outside its scope"
        );

        _reset_tooltip_registry();
    }

    #[test]
    fn button_plain_tooltip_appears_after_hover_delay() {
        let mut tree = tree_with_backend();
        let btn = tree.add(Button::new(lit!("Save")).tooltip(lit!("Save the document")));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        assert!(tree.active_overlays().is_empty());
        tree.pointer_move(tree.bounds(btn).center());
        assert!(
            tree.active_overlays().is_empty(),
            "plain tooltip should not appear instantly — waits for delay"
        );
        // Plain tooltip uses theme `tooltip_delay` (500 ms default).
        tree.advance_time(Duration::from_millis(550));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "plain tooltip should have appeared after the hover delay"
        );
    }

    #[test]
    fn inline_content_tooltip_attaches_without_registry_key() {
        _reset_tooltip_registry();
        // No install_tooltip_registry — we rely on inline content.
        let mut tree = tree_with_backend();
        let content = TooltipContent::new("inline-only", lit!("Inline content"));
        let btn = tree.add(Button::new(lit!("Go")).rich_tooltip_content(content));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        tree.pointer_move(tree.bounds(btn).center());
        tree.advance_time(Duration::from_millis(500) + Duration::from_millis(50));

        assert_eq!(tree.active_overlays().len(), 1);

        _reset_tooltip_registry();
    }

    // ---- Part A: the "wall of tooltips" fix ------------------------------

    #[test]
    fn menu_container_focus_does_not_fan_out_item_tooltips() {
        // The reported bug: opening a context menu focuses the whole
        // `MenuList` panel, which — before the fix — promoted EVERY item's
        // rich tooltip at once (a wall). The container-fan-out guard
        // (`reverse.len() == 1`) suppresses it.
        _reset_tooltip_registry();
        install_tooltip_registry(vec![
            TooltipContent::new("a", lit!("Tip A")),
            TooltipContent::new("b", lit!("Tip B")),
            TooltipContent::new("c", lit!("Tip C")),
        ]);

        let mut tree = tree_with_backend();
        let menu = tree.add(
            MenuList::new()
                .item(MenuItem::new(lit!("A")).rich_tooltip("a"))
                .item(MenuItem::new(lit!("B")).rich_tooltip("b"))
                .item(MenuItem::new(lit!("C")).rich_tooltip("c")),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        tree.focus(menu);
        tree.advance_time(tree.theme().motion.tooltip_delay + Duration::from_millis(50));
        assert!(
            tree.active_overlays().is_empty(),
            "focusing the menu container must not fan out item tooltips (the wall)"
        );

        _reset_tooltip_registry();
    }

    #[test]
    fn self_anchored_focusable_tooltip_shows_exactly_one_overlay() {
        // A widget that anchors its own sticky tooltip to its *own* id
        // (the `TabHeader` / `ColorSwatch` shape) matches BOTH the direct and
        // reverse predicates, because `is_descendant_of` is reflexive. The
        // mutually-exclusive `if / else if` routing must promote it exactly
        // once — two independent filters would double-`show_overlay` and leak
        // an orphaned overlay.
        let mut tree = tree_with_backend();
        let anchor = tree.add(Button::new(lit!("Self")));
        let content = tree.add(TooltipWidget::new(lit!("Tip")));
        tree.attach_tooltip_with_sticky(
            anchor,
            content,
            Duration::from_millis(200),
            Some(Duration::from_secs(2)),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.focus(anchor);
        tree.advance_time(Duration::from_millis(250));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "self-anchored focus shows exactly one overlay (no reflexive dup)"
        );
    }

    #[test]
    fn single_button_rich_tooltip_still_shows_on_focus() {
        // Regression guard for the composing-widget case: `Button` keeps focus
        // on its outer node but anchors the tooltip on an inner root (the sole
        // reverse match). It must still promote on focus after the fix.
        _reset_tooltip_registry();
        install_tooltip_registry(vec![TooltipContent::new("k", lit!("Body"))]);
        let mut tree = tree_with_backend();
        let btn = tree.add(Button::new(lit!("Focus me")).rich_tooltip("k"));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.focus(btn);
        tree.advance_time(tree.theme().motion.tooltip_delay + Duration::from_millis(50));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "a single composing control still auto-shows its rich tooltip on focus"
        );
        _reset_tooltip_registry();
    }

    #[test]
    fn segmented_control_focus_does_not_fan_out_segment_tooltips() {
        // A `SegmentedControl` is a single focus stop owning many segment
        // tooltips (the segments anchor to their own ids, and the control is
        // their focusable ancestor) — the same fan-out shape as a menu. The
        // `reverse.len() == 1` guard protects it for free.
        _reset_tooltip_registry();
        install_tooltip_registry(vec![
            TooltipContent::new("s0", lit!("Seg 0")),
            TooltipContent::new("s1", lit!("Seg 1")),
        ]);
        let mut tree = tree_with_backend();
        let selected = teksilo_core::signal::Signal::new(0usize);
        let sc = tree.add(
            crate::segmented_control::SegmentedControl::new(selected)
                .segment(crate::segmented_control::Segment::new(lit!("A")).rich_tooltip("s0"))
                .segment(crate::segmented_control::Segment::new(lit!("B")).rich_tooltip("s1")),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.focus(sc);
        assert!(
            tree.active_overlays().is_empty(),
            "focusing a SegmentedControl must not fan out its segment tooltips"
        );
        // Ripen the delay too, so this proves the fan-out guard rather than
        // merely that focus no longer shows a tip on arrival.
        tree.advance_time(tree.theme().motion.tooltip_delay + Duration::from_millis(50));
        assert!(
            tree.active_overlays().is_empty(),
            "…and still none once the delay has elapsed"
        );
        _reset_tooltip_registry();
    }

    // ---- Part B: side placement ------------------------------------------

    #[test]
    fn side_placement_opens_to_the_trailing_side() {
        let mut tree = tree_with_backend();
        // Anchor nested at top-leading of a VStack so it stays small (sized to
        // content, not stretched) with room to its trailing side and below.
        let anchor = tree.add(Button::new(lit!("Anchor")));
        let content = tree.add(TooltipWidget::new(lit!("Tip")));
        tree.attach_tooltip_with_placement(
            anchor,
            content,
            Duration::from_millis(200),
            TooltipPlacement::Side,
        );
        let _root = tree.add(VStack::new().add_child(anchor));
        tree.layout(SizeProposal::exact(600.0, 400.0));
        tree.pointer_move(tree.bounds(anchor).center());
        tree.advance_time(Duration::from_millis(250));
        // Re-layout so the overlay positioner runs on the freshly-shown tooltip.
        tree.layout(SizeProposal::exact(600.0, 400.0));

        let a = tree.bounds(anchor);
        let t = tree
            .overlay_manager()
            .bounds_for_content(content)
            .expect("Side tooltip overlay shown");
        assert!(
            t.x >= a.x + a.width,
            "Side tooltip opens to the trailing side: t.x {} >= anchor right {}",
            t.x,
            a.x + a.width
        );
        assert!(
            t.y < a.y + a.height,
            "Side tooltip is aligned to the anchor top, not below it"
        );
    }

    #[test]
    fn below_placement_opens_under_the_anchor() {
        let mut tree = tree_with_backend();
        let anchor = tree.add(Button::new(lit!("Anchor")));
        let content = tree.add(TooltipWidget::new(lit!("Tip")));
        // Default placement is Below.
        tree.attach_tooltip(anchor, content, Duration::from_millis(200));
        let _root = tree.add(VStack::new().add_child(anchor));
        tree.layout(SizeProposal::exact(600.0, 400.0));
        tree.pointer_move(tree.bounds(anchor).center());
        tree.advance_time(Duration::from_millis(250));
        tree.layout(SizeProposal::exact(600.0, 400.0));

        let a = tree.bounds(anchor);
        let t = tree
            .overlay_manager()
            .bounds_for_content(content)
            .expect("Below tooltip overlay shown");
        assert!(
            t.y >= a.y + a.height,
            "Below tooltip opens under the anchor: t.y {} >= anchor bottom {}",
            t.y,
            a.y + a.height
        );
    }

    // ---- Part C: keyboard reachability of menu item tooltips -------------

    #[test]
    fn keyboard_menu_navigation_surfaces_highlighted_item_tooltip() {
        _reset_tooltip_registry();
        install_tooltip_registry(vec![
            TooltipContent::new("a", lit!("Tip A")),
            TooltipContent::new("b", lit!("Tip B")),
        ]);

        let mut tree = tree_with_backend();
        // Third item has NO tooltip — highlighting it must dismiss the prior
        // one and show nothing.
        let menu = tree.add(
            MenuList::new()
                .item(MenuItem::new(lit!("A")).rich_tooltip("a"))
                .item(MenuItem::new(lit!("B")).rich_tooltip("b"))
                .item(MenuItem::new(lit!("C"))),
        );
        tree.layout(SizeProposal::exact(400.0, 300.0));

        // Focus the menu panel (as the open path does): no wall.
        tree.focus(menu);
        assert!(
            tree.active_overlays().is_empty(),
            "no tooltip on menu focus (Part A)"
        );

        // Arrow-key highlight surfaces exactly the highlighted item's tooltip.
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "ArrowDown surfaces the highlighted item's tooltip (Part C)"
        );

        // Moving the highlight dismisses the previous tooltip and shows the
        // next — still exactly one, never a growing stack.
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "moving the highlight replaces the tooltip (still exactly one)"
        );

        // Highlighting a tooltip-less item dismisses the prior tooltip and
        // shows nothing.
        tree.press_key(Key::ArrowDown, Modifiers::NONE);
        assert!(
            tree.active_overlays().is_empty(),
            "highlighting a tooltip-less item clears the previous tooltip"
        );

        _reset_tooltip_registry();
    }

    // ---- dwell indicator: continuous update while the pointer is still -----

    #[test]
    fn dwelling_tooltip_wake_deadline_is_due_at_its_wake() {
        // Regression for "the dwell indicator only updates when the mouse
        // moves": the 500 ms dwell wake deadline must be rounded off the LAST
        // RENDERED FRAME (`last_frame_time`), not off `Instant::now()`.
        //
        // The app's `request_redraw_due` only redraws a window whose
        // `next_timer_deadline() <= now`. A `now`-rounded deadline rolls to the
        // NEXT (future) step the instant its own wake fires, so `<= now` never
        // holds, the window is never redrawn, and the dwell freezes until an
        // unrelated input event nudges the loop. Rounding off `last_frame_time`
        // keeps the deadline `<= now` at its wake — one redraw per boundary.
        //
        // Here: show the tooltip, freeze `last_frame_time` at the show render,
        // then let real time cross the first 500 ms boundary WITHOUT another
        // render (the stationary-pointer case) and assert the deadline is due.
        _reset_tooltip_registry();
        install_tooltip_registry(vec![TooltipContent::new("k", lit!("Body"))]);
        let mut tree = tree_with_backend();
        // Reduced motion removes the fade animation, so `next_timer_deadline`
        // below reflects ONLY the dwell wake — not a fade deadline that would
        // pass the assert regardless of the dwell fix.
        tree.set_accessibility_preferences(false, true, 1.0);
        let btn = tree.add(Button::new(lit!("Hover")).rich_tooltip("k"));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        tree.pointer_move(tree.bounds(btn).center());
        tree.advance_time(Duration::from_millis(550)); // past the hover delay → shown
        // A render pins `last_frame_time` at ~= the show instant.
        tree.layout(SizeProposal::exact(400.0, 200.0));
        assert_eq!(tree.active_overlays().len(), 1, "rich tooltip shown");

        // Real time crosses the first 500 ms step boundary with NO further
        // render (last_frame_time stays frozen) — exactly what a still pointer
        // gives the event loop.
        std::thread::sleep(Duration::from_millis(600));

        let deadline = tree
            .next_timer_deadline()
            .expect("a dwelling tooltip must schedule a wake deadline");
        assert!(
            deadline <= std::time::Instant::now(),
            "the dwell wake deadline must be DUE at its own wake (pinned to \
             last_frame_time); a still-future deadline is the freeze bug"
        );

        _reset_tooltip_registry();
    }

    #[test]
    fn plain_tooltip_schedules_no_dwell_wake() {
        // A plain (non-sticky) tooltip has no dwell timer, so once shown it must
        // NOT keep scheduling wake deadlines — the dwell wake is scoped to
        // rich/composite tooltips only. (`next_timer_deadline` may still be
        // Some for other reasons, but not from a dwell; here nothing else is
        // active, so it must be None once the tooltip is shown and settled.)
        let mut tree = tree_with_backend();
        tree.set_accessibility_preferences(false, true, 1.0); // reduced motion → no fade deadline
        let btn = tree.add(Button::new(lit!("Hover")).tooltip(lit!("Plain")));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        tree.pointer_move(tree.bounds(btn).center());
        tree.advance_time(Duration::from_millis(550));
        assert_eq!(tree.active_overlays().len(), 1, "plain tooltip shown");
        tree.layout(SizeProposal::exact(400.0, 200.0));

        assert!(
            tree.next_timer_deadline().is_none(),
            "a plain tooltip must not schedule a dwell wake deadline"
        );
    }
}
