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
//!    sanitized against the monitor it overlaps most and applied to the
//!    [`WindowConfig`]. A coordinate from a now-disconnected monitor
//!    is recentered onto the current primary; an oversized rectangle
//!    is clamped down. See
//!    [`PerWindowState::sanitize`](teksilo_settings::PerWindowState::sanitize).
//!    Once the window exists, still hidden, its frame is known, and
//!    [`keep_inside_work_area`] moves and shrinks it into its monitor's
//!    work area, title bar included.
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
use teksilo_platform::desktop::DesktopRect;
use teksilo_settings::{PerWindowState, WindowStateService};

/// Default minimum sanitize size when the config doesn't supply one, for a
/// window configured at least this large (see [`restored_geometry`]).
const DEFAULT_MIN_SIZE: (u32, u32) = (320, 240);

/// The monitor assumed when none is reachable (headless and wired-only
/// hosts).
const FALLBACK_MONITOR: LogicalArea = LogicalArea {
    x: 0,
    y: 0,
    width: 1920,
    height: 1080,
};

/// A monitor's rectangle in logical pixels, the units a [`WindowConfig`]
/// and the saved geometry are in.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LogicalArea {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl LogicalArea {
    fn of(monitor: &winit::monitor::MonitorHandle) -> Self {
        let scale = monitor.scale_factor();
        let position: winit::dpi::LogicalPosition<i32> = monitor.position().to_logical(scale);
        let size: winit::dpi::LogicalSize<u32> = monitor.size().to_logical(scale);
        LogicalArea {
            x: position.x,
            y: position.y,
            width: size.width.max(1),
            height: size.height.max(1),
        }
    }

    fn overlap(&self, saved: &PerWindowState) -> i64 {
        let width = (self.x as i64 + self.width as i64).min(saved.x as i64 + saved.width as i64)
            - (self.x as i64).max(saved.x as i64);
        let height = (self.y as i64 + self.height as i64).min(saved.y as i64 + saved.height as i64)
            - (self.y as i64).max(saved.y as i64);
        width.max(0) * height.max(0)
    }
}

/// The monitors, the primary first, in logical pixels.
fn logical_monitors(target: &winit::event_loop::ActiveEventLoop) -> Vec<LogicalArea> {
    let primary = target.primary_monitor();
    primary
        .iter()
        .cloned()
        .chain(
            target
                .available_monitors()
                .filter(|monitor| Some(monitor) != primary.as_ref()),
        )
        .map(|monitor| LogicalArea::of(&monitor))
        .collect()
}

/// The monitor `saved` comes back on: the one it overlaps most, else the
/// first (the primary, when the window system names one), whose own
/// monitor is gone.
fn restore_monitor(monitors: &[LogicalArea], saved: &PerWindowState) -> LogicalArea {
    monitors
        .iter()
        .copied()
        .filter(|monitor| monitor.overlap(saved) > 0)
        .max_by_key(|monitor| monitor.overlap(saved))
        .or_else(|| monitors.first().copied())
        .unwrap_or(FALLBACK_MONITOR)
}

/// Apply any saved geometry for `config.string_id` (sanitized against
/// the monitor it comes back on) to `config` in place, and say whether
/// there was any. No-op if the service has no entry for this label, or
/// if no service / id is registered.
pub(crate) fn apply_restored_geometry(
    config: &mut WindowConfig,
    service: &WindowStateService,
    target: &winit::event_loop::ActiveEventLoop,
) -> bool {
    let Some(label) = config.string_id.clone() else {
        return false;
    };
    let Some(saved) = service.state_for(&label) else {
        return false;
    };

    let monitor = restore_monitor(&logical_monitors(target), &saved);
    let sanitized = restored_geometry(config, &saved, monitor);

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
    true
}

/// The geometry `saved` restores `config` to on `monitor`: see
/// [`PerWindowState::sanitize`], which works in the monitor's own
/// coordinates. The size is kept at least the config's minimum, else
/// [`DEFAULT_MIN_SIZE`] on each axis where the window is configured at
/// least that large: a window smaller by design (a tool palette) comes
/// back at its own size.
fn restored_geometry(
    config: &WindowConfig,
    saved: &PerWindowState,
    monitor: LogicalArea,
) -> PerWindowState {
    let min_size = config.min_size.unwrap_or((
        DEFAULT_MIN_SIZE.0.min(config.size.0),
        DEFAULT_MIN_SIZE.1.min(config.size.1),
    ));
    let on_monitor = PerWindowState {
        x: saved.x.saturating_sub(monitor.x),
        y: saved.y.saturating_sub(monitor.y),
        ..saved.clone()
    };
    let mut restored = on_monitor.sanitize(min_size, (monitor.width, monitor.height));
    restored.x = restored.x.saturating_add(monitor.x);
    restored.y = restored.y.saturating_add(monitor.y);
    restored
}

/// Keep restored `window`, still hidden, inside the work area of the
/// monitor it is on: the monitor less the taskbar, the Dock and the menu
/// bar, the panels. Its title bar is counted, which the saved geometry
/// cannot be (it is the content's size), and the window is shrunk no
/// further than `min_size`. Nothing on Wayland, where a client is told
/// neither where its window is nor where the panels are.
pub(crate) fn keep_inside_work_area(window: &winit::window::Window, min_size: Option<(u32, u32)>) {
    use teksilo_platform::desktop;

    let Some((outer, inner)) = desktop::window_frame(window) else {
        return;
    };
    let Some(work_area) = desktop::monitors(window)
        .into_iter()
        .filter(|monitor| monitor.bounds.overlap_area(&outer) > 0.0)
        .max_by(|a, b| {
            a.bounds
                .overlap_area(&outer)
                .total_cmp(&b.bounds.overlap_area(&outer))
        })
        .map(|monitor| monitor.work_area)
    else {
        return;
    };
    let per_logical = desktop::units_per_logical(window);
    let min_size = min_size.map_or((1.0, 1.0), |(width, height)| {
        (width as f64 * per_logical, height as f64 * per_logical)
    });
    if let Some((position, inner_size)) = fit_in_work_area(outer, inner, work_area, min_size) {
        desktop::set_window_frame(window, position, inner_size);
    }
}

/// Where a window whose frame is `outer` and content `inner` goes to lie
/// inside `work_area`, all in desktop coordinates: the frame's top-left
/// corner and the content's size, or `None` when it is inside already.
///
/// The content is shrunk to fit, down to `min_size`, then the window is
/// moved in; one too large even so has its top-left corner at the work
/// area's, its title bar in reach. What must fit is the content and all
/// of the frame above it: the frame's sides and bottom may run past,
/// since on Windows they are mostly the invisible resize margins, and a
/// window snapped to a screen edge has them off the screen.
fn fit_in_work_area(
    outer: DesktopRect,
    inner: DesktopRect,
    work_area: DesktopRect,
    min_size: (f64, f64),
) -> Option<((f64, f64), (f64, f64))> {
    let above = (inner.y - outer.y).max(0.0);
    let beside = inner.x - outer.x;
    let width = inner.width.min(work_area.width).max(min_size.0).max(1.0);
    let height = inner
        .height
        .min(work_area.height - above)
        .max(min_size.1)
        .max(1.0);
    let fit = |start: f64, length: f64, from: f64, to: f64| {
        if length >= to - from {
            from
        } else {
            start.clamp(from, to - length)
        }
    };
    let x = fit(inner.x, width, work_area.x, work_area.right()) - beside;
    let y = fit(outer.y, above + height, work_area.y, work_area.bottom());
    let moved = (x - outer.x).abs() >= 0.5 || (y - outer.y).abs() >= 0.5;
    let resized = (width - inner.width).abs() >= 0.5 || (height - inner.height).abs() >= 0.5;
    (moved || resized).then_some(((x, y), (width, height)))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(width: u32, height: u32) -> PerWindowState {
        PerWindowState {
            label: "palette".into(),
            x: 100,
            y: 80,
            width,
            height,
            placement: teksilo_core::WindowPlacement::Floating,
        }
    }

    /// A window smaller than the default floor (a tool palette) comes back
    /// at its own size: the floor is never larger than the size the window
    /// is configured with. It grew to 320 x 240 on every launch.
    #[test]
    fn a_window_smaller_than_the_default_floor_keeps_its_size() {
        let config = WindowConfig::new().size(200, 150);
        let restored = restored_geometry(&config, &saved(200, 150), FALLBACK_MONITOR);
        assert_eq!((restored.width, restored.height), (200, 150));
    }

    /// The floor is taken axis by axis: a wide, short window keeps the
    /// default floor across and its own height down.
    #[test]
    fn the_floor_is_the_smaller_of_the_default_and_the_size_on_each_axis() {
        let config = WindowConfig::new().size(600, 100);
        let restored = restored_geometry(&config, &saved(10, 10), FALLBACK_MONITOR);
        assert_eq!((restored.width, restored.height), (320, 100));
    }

    /// A configured minimum wins over both.
    #[test]
    fn a_configured_minimum_is_the_floor() {
        let config = WindowConfig::new().size(200, 150).min_size(120, 90);
        let restored = restored_geometry(&config, &saved(10, 10), FALLBACK_MONITOR);
        assert_eq!((restored.width, restored.height), (120, 90));
    }

    const PRIMARY: LogicalArea = LogicalArea {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };
    const LEFT_OF_PRIMARY: LogicalArea = LogicalArea {
        x: -1280,
        y: 0,
        width: 1280,
        height: 1024,
    };

    fn at(x: i32, y: i32, width: u32, height: u32) -> PerWindowState {
        PerWindowState {
            label: "main".into(),
            x,
            y,
            width,
            height,
            placement: teksilo_core::WindowPlacement::Floating,
        }
    }

    /// A window left on a second monitor comes back there. It came back
    /// centred on the primary: only the primary was looked at, and the
    /// window was off it.
    #[test]
    fn a_window_on_a_second_monitor_comes_back_on_it() {
        let monitors = [PRIMARY, LEFT_OF_PRIMARY];
        let saved = at(-1100, 100, 800, 600);
        let monitor = restore_monitor(&monitors, &saved);
        assert_eq!(monitor, LEFT_OF_PRIMARY);
        let restored = restored_geometry(&WindowConfig::new().size(800, 600), &saved, monitor);
        assert_eq!((restored.x, restored.y), (-1100, 100));
    }

    /// The monitor it overlaps most wins.
    #[test]
    fn a_window_across_two_monitors_goes_with_the_larger_share() {
        let monitors = [PRIMARY, LEFT_OF_PRIMARY];
        assert_eq!(
            restore_monitor(&monitors, &at(-300, 100, 800, 600)),
            PRIMARY
        );
        assert_eq!(
            restore_monitor(&monitors, &at(-600, 100, 800, 600)),
            LEFT_OF_PRIMARY
        );
    }

    /// A window whose monitor is gone comes back centred on the first
    /// monitor, the primary.
    #[test]
    fn a_window_whose_monitor_is_gone_is_centred_on_the_primary() {
        let saved = at(3000, 100, 800, 600);
        let monitor = restore_monitor(&[PRIMARY, LEFT_OF_PRIMARY], &saved);
        assert_eq!(monitor, PRIMARY);
        let restored = restored_geometry(&WindowConfig::new().size(800, 600), &saved, monitor);
        assert_eq!((restored.x, restored.y), (560, 100));
    }

    /// The size is clamped to the monitor the window comes back on, not
    /// to the primary.
    #[test]
    fn the_size_is_clamped_to_its_own_monitor() {
        let saved = at(-1200, 0, 1600, 1060);
        let restored = restored_geometry(
            &WindowConfig::new().size(800, 600),
            &saved,
            restore_monitor(&[PRIMARY, LEFT_OF_PRIMARY], &saved),
        );
        assert_eq!((restored.width, restored.height), (1280, 1024));
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> DesktopRect {
        DesktopRect {
            x,
            y,
            width,
            height,
        }
    }

    /// A Windows frame: 8 px on each side and below, the outer 7 of them
    /// invisible, and a 31 px title bar.
    fn windows_frame(x: f64, y: f64, width: f64, height: f64) -> (DesktopRect, DesktopRect) {
        (
            rect(x, y, width + 16.0, height + 39.0),
            rect(x + 8.0, y + 31.0, width, height),
        )
    }

    /// 1667 x 1006 with a 48 px taskbar along the bottom.
    const WORK_AREA: DesktopRect = DesktopRect {
        x: 0.0,
        y: 0.0,
        width: 1667.0,
        height: 958.0,
    };

    /// The window seen in the Windows VM: saved 1644 x 1045, taller than
    /// the 1006 px screen. It comes back as tall as the work area less its
    /// title bar, and moved in so its right edge is on the screen.
    #[test]
    fn a_window_taller_than_the_work_area_is_shrunk_below_its_title_bar() {
        let (outer, inner) = windows_frame(52.0, 52.0, 1644.0, 1045.0);
        assert_eq!(
            fit_in_work_area(outer, inner, WORK_AREA, (1.0, 1.0)),
            Some(((15.0, 0.0), (1644.0, 927.0)))
        );
    }

    #[test]
    fn a_window_inside_stays_where_it_is() {
        let (outer, inner) = windows_frame(100.0, 100.0, 800.0, 600.0);
        assert_eq!(fit_in_work_area(outer, inner, WORK_AREA, (1.0, 1.0)), None);
    }

    /// A window that runs under the taskbar moves up, its size kept,
    /// until its content's bottom edge meets the taskbar.
    #[test]
    fn a_window_under_the_taskbar_moves_up() {
        let (outer, inner) = windows_frame(100.0, 500.0, 800.0, 600.0);
        assert_eq!(
            fit_in_work_area(outer, inner, WORK_AREA, (1.0, 1.0)),
            Some(((100.0, 958.0 - 31.0 - 600.0), (800.0, 600.0)))
        );
    }

    /// A window off the left edge moves right; one snapped to the left
    /// edge, its invisible border off the screen, stays.
    #[test]
    fn the_side_borders_may_run_off_the_screen() {
        let (outer, inner) = windows_frame(-300.0, 100.0, 800.0, 600.0);
        assert_eq!(
            fit_in_work_area(outer, inner, WORK_AREA, (1.0, 1.0)),
            Some(((-8.0, 100.0), (800.0, 600.0)))
        );
        let (outer, inner) = windows_frame(-8.0, 0.0, 800.0, 600.0);
        assert_eq!(fit_in_work_area(outer, inner, WORK_AREA, (1.0, 1.0)), None);
    }

    /// The title bar is never pushed above the work area: a window that
    /// cannot fit even at its minimum size has its top-left corner at the
    /// work area's.
    #[test]
    fn a_window_too_large_at_its_minimum_keeps_its_title_bar_in_reach() {
        let (outer, inner) = windows_frame(300.0, 200.0, 1800.0, 1200.0);
        assert_eq!(
            fit_in_work_area(outer, inner, WORK_AREA, (1700.0, 1000.0)),
            Some(((-8.0, 0.0), (1700.0, 1000.0)))
        );
    }

    /// macOS: the frame is the content and a title bar above it, and the
    /// work area starts below the menu bar.
    #[test]
    fn a_macos_window_is_kept_below_the_menu_bar() {
        let work_area = rect(0.0, 25.0, 1440.0, 875.0 - 70.0);
        let outer = rect(200.0, 0.0, 900.0, 728.0);
        let inner = rect(200.0, 28.0, 900.0, 700.0);
        assert_eq!(
            fit_in_work_area(outer, inner, work_area, (1.0, 1.0)),
            Some(((200.0, 25.0), (900.0, 700.0)))
        );
    }
}
