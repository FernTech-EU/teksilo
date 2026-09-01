// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Unified reactivity primitives for Teksilo.
//!
//! `Signal<T>` is the single reactive type. `Prop<T>` is the widget
//! property type for static values and signal bindings. `ObserverHandle`
//! is an RAII guard — dropping it removes the observer callback.

use std::cell::{Cell, Ref, RefCell};
use std::rc::{Rc, Weak};

use crate::binding::{Binding, BindingLevel, BindingRegistry};
use crate::widget_id::WidgetId;

// ---------------------------------------------------------------------------
// Feedback-loop guard (debug-only)
// ---------------------------------------------------------------------------
//
// The snapshot-and-release notification model lets an observer freely
// re-enter `set` on the same (or another) signal. That flexibility also means
// an *accidental* feedback loop — signal A's observer writes B, B's observer
// writes A, neither guarded by an equality check — recurses without bound and
// blows the stack with no actionable diagnostic. In debug builds we track the
// per-thread notification depth and panic with a pointer at the likely cause
// once it crosses a limit set far above any legitimate synchronous cascade.
// Release builds carry no counter and no check.
#[cfg(debug_assertions)]
const SIGNAL_NOTIFY_DEPTH_LIMIT: u32 = 256;

#[cfg(debug_assertions)]
thread_local! {
    static SIGNAL_NOTIFY_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// RAII increment of the per-thread Signal-notification depth (debug only).
#[cfg(debug_assertions)]
struct NotifyDepthGuard;

#[cfg(debug_assertions)]
impl NotifyDepthGuard {
    fn enter() -> Self {
        SIGNAL_NOTIFY_DEPTH.with(|d| {
            let next = d.get() + 1;
            assert!(
                next <= SIGNAL_NOTIFY_DEPTH_LIMIT,
                "Signal notification nested {next} deep (limit {SIGNAL_NOTIFY_DEPTH_LIMIT}) — \
                 almost certainly an unbounded feedback loop between observers (e.g. signal A's \
                 observer sets B and B's observer sets A). Break the cycle: guard the write with \
                 an equality check (`if sig.get() != v {{ sig.set(v) }}`), or drop one edge with \
                 a WeakSignal."
            );
            d.set(next);
        });
        NotifyDepthGuard
    }
}

#[cfg(debug_assertions)]
impl Drop for NotifyDepthGuard {
    fn drop(&mut self) {
        SIGNAL_NOTIFY_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

// ---------------------------------------------------------------------------
// ObserverHandle — RAII guard for observer cleanup
// ---------------------------------------------------------------------------

/// RAII guard for an observer callback. Dropping the handle removes the
/// callback from the signal, preventing memory leaks.
pub struct ObserverHandle {
    /// Reference to the signal (keeps it alive while the handle exists).
    _signal: Rc<dyn std::any::Any>,
    observer_id: u64,
    remover: Rc<dyn Fn(u64)>,
}

impl ObserverHandle {
    /// Create a new observer handle.
    ///
    /// - `keeper`: an `Rc` that keeps the observed source alive while this handle exists.
    /// - `observer_id`: the ID identifying this observer.
    /// - `remover`: called with `observer_id` when the handle is dropped, to unregister the callback.
    pub fn new(keeper: Rc<dyn std::any::Any>, observer_id: u64, remover: Rc<dyn Fn(u64)>) -> Self {
        Self {
            _signal: keeper,
            observer_id,
            remover,
        }
    }

    /// Explicitly detach the observer without dropping the handle.
    pub fn detach(self) {
        // Drop runs automatically, which calls remover
        drop(self);
    }
}

impl Drop for ObserverHandle {
    fn drop(&mut self) {
        (self.remover)(self.observer_id);
    }
}

impl std::fmt::Debug for ObserverHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObserverHandle")
            .field("observer_id", &self.observer_id)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Signal internals
// ---------------------------------------------------------------------------

struct ObserverEntry<T> {
    id: u64,
    callback: Rc<dyn Fn(&T)>,
}

/// Register `callback` on a mutable root and hand back the RAII guard that
/// unregisters it.
///
/// Shared by [`Signal::try_observe`] and by the `subscribe` hook a mutable
/// root publishes on its [`DerivedSource`], so a derived signal's observer
/// attaches by exactly the same mechanism as a direct one — there is no
/// second registration path that could drift from this one.
fn observe_inner<T: 'static>(
    inner: &Rc<RefCell<MutableInner<T>>>,
    callback: Rc<dyn Fn(&T)>,
) -> ObserverHandle {
    let id = {
        let mut guard = inner.borrow_mut();
        let id = guard.next_observer_id;
        guard.next_observer_id += 1;
        guard.observers.push(ObserverEntry { id, callback });
        id
    };
    ObserverHandle {
        _signal: inner.clone(),
        observer_id: id,
        remover: {
            let inner = inner.clone();
            Rc::new(move |observer_id| {
                inner.borrow_mut().observers.retain(|e| e.id != observer_id);
            })
        },
    }
}

struct MutableInner<T> {
    value: T,
    /// Monotonic change counter, advanced by every write (`try_set`, and
    /// arming an animation). **Never reset.**
    ///
    /// This replaced a plain `dirty: bool` because a boolean is
    /// *consumer-shaped state stored on the producer*: one flag, but
    /// potentially many independent consumers. Each open window owns its
    /// own [`crate::binding::BindingRegistry`], and that registry's flush
    /// pass both read AND cleared the flag — so with two windows bound to
    /// one signal, whichever tree reconciled first consumed the flag and
    /// every other window silently skipped its otherwise-correct binding.
    /// Not a delayed rebuild: a permanently missed one, until an unrelated
    /// later write raced a different window into observing it first (which
    /// window lost was decided by `HashMap` iteration order).
    ///
    /// A counter moves the per-consumer half of that state to the
    /// consumer: nothing here is consumed, and each registry remembers the
    /// generation it last acted on (`BindingGroup::last_seen`). N readers
    /// are then trivially independent.
    generation: u64,
    observers: Vec<ObserverEntry<T>>,
    next_observer_id: u64,
    /// Drop guards attached to this signal via `attach_keepalive`. Used
    /// by adapters (e.g., `LocalizedString::to_signal`) that observe an
    /// external source and need their `ObserverHandle` to live exactly
    /// as long as the signal it updates — when the last `Signal<T>`
    /// clone drops, this `Vec` drops, which drops every stored handle,
    /// which detaches their callbacks from the source. Without this,
    /// such adapters would have to `mem::forget` their handles and
    /// leak both the observer entry on the source and the target
    /// signal it kept alive through a strong `Rc` clone.
    keepalive: Vec<Box<dyn std::any::Any>>,
}

/// Animation-specific state, only for `Signal<f32>`.
struct AnimationState {
    pending: Option<crate::animation::AnimationRequest>,
    target: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SignalAccessError {
    #[error("signal is read-only")]
    ReadOnly,
    #[error("signal does not support animation")]
    AnimationUnsupported,
}

/// Weak reference to a mutable signal. Produced by `Signal::downgrade`.
///
/// Unlike a `Signal<T>` clone (which is an `Rc`), a `WeakSignal<T>`
/// does not extend the lifetime of the underlying `MutableInner<T>`.
/// Use this inside observer callbacks that should not keep the
/// observed-target signal alive — otherwise the strong `Rc` captured
/// by the closure forms a reference cycle with the inner that holds
/// the observer, and neither gets freed.
pub struct WeakSignal<T> {
    inner: Weak<RefCell<MutableInner<T>>>,
    animation: Option<Weak<RefCell<AnimationState>>>,
}

impl<T> Clone for WeakSignal<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            animation: self.animation.clone(),
        }
    }
}

impl<T: 'static> WeakSignal<T> {
    /// Try to upgrade the weak reference into a live `Signal<T>`.
    /// Returns `None` if the target signal has already been dropped.
    pub fn upgrade(&self) -> Option<Signal<T>> {
        let inner = self.inner.upgrade()?;
        // If the original was animated, preserve that — but if the
        // animation state was freed independently we degrade to a
        // non-animated signal rather than failing the upgrade.
        let animation = self.animation.as_ref().and_then(|weak| weak.upgrade());
        Some(Signal {
            kind: SignalKind::Mutable { inner, animation },
        })
    }
}

pub(crate) struct WeakAnimatedSignal {
    inner: Weak<RefCell<MutableInner<f32>>>,
    animation: Weak<RefCell<AnimationState>>,
}

impl WeakAnimatedSignal {
    pub(crate) fn upgrade(&self) -> Option<Signal<f32>> {
        Some(Signal {
            kind: SignalKind::Mutable {
                inner: self.inner.upgrade()?,
                animation: Some(self.animation.upgrade()?),
            },
        })
    }

    pub(crate) fn same_signal(&self, signal: &Signal<f32>) -> bool {
        match &signal.kind {
            SignalKind::Mutable { inner, .. } => self.inner.as_ptr() == Rc::as_ptr(inner),
            SignalKind::Derived { .. } => false,
        }
    }
}

/// One upstream source a [`SignalKind::Derived`] signal depends on.
///
/// A single-source derived signal (the typical `map` case) carries one
/// entry; multi-source derived signals (`zip`, `zip3`, `and`, `or`)
/// carry one per observed mutable root. Change-tracking walks the whole
/// vec — a consumer is stale if *any* entry's generation moved past what
/// that consumer last acted on.
///
/// **Every `generation` closure in the system is monotonically
/// non-decreasing**, whether it reads a mutable root's counter directly
/// or is a composite built by [`coalesced_source`]. Callers rely on that:
/// it is what lets [`Signal::generation`] fold a multi-source derived
/// signal down to a single `u64` by summing, with no risk that one
/// source's advance is cancelled by another's retreat.
///
/// Alongside the poll-based `generation`, a source may publish a
/// **push**-based `subscribe`. Generation polling serves the frame loop,
/// which asks every binding once a frame anyway; an observer has no frame
/// loop behind it and has to be told. See [`Signal::try_observe`].
#[derive(Clone)]
struct DerivedSource {
    /// Current generation of this upstream. Compare against a remembered
    /// value to decide staleness; never "clear" it.
    generation: Rc<dyn Fn() -> u64>,
    /// Stable identity of the upstream mutable root — used by
    /// [`BindingRegistry`] to dedup repeated `bind_to` calls.
    source_id: usize,
    /// Register a nullary change callback on this upstream, returning the
    /// guard that unregisters it.
    ///
    /// `None` where the set of roots is chosen *dynamically* and so cannot
    /// be subscribed to once and for all — [`Signal::flat_map`], whose
    /// inner signal is re-selected on every poll. A derived signal with any
    /// such source stays unobservable, and [`Signal::try_observe`] says so
    /// with [`SignalAccessError::ReadOnly`] rather than attaching an
    /// observer that would go quiet the moment the inner switched.
    subscribe: Option<SubscribeHook>,
}

/// A [`DerivedSource`]'s push-side registration: takes the callback to run
/// on every change of the upstream, returns the guard that detaches it.
///
/// Type-erased over the upstream's own `T` — a derived signal recomputes
/// its own value rather than reading the root's, so the notification only
/// has to say *that* something moved.
type SubscribeHook = Rc<dyn Fn(Rc<dyn Fn()>) -> ObserverHandle>;

/// Fold an arbitrary — and possibly *changing* — set of upstream
/// generations into ONE monotone counter, presented as a single
/// [`DerivedSource`].
///
/// `inputs` is polled on demand and returns the current generation of
/// every upstream the composite currently depends on. Whenever that
/// vector differs from the one seen at the previous poll (in length or
/// in any element), the composite's own counter advances by one.
///
/// Summing the inputs would be enough for a *fixed* set of monotone
/// upstreams, but not for [`Signal::flat_map`], whose selected inner
/// signal is re-chosen on every poll: switching inners makes its
/// generation term jump arbitrarily, including downwards, and a drop
/// that exactly cancelled an increase elsewhere would hide a real
/// change. Re-deriving an own counter from "did the input vector change
/// at all" is immune to that, and keeps this source monotone for
/// everyone downstream.
///
/// Crucially the memo is **never consumed**: the first registry to poll
/// advances the counter, and every registry polling afterwards reads the
/// same, already-advanced value and compares it against its OWN
/// last-seen. That is what makes one composite safe to share between N
/// independently-reconciled `WidgetTree`s — the property the whole
/// generation scheme exists to provide.
fn coalesced_source(
    inputs: Rc<dyn Fn() -> Vec<u64>>,
    subscribe: Option<SubscribeHook>,
) -> DerivedSource {
    // The token anchors a unique heap address used as `source_id`; the
    // closure below owns it, so the address stays valid — and therefore
    // unambiguous — for exactly as long as this source is reachable.
    let token: Rc<()> = Rc::new(());
    let source_id = Rc::as_ptr(&token) as usize;
    let state: Rc<(Cell<u64>, RefCell<Option<Vec<u64>>>)> =
        Rc::new((Cell::new(0), RefCell::new(None)));
    DerivedSource {
        generation: Rc::new(move || {
            let _keep = &token;
            let now = inputs();
            let (own, seen) = &*state;
            let mut seen = seen.borrow_mut();
            if seen.as_deref() != Some(now.as_slice()) {
                *seen = Some(now);
                own.set(own.get().wrapping_add(1));
            }
            own.get()
        }),
        source_id,
        subscribe,
    }
}

/// Build one [`SubscribeHook`] that fans a single callback out to every
/// source in `sources`, or `None` if any of them is poll-only.
///
/// All-or-nothing on purpose: a partial subscription is the worst outcome
/// available here — it would report *some* changes and silently miss the
/// rest, which reads as a stale UI with no error anywhere to explain it.
fn fan_out_subscribe(sources: &[DerivedSource]) -> Option<SubscribeHook> {
    let hooks: Option<Vec<SubscribeHook>> = sources.iter().map(|s| s.subscribe.clone()).collect();
    let hooks = hooks?;
    Some(Rc::new(move |notify: Rc<dyn Fn()>| {
        let handles: Vec<ObserverHandle> = hooks.iter().map(|hook| hook(notify.clone())).collect();
        // The keeper owns every per-source guard, so dropping the composite
        // handle detaches all of them; there is no observer of its own to
        // remove, hence the no-op remover.
        ObserverHandle::new(Rc::new(handles), 0, Rc::new(|_| {}))
    }))
}

enum SignalKind<T> {
    Mutable {
        inner: Rc<RefCell<MutableInner<T>>>,
        animation: Option<Rc<RefCell<AnimationState>>>,
    },
    Derived {
        compute: Rc<dyn Fn() -> T>,
        /// The upstream mutable roots this derived signal depends on.
        /// Typically one entry; `zip`/`zip3`/`and`/`or` produce many.
        /// Deduped by `source_id` at construction.
        sources: Vec<DerivedSource>,
    },
}

// ---------------------------------------------------------------------------
// Signal<T>
// ---------------------------------------------------------------------------

/// A reactive value. Created via `Signal::new(value)` for mutable signals
/// or `signal.map(f)` for derived (read-only) signals.
pub struct Signal<T> {
    kind: SignalKind<T>,
}

impl<T: 'static> Signal<T> {
    /// Create a mutable signal with an initial value.
    pub fn new(value: T) -> Self {
        Self {
            kind: SignalKind::Mutable {
                inner: Rc::new(RefCell::new(MutableInner {
                    value,
                    generation: 0,
                    observers: Vec::new(),
                    next_observer_id: 1,
                    keepalive: Vec::new(),
                })),
                animation: None,
            },
        }
    }

    /// Attach an arbitrary drop guard that lives exactly as long as the
    /// signal does — the guard is dropped when the last `Signal<T>`
    /// clone is freed (i.e., when `MutableInner<T>` is freed). Intended
    /// for adapters that observe an external source and want their
    /// `ObserverHandle` to auto-unsubscribe when the signal they're
    /// driving becomes unreachable.
    ///
    /// On a derived (read-only) signal this is a no-op; the adapter
    /// pattern only makes sense for mutable signals.
    pub fn attach_keepalive<G: 'static>(&self, guard: G) {
        if let SignalKind::Mutable { inner, .. } = &self.kind {
            inner.borrow_mut().keepalive.push(Box::new(guard));
        }
    }

    /// Get a weak reference to this signal. Callbacks registered on an
    /// external source should capture the `WeakSignal` instead of a
    /// strong `Signal<T>` clone — otherwise the callback's strong `Rc`
    /// keeps the inner alive indefinitely, creating a reference cycle.
    ///
    /// Returns `None` for derived (read-only) signals, which have no
    /// shared inner to downgrade.
    pub fn downgrade(&self) -> Option<WeakSignal<T>> {
        match &self.kind {
            SignalKind::Mutable { inner, animation } => Some(WeakSignal {
                inner: Rc::downgrade(inner),
                animation: animation.as_ref().map(Rc::downgrade),
            }),
            SignalKind::Derived { .. } => None,
        }
    }

    /// Register an observer callback. Returns an `ObserverHandle` — dropping
    /// the handle removes the callback.
    ///
    /// Works on a derived signal too; see [`try_observe`](Self::try_observe)
    /// for the one shape that does not, and panics here.
    pub fn observe(&self, f: impl Fn(&T) + 'static) -> ObserverHandle {
        self.try_observe(f)
            .expect("observe() needs a signal with fixed mutable roots")
    }

    /// Fallible [`observe`](Self::observe).
    ///
    /// A **derived** signal is observable as well as readable: it keeps the
    /// mutable roots it was built from, so this registers on each of them and
    /// recomputes on any change. That matters because `enabled(..)`,
    /// `checked(..)` and every other `Prop` accept a derived signal
    /// everywhere else — a consumer that pushes instead of polling (the
    /// macOS native menu bridge is the one in the tree) would otherwise
    /// reject exactly the bindings the rest of the framework invites.
    ///
    /// The callback is handed the derived signal's own recomputed value, not
    /// the root's. It may fire more than once for a single logical change: a
    /// derived signal over N roots has N registrations, and a caller writing
    /// two of them notifies twice. Deltas applied by an observer should
    /// therefore be idempotent — which is the same discipline
    /// [`set`](Self::set) already imposes, since it fans out unconditionally.
    ///
    /// Returns [`SignalAccessError::ReadOnly`] only for a signal whose roots
    /// are re-chosen as it is read — [`flat_map`](Self::flat_map) — where
    /// there is nothing stable to attach to.
    pub fn try_observe(
        &self,
        f: impl Fn(&T) + 'static,
    ) -> Result<ObserverHandle, SignalAccessError> {
        match &self.kind {
            SignalKind::Mutable { inner, .. } => Ok(observe_inner(inner, Rc::new(f))),
            SignalKind::Derived { compute, sources } => {
                let subscribe = fan_out_subscribe(sources).ok_or(SignalAccessError::ReadOnly)?;
                let compute = compute.clone();
                let f = Rc::new(f);
                Ok(subscribe(Rc::new(move || f(&compute()))))
            }
        }
    }

    /// Number of active observers on this signal. Derived signals always return 0.
    pub fn observer_count(&self) -> usize {
        match &self.kind {
            SignalKind::Mutable { inner, .. } => inner.borrow().observers.len(),
            SignalKind::Derived { .. } => 0,
        }
    }

    /// Whether two Signal handles point to the same underlying value.
    pub fn same(a: &Self, b: &Self) -> bool {
        match (&a.kind, &b.kind) {
            (SignalKind::Mutable { inner: a, .. }, SignalKind::Mutable { inner: b, .. }) => {
                Rc::ptr_eq(a, b)
            }
            _ => false,
        }
    }
}

impl<T: Clone + 'static> Signal<T> {
    /// Set a new value. Marks the signal as dirty and notifies observers.
    /// Panics if called on a derived (read-only) signal.
    ///
    /// Observers may freely re-enter `set`/`try_set`/`observe`, or drop their
    /// `ObserverHandle`, on this same signal from within their callback — see
    /// [`try_set`](Self::try_set).
    pub fn set(&self, value: T) {
        self.try_set(value)
            .expect("cannot set() on a derived Signal — it is read-only");
    }

    /// Set a new value only if it differs from the current one, returning
    /// whether it changed.
    ///
    /// [`set`](Self::set) has no equality check by design — it writes and fans
    /// out to every observer unconditionally. That is the right default for a
    /// signal carrying a value whose identity matters, but it makes a
    /// *republish* of an unchanged value cost a full observer walk. On a
    /// per-frame path that is pure waste: the text editors' scroll-metric step
    /// republishes four signals every tick and measured ~5% of frame CPU in
    /// `set<f32>` before its call sites were guarded by hand.
    ///
    /// This is also exactly the guard the [`try_set`](Self::try_set) docs
    /// prescribe for reactive writes that might cycle, so reach for this rather
    /// than open-coding `if sig.get() != v { sig.set(v) }` — it is the same
    /// thing, named, and it cannot be forgotten at one call site out of four.
    ///
    /// Equality is `PartialEq`, not an epsilon. For floats that is deliberate:
    /// a tolerance like `f32::EPSILON` is the machine epsilon *near 1.0*, so
    /// past a magnitude of about 1.0 the smallest representable step already
    /// exceeds it and the comparison silently degrades into exact inequality
    /// anyway — while near zero it would suppress writes that genuinely
    /// changed. A caller that truly wants a tolerance wants a domain-specific
    /// one, and should say so at its own call site.
    ///
    /// Panics on a derived (read-only) signal, like [`set`](Self::set).
    pub fn set_if_changed(&self, value: T) -> bool
    where
        T: PartialEq,
    {
        if self.get() == value {
            return false;
        }
        self.set(value);
        true
    }

    /// Fallible [`set`](Self::set): returns [`SignalAccessError::ReadOnly`]
    /// for a derived signal instead of panicking.
    ///
    /// The new value and the observer callbacks are snapshotted while the
    /// inner `RefCell` is borrowed, then **all** borrows are released before
    /// any callback runs. An observer is therefore free to re-enter
    /// `set`/`try_set`/`observe`, or drop an `ObserverHandle`, on this same
    /// signal without a `RefCell` borrow conflict — mirroring the
    /// mutate-then-notify discipline used across `teksilo-data`. Each
    /// notification delivers the value as written by *that* call; observer
    /// additions or removals made during a callback take effect only on
    /// subsequent notifications.
    ///
    /// # Feedback loops
    ///
    /// Re-entrancy is supported, but a write cascade that never settles — A's
    /// observer writes B and B's observer writes A, with no equality guard — is
    /// an unbounded recursion that will overflow the stack. Guard reactive
    /// writes that may cycle (`if sig.get() != v { sig.set(v) }`) or break one
    /// edge with a [`WeakSignal`]. In debug builds a depth guard turns a runaway
    /// loop into a diagnostic panic instead of a silent stack overflow; release
    /// builds carry no such check.
    pub fn try_set(&self, value: T) -> Result<(), SignalAccessError> {
        match &self.kind {
            SignalKind::Mutable { inner, .. } => {
                let (snapshot, callbacks) = {
                    let mut guard = inner.borrow_mut();
                    guard.value = value;
                    guard.generation = guard.generation.wrapping_add(1);
                    let callbacks: Vec<_> =
                        guard.observers.iter().map(|e| e.callback.clone()).collect();
                    (guard.value.clone(), callbacks)
                };
                // Debug-only: a re-entrant observer bumps this depth; an
                // unbounded feedback loop trips the limit and panics with a
                // diagnostic rather than overflowing the stack.
                #[cfg(debug_assertions)]
                let _depth = NotifyDepthGuard::enter();
                for cb in &callbacks {
                    cb(&snapshot);
                }
                Ok(())
            }
            SignalKind::Derived { .. } => Err(SignalAccessError::ReadOnly),
        }
    }

    /// Read the current value (cloned).
    pub fn get(&self) -> T {
        match &self.kind {
            SignalKind::Mutable { inner, .. } => inner.borrow().value.clone(),
            SignalKind::Derived { compute, .. } => compute(),
        }
    }

    /// Read the current value by reference (only for mutable signals).
    /// Panics on derived signals.
    pub fn get_ref(&self) -> Ref<'_, T> {
        self.try_get_ref()
            .expect("get_ref() is only supported on mutable signals")
    }

    pub fn try_get_ref(&self) -> Result<Ref<'_, T>, SignalAccessError> {
        match &self.kind {
            SignalKind::Mutable { inner, .. } => Ok(Ref::map(inner.borrow(), |guard| &guard.value)),
            SignalKind::Derived { .. } => Err(SignalAccessError::ReadOnly),
        }
    }

    /// Create a derived (read-only) signal whose value is computed from
    /// this signal. The closure runs lazily when the derived signal is read.
    pub fn map<U: Clone + 'static>(&self, f: impl Fn(&T) -> U + 'static) -> Signal<U> {
        let compute = self.as_compute();
        let sources = self.as_sources();
        Signal {
            kind: SignalKind::Derived {
                compute: Rc::new(move || f(&compute())),
                sources,
            },
        }
    }

    /// Zip this signal with another, producing a derived signal that
    /// observes both upstream sources. The resulting signal is marked
    /// dirty whenever *either* source flips, so widgets binding to it
    /// correctly re-render on any input change.
    ///
    /// Combine with [`Signal::map`] for n-ary predicates:
    ///
    /// ```
    /// # use teksilo_core::Signal;
    /// # let focus = Signal::new(false);
    /// # let readonly = Signal::new(false);
    /// let composite = focus.zip(&readonly).map(|(f, r)| *f && !*r);
    /// ```
    pub fn zip<U: Clone + 'static>(&self, other: &Signal<U>) -> Signal<(T, U)> {
        let a = self.as_compute();
        let b = other.as_compute();
        let mut sources = self.as_sources();
        merge_sources(&mut sources, other.as_sources());
        Signal {
            kind: SignalKind::Derived {
                compute: Rc::new(move || (a(), b())),
                sources,
            },
        }
    }

    /// Zip three signals. See [`Signal::zip`].
    pub fn zip3<U: Clone + 'static, V: Clone + 'static>(
        &self,
        b: &Signal<U>,
        c: &Signal<V>,
    ) -> Signal<(T, U, V)> {
        let fa = self.as_compute();
        let fb = b.as_compute();
        let fc = c.as_compute();
        let mut sources = self.as_sources();
        merge_sources(&mut sources, b.as_sources());
        merge_sources(&mut sources, c.as_sources());
        Signal {
            kind: SignalKind::Derived {
                compute: Rc::new(move || (fa(), fb(), fc())),
                sources,
            },
        }
    }

    /// Like [`map`](Self::map), but the resulting derived signal
    /// presents a **single** combined `DerivedSource` to binding
    /// registrations instead of one per upstream root.
    ///
    /// For animation-driven multi-axis signals — e.g. a
    /// `pan_x.zip3(pan_y, zoom).zip(rotation)` view transform that
    /// flips all four sources on every animation tick — this collapses
    /// the per-tick binding work from O(N) to O(1) without changing
    /// dirty-tracking semantics: the composite source is dirty when
    /// any underlying source is, and clearing the composite clears
    /// every underlying source.
    ///
    /// Use only when the derived signal's value depends on **all**
    /// upstream sources being read together (a compose function);
    /// when only one upstream changes per frame, [`map`](Self::map) is
    /// equivalent and cheaper.
    pub fn map_coalesced<U: Clone + 'static>(&self, f: impl Fn(&T) -> U + 'static) -> Signal<U> {
        let compute = self.as_compute();
        let underlying = self.as_sources();
        if underlying.len() <= 1 {
            // One source already — no coalescing benefit; delegate
            // to plain map to avoid an extra indirection.
            return self.map(f);
        }
        // The coalesced source stands in for a *fixed* set of roots, so it
        // can still be subscribed to — one registration per root, behind the
        // one hook. (`flat_map` below is the case that cannot.)
        let subscribe = fan_out_subscribe(&underlying);
        let coalesced = coalesced_source(
            Rc::new(move || underlying.iter().map(|s| (s.generation)()).collect()),
            subscribe,
        );
        Signal {
            kind: SignalKind::Derived {
                compute: Rc::new(move || f(&compute())),
                sources: vec![coalesced],
            },
        }
    }

    /// Switch / bind: derive a signal whose value **and** dirty-tracking
    /// follow the inner `Signal<U>` selected by `f` from this signal's
    /// current value. When *this* signal changes, `f` re-selects a
    /// (possibly different) inner signal and the result follows that one
    /// instead — the classic reactive "switchLatest" combinator.
    ///
    /// Unlike [`map`](Self::map), the result depends on an inner source
    /// that is chosen dynamically, so it exposes a **single composite
    /// `DerivedSource`** whose dirty/clear evaluate the currently-selected
    /// inner each time they are polled. Binding registration stays O(1)
    /// regardless of how many distinct inner signals `f` may return.
    ///
    /// Typical use — track the *active* item's reactive flag out of a set:
    ///
    /// ```
    /// # use teksilo_core::Signal;
    /// # let current_step: Signal<usize> = Signal::new(0);
    /// # let completion: Vec<Signal<bool>> = vec![Signal::new(true), Signal::new(false)];
    /// // disable Next until the currently-shown step's gate is satisfied
    /// let gate = current_step.flat_map(move |i| completion[*i].clone());
    /// // ctx.enabled_when(next_id, gate);
    /// # let _ = gate;
    /// ```
    pub fn flat_map<U: Clone + 'static>(&self, f: impl Fn(&T) -> Signal<U> + 'static) -> Signal<U> {
        let f: Rc<dyn Fn(&T) -> Signal<U>> = Rc::new(f);
        let outer_compute = self.as_compute();
        let outer_sources = self.as_sources();

        // compute: select the inner signal from the outer value, read it.
        let compute: Rc<dyn Fn() -> U> = {
            let f = f.clone();
            let outer_compute = outer_compute.clone();
            Rc::new(move || f(&outer_compute()).get())
        };

        // One composite source, whose inputs are the outer sources' own
        // generations PLUS the generations of whichever inner signal is
        // selected right now. The inner half is spliced in as its
        // individual leaf generations rather than as one folded `u64`:
        // the selected inner can change identity between polls, so the
        // *shape* of the input vector is itself information — a switch
        // from a 1-source inner to a 2-source one is a change even if
        // the numbers happen to line up. `coalesced_source` turns the
        // whole vector back into a monotone counter.
        let composite = coalesced_source(
            {
                let f = f.clone();
                let outer_compute = outer_compute.clone();
                Rc::new(move || {
                    let mut gens: Vec<u64> =
                        outer_sources.iter().map(|s| (s.generation)()).collect();
                    gens.extend(
                        f(&outer_compute())
                            .as_sources()
                            .iter()
                            .map(|s| (s.generation)()),
                    );
                    gens
                })
            },
            // Poll-only: `f` re-selects the inner signal on every poll, so
            // there is no stable set of roots to attach an observer to.
            None,
        );

        Signal {
            kind: SignalKind::Derived {
                compute,
                sources: vec![composite],
            },
        }
    }

    /// Borrow a compute closure that reads this signal's current value.
    /// For mutable signals this clones the inner cell's value; for
    /// derived signals it clones the parent compute `Rc`.
    fn as_compute(&self) -> Rc<dyn Fn() -> T> {
        match &self.kind {
            SignalKind::Mutable { inner, .. } => {
                let source = inner.clone();
                Rc::new(move || source.borrow().value.clone())
            }
            SignalKind::Derived { compute, .. } => compute.clone(),
        }
    }

    /// Bind this signal to a widget at the given dirty-tracking level.
    ///
    /// For a mutable or single-source derived signal this registers one
    /// binding. For a multi-source derived signal (built via `zip` /
    /// `zip3` / `and` / `or`) this registers one binding per observed
    /// mutable root so dirty flips on *any* source correctly re-render
    /// the widget.
    ///
    /// Idempotent per `(widget_id, source_id, bucket)` tuple: duplicate
    /// calls collapse in the [`BindingRegistry`], promoting the level
    /// if the incoming one has higher priority.
    pub fn bind_to(&self, widget_id: WidgetId, registry: &BindingRegistry, level: BindingLevel) {
        for src in self.as_sources() {
            registry.register(Binding {
                widget_id,
                level,
                generation: src.generation,
                source_id: src.source_id,
            });
        }
    }

    /// Materialise this signal's upstream sources as a `Vec`. Mutable
    /// signals yield one entry anchored on their inner `Rc`; derived
    /// signals clone their existing sources vec.
    fn as_sources(&self) -> Vec<DerivedSource> {
        match &self.kind {
            SignalKind::Mutable { inner, .. } => {
                let gen_src = inner.clone();
                let sub_src = inner.clone();
                let source_id = Rc::as_ptr(inner) as *const () as usize;
                vec![DerivedSource {
                    // The closure owns an `Rc` clone of the inner, so the
                    // address used as `source_id` cannot be recycled by a
                    // different signal while this source is reachable.
                    generation: Rc::new(move || gen_src.borrow().generation),
                    source_id,
                    // A mutable root can be pushed from, so it publishes the
                    // hook. The callback discards the root's value: whoever
                    // registered it is downstream of a `map`/`zip` and wants
                    // its own recomputed value, not this one.
                    subscribe: Some(Rc::new(move |notify: Rc<dyn Fn()>| {
                        observe_inner(&sub_src, Rc::new(move |_: &T| notify()))
                    })),
                }]
            }
            SignalKind::Derived { sources, .. } => sources.clone(),
        }
    }
}

/// Extend `dst` with entries from `incoming`, deduping by `source_id`
/// so the same mutable root is never registered twice in a combined
/// derived signal (e.g. `a.zip(&a.map(...))`).
fn merge_sources(dst: &mut Vec<DerivedSource>, incoming: Vec<DerivedSource>) {
    for s in incoming {
        if !dst.iter().any(|d| d.source_id == s.source_id) {
            dst.push(s);
        }
    }
}

impl<T: 'static> Signal<T> {
    pub fn is_mutable(&self) -> bool {
        matches!(self.kind, SignalKind::Mutable { .. })
    }

    /// This signal's current change generation — a monotonically
    /// non-decreasing counter advanced by every write. Two reads returning
    /// the same value mean nothing changed in between; any difference
    /// means something did.
    ///
    /// There is deliberately no way to *reset* it. Dirty tracking is
    /// "compare against what I last acted on", and the remembered value
    /// belongs to the consumer — see [`BindingRegistry`], which keeps
    /// one per bound source. A resettable flag on the signal itself would
    /// mean N consumers fighting over one slot, which is precisely the bug
    /// this counter replaced (see `MutableInner::generation`).
    ///
    /// For a derived signal this folds every upstream into one number by
    /// summing. That is exact rather than merely convenient: every
    /// upstream generation is itself monotone, so a sum can only stay put
    /// when all of them do.
    pub fn generation(&self) -> u64 {
        match &self.kind {
            SignalKind::Mutable { inner, .. } => inner.borrow().generation,
            SignalKind::Derived { sources, .. } => sources
                .iter()
                .map(|s| (s.generation)())
                .fold(0u64, u64::wrapping_add),
        }
    }
}

// ---------------------------------------------------------------------------
// Signal<bool> — boolean combinators for composite enabled_when predicates
// ---------------------------------------------------------------------------

impl Signal<bool> {
    /// Logical AND of two boolean signals. The resulting derived signal
    /// tracks both upstream sources and is marked dirty whenever either
    /// changes (no short-circuit on the dirty side — semantic correctness
    /// over micro-optimisation).
    pub fn and(&self, other: &Signal<bool>) -> Signal<bool> {
        self.zip(other).map(|(a, b)| *a && *b)
    }

    /// Logical OR of two boolean signals. Same dirty-tracking semantics
    /// as [`Signal::and`].
    pub fn or(&self, other: &Signal<bool>) -> Signal<bool> {
        self.zip(other).map(|(a, b)| *a || *b)
    }

    /// Logical NOT of a boolean signal.
    pub fn not(&self) -> Signal<bool> {
        self.map(|b| !*b)
    }
}

// ---------------------------------------------------------------------------
// Signal<f32> — animation support
// ---------------------------------------------------------------------------

impl Signal<f32> {
    /// Create a new `Signal<f32>` with animation support.
    pub fn new_animated(value: f32) -> Self {
        Self {
            kind: SignalKind::Mutable {
                inner: Rc::new(RefCell::new(MutableInner {
                    value,
                    generation: 0,
                    observers: Vec::new(),
                    next_observer_id: 1,
                    keepalive: Vec::new(),
                })),
                animation: Some(Rc::new(RefCell::new(AnimationState {
                    pending: None,
                    target: None,
                }))),
            },
        }
    }

    pub fn supports_animation(&self) -> bool {
        matches!(
            &self.kind,
            SignalKind::Mutable {
                animation: Some(_),
                ..
            }
        )
    }

    pub(crate) fn weak_handle(&self) -> Option<WeakAnimatedSignal> {
        match &self.kind {
            SignalKind::Mutable {
                inner,
                animation: Some(animation),
            } => Some(WeakAnimatedSignal {
                inner: Rc::downgrade(inner),
                animation: Rc::downgrade(animation),
            }),
            _ => None,
        }
    }

    /// Animate to a target value over a duration with an easing curve.
    pub fn animate_to(
        &self,
        target: f32,
        duration: std::time::Duration,
        easing: teksilo_tokens::Easing,
    ) {
        self.animate_to_with_frame_interval(target, duration, easing, None);
    }

    pub fn animate_to_with_frame_interval(
        &self,
        target: f32,
        duration: std::time::Duration,
        easing: teksilo_tokens::Easing,
        frame_interval: Option<std::time::Duration>,
    ) {
        self.try_animate_to_with_frame_interval(target, duration, easing, frame_interval)
            .unwrap_or_else(|err| match err {
                SignalAccessError::ReadOnly => {
                    panic!("animate_to is not supported on derived signals")
                }
                SignalAccessError::AnimationUnsupported => {
                    panic!(
                        "animate_to called on Signal<f32> without animation support; use Signal::new_animated()"
                    )
                }
            });
    }

    pub fn try_animate_to(
        &self,
        target: f32,
        duration: std::time::Duration,
        easing: teksilo_tokens::Easing,
    ) -> Result<(), SignalAccessError> {
        self.try_animate_to_with_frame_interval(target, duration, easing, None)
    }

    pub fn try_animate_to_with_frame_interval(
        &self,
        target: f32,
        duration: std::time::Duration,
        easing: teksilo_tokens::Easing,
        frame_interval: Option<std::time::Duration>,
    ) -> Result<(), SignalAccessError> {
        self.try_animate_with_options(crate::animation::AnimationRequest {
            target,
            duration,
            easing,
            frame_interval,
            looping: false,
            epsilon: 0.0,
            max_duration: None,
        })
    }

    /// Start a looping animation from the current value to `target`,
    /// repeating with the given period. Runs until cancelled.
    /// Frame updates are capped at `frame_interval` (default 60 Hz).
    pub fn animate_looping(
        &self,
        target: f32,
        period: std::time::Duration,
        easing: teksilo_tokens::Easing,
        frame_interval: Option<std::time::Duration>,
    ) {
        let _ = self.try_animate_with_options(crate::animation::AnimationRequest {
            target,
            duration: period,
            easing,
            frame_interval,
            looping: true,
            epsilon: 0.0,
            max_duration: None,
        });
    }

    /// Generic entry point that accepts a fully-built `AnimationRequest`.
    /// Use this to set `epsilon` (pixel-stable quantization) or
    /// `max_duration` (opt-in wall-clock cap) without having to thread
    /// through every convenience wrapper.
    pub fn try_animate_with_options(
        &self,
        request: crate::animation::AnimationRequest,
    ) -> Result<(), SignalAccessError> {
        match &self.kind {
            SignalKind::Mutable {
                inner,
                animation: Some(animation),
            } => {
                let mut anim = animation.borrow_mut();
                anim.target = Some(request.target);
                anim.pending = Some(request);
                drop(anim);
                // Arming an animation is a change like any other write:
                // bound widgets must reconcile so the tree picks the
                // pending request up in `process_pending_animations`.
                let mut guard = inner.borrow_mut();
                guard.generation = guard.generation.wrapping_add(1);
                Ok(())
            }
            SignalKind::Mutable {
                animation: None, ..
            } => Err(SignalAccessError::AnimationUnsupported),
            SignalKind::Derived { .. } => Err(SignalAccessError::ReadOnly),
        }
    }

    /// Returns the target value of the current or pending animation, if any.
    pub fn animation_target(&self) -> Option<f32> {
        match &self.kind {
            SignalKind::Mutable { animation, .. } => {
                animation.as_ref().and_then(|a| a.borrow().target)
            }
            _ => None,
        }
    }

    /// Clear the animation target. Called by the animation scheduler when
    /// an animation completes.
    pub fn clear_animation_target(&self) {
        if let SignalKind::Mutable { animation, .. } = &self.kind
            && let Some(a) = animation
        {
            a.borrow_mut().target = None;
        }
    }

    /// Take a pending animation request, if any.
    pub fn take_pending_animation(&self) -> Option<crate::animation::AnimationRequest> {
        match &self.kind {
            SignalKind::Mutable { animation, .. } => animation
                .as_ref()
                .and_then(|a| a.borrow_mut().pending.take()),
            _ => None,
        }
    }

    /// Whether there is a pending animation request.
    pub fn has_pending_animation(&self) -> bool {
        match &self.kind {
            SignalKind::Mutable { animation, .. } => animation
                .as_ref()
                .is_some_and(|a| a.borrow().pending.is_some()),
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Clone, Debug
// ---------------------------------------------------------------------------

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        Self {
            kind: match &self.kind {
                SignalKind::Mutable { inner, animation } => SignalKind::Mutable {
                    inner: inner.clone(),
                    animation: animation.clone(),
                },
                SignalKind::Derived { compute, sources } => SignalKind::Derived {
                    compute: compute.clone(),
                    sources: sources.clone(),
                },
            },
        }
    }
}

impl<T: std::fmt::Debug + 'static> std::fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            SignalKind::Mutable { inner, .. } => f
                .debug_struct("Signal::Mutable")
                .field("value", &inner.borrow().value)
                .field("generation", &inner.borrow().generation)
                .finish(),
            SignalKind::Derived { .. } => f.write_str("Signal::Derived(..)"),
        }
    }
}

// ---------------------------------------------------------------------------
// Prop<T> — widget property type
// ---------------------------------------------------------------------------

/// A property value that is either static or bound to a reactive signal.
/// Widget property methods accept `impl Into<Prop<T>>` for flexibility.
pub enum Prop<T: Clone + 'static> {
    /// A fixed value, set once.
    Static(T),
    /// Bound to a signal; value read lazily on each use.
    Bound(Signal<T>),
}

impl<T: Clone + 'static> Prop<T> {
    /// Resolve the current value.
    pub fn get(&self) -> T {
        match self {
            Prop::Static(v) => v.clone(),
            Prop::Bound(signal) => signal.get(),
        }
    }

    /// Register dirty tracking for this prop if it is bound.
    pub fn register_if_bound(
        &self,
        widget_id: WidgetId,
        registry: &BindingRegistry,
        level: BindingLevel,
    ) {
        if let Prop::Bound(signal) = self {
            signal.bind_to(widget_id, registry, level);
        }
    }

    /// Return the underlying signal if bound, or wrap a static value in a
    /// fresh, unshared signal. Use when an existing code path needs a
    /// `Signal<T>` (e.g. `ctx.effect(&signal, ...)`) but the widget field
    /// was widened from `Signal<T>` to `Prop<T>` — the derived signal
    /// preserves reactivity for the `Bound` case with minimal churn.
    pub fn as_signal(&self) -> Signal<T> {
        match self {
            Prop::Static(v) => Signal::new(v.clone()),
            Prop::Bound(signal) => signal.clone(),
        }
    }
}

impl<T: Clone + 'static> Clone for Prop<T> {
    fn clone(&self) -> Self {
        match self {
            Prop::Static(v) => Prop::Static(v.clone()),
            Prop::Bound(s) => Prop::Bound(s.clone()),
        }
    }
}

impl<T: Clone + std::fmt::Debug + 'static> std::fmt::Debug for Prop<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Prop::Static(v) => write!(f, "Prop::Static({:?})", v),
            Prop::Bound(_) => f.write_str("Prop::Bound(..)"),
        }
    }
}

impl<T: Clone + 'static> From<T> for Prop<T> {
    fn from(value: T) -> Self {
        Prop::Static(value)
    }
}

impl<T: Clone + 'static> From<Signal<T>> for Prop<T> {
    fn from(signal: Signal<T>) -> Self {
        Prop::Bound(signal)
    }
}

// String ergonomics: let `impl Into<Prop<String>>` setters accept a borrowed
// string literal / `&String` the same way the old `impl Into<String>` setters
// did (the blanket `From<T>` only covers an owned `String`). Keeps call sites
// like `.name("x")` / `.suffix("x")` compiling after the widening.
impl From<&str> for Prop<String> {
    fn from(s: &str) -> Self {
        Prop::Static(s.to_owned())
    }
}

impl From<&String> for Prop<String> {
    fn from(s: &String) -> Self {
        Prop::Static(s.clone())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_get_set() {
        let s = Signal::new(42);
        assert_eq!(s.get(), 42);
        s.set(99);
        assert_eq!(s.get(), 99);
    }

    #[test]
    fn generation_advances_on_every_write_and_never_resets() {
        let s = Signal::new(0);
        let start = s.generation();
        assert_eq!(s.generation(), start, "reading is not a change");

        s.set(1);
        let after_one = s.generation();
        assert_ne!(after_one, start, "a write advances the generation");

        s.set(1);
        let after_republish = s.generation();
        assert_ne!(
            after_republish, after_one,
            "`set` is unconditional — a republish of the same value is still \
             a write, and callers who want the equality guard use \
             `set_if_changed`"
        );

        assert!(!s.set_if_changed(1), "value is unchanged");
        assert_eq!(
            s.generation(),
            after_republish,
            "`set_if_changed` with an identical value writes nothing at all"
        );
    }

    /// The property every consumer relies on: staleness is "the
    /// generation moved since I last looked", and looking is free of
    /// consequence — so any number of independent consumers can each
    /// track the same signal without interfering.
    #[test]
    fn observing_the_generation_does_not_consume_it() {
        let s = Signal::new(0);
        let (mut seen_a, mut seen_b) = (s.generation(), s.generation());

        s.set(1);
        assert_ne!(s.generation(), seen_a);
        seen_a = s.generation();
        assert_ne!(
            s.generation(),
            seen_b,
            "consumer A catching up must leave consumer B behind, not clean"
        );
        seen_b = s.generation();

        assert_eq!(s.generation(), seen_a);
        assert_eq!(s.generation(), seen_b);
    }

    #[test]
    fn signal_clone_shares() {
        let a = Signal::new(10);
        let b = a.clone();
        a.set(20);
        assert_eq!(b.get(), 20);
        assert!(Signal::same(&a, &b));
    }

    #[test]
    fn signal_map_derived() {
        let text = Signal::new(String::from("hello"));
        let len = text.map(|t| t.len());
        assert_eq!(len.get(), 5);
        text.set(String::from("hi"));
        assert_eq!(len.get(), 2);
    }

    #[test]
    fn signal_map_chained() {
        let s = Signal::new(5);
        let doubled = s.map(|v| v * 2);
        let as_string = doubled.map(|v| format!("{}", v));
        assert_eq!(as_string.get(), "10");
        s.set(7);
        assert_eq!(as_string.get(), "14");
    }

    #[test]
    fn signal_derived_generation_tracks_source() {
        let s = Signal::new(0);
        let derived = s.map(|v| v + 1);
        let seen = derived.generation();
        s.set(5);
        assert_ne!(
            derived.generation(),
            seen,
            "the source's write shows through"
        );
        let seen = derived.generation();
        assert_eq!(
            derived.generation(),
            seen,
            "and settles with no further write"
        );
    }

    #[test]
    fn flat_map_follows_selected_inner_value() {
        let a = Signal::new(10);
        let b = Signal::new(20);
        let which = Signal::new(0usize);
        let (a2, b2) = (a.clone(), b.clone());
        let out = which.flat_map(move |i| if *i == 0 { a2.clone() } else { b2.clone() });

        assert_eq!(out.get(), 10); // follows a
        a.set(11);
        assert_eq!(out.get(), 11); // tracks a's value
        which.set(1);
        assert_eq!(out.get(), 20); // switched to b
        b.set(21);
        assert_eq!(out.get(), 21);
        a.set(999); // a is no longer selected — ignored
        assert_eq!(out.get(), 21);
    }

    #[test]
    fn flat_map_generation_tracks_outer_and_current_inner() {
        let a = Signal::new(0);
        let b = Signal::new(0);
        let which = Signal::new(0usize);
        let (a2, b2) = (a.clone(), b.clone());
        let out = which.flat_map(move |i| if *i == 0 { a2.clone() } else { b2.clone() });
        let mut seen = out.generation();

        // The current inner (a) flipping advances the result.
        a.set(5);
        assert_ne!(out.generation(), seen);
        seen = out.generation();
        assert_eq!(out.generation(), seen);

        // The non-selected inner (b) flipping does NOT.
        b.set(7);
        assert_eq!(out.generation(), seen, "b is not selected");

        // The outer selector flipping does.
        which.set(1);
        assert_ne!(out.generation(), seen);
        seen = out.generation();

        // Now b is selected, so b flipping is tracked and a is ignored.
        b.set(8);
        assert_ne!(out.generation(), seen);
        seen = out.generation();
        a.set(9);
        assert_eq!(out.generation(), seen, "a is no longer selected");
    }

    /// `flat_map` is why the composite source memoises a counter rather
    /// than summing its inputs. Switching the selected inner makes that
    /// term jump arbitrarily — here it jumps *down*, from a heavily
    /// written signal to a fresh one, by exactly as much as the outer
    /// selector's own write advanced. A sum would land on the same total
    /// and report "nothing changed" for a switch that changed everything.
    #[test]
    fn flat_map_survives_an_inner_switch_that_would_cancel_out_in_a_sum() {
        let hot = Signal::new(0_i32);
        let cold = Signal::new(0_i32);
        // `which` reaches generation 1 on the switch below, so drive
        // `hot` exactly one generation ahead of `cold`: switching from
        // hot to cold then costs -1 while `which` contributes +1.
        hot.set(1);
        let (hot2, cold2) = (hot.clone(), cold.clone());
        let which = Signal::new(0usize);
        let out = which.flat_map(move |i| if *i == 0 { hot2.clone() } else { cold2.clone() });

        assert_eq!(out.get(), 1, "starts on `hot`");
        let seen = out.generation();

        which.set(1);

        assert_eq!(out.get(), 0, "the value really did change");
        assert_ne!(
            out.generation(),
            seen,
            "and the generation says so — a plain sum of (outer + inner) \
             would have been unchanged here"
        );
    }

    /// The cross-window property at the level of a composite source: a
    /// `flat_map`'s memo advances once and is then read, unchanged, by
    /// every consumer polling afterwards. If the memo were consumed by
    /// the first reader (the way the old `clear_dirty` consumed a shared
    /// flag) the second window would never rebuild.
    #[test]
    fn a_composite_sources_generation_is_readable_by_every_consumer() {
        let inner = Signal::new(0_i32);
        let inner2 = inner.clone();
        let which = Signal::new(0usize);
        let out = which.flat_map(move |_| inner2.clone());

        let (window_a, window_b) = (out.generation(), out.generation());
        inner.set(1);

        let a_now = out.generation();
        assert_ne!(a_now, window_a, "window A notices");
        assert_ne!(
            out.generation(),
            window_b,
            "and window B still notices, after A already looked"
        );
        assert_eq!(out.generation(), a_now, "both see the SAME new generation");
    }

    #[test]
    fn flat_map_binding_rerenders_on_inner_and_outer_change() {
        use crate::binding::{BindingLevel, BindingRegistry};
        use slotmap::KeyData;
        let fake_id: WidgetId = KeyData::from_ffi(1).into();
        let inner = Signal::new(false);
        let which = Signal::new(0usize);
        let inner2 = inner.clone();
        let gate = which.flat_map(move |_| inner2.clone());

        let registry = BindingRegistry::new();
        gate.bind_to(fake_id, &registry, BindingLevel::Relayout);
        assert!(registry.flush_dirty().is_empty());

        // A change to the currently-selected inner must dirty the bound widget.
        inner.set(true);
        let dirty = registry.flush_dirty();
        assert_eq!(dirty.len(), 1, "selected-inner change must re-render");
        assert_eq!(dirty[0].0, fake_id);

        // A change to the outer selector must also dirty it.
        which.set(0);
        let dirty = registry.flush_dirty();
        assert_eq!(dirty.len(), 1, "outer-selector change must re-render");
    }

    #[test]
    fn observer_called_on_set() {
        use std::cell::Cell;
        let s = Signal::new(0);
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        let _handle = s.observe(move |val| {
            assert_eq!(*val, 42);
            c.set(true);
        });
        s.set(42);
        assert!(called.get());
    }

    /// The whole reason `set_if_changed` exists: `set` fans out to every
    /// observer even when the value is identical, which on a per-frame
    /// republish path is pure waste.
    #[test]
    fn set_if_changed_does_not_notify_when_the_value_is_identical() {
        use std::cell::Cell;
        let s = Signal::new(7);
        let calls = Rc::new(Cell::new(0));
        let c = calls.clone();
        let _handle = s.observe(move |_| c.set(c.get() + 1));

        assert!(
            !s.set_if_changed(7),
            "an identical write must report no change"
        );
        assert_eq!(calls.get(), 0, "an identical write must not walk observers");

        assert!(
            s.set_if_changed(8),
            "a differing write must report a change"
        );
        assert_eq!(calls.get(), 1, "a differing write must notify");
        assert_eq!(s.get(), 8);
    }

    /// Guarding a write is what breaks an A→B→A observer cycle, which the
    /// `try_set` docs prescribe and which callers previously hand-rolled.
    #[test]
    fn set_if_changed_settles_a_two_signal_feedback_loop() {
        let a = Signal::new(0);
        let b = Signal::new(0);
        let _ha = {
            let b = b.clone();
            a.observe(move |v| {
                b.set_if_changed(*v);
            })
        };
        let _hb = {
            let a = a.clone();
            b.observe(move |v| {
                a.set_if_changed(*v);
            })
        };
        // Without the equality guard this recurses until the depth guard trips.
        a.set(5);
        assert_eq!(b.get(), 5);
        assert_eq!(a.get(), 5);
    }

    #[test]
    fn observer_removed_on_handle_drop() {
        use std::cell::Cell;
        let s = Signal::new(0);
        let count = Rc::new(Cell::new(0));
        let c = count.clone();
        let handle = s.observe(move |_| {
            c.set(c.get() + 1);
        });
        s.set(1);
        assert_eq!(count.get(), 1);
        drop(handle);
        s.set(2);
        assert_eq!(count.get(), 1); // Not called again
    }

    #[test]
    fn multiple_observers() {
        use std::cell::Cell;
        let s = Signal::new(0);
        let count = Rc::new(Cell::new(0));
        let c1 = count.clone();
        let c2 = count.clone();
        let _h1 = s.observe(move |_| c1.set(c1.get() + 1));
        let _h2 = s.observe(move |_| c2.set(c2.get() + 1));
        s.set(10);
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn binding_registry_integration() {
        use slotmap::KeyData;
        let fake_id: WidgetId = KeyData::from_ffi(1).into();
        let registry = BindingRegistry::new();
        let s = Signal::new(0);
        s.bind_to(fake_id, &registry, BindingLevel::RepaintOnly);

        assert!(registry.flush_dirty().is_empty());
        s.set(42);
        let dirty = registry.flush_dirty();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].0, fake_id);
        assert_eq!(dirty[0].1, BindingLevel::RepaintOnly);
        assert!(registry.flush_dirty().is_empty());
    }

    #[test]
    fn derived_binding_registry() {
        use slotmap::KeyData;
        let fake_id: WidgetId = KeyData::from_ffi(1).into();
        let registry = BindingRegistry::new();
        let s = Signal::new(0);
        let doubled = s.map(|v| v * 2);
        doubled.bind_to(fake_id, &registry, BindingLevel::Relayout);

        assert!(registry.flush_dirty().is_empty());
        s.set(5);
        let dirty = registry.flush_dirty();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].1, BindingLevel::Relayout);
    }

    #[test]
    fn get_ref_works() {
        let s = Signal::new(String::from("hello"));
        {
            let r = s.get_ref();
            assert_eq!(&*r, "hello");
        }
    }

    #[test]
    #[should_panic(expected = "cannot set() on a derived Signal")]
    fn set_on_derived_panics() {
        let s = Signal::new(0);
        let d = s.map(|v| v + 1);
        d.set(99);
    }

    #[test]
    fn prop_static() {
        let p: Prop<i32> = 42.into();
        assert_eq!(p.get(), 42);
    }

    #[test]
    fn prop_bound() {
        let s = Signal::new(10);
        let p: Prop<i32> = s.clone().into();
        assert_eq!(p.get(), 10);
        s.set(20);
        assert_eq!(p.get(), 20);
    }

    #[test]
    fn prop_register_if_bound() {
        use slotmap::KeyData;
        let fake_id: WidgetId = KeyData::from_ffi(1).into();
        let registry = BindingRegistry::new();

        let s = Signal::new(0);
        let p: Prop<i32> = s.clone().into();
        p.register_if_bound(fake_id, &registry, BindingLevel::RepaintOnly);

        s.set(1);
        let dirty = registry.flush_dirty();
        assert_eq!(dirty.len(), 1);

        // Static prop does not register
        let p2: Prop<i32> = 42.into();
        p2.register_if_bound(fake_id, &registry, BindingLevel::RepaintOnly);
        assert!(registry.flush_dirty().is_empty());
    }

    // --- Multi-source derived signals (zip / zip3 / and / or / not) -------

    #[test]
    fn zip_reads_both_sources() {
        let a = Signal::new(1_i32);
        let b = Signal::new("x".to_string());
        let z = a.zip(&b);
        assert_eq!(z.get(), (1, "x".to_string()));
        a.set(7);
        b.set("y".to_string());
        assert_eq!(z.get(), (7, "y".to_string()));
    }

    #[test]
    fn zip_generation_advances_when_either_source_is_written() {
        let a = Signal::new(0_i32);
        let b = Signal::new(0_i32);
        let z = a.zip(&b);
        let mut seen = z.generation();

        a.set(1);
        assert_ne!(z.generation(), seen, "a write to the first source shows");
        seen = z.generation();

        b.set(2);
        assert_ne!(z.generation(), seen, "a write to the second source shows");
        seen = z.generation();

        assert_eq!(z.generation(), seen, "and settles with no further write");
    }

    /// A multi-source derived signal folds its upstreams into one number
    /// by summing, and that is only sound because every upstream
    /// generation is monotone: writes to *different* sources can never
    /// cancel each other out.
    #[test]
    fn zip_generation_reflects_writes_to_both_sources_independently() {
        let a = Signal::new(0_i32);
        let b = Signal::new(0_i32);
        let z = a.zip(&b);

        let start = z.generation();
        a.set(1);
        let after_a = z.generation();
        b.set(1);
        let after_b = z.generation();

        assert!(
            after_a > start && after_b > after_a,
            "monotone in both sources: {start} < {after_a} < {after_b}"
        );
    }

    #[test]
    fn zip3_reads_three_sources() {
        let a = Signal::new(1_i32);
        let b = Signal::new(2_i32);
        let c = Signal::new(3_i32);
        let z = a.zip3(&b, &c);
        assert_eq!(z.get(), (1, 2, 3));
        c.set(30);
        assert_eq!(z.get(), (1, 2, 30));
    }

    #[test]
    fn zip3_generation_advances_for_any_source() {
        let a = Signal::new(0_i32);
        let b = Signal::new(0_i32);
        let c = Signal::new(0_i32);
        let z = a.zip3(&b, &c);
        let mut seen = z.generation();

        for write in [&c, &b, &a] {
            write.set(1);
            assert_ne!(z.generation(), seen);
            seen = z.generation();
        }
    }

    #[test]
    fn and_reads_logical_and() {
        let a = Signal::new(true);
        let b = Signal::new(false);
        let anded = a.and(&b);
        assert!(!anded.get());
        b.set(true);
        assert!(anded.get());
        a.set(false);
        assert!(!anded.get());
    }

    #[test]
    fn or_reads_logical_or() {
        let a = Signal::new(false);
        let b = Signal::new(false);
        let ored = a.or(&b);
        assert!(!ored.get());
        a.set(true);
        assert!(ored.get());
        a.set(false);
        b.set(true);
        assert!(ored.get());
    }

    #[test]
    fn not_reads_logical_negation() {
        let a = Signal::new(true);
        let n = a.not();
        assert!(!n.get());
        a.set(false);
        assert!(n.get());
    }

    #[test]
    fn combined_predicate_fires_binding_on_any_source() {
        use crate::binding::BindingRegistry;
        use slotmap::KeyData;

        let reg = BindingRegistry::new();
        let id: WidgetId = KeyData::from_ffi(1).into();

        let focus = Signal::new(false);
        let readonly = Signal::new(true);
        let in_editor = Signal::new(true);

        // Composite: focus && !readonly && in_editor — built with
        // combinators, bound to a widget at Relayout level.
        let when = focus.and(&readonly.not()).and(&in_editor);
        when.bind_to(id, &reg, BindingLevel::Relayout);
        assert!(!when.get(), "all sources start producing false");

        // Flip any source — the registry must see a dirty binding.
        focus.set(true);
        let dirty = reg.flush_dirty();
        assert_eq!(dirty.len(), 1, "focus change must fire the binding");
        assert_eq!(dirty[0].0, id);

        // Flip a different source — same widget, same outcome.
        readonly.set(false);
        let dirty = reg.flush_dirty();
        assert_eq!(dirty.len(), 1, "readonly change must fire the binding");
        // And the predicate now reads true.
        assert!(when.get());

        // Third source.
        in_editor.set(false);
        let dirty = reg.flush_dirty();
        assert_eq!(dirty.len(), 1, "in_editor change must fire the binding");
        assert!(!when.get());
    }

    #[test]
    fn zip_dedups_identical_source() {
        // `a.zip(&a.map(|v| v + 1))` shares one upstream mutable root.
        // The derived should register exactly one binding per widget,
        // not two.
        use crate::binding::BindingRegistry;
        use slotmap::KeyData;

        let reg = BindingRegistry::new();
        let id: WidgetId = KeyData::from_ffi(1).into();
        let a = Signal::new(0_i32);
        let derived = a.map(|v| v + 1);
        let z = a.zip(&derived);
        z.bind_to(id, &reg, BindingLevel::RepaintOnly);
        assert_eq!(
            reg.len(),
            1,
            "duplicate upstream root must register once, not twice"
        );
    }

    #[test]
    fn signal_animated_f32() {
        let s = Signal::<f32>::new_animated(0.0);
        assert!(!s.has_pending_animation());
        s.animate_to(
            100.0,
            std::time::Duration::from_millis(200),
            teksilo_tokens::Easing::Linear,
        );
        assert!(s.has_pending_animation());
        assert_eq!(s.animation_target(), Some(100.0));
        let req = s.take_pending_animation().unwrap();
        assert_eq!(req.target, 100.0);
        assert!(!s.has_pending_animation());
    }

    #[test]
    fn map_coalesced_collapses_multi_source_to_one_binding() {
        // A 4-source zip projects through map_coalesced; the
        // resulting derived signal exposes a single combined
        // DerivedSource. Verifies the source-count collapse.
        let a = Signal::new(1u32);
        let b = Signal::new(2u32);
        let c = Signal::new(3u32);
        let d = Signal::new(4u32);
        let composite = a
            .zip3(&b, &c)
            .zip(&d)
            .map_coalesced(|((x, y, z), w)| *x + *y + *z + *w);
        // Plain `map` would produce 4 sources; `map_coalesced` 1.
        assert_eq!(composite.as_sources().len(), 1);
        assert_eq!(composite.get(), 10);
        // Writing ANY underlying source advances the composite's single
        // generation, and it stays put in between.
        let mut seen = composite.generation();
        a.set(10);
        assert_ne!(composite.generation(), seen);
        seen = composite.generation();
        assert_eq!(composite.generation(), seen);
        c.set(30);
        assert_ne!(composite.generation(), seen);
    }

    /// The coalesced composite is one memo shared by every consumer of
    /// the derived signal — including two different windows' binding
    /// registries. Reading it must not consume it.
    #[test]
    fn map_coalesced_generation_is_readable_by_every_consumer() {
        let a = Signal::new(1u32);
        let b = Signal::new(2u32);
        let composite = a.zip(&b).map_coalesced(|(x, y)| *x + *y);

        let (window_a, window_b) = (composite.generation(), composite.generation());
        b.set(5);

        let a_now = composite.generation();
        assert_ne!(a_now, window_a);
        assert_ne!(
            composite.generation(),
            window_b,
            "the first read must not have cleared anything"
        );
        assert_eq!(composite.generation(), a_now);
    }

    #[test]
    fn map_coalesced_with_single_source_delegates_to_map() {
        // No coalescing benefit when there's only one source —
        // `map_coalesced` is equivalent to `map`.
        let a = Signal::new(7u32);
        let derived = a.map_coalesced(|v| *v * 2);
        assert_eq!(derived.get(), 14);
        assert_eq!(derived.as_sources().len(), 1);
    }

    #[test]
    fn reentrant_set_in_observer_does_not_panic() {
        // An observer that writes the same signal must not trip the inner
        // RefCell: no borrow is held while callbacks run. (Before the fix,
        // try_set held a shared borrow across the callback loop, so the
        // nested borrow_mut panicked with BorrowMutError.)
        let s = Signal::new(0_i32);
        let s2 = s.clone();
        let _handle = s.observe(move |v| {
            // Recurse exactly once — only re-enter while the value is 1.
            if *v == 1 {
                s2.set(2);
            }
        });
        s.set(1);
        assert_eq!(s.get(), 2);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "feedback loop")]
    fn unbounded_feedback_loop_panics_with_diagnostic() {
        // A's observer bumps B and B's observer bumps A, unconditionally, so
        // the cascade never settles. The debug depth guard must convert the
        // would-be stack overflow into an actionable panic.
        let a = Signal::new(0_i32);
        let b = Signal::new(0_i32);
        let b_for_a = b.clone();
        let _ha = a.observe(move |v| b_for_a.set(*v + 1));
        let a_for_b = a.clone();
        let _hb = b.observe(move |v| a_for_b.set(*v + 1));
        a.set(1);
    }

    #[test]
    fn detaching_observer_during_notification_does_not_panic() {
        use std::cell::RefCell;
        // Observer A drops observer B's handle when fired; B's remover takes
        // borrow_mut on the same inner. Must not panic now that no borrow is
        // held during the callback loop.
        let s = Signal::new(0_i32);
        let b_slot: Rc<RefCell<Option<ObserverHandle>>> = Rc::new(RefCell::new(None));
        let b_slot_for_a = b_slot.clone();
        let _a = s.observe(move |_| {
            b_slot_for_a.borrow_mut().take();
        });
        let b = s.observe(|_| {});
        *b_slot.borrow_mut() = Some(b);
        s.set(1);
    }

    /// The bug this exists to stop: `MenuEntry::enabled(..)` takes any
    /// `Prop<bool>`, so a caller reasonably passes `unsaved.and(&mode.not())`
    /// — and the macOS native-menu bridge, which pushes rather than polls,
    /// used to abort the process on it.
    #[test]
    fn observing_a_derived_signal_reports_every_root() {
        use std::cell::RefCell;

        let unsaved = Signal::new(false);
        let backup_mode = Signal::new(false);
        let can_save = unsaved.and(&backup_mode.not());

        let seen: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        let _h = can_save.observe(move |v| sink.borrow_mut().push(*v));

        unsaved.set(true);
        assert_eq!(*seen.borrow(), vec![true], "the first root pushes through");

        backup_mode.set(true);
        assert_eq!(
            *seen.borrow(),
            vec![true, false],
            "so does the second — a derived signal observes ALL its roots, \
             not just the one it was built from first"
        );
    }

    #[test]
    fn dropping_a_derived_observer_detaches_from_every_root() {
        use std::cell::RefCell;

        let a = Signal::new(0_i32);
        let b = Signal::new(0_i32);
        let sum = a.zip(&b).map(|(x, y)| *x + *y);

        let count: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));
        let sink = count.clone();
        let h = sum.observe(move |_| *sink.borrow_mut() += 1);

        a.set(1);
        b.set(1);
        assert_eq!(*count.borrow(), 2);
        assert_eq!(a.observer_count(), 1);
        assert_eq!(b.observer_count(), 1);

        drop(h);
        assert_eq!(a.observer_count(), 0, "no root keeps a stale observer");
        assert_eq!(b.observer_count(), 0);

        a.set(2);
        b.set(2);
        assert_eq!(*count.borrow(), 2, "and none of them still fires");
    }

    /// `flat_map` re-selects its inner signal as it is read, so there is no
    /// fixed root to attach to. It reports that rather than attaching an
    /// observer that would go quiet the moment the inner switched.
    #[test]
    fn observing_a_flat_mapped_signal_is_refused() {
        let which = Signal::new(0_usize);
        let inners = [Signal::new(1_i32), Signal::new(2_i32)];
        let picked = which.flat_map(move |i| inners[*i].clone());

        assert!(matches!(
            picked.try_observe(|_| {}),
            Err(SignalAccessError::ReadOnly)
        ));
    }

    /// `map_coalesced` folds a *fixed* set of roots into one source, so
    /// unlike `flat_map` it stays observable.
    #[test]
    fn observing_a_coalesced_signal_still_works() {
        use std::cell::RefCell;

        let x = Signal::new(0_i32);
        let y = Signal::new(0_i32);
        let both = x.zip(&y).map_coalesced(|(a, b)| *a + *b);

        let seen: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        let _h = both.observe(move |v| sink.borrow_mut().push(*v));

        x.set(3);
        y.set(4);
        assert_eq!(*seen.borrow(), vec![3, 7]);
    }

    #[test]
    fn registering_observer_during_notification_does_not_panic() {
        use std::cell::RefCell;
        // Registering a new observer mid-callback takes borrow_mut on the
        // same inner (via try_observe). Must not panic.
        let s = Signal::new(0_i32);
        let s2 = s.clone();
        let extra: Rc<RefCell<Option<ObserverHandle>>> = Rc::new(RefCell::new(None));
        let extra2 = extra.clone();
        let _h = s.observe(move |_| {
            if extra2.borrow().is_none() {
                *extra2.borrow_mut() = Some(s2.observe(|_| {}));
            }
        });
        s.set(1);
    }
}
