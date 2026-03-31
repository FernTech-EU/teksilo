//! Application-level events for cross-thread communication.
//!
//! Background threads post `AppEvent`s to the UI thread via an event loop
//! proxy. The UI thread processes them like any other input event.

use std::any::Any;

use crate::app_command::ErasedCommand;

/// Events posted to the UI thread from background threads or timers.
#[derive(Debug)]
pub enum AppEvent {
    /// A typed application command (same as widget-emitted commands).
    Command(ErasedCommand),

    /// A background operation completed.
    BackgroundComplete {
        operation_id: String,
    },

    /// A background operation reports progress.
    BackgroundProgress {
        operation_id: String,
        percent: f32,
        message: String,
    },

    /// An external event from any source (type-erased for extensibility).
    External(Box<dyn Any + Send>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_event_command_variant() {
        use crate::app_command::AppCommand;

        #[derive(Debug, Clone)]
        struct TestCmd;
        impl AppCommand for TestCmd {}

        let event = AppEvent::Command(ErasedCommand::new(TestCmd));
        assert!(matches!(event, AppEvent::Command(_)));
    }

    #[test]
    fn app_event_background_complete() {
        let event = AppEvent::BackgroundComplete {
            operation_id: "export-123".to_string(),
        };
        if let AppEvent::BackgroundComplete { operation_id } = event {
            assert_eq!(operation_id, "export-123");
        } else {
            panic!("Expected BackgroundComplete");
        }
    }

    #[test]
    fn app_event_background_progress() {
        let event = AppEvent::BackgroundProgress {
            operation_id: "export-123".to_string(),
            percent: 0.5,
            message: "Halfway done".to_string(),
        };
        if let AppEvent::BackgroundProgress { percent, .. } = event {
            assert!((percent - 0.5).abs() < 0.001);
        } else {
            panic!("Expected BackgroundProgress");
        }
    }

    #[test]
    fn app_event_external_any() {
        let event = AppEvent::External(Box::new(42i32));
        if let AppEvent::External(payload) = event {
            assert_eq!(*payload.downcast_ref::<i32>().unwrap(), 42);
        } else {
            panic!("Expected External");
        }
    }
}
