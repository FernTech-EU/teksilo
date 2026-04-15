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
//! - The top-right [`DwellIndicator`] reads the step signal and
//!   paints an empty circle filling progressively in 4 wedges.
//! - At step 4 the indicator flips to a pin icon and the widget's
//!   `sticky` signal goes true. The widget tree (via
//!   `attach_tooltip_with_sticky`) auto-promotes the overlay on
//!   the same 2 s timer: removes the entry from the hover tracker
//!   and swaps the dismiss behavior to `EscapeOrClickOutside`. The
//!   widget's a11y role flips from `Tooltip` to `Dialog`.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::build_context::BuildContext;
use fern_core::overlay::{DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest};
use fern_core::signal::Signal;
use fern_core::widget::{LayoutContext, PaintContext, Widget};
use fern_core::widget_id::WidgetId;
use fern_i18n::LocalizedString;
use fern_tokens::CornerRadius;

use crate::accordion::Accordion;
use crate::primitives::{Grid, Padding, Spacer, TextWidget, TrackSize, VStack};
use crate::tooltip::dwell_indicator::DwellIndicator;
use crate::tooltip::registry::{with_tooltip_registry, TooltipContent, TooltipRegistry};

/// Total dwell time before the tooltip promotes to sticky.
pub(crate) const DWELL_PROMOTION: Duration = Duration::from_secs(2);
/// Maximum step value (4 = full circle = pin icon).
const DWELL_STEPS: u32 = 4;
/// Per-step dwell duration: total / steps = 500 ms.
const DWELL_STEP_DURATION: Duration = Duration::from_millis(
    (DWELL_PROMOTION.as_millis() / DWELL_STEPS as u128) as u64,
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
}

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
        }
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
        let new_step = ((elapsed.as_millis() / DWELL_STEP_DURATION.as_millis()) as u32)
            .min(DWELL_STEPS);
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

        let theme = ctx.theme().clone();
        let style = theme.components.tooltip;
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
        // Only pre-create for keys that actually exist in the registry.
        let registered: Vec<String> = nested_keys
            .into_iter()
            .filter(|k| {
                with_tooltip_registry(|r| r.get(k).is_some()).unwrap_or(false)
            })
            .collect();
        for key in &registered {
            let nested = RichTooltipWidget::from_key(key.clone());
            let nested_id = ctx.add(nested);
            // Mark dormant immediately. `ctx.add` inserts widgets at
            // arena top-level (not as children of `self`), so without
            // this they would render alongside the main scene and
            // never wait for a hover event. Activation happens in
            // `make_link_click_handler` when the user clicks a `:key`
            // link.
            ctx.set_dormant(nested_id);
            nested_ids.insert(key.clone(), nested_id);
        }
        let nested_map = Rc::new(nested_ids);

        // Resolve the shortcut label: manual override first, fall back
        // to `ctx.shortcut_label_for_any(command)` for command-bound
        // tooltips so the live ShortcutMap is the source of truth.
        let shortcut_text: Option<String> = content
            .shortcut_label
            .clone()
            .or_else(|| {
                content
                    .command
                    .as_ref()
                    .and_then(|c| ctx.shortcut_label_for_any(c.as_ref()))
            });

        // Body row: text + optional shortcut chip.
        // a11y_hidden: the tooltip root owns `set_name(body_text)`, so the
        // body TextWidget would duplicate it as a child Label node.
        let body_widget = TextWidget::new_literal(body_source)
            .style(theme.typography.small.clone())
            .color(theme.colors.tooltip_text)
            .markup(true)
            .on_link_click(make_link_click_handler(nested_map.clone(), self_id))
            .a11y_hidden();
        let body_id = ctx.add(body_widget);

        let header: WidgetId = if let Some(shortcut) = shortcut_text {
            let shortcut_widget = TextWidget::new_literal(shortcut)
                .style(theme.typography.small.clone())
                .color(theme.colors.tooltip_shortcut)
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

        // Dwell indicator — created up front so we can drop it into
        // either the Accordion header row (when there is a "more"
        // body) or a dedicated footer row (no "more" body).
        let indicator = ctx.add(DwellIndicator::new(
            self.dwell_step.clone(),
            self.sticky.clone(),
            theme.colors.tooltip_text,
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
        let footer_left: WidgetId = if let Some(more_text) = more_source {
            let more_widget = TextWidget::new_literal(more_text)
                .style(theme.typography.small.clone())
                .color(theme.colors.tooltip_text)
                .markup(true)
                .on_link_click(make_link_click_handler(nested_map.clone(), self_id));
            let expanded = ctx.signal(false);
            // Smaller title style so the disclosure label doesn't
            // dominate the footer row inside a tooltip. Keep the
            // body's line height so the chevron icon aligns
            // vertically with the indicator on the same baseline.
            let mut accordion_title_style = theme.typography.tiny.clone();
            accordion_title_style.line_height = theme.typography.small.line_height;
            let accordion = Accordion::new(LocalizedString::literal("More"), expanded)
                .title_color(theme.colors.tooltip_text)
                .title_style(accordion_title_style)
                .content(more_widget);
            ctx.add(accordion)
        } else {
            ctx.add(Spacer::new())
        };

        let footer_row = ctx.add(
            Grid::new()
                .columns(vec![TrackSize::Fractional(1.0), TrackSize::Auto])
                .rows(vec![TrackSize::Auto])
                .column_gap(8.0)
                .add_child(footer_left)
                .add_child(indicator),
        );

        let root_content = ctx.add(
            VStack::new()
                .spacing(6.0)
                .add_child(header)
                .add_child(footer_row),
        );

        // Wrap everything in padding matching the existing TooltipStyle
        // tokens so RichTooltipWidget drops into the same chrome the
        // plain TooltipWidget uses.
        let padded = ctx.add(
            Padding::symmetric(style.padding_vertical, style.padding_horizontal)
                .set_child(root_content),
        );

        self.root_child_id = Some(padded);
        vec![padded]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        // Clamp proposal width to the tooltip max_width token so long
        // bodies wrap rather than stretching the surface.
        let max_w = ctx.theme.components.tooltip.max_width;
        let clamped = SizeProposal {
            width: Some(
                proposal
                    .width
                    .map(|w| w.min(max_w))
                    .unwrap_or(max_w),
            ),
            height: proposal.height,
        };
        self.root_child_id
            .and_then(|id| ctx.child_size(id, clamped))
            .unwrap_or_else(|| Size::new(0.0, 0.0))
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let style = ctx.theme.components.tooltip;
        let radius = CornerRadius::uniform(style.corner_radius);
        canvas.fill_rounded_rect(bounds, radius, ctx.theme.colors.tooltip_bg);
        // paint() is the visibility hook — only called when the
        // tooltip is active. Drives the dwell-promotion timer.
        self.tick_dwell();
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Role flips from Tooltip to Dialog (non-modal) once the
        // tooltip is sticky, since a sticky tooltip behaves like a
        // persistent panel rather than an ephemeral hover hint.
        let role = if self.sticky.get() {
            fern_core::accesskit::Role::Dialog
        } else {
            fern_core::accesskit::Role::Tooltip
        };
        builder.set_role(role);
        if let Some(content) = self.content.as_ref() {
            builder.set_name(&content.text.resolve_now());
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
) -> impl Fn(&str, &mut fern_core::widget::EventContext) + 'static {
    move |url, ctx| {
        if let Some(key) = TooltipRegistry::parse_url(url) {
            if let Some(&content_id) = nested.get(key) {
                ctx.activate(content_id);
                ctx.show_overlay(OverlayRequest {
                    content_id,
                    anchor: anchor_id,
                    placement: OverlayPlacement::NearAnchor {
                        offset: fern_canvas::Vec2 { x: 0.0, y: 8.0 },
                    },
                    dismiss: DismissBehavior::EscapeOrClickOutside,
                    layer: OverlayLayer::InTree,
                    parent_overlay: None,
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
/// `text-typeset::InlineMarkup::parse`: `fern-widgets` doesn't depend
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
            if end < bytes.len() && bytes[end] == b')' && start < end
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
        scan_tooltip_key_urls(
            "see [docs](:docs-key) and [more](:more-key) here",
            &mut out,
        );
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
}

