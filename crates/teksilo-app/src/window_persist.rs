// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Auto save / restore of window geometry, driven by the
//! [`WindowStateService`] registered
//! via `TeksiloAppBuilder::settings(...)`.
//!
//! Two integration points run inside `WindowManager::create_window`:
//!
//! 1. **Restore** — before the winit window is built, the saved
//!    [`PerWindowState`] (if any) is
//!    sanitized against the current monitor and applied to the
//!    [`WindowConfig`]. A coordinate from a now-disconnected monitor
//!    is recentered onto the current primary; an oversized rectangle
//!    is clamped down. See
//!    [`PerWindowState::sanitize`](teksilo_settings::PerWindowState::sanitize).
//! 2. **Save** — after [`WindowState`] is constructed, observers on
//!    the `size`, `position`, and `placement` signals push the
//!    current geometry into the service on every change. The
//!    debounced writer coalesces rapid drag bursts into one disk
//!    write.
//!
//! Auto-persistence is a strict opt-in: a window only participates
//! when its [`WindowConfig`] has a non-empty `id(...)` *and* a
//! `WindowStateService` is registered. Modal dialogs and ephemeral
//! popovers are thus naturally excluded — they don't carry stable ids.

use teksilo_core::ObserverHandle;
use teksilo_core::{Signal, WindowConfig, WindowState};
use teksilo_settings::{PerWindowState, WindowStateService};

/// Default minimum sanitize size when the config doesn't supply one.
const DEFAULT_MIN_SIZE: (u32, u32) = (320, 240);

/// Conservative work-area fallback when no winit monitor is available
/// (e.g. headless tests). Same fallback we'd hard-code app-side, now
/// in one place.
const FALLBACK_WORK_AREA: (u32, u32) = (1920, 1080);

/// Resolve the active monitor's work area in logical pixels. Returns
/// the fallback when no monitor is reachable from the active event
/// loop (a possibility on headless / wired-only hosts).
fn monitor_work_area(target: &winit::event_loop::ActiveEventLoop) -> (u32, u32) {
    if let Some(monitor) = target.primary_monitor() {
        let scale = monitor.scale_factor();
        let logical: winit::dpi::LogicalSize<u32> = monitor.size().to_logical(scale);
        return (logical.width.max(1), logical.height.max(1));
    }
    if let Some(monitor) = target.available_monitors().next() {
        let scale = monitor.scale_factor();
        let logical: winit::dpi::LogicalSize<u32> = monitor.size().to_logical(scale);
        return (logical.width.max(1), logical.height.max(1));
    }
    FALLBACK_WORK_AREA
}

/// Apply any saved geometry for `config.string_id` (sanitized against
/// the current monitor) to `config` in place. No-op if the service
/// has no entry for this label, or if no service / id is registered.
pub(crate) fn apply_restored_geometry(
    config: &mut WindowConfig,
    service: &WindowStateService,
    target: &winit::event_loop::ActiveEventLoop,
) {
    let Some(label) = config.string_id.clone() else {
        return;
    };
    let Some(saved) = service.state_for(&label) else {
        return;
    };

    let min_size = config.min_size.unwrap_or(DEFAULT_MIN_SIZE);
    let work_area = monitor_work_area(target);
    let sanitized = saved.sanitize(min_size, work_area);

    // Override the geometry fields. We never *clear* an explicit
    // app-supplied position to None — the saved state always carries
    // a position (sanitized).
    config.size = (sanitized.width, sanitized.height);
    config.position = Some((sanitized.x, sanitized.y));
    // The full placement enum round-trips: Floating, Maximized, and
    // Fullscreen all restore to their saved variant. Minimized was
    // already downgraded to Floating by `sanitize` so the app
    // doesn't appear to fail to start.
    config.initial_placement = sanitized.placement;
}

/// Install observers on `state`'s size / position / placement signals
/// that record into `service` under `label`. Returns the handles —
/// drop them to stop tracking. Caller stashes them on the
/// `ManagedWindow` so they live as long as the window itself.
pub(crate) fn install_persist_observers(
    state: &WindowState,
    service: WindowStateService,
    label: String,
) -> Vec<ObserverHandle> {
    let mut handles = Vec::with_capacity(3);

    // Snapshot once on install so the on-disk state matches the
    // window's actual creation geometry — important because the OS
    // may have clamped or repositioned the saved values further
    // (DPI rounding, snap-to-grid window managers, etc.).
    record_snapshot(&service, state, &label);

    // We deliberately capture the three child signals *by value*
    // instead of cloning the whole `WindowState`. `WindowState` is
    // `Rc<WindowStateInner>`, and `WindowStateInner` itself owns
    // these signals plus a `Vec<ObserverHandle>` of OS-sync observers
    // bound to the same signals. If the closure held a `WindowState`
    // clone, the registered observer would form an Rc cycle:
    // signal_X.observers[i] → Rc<dyn Fn> → WindowState →
    // WindowStateInner._observer_handles[j] → signal_X. Tearing the
    // first ObserverHandle then triggers `signal_X.borrow_mut()
    // .retain(...)`, which drops the closure and the inner Rc — and
    // dropping `WindowStateInner._observer_handles` mid-retain calls
    // `signal_X.borrow_mut()` *again*, panicking with
    // "RefCell already borrowed". Capturing only the signals breaks
    // the cycle: each `Signal<T>` is its own `Rc<RefCell<...>>`, so
    // the closure keeps the *signal innards* alive (harmless — the
    // OS-sync observers would have done that anyway) without
    // pinning `WindowStateInner`.
    let placement = state.placement().clone();
    let size = state.size().clone();
    let position = state.position().clone();

    handles.push(observe(
        state.size(),
        service.clone(),
        placement.clone(),
        size.clone(),
        position.clone(),
        label.clone(),
    ));
    handles.push(observe(
        state.position(),
        service.clone(),
        placement.clone(),
        size.clone(),
        position.clone(),
        label.clone(),
    ));
    handles.push(observe(
        state.placement(),
        service,
        placement,
        size,
        position,
        label,
    ));
    handles
}

fn observe<T: Clone + 'static>(
    signal: &Signal<T>,
    service: WindowStateService,
    placement: Signal<teksilo_core::WindowPlacement>,
    size: Signal<(u32, u32)>,
    position: Signal<(i32, i32)>,
    label: String,
) -> ObserverHandle {
    signal.observe(move |_| {
        let (w, h) = size.get();
        let (x, y) = position.get();
        let placement = placement.get();
        let _ = service.record(PerWindowState {
            label: label.clone(),
            x,
            y,
            width: w,
            height: h,
            placement,
        });
    })
}

fn record_snapshot(service: &WindowStateService, state: &WindowState, label: &str) {
    let (w, h) = state.size().get();
    let (x, y) = state.position().get();
    let placement = state.placement().get();
    let _ = service.record(PerWindowState {
        label: label.to_string(),
        x,
        y,
        width: w,
        height: h,
        placement,
    });
}
