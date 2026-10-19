// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use super::{
    BREEZERC_PATH, CONFIG_CHANGED, ChangeSource, Debounce, GLOBAL_SETTINGS, GLOBAL_SETTINGS_PATH,
    Heard, KCONFIG_NOTIFY, KDEGLOBALS_PATH, LISTENED, NOTIFY_CHANGE, PORTAL_PATH, PORTAL_SETTINGS,
    Running, SETTING_CHANGED, Stop, run, signal_matters,
};

const MS: Duration = Duration::from_millis(1);

// ── Signals ─────────────────────────────────────────────────────────────────

#[test]
fn a_portal_setting_matters_by_its_namespace() {
    let portal =
        |namespace| signal_matters(PORTAL_SETTINGS, SETTING_CHANGED, PORTAL_PATH, namespace);
    assert!(portal(Some("org.freedesktop.appearance")));
    assert!(portal(Some("org.kde.kdeglobals.General")));
    assert!(portal(Some("org.kde.kdeglobals.KDE")));
    assert!(portal(Some("org.kde.kdeglobals.Colors:View")));
    // KDE's other namespaces, GNOME's, a bare prefix and an unreadable body.
    assert!(!portal(Some("org.kde.VirtualKeyboard")));
    assert!(!portal(Some("org.kde.TabletMode")));
    assert!(!portal(Some("org.gnome.desktop.interface")));
    assert!(!portal(Some("org.kde.kdeglobals.")));
    assert!(!portal(Some("org.kde.kdeglobals")));
    assert!(!portal(Some("org.freedesktop.appearance.extra")));
    assert!(!portal(None));
}

#[test]
fn the_kde_signals_matter_on_their_paths_only() {
    assert!(signal_matters(
        GLOBAL_SETTINGS,
        NOTIFY_CHANGE,
        GLOBAL_SETTINGS_PATH,
        None
    ));
    assert!(signal_matters(
        KCONFIG_NOTIFY,
        CONFIG_CHANGED,
        KDEGLOBALS_PATH,
        None
    ));
    assert!(signal_matters(
        KCONFIG_NOTIFY,
        CONFIG_CHANGED,
        BREEZERC_PATH,
        None
    ));
    // Another configuration file, another path, another member.
    assert!(!signal_matters(
        KCONFIG_NOTIFY,
        CONFIG_CHANGED,
        "/kwinrc",
        None
    ));
    assert!(!signal_matters(
        GLOBAL_SETTINGS,
        NOTIFY_CHANGE,
        "/KGlobalSettings/other",
        None
    ));
    assert!(!signal_matters(
        GLOBAL_SETTINGS,
        "slotNotifyChange",
        GLOBAL_SETTINGS_PATH,
        None
    ));
    // The right member and path under another interface.
    assert!(!signal_matters(
        "org.example.Other",
        CONFIG_CHANGED,
        KDEGLOBALS_PATH,
        None
    ));
    assert!(!signal_matters(
        "org.example.Other",
        SETTING_CHANGED,
        PORTAL_PATH,
        Some("org.freedesktop.appearance")
    ));
}

#[test]
fn every_listened_signal_matters() {
    for (interface, member, path) in LISTENED {
        assert!(
            signal_matters(interface, member, path, Some("org.freedesktop.appearance")),
            "{interface}.{member} on {path}"
        );
    }
}

// ── Debounce ────────────────────────────────────────────────────────────────

#[test]
fn a_window_opens_on_the_first_change_and_is_due_once() {
    let start = Instant::now();
    let mut debounce = Debounce::new(150 * MS);
    assert_eq!(debounce.deadline(), None);
    assert!(!debounce.take_due(start + 1000 * MS), "nothing heard");

    debounce.heard(start);
    debounce.heard(start + 100 * MS);
    assert_eq!(
        debounce.deadline(),
        Some(start + 150 * MS),
        "a later change does not move the end"
    );
    assert!(!debounce.take_due(start + 149 * MS));
    assert!(debounce.take_due(start + 150 * MS));
    assert!(!debounce.take_due(start + 151 * MS), "due once");
    assert_eq!(debounce.deadline(), None);

    debounce.heard(start + 200 * MS);
    assert_eq!(debounce.deadline(), Some(start + 350 * MS));
}

/// A source whose changes come at fixed instants, on a clock it moves
/// itself: to each change it reports, and to the deadline when that comes
/// first. It closes when its changes are spent and no window is open, or,
/// when `closes_early`, as soon as they are spent.
struct Scripted {
    now: Rc<Cell<Instant>>,
    changes: VecDeque<Instant>,
    closes_early: bool,
}

impl ChangeSource for Scripted {
    fn wait(&mut self, deadline: Option<Instant>) -> Heard {
        if self.closes_early && self.changes.is_empty() {
            return Heard::Closed;
        }
        match (self.changes.front().copied(), deadline) {
            (Some(at), Some(due)) if at > due => {
                self.now.set(due);
                Heard::Deadline
            }
            (Some(at), _) => {
                self.changes.pop_front();
                self.now.set(at);
                Heard::Change
            }
            (None, Some(due)) => {
                self.now.set(due);
                Heard::Deadline
            }
            (None, None) => Heard::Closed,
        }
    }
}

/// The milliseconds after the start at which [`run`] posts, for changes
/// at `changes` milliseconds with a window of `window` milliseconds.
fn posts_for(window: u64, changes: &[u64]) -> Vec<u64> {
    posts_for_source(window, changes, false)
}

/// [`posts_for`] on a source that closes right after its last change.
fn posts_for_closing(window: u64, changes: &[u64]) -> Vec<u64> {
    posts_for_source(window, changes, true)
}

fn posts_for_source(window: u64, changes: &[u64], closes_early: bool) -> Vec<u64> {
    let start = Instant::now();
    let now = Rc::new(Cell::new(start));
    let mut source = Scripted {
        now: Rc::clone(&now),
        changes: changes.iter().map(|ms| start + *ms as u32 * MS).collect(),
        closes_early,
    };
    let posted = std::cell::RefCell::new(Vec::new());
    run(
        &mut source,
        &Stop::default(),
        &mut Debounce::new(window as u32 * MS),
        &|| now.get(),
        &|| {
            posted
                .borrow_mut()
                .push((now.get() - start).as_millis() as u64)
        },
    );
    posted.into_inner()
}

#[test]
fn a_burst_collapses_to_one_payload() {
    assert_eq!(posts_for(150, &[0, 5, 10, 60, 149]), vec![150]);
}

#[test]
fn separate_bursts_give_one_payload_each() {
    assert_eq!(posts_for(150, &[0, 10, 500, 510, 520]), vec![150, 650]);
}

#[test]
fn a_change_after_its_window_ends_opens_another() {
    // A change at the very end of a window is covered by its payload; one
    // just after opens the next window.
    assert_eq!(posts_for(150, &[0, 150, 151]), vec![150, 301]);
}

#[test]
fn nothing_heard_posts_nothing() {
    assert_eq!(posts_for(150, &[]), Vec::<u64>::new());
}

#[test]
fn every_change_is_followed_by_a_payload_no_earlier_than_it() {
    // A fixed pseudo-random sequence of gaps, from 0 to 400 ms.
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut at = 0;
    let mut changes = Vec::new();
    for _ in 0..200 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        at += state % 401;
        changes.push(at);
    }
    let posts = posts_for(150, &changes);
    for change in &changes {
        assert!(
            posts
                .iter()
                .any(|post| post >= change && post - change <= 150),
            "the change at {change} ms is followed by a payload within the window"
        );
    }
    for pair in posts.windows(2) {
        assert!(
            pair[1] - pair[0] >= 150,
            "payloads {pair:?} closer than a window"
        );
    }
    assert!(
        posts.len() < changes.len(),
        "nearby changes share a payload"
    );
}

#[test]
fn a_stopped_watch_posts_nothing() {
    let start = Instant::now();
    let now = Rc::new(Cell::new(start));
    let mut source = Scripted {
        now: Rc::clone(&now),
        changes: [start, start + 10 * MS].into(),
        closes_early: false,
    };
    let stop = Stop::default();
    stop.stop();
    let posted = Cell::new(0);
    run(
        &mut source,
        &stop,
        &mut Debounce::new(150 * MS),
        &|| now.get(),
        &|| posted.set(posted.get() + 1),
    );
    assert_eq!(posted.get(), 0);
}

#[test]
fn a_window_open_when_the_source_closes_is_posted_then() {
    // The source closes 10 ms into a window of 150: the changes it heard
    // are posted for at the close, not dropped.
    assert_eq!(posts_for_closing(150, &[0, 10]), vec![10]);
    // A window that already ended was posted for at its end, and the close
    // after it posts nothing more.
    assert_eq!(posts_for_closing(150, &[0, 200]), vec![150, 200]);
    assert_eq!(posts_for_closing(150, &[]), Vec::<u64>::new());
}

#[test]
fn a_window_open_when_a_stopped_watch_closes_posts_nothing() {
    let start = Instant::now();
    let now = Rc::new(Cell::new(start));
    let mut source = Scripted {
        now: Rc::clone(&now),
        changes: VecDeque::new(),
        closes_early: true,
    };
    let stop = Stop::default();
    stop.stop();
    let mut debounce = Debounce::new(150 * MS);
    debounce.heard(start);
    let posted = Cell::new(0);
    run(&mut source, &stop, &mut debounce, &|| now.get(), &|| {
        posted.set(posted.get() + 1)
    });
    assert_eq!(posted.get(), 0);
}

#[test]
fn a_pause_ends_at_once_when_the_watch_stops() {
    let stop = Arc::new(Stop::default());
    assert!(stop.pause(MS), "a pause nothing stops runs out");
    let stopper = Arc::clone(&stop);
    let thread = std::thread::spawn(move || {
        std::thread::sleep(20 * MS);
        stopper.stop();
    });
    let paused = Instant::now();
    assert!(!stop.pause(Duration::from_secs(30)), "the stop ends it");
    assert!(paused.elapsed() < Duration::from_secs(5));
    thread.join().unwrap();
    assert!(!stop.pause(Duration::from_secs(30)), "and every later one");
}

// ── The thread ──────────────────────────────────────────────────────────────

/// A source fed through a channel, watching the stop flag between short
/// waits.
struct Channel {
    changes: Receiver<()>,
    stop: Arc<Stop>,
}

impl ChangeSource for Channel {
    fn wait(&mut self, deadline: Option<Instant>) -> Heard {
        loop {
            if self.stop.is_stopped() {
                return Heard::Closed;
            }
            let slice = deadline.map_or(5 * MS, |due| {
                due.saturating_duration_since(Instant::now()).min(5 * MS)
            });
            match self.changes.recv_timeout(slice) {
                Ok(()) => return Heard::Change,
                Err(RecvTimeoutError::Timeout) => {
                    if deadline.is_some_and(|due| Instant::now() >= due) {
                        return Heard::Deadline;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return Heard::Closed,
            }
        }
    }
}

/// A running watch over a [`Channel`] with a window of `window`, the
/// sender feeding it, and how many times it has posted.
fn channel_watch(window: Duration) -> (Running, mpsc::Sender<()>, Arc<AtomicUsize>) {
    let (send, changes) = mpsc::channel();
    let posted = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&posted);
    let mut changes = Some(changes);
    let running = Running::start(
        window,
        10 * MS,
        move |stop| {
            Some(Channel {
                changes: changes.take()?,
                stop,
            })
        },
        move || {
            counter.fetch_add(1, Ordering::SeqCst);
        },
    )
    .expect("start the watch thread");
    (running, send, posted)
}

/// Wait until `count` reaches `at_least`, for at most two seconds.
fn wait_for(count: &AtomicUsize, at_least: usize) {
    let until = Instant::now() + Duration::from_secs(2);
    while count.load(Ordering::SeqCst) < at_least && Instant::now() < until {
        std::thread::sleep(5 * MS);
    }
}

#[test]
fn the_thread_posts_once_per_burst() {
    let window = 40 * MS;
    let (_running, send, posted) = channel_watch(window);
    for _ in 0..5 {
        send.send(()).unwrap();
    }
    wait_for(&posted, 1);
    std::thread::sleep(window * 3);
    assert_eq!(posted.load(Ordering::SeqCst), 1, "one burst, one payload");

    send.send(()).unwrap();
    wait_for(&posted, 2);
    std::thread::sleep(window * 3);
    assert_eq!(posted.load(Ordering::SeqCst), 2, "a second burst, a second");
}

#[test]
fn nothing_is_posted_after_drop() {
    let window = 40 * MS;
    let (running, send, posted) = channel_watch(window);
    send.send(()).unwrap();
    let dropped = Instant::now();
    drop(running);
    assert!(
        dropped.elapsed() < Duration::from_secs(1),
        "the drop joins promptly"
    );
    let _ = send.send(());
    std::thread::sleep(window * 4);
    assert_eq!(
        posted.load(Ordering::SeqCst),
        0,
        "the window still open at the drop posts nothing"
    );
}

/// A running watch with a window of `window` that opens a [`Channel`] on
/// `first`, then one for each receiver sent on the returned sender, trying
/// every `retry`, and counts its attempts to open and its posts.
fn reopening_watch(
    window: Duration,
    retry: Duration,
    first: Receiver<()>,
) -> (
    Running,
    mpsc::Sender<Receiver<()>>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
) {
    let (sources, next) = mpsc::channel::<Receiver<()>>();
    sources.send(first).unwrap();
    let opens = Arc::new(AtomicUsize::new(0));
    let posted = Arc::new(AtomicUsize::new(0));
    let (open_count, post_count) = (Arc::clone(&opens), Arc::clone(&posted));
    let running = Running::start(
        window,
        retry,
        move |stop| {
            open_count.fetch_add(1, Ordering::SeqCst);
            let changes = next.try_recv().ok()?;
            Some(Channel { changes, stop })
        },
        move || {
            post_count.fetch_add(1, Ordering::SeqCst);
        },
    )
    .expect("start the watch thread");
    (running, sources, opens, posted)
}

#[test]
fn a_closed_source_is_posted_for_and_opened_again() {
    let window = 40 * MS;
    let (first_send, first) = mpsc::channel();
    let (running, sources, opens, posted) = reopening_watch(window, 10 * MS, first);
    wait_for(&opens, 1);

    // A change, then the source closes inside its window.
    first_send.send(()).unwrap();
    drop(first_send);
    wait_for(&posted, 1);
    assert_eq!(
        posted.load(Ordering::SeqCst),
        1,
        "the open window is posted"
    );

    // With nothing to open, the watch keeps trying, and posts nothing.
    wait_for(&opens, 4);
    assert!(opens.load(Ordering::SeqCst) >= 4, "it tries again");
    assert_eq!(posted.load(Ordering::SeqCst), 1);

    // Opened again: one payload for what nobody heard, then one per burst.
    let (second_send, second) = mpsc::channel();
    sources.send(second).unwrap();
    wait_for(&posted, 2);
    std::thread::sleep(window * 3);
    assert_eq!(
        posted.load(Ordering::SeqCst),
        2,
        "the reopening reads again"
    );
    let opened = opens.load(Ordering::SeqCst);
    second_send.send(()).unwrap();
    wait_for(&posted, 3);
    std::thread::sleep(window * 3);
    assert_eq!(posted.load(Ordering::SeqCst), 3, "the new source is heard");
    assert_eq!(opens.load(Ordering::SeqCst), opened, "and kept");

    let dropped = Instant::now();
    drop(running);
    assert!(dropped.elapsed() < Duration::from_secs(1));
    assert_eq!(posted.load(Ordering::SeqCst), 3);
}

#[test]
fn a_watch_waiting_to_reopen_is_dropped_promptly() {
    let (first_send, first) = mpsc::channel();
    let (running, _sources, opens, posted) =
        reopening_watch(10 * MS, Duration::from_secs(30), first);
    wait_for(&opens, 1);
    drop(first_send);
    // Let the thread see the close and start its pause.
    std::thread::sleep(50 * MS);
    let dropped = Instant::now();
    drop(running);
    assert!(
        dropped.elapsed() < Duration::from_secs(5),
        "the drop ends the pause"
    );
    assert_eq!(opens.load(Ordering::SeqCst), 1);
    assert_eq!(posted.load(Ordering::SeqCst), 0, "nothing was heard");
}

#[test]
fn a_source_that_cannot_open_ends_the_thread() {
    let running = Running::start(
        10 * MS,
        10 * MS,
        |_stop| None::<Channel>,
        || panic!("nothing to hear"),
    )
    .expect("start the watch thread");
    let dropped = Instant::now();
    drop(running);
    assert!(dropped.elapsed() < Duration::from_secs(1));
}

// ── The configuration files ─────────────────────────────────────────────────

/// Unix only: XDG paths must start with `/`, so a Windows scratch directory
/// (`C:\...`) is not one and the cascade falls back to `/etc/xdg`. The watch
/// reads these files on Linux alone (`OsThemeWatch::new` is `None` elsewhere).
#[cfg(unix)]
mod cascade_files {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};

    use teksilo_kde_scheme::KdeConfigLocations;

    use super::super::{ConfigStamps, config_files, modified};

    fn set_modified(path: &Path, time: SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .expect("open the file")
            .set_modified(time)
            .expect("set its modification time");
    }

    #[test]
    fn a_file_of_either_cascade_written_made_or_removed_is_a_change() {
        let dir = tempfile::tempdir().expect("make a scratch directory");
        let root = dir.path().to_owned();
        let config = root.join("config");
        let etc = root.join("etc");
        for d in [&config, &etc, &config.join("kdedefaults")] {
            std::fs::create_dir_all(d).expect("make a configuration directory");
        }
        let vars: Vec<(&str, String)> = vec![
            ("HOME", root.display().to_string()),
            ("XDG_CONFIG_HOME", config.display().to_string()),
            ("XDG_CONFIG_DIRS", etc.display().to_string()),
            ("XDG_CURRENT_DESKTOP", "KDE".to_owned()),
            ("KDE_SKIP_KDERC", "1".to_owned()),
        ];
        let locations = KdeConfigLocations::from_env(|name: &str| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.clone())
        });
        let paths = config_files(&locations);
        assert!(
            paths.iter().all(|path| path.starts_with(&root)),
            "every file is in the scratch directory: {paths:?}"
        );
        let has = |path: PathBuf| assert!(paths.contains(&path), "{} is watched", path.display());
        has(config.join("kdeglobals"));
        has(config.join("breezerc"));
        has(config.join("kdedefaults").join("kdeglobals"));
        has(etc.join("kdeglobals"));
        has(etc.join("breezerc"));

        let user = config.join("kdeglobals");
        let system_breezerc = etc.join("breezerc");
        std::fs::write(&user, "[General]\nColorScheme=BreezeLight\n").unwrap();
        std::fs::write(&system_breezerc, "[Style]\n").unwrap();
        let base = SystemTime::now() - Duration::from_secs(3600);
        set_modified(&user, base);
        set_modified(&system_breezerc, base);

        let mut stamps = ConfigStamps::read(paths, modified);
        assert!(!stamps.refresh(modified), "nothing changed");

        set_modified(&user, base + Duration::from_secs(10));
        assert!(
            stamps.refresh(modified),
            "the user's kdeglobals was written"
        );
        assert!(!stamps.refresh(modified), "and is counted once");

        std::fs::write(config.join("breezerc"), "[Common]\n").unwrap();
        assert!(stamps.refresh(modified), "the user's breezerc was made");

        std::fs::write(config.join("kdedefaults").join("kdeglobals"), "[KDE]\n").unwrap();
        assert!(stamps.refresh(modified), "the kdedefaults layer was made");

        std::fs::remove_file(&system_breezerc).unwrap();
        assert!(stamps.refresh(modified), "the system breezerc was removed");
        assert!(!stamps.refresh(modified));

        // A file outside both cascades is not watched.
        std::fs::write(config.join("kwinrc"), "[Windows]\n").unwrap();
        assert!(!stamps.refresh(modified));
    }
}

// ── The real bus ────────────────────────────────────────────────────────────

/// The watch on a private `dbus-daemon`, posting through a recording
/// poster. Opt-in: run with `TEKSILO_DBUS_TESTS=1`.
#[cfg(target_os = "linux")]
mod real_bus {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};

    use teksilo_core::{AppEventPoster, SubscriptionId};
    use zbus::zvariant::Value;

    use super::super::{DEBOUNCE, OsThemeChanged, OsThemeWatch, RECONNECT_EVERY};
    use crate::linux_portal::tests::real_bus::{PrivateBus, enabled};

    /// Counts the [`OsThemeChanged`] payloads posted to it, and every other
    /// payload apart.
    #[derive(Clone, Default)]
    struct Recorder {
        changes: Arc<AtomicUsize>,
        others: Arc<AtomicUsize>,
    }

    impl AppEventPoster for Recorder {
        fn post_subscription_event(&self, _: SubscriptionId, _: Box<dyn std::any::Any + Send>) {
            self.others.fetch_add(1, Ordering::SeqCst);
        }

        fn post_external(&self, payload: Box<dyn std::any::Any + Send>) {
            if payload.is::<OsThemeChanged>() {
                self.changes.fetch_add(1, Ordering::SeqCst);
            } else {
                self.others.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    impl Recorder {
        fn changes(&self) -> usize {
            self.changes.load(Ordering::SeqCst)
        }

        /// Wait for `expected` payloads, then a little more to see that no
        /// further one comes, and return the count.
        fn settle(&self, expected: usize) -> usize {
            let until = Instant::now() + Duration::from_secs(3);
            while self.changes() < expected && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(DEBOUNCE * 2);
            self.changes()
        }
    }

    /// Something that sends one signal.
    type Sender = Box<dyn Fn(&zbus::Connection) -> zbus::Result<()>>;

    fn emit<B>(
        connection: &zbus::Connection,
        path: &str,
        interface: &str,
        member: &str,
        body: &B,
    ) -> zbus::Result<()>
    where
        B: zbus::export::serde::Serialize + zbus::zvariant::DynamicType,
    {
        async_io::block_on(connection.emit_signal(
            None::<zbus::names::BusName<'_>>,
            path,
            interface,
            member,
            body,
        ))
    }

    fn portal(
        namespace: &'static str,
        key: &'static str,
    ) -> impl Fn(&zbus::Connection) -> zbus::Result<()> {
        move |c| {
            emit(
                c,
                "/org/freedesktop/portal/desktop",
                "org.freedesktop.portal.Settings",
                "SettingChanged",
                &(namespace, key, Value::U32(1)),
            )
        }
    }

    fn config_changed(c: &zbus::Connection, path: &str) -> zbus::Result<()> {
        let groups: HashMap<String, Vec<Vec<u8>>> =
            HashMap::from([("General".to_owned(), vec![b"ColorScheme".to_vec()])]);
        emit(c, path, "org.kde.kconfig.notify", "ConfigChanged", &groups)
    }

    fn notify_change(c: &zbus::Connection) -> zbus::Result<()> {
        emit(
            c,
            "/KGlobalSettings",
            "org.kde.KGlobalSettings",
            "notifyChange",
            &(0_i32, 0_i32),
        )
    }

    #[test]
    fn hears_each_listened_signal_once_per_debounce_window() {
        if !enabled() {
            return;
        }
        let bus = PrivateBus::start();
        let recorder = Recorder::default();
        let (opened, is_open) = mpsc::channel();
        let watch = OsThemeWatch::on_bus(Some(bus.address.clone()), recorder.clone(), move |ok| {
            let _ = opened.send(ok);
        })
        .expect("start the watch");
        assert_eq!(
            is_open.recv_timeout(Duration::from_secs(5)),
            Ok(true),
            "the watch reached the bus and subscribed"
        );
        let sender = async_io::block_on(
            zbus::connection::Builder::address(bus.address.as_str())
                .expect("the bus address")
                .build(),
        )
        .expect("connect the sender");

        // Each listened signal alone: one payload each.
        let listened: Vec<(&str, Sender)> = vec![
            (
                "appearance color-scheme",
                Box::new(portal("org.freedesktop.appearance", "color-scheme")),
            ),
            (
                "kdeglobals General ColorScheme",
                Box::new(portal("org.kde.kdeglobals.General", "ColorScheme")),
            ),
            ("notifyChange", Box::new(notify_change)),
            (
                "ConfigChanged /kdeglobals",
                Box::new(|c| config_changed(c, "/kdeglobals")),
            ),
            (
                "ConfigChanged /breezerc",
                Box::new(|c| config_changed(c, "/breezerc")),
            ),
        ];
        let mut expected = 0;
        for (name, send) in &listened {
            send(&sender).expect("send the signal");
            expected += 1;
            assert_eq!(recorder.settle(expected), expected, "{name}");
        }

        // A burst of every one of them: one payload.
        for _ in 0..3 {
            for (_, send) in &listened {
                send(&sender).expect("send the signal");
            }
        }
        expected += 1;
        assert_eq!(recorder.settle(expected), expected, "a burst");

        // Signals the watch does not listen to, or ignores.
        let ignored: Vec<Sender> = vec![
            Box::new(portal("org.kde.VirtualKeyboard", "active")),
            Box::new(portal("org.gnome.desktop.interface", "gtk-theme")),
            Box::new(|c| config_changed(c, "/kwinrc")),
            Box::new(|c| {
                emit(
                    c,
                    "/kdeglobals",
                    "org.example.Other",
                    "ConfigChanged",
                    &(0_i32,),
                )
            }),
        ];
        for send in &ignored {
            send(&sender).expect("send the signal");
        }
        assert_eq!(recorder.settle(expected), expected, "nothing it ignores");

        // Nothing once the watch is dropped.
        drop(watch);
        for (_, send) in &listened {
            send(&sender).expect("send the signal");
        }
        assert_eq!(recorder.settle(expected), expected, "nothing after drop");
        assert_eq!(recorder.others.load(Ordering::SeqCst), 0);
    }

    /// A connection to the bus, for sending.
    fn sender_on(address: &str) -> zbus::Connection {
        async_io::block_on(
            zbus::connection::Builder::address(address)
                .expect("the bus address")
                .build(),
        )
        .expect("connect the sender")
    }

    #[test]
    fn a_bus_that_goes_away_is_posted_for_and_reached_again() {
        if !enabled() {
            return;
        }
        let mut bus = PrivateBus::start_restartable();
        let recorder = Recorder::default();
        let (opened, is_open) = mpsc::channel();
        let watch = OsThemeWatch::on_bus(Some(bus.address.clone()), recorder.clone(), move |ok| {
            let _ = opened.send(ok);
        })
        .expect("start the watch");
        assert_eq!(is_open.recv_timeout(Duration::from_secs(5)), Ok(true));

        // A change, and the bus killed inside its debounce window.
        let sender = sender_on(&bus.address);
        config_changed(&sender, "/kdeglobals").expect("send the signal");
        std::thread::sleep(Duration::from_millis(20));
        bus.kill();
        drop(sender);
        assert_eq!(
            recorder.settle(1),
            1,
            "the change heard before the bus went"
        );

        // The bus back at its address: the watch reaches it again and reads
        // the desktop once for what it could not hear meanwhile.
        bus.restart();
        let reached = Instant::now() + RECONNECT_EVERY * 5;
        while Instant::now() < reached {
            if is_open.recv_timeout(Duration::from_millis(50)) == Ok(true) {
                break;
            }
        }
        assert!(Instant::now() < reached, "the watch reached the bus again");
        assert_eq!(recorder.settle(2), 2, "one payload for the gap");

        // And it hears the new bus.
        let sender = sender_on(&bus.address);
        notify_change(&sender).expect("send the signal");
        assert_eq!(recorder.settle(3), 3, "a signal on the new bus");
        drop(watch);
        assert_eq!(recorder.others.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_bus_that_is_not_there_leaves_a_silent_watch() {
        if !enabled() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("teksilo-watch-absent-{}", std::process::id()));
        let (opened, is_open) = mpsc::channel();
        let recorder = Recorder::default();
        let watch = OsThemeWatch::on_bus(
            Some(format!("unix:path={}", dir.join("no-bus").display())),
            recorder.clone(),
            move |ok| {
                let _ = opened.send(ok);
            },
        )
        .expect("start the watch");
        assert_eq!(is_open.recv_timeout(Duration::from_secs(5)), Ok(false));
        drop(watch);
        assert_eq!(recorder.changes(), 0);
    }
}
