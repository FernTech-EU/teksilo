// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! An in-memory [`TerminalEngine`] test double. It performs *no* VT emulation —
//! it records what the view writes/resizes/kills and returns a snapshot the
//! test sets directly. Used by this crate's own headless tests and available to
//! downstream apps for testing terminal-driven UI without spawning a real
//! shell. Mirrors `teksilo_webview::MemoryWebViewBackend`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::Read;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};

use crate::engine::{
    CellSide, GridSnapshot, PtyGeom, Scroll, SelectionKind, SpawnedEngine, TermEvent, TermMode,
    TerminalCommand, TerminalEngine, TerminalEngineFactory, TerminalExit,
};

/// The shared, test-observable state behind a [`MemoryEngine`]. Obtain it from
/// [`MemoryEngineFactory::shared`] before building the widget, then read/write
/// it from the test.
///
/// `#[non_exhaustive]`, because the set of things a test can observe grows every
/// time the view learns to drive one more part of an engine. The supported way
/// to get one is [`MemoryEngineFactory::shared`], and reading and assigning
/// fields on the handle it hands back is what the attribute leaves untouched:
///
/// ```
/// use teksilo_terminal::MemoryEngineFactory;
///
/// let factory = MemoryEngineFactory::new();
/// let shared = factory.shared();
/// shared.borrow_mut().history_len = 100;
/// assert!(shared.borrow().writes.is_empty());
/// ```
///
/// What it forbids is the one form a new field would break — an out-of-crate
/// struct literal, functional-update syntax included:
///
/// ```compile_fail
/// let shared = teksilo_terminal::MemoryShared {
///     history_len: 100,
///     ..Default::default()
/// };
/// ```
#[derive(Default)]
#[non_exhaustive]
pub struct MemoryShared {
    /// Every byte the view wrote toward the "child" (encoded keystrokes, paste).
    pub writes: Vec<u8>,
    /// Every byte the view fed to the parser (`advance`).
    pub advanced: Vec<u8>,
    /// The grid the engine reports from [`TerminalEngine::snapshot`]. `None`
    /// yields a blank grid sized to the last geometry.
    pub snapshot: Option<GridSnapshot>,
    /// The mode the engine reports.
    pub mode: TermMode,
    /// Events the next [`TerminalEngine::drain_events`] will return.
    pub events: Vec<TermEvent>,
    /// The text the engine reports as selected.
    pub selection_text: Option<String>,
    /// Recorded resize requests.
    pub resizes: Vec<PtyGeom>,
    /// Set once [`TerminalEngine::kill`] has been called.
    pub killed: bool,
    /// The value the engine returns from [`TerminalEngine::poll_exit`].
    pub exit: Option<TerminalExit>,
    /// How long each `advance` takes, so a test can make parsing slow.
    #[cfg(test)]
    pub(crate) advance_delay: Option<std::time::Duration>,
    /// The current scrollback display offset the engine reports.
    pub display_offset: usize,
    /// The scrollback length the engine reports.
    pub history_len: usize,
    /// Recorded selection anchors (`(line, column, kind)`).
    pub selections: Vec<(usize, usize, SelectionKind)>,
    /// Recorded selection heads (`(line, column, side)`) from
    /// [`TerminalEngine::selection_update`].
    pub selection_updates: Vec<(usize, usize, CellSide)>,
    /// Every scrollback movement the view asked for, in order.
    pub scrolls: Vec<Scroll>,
    /// How many times the view asked to select the whole buffer.
    pub select_all_calls: usize,
    /// How many times the view asked to clear the visible screen.
    pub clear_screen_calls: usize,
}

/// The in-memory engine (see the module docs). Its "child" prints what the
/// test writes to its [`MemoryOutput`], and runs until that is closed, or the
/// engine is killed or dropped.
pub struct MemoryEngine {
    shared: Rc<RefCell<MemoryShared>>,
    geom: PtyGeom,
    output: MemoryOutput,
}

impl MemoryEngine {
    fn blank_snapshot(&self) -> GridSnapshot {
        let cols = self.geom.cols as usize;
        let rows = self.geom.rows as usize;
        GridSnapshot {
            columns: cols,
            screen_lines: rows,
            cells: vec![crate::engine::Cell::default(); cols * rows],
            cursor: crate::engine::CursorInfo {
                line: 0,
                column: 0,
                shape: crate::engine::TermCursorShape::Block,
                visible: true,
            },
            selection: None,
            display_offset: 0,
            history_len: 0,
        }
    }
}

impl TerminalEngine for MemoryEngine {
    fn advance(&mut self, bytes: &[u8]) {
        #[cfg(test)]
        if let Some(delay) = self.shared.borrow().advance_delay {
            std::thread::sleep(delay);
        }
        self.shared.borrow_mut().advanced.extend_from_slice(bytes);
    }
    fn write(&mut self, bytes: &[u8]) {
        self.shared.borrow_mut().writes.extend_from_slice(bytes);
    }
    fn resize(&mut self, geom: PtyGeom) {
        self.geom = geom;
        self.shared.borrow_mut().resizes.push(geom);
    }
    fn snapshot(&self) -> GridSnapshot {
        self.shared
            .borrow()
            .snapshot
            .clone()
            .unwrap_or_else(|| self.blank_snapshot())
    }
    /// Records the request **and** models the ring position, so a test can
    /// watch the viewport move: `display_offset` walks between 0 (the live
    /// prompt) and `history_len` (the oldest scrollback line) exactly as a real
    /// engine's does. Without the model, every scroll answered "nothing moved"
    /// and no test could tell a working scroll from a dropped one.
    fn scroll(&mut self, scroll: Scroll) {
        let mut shared = self.shared.borrow_mut();
        shared.scrolls.push(scroll);
        let history = shared.history_len as i64;
        let current = shared.display_offset as i64;
        let next = match scroll {
            Scroll::Delta(n) => current + n as i64,
            Scroll::PageUp => current + self.geom.rows as i64,
            Scroll::PageDown => current - self.geom.rows as i64,
            Scroll::Top => history,
            Scroll::Bottom => 0,
        };
        shared.display_offset = next.clamp(0, history) as usize;
    }
    fn mode(&self) -> TermMode {
        self.shared.borrow().mode
    }
    fn history_len(&self) -> usize {
        self.shared.borrow().history_len
    }
    fn display_offset(&self) -> usize {
        self.shared.borrow().display_offset
    }
    /// Records the anchor, and — for a `Word` or `Line` selection — reports some
    /// selected text.
    ///
    /// The second half is not decoration. A real engine's word or line selection
    /// covers cells the moment it is anchored, so a view that asks "is anything
    /// selected?" straight after a double-click is told yes. Without it every
    /// such check answered no here, and a test that turned on what the answer
    /// gates could not fail.
    fn selection_start(
        &mut self,
        line: usize,
        column: usize,
        _side: crate::engine::CellSide,
        kind: SelectionKind,
    ) {
        let mut shared = self.shared.borrow_mut();
        shared.selections.push((line, column, kind));
        if matches!(kind, SelectionKind::Word | SelectionKind::Line) {
            shared.selection_text = Some("selected".to_string());
        }
    }
    fn selection_update(&mut self, line: usize, column: usize, side: CellSide) {
        self.shared
            .borrow_mut()
            .selection_updates
            .push((line, column, side));
    }
    fn select_all(&mut self) {
        self.shared.borrow_mut().select_all_calls += 1;
    }
    fn selection_clear(&mut self) {
        let mut shared = self.shared.borrow_mut();
        shared.selection_text = None;
        if let Some(snapshot) = shared.snapshot.as_mut() {
            snapshot.selection = None;
        }
    }
    fn selection_text(&self) -> Option<String> {
        self.shared.borrow().selection_text.clone()
    }
    fn clear_screen(&mut self) {
        self.shared.borrow_mut().clear_screen_calls += 1;
    }
    fn reset(&mut self) {}
    fn drain_events(&mut self) -> Vec<TermEvent> {
        std::mem::take(&mut self.shared.borrow_mut().events)
    }
    fn poll_exit(&mut self) -> Option<TerminalExit> {
        self.shared.borrow().exit
    }
    fn kill(&mut self) {
        self.shared.borrow_mut().killed = true;
        // A killed child prints nothing more: the reader sees the end.
        self.output.close();
    }
}

impl Drop for MemoryEngine {
    /// The child goes with its engine, so a reader blocked on its output ends.
    fn drop(&mut self) {
        self.output.close();
    }
}

/// The output side of one spawned [`MemoryEngine`]'s child: what a test
/// writes here, the terminal reads as if the child had printed it. `Send`,
/// `Sync` and `Clone`, so a test can write from any thread.
///
/// ```
/// use teksilo_terminal::MemoryEngineFactory;
///
/// let factory = MemoryEngineFactory::new();
/// let output = factory.output();
/// let remote = output.clone();
/// std::thread::spawn(move || remote.write(b"hello")).join().unwrap();
/// output.close();
/// ```
#[derive(Clone)]
pub struct MemoryOutput {
    inner: Arc<OutputChannel>,
}

struct OutputChannel {
    state: Mutex<OutputState>,
    changed: Condvar,
}

#[derive(Default)]
struct OutputState {
    pending: VecDeque<u8>,
    closed: bool,
    /// The reader returned the end of the output, or was dropped.
    released: bool,
    /// The reader is blocked in a read, so a test can tell it is.
    #[cfg(test)]
    reading: bool,
}

impl MemoryOutput {
    fn new() -> Self {
        Self {
            inner: Arc::new(OutputChannel {
                state: Mutex::new(OutputState::default()),
                changed: Condvar::new(),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, OutputState> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Queue `bytes` as if the child printed them. Ignored once closed.
    pub fn write(&self, bytes: &[u8]) {
        let mut state = self.lock();
        if state.closed {
            return;
        }
        state.pending.extend(bytes);
        drop(state);
        self.inner.changed.notify_all();
    }

    /// End the child's output: the terminal reads what is left, then the end,
    /// and reports the child's exit at its next pull. Idempotent.
    pub fn close(&self) {
        self.lock().closed = true;
        self.inner.changed.notify_all();
    }

    /// Bytes written and not yet read by the terminal.
    #[cfg(test)]
    pub(crate) fn pending_len(&self) -> usize {
        self.lock().pending.len()
    }

    /// Wait until the terminal's reader is blocked in a read, or `timeout`
    /// passes.
    #[cfg(test)]
    pub(crate) fn wait_reader_blocked(&self, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        while !self.lock().reading {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::yield_now();
        }
        true
    }

    /// Wait until the terminal's reader has seen the end of the output, or
    /// `timeout` passes.
    #[cfg(test)]
    pub(crate) fn wait_reader_released(&self, timeout: std::time::Duration) -> bool {
        let state = self.lock();
        let (state, _) = self
            .inner
            .changed
            .wait_timeout_while(state, timeout, |state| !state.released)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.released
    }
}

impl std::fmt::Debug for MemoryOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.lock();
        f.debug_struct("MemoryOutput")
            .field("pending", &state.pending.len())
            .field("closed", &state.closed)
            .finish()
    }
}

/// The reader the terminal's reader thread reads a [`MemoryEngine`]'s child
/// through: blocks until the test writes, returns the end once the output is
/// closed and drained.
struct MemoryReader {
    output: MemoryOutput,
}

impl Read for MemoryReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        #[cfg_attr(not(test), allow(unused_mut))]
        let mut state = self.output.lock();
        #[cfg(test)]
        {
            state.reading = true;
        }
        #[cfg_attr(not(test), allow(unused_mut))]
        let mut state = self
            .output
            .inner
            .changed
            .wait_while(state, |state| state.pending.is_empty() && !state.closed)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(test)]
        {
            state.reading = false;
        }
        if state.pending.is_empty() {
            state.released = true;
            drop(state);
            self.output.inner.changed.notify_all();
            return Ok(0);
        }
        let n = buf.len().min(state.pending.len());
        for (slot, byte) in buf.iter_mut().zip(state.pending.drain(..n)) {
            *slot = byte;
        }
        Ok(n)
    }
}

impl Drop for MemoryReader {
    fn drop(&mut self) {
        self.output.lock().released = true;
        self.output.inner.changed.notify_all();
    }
}

/// Spawns [`MemoryEngine`]s sharing one [`MemoryShared`] state. Each spawned
/// engine's child has an output of its own: take it with
/// [`output`](Self::output) before the terminal is mounted.
#[derive(Clone)]
pub struct MemoryEngineFactory {
    shared: Rc<RefCell<MemoryShared>>,
    /// The output the next engine spawned reads; replaced at each spawn.
    /// Shared by clones of the factory.
    next_output: Rc<RefCell<MemoryOutput>>,
}

impl Default for MemoryEngineFactory {
    fn default() -> Self {
        Self {
            shared: Rc::default(),
            next_output: Rc::new(RefCell::new(MemoryOutput::new())),
        }
    }
}

impl MemoryEngineFactory {
    pub fn new() -> Self {
        Self::default()
    }

    /// The shared state — read/written by the test to observe or drive the
    /// engine the widget will spawn.
    pub fn shared(&self) -> Rc<RefCell<MemoryShared>> {
        self.shared.clone()
    }

    /// The output of the next engine this factory, or a clone of it, spawns.
    /// Take it before mounting the terminal, like [`shared`](Self::shared).
    /// Writes made before the spawn are delivered; closing it before the
    /// spawn makes the child exit at once. After a spawn, a new call returns
    /// the following engine's.
    pub fn output(&self) -> MemoryOutput {
        self.next_output.borrow().clone()
    }
}

impl TerminalEngineFactory for MemoryEngineFactory {
    fn spawn(
        &self,
        _command: &TerminalCommand,
        geom: PtyGeom,
        _scrollback_lines: usize,
    ) -> std::io::Result<SpawnedEngine> {
        let output = std::mem::replace(&mut *self.next_output.borrow_mut(), MemoryOutput::new());
        Ok(SpawnedEngine {
            engine: Box::new(MemoryEngine {
                shared: self.shared.clone(),
                geom,
                output: output.clone(),
            }),
            reader: Box::new(MemoryReader { output }),
        })
    }
}
