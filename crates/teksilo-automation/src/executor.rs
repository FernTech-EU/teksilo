// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The single core function that performs every automation operation
//! against a live [`WidgetTree`].
//!
//! ```text
//! execute(tree: &mut WidgetTree, ops: &mut dyn WindowOps,
//!         op: &AutomationOp, settle: &SettleSpec) -> AutomationReply
//! ```
//!
//! `WidgetTree` is `Rc/RefCell`-based and therefore `!Send`, so it lives on
//! exactly one thread; the async / socket layers marshal `Send` DTOs to it
//! and call this. Two ops can't be served here — `ListWindows` (needs the
//! window manager) and `Screenshot` (needs a GPU / platform window). Both
//! return [`codes::HOST_REQUIRED`]; the headless tree-thread and the live
//! bridge intercept them with the extra context they alone hold.

use std::time::{Duration, Instant};

use teksilo_canvas::{Point, Rect};
use teksilo_core::WidgetTree;
use teksilo_core::accesskit;
use teksilo_core::event::{ButtonMask, Key, Modifiers, PointerButton, ScrollDelta, WidgetEvent};
use teksilo_core::pointer::touch_action::TouchAction;
use teksilo_core::pointer::{
    PointerId, PointerInfo, PointerPhase, PointerSample, ScrollSample, ScrollSource,
};
use teksilo_core::widget_id::WidgetId;
use teksilo_core::window::WindowOps;

use crate::dto::{
    AnnouncementDto, Assertion, AssertionResult, AutomationOp, AutomationReply, NodeBounds,
    NodeRef, PointerKindDto, PointerReport, SemanticNode, SequenceMemberDto, SettleSpec,
    ShortcutInfo, TouchPhaseDto, TouchSequenceReport, TouchStep, TouchStepReport, WaitCondition,
    codes,
};

/// Perform one automation operation. See the module docs.
///
/// Whatever the operation, the tree is handed back to the wall clock before the
/// reply returns — see [`WidgetTree::resume_real_time`]. Advancing the
/// simulated clock freezes both the input timeline and the animation clock so
/// the operation is deterministic; leaving them frozen would stop a *live*
/// attached window from ever measuring another gesture, or advancing another
/// animation frame.
pub fn execute(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    op: &AutomationOp,
    settle: &SettleSpec,
) -> AutomationReply {
    let reply = execute_op(tree, ops, op, settle);
    // One site rather than one per time-moving arm: any op can reach a settle,
    // and an arm added later must not have to remember this.
    tree.resume_real_time();
    reply
}

fn execute_op(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    op: &AutomationOp,
    settle: &SettleSpec,
) -> AutomationReply {
    match op {
        // ---- Query ----
        AutomationOp::SnapshotTree { max_depth } => {
            let update = tree.sync_accessibility();
            AutomationReply::ok(snapshot_json(&update, *max_depth))
        }
        AutomationOp::ReadNode { node } => {
            let update = tree.sync_accessibility();
            match find_node(&update, *node) {
                Some(sn) => AutomationReply::ok_json(&sn),
                None => AutomationReply::err(codes::NOT_FOUND, format!("no node {node}")),
            }
        }
        AutomationOp::LayoutTree {
            max_depth,
            include_debug,
        } => AutomationReply::ok(layout_tree_json(tree, *max_depth, *include_debug)),
        AutomationOp::InspectNode { node } => {
            let nid = accesskit::NodeId(*node);
            if teksilo_core::accessibility::is_synthetic(nid) {
                return AutomationReply::err(
                    codes::NOT_FOUND,
                    "synthetic node has no backing widget — use read_node for its AT detail",
                );
            }
            let widget = teksilo_core::accessibility::node_id_to_widget_id_maybe(nid)
                .filter(|w| tree.widget_type_name(*w).is_some());
            match widget {
                Some(w) => AutomationReply::ok_json(&layout_node(tree, w, true)),
                None => {
                    AutomationReply::err(codes::NOT_FOUND, format!("no widget for node {node}"))
                }
            }
        }
        AutomationOp::FindNode { role, label } => {
            let update = tree.sync_accessibility();
            let found = find_node_ref(&update, role.as_deref(), label.as_deref());
            AutomationReply::ok(serde_json::json!({ "node": found }))
        }
        AutomationOp::AssertNode { node, assertion } => {
            let update = tree.sync_accessibility();
            // A false assertion is a *failure*, not a successful report of one.
            //
            // It used to come back as `Ok(AssertionResult { passed: false })`,
            // which every transport and every caller had to remember to unwrap
            // and check — and a caller that forgot got a green result for a
            // failed assertion, which is the worst possible default for a
            // testing tool. The MCP server carried a bolt-on that re-read its
            // own JSON payload to set `is_error`; the socket bridge and every
            // direct `execute` caller had nothing.
            //
            // Deciding it here means every transport inherits it: rmcp already
            // maps `AutomationReply::Err` to `CallToolResult::error`, so
            // `isError` falls out with no special case.
            match evaluate_assertion(&update, *node, assertion) {
                Ok(result) => AutomationReply::ok_json(&result),
                Err(reply) => reply,
            }
        }
        AutomationOp::ListWindows => AutomationReply::err(
            codes::HOST_REQUIRED,
            "list_windows is served by the host (window manager / headless shim)",
        ),

        // ---- AT-action driving ----
        AutomationOp::InvokeAction { node, action } => {
            let Some(act) = action_from_str(action) else {
                return AutomationReply::err(
                    codes::UNKNOWN_NAME,
                    format!("unknown action '{action}'"),
                );
            };
            dispatch_action_and_settle(tree, ops, settle, *node, act, None)
        }
        AutomationOp::FocusNode { node } => {
            dispatch_action_and_settle(tree, ops, settle, *node, accesskit::Action::Focus, None)
        }
        AutomationOp::SetValue { node, value } => dispatch_action_and_settle(
            tree,
            ops,
            settle,
            *node,
            accesskit::Action::SetValue,
            Some(accesskit::ActionData::Value(value.clone().into_boxed_str())),
        ),
        AutomationOp::Expand { node } => {
            dispatch_action_and_settle(tree, ops, settle, *node, accesskit::Action::Expand, None)
        }
        AutomationOp::Collapse { node } => {
            dispatch_action_and_settle(tree, ops, settle, *node, accesskit::Action::Collapse, None)
        }
        AutomationOp::Scroll {
            node,
            dx,
            dy,
            ctrl,
            shift,
            alt,
            meta,
            command,
        } => {
            let update = tree.sync_accessibility();
            let Some(widget) = resolve_widget(tree, &update, *node) else {
                return AutomationReply::err(codes::NOT_FOUND, format!("no node {node}"));
            };
            let c = center(tree.bounds(widget));
            // Route the wheel: hover the target first (scroll dispatches to
            // the hovered/focused widget), then deliver the delta.
            let m = modifiers(*ctrl, *shift, *alt, *meta, *command);
            pointer_move(tree, ops, c, m);
            // Modifiers are carried, not hardcoded to `NONE`: a modifier-held
            // wheel is a distinct gesture (Ctrl-wheel-to-zoom is why
            // `WidgetEvent::Scroll` has this field at all), and a probe that
            // could only send a bare wheel could not reach it.
            //
            // A `ScrollSample` rather than the bare `WidgetEvent::scroll`
            // constructor, for one field: the source. This scroll is
            // [`ScrollSource::Programmatic`] — the app scrolled itself — and it
            // used to arrive as `Wheel`, because that is what
            // `InputSnapshot::from_event` gives a legacy event that names no
            // source. A widget that branches on the source (one notch = one
            // item for a wheel, follow-exactly for a driver) was therefore told
            // a driven scroll was a user turning a wheel. Everything else about
            // the sample is what it always was: no position, so it still routes
            // by hover; `ScrollPhase::Discrete`; the singular mouse pointer.
            // The route is unchanged too — `ScrollDelivery::for_source` sends
            // only `TouchPan` down the pan-claimant chain, so a programmatic
            // scroll bubbles exactly as a wheel notch does and no pan claimant
            // competes for it.
            let mut sample =
                ScrollSample::wheel(ScrollDelta::Pixels { x: *dx, y: *dy }, m, tree.input_now());
            sample.source = ScrollSource::Programmatic;
            tree.dispatch_scroll_with_ops(sample, ops);
            finish_settle(tree, ops, settle)
        }

        // ---- Synthetic input ----
        AutomationOp::InjectPointer {
            x,
            y,
            action,
            button,
            kind,
            pointer_id,
            pressure,
            tilt,
            ctrl,
            shift,
            alt,
            meta,
            command,
        } => {
            let p = Point::new(*x, *y);
            let m = modifiers(*ctrl, *shift, *alt, *meta, *command);
            let outcome = match kind {
                // The pre-touch path, unchanged: a legacy `PointerDown` /
                // `PointerUp` pair, whose `InputSnapshot` names
                // `PointerInfo::mouse`. Kept as its own arm rather than folded
                // into a `PointerSample` for uniformity, because the two lower
                // to the same widget events only as long as nothing about the
                // sample differs, and a mouse is the one device every existing
                // script and every existing assertion was written against.
                PointerKindDto::Mouse => inject_mouse(
                    tree,
                    ops,
                    p,
                    button.to_core(),
                    m,
                    *action,
                    *pointer_id,
                    *pressure,
                    *tilt,
                ),
                PointerKindDto::Touch | PointerKindDto::Pen => inject_direct(
                    tree,
                    ops,
                    p,
                    m,
                    *action,
                    *kind,
                    *pointer_id,
                    *pressure,
                    *tilt,
                ),
            };
            if let Err(reply) = outcome {
                return reply;
            }
            finish_settle(tree, ops, settle)
        }
        AutomationOp::InjectTouchSequence { steps } => match run_touch_sequence(tree, ops, steps) {
            Err(reply) => reply,
            Ok(step_reports) => match run_settle(tree, ops, settle) {
                Some(code) => AutomationReply::err(code, "settle exceeded its time budget"),
                None => AutomationReply::ok_json(&TouchSequenceReport {
                    steps: step_reports,
                    live: live_pointer_reports(tree),
                }),
            },
        },
        AutomationOp::Pinch {
            ax0,
            ay0,
            bx0,
            by0,
            ax1,
            ay1,
            bx1,
            by1,
            steps,
        } => {
            // Both contacts land before either moves: the recognizer's
            // reference span is the distance between the two landings, so a
            // pinch whose second finger arrives after the first has moved
            // describes a different gesture entirely.
            let steps = (*steps).max(1);
            let (a0, b0) = (Point::new(*ax0, *ay0), Point::new(*bx0, *by0));
            let (a1, b1) = (Point::new(*ax1, *ay1), Point::new(*bx1, *by1));
            let mut plan = vec![
                TouchStep {
                    contact: 0,
                    phase: TouchPhaseDto::Down,
                    x: a0.x,
                    y: a0.y,
                    advance_ms: 0,
                },
                TouchStep {
                    contact: 1,
                    phase: TouchPhaseDto::Down,
                    x: b0.x,
                    y: b0.y,
                    advance_ms: 0,
                },
            ];
            for step in 1..=steps {
                let t = step as f32 / steps as f32;
                let a = lerp(a0, a1, t);
                let b = lerp(b0, b1, t);
                plan.push(TouchStep {
                    contact: 0,
                    phase: TouchPhaseDto::Move,
                    x: a.x,
                    y: a.y,
                    advance_ms: 0,
                });
                plan.push(TouchStep {
                    contact: 1,
                    phase: TouchPhaseDto::Move,
                    x: b.x,
                    y: b.y,
                    advance_ms: 0,
                });
            }
            plan.push(TouchStep {
                contact: 0,
                phase: TouchPhaseDto::Up,
                x: a1.x,
                y: a1.y,
                advance_ms: 0,
            });
            plan.push(TouchStep {
                contact: 1,
                phase: TouchPhaseDto::Up,
                x: b1.x,
                y: b1.y,
                advance_ms: 0,
            });
            match run_touch_sequence(tree, ops, &plan) {
                Err(reply) => reply,
                Ok(_) => finish_settle(tree, ops, settle),
            }
        }
        AutomationOp::Fling {
            from_x,
            from_y,
            to_x,
            to_y,
            over_ms,
        } => {
            let from = Point::new(*from_x, *from_y);
            let to = Point::new(*to_x, *to_y);
            // Sampled at one 60 Hz frame per step, and never fewer steps than
            // the velocity tracker's minimum sample count: a flick described by
            // two far-apart samples yields no velocity at all, and would report
            // success while silently never flinging. The cadence is a duration,
            // not a whole number of milliseconds, which is why this walks the
            // path itself instead of building `TouchStep`s (whose `advance_ms`
            // is milliseconds, as a wire field should be).
            let total = Duration::from_millis(*over_ms);
            let steps = (total.as_micros() as u64)
                .div_ceil(FLING_SAMPLE_INTERVAL.as_micros() as u64)
                .max(MIN_FLING_SAMPLES);
            let per_step = total / steps as u32;
            freeze_clock(tree, ops);
            let id = mint_or_reuse(tree, teksilo_tokens::PointerKind::Touch, None);
            direct_dispatch(
                tree,
                ops,
                id,
                teksilo_tokens::PointerKind::Touch,
                PointerPhase::Down,
                from,
                true,
                Modifiers::NONE,
                None,
                None,
            );
            for step in 1..=steps {
                tree.advance_time_with_ops(per_step, ops);
                let t = step as f32 / steps as f32;
                direct_dispatch(
                    tree,
                    ops,
                    id,
                    teksilo_tokens::PointerKind::Touch,
                    PointerPhase::Move,
                    lerp(from, to, t),
                    true,
                    Modifiers::NONE,
                    None,
                    None,
                );
            }
            direct_dispatch(
                tree,
                ops,
                id,
                teksilo_tokens::PointerKind::Touch,
                PointerPhase::Up,
                to,
                false,
                Modifiers::NONE,
                None,
                None,
            );
            finish_settle(tree, ops, settle)
        }
        AutomationOp::LongPress { x, y, kind } => {
            let p = Point::new(*x, *y);
            // The hold is the *device's* threshold, read off the active input
            // profile rather than written here: the recognizer fires at
            // `>= long_press`, so a script that picked its own number would
            // either miss the threshold on a device it did not know about or
            // stop the threshold itself from ever being asserted.
            let hold = tree.theme().input.profile(kind.to_core()).long_press;
            freeze_clock(tree, ops);
            match kind {
                PointerKindDto::Mouse => {
                    pointer_down(tree, ops, p, PointerButton::Primary, Modifiers::NONE);
                    tree.advance_time_with_ops(hold, ops);
                    pointer_up(tree, ops, p, PointerButton::Primary, Modifiers::NONE);
                }
                PointerKindDto::Touch | PointerKindDto::Pen => {
                    let core_kind = kind.to_core();
                    let id = mint_or_reuse(tree, core_kind, None);
                    // A resting tip reports the pressure a press has; the lift
                    // reports none, exactly as a digitizer does.
                    direct_dispatch(
                        tree,
                        ops,
                        id,
                        core_kind,
                        PointerPhase::Down,
                        p,
                        true,
                        Modifiers::NONE,
                        Some(0.5),
                        None,
                    );
                    tree.advance_time_with_ops(hold, ops);
                    direct_dispatch(
                        tree,
                        ops,
                        id,
                        core_kind,
                        PointerPhase::Up,
                        p,
                        false,
                        Modifiers::NONE,
                        Some(0.0),
                        None,
                    );
                }
            }
            finish_settle(tree, ops, settle)
        }
        AutomationOp::CancelPointer { pointer_id } => {
            let Some(info) = tree.live_pointers().find(|i| i.id.get() == *pointer_id) else {
                return AutomationReply::err(
                    codes::NOT_FOUND,
                    format!("no live pointer {pointer_id}; call query_pointers for the live set"),
                );
            };
            let at = tree
                .pointer_position(info.id)
                .unwrap_or(Point::new(0.0, 0.0));
            freeze_clock(tree, ops);
            direct_dispatch(
                tree,
                ops,
                info.id,
                info.kind,
                PointerPhase::Cancel,
                at,
                false,
                Modifiers::NONE,
                None,
                None,
            );
            finish_settle(tree, ops, settle)
        }
        AutomationOp::QueryPointers => AutomationReply::ok_json(&live_pointer_reports(tree)),
        AutomationOp::SetDensity { density } => {
            tree.set_input_density(density.to_core());
            finish_settle(tree, ops, settle)
        }
        AutomationOp::RightClick { node } => {
            // Resolve the node's pointer point (prefers its own AT bounds, so a
            // synthetic child — scene item, rich-text run — is right-clicked at
            // its own centre, not its owning widget's), then drive a real
            // Secondary press+release. That runs the same `PointerDown`
            // → `show_context_menu_for` path a user's right-click does, so the
            // widget's `.context_menu(..)` factory opens.
            let update = tree.sync_accessibility();
            let resolved = teksilo_core::accessibility::audit::logical_bounds(&update);
            let Some(p) = node_point(tree, &update, &resolved, *node) else {
                return AutomationReply::err(codes::NOT_FOUND, format!("no node {node}"));
            };
            pointer_down(
                tree,
                ops,
                p,
                teksilo_core::PointerButton::Secondary,
                Modifiers::NONE,
            );
            pointer_up(
                tree,
                ops,
                p,
                teksilo_core::PointerButton::Secondary,
                Modifiers::NONE,
            );
            finish_settle(tree, ops, settle)
        }
        AutomationOp::InjectKey {
            key,
            ctrl,
            shift,
            alt,
            meta,
            command,
        } => {
            let Some(k) = key_from_str(key) else {
                return AutomationReply::err(codes::UNKNOWN_NAME, format!("unknown key '{key}'"));
            };
            press_key(
                tree,
                ops,
                k,
                modifiers(*ctrl, *shift, *alt, *meta, *command),
            );
            finish_settle(tree, ops, settle)
        }
        AutomationOp::TypeText { node, text } => {
            let update = tree.sync_accessibility();
            let Some(widget) = resolve_widget(tree, &update, *node) else {
                return AutomationReply::err(codes::NOT_FOUND, format!("no node {node}"));
            };
            // `type_text` routes to the *focused* widget, so focus first.
            focus_for_typing(tree, ops, widget);
            type_text(tree, ops, text);
            finish_settle(tree, ops, settle)
        }
        AutomationOp::TypeIme {
            node,
            preedit,
            commit,
        } => {
            let update = tree.sync_accessibility();
            let Some(widget) = resolve_widget(tree, &update, *node) else {
                return AutomationReply::err(codes::NOT_FOUND, format!("no node {node}"));
            };
            focus_for_typing(tree, ops, widget);
            if let Some(text) = preedit {
                tree.dispatch_event_with_ops(
                    WidgetEvent::ImeComposition {
                        text: text.clone(),
                        cursor: None,
                    },
                    ops,
                );
            }
            if let Some(text) = commit {
                tree.dispatch_event_with_ops(WidgetEvent::ImeCommit { text: text.clone() }, ops);
            }
            finish_settle(tree, ops, settle)
        }
        AutomationOp::DragNode {
            node,
            to_node,
            to_x,
            to_y,
        } => {
            let update = tree.sync_accessibility();
            let resolved = teksilo_core::accessibility::audit::logical_bounds(&update);
            let Some(from) = node_point(tree, &update, &resolved, *node) else {
                return AutomationReply::err(codes::NOT_FOUND, format!("no node {node}"));
            };
            let to = if let Some(tn) = to_node {
                match node_point(tree, &update, &resolved, *tn) {
                    Some(p) => p,
                    None => {
                        return AutomationReply::err(codes::NOT_FOUND, format!("no node {tn}"));
                    }
                }
            } else if let (Some(x), Some(y)) = (to_x, to_y) {
                Point::new(*x, *y)
            } else {
                return AutomationReply::err(
                    codes::BAD_ARGUMENT,
                    "drag_node needs to_node or (to_x, to_y)",
                );
            };
            drag(tree, ops, from, to);
            finish_settle(tree, ops, settle)
        }

        // ---- Introspection ----
        AutomationOp::GetOverlays => {
            let overlays = tree.active_overlays();
            let ids: Vec<String> = overlays.iter().map(|o| format!("{o:?}")).collect();
            AutomationReply::ok(serde_json::json!({ "count": overlays.len(), "ids": ids }))
        }
        AutomationOp::GetShortcuts => {
            let list: Vec<ShortcutInfo> = tree
                .shortcut_registry()
                .iter_effective()
                .map(|eff| ShortcutInfo {
                    id: eff.shortcut.id.to_string(),
                    name: Some(eff.shortcut.name.get()).filter(|n| !n.is_empty()),
                    primary: eff.primary.map(format_keystroke),
                    secondary: eff.secondary.map(format_keystroke),
                    enabled: eff.enabled,
                })
                .collect();
            AutomationReply::ok_json(&list)
        }
        AutomationOp::ListLiveRegions => {
            let update = tree.sync_accessibility();
            let focus = update.focus;
            let semantic_ctx = SemanticContext::new(&update);
            // Only the regions a platform adapter walks. A hidden live region,
            // or one inside a hidden subtree, is no region any reader has: the
            // adapters announce nothing from a node their filter excludes, and
            // the framework's own idle announcer nodes are exactly that.
            let reachable = teksilo_core::accessibility::audit::nodes_in_filtered_tree(&update);
            let regions: Vec<SemanticNode> = update
                .nodes
                .iter()
                .filter(|(id, n)| {
                    matches!(
                        n.live(),
                        Some(accesskit::Live::Polite) | Some(accesskit::Live::Assertive)
                    ) && reachable.contains(id)
                })
                .map(|(id, n)| semantic_node(*id, n, focus, &semantic_ctx))
                .collect();
            AutomationReply::ok_json(&regions)
        }
        AutomationOp::PullAnnouncements { since_seq } => {
            // Re-sync so the latest rebuild's announcements are captured.
            tree.sync_accessibility();
            let list: Vec<AnnouncementDto> = tree
                .announcements_since(*since_seq)
                .into_iter()
                .map(AnnouncementDto::from)
                .collect();
            AutomationReply::ok_json(&list)
        }

        // ---- Time / settle ----
        AutomationOp::AdvanceClock { millis } => {
            // Over the caller's sink: a long press this jump recognizes runs
            // its handler, and that handler may open a window.
            tree.advance_time_with_ops(Duration::from_millis(*millis), ops);
            tree.sync_accessibility();
            AutomationReply::ok_unit()
        }
        AutomationOp::Settle => finish_settle(tree, ops, settle),
        AutomationOp::WaitForCondition { condition } => {
            wait_for_condition(tree, ops, settle, condition)
        }

        // ---- Visual (host-handled) ----
        AutomationOp::Screenshot { .. } => AutomationReply::err(
            codes::HOST_REQUIRED,
            "screenshot pixels are produced by the host (offscreen renderer / platform window)",
        ),
    }
}

// ---------------------------------------------------------------------------
// Synthetic input
// ---------------------------------------------------------------------------
//
// `WidgetTree`'s `test_api` has a ready-made method for each of these
// (`pointer_move`, `press_key`, `drag`, …) and this module used to call them.
// It must not: every one of them goes through
// [`WidgetTree::dispatch_event`](teksilo_core::WidgetTree::dispatch_event), the
// *standalone-tree* variant, which substitutes a `NoopWindowOps` — and
// `NoopWindowOps::open_window` **panics**, by design, because a standalone tree
// has no winit back-end to create a window in.
//
// Against a live app that is the wrong sink and it is not a degraded one: the
// executor is handed the real `WindowOpsImpl` (`teksilo-app`'s `run_in_window`)
// and then threw it away, so an injected click or keystroke on any command that
// opens, enumerates or focuses a window took the whole application down —
// mid-probe, with a panic naming a "standalone WidgetTree" that the automated
// session plainly was not. Found driving a real "New Window" menu command.
//
// The AT-action ops (`invoke_action` and friends) never had the problem: they
// go through `dispatch_action_and_settle`, which passes `ops` down. So did
// `scroll`. These helpers make the synthetic-input ops behave the same way —
// same events, same order, real ops.

// The three below are the `PointerKindDto::Mouse` path only — a `touch` or `pen`
// injection builds a real `PointerSample` and enters through the tree's pointer
// door instead (see `inject_pointer`), so the mouse the `WidgetEvent`
// constructors default to is the device these describe.

/// `modifiers` is threaded rather than defaulted for the same reason the wheel's
/// are: a drag reads Shift and Ctrl from the *move*, so a probe that could only
/// send a bare move could not reach a Shift-extend selection or a Ctrl-additive
/// marquee — and it would report success while doing it.
fn pointer_move(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    position: Point,
    modifiers: Modifiers,
) {
    tree.dispatch_event_with_ops(WidgetEvent::pointer_move_with(position, modifiers), ops);
}

fn pointer_down(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    position: Point,
    button: teksilo_core::PointerButton,
    modifiers: Modifiers,
) {
    tree.dispatch_event_with_ops(WidgetEvent::pointer_down(position, button, modifiers), ops);
}

fn pointer_up(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    position: Point,
    button: teksilo_core::PointerButton,
    modifiers: Modifiers,
) {
    tree.dispatch_event_with_ops(WidgetEvent::pointer_up(position, button, modifiers), ops);
}

/// Press and release one key, carrying the text the platform attaches to it
/// ([`Key::to_text`]) so a driven run matches a hand-driven one.
///
/// It sent `text: None` for every key, which made `inject_key` a *weaker*
/// probe than a real keypress rather than an equivalent one — an Escape that
/// a focused field swallowed came back through this path looking fine.
fn press_key(tree: &mut WidgetTree, ops: &mut dyn WindowOps, key: Key, modifiers: Modifiers) {
    tree.dispatch_event_with_ops(
        WidgetEvent::KeyDown {
            key,
            modifiers,
            text: key.to_text().map(str::to_string),
        },
        ops,
    );
    tree.dispatch_event_with_ops(WidgetEvent::KeyUp { key, modifiers }, ops);
}

/// Give the keyboard to `widget`, or to the editor it stands for.
///
/// An editor (`RichTextEditor`, `CodeEditor`, `LogView`) takes focus and the
/// keys on its own widget and publishes its text, and its focus, on a body
/// that is not focusable, so the body is the node a caller finds as focused or
/// by the editor's name. Focusing the body would still pass the keys up to the
/// editor, but blur it: no caret, no input method, no focus ring.
fn focus_for_typing(tree: &mut WidgetTree, ops: &mut dyn WindowOps, widget: WidgetId) {
    let target = tree.focusable_composite_behind(widget).unwrap_or(widget);
    tree.focus_ops(target, ops);
}

/// Type `text` into the focused widget, one `KeyDown` per character — the
/// caller focuses the target first (`focus_ops`). Mirrors `test_api::type_text`,
/// whose `widget` parameter is likewise unused: focus is what routes a key
/// event, not the node the caller named.
fn type_text(tree: &mut WidgetTree, ops: &mut dyn WindowOps, text: &str) {
    for ch in text.chars() {
        tree.dispatch_event_with_ops(
            WidgetEvent::KeyDown {
                key: Key::Character(ch),
                modifiers: Modifiers::NONE,
                text: Some(ch.to_string()),
            },
            ops,
        );
    }
}

// ---------------------------------------------------------------------------
// Direct pointers — touch and pen
// ---------------------------------------------------------------------------
//
// A touch or pen op builds a real
// [`PointerSample`](teksilo_core::PointerSample) and pushes it through
// [`dispatch_pointer_with_ops`](teksilo_core::WidgetTree::dispatch_pointer_with_ops),
// the tree's one pointer ingress door — never a fabricated `WidgetEvent`. The
// hit-test-by-kind, the per-kind slop, the pointer table, the cross-widget
// sequence, the pan session, the palm watch and the pinch feed all hang off
// that door, so a helper that stepped around it would exercise the helper
// rather than the framework.
//
// `WidgetTree`'s own A21 test helpers build samples of exactly this shape, and
// this module deliberately does not call them: every one of them ends in
// `dispatch_pointer`, the standalone variant that substitutes a
// `NoopWindowOps` whose `open_window` panics. That is the trap the "Synthetic
// input" section above was written for, and a finger is no more exempt from it
// than a mouse — a long press this module recognizes runs a handler, and that
// handler may open a window.
//
// The sample's shape mirrors `teksilo-platform`'s translator: a contact holds
// `ButtonMask::PRIMARY` while it is down and reports `Some(Primary)` on the two
// phases that change a button; a stylus adds its axes.

/// The cadence a [`AutomationOp::Fling`]
/// samples at: one 60 Hz frame, which is under the velocity tracker's
/// [`STOP_GAP`](teksilo_core::kinetic::STOP_GAP) and therefore never splits a
/// flick into two unrelated runs.
const FLING_SAMPLE_INTERVAL: Duration = Duration::from_micros(16_667);

/// The fewest moves a fling may be described by — the velocity tracker's own
/// minimum, below which it reports no velocity and the flick silently coasts
/// nowhere.
const MIN_FLING_SAMPLES: u64 = teksilo_core::kinetic::MIN_SAMPLE_SIZE as u64;

/// Linear interpolation between two points, `t` in `0.0..=1.0`. Every
/// multi-sample op walks its path with this, so a pinch and a fling place their
/// intermediate samples by one rule.
fn lerp(from: Point, to: Point, t: f32) -> Point {
    Point::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t)
}

/// Put the tree on the simulated clock before an op's first sample.
///
/// A zero-duration advance, because [`WidgetTree::advance_time_with_ops`] is
/// the one door onto simulated time and the switch itself is `pub(super)` to
/// `widget_tree`. It is the same tick an `advance_clock {millis: 0}` runs, and
/// it is what makes an op's samples deterministic: on the wall clock two
/// consecutive samples are stamped however many microseconds apart the host
/// happened to run two lines of code, so a drag meaning "travel 200 dp, no time
/// passes" would instead describe a flick at some thousands of dp per second —
/// differently on every machine. Once frozen, the interval between two samples
/// is exactly what the op advanced and nothing else.
///
/// `execute` hands the clock back at its single exit, so the freeze never
/// outlives the op.
fn freeze_clock(tree: &mut WidgetTree, ops: &mut dyn WindowOps) {
    tree.advance_time_with_ops(Duration::ZERO, ops);
}

/// The identity a direct-pointer op should use.
///
/// An explicit id wins. Failing that a **pen** reuses the live stylus if there
/// is one — a stylus is singular and it hovers, so its table entry outlives a
/// lift and the next sample continues the same session — while a **finger**
/// always mints: a backend reuses its own contact ids the moment a finger
/// lifts, so identity is per press.
fn mint_or_reuse(
    tree: &WidgetTree,
    kind: teksilo_tokens::PointerKind,
    explicit: Option<PointerId>,
) -> PointerId {
    if let Some(id) = explicit {
        return id;
    }
    if matches!(kind, teksilo_tokens::PointerKind::Pen(_))
        && let Some(info) = tree
            .live_pointers()
            .find(|i| matches!(i.kind, teksilo_tokens::PointerKind::Pen(_)))
    {
        return info.id;
    }
    tree.new_contact()
}

/// Build one direct-pointer sample and push it through the tree's pointer door.
#[allow(clippy::too_many_arguments)]
fn direct_dispatch(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    id: PointerId,
    kind: teksilo_tokens::PointerKind,
    phase: PointerPhase,
    at: Point,
    down: bool,
    modifiers: Modifiers,
    pressure: Option<f32>,
    tilt: Option<[f32; 2]>,
) {
    let mut pointer = PointerInfo::touch(id, tree.input_now());
    pointer.kind = kind;
    pointer.buttons = if down {
        ButtonMask::PRIMARY
    } else {
        ButtonMask::NONE
    };
    pointer.axes.pressure = pressure;
    pointer.axes.tilt = tilt.map(|[x, y]| (x, y));
    let sample = PointerSample {
        pointer,
        phase,
        position: at,
        // The translator reports a button only where one changed.
        button: match phase {
            PointerPhase::Down | PointerPhase::Up => Some(PointerButton::Primary),
            PointerPhase::Move | PointerPhase::Cancel => None,
        },
        modifiers,
        coalesced: Vec::new(),
    };
    tree.dispatch_pointer_with_ops(sample, ops);
}

/// The mouse arm of `inject_pointer`: the pre-touch path, and a refusal for the
/// three fields a mouse cannot carry.
///
/// Refusing rather than ignoring, because this op's whole contract is that an
/// argument it does not act on is an error — a silently-dropped `pressure` on a
/// mouse is the same defect as a silently-dropped misspelled field, and the
/// caller who wrote it believed they had said something.
#[allow(clippy::too_many_arguments)]
fn inject_mouse(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    p: Point,
    button: PointerButton,
    m: Modifiers,
    action: crate::dto::PointerAction,
    pointer_id: Option<u64>,
    pressure: Option<f32>,
    tilt: Option<[f32; 2]>,
) -> Result<(), AutomationReply> {
    use crate::dto::PointerAction as PA;
    if pointer_id.is_some() {
        return Err(AutomationReply::err(
            codes::BAD_ARGUMENT,
            "a mouse has one identity — pointer_id is only meaningful for kind=touch or kind=pen",
        ));
    }
    if pressure.is_some() || tilt.is_some() {
        return Err(AutomationReply::err(
            codes::BAD_ARGUMENT,
            "pressure and tilt are digitizer axes — set kind=pen to send them",
        ));
    }
    match action {
        PA::Move => pointer_move(tree, ops, p, m),
        PA::Down => pointer_down(tree, ops, p, button, m),
        PA::Up => pointer_up(tree, ops, p, button, m),
        PA::Click => {
            pointer_down(tree, ops, p, button, m);
            pointer_up(tree, ops, p, button, m);
        }
        PA::DoubleClick => {
            // Both pairs in one op, with no settle between them: a
            // client sending two `Click` ops cannot make a double-click,
            // because the round trip between them is longer than the
            // recogniser's window.
            pointer_down(tree, ops, p, button, m);
            pointer_up(tree, ops, p, button, m);
            pointer_down(tree, ops, p, button, m);
            pointer_up(tree, ops, p, button, m);
        }
    }
    Ok(())
}

/// The touch / pen arm of `inject_pointer`.
///
/// A `down` mints a contact and a `click` is a whole contact's life, so both
/// make their own identity. A `move` or an `up` continues one that is already
/// live, and may name it with `pointer_id`; naming it is optional exactly while
/// it is unambiguous — one live pointer of that kind — because a script driving
/// two fingers through separate ops that never said which one it meant would
/// move whichever the table happened to yield first. The one phase that may
/// find nothing live and still proceed is a **pen** `move`: a stylus moves in
/// proximity without ever having been down, which is the framework's only
/// direct-pointer hover.
#[allow(clippy::too_many_arguments)]
fn inject_direct(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    p: Point,
    m: Modifiers,
    action: crate::dto::PointerAction,
    kind: PointerKindDto,
    pointer_id: Option<u64>,
    pressure: Option<f32>,
    tilt: Option<[f32; 2]>,
) -> Result<(), AutomationReply> {
    use crate::dto::PointerAction as PA;
    let core_kind = kind.to_core();
    // `pointer_id` addresses a contact that is *already live*, so it belongs to
    // exactly the phases that continue one. A `down` mints an identity and a
    // click is a whole contact's life; accepting an id there would let a caller
    // name a finger that the op is about to replace, and believe they had.
    let addressable = matches!(action, PA::Move | PA::Up);
    if pointer_id.is_some() && !addressable {
        return Err(AutomationReply::err(
            codes::BAD_ARGUMENT,
            "pointer_id addresses a contact that is already down — it belongs              on a move or an up, not on a down or a click, which mint their own",
        ));
    }
    let explicit = match pointer_id {
        None => None,
        Some(raw) => match tree.live_pointers().find(|i| i.id.get() == raw) {
            Some(info) => Some(info.id),
            None => {
                return Err(AutomationReply::err(
                    codes::NOT_FOUND,
                    format!("no live pointer {raw}; call query_pointers for the live set"),
                ));
            }
        },
    };
    freeze_clock(tree, ops);
    match action {
        PA::Down => {
            let id = mint_or_reuse(tree, core_kind, None);
            direct_dispatch(
                tree,
                ops,
                id,
                core_kind,
                PointerPhase::Down,
                p,
                true,
                m,
                pressure,
                tilt,
            );
        }
        PA::Move | PA::Up => {
            // A pen in proximity moves without ever having been down, so a
            // `move` may mint one; a finger cannot, and says so.
            let id = match explicit {
                Some(id) => id,
                None => match sole_live_of_kind(tree, core_kind) {
                    Ok(Some(id)) => id,
                    Ok(None) if matches!(action, PA::Move) && kind == PointerKindDto::Pen => {
                        tree.new_contact()
                    }
                    Ok(None) => {
                        return Err(AutomationReply::err(
                            codes::BAD_ARGUMENT,
                            "no live pointer of that kind — a move or up must follow a down",
                        ));
                    }
                    Err(n) => {
                        return Err(AutomationReply::err(
                            codes::BAD_ARGUMENT,
                            format!(
                                "{n} live pointers of that kind — name one with pointer_id, \
                                 or drive the whole gesture with inject_touch_sequence"
                            ),
                        ));
                    }
                },
            };
            // Whether the sample reports a held button is read off the table,
            // not assumed from the phase: a pen in proximity moves with nothing
            // down, and a finger dragging has `PRIMARY` held. An `up` is a
            // release, so it holds nothing whatever the table said.
            let phase = if matches!(action, PA::Move) {
                PointerPhase::Move
            } else {
                PointerPhase::Up
            };
            let down = phase == PointerPhase::Move
                && tree
                    .live_pointers()
                    .find(|i| i.id == id)
                    .is_some_and(|i| !i.buttons.is_empty());
            direct_dispatch(tree, ops, id, core_kind, phase, p, down, m, pressure, tilt);
        }
        PA::Click | PA::DoubleClick => {
            let repeats = if matches!(action, PA::DoubleClick) {
                2
            } else {
                1
            };
            for _ in 0..repeats {
                // A fresh contact per tap: a finger that lifts and lands again
                // is a new contact, which is what makes the two taps a
                // double-tap rather than one contact reporting twice.
                let id = mint_or_reuse(tree, core_kind, None);
                direct_dispatch(
                    tree,
                    ops,
                    id,
                    core_kind,
                    PointerPhase::Down,
                    p,
                    true,
                    m,
                    pressure,
                    tilt,
                );
                direct_dispatch(
                    tree,
                    ops,
                    id,
                    core_kind,
                    PointerPhase::Up,
                    p,
                    false,
                    m,
                    pressure,
                    tilt,
                );
            }
        }
    }
    Ok(())
}

/// The one live pointer of `kind`, `Ok(None)` if there is none, or `Err(n)` if
/// there are `n > 1` and the caller must say which.
fn sole_live_of_kind(
    tree: &WidgetTree,
    kind: teksilo_tokens::PointerKind,
) -> Result<Option<PointerId>, usize> {
    let matches_kind = |k: teksilo_tokens::PointerKind| match kind {
        teksilo_tokens::PointerKind::Pen(_) => matches!(k, teksilo_tokens::PointerKind::Pen(_)),
        other => k == other,
    };
    let ids: Vec<PointerId> = tree
        .live_pointers()
        .filter(|i| matches_kind(i.kind))
        .map(|i| i.id)
        .collect();
    match ids.len() {
        0 => Ok(None),
        1 => Ok(Some(ids[0])),
        n => Err(n),
    }
}

/// Drive a whole multi-touch gesture, reporting the arbitration after every
/// step.
///
/// The observation is taken **immediately** after each sample and before any
/// settle: an arbitration is decided sample by sample, so a report taken after
/// the gesture had been settled would answer for the end of the press and not
/// for the step that was asked about.
fn run_touch_sequence(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    steps: &[TouchStep],
) -> Result<Vec<TouchStepReport>, AutomationReply> {
    if steps.is_empty() {
        return Err(AutomationReply::err(
            codes::BAD_ARGUMENT,
            "inject_touch_sequence needs at least one step",
        ));
    }
    freeze_clock(tree, ops);
    let mut slots: std::collections::BTreeMap<u32, PointerId> = std::collections::BTreeMap::new();
    let mut reports = Vec::with_capacity(steps.len());
    for step in steps {
        if step.advance_ms > 0 {
            tree.advance_time_with_ops(Duration::from_millis(step.advance_ms), ops);
        }
        let at = Point::new(step.x, step.y);
        let id = match step.phase {
            TouchPhaseDto::Down => {
                let id = tree.new_contact();
                slots.insert(step.contact, id);
                id
            }
            _ => match slots.get(&step.contact) {
                Some(id) => *id,
                None => {
                    return Err(AutomationReply::err(
                        codes::BAD_ARGUMENT,
                        format!(
                            "contact {} is not down — a move, up or cancel must follow a down",
                            step.contact
                        ),
                    ));
                }
            },
        };
        let (phase, down) = match step.phase {
            TouchPhaseDto::Down => (PointerPhase::Down, true),
            TouchPhaseDto::Move => (PointerPhase::Move, true),
            TouchPhaseDto::Up => (PointerPhase::Up, false),
            TouchPhaseDto::Cancel => (PointerPhase::Cancel, false),
        };
        direct_dispatch(
            tree,
            ops,
            id,
            teksilo_tokens::PointerKind::Touch,
            phase,
            at,
            down,
            Modifiers::NONE,
            None,
            None,
        );
        if matches!(step.phase, TouchPhaseDto::Up | TouchPhaseDto::Cancel) {
            slots.remove(&step.contact);
        }
        reports.push(TouchStepReport {
            contact: step.contact,
            pointer_id: id.get(),
            pointer: pointer_report_for(tree, id),
        });
    }
    Ok(reports)
}

/// Every live pointer, reported.
fn live_pointer_reports(tree: &WidgetTree) -> Vec<PointerReport> {
    let infos: Vec<PointerInfo> = tree.live_pointers().collect();
    infos.into_iter().map(|i| pointer_report(tree, i)).collect()
}

/// One pointer's report, or `None` if it is no longer in the table — a finger
/// is simply gone after its up or cancel, and reporting a stale copy would let
/// a script assert a winner for a pointer that no longer exists.
fn pointer_report_for(tree: &WidgetTree, id: PointerId) -> Option<PointerReport> {
    let info = tree.live_pointers().find(|i| i.id == id)?;
    Some(pointer_report(tree, info))
}

fn pointer_report(tree: &WidgetTree, info: PointerInfo) -> PointerReport {
    PointerReport {
        pointer_id: info.id.get(),
        kind: PointerKindDto::from_core(info.kind),
        primary: info.primary,
        down: !info.buttons.is_empty(),
        position: tree.pointer_position(info.id).map(|p| [p.x, p.y]),
        pressure: info.axes.pressure,
        tilt: info.axes.tilt.map(|(x, y)| [x, y]),
        captured_by: tree.captured_by(info.id).map(node_ref_of),
        touch_action: render_touch_action(tree.sequence_touch_action(info.id)).to_string(),
        sequence_members: tree
            .sequence_members(info.id)
            .into_iter()
            .map(|(id, role, state)| SequenceMemberDto {
                node: node_ref_of(id),
                role: member_role_name(role),
                state: member_state_name(state),
            })
            .collect(),
        sequence_winner: tree.sequence_winner(info.id).map(node_ref_of),
    }
}

fn node_ref_of(id: WidgetId) -> NodeRef {
    teksilo_core::accessibility::widget_id_to_node_id(id).0
}

/// A `TouchAction` under the name it is declared by. `Debug` prints the raw bit
/// field, which no client can decode. The same seven names the generated
/// arbitration-matrix table in `docs/events-and-gestures.md` uses.
fn render_touch_action(action: TouchAction) -> &'static str {
    match action {
        TouchAction::AUTO => "AUTO",
        TouchAction::NONE => "NONE",
        TouchAction::PAN => "PAN",
        TouchAction::PAN_X => "PAN_X",
        TouchAction::PAN_Y => "PAN_Y",
        TouchAction::PINCH_ZOOM => "PINCH_ZOOM",
        TouchAction::MANIPULATION => "MANIPULATION",
        _ => "(composite)",
    }
}

/// The wire name for a member's role. `MemberRole::Pan` carries the claim it
/// was enrolled for; the wire names the role, because the axes are already
/// pinned by the frozen touch action beside it.
///
/// `MemberRole` is `#[non_exhaustive]`: a role added later renders as its
/// `Debug` form rather than panicking. A DTO builder that panicked would take
/// down the thread that owns the `!Send` tree, and with it every later op.
fn member_role_name(role: teksilo_core::gesture::MemberRole) -> String {
    use teksilo_core::gesture::MemberRole as R;
    match role {
        R::Gesture => "gesture".to_string(),
        R::Pan(_) => "pan".to_string(),
        R::RawDrag => "raw_drag".to_string(),
        R::RawPreview => "raw_preview".to_string(),
        other => format!("{other:?}"),
    }
}

fn member_state_name(state: teksilo_core::gesture::MemberState) -> String {
    use teksilo_core::gesture::MemberState as S;
    match state {
        S::Possible => "possible".to_string(),
        S::Held => "held".to_string(),
        S::Rejected => "rejected".to_string(),
        S::Won => "won".to_string(),
        other => format!("{other:?}"),
    }
}

fn drag(tree: &mut WidgetTree, ops: &mut dyn WindowOps, from: Point, to: Point) {
    pointer_down(
        tree,
        ops,
        from,
        teksilo_core::PointerButton::Primary,
        Modifiers::NONE,
    );
    // `DragNode` names no modifiers, so there are none to thread; `NONE` here
    // matches the press and the release either side of it.
    pointer_move(tree, ops, to, Modifiers::NONE);
    pointer_up(
        tree,
        ops,
        to,
        teksilo_core::PointerButton::Primary,
        Modifiers::NONE,
    );
}

// ---------------------------------------------------------------------------
// Settle
// ---------------------------------------------------------------------------

/// Run the settle described by `settle` and then re-sync the AT tree.
/// Returns `Some(code)` if the wall-clock budget was exceeded (the loop is
/// sim-clock-driven, so it can only overrun on a pathological animation),
/// else `None`.
pub fn run_settle(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    settle: &SettleSpec,
) -> Option<&'static str> {
    let deadline = Instant::now() + Duration::from_millis(settle.settle_timeout_ms.max(1));
    // One door for both halves of the settle. The requested clock jump and the
    // animation frames below go through the same `advance_time`, so the
    // simulated clock moves by exactly `clock_millis + 16 * frames` and the
    // gesture, fling, tooltip and animation deadlines all land on it. When
    // these were two calls they advanced the clock twice and moved disjoint
    // halves of the tree.
    if settle.clock_millis > 0 {
        tree.advance_time_with_ops(Duration::from_millis(settle.clock_millis), ops);
    }
    let mut frames = 0u32;
    let mut timed_out = false;
    while tree.has_active_animations() && frames < settle.max_anim_frames {
        tree.advance_time_with_ops(Duration::from_millis(16), ops);
        frames += 1;
        if Instant::now() >= deadline {
            timed_out = true;
            break;
        }
    }
    if settle.layout_after {
        let proposal = tree.last_proposal();
        tree.layout_with_ops(proposal, ops);
    }
    tree.sync_accessibility();
    // `pub`, and called directly by the headless tree-thread and the live
    // bridge as well as from `execute`. Hand the clock back here too, so a
    // caller that never goes through `execute` does not leave a live window
    // frozen. Idempotent — `execute` calling it again is free.
    tree.resume_real_time();
    timed_out.then_some(codes::SETTLE_TIMEOUT)
}

fn finish_settle(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    settle: &SettleSpec,
) -> AutomationReply {
    match run_settle(tree, ops, settle) {
        Some(code) => AutomationReply::err(code, "settle exceeded its time budget"),
        None => AutomationReply::ok_unit(),
    }
}

fn dispatch_action_and_settle(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    settle: &SettleSpec,
    node: NodeRef,
    action: accesskit::Action,
    data: Option<accesskit::ActionData>,
) -> AutomationReply {
    // Sync first so the synthetic-parent map is fresh AND so we can confirm
    // the node is actually live: `node_id_to_widget_id_maybe` happily decodes
    // any non-synthetic u64 into a `WidgetId`, so presence in the AT tree —
    // not a non-`None` resolution — is the real liveness check.
    let update = tree.sync_accessibility();
    if !node_present(&update, node) {
        return AutomationReply::err(codes::NOT_FOUND, format!("no node {node}"));
    }
    let handled = tree.dispatch_access_action(accesskit::NodeId(node), action, data, ops);
    // Settle regardless: an action that WAS handled must have its effects
    // flushed before we reply, and one that wasn't costs a frame at most.
    let reply = finish_settle(tree, ops, settle);
    if handled || !reply.is_ok() {
        return reply;
    }
    // The node is real but nothing acted on the action. Reporting success here
    // is what made an unsupported action indistinguishable from a working one,
    // so name the actions the node does advertise — that is almost always
    // enough for the caller to fix the call.
    let advertised = find_node(&update, node)
        .map(|sn| sn.actions.join(", "))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "none".to_string());
    AutomationReply::err(
        codes::UNHANDLED_ACTION,
        format!(
            "node {node} did not handle '{}'; it advertises: {advertised}",
            action_name(action).unwrap_or("?")
        ),
    )
}

/// One simulated frame at ~60 Hz — how far the wait advances the tree per poll.
const WAIT_FRAME: Duration = Duration::from_millis(16);

/// Poll `condition`, advancing the simulated clock a frame at a time.
///
/// `settle.settle_timeout_ms` is spent as **simulated** time, in whole frames,
/// so the same wait resolves identically on every platform. Bounding it by wall
/// clock — what this used to do — made the outcome depend on the host's timer
/// granularity: each poll slept 1 ms, but a 1 ms sleep costs up to 15.6 ms on
/// Windows, so the same wall budget bought roughly a fifteenth of the frames
/// and a fifteenth of the simulated time. A wait that passed on Linux timed out
/// on Windows, with nothing in the reply to say why.
///
/// Nothing else can move the tree while this runs — the tree is `!Send` and this
/// loop owns the only thread that touches it — so simulated frames are the only
/// thing that can make the predicate true, and sleeping bought no progress at
/// all. Dropping the sleep also removes the reason it was there: the loop is now
/// bounded by a frame count rather than by the timeout, so it finishes in
/// microseconds instead of occupying the thread for the whole budget.
fn wait_for_condition(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    settle: &SettleSpec,
    condition: &WaitCondition,
) -> AutomationReply {
    let budget_ms = settle.settle_timeout_ms.max(1);
    let frame_ms = WAIT_FRAME.as_millis() as u64;
    let max_frames = budget_ms.div_ceil(frame_ms);
    // Wall-clock backstop: it exists only so a pathological tree (an unbounded
    // rebuild each frame) cannot spin forever, and must never be what ends an
    // ordinary wait. `budget_ms` is already enormously generous for that job —
    // the frames are pure in-memory work and finish in microseconds — while a
    // larger multiple would break the guarantee the *caller* of this budget is
    // relying on: the live bridge clamps `settle_timeout_ms` to 2 s precisely
    // so no op can freeze the winit main thread for longer (see
    // `clamp_live_settle`), and a 10× backstop quietly turned that into 20 s —
    // past even the bridge's own 15 s reply deadline, so the client would be
    // told the request timed out while the UI stayed frozen.
    let backstop =
        Instant::now() + Duration::from_millis(budget_ms).max(Duration::from_millis(250));

    // `..=` so a budget of one frame still gets an initial check *and* a frame.
    for _ in 0..=max_frames {
        let update = tree.sync_accessibility();
        if condition_met(tree, &update, condition) {
            return AutomationReply::ok_unit();
        }
        if Instant::now() >= backstop {
            break;
        }
        // Drive timed / animated state forward one frame, then re-layout so
        // reactive (AccessibilityOnly) bindings flush before the next sync.
        // ONE call: `advance_time` moves everything the frame is meant to move,
        // and pairing it with a second door advanced the simulated clock by two
        // frames per poll while `WAIT_FRAME` and this function's own doc both
        // say one.
        tree.advance_time_with_ops(WAIT_FRAME, ops);
        let proposal = tree.last_proposal();
        tree.layout_with_ops(proposal, ops);
    }
    AutomationReply::err(
        codes::WAIT_TIMEOUT,
        "wait_for_condition timed out before the predicate held",
    )
}

fn condition_met(
    tree: &WidgetTree,
    update: &accesskit::TreeUpdate,
    condition: &WaitCondition,
) -> bool {
    match condition {
        WaitCondition::NodeExists { role, label } => {
            find_node_ref(update, role.as_deref(), label.as_deref()).is_some()
        }
        WaitCondition::NodeValue { node, expected } => update
            .nodes
            .iter()
            .find(|(id, _)| id.0 == *node)
            .map(|(_, n)| n.value() == Some(expected.as_str()))
            .unwrap_or(false),
        WaitCondition::NodeGone { node } => !update.nodes.iter().any(|(id, _)| id.0 == *node),
        WaitCondition::AtVersionAtLeast { version } => tree.at_version().get() >= *version,
    }
}

// ---------------------------------------------------------------------------
// Node / tree helpers
// ---------------------------------------------------------------------------

/// Whether `node` is present in the freshly-synced AT tree (the reliable
/// liveness check — see [`dispatch_action_and_settle`]).
fn node_present(update: &accesskit::TreeUpdate, node: NodeRef) -> bool {
    update.nodes.iter().any(|(id, _)| id.0 == node)
}

/// Resolve a *present* [`NodeRef`] to its owning [`WidgetId`] — directly for
/// a widget node, or via the synthetic-parent map for a widget-emitted
/// child. Returns `None` when the node isn't in the live tree.
fn resolve_widget(
    tree: &WidgetTree,
    update: &accesskit::TreeUpdate,
    node: NodeRef,
) -> Option<WidgetId> {
    if !node_present(update, node) {
        return None;
    }
    let nid = accesskit::NodeId(node);
    teksilo_core::accessibility::node_id_to_widget_id_maybe(nid)
        .or_else(|| tree.widget_for_synthetic(nid))
}

fn center(r: Rect) -> Point {
    Point::new(r.x + r.width * 0.5, r.y + r.height * 0.5)
}

/// The pointer point to use when driving a gesture at `node`. Prefers the
/// node's own AT bounds — correct for *synthetic* children (scene items,
/// rich-text runs) whose owning widget may span far more area than the child
/// — and falls back to the owning widget's arena bounds. `None` only when the
/// node is absent from the live tree.
///
/// **Resolved, not raw.** A node's `bounds` are stated in the space of the
/// nearest ancestor declaring a transform, which for anything inside a
/// `SceneView` is the scene's, not the window's. Aiming a synthetic press at
/// the raw rectangle put it where the *model* says the card is rather than
/// where it is painted — correct only at pan zero. The fallback has the same
/// problem for the same reason (arena bounds under a content transform are
/// scene coordinates), so it goes through the resolved map too where the node
/// is in it.
fn node_point(
    tree: &WidgetTree,
    update: &accesskit::TreeUpdate,
    resolved: &std::collections::HashMap<accesskit::NodeId, teksilo_canvas::Rect>,
    node: NodeRef,
) -> Option<Point> {
    if let Some(rect) = resolved.get(&accesskit::NodeId(node)) {
        return Some(center(*rect));
    }
    let widget = resolve_widget(tree, update, node)?;
    Some(center(tree.bounds(widget)))
}

/// What a probe needs about a whole update that only the consumer can
/// answer: which nodes are reviewable, and what name each announces once
/// `labelled_by` is resolved.
///
/// Computed once per snapshot — building a consumer tree is O(nodes), so
/// asking node by node would be quadratic.
struct SemanticContext {
    text: std::collections::HashMap<
        accesskit::NodeId,
        teksilo_core::accessibility::audit::NodeTextInfo,
    >,
    names: std::collections::HashMap<accesskit::NodeId, String>,
    /// Each node's rectangle with the transforms above it composed in, in
    /// logical pixels. The raw property is the node's box in a space the node
    /// does not name, which is the window's for most of the tree and the
    /// scene's inside a `SceneView`.
    bounds: std::collections::HashMap<accesskit::NodeId, teksilo_canvas::Rect>,
}

impl SemanticContext {
    fn new(update: &accesskit::TreeUpdate) -> Self {
        use teksilo_core::accessibility::audit;
        Self {
            text: audit::text_infos(update),
            names: audit::resolved_names(update),
            bounds: audit::logical_bounds(update),
        }
    }
}

/// Build a [`SemanticNode`] from a raw AccessKit node.
fn semantic_node(
    id: accesskit::NodeId,
    node: &accesskit::Node,
    focus: accesskit::NodeId,
    ctx: &SemanticContext,
) -> SemanticNode {
    let toggled = node.toggled().map(|t| {
        match t {
            accesskit::Toggled::True => "true",
            accesskit::Toggled::False => "false",
            accesskit::Toggled::Mixed => "mixed",
        }
        .to_string()
    });
    let live = match node.live() {
        Some(accesskit::Live::Polite) => Some("polite".to_string()),
        Some(accesskit::Live::Assertive) => Some("assertive".to_string()),
        _ => None,
    };
    let bounds = ctx.bounds.get(&id).map(|r| NodeBounds {
        x: r.x as f64,
        y: r.y as f64,
        width: r.width as f64,
        height: r.height as f64,
    });
    let actions = ADVERTISABLE_ACTIONS
        .iter()
        .filter(|(a, _)| node.supports_action(*a))
        .map(|(_, name)| name.to_string())
        .collect();
    // Runs are excluded from object navigation by the consumer's own
    // filter, so a reader never lands on one; listing them as children
    // would bury every snapshot under nodes nobody can reach. They stay in
    // `nodes` and stay addressable.
    let children: Vec<_> = node
        .children()
        .iter()
        .filter(|_| node.role() != accesskit::Role::TextRun)
        .map(|c| c.0)
        .collect();
    let own_label = node.label().map(|s| s.to_string());
    let label = ctx.names.get(&id).cloned().or_else(|| own_label.clone());
    let raw_label = own_label.filter(|own| Some(own) != label.as_ref());
    let text = ctx.text.get(&id).map(|info| crate::dto::TextRangeInfo {
        run_count: info.run_count,
        document_text: info.document_text.clone(),
        has_geometry: info.has_geometry,
        direction: info.direction.map(|d| {
            match d {
                accesskit::TextDirection::LeftToRight => "left_to_right",
                accesskit::TextDirection::RightToLeft => "right_to_left",
                accesskit::TextDirection::TopToBottom => "top_to_bottom",
                accesskit::TextDirection::BottomToTop => "bottom_to_top",
            }
            .to_string()
        }),
    });
    SemanticNode {
        id: id.0,
        role: format!("{:?}", node.role()),
        label,
        raw_label,
        text,
        value: node.value().map(|s| s.to_string()),
        description: node.description().map(|s| s.to_string()),
        toggled,
        expanded: node.is_expanded(),
        selected: node.is_selected(),
        level: node.level(),
        disabled: node.is_disabled(),
        focused: id == focus,
        live,
        numeric_value: node.numeric_value(),
        bounds,
        actions,
        children,
    }
}

/// Build a [`LayoutNode`](crate::dto::LayoutNode) for one arena widget.
fn layout_node(tree: &WidgetTree, id: WidgetId, include_debug: bool) -> crate::dto::LayoutNode {
    let to_ref = |w: WidgetId| teksilo_core::accessibility::widget_id_to_node_id(w).0;
    let b = tree.bounds(id);
    crate::dto::LayoutNode {
        id: to_ref(id),
        type_name: tree
            .widget_type_name(id)
            .map(|s| s.to_string())
            .unwrap_or_else(|| "?".to_string()),
        bounds: NodeBounds {
            x: b.x as f64,
            y: b.y as f64,
            width: b.width as f64,
            height: b.height as f64,
        },
        active: tree.is_active(id),
        clips_children: tree.widget_clips_children(id),
        parent: tree.parent(id).map(to_ref),
        children: tree.children(id).into_iter().map(to_ref).collect(),
        debug: if include_debug {
            tree.widget_debug_string(id)
        } else {
            None
        },
    }
}

/// Walk the arena widget tree from the roots (BFS, depth-capped), keying every
/// widget by the same `NodeRef` space as the AT tools.
fn layout_tree_json(
    tree: &WidgetTree,
    max_depth: Option<usize>,
    include_debug: bool,
) -> serde_json::Value {
    use std::collections::{HashSet, VecDeque};
    let to_ref = |w: WidgetId| teksilo_core::accessibility::widget_id_to_node_id(w).0;
    let roots = tree.roots();
    let mut out: Vec<crate::dto::LayoutNode> = Vec::new();
    let mut seen: HashSet<WidgetId> = HashSet::new();
    let mut queue: VecDeque<(WidgetId, usize)> = roots.iter().map(|r| (*r, 0usize)).collect();
    while let Some((id, depth)) = queue.pop_front() {
        if !seen.insert(id) {
            continue;
        }
        let descend = max_depth.map(|d| depth < d).unwrap_or(true);
        let mut node = layout_node(tree, id, include_debug);
        if !descend {
            // At the cap: drop child refs so there are no dangling ids.
            node.children.clear();
        }
        out.push(node);
        if descend {
            for c in tree.children(id) {
                queue.push_back((c, depth + 1));
            }
        }
    }
    serde_json::json!({
        "roots": roots.into_iter().map(to_ref).collect::<Vec<_>>(),
        "nodes": out,
    })
}

fn find_node(update: &accesskit::TreeUpdate, node: NodeRef) -> Option<SemanticNode> {
    let focus = update.focus;
    let semantic_ctx = SemanticContext::new(update);
    update
        .nodes
        .iter()
        .find(|(id, _)| id.0 == node)
        .map(|(id, n)| semantic_node(*id, n, focus, &semantic_ctx))
}

/// First node (in AT/build order) whose role and/or label match. A `None`
/// filter matches anything; role compares against the role's `Debug` name
/// case-insensitively; label compares for exact equality.
fn find_node_ref(
    update: &accesskit::TreeUpdate,
    role: Option<&str>,
    label: Option<&str>,
) -> Option<NodeRef> {
    update
        .nodes
        .iter()
        .find(|(_, n)| {
            let role_ok = role
                .map(|r| format!("{:?}", n.role()).eq_ignore_ascii_case(r))
                .unwrap_or(true);
            let label_ok = label.map(|l| n.label() == Some(l)).unwrap_or(true);
            role_ok && label_ok
        })
        .map(|(id, _)| id.0)
}

fn snapshot_json(update: &accesskit::TreeUpdate, max_depth: Option<usize>) -> serde_json::Value {
    use std::collections::{HashMap, HashSet, VecDeque};
    let focus = update.focus;
    let map: HashMap<accesskit::NodeId, &accesskit::Node> =
        update.nodes.iter().map(|(id, n)| (*id, n)).collect();
    let root = update.tree.as_ref().map(|t| t.root);
    let semantic_ctx = SemanticContext::new(update);
    let mut out: Vec<SemanticNode> = Vec::new();
    match root {
        Some(root) => {
            let mut seen: HashSet<accesskit::NodeId> = HashSet::new();
            let mut queue: VecDeque<(accesskit::NodeId, usize)> = VecDeque::new();
            queue.push_back((root, 0));
            while let Some((nid, depth)) = queue.pop_front() {
                if !seen.insert(nid) {
                    continue;
                }
                if let Some(node) = map.get(&nid) {
                    let descend = max_depth.map(|d| depth < d).unwrap_or(true);
                    let mut sn = semantic_node(nid, node, focus, &semantic_ctx);
                    // At the depth cap the children aren't emitted, so drop the
                    // child refs rather than leave dangling ids pointing at
                    // nodes absent from `nodes`.
                    if !descend {
                        sn.children.clear();
                    }
                    out.push(sn);
                    if descend {
                        for c in node.children() {
                            queue.push_back((*c, depth + 1));
                        }
                    }
                }
            }
        }
        None => {
            for (id, n) in &update.nodes {
                out.push(semantic_node(*id, n, focus, &semantic_ctx));
            }
        }
    }
    serde_json::json!({
        "root": root.map(|r| r.0),
        "focus": focus.0,
        "nodes": out,
    })
}

/// Evaluate an assertion, distinguishing three outcomes rather than two.
///
/// `Ok` is a passing assertion. `Err` is either a genuinely false assertion
/// (`ASSERTION_FAILED`) or a node reference that names nothing
/// (`NOT_FOUND`) — and telling those apart is the point. "The button is not
/// focused" and "there is no such button" are different bugs, and a caller that
/// sees one message for both chases the wrong one.
///
/// `Assertion::Exists` against a missing node is `ASSERTION_FAILED`, not
/// `NOT_FOUND`: asking whether something exists and being told it does not is
/// an answer, not a lookup error.
fn evaluate_assertion(
    update: &accesskit::TreeUpdate,
    node: NodeRef,
    assertion: &Assertion,
) -> Result<AssertionResult, AutomationReply> {
    let found = update.nodes.iter().find(|(id, _)| id.0 == node);
    let pass = |passed: bool, detail: Option<String>| AssertionResult { passed, detail };
    let Some((id, n)) = found else {
        return Err(if matches!(assertion, Assertion::Exists) {
            AutomationReply::err(
                codes::ASSERTION_FAILED,
                format!("assertion 'exists' failed: node {node} is not in the tree"),
            )
        } else {
            AutomationReply::err(
                codes::NOT_FOUND,
                format!("node {node} is not in the tree, so nothing could be asserted about it"),
            )
        });
    };
    let result = match assertion {
        Assertion::Exists => pass(true, None),
        Assertion::Focused => {
            let ok = id.0 == update.focus.0;
            pass(ok, (!ok).then(|| "node is not focused".to_string()))
        }
        Assertion::RoleEquals { value } => {
            let actual = format!("{:?}", n.role());
            let ok = actual.eq_ignore_ascii_case(value);
            pass(
                ok,
                (!ok).then(|| format!("role is '{actual}', expected '{value}'")),
            )
        }
        Assertion::LabelEquals { value } => {
            let actual = n.label();
            let ok = actual == Some(value.as_str());
            pass(
                ok,
                (!ok).then(|| format!("label is {actual:?}, expected '{value}'")),
            )
        }
        Assertion::LabelContains { value } => {
            let actual = n.label().unwrap_or("");
            let ok = actual.contains(value.as_str());
            pass(
                ok,
                (!ok).then(|| format!("label '{actual}' does not contain '{value}'")),
            )
        }
        Assertion::ValueEquals { value } => {
            let actual = n.value();
            let ok = actual == Some(value.as_str());
            pass(
                ok,
                (!ok).then(|| format!("value is {actual:?}, expected '{value}'")),
            )
        }
        Assertion::Toggled { value } => {
            // A bool assertion must NOT collapse `Mixed` (tristate /
            // indeterminate) into `false`: `Toggled { value: false }` on a
            // partially-checked parent checkbox should FAIL, not silently pass.
            let state = n.toggled();
            let ok = matches!(
                (value, state),
                (true, Some(accesskit::Toggled::True)) | (false, Some(accesskit::Toggled::False))
            );
            let actual = match state {
                Some(accesskit::Toggled::True) => "true",
                Some(accesskit::Toggled::False) => "false",
                Some(accesskit::Toggled::Mixed) => "mixed",
                None => "none",
            };
            pass(
                ok,
                (!ok).then(|| format!("toggled is {actual}, expected {value}")),
            )
        }
        Assertion::Expanded { value } => {
            let actual = n.is_expanded().unwrap_or(false);
            let ok = actual == *value;
            pass(
                ok,
                (!ok).then(|| format!("expanded is {actual}, expected {value}")),
            )
        }
        Assertion::Selected { value } => {
            let actual = n.is_selected().unwrap_or(false);
            let ok = actual == *value;
            pass(
                ok,
                (!ok).then(|| format!("selected is {actual}, expected {value}")),
            )
        }
        Assertion::Disabled { value } => {
            let actual = n.is_disabled();
            let ok = actual == *value;
            pass(
                ok,
                (!ok).then(|| format!("disabled is {actual}, expected {value}")),
            )
        }
        Assertion::SupportsTextRanges => {
            let info = teksilo_core::accessibility::audit::text_infos(update);
            let ok = info.contains_key(id);
            pass(
                ok,
                (!ok).then(|| {
                    "node carries no text ranges, so a screen reader cannot review it by \
                     character, word or line"
                        .to_string()
                }),
            )
        }
        Assertion::DocumentTextEquals { value } => {
            let info = teksilo_core::accessibility::audit::text_infos(update);
            match info.get(id) {
                Some(text) => {
                    let ok = &text.document_text == value;
                    pass(
                        ok,
                        (!ok).then(|| {
                            format!(
                                "the reviewable text is '{}', expected '{value}'",
                                text.document_text
                            )
                        }),
                    )
                }
                None => pass(false, Some("node carries no text ranges".to_string())),
            }
        }
    };
    if result.passed {
        Ok(result)
    } else {
        // The detail is the whole value of the failure — "label is None,
        // expected 'Save'" is what tells the caller what to fix. It goes in the
        // message rather than being dropped, and the code says this was a real
        // node whose property did not match.
        Err(AutomationReply::err(
            codes::ASSERTION_FAILED,
            match result.detail {
                Some(detail) => format!("assertion failed on node {node}: {detail}"),
                None => format!("assertion failed on node {node}"),
            },
        ))
    }
}

// ---------------------------------------------------------------------------
// Name <-> enum mapping
// ---------------------------------------------------------------------------

/// Actions surfaced in a `SemanticNode.actions` list, paired with the
/// snake_case name an automation client uses in `invoke_action`.
const ADVERTISABLE_ACTIONS: &[(accesskit::Action, &str)] = &[
    (accesskit::Action::Click, "click"),
    (accesskit::Action::Focus, "focus"),
    (accesskit::Action::Increment, "increment"),
    (accesskit::Action::Decrement, "decrement"),
    (accesskit::Action::Expand, "expand"),
    (accesskit::Action::Collapse, "collapse"),
    (accesskit::Action::SetValue, "set_value"),
    (accesskit::Action::ShowContextMenu, "show_context_menu"),
    (accesskit::Action::ScrollIntoView, "scroll_into_view"),
    (accesskit::Action::ScrollUp, "scroll_up"),
    (accesskit::Action::ScrollDown, "scroll_down"),
    (accesskit::Action::ScrollLeft, "scroll_left"),
    (accesskit::Action::ScrollRight, "scroll_right"),
];

/// The snake_case name for an `accesskit::Action`, for error messages.
fn action_name(action: accesskit::Action) -> Option<&'static str> {
    ADVERTISABLE_ACTIONS
        .iter()
        .find(|(a, _)| *a == action)
        .map(|(_, name)| *name)
}

/// Map an automation action name to an `accesskit::Action`. Accepts the
/// snake_case names plus a few intuitive aliases.
fn action_from_str(s: &str) -> Option<accesskit::Action> {
    use accesskit::Action as A;
    let lower = s.to_ascii_lowercase();
    Some(match lower.as_str() {
        "click" | "default" | "press" | "activate" => A::Click,
        "focus" => A::Focus,
        "blur" => A::Blur,
        "increment" => A::Increment,
        "decrement" => A::Decrement,
        "expand" => A::Expand,
        "collapse" => A::Collapse,
        "set_value" => A::SetValue,
        "show_context_menu" | "context_menu" => A::ShowContextMenu,
        "scroll_into_view" => A::ScrollIntoView,
        "scroll_up" => A::ScrollUp,
        "scroll_down" => A::ScrollDown,
        "scroll_left" => A::ScrollLeft,
        "scroll_right" => A::ScrollRight,
        "show_tooltip" => A::ShowTooltip,
        "hide_tooltip" => A::HideTooltip,
        _ => return None,
    })
}

/// Build a [`Modifiers`] set from the DTO's flags.
///
/// `ctrl` is **literal** Control on every platform; `command` is the platform's
/// primary accelerator — Control on Windows and Linux, Command on macOS.
/// Keeping them apart is what lets one agent script drive all three platforms:
/// a Teksilo shortcut declared as `Ctrl+S` resolves to the Command chord on
/// macOS, so a probe sending literal Control there matches no binding and —
/// worse — reports success, because the key really was injected. See
/// [`Modifiers::COMMAND`].
fn modifiers(ctrl: bool, shift: bool, alt: bool, meta: bool, command: bool) -> Modifiers {
    let mut m = Modifiers::NONE;
    if ctrl {
        m = m | Modifiers::CTRL;
    }
    if command {
        m = m | Modifiers::COMMAND;
    }
    if shift {
        m = m | Modifiers::SHIFT;
    }
    if alt {
        m = m | Modifiers::ALT;
    }
    if meta {
        m = m | Modifiers::SUPER;
    }
    m
}

fn format_keystroke(ks: teksilo_core::shortcut::KeyStroke) -> String {
    // `Modifiers` Display emits a trailing "+", e.g. "Ctrl+"; `Key` Display
    // emits the key name. Together: "Ctrl+S".
    format!("{}{}", ks.modifiers, ks.key)
}

/// Map an automation key name to a [`Key`]. Accepts named keys
/// (case-insensitive), single characters, and ASCII letters.
fn key_from_str(s: &str) -> Option<Key> {
    let lower = s.to_ascii_lowercase();
    let named = match lower.as_str() {
        "space" | " " => Some(Key::Space),
        "enter" | "return" => Some(Key::Enter),
        "escape" | "esc" => Some(Key::Escape),
        "tab" => Some(Key::Tab),
        "backspace" => Some(Key::Backspace),
        "delete" | "del" => Some(Key::Delete),
        "up" | "arrowup" => Some(Key::ArrowUp),
        "down" | "arrowdown" => Some(Key::ArrowDown),
        "left" | "arrowleft" => Some(Key::ArrowLeft),
        "right" | "arrowright" => Some(Key::ArrowRight),
        "home" => Some(Key::Home),
        "end" => Some(Key::End),
        "pageup" => Some(Key::PageUp),
        "pagedown" => Some(Key::PageDown),
        "capslock" => Some(Key::CapsLock),
        "f1" => Some(Key::F1),
        "f2" => Some(Key::F2),
        "f3" => Some(Key::F3),
        "f4" => Some(Key::F4),
        "f5" => Some(Key::F5),
        "f6" => Some(Key::F6),
        "f7" => Some(Key::F7),
        "f8" => Some(Key::F8),
        "f9" => Some(Key::F9),
        "f10" => Some(Key::F10),
        "f11" => Some(Key::F11),
        "f12" => Some(Key::F12),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    // A single character.
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) => {
            // ASCII letters MUST map to the named `Key::A..Key::Z` variants,
            // not `Key::Character`: shortcuts register with `Key::S` etc., and
            // `KeyStroke` equality is by variant, so `inject_key {key:"s",
            // ctrl:true}` would otherwise never fire a `Ctrl+S` shortcut.
            if ch.is_ascii_alphabetic() {
                Some(letter_key(ch.to_ascii_uppercase()))
            } else {
                Some(Key::Character(ch))
            }
        }
        _ => None,
    }
}

/// Map an uppercase ASCII letter to its `Key::A..Key::Z` variant.
fn letter_key(upper: char) -> Key {
    match upper {
        'A' => Key::A,
        'B' => Key::B,
        'C' => Key::C,
        'D' => Key::D,
        'E' => Key::E,
        'F' => Key::F,
        'G' => Key::G,
        'H' => Key::H,
        'I' => Key::I,
        'J' => Key::J,
        'K' => Key::K,
        'L' => Key::L,
        'M' => Key::M,
        'N' => Key::N,
        'O' => Key::O,
        'P' => Key::P,
        'Q' => Key::Q,
        'R' => Key::R,
        'S' => Key::S,
        'T' => Key::T,
        'U' => Key::U,
        'V' => Key::V,
        'W' => Key::W,
        'X' => Key::X,
        'Y' => Key::Y,
        // Only reached for ASCII alphabetic chars, so 'Z' is the last case.
        _ => Key::Z,
    }
}
