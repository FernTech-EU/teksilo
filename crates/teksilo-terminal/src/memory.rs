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
    GridSnapshot, PtyGeom, Scroll, SelectionKind, SpawnedEngine, TermEvent, TermMode,
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
    fn scroll(&mut self, _scroll: Scroll) {}
    fn mode(&self) -> TermMode {
        self.shared.borrow().mode
    }
    fn history_len(&self) -> usize {
        self.shared.borrow().history_len
    }
    fn display_offset(&self) -> usize {
        self.shared.borrow().display_offset
    }
    fn selection_start(
        &mut self,
        line: usize,
        column: usize,
        _side: crate::engine::CellSide,
        kind: SelectionKind,
    ) {
        self.shared
            .borrow_mut()
            .selections
            .push((line, column, kind));
    }
    fn selection_update(&mut self, _line: usize, _column: usize, _side: crate::engine::CellSide) {}
    fn select_all(&mut self) {}
    fn selection_clear(&mut self) {
        self.shared.borrow_mut().selection_text = None;
    }
    fn selection_text(&self) -> Option<String> {
        self.shared.borrow().selection_text.clone()
    }
    fn clear_screen(&mut self) {}
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
