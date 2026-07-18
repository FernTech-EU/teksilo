// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `LogView` demo — a scalable, tail-following streaming log.
//!
//! Run with: `cargo run -p log_view` (add `--release` for a smoother stream).
//!
//! What's on screen:
//!
//! - A [`LogView`] filling the window, fed by a synthetic producer that appends
//!   ~40 lines a frame while running. Let it run for a minute and it passes
//!   100 000+ lines — only the visible window is ever laid out, so memory and
//!   frame time stay flat (see the render-window figures in
//!   `text-typeset/docs/streaming-baseline.md`).
//! - **Severity colour** injected by the application: an `ERROR` line is red, a
//!   `WARN` line amber, `DEBUG` grey — the view knows how to colour a line, the
//!   app decides what an error looks like (it classifies the line text).
//! - **Follow the tail**: new lines stick the view to the bottom *while it is
//!   already at the bottom*. Scroll up to read history and it pauses; scroll
//!   back (or press ↓ Bottom) and it resumes — derived from scroll position.
//! - **Scrollback cap**: the oldest lines are evicted past 50 000, so the buffer
//!   stays bounded no matter how long it runs.
//!
//! Controls: **Start / Stop** the producer, **Burst** 10 000 lines at once (to
//! watch the windowing hold), **Clear**, and **↓ Bottom**. Select any visible
//! text and Ctrl+C copies it. The status line shows generated vs. retained.

use std::cell::Cell;
use std::rc::Rc;

use bastyde::core::WidgetPlacement;
use bastyde::prelude::*;
use bastyde::tokens::Color;
use bastyde::widgets::{
    Button, ButtonVariant, Expand, HStack, LogView, LogViewHandle, TextWidget, ThemeSwitcher,
    Toolbar,
};

/// Retained-line cap. The buffer oscillates just above this as it streams.
const SCROLLBACK: usize = 50_000;
/// Lines the producer appends per frame while running.
const LINES_PER_TICK: u64 = 40;
/// A single burst, to watch the windowed layout hold under a flood.
const BURST: u64 = 10_000;

const COMPONENTS: &[&str] = &[
    "net",
    "db",
    "cache",
    "auth",
    "scheduler",
    "worker",
    "gpu",
    "fs",
    "ipc",
    "render",
];

/// The colour an app paints a line by, from the line's own text. Language-agnostic:
/// the view calls this per visible line; the app decides what a level looks like.
fn severity_color(line: &str) -> Option<Color> {
    if line.contains(" ERROR ") {
        Some(Color::new(0.92, 0.36, 0.36, 1.0))
    } else if line.contains(" WARN ") {
        Some(Color::new(0.92, 0.72, 0.28, 1.0))
    } else if line.contains(" DEBUG ") {
        Some(Color::new(0.55, 0.60, 0.70, 1.0))
    } else {
        None
    }
}

/// A deterministic synthetic log line for sequence number `n`.
fn make_line(n: u64) -> String {
    // Deterministic pseudo-severity so the demo needs no RNG.
    let level = match n % 40 {
        7 | 29 => "ERROR",
        k if k % 6 == 0 => "WARN",
        k if k % 5 == 0 => "DEBUG",
        _ => "INFO",
    };
    let comp = COMPONENTS[(n as usize) % COMPONENTS.len()];
    let secs = n / 1000;
    format!(
        "[{:02}:{:02}:{:02}.{:03}] {} {}: event {} handled in {}ms",
        (secs / 3600) % 24,
        (secs / 60) % 60,
        secs % 60,
        n % 1000,
        level,
        comp,
        n,
        (n * 7) % 43,
    )
}

/// The demo root: owns the log, its handle, and the producer's run state.
struct LogDemo {
    log: Option<LogView>,
    handle: LogViewHandle,
    running: Signal<bool>,
    generated: Signal<u64>,
    /// The next sequence number to emit — a plain counter, off the reactive path.
    next: Rc<Cell<u64>>,

    log_id: Option<WidgetId>,
    toolbar_id: Option<WidgetId>,
}

impl std::fmt::Debug for LogDemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogDemo").finish_non_exhaustive()
    }
}

impl LogDemo {
    fn new() -> Self {
        let log = LogView::new()
            .scrollback_limit(SCROLLBACK)
            .severity_highlighter(severity_color)
            .font_family("monospace");
        let handle = log.handle();
        Self {
            log: Some(log),
            handle,
            running: Signal::new(false),
            generated: Signal::new(0),
            next: Rc::new(Cell::new(0)),
            log_id: None,
            toolbar_id: None,
        }
    }

    /// Emit `count` lines now and advance the counters.
    fn emit(&self, count: u64) {
        let start = self.next.get();
        let batch: Vec<String> = (0..count).map(|i| make_line(start + i)).collect();
        self.next.set(start + count);
        self.generated.set(self.generated.get() + count);
        self.handle.append_lines(batch);
    }

    fn controls(&self, ctx: &mut BuildContext) -> WidgetId {
        // "N generated · M retained · running/paused" — all reactive.
        let status = {
            let generated = self.generated.clone();
            let retained = self.handle.line_count();
            let running = self.running.clone();
            generated.zip3(&retained, &running).map(|(g, r, run)| {
                let state = if *run { "streaming" } else { "paused" };
                format!("{g} generated \u{00B7} {r} retained \u{00B7} {state}")
            })
        };

        let start_btn = {
            let running = self.running.clone();
            Button::new(lit!("Start / Stop"))
                .variant(ButtonVariant::Filled)
                .on_activate_fn(move |ctx| {
                    let now = !running.get();
                    running.set(now);
                    if now {
                        // Kick the loop; each append then keeps frames coming.
                        ctx.request_frame();
                    }
                })
        };

        let burst_btn = {
            let handle = self.handle.clone();
            let generated = self.generated.clone();
            let next = self.next.clone();
            Button::new(lit!("Burst 10k")).on_activate_fn(move |ctx| {
                let start = next.get();
                let batch: Vec<String> = (0..BURST).map(|i| make_line(start + i)).collect();
                next.set(start + BURST);
                generated.set(generated.get() + BURST);
                handle.append_lines(batch);
                ctx.request_frame();
            })
        };

        let clear_btn = {
            let handle = self.handle.clone();
            let generated = self.generated.clone();
            let next = self.next.clone();
            Button::new(lit!("Clear")).on_activate_fn(move |ctx| {
                handle.clear();
                generated.set(0);
                next.set(0);
                ctx.request_frame();
            })
        };

        let bottom_btn = {
            let handle = self.handle.clone();
            Button::new(lit!("\u{2193} Bottom")).on_activate_fn(move |ctx| {
                handle.scroll_to_bottom();
                ctx.request_frame();
            })
        };

        ctx.add(
            Toolbar::new().child(
                HStack::new()
                    .spacing(8.0)
                    .child(start_btn)
                    .child(burst_btn)
                    .child(clear_btn)
                    .child(bottom_btn)
                    .child(
                        Expand::new().child(
                            TextWidget::new(lit!(""))
                                .text(status)
                                .style(TextStyleRole::Small),
                        ),
                    )
                    .child(ThemeSwitcher::new()),
            ),
        )
    }
}

impl Widget for LogDemo {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // The producer: while running, append a batch each frame. The LogView
        // wakes the loop on every append, so the stream self-sustains until Stop.
        {
            let running = self.running.clone();
            let next = self.next.clone();
            let handle = self.handle.clone();
            let generated = self.generated.clone();
            ctx.effect(&ctx.frame_tick(), move |_delta| {
                if !running.get() {
                    return;
                }
                let start = next.get();
                let batch: Vec<String> =
                    (0..LINES_PER_TICK).map(|i| make_line(start + i)).collect();
                next.set(start + LINES_PER_TICK);
                generated.set(generated.get() + LINES_PER_TICK);
                handle.append_lines(batch);
            });
        }

        let toolbar = self.controls(ctx);
        self.toolbar_id = Some(toolbar);

        let log = self.log.take().expect("LogDemo built once");
        let log_id = ctx.add(log);
        self.log_id = Some(log_id);

        // Seed a few lines so the view is not empty on launch.
        self.emit(30);

        vec![toolbar, log_id]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> bastyde::core::widget::LayoutResponse {
        Size::new(
            proposal.width.unwrap_or(900.0),
            proposal.height.unwrap_or(600.0),
        )
        .into()
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let toolbar_h = self
            .toolbar_id
            .and_then(|id| ctx.child_size(id, SizeProposal::with_width(bounds.width)))
            .map(|s| s.height)
            .unwrap_or(44.0);
        for child in children.iter_mut() {
            if Some(child.id) == self.toolbar_id {
                child.origin = Point::new(bounds.x, bounds.y);
                child.size = Size::new(bounds.width, toolbar_h);
            } else if Some(child.id) == self.log_id {
                child.origin = Point::new(bounds.x, bounds.y + toolbar_h);
                child.size = Size::new(bounds.width, (bounds.height - toolbar_h).max(0.0));
            }
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        let mut ids = Vec::new();
        ids.extend(self.toolbar_id);
        ids.extend(self.log_id);
        ids
    }
}

fn main() {
    BastydeAppBuilder::new()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::dark())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde \u{2014} LogView (streaming)")
                .size(960, 640)
                .root(|tree, _state| tree.add(LogDemo::new())),
        )
        .run();
}
