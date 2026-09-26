// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Reports sent from a thread of their own, so the event loop only ever
//! waits, briefly, on a channel.
//!
//! The worker owns the link to the registry and answers the reports it is
//! handed one at a time, in the order they were made. A press sends its
//! report with a one-slot channel for the answer and waits on it for
//! [`Timing::wait`]; a release sends its report and returns. Reports are
//! numbered, and the worker publishes the number of the last one it finished,
//! which is how the event loop knows whether a press it gave up on has been
//! answered since: until it has, the registry is taken to be stalled and
//! presses are reported without waiting.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, channel, sync_channel};
use std::time::{Duration, Instant};

use super::{DeviceEvent, DeviceEventKind, KeyEventReporter, ReportOutcome};

/// The registry, as the worker thread talks to it.
pub(super) trait RegistryLink: Send {
    /// Report one key; `Ok(true)` when a listener took it.
    fn notify(&mut self, event: &DeviceEvent) -> Result<bool, LinkError>;
}

/// Why a report could not be made.
#[derive(Debug, thiserror::Error)]
pub(super) enum LinkError {
    /// There is no accessibility bus to connect to.
    #[error("no accessibility bus: {0}")]
    NoBus(String),
    /// The call was answered with an error, or timed out; the connection is
    /// still good.
    #[error("the registry did not take the report: {0}")]
    Refused(String),
    /// The connection itself failed and has to be made again.
    #[error("the accessibility bus connection failed: {0}")]
    Broken(String),
}

/// Makes a link to the registry.
pub(super) type Connector = Box<dyn FnMut() -> Result<Box<dyn RegistryLink>, LinkError> + Send>;

/// How long things may take.
#[derive(Debug, Clone, Copy)]
pub(super) struct Timing {
    /// How long a press waits for its answer.
    pub wait: Duration,
    /// How long after a failed connection before trying again.
    pub retry: Duration,
    /// How many presses may wait for the worker before new ones go
    /// unreported. Releases are never turned away: each one follows a press
    /// or a key coming up, so they are bounded by the keys held.
    pub queue: usize,
    /// How old a press may grow in the queue before the worker drops it
    /// rather than replay it to a registry that has woken up.
    pub stale: Duration,
}

impl Timing {
    /// 200 ms: Qt waits 100 ms, GTK 3 500 ms. Long enough for a screen reader
    /// written in Python on a busy machine, short enough that the one pause a
    /// hung registry costs is a hitch and not a freeze.
    pub(super) const PLATFORM: Self = Self {
        wait: Duration::from_millis(200),
        retry: Duration::from_secs(5),
        queue: 64,
        stale: Duration::from_secs(1),
    };
}

/// One report, numbered in the order it was made.
struct Job {
    number: u64,
    queued_at: Instant,
    event: DeviceEvent,
    /// Where the answer goes, for a press. `None` for a release, and for a
    /// press reported while the registry is stalled.
    answer: Option<SyncSender<ReportOutcome>>,
}

/// A [`KeyEventReporter`] whose reports a worker thread sends.
pub(super) struct ThreadedReporter {
    jobs: Sender<Job>,
    /// The number of the last report the worker finished.
    finished: Arc<AtomicU64>,
    /// Presses queued and not yet finished by the worker.
    presses_waiting: Arc<AtomicUsize>,
    /// How many may be; see [`Timing::queue`].
    queue: usize,
    next: u64,
    /// The press the event loop stopped waiting for, while it is unanswered.
    stalled_at: Option<u64>,
    wait: Duration,
    /// The worker is gone (it panicked); said once, then every key is
    /// delivered unreported.
    stopped: bool,
}

impl ThreadedReporter {
    /// Start the worker. It connects at once, so that the first key finds the
    /// link made.
    pub(super) fn spawn(connect: Connector, timing: Timing) -> Self {
        let (jobs, queue) = channel();
        let finished = Arc::new(AtomicU64::new(0));
        let presses_waiting = Arc::new(AtomicUsize::new(0));
        let worker = Worker {
            finished: Arc::clone(&finished),
            presses_waiting: Arc::clone(&presses_waiting),
            timing,
        };
        // A worker that cannot start leaves every send failing, and every key
        // delivered unreported: the state before this module existed.
        let _ = std::thread::Builder::new()
            .name("teksilo-key-report".into())
            .spawn(move || worker.run(queue, connect));
        Self {
            jobs,
            finished,
            presses_waiting,
            queue: timing.queue,
            next: 1,
            stalled_at: None,
            wait: timing.wait,
            stopped: false,
        }
    }

    fn stalled(&mut self) -> bool {
        match self.stalled_at {
            Some(number) if self.finished.load(Ordering::Acquire) < number => true,
            _ => {
                self.stalled_at = None;
                false
            }
        }
    }

    /// Queue a report; its number, or `None` when too many presses wait
    /// already (a press only) or the worker is gone.
    fn send(
        &mut self,
        event: &DeviceEvent,
        answer: Option<SyncSender<ReportOutcome>>,
    ) -> Option<u64> {
        let press = event.kind == DeviceEventKind::Pressed;
        if self.stopped || (press && self.presses_waiting.load(Ordering::Acquire) >= self.queue) {
            return None;
        }
        let number = self.next;
        let job = Job {
            number,
            queued_at: Instant::now(),
            event: event.clone(),
            answer,
        };
        if press {
            self.presses_waiting.fetch_add(1, Ordering::AcqRel);
        }
        if self.jobs.send(job).is_err() {
            if press {
                self.presses_waiting.fetch_sub(1, Ordering::AcqRel);
            }
            self.worker_stopped();
            return None;
        }
        self.next += 1;
        Some(number)
    }

    /// The worker is gone, which only a panic in it does: say so once.
    fn worker_stopped(&mut self) {
        if !self.stopped {
            eprintln!(
                "teksilo-platform: the key reporter stopped; keys are no longer reported to \
                 assistive technology"
            );
            self.stopped = true;
        }
    }
}

impl KeyEventReporter for ThreadedReporter {
    fn report_press(&mut self, event: &DeviceEvent) -> ReportOutcome {
        if self.stalled() {
            self.send(event, None);
            return ReportOutcome::Unanswered;
        }
        let (answer, answered) = sync_channel(1);
        let Some(number) = self.send(event, Some(answer)) else {
            return ReportOutcome::Unanswered;
        };
        match answered.recv_timeout(self.wait) {
            Ok(outcome) => outcome,
            Err(RecvTimeoutError::Timeout) => {
                self.stalled_at = Some(number);
                ReportOutcome::Unanswered
            }
            // The worker always answers; an answer channel dropped unanswered
            // is the worker dying on this very report.
            Err(RecvTimeoutError::Disconnected) => {
                self.worker_stopped();
                ReportOutcome::Unanswered
            }
        }
    }

    fn report_release(&mut self, event: &DeviceEvent) {
        self.send(event, None);
    }
}

/// The worker thread's side.
struct Worker {
    finished: Arc<AtomicU64>,
    presses_waiting: Arc<AtomicUsize>,
    timing: Timing,
}

impl Worker {
    /// Connect, then answer each report in turn.
    fn run(self, queue: Receiver<Job>, mut connect: Connector) {
        let retry = self.timing.retry;
        let mut link: Option<Box<dyn RegistryLink>> = None;
        let mut last_attempt: Option<Instant> = None;
        let mut warned = false;
        let mut reconnect = |link: &mut Option<Box<dyn RegistryLink>>| {
            if link.is_some() || last_attempt.is_some_and(|at| at.elapsed() < retry) {
                return;
            }
            last_attempt = Some(Instant::now());
            match connect() {
                Ok(made) => *link = Some(made),
                Err(error) => {
                    if !warned {
                        eprintln!(
                            "teksilo-platform: keys are not reported to assistive technology: \
                             {error}"
                        );
                        warned = true;
                    }
                }
            }
        };
        reconnect(&mut link);
        while let Ok(job) = queue.recv() {
            let press = job.event.kind == DeviceEventKind::Pressed;
            let outcome = if press && job.queued_at.elapsed() > self.timing.stale {
                // Queued behind a registry that hung; the key is long gone.
                ReportOutcome::Unanswered
            } else {
                reconnect(&mut link);
                match link.as_mut().map(|made| made.notify(&job.event)) {
                    Some(Ok(true)) => ReportOutcome::Consumed,
                    Some(Ok(false)) => ReportOutcome::NotConsumed,
                    Some(Err(LinkError::Refused(_))) | None => ReportOutcome::Unanswered,
                    Some(Err(_)) => {
                        link = None;
                        ReportOutcome::Unanswered
                    }
                }
            };
            if let Some(answer) = job.answer {
                // Nobody listening any more is the event loop having given up.
                let _ = answer.try_send(outcome);
            }
            if press {
                self.presses_waiting.fetch_sub(1, Ordering::AcqRel);
            }
            self.finished.store(job.number, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    use super::{Connector, LinkError, RegistryLink, ThreadedReporter, Timing};
    use crate::key_report::{DeviceEvent, DeviceEventKind, KeyEventReporter, ReportOutcome};

    fn key(kind: DeviceEventKind, keysym: u32) -> DeviceEvent {
        DeviceEvent {
            kind,
            keysym,
            hw_code: 0,
            modifiers: 0,
            timestamp: 0,
            event_string: String::new(),
            is_text: false,
        }
    }

    fn press(keysym: u32) -> DeviceEvent {
        key(DeviceEventKind::Pressed, keysym)
    }

    fn timing(wait_ms: u64) -> Timing {
        Timing {
            wait: Duration::from_millis(wait_ms),
            retry: Duration::ZERO,
            queue: 64,
            stale: Duration::from_secs(60),
        }
    }

    fn release(keysym: u32) -> DeviceEvent {
        key(DeviceEventKind::Released, keysym)
    }

    fn heard(registry: &Registry) -> Vec<(DeviceEventKind, u32)> {
        registry
            .heard
            .lock()
            .unwrap()
            .iter()
            .map(|e| (e.kind, e.keysym))
            .collect()
    }

    /// A door the registry waits behind until a test opens it.
    #[derive(Clone, Default)]
    struct Door(Arc<(Mutex<bool>, Condvar)>);

    impl Door {
        fn open(&self) {
            *self.0.0.lock().unwrap() = true;
            self.0.1.notify_all();
        }

        fn wait(&self) {
            let mut open = self.0.0.lock().unwrap();
            while !*open {
                open = self.0.1.wait(open).unwrap();
            }
        }
    }

    /// A registry that records what it hears and answers `consume`, after
    /// waiting at `door` when there is one.
    #[derive(Clone, Default)]
    struct Registry {
        heard: Arc<Mutex<Vec<DeviceEvent>>>,
        consume: bool,
        door: Option<Door>,
    }

    impl RegistryLink for Registry {
        fn notify(&mut self, event: &DeviceEvent) -> Result<bool, LinkError> {
            if let Some(door) = &self.door {
                door.wait();
            }
            self.heard.lock().unwrap().push(event.clone());
            Ok(self.consume)
        }
    }

    fn connecting_to(registry: Registry) -> Connector {
        Box::new(move || Ok(Box::new(registry.clone()) as Box<dyn RegistryLink>))
    }

    fn eventually(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(Instant::now() < deadline, "never happened: {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_press_waits_for_the_registry_to_answer() {
        let taking = Registry {
            consume: true,
            ..Registry::default()
        };
        let mut reporter = ThreadedReporter::spawn(connecting_to(taking.clone()), timing(2000));
        assert_eq!(reporter.report_press(&press(0x74)), ReportOutcome::Consumed);
        assert_eq!(taking.heard.lock().unwrap().len(), 1);

        let passing = Registry::default();
        let mut reporter = ThreadedReporter::spawn(connecting_to(passing), timing(2000));
        assert_eq!(
            reporter.report_press(&press(0x74)),
            ReportOutcome::NotConsumed
        );
    }

    #[test]
    fn reports_reach_the_registry_in_order() {
        let registry = Registry::default();
        let mut reporter = ThreadedReporter::spawn(connecting_to(registry.clone()), timing(2000));
        reporter.report_press(&press(1));
        reporter.report_release(&key(DeviceEventKind::Released, 1));
        reporter.report_press(&press(2));
        let heard: Vec<_> = registry
            .heard
            .lock()
            .unwrap()
            .iter()
            .map(|e| (e.kind, e.keysym))
            .collect();
        assert_eq!(
            heard,
            vec![
                (DeviceEventKind::Pressed, 1),
                (DeviceEventKind::Released, 1),
                (DeviceEventKind::Pressed, 2)
            ]
        );
    }

    #[test]
    fn a_release_is_never_waited_for() {
        let door = Door::default();
        let registry = Registry {
            door: Some(door.clone()),
            ..Registry::default()
        };
        let mut reporter = ThreadedReporter::spawn(connecting_to(registry.clone()), timing(2000));
        let started = Instant::now();
        reporter.report_release(&key(DeviceEventKind::Released, 1));
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "a release waited {:?}",
            started.elapsed()
        );
        door.open();
        eventually("the release reaches the registry", || {
            registry.heard.lock().unwrap().len() == 1
        });
    }

    /// A registry that hangs costs the first press one bounded wait; the
    /// presses after it do not wait at all, until it answers again.
    #[test]
    fn a_hung_registry_costs_one_wait_not_one_per_key() {
        let door = Door::default();
        let registry = Registry {
            door: Some(door.clone()),
            consume: true,
            ..Registry::default()
        };
        let mut reporter = ThreadedReporter::spawn(connecting_to(registry.clone()), timing(150));

        let started = Instant::now();
        assert_eq!(reporter.report_press(&press(1)), ReportOutcome::Unanswered);
        let first = started.elapsed();
        assert!(
            first >= Duration::from_millis(150),
            "gave up after {first:?}"
        );
        assert!(first < Duration::from_secs(2), "held the key {first:?}");

        let started = Instant::now();
        assert_eq!(reporter.report_press(&press(2)), ReportOutcome::Unanswered);
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "the second press waited {:?} behind a hung registry",
            started.elapsed()
        );

        door.open();
        eventually("the registry catches up", || {
            registry.heard.lock().unwrap().len() == 2
        });
        assert_eq!(
            reporter.report_press(&press(3)),
            ReportOutcome::Consumed,
            "once the registry answers again, presses wait for it again"
        );
    }

    #[test]
    fn a_missing_registry_answers_at_once() {
        let connect: Connector = Box::new(|| Err(LinkError::NoBus("no bus in this test".into())));
        let mut reporter = ThreadedReporter::spawn(connect, timing(2000));
        let started = Instant::now();
        assert_eq!(reporter.report_press(&press(1)), ReportOutcome::Unanswered);
        assert!(
            started.elapsed() < Duration::from_millis(1000),
            "waited {:?} for a registry that is not there",
            started.elapsed()
        );
    }

    /// The bus appearing after the application started, or coming back after
    /// it went away: the worker connects again.
    #[test]
    fn a_registry_that_appears_later_is_found() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let counted = attempts.clone();
        let registry = Registry {
            consume: true,
            ..Registry::default()
        };
        let connect: Connector = Box::new(move || {
            if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(LinkError::NoBus("not yet".into()))
            } else {
                Ok(Box::new(registry.clone()) as Box<dyn RegistryLink>)
            }
        });
        let mut reporter = ThreadedReporter::spawn(connect, timing(2000));
        eventually("the first connection attempt", || {
            attempts.load(Ordering::SeqCst) >= 1
        });
        assert_eq!(reporter.report_press(&press(1)), ReportOutcome::Consumed);
    }

    /// A connection that fails mid-session is made again.
    #[test]
    fn a_broken_link_is_made_again() {
        struct Breaks;
        impl RegistryLink for Breaks {
            fn notify(&mut self, _: &DeviceEvent) -> Result<bool, LinkError> {
                Err(LinkError::Broken("the bus went away".into()))
            }
        }
        let attempts = Arc::new(AtomicUsize::new(0));
        let counted = attempts.clone();
        let connect: Connector = Box::new(move || {
            if counted.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(Box::new(Breaks) as Box<dyn RegistryLink>)
            } else {
                Ok(Box::new(Registry {
                    consume: true,
                    ..Registry::default()
                }) as Box<dyn RegistryLink>)
            }
        });
        let mut reporter = ThreadedReporter::spawn(connect, timing(2000));
        assert_eq!(reporter.report_press(&press(1)), ReportOutcome::Unanswered);
        assert_eq!(reporter.report_press(&press(2)), ReportOutcome::Consumed);
    }

    /// A registry error (no registry on the bus, an unknown method) is an
    /// answer, not a hang: the key is delivered at once.
    #[test]
    fn a_refused_report_is_answered_at_once() {
        struct Refuses;
        impl RegistryLink for Refuses {
            fn notify(&mut self, _: &DeviceEvent) -> Result<bool, LinkError> {
                Err(LinkError::Refused(
                    "org.freedesktop.DBus.Error.ServiceUnknown".into(),
                ))
            }
        }
        let connect: Connector = Box::new(|| Ok(Box::new(Refuses) as Box<dyn RegistryLink>));
        let mut reporter = ThreadedReporter::spawn(connect, timing(2000));
        let started = Instant::now();
        assert_eq!(reporter.report_press(&press(1)), ReportOutcome::Unanswered);
        assert!(started.elapsed() < Duration::from_millis(1000));
    }

    /// An error the registry answers with (no registry on the bus yet) is not
    /// a broken connection: the link is kept, not made again for every key.
    #[test]
    fn a_refused_report_keeps_the_connection() {
        struct Refuses;
        impl RegistryLink for Refuses {
            fn notify(&mut self, _: &DeviceEvent) -> Result<bool, LinkError> {
                Err(LinkError::Refused(
                    "org.freedesktop.DBus.Error.ServiceUnknown".into(),
                ))
            }
        }
        let attempts = Arc::new(AtomicUsize::new(0));
        let counted = attempts.clone();
        let connect: Connector = Box::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(Refuses) as Box<dyn RegistryLink>)
        });
        let mut reporter = ThreadedReporter::spawn(connect, timing(2000));
        for n in 0..3 {
            assert_eq!(reporter.report_press(&press(n)), ReportOutcome::Unanswered);
        }
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            1,
            "connected again after a refusal"
        );
    }

    /// Presses pile up behind a hung registry only so far; past that they
    /// are not reported, and the key is still delivered at once.
    #[test]
    fn presses_pile_up_only_so_far() {
        let door = Door::default();
        let registry = Registry {
            door: Some(door.clone()),
            ..Registry::default()
        };
        let mut reporter = ThreadedReporter::spawn(
            connecting_to(registry.clone()),
            Timing {
                queue: 2,
                ..timing(50)
            },
        );
        assert_eq!(reporter.report_press(&press(1)), ReportOutcome::Unanswered);
        for n in 2..6 {
            let started = Instant::now();
            assert_eq!(reporter.report_press(&press(n)), ReportOutcome::Unanswered);
            assert!(started.elapsed() < Duration::from_millis(500));
        }
        door.open();
        eventually("the registry catches up", || heard(&registry).len() >= 2);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            heard(&registry),
            vec![(DeviceEventKind::Pressed, 1), (DeviceEventKind::Pressed, 2)]
        );
    }

    /// A release is never lost to a full queue: the registry must hear it,
    /// or the Orca modifier (Insert, Caps Lock) stays down in libatspi's
    /// legacy device, and every later key matches an Orca command.
    #[test]
    fn a_release_is_never_lost_to_a_full_queue() {
        let door = Door::default();
        let registry = Registry {
            door: Some(door.clone()),
            ..Registry::default()
        };
        let mut reporter = ThreadedReporter::spawn(
            connecting_to(registry.clone()),
            Timing {
                queue: 2,
                ..timing(50)
            },
        );
        for n in 0..6 {
            reporter.report_press(&press(n));
        }
        for n in 0..6 {
            reporter.report_release(&release(n));
        }
        door.open();
        eventually("every release reaches the registry", || {
            heard(&registry)
                .iter()
                .filter(|(kind, _)| *kind == DeviceEventKind::Released)
                .count()
                == 6
        });
    }

    /// Presses that waited behind a hung registry longer than `stale` are not
    /// replayed when it wakes up: the reader would act on keys long gone.
    /// Releases still are.
    #[test]
    fn stale_presses_are_dropped_and_releases_kept() {
        let door = Door::default();
        let registry = Registry {
            door: Some(door.clone()),
            ..Registry::default()
        };
        let mut reporter = ThreadedReporter::spawn(
            connecting_to(registry.clone()),
            Timing {
                stale: Duration::from_millis(100),
                ..timing(50)
            },
        );
        reporter.report_press(&press(1));
        reporter.report_press(&press(2));
        reporter.report_press(&press(3));
        reporter.report_release(&release(1));
        std::thread::sleep(Duration::from_millis(300));
        door.open();
        eventually("the release reaches the registry", || {
            heard(&registry).contains(&(DeviceEventKind::Released, 1))
        });
        assert_eq!(
            heard(&registry),
            vec![
                (DeviceEventKind::Pressed, 1),
                (DeviceEventKind::Released, 1)
            ],
            "presses 2 and 3 were a quarter of a second old"
        );
    }

    /// A worker that dies (a panic in the D-Bus layer) holds no key and is
    /// noticed, not silently lost.
    #[test]
    fn a_worker_that_dies_is_noticed_and_holds_nothing() {
        struct Panics;
        impl RegistryLink for Panics {
            fn notify(&mut self, _: &DeviceEvent) -> Result<bool, LinkError> {
                panic!("a panic in the link, on purpose, in a test");
            }
        }
        let connect: Connector = Box::new(|| Ok(Box::new(Panics) as Box<dyn RegistryLink>));
        let mut reporter = ThreadedReporter::spawn(connect, timing(2000));
        let started = Instant::now();
        assert_eq!(reporter.report_press(&press(1)), ReportOutcome::Unanswered);
        assert_eq!(reporter.report_press(&press(2)), ReportOutcome::Unanswered);
        reporter.report_release(&release(2));
        assert!(started.elapsed() < Duration::from_millis(1000));
        assert!(reporter.stopped, "the worker's death went unnoticed");
    }
}
