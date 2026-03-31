use std::any::Any;
use std::fmt::Debug;

/// Trait bound for application-defined command enums.
pub trait AppCommand: 'static + Send + Clone + Debug {}

/// Type-erased command wrapper for storing commands in the widget tree.
pub struct ErasedCommand(Box<dyn Any + Send>);

impl ErasedCommand {
    pub fn new<C: AppCommand>(cmd: C) -> Self {
        Self(Box::new(cmd))
    }

    pub fn downcast_ref<C: AppCommand>(&self) -> Option<&C> {
        self.0.downcast_ref::<C>()
    }
}

impl Debug for ErasedCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ErasedCommand")
    }
}
