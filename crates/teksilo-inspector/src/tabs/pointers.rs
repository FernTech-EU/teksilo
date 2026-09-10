// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Pointers tab — what the input layer is doing, and what the tree told it to
//! do.
//!
//! Two halves, and they answer different questions.
//!
//! **The declarations** (always live) are read straight out of the arena: every
//! node in the user app that declares a pan claim, a `touch_action`, a gesture
//! dead zone, a hit-slop policy, a multi-contact policy or a drag activation,
//! with the *effective* touch action folded down its ancestor chain beside its
//! own. This is the half that answers "why does my list not pan under a
//! finger" — the answer is almost always a declaration several nodes above it.
//!
//! **The live contacts** need a watch to be armed, and the button says so.
//! Nothing a widget can reach reports the tree's live pointers: `LayoutContext`
//! carries focus, shortcuts and overlays but no pointer state, and an
//! `EventContext` only ever describes the sample being dispatched. So the watch
//! is a real full-window surface that takes the input and reports it —
//! `PointerWatchOverlay`, the same shape as the picker's overlay, off by
//! default because while it is on the app under it receives nothing.
//!
//! What the tab therefore cannot show is a pointer working the app *while* the
//! app has it. That needs one accessor on the tree; see the package report.

use std::cell::RefCell;

use teksilo_canvas::{Canvas, Point, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::arena::WidgetArena;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, WidgetEvent};
use teksilo_core::gesture::MultiContact;
use teksilo_core::pointer::touch_action::TouchAction;
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{InputTokens, PointerKind, TextRole};

use crate::state::InspectorState;
use crate::tabs::{ROW_HEIGHT, ROW_PADDING_X, last_segment};

/// One live pointer, as the watch probe last saw it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PointerRow {
    /// Raw pointer id — the same number `PointerId` shows in a trace line.
    pub id: u64,
    pub kind: PointerKind,
    pub position: Point,
    /// Whether the contact is down (a pen or mouse may be hovering).
    pub down: bool,
    /// The touch action frozen for this pointer's press, formatted.
    pub touch_action: String,
    /// Whether the framework holds a press for it.
    pub pressed: bool,
}

/// Human-readable flag list for a [`TouchAction`] — the derived `Debug` prints
/// the bit pattern, which is no use in a panel.
pub(crate) fn format_touch_action(action: TouchAction) -> String {
    if action == TouchAction::AUTO {
        return "AUTO".to_string();
    }
    if action.is_none() {
        return "NONE".to_string();
    }
    let mut parts: Vec<&str> = Vec::new();
    if action.allows_pan_x() {
        parts.push("PAN_X");
    }
    if action.allows_pan_y() {
        parts.push("PAN_Y");
    }
    if action.allows_pinch() {
        parts.push("PINCH_ZOOM");
    }
    parts.join("|")
}

/// The effective touch action at `id`: the intersection of every declaration
/// from the root down to it.
///
/// Recomputed here rather than asked of the tree, because the tree's own fold
/// (`WidgetTree::effective_touch_action`) is `pub(crate)` and a widget has no
/// way to reach it. Same rule: intersect, root first.
fn effective_touch_action(arena: &WidgetArena, id: WidgetId) -> TouchAction {
    let mut chain: Vec<WidgetId> = Vec::new();
    let mut cursor = Some(id);
    while let Some(node) = cursor {
        chain.push(node);
        cursor = arena.parent(node);
    }
    chain
        .iter()
        .fold(TouchAction::AUTO, |acc, node| match arena.get(*node) {
            Some(n) => acc.intersect(n.touch_action),
            None => acc,
        })
}

/// One declaration line, or `None` if the node declares nothing.
fn declaration_line(arena: &WidgetArena, id: WidgetId) -> Option<String> {
    let node = arena.get(id)?;
    let mut parts: Vec<String> = Vec::new();
    if let Some(claim) = node.pan_claim {
        parts.push(format!(
            "pan={:?}{}",
            claim.axes,
            if claim.kinetic { " kinetic" } else { "" }
        ));
    }
    if node.touch_action != TouchAction::AUTO {
        parts.push(format!(
            "touch-action={}",
            format_touch_action(node.touch_action)
        ));
    }
    if node.gesture_dead_zone {
        parts.push("dead-zone".to_string());
    }
    if node.no_hit_slop {
        parts.push("no-hit-slop".to_string());
    }
    if let Some(slop) = node.hit_slop {
        parts.push(format!("hit-slop={:?}", slop));
    }
    if node.multi_contact != MultiContact::First {
        parts.push(format!("multi-contact={:?}", node.multi_contact));
    }
    if node.drag_activation != teksilo_tokens::DragActivation::Auto {
        parts.push(format!("drag={:?}", node.drag_activation));
    }
    if parts.is_empty() {
        return None;
    }
    let effective = effective_touch_action(arena, id);
    Some(format!(
        "{}(#{:?})  {}  [effective {}]",
        last_segment(node.widget.type_name()),
        id,
        parts.join("  "),
        format_touch_action(effective)
    ))
}

pub(crate) struct PointersTab {
    state: InspectorState,
    lines: RefCell<Vec<String>>,
}

impl PointersTab {
    pub fn new(state: InspectorState) -> Self {
        Self {
            state,
            lines: RefCell::new(Vec::new()),
        }
    }

    /// The whole tab, as text. Built in `layout_response` and consumed by both
    /// `paint` and `accessibility`, so the panel reads the same to a screen
    /// reader as it looks.
    fn build_lines(&self, ctx: &LayoutContext) -> Vec<String> {
        let tokens: &InputTokens = &ctx.theme.input;
        let touch = tokens.profile(PointerKind::Touch);
        let mut lines = vec![
            format!(
                "density={:?}  target={}dp  grab={}dp  slop-budget={}dp  spacing×{:.2}  touch={}",
                tokens.density,
                tokens.target_size,
                tokens.grab_size,
                tokens.slop_budget,
                tokens.spacing_factor,
                if tokens.touch_enabled { "on" } else { "off" },
            ),
            format!(
                "touch profile: tap-slop={}  drag-slop={}  pan-slop={}  hold={}ms  drag={:?}",
                touch.tap_slop,
                touch.drag_slop,
                touch
                    .pan_slop
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "—".to_string()),
                touch.long_press.as_millis(),
                touch.drag_activation,
            ),
        ];

        let watching = self.state.pointer_watch.get();
        let rows = self.state.pointer_rows.get();
        lines.push(if watching {
            format!(
                "watch: ARMED — the app receives no input ({} live)",
                rows.len()
            )
        } else {
            "watch: off — press Watch to report live contacts".to_string()
        });
        if watching && rows.is_empty() {
            lines.push("  (no pointer seen yet)".to_string());
        }
        for row in &rows {
            lines.push(format!(
                "  #{} {:?} at ({:.0},{:.0}) {} touch-action={} {}",
                row.id,
                row.kind,
                row.position.x,
                row.position.y,
                if row.down { "down" } else { "hover" },
                row.touch_action,
                if row.pressed { "pressed" } else { "" },
            ));
        }

        lines.push(String::new());
        lines.push("declarations (user tree):".to_string());
        let mut declared = 0_usize;
        if let Some(arena) = ctx.arena() {
            for root in self.state.user_root_ids.get() {
                collect_declarations(arena, root, &mut lines, &mut declared);
            }
        }
        if declared == 0 {
            lines.push("  (nothing in the user tree declares a pointer policy)".to_string());
        }
        lines
    }
}

fn collect_declarations(
    arena: &WidgetArena,
    id: WidgetId,
    out: &mut Vec<String>,
    declared: &mut usize,
) {
    if let Some(line) = declaration_line(arena, id) {
        out.push(format!("  {line}"));
        *declared += 1;
    }
    for child in arena.children(id) {
        collect_declarations(arena, *child, out, declared);
    }
}

impl std::fmt::Debug for PointersTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PointersTab").finish()
    }
}

impl PointersTab {
    /// The lines the last layout pass produced. Test-only reader; the tab
    /// opts into `Widget::as_any` so a test can reach it once mounted.
    #[cfg(test)]
    pub(crate) fn lines_for_test(&self) -> Vec<String> {
        self.lines.borrow().clone()
    }
}

impl Widget for PointersTab {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Relayout whenever the watch reports something, or is armed.
        let self_id = ctx.self_id();
        self.state
            .pointer_rows
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        self.state
            .pointer_watch
            .bind_to(self_id, ctx.binding_registry(), BindingLevel::Relayout);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let lines = self.build_lines(ctx);
        // Read-only text: a line here is not a target and does not grow with
        // the density (see `crate::tabs::row_height`).
        let height = (lines.len().max(1) as f32) * ROW_HEIGHT;
        *self.lines.borrow_mut() = lines;
        proposal.resolve(0.0, height).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let style = &theme.typography.mono;
        let primary = TextRole::Primary.resolve(&theme.colors);
        for (i, line) in self.lines.borrow().iter().enumerate() {
            let y = bounds.y + (i as f32) * ROW_HEIGHT + 2.0;
            let rect = Rect::new(bounds.x + ROW_PADDING_X, y, bounds.width, ROW_HEIGHT);
            canvas.draw_text(line, rect, style, primary);
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Painted text, so the house convention for painted text: one
        // `Role::Label` whose name is what is on screen (`TextWidget` does the
        // same). Without it the panel is a blank rectangle to a screen reader.
        builder.set_role(teksilo_core::accesskit::Role::Label);
        builder.set_name(self.lines.borrow().join("\n"));
    }
}

/// The armed watch: a full-window surface that takes every pointer event and
/// reports it into [`InspectorState::pointer_rows`].
///
/// Mounted by `InspectorShell` only while `pointer_watch` is on, exactly as
/// `PickerOverlay` is mounted only while picking — and, like it, it **takes the
/// app's input** while mounted. That is the whole mechanism: a widget can only
/// report the pointers dispatched to it.
///
/// What takes the application's input is the probe's *position* — it is the
/// topmost hittable node over the whole window, so the press never reaches the
/// tree beneath it. Answering `Handled` records the consumption and stops the
/// bubble, but it is not what denies the press: measured by returning `Ignored`
/// instead, which changes nothing observable, because the widgets under the
/// probe are its siblings' descendants rather than its ancestors. It declares
/// **no** multi-contact
/// policy: a second finger is reported without one — measured by deleting the
/// `MultiContact::All` this originally carried and watching
/// `the_armed_watch_reports_both_fingers` stay green — and a declaration that
/// changes nothing is a claim no test can hold.
pub(crate) struct PointerWatchOverlay {
    state: InspectorState,
}

impl PointerWatchOverlay {
    pub fn new(state: InspectorState) -> Self {
        Self { state }
    }
}

impl std::fmt::Debug for PointerWatchOverlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PointerWatchOverlay").finish()
    }
}

impl Widget for PointerWatchOverlay {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let state = self.state.clone();
        let handlers = HandlerSet::new()
            .focusable(false)
            // Nothing default may run over the probe — a pan or a hold here
            // would be the framework acting on a sample being reported.
            .touch_action(TouchAction::NONE)
            .on_pointer_event(move |event, ctx| {
                let pointer = ctx.pointer();
                let mut rows = state.pointer_rows.get();
                let position = match event {
                    WidgetEvent::PointerDown { position, .. }
                    | WidgetEvent::PointerUp { position, .. }
                    | WidgetEvent::PointerMove { position, .. } => Some(*position),
                    _ => None,
                };
                let down = matches!(
                    event,
                    WidgetEvent::PointerDown { .. } | WidgetEvent::PointerMove { .. }
                ) && !pointer.buttons.is_empty();
                let ends = matches!(
                    event,
                    WidgetEvent::PointerUp { .. } | WidgetEvent::PointerCancel { .. }
                );
                let id = pointer.id.get();
                rows.retain(|r| r.id != id);
                if !ends {
                    rows.push(PointerRow {
                        id,
                        kind: pointer.kind,
                        position: position.unwrap_or(Point::ZERO),
                        down,
                        touch_action: format_touch_action(ctx.touch_action()),
                        pressed: ctx.press_is_inside(),
                    });
                    rows.sort_by_key(|r| r.id);
                } else if !pointer.kind.hovers() {
                    // A lifted contact ceases to exist; a mouse or a pen stays
                    // on the list, hovering, which is what the pointer table
                    // itself does.
                    rows.retain(|r| r.id != id);
                }
                state.pointer_rows.set(rows);
                EventResponse::Handled
            });
        ctx.apply_self_handlers(handlers);
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // Fill whatever it is given, and report nothing on an intrinsic query
        // so it never inflates its parent (the picker overlay's rule, and for
        // the same reason).
        let w = proposal.width.unwrap_or(0.0);
        let h = proposal.height.unwrap_or(0.0);
        teksilo_canvas::Size::new(w, h).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, _ctx: &PaintContext) {
        // A faint tint plus a mark under each contact: the probe has to be
        // visibly on, because while it is the app receives nothing.
        let tint = teksilo_tokens::Color::from_rgba(1.0, 0.45, 0.0, 0.05);
        canvas.fill_rounded_rect(bounds, teksilo_tokens::CornerRadius::ZERO, tint);
        let mark = teksilo_tokens::Color::from_rgba(1.0, 0.45, 0.0, 0.55);
        for row in self.state.pointer_rows.get() {
            let r = 12.0;
            canvas.fill_rounded_rect(
                Rect::new(row.position.x - r, row.position.y - r, r * 2.0, r * 2.0),
                teksilo_tokens::CornerRadius::uniform(r),
                mark,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Not decoration: while it is mounted it is the surface taking every
        // press, so an assistive-technology user must be told it is there.
        builder.set_role(teksilo_core::accesskit::Role::Group);
        builder.set_name("Pointer watch — the application receives no input while this is armed");
    }
}
