// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Application-level events for cross-thread communication.
//!
//! Background threads post `AppEvent`s to the UI thread via an event loop
//! proxy. The UI thread processes them like any other input event.

use std::any::Any;
use std::path::PathBuf;

use crate::event_source::SubscriptionId;

/// Events posted to the UI thread from background threads or timers.
pub enum AppEvent {
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
    /// The bastyde-app handler calls `I18nManager::reload_from_path` and
    /// bumps the translation version signal.
    I18nReload { locale: String, path: PathBuf },

    /// A settings file managed by `bastyde-settings` changed on disk —
    /// either a peer process's write, or (harmlessly) this very
    /// process's own write being noticed by its own watcher. The
    /// bastyde-app handler looks `path` up in the app's
    /// `bastyde_settings::SettingsRegistry` (via `app_state`) and calls
    /// `Reloadable::reload_from_disk` on whatever owns it — a no-op if
    /// nothing is registered for that path, or if the content is
    /// unchanged (the self-write case).
    SettingsReload { path: PathBuf },

    /// A `bastyde-settings` `DebouncedWriter` gave up on a queued write —
    /// after `MAX_WRITE_ATTEMPTS` retries, or at `Unregister` teardown
    /// with a write still failing. The queued patches for `path` were
    /// permanently discarded.
    SettingsWriteFailed {
        path: PathBuf,
        attempts: u32,
        dropped_patches: usize,
        message: String,
    },
}

/// A generic, thread-safe "please repaint this window now" request, posted as
/// an [`AppEvent::External`] payload from a background thread via
/// [`AppEventPoster::post_external`](crate::AppEventPoster::post_external).
///
/// A bare redraw request re-presents each node's cached paint frame, so a
/// widget whose content changed **off the UI thread** — a terminal emulator's
/// PTY-reader thread, a video decoder, a streaming data source — would not have
/// its `paint()` re-run. bastyde-app routes this request by marking the named
/// window's tree paint-dirty ([`WidgetTree::mark_all_needs_paint_only`](crate::widget_tree::WidgetTree::mark_all_needs_paint_only))
/// before the redraw, so the changed widget repaints. It is the off-thread
/// analogue of `ctx.request_frame()` (which is UI-thread only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepaintWindowRequest {
    /// The window whose tree should be marked paint-dirty and redrawn.
    pub window_id: crate::window::BastydeWindowId,
}

impl std::fmt::Debug for AppEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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
            Self::SettingsReload { path } => f
                .debug_struct("SettingsReload")
                .field("path", path)
                .finish(),
            Self::SettingsWriteFailed {
                path,
                attempts,
                dropped_patches,
                message,
            } => f
                .debug_struct("SettingsWriteFailed")
                .field("path", path)
                .field("attempts", attempts)
                .field("dropped_patches", dropped_patches)
                .field("message", message)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
