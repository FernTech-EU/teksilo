// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Compositions an input-source switch left with their input method, on
//! macOS: the half that needs no platform.
//!
//! An input method commits its composition as the user switches away from
//! it, and does so before the switch is announced: measured on macOS 27
//! (round 10), 2-Set Korean marks its syllable again and inserts it, then
//! `NSTextInputContextKeyboardSelectionDidChangeNotification` arrives. In
//! Teksilo it usually did not (Korean committed in 2 tries of 8, Kotoeri in
//! none of 2; TextEdit in every try): it read the composition back
//! (`hasMarkedText`, `markedRange`, `attributedSubstringForProposedRange:`),
//! inserted nothing and kept it. It delivered it later, outside any key
//! press, as its source was selected or left again, into whichever field had
//! the focus then: Kotoeri as one `insertText:` of the text, Korean as a
//! `setMarkedText:` of the same text followed by its `insertText:`. Asking
//! the input context to discard the composition after the switch did not
//! stop it, as the context's discard goes to the source selected then. Once
//! `markedRange` was answered from the document after a commit in a key
//! press (round 11), Korean and Kotoeri committed at every switch, and
//! Pinyin at none.
//!
//! So the view keeps such a composition as committed itself when the switch
//! is announced (see the macOS backend), and records it here. [`LeftBehind`]
//! tells the input method's later delivery of that text, which is dropped:
//! the field already has it. A record goes when its delivery comes, and when
//! its input method acts in a key press again (it is composing anew, so it
//! holds nothing from before); [`MAX_KEPT`] are kept, one per input method.
//!
//! A delivery inside a key press (the first key after the input method is
//! selected again) was not seen, and is not looked for: the key's own
//! answer from the input method forgets the record.
//!
//! # What an input method shows is not always what it commits
//!
//! Pinyin shows `nihao` as `ni hao`: the space between its syllables is for
//! display. Measured on macOS 27 (round 11), it left `ni hao` marked at the
//! switch in Teksilo in 3 tries of 3, and delivered `nihao`, the letters
//! alone, at its next turn; in TextEdit it committed `nihao` at the switch
//! itself. So the view keeps a typed composition as the input method would
//! commit it ([`as_committed`]: `nihao`), and a delivery is told from the
//! recorded text whatever display separators either holds
//! ([`DISPLAY_SEPARATORS`]): the space macOS Pinyin shows, and the
//! apostrophe Microsoft Pinyin shows (`ni'hao`). A composition a field ends
//! itself (a click) keeps what it shows, separators included, and its
//! delivery is still told from it.
//!
//! Kotoeri shows the romaji it has not converted yet (`にほn`) and commits
//! them as kana (`にほん`, measured on macOS 27 in round 11, when it
//! committed at the switch itself; TextEdit the same in round 12). The view
//! cannot convert romaji, so it keeps what Kotoeri showed, and a delivery is
//! also told from the recorded text when it is that text with its trailing
//! romaji converted to kana, or dropped ([`delivers`]). When that late copy
//! comes, the field keeps what it was given at the switch: the copy arrives
//! at the input method's next turn, seconds or minutes later and in
//! whichever field has the focus then, and replacing text the user has
//! seen, and may have edited around since, would be a change nobody asked
//! for at that moment.

#![cfg_attr(
    not(target_os = "macos"),
    allow(
        dead_code,
        reason = "only the macOS view keeps a composition at a switch"
    )
)]

/// The most compositions recorded at once: one per input method switched
/// away from, the oldest forgotten first.
pub(crate) const MAX_KEPT: usize = 4;

/// What an input method puts between the segments of a composition for
/// display, which its commit does not hold: the space macOS Pinyin shows
/// between syllables (`ni hao` for `nihao`, measured on macOS 27) and the
/// apostrophe Microsoft Pinyin shows (`ni'hao`). A recorded composition and
/// a delivery are compared without them.
pub(crate) const DISPLAY_SEPARATORS: [char; 2] = [' ', '\''];

/// `text` without its [`DISPLAY_SEPARATORS`].
fn without_separators(text: &str) -> Vec<char> {
    text.chars()
        .filter(|c| !DISPLAY_SEPARATORS.contains(c))
        .collect()
}

/// Whether `delivered` is the input method's commit of the composition the
/// view kept as `kept`, which is not separators alone. With
/// [`DISPLAY_SEPARATORS`] left out of both:
///
/// - the same text;
/// - or `kept` is text ending in romaji the input method showed unconverted
///   (a run of ASCII letters after a character that is not one), and
///   `delivered` is that text with the romaji converted to kana, or dropped:
///   Kotoeri's `にほn` committed as `にほん`. The kana are Hiragana or
///   Katakana, at most one per letter of the romaji. The text before the
///   romaji must be there: an unrelated kana insertion (from the Japanese
///   Kana Palette) is not taken for the commit of romaji alone, so a
///   composition of romaji alone (`n`) is told only from the same text.
pub(crate) fn delivers(kept: &str, delivered: &str) -> bool {
    let kept = without_separators(kept);
    let delivered = without_separators(delivered);
    if kept.is_empty() {
        return false;
    }
    if kept == delivered {
        return true;
    }
    let romaji = kept
        .iter()
        .rev()
        .take_while(|c| c.is_ascii_alphabetic())
        .count();
    let before = &kept[..kept.len() - romaji];
    if romaji == 0 || before.is_empty() {
        return false;
    }
    let Some(converted) = delivered.strip_prefix(before) else {
        return false;
    };
    let is_kana = |c: &char| ('\u{3041}'..='\u{30FF}').contains(c);
    converted.len() <= romaji && converted.iter().all(is_kana)
}

/// The marked text `marked` as its input method commits it, for the view to
/// keep at a switch the input method left it marked across: a run of spaces
/// between two ASCII letters is the space Pinyin shows between the syllables
/// of the letters typed, and goes (`ni hao` is committed as `nihao`, as
/// Pinyin commits it in TextEdit, measured on macOS 27). Any other space
/// stays: after a converted character (`你 hao`), at either end, and in the
/// text of a dictation (`dictating`), whose spaces are between its words. An
/// apostrophe stays too: macOS Pinyin shows one only where it was typed, to
/// part two syllables (`xi'an`), and commits it.
pub(crate) fn as_committed(marked: &str, dictating: bool) -> String {
    if dictating {
        return marked.to_owned();
    }
    let chars: Vec<char> = marked.chars().collect();
    let mut committed = String::with_capacity(marked.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != ' ' {
            committed.push(chars[index]);
            index += 1;
            continue;
        }
        let run_end = chars[index..]
            .iter()
            .position(|&c| c != ' ')
            .map_or(chars.len(), |offset| index + offset);
        let between_letters = index > 0
            && chars[index - 1].is_ascii_alphabetic()
            && chars.get(run_end).is_some_and(char::is_ascii_alphabetic);
        if !between_letters {
            committed.extend(&chars[index..run_end]);
        }
        index = run_end;
    }
    committed
}

/// What an `insertText:` made outside a key press is, against the
/// compositions recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Delivery {
    /// Text of its own: it goes on as any insertion does.
    New,
    /// A recorded composition, delivered again with nothing marked (Kotoeri):
    /// dropped.
    Again,
    /// A recorded composition, marked again outside a key press and now
    /// delivered (2-Set Korean): winit's marked text, which is that mark, is
    /// ended, and the text dropped.
    MarkedAgain,
}

/// A composition the view kept as committed at a switch, which its input
/// method may still hold.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Kept {
    /// The source it was composed under.
    source: String,
    /// Its text, as the view kept it: the input method's commit of its
    /// marked text ([`as_committed`]), or the composition a field showed.
    text: String,
    /// The latest `setMarkedText:` outside a key press marked this text
    /// again, with nothing marked before it.
    marked_again: bool,
}

/// The compositions kept at a switch whose input method may deliver them
/// again. See the [module documentation](self).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LeftBehind {
    kept: Vec<Kept>,
}

impl LeftBehind {
    /// The view kept `text`, composed under `source`, as committed, while
    /// its input method was no longer selected and may still hold it. It
    /// takes the place of an earlier record for the same source: an input
    /// method holds one composition. A text of separators alone is not
    /// recorded: it would be told from nothing.
    pub(crate) fn kept(&mut self, source: &str, text: &str) {
        if without_separators(text).is_empty() {
            return;
        }
        self.kept.retain(|kept| kept.source != source);
        self.kept.push(Kept {
            source: source.to_owned(),
            text: text.to_owned(),
            marked_again: false,
        });
        if self.kept.len() > MAX_KEPT {
            self.kept.remove(0);
        }
    }

    /// The input method marked `text` outside a key press, winit holding
    /// marked text before or not (`held_before`): with nothing marked
    /// before, the first half of a recorded composition's delivery, when it
    /// is one's text, its display separators aside. Any other mark is not.
    /// Returns whether it is one.
    pub(crate) fn marked_outside_key(&mut self, text: &str, held_before: bool) -> bool {
        let mut again = false;
        for kept in &mut self.kept {
            kept.marked_again = !held_before && delivers(&kept.text, text);
            again |= kept.marked_again;
        }
        again
    }

    /// The input method inserted `text` outside a key press, winit holding
    /// marked text or not (`holds_marked_text`): whether it delivers a
    /// recorded composition again, its display separators aside (Pinyin's
    /// `nihao` for a kept `ni hao`). With marked text, only when that is the
    /// recorded composition marked again ([`marked_outside_key`]): marked
    /// text of any other origin makes the insertion a commit of its own. A
    /// delivery found takes its record.
    ///
    /// [`marked_outside_key`]: Self::marked_outside_key
    pub(crate) fn inserted_outside_key(&mut self, text: &str, holds_marked_text: bool) -> Delivery {
        let found = self.kept.iter().position(|kept| {
            delivers(&kept.text, text) && (kept.marked_again || !holds_marked_text)
        });
        self.forget_marks();
        match found {
            Some(index) => {
                self.kept.remove(index);
                if holds_marked_text {
                    Delivery::MarkedAgain
                } else {
                    Delivery::Again
                }
            }
            None => Delivery::New,
        }
    }

    /// The input method of `source` marked or inserted text in a key press:
    /// it is in use again, and holds nothing it composed before the switch.
    /// Its record goes, and no mark made before counts as a delivery's first
    /// half. `None` for a source that could not be read: only the latter.
    pub(crate) fn input_method_acted(&mut self, source: Option<&str>) {
        if let Some(source) = source {
            self.kept.retain(|kept| kept.source != source);
        }
        self.forget_marks();
    }

    /// Whether nothing is recorded, so a caller can skip reading the
    /// selected source.
    pub(crate) fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }

    fn forget_marks(&mut self) {
        for kept in &mut self.kept {
            kept.marked_again = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KOREAN: &str = "com.apple.inputmethod.Korean.2SetKorean";
    const FRENCH: &str = "com.apple.keylayout.French-PC";
    const KOTOERI: &str = "com.apple.inputmethod.Kotoeri.RomajiTyping.Japanese";
    const PINYIN: &str = "com.apple.inputmethod.SCIM.ITABC";

    #[test]
    fn pinyin_is_kept_as_its_letters_and_their_later_delivery_dropped() {
        // Round 11 (k11.log 15:53:28.393, 15:53:30.135): `ni hao` marked,
        // French-PC selected, Pinyin leaves it marked; selected again, it
        // inserts `nihao` outside any key press with nothing marked.
        let kept = as_committed("ni hao", false);
        assert_eq!(kept, "nihao", "what Pinyin commits in TextEdit");
        let mut left = LeftBehind::default();
        left.kept(PINYIN, &kept);
        assert_eq!(left.inserted_outside_key("nihao", false), Delivery::Again);
        assert!(left.is_empty());
    }

    #[test]
    fn a_delivery_is_told_whatever_display_separators_either_holds() {
        // A field that ended the composition itself kept `ni hao`.
        let mut left = LeftBehind::default();
        left.kept(PINYIN, "ni hao");
        assert_eq!(left.inserted_outside_key("nihao", false), Delivery::Again);
        // Microsoft Pinyin's apostrophes, either way round.
        left.kept(PINYIN, "ni'hao");
        assert_eq!(left.inserted_outside_key("nihao", false), Delivery::Again);
        left.kept(PINYIN, "nihao");
        assert_eq!(left.inserted_outside_key("ni'hao", false), Delivery::Again);
        // Marked again with its separators, then delivered without them.
        left.kept(PINYIN, "nihao");
        assert!(left.marked_outside_key("ni hao", false));
        assert_eq!(
            left.inserted_outside_key("nihao", true),
            Delivery::MarkedAgain
        );
    }

    #[test]
    fn separators_are_all_that_may_differ() {
        let mut left = LeftBehind::default();
        left.kept(PINYIN, "ni hao");
        assert_eq!(left.inserted_outside_key("nihaoma", false), Delivery::New);
        assert_eq!(left.inserted_outside_key("niha", false), Delivery::New);
        assert_eq!(left.inserted_outside_key("你好", false), Delivery::New);
        assert_eq!(left.inserted_outside_key(" ", false), Delivery::New);
        assert_eq!(left.inserted_outside_key("'", false), Delivery::New);
        assert!(!left.is_empty());
        // Separators alone are not recorded.
        let mut left = LeftBehind::default();
        left.kept(PINYIN, " ' ");
        assert!(left.is_empty());
    }

    #[test]
    fn kotoeri_committing_its_romaji_as_kana_is_told_from_what_it_showed() {
        // Round 12, macOS 27: にほn composing; at a real switch Kotoeri
        // commits にほん (round 11, and TextEdit). Had the view kept にほn,
        // that copy must be dropped, not typed after it.
        let mut left = LeftBehind::default();
        left.kept(KOTOERI, &as_committed("にほn", false));
        assert_eq!(left.inserted_outside_key("にほん", false), Delivery::Again);
        assert!(left.is_empty());
        // Katakana, a romaji run of two letters, or the romaji dropped.
        assert!(delivers("ニホn", "ニホン"));
        assert!(!delivers("にほky", "にほきy"), "a letter is not kana");
        assert!(delivers("にほky", "にほき"));
        assert!(delivers("にほk", "にほ"));
        // Marked again as it shows, then committed as kana.
        left.kept(KOTOERI, "にほn");
        assert!(left.marked_outside_key("にほn", false));
        assert_eq!(
            left.inserted_outside_key("にほん", true),
            Delivery::MarkedAgain
        );
    }

    #[test]
    fn a_romaji_commit_must_keep_what_came_before_it() {
        assert!(!delivers("にほn", "にん"), "the text before the romaji");
        assert!(!delivers("にほn", "にほんご"), "more kana than letters");
        assert!(!delivers("にほn", "にほ😀"));
        assert!(!delivers("にほn", "にほN"));
        assert!(!delivers("ニホン", "ニホンご"), "no romaji was pending");
        assert!(
            !delivers("n", "ん"),
            "romaji alone: a kana insertion from a palette is not taken for it"
        );
        assert!(
            !delivers("nihao", "に"),
            "Pinyin's letters are not romaji before kana"
        );
        assert!(!delivers("", ""), "nothing kept");
    }

    #[test]
    fn only_the_spaces_between_typed_letters_go_from_the_kept_text() {
        assert_eq!(as_committed("ni hao ma", false), "nihaoma");
        assert_eq!(as_committed("ni h", false), "nih");
        assert_eq!(as_committed("ni  hao", false), "nihao", "a run of them");
        assert_eq!(as_committed("xi'an", false), "xi'an", "a typed apostrophe");
        assert_eq!(as_committed("你 hao", false), "你 hao");
        assert_eq!(as_committed(" ni ", false), " ni ", "at either end");
        assert_eq!(as_committed("ni3 hao", false), "ni3 hao");
        assert_eq!(as_committed("ニホン", false), "ニホン");
        assert_eq!(as_committed("글", false), "글");
        assert_eq!(as_committed("", false), "");
        assert_eq!(
            as_committed("hello world", true),
            "hello world",
            "a dictation's spaces are between its words"
        );
    }

    #[test]
    fn kotoeri_delivering_its_composition_again_is_dropped_once() {
        // Round 10 (p6.log 14:59:17.930): ニホン left marked in the Title at
        // the switch to French-PC; at Kotoeri's next turn it inserted ニホン,
        // with nothing marked, into the editor focused then.
        let mut left = LeftBehind::default();
        left.kept(KOTOERI, "ニホン");
        assert_eq!(left.inserted_outside_key("ニホン", false), Delivery::Again);
        assert_eq!(
            left.inserted_outside_key("ニホン", false),
            Delivery::New,
            "the record went with the delivery"
        );
        assert!(left.is_empty());
    }

    #[test]
    fn korean_marking_and_delivering_its_composition_again_is_dropped() {
        // Round 10 (p6b.log 15:04:10.802, p6c.log): 글 left marked at the
        // switch; at 2-Set Korean's next turn, setMarkedText 글 then
        // insertText 글, outside any key press.
        let mut left = LeftBehind::default();
        left.kept(KOREAN, "글");
        assert!(left.marked_outside_key("글", false));
        assert_eq!(left.inserted_outside_key("글", true), Delivery::MarkedAgain);
        assert!(left.is_empty());
    }

    #[test]
    fn a_commit_of_a_composition_marked_otherwise_is_not_a_delivery() {
        let mut left = LeftBehind::default();
        left.kept(KOREAN, "글");
        // The user composes 글 again with Korean, in key presses: the record
        // goes with the first one.
        left.input_method_acted(Some(KOREAN));
        assert_eq!(left.inserted_outside_key("글", true), Delivery::New);

        // Text marked while winit held other marked text: a composition of
        // its own, whose commit stands.
        let mut left = LeftBehind::default();
        left.kept(KOREAN, "글");
        assert!(!left.marked_outside_key("글", true));
        assert_eq!(left.inserted_outside_key("글", true), Delivery::New);
        // Marked text left by something else (a dictation), committed with
        // the same text: not the record's mark either.
        left.marked_outside_key("ニホン", false);
        assert_eq!(left.inserted_outside_key("글", true), Delivery::New);
        assert!(!left.is_empty(), "the record is still waiting");
    }

    #[test]
    fn a_mark_counts_only_until_the_next_call() {
        let mut left = LeftBehind::default();
        left.kept(KOREAN, "글");
        left.marked_outside_key("글", false);
        left.marked_outside_key("", true);
        assert_eq!(left.inserted_outside_key("글", true), Delivery::New);
        left.marked_outside_key("글", false);
        left.inserted_outside_key("😀", false);
        assert_eq!(left.inserted_outside_key("글", true), Delivery::New);
        left.marked_outside_key("글", false);
        left.input_method_acted(None);
        assert_eq!(left.inserted_outside_key("글", true), Delivery::New);
        assert_eq!(
            left.inserted_outside_key("글", false),
            Delivery::Again,
            "with nothing marked it is still the delivery"
        );
    }

    #[test]
    fn typing_with_another_source_keeps_the_record() {
        // French-PC typing between the switch and Kotoeri's next turn: the
        // record waits for Kotoeri.
        let mut left = LeftBehind::default();
        left.kept(KOTOERI, "ニホン");
        left.input_method_acted(Some(FRENCH));
        assert_eq!(left.inserted_outside_key("ニホン", false), Delivery::Again);
    }

    #[test]
    fn other_text_inserted_outside_a_key_press_goes_on() {
        let mut left = LeftBehind::default();
        left.kept(KOTOERI, "ニホン");
        assert_eq!(left.inserted_outside_key("😀", false), Delivery::New);
        assert_eq!(left.inserted_outside_key("ニホ", false), Delivery::New);
        assert_eq!(left.inserted_outside_key("", false), Delivery::New);
        assert!(!left.is_empty());
    }

    #[test]
    fn one_record_per_input_method_and_a_few_at_most() {
        let mut left = LeftBehind::default();
        left.kept(KOREAN, "한");
        left.kept(KOREAN, "글");
        assert_eq!(
            left.inserted_outside_key("한", false),
            Delivery::New,
            "Korean holds one composition, the latest"
        );
        left.kept(KOTOERI, "ニホン");
        assert_eq!(left.inserted_outside_key("글", false), Delivery::Again);
        assert_eq!(left.inserted_outside_key("ニホン", false), Delivery::Again);

        let mut left = LeftBehind::default();
        for n in 0..=MAX_KEPT {
            left.kept(&format!("source{n}"), &format!("text{n}"));
        }
        assert_eq!(
            left.inserted_outside_key("text0", false),
            Delivery::New,
            "the oldest is forgotten"
        );
        assert_eq!(
            left.inserted_outside_key(&format!("text{MAX_KEPT}"), false),
            Delivery::Again
        );
        left.kept(KOREAN, "");
        assert_eq!(left.kept.len(), MAX_KEPT - 1, "nothing to record");
    }
}
