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

impl EditCommandKind {
    /// True for commands that modify the document. Navigation and copy
    /// never mutate.
    pub fn mutates_document(&self) -> bool {
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

/// Command filter consulted before any cursor call in `keyboard.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandFilter {
    /// Every command accepted (editor preset).
    All,
    /// Mutating commands rejected; navigation and copy/select-all accepted
    /// (read-only preset).
    ReadOnly,
}

impl CommandFilter {
    pub fn accepts(&self, cmd: EditCommandKind) -> bool {
        match self {
            Self::All => true,
            Self::ReadOnly => {
                if !cmd.mutates_document() {
                    return true;
                }
                // Navigation-adjacent commands that only *read* — already
                // covered above via `mutates_document()`. Cut/Paste fall
                // through here and are rejected.
                false
            }
        }
    }
}

/// How the caret is presented. `read_only` hides the caret entirely
/// (`Hidden`); custom presets may use `StaticVisible` for a focusable
/// surface that shows a cursor without animation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaretPolicy {
    /// Caret blinks while the widget has focus (editor preset).
    Blinking,
    /// Caret visible but not blinking. Use for focusable surfaces that
    /// need a visible insertion point without distracting animation —
    /// e.g. a custom read-only editor the user can navigate and copy
    /// but that must not suggest editability. Neither built-in preset
    /// uses this value; construct a custom `PolicyBundle` to opt in.
    StaticVisible,
    /// Caret not rendered at all (read-only preset).
    Hidden,
}

/// Drives the AccessKit role selection in `Widget::accessibility()`. The
/// editor preset reports `MultilineTextInput` and handles
/// `Action::SetValue`; read-only reports `Document` and never declares
/// `Action::SetValue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessibilityRole {
    Editor,
    Document,
}

/// Clipboard surface exposed by the widget. `CopyAndSelectAllOnly` is
/// the read-only preset (no cut/paste). The command filter already
/// rejects cut/paste for ReadOnly, so this enum drives UI affordances
/// such as disabled menu items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardPolicy {
    Full,
    CopyAndSelectAllOnly,
}

impl ClipboardPolicy {
    pub fn allows_cut(&self) -> bool {
        matches!(self, Self::Full)
    }
    pub fn allows_paste(&self) -> bool {
        matches!(self, Self::Full)
    }
    /// `PasteUnformatted` mirrors `Paste` today: both are gated by the
    /// same policy bit. Kept as a separate accessor so a future preset
    /// that admits plain-only paste while rejecting rich paste can
    /// diverge without changing call sites.
    pub fn allows_paste_unformatted(&self) -> bool {
        matches!(self, Self::Full)
    }
    /// Always `true` — copying is allowed under every policy, including
    /// `CopyAndSelectAllOnly`. Provided as a method (rather than a
    /// hardcoded literal at call sites) so a future preset can diverge
    /// without changing callers.
    pub fn allows_copy(&self) -> bool {
        true
    }
}

/// One bundle per construction preset. Single-source-of-truth for the
/// four independent decisions.
#[derive(Debug, Clone, Copy)]
pub struct PolicyBundle {
    pub command_filter: CommandFilter,
    pub caret_policy: CaretPolicy,
    pub access_role: AccessibilityRole,
    pub clipboard_policy: ClipboardPolicy,
}

impl PolicyBundle {
    pub const fn is_read_only(&self) -> bool {
        matches!(self.access_role, AccessibilityRole::Document)
    }
}

/// The full editor preset: every command accepted, caret blinks,
/// `MultilineTextInput` role, full clipboard support.
pub const EDITOR_PRESET: PolicyBundle = PolicyBundle {
    command_filter: CommandFilter::All,
    caret_policy: CaretPolicy::Blinking,
    access_role: AccessibilityRole::Editor,
    clipboard_policy: ClipboardPolicy::Full,
};

/// The read-only preset: only navigation + copy/select-all,
/// `Document` role, no cut/paste. The caret is hidden entirely —
/// view-only widgets ship without any caret affordance.
/// Applications that need a focusable read-only surface with a
/// visible caret can construct a custom preset via `PolicyBundle`.
pub const READ_ONLY_PRESET: PolicyBundle = PolicyBundle {
    command_filter: CommandFilter::ReadOnly,
    caret_policy: CaretPolicy::Hidden,
    access_role: AccessibilityRole::Document,
    clipboard_policy: ClipboardPolicy::CopyAndSelectAllOnly,
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
