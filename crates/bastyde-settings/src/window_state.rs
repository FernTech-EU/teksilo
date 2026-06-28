// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Per-window geometry persistence via [`WindowStateService`].
//!
//! Each named window — identified by a stable string label such as
//! `"main"` or `"inspector"` — can have its position, size, and
//! placement (`Floating` / `Maximized` / `Fullscreen`) saved across
//! sessions. In-memory is the source of truth: [`record`] updates an
//! in-memory [`WindowStateFile`] and schedules a debounced atomic write
//! (write-temp + rename); [`state_for`] reads directly from memory
//! without touching disk. On load, the file is migrated through
//! [`Migrator`] steps (currently v1 → v2: `maximized: bool` →
//! `placement: WindowPlacement`) before deserializing, and corrupt
//! files are quarantined automatically by [`SettingsFile`].
//!
//! In a typical Bastyde app, `WindowStateService` is managed by the
//! framework's `SettingsBundle` and wired automatically when the
//! `WindowConfig` carries a stable `id(...)` — no widget-side plumbing
//! needed. The service is only used directly when building custom window
//! management or embedding it outside the standard `BastydeAppBuilder`
//! path.
//!
//! ## Wayland caveat
//!
//! Wayland does not let applications choose their window position;
//! the compositor places windows. Position fields (`x`, `y`) are still
//! recorded and persisted (so the config roams across an X11/Wayland
//! switch), but a Wayland host must ignore them when restoring.
//! Width, height, and [`WindowPlacement`] are honored on every platform.
//!
//! ## Example
//!
//! ```ignore
//! use std::time::Duration;
//! use bastyde_settings::{AppPaths, WindowStateService, PerWindowState};
//! use bastyde_core::WindowPlacement;
//!
//! // In tests use AppPaths::for_testing(tmp_dir); in production use AppPaths::new(...).
//! let paths = AppPaths::for_testing(std::path::Path::new("/tmp/my-app"));
//! let svc = WindowStateService::open_with_delay(&paths, Duration::ZERO).unwrap();
//!
//! // On window move / resize, record the new geometry.
//! svc.record(PerWindowState {
//!     label: "main".into(),
//!     x: 100, y: 80,
//!     width: 1280, height: 800,
//!     placement: WindowPlacement::Floating,
//! }).unwrap();
//!
//! // On next launch, restore if available.
//! if let Some(saved) = svc.state_for("main") {
//!     let ready = saved.sanitize((400, 300), (1920, 1080));
//!     println!("restore to {}x{} at ({},{})", ready.width, ready.height, ready.x, ready.y);
//! }
//! ```
//!
//! [`record`]: WindowStateService::record
//! [`state_for`]: WindowStateService::state_for

use std::path::{Path, PathBuf};
use std::time::Duration;

use bastyde_core::WindowPlacement;
use serde::{Deserialize, Serialize};

use crate::file::{SettingsFile, SettingsFileError};
use crate::migration::{MigrationError, Migrator, Versioned};
use crate::path::AppPaths;
use crate::store::DEFAULT_DEBOUNCE;

/// Persisted geometry for one labeled window.
///
/// `placement` captures the full `WindowPlacement` enum (Floating /
/// Maximized / Fullscreen / Minimized). On restore, `Minimized` is
/// downgraded to `Floating` so the app doesn't appear to fail to
/// start; every other variant is honored if the OS supports it
/// (Wayland will ignore `position` regardless — see `sanitize`'s
/// docs).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PerWindowState {
    pub label: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub placement: WindowPlacement,
}

impl PerWindowState {
    /// Validate this state against a `(width, height)` work area
    /// (typically the size of the largest available monitor's usable
    /// region) and return a sanitized copy:
    ///
    /// * `width` / `height` are clamped to `[min, work_area]`. If the
    ///   minimum is larger than the work area the work area wins —
    ///   this should not happen with sensible mins (e.g. 320x240).
    /// * The position is checked: if the **top-left** point lies
    ///   outside `[0, work_area_w) x [0, work_area_h)` *and* the
    ///   window would not have at least 50 logical-pixel intersection
    ///   with the work area, the position is recentered on the
    ///   monitor so the window comes back on screen instead of
    ///   spawning at coordinates from a missing monitor.
    /// * `maximized` and `label` are preserved.
    ///
    /// Use this on app startup with `(work_area_w, work_area_h)`
    /// pulled from the OS (e.g. winit's `MonitorHandle::size()` minus
    /// known taskbars). Without an OS hint, pass conservative
    /// fallbacks like `(1920, 1080)` — the result still improves on
    /// re-using stale coordinates from a monitor that's no longer
    /// connected.
    pub fn sanitize(&self, min_size: (u32, u32), work_area: (u32, u32)) -> PerWindowState {
        let (min_w, min_h) = min_size;
        let (max_w, max_h) = work_area;

        let width = clamp_size(self.width, min_w, max_w);
        let height = clamp_size(self.height, min_h, max_h);

        // Compute the intersection between the saved rectangle and
        // the work area, axis by axis. Recenter only the axes that
        // actually fall short of `MIN_VISIBLE_PX` — preserving the
        // user's position on any axis that's still on-screen.
        const MIN_VISIBLE_PX: i32 = 50;
        let saved_right = self.x.saturating_add(width as i32);
        let saved_bottom = self.y.saturating_add(height as i32);
        let visible_w = saved_right.min(max_w as i32) - self.x.max(0);
        let visible_h = saved_bottom.min(max_h as i32) - self.y.max(0);

        let x = if visible_w < MIN_VISIBLE_PX {
            ((max_w as i32) - (width as i32)).max(0) / 2
        } else {
            self.x
        };
        let y = if visible_h < MIN_VISIBLE_PX {
            ((max_h as i32) - (height as i32)).max(0) / 2
        } else {
            self.y
        };

        // Minimized is downgraded on restore: a window that comes
        // back invisible looks like the app failed to start. Every
        // other placement variant round-trips.
        let placement = match self.placement {
            WindowPlacement::Minimized => WindowPlacement::Floating,
            other => other,
        };

        PerWindowState {
            label: self.label.clone(),
            x,
            y,
            width,
            height,
            placement,
        }
    }
}

fn clamp_size(value: u32, min: u32, max: u32) -> u32 {
    if max < min {
        // Pathological hint — return the larger of the two so we
        // don't go below the user's declared minimum, and don't
        // produce a 0-sized window.
        return min.max(1);
    }
    value.clamp(min, max).max(1)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct WindowStateFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "Vec::new")]
    pub windows: Vec<PerWindowState>,
}

fn default_version() -> u32 {
    WindowStateFile::CURRENT_VERSION
}

impl Default for WindowStateFile {
    fn default() -> Self {
        Self {
            version: WindowStateFile::CURRENT_VERSION,
            windows: Vec::new(),
        }
    }
}

impl Versioned for WindowStateFile {
    /// v1: `maximized: bool`
    /// v2: `placement: WindowPlacement` (full enum). Migration step
    ///     below converts `maximized = true` → `placement = "Maximized"`.
    const CURRENT_VERSION: u32 = 2;
    fn version(&self) -> u32 {
        self.version
    }
    fn set_version(&mut self, v: u32) {
        self.version = v;
    }
}

/// v1 → v2: replace each window entry's `maximized: bool` with
/// `placement: WindowPlacement` (`"Maximized"` if `maximized = true`,
/// `"Floating"` otherwise). The bool is dropped from the v2 shape.
fn migrate_v1_to_v2(mut raw: toml::Value) -> Result<toml::Value, String> {
    let table = raw
        .as_table_mut()
        .ok_or_else(|| "WindowStateFile root is not a table".to_string())?;

    if let Some(windows) = table.get_mut("windows").and_then(|v| v.as_array_mut()) {
        for entry in windows {
            let Some(entry_table) = entry.as_table_mut() else {
                continue;
            };
            let was_maximized = entry_table
                .get("maximized")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            entry_table.remove("maximized");
            entry_table.insert(
                "placement".into(),
                toml::Value::String(
                    if was_maximized {
                        "Maximized"
                    } else {
                        "Floating"
                    }
                    .into(),
                ),
            );
        }
    }
    Ok(raw)
}

fn make_migrator() -> Migrator<WindowStateFile> {
    Migrator::new().step(1, migrate_v1_to_v2)
}

/// Document the migration error type publicly so callers can pattern
/// match — used in tests below.
#[allow(dead_code)]
fn _migration_error_is_exported(_: MigrationError) {}

/// Persistent, in-memory-backed store for per-window geometry.
///
/// Holds a [`SettingsFile`]-backed collection of [`PerWindowState`] entries
/// keyed by a stable string label. Each [`record`] call updates the
/// in-memory snapshot and schedules a debounced atomic flush to disk;
/// [`state_for`] reads directly from memory with no I/O.
///
/// [`record`]: WindowStateService::record
/// [`state_for`]: WindowStateService::state_for
#[derive(Clone)]
pub struct WindowStateService {
    file: SettingsFile<WindowStateFile>,
}

impl WindowStateService {
    /// Open the window-state file at the standard location inside `paths`,
    /// using the default debounce delay.
    pub fn open(paths: &AppPaths) -> Result<Self, SettingsFileError> {
        Self::open_with_delay(paths, DEFAULT_DEBOUNCE)
    }

    /// Open the window-state file at the standard location inside `paths`,
    /// flushing debounced writes after `delay`. Pass `Duration::ZERO` in
    /// tests for synchronous behaviour.
    pub fn open_with_delay(paths: &AppPaths, delay: Duration) -> Result<Self, SettingsFileError> {
        let file = SettingsFile::load(paths.data_file("window_state"), delay, &make_migrator())?;
        Ok(Self { file })
    }

    /// Open the window-state file at an explicit `path` with the given
    /// debounce `delay`. Useful when the caller controls the file location
    /// directly (e.g. integration tests writing to a known temp path).
    pub fn open_at(path: PathBuf, delay: Duration) -> Result<Self, SettingsFileError> {
        let file = SettingsFile::load(path, delay, &make_migrator())?;
        Ok(Self { file })
    }

    /// Saved state for the window with `label`, or `None` if there's
    /// no entry yet.
    pub fn state_for(&self, label: &str) -> Option<PerWindowState> {
        self.file
            .borrow()
            .windows
            .iter()
            .find(|w| w.label == label)
            .cloned()
    }

    /// Record the current geometry for `label`. Replaces any prior
    /// entry. Schedules a debounced write.
    pub fn record(&self, state: PerWindowState) -> Result<(), SettingsFileError> {
        self.file.mutate(|file| {
            if let Some(existing) = file.windows.iter_mut().find(|w| w.label == state.label) {
                *existing = state;
            } else {
                file.windows.push(state);
            }
        })
    }

    /// Forget the entry for `label`.
    pub fn forget(&self, label: &str) -> Result<(), SettingsFileError> {
        self.file.mutate(|file| {
            file.windows.retain(|w| w.label != label);
        })
    }

    /// All recorded labels. Useful for "restore last session" features.
    pub fn labels(&self) -> Vec<String> {
        self.file
            .borrow()
            .windows
            .iter()
            .map(|w| w.label.clone())
            .collect()
    }

    /// Write any pending in-memory changes to disk immediately, bypassing
    /// the debounce timer. Useful before process exit or in tests that
    /// verify on-disk content.
    pub fn flush_now(&self) -> Result<(), SettingsFileError> {
        self.file.flush_now()
    }

    /// Absolute path of the underlying TOML file managed by this service.
    pub fn path(&self) -> &Path {
        self.file.path()
    }
}

impl std::fmt::Debug for WindowStateService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowStateService")
            .field("path", &self.file.path())
            .field("labels", &self.labels())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open(dir: &Path) -> WindowStateService {
        let paths = AppPaths::for_testing(dir);
        WindowStateService::open_with_delay(&paths, Duration::ZERO).unwrap()
    }

    #[test]
    fn record_then_recall() {
        let dir = tempdir().unwrap();
        let svc = open(dir.path());

        svc.record(PerWindowState {
            label: "main".into(),
            x: 100,
            y: 200,
            width: 800,
            height: 600,
            placement: WindowPlacement::Floating,
        })
        .unwrap();

        let got = svc.state_for("main").unwrap();
        assert_eq!(got.x, 100);
        assert_eq!(got.width, 800);
    }

    #[test]
    fn record_replaces_existing_entry() {
        let dir = tempdir().unwrap();
        let svc = open(dir.path());

        for i in 0..3 {
            svc.record(PerWindowState {
                label: "main".into(),
                x: i,
                y: 0,
                width: 100,
                height: 100,
                placement: WindowPlacement::Floating,
            })
            .unwrap();
        }
        assert_eq!(svc.labels(), vec!["main".to_string()]);
        assert_eq!(svc.state_for("main").unwrap().x, 2);
    }

    #[test]
    fn multiple_windows_independent() {
        let dir = tempdir().unwrap();
        let svc = open(dir.path());

        svc.record(PerWindowState {
            label: "main".into(),
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            placement: WindowPlacement::Floating,
        })
        .unwrap();
        svc.record(PerWindowState {
            label: "log".into(),
            x: 1000,
            y: 0,
            width: 400,
            height: 800,
            placement: WindowPlacement::Maximized,
        })
        .unwrap();

        assert_eq!(svc.state_for("main").unwrap().width, 100);
        assert_eq!(
            svc.state_for("log").unwrap().placement,
            WindowPlacement::Maximized
        );
    }

    #[test]
    fn forget_removes_entry() {
        let dir = tempdir().unwrap();
        let svc = open(dir.path());
        svc.record(PerWindowState {
            label: "main".into(),
            x: 0,
            y: 0,
            width: 100,
            height: 100,
            placement: WindowPlacement::Floating,
        })
        .unwrap();
        svc.forget("main").unwrap();
        assert!(svc.state_for("main").is_none());
    }

    // -- sanitize --------------------------------------------------------

    fn sample(x: i32, y: i32, w: u32, h: u32) -> PerWindowState {
        PerWindowState {
            label: "main".into(),
            x,
            y,
            width: w,
            height: h,
            placement: WindowPlacement::Floating,
        }
    }

    #[test]
    fn sanitize_clamps_oversized_window() {
        // Saved 3000x2000 (e.g., user had a 4K monitor) but now on
        // 1920x1080 — width and height should clamp.
        let s = sample(0, 0, 3000, 2000).sanitize((400, 300), (1920, 1080));
        assert_eq!(s.width, 1920);
        assert_eq!(s.height, 1080);
    }

    #[test]
    fn sanitize_promotes_undersized_window_to_min() {
        let s = sample(0, 0, 100, 100).sanitize((400, 300), (1920, 1080));
        assert_eq!(s.width, 400);
        assert_eq!(s.height, 300);
    }

    #[test]
    fn sanitize_recenters_offscreen_position() {
        // Saved on a now-disconnected secondary monitor at x=2200.
        let s = sample(2200, 100, 800, 600).sanitize((320, 240), (1920, 1080));
        // Center: (1920-800)/2 = 560
        assert_eq!(s.x, 560);
        // y had 100px overlap; y stays.
        assert_eq!(s.y, 100);
    }

    #[test]
    fn sanitize_keeps_position_when_visible_enough() {
        // Window at (1500, 100), 800x600 on a 1920x1080. Right edge
        // is at 2300 (off-screen) but plenty visible on the left
        // — keep position.
        let s = sample(1500, 100, 800, 600).sanitize((320, 240), (1920, 1080));
        assert_eq!(s.x, 1500);
        assert_eq!(s.y, 100);
    }

    #[test]
    fn sanitize_recenters_when_top_left_is_negative() {
        let s = sample(-2000, -2000, 800, 600).sanitize((320, 240), (1920, 1080));
        // x recenter: (1920-800)/2 = 560
        assert_eq!(s.x, 560);
        // y recenter: (1080-600)/2 = 240
        assert_eq!(s.y, 240);
    }

    #[test]
    fn sanitize_preserves_maximized_and_label() {
        let mut p = sample(0, 0, 800, 600);
        p.placement = WindowPlacement::Maximized;
        p.label = "log".into();
        let s = p.sanitize((320, 240), (1920, 1080));
        assert_eq!(s.placement, WindowPlacement::Maximized);
        assert_eq!(s.label, "log");
    }

    #[test]
    fn sanitize_preserves_fullscreen() {
        let mut p = sample(0, 0, 800, 600);
        p.placement = WindowPlacement::Fullscreen;
        let s = p.sanitize((320, 240), (1920, 1080));
        assert_eq!(s.placement, WindowPlacement::Fullscreen);
    }

    #[test]
    fn sanitize_downgrades_minimized_to_floating() {
        // A window saved while minimized must come back visible —
        // otherwise the user thinks the app failed to start.
        let mut p = sample(100, 100, 800, 600);
        p.placement = WindowPlacement::Minimized;
        let s = p.sanitize((320, 240), (1920, 1080));
        assert_eq!(s.placement, WindowPlacement::Floating);
    }

    #[test]
    fn sanitize_handles_pathological_min_above_work_area() {
        // If min > max, neither produces a sensible window. We guard
        // against zero-sized output by clamping width to at least 1.
        let s = sample(0, 0, 5000, 5000).sanitize((400, 300), (200, 200));
        assert!(s.width > 0 && s.height > 0);
    }

    #[test]
    fn migrates_v1_maximized_bool_to_v2_placement_enum() {
        // Hand-write a v1 file (`maximized: bool`) and verify
        // `WindowStateService::open` runs the migration on read,
        // producing a v2 in-memory representation with the matching
        // placement enum.
        let dir = tempdir().unwrap();
        let path = dir.path().join("window_state.toml");
        std::fs::write(
            &path,
            "version = 1\n\n\
             [[windows]]\n\
             label = \"main\"\n\
             x = 100\n\
             y = 200\n\
             width = 800\n\
             height = 600\n\
             maximized = true\n\n\
             [[windows]]\n\
             label = \"log\"\n\
             x = 0\n\
             y = 0\n\
             width = 400\n\
             height = 300\n\
             maximized = false\n",
        )
        .unwrap();

        let svc = WindowStateService::open_at(path.clone(), Duration::ZERO).unwrap();
        assert_eq!(
            svc.state_for("main").unwrap().placement,
            WindowPlacement::Maximized
        );
        assert_eq!(
            svc.state_for("log").unwrap().placement,
            WindowPlacement::Floating
        );

        // Mutate to trigger a flush of the migrated structure, then
        // verify the on-disk shape is now v2 (no `maximized` field).
        svc.forget("log").unwrap();
        svc.flush_now().unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: toml::Value = toml::from_str(&raw).unwrap();
        assert_eq!(parsed.get("version").and_then(|v| v.as_integer()), Some(2));
        let win = parsed
            .get("windows")
            .and_then(|w| w.as_array())
            .and_then(|a| a.first())
            .unwrap();
        assert!(
            win.get("maximized").is_none(),
            "maximized field should be gone in v2"
        );
        assert_eq!(
            win.get("placement").and_then(|v| v.as_str()),
            Some("Maximized"),
        );
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tempdir().unwrap();
        {
            let svc = open(dir.path());
            svc.record(PerWindowState {
                label: "main".into(),
                x: 50,
                y: 50,
                width: 1024,
                height: 768,
                placement: WindowPlacement::Fullscreen,
            })
            .unwrap();
            svc.flush_now().unwrap();
        }

        let svc = open(dir.path());
        let got = svc.state_for("main").unwrap();
        assert_eq!(got.width, 1024);
        assert_eq!(got.height, 768);
        assert_eq!(got.placement, WindowPlacement::Fullscreen);
    }
}
