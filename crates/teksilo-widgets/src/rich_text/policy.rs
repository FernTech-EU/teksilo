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
    /// True for commands that can take text away — a delete, a cut, or a
    /// history step that reverts one.
    ///
    /// Deliberately much narrower than [`mutates_document`](Self::mutates_document):
    ///
    /// * **Inserts are never regressive.** `InsertChar`/`InsertBlock`/
    ///   `InsertTab`/`Paste` only add — the type-over case (inserting *over* a
    ///   selection) is handled by collapsing the selection at the insert site,
    ///   not by rejecting the keystroke.
    /// * **Structure commands are not regressive.** `ExitList`, `ExitFrame`,
    ///   the blockquote depth pair and `IndentBlock`/`DedentBlock` re-shape a
    ///   block without dropping any of its characters. Rejecting them would
    ///   leave a writer stuck inside a list or a quote with no way back out —
    ///   punishing them for a structure they created, which is not what
    ///   "don't delete your prose" means.
    /// * **Formatting is not regressive.** Turning bold off changes how existing
    ///   text looks, never whether it is there.
    /// * **`Redo` is regressive**, not just `Undo`: it can re-apply a deletion
    ///   made before the mode was switched on.
    fn is_regressive(&self) -> bool {
        matches!(
            self,
            Self::DeletePrev
                | Self::DeleteNext
                | Self::DeleteWordLeft
                | Self::DeleteWordRight
                | Self::Cut
                | Self::Undo
                | Self::Redo
        )
    }

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

    /// The whole point of the forward-only filter: the draft may grow but
    /// nothing already written can be taken away from the keyboard.
    #[test]
    fn forward_only_filter_blocks_every_regressive_command() {
        let f = CommandFilter::ForwardOnly;
        for cmd in [
            EditCommandKind::DeletePrev,
            EditCommandKind::DeleteNext,
            EditCommandKind::DeleteWordLeft,
            EditCommandKind::DeleteWordRight,
            EditCommandKind::Cut,
            EditCommandKind::Undo,
        ] {
            assert!(
                !f.accepts(cmd),
                "{cmd:?} takes text away and must be rejected"
            );
        }
        assert!(
            !f.accepts(EditCommandKind::Redo),
            "redo can re-apply a deletion made before the mode was switched on"
        );
    }

    /// A forward-only surface is still an editor: everything additive, every
    /// navigation, and — critically — the structure commands that let a writer
    /// leave a list or a blockquote must all keep working.
    #[test]
    fn forward_only_filter_allows_additive_navigation_and_structure() {
        let f = CommandFilter::ForwardOnly;
        for cmd in [
            EditCommandKind::InsertChar,
            EditCommandKind::InsertBlock,
            EditCommandKind::InsertBlockForced,
            EditCommandKind::InsertTab,
            EditCommandKind::Paste,
            EditCommandKind::PasteUnformatted,
            EditCommandKind::Copy,
            EditCommandKind::SelectAll,
            EditCommandKind::MoveLeft,
            EditCommandKind::MoveDocEnd,
            EditCommandKind::SelectWordRight,
            EditCommandKind::ToggleBold,
            EditCommandKind::ToggleItalic,
            EditCommandKind::ToggleUnderline,
            EditCommandKind::IndentBlock,
            EditCommandKind::DedentBlock,
            EditCommandKind::ToggleBlockquote,
            EditCommandKind::IncreaseBlockquoteDepth,
            EditCommandKind::DecreaseBlockquoteDepth,
            EditCommandKind::NavigateTableCell,
            EditCommandKind::NavigateTableCellDown,
        ] {
            assert!(
                f.accepts(cmd),
                "{cmd:?} adds or navigates and must be allowed"
            );
        }
        assert!(
            f.accepts(EditCommandKind::ExitList) && f.accepts(EditCommandKind::ExitFrame),
            "popping out of a list or a quote drops no characters — blocking it \
             would trap the writer inside the structure with no way out"
        );
    }

    /// `is_regressive` must be strictly narrower than `mutates_document`:
    /// anything that takes text away necessarily changes the document, and the
    /// forward-only filter must accept strictly more than the read-only one.
    #[test]
    fn regressive_is_a_strict_subset_of_mutating() {
        let all = [
            EditCommandKind::InsertChar,
            EditCommandKind::InsertBlock,
            EditCommandKind::InsertBlockForced,
            EditCommandKind::InsertTab,
            EditCommandKind::NavigateTableCell,
            EditCommandKind::NavigateTableCellDown,
            EditCommandKind::ExitList,
            EditCommandKind::ExitFrame,
            EditCommandKind::ToggleBlockquote,
            EditCommandKind::IncreaseBlockquoteDepth,
            EditCommandKind::DecreaseBlockquoteDepth,
            EditCommandKind::DeletePrev,
            EditCommandKind::DeleteNext,
            EditCommandKind::DeleteWordLeft,
            EditCommandKind::DeleteWordRight,
            EditCommandKind::IndentBlock,
            EditCommandKind::DedentBlock,
            EditCommandKind::ToggleBold,
            EditCommandKind::ToggleItalic,
            EditCommandKind::ToggleUnderline,
            EditCommandKind::Undo,
            EditCommandKind::Redo,
            EditCommandKind::MoveLeft,
            EditCommandKind::Copy,
            EditCommandKind::Cut,
            EditCommandKind::Paste,
            EditCommandKind::PasteUnformatted,
        ];
        use crate::common::editor_runtime::EditorCommand;
        let mut strictly_narrower = false;
        for cmd in all {
            if cmd.is_regressive() {
                assert!(
                    cmd.mutates_document(),
                    "{cmd:?} claims to take text away without mutating the document"
                );
            }
            if cmd.mutates_document() && !cmd.is_regressive() {
                strictly_narrower = true;
            }
            // Whatever read-only permits, forward-only permits too.
            if CommandFilter::ReadOnly.accepts(cmd) {
                assert!(
                    CommandFilter::ForwardOnly.accepts(cmd),
                    "{cmd:?} is allowed read-only but rejected forward-only"
                );
            }
        }
        assert!(strictly_narrower, "the two predicates collapsed into one");
    }

    /// Type-over is a delete that happens below the command layer, so the
    /// filter has to tell insert sites when to collapse the selection first.
    #[test]
    fn only_forward_only_collapses_the_selection_before_inserting() {
        assert!(CommandFilter::ForwardOnly.collapses_selection_before_insert());
        assert!(!CommandFilter::All.collapses_selection_before_insert());
        assert!(!CommandFilter::ReadOnly.collapses_selection_before_insert());
    }

    /// An assistive-technology `SetValue` replaces the whole document; only an
    /// unrestricted surface may do that.
    #[test]
    fn only_the_unrestricted_filter_allows_wholesale_replacement() {
        assert!(CommandFilter::All.allows_wholesale_replacement());
        assert!(!CommandFilter::ForwardOnly.allows_wholesale_replacement());
        assert!(!CommandFilter::ReadOnly.allows_wholesale_replacement());
    }

    /// A forward-only preset must stay an *editor* in every other dimension:
    /// hiding the caret or reporting `Document` to a screen reader would turn a
    /// drafting mode into a viewer.
    #[test]
    fn with_command_filter_changes_only_the_filter() {
        let fwd = EDITOR_PRESET.with_command_filter(CommandFilter::ForwardOnly);
        assert_eq!(fwd.command_filter, CommandFilter::ForwardOnly);
        assert!(!fwd.is_read_only());
        assert_eq!(fwd.caret_policy, EDITOR_PRESET.caret_policy);
        assert_eq!(fwd.access_role, EDITOR_PRESET.access_role);
        assert_eq!(fwd.clipboard_policy, EDITOR_PRESET.clipboard_policy);
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
