// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The text widgets' half of `TEKSILO_TEXT_INPUT_TRACE`: what a text widget
//! did with what the platform's input system sent it, written to stderr in
//! the platform trace's format (`[text-input] widget: ...`), so one log reads
//! in order. Diagnostic only, and free while the variable is unset.

use std::sync::OnceLock;

use teksilo_text::text_document::TextDocument;

/// Whether `TEKSILO_TEXT_INPUT_TRACE` is set to anything but `0`. Read once.
pub(crate) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("TEKSILO_TEXT_INPUT_TRACE").is_some_and(|value| value != "0")
    })
}

/// Write one trace line, if tracing is on.
pub(crate) fn trace(args: std::fmt::Arguments<'_>) {
    if enabled() {
        eprintln!("[text-input] widget: {args}");
    }
}

/// Run an undo or a redo (`step`) on `document`, tracing what it did: its
/// result, whether a step was there to take and is still there, and whether
/// the text changed. An undo that answers an error leaves its step on the
/// stack and changes nothing, which is otherwise silent.
pub(crate) fn traced_history_step(
    document: &TextDocument,
    what: &str,
    step: impl FnOnce() -> Result<(), teksilo_text::text_document::DocumentError>,
) {
    if !enabled() {
        let _ = step();
        return;
    }
    let before = (
        document.can_undo(),
        document.can_redo(),
        document.to_addressable_text().ok(),
    );
    let result = step();
    let after = (
        document.can_undo(),
        document.can_redo(),
        document.to_addressable_text().ok(),
    );
    trace(format_args!(
        "{what} -> {result:?}; can undo {} -> {}, can redo {} -> {}, text {}",
        before.0,
        after.0,
        before.1,
        after.1,
        if before.2 == after.2 {
            "unchanged"
        } else {
            "changed"
        },
    ));
}
