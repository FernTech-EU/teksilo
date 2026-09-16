// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The **reversible-mutation seam**: what the scene tells a data layer so that
//! layer can reverse an edit.
//!
//! # What this is not
//!
//! There is no stack here, no history, no command list and no `undo()`. Undo
//! belongs to the data layer, and the framework's job stops at handing that
//! layer a record complete enough to invert. Everything below is *mechanism*;
//! every policy question — what counts as one undo step, whether a cancelled
//! gesture clears the redo stack, how long history is kept — is answered on the
//! other side of the seam.
//!
//! "Complete enough to invert" is a claim with teeth, so it is worth stating
//! exactly: **every [`SceneEdit::Change`] in a record carries both sides of
//! what it replaced**, and a [`SceneEdit::Removed`] carries the removed entry
//! itself. Nothing else is in there. Two notifications the scene emits are
//! therefore deliberately *not* recorded, and
//! [`ItemChange::is_edit`](crate::ItemChange::is_edit) is the test that says
//! which:
//!
//! | not recorded | why |
//! | --- | --- |
//! | [`ItemChange::VisibilityChanged`] | a convenience beside the `FlagsChanged` that describes the same mutation — recording both would make hiding a card two edits, and only through one of the two flag doors |
//! | [`ItemChange::HandlersChanged`] with no `replaced` | `Scene::handlers_mut` hands out a `&mut` and is told nothing about what the caller does with it, so it can describe the change to no one. `Scene::set_item_handlers` knows both sides and is recorded. |
//!
//! Both still ride the notification channel, because a view or a cache does
//! need to hear about them. The distinction is between *telling* and
//! *describing*.
//!
//! # Two channels, because the types force it
//!
//! [`Scene::item_change_signal`](crate::Scene::item_change_signal) carries a
//! [`SceneChange`] — an [`ItemChange`] wrapped in an envelope that says *which
//! transaction* it belongs to, *whose* change it was, *whether it counts as
//! history*, and *whether it is a per-frame artefact*. That channel is cheap,
//! synchronous and `Clone`, and the view reconciles from it.
//!
//! It cannot carry ownership. `Signal<T>` snapshots its value before fanning
//! out, so `T: Clone`, and a removed entry holds a `Box<dyn SceneItem>` or a
//! `Box<dyn Widget>` — neither clonable. So a removal's *contents* travel a
//! second, owning channel: the [`SceneTransactionRecord`] delivered to the edit
//! sink. See [`crate::salvage`] for why that matters and what it carries.
//!
//! # A transaction is a scope, and the scope is the write scope
//!
//! Every `SceneModel` mutator already runs inside a write scope (that is how
//! the change fan-out escapes the `RefCell` borrow), and
//! [`SceneWriteGuard`](crate::SceneWriteGuard) opens one around a whole block.
//! A transaction is the same boundary: one scope, one [`TxnId`]. So a subtree
//! `remove` is one transaction with N edits without anything being wrapped by
//! hand, and a block of edits under a write guard is one transaction because it
//! is one scope.
//!
//! [`SceneTransaction`](crate::SceneTransaction) sits *above* that: it holds the scope open across
//! several mutator calls and stamps them with a chosen
//! [`ChangeSource`]/[`HistoryMode`]. Nesting **joins** — an inner transaction
//! adds no boundary and the outer stamp wins — so an observer that opens its
//! own transaction inside a framework-opened gesture cannot split that gesture
//! in two.
//!
//! # Where the sink runs, and why its own writes are journaled
//!
//! The sink is invoked with **no borrow on the scene**, which is what lets it
//! read the scene and write it back. A sink that writes is not a corner case —
//! validation, clamping and mirroring all do it — and if those writes were
//! invisible the record the app holds would say `old → new` while the scene sat
//! at something else, and a redo would replay the wrong value. So a sink write
//! opens its own transaction like any other, its record queues behind the one
//! being delivered, and the same delivery loop hands it over on a later round.
//! The sink is never re-entered while it is running; it is also never taken out
//! of its slot, so a panicking sink does not vanish.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use teksilo_core::signal::Signal;

use crate::item::ItemId;
use crate::salvage::RemovedItem;
use crate::scene::{CascadeBudget, ItemChange};

/// Process-unique id for one transaction.
///
/// Rides on every [`SceneChange`] emitted inside it, so a consumer watching the
/// cheap notification channel can group a fan-out that arrived as N separate
/// notifications — and match it to the owning [`SceneTransactionRecord`] when
/// one is delivered.
/// Ids count up from 1; `TxnId(0)` is the "no transaction yet" sentinel the
/// change signal is seeded with and no scene mutation can ever carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct TxnId(u64);

impl TxnId {
    /// Raw value, for a consumer keying its own map on a transaction.
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// Whether this is the sentinel the change and transaction signals hold
    /// before anything has been committed. Never `true` for a transaction the
    /// scene actually opened.
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// *Whose* change this is.
///
/// Ambient on the open transaction rather than a parameter on every mutator:
/// widening two dozen public signatures to thread it would be a worse API and
/// would still miss the framework's own gesture code, which is the one caller
/// an app cannot reach.
///
/// The framework opens [`ChangeSource::User`] transactions around exactly the
/// places it writes the model on the user's behalf — the item-drag commit, the
/// selection-transform commit and the `Alt`+arrow nudge. Without that, an app
/// could not tell a finished drag from a programmatic move: both arrive as a
/// bare `LocalPosChanged`.
///
/// `#[non_exhaustive]`: this crate has out-of-tree consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ChangeSource {
    /// A direct user interaction — a drag, a nudge, a transform handle.
    User,
    /// The app moved the scene itself: a document load, a layout pass, a data
    /// re-source, an import, an animation tick. The default, because a mutation
    /// nobody claimed is not a user's.
    #[default]
    Programmatic,
    /// Applied on behalf of a peer — collaboration, replay, a data-layer redo.
    /// A consumer re-broadcasting or recording history is expected to skip
    /// these; the framework does not interpret it.
    Remote,
}

/// What a history *above* the scene should do with this transaction.
///
/// Three-valued, matching what the editors that have solved this converged on
/// (tldraw's `history: 'record' | 'record-preserveRedoStack' | 'ignore'`). The
/// framework never interprets it. It carries it faithfully and nothing else —
/// which is the whole point: deciding what counts as an undo step is the data
/// layer's job, and the scene's job is to make the decision expressible.
///
/// `#[non_exhaustive]`: this crate has out-of-tree consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum HistoryMode {
    /// A document edit: push an entry, clear redo. The default.
    #[default]
    Record,
    /// Push an entry but leave the redo stack alone. For a change that is
    /// undoable without invalidating a redo branch.
    RecordPreserveRedo,
    /// Do not push. Views still reconcile; history does not move.
    Ignore,
}

/// How a transaction finished.
///
/// `#[non_exhaustive]`: this crate has out-of-tree consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum TxnOutcome {
    /// The normal ending: the guard dropped, or the write scope closed.
    #[default]
    Committed,
    /// [`SceneTransaction::abandon`](crate::SceneTransaction::abandon) — a cancelled interaction.
    ///
    /// The scene is **not** rolled back. The framework owns no
    /// inverse-application routine and will not grow one, because applying an
    /// inverse is the first 80 % of an undo stack. The record is delivered so
    /// the consumer can revert from the `old` values it was handed, without
    /// pushing anything a redo could replay.
    Abandoned,
}

/// One [`ItemChange`] plus the context the notification channel needs in order
/// to be usable as a change feed rather than just a repaint trigger.
///
/// This is what
/// [`Scene::item_change_signal`](crate::Scene::item_change_signal) carries.
///
/// `#[non_exhaustive]`: consumers read it, the scene constructs it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SceneChange {
    /// The transaction this change belongs to. Every change emitted inside one
    /// write scope — or one [`SceneTransaction`](crate::SceneTransaction) — shares it.
    pub txn: TxnId,
    /// Whose change it is. See [`ChangeSource`].
    pub source: ChangeSource,
    /// What a history above the scene should do with it. See [`HistoryMode`].
    pub history: HistoryMode,
    /// A change that must be **rendered** but must not be **recorded**.
    ///
    /// Set for per-frame model churn that is an artefact of animation rather
    /// than an edit: the scene's own
    /// [`refresh_dynamic_bounds`](crate::Scene::refresh_dynamic_bounds), which
    /// re-reads every signal-driven AABB each build and so emits a
    /// `LocalBoundsChanged` per frame for an animating item. An app driving its
    /// own per-frame stream — an item whose `transform` follows a rotation
    /// signal — marks it the same way with
    /// [`SceneTransaction::ephemeral`](crate::SceneTransaction::ephemeral), so the knob is not one-sided across the
    /// seam.
    pub ephemeral: bool,
    /// The change itself.
    pub change: ItemChange,
}

impl SceneChange {
    /// The item this change is about.
    pub fn id(&self) -> ItemId {
        self.change.id()
    }

    /// A default envelope around `change`, for a test that drives
    /// `item_change_signal` directly rather than through a mutator.
    ///
    /// Test-only: a `SceneChange` a consumer receives is always stamped by the
    /// transaction it came from, and a public constructor would let one be
    /// forged with a transaction id no transaction ever had.
    #[cfg(test)]
    pub(crate) fn for_test(change: ItemChange) -> Self {
        Self {
            txn: TxnId::default(),
            source: ChangeSource::default(),
            history: HistoryMode::default(),
            ephemeral: false,
            change,
        }
    }
}

/// The ownership an [`ItemChange`] cannot carry, attached to the removal that
/// produced it.
///
/// `#[non_exhaustive]`: this crate has out-of-tree consumers.
#[derive(Debug)]
#[non_exhaustive]
pub enum Salvage {
    /// The framework owns the removed entry and is handing it over. Feed it
    /// back to [`Scene::restore`](crate::Scene::restore) to undo the removal.
    Owned(Box<RemovedItem>),
    /// The caller used [`Scene::take`](crate::Scene::take) and already holds
    /// the salvage, so the record carries only the fact of the removal.
    TakenByCaller,
}

/// One edit inside a [`SceneTransactionRecord`], in application order.
///
/// Invert a transaction by walking `edits` in **reverse** and applying each
/// `old`; a `Removed` inverts to
/// [`Scene::restore`](crate::Scene::restore).
///
/// Only a removal needs an owning variant. Every other mutator either carries
/// its own `old` in the [`ItemChange`] or hands the replaced value straight
/// back to its caller —
/// [`Scene::replace_item`](crate::Scene::replace_item) returns the box it
/// swapped out, so the app that called it already owns what it would need to
/// put back.
///
/// A [`Change`](Self::Change) here always satisfies
/// [`ItemChange::is_edit`](crate::ItemChange::is_edit): the scene's derived
/// notifications never reach a record. See the module header for the two and
/// why.
///
/// `#[non_exhaustive]`: this crate has out-of-tree consumers.
#[derive(Debug)]
#[non_exhaustive]
pub enum SceneEdit {
    /// Any non-removal change, verbatim from the notification channel.
    Change(ItemChange),
    /// A removal, with what it destroyed. Descendants come before the named
    /// root, the order `remove` announces in.
    Removed {
        /// The removed item.
        id: ItemId,
        /// Its contents, or a note that the caller kept them.
        salvage: Salvage,
    },
}

/// One committed transaction, delivered to the edit sink with the scene
/// unborrowed.
///
/// Owning — it carries the boxes a removal would otherwise drop — and therefore
/// cannot travel through a `Signal`. That is the whole reason the seam has two
/// channels; see the module docs.
///
/// `#[non_exhaustive]`: this crate has out-of-tree consumers.
#[derive(Debug)]
#[non_exhaustive]
pub struct SceneTransactionRecord {
    /// Matches the `txn` on every [`SceneChange`] this transaction emitted.
    pub txn: TxnId,
    /// Whose change it was.
    pub source: ChangeSource,
    /// What a history above the scene should do with it.
    pub history: HistoryMode,
    /// How it finished.
    pub outcome: TxnOutcome,
    /// `true` iff **every** edit in it was emitted while
    /// [`SceneTransaction::ephemeral`](crate::SceneTransaction::ephemeral) (or the scene's own dynamic-bounds
    /// refresh) was in effect. Never `true` for an empty record.
    pub ephemeral: bool,
    /// The edits, in application order.
    pub edits: Vec<SceneEdit>,
}

impl SceneTransactionRecord {
    /// Whether this transaction changed nothing. An empty record is never
    /// delivered to the sink — a mutator that early-returned on an unchanged
    /// value has nothing to reverse.
    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }
}

/// The one transaction currently accumulating.
struct OpenTxn {
    id: TxnId,
    source: ChangeSource,
    history: HistoryMode,
    outcome: TxnOutcome,
    squash: bool,
    /// AND of `ephemeral` over every edit recorded so far; meaningless (and
    /// forced to `false`) when no edit was recorded.
    all_ephemeral: bool,
    edits: Vec<SceneEdit>,
}

/// RAII clear of [`EditJournal::delivering`], so a sink that panics cannot
/// leave the journal permanently silent. Holds the `Rc` directly, so `Drop`
/// never needs the scene's `RefCell`.
struct DeliverGuard(Rc<EditJournal>);

impl Drop for DeliverGuard {
    fn drop(&mut self) {
        self.0.delivering.set(false);
    }
}

/// The scene's transaction state machine, its edit sink and the queue of
/// records awaiting delivery.
///
/// Lives behind an `Rc` **beside** the `Scene` (like the change queue), so a
/// delivery runs holding no borrow on the scene at all — which is what lets the
/// sink read and write it.
pub(crate) struct EditJournal {
    next_id: Cell<u64>,
    /// Open write scopes / transactions. The transaction commits when this
    /// reaches zero.
    depth: Cell<u32>,
    /// Emissions made while this is non-zero are tagged `ephemeral`.
    ephemeral_depth: Cell<u32>,
    open: RefCell<Option<OpenTxn>>,
    sink: RefCell<Option<Box<dyn FnMut(SceneTransactionRecord)>>>,
    records: RefCell<VecDeque<SceneTransactionRecord>>,
    delivering: Cell<bool>,
    txn_signal: Signal<TxnId>,
}

impl std::fmt::Debug for EditJournal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditJournal")
            .field("depth", &self.depth.get())
            .field("queued_records", &self.records.borrow().len())
            .field(
                "has_sink",
                &self.sink.try_borrow().map(|s| s.is_some()).ok(),
            )
            .field("delivering", &self.delivering.get())
            .finish()
    }
}

impl EditJournal {
    pub(crate) fn new() -> Self {
        Self {
            next_id: Cell::new(1),
            depth: Cell::new(0),
            ephemeral_depth: Cell::new(0),
            open: RefCell::new(None),
            sink: RefCell::new(None),
            records: RefCell::new(VecDeque::new()),
            delivering: Cell::new(false),
            txn_signal: Signal::new(TxnId(0)),
        }
    }

    pub(crate) fn txn_signal(&self) -> Signal<TxnId> {
        self.txn_signal.clone()
    }

    /// Whether a sink is installed — i.e. whether anything would read the
    /// owning record. With none, removals drop their salvage exactly as they
    /// always did, and no edit vector is ever built.
    pub(crate) fn recording(&self) -> bool {
        self.sink.try_borrow().map(|s| s.is_some()).unwrap_or(true)
    }

    /// Install the edit sink, returning the previous one.
    pub(crate) fn set_sink(
        &self,
        sink: Option<Box<dyn FnMut(SceneTransactionRecord)>>,
    ) -> Option<Box<dyn FnMut(SceneTransactionRecord)>> {
        let mut slot = self.sink.try_borrow_mut().expect(
            "SceneModel::set_edit_sink from inside the edit sink itself. The sink is \
             held for the duration of its call; replace it from outside one.",
        );
        std::mem::replace(&mut *slot, sink)
    }

    /// Open a scope. The first one starts a transaction stamped
    /// `source`/`history`; deeper ones join it and their stamp is ignored.
    pub(crate) fn enter_scope(&self, source: ChangeSource, history: HistoryMode) {
        let depth = self.depth.get();
        self.depth.set(depth + 1);
        if depth == 0 {
            let id = TxnId(self.next_id.get());
            self.next_id.set(self.next_id.get().wrapping_add(1));
            *self.open.borrow_mut() = Some(OpenTxn {
                id,
                source,
                history,
                outcome: TxnOutcome::Committed,
                squash: false,
                all_ephemeral: true,
                edits: Vec::new(),
            });
        }
    }

    /// Close a scope. At depth zero the transaction commits: its record is
    /// built and queued, and the caller delivers it once the scene's borrow is
    /// free.
    pub(crate) fn exit_scope(&self) {
        let depth = self.depth.get();
        debug_assert!(depth > 0, "EditJournal scope underflow");
        let depth = depth.saturating_sub(1);
        self.depth.set(depth);
        if depth > 0 {
            return;
        }
        let Some(mut txn) = self.open.borrow_mut().take() else {
            return;
        };
        if txn.edits.is_empty() {
            // Nothing happened — a mutator that early-returned on an unchanged
            // value. Delivering an empty record would make a snap-to-grid sink
            // that writes only when it has something to correct loop forever
            // against its own no-op.
            return;
        }
        if txn.squash {
            squash_edits(&mut txn.edits);
        }
        self.records.borrow_mut().push_back(SceneTransactionRecord {
            txn: txn.id,
            source: txn.source,
            history: txn.history,
            outcome: txn.outcome,
            ephemeral: txn.all_ephemeral,
            edits: txn.edits,
        });
    }

    /// The stamp for a change emitted right now. Mints a standalone
    /// transaction when nothing is open, which is the bare-`&mut Scene` door
    /// (no write scope, so no scope to be the boundary).
    pub(crate) fn stamp(&self) -> (TxnId, ChangeSource, HistoryMode, bool) {
        let ephemeral = self.ephemeral_depth.get() > 0;
        if let Some(open) = self.open.borrow().as_ref() {
            return (open.id, open.source, open.history, ephemeral);
        }
        let id = TxnId(self.next_id.get());
        self.next_id.set(self.next_id.get().wrapping_add(1));
        (
            id,
            ChangeSource::default(),
            HistoryMode::default(),
            ephemeral,
        )
    }

    /// Record one edit against the open transaction. Silently does nothing
    /// when no sink is installed (nothing would read it) or when no
    /// transaction is open (the bare-`&mut Scene` door with no scope: the
    /// change is still announced, it is just not accumulated).
    pub(crate) fn record(&self, edit: SceneEdit) {
        if !self.recording() {
            return;
        }
        if let Some(open) = self.open.borrow_mut().as_mut() {
            open.all_ephemeral &= self.ephemeral_depth.get() > 0;
            open.edits.push(edit);
        }
    }

    /// Mark the open transaction cancelled. No-op with none open.
    pub(crate) fn set_outcome(&self, outcome: TxnOutcome) {
        if let Some(open) = self.open.borrow_mut().as_mut() {
            open.outcome = outcome;
        }
    }

    /// Ask the open transaction to coalesce adjacent same-subject continuous
    /// edits when it commits.
    pub(crate) fn request_squash(&self) {
        if let Some(open) = self.open.borrow_mut().as_mut() {
            open.squash = true;
        }
    }

    pub(crate) fn enter_ephemeral(&self) {
        self.ephemeral_depth.set(self.ephemeral_depth.get() + 1);
    }

    pub(crate) fn exit_ephemeral(&self) {
        self.ephemeral_depth
            .set(self.ephemeral_depth.get().saturating_sub(1));
    }

    /// How many scopes are open — a write scope, a `SceneWriteGuard`, or a
    /// [`SceneTransaction`](crate::SceneTransaction) each raise it by one.
    pub(crate) fn depth(&self) -> u32 {
        self.depth.get()
    }

    /// Whether any record is waiting to be handed to the sink.
    pub(crate) fn has_records(&self) -> bool {
        !self.records.borrow().is_empty()
    }

    /// Hand every queued record to the sink, in commit order, until the queue
    /// is empty.
    ///
    /// Holds no borrow on the scene, so the sink may read it and write it back.
    /// A sink write commits its own transaction, which queues behind whatever
    /// is left here and is delivered by this same loop — so the sink's own
    /// edits are journaled rather than silently escaping the history it is
    /// feeding.
    ///
    /// Re-entered from inside the sink (a sink write ends up here again), it
    /// returns immediately and lets the outermost loop pick the new record up.
    ///
    /// # Termination
    ///
    /// The first round is the caller's own batch, finite by construction and
    /// free. From the second on, every delivery is one the *sink* caused, and
    /// is counted against `budget.total` — the same flat bound the change
    /// fan-out uses, for the same reason: an iterative loop with no bound is a
    /// frozen UI thread with no diagnostic. An empty record is never queued, so
    /// a sink that only writes when it has a correction to make settles on its
    /// own.
    pub(crate) fn deliver(self: &Rc<Self>, budget: CascadeBudget) {
        if self.delivering.get() {
            return;
        }
        if self.records.borrow().is_empty() {
            return;
        }
        self.delivering.set(true);
        let _guard = DeliverGuard(Rc::clone(self));

        let mut charged: u64 = 0;
        let mut cascading = false;
        loop {
            let mut remaining = self.records.borrow().len();
            if remaining == 0 {
                break;
            }
            while remaining > 0 {
                // Popped before delivery and never buffered locally: a sink
                // that panics must cost exactly its own record, leaving the
                // rest of the queue intact for the next flush.
                let next = self.records.borrow_mut().pop_front();
                let Some(record) = next else { break };
                remaining -= 1;
                if cascading {
                    charged += 1;
                    if charged > budget.total {
                        runaway_sink(charged, budget.total, &record);
                    }
                }
                let txn = record.txn;
                // Borrowed, not taken: take-call-put-back loses the sink on a
                // panic, and — worse — runs the sink's own writes with no sink
                // installed, so a sink that corrects a value would leave the
                // record the app holds disagreeing with the scene. Borrowing
                // keeps it installed; `delivering` is what stops re-entry.
                if let Ok(mut slot) = self.sink.try_borrow_mut()
                    && let Some(sink) = slot.as_mut()
                {
                    sink(record);
                }
                // After the sink, still unborrowed. This is the door a
                // per-gesture reaction (persistence, validation, a document
                // dirty flag) belongs on — once per transaction, not once per
                // intermediate write.
                self.txn_signal.set(txn);
            }
            cascading = true;
        }
    }
}

/// RAII raise of the journal's ephemeral depth.
///
/// Used by the scene's own per-frame dynamic-bounds refresh and by
/// [`SceneTransaction::ephemeral`](crate::SceneTransaction::ephemeral). A guard rather than a pair of calls so an
/// early return — `refresh_dynamic_bounds` has several — cannot leave the whole
/// scene permanently tagged "do not record".
pub(crate) struct EphemeralScope(Rc<EditJournal>);

impl EphemeralScope {
    pub(crate) fn new(journal: &Rc<EditJournal>) -> Self {
        journal.enter_ephemeral();
        Self(Rc::clone(journal))
    }
}

impl Drop for EphemeralScope {
    fn drop(&mut self) {
        self.0.exit_ephemeral();
    }
}

/// RAII transaction scope for a mutator that emits many changes from one call.
///
/// [`Scene::take`](crate::Scene::take) is the only one, and it needs the guard
/// rather than a matching pair of calls because it has several exits. Stamped
/// with the defaults, so through a `SceneModel` — where a write scope is always
/// already open — it nests and the outer stamp wins.
pub(crate) struct RemovalScope(Rc<EditJournal>);

impl RemovalScope {
    pub(crate) fn new(journal: &Rc<EditJournal>) -> Self {
        journal.enter_scope(ChangeSource::default(), HistoryMode::default());
        Self(Rc::clone(journal))
    }
}

impl Drop for RemovalScope {
    fn drop(&mut self) {
        self.0.exit_scope();
    }
}

/// Panic for an edit sink that keeps producing transactions.
#[cold]
#[inline(never)]
fn runaway_sink(delivered: u64, limit: u64, record: &SceneTransactionRecord) -> ! {
    panic!(
        "scene edit sink did not settle: the sink installed by \
         `SceneModel::set_edit_sink` has produced {delivered} further transactions \
         from one delivery without the queue emptying, past this scene's budget of \
         {limit} (`CascadeBudget::total`). The last record was {record:?}. A sink is \
         allowed to write the scene — that is what it is for — but its write must \
         converge: write only when there is something to correct, and compare before \
         you write. A transaction that changes nothing produces no record, so a \
         converging sink stops on its own. If the cascade is genuinely this large and \
         does terminate, raise `CascadeBudget::total` via \
         `SceneModel::set_cascade_budget`."
    )
}

/// Which continuous-value edits coalesce, keyed so two different fields of one
/// item never collapse into each other.
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum SquashKey {
    LocalPos(ItemId),
    LocalBounds(ItemId),
    Transform(ItemId),
    Opacity(ItemId),
    Z(ItemId),
    Placement(ItemId),
}

fn squash_key(change: &ItemChange) -> Option<SquashKey> {
    Some(match *change {
        ItemChange::LocalPosChanged { id, .. } => SquashKey::LocalPos(id),
        ItemChange::LocalBoundsChanged { id, .. } => SquashKey::LocalBounds(id),
        ItemChange::TransformChanged { id, .. } => SquashKey::Transform(id),
        ItemChange::OpacityChanged { id, .. } => SquashKey::Opacity(id),
        ItemChange::ZChanged { id, .. } => SquashKey::Z(id),
        ItemChange::PlacementChanged { id, .. } => SquashKey::Placement(id),
        _ => return None,
    })
}

/// Fold a transaction's repeated writes to one continuous quantity into a
/// single edit keeping the **first** `old` and the **last** `new`.
///
/// Only the six continuous quantities above, and only when the transaction
/// asked for it ([`SceneTransaction::squash`](crate::SceneTransaction::squash)) — a coalesced record has lost
/// the intermediate path, which a replay or presence consumer needs, and losing
/// it silently by default would be exactly the kind of quiet data loss this
/// seam exists to stop. Every other edit kind passes through untouched, in
/// order.
fn squash_edits(edits: &mut Vec<SceneEdit>) {
    use std::collections::HashMap;
    let mut first_seen: HashMap<SquashKey, usize> = HashMap::new();
    let mut out: Vec<SceneEdit> = Vec::with_capacity(edits.len());
    for edit in edits.drain(..) {
        let SceneEdit::Change(change) = &edit else {
            out.push(edit);
            continue;
        };
        let Some(key) = squash_key(change) else {
            out.push(edit);
            continue;
        };
        if let Some(&at) = first_seen.get(&key) {
            let SceneEdit::Change(kept) = &mut out[at] else {
                unreachable!("only Change edits are indexed by squash_key")
            };
            let SceneEdit::Change(newer) = edit else {
                unreachable!("matched as a Change above")
            };
            merge_new(kept, newer);
        } else {
            first_seen.insert(key, out.len());
            out.push(edit);
        }
    }
    *edits = out;
}

/// Overwrite `kept`'s `new` with `newer`'s, keeping `kept`'s `old`. Both are
/// the same variant about the same item, guaranteed by the [`SquashKey`] match.
fn merge_new(kept: &mut ItemChange, newer: ItemChange) {
    match (kept, newer) {
        (
            ItemChange::LocalPosChanged { new: keep, .. },
            ItemChange::LocalPosChanged { new: last, .. },
        ) => *keep = last,
        (
            ItemChange::LocalBoundsChanged { new: keep, .. },
            ItemChange::LocalBoundsChanged { new: last, .. },
        ) => *keep = last,
        (
            ItemChange::TransformChanged { new: keep, .. },
            ItemChange::TransformChanged { new: last, .. },
        ) => *keep = last,
        (
            ItemChange::OpacityChanged { new: keep, .. },
            ItemChange::OpacityChanged { new: last, .. },
        ) => *keep = last,
        (ItemChange::ZChanged { new: keep, .. }, ItemChange::ZChanged { new: last, .. }) => {
            *keep = last
        }
        (
            ItemChange::PlacementChanged { new: keep, .. },
            ItemChange::PlacementChanged { new: last, .. },
        ) => *keep = last,
        _ => unreachable!("squash_key pairs identical variants about one item"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Placement;
    use teksilo_canvas::{Point, Transform2D};

    fn moved(id: ItemId, from: f32, to: f32) -> SceneEdit {
        SceneEdit::Change(ItemChange::LocalPosChanged {
            id,
            old: Point::new(from, 0.0),
            new: Point::new(to, 0.0),
        })
    }

    fn id(n: u64) -> ItemId {
        // `ItemId::next` is crate-private and monotonic; minting two here keeps
        // the test independent of how many ids other tests have consumed.
        let _ = n;
        ItemId::next()
    }

    #[test]
    fn squash_keeps_the_first_old_and_the_last_new() {
        let a = id(1);
        let mut edits = vec![moved(a, 0.0, 1.0), moved(a, 1.0, 2.0), moved(a, 2.0, 7.0)];
        squash_edits(&mut edits);
        assert_eq!(edits.len(), 1);
        let SceneEdit::Change(ItemChange::LocalPosChanged { old, new, .. }) = &edits[0] else {
            panic!("expected one LocalPosChanged")
        };
        assert_eq!(*old, Point::new(0.0, 0.0));
        assert_eq!(*new, Point::new(7.0, 0.0));
    }

    #[test]
    fn squash_never_merges_two_items_or_two_fields() {
        let a = id(1);
        let b = id(2);
        let mut edits = vec![
            moved(a, 0.0, 1.0),
            moved(b, 0.0, 5.0),
            SceneEdit::Change(ItemChange::ZChanged {
                id: a,
                old: 0.0,
                new: 1.0,
            }),
            moved(a, 1.0, 3.0),
        ];
        squash_edits(&mut edits);
        // a's two moves fold; b's move and a's z stay distinct.
        assert_eq!(edits.len(), 3);
    }

    #[test]
    fn squash_leaves_discrete_edits_alone_and_in_order() {
        let a = id(1);
        let mut edits = vec![
            SceneEdit::Change(ItemChange::Added { id: a }),
            moved(a, 0.0, 1.0),
            SceneEdit::Removed {
                id: a,
                salvage: Salvage::TakenByCaller,
            },
            moved(a, 1.0, 2.0),
        ];
        squash_edits(&mut edits);
        // The two moves fold into the first; Added and Removed keep their slots
        // in order, because collapsing structure would reorder the history.
        assert_eq!(edits.len(), 3);
        assert!(matches!(
            edits[0],
            SceneEdit::Change(ItemChange::Added { .. })
        ));
        assert!(matches!(
            edits[1],
            SceneEdit::Change(ItemChange::LocalPosChanged { .. })
        ));
        assert!(matches!(edits[2], SceneEdit::Removed { .. }));
    }

    #[test]
    fn squash_folds_placement_writes() {
        let a = id(1);
        let p = |x: f32| Placement {
            parent: None,
            z: 0.0,
            local_pos: Point::new(x, 0.0),
            transform: Transform2D::identity(),
        };
        let mut edits = vec![
            SceneEdit::Change(ItemChange::PlacementChanged {
                id: a,
                old: p(0.0),
                new: p(1.0),
            }),
            SceneEdit::Change(ItemChange::PlacementChanged {
                id: a,
                old: p(1.0),
                new: p(9.0),
            }),
        ];
        squash_edits(&mut edits);
        assert_eq!(edits.len(), 1);
        let SceneEdit::Change(ItemChange::PlacementChanged { old, new, .. }) = &edits[0] else {
            panic!("expected one PlacementChanged")
        };
        assert_eq!(old.local_pos.x, 0.0);
        assert_eq!(new.local_pos.x, 9.0);
    }
}
