// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Pointer and scroll dispatch for the rich text editor.
//!
//! Owns three handler entry points that the widget installs in
//! `build()`:
//!
//!  * [`handle_pointer_event`] — PointerDown (caret placement +
//!    drag-select start), PointerMove (drag-select extension and
//!    auto-scroll velocity computation), PointerUp (drag teardown).
//!    Returns `EventResponse::Ignored` for PointerDown so the
//!    gesture arena's `DoubleTapRecognizer` / `TripleTapRecognizer`
//!    also see the event.
//!  * [`handle_double_tap`] / [`handle_triple_tap`] — word and
//!    paragraph selection on successive clicks. The independent
//!    cooperative recognizers in `teksilo-core::gesture` guarantee that
//!    both fire in an escalating click sequence.
//!  * [`handle_long_press`] — a hold selects the word under a finger and
//!    raises the touch affordances.
//!
//! # Two devices, two commit points
//!
//! A **precise** pointer commits on the press, exactly as it always has: a
//! click is a click, and a press that never becomes anything else is still one.
//!
//! A **direct** pointer — a finger, a pen — defers the whole decision to the
//! release, because the same contact is the opening sample of a *pan* and a
//! panning finger must leave the caret and the selection exactly as it found
//! them. No release-time predicate can rescue a caret already written on
//! `PointerDown`, so the write itself moves to the release, gated on
//! [`release_completes_the_press`](crate::data_views::release_completes_the_press)
//! — the same rule, and the same predicate, that the five data views adopted for
//! their row selection and the single-line stack for its caret.
//!
//! Three press-time commitments therefore have no direct-pointer form, and each
//! is a deliberate limitation rather than an oversight:
//!
//! * **Drag-select.** A finger's drag pans; the range is chosen with the
//!   selection handles the hold raises.
//! * **Picking up selected text.** Dragging a passage out of the editor stays a
//!   precise-pointer gesture.
//! * **Resizing an inline picture.** The corner grip latches on a press, and a
//!   press is what the pan needs. Its hit rectangle is nonetheless widened for
//!   *every* pointer kind — see [`grip_reach`] — because the target has to clear
//!   the 24 dp floor for the pointer that can reach it.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};
use teksilo_core::event::{EventResponse, PointerButton, WidgetEvent};
use teksilo_core::widget::{CursorIcon, EventContext};
use teksilo_text::text_document::{MoveMode, SelectionType};

use teksilo_tokens::{InputTokens, PointerKind, TargetRole};

use super::hit_test;
use super::state::{DragState, SharedState};
use super::sync_cursor_signals;
use super::touch_mount::{EditorTouch, ToolbarIntent};

/// Convert a **wrapper-node-local** pointer position (as delivered by the
/// framework dispatch) into the **engine/body-local** space that
/// text-typeset's `hit_test` expects. The body is inset within the
/// wrapper, so reconstruct the window point (`position + node_origin`)
/// and subtract the body origin (`viewport_origin`). At wrapper-origin 0
/// this reduces to `position - viewport_origin`.
fn to_engine_local(state: &SharedState, position: &Point) -> Point {
    let st = state.borrow();
    Point::new(
        position.x + st.node_origin.x - st.viewport_origin.x,
        position.y + st.node_origin.y - st.viewport_origin.y,
    )
}

/// Smallest side a resize may leave, in logical pixels.
///
/// A picture dragged to nothing cannot be grabbed again — its handles would
/// have nowhere to sit — so the drag stops here rather than letting the writer
/// lose the image behind an undo.
const RESIZE_MIN_EDGE: f32 = 24.0;

/// How far outside a grip's own square a press still counts.
///
/// The grip is drawn small so it does not cover a thumbnail; this is what makes
/// it hittable without care. Generous on purpose — the cost of overshooting is
/// a resize the writer did not want, which one Escape or Ctrl+Z undoes, while
/// the cost of undershooting is a feature that feels broken.
const RESIZE_HANDLE_SLOP: f32 = 5.0;

/// Half-extent of a grip's hit square at the live density.
///
/// Two mechanisms, layered, and the order is what makes the mouse's behaviour
/// survive: [`handle_at`]'s own reach — the painted square plus
/// [`RESIZE_HANDLE_SLOP`] — is tried first and is unchanged, and this is the
/// **miss-only** top-up consulted after it. That is the same division of labour
/// `Widget::hit_outset` and the hit-test's slop pass keep: a grip that already
/// answers keeps answering, and a near miss is re-attributed rather than a
/// neighbour being beaten.
///
/// Widened for **every** pointer kind, not only a direct one: WCAG 2.2 SC 2.5.8
/// asks for a 24 dp target whatever the pointer is, and a 9 dp painted square is
/// a long way under it. Nothing sits behind the grip but the picture it belongs
/// to and the editor's own node, so the widened band punches no hole in a
/// neighbour — which is the reason a widened *node* usually has to be gated on
/// the press it will accept.
///
/// The top-up is clamped to a quarter of the picture's shorter side, so the four
/// grips can never grow into each other or over the middle of the picture: on a
/// thumbnail, `Touch`'s 44 dp projection would otherwise leave nothing to click
/// but grips. The clamp never lowers the reach below [`handle_at`]'s own.
///
/// It takes **no pointer kind**, deliberately. A widening that applied to a
/// finger alone would be a parameter nothing in a mounted editor could vary —
/// the grip's press arm is precise-pointer-only — and a conformance floor that
/// only one device gets is not a floor. The density is the whole input.
fn grip_reach(tokens: &InputTokens, rect: [f32; 4]) -> f32 {
    use teksilo_core::styles::density::dp;
    let base = super::paint::RESIZE_HANDLE_SIZE / 2.0 + RESIZE_HANDLE_SLOP;
    let projected = dp(super::paint::RESIZE_HANDLE_SIZE, TargetRole::Target, tokens) / 2.0;
    let quarter = rect[2].min(rect[3]) / 4.0;
    base.max(projected.min(quarter))
}

/// The corner grip under `local`, if the selected image has one there.
///
/// Returns the image's name, its rect, its offset, and which corner was taken.
fn grabbed_handle(
    state: &SharedState,
    local: Point,
) -> Option<(String, [f32; 4], usize, (f32, f32))> {
    let (selected, tokens) = {
        let st = state.borrow();
        (st.selected_image.borrow().clone()?, st.input_tokens)
    };
    let corner = corner_at(selected.rect, local, &tokens)?;
    Some((selected.name, selected.rect, selected.offset, corner))
}

/// Which corner of `rect` a press at `local` takes, over both mechanisms.
///
/// [`handle_at`]'s own reach first — the painted square plus
/// [`RESIZE_HANDLE_SLOP`], unchanged — and, only when that missed, the
/// density-projected [`grip_reach`]. Miss-only, in that order, which is what
/// keeps the aiming a precise pointer already had byte for byte while the target
/// as a whole reaches the conformance floor.
fn corner_at(rect: [f32; 4], local: Point, tokens: &InputTokens) -> Option<(f32, f32)> {
    handle_at(rect, local).or_else(|| handle_near(rect, local, grip_reach(tokens, rect)))
}

/// The corner of `rect` whose centre is **nearest** `local`, within `reach`.
///
/// Nearest rather than first-declared, because a widened reach makes adjacent
/// grips overlap on a small picture and sibling order is not an aiming rule —
/// the same tie-break the hit test's slop pass uses between adjacent grips.
fn handle_near(rect: [f32; 4], local: Point, reach: f32) -> Option<(f32, f32)> {
    let [x, y, w, h] = rect;
    super::paint::RESIZE_CORNERS
        .into_iter()
        .filter_map(|(fx, fy)| {
            let (cx, cy) = (x + w * fx, y + h * fy);
            let (dx, dy) = (local.x - cx, local.y - cy);
            (dx.abs() <= reach && dy.abs() <= reach).then_some(((fx, fy), dx * dx + dy * dy))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(corner, _)| corner)
}

/// Which corner of `rect` a press at `local` grabs, if any.
///
/// Split out from [`grabbed_handle`] so the aiming rule — the half of this that
/// decides whether the feature is usable — can be tested without a widget tree,
/// a window, or a pointer device.
fn handle_at(rect: [f32; 4], local: Point) -> Option<(f32, f32)> {
    let [x, y, w, h] = rect;
    let reach = super::paint::RESIZE_HANDLE_SIZE / 2.0 + RESIZE_HANDLE_SLOP;
    super::paint::RESIZE_CORNERS.into_iter().find(|&(fx, fy)| {
        let (cx, cy) = (x + w * fx, y + h * fy);
        (local.x - cx).abs() <= reach && (local.y - cy).abs() <= reach
    })
}

/// The rect a corner drag proposes, with the picture's proportions kept.
///
/// The corner opposite the one being dragged stays put, so the gesture reads
/// the way it does everywhere else. Proportions are kept by following whichever
/// axis the pointer moved *proportionally* further — taking width alone would
/// ignore a drag that was mostly vertical, and averaging the two makes a
/// diagonal drag lag behind the pointer on both.
///
/// The result is always positioned at the image's original top-left: the
/// picture sits in a text flow, so the layout decides where it lands once the
/// size changes. Anchoring the preview anywhere else would show the writer a
/// position the reflow is about to contradict.
fn proportional_resize(origin: [f32; 4], corner: (f32, f32), local: Point) -> [f32; 4] {
    let [x, y, w, h] = origin;
    if w <= 0.0 || h <= 0.0 {
        return origin;
    }
    // The fixed corner is the diagonal opposite of the grabbed one.
    let anchor_x = x + w * (1.0 - corner.0);
    let anchor_y = y + h * (1.0 - corner.1);
    let dragged_w = (local.x - anchor_x).abs();
    let dragged_h = (local.y - anchor_y).abs();

    let sx = dragged_w / w;
    let sy = dragged_h / h;
    let scale = if (sx - 1.0).abs() >= (sy - 1.0).abs() {
        sx
    } else {
        sy
    };
    let scale = scale.max(RESIZE_MIN_EDGE / w.min(h));
    [x, y, (w * scale).max(1.0), (h * scale).max(1.0)]
}

/// Whether a press on a link **follows** it rather than placing a caret in its
/// text.
///
/// One rule, consulted by both pointer paths, because the two answers only
/// differ in what they can ask for:
///
/// * **A direct pointer always follows.** There is no Ctrl to hold on a touch
///   screen, so the precise pointer's split has no direct form — and a link a
///   finger cannot follow by tapping it reads as broken. The way to reach a
///   link's *text* with a finger is the hold, which selects the word under it.
/// * **A precise pointer follows a link in a read-only surface** and needs
///   Ctrl(⌘) in an editable one. The reason for the split is an *authoring* one:
///   the writer who clicks their own link is far more often trying to edit its
///   text than to leave the document, and intercepting every click left the text
///   inside a link unreachable by pointer entirely. On a viewer that reason has
///   no force — the text cannot be edited, and the caret the click places is
///   `CaretPolicy::Hidden` — while a link a plain click ignores reads as broken.
fn link_follows(kind: PointerKind, read_only: bool, command_held: bool) -> bool {
    kind.is_direct() || read_only || command_held
}

/// How far the pointer must travel from a press inside the selection before it
/// counts as dragging that text rather than as a click that happened to wobble.
///
/// The pointer's **own** `drag_slop` — 5 dp for a mouse, 2 dp for a pen, 18 dp
/// for a finger — rather than one hardcoded distance for every device. It used
/// to be a local `4.0`, which is a mouse figure invented here; `GestureProfile`
/// carries the framework's, and the mouse's 5 dp is documented there as the
/// constant Teksilo already used elsewhere. A finger needs far more: 4 dp of
/// travel is inside the jitter of holding still.
///
/// The touch and pen answers are **not reachable from a mounted editor today**:
/// `DragState::PendingTextDrag` is armed only on the precise-pointer press arm,
/// because a direct pointer's press belongs to the pan. Dragging a passage with
/// a finger is P31's. So this is a pure function with its own test rather than a
/// value only one device can observe.
fn text_drag_threshold(kind: PointerKind, tokens: &InputTokens) -> f32 {
    tokens.profile(kind).drag_slop
}

/// Whether `offset` falls inside the current selection.
///
/// The end boundary is excluded so a click at the very edge of a selection
/// still collapses it — otherwise the only way out of a selection ending at the
/// caret would be to click somewhere else entirely.
fn selection_contains(state: &SharedState, offset: usize) -> bool {
    let st = state.borrow();
    if !st.cursor.has_selection() {
        return false;
    }
    let (a, b) = (st.cursor.anchor(), st.cursor.position());
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    (lo..hi).contains(&offset)
}

/// Begin dragging the current selection.
///
/// The payload carries the `DocumentFragment` for editors (formatting intact)
/// *and* a `text/plain` MIME alternative, which is what lets the same gesture
/// continue into another application: the framework escalates an in-app drag to
/// a native OS drag at the window boundary when the payload has MIME data, so
/// dragging prose into a text editor costs nothing extra here.
fn start_text_drag(state: &SharedState, ctx: &mut EventContext) {
    let payload = {
        let st = state.borrow();
        let Some(source) = st.self_id else {
            return;
        };
        if !st.cursor.has_selection() {
            return;
        }
        let (a, b) = (st.cursor.anchor(), st.cursor.position());
        let range = if a <= b { (a, b) } else { (b, a) };
        let fragment = st.cursor.selection();
        let text = fragment.to_plain_text().to_string();
        if text.is_empty() {
            return;
        }
        teksilo_core::DragPayload::typed(super::EditorTextDrag {
            source,
            range,
            fragment,
            text: text.clone(),
        })
        .with_mime("text/plain", text.into_bytes())
    };
    let source = state.borrow().self_id;
    if let Some(source) = source {
        ctx.start_drag(source, payload);
    }
    state.borrow_mut().drag_state = DragState::Idle;
    ctx.request_frame();
}

/// Insert dragged editor text at the caret, removing the original when the drop
/// landed in the editor it came from.
///
/// Returns whether anything was accepted.
pub(super) fn apply_text_drop(
    state: &SharedState,
    drag: &super::EditorTextDrag,
    same_editor: bool,
) -> bool {
    let (lo, hi) = drag.range;
    let drop_at = state.borrow().cursor.position();

    if !same_editor {
        let st = state.borrow();
        super::keyboard::collapse_selection_before_insert(&st);
        let _ = st.cursor.insert_fragment(&drag.fragment);
        return true;
    }

    // A same-editor drop is a *move*: the passage is removed from where it was.
    // That is a deletion by mouse, so it answers to the same filter Backspace
    // does — otherwise a forward-only surface could still be emptied out one
    // dragged phrase at a time. Refuse rather than degrade to a copy: silently
    // duplicating the passage would be a stranger outcome than nothing
    // happening. (Dragging *between* editors above is a copy and stays allowed:
    // the source keeps its text.)
    if !state
        .borrow()
        .policy
        .command_filter
        .accepts(super::policy::EditCommandKind::Cut)
    {
        return false;
    }

    // Dropped inside the very text being dragged: there is no move to make, and
    // deleting the range would destroy the selection the writer was carrying.
    // Accept it so the drag ends here rather than bubbling to a parent that
    // would act on it.
    if (lo..=hi).contains(&drop_at) {
        return true;
    }

    // Remove the original first, then insert. Deleting shifts everything after
    // the range left by its length, so a drop *after* the range has to be
    // re-based or the text lands that many characters too far right.
    {
        let st = state.borrow();
        st.cursor.set_position(lo, MoveMode::MoveAnchor);
        st.cursor.set_position(hi, MoveMode::KeepAnchor);
        let _ = st.cursor.remove_selected_text();
        let target = if drop_at > hi {
            drop_at - (hi - lo)
        } else {
            drop_at
        };
        st.cursor.set_position(target, MoveMode::MoveAnchor);
        let _ = st.cursor.insert_fragment(&drag.fragment);
    }
    true
}

pub(super) fn handle_pointer_event(
    state: &SharedState,
    touch: &Rc<EditorTouch>,
    v_scrollbar_bounds: &Rc<Cell<Rect>>,
    h_scrollbar_bounds: &Rc<Cell<Rect>>,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    if ctx.pointer_kind().is_direct() {
        return handle_direct_pointer_event(
            state,
            touch,
            v_scrollbar_bounds,
            h_scrollbar_bounds,
            event,
            ctx,
        );
    }
    match event {
        WidgetEvent::PointerDown {
            position,
            button,
            modifiers,
            ..
        } => {
            if *button != PointerButton::Primary {
                // Secondary / middle are for the application's own
                // context menu; let them bubble.
                return EventResponse::Ignored;
            }
            // A cursor has taken over. Touch chrome — handles, the selection
            // toolbar — is standing on the text it is trying to reach, and the
            // affordance band is exempt from outside-press dismissal, so nothing
            // else on a hybrid machine would ever remove it. Free when there is
            // nothing raised; see `EditorTouch::dismiss`.
            touch.dismiss();
            // The wrapper's `on_pointer_event` runs in the preview
            // pass on every event aimed at a descendant, including
            // the overlay scrollbars. A press here would otherwise
            // latch `drag_state = Selecting` against the text under
            // the bar and then `EventResponse::Handled` would steal
            // the subsequent PointerMove from the scrollbar's
            // gesture arena.
            if v_scrollbar_bounds.get().contains(*position)
                || h_scrollbar_bounds.get().contains(*position)
            {
                return EventResponse::Ignored;
            }
            let shift = modifiers.shift();
            let local = to_engine_local(state, position);
            // A resize grip is checked before the hit-test, because a grip sits
            // *outside* the picture: the engine reports `HitRegion::Image` only
            // within the image's own rect, so by the time the hit-test has an
            // answer the corner has already been missed.
            if let Some((name, rect, offset, corner)) = grabbed_handle(state, local) {
                let mut st = state.borrow_mut();
                st.drag_state = DragState::ResizingImage {
                    name,
                    offset,
                    origin: rect,
                    corner,
                };
                st.resize_preview.set(Some(rect));
                drop(st);
                ctx.request_frame();
                // `Ignored`, like every other press path here. Returning
                // `Handled` consumes the event, and the gesture arena that
                // consumed press never delivers the moves that follow — so the
                // grip would latch and the drag would never arrive.
                return EventResponse::Ignored;
            }
            let hit = {
                let st = state.borrow();
                hit_test::hit_test_at(&st.engine, local, 0.0, 0.0)
            };
            let Some(hit) = hit else {
                // Return Ignored so the gesture arena still sees the
                // event — a click that missed text still activates
                // the widget, and the double/triple tap recognizers
                // need every press to progress their state machines.
                return EventResponse::Ignored;
            };
            // A viewer follows a link on a plain click; an editor still asks for
            // Ctrl(⌘). See the arm below for why the two differ.
            let read_only = state.borrow().policy.is_read_only();
            match &hit.region {
                teksilo_text::HitRegion::Link { href }
                    if link_follows(ctx.pointer_kind(), read_only, modifiers.command()) =>
                {
                    // Follow the link: do not move the caret. Dispatch to the
                    // widget's installed `on_link_activated` callback (if any)
                    // so applications can open the link / route to their
                    // router. Clone the `Rc` out of the state borrow before
                    // invoking so the handler can mutate widget state if it
                    // wants.
                    //
                    // Which clicks qualify depends on what kind of surface this
                    // is, which is exactly what `PolicyBundle::is_read_only`
                    // answers:
                    //
                    // * **Editable.** Ctrl(⌘)+click only; a *plain* click falls
                    //   through to ordinary caret placement below. The writer
                    //   who clicks their own link is far more often trying to
                    //   edit its text than to leave the document, and
                    //   intercepting every click left the text inside a link
                    //   unreachable by pointer entirely.
                    // * **Read-only.** A plain click follows it. The rationale
                    //   above is an *authoring* rationale and simply does not
                    //   apply: there is no text to edit, and the caret the click
                    //   would otherwise place is hidden — while a link the reader
                    //   cannot follow by clicking it is a link that reads as
                    //   broken. This is the browser default,
                    //   and a viewer can afford it for the same reason a browser
                    //   can — its text is not editable.
                    let callback = state.borrow().on_link_activated.clone();
                    if let Some(cb) = callback {
                        cb(href.as_str(), ctx);
                    }
                    ctx.request_frame();
                    return EventResponse::Ignored;
                }
                teksilo_text::HitRegion::Image { name } => {
                    let callback = state.borrow().on_image_activated.clone();
                    if let Some(cb) = callback {
                        // The caret is deliberately left where it was, as for a
                        // link. The offset is handed over instead, so a host that
                        // wants the image selected can say so itself — and one
                        // that only wants to open a viewer is not left having to
                        // put the caret back.
                        cb(
                            &super::ImageActivation {
                                name: name.clone(),
                                offset: hit.position,
                            },
                            ctx,
                        );
                    }
                    // The same press bookkeeping an ordinary click does, minus
                    // the caret placement — a click on a picture is still a
                    // click, and the state it leaves behind has to say so.
                    //
                    // `drag_state` above all: without it a drag that *starts*
                    // on a picture never becomes a selection (`PointerMove`
                    // gates on `is_dragging`), so the only way to select an
                    // image together with the words after it was to start the
                    // drag somewhere else and come back over it.
                    {
                        let mut st = state.borrow_mut();
                        st.drag_state = DragState::Selecting {
                            auto_scroll_v_per_s: 0.0,
                        };
                        st.preferred_x = None;
                        st.select_all_level = 0;
                        st.select_all_anchor_cell = None;
                        st.mouse_anchored = true;
                    }
                    ctx.request_frame();
                    return EventResponse::Ignored;
                }
                _ => {}
            }
            // A press *inside* the selection may be the writer picking that
            // text up to drag it elsewhere, so the selection has to survive
            // until the pointer says whether it was a drag or a plain click
            // (`PointerMove` and `PointerUp` below each resolve one way).
            // Shift+click is always an extend and never a drag.
            if !shift && selection_contains(state, hit.position) {
                let mut st = state.borrow_mut();
                st.drag_state = DragState::PendingTextDrag {
                    origin: [position.x, position.y],
                };
                st.mouse_anchored = true;
                drop(st);
                return EventResponse::Ignored;
            }
            // Place the cursor. Shift+click extends the selection
            // from the existing anchor; plain click collapses it.
            {
                let mut st = state.borrow_mut();
                let mode = if shift {
                    MoveMode::KeepAnchor
                } else {
                    MoveMode::MoveAnchor
                };
                st.cursor.set_position(hit.position, mode);
                // Affinity: at a soft-wrap boundary the typesetter
                // returned Upstream when the click landed on line
                // K+1's left edge (the visual START of the wrapped
                // line). At every other position the hit-test returns
                // Downstream and there is nothing to change.
                st.cursor_affinity = hit.affinity;
                // A fresh press starts a drag-select session.
                // Stored velocity is 0 until PointerMove detects an
                // auto-scroll zone.
                st.drag_state = DragState::Selecting {
                    auto_scroll_v_per_s: 0.0,
                };
                st.preferred_x = None;
                // Click resets the Ctrl+A ladder so a follow-up
                // Ctrl+A starts fresh at level 1.
                st.select_all_level = 0;
                st.select_all_anchor_cell = None;
                // The pointer now owns the caret: stand the typewriter pin down
                // so this click *becomes* the resting position instead of the
                // page lurching to re-centre on it. Cleared by the next
                // keyboard-driven caret move, which resumes pinning.
                st.mouse_anchored = true;
            }
            sync_cursor_signals(state);
            // Reveal the placed caret in any enclosing scroll area, so
            // click-placement is consistent with keyboard caret motion (no-op
            // when the caret is already visible / follow disabled).
            super::keyboard::chase_caret_into_view(state, ctx);
            // The caret moved, so the OS IME candidate window has to move with
            // it — every *keyboard* caret move already reports this and a
            // pointer placement did not, which left the candidate list beside
            // wherever the caret last was typed to. The editor's own reporter,
            // never the controller's: this one holds the focus / read-only /
            // layout guard and the ibus-feedback-loop dedup.
            super::keyboard::report_ime_cursor_area(state, ctx);
            ctx.request_frame();
            // Return Ignored so the gesture arena (DoubleTap /
            // TripleTap) also sees this PointerDown. Returning
            // Handled here would consume the event and the arena
            // would never fire `on_double_tap` / `on_triple_tap`.
            EventResponse::Ignored
        }
        WidgetEvent::PointerMove { position, .. } => {
            // Drag-select extension. The `drag_state` field tells us
            // whether a primary button is still held; if it isn't,
            // we ignore the move.
            let (is_dragging, resizing, viewport_height) = {
                let st = state.borrow();
                let dragging = matches!(st.drag_state, DragState::Selecting { .. });
                let resizing = match &st.drag_state {
                    DragState::ResizingImage { origin, corner, .. } => Some((*origin, *corner)),
                    _ => None,
                };
                (dragging, resizing, st.viewport_height)
            };
            // A press inside the selection that has now travelled far enough:
            // hand the selected text to the framework as a drag. From here the
            // drag belongs to the drag system — this widget's own pointer
            // machine goes back to Idle rather than also trying to select.
            let pending = match &state.borrow().drag_state {
                DragState::PendingTextDrag { origin } => Some(*origin),
                _ => None,
            };
            if let Some(origin) = pending {
                let dx = position.x - origin[0];
                let dy = position.y - origin[1];
                let threshold = {
                    let st = state.borrow();
                    text_drag_threshold(ctx.pointer_kind(), &st.input_tokens)
                };
                if dx * dx + dy * dy < threshold * threshold {
                    return EventResponse::Handled;
                }
                start_text_drag(state, ctx);
                return EventResponse::Handled;
            }
            if let Some((origin, corner)) = resizing {
                let local = to_engine_local(state, position);
                let proposed = proportional_resize(origin, corner, local);
                state.borrow().resize_preview.set(Some(proposed));
                ctx.request_frame();
                return EventResponse::Handled;
            }
            if !is_dragging {
                // Not a drag, so this is a plain hover. In a viewer a plain
                // click follows a link (see the press arm), and a link that
                // acts like a link has to *look* like one under the pointer —
                // without this the reader gets an I-beam over the one thing on
                // the page that is not text to select. Editable surfaces are
                // left alone: there the click places a caret, so an I-beam is
                // the truthful cursor.
                if state.borrow().policy.is_read_only() {
                    let local = to_engine_local(state, position);
                    let over_link = {
                        let st = state.borrow();
                        hit_test::hit_test_at(&st.engine, local, 0.0, 0.0).is_some_and(|hit| {
                            matches!(hit.region, teksilo_text::HitRegion::Link { .. })
                        })
                    };
                    if over_link {
                        ctx.set_cursor(CursorIcon::Pointer);
                    }
                }
                return EventResponse::Ignored;
            }
            let local = to_engine_local(state, position);
            // Clamp y into [2.0, viewport_height - 2.0] before
            // hit-testing so a drag that leaves the viewport still
            // resolves to a valid position on the edge line
            // (matches the godot reference's `handle_drag_select`).
            let clamped = Point::new(
                local.x,
                local.y.clamp(2.0, (viewport_height - 2.0).max(2.0)),
            );
            let hit = {
                let st = state.borrow();
                hit_test::hit_test_at(&st.engine, clamped, 0.0, 0.0)
            };
            if let Some(hit) = hit {
                {
                    let mut st = state.borrow_mut();
                    st.cursor.set_position(hit.position, MoveMode::KeepAnchor);
                    st.cursor_affinity = hit.affinity;
                }
                sync_cursor_signals(state);
            }
            // Compute auto-scroll velocity for the frame loop. The
            // 20 px margin and 60 px/frame max (normalized to
            // 60 * 60 = 3600 px/s so delta scaling matches the
            // reference without depending on refresh rate) come
            // from the godot reference's `compute_drag_velocity`.
            {
                let mut st = state.borrow_mut();
                let v = if local.y < 20.0 {
                    let intensity = ((20.0 - local.y) / 20.0).clamp(0.0, 1.0);
                    -60.0 * 60.0 * intensity
                } else if local.y > viewport_height - 20.0 {
                    let intensity = ((local.y - (viewport_height - 20.0)) / 20.0).clamp(0.0, 1.0);
                    60.0 * 60.0 * intensity
                } else {
                    0.0
                };
                st.drag_state = DragState::Selecting {
                    auto_scroll_v_per_s: v,
                };
                if v != 0.0 {
                    // Drag near an edge keeps the frame loop pumping
                    // so auto-scroll continues without needing the
                    // user to wiggle the mouse.
                    if let Some(handle) = &st.frame_request {
                        handle.set(true);
                    }
                }
            }
            ctx.request_frame();
            EventResponse::Handled
        }
        WidgetEvent::PointerUp { position, .. } => {
            // A press inside the selection that never travelled: it was an
            // ordinary click after all, so honour it now — collapse the
            // selection onto it, which is exactly what the press deferred.
            if matches!(state.borrow().drag_state, DragState::PendingTextDrag { .. }) {
                let local = to_engine_local(state, position);
                let hit = {
                    let st = state.borrow();
                    hit_test::hit_test_at(&st.engine, local, 0.0, 0.0)
                };
                {
                    let mut st = state.borrow_mut();
                    st.drag_state = DragState::Idle;
                    if let Some(hit) = hit {
                        st.cursor.set_position(hit.position, MoveMode::MoveAnchor);
                        st.cursor_affinity = hit.affinity;
                        st.preferred_x = None;
                        st.select_all_level = 0;
                        st.select_all_anchor_cell = None;
                    }
                }
                sync_cursor_signals(state);
                ctx.request_frame();
                return EventResponse::Ignored;
            }
            // A resize is reported once, here — not on every move. The document
            // is the durable record, and rewriting it per pointer move would put
            // a hundred entries on the undo stack for one gesture.
            let finished = {
                let st = state.borrow();
                match (&st.drag_state, st.resize_preview.get()) {
                    (DragState::ResizingImage { name, offset, .. }, Some(rect)) => {
                        Some((st.on_image_resized.clone(), name.clone(), *offset, rect))
                    }
                    _ => None,
                }
            };
            {
                let mut st = state.borrow_mut();
                st.drag_state = DragState::Idle;
                st.resize_preview.set(None);
            }
            if let Some((callback, name, offset, rect)) = finished {
                if let Some(cb) = callback {
                    cb(
                        &super::ImageResize {
                            name,
                            offset,
                            width: rect[2].round().max(1.0) as u32,
                            height: rect[3].round().max(1.0) as u32,
                        },
                        ctx,
                    );
                }
                ctx.request_frame();
                return EventResponse::Handled;
            }
            EventResponse::Ignored
        }
        _ => EventResponse::Ignored,
    }
}

/// A finger or a pen: nothing on the press, everything on a release that still
/// belongs to it.
///
/// `drag_state` is deliberately never armed here, so the precise-pointer
/// `PointerMove` arm above stays inert for a direct pointer without needing a
/// second guard of its own — and with it go the drag-select session, the
/// pending text drag and the image-resize latch, all three of which are press-
/// time commitments a pan cannot coexist with. See the module docs.
///
/// Every arm answers `Ignored`: the press has to keep reaching the gesture arena
/// (the hold that selects a word, the double and triple taps), the release has
/// to keep reaching the tap recognizer, and the whole contact has to stay
/// available to the pan claim the surface installs.
fn handle_direct_pointer_event(
    state: &SharedState,
    touch: &Rc<EditorTouch>,
    v_scrollbar_bounds: &Rc<Cell<Rect>>,
    h_scrollbar_bounds: &Rc<Cell<Rect>>,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    match event {
        WidgetEvent::PointerDown { position, .. } => {
            // The overlay scroll bars run their own drags; a press on one is
            // theirs, and must not clear this contact's hold record.
            if v_scrollbar_bounds.get().contains(*position)
                || h_scrollbar_bounds.get().contains(*position)
            {
                return EventResponse::Ignored;
            }
            // A fresh press: forget any hold this contact's id carried from a
            // previous gesture, so a stale record can never eat a real tap.
            touch.take_hold_consumed(ctx.pointer().id);
            // …and remember where it landed, so the release can tell a tap from
            // a pan. See `EditorTouch::press_is_still_a_tap` for why the
            // framework's coarse tap boundary cannot answer that here.
            touch.mark_press(ctx.pointer().id, ctx.pointer_position());
            // …and take the toolbar down now rather than on the release. Its
            // commands are aimed at a selection this press is about to replace,
            // and a menu that lingers under the finger through the whole press
            // reads as the press having missed.
            touch.hide_toolbar();
            EventResponse::Ignored
        }
        WidgetEvent::PointerUp { button, .. } => {
            if *button != PointerButton::Primary {
                return EventResponse::Ignored;
            }
            // A hold fires from the gesture timer, so its release arrives here
            // after the word is already selected. Placing a caret now would
            // collapse it.
            if touch.take_hold_consumed(ctx.pointer().id) {
                return EventResponse::Ignored;
            }
            // A contact whose press a scrollable above claimed was panning.
            // Nothing it did is a caret placement.
            if !crate::data_views::release_completes_the_press(ctx) {
                return EventResponse::Ignored;
            }
            // The sample's own window position: what the arm above receives is
            // already wrapper-local, and the engine wants body-local.
            let Some(window) = ctx.pointer_position() else {
                return EventResponse::Ignored;
            };
            // …and a contact that travelled further than a tap of its kind may
            // was panning *this* surface, which the predicate above cannot see:
            // the surface is both the press's owner and the pan's claimant, so
            // nothing was claimed elsewhere, and a coarse pointer's tap boundary
            // is the node's whole rectangle.
            let tap_slop = {
                let st = state.borrow();
                st.input_tokens.profile(ctx.pointer_kind()).tap_slop
            };
            if !touch.press_is_still_a_tap(ctx.pointer().id, window, tap_slop) {
                return EventResponse::Ignored;
            }
            let hit = {
                let st = state.borrow();
                let local = super::touch::window_to_engine_local(&st, window);
                hit_test::hit_test_at(&st.engine, local, 0.0, 0.0)
            };
            let Some(hit) = hit else {
                return EventResponse::Ignored;
            };
            match &hit.region {
                // No modifier, in the editable face too — see [`link_follows`].
                // The guard is spelled out rather than assumed, so the one rule
                // stays the only place the decision is made.
                teksilo_text::HitRegion::Link { href }
                    if link_follows(
                        ctx.pointer_kind(),
                        state.borrow().policy.is_read_only(),
                        false,
                    ) =>
                {
                    let callback = state.borrow().on_link_activated.clone();
                    if let Some(cb) = callback {
                        cb(href.as_str(), ctx);
                    }
                    ctx.request_frame();
                    return EventResponse::Ignored;
                }
                teksilo_text::HitRegion::Image { name } => {
                    let callback = state.borrow().on_image_activated.clone();
                    if let Some(cb) = callback {
                        cb(
                            &super::ImageActivation {
                                name: name.clone(),
                                offset: hit.position,
                            },
                            ctx,
                        );
                    }
                    ctx.request_frame();
                    return EventResponse::Ignored;
                }
                _ => {}
            }
            {
                let mut st = state.borrow_mut();
                st.cursor.set_position(hit.position, MoveMode::MoveAnchor);
                st.cursor_affinity = hit.affinity;
                st.preferred_x = None;
                st.select_all_level = 0;
                st.select_all_anchor_cell = None;
                st.mouse_anchored = true;
            }
            sync_cursor_signals(state);
            super::keyboard::chase_caret_into_view(state, ctx);
            super::keyboard::report_ime_cursor_area(state, ctx);
            // A tap places a caret; it does not ask for a menu. The toolbar
            // belongs to a deliberate selection — a hold, a multi-tap, or the
            // end of a handle drag.
            touch.raise(ctx, ToolbarIntent::Hide);
            ctx.request_frame();
            EventResponse::Ignored
        }
        _ => EventResponse::Ignored,
    }
}

/// Select the word under `event` and raise the affordances — the touch hold.
///
/// The mouse refusal is
/// [`TouchSelection::on_long_press`](teksilo_core::text_touch::TouchSelection::on_long_press)'s,
/// not a second copy here: the gesture's own pointer is handed to it and it
/// guards on that rather than on the context, which on a timer-recognised
/// gesture used to answer for the wrong device entirely.
///
/// What stays here is the part core cannot do: the coordinate conversion. No
/// sample is being dispatched, so `pointer_position` is `None`; the tap's own
/// position is wrapper-local and the *editor* does not move mid-press, so
/// `local + node_origin` is exact — the same arithmetic
/// [`to_engine_local`] already does in the other direction.
pub(super) fn handle_long_press(
    state: &SharedState,
    touch: &Rc<EditorTouch>,
    event: &teksilo_core::gesture::TapEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    let window = {
        let st = state.borrow();
        Point::new(
            event.position.x + st.node_origin.x,
            event.position.y + st.node_origin.y,
        )
    };
    if !touch.select_word_at(event.pointer, window, ctx) {
        return EventResponse::Ignored;
    }
    touch.mark_hold_consumed(event.pointer.id);
    ctx.request_frame();
    EventResponse::Handled
}

/// Select word under the caret on double-click.
pub(super) fn handle_double_tap(state: &SharedState, pos: Point, ctx: &mut EventContext) {
    tap_select(state, pos, SelectionType::WordUnderCursor);
    super::keyboard::chase_caret_into_view(state, ctx);
    ctx.request_frame();
}

/// Select block under the caret on triple-click. Matches the godot reference's
/// own triple-click rule.
pub(super) fn handle_triple_tap(state: &SharedState, pos: Point, ctx: &mut EventContext) {
    tap_select(state, pos, SelectionType::BlockUnderCursor);
    super::keyboard::chase_caret_into_view(state, ctx);
    ctx.request_frame();
}

fn tap_select(state: &SharedState, pos: Point, kind: SelectionType) {
    // Double/triple-click is pointer-driven selection: same rule as a plain
    // click — the pin stands down rather than yanking the page under a
    // selection the user is making with the mouse.
    state.borrow_mut().mouse_anchored = true;
    let local = to_engine_local(state, &pos);
    let hit = {
        let st = state.borrow();
        hit_test::hit_test_at(&st.engine, local, 0.0, 0.0)
    };
    if let Some(hit) = hit {
        let st = state.borrow();
        st.cursor.set_position(hit.position, MoveMode::MoveAnchor);
        st.cursor.select(kind);
        drop(st);
        sync_cursor_signals(state);
    }
}

/// Convert a **window**-space point (the coordinate a context-menu factory is
/// handed by `show_context_menu_for`) into engine/body-local space. The body's
/// top-left in the window is `viewport_origin`, so `window - viewport_origin`
/// lands in the space `hit_test` expects — no `node_origin` term, because the
/// input is already a window point (unlike [`to_engine_local`], whose input is
/// wrapper-local).
fn engine_local_of_window(state: &SharedState, window_position: Point) -> Point {
    let st = state.borrow();
    Point::new(
        window_position.x - st.viewport_origin.x,
        window_position.y - st.viewport_origin.y,
    )
}

/// Reposition the caret to a right-click point **in window coordinates** when
/// the click lands *outside* the current selection — the platform convention
/// for "right-click, then Cut / Copy / Paste (/ add word) at the new caret". A
/// click *inside* the selection preserves it, so the menu's Cut/Copy still act
/// on the selection.
///
/// This has to run from inside the context-menu factory: the editor never sees
/// the Secondary `PointerDown` through its own pointer handler, because
/// `teksilo-core`'s `show_context_menu_for` consumes it (and returns early)
/// before `dispatch_to_widget` is ever called. Mirrors the single-line
/// [`TextInputField`](crate::primitives::text_input_field)'s behavior.
pub(super) fn reposition_caret_for_context_menu(state: &SharedState, window_position: Point) {
    let local = engine_local_of_window(state, window_position);
    let hit = {
        let st = state.borrow();
        hit_test::hit_test_at(&st.engine, local, 0.0, 0.0)
    };
    let Some(hit) = hit else {
        return;
    };
    {
        let st = state.borrow();
        if st.cursor.has_selection() {
            let (lo, hi) = (st.cursor.selection_start(), st.cursor.selection_end());
            if hit.position >= lo && hit.position <= hi {
                // Click inside the selection — keep it so Cut/Copy act on it.
                return;
            }
        }
    }
    {
        let mut st = state.borrow_mut();
        st.cursor.set_position(hit.position, MoveMode::MoveAnchor);
        st.cursor_affinity = hit.affinity;
        st.preferred_x = None;
        st.select_all_level = 0;
        st.select_all_anchor_cell = None;
    }
    sync_cursor_signals(state);
}

/// Hit-test a **window**-space point to a document char offset — the primitive
/// behind [`EditorHandle::offset_at_point`](super::EditorHandle::offset_at_point).
/// `None` when the point resolves to no text.
/// Move the caret to the drop point under a hovering drag, in the coordinate
/// space the widget's own drag handlers receive (widget-local).
///
/// A drag with no caret under it asks the writer to aim at nothing: they can see
/// the pointer but not where the text will land, and the two are never the same
/// place because a caret snaps to a character boundary. So the caret follows the
/// drag, and dropping puts the payload exactly where the caret already is.
///
/// Returns `false` when the pointer is not over any text, so the caller can
/// decline the drop rather than insert somewhere arbitrary.
pub(super) fn move_caret_for_drag(state: &SharedState, local: Point) -> bool {
    let hit = {
        let st = state.borrow();
        hit_test::hit_test_at(&st.engine, to_engine_local(state, &local), 0.0, 0.0)
    };
    let Some(hit) = hit else { return false };
    {
        let mut st = state.borrow_mut();
        // Collapsed, not extended: a drop replaces nothing, and leaving an
        // anchor behind would make the insertion look like it was about to
        // overwrite the selection it came from.
        st.cursor.set_position(hit.position, MoveMode::MoveAnchor);
        st.cursor_affinity = hit.affinity;
        // Show it even though the drag holds no focus here — see `drop_caret`.
        st.drop_caret = true;
    }
    sync_cursor_signals(state);
    true
}

/// Stop showing the drop caret — the drag left, was cancelled, or has landed.
pub(super) fn clear_drop_caret(state: &SharedState) {
    state.borrow_mut().drop_caret = false;
}

pub(super) fn offset_at_window_point(state: &SharedState, window_position: Point) -> Option<usize> {
    let local = engine_local_of_window(state, window_position);
    let st = state.borrow();
    hit_test::hit_test_at(&st.engine, local, 0.0, 0.0).map(|hit| hit.position)
}

#[cfg(test)]
mod resize_tests {
    use super::*;

    /// A 200×100 picture at (50, 20) — deliberately not square, so a fix that
    /// silently squares it up would show.
    const IMG: [f32; 4] = [50.0, 20.0, 200.0, 100.0];
    const BOTTOM_RIGHT: (f32, f32) = (1.0, 1.0);
    const TOP_LEFT: (f32, f32) = (0.0, 0.0);

    fn ratio(rect: [f32; 4]) -> f32 {
        rect[2] / rect[3]
    }

    #[test]
    fn dragging_a_corner_keeps_the_proportions() {
        // Straight out along the diagonal, then a drag that is mostly
        // horizontal, then one that is mostly vertical. All three must keep the
        // 2:1 shape — that is the whole promise of the gesture.
        for target in [
            Point::new(450.0, 220.0),
            Point::new(450.0, 130.0),
            Point::new(260.0, 320.0),
        ] {
            let out = proportional_resize(IMG, BOTTOM_RIGHT, target);
            assert!(
                (ratio(out) - ratio(IMG)).abs() < 0.001,
                "proportions drifted: {out:?} is {:.3}, wanted {:.3}",
                ratio(out),
                ratio(IMG)
            );
        }
    }

    #[test]
    fn dragging_outward_grows_and_inward_shrinks() {
        let bigger = proportional_resize(IMG, BOTTOM_RIGHT, Point::new(450.0, 220.0));
        assert!(bigger[2] > IMG[2], "{bigger:?}");
        let smaller = proportional_resize(IMG, BOTTOM_RIGHT, Point::new(150.0, 70.0));
        assert!(smaller[2] < IMG[2], "{smaller:?}");
    }

    #[test]
    fn the_opposite_corner_is_what_stays_put() {
        // Grabbing the TOP-LEFT measures against the bottom-right, so dragging
        // up and left must GROW the picture. A version that always measured
        // from the top-left would shrink it here — the sign error that makes
        // two of the four grips feel inverted.
        let out = proportional_resize(IMG, TOP_LEFT, Point::new(0.0, 0.0));
        assert!(
            out[2] > IMG[2],
            "dragging the top-left up and out must grow: {out:?}"
        );
    }

    #[test]
    fn a_resize_cannot_shrink_the_image_out_of_reach() {
        // Dragged far past the anchor. A picture with no area has no grips, so
        // it could never be resized back — the drag has to stop short.
        let out = proportional_resize(IMG, BOTTOM_RIGHT, Point::new(51.0, 21.0));
        assert!(out[2] >= RESIZE_MIN_EDGE, "{out:?}");
        assert!(out[3] >= RESIZE_MIN_EDGE, "{out:?}");
        assert!(
            (ratio(out) - ratio(IMG)).abs() < 0.001,
            "the floor must not distort it: {out:?}"
        );
    }

    #[test]
    fn the_preview_stays_at_the_images_own_place_in_the_text() {
        // Whatever corner is dragged, the result is anchored at the original
        // top-left: the picture sits in a text flow, so the reflow decides
        // where it lands. Showing it anywhere else previews a position the
        // relayout is about to contradict.
        for corner in [TOP_LEFT, BOTTOM_RIGHT, (1.0, 0.0), (0.0, 1.0)] {
            let out = proportional_resize(IMG, corner, Point::new(400.0, 300.0));
            assert_eq!((out[0], out[1]), (IMG[0], IMG[1]), "{corner:?} moved it");
        }
    }

    #[test]
    fn a_degenerate_image_is_left_alone() {
        let zero = [10.0, 10.0, 0.0, 0.0];
        assert_eq!(
            proportional_resize(zero, BOTTOM_RIGHT, Point::new(99.0, 99.0)),
            zero
        );
    }

    // ── grabbing a grip ─────────────────────────────────────────────────

    #[test]
    fn each_corner_is_grabbable_from_either_side_of_its_edge() {
        // A grip straddles the corner, so it must answer both from inside the
        // picture and from just outside it — the outside half is the reason the
        // engine's own hit-test cannot do this job.
        for (fx, fy) in super::super::paint::RESIZE_CORNERS {
            let (cx, cy) = (IMG[0] + IMG[2] * fx, IMG[1] + IMG[3] * fy);
            for (dx, dy) in [(0.0, 0.0), (-4.0, -4.0), (4.0, 4.0)] {
                assert_eq!(
                    handle_at(IMG, Point::new(cx + dx, cy + dy)),
                    Some((fx, fy)),
                    "corner {fx},{fy} missed at offset {dx},{dy}"
                );
            }
        }
    }

    #[test]
    fn the_middle_of_the_picture_grabs_nothing() {
        // Otherwise clicking a picture to select it would start a resize.
        let centre = Point::new(IMG[0] + IMG[2] / 2.0, IMG[1] + IMG[3] / 2.0);
        assert_eq!(handle_at(IMG, centre), None);
    }

    #[test]
    fn a_press_well_clear_of_a_corner_grabs_nothing() {
        // Twenty pixels out on both axes: prose beside the picture must stay
        // ordinary prose.
        assert_eq!(
            handle_at(IMG, Point::new(IMG[0] - 20.0, IMG[1] - 20.0)),
            None
        );
        assert_eq!(
            handle_at(
                IMG,
                Point::new(IMG[0] + IMG[2] + 20.0, IMG[1] + IMG[3] + 20.0)
            ),
            None
        );
    }

    #[test]
    fn the_edges_between_corners_grab_nothing() {
        // Corners only — this resize keeps proportions, so an edge grip would
        // promise a stretch it will not perform.
        let mid_top = Point::new(IMG[0] + IMG[2] / 2.0, IMG[1]);
        let mid_left = Point::new(IMG[0], IMG[1] + IMG[3] / 2.0);
        assert_eq!(handle_at(IMG, mid_top), None);
        assert_eq!(handle_at(IMG, mid_left), None);
    }
}

/// Test doors onto the pure geometry above, so the aiming rules can be checked
/// without a widget tree, a window, or a pointer device.
#[cfg(test)]
mod test_support {
    use super::*;

    pub(crate) fn grip_reach_for_test(tokens: &InputTokens, rect: [f32; 4]) -> f32 {
        grip_reach(tokens, rect)
    }

    pub(crate) fn handle_near_for_test(
        rect: [f32; 4],
        local: Point,
        reach: f32,
    ) -> Option<(f32, f32)> {
        handle_near(rect, local, reach)
    }

    pub(crate) fn corner_at_for_test(
        rect: [f32; 4],
        local: Point,
        tokens: &InputTokens,
    ) -> Option<(f32, f32)> {
        corner_at(rect, local, tokens)
    }

    pub(crate) fn text_drag_threshold_for_test(kind: PointerKind, tokens: &InputTokens) -> f32 {
        text_drag_threshold(kind, tokens)
    }

    pub(crate) fn link_follows_for_test(
        kind: PointerKind,
        read_only: bool,
        command_held: bool,
    ) -> bool {
        link_follows(kind, read_only, command_held)
    }
}

#[cfg(test)]
pub(super) use test_support::{
    corner_at_for_test, grip_reach_for_test, handle_near_for_test, link_follows_for_test,
    text_drag_threshold_for_test,
};
