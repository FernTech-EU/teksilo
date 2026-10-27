// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where a window may go: each monitor's rectangle and its work area (the
//! monitor less what the desktop keeps for itself: the taskbar, the Dock and
//! the menu bar, a panel), and a window's frame, in one coordinate space.
//!
//! winit reports a monitor's rectangle but not its work area, and its pixel
//! positions are one space on Windows and X11 but not on macOS, where a
//! "physical" position is a position in points times the scale factor of the
//! monitor or window it belongs to: a Retina screen and the 1x screen beside
//! it overlap in it. The space here is pixels on Windows and X11 and points
//! on macOS, the units each of them places windows in.
//!
//! A Wayland client is told neither where its window is nor what the panels
//! take: [`window_frame`] answers `None` there.

use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::window::Window;

/// A rectangle in desktop coordinates (see the module's documentation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesktopRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl DesktopRect {
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    /// The area `self` and `other` share, zero when they do not meet.
    pub fn overlap_area(&self, other: &DesktopRect) -> f64 {
        let width = self.right().min(other.right()) - self.x.max(other.x);
        let height = self.bottom().min(other.bottom()) - self.y.max(other.y);
        width.max(0.0) * height.max(0.0)
    }

    /// The rectangle `self` and `other` share, `None` when they do not meet.
    pub fn intersection(&self, other: &DesktopRect) -> Option<DesktopRect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let width = self.right().min(other.right()) - x;
        let height = self.bottom().min(other.bottom()) - y;
        (width > 0.0 && height > 0.0).then_some(DesktopRect {
            x,
            y,
            width,
            height,
        })
    }

    fn from_pixels(position: PhysicalPosition<i32>, size: PhysicalSize<u32>, unit: f64) -> Self {
        DesktopRect {
            x: position.x as f64 * unit,
            y: position.y as f64 * unit,
            width: size.width as f64 * unit,
            height: size.height as f64 * unit,
        }
    }
}

/// A monitor's whole rectangle and its work area, in desktop coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DesktopMonitor {
    pub bounds: DesktopRect,
    pub work_area: DesktopRect,
}

/// How many desktop units one pixel of something at `scale_factor` is.
fn units_per_pixel(scale_factor: f64) -> f64 {
    if cfg!(target_os = "macos") {
        1.0 / scale_factor
    } else {
        1.0
    }
}

/// How many desktop units one logical pixel of `window` is.
pub fn units_per_logical(window: &Window) -> f64 {
    let scale = window.scale_factor();
    scale * units_per_pixel(scale)
}

/// `window`'s frame and its content, `(outer, inner)`, in desktop
/// coordinates. `None` where the window system does not say where a window
/// is (Wayland).
pub fn window_frame(window: &Window) -> Option<(DesktopRect, DesktopRect)> {
    let unit = units_per_pixel(window.scale_factor());
    let outer = window.outer_position().ok()?;
    let inner = window.inner_position().ok()?;
    Some((
        DesktopRect::from_pixels(outer, window.outer_size(), unit),
        DesktopRect::from_pixels(inner, window.inner_size(), unit),
    ))
}

/// Size `window`'s content to `inner_size`, then put its frame's top-left
/// corner at `position`, both in desktop coordinates. In that order: AppKit
/// keeps a window's bottom-left corner where it is when it resizes it.
pub fn set_window_frame(window: &Window, position: (f64, f64), inner_size: (f64, f64)) {
    let unit = units_per_pixel(window.scale_factor());
    let _ = window.request_inner_size(PhysicalSize::new(
        (inner_size.0 / unit).round().max(1.0) as u32,
        (inner_size.1 / unit).round().max(1.0) as u32,
    ));
    window.set_outer_position(PhysicalPosition::new(
        (position.0 / unit).round() as i32,
        (position.1 / unit).round() as i32,
    ));
}

/// Every monitor, with its work area. A monitor whose work area cannot be
/// read has its whole rectangle as one.
pub fn monitors(window: &Window) -> Vec<DesktopMonitor> {
    #[cfg(target_os = "macos")]
    {
        let _ = window;
        macos::monitors()
    }
    #[cfg(not(target_os = "macos"))]
    {
        #[cfg(all(unix, not(target_os = "macos")))]
        let x11_work_area = x11_work_area(window);
        window
            .available_monitors()
            .map(|monitor| {
                let bounds = DesktopRect::from_pixels(
                    monitor.position(),
                    monitor.size(),
                    units_per_pixel(monitor.scale_factor()),
                );
                #[cfg(target_os = "windows")]
                if let Some(read) = win32::monitor(&monitor) {
                    return read;
                }
                #[cfg(all(unix, not(target_os = "macos")))]
                if let Some(work_area) = x11_work_area.and_then(|area| area.intersection(&bounds)) {
                    return DesktopMonitor { bounds, work_area };
                }
                DesktopMonitor {
                    bounds,
                    work_area: bounds,
                }
            })
            .collect()
    }
}

/// The X11 desktop's work area, `_NET_WORKAREA` for the current desktop.
/// It is one rectangle for the whole screen: a monitor's is its share of
/// it, as GTK and Qt take it.
#[cfg(all(unix, not(target_os = "macos")))]
fn x11_work_area(window: &Window) -> Option<DesktopRect> {
    use raw_window_handle::HasDisplayHandle;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    let display = window.display_handle().ok()?.as_raw();
    if crate::window_system::window_system_for_display_handle(&display)
        != crate::window_system::WindowSystem::X11
    {
        return None;
    }
    let connection = crate::x11::X11Connection::open().ok()?;
    let atom = |name: &[u8]| {
        let reply = connection
            .conn()
            .intern_atom(true, name)
            .ok()?
            .reply()
            .ok()?;
        (reply.atom != x11rb::NONE).then_some(reply.atom)
    };
    let cardinals = |property| {
        connection
            .get_property_full(connection.root(), property, AtomEnum::CARDINAL.into())
            .ok()
            .flatten()
            .map(|value| value.as_u32s())
    };
    let desktop = atom(b"_NET_CURRENT_DESKTOP")
        .and_then(cardinals)
        .and_then(|value| value.first().copied())
        .unwrap_or(0) as usize;
    let areas = cardinals(atom(b"_NET_WORKAREA")?)?;
    let (areas, _) = areas.as_chunks::<4>();
    let [x, y, width, height] = *areas.get(desktop).or_else(|| areas.first())?;
    Some(DesktopRect {
        x: x as i32 as f64,
        y: y as i32 as f64,
        width: width as f64,
        height: height as f64,
    })
}

#[cfg(target_os = "windows")]
mod win32 {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITORINFO};
    use winit::monitor::MonitorHandle;
    use winit::platform::windows::MonitorHandleExtWindows;

    use super::{DesktopMonitor, DesktopRect};

    /// The monitor's rectangle and work area, as Windows gives them.
    pub(super) fn monitor(monitor: &MonitorHandle) -> Option<DesktopMonitor> {
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: the handle is winit's, of a monitor it has just listed,
        // and `info` is the size `cbSize` says.
        let read = unsafe { GetMonitorInfoW(HMONITOR(monitor.hmonitor() as *mut _), &mut info) };
        read.as_bool().then(|| DesktopMonitor {
            bounds: rect(info.rcMonitor),
            work_area: rect(info.rcWork),
        })
    }

    fn rect(rect: RECT) -> DesktopRect {
        DesktopRect {
            x: rect.left as f64,
            y: rect.top as f64,
            width: (rect.right - rect.left) as f64,
            height: (rect.bottom - rect.top) as f64,
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;
    use objc2_core_graphics::{CGDisplayBounds, CGMainDisplayID};
    use objc2_foundation::NSRect;

    use super::{DesktopMonitor, DesktopRect};

    /// Every screen, `visibleFrame` its work area: the screen less the menu
    /// bar and the Dock.
    pub(super) fn monitors() -> Vec<DesktopMonitor> {
        let Some(mtm) = MainThreadMarker::new() else {
            return Vec::new();
        };
        // AppKit's screen coordinates rise from the bottom of the main
        // display; winit's, like every other platform's, fall from its top.
        let main_height = CGDisplayBounds(CGMainDisplayID()).size.height;
        let flip = |rect: NSRect| DesktopRect {
            x: rect.origin.x,
            y: main_height - rect.origin.y - rect.size.height,
            width: rect.size.width,
            height: rect.size.height,
        };
        NSScreen::screens(mtm)
            .iter()
            .map(|screen| DesktopMonitor {
                bounds: flip(screen.frame()),
                work_area: flip(screen.visibleFrame()),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::DesktopRect;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> DesktopRect {
        DesktopRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn overlap_is_the_shared_area_and_zero_apart() {
        let screen = rect(0.0, 0.0, 100.0, 100.0);
        assert_eq!(
            screen.overlap_area(&rect(50.0, 80.0, 100.0, 100.0)),
            50.0 * 20.0
        );
        assert_eq!(screen.overlap_area(&rect(100.0, 0.0, 10.0, 10.0)), 0.0);
        assert_eq!(screen.overlap_area(&rect(-200.0, -200.0, 10.0, 10.0)), 0.0);
    }

    #[test]
    fn intersection_is_none_apart() {
        let screen = rect(0.0, 0.0, 100.0, 100.0);
        assert_eq!(
            screen.intersection(&rect(-10.0, 30.0, 50.0, 200.0)),
            Some(rect(0.0, 30.0, 40.0, 70.0))
        );
        assert_eq!(screen.intersection(&rect(100.0, 0.0, 10.0, 10.0)), None);
    }
}
