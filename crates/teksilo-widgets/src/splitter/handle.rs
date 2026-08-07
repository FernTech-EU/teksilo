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

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::{Point, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::{Action, Orientation as A11yOrientation, Role};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, PointerButton, WidgetEvent};
use teksilo_core::focus::FocusOrigin;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{SharedSplitterStyle, SplitterStyleConfig};
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{Easing, Orientation};

use super::model::SplitterModel;

/// Hover-dwell total before the focus indicator is fully faded in (hold
/// then fade). Matches the old SplitView dwell so incidental crossings
/// stay unobtrusive.
const HOVER_DWELL_TOTAL: Duration = Duration::from_millis(400);
/// Fade-out duration on hover-leave.
const HOVER_FADE_OUT: Duration = Duration::from_millis(120);

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
                        focus_origin.set(Some(FocusOrigin::Pointer));
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
                        if !is_dragging.get() {
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
                        FocusOrigin::Pointer
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
            handlers = handlers.on_key(move |event, _ctx| {
                if !enabled {
                    return EventResponse::Ignored;
                }
                let WidgetEvent::KeyDown { key, .. } = event else {
                    return EventResponse::Ignored;
                };

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

                // `grow` = pane i takes space from pane i+1.
                let grow = match (orientation, key) {
                    (Orientation::Horizontal, Key::ArrowRight) => Some(true),
                    (Orientation::Horizontal, Key::ArrowLeft) => Some(false),
                    (Orientation::Vertical, Key::ArrowDown) => Some(true),
                    (Orientation::Vertical, Key::ArrowUp) => Some(false),
                    (_, Key::Home) => {
                        commit_resize(&model, i, lo, pair);
                        focus_origin.set(Some(FocusOrigin::Keyboard));
                        return EventResponse::Handled;
                    }
                    (_, Key::End) => {
                        commit_resize(&model, i, hi, pair);
                        focus_origin.set(Some(FocusOrigin::Keyboard));
                        return EventResponse::Handled;
                    }
                    _ => None,
                };
                let Some(grow) = grow else {
                    return EventResponse::Ignored;
                };
                let target = if grow { size_i + step } else { size_i - step };
                let new_i = if lo > hi {
                    pair * 0.5
                } else {
                    target.clamp(lo, hi)
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
