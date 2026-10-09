// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `live-image-bench`: one `LiveImage` fed by a producer whose workload is
//! chosen on the command line, to measure what a live picture costs in a real
//! window. The window shows the picture and nothing else, so what its UI
//! thread does is the picture's cost plus the framework's floor.
//!
//! `tools/live_image_measure.py` runs it under `TEKSILO_IDLE_TRACE=1` with the
//! `live-image-timings` feature, and reads the trace, the UI thread's CPU time
//! and the process's GPU memory:
//!
//! ```text
//! cargo run -p live-image-bench --release --features live-image-timings -- \
//!     --workload rects --seconds 60
//! ```
//!
//! Options:
//!
//! - `--workload rects|full|copy`: what each commit writes. `rects`, the
//!   default, is four rects covering 8 % of the frame, moving every commit.
//!   `full` is a whole frame, `write_frame`. `copy` is copies only: the frame
//!   scrolls up 16 rows (`copy_within`) and the strip uncovered is written
//!   (`write_rect`), as a terminal scrolls.
//! - `--size WxH`: the frame, 720x1280 by default.
//! - `--rate HZ`: commits a second, 60 by default.
//! - `--seconds S`: exit S seconds after start; by default, run until closed.
//! - `--rotate-every S`: swap the frame's sides every S seconds.
//! - `--second-window-at S`: S seconds after start, open a second window
//!   showing the same source (0: with the first).
//! - `--close-second-at S`: close it S seconds after start.
//! - `--second-window-every S`: from `--second-window-at` (or S) on, open
//!   the second window and close it again every S seconds.
//! - `--churn S`: every S seconds, take the picture out of the window, or put
//!   a new one back.
//!
//! Each scheduled event prints a `live_image_bench t=<seconds> <event>` line
//! on stderr, so a script can line its samples up with it. So does a commit
//! that keeps the producer more than 50 ms (`slow commit`): a commit wakes
//! every window that shows the source, and must never wait for one, not even
//! one closing.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use teksilo::core::WidgetPlacement;
use teksilo::core::binding::BindingLevel;
use teksilo::prelude::*;
use teksilo::widgets::{ImageFit, LiveImage, LiveImageSizing};

const USAGE: &str = "usage: live-image-bench [--workload rects|full|copy] [--size WxH] \
                     [--rate HZ] [--seconds S] [--rotate-every S] [--second-window-at S] \
                     [--close-second-at S] [--churn S]";

/// A commit that keeps the producer longer than this is reported.
const SLOW_COMMIT: Duration = Duration::from_millis(50);

/// What each commit writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Workload {
    Rects,
    Full,
    Copy,
}

#[derive(Debug, Clone)]
struct Options {
    workload: Workload,
    size: (u32, u32),
    rate: f64,
    seconds: Option<f64>,
    rotate_every: Option<f64>,
    second_window_at: Option<f64>,
    close_second_at: Option<f64>,
    second_window_every: Option<f64>,
    churn: Option<f64>,
}

impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Options {
            workload: Workload::Rects,
            size: (720, 1280),
            rate: 60.0,
            seconds: None,
            rotate_every: None,
            second_window_at: None,
            close_second_at: None,
            second_window_every: None,
            churn: None,
        };
        while let Some(flag) = args.next() {
            let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
            let seconds = |text: String| -> Result<f64, String> {
                text.parse::<f64>()
                    .ok()
                    .filter(|s| s.is_finite() && *s >= 0.0)
                    .ok_or_else(|| format!("{flag}: `{text}` is not a number of seconds"))
            };
            match flag.as_str() {
                "--workload" => {
                    options.workload = match value()?.as_str() {
                        "rects" => Workload::Rects,
                        "full" => Workload::Full,
                        "copy" => Workload::Copy,
                        other => return Err(format!("no workload `{other}`")),
                    }
                }
                "--size" => {
                    let text = value()?;
                    options.size = text
                        .split_once('x')
                        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                        .filter(|&(w, h): &(u32, u32)| {
                            (32..=16_384).contains(&w) && (32..=16_384).contains(&h)
                        })
                        .ok_or_else(|| {
                            format!("--size: `{text}` is not WxH, 32 to 16384 a side")
                        })?;
                }
                "--rate" => {
                    let text = value()?;
                    options.rate = text
                        .parse::<f64>()
                        .ok()
                        .filter(|r| *r > 0.0 && *r <= 1000.0)
                        .ok_or_else(|| format!("--rate: `{text}` is not 1 to 1000 Hz"))?;
                }
                "--seconds" => options.seconds = Some(seconds(value()?)?),
                "--rotate-every" => {
                    options.rotate_every = Some(seconds(value()?)?).filter(|s| *s > 0.0)
                }
                "--second-window-at" => options.second_window_at = Some(seconds(value()?)?),
                "--close-second-at" => options.close_second_at = Some(seconds(value()?)?),
                "--second-window-every" => {
                    options.second_window_every = Some(seconds(value()?)?).filter(|s| *s > 0.0)
                }
                "--churn" => options.churn = Some(seconds(value()?)?).filter(|s| *s > 0.0),
                "-h" | "--help" => return Err(String::new()),
                other => return Err(format!("unknown option `{other}`")),
            }
        }
        Ok(options)
    }
}

/// Print a line marking an event, with the time since start.
fn note(started: Instant, event: &str) {
    eprintln!(
        "live_image_bench t={:.3} {event}",
        started.elapsed().as_secs_f64()
    );
}

// ── The producer ──

/// A frame of `w × h` BGRX pixels, a gradient.
fn pattern(w: u32, h: u32) -> Vec<u8> {
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            px.extend_from_slice(&[(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255]);
        }
    }
    px
}

/// The four rects commit `n` writes: together 8 % of a `w × h` frame, each
/// twice as wide as it is tall, each moving along its own path.
fn rects(w: u32, h: u32, n: u64) -> [PixelRect; 4] {
    let area = 0.02 * f64::from(w) * f64::from(h);
    let rw = ((2.0 * area).sqrt() as u32).clamp(1, w);
    let rh = (rw / 2).clamp(1, h);
    std::array::from_fn(|i| {
        let i = i as u64;
        let x = (n * (5 + 2 * i) + i * 157) % u64::from(w - rw + 1);
        let y = (n * (3 + i) + i * 311) % u64::from(h - rh + 1);
        PixelRect::new(x as u32, y as u32, rw, rh)
    })
}

/// Fill `buf` with `len` bytes of one colour from `n` and `i`.
fn fill(buf: &mut Vec<u8>, len: usize, n: u64, i: u64) {
    let c = [
        (n * 7 + i * 60) as u8,
        (n * 3 + i * 90) as u8,
        (n + i * 30) as u8,
        255,
    ];
    buf.clear();
    buf.extend(c.iter().copied().cycle().take(len));
}

fn produce(writer: LiveImageWriter, options: Options, started: Instant) {
    let (mut w, mut h) = options.size;
    let mut frame = pattern(w, h);
    let mut scratch = Vec::new();
    if let Err(error) = writer.write_frame(w, h, &frame, w as usize * 4) {
        eprintln!("live-image-bench: the first frame was refused: {error}");
        return;
    }
    let period = Duration::from_secs_f64(1.0 / options.rate);
    let rotation = options.rotate_every.map(Duration::from_secs_f64);
    let mut next_rotation = rotation.map(|every| started + every);
    let mut next = Instant::now() + period;
    let mut n = 0u64;
    loop {
        n += 1;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        }
        next = next.max(Instant::now() - period) + period;

        if let (Some(at), Some(every)) = (next_rotation, rotation)
            && Instant::now() >= at
        {
            (w, h) = (h, w);
            frame = pattern(w, h);
            let _ = writer.write_frame(w, h, &frame, w as usize * 4);
            note(started, &format!("rotated {w}x{h}"));
            next_rotation = Some(at + every);
            continue;
        }
        let stride = w as usize * 4;
        let commit_started = Instant::now();
        match options.workload {
            Workload::Full => {
                // Eight rows change, so every frame differs from the last.
                let band = (n * 8 % u64::from(h)) as usize;
                for row in band..(band + 8).min(h as usize) {
                    fill(&mut scratch, stride, n, 0);
                    frame[row * stride..(row + 1) * stride].copy_from_slice(&scratch);
                }
                let _ = writer.write_frame(w, h, &frame, stride);
            }
            Workload::Rects => {
                if let Ok(mut guard) = writer.lock() {
                    for (i, rect) in rects(w, h, n).into_iter().enumerate() {
                        fill(
                            &mut scratch,
                            (rect.width * rect.height * 4) as usize,
                            n,
                            i as u64,
                        );
                        let _ = guard.write_rect(rect, &scratch, rect.width as usize * 4);
                    }
                    guard.commit();
                }
            }
            Workload::Copy => {
                let strip = 16.min(h - 1);
                if let Ok(mut guard) = writer.lock() {
                    let _ = guard.copy_within(PixelRect::new(0, strip, w, h - strip), 0, 0);
                    fill(&mut scratch, strip as usize * stride, n, 0);
                    let _ =
                        guard.write_rect(PixelRect::new(0, h - strip, w, strip), &scratch, stride);
                    guard.commit();
                }
            }
        }
        let took = commit_started.elapsed();
        if took > SLOW_COMMIT {
            note(started, &format!("slow commit {} ms", took.as_millis()));
        }
    }
}

// ── The director: the scheduled events, posted to the UI thread ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    OpenSecond,
    CloseSecond,
    Churn,
    Exit,
}

/// The events `options` schedules, in the order they fall due, as seconds
/// since start. A repeated event (churn, the second window's cycle) repeats
/// up to the exit, or for a day when the run has none.
fn schedule(options: &Options) -> Vec<(f64, Event)> {
    let mut events = Vec::new();
    let end = options.seconds.unwrap_or(86_400.0);
    match options.second_window_every {
        Some(every) => {
            let mut at = options.second_window_at.unwrap_or(every);
            let mut open = true;
            while at < end {
                events.push((
                    at,
                    if open {
                        Event::OpenSecond
                    } else {
                        Event::CloseSecond
                    },
                ));
                (at, open) = (at + every, !open);
            }
        }
        None => events.extend(options.second_window_at.map(|at| (at, Event::OpenSecond))),
    }
    events.extend(options.close_second_at.map(|at| (at, Event::CloseSecond)));
    events.extend(options.seconds.map(|at| (at, Event::Exit)));
    if let Some(every) = options.churn {
        let mut at = every;
        while at < end {
            events.push((at, Event::Churn));
            at += every;
        }
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0));
    events
}

/// Mounts a new `LiveImage` of `source` while `mounted` holds, and none
/// otherwise: a change rebuilds it, so each mount is a new widget and a new
/// attachment, and each unmount destroys them.
#[derive(Debug)]
struct Mount {
    mounted: Signal<bool>,
    source: LiveImageSource,
    child: Option<WidgetId>,
}

impl Widget for Mount {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        self.mounted
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
        self.child = self.mounted.get().then(|| {
            ctx.add(
                LiveImage::new(self.source.clone())
                    .sizing(LiveImageSizing::Fill)
                    .fit(ImageFit::Contain)
                    .alt(lit!("Benchmark picture")),
            )
        });
        self.child.into_iter().collect()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child.into_iter().collect()
    }
}

fn main() {
    let options = match Options::parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            if !error.is_empty() {
                eprintln!("live-image-bench: {error}");
            }
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    let started = Instant::now();
    let source = LiveImageSource::builder(LivePixelFormat::Bgrx8)
        .label("bench")
        .build();
    {
        let (writer, options) = (source.writer(), options.clone());
        std::thread::Builder::new()
            .name("producer".into())
            .spawn(move || produce(writer, options, started))
            .expect("spawn the producer");
    }
    note(
        started,
        &format!(
            "start workload={:?} size={}x{} rate={}",
            options.workload, options.size.0, options.size.1, options.rate
        ),
    );

    let mounted: Rc<RefCell<Option<Signal<bool>>>> = Rc::new(RefCell::new(None));
    let second: Rc<Cell<Option<TeksiloWindowId>>> = Rc::new(Cell::new(None));
    let events = schedule(&options);
    let (main_source, second_source, root_mounted) =
        (source.clone(), source.clone(), mounted.clone());

    TeksiloAppBuilder::new()
        .theme(teksilo::presets::intui::dark())
        .on_ready(move |proxy| {
            std::thread::Builder::new()
                .name("director".into())
                .spawn(move || {
                    for (at, event) in events {
                        let due = started + Duration::from_secs_f64(at);
                        let now = Instant::now();
                        if due > now {
                            std::thread::sleep(due - now);
                        }
                        proxy.send_external(event);
                    }
                })
                .expect("spawn the director");
        })
        .on_external_with_ctx(move |payload, ctx| {
            let Some(event) = payload.downcast_ref::<Event>() else {
                return false;
            };
            match event {
                Event::OpenSecond => {
                    let source = second_source.clone();
                    second.set(Some(
                        ctx.open_window(
                            WindowConfig::new()
                                .title("live-image-bench — second window")
                                .size(600, 600)
                                .root(move |tree, _state| {
                                    tree.add(
                                        LiveImage::new(source.clone())
                                            .sizing(LiveImageSizing::Fill)
                                            .fit(ImageFit::Contain)
                                            .alt(lit!("Benchmark picture, second window")),
                                    )
                                }),
                        ),
                    ));
                    note(started, "opened second window");
                }
                Event::CloseSecond => {
                    if let Some(id) = second.take() {
                        ctx.close_window_by_id(id);
                        note(started, "closed second window");
                    }
                }
                Event::Churn => {
                    if let Some(signal) = mounted.borrow().as_ref() {
                        let on = !signal.get();
                        signal.set(on);
                        note(started, if on { "mounted" } else { "unmounted" });
                    }
                }
                Event::Exit => {
                    note(started, "exit");
                    std::process::exit(0);
                }
            }
            true
        })
        .initial_window(
            WindowConfig::new()
                .title("live-image-bench")
                .size(900, 900)
                .root(move |tree, _state| {
                    let signal = Signal::new(true);
                    *root_mounted.borrow_mut() = Some(signal.clone());
                    tree.add(Mount {
                        mounted: signal,
                        source: main_source.clone(),
                        child: None,
                    })
                }),
        )
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn the_four_rects_cover_eight_percent_of_the_frame_and_stay_inside_it() {
        for (w, h) in [(720, 1280), (1280, 720), (1920, 1080), (64, 64)] {
            for n in [0, 1, 17, 10_000] {
                let rects = rects(w, h, n);
                let area: u64 = rects
                    .iter()
                    .map(|r| u64::from(r.width) * u64::from(r.height))
                    .sum();
                let share = area as f64 / (f64::from(w) * f64::from(h));
                assert!((0.07..=0.081).contains(&share), "{w}x{h}: {share}");
                for r in rects {
                    assert!(
                        r.x + r.width <= w && r.y + r.height <= h,
                        "{w}x{h} {n}: {r:?}"
                    );
                }
            }
        }
        assert_eq!(
            rects(720, 1280, 0)[0].width * rects(720, 1280, 0)[0].height,
            192 * 96
        );
    }

    #[test]
    fn options_parse_and_refuse_what_they_do_not_know() {
        let o = parse(&[
            "--workload",
            "full",
            "--size",
            "1920x1080",
            "--seconds",
            "70",
        ])
        .unwrap();
        assert_eq!(
            (o.workload, o.size, o.seconds),
            (Workload::Full, (1920, 1080), Some(70.0))
        );
        assert!(parse(&["--workload", "video"]).is_err());
        assert!(parse(&["--size", "1920"]).is_err());
        assert!(parse(&["--rate", "0"]).is_err());
        assert!(parse(&["--frobnicate"]).is_err());
        assert!(parse(&["--seconds"]).is_err());
    }

    #[test]
    fn the_second_window_cycles_open_and_shut() {
        let o = parse(&[
            "--second-window-at",
            "1",
            "--second-window-every",
            "2",
            "--seconds",
            "6",
        ])
        .unwrap();
        assert_eq!(
            schedule(&o),
            vec![
                (1.0, Event::OpenSecond),
                (3.0, Event::CloseSecond),
                (5.0, Event::OpenSecond),
                (6.0, Event::Exit),
            ]
        );
    }

    #[test]
    fn the_schedule_is_in_time_order_and_churns_until_the_exit() {
        let o = parse(&[
            "--churn",
            "0.5",
            "--seconds",
            "2",
            "--second-window-at",
            "0.7",
        ])
        .unwrap();
        let events = schedule(&o);
        assert_eq!(
            events,
            vec![
                (0.5, Event::Churn),
                (0.7, Event::OpenSecond),
                (1.0, Event::Churn),
                (1.5, Event::Churn),
                (2.0, Event::Exit),
            ]
        );
    }
}
