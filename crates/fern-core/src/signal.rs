//! Unified reactivity primitives for FernUI.
//!
//! `Signal<T>` is the single reactive type. `Prop<T>` is the widget
//! property type for static values and signal bindings. `ObserverHandle`
//! is an RAII guard — dropping it removes the observer callback.

use std::cell::{Ref, RefCell};
use std::rc::{Rc, Weak};

use crate::binding::{Binding, BindingLevel, BindingRegistry};
use crate::widget_id::WidgetId;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalAccessError {
    ReadOnly,
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

enum SignalKind<T> {
    Mutable {
        inner: Rc<RefCell<MutableInner<T>>>,
        animation: Option<Rc<RefCell<AnimationState>>>,
    },
    Derived {
        compute: Rc<dyn Fn() -> T>,
        source_dirty: Rc<dyn Fn() -> bool>,
        source_clear: Rc<dyn Fn()>,
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

    /// Set a new value. Marks the signal as dirty and notifies observers.
    /// Panics if called on a derived (read-only) signal.
    pub fn set(&self, value: T) {
        self.try_set(value)
            .expect("cannot set() on a derived Signal — it is read-only");
    }

    pub fn try_set(&self, value: T) -> Result<(), SignalAccessError> {
        match &self.kind {
            SignalKind::Mutable { inner, .. } => {
                let mut guard = inner.borrow_mut();
                guard.value = value;
                guard.dirty = true;
                let callbacks: Vec<_> =
                    guard.observers.iter().map(|e| e.callback.clone()).collect();
                drop(guard);
                let guard = inner.borrow();
                for cb in &callbacks {
                    cb(&guard.value);
                }
                Ok(())
            }
            SignalKind::Derived { .. } => Err(SignalAccessError::ReadOnly),
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
        match &self.kind {
            SignalKind::Mutable { inner, .. } => {
                let source = inner.clone();
                let dirty_source = inner.clone();
                let clear_source = inner.clone();
                Signal {
                    kind: SignalKind::Derived {
                        compute: Rc::new(move || {
                            let guard = source.borrow();
                            f(&guard.value)
                        }),
                        source_dirty: Rc::new(move || dirty_source.borrow().dirty),
                        source_clear: Rc::new(move || clear_source.borrow_mut().dirty = false),
                    },
                }
            }
            SignalKind::Derived {
                compute,
                source_dirty,
                source_clear,
            } => {
                let parent_compute = compute.clone();
                let sd = source_dirty.clone();
                let sc = source_clear.clone();
                Signal {
                    kind: SignalKind::Derived {
                        compute: Rc::new(move || {
                            let val = parent_compute();
                            f(&val)
                        }),
                        source_dirty: sd,
                        source_clear: sc,
                    },
                }
            }
        }
    }

    /// Bind this signal to a widget at the given dirty-tracking level.
    pub fn bind_to(&self, widget_id: WidgetId, registry: &BindingRegistry, level: BindingLevel) {
        let (is_dirty, clear_dirty) = self.dirty_fns();
        if let (Some(is_dirty), Some(clear_dirty)) = (is_dirty, clear_dirty) {
            registry.register(Binding {
                widget_id,
                level,
                is_dirty,
                clear_dirty,
            });
        }
    }

    #[allow(clippy::type_complexity)]
    fn dirty_fns(&self) -> (Option<Rc<dyn Fn() -> bool>>, Option<Rc<dyn Fn()>>) {
        match &self.kind {
            SignalKind::Mutable { inner, .. } => {
                let dirty_inner = inner.clone();
                let clear_inner = inner.clone();
                (
                    Some(Rc::new(move || dirty_inner.borrow().dirty)),
                    Some(Rc::new(move || clear_inner.borrow_mut().dirty = false)),
                )
            }
            SignalKind::Derived {
                source_dirty,
                source_clear,
                ..
            } => (Some(source_dirty.clone()), Some(source_clear.clone())),
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
            SignalKind::Derived { source_dirty, .. } => source_dirty(),
        }
    }

    pub fn clear_dirty(&self) {
        match &self.kind {
            SignalKind::Mutable { inner, .. } => inner.borrow_mut().dirty = false,
            SignalKind::Derived { source_clear, .. } => source_clear(),
        }
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
        easing: fern_tokens::Easing,
    ) {
        self.animate_to_with_frame_interval(target, duration, easing, None);
    }

    pub fn animate_to_with_frame_interval(
        &self,
        target: f32,
        duration: std::time::Duration,
        easing: fern_tokens::Easing,
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
        easing: fern_tokens::Easing,
    ) -> Result<(), SignalAccessError> {
        self.try_animate_to_with_frame_interval(target, duration, easing, None)
    }

    pub fn try_animate_to_with_frame_interval(
        &self,
        target: f32,
        duration: std::time::Duration,
        easing: fern_tokens::Easing,
        frame_interval: Option<std::time::Duration>,
    ) -> Result<(), SignalAccessError> {
        match &self.kind {
            SignalKind::Mutable {
                inner,
                animation: Some(animation),
            } => {
                let mut anim = animation.borrow_mut();
                anim.pending = Some(crate::animation::AnimationRequest {
                    target,
                    duration,
                    easing,
                    frame_interval,
                    looping: false,
                });
                anim.target = Some(target);
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

    /// Start a looping animation from the current value to `target`,
    /// repeating with the given period. Runs until cancelled.
    /// Frame updates are capped at `frame_interval` (default ~30fps).
    pub fn animate_looping(
        &self,
        target: f32,
        period: std::time::Duration,
        easing: fern_tokens::Easing,
        frame_interval: Option<std::time::Duration>,
    ) {
        if let SignalKind::Mutable {
            inner,
            animation: Some(animation),
        } = &self.kind
        {
            let mut anim = animation.borrow_mut();
            anim.pending = Some(crate::animation::AnimationRequest {
                target,
                duration: period,
                easing,
                frame_interval,
                looping: true,
            });
            anim.target = Some(target);
            drop(anim);
            inner.borrow_mut().dirty = true;
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
                SignalKind::Derived {
                    compute,
                    source_dirty,
                    source_clear,
                } => SignalKind::Derived {
                    compute: compute.clone(),
                    source_dirty: source_dirty.clone(),
                    source_clear: source_clear.clone(),
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

    #[test]
    fn signal_animated_f32() {
        let s = Signal::<f32>::new_animated(0.0);
        assert!(!s.has_pending_animation());
        s.animate_to(
            100.0,
            std::time::Duration::from_millis(200),
            fern_tokens::Easing::Linear,
        );
        assert!(s.has_pending_animation());
        assert_eq!(s.animation_target(), Some(100.0));
        let req = s.take_pending_animation().unwrap();
        assert_eq!(req.target, 100.0);
        assert!(!s.has_pending_animation());
    }

}
