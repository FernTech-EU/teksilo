// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! App-wide composable `AppEvent` observers.
//!
//! `TeksiloAppBuilder::on_app_event` is a single `Option<Box<dyn
//! FnMut(&AppEvent)>>` slot on the builder — a second call **replaces**
//! the first. That's fine for application code (an app registers at
//! most one handler), but it means a *framework* extension (an
//! `install_*` helper in `teksilo` or a sibling crate) cannot safely
//! register its own `AppEvent` reaction: it would either clobber the
//! app's handler, or be clobbered by it, depending on install order.
//!
//! `AppEventObservers` fixes this exactly the way
//! [`DefaultPostRoot`](crate::DefaultPostRoot) fixes the analogous
//! problem for post-root window chrome: it's a single type-keyed
//! `app_state` slot, but `TeksiloAppBuilder::register_app_event_observer`
//! *composes* into it instead of replacing it, so every registered
//! observer runs. Observers run in addition to — never instead of — the
//! single `on_app_event` handler; see `TeksiloAppHandler::user_event`
//! where both are dispatched.

use std::rc::Rc;

use teksilo_core::app_event::AppEvent;

/// Closure invoked with every `AppEvent` the UI thread receives, in
/// addition to the app's own `on_app_event` handler (if any). Stored in
/// `app_state` so it composes across every extension that registers
/// one via [`TeksiloAppBuilder::register_app_event_observer`](crate::TeksiloAppBuilder::register_app_event_observer).
#[derive(Clone)]
pub struct AppEventObservers(pub Rc<dyn Fn(&AppEvent)>);

impl AppEventObservers {
    pub fn new(f: impl Fn(&AppEvent) + 'static) -> Self {
        Self(Rc::new(f))
    }
}

impl std::fmt::Debug for AppEventObservers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("AppEventObservers")
            .field(&"<closure>")
            .finish()
    }
}
