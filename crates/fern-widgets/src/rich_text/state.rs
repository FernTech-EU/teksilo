//! Shared mutable state for a single `RichTextEditor` instance.
//!
//! The widget's build-time effects and event handlers all need mutable
//! access to the editor's inner state (engine, cursor, scroll signals,
//! pending document events, image cache). `Rc<RefCell<State>>` is the
//! simplest sound way to share that across closures — `&mut self` on
//! `Widget::build()` lives too briefly for effect callbacks to borrow it
//! directly.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use fern_core::Signal;
use fern_text::{RichTextEngine, WrapMode};
use fern_text::text_document::{DocumentEvent, DocumentFragment, Subscription, TextCursor, TextDocument};

use super::image_cache::ImageCache;
use super::policy::{CaretPolicy, PolicyBundle};

pub(crate) type SharedState = Rc<RefCell<EditorState>>;

pub(crate) struct EditorState {
    pub document: TextDocument,
    pub engine: RichTextEngine,
    pub cursor: TextCursor,

    pub policy: PolicyBundle,

    // Reactive bridge — cloned into children (scroll bars, selection badges, etc.)
    pub document_version: Signal<u64>,
    pub has_selection: Signal<bool>,
    pub caret_visible: Signal<bool>,
    pub cursor_position: Signal<usize>,
    pub cursor_anchor: Signal<usize>,
    /// Reactive undo availability — bound by toolbars, updated by the
    /// frame loop when `DocumentEvent::UndoRedoChanged` arrives via
    /// the per-widget event queue.
    pub can_undo: Signal<bool>,
    pub can_redo: Signal<bool>,

    // Scroll state — NOT inside a ScrollArea (§27.10.5).
    pub scroll_x: Signal<f32>,
    pub scroll_y: Signal<f32>,
    pub max_scroll_x: Signal<f32>,
    pub max_scroll_y: Signal<f32>,
    pub viewport_ratio_x: Signal<f32>,
    pub viewport_ratio_y: Signal<f32>,

    // Viewport (widget bounds at last layout). Populated by `paint()`
    // (since the editor is a leaf widget and `place_children` is
    // never called). `viewport_origin` is the widget's top-left in
    // window coordinates, used by event handlers to convert
    // pointer positions (which arrive window-local) into the
    // widget-local space text-typeset's `hit_test` expects.
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub viewport_origin: fern_canvas::Point,

    // Layout strategy state.
    pub needs_full_layout: bool,
    pub last_relayout_block_id: Option<usize>,
    pub content_dirty: bool,

    // Wrap mode as configured by the builder.
    pub wrap_mode: WrapMode,

    // Focus — mirrored from `on_focus` so paint can gate the caret.
    pub has_focus: bool,

    /// Sticky preferred X for vertical navigation (§27.10.12). Set
    /// the first time Up/Down/PageUp/PageDown is pressed, preserved
    /// across further vertical presses so the cursor keeps trying to
    /// land on the same visual column even when crossing short
    /// lines. Cleared on any horizontal or edit action.
    pub preferred_x: Option<f32>,

    /// Wall-clock instant of the last caret-visibility toggle, or
    /// `None` if the caret has never blinked (first focus / reset).
    /// Compared against `Instant::now()` on every frame-loop tick so
    /// the blink rate is independent of frame cadence: if ticks are
    /// skipped, delayed, or clamped, the next tick catches up on
    /// the missed toggles and the visible rhythm stays locked to
    /// `CARET_BLINK_INTERVAL` wall-clock seconds.
    pub blink_last_toggle: Option<std::time::Instant>,

    /// Shared handle into `WidgetTree::frame_tick_requested`. Stashed
    /// here so the frame-tick effect can chain-request another tick
    /// (blink, drag auto-scroll) without needing mutable access to
    /// the tree.
    pub frame_request: Option<Rc<std::cell::Cell<bool>>>,

    // Shared-document event routing. See gap 10 of the plan: each
    // editor subscribes via `on_change` and buffers events in its own
    // queue. The `_event_subscription` field is kept alive by the
    // state so dropping the state unregisters the callback.
    pub event_queue: Arc<Mutex<VecDeque<DocumentEvent>>>,
    pub _event_subscription: Subscription,

    // Resource caches.
    pub image_cache: ImageCache,

    // --- M8b editor preset state (unused by read-only preset) ----------

    /// Accumulates typed characters within a single frame, flushed as
    /// one `cursor.insert_text(batch)` at the start of the next
    /// `frame_loop::tick`. Batching matches the godot reference
    /// (rich_text_edit.rs:296) and collapses a burst of keystrokes
    /// into a single `ContentsChanged` event so incremental relayout
    /// and debounced `text_changed` emission stay O(burst) instead of
    /// O(keystrokes).
    pub pending_chars: String,

    /// Seconds since the last debounce drain. Starts at `1.0`
    /// (already expired) so the very first frame after construction
    /// publishes `can_undo`/`can_redo` immediately without having to
    /// wait 150 ms.
    pub debounce_timer: f32,

    /// Set whenever the document mutated this frame (insert, delete,
    /// format). Drained and emitted as `on_text_changed` command once
    /// the debounce timer crosses 150 ms. Distinct from
    /// `pending_format_changed` so a pure-format edit doesn't pretend
    /// text changed.
    pub pending_text_changed: bool,
    pub pending_format_changed: bool,

    /// Latest `(can_undo, can_redo)` pair from a `DocumentEvent::UndoRedoChanged`
    /// arriving during `drain_events`. Debounced alongside text/format
    /// changes so rapid typing doesn't hammer toolbar observers.
    pub pending_undo_redo: Option<(bool, bool)>,

    /// Active drag-select session state. `Idle` when no primary button is
    /// held; `Selecting` while the user is extending a selection with the
    /// pointer, with a cached auto-scroll velocity for when the pointer
    /// approaches the viewport edges.
    pub drag_state: DragState,

    /// In-process rich clipboard fragment captured by the last Ctrl+C /
    /// Ctrl+X. Compared against the system clipboard plain text on
    /// paste — if they match, the fragment is reinserted to preserve
    /// formatting; otherwise the system text lands as plain text.
    pub rich_clipboard_fragment: Option<DocumentFragment>,
    pub rich_clipboard_plain: Option<String>,

    /// Ctrl+A escalation ladder position. See `keyboard.rs`: when the
    /// caret is inside a table cell the ladder climbs through 4 levels
    /// (paragraph → cell → table → document); outside a table it is a
    /// single-shot `SelectionType::Document` and stays at 0. Reset to 0
    /// by any non-SelectAll key action (matching godot edit.rs:520-521).
    pub select_all_level: u8,

    /// Cached flow snapshot used by the accessibility pass. The
    /// `Widget::accessibility` walk iterates blocks and fragments
    /// to emit AccessKit `Role::Paragraph` / `Role::TextRun`
    /// children; the snapshot itself doesn't change between
    /// rebuilds triggered by focus / resize, so caching it avoids
    /// re-walking the document tree. Invalidated from
    /// `drain_events` when a `ContentsChanged` or `FormatChanged`
    /// event arrives.
    pub accessibility_flow_snapshot: RefCell<
        Option<fern_text::text_document::FlowSnapshot>,
    >,

    /// Per-synthetic-NodeId lookup table populated during the
    /// accessibility walk. Maps each emitted `Role::TextRun` NodeId
    /// to its text-document element_id, absolute-document
    /// character start, and run text. Used by the
    /// `on_access_action_request` handler to convert AccessKit
    /// `SetTextSelection` requests (which reference TextRun NodeIds
    /// and in-run character indices) back into document-absolute
    /// cursor positions.
    pub synthetic_to_element: RefCell<
        std::collections::HashMap<fern_core::accesskit::NodeId, SyntheticElementRef>,
    >,

    /// `(table_id, row, column, rows, columns)` remembered from the
    /// Ctrl+A ladder's level-1 call. After `select(BlockUnderCursor)`
    /// the cursor's position lands on the boundary between the
    /// selected block and the next, which for a single-block cell's
    /// last block means `current_table_cell()` would return `None`
    /// on the following Ctrl+A press — skipping the cell / table
    /// levels and jumping straight to document. Caching the full
    /// cell reference at level 1 keeps the ladder stable across
    /// mid-sequence boundary movement. Cleared whenever
    /// `select_all_level` resets.
    pub select_all_anchor_cell: Option<SelectAllAnchorCell>,
}

/// Cached snapshot of the table cell the caret sat inside at Ctrl+A
/// level 1. Used by levels 2 and 3 to dodge the boundary-ambiguity
/// issue where `TextCursor::current_table_cell()` returns `None`
/// when the cursor is at a block edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectAllAnchorCell {
    pub table_id: usize,
    pub row: usize,
    pub column: usize,
    pub table_rows: usize,
    pub table_columns: usize,
}

/// Per-synthetic-NodeId element reference populated during the
/// rich text editor's accessibility walk. Lets the
/// `on_access_action_request` handler convert an AccessKit
/// `TextSelection` (TextRun NodeId + character index within run)
/// back into a document-absolute cursor position.
#[derive(Debug, Clone)]
pub struct SyntheticElementRef {
    /// Stable element id in text-document.
    pub element_id: u64,
    /// Absolute character position of the run's first character
    /// within the full document.
    pub absolute_start: usize,
    /// The run's text, cached so the handler can convert a char
    /// index to a byte offset without re-querying the document.
    pub text: String,
}

/// Drag-select session lifecycle. Plain `cursor.set_position(hit,
/// KeepAnchor)` handles both text and rectangular cell selection — the
/// cell case falls out automatically from `TextCursor::selection_kind()`
/// at [../text-document/crates/public_api/src/cursor.rs:1200].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragState {
    Idle,
    Selecting {
        /// Per-second scroll velocity requested by the near-edge
        /// auto-scroll ramp. Applied by the frame loop on every tick.
        auto_scroll_v_per_s: f32,
    },
}

impl EditorState {
    pub fn new(
        document: TextDocument,
        engine: RichTextEngine,
        policy: PolicyBundle,
        wrap_mode: WrapMode,
    ) -> SharedState {
        let cursor = document.cursor();

        let event_queue = Arc::new(Mutex::new(VecDeque::<DocumentEvent>::new()));
        let subscription = {
            let queue = event_queue.clone();
            document.on_change(move |event| {
                if let Ok(mut q) = queue.lock() {
                    q.push_back(event);
                }
            })
        };

        let caret_visible = match policy.caret_policy {
            CaretPolicy::Hidden => Signal::new(false),
            CaretPolicy::StaticVisible => Signal::new(true),
            CaretPolicy::Blinking => Signal::new(true),
        };

        // Seed can_undo/can_redo with the document's current state so
        // toolbars wired via `bind_to` see the correct value before the
        // first debounce drain fires.
        let initial_can_undo = document.can_undo();
        let initial_can_redo = document.can_redo();

        Rc::new(RefCell::new(Self {
            document,
            engine,
            cursor,
            policy,
            document_version: Signal::new(0),
            has_selection: Signal::new(false),
            caret_visible,
            cursor_position: Signal::new(0),
            cursor_anchor: Signal::new(0),
            can_undo: Signal::new(initial_can_undo),
            can_redo: Signal::new(initial_can_redo),
            scroll_x: Signal::new(0.0),
            scroll_y: Signal::new(0.0),
            max_scroll_x: Signal::new(0.0),
            max_scroll_y: Signal::new(0.0),
            viewport_ratio_x: Signal::new(1.0),
            viewport_ratio_y: Signal::new(1.0),
            viewport_width: 0.0,
            viewport_height: 0.0,
            viewport_origin: fern_canvas::Point::ZERO,
            needs_full_layout: true,
            last_relayout_block_id: None,
            content_dirty: true,
            wrap_mode,
            has_focus: false,
            event_queue,
            _event_subscription: subscription,
            image_cache: ImageCache::new(),
            preferred_x: None,
            blink_last_toggle: None,
            frame_request: None,
            pending_chars: String::new(),
            // Godot reference starts `debounce_timer` at 1.0 (already
            // expired, > 0.15 s window) so the first tick flushes the
            // initial state immediately instead of waiting 150 ms for
            // the first visible update.
            debounce_timer: 1.0,
            pending_text_changed: false,
            pending_format_changed: false,
            pending_undo_redo: None,
            drag_state: DragState::Idle,
            rich_clipboard_fragment: None,
            rich_clipboard_plain: None,
            select_all_level: 0,
            select_all_anchor_cell: None,
            accessibility_flow_snapshot: RefCell::new(None),
            synthetic_to_element: RefCell::new(std::collections::HashMap::new()),
        }))
    }

    /// Drain the local event queue, classifying events for the layout
    /// strategy. Returns `(had_events, pending_single_pos)`:
    /// `pending_single_pos` is `Some(pos)` only if every event in this
    /// batch was a `ContentsChanged { blocks_affected == 1 }` on the
    /// same block and `needs_full_layout` was already false — otherwise
    /// the frame loop uses `layout_full`.
    pub fn drain_events(&mut self) -> (bool, Option<usize>) {
        let mut had_events = false;
        let mut single_pos: Option<usize> = None;

        let drained: Vec<DocumentEvent> = {
            let mut q = self.event_queue.lock().expect("event queue mutex poisoned");
            q.drain(..).collect()
        };

        // Invalidate the accessibility flow snapshot + synthetic-id
        // map whenever the document actually changes structure or
        // content. Format-only edits (FormatChanged) also
        // invalidate because a new bold run creates a new TextRun
        // node in the accessibility tree with a different
        // synthetic NodeId. The document_version bump at the end
        // of drain_events drives AccessibilityOnly binding
        // propagation, so the widget tree's a11y_dirty flag will
        // flip during process_state_changes in the same frame.
        let mut a11y_snapshot_dirty = false;
        for event in drained {
            had_events = true;
            match event {
                DocumentEvent::ContentsChanged {
                    position,
                    blocks_affected,
                    ..
                } => {
                    self.pending_text_changed = true;
                    a11y_snapshot_dirty = true;
                    if blocks_affected <= 1 && !self.needs_full_layout {
                        single_pos = Some(position);
                    } else {
                        self.needs_full_layout = true;
                        single_pos = None;
                    }
                }
                DocumentEvent::FormatChanged { .. } => {
                    self.pending_format_changed = true;
                    a11y_snapshot_dirty = true;
                    self.needs_full_layout = true;
                    single_pos = None;
                }
                DocumentEvent::DocumentReset
                | DocumentEvent::FlowElementsInserted { .. }
                | DocumentEvent::FlowElementsRemoved { .. }
                | DocumentEvent::BlockCountChanged(_) => {
                    self.pending_text_changed = true;
                    a11y_snapshot_dirty = true;
                    self.needs_full_layout = true;
                    single_pos = None;
                }
                DocumentEvent::UndoRedoChanged { can_undo, can_redo } => {
                    // Stash for the frame loop's debounce drain — don't
                    // fire the signal mid-event so a burst of edits
                    // emits one `undo_redo_changed` per debounce window,
                    // not per keystroke.
                    self.pending_undo_redo = Some((can_undo, can_redo));
                }
                DocumentEvent::ModificationChanged(_)
                | DocumentEvent::LongOperationProgress { .. }
                | DocumentEvent::LongOperationFinished { .. } => {}
            }
        }

        if had_events {
            self.content_dirty = true;
            self.document_version
                .set(self.document_version.get().wrapping_add(1));
        }

        // Drop the cached flow snapshot and synthetic-id lookup
        // whenever the document structure / content / formatting
        // changed. The next accessibility walk rebuilds both
        // lazily from a fresh `document.snapshot_flow()`.
        if a11y_snapshot_dirty {
            *self.accessibility_flow_snapshot.borrow_mut() = None;
            self.synthetic_to_element.borrow_mut().clear();
        }

        (had_events, single_pos)
    }
}
