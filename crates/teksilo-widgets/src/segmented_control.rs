// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! SegmentedControl — mutually exclusive segments in a horizontal row.
//!
//! Each segment is a real composed widget — a centered icon + label with
//! a reactive tint — built from a [`Segment`] descriptor. Selection is
//! bound to a `Signal<Option<SegmentId>>`: **keyed, not positional**, so
//! inserting or removing a segment never silently re-points the
//! selection at a different one. The chrome (rounded frame, hover tint,
//! selected-segment surface) is delegated to the active
//! [`SegmentedControlStyle`](teksilo_core::styles::SegmentedControlStyle).
//!
//! ```ignore
//! const LIST: SegmentId = SegmentId::from_u64(1);
//! const GRID: SegmentId = SegmentId::from_u64(2);
//!
//! let view = ctx.signal(Some(LIST));
//! SegmentedControl::new(view.clone())
//!     .segment(Segment::new(tr!(list_view())).id(LIST).icon(|| IconWidget::list(14.0)))
//!     .segment(Segment::new(tr!(grid_view())).id(GRID).icon(|| IconWidget::grid(14.0)))
//!
//! // Pairing with a Switcher:
//! Switcher::new(segmented_control::index_signal(&view, &[LIST, GRID]))
//! ```
//!
//! ## When to use
//!
//! - Use a `SegmentedControl` for mutually exclusive modes that read
//!   well as a compact horizontal strip (view mode, time period).
//! - Prefer a `ComboBox` when the options are many *and* the strip form
//!   buys nothing — though a segmented control no longer breaks down at
//!   seven segments, because it overflows (below).
//! - Prefer `RadioButton` / `RadioTileGroup` when the options need
//!   vertical space or descriptions.
//!
//! ## Width: overflow, not squeeze
//!
//! When the segments do not fit, the ones that do not fit move into a
//! trailing chevron menu rather than all of them compressing into
//! ellipsised stubs ([`SegmentOverflow::Menu`], the default; opt out with
//! [`SegmentOverflow::Compress`]).
//!
//! Declaration order is stable, with exactly one exception: **the
//! selected segment is always visible**. If it would have been pushed
//! into the menu it takes the *last* slot, and it stays there until
//! another segment is chosen from the menu — so the strip does not
//! reshuffle under the pointer, and the promotion is forgotten once the
//! control is wide enough to show everything again.
//!
//! ```text
//! Declared: [A][B][C][D][E][F][G]   fits 4 + chevron
//!
//! start, A selected     [A][B][C][D][v]   menu: E F G
//! pick F from menu      [A][B][C][F][v]   menu: D E G
//! click A (F stays)     [A][B][C][F][v]   menu: D E G
//! widen to full fit     [A][B][C][D][E][F][G]
//! ```
//!
//! ## Accessibility
//!
//! `Role::RadioGroup` on the control with `active_descendant` pointing at
//! the selected segment; `Role::RadioButton` per segment, carrying
//! "N of M" over the whole segment list — including segments currently in
//! the overflow menu, which are still reachable. Arrow keys cycle
//! selection (RTL-aware, resolved at event time) and Home/End jump to the
//! ends, both skipping disabled segments; stepping onto an overflowed
//! segment promotes it into view. `Increment`/`Decrement` AT actions
//! mirror the arrows.
//!
//! The strip is **one** tab stop. While the control is overflowing the
//! chevron adds a second, because an overflow menu that no keyboard can
//! reach is not an overflow menu; it cannot join the arrow sequence,
//! since here arrows move *selection* rather than a roving focus.

mod cell;
mod id;
mod overflow;

#[cfg(test)]
mod tests;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::focus::FocusOrigin;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::{
    SegmentSlotGeometry, SegmentSlots, SegmentedControlStyleConfig, SharedSegmentedControlStyle,
};
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::LocalizedString;

use crate::primitives::IconWidget;
use crate::styles::recipe_segmented_control_style::{
    SEGMENTED_CONTROL_BORDER_WIDTH, SEGMENTED_CONTROL_HEIGHT, SEGMENTED_CONTROL_PADDING_HORIZONTAL,
    SEGMENTED_CONTROL_PADDING_VERTICAL,
};
use cell::SegmentCell;
use overflow::Plan;

pub use id::SegmentId;

/// Fallback line height when no text backend is available.
const FALLBACK_LINE_HEIGHT: f32 = 16.0;
/// Gap between a segment's icon and its label.
pub(crate) const SEGMENT_ICON_LABEL_SPACING: f32 = 6.0;
/// Size of the overflow chevron glyph.
const OVERFLOW_ICON_SIZE: f32 = 12.0;

/// Factory that builds a segment's leading icon. `Rc` (not `Box`) so a
/// `Segment` descriptor can be cloned into a fresh cell on every rebuild
/// without consuming it.
pub(crate) type IconFactory = Rc<dyn Fn() -> IconWidget>;

/// What a segment paints: its icon, its label, or both.
///
/// Set on the control with
/// [`SegmentedControl::display`](super::SegmentedControl::display); it
/// applies to every segment. Mirrors `TabWidget`'s `TabDisplayMode`.
///
/// Icon-only is the classic compact fallback *before* overflow kicks in:
/// a bar of icon-only segments fits far more of them, so switching to
/// [`Icon`](SegmentDisplay::Icon) can be the difference between a
/// complete strip and a chevron menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SegmentDisplay {
    /// Paint whatever the segment declares — icon *and* label when both
    /// are present, label alone otherwise. The default, and the
    /// behaviour of every `SegmentedControl` before this mode existed.
    #[default]
    Auto,
    /// Label only. A declared icon is suppressed.
    Text,
    /// Icon only; the label is promoted to the hover tooltip (unless the
    /// segment already declares one). A segment with **no** icon falls
    /// back to its label, so the mode is never a silent no-op.
    Icon,
    /// Icon and label. Identical to [`Auto`](SegmentDisplay::Auto) for a
    /// segment that declares both; kept for parity with
    /// `TabDisplayMode` so a caller can be explicit.
    IconText,
}

/// How the visible segments divide the control's width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SegmentSizing {
    /// Every visible segment gets the same width — the Apple / IntUI
    /// look, and the behaviour of every `SegmentedControl` before this
    /// knob existed. The fit calculation uses the *widest* segment's
    /// natural width as the unit, so segments never look ragged.
    #[default]
    Uniform,
    /// Every visible segment gets its own natural width, and leftover
    /// space (when the control fills a wider slot) is shared equally.
    /// Fits more short segments before overflowing, at the cost of an
    /// uneven strip.
    Fit,
}

/// What the control does when its segments do not fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SegmentOverflow {
    /// Move the segments that do not fit into a trailing chevron menu,
    /// keeping the rest at a legible width. The selected segment is
    /// always among the visible ones. This is the default.
    #[default]
    Menu,
    /// Keep every segment on the strip and let them compress, truncating
    /// labels with an ellipsis. The behaviour of every
    /// `SegmentedControl` before overflow existed — appropriate for two
    /// or three short segments that will never realistically overflow.
    Compress,
}

/// One segment descriptor: a localized label with a stable
/// [`SegmentId`], an optional leading icon, a hover tooltip, and
/// reactive disabled / visible flags.
#[derive(Clone)]
pub struct Segment {
    pub(crate) id: SegmentId,
    pub(crate) label: LocalizedString,
    pub(crate) icon: Option<IconFactory>,
    /// Plain-text hover tooltip — mutually exclusive with
    /// `rich_tooltip_source` and `composite_tooltip_factory`.
    pub(crate) tooltip: Option<LocalizedString>,
    /// Rich-tooltip source — mutually exclusive with `tooltip` and
    /// `composite_tooltip_factory`. `RichTooltipSource` is `Clone`.
    pub(crate) rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Composite-tooltip factory — mutually exclusive with `tooltip` and
    /// `rich_tooltip_source`. Stored as an `Rc<dyn Fn>` (not `Box<dyn
    /// Widget>`) so the `Segment: Clone` derive stays intact.
    pub(crate) composite_tooltip_factory: Option<Rc<dyn Fn() -> Box<dyn Widget>>>,
    pub(crate) disabled: Prop<bool>,
    pub(crate) visible: Prop<bool>,
}

impl Segment {
    /// A text segment with a freshly allocated [`SegmentId`]. The label
    /// may come from `tr!(...)` (translated — follows a live locale
    /// switch) or `lit!(...)` (untranslated).
    ///
    /// Call [`id`](Self::id) when the segment needs a *stable* identity —
    /// one that survives a restart, or that another crate can name.
    pub fn new(label: impl Into<LocalizedString>) -> Self {
        Self {
            id: SegmentId::fresh(),
            label: label.into(),
            icon: None,
            tooltip: None,
            rich_tooltip_source: None,
            composite_tooltip_factory: None,
            disabled: Prop::Static(false),
            visible: Prop::Static(true),
        }
    }

    /// Give this segment an app-chosen stable identity, replacing the
    /// fresh id [`new`](Self::new) allocated. Use this whenever the
    /// selection is persisted or the segment is contributed by another
    /// crate.
    pub fn id(mut self, id: SegmentId) -> Self {
        self.id = id;
        self
    }

    /// This segment's identity.
    pub fn segment_id(&self) -> SegmentId {
        self.id
    }

    /// Add a leading icon. The factory is invoked at build time (and on
    /// rebuild); the icon's tint is bound reactively to the segment's
    /// selected / focus / enabled state so it matches the label.
    pub fn icon(mut self, factory: impl Fn() -> IconWidget + 'static) -> Self {
        self.icon = Some(Rc::new(factory));
        self
    }

    /// Hover tooltip — most useful for icon-only segments.
    ///
    /// Mutually exclusive with [`rich_tooltip`](Self::rich_tooltip) /
    /// [`rich_tooltip_content`](Self::rich_tooltip_content) /
    /// [`composite_tooltip`](Self::composite_tooltip) — last call wins.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_factory = None;
        self
    }

    /// Rich hover tooltip resolved from the app-wide registry by key.
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip) /
    /// [`rich_tooltip_content`](Self::rich_tooltip_content) /
    /// [`composite_tooltip`](Self::composite_tooltip) — last call wins.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip = None;
        self.composite_tooltip_factory = None;
        self
    }

    /// Rich hover tooltip driven by an inline
    /// [`TooltipContent`](crate::tooltip::TooltipContent) entry
    /// (no registry key needed).
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip) /
    /// [`rich_tooltip`](Self::rich_tooltip) /
    /// [`composite_tooltip`](Self::composite_tooltip) — last call wins.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip = None;
        self.composite_tooltip_factory = None;
        self
    }

    /// Composite hover tooltip built by a factory closure at attach time.
    ///
    /// The factory is called once per `build()` to produce the tooltip
    /// body widget. It is stored as an `Rc<dyn Fn>` so that `Segment`
    /// remains `Clone`.
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip) /
    /// [`rich_tooltip`](Self::rich_tooltip) /
    /// [`rich_tooltip_content`](Self::rich_tooltip_content) — last call wins.
    pub fn composite_tooltip(mut self, factory: impl Fn() -> Box<dyn Widget> + 'static) -> Self {
        self.composite_tooltip_factory = Some(Rc::new(factory));
        self.tooltip = None;
        self.rich_tooltip_source = None;
        self
    }

    /// Disable this segment: not selectable via click or keyboard,
    /// dimmed, and announced disabled to assistive tech.
    ///
    /// Accepts a `bool` or a `Signal<bool>` — a bound signal flips the
    /// segment live, with **no rebuild**, and keyboard stepping honours
    /// the new value immediately (the flags are read at event time, not
    /// snapshotted at build time).
    pub fn disabled(mut self, disabled: impl Into<Prop<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Hide this segment entirely: it leaves the strip, the overflow
    /// menu, the keyboard order, and the accessibility tree, and it is
    /// excluded from the overflow calculation.
    ///
    /// Distinct from *overflowed* — an overflowed segment is still
    /// reachable from the chevron menu, a hidden one is not there at all.
    /// Accepts a `bool` or a `Signal<bool>`; a bound signal re-runs the
    /// overflow plan with no rebuild.
    pub fn visible(mut self, visible: impl Into<Prop<bool>>) -> Self {
        self.visible = visible.into();
        self
    }
}

/// Label-only convenience: `tr!(day())` / `lit!("Off")` flow straight
/// into `.segment(...)` / `.segments([...])` without `Segment::new`.
impl From<LocalizedString> for Segment {
    fn from(label: LocalizedString) -> Self {
        Segment::new(label)
    }
}

impl std::fmt::Debug for Segment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Segment")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("has_icon", &self.icon.is_some())
            .field("disabled", &self.disabled.get())
            .field("visible", &self.visible.get())
            .finish()
    }
}

/// Derive a `Switcher`-compatible index from a keyed selection.
///
/// `SegmentedControl` is keyed precisely so that a contributed segment
/// cannot silently re-point the selection, but `Switcher` is index-driven
/// — this is the adapter between the two. Unknown or absent ids resolve
/// to `0`, matching `Switcher`'s own out-of-range behaviour.
///
/// ```ignore
/// Switcher::new(segmented_control::index_signal(&view, &[LIST, GRID, COLUMNS]))
///     .child(list_pane)
///     .child(grid_pane)
///     .child(columns_pane)
/// ```
pub fn index_signal(selected: &Signal<Option<SegmentId>>, ids: &[SegmentId]) -> Signal<usize> {
    let ids: Rc<Vec<SegmentId>> = Rc::new(ids.to_vec());
    selected.map(move |current| {
        current
            .and_then(|id| ids.iter().position(|&candidate| candidate == id))
            .unwrap_or(0)
    })
}

/// A segmented control binding a `Signal<Option<SegmentId>>` to a row of
/// mutually exclusive segments. Build the segment list with
/// [`segment`](Self::segment) or [`segments`](Self::segments).
pub struct SegmentedControl {
    /// Segment descriptors. Retained (cloned, not consumed, into cells on
    /// each build) so the control is rebuild-safe and so `layout_response`
    /// / `accessibility` can read labels even when measured while dormant.
    segments: Vec<Segment>,
    /// The public, keyed selection.
    selected: Signal<Option<SegmentId>>,
    /// Optional positional mirror installed by [`indexed`](Self::indexed).
    /// Addresses the **declared** list, so hiding a segment does not
    /// renumber it under the app's feet.
    index_mirror: Option<Signal<usize>>,
    /// Private index mirror over the **live** segment list, kept in
    /// bidirectional sync with `selected` at build time.
    ///
    /// Every internal interactive path — cell taps, AT clicks, arrow
    /// keys, overflow-menu rows — writes *only* this. `selected` is
    /// written only by the app and by the index→id effect. A second
    /// direct writer of `selected` reintroduces the two-writer race the
    /// `TabBar` bridge exists to avoid.
    index: Signal<usize>,
    /// Enabled state, static or reactive; forwarded to the arena at
    /// build time.
    enabled: Prop<bool>,
    /// Accessible name for the group.
    label: Option<LocalizedString>,
    /// Live segment index under the pointer, if any.
    hovered_segment: Signal<Option<usize>>,
    /// Raw keyboard/pointer focus (any modality). The keyboard-only focus
    /// ring and the focus-driven selected-segment accent fill are derived
    /// live from this × the input-modality signal in `build()`
    /// (`:focus-visible`).
    focused: Signal<bool>,
    /// Per-call override for the chrome.
    style_override: Option<SharedSegmentedControlStyle>,
    /// Per-call override for every segment's label text style (font, size,
    /// weight). `None` ⇒ the default `TextStyleRole::Small`. Text *color*
    /// stays state-driven (selected → `OnAccent`, disabled → `Disabled`)
    /// and is intentionally not overridable.
    label_style: Option<teksilo_core::color_prop::TextStyleProp>,
    display: SegmentDisplay,
    sizing: SegmentSizing,
    overflow_mode: SegmentOverflow,
    fill_width: bool,
    on_change: Option<Rc<dyn Fn(SegmentId, &mut EventContext)>>,

    // ── Build-time state ────────────────────────────────────────────
    /// Declaration indices of the segments whose `visible` prop is true,
    /// resolved once per build.
    live: Vec<usize>,
    /// Ids of the live segments, parallel to `live`.
    live_ids: Vec<SegmentId>,
    /// One cell per live segment, parallel to `live`.
    cell_ids: Vec<WidgetId>,
    /// Currently-active cell ids, for `push_to_radio_group`. Shared with
    /// the cells and refreshed from `place_children`.
    group_ids: Rc<RefCell<Vec<WidgetId>>>,
    /// Per-live-segment overflow flags, published from `place_children`.
    /// Seeded all-false at build time: the framework polls every
    /// `visible_when` prop on the *first* pass, before any plan exists.
    overflowed: Signal<Vec<bool>>,
    is_overflowing: Signal<bool>,
    /// Resolved slot geometry handed to the chrome.
    slots: SegmentSlots,
    /// Sticky promotion: the segment forced into the last slot. Plain
    /// `Cell` (not a `Signal`) so mutating it from `place_children`
    /// dirties nothing.
    promoted: Cell<Option<SegmentId>>,
    /// Equality guard for the published plan — without it every layout
    /// pass would re-dirty the visibility props and the tree would never
    /// go quiet.
    last_plan: RefCell<Plan>,
    chrome_id: Option<WidgetId>,
    chevron_id: Option<WidgetId>,
    /// Build-time children — chrome first (back), then one `SegmentCell`
    /// per live segment, then the overflow trigger.
    children: Vec<WidgetId>,
}

impl SegmentedControl {
    /// Create an empty segmented control bound to `selected`. Add segments
    /// with [`segment`](Self::segment) or [`segments`](Self::segments).
    pub fn new(selected: Signal<Option<SegmentId>>) -> Self {
        Self {
            segments: Vec::new(),
            selected,
            index_mirror: None,
            index: Signal::new(0),
            enabled: Prop::Static(true),
            label: None,
            hovered_segment: Signal::new(None),
            focused: Signal::new(false),
            style_override: None,
            label_style: None,
            display: SegmentDisplay::default(),
            sizing: SegmentSizing::default(),
            overflow_mode: SegmentOverflow::default(),
            fill_width: true,
            on_change: None,
            live: Vec::new(),
            live_ids: Vec::new(),
            cell_ids: Vec::new(),
            group_ids: Rc::new(RefCell::new(Vec::new())),
            overflowed: Signal::new(Vec::new()),
            is_overflowing: Signal::new(false),
            slots: SegmentSlots::new(),
            promoted: Cell::new(None),
            last_plan: RefCell::new(Plan::default()),
            chrome_id: None,
            chevron_id: None,
            children: Vec::new(),
        }
    }

    /// Bind a **positional** `Signal<usize>` instead of a keyed
    /// selection, mirrored in both directions.
    ///
    /// Use this only when position *is* the meaning and the segment list
    /// is closed and local — an enum discriminant over a fixed `ALL`
    /// array, a `Switcher` index, a settings choice. For anything else
    /// prefer [`new`](Self::new): an index silently stops meaning the
    /// same thing the moment a segment is inserted ahead of it, which is
    /// the entire reason selection is keyed. A persisted selection, or
    /// segments contributed by another crate, are both firmly in
    /// "anything else".
    ///
    /// Positions address the **declared** list, so a segment hidden with
    /// [`Segment::visible`] does not renumber the others.
    ///
    /// ```ignore
    /// // `bucket_idx` already drives the rollup maths and a Switcher.
    /// SegmentedControl::indexed(bucket_idx.clone())
    ///     .segments([lit!("×2"), lit!("×4"), lit!("×8")])
    /// ```
    pub fn indexed(index: Signal<usize>) -> Self {
        let mut control = Self::new(Signal::new(None));
        control.index_mirror = Some(index);
        control
    }

    /// Append one segment. Accepts a [`Segment`] or, via
    /// `From<LocalizedString>`, a bare `tr!(...)` / `lit!(...)` label
    /// (which gets a freshly allocated [`SegmentId`]).
    pub fn segment(mut self, segment: impl Into<Segment>) -> Self {
        self.segments.push(segment.into());
        self
    }

    /// Append several segments. Label-only:
    /// `.segments([tr!(day()), tr!(week())])`; rich:
    /// `.segments([Segment::new(...).id(DAY).icon(...), ...])`.
    pub fn segments(mut self, segments: impl IntoIterator<Item = impl Into<Segment>>) -> Self {
        self.segments.extend(segments.into_iter().map(Into::into));
        self
    }

    /// The ids of the segments added so far, in declaration order.
    /// Convenient for feeding [`index_signal`] without repeating the list.
    pub fn segment_ids(&self) -> Vec<SegmentId> {
        self.segments.iter().map(|s| s.id).collect()
    }

    /// Set the enabled state, statically or reactively. Forwarded to
    /// the arena at build time via
    /// `ctx.enabled_when(segmented_control_id, self.enabled.clone())`.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Accessible name for the group — e.g. "View mode". Screen readers
    /// announce it before the selected segment. Matches
    /// [`RadioGroup::label`](crate::radio_group::RadioGroup::label) and
    /// [`RadioTileGroup::label`](crate::radio_tile_group::RadioTileGroup::label).
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Called whenever the user changes the selection — by click, arrow
    /// key, assistive technology, or the overflow menu. Receives the
    /// newly selected [`SegmentId`] and an `EventContext`, so it can do
    /// things a bare `Signal` write cannot (`ctx.set_locale(...)`,
    /// `ctx.send_intent(...)`, opening a window).
    ///
    /// Does **not** fire for programmatic writes to the bound signal —
    /// there is no event in flight to carry. Observe the signal for that.
    pub fn on_change(mut self, f: impl Fn(SegmentId, &mut EventContext) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    /// Per-call override for the segmented-control chrome.
    pub fn style(mut self, style: impl teksilo_core::styles::SegmentedControlStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    /// Override every segment's label text style (font, size, weight).
    /// Accepts a `TextStyleRole`, a `TextStyle`, or a `Signal` of either.
    /// Default (unset) is `TextStyleRole::Small`. Text color stays
    /// state-driven and is intentionally not overridable here.
    pub fn text_style(mut self, style: impl Into<teksilo_core::color_prop::TextStyleProp>) -> Self {
        self.label_style = Some(style.into());
        self
    }

    /// What each segment paints: its icon, its label, or both. See
    /// [`SegmentDisplay`]. Icon-only fits far more segments, so it is
    /// worth reaching for *before* the control starts overflowing.
    pub fn display(mut self, display: SegmentDisplay) -> Self {
        self.display = display;
        self
    }

    /// How the visible segments divide the width. See [`SegmentSizing`].
    pub fn sizing(mut self, sizing: SegmentSizing) -> Self {
        self.sizing = sizing;
        self
    }

    /// What to do when the segments do not fit. See [`SegmentOverflow`].
    pub fn overflow(mut self, mode: SegmentOverflow) -> Self {
        self.overflow_mode = mode;
        self
    }

    /// Reactive "some segments are in the overflow menu right now".
    ///
    /// Republished from `place_children` behind an equality guard, so it
    /// is safe for `RepaintOnly` / `AccessibilityOnly` consumers and for
    /// `Relayout` consumers that do not feed back into this control's own
    /// width. Mirrors [`Toolbar::is_overflowing`](crate::toolbar::Toolbar::is_overflowing).
    pub fn is_overflowing(&self) -> Signal<bool> {
        self.is_overflowing.clone()
    }

    /// Whether the control claims all the width offered to it (the
    /// default, and the behaviour before this knob existed) or hugs its
    /// segments.
    ///
    /// `false` also makes the control *shrinkable*: in an over-constrained
    /// stack it compresses — and overflows — instead of spilling past its
    /// bounds.
    pub fn fill_width(mut self, fill: bool) -> Self {
        self.fill_width = fill;
        self
    }

    /// Inset-by-focus-ring-envelope bounds — the actual frame /
    /// segment-grid area. Mirrors the recipe's compute_visual so
    /// children land where the chrome paints.
    fn compute_visual(bounds: Rect, theme: &teksilo_core::Theme) -> Rect {
        let envelope = theme.shape.focus_ring_offset + theme.shape.focus_ring_width;
        Rect::new(
            bounds.x + envelope,
            bounds.y + envelope,
            (bounds.width - envelope * 2.0).max(0.0),
            (bounds.height - envelope * 2.0).max(0.0),
        )
    }

    /// The grid area inside the frame's stroke.
    fn compute_inner(visual: Rect) -> Rect {
        let bw = SEGMENTED_CONTROL_BORDER_WIDTH;
        Rect::new(
            visual.x + bw,
            visual.y + bw,
            (visual.width - bw * 2.0).max(0.0),
            (visual.height - bw * 2.0).max(0.0),
        )
    }

    /// Measure every live cell's intrinsic width, plus the chevron's.
    ///
    /// Uses [`LayoutContext::measure_intrinsic`], which measures even
    /// **dormant** widgets — the segments that overflowed into the menu
    /// still have to report a width, or the control could never work out
    /// when they fit again.
    fn measure(&self, ctx: &LayoutContext) -> (Vec<f32>, f32, f32) {
        let probe = SizeProposal::unspecified();
        let mut widths = Vec::with_capacity(self.cell_ids.len());
        let mut tallest = 0.0_f32;
        for &id in &self.cell_ids {
            let size = ctx
                .measure_intrinsic(id, probe)
                .unwrap_or(Size::new(0.0, 0.0));
            widths.push(size.width);
            tallest = tallest.max(size.height);
        }
        let chevron = self
            .chevron_id
            .and_then(|id| ctx.measure_intrinsic(id, probe))
            .map(|s| s.width)
            .unwrap_or(0.0);
        (widths, chevron, tallest)
    }

    /// Run the overflow plan for `inner_width`, applying and maintaining
    /// the sticky promotion.
    ///
    /// Pure apart from `promoted`: the two `plan` calls share one
    /// measurement pass, and the second only happens when the selection
    /// would otherwise have been hidden.
    fn resolve_plan(&self, inner_width: f32, natural: &[f32], chevron: f32) -> Plan {
        let compress = self.overflow_mode == SegmentOverflow::Compress;
        let live_count = natural.len();
        if live_count == 0 {
            return Plan::default();
        }
        let promoted_index = self
            .promoted
            .get()
            .and_then(|id| self.live_ids.iter().position(|&candidate| candidate == id));

        let mut plan = overflow::plan(
            inner_width,
            natural,
            promoted_index,
            chevron,
            self.sizing,
            compress,
        );

        // The invariant: the selected segment is always on the strip. If
        // the plan hid it, promote it and re-plan — once; the re-planned
        // `must` is by construction satisfiable, because `plan` keeps at
        // least the forced segment.
        let selected = self.index.get().min(live_count - 1);
        if !plan.is_visible(selected) {
            self.promoted.set(Some(self.live_ids[selected]));
            plan = overflow::plan(
                inner_width,
                natural,
                Some(selected),
                chevron,
                self.sizing,
                compress,
            );
        }

        // Forget the promotion once everything fits, so a later, unrelated
        // narrowing starts from clean declaration order rather than
        // resurrecting a pick the user made minutes ago.
        if !plan.show_chevron {
            self.promoted.set(None);
        }
        plan
    }

    /// Next selectable live index in `dir` (true = forward), wrapping and
    /// skipping disabled segments. Returns `current` if no other segment
    /// is enabled.
    ///
    /// Reads the disabled flags **live** — they are `Prop<bool>`s that an
    /// app may flip through a bound signal with no rebuild, so a snapshot
    /// taken at build time would go stale.
    fn step_selection(current: usize, forward: bool, disabled: &[Prop<bool>]) -> usize {
        let n = disabled.len();
        if n == 0 {
            return current;
        }
        let mut i = current;
        for _ in 0..n {
            i = if forward {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            };
            if !disabled[i].get() {
                return i;
            }
        }
        current
    }

    /// First / last enabled live index, for Home / End.
    fn edge_selection(current: usize, last: bool, disabled: &[Prop<bool>]) -> usize {
        let n = disabled.len();
        if n == 0 {
            return current;
        }
        let found = if last {
            (0..n).rev().find(|i| !disabled[*i].get())
        } else {
            (0..n).find(|i| !disabled[*i].get())
        };
        found.unwrap_or(current)
    }
}

impl std::fmt::Debug for SegmentedControl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SegmentedControl")
            .field("segments", &self.segments.len())
            .field("live", &self.live.len())
            .field("selected", &self.selected.get())
            .field("enabled", &self.enabled.get())
            .finish()
    }
}

impl Widget for SegmentedControl {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Forward the enabled state to the arena; see IconButton.
        ctx.enabled_when(self_id, self.enabled.clone());
        let effective_enabled = ctx.effective_enabled_signal(self_id);

        // ── Live segment list ───────────────────────────────────────
        //
        // Hiding a segment is a *structural* change, not a resize: it
        // renumbers the live list the index mirror addresses. Bind
        // `visible` at `Rebuild` so the whole bridge is rebuilt
        // consistently; the keyed selection survives it, which is
        // precisely why the public signal is keyed.
        {
            let registry = ctx.binding_registry();
            for segment in &self.segments {
                segment.visible.register_if_bound(
                    self_id,
                    registry,
                    teksilo_core::binding::BindingLevel::Rebuild,
                );
            }
        }
        self.live = (0..self.segments.len())
            .filter(|&i| self.segments[i].visible.get())
            .collect();
        self.live_ids = self.live.iter().map(|&i| self.segments[i].id).collect();
        let live_count = self.live.len();

        // ── Optional positional mirror (`indexed`) ──────────────────
        //
        // Seeded before the keyed sync below, so that sync sees an id it
        // can resolve. Declared positions, not live ones. This is the one
        // sanctioned second writer of `selected`; every hop is
        // equality-guarded, so the cycle
        // mirror → selected → index → selected settles in one round.
        if let Some(mirror) = self.index_mirror.clone() {
            let declared: Vec<SegmentId> = self.segments.iter().map(|s| s.id).collect();
            let from_position = |position: usize| declared.get(position).copied();

            if self.selected.get().is_none_or(|id| !declared.contains(&id)) {
                self.selected.set(from_position(mirror.get()));
            }

            {
                let declared = declared.clone();
                let selected = self.selected.clone();
                ctx.effect(&mirror, move |position| {
                    let target = declared.get(*position).copied();
                    if target.is_some() && selected.get() != target {
                        selected.set(target);
                    }
                });
            }
            {
                let declared = declared.clone();
                let mirror = mirror.clone();
                ctx.effect(&self.selected, move |maybe_id| {
                    if let Some(id) = maybe_id
                        && let Some(position) =
                            declared.iter().position(|&candidate| candidate == *id)
                        && mirror.get() != position
                    {
                        mirror.set(position);
                    }
                });
            }
        }

        // ── id ↔ index bridge (the TabBar recipe) ───────────────────
        //
        // Both directions resolve against the *live* list rebuilt above.
        // A build-time snapshot in one direction and a live lookup in the
        // other is what makes the two effects disagree after a reorder and
        // feed back unboundedly.
        let id_to_index: HashMap<SegmentId, usize> = self
            .live_ids
            .iter()
            .enumerate()
            .map(|(i, &id)| (id, i))
            .collect();

        if live_count > 0 {
            match self
                .selected
                .get()
                .and_then(|id| id_to_index.get(&id).copied())
            {
                Some(target) => {
                    if self.index.get() != target {
                        self.index.set(target);
                    }
                }
                None => {
                    // Stale or absent id: keep the previous *position*
                    // clamped into range and re-stamp the id that now
                    // lives there — the "select the neighbour" convention.
                    let clamped = self.index.get().min(live_count - 1);
                    if self.index.get() != clamped {
                        self.index.set(clamped);
                    }
                    let resolved = self.live_ids[clamped];
                    if self.selected.get() != Some(resolved) {
                        self.selected.set(Some(resolved));
                    }
                }
            }
        } else if self.selected.get().is_some() {
            self.selected.set(None);
        }

        {
            let map = id_to_index.clone();
            let index = self.index.clone();
            ctx.effect(&self.selected, move |maybe_id| {
                if let Some(id) = maybe_id
                    && let Some(&target) = map.get(id)
                    && index.get() != target
                {
                    index.set(target);
                }
            });
        }
        {
            let ids = self.live_ids.clone();
            let selected = self.selected.clone();
            ctx.effect(&self.index, move |i| {
                let resolved = ids.get(*i).copied();
                if selected.get() != resolved {
                    selected.set(resolved);
                }
            });
        }

        // Selection drives three different kinds of work, on three nodes:
        // the plan (this node, Relayout — promotion can change which
        // segments are on the strip), the announced `active_descendant`
        // (this node, AccessibilityOnly — a relayout no longer re-walks
        // the AT tree), and the chrome's fill (the chrome node, its own
        // RepaintOnly binding).
        {
            let registry = ctx.binding_registry();
            self.index.bind_to(
                self_id,
                registry,
                teksilo_core::binding::BindingLevel::Relayout,
            );
            self.index.bind_to(
                self_id,
                registry,
                teksilo_core::binding::BindingLevel::AccessibilityOnly,
            );
        }

        // Seed the overflow flags before anything can read them: the
        // framework polls every `visible_when` prop on the first layout
        // pass, which happens before this widget's `place_children` has
        // ever run.
        self.overflowed.set(vec![false; live_count]);
        self.is_overflowing.set(false);
        *self.last_plan.borrow_mut() = Plan::default();
        self.slots.publish(SegmentSlotGeometry::default());
        self.group_ids.borrow_mut().clear();

        let index = self.index.clone();
        let hovered_segment = self.hovered_segment.clone();
        // `:focus-visible`: derive the keyboard/pointer origin live from the
        // input-modality signal (true after a key event, false after
        // pointer-down) rather than snapshotting hover at focus time. The
        // chrome reads `Some(_)` for the selected-segment accent fill (any
        // focus) and `Some(Keyboard)` for the focus ring, so this keeps the
        // fill on a click while making the ring keyboard-only.
        let focused = self.focused.clone();
        let focus_origin = self.focused.zip(&ctx.focus_visible()).map(|(f, v)| {
            if !*f {
                None
            } else if *v {
                Some(FocusOrigin::Keyboard)
            } else {
                Some(FocusOrigin::Pointer)
            }
        });

        // One funnel for every internal selection write, so `on_change`
        // fires exactly once per user-driven change and the index mirror
        // stays the single write target.
        let select: Rc<dyn Fn(usize, &mut EventContext)> = {
            let index = index.clone();
            let ids = self.live_ids.clone();
            let on_change = self.on_change.clone();
            Rc::new(move |target, ctx| {
                if index.get() == target {
                    return;
                }
                index.set(target);
                if let Some(callback) = &on_change
                    && let Some(id) = ids.get(target).copied()
                {
                    callback(id, ctx);
                }
            })
        };

        // Build chrome leaf first (so it sits at index 0 in `children`
        // and paints behind the segment cells).
        let style: SharedSegmentedControlStyle = self
            .style_override
            .clone()
            .or_else(|| ctx.theme().style_slots.segmented_control.clone())
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeSegmentedControlStyle::default()));
        let chrome_id = style.make_body(
            &SegmentedControlStyleConfig {
                slots: self.slots.clone(),
                selected: index.clone(),
                hovered_segment: hovered_segment.clone(),
                focus_origin: focus_origin.clone(),
                is_enabled: effective_enabled.clone(),
            },
            ctx,
        );
        self.chrome_id = Some(chrome_id);

        self.children.clear();
        self.children.push(chrome_id);
        self.cell_ids.clear();

        for (live_index, &segment_index) in self.live.iter().enumerate() {
            let segment = &self.segments[segment_index];
            let id = ctx.add(SegmentCell {
                label: segment.label.clone(),
                icon: segment.icon.clone(),
                tooltip: segment.tooltip.clone(),
                rich_tooltip_source: segment.rich_tooltip_source.clone(),
                composite_tooltip_factory: segment.composite_tooltip_factory.clone(),
                label_style: self.label_style.clone(),
                display: self.display,
                disabled: segment.disabled.clone(),
                index: live_index,
                live_count,
                selected: index.clone(),
                hovered_segment: hovered_segment.clone(),
                focus_origin: focus_origin.clone(),
                group_ids: self.group_ids.clone(),
                select: select.clone(),
                content_id: None,
            });
            self.cell_ids.push(id);
            self.children.push(id);
        }

        // Gate each cell on "not overflowed". Fail open on a short flag
        // vector so the very first poll — which happens before any plan
        // exists — reads as visible rather than panicking.
        for (live_index, &cell_id) in self.cell_ids.iter().enumerate() {
            let flags = self.overflowed.clone();
            let on_strip = flags.map(move |f| f.get(live_index).copied() != Some(true));
            ctx.visible_when(cell_id, on_strip);
        }

        // Overflow trigger. Built unconditionally (so it can be measured
        // while dormant) but only *shown* while something has overflowed,
        // so it never reserves width it does not need.
        if live_count > 0 && self.overflow_mode == SegmentOverflow::Menu {
            let chevron_id = overflow::build_overflow_trigger(
                ctx,
                &self.segments,
                &self.live,
                &index,
                &self.overflowed,
                OVERFLOW_ICON_SIZE,
                select.clone(),
            );
            ctx.visible_when(chevron_id, self.is_overflowing.clone());
            self.chevron_id = Some(chevron_id);
            self.children.push(chevron_id);
        } else {
            self.chevron_id = None;
        }

        // Framework gates events on `arena.is_enabled`; focus walker
        // skips disabled subtrees.
        let mut handlers = HandlerSet::new()
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        // Hover-out on the parent clears the segment highlight when the
        // pointer leaves the control entirely.
        {
            let hovered_segment = hovered_segment.clone();
            handlers = handlers.on_hover(move |entered, _ctx| {
                if !entered {
                    hovered_segment.set(None);
                }
            });
        }

        // Live disabled flags, in live order. Held as `Prop`s and read at
        // event time: an app may flip a bound signal with no rebuild, and
        // a `Vec<bool>` snapshotted here would silently go stale.
        let disabled: Rc<Vec<Prop<bool>>> = Rc::new(
            self.live
                .iter()
                .map(|&i| self.segments[i].disabled.clone())
                .collect(),
        );

        // Arrow keys cycle selection, Home/End jump to the ends, both
        // skipping disabled segments. Focus stays on the control.
        {
            let index = index.clone();
            let disabled = disabled.clone();
            let select = select.clone();
            let cell_ids = self.cell_ids.clone();
            handlers = handlers.on_key(move |event, ctx: &mut EventContext| {
                if live_count == 0 {
                    return EventResponse::Ignored;
                }
                let WidgetEvent::KeyDown { key, .. } = event else {
                    return EventResponse::Ignored;
                };
                // Resolve direction at *event* time, so a locale flip
                // re-maps the arrows with no rebuild.
                let (previous, next) = if ctx.is_rtl() {
                    (Key::ArrowRight, Key::ArrowLeft)
                } else {
                    (Key::ArrowLeft, Key::ArrowRight)
                };
                let current = index.get().min(live_count - 1);
                let target = if *key == next {
                    Self::step_selection(current, true, &disabled)
                } else if *key == previous {
                    Self::step_selection(current, false, &disabled)
                } else if *key == Key::Home {
                    Self::edge_selection(current, false, &disabled)
                } else if *key == Key::End {
                    Self::edge_selection(current, true, &disabled)
                } else {
                    return EventResponse::Ignored;
                };
                if target != current {
                    select(target, ctx);
                    // Reveal the newly selected segment in any enclosing
                    // scroll area — an AT/keyboard move does not shift
                    // focus, so the framework's focus-follow cannot.
                    if let Some(&id) = cell_ids.get(target) {
                        ctx.ensure_widget_visible(id);
                    }
                }
                EventResponse::Handled
            });
        }

        // Focus handler. Track raw focus only; the keyboard/pointer
        // distinction (for the ring and the selected-segment accent fill) is
        // derived live from the input-modality signal in `build()`
        // (`:focus-visible`), so clicking to focus then pressing a key
        // reveals the ring.
        {
            let focused = focused.clone();
            handlers = handlers.on_focus(move |gained, _ctx| {
                focused.set(gained);
            });
        }

        // Access actions — increment/decrement cycle selection (skipping
        // disabled segments).
        {
            let index = index.clone();
            let disabled = disabled.clone();
            let select = select.clone();
            let cell_ids = self.cell_ids.clone();
            handlers = handlers.on_access_action(move |action, ctx: &mut EventContext| {
                if live_count == 0 {
                    return EventResponse::Ignored;
                }
                let current = index.get().min(live_count - 1);
                let target = if action == teksilo_core::accesskit::Action::Increment {
                    Self::step_selection(current, true, &disabled)
                } else if action == teksilo_core::accesskit::Action::Decrement {
                    Self::step_selection(current, false, &disabled)
                } else {
                    return EventResponse::Ignored;
                };
                if target != current {
                    select(target, ctx);
                    if let Some(&id) = cell_ids.get(target) {
                        ctx.ensure_widget_visible(id);
                    }
                }
                EventResponse::Handled
            });
        }

        ctx.apply_self_handlers(handlers);

        self.children.clone()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        let envelope = ctx.theme.shape.focus_ring_offset + ctx.theme.shape.focus_ring_width;
        let chrome = envelope * 2.0 + SEGMENTED_CONTROL_BORDER_WIDTH * 2.0;

        // Real measurement, not a per-character guess: this is what makes
        // a control in an `HStack` claim the width its labels actually
        // need, and what the overflow plan is calibrated against.
        let (natural, chevron, tallest) = self.measure(ctx);
        let content_width: f32 = match self.sizing {
            SegmentSizing::Uniform => {
                let widest = natural.iter().copied().fold(0.0_f32, f32::max);
                widest * natural.len() as f32
            }
            SegmentSizing::Fit => natural.iter().sum(),
        };
        let natural_width = content_width + chrome;
        // One ellipsized segment plus the chevron: the narrowest the
        // control can be and still mean something.
        let min_width = SEGMENTED_CONTROL_PADDING_HORIZONTAL * 2.0 + chevron + chrome;

        // The content height is measured, not assumed, so a 200 % global
        // text scale grows the control instead of clipping its labels.
        let visual_height = (tallest.max(FALLBACK_LINE_HEIGHT)
            + SEGMENTED_CONTROL_PADDING_VERTICAL * 2.0)
            .max(SEGMENTED_CONTROL_HEIGHT);
        let height = visual_height + envelope * 2.0;

        if self.fill_width {
            Size::new(proposal.width.unwrap_or(natural_width), height).into()
        } else {
            LayoutResponse::shrinkable(
                Size::new(natural_width, height),
                Size::new(min_width.min(natural_width), height),
                1.0,
            )
        }
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        if children.is_empty() {
            return;
        }

        let visual = Self::compute_visual(bounds, ctx.theme);
        let inner = Self::compute_inner(visual);
        let (natural, chevron_width, _) = self.measure(ctx);
        let plan = self.resolve_plan(inner.width, &natural, chevron_width);

        // Reading-order offsets, mirrored onto the axis afterwards so RTL
        // needs no separate code path. "Last slot" therefore means last in
        // *reading* order — next to the chevron — in both directions.
        let rtl = ctx.is_rtl();
        let place = |offset: f32, width: f32| -> Rect {
            let x = if rtl {
                inner.x + (inner.width - offset - width)
            } else {
                inner.x + offset
            };
            Rect::new(x, inner.y, width, inner.height)
        };

        let mut slot_rects = Vec::with_capacity(plan.visible.len());
        let mut offset = 0.0_f32;
        for &width in &plan.widths {
            slot_rects.push(place(offset, width));
            offset += width;
        }
        let overflow_rect = plan
            .show_chevron
            .then(|| place(offset, (inner.width - offset).max(0.0)));

        // Publish the resolved geometry for the chrome. Read during the
        // paint that follows this very layout pass, so no binding needed.
        self.slots.publish(SegmentSlotGeometry {
            frame: visual,
            segments: slot_rects.clone(),
            order: plan.visible.clone(),
            overflow: overflow_rect,
        });

        // ── Place the children, dispatching by id ───────────────────
        //
        // The slice holds only *active* children, so an overflowed (and
        // therefore dormant) cell has no entry at all and positions do not
        // line up with `self.children`.
        let mut active_cells: Vec<WidgetId> = Vec::with_capacity(plan.visible.len());
        for placement in children.iter_mut() {
            if Some(placement.id) == self.chrome_id {
                placement.origin = bounds.origin();
                placement.size = bounds.size();
                continue;
            }
            if Some(placement.id) == self.chevron_id {
                let rect = overflow_rect.unwrap_or(Rect::new(inner.right(), inner.y, 0.0, 0.0));
                placement.origin = rect.origin();
                placement.size = rect.size();
                continue;
            }
            let Some(live_index) = self.cell_ids.iter().position(|&id| id == placement.id) else {
                continue;
            };
            match plan.slot_of(live_index) {
                Some(slot) => {
                    let rect = slot_rects[slot];
                    placement.origin = rect.origin();
                    placement.size = rect.size();
                    active_cells.push(placement.id);
                }
                None => {
                    // Overflowed on *this* pass but not yet dormant (that
                    // lands next pass). Collapse it so it does not flash
                    // over the strip in the meantime.
                    placement.origin = Point::new(inner.x, inner.y);
                    placement.size = Size::new(0.0, 0.0);
                }
            }
        }

        // Sibling relations for `push_to_radio_group`: only cells that are
        // actually on the strip, since a dormant cell emits no AccessKit
        // node and referencing its id would dangle.
        {
            let mut group = self.group_ids.borrow_mut();
            if *group != active_cells {
                *group = active_cells;
            }
        }

        // ── Publish, behind an equality guard ───────────────────────
        //
        // These writes dirty the binding registry; `process_state_changes`
        // translates them into dormancy transitions at the top of the
        // *next* layout pass. Without the guard every pass would re-dirty
        // the visibility props and the tree would never settle.
        if *self.last_plan.borrow() != plan {
            let mut flags = vec![false; natural.len()];
            for &index in &plan.overflowed {
                if let Some(slot) = flags.get_mut(index) {
                    *slot = true;
                }
            }
            self.overflowed.set(flags);
            if self.is_overflowing.get() != plan.show_chevron {
                self.is_overflowing.set(plan.show_chevron);
            }
            // A segment that overflows while hovered fires no
            // `PointerLeave`; its cell clears the shared slot from its own
            // dormancy hook, but do it here too so the chrome never paints
            // one stale frame.
            if let Some(hovered) = self.hovered_segment.get()
                && !plan.is_visible(hovered)
            {
                self.hovered_segment.set(None);
            }
            *self.last_plan.borrow_mut() = plan;
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(teksilo_core::accesskit::Role::RadioGroup);
        if let Some(name) = &self.label {
            builder.set_name(name.resolve_now());
        }
        // The set size belongs on the container, not on each item:
        // AccessKit's `size_of_set` differs from ARIA's per-item
        // `aria-setsize`, and `size_of_set_from_container` resolves an
        // item's set size by walking *up* from it.
        // `live`, not the rendered cells: a segment pushed into the
        // overflow menu is still one of the choices, so it still counts.
        if !self.live.is_empty() {
            builder.set_size_of_set(self.live.len());
        }
        let selected = self.index.get();
        if let Some(segment_index) = self.live.get(selected) {
            builder.set_value(self.segments[*segment_index].label.resolve_now());
        }
        // Roving focus: focus stays on the group, which points at the
        // selected segment. Only meaningful while that cell is on the
        // strip — an overflowed cell is dormant and has no AT node, but
        // the plan guarantees the selected one never is.
        if let Some(&cell) = self.cell_ids.get(selected)
            && self.group_ids.borrow().contains(&cell)
        {
            builder.set_active_descendant(teksilo_core::accessibility::widget_id_to_node_id(cell));
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        builder.add_action(teksilo_core::accesskit::Action::Focus);
        builder.add_action(teksilo_core::accesskit::Action::Increment);
        builder.add_action(teksilo_core::accesskit::Action::Decrement);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.children.clone()
    }
}
