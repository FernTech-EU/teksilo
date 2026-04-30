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
//! in `fern_core::widget_tree::overlay_impl::attach_tooltip`, which
//! takes any `content_id` and doesn't care whether it wraps plain text
//! or rich content. Rich tooltips drop into the existing hover plumbing
//! without a separate attachment path.

use std::time::Duration;

use fern_core::build_context::BuildContext;
use fern_core::widget_id::WidgetId;

use crate::tooltip::registry::TooltipContent;
use crate::tooltip::rich::{RichTooltipWidget, DWELL_PROMOTION};

/// Default hover-to-show delay for rich tooltips — matches the plain
/// tooltip delay used by Button, Link, and MenuItem today.
pub const DEFAULT_RICH_TOOLTIP_DELAY: Duration = Duration::from_millis(200);

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
/// attach_rich_tooltip(ctx, root, "save-as-details", DEFAULT_RICH_TOOLTIP_DELAY);
/// ```
pub fn attach_rich_tooltip(
    ctx: &mut BuildContext,
    anchor_id: WidgetId,
    key: impl Into<String>,
    delay: Duration,
) -> WidgetId {
    let tooltip = RichTooltipWidget::from_key(key);
    // Grab the sink BEFORE handing the widget to the arena — after
    // `ctx.add(tooltip)` we can't borrow the widget back. The sink is
    // an Rc<Cell<..>> that the tree updates on show / dismiss and the
    // widget reads from `paint()` to drive its dwell indicator.
    let sink = tooltip.shown_at_sink();
    let tooltip_id = ctx.add(tooltip);
    ctx.attach_tooltip_with_sticky_sink(
        anchor_id,
        tooltip_id,
        delay,
        Some(DWELL_PROMOTION),
        sink,
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
    let tooltip = RichTooltipWidget::new(content);
    let sink = tooltip.shown_at_sink();
    let tooltip_id = ctx.add(tooltip);
    ctx.attach_tooltip_with_sticky_sink(
        anchor_id,
        tooltip_id,
        delay,
        Some(DWELL_PROMOTION),
        sink,
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
    match source {
        RichTooltipSource::Key(k) => attach_rich_tooltip(ctx, anchor_id, k, delay),
        RichTooltipSource::Content(c) => {
            attach_rich_tooltip_content(ctx, anchor_id, c, delay)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::button::Button;
    use crate::tooltip::registry::{
        _reset_tooltip_registry, install_tooltip_registry, TooltipContent,
    };
    use fern_canvas::{MockTextBackend, SizeProposal};
    use fern_core::widget_tree::WidgetTree;
    use fern_i18n::LocalizedString;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn tree_with_backend() -> WidgetTree {
        WidgetTree::new().with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())))
    }

    #[test]
    fn button_rich_tooltip_appears_after_hover_delay() {
        _reset_tooltip_registry();
        install_tooltip_registry(vec![TooltipContent::new(
            "save-as",
            LocalizedString::literal("Save the current file under a new name"),
        )]);

        let mut tree = tree_with_backend();
        let btn =
            tree.add(Button::new_literal("Save As").rich_tooltip("save-as"));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        // No tooltip visible before hover.
        assert!(tree.active_overlays().is_empty());

        tree.pointer_move(tree.bounds(btn).center());
        assert!(
            tree.active_overlays().is_empty(),
            "tooltip should not appear instantly — waits for delay"
        );

        tree.advance_time(DEFAULT_RICH_TOOLTIP_DELAY + Duration::from_millis(50));

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
        install_tooltip_registry(vec![TooltipContent::new(
            "help",
            LocalizedString::literal("Help body"),
        )]);

        let mut tree = tree_with_backend();
        // Plain set first, then rich: rich should win (latest setter
        // clears the other field).
        let btn = tree.add(
            Button::new_literal("Help")
                .tooltip_literal("stale plain text")
                .rich_tooltip("help"),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));
        tree.pointer_move(tree.bounds(btn).center());
        tree.advance_time(DEFAULT_RICH_TOOLTIP_DELAY + Duration::from_millis(50));

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
    fn rich_tooltip_auto_promotes_on_keyboard_focus() {
        _reset_tooltip_registry();
        install_tooltip_registry(vec![TooltipContent::new(
            "focus-key",
            LocalizedString::literal("Focus-shown body"),
        )]);

        let mut tree = tree_with_backend();
        let btn = tree.add(
            Button::new_literal("Focus me").rich_tooltip("focus-key"),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));

        assert!(tree.active_overlays().is_empty());

        // Simulate keyboard focus of the button — no hover, no delay.
        tree.focus(btn);

        assert_eq!(
            tree.active_overlays().len(),
            1,
            "rich tooltip should appear immediately when its anchor is keyboard-focused"
        );

        _reset_tooltip_registry();
    }

    #[test]
    fn focus_promoted_tooltip_dismisses_when_focus_leaves_scope() {
        _reset_tooltip_registry();
        install_tooltip_registry(vec![TooltipContent::new(
            "leave-key",
            LocalizedString::literal("Goes away"),
        )]);

        let mut tree = tree_with_backend();
        let btn = tree.add(
            Button::new_literal("Anchor").rich_tooltip("leave-key"),
        );
        let other = tree.add(Button::new_literal("Elsewhere"));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        tree.focus(btn);
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
    fn inline_content_tooltip_attaches_without_registry_key() {
        _reset_tooltip_registry();
        // No install_tooltip_registry — we rely on inline content.
        let mut tree = tree_with_backend();
        let content = TooltipContent::new(
            "inline-only",
            LocalizedString::literal("Inline content"),
        );
        let btn = tree.add(Button::new_literal("Go").rich_tooltip_content(content));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        tree.pointer_move(tree.bounds(btn).center());
        tree.advance_time(DEFAULT_RICH_TOOLTIP_DELAY + Duration::from_millis(50));

        assert_eq!(tree.active_overlays().len(), 1);

        _reset_tooltip_registry();
    }
}
