// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Whether winit's `insertText:` commits, on macOS: the half that needs no
//! platform.
//!
//! winit's view commits an `insertText:` only while it holds marked text
//! *and* its IME is enabled. At the start of each `keyDown:` it compares the
//! input context's selected keyboard input source with the one it recorded,
//! and when they differ it disables its IME (it sends `Ime::Disabled`) but
//! keeps its marked text. Its next `setMarkedText:` enables it again and
//! records the source then selected.
//!
//! An input method normally ends its composition as the user switches away
//! from it. Once on macOS 27 it did not: 2-Set Korean left 한 marked, the
//! user selected Kotoeri, and Kotoeri answered Space by inserting a
//! full-width space while winit still held 한. winit had disabled its IME at
//! that `keyDown:`, so it committed nothing, the hook took the press for a
//! commit, and the space was lost, with 한 still marked. [`WinitIme`] follows
//! the part of winit's state that decides it, so the hook can commit such an
//! insertion itself ([`InKeyInsertion::HookCommits`]).
//!
//! It also keeps what winit holds as marked text and the source selected
//! when it was marked, so that the view can tell, when the selected source
//! changes, that the input method which composed it is no longer the one
//! selected ([`WinitIme::left_behind`]): measured on macOS 27 (round 10),
//! 2-Set Korean and Kotoeri usually leave their composition marked at the
//! switch, and the view then keeps it as committed itself.

#![cfg_attr(
    not(target_os = "macos"),
    allow(dead_code, reason = "only the macOS view follows winit's state")
)]

/// What becomes of an `insertText:` the input method makes inside a key
/// press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InKeyInsertion {
    /// winit commits it: it holds marked text and its IME is enabled.
    WinitCommits,
    /// winit holds marked text but disabled its IME for an input-source
    /// change, and would drop it: the hook commits it in winit's place.
    HookCommits,
    /// It types the key: winit holds no marked text (it reports the key
    /// with its characters, or, for a string other than the key's, the hook
    /// hands the string to the application), or the string starts with a
    /// control character, which winit takes from no one.
    Types,
}

/// The part of winit's IME state that decides whether its `insertText:`
/// commits, followed from the calls the hook sees. Only meaningful while
/// winit holds marked text, which only a `setMarkedText:` gives it, and
/// [`marked`](Self::marked) brings this back in step with winit at each one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WinitIme {
    /// The input source winit recorded: the one selected at the latest
    /// `setMarkedText:`, or at the `keyDown:` that found it changed. `""` for
    /// none, as winit records it.
    source: String,
    /// winit disabled its IME at a `keyDown:` for an input-source change,
    /// while it held marked text, and no `setMarkedText:` has enabled it
    /// since.
    reset: bool,
    /// The string of the latest `setMarkedText:` that reached winit: while
    /// winit holds marked text, its marked text (only a `setMarkedText:`
    /// gives winit marked text, and anything that takes it empties it).
    marked_text: String,
    /// The source selected at that `setMarkedText:`: the input method that
    /// composed winit's marked text. `""` for none.
    marked_under: String,
}

impl WinitIme {
    /// A `setMarkedText:` of `text` reaches winit with `source` selected:
    /// winit's IME is enabled, with that source. winit records the source
    /// only when its IME was disabled; when it was enabled, the one it holds
    /// is the source of the latest key press, which is the same unless the
    /// source changed between that key press and this mark, as only marked
    /// text set outside a key press could find (not measured).
    pub(crate) fn marked(&mut self, source: Option<&str>, text: &str) {
        self.source = source.unwrap_or_default().to_owned();
        self.reset = false;
        text.clone_into(&mut self.marked_text);
        self.source.clone_into(&mut self.marked_under);
    }

    /// The selected keyboard input source changed to `selected`, winit
    /// holding marked text or not (`holds_marked_text`): the text winit holds
    /// and the source it was composed under, when the input method that
    /// composed it is no longer the one selected and has left it marked.
    ///
    /// `None` with nothing marked (the input method committed at the switch,
    /// before the change is announced: measured on macOS 27), while the
    /// source that composed it is still selected, when either source is
    /// unknown, and when the source selected is another mode of the input
    /// method that composed it, as `same_input_method(composed_under,
    /// selected)` answers (see [`same_input_method`]): Kotoeri's Katakana
    /// after its Hiragana is the same input method, which keeps its
    /// composition through the change (measured in TextEdit on macOS 27,
    /// round 12) and still holds it. winit counts any change of source,
    /// modes included, at its next `keyDown:`, and is enabled again by the
    /// input method's next mark.
    pub(crate) fn left_behind(
        &self,
        selected: Option<&str>,
        holds_marked_text: bool,
        same_input_method: impl FnOnce(&str, &str) -> bool,
    ) -> Option<(&str, &str)> {
        let selected = selected.filter(|selected| !selected.is_empty())?;
        (holds_marked_text
            && !self.marked_text.is_empty()
            && !self.marked_under.is_empty()
            && selected != self.marked_under
            && !same_input_method(&self.marked_under, selected))
        .then_some((self.marked_text.as_str(), self.marked_under.as_str()))
    }

    /// A `keyDown:` is about to reach winit with `source` selected, winit
    /// holding marked text or not (`holds_marked_text`). Returns whether
    /// winit now disables its IME for a source change while it holds marked
    /// text. Without marked text winit may disable it too, but nothing it
    /// does commits until a `setMarkedText:`, which [`marked`](Self::marked)
    /// follows.
    pub(crate) fn key_down(&mut self, source: Option<&str>, holds_marked_text: bool) -> bool {
        let source = source.unwrap_or_default();
        if holds_marked_text && !self.reset && self.source != source {
            self.source = source.to_owned();
            self.reset = true;
            true
        } else {
            false
        }
    }

    /// What becomes of an in-key `insertText:` of `text`, winit holding
    /// marked text or not.
    pub(crate) fn insertion(&self, holds_marked_text: bool, text: &str) -> InKeyInsertion {
        if !holds_marked_text || text.chars().next().is_some_and(char::is_control) {
            InKeyInsertion::Types
        } else if self.reset {
            InKeyInsertion::HookCommits
        } else {
            InKeyInsertion::WinitCommits
        }
    }
}

/// Whether the keyboard input sources `a` and `b` are served by one input
/// method: two modes of it (Kotoeri's Hiragana,
/// `com.apple.inputmethod.Kotoeri.RomajiTyping.Japanese`, and its Katakana,
/// `….Japanese.Katakana`), or the same source. `a_method` and `b_method` are
/// the input method each is a mode of, by its bundle identifier, as the
/// system's Text Input Sources describe it, `None` for a source that is not
/// an input mode (a keyboard layout) or one they did not describe.
///
/// Two described modes are of one input method when their bundle
/// identifiers are the same. Where either is not described, a mode named
/// with the other's identifier and more (`….Japanese.Katakana` after
/// `….Japanese`) is taken for the same input method: the shape of the
/// identifiers macOS gives an input method's modes, and of the measured
/// change. Anything else is another input method, keyboard layouts
/// included: a dead key marked under one layout is not the next layout's.
pub(crate) fn same_input_method(
    a: &str,
    a_method: Option<&str>,
    b: &str,
    b_method: Option<&str>,
) -> bool {
    if a == b {
        return true;
    }
    match (a_method, b_method) {
        (Some(a_method), Some(b_method)) => a_method == b_method,
        _ => {
            let extends = |mode: &str, of: &str| {
                mode.strip_prefix(of)
                    .is_some_and(|rest| rest.len() > 1 && rest.starts_with('.'))
            };
            extends(a, b) || extends(b, a)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KOREAN: &str = "com.apple.inputmethod.Korean.2SetKorean";
    const FRENCH: &str = "com.apple.keylayout.French-PC";
    const KOTOERI: &str = "com.apple.inputmethod.Kotoeri.RomajiTyping.Japanese";
    const KATAKANA: &str = "com.apple.inputmethod.Kotoeri.RomajiTyping.Japanese.Katakana";
    const KOTOERI_ROMAN: &str = "com.apple.inputmethod.Kotoeri.RomajiTyping.Roman";
    const PINYIN: &str = "com.apple.inputmethod.SCIM.ITABC";
    const SHUANGPIN: &str = "com.apple.inputmethod.SCIM.Shuangpin";
    const ABC: &str = "com.apple.keylayout.ABC";
    const KANA_TYPING: &str = "com.apple.inputmethod.Kotoeri.KanaTyping.Japanese";
    const TRADITIONAL_PINYIN: &str = "com.apple.inputmethod.TCIM.Pinyin";

    /// The input method each source of these tests is a mode of, as Text
    /// Input Sources described them on macOS 27 (`input_method_of`, asked
    /// for each of these sources): Kotoeri's Romaji typing modes (Hiragana,
    /// Katakana, Roman) are one input method and its Kana typing another,
    /// Simplified Chinese's Pinyin and Shuangpin one, Traditional Chinese
    /// another, and a keyboard layout is not a mode.
    fn method_of(source: &str) -> Option<&'static str> {
        [
            "com.apple.inputmethod.Kotoeri.RomajiTyping",
            "com.apple.inputmethod.Kotoeri.KanaTyping",
            "com.apple.inputmethod.SCIM",
            "com.apple.inputmethod.TCIM",
            "com.apple.inputmethod.Korean",
        ]
        .into_iter()
        .find(|method| {
            source
                .strip_prefix(method)
                .is_some_and(|mode| mode.starts_with('.'))
        })
    }

    /// [`same_input_method`] with the sources described.
    fn described(a: &str, b: &str) -> bool {
        same_input_method(a, method_of(a), b, method_of(b))
    }

    /// [`same_input_method`] with nothing described, as when Text Input
    /// Sources do not answer.
    fn undescribed(a: &str, b: &str) -> bool {
        same_input_method(a, None, b, None)
    }

    #[test]
    fn a_mode_switch_within_one_input_method_leaves_the_composition_alone() {
        // Round 12, macOS 27: にほn composing under Kotoeri's Hiragana, then
        // Katakana chosen from the input menu, then Hiragana again. TextEdit
        // keeps the composition through both; Kotoeri commits it only at the
        // switch to French-PC.
        let mut ime = WinitIme::default();
        ime.marked(Some(KOTOERI), "にほn");
        assert_eq!(ime.left_behind(Some(KATAKANA), true, described), None);
        assert_eq!(
            ime.left_behind(Some(KATAKANA), true, undescribed),
            None,
            "with no description, from the identifiers"
        );
        assert_eq!(ime.left_behind(Some(KOTOERI), true, described), None);
        assert_eq!(
            ime.left_behind(Some(FRENCH), true, described),
            Some(("にほn", KOTOERI)),
            "the real switch"
        );
        // Marked again under Katakana, then Hiragana chosen back.
        ime.marked(Some(KATAKANA), "ニホn");
        assert_eq!(ime.left_behind(Some(KOTOERI), true, undescribed), None);
        assert_eq!(
            ime.left_behind(Some(FRENCH), true, undescribed),
            Some(("ニホn", KATAKANA))
        );
    }

    #[test]
    fn modes_of_one_input_method_are_told_by_their_description() {
        assert!(described(KOTOERI, KATAKANA));
        assert!(described(KATAKANA, KOTOERI_ROMAN), "sibling modes");
        assert!(described(PINYIN, SHUANGPIN));
        assert!(!described(KOTOERI, KANA_TYPING), "Romaji and Kana typing");
        assert!(!described(PINYIN, TRADITIONAL_PINYIN));
        assert!(!described(KOTOERI, PINYIN));
        assert!(!described(PINYIN, FRENCH));
        assert!(!described(KOREAN, KOTOERI));
        assert!(described(FRENCH, FRENCH));
        assert!(
            !described(FRENCH, ABC),
            "two keyboard layouts are two sources: a dead key is not carried"
        );
        // Described with another input method each, whatever their names.
        assert!(!same_input_method(
            KOTOERI,
            Some("com.apple.inputmethod.Kotoeri.RomajiTyping"),
            KATAKANA,
            Some("com.example.inputmethod.Other")
        ));
    }

    #[test]
    fn undescribed_sources_are_one_input_method_only_by_a_longer_name() {
        assert!(undescribed(KOTOERI, KATAKANA));
        assert!(undescribed(KATAKANA, KOTOERI));
        assert!(
            !undescribed(KATAKANA, KOTOERI_ROMAN),
            "siblings are not told apart without a description"
        );
        assert!(!undescribed(PINYIN, SHUANGPIN));
        assert!(!undescribed(KOTOERI, FRENCH));
        assert!(!undescribed(
            "com.apple.keylayout.French",
            "com.apple.keylayout.French-PC"
        ));
        assert!(
            !undescribed(KOTOERI, &format!("{KOTOERI}.")),
            "an empty mode"
        );
        // One described, the other not: the names decide.
        assert!(same_input_method(
            KOTOERI,
            method_of(KOTOERI),
            KATAKANA,
            None
        ));
        assert!(!same_input_method(
            KOTOERI,
            method_of(KOTOERI),
            FRENCH,
            None
        ));
    }

    #[test]
    fn a_composition_typed_under_one_source_is_committed_by_winit() {
        // 2-Set Korean: ㅎ ㅏ ㄴ marked, then ㄱ commits 한 and marks ㄱ.
        let mut ime = WinitIme::default();
        assert!(!ime.key_down(Some(KOREAN), false));
        ime.marked(Some(KOREAN), "ㅎ");
        for syllable in ["하", "한", "ㄱ"] {
            assert!(!ime.key_down(Some(KOREAN), true));
            ime.marked(Some(KOREAN), syllable);
        }
        assert!(!ime.key_down(Some(KOREAN), true));
        assert_eq!(ime.insertion(true, "한"), InKeyInsertion::WinitCommits);
    }

    #[test]
    fn a_composition_left_marked_across_a_switch_is_committed_by_the_hook() {
        // Round 7, macOS 27: 한 marked under 2-Set Korean; French-PC, then
        // Kotoeri selected with no key between and 한 never committed; Space
        // under Kotoeri inserts U+3000 while winit still holds 한.
        let mut ime = WinitIme::default();
        ime.marked(Some(KOREAN), "하");
        assert!(!ime.key_down(Some(KOREAN), true));
        ime.marked(Some(KOREAN), "한");
        assert!(
            ime.key_down(Some(KOTOERI), true),
            "winit disables its IME for the switch"
        );
        assert_eq!(
            ime.insertion(true, "\u{3000}"),
            InKeyInsertion::HookCommits,
            "winit would drop it"
        );
    }

    #[test]
    fn the_reset_lasts_until_the_next_mark() {
        let mut ime = WinitIme::default();
        ime.marked(Some(KOREAN), "한");
        assert!(ime.key_down(Some(FRENCH), true));
        // A key that inserted nothing: the next key still finds winit
        // disabled, and does not report the switch again.
        assert!(!ime.key_down(Some(FRENCH), true));
        assert_eq!(ime.insertion(true, "x"), InKeyInsertion::HookCommits);
        // The new input method marks: winit is enabled again, under it.
        ime.marked(Some(KOTOERI), "にほん");
        assert!(!ime.key_down(Some(KOTOERI), true));
        assert_eq!(ime.insertion(true, "日本"), InKeyInsertion::WinitCommits);
    }

    #[test]
    fn a_switch_with_nothing_marked_changes_nothing_that_commits() {
        // The usual switch: the old input method commits at the switch, so
        // winit holds nothing at the next key, whose first mark brings the
        // new source.
        let mut ime = WinitIme::default();
        ime.marked(Some(KOREAN), "한");
        assert!(!ime.key_down(Some(KOTOERI), false));
        assert_eq!(ime.insertion(false, "\u{3000}"), InKeyInsertion::Types);
        ime.marked(Some(KOTOERI), "か");
        assert!(!ime.key_down(Some(KOTOERI), true));
        assert_eq!(ime.insertion(true, "か"), InKeyInsertion::WinitCommits);
    }

    #[test]
    fn a_control_string_is_never_committed() {
        let mut ime = WinitIme::default();
        ime.marked(Some(KOREAN), "한");
        assert_eq!(ime.insertion(true, "\r"), InKeyInsertion::Types);
        assert!(ime.key_down(Some(KOTOERI), true));
        assert_eq!(ime.insertion(true, "\u{1b}"), InKeyInsertion::Types);
    }

    #[test]
    fn no_source_counts_as_winits_empty_source() {
        let mut ime = WinitIme::default();
        ime.marked(None, "한");
        assert!(!ime.key_down(Some(""), true));
        assert!(ime.key_down(Some(KOREAN), true));
    }

    #[test]
    fn a_composition_marked_under_a_source_no_longer_selected_is_left_behind() {
        // Round 10, macOS 27: 글 marked under 2-Set Korean, French-PC
        // selected, and the input method inserted nothing at the switch.
        let mut ime = WinitIme::default();
        ime.marked(Some(KOREAN), "그");
        ime.marked(Some(KOREAN), "글");
        assert_eq!(
            ime.left_behind(Some(FRENCH), true, described),
            Some(("글", KOREAN))
        );
        // Kotoeri's ニホン, after Space, the same way.
        ime.marked(Some(KOTOERI), "ニホン");
        assert_eq!(
            ime.left_behind(Some(FRENCH), true, described),
            Some(("ニホン", KOTOERI))
        );
    }

    #[test]
    fn a_composition_its_input_method_committed_is_not_left_behind() {
        // Round 10, 2 tries in 8: the input method marks its text again and
        // commits it before the switch is announced, so winit holds nothing.
        let mut ime = WinitIme::default();
        ime.marked(Some(KOREAN), "글");
        assert_eq!(ime.left_behind(Some(FRENCH), false, described), None);
    }

    #[test]
    fn a_composition_goes_on_while_its_source_is_selected() {
        let mut ime = WinitIme::default();
        ime.marked(Some(KOTOERI), "にほn");
        assert_eq!(
            ime.left_behind(Some(KOTOERI), true, described),
            None,
            "a change announced for the source composing"
        );
        assert_eq!(
            ime.left_behind(None, true, described),
            None,
            "no source to compare"
        );
        assert_eq!(
            ime.left_behind(Some(""), true, described),
            None,
            "no source either"
        );
        ime.marked(None, "にほn");
        assert_eq!(
            ime.left_behind(Some(FRENCH), true, described),
            None,
            "the source it was composed under is not known"
        );
    }

    #[test]
    fn the_input_method_that_composed_is_the_one_at_the_mark_not_at_the_reset() {
        // 한 marked under Korean, a key under French-PC disables winit's IME
        // and records French-PC as winit's source; the marked text is still
        // Korean's.
        let mut ime = WinitIme::default();
        ime.marked(Some(KOREAN), "한");
        assert!(ime.key_down(Some(FRENCH), true));
        assert_eq!(
            ime.left_behind(Some(FRENCH), true, described),
            Some(("한", KOREAN))
        );
        assert_eq!(ime.left_behind(Some(KOREAN), true, described), None);
    }

    #[test]
    fn an_emptied_mark_leaves_nothing_behind() {
        let mut ime = WinitIme::default();
        ime.marked(Some(KOREAN), "한");
        ime.marked(Some(KOREAN), "");
        assert_eq!(
            ime.left_behind(Some(FRENCH), true, described),
            None,
            "a stale answer from winit claiming marked text"
        );
    }
}
