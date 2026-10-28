// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! macOS: hooks on winit's `WinitView`.
//!
//! winit's view is the window's `NSTextInputClient`. Everything the text
//! input system does (an input method, the press-and-hold accent popup,
//! Emoji & Symbols, Dictation, AutoFill, Writing Tools) reads the document
//! and writes into it through that protocol, and winit 0.30 answers only
//! what an IME composition typed during a key press needs.
//!
//! # The view answers from the focused document
//!
//! There is one `WinitView` per window and many text widgets in it, so the
//! view answers the input system's questions from the document of the text
//! widget that has the keyboard focus, through the window tree's
//! [`TextSurfaces`] (see [`teksilo_core::text_input_document`]); the
//! counting is [`answers`], and only the AppKit calls are
//! here. With no such widget, or one that cannot answer at that moment, each
//! question gets winit's own answer, as before.
//!
//! - `selectedRange`: the selection, in UTF-16 units.
//! - `markedRange`: the document's composition while winit's own
//!   `hasMarkedText` says it holds marked text, `NSNotFound` otherwise.
//! - `attributedSubstringForProposedRange:actualRange:`: the text of the
//!   range, plain, widened to whole characters, with the range it is in
//!   `actualRange`; nil for a secure field.
//! - `characterIndexForPoint:`: the character under the screen point,
//!   `NSNotFound` where there is none.
//! - `firstRectForCharacterRange:actualRange:`: the first line of the range
//!   (of the selection, for `NSNotFound`) on screen, and the range it holds.
//!
//! Window-logical coordinates are `WinitView`'s own space (it is flipped,
//! and Teksilo hands winit its IME cursor area in it unchanged), so a
//! rectangle reaches the screen as winit's own `firstRect` does,
//! `convertRect:toView:nil` then the window's `convertRectToScreen:`, and a
//! point comes back with `convertPointFromScreen:` then
//! `convertPoint:fromView:nil`.
//!
//! `hasMarkedText`, `setMarkedText:selectedRange:replacementRange:` and
//! `unmarkText` stay winit's own: its `keyDown:` decides what a key press
//! did from its own marked-text state.
//!
//! The view also gains what Writing Tools asks before it acts, which neither
//! `WinitView` nor `NSView` has: `isEditable`, `writingToolsBehavior`
//! (`Limited`, the panel, where the focused widget allows Writing Tools,
//! `None` otherwise), `allowsWritingToolsAffordance` (no),
//! `allowedWritingToolsResultOptions` (plain text), and the screen
//! rectangles `documentVisibleRect` and `unionRectInVisibleSelectedRange`.
//!
//! With these answers Dictation, the press-and-hold accent popup and Writing
//! Tools in its panel work (measured on macOS 27, where each had given up on
//! the `NSNotFound` `selectedRange` answered).
//!
//! # Text inserted outside a key press
//!
//! winit's `insertText:replacementRange:` commits only while it holds marked
//! text, so an insertion made without a composition was dropped: Emoji &
//! Symbols, measured on macOS 27, inserts an `NSString` with no marked text
//! and no key press running. The replacement leaves winit's own method to
//! run whenever a `keyDown:` is running (typing, and a composition a key
//! ends, which winit turns into its `KeyboardInput` text and its
//! `Ime::Commit`) and on a view nobody attached. Otherwise it takes the text
//! itself:
//!
//! - a string that starts with a control character is dropped, as winit
//!   drops it;
//! - a composition the view held is ended with winit's own `unmarkText`,
//!   which clears winit's marked text, reports an empty preedit and returns
//!   winit to its ground state, so the next key press is not mistaken for
//!   the end of a commit;
//! - a replacement range the input system names is converted to the focused
//!   document's characters;
//! - the text is posted as an [`ImeCommitPayload`], whose origin
//!   [`classify`](super::classify) decides from the composition, the
//!   `NSTextAlternatives` attribute and the dictation and Writing Tools
//!   sessions. An empty text is posted only when it replaces a range (a
//!   deletion).
//!
//! The Writing Tools session is set on the key window's view when
//! `NSApplication`'s `showWritingTools:` runs, and cleared by that view's next
//! `keyDown:` (not by the window losing key: the Writing Tools panel takes
//! it).
//!
//! # Dictation, through private AppKit methods
//!
//! Dictation on macOS 27 streams its words through marked text and commits
//! them as a plain `NSString` with no `NSTextAlternatives`: on the view it
//! looks exactly like a composition. What tells it apart is the bracket
//! AppKit puts around it, which only **private** `NSTextInputContext`
//! methods show: `_dictationDidStart:` before the first word,
//! `_dictationDidEnd:` at the end, whichever way dictation was started.
//! Both are hooked, so a view is dictating between them;
//! `NSApplication`'s `stopDictation:` ends every session, and closing the
//! window ends its own. The public `startDictation:` (the Edit menu) does
//! not open one while `_dictationDidStart:` is hooked: on macOS 27 it fired
//! while dictation never began, which would have left the next insertion
//! tagged as dictation.
//!
//! The end does not always come last. When dictation ends on silence, the
//! last sentence is committed, then `_dictationDidEnd:` arrives. When the
//! user stops it, `_dictationDidEnd:` arrives about 50 ms **before** the
//! last sentence is committed, while it is still marked. So an end (or a
//! `stopDictation:`) that finds winit holding marked text leaves the session
//! ending rather than closed, and the session closes when that composition
//! is over: committed outside a key press (as dictation), unmarked by the
//! input method, marked as an empty string, or at the next `keyDown:`. The
//! rule is [`DictationSession`], tested on Linux.
//!
//! These are private API, used with the maintainer's approval because
//! nothing public tells dictation from a composition. Each is installed only
//! where the class has the selector with the type encoding checked here (an
//! object argument, no result); where a release drops or changes one, it is
//! not installed, nothing breaks, and dictation started from the dictation
//! key reads as [`ImeOrigin::Composition`] again. Without
//! `_dictationDidStart:`, `startDictation:` opens the session instead, so
//! dictation from the Edit menu still reads as dictation, and a request that
//! comes to nothing leaves the session open until `_dictationDidEnd:`,
//! `stopDictation:` or the window closing. `_stopDictation:` is not
//! used: AppKit sends it each time a window resigns key, dictating or not.
//!
//! # Keys the input method swallows
//!
//! winit hands a key press to the input method (`interpretKeyEvents:`, when
//! IME is allowed) and then reports it as a `KeyboardInput` with its text,
//! whatever the input method did, unless the input method composed. The
//! press-and-hold accent popup takes the held letter's auto-repeats and the
//! key that picks an accent without calling anything on the view, then
//! inserts the accent on its own, so their text was typed too ("eeê" for
//! "ê"). The view now records, per `keyDown:`, whether
//! `interpretKeyEvents:` ran and whether the input method acted on the view
//! meanwhile (`insertText:replacementRange:`,
//! `setMarkedText:selectedRange:replacementRange:`, `unmarkText`,
//! `doCommandBySelector:`), the bookkeeping in
//! [`KeyPress`]. The application asks
//! [`key_was_swallowed`] while handling the `KeyboardInput`, which winit
//! delivers from inside that `keyDown:`, and drops the text of a swallowed
//! key; the key itself is still delivered. A key with no input method
//! (`interpretKeyEvents:` never ran), typing (`insertText:`), an arrow or
//! Escape (`doCommandBySelector:`) and a dead key (`setMarkedText:`) are not
//! swallowed. When winit buffers the event because its handler is busy, it
//! arrives after the `keyDown:` and its text is typed, as before.
//!
//! Not handled yet, and traced as such for the next measurement to decide:
//! an `insertText:` with a replacement range inside a key press, and a
//! `setMarkedText:` with one. Both still go to winit, which ignores the
//! range.
//!
//! # How the classes are patched
//!
//! The hook replaces methods of the `WinitView` class at run time and keeps
//! each original implementation, which the replacement calls where it runs
//! the original. A method the class declares itself is replaced in place
//! (`method_setImplementation`); one it inherits gets an override on the
//! class (`class_addMethod`), since replacing the inherited implementation
//! would change it for every class that shares it. A method is replaced only
//! once its type encoding has been checked against the replacement's
//! signature; one that is missing, or differs, is left alone and the trace
//! says so. AccessKit moves each view to a runtime subclass of `WinitView`
//! that overrides accessibility selectors only, so those views reach the
//! replacements through inheritance. The Edit menu's actions
//! (`showWritingTools:`, `startDictation:`, `stopDictation:`,
//! `orderFrontCharacterPalette:`) are hooked the same way on `NSApplication`
//! (and all but `stopDictation:` on the view, should it ever inherit them),
//! and `NSTextInputContext`'s private dictation and Writing Tools methods
//! where the class has them.
//!
//! The classes are patched once per process, at the first attach; patching
//! again finds its own implementations in place and changes nothing. Each
//! [`Hook`] says when it goes in: those the behaviour above needs (on the
//! view the answers, `keyDown:`, `interpretKeyEvents:` and the input
//! method's actions; on `NSApplication` `showWritingTools:`,
//! `startDictation:` and `stopDictation:`; on `NSTextInputContext` the two
//! `_dictationDid…:`) always, every other one only with tracing on, and
//! those only trace and call the original. The methods the
//! view lacks are added with `class_addMethod` ([`Addition`]), each only
//! where neither `WinitView` nor a superclass has the selector, with a type
//! encoding written from its implementation's signature and checked once
//! added. winit's own
//! `hasMarkedText` and `unmarkText` are recorded before anything is replaced,
//! so the hooks call them directly whether the trace replaced them or not.
//! Views are registered per window, and a callback on a view that is not
//! (never attached, or detached since) runs winit's own method unchanged.
//!
//! # Tracing
//!
//! ```text
//! TEKSILO_TEXT_INPUT_TRACE=1 cargo run -p rich-text-editor
//! ```
//!
//! Each line names the selector, the view and its window, the depth of
//! `keyDown:` calls it runs inside, whether the view holds marked text (as
//! winit's own `hasMarkedText` answers, `?` where it cannot be asked), the
//! type of `NSApp.currentEvent`, and then, after a `|`, the arguments and
//! the result, with where an answer came from (the document, winit, or no
//! document). A line repeated as is is written once, then counted. An
//! `insertText:` the hook takes over is followed by a line saying what it did
//! with the text.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::fmt::{self, Write as _};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use objc2::encode::{Encode, EncodeArguments, EncodeReturn};
use objc2::rc::Retained;
use objc2::runtime::{
    AnyClass, AnyObject, Bool, Imp, Method, MethodImplementation, Sel, VerificationError,
};
use objc2::{MainThreadMarker, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSEvent, NSEventType, NSResponder, NSTextAlternativesAttributeName, NSView,
};
use objc2_foundation::{NSArray, NSAttributedString, NSPoint, NSRange, NSRect, NSSize, NSString};
use raw_window_handle::RawWindowHandle;
use teksilo_canvas::Point as WindowPoint;
use teksilo_canvas::Rect as WindowRect;
use teksilo_core::raw_handle::ParentHandle;
use teksilo_core::text_input_document::TextInputDocument;
use teksilo_core::text_surface::TextSurfaces;
use teksilo_core::window::TeksiloWindowId;
use teksilo_core::{AppEventPoster, ImeOrigin};

use super::answers::{self, NOT_FOUND, Substring, Utf16Range};
use super::dictation::{DictationEvent, DictationSession};
use super::key_press::KeyPress;
use super::{CommitSignals, ImeCommitPayload};

// ---------------------------------------------------------------------------
// The attached views
// ---------------------------------------------------------------------------

/// A view attached to a window.
struct AttachedView {
    window_id: TeksiloWindowId,
    /// Where the text the view is sent outside a key press is posted.
    poster: Arc<dyn AppEventPoster>,
    /// The window tree's text surfaces: the view answers the input system
    /// from the focused one's document.
    surfaces: TextSurfaces,
    /// Whether Writing Tools was opened on the view and no key was pressed
    /// in it since: what it inserts meanwhile is
    /// [`ImeOrigin::WritingTools`]. Set by `NSApplication`'s
    /// `showWritingTools:`, cleared by the view's next `keyDown:`, and left
    /// alone when the window resigns key, since the Writing Tools panel takes
    /// key while it works. Provisional, like [`classify`](super::classify).
    writing_tools: Cell<bool>,
    /// Where dictation stands on the view: what it inserts while it is open
    /// or ending is [`ImeOrigin::Dictation`]. Opened by the input context's
    /// private `_dictationDidStart:` (by `NSApplication`'s `startDictation:`
    /// only where that hook is not installed); `_dictationDidEnd:` and
    /// `stopDictation:` close it, or leave it ending while winit holds
    /// marked text, until that composition is over. See
    /// [`DictationSession`].
    dictation: Cell<DictationSession>,
}

thread_local! {
    /// The attached views, by `NSView` address. Main thread only, where
    /// AppKit creates views and calls their text input methods.
    static VIEWS: RefCell<HashMap<usize, AttachedView>> = RefCell::new(HashMap::new());

    /// How many `keyDown:` calls are running, innermost included.
    static KEY_DOWN_DEPTH: Cell<u32> = const { Cell::new(0) };

    /// The running `keyDown:` calls, innermost last: the view's address and
    /// what the input method did with the key.
    static KEY_PRESSES: RefCell<Vec<(usize, KeyPress)>> = const { RefCell::new(Vec::new()) };

    /// The selectors `respondsToSelector:` was already asked about, each
    /// traced once.
    static PROBED: RefCell<HashSet<Sel>> = RefCell::new(HashSet::new());

    /// The last trace line written, and how many times it came again since.
    static LAST_LINE: RefCell<(String, u32)> = const { RefCell::new((String::new(), 0)) };
}

pub(super) fn attach(
    window_id: TeksiloWindowId,
    parent: &ParentHandle,
    poster: Arc<dyn AppEventPoster>,
    surfaces: TextSurfaces,
) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = parent.raw_window_handle() else {
        return;
    };
    let view = handle.ns_view.as_ptr() as usize;
    VIEWS.with_borrow_mut(|views| {
        views.insert(
            view,
            AttachedView {
                window_id,
                poster,
                surfaces,
                writing_tools: Cell::new(false),
                dictation: Cell::new(DictationSession::Closed),
            },
        );
    });
    install(mtm);
}

pub(super) fn detach(window_id: TeksiloWindowId) {
    let _ = VIEWS.try_with(|views| {
        if let Ok(mut views) = views.try_borrow_mut() {
            views.retain(|_, view| view.window_id != window_id);
        }
    });
}

/// `f` of the attachment of `view`, if it is attached. `None` too while the
/// views are borrowed, which a callback reentering an attach or a detach
/// would find: the view is then treated as unattached.
fn with_attached<R>(view: *const AnyObject, f: impl FnOnce(&AttachedView) -> R) -> Option<R> {
    VIEWS
        .try_with(|views| {
            let views = views.try_borrow().ok()?;
            views.get(&(view as usize)).map(f)
        })
        .ok()
        .flatten()
}

/// The window `view` is attached to, if it is.
fn attached_window(view: *const AnyObject) -> Option<TeksiloWindowId> {
    with_attached(view, |view| view.window_id)
}

/// Ask the document of the text widget that has the keyboard focus in
/// `view`'s window. `None` when the view is not attached, when no text widget
/// with a document has the focus, when `ask` answers `None` (the document is
/// busy, or has nothing to say), and when anything on the way panics, which
/// stops here: the caller then answers as winit would.
fn ask_document<R>(
    view: *mut AnyObject,
    ask: impl FnOnce(&dyn TextInputDocument) -> Option<R>,
) -> Option<R> {
    // Cloned out, so the views are not borrowed while the widget answers.
    let surfaces = with_attached(view, |attached| attached.surfaces.clone())?;
    catch_unwind(AssertUnwindSafe(|| {
        surfaces.with_focused_input_document(ask).flatten()
    }))
    .ok()
    .flatten()
}

/// Run `answer`; a panic in it stops here and gives `fallback()`: nothing
/// unwinds into AppKit.
fn guarded<R>(answer: impl FnOnce() -> R, fallback: impl FnOnce() -> R) -> R {
    catch_unwind(AssertUnwindSafe(answer)).unwrap_or_else(|_| fallback())
}

/// Where an answer to the input system came from, for the trace.
#[derive(Clone, Copy)]
enum Source {
    /// The focused text widget's document.
    Document,
    /// winit's own method.
    Winit,
    /// No document answered, for a method winit does not have.
    NoDocument,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Document => "from the document",
            Self::Winit => "from winit",
            Self::NoDocument => "no document",
        })
    }
}

fn ns_range(range: Utf16Range) -> NSRange {
    NSRange::new(range.location, range.length)
}

fn utf16_range(range: NSRange) -> Utf16Range {
    Utf16Range {
        location: range.location,
        length: range.length,
    }
}

/// `view` as an `NSView`, if it is one.
fn as_view<'a>(view: *mut AnyObject) -> Option<&'a NSView> {
    // SAFETY: AppKit passes the receiver, alive for the call.
    unsafe { view.as_ref() }?.downcast_ref::<NSView>()
}

/// A window-logical rectangle in screen coordinates. Window-logical is
/// `WinitView`'s own space: it is flipped (top-left origin, as Teksilo's
/// window coordinates are) and measured in points, and Teksilo hands winit
/// its IME cursor area in it unchanged. So this converts as winit's own
/// `firstRectForCharacterRange:actualRange:` does: to the window with
/// `convertRect:toView:nil`, then to the screen with the window's
/// `convertRectToScreen:`. `None` for a view not in a window.
fn window_rect_to_screen(view: *mut AnyObject, rect: WindowRect) -> Option<NSRect> {
    let view = as_view(view)?;
    let window = view.window()?;
    let rect = NSRect::new(
        NSPoint::new(f64::from(rect.x), f64::from(rect.y)),
        NSSize::new(f64::from(rect.width), f64::from(rect.height)),
    );
    Some(window.convertRectToScreen(view.convertRect_toView(rect, None)))
}

/// A screen point in window-logical coordinates: the way back from
/// [`window_rect_to_screen`], with the window's `convertPointFromScreen:`
/// then `convertPoint:fromView:nil`.
fn screen_point_to_window(view: *mut AnyObject, point: NSPoint) -> Option<WindowPoint> {
    let view = as_view(view)?;
    let window = view.window()?;
    let point = view.convertPoint_fromView(window.convertPointFromScreen(point), None);
    // Window-logical coordinates are `f32`, as every Teksilo coordinate is;
    // a point on a screen fits one.
    Some(WindowPoint::new(point.x as f32, point.y as f32))
}

// ---------------------------------------------------------------------------
// Installing
// ---------------------------------------------------------------------------

/// One replaced method's place in [`ORIGINALS`].
#[derive(Clone, Copy)]
enum Slot {
    KeyDown,
    InsertText,
    SetMarkedText,
    UnmarkText,
    HasMarkedText,
    MarkedRange,
    SelectedRange,
    AttributedSubstring,
    CharacterIndex,
    FirstRect,
    ValidAttributes,
    DoCommand,
    RespondsToSelector,
    InterpretKeyEvents,
    ViewShowWritingTools,
    ViewStartDictation,
    ViewCharacterPalette,
    AppShowWritingTools,
    AppStartDictation,
    AppCharacterPalette,
    AppStopDictation,
    ContextDictationDidStart,
    ContextDictationDidEnd,
    ContextStopDictation,
    ContextShowWritingTools,
}

impl Slot {
    const fn index(self) -> usize {
        self as usize
    }
}

/// One slot per [`Slot`]: the last one's index, plus one.
const SLOTS: usize = Slot::ContextShowWritingTools.index() + 1;

/// The implementation each replacement took the place of. Set before the
/// replacement is installed, never changed after.
static ORIGINALS: [OnceLock<Imp>; SLOTS] = [const { OnceLock::new() }; SLOTS];

/// The `WinitView` class, once found.
static WINIT_VIEW: OnceLock<&'static AnyClass> = OnceLock::new();

/// Whether `NSTextInputContext`'s private `_dictationDidStart:` runs
/// [`context_dictation_did_start`], so dictation reports its own start. Set
/// once the classes are patched.
static DICTATION_STARTS_REPORTED: AtomicBool = AtomicBool::new(false);

/// A method of winit's view the hooks call without replacing it, or call
/// past the trace's replacement of it.
#[derive(Clone, Copy)]
enum Own {
    HasMarkedText,
    UnmarkText,
}

impl Own {
    const fn index(self) -> usize {
        self as usize
    }
}

/// winit's own implementation of each [`Own`] method, recorded before any
/// hook goes in and checked against the signature it is called with.
static OWN: [OnceLock<Imp>; 2] = [const { OnceLock::new() }; 2];

/// `- (void)method:(id)argument`: `keyDown:` and the menu actions.
type ObjectArgFn = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject);
/// `- (void)method`
type VoidFn = unsafe extern "C-unwind" fn(*mut AnyObject, Sel);
/// `- (void)method:(id)first second:(id)second`
type TwoObjectArgFn =
    unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject);
/// `- (BOOL)method`
type BoolFn = unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> Bool;
/// `- (NSRange)method`
type RangeFn = unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> NSRange;
/// `- (id)method`
type ObjectFn = unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> *mut AnyObject;
/// `insertText:replacementRange:`
type InsertTextFn = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, NSRange);
/// `setMarkedText:selectedRange:replacementRange:`
type SetMarkedTextFn =
    unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, NSRange, NSRange);
/// `attributedSubstringForProposedRange:actualRange:`
type SubstringFn =
    unsafe extern "C-unwind" fn(*mut AnyObject, Sel, NSRange, *mut NSRange) -> *mut AnyObject;
/// `characterIndexForPoint:`
type IndexForPointFn = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, NSPoint) -> usize;
/// `firstRectForCharacterRange:actualRange:`
type FirstRectFn =
    unsafe extern "C-unwind" fn(*mut AnyObject, Sel, NSRange, *mut NSRange) -> NSRect;
/// `doCommandBySelector:`
type SelectorArgFn = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, Option<Sel>);
/// `respondsToSelector:`
type RespondsFn = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, Option<Sel>) -> Bool;
/// `- (NSRect)method`
type RectFn = unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> NSRect;
/// `- (NSInteger)method`
type IntegerFn = unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> isize;
/// `- (NSUInteger)method`
type UnsignedFn = unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> usize;

/// Call the implementation a replacement took the place of, with the
/// replacement's own arguments. `$fallback` when there is none, which
/// cannot happen: the slot is filled before the replacement is installed.
macro_rules! call_original {
    ($slot:expr, $fn_type:ty, $fallback:expr, ($($arg:expr),* $(,)?)) => {
        match ORIGINALS[$slot].get() {
            // SAFETY: the slot holds the implementation the running
            // replacement took the place of, whose type encoding
            // `install_hook` checked against `$fn_type` (the replacement's
            // own signature) before replacing it. The arguments are the
            // replacement's own, passed on unchanged.
            Some(&original) => unsafe {
                std::mem::transmute::<Imp, $fn_type>(original)($($arg),*)
            },
            None => $fallback,
        }
    };
}

/// When a [`Hook`] goes in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Install {
    /// Always: the replacement changes what the method does, and traces it
    /// when tracing is on.
    Always,
    /// Only with tracing on: the replacement traces the call and runs the
    /// original, changing nothing.
    WhenTracing,
}

/// A method to replace.
struct Hook {
    install: Install,
    selector: &'static CStr,
    /// Its place in [`ORIGINALS`].
    slot: usize,
    replacement: Imp,
    /// Whether a class's method has the replacement's signature.
    verify: fn(&AnyClass, Sel) -> Result<(), VerificationError>,
}

/// The signature check for an implementation of type `F`.
fn verifier<F: MethodImplementation>(_: F) -> fn(&AnyClass, Sel) -> Result<(), VerificationError> {
    |class, selector| class.verify_sel::<F::Arguments, F::Return>(selector)
}

/// A [`Hook`] replacing `$selector` with `$replacement`, a function of
/// type `$fn_type`, whose original goes to `$slot`. `$install` names the
/// [`Install`] variant that says when it goes in.
macro_rules! hook {
    ($install:ident, $selector:expr, $slot:expr, $replacement:expr, $fn_type:ty) => {{
        let replacement: $fn_type = $replacement;
        Hook {
            install: Install::$install,
            selector: $selector,
            slot: $slot.index(),
            // SAFETY: `Imp` is the untyped form of a method implementation.
            // `install_hook` installs it only where the method's type
            // encoding matches `$fn_type`.
            replacement: unsafe { std::mem::transmute::<$fn_type, Imp>(replacement) },
            verify: verifier(replacement),
        }
    }};
}

/// A trace-only [`Hook`] on a menu action, traced by `$wrapper`.
macro_rules! action_hook {
    ($selector:expr, $slot:ident, $wrapper:ident) => {
        hook!(
            WhenTracing,
            $selector,
            Slot::$slot,
            $wrapper::<{ Slot::$slot.index() }>,
            ObjectArgFn
        )
    };
}

/// The hooks on `WinitView`. Always in: `keyDown:` and
/// `interpretKeyEvents:` (the key press and its hand-over to the input
/// method), `insertText:replacementRange:`, the input method's other actions
/// (`setMarkedText:selectedRange:replacementRange:`, `unmarkText`,
/// `doCommandBySelector:`, which only note that it acted), and those that
/// answer from the focused document. The rest trace. `hasMarkedText`,
/// `setMarkedText:` and `unmarkText` still run winit's own method: its
/// `keyDown:` reads its own marked text state.
fn view_hooks() -> [Hook; 17] {
    [
        hook!(Always, c"keyDown:", Slot::KeyDown, key_down, ObjectArgFn),
        hook!(
            Always,
            c"interpretKeyEvents:",
            Slot::InterpretKeyEvents,
            interpret_key_events,
            ObjectArgFn
        ),
        hook!(
            Always,
            c"insertText:replacementRange:",
            Slot::InsertText,
            insert_text,
            InsertTextFn
        ),
        hook!(
            Always,
            c"setMarkedText:selectedRange:replacementRange:",
            Slot::SetMarkedText,
            set_marked_text,
            SetMarkedTextFn
        ),
        hook!(Always, c"unmarkText", Slot::UnmarkText, unmark_text, VoidFn),
        hook!(
            WhenTracing,
            c"hasMarkedText",
            Slot::HasMarkedText,
            has_marked_text,
            BoolFn
        ),
        hook!(
            Always,
            c"markedRange",
            Slot::MarkedRange,
            marked_range,
            RangeFn
        ),
        hook!(
            Always,
            c"selectedRange",
            Slot::SelectedRange,
            selected_range,
            RangeFn
        ),
        hook!(
            Always,
            c"attributedSubstringForProposedRange:actualRange:",
            Slot::AttributedSubstring,
            attributed_substring,
            SubstringFn
        ),
        hook!(
            Always,
            c"characterIndexForPoint:",
            Slot::CharacterIndex,
            character_index_for_point,
            IndexForPointFn
        ),
        hook!(
            Always,
            c"firstRectForCharacterRange:actualRange:",
            Slot::FirstRect,
            first_rect,
            FirstRectFn
        ),
        hook!(
            WhenTracing,
            c"validAttributesForMarkedText",
            Slot::ValidAttributes,
            valid_attributes,
            ObjectFn
        ),
        hook!(
            Always,
            c"doCommandBySelector:",
            Slot::DoCommand,
            do_command_by_selector,
            SelectorArgFn
        ),
        hook!(
            WhenTracing,
            c"respondsToSelector:",
            Slot::RespondsToSelector,
            responds_to_selector,
            RespondsFn
        ),
        action_hook!(c"showWritingTools:", ViewShowWritingTools, view_action),
        action_hook!(c"startDictation:", ViewStartDictation, view_action),
        action_hook!(
            c"orderFrontCharacterPalette:",
            ViewCharacterPalette,
            view_action
        ),
    ]
}

/// The hooks on `NSApplication`. `showWritingTools:`, `startDictation:` and
/// `stopDictation:` always go in, for the Writing Tools and dictation
/// sessions; the rest trace.
fn app_hooks() -> [Hook; 4] {
    [
        hook!(
            Always,
            c"showWritingTools:",
            Slot::AppShowWritingTools,
            app_show_writing_tools,
            ObjectArgFn
        ),
        hook!(
            Always,
            c"startDictation:",
            Slot::AppStartDictation,
            app_start_dictation,
            ObjectArgFn
        ),
        action_hook!(
            c"orderFrontCharacterPalette:",
            AppCharacterPalette,
            app_action
        ),
        hook!(
            Always,
            c"stopDictation:",
            Slot::AppStopDictation,
            app_stop_dictation,
            ObjectArgFn
        ),
    ]
}

/// A method `WinitView` lacks, added to it so the input system finds it.
/// Always installed, and only where neither the class nor a superclass
/// (`NSView`) has the selector.
struct Addition {
    selector: &'static CStr,
    implementation: Imp,
    /// The method's type encoding, written from the implementation's
    /// signature.
    types: CString,
    /// Whether the class's method, once added, has that signature.
    verify: fn(&AnyClass, Sel) -> Result<(), VerificationError>,
}

/// The Objective-C type encoding of an implementation of type `F`, as objc2
/// writes it for a method it declares: the return type, `self`, `_cmd`,
/// then each argument.
fn type_encoding_of<F: MethodImplementation>(_: F) -> CString {
    let mut types = format!(
        "{}{}{}",
        <F::Return as EncodeReturn>::ENCODING_RETURN,
        <*mut AnyObject as Encode>::ENCODING,
        <Sel as Encode>::ENCODING
    );
    for encoding in <F::Arguments as EncodeArguments>::ENCODINGS {
        let _ = write!(types, "{encoding}");
    }
    // An encoding never holds a NUL.
    CString::new(types).unwrap_or_default()
}

/// An [`Addition`] of `$selector`, answered by `$implementation`, a
/// function of type `$fn_type`.
macro_rules! addition {
    ($selector:expr, $implementation:expr, $fn_type:ty) => {{
        let implementation: $fn_type = $implementation;
        Addition {
            selector: $selector,
            // SAFETY: `Imp` is the untyped form of a method implementation;
            // the method is added with the type encoding of `$fn_type`.
            implementation: unsafe { std::mem::transmute::<$fn_type, Imp>(implementation) },
            types: type_encoding_of(implementation),
            verify: verifier(implementation),
        }
    }};
}

/// What Writing Tools asks the view before it acts (measured on macOS 27,
/// where `respondsToSelector:` answered no to each), and the visible text
/// rectangle `NSTextInputClient` describes.
fn view_additions() -> [Addition; 6] {
    [
        addition!(c"documentVisibleRect", document_visible_rect, RectFn),
        addition!(
            c"unionRectInVisibleSelectedRange",
            union_rect_in_visible_selected_range,
            RectFn
        ),
        addition!(c"isEditable", is_editable, BoolFn),
        addition!(c"writingToolsBehavior", writing_tools_behavior, IntegerFn),
        addition!(
            c"allowsWritingToolsAffordance",
            allows_writing_tools_affordance,
            BoolFn
        ),
        addition!(
            c"allowedWritingToolsResultOptions",
            allowed_writing_tools_result_options,
            UnsignedFn
        ),
    ]
}

/// Add `addition` to `class`, unless the class or a superclass already has
/// the selector, which is then left alone.
fn add_method(class: &AnyClass, addition: Addition) {
    let selector = Sel::register(addition.selector);
    let class_name = class.name().to_string_lossy();
    if let Some(existing) = class.instance_method(selector) {
        if existing.implementation() as usize != addition.implementation as usize {
            let encoding = type_encoding(existing);
            trace_args(format_args!(
                "install: {class_name} already has {selector} ({encoding}), not added"
            ));
        }
        return;
    }
    // The runtime keeps the pointer: the string lives as long as the
    // process, and is made once per selector.
    let types: &'static CStr = Box::leak(addition.types.into_boxed_c_str());
    // SAFETY: the implementation has the signature `types` describes, and
    // the class has no method for the selector.
    let added = unsafe {
        objc2::ffi::class_addMethod(
            class as *const AnyClass as *mut AnyClass,
            selector,
            addition.implementation,
            types.as_ptr(),
        )
    };
    if !added.as_bool() {
        trace_args(format_args!(
            "install: {class_name} {selector}: class_addMethod refused, not added"
        ));
        return;
    }
    match (addition.verify)(class, selector) {
        Ok(()) => trace_args(format_args!(
            "install: {class_name} {selector} added ({})",
            types.to_string_lossy()
        )),
        Err(error) => trace_args(format_args!(
            "install: {class_name} {selector} added, but its signature does not check ({error})"
        )),
    }
}

/// Private methods, found on macOS 27; each is skipped where it is missing,
/// or where its type encoding is not the one checked here. The two
/// `_dictationDid…:` always go in, for the dictation session: AppKit sends
/// them around a dictation however it was started (the Edit menu, the
/// dictation key). `_stopDictation:` only traces: it is sent each time a
/// window resigns key, dictating or not.
fn context_hooks() -> [Hook; 4] {
    [
        hook!(
            Always,
            c"_dictationDidStart:",
            Slot::ContextDictationDidStart,
            context_dictation_did_start,
            ObjectArgFn
        ),
        hook!(
            Always,
            c"_dictationDidEnd:",
            Slot::ContextDictationDidEnd,
            context_dictation_did_end,
            ObjectArgFn
        ),
        action_hook!(c"_stopDictation:", ContextStopDictation, context_action),
        hook!(
            WhenTracing,
            c"showWritingTools:forDelegate:",
            Slot::ContextShowWritingTools,
            context_show_writing_tools,
            TwoObjectArgFn
        ),
    ]
}

/// Patch the classes, once per process. On the main thread, once winit has
/// created a window, so that `WinitView` is registered.
fn install(_mtm: MainThreadMarker) {
    static INSTALLED: AtomicBool = AtomicBool::new(false);
    if INSTALLED.load(Ordering::Relaxed) {
        return;
    }
    let Some(view_class) = AnyClass::get(c"WinitView") else {
        trace("install: no WinitView class yet, nothing hooked");
        return;
    };
    INSTALLED.store(true, Ordering::Relaxed);
    let _ = WINIT_VIEW.set(view_class);
    // Before any hook: what is recorded must be winit's, not a replacement.
    record_own(
        view_class,
        Own::HasMarkedText,
        c"hasMarkedText",
        |class, selector| class.verify_sel::<(), Bool>(selector),
    );
    record_own(
        view_class,
        Own::UnmarkText,
        c"unmarkText",
        |class, selector| class.verify_sel::<(), ()>(selector),
    );
    let tracing = super::trace_enabled();
    let wanted = |hook: &&Hook| tracing || hook.install == Install::Always;
    for hook in view_hooks().iter().filter(wanted) {
        install_hook(view_class, hook);
    }
    for addition in view_additions() {
        add_method(view_class, addition);
    }
    match AnyClass::get(c"NSApplication") {
        Some(class) => app_hooks()
            .iter()
            .filter(wanted)
            .for_each(|hook| install_hook(class, hook)),
        None => trace("install: no NSApplication class"),
    }
    match AnyClass::get(c"NSTextInputContext") {
        Some(class) => context_hooks()
            .iter()
            .filter(wanted)
            .for_each(|hook| install_hook(class, hook)),
        None => trace("install: no NSTextInputContext class"),
    }
    let did_start: ObjectArgFn = context_dictation_did_start;
    let starts_reported = AnyClass::get(c"NSTextInputContext")
        .and_then(|class| class.instance_method(sel!(_dictationDidStart:)))
        .is_some_and(|method| method.implementation() as usize == did_start as usize);
    DICTATION_STARTS_REPORTED.store(starts_reported, Ordering::Relaxed);
    if !starts_reported {
        trace("install: dictation starts are not reported; startDictation: opens the session");
    }
    if tracing {
        trace_related_methods();
    }
}

/// Record `class`'s implementation of `selector` as `own`, if the class has
/// one and `verify` accepts its signature.
fn record_own(
    class: &AnyClass,
    own: Own,
    selector: &CStr,
    verify: fn(&AnyClass, Sel) -> Result<(), VerificationError>,
) {
    let selector = Sel::register(selector);
    let class_name = class.name().to_string_lossy();
    let Some(method) = class.instance_method(selector) else {
        trace_args(format_args!(
            "install: {class_name} has no {selector}, not recorded"
        ));
        return;
    };
    if let Err(error) = verify(class, selector) {
        let encoding = type_encoding(method);
        trace_args(format_args!(
            "install: {class_name} {selector} is {encoding} ({error}), not recorded"
        ));
        return;
    }
    let _ = OWN[own.index()].set(method.implementation());
}

/// Replace one method of `class`, keeping its implementation in the hook's
/// slot. Nothing happens when the method is missing, has another
/// signature, or already runs the replacement.
fn install_hook(class: &AnyClass, hook: &Hook) {
    let selector = Sel::register(hook.selector);
    let class_name = class.name().to_string_lossy();
    let Some(method) = class.instance_method(selector) else {
        trace_args(format_args!(
            "install: {class_name} has no {selector}, not hooked"
        ));
        return;
    };
    let current = method.implementation();
    if current as usize == hook.replacement as usize {
        return;
    }
    if let Err(error) = (hook.verify)(class, selector) {
        let encoding = type_encoding(method);
        trace_args(format_args!(
            "install: {class_name} {selector} is {encoding} ({error}), not hooked"
        ));
        return;
    }
    let inherited = class
        .superclass()
        .and_then(|superclass| superclass.instance_method(selector))
        .is_some_and(|inherited| ptr::eq(inherited, method));
    if ORIGINALS[hook.slot].set(current).is_err() {
        trace_args(format_args!(
            "install: {class_name} {selector}: its slot is taken, not hooked"
        ));
        return;
    }
    if inherited {
        // SAFETY: the replacement has the signature of the inherited method
        // (checked above), and the types string is that method's own.
        let added = unsafe {
            objc2::ffi::class_addMethod(
                class as *const AnyClass as *mut AnyClass,
                selector,
                hook.replacement,
                objc2::ffi::method_getTypeEncoding(method),
            )
        };
        let outcome = if added.as_bool() {
            "hooked (an override of the inherited method)"
        } else {
            "not hooked (class_addMethod refused)"
        };
        trace_args(format_args!("install: {class_name} {selector} {outcome}"));
    } else {
        // SAFETY: the replacement has the method's signature (checked
        // above) and calls the implementation it replaces, so it is as safe
        // as that one.
        let replaced = unsafe { method.set_implementation(hook.replacement) };
        let outcome = if replaced as usize == current as usize {
            "hooked"
        } else {
            "hooked, but its implementation changed while being replaced"
        };
        trace_args(format_args!("install: {class_name} {selector} {outcome}"));
    }
}

/// A method's type encoding as the runtime holds it, `?` when it has none.
fn type_encoding(method: &Method) -> Cow<'_, str> {
    // SAFETY: `method` is a live method; the runtime returns its type
    // encoding as a C string it owns, or null.
    let encoding = unsafe { objc2::ffi::method_getTypeEncoding(method) };
    if encoding.is_null() {
        return Cow::Borrowed("?");
    }
    // SAFETY: non-null and NUL-terminated, and it lives as long as the
    // method (method descriptions are never freed).
    unsafe { CStr::from_ptr(encoding) }.to_string_lossy()
}

/// Trace the methods whose names mention dictation, writing tools, the
/// character palette or AutoFill on the classes the text input system
/// works through, with their type encodings: what a later hook may need,
/// read from the system that runs.
fn trace_related_methods() {
    const CLASSES: [&CStr; 4] = [
        c"NSTextInputContext",
        c"NSApplication",
        c"NSResponder",
        c"NSView",
    ];
    const WORDS: [&str; 4] = ["dictation", "writingtools", "characterpalette", "autofill"];
    for name in CLASSES {
        let Some(class) = AnyClass::get(name) else {
            continue;
        };
        for method in class.instance_methods().iter() {
            let selector = method.name();
            let lowered = selector.name().to_string_lossy().to_ascii_lowercase();
            if !WORDS.iter().any(|word| lowered.contains(word)) {
                continue;
            }
            trace_args(format_args!(
                "related: {} {selector} {}",
                name.to_string_lossy(),
                type_encoding(method)
            ));
        }
    }
}

// ---------------------------------------------------------------------------
// Tracing
// ---------------------------------------------------------------------------

fn trace(message: &str) {
    super::trace(format_args!("{message}"));
}

fn trace_args(args: fmt::Arguments<'_>) {
    super::trace(args);
}

/// Build and write one trace line, if tracing is on. A line identical to
/// the one before it is counted rather than written (AppKit asks the same
/// question many times over), and the count is written before the next
/// different line. A panic while doing so stops here: it must not unwind
/// into AppKit, and the original method is called whatever happens to the
/// line.
fn log(build: impl FnOnce(&mut String)) {
    if !super::trace_enabled() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let mut line = String::new();
        build(&mut line);
        let repeated = LAST_LINE
            .try_with(|last| {
                let Ok(mut last) = last.try_borrow_mut() else {
                    return Some(0);
                };
                let (last_line, repeats) = &mut *last;
                if *last_line == line {
                    *repeats += 1;
                    return None;
                }
                let earlier = *repeats;
                *last_line = line.clone();
                *repeats = 0;
                Some(earlier)
            })
            .unwrap_or(Some(0));
        match repeated {
            None => {}
            Some(0) => super::trace(format_args!("{line}")),
            Some(earlier) => {
                super::trace(format_args!("(the line before, {earlier} more times)"));
                super::trace(format_args!("{line}"));
            }
        }
    }));
}

/// The `keyDown:` depth, raised for as long as the value lives (unwinding
/// included), and the key press's record in [`KEY_PRESSES`].
struct KeyDownScope {
    /// Whether the record went in, so that only it is taken out.
    pushed: bool,
}

impl KeyDownScope {
    fn enter(view: *mut AnyObject) -> Self {
        let _ = KEY_DOWN_DEPTH.try_with(|depth| depth.set(depth.get().saturating_add(1)));
        let pushed = KEY_PRESSES
            .try_with(|presses| {
                presses
                    .try_borrow_mut()
                    .map(|mut presses| presses.push((view as usize, KeyPress::default())))
                    .is_ok()
            })
            .unwrap_or(false);
        Self { pushed }
    }
}

impl Drop for KeyDownScope {
    fn drop(&mut self) {
        let _ = KEY_DOWN_DEPTH.try_with(|depth| depth.set(depth.get().saturating_sub(1)));
        if self.pushed {
            let _ = KEY_PRESSES.try_with(|presses| {
                if let Ok(mut presses) = presses.try_borrow_mut() {
                    presses.pop();
                }
            });
        }
    }
}

fn key_down_depth() -> u32 {
    KEY_DOWN_DEPTH.try_with(Cell::get).unwrap_or(0)
}

/// Run `f` on the innermost running key press: the view it was sent to,
/// and what the input method did with it. `None` outside `keyDown:`.
fn with_key_press<R>(f: impl FnOnce(usize, &mut KeyPress) -> R) -> Option<R> {
    KEY_PRESSES
        .try_with(|presses| {
            let mut presses = presses.try_borrow_mut().ok()?;
            let (view, press) = presses.last_mut()?;
            Some(f(*view, press))
        })
        .ok()
        .flatten()
}

/// The input method acted on a view: a call it makes while it has a key
/// (inside `interpretKeyEvents:`) is its answer to that key.
fn note_input_method_acted() {
    let _ = with_key_press(|_, press| press.note_acted());
}

/// See [`super::key_was_swallowed`]. The innermost running `keyDown:` must
/// be on `window_id`'s view; winit queues its `KeyboardInput` inside that
/// `keyDown:`, after `interpretKeyEvents:`, and delivers it there when its
/// event handler is idle. When the handler is busy winit runs the event
/// later, outside the `keyDown:`, and this answers `false`: the text is
/// typed, as it was before this check existed.
pub(super) fn key_was_swallowed(window_id: TeksiloWindowId) -> bool {
    let Some((view, swallowed)) = with_key_press(|view, press| (view, press.swallowed())) else {
        return false;
    };
    let swallowed = swallowed && attached_window(view as *const AnyObject) == Some(window_id);
    if swallowed {
        log(|line| {
            let _ = write!(
                line,
                "keyDown: swallowed by the input method, its text is not typed {}",
                Context::of(view as *mut AnyObject)
            );
        });
    }
    swallowed
}

/// Whether `object` is an instance of `class` or of a subclass.
fn is_kind_of(object: &AnyObject, class: &AnyClass) -> bool {
    // SAFETY: `isKindOfClass:` takes a class and returns a BOOL; every
    // object answers it.
    unsafe { msg_send![object, isKindOfClass: class] }
}

/// Whether the view holds marked text, as winit's own `hasMarkedText`
/// answers. `None` for anything but an attached `WinitView`, whose state
/// winit's method reads.
fn original_has_marked_text(view: *mut AnyObject) -> Option<bool> {
    attached_window(view)?;
    let winit_view = WINIT_VIEW.get()?;
    // SAFETY: the address belongs to a live object (it was handed to the
    // caller by AppKit); checked to be a `WinitView` before winit's method
    // reads its state.
    let object = unsafe { view.as_ref() }?;
    if !is_kind_of(object, winit_view) {
        return None;
    }
    let own = *OWN[Own::HasMarkedText.index()].get()?;
    // SAFETY: `OWN` holds `WinitView`'s own `hasMarkedText`, whose signature
    // `record_own` checked against `BoolFn`'s; `view` is a `WinitView`.
    let marked = unsafe { std::mem::transmute::<Imp, BoolFn>(own)(view, sel!(hasMarkedText)) };
    Some(marked.as_bool())
}

/// Run winit's own `unmarkText` on `view`, an attached `WinitView`, past
/// the trace's replacement. winit then clears its marked text, has the input
/// context discard its own, reports an empty preedit (synchronously, unless
/// an event handler is running) and returns to its ground IME state.
/// `false` when winit's method was not recorded, and nothing ran.
fn winit_unmark_text(view: *mut AnyObject) -> bool {
    let Some(&own) = OWN[Own::UnmarkText.index()].get() else {
        return false;
    };
    // SAFETY: `OWN` holds `WinitView`'s own `unmarkText`, whose signature
    // `record_own` checked against `VoidFn`'s; the caller checked that
    // `view` is an attached `WinitView`.
    unsafe { std::mem::transmute::<Imp, VoidFn>(own)(view, sel!(unmarkText)) };
    true
}

/// Where a call landed, and the state around it.
struct Context {
    view: *mut AnyObject,
    window: Option<TeksiloWindowId>,
    depth: u32,
    marked: Option<bool>,
    /// `None` off the main thread; `Some(None)` with no current event.
    event: Option<Option<NSEventType>>,
}

impl Context {
    fn of(view: *mut AnyObject) -> Self {
        Self::with_marked(view, original_has_marked_text(view))
    }

    fn with_marked(view: *mut AnyObject, marked: Option<bool>) -> Self {
        let event = MainThreadMarker::new().map(|mtm| {
            NSApplication::sharedApplication(mtm)
                .currentEvent()
                .map(|event| event.r#type())
        });
        Self {
            view,
            window: attached_window(view),
            depth: key_down_depth(),
            marked,
            event,
        }
    }
}

impl fmt::Display for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.view.is_null() {
            f.write_str("view=nil")?;
        } else {
            write!(f, "view={:p}", self.view)?;
        }
        match self.window {
            Some(window) => write!(f, " window={}", window.raw())?,
            None => f.write_str(" window=unregistered")?,
        }
        write!(f, " depth={}", self.depth)?;
        match self.marked {
            Some(marked) => write!(f, " marked={marked}")?,
            None => f.write_str(" marked=?")?,
        }
        match self.event {
            None => f.write_str(" event=?"),
            Some(None) => f.write_str(" event=none"),
            Some(Some(kind)) => write!(f, " event={}", EventType(kind)),
        }
    }
}

/// An `NSEventType` by its name.
struct EventType(NSEventType);

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self.0 {
            NSEventType::LeftMouseDown => "LeftMouseDown",
            NSEventType::LeftMouseUp => "LeftMouseUp",
            NSEventType::RightMouseDown => "RightMouseDown",
            NSEventType::RightMouseUp => "RightMouseUp",
            NSEventType::MouseMoved => "MouseMoved",
            NSEventType::LeftMouseDragged => "LeftMouseDragged",
            NSEventType::RightMouseDragged => "RightMouseDragged",
            NSEventType::MouseEntered => "MouseEntered",
            NSEventType::MouseExited => "MouseExited",
            NSEventType::KeyDown => "KeyDown",
            NSEventType::KeyUp => "KeyUp",
            NSEventType::FlagsChanged => "FlagsChanged",
            NSEventType::AppKitDefined => "AppKitDefined",
            NSEventType::SystemDefined => "SystemDefined",
            NSEventType::ApplicationDefined => "ApplicationDefined",
            NSEventType::Periodic => "Periodic",
            NSEventType::CursorUpdate => "CursorUpdate",
            NSEventType::ScrollWheel => "ScrollWheel",
            NSEventType::OtherMouseDown => "OtherMouseDown",
            NSEventType::OtherMouseUp => "OtherMouseUp",
            NSEventType::OtherMouseDragged => "OtherMouseDragged",
            NSEventType::Gesture => "Gesture",
            NSEventType::Magnify => "Magnify",
            NSEventType::Swipe => "Swipe",
            NSEventType::Rotate => "Rotate",
            NSEventType::BeginGesture => "BeginGesture",
            NSEventType::EndGesture => "EndGesture",
            NSEventType::Pressure => "Pressure",
            other => return write!(f, "type{}", other.0),
        };
        f.write_str(name)
    }
}

/// An `NSRange` as `start..end`, or `none` at `NSNotFound`.
struct Range(NSRange);

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let NSRange { location, length } = self.0;
        if location == NOT_FOUND {
            if length == 0 {
                f.write_str("none")
            } else {
                write!(f, "none+{length}")
            }
        } else {
            write!(f, "{location}..{}", location.saturating_add(length))
        }
    }
}

/// An `NSRange` out-pointer, as the callee left it.
struct OutRange(*mut NSRange);

impl fmt::Display for OutRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // SAFETY: the caller of the traced method passed either null or a
        // pointer to an `NSRange` it owns for the length of the call.
        match unsafe { self.0.as_ref() } {
            Some(range) => write!(f, "{}", Range(*range)),
            None => f.write_str("null"),
        }
    }
}

/// An `NSUInteger` index, `none` at `NSNotFound`.
struct Index(usize);

impl fmt::Display for Index {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == NOT_FOUND {
            f.write_str("none")
        } else {
            write!(f, "{}", self.0)
        }
    }
}

struct Point(NSPoint);

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({:.1}, {:.1})", self.0.x, self.0.y)
    }
}

struct Rect(NSRect);

impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let NSRect { origin, size } = self.0;
        write!(
            f,
            "({:.1}, {:.1}) {:.1}x{:.1}",
            origin.x, origin.y, size.width, size.height
        )
    }
}

/// Longest text written out in full, in characters.
const TEXT_LIMIT: usize = 400;
/// Longest attribute value description, in characters.
const VALUE_LIMIT: usize = 80;
/// Most attribute runs written out.
const RUN_LIMIT: usize = 32;

/// Write `text` escaped, cut after `limit` characters.
fn write_escaped(out: &mut String, text: &str, limit: usize) {
    if text.chars().count() > limit {
        let cut: String = text.chars().take(limit).collect();
        let _ = write!(out, "{cut:?}...");
    } else {
        let _ = write!(out, "{text:?}");
    }
}

fn write_ns_string(out: &mut String, text: &NSString) {
    write_escaped(out, &text.to_string(), TEXT_LIMIT);
    let _ = write!(out, " len16={}", text.length());
}

fn class_name(object: &AnyObject) -> String {
    object.class().name().to_string_lossy().into_owned()
}

/// Write a string argument or result: its class, its text, its length in
/// UTF-16 units, and for an attributed string its attribute runs.
fn write_text(out: &mut String, object: *mut AnyObject) {
    // SAFETY: AppKit passes a live object or nil, alive for the call.
    let Some(object) = (unsafe { object.as_ref() }) else {
        out.push_str("nil");
        return;
    };
    if let Some(attributed) = object.downcast_ref::<NSAttributedString>() {
        out.push_str("NSAttributedString ");
        write_ns_string(out, &attributed.string());
        out.push_str(" attrs=[");
        write_attribute_runs(out, attributed);
        out.push(']');
    } else if let Some(text) = object.downcast_ref::<NSString>() {
        out.push_str("NSString ");
        write_ns_string(out, text);
    } else {
        out.push_str(&class_name(object));
    }
}

/// Write each attribute run of `text`: its range, then each attribute's
/// name and value.
fn write_attribute_runs(out: &mut String, text: &NSAttributedString) {
    let length = text.length();
    let mut index = 0;
    let mut runs = 0;
    while index < length {
        if runs == RUN_LIMIT {
            out.push_str(", ...");
            break;
        }
        let mut run = NSRange::new(0, 0);
        // SAFETY: `index` is inside the string, so nothing is raised, and
        // `run` is a valid out-pointer.
        let attributes = unsafe { text.attributesAtIndex_effectiveRange(index, &mut run) };
        if runs > 0 {
            out.push_str(", ");
        }
        let _ = write!(out, "{}", Range(run));
        let keys = attributes.allKeys();
        if keys.is_empty() {
            out.push_str(" (none)");
        }
        for key in keys.iter() {
            let _ = write!(out, " {key}");
            if let Some(value) = attributes.objectForKey(&key) {
                write_attribute_value(out, &value);
            }
        }
        runs += 1;
        let end = run.location.saturating_add(run.length);
        if end <= index {
            break;
        }
        index = end;
    }
}

/// Write an attribute's value: an `NSTextAlternatives` with its strings,
/// anything else by its description.
fn write_attribute_value(out: &mut String, value: &AnyObject) {
    let alternatives_class = AnyClass::get(c"NSTextAlternatives");
    if alternatives_class.is_some_and(|class| is_kind_of(value, class)) {
        // SAFETY: an `NSTextAlternatives` answers `primaryString` and
        // `alternativeStrings` with an `NSString` and an `NSArray` of them
        // (nil is tolerated).
        let (primary, alternatives): (
            Option<Retained<NSString>>,
            Option<Retained<NSArray<NSString>>>,
        ) = unsafe {
            (
                msg_send![value, primaryString],
                msg_send![value, alternativeStrings],
            )
        };
        out.push_str("(primary=");
        match primary {
            Some(primary) => write_escaped(out, &primary.to_string(), TEXT_LIMIT),
            None => out.push_str("nil"),
        }
        out.push_str(" alternatives=[");
        for (n, alternative) in alternatives
            .iter()
            .flat_map(|array| array.iter())
            .enumerate()
        {
            if n > 0 {
                out.push_str(", ");
            }
            write_escaped(out, &alternative.to_string(), TEXT_LIMIT);
        }
        out.push_str("])");
    } else {
        // SAFETY: every object answers `description` with an `NSString`
        // (nil is tolerated).
        let description: Option<Retained<NSString>> = unsafe { msg_send![value, description] };
        let _ = write!(out, "<{}>=", class_name(value));
        match description {
            Some(description) => write_escaped(out, &description.to_string(), VALUE_LIMIT),
            None => out.push_str("nil"),
        }
    }
}

/// Write the strings of an `NSArray` result.
fn write_string_array(out: &mut String, array: *mut AnyObject) {
    // SAFETY: a live object or nil, alive for the call.
    let Some(object) = (unsafe { array.as_ref() }) else {
        out.push_str("nil");
        return;
    };
    let Some(array) = object.downcast_ref::<NSArray>() else {
        out.push_str(&class_name(object));
        return;
    };
    out.push('[');
    for (n, item) in array.iter().enumerate() {
        if n > 0 {
            out.push_str(", ");
        }
        match item.downcast_ref::<NSString>() {
            Some(text) => {
                let _ = write!(out, "{text}");
            }
            None => out.push_str(&class_name(&item)),
        }
    }
    out.push(']');
}

/// Write a menu action's sender: its class, and a menu item's title or a
/// notification's name.
fn write_sender(out: &mut String, sender: *mut AnyObject) {
    // SAFETY: a live object or nil, alive for the call.
    let Some(sender) = (unsafe { sender.as_ref() }) else {
        out.push_str("nil");
        return;
    };
    out.push_str(&class_name(sender));
    let titled = AnyClass::get(c"NSMenuItem").is_some_and(|class| is_kind_of(sender, class));
    let named = AnyClass::get(c"NSNotification").is_some_and(|class| is_kind_of(sender, class));
    let label: Option<Retained<NSString>> = if titled {
        // SAFETY: an `NSMenuItem` answers `title` with an `NSString`.
        unsafe { msg_send![sender, title] }
    } else if named {
        // SAFETY: an `NSNotification` answers `name` with an `NSString`.
        unsafe { msg_send![sender, name] }
    } else {
        None
    };
    if let Some(label) = label {
        out.push(' ');
        write_escaped(out, &label.to_string(), TEXT_LIMIT);
    }
}

/// Write a key event's characters, key code, repeat flag and modifiers.
fn write_key_event(out: &mut String, event: *mut AnyObject) {
    // SAFETY: AppKit passes a live event or nil, alive for the call.
    let Some(object) = (unsafe { event.as_ref() }) else {
        out.push_str(" event=nil");
        return;
    };
    let Some(event) = object.downcast_ref::<NSEvent>() else {
        let _ = write!(out, " event={}", class_name(object));
        return;
    };
    let kind = event.r#type();
    if kind != NSEventType::KeyDown && kind != NSEventType::KeyUp {
        // A key event's accessors raise on any other type.
        let _ = write!(out, " event={}", EventType(kind));
        return;
    }
    out.push_str(" chars=");
    match event.characters() {
        Some(text) => write_escaped(out, &text.to_string(), TEXT_LIMIT),
        None => out.push_str("nil"),
    }
    out.push_str(" unmodified=");
    match event.charactersIgnoringModifiers() {
        Some(text) => write_escaped(out, &text.to_string(), TEXT_LIMIT),
        None => out.push_str("nil"),
    }
    let _ = write!(
        out,
        " keyCode={} repeat={} flags={:#x}",
        event.keyCode(),
        event.isARepeat(),
        event.modifierFlags().bits()
    );
}

// ---------------------------------------------------------------------------
// The replacements
// ---------------------------------------------------------------------------

/// Always installed. Counts the `keyDown:` calls running, which is how
/// [`insert_text`] tells typing from an insertion made outside a key press,
/// and ends the view's Writing Tools session and a dictation session left
/// ending: a key pressed in the view is the user typing again.
extern "C-unwind" fn key_down(this: *mut AnyObject, cmd: Sel, event: *mut AnyObject) {
    let _scope = KeyDownScope::enter(this);
    log(|line| {
        let _ = write!(line, "keyDown: {} |", Context::of(this));
        write_key_event(line, event);
    });
    let ended = with_attached(this, |view| view.writing_tools.replace(false)).unwrap_or(false);
    if ended {
        log(|line| {
            let _ = write!(
                line,
                "keyDown: the Writing Tools session ends {}",
                Context::of(this)
            );
        });
    }
    composition_over(this, "keyDown:");
    call_original!(Slot::KeyDown.index(), ObjectArgFn, (), (this, cmd, event));
    log(|line| {
        let _ = write!(line, "keyDown: end {}", Context::of(this));
    });
}

/// Always installed, as an override of `NSResponder`'s method that calls it:
/// winit hands each key press to the input method through it when IME is
/// allowed. While it runs, the input method has the key; whether it acted on
/// the view by the time it returns says whether it swallowed the key
/// ([`key_was_swallowed`]).
extern "C-unwind" fn interpret_key_events(this: *mut AnyObject, cmd: Sel, events: *mut AnyObject) {
    /// The hand-over, ended for as long as the value lives (unwinding
    /// included).
    struct Interpreting;
    impl Drop for Interpreting {
        fn drop(&mut self) {
            let _ = with_key_press(|_, press| press.end_interpreting());
        }
    }
    let _ = with_key_press(|_, press| press.begin_interpreting());
    let interpreting = Interpreting;
    log(|line| {
        let _ = write!(line, "interpretKeyEvents: {}", Context::of(this));
    });
    call_original!(
        Slot::InterpretKeyEvents.index(),
        ObjectArgFn,
        (),
        (this, cmd, events)
    );
    drop(interpreting);
    log(|line| {
        let verdict = with_key_press(|_, press| press.swallowed());
        let _ = write!(
            line,
            "interpretKeyEvents: end {} | {}",
            Context::of(this),
            match verdict {
                Some(true) => "swallowed: the input method did nothing with the key",
                Some(false) => "the input method acted",
                None => "outside a keyDown:",
            }
        );
    });
}

/// Always installed. winit's own method runs inside a `keyDown:` and on a
/// view nobody attached; anything else is text the input system inserted on
/// its own, which [`insert_out_of_band`] takes. A panic there runs winit's
/// method, which after an unmark has nothing left to commit, so no text is
/// committed twice. Inside a key press, it is the input method acting on the
/// key: the key is not swallowed.
extern "C-unwind" fn insert_text(
    this: *mut AnyObject,
    cmd: Sel,
    text: *mut AnyObject,
    replacement: NSRange,
) {
    note_input_method_acted();
    log(|line| {
        let _ = write!(
            line,
            "insertText:replacementRange: {} | text=",
            Context::of(this)
        );
        write_text(line, text);
        let _ = write!(line, " replacement={}", Range(replacement));
    });
    let taken = catch_unwind(AssertUnwindSafe(|| {
        insert_out_of_band(this, text, replacement)
    }))
    .unwrap_or(Inserted::ByWinit);
    if taken == Inserted::ByWinit {
        call_original!(
            Slot::InsertText.index(),
            InsertTextFn,
            (),
            (this, cmd, text, replacement)
        );
    } else {
        log(|line| {
            let _ = write!(line, "insertText:replacementRange: -> {taken}");
        });
    }
}

/// What became of an `insertText:replacementRange:`.
#[derive(Clone, PartialEq, Eq)]
enum Inserted {
    /// winit's own method runs: a key press is running, the view is not
    /// attached, or the text is neither an `NSString` nor an
    /// `NSAttributedString`.
    ByWinit,
    /// Dropped, as winit drops it: it starts with a control character.
    Control,
    /// Empty, and replacing nothing: nothing to post. A composition the view
    /// held is over.
    Empty,
    /// Posted to the view's window from this origin, replacing these
    /// characters of the focused document (the selection, for none).
    Posted(ImeOrigin, Option<std::ops::Range<usize>>),
}

impl fmt::Display for Inserted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ByWinit => f.write_str("winit's own"),
            Self::Control => f.write_str("dropped (a control character)"),
            Self::Empty => f.write_str("nothing to post (empty)"),
            Self::Posted(origin, None) => write!(f, "posted as {origin:?}"),
            Self::Posted(origin, Some(chars)) => write!(
                f,
                "posted as {origin:?}, replacing characters {}..{}",
                chars.start, chars.end
            ),
        }
    }
}

/// Take `text`, which the input system inserted into `view` outside a key
/// press, unless winit's own method is to run ([`Inserted::ByWinit`]).
fn insert_out_of_band(
    view: *mut AnyObject,
    text: *mut AnyObject,
    replacement: NSRange,
) -> Inserted {
    if key_down_depth() > 0 {
        if replacement.location != NOT_FOUND {
            log(|line| {
                let _ = write!(
                    line,
                    "insertText:replacementRange: in-key replacement not handled: \
                     replacement={} left to winit, which ignores it",
                    Range(replacement)
                );
            });
        }
        return Inserted::ByWinit;
    }
    let Some((window_id, poster, writing_tools, dictation)) = with_attached(view, |attached| {
        (
            attached.window_id,
            Arc::clone(&attached.poster),
            attached.writing_tools.get(),
            attached.dictation.get().is_dictating(),
        )
    }) else {
        return Inserted::ByWinit;
    };
    let Some((string, alternatives)) = read_inserted(text) else {
        return Inserted::ByWinit;
    };
    if string.chars().next().is_some_and(char::is_control) {
        return Inserted::Control;
    }
    let Some(had_marked_text) = original_has_marked_text(view) else {
        return Inserted::ByWinit;
    };
    if had_marked_text {
        if !winit_unmark_text(view) {
            // winit commits a composition itself.
            return Inserted::ByWinit;
        }
        log(|line| {
            let _ = write!(
                line,
                "insertText:replacementRange: ended the composition with winit's unmarkText {}",
                Context::of(view)
            );
        });
    }
    // A range the input system names, in the focused document's characters.
    let replacement = utf16_range(replacement);
    let replacement_chars = replacement.to_range().and_then(|_| {
        let chars = ask_document(view, |document| {
            answers::replacement_chars(document, replacement)
        });
        if chars.is_none() {
            log(|line| {
                let _ = write!(
                    line,
                    "insertText:replacementRange: replacement={} not mapped: no document \
                     answered, the text replaces the selection",
                    Range(ns_range(replacement))
                );
            });
        }
        chars
    });
    if string.is_empty()
        && replacement_chars
            .as_ref()
            .is_none_or(|range| range.is_empty())
    {
        composition_over(view, "insertText:replacementRange:");
        return Inserted::Empty;
    }
    let origin = super::classify(CommitSignals {
        had_marked_text,
        carries_text_alternatives: alternatives,
        dictation_session: dictation,
        writing_tools_session: writing_tools,
    });
    poster.post_external(Box::new(ImeCommitPayload {
        window_id_owner: window_id,
        text: string,
        origin,
        replacement: replacement_chars.clone(),
    }));
    // Classified first: the commit that ends an ending dictation is still
    // dictation.
    composition_over(view, "insertText:replacementRange:");
    Inserted::Posted(origin, replacement_chars)
}

/// The view's composition is over (committed, unmarked, emptied, or a key
/// was pressed): a dictation session left ending for it closes. `by` names
/// the call, for the trace.
fn composition_over(view: *mut AnyObject, by: &str) {
    set_dictation(view, DictationEvent::CompositionOver, by);
}

/// The text `insertText:` was handed, and whether it carries text
/// alternatives. `None` for anything but an `NSString` or an
/// `NSAttributedString` (nil included).
fn read_inserted(text: *mut AnyObject) -> Option<(String, bool)> {
    // SAFETY: AppKit passes a live object or nil, alive for the call.
    let object = unsafe { text.as_ref() }?;
    if let Some(attributed) = object.downcast_ref::<NSAttributedString>() {
        Some((
            attributed.string().to_string(),
            carries_text_alternatives(attributed),
        ))
    } else {
        object
            .downcast_ref::<NSString>()
            .map(|text| (text.to_string(), false))
    }
}

/// Whether some run of `text` carries the `NSTextAlternatives` attribute,
/// the alternatives AppKit attaches to the words a dictation recognised.
fn carries_text_alternatives(text: &NSAttributedString) -> bool {
    // SAFETY: an AppKit constant, set before any code runs.
    let key = unsafe { NSTextAlternativesAttributeName };
    let length = text.length();
    let mut index = 0;
    while index < length {
        let mut run = NSRange::new(0, 0);
        // SAFETY: `index` is inside the string, so nothing is raised, and
        // `run` is a valid out-pointer.
        let value = unsafe { text.attribute_atIndex_effectiveRange(key, index, &mut run) };
        if value.is_some() {
            return true;
        }
        let end = run.location.saturating_add(run.length);
        index = if end > index { end } else { index + 1 };
    }
    false
}

/// Always installed, for the key press it may answer (a dead key, an input
/// method composing); winit's own method runs. An empty string ends the
/// composition, and so a dictation session left ending.
extern "C-unwind" fn set_marked_text(
    this: *mut AnyObject,
    cmd: Sel,
    text: *mut AnyObject,
    selected: NSRange,
    replacement: NSRange,
) {
    note_input_method_acted();
    log(|line| {
        let _ = write!(
            line,
            "setMarkedText:selectedRange:replacementRange: {} | text=",
            Context::of(this)
        );
        write_text(line, text);
        let _ = write!(
            line,
            " selected={} replacement={}",
            Range(selected),
            Range(replacement)
        );
    });
    if replacement.location != NOT_FOUND {
        log(|line| {
            let _ = write!(
                line,
                "setMarkedText:selectedRange:replacementRange: marked replacement not handled: \
                 replacement={} left to winit, which ignores it",
                Range(replacement)
            );
        });
    }
    call_original!(
        Slot::SetMarkedText.index(),
        SetMarkedTextFn,
        (),
        (this, cmd, text, selected, replacement)
    );
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if read_inserted(text).is_some_and(|(text, _)| text.is_empty()) {
            composition_over(this, "setMarkedText:selectedRange:replacementRange:");
        }
    }));
}

/// Always installed, for the key press it may answer; winit's own method
/// runs. The composition is over, and so a dictation session left ending.
/// The hooks' own unmark calls winit's method directly and is counted
/// neither as the input method acting nor here.
extern "C-unwind" fn unmark_text(this: *mut AnyObject, cmd: Sel) {
    note_input_method_acted();
    log(|line| {
        let _ = write!(line, "unmarkText {}", Context::of(this));
    });
    call_original!(Slot::UnmarkText.index(), VoidFn, (), (this, cmd));
    let _ = catch_unwind(AssertUnwindSafe(|| composition_over(this, "unmarkText")));
}

extern "C-unwind" fn has_marked_text(this: *mut AnyObject, cmd: Sel) -> Bool {
    let marked = call_original!(Slot::HasMarkedText.index(), BoolFn, Bool::NO, (this, cmd));
    log(|line| {
        // Asking again would run winit's method twice: its answer is this.
        let known = attached_window(this).map(|_| marked.as_bool());
        let _ = write!(
            line,
            "hasMarkedText {} | -> {}",
            Context::with_marked(this, known),
            marked.as_bool()
        );
    });
    marked
}

/// Always installed. The focused document's composition while winit holds
/// marked text, none while it does not; winit's own answer with no
/// document, or one that has no composition for winit's marked text.
extern "C-unwind" fn marked_range(this: *mut AnyObject, cmd: Sel) -> NSRange {
    let from_document = guarded(
        || {
            let view_has_marked_text = original_has_marked_text(this)?;
            ask_document(this, |document| {
                answers::marked_range(document, view_has_marked_text)
            })
        },
        || None,
    );
    let (range, source) = match from_document {
        Some(range) => (ns_range(range), Source::Document),
        None => (
            call_original!(
                Slot::MarkedRange.index(),
                RangeFn,
                NSRange::new(NOT_FOUND, 0),
                (this, cmd)
            ),
            Source::Winit,
        ),
    };
    log(|line| {
        let _ = write!(
            line,
            "markedRange {} | -> {} ({source})",
            Context::of(this),
            Range(range)
        );
    });
    range
}

/// Always installed. The focused document's selection; winit's own answer
/// (none) with no document.
extern "C-unwind" fn selected_range(this: *mut AnyObject, cmd: Sel) -> NSRange {
    let from_document = guarded(|| ask_document(this, answers::selected_range), || None);
    let (range, source) = match from_document {
        Some(range) => (ns_range(range), Source::Document),
        None => (
            call_original!(
                Slot::SelectedRange.index(),
                RangeFn,
                NSRange::new(NOT_FOUND, 0),
                (this, cmd)
            ),
            Source::Winit,
        ),
    };
    log(|line| {
        let _ = write!(
            line,
            "selectedRange {} | -> {} ({source})",
            Context::of(this),
            Range(range)
        );
    });
    range
}

/// Always installed. The focused document's text in the range, as a plain
/// attributed string, with the range it actually is in `actual`; nil for a
/// secure field or no range. winit's own answer (nil) with no document.
extern "C-unwind" fn attributed_substring(
    this: *mut AnyObject,
    cmd: Sel,
    range: NSRange,
    actual: *mut NSRange,
) -> *mut AnyObject {
    let from_document = guarded(
        || {
            ask_document(this, |document| {
                answers::substring(document, utf16_range(range))
            })
        },
        || None,
    );
    let (substring, source) = match from_document {
        Some(Substring::Nil) => (ptr::null_mut(), Source::Document),
        Some(Substring::Text {
            text,
            actual: range,
        }) => {
            // SAFETY: the caller passed null or an `NSRange` it owns for the
            // length of the call.
            if let Some(actual) = unsafe { actual.as_mut() } {
                *actual = ns_range(range);
            }
            let text = NSAttributedString::from_nsstring(&NSString::from_str(&text));
            // Autoreleased, as the method's family (none) returns it.
            let text: *mut NSAttributedString = Retained::autorelease_return(text);
            (text.cast::<AnyObject>(), Source::Document)
        }
        None => (
            call_original!(
                Slot::AttributedSubstring.index(),
                SubstringFn,
                ptr::null_mut(),
                (this, cmd, range, actual)
            ),
            Source::Winit,
        ),
    };
    log(|line| {
        let _ = write!(
            line,
            "attributedSubstringForProposedRange:actualRange: {} | range={} actual={} -> ",
            Context::of(this),
            Range(range),
            OutRange(actual)
        );
        write_text(line, substring);
        let _ = write!(line, " ({source})");
    });
    substring
}

/// Always installed. The UTF-16 offset of the focused document's character
/// under the screen point, `NSNotFound` where it has none; winit's own
/// answer (0) with no document.
extern "C-unwind" fn character_index_for_point(
    this: *mut AnyObject,
    cmd: Sel,
    point: NSPoint,
) -> usize {
    let from_document = guarded(
        || {
            let window_point = screen_point_to_window(this, point)?;
            ask_document(this, |document| {
                answers::character_index(document, window_point)
            })
        },
        || None,
    );
    let (index, source) = match from_document {
        Some(index) => (index, Source::Document),
        None => (
            call_original!(
                Slot::CharacterIndex.index(),
                IndexForPointFn,
                NOT_FOUND,
                (this, cmd, point)
            ),
            Source::Winit,
        ),
    };
    log(|line| {
        let _ = write!(
            line,
            "characterIndexForPoint: {} | point={} -> {} ({source})",
            Context::of(this),
            Point(point),
            Index(index)
        );
    });
    index
}

/// Always installed. The screen rectangle of the first line of the range in
/// the focused document (of the selection, for no range), with the range
/// that line holds in `actual`; winit's own answer (its IME cursor area)
/// with no document.
extern "C-unwind" fn first_rect(
    this: *mut AnyObject,
    cmd: Sel,
    range: NSRange,
    actual: *mut NSRange,
) -> NSRect {
    let from_document = guarded(
        || {
            let (rect, covered) = ask_document(this, |document| {
                answers::first_rect(document, utf16_range(range))
            })?;
            Some((window_rect_to_screen(this, rect)?, covered))
        },
        || None,
    );
    let (rect, source) = match from_document {
        Some((rect, covered)) => {
            // SAFETY: the caller passed null or an `NSRange` it owns for the
            // length of the call.
            if let Some(actual) = unsafe { actual.as_mut() } {
                *actual = ns_range(covered);
            }
            (rect, Source::Document)
        }
        None => (
            call_original!(
                Slot::FirstRect.index(),
                FirstRectFn,
                NSRect::ZERO,
                (this, cmd, range, actual)
            ),
            Source::Winit,
        ),
    };
    log(|line| {
        let _ = write!(
            line,
            "firstRectForCharacterRange:actualRange: {} | range={} actual={} -> {} ({source})",
            Context::of(this),
            Range(range),
            OutRange(actual),
            Rect(rect)
        );
    });
    rect
}

extern "C-unwind" fn valid_attributes(this: *mut AnyObject, cmd: Sel) -> *mut AnyObject {
    let attributes = call_original!(
        Slot::ValidAttributes.index(),
        ObjectFn,
        ptr::null_mut(),
        (this, cmd)
    );
    log(|line| {
        let _ = write!(
            line,
            "validAttributesForMarkedText {} | -> ",
            Context::of(this)
        );
        write_string_array(line, attributes);
    });
    attributes
}

/// Always installed, for the key press it answers (an arrow, Escape, a
/// command key with no menu item); winit's own method runs.
extern "C-unwind" fn do_command_by_selector(this: *mut AnyObject, cmd: Sel, command: Option<Sel>) {
    note_input_method_acted();
    log(|line| {
        let _ = write!(
            line,
            "doCommandBySelector: {} | command=",
            Context::of(this)
        );
        match command {
            Some(command) => {
                let _ = write!(line, "{command}");
            }
            None => line.push_str("nil"),
        }
    });
    call_original!(
        Slot::DoCommand.index(),
        SelectorArgFn,
        (),
        (this, cmd, command)
    );
}

/// Added. The visible text of the focused document, on screen; an empty
/// rectangle with no document.
extern "C-unwind" fn document_visible_rect(this: *mut AnyObject, _cmd: Sel) -> NSRect {
    let from_document = guarded(
        || window_rect_to_screen(this, ask_document(this, answers::visible_rect)?),
        || None,
    );
    let (rect, source) = match from_document {
        Some(rect) => (rect, Source::Document),
        None => (NSRect::ZERO, Source::NoDocument),
    };
    log(|line| {
        let _ = write!(
            line,
            "documentVisibleRect {} | -> {} ({source})",
            Context::of(this),
            Rect(rect)
        );
    });
    rect
}

/// Added. The visible part of the focused document's selection, on screen
/// (its caret, for none); an empty rectangle with no document or nothing
/// visible.
extern "C-unwind" fn union_rect_in_visible_selected_range(
    this: *mut AnyObject,
    _cmd: Sel,
) -> NSRect {
    let from_document = guarded(
        || {
            let rect = ask_document(this, answers::union_rect_in_visible_selection)?;
            window_rect_to_screen(this, rect)
        },
        || None,
    );
    let (rect, source) = match from_document {
        Some(rect) => (rect, Source::Document),
        None => (NSRect::ZERO, Source::NoDocument),
    };
    log(|line| {
        let _ = write!(
            line,
            "unionRectInVisibleSelectedRange {} | -> {} ({source})",
            Context::of(this),
            Rect(rect)
        );
    });
    rect
}

/// Added. Whether the focused widget takes text; no with no document.
extern "C-unwind" fn is_editable(this: *mut AnyObject, _cmd: Sel) -> Bool {
    let (editable, source) = guarded(
        || match ask_document(this, |document| Some(answers::is_editable(Some(document)))) {
            Some(editable) => (editable, Source::Document),
            None => (answers::is_editable(None), Source::NoDocument),
        },
        || (answers::is_editable(None), Source::NoDocument),
    );
    log(|line| {
        let _ = write!(
            line,
            "isEditable {} | -> {editable} ({source})",
            Context::of(this)
        );
    });
    Bool::new(editable)
}

/// Added. Writing Tools in its panel (`NSWritingToolsBehaviorLimited`) where
/// the focused widget allows it, not at all (`NSWritingToolsBehaviorNone`)
/// otherwise and with no document.
extern "C-unwind" fn writing_tools_behavior(this: *mut AnyObject, _cmd: Sel) -> isize {
    let (behavior, source) = guarded(
        || match ask_document(this, |document| {
            Some(answers::writing_tools_behavior(Some(document)))
        }) {
            Some(behavior) => (behavior, Source::Document),
            None => (answers::writing_tools_behavior(None), Source::NoDocument),
        },
        || (answers::writing_tools_behavior(None), Source::NoDocument),
    );
    log(|line| {
        let _ = write!(
            line,
            "writingToolsBehavior {} | -> {behavior} ({source})",
            Context::of(this)
        );
    });
    behavior
}

/// Added. No: Teksilo draws no Writing Tools affordance beside the text.
extern "C-unwind" fn allows_writing_tools_affordance(this: *mut AnyObject, _cmd: Sel) -> Bool {
    log(|line| {
        let _ = write!(
            line,
            "allowsWritingToolsAffordance {} | -> false",
            Context::of(this)
        );
    });
    Bool::NO
}

/// Added. Plain text (`NSWritingToolsResultPlainText`): a result's
/// formatting is not applied.
extern "C-unwind" fn allowed_writing_tools_result_options(
    this: *mut AnyObject,
    _cmd: Sel,
) -> usize {
    log(|line| {
        let _ = write!(
            line,
            "allowedWritingToolsResultOptions {} | -> {} (plain text)",
            Context::of(this),
            answers::WRITING_TOOLS_PLAIN_TEXT
        );
    });
    answers::WRITING_TOOLS_PLAIN_TEXT
}

/// Traced once per selector: AppKit asks this constantly, and the point is
/// to learn which questions the text input system asks.
///
/// The only line that does not ask winit's `hasMarkedText` (`marked=?`):
/// this one is also sent while a view is created and torn down, when
/// winit's state is not there to read.
extern "C-unwind" fn responds_to_selector(
    this: *mut AnyObject,
    cmd: Sel,
    selector: Option<Sel>,
) -> Bool {
    let answer = call_original!(
        Slot::RespondsToSelector.index(),
        RespondsFn,
        Bool::NO,
        (this, cmd, selector)
    );
    if let Some(selector) = selector
        && super::trace_enabled()
        && first_probe(selector)
    {
        log(|line| {
            let _ = write!(
                line,
                "respondsToSelector: {} | selector={selector} -> {} (first time asked)",
                Context::with_marked(this, None),
                answer.as_bool()
            );
        });
    }
    answer
}

/// Whether `selector` is asked about for the first time on this thread.
fn first_probe(selector: Sel) -> bool {
    PROBED
        .try_with(|probed| {
            probed
                .try_borrow_mut()
                .is_ok_and(|mut probed| probed.insert(selector))
        })
        .unwrap_or(false)
}

/// A menu action sent to the view.
extern "C-unwind" fn view_action<const SLOT: usize>(
    this: *mut AnyObject,
    cmd: Sel,
    sender: *mut AnyObject,
) {
    log(|line| {
        let _ = write!(line, "{cmd} (view) {} | sender=", Context::of(this));
        write_sender(line, sender);
    });
    call_original!(SLOT, ObjectArgFn, (), (this, cmd, sender));
}

/// The key window's first responder, on the main thread.
fn key_first_responder() -> Option<Retained<NSResponder>> {
    let mtm = MainThreadMarker::new()?;
    NSApplication::sharedApplication(mtm)
        .keyWindow()
        .and_then(|window| window.firstResponder())
}

/// A raw pointer to a responder, for [`Context`] and the attached views.
fn responder_ptr(responder: Option<&NSResponder>) -> *mut AnyObject {
    object_ptr(responder.map(|responder| {
        let responder: &AnyObject = responder.as_ref();
        responder
    }))
}

/// Trace a menu action sent to `NSApp`; the context is the key window's
/// first responder.
fn trace_app_action(cmd: Sel, sender: *mut AnyObject) {
    log(|line| {
        let responder = key_first_responder();
        let _ = write!(
            line,
            "{cmd} (NSApplication, first responder) {} | sender=",
            Context::of(responder_ptr(responder.as_deref()))
        );
        write_sender(line, sender);
    });
}

/// A menu action sent to `NSApp`, traced.
extern "C-unwind" fn app_action<const SLOT: usize>(
    this: *mut AnyObject,
    cmd: Sel,
    sender: *mut AnyObject,
) {
    trace_app_action(cmd, sender);
    call_original!(SLOT, ObjectArgFn, (), (this, cmd, sender));
}

/// Always installed. Writing Tools opens on the key window's first
/// responder: if that is an attached view, what it inserts from now until
/// the next key press in it is [`ImeOrigin::WritingTools`]. Set before
/// AppKit's method runs, in case it inserts at once.
extern "C-unwind" fn app_show_writing_tools(
    this: *mut AnyObject,
    cmd: Sel,
    sender: *mut AnyObject,
) {
    trace_app_action(cmd, sender);
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let responder = key_first_responder();
        let view = responder_ptr(responder.as_deref());
        let began = with_attached(view, |attached| attached.writing_tools.set(true)).is_some();
        if began {
            log(|line| {
                let _ = write!(
                    line,
                    "showWritingTools: a Writing Tools session begins {}",
                    Context::of(view)
                );
            });
        }
    }));
    call_original!(
        Slot::AppShowWritingTools.index(),
        ObjectArgFn,
        (),
        (this, cmd, sender)
    );
}

/// Always installed. The Edit menu asks dictation to start on the key
/// window's first responder. Where the private `_dictationDidStart:` hook is
/// installed, that is left to it: on macOS 27 this fired while dictation
/// never began. Where it is not, this is the only start there is, and an
/// attached view is dictating from now (until `_dictationDidEnd:`,
/// `stopDictation:` or the window closing), set before AppKit's method runs;
/// a request that comes to nothing then leaves the view dictating until one
/// of those.
extern "C-unwind" fn app_start_dictation(this: *mut AnyObject, cmd: Sel, sender: *mut AnyObject) {
    trace_app_action(cmd, sender);
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let starts_reported = DICTATION_STARTS_REPORTED.load(Ordering::Relaxed);
        let responder = key_first_responder();
        let view = responder_ptr(responder.as_deref());
        set_dictation(
            view,
            DictationEvent::StartRequested { starts_reported },
            "startDictation:",
        );
        if starts_reported {
            log(|line| {
                let _ = write!(
                    line,
                    "startDictation: the session waits for _dictationDidStart: {}",
                    Context::of(view)
                );
            });
        }
    }));
    call_original!(
        Slot::AppStartDictation.index(),
        ObjectArgFn,
        (),
        (this, cmd, sender)
    );
}

/// Always installed. Dictation stops for the application: every attached
/// view's session closes, or is left ending while winit holds marked text
/// on it (the last words, still to be committed).
extern "C-unwind" fn app_stop_dictation(this: *mut AnyObject, cmd: Sel, sender: *mut AnyObject) {
    trace_app_action(cmd, sender);
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let views: Vec<usize> = VIEWS
            .try_with(|views| {
                views
                    .try_borrow()
                    .map(|views| views.keys().copied().collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        for view in views {
            let view = view as *mut AnyObject;
            let marked = original_has_marked_text(view).unwrap_or(false);
            set_dictation(
                view,
                DictationEvent::StopRequested { marked },
                "stopDictation:",
            );
        }
    }));
    call_original!(
        Slot::AppStopDictation.index(),
        ObjectArgFn,
        (),
        (this, cmd, sender)
    );
}

/// Always installed where `NSTextInputContext` has this private method
/// (macOS 27 does): dictation has started on the context's client, which is
/// dictating from now.
extern "C-unwind" fn context_dictation_did_start(
    this: *mut AnyObject,
    cmd: Sel,
    argument: *mut AnyObject,
) {
    trace_context_action(this, cmd, argument);
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let client = input_context_client(this);
        set_dictation(
            object_ptr(client.as_deref()),
            DictationEvent::Started,
            "_dictationDidStart:",
        );
    }));
    call_original!(
        Slot::ContextDictationDidStart.index(),
        ObjectArgFn,
        (),
        (this, cmd, argument)
    );
}

/// Always installed where `NSTextInputContext` has this private method:
/// dictation has ended on the context's client. On macOS 27 it comes after
/// the dictation's last commit when dictation ends on silence, but about
/// 50 ms before it when the user stops dictation, with the last sentence
/// still marked: then the session is left ending until that composition is
/// over, so the sentence is still committed as dictation.
extern "C-unwind" fn context_dictation_did_end(
    this: *mut AnyObject,
    cmd: Sel,
    argument: *mut AnyObject,
) {
    trace_context_action(this, cmd, argument);
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let view = object_ptr(input_context_client(this).as_deref());
        let marked = original_has_marked_text(view).unwrap_or(false);
        set_dictation(view, DictationEvent::Ended { marked }, "_dictationDidEnd:");
    }));
    call_original!(
        Slot::ContextDictationDidEnd.index(),
        ObjectArgFn,
        (),
        (this, cmd, argument)
    );
}

/// Move the dictation session of `view`, if it is attached, on by `event`;
/// `by` names the call, for the trace.
fn set_dictation(view: *mut AnyObject, event: DictationEvent, by: &str) {
    let change = with_attached(view, |attached| {
        let before = attached.dictation.get();
        let after = before.after(event);
        attached.dictation.set(after);
        (before, after)
    });
    if let Some((before, after)) = change
        && before != after
    {
        log(|line| {
            let _ = write!(
                line,
                "{by} the dictation session {before:?} -> {after:?} {}",
                Context::of(view)
            );
        });
    }
}

/// The view an `NSTextInputContext` serves: its client.
fn input_context_client(context: *mut AnyObject) -> Option<Retained<AnyObject>> {
    // SAFETY: `context` is a live `NSTextInputContext`, whose `client` is an
    // object or nil.
    unsafe { context.as_ref() }.and_then(|context| unsafe { msg_send![context, client] })
}

/// A raw pointer to an object, for [`Context`]; null for none.
fn object_ptr(object: Option<&AnyObject>) -> *mut AnyObject {
    object.map_or(ptr::null_mut(), |object| ptr::from_ref(object).cast_mut())
}

/// `NSTextInputContext`'s `showWritingTools:forDelegate:`; the context is
/// its client.
extern "C-unwind" fn context_show_writing_tools(
    this: *mut AnyObject,
    cmd: Sel,
    sender: *mut AnyObject,
    delegate: *mut AnyObject,
) {
    log(|line| {
        let client = input_context_client(this);
        let _ = write!(
            line,
            "{cmd} (NSTextInputContext, client) {} | sender=",
            Context::of(object_ptr(client.as_deref()))
        );
        write_sender(line, sender);
        line.push_str(" delegate=");
        write_sender(line, delegate);
    });
    call_original!(
        Slot::ContextShowWritingTools.index(),
        TwoObjectArgFn,
        (),
        (this, cmd, sender, delegate)
    );
}

/// Trace a private `NSTextInputContext` method; the context is its client.
fn trace_context_action(this: *mut AnyObject, cmd: Sel, argument: *mut AnyObject) {
    log(|line| {
        let client = input_context_client(this);
        let _ = write!(
            line,
            "{cmd} (NSTextInputContext, client) {} | argument=",
            Context::of(object_ptr(client.as_deref()))
        );
        write_sender(line, argument);
    });
}

/// A private `NSTextInputContext` method, traced.
extern "C-unwind" fn context_action<const SLOT: usize>(
    this: *mut AnyObject,
    cmd: Sel,
    argument: *mut AnyObject,
) {
    trace_context_action(this, cmd, argument);
    call_original!(SLOT, ObjectArgFn, (), (this, cmd, argument));
}

#[cfg(test)]
mod tests {
    //! The install mechanism, on classes of the test's own: no window, no
    //! `WinitView`. Each test owns its classes and its slot, since the slots
    //! are process-wide.

    use objc2::runtime::NSObject;
    use objc2::{ClassType, define_class};

    use super::*;

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; no `Drop`.
        #[unsafe(super(NSObject))]
        #[name = "TeksiloTextInputHookTestBase"]
        struct TestBase;

        impl TestBase {
            #[unsafe(method(hasMarkedText))]
            fn has_marked_text(&self) -> bool {
                true
            }

            /// `unmarkText` with a return value the hook does not expect.
            #[unsafe(method(unmarkText))]
            fn unmark_text(&self) -> bool {
                true
            }
        }
    );

    define_class!(
        // SAFETY: as above.
        #[unsafe(super(TestBase, NSObject))]
        #[name = "TeksiloTextInputHookTestSub"]
        struct TestSub;
    );

    fn implementation(class: &AnyClass, selector: Sel) -> usize {
        class
            .instance_method(selector)
            .map_or(0, |method| method.implementation() as usize)
    }

    #[test]
    fn a_declared_method_is_replaced_and_still_answers_through_the_original() {
        let class = TestBase::class();
        let hook = hook!(
            WhenTracing,
            c"hasMarkedText",
            Slot::HasMarkedText,
            has_marked_text,
            BoolFn
        );
        let before = implementation(class, sel!(hasMarkedText));

        install_hook(class, &hook);

        assert_eq!(
            implementation(class, sel!(hasMarkedText)),
            hook.replacement as usize
        );
        assert_eq!(
            ORIGINALS[Slot::HasMarkedText.index()]
                .get()
                .map(|imp| *imp as usize),
            Some(before)
        );
        // SAFETY: `new` makes an instance; `hasMarkedText` returns a BOOL.
        let object: Retained<TestBase> = unsafe { msg_send![TestBase::class(), new] };
        // SAFETY: as above.
        let marked: bool = unsafe { msg_send![&*object, hasMarkedText] };
        assert!(marked, "the replacement returns what the original answers");

        // Installing again changes nothing.
        install_hook(class, &hook);
        assert_eq!(
            implementation(class, sel!(hasMarkedText)),
            hook.replacement as usize
        );
    }

    #[test]
    fn an_inherited_method_gets_an_override_and_the_superclass_keeps_its_own() {
        let class = TestSub::class();
        let hook = hook!(
            WhenTracing,
            c"respondsToSelector:",
            Slot::RespondsToSelector,
            responds_to_selector,
            RespondsFn
        );
        let inherited = implementation(NSObject::class(), sel!(respondsToSelector:));

        install_hook(class, &hook);

        assert_eq!(
            implementation(class, sel!(respondsToSelector:)),
            hook.replacement as usize
        );
        assert_eq!(
            implementation(TestBase::class(), sel!(respondsToSelector:)),
            inherited
        );
        assert_eq!(
            implementation(NSObject::class(), sel!(respondsToSelector:)),
            inherited
        );
        // SAFETY: `new` makes an instance; `respondsToSelector:` takes a
        // selector and returns a BOOL.
        let object: Retained<TestSub> = unsafe { msg_send![TestSub::class(), new] };
        // SAFETY: as above.
        let yes: bool = unsafe { msg_send![&*object, respondsToSelector: sel!(hasMarkedText)] };
        // SAFETY: as above.
        let no: bool = unsafe { msg_send![&*object, respondsToSelector: sel!(selectedRange)] };
        assert!(yes && !no, "the replacement answers as NSObject does");

        install_hook(class, &hook);
        assert_eq!(
            implementation(class, sel!(respondsToSelector:)),
            hook.replacement as usize
        );
    }

    #[test]
    fn a_method_with_another_signature_or_none_is_left_alone() {
        let class = TestBase::class();
        let before = implementation(class, sel!(unmarkText));

        install_hook(
            class,
            &hook!(
                WhenTracing,
                c"unmarkText",
                Slot::UnmarkText,
                unmark_text,
                VoidFn
            ),
        );
        install_hook(
            class,
            &hook!(
                WhenTracing,
                c"markedRange",
                Slot::MarkedRange,
                marked_range,
                RangeFn
            ),
        );

        assert_eq!(implementation(class, sel!(unmarkText)), before);
        assert!(ORIGINALS[Slot::UnmarkText.index()].get().is_none());
        assert!(class.instance_method(sel!(markedRange)).is_none());
        assert!(ORIGINALS[Slot::MarkedRange.index()].get().is_none());
    }
}

/// End the window's composition without committing it again. Not done yet:
/// `false` makes the caller reset the input method through winit.
pub(super) fn finish_composition(_window_id: TeksiloWindowId) -> bool {
    false
}

/// The replacement range of the latest `setMarkedText:`, taken once. Not
/// recorded yet.
pub(super) fn take_marked_replacement(
    _window_id: TeksiloWindowId,
) -> Option<std::ops::Range<usize>> {
    None
}
