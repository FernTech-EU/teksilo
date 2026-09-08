// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! One arbitration object per live pointer: who is competing for this press,
//! and which of them owns it.
//!
//! # What this replaces
//!
//! Before this module the framework had exactly one piece of cross-widget
//! arbitration: `drag_observers`, a `Vec<WidgetId>` on the tree holding the
//! draggable ancestors armed by the current press. It was single-pointer (one
//! `Vec` for the whole tree), drag-only (a scrollable, a previewer or an
//! explicit captor could not be a competitor at all), and its decision
//! procedure was implicit in the order of three helper functions.
//!
//! A [`PointerSequence`] is the same idea made explicit and made plural: one
//! per live [`PointerId`](crate::pointer::PointerId), stored on that pointer's
//! [`PointerEntry`](crate::pointer::table::PointerEntry), carrying the frozen hit
//! path, the frozen [`TouchAction`], every enrolled [`SequenceMember`], and the
//! winner once one is decided.
//!
//! # The ordered decision procedure
//!
//! Stated once here, implemented in `widget_tree/pointer_router.rs`:
//!
//! 1. **At press**: hit-test to a target, freeze the hit path (target → root),
//!    intersect and freeze [`TouchAction`] root-to-target, and record the
//!    innermost `gesture_dead_zone` node on that path as the enrolment
//!    boundary.
//! 2. **The raw-preview pass runs first, root-first.** The first ancestor whose
//!    `on_pointer_event` answers `Handled` claims the sequence outright as a
//!    [`MemberRole::RawPreview`] winner. This order is load-bearing —
//!    `rich_text/mouse.rs` documents relying on an outer wrapper seeing a press
//!    before an inner one — so previewers are deliberately **not** folded into
//!    the innermost-first member order below.
//! 3. **An explicit [`capture_pointer`](crate::widget::EventContext::capture_pointer)
//!    from an undecided sequence is an arbitration act**, not plumbing: the
//!    caller is enrolled as [`MemberRole::RawDrag`], and for a precise pointer
//!    with no eligible pan competitor the sequence is decided there and then.
//!    Three shipped widgets drive their whole interaction this way — the
//!    splitter handle, the dock resize handle and the table column grip all
//!    return `Ignored` from `on_pointer_event`, capture, and work from
//!    `PointerMove` with no recognizer at all.
//! 4. **On move while undecided**: timers before positional thresholds, then
//!    members innermost-first. A `RawDrag` wins past `drag_slop`; a `Gesture`
//!    wins when its own recognizer recognizes; a [`MemberRole::Pan`] wins only
//!    on an axis the frozen `TouchAction` permits and only past `pan_slop`.
//! 5. **On up**: the release sweep — the innermost still-`Possible` member with
//!    a completable gesture wins, which is the pre-existing
//!    `arena.process(Up) -> Tap`.
//!
//! # Why the mouse is unchanged
//!
//! [`GestureProfile::pan_slop`] is `None` for a mouse and
//! [`PanClaim::devices`] defaults to direct pointers, so **no pan member is
//! ever eligible for a mouse**. Every mouse sequence is therefore either
//! decided at press (an explicit capture) or arbitrated exactly as
//! `drag_observers` arbitrated it: ancestors innermost-first, each latching at
//! its own `drag_slop`, which for the mouse profile is the 5.0 it has always
//! been. On touch the same widget defers by `drag_slop` (18) and still beats a
//! scroller, because `pan_slop` (36) is larger.
//!
//! Reference: `docs/events-and-gestures.md`.

use teksilo_canvas::Point;
use teksilo_tokens::{DragActivation, GestureProfile};

use crate::pointer::touch_action::{Axis, PanClaim, TouchAction};
use crate::pointer::{EventTime, PointerInfo};
use crate::widget_id::WidgetId;

/// What a member is competing *as*.
///
/// The role decides which threshold the member wins on and, for `Pan`, which
/// axes the frozen [`TouchAction`] has to permit.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MemberRole {
    /// A node whose own gesture recognizers (`on_drag` / `on_swipe`) are
    /// competing. This is what `drag_observers` used to hold, and it is what an
    /// ancestor of the pressed control is enrolled as.
    Gesture,
    /// A scroll container that declared a [`PanClaim`]. Only ever enrolled for
    /// a pointer kind the claim's `devices` mask admits and only when the
    /// pointer's profile has a `pan_slop` — so never for a mouse.
    Pan(PanClaim),
    /// A node that took the pointer by an explicit
    /// [`capture_pointer`](crate::widget::EventContext::capture_pointer) while
    /// the sequence was undecided, and drives its interaction from
    /// `PointerMove` rather than from a recognizer.
    RawDrag,
    /// A node that answered `Handled` from the root-first preview pass. It has
    /// already won by the time it is enrolled; the role exists so
    /// [`WidgetTree::sequence_members`](crate::WidgetTree::sequence_members)
    /// can report *why*.
    RawPreview,
}

/// Where one member stands in the arbitration.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberState {
    /// Still in the running.
    Possible,
    /// Deferring its own decision — see
    /// [`hold_gesture`](crate::widget::EventContext::hold_gesture). Released
    /// automatically at `profile.max_hold`; the framework itself never holds.
    Held,
    /// Out of the running, either by its own choice
    /// ([`reject_gesture`](crate::widget::EventContext::reject_gesture)), by a
    /// threshold it can no longer meet, or because a peer won.
    Rejected,
    /// The winner.
    Won,
}

/// One competitor for a pointer sequence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SequenceMember {
    /// The competing node.
    pub id: WidgetId,
    /// What it is competing as.
    pub role: MemberRole,
    /// The earliest time this member may win, when its activation defers it.
    /// `None` means "as soon as its threshold is met".
    pub eligible_at: Option<EventTime>,
    /// Where it stands.
    pub state: MemberState,
    /// When `true`, this member self-rejects the moment the pointer leaves the
    /// tap boundary — the [`DragActivation::AfterLongPress`] rule.
    pub(crate) rejects_on_tap_slop: bool,
    /// When [`state`](Self::state) became [`MemberState::Held`].
    pub(crate) held_since: Option<EventTime>,
}

impl SequenceMember {
    /// A fresh member in the running.
    pub(crate) fn new(id: WidgetId, role: MemberRole) -> Self {
        Self {
            id,
            role,
            eligible_at: None,
            state: MemberState::Possible,
            rejects_on_tap_slop: false,
            held_since: None,
        }
    }

    /// Whether this member could still win.
    pub fn is_live(&self) -> bool {
        matches!(self.state, MemberState::Possible | MemberState::Held)
    }

    /// Whether this member may win at `now`. A member deferred by
    /// [`DragActivation::AfterLongPress`] cannot win before its timer, and a
    /// holding member cannot win at all until it releases.
    pub fn is_eligible_at(&self, now: EventTime) -> bool {
        self.state == MemberState::Possible
            && self.eligible_at.is_none_or(|deadline| now >= deadline)
    }
}

/// Where a press stops being a tap.
///
/// One predicate, three consumers: it fails a tap, it triggers
/// [`GestureArenaSet::cancel_taps`](super::GestureArenaSet::cancel_taps), and
/// it will clear the framework press visual. A coarse pointer uses `Bounds`
/// because a finger's reported centre wanders several device pixels while
/// resting inside the control it is pressing; a precise pointer keeps the
/// radius it always had.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TapBoundary {
    /// The press fails once it travels further than this from its origin.
    Radius(f32),
    /// The press fails once it leaves the pressed node's bounds.
    Bounds,
}

impl TapBoundary {
    /// The boundary a pointer of this kind uses.
    ///
    /// `Radius(profile.tap_slop)` for a precise pointer — the pre-existing
    /// rule, unchanged — and `Bounds` for a coarse one.
    pub fn for_pointer(pointer: &PointerInfo, profile: &GestureProfile) -> Self {
        if pointer.kind.is_coarse() {
            Self::Bounds
        } else {
            Self::Radius(profile.tap_slop)
        }
    }

    /// Whether `position` has left the boundary, given the press origin and the
    /// pressed node's bounds in the same (window-logical) space.
    ///
    /// A `Bounds` boundary with no bounds to test against — the pressed node
    /// went away — falls back to the radius, so the answer is never "the press
    /// can travel anywhere".
    ///
    /// So does a `Bounds` boundary whose press **began outside the node**. A
    /// press can be accepted for a node it did not land in: a control that
    /// declares a [`Widget::hit_outset`] is offered the ring around it (a 12 dp
    /// twist arrow lifted to a 24 dp target, a 16 dp clear affordance inside a
    /// text field), and the miss-only slop pass re-attributes a near miss the
    /// same way. For such a press the node's rectangle never contained the
    /// origin, so testing `position` against it alone would report the press as
    /// already-left at the instant it arrived — and the tap could never
    /// complete, silently making the whole outset mechanism useless to every
    /// coarse-pointer tap. The rule for that case is the pointer's own radius
    /// around where it landed, unioned with the node's bounds so sliding *onto*
    /// the control keeps the press alive. Android's `ViewGroup` takes the same
    /// shape from the other direction (`pointInView(x, y, mTouchSlop)` — the
    /// view's rect inflated by touch slop).
    ///
    /// A press that began inside the node is untouched: the rectangle is the
    /// boundary, exactly as before.
    ///
    /// [`Widget::hit_outset`]: crate::widget::Widget::hit_outset
    pub fn left(
        &self,
        origin: Point,
        position: Point,
        bounds: Option<teksilo_canvas::Rect>,
        profile: &GestureProfile,
    ) -> bool {
        match self {
            Self::Radius(radius) => super::distance(origin, position) > *radius,
            Self::Bounds => match bounds {
                Some(rect) if rect.contains(origin) => !rect.contains(position),
                Some(rect) => {
                    !rect.contains(position) && super::distance(origin, position) > profile.tap_slop
                }
                None => super::distance(origin, position) > profile.tap_slop,
            },
        }
    }
}

/// Everything the tree knows about one press: who is competing for it, and who
/// won.
///
/// Lives in [`PointerEntry::sequence`](crate::pointer::table::PointerEntry::sequence)
/// for as long as the pointer is down. The ordered decision procedure this
/// type is the state of is written out in `docs/events-and-gestures.md` §4.2 and
/// in this module's own header.
#[derive(Debug, Clone, PartialEq)]
pub struct PointerSequence {
    pointer: PointerInfo,
    path: Vec<WidgetId>,
    touch_action: TouchAction,
    dead_zone_boundary: Option<WidgetId>,
    members: Vec<SequenceMember>,
    winner: Option<WidgetId>,
    capture: Option<WidgetId>,
    press_origin: Point,
    last_position: Point,
    started_at: EventTime,
    pressed_owner: Option<WidgetId>,
    terminating: bool,
    taps_cancelled: bool,
}

impl PointerSequence {
    /// Open a sequence for `pointer`'s press at `origin`.
    ///
    /// `path` runs target → root and is **frozen**: a rebuild mid-gesture
    /// cannot change who was competing for a press that has already started.
    pub fn new(
        pointer: PointerInfo,
        path: Vec<WidgetId>,
        touch_action: TouchAction,
        dead_zone_boundary: Option<WidgetId>,
        origin: Point,
        started_at: EventTime,
    ) -> Self {
        Self {
            pointer,
            path,
            touch_action,
            dead_zone_boundary,
            members: Vec::new(),
            winner: None,
            capture: None,
            press_origin: origin,
            last_position: origin,
            started_at,
            pressed_owner: None,
            terminating: false,
            taps_cancelled: false,
        }
    }

    /// Which pointer this sequence follows.
    pub fn pointer(&self) -> PointerInfo {
        self.pointer
    }

    /// The frozen hit path, target → root.
    pub fn path(&self) -> &[WidgetId] {
        &self.path
    }

    /// The [`TouchAction`] frozen at press — the intersection of every
    /// declaration from the root down to the pressed target.
    pub fn touch_action(&self) -> TouchAction {
        self.touch_action
    }

    /// The innermost `gesture_dead_zone` node on the frozen path, if any.
    /// Nothing at or above it may be enrolled.
    pub fn dead_zone_boundary(&self) -> Option<WidgetId> {
        self.dead_zone_boundary
    }

    /// Every enrolled member, innermost first.
    pub fn members(&self) -> &[SequenceMember] {
        &self.members
    }

    /// The node that owns this press, once one has been decided.
    pub fn winner(&self) -> Option<WidgetId> {
        self.winner
    }

    /// Whether arbitration is over.
    pub fn is_decided(&self) -> bool {
        self.winner.is_some()
    }

    /// The node holding this pointer's capture, as the sequence recorded it.
    pub fn capture(&self) -> Option<WidgetId> {
        self.capture
    }

    /// Record who holds the capture.
    pub fn set_capture(&mut self, captor: Option<WidgetId>) {
        self.capture = captor;
    }

    /// The node whose gesture arena took the press — the tap owner, when the
    /// press was not claimed by anything else.
    pub fn pressed_owner(&self) -> Option<WidgetId> {
        self.pressed_owner
    }

    /// Record the node whose gesture arena took the press.
    pub fn set_pressed_owner(&mut self, owner: Option<WidgetId>) {
        self.pressed_owner = owner;
    }

    /// Where the press landed.
    pub fn press_origin(&self) -> Point {
        self.press_origin
    }

    /// Where the pointer was at its most recent sample.
    pub fn last_position(&self) -> Point {
        self.last_position
    }

    /// Record the pointer's current position.
    pub fn set_last_position(&mut self, position: Point) {
        self.last_position = position;
    }

    /// When the press landed, on the tree's input timeline.
    pub fn started_at(&self) -> EventTime {
        self.started_at
    }

    /// Whether the press has already been told it is no longer a tap.
    ///
    /// The `cancel_taps` revocation fires **once** per press: it resets the
    /// node's [`TapStreak`](super::TapStreak), and repeating it on every
    /// subsequent move would keep clearing state a live drag may still want.
    pub fn taps_cancelled(&self) -> bool {
        self.taps_cancelled
    }

    /// Record that the tap family has been revoked for this press.
    pub fn set_taps_cancelled(&mut self) {
        self.taps_cancelled = true;
    }

    /// Whether the sequence is inside its own terminal dispatch — set while the
    /// `Up` that ends it is being delivered, so a teardown triggered from a
    /// handler cannot cancel a press that has already completed.
    pub fn is_terminating(&self) -> bool {
        self.terminating
    }

    /// Mark the sequence as inside its terminal dispatch.
    pub fn set_terminating(&mut self, terminating: bool) {
        self.terminating = terminating;
    }

    /// How far the pointer has travelled from the press point.
    pub fn travel(&self) -> f32 {
        super::distance(self.press_origin, self.last_position)
    }

    /// How far the pointer has travelled along one axis.
    pub fn travel_on(&self, axis: Axis) -> f32 {
        match axis {
            Axis::X => (self.last_position.x - self.press_origin.x).abs(),
            Axis::Y => (self.last_position.y - self.press_origin.y).abs(),
        }
    }

    /// The slop a positional member of this sequence latches at.
    ///
    /// `profile.drag_slop` in every configuration **except** a direct pointer
    /// under a frozen [`TouchAction::NONE`], where the subtree has declared
    /// that a contact does nothing but manipulate it and the jitter floor is
    /// the right threshold. A precise pointer always uses `drag_slop`: reading
    /// `slop_precise` for it would silently retune every mouse drag latch from
    /// 5 dp to 2.
    pub fn latch_slop(&self, profile: &GestureProfile) -> f32 {
        if self.pointer.kind.is_direct() && self.touch_action.is_none() {
            profile.slop_precise
        } else {
            profile.drag_slop
        }
    }

    /// Whether `id` may be enrolled at all: it must be on the frozen path and
    /// strictly below the dead-zone boundary.
    pub fn may_enrol(&self, id: WidgetId) -> bool {
        let Some(index) = self.path.iter().position(|p| *p == id) else {
            return false;
        };
        match self.dead_zone_boundary {
            Some(boundary) => match self.path.iter().position(|p| *p == boundary) {
                Some(boundary_index) => index < boundary_index,
                None => true,
            },
            None => true,
        }
    }

    /// Depth of `id` on the frozen path, innermost first. Used to keep
    /// [`members`](Self::members) sorted no matter what order enrolment
    /// happened in.
    fn depth_of(&self, id: WidgetId) -> usize {
        self.path
            .iter()
            .position(|p| *p == id)
            .unwrap_or(usize::MAX)
    }

    /// Whether `id` is already enrolled.
    pub fn has_member(&self, id: WidgetId) -> bool {
        self.members.iter().any(|m| m.id == id)
    }

    /// Enrol `id` as a competitor, keeping the member list innermost-first.
    ///
    /// Refused — and reported as `false` — when `id` is at or above the
    /// dead-zone boundary, when it is not on the frozen path, or when it is
    /// already enrolled.
    pub fn enrol(&mut self, id: WidgetId, role: MemberRole) -> bool {
        if self.has_member(id) || !self.may_enrol(id) {
            return false;
        }
        let member = SequenceMember::new(id, role);
        let depth = self.depth_of(id);
        let at = self
            .members
            .iter()
            .position(|m| self.depth_of(m.id) > depth)
            .unwrap_or(self.members.len());
        self.members.insert(at, member);
        true
    }

    /// Enrol a drag member whose [`DragActivation`] defers it.
    ///
    /// `AfterLongPress` (and `Auto` resolving to it) sets `eligible_at` to the
    /// long-press deadline and arms the self-rejection rule: the member is out
    /// the moment the press travels past the tap boundary, because that travel
    /// is a pan, not a considered grab.
    pub fn enrol_drag(
        &mut self,
        id: WidgetId,
        role: MemberRole,
        activation: DragActivation,
        profile: &GestureProfile,
    ) -> bool {
        if !self.enrol(id, role) {
            return false;
        }
        if self.resolve_activation(activation) == DragActivation::AfterLongPress
            && let Some(member) = self.members.iter_mut().find(|m| m.id == id)
        {
            member.eligible_at = Some(self.started_at + profile.long_press);
            member.rejects_on_tap_slop = true;
        }
        true
    }

    /// What [`DragActivation::Auto`] means for this sequence.
    ///
    /// `Immediate` for a precise pointer or a subtree that has declared
    /// [`TouchAction::NONE`] (nothing else can want the press); `AfterLongPress`
    /// for a coarse pointer with an eligible pan competitor, because the axis
    /// is already spoken for.
    pub fn resolve_activation(&self, activation: DragActivation) -> DragActivation {
        match activation {
            DragActivation::Auto => {
                if !self.pointer.kind.is_direct() || self.touch_action.is_none() {
                    DragActivation::Immediate
                } else if self.has_eligible_pan() {
                    DragActivation::AfterLongPress
                } else {
                    DragActivation::Immediate
                }
            }
            other => other,
        }
    }

    /// Whether any live member is a pan claimant. A mouse never has one:
    /// [`GestureProfile::pan_slop`] is `None` for it and [`PanClaim::devices`]
    /// admits only direct pointers.
    pub fn has_eligible_pan(&self) -> bool {
        self.members
            .iter()
            .any(|m| m.is_live() && matches!(m.role, MemberRole::Pan(_)))
    }

    /// Whether `claim` is eligible for this sequence's pointer at all: the
    /// claim must admit the device, the pointer's profile must have a pan slop,
    /// and the frozen [`TouchAction`] must permit at least one claimed axis.
    pub fn pan_is_eligible(&self, claim: &PanClaim, profile: &GestureProfile) -> bool {
        if profile.pan_slop.is_none() {
            return false;
        }
        if !claim.devices.contains(self.pointer.kind) {
            return false;
        }
        [Axis::X, Axis::Y]
            .into_iter()
            .any(|axis| claim.axes.contains(axis) && self.touch_action.allows_pan(axis))
    }

    /// The axis a pan member of this sequence would win on, if its travel has
    /// passed `pan_slop` on one the claim and the frozen action both permit.
    ///
    /// A diagonal tie resolves by **dominant axis** — the one that has moved
    /// further — so a pan that is mostly vertical scrolls vertically even when
    /// both axes are claimed.
    pub fn pan_axis_past_slop(&self, claim: &PanClaim, profile: &GestureProfile) -> Option<Axis> {
        let slop = profile.pan_slop?;
        let mut candidates: Vec<(Axis, f32)> = [Axis::X, Axis::Y]
            .into_iter()
            .filter(|axis| claim.axes.contains(*axis) && self.touch_action.allows_pan(*axis))
            .map(|axis| (axis, self.travel_on(axis)))
            .filter(|(_, travel)| *travel >= slop)
            .collect();
        // Dominant axis first; ties keep X, which is the declaration order.
        candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
        candidates.first().map(|(axis, _)| *axis)
    }

    /// Declare `id` the winner and reject every other live member.
    ///
    /// Returns the members that were knocked out, so the caller can cancel each
    /// exactly once.
    pub fn decide(&mut self, id: WidgetId) -> Vec<WidgetId> {
        self.winner = Some(id);
        let mut losers = Vec::new();
        for member in &mut self.members {
            if member.id == id {
                member.state = MemberState::Won;
            } else if member.is_live() {
                member.state = MemberState::Rejected;
                losers.push(member.id);
            }
        }
        losers
    }

    /// Withdraw `id` from the running.
    pub fn reject(&mut self, id: WidgetId) {
        if let Some(member) = self.members.iter_mut().find(|m| m.id == id)
            && member.is_live()
        {
            member.state = MemberState::Rejected;
        }
    }

    /// Defer `id`'s decision until it releases or `profile.max_hold` elapses.
    pub fn hold(&mut self, id: WidgetId, now: EventTime) {
        if let Some(member) = self.members.iter_mut().find(|m| m.id == id)
            && member.state == MemberState::Possible
        {
            member.state = MemberState::Held;
            member.held_since = Some(now);
        }
    }

    /// End `id`'s hold, putting it back in the running.
    pub fn release_hold(&mut self, id: WidgetId) {
        if let Some(member) = self.members.iter_mut().find(|m| m.id == id)
            && member.state == MemberState::Held
        {
            member.state = MemberState::Possible;
            member.held_since = None;
        }
    }

    /// Release every hold older than `profile.max_hold`.
    ///
    /// A hold exists so an **application** recognizer can await an
    /// asynchronous decision; leaving one standing would strand the press, so
    /// the framework times it out rather than trusting the holder.
    pub fn expire_holds(&mut self, now: EventTime, profile: &GestureProfile) {
        for member in &mut self.members {
            if member.state == MemberState::Held
                && let Some(since) = member.held_since
                && now.saturating_since(since) >= profile.max_hold
            {
                member.state = MemberState::Possible;
                member.held_since = None;
            }
        }
    }

    /// Whether any member is holding.
    pub fn is_held(&self) -> bool {
        self.members.iter().any(|m| m.state == MemberState::Held)
    }

    /// When [`expire_holds`](Self::expire_holds) next has work: the earliest
    /// instant at which a standing hold reaches `profile.max_hold`.
    ///
    /// **A deferred member's `eligible_at` is deliberately not a term here.**
    /// It looks like a sibling deadline and is not one. Nothing happens at that
    /// instant: eligibility is never *stored*, it is re-derived by
    /// [`SequenceMember::is_eligible_at`] against whatever instant its caller
    /// names, and no reader *transitions* anything on reaching it. Two call
    /// sites read it — the arbitration walk, and the arena gate the ordinary
    /// bubble and the timer tick share, the one naming the sample being
    /// dispatched and the other the tick's own instant — and each of them only
    /// answers a question its caller already had. A press that has sat
    /// still past its `long_press` is already eligible the moment it moves,
    /// with no intervening tick, so waking the event loop at `eligible_at`
    /// would buy an idle frame with nothing to do in it. The expiry of a hold
    /// is the opposite: it is a stored state transition, and if nobody performs
    /// it the hold stands past the duration the framework promises to trust it
    /// for.
    pub fn next_hold_deadline(&self, profile: &GestureProfile) -> Option<EventTime> {
        self.members
            .iter()
            .filter(|m| m.state == MemberState::Held)
            .filter_map(|m| m.held_since.map(|since| since + profile.max_hold))
            .min()
    }

    /// Drop every member whose node is no longer active, reporting them so the
    /// caller can cancel each individually.
    ///
    /// Run every sample: a rebuild mints fresh ids, and a member left pointing
    /// at a destroyed node would either be fed events forever or silently win.
    /// The *sequence* dies only when the winner or the captor dies — see
    /// [`lost_owner`](Self::lost_owner).
    pub fn revalidate(&mut self, arena: &crate::arena::WidgetArena) -> Vec<WidgetId> {
        let mut dead = Vec::new();
        self.members.retain(|member| {
            if arena.is_active(member.id) {
                true
            } else {
                dead.push(member.id);
                false
            }
        });
        dead
    }

    /// Whether the node that owns this sequence — its winner, or failing that
    /// its captor — has gone away. The sequence itself must then be cancelled.
    pub fn lost_owner(&self, arena: &crate::arena::WidgetArena) -> bool {
        let owner = self.winner.or(self.capture);
        owner.is_some_and(|id| !arena.is_active(id))
    }

    /// The role and state of every member, for
    /// [`WidgetTree::sequence_members`](crate::WidgetTree::sequence_members).
    pub fn member_report(&self) -> Vec<(WidgetId, MemberRole, MemberState)> {
        self.members
            .iter()
            .map(|m| (m.id, m.role, m.state))
            .collect()
    }

    /// Every live member of one role, innermost first.
    pub(crate) fn live_ids_with<F: Fn(&MemberRole) -> bool>(&self, filter: F) -> Vec<WidgetId> {
        self.members
            .iter()
            .filter(|m| m.is_live() && filter(&m.role))
            .map(|m| m.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pointer::{BackendDeviceKey, PointerIdAllocator};
    use crate::widget_id::WidgetId;
    use slotmap::KeyData;
    use teksilo_tokens::{PointerKind, TargetDensity};

    fn tokens() -> teksilo_tokens::InputTokens {
        teksilo_tokens::InputTokens::for_density(TargetDensity::Compact)
    }

    fn mouse() -> PointerInfo {
        PointerInfo::mouse(EventTime::ZERO)
    }

    fn finger() -> PointerInfo {
        let id = PointerIdAllocator::global().begin(BackendDeviceKey::DEFAULT, 7);
        PointerInfo::touch(id, EventTime::ZERO)
    }

    fn seq(pointer: PointerInfo, action: TouchAction, path: Vec<WidgetId>) -> PointerSequence {
        PointerSequence::new(pointer, path, action, None, Point::ZERO, EventTime::ZERO)
    }

    /// Synthetic ids for the pure-logic tests: the sequence only ever compares
    /// and orders them, so no arena is needed to make them meaningful.
    fn ids(n: u64) -> Vec<WidgetId> {
        (0..n)
            .map(|i| KeyData::from_ffi((1u64 << 32) | (i + 1)).into())
            .collect()
    }

    #[test]
    fn a_mouse_latches_at_five_in_every_configuration() {
        // The single most important invariant in the package: no frozen
        // TouchAction, and no density, may retune the mouse drag latch.
        let tokens = tokens();
        let profile = tokens.profile(PointerKind::Mouse);
        for action in [
            TouchAction::AUTO,
            TouchAction::NONE,
            TouchAction::PAN,
            TouchAction::PAN_X,
            TouchAction::PAN_Y,
            TouchAction::PINCH_ZOOM,
            TouchAction::MANIPULATION,
        ] {
            let s = seq(mouse(), action, ids(1));
            assert_eq!(
                s.latch_slop(profile),
                5.0,
                "a mouse under {action:?} must latch at 5.0"
            );
        }
    }

    #[test]
    fn slop_precise_reaches_only_a_direct_pointer_under_a_frozen_none() {
        let tokens = tokens();
        let touch_profile = tokens.profile(PointerKind::Touch);
        let none = seq(finger(), TouchAction::NONE, ids(1));
        assert_eq!(none.latch_slop(touch_profile), touch_profile.slop_precise);
        let auto = seq(finger(), TouchAction::AUTO, ids(1));
        assert_eq!(auto.latch_slop(touch_profile), touch_profile.drag_slop);
    }

    #[test]
    fn a_mouse_never_has_an_eligible_pan_member() {
        let tokens = tokens();
        let profile = tokens.profile(PointerKind::Mouse);
        let s = seq(mouse(), TouchAction::AUTO, ids(1));
        assert!(!s.pan_is_eligible(&PanClaim::both(), profile));
    }

    #[test]
    fn members_stay_innermost_first_whatever_order_they_enrol_in() {
        let path = ids(4);
        let mut s = seq(mouse(), TouchAction::AUTO, path.clone());
        assert!(s.enrol(path[3], MemberRole::Gesture));
        assert!(s.enrol(path[1], MemberRole::RawDrag));
        assert!(s.enrol(path[2], MemberRole::Gesture));
        let order: Vec<_> = s.members().iter().map(|m| m.id).collect();
        assert_eq!(order, vec![path[1], path[2], path[3]]);
    }

    #[test]
    fn the_dead_zone_boundary_refuses_everything_at_or_above_it() {
        let path = ids(4);
        let mut s = PointerSequence::new(
            mouse(),
            path.clone(),
            TouchAction::AUTO,
            Some(path[2]),
            Point::ZERO,
            EventTime::ZERO,
        );
        assert!(s.enrol(path[1], MemberRole::Gesture), "below the boundary");
        assert!(
            !s.enrol(path[2], MemberRole::Gesture),
            "the boundary itself"
        );
        assert!(!s.enrol(path[3], MemberRole::Gesture), "above the boundary");
    }

    #[test]
    fn deciding_rejects_every_other_live_member_exactly_once() {
        let path = ids(3);
        let mut s = seq(mouse(), TouchAction::AUTO, path.clone());
        s.enrol(path[0], MemberRole::Gesture);
        s.enrol(path[1], MemberRole::Gesture);
        s.enrol(path[2], MemberRole::Gesture);
        let losers = s.decide(path[1]);
        assert_eq!(losers, vec![path[0], path[2]]);
        assert_eq!(s.winner(), Some(path[1]));
        // A second decide reports nothing new: the losers are no longer live.
        assert!(s.decide(path[1]).is_empty());
    }

    #[test]
    fn after_long_press_defers_eligibility_and_arms_self_rejection() {
        let tokens = tokens();
        let profile = tokens.profile(PointerKind::Touch);
        let path = ids(2);
        let mut s = seq(finger(), TouchAction::PAN_Y, path.clone());
        s.enrol_drag(
            path[0],
            MemberRole::Gesture,
            DragActivation::AfterLongPress,
            profile,
        );
        let member = s.members()[0];
        assert_eq!(
            member.eligible_at,
            Some(EventTime::ZERO + profile.long_press)
        );
        assert!(member.rejects_on_tap_slop);
        assert!(!member.is_eligible_at(EventTime::ZERO));
        assert!(member.is_eligible_at(EventTime::ZERO + profile.long_press));
    }

    #[test]
    fn auto_activation_defers_only_a_coarse_pointer_facing_a_pan() {
        let path = ids(2);

        // A mouse is always immediate.
        let mut m = seq(mouse(), TouchAction::AUTO, path.clone());
        m.enrol(path[1], MemberRole::Pan(PanClaim::vertical()));
        assert_eq!(
            m.resolve_activation(DragActivation::Auto),
            DragActivation::Immediate
        );

        // A finger with no pan competitor is immediate too.
        let bare = seq(finger(), TouchAction::AUTO, path.clone());
        assert_eq!(
            bare.resolve_activation(DragActivation::Auto),
            DragActivation::Immediate
        );

        // A finger facing a pan claimant defers.
        let mut contested = seq(finger(), TouchAction::AUTO, path.clone());
        contested.enrol(path[1], MemberRole::Pan(PanClaim::vertical()));
        assert_eq!(
            contested.resolve_activation(DragActivation::Auto),
            DragActivation::AfterLongPress
        );
    }

    #[test]
    fn a_pan_wins_on_the_dominant_axis_and_only_where_permitted() {
        let tokens = tokens();
        let profile = tokens.profile(PointerKind::Touch);
        let slop = profile.pan_slop.expect("touch pans");
        let path = ids(1);

        let mut s = seq(finger(), TouchAction::PAN, path);
        s.set_last_position(Point::new(slop + 10.0, slop + 1.0));
        assert_eq!(
            s.pan_axis_past_slop(&PanClaim::both(), profile),
            Some(Axis::X),
            "the axis that travelled further wins the diagonal"
        );

        // The frozen action forbids X, so the same travel resolves to Y.
        let mut only_y = seq(finger(), TouchAction::PAN_Y, ids(1));
        only_y.set_last_position(Point::new(slop + 10.0, slop + 1.0));
        assert_eq!(
            only_y.pan_axis_past_slop(&PanClaim::both(), profile),
            Some(Axis::Y)
        );
    }

    #[test]
    fn a_hold_expires_at_max_hold_and_not_before() {
        let tokens = tokens();
        let profile = tokens.profile(PointerKind::Mouse);
        let path = ids(1);
        let mut s = seq(mouse(), TouchAction::AUTO, path.clone());
        s.enrol(path[0], MemberRole::Gesture);
        s.hold(path[0], EventTime::ZERO);
        assert!(s.is_held());

        s.expire_holds(EventTime::from_duration(profile.max_hold / 2), profile);
        assert!(s.is_held(), "a hold survives until max_hold");

        s.expire_holds(EventTime::from_duration(profile.max_hold), profile);
        assert!(!s.is_held(), "and is released at it");
        assert_eq!(s.members()[0].state, MemberState::Possible);
    }

    #[test]
    fn revalidate_drops_dead_members_one_at_a_time() {
        // The tree-level half — losing the captor cancels the whole sequence —
        // is pinned in `gesture_dispatch_impl`; in a real tree a member is
        // always an ancestor of the captor and so cannot die on its own, which
        // is why the per-member rule is asserted here.
        let mut arena = crate::arena::WidgetArena::new();
        let live = arena.insert(Box::new(crate::test_widgets::FillWidget::new()));
        let doomed = arena.insert(Box::new(crate::test_widgets::FillWidget::new()));
        let mut s = seq(mouse(), TouchAction::AUTO, vec![doomed, live]);
        s.enrol(doomed, MemberRole::Gesture);
        s.enrol(live, MemberRole::Gesture);
        s.set_capture(Some(live));

        assert!(s.revalidate(&arena).is_empty(), "nothing has died yet");
        arena.destroy(doomed);

        assert_eq!(s.revalidate(&arena), vec![doomed]);
        assert_eq!(
            s.members().iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![live],
            "only the dead member is dropped"
        );
        assert!(!s.lost_owner(&arena), "the captor is still alive");

        arena.destroy(live);
        assert!(
            s.lost_owner(&arena),
            "losing the captor is what cancels the sequence"
        );
    }

    #[test]
    fn the_tap_boundary_is_a_radius_for_a_mouse_and_bounds_for_a_finger() {
        let tokens = tokens();
        let mouse_profile = tokens.profile(PointerKind::Mouse);
        let touch_profile = tokens.profile(PointerKind::Touch);
        assert_eq!(
            TapBoundary::for_pointer(&mouse(), mouse_profile),
            TapBoundary::Radius(mouse_profile.tap_slop)
        );
        assert_eq!(
            TapBoundary::for_pointer(&finger(), touch_profile),
            TapBoundary::Bounds
        );

        // A coarse press well past tap_slop but still inside the control has
        // NOT left the boundary — that is the whole point of `Bounds`.
        let bounds = teksilo_canvas::Rect::new(0.0, 0.0, 100.0, 100.0);
        assert!(!TapBoundary::Bounds.left(
            Point::new(50.0, 50.0),
            Point::new(50.0, 80.0),
            Some(bounds),
            touch_profile,
        ));
        assert!(TapBoundary::Bounds.left(
            Point::new(50.0, 50.0),
            Point::new(50.0, 120.0),
            Some(bounds),
            touch_profile,
        ));
        // With no bounds to test, it falls back to the radius rather than
        // letting the press travel anywhere.
        assert!(TapBoundary::Bounds.left(
            Point::new(50.0, 50.0),
            Point::new(50.0, 80.0),
            None,
            touch_profile,
        ));
    }

    /// A press accepted through a `Widget::hit_outset` begins outside the node
    /// it was accepted for, so the node's rectangle cannot be its boundary:
    /// with `Bounds` taken literally the press is "already gone" on arrival and
    /// the tap can never complete. It falls back to the pointer's own radius
    /// around where it landed, and sliding onto the control keeps it alive.
    #[test]
    fn a_press_that_began_outside_the_node_is_bounded_by_its_own_radius() {
        let tokens = tokens();
        let touch_profile = tokens.profile(PointerKind::Touch);
        let rect = teksilo_canvas::Rect::new(0.0, 0.0, 12.0, 12.0);
        // Landed 4 dp past the trailing edge — inside the outset ring the
        // arena offered it, outside the rectangle.
        let origin = Point::new(16.0, 6.0);
        assert!(
            !TapBoundary::Bounds.left(origin, origin, Some(rect), touch_profile),
            "a press cannot have left the boundary on the sample that opened it",
        );
        assert!(
            !TapBoundary::Bounds.left(origin, Point::new(6.0, 6.0), Some(rect), touch_profile),
            "sliding onto the control keeps the press",
        );
        assert!(
            TapBoundary::Bounds.left(
                origin,
                Point::new(16.0 + touch_profile.tap_slop + 1.0, 6.0),
                Some(rect),
                touch_profile,
            ),
            "and past the radius it is gone, so the abort gesture still works",
        );
    }

    /// The union term, on its own.
    ///
    /// The radius half of the outside-origin rule is a *travel* allowance, and
    /// on a small control it runs out before the finger has finished arriving:
    /// a contact that lands in the outset ring of a wide control and then
    /// slides well past `tap_slop` **onto** the control is further from its
    /// origin than the radius permits and squarely inside the rectangle. Only
    /// the union with the node's bounds keeps that press alive; with the
    /// `!rect.contains(position)` term gone, the radius alone kills a press
    /// that is sitting on the middle of the thing it is pressing.
    #[test]
    fn sliding_onto_the_control_keeps_a_press_the_radius_alone_would_lose() {
        let tokens = tokens();
        let touch_profile = tokens.profile(PointerKind::Touch);
        let rect = teksilo_canvas::Rect::new(0.0, 0.0, 100.0, 20.0);
        // 4 dp past the trailing edge — inside the outset ring, outside the rect.
        let origin = Point::new(104.0, 10.0);
        // 24 dp of travel, against a Touch `tap_slop` of 18: past the radius,
        // and 20 dp inside the control.
        let onto = Point::new(80.0, 10.0);
        assert!(
            super::super::distance(origin, onto) > touch_profile.tap_slop,
            "the probe is only discriminating while the travel exceeds tap_slop",
        );
        assert!(rect.contains(onto), "…and lands inside the control");
        assert!(
            !TapBoundary::Bounds.left(origin, onto, Some(rect), touch_profile),
            "a finger resting on the control it pressed has not left it",
        );
    }

    /// Which rule applies is decided by the **origin**, not by where the
    /// pointer is now.
    ///
    /// The two questions agree on most samples, which is why the distinction
    /// has to be pinned on the one geometry where they cannot: a press that
    /// began *inside* the node and has moved a short way outside it. The rule
    /// for that press is the rectangle — it left the moment it crossed the
    /// edge, however little it travelled — while a press that began outside is
    /// allowed the pointer's radius around where it landed, so with the same
    /// `position` it has not left at all. Reading `position` instead of
    /// `origin` collapses both onto the second answer and silently hands every
    /// coarse press that starts inside a control a `tap_slop` grace band
    /// outside it, which is exactly the slop `Bounds` exists to replace.
    #[test]
    fn the_boundary_rule_is_chosen_by_where_the_press_began() {
        let tokens = tokens();
        let touch_profile = tokens.profile(PointerKind::Touch);
        let rect = teksilo_canvas::Rect::new(0.0, 0.0, 100.0, 20.0);
        // One sample, 4 dp past the trailing edge, reached from two origins —
        // both within `tap_slop` of it, so the radius rule cannot fail either.
        let position = Point::new(104.0, 10.0);
        let from_inside = Point::new(96.0, 10.0);
        let from_outside = Point::new(108.0, 10.0);
        assert!(rect.contains(from_inside), "the first press began inside");
        assert!(
            !rect.contains(from_outside) && !rect.contains(position),
            "the second began outside, and neither sample is in the rect",
        );
        for origin in [from_inside, from_outside] {
            assert!(
                super::super::distance(origin, position) < touch_profile.tap_slop,
                "the probe only discriminates while the travel is inside tap_slop",
            );
        }

        assert!(
            TapBoundary::Bounds.left(from_inside, position, Some(rect), touch_profile),
            "a press that began inside the node is bounded by the node: crossing \
             the edge ends it, with no radius grace outside",
        );
        assert!(
            !TapBoundary::Bounds.left(from_outside, position, Some(rect), touch_profile),
            "a press that began outside is bounded by its own radius, and this \
             one has barely moved",
        );
    }
}
