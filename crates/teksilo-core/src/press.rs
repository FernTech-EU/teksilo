// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Per-node press state — the framework's answer to "is this control being
//! held down right now".
//!
//! # Why the router owns it
//!
//! A press visual used to be each widget's own bookkeeping: an
//! `on_pointer_event` handler flipping a state enum on `PointerDown` and back
//! on `PointerUp`. That works for a mouse and fails for a finger, for four
//! reasons the widget cannot see from inside a handler:
//!
//! * **Slide-off.** WCAG 2.2 SC 2.5.2 says a press that travels off its target
//!   is abandoned. The predicate for "off its target" is the pointer's
//!   [`TapBoundary`](crate::gesture::TapBoundary) — a radius for a mouse, the
//!   node's own bounds for a finger — and it is the router that holds the
//!   press origin to measure from.
//! * **Re-entry.** Sliding back on restores the press. A handler that only
//!   sees `PointerUp` inside its bounds has already forgotten the press left.
//! * **A peer claim.** When a pan claimant or an ancestor drag wins the
//!   arbitration, the pressed control loses the press without ever seeing a
//!   release. Only the arbitration knows.
//! * **The feedback delay.** A finger resting on a list row must not flash the
//!   row before the pan has been ruled out. That delay is a property of the
//!   press's surroundings, not of the control.
//!
//! So the state lives here, keyed by the node whose gesture arena took the
//! press, and reaches recipes as one `Signal<bool>` per node
//! ([`BuildContext::pressed_signal`](crate::BuildContext::pressed_signal)).
//!
//! # What "pressed" means
//!
//! Three questions, deliberately separate, because a press visual and a press
//! are not the same thing:
//!
//! | question | true when |
//! | --- | --- |
//! | `PressTable::owner_of` | the node is held, wherever the pointer is now |
//! | `Press::inside` | …and the pointer has not left the tap boundary |
//! | `Press::showing` | …and the feedback delay has elapsed |
//!
//! `showing` is what the node's signal mirrors and what a recipe paints. The
//! three reach a widget as `WidgetTree::pressed_by` / `press_is_inside` /
//! `is_pressed`, and a handler as the matching `EventContext` queries.

use std::time::Duration;

use crate::pointer::{EventTime, PointerId};
use crate::widget_id::WidgetId;

/// One live press, as the router tracks it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Press {
    /// The contact holding it. A release only clears the press it owns, which
    /// is what stops a second finger's lift from clearing the first finger's
    /// visual.
    pub(crate) pointer: PointerId,
    /// The node whose gesture arena took the press — the node whose `on_tap`
    /// would fire, and the one whose signal this record drives. `None` until
    /// the press has been dispatched and the arena has claimed it; `None` for
    /// good for a press no arena took at all, and for one made with a button
    /// the owner does not act on — a middle-click, or a right-click on a node
    /// with no context menu, must raise no visual for an activation that can
    /// never follow.
    ///
    /// An ownerless record is still a record: it holds the focusable a direct
    /// pointer's press deferred to its release.
    pub(crate) owner: Option<WidgetId>,
    /// The focusable the press landed on, if any.
    ///
    /// A direct pointer defers its focus assignment to the release and only
    /// makes it when the release lands on this same node — the press-then-
    /// slide-off gesture must leave focus where it was.
    pub(crate) focusable: Option<WidgetId>,
    /// Whether the pointer is inside the press's tap boundary.
    pub(crate) inside: bool,
    /// The instant the visual may first appear. `None` once it may — either
    /// because no delay applied or because the delay has elapsed.
    pub(crate) show_at: Option<EventTime>,
}

impl Press {
    /// Whether the visual is showing: held, inside, and past any delay.
    pub(crate) fn showing(&self) -> bool {
        self.inside && self.show_at.is_none()
    }

    /// Whether the press is held but its feedback delay has not elapsed.
    pub(crate) fn pending(&self) -> bool {
        self.show_at.is_some()
    }

    /// Resolve the feedback delay against `now`, and report whether that
    /// changed anything.
    fn resolve_delay(&mut self, now: EventTime) -> bool {
        match self.show_at {
            Some(deadline) if now >= deadline => {
                self.show_at = None;
                true
            }
            _ => false,
        }
    }
}

/// Every live press in one tree.
///
/// A `Vec` rather than a map: there is one entry per contact, so at most a
/// handful, and every access is either "this pointer's" or "this node's".
#[derive(Debug, Default)]
pub(crate) struct PressTable {
    presses: Vec<Press>,
}

impl PressTable {
    /// Open a press for `pointer`.
    ///
    /// `delay` is the press-feedback delay to observe before the visual may
    /// appear — `None` when it may appear at once. A pointer already holding a
    /// press replaces it: a second `Down` for the same contact can only mean
    /// the previous one was never closed.
    pub(crate) fn press(
        &mut self,
        pointer: PointerId,
        focusable: Option<WidgetId>,
        now: EventTime,
        delay: Option<Duration>,
    ) {
        self.presses.retain(|p| p.pointer != pointer);
        self.presses.push(Press {
            pointer,
            owner: None,
            focusable,
            inside: true,
            show_at: delay.map(|d| now + d),
        });
    }

    /// Name the node whose gesture arena took `pointer`'s press.
    ///
    /// Refused when another contact already owns that node's visual: under
    /// [`MultiContact::First`](crate::gesture::MultiContact) the second contact
    /// is terminated before it reaches the arena at all, and under any other
    /// policy the first contact still owns the boolean the recipe paints —
    /// otherwise the first release would clear a visual the second is holding.
    pub(crate) fn set_owner(&mut self, pointer: PointerId, owner: WidgetId) {
        if self
            .presses
            .iter()
            .any(|p| p.pointer != pointer && p.owner == Some(owner))
        {
            return;
        }
        if let Some(press) = self.presses.iter_mut().find(|p| p.pointer == pointer) {
            press.owner = Some(owner);
        }
    }

    /// This pointer's press, if it holds one.
    pub(crate) fn get(&self, pointer: PointerId) -> Option<&Press> {
        self.presses.iter().find(|p| p.pointer == pointer)
    }

    /// This pointer's press, mutably.
    pub(crate) fn get_mut(&mut self, pointer: PointerId) -> Option<&mut Press> {
        self.presses.iter_mut().find(|p| p.pointer == pointer)
    }

    /// The contact holding `node`'s press, whether or not its visual is
    /// showing.
    pub(crate) fn owner_of(&self, node: WidgetId) -> Option<PointerId> {
        self.presses
            .iter()
            .find(|p| p.owner == Some(node))
            .map(|p| p.pointer)
    }

    /// The press driving `node`'s visual, if any.
    pub(crate) fn for_node(&self, node: WidgetId) -> Option<&Press> {
        self.presses.iter().find(|p| p.owner == Some(node))
    }

    /// Close `pointer`'s press and report the node whose visual it drove, so
    /// the caller can republish that node's signal.
    pub(crate) fn release(&mut self, pointer: PointerId) -> Option<WidgetId> {
        let index = self.presses.iter().position(|p| p.pointer == pointer)?;
        self.presses.swap_remove(index).owner
    }

    /// Resolve every pending feedback delay against `now`, and report the
    /// nodes whose visual just appeared.
    pub(crate) fn resolve_delays(&mut self, now: EventTime) -> Vec<WidgetId> {
        self.presses
            .iter_mut()
            .filter_map(|p| p.resolve_delay(now).then_some(p.owner).flatten())
            .collect()
    }

    /// The earliest instant at which a pending press wants to be looked at
    /// again, so the event loop can be woken for a finger that is not moving.
    pub(crate) fn next_deadline(&self) -> Option<EventTime> {
        self.presses.iter().filter_map(|p| p.show_at).min()
    }

    /// Whether any press is live.
    pub(crate) fn is_empty(&self) -> bool {
        self.presses.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh contact from the process allocator — the only thing allowed to
    /// mint one.
    fn contact() -> PointerId {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(70_000);
        crate::pointer::PointerIdAllocator::global().begin(
            crate::pointer::BackendDeviceKey::DEFAULT,
            NEXT.fetch_add(1, Ordering::Relaxed),
        )
    }

    /// A node id with no tree behind it — the table stores ids and never
    /// dereferences them.
    fn node() -> WidgetId {
        use slotmap::SlotMap;
        let mut map: SlotMap<WidgetId, ()> = SlotMap::with_key();
        map.insert(())
    }

    #[test]
    fn a_release_clears_only_the_press_it_owns() {
        // Two contacts, two presses. Lifting the second must leave the first
        // holding its own record — the visual belongs to the contact that
        // opened it.
        let mut table = PressTable::default();
        let (node, first, second) = (node(), contact(), contact());
        table.press(first, None, EventTime::ZERO, None);
        table.set_owner(first, node);
        table.press(second, None, EventTime::ZERO, None);

        table.release(second);
        assert_eq!(table.owner_of(node), Some(first));
        assert!(table.for_node(node).is_some_and(Press::showing));
    }

    #[test]
    fn a_second_contact_does_not_take_over_a_nodes_visual() {
        // Whichever policy let the second contact reach the arena, the first
        // one keeps the boolean: otherwise the first release clears a visual
        // the second is still holding.
        let mut table = PressTable::default();
        let (node, first, second) = (node(), contact(), contact());
        table.press(first, None, EventTime::ZERO, None);
        table.set_owner(first, node);
        table.press(second, None, EventTime::ZERO, None);
        table.set_owner(second, node);

        assert_eq!(table.owner_of(node), Some(first));
        assert_eq!(
            table.get(second).and_then(|p| p.owner),
            None,
            "the second contact holds a press, but not this node's visual"
        );
    }

    #[test]
    fn a_delayed_press_shows_only_once_its_deadline_passes() {
        let mut table = PressTable::default();
        let (node, held) = (node(), contact());
        table.press(
            held,
            None,
            EventTime::ZERO,
            Some(Duration::from_millis(100)),
        );
        table.set_owner(held, node);

        let press = *table.for_node(node).expect("press");
        assert!(press.pending() && !press.showing());
        assert_eq!(table.next_deadline(), Some(EventTime::from_millis(100)),);

        assert!(table.resolve_delays(EventTime::from_millis(99)).is_empty());
        assert_eq!(
            table.resolve_delays(EventTime::from_millis(100)),
            vec![node]
        );
        assert!(table.for_node(node).is_some_and(Press::showing));
        assert_eq!(table.next_deadline(), None);
    }
}
