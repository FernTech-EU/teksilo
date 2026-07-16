//! The `Terminal` widget: the accessible, cross-platform Console view. Owns
//! rendering, input encoding, selection, accessibility and lifecycle; the PTY +
//! VT emulation live behind [`crate::engine::TerminalEngine`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use bastyde_canvas::{Canvas, Point, Rect, Size, SizeProposal};
use bastyde_core::binding::BindingLevel;
use bastyde_core::build_context::BuildContext;
use bastyde_core::event::{EventResponse, Key, Modifiers, PointerButton, WidgetEvent};
use bastyde_core::gesture::TapEvent;
use bastyde_core::ime::ImeContext;
use bastyde_core::signal::Signal;
use bastyde_core::styles::Theme;
use bastyde_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use bastyde_core::widget_builder::HandlerSet;
use bastyde_core::widget_id::WidgetId;
use bastyde_platform::ClipboardHandle;
use bastyde_tokens::TextStyle;

use crate::a11y::{self, LiveAnnouncer};
use crate::color_scheme::ColorScheme;
use crate::engine::{
    CellSide, GridSnapshot, Scroll, SelectionKind, TermCursorShape, TermEvent, TerminalCommand,
    TerminalEngineFactory, TerminalExit,
};
use crate::input::{self, InputConfig};
use crate::mouse::{self, MouseButton, MouseKind};
use crate::render::{self, CellMetrics, RenderParams};
use crate::state::{
    self, DragState, DrainResult, TerminalState, blank_snapshot, compute_layout, drain_and_advance,
};
use crate::style::{RecipeTerminalStyle, TerminalChrome, TerminalStyle};

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

        // Handlers.
        let read_only = self.state.borrow().read_only;
        let mut handlers = HandlerSet::new()
            .focusable(true)
            .keyboard_capture(true)
            .cursor(CursorIcon::Text);
        if !read_only {
            handlers = handlers.ime_input(ImeContext::text());
        }

        let st = self.state.clone();
        handlers = handlers.on_focus(move |gained, _ctx| {
            let mut st = st.borrow_mut();
            st.focused = gained;
            st.blink_on = true;
            st.blink_last = None;
        });

        let st = self.state.clone();
        handlers = handlers.on_key(move |event, ctx| keyboard_handler(&st, event, ctx));

        let st = self.state.clone();
        let sig = self.signals.clone();
        handlers =
            handlers.on_pointer_event(move |event, ctx| pointer_handler(&st, &sig, event, ctx));

        let st = self.state.clone();
        handlers = handlers.on_scroll(move |event, ctx| scroll_handler(&st, event, ctx));

        let st = self.state.clone();
        let sig = self.signals.clone();
        handlers = handlers.on_double_tap(move |tap, ctx| {
            select_at(&st, &sig, tap, SelectionKind::Word);
            ctx.request_frame();
        });

        let st = self.state.clone();
        let sig = self.signals.clone();
        handlers = handlers.on_triple_tap(move |tap, ctx| {
            select_at(&st, &sig, tap, SelectionKind::Line);
            ctx.request_frame();
        });

        ctx.apply_self_handlers(handlers);

        // Cursor-blink / visual-bell frame effect.
        let st = self.state.clone();
        let tick = ctx.frame_tick();
        ctx.effect(&tick, move |_delta| tick_frame(&st));

        // Mirror window-active state (drives caret hiding / desaturation).
        let st = self.state.clone();
        let wa = ctx.window_active_signal();
        ctx.effect(&wa, move |active| {
            st.borrow_mut().window_active = *active;
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
        _ctx: &LayoutContext,
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
        self.apply_drain(drain);

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
                let flash = bastyde_tokens::Color::new(
                    scheme.foreground.r(),
                    scheme.foreground.g(),
                    scheme.foreground.b(),
                    alpha.max(0.0),
                );
                canvas.fill_rect(bounds, flash);
            }
        }
    }

    fn accessibility(&self, builder: &mut bastyde_core::accessibility::AccessNodeBuilder) {
        let st = self.state.borrow();
        a11y::build_terminal_a11y(builder, &st.snapshot, &self.label);
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
            eprintln!("bastyde-terminal: failed to spawn `{prog}`: {err}");
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
            "bastyde-terminal: font family `{family}` is not monospace; the grid \
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
            // Shift+PageUp/Down scrolls the scrollback.
            if *key == Key::PageUp && modifiers.shift() {
                scroll_view(state, Scroll::PageUp, ctx);
                return EventResponse::Handled;
            }
            if *key == Key::PageDown && modifiers.shift() {
                scroll_view(state, Scroll::PageDown, ctx);
                return EventResponse::Handled;
            }

            if state.borrow().read_only {
                return EventResponse::Handled;
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
                let mut st = state.borrow_mut();
                if let Some(engine) = st.engine.as_mut() {
                    // A keystroke returns to the live prompt.
                    engine.scroll(Scroll::Bottom);
                    engine.write(&bytes);
                }
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

fn pointer_handler(
    state: &Rc<RefCell<TerminalState>>,
    signals: &TerminalSignals,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    match event {
        WidgetEvent::PointerDown {
            position,
            button,
            modifiers,
        } => {
            let cell = cell_at_position(state, *position);
            let report = { mouse_reporting_active(&state.borrow()) && !modifiers.shift() };
            if report {
                if let (Some(mb), Some((col, row, _))) = (to_mouse_button(*button), cell) {
                    let mut st = state.borrow_mut();
                    report_mouse(&mut st, MouseKind::Press, mb, col, row, *modifiers);
                    st.mouse_button_held = Some(mb);
                }
                return EventResponse::Handled;
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
                return EventResponse::Handled;
            }
            EventResponse::Ignored
        }
        WidgetEvent::PointerMove { position } => {
            let cell = cell_at_position(state, *position);
            let held = state.borrow().mouse_button_held;
            if let Some(mb) = held {
                if let Some((col, row, _)) = cell {
                    let mut st = state.borrow_mut();
                    if mouse_reporting_active(&st) {
                        report_mouse(&mut st, MouseKind::Drag, mb, col, row, Modifiers::NONE);
                    }
                }
                return EventResponse::Handled;
            }
            // Any-motion reporting (mode 1003).
            let motion = {
                let st = state.borrow();
                mouse_reporting_active(&st)
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
                return EventResponse::Handled;
            }
            EventResponse::Ignored
        }
        WidgetEvent::PointerUp {
            position,
            button,
            modifiers,
        } => {
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
                    return EventResponse::Handled;
                }
            }
            EventResponse::Ignored
        }
        _ => EventResponse::Ignored,
    }
}

fn scroll_handler(
    state: &Rc<RefCell<TerminalState>>,
    event: &WidgetEvent,
    ctx: &mut EventContext,
) -> EventResponse {
    let WidgetEvent::Scroll { delta, modifiers } = event else {
        return EventResponse::Ignored;
    };
    let lines = match delta {
        bastyde_core::event::ScrollDelta::Lines { y, .. } => *y,
        bastyde_core::event::ScrollDelta::Pixels { y, .. } => y / 16.0,
    };
    if lines == 0.0 {
        return EventResponse::Handled;
    }

    // Report the wheel to the child if it enabled mouse reporting (Shift forces
    // local scrollback).
    let report = mouse_reporting_active(&state.borrow()) && !modifiers.shift();
    if report {
        let button = if lines > 0.0 {
            MouseButton::WheelUp
        } else {
            MouseButton::WheelDown
        };
        let count = (lines.abs().round() as usize).max(1);
        let mut st = state.borrow_mut();
        for _ in 0..count {
            report_mouse(&mut st, MouseKind::Press, button, 0, 0, *modifiers);
        }
        drop(st);
        ctx.request_frame();
        return EventResponse::Handled;
    }

    // Otherwise scroll the local scrollback (positive delta = older lines).
    let n = lines.round() as i32;
    if n != 0 {
        scroll_view(state, Scroll::Delta(n), ctx);
    }
    EventResponse::Handled
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

fn select_at(
    state: &Rc<RefCell<TerminalState>>,
    signals: &TerminalSignals,
    tap: &TapEvent,
    kind: SelectionKind,
) {
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
}

/// Map a pointer position to `(column, row, cell-side)`. The side is which half
/// of the cell the pointer fell on, so a right-to-left selection drag includes
/// the same cells as a left-to-right one.
fn cell_at_position(
    state: &Rc<RefCell<TerminalState>>,
    position: Point,
) -> Option<(usize, usize, CellSide)> {
    let st = state.borrow();
    let x = position.x - st.origin.x;
    let y = position.y - st.origin.y;
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
