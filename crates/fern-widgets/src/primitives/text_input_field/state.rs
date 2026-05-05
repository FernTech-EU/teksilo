//! Shared mutable state for a single `TextInput` instance.
//!
//! Mirrors the `Rc<RefCell<State>>` pattern from
//! [`rich_text::state`](super::super::rich_text) but stripped down
//! for single-line plain-text editing: no scroll bars, no rich
//! formatting, no table/cell state, no image cache.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use fern_canvas::Point;
use fern_core::signal::Signal;
use fern_core::widget::EventContext;
use fern_core::widget_id::WidgetId;
use fern_text::text_document::{DocumentEvent, Subscription, TextCursor, TextDocument};
use fern_text::{RichTextEngine, WrapMode};

use crate::rich_text::image_cache::ImageCache;

/// Type-erased action closure, identical to the one in `button.rs`.
pub(crate) type CommandFactory = Box<dyn Fn(&mut EventContext)>;

/// Per-character input-filter predicate. Returning `false` rejects the
/// character before it enters the document. Applied uniformly to
/// keyboard input, IME commits, and clipboard paste so a filtered
/// field cannot receive disallowed characters through any path.
pub(crate) type CharFilter = Rc<dyn Fn(char) -> bool>;

pub(crate) type SharedState = Rc<RefCell<TextInputState>>;

/// Drag-select session lifecycle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DragState {
    Idle,
    Selecting,
}

pub(crate) struct TextInputState {
    pub document: TextDocument,
    pub engine: RichTextEngine,
    pub cursor: TextCursor,

    // ── Reactive signals ────────────────────────────────────────────
    /// Current text content, kept in sync with the document by the
    /// frame-tick effect. Placeholder visibility and clear-button
    /// visibility bind to this.
    pub text_signal: Signal<String>,
    pub cursor_position: Signal<usize>,
    pub cursor_anchor: Signal<usize>,
    pub has_selection: Signal<bool>,
    pub caret_visible: Signal<bool>,
    pub can_undo: Signal<bool>,
    pub can_redo: Signal<bool>,

    // ── Frame infrastructure ────────────────────────────────────────
    pub frame_request: Option<Rc<Cell<bool>>>,
    pub frame_wake_at: Option<Rc<Cell<Option<std::time::Instant>>>>,
    /// Wall-clock of the last caret blink toggle.
    pub blink_last_toggle: Option<std::time::Instant>,

    // ── Horizontal scroll ───────────────────────────────────────────
    /// Pixel offset applied via canvas translation when text overflows
    /// the viewport width. Managed by `ensure_caret_visible_h`.
    pub scroll_x: f32,
    pub viewport_width: f32,
    pub viewport_origin: Point,

    // ── Pending chars (batched per frame) ────────────────────────────
    pub pending_chars: String,
    pub pending_text_changed: bool,
    /// Text update deferred from tick() to avoid RefCell double-borrow.
    /// The frame-tick effect reads and applies this after dropping the
    /// mutable borrow on state.
    pub deferred_text_update: Option<String>,

    // ── Debounce ────────────────────────────────────────────────────
    pub debounce_timer: f32,
    pub pending_undo_redo: Option<(bool, bool)>,

    // ── Document event subscription ─────────────────────────────────
    pub event_queue: Arc<Mutex<VecDeque<DocumentEvent>>>,
    pub _event_subscription: Subscription,

    // ── Input state ─────────────────────────────────────────────────
    pub has_focus: bool,
    pub drag_state: DragState,
    pub needs_full_layout: bool,
    pub content_dirty: bool,
    /// Empty image cache kept to satisfy paint_frame's API. TextInput
    /// never has inline images.
    pub image_cache: ImageCache,

    // ── Configuration (copied from TextInput at build time) ─────────
    pub max_length: Option<usize>,
    pub read_only: bool,
    pub on_submit: Option<Rc<CommandFactory>>,
    /// Fired exactly once per focus-loss, AFTER the cursor/selection
    /// have been cleared and scroll reset. Used by SpinBox-style
    /// widgets to parse/clamp/reformat on blur.
    pub on_blur: Option<Rc<CommandFactory>>,
    /// Per-character input-filter predicate. `None` admits every
    /// non-control character; `Some(f)` additionally requires `f(c)
    /// == true`. Applied to keyboard input, IME commits, and paste.
    pub char_filter: Option<CharFilter>,
    pub placeholder: String,

    // ── Non-editable suffix ─────────────────────────────────────────
    /// Fixed trailing string rendered flush-right inside the border,
    /// the cursor can never enter it. Empty string = no suffix.
    pub suffix: String,
    /// Independent engine holding the suffix's own single-block flow.
    /// Shares the app's `SharedTypesetter` with the main engine so
    /// glyphs land in the same atlas. `None` until the first paint
    /// that sees a non-empty suffix, which is when we can count on
    /// `SharedTypesetter` being available via `app_state`.
    pub suffix_engine: Option<RichTextEngine>,
    /// Cached logical width of the suffix in pixels, filled when
    /// `suffix_engine` is laid out. Drives both the reduced text
    /// viewport (so text scrolls behind a fixed suffix) and the
    /// suffix paint origin.
    pub suffix_width: f32,

    /// The field widget's own id, used as anchor for overlays (e.g.
    /// the autocomplete popup) and for downstream tests that snapshot
    /// AT trees keyed by widget id.
    pub field_widget_id: Option<WidgetId>,
}

/// Configuration bundle passed from `TextInput::build()` to
/// `TextInputState::new()`. Grouped into a struct to keep the public
/// constructor stable as new hooks are added (SpinBox needs three
/// extra fields over plain TextInput, and a positional argument
/// constructor would have grown to eight or nine parameters).
pub(crate) struct TextInputConfig {
    pub initial_text: String,
    pub max_length: Option<usize>,
    pub read_only: bool,
    pub on_submit: Option<Rc<CommandFactory>>,
    pub on_blur: Option<Rc<CommandFactory>>,
    pub char_filter: Option<CharFilter>,
    pub placeholder: String,
    pub suffix: String,
}

impl TextInputState {
    pub fn new(config: TextInputConfig) -> SharedState {
        let TextInputConfig {
            initial_text,
            max_length,
            read_only,
            on_submit,
            on_blur,
            char_filter,
            placeholder,
            suffix,
        } = config;
        let document = TextDocument::new();
        if !initial_text.is_empty() {
            let _ = document.set_plain_text(&initial_text);
        }
        let cursor = document.cursor();

        let mut engine = RichTextEngine::private_default();
        engine.set_wrap_mode(WrapMode::None);

        let event_queue = Arc::new(Mutex::new(VecDeque::<DocumentEvent>::new()));
        let subscription = {
            let queue = event_queue.clone();
            document.on_change(move |event| {
                if let Ok(mut q) = queue.lock() {
                    q.push_back(event);
                }
            })
        };

        let initial_can_undo = document.can_undo();
        let initial_can_redo = document.can_redo();

        Rc::new(RefCell::new(Self {
            document,
            engine,
            cursor,
            text_signal: Signal::new(initial_text.clone()),
            cursor_position: Signal::new(0),
            cursor_anchor: Signal::new(0),
            has_selection: Signal::new(false),
            caret_visible: Signal::new(true),
            can_undo: Signal::new(initial_can_undo),
            can_redo: Signal::new(initial_can_redo),
            frame_request: None,
            frame_wake_at: None,
            blink_last_toggle: None,
            scroll_x: 0.0,
            viewport_width: 0.0,
            viewport_origin: Point::ZERO,
            pending_chars: String::new(),
            pending_text_changed: false,
            deferred_text_update: None,
            debounce_timer: 1.0, // already expired so first tick flushes immediately
            pending_undo_redo: None,
            event_queue,
            _event_subscription: subscription,
            has_focus: false,
            drag_state: DragState::Idle,
            needs_full_layout: true,
            content_dirty: true,
            image_cache: ImageCache::new(),
            max_length,
            read_only,
            on_submit,
            on_blur,
            char_filter,
            placeholder,
            suffix,
            suffix_engine: None,
            suffix_width: 0.0,
            field_widget_id: None,
        }))
    }

    /// Whether the configured `char_filter` (if any) admits this
    /// character. `None` admits every character; inverted so callers
    /// can write `if !st.char_filter_admits(c) { skip }`.
    pub fn char_filter_admits(&self, c: char) -> bool {
        self.char_filter.as_ref().is_none_or(|f| f(c))
    }

    /// Drain the local event queue. Returns `true` if any events
    /// were processed (the frame loop should re-layout).
    pub fn drain_events(&mut self) -> bool {
        let drained: Vec<DocumentEvent> = {
            let mut q = self.event_queue.lock().expect("event queue mutex poisoned");
            q.drain(..).collect()
        };

        let mut had_events = false;
        for event in drained {
            had_events = true;
            match event {
                DocumentEvent::ContentsChanged { .. }
                | DocumentEvent::DocumentReset
                | DocumentEvent::FlowElementsInserted { .. }
                | DocumentEvent::FlowElementsRemoved { .. }
                | DocumentEvent::BlockCountChanged(_) => {
                    self.pending_text_changed = true;
                    self.needs_full_layout = true;
                }
                DocumentEvent::FormatChanged { .. } => {
                    self.needs_full_layout = true;
                }
                DocumentEvent::UndoRedoChanged { can_undo, can_redo } => {
                    self.pending_undo_redo = Some((can_undo, can_redo));
                }
                DocumentEvent::ModificationChanged(_)
                | DocumentEvent::LongOperationProgress { .. }
                | DocumentEvent::LongOperationFinished { .. } => {}
            }
        }

        if had_events {
            self.content_dirty = true;
        }

        had_events
    }
}

/// Sync the cursor position and selection signals from the current
/// cursor state. Called after any keyboard or mouse action that moves
/// the caret.
pub(crate) fn sync_cursor_signals(state: &SharedState) {
    let st = state.borrow();
    let pos = st.cursor.position();
    let anchor = st.cursor.anchor();
    let has_sel = st.cursor.has_selection();
    if st.cursor_position.get() != pos {
        st.cursor_position.set(pos);
    }
    if st.cursor_anchor.get() != anchor {
        st.cursor_anchor.set(anchor);
    }
    if st.has_selection.get() != has_sel {
        st.has_selection.set(has_sel);
    }
    // Reset blink phase so the caret pops on immediately after movement.
    drop(st);
    let mut st = state.borrow_mut();
    st.blink_last_toggle = Some(std::time::Instant::now());
    st.caret_visible.set(true);
}
