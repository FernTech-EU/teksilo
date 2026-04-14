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
use fern_text::text_document::{DocumentEvent, Subscription, TextCursor, TextDocument};

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

        for event in drained {
            had_events = true;
            match event {
                DocumentEvent::ContentsChanged {
                    position,
                    blocks_affected,
                    ..
                } => {
                    if blocks_affected <= 1 && !self.needs_full_layout {
                        single_pos = Some(position);
                    } else {
                        self.needs_full_layout = true;
                        single_pos = None;
                    }
                }
                DocumentEvent::FormatChanged { .. }
                | DocumentEvent::DocumentReset
                | DocumentEvent::FlowElementsInserted { .. }
                | DocumentEvent::FlowElementsRemoved { .. }
                | DocumentEvent::BlockCountChanged(_) => {
                    self.needs_full_layout = true;
                    single_pos = None;
                }
                DocumentEvent::UndoRedoChanged { .. }
                | DocumentEvent::ModificationChanged(_)
                | DocumentEvent::LongOperationProgress { .. }
                | DocumentEvent::LongOperationFinished { .. } => {}
            }
        }

        if had_events {
            self.content_dirty = true;
            self.document_version
                .set(self.document_version.get().wrapping_add(1));
        }

        (had_events, single_pos)
    }
}
