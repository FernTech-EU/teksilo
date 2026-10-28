// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Completing a composition on Windows: the half that needs no platform.
//!
//! When the field ends a composition (a click elsewhere, a focus change), the
//! Windows hook asks the input method to complete it (`ImmNotifyIME` with
//! `CPS_COMPLETE`), as a native edit control does. The input method commits
//! its own form of the composition, which can differ from what it showed:
//! measured in Notepad (round 12), Microsoft Pinyin shows `nihao` as
//! "ni'hao" and commits "nihao", while an apostrophe the writer typed, as in
//! "xi'an", stays. The field keeps that commit.
//!
//! The input method answers from inside the call, with `WM_IME_COMPOSITION`
//! and `WM_IME_ENDCOMPOSITION` sent to the window. winit handles them as they
//! come, but holds the events they make until the application has handled
//! the press or the focus change it is in, so its `Ime::Commit` of the text
//! the field has just kept arrives after it, with the caret moved: the
//! application drops it. A subclass shows each message to an
//! [`ImmCompletion`] just before winit has it, with the input context's
//! strings read as winit reads them, and the completion says both what the
//! input method committed and what winit commits for those messages.
//!
//! What winit 0.30 makes of them (`WM_IME_COMPOSITION` and
//! `WM_IME_ENDCOMPOSITION` in its Windows `event_loop.rs`), while the
//! window's IME is allowed, from its IME state, which is `Preedit` while the
//! field holds a composition (only a preedit with text gives it one, and
//! winit sends that only as it enters `Preedit`):
//!
//! - `WM_IME_COMPOSITION` is ignored in the `Disabled` state. Otherwise, with
//!   `GCS_RESULTSTR` in its `lParam` and a result string it can read, winit
//!   commits that string, an empty one too, and the state becomes `Enabled`;
//!   then, with `GCS_COMPSTR` and a composition string it can read, it shows
//!   that string and the state becomes `Preedit`.
//! - `WM_IME_ENDCOMPOSITION` in the `Preedit` state commits the result string
//!   winit can read then (a workaround for Hangul input methods, which sent
//!   their last result after this message); the state becomes `Disabled`.
//!
//! A string winit cannot read (an error from the input context, UTF-16 that
//! is not valid) is one it does nothing with.

#![cfg_attr(
    not(target_os = "windows"),
    allow(dead_code, reason = "only the Windows hook completes, for now")
)]

/// `GCS_COMPSTR`: the message carries the composition string.
pub(crate) const GCS_COMPSTR: u32 = 0x0008;
/// `GCS_RESULTSTR`: the message carries a result string.
pub(crate) const GCS_RESULTSTR: u32 = 0x0800;

/// winit's IME state for the window (its Windows `ImeState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WinitState {
    Disabled,
    Enabled,
    Preedit,
}

/// A message the input method sent the window while its composition was
/// being completed, with the input context's strings as winit reads them for
/// it: `None` for one it cannot read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImmMessage {
    /// `WM_IME_COMPOSITION` with its `lParam` flags, the result string (read
    /// with `GCS_RESULTSTR` among them, else `None`) and the composition
    /// string (with `GCS_COMPSTR`, else `None`).
    Composition {
        flags: u32,
        result: Option<String>,
        composition: Option<String>,
    },
    /// `WM_IME_ENDCOMPOSITION`, with the result string read then.
    End { result: Option<String> },
}

/// What the input method commits as the hook completes a composition, and
/// what winit makes of it. See the [module documentation](self).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImmCompletion {
    state: WinitState,
    /// The result strings the input method's composition messages carried,
    /// in order.
    results: String,
    /// The composition string of the latest composition message, while it
    /// carried one.
    composing: Option<String>,
    /// What winit commits for the messages, in order.
    winit_commits: Vec<String>,
}

impl Default for ImmCompletion {
    fn default() -> Self {
        Self::new()
    }
}

impl ImmCompletion {
    /// For a window whose field holds a composition, which winit shows from
    /// its `Preedit` state.
    pub(crate) fn new() -> Self {
        Self {
            state: WinitState::Preedit,
            results: String::new(),
            composing: None,
            winit_commits: Vec::new(),
        }
    }

    /// The input method sent `message`, which winit is about to handle.
    pub(crate) fn message(&mut self, message: &ImmMessage) {
        match message {
            ImmMessage::Composition {
                flags,
                result,
                composition,
            } => {
                // A result is the input method's commit whatever winit does
                // with it: after its `WM_IME_ENDCOMPOSITION` it ignores the
                // message, which is how a Hangul input method's last result
                // would be lost to it.
                if flags & GCS_RESULTSTR != 0
                    && let Some(result) = result
                {
                    self.results.push_str(result);
                }
                self.composing = composition.clone().filter(|_| flags & GCS_COMPSTR != 0);
                if self.state == WinitState::Disabled {
                    return;
                }
                if flags & GCS_RESULTSTR != 0
                    && let Some(result) = result
                {
                    self.winit_commits.push(result.clone());
                    self.state = WinitState::Enabled;
                }
                if flags & GCS_COMPSTR != 0 && composition.is_some() {
                    self.state = WinitState::Preedit;
                }
            }
            ImmMessage::End { result } => {
                if self.state == WinitState::Preedit
                    && let Some(result) = result
                {
                    self.winit_commits.push(result.clone());
                }
                self.state = WinitState::Disabled;
                self.composing = None;
            }
        }
    }

    /// What the input method still composes, as winit sees it, when the
    /// completion left it composing: it sent nothing for it, or composed
    /// again in its latest message. The hook then cancels it, so that it
    /// composes no more at the place the field has left. The text is the
    /// latest composition string it sent, empty when it sent none.
    pub(crate) fn left_composing(&self) -> Option<&str> {
        (self.state == WinitState::Preedit).then(|| self.composing.as_deref().unwrap_or(""))
    }

    /// The text the input method committed for the composition, for the field
    /// to keep: its results in order, then `left`, what it still composed
    /// after the completion ([`left_composing`](Self::left_composing)), which
    /// the hook cancelled and the writer still saw. `None` when it committed
    /// nothing, or only empty results: the field keeps what the composition
    /// showed rather than lose it.
    pub(crate) fn committed(&self, left: Option<&str>) -> Option<String> {
        if self.results.is_empty() {
            return None;
        }
        let mut committed = self.results.clone();
        committed.push_str(left.unwrap_or(""));
        Some(committed)
    }

    /// What winit commits for the messages, in order, an empty string
    /// included: what it will deliver as `Ime::Commit` once the application's
    /// handler returns.
    pub(crate) fn winit_commits(&self) -> &[String] {
        &self.winit_commits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `GCS_RESULTREADCLAUSE | GCS_RESULTSTR | GCS_RESULTCLAUSE`, Microsoft
    /// Pinyin's commit (measured, round 4).
    const PINYIN_COMMIT: u32 = 0x1c00;
    /// `GCS_COMPSTR | GCS_COMPATTR | GCS_COMPCLAUSE | GCS_CURSORPOS |
    /// GCS_DELTASTART`, Pinyin's composition.
    const PINYIN_COMPOSITION: u32 = 0x1b8;

    fn result(flags: u32, text: &str) -> ImmMessage {
        ImmMessage::Composition {
            flags,
            result: Some(text.to_owned()),
            composition: None,
        }
    }

    fn end(text: &str) -> ImmMessage {
        ImmMessage::End {
            result: Some(text.to_owned()),
        }
    }

    fn cleared() -> ImmMessage {
        ImmMessage::Composition {
            flags: 0,
            result: None,
            composition: None,
        }
    }

    fn completed(messages: &[ImmMessage]) -> ImmCompletion {
        let mut completion = ImmCompletion::new();
        for message in messages {
            completion.message(message);
        }
        completion
    }

    #[test]
    fn a_result_then_the_end_is_committed_once_by_winit() {
        let completion = completed(&[result(PINYIN_COMMIT, "nihao"), end("nihao")]);
        assert_eq!(completion.committed(None).as_deref(), Some("nihao"));
        assert_eq!(
            completion.winit_commits(),
            ["nihao"],
            "the end finds winit out of its preedit state and commits nothing more"
        );
        assert_eq!(completion.left_composing(), None);
    }

    #[test]
    fn a_cleared_composition_before_the_result_changes_nothing() {
        let completion = completed(&[cleared(), result(0x1e00, "にほん"), end("")]);
        assert_eq!(completion.committed(None).as_deref(), Some("にほん"));
        assert_eq!(completion.winit_commits(), ["にほん"]);
    }

    /// The order a Hangul input method used to send: the end first, its
    /// result after. winit commits what the input context holds at the end,
    /// and ignores the result that follows; the field keeps the result.
    #[test]
    fn a_result_after_the_end_is_the_input_methods_but_winit_commits_at_the_end() {
        let completion = completed(&[end("\u{AE00}"), result(GCS_RESULTSTR, "\u{AE00}")]);
        assert_eq!(completion.committed(None).as_deref(), Some("\u{AE00}"));
        assert_eq!(completion.winit_commits(), ["\u{AE00}"]);

        // The result set only after the end: winit commits the empty string it
        // read then, and the field still keeps the syllable.
        let completion = completed(&[end(""), result(GCS_RESULTSTR, "\u{AE00}")]);
        assert_eq!(completion.committed(None).as_deref(), Some("\u{AE00}"));
        assert_eq!(completion.winit_commits(), [""]);
    }

    /// An input method that answers the completion with an end and no result:
    /// winit commits what the input context holds then, and the field keeps
    /// what the composition showed.
    #[test]
    fn an_end_with_no_result_commits_nothing_for_the_field() {
        let completion = completed(&[cleared(), end("")]);
        assert_eq!(completion.committed(None), None);
        assert_eq!(completion.winit_commits(), [""]);
        assert_eq!(completion.left_composing(), None);
    }

    #[test]
    fn an_empty_result_is_no_commit_for_the_field() {
        let completion = completed(&[result(PINYIN_COMMIT, ""), end("")]);
        assert_eq!(completion.committed(None), None);
        assert_eq!(completion.winit_commits(), [""]);
    }

    #[test]
    fn several_results_are_kept_in_order_and_dropped_one_by_one() {
        let completion = completed(&[
            result(GCS_RESULTSTR, "\u{65E5}"),
            result(GCS_RESULTSTR, "\u{672C}"),
            end("\u{672C}"),
        ]);
        assert_eq!(
            completion.committed(None).as_deref(),
            Some("\u{65E5}\u{672C}")
        );
        assert_eq!(completion.winit_commits(), ["\u{65E5}", "\u{672C}"]);
    }

    #[test]
    fn nothing_sent_leaves_the_input_method_composing() {
        let completion = completed(&[]);
        assert_eq!(completion.left_composing(), Some(""));
        assert_eq!(completion.committed(None), None);
        assert!(completion.winit_commits().is_empty());
    }

    /// A result that starts the next composition in the same message: the
    /// hook cancels what is left, and the field keeps the result and what was
    /// left, which the writer still saw. The cancel's end then finds winit
    /// composing and commits what the input context holds once more.
    #[test]
    fn a_composition_left_after_the_result_is_kept_after_it() {
        let mut completion = completed(&[ImmMessage::Composition {
            flags: PINYIN_COMMIT | PINYIN_COMPOSITION,
            result: Some("\u{4F60}".to_owned()),
            composition: Some("hao".to_owned()),
        }]);
        let left = completion.left_composing().map(str::to_owned);
        assert_eq!(left.as_deref(), Some("hao"));
        completion.message(&cleared());
        completion.message(&end("\u{4F60}"));
        assert_eq!(completion.left_composing(), None);
        assert_eq!(
            completion.committed(left.as_deref()).as_deref(),
            Some("\u{4F60}hao")
        );
        assert_eq!(completion.winit_commits(), ["\u{4F60}", "\u{4F60}"]);
    }

    #[test]
    fn a_string_winit_cannot_read_is_not_committed() {
        let completion = completed(&[
            ImmMessage::Composition {
                flags: GCS_RESULTSTR,
                result: None,
                composition: None,
            },
            ImmMessage::End { result: None },
        ]);
        assert_eq!(completion.committed(None), None);
        assert!(completion.winit_commits().is_empty());
    }

    #[test]
    fn a_message_after_the_end_is_ignored_by_winit() {
        let completion = completed(&[
            result(GCS_RESULTSTR, "a"),
            end("a"),
            ImmMessage::Composition {
                flags: GCS_COMPSTR,
                result: None,
                composition: Some("b".to_owned()),
            },
        ]);
        assert_eq!(completion.winit_commits(), ["a"]);
        assert_eq!(completion.left_composing(), None, "winit is disabled");
    }
}
