// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Hearing the desktop's appearance change while the application runs: its
//! accent, its colour scheme, its light or dark preference, its fonts and
//! its animation speed.
//!
//! An [`OsThemeWatch`] listens on the D-Bus session bus, on a thread of its
//! own with a connection of its own, for the four signals a Linux desktop
//! sends when one of those changes:
//!
//! - the desktop portal's `org.freedesktop.portal.Settings.SettingChanged`
//!   on `/org/freedesktop/portal/desktop`, for a setting of the
//!   `org.freedesktop.appearance` namespace (colour scheme, accent colour,
//!   contrast, reduced motion) or of an `org.kde.kdeglobals.<Group>`
//!   namespace (KDE's palette, font and widget style); a setting of any
//!   other namespace is not heard;
//! - KDE's `org.kde.KGlobalSettings.notifyChange` on `/KGlobalSettings`;
//! - KDE's `org.kde.kconfig.notify.ConfigChanged` on `/kdeglobals` and on
//!   `/breezerc`, sent when either file is written.
//!
//! A signal is matched on its interface, member and path, from whichever
//! sender sends it. What a KDE signal says changed is not decoded: any one
//! of them means everything is read again.
//!
//! One change on the desktop usually arrives as several signals within a
//! few milliseconds (a new colour scheme is a `ConfigChanged`, a
//! `notifyChange` and several `SettingChanged`). The watch therefore
//! debounces: the first signal opens a window of [`DEBOUNCE`], and when the
//! window ends one [`OsThemeChanged`] is posted through
//! [`AppEventPoster::post_external`], whatever was heard during it. A signal
//! heard after the window ended opens another. Every signal is thus
//! followed by a payload posted no earlier than it, so the reading the
//! payload prompts sees every change signalled before it.
//!
//! The watch reads nothing itself; the payload's receiver queries the
//! desktop again ([`query_native_desktop`](super::query_native_desktop),
//! [`query_color_scheme`](super::query_color_scheme)).
//!
//! A session with no bus, or a signal that never came, is covered by
//! [`OsThemeWatch::files_changed`]: it compares the modification times of
//! the files of the `kdeglobals` and `breezerc` cascades with those it last
//! saw, for a caller to ask when a window takes focus.
//!
//! There is no watch elsewhere than on Linux: [`OsThemeWatch::new`] answers
//! `None`, and the window system's own light or dark notification is all
//! there is.

// Elsewhere than on Linux nothing starts a watch; the machinery is still
// compiled there, so its tests run on every platform.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

#[cfg(target_os = "linux")]
mod bus;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Waker};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use teksilo_core::AppEventPoster;
use teksilo_kde_scheme::KdeConfigLocations;

/// How long after the first signal of a change the watch posts its payload.
pub const DEBOUNCE: Duration = Duration::from_millis(150);

/// The portal's settings interface and object path, and its change signal.
const PORTAL_SETTINGS: &str = "org.freedesktop.portal.Settings";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTING_CHANGED: &str = "SettingChanged";
/// KDE's global settings change signal.
const GLOBAL_SETTINGS: &str = "org.kde.KGlobalSettings";
const GLOBAL_SETTINGS_PATH: &str = "/KGlobalSettings";
const NOTIFY_CHANGE: &str = "notifyChange";
/// KDE's configuration-file change signal, and the files it is heard for.
const KCONFIG_NOTIFY: &str = "org.kde.kconfig.notify";
const CONFIG_CHANGED: &str = "ConfigChanged";
const KDEGLOBALS_PATH: &str = "/kdeglobals";
const BREEZERC_PATH: &str = "/breezerc";

/// Every signal the watch listens to: interface, member and object path.
const LISTENED: [(&str, &str, &str); 4] = [
    (PORTAL_SETTINGS, SETTING_CHANGED, PORTAL_PATH),
    (GLOBAL_SETTINGS, NOTIFY_CHANGE, GLOBAL_SETTINGS_PATH),
    (KCONFIG_NOTIFY, CONFIG_CHANGED, KDEGLOBALS_PATH),
    (KCONFIG_NOTIFY, CONFIG_CHANGED, BREEZERC_PATH),
];

/// What an [`OsThemeWatch`] posts through
/// [`AppEventPoster::post_external`] once a change on the desktop has been
/// heard and its debounce window has ended. It carries nothing: the
/// receiver reads the desktop again, so posting one by any other means asks
/// for the same reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OsThemeChanged;

/// Watches the desktop's appearance; see the [module](self). Dropping it
/// stops the watch: its thread is told to stop and joined, and nothing is
/// posted once the drop has returned.
pub struct OsThemeWatch {
    /// The listening thread.
    _running: Running,
    /// The configuration files [`files_changed`](Self::files_changed)
    /// compares, as last seen.
    files: ConfigStamps,
}

impl std::fmt::Debug for OsThemeWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OsThemeWatch")
            .field("files", &self.files.files.len())
            .finish()
    }
}

impl OsThemeWatch {
    /// Start watching, posting each [`OsThemeChanged`] through `poster`
    /// from the watch's own thread. `None` on every platform but Linux, and
    /// when the thread cannot be started.
    ///
    /// The bus is connected to on the watch's thread, so this returns at
    /// once. A session with no bus leaves a watch that hears nothing, whose
    /// [`files_changed`](Self::files_changed) still answers.
    pub fn new(poster: impl AppEventPoster) -> Option<Self> {
        #[cfg(target_os = "linux")]
        {
            Self::on_bus(None, poster, |_| {})
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = poster;
            None
        }
    }

    /// [`new`](Self::new) on the bus at `address` (`None`: the session bus
    /// the environment names). `opened` is told on the watch's thread
    /// whether the bus was reached and every signal subscribed to, before
    /// any is heard.
    #[cfg(target_os = "linux")]
    fn on_bus(
        address: Option<String>,
        poster: impl AppEventPoster,
        opened: impl FnOnce(bool) + Send + 'static,
    ) -> Option<Self> {
        let locations = KdeConfigLocations::from_env(super::kde::process_env);
        let files = ConfigStamps::read(config_files(&locations), modified);
        let running = Running::start(
            DEBOUNCE,
            move |stop| {
                let source = bus::BusSource::connect(address.as_deref(), stop);
                opened(source.is_some());
                source
            },
            move || poster.post_external(Box::new(OsThemeChanged)),
        )?;
        Some(Self {
            _running: running,
            files,
        })
    }

    /// Whether a file of the `kdeglobals` or `breezerc` cascade was
    /// written, made or removed since the watch started or since the last
    /// call, by its modification time; the times read become the ones the
    /// next call compares with. For the main thread, when a window takes
    /// focus, and after an [`OsThemeChanged`] so that the change it was
    /// posted for is not counted twice.
    pub fn files_changed(&mut self) -> bool {
        self.files.refresh(modified)
    }
}

// ── Signals ─────────────────────────────────────────────────────────────────

/// Whether a signal with this interface, member and object path is one the
/// watch acts on. `namespace` is the first argument of a portal
/// `SettingChanged`, `None` when it could not be read.
fn signal_matters(interface: &str, member: &str, path: &str, namespace: Option<&str>) -> bool {
    match (interface, member, path) {
        (PORTAL_SETTINGS, SETTING_CHANGED, PORTAL_PATH) => {
            namespace.is_some_and(portal_namespace_matters)
        }
        (GLOBAL_SETTINGS, NOTIFY_CHANGE, GLOBAL_SETTINGS_PATH) => true,
        (KCONFIG_NOTIFY, CONFIG_CHANGED, KDEGLOBALS_PATH | BREEZERC_PATH) => true,
        _ => false,
    }
}

/// Whether a portal setting of `namespace` can change the theme or the
/// animation speed: `org.freedesktop.appearance`, and the
/// `org.kde.kdeglobals.<Group>` namespaces KDE's portal serves `kdeglobals`
/// under.
fn portal_namespace_matters(namespace: &str) -> bool {
    namespace == "org.freedesktop.appearance"
        || namespace
            .strip_prefix("org.kde.kdeglobals.")
            .is_some_and(|group| !group.is_empty())
}

// ── Debounce ────────────────────────────────────────────────────────────────

/// The debounce, on instants it is given: the first change opens a window
/// of `window`, later changes inside it do not move its end, and the window
/// is due once its end is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Debounce {
    window: Duration,
    due: Option<Instant>,
}

impl Debounce {
    fn new(window: Duration) -> Self {
        Self { window, due: None }
    }

    /// A change heard at `now`.
    fn heard(&mut self, now: Instant) {
        if self.due.is_none() {
            self.due = Some(now + self.window);
        }
    }

    /// When the open window ends; `None` while none is open.
    fn deadline(&self) -> Option<Instant> {
        self.due
    }

    /// Whether the open window has ended by `now`. A window that has is
    /// closed, so it is due once.
    fn take_due(&mut self, now: Instant) -> bool {
        match self.due {
            Some(due) if now >= due => {
                self.due = None;
                true
            }
            _ => false,
        }
    }
}

/// What waiting on a [`ChangeSource`] ended with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Heard {
    /// A change was heard.
    Change,
    /// The deadline passed first.
    Deadline,
    /// The source has closed, or the watch is stopping.
    Closed,
}

/// Where changes are heard from: the session bus, or a test's fake.
trait ChangeSource {
    /// Wait for the next change, until `deadline` (`None`: for as long as
    /// it takes), or until the source closes or the watch stops.
    fn wait(&mut self, deadline: Option<Instant>) -> Heard;
}

/// Hear changes from `source` and call `post` once at the end of each
/// debounce window, reading the time from `clock`, until the source closes
/// or `stop` is set. Nothing is posted once `stop` is set.
fn run(
    source: &mut impl ChangeSource,
    stop: &Stop,
    debounce: &mut Debounce,
    clock: &impl Fn() -> Instant,
    post: &impl Fn(),
) {
    loop {
        match source.wait(debounce.deadline()) {
            Heard::Change => debounce.heard(clock()),
            Heard::Deadline => {}
            Heard::Closed => return,
        }
        if stop.is_stopped() {
            return;
        }
        if debounce.take_due(clock()) {
            post();
        }
    }
}

// ── The thread ──────────────────────────────────────────────────────────────

/// The flag a watch is stopped by, with the waker of the future waiting on
/// it.
#[derive(Debug, Default)]
struct Stop {
    stopped: AtomicBool,
    waker: Mutex<Option<Waker>>,
}

impl Stop {
    /// Set the flag and wake whatever waits on it.
    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        let waker = self
            .waker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// Whether the flag is set, registering `cx`'s waker to be woken when it
    /// is. The waker is stored before the flag is read, so a stop between
    /// the two is either seen here or wakes it.
    fn poll_stopped(&self, cx: &Context<'_>) -> bool {
        *self.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(cx.waker().clone());
        self.is_stopped()
    }
}

/// The watch's thread; dropping it stops the thread and joins it.
struct Running {
    stop: Arc<Stop>,
    thread: Option<JoinHandle<()>>,
}

impl Running {
    /// Start the thread: it opens its source with `open` (`None`: nothing
    /// to listen to, and the thread ends), then [`run`]s it with a debounce
    /// window of `window`, calling `post` at the end of each.
    fn start<S: ChangeSource>(
        window: Duration,
        open: impl FnOnce(Arc<Stop>) -> Option<S> + Send + 'static,
        post: impl Fn() + Send + 'static,
    ) -> Option<Running> {
        let stop = Arc::new(Stop::default());
        let thread_stop = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("teksilo-os-theme-watch".to_owned())
            .spawn(move || {
                let Some(mut source) = open(Arc::clone(&thread_stop)) else {
                    return;
                };
                run(
                    &mut source,
                    &thread_stop,
                    &mut Debounce::new(window),
                    &Instant::now,
                    &post,
                );
            })
            .ok()?;
        Some(Running {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// ── The configuration files ─────────────────────────────────────────────────

/// Files and their modification times as last read, `None` for a file
/// that did not exist or could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfigStamps {
    files: Vec<(PathBuf, Option<SystemTime>)>,
}

impl ConfigStamps {
    /// The times of `paths`, read with `modified`.
    fn read(
        paths: impl IntoIterator<Item = PathBuf>,
        modified: impl Fn(&Path) -> Option<SystemTime>,
    ) -> Self {
        let files = paths
            .into_iter()
            .map(|path| {
                let stamp = modified(&path);
                (path, stamp)
            })
            .collect();
        Self { files }
    }

    /// Read every time again with `modified`; whether any differs from the
    /// one read before, a file made or removed included.
    fn refresh(&mut self, modified: impl Fn(&Path) -> Option<SystemTime>) -> bool {
        let mut changed = false;
        for (path, stamp) in &mut self.files {
            let now = modified(path);
            if now != *stamp {
                *stamp = now;
                changed = true;
            }
        }
        changed
    }
}

/// Every file of the `kdeglobals` and `breezerc` cascades, existing or not.
fn config_files(locations: &KdeConfigLocations) -> Vec<PathBuf> {
    locations
        .kdeglobals_cascade()
        .into_iter()
        .chain(locations.breezerc_cascade())
        .map(|layer| layer.path)
        .collect()
}

/// A file's modification time; `None` when it does not exist or the time
/// cannot be read.
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}
