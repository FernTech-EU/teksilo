// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `DockResizeHandle` — the draggable divider between a side's content region
//! and the centre. Unlike a [`SplitterHandle`](crate::splitter), it negotiates
//! a *single* side size against the centre's slack rather than a pair of
//! panes, so it is its own widget. It reuses the Splitter's window-absolute
//! anti-jump drag math (recover the window position via the handle's own
//! `self_bounds`, then map into the stable container) and emits a
//! `Role::Splitter` accessibility node.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::{Point, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
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

use super::geometry::DockSide;
use super::model::DockingModel;

/// Keyboard resize step (px) and drag-past-min snap-to-hide threshold (px).
const KEYBOARD_STEP: f32 = 16.0;
const SNAP_OFFSET: f32 = 30.0;
/// Hover-progress animation timings — mirror the Splitter handle so the dock
/// divider feels identical.
const HOVER_DWELL_TOTAL: Duration = Duration::from_millis(400);
const HOVER_FADE_OUT: Duration = Duration::from_millis(120);

pub(super) struct DockResizeHandleConfig {
    pub side: DockSide,
    pub model: DockingModel,
    pub enabled: bool,
    pub is_rtl: bool,
    pub container_bounds: Rc<Cell<Rect>>,
}

pub(super) struct DockResizeHandle {
    side: DockSide,
    model: DockingModel,
    enabled: bool,
    is_rtl: bool,
    container_bounds: Rc<Cell<Rect>>,
    self_bounds: Rc<Cell<Rect>>,
    is_hovered: Signal<bool>,
    /// Animated hover progress driving the shared Splitter handle body.
    hover_progress: Signal<f32>,
    is_dragging: Signal<bool>,
    focus_origin: Signal<Option<FocusOrigin>>,
    /// Pointer offset captured at press: `side_main_press − (rail + size)`.
    drag_offset: Rc<Cell<f32>>,
    body_id: Option<WidgetId>,
}

impl std::fmt::Debug for DockResizeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockResizeHandle")
            .field("side", &self.side)
            .finish()
    }
}

impl DockResizeHandle {
    pub(super) fn new(config: DockResizeHandleConfig) -> Self {
        Self {
            side: config.side,
            model: config.model,
            enabled: config.enabled,
            is_rtl: config.is_rtl,
            container_bounds: config.container_bounds,
            self_bounds: Rc::new(Cell::new(Rect::ZERO)),
            is_hovered: Signal::new(false),
            hover_progress: Signal::new_animated(0.0),
            is_dragging: Signal::new(false),
            focus_origin: Signal::new(None),
            drag_offset: Rc::new(Cell::new(0.0)),
            body_id: None,
        }
    }

    fn cursor(&self) -> CursorIcon {
        if self.side.is_horizontal_axis() {
            CursorIcon::ColResize
        } else {
            CursorIcon::RowResize
        }
    }
}

/// Distance from the side's *outer* edge to the pointer along the thickness
/// axis, recovered to window-absolute first (`self_bounds`) then mapped into
/// the stable container — the anti-jump trick from `SplitterHandle`.
fn side_main(side: DockSide, p: Point, self_bounds: Rect, container: Rect, rtl: bool) -> f32 {
    match side {
        DockSide::Leading => {
            let wx = p.x + self_bounds.x;
            if rtl {
                container.x + container.width - wx
            } else {
                wx - container.x
            }
        }
        DockSide::Trailing => {
            let wx = p.x + self_bounds.x;
            if rtl {
                wx - container.x
            } else {
                container.x + container.width - wx
            }
        }
        DockSide::Top => p.y + self_bounds.y - container.y,
        DockSide::Bottom => container.y + container.height - (p.y + self_bounds.y),
    }
}

impl Widget for DockResizeHandle {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.is_hovered.bind_to(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );
        self.is_dragging.bind_to(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );
        self.focus_origin.bind_to(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );

        // Body: the SAME visual the Splitter draws — resolved from the active
        // `SplitterStyle` (per-call override → theme slot → recipe default), so
        // the dock divider's grip, colours, sizes and hover/drag/focus states
        // are identical to a Splitter handle.
        ctx.register_animated_signal(&self.hover_progress);
        let style: SharedSplitterStyle = ctx
            .theme()
            .style_slots
            .splitter
            .clone()
            .unwrap_or_else(|| Rc::new(crate::styles::RecipeSplitterStyle::default()));
        let orientation = if self.side.is_horizontal_axis() {
            Orientation::Horizontal
        } else {
            Orientation::Vertical
        };
        let cfg = SplitterStyleConfig {
            orientation,
            is_hovered: self.is_hovered.clone(),
            is_dragging: self.is_dragging.clone(),
            is_disabled: Signal::new(!self.enabled),
            focus_origin: self.focus_origin.clone(),
            hover_progress: self.hover_progress.clone(),
        };
        let body = style.make_handle(&cfg, ctx);
        self.body_id = Some(body);

        if !self.enabled {
            ctx.apply_self_handlers(HandlerSet::new());
            return vec![body];
        }

        let side = self.side;
        // Policy: when collapsing is locked the handle still RESIZES, but its
        // hide affordances (snap-past-min, double-click, Home / Enter, AccessKit
        // Collapse) are suppressed. Show actions (End / Expand) stay.
        let allow_collapse = self.model.policy().allow_side_collapse;
        let model = self.model.clone();
        let container_bounds = self.container_bounds.clone();
        let self_bounds = self.self_bounds.clone();
        let drag_offset = self.drag_offset.clone();
        let is_dragging = self.is_dragging.clone();
        let hovered_h = self.is_hovered.clone();
        let focus_h = self.focus_origin.clone();
        let rtl = self.is_rtl;

        let mut handlers = HandlerSet::new()
            .focusable(true)
            .cursor(self.cursor())
            .on_hover({
                let hovered = hovered_h.clone();
                let hover_progress = self.hover_progress.clone();
                move |entered, _| {
                    hovered.set(entered);
                    if entered {
                        hover_progress.animate_to(1.0, HOVER_DWELL_TOTAL, Easing::Linear);
                    } else {
                        hover_progress.animate_to(0.0, HOVER_FADE_OUT, Easing::Linear);
                    }
                }
            })
            .on_focus({
                let focus = focus_h.clone();
                let hov = hovered_h.clone();
                move |gained, _| {
                    if !gained {
                        focus.set(None);
                    } else {
                        focus.set(Some(if hov.get() {
                            FocusOrigin::Pointer
                        } else {
                            FocusOrigin::Keyboard
                        }));
                    }
                }
            });

        // Drag-to-resize.
        {
            let model = model.clone();
            let container_bounds = container_bounds.clone();
            let self_bounds = self_bounds.clone();
            let drag_offset = drag_offset.clone();
            let is_dragging_h = is_dragging.clone();
            let focus = focus_h.clone();
            let drag_self_id = self_id;
            handlers =
                handlers.on_pointer_event(move |event, ctx: &mut EventContext| match event {
                    WidgetEvent::PointerDown {
                        position, button, ..
                    } => {
                        if *button != PointerButton::Primary {
                            return EventResponse::Ignored;
                        }
                        let container = container_bounds.get();
                        let main = side_main(side, *position, self_bounds.get(), container, rtl);
                        let rail = model.side_rail_thickness(side);
                        let size = model.side_size(side);
                        drag_offset.set(main - (rail + size));
                        is_dragging_h.set(true);
                        focus.set(Some(FocusOrigin::Pointer));
                        ctx.capture_pointer();
                        ctx.request_focus(drag_self_id);
                        EventResponse::Ignored
                    }
                    WidgetEvent::PointerMove { position } => {
                        if !is_dragging_h.get() {
                            return EventResponse::Ignored;
                        }
                        let container = container_bounds.get();
                        let main = side_main(side, *position, self_bounds.get(), container, rtl);
                        let rail = model.side_rail_thickness(side);
                        let new_size = main - rail - drag_offset.get();
                        let min = model.side_min_size(side);
                        if allow_collapse && new_size < min - SNAP_OFFSET {
                            model.set_side_visible_immediate(side, false);
                            is_dragging_h.set(false);
                            ctx.release_pointer();
                            return EventResponse::Handled;
                        }
                        // Collapse locked: clamp at min instead of snapping shut.
                        model.set_side_size(side, new_size.max(min));
                        EventResponse::Handled
                    }
                    WidgetEvent::PointerUp { .. } => {
                        if is_dragging_h.get() {
                            is_dragging_h.set(false);
                            ctx.release_pointer();
                        }
                        EventResponse::Ignored
                    }
                    _ => EventResponse::Ignored,
                });
        }

        // Double-click to hide (suppressed when collapsing is locked).
        if allow_collapse {
            let model = model.clone();
            handlers = handlers.on_double_tap(move |_e, _ctx| {
                model.set_side_visible(side, false);
            });
        }

        // Keyboard resize + show/hide.
        {
            let model = model.clone();
            handlers = handlers.on_key(move |event, _ctx| {
                let step = match event {
                    WidgetEvent::KeyDown { key, .. } => match (side.is_horizontal_axis(), key) {
                        (true, Key::ArrowLeft) | (false, Key::ArrowUp) => Some(-KEYBOARD_STEP),
                        (true, Key::ArrowRight) | (false, Key::ArrowDown) => Some(KEYBOARD_STEP),
                        // Home collapses, Enter toggles — both suppressed when
                        // collapsing is locked. End (show) always works.
                        (_, Key::Home) if allow_collapse => {
                            model.set_side_visible(side, false);
                            return EventResponse::Handled;
                        }
                        (_, Key::End) => {
                            model.set_side_visible(side, true);
                            return EventResponse::Handled;
                        }
                        (_, Key::Enter) if allow_collapse => {
                            model.toggle_side_visible(side);
                            return EventResponse::Handled;
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(delta) = step {
                    let min = model.side_min_size(side);
                    model.set_side_size(side, (model.side_size(side) + delta).max(min));
                    EventResponse::Handled
                } else {
                    EventResponse::Ignored
                }
            });
        }

        // AccessKit Increment / Decrement / Collapse / Expand.
        {
            let model = model.clone();
            handlers = handlers.on_access_action(move |action, _ctx| {
                use teksilo_core::accesskit::Action;
                match action {
                    Action::Increment => {
                        let min = model.side_min_size(side);
                        model.set_side_size(side, (model.side_size(side) + KEYBOARD_STEP).max(min));
                        EventResponse::Handled
                    }
                    Action::Decrement => {
                        let min = model.side_min_size(side);
                        model.set_side_size(side, (model.side_size(side) - KEYBOARD_STEP).max(min));
                        EventResponse::Handled
                    }
                    Action::Collapse if allow_collapse => {
                        model.set_side_visible(side, false);
                        EventResponse::Handled
                    }
                    Action::Expand => {
                        model.set_side_visible(side, true);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }

        ctx.apply_self_handlers(handlers);
        vec![body]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.body_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        self.self_bounds.set(bounds);
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        use teksilo_core::accesskit::{Action, Orientation as A11yOrientation, Role};
        builder.set_role(Role::Splitter);
        // A vertical divider bar (leading/trailing) emits Vertical orientation
        // (the bar axis), matching SplitterHandle.
        builder.set_orientation(if self.side.is_horizontal_axis() {
            A11yOrientation::Vertical
        } else {
            A11yOrientation::Horizontal
        });
        let container = self.container_bounds.get();
        let extent = if self.side.is_horizontal_axis() {
            container.width
        } else {
            container.height
        };
        if extent > 0.0 {
            let frac = (self.model.side_size(self.side) / extent).clamp(0.0, 1.0) * 100.0;
            builder.set_numeric_value(frac as f64);
            builder.set_min_numeric_value(0.0);
            builder.set_max_numeric_value(100.0);
            builder.set_value(format!("{frac:.0}%"));
        }
        builder.set_expanded(self.model.is_side_visible(self.side));
        if self.enabled {
            builder.add_action(Action::Focus);
            builder.add_action(Action::Increment);
            builder.add_action(Action::Decrement);
            builder.add_action(Action::Collapse);
            builder.add_action(Action::Expand);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.body_id.into_iter().collect()
    }
}
