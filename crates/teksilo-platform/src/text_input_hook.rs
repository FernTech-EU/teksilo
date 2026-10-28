// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The platform's text input system, past what winit forwards.
//!
//! winit hands an application two kinds of text: what a key press produced,
//! and the end of an IME composition the user typed. The text input system
//! sends more than that, and winit 0.30 drops the rest:
//!
//! - **macOS.** Emoji & Symbols, Dictation, AutoFill and Writing Tools all
//!   write through `NSTextInputClient` on winit's view, outside any key
//!   press. winit commits an `insertText:` only while it holds marked text,
//!   so the string is discarded. The same view answers every question about
//!   the document (`selectedRange`, `attributedSubstringForProposedRange:`)
//!   with "nothing", so nothing that reads or corrects text can work.
//! - **Windows.** winit reads the input system through IMM32 alone. The
//!   emoji panel and voice typing reach it as IMM results, so they arrive as
//!   compositions: nothing tells voice typing apart without a TSF text store,
//!   and winit cannot end a composition the field has already kept.
//!
//! This module attaches to each window's native surface once it exists and
//! detaches when it closes. Text the input system inserts outside a key press
//! is posted to the application as an [`ImeCommitPayload`], which `teksilo-app`
//! delivers to the window's focused widget as a
//! [`WidgetEvent::ImeCommit`](teksilo_core::WidgetEvent::ImeCommit). Which
//! part of the input system the text came from is decided by one rule shared
//! by the platforms, `classify`.
//!
//! The native surface also answers the input system's questions about the
//! document (where the selection is, what the text of a range is, where a
//! range is drawn, which character is under a point) from the document of the
//! window's focused text widget, which its tree's
//! [`TextSurfaces`] reach; `answers` holds the part of that which needs no
//! platform.
//!
//! A key press the input method takes for itself, without typing or
//! composing anything, must not type its text either; one it answers with a
//! commit of its own is the input method's, as the key that ends a
//! composition is; and one it types as another character (a full-width
//! space) types that character: [`key_taken_by_input_method`] tells the
//! application, while it handles the key, which it was.
//!
//! Where it stands, measured on macOS 27: Emoji & Symbols, the press-and-hold
//! accent popup, Dictation and Writing Tools (in its panel) insert into the
//! focused field. Dictation is told apart from a composition through private
//! AppKit methods (see the macOS backend). A composition its input method
//! leaves marked when the user switches to another input source is kept as
//! committed where it is, and the input method's later delivery of it is
//! dropped. On Windows the hook has the input method complete a composition
//! the field ends, and tells the field what it committed
//! ([`finish_composition`]); answers the input method's requests about the
//! document (where a character is drawn, the text to reconvert, the text
//! around the composition), so a reconversion composes over the text it
//! reconverts ([`take_marked_replacement`]); gives the window the input
//! scope of the focused field ([`set_input_purpose`]); and when asked, it
//! traces.
//!
//! With `TEKSILO_TEXT_INPUT_TRACE=1` set the hooks also log every call they
//! see to stderr, which is how the per-OS behaviour they rely on was measured.
//! `TEKSILO_TEXT_INPUT_TRACE=stacks` adds, on macOS, a short call stack to the
//! input method's calls inside a key press, and every `respondsToSelector:`
//! asked of the view inside one.

use std::ops::Range;
use std::sync::{Arc, OnceLock};

use teksilo_core::raw_handle::ParentHandle;
use teksilo_core::text_surface::TextSurfaces;
use teksilo_core::window::TeksiloWindowId;
use teksilo_core::{AppEventPoster, ImeOrigin};

mod answers;
mod composition;
mod dictation;
mod imm_completion;
mod imm_requests;
mod key_press;
mod left_behind;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
mod winit_ime;

/// Whether `TEKSILO_TEXT_INPUT_TRACE` is set to anything but `0`. Read once.
pub fn trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("TEKSILO_TEXT_INPUT_TRACE").is_some_and(|value| value != "0")
    })
}

/// Whether `TEKSILO_TEXT_INPUT_TRACE` is `stacks`, which turns tracing on
/// and adds the detail for telling who answers a key: a short call stack on
/// the input method's calls inside a key press, and every
/// `respondsToSelector:` asked of the view inside one. Read once.
#[allow(dead_code)] // Only the macOS backend writes stacks.
pub(crate) fn trace_stacks_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("TEKSILO_TEXT_INPUT_TRACE").is_some_and(|value| value == "stacks")
    })
}

/// Write one trace line to stderr, if tracing is on.
#[allow(dead_code)] // Only the macOS and Windows backends trace.
pub(crate) fn trace(args: std::fmt::Arguments<'_>) {
    if trace_enabled() {
        eprintln!("[text-input] {args}");
    }
}

/// Text the platform's input system committed into a window outside what
/// winit reports, posted through the window's
/// [`AppEventPoster::post_external`]. `teksilo-app` delivers it to the
/// window's focused widget as a
/// [`WidgetEvent::ImeCommit`](teksilo_core::WidgetEvent::ImeCommit) with the
/// same fields; a payload for a window that has closed since is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImeCommitPayload {
    /// The window whose native surface received the text.
    pub window_id_owner: TeksiloWindowId,
    /// The committed text.
    pub text: String,
    /// Which part of the input system committed it.
    pub origin: ImeOrigin,
    /// The range of the focused field's text it replaces, in that field's
    /// character offsets, when the input system named one. `None` replaces
    /// the selection, or inserts at the caret.
    pub replacement: Option<Range<usize>>,
}

/// What the platform tells about text that arrived outside a key press, for
/// [`classify`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(
    not(target_os = "macos"),
    allow(dead_code, reason = "only the macOS hook classifies, for now")
)]
pub(crate) struct CommitSignals {
    /// The window held a composition (marked text), which the text ends.
    pub(crate) had_marked_text: bool,
    /// The text carries alternatives for some of its words (on macOS, the
    /// `NSTextAlternatives` attribute anywhere in the attributed string).
    pub(crate) carries_text_alternatives: bool,
    /// The platform's dictation is running on the window, or has ended with
    /// its last words still being composed: see
    /// [`DictationSession`](dictation::DictationSession) for what opens and
    /// closes it.
    pub(crate) dictation_session: bool,
    /// A writing assistant was opened on the window and the user has not
    /// pressed a key in it since (macOS Writing Tools).
    pub(crate) writing_tools_session: bool,
}

/// Which part of the input system committed text that arrived outside a key
/// press, from what the platform tells about it, in this order:
///
/// - text carrying alternatives, or arriving during a dictation session:
///   [`ImeOrigin::Dictation`];
/// - during a Writing Tools session: [`ImeOrigin::WritingTools`];
/// - ending a composition: [`ImeOrigin::Composition`];
/// - otherwise the input system inserted it on its own (Emoji & Symbols, an
///   accent picked from the press-and-hold popup, AutoFill):
///   [`ImeOrigin::Insertion`].
///
/// What was measured on macOS 27: Emoji & Symbols and the accent popup
/// insert an `NSString` with no marked text outside a key press; Writing
/// Tools replaces the selection with one `NSString` outside a key press,
/// inside the session `showWritingTools:` opens; dictation streams its words
/// through marked text (`setMarkedText:`) and commits them as a plain
/// `NSString`, with **no** `NSTextAlternatives`, so only its session tells
/// it from a composition. When the user stops dictation, its end is
/// reported before the last sentence is committed, so the session stays
/// open for that commit. The session is read from private AppKit methods
/// (see the macOS backend and
/// [`DictationSession`](dictation::DictationSession)); where they are
/// missing it opens only when the Edit menu starts dictation, and dictation
/// from the dictation key reads as [`ImeOrigin::Composition`]. Alternatives
/// still count, for a platform or a release that does attach them.
#[cfg_attr(
    not(target_os = "macos"),
    allow(dead_code, reason = "only the macOS hook classifies, for now")
)]
pub(crate) fn classify(signals: CommitSignals) -> ImeOrigin {
    if signals.carries_text_alternatives || signals.dictation_session {
        ImeOrigin::Dictation
    } else if signals.writing_tools_session {
        ImeOrigin::WritingTools
    } else if signals.had_marked_text {
        ImeOrigin::Composition
    } else {
        ImeOrigin::Insertion
    }
}

/// What the platform's input method made of a key press, when it did not
/// hand the key back as it was, as [`key_taken_by_input_method`] answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TakenKey {
    /// It had the key and did nothing with it: the press-and-hold accent
    /// popup taking a repeat of the held letter, or the key that picks an
    /// accent. The key still counts, for shortcuts and navigation; its text
    /// is not typed.
    Swallowed,
    /// It answered the key with a commit of its own and handed nothing back:
    /// 2-Set Korean answering Space with nothing composed by marking a space
    /// and committing it. The press is the input method's, as the key that
    /// ends a composition is, which winit does not report: it reaches no
    /// widget, and its text is not typed a second time.
    Committed,
    /// It typed the key, with nothing composed, as this text rather than the
    /// key's own characters: Kotoeri's full-width space (U+3000) for Space,
    /// Pinyin's "；" for ";". The key goes on with this text in place of the
    /// characters winit reports for it.
    Substituted(String),
}

/// What the platform's input method took of the key press `window_id` is
/// handling right now, if anything: see [`TakenKey`].
///
/// Ask it while handling winit's `KeyboardInput` for a pressed key, which
/// macOS delivers from inside the view's `keyDown:`, after the input method
/// has had the key. Outside a key press, for another window, for a key the
/// input method never saw or handed back as it was (typed its own
/// characters, or passed it on as a command), and on every platform but
/// macOS, `None`: the key and its text go on as winit reports them. See the
/// macOS backend for what is tracked.
pub fn key_taken_by_input_method(window_id: TeksiloWindowId) -> Option<TakenKey> {
    #[cfg(target_os = "macos")]
    {
        macos::key_taken_by_input_method(window_id)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window_id;
        None
    }
}

/// How the platform ended a window's composition, as [`finish_composition`]
/// reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FinishedComposition {
    /// The text the input method committed for the composition as it ended,
    /// for the field to keep in place of the text the composition showed.
    /// `None` keeps what it showed: on macOS, and on Windows when the input
    /// method committed nothing, or only an empty string.
    pub committed: Option<String>,
    /// What winit delivers as `Ime::Commit` for the window once the
    /// application's handler returns, in order, an empty string included: the
    /// copy of the commit above, and anything else winit reads again. The
    /// application drops them: the field has kept the composition, and the
    /// caret has moved since. Empty on macOS.
    pub late_commits: Vec<String>,
}

/// End the window's composition (a click elsewhere, a focus change), and say
/// what the input method committed for it.
///
/// Called from inside the application's handler while the focused field
/// still holds the composition; the field keeps the text right after.
/// Without this the input method still holds the composition and delivers
/// its text later, at the old place or in another field. `None` means the
/// platform did not do it, and the caller falls back to resetting the input
/// method by toggling winit's IME on the window (`set_ime_allowed(false)`
/// then `set_ime_allowed(true)`); the field keeps what the composition
/// showed.
///
/// macOS does it for an attached window: the view ends winit's marked text
/// and has the input method discard its composition, and the field keeps
/// what it showed, as TextEdit does. Windows asks the input method to
/// complete the composition in the window's input context (`ImmNotifyIME`
/// with `CPS_COMPLETE`), as Notepad's text control does, and the field keeps
/// what the input method commits (Microsoft Pinyin's "nihao" for the
/// "ni'hao" it showed); winit's own copy of that commit is in
/// [`FinishedComposition::late_commits`]. Elsewhere, and for a window with
/// no input context, `None`.
pub fn finish_composition(window_id: TeksiloWindowId) -> Option<FinishedComposition> {
    #[cfg(target_os = "macos")]
    {
        macos::finish_composition(window_id).then(FinishedComposition::default)
    }
    #[cfg(target_os = "windows")]
    {
        windows::finish_composition(window_id)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = window_id;
        None
    }
}

/// The range of the focused field's text (its character offsets) that the
/// window's next composition replaces, taken once: on macOS, the range the
/// input method gave its latest `setMarkedText:` (Korean Hanja conversion of
/// the syllable just typed); on Windows, the range of an IMM32 reconversion,
/// which the input method confirmed with `IMR_CONFIRMRECONVERTSTRING` or
/// which was selected when it asked for the text (`IMR_RECONVERTSTRING`).
///
/// The app calls it while it translates winit's `Ime::Preedit` for the
/// window, and sets the result as the `replacement` of
/// [`WidgetEvent::ImeComposition`](teksilo_core::WidgetEvent::ImeComposition).
/// `None` when the input method named none, for another window, a second
/// time for the same update, and on every other platform.
pub fn take_marked_replacement(window_id: TeksiloWindowId) -> Option<Range<usize>> {
    #[cfg(target_os = "macos")]
    {
        macos::take_marked_replacement(window_id)
    }
    #[cfg(target_os = "windows")]
    {
        windows::take_marked_replacement(window_id)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = window_id;
        None
    }
}

/// Tell the platform's input system what the window's focused text field is
/// for, beyond what winit forwards. Main thread only, from the application's
/// handling of an event, never from inside a window procedure ahead of
/// winit: the call may deliver messages.
///
/// On Windows, the window's input scope (`SetInputScope`): `IS_PASSWORD`
/// for [`ImePurpose::Password`](teksilo_core::ImePurpose::Password), `IS_DEFAULT` otherwise; winit 0.30's
/// `set_ime_purpose` does nothing there. A window given a scope is set back
/// to `IS_DEFAULT` when it is detached. Elsewhere winit's own
/// `set_ime_purpose` says it, and this does nothing.
pub fn set_input_purpose(window: &ParentHandle, purpose: teksilo_core::ImePurpose) {
    #[cfg(target_os = "windows")]
    windows::set_input_purpose(window, purpose);
    #[cfg(not(target_os = "windows"))]
    let _ = (window, purpose);
}

/// Attach to a window's native surface. Main thread only, once the window
/// exists. A no-op where the platform needs nothing.
///
/// `poster` takes the text the input system inserts outside what winit
/// reports. `surfaces` are the window tree's
/// ([`WidgetTree::text_surfaces`](teksilo_core::WidgetTree::text_surfaces)):
/// the native surface answers the input system's questions about the
/// document (the selection, the text, where a range is drawn) from the one
/// that has the keyboard focus.
pub fn attach(
    window_id: TeksiloWindowId,
    parent: &ParentHandle,
    poster: Arc<dyn AppEventPoster>,
    surfaces: TextSurfaces,
) {
    #[cfg(target_os = "macos")]
    macos::attach(window_id, parent, poster, surfaces);
    #[cfg(target_os = "windows")]
    windows::attach(window_id, parent, poster, surfaces);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (window_id, parent, poster, surfaces);
}

/// Forget a window. Safe to call for a window that was never attached.
pub fn detach(window_id: TeksiloWindowId) {
    #[cfg(target_os = "macos")]
    macos::detach(window_id);
    #[cfg(target_os = "windows")]
    windows::detach(window_id);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = window_id;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_follows_the_precedence_dictation_writing_tools_marked_text() {
        use ImeOrigin::{Composition, Dictation, Insertion, WritingTools};
        // Every combination of the four signals.
        for bits in 0..16u8 {
            let signals = CommitSignals {
                had_marked_text: bits & 1 != 0,
                carries_text_alternatives: bits & 2 != 0,
                dictation_session: bits & 4 != 0,
                writing_tools_session: bits & 8 != 0,
            };
            let expected = if signals.carries_text_alternatives || signals.dictation_session {
                Dictation
            } else if signals.writing_tools_session {
                WritingTools
            } else if signals.had_marked_text {
                Composition
            } else {
                Insertion
            };
            assert_eq!(classify(signals), expected, "{signals:?}");
        }
        // The cases the macOS backend meets, spelled out.
        let measured = [
            // Emoji & Symbols, an accent picked from the popup (measured).
            (CommitSignals::default(), Insertion),
            // A Japanese candidate clicked with the mouse (not measured yet).
            (
                CommitSignals {
                    had_marked_text: true,
                    ..CommitSignals::default()
                },
                Composition,
            ),
            // Dictation: marked text, no alternatives, inside its session
            // (measured).
            (
                CommitSignals {
                    had_marked_text: true,
                    dictation_session: true,
                    ..CommitSignals::default()
                },
                Dictation,
            ),
            // Writing Tools' Replace (measured).
            (
                CommitSignals {
                    writing_tools_session: true,
                    ..CommitSignals::default()
                },
                WritingTools,
            ),
            // Dictating while the Writing Tools session is still open.
            (
                CommitSignals {
                    had_marked_text: true,
                    dictation_session: true,
                    writing_tools_session: true,
                    ..CommitSignals::default()
                },
                Dictation,
            ),
        ];
        for (signals, expected) in measured {
            assert_eq!(classify(signals), expected, "{signals:?}");
        }
    }
}
