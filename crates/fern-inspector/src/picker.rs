//! Picker tool — captures a click and resolves it to a `WidgetId` on
//! the next layout pass. See `InspectorState::picker_mode` and
//! `InspectorState::pending_pick_point`.

use fern_canvas::{Canvas, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::binding::BindingLevel;
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, PointerButton, WidgetEvent};
use fern_core::overlay::{
    DismissBehavior, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use fern_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_tokens::Color;

use crate::state::{InspectorState, PickChainEntry};
use crate::tabs::last_segment;

/// Transparent leaf widget that covers the user-root subregion when
/// picker mode is active. Captures the next pointer-down and stashes
/// the position into `pending_pick_point`. The actual hit-test
/// (mapping point → widget id) is performed by `PickResolver` on the
/// following layout pass, where it can read the arena via
/// `LayoutContext`.
///
/// Mounted only when `picker_mode == true` (see `InspectorShell`).
/// While mounted it is **not** `event_pass_through` — that's the whole
/// point: it intercepts the click so the user's app doesn't react to
/// it.
pub(crate) struct PickerOverlay {
    state: InspectorState,
}

impl PickerOverlay {
    pub fn new(state: InspectorState) -> Self {
        Self { state }
    }
}

impl std::fmt::Debug for PickerOverlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PickerOverlay").finish()
    }
}

impl Widget for PickerOverlay {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Two-stage flow:
        //
        // 1. **PointerDown** stashes the click point. The framework
        //    runs layout next; `PickResolver` reads the point, hit-
        //    tests against each user-root subtree, walks the parent
        //    chain (up to 10 entries), and stores the chain into
        //    `state.pending_pick_chain` together with
        //    `state.pick_menu_anchor`. `picker_mode` stays on so the
        //    overlay remains active and the next pointer event still
        //    routes here.
        //
        // 2. **PointerUp** (or any subsequent pointer event in the
        //    same drag) reads the now-populated chain and presents
        //    the chain menu via `ctx.show_overlay`. The menu's per-
        //    row `Button::on_activate_fn` commits a selection, exits
        //    picker mode, and dismisses the overlay; the menu's
        //    `on_dismiss` does the same when the user dismisses with
        //    Escape or click-outside.
        //
        // Splitting across PointerDown and PointerUp lets us hit-test
        // *inside* a layout pass (where `LayoutContext::arena` is
        // available) while still showing the overlay from a path
        // that has `EventContext` access — neither context covers
        // both surfaces alone.
        let self_id = ctx.self_id();
        let state_for_handler = self.state.clone();
        let handlers = HandlerSet::new()
            .focusable(false)
            .on_pointer_event(move |event, ctx| match event {
                WidgetEvent::PointerDown {
                    position,
                    button: PointerButton::Primary,
                    ..
                } => {
                    state_for_handler.pending_pick_point.set(Some(*position));
                    EventResponse::Handled
                }
                WidgetEvent::PointerUp {
                    button: PointerButton::Primary,
                    ..
                } => {
                    show_chain_menu(&state_for_handler, ctx, self_id);
                    EventResponse::Handled
                }
                // Eat all other pointer events while picking so the
                // user's widgets don't get confusing partial input.
                _ => EventResponse::Handled,
            });
        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // Take the full proposed area so we sit on top of the entire
        // user-root subregion and capture pointer events anywhere in
        // it. With unspecified proposals (intrinsic queries from
        // ZStack) we still report 0 so we don't inflate the parent's
        // size — that's only relevant when picker mode is off, in
        // which case `visible_when` keeps us dormant anyway.
        let w = proposal.width.unwrap_or(0.0);
        let h = proposal.height.unwrap_or(0.0);
        fern_canvas::Size::new(w, h).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        // Faint tint so the user sees that "pick mode" is on.
        let tint = Color::from_rgba(0.13, 0.55, 1.0, 0.05);
        canvas.fill_rounded_rect(bounds, fern_tokens::CornerRadius::ZERO, tint);
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

/// Invisible leaf widget. On every layout pass, if
/// `pending_pick_point` is set, hit-tests it against the arena via
/// `LayoutContext::widget_at_point` (excluding the inspector shell's
/// own subtree so the picker never picks itself). Updates
/// `selected_id`, clears `pending_pick_point`, and turns off
/// `picker_mode` once a pointer-down was resolved.
pub(crate) struct PickResolver {
    state: InspectorState,
}

impl PickResolver {
    pub fn new(state: InspectorState) -> Self {
        Self { state }
    }
}

impl std::fmt::Debug for PickResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PickResolver").finish()
    }
}

impl Widget for PickResolver {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Re-run layout when the pending point changes.
        let self_id = ctx.self_id();
        self.state.pending_pick_point.bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::Relayout,
        );
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        if let Some(point) = self.state.pending_pick_point.get() {
            // Hit-test inside each user-root subtree (one per window).
            // Walking the user_root subtrees rather than `arena.roots()`
            // means we never resolve into the inspector's own chrome
            // (panel, picker overlay, highlight) — it sits as a
            // sibling of the user_root inside the InspectorShell, not
            // a descendant. First match wins, deepest-first per
            // subtree.
            let user_roots = self.state.user_root_ids.get();
            let arena = ctx.arena();
            let hit = user_roots
                .iter()
                .find_map(|&root| arena.and_then(|a| a.hit_test_in_subtree(root, point)));
            if let (Some(id), Some(arena)) = (hit, arena) {
                // Walk parent chain — deepest first. Stop at the
                // containing user-root id (inclusive) or after the
                // 10th entry. Capping the chain at 10 keeps the menu
                // scannable on dense composites (e.g. a `Button` deep
                // inside `Padding(Card(VStack(...)))`); deeper
                // ancestors are accessible by re-picking on the
                // chosen widget.
                const MAX_CHAIN: usize = 10;
                let label_for = |wid: WidgetId| -> String {
                    arena
                        .get(wid)
                        .map(|node| last_segment(node.widget.type_name()).to_string())
                        .unwrap_or_else(|| format!("#{wid:?}"))
                };
                let mut chain: Vec<PickChainEntry> = Vec::with_capacity(MAX_CHAIN);
                chain.push(PickChainEntry {
                    id,
                    label: label_for(id),
                });
                let mut cur = id;
                while chain.len() < MAX_CHAIN && !user_roots.contains(&cur) {
                    match arena.parent(cur) {
                        Some(parent) => {
                            chain.push(PickChainEntry {
                                id: parent,
                                label: label_for(parent),
                            });
                            cur = parent;
                        }
                        None => break,
                    }
                }
                self.state.pending_pick_chain.set(chain);
                self.state.pick_menu_anchor.set(Some(point));
            }
            self.state.pending_pick_point.set(None);
            // Picker_mode stays on until the menu's selection /
            // dismissal callback turns it off — see the menu wiring
            // in `InspectorShell::build`.
        }
        proposal.resolve(0.0, 0.0).into()
    }

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
}

/// Bridge from the picker's `PointerUp` handler to the chain-menu
/// overlay registered in `InspectorShell::build`. Reads the chain
/// + anchor stashed by `PickResolver` in the previous layout pass,
/// activates the pre-registered menu widget, and submits the
/// overlay request through `EventContext`. Clearing the anchor
/// signal here gates re-entry: subsequent `PointerUp` events that
/// arrive before the next picker click are no-ops.
fn show_chain_menu(
    state: &InspectorState,
    ctx: &mut fern_core::widget::EventContext<'_>,
    overlay_anchor: WidgetId,
) {
    let Some(point) = state.pick_menu_anchor.get() else {
        return;
    };
    let Some(menu_id) = state.pick_menu_id.get() else {
        return;
    };
    if state.pending_pick_chain.get().is_empty() {
        return;
    }
    state.pick_menu_anchor.set(None);
    ctx.activate(menu_id);
    let state_for_dismiss = state.clone();
    ctx.show_overlay(OverlayRequest {
        content_id: menu_id,
        anchor: overlay_anchor,
        placement: OverlayPlacement::AtPointer(point),
        dismiss: DismissBehavior::EscapeOrClickOutside,
        layer: OverlayLayer::InTree,
        parent_overlay: None,
        on_dismiss: Some(std::rc::Rc::new(move || {
            // Click-outside / Escape: discard the chain and exit
            // picker mode. The Pick toolbar button can re-enter
            // the picker.
            state_for_dismiss.pending_pick_chain.set(Vec::new());
            if state_for_dismiss.picker_mode.get() {
                state_for_dismiss.picker_mode.set(false);
            }
        })),
        fade_duration: None,
    });
}
