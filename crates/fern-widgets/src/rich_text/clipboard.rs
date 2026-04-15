//! System clipboard integration for the rich text editor.
//!
//! Three free functions — `copy`, `cut`, `paste` — implement the
//! godot reference's in-process rich fragment preservation pattern at
//! [rich_text_edit.rs:2048-2087](../../../../godot-rich-text/src/rich_text_edit.rs#L2048-L2087).
//!
//! The functions talk to a `ClipboardHandle` retrieved via
//! `EventContext::app_state::<ClipboardHandle>()`. In headless tests
//! or builds without the `fern-app/clipboard` feature the handle is
//! absent and copy/cut/paste silently no-op — the same behaviour as
//! trying to paste an image into a pure-text editor. No panic, no
//! error propagation — the user sees nothing happen and the command
//! filter's UI affordance drives the expectation.
//!
//! Self-round-trip detection uses plain-text equality between the
//! stored fragment's plain text and the system clipboard text. This is
//! a known limitation from the reference: two copies of identical
//! plain text with different formatting lose their distinction. A
//! post-M8 fix would embed an internal marker header in the system
//! clipboard to disambiguate. Until then, paste-into-the-same-editor
//! is rich, paste-from-another-app is plain, and that's the
//! intentional tradeoff.

use fern_core::widget::EventContext;
use fern_platform::clipboard::ClipboardHandle;

use super::state::EditorState;

/// Copy the current selection to the system clipboard. No-op when
/// there is no selection — matches editor convention (the menu item
/// and the Ctrl+C shortcut both stay silent rather than capturing an
/// empty fragment).
pub(crate) fn copy(state: &mut EditorState, ctx: &EventContext) {
    if !state.cursor.has_selection() {
        return;
    }
    let fragment = state.cursor.selection();
    let plain = fragment.to_plain_text().to_string();
    if let Some(cb) = ctx.app_state::<ClipboardHandle>() {
        let _ = cb.set_text(&plain);
    }
    state.rich_clipboard_fragment = Some(fragment);
    state.rich_clipboard_plain = Some(plain);
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

/// Paste from the system clipboard. If the stored fragment's plain
/// text matches what the system clipboard reports, reinsert the rich
/// fragment (preserves formatting). Otherwise fall back to plain-text
/// insertion of whatever the system clipboard has. Clears any
/// existing selection after insertion so the caret sits at the end of
/// the pasted content rather than keeping the newly inserted range
/// selected — matches godot behaviour.
pub(crate) fn paste(state: &mut EditorState, ctx: &EventContext) {
    let Some(cb) = ctx.app_state::<ClipboardHandle>() else {
        return;
    };
    let Ok(system) = cb.get_text() else {
        return;
    };
    if system.is_empty() {
        return;
    }
    let use_rich = state.rich_clipboard_plain.as_deref() == Some(system.as_str())
        && state.rich_clipboard_fragment.is_some();
    if use_rich {
        let fragment = state.rich_clipboard_fragment.as_ref().unwrap().clone();
        let _ = state.cursor.insert_fragment(&fragment);
    } else {
        let _ = state.cursor.insert_text(&system);
    }
    state.cursor.clear_selection();
    state.pending_text_changed = true;
}
