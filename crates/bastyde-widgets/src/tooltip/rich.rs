// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `RichTooltipWidget` — rich tooltip content surface.
//!
//! Renders a [`TooltipContent`] entry (body + optional shortcut hint +
//! optional "more" disclosure) inside the existing tooltip rounded-rect
//! chrome. Body and long-form text go through `TextWidget` with inline
//! markup enabled, so `[label](url)` / `*italic*` / `**bold**` render
//! correctly and inline links participate in the nested-tooltip
//! cascade.
//!
//! This widget is the **content** of a tooltip overlay — the anchor /
//! hover trigger / overlay lifetime live in the surrounding attach
//! API. A caller (the owning widget's build) wraps a `RichTooltipWidget`
//! into an `OverlayRequest` or attaches it via the simple tooltip
//! attach API once that integration lands.
//!
//! Sticky-on-dwell:
//! - At t=0 the tooltip is shown by the normal hover path.
//! - The widget tracks visible-paint time via `paint()` interior
//!   mutability and, every 500 ms, advances a `Signal<u32>` step
//!   counter from 0 to 4.
//! - The top-right `DwellIndicator` reads the step signal and
//!   paints an empty circle filling progressively in 4 wedges.
//! - At step 4 the indicator flips to a pin icon and the widget's
//!   `sticky` signal goes true. The widget tree (via
//!   `attach_tooltip_with_sticky`) auto-promotes the overlay on
//!   the same 2 s timer: removes the entry from the hover tracker
//!   and swaps the dismiss behavior to `EscapeOrClickOutside`. The
//!   widget's a11y role flips from `Tooltip` to `Dialog` and a
//!   `Focus` action is advertised on the node. Promotion does **not**
//!   move keyboard focus into the panel — the user Tabs in. This is
//!   the correct pattern for a non-modal sticky panel (it never steals
//!   focus from whatever the user was doing).
//! - Cascade children (tooltips opened from a `[label](:key)` link via
//!   `RichTooltipWidget::cascade_child`) **omit the indicator
//!   entirely**: they're shown by an explicit click and are already
//!   persistent, so there's no hover dwell-to-sticky path to visualize.
//!   They also skip straight to the persistent a11y treatment — a
//!   non-modal `Dialog` advertising `Focus`, same as a dwell-promoted
//!   tooltip — rather than reading as an ephemeral `Tooltip`.

use bastyde_i18n::lit;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use bastyde_canvas::{Canvas, Rect, Size, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, PaintContext, Widget};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_tokens::{CornerRadius, TextRole, TextStyleRole};

use crate::accordion::Accordion;
use crate::keystroke_format::format_keystroke;
use crate::primitives::{Grid, Padding, Spacer, TextWidget, TrackSize, VStack};
use crate::tooltip::dwell_indicator::DwellIndicator;
use crate::tooltip::registry::{TooltipContent, TooltipRegistry, with_tooltip_registry};

/// Total dwell time before the tooltip promotes to sticky.
///
/// **The single source of truth for the dwell clock.** `composite.rs`, the
/// tree's dwell wake-up scheduler (`WidgetTree::next_timer_deadline`) and the
/// `DwellIndicator`'s wedge geometry all derive from these three constants
/// rather than restating them; they used to be four unlinked copies, so a
/// change here silently desynced the visible indicator from the promotion it
/// was supposed to be counting down.
pub(crate) const DWELL_PROMOTION: Duration = Duration::from_secs(2);
/// Maximum step value (4 = full circle = pin icon).
///
/// Pinned to the tree's `TOOLTIP_DWELL_STEPS`, which is what decides how often
/// the event loop wakes during a dwell: if the indicator drew more steps than
/// the tree scheduled wake-ups for, the wedge would skip; fewer, and it would
/// repaint identically on wake-ups that changed nothing.
pub(crate) const DWELL_STEPS: u32 = bastyde_core::widget_tree::TOOLTIP_DWELL_STEPS;
/// Per-step dwell duration: total / steps = 500 ms.
pub(crate) const DWELL_STEP_DURATION: Duration =
    Duration::from_millis((DWELL_PROMOTION.as_millis() / DWELL_STEPS as u128) as u64);

// The step arithmetic above truncates. Unless the promotion window divides
// exactly into its steps, the indicator's last wedge and the actual promotion
// drift apart — four 490 ms steps would fill the circle 40 ms early and sit
// full while nothing happened.
const _: () = assert!(
    DWELL_PROMOTION
        .as_millis()
        .is_multiple_of(DWELL_STEPS as u128),
    "DWELL_PROMOTION must divide exactly into DWELL_STEPS"
);
const _: () = assert!(
    DWELL_STEP_DURATION.as_millis() * DWELL_STEPS as u128 == DWELL_PROMOTION.as_millis(),
    "dwell steps must sum to exactly DWELL_PROMOTION"
);

/// Rich tooltip content widget.
///
/// Internally composes a rounded rect surface with a VStack of
/// `TextWidget`s for body text / shortcut / optional "more" accordion
/// disclosure.
pub struct RichTooltipWidget {
    content: Option<TooltipContent>,
    /// Pending key to resolve against the registry at build time.
    /// Used when constructed via `from_key` — we defer resolution so
    /// that the registry install order doesn't matter.
    pending_key: Option<String>,
    root_child_id: Option<WidgetId>,
    // ── Dwell state machine ──
    /// Dwell step in 0..=4. 0 = empty circle, 4 = pin icon.
    /// Updated from `paint()` based on elapsed visible time.
    dwell_step: Signal<u32>,
    /// True after the dwell timer has reached `DWELL_PROMOTION`.
    /// Drives the indicator pin variant and the a11y role flip.
    sticky: Signal<bool>,
    /// Shared with the widget tree's tooltip entry. The tree writes
    /// `Some(now)` when the tooltip is shown and `None` when it is
    /// dismissed. The widget reads it from `paint()` to compute the
    /// authoritative elapsed dwell time — no paint-gap heuristic.
    shown_at_sink: Rc<Cell<Option<Instant>>>,
    /// True when this tooltip was opened as a *child* of another tooltip
    /// via a `[label](:key)` cascade link. Cascade children are shown by
    /// an explicit click and are already persistent
    /// (`EscapeOrClickOutside`), so the hover dwell-to-sticky affordance
    /// — and its [`DwellIndicator`] — don't apply: the indicator is
    /// suppressed in `build()`.
    is_cascade_child: bool,
    /// The keys of every tooltip strictly above this one on the current
    /// cascade path (root → … → this tooltip's parent). Threaded down so
    /// `build()` can refuse to pre-create a nested child whose key is
    /// already an ancestor — that would close a `[label](:key)` cycle
    /// (e.g. `book → chapter → end-of-book → book`) and, because
    /// pre-creation is eager and recursive, overflow the stack. Empty on
    /// the root tooltip; each cascade child receives its parent's path
    /// plus the parent's own key. See [`MAX_CASCADE_DEPTH`].
    cascade_ancestors: Vec<String>,
}

/// Hard cap on cascade nesting depth. The `cascade_ancestors` visited-set
/// already guarantees termination (no key repeats on a path, so depth is
/// bounded by the registry size), but a large, densely cross-linked
/// registry could still enumerate very long simple paths. This caps the
/// eager pre-creation regardless of graph shape; a link deeper than this
/// still renders as text but opens nothing. Realistic drill-down is one or
/// two levels, so the ceiling is generous.
const MAX_CASCADE_DEPTH: usize = 8;

impl std::fmt::Debug for RichTooltipWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RichTooltipWidget")
            .field("has_content", &self.content.is_some())
            .field("pending_key", &self.pending_key)
            .finish()
    }
}

impl RichTooltipWidget {
    /// Construct a rich tooltip that renders an explicit
    /// [`TooltipContent`] entry. Use this for one-off tooltips that
    /// aren't registered in the central registry.
    pub fn new(content: TooltipContent) -> Self {
        Self {
            content: Some(content),
            pending_key: None,
            root_child_id: None,
            dwell_step: Signal::new(0),
            sticky: Signal::new(false),
            shown_at_sink: Rc::new(Cell::new(None)),
            is_cascade_child: false,
            cascade_ancestors: Vec::new(),
        }
    }

    /// Construct a rich tooltip that resolves its content from the
    /// thread-local [`TooltipRegistry`] at build time using the given
    /// key. This is the common path — applications register their
    /// full tooltip catalog once at boot, then refer to entries by
    /// key from hover-trigger sites.
    pub fn from_key(key: impl Into<String>) -> Self {
        Self {
            content: None,
            pending_key: Some(key.into()),
            root_child_id: None,
            dwell_step: Signal::new(0),
            sticky: Signal::new(false),
            shown_at_sink: Rc::new(Cell::new(None)),
            is_cascade_child: false,
            cascade_ancestors: Vec::new(),
        }
    }

    /// Mark this tooltip as a cascade child — one opened from another
    /// tooltip's `[label](:key)` link rather than by hover. Suppresses
    /// the dwell-to-sticky [`DwellIndicator`], which is meaningless on a
    /// tooltip that's already persistent. Internal to the cascade
    /// mechanism (`RichTooltipWidget::build` pre-creates these).
    pub(crate) fn cascade_child(mut self) -> Self {
        self.is_cascade_child = true;
        self
    }

    /// Record the cascade path (keys of every ancestor tooltip) this
    /// tooltip hangs from, so `build()` can break `[label](:key)` cycles
    /// and honour [`MAX_CASCADE_DEPTH`]. Internal to the cascade
    /// mechanism — set by `build()` when it pre-creates a nested child.
    pub(crate) fn with_cascade_ancestors(mut self, ancestors: Vec<String>) -> Self {
        self.cascade_ancestors = ancestors;
        self
    }

    /// Clone of the tooltip's `shown_at` sink — used by the attach
    /// helper to thread the same `Rc<Cell<..>>` through to the
    /// widget tree's `attach_tooltip_with_sticky_sink`.
    pub fn shown_at_sink(&self) -> Rc<Cell<Option<Instant>>> {
        self.shown_at_sink.clone()
    }

    /// Recompute dwell step + sticky from the authoritative
    /// `shown_at_sink` value. Called from `paint()` so the indicator
    /// progresses on every frame the tooltip is visible.
    fn tick_dwell(&self) {
        let Some(shown_at) = self.shown_at_sink.get() else {
            // Tooltip is not currently shown according to the tree.
            // Reset the visible state so a future show starts at 0.
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

impl Widget for RichTooltipWidget {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Resolve the content: either the inline entry or a registry
        // lookup via the pending key.
        if self.content.is_none()
            && let Some(key) = self.pending_key.as_deref()
        {
            self.content = with_tooltip_registry(|reg| reg.get(key).cloned()).flatten();
        }

        let Some(content) = self.content.clone() else {
            // No content resolved (unknown key or missing registry) —
            // fall back to an empty Spacer so layout doesn't crash.
            let id = ctx.add(Spacer::new());
            self.root_child_id = Some(id);
            return vec![id];
        };

        // Snapshot the theme once for static text styles; reactive colors
        // are piped into leaf widgets via `theme_signal.map(...)` below.
        // Rich tooltips are short-lived overlays, so a theme switch that
        // happens while one is already visible refreshes the next time
        // it's re-shown (or immediately for the signal-bound colors).
        let theme_signal = ctx.theme_signal();
        let theme = theme_signal.get();
        use crate::styles::recipe_tooltip_style as tt;
        let self_id = ctx.self_id();

        // Pre-create dormant RichTooltipWidgets for every :key URL
        // referenced by the body text or the "more" body. The
        // resulting `HashMap<key → WidgetId>` is shared with the link
        // handler closures so a link click / hover can open the
        // matching child overlay via `show_overlay`. Pre-creating
        // (rather than creating at event time) matches the established
        // menu-submenu pattern in menu_item.rs.
        let mut nested_ids: HashMap<String, WidgetId> = HashMap::new();
        let body_source = content.text.resolve_now();
        let more_source = content.more.as_ref().map(|m| m.resolve_now());
        let mut nested_keys: Vec<String> = Vec::new();
        scan_tooltip_key_urls(&body_source, &mut nested_keys);
        if let Some(ref m) = more_source {
            scan_tooltip_key_urls(m, &mut nested_keys);
        }
        nested_keys.sort();
        nested_keys.dedup();
        // The cascade path handed to any child: this tooltip's ancestors
        // plus its own key. Guards the eager, recursive pre-creation below
        // against `[label](:key)` cycles (e.g. `book → chapter →
        // end-of-book → book`), which would otherwise recurse forever and
        // overflow the stack.
        let mut child_ancestors = self.cascade_ancestors.clone();
        child_ancestors.push(content.key.clone());
        let at_depth_limit = child_ancestors.len() >= MAX_CASCADE_DEPTH;
        // Pre-create only for keys that are registered, not already on the
        // cascade path (cycle break — a link back to self or an ancestor is
        // dropped), and within the depth budget.
        let registered: Vec<String> = if at_depth_limit {
            Vec::new()
        } else {
            nested_keys
                .into_iter()
                .filter(|k| !child_ancestors.contains(k))
                .filter(|k| with_tooltip_registry(|r| r.get(k).is_some()).unwrap_or(false))
                .collect()
        };
        for key in &registered {
            let nested = RichTooltipWidget::from_key(key.clone())
                .cascade_child()
                .with_cascade_ancestors(child_ancestors.clone());
            // Detached, not a child. Adding these under `children()` would
            // propagate `ctx.activate(nested)` down to their own sub-nested
            // tooltips when the user clicks a link to open one. Those have no
            // overlay registration yet, so they would miss `overlay_skip` and
            // the paint walk would render them as ordinary children at zero
            // size, spilling their TextWidgets' glyphs at the parent origin
            // (the "ghost text" cascade bug).
            //
            // `add_detached` keeps them out of that walk while still recording
            // who owns them, so the arena reaps them with this tooltip. They
            // used to be added with a bare `ctx.add`, which owns nothing: the
            // note here read "nested tooltip widgets leak memory across
            // rebuilds of the host; this is acceptable because
            // RichTooltipWidget is built once per overlay show". It is not
            // built once per show — the content widget is `ctx.add`ed and
            // built on every build of its *anchor*, and each build pre-creates
            // this whole cascade recursively. A writer clicking around an
            // outline stranded thousands of ~16-widget subtrees a minute.
            let nested_id = ctx.add_detached(nested);
            // Dormant immediately: a parentless node is laid out and painted
            // as a root otherwise, instead of waiting for a hover. Activation
            // happens in `make_link_click_handler` when the user clicks a
            // `:key` link.
            ctx.set_dormant(nested_id);
            nested_ids.insert(key.clone(), nested_id);
        }
        let nested_map = Rc::new(nested_ids);

        // Resolve the shortcut label: the manual override wins;
        // otherwise, if the tooltip was bound to a shortcut id via
        // `.for_shortcut(id)`, the effective primary keystroke is
        // pulled from the tree's `ShortcutRegistry`. The registry's
        // `version` signal is bound to the tooltip at `Relayout`
        // level so user rebinds and late registrations refresh the
        // chip on the next pass.
        let shortcut_text: Option<String> = content.shortcut_label.clone().or_else(|| {
            content.shortcut_id.and_then(|id| {
                ctx.effective_shortcut(id)
                    .and_then(|eff| eff.primary.map(format_keystroke))
            })
        });
        if content.shortcut_id.is_some() {
            // Rebuild (not Relayout): the chip's text is read from
            // the registry by value during build(), so a rebind only
            // updates when build() re-enters.
            ctx.shortcut_version().bind_to(
                ctx.self_id(),
                ctx.binding_registry(),
                bastyde_core::binding::BindingLevel::Rebuild,
            );
        }

        // Body row: text + optional shortcut chip.
        // a11y_hidden: the tooltip root owns `set_name(body_text)`, so the
        // body TextWidget would duplicate it as a child Label node.
        // Bind the body to the `LocalizedString` itself (not a resolved
        // snapshot) so a `tr!(...)` source re-renders on locale change
        // without rebuilding the tooltip. `body_source` above is only the
        // build-time snapshot used to pre-scan `:key` cascade links.
        let body_widget = TextWidget::new(content.text.clone())
            .style(TextStyleRole::Small)
            .color(TextRole::TooltipText)
            .markup(true)
            .on_link_click(make_link_click_handler(nested_map.clone(), self_id))
            .a11y_hidden();
        let body_id = ctx.add(body_widget);

        let header: WidgetId = if let Some(shortcut) = shortcut_text {
            let shortcut_widget = TextWidget::new(lit!(shortcut))
                .style(TextStyleRole::Small)
                .color(TextRole::TooltipShortcut)
                .single_line()
                .a11y_hidden();
            let shortcut_id = ctx.add(shortcut_widget);
            // Grid is used here (not HStack + Spacer) because the body
            // text needs a width proposal that excludes the shortcut
            // column so it wraps correctly. HStack would propose the
            // body's natural single-line width and the shortcut chip
            // would overflow off the right edge of the tooltip.
            ctx.add(
                Grid::new()
                    .columns(vec![TrackSize::Fractional(1.0), TrackSize::Auto])
                    .rows(vec![TrackSize::Auto])
                    .column_gap(8.0)
                    .add_child(body_id)
                    .add_child(shortcut_id),
            )
        } else {
            body_id
        };

        // Optional "more" disclosure accordion, independent of the dwell
        // indicator. `None` when the entry has no long-form body.
        let more_accordion: Option<WidgetId> = if let Some(more_ls) = content.more.clone() {
            // Bind the long-form body reactively too (the `more_source`
            // snapshot is only for the build-time `:key` cascade scan).
            let more_widget = TextWidget::new(more_ls)
                .style(TextStyleRole::Small)
                .color(TextRole::TooltipText)
                .markup(true)
                .on_link_click(make_link_click_handler(nested_map.clone(), self_id));
            let expanded = ctx.signal(false);
            // Smaller title style so the disclosure label doesn't
            // dominate the footer row inside a tooltip. Keep the
            // body's line height so the chevron icon aligns
            // vertically with the indicator on the same baseline.
            let mut accordion_title_style = theme.typography.tiny.clone();
            accordion_title_style.line_height = theme.typography.small.line_height;
            // Framework-owned chrome string → resolve against the
            // bastyde-widgets bundle (locales/*.ftl) via tr_widget!, so it
            // translates with the active locale and apps can override it.
            let accordion = Accordion::new(bastyde_i18n::tr_widget!(tooltip_more()), expanded)
                .title_color(theme.colors.tooltip_text)
                .title_style(accordion_title_style)
                .content(more_widget);
            Some(ctx.add(accordion))
        } else {
            None
        };

        let mut root_vstack = VStack::new().spacing(6.0).add_child(header);

        if self.is_cascade_child {
            // Cascade children are opened by an explicit click and are
            // already persistent (`EscapeOrClickOutside`) — there's no
            // hover dwell-to-sticky path, so the `DwellIndicator` would
            // be meaningless. Drop it; keep only the "more" accordion
            // when the entry has one.
            if let Some(accordion) = more_accordion {
                root_vstack = root_vstack.add_child(accordion);
            }
        } else {
            // Dwell indicator, right-anchored in a footer row.
            let indicator = ctx.add(DwellIndicator::new(
                self.dwell_step.clone(),
                self.sticky.clone(),
                TextRole::TooltipText,
            ));
            // Footer row uses a Grid (Fractional + Auto columns) so the
            // accordion column receives an explicit width proposal during
            // Grid's pass-2 measurement. This lets the accordion's
            // expanded "more" content wrap correctly inside the tooltip,
            // and keeps the indicator right-anchored on the same row as
            // the accordion's disclosure label.
            //
            // - Column 0 (Fractional 1.0): accordion (or empty Spacer
            //   when no "more" body) — receives `tooltip_max_width
            //   - indicator_width - column_gap`.
            // - Column 1 (Auto): the dwell indicator — sized to its
            //   intrinsic 14×14.
            let footer_left = more_accordion.unwrap_or_else(|| ctx.add(Spacer::new()));
            let footer_row = ctx.add(
                Grid::new()
                    .columns(vec![TrackSize::Fractional(1.0), TrackSize::Auto])
                    .rows(vec![TrackSize::Auto])
                    .column_gap(8.0)
                    .add_child(footer_left)
                    .add_child(indicator),
            );
            root_vstack = root_vstack.add_child(footer_row);
        }

        let root_content = ctx.add(root_vstack);

        // Wrap everything in padding matching the existing TooltipStyle
        // tokens so RichTooltipWidget drops into the same chrome the
        // plain TooltipWidget uses.
        let padded = ctx.add(
            Padding::symmetric(tt::TOOLTIP_PADDING_VERTICAL, tt::TOOLTIP_PADDING_HORIZONTAL)
                .child_id(root_content),
        );

        self.root_child_id = Some(padded);

        // Sticky tooltip becomes a focusable Dialog. Keyboard users
        // press Tab to enter the promoted surface (e.g. to click
        // inline links). Ephemeral tooltips dismiss on pointer-leave
        // so they can't realistically be tab targets; leaving
        // `focusable(true)` unconditionally avoids a rebuild on every
        // sticky flip.
        let handlers = HandlerSet::new().focusable(true);
        ctx.apply_self_handlers(handlers);

        // Rebind the sticky signal at AccessibilityOnly so the role
        // flip (Tooltip → Dialog) and the `Action::Focus` addition in
        // `accessibility()` reach AT without a relayout or repaint.
        self.sticky.bind_to(
            self_id,
            ctx.binding_registry(),
            bastyde_core::binding::BindingLevel::AccessibilityOnly,
        );

        vec![padded]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> bastyde_core::widget::LayoutResponse {
        // Clamp proposal width to the tooltip max_width token so long
        // bodies wrap rather than stretching the surface.
        let max_w = crate::styles::recipe_tooltip_style::TOOLTIP_MAX_WIDTH;
        let clamped = SizeProposal {
            width: Some(proposal.width.map(|w| w.min(max_w)).unwrap_or(max_w)),
            height: proposal.height,
        };
        self.root_child_id
            .and_then(|id| ctx.child_size(id, clamped))
            .unwrap_or_else(|| Size::new(0.0, 0.0))
            .into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let radius =
            CornerRadius::uniform(crate::styles::recipe_tooltip_style::TOOLTIP_CORNER_RADIUS);
        super::paint_tooltip_shadows(canvas, bounds, radius, ctx);
        canvas.fill_rounded_rect(bounds, radius, ctx.theme.colors.tooltip_bg);
        // paint() is the visibility hook — only called when the
        // tooltip is active. Drives the dwell-promotion timer.
        self.tick_dwell();
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // A tooltip reads as a persistent, focusable panel (non-modal
        // `Role::Dialog` + advertised `Focus`) rather than an ephemeral
        // hover hint when either: it has dwell-promoted to sticky, or it
        // is a cascade child — opened by an explicit click and already
        // persistent (`EscapeOrClickOutside`) from the moment it shows.
        let persistent = self.sticky.get() || self.is_cascade_child;
        let role = if persistent {
            bastyde_core::accesskit::Role::Dialog
        } else {
            bastyde_core::accesskit::Role::Tooltip
        };
        builder.set_role(role);
        if let Some(content) = self.content.as_ref() {
            builder.set_name(content.text.resolve_now());
        }
        if persistent {
            builder.add_action(bastyde_core::accesskit::Action::Focus);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.map(|id| vec![id]).unwrap_or_default()
    }
}

/// Build an `on_link_click` handler closure with two behaviours:
///
/// - When the clicked URL is a tooltip key (`:key`), spawn the
///   corresponding pre-created nested `RichTooltipWidget` as a child
///   overlay of the current tooltip.
/// - When the URL is anything else (`http://`, `https://`, `mailto:`,
///   a bare file path, …), hand it off to the [`open`] crate, which
///   spawns the OS default handler — a browser for web URLs, a mail
///   client for `mailto:`, a file manager for paths, and so on.
///
/// Errors from `open::that` are swallowed and logged at debug level:
/// a failed launch shouldn't crash the UI, and the user can always
/// retry or copy the URL elsewhere. We deliberately don't block the
/// event loop — `open::that` is already synchronous but returns
/// quickly (it spawns the child handler and detaches).
fn make_link_click_handler(
    nested: Rc<HashMap<String, WidgetId>>,
    anchor_id: WidgetId,
) -> impl Fn(&str, &mut bastyde_core::widget::EventContext) + 'static {
    move |url, ctx| {
        if let Some(key) = TooltipRegistry::parse_url(url) {
            if let Some(&content_id) = nested.get(key) {
                ctx.activate(content_id);
                ctx.show_overlay(OverlayRequest {
                    content_id,
                    anchor: anchor_id,
                    placement: OverlayPlacement::NearAnchor {
                        offset: bastyde_canvas::Vec2 { x: 0.0, y: 8.0 },
                    },
                    dismiss: DismissBehavior::EscapeOrClickOutside,
                    layer: OverlayLayer::InTree,
                    // `None` is intentional and correct: the handler
                    // can't know its own overlay id, so the dispatch
                    // layer injects the real parent
                    // (`overlay_ancestor_for_widget(source_widget)` in
                    // event_dispatch_impl.rs). That links this nested
                    // tooltip to the one it was opened from, so
                    // dismissing the parent cascade-closes it
                    // (`OverlayManager::dismiss_immediate` BFS). Same
                    // mechanism MenuItem submenus rely on.
                    parent_overlay: None,
                    on_dismiss: None,
                    fade_duration: None,
                });
            }
            return;
        }

        // Non-tooltip URL — delegate to the OS default handler.
        // Skip in cfg(test) builds so unit tests don't actually try
        // to launch a browser against mock URLs. Errors from
        // `open::that` are intentionally swallowed: a failed launch
        // shouldn't take down the UI, and there's no inline surface
        // to report it to from inside a tooltip.
        #[cfg(not(test))]
        {
            let _ = open::that(url);
        }
        #[cfg(test)]
        let _ = url;
    }
}

/// Scan a minimal-markdown source string for `[label](:key)` link
/// URLs and append every `key` (the part after the leading colon) to
/// `out`. Duplicates are handled by the caller via `sort` + `dedup`.
///
/// This is a deliberate small scanner rather than a re-import of
/// `text-typeset::InlineMarkup::parse`: `bastyde-widgets` doesn't depend
/// on `text-typeset` directly, and extracting just the tooltip key
/// URLs doesn't need the full shaping-aware parser.
fn scan_tooltip_key_urls(source: &str, out: &mut Vec<String>) {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i + 3 < bytes.len() {
        // Telltale sequence for a tooltip-key link: `](:`
        if bytes[i] == b']' && bytes[i + 1] == b'(' && bytes[i + 2] == b':' {
            let start = i + 3;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b')' && bytes[end] != b'\\' {
                end += 1;
            }
            if end < bytes.len()
                && bytes[end] == b')'
                && start < end
                && let Ok(key) = std::str::from_utf8(&bytes[start..end])
            {
                out.push(key.to_string());
            }
            i = end + 1;
        } else {
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_tooltip_key_urls_finds_colon_keys() {
        let mut out = Vec::new();
        scan_tooltip_key_urls("see [docs](:docs-key) and [more](:more-key) here", &mut out);
        assert_eq!(out, vec!["docs-key".to_string(), "more-key".to_string()]);
    }

    #[test]
    fn scan_tooltip_key_urls_ignores_http_links() {
        let mut out = Vec::new();
        scan_tooltip_key_urls("go to [example](https://example.com)", &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn scan_tooltip_key_urls_mixed() {
        let mut out = Vec::new();
        scan_tooltip_key_urls(
            "[regular](https://x) and [tip](:my-key) and [also](:other)",
            &mut out,
        );
        assert_eq!(out, vec!["my-key".to_string(), "other".to_string()]);
    }

    #[test]
    fn scan_tooltip_key_urls_empty_source() {
        let mut out = Vec::new();
        scan_tooltip_key_urls("", &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn scan_tooltip_key_urls_no_links() {
        let mut out = Vec::new();
        scan_tooltip_key_urls("no links here at all", &mut out);
        assert!(out.is_empty());
    }

    fn rich_tooltip_height(content: TooltipContent, cascade: bool) -> f32 {
        use bastyde_canvas::MockTextBackend;
        use bastyde_core::widget_tree::WidgetTree;
        use std::cell::RefCell;

        let mut tree =
            WidgetTree::new().with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        let mut w = RichTooltipWidget::new(content);
        if cascade {
            w = w.cascade_child();
        }
        let id = tree.add(w);
        // Loose height (width-only) proposal so the tooltip sizes to its
        // natural content height — an exact height would just be echoed
        // back as the root's bounds. The VStack has no flexible children,
        // so the measured height reflects the real layout.
        tree.layout(SizeProposal::with_width(400.0));
        tree.bounds(id).height
    }

    #[test]
    fn cascade_child_omits_dwell_indicator() {
        // A normal rich tooltip carries the dwell-to-sticky indicator in
        // a footer row; a cascade child (opened from a `[label](:key)`
        // link) suppresses it, since it's already persistent and has no
        // hover-to-sticky path. With identical content, the cascade
        // child therefore lays out shorter — no indicator footer row.
        let make = || TooltipContent::new("k", lit!("Tooltip body"));
        let normal_h = rich_tooltip_height(make(), false);
        let cascade_h = rich_tooltip_height(make(), true);
        assert!(
            cascade_h < normal_h,
            "cascade child should be shorter without the dwell-indicator footer \
             (cascade = {cascade_h}, normal = {normal_h})"
        );
    }

    #[test]
    fn cascade_child_announces_as_persistent_dialog() {
        use bastyde_core::accessibility::AccessNodeBuilder;
        use bastyde_core::accesskit::{Action, Role};

        // A cascade child is persistent and focusable from the moment it
        // shows, so it reads as a non-modal `Dialog` advertising `Focus`
        // — not an ephemeral `Tooltip`.
        let child = RichTooltipWidget::new(TooltipContent::new("k", lit!("Body"))).cascade_child();
        let mut cb = AccessNodeBuilder::new();
        child.accessibility(&mut cb);
        assert_eq!(
            cb.role(),
            Role::Dialog,
            "cascade child should read as Dialog"
        );
        assert!(
            cb.actions().contains(&Action::Focus),
            "cascade child should advertise the Focus action"
        );

        // A normal tooltip stays an ephemeral Tooltip until it dwell-promotes.
        let normal = RichTooltipWidget::new(TooltipContent::new("k", lit!("Body")));
        let mut nb = AccessNodeBuilder::new();
        normal.accessibility(&mut nb);
        assert_eq!(
            nb.role(),
            Role::Tooltip,
            "non-cascade tooltip stays a Tooltip when not sticky"
        );
        assert!(
            !nb.actions().contains(&Action::Focus),
            "non-sticky tooltip should not advertise Focus"
        );
    }

    #[test]
    fn cyclic_cascade_links_do_not_overflow_the_stack() {
        use crate::tooltip::registry::{_reset_tooltip_registry, install_tooltip_registry};
        use bastyde_canvas::MockTextBackend;
        use bastyde_core::widget_tree::WidgetTree;
        use std::cell::RefCell;

        // The shape that used to overflow: a `[label](:key)` cycle
        // `a → b → c → a`, plus a self-link `a → a`. Eager, recursive
        // pre-creation of cascade children followed the cycle forever.
        _reset_tooltip_registry();
        install_tooltip_registry(vec![
            TooltipContent::new("a", lit!("A cites [b](:b) and itself [a](:a)")),
            TooltipContent::new("b", lit!("B cites [c](:c)")),
            TooltipContent::new("c", lit!("C cites back to [a](:a)")),
        ]);

        let mut tree =
            WidgetTree::new().with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        // Reaching layout at all proves the visited-set guard terminated
        // the cascade pre-creation instead of recursing to a stack overflow.
        let id = tree.add(RichTooltipWidget::from_key("a"));
        tree.layout(SizeProposal::with_width(400.0));
        assert!(tree.bounds(id).height >= 0.0);

        _reset_tooltip_registry();
    }

    /// Rebuilding a rich tooltip must not strand the cascade children it
    /// pre-creates.
    ///
    /// `build()` eagerly pre-creates one `RichTooltipWidget` per `[label](:key)`
    /// link in the body, recursively — and those are deliberately not children
    /// (activating one would propagate down to its own sub-nested tooltips,
    /// which paint inline as "ghost text"). Held by a bare `ctx.add` they were
    /// owned by nobody, so every rebuild left the whole cascade behind: in
    /// Skribisto, whose outline attaches these to its Create/Convert menus,
    /// clicking between two rows stranded ~2 600 widgets *per click* and grew
    /// the arena's slotmap past 124 MiB in a couple of minutes.
    #[test]
    fn rebuilding_a_rich_tooltip_reaps_its_cascade_children() {
        use crate::tooltip::registry::{_reset_tooltip_registry, install_tooltip_registry};
        use bastyde_canvas::MockTextBackend;
        use bastyde_core::widget_tree::WidgetTree;
        use std::cell::RefCell;

        _reset_tooltip_registry();
        install_tooltip_registry(vec![
            TooltipContent::new("root", lit!("Root cites [b](:b) and [c](:c)")),
            TooltipContent::new("b", lit!("B cites [d](:d)")),
            TooltipContent::new("c", lit!("C is a leaf")),
            TooltipContent::new("d", lit!("D is a leaf")),
        ]);

        let mut tree =
            WidgetTree::new().with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        let id = tree.add(RichTooltipWidget::from_key("root"));
        tree.layout(SizeProposal::with_width(400.0));

        let baseline = tree.widget_count();
        assert!(
            baseline > 1,
            "the cascade must actually pre-create children"
        );
        for _ in 0..10 {
            tree.arena_mark_needs_rebuild_for_testing(id);
            tree.layout(SizeProposal::with_width(400.0));
        }
        assert_eq!(
            tree.widget_count(),
            baseline,
            "each rebuild stranded another copy of the cascade"
        );

        // …and destroying the tooltip takes the cascade with it.
        tree.destroy_subtree_for_testing(id);
        tree.layout(SizeProposal::with_width(400.0));
        assert_eq!(
            tree.widget_count(),
            0,
            "the whole cascade must die with the tooltip that owns it"
        );

        _reset_tooltip_registry();
    }

    /// Host with two focusable-by-`tree.focus()` anchors, the first carrying a
    /// rich tooltip. Used to drive the keyboard path end to end: focus
    /// promotion, Tab-into-the-surface, Escape, and re-summoning.
    #[derive(Debug)]
    struct FocusTooltipHost {
        anchor_id: Option<WidgetId>,
        elsewhere_id: Option<WidgetId>,
        ids_sink: Rc<std::cell::Cell<Option<(WidgetId, WidgetId, WidgetId)>>>,
    }

    impl bastyde_core::widget::Widget for FocusTooltipHost {
        fn build(&mut self, ctx: &mut bastyde_core::build_context::BuildContext) -> Vec<WidgetId> {
            let anchor = ctx.add(crate::primitives::TextWidget::new(lit!("anchor")));
            let elsewhere = ctx.add(crate::primitives::TextWidget::new(lit!("elsewhere")));
            let tip = crate::tooltip::attach::attach_rich_tooltip_content(
                ctx,
                anchor,
                TooltipContent::new("focus-tip", lit!("Focus-promoted body")),
                ctx.theme().motion.tooltip_delay,
            );
            self.anchor_id = Some(anchor);
            self.elsewhere_id = Some(elsewhere);
            self.ids_sink.set(Some((anchor, elsewhere, tip)));
            vec![anchor, elsewhere]
        }
        fn layout_response(
            &self,
            proposal: bastyde_canvas::SizeProposal,
            ctx: &bastyde_core::LayoutContext<'_>,
        ) -> bastyde_core::LayoutResponse {
            self.anchor_id
                .and_then(|id| ctx.child_size(id, proposal))
                .unwrap_or_else(|| bastyde_canvas::Size::new(0.0, 0.0))
                .into()
        }
        fn children(&self) -> Vec<WidgetId> {
            self.anchor_id
                .into_iter()
                .chain(self.elsewhere_id)
                .collect()
        }
    }

    /// `reduced_motion` is a real behavioural axis here, not a cosmetic one.
    /// With the fade enabled, a dismissed overlay lingers while it fades and
    /// `try_dismiss_top_on_escape` reports no content ids yet, so the entry
    /// keeps its stale `overlay_id` and `tooltip_focus_enter` skips it for
    /// unrelated reasons. That masks the re-summon bug for most users and
    /// exposes it only for those who have asked for reduced motion — i.e.
    /// exactly the audience most likely to be navigating by keyboard.
    fn focus_tooltip_tree_with(
        reduced_motion: bool,
    ) -> (
        bastyde_core::widget_tree::WidgetTree,
        WidgetId,
        WidgetId,
        WidgetId,
    ) {
        use bastyde_canvas::MockTextBackend;
        use bastyde_core::widget_tree::WidgetTree;
        use std::cell::RefCell;

        let mut tree =
            WidgetTree::new().with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        tree.set_accessibility_preferences(false, reduced_motion, 1.0);
        let sink = Rc::new(std::cell::Cell::new(None));
        tree.add(FocusTooltipHost {
            anchor_id: None,
            elsewhere_id: None,
            ids_sink: sink.clone(),
        });
        tree.layout(SizeProposal::exact(400.0, 200.0));
        let (anchor, elsewhere, tip) = sink.get().expect("host built");
        (tree, anchor, elsewhere, tip)
    }

    /// Two real `Button`s, the first carrying a rich tooltip — the shape every
    /// real call site has, and the one Tab traversal can be reasoned about in.
    fn two_buttons_first_with_tooltip()
    -> (bastyde_core::widget_tree::WidgetTree, WidgetId, WidgetId) {
        use crate::button::Button;
        use bastyde_canvas::MockTextBackend;
        use bastyde_core::widget_tree::WidgetTree;
        use std::cell::RefCell;

        let mut tree =
            WidgetTree::new().with_text_backend(Rc::new(RefCell::new(MockTextBackend::new())));
        tree.set_accessibility_preferences(false, true, 1.0);
        let first = tree.add(
            Button::new(lit!("First")).rich_tooltip_content(TooltipContent::new(
                "first-tip",
                lit!("Body of the first button's tip"),
            )),
        );
        let second = tree.add(Button::new(lit!("Second")));
        tree.layout(SizeProposal::exact(400.0, 200.0));
        (tree, first, second)
    }

    /// Focus *arms* the tip's delay; it does not show on arrival.
    ///
    /// The pointer has always required a pause before a tip appears. Showing
    /// the instant focus arrived made a Tab sweep across a row of tooltipped
    /// buttons strobe a tip at every stop.
    #[test]
    fn focus_arms_the_delay_rather_than_showing_the_tooltip_on_arrival() {
        let (mut tree, first, _second) = two_buttons_first_with_tooltip();

        tree.focus(first);

        assert!(
            tree.active_overlays().is_empty(),
            "focus arriving must not pop a tip — it arms the same delay the \\
             pointer arms"
        );

        tree.advance_time(tree.theme().motion.tooltip_delay + Duration::from_millis(50));

        assert_eq!(
            tree.active_overlays().len(),
            1,
            "and the tip appears once focus has come to rest for the delay"
        );
        assert!(
            !tree.tooltip_is_sticky_within(first),
            "resting long enough to show is still not long enough to promote"
        );
    }

    /// Tabbing straight past a control never shows its tip at all.
    #[test]
    fn tabbing_through_without_resting_never_shows_a_tooltip() {
        use bastyde_core::event::{Key, Modifiers};

        let (mut tree, first, second) = two_buttons_first_with_tooltip();
        tree.focus(first);

        // Move on well before the delay ripens.
        tree.advance_time(Duration::from_millis(80));
        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(tree.focused(), Some(second));

        // Let far more than the delay pass on the *new* control.
        tree.advance_time(tree.theme().motion.tooltip_delay * 4);

        assert!(
            tree.active_overlays().is_empty(),
            "a tip armed by focus that has already moved on must be disarmed, \\
             not left to open a beat later over a control the user has left"
        );
    }

    /// Tab from a control whose tip is merely *shown* continues past it.
    ///
    /// An unpromoted tip appeared because focus arrived, not because the user
    /// asked to enter it, so it takes no Tab stop — the ARIA tooltip rule.
    #[test]
    fn tab_skips_an_unpromoted_tooltip_and_goes_to_the_next_control() {
        use bastyde_core::event::{Key, Modifiers};

        let (mut tree, first, second) = two_buttons_first_with_tooltip();
        tree.focus(first);
        tree.advance_time(tree.theme().motion.tooltip_delay + Duration::from_millis(50));
        assert_eq!(tree.active_overlays().len(), 1);

        tree.press_key(Key::Tab, Modifiers::NONE);

        assert_eq!(
            tree.focused(),
            Some(second),
            "an unpromoted tip is informational and must not capture Tab"
        );
    }

    /// A promoted panel takes the Tab stop **directly after its anchor** —
    /// not wherever arena insertion order happened to put its parentless root.
    #[test]
    fn a_promoted_tooltip_takes_the_tab_stop_right_after_its_anchor() {
        use bastyde_core::event::{Key, Modifiers};

        let (mut tree, first, second) = two_buttons_first_with_tooltip();
        tree.focus(first);
        tree.advance_time(tree.theme().motion.tooltip_delay + Duration::from_millis(50));
        let tip = tree
            .tooltip_content_within(first)
            .expect("the button registered a tooltip");
        tree.promote_tooltip_to_sticky(tip);

        tree.press_key(Key::Tab, Modifiers::NONE);
        let after_anchor = tree.focused().expect("Tab landed somewhere");
        assert!(
            after_anchor == tip || tree.is_descendant_of(after_anchor, tip),
            "a promoted panel belongs immediately after the control it \
             describes, the way a disclosure's panel follows its button"
        );

        tree.press_key(Key::Tab, Modifiers::NONE);
        assert_eq!(
            tree.focused(),
            Some(second),
            "and traversal continues to the next control once past it"
        );
    }

    /// Escape must hand focus back to the anchor.
    ///
    /// The keyboard path (`tooltip_focus_enter`) opens its overlay with the
    /// plain `show_overlay`, which records no focus-restore target — so the
    /// Escape handler skipped its restore branch entirely, leaving focus
    /// stranded on the tooltip content that had just gone dormant. The next
    /// `revalidate_interaction_state` pass then drops it to `None`, so a user
    /// who Tabbed into the surface to reach a link or control ended up with no
    /// focus at all and had to Tab in from the top of the window again.
    #[test]
    fn escape_returns_focus_to_the_anchor_after_tabbing_into_a_focus_promoted_tooltip() {
        use bastyde_core::event::{Key, Modifiers};

        let (mut tree, anchor, _elsewhere, tip) = focus_tooltip_tree_with(true);

        tree.focus(anchor);
        tree.advance_time(tree.theme().motion.tooltip_delay + Duration::from_millis(50));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "focus coming to rest must surface the rich tooltip"
        );
        // Focus no longer promotes on arrival; the dwell does. Promote through
        // the public API so the test does not depend on wall-clock timing.
        tree.promote_tooltip_to_sticky(tip);

        // Move into the surface — the step that makes the bug observable. With
        // focus still on the anchor, `focus_with_origin_ops` early-returns on
        // an already-focused id and nothing is lost.
        tree.focus(tip);
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "focus moving into the tooltip's own content must not dismiss it"
        );

        tree.press_key(Key::Escape, Modifiers::NONE);

        assert!(
            tree.active_overlays().is_empty(),
            "Escape must dismiss the focus-promoted tooltip"
        );
        assert_eq!(
            tree.focused(),
            Some(anchor),
            "Escape must return focus to the anchor, not strand it on the \
             dismissed surface"
        );
    }

    /// …and must not re-open the tooltip on that same keystroke.
    ///
    /// The restore runs the ordinary focus path, which ends in
    /// `tooltip_focus_enter`; by then `dormant_dismissed_content` has cleared
    /// the entry's `overlay_id`, so without the suppression flag the entry
    /// looks eligible again and the tip the user just dismissed springs back.
    #[test]
    fn escape_does_not_immediately_re_summon_the_tooltip_it_dismissed() {
        use bastyde_core::event::{Key, Modifiers};

        let (mut tree, anchor, _elsewhere, tip) = focus_tooltip_tree_with(true);
        tree.focus(anchor);
        tree.advance_time(tree.theme().motion.tooltip_delay + Duration::from_millis(50));
        tree.promote_tooltip_to_sticky(tip);
        tree.focus(tip);
        tree.press_key(Key::Escape, Modifiers::NONE);

        assert!(
            tree.active_overlays().is_empty(),
            "the restored focus must not re-trigger the tooltip it just closed"
        );
    }

    /// The suppression is served by leaving, not permanent: Tab away and back
    /// and the tooltip returns. Otherwise one Escape would mute that anchor
    /// for the rest of the session.
    #[test]
    fn a_dismissed_focus_tooltip_returns_after_focus_leaves_and_comes_back() {
        use bastyde_core::event::{Key, Modifiers};

        let (mut tree, anchor, elsewhere, tip) = focus_tooltip_tree_with(true);
        tree.focus(anchor);
        tree.advance_time(tree.theme().motion.tooltip_delay + Duration::from_millis(50));
        tree.promote_tooltip_to_sticky(tip);
        tree.focus(tip);
        tree.press_key(Key::Escape, Modifiers::NONE);
        assert!(tree.active_overlays().is_empty());

        tree.focus(elsewhere);
        tree.advance_time(tree.theme().motion.tooltip_delay + Duration::from_millis(50));
        assert!(
            tree.active_overlays().is_empty(),
            "an unrelated widget must not surface the anchor's tooltip"
        );

        tree.focus(anchor);
        tree.advance_time(tree.theme().motion.tooltip_delay + Duration::from_millis(50));
        assert_eq!(
            tree.active_overlays().len(),
            1,
            "returning to the anchor must summon its tooltip again"
        );
    }
}
