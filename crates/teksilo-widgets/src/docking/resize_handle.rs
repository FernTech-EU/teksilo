// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `DockResizeHandle` — the draggable divider between a side's content region
//! and the centre. Unlike a [`SplitterHandle`](crate::splitter), it negotiates
//! a *single* side size against the centre's slack rather than a pair of
//! panes, so it is its own widget. It reuses the Splitter's window-absolute
//! anti-jump drag math (recover the window position via the handle's own
//! `self_bounds`, then map into the stable container) and emits a
//! `Role::Splitter` accessibility node.
//!
//! ## Reaching the divider with a finger
//!
//! The handle borrows the active [`SplitterStyle`]'s body, so it is as thin as
//! a splitter gutter — 6 dp — and it keeps that thickness at every density: a
//! dock divider that grew with the density would take its width out of the
//! panel beside it, which is content, not slack.
//!
//! The grab instead comes from [`Widget::hit_outset`], the same mechanism the
//! splitter handle uses. For a direct pointer the handle is offered the press
//! against bounds inflated across the thickness axis to the density's target
//! size (24 dp Compact, 44 dp Touch); the arena runs that pre-pass before the
//! ordinary reverse-sibling walk, so the widened band beats the side panel and
//! the centre it lies between. For a precise pointer the outset is zero and a
//! mouse press lands exactly where it did before.
//!
//! Layout is untouched: the geometry engine still hands the handle the same
//! rectangle, the side keeps its size, and nothing repaints differently.
//!
//! A second contact arriving during a resize is refused, not served: the drag
//! recovers its anti-jump offset from the pressing pointer's position, so a
//! second finger would make the divider leap to wherever it landed.
//!
//! [`SplitterStyle`]: teksilo_core::styles::SplitterStyle

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use teksilo_canvas::{EdgeInsets, Point, Rect, SizeProposal};
use teksilo_core::TouchAction;
use teksilo_core::accessibility::AccessNodeBuilder;
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

use super::geometry::DockSide;
use super::model::DockingModel;

/// Keyboard resize step (px) and drag-past-min snap-to-hide threshold (px).
const KEYBOARD_STEP: f32 = 16.0;
const SNAP_OFFSET: f32 = 30.0;
/// Hover-progress animation timings — mirror the Splitter handle so the dock
/// divider feels identical.
const HOVER_DWELL_TOTAL: Duration = Duration::from_millis(400);
const HOVER_FADE_OUT: Duration = Duration::from_millis(120);

/// Per-side hit inflation lifting a `visual`-thick grip to the density's target
/// size, for a **direct** pointer only. The dock divider's twin of the splitter
/// handle's own helper — see that file for why routing a hit outset (never a
/// paint dimension) through [`dp`] is not the density-rule violation it looks
/// like.
fn grab_outset(visual: f32, kind: PointerKind, tokens: &InputTokens) -> f32 {
    if !kind.is_direct() || !visual.is_finite() || visual <= 0.0 {
        return 0.0;
    }
    ((dp(visual, TargetRole::Target, tokens) - visual) * 0.5).max(0.0)
}

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
        let style: SharedSplitterStyle =
            ctx.theme().style_slots.splitter.clone().unwrap_or_else(|| {
                Rc::new(crate::styles::RecipeSplitterStyle::for_tokens(
                    &ctx.theme().input,
                ))
            });
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
            // The divider's drag *is* the interaction: forbid every default
            // touch behaviour in this subtree and arm the drag at the slop
            // rather than after a long press. Direct-pointer policy only — a
            // mouse reads neither, so the mouse drag is unchanged.
            .touch_action(TouchAction::NONE)
            .drag_activation(DragActivation::Immediate)
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
                            FocusOrigin::POINTER
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
                        // One divider, one contact: a second finger would
                        // recapture the anti-jump offset from its own position
                        // and make the side leap. `MultiContact::First` gates
                        // the gesture arena, not raw pointer dispatch, so the
                        // guard belongs here. A mouse is always primary.
                        if !ctx.pointer().primary || is_dragging_h.get() {
                            return EventResponse::Ignored;
                        }
                        let container = container_bounds.get();
                        let main = side_main(side, *position, self_bounds.get(), container, rtl);
                        let rail = model.side_rail_thickness(side);
                        let size = model.side_size(side);
                        drag_offset.set(main - (rail + size));
                        is_dragging_h.set(true);
                        focus.set(Some(FocusOrigin::Pointer(ctx.pointer_kind())));
                        ctx.capture_pointer();
                        ctx.request_focus(drag_self_id);
                        EventResponse::Ignored
                    }
                    WidgetEvent::PointerMove { position } => {
                        // The local flag says "I started a resize";
                        // `owns_pointer` says "and I still own the press".
                        // Capture is an arbitration act, so a handle that lost
                        // it must stop driving.
                        if !is_dragging_h.get() || !ctx.owns_pointer() {
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
            handlers = handlers.on_key(move |event, ctx| {
                let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
                    return EventResponse::Ignored;
                };

                // Nothing here is the application's chord. `Enter` was
                // matched before this test, so `Ctrl+Enter` hid the side and
                // reported the key handled — the one row of
                // `docs/range-keyboard.md` the handle did not honour.
                if range_nav::is_accelerator_chord(*modifiers) {
                    return EventResponse::Ignored;
                }

                // Enter toggles the side — suppressed when collapsing is
                // locked, as it always was.
                if matches!(key, Key::Enter) && allow_collapse {
                    model.toggle_side_visible(side);
                    return EventResponse::Handled;
                }

                // Same chord table as a `Splitter` divider: arrows and the two
                // edges, no page keys. Direction is read at event time so a
                // locale flip needs no rebuild.
                let arrows = if side.is_horizontal_axis() {
                    RangeAxis::Horizontal
                } else {
                    RangeAxis::Vertical
                };
                let Some(mv) = range_nav::range_move(
                    *key,
                    *modifiers,
                    RangeKind::Divider,
                    arrows,
                    ctx.is_rtl(),
                ) else {
                    return EventResponse::Ignored;
                };
                match mv {
                    RangeMove::Step { increase } => {
                        // Two inversions, kept apart because they are
                        // different facts. The first — that `increase` is
                        // axis-relative, so `Up` increases the value while
                        // decreasing y — belongs to every edge-anchored
                        // control and lives in `towards_trailing`.
                        //
                        // The second is this widget's own: a side grows when
                        // its handle moves *away* from the side — outward for
                        // Leading and Top, inward for Trailing and Bottom. The
                        // pointer path has always accounted for that
                        // (`side_main`); the keyboard did not, so `Right`
                        // moved a trailing handle the way it does not point.
                        // The layout direction is already folded into
                        // `increase` by `range_move`.
                        let trailing_ward =
                            range_nav::towards_trailing(increase, side.is_horizontal_axis());
                        let grows_trailing_ward = matches!(side, DockSide::Leading | DockSide::Top);
                        let delta = if trailing_ward == grows_trailing_ward {
                            KEYBOARD_STEP
                        } else {
                            -KEYBOARD_STEP
                        };
                        let min = model.side_min_size(side);
                        model.set_side_size(side, (model.side_size(side) + delta).max(min));
                        EventResponse::Handled
                    }
                    // A dock side's low extreme *is* hidden: it is the only
                    // thing "give everything to the centre" can mean for a side
                    // that collapses. With collapsing locked, `Home` falls
                    // through as it did before.
                    RangeMove::ToMin if allow_collapse => {
                        model.set_side_visible(side, false);
                        EventResponse::Handled
                    }
                    RangeMove::ToMax => {
                        model.set_side_visible(side, true);
                        EventResponse::Handled
                    }
                    // `Divider` never yields a page move; asserted in
                    // `range_nav`'s own tests.
                    RangeMove::ToMin | RangeMove::Page { .. } => EventResponse::Ignored,
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

    /// The grab band, across the thickness axis only.
    ///
    /// A leading or trailing side is divided by a vertical bar, thin on the
    /// reading axis; a top or bottom side by a horizontal one. The thickness is
    /// read from the handle's own last-placed bounds rather than from a
    /// constant, because the body comes from the active
    /// [`SplitterStyle`](teksilo_core::styles::SplitterStyle) and a theme is
    /// free to make it thicker — an already-generous grip then earns no
    /// inflation at all, which is what [`dp`]'s floor semantics give for free.
    ///
    /// Zero before the first layout (degenerate bounds) and zero for a disabled
    /// handle, which refuses the press: widening a node that then ignores the
    /// press would punch a hole in the panel behind it.
    fn hit_outset(&self, kind: PointerKind, tokens: &InputTokens) -> EdgeInsets {
        if !self.enabled {
            return EdgeInsets::ZERO;
        }
        let bounds = self.self_bounds.get();
        let horizontal_axis = self.side.is_horizontal_axis();
        let thickness = if horizontal_axis {
            bounds.width
        } else {
            bounds.height
        };
        let out = grab_outset(thickness, kind, tokens);
        if out <= 0.0 {
            return EdgeInsets::ZERO;
        }
        if horizontal_axis {
            EdgeInsets::new(0.0, out, 0.0, out)
        } else {
            EdgeInsets::new(out, 0.0, out, 0.0)
        }
    }
}

#[cfg(test)]
mod grab_tests {
    use std::time::Duration;

    use teksilo_canvas::{Point, Size, SizeProposal};
    use teksilo_core::accesskit::Role;
    use teksilo_core::pointer::{
        BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
        PointerSample,
    };
    use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
    use teksilo_core::widget_id::WidgetId;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_i18n::lit;
    use teksilo_tokens::TargetDensity;

    use crate::docking::{
        DockOpenLocation, DockSide, DockWidget, DockWidgetId, DockingLayout, DockingModel,
    };

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(&self, _p: SizeProposal, _c: &LayoutContext) -> LayoutResponse {
            Size::new(self.0, self.1).into()
        }
    }

    /// A layout with one leading dock open, settled past its reveal animation.
    ///
    /// The centre is **tappable** on purpose: over inert content the miss-only
    /// slop pass re-attributes a nearby press to the divider by itself, and a
    /// hit test written over an inert centre would pass with `hit_outset`
    /// deleted. Beating a target that owns the press is what the outset is for.
    fn leading_dock(density: TargetDensity) -> (WidgetTree, WidgetId, DockingModel) {
        use teksilo_core::widget_builder::WidgetBuilder;
        let model = DockingModel::new();
        let id = DockWidgetId::fresh();
        let dw = DockWidget::new(id, lit!("Explorer"), |_| FixedLeaf(120.0, 120.0));
        let mut tree = WidgetTree::new()
            .with_theme(teksilo_core::presets::intui::light().with_density(density));
        let root = tree.add(
            DockingLayout::new(model.clone())
                .center(FixedLeaf(200.0, 200.0).on_tap(|_, _| {}))
                .dock(dw),
        );
        model.open_dock(id, DockOpenLocation::side(DockSide::Leading));
        tree.layout(SizeProposal::exact(1000.0, 800.0));
        tree.tick_animations(Duration::from_millis(600));
        tree.layout(SizeProposal::exact(1000.0, 800.0));
        (tree, root, model)
    }

    /// Depth-first search for the `Role::Splitter` node — the dock's resize
    /// handle. The layout shape is not this test's subject, so it is found by
    /// what it *is* rather than by an index path.
    fn find_splitter(tree: &WidgetTree, id: WidgetId) -> Option<WidgetId> {
        if tree.accessibility_node(id).role() == Role::Splitter && tree.bounds(id).width > 0.0 {
            return Some(id);
        }
        tree.children(id)
            .iter()
            .find_map(|&c| find_splitter(tree, c))
    }

    fn finger(raw: u64, primary: bool) -> PointerInfo {
        let id: PointerId = PointerIdAllocator::global().begin(BackendDeviceKey::new(0x50D0), raw);
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

    /// Hit-only: the divider keeps its painted thickness and the side keeps its
    /// size at every density. The grab lives between the pointer and the arena.
    #[test]
    fn the_grab_moves_no_layout_at_any_density() {
        let mut widths = Vec::new();
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let (tree, root, model) = leading_dock(density);
            let handle = find_splitter(&tree, root).expect("a dock resize handle");
            widths.push((
                tree.bounds(handle).width,
                model.side_size(DockSide::Leading),
            ));
        }
        let first = widths[0];
        for (w, size) in &widths {
            assert_eq!(
                *w, first.0,
                "the divider is painted the same at every density"
            );
            assert_eq!(*size, first.1, "and the side keeps its size");
        }
    }

    /// A mouse press beside the divider lands where it always did — the band is
    /// direct-pointer-only.
    #[test]
    fn a_mouse_press_beside_the_divider_does_not_reach_it() {
        let (tree, root, _) = leading_dock(TargetDensity::Compact);
        let handle = find_splitter(&tree, root).expect("a dock resize handle");
        let bar = tree.bounds(handle);
        let mouse = PointerInfo::mouse(EventTime::ZERO);
        let at = Point::new(bar.right() + 5.0, bar.center().y);
        let hit = tree.hit_test_for(at, &mouse);
        assert!(
            hit.is_none_or(|id| id != handle && !tree.is_descendant_of(id, handle)),
            "a mouse 5 dp past the divider must not reach it, got {hit:?}"
        );
    }

    /// A finger reaches the divider over the tappable content painted beside
    /// it.
    ///
    /// Probed at **Touch**, 14 dp out: the ring is 19 dp there while the
    /// miss-only slop pass reaches at most its 8 dp radius, so only the outset
    /// can answer and the test fails if `hit_outset` is removed.
    #[test]
    fn a_finger_grabs_the_divider_over_the_content_beside_it() {
        let (tree, root, _) = leading_dock(TargetDensity::Touch);
        let handle = find_splitter(&tree, root).expect("a dock resize handle");
        let bar = tree.bounds(handle);
        let contact = finger(21, true);
        for at in [
            Point::new(bar.right() + 14.0, bar.center().y),
            Point::new(bar.x - 14.0, bar.center().y),
        ] {
            let hit = tree.hit_test_for(at, &contact);
            assert!(
                hit.is_some_and(|id| id == handle || tree.is_descendant_of(id, handle)),
                "the widened divider must take the press at {at:?}, got {hit:?}"
            );
        }
    }

    /// A mouse drag resizes the side by exactly the distance travelled, as it
    /// always has.
    #[test]
    fn a_mouse_drag_resizes_the_side_by_exactly_the_travel() {
        let (mut tree, root, model) = leading_dock(TargetDensity::Compact);
        let handle = find_splitter(&tree, root).expect("a dock resize handle");
        let start = tree.bounds(handle).center();
        let before = model.side_size(DockSide::Leading);

        tree.pointer_down_button(start, teksilo_core::event::PointerButton::Primary);
        tree.pointer_move(Point::new(start.x + 40.0, start.y));
        tree.pointer_up_button(
            Point::new(start.x + 40.0, start.y),
            teksilo_core::event::PointerButton::Primary,
        );

        assert!(
            (model.side_size(DockSide::Leading) - (before + 40.0)).abs() < 0.01,
            "the side must widen by exactly 40 dp: {} → {}",
            before,
            model.side_size(DockSide::Leading)
        );
    }

    /// A second contact landing on the divider mid-resize must not recapture
    /// the anti-jump offset — the first finger's next move would otherwise be
    /// measured against the intruder and the side would leap.
    #[test]
    fn a_second_contact_during_a_resize_is_ignored() {
        let (mut tree, root, model) = leading_dock(TargetDensity::Compact);
        let handle = find_splitter(&tree, root).expect("a dock resize handle");
        let start = tree.bounds(handle).center();
        let before = model.side_size(DockSide::Leading);

        let first = finger(22, true);
        tree.dispatch_pointer(sample(first, PointerPhase::Down, start));
        tree.dispatch_pointer(sample(
            first,
            PointerPhase::Move,
            Point::new(start.x + 20.0, start.y),
        ));
        tree.layout(SizeProposal::exact(1000.0, 800.0));

        let second = finger(23, false);
        let bar = tree.bounds(handle);
        let intruder = Point::new(bar.center().x + 6.0, bar.center().y + 30.0);
        assert_eq!(
            tree.hit_test_for(intruder, &second)
                .map(|id| id == handle || tree.is_descendant_of(id, handle)),
            Some(true),
            "the intruder must actually reach the handle, or this proves nothing"
        );
        tree.dispatch_pointer(sample(second, PointerPhase::Down, intruder));

        tree.dispatch_pointer(sample(
            first,
            PointerPhase::Move,
            Point::new(start.x + 50.0, start.y),
        ));
        tree.layout(SizeProposal::exact(1000.0, 800.0));

        assert!(
            (model.side_size(DockSide::Leading) - (before + 50.0)).abs() < 0.01,
            "the side must track the first finger, not the intruder: {} → {}",
            before,
            model.side_size(DockSide::Leading)
        );
    }
}
