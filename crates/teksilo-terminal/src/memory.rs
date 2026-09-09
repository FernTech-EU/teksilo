// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! An in-memory [`TerminalEngine`] test double. It performs *no* VT emulation —
//! it records what the view writes/resizes/kills and returns a snapshot the
//! test sets directly. Used by this crate's own headless tests and available to
//! downstream apps for testing terminal-driven UI without spawning a real
//! shell. Mirrors `teksilo_webview::MemoryWebViewBackend`.

use std::cell::RefCell;
use std::io::Read;
use std::rc::Rc;

use crate::engine::{
    CellSide, GridSnapshot, PtyGeom, Scroll, SelectionKind, SpawnedEngine, TermEvent, TermMode,
    TerminalCommand, TerminalEngine, TerminalEngineFactory, TerminalExit,
};

/// The shared, test-observable state behind a [`MemoryEngine`]. Obtain it from
/// [`MemoryEngineFactory::shared`] before building the widget, then read/write
/// it from the test.
#[derive(Default)]
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

/// The in-memory engine (see the module docs).
pub struct MemoryEngine {
    shared: Rc<RefCell<MemoryShared>>,
    geom: PtyGeom,
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
    }
}

/// A reader that is always at end-of-file — a spawned reader thread reading it
/// exits immediately (there is no real child).
struct EofReader;

impl Read for EofReader {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}

/// Spawns [`MemoryEngine`]s sharing one [`MemoryShared`] state.
#[derive(Default, Clone)]
pub struct MemoryEngineFactory {
    shared: Rc<RefCell<MemoryShared>>,
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
}

impl TerminalEngineFactory for MemoryEngineFactory {
    fn spawn(
        &self,
        _command: &TerminalCommand,
        geom: PtyGeom,
        _scrollback_lines: usize,
    ) -> std::io::Result<SpawnedEngine> {
        Ok(SpawnedEngine {
            engine: Box::new(MemoryEngine {
                shared: self.shared.clone(),
                geom,
            }),
            reader: Box::new(EofReader),
        })
    }
}
