//! System clipboard integration for the rich text editor.
//!
//! Three free functions — `copy`, `cut`, `paste` — implement the
//! godot reference's in-process rich fragment preservation pattern at
//! [rich_text_edit.rs:2048-2087](../../../../godot-rich-text/src/rich_text_edit.rs#L2048-L2087).
//!
//! The functions talk to a `ClipboardHandle` retrieved via
//! `EventContext::app_state::<ClipboardHandle>()`. In headless tests
//! or builds without the `bastyde-app/clipboard` feature the handle is
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

use std::time::{SystemTime, UNIX_EPOCH};

use bastyde_core::widget::EventContext;
use bastyde_platform::clipboard::ClipboardHandle;

use super::state::EditorState;

/// HTML comment prefix embedded in the clipboard payload to flag a
/// self-copy. The trailing hex token is regenerated per copy.
const MARKER_PREFIX: &str = "<!--bastyde-rtc:";
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
/// copy/paste pair re-inserts the original [`DocumentFragment`]
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
    if let Some(cb) = ctx.app_state::<ClipboardHandle>() {
        // `set_html` writes both payloads in one transaction. Backends
        // without native HTML support see the default trait body and
        // fall back to `set_text(plain)`.
        let _ = cb.set_html(&html, &plain);
    }
    state.rich_clipboard_fragment = Some(fragment);
    state.rich_clipboard_plain = Some(plain);
    state.rich_clipboard_marker = Some(marker);
}

/// Generate a unique per-copy marker. Mixes a monotonic nanosecond
/// timestamp with the fragment's memory identity so back-to-back
/// copies in the same millisecond still differ. Not a cryptographic
/// identifier — the only requirement is that an unrelated app can't
/// plausibly emit the same string.
fn new_marker() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:032x}")
}

/// Extract the marker token from a clipboard HTML payload written by
/// `copy`. Returns `None` for any payload that doesn't start with
/// `<!--bastyde-rtc:...-->` — which covers every payload emitted by any
/// other app.
fn payload_marker(html: &str) -> Option<&str> {
    let rest = html.strip_prefix(MARKER_PREFIX)?;
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
/// 1. **Self-round-trip rich fragment** — if the system clipboard's
///    plain text matches what this editor last copied, reinsert the
///    stored [`DocumentFragment`] so intra-editor formatting round-trips
///    losslessly (retains table cells, heading levels, spans that don't
///    serialise into HTML losslessly).
/// 2. **External HTML payload** — if the clipboard carries `text/html`
///    / `CF_HTML` / `public.html`, parse it into a `DocumentFragment`
///    via text-document and insert. This is the path that makes
///    rich paste *from another app* work (Firefox, Word, Google Docs,
///    etc.).
/// 3. **Plain-text fallback** — when neither rich path applies, split
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
        && let Some(frag) = state.rich_clipboard_fragment.as_ref()
    {
        let _ = state.cursor.insert_fragment(&frag.clone());
        state.cursor.clear_selection();
        state.pending_text_changed = true;
        return;
    }

    // 2. External HTML payload. Re-uses the payload we already fetched
    //    above when present; the marker check proved it's not ours.
    //    `TextCursor::insert_html` parses the HTML into a
    //    `DocumentFragment` (via text-document's
    //    `DocumentFragment::from_html`) and inserts it at the caret.
    if let Some(html) = html_payload.as_deref()
        && state.cursor.insert_html(html).is_ok()
    {
        state.cursor.clear_selection();
        state.pending_text_changed = true;
        return;
    }

    // 3. Plain-text fallback.
    let Ok(text) = cb.get_text() else {
        return;
    };
    if text.is_empty() {
        return;
    }
    insert_multiline_plain(state, &text);
    state.cursor.clear_selection();
    state.pending_text_changed = true;
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
    let Ok(text) = cb.get_text() else {
        return;
    };
    if text.is_empty() {
        return;
    }
    insert_multiline_plain(state, &text);
    state.cursor.clear_selection();
    state.pending_text_changed = true;
}

/// Insert plain text that may contain line breaks, splitting on
/// `\n` into separate blocks. `text-document::TextCursor::insert_text`
/// treats `\n` as a literal scalar inside one block, so pasting a
/// multi-line clipboard payload without this split leaves every line
/// fused into one paragraph. Normalises `\r\n` and bare `\r` first so
/// Windows- and classic-Mac clipboards round-trip cleanly.
fn insert_multiline_plain(state: &mut EditorState, text: &str) {
    let normalised = text.replace("\r\n", "\n").replace('\r', "\n");
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
}
