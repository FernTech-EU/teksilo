// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `LiveImage` demo: a picture another thread rewrites sixty times a second,
//! the way a VM screen, a video or a camera preview is.
//!
//! Run with `cargo run -p live-image-demo`.
//!
//! What is on screen:
//!
//! - A phone-shaped guest screen, 720 × 1280, that a producer thread draws at
//!   60 Hz: a frame counter in its status bar, a bouncing ball and the
//!   guest's cursor. The guest paints what changed into its framebuffer, and
//!   each frame commits only the rectangles that changed, so the window
//!   uploads a few kilobytes, repaints no widget and lays nothing out.
//! - **Whole frames** hands the whole framebuffer over at every frame
//!   instead, as a VM host copying its guest's framebuffer at each vsync
//!   does, through a `LiveImageDiffWriter`, which commits only what changed.
//!   With **Still guest** on too, the producer keeps handing over identical
//!   frames at 60 Hz and the window still wakes for nothing.
//! - **Rotate** turns the guest: the producer resizes its frame to
//!   1280 × 720 and back, and the picture's box takes the new shape.
//! - **Pause producer** stops it. The window then has nothing to do and
//!   wakes for nothing: with `TEKSILO_IDLE_TRACE=1` the trace stays silent.
//! - **Nearest** samples the picture pixel for pixel instead of smoothing it.
//! - **Second window** shows the same source in another window: one
//!   producer, one texture per window.
//! - The guest takes input: press, drag, touch with several fingers, and type
//!   once the picture has focus. Each event is mapped to the guest pixel it
//!   lands on, the guest's cursor moves there, and the line under the picture
//!   says what the guest received. Ctrl+Tab leaves the picture.
//!
//! The line under the picture is also the picture's accessible description:
//! `example_live_image.py`, in the probe harness's examples, drives this demo
//! through it.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use teksilo::core::event::{EventResponse, Modifiers, WidgetEvent};
use teksilo::core::{LongPressRole, MultiContact, TouchAction};
use teksilo::prelude::*;
use teksilo::widgets::{
    Button, Checkbox, Expand, HStack, ImageFit, LiveImage, LiveImageHandle, LiveImageSizing,
    ScalingFilter, Switcher, TextWidget, VStack,
};

/// The guest's screen, portrait.
const GUEST_W: u32 = 720;
const GUEST_H: u32 = 1280;
/// One frame at 60 Hz.
const FRAME: Duration = Duration::from_micros(16_667);
/// The status bar's height, and its frame counter: sixteen blocks, one per
/// bit.
const BAR_H: u32 = 48;
const BLOCK: u32 = 24;
const BLOCK_GAP: u32 = 4;
const BLOCKS_X: u32 = 24;
const BLOCKS_Y: u32 = 12;
/// The ball's diameter, and the cursor's arm.
const BALL: u32 = 64;
const ARM: u32 = 16;

// ── The producer ──

/// What the window asks of the producer. Shared with the producer thread.
struct Controls {
    paused: Mutex<bool>,
    resumed: Condvar,
    rotate: AtomicBool,
    /// Hand the whole framebuffer over every frame, through the diff writer.
    whole_frames: AtomicBool,
    /// The guest stops moving: its counter and its ball hold still.
    still: AtomicBool,
    /// The guest's cursor, `x << 32 | y` in source pixels, or `NO_CURSOR`.
    cursor: AtomicU64,
}

const NO_CURSOR: u64 = u64::MAX;

impl Controls {
    fn new() -> Self {
        Self {
            paused: Mutex::new(false),
            resumed: Condvar::new(),
            rotate: AtomicBool::new(false),
            whole_frames: AtomicBool::new(false),
            still: AtomicBool::new(false),
            cursor: AtomicU64::new(NO_CURSOR),
        }
    }

    fn set_paused(&self, paused: bool) {
        *self.paused.lock().unwrap_or_else(|e| e.into_inner()) = paused;
        self.resumed.notify_all();
    }

    /// Block while paused: a paused producer costs nothing.
    fn wait_while_paused(&self) {
        let mut paused = self.paused.lock().unwrap_or_else(|e| e.into_inner());
        while *paused {
            paused = self.resumed.wait(paused).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn set_cursor(&self, at: Option<(u32, u32)>) {
        let packed = at.map_or(NO_CURSOR, |(x, y)| u64::from(x) << 32 | u64::from(y));
        self.cursor.store(packed, Ordering::Relaxed);
    }

    fn cursor(&self) -> Option<(u32, u32)> {
        match self.cursor.load(Ordering::Relaxed) {
            NO_CURSOR => None,
            packed => Some(((packed >> 32) as u32, packed as u32)),
        }
    }
}

/// What the guest shows: everything one frame's pixels are computed from.
struct Guest {
    width: u32,
    height: u32,
    frame: u64,
    ball: (f32, f32),
    velocity: (f32, f32),
    cursor: Option<(u32, u32)>,
}

impl Guest {
    fn new() -> Self {
        Self {
            width: GUEST_W,
            height: GUEST_H,
            frame: 0,
            ball: (100.0, 300.0),
            velocity: (5.0, 7.0),
            cursor: None,
        }
    }

    fn rotate(&mut self) {
        (self.width, self.height) = (self.height, self.width);
        let (max_x, max_y) = self.ball_limits();
        self.ball = (self.ball.0.min(max_x), self.ball.1.min(max_y));
        self.cursor = None;
    }

    fn ball_limits(&self) -> (f32, f32) {
        ((self.width - BALL) as f32, (self.height - BALL) as f32)
    }

    fn ball_rect(&self) -> PixelRect {
        PixelRect::new(self.ball.0 as u32, self.ball.1 as u32, BALL, BALL)
    }

    fn cursor_rect(&self) -> Option<PixelRect> {
        let (x, y) = self.cursor?;
        let x0 = x.saturating_sub(ARM);
        let y0 = y.saturating_sub(ARM);
        let x1 = (x + ARM + 1).min(self.width);
        let y1 = (y + ARM + 1).min(self.height);
        Some(PixelRect::new(x0, y0, x1 - x0, y1 - y0))
    }

    fn counter_rect() -> PixelRect {
        PixelRect::new(BLOCKS_X, BLOCKS_Y, 16 * (BLOCK + BLOCK_GAP), BLOCK)
    }

    /// Move the ball one frame, bouncing off the edges below the bar.
    fn advance(&mut self) {
        self.frame += 1;
        let (max_x, max_y) = self.ball_limits();
        let (mut x, mut y) = (self.ball.0 + self.velocity.0, self.ball.1 + self.velocity.1);
        if x < 0.0 || x > max_x {
            self.velocity.0 = -self.velocity.0;
            x = x.clamp(0.0, max_x);
        }
        if y < BAR_H as f32 || y > max_y {
            self.velocity.1 = -self.velocity.1;
            y = y.clamp(BAR_H as f32, max_y);
        }
        self.ball = (x, y);
    }

    /// The colour of pixel `(x, y)`, as BGRX bytes: the background, the
    /// status bar and its counter, then the ball, then the cursor.
    fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let mut rgb = if y < BAR_H {
            let i = x.wrapping_sub(BLOCKS_X) / (BLOCK + BLOCK_GAP);
            let in_block = x >= BLOCKS_X
                && i < 16
                && (x - BLOCKS_X) % (BLOCK + BLOCK_GAP) < BLOCK
                && (BLOCKS_Y..BLOCKS_Y + BLOCK).contains(&y);
            if in_block && self.frame >> i & 1 == 1 {
                [120, 220, 140]
            } else if in_block {
                [40, 48, 56]
            } else {
                [24, 28, 34]
            }
        } else {
            let t = (y - BAR_H) as f32 / (self.height - BAR_H) as f32;
            let s = x as f32 / self.width as f32;
            [
                (40.0 + 60.0 * t) as u8,
                (70.0 + 40.0 * s) as u8,
                (140.0 - 50.0 * t) as u8,
            ]
        };
        let (bx, by) = (
            self.ball.0 + BALL as f32 / 2.0,
            self.ball.1 + BALL as f32 / 2.0,
        );
        let (dx, dy) = (x as f32 + 0.5 - bx, y as f32 + 0.5 - by);
        if dx * dx + dy * dy <= (BALL as f32 / 2.0).powi(2) {
            rgb = [240, 180, 60];
        }
        if let Some((cx, cy)) = self.cursor {
            let (dx, dy) = (x.abs_diff(cx), y.abs_diff(cy));
            if (dx <= ARM && dy <= 1) || (dy <= ARM && dx <= 1) {
                rgb = if dx == 0 || dy == 0 {
                    [255, 255, 255]
                } else {
                    [0, 0, 0]
                };
            }
        }
        [rgb[2], rgb[1], rgb[0], 0]
    }

    /// The bytes a row of the guest's framebuffer takes.
    fn stride(&self) -> usize {
        (self.width * 4) as usize
    }

    /// Paint `rect` into the guest's framebuffer.
    fn paint(&self, framebuffer: &mut [u8], rect: PixelRect) {
        let Some(rect) = rect.intersect(&PixelRect::full(self.width, self.height)) else {
            return;
        };
        let stride = self.stride();
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                let at = y as usize * stride + x as usize * 4;
                framebuffer[at..at + 4].copy_from_slice(&self.pixel(x, y));
            }
        }
    }

    /// The whole framebuffer, at the guest's size.
    fn repaint(&self, framebuffer: &mut Vec<u8>) {
        framebuffer.clear();
        framebuffer.resize(self.stride() * self.height as usize, 0);
        self.paint(framebuffer, PixelRect::full(self.width, self.height));
    }
}

/// Run the guest at 60 Hz until the process ends. It paints what changed
/// into its framebuffer; the producer then commits those rects in a
/// transaction, or hands the whole framebuffer to a `LiveImageDiffWriter`,
/// which finds them itself.
fn produce(writer: LiveImageWriter, controls: Arc<Controls>) {
    let mut guest = Guest::new();
    let mut framebuffer = Vec::new();
    guest.repaint(&mut framebuffer);
    let mut writer = LiveImageDiffWriter::new(writer);
    let mut dirty = vec![PixelRect::full(guest.width, guest.height)];
    let mut next = Instant::now();
    loop {
        if controls.whole_frames.load(Ordering::Relaxed) {
            let _ = writer.write_frame(guest.width, guest.height, &framebuffer, guest.stride());
        } else if !dirty.is_empty()
            && let Ok(mut guard) = writer.writer().lock()
            && guard.resize(guest.width, guest.height).is_ok()
        {
            for rect in &dirty {
                let at = rect.y as usize * guest.stride() + rect.x as usize * 4;
                let _ = guard.write_rect(*rect, &framebuffer[at..], guest.stride());
            }
            guard.commit();
        }
        dirty.clear();

        next += FRAME;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now;
        }
        controls.wait_while_paused();

        if controls.rotate.swap(false, Ordering::AcqRel) {
            guest.rotate();
            controls.set_cursor(None);
            guest.repaint(&mut framebuffer);
            dirty.push(PixelRect::full(guest.width, guest.height));
            continue;
        }
        let old_ball = guest.ball_rect();
        let old_cursor = guest.cursor_rect();
        guest.cursor = controls
            .cursor()
            .filter(|&(x, y)| x < guest.width && y < guest.height);
        if !controls.still.load(Ordering::Relaxed) {
            guest.advance();
            dirty.push(Guest::counter_rect());
            dirty.push(old_ball.union(&guest.ball_rect()));
        }
        if old_cursor != guest.cursor_rect() {
            dirty.extend([old_cursor, guest.cursor_rect()].into_iter().flatten());
        }
        for rect in &dirty {
            guest.paint(&mut framebuffer, *rect);
        }
        let bounds = PixelRect::full(guest.width, guest.height);
        dirty.retain_mut(|rect| match rect.intersect(&bounds) {
            Some(inside) => {
                *rect = inside;
                true
            }
            None => false,
        });
    }
}

// ── The guest's input ──

/// What the guest received last, and the contacts it holds down.
#[derive(Clone)]
struct GuestInput {
    log: Signal<String>,
    down: Rc<RefCell<BTreeSet<u64>>>,
    controls: Arc<Controls>,
}

impl GuestInput {
    fn new(controls: Arc<Controls>) -> Self {
        Self {
            log: Signal::new("nothing yet".to_string()),
            down: Rc::default(),
            controls,
        }
    }

    /// A pointer event, mapped to the guest pixel it lands on.
    fn pointer(&self, handle: &LiveImageHandle, event: &WidgetEvent) -> EventResponse {
        let (phase, position, pointer) = match event {
            WidgetEvent::PointerDown {
                position, pointer, ..
            } => ("down", *position, *pointer),
            WidgetEvent::PointerMove {
                position, pointer, ..
            } => ("move", *position, *pointer),
            WidgetEvent::PointerUp {
                position, pointer, ..
            } => ("up", *position, *pointer),
            _ => return EventResponse::Ignored,
        };
        let id = pointer.id.get();
        let held = {
            let mut down = self.down.borrow_mut();
            match phase {
                "down" => {
                    down.insert(id);
                }
                "up" => {
                    down.remove(&id);
                }
                _ => {}
            }
            down.len()
        };
        let kind = format!("{:?}", pointer.kind).to_lowercase();
        let at = handle.map_to_source(position);
        if at.is_some() {
            self.controls.set_cursor(at);
        }
        // A hover with nothing down is not worth a line.
        if phase == "move" && held == 0 {
            return EventResponse::Handled;
        }
        self.log.set(match at {
            Some((x, y)) => format!("{phase} {kind} {id} at {x},{y} · {held} down"),
            None => format!("{phase} {kind} {id} outside · {held} down"),
        });
        EventResponse::Handled
    }

    fn cancel(&self, id: u64, kind: String) {
        let held = {
            let mut down = self.down.borrow_mut();
            down.remove(&id);
            down.len()
        };
        self.log.set(format!("cancel {kind} {id} · {held} down"));
    }

    fn key(&self, event: &WidgetEvent) -> EventResponse {
        let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
            return EventResponse::Ignored;
        };
        self.log.set(format!("key {}{key:?}", chord(*modifiers)));
        EventResponse::Handled
    }
}

/// `Ctrl+`, `Shift+`… for the modifiers held, in that order.
fn chord(modifiers: Modifiers) -> String {
    [
        (modifiers.ctrl(), "Ctrl+"),
        (modifiers.alt(), "Alt+"),
        (modifiers.shift(), "Shift+"),
        (modifiers.super_key(), "Super+"),
    ]
    .into_iter()
    .filter_map(|(held, name)| held.then_some(name))
    .collect()
}

/// The guest screen: a `LiveImage` that takes every input a guest takes.
fn guest_screen(
    source: &LiveImageSource,
    input: &GuestInput,
    scaling: ScalingFilter,
) -> impl Widget {
    let handle = LiveImageHandle::new();
    let (pointer_input, pointer_handle) = (input.clone(), handle.clone());
    let (cancel_input, key_input) = (input.clone(), input.clone());
    LiveImage::new(source.clone())
        .with_handle(&handle)
        .sizing(LiveImageSizing::Aspect)
        .fit(ImageFit::Contain)
        .scaling(scaling)
        .placeholder(lit!("Waiting for the guest"))
        .alt(lit!("Guest screen"))
        .access_description(input.log.clone())
        .focusable(true)
        .keyboard_capture(true)
        .multi_contact(MultiContact::All)
        .long_press_role(LongPressRole::None)
        .touch_action(TouchAction::NONE)
        .on_pointer_event(move |event, _ctx| pointer_input.pointer(&pointer_handle, event))
        .on_pointer_cancel(move |pointer, _reason, _ctx| {
            cancel_input.cancel(
                pointer.id.get(),
                format!("{:?}", pointer.kind).to_lowercase(),
            );
        })
        .on_key(move |event, _ctx| key_input.key(event))
}

/// The second window: the same source, nothing else.
fn second_window(source: LiveImageSource) -> WindowConfig {
    WindowConfig::new()
        .title("LiveImage demo — second window")
        .size(420, 720)
        .root(move |tree, _state| {
            tree.add(
                LiveImage::new(source)
                    .sizing(LiveImageSizing::Aspect)
                    .placeholder(lit!("Waiting for the guest"))
                    .alt(lit!("Guest screen, second window")),
            )
        })
}

fn main() {
    let source = LiveImageSource::builder(LivePixelFormat::Bgrx8)
        .label("guest-screen")
        .build();
    let controls = Arc::new(Controls::new());
    {
        let writer = source.writer();
        let controls = controls.clone();
        std::thread::Builder::new()
            .name("guest".into())
            .spawn(move || produce(writer, controls))
            .expect("spawn the guest's producer");
    }

    TeksiloAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(teksilo::presets::intui::dark())
        .initial_window(
            WindowConfig::new()
                .title("LiveImage demo")
                .size(900, 820)
                .root(move |tree, _state| {
                    let input = GuestInput::new(controls.clone());
                    let paused = Signal::new(false);
                    let nearest = Signal::new(false);
                    let whole_frames = Signal::new(false);
                    let still = Signal::new(false);
                    let (whole_controls, still_controls) = (controls.clone(), controls.clone());
                    let page = nearest.map(|&n| usize::from(n));
                    let pause_controls = controls.clone();
                    let rotate_controls = controls.clone();
                    let second = source.clone();
                    tree.add(
                        VStack::new()
                            .spacing(8.0)
                            .child(
                                HStack::new()
                                    .spacing(12.0)
                                    .child(
                                        Checkbox::new(paused)
                                            .label(lit!("Pause producer"))
                                            .on_change(move |on, _ctx| {
                                                pause_controls.set_paused(on)
                                            }),
                                    )
                                    .child(Button::new(lit!("Rotate")).on_activate_fn(
                                        move |_ctx| {
                                            rotate_controls.rotate.store(true, Ordering::Release);
                                        },
                                    ))
                                    .child(Checkbox::new(nearest).label(lit!("Nearest")))
                                    .child(
                                        Checkbox::new(whole_frames)
                                            .label(lit!("Whole frames"))
                                            .on_change(move |on, _ctx| {
                                                whole_controls
                                                    .whole_frames
                                                    .store(on, Ordering::Relaxed);
                                            }),
                                    )
                                    .child(
                                        Checkbox::new(still).label(lit!("Still guest")).on_change(
                                            move |on, _ctx| {
                                                still_controls.still.store(on, Ordering::Relaxed);
                                            },
                                        ),
                                    )
                                    .child(Button::new(lit!("Second window")).on_activate_fn(
                                        move |ctx| {
                                            ctx.open_window(second_window(second.clone()));
                                        },
                                    )),
                            )
                            .child(
                                Expand::new().child(
                                    Switcher::new(page)
                                        .child(guest_screen(&source, &input, ScalingFilter::Linear))
                                        .child(guest_screen(
                                            &source,
                                            &input,
                                            ScalingFilter::Nearest,
                                        )),
                                ),
                            )
                            .child(TextWidget::new(lit!("")).text(input.log.clone())),
                    )
                }),
        )
        .run();
}
