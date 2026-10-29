// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What the input method made of a key press: the half that needs no
//! platform.
//!
//! A window hands each key press to the platform's input method, and the
//! input method answers by acting on the window: it inserts text, marks a
//! composition, ends one, or names a command (an arrow, Escape). winit then
//! reports the key with the characters of its key event unless it decides
//! the input method composed. Three answers get that wrong:
//!
//! - **The input method did nothing**: it took the key for itself. On macOS
//!   the press-and-hold accent popup takes the auto-repeats of the held
//!   letter and the key that picks an accent, then inserts the accent on its
//!   own. The key's text was typed beside the accent ("eeê" for "ê").
//! - **The input method committed text of its own for the key and handed
//!   nothing back.** macOS 2-Set Korean answers Space with nothing composed
//!   by marking a space, committing it, then emptying its marked text. winit
//!   tells a composition from typing by comparing its IME state before and
//!   after the key, and that last empty marked text brings the state back to
//!   where it started, so it reported the key with its text too: two spaces.
//! - **The input method typed the key as another character.** With nothing
//!   composed, macOS Kotoeri inserts a full-width space (U+3000) for Space and
//!   Pinyin a full-width "；" for ";". winit drops an insertion made with no
//!   marked text and reports the key with its own characters, so the field
//!   got " " and ";" where TextEdit gets the input method's characters.
//!
//! [`KeyPress`] records, for one key press, whether the press went to the
//! input method, the characters it was handed, and what the input method did
//! while it had it. [`taken`](KeyPress::taken) is the verdict.

#![cfg_attr(
    not(target_os = "macos"),
    allow(dead_code, reason = "only the macOS view tells, for now")
)]

use super::TakenKey;

/// What the input method did with one key press.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct KeyPress {
    /// The press was handed to the input method (on macOS,
    /// `interpretKeyEvents:` ran for it).
    interpreted: bool,
    /// How many hand-overs are running: the input method still has the key.
    interpreting: u32,
    /// The characters of the key event the input method was handed: what
    /// winit types for the key when it reports it.
    characters: Option<String>,
    /// The input method acted on the window while it had the key.
    acted: bool,
    /// It committed the window's composition while it had the key, and typed
    /// nothing after that.
    committed: bool,
    /// What it typed (inserted with no composition held) while it had the
    /// key, since its last commit, in order.
    typed: String,
    /// It named a command while it had the key: the key is handed back as a
    /// key, and winit reports it.
    commanded: bool,
    /// The window's marked text is what was left of a composition committed
    /// in this press: winit keeps it until the press is over, though the
    /// field has taken the commit. Set once the commit is through
    /// ([`note_commit_through`](Self::note_commit_through)), cleared by the
    /// next composing call.
    marked_left_over: bool,
}

impl KeyPress {
    /// The press is handed to the input method.
    pub(crate) fn begin_interpreting(&mut self) {
        self.interpreted = true;
        self.interpreting = self.interpreting.saturating_add(1);
    }

    /// The input method has handed the press back.
    pub(crate) fn end_interpreting(&mut self) {
        self.interpreting = self.interpreting.saturating_sub(1);
    }

    /// The characters of the key event the input method is handed (`None`
    /// for an event with none). The first hand-over's count: a nested one
    /// is the same key.
    pub(crate) fn note_characters(&mut self, characters: Option<&str>) {
        if !self.interpreted {
            self.characters = characters.map(str::to_owned);
        }
    }

    /// The input method composed: it marked text, emptied its marked text,
    /// or ended its composition. That neither types the key nor commits
    /// anything, and leaves a commit made before it standing.
    ///
    /// Like every `note_` method, it counts only while the input method has
    /// the key: the same calls made before it has the key or after it handed
    /// it back are not its answer to this press.
    pub(crate) fn note_composed(&mut self) {
        // Any marking replaces what a commit left: it is marked text again.
        self.marked_left_over = false;
        if self.interpreting > 0 {
            self.acted = true;
        }
    }

    /// A commit of the window's composition has gone through (winit's
    /// `insertText:` returned, or the hook committed in winit's place):
    /// the marked text winit still holds is a leftover until the next
    /// composing call. Noted after the commit, never before it: winit asks
    /// `hasMarkedText` while it commits, and must hear yes then.
    pub(crate) fn note_commit_through(&mut self) {
        self.marked_left_over = true;
    }

    /// Whether the marked text winit holds is a commit's leftover, which the
    /// view does not count as marked text (`hasMarkedText` answers no, as a
    /// native text view does after a commit).
    pub(crate) fn marked_text_is_left_over(&self) -> bool {
        self.marked_left_over
    }

    /// The input method committed the composition the window held. What it
    /// typed before is not counted: the press reads as the commit (not
    /// measured: no input method was seen to type, then commit, in one key).
    pub(crate) fn note_committed(&mut self) {
        if self.interpreting > 0 {
            self.acted = true;
            self.committed = true;
            self.typed.clear();
        }
    }

    /// The input method inserted `text` with no composition held: it typed
    /// the key, whatever it committed before, as `text`. The window types
    /// what it typed in place of the key's characters when the two differ.
    /// An empty `text` (the platform's caller passes one for a string it
    /// does not take, one starting with a control character) types the key
    /// as its characters.
    pub(crate) fn note_typed(&mut self, text: &str) {
        if self.interpreting > 0 {
            self.acted = true;
            self.committed = false;
            self.typed.push_str(text);
        }
    }

    /// The input method named a command (an arrow, Escape, Return): the key
    /// goes on as a key, before or after a commit, with its characters.
    /// winit reports it (its `forward_key_to_app`), and so does this.
    pub(crate) fn note_commanded(&mut self) {
        if self.interpreting > 0 {
            self.acted = true;
            self.commanded = true;
        }
    }

    /// What the input method took of the press, if anything: see
    /// [`TakenKey`]. `None` for a press the input method never saw (a window
    /// with no input method enabled), for one it still has, and for one it
    /// handed back as it was: it typed the key's own characters, or named a
    /// command.
    pub(crate) fn taken(&self) -> Option<TakenKey> {
        if !self.interpreted || self.interpreting > 0 || self.commanded {
            None
        } else if !self.acted {
            Some(TakenKey::Swallowed)
        } else if self.committed {
            Some(TakenKey::Committed)
        } else if !self.typed.is_empty()
            && self.typed != self.characters.as_deref().unwrap_or_default()
        {
            Some(TakenKey::Substituted(self.typed.clone()))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A press of a key whose event carries `characters`, handed to the
    /// input method, which does `acts` while it has it.
    fn press_of(characters: &str, acts: impl FnOnce(&mut KeyPress)) -> KeyPress {
        let mut press = KeyPress::default();
        press.note_characters(Some(characters));
        press.begin_interpreting();
        acts(&mut press);
        press.end_interpreting();
        press
    }

    /// A press of Space.
    fn press(acts: impl FnOnce(&mut KeyPress)) -> KeyPress {
        press_of(" ", acts)
    }

    /// After a commit inside a press, winit's marked text is a leftover
    /// until the input method marks again: Korean commits 한 (Option+Return)
    /// and then marks 韓 over it (Hanja) in the same press.
    #[test]
    fn a_commits_marked_text_is_a_leftover_until_the_next_mark() {
        let mut press = KeyPress::default();
        press.begin_interpreting();
        press.note_committed();
        assert!(
            !press.marked_text_is_left_over(),
            "not before the commit is through: winit asks while it commits"
        );
        press.note_commit_through();
        assert!(press.marked_text_is_left_over());
        press.note_composed();
        assert!(!press.marked_text_is_left_over(), "marked again");
        press.end_interpreting();
    }

    #[test]
    fn a_key_the_input_method_never_saw_is_not_taken() {
        assert_eq!(KeyPress::default().taken(), None, "no input method");
    }

    #[test]
    fn a_typed_letter_is_not_taken() {
        // A layout with no input method, or one that inserts the key's own
        // character (`insertText:` with nothing marked): winit types it from
        // the key press, exactly as before.
        assert_eq!(press_of("m", |press| press.note_typed("m")).taken(), None);
        assert_eq!(press(|press| press.note_typed(" ")).taken(), None);
    }

    #[test]
    fn a_command_or_a_composition_is_not_taken() {
        // An arrow or Escape (doCommandBySelector:), a dead key
        // (setMarkedText:): winit reports the key or keeps it, as before.
        assert_eq!(press(KeyPress::note_commanded).taken(), None);
        assert_eq!(press(KeyPress::note_composed).taken(), None);
    }

    #[test]
    fn a_key_the_input_method_kept_is_swallowed() {
        let press = press_of("e", |_| {});
        assert_eq!(
            press.taken(),
            Some(TakenKey::Swallowed),
            "the accent popup took the repeat"
        );
    }

    #[test]
    fn korean_space_with_nothing_composed_is_the_input_methods_commit() {
        // macOS 27, 2-Set Korean, Space with no syllable composed:
        // `setMarkedText " "`, `insertText " "` (winit commits it), then
        // `setMarkedText ""`, all inside the key press, and nothing handed
        // back. The space is the commit; the key's own " " must not follow.
        let press = press(|press| {
            press.note_composed();
            press.note_committed();
            press.note_composed();
        });
        assert_eq!(press.taken(), Some(TakenKey::Committed));
    }

    #[test]
    fn kotoeri_space_with_nothing_composed_types_its_full_width_space() {
        // macOS 27, Kotoeri (Hiragana), Space with nothing composed: one
        // `insertText "\u{3000}"` with nothing marked. TextEdit gets
        // U+3000; the key's own characters are " ".
        let press = press(|press| press.note_typed("\u{3000}"));
        assert_eq!(
            press.taken(),
            Some(TakenKey::Substituted("\u{3000}".to_string()))
        );
    }

    #[test]
    fn pinyin_punctuation_types_its_full_width_form() {
        // macOS 27, Pinyin, key code 43 (";" on the layout): one
        // `insertText "；"` with nothing marked.
        let press = press_of(";", |press| press.note_typed("；"));
        assert_eq!(press.taken(), Some(TakenKey::Substituted("；".to_string())));
    }

    #[test]
    fn several_insertions_in_one_key_are_typed_in_order() {
        let press = press_of(";", |press| {
            press.note_typed("；");
            press.note_typed("“");
        });
        assert_eq!(
            press.taken(),
            Some(TakenKey::Substituted("；“".to_string()))
        );
        // The same characters as the key's, in two calls: typed as before.
        let press = press_of("ab", |press| {
            press.note_typed("a");
            press.note_typed("b");
        });
        assert_eq!(press.taken(), None);
    }

    #[test]
    fn a_string_the_window_does_not_take_types_the_key() {
        // The platform passes an empty text for a string that starts with a
        // control character, which winit does not take either.
        let press = press_of("\r", |press| press.note_typed(""));
        assert_eq!(press.taken(), None);
    }

    #[test]
    fn a_key_with_no_characters_types_what_the_input_method_typed() {
        let mut press = KeyPress::default();
        press.note_characters(None);
        press.begin_interpreting();
        press.note_typed("x");
        press.end_interpreting();
        assert_eq!(press.taken(), Some(TakenKey::Substituted("x".to_string())));
    }

    #[test]
    fn a_command_after_a_commit_hands_the_key_back() {
        // 2-Set Korean, Escape with 가 composed (measured on macOS 27), and
        // Return: the syllable is committed, the marked text emptied, then the
        // key is passed on as `cancelOperation:` (`insertNewline:`). winit
        // reports it, and it stays a key.
        let press = press(|press| {
            press.note_composed();
            press.note_committed();
            press.note_composed();
            press.note_commanded();
        });
        assert_eq!(press.taken(), None);
    }

    #[test]
    fn a_command_before_a_commit_hands_the_key_back_too() {
        // winit forwards a key whose command was named before the commit as
        // well (its `forward_key_to_app` stays set).
        let press = press(|press| {
            press.note_composed();
            press.note_commanded();
            press.note_committed();
        });
        assert_eq!(press.taken(), None);
    }

    #[test]
    fn typing_after_a_commit_types_the_key_as_typed() {
        let own = press(|press| {
            press.note_committed();
            press.note_composed();
            press.note_typed(" ");
        });
        assert_eq!(own.taken(), None);
        let other = press(|press| {
            press.note_committed();
            press.note_composed();
            press.note_typed("\u{3000}");
        });
        assert_eq!(
            other.taken(),
            Some(TakenKey::Substituted("\u{3000}".to_string()))
        );
    }

    #[test]
    fn a_commit_after_typing_is_the_answer() {
        let press = press(|press| {
            press.note_typed("\u{3000}");
            press.note_composed();
            press.note_committed();
        });
        assert_eq!(press.taken(), Some(TakenKey::Committed));
    }

    #[test]
    fn a_commit_then_a_new_composition_is_still_the_commit() {
        // 2-Set Korean, ㄱ after the composed 가: `insertText "가"`, then
        // `setMarkedText "ㄱ"`. winit keeps the key itself here; were the new
        // composition emptied again, the key would still not be reported.
        let press = press(|press| {
            press.note_committed();
            press.note_composed();
            press.note_composed();
        });
        assert_eq!(press.taken(), Some(TakenKey::Committed));
    }

    #[test]
    fn nothing_is_decided_while_the_input_method_has_the_key() {
        let mut press = KeyPress::default();
        press.begin_interpreting();
        assert_eq!(press.taken(), None);
        press.note_committed();
        press.note_characters(Some("x"));
        press.begin_interpreting();
        press.end_interpreting();
        assert_eq!(press.taken(), None, "still inside the outer hand-over");
        press.end_interpreting();
        assert_eq!(press.taken(), Some(TakenKey::Committed));
    }

    #[test]
    fn a_nested_hand_over_keeps_the_first_characters() {
        let mut press = KeyPress::default();
        press.note_characters(Some(" "));
        press.begin_interpreting();
        press.note_characters(Some("\u{3000}"));
        press.begin_interpreting();
        press.note_typed("\u{3000}");
        press.end_interpreting();
        press.end_interpreting();
        assert_eq!(
            press.taken(),
            Some(TakenKey::Substituted("\u{3000}".to_string()))
        );
    }

    #[test]
    fn an_action_outside_the_hand_over_is_not_an_answer() {
        let mut press = KeyPress::default();
        press.note_commanded();
        press.note_typed("x");
        press.begin_interpreting();
        press.end_interpreting();
        press.note_committed();
        press.note_composed();
        press.note_typed("y");
        assert_eq!(
            press.taken(),
            Some(TakenKey::Swallowed),
            "acting before or after the input method had the key does not count"
        );
    }

    #[test]
    fn an_unbalanced_end_does_not_underflow() {
        let mut press = KeyPress::default();
        press.end_interpreting();
        assert_eq!(press.taken(), None);
    }
}
