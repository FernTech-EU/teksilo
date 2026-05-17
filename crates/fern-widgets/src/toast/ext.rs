//! Extension methods on [`EventContext`] for showing and dismissing
//! toasts. Mirrors the [`EventContextFileDialogExt`] pattern from
//! `fern-platform`.
//!
//! Apps `use fern_widgets::toast::EventContextToastExt;` (or
//! `use fern_ui::prelude::*;` once the umbrella re-exports it) and
//! then call `ctx.show_toast(toast)` / `ctx.dismiss_toast(handle)`
//! from any handler.

use fern_core::widget::EventContext;

use crate::toast::registry::ToastRegistry;
use crate::toast::{Toast, ToastHandle};

/// Convenience methods on [`EventContext`] for the toast system. The
/// registry is looked up via [`EventContext::app_state`] — the
/// `install_toast` extension trait registers `ToastRegistry` there
/// at app boot.
///
/// All methods are no-ops (returning a dropped [`ToastHandle`] where
/// applicable) when `install_toast` was not called — a one-shot
/// `log::warn!` fires the first time the missing registration is
/// detected, so missing installs surface in logs without crashing
/// app code that defensively calls `show_toast`.
pub trait EventContextToastExt {
    /// Present a [`Toast`] through the installed
    /// [`ToastHost`](crate::toast::host::ToastHost). Returns a
    /// [`ToastHandle`] for programmatic control.
    fn show_toast(&mut self, toast: Toast) -> ToastHandle;

    /// Programmatically dismiss a toast by handle, with cause
    /// [`ToastDismissCause::Programmatic`]. Equivalent to
    /// `handle.dismiss(ctx)`. No-op if the toast has already been
    /// dismissed.
    fn dismiss_toast(&mut self, handle: &ToastHandle);
}

impl EventContextToastExt for EventContext<'_> {
    fn show_toast(&mut self, toast: Toast) -> ToastHandle {
        let Some(registry) = self.app_state::<ToastRegistry>().cloned() else {
            warn_missing_install();
            // Return a dropped handle — `is_alive` returns false,
            // `dismiss` is a no-op. The caller's `on_dismiss`
            // callback (if any) is silently dropped along with the
            // toast.
            return ToastHandle::new(crate::toast::ToastHandleInner {
                entry_id: 0,
                dismissed: std::cell::Cell::new(true),
                registry: ToastRegistry::new(crate::toast::host::ToastInstallOptions::default()),
            });
        };
        let (handle, overflow_callback) = registry.enqueue(toast);
        // Slot-pool overflow: fire on_dismiss synchronously with the
        // handler context the caller is in.
        if let Some((cause, cb)) = overflow_callback {
            cb(cause, self);
        }
        handle
    }

    fn dismiss_toast(&mut self, handle: &ToastHandle) {
        // Delegate to the handle's own dismiss path.
        let handle = handle.clone();
        handle.dismiss(self);
    }
}

/// One-shot stderr warning when `show_toast` is called without
/// `install_toast`. Uses a thread-local flag so noisy callers in a
/// tight loop don't spam the output. (Stderr rather than `log::warn!`
/// to avoid adding a `log` dependency to fern-widgets — the registry
/// missing is a setup error rather than a runtime condition that
/// needs structured logging.)
fn warn_missing_install() {
    thread_local! {
        static WARNED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    WARNED.with(|w| {
        if !w.get() {
            eprintln!(
                "[fern-widgets::toast] ctx.show_toast(...) called without install_toast(opts) \
                 on the FernAppBuilder — the toast was dropped. See fern_ui::install_toast \
                 or fern_widgets::toast docs."
            );
            w.set(true);
        }
    });
}
