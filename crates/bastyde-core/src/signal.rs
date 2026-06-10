//! Unified reactivity primitives for Bastyde.
//!
//! `Signal<T>` is the single reactive type. `Prop<T>` is the widget
//! property type for static values and signal bindings. `ObserverHandle`
//! is an RAII guard — dropping it removes the observer callback.

use std::cell::{Ref, RefCell};
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

struct MutableInner<T> {
    value: T,
    dirty: bool,
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
/// carry one per observed mutable root. Dirty-tracking walks the whole
/// vec: `is_dirty` is the OR, `clear_dirty` iterates all clears.
#[derive(Clone)]
struct DerivedSource {
    dirty: Rc<dyn Fn() -> bool>,
    clear: Rc<dyn Fn()>,
    /// Stable identity of the upstream mutable root — used by
    /// [`BindingRegistry`] to dedup repeated `bind_to` calls.
    source_id: usize,
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
                    dirty: false,
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
    pub fn observe(&self, f: impl Fn(&T) + 'static) -> ObserverHandle {
        self.try_observe(f)
            .expect("observe() is only supported on mutable signals")
    }

    pub fn try_observe(
        &self,
        f: impl Fn(&T) + 'static,
    ) -> Result<ObserverHandle, SignalAccessError> {
        match &self.kind {
            SignalKind::Mutable { inner, .. } => {
                let mut guard = inner.borrow_mut();
                let id = guard.next_observer_id;
                guard.next_observer_id += 1;
                guard.observers.push(ObserverEntry {
                    id,
                    callback: Rc::new(f),
                });
                Ok(ObserverHandle {
                    _signal: inner.clone(),
                    observer_id: id,
                    remover: {
                        let inner = inner.clone();
                        Rc::new(move |observer_id| {
                            inner.borrow_mut().observers.retain(|e| e.id != observer_id);
                        })
                    },
                })
            }
            SignalKind::Derived { .. } => Err(SignalAccessError::ReadOnly),
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

    /// Fallible [`set`](Self::set): returns [`SignalAccessError::ReadOnly`]
    /// for a derived signal instead of panicking.
    ///
    /// The new value and the observer callbacks are snapshotted while the
    /// inner `RefCell` is borrowed, then **all** borrows are released before
    /// any callback runs. An observer is therefore free to re-enter
    /// `set`/`try_set`/`observe`, or drop an `ObserverHandle`, on this same
    /// signal without a `RefCell` borrow conflict — mirroring the
    /// mutate-then-notify discipline used across `bastyde-data`. Each
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
                    guard.dirty = true;
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
    /// # use bastyde_core::Signal;
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
        // The token anchors a unique heap address used as `source_id`.
        // Each closure captures a clone so the Rc lives as long as the
        // resulting signal's source vec.
        let token: Rc<()> = Rc::new(());
        let source_id = Rc::as_ptr(&token) as usize;
        let dirty_token = token.clone();
        let dirty_underlying = underlying.clone();
        let clear_token = token;
        let clear_underlying = underlying;
        let coalesced = DerivedSource {
            dirty: Rc::new(move || {
                let _keep = &dirty_token;
                dirty_underlying.iter().any(|s| (s.dirty)())
            }),
            clear: Rc::new(move || {
                let _keep = &clear_token;
                for s in &clear_underlying {
                    (s.clear)();
                }
            }),
            source_id,
        };
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
    /// # use bastyde_core::Signal;
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

        // One composite source: dirty when the outer sources are dirty OR
        // the *current* inner signal's sources are dirty. The token anchors
        // a unique heap address used as the stable `source_id`.
        let token: Rc<()> = Rc::new(());
        let source_id = Rc::as_ptr(&token) as usize;

        let dirty: Rc<dyn Fn() -> bool> = {
            let token = token.clone();
            let f = f.clone();
            let outer_compute = outer_compute.clone();
            let outer_sources = outer_sources.clone();
            Rc::new(move || {
                let _keep = &token;
                outer_sources.iter().any(|s| (s.dirty)()) || f(&outer_compute()).is_dirty()
            })
        };
        let clear: Rc<dyn Fn()> = {
            let f = f.clone();
            let outer_compute = outer_compute.clone();
            Rc::new(move || {
                let _keep = &token;
                for s in &outer_sources {
                    (s.clear)();
                }
                f(&outer_compute()).clear_dirty();
            })
        };

        Signal {
            kind: SignalKind::Derived {
                compute,
                sources: vec![DerivedSource {
                    dirty,
                    clear,
                    source_id,
                }],
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
                is_dirty: src.dirty,
                clear_dirty: src.clear,
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
                let dirty_src = inner.clone();
                let clear_src = inner.clone();
                let source_id = Rc::as_ptr(inner) as *const () as usize;
                vec![DerivedSource {
                    dirty: Rc::new(move || dirty_src.borrow().dirty),
                    clear: Rc::new(move || clear_src.borrow_mut().dirty = false),
                    source_id,
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

    pub fn is_dirty(&self) -> bool {
        match &self.kind {
            SignalKind::Mutable { inner, .. } => inner.borrow().dirty,
            SignalKind::Derived { sources, .. } => sources.iter().any(|s| (s.dirty)()),
        }
    }

    pub fn clear_dirty(&self) {
        match &self.kind {
            SignalKind::Mutable { inner, .. } => inner.borrow_mut().dirty = false,
            SignalKind::Derived { sources, .. } => {
                for s in sources {
                    (s.clear)();
                }
            }
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
                    dirty: false,
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
        easing: bastyde_tokens::Easing,
    ) {
        self.animate_to_with_frame_interval(target, duration, easing, None);
    }

    pub fn animate_to_with_frame_interval(
        &self,
        target: f32,
        duration: std::time::Duration,
        easing: bastyde_tokens::Easing,
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
        easing: bastyde_tokens::Easing,
    ) -> Result<(), SignalAccessError> {
        self.try_animate_to_with_frame_interval(target, duration, easing, None)
    }

    pub fn try_animate_to_with_frame_interval(
        &self,
        target: f32,
        duration: std::time::Duration,
        easing: bastyde_tokens::Easing,
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
        easing: bastyde_tokens::Easing,
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
                inner.borrow_mut().dirty = true;
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
                .field("dirty", &inner.borrow().dirty)
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
    fn signal_dirty_tracking() {
        let s = Signal::new(0);
        assert!(!s.is_dirty());
        s.set(1);
        assert!(s.is_dirty());
        s.clear_dirty();
        assert!(!s.is_dirty());
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
    fn signal_derived_dirty_tracks_source() {
        let s = Signal::new(0);
        let derived = s.map(|v| v + 1);
        assert!(!derived.is_dirty());
        s.set(5);
        assert!(derived.is_dirty());
        derived.clear_dirty();
        assert!(!derived.is_dirty());
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
    fn flat_map_dirty_tracks_outer_and_current_inner() {
        let a = Signal::new(0);
        let b = Signal::new(0);
        let which = Signal::new(0usize);
        let (a2, b2) = (a.clone(), b.clone());
        let out = which.flat_map(move |i| if *i == 0 { a2.clone() } else { b2.clone() });
        out.clear_dirty();
        assert!(!out.is_dirty());

        // The current inner (a) flipping makes the result dirty.
        a.set(5);
        assert!(out.is_dirty());
        out.clear_dirty();
        assert!(!out.is_dirty());

        // The non-selected inner (b) flipping does NOT.
        b.set(7);
        assert!(!out.is_dirty());

        // The outer selector flipping makes the result dirty.
        which.set(1);
        assert!(out.is_dirty());
        out.clear_dirty();

        // Now b is selected, so b flipping is tracked and a is ignored.
        b.set(8);
        assert!(out.is_dirty());
        out.clear_dirty();
        a.set(9);
        assert!(!out.is_dirty());
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
    fn zip_is_dirty_when_either_source_dirty() {
        let a = Signal::new(0_i32);
        let b = Signal::new(0_i32);
        let z = a.zip(&b);
        assert!(!z.is_dirty());

        a.set(1);
        assert!(z.is_dirty(), "dirty from first source must propagate");
        z.clear_dirty();
        assert!(!z.is_dirty());

        b.set(2);
        assert!(z.is_dirty(), "dirty from second source must propagate");
        z.clear_dirty();
        assert!(!z.is_dirty());
    }

    #[test]
    fn zip_clear_dirty_clears_all_sources() {
        let a = Signal::new(0_i32);
        let b = Signal::new(0_i32);
        let z = a.zip(&b);
        a.set(1);
        b.set(1);
        assert!(a.is_dirty() && b.is_dirty() && z.is_dirty());
        z.clear_dirty();
        assert!(!a.is_dirty());
        assert!(!b.is_dirty());
        assert!(!z.is_dirty());
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
    fn zip3_is_dirty_when_any_source_dirty() {
        let a = Signal::new(0_i32);
        let b = Signal::new(0_i32);
        let c = Signal::new(0_i32);
        let z = a.zip3(&b, &c);

        c.set(99);
        assert!(z.is_dirty());
        z.clear_dirty();

        b.set(7);
        assert!(z.is_dirty());
        z.clear_dirty();

        a.set(1);
        assert!(z.is_dirty());
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
            bastyde_tokens::Easing::Linear,
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
        // Dirtying any underlying source flips the composite's
        // dirty bit; clearing the composite clears all underlying.
        assert!(!composite.is_dirty());
        a.set(10);
        assert!(composite.is_dirty());
        composite.clear_dirty();
        assert!(!composite.is_dirty());
        assert!(!a.is_dirty());
        // Setting another underlying after clear should re-dirty.
        c.set(30);
        assert!(composite.is_dirty());
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
