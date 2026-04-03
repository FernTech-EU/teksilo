use std::cell::RefCell;
use std::rc::Rc;

use crate::widget_id::WidgetId;

// --- Binding registry (shared between State instances and WidgetTree) ---

/// Dirty-tracking granularity for a property binding.
/// Determined by the primitive widget implementor, not the consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingLevel {
    /// Visual-only change (color, opacity). Marks the widget for repaint;
    /// layout is skipped.
    RepaintOnly,
    /// Size-affecting change (text content, constraint value). Marks the widget
    /// for relayout and propagates upward through ancestors.
    Relayout,
}

/// A registered binding between a State and a widget property.
#[derive(Clone)]
pub(crate) struct Binding {
    /// Widget to mark dirty when the source state changes.
    pub widget_id: WidgetId,
    /// The dirty-tracking level for this binding.
    pub level: BindingLevel,
    /// Check if the source state is dirty.
    pub is_dirty: Rc<dyn Fn() -> bool>,
    /// Clear the dirty flag on the source state.
    pub clear_dirty: Rc<dyn Fn()>,
}

/// Shared registry of all active property bindings.
#[derive(Clone, Default)]
pub struct BindingRegistry {
    pub(crate) bindings: Rc<RefCell<Vec<Binding>>>,
}

impl BindingRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn register(&self, binding: Binding) {
        self.bindings.borrow_mut().push(binding);
    }

    /// Return widget IDs that need updating due to state changes,
    /// along with the maximum binding level for each widget.
    /// Clears the dirty flags.
    pub(crate) fn flush_dirty(&self) -> Vec<(WidgetId, BindingLevel)> {
        let bindings = self.bindings.borrow();
        let mut dirty_map: std::collections::HashMap<WidgetId, BindingLevel> =
            std::collections::HashMap::new();
        // Collect all dirty bindings first, then clear. Multiple bindings may
        // share the same underlying dirty flag (e.g. derived states from the
        // same source). Clearing immediately would cause later bindings to miss
        // the change.
        let mut to_clear: Vec<&Rc<dyn Fn()>> = Vec::new();
        for b in bindings.iter() {
            if (b.is_dirty)() {
                let entry = dirty_map.entry(b.widget_id).or_insert(b.level);
                if b.level == BindingLevel::Relayout {
                    *entry = BindingLevel::Relayout;
                }
                to_clear.push(&b.clear_dirty);
            }
        }
        // Now clear all dirty flags
        for clear in to_clear {
            clear();
        }
        dirty_map.into_iter().collect()
    }
}

impl std::fmt::Debug for BindingRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BindingRegistry")
            .field("count", &self.bindings.borrow().len())
            .finish()
    }
}

// --- ReadableState trait ---

/// A readable reactive value. Implemented by `State<T>` and `DerivedState<T>`.
pub trait ReadableState<T: Clone>: 'static {
    /// Read the current value (cloned).
    fn read(&self) -> T;
}

// --- StateHandle ---

/// A type-erased handle to a readable reactive state.
/// Cheap to clone (Rc inside). Carries optional dirty-tracking
/// closures for automatic binding registration.
pub struct StateHandle<T: Clone + 'static> {
    inner: Rc<dyn ReadableState<T>>,
    is_dirty: Option<Rc<dyn Fn() -> bool>>,
    clear_dirty: Option<Rc<dyn Fn()>>,
}

impl<T: Clone + 'static> StateHandle<T> {
    pub fn read(&self) -> T {
        self.inner.read()
    }

    /// Register this handle's dirty tracking for a widget at the given level.
    /// No-op if the handle has no dirty tracking (e.g., static).
    pub fn register(&self, widget_id: WidgetId, registry: &BindingRegistry, level: BindingLevel) {
        if let (Some(is_dirty), Some(clear_dirty)) =
            (self.is_dirty.clone(), self.clear_dirty.clone())
        {
            registry.register(Binding {
                widget_id,
                level,
                is_dirty,
                clear_dirty,
            });
        }
    }
}

impl<T: Clone + 'static> Clone for StateHandle<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            is_dirty: self.is_dirty.clone(),
            clear_dirty: self.clear_dirty.clone(),
        }
    }
}

impl<T: Clone + 'static> std::fmt::Debug for StateHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StateHandle(..)")
    }
}

// --- Reactive<T> ---

/// A property value that is either static or bound to a reactive state.
pub enum Reactive<T: Clone + 'static> {
    /// A fixed value, set at build time.
    Static(T),
    /// Bound to a reactive state; value is read lazily on each use.
    Bound(StateHandle<T>),
}

impl<T: Clone + 'static> Reactive<T> {
    /// Resolve the current value.
    pub fn get(&self) -> T {
        match self {
            Reactive::Static(v) => v.clone(),
            Reactive::Bound(handle) => handle.read(),
        }
    }

    /// Register dirty tracking for this reactive if it is bound.
    /// Called by Widget::register_bindings(). The `level` determines
    /// whether a state change triggers repaint-only or full relayout.
    pub fn register_if_bound(
        &self,
        widget_id: WidgetId,
        registry: &BindingRegistry,
        level: BindingLevel,
    ) {
        if let Reactive::Bound(handle) = self {
            handle.register(widget_id, registry, level);
        }
    }
}

impl<T: Clone + 'static> Clone for Reactive<T> {
    fn clone(&self) -> Self {
        match self {
            Reactive::Static(v) => Reactive::Static(v.clone()),
            Reactive::Bound(h) => Reactive::Bound(h.clone()),
        }
    }
}

impl<T: Clone + std::fmt::Debug + 'static> std::fmt::Debug for Reactive<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reactive::Static(v) => write!(f, "Static({:?})", v),
            Reactive::Bound(_) => f.write_str("Bound(..)"),
        }
    }
}

impl<T: Clone + 'static> From<T> for Reactive<T> {
    fn from(value: T) -> Self {
        Reactive::Static(value)
    }
}

impl<T: Clone + 'static> From<State<T>> for Reactive<T> {
    fn from(s: State<T>) -> Self {
        Reactive::Bound(StateHandle::from(s))
    }
}

impl<T: Clone + 'static> From<DerivedState<T>> for Reactive<T> {
    fn from(d: DerivedState<T>) -> Self {
        Reactive::Bound(StateHandle::from(d))
    }
}

impl<T: Clone + 'static> From<State<T>> for StateHandle<T> {
    fn from(s: State<T>) -> Self {
        let dirty_inner = s.inner.clone();
        let clear_inner = s.inner.clone();
        StateHandle {
            inner: Rc::new(s),
            is_dirty: Some(Rc::new(move || dirty_inner.borrow().dirty)),
            clear_dirty: Some(Rc::new(move || clear_inner.borrow_mut().dirty = false)),
        }
    }
}

impl<T: Clone + 'static> From<DerivedState<T>> for StateHandle<T> {
    fn from(d: DerivedState<T>) -> Self {
        let is_dirty = d.source_dirty.clone();
        let clear_dirty = d.source_clear.clone();
        StateHandle {
            inner: Rc::new(d),
            is_dirty,
            clear_dirty,
        }
    }
}

// --- State<T> ---

/// A reactive state value. When set, marks itself dirty.
/// Can be bound to widget properties via a BindingRegistry.
pub struct State<T> {
    inner: Rc<RefCell<StateInner<T>>>,
}

/// Opaque ID for an observer callback, used to remove it later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObserverId(u64);

struct ObserverEntry<T> {
    id: ObserverId,
    callback: Rc<dyn Fn(&T)>,
}

/// A pending animation request on a `State<f32>`.
#[derive(Debug, Clone)]
pub struct AnimationRequest {
    pub target: f32,
    pub duration: std::time::Duration,
    pub easing: fern_tokens::Easing,
}

struct StateInner<T> {
    value: T,
    dirty: bool,
    observers: Vec<ObserverEntry<T>>,
    next_observer_id: u64,
    /// Pending animation request (only meaningful for State<f32>).
    /// Picked up by the AnimationScheduler on the next frame.
    pending_animation: Option<AnimationRequest>,
}

impl<T: 'static> State<T> {
    pub fn new(value: T) -> Self {
        Self {
            inner: Rc::new(RefCell::new(StateInner {
                value,
                dirty: false,
                observers: Vec::new(),
                next_observer_id: 1,
                pending_animation: None,
            })),
        }
    }

    pub fn get(&self) -> std::cell::Ref<'_, T> {
        std::cell::Ref::map(self.inner.borrow(), |inner| &inner.value)
    }

    pub fn set(&self, value: T) {
        let mut inner = self.inner.borrow_mut();
        inner.value = value;
        inner.dirty = true;
        // Notify observers (clone the callback list to avoid borrow conflicts)
        let callbacks: Vec<_> = inner.observers.iter().map(|e| e.callback.clone()).collect();
        drop(inner);
        let inner_ref = self.inner.borrow();
        for cb in &callbacks {
            cb(&inner_ref.value);
        }
    }

    /// Register an observer callback. Called with a reference to the new value
    /// whenever `set()` is called. Returns an `ObserverId` for later removal.
    /// For application-layer notifications, not for widget bindings (use `bind_to` for those).
    pub fn observe(&self, callback: impl Fn(&T) + 'static) -> ObserverId {
        let mut inner = self.inner.borrow_mut();
        let id = ObserverId(inner.next_observer_id);
        inner.next_observer_id += 1;
        inner.observers.push(ObserverEntry {
            id,
            callback: Rc::new(callback),
        });
        id
    }

    /// Remove a previously registered observer by its ID.
    pub fn remove_observer(&self, id: ObserverId) {
        self.inner.borrow_mut().observers.retain(|e| e.id != id);
    }

    pub fn is_dirty(&self) -> bool {
        self.inner.borrow().dirty
    }

    pub fn clear_dirty(&self) {
        self.inner.borrow_mut().dirty = false;
    }

    /// Create a derived read-only state that transforms this state's value.
    pub fn map<U: 'static>(&self, f: impl Fn(&T) -> U + 'static) -> DerivedState<U> {
        let source = self.inner.clone();
        let dirty_source = self.inner.clone();
        let clear_source = self.inner.clone();
        DerivedState {
            compute: Rc::new(move || {
                let inner = source.borrow();
                f(&inner.value)
            }),
            source_dirty: Some(Rc::new(move || dirty_source.borrow().dirty)),
            source_clear: Some(Rc::new(move || clear_source.borrow_mut().dirty = false)),
        }
    }

    /// Bind this state to a widget at the given dirty-tracking level.
    pub fn bind_to(&self, widget_id: WidgetId, registry: &BindingRegistry, level: BindingLevel) {
        let inner = self.inner.clone();
        let inner2 = self.inner.clone();
        registry.register(Binding {
            widget_id,
            level,
            is_dirty: Rc::new(move || inner.borrow().dirty),
            clear_dirty: Rc::new(move || inner2.borrow_mut().dirty = false),
        });
    }
}

impl<T> State<T> {
    /// Check if two State handles point to the same underlying state.
    pub fn same(a: &State<T>, b: &State<T>) -> bool {
        Rc::ptr_eq(&a.inner, &b.inner)
    }
}

impl State<f32> {
    /// Animate this state smoothly from its current value to `target` over
    /// `duration` using `easing`. The animation is driven automatically by
    /// the framework — each frame interpolates the value and calls `set()`.
    ///
    /// If the state is already being animated, the previous animation is
    /// replaced (the current in-flight value becomes the new start).
    ///
    /// ```ignore
    /// sidebar_width.set_animated(0.0, Duration::from_millis(200), Easing::EaseInOut);
    /// ```
    pub fn set_animated(
        &self,
        target: f32,
        duration: std::time::Duration,
        easing: fern_tokens::Easing,
    ) {
        let mut inner = self.inner.borrow_mut();
        inner.pending_animation = Some(AnimationRequest {
            target,
            duration,
            easing,
        });
        inner.dirty = true; // trigger a frame so the scheduler picks this up
    }

    /// Take a pending animation request, if any. Called by the animation
    /// scheduler during its tick to start the animation.
    pub fn take_pending_animation(&self) -> Option<AnimationRequest> {
        self.inner.borrow_mut().pending_animation.take()
    }

    /// Whether there is a pending animation request.
    pub fn has_pending_animation(&self) -> bool {
        self.inner.borrow().pending_animation.is_some()
    }
}

impl<T: Clone + 'static> ReadableState<T> for State<T> {
    fn read(&self) -> T {
        self.get().clone()
    }
}

impl<T> Clone for State<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for State<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State")
            .field("value", &*self.inner.borrow())
            .finish()
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for StateInner<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateInner")
            .field("value", &self.value)
            .field("dirty", &self.dirty)
            .finish()
    }
}

// --- DerivedState<T> ---

/// A read-only derived state that computes its value from a source state.
pub struct DerivedState<T> {
    compute: Rc<dyn Fn() -> T>,
    /// Dirty check from the source State (if created via State::map).
    source_dirty: Option<Rc<dyn Fn() -> bool>>,
    /// Clear dirty on the source State.
    source_clear: Option<Rc<dyn Fn()>>,
}

impl<T> DerivedState<T> {
    pub fn get(&self) -> T {
        (self.compute)()
    }

    /// Bind this derived state's source to a widget at the given dirty-tracking level.
    pub fn bind_to(&self, widget_id: WidgetId, registry: &BindingRegistry, level: BindingLevel) {
        if let (Some(is_dirty), Some(clear_dirty)) =
            (self.source_dirty.clone(), self.source_clear.clone())
        {
            registry.register(Binding {
                widget_id,
                level,
                is_dirty,
                clear_dirty,
            });
        }
    }
}

impl<T: Clone + 'static> ReadableState<T> for DerivedState<T> {
    fn read(&self) -> T {
        self.get()
    }
}

impl<T> Clone for DerivedState<T> {
    fn clone(&self) -> Self {
        Self {
            compute: self.compute.clone(),
            source_dirty: self.source_dirty.clone(),
            source_clear: self.source_clear.clone(),
        }
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for DerivedState<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DerivedState")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_get_set() {
        let s = State::new(42);
        assert_eq!(*s.get(), 42);
        s.set(99);
        assert_eq!(*s.get(), 99);
    }

    #[test]
    fn state_dirty_tracking() {
        let s = State::new(0);
        assert!(!s.is_dirty());
        s.set(1);
        assert!(s.is_dirty());
        s.clear_dirty();
        assert!(!s.is_dirty());
    }

    #[test]
    fn derived_state_updates() {
        let text = State::new(String::new());
        let is_empty = text.map(|t| t.is_empty());
        assert!(is_empty.get());
        text.set("hello".to_string());
        assert!(!is_empty.get());
    }

    #[test]
    fn state_clone_shares() {
        let a = State::new(10);
        let b = a.clone();
        a.set(20);
        assert_eq!(*b.get(), 20);
    }

    #[test]
    fn binding_registry_tracks_dirty_widgets() {
        use slotmap::KeyData;
        let fake_id: WidgetId = KeyData::from_ffi(1).into();

        let registry = BindingRegistry::new();
        let s = State::new(0);
        s.bind_to(fake_id, &registry, BindingLevel::RepaintOnly);

        assert!(registry.flush_dirty().is_empty());

        s.set(42);
        let dirty = registry.flush_dirty();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].0, fake_id);
        assert_eq!(dirty[0].1, BindingLevel::RepaintOnly);

        assert!(registry.flush_dirty().is_empty());
    }

    // --- Reactive<T> tests ---

    #[test]
    fn reactive_static_returns_value() {
        let r: Reactive<i32> = Reactive::Static(42);
        assert_eq!(r.get(), 42);
    }

    #[test]
    fn reactive_bound_from_state_reads_current() {
        let s = State::new(10);
        let r: Reactive<i32> = s.clone().into();
        assert_eq!(r.get(), 10);
        s.set(20);
        assert_eq!(r.get(), 20);
    }

    #[test]
    fn reactive_bound_from_derived_reads_computed() {
        let s = State::new(5);
        let doubled = s.map(|v| v * 2);
        let r: Reactive<i32> = doubled.into();
        assert_eq!(r.get(), 10);
        s.set(7);
        assert_eq!(r.get(), 14);
    }

    #[test]
    fn state_handle_from_state() {
        let s = State::new(99);
        let h: StateHandle<i32> = s.clone().into();
        assert_eq!(h.read(), 99);
        s.set(100);
        assert_eq!(h.read(), 100);
    }

    #[test]
    fn state_handle_from_derived() {
        let s = State::new("hello".to_string());
        let len = s.map(|t| t.len());
        let h: StateHandle<usize> = len.into();
        assert_eq!(h.read(), 5);
        s.set("hi".to_string());
        assert_eq!(h.read(), 2);
    }

    #[test]
    fn derived_state_bind_to_marks_dirty() {
        use slotmap::KeyData;
        let fake_id: WidgetId = KeyData::from_ffi(1).into();

        let registry = BindingRegistry::new();
        let s = State::new(0);
        let doubled = s.map(|v| v * 2);

        // Bind the derived state (which delegates to source's dirty flag)
        doubled.bind_to(fake_id, &registry, BindingLevel::Relayout);

        assert!(registry.flush_dirty().is_empty());

        s.set(5);
        let dirty = registry.flush_dirty();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].0, fake_id);
        assert_eq!(dirty[0].1, BindingLevel::Relayout);
    }

    #[test]
    fn reactive_from_plain_value() {
        // From<T> conversion
        let r: Reactive<f32> = 3.14.into();
        assert!((r.get() - 3.14).abs() < 0.001);
    }

    #[test]
    fn observer_called_on_set() {
        use std::cell::Cell;
        let s = State::new(0);
        let called = Rc::new(Cell::new(false));
        let c = called.clone();
        s.observe(move |val| {
            assert_eq!(*val, 42);
            c.set(true);
        });
        s.set(42);
        assert!(called.get());
    }

    #[test]
    fn multiple_observers() {
        use std::cell::Cell;
        let s = State::new(0);
        let count = Rc::new(Cell::new(0));
        let c1 = count.clone();
        let c2 = count.clone();
        s.observe(move |_| c1.set(c1.get() + 1));
        s.observe(move |_| c2.set(c2.get() + 1));
        s.set(10);
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn flush_dirty_finds_all_bindings_from_same_source() {
        // Multiple derived states from the same source all share one dirty flag.
        // flush_dirty must detect ALL of them, not just the first.
        use slotmap::KeyData;
        let id_a: WidgetId = KeyData::from_ffi(1).into();
        let id_b: WidgetId = KeyData::from_ffi(2).into();
        let id_c: WidgetId = KeyData::from_ffi(3).into();

        let registry = BindingRegistry::new();
        let source = State::new(0);
        let derived_a = source.map(|v| v + 1);
        let derived_b = source.map(|v| v + 2);
        let derived_c = source.map(|v| v + 3);

        derived_a.bind_to(id_a, &registry, BindingLevel::RepaintOnly);
        derived_b.bind_to(id_b, &registry, BindingLevel::RepaintOnly);
        derived_c.bind_to(id_c, &registry, BindingLevel::RepaintOnly);

        source.set(42);
        let dirty = registry.flush_dirty();

        // All three widgets should be dirty, not just the first one
        let dirty_ids: std::collections::HashSet<WidgetId> =
            dirty.iter().map(|(id, _)| *id).collect();
        assert!(dirty_ids.contains(&id_a), "widget A missing from dirty set");
        assert!(dirty_ids.contains(&id_b), "widget B missing from dirty set");
        assert!(dirty_ids.contains(&id_c), "widget C missing from dirty set");
    }
}
