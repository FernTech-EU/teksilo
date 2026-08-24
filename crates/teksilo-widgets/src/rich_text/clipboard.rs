// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! System clipboard integration for the rich text editor.
//!
//! Three free functions — `copy`, `cut`, `paste` — implement the
//! godot reference's in-process rich fragment preservation pattern at
//! [rich_text_edit.rs:2048-2087](../../../../godot-rich-text/src/rich_text_edit.rs#L2048-L2087).
//!
//! The functions talk to a `ClipboardHandle` retrieved via
//! `EventContext::app_state::<ClipboardHandle>()`. In headless tests
//! or builds without the `teksilo-app/clipboard` feature the handle is
//! absent and copy/cut/paste silently no-op — the same behaviour as
//! trying to paste an image into a pure-text editor. No panic, no
//! error propagation — the user sees nothing happen and the command
//! filter's UI affordance drives the expectation.
//!
//! Self-round-trip detection embeds an opaque marker as an HTML
//! comment at the head of the clipboard HTML payload and re-reads it
//! on paste. Plain-text equality alone is ambiguous — two different
//! apps can publish identical plain text with different formatting —
//! so the marker is the reliable signal that *this* editor wrote the
//! clipboard. The marker is regenerated on every copy/cut so a stale
//! state from an earlier session can never accidentally match a later
//! external copy whose plain text happens to coincide.
//!
//! Plain-text equality does serve as a **last** resort, for the one case the
//! marker cannot reach: a clipboard backend that carries no HTML at all. There
//! is no foreign rich payload to be confused with then, so matching the text
//! this editor last copied is unambiguous, and without it an intra-app
//! copy/paste would lose its formatting on every such backend.

use std::time::{SystemTime, UNIX_EPOCH};

use teksilo_core::widget::EventContext;
use teksilo_platform::clipboard::ClipboardHandle;

use super::state::EditorState;

/// HTML comment prefix embedded in the clipboard payload to flag a
/// self-copy. The trailing hex token is regenerated per copy.
const MARKER_PREFIX: &str = "<!--teksilo-rtc:";
const MARKER_SUFFIX: &str = "-->";

/// Copy the current selection to the system clipboard. No-op when
/// there is no selection — matches editor convention (the menu item
/// and the Ctrl+C shortcut both stay silent rather than capturing an
/// empty fragment).
///
/// Writes both HTML and plain-text payloads so rich paste works in
/// any other application that understands `text/html` (Firefox,
/// Word, Google Docs, Apple Notes, …). `DocumentFragment::to_html`
/// is a lossless-enough serialisation to survive the round-trip
/// through arboard's platform backends. Backends without HTML
/// support degrade gracefully — the `set_html` default body writes
/// just the plain-text alternative.
///
/// The rich fragment + plain text are also stashed on editor state
/// for self-round-trip detection during paste: an intra-editor
/// copy/paste pair re-inserts the original `DocumentFragment`
/// rather than round-tripping through HTML (cheaper and bit-exact).
pub(crate) fn copy(state: &mut EditorState, ctx: &EventContext) {
    if !state.cursor.has_selection() {
        return;
    }
    let fragment = state.cursor.selection();
    let plain = fragment.to_plain_text().to_string();
    let marker = new_marker();
    let html = format!(
        "{MARKER_PREFIX}{marker}{MARKER_SUFFIX}{}",
        fragment.to_html()
    );
    // Whether the marker actually reached the clipboard. The plain-text identity
    // below is a fallback for backends that cannot carry it at all, so it is
    // kept only when the marker did **not** land: on a backend that took the
    // HTML, a later `set_text` from another application can leave text
    // identical to ours with no marker behind it — the exact ambiguity the
    // marker exists to resolve — and reusing our stale fragment there would
    // paste this editor's old formatting onto somebody else's text.
    let mut carries_marker = false;
    if let Some(cb) = ctx.app_state::<ClipboardHandle>() {
        // `set_html` writes both payloads in one transaction. Backends
        // without native HTML support see the default trait body and
        // fall back to `set_text(plain)`.
        let _ = cb.set_html(&html, &plain);
        carries_marker = cb.has_html();
    }
    state.rich_clipboard_fragment = Some(fragment);
    state.rich_clipboard_marker = Some(marker);
    state.rich_clipboard_plain = (!carries_marker).then_some(plain);
}

/// Generate a per-copy marker from the wall clock, in nanoseconds.
///
/// Not a cryptographic identifier, and not a unique one either: the clock is
/// not monotonic, so a step backwards can repeat a value. Neither matters. The
/// only requirement is that an *unrelated* application cannot plausibly emit
/// the same string, and a 32-hex-digit token behind a `teksilo-rtc:` prefix
/// clears that by a wide margin. Repeating our own token costs nothing either:
/// the fragment it guards was overwritten by the same copy that regenerated
/// it.
fn new_marker() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:032x}")
}

/// How far into a clipboard payload the marker is still ours.
///
/// Long enough for any plausible wrapper preamble, short enough that a
/// `teksilo-rtc:` comment sitting in the *body* of a foreign document cannot
/// masquerade as one.
const MARKER_SEARCH_WINDOW: usize = 1024;

/// Extract the marker token from a clipboard HTML payload written by
/// `copy`. Returns `None` for any payload that doesn't carry
/// `<!--teksilo-rtc:...-->` near its head — which covers every payload emitted
/// by any other app.
///
/// Near its head, not at byte 0: platform clipboards are entitled to wrap what
/// they are handed, and macOS does. `arboard`'s AppKit backend puts every
/// payload inside `<html><head><meta …></head><body>…</body></html>` on write
/// and hands the wrapper straight back on read, so a strict prefix test never
/// matched there and every intra-editor copy/paste on macOS silently fell
/// through to re-parsing its own HTML. (X11 and Wayland round-trip verbatim;
/// Windows' CF_HTML reader slices to `StartFragment`, which lands exactly on
/// the marker.)
fn payload_marker(html: &str) -> Option<&str> {
    // Truncate on a character boundary — a clipboard payload is arbitrary UTF-8
    // and slicing it mid-codepoint would panic on somebody else's document.
    let limit = html
        .char_indices()
        .find(|(i, _)| *i >= MARKER_SEARCH_WINDOW)
        .map_or(html.len(), |(i, _)| i);
    let start = html[..limit].find(MARKER_PREFIX)? + MARKER_PREFIX.len();
    let rest = &html[start..];
    let end = rest.find(MARKER_SUFFIX)?;
    Some(&rest[..end])
}

/// Cut the current selection: copy first, then remove. `pending_text_changed`
/// is set so the debounce drain publishes a `text_changed` command once
/// the 150 ms window closes. No-op with no selection.
pub(crate) fn cut(state: &mut EditorState, ctx: &EventContext) {
    if !state.cursor.has_selection() {
        return;
    }
    copy(state, ctx);
    let _ = state.cursor.remove_selected_text();
    state.pending_text_changed = true;
}

/// Paste from the system clipboard. Prefers richer payloads, in order:
///
/// 1. **Self-round-trip rich fragment** — if the clipboard's HTML payload
///    carries the marker this editor's last copy embedded, reinsert the
///    stored `DocumentFragment` so intra-editor formatting round-trips
///    losslessly (retains table cells, heading levels, spans that don't
///    serialise into HTML losslessly). The marker, not plain-text equality:
///    two applications can publish identical text with different formatting.
/// 2. **External HTML payload** — if the clipboard carries `text/html`
///    / `CF_HTML` / `public.html`, parse it into a `DocumentFragment`
///    via text-document and insert. This is the path that makes
///    rich paste *from another app* work (Firefox, Word, Google Docs,
///    etc.).
/// 3. **Self-round-trip by plain text** — with no HTML payload at all there
///    is no foreign rich content to be confused with, so text identical to
///    what this editor last copied reinserts the stored fragment. Only this
///    keeps formatting on a clipboard backend that cannot carry HTML, where
///    step 1 can never fire.
/// 4. **Plain-text fallback** — when no rich path applies, split
///    the clipboard text on `\n` / `\r\n` / `\r` and insert as separate
///    blocks. Without the split, a multi-line clipboard payload would
///    collapse into one block with literal newline scalars —
///    `text-document::TextCursor::insert_text` never splits blocks on
///    its own.
///
/// Clears any existing selection after insertion so the caret sits at
/// the end of the pasted content rather than keeping the newly inserted
/// range selected — matches godot behaviour.
pub(crate) fn paste(state: &mut EditorState, ctx: &EventContext) {
    let Some(cb) = ctx.app_state::<ClipboardHandle>() else {
        return;
    };

    // Every branch below inserts, and each insertion primitive removes the
    // selected range first. Collapse once, up here, so a forward-only surface
    // pastes *after* the selection rather than over it.
    super::keyboard::collapse_selection_before_insert(state);

    // 1. Self-round-trip rich fragment. Inspect the HTML payload for
    //    our marker rather than comparing plain text, which is
    //    ambiguous across apps.
    let html_payload: Option<String> = if cb.has_html() {
        cb.get_html().ok().filter(|h| !h.is_empty())
    } else {
        None
    };
    if let (Some(html), Some(stored_marker)) = (
        html_payload.as_deref(),
        state.rich_clipboard_marker.as_deref(),
    ) && payload_marker(html).is_some_and(|m| m == stored_marker)
        && insert_stored_fragment(state)
    {
        return;
    }

    // 2. External HTML payload. Re-uses the payload we already fetched
    //    above when present; the marker check proved it's not ours.
    //    `TextCursor::insert_html` parses the HTML into a
    //    `DocumentFragment` (via text-document's
    //    `DocumentFragment::from_html`) and inserts it at the caret.
    if let Some(html) = html_payload.as_deref()
        && report_measured(state, |st| {
            st.cursor.begin_edit_block();
            let out = st.cursor.insert_html(html);
            st.cursor.end_edit_block();
            out
        })
        .is_ok()
    {
        state.cursor.clear_selection();
        state.pending_text_changed = true;
        return;
    }

    let Ok(text) = cb.get_text() else {
        return;
    };
    if text.is_empty() {
        return;
    }

    // 3. Self-round-trip by plain text. `rich_clipboard_plain` is set only when
    //    the copy could not put its marker on the clipboard — a backend
    //    inheriting the default `set_html` body writes the plain alternative
    //    and drops the rest — so reaching here means there was never a marker
    //    to miss, and text equality is the only identity available. On a
    //    backend that does carry HTML the field is `None` and this cannot fire,
    //    which keeps another application's identical plain text from picking up
    //    our stale formatting.
    if html_payload.is_none()
        && state.rich_clipboard_plain.as_deref() == Some(text.as_str())
        && insert_stored_fragment(state)
    {
        return;
    }

    // 4. Plain-text fallback.
    insert_multiline_plain(state, &text);
    state.cursor.clear_selection();
    state.pending_text_changed = true;
}

/// Reinsert the fragment stashed by the last copy/cut, reporting how much
/// arrived. `false` when there is nothing stashed, so the caller falls through
/// to the next payload shape.
fn insert_stored_fragment(state: &mut EditorState) -> bool {
    let Some(frag) = state.rich_clipboard_fragment.clone() else {
        return false;
    };
    report_measured(state, |st| {
        st.cursor.begin_edit_block();
        let _ = st.cursor.insert_fragment(&frag);
        st.cursor.end_edit_block();
    });
    state.cursor.clear_selection();
    state.pending_text_changed = true;
    true
}

/// Paste plain text only, bypassing any rich payload. Bound to
/// Ctrl+Shift+V / ⌘⇧V and exposed from the default context menu as
/// "Paste Unformatted". Skips both the self-round-trip fragment
/// reinsertion and the HTML parse path — the user explicitly asked
/// for plain text, so even if the clipboard has a richer payload
/// we insert `get_text()` verbatim.
pub(crate) fn paste_unformatted(state: &mut EditorState, ctx: &EventContext) {
    let Some(cb) = ctx.app_state::<ClipboardHandle>() else {
        return;
    };
    let mut text = cb.get_text().unwrap_or_default();
    if text.is_empty() {
        // An application is free to publish `text/html` with an empty plain-text
        // alternative, and some do. Reading only `get_text` then made
        // Paste Unformatted a command that visibly did nothing while
        // `can_paste` — which asks `has_text() || has_html()` — kept offering
        // it. Flattening the rich payload is what the user asked for anyway.
        let Some(html) = cb.get_html().ok().filter(|h| !h.is_empty()) else {
            return;
        };
        text = teksilo_text::text_document::DocumentFragment::from_html(&html)
            .to_plain_text()
            .to_string();
        if text.is_empty() {
            return;
        }
    }
    // As in `paste`: append after the selection rather than over it when the
    // filter forbids taking text away.
    super::keyboard::collapse_selection_before_insert(state);
    insert_multiline_plain(state, &text);
    state.cursor.clear_selection();
    state.pending_text_changed = true;
}

/// Whether a [`paste`] would insert anything — `true` iff the system
/// clipboard carries text **or** an HTML payload.
///
/// This is the union of the shapes `paste` can actually consume: its
/// self-round-trip (step 1) and external-HTML (step 2) branches insert
/// from `get_html()` independently of any plain-text companion, and the
/// plain-text branch (step 3) handles the rest. Probing only
/// `has_text()` would wrongly report an HTML-only clipboard — an app
/// that published `text/html` without a `text/plain` alternative — as
/// un-pasteable, greying out a Paste command that would in fact succeed.
///
/// Returns `false` when no clipboard backend is installed (headless /
/// feature-off builds), matching `paste`'s own silent no-op. `has_html`
/// can round-trip to the X11 selection owner, so this is a
/// menu-build-time query, never a per-frame one.
pub(crate) fn can_paste(ctx: &EventContext) -> bool {
    ctx.app_state::<ClipboardHandle>()
        .map(|cb| cb.has_text() || cb.has_html())
        .unwrap_or(false)
}

/// Insert plain text that may contain line breaks, splitting on
/// `\n` into separate blocks. `text-document::TextCursor::insert_text`
/// treats `\n` as a literal scalar inside one block, so pasting a
/// multi-line clipboard payload without this split leaves every line
/// fused into one paragraph. Normalises `\r\n` and bare `\r` first so
/// Windows- and classic-Mac clipboards round-trip cleanly.
fn insert_multiline_plain(state: &mut EditorState, text: &str) {
    let normalised = text.replace("\r\n", "\n").replace('\r', "\n");
    // One paste, one undo step. Without the edit block each line is its own
    // entry, so undoing a four-line paste took four presses of Ctrl+Z and left
    // three quarters of it behind — every other multi-step mutation in the
    // editor is grouped for exactly this reason.
    state.cursor.begin_edit_block();
    // Counted from the payload rather than measured afterwards, because this is
    // the one paste path where the text itself is in hand. The newlines count:
    // a pasted paragraph break is text that arrived, and dropping it would make
    // a five-paragraph paste report four characters short for no reason a reader
    // could see.
    state.report_inserted(super::EditSource::Clipboard, &normalised);
    let mut lines = normalised.split('\n');
    if let Some(first) = lines.next() {
        let _ = state.cursor.insert_text(first);
    }
    for line in lines {
        let _ = state.cursor.insert_block();
        if !line.is_empty() {
            let _ = state.cursor.insert_text(line);
        }
    }
    state.cursor.end_edit_block();
}

/// How many characters an insertion actually added, measured around it.
///
/// For the two rich paste paths, where what goes in is a fragment or a parsed
/// HTML tree and the plain-text length is not in hand. A bare before/after delta
/// would **undercount every paste over a selection** — the insert removes the
/// selection first, so the delta is what arrived minus what went — which is the
/// common case rather than a corner. Adding the replaced length back makes it
/// exact:
///
/// ```text
/// after = before - replaced + arrived   ⇒   arrived = after - before + replaced
/// ```
fn report_measured<R>(state: &mut EditorState, insert: impl FnOnce(&mut EditorState) -> R) -> R {
    let measuring = state.on_text_inserted.is_some();
    // Nothing is listening, so nothing is measured: two `character_count()`
    // walks per paste is not a cost a build with no consumer should pay.
    if !measuring {
        return insert(state);
    }
    // An unreadable selection counts as none: undercounting a paste is a smaller
    // wrong than refusing to report it, and this cannot fail in practice — the
    // selection is the caret's own range in a document already in memory.
    let replaced = state
        .cursor
        .selected_text()
        .map(|s| s.chars().count())
        .unwrap_or(0);
    let before = state.document.character_count();
    let out = insert(state);
    let after = state.document.character_count();
    let arrived = (after + replaced).saturating_sub(before);
    state.report_inserted_chars(super::EditSource::Clipboard, arrived);
    out
}
