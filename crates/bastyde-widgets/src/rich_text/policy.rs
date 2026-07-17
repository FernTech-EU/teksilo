// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Policy bundles for RichTextEditor construction presets.
//!
//! A `PolicyBundle` captures every decision that varies between the
//! `editor()` and `read_only()` constructors so each widget never
//! consults a `read_only: bool` flag. The four independent dimensions
//! mirror §27.10.1: command filter, caret behaviour, accessibility role,
//! and clipboard capabilities.

/// Commands the keyboard handler may emit. Editing commands map to
/// `TextCursor` mutations; navigation commands are always accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditCommandKind {
    // Editing
    InsertChar,
    InsertBlock,
    /// Explicit block-insert, bypassing any Enter-in-table-cell
    /// navigation (Ctrl+Enter). Separate from `InsertBlock` so an
    /// app that wants only one of the two behaviours can gate them
    /// independently through the `CommandFilter`.
    InsertBlockForced,
    /// Literal tab insertion (Tab key outside tables and lists).
    InsertTab,
    /// Navigate to an adjacent table cell (Tab / Shift+Tab when the
    /// caret is inside a table).
    NavigateTableCell,
    /// Enter inside a table cell: navigate to the cell below (or
    /// step out of the table on the last row).
    NavigateTableCellDown,
    /// Exit a list item (Backspace at block-start, indent 0).
    ExitList,
    /// Pop the cursor out of the innermost enclosing blockquote frame
    /// (Backspace at the first position of the first quoted block;
    /// Enter on an empty quoted paragraph; Delete at the last position
    /// of the last quoted block).
    ExitFrame,
    /// Wrap the current block (or selection) in a blockquote, or
    /// unwrap if already inside one. Toolbar / Ctrl+Shift+Q.
    ToggleBlockquote,
    /// Tab inside a blockquote (no list active) to nest deeper.
    IncreaseBlockquoteDepth,
    /// Shift+Tab inside a blockquote (no list active) to nest shallower.
    DecreaseBlockquoteDepth,
    DeletePrev,
    DeleteNext,
    DeleteWordLeft,
    DeleteWordRight,
    IndentBlock,
    DedentBlock,
    ToggleBold,
    ToggleItalic,
    ToggleUnderline,
    Undo,
    Redo,

    // Navigation (always allowed regardless of policy)
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    MoveWordLeft,
    MoveWordRight,
    MoveHome,
    MoveEnd,
    MoveDocStart,
    MoveDocEnd,
    PageUp,
    PageDown,
    SelectLeft,
    SelectRight,
    SelectUp,
    SelectDown,
    SelectWordLeft,
    SelectWordRight,
    SelectHome,
    SelectEnd,
    SelectDocStart,
    SelectDocEnd,
    SelectAll,

    // Clipboard (allowed subset depends on policy)
    Copy,
    Cut,
    Paste,
    /// Paste as plain text — strips any rich fragment or HTML payload
    /// and inserts only the plain-text portion. Bound to Ctrl+Shift+V
    /// (⌘⇧V on macOS). Distinct from [`Paste`](Self::Paste) so the
    /// command filter and `ClipboardPolicy` can gate it independently
    /// (e.g. a future "no-paste" preset could still allow explicit
    /// plain-text pasting).
    PasteUnformatted,
}

impl crate::common::editor_runtime::EditorCommand for EditCommandKind {
    /// True for commands that modify the document. Navigation and copy
    /// never mutate.
    fn mutates_document(&self) -> bool {
        matches!(
            self,
            Self::InsertChar
                | Self::InsertBlock
                | Self::InsertBlockForced
                | Self::InsertTab
                | Self::ExitList
                | Self::ExitFrame
                | Self::ToggleBlockquote
                | Self::IncreaseBlockquoteDepth
                | Self::DecreaseBlockquoteDepth
                | Self::NavigateTableCell
                | Self::NavigateTableCellDown
                | Self::DeletePrev
                | Self::DeleteNext
                | Self::DeleteWordLeft
                | Self::DeleteWordRight
                | Self::IndentBlock
                | Self::DedentBlock
                | Self::ToggleBold
                | Self::ToggleItalic
                | Self::ToggleUnderline
                | Self::Undo
                | Self::Redo
                | Self::Cut
                | Self::Paste
                | Self::PasteUnformatted
        )
    }
}

// The policy machinery — command filter, AT role, clipboard surface, and the
// bundle that ties them together — is shared with every other text surface and
// lives in `common::editor_runtime`. Only the *vocabulary* above is
// rich-text-specific. Re-exported here because these are their public names.
pub use crate::common::editor_runtime::{
    AccessibilityRole, CaretPolicy, ClipboardPolicy, CommandFilter, EDITOR_PRESET, PolicyBundle,
    READ_ONLY_PRESET,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_filter_blocks_mutations() {
        let f = CommandFilter::ReadOnly;
        assert!(!f.accepts(EditCommandKind::InsertChar));
        assert!(!f.accepts(EditCommandKind::DeletePrev));
        assert!(!f.accepts(EditCommandKind::ToggleBold));
        assert!(!f.accepts(EditCommandKind::Cut));
        assert!(!f.accepts(EditCommandKind::Paste));
        assert!(
            !f.accepts(EditCommandKind::PasteUnformatted),
            "read-only must reject PasteUnformatted — it mutates the document"
        );
        assert!(!f.accepts(EditCommandKind::Undo));
    }

    #[test]
    fn paste_unformatted_policy_mirrors_paste() {
        let full = ClipboardPolicy::Full;
        let ro = ClipboardPolicy::CopyAndSelectAllOnly;
        assert!(full.allows_paste());
        assert!(full.allows_paste_unformatted());
        assert!(!ro.allows_paste());
        assert!(!ro.allows_paste_unformatted());
    }

    #[test]
    fn read_only_filter_allows_navigation_and_copy() {
        let f = CommandFilter::ReadOnly;
        assert!(f.accepts(EditCommandKind::MoveLeft));
        assert!(f.accepts(EditCommandKind::SelectWordRight));
        assert!(f.accepts(EditCommandKind::MoveDocEnd));
        assert!(f.accepts(EditCommandKind::Copy));
        assert!(f.accepts(EditCommandKind::SelectAll));
    }

    #[test]
    fn editor_filter_accepts_everything() {
        let f = CommandFilter::All;
        for cmd in [
            EditCommandKind::InsertChar,
            EditCommandKind::Paste,
            EditCommandKind::Undo,
            EditCommandKind::Copy,
            EditCommandKind::MoveHome,
        ] {
            assert!(f.accepts(cmd));
        }
    }

    #[test]
    fn presets_report_read_only_correctly() {
        assert!(!EDITOR_PRESET.is_read_only());
        assert!(READ_ONLY_PRESET.is_read_only());
    }
}
