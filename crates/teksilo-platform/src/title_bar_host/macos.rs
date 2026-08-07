// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! macOS title bar host. Uses `objc2-app-kit` to inspect the `NSWindow`
//! behind the winit window: we measure the standard traffic-light cluster
//! at construction time to reserve the correct leading inset. Drag stays on
//! winit's own surface; edge resize is handled natively by the `NSWindow`
//! frame (we return `false` from `needs_custom_resize_handles`, so the
//! application skips installing a `WindowFrame` overlay).

use std::sync::Arc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSWindow, NSWindowButton};
use teksilo_canvas::{Point, Size};
use teksilo_core::{
    HitRegions, PlatformError, PlatformTitleBarHost, ResizeEdge, TitleBarHostCallbacks,
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

pub struct MacOsHost {
    window: Arc<Window>,
    leading_inset: Size,
}

impl MacOsHost {
    pub fn new(
        window: Arc<Window>,
        _callbacks: TitleBarHostCallbacks,
    ) -> Result<Self, PlatformError> {
        let _mtm = MainThreadMarker::new()
            .ok_or_else(|| PlatformError::Os("MacOsHost::new must run on main thread".into()))?;

        // SAFETY: the winit window is alive as long as our `Arc<Window>`
        // holds a clone, which outlives this block; its NSView owns the
        // NSWindow. The retained NSWindow lives only long enough to measure
        // the traffic-light cluster; after that we drop it.
        let ns_window = unsafe { ns_window_from_winit(&window)? };
        let leading_inset = measure_traffic_light_inset(&ns_window);

        Ok(Self {
            window,
            leading_inset,
        })
    }
}

unsafe fn ns_window_from_winit(window: &Arc<Window>) -> Result<Retained<NSWindow>, PlatformError> {
    let handle = window
        .window_handle()
        .map_err(|e| PlatformError::Os(format!("window_handle: {e}")))?;
    let RawWindowHandle::AppKit(raw) = handle.as_raw() else {
        return Err(PlatformError::Os("expected AppKit window handle".into()));
    };
    // Re-retain the NSView pointer winit hands us, then ask it for its
    // parent NSWindow. Matches the pattern in `window_system::attach_child_window`.
    let ns_view: Retained<NSView> = unsafe { Retained::retain(raw.ns_view.as_ptr().cast()) }
        .ok_or_else(|| PlatformError::Os("failed to retain NSView".into()))?;
    ns_view
        .window()
        .ok_or_else(|| PlatformError::Os("NSView has no attached NSWindow".into()))
}

fn measure_traffic_light_inset(ns_window: &NSWindow) -> Size {
    // Standard macOS layout: 3 buttons at ~14 px diameter with 6 px gap,
    // close button inset ~8 px from the window's leading edge. The exact
    // layout can drift across OS versions, so we measure the buttons the
    // system vended us rather than hard-coding their frames.
    let close = ns_window.standardWindowButton(NSWindowButton::CloseButton);
    let zoom = ns_window.standardWindowButton(NSWindowButton::ZoomButton);
    if let (Some(close), Some(zoom)) = (close, zoom) {
        let cf = close.frame();
        let zf = zoom.frame();
        // Width = zoom right edge minus close left edge, plus close's own
        // leading inset, plus trailing padding between the zoom button and
        // the start of app-drawn content.
        let leading_edge = cf.origin.x as f32;
        let cluster_width = (zf.origin.x + zf.size.width - cf.origin.x) as f32;
        let trailing_padding = 12.0_f32;
        let width = leading_edge + cluster_width + trailing_padding;
        let height = cf.size.height as f32;
        Size::new(width, height)
    } else {
        // Fallback: the documented default cluster on macOS 10.15+.
        Size::new(78.0, 22.0)
    }
}

impl PlatformTitleBarHost for MacOsHost {
    fn reserved_leading_inset(&self) -> Size {
        self.leading_inset
    }

    fn reserved_trailing_inset(&self) -> Size {
        Size::ZERO
    }

    fn renders_custom_controls(&self) -> bool {
        // The OS draws the traffic lights; we must not render our own
        // minimize/maximize/close cluster on top of them.
        false
    }

    fn needs_custom_resize_handles(&self) -> bool {
        // NSWindow's native frame still services edge resize even with
        // `titlebarAppearsTransparent` + `fullSizeContentView`.
        false
    }

    fn begin_drag(&self) -> Result<(), PlatformError> {
        self.window
            .drag_window()
            .map_err(|e| PlatformError::Os(e.to_string()))
    }

    fn begin_resize(&self, _edge: ResizeEdge) -> Result<(), PlatformError> {
        // winit 0.30's `drag_resize_window` is Windows/Linux-only. On macOS
        // the native NSWindow frame handles edge resize, and the widget
        // layer never calls us because `needs_custom_resize_handles` is
        // false — but keep a defensive return in case someone does.
        Err(PlatformError::Unsupported)
    }

    fn show_window_menu(&self, _at: Point) -> Result<(), PlatformError> {
        // macOS has no equivalent of xdg-shell's client-requested window
        // menu. DragRegion right-clicks reach us but we intentionally
        // no-op; the standard Cocoa Dock / title-bar context menu is
        // still triggered by right-clicking the dock icon.
        Ok(())
    }

    fn update_hit_regions(&self, _regions: &HitRegions) {
        // Unused on macOS: we don't intercept the hit-test flow, the
        // native NSWindow frame dispatches resize and the OS handles
        // traffic-light clicks directly.
    }
}
