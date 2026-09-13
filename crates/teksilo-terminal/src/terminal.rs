// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The `Terminal` widget: the accessible, cross-platform Console view. Owns
//! rendering, input encoding, selection, accessibility and lifecycle; the PTY +
//! VT emulation live behind [`crate::engine::TerminalEngine`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use teksilo_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, Modifiers, PointerButton, WidgetEvent};
use teksilo_core::gesture::TapEvent;
use teksilo_core::ime::ImeContext;
use teksilo_core::pointer::touch_action::{PanAxes, PanClaim};
use teksilo_core::pointer::{ScrollPhase, ScrollSource};
use teksilo_core::signal::Signal;
use teksilo_core::styles::Theme;
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_platform::ClipboardHandle;
use teksilo_tokens::{PointerKind, TextStyle};

use crate::a11y::{self, LiveAnnouncer};
use crate::color_scheme::ColorScheme;
use crate::engine::{
    CellSide, GridSnapshot, Scroll, SelectionKind, TermCursorShape, TermEvent, TerminalCommand,
    TerminalEngineFactory, TerminalExit,
};
use crate::input::{self, InputConfig};
use crate::menu::{self, TerminalMenuCommand};
use crate::mouse::{self, MouseButton, MouseKind, TouchReporting};
use crate::render::{self, CellMetrics, RenderParams};
use crate::state::{
    self, DragState, DrainResult, TerminalState, blank_snapshot, compute_layout, drain_and_advance,
};
use crate::style::{RecipeTerminalStyle, TerminalChrome, TerminalStyle};
use crate::touch::{self, MagnifierPainter, TerminalTouch};

/// The preferred cursor shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorStyle {
    Block,
    Beam,
    Underline,
}

/// How the bell (`^G`) is presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BellStyle {
    /// Briefly flash the terminal.
    Visual,
    /// No built-in feedback (use the `on_bell` callback instead).
    None,
}

/// What happens to the child process when the widget is destroyed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalClosePolicy {
    /// Kill the child (`SIGKILL`) when the widget is dropped — the default; a
    /// terminal shouldn't outlive its view.
    KillOnDrop,
    /// Don't kill the child on drop. It receives `SIGHUP` when its PTY closes
    /// as the engine drops, so it exits unless it ignores the hangup (e.g.
    /// `nohup`, a detached `tmux`/`screen`, or `trap '' HUP`).
    LeaveRunning,
}

/// The bundle of reactive signals the terminal publishes. Cloning shares the
/// underlying signals (they are `Rc`-backed), so the widget, its state and the
/// [`TerminalController`] all observe the same values.
#[derive(Clone)]
pub(crate) struct TerminalSignals {
    pub(crate) document_version: Signal<u64>,
    pub(crate) title: Signal<String>,
    pub(crate) cwd: Signal<String>,
    pub(crate) child_running: Signal<bool>,
    pub(crate) has_selection: Signal<bool>,
    pub(crate) alt_screen: Signal<bool>,
    pub(crate) columns: Signal<usize>,
    pub(crate) rows: Signal<usize>,
    pub(crate) last_output_line: Signal<String>,
    pub(crate) exit: Signal<Option<TerminalExit>>,
}

impl TerminalSignals {
    fn new() -> Self {
        Self {
            document_version: Signal::new(0),
            title: Signal::new(String::new()),
            cwd: Signal::new(String::new()),
            child_running: Signal::new(false),
            has_selection: Signal::new(false),
            alt_screen: Signal::new(false),
            columns: Signal::new(80),
            rows: Signal::new(24),
            last_output_line: Signal::new(String::new()),
            exit: Signal::new(None),
        }
    }
}

type TitleCallback = Box<dyn Fn(&str)>;
type UnitCallback = Box<dyn Fn()>;
type ExitCallback = Box<dyn Fn(TerminalExit)>;

#[derive(Default)]
struct Callbacks {
    on_title: Option<TitleCallback>,
    on_bell: Option<UnitCallback>,
    on_cwd: Option<TitleCallback>,
    on_child_exited: Option<ExitCallback>,
}

/// A cloneable handle to a live terminal, for driving it from app code outside
/// `build()` (write to the child, control scrollback/selection, observe state).
/// Holds a `Weak` reference, so keeping a controller does **not** keep the child
/// process alive after the widget is gone.
#[derive(Clone)]
pub struct TerminalController {
    state: Weak<RefCell<TerminalState>>,
    signals: TerminalSignals,
}

impl TerminalController {
    fn with_state<R>(&self, f: impl FnOnce(&mut TerminalState) -> R) -> Option<R> {
        self.state.upgrade().map(|s| f(&mut s.borrow_mut()))
    }

    /// Write raw bytes to the child's input.
    pub fn write(&self, bytes: &[u8]) {
        self.with_state(|st| {
            if let Some(engine) = st.engine.as_mut() {
                engine.write(bytes);
            }
        });
    }

    /// Write UTF-8 text to the child's input.
    pub fn feed_text(&self, text: &str) {
        self.write(text.as_bytes());
    }

    /// Paste text (wrapped in bracketed-paste markers if the child enabled it).
    pub fn paste(&self, text: &str) {
        self.with_state(|st| {
            let mode = st.engine.as_ref().map(|e| e.mode()).unwrap_or_default();
            let bytes = input::encode_paste(text, mode);
            if let Some(engine) = st.engine.as_mut() {
                engine.write(&bytes);
            }
        });
    }

    /// Clear the visible screen (scrollback is retained).
    pub fn clear(&self) {
        self.mutate(|st| {
            if let Some(engine) = st.engine.as_mut() {
                engine.clear_screen();
            }
        });
    }

    /// Full reset (screen + scrollback + modes).
    pub fn reset(&self) {
        self.mutate(|st| {
            if let Some(engine) = st.engine.as_mut() {
                engine.reset();
            }
        });
    }

    /// Scroll back to the live prompt.
    pub fn scroll_to_bottom(&self) {
        self.mutate(|st| {
            if let Some(engine) = st.engine.as_mut() {
                engine.scroll(Scroll::Bottom);
            }
        });
    }

    /// Scroll by a number of lines (positive = toward older output).
    pub fn scroll_lines(&self, delta: i32) {
        self.mutate(|st| {
            if let Some(engine) = st.engine.as_mut() {
                engine.scroll(Scroll::Delta(delta));
            }
        });
    }

    /// Select the entire buffer.
    pub fn select_all(&self) {
        self.mutate(|st| {
            if let Some(engine) = st.engine.as_mut() {
                engine.select_all();
            }
        });
        self.signals
            .has_selection
            .set(self.selection_text().is_some());
    }

    /// Clear any selection.
    pub fn clear_selection(&self) {
        self.mutate(|st| {
            if let Some(engine) = st.engine.as_mut() {
                engine.selection_clear();
            }
        });
        self.signals.has_selection.set(false);
    }

    /// The currently-selected text, if any.
    pub fn selection_text(&self) -> Option<String> {
        self.state
            .upgrade()
            .and_then(|s| s.borrow().engine.as_ref().and_then(|e| e.selection_text()))
    }

    /// A mutation that produces no child echo — refresh the snapshot and wake
    /// the UI loop so it repaints.
    fn mutate(&self, f: impl FnOnce(&mut TerminalState)) {
        self.with_state(|st| {
            f(st);
            st.refresh_snapshot();
            st.wake();
        });
    }

    /// The window/tab title reported by the child (OSC 0/2).
    pub fn title_signal(&self) -> Signal<String> {
        self.signals.title.clone()
    }
    /// The current working directory reported by the child (OSC 7).
    pub fn cwd_signal(&self) -> Signal<String> {
        self.signals.cwd.clone()
    }
    /// Whether the child process is still running.
    pub fn child_running_signal(&self) -> Signal<bool> {
        self.signals.child_running.clone()
    }
    /// Whether there is an active text selection.
    pub fn has_selection_signal(&self) -> Signal<bool> {
        self.signals.has_selection.clone()
    }
    /// Whether the alternate screen buffer is active (a full-screen app).
    pub fn is_alt_screen_signal(&self) -> Signal<bool> {
        self.signals.alt_screen.clone()
    }
    /// The terminal's column count.
    pub fn columns_signal(&self) -> Signal<usize> {
        self.signals.columns.clone()
    }
    /// The terminal's row count.
    pub fn rows_signal(&self) -> Signal<usize> {
        self.signals.rows.clone()
    }
    /// The child's exit result, once it has exited.
    pub fn exit_signal(&self) -> Signal<Option<TerminalExit>> {
        self.signals.exit.clone()
    }
}

/// The terminal-emulator widget. See the crate docs for the design.
pub struct Terminal {
    state: Rc<RefCell<TerminalState>>,
    signals: TerminalSignals,
    callbacks: Callbacks,
    style: Rc<dyn TerminalStyle>,
    factory: Option<Box<dyn TerminalEngineFactory>>,
    command: TerminalCommand,
    scrollback: usize,
    close_policy: TerminalClosePolicy,
    font: Option<TextStyle>,
    bell: BellStyle,
    follow_text_scale: bool,
    label: String,
    mount_queued: Cell<bool>,
    announcer_id: Option<WidgetId>,
    touch: Rc<TerminalTouch>,
}

impl std::fmt::Debug for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terminal")
            .field("scrollback", &self.scrollback)
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

impl Terminal {
    /// A terminal backed by a custom [`TerminalEngineFactory`].
    pub fn with_engine_factory(factory: impl TerminalEngineFactory) -> Self {
        Self {
            state: Rc::new(RefCell::new(TerminalState::new(ColorScheme::default()))),
            signals: TerminalSignals::new(),
            callbacks: Callbacks::default(),
            style: Rc::new(RecipeTerminalStyle),
            factory: Some(Box::new(factory)),
            command: TerminalCommand::shell(),
            scrollback: 10_000,
            close_policy: TerminalClosePolicy::KillOnDrop,
            font: None,
            bell: BellStyle::Visual,
            follow_text_scale: false,
            label: "Terminal".to_string(),
            mount_queued: Cell::new(false),
            announcer_id: None,
            touch: TerminalTouch::new(),
        }
    }

    /// A terminal running the user's default shell, using the default
    /// (`portable-pty` + `alacritty_terminal`) engine.
    #[cfg(feature = "alacritty")]
    pub fn new() -> Self {
        Self::with_engine_factory(crate::AlacrittyEngineFactory)
    }

    /// A terminal running a specific command, using the default engine.
    #[cfg(feature = "alacritty")]
    pub fn with_command(command: TerminalCommand) -> Self {
        Self::new().command(command)
    }

    /// A cloneable handle for driving this terminal from elsewhere.
    pub fn controller(&self) -> TerminalController {
        TerminalController {
            state: Rc::downgrade(&self.state),
            signals: self.signals.clone(),
        }
    }

    // --- Builder: process ---

    /// The command to run (program, args, env, cwd). Overrides the default shell.
    pub fn command(mut self, command: TerminalCommand) -> Self {
        self.command = command;
        self
    }
    /// Run `program` with `args` instead of the default shell.
    pub fn shell(
        mut self,
        program: impl Into<String>,
        args: impl IntoIterator<Item = String>,
    ) -> Self {
        self.command.program = Some(program.into());
        self.command.args = args.into_iter().collect();
        self
    }
    /// Set the child's initial working directory.
    pub fn working_directory(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.command.cwd = Some(dir.into());
        self
    }
    /// Add an environment variable for the child.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.command.env.push((key.into(), value.into()));
        self
    }
    /// What to do with the child when the widget is destroyed.
    pub fn on_close(mut self, policy: TerminalClosePolicy) -> Self {
        self.close_policy = policy;
        self
    }

    // --- Builder: appearance ---

    /// The monospace font. Defaults to the theme's `typography.mono`.
    pub fn font(mut self, font: TextStyle) -> Self {
        self.font = Some(font);
        self
    }
    /// The colour scheme (16 ANSI slots + defaults). Defaults to a dark scheme.
    pub fn color_scheme(self, scheme: ColorScheme) -> Self {
        self.state.borrow_mut().scheme = scheme;
        self
    }
    /// The preferred cursor shape.
    pub fn cursor_shape(self, shape: CursorStyle) -> Self {
        self.state.borrow_mut().cursor_style_pref = shape;
        self
    }
    /// Whether the cursor blinks while focused.
    pub fn cursor_blink(self, blink: bool) -> Self {
        self.state.borrow_mut().cursor_blink = blink;
        self
    }
    /// Whether the terminal font follows the global text-scale accessibility
    /// setting (off by default — terminal font sizes are usually WYSIWYG).
    pub fn follow_text_scale(mut self, follow: bool) -> Self {
        self.follow_text_scale = follow;
        self
    }
    /// A Tier-3 chrome style override.
    pub fn style(mut self, style: impl TerminalStyle) -> Self {
        self.style = Rc::new(style);
        self
    }

    // --- Builder: behaviour ---

    /// The scrollback capacity, in lines.
    pub fn scrollback_lines(mut self, lines: usize) -> Self {
        self.scrollback = lines;
        self
    }
    /// Whether new output snaps the view back to the live prompt.
    pub fn scroll_on_output(self, enable: bool) -> Self {
        self.state.borrow_mut().scroll_on_output = enable;
        self
    }
    /// How the bell is presented.
    pub fn bell(mut self, bell: BellStyle) -> Self {
        self.bell = bell;
        self
    }
    /// Read-only mode: the view renders and scrolls but sends no input.
    pub fn read_only(self, read_only: bool) -> Self {
        self.state.borrow_mut().read_only = read_only;
        self
    }
    /// Whether mouse events are reported to full-screen apps that request them.
    pub fn mouse_reporting(self, enable: bool) -> Self {
        self.state.borrow_mut().mouse_reporting = enable;
        self
    }

    /// What a **direct** pointer (a finger, a pen tip) is reported to the child
    /// program as while it has mouse tracking on. Default
    /// [`TouchReporting::Off`] — a finger is never reported.
    ///
    /// Turning it to [`TouchReporting::AsButton1`] hands a single contact to the
    /// child as mouse button 1 and, with it, the only gesture the view had for
    /// selecting and for opening its menu. Two contacts are never reported, so a
    /// two-finger pan still reaches the scrollback; that is the way back.
    /// See [`TouchReporting`] for the bytes.
    pub fn touch_mouse_reporting(self, reporting: TouchReporting) -> Self {
        self.state.borrow_mut().touch_reporting = reporting;
        self
    }

    /// Whether `Alt+<key>` is sent as an ESC prefix ("Option as Meta").
    pub fn alt_sends_escape(self, enable: bool) -> Self {
        self.state.borrow_mut().alt_sends_escape = enable;
        self
    }
    /// The accessible name for the terminal.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    // --- Builder: events ---

    /// Called when the child sets the window/tab title (OSC 0/2).
    pub fn on_title_changed(mut self, f: impl Fn(&str) + 'static) -> Self {
        self.callbacks.on_title = Some(Box::new(f));
        self
    }
    /// Called when the bell rings.
    pub fn on_bell(mut self, f: impl Fn() + 'static) -> Self {
        self.callbacks.on_bell = Some(Box::new(f));
        self
    }
    /// Called when the working directory changes (OSC 7).
    pub fn on_cwd_changed(mut self, f: impl Fn(&str) + 'static) -> Self {
        self.callbacks.on_cwd = Some(Box::new(f));
        self
    }
    /// Called when the child process exits.
    pub fn on_child_exited(mut self, f: impl Fn(TerminalExit) + 'static) -> Self {
        self.callbacks.on_child_exited = Some(Box::new(f));
        self
    }

    // --- Internal helpers ---

    fn resolve_font(&self, theme: &Theme, text_scale: f32) -> TextStyle {
        match &self.font {
            Some(font) => {
                if self.follow_text_scale {
                    TextStyle {
                        size: font.size * text_scale,
                        ..font.clone()
                    }
                } else {
                    font.clone()
                }
            }
            None => theme.typography.mono.clone(),
        }
    }

    /// The closure that fills the magnifier lens: **only** the grid layer,
    /// re-emitted in window coordinates.
    ///
    /// It is re-entered during the same frame inside the lens's transform-and-
    /// clip scope, so it deliberately does none of what
    /// [`Terminal::paint`](Widget::paint) does around the same call: no chrome,
    /// no drain (a second drain in one frame would feed the engine bytes the
    /// first one already consumed), no clip of its own (`replay` installs the
    /// lens clip and a `clear_clip` here would destroy it) and no visual bell.
    /// What is left — repainting an already-computed snapshot — produces the same
    /// display list twice, which is what the contract asks for.
    fn magnifier_painter(&self) -> MagnifierPainter {
        let state = self.state.clone();
        let font = self.font.clone();
        let follow_text_scale = self.follow_text_scale;
        Rc::new(move |canvas, ctx: &PaintContext<'_>| {
            let st = state.borrow();
            let base = match &font {
                Some(font) if follow_text_scale => TextStyle {
                    size: font.size * ctx.text_scale,
                    ..font.clone()
                },
                Some(font) => font.clone(),
                None => ctx.theme.typography.mono.clone(),
            };
            render::paint_grid(
                canvas,
                &RenderParams {
                    snapshot: &st.snapshot,
                    scheme: &st.scheme,
                    metrics: st.metrics,
                    origin: st.origin,
                    base_font: &base,
                    focused: st.focused && ctx.window_active,
                    // The lens is a still: a caret caught mid-blink would blink
                    // inside it on a rhythm of its own.
                    cursor_on: true,
                    cursor_shape: effective_cursor_shape(
                        st.cursor_style_pref,
                        st.snapshot.cursor.shape,
                    ),
                },
            );
        })
    }

    /// React to a completed drain: fire callbacks + update reactive signals.
    /// Runs with no outstanding `state` borrow (touches only `self`).
    ///
    /// This is called from `paint()` (the reliable drain point on an off-thread
    /// repaint), so the user callbacks (`on_title` / `on_bell` / `on_cwd` /
    /// `on_child_exited`) fire during the render pass. They are deliberately
    /// plain `Fn` (no `EventContext`) and the events that trigger them are
    /// infrequent (title/bell/cwd/exit, not per-frame), so a callback can only
    /// touch captured signals/handles — keep them lightweight.
    fn apply_drain(&self, drain: DrainResult) {
        for event in drain.events {
            match event {
                TermEvent::Title(title) => {
                    self.signals.title.set(title.clone());
                    if let Some(cb) = &self.callbacks.on_title {
                        cb(&title);
                    }
                }
                TermEvent::ResetTitle => {
                    self.signals.title.set(self.label.clone());
                    if let Some(cb) = &self.callbacks.on_title {
                        cb(&self.label);
                    }
                }
                TermEvent::Bell => {
                    if let Some(cb) = &self.callbacks.on_bell {
                        cb();
                    }
                    if self.bell == BellStyle::Visual {
                        let mut st = self.state.borrow_mut();
                        st.bell_flash = Some(Instant::now());
                        if let Some(fr) = &st.frame_request {
                            fr.set(true);
                        }
                    }
                }
                TermEvent::CwdChanged(uri) => {
                    self.signals.cwd.set(uri.clone());
                    if let Some(cb) = &self.callbacks.on_cwd {
                        cb(&uri);
                    }
                }
                TermEvent::ClipboardStore(_) | TermEvent::CursorBlinkChanged(_) => {
                    // OSC 52 write is denied by default (see the engine); the
                    // cursor blink preference is view-driven.
                }
                TermEvent::Exited(exit) => self.report_exit(exit),
            }
        }

        if drain.content_changed {
            // Bumping this (bound at `AccessibilityOnly`) re-walks the a11y row
            // tree so a screen reader sees fresh content. It runs at most once
            // per repaint, and repaints are coalesced (see `post_repaint`), so
            // under a flood of output the a11y rebuild is bounded to the frame
            // rate rather than per-read-chunk. (Gating it on an active AT client
            // would avoid the work entirely when no screen reader is attached —
            // a future optimisation once the framework surfaces that state.)
            let v = self.signals.document_version.get();
            self.signals.document_version.set(v.wrapping_add(1));

            let (has_selection, alt_screen, announcement) = {
                let mut st = self.state.borrow_mut();
                let has_selection = st
                    .engine
                    .as_ref()
                    .and_then(|e| e.selection_text())
                    .is_some();
                let alt_screen = st
                    .engine
                    .as_ref()
                    .map(|e| e.mode().alt_screen)
                    .unwrap_or(false);
                let cursor_line = st.snapshot.cursor.line;
                let announcement = if cursor_line != st.prev_cursor_line {
                    let line = row_text(&st.snapshot, st.prev_cursor_line);
                    st.prev_cursor_line = cursor_line;
                    (!line.trim().is_empty()).then_some(line)
                } else {
                    None
                };
                (has_selection, alt_screen, announcement)
            };
            self.signals.has_selection.set(has_selection);
            self.signals.alt_screen.set(alt_screen);
            if let Some(line) = announcement {
                self.signals.last_output_line.set(line);
            }
        }

        if drain.eof {
            let exit = {
                let mut st = self.state.borrow_mut();
                st.engine.as_mut().and_then(|e| e.poll_exit())
            }
            .unwrap_or(TerminalExit {
                success: true,
                code: None,
            });
            self.report_exit(exit);
        }
    }

    fn report_exit(&self, exit: TerminalExit) {
        let already = {
            let mut st = self.state.borrow_mut();
            let already = st.exit_reported;
            st.exit_reported = true;
            already
        };
        if already {
            return;
        }
        self.signals.child_running.set(false);
        self.signals.exit.set(Some(exit));
        if let Some(cb) = &self.callbacks.on_child_exited {
            cb(exit);
        }
    }
}

#[cfg(feature = "alacritty")]
impl Default for Terminal {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Terminal {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();

        // New output re-walks the AT tree (no rebuild) via this binding.
        self.signals.document_version.bind_to(
            self_id,
            ctx.binding_registry(),
            BindingLevel::AccessibilityOnly,
        );

        // Store the frame-request handle so the bell / blink can schedule frames.
        self.state.borrow_mut().frame_request = Some(ctx.frame_request_handle());

        // The live-region announcer child.
        let announcer = ctx.add(LiveAnnouncer {
            text: self.signals.last_output_line.clone(),
        });
        self.announcer_id = Some(announcer);

        // Touch selection: the controller, and the overlay content that paints
        // its handles. Built here because `prefers_reduced_motion` has no
        // accessor on `EventContext` — the preference has to be read while a
        // `BuildContext` is in hand.
        let theme = ctx.theme_signal().get();
        let (affordances, handle_recipe, lens_recipe) =
            touch::configure_controller(&self.touch, &theme, ctx.prefers_reduced_motion());
        let layer = ctx.add_detached(touch::AffordanceHost::new(
            affordances,
            handle_recipe,
            lens_recipe,
            touch::delegate(&self.touch, &self.state),
            Some(self.magnifier_painter()),
            Rc::downgrade(&self.touch),
        ));
        ctx.set_dormant(layer);
        touch::attach(&self.touch, self_id, layer);

        // Handlers.
        let read_only = self.state.borrow().read_only;
        let mut handlers = HandlerSet::new()
            .focusable(true)
            .keyboard_capture(true)
            .cursor(CursorIcon::Text)
            // A finger pans the scrollback vertically, with a kinetic hand-off
            // on release. Direct pointers only, which is `PanClaim`'s default:
            // a wheel keeps the route it has always had (`ScrollDelivery`
            // sends only a `TouchPan` along the claimant chain).
            .pan_claim(PanClaim {
                axes: PanAxes::Y,
                kinetic: true,
                ..PanClaim::default()
            });
        if !read_only {
            handlers = handlers.ime_input(ImeContext::text());
        }

        let st = self.state.clone();
        let tch = self.touch.clone();
        handlers = handlers.on_focus(move |gained, _ctx| {
            {
                let mut st = st.borrow_mut();
                st.focused = gained;
                st.blink_on = true;
                st.blink_last = None;
            }
            if !gained {
                // One of the four retirement paths the host owns; the affordance
                // band takes nothing down by itself.
                tch.dismiss();
            }
        });

        let st = self.state.clone();
        let tch = self.touch.clone();
        handlers = handlers.on_key(move |event, ctx| keyboard_handler(&st, &tch, event, ctx));

        let st = self.state.clone();
        let sig = self.signals.clone();
        let tch = self.touch.clone();
        handlers = handlers
            .on_pointer_event(move |event, ctx| pointer_handler(&st, &sig, &tch, event, ctx));

        let st = self.state.clone();
        let tch = self.touch.clone();
        handlers = handlers.on_scroll(move |event, ctx| scroll_handler(&st, &tch, event, ctx));

        let st = self.state.clone();
        let sig = self.signals.clone();
        let tch = self.touch.clone();
        handlers = handlers.on_double_tap(move |tap, ctx| {
            select_at(&st, &sig, &tch, tap, SelectionKind::Word, ctx);
        });

        let st = self.state.clone();
        let sig = self.signals.clone();
        let tch = self.touch.clone();
        handlers = handlers.on_triple_tap(move |tap, ctx| {
            select_at(&st, &sig, &tch, tap, SelectionKind::Line, ctx);
        });

        // The two `ScrollUp` / `ScrollDown` actions the `Role::Terminal` node
        // has advertised since it was written, and which nothing implemented.
        let st = self.state.clone();
        let tch = self.touch.clone();
        handlers = handlers.on_access_action(move |action, ctx| match action {
            accesskit::Action::ScrollUp => {
                access_scroll(&st, &tch, Scroll::PageUp, ctx);
                EventResponse::Handled
            }
            accesskit::Action::ScrollDown => {
                access_scroll(&st, &tch, Scroll::PageDown, ctx);
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        });

        // The context menu — Copy / Paste / Select all / Clear. Reached three
        // ways, and only the first is new to touch: the tree-owned long-press
        // route (a hold, which resolves to this factory because the node
        // installs one and no `on_long_press` handler is on the path), a
        // right-click, and an assistive client's `ShowContextMenu`.
        let st = self.state.clone();
        let sig = self.signals.clone();
        let tch = self.touch.clone();
        handlers = handlers.context_menu(move |_position, _ctx| {
            let run_state = st.clone();
            let run_signals = sig.clone();
            let _ = &tch;
            Some(menu::build_menu(
                &st,
                Rc::new(move |command, ctx| {
                    run_command(&run_state, &run_signals, command, ctx);
                }),
            ))
        });

        ctx.apply_self_handlers(handlers);

        // Cursor-blink / visual-bell frame effect.
        let st = self.state.clone();
        let tick = ctx.frame_tick();
        ctx.effect(&tick, move |_delta| tick_frame(&st));

        // Mirror window-active state (drives caret hiding / desaturation).
        let st = self.state.clone();
        let tch = self.touch.clone();
        let wa = ctx.window_active_signal();
        ctx.effect(&wa, move |active| {
            st.borrow_mut().window_active = *active;
            if !*active {
                tch.dismiss();
            }
        });

        // Spawn the engine + reader thread after mount (first build only).
        if !self.mount_queued.get() {
            self.mount_queued.set(true);
            let state = self.state.clone();
            let signals = self.signals.clone();
            let factory = self.factory.take();
            let command = self.command.clone();
            let scrollback = self.scrollback;
            ctx.run_after_mount(move |ectx| {
                spawn_engine(&state, &signals, factory, &command, scrollback, ectx);
            });
        }

        vec![announcer]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let font = self.resolve_font(ctx.theme, ctx.text_scale);
        let metrics = measure_cell(ctx, &font);
        self.state.borrow_mut().metrics = metrics;

        let inset = self.style.content_inset();
        let default_w = 80.0 * metrics.width + inset * 2.0;
        let default_h = 24.0 * metrics.height + inset * 2.0;
        let size = proposal.resolve(default_w, default_h);
        // A terminal fills the space it's given (grows to claim slack).
        LayoutResponse::flexible(size, 1.0)
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        ctx: &LayoutContext,
    ) {
        let inset = self.style.content_inset();
        let dims_changed;
        let (cols, rows, origin);
        {
            let metrics = self.state.borrow().metrics;
            let (c, r, geom, o) = compute_layout(bounds, metrics, inset);
            cols = c;
            rows = r;
            origin = o;
            let mut st = self.state.borrow_mut();
            st.origin = origin;
            st.bounds = bounds;
            dims_changed = (cols, rows) != (st.cols, st.rows);
            if dims_changed {
                st.cols = cols;
                st.rows = rows;
                st.geom = geom;
                if let Some(engine) = st.engine.as_mut() {
                    engine.resize(geom);
                    st.snapshot = engine.snapshot();
                } else {
                    st.snapshot = blank_snapshot(cols, rows);
                }
            }
        }
        if dims_changed {
            self.signals.columns.set(cols);
            self.signals.rows.set(rows);
        }
        // The one place the published affordance geometry is re-derived from the
        // widget's own: it runs after `origin` and `bounds` are settled and
        // *before* the layer's children are placed, so a handle never lags the
        // grid it marks by a frame. A no-op until a finger has raised something
        // (`TouchSelection::refresh` returns early), and `TextAffordances::publish`
        // drops an unchanged state, so a steady terminal pays one comparison.
        {
            let mut st = self.state.borrow_mut();
            self.touch.refresh(&mut st, ctx.layout_direction);
        }
        // The announcer is a zero-size child.
        for placement in children.iter_mut() {
            placement.origin = origin;
            placement.size = Size::ZERO;
        }
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        // Chrome (background + focus ring).
        let (focused, scheme) = {
            let st = self.state.borrow();
            (st.focused, st.scheme.clone())
        };
        let chrome = TerminalChrome {
            focused,
            window_active: ctx.window_active,
        };
        self.style
            .paint_frame(canvas, bounds, ctx.theme, &scheme, &chrome);

        // Drain the child's pending output and advance the engine.
        let drain = {
            let mut st = self.state.borrow_mut();
            st.window_active = ctx.window_active;
            drain_and_advance(&mut st)
        };
        let content_changed = drain.content_changed;
        self.apply_drain(drain);
        if content_changed {
            // New output keeps the engine's selection (it is in buffer
            // coordinates) but re-projects it into the viewport, so the handles
            // move with the text rather than being retired by it. A repaint-only
            // frame runs no layout, which is why this cannot be left to
            // `place_children` alone.
            //
            // **Reviewed, not tested.** `content_changed` is true only when the
            // PTY reader thread has queued bytes, and the `MemoryEngine`'s reader
            // is at end-of-file by construction — a headless tree can reach this
            // branch by no route at all. The `place_children` twin above carries
            // the same call and is pinned by
            // `the_handles_follow_a_selection_change_across_a_layout`.
            let mut st = self.state.borrow_mut();
            self.touch.refresh(&mut st, ctx.layout_direction);
        }

        // Render the grid. Hold the state borrow across paint_grid (it only
        // reads the snapshot) rather than cloning the whole grid every frame.
        let font = self.resolve_font(ctx.theme, ctx.text_scale);
        let bell_flash = {
            let st = self.state.borrow();
            let active_focus = st.focused && ctx.window_active;
            let cursor_on = if st.cursor_blink && active_focus {
                st.blink_on
            } else {
                true
            };
            let cursor_shape =
                effective_cursor_shape(st.cursor_style_pref, st.snapshot.cursor.shape);
            let content = Rect::new(
                st.origin.x,
                st.origin.y,
                st.snapshot.columns as f32 * st.metrics.width,
                st.snapshot.screen_lines as f32 * st.metrics.height,
            );
            canvas.set_clip(content);
            render::paint_grid(
                canvas,
                &RenderParams {
                    snapshot: &st.snapshot,
                    scheme: &scheme,
                    metrics: st.metrics,
                    origin: st.origin,
                    base_font: &font,
                    focused: st.focused && ctx.window_active,
                    cursor_on,
                    cursor_shape,
                },
            );
            canvas.clear_clip();
            st.bell_flash
        };

        // Visual bell: a brief accent overlay that fades out.
        if let Some(t) = bell_flash {
            let elapsed = t.elapsed();
            if elapsed < Duration::from_millis(150) {
                let alpha = 0.25 * (1.0 - elapsed.as_secs_f32() / 0.15);
                let flash = teksilo_tokens::Color::new(
                    scheme.foreground.r(),
                    scheme.foreground.g(),
                    scheme.foreground.b(),
                    alpha.max(0.0),
                );
                canvas.fill_rect(bounds, flash);
            }
        }
    }

    fn accessibility(&self, builder: &mut teksilo_core::accessibility::AccessNodeBuilder) {
        let st = self.state.borrow();
        // The grid's offset inside the widget: `AccessNodeBuilder::build`
        // translates the emitted rects into window space itself, once the walker
        // has written the terminal's own box.
        let origin = Point::new(st.origin.x - st.bounds.x, st.origin.y - st.bounds.y);
        a11y::build_terminal_a11y(builder, &st.snapshot, st.metrics, origin, &self.label);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.announcer_id.into_iter().collect()
    }

    fn clips_children(&self) -> bool {
        true
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Signal the reader thread to stop, and (per policy) kill the child so
        // it can't outlive the view even if a controller keeps the state alive.
        let st = self.state.borrow();
        st.reader_stop
            .store(true, std::sync::atomic::Ordering::Relaxed);
        drop(st);
        if self.close_policy == TerminalClosePolicy::KillOnDrop
            && let Some(engine) = self.state.borrow_mut().engine.as_mut()
        {
            engine.kill();
        }
    }
}

// --- Free functions: spawning, measuring, handlers ---

fn spawn_engine(
    state: &Rc<RefCell<TerminalState>>,
    signals: &TerminalSignals,
    factory: Option<Box<dyn TerminalEngineFactory>>,
    command: &TerminalCommand,
    scrollback: usize,
    ectx: &mut EventContext,
) {
    let Some(factory) = factory else {
        return;
    };
    // The widget may have been dropped between build() queuing this mount action
    // and the action running (a same-tick mount+unmount). `Terminal::Drop` sets
    // `reader_stop`, so don't spawn a child that nobody is left to kill.
    if state
        .borrow()
        .reader_stop
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        return;
    }
    if state.borrow().engine.is_some() {
        return;
    }
    let poster = ectx.poster().cloned();
    let window_id = ectx.window().map(|w| w.id());
    let geom = state.borrow().geom;

    match factory.spawn(command, geom, scrollback) {
        Ok(spawned) => {
            let (reader_shared, stop, pending) = {
                let mut st = state.borrow_mut();
                st.engine = Some(spawned.engine);
                st.poster = poster.clone();
                st.window_id = window_id;
                st.refresh_snapshot();
                (
                    st.reader.clone(),
                    st.reader_stop.clone(),
                    st.repaint_pending.clone(),
                )
            };
            signals.child_running.set(true);
            // The reader thread needs both a poster and the target window id to
            // route its repaint requests; a windowless (headless) tree has
            // neither, so the engine is still usable via the controller.
            if let (Some(poster), Some(window_id)) = (poster, window_id) {
                state::spawn_reader_thread(
                    spawned.reader,
                    reader_shared,
                    stop,
                    pending,
                    poster,
                    window_id,
                );
            }
        }
        Err(err) => {
            // The child never started. Surface it: apps observing `exit_signal()`
            // see the failure, and the reason (which a signal can't carry) is
            // logged to stderr.
            let prog = command.program.as_deref().unwrap_or("<default shell>");
            eprintln!("teksilo-terminal: failed to spawn `{prog}`: {err}");
            signals.child_running.set(false);
            signals.exit.set(Some(TerminalExit {
                success: false,
                code: None,
            }));
        }
    }
}

fn measure_cell(ctx: &LayoutContext, font: &TextStyle) -> CellMetrics {
    if let Some(backend) = ctx.text_backend {
        let mut backend = backend.borrow_mut();
        let wide = backend.layout_single_line("M", font, None);
        // A monospace font advances 'i' the same as 'M'; if it doesn't, the
        // caller gave us a proportional font and the grid will misalign. Warn
        // once (the cell size still tracks 'M' as a best effort).
        let narrow = backend.layout_single_line("i", font, None);
        if (wide.width - narrow.width).abs() > 1.0 {
            warn_non_monospace(&font.family);
        }
        CellMetrics {
            width: wide.width.max(1.0),
            height: wide.height.max(1.0),
        }
    } else {
        CellMetrics {
            width: (font.size * 0.6).max(1.0),
            height: (font.size * font.line_height).max(1.0),
        }
    }
}

/// Warn (once per process) that a non-monospace font was configured for a
/// terminal — the grid can't align proportional glyphs.
fn warn_non_monospace(family: &str) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        eprintln!(
            "teksilo-terminal: font family `{family}` is not monospace; the grid \
             will misalign. Use a monospace font (e.g. the theme's `typography.mono`)."
        );
    }
}

fn effective_cursor_shape(pref: CursorStyle, reported: TermCursorShape) -> TermCursorShape {
    match reported {
        TermCursorShape::Hidden => TermCursorShape::Hidden,
        _ => match pref {
            CursorStyle::Block => TermCursorShape::Block,
            CursorStyle::Beam => TermCursorShape::Beam,
            CursorStyle::Underline => TermCursorShape::Underline,
        },
    }
}

/// Blink toggle + visual-bell re-arm, run each frame the loop ticks.
fn tick_frame(state: &Rc<RefCell<TerminalState>>) {
    const BLINK_INTERVAL: Duration = Duration::from_millis(500);
    let mut st = state.borrow_mut();
    let active = st.focused && st.window_active && st.cursor_blink;
    let mut want_more = false;

    if active {
        let now = Instant::now();
        let toggle = match st.blink_last {
            Some(last) => now.duration_since(last) >= BLINK_INTERVAL,
            None => true,
        };
        if toggle {
            st.blink_on = !st.blink_on;
            st.blink_last = Some(now);
        }
        want_more = true;
    } else if !st.blink_on {
        st.blink_on = true;
    }

    // Keep the visual bell animating until it fades.
    if let Some(t) = st.bell_flash {
        if t.elapsed() < Duration::from_millis(160) {
            want_more = true;
        } else {
            st.bell_flash = None;
        }
    }

    if want_more && let Some(fr) = &st.frame_request {
        fr.set(true);
    }
}

fn keyboard_handler(
    state: &Rc<RefCell<TerminalState>>,
    touch: &Rc<TerminalTouch>,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    match event {
        WidgetEvent::KeyDown {
            key,
            modifiers,
            text,
        } => {
            // Copy / paste chords (platform-aware) are handled by the view, not
            // forwarded to the child.
            if is_copy_chord(*key, *modifiers) {
                copy_selection(state, ctx);
                return EventResponse::Handled;
            }
            if is_paste_chord(*key, *modifiers) {
                paste_clipboard(state, ctx);
                return EventResponse::Handled;
            }
            // Shift+PageUp/Down scrolls the scrollback. Either one moves the
            // viewport under the touch affordances, whose offsets are viewport
            // cells, so they are retired rather than left naming other text.
            if *key == Key::PageUp && modifiers.shift() {
                scroll_view(state, Scroll::PageUp, ctx);
                touch.dismiss();
                return EventResponse::Handled;
            }
            if *key == Key::PageDown && modifiers.shift() {
                scroll_view(state, Scroll::PageDown, ctx);
                touch.dismiss();
                return EventResponse::Handled;
            }

            // The keyboard-capture escape chord (WCAG 2.1.2). A terminal
            // encodes Tab as `\t` and Shift+Tab as CSI Z, so plain Tab can
            // never leave — Ctrl+Tab / Ctrl+Shift+Tab are the way out, and
            // must reach the framework's focus cycling as `Ignored` rather
            // than being written to the child. `dispatch_event_impl` already
            // intercepts this chord for every `keyboard_capture` node before
            // the widget is consulted; declining it here too keeps the widget
            // correct in isolation (a direct `keyboard_handler` call, a host
            // that dispatches keys itself) instead of relying on the caller.
            // Literal `ctrl()`: ⌘⇥ is the macOS application switcher and
            // never arrives, so Ctrl+Tab is Ctrl+Tab on every platform.
            if *key == Key::Tab && modifiers.ctrl() {
                return EventResponse::Ignored;
            }

            if state.borrow().read_only {
                // Nothing to forward: a read-only terminal has no child to
                // receive the keystroke, so consuming it would trap focus for
                // no benefit. Decline instead and let the framework's normal
                // key routing (Tab cycling included) proceed.
                return EventResponse::Ignored;
            }

            let (mode, cfg) = {
                let st = state.borrow();
                let mode = st.engine.as_ref().map(|e| e.mode()).unwrap_or_default();
                (
                    mode,
                    InputConfig {
                        alt_sends_escape: st.alt_sends_escape,
                    },
                )
            };
            if let Some(bytes) = input::encode_key(*key, *modifiers, text.as_deref(), mode, cfg) {
                {
                    let mut st = state.borrow_mut();
                    if let Some(engine) = st.engine.as_mut() {
                        // A keystroke returns to the live prompt.
                        engine.scroll(Scroll::Bottom);
                        engine.write(&bytes);
                    }
                }
                // …which moves the viewport, so the affordances go.
                touch.dismiss();
            }
            ctx.request_frame();
            // A keyboard-capture surface consumes every key.
            EventResponse::Handled
        }
        WidgetEvent::ImeCommit { text } => {
            if !state.borrow().read_only {
                let mut st = state.borrow_mut();
                if let Some(engine) = st.engine.as_mut() {
                    engine.scroll(Scroll::Bottom);
                    engine.write(text.as_bytes());
                }
            }
            ctx.request_frame();
            EventResponse::Handled
        }
        WidgetEvent::ImeComposition { .. } => EventResponse::Handled,
        _ => EventResponse::Ignored,
    }
}

/// Whether the child has enabled mouse reporting and the widget allows it.
fn mouse_reporting_active(st: &TerminalState) -> bool {
    st.mouse_reporting
        && st
            .engine
            .as_ref()
            .map(|e| e.mode().mouse_reporting())
            .unwrap_or(false)
}

fn to_mouse_button(button: PointerButton) -> Option<MouseButton> {
    match button {
        PointerButton::Primary => Some(MouseButton::Left),
        PointerButton::Middle => Some(MouseButton::Middle),
        PointerButton::Secondary => Some(MouseButton::Right),
        _ => None,
    }
}

/// Encode a mouse event and write it to the child, if reporting is active.
fn report_mouse(
    st: &mut TerminalState,
    kind: MouseKind,
    button: MouseButton,
    col: usize,
    row: usize,
    modifiers: Modifiers,
) {
    let Some(mode) = st.engine.as_ref().map(|e| e.mode()) else {
        return;
    };
    if let Some(bytes) = mouse::encode_mouse(kind, button, col, row, modifiers, mode)
        && let Some(engine) = st.engine.as_mut()
    {
        engine.write(&bytes);
    }
}

/// Whether a pointer of `kind` is reported to the child right now.
///
/// Two independent gates, and they answer for different devices. The child's
/// own tracking mode plus the widget's `mouse_reporting` opt-out decide whether
/// *anything* is reported; [`TouchReporting`] decides whether a **direct**
/// pointer is one of the things that is. A mouse never consults the second.
///
/// The two-contact clause is the "two-finger pan always local" rule at its
/// source: with a second finger down, neither contact is reported, so the pan
/// the two of them make reaches the scrollback instead of the child.
fn reports_pointer(st: &TerminalState, kind: PointerKind) -> bool {
    if !mouse_reporting_active(st) {
        return false;
    }
    if !kind.is_direct() {
        return true;
    }
    st.touch_reporting == TouchReporting::AsButton1 && st.contacts <= 1
}

/// One direct contact has left the surface — lifted or revoked.
///
/// The pan bookkeeping is cleared on the **last** contact rather than on the
/// owner's own `Ended`, because a revoked gesture never delivers one: a
/// `PointerCancel` closes the pan session with no `Ended` phase, and a
/// `pan_owner` left behind by that would silently absorb every later pan from a
/// different contact.
fn release_contact(st: &mut TerminalState) {
    st.contacts = st.contacts.saturating_sub(1);
    if st.contacts == 0 {
        st.pan_owner = None;
        st.scroll_residue = 0.0;
    }
}

/// The widget-local position an event carries, if it carries one.
fn event_position(event: &WidgetEvent) -> Option<Point> {
    match event {
        WidgetEvent::PointerDown { position, .. }
        | WidgetEvent::PointerUp { position, .. }
        | WidgetEvent::PointerMove { position, .. } => Some(*position),
        _ => None,
    }
}

fn pointer_handler(
    state: &Rc<RefCell<TerminalState>>,
    signals: &TerminalSignals,
    touch: &Rc<TerminalTouch>,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    let kind = ctx.pointer_kind();
    // Recorded before anything decides whether to act on the event, because the
    // consumer is the *wheel*, which arrives through a different handler and
    // with no position of its own. See `TerminalState::last_local_pointer`.
    if let Some(position) = event_position(event) {
        state.borrow_mut().last_local_pointer = Some(position);
    }
    match event {
        WidgetEvent::PointerDown {
            position,
            button,
            modifiers,
            ..
        } => {
            if kind.is_direct() {
                state.borrow_mut().contacts += 1;
            } else {
                // A cursor arriving retires whatever a finger left standing.
                // The affordance band is exempt from outside-press dismissal —
                // every cell a press lands on is "outside" a handle — so this
                // is the rule's only home. Free when nothing is raised.
                touch.dismiss();
            }
            let cell = cell_at_position(state, *position);
            let report = { reports_pointer(&state.borrow(), kind) && !modifiers.shift() };
            if report {
                let button = match kind.is_direct() {
                    // A finger has no buttons; `AsButton1` says which one it
                    // stands in for, and there is no VT encoding that could say
                    // it was a finger.
                    true => Some(MouseButton::Left),
                    false => to_mouse_button(*button),
                };
                if let (Some(mb), Some((col, row, _))) = (button, cell) {
                    let mut st = state.borrow_mut();
                    report_mouse(&mut st, MouseKind::Press, mb, col, row, *modifiers);
                    st.mouse_button_held = Some(mb);
                }
                // The child owns this press outright: no local selection, and
                // no gesture arena either, so nothing can double-tap a word out
                // from under a full-screen program.
                return EventResponse::Handled;
            }
            if kind.is_direct() {
                // A finger starts no selection drag. Its press is still
                // undecided between a pan (the claim below), a tap, and the
                // tree-owned hold that opens the context menu — and a drag
                // begun here would take the contact away from all three.
                // Touch selects by double- or triple-tap and adjusts with the
                // handles.
                return EventResponse::Ignored;
            }
            if *button == PointerButton::Primary {
                if let Some((col, row, side)) = cell {
                    let kind = if modifiers.alt() {
                        SelectionKind::Block
                    } else {
                        SelectionKind::Simple
                    };
                    let mut st = state.borrow_mut();
                    if let Some(engine) = st.engine.as_mut() {
                        engine.selection_start(row, col, side, kind);
                    }
                    st.drag = Some(DragState { moved: false });
                    st.refresh_snapshot();
                }
                ctx.request_frame();
                // **Ignored, not Handled** — and this is a behaviour change.
                // `on_pointer_event` runs *before* the gesture arena and a
                // `Handled` skips it, so answering `Handled` here meant the
                // arena never saw a press: the widget's own `on_double_tap` and
                // `on_triple_tap` could not fire, and the documented word- and
                // line-selection did nothing. The selection anchor is already
                // set above; declining the event only lets the arena run, and
                // the arena answers `Handled` in this widget's stead (it takes
                // the implicit Down..Up capture with it, which is what keeps a
                // selection drag alive past the widget's own edge).
                return EventResponse::Ignored;
            }
            EventResponse::Ignored
        }
        WidgetEvent::PointerMove { position, .. } => {
            let cell = cell_at_position(state, *position);
            let held = state.borrow().mouse_button_held;
            if let Some(mb) = held {
                if let Some((col, row, _)) = cell {
                    let mut st = state.borrow_mut();
                    if reports_pointer(&st, kind) {
                        report_mouse(&mut st, MouseKind::Drag, mb, col, row, Modifiers::NONE);
                    }
                }
                return EventResponse::Handled;
            }
            // Any-motion reporting (mode 1003).
            let motion = {
                let st = state.borrow();
                reports_pointer(&st, kind)
                    && st
                        .engine
                        .as_ref()
                        .map(|e| e.mode().mouse_motion)
                        .unwrap_or(false)
            };
            if motion {
                if let Some((col, row, _)) = cell {
                    let mut st = state.borrow_mut();
                    report_mouse(
                        &mut st,
                        MouseKind::Motion,
                        MouseButton::Left,
                        col,
                        row,
                        Modifiers::NONE,
                    );
                }
                return EventResponse::Handled;
            }
            // Local selection drag.
            if state.borrow().drag.is_some() {
                if let Some((col, row, side)) = cell {
                    let mut st = state.borrow_mut();
                    if let Some(drag) = st.drag.as_mut() {
                        drag.moved = true;
                    }
                    if let Some(engine) = st.engine.as_mut() {
                        engine.selection_update(row, col, side);
                    }
                    st.refresh_snapshot();
                }
                ctx.request_frame();
                // Declined for the same reason the press is: the multi-tap
                // recognizers need the moves to know the contact wandered.
                return EventResponse::Ignored;
            }
            EventResponse::Ignored
        }
        WidgetEvent::PointerUp {
            position,
            button,
            modifiers,
            ..
        } => {
            if kind.is_direct() {
                release_contact(&mut state.borrow_mut());
            }
            let held = state.borrow().mouse_button_held;
            if let Some(mb) = held {
                let (col, row) = cell_at_position(state, *position)
                    .map(|(c, r, _)| (c, r))
                    .unwrap_or((0, 0));
                let mut st = state.borrow_mut();
                if mouse_reporting_active(&st) {
                    report_mouse(&mut st, MouseKind::Release, mb, col, row, *modifiers);
                }
                st.mouse_button_held = None;
                return EventResponse::Handled;
            }
            if *button == PointerButton::Primary {
                let (had_drag, moved) = {
                    let st = state.borrow();
                    (st.drag.is_some(), st.drag.map(|d| d.moved).unwrap_or(false))
                };
                if had_drag {
                    let mut st = state.borrow_mut();
                    st.drag = None;
                    if !moved {
                        if let Some(engine) = st.engine.as_mut() {
                            engine.selection_clear();
                        }
                        st.refresh_snapshot();
                    }
                    let has_sel = st
                        .engine
                        .as_ref()
                        .and_then(|e| e.selection_text())
                        .is_some();
                    drop(st);
                    signals.has_selection.set(has_sel);
                    ctx.request_frame();
                    // Declined so the arena sees the release: a double- or
                    // triple-tap streak is counted on the *ups*.
                    return EventResponse::Ignored;
                }
            }
            EventResponse::Ignored
        }
        WidgetEvent::PointerCancel { .. } => {
            if kind.is_direct() {
                release_contact(&mut state.borrow_mut());
            }
            EventResponse::Ignored
        }
        _ => EventResponse::Ignored,
    }
}

/// The scrollback is a **ring position quantised to whole lines**, not a pixel
/// offset with a maximum — so this is where a pixel stream becomes line steps.
///
/// Three things follow from the quantisation, and all three are why the
/// terminal does not adopt `ScrollableBehavior`:
///
/// * there is no fractional position to hold, so a sample worth less than a
///   line has to be **banked** (`scroll_residue`) rather than applied or
///   dropped. The old code divided pixels by a hardcoded `16.0` and rounded,
///   which made every trackpad sample under half a line — and every slow finger
///   — move nothing at all, permanently;
/// * there is no offset to rubber-band, so a pan past either end of the ring
///   has nowhere to go. What a kinetic pan means here is therefore exactly:
///   line steps arrive at frame rate from the fling pump and stop when the
///   simulation stops or the ring ends. There is no overscroll and no settle
///   because there is no continuum to settle onto;
/// * a fling that runs out of scrollback should hand the rest outward, so the
///   pan path answers `Ignored` at the boundary. The wheel keeps answering
///   `Handled` unconditionally — a terminal absorbs the wheel the way every
///   terminal does, and that is the byte-for-byte mouse behaviour.
///
/// Sign: [`ScrollDelta`](teksilo_core::event::ScrollDelta) is positive when the
/// scroll **offset grows**, i.e. toward the end of the content — the platform
/// layer negates winit's natural sign to get there
/// (`event_translation::scroll_delta`). [`Scroll::Delta`] is positive toward
/// **older** output, which is the other direction. Hence the negation below,
/// and hence the wheel used to scroll backwards.
fn scroll_handler(
    state: &Rc<RefCell<TerminalState>>,
    touch: &Rc<TerminalTouch>,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    let WidgetEvent::Scroll {
        delta,
        modifiers,
        phase,
        pointer,
        window_position,
        ..
    } = event
    else {
        return EventResponse::Ignored;
    };
    let is_pan = ctx.scroll_source() == ScrollSource::TouchPan;

    if is_pan {
        // A finger the child owns is not scrolling anything local. With one
        // contact under `AsButton1` the press was already reported and its
        // moves are going out as drag reports; absorbing the synthesised pan
        // keeps an enclosing scrollable from moving under the same finger.
        if reports_pointer(&state.borrow(), pointer.kind) {
            return EventResponse::Handled;
        }
        // Pan sessions are per contact, so two fingers deliver two streams.
        // Honour one of them; the other is absorbed, not chained, because the
        // gesture is this surface's either way.
        {
            let mut st = state.borrow_mut();
            match st.pan_owner {
                Some(owner) if owner != pointer.id => return EventResponse::Handled,
                Some(_) => {}
                None => st.pan_owner = Some(pointer.id),
            }
            if *phase == ScrollPhase::Ended {
                st.pan_owner = None;
                st.scroll_residue = 0.0;
                return EventResponse::Handled;
            }
        }
    }

    // Lines, in the framework's sign (positive = toward the end of the
    // content = toward newer output).
    let lines = {
        let st = state.borrow();
        match delta {
            teksilo_core::event::ScrollDelta::Lines { y, .. } => *y,
            teksilo_core::event::ScrollDelta::Pixels { y, .. } => y / st.metrics.height.max(1.0),
        }
    };
    if lines == 0.0 {
        return EventResponse::Handled;
    }

    // Report the wheel to the child if it enabled mouse reporting (Shift forces
    // local scrollback). A pan is never reported: a finger's scroll is the
    // view's, and `AsButton1` has already been given its chance above.
    let report = !is_pan && reports_pointer(&state.borrow(), pointer.kind) && !modifiers.shift();
    if report {
        let button = if lines < 0.0 {
            MouseButton::WheelUp
        } else {
            MouseButton::WheelDown
        };
        let count = (lines.abs().round() as usize).max(1);
        let (col, row) = wheel_report_cell(state, *window_position);
        let mut st = state.borrow_mut();
        for _ in 0..count {
            report_mouse(&mut st, MouseKind::Press, button, col, row, *modifiers);
        }
        drop(st);
        ctx.request_frame();
        return EventResponse::Handled;
    }

    // Otherwise scroll the local scrollback. `Scroll::Delta` counts toward
    // older output, so the sign flips here.
    let older = -lines;
    let (steps, before, room) = {
        let mut st = state.borrow_mut();
        st.scroll_residue += older;
        let steps = st.scroll_residue.trunc();
        st.scroll_residue -= steps;
        let (offset, history) = st
            .engine
            .as_ref()
            .map(|e| (e.display_offset(), e.history_len()))
            .unwrap_or((0, 0));
        let room = if older > 0.0 {
            offset < history
        } else {
            offset > 0
        };
        (steps as i32, offset, room)
    };
    if steps != 0 {
        scroll_view(state, Scroll::Delta(steps), ctx);
        // The viewport moved under the affordances, so their cell coordinates
        // no longer name the same text.
        touch.dismiss();
    }
    let moved = state
        .borrow()
        .engine
        .as_ref()
        .map(|e| e.display_offset())
        .unwrap_or(0)
        != before;

    if is_pan && !moved && !room {
        // The ring has nothing left in that direction, so the rest of the
        // gesture belongs to whatever encloses this terminal.
        EventResponse::Ignored
    } else {
        EventResponse::Handled
    }
}

fn scroll_view(state: &Rc<RefCell<TerminalState>>, scroll: Scroll, ctx: &mut EventContext) {
    let mut st = state.borrow_mut();
    if let Some(engine) = st.engine.as_mut() {
        engine.scroll(scroll);
    }
    st.refresh_snapshot();
    drop(st);
    ctx.request_frame();
}

/// Select the word or line under a multi-tap, and — for a **finger** — raise the
/// handles that adjust it.
///
/// A cursor gets no touch chrome: it has a drag for adjusting a selection and
/// two handles hanging off the grid would be in its way. The word and line
/// semantics themselves are the engine's for both devices, so the two can never
/// disagree about what a "word" is.
fn select_at(
    state: &Rc<RefCell<TerminalState>>,
    signals: &TerminalSignals,
    touch: &Rc<TerminalTouch>,
    tap: &TapEvent,
    kind: SelectionKind,
    ctx: &mut EventContext,
) {
    // While the child owns the pointer its taps are already going out as button
    // reports; selecting locally on top of that would highlight text under a
    // full-screen program that never asked for it.
    if reports_pointer(&state.borrow(), tap.pointer.kind) {
        return;
    }
    let cell = cell_at_position(state, tap.position);
    let mut st = state.borrow_mut();
    if let Some((col, row, side)) = cell {
        if let Some(engine) = st.engine.as_mut() {
            // Word/Line selections expand to their own boundaries, so the side
            // is immaterial; pass it through for consistency.
            engine.selection_start(row, col, side, kind);
            engine.selection_update(row, col, side);
        }
        st.refresh_snapshot();
    }
    let has_sel = st
        .engine
        .as_ref()
        .and_then(|e| e.selection_text())
        .is_some();
    drop(st);
    signals.has_selection.set(has_sel);
    if tap.pointer.kind.is_direct() && has_sel {
        touch.raise(state, ctx);
    }
    ctx.request_frame();
}

/// Run one context-menu command.
fn run_command(
    state: &Rc<RefCell<TerminalState>>,
    signals: &TerminalSignals,
    command: TerminalMenuCommand,
    ctx: &mut EventContext,
) {
    match command {
        TerminalMenuCommand::Copy => copy_selection(state, ctx),
        TerminalMenuCommand::Paste => paste_clipboard(state, ctx),
        TerminalMenuCommand::SelectAll => {
            {
                let mut st = state.borrow_mut();
                if let Some(engine) = st.engine.as_mut() {
                    engine.select_all();
                }
                st.refresh_snapshot();
            }
            let has_sel = state
                .borrow()
                .engine
                .as_ref()
                .and_then(|e| e.selection_text())
                .is_some();
            signals.has_selection.set(has_sel);
        }
        TerminalMenuCommand::Clear => {
            {
                let mut st = state.borrow_mut();
                if let Some(engine) = st.engine.as_mut() {
                    engine.clear_screen();
                }
                st.refresh_snapshot();
            }
            signals.has_selection.set(false);
        }
    }
    // No affordance bookkeeping here, and the reason is worth recording because
    // the obvious code is dead code. Two of these commands move the selection, so
    // an explicit `dismiss()` or `refresh()` looks called for — but the only route
    // into this function is the context menu, and mounting that menu already took
    // the affordance band down (`show_context_menu_for` → `dismiss_except`, which
    // keeps only overlays containing the clicked widget). Measured: a `dismiss()`
    // in either arm could be deleted with every test green. What the selection
    // change does need is the geometry re-derived, and `place_children` does that
    // on the layout this frame's `request_frame` brings — which is also why this
    // function takes no affordance handle at all.
    ctx.request_frame();
}

/// One page of scrollback, from an assistive client's `ScrollUp` / `ScrollDown`.
///
/// A page rather than a line because that is what the two AccessKit actions mean
/// everywhere else in the framework, and because a screen reader driving a
/// terminal a line at a time would need one action per line of a 10 000-line
/// buffer.
fn access_scroll(
    state: &Rc<RefCell<TerminalState>>,
    touch: &Rc<TerminalTouch>,
    scroll: Scroll,
    ctx: &mut EventContext,
) {
    scroll_view(state, scroll, ctx);
    touch.dismiss();
    ctx.request_accessibility_update();
}

/// Map a pointer position to `(column, row, cell-side)`. The side is which half
/// of the cell the pointer fell on, so a right-to-left selection drag includes
/// the same cells as a left-to-right one.
fn cell_at_position(
    state: &Rc<RefCell<TerminalState>>,
    position: Point,
) -> Option<(usize, usize, CellSide)> {
    let st = state.borrow();
    // `position` is **widget-local**: `WidgetTree::localize_event` rewrites
    // every pointer position and every `TapEvent` through
    // `WidgetArena::local_pointer_position`, which subtracts the node's bounds
    // origin, before a handler sees it. `origin` is in **window** space (it is
    // `bounds.origin` plus the chrome inset), so the offset between the two is
    // the inset alone.
    //
    // Subtracting the whole of `origin` was a defect: for a terminal anywhere
    // but the window's top-left corner it moved every press left and up by the
    // widget's own position, so a press at the terminal's first cell resolved
    // to a negative coordinate and the function answered `None` — no
    // selection, no cell under the pointer, and no mouse report, for a mouse
    // as much as for a finger.
    let x = position.x - (st.origin.x - st.bounds.x);
    let y = position.y - (st.origin.y - st.bounds.y);
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let cw = st.metrics.width;
    let (col, row) = mouse::cell_at(x, y, cw, st.metrics.height, st.cols, st.rows);
    let side = if cw > 0.0 && (x - col as f32 * cw) >= cw / 2.0 {
        CellSide::Right
    } else {
        CellSide::Left
    };
    Some((col, row, side))
}

/// Convert a **window**-space point into the widget-local space
/// [`cell_at_position`] reads.
///
/// This is the conversion the framework contract asks a consumer to make.
/// [`WidgetTree::localize_event`] rewrites every pointer position and every
/// gesture into widget-local space before a handler sees it; the two fields it
/// deliberately leaves alone say so in their own names —
/// `Scroll::window_position` and `PointerCancel::window_position` — because both
/// of their framework uses need window space (the router routes by the first,
/// and the kinetic tracker behind a pan follows the pointer rather than the
/// widget). A widget that wants cell coordinates out of one therefore converts,
/// and converts *into* the local space rather than growing a second copy of the
/// cell arithmetic: getting the origin inset wrong is the mistake this file has
/// already made once.
///
/// [`WidgetTree::localize_event`]: teksilo_core::widget_tree::WidgetTree
fn window_to_local(st: &TerminalState, position: Point) -> Point {
    Point::new(position.x - st.bounds.x, position.y - st.bounds.y)
}

/// The cell a wheel notch's VT report names.
///
/// The cell under the pointer, the same as a press names: a full-screen program
/// reads the coordinates to decide *which* pane the wheel turned over, so a
/// report pinned at the grid's origin tells `less` in a split `tmux` that the
/// user is always pointing at the top-left corner. The origin survives only as
/// the answer when there is no pointer position to be had — a wheel before the
/// cursor has ever been over the terminal.
///
/// Two sources, because a wheel has two producers: a positioned sample answers
/// from its own position (window space, so through [`window_to_local`]), and a
/// bare hover-routed notch answers from where the pointer last was. Both end in
/// [`cell_at_position`].
fn wheel_report_cell(
    state: &Rc<RefCell<TerminalState>>,
    position: Option<Point>,
) -> (usize, usize) {
    let local = match position {
        Some(window) => window_to_local(&state.borrow(), window),
        None => match state.borrow().last_local_pointer {
            Some(local) => local,
            None => return (0, 0),
        },
    };
    cell_at_position(state, local)
        .map(|(col, row, _)| (col, row))
        .unwrap_or((0, 0))
}

fn copy_selection(state: &Rc<RefCell<TerminalState>>, ctx: &mut EventContext) {
    let text = state
        .borrow()
        .engine
        .as_ref()
        .and_then(|e| e.selection_text());
    if let (Some(text), Some(clipboard)) = (text, ctx.app_state::<ClipboardHandle>()) {
        let _ = clipboard.set_text(&text);
    }
}

fn paste_clipboard(state: &Rc<RefCell<TerminalState>>, ctx: &mut EventContext) {
    if state.borrow().read_only {
        return;
    }
    let Some(clipboard) = ctx.app_state::<ClipboardHandle>() else {
        return;
    };
    let Ok(text) = clipboard.get_text() else {
        return;
    };
    let mode = state
        .borrow()
        .engine
        .as_ref()
        .map(|e| e.mode())
        .unwrap_or_default();
    let bytes = input::encode_paste(&text, mode);
    let mut st = state.borrow_mut();
    if let Some(engine) = st.engine.as_mut() {
        engine.scroll(Scroll::Bottom);
        engine.write(&bytes);
    }
    drop(st);
    ctx.request_frame();
}

/// Concatenate a snapshot row's cell text (skipping wide-glyph spacers).
fn row_text(snapshot: &GridSnapshot, row: usize) -> String {
    let mut text = String::with_capacity(snapshot.columns);
    for col in 0..snapshot.columns {
        if let Some(cell) = snapshot.cell(row, col) {
            if cell.attrs.wide_spacer {
                continue;
            }
            text.push_str(&cell.text());
        }
    }
    text.trim_end().to_string()
}

/// The platform copy chord: ⌘C on macOS, Ctrl+Shift+C elsewhere.
fn is_copy_chord(key: Key, mods: Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        key == Key::C && mods.super_key()
    } else {
        key == Key::C && mods.ctrl() && mods.shift()
    }
}

/// The platform paste chord: ⌘V on macOS, Ctrl+Shift+V elsewhere.
fn is_paste_chord(key: Key, mods: Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        key == Key::V && mods.super_key()
    } else {
        key == Key::V && mods.ctrl() && mods.shift()
    }
}
