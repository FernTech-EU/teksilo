use std::rc::Rc;

use crate::signal::{Prop, Signal};
use crate::state::{DerivedState, ReadableState, State, StateHandle};

struct SignalReader<T: Clone + 'static>(Signal<T>);

impl<T: Clone + 'static> ReadableState<T> for SignalReader<T> {
    fn read(&self) -> T {
        self.0.get()
    }
}

pub(crate) fn state_handle_from_signal<T: Clone + 'static>(signal: Signal<T>) -> StateHandle<T> {
    let dirty_signal = signal.clone();
    let clear_signal = signal.clone();
    StateHandle::new_with_tracking(
        Rc::new(SignalReader(signal)),
        Some(Rc::new(move || dirty_signal.is_dirty())),
        Some(Rc::new(move || clear_signal.clear_dirty())),
    )
}

impl<T: Clone + 'static> From<State<T>> for Prop<T> {
    fn from(state: State<T>) -> Self {
        Prop::Bound(Signal::from(state))
    }
}

impl<T: Clone + 'static> From<DerivedState<T>> for Prop<T> {
    fn from(derived: DerivedState<T>) -> Self {
        Prop::Bound(Signal::from(derived))
    }
}
