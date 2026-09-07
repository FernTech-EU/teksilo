// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Per-platform implementations of [`teksilo_core::PlatformTitleBarHost`].
//!
//! Each backend lives in its own submodule under `title_bar_host/`. The
//! [`create_title_bar_host`] factory picks the right one based on the current
//! platform and — on unix — the window's live display handle. When no backend
//! can serve the window it returns [`PlatformError::Unsupported`] and the
//! application falls back to native server-side decorations.
//!
//! X11 is supported, but conditionally: custom chrome there depends on the
//! window manager implementing `_NET_WM_MOVERESIZE`, since a borderless window
//! has no other way to be moved or resized. See `title_bar_host/x11.rs`.

use std::rc::Rc;
use std::sync::Arc;

use teksilo_core::{
    HitRegions, PlatformError, PlatformTitleBarHost, ResizeBorders, ResizeEdge,
    TitleBarHostCallbacks,
};
use winit::window::{ResizeDirection, Window};

/// Map a teksilo-core [`ResizeEdge`] to winit's [`ResizeDirection`].
/// Used by the Wayland and Windows backends — both delegate
/// interactive resize to winit, which translates internally to the
/// platform's native protocol (xdg-shell `resize`, `WM_NCLBUTTONDOWN`
/// with `HTLEFT`/etc.).
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(crate) fn edge_to_direction(edge: ResizeEdge) -> ResizeDirection {
    match edge {
        ResizeEdge::Top => ResizeDirection::North,
        ResizeEdge::TopRight => ResizeDirection::NorthEast,
        ResizeEdge::Right => ResizeDirection::East,
        ResizeEdge::BottomRight => ResizeDirection::SouthEast,
        ResizeEdge::Bottom => ResizeDirection::South,
        ResizeEdge::BottomLeft => ResizeDirection::SouthWest,
        ResizeEdge::Left => ResizeDirection::West,
        ResizeEdge::TopLeft => ResizeDirection::NorthWest,
    }
}

// ---------------------------------------------------------------------------
// The non-client resize band
// ---------------------------------------------------------------------------

/// Whether `regions` is a **band update** rather than a full chrome snapshot.
///
/// [`HitRegions`] is a whole-snapshot channel with one aggregator per window —
/// `TitleBar::after_paint` collects the drag region, the dead-zone holes and the
/// three control buttons into one payload every frame. A `WindowFrame` has a
/// second, disjoint thing to say (how wide its resize strips will actually
/// catch, once [`Widget::hit_outset`] has widened them for a coarse pointer),
/// and it must be able to say it without erasing the aggregate.
///
/// The two are told apart by shape, not by a flag: a payload that carries a
/// non-zero [`ResizeBorders`] **and nothing else** is a band update. The
/// aggregator never produces that — it builds from `HitRegions::new()` and
/// leaves the band zero — so the classification is unambiguous in both
/// directions, including for the deliberately-empty snapshot a title bar
/// publishes to *clear* its regions when its control cluster is hidden.
///
/// [`Widget::hit_outset`]: teksilo_core::widget::Widget::hit_outset
pub fn is_resize_band_update(regions: &HitRegions) -> bool {
    let b = regions.resize_borders;
    let has_band = b.top > 0.0 || b.right > 0.0 || b.bottom > 0.0 || b.left > 0.0;
    has_band
        && regions.minimize.is_none()
        && regions.maximize.is_none()
        && regions.close.is_none()
        && regions.drag.is_empty()
        && regions.no_drag.is_empty()
}

/// Fold an incoming payload into the stored snapshot.
///
/// A band update (see [`is_resize_band_update`]) writes only
/// [`HitRegions::resize_borders`]; anything else replaces the snapshot whole,
/// which is what keeps a title bar able to clear its own regions.
pub fn merge_hit_regions(stored: &mut HitRegions, incoming: HitRegions) {
    if is_resize_band_update(&incoming) {
        stored.resize_borders = incoming.resize_borders;
    } else {
        *stored = incoming;
    }
}

/// The resize band the non-client hit test should use, per edge.
///
/// `os_metric` is what the window manager itself allows (on Windows,
/// `SM_CXPADDEDBORDER + SM_CXFRAME` — around 8 physical pixels at 100 %).
/// `published` is the widget layer's *coarse* band, already converted to the
/// same units, or all-zero when no widget published one.
///
/// Two rules, and the first one is the whole reason this is a function rather
/// than a `max`:
///
/// * **A precise pointer gets the OS metric, exactly.** Widening the band for a
///   mouse would steal presses from the client area along every edge of every
///   window — an 8 px border becoming 24 px is a quarter of a toolbar. The
///   coarse band exists because a finger cannot aim at 8 px, not because 8 px
///   is wrong.
/// * **The band never shrinks.** A widget that publishes a band narrower than
///   the OS metric (a frame built with a 2 dp strip, say) must not take away
///   resize area the window manager was already giving; the published value is
///   a floor to raise to, never a ceiling.
pub fn resize_band(os_metric: f32, published: ResizeBorders, coarse: bool) -> ResizeBorders {
    if !coarse {
        return ResizeBorders::uniform(os_metric);
    }
    ResizeBorders {
        top: published.top.max(os_metric),
        right: published.right.max(os_metric),
        bottom: published.bottom.max(os_metric),
        left: published.left.max(os_metric),
    }
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(unix, not(target_os = "macos")))]
mod wayland;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(all(unix, not(target_os = "macos")))]
mod x11;

#[cfg(target_os = "macos")]
pub use macos::MacOsHost;
#[cfg(all(unix, not(target_os = "macos")))]
pub use wayland::WaylandHost;
#[cfg(target_os = "windows")]
pub use windows::WindowsHost;
#[cfg(all(unix, not(target_os = "macos")))]
pub use x11::X11Host;

/// Construct a title bar host for the given winit window. Returns
/// [`PlatformError::Unsupported`] when the window system cannot support custom
/// chrome — on X11 that means no EWMH window manager, or one without
/// `_NET_WM_MOVERESIZE`.
///
/// On unix the backend is chosen from the window's **live**
/// `RawDisplayHandle`, not from the environment. `WAYLAND_DISPLAY` and
/// `DISPLAY` are both set in essentially every modern session, so only the
/// handle can say which backend winit actually created — and using one source
/// of truth here keeps the title bar and the DnD backend from ever disagreeing
/// about the same window.
///
/// The host borrows an `Arc` clone of the window so it can keep calling
/// winit (`drag_window`, `set_minimized`, ...) for the lifetime of the
/// title bar widget. `callbacks` carries closures that route operations
/// which must hop through the event loop (currently just `close`) back
/// to `WindowManager` — see [`TitleBarHostCallbacks`].
pub fn create_title_bar_host(
    window: Arc<Window>,
    callbacks: TitleBarHostCallbacks,
) -> Result<Rc<dyn PlatformTitleBarHost>, PlatformError> {
    #[cfg(target_os = "windows")]
    {
        WindowsHost::new(window, callbacks).map(|h| Rc::new(h) as Rc<dyn PlatformTitleBarHost>)
    }

    #[cfg(target_os = "macos")]
    {
        MacOsHost::new(window, callbacks).map(|h| Rc::new(h) as Rc<dyn PlatformTitleBarHost>)
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use winit::raw_window_handle::HasDisplayHandle;

        use crate::window_system::{WindowSystem, window_system_for_display_handle};

        let display = window
            .display_handle()
            .map_err(|e| PlatformError::Os(e.to_string()))?;

        match window_system_for_display_handle(&display.as_raw()) {
            WindowSystem::Wayland => WaylandHost::new(window, callbacks)
                .map(|h| Rc::new(h) as Rc<dyn PlatformTitleBarHost>),
            WindowSystem::X11 => {
                // `X11Host::new` refuses when the window manager can't service
                // `_NET_WM_MOVERESIZE`; the caller then keeps native
                // decorations. The same probe already gated
                // `with_decorations(false)` at window-creation time, so the two
                // decisions agree.
                X11Host::new(window, callbacks).map(|h| Rc::new(h) as Rc<dyn PlatformTitleBarHost>)
            }
            WindowSystem::Unknown => {
                eprintln!(
                    "teksilo-platform: window reports neither an X11 nor a Wayland \
                     display handle; custom TitleBar disabled"
                );
                Err(PlatformError::Unsupported)
            }
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
    {
        let _ = (window, callbacks);
        Err(PlatformError::Unsupported)
    }
}

#[cfg(test)]
mod resize_band_tests {
    use super::*;
    use teksilo_canvas::Rect;

    fn band(v: f32) -> ResizeBorders {
        ResizeBorders::uniform(v)
    }

    /// The invariant the whole mechanism stands on: with a precise pointer the
    /// band is the OS metric and nothing else, whatever a widget published.
    #[test]
    fn a_mouse_gets_the_os_metric_untouched() {
        for published in [0.0_f32, 6.0, 24.0, 44.0, 200.0] {
            let out = resize_band(8.0, band(published), false);
            assert_eq!(out.top, 8.0);
            assert_eq!(out.right, 8.0);
            assert_eq!(out.bottom, 8.0);
            assert_eq!(out.left, 8.0);
        }
    }

    /// A coarse pointer takes the published band where it is wider.
    #[test]
    fn a_coarse_pointer_takes_the_published_band() {
        let out = resize_band(8.0, band(24.0), true);
        assert_eq!(out.top, 24.0);
        assert_eq!(out.left, 24.0);
    }

    /// …and never less than the OS metric, so a narrow publication cannot take
    /// away resize area the window manager was already giving.
    #[test]
    fn the_published_band_is_a_floor_never_a_ceiling() {
        let out = resize_band(8.0, band(2.0), true);
        assert_eq!(out.top, 8.0);
        assert_eq!(out.bottom, 8.0);
    }

    /// No publication at all is the same as a mouse: the OS metric stands.
    #[test]
    fn an_unpublished_band_falls_back_to_the_os_metric() {
        let out = resize_band(8.0, ResizeBorders::default(), true);
        assert_eq!(out.top, 8.0);
        assert_eq!(out.right, 8.0);
    }

    /// Per-edge, not uniform: a frame is free to publish four different numbers.
    #[test]
    fn each_edge_is_resolved_on_its_own() {
        let published = ResizeBorders {
            top: 44.0,
            right: 4.0,
            bottom: 24.0,
            left: 0.0,
        };
        let out = resize_band(8.0, published, true);
        assert_eq!(out.top, 44.0);
        assert_eq!(out.right, 8.0, "below the metric → the metric");
        assert_eq!(out.bottom, 24.0);
        assert_eq!(out.left, 8.0, "unpublished → the metric");
    }

    /// A band-only payload is recognised as an update to the band.
    #[test]
    fn a_band_only_payload_is_a_band_update() {
        let regions = HitRegions {
            resize_borders: band(24.0),
            ..HitRegions::default()
        };
        assert!(is_resize_band_update(&regions));
    }

    /// The title bar's aggregate snapshot never is — it leaves the band zero.
    #[test]
    fn a_chrome_snapshot_is_not_a_band_update() {
        let regions = HitRegions {
            drag: vec![Rect::new(0.0, 0.0, 400.0, 32.0)],
            ..HitRegions::default()
        };
        assert!(!is_resize_band_update(&regions));
    }

    /// Nor is the deliberately empty one a title bar publishes to *clear* its
    /// regions when its control cluster goes hidden — which is exactly the case
    /// a "merge when empty" rule would have got wrong.
    #[test]
    fn an_empty_clearing_snapshot_is_not_a_band_update() {
        assert!(!is_resize_band_update(&HitRegions::default()));
    }

    /// A band update leaves every other region standing.
    #[test]
    fn merging_a_band_update_preserves_the_chrome_snapshot() {
        let mut stored = HitRegions {
            drag: vec![Rect::new(0.0, 0.0, 400.0, 32.0)],
            close: Some(Rect::new(370.0, 0.0, 30.0, 32.0)),
            ..HitRegions::default()
        };
        merge_hit_regions(
            &mut stored,
            HitRegions {
                resize_borders: band(24.0),
                ..HitRegions::default()
            },
        );
        assert_eq!(stored.drag.len(), 1, "the drag rect survived");
        assert!(stored.close.is_some(), "the close button survived");
        assert_eq!(stored.resize_borders.top, 24.0);
    }

    /// A chrome snapshot replaces wholesale — including clearing a stale band,
    /// so a frame that stops publishing is not remembered forever.
    #[test]
    fn merging_a_chrome_snapshot_replaces_everything() {
        let mut stored = HitRegions {
            resize_borders: band(24.0),
            close: Some(Rect::new(370.0, 0.0, 30.0, 32.0)),
            ..HitRegions::default()
        };
        merge_hit_regions(&mut stored, HitRegions::default());
        assert_eq!(stored.resize_borders.top, 0.0);
        assert!(stored.close.is_none());
    }
}
