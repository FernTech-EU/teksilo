// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Extension methods on [`EventContext`] for showing and dismissing
//! toasts. Mirrors the `EventContextFileDialogExt` pattern from
//! `bastyde-platform`.
//!
//! Apps `use bastyde_widgets::toast::EventContextToastExt;` (or
//! `use bastyde::prelude::*;` once the umbrella re-exports it) and
//! then call `ctx.show_toast(toast)` / `ctx.dismiss_toast(handle)`
//! from any handler.

use bastyde_core::widget::EventContext;

use crate::toast::registry::ToastRegistry;
use crate::toast::{Toast, ToastHandle, ToastRoute};

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
    /// `ToastDismissCause::Programmatic`. Equivalent to
    /// `handle.dismiss(ctx)`. No-op if the toast has already been
    /// dismissed.
    fn dismiss_toast(&mut self, handle: &ToastHandle);
}

impl EventContextToastExt for EventContext<'_> {
    fn show_toast(&mut self, mut toast: Toast) -> ToastHandle {
        // Default routing: a toast presented with no explicit
        // `.target()` / `.broadcast()` is tagged with the presenting
        // window's id. This is the one join point that has both a
        // `Toast` and a real `EventContext` (hence a real window) —
        // it's what makes every pre-existing `Toast::info(...).present(ctx)`
        // call site in every app correct with zero changes: a
        // single-window app has exactly one window, so "origin
        // window" behaves identically to the old "one shared queue".
        if toast.target.is_none() {
            toast.target = self.window().map(|w| ToastRoute::Window(w.id()));
        }
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
/// to avoid adding a `log` dependency to bastyde-widgets — the registry
/// missing is a setup error rather than a runtime condition that
/// needs structured logging.)
fn warn_missing_install() {
    thread_local! {
        static WARNED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    WARNED.with(|w| {
        if !w.get() {
            eprintln!(
                "[bastyde-widgets::toast] ctx.show_toast(...) called without install_toast(opts) \
                 on the BastydeAppBuilder — the toast was dropped. See bastyde::install_toast \
                 or bastyde_widgets::toast docs."
            );
            w.set(true);
        }
    });
}
