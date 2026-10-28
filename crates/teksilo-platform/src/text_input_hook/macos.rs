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
//! The hook replaces methods of the `WinitView` class at run time and keeps
//! each original implementation, which the replacement calls. A method the
//! class declares itself is replaced in place (`method_setImplementation`);
//! one it inherits gets an override on the class (`class_addMethod`), since
//! replacing the inherited implementation would change it for every class
//! that shares it. A method is replaced only once its type encoding has
//! been checked against the replacement's signature; one that is missing,
//! or differs, is left alone and the trace says so. AccessKit moves each
//! view to a runtime subclass of `WinitView` that overrides accessibility
//! selectors only, so those views reach the replacements through
//! inheritance. The Edit menu's actions (`showWritingTools:`,
//! `startDictation:`, `stopDictation:`, `orderFrontCharacterPalette:`) are
//! hooked the same way on `NSApplication` (and all but `stopDictation:` on
//! the view, should it ever inherit them), and `NSTextInputContext`'s
//! private dictation and Writing Tools methods where the class has them.
//!
//! The classes are patched once per process, at the first attach with
//! tracing on; patching again finds its own implementations in place and
//! changes nothing. Views are registered per window either way.
//!
//! For now every replacement only traces and calls the original, so the
//! application behaves exactly as without it: this is the measurement of
//! what each feature of the text input system calls on the view.
//!
//! ```text
//! TEKSILO_TEXT_INPUT_TRACE=1 cargo run -p rich-text-editor
//! ```
//!
//! Each line names the selector, the view and its window, the depth of
//! `keyDown:` calls it runs inside, whether the view holds marked text (as
//! winit's own `hasMarkedText` answers, `?` where it cannot be asked), the
//! type of `NSApp.currentEvent`, and then, after a `|`, the arguments and
//! the result. A line repeated as is is written once, then counted.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ffi::CStr;
use std::fmt::{self, Write as _};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::{
    AnyClass, AnyObject, Bool, Imp, Method, MethodImplementation, Sel, VerificationError,
};
use objc2::{MainThreadMarker, msg_send, sel};
use objc2_app_kit::{NSApplication, NSEvent, NSEventType, NSResponder};
use objc2_foundation::{NSArray, NSAttributedString, NSPoint, NSRange, NSRect, NSString};
use raw_window_handle::RawWindowHandle;
use teksilo_core::AppEventPoster;
use teksilo_core::raw_handle::ParentHandle;
use teksilo_core::window::TeksiloWindowId;

// ---------------------------------------------------------------------------
// The attached views
// ---------------------------------------------------------------------------

/// A view attached to a window.
struct AttachedView {
    window_id: TeksiloWindowId,
    /// Where the text the view is sent will be posted.
    #[expect(
        dead_code,
        reason = "read once the hooks deliver text instead of tracing it"
    )]
    poster: Arc<dyn AppEventPoster>,
}

thread_local! {
    /// The attached views, by `NSView` address. Main thread only, where
    /// AppKit creates views and calls their text input methods.
    static VIEWS: RefCell<HashMap<usize, AttachedView>> = RefCell::new(HashMap::new());

    /// How many `keyDown:` calls are running, innermost included.
    static KEY_DOWN_DEPTH: Cell<u32> = const { Cell::new(0) };

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
) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = parent.raw_window_handle() else {
        return;
    };
    let view = handle.ns_view.as_ptr() as usize;
    VIEWS.with_borrow_mut(|views| {
        views.insert(view, AttachedView { window_id, poster });
    });
    if super::trace_enabled() {
        install(mtm);
    }
}

pub(super) fn detach(window_id: TeksiloWindowId) {
    let _ = VIEWS.try_with(|views| {
        if let Ok(mut views) = views.try_borrow_mut() {
            views.retain(|_, view| view.window_id != window_id);
        }
    });
}

/// The window `view` is attached to, if it is.
fn attached_window(view: *const AnyObject) -> Option<TeksiloWindowId> {
    VIEWS
        .try_with(|views| {
            let views = views.try_borrow().ok()?;
            views.get(&(view as usize)).map(|view| view.window_id)
        })
        .ok()
        .flatten()
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

/// A method to replace.
struct Hook {
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
/// type `$fn_type`, whose original goes to `$slot`.
macro_rules! hook {
    ($selector:expr, $slot:expr, $replacement:expr, $fn_type:ty) => {{
        let replacement: $fn_type = $replacement;
        Hook {
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

/// A [`Hook`] on a menu action, traced by `$wrapper`.
macro_rules! action_hook {
    ($selector:expr, $slot:ident, $wrapper:ident) => {
        hook!(
            $selector,
            Slot::$slot,
            $wrapper::<{ Slot::$slot.index() }>,
            ObjectArgFn
        )
    };
}

fn view_hooks() -> [Hook; 16] {
    [
        hook!(c"keyDown:", Slot::KeyDown, key_down, ObjectArgFn),
        hook!(
            c"insertText:replacementRange:",
            Slot::InsertText,
            insert_text,
            InsertTextFn
        ),
        hook!(
            c"setMarkedText:selectedRange:replacementRange:",
            Slot::SetMarkedText,
            set_marked_text,
            SetMarkedTextFn
        ),
        hook!(c"unmarkText", Slot::UnmarkText, unmark_text, VoidFn),
        hook!(
            c"hasMarkedText",
            Slot::HasMarkedText,
            has_marked_text,
            BoolFn
        ),
        hook!(c"markedRange", Slot::MarkedRange, marked_range, RangeFn),
        hook!(
            c"selectedRange",
            Slot::SelectedRange,
            selected_range,
            RangeFn
        ),
        hook!(
            c"attributedSubstringForProposedRange:actualRange:",
            Slot::AttributedSubstring,
            attributed_substring,
            SubstringFn
        ),
        hook!(
            c"characterIndexForPoint:",
            Slot::CharacterIndex,
            character_index_for_point,
            IndexForPointFn
        ),
        hook!(
            c"firstRectForCharacterRange:actualRange:",
            Slot::FirstRect,
            first_rect,
            FirstRectFn
        ),
        hook!(
            c"validAttributesForMarkedText",
            Slot::ValidAttributes,
            valid_attributes,
            ObjectFn
        ),
        hook!(
            c"doCommandBySelector:",
            Slot::DoCommand,
            do_command_by_selector,
            SelectorArgFn
        ),
        hook!(
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

fn app_hooks() -> [Hook; 4] {
    [
        action_hook!(c"showWritingTools:", AppShowWritingTools, app_action),
        action_hook!(c"startDictation:", AppStartDictation, app_action),
        action_hook!(
            c"orderFrontCharacterPalette:",
            AppCharacterPalette,
            app_action
        ),
        action_hook!(c"stopDictation:", AppStopDictation, app_action),
    ]
}

/// Private methods, found on macOS 27; each is skipped where it is missing.
fn context_hooks() -> [Hook; 4] {
    [
        action_hook!(
            c"_dictationDidStart:",
            ContextDictationDidStart,
            context_action
        ),
        action_hook!(c"_dictationDidEnd:", ContextDictationDidEnd, context_action),
        action_hook!(c"_stopDictation:", ContextStopDictation, context_action),
        hook!(
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
    for hook in view_hooks() {
        install_hook(view_class, &hook);
    }
    match AnyClass::get(c"NSApplication") {
        Some(class) => app_hooks()
            .iter()
            .for_each(|hook| install_hook(class, hook)),
        None => trace("install: no NSApplication class"),
    }
    match AnyClass::get(c"NSTextInputContext") {
        Some(class) => context_hooks()
            .iter()
            .for_each(|hook| install_hook(class, hook)),
        None => trace("install: no NSTextInputContext class"),
    }
    trace_related_methods();
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
/// included).
struct KeyDownScope;

impl KeyDownScope {
    fn enter() -> Self {
        let _ = KEY_DOWN_DEPTH.try_with(|depth| depth.set(depth.get().saturating_add(1)));
        Self
    }
}

impl Drop for KeyDownScope {
    fn drop(&mut self) {
        let _ = KEY_DOWN_DEPTH.try_with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

fn key_down_depth() -> u32 {
    KEY_DOWN_DEPTH.try_with(Cell::get).unwrap_or(0)
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
    let original = *ORIGINALS[Slot::HasMarkedText.index()].get()?;
    // SAFETY: the slot holds `WinitView`'s own `hasMarkedText`, whose
    // signature `install_hook` checked against `BoolFn`; `view` is a
    // `WinitView`.
    let marked = unsafe { std::mem::transmute::<Imp, BoolFn>(original)(view, sel!(hasMarkedText)) };
    Some(marked.as_bool())
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

/// `NSNotFound`, an `NSInteger`'s largest value.
const NOT_FOUND: usize = isize::MAX as usize;

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

extern "C-unwind" fn key_down(this: *mut AnyObject, cmd: Sel, event: *mut AnyObject) {
    let _scope = KeyDownScope::enter();
    log(|line| {
        let _ = write!(line, "keyDown: {} |", Context::of(this));
        write_key_event(line, event);
    });
    call_original!(Slot::KeyDown.index(), ObjectArgFn, (), (this, cmd, event));
    log(|line| {
        let _ = write!(line, "keyDown: end {}", Context::of(this));
    });
}

extern "C-unwind" fn insert_text(
    this: *mut AnyObject,
    cmd: Sel,
    text: *mut AnyObject,
    replacement: NSRange,
) {
    log(|line| {
        let _ = write!(
            line,
            "insertText:replacementRange: {} | text=",
            Context::of(this)
        );
        write_text(line, text);
        let _ = write!(line, " replacement={}", Range(replacement));
    });
    call_original!(
        Slot::InsertText.index(),
        InsertTextFn,
        (),
        (this, cmd, text, replacement)
    );
}

extern "C-unwind" fn set_marked_text(
    this: *mut AnyObject,
    cmd: Sel,
    text: *mut AnyObject,
    selected: NSRange,
    replacement: NSRange,
) {
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
    call_original!(
        Slot::SetMarkedText.index(),
        SetMarkedTextFn,
        (),
        (this, cmd, text, selected, replacement)
    );
}

extern "C-unwind" fn unmark_text(this: *mut AnyObject, cmd: Sel) {
    log(|line| {
        let _ = write!(line, "unmarkText {}", Context::of(this));
    });
    call_original!(Slot::UnmarkText.index(), VoidFn, (), (this, cmd));
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

extern "C-unwind" fn marked_range(this: *mut AnyObject, cmd: Sel) -> NSRange {
    let range = call_original!(
        Slot::MarkedRange.index(),
        RangeFn,
        NSRange::new(NOT_FOUND, 0),
        (this, cmd)
    );
    log(|line| {
        let _ = write!(
            line,
            "markedRange {} | -> {}",
            Context::of(this),
            Range(range)
        );
    });
    range
}

extern "C-unwind" fn selected_range(this: *mut AnyObject, cmd: Sel) -> NSRange {
    let range = call_original!(
        Slot::SelectedRange.index(),
        RangeFn,
        NSRange::new(NOT_FOUND, 0),
        (this, cmd)
    );
    log(|line| {
        let _ = write!(
            line,
            "selectedRange {} | -> {}",
            Context::of(this),
            Range(range)
        );
    });
    range
}

extern "C-unwind" fn attributed_substring(
    this: *mut AnyObject,
    cmd: Sel,
    range: NSRange,
    actual: *mut NSRange,
) -> *mut AnyObject {
    let substring = call_original!(
        Slot::AttributedSubstring.index(),
        SubstringFn,
        ptr::null_mut(),
        (this, cmd, range, actual)
    );
    log(|line| {
        let _ = write!(
            line,
            "attributedSubstringForProposedRange:actualRange: {} | range={} actual={} -> ",
            Context::of(this),
            Range(range),
            OutRange(actual)
        );
        write_text(line, substring);
    });
    substring
}

extern "C-unwind" fn character_index_for_point(
    this: *mut AnyObject,
    cmd: Sel,
    point: NSPoint,
) -> usize {
    let index = call_original!(
        Slot::CharacterIndex.index(),
        IndexForPointFn,
        NOT_FOUND,
        (this, cmd, point)
    );
    log(|line| {
        let _ = write!(
            line,
            "characterIndexForPoint: {} | point={} -> {}",
            Context::of(this),
            Point(point),
            Index(index)
        );
    });
    index
}

extern "C-unwind" fn first_rect(
    this: *mut AnyObject,
    cmd: Sel,
    range: NSRange,
    actual: *mut NSRange,
) -> NSRect {
    let rect = call_original!(
        Slot::FirstRect.index(),
        FirstRectFn,
        NSRect::ZERO,
        (this, cmd, range, actual)
    );
    log(|line| {
        let _ = write!(
            line,
            "firstRectForCharacterRange:actualRange: {} | range={} actual={} -> {}",
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

extern "C-unwind" fn do_command_by_selector(this: *mut AnyObject, cmd: Sel, command: Option<Sel>) {
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

/// A menu action sent to `NSApp`; the context is the key window's first
/// responder.
extern "C-unwind" fn app_action<const SLOT: usize>(
    this: *mut AnyObject,
    cmd: Sel,
    sender: *mut AnyObject,
) {
    log(|line| {
        let responder = MainThreadMarker::new().and_then(|mtm| {
            NSApplication::sharedApplication(mtm)
                .keyWindow()
                .and_then(|window| window.firstResponder())
        });
        let view = object_ptr(responder.as_deref().map(|responder: &NSResponder| {
            let responder: &AnyObject = responder.as_ref();
            responder
        }));
        let _ = write!(
            line,
            "{cmd} (NSApplication, first responder) {} | sender=",
            Context::of(view)
        );
        write_sender(line, sender);
    });
    call_original!(SLOT, ObjectArgFn, (), (this, cmd, sender));
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

/// A private `NSTextInputContext` method; the context is its client.
extern "C-unwind" fn context_action<const SLOT: usize>(
    this: *mut AnyObject,
    cmd: Sel,
    argument: *mut AnyObject,
) {
    log(|line| {
        let client = input_context_client(this);
        let _ = write!(
            line,
            "{cmd} (NSTextInputContext, client) {} | argument=",
            Context::of(object_ptr(client.as_deref()))
        );
        write_sender(line, argument);
    });
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
            &hook!(c"unmarkText", Slot::UnmarkText, unmark_text, VoidFn),
        );
        install_hook(
            class,
            &hook!(c"markedRange", Slot::MarkedRange, marked_range, RangeFn),
        );

        assert_eq!(implementation(class, sel!(unmarkText)), before);
        assert!(ORIGINALS[Slot::UnmarkText.index()].get().is_none());
        assert!(class.instance_method(sel!(markedRange)).is_none());
        assert!(ORIGINALS[Slot::MarkedRange.index()].get().is_none());
    }
}
