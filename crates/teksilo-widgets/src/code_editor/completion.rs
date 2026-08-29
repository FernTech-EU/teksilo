// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Injected code completion: a caret-anchored suggestion popup.
//!
//! Language-agnostic like everything else in this module. The editor knows how
//! to show a list, filter it by the word before the caret, and replace that word
//! on accept; the *candidates* come from an application-supplied provider
//! (`Fn(&CompletionContext) -> Vec<CompletionItem>`) — keywords, identifiers in
//! scope, an LSP's reply, whatever the app knows. The editor knows nothing about
//! any language.
//!
//! # Why the editor owns the keys
//!
//! Unlike a ComboBox — whose dropdown keeps focus *inside* the overlay so arrow
//! keys bubble to it — a completion popup keeps focus in the **editor** (you are
//! still typing). The popup is a detached overlay, not an ancestor of the focused
//! editor, so keys cannot bubble to it. The editor's own keyboard handler
//! therefore drives navigation directly while the popup is open, and this module
//! drives trigger / filter / dismiss from the document state after each edit. The
//! popup widget ([`CompletionPanel`]) is purely presentational: it renders the
//! current session from the shared state and rebuilds when the session version or
//! the selection changes.
//!
//! # Accessibility
//!
//! The listbox pattern every value-picker in the framework uses (ComboBox,
//! SearchField): the editor's node keeps focus and carries `HasPopup::Listbox` +
//! `AutoComplete::List`, announces `expanded`, and points `active_descendant` at
//! the highlighted row; the popup is a `Role::ListBox` of `Role::ListBoxOption`
//! rows. Focus never moves into the popup.

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::Point;
use teksilo_core::Signal;
use teksilo_core::accesskit::Role;
use teksilo_core::build_context::BuildContext;
use teksilo_core::overlay::{
    DismissBehavior, OverlayDismissCallback, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use teksilo_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_tokens::{SurfaceRole, TextRole, TextStyleRole};

use super::state::{CodeEditorState, SharedState};
use super::{semantics, sync_cursor_signals};

/// The most rows a completion popup shows at once; a longer filtered list
/// windows around the selection.
const MAX_VISIBLE_ROWS: usize = 10;

// ─────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────

/// A completion candidate. Build with [`CompletionItem::new`] and the fluent
/// setters; `insert_text` defaults to `label`.
#[derive(Debug, Clone)]
pub struct CompletionItem {
    /// The text shown in the list.
    pub label: String,
    /// The text that replaces the word being completed when accepted. Defaults
    /// to `label`.
    pub insert_text: String,
    /// Optional dimmed detail shown at the trailing edge of the row (a type, a
    /// signature, a source).
    pub detail: Option<String>,
    /// A category driving the row's leading badge — purely visual, no behaviour.
    pub kind: CompletionKind,
}

impl CompletionItem {
    /// A candidate whose inserted text is its label.
    pub fn new(label: impl Into<String>) -> Self {
        let label = label.into();
        Self {
            insert_text: label.clone(),
            label,
            detail: None,
            kind: CompletionKind::Text,
        }
    }

    /// Override the text inserted on accept (when it differs from the label).
    pub fn insert_text(mut self, text: impl Into<String>) -> Self {
        self.insert_text = text.into();
        self
    }

    /// Trailing dimmed detail (a type or signature).
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// The leading badge category.
    pub fn kind(mut self, kind: CompletionKind) -> Self {
        self.kind = kind;
        self
    }
}

/// The category of a completion candidate — drives a small leading badge only.
/// Deliberately a fixed, language-neutral set: the editor renders a glyph, the
/// application decides which candidate is which kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Text,
    Keyword,
    Function,
    Method,
    Variable,
    Field,
    Type,
    Module,
    Constant,
    Snippet,
}

impl CompletionKind {
    /// A short badge glyph. Kept to letters so it renders in any font (no icon
    /// dependency) and reads under a screen magnifier.
    fn badge(self) -> &'static str {
        match self {
            CompletionKind::Text => "a",
            CompletionKind::Keyword => "k",
            CompletionKind::Function => "ƒ",
            CompletionKind::Method => "m",
            CompletionKind::Variable => "v",
            CompletionKind::Field => "•",
            CompletionKind::Type => "T",
            CompletionKind::Module => "☐",
            CompletionKind::Constant => "c",
            CompletionKind::Snippet => "▢",
        }
    }
}

/// What a completion provider is told about the caret when asked for candidates.
pub struct CompletionContext<'a> {
    /// The identifier characters immediately before the caret.
    pub prefix: &'a str,
    /// The whole current line.
    pub line: &'a str,
    /// The caret's column within the line (character index).
    pub column: usize,
    /// The caret's absolute document position.
    pub position: usize,
}

/// The application-supplied source of candidates.
pub(super) type Provider = Rc<dyn Fn(&CompletionContext) -> Vec<CompletionItem>>;

// ─────────────────────────────────────────────────────────────────────────
// Session + state
// ─────────────────────────────────────────────────────────────────────────

/// The live completion for one word: the candidates the provider gave for it,
/// and the subset matching the current prefix.
struct Session {
    candidates: Vec<CompletionItem>,
    filtered: Vec<usize>,
    /// Document position where the completed word starts — the anchor and the
    /// replace-from point.
    word_start: usize,
}

/// Completion configuration and live state, held on [`CodeEditorState`].
pub(crate) struct CompletionState {
    pub(super) provider: Option<Provider>,
    /// Whether typing an identifier character opens the popup automatically.
    pub(super) auto_trigger: bool,

    session: Option<Session>,
    /// The word position where Escape suppressed completion, so it does not
    /// immediately reopen while the caret stays on that word.
    suppressed_at: Option<usize>,

    /// The pre-created, normally-dormant popup content node.
    pub(super) panel_id: Option<WidgetId>,
    /// The highlighted row's WidgetId, published by the panel build and read by
    /// the body's a11y to point `active_descendant` at it (the roving-focus
    /// pattern — focus stays on the editor).
    pub(super) active_row: Rc<Cell<Option<WidgetId>>>,

    /// Whether the popup is currently shown (drives the panel's `visible_when`,
    /// the body's `expanded`, and the keyboard's routing).
    pub open: Signal<bool>,
    /// The highlighted row, as an index into the *filtered* list. Set
    /// unconditionally on every (re)filter and every arrow move, so the panel —
    /// bound to it at `Rebuild` — re-renders on both; no separate version signal
    /// is needed.
    pub selected: Signal<usize>,
}

impl CompletionState {
    pub(super) fn new() -> Self {
        Self {
            provider: None,
            auto_trigger: true,
            session: None,
            suppressed_at: None,
            panel_id: None,
            active_row: Rc::new(Cell::new(None)),
            open: Signal::new(false),
            selected: Signal::new(0),
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.open.get()
    }

    pub(super) fn has_provider(&self) -> bool {
        self.provider.is_some()
    }

    /// Every filtered candidate, cloned — used only by tests. The panel clones
    /// just its visible window via [`window_items`](Self::window_items).
    #[cfg(test)]
    fn visible_items(&self) -> Vec<CompletionItem> {
        self.window_items(0, self.filtered_len())
    }

    /// The filtered candidates in `[start, end)`, cloned for the panel — only the
    /// rows it will actually render, not the whole (possibly large) list.
    fn window_items(&self, start: usize, end: usize) -> Vec<CompletionItem> {
        match &self.session {
            Some(s) => s.filtered[start.min(s.filtered.len())..end.min(s.filtered.len())]
                .iter()
                .map(|&i| s.candidates[i].clone())
                .collect(),
            None => Vec::new(),
        }
    }

    /// Number of filtered rows.
    fn filtered_len(&self) -> usize {
        self.session.as_ref().map(|s| s.filtered.len()).unwrap_or(0)
    }

    /// The (word_start, insert_text) for a filtered index, if valid.
    fn item_at(&self, filtered_index: usize) -> Option<(usize, String)> {
        let s = self.session.as_ref()?;
        let cand = *s.filtered.get(filtered_index)?;
        Some((s.word_start, s.candidates[cand].insert_text.clone()))
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Driver: trigger / filter / dismiss
// ─────────────────────────────────────────────────────────────────────────

/// Why `react` is running — decides whether the popup may *open* (only typing or
/// an explicit request opens it; an edit or a move only updates or dismisses one
/// already open).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Trigger {
    /// An identifier character was typed.
    Typed,
    /// A deletion (Backspace / Delete).
    Edited,
    /// The caret moved without editing.
    Moved,
    /// An explicit request (Ctrl+Space).
    Forced,
}

/// Re-evaluate completion after an edit, move, or explicit request. Opens,
/// re-filters, or dismisses the popup as the document state dictates. A no-op
/// without a provider.
pub(super) fn react(state: &SharedState, ctx: &mut EventContext, trigger: Trigger) {
    if !state.borrow().completion.has_provider() {
        return;
    }
    // Flush any batched typing so the prefix reflects what the user actually
    // typed (mirrors `type_bracket_char`); nothing to flush on a pure move.
    // Keeping the caret signals in step, since the ordinary batch path would
    // have synced them from the frame loop instead.
    let flushed = {
        let mut st = state.borrow_mut();
        if st.pending_chars.is_empty() {
            false
        } else {
            let batch = std::mem::take(&mut st.pending_chars);
            super::frame_loop::insert_at_every_caret(&mut st, &batch);
            true
        }
    };
    if flushed {
        sync_cursor_signals(state);
    }

    // Fetch a fresh word's candidates OUTSIDE any borrow: an app provider may
    // reach back into the editor through a captured handle, and calling it while
    // the RefCell is borrowed would panic. Nothing else runs between this read
    // and the evaluate below, so the word the fetch is keyed to stays current.
    let fetched = {
        let req = {
            let st = state.borrow();
            prepare_fetch(&st, trigger)
        };
        req.map(|r| {
            let cx = CompletionContext {
                prefix: &r.prefix,
                line: &r.line,
                column: r.column,
                position: r.position,
            };
            (r.word_start, (r.provider)(&cx))
        })
    };

    let decision = {
        let mut st = state.borrow_mut();
        evaluate(&mut st, trigger, fetched)
    };

    match decision {
        Decision::Open(anchor) => open_or_update(state, ctx, anchor),
        Decision::Update => {}
        Decision::Dismiss => close(state, ctx),
        Decision::Idle => {}
    }
}

enum Decision {
    /// Show (or keep showing) the popup, anchored at this window point.
    Open(Point),
    /// Keep the open popup; content already refreshed via the selection signal.
    Update,
    /// Close the popup if open.
    Dismiss,
    /// Do nothing.
    Idle,
}

/// What a fresh-word candidate fetch needs, gathered under a read borrow so the
/// provider can then be called without one.
struct FetchReq {
    word_start: usize,
    prefix: String,
    line: String,
    column: usize,
    position: usize,
    provider: Provider,
}

/// Decide, under a read borrow, whether a fresh-word provider fetch is warranted
/// (the popup would proceed *and* the word changed). Mirrors evaluate's early
/// gates exactly, so the two never disagree on which word is current.
fn prepare_fetch(st: &CodeEditorState, trigger: Trigger) -> Option<FetchReq> {
    // Single-caret only, no active selection.
    if st.cursor.has_selection() || !st.extra_carets.is_empty() {
        return None;
    }
    let was_open = st.completion.open.get();
    let may_open = matches!(trigger, Trigger::Forced)
        || (trigger == Trigger::Typed && st.completion.auto_trigger);
    if !was_open && !may_open {
        return None;
    }
    let pos = st.cursor.position();
    let (word_start, prefix) = semantics::word_prefix_before_caret(st, pos);
    if st.completion.suppressed_at == Some(word_start) && trigger != Trigger::Forced {
        return None;
    }
    if prefix.is_empty() && trigger != Trigger::Forced {
        return None;
    }
    // Only a genuinely new word needs a fetch; refining one reuses the cache.
    let fresh = st
        .completion
        .session
        .as_ref()
        .map(|s| s.word_start != word_start)
        .unwrap_or(true);
    if !fresh {
        return None;
    }
    let (line, column) = st
        .document
        .snapshot_block_at_position_without_highlights(pos)
        .map(|b| (b.text, pos - b.position))
        .unwrap_or_default();
    Some(FetchReq {
        word_start,
        prefix,
        line,
        column,
        position: pos,
        provider: st.completion.provider.clone()?,
    })
}

/// Test hook: run the fetch-then-evaluate cycle without an overlay, so the pure
/// session transition is inspectable via [`CompletionState::test_labels`].
#[cfg(test)]
impl CompletionState {
    pub(super) fn test_labels(&self) -> Vec<String> {
        self.visible_items().into_iter().map(|i| i.label).collect()
    }

    pub(super) fn test_set_suppressed(&mut self, at: Option<usize>) {
        self.suppressed_at = at;
    }
}

#[cfg(test)]
pub(super) fn test_evaluate(state: &SharedState, trigger: Trigger) {
    let fetched = {
        let req = {
            let st = state.borrow();
            prepare_fetch(&st, trigger)
        };
        req.map(|r| {
            let cx = CompletionContext {
                prefix: &r.prefix,
                line: &r.line,
                column: r.column,
                position: r.position,
            };
            (r.word_start, (r.provider)(&cx))
        })
    };
    let mut st = state.borrow_mut();
    let _ = evaluate(&mut st, trigger, fetched);
}

/// The state transition: recompute the session and decide the popup's fate.
/// `fetched` carries candidates already obtained (outside the borrow) for a
/// fresh word — the provider is never called here. Sets the selection signal
/// (a `bind_to` observer only marks the panel dirty, so no re-entrant borrow);
/// the `open` signal is set later, outside any borrow, by the overlay calls.
fn evaluate(
    st: &mut CodeEditorState,
    trigger: Trigger,
    fetched: Option<(usize, Vec<CompletionItem>)>,
) -> Decision {
    let was_open = st.completion.open.get();

    // A forced request lifts any Escape suppression for the current word.
    if trigger == Trigger::Forced {
        st.completion.suppressed_at = None;
    }

    // Completion is single-caret by decision: a selection or several carets means
    // the user is doing something else. Accepting only ever touches the primary
    // caret, so activating with extras would silently discard them.
    if st.cursor.has_selection() || !st.extra_carets.is_empty() {
        st.completion.session = None;
        return if was_open {
            Decision::Dismiss
        } else {
            Decision::Idle
        };
    }

    // Cheap gate before the per-line prefix scan: a closed popup this trigger may
    // not open has nothing to do (plain navigation with no popup, the common
    // case for an editor that has a provider installed).
    let may_open = matches!(trigger, Trigger::Forced)
        || (trigger == Trigger::Typed && st.completion.auto_trigger);
    if !was_open && !may_open {
        return Decision::Idle;
    }

    let pos = st.cursor.position();
    let (word_start, prefix) = semantics::word_prefix_before_caret(st, pos);

    if st.completion.suppressed_at == Some(word_start) && trigger != Trigger::Forced {
        return Decision::Idle;
    }
    if st.completion.suppressed_at.is_some() && st.completion.suppressed_at != Some(word_start) {
        st.completion.suppressed_at = None;
    }

    // A move onto a different word closes an open popup (you navigated away).
    if was_open
        && trigger == Trigger::Moved
        && st.completion.session.as_ref().map(|s| s.word_start) != Some(word_start)
    {
        st.completion.session = None;
        return Decision::Dismiss;
    }

    // Nothing to complete on an empty prefix unless explicitly forced.
    if prefix.is_empty() && trigger != Trigger::Forced {
        st.completion.session = None;
        return if was_open {
            Decision::Dismiss
        } else {
            Decision::Idle
        };
    }

    // (Re)build the session for a new word from the pre-fetched candidates; keep
    // the cached ones while refining the same word.
    let fresh_word = st
        .completion
        .session
        .as_ref()
        .map(|s| s.word_start != word_start)
        .unwrap_or(true);
    if fresh_word {
        let candidates = match fetched {
            Some((ws, cands)) if ws == word_start => cands,
            _ => Vec::new(),
        };
        st.completion.session = Some(Session {
            candidates,
            filtered: Vec::new(),
            word_start,
        });
    }

    // Filter by the current prefix (case-insensitive prefix match). An empty
    // prefix (forced) matches everything.
    let lower = prefix.to_lowercase();
    let filtered: Vec<usize> = {
        let s = st.completion.session.as_ref().expect("session set above");
        s.candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| lower.is_empty() || c.label.to_lowercase().starts_with(&lower))
            .map(|(i, _)| i)
            .collect()
    };
    let empty = filtered.is_empty();
    if let Some(s) = st.completion.session.as_mut() {
        s.filtered = filtered;
    }

    if empty {
        return if was_open {
            Decision::Dismiss
        } else {
            Decision::Idle
        };
    }

    // Keep the selection in range; a fresh word restarts at the top. `set` is
    // unconditional, so the panel (bound at Rebuild) re-renders even when the
    // index is unchanged but the filtered set is not.
    let len = st.completion.filtered_len();
    let sel = if fresh_word {
        0
    } else {
        st.completion.selected.get().min(len - 1)
    };
    st.completion.selected.set(sel);

    if was_open {
        Decision::Update
    } else {
        // Anchor at the START of the word so the popup stays put while typing.
        // Before a layout there is no rect; the next keystroke retries (react
        // runs per key), so this self-heals rather than sticking.
        match super::keyboard::window_rect_at(st, word_start) {
            Some(r) => Decision::Open(Point::new(r.x, r.y + r.height)),
            None => Decision::Idle,
        }
    }
}

/// Show the popup (or, if somehow already shown, leave it) at `anchor`.
fn open_or_update(state: &SharedState, ctx: &mut EventContext, anchor: Point) {
    let (panel_id, self_id, open_sig) = {
        let st = state.borrow();
        (
            st.completion.panel_id,
            st.self_id,
            st.completion.open.clone(),
        )
    };
    let (Some(panel_id), Some(self_id)) = (panel_id, self_id) else {
        return;
    };
    open_sig.set_if_changed(true);
    // Build the panel if this is its first open, before the overlay below is
    // measured against it.
    ctx.materialize_now(panel_id);
    ctx.activate(panel_id);

    let on_dismiss: OverlayDismissCallback = {
        let open = open_sig.clone();
        Rc::new(move || {
            if open.get() {
                open.set(false);
            }
        })
    };
    ctx.show_overlay(OverlayRequest {
        content_id: panel_id,
        anchor: self_id,
        placement: OverlayPlacement::AtPointer(anchor),
        dismiss: DismissBehavior::ClickOutside,
        layer: OverlayLayer::InTree,
        parent_overlay: None,
        on_dismiss: Some(on_dismiss),
        fade_duration: None,
    });
    ctx.request_frame();
}

/// Close the popup and forget the session. Idempotent, and safe to call after a
/// framework-driven dismissal (click-outside), which flips `open` via the
/// `on_dismiss` callback but leaves the session — so the session is cleared here
/// **unconditionally**, and the `open` signal is set outside the borrow so its
/// `visible_when` fan-out cannot re-enter.
pub(super) fn close(state: &SharedState, ctx: &mut EventContext) {
    let (was_open, panel_id, open_sig) = {
        let mut st = state.borrow_mut();
        st.completion.session = None;
        (
            st.completion.open.get(),
            st.completion.panel_id,
            st.completion.open.clone(),
        )
    };
    open_sig.set_if_changed(false);
    if was_open && let Some(pid) = panel_id {
        ctx.dismiss_overlay_by_content(pid);
    }
    ctx.request_frame();
}

// ─────────────────────────────────────────────────────────────────────────
// Keyboard navigation (called from keyboard.rs while the popup is open)
// ─────────────────────────────────────────────────────────────────────────

/// Move the highlighted row by `delta`, wrapping. Repaints via the selection
/// signal (the panel rebuilds its window around it).
pub(super) fn move_selection(state: &SharedState, delta: i32) {
    let st = state.borrow();
    let len = st.completion.filtered_len();
    if len == 0 {
        return;
    }
    let cur = st.completion.selected.get() as i32;
    let next = cur + delta;
    let wrapped = next.rem_euclid(len as i32) as usize;
    st.completion.selected.set(wrapped);
}

/// Accept the currently-highlighted candidate.
pub(super) fn accept_selected(state: &SharedState, ctx: &mut EventContext) {
    let sel = state.borrow().completion.selected.get();
    commit(state, ctx, sel);
}

/// Accept a candidate by filtered index (also the mouse-click path).
///
/// Re-validates against the *live* caret before applying: a popup can go stale
/// between opening and accepting (a Ctrl-chord that changed the document or
/// selection while it lingered), and blindly replacing `[session.word_start,
/// caret]` could delete from the old word to wherever the caret now is — up to
/// the end of the document under a select-all. So the accept only proceeds when
/// the caret is still on the session's word with no selection, and it replaces
/// the **whole** identifier there (start to end), not merely up to the caret.
pub(super) fn commit(state: &SharedState, ctx: &mut EventContext, filtered_index: usize) {
    let accepted = state.borrow().completion.item_at(filtered_index);
    let Some((session_word_start, insert)) = accepted else {
        close(state, ctx);
        return;
    };
    let span = {
        let st = state.borrow();
        if st.cursor.has_selection() {
            None
        } else {
            let pos = st.cursor.position();
            let (word_start, _) = semantics::word_prefix_before_caret(&st, pos);
            if word_start != session_word_start {
                None // the caret left the completing word — do not apply
            } else {
                Some((word_start, semantics::identifier_end(&st, pos)))
            }
        }
    };
    if let Some((start, end)) = span {
        let mut st = state.borrow_mut();
        semantics::accept_completion(&mut st, start, end, &insert);
        st.pending_text_changed = true;
    }
    close(state, ctx);
    sync_cursor_signals(state);
    super::keyboard::ensure_caret_visible(state);
    ctx.request_frame();
}

/// Escape: close the popup and suppress reopening for the current word.
pub(super) fn dismiss_suppress(state: &SharedState, ctx: &mut EventContext) {
    {
        let mut st = state.borrow_mut();
        let pos = st.cursor.position();
        let (word_start, _) = semantics::word_prefix_before_caret(&st, pos);
        st.completion.suppressed_at = Some(word_start);
    }
    close(state, ctx);
}

// ─────────────────────────────────────────────────────────────────────────
// The popup widget
// ─────────────────────────────────────────────────────────────────────────

/// The presentational suggestion list — reads the live session from the shared
/// state, rebuilds when the session version or the selection changes, and
/// commits a row on tap. It holds no completion logic of its own.
pub(super) struct CompletionPanel {
    state: SharedState,
    root: Option<WidgetId>,
}

impl std::fmt::Debug for CompletionPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompletionPanel").finish_non_exhaustive()
    }
}

impl CompletionPanel {
    pub(super) fn new(state: &SharedState) -> Self {
        Self {
            state: state.clone(),
            root: None,
        }
    }
}

impl Widget for CompletionPanel {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        use crate::primitives::{HStack, Padding, RectWidget, Spacer, TextWidget, VStack, ZStack};
        use teksilo_core::binding::BindingLevel;
        use teksilo_i18n::lit;
        use teksilo_tokens::CornerRadius;

        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        {
            // The selection signal is set on every (re)filter and every arrow
            // move, so a single Rebuild binding covers both the row set changing
            // and the highlight moving — no separate version signal is needed.
            let st = self.state.borrow();
            st.completion
                .selected
                .bind_to(self_id, registry, BindingLevel::Rebuild);
        }

        let (total, selected) = {
            let st = self.state.borrow();
            (st.completion.filtered_len(), st.completion.selected.get())
        };
        if total == 0 {
            self.state.borrow().completion.active_row.set(None);
            self.root = None;
            return Vec::new();
        }

        // Window the rows around the selection, and clone only that window.
        let m = MAX_VISIBLE_ROWS.min(total);
        let mut start = 0usize;
        if selected >= m {
            start = selected - m + 1;
        }
        if start > total - m {
            start = total - m;
        }
        let end = start + m;
        let items = self.state.borrow().completion.window_items(start, end);

        let mut rows = VStack::new().spacing(1.0);
        let mut active_row = None;
        for (local, item) in items.iter().enumerate() {
            let i = start + local;
            let highlighted = i == selected;

            let badge = TextWidget::new(lit!(item.kind.badge()))
                .style(TextStyleRole::Small)
                .color(TextRole::Secondary);
            let label = TextWidget::new(lit!(item.label.clone())).style(TextStyleRole::Body);
            let mut line = HStack::new()
                .spacing(6.0)
                .child(badge)
                .child(label)
                .child(Spacer::new());
            if let Some(detail) = &item.detail {
                line = line.child(
                    TextWidget::new(lit!(detail.clone()))
                        .style(TextStyleRole::Small)
                        .color(TextRole::Secondary),
                );
            }

            let row_state = self.state.clone();
            let filtered_index = i;
            let posinset = i + 1;
            // Only the highlighted row paints a background; the rest are bare, so
            // no "transparent" role is needed.
            let mut row = ZStack::new();
            if highlighted {
                row = row.child(
                    RectWidget::new()
                        .background(SurfaceRole::Selected)
                        .corner_radius(CornerRadius::uniform(4.0)),
                );
            }
            let row = row
                .child(Padding::symmetric(3.0, 8.0).child(line))
                .on_tap(move |_event, ctx| {
                    commit(&row_state, ctx, filtered_index);
                })
                .access_role(Role::ListBoxOption)
                .access_customize(move |b| {
                    b.inner_mut().set_selected(highlighted);
                    b.inner_mut().set_position_in_set(posinset);
                    b.inner_mut().set_size_of_set(total);
                });
            let id = ctx.add(row);
            if highlighted {
                active_row = Some(id);
            }
            rows = rows.add_child(id);
        }
        self.state.borrow().completion.active_row.set(active_row);

        // A themed container: raised surface, hairline border, rounded.
        let container = ZStack::new()
            .child(
                RectWidget::new()
                    .background(SurfaceRole::Raised)
                    .border_color(teksilo_tokens::BorderRole::Default)
                    .border_width(1.0)
                    .corner_radius(CornerRadius::uniform(6.0)),
            )
            .child(Padding::symmetric(4.0, 4.0).child(rows));
        let container_id = ctx.add(container);
        self.root = Some(container_id);
        vec![container_id]
    }

    fn layout_response(
        &self,
        proposal: teksilo_canvas::SizeProposal,
        ctx: &LayoutContext,
    ) -> LayoutResponse {
        // Size to the container's content — the popup is intrinsic, not greedy.
        self.root
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| teksilo_canvas::Size::new(0.0, 0.0))
            .into()
    }

    fn place_children(
        &self,
        bounds: teksilo_canvas::Rect,
        _proposal: teksilo_canvas::SizeProposal,
        children: &mut [teksilo_core::widget::WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        if let Some(child) = children.first_mut() {
            child.origin = Point::new(bounds.x, bounds.y);
            child.size = teksilo_canvas::Size::new(bounds.width, bounds.height);
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root.into_iter().collect()
    }

    fn accessibility(&self, builder: &mut teksilo_core::accessibility::AccessNodeBuilder) {
        builder.set_role(Role::ListBox);
    }

    fn clips_children(&self) -> bool {
        true
    }
}
