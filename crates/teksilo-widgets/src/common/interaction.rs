// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The framework press, as a control consumes it.
//!
//! A control's press visual used to be its own bookkeeping: `PointerDown` sets
//! a flag, `PointerUp` and `PointerLeave` clear it. That is correct for a
//! mouse and wrong for a finger in four ways the widget cannot see from inside
//! a handler — a press that slides off its target, a press that slides back
//! on, a press a pan claimant takes away without a release, and a press that
//! must not light up at all until the pan has been ruled out. The router keeps
//! the state instead; `docs/touch-and-pen.md` §7 has the reasoning and the
//! census of what still actuates on press.
//!
//! Three shapes of consumer, and the module serves all three:
//!
//! * a control whose visual **is** the framework press binds
//!   [`bind_pressed`] (or reads `ctx.pressed_signal()` directly) and deletes
//!   its own handler;
//! * a control whose visual is one value of a small enum — because the same
//!   signal also carries hover and focus — binds [`bind_press_state`], which
//!   moves only the pressed cell and asks the caller where the control rests
//!   when the press ends;
//! * a control that keeps its own state machine — because a keyboard
//!   activation drives the same visual, or because a theme captured the signal
//!   — consults [`press_shows_now`] and [`press_survives`] from inside the
//!   handlers it already has.

use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::widget::EventContext;

/// Mirror the framework press onto a control's own pressed signal.
///
/// Keeps the control's existing `Signal<bool>` identity — a Tier-3 style that
/// captured it, or a test observing it, goes on working — while the value
/// behind it becomes the router's. Call once from `build()`; the observer is
/// owned by the build scope and goes away with it.
///
/// The mirror is seeded from the current state rather than from `false`, so a
/// rebuild that happens *during* a press (a signal the press itself wrote)
/// does not blink the visual off.
pub(crate) fn bind_pressed(ctx: &mut BuildContext, mirror: Signal<bool>) {
    let pressed = ctx.pressed_signal();
    mirror.set(pressed.get());
    ctx.effect(&pressed, move |p| mirror.set(*p));
}

/// Drive an enum-valued interaction signal's *pressed* cell from the framework
/// press.
///
/// The general form of [`bind_pressed`], for a control whose visual state is
/// one value of a small enum rather than a bare bool: the ToolBox header's
/// `HeaderInteraction`, a calendar zoom cell's two bools, a `RadioTile`'s
/// `InteractionState`. Each of those already had a `Pressed` value its *theme*
/// painted and no pointer ever reached — the value was written by the keyboard
/// path alone, or by nothing at all — so the recipe's pressed chrome was
/// unreachable for a mouse as well as for a finger.
///
/// `resting` is consulted only when the press ends: it answers "where does
/// this control sit now", which the signal itself cannot, because while it
/// holds `pressed` the hover truth has nowhere to live. The `pressed` guard on
/// that branch is what keeps a release from overwriting a state the control's
/// own `on_tap` has already chosen.
///
/// Seeded from the live press rather than from the resting value, so a rebuild
/// that lands *during* a press does not blink the visual off.
pub(crate) fn bind_press_state<T>(
    ctx: &mut BuildContext,
    state: Signal<T>,
    pressed_value: T,
    resting: impl Fn() -> T + 'static,
) where
    T: Clone + PartialEq + 'static,
{
    let pressed = ctx.pressed_signal();
    if pressed.get() {
        state.set(pressed_value.clone());
    }
    ctx.effect(&pressed, move |showing| {
        if *showing {
            state.set(pressed_value.clone());
        } else if state.get() == pressed_value {
            state.set(resting());
        }
    });
}

/// Whether a press visual may light up on the sample being handled.
///
/// `false` only while the framework is withholding the visual — a direct
/// pointer pressed inside a pan claimant, whose press-feedback delay has not
/// elapsed. A control that lights its own visual from `PointerDown` guards it
/// with this so a finger resting on a list row does not flash the row before
/// the pan is ruled out; the framework's own
/// [`pressed_signal`](teksilo_core::BuildContext::pressed_signal) turns it on when the delay
/// expires.
///
/// Always `true` for a mouse: an indirect pointer opens no pan session, so
/// there is no ambiguity to wait out and the visual appears at once, exactly as
/// it did before the touch programme.
pub(crate) fn press_shows_now(ctx: &EventContext) -> bool {
    !ctx.press_pending()
}

/// Whether the press being handled has stayed on its target.
///
/// The question a control asks on a move or a release when it has to decide
/// whether the press it is holding is still live: `false` once the pointer has
/// left the press's tap boundary — a radius for a precise pointer, the node's
/// own bounds for a finger, the same predicate that fails the tap. WCAG 2.2
/// SC 2.5.2's abort gesture, and reversible: sliding back on makes it `true`
/// again.
pub(crate) fn press_survives(ctx: &EventContext) -> bool {
    ctx.press_is_inside()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use teksilo_canvas::{Point, SizeProposal};
    use teksilo_core::event::{EventResponse, PointerButton, WidgetEvent};
    use teksilo_core::pointer::touch_action::PanClaim;
    use teksilo_core::pointer::{
        BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
        PointerSample,
    };
    use teksilo_core::widget_builder::WidgetBuilder;
    use teksilo_core::widget_tree::WidgetTree;

    use super::*;
    use crate::primitives::{FixedSize, RectWidget, VStack};

    fn finger() -> PointerId {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        PointerIdAllocator::global().begin(
            BackendDeviceKey::new(0x010E),
            NEXT.fetch_add(1, Ordering::Relaxed),
        )
    }

    fn touch(id: PointerId, phase: PointerPhase, at: Point, ms: u64) -> PointerSample {
        PointerSample {
            pointer: PointerInfo::touch(id, EventTime::from_millis(ms)),
            phase,
            position: at,
            button: None,
            modifiers: teksilo_core::event::Modifiers::NONE,
            coalesced: Vec::new(),
        }
    }

    /// A row inside a scrollable, reporting the two predicates from its own
    /// pointer handler. Returns `(what the row saw, the tree)`.
    fn row_in_a_list() -> (Rc<RefCell<Vec<(bool, bool)>>>, WidgetTree, Point) {
        let seen: Rc<RefCell<Vec<(bool, bool)>>> = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let row = tree.add(
            FixedSize::new()
                .width(200.0)
                .height(48.0)
                .child(RectWidget::new())
                .on_tap(|_e, _c| {})
                .on_pointer_event(move |event, ctx| {
                    if !matches!(event, WidgetEvent::PointerMove { .. }) {
                        log.borrow_mut()
                            .push((press_shows_now(ctx), press_survives(ctx)));
                    }
                    EventResponse::Ignored
                }),
        );
        let list = tree.add(
            VStack::new()
                .add_child(row)
                .pan_claim(PanClaim::vertical())
                .on_scroll(|_e, _c| EventResponse::Handled),
        );
        tree.layout(SizeProposal::exact(200.0, 400.0));
        let _ = list;
        let at = tree.bounds(row).center();
        (seen, tree, at)
    }

    /// Inside a claimant, a finger's press is real but its visual is withheld:
    /// `press_survives` is already true while `press_shows_now` is not.
    #[test]
    fn a_finger_inside_a_claimant_is_pressed_before_it_may_show() {
        let (seen, mut tree, at) = row_in_a_list();
        let id = finger();
        tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
        assert_eq!(
            seen.borrow().as_slice(),
            &[(false, true)],
            "held, but not yet allowed to light up",
        );
    }

    /// A mouse pressing the very same row waits for nothing — an indirect
    /// pointer opens no pan session, so the delay never reaches it.
    #[test]
    fn a_mouse_inside_the_same_claimant_may_show_at_once() {
        let (seen, mut tree, at) = row_in_a_list();
        tree.dispatch_event(WidgetEvent::PointerDown {
            position: at,
            button: PointerButton::Primary,
            modifiers: teksilo_core::event::Modifiers::NONE,
        });
        assert_eq!(seen.borrow().as_slice(), &[(true, true)]);
    }

    /// A release that landed off the row reports `press_survives` false, so a
    /// control keeping its own machine knows not to complete the activation.
    #[test]
    fn a_release_off_the_row_does_not_survive() {
        let (seen, mut tree, at) = row_in_a_list();
        let id = finger();
        let away = Point::new(at.x, at.y + 900.0);
        tree.dispatch_pointer(touch(id, PointerPhase::Down, at, 0));
        tree.dispatch_pointer(touch(id, PointerPhase::Move, away, 20));
        tree.dispatch_pointer(touch(id, PointerPhase::Up, away, 40));
        assert_eq!(
            seen.borrow().last().copied(),
            Some((true, false)),
            "the delay has long elapsed, but the press is no longer on its target",
        );
    }
}
