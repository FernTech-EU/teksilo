// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Per-window geometry persistence via [`WindowStateService`].
//!
//! Each named window — identified by a stable string label such as
//! `"main"` or `"inspector"` — can have its position, size, and
//! placement (`Floating` / `Maximized` / `Fullscreen`) saved across
//! sessions. In-memory is the source of truth: [`state_for`] reads
//! directly from memory without touching disk. On load, the file is
//! migrated through [`Migrator`] steps (currently v1 → v2: `maximized:
//! bool` → `placement: WindowPlacement`) before deserializing, and
//! corrupt files are quarantined automatically by [`SettingsFile`](crate::SettingsFile).
//!
//! ## `record` is debounced, not synchronous
//!
//! See [`WindowStateService`]'s "Why this is debounced, unlike
//! `SettingsFile`" doc below for the full rationale: [`record`]/[`forget`]
//! update the in-memory state instantly and schedule a coalesced, locked
//! read-merge-write via a [`DebouncedWriter`] — a live window drag (which
//! calls [`record`] once per reported geometry frame) costs one disk
//! write per debounce window, not one per frame.
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
//! [`forget`]: WindowStateService::forget
//! [`state_for`]: WindowStateService::state_for
//! [`DebouncedWriter`]: crate::flush::DebouncedWriter

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use bastyde_core::WindowPlacement;
use serde::{Deserialize, Serialize};

use crate::DEFAULT_DEBOUNCE;
use crate::file::{SettingsFileError, disk_stamp, read_toml_with_retry};
use crate::flush::{DebouncedWriter, FlushError};
use crate::migration::{MigrationError, Migrator, Versioned};
use crate::path::AppPaths;
use crate::reload::Reloadable;

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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
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

/// Read + migrate the document at `path`, or `default` when it does not exist.
fn read_window_state_or_default(
    path: &Path,
    migrator: &Migrator<WindowStateFile>,
) -> Result<WindowStateFile, SettingsFileError> {
    match read_toml_with_retry(path)? {
        Some(raw) => {
            let mut file = migrator.run(raw).map_err(SettingsFileError::Migrate)?;
            file.version = <WindowStateFile as Versioned>::CURRENT_VERSION;
            Ok(file)
        }
        None => Ok(WindowStateFile::default()),
    }
}

/// Like [`read_window_state_or_default`], but for text already in hand — which
/// is what a [`crate::flush::Patch`] closure receives (the current file text,
/// read by the worker under the lock) rather than a path to re-read.
fn parse_window_state_text(
    text: Option<&str>,
    migrator: &Migrator<WindowStateFile>,
) -> Result<WindowStateFile, SettingsFileError> {
    match text {
        Some(text) => {
            let raw: toml::Value = toml::from_str(text).map_err(SettingsFileError::Parse)?;
            let mut file = migrator.run(raw).map_err(SettingsFileError::Migrate)?;
            file.version = <WindowStateFile as Versioned>::CURRENT_VERSION;
            Ok(file)
        }
        None => Ok(WindowStateFile::default()),
    }
}

/// Document the migration error type publicly so callers can pattern
/// match — used in tests below.
#[allow(dead_code)]
fn _migration_error_is_exported(_: MigrationError) {}

/// One replayable mutation of the window-state document.
///
/// Pure owned data (a `PerWindowState`, a label), so it is `Send` and can be
/// captured into a [`crate::flush::Patch`] and applied on the writer thread to
/// the document *just read under the lock* — which is what lets two processes
/// record two different windows without either erasing the other.
#[derive(Clone, Debug)]
enum WindowOp {
    Set(Box<PerWindowState>),
    Forget(String),
}

fn apply_window_op(windows: &mut Vec<PerWindowState>, op: &WindowOp) {
    match op {
        WindowOp::Set(state) => match windows.iter_mut().find(|w| w.label == state.label) {
            Some(existing) => *existing = (**state).clone(),
            None => windows.push((**state).clone()),
        },
        WindowOp::Forget(label) => windows.retain(|w| w.label != *label),
    }
}

/// Persistent, in-memory-backed store for per-window geometry.
///
/// Entries are [`PerWindowState`], keyed by a stable string label. [`state_for`]
/// reads straight from memory with no I/O.
///
/// ## Why this is debounced, unlike [`SettingsFile`](crate::SettingsFile)
///
/// `SettingsFile`'s `mutate` is a *synchronous* locked read-modify-write, which
/// is right for a document written rarely (a settings change; one record per
/// backup). Window geometry is the opposite: `bastyde-app`'s `window_persist`
/// observes the `size` / `position` / `placement` signals and calls [`record`]
/// on **every change** — i.e. once per frame while the user drags a window. A
/// synchronous `flock` + read + parse + serialize + fsync per frame would make
/// dragging visibly janky.
///
/// So this service owns its own [`DebouncedWriter`] and schedules a
/// `WindowOp` patch per `record`, exactly like [`crate::PersistedListModel`]:
/// in-memory state updates instantly (so `state_for` is always current), and
/// the burst collapses into **one** locked read-merge-write at the debounce
/// deadline. Frequent writes ⇒ debounced patch; rare writes ⇒ synchronous
/// locked RMW. Both are cross-process correct; they differ only in when the
/// disk write happens.
///
/// [`record`]: WindowStateService::record
/// [`state_for`]: WindowStateService::state_for
#[derive(Clone)]
pub struct WindowStateService {
    /// Instant, authoritative-for-reads view. Kept in step with every op.
    current: Rc<RefCell<WindowStateFile>>,
    writer: Rc<DebouncedWriter>,
    migrator: Migrator<WindowStateFile>,
    /// `(mtime, len)` of the last write *we* made — so the file watcher can tell
    /// a peer's write from the echo of our own and not reload pointlessly.
    last_known_stamp: Rc<Cell<(Option<SystemTime>, Option<u64>)>>,
    /// The REAL post-write `(mtime, len)` stamp of our own most recent
    /// debounced write, delivered by [`DebouncedWriter`]'s
    /// [`WriteLandedSink`](crate::flush::WriteLandedSink) the instant it
    /// lands on the shared worker thread — `Arc<Mutex<_>>`, not `Rc<Cell<_>>`,
    /// because it is written from that (non-UI) thread. `reload_from_disk`
    /// drains it (adopting the value into `last_known_stamp`) before doing
    /// its own `disk_stamp` comparison, so a debounced `apply()` write no
    /// longer looks like a peer's change and forces a wasted re-parse (F11).
    pending_write_stamp: Arc<Mutex<Option<crate::flush::LandedStamp>>>,
}

impl WindowStateService {
    /// Open the window-state file at the standard location inside `paths`.
    pub fn open(paths: &AppPaths) -> Result<Self, SettingsFileError> {
        Self::open_at(paths.data_file("window_state"), DEFAULT_DEBOUNCE)
    }

    /// Open at the standard location with an explicit debounce window.
    pub fn open_with_delay(paths: &AppPaths, delay: Duration) -> Result<Self, SettingsFileError> {
        Self::open_at(paths.data_file("window_state"), delay)
    }

    /// Open the window-state file at an explicit `path`.
    ///
    /// `delay` is the debounce window: geometry changes arriving inside it
    /// coalesce into a single disk write. `Duration::ZERO` writes on the
    /// worker's next tick (used by tests).
    pub fn open_at(path: PathBuf, delay: Duration) -> Result<Self, SettingsFileError> {
        let migrator = make_migrator();
        let current = read_window_state_or_default(&path, &migrator)?;
        let stamp = disk_stamp(&path);
        let writer = Rc::new(DebouncedWriter::new(path, delay));
        let pending_write_stamp = Arc::new(Mutex::new(None));
        let pending_write_stamp_for_sink = Arc::clone(&pending_write_stamp);
        writer.set_landed_sink(Arc::new(move |landed| {
            *pending_write_stamp_for_sink.lock().unwrap() = Some(landed);
        }));
        Ok(Self {
            current: Rc::new(RefCell::new(current)),
            writer,
            migrator,
            last_known_stamp: Rc::new(Cell::new(stamp)),
            pending_write_stamp,
        })
    }

    /// Saved state for the window with `label`, or `None` if there's no entry.
    pub fn state_for(&self, label: &str) -> Option<PerWindowState> {
        self.current
            .borrow()
            .windows
            .iter()
            .find(|w| w.label == label)
            .cloned()
    }

    /// Record the current geometry for `label`, replacing any prior entry.
    ///
    /// Updates memory immediately and schedules a debounced, locked
    /// read-merge-write — so a drag costs one write, not one per frame.
    pub fn record(&self, state: PerWindowState) -> Result<(), SettingsFileError> {
        self.apply(WindowOp::Set(Box::new(state)));
        Ok(())
    }

    /// Forget the entry for `label`.
    pub fn forget(&self, label: &str) -> Result<(), SettingsFileError> {
        self.apply(WindowOp::Forget(label.to_string()));
        Ok(())
    }

    /// All recorded labels. Useful for "restore last session" features.
    pub fn labels(&self) -> Vec<String> {
        self.current
            .borrow()
            .windows
            .iter()
            .map(|w| w.label.clone())
            .collect()
    }

    fn apply(&self, op: WindowOp) {
        apply_window_op(&mut self.current.borrow_mut().windows, &op);

        let migrator = self.migrator.clone();
        let patch: crate::flush::Patch = Box::new(move |current: Option<String>| {
            // Merge against the document as it is on disk RIGHT NOW, not against
            // this process's snapshot — a peer may have recorded its own window
            // in the meantime, and it must survive.
            let mut file = parse_window_state_text(current.as_deref(), &migrator)
                .map_err(|e| FlushError::Merge(e.to_string()))?;
            apply_window_op(&mut file.windows, &op);
            file.version = <WindowStateFile as Versioned>::CURRENT_VERSION;
            toml::to_string_pretty(&file).map_err(|e| FlushError::Merge(e.to_string()))
        });
        self.writer.schedule(patch);
    }

    /// Flush any pending geometry to disk immediately, bypassing the debounce.
    ///
    /// Flushes the **op queue**, never a re-derived snapshot of the in-memory
    /// document — dumping the snapshot is exactly how a cleanly-exiting process
    /// would erase a peer's window entry.
    pub fn flush_now(&self) -> Result<(), SettingsFileError> {
        self.writer.flush_now().map_err(SettingsFileError::Flush)?;
        self.last_known_stamp.set(disk_stamp(self.writer.path()));
        Ok(())
    }

    /// Absolute path of the underlying TOML file managed by this service.
    pub fn path(&self) -> &Path {
        self.writer.path()
    }
}

impl Reloadable for WindowStateService {
    fn path(&self) -> &Path {
        WindowStateService::path(self)
    }

    /// Pick up a peer process's window entry.
    ///
    /// Merging is trivially safe here: entries are keyed by `label`, and
    /// distinct labels (distinct windows) never conflict — so whatever a peer
    /// wrote simply appears. The `(mtime, len)` stamp check short-circuits the
    /// echo of our *own* write, and the content comparison guarantees we never
    /// touch anything when nothing actually changed.
    fn reload_from_disk(&self) -> Result<bool, SettingsFileError> {
        // Adopt our own debounced write's REAL landed stamp, if one arrived
        // since we last looked (F11). This is exact, not probabilistic: the
        // value was produced by an actual `fs::metadata` call taken on the
        // worker thread immediately after our own write really landed, so
        // adopting it can never cause a later, genuinely distinct peer write
        // to be missed — the very next `disk_stamp` call below will differ
        // from this now-current `last_known_stamp` if anything further
        // changed on disk.
        if let Some(landed) = self.pending_write_stamp.lock().unwrap().take() {
            self.last_known_stamp.set(landed);
        }

        let path = self.writer.path();
        let current_stamp = disk_stamp(path);
        if current_stamp == self.last_known_stamp.get() {
            return Ok(false);
        }

        let file = read_window_state_or_default(path, &self.migrator)?;
        self.last_known_stamp.set(current_stamp);

        if *self.current.borrow() == file {
            return Ok(false);
        }
        *self.current.borrow_mut() = file;
        Ok(true)
    }
}

impl std::fmt::Debug for WindowStateService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowStateService")
            .field("path", &self.path())
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

    /// THE HEADLINE TEST for this type. Two independent `WindowStateService`
    /// handles over the *same* file — standing in for two Skribisto
    /// processes sharing `window_state.toml` — each record a *different*
    /// window label with no coordination between them. Because `record`
    /// always goes through the locked read-modify-write, both labels must
    /// survive: distinct labels never conflict, so the merge is trivially
    /// clean, but the *old* whole-snapshot design would still have let one
    /// handle's stale in-memory copy clobber the other's label entirely.
    #[test]
    fn two_handles_recording_different_labels_both_survive() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());

        let a = WindowStateService::open(&paths).unwrap();
        let b = WindowStateService::open(&paths).unwrap();

        a.record(PerWindowState {
            label: "main".into(),
            x: 10,
            y: 20,
            width: 800,
            height: 600,
            placement: WindowPlacement::Floating,
        })
        .unwrap();
        b.record(PerWindowState {
            label: "inspector".into(),
            x: 900,
            y: 20,
            width: 300,
            height: 600,
            placement: WindowPlacement::Floating,
        })
        .unwrap();

        // `record` is debounced (geometry changes arrive once per frame during a
        // drag), so force both queues out before reading the file back.
        a.flush_now().unwrap();
        b.flush_now().unwrap();

        // A third, fresh handle proves both labels are actually on disk
        // together.
        let c = WindowStateService::open(&paths).unwrap();
        let mut labels = c.labels();
        labels.sort();
        assert_eq!(labels, vec!["inspector".to_string(), "main".to_string()]);
        assert_eq!(c.state_for("main").unwrap().width, 800);
        assert_eq!(c.state_for("inspector").unwrap().width, 300);
    }

    /// A window drag fires `record` on every frame. Those must coalesce into
    /// **one** disk write, not one per frame.
    ///
    /// This is a regression guard: `record` originally went through
    /// `SettingsFile::mutate`, whose write is a *synchronous* locked
    /// read-modify-write (right for rarely-written documents like `backup.toml`,
    /// catastrophic here) — so a drag did a `flock` + read + parse + serialize +
    /// fsync **per frame**.
    #[test]
    fn a_burst_of_records_coalesces_into_one_write() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("window_state.toml");
        // A real debounce window, so the burst genuinely has something to
        // coalesce into.
        let svc = WindowStateService::open_at(path.clone(), Duration::from_millis(50)).unwrap();

        for i in 0..60 {
            svc.record(PerWindowState {
                label: "main".into(),
                x: i,
                y: i,
                width: 800,
                height: 600,
                placement: WindowPlacement::Floating,
            })
            .unwrap();
        }

        // Nothing has touched the disk yet: 60 frames of dragging, zero writes.
        assert!(
            !path.exists(),
            "a burst of records must not write per-record"
        );

        svc.flush_now().unwrap();

        // One write, carrying the LAST geometry — no intermediate frame leaked.
        let on_disk = read_window_state_or_default(&path, &make_migrator()).unwrap();
        assert_eq!(on_disk.windows.len(), 1);
        assert_eq!(on_disk.windows[0].x, 59);
        // ...and memory agreed all along, without any I/O.
        assert_eq!(svc.state_for("main").unwrap().x, 59);
    }

    #[test]
    fn reload_from_disk_picks_up_a_peers_recorded_label() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());

        let a = WindowStateService::open(&paths).unwrap();
        let b = WindowStateService::open(&paths).unwrap();

        a.record(PerWindowState {
            label: "main".into(),
            x: 1,
            y: 2,
            width: 111,
            height: 222,
            placement: WindowPlacement::Floating,
        })
        .unwrap();
        a.flush_now().unwrap(); // `record` is debounced — force it to disk

        assert!(b.state_for("main").is_none(), "b hasn't reloaded yet");
        assert!(Reloadable::reload_from_disk(&b).unwrap());
        assert_eq!(b.state_for("main").unwrap().width, 111);
    }

    /// Wait (bounded) for `svc`'s own debounced write to land on the shared
    /// worker thread — confirmed by its `WriteLandedSink` callback actually
    /// firing (`pending_write_stamp` becomes `Some`), not merely by the file
    /// existing (which can be observed by this thread a hair before the
    /// worker thread has finished running the sink in the same tick).
    ///
    /// Deliberately does **not** use `flush_now()`: that method independently
    /// sets `last_known_stamp` itself (pre-existing code, unrelated to the
    /// F11 fix), which would mask whether the fix under test did anything at
    /// all. Polling `pending_write_stamp` directly exercises exactly the
    /// natural, un-forced debounce path a live geometry drag takes.
    fn wait_for_own_write_to_land(svc: &WindowStateService) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if svc.pending_write_stamp.lock().unwrap().is_some() {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "debounced write never landed"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// THE F11 REGRESSION TEST. Proves `reload_from_disk` adopts our own
    /// debounced write's REAL landed stamp (via `WriteLandedSink`) into
    /// `last_known_stamp` *before* doing its disk-stamp staleness check —
    /// and that the check that follows is driven purely by the `(mtime,
    /// len)` stamp, never by re-reading content.
    ///
    /// We corrupt the on-disk file to invalid TOML but force its `(mtime,
    /// len)` stamp to exactly match what our own write just produced (same
    /// byte length, `File::set_modified` restores the exact mtime). Before
    /// the fix, `last_known_stamp` is never refreshed by `apply()` — it is
    /// still whatever `open_at` captured before any write happened — so it
    /// cannot match *any* post-write stamp, forcing `reload_from_disk` to
    /// attempt `read_window_state_or_default` on the now-corrupted file and
    /// propagate a parse error. After the fix, the adopted stamp matches the
    /// (deliberately stamp-preserved) corrupted file exactly, so the method
    /// short-circuits to `Ok(false)` and the corrupted bytes are never
    /// parsed at all.
    #[test]
    fn reload_from_disk_short_circuits_via_adopted_landed_stamp_not_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("window_state.toml");
        let svc = WindowStateService::open_at(path.clone(), Duration::ZERO).unwrap();

        svc.record(PerWindowState {
            label: "main".into(),
            x: 1,
            y: 2,
            width: 111,
            height: 222,
            placement: WindowPlacement::Floating,
        })
        .unwrap();
        wait_for_own_write_to_land(&svc);

        let good_bytes = std::fs::read(&path).unwrap();
        let good_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();

        // Corrupt the content but preserve the exact (mtime, len) stamp —
        // proving the short-circuit below is stamp-driven, not content-driven.
        let mut garbage = vec![b'x'; good_bytes.len()];
        garbage[0] = b'['; // still not valid TOML syntax
        assert_eq!(garbage.len(), good_bytes.len(), "must preserve `len`");
        std::fs::write(&path, &garbage).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(good_mtime)
            .unwrap();
        assert_eq!(
            disk_stamp(&path),
            (Some(good_mtime), Some(good_bytes.len() as u64)),
            "test setup must reproduce the exact landed stamp"
        );
        // Sanity: the corrupted bytes really are invalid TOML, so a
        // read+parse attempt (the pre-fix behavior) would error out.
        assert!(toml::from_str::<toml::Value>(&String::from_utf8(garbage).unwrap()).is_err());

        let result = Reloadable::reload_from_disk(&svc);
        assert!(
            matches!(result, Ok(false)),
            "expected Ok(false) via the adopted landed-stamp short-circuit \
             (corrupted content must never be read), got {result:?}"
        );
    }

    /// Correctness guard alongside the test above: the stamp-adoption fast
    /// path must NOT swallow a genuine peer write that lands *after* our own.
    #[test]
    fn reload_from_disk_still_detects_a_peer_write_after_our_own_lands() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());

        let a = WindowStateService::open_with_delay(&paths, Duration::ZERO).unwrap();
        a.record(PerWindowState {
            label: "main".into(),
            x: 1,
            y: 2,
            width: 111,
            height: 222,
            placement: WindowPlacement::Floating,
        })
        .unwrap();
        wait_for_own_write_to_land(&a);

        // A second, independent handle over the same file stands in for a
        // peer process recording a *different* window label after `a`'s own
        // write already landed.
        let b = WindowStateService::open_with_delay(&paths, Duration::ZERO).unwrap();
        b.record(PerWindowState {
            label: "inspector".into(),
            x: 9,
            y: 9,
            width: 300,
            height: 400,
            placement: WindowPlacement::Floating,
        })
        .unwrap();
        wait_for_own_write_to_land(&b);

        // `a` never called `reload_from_disk` before now, so its own
        // (already-drained-on-next-call) landed stamp is still pending — the
        // fast path must still notice that the *current* on-disk stamp has
        // moved on past it (because of `b`'s later write) and do a real
        // reload, not swallow the peer's change.
        assert!(Reloadable::reload_from_disk(&a).unwrap());
        assert_eq!(a.state_for("inspector").unwrap().width, 300);
        // ...and `a`'s own entry is still intact after picking up the peer's.
        assert_eq!(a.state_for("main").unwrap().width, 111);
    }

    #[test]
    fn reload_from_disk_returns_false_when_unchanged() {
        let dir = tempdir().unwrap();
        let svc = open(dir.path());
        assert!(!Reloadable::reload_from_disk(&svc).unwrap());
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
