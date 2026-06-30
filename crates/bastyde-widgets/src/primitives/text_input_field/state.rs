// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared mutable state for a single `TextInput` instance.
//!
//! Mirrors the `Rc<RefCell<State>>` pattern from
//! `rich_text::state` but stripped down
//! for single-line plain-text editing: no scroll bars, no rich
//! formatting, no table/cell state, no image cache.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use bastyde_canvas::Point;
use bastyde_core::signal::Signal;
use bastyde_core::widget::EventContext;
use bastyde_core::widget_id::WidgetId;
use bastyde_text::text_document::{DocumentEvent, Subscription, TextCursor, TextDocument};
use bastyde_text::{RichTextEngine, WrapMode};

use super::{AtRevealPolicy, EchoMode};
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
    /// Whether the host window is currently active (`focused AND not
    /// occluded`). Mirrored from `BuildContext::window_active_signal` by an
    /// effect in `TextInputField::build` (the frame-loop `tick` has no
    /// context). Gates the caret alongside `has_focus` — the caret hides in an
    /// inactive window. Starts `true` to match the tree's initial value.
    pub window_active: bool,
    pub drag_state: DragState,
    pub needs_full_layout: bool,
    pub content_dirty: bool,
    /// Last applied global text-scale factor (`ctx.text_scale`). Tracked so the
    /// engine's logical `font_scale` is only re-set (and a relayout forced) when
    /// the accessibility scale actually changes.
    pub last_text_scale: f32,
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

    // ── Secure / password masking ───────────────────────────────────
    /// When `true`, this is a secure (password) field: glyphs are
    /// masked per `echo_mode` unless currently revealed.
    pub secure: bool,
    /// How a secure field echoes characters. Ignored when `secure` is
    /// `false`.
    pub echo_mode: EchoMode,
    /// Replacement glyph for `Masked` / `RevealWhileTyping` modes
    /// (default `'•'`). Applied at the engine layer so plaintext never
    /// reaches the shaper or glyph atlas while masked.
    pub echo_char: char,
    /// External reveal toggle, shared with the eye button. `Some(true)`
    /// shows plaintext regardless of `echo_mode`; `None` is a secure
    /// field with no reveal affordance.
    pub revealed: Option<Signal<bool>>,
    /// How a revealed secure field reports to assistive technology.
    pub at_reveal_policy: AtRevealPolicy,
    /// Whether copy / cut are permitted. Plain fields default `true`;
    /// secure fields default `false` (still copyable when revealed).
    pub allow_copy: bool,
    /// Empty document used as the layout source for `NoEcho` masking so
    /// nothing — not even length — is shown. The real `document` stays
    /// the source of truth for editing.
    pub empty_doc: TextDocument,

    /// The field widget's own id, used as anchor for overlays (e.g.
    /// the autocomplete popup) and for downstream tests that snapshot
    /// AT trees keyed by widget id.
    pub field_widget_id: Option<WidgetId>,

    // ── IME composition (preedit) ────────────────────────────────────
    /// Active IME preedit (composition) string, or `None` when not
    /// composing. The text is inserted into `document` tentatively and
    /// tracked by `ime_preedit_range`; secure masking / echo applies to
    /// it like any other content, so a password preedit shows as bullets.
    pub ime_preedit: Option<String>,
    /// Character range in `document` currently occupied by the live
    /// preedit, so a follow-up composition event can remove and replace it.
    pub ime_preedit_range: Option<std::ops::Range<usize>>,
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
    pub secure: bool,
    pub echo_mode: EchoMode,
    pub echo_char: char,
    pub revealed: Option<Signal<bool>>,
    pub at_reveal_policy: AtRevealPolicy,
    pub allow_copy: bool,
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
            secure,
            echo_mode,
            echo_char,
            revealed,
            at_reveal_policy,
            allow_copy,
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
            window_active: true,
            drag_state: DragState::Idle,
            needs_full_layout: true,
            content_dirty: true,
            last_text_scale: 1.0,
            image_cache: ImageCache::new(),
            max_length,
            read_only,
            on_submit,
            on_blur,
            char_filter,
            placeholder,
            suffix,
            secure,
            echo_mode,
            echo_char,
            revealed,
            at_reveal_policy,
            allow_copy,
            empty_doc: TextDocument::new(),
            suffix_engine: None,
            suffix_width: 0.0,
            field_widget_id: None,
            ime_preedit: None,
            ime_preedit_range: None,
        }))
    }

    /// Whether the configured `char_filter` (if any) admits this
    /// character. `None` admits every character; inverted so callers
    /// can write `if !st.char_filter_admits(c) { skip }`.
    pub fn char_filter_admits(&self, c: char) -> bool {
        self.char_filter.as_ref().is_none_or(|f| f(c))
    }

    // ── Secure-field masking ────────────────────────────────────────

    /// Whether plaintext is currently shown despite `secure` — the
    /// reveal toggle is on, or `RevealWhileTyping` is active and the
    /// field is focused. Always `true` for non-secure fields.
    pub fn reveal_active(&self) -> bool {
        if !self.secure {
            return true;
        }
        let toggled = self.revealed.as_ref().is_some_and(|s| s.get());
        toggled || (self.echo_mode == EchoMode::RevealWhileTyping && self.has_focus)
    }

    /// Whether the displayed glyphs should be masked right now.
    pub fn should_mask(&self) -> bool {
        self.secure && !self.reveal_active()
    }

    /// Whether copy / cut of the field's text is currently permitted.
    /// Plain fields always allow it; secure fields allow it only when
    /// the developer opted in (`allow_copy`) or the text is currently
    /// revealed.
    pub fn copy_allowed(&self) -> bool {
        !self.secure || self.allow_copy || self.reveal_active()
    }

    /// Run a full layout, applying secure masking. Installs the echo
    /// char on the engine (or clears it), and for `NoEcho` while masked
    /// lays out an empty source so nothing — not even length — is
    /// shown. The real `document` is never mutated: masking is
    /// display-only, so caret / selection / hit-test (all char-indexed)
    /// stay aligned because one echo char is emitted per source char.
    /// Apply the global accessibility text scale to the shaping engine(s).
    ///
    /// `scale` is `ctx.text_scale` (combined user×OS factor). When it changes,
    /// the main engine's logical `font_scale` is updated so the value text grows
    /// (advances + line height + content height), a full relayout is forced, and
    /// the suffix engine is re-laid out at the new scale so its width stays
    /// correct. Cheap no-op when the scale is unchanged.
    pub fn apply_font_scale(&mut self, scale: f32) {
        if (self.last_text_scale - scale).abs() <= f32::EPSILON {
            return;
        }
        self.last_text_scale = scale;
        self.engine.set_font_scale(scale);
        self.needs_full_layout = true;
        if !self.suffix.is_empty()
            && let Some(engine) = self.suffix_engine.as_mut()
        {
            engine.set_font_scale(scale);
            let doc = TextDocument::new();
            let _ = doc.set_plain_text(&self.suffix);
            let flow = doc.snapshot_flow();
            engine.layout_full(&flow);
            self.suffix_width = engine.max_content_width();
        }
    }

    pub fn layout_full_masked(&mut self) {
        let masked = self.should_mask();
        let echo = if masked && self.echo_mode != EchoMode::NoEcho {
            Some(self.echo_char)
        } else {
            None
        };
        if self.engine.echo_char() != echo {
            self.engine.set_echo_char(echo);
        }
        if masked && self.echo_mode == EchoMode::NoEcho {
            let flow = self.empty_doc.snapshot_flow();
            self.engine.layout_full(&flow);
        } else {
            let flow = self.document.snapshot_flow();
            self.engine.layout_full(&flow);
        }
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
                DocumentEvent::FormatChanged { .. }
                // text_input_field never installs a SyntaxHighlighter, so this
                // cannot arrive in practice; handle conservatively (full
                // relayout) for exhaustiveness and shared-document safety.
                | DocumentEvent::HighlightPaintChanged { .. } => {
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

#[cfg(test)]
mod secure_tests {
    use super::*;

    fn cfg(
        secure: bool,
        echo_mode: EchoMode,
        revealed: Option<Signal<bool>>,
        allow_copy: bool,
    ) -> TextInputConfig {
        TextInputConfig {
            initial_text: "abc".to_string(),
            max_length: None,
            read_only: false,
            on_submit: None,
            on_blur: None,
            char_filter: None,
            placeholder: String::new(),
            suffix: String::new(),
            secure,
            echo_mode,
            echo_char: '\u{2022}',
            revealed,
            at_reveal_policy: AtRevealPolicy::SwapRole,
            allow_copy,
        }
    }

    #[test]
    fn plain_field_never_masks_and_allows_copy() {
        let st = TextInputState::new(cfg(false, EchoMode::Masked, None, true));
        let st = st.borrow();
        assert!(!st.should_mask());
        assert!(st.reveal_active());
        assert!(st.copy_allowed());
    }

    #[test]
    fn masked_secure_field_masks_and_blocks_copy() {
        let st = TextInputState::new(cfg(true, EchoMode::Masked, None, false));
        let st = st.borrow();
        assert!(st.should_mask());
        assert!(!st.reveal_active());
        assert!(!st.copy_allowed(), "masked secure field must block copy");
    }

    #[test]
    fn revealed_secure_field_unmasks_and_allows_copy() {
        let revealed = Signal::new(true);
        let st = TextInputState::new(cfg(true, EchoMode::Masked, Some(revealed), false));
        let st = st.borrow();
        assert!(!st.should_mask());
        assert!(st.copy_allowed(), "copy allowed once revealed");
    }

    #[test]
    fn allow_copy_opt_in_permits_copy_while_masked() {
        let st = TextInputState::new(cfg(true, EchoMode::Masked, None, true));
        let st = st.borrow();
        assert!(st.should_mask(), "still visually masked");
        assert!(st.copy_allowed(), "developer opted into copy");
    }

    #[test]
    fn reveal_while_typing_unmasks_only_when_focused() {
        let st = TextInputState::new(cfg(true, EchoMode::RevealWhileTyping, None, false));
        assert!(st.borrow().should_mask(), "masked when unfocused");
        st.borrow_mut().has_focus = true;
        let s = st.borrow();
        assert!(!s.should_mask(), "revealed while focused");
        assert!(s.copy_allowed(), "copy allowed while revealed by typing");
    }
}
