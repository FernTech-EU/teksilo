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
}

impl EditCommandKind {
    /// True for commands that modify the document. Navigation and copy
    /// never mutate.
    pub fn mutates_document(&self) -> bool {
        matches!(
            self,
            Self::InsertChar
                | Self::InsertBlock
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

/// How the caret is presented. `read_only` uses `StaticVisible` so
/// keyboard users can still see the focus point; a pure viewer would
/// pick `Hidden`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaretPolicy {
    /// Caret blinks while the widget has focus (editor preset).
    Blinking,
    /// Caret visible but not blinking (read-only preset).
    StaticVisible,
    /// Caret not rendered at all.
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
/// view-only widgets ship without any caret affordance (§27.10.1).
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
        assert!(!f.accepts(EditCommandKind::Undo));
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
