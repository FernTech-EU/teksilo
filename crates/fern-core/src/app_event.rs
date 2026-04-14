//! Application-level events for cross-thread communication.
//!
//! Background threads post `AppEvent`s to the UI thread via an event loop
//! proxy. The UI thread processes them like any other input event.

use std::any::Any;
use std::path::PathBuf;

use crate::app_command::ErasedCommand;
use crate::event_source::SubscriptionId;

/// Events posted to the UI thread from background threads or timers.
pub enum AppEvent {
    /// A typed application command (same as widget-emitted commands).
    Command(ErasedCommand),

    /// A background operation completed.
    BackgroundComplete { operation_id: String },

    /// A background operation reports progress.
    BackgroundProgress {
        operation_id: String,
        percent: f32,
        message: String,
    },

    /// An external event from any source (type-erased for extensibility).
    External(Box<dyn Any + Send>),

    /// A backend event delivered to a widget that subscribed via
    /// `BuildContext::subscribe_event`. The `sub_id` keys the UI-side
    /// callback in the tree's `TreeAppContext::subscription_callbacks` map;
    /// the `event` is downcast back to the subscriber's expected type.
    SubscriptionEvent {
        sub_id: SubscriptionId,
        event: Box<dyn Any + Send>,
    },

    /// An `.ftl` translation file registered via
    /// `I18nConfig::runtime_override(locale, path)` changed on disk.
    /// The fern-app handler calls `I18nManager::reload_from_path` and
    /// bumps the translation version signal. Architecture §12.6.
    I18nReload {
        locale: String,
        path: PathBuf,
    },
}

impl std::fmt::Debug for AppEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command(cmd) => f.debug_tuple("Command").field(cmd).finish(),
            Self::BackgroundComplete { operation_id } => f
                .debug_struct("BackgroundComplete")
                .field("operation_id", operation_id)
                .finish(),
            Self::BackgroundProgress {
                operation_id,
                percent,
                message,
            } => f
                .debug_struct("BackgroundProgress")
                .field("operation_id", operation_id)
                .field("percent", percent)
                .field("message", message)
                .finish(),
            Self::External(_) => f.debug_tuple("External").field(&"..").finish(),
            Self::SubscriptionEvent { sub_id, .. } => f
                .debug_struct("SubscriptionEvent")
                .field("sub_id", sub_id)
                .field("event", &"..")
                .finish(),
            Self::I18nReload { locale, path } => f
                .debug_struct("I18nReload")
                .field("locale", locale)
                .field("path", path)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_event_command_variant() {
        use crate::app_command::AppCommand;

        #[derive(Debug, Clone, PartialEq)]
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
