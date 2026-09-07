// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `SplitterHandle` — one draggable divider between pane `i` and pane
//! `i+1` of an N-pane [`Splitter`](super::Splitter).
//!
//! Owns all interaction for its gap: anti-jump pointer drag, drag-past-min
//! snap-to-collapse (+ drag-back restore), double-click and keyboard
//! collapse of the adjacent collapsible pane, keyboard / AccessKit resize,
//! and the `Role::Splitter` accessibility node. The visual chrome is
//! delegated to the active [`SplitterStyle`](teksilo_core::styles::SplitterStyle);
//! this widget routes input and
//! sizes the body to the model's gutter thickness × the cross axis.
//!
//! Boundary math runs in *container-main-local* coordinates (0 at the
//! container's main-axis leading edge). For RTL horizontal splits the
//! coordinate is mirrored from the trailing edge, so the same formulas
//! work in both directions (model index 0 is always the leading pane).
//!
//! ## Reaching a 6 dp gutter with a finger
//!
//! The divider's **paint is untouched at every density**: the gutter stays
//! [`SPLITTER_GUTTER_THICKNESS`](super::SPLITTER_GUTTER_THICKNESS) dp wide and
//! the resting line the style draws inside it is painted unconditionally, so
//! there is no touch *reveal* to owe — only a touch *grab*.
//!
//! The grab is [`Widget::hit_outset`]: for a direct pointer the handle is
//! offered the press against bounds inflated across the thickness axis to the
//! density's target size (24 dp Compact, 44 dp Touch), and the arena's outset
//! pre-pass runs *before* the ordinary reverse-sibling walk, so the widened
//! gutter wins over the two panes it lies between instead of losing to
//! whichever is painted on top. For a precise pointer the outset is zero, so a
//! mouse press resolves exactly where it always did.
//!
//! Nothing about that moves layout: the panes keep every pixel they had, the
//! handle keeps its own bounds, and `place_children` is not consulted. The
//! band is a hit-test-only inflation living between the pointer and the arena.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::{EdgeInsets, Point, Rect, Size, SizeProposal};
use teksilo_core::TouchAction;
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::{Action, Orientation as A11yOrientation, Role};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, PointerButton, WidgetEvent};
use teksilo_core::focus::FocusOrigin;
use teksilo_core::signal::Signal;
use teksilo_core::styles::density::dp;
use teksilo_core::styles::{SharedSplitterStyle, SplitterStyleConfig};
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{DragActivation, Easing, InputTokens, Orientation, PointerKind, TargetRole};

use crate::common::range_nav::{self, RangeAxis, RangeKind, RangeMove};

use super::model::SplitterModel;

/// Hover-dwell total before the focus indicator is fully faded in (hold
/// then fade). Matches the old SplitView dwell so incidental crossings
/// stay unobtrusive.
const HOVER_DWELL_TOTAL: Duration = Duration::from_millis(400);
/// Fade-out duration on hover-leave.
const HOVER_FADE_OUT: Duration = Duration::from_millis(120);

/// Per-side hit inflation that lifts a `visual`-thick grip to the density's
/// target size, for a **direct** pointer only.
///
/// This is the one arithmetic the three window-chrome grips share (splitter
/// gutter, dock resize handle, window-frame resize strip): the grip keeps its
/// painted thickness, and the difference between that and
/// [`TargetRole::Target`]'s floor is split evenly across the two sides of the
/// thickness axis. 6 dp against a 24 dp Compact floor gives the 9 dp the touch
/// design's constants table names; against Touch's 44 dp floor it gives 19 dp.
///
/// Routing a 6 dp *paint* dimension through [`dp`] would be the density-rule
/// violation P20 recorded — a `Target` may only take the floor when its Compact
/// value already clears 24 dp. Nothing is painted from this: the value is a
/// hit-test outset, consumed by [`Widget::hit_outset`] and by nothing else, and
/// the grip's own box is untouched at every density.
///
/// Zero for a precise pointer: a mouse hot-spot is exact and occludes nothing,
/// so widening its targets would steal presses from the panes either side.
fn grab_outset(visual: f32, kind: PointerKind, tokens: &InputTokens) -> f32 {
    if !kind.is_direct() {
        return 0.0;
    }
    ((dp(visual, TargetRole::Target, tokens) - visual) * 0.5).max(0.0)
}

/// Grouped construction args (so `Splitter` passes the resolved style /
/// gutter / shared cells in one shot).
pub(super) struct SplitterHandleConfig {
    pub model: SplitterModel,
    pub index: usize,
    pub enabled: bool,
    pub gutter_thickness: f32,
    pub style: SharedSplitterStyle,
    /// Container bounds, written by `Splitter::place_children`.
    pub container_bounds: Rc<Cell<Rect>>,
    /// Layout direction, written by `Splitter::place_children`.
    pub is_rtl: Rc<Cell<bool>>,
    /// Effective per-pane main-axis sizes from the latest `distribute()`,
    /// written by `Splitter::place_children`. Read at event time to map a
    /// pointer position back to a boundary.
    pub layout_sizes: Rc<std::cell::RefCell<Vec<f32>>>,
    /// Effective per-gap gutter widths (0 when a neighbor is hidden), written
    /// by `Splitter::place_children`. Needed so `pre` reflects real positions.
    pub layout_gutters: Rc<std::cell::RefCell<Vec<f32>>>,
    /// `true` while both adjacent panes are visible. When false the handle is
    /// arena-disabled (Tab-skipped, event-gated) and hidden from the AT tree.
    pub active: Signal<bool>,
}

pub(super) struct SplitterHandle {
    model: SplitterModel,
    index: usize,
    enabled: bool,
    gutter_thickness: f32,
    style: SharedSplitterStyle,
    container_bounds: Rc<Cell<Rect>>,
    is_rtl: Rc<Cell<bool>>,
    layout_sizes: Rc<std::cell::RefCell<Vec<f32>>>,
    layout_gutters: Rc<std::cell::RefCell<Vec<f32>>>,
    active: Signal<bool>,
    /// This handle's own absolute bounds, written by its `place_children`.
    /// Pointer events arrive localized to *this* (moving) handle, so we add
    /// the handle's origin back to recover stable window-absolute
    /// coordinates before mapping to the container — otherwise the handle
    /// sliding under the cursor mid-drag feeds back into the size and the
    /// divider lags / jitters.
    self_bounds: Rc<Cell<Rect>>,
    is_hovered: Signal<bool>,
    is_dragging: Signal<bool>,
    focus_origin: Signal<Option<FocusOrigin>>,
    hover_progress: Signal<f32>,
    body_id: Option<WidgetId>,
}

impl SplitterHandle {
    pub(super) fn new(config: SplitterHandleConfig) -> Self {
        Self {
            model: config.model,
            index: config.index,
            enabled: config.enabled,
            gutter_thickness: config.gutter_thickness,
            style: config.style,
            container_bounds: config.container_bounds,
            is_rtl: config.is_rtl,
            layout_sizes: config.layout_sizes,
            layout_gutters: config.layout_gutters,
            active: config.active,
            self_bounds: Rc::new(Cell::new(Rect::ZERO)),
            is_hovered: Signal::new(false),
            is_dragging: Signal::new(false),
            focus_origin: Signal::new(None),
            hover_progress: Signal::new_animated(0.0),
            body_id: None,
        }
    }
}

impl std::fmt::Debug for SplitterHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplitterHandle")
            .field("index", &self.index)
            .field("enabled", &self.enabled)
            .finish()
    }
}

/// Main-axis coordinate of a pointer event, in container-main-local space
/// (0 at the leading edge; mirrored for RTL horizontal so model order is
/// preserved).
///
/// `p` is localized to *this handle's* bounds, so we add `self_bounds`'
/// origin back to recover the stable window-absolute position before
/// mapping into the (non-moving) container. Without this, the handle
/// sliding mid-drag would shift the local origin and the size would chase
/// a moving target.
fn container_main(
    p: Point,
    self_bounds: Rect,
    container: Rect,
    orientation: Orientation,
    rtl: bool,
) -> f32 {
    match orientation {
        Orientation::Horizontal => {
            let window_x = p.x + self_bounds.x;
            if rtl {
                container.x + container.width - window_x
            } else {
                window_x - container.x
            }
        }
        Orientation::Vertical => p.y + self_bounds.y - container.y,
    }
}

/// `(pre, size_i, size_{i+1}, gutter_i)` for handle `index`. `pre` is the
/// local offset of pane `index`'s leading edge, using the *actual* (possibly
/// shrunken) gutter widths so a hidden pane before this handle doesn't throw
/// the drag offset off.
fn geometry(sizes: &[f32], gutters: &[f32], index: usize) -> Option<(f32, f32, f32, f32)> {
    if index + 1 >= sizes.len() {
        return None;
    }
    let take = index.min(gutters.len());
    let pre: f32 = sizes[..index].iter().sum::<f32>() + gutters[..take].iter().sum::<f32>();
    let gut = gutters.get(index).copied().unwrap_or(0.0);
    Some((pre, sizes[index], sizes[index + 1], gut))
}

/// The adjacent pane the user's collapse gesture should target: prefer
/// restoring a collapsed collapsible neighbor, else collapse a collapsible
/// one (leading pane `i` preferred).
fn collapse_target(model: &SplitterModel, i: usize) -> Option<usize> {
    let j = i + 1;
    if model.is_collapsed(i) && model.is_collapsible(i) {
        Some(i)
    } else if model.is_collapsed(j) && model.is_collapsible(j) {
        Some(j)
    } else if model.is_collapsible(i) {
        Some(i)
    } else if model.is_collapsible(j) {
        Some(j)
    } else {
        None
    }
}

impl Widget for SplitterHandle {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // hover_progress was created outside the tree (Signal::new_animated),
        // so register it with the scheduler or animate_to no-ops.
        ctx.register_animated_signal(&self.hover_progress);

        let enabled = self.enabled;
        let orientation = self.model.orientation();
        let resize_cursor = match orientation {
            Orientation::Horizontal => CursorIcon::ColResize,
            Orientation::Vertical => CursorIcon::RowResize,
        };

        // Build the visual body via the active style.
        let cfg = SplitterStyleConfig {
            orientation,
            is_hovered: self.is_hovered.clone(),
            is_dragging: self.is_dragging.clone(),
            is_disabled: Signal::new(!enabled),
            focus_origin: self.focus_origin.clone(),
            hover_progress: self.hover_progress.clone(),
        };
        let body_id = self.style.make_handle(&cfg, ctx);
        self.body_id = Some(body_id);

        let index = self.index;
        let gutter = self.gutter_thickness;
        let model = self.model.clone();
        let container_bounds = self.container_bounds.clone();
        let is_rtl = self.is_rtl.clone();
        let layout_sizes = self.layout_sizes.clone();
        let layout_gutters = self.layout_gutters.clone();

        // Per-drag captured state (stable across the drag even if a far
        // pane animates).
        let drag_offset = Rc::new(Cell::new(0.0_f32));
        let drag_pre = Rc::new(Cell::new(0.0_f32));
        let drag_pair = Rc::new(Cell::new(0.0_f32));
        let drag_gutter = Rc::new(Cell::new(gutter));

        let mut handlers = HandlerSet::new().focusable(enabled).cursor(if enabled {
            resize_cursor
        } else {
            CursorIcon::Default
        });
        if enabled {
            // The divider's drag *is* the interaction, so a contact on it must
            // not be held back to see whether a pan develops: `NONE` forbids
            // every default touch behaviour in this subtree and `Immediate`
            // arms the drag at the slop rather than after a long press. Both
            // are direct-pointer policy — a mouse consults neither, so the
            // mouse drag below is byte-identical to what it was.
            handlers = handlers
                .touch_action(TouchAction::NONE)
                .drag_activation(DragActivation::Immediate);
        }

        // --- Pointer drag (anti-jump) + snap-to-collapse / restore ------
        {
            let model = model.clone();
            let container_bounds = container_bounds.clone();
            let is_rtl = is_rtl.clone();
            let layout_sizes = layout_sizes.clone();
            let layout_gutters = layout_gutters.clone();
            let self_bounds = self.self_bounds.clone();
            let is_dragging = self.is_dragging.clone();
            let focus_origin = self.focus_origin.clone();
            let drag_offset = drag_offset.clone();
            let drag_pre = drag_pre.clone();
            let drag_pair = drag_pair.clone();
            let drag_gutter = drag_gutter.clone();
            // Per-drag hysteresis latch: set when a collapsed neighbor is
            // pulled back open during *this* drag. While set, the normal
            // "snap shut at min−snap" rule is suspended for that pane (it
            // only re-collapses if shoved nearly all the way back), so a
            // just-restored pane doesn't immediately fall back into the
            // collapse zone. Cleared once the pane is dragged out to its
            // full min (it's then a normal expanded pane again).
            let expanding_i = Rc::new(Cell::new(false));
            let expanding_j = Rc::new(Cell::new(false));
            handlers = handlers.on_pointer_event(move |event, ctx: &mut EventContext| {
                if !enabled {
                    return EventResponse::Ignored;
                }
                match event {
                    WidgetEvent::PointerDown {
                        position, button, ..
                    } => {
                        if *button != PointerButton::Primary {
                            return EventResponse::Ignored;
                        }
                        // One divider serves one contact. A second finger
                        // landing on the gutter mid-resize is not a second
                        // resize: it would re-capture the anti-jump offsets
                        // from its own position and make the divider leap.
                        // `MultiContact::First` governs the gesture arena, not
                        // raw pointer dispatch, so the guard has to be here.
                        // A mouse is always primary, so this never fires for
                        // one.
                        if !ctx.pointer().primary || is_dragging.get() {
                            return EventResponse::Ignored;
                        }
                        let sizes = layout_sizes.borrow().clone();
                        let gutters = layout_gutters.borrow().clone();
                        let Some((pre, size_i, size_ip1, gut)) = geometry(&sizes, &gutters, index)
                        else {
                            return EventResponse::Ignored;
                        };
                        let container = container_bounds.get();
                        let rtl = is_rtl.get();
                        let p_main = container_main(
                            *position,
                            self_bounds.get(),
                            container,
                            orientation,
                            rtl,
                        );
                        let handle_center = pre + size_i + gut / 2.0;
                        drag_offset.set(p_main - handle_center);
                        drag_pre.set(pre);
                        drag_pair.set(size_i + size_ip1);
                        drag_gutter.set(gut);
                        expanding_i.set(false);
                        expanding_j.set(false);
                        is_dragging.set(true);
                        focus_origin.set(Some(FocusOrigin::Pointer(ctx.pointer_kind())));
                        ctx.capture_pointer();
                        ctx.request_focus(self_id);
                        // Return `Ignored` so the gesture arena still sees this
                        // Down and can recognize a double-tap (it would be
                        // skipped if we returned `Handled`). The arena does its
                        // own implicit capture; our side-effects above already
                        // ran. A drag's far-apart Down/Up fails the tap distance
                        // check, so it never registers as a (double-)tap.
                        EventResponse::Ignored
                    }
                    WidgetEvent::PointerMove { position } => {
                        // `owns_pointer` as well as the local flag: capturing
                        // the pointer is an arbitration act, and a handle that
                        // lost the press (a peer claimed it, the sequence was
                        // cancelled) must stop driving even though its own flag
                        // is still set.
                        if !is_dragging.get() || !ctx.owns_pointer() {
                            return EventResponse::Ignored;
                        }
                        let container = container_bounds.get();
                        let rtl = is_rtl.get();
                        let p_main = container_main(
                            *position,
                            self_bounds.get(),
                            container,
                            orientation,
                            rtl,
                        );
                        let pre = drag_pre.get();
                        let pair = drag_pair.get();
                        let raw_i = p_main - drag_offset.get() - drag_gutter.get() / 2.0 - pre;

                        let i = index;
                        let j = index + 1;
                        let min_i = model.min_size(i);
                        let min_j = model.min_size(j);
                        let snap = model.snap_offset();
                        // Pull a collapsed pane open after this much travel;
                        // re-collapse a just-restored pane only after shoving
                        // it nearly all the way back. `collapse_back <
                        // restore_out` is the hysteresis gap that prevents the
                        // open/shut oscillation.
                        let restore_out = (snap * 0.6).max(12.0);
                        let collapse_back = (snap * 0.2).max(4.0);
                        // `raw_i` is pane i's desired size; pane j's is the rest.
                        let raw_j = pair - raw_i;

                        // ---- Leading pane i --------------------------------
                        if model.is_collapsible(i) {
                            if model.is_collapsed(i) {
                                if raw_i > restore_out {
                                    model.set_collapsed_immediate(i, false);
                                    expanding_i.set(true);
                                } else {
                                    // Stay collapsed — the boundary can't move
                                    // until the pane is pulled open. Don't touch
                                    // its stored size (the restore target).
                                    return EventResponse::Handled;
                                }
                            } else if expanding_i.get() {
                                if raw_i >= min_i {
                                    expanding_i.set(false); // fully out → normal
                                } else if raw_i < collapse_back {
                                    expanding_i.set(false);
                                    model.set_collapsed_immediate(i, true);
                                    model.set_pair_sizes(i, 0.0, pair);
                                    is_dragging.set(false);
                                    ctx.release_pointer();
                                    return EventResponse::Handled;
                                }
                                // else: still restoring — suspend the min-snap.
                            } else if raw_i < min_i - snap {
                                model.set_collapsed_immediate(i, true);
                                model.set_pair_sizes(i, 0.0, pair);
                                is_dragging.set(false);
                                ctx.release_pointer();
                                return EventResponse::Handled;
                            }
                        }

                        // ---- Trailing pane j -------------------------------
                        if model.is_collapsible(j) {
                            if model.is_collapsed(j) {
                                if raw_j > restore_out {
                                    model.set_collapsed_immediate(j, false);
                                    expanding_j.set(true);
                                } else {
                                    return EventResponse::Handled;
                                }
                            } else if expanding_j.get() {
                                if raw_j >= min_j {
                                    expanding_j.set(false);
                                } else if raw_j < collapse_back {
                                    expanding_j.set(false);
                                    model.set_collapsed_immediate(j, true);
                                    model.set_pair_sizes(i, pair, 0.0);
                                    is_dragging.set(false);
                                    ctx.release_pointer();
                                    return EventResponse::Handled;
                                }
                            } else if raw_j < min_j - snap {
                                model.set_collapsed_immediate(j, true);
                                model.set_pair_sizes(i, pair, 0.0);
                                is_dragging.set(false);
                                ctx.release_pointer();
                                return EventResponse::Handled;
                            }
                        }

                        // Apply the resize. `distribute` clamps each expanded
                        // pane to its own min, so passing the raw desired sizes
                        // is enough — a restored pane sits at its min until the
                        // cursor passes it, then tracks.
                        let lo = 0.0_f32;
                        let hi = pair;
                        let new_i = raw_i.clamp(lo, hi);
                        model.set_pair_sizes(i, new_i, pair - new_i);
                        EventResponse::Handled
                    }
                    WidgetEvent::PointerUp { .. } => {
                        if is_dragging.get() {
                            is_dragging.set(false);
                            ctx.release_pointer();
                        }
                        // Always `Ignored` so the gesture arena receives the Up
                        // and can complete tap / double-tap recognition.
                        EventResponse::Ignored
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }

        // --- Double-click → toggle adjacent collapsible pane -----------
        {
            let model = model.clone();
            handlers = handlers.on_double_tap(move |_event, _ctx| {
                if !enabled {
                    return;
                }
                if let Some(t) = collapse_target(&model, index) {
                    model.toggle_collapsed(t);
                }
            });
        }

        // --- Hover (dwell-driven focus indicator) ----------------------
        {
            let is_hovered = self.is_hovered.clone();
            let hover_progress = self.hover_progress.clone();
            handlers = handlers.on_hover(move |entered, _ctx| {
                if !enabled {
                    is_hovered.set(false);
                    hover_progress.animate_to(0.0, HOVER_FADE_OUT, Easing::Linear);
                    return;
                }
                is_hovered.set(entered);
                if entered {
                    hover_progress.animate_to(1.0, HOVER_DWELL_TOTAL, Easing::Linear);
                } else {
                    hover_progress.animate_to(0.0, HOVER_FADE_OUT, Easing::Linear);
                }
            });
        }

        // --- Focus (track keyboard vs pointer origin) ------------------
        {
            let focus_origin = self.focus_origin.clone();
            let hovered = self.is_hovered.clone();
            handlers = handlers.on_focus(move |gained, _ctx| {
                if gained {
                    let origin = if hovered.get() {
                        FocusOrigin::POINTER
                    } else {
                        FocusOrigin::Keyboard
                    };
                    focus_origin.set(Some(origin));
                } else {
                    focus_origin.set(None);
                }
            });
        }

        // --- Keyboard: resize (arrows / Home / End) + Enter toggle -----
        {
            let model = model.clone();
            let layout_sizes = layout_sizes.clone();
            let layout_gutters = layout_gutters.clone();
            let focus_origin = self.focus_origin.clone();
            let is_rtl = is_rtl.clone();
            handlers = handlers.on_key(move |event, _ctx| {
                if !enabled {
                    return EventResponse::Ignored;
                }
                let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
                    return EventResponse::Ignored;
                };

                // Nothing here is the application's chord. `Enter` was
                // matched before this test, so `Ctrl+Enter` collapsed a pane
                // and reported the key handled — the one row of
                // `docs/range-keyboard.md` the divider did not honour.
                if range_nav::is_accelerator_chord(*modifiers) {
                    return EventResponse::Ignored;
                }

                // Enter toggles the adjacent collapsible pane (animated).
                if matches!(key, Key::Enter) {
                    if let Some(t) = collapse_target(&model, index) {
                        model.toggle_collapsed(t);
                        return EventResponse::Handled;
                    }
                    return EventResponse::Ignored;
                }

                let sizes = layout_sizes.borrow().clone();
                let gutters = layout_gutters.borrow().clone();
                let Some((_pre, size_i, size_ip1, _gut)) = geometry(&sizes, &gutters, index) else {
                    return EventResponse::Ignored;
                };
                let i = index;
                let j = index + 1;
                let pair = size_i + size_ip1;
                let lo = if model.is_collapsed(i) {
                    0.0
                } else {
                    model.min_size(i)
                };
                let hi = if model.is_collapsed(j) {
                    pair
                } else {
                    pair - model.min_size(j)
                };
                let step = model.keyboard_step_px();

                // A divider binds the arrows and the two edges, and no page
                // keys — the ARIA window-splitter pattern asks only for `Home`
                // and `End`, `QSplitterHandle` binds no page keys, and a
                // divider has no unit a page could be a multiple of.
                //
                // The direction comes from the cell `Splitter::place_children`
                // already writes and the drag path already reads. The pane
                // order and the drag math have been mirrored since this widget
                // shipped; the arrows were not, so `->` pulled the divider left
                // in a right-to-left window.
                let arrows = match orientation {
                    Orientation::Horizontal => RangeAxis::Horizontal,
                    Orientation::Vertical => RangeAxis::Vertical,
                };
                let Some(mv) = range_nav::range_move(
                    *key,
                    *modifiers,
                    RangeKind::Divider,
                    arrows,
                    is_rtl.get(),
                ) else {
                    return EventResponse::Ignored;
                };
                // `grow` = pane i takes space from pane i+1, which happens
                // when the divider moves *away* from pane i — trailing-ward.
                // Not `increase` directly: that is axis-relative, so on a
                // vertical splitter `Up` increases and `Down` decreases, and
                // reading it as a direction inverts the vertical pair.
                let new_i = match mv {
                    RangeMove::Step { increase } => {
                        let grow = range_nav::towards_trailing(
                            increase,
                            matches!(orientation, Orientation::Horizontal),
                        );
                        let target = if grow { size_i + step } else { size_i - step };
                        if lo > hi {
                            pair * 0.5
                        } else {
                            target.clamp(lo, hi)
                        }
                    }
                    RangeMove::ToMin => lo,
                    RangeMove::ToMax => hi,
                    // `Divider` never yields a page move; asserted in
                    // `range_nav`'s own tests.
                    RangeMove::Page { .. } => return EventResponse::Ignored,
                };
                commit_resize(&model, i, new_i, pair);
                focus_origin.set(Some(FocusOrigin::Keyboard));
                EventResponse::Handled
            });
        }

        // --- AccessKit actions: Increment / Decrement / Expand / Collapse
        {
            let model = model.clone();
            let layout_sizes = layout_sizes.clone();
            let layout_gutters = layout_gutters.clone();
            handlers = handlers.on_access_action(move |action, _ctx| {
                if !enabled {
                    return EventResponse::Ignored;
                }
                match action {
                    Action::Increment | Action::Decrement => {
                        let sizes = layout_sizes.borrow().clone();
                        let gutters = layout_gutters.borrow().clone();
                        let Some((_pre, size_i, size_ip1, _gut)) =
                            geometry(&sizes, &gutters, index)
                        else {
                            return EventResponse::Ignored;
                        };
                        let i = index;
                        let j = index + 1;
                        let pair = size_i + size_ip1;
                        let lo = if model.is_collapsed(i) {
                            0.0
                        } else {
                            model.min_size(i)
                        };
                        let hi = if model.is_collapsed(j) {
                            pair
                        } else {
                            pair - model.min_size(j)
                        };
                        let step = model.keyboard_step_px();
                        let delta = if matches!(action, Action::Increment) {
                            step
                        } else {
                            -step
                        };
                        let new_i = if lo > hi {
                            pair * 0.5
                        } else {
                            (size_i + delta).clamp(lo, hi)
                        };
                        commit_resize(&model, i, new_i, pair);
                        EventResponse::Handled
                    }
                    Action::Collapse => {
                        if let Some(t) = collapse_target(&model, index) {
                            model.set_collapsed(t, true);
                            return EventResponse::Handled;
                        }
                        EventResponse::Ignored
                    }
                    Action::Expand => {
                        if let Some(t) = collapse_target(&model, index) {
                            model.set_collapsed(t, false);
                            return EventResponse::Handled;
                        }
                        EventResponse::Ignored
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }

        ctx.apply_self_handlers(handlers);
        // Bind the hover_progress + interaction signals so the body
        // repaints; the body itself binds them in its own build, but the
        // handle drives hover_progress animation.
        let registry = ctx.binding_registry();
        self.hover_progress
            .bind_to(self_id, registry, BindingLevel::RepaintOnly);

        vec![body_id]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        match self.model.orientation() {
            Orientation::Horizontal => Size::new(
                self.gutter_thickness,
                proposal.height.unwrap_or(self.gutter_thickness),
            ),
            Orientation::Vertical => Size::new(
                proposal.width.unwrap_or(self.gutter_thickness),
                self.gutter_thickness,
            ),
        }
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Record our own absolute bounds so the drag handlers can recover
        // window-absolute pointer coordinates (events arrive localized to
        // this handle, which moves as the divider is dragged).
        self.self_bounds.set(bounds);
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.body_id.into_iter().collect()
    }

    /// The grab band, across the thickness axis only.
    ///
    /// A horizontal splitter puts its panes side by side, so its divider is a
    /// vertical bar and the axis it is thin on is the reading axis — hence
    /// `leading`/`trailing`. A vertical splitter is the transpose. The band is
    /// never applied along the bar's length: the handle already spans the whole
    /// cross axis, and inflating it there would reach into whatever sits above
    /// or below the splitter.
    ///
    /// Zero whenever the handle would refuse the press anyway — disabled, or
    /// parked because a neighbouring pane is hidden — because a widened node
    /// that then ignores the press is a hole punched in the panes behind it.
    fn hit_outset(&self, kind: PointerKind, tokens: &InputTokens) -> EdgeInsets {
        if !self.enabled || !self.active.get() {
            return EdgeInsets::ZERO;
        }
        let out = grab_outset(self.gutter_thickness, kind, tokens);
        if out <= 0.0 {
            return EdgeInsets::ZERO;
        }
        match self.model.orientation() {
            Orientation::Horizontal => EdgeInsets::new(0.0, out, 0.0, out),
            Orientation::Vertical => EdgeInsets::new(out, 0.0, out, 0.0),
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // A handle whose gutter is hidden (a neighbor is hidden) is removed
        // from the AT tree entirely — it reads as absent, like its gutter.
        if !self.active.get() {
            builder.set_hidden();
            return;
        }
        builder.set_role(Role::Splitter);
        builder.set_name(teksilo_i18n::tr_widget!(a11y_splitter_divider_name()).resolve_now());

        // Value = pane i's share of the pair, as a percent.
        let sizes = self.layout_sizes.borrow();
        let gutters = self.layout_gutters.borrow();
        let (size_i, pair) = match geometry(&sizes, &gutters, self.index) {
            Some((_pre, a, b, _g)) => (a, a + b),
            None => (0.0, 0.0),
        };
        drop(sizes);
        drop(gutters);
        let frac = if pair > 0.0 { size_i / pair } else { 0.5 };
        builder.set_numeric_value((frac * 100.0) as f64);
        builder.set_min_numeric_value(0.0);
        builder.set_max_numeric_value(100.0);
        builder.set_value(format!("{:.0}%", frac * 100.0));
        if pair > 0.0 {
            builder.set_numeric_value_step((self.model.keyboard_step_px() / pair * 100.0) as f64);
        }

        // A horizontal splitter (panes side-by-side) has a *vertical* bar.
        let handle_orientation = match self.model.orientation() {
            Orientation::Horizontal => A11yOrientation::Vertical,
            Orientation::Vertical => A11yOrientation::Horizontal,
        };
        builder.set_orientation(handle_orientation);

        // Expanded state of the adjacent collapsible pane, if any.
        let i = self.index;
        let j = self.index + 1;
        let collapsible_neighbor = if self.model.is_collapsible(i) {
            Some(i)
        } else if self.model.is_collapsible(j) {
            Some(j)
        } else {
            None
        };
        if let Some(n) = collapsible_neighbor {
            builder.set_expanded(!self.model.is_collapsed(n));
        }

        if !self.enabled {
            builder.set_disabled();
        } else {
            builder.add_action(Action::Focus);
            builder.add_action(Action::Increment);
            builder.add_action(Action::Decrement);
            if collapsible_neighbor.is_some() {
                builder.add_action(Action::Collapse);
                builder.add_action(Action::Expand);
            }
        }
    }
}

/// Set pane `i` to `new_i` and its `i+1` neighbor to the remainder of
/// `pair`, in one model mutation.
fn commit_resize(model: &SplitterModel, i: usize, new_i: f32, pair: f32) {
    let clamped = new_i.clamp(0.0, pair);
    model.set_pair_sizes(i, clamped, pair - clamped);
}

#[cfg(test)]
mod grab_tests {
    use teksilo_canvas::{Point, Size, SizeProposal};
    use teksilo_core::event::PointerButton;
    use teksilo_core::pointer::{
        BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
        PointerSample,
    };
    use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_tokens::{Orientation, TargetDensity};

    use crate::splitter::{PaneDescriptor, SPLITTER_GUTTER_THICKNESS, Splitter, SplitterModel};

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);

    impl Widget for FixedLeaf {
        fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    fn h_model(sizes: &[f32]) -> SplitterModel {
        SplitterModel::from_panes(
            sizes
                .iter()
                .map(|&s| PaneDescriptor::new().size(s).min_size(0.0).stretch(0.0))
                .collect(),
            Orientation::Horizontal,
        )
    }

    /// A 400 × 200 two-pane horizontal splitter, panes 197 dp each with the
    /// 6 dp gutter between them. Returns the tree and the root.
    ///
    /// The panes are **tappable**, which is load-bearing for every hit test
    /// here: an inert pane lets the miss-only slop pass re-attribute a nearby
    /// press to the divider all by itself, and a test over inert panes would
    /// pass with `hit_outset` deleted. A pane that owns the press is the case
    /// the outset exists for — it is the only mechanism that can *beat* a
    /// competing target rather than merely fill a vacuum.
    fn two_pane_with_tappable_panes(
        density: TargetDensity,
    ) -> (WidgetTree, teksilo_core::widget_id::WidgetId) {
        use teksilo_core::widget_builder::WidgetBuilder;
        let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
        let mut tree = WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light().with_density(density));
        let root = tree.add(
            Splitter::new(h_model(&[avail * 0.5, avail * 0.5]))
                .pane(FixedLeaf(100.0, 40.0).on_tap(|_, _| {}))
                .pane(FixedLeaf(100.0, 40.0).on_tap(|_, _| {})),
        );
        tree.layout(SizeProposal::exact(400.0, 200.0));
        (tree, root)
    }

    fn two_pane() -> (WidgetTree, teksilo_core::widget_id::WidgetId) {
        two_pane_with_tappable_panes(TargetDensity::Compact)
    }

    fn finger(raw: u64, primary: bool) -> PointerInfo {
        let id: PointerId = PointerIdAllocator::global().begin(BackendDeviceKey::new(0x5031), raw);
        let mut info = PointerInfo::touch(id, EventTime::ZERO);
        info.primary = primary;
        info
    }

    fn sample(pointer: PointerInfo, phase: PointerPhase, at: Point) -> PointerSample {
        PointerSample {
            pointer,
            phase,
            position: at,
            button: None,
            modifiers: teksilo_core::event::Modifiers::NONE,
            coalesced: Vec::new(),
        }
    }

    /// The mouse invariant: the widened band is direct-pointer-only, so a
    /// press 9 dp clear of the 6 dp gutter still lands on the pane, exactly as
    /// it did before the grab existed.
    #[test]
    fn a_mouse_press_beside_the_gutter_still_lands_on_the_pane() {
        let (tree, root) = two_pane();
        let handle = tree.child_widget(root, 1);
        let pane1 = tree.child_widget(root, 2);
        let gutter = tree.bounds(handle);
        let mouse = PointerInfo::mouse(EventTime::ZERO);

        let just_outside = Point::new(gutter.right() + 4.0, gutter.center().y);
        let hit = tree.hit_test_for(just_outside, &mouse);
        assert!(
            hit == Some(pane1) || tree.is_descendant_of(hit.unwrap(), pane1),
            "a mouse 4 dp past the gutter must reach the pane, got {hit:?}"
        );
        // And inside the gutter it still reaches the handle.
        let inside = tree.hit_test_for(gutter.center(), &mouse);
        assert!(
            inside == Some(handle) || tree.is_descendant_of(inside.unwrap(), handle),
            "a mouse inside the gutter must reach the handle, got {inside:?}"
        );
    }

    /// A finger reaches the 6 dp gutter from inside the tappable pane painted
    /// beside it — which is what "wins over the panes it overlaps" means.
    ///
    /// Probed at **Touch**, 14 dp out: the ring is 19 dp there while the
    /// miss-only slop pass reaches at most its 8 dp radius, so only the outset
    /// can answer, and the test fails if `hit_outset` is removed.
    #[test]
    fn a_finger_grabs_the_gutter_over_the_pane_beside_it() {
        let (tree, root) = two_pane_with_tappable_panes(TargetDensity::Touch);
        let handle = tree.child_widget(root, 1);
        let pane1 = tree.child_widget(root, 2);
        let gutter = tree.bounds(handle);
        let contact = finger(1, true);

        for at in [
            Point::new(gutter.right() + 14.0, gutter.center().y),
            Point::new(gutter.x - 14.0, gutter.center().y),
        ] {
            let hit = tree.hit_test_for(at, &contact);
            assert!(
                hit.is_some_and(|id| id == handle || tree.is_descendant_of(id, handle)),
                "the widened gutter must beat the pane it overlaps at {at:?}, got {hit:?}"
            );
        }
        assert!(
            tree.bounds(pane1)
                .contains(Point::new(gutter.right() + 14.0, gutter.center().y)),
            "the probe must be inside the pane, or it proves nothing"
        );
        // Past the ring the pane takes it back.
        let at = Point::new(gutter.right() + 30.0, gutter.center().y);
        let hit = tree.hit_test_for(at, &contact).unwrap();
        assert!(
            hit == pane1 || tree.is_descendant_of(hit, pane1),
            "30 dp away is outside the band, got {hit:?}"
        );
    }

    /// Hit-only: the grab moves no layout. Every pane and the gutter itself
    /// occupy the identical rectangles they would with the mechanism absent —
    /// asserted against the arithmetic, not against a snapshot, so the test
    /// still means something if the fixture changes.
    #[test]
    fn the_grab_moves_no_layout_at_any_density() {
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
            let mut tree = WidgetTree::new()
                .with_theme(teksilo_core::presets::intui::light().with_density(density));
            let root = tree.add(
                Splitter::new(h_model(&[avail * 0.5, avail * 0.5]))
                    .pane(FixedLeaf(100.0, 40.0))
                    .pane(FixedLeaf(100.0, 40.0)),
            );
            tree.layout(SizeProposal::exact(400.0, 200.0));

            let gutter = tree.bounds(tree.child_widget(root, 1));
            assert_eq!(
                gutter.width, SPLITTER_GUTTER_THICKNESS,
                "the gutter is painted 6 dp at {density:?}"
            );
            assert_eq!(tree.bounds(tree.child_widget(root, 0)).width, avail * 0.5);
            assert_eq!(tree.bounds(tree.child_widget(root, 2)).width, avail * 0.5);
        }
    }

    /// A mouse drag is byte-identical to what it was: the same press, the same
    /// two moves, the same resulting pane sizes to the last float.
    #[test]
    fn a_mouse_drag_lands_the_divider_exactly_where_it_always_did() {
        let (mut tree, root) = two_pane();
        let handle = tree.child_widget(root, 1);
        let start = tree.bounds(handle).center();
        tree.pointer_down_button(start, PointerButton::Primary);
        tree.pointer_move(Point::new(start.x + 60.0, start.y));
        tree.pointer_up_button(Point::new(start.x + 60.0, start.y), PointerButton::Primary);
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
        let w0 = tree.bounds(tree.child_widget(root, 0)).width;
        assert!(
            (w0 - (avail * 0.5 + 60.0)).abs() < 0.01,
            "pane 0 must land exactly 60 dp wider, got {w0}"
        );
    }

    /// A second finger arriving mid-resize is refused.
    ///
    /// The failure this pins is not that the intruder drives the divider —
    /// `owns_pointer` already stops its *moves* — but that its **press**
    /// recaptures the anti-jump offset from wherever it landed. The first
    /// finger's next move would then be measured against the intruder's
    /// position, and the divider would leap by the distance between them.
    #[test]
    fn a_second_contact_during_a_resize_is_ignored() {
        let avail = 400.0 - SPLITTER_GUTTER_THICKNESS;
        let (mut tree, root) = two_pane();
        let handle = tree.child_widget(root, 1);
        let start = tree.bounds(handle).center();

        let first = finger(11, true);
        tree.dispatch_pointer(sample(first, PointerPhase::Down, start));
        tree.dispatch_pointer(sample(
            first,
            PointerPhase::Move,
            Point::new(start.x + 40.0, start.y),
        ));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        // A second contact lands on the gutter — inside its grab band, so it
        // really does reach the handle — 15 dp off the divider's centre.
        let second = finger(12, false);
        let gutter = tree.bounds(handle);
        let intruder = Point::new(gutter.center().x + 6.0, gutter.center().y + 15.0);
        assert_eq!(
            tree.hit_test_for(intruder, &second)
                .map(|id| tree.is_descendant_of(id, handle) || id == handle),
            Some(true),
            "the intruder must actually reach the handle, or this proves nothing"
        );
        tree.dispatch_pointer(sample(second, PointerPhase::Down, intruder));

        // The first finger keeps going. Its offsets must be the ones it
        // captured at ITS press.
        tree.dispatch_pointer(sample(
            first,
            PointerPhase::Move,
            Point::new(start.x + 70.0, start.y),
        ));
        tree.layout(SizeProposal::exact(400.0, 200.0));

        let w0 = tree.bounds(tree.child_widget(root, 0)).width;
        assert!(
            (w0 - (avail * 0.5 + 70.0)).abs() < 0.01,
            "the divider must track the first finger, not the intruder: {w0}"
        );
    }

    /// The arithmetic itself, so a density change is a one-line diff rather
    /// than a re-derivation: 6 dp against a 24 dp Compact floor is 9 dp a side,
    /// against Touch's 44 dp floor 19 dp, and zero for a precise pointer at
    /// every density.
    #[test]
    fn the_band_is_the_target_floor_split_across_the_thickness() {
        use teksilo_tokens::{InputTokens, PointerKind};

        for (density, expected) in [
            (TargetDensity::Compact, 9.0_f32),
            (TargetDensity::Comfortable, 13.0),
            (TargetDensity::Touch, 19.0),
        ] {
            let tokens = InputTokens::for_density(density);
            assert_eq!(
                super::grab_outset(SPLITTER_GUTTER_THICKNESS, PointerKind::Touch, &tokens),
                expected,
                "{density:?}"
            );
            assert_eq!(
                super::grab_outset(SPLITTER_GUTTER_THICKNESS, PointerKind::Mouse, &tokens),
                0.0,
                "a precise pointer is never widened ({density:?})"
            );
        }
    }

    /// An already-generous grip earns nothing: `dp` is a floor, so a theme that
    /// paints a 32 dp gutter gets no outset at Compact at all.
    #[test]
    fn a_gutter_that_already_clears_the_floor_is_not_widened() {
        use teksilo_tokens::{InputTokens, PointerKind};
        let tokens = InputTokens::for_density(TargetDensity::Compact);
        assert_eq!(super::grab_outset(32.0, PointerKind::Touch, &tokens), 0.0);
    }
}
