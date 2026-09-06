// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The live pointer table: one entry per pointer the tree currently knows
//! about, and the two elected roles that replace the tree's old singular
//! pointer state.
//!
//! # Three notions that used to be one
//!
//! Before this table the tree had a single `hovered`, a single
//! `last_pointer_position` and a single `pointer_captured_by`, and "the
//! pointer" meant all three at once. Multi-touch splits that into three
//! genuinely different questions:
//!
//! 1. [`PointerInfo::primary`](crate::pointer::PointerInfo::primary) is the
//!    **W3C Pointer Events Level 3 per-kind flag**. Every mouse event is
//!    primary, and so is the first touch of a sequence. On a hybrid machine a
//!    mouse and a first touch are *both* primary. It is a property of the
//!    sample, decided by whoever produced it, and this table never rewrites it.
//! 2. [`PointerTable::primary`] is **Teksilo's** single pointer — the one that
//!    backs the legacy singular accessors
//!    ([`WidgetTree::hovered`](crate::WidgetTree::hovered),
//!    [`WidgetTree::last_pointer_position`](crate::WidgetTree::last_pointer_position)).
//!    Exactly one live pointer holds the role, a mouse always wins it, and it
//!    is a table-level election rather than anything carried on a sample.
//! 3. [`PointerTable::hover_owner`] is the most recent **hovering-capable**
//!    pointer — a mouse, or a pen in proximity ([`PointerKind::hovers`]).
//!
//! **Hover follows the hover owner, never the primary.** Enter/leave, the
//! cursor, tooltip dwell and every `on_hover` handler are hover-owner-only, and
//! a touch contact is never a hover owner: a finger has no hover state to
//! report, so a contact that wrote hover would make every hover affordance fire
//! on tap and stick there after the lift. Affordances that today rely on hover
//! get their own touch routes; they do not get them by pretending a finger
//! hovers.
//!
//! # Why a `Vec`
//!
//! The live set is at most [`PointerTable::DEFAULT_CAP`] entries, so every
//! operation is a linear scan of a handful of elements — cheaper than hashing
//! a [`PointerId`], and it keeps iteration in a stable, oldest-first order.
//!
//! Reference: `docs/touch-and-pen.md`.

use teksilo_canvas::Point;
use teksilo_tokens::PointerKind;

use super::{PointerId, PointerInfo, PointerPhase, PointerSample};
use crate::widget_id::WidgetId;

/// Everything the tree tracks about one live pointer.
///
/// One of these exists from the pointer's first sample until it lifts (a
/// contact) or the window loses it (a hovering pointer never lifts, so a
/// mouse's entry persists for the life of the tree once it has been seen).
///
/// `#[non_exhaustive]`: the gesture-sequence handle and the velocity tracker
/// land here in later packages, and neither should break a construction site.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct PointerEntry {
    /// Who this pointer is, as of its most recent sample.
    pub info: PointerInfo,
    /// Where it is now, in window-logical coordinates.
    pub position: Point,
    /// Where its most recent press landed. Equal to
    /// [`position`](Self::position) until the first press, so a slop test
    /// against it is never nonsense.
    pub down_position: Point,
    /// The widget it is hovering, if it is a hovering-capable pointer that
    /// currently holds the hover-owner role. Always `None` for a contact.
    pub hovered: Option<WidgetId>,
    /// The widget that captured *this* pointer. Capture is per pointer: two
    /// contacts hold independent captures, and each is released only by its
    /// own Up or Cancel.
    pub captured_by: Option<WidgetId>,
    /// Strict ancestors of [`hovered`](Self::hovered) whose `hover_within`
    /// signal this pointer currently holds `true`.
    pub hover_within: Vec<WidgetId>,
    /// The arbitration in progress for this pointer's press: who is competing
    /// for it, and who won. `None` between presses — a hovering mouse has an
    /// entry but no sequence. See
    /// [`PointerSequence`](crate::gesture::PointerSequence).
    pub sequence: Option<crate::gesture::PointerSequence>,
}

impl PointerEntry {
    /// A fresh entry for `info` first seen at `position`.
    fn new(info: PointerInfo, position: Point) -> Self {
        Self {
            info,
            position,
            down_position: position,
            hovered: None,
            captured_by: None,
            hover_within: Vec::new(),
            sequence: None,
        }
    }

    /// Whether this pointer can report a position without a button held, and
    /// so can own hover. See [`PointerKind::hovers`].
    pub fn hovers(&self) -> bool {
        self.info.kind.hovers()
    }

    /// Whether this pointer is touching the surface: a contact is touching for
    /// as long as it is live, a hovering-capable pointer only while a button
    /// is held.
    pub fn is_contacting(&self) -> bool {
        !self.hovers() || !self.info.buttons.is_empty()
    }
}

/// The live pointers, plus the primary and hover-owner elections.
///
/// See the [module docs](self) for what those two roles mean and why they are
/// not the same thing as [`PointerInfo::primary`].
#[derive(Clone, Debug)]
pub struct PointerTable {
    entries: Vec<PointerEntry>,
    primary: Option<PointerId>,
    hover_owner: Option<PointerId>,
    cap: usize,
}

impl Default for PointerTable {
    fn default() -> Self {
        Self::new()
    }
}

impl PointerTable {
    /// How many pointers the table holds at once.
    ///
    /// Ten: the maximum simultaneous contacts a Windows digitiser and a
    /// Wayland `wl_touch` seat both report. An eleventh arrival is refused at
    /// [`begin`](Self::begin) rather than evicting one of the ten, because
    /// evicting a contact mid-gesture is how a pinch turns into a fling.
    pub const DEFAULT_CAP: usize = 10;

    /// An empty table with the default cap.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            primary: None,
            hover_owner: None,
            cap: Self::DEFAULT_CAP,
        }
    }

    /// An empty table that holds at most `cap` pointers. For tests that want
    /// to reach the cap without simulating ten fingers.
    pub fn with_cap(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            ..Self::new()
        }
    }

    /// The cap this table was built with.
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// The entry for `id`, if that pointer is live.
    pub fn get(&self, id: PointerId) -> Option<&PointerEntry> {
        self.entries.iter().find(|e| e.info.id == id)
    }

    /// Mutable access to the entry for `id`, if that pointer is live.
    pub fn get_mut(&mut self, id: PointerId) -> Option<&mut PointerEntry> {
        self.entries.iter_mut().find(|e| e.info.id == id)
    }

    /// Admit `sample`'s pointer, creating its entry on first sight and
    /// refreshing it afterwards.
    ///
    /// Returns `None` — and traces the refusal — when the sample must not be
    /// dispatched at all:
    ///
    /// * the backend flagged the contact as a palm
    ///   ([`PointerInfo::palm`](crate::pointer::PointerInfo::palm)), or
    /// * the table is full ([`DEFAULT_CAP`](Self::DEFAULT_CAP)).
    ///
    /// A pointer already in the table is always refreshed, cap or no cap: the
    /// cap bounds how many pointers exist, never how many samples an admitted
    /// pointer may send.
    pub fn begin(&mut self, sample: &PointerSample) -> Option<PointerId> {
        if !self.would_admit(&sample.pointer) {
            return None;
        }
        self.admit(
            sample.pointer,
            sample.position,
            sample.phase == PointerPhase::Down,
        )
    }

    /// Whether [`begin`](Self::begin) would accept `info`, without touching the
    /// table — and tracing the refusal, since a dropped sample that leaves no
    /// evidence is the hardest input bug there is.
    ///
    /// The dispatcher asks this *before* it lowers a sample onto an event, so a
    /// refused pointer produces no event at all rather than one that is later
    /// discovered to have no entry behind it.
    pub fn would_admit(&self, info: &PointerInfo) -> bool {
        if self.contains(info.id) {
            // The cap bounds how many pointers exist, never how many samples an
            // admitted pointer may send.
            return true;
        }
        if info.palm {
            crate::trace_input!(
                Samples,
                "dropping {:?}: the backend classified it as a palm",
                info.id
            );
            return false;
        }
        if self.entries.len() >= self.cap {
            crate::trace_input!(
                Samples,
                "dropping {:?}: the pointer table already holds {} pointers",
                info.id,
                self.cap
            );
            return false;
        }
        true
    }

    /// The primitive behind [`begin`](Self::begin), for the legacy
    /// [`WidgetEvent`](crate::event::WidgetEvent) path, which has no
    /// [`PointerSample`] to hand.
    ///
    /// `is_down` records `position` as the entry's
    /// [`down_position`](PointerEntry::down_position).
    pub fn admit(
        &mut self,
        info: PointerInfo,
        position: Point,
        is_down: bool,
    ) -> Option<PointerId> {
        let id = info.id;
        if let Some(entry) = self.get_mut(id) {
            entry.info = info;
            entry.position = position;
            if is_down {
                entry.down_position = position;
            }
            self.elect();
            return Some(id);
        }
        if !self.would_admit(&info) {
            return None;
        }
        let mut entry = PointerEntry::new(info, position);
        if is_down {
            entry.down_position = position;
        }
        self.entries.push(entry);
        self.elect();
        Some(id)
    }

    /// Forget `id`, returning its entry if it was live.
    ///
    /// Called for a contact's Up or Cancel. A hovering-capable pointer is
    /// **not** ended by an Up — a mouse that releases a button is still there,
    /// still hovering, and its entry is what every legacy singular accessor
    /// reads.
    pub fn end(&mut self, id: PointerId) -> Option<PointerEntry> {
        let index = self.entries.iter().position(|e| e.info.id == id)?;
        let entry = self.entries.remove(index);
        self.elect();
        Some(entry)
    }

    /// Teksilo's single pointer: the one backing the legacy singular
    /// accessors. A mouse is preferred; failing that, the oldest live pointer.
    ///
    /// Not to be confused with
    /// [`PointerInfo::primary`](crate::pointer::PointerInfo::primary) — see
    /// the [module docs](self).
    pub fn primary(&self) -> Option<&PointerEntry> {
        self.primary.and_then(|id| self.get(id))
    }

    /// The most recent hovering-capable pointer, if any is live.
    ///
    /// Hover, enter/leave, the cursor and tooltip dwell all follow this
    /// pointer. A touch contact is never it.
    pub fn hover_owner(&self) -> Option<&PointerEntry> {
        self.hover_owner.and_then(|id| self.get(id))
    }

    /// Mutable access to the hover owner's entry.
    pub fn hover_owner_mut(&mut self) -> Option<&mut PointerEntry> {
        let id = self.hover_owner?;
        self.get_mut(id)
    }

    /// The hover owner's id, without borrowing its entry.
    pub fn hover_owner_id(&self) -> Option<PointerId> {
        self.hover_owner
    }

    /// The primary pointer's id, without borrowing its entry.
    pub fn primary_id(&self) -> Option<PointerId> {
        self.primary
    }

    /// Every live pointer, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &PointerEntry> + '_ {
        self.entries.iter()
    }

    /// Every live pointer, mutably, oldest first.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut PointerEntry> + '_ {
        self.entries.iter_mut()
    }

    /// How many live pointers there are, hovering or not.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no pointer is live.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many live pointers are **touching the surface** — every contact,
    /// plus any hovering-capable pointer with a button held. This is the
    /// number a multi-touch recognizer counts fingers with; it is `0` for a
    /// resting mouse and `2` for a two-finger pinch.
    pub fn contact_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_contacting()).count()
    }

    /// Whether `id` names a live pointer.
    pub fn contains(&self, id: PointerId) -> bool {
        self.get(id).is_some()
    }

    /// Scrub widget references that no longer name an active node.
    ///
    /// A rebuild mints fresh ids, so a hover target, a capture or a
    /// `hover_within` ancestor recorded before it can be dangling afterwards —
    /// and a dangling capture swallows every later Move and Up, because
    /// dispatch refuses an inactive target. Called from the tree's
    /// post-layout revalidation *after* it has run the hover-owner's own
    /// signal-firing recovery, so this is the sweep for every other pointer.
    pub fn retain_active(&mut self, arena: &crate::arena::WidgetArena) {
        for entry in &mut self.entries {
            if entry.hovered.is_some_and(|id| !arena.is_active(id)) {
                entry.hovered = None;
            }
            if entry.captured_by.is_some_and(|id| !arena.is_active(id)) {
                entry.captured_by = None;
            }
            entry.hover_within.retain(|id| arena.is_active(*id));
        }
    }

    /// Release every capture in the table.
    ///
    /// The window went away under the interaction — it lost focus, or was
    /// occluded — so no pointer will ever see the Up that would have released
    /// it. A capture left standing in that state strands the whole window:
    /// every later move is redelivered to the abandoned widget instead of
    /// hit-testing.
    pub fn release_all_captures(&mut self) {
        for entry in &mut self.entries {
            entry.captured_by = None;
        }
    }

    /// Release every capture held on `widget` — it is going away, and a
    /// capture that outlives its owner strands the pointer.
    pub fn release_captures_of(&mut self, widget: WidgetId) {
        for entry in &mut self.entries {
            if entry.captured_by == Some(widget) {
                entry.captured_by = None;
            }
        }
    }

    /// Re-run both elections after the live set or a pointer's kind changed.
    ///
    /// **Primary**: a mouse if one is live (there is at most one), else the
    /// oldest pointer — ids are minted monotonically, so "oldest" is "lowest
    /// id", and a stable choice is what keeps the legacy accessors from
    /// flickering between two contacts.
    ///
    /// **Hover owner**: the *most recent* hovering-capable pointer, which for
    /// a mouse-plus-pen machine means the one the user last used. Preserved
    /// across an election when it is still live and still hovering-capable, so
    /// a contact arriving alongside a hovering mouse cannot take the role.
    fn elect(&mut self) {
        self.primary = self
            .entries
            .iter()
            .find(|e| e.info.kind == PointerKind::Mouse)
            .or_else(|| self.entries.iter().min_by_key(|e| e.info.id))
            .map(|e| e.info.id);

        let owner_still_valid = self
            .hover_owner
            .and_then(|id| self.get(id))
            .is_some_and(|e| e.hovers());
        if !owner_still_valid {
            self.hover_owner = self
                .entries
                .iter()
                .filter(|e| e.hovers())
                .max_by_key(|e| e.info.id)
                .map(|e| e.info.id);
        }
    }

    /// Hand the hover-owner role to `id`, reporting the pointer that lost it.
    ///
    /// Called when a hovering-capable pointer produces a sample: the later
    /// sample wins, and the caller sends the displaced owner a
    /// [`PointerLeave`](crate::event::WidgetEvent::PointerLeave). A contact is
    /// refused outright — it can never own hover — and so is a pointer that is
    /// not live.
    pub fn claim_hover_owner(&mut self, id: PointerId) -> Option<PointerId> {
        if !self.get(id).is_some_and(|e| e.hovers()) {
            return None;
        }
        let previous = self.hover_owner;
        if previous == Some(id) {
            return None;
        }
        self.hover_owner = Some(id);
        previous.filter(|prev| self.contains(*prev))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pointer::{EventTime, PointerAxes};
    use teksilo_tokens::PenKind;

    fn id(raw: u64) -> PointerId {
        // Mint through the allocator so ids stay monotonic and distinct from
        // `PointerId::MOUSE`; the device key is per-test so nothing collides.
        let alloc = crate::pointer::PointerIdAllocator::global();
        let device = crate::pointer::BackendDeviceKey::new(0x7AB1E ^ raw);
        let out = alloc.begin(device, raw);
        alloc.end(device, raw);
        out
    }

    fn touch(pid: PointerId) -> PointerInfo {
        PointerInfo::touch(pid, EventTime::ZERO)
    }

    fn mouse() -> PointerInfo {
        PointerInfo::mouse(EventTime::ZERO)
    }

    fn pen(pid: PointerId) -> PointerInfo {
        let mut info = PointerInfo::touch(pid, EventTime::ZERO);
        info.kind = PointerKind::Pen(PenKind::Pen);
        info
    }

    fn sample(info: PointerInfo, phase: PointerPhase, at: Point) -> PointerSample {
        PointerSample {
            pointer: info,
            phase,
            position: at,
            button: None,
            modifiers: crate::event::Modifiers::NONE,
            coalesced: Vec::new(),
        }
    }

    #[test]
    fn a_mouse_wins_the_primary_role_over_an_older_contact() {
        let mut table = PointerTable::new();
        let finger = id(1);
        table.admit(touch(finger), Point::ZERO, true);
        assert_eq!(table.primary_id(), Some(finger), "the only live pointer");

        table.admit(mouse(), Point::new(5.0, 5.0), false);
        assert_eq!(
            table.primary_id(),
            Some(PointerId::MOUSE),
            "a mouse always wins the primary role"
        );
    }

    #[test]
    fn a_contact_is_never_the_hover_owner() {
        let mut table = PointerTable::new();
        let finger = id(2);
        table.admit(touch(finger), Point::ZERO, true);
        assert_eq!(table.hover_owner_id(), None, "a finger cannot hover");
        assert_eq!(table.claim_hover_owner(finger), None);
        assert_eq!(table.hover_owner_id(), None);
    }

    #[test]
    fn a_pen_can_own_hover_and_displaces_the_mouse() {
        let mut table = PointerTable::new();
        table.admit(mouse(), Point::ZERO, false);
        assert_eq!(table.hover_owner_id(), Some(PointerId::MOUSE));

        let stylus = id(3);
        table.admit(pen(stylus), Point::new(2.0, 2.0), false);
        // The pen is admitted but does not seize the role until it produces a
        // sample the tree routes — `claim_hover_owner` is that moment.
        assert_eq!(table.hover_owner_id(), Some(PointerId::MOUSE));
        assert_eq!(
            table.claim_hover_owner(stylus),
            Some(PointerId::MOUSE),
            "the displaced owner is reported so it can be sent a leave"
        );
        assert_eq!(table.hover_owner_id(), Some(stylus));
    }

    #[test]
    fn ending_the_hover_owner_falls_back_to_another_hovering_pointer() {
        let mut table = PointerTable::new();
        table.admit(mouse(), Point::ZERO, false);
        let stylus = id(4);
        table.admit(pen(stylus), Point::ZERO, false);
        table.claim_hover_owner(stylus);

        table.end(stylus);
        assert_eq!(table.hover_owner_id(), Some(PointerId::MOUSE));
    }

    #[test]
    fn captures_are_independent_per_pointer() {
        let mut table = PointerTable::new();
        let a = id(5);
        let b = id(6);
        table.admit(touch(a), Point::ZERO, true);
        table.admit(touch(b), Point::new(50.0, 0.0), true);

        let mut arena = crate::arena::WidgetArena::new();
        let w1 = arena.insert(Box::new(crate::test_widgets::FillWidget::new()));
        let w2 = arena.insert(Box::new(crate::test_widgets::FillWidget::new()));
        table.get_mut(a).expect("a is live").captured_by = Some(w1);
        table.get_mut(b).expect("b is live").captured_by = Some(w2);

        table.end(a);
        assert_eq!(
            table.get(b).and_then(|e| e.captured_by),
            Some(w2),
            "ending one contact must leave the other's capture alone"
        );
    }

    #[test]
    fn the_eleventh_contact_is_refused() {
        let mut table = PointerTable::new();
        for n in 0..PointerTable::DEFAULT_CAP {
            let pid = id(100 + n as u64);
            assert_eq!(
                table.begin(&sample(touch(pid), PointerPhase::Down, Point::ZERO)),
                Some(pid),
                "contact {} is within the cap",
                n + 1
            );
        }
        assert_eq!(table.len(), PointerTable::DEFAULT_CAP);

        let overflow = id(200);
        assert_eq!(
            table.begin(&sample(touch(overflow), PointerPhase::Down, Point::ZERO)),
            None,
            "the eleventh contact must be refused, not evicted onto another"
        );
        assert_eq!(table.len(), PointerTable::DEFAULT_CAP);
    }

    #[test]
    fn a_live_pointer_is_refreshed_even_at_the_cap() {
        let mut table = PointerTable::with_cap(1);
        let finger = id(7);
        table.admit(touch(finger), Point::ZERO, true);
        assert_eq!(
            table.admit(touch(finger), Point::new(9.0, 9.0), false),
            Some(finger),
            "the cap bounds pointers, not samples"
        );
        assert_eq!(
            table.get(finger).map(|e| e.position),
            Some(Point::new(9.0, 9.0))
        );
        assert_eq!(
            table.get(finger).map(|e| e.down_position),
            Some(Point::ZERO),
            "a move must not move the press origin"
        );
    }

    #[test]
    fn a_palm_is_refused() {
        let mut table = PointerTable::new();
        let mut info = touch(id(8));
        info.palm = true;
        assert_eq!(
            table.begin(&sample(info, PointerPhase::Down, Point::ZERO)),
            None
        );
        assert!(table.is_empty());
    }

    #[test]
    fn contact_count_ignores_a_resting_mouse() {
        let mut table = PointerTable::new();
        table.admit(mouse(), Point::ZERO, false);
        assert_eq!(
            table.contact_count(),
            0,
            "a hovering mouse is not a contact"
        );

        let mut pressed = mouse();
        pressed.buttons = crate::event::ButtonMask::PRIMARY;
        table.admit(pressed, Point::ZERO, true);
        assert_eq!(table.contact_count(), 1, "a held button is a contact");

        table.admit(touch(id(9)), Point::ZERO, true);
        assert_eq!(table.contact_count(), 2);
    }

    #[test]
    fn retain_active_scrubs_dead_widget_references() {
        let mut arena = crate::arena::WidgetArena::new();
        let live = arena.insert(Box::new(crate::test_widgets::FillWidget::new()));
        let dead = WidgetId::default(); // the slotmap null key: never active

        let mut table = PointerTable::new();
        let finger = id(10);
        table.admit(touch(finger), Point::ZERO, true);
        {
            let entry = table.get_mut(finger).expect("finger is live");
            entry.hovered = Some(dead);
            entry.captured_by = Some(dead);
            entry.hover_within = vec![live, dead];
        }
        table.retain_active(&arena);

        let entry = table.get(finger).expect("finger is still live");
        assert_eq!(entry.hovered, None);
        assert_eq!(entry.captured_by, None);
        assert_eq!(entry.hover_within, vec![live]);
    }

    /// The default axes are carried through unchanged — the table stores the
    /// sample's `PointerInfo` verbatim rather than rebuilding it.
    #[test]
    fn the_entry_keeps_the_samples_own_info() {
        let mut table = PointerTable::new();
        let stylus = id(11);
        let mut info = pen(stylus);
        info.axes = PointerAxes {
            pressure: Some(0.4),
            ..PointerAxes::default()
        };
        table.admit(info, Point::new(1.0, 2.0), true);
        assert_eq!(
            table.get(stylus).map(|e| e.info.axes.pressure),
            Some(Some(0.4))
        );
    }
}
