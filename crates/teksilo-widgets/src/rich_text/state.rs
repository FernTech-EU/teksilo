// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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

use teksilo_core::Signal;
use teksilo_text::text_document::{
    DocumentEvent, DocumentFragment, HighlightMask, Subscription, TextCursor, TextDocument,
};
use teksilo_text::{CursorAffinity, RichTextEngine, WrapMode};

use super::caret_highlight::CaretHighlightSession;

/// One annotation (a comment thread) covering `[start, end)` of the document, in
/// document-absolute **character** offsets — the space cursors and `FindMatch`
/// speak.
///
/// The framework stays ignorant of what an annotation *is*: the host supplies
/// already-resolved spans and the text to announce, and this only turns them into
/// AccessKit nodes. That keeps a comment feature's anchoring rules — which are
/// application policy — out of the widget.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TextAnnotationSpan {
    pub start: usize,
    pub end: usize,
    /// Durable identity of the annotation, so its synthetic `NodeId` is stable
    /// across rebuilds and a screen reader's cursor is not thrown out of the
    /// thread by an unrelated edit elsewhere.
    pub group_id: u64,
    /// What a screen reader should read: author, body, reply count, state.
    pub summary: String,
}
use super::image_cache::ImageCache;
use super::policy::{CaretPolicy, PolicyBundle};
use crate::common::editor_runtime::{CaretBlink, Debounce};

pub(crate) type SharedState = Rc<RefCell<EditorState>>;

pub(crate) struct EditorState {
    pub document: TextDocument,
    pub engine: RichTextEngine,
    pub cursor: TextCursor,

    pub policy: PolicyBundle,

    // Reactive bridge — cloned into children (scroll bars, selection badges, etc.)
    pub document_version: Signal<u64>,
    /// Bumps **only** on format-only document events
    /// ([`DocumentEvent::FormatChanged`]). Distinct from
    /// `document_version`, which bumps on both content and format
    /// changes — toolbars that want to react to just format changes
    /// (e.g. refresh Bold / Italic button state) observe this signal.
    pub format_version: Signal<u64>,
    /// Bumps once per [`DocumentEvent::LongOperationFinished`]. Starts
    /// at 0; observers see a strictly increasing count as async
    /// `set_html` / `set_markdown` imports complete.
    pub document_loaded_count: Signal<u64>,
    /// Optional user callback fired once per drain batch that contained a
    /// genuine **content edit** ([`DocumentEvent::ContentsChanged`]) and was
    /// not a programmatic load/reset. Set via
    /// [`RichTextEditor::on_change`](super::RichTextEditor::on_change); runs on
    /// the UI thread, so it may touch `Signal`s (e.g. flip a dirty flag).
    pub on_change: Option<Rc<dyn Fn()>>,
    /// Optional user callback fired **at each insertion**, with where the text
    /// came from and how many characters it was. Set via
    /// [`RichTextEditor::on_text_inserted`](super::RichTextEditor::on_text_inserted).
    ///
    /// Deliberately not folded into [`Self::on_change`]: that one fires once per
    /// drain batch and says only *that* something changed, which is the right
    /// shape for a dirty flag and the wrong one for counting. A batch can carry
    /// a typed run and a paste, and after the fact nothing can separate them.
    pub on_text_inserted: Option<Rc<dyn Fn(super::EditSource, usize)>>,
    // NOTE: `report_inserted` below is the only correct way to fire it. Calling
    // the callback directly from an insertion site would fire it for an empty
    // string, which is not text arriving.
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

    // Viewport (the body's bounds at the last layout pass). Written only by
    // [`EditorState::sync_viewport`], which the body calls from BOTH
    // `place_children` (authoritative — layout runs first) and `paint`
    // (idempotent fallback). `viewport_origin` is the **body's** top-left in
    // window coordinates — the engine lays text out from there.
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub viewport_origin: teksilo_canvas::Point,

    // The **wrapper** node's top-left in window coordinates, recorded by
    // the wrapper's `place_children`. Pointer positions now arrive
    // wrapper-node-local (the framework converts once at dispatch), so to
    // reach the body/engine space the handler reconstructs the window
    // point (`position + node_origin`) and subtracts the body origin
    // (`viewport_origin`): `local = position + node_origin -
    // viewport_origin`. The body is inset within the wrapper, so the two
    // origins differ.
    pub node_origin: teksilo_canvas::Point,

    // Layout strategy state.
    pub needs_full_layout: bool,
    pub last_relayout_block_id: Option<usize>,
    pub content_dirty: bool,
    /// True when the most recent layout pass (in `frame_loop::tick`)
    /// ran `layout_full`. Consumed (cleared to false) by
    /// `RichTextEditorBody::paint` to pick `RenderChoice::Full`.
    /// Needed because tick clears `needs_full_layout` after running
    /// the full layout, so paint can't infer it from that flag alone.
    pub pending_full_render: bool,
    /// Set by a `DocumentEvent::HighlightPaintChanged` (paint-only highlight
    /// change). `frame_loop::tick` consumes it: it recolors the cached layout
    /// via `engine.apply_paint_highlights` and forces a re-render, WITHOUT a
    /// reshape/reflow. Distinct from `needs_full_layout` — a paint-only change
    /// never changes glyph metrics.
    pub pending_recolor: bool,

    /// The document extent a pending recolor covers, from
    /// [`DocumentEvent::HighlightPaintChanged`]. `None` means "unknown — the whole document",
    /// which is what the document-wide operations report (installing or retiring a highlighter,
    /// a full rehighlight) and what several accumulated changes collapse to.
    ///
    /// When it *is* known and fits inside one block, `frame_loop::tick` recolors that block
    /// alone instead of re-snapshotting the document — the difference between O(block) and
    /// O(document) on every keystroke that moves a caret band, a find match or a spell squiggle.
    pub pending_recolor_range: Option<(usize, usize)>,

    /// This view's ambient caret band (the sentence or paragraph being written in), when the
    /// host asked for one. `None` — the default — costs nothing: no session is registered on
    /// the document at all.
    pub caret_highlight: Option<CaretHighlightSession>,
    /// Whether the band was last told to draw — focus *and* no selection, as `frame_loop`
    /// computes it. Tracked here so a change is noticed without the focus or selection paths
    /// having to know the band exists.
    pub caret_highlight_active: bool,

    // Wrap mode as configured by the builder.
    pub wrap_mode: WrapMode,

    /// Whether this view applies the document's syntax/search/spell
    /// highlights. When `false` the view pulls a *clean* snapshot
    /// (no highlights at all) and ignores `HighlightPaintChanged`, so a
    /// read-only preview can mirror the same shared `TextDocument` while
    /// staying bare of authoring-time highlighting. The single source of
    /// truth — `frame_loop`/`drain_events` read it and pass it to the
    /// engine's per-block relayout. Default `true`; `read_only` defaults
    /// it to `false` (override either way via `RichTextEditor::show_highlights`).
    pub show_highlights: bool,
    /// Annotation bodies (comment threads) covering ranges of this document, for
    /// the accessibility tree only.
    ///
    /// Deliberately separate from the highlight sessions that *paint* them: paint
    /// says "something is here", while this says what it is and lets a screen
    /// reader navigate into it. A sighted user gets the underline; an AT user gets
    /// `aria-details` to a `Role::Comment` node. Neither is derivable from the
    /// other — a highlight carries no text, and this carries no colour.
    pub annotation_spans: Vec<TextAnnotationSpan>,

    /// Window the render to the accumulated ancestor clip instead of this
    /// widget's own bounds. `false` by default: a normal self-scrolling editor
    /// culls correctly from its own `scroll_y`. An editor laid out at full
    /// document height inside an outer `ScrollArea` ("dubious mode") sets this
    /// `true` so paint-time culling follows the visible clip band rather than
    /// the whole-document viewport. Read in `paint()`; drives
    /// `engine.set_render_window`. See
    /// [`RichTextEditor::window_to_clip`](crate::rich_text::RichTextEditor::window_to_clip).
    pub window_to_clip: bool,
    /// Whether this editor guesses its height from its text before anything has
    /// laid it out. See
    /// [`RichTextEditor::estimate_height_before_layout`](crate::rich_text::RichTextEditor::estimate_height_before_layout).
    pub estimate_height_before_layout: bool,
    /// The widest width this body has ever been asked to measure at.
    ///
    /// Only read by the height guess, and only until a real layout exists.
    ///
    /// A measurement pass may carry any width, and one of them carries a width that
    /// is not a measure at all. `linear_layout::negotiate` opens with an **intrinsic
    /// probe** — every child asked at `width: None`, meaning "how big do you want to
    /// be". Skribisto's writing column resolves that `None` to `0.0`, floors it at
    /// its own `MIN_COLUMN_WIDTH` of 100, and its editor's 12 px content padding
    /// takes 24 off: **76**, every time, for a column whose text wraps at 447. A
    /// guess made against it claimed six times the true height.
    ///
    /// By the time the number arrives here it is an ordinary `Some(76.0)` and
    /// nothing distinguishes it from a genuinely narrow placement — the intent is
    /// destroyed two layers up. The widest is then the honest predictor: 76 is the
    /// floor of that whole chain, so any real measurement beats it, and a narrow
    /// proposal is a minimum-size question while the layout that follows uses the
    /// generous one.
    ///
    /// ⚠ It only grows. An editor that is measured wide, never laid out, and then
    /// **permanently** narrowed — the writer opens the Inspector on a Full Book —
    /// keeps guessing against the stale width until it is scrolled to. Bounded in
    /// practice because a stream's rows are short-lived, and the failure is one
    /// under-estimate rather than the sixfold over-estimate it replaces.
    pub widest_measured_width: f32,

    /// Which highlight sessions THIS view renders (`show_highlights` is the master switch
    /// above it: `false` suppresses everything regardless of this mask). Default is
    /// [`HighlightMask::all`] — every session on the document. A per-editor find banner sets
    /// this to a narrower set so two panes over one shared document can highlight different
    /// queries. See [`Self::effective_mask`].
    pub highlight_mask: HighlightMask,

    /// `true` once the app explicitly set a text color via
    /// Last text color applied to the typesetter. Tracked so a theme
    /// swap (light ↔ dark) can force a full re-render — without it
    /// paint() would happily call `engine.with_render_cursor_only`,
    /// which reuses the cached glyph quads with their old colors baked
    /// in, leaving the visible text unchanged until the next typing /
    /// scroll event triggered a Full or Block render.
    pub last_text_color: Option<[f32; 4]>,

    /// Whether this editor follows the global accessibility text scale
    /// (`ctx.text_scale`). `true` by default; set `false` via
    /// [`RichTextEditor::follow_text_scale`](crate::rich_text::RichTextEditor::follow_text_scale)
    /// for documents whose font sizes are
    /// content (e.g. a WYSIWYG editor) that should not inflate with the UI
    /// accessibility setting.
    pub follow_text_scale: bool,
    /// Per-editor logical font-size multiplier (`1.0` = 100 %), composed with
    /// the a11y text scale at paint:
    /// `engine.font_scale = (follow ? ctx.text_scale : 1.0) × font_size_scale`.
    /// Sharp "text size" — real shaping at a larger ppem. Default `1.0`.
    pub font_size_scale: f32,
    /// Last `font_scale` pushed to the engine. Tracked so the paint pass only
    /// re-sets it (and forces a relayout) when the effective scale changes.
    pub last_font_scale: f32,

    /// Last caret colour applied to the typesetter. The paint pass syncs
    /// the engine's cursor colour with the active theme's `editor_caret`
    /// role each frame so light / dark theme swaps reach the blinking
    /// caret (the engine defaults it to opaque black). Tracked so a theme
    /// swap forces a render this frame instead of waiting for the next
    /// blink toggle to repaint the caret in the new colour.
    pub last_cursor_color: Option<[f32; 4]>,

    /// Last selection-highlight colour applied to the engine. Tracked (like
    /// the caret) so an app-set `selection_color` change forces a render this
    /// frame. `None` until the app sets a colour.
    pub last_selection_color: Option<[f32; 4]>,

    /// App-set colour overrides (`impl Into<ColorProp>` — Color / theme role /
    /// Signal), resolved against the active theme on each paint. `None` tracks
    /// the theme's editor roles. Set by the `RichTextEditor` builders, read by
    /// `RichTextEditorBody::paint`; `background_prop` is consumed by
    /// `RichTextEditor::build` (threaded into the style's `make_body`).
    pub text_color_prop: Option<teksilo_core::color_prop::ColorProp>,
    pub caret_color_prop: Option<teksilo_core::color_prop::ColorProp>,
    pub selection_color_prop: Option<teksilo_core::color_prop::ColorProp>,
    pub background_prop: Option<teksilo_core::color_prop::ColorProp>,

    /// Last code-block background colour applied to the engine. A
    /// change forces a full `layout_full` (not just a render) because
    /// the converted `BlockLayoutParams.background_color` is baked in
    /// at layout time, not at render time.
    pub last_code_block_bg: Option<[f32; 4]>,
    /// Last code-block foreground colour applied to the engine. Same
    /// rationale as `last_code_block_bg` — fragment foregrounds are
    /// baked into the layout's shaped runs.
    pub last_code_block_fg: Option<[f32; 4]>,
    /// Last link foreground pushed to the engine, so a theme swap can be
    /// told from a no-op repaint. Same reason as `last_code_block_fg`: the
    /// colour is baked in at layout time.
    pub last_link_fg: Option<[f32; 4]>,

    // Focus — mirrored from `on_focus` so paint can gate the caret.
    pub has_focus: bool,

    /// A drag is hovering this editor, and the caret is showing where it would
    /// land.
    ///
    /// The ordinary caret is gated on focus, which a drag never gives the
    /// editor it is hovering — the focus stays wherever the drag began, often
    /// in a different editor entirely. So the one caret the writer actually
    /// needs to see, the one promising where the text will land, is exactly the
    /// one the focus gate hides. This overrides it for as long as the drag is
    /// overhead, and it does not blink: a drop target that flashes on and off
    /// reads as uncertainty about whether it will accept.
    pub drop_caret: bool,

    /// When `true` (**the default**), moving the caret reveals it inside any
    /// *enclosing* scroll area (via `EventContext::ensure_visible`) — the
    /// standard editor "caret stays on screen while you type / navigate"
    /// behaviour. It fires only on a caret *move*, never on a plain wheel /
    /// scrollbar scroll, so the reader can still scroll freely away from the
    /// caret and the view stays put until the caret next moves.
    ///
    /// This matters most for a document editor that **grows** to its content
    /// with its own scroll suppressed (a flowing page inside an outer
    /// `ScrollArea`): there the editor's *internal* caret-visibility is a no-op
    /// (it shows all its content), so the enclosing page-follow is the only
    /// thing that keeps the caret visible. Set
    /// [`RichTextEditor::follow_caret_in_page(false)`](crate::rich_text::RichTextEditor::follow_caret_in_page)
    /// for the rare case where the surrounding page must never move on a caret
    /// change. See `chase_caret_into_view`.
    pub follow_caret_in_page: bool,

    /// Whether the host window is currently active (`focused AND not
    /// occluded`). Mirrored from `BuildContext::window_active_signal` by an
    /// effect in `RichTextEditor::build` (the frame-loop `tick` has no context,
    /// so it can't observe the signal itself). Gates the caret alongside
    /// `has_focus`: the caret is hidden whenever the window is inactive, the
    /// universal desktop convention. Starts `true` to match the tree's initial
    /// window-active value.
    pub window_active: bool,

    /// Reactive mirror of `has_focus`, kept in lockstep by the
    /// `on_focus` handler. Exposed so the composing
    /// `RichTextEditor` shell can pass it into
    /// `RichTextEditorStyle::make_body` and drive a focus-aware
    /// border without polling.
    pub focus_signal: Signal<bool>,

    /// The wrapper widget's own id, stashed on every build so a held
    /// [`EditorHandle`](super::EditorHandle) can move keyboard focus back to the
    /// editor (e.g. a find banner returning focus to the prose on Escape). The
    /// wrapper is the `.focusable(true)` node, so `request_focus` on it lands
    /// exactly where a click would.
    pub self_id: Option<teksilo_core::widget_id::WidgetId>,

    /// Sticky preferred X for vertical navigation. Set
    /// the first time Up/Down/PageUp/PageDown is pressed, preserved
    /// across further vertical presses so the cursor keeps trying to
    /// land on the same visual column even when crossing short
    /// lines. Cleared on any horizontal or edit action.
    pub preferred_x: Option<f32>,

    /// Which side of a soft-wrap boundary the caret renders at. Only
    /// has an effect when `cursor.position()` happens to be a wrap
    /// boundary (the same character offset appears at the end of one
    /// display line and the start of the next). Default is
    /// `Downstream`, which matches the pre-affinity behavior:
    /// end-of-previous-line placement. Mouse clicks set it from
    /// `HitTestResult::affinity`; vertical navigation
    /// (Up/Down/PageUp/PageDown/Home/End) re-derives it via the
    /// typesetter's hit-test after the move; edits, Left/Right, and
    /// programmatic cursor mutations reset to `Downstream`.
    ///
    /// Stored on `EditorState` rather than on `TextCursor` because
    /// affinity is a display concern that requires the layout engine
    /// to interpret — see `docs/architecture.md` / the design rationale
    /// in the commit message that introduced this field.
    pub cursor_affinity: CursorAffinity,

    /// Caret blink phase. Wall-clock driven, so the visible rhythm stays
    /// locked to real seconds no matter how the frame scheduler behaves.
    /// Shared with the other text surfaces — see
    /// [`common::editor_runtime::CaretBlink`](crate::common::editor_runtime::CaretBlink).
    pub blink: CaretBlink,

    /// Shared handle into `WidgetTree::frame_tick_requested`. Stashed
    /// here so the frame-tick effect can chain-request another tick
    /// (blink, drag auto-scroll) without needing mutable access to
    /// the tree.
    pub frame_request: Option<Rc<std::cell::Cell<bool>>>,

    /// Shared handle into `WidgetTree::pending_wake_at`. Used by the
    /// caret blink path to schedule a one-shot 500 ms wake-up instead
    /// of keeping the frame loop pumping at the OS's max rate.
    pub frame_wake_at: Option<Rc<std::cell::Cell<Option<std::time::Instant>>>>,

    // Shared-document event routing: each editor subscribes via
    // `on_change` and buffers events in its own queue. The
    // `_event_subscription` field is kept alive by the state so
    // dropping the state unregisters the callback.
    pub event_queue: Arc<Mutex<VecDeque<DocumentEvent>>>,
    pub _event_subscription: Subscription,

    // Resource caches.
    pub image_cache: ImageCache,

    // --- M8b editor preset state (unused by read-only preset) ----------
    /// Accumulates typed characters within a single frame, flushed as
    /// one `cursor.insert_text(batch)` at the start of the next
    /// `frame_loop::tick`. Batching matches the godot reference, and collapses
    /// a burst of keystrokes into a single `ContentsChanged` event so
    /// incremental relayout and debounced `text_changed` emission stay
    /// O(burst) instead of O(keystrokes).
    pub pending_chars: String,
    /// How many of [`Self::pending_chars`] were typed, and how many were the
    /// settled result of an IME composition.
    ///
    /// Two counters rather than one label on the batch, because both routes push
    /// into the same string and the frame loop flushes it as a unit. A single
    /// label would have to pick one for a mixed batch — and while mixing is
    /// vanishingly unlikely (an active IME swallows the raw keys), a count that
    /// is exact costs two `usize`s and never has to be reasoned about again.
    ///
    /// Reset with the batch. See [`Self::report_inserted_chars`].
    pub pending_typed_chars: usize,
    pub pending_ime_chars: usize,

    /// Active IME preedit text — the unfinalised string the input
    /// method renders while the user is composing (CJK, Korean,
    /// dead-key accents on Linux). `Some(text)` means there is a
    /// tentative insert at `ime_preedit_range`; empty text + `Some`
    /// means the composition was cancelled but the old range still
    /// needs clearing. `None` means no active composition.
    pub ime_preedit: Option<String>,
    /// Character range (scalar-indexed, matching
    /// `TextCursor::position`) of the tentative preedit insert. The
    /// composition handler removes this range before inserting the
    /// next preedit string so the document always reflects the
    /// current IME state.
    pub ime_preedit_range: Option<std::ops::Range<usize>>,

    /// Document position (scalar-indexed caret offset) of the most recent
    /// [`chase_caret_into_view`](super::keyboard::chase_caret_into_view). The
    /// page-follow chase reveals the caret only when it actually *moves*: a
    /// repeat call at the same position (IME preedit churn on Linux, a no-op
    /// nav key, a redundant click) is skipped so it can't yank the page back
    /// after the user has deliberately scrolled the caret off-screen.
    pub last_chase_pos: Option<usize>,

    /// Window-space `y` of the caret at the most recent **pinned** chase, used
    /// to extend the `last_chase_pos` dedup while typewriter scrolling is on.
    ///
    /// Position alone is not enough for a pin: a reflow — a soft-wrap change, a
    /// typography or zoom change, a window resize — moves the caret's *rect*
    /// while its document offset stands still, and a pin that skipped those
    /// would silently drift off its line and stay there.
    pub last_chase_y: Option<f32>,

    /// Typewriter scrolling: where to pin the caret line in the enclosing scroll
    /// area, as a fraction of the viewport height (`0.5` = centred). `None`
    /// (default) leaves the plain minimal-reveal follow in charge. Set from
    /// [`RichTextEditor::typewriter`](crate::rich_text::RichTextEditor::typewriter).
    pub typewriter: Option<f32>,

    /// Whether the caret was last placed by the **pointer**. While set, the
    /// typewriter pin stands down and the click position becomes the new
    /// resting place; the next keystroke clears it and pinning resumes.
    ///
    /// Every well-regarded typewriter implementation converges on this rule
    /// (Ulysses' "Variable", the CodeMirror plugins' `movedByMouse`, Sublime's
    /// trigger list, VS Code's `cursorSurroundingLinesStyle`), and the editors
    /// that omit it — Typora, Zettlr — carry open bugs about the view fighting
    /// the mouse and about drag-selection becoming unusable.
    pub mouse_anchored: bool,

    /// Last IME candidate-window rectangle reported to the platform via
    /// [`report_ime_cursor_area`](super::keyboard::report_ime_cursor_area).
    /// Reporting is deduped against this: re-sending an unchanged area is not
    /// only wasted work but, on some winit IME backends (ibus/fcitx), echoes
    /// back a fresh empty `Ime::Preedit` — a self-sustaining feedback loop.
    pub last_ime_area: Option<teksilo_canvas::Rect>,

    /// Coalescing window for `text_changed` / `format_changed` /
    /// `undo_redo_changed`. Starts already-expired so the first frame
    /// publishes `can_undo`/`can_redo` without a 150 ms wait. Shared with the
    /// other text surfaces — see
    /// [`common::editor_runtime::Debounce`](crate::common::editor_runtime::Debounce).
    pub debounce: Debounce,

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
    /// Ctrl+X. On paste the HTML payload is inspected for
    /// `rich_clipboard_marker`; on a match the fragment is reinserted
    /// to preserve formatting, otherwise the clipboard's own payload is
    /// parsed. Plain-text equality alone is not sufficient because two
    /// different apps can publish identical plain text with different
    /// formatting; the embedded marker disambiguates.
    pub rich_clipboard_fragment: Option<DocumentFragment>,
    /// Plain-text form of the same copy, and the fallback identity check on a
    /// clipboard backend that could not carry the marker.
    ///
    /// Set **only** when the copy found no HTML payload on the clipboard
    /// afterwards, which is how a backend inheriting the default `set_html`
    /// body announces itself. On a backend that does carry HTML this stays
    /// `None`, because there the marker is the identity and text equality is
    /// ambiguous: another application can publish the same words with different
    /// formatting, and matching on them would paste this editor's stale
    /// formatting onto somebody else's text.
    pub rich_clipboard_plain: Option<String>,
    /// Opaque token embedded as an HTML comment in the clipboard HTML
    /// payload of the most recent copy/cut. Regenerated on every copy,
    /// so stale markers from a previous session or a cleared state
    /// never match.
    pub rich_clipboard_marker: Option<String>,

    /// Ctrl+A escalation ladder position. See `keyboard.rs`: when the
    /// caret is inside a table cell the ladder climbs through 4 levels
    /// (paragraph → cell → table → document); outside a table it is a
    /// single-shot `SelectionType::Document` and stays at 0. Reset to 0
    /// by any non-SelectAll key action (matching the godot reference).
    pub select_all_level: u8,

    /// Cached flow snapshot used by the accessibility pass. The
    /// `Widget::accessibility` walk iterates blocks and fragments
    /// to emit AccessKit `Role::Paragraph` / `Role::TextRun`
    /// children; the snapshot itself doesn't change between
    /// rebuilds triggered by focus / resize, so caching it avoids
    /// re-walking the document tree. Invalidated from
    /// `drain_events` when a `ContentsChanged` or `FormatChanged`
    /// event arrives.
    pub accessibility_flow_snapshot: RefCell<Option<teksilo_text::text_document::FlowSnapshot>>,

    /// Per-synthetic-NodeId lookup table populated during the
    /// accessibility walk. Maps each emitted `Role::TextRun` NodeId
    /// to its text-document element_id, absolute-document
    /// character start, and run text. Used by the
    /// `on_access_action_request` handler to convert AccessKit
    /// `SetTextSelection` requests (which reference TextRun NodeIds
    /// and in-run character indices) back into document-absolute
    /// cursor positions.
    pub synthetic_to_element:
        RefCell<std::collections::HashMap<teksilo_core::accesskit::NodeId, SyntheticElementRef>>,

    /// Callback invoked on a Primary-click whose hit lands on a
    /// `HitRegion::Link`. Installed via
    /// [`RichTextEditor::on_link_activated`](super::RichTextEditor::on_link_activated).
    /// `Rc` rather than `Box` so the mouse handler can clone it out
    /// of the state borrow to invoke — running the callback itself
    /// with `state.borrow()` held would deadlock if the handler calls
    /// back into the widget's API.
    pub on_link_activated:
        Option<std::rc::Rc<dyn Fn(&str, &mut teksilo_core::widget::EventContext)>>,
    /// Asked for an image's bytes when the document has no resource under that
    /// name — see [`super::RichTextEditor::on_image_missing`].
    pub image_resolver: Option<super::image_cache::ImageResolver>,
    /// Where the selected image was last painted, so a press can tell whether it
    /// landed on one of its handles. A `RefCell` because the paint pass writes
    /// it while other fields of this struct are mutably borrowed beside it, and
    /// the value is not `Copy` (it names the picture).
    pub selected_image: RefCell<Option<SelectedImageRect>>,
    /// The rect a resize drag is currently proposing, drawn as an outline. The
    /// document is left alone until the pointer is released: relaying out the
    /// whole block on every pointer move would make a drag on a long scene
    /// stutter, for a preview an outline shows just as well.
    pub resize_preview: std::cell::Cell<Option<[f32; 4]>>,
    /// Called with the paths of files dropped on the editor — see
    /// [`super::RichTextEditor::on_files_dropped`].
    pub on_files_dropped:
        Option<std::rc::Rc<dyn Fn(&[std::path::PathBuf], &mut teksilo_core::widget::EventContext)>>,
    /// Called when a resize drag ends — see
    /// [`super::RichTextEditor::on_image_resized`].
    pub on_image_resized:
        Option<std::rc::Rc<dyn Fn(&super::ImageResize, &mut teksilo_core::widget::EventContext)>>,
    /// Callback invoked on a Primary-click whose hit lands on a
    /// `HitRegion::Image`. Same Rc / borrow-release convention as
    /// [`on_link_activated`](Self::on_link_activated).
    pub on_image_activated: Option<
        std::rc::Rc<dyn Fn(&super::ImageActivation, &mut teksilo_core::widget::EventContext)>,
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
// No longer `Copy`: `ResizingImage` names the picture it is resizing, and the
// release has to report that name. Every reader clones or matches by reference.
#[derive(Debug, Clone, PartialEq)]
pub enum DragState {
    Idle,
    Selecting {
        /// Per-second scroll velocity requested by the near-edge
        /// auto-scroll ramp. Applied by the frame loop on every tick.
        auto_scroll_v_per_s: f32,
    },
    /// A press landed inside the existing selection.
    ///
    /// Whether that press is a click (which collapses the selection onto it)
    /// or the beginning of a drag of the selected text cannot be known until
    /// the pointer either moves past the threshold or is released — so the
    /// selection is left standing until it says which. Collapsing eagerly on
    /// press is what makes a selection impossible to pick up: the text is gone
    /// from the selection before the drag can carry it.
    PendingTextDrag {
        /// Press position in widget coordinates, for the movement threshold.
        origin: [f32; 2],
    },
    /// Dragging a corner handle of the selected inline image.
    ///
    /// Deliberately not a variant of `Selecting`: the two share a pointer
    /// gesture and nothing else. A resize never moves the caret, never
    /// auto-scrolls, and ends by reporting a size rather than by leaving a
    /// selection behind.
    ResizingImage {
        /// The image being resized, so the release can name it.
        name: String,
        /// Its `U+FFFC`'s document offset — the identity, since a document may
        /// hold one picture in several places.
        offset: usize,
        /// The image's rect when the drag began, in engine-local coordinates.
        /// Every frame's new size is derived from this rather than from the
        /// previous frame's, so rounding cannot accumulate over a long drag.
        origin: [f32; 4],
        /// The corner that was grabbed, as `(x, y)` unit multipliers: `(0, 0)`
        /// is top-left, `(1, 1)` bottom-right. The opposite corner is the one
        /// that stays put while the pointer moves.
        corner: (f32, f32),
    },
}

/// The selected inline image's on-screen rect, recorded by the paint pass.
///
/// The pointer handler needs the picture's geometry to know whether a press
/// landed on a resize handle — and a handle sits *outside* the image, so the
/// engine's own hit-test cannot answer it (it reports `HitRegion::Image` only
/// within the picture). The paint pass is the one place that already has both
/// the rect and the selection, so it writes what it saw.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectedImageRect {
    /// The image's resource name, so a resize can report which picture it was.
    pub name: String,
    /// `[x, y, width, height]` in engine-local coordinates — the same space
    /// `to_engine_local` produces, so a pointer position compares directly.
    pub rect: [f32; 4],
    /// Document offset of the image's `U+FFFC`.
    pub offset: usize,
}

impl EditorState {
    /// Tell whoever is listening that `text` just arrived through `source`.
    ///
    /// **Call this beside the insertion, with the string that was inserted.**
    /// Not afterwards from a position delta: an insertion that replaces a
    /// selection moves the caret by a different number than it wrote, and a
    /// consumer counting characters wants the second.
    ///
    /// Empty insertions report nothing — there is no such thing as zero
    /// characters arriving, and a consumer would have to filter them out again.
    pub fn report_inserted(&self, source: super::EditSource, text: &str) {
        self.report_inserted_chars(source, text.chars().count());
    }

    /// As [`Self::report_inserted`], for a caller that counted as it went.
    ///
    /// Zero reports nothing — there is no such thing as zero characters
    /// arriving, and a consumer would only have to filter it out again.
    pub fn report_inserted_chars(&self, source: super::EditSource, chars: usize) {
        if chars == 0 {
            return;
        }
        if let Some(callback) = self.on_text_inserted.as_ref() {
            callback(source, chars);
        }
    }

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
            annotation_spans: Vec::new(),
            document_version: Signal::new(0),
            format_version: Signal::new(0),
            document_loaded_count: Signal::new(0),
            on_change: None,
            on_text_inserted: None,
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
            viewport_origin: teksilo_canvas::Point::ZERO,
            node_origin: teksilo_canvas::Point::ZERO,
            needs_full_layout: true,
            last_relayout_block_id: None,
            content_dirty: true,
            pending_full_render: true,
            pending_recolor: false,
            pending_recolor_range: None,
            caret_highlight: None,
            caret_highlight_active: false,
            wrap_mode,
            show_highlights: true,
            window_to_clip: false,
            estimate_height_before_layout: false,
            widest_measured_width: 0.0,
            highlight_mask: HighlightMask::all(),
            last_text_color: None,
            follow_text_scale: true,
            font_size_scale: 1.0,
            last_font_scale: 1.0,
            last_cursor_color: None,
            last_selection_color: None,
            text_color_prop: None,
            caret_color_prop: None,
            selection_color_prop: None,
            background_prop: None,
            last_code_block_bg: None,
            last_code_block_fg: None,
            last_link_fg: None,
            has_focus: false,
            drop_caret: false,
            follow_caret_in_page: true,
            window_active: true,
            focus_signal: Signal::new(false),
            self_id: None,
            event_queue,
            _event_subscription: subscription,
            image_cache: ImageCache::new(),
            preferred_x: None,
            cursor_affinity: CursorAffinity::default(),
            blink: CaretBlink::new(),
            frame_request: None,
            frame_wake_at: None,
            pending_chars: String::new(),
            pending_typed_chars: 0,
            pending_ime_chars: 0,
            ime_preedit: None,
            ime_preedit_range: None,
            last_chase_pos: None,
            last_chase_y: None,
            typewriter: None,
            mouse_anchored: false,
            last_ime_area: None,
            // `Debounce::new` starts already-expired so the first tick
            // flushes initial state instead of waiting out a window.
            debounce: Debounce::new(),
            pending_text_changed: false,
            pending_format_changed: false,
            pending_undo_redo: None,
            drag_state: DragState::Idle,
            rich_clipboard_fragment: None,
            rich_clipboard_plain: None,
            rich_clipboard_marker: None,
            on_link_activated: None,
            image_resolver: None,
            selected_image: RefCell::new(None),
            resize_preview: std::cell::Cell::new(None),
            on_files_dropped: None,
            on_image_resized: None,
            on_image_activated: None,
            select_all_level: 0,
            select_all_anchor_cell: None,
            accessibility_flow_snapshot: RefCell::new(None),
            synthetic_to_element: RefCell::new(std::collections::HashMap::new()),
        }))
    }

    /// Adopt `bounds` as the body's viewport — the single writer of
    /// `viewport_origin` / `viewport_width` / `viewport_height`.
    ///
    /// Called from BOTH `RichTextEditorBody::place_children` (the authority —
    /// layout runs before paint, so the engine is sized before the first
    /// `layout_full` ever runs) and `RichTextEditorBody::paint` (an idempotent
    /// echo, for any path that paints without a preceding layout). Calling it
    /// twice in a frame is safe: the second call sees no change and does nothing.
    ///
    /// **The four side effects must stay welded together.** `viewport_width` /
    /// `viewport_height` are themselves the change detector, so a caller that
    /// writes them *without* also pushing `engine.set_viewport` +
    /// `needs_full_layout` blinds every later caller: the engine then runs its
    /// first `layout_full` against an uninitialised viewport, wraps the text at a
    /// degenerate width, and — because `needs_full_layout` is cleared afterwards
    /// — keeps that broken layout forever. Keep the write and its consequences in
    /// this one place.
    ///
    /// Returns `true` if the viewport size actually changed.
    pub fn sync_viewport(&mut self, bounds: teksilo_canvas::Rect) -> bool {
        self.viewport_origin = teksilo_canvas::Point::new(bounds.x, bounds.y);
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

    /// The sessions this view actually renders: its [`highlight_mask`](Self::highlight_mask),
    /// or nothing at all when `show_highlights` is off (the master switch a bare preview flips
    /// to stay clean).
    pub fn effective_mask(&self) -> HighlightMask {
        if self.show_highlights {
            self.highlight_mask.clone()
        } else {
            HighlightMask::none()
        }
    }

    /// Engine `font_scale` for this frame: a11y text scale (if followed) ×
    /// per-editor [`font_size_scale`](Self::font_size_scale). Clamped to the
    /// same band as [`teksilo_text::RichTextEngine::set_font_scale`].
    pub fn effective_font_scale(&self, text_scale: f32) -> f32 {
        let a11y = if self.follow_text_scale {
            text_scale
        } else {
            1.0
        };
        (a11y * self.font_size_scale).clamp(0.1, 10.0)
    }

    /// Snapshot the document's flow in this view's highlight flavor — only the sessions
    /// [`effective_mask`](Self::effective_mask) admits. Every full-layout / a11y snapshot pull
    /// routes through here, so a bare view never observes the document's highlighting and two
    /// panes over one document can differ. (A11y correctness rides on the fact that paint-only
    /// sessions — find, spell — never touch the text fragments the AT tree reads; only
    /// metric-affecting sessions, e.g. syntax bold, reach it.)
    pub fn flow_snapshot(&self) -> teksilo_text::text_document::FlowSnapshot {
        self.document.snapshot_flow_masked(&self.effective_mask())
    }

    /// The snapshot the accessibility tree is built from — same masked flavor as
    /// [`flow_snapshot`](Self::flow_snapshot), but without the paint-only overlay
    /// (`paint_highlights`). The AT walk reads the fragments and their geometry
    /// and never the overlay, so this is byte-identical for its purposes while
    /// skipping the per-block `extract_paint_spans` work — the dominant cost of
    /// rebuilding the a11y tree over a document carrying a spell-checker's tens
    /// of thousands of ranges. (Metric sessions still split fragments here, so a
    /// syntax-bold run is reported to the reader exactly as before.)
    pub fn flow_snapshot_for_a11y(&self) -> teksilo_text::text_document::FlowSnapshot {
        self.document
            .snapshot_flow_masked_no_paint(&self.effective_mask())
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
        let mut saw_format_change = false;
        let mut document_loaded_pulses = 0_u64;
        // Track genuine user edits vs programmatic loads/resets, so the
        // user `on_change` callback fires only when the *content* was edited
        // (not when `set_djot`/`set_markdown` repopulates the document).
        let mut saw_content_change = false;
        let mut saw_reset_or_load = false;
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
                    saw_content_change = true;
                    if blocks_affected <= 1 && !self.needs_full_layout {
                        single_pos = Some(position);
                    } else {
                        self.needs_full_layout = true;
                        single_pos = None;
                    }
                }
                DocumentEvent::FormatChanged { .. } => {
                    self.pending_format_changed = true;
                    saw_format_change = true;
                    a11y_snapshot_dirty = true;
                    self.needs_full_layout = true;
                    single_pos = None;
                }
                DocumentEvent::HighlightPaintChanged { position, length } => {
                    // A view with highlights off never shows paint highlights,
                    // so this event is a pure no-op for it: don't recolor, don't
                    // even dirty the AT snapshot (its clean snapshot is
                    // unaffected). This is the "zero work on a search keystroke"
                    // win for the bare preview pane.
                    if self.show_highlights {
                        // Paint-only highlight change: the shaping input is
                        // unchanged, so recolor the cached layout without a
                        // reshape/reflow. `tick` consumes `pending_recolor`.
                        //
                        // Accumulate BEFORE setting the flag: `pending_recolor` is what
                        // distinguishes "first change this frame" (adopt its extent) from
                        // "widen what is already accumulated".
                        self.pending_recolor_range = accumulate_recolor_range(
                            self.pending_recolor,
                            self.pending_recolor_range,
                            position,
                            length,
                        );
                        self.pending_recolor = true;
                        // Colors on TextRun nodes changed, so the AT snapshot is
                        // stale — but node identity is unaffected, so keep the
                        // synthetic→element id map (only invalidate the snapshot).
                        a11y_snapshot_dirty = true;
                        // Deliberately NOT setting needs_full_layout /
                        // pending_format_changed / pending_text_changed.
                    }
                }
                // A programmatic repopulation (`set_plain_text` / `clear` /
                // `set_djot` / `set_markdown` / `set_html`) is the ONLY thing
                // that queues `DocumentReset` — text-document emits it from
                // exactly three explicit sites, and never from an edit path.
                // So it, alone, is the reliable "this was a load, stay quiet"
                // signal for `on_change`.
                DocumentEvent::DocumentReset => {
                    self.pending_text_changed = true;
                    a11y_snapshot_dirty = true;
                    saw_reset_or_load = true;
                    self.needs_full_layout = true;
                    single_pos = None;
                }
                // Structural edits. These are emitted by text-document's
                // GENERIC post-mutation detectors (`check_block_count_changed`
                // / `check_flow_changed`), so they fire for genuine user edits
                // — pressing Enter, a backspace that merges two paragraphs, a
                // multi-paragraph paste, an AT `SetValue` — and must count as
                // content changes. Lumping them in with `DocumentReset` (they
                // once were) silently suppressed `on_change` for every edit
                // that changed the block count.
                //
                // A load stays suppressed regardless: it queues `DocumentReset`
                // in the SAME batch as any `BlockCountChanged` it triggers (and
                // emits no `FlowElements*` at all, because the reset paths call
                // `reset_cached_child_order`, which resyncs silently).
                DocumentEvent::FlowElementsInserted { .. }
                | DocumentEvent::FlowElementsRemoved { .. }
                | DocumentEvent::BlockCountChanged(_) => {
                    self.pending_text_changed = true;
                    a11y_snapshot_dirty = true;
                    saw_content_change = true;
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
                DocumentEvent::LongOperationFinished { .. } => {
                    document_loaded_pulses += 1;
                }
                // `TextInserted` is attribution, not layout: it says which
                // channel some text arrived through, alongside the
                // `ContentsChanged` that already told this state everything it
                // needs. This widget reports arrivals through its own
                // [`EditSource`](crate::rich_text::EditSource) callback, which
                // knows the channel at the point of the keystroke rather than
                // inferring it from a document event.
                DocumentEvent::TextInserted { .. }
                | DocumentEvent::ModificationChanged(_)
                | DocumentEvent::LongOperationProgress { .. } => {}
            }
        }

        // Bump format_version once per batch if any event in the batch
        // was a FormatChanged. Multiple FormatChanged events in the
        // same frame collapse into a single pulse — observers see the
        // batched count, not per-event fires, which matches how the
        // paint pass already batches work.
        if saw_format_change {
            self.format_version
                .set(self.format_version.get().wrapping_add(1));
        }
        // Document-loaded pulses accumulate: a batch with two
        // LongOperationFinished events bumps by 2 so observers can
        // count imports correctly (rare but possible if two async
        // loads finish in the same tick).
        if document_loaded_pulses > 0 {
            self.document_loaded_count.set(
                self.document_loaded_count
                    .get()
                    .wrapping_add(document_loaded_pulses),
            );
            saw_reset_or_load = true;
        }

        if had_events {
            self.content_dirty = true;
            self.document_version
                .set(self.document_version.get().wrapping_add(1));
        }

        // Fire the user edit callback only for genuine user edits — not for
        // a programmatic load/reset (`set_djot`/`set_markdown` repopulate), and
        // not while an IME composition is still in progress: each intermediate
        // preedit keystroke (every CJK/Kana candidate change) mutates the
        // document through this same `ContentsChanged` path, but it is not yet
        // a settled edit. `ime_preedit` is `None` once the composition either
        // commits (`clear_ime_preedit` runs before the commit's own insert) or
        // is cancelled to empty, so gating on it fires `on_change` exactly once
        // for the final, real result.
        //
        // **A formatting change counts.** It was omitted for as long as this
        // callback existed, and the omission was not visible from here: a host
        // typically wires `on_change` to "the document has unsaved changes", so
        // bolding a word — or linking one, or setting a heading — left the app
        // believing nothing had happened. No autosave was scheduled and no
        // close guard fired, and the edit survived only if the writer happened
        // to type something afterwards. `FormatChanged` is as much the writer's
        // work as a keystroke is.
        //
        // A load is still suppressed, and by the same guard rather than a new
        // one: `DocumentReset` lands in the same drained batch as any
        // formatting the load applies, so `saw_reset_or_load` covers this arm
        // exactly as it already covered content.
        if (saw_content_change || saw_format_change)
            && !saw_reset_or_load
            && self.ime_preedit.is_none()
            && let Some(cb) = self.on_change.clone()
        {
            cb();
        }

        // Drop the cached flow snapshot and synthetic-id lookup
        // whenever the document structure / content / formatting
        // changed. The next accessibility walk rebuilds both
        // lazily from a fresh `document.snapshot_flow()`.
        if a11y_snapshot_dirty {
            self.invalidate_accessibility_cache();
        }

        (had_events, single_pos)
    }

    /// Drop the cached accessibility snapshot so the next AT walk rebuilds it from a fresh
    /// (masked) `flow_snapshot()`.
    ///
    /// The document-event path above invalidates this when the document changes; a **per-view**
    /// change that fires no document event — a runtime `set_highlight_mask` that drops a
    /// metric-affecting session (e.g. syntax bold) out of this pane's view — must invalidate it
    /// too, or a screen reader keeps hearing formatting the pane has stopped rendering.
    pub fn invalidate_accessibility_cache(&self) {
        *self.accessibility_flow_snapshot.borrow_mut() = None;
        self.synthetic_to_element.borrow_mut().clear();
    }
}

/// Fold a `HighlightPaintChanged` extent into whatever this frame has accumulated so far.
///
/// `pending` says whether anything is accumulated yet: the **first** change of a frame adopts
/// its own extent, later ones widen it. Without that distinction the initial `None` — which
/// means *unknown* — would swallow every real extent and the block-scoped recolor could never
/// fire at all.
///
/// A `length` of `0` is text-document's "unknown — assume the whole document", and it is
/// **sticky**: once one lands in a frame the accumulated range collapses to `None` and stays
/// there until the recolor consumes it. That is what keeps the fast path safe for the
/// operations that still report `0, 0` — installing or retiring a highlighter, a full
/// rehighlight — which really do change everything.
pub(crate) fn accumulate_recolor_range(
    pending: bool,
    current: Option<(usize, usize)>,
    position: usize,
    length: usize,
) -> Option<(usize, usize)> {
    if length == 0 {
        return None;
    }
    if !pending {
        return Some((position, length));
    }
    match current {
        // Already unknown: nothing narrows it back down.
        None => None,
        Some((start, len)) => {
            let lo = start.min(position);
            let hi = (start + len).max(position + length);
            Some((lo, hi - lo))
        }
    }
}

#[cfg(test)]
mod recolor_range_tests {
    use super::accumulate_recolor_range;

    /// The bug this function's `pending` flag exists to prevent: the field starts at `None`
    /// (unknown), so a first change that folded into it would collapse to unknown and the
    /// block-scoped recolor would never run.
    #[test]
    fn the_first_change_of_a_frame_adopts_its_own_extent() {
        assert_eq!(
            accumulate_recolor_range(false, None, 40, 12),
            Some((40, 12))
        );
    }

    #[test]
    fn later_changes_widen_what_is_accumulated() {
        let acc = accumulate_recolor_range(false, None, 40, 12);
        assert_eq!(accumulate_recolor_range(true, acc, 10, 5), Some((10, 42)));
    }

    #[test]
    fn an_unknown_extent_is_sticky_in_both_directions() {
        // An unknown change poisons an accumulated range…
        let acc = accumulate_recolor_range(false, None, 40, 12);
        assert_eq!(accumulate_recolor_range(true, acc, 0, 0), None);
        // …and a later known change cannot narrow it back down.
        assert_eq!(accumulate_recolor_range(true, None, 40, 12), None);
    }

    #[test]
    fn an_unknown_first_change_stays_unknown() {
        assert_eq!(accumulate_recolor_range(false, None, 0, 0), None);
    }
}
