// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The code editor's shared mutable core.
//!
//! One `Rc<RefCell<CodeEditorState>>` is threaded through the wrapper widget,
//! the paint-only body, the gutter, and every event handler — the same shape
//! the rich text editor uses, for the same reason: the widgets are separate
//! nodes in the arena and this is the only thing that joins them.
//!
//! It is a *sibling* of `rich_text::EditorState`, not a reuse of it, because
//! the two documents are different animals. That state carries a table-aware
//! Ctrl+A ladder, a rich clipboard fragment, link and image activation
//! callbacks, and code-block colours; none of that means anything in a source
//! file. This one carries multiple carets, an indentation policy, and a
//! streaming tail. What the two genuinely share — the caret blink clock, the
//! debounce window, the scroll-metric arithmetic — is the crate-internal
//! `common::editor_runtime`, so the duplication here is the part that is
//! honestly different.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use bastyde_core::Signal;
use bastyde_text::text_document::{DocumentEvent, Subscription, TextCursor, TextDocument};
use bastyde_text::{CursorAffinity, RichTextEngine, WrapMode};

use super::config::CodeConfig;
use crate::common::editor_runtime::{CaretBlink, CaretPolicy, Debounce, PolicyBundle};
use crate::rich_text::image_cache::ImageCache;

pub(crate) type SharedState = Rc<RefCell<CodeEditorState>>;

/// Pointer drag session.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DragState {
    Idle,
    /// Extending a selection with the pointer held. `auto_scroll_v_per_s` is
    /// the edge-proximity scroll velocity the frame loop applies each tick, so
    /// scrolling continues while the pointer is held still near an edge.
    Selecting {
        auto_scroll_v_per_s: f32,
    },
}

pub(crate) struct CodeEditorState {
    // --- Document + engine -------------------------------------------------
    pub document: TextDocument,
    pub engine: RichTextEngine,
    /// The primary caret. Always present; it is the one the viewport chases
    /// and the one the accessibility tree reports as *the* selection.
    pub cursor: TextCursor,
    /// Secondary carets, in document order, none coinciding with `cursor`.
    ///
    /// Empty for ordinary single-caret editing, which is the case worth keeping
    /// cheap — a `Vec` that is usually empty costs one null check, where making
    /// every caret an element of one collection would add an indirection to the
    /// hot path and lose the "primary" distinction the AT tree needs.
    pub extra_carets: Vec<TextCursor>,

    pub policy: PolicyBundle,
    pub config: CodeConfig,

    /// Completion provider, session, and the reactive surface the popup binds.
    pub completion: super::completion::CompletionState,

    /// Streaming state, present only for a [`LogView`](super::LogView). Its
    /// presence *is* the "this is a streaming view" flag: [`is_streaming`](Self::is_streaming)
    /// checks it, and `drain_events` re-windows instead of relaying-out when it
    /// is set. `None` for every editable editor, whose paths are then byte-for-byte
    /// unchanged.
    pub log: Option<super::log_stream::LogStreamState>,

    // --- Reactive surface --------------------------------------------------
    pub document_version: Signal<u64>,
    pub caret_visible: Signal<bool>,
    pub cursor_position: Signal<usize>,
    pub cursor_anchor: Signal<usize>,
    pub has_selection: Signal<bool>,
    pub can_undo: Signal<bool>,
    pub can_redo: Signal<bool>,
    /// Number of carets, published so a status bar can show "3 cursors"
    /// without polling. `1` when only the primary is live.
    pub caret_count: Signal<usize>,
    /// The pair of document positions of the bracket next to the primary caret
    /// and its match, or `None`. Maintained only when `config.match_brackets`
    /// is on — it is the language-agnostic *computation* of the match, published
    /// reactively so a status surface or the paint layer can read it; the
    /// highlight rectangle is drawn with the current-line band in the paint
    /// phase, since both are "paint at a document position" and belong together.
    pub bracket_match: Signal<Option<(usize, usize)>>,
    /// Total logical lines, i.e. blocks. Published so the gutter can size
    /// itself to the widest number without asking the document.
    ///
    /// Maintained from `DocumentEvent::BlockCountChanged`, which carries the
    /// count, rather than by calling `TextDocument::block_count()`. That call
    /// claims to be O(1) and is not: it fetches every block, reads each one's
    /// content from the rope, and word-counts it before returning the cached
    /// number it already had. Seeded once here at construction, where paying it
    /// exactly once is fine.
    pub line_count: Signal<usize>,
    /// Fires once per drain batch that contained a genuine content edit.
    pub on_change: Option<Rc<dyn Fn()>>,

    // --- Scroll ------------------------------------------------------------
    // Not a ScrollArea: the editor drives its own overlay bars, because wrap
    // width depends on bar visibility and bar visibility depends on content
    // height, which is a circular measurement inside a ScrollArea.
    pub scroll_x: Signal<f32>,
    pub scroll_y: Signal<f32>,
    pub max_scroll_x: Signal<f32>,
    pub max_scroll_y: Signal<f32>,
    pub viewport_ratio_x: Signal<f32>,
    pub viewport_ratio_y: Signal<f32>,

    // --- Viewport ----------------------------------------------------------
    /// Written only by [`sync_viewport`](Self::sync_viewport).
    pub viewport_width: f32,
    pub viewport_height: f32,
    /// The body's top-left in window coordinates — the engine lays out from
    /// here.
    pub viewport_origin: bastyde_canvas::Point,
    /// The wrapper node's top-left in window coordinates. Pointer positions
    /// arrive wrapper-local, so reaching engine space is
    /// `position + node_origin - viewport_origin`; the body is inset within
    /// the wrapper, so the two origins differ.
    pub node_origin: bastyde_canvas::Point,

    // --- Layout strategy ---------------------------------------------------
    pub needs_full_layout: bool,
    pub last_relayout_block_id: Option<usize>,
    pub content_dirty: bool,
    /// Tells `paint` to render the whole frame rather than a cheaper subset.
    pub pending_full_render: bool,
    /// A paint-only highlight change (the syntax highlighter recoloured
    /// without the text changing). Recolours the cached layout without
    /// reshaping — a colour never changes a glyph advance.
    pub pending_recolor: bool,
    pub wrap_mode: WrapMode,
    /// Cull the render to the visible clip band (opt-in). Meaningful only when
    /// the editor is laid out at full document height inside an outer scroller,
    /// where the body's own bounds span the whole document and the on-screen
    /// slice is a narrow clip — then this renders only the visible band. Off by
    /// default: a normally-scrolling editor already renders a viewport's worth.
    pub window_to_clip: bool,

    // --- Shared runtime ----------------------------------------------------
    pub blink: CaretBlink,
    pub debounce: Debounce,

    // --- Focus -------------------------------------------------------------
    pub has_focus: bool,
    /// `focused AND not occluded` for the host window. A caret in an inactive
    /// window is hidden. Starts `true` to match a fresh tree.
    pub window_active: bool,
    pub focus_signal: Signal<bool>,
    pub self_id: Option<bastyde_core::widget_id::WidgetId>,

    // --- Frame loop handles ------------------------------------------------
    pub frame_request: Option<Rc<std::cell::Cell<bool>>>,
    pub frame_wake_at: Option<Rc<std::cell::Cell<Option<std::time::Instant>>>>,

    // --- Document events ---------------------------------------------------
    pub event_queue: Arc<Mutex<VecDeque<DocumentEvent>>>,
    pub _event_subscription: Subscription,

    // --- Input -------------------------------------------------------------
    /// Characters typed this frame, flushed as one insert at the next tick.
    /// Batching collapses a burst of keystrokes into a single
    /// `ContentsChanged`, keeping relayout O(burst) rather than O(keystrokes).
    pub pending_chars: String,
    pub ime_preedit: Option<String>,
    pub ime_preedit_range: Option<std::ops::Range<usize>>,
    pub last_ime_area: Option<bastyde_canvas::Rect>,
    pub last_chase_pos: Option<usize>,

    pub pending_text_changed: bool,
    pub pending_undo_redo: Option<(bool, bool)>,

    // --- Navigation --------------------------------------------------------
    /// Sticky column for vertical navigation, so crossing a short line does
    /// not permanently pull the caret leftward.
    pub preferred_x: Option<f32>,
    pub cursor_affinity: CursorAffinity,

    // --- Appearance --------------------------------------------------------
    pub follow_text_scale: bool,
    /// Per-editor logical font-size multiplier (`1.0` = 100 %), composed with
    /// a11y text scale at paint. Default `1.0`.
    pub font_size_scale: f32,
    pub last_font_scale: f32,
    pub text_color_prop: Option<bastyde_core::color_prop::ColorProp>,
    pub caret_color_prop: Option<bastyde_core::color_prop::ColorProp>,
    pub selection_color_prop: Option<bastyde_core::color_prop::ColorProp>,
    pub background_prop: Option<bastyde_core::color_prop::ColorProp>,
    pub last_text_color: Option<[f32; 4]>,
    pub last_cursor_color: Option<[f32; 4]>,
    pub last_selection_color: Option<[f32; 4]>,
    /// Whether the caret's line gets a background wash.
    pub current_line_highlight: bool,

    pub drag_state: DragState,

    // --- Resources ---------------------------------------------------------
    /// Required by the shared `rich_text::paint::paint_frame` walker. A code
    /// document has no inline images, so this stays empty — it exists to
    /// satisfy the paint contract rather than because the editor uses it.
    pub image_cache: ImageCache,

    // --- Accessibility -----------------------------------------------------
    pub accessibility_flow_snapshot: RefCell<Option<bastyde_text::text_document::FlowSnapshot>>,
    pub synthetic_to_element:
        RefCell<std::collections::HashMap<bastyde_core::accesskit::NodeId, SyntheticElementRef>>,
    /// Whether appended lines are announced to assistive technology.
    ///
    /// **Off by default, and deliberately opt-in.** A live region is the right
    /// semantic for a handful of meaningful events and actively harmful for a
    /// build log at fifty lines a second: the screen reader queues every one and
    /// the user cannot get a word in. The widget cannot tell those two cases
    /// apart, so the application says which it is.
    pub announce_appends: bool,
}

/// Where an emitted `Role::TextRun` lives in the document, so an AT-initiated
/// `SetTextSelection` (which speaks in run NodeIds and in-run character
/// indices) can be resolved back to a document-absolute cursor position.
#[derive(Debug, Clone)]
pub struct SyntheticElementRef {
    pub element_id: u64,
    pub absolute_start: usize,
    pub text: String,
}

impl CodeEditorState {
    pub fn new(
        document: TextDocument,
        engine: RichTextEngine,
        policy: PolicyBundle,
        config: CodeConfig,
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

        // Seed the caret's visibility from the policy so a viewer never flashes
        // a caret on its first frame before the first tick can hide it.
        let caret_visible = Signal::new(match policy.caret_policy {
            CaretPolicy::Hidden => false,
            CaretPolicy::StaticVisible | CaretPolicy::Blinking => true,
        });

        // Seed undo state from the document so a toolbar bound before the first
        // debounce drain still renders correctly.
        let initial_can_undo = document.can_undo();
        let initial_can_redo = document.can_redo();
        // The one place the expensive block_count() is acceptable: once, at
        // construction. Every later change arrives via BlockCountChanged.
        let initial_lines = document.block_count();

        Rc::new(RefCell::new(Self {
            document,
            engine,
            cursor,
            extra_carets: Vec::new(),
            policy,
            config,
            completion: super::completion::CompletionState::new(),
            log: None,
            document_version: Signal::new(0),
            caret_visible,
            cursor_position: Signal::new(0),
            cursor_anchor: Signal::new(0),
            has_selection: Signal::new(false),
            can_undo: Signal::new(initial_can_undo),
            can_redo: Signal::new(initial_can_redo),
            caret_count: Signal::new(1),
            bracket_match: Signal::new(None),
            line_count: Signal::new(initial_lines),
            on_change: None,
            scroll_x: Signal::new(0.0),
            scroll_y: Signal::new(0.0),
            max_scroll_x: Signal::new(0.0),
            max_scroll_y: Signal::new(0.0),
            viewport_ratio_x: Signal::new(1.0),
            viewport_ratio_y: Signal::new(1.0),
            viewport_width: 0.0,
            viewport_height: 0.0,
            viewport_origin: bastyde_canvas::Point::ZERO,
            node_origin: bastyde_canvas::Point::ZERO,
            needs_full_layout: true,
            last_relayout_block_id: None,
            content_dirty: true,
            pending_full_render: true,
            pending_recolor: false,
            wrap_mode,
            window_to_clip: false,
            blink: CaretBlink::new(),
            debounce: Debounce::new(),
            has_focus: false,
            window_active: true,
            focus_signal: Signal::new(false),
            self_id: None,
            frame_request: None,
            frame_wake_at: None,
            event_queue,
            _event_subscription: subscription,
            pending_chars: String::new(),
            ime_preedit: None,
            ime_preedit_range: None,
            last_ime_area: None,
            last_chase_pos: None,
            pending_text_changed: false,
            pending_undo_redo: None,
            preferred_x: None,
            cursor_affinity: CursorAffinity::default(),
            follow_text_scale: true,
            font_size_scale: 1.0,
            last_font_scale: 1.0,
            text_color_prop: None,
            caret_color_prop: None,
            selection_color_prop: None,
            background_prop: None,
            last_text_color: None,
            last_cursor_color: None,
            last_selection_color: None,
            current_line_highlight: false,
            drag_state: DragState::Idle,
            image_cache: ImageCache::new(),
            accessibility_flow_snapshot: RefCell::new(None),
            synthetic_to_element: RefCell::new(std::collections::HashMap::new()),
            announce_appends: false,
        }))
    }

    /// Engine `font_scale` for this frame: a11y text scale (if followed) ×
    /// per-editor [`font_size_scale`](Self::font_size_scale).
    pub fn effective_font_scale(&self, text_scale: f32) -> f32 {
        let a11y = if self.follow_text_scale {
            text_scale
        } else {
            1.0
        };
        (a11y * self.font_size_scale).clamp(0.1, 10.0)
    }

    /// Adopt the body's final bounds. Returns whether the viewport actually
    /// changed size.
    ///
    /// The single writer of the viewport fields, and the only place that pairs
    /// `engine.set_viewport` with `needs_full_layout` — splitting those two
    /// apart is how a resize ends up laying text out at the old width.
    pub fn sync_viewport(&mut self, bounds: bastyde_canvas::Rect) -> bool {
        self.viewport_origin = bastyde_canvas::Point::new(bounds.x, bounds.y);
        let changed = (self.viewport_width - bounds.width).abs() > 0.5
            || (self.viewport_height - bounds.height).abs() > 0.5;
        if changed {
            self.viewport_width = bounds.width;
            self.viewport_height = bounds.height;
            self.engine.set_viewport(bounds.width, bounds.height);
            self.needs_full_layout = true;
        }
        changed
    }

    /// Whether this is a streaming log view rather than an editor. Drives the
    /// one branch in `drain_events` that keeps a per-line append off the O(n)
    /// full-relayout path.
    pub fn is_streaming(&self) -> bool {
        self.log.is_some()
    }

    /// Ask the streaming layer to re-window on the next tick — the streaming
    /// counterpart of setting `needs_full_layout`, cheap where that is O(n).
    fn mark_rewindow(&mut self) {
        if let Some(log) = self.log.as_mut() {
            log.needs_rewindow = true;
        }
    }

    /// Every live caret, primary first.
    pub fn all_carets(&self) -> impl Iterator<Item = &TextCursor> {
        std::iter::once(&self.cursor).chain(self.extra_carets.iter())
    }

    /// Drop every secondary caret, returning whether any existed.
    pub fn clear_extra_carets(&mut self) -> bool {
        if self.extra_carets.is_empty() {
            return false;
        }
        self.extra_carets.clear();
        true
    }

    /// Drop secondary carets that sit where another caret already is.
    ///
    /// The invariant is "no two carets share a position", and it is
    /// correctness rather than tidiness: two carets stacked on one offset each
    /// insert the character typed, so the user gets `ZZ` for one keypress.
    /// Collisions are not exotic — two carets on the same line both pressing
    /// Home land on the same column, which is a thing people do constantly.
    ///
    /// It lives on the state rather than in the keyboard layer because *every*
    /// mutation path has to hold it, including the ones the keyboard never
    /// touches (a pointer alt-click, a programmatic caret move). A rule
    /// enforced at only some of its call sites is not a rule.
    ///
    /// The primary always survives: it is the caret the accessibility tree
    /// reports and the viewport chases.
    pub fn merge_collided_carets(&mut self) {
        if self.extra_carets.is_empty() {
            return;
        }
        let mut seen = vec![self.cursor.position()];
        self.extra_carets.retain(|c| {
            let p = c.position();
            if seen.contains(&p) {
                false
            } else {
                seen.push(p);
                true
            }
        });
    }

    /// The flow snapshot the accessibility walk reads.
    ///
    /// Uses the document's no-paint variant: the AT tree walks fragments, never
    /// the paint overlay, so computing a paint span per highlight range here
    /// would be pure waste (it dominated the a11y rebuild on a large highlighted
    /// document). A code editor always renders every highlight session, so the
    /// mask is `all` — there is no show/hide-highlights toggle to honour.
    pub fn flow_snapshot_for_a11y(&self) -> bastyde_text::text_document::FlowSnapshot {
        self.document
            .snapshot_flow_masked_no_paint(&bastyde_text::text_document::HighlightMask::all())
    }

    /// Invalidate the cached accessibility snapshot. Called whenever the
    /// document's content or formatting changes: a new run means new synthetic
    /// NodeIds, so a stale snapshot would report the AT tree of the previous
    /// edit.
    pub fn invalidate_accessibility_cache(&self) {
        *self.accessibility_flow_snapshot.borrow_mut() = None;
        self.synthetic_to_element.borrow_mut().clear();
    }

    /// Drain the per-widget document-event queue.
    ///
    /// Returns `(had_events, single_block_position)`. The second is `Some` only
    /// when the batch touched exactly one block and a full layout is not
    /// already pending — the hint that lets the frame loop relayout one block
    /// instead of the document.
    pub fn drain_events(&mut self) -> (bool, Option<usize>) {
        let drained: Vec<DocumentEvent> = {
            let mut q = self.event_queue.lock().expect("event queue mutex poisoned");
            q.drain(..).collect()
        };
        if drained.is_empty() {
            return (false, None);
        }

        let streaming = self.is_streaming();
        let mut single_pos: Option<usize> = None;
        let mut a11y_dirty = false;
        let mut saw_content_change = false;

        for event in drained {
            match event {
                DocumentEvent::ContentsChanged {
                    position,
                    blocks_affected,
                    ..
                } => {
                    self.pending_text_changed = true;
                    saw_content_change = true;
                    a11y_dirty = true;
                    if streaming {
                        // A streaming append never relays out the whole buffer:
                        // the tick re-windows the visible rows instead.
                        self.mark_rewindow();
                    } else if blocks_affected <= 1 && !self.needs_full_layout {
                        single_pos = Some(position);
                    } else {
                        self.needs_full_layout = true;
                        single_pos = None;
                    }
                }
                DocumentEvent::FormatChanged { .. } => {
                    // A format change can alter glyph metrics, so it needs a
                    // reshape — unlike HighlightPaintChanged below.
                    if streaming {
                        self.mark_rewindow();
                    } else {
                        self.needs_full_layout = true;
                        single_pos = None;
                    }
                    a11y_dirty = true;
                }
                DocumentEvent::HighlightPaintChanged { .. } => {
                    // Colour only: the syntax highlighter repainted without the
                    // text changing. Recolour the cached layout rather than
                    // reshaping it — this is the whole reason the event exists,
                    // and it is what makes highlighting a large file viable.
                    self.pending_recolor = true;
                }
                DocumentEvent::UndoRedoChanged { can_undo, can_redo } => {
                    self.pending_undo_redo = Some((can_undo, can_redo));
                }
                DocumentEvent::BlockCountChanged(count) => {
                    // The event carries the count, which is the only affordable
                    // way to know it: `TextDocument::block_count()` advertises
                    // "O(1) — reads cached value" and then fetches every block,
                    // reads each one's content from the rope, and splits it by
                    // whitespace to count words before returning the cached
                    // number it already had. A gutter sizing itself from that
                    // per frame would word-count the document every frame.
                    //
                    // A streaming view owns its own count (the stat undercounts
                    // the document's initial block), so it is not published here.
                    if !streaming {
                        self.line_count.set_if_changed(count);
                    }
                    // The line count changing means lines were added or removed,
                    // which moves every line below them.
                    if streaming {
                        self.mark_rewindow();
                    } else {
                        self.needs_full_layout = true;
                        single_pos = None;
                    }
                    a11y_dirty = true;
                }
                _ => {
                    // Anything structural we do not model precisely: relayout,
                    // or re-window when streaming.
                    if streaming {
                        self.mark_rewindow();
                    } else {
                        self.needs_full_layout = true;
                        single_pos = None;
                    }
                    a11y_dirty = true;
                }
            }
        }

        if a11y_dirty {
            self.invalidate_accessibility_cache();
        }
        // Bump last: the AccessibilityOnly binding on this signal is what flips
        // the tree's a11y_dirty flag, so the cache above must already be clear
        // by the time observers run.
        self.document_version.set(self.document_version.get() + 1);

        if saw_content_change && let Some(cb) = self.on_change.clone() {
            cb();
        }

        (true, single_pos)
    }
}
