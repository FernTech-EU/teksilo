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
//! document (where a character is drawn, the text around the composition);
//! and when asked, it traces.
//!
//! With `TEKSILO_TEXT_INPUT_TRACE=1` set the hooks also log every call they
//! see to stderr, which is how the per-OS behaviour they rely on was measured.
//! `TEKSILO_TEXT_INPUT_TRACE=stacks` adds, on macOS, a short call stack to the
//! input method's calls inside a key press, and every `respondsToSelector:`
//! asked of the view inside one.

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
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
mod key_trace;
mod left_behind;
#[cfg(target_os = "macos")]
mod macos;
mod tsf_lock;
mod tsf_mirror;
#[cfg(target_os = "windows")]
mod windows;
mod winit_ime;
mod write_half;

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

/// Text the platform's input system inserted outside a key press, already
/// written into the window's focused field through its write half (macOS,
/// design decision 7 of `engineering/docs/tsf-text-store.md`): posted
/// through the window's [`AppEventPoster::post_external`] so that
/// `teksilo-app` does what it does for a commit, the field's own work
/// aside: the user's activation, the window's empty-preedit dedup reset, a
/// settle and a redraw. A payload for a window that has closed since is
/// dropped.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImeWrittenPayload {
    /// The window whose focused field took the text.
    pub window_id_owner: TeksiloWindowId,
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
/// macOS input method gave its latest `setMarkedText:` as the text the
/// composition replaces, taken once.
///
/// The app calls it while it translates winit's `Ime::Preedit` for the
/// window, and sets the result as the `replacement` of
/// [`WidgetEvent::ImeComposition`](teksilo_core::WidgetEvent::ImeComposition).
/// `None` when the input method named none, for another window, a second
/// time for the same update, and on every platform but macOS.
pub fn take_marked_replacement(window_id: TeksiloWindowId) -> Option<Range<usize>> {
    #[cfg(target_os = "macos")]
    {
        macos::take_marked_replacement(window_id)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window_id;
        None
    }
}

/// Which path the platform's text input system takes to a focused text
/// field: the application's choice (`TeksiloAppBuilder::windows_text_store`,
/// on by default), or `TEKSILO_TSF` (read once) when it names a mode.
/// Windows only; `Off` everywhere else.
///
/// The TSF text store (`engineering/docs/tsf-text-store.md`) answers text
/// services from the focused field's document and writes what they write
/// into it.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextStoreMode {
    /// IMM32 alone, through winit.
    Off,
    /// A TSF text store for every focused field that describes its
    /// document, and winit's IME off there (the default; `TEKSILO_TSF=1`).
    Store,
    /// `TEKSILO_TSF=associated`: the same store, with winit's IME (its IMM
    /// context) left on, for comparing the two (M10).
    StoreKeepingImm,
}

/// Whether the application asked for the text store
/// (`TeksiloAppBuilder::windows_text_store`), before any window exists.
static STORE_PREFERRED: AtomicBool = AtomicBool::new(true);

/// Ask for the text store, or not, from the application's builder. Read by
/// [`text_store_mode`] when `TEKSILO_TSF` says nothing; set before the first
/// window is attached.
#[doc(hidden)]
pub fn set_text_store_preferred(preferred: bool) {
    STORE_PREFERRED.store(preferred, Ordering::Relaxed);
}

/// The [`TextStoreMode`] this process runs with: `TEKSILO_TSF` when it is
/// set to a mode (`0` turns the store off whatever the application asked,
/// `1` turns it on), else what the application asked for
/// ([`set_text_store_preferred`]). Off on every platform but Windows.
#[doc(hidden)]
pub fn text_store_mode() -> TextStoreMode {
    static FROM_ENVIRONMENT: OnceLock<Option<TextStoreMode>> = OnceLock::new();
    if !cfg!(target_os = "windows") {
        return TextStoreMode::Off;
    }
    let from_environment = *FROM_ENVIRONMENT
        .get_or_init(|| parse_text_store_mode(std::env::var("TEKSILO_TSF").ok().as_deref()));
    from_environment.unwrap_or(if STORE_PREFERRED.load(Ordering::Relaxed) {
        TextStoreMode::Store
    } else {
        TextStoreMode::Off
    })
}

/// The [`TextStoreMode`] a window runs with: [`text_store_mode`], but
/// [`TextStoreMode::Off`] for a window that has no store, because TSF could
/// not be activated on its thread or the window was never attached. With the
/// store on by default, a window whose store did not start keeps IMM32
/// rather than losing its input method.
#[doc(hidden)]
pub fn text_store_mode_for(window: &ParentHandle) -> TextStoreMode {
    #[cfg(target_os = "windows")]
    {
        windows::text_store_mode_for(window)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        TextStoreMode::Off
    }
}

/// Whether the active input method is an IMM32 one that has no TSF text
/// service (a keyboard layout handle whose device half starts with `E`):
/// it reaches a field only through IMM32, so a field takes the IMM path
/// while it is active. Always `false` but on Windows.
#[doc(hidden)]
pub fn legacy_input_method_active() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::legacy_input_method_active()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// `TEKSILO_TSF`'s value as a mode: `0` (or `off`), `1` (or `store`) and
/// `associated`; `None` when unset or anything else, which leaves the
/// choice to the application.
fn parse_text_store_mode(value: Option<&str>) -> Option<TextStoreMode> {
    match value.map(str::trim) {
        Some("0" | "off") => Some(TextStoreMode::Off),
        Some("1" | "store") => Some(TextStoreMode::Store),
        Some("associated") => Some(TextStoreMode::StoreKeepingImm),
        _ => None,
    }
}

/// What has the keyboard focus in a window, as the text store sees it.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreFocus {
    /// No text field: TSF gets an empty, disabled context.
    None,
    /// A secure field (a password): a context of its own that holds no text
    /// and takes no input method (keyboard disabled), whose input scope is
    /// `IS_PASSWORD` so that the touch keyboard shows its password layout,
    /// with speech turned off on the thread while it has the focus (design
    /// section 6).
    Secure {
        /// The focused widget.
        widget: teksilo_core::WidgetId,
    },
    /// A text field that does not describe its document (the terminal): the
    /// IMM path, CUAS's own document.
    Imm,
    /// A text field that describes its document: a store of its own, a new
    /// one each time a widget takes the focus.
    Document {
        /// The focused widget.
        widget: teksilo_core::WidgetId,
    },
}

/// End the composition a text service holds in the window's text store, if
/// one is open, and let the text service write its last text (a Japanese
/// trailing `n` becomes `ん`): what `teksilo-app` does before it dispatches a
/// press, so that the press hit-tests the text the text service left
/// (`engineering/docs/tsf-text-store.md`, 4.11). Main thread only, outside
/// the dispatch of an event. Whether a composition was open: the caller then
/// lays the window out before it dispatches. Does nothing unless
/// [`text_store_mode`] is on.
#[doc(hidden)]
pub fn terminate_text_store_composition(window: &ParentHandle) -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::terminate_text_store_composition(window)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        false
    }
}

/// Whether a text service took a key (it reached the window as its key-up
/// only) since the last call: a key of a chord the window never saw go
/// down, for the bare-Alt menu, which `teksilo-app` asks as Alt goes down and
/// as it comes up. Always `false` unless [`text_store_mode`] is on.
#[doc(hidden)]
pub fn take_key_taken_by_text_store() -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::take_key_taken_by_text_store()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Asks `teksilo-app` to call [`set_text_store_focus`] for a window again,
/// between dispatches: the window's text store has a lock request waiting
/// for changes it could not report yet, or skipped a call made while a text
/// service held a lock. `teksilo-app` does what it does after a dispatched
/// event (`settle_ime`); a payload for a window that has closed since is
/// dropped.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextStorePayload {
    /// The window whose text store asks.
    pub window_id_owner: TeksiloWindowId,
}

/// Give the window's text store `focus`, and bring its copy of the focused
/// document up to date. Main thread only, from the application's handling
/// of an event (it makes TSF calls, which can deliver messages), after
/// every dispatched event. Does nothing unless [`text_store_mode`] is on.
#[doc(hidden)]
pub fn set_text_store_focus(window: &ParentHandle, focus: StoreFocus) {
    #[cfg(target_os = "windows")]
    windows::set_text_store_focus(window, focus);
    #[cfg(not(target_os = "windows"))]
    let _ = (window, focus);
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
    fn the_environment_decides_the_text_store_only_when_it_names_a_mode() {
        assert_eq!(parse_text_store_mode(None), None);
        assert_eq!(parse_text_store_mode(Some("")), None);
        assert_eq!(parse_text_store_mode(Some("0")), Some(TextStoreMode::Off));
        assert_eq!(parse_text_store_mode(Some("off")), Some(TextStoreMode::Off));
        assert_eq!(parse_text_store_mode(Some("1")), Some(TextStoreMode::Store));
        assert_eq!(
            parse_text_store_mode(Some(" store ")),
            Some(TextStoreMode::Store)
        );
        assert_eq!(
            parse_text_store_mode(Some("associated")),
            Some(TextStoreMode::StoreKeepingImm)
        );
    }

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
