// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Windows: ending a composition, and a subclass tap that traces.
//!
//! Every attached window's HWND is kept, so that [`finish_composition`] can
//! end the window's IME composition when the focused field has already kept
//! the text it showed; see that function for what winit does around it.
//!
//! winit 0.30 reads the text input system through IMM32 alone: a commit is
//! the `GCS_RESULTSTR` of a `WM_IME_COMPOSITION`, and everything else is a
//! key press. Measured on Windows 11, voice typing (Win+H) and the emoji
//! panel (Win+.) both deliver their text to such a window as IMM results,
//! which CUAS bridges from TSF, and neither the messages nor TSF's
//! compartments tell them apart from each other or from an input method's
//! commit. Telling voice typing apart needs a TSF text store of our own
//! (`engineering/docs/tsf-text-store.md`).
//!
//! The tap is a `SetWindowSubclass` on winit's HWND, so it sees every message
//! winit sees, just before winit does. It is a tap, never a filter: **every**
//! message is passed on with `DefSubclassProc`, unconditionally, so what winit
//! receives is unchanged. It follows the two rules `pen/windows.rs` sets for
//! sharing an HWND with the title bar's, the pen's and AccessKit's subclasses:
//! a subclass id from a private range, and always chaining.
//!
//! # The tap only traces
//!
//! It is installed only when `TEKSILO_TEXT_INPUT_TRACE` is set, and then logs
//! to stderr the keyboard, character and IME messages the window receives,
//! each with where `GetCurrentInputMessageSource` says it came from and its
//! `GetMessageExtraInfo`. After winit has handled the start of a composition,
//! a composition message or the end of one, it also logs:
//!
//! - the IMM composition and result strings (not at the start);
//! - who owns the compositions in TSF's focused context;
//! - the active keyboard TIP.
//!
//! Once, when it attaches, it logs the same TSF state as a baseline, and with
//! it TSF's speech, handwriting and keyboard compartments, read at each of the
//! four levels they can be set at (global, thread, the focused document, its
//! top context) along with any other compartment listed there, and the
//! foreground window's class and the image of the process that owns it.
//!
//! Run it from PowerShell:
//!
//! ```text
//! $env:TEKSILO_TEXT_INPUT_TRACE=1; cargo run -p rich-text-editor
//! ```
//!
//! then type, compose with an IME, dictate with Win+H and pick from Win+. in
//! the editor. A line reads:
//!
//! ```text
//! [text-input] #57 Window(1) WM_KEYDOWN vk=0xe7 VK_PACKET scan=0x00 ext=0 alt=0 prev=0 up=0 rep=1 src=keyboard/injected extra=0x0
//! [text-input] #58 Window(1) WM_CHAR unit=0xd83d high-surrogate scan=0x00 ext=0 alt=0 prev=0 up=0 rep=1 src=keyboard/injected extra=0x0
//! [text-input] #61 Window(1) WM_IME_COMPOSITION last=0x006f 'o' flags=0x1bf [GCS_COMPREADSTR|...] src=unavailable/unavailable extra=0x0
//! [text-input] #61   imm: comp="hello" [0068 0065 006c 006c 006f] result=<none> cursor=5 delta=0
//! [text-input] #61   tsf: apartment=main STA, thread focus=true, top context dynamic=0x0 static=0x4 [TRANSITORY]
//! [text-input] #61   tsf: composition 0 owner={...}
//! [text-input] #61   tsf: active keyboard profile: TIP langid=0x0409 clsid={...} profile={...} hkl=0x0 flags=0x3 [ACTIVE|ENABLED]
//! ```
//!
//! and the attach baseline adds:
//!
//! ```text
//! [text-input] #3   tsf: global compartments (9 listed): SPEECH_OPENCLOSE=I4 0, SPEECH_DISABLED=empty (unlisted), ...
//! [text-input] #3   tsf: global compartments, others: SPEECH_UI_STATUS=I4 0, {...}=UNKNOWN non-null
//! [text-input] #3   fg: hwnd=0x40a1c class="CASCADIA_HOSTING_WINDOW_CLASS" pid=7312 tid=7316 this window=false this process=false image="C:\\...\\WindowsTerminal.exe"
//! ```
//!
//! The number is one sequence across every window, so the lines a probe adds
//! carry the number of the message they belong to. Lines a message prints
//! while winit is handling an earlier one come between that message's line
//! and its probe lines.
//!
//! # What it has measured
//!
//! Voice typing and the emoji panel arrive identically: one
//! `WM_IME_COMPOSITION` carrying `GCS_RESULTSTR` alone, with no composition
//! before it and none in TSF's top context, whatever profile is active. A
//! keyboard IME's commit differs: a composition comes first, the commit's
//! flags are `0x1e00`, and TSF names the composition's owner. Reading the
//! compartments and the foreground window at every composition told the
//! first two apart no better, which is why they are now read only once.
//!
//! # The probes change nothing they measure
//!
//! **Nothing that can deliver a message runs before winit has the current
//! one.** A TSF call can be a cross-process COM call, and one made from an STA
//! waits in a modal loop that delivers the messages other threads send to the
//! window meanwhile. That reordered input: a Korean IME commits a syllable and
//! at once starts the next one, and with compartments read at the commit, the
//! next syllable's `WM_IME_STARTCOMPOSITION` and `WM_IME_COMPOSITION` reached
//! winit before the commit did, which lost the committed syllable and its
//! preedit. So before `DefSubclassProc` the tap only reads what describes the
//! message being dispatched (its input source and extra info, which a later
//! call would no longer report) and writes the log line. The IMM and TSF
//! probes run after `DefSubclassProc` returns: winit has handled the message
//! by then, and its own read does not consume the strings, so the IMM probe
//! reads what winit read unless something winit's handling caused changed
//! them, in which case the change is in the log between the two. Those probes
//! touch no per-window state, since winit's handling may have closed the
//! window and freed it.
//!
//! The tap never initialises COM: winit's `OleInitialize` has already made the
//! UI thread an STA, and initialising it again with another model could only
//! fail or, on a thread that had none, fix its apartment. When COM is not
//! initialised the TSF probe says so and stops. Nor does it create a thread
//! manager: `CoCreateInstance(CLSID_TF_ThreadMgr)` returns the thread's one
//! when it exists but creates one when it does not, so the probe asks msctf for
//! the existing one with `TF_GetThreadMgr`. A message the window receives
//! while a probe runs is logged, marked, and probes nothing, so a probe never
//! recurses. Nor is one probed that winit's handling of another message sent
//! (it is marked too): probing there would deliver messages while winit is
//! still in the middle of that other one. For the same reason the attach
//! baseline is skipped when the window is created while winit is handling a
//! message of another tapped window.
//!
//! Compartments are read, never written. Reading one goes through
//! `GetCompartment`, which may create this process's handle on a compartment
//! it has not touched yet; that writes no value and notifies no sink. So that
//! the baseline shows what existed before the tap looked, each level's
//! compartments are listed (`EnumCompartments`) before any is read, and a
//! watched compartment that was not listed is marked `(unlisted)`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use raw_window_handle::RawWindowHandle;
use teksilo_core::AppEventPoster;
use teksilo_core::raw_handle::ParentHandle;
use teksilo_core::text_surface::TextSurfaces;
use teksilo_core::window::TeksiloWindowId;

use windows::Win32::Foundation::{
    CO_E_NOTINITIALIZED, CloseHandle, HWND, LPARAM, LRESULT, S_OK, WPARAM,
};
use windows::Win32::System::Com::{
    APTTYPE, APTTYPE_MAINSTA, APTTYPE_MTA, APTTYPE_NA, APTTYPE_STA, APTTYPEQUALIFIER,
    CLSCTX_INPROC_SERVER, CoCreateInstance, CoGetApartmentType,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::System::Variant::{
    VARIANT, VT_BOOL, VT_BSTR, VT_EMPTY, VT_I2, VT_I4, VT_INT, VT_UI4, VT_UINT, VT_UNKNOWN,
    VariantClear,
};
use windows::Win32::UI::Input::Ime::{
    CPS_CANCEL, CS_INSERTCHAR, CS_NOMOVECARET, GCS_COMPATTR, GCS_COMPCLAUSE, GCS_COMPREADATTR,
    GCS_COMPREADCLAUSE, GCS_COMPREADSTR, GCS_COMPSTR, GCS_CURSORPOS, GCS_DELTASTART,
    GCS_RESULTCLAUSE, GCS_RESULTREADCLAUSE, GCS_RESULTREADSTR, GCS_RESULTSTR, HIMC,
    IME_COMPOSITION_STRING, IMN_CHANGECANDIDATE, IMN_CLOSECANDIDATE, IMN_CLOSESTATUSWINDOW,
    IMN_GUIDELINE, IMN_OPENCANDIDATE, IMN_OPENSTATUSWINDOW, IMN_PRIVATE, IMN_SETCANDIDATEPOS,
    IMN_SETCOMPOSITIONFONT, IMN_SETCOMPOSITIONWINDOW, IMN_SETCONVERSIONMODE, IMN_SETOPENSTATUS,
    IMN_SETSENTENCEMODE, IMN_SETSTATUSWINDOWPOS, IMN_SOFTKBDDESTROYED, IMR_CANDIDATEWINDOW,
    IMR_COMPOSITIONFONT, IMR_COMPOSITIONWINDOW, IMR_CONFIRMRECONVERTSTRING, IMR_DOCUMENTFEED,
    IMR_QUERYCHARPOSITION, IMR_RECONVERTSTRING, ImmGetCompositionStringW, ImmGetContext,
    ImmNotifyIME, ImmReleaseContext, NI_COMPOSITIONSTR,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_PACKET, VK_PROCESSKEY};
use windows::Win32::UI::Input::{
    GetCurrentInputMessageSource, IMDT_KEYBOARD, IMDT_MOUSE, IMDT_PEN, IMDT_TOUCH, IMDT_TOUCHPAD,
    IMDT_UNAVAILABLE, IMO_HARDWARE, IMO_INJECTED, IMO_SYSTEM, IMO_UNAVAILABLE,
    INPUT_MESSAGE_SOURCE,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::TextServices::{
    CLSID_TF_InputProcessorProfiles, GUID_COMPARTMENT_CONVERSIONMODEBIAS,
    GUID_COMPARTMENT_EMPTYCONTEXT, GUID_COMPARTMENT_ENABLED_PROFILES_UPDATED,
    GUID_COMPARTMENT_HANDWRITING_OPENCLOSE, GUID_COMPARTMENT_KEYBOARD_DISABLED,
    GUID_COMPARTMENT_KEYBOARD_INPUTMODE, GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION,
    GUID_COMPARTMENT_KEYBOARD_INPUTMODE_SENTENCE, GUID_COMPARTMENT_KEYBOARD_OPENCLOSE,
    GUID_COMPARTMENT_SAPI_AUDIO, GUID_COMPARTMENT_SPEECH_CFGMENU, GUID_COMPARTMENT_SPEECH_DISABLED,
    GUID_COMPARTMENT_SPEECH_GLOBALSTATE, GUID_COMPARTMENT_SPEECH_OPENCLOSE,
    GUID_COMPARTMENT_SPEECH_UI_STATUS, GUID_COMPARTMENT_TIPUISTATUS,
    GUID_COMPARTMENT_TRANSITORYEXTENSION, GUID_COMPARTMENT_TRANSITORYEXTENSION_DOCUMENTMANAGER,
    GUID_COMPARTMENT_TRANSITORYEXTENSION_PARENT, GUID_TFCAT_TIP_KEYBOARD, ITfCompartmentMgr,
    ITfContext, ITfContextComposition, ITfInputProcessorProfileMgr, ITfThreadMgr,
    TF_INPUTPROCESSORPROFILE, TF_IPP_FLAG_ACTIVE, TF_IPP_FLAG_ENABLED,
    TF_IPP_FLAG_SUBSTITUTEDBYINPUTPROCESSOR, TF_PROFILETYPE_INPUTPROCESSOR,
    TF_PROFILETYPE_KEYBOARDLAYOUT, TS_SS_DISJOINTSEL, TS_SS_NOHIDDENTEXT, TS_SS_REGIONS,
    TS_SS_TKBAUTOCORRECTENABLE, TS_SS_TKBPREDICTIONENABLE, TS_SS_TRANSITORY, TS_SS_UWPCONTROL,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetMessageExtraInfo, GetWindowThreadProcessId,
    UNICODE_NOCHAR, WM_CHAR, WM_DEADCHAR, WM_IME_CHAR, WM_IME_COMPOSITION, WM_IME_ENDCOMPOSITION,
    WM_IME_KEYDOWN, WM_IME_KEYUP, WM_IME_NOTIFY, WM_IME_REQUEST, WM_IME_SELECT, WM_IME_SETCONTEXT,
    WM_IME_STARTCOMPOSITION, WM_INPUTLANGCHANGE, WM_KEYDOWN, WM_KEYUP, WM_NCDESTROY, WM_SYSCHAR,
    WM_SYSDEADCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_UNICHAR,
};
use windows::core::{GUID, HRESULT, Interface, PWSTR, s, w};

/// One trace line, through the module's switch.
macro_rules! tap_trace {
    ($($arg:tt)*) => {
        super::trace(format_args!($($arg)*))
    };
}

// ---------------------------------------------------------------------------
// Attaching, and installing and removing the tap
// ---------------------------------------------------------------------------

/// Subclass ids for this tap: a private range, distinct from the title bar's
/// (`0xFE_111_000`), the pen's (`0xFE_112_000`) and the quit target's
/// (`0xFE_113_000`), so no two can ever collide on one HWND.
static NEXT_SUBCLASS_ID: AtomicUsize = AtomicUsize::new(0xFE_114_000);

/// Orders every traced message, across windows.
static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// Every attached window's HWND, tracing or not: what
    /// [`finish_composition`] acts on. `attach`, `detach` and
    /// `finish_composition` run on the UI thread, which owns every HWND.
    static WINDOWS: RefCell<HashMap<TeksiloWindowId, HWND>> = RefCell::new(HashMap::new());
    /// The installed taps. `attach` and `detach` run on the UI thread, which
    /// owns every HWND they touch.
    static TAPS: RefCell<HashMap<TeksiloWindowId, Tap>> = RefCell::new(HashMap::new());
    /// How many messages winit is handling on this thread, passed on by a tap.
    static HANDLING: Cell<u32> = const { Cell::new(0) };
    /// Whether a probe is running on this thread.
    static PROBING: Cell<bool> = const { Cell::new(false) };
}

/// What the subclass procedure reads through `dwRefData`.
#[derive(Debug)]
struct TapState {
    window_id: TeksiloWindowId,
}

/// One installed subclass. Dropping it removes the subclass before the state
/// the procedure borrows goes away.
#[derive(Debug)]
struct Tap {
    hwnd: HWND,
    subclass_id: usize,
    state: Box<TapState>,
}

impl Tap {
    /// The `dwRefData` the procedure is installed with: the boxed state, which
    /// stays put however the `Tap` itself moves.
    fn ref_data(&self) -> usize {
        &*self.state as *const TapState as usize
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        // Before `state` is freed, so the procedure can never read freed
        // memory. Synchronous on this thread. Fails harmlessly when the window
        // is already gone (the procedure removed itself at `WM_NCDESTROY`).
        unsafe {
            let _ = RemoveWindowSubclass(self.hwnd, Some(text_input_proc), self.subclass_id);
        }
    }
}

pub(super) fn attach(
    window_id: TeksiloWindowId,
    parent: &ParentHandle,
    poster: Arc<dyn AppEventPoster>,
    surfaces: TextSurfaces,
) {
    // Nothing is posted or answered yet.
    let _ = (poster, surfaces);
    let RawWindowHandle::Win32(handle) = parent.raw_window_handle() else {
        return;
    };
    let hwnd = HWND(handle.hwnd.get() as *mut c_void);

    // A second attach for one window replaces the first.
    detach(window_id);
    WINDOWS.with_borrow_mut(|windows| windows.insert(window_id, hwnd));
    if super::trace_enabled() {
        install_tap(window_id, hwnd);
    }
}

/// Install the trace subclass on a window.
fn install_tap(window_id: TeksiloWindowId, hwnd: HWND) {
    let tap = Tap {
        hwnd,
        subclass_id: NEXT_SUBCLASS_ID.fetch_add(1, Ordering::Relaxed),
        state: Box::new(TapState { window_id }),
    };
    let installed =
        unsafe { SetWindowSubclass(hwnd, Some(text_input_proc), tap.subclass_id, tap.ref_data()) };
    let seq = next_sequence();
    if !installed.as_bool() {
        tap_trace!("#{seq} {window_id} SetWindowSubclass returned FALSE; not traced");
        return;
    }
    tap_trace!(
        "#{seq} {window_id} attached hwnd={:#x} subclass={:#x}",
        hwnd.0 as usize,
        tap.subclass_id
    );
    // A baseline for the compositions to come. Not while winit is handling a
    // message: a probe can deliver the messages sent to the window meanwhile,
    // and winit would see them in the middle of that one.
    if winit_is_handling() {
        tap_trace!("#{seq}   baseline skipped: winit is handling a message");
    } else {
        let _probing = Probing::enter();
        probe_tsf(seq, Reach::Baseline);
        probe_foreground(seq, hwnd);
    }
    TAPS.with_borrow_mut(|taps| taps.insert(window_id, tap));
}

pub(super) fn detach(window_id: TeksiloWindowId) {
    WINDOWS.with_borrow_mut(|windows| windows.remove(&window_id));
    // Taken out of the map first and dropped after the borrow ends, so
    // nothing `RemoveWindowSubclass` might run can find the map borrowed.
    let tap = TAPS.with_borrow_mut(|taps| taps.remove(&window_id));
    drop(tap);
}

fn next_sequence() -> u64 {
    NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// The subclass procedure
// ---------------------------------------------------------------------------

/// Logs, chains, and probes after winit has handled the message.
unsafe extern "system" fn text_input_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    uid: usize,
    dw_ref_data: usize,
) -> LRESULT {
    // Before winit has the message: only the log line, which nothing that
    // can deliver another message goes into.
    let probe_after = if dw_ref_data == 0 {
        None
    } else {
        // SAFETY: `dw_ref_data` is the boxed `TapState` of the `Tap` that
        // installed this subclass. Only code winit runs can drop that `Tap`,
        // and none has run yet, so it is alive for this part of the call.
        let state: &TapState = unsafe { &*(dw_ref_data as *const TapState) };
        // No panic may unwind into user32.
        catch_unwind(AssertUnwindSafe(|| log_message(state, msg, wparam, lparam)))
            .ok()
            .flatten()
    };
    if msg == WM_NCDESTROY {
        // The documented place to remove a subclass. `Tap::drop` then finds
        // nothing to remove, which is harmless.
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(text_input_proc), uid);
        }
    }
    let result = {
        let _handling = Handling::enter();
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    };
    // After winit has handled it: the probes. They read no `TapState`, since
    // winit's handling may have closed the window and freed it.
    if let Some(seq) = probe_after {
        let _ = catch_unwind(AssertUnwindSafe(|| probe_message(hwnd, msg, seq)));
    }
    result
}

/// How a message reached the tap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arrival {
    /// Dispatched with nothing of the tap's on the stack.
    Direct,
    /// Sent while winit was handling another message of a tapped window.
    WhileHandling,
    /// Delivered by one of the tap's probes.
    DuringProbe,
}

impl Arrival {
    /// How the message being dispatched arrived. A thread whose locals are
    /// gone counts as probing, so it probes nothing.
    fn now() -> Self {
        if PROBING.try_with(Cell::get).unwrap_or(true) {
            Self::DuringProbe
        } else if winit_is_handling() {
            Self::WhileHandling
        } else {
            Self::Direct
        }
    }

    /// What the log line adds to say so.
    fn mark(self) -> &'static str {
        match self {
            Self::Direct => "",
            Self::WhileHandling => " (sent while winit handled another message)",
            Self::DuringProbe => " (sent during a probe)",
        }
    }
}

/// Whether winit is handling a message a tap passed on, on this thread.
fn winit_is_handling() -> bool {
    HANDLING.try_with(Cell::get).unwrap_or(0) > 0
}

/// Counts a message winit is handling, for as long as it lives.
struct Handling;

impl Handling {
    fn enter() -> Self {
        let _ = HANDLING.try_with(|count| count.set(count.get() + 1));
        Self
    }
}

impl Drop for Handling {
    fn drop(&mut self) {
        let _ = HANDLING.try_with(|count| count.set(count.get().saturating_sub(1)));
    }
}

/// Marks the thread as probing for as long as it lives, restoring what it
/// found (on a panic too).
struct Probing {
    outer: bool,
}

impl Probing {
    fn enter() -> Self {
        let outer = PROBING
            .try_with(|probing| probing.replace(true))
            .unwrap_or(true);
        Self { outer }
    }
}

impl Drop for Probing {
    fn drop(&mut self) {
        let _ = PROBING.try_with(|probing| probing.set(self.outer));
    }
}

/// Log one message, before winit has it. Returns the message's sequence
/// number when it is one to probe once winit has handled it: a composition
/// message that arrived directly.
///
/// Calls nothing that can deliver a message.
fn log_message(state: &TapState, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<u64> {
    let name = message_name(msg)?;
    // Both describe the message being dispatched, and a later call would no
    // longer report them.
    let source = input_source();
    let extra = unsafe { GetMessageExtraInfo() }.0;
    let seq = next_sequence();
    let arrival = Arrival::now();
    tap_trace!(
        "#{seq} {} {name} {} src={source} extra={extra:#x}{}",
        state.window_id,
        describe(msg, wparam, lparam),
        arrival.mark()
    );
    let probed = matches!(
        msg,
        WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION | WM_IME_ENDCOMPOSITION
    );
    (probed && arrival == Arrival::Direct).then_some(seq)
}

/// Probe IMM and TSF for a composition message winit has just handled.
///
/// The IMM strings after `WM_IME_ENDCOMPOSITION` are what winit's own handling
/// of it read: while it still holds a preedit there, it commits the result
/// string again (see [`finish_composition`]).
fn probe_message(hwnd: HWND, msg: u32, seq: u64) {
    let _probing = Probing::enter();
    if matches!(msg, WM_IME_COMPOSITION | WM_IME_ENDCOMPOSITION) {
        probe_imm(hwnd, seq);
    }
    probe_tsf(seq, Reach::Message);
}

// ---------------------------------------------------------------------------
// Describing a message
// ---------------------------------------------------------------------------

/// The messages the tap logs, by name. `None` for every other message.
fn message_name(msg: u32) -> Option<&'static str> {
    Some(match msg {
        WM_KEYDOWN => "WM_KEYDOWN",
        WM_KEYUP => "WM_KEYUP",
        WM_SYSKEYDOWN => "WM_SYSKEYDOWN",
        WM_SYSKEYUP => "WM_SYSKEYUP",
        WM_CHAR => "WM_CHAR",
        WM_SYSCHAR => "WM_SYSCHAR",
        WM_DEADCHAR => "WM_DEADCHAR",
        WM_SYSDEADCHAR => "WM_SYSDEADCHAR",
        WM_UNICHAR => "WM_UNICHAR",
        WM_IME_STARTCOMPOSITION => "WM_IME_STARTCOMPOSITION",
        WM_IME_ENDCOMPOSITION => "WM_IME_ENDCOMPOSITION",
        WM_IME_COMPOSITION => "WM_IME_COMPOSITION",
        WM_IME_NOTIFY => "WM_IME_NOTIFY",
        WM_IME_SETCONTEXT => "WM_IME_SETCONTEXT",
        WM_IME_CHAR => "WM_IME_CHAR",
        WM_IME_REQUEST => "WM_IME_REQUEST",
        WM_IME_SELECT => "WM_IME_SELECT",
        WM_IME_KEYDOWN => "WM_IME_KEYDOWN",
        WM_IME_KEYUP => "WM_IME_KEYUP",
        WM_INPUTLANGCHANGE => "WM_INPUTLANGCHANGE",
        _ => return None,
    })
}

/// The fields of one logged message, decoded.
fn describe(msg: u32, wparam: WPARAM, lparam: LPARAM) -> String {
    let (w, l) = (wparam.0, lparam.0);
    match msg {
        WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP | WM_IME_KEYDOWN | WM_IME_KEYUP => {
            format!("{} {}", virtual_key(w), key_flags(l))
        }
        WM_CHAR | WM_SYSCHAR | WM_DEADCHAR | WM_SYSDEADCHAR | WM_IME_CHAR => {
            format!("{} {}", utf16_unit(w), key_flags(l))
        }
        WM_UNICHAR if w as u32 == UNICODE_NOCHAR => "probe (UNICODE_NOCHAR)".to_owned(),
        WM_UNICHAR => format!("{} {}", code_point(w), key_flags(l)),
        WM_IME_COMPOSITION => format!(
            "last={} flags={:#x} [{}]",
            utf16_unit(w),
            l as u32,
            flag_names(l as u32, COMPOSITION_FLAGS)
        ),
        WM_IME_NOTIFY => format!("{} lparam={l:#x}", named(w as u32, NOTIFY_COMMANDS)),
        WM_IME_SETCONTEXT => format!(
            "active={} show={:#x} [{}]",
            w != 0,
            l as u32,
            flag_names(l as u32, SHOW_UI_FLAGS)
        ),
        WM_IME_REQUEST => named(w as u32, REQUEST_COMMANDS),
        WM_IME_SELECT => format!("selected={} hkl={l:#x}", w != 0),
        WM_INPUTLANGCHANGE => format!("charset={w} hkl={l:#x}"),
        _ => String::new(),
    }
}

/// A virtual key, with the two that matter here named: `VK_PACKET` (a
/// Unicode unit injected as a keystroke) and `VK_PROCESSKEY` (a key an IME
/// took).
fn virtual_key(wparam: usize) -> String {
    let vk = wparam as u16;
    let tag = if vk == VK_PACKET.0 {
        " VK_PACKET"
    } else if vk == VK_PROCESSKEY.0 {
        " VK_PROCESSKEY"
    } else {
        ""
    };
    format!("vk={vk:#04x}{tag}")
}

/// The keystroke fields of a key or character message's `lParam`. Whether the
/// keystroke was injected is not among them: that is the input source's
/// `originId`.
fn key_flags(lparam: isize) -> String {
    let bits = lparam as u32;
    format!(
        "scan={:#04x} ext={} alt={} prev={} up={} rep={}",
        (bits >> 16) & 0xFF,
        (bits >> 24) & 1,
        (bits >> 29) & 1,
        (bits >> 30) & 1,
        (bits >> 31) & 1,
        bits & 0xFFFF
    )
}

/// One UTF-16 unit, marked when it is half of a surrogate pair.
fn utf16_unit(wparam: usize) -> String {
    let unit = wparam as u32;
    match unit {
        0xD800..=0xDBFF => format!("unit={unit:#06x} high-surrogate"),
        0xDC00..=0xDFFF => format!("unit={unit:#06x} low-surrogate"),
        _ => match char::from_u32(unit) {
            Some(c) => format!("unit={unit:#06x} {c:?}"),
            None => format!("unit={unit:#06x}"),
        },
    }
}

/// One UTF-32 code point, as `WM_UNICHAR` carries it.
fn code_point(wparam: usize) -> String {
    let value = wparam as u32;
    match char::from_u32(value) {
        Some(c) => format!("U+{value:04X} {c:?}"),
        None => format!("U+{value:04X} (not a scalar value)"),
    }
}

/// The names of the bits set in `bits`, with any bit the table does not name
/// left as hex.
fn flag_names(bits: u32, table: &[(u32, &str)]) -> String {
    let mut names = Vec::new();
    let mut rest = bits;
    for &(flag, name) in table {
        if flag != 0 && bits & flag == flag {
            names.push(name.to_owned());
            rest &= !flag;
        }
    }
    if rest != 0 {
        names.push(format!("{rest:#x}"));
    }
    if names.is_empty() {
        "none".to_owned()
    } else {
        names.join("|")
    }
}

/// The name of a command value, or the value in hex.
fn named(value: u32, table: &[(u32, &str)]) -> String {
    table
        .iter()
        .find(|&&(known, _)| known == value)
        .map_or_else(|| format!("{value:#x}"), |&(_, name)| name.to_owned())
}

/// `WM_IME_COMPOSITION`'s `lParam` bits.
const COMPOSITION_FLAGS: &[(u32, &str)] = &[
    (GCS_COMPREADSTR.0, "GCS_COMPREADSTR"),
    (GCS_COMPREADATTR.0, "GCS_COMPREADATTR"),
    (GCS_COMPREADCLAUSE.0, "GCS_COMPREADCLAUSE"),
    (GCS_COMPSTR.0, "GCS_COMPSTR"),
    (GCS_COMPATTR.0, "GCS_COMPATTR"),
    (GCS_COMPCLAUSE.0, "GCS_COMPCLAUSE"),
    (GCS_CURSORPOS.0, "GCS_CURSORPOS"),
    (GCS_DELTASTART.0, "GCS_DELTASTART"),
    (GCS_RESULTREADSTR.0, "GCS_RESULTREADSTR"),
    (GCS_RESULTREADCLAUSE.0, "GCS_RESULTREADCLAUSE"),
    (GCS_RESULTSTR.0, "GCS_RESULTSTR"),
    (GCS_RESULTCLAUSE.0, "GCS_RESULTCLAUSE"),
    (CS_INSERTCHAR, "CS_INSERTCHAR"),
    (CS_NOMOVECARET, "CS_NOMOVECARET"),
];

/// `WM_IME_SETCONTEXT`'s `lParam` bits (`ISC_SHOWUI*`).
const SHOW_UI_FLAGS: &[(u32, &str)] = &[
    (0x8000_0000, "COMPOSITIONWINDOW"),
    (0x4000_0000, "GUIDELINE"),
    (0x0000_0001, "CANDIDATEWINDOW0"),
    (0x0000_0002, "CANDIDATEWINDOW1"),
    (0x0000_0004, "CANDIDATEWINDOW2"),
    (0x0000_0008, "CANDIDATEWINDOW3"),
];

/// `WM_IME_NOTIFY`'s `wParam` commands.
const NOTIFY_COMMANDS: &[(u32, &str)] = &[
    (IMN_CLOSESTATUSWINDOW, "IMN_CLOSESTATUSWINDOW"),
    (IMN_OPENSTATUSWINDOW, "IMN_OPENSTATUSWINDOW"),
    (IMN_CHANGECANDIDATE, "IMN_CHANGECANDIDATE"),
    (IMN_CLOSECANDIDATE, "IMN_CLOSECANDIDATE"),
    (IMN_OPENCANDIDATE, "IMN_OPENCANDIDATE"),
    (IMN_SETCONVERSIONMODE, "IMN_SETCONVERSIONMODE"),
    (IMN_SETSENTENCEMODE, "IMN_SETSENTENCEMODE"),
    (IMN_SETOPENSTATUS, "IMN_SETOPENSTATUS"),
    (IMN_SETCANDIDATEPOS, "IMN_SETCANDIDATEPOS"),
    (IMN_SETCOMPOSITIONFONT, "IMN_SETCOMPOSITIONFONT"),
    (IMN_SETCOMPOSITIONWINDOW, "IMN_SETCOMPOSITIONWINDOW"),
    (IMN_SETSTATUSWINDOWPOS, "IMN_SETSTATUSWINDOWPOS"),
    (IMN_GUIDELINE, "IMN_GUIDELINE"),
    (IMN_PRIVATE, "IMN_PRIVATE"),
    (IMN_SOFTKBDDESTROYED, "IMN_SOFTKBDDESTROYED"),
];

/// `WM_IME_REQUEST`'s `wParam` commands.
const REQUEST_COMMANDS: &[(u32, &str)] = &[
    (IMR_COMPOSITIONWINDOW, "IMR_COMPOSITIONWINDOW"),
    (IMR_CANDIDATEWINDOW, "IMR_CANDIDATEWINDOW"),
    (IMR_COMPOSITIONFONT, "IMR_COMPOSITIONFONT"),
    (IMR_RECONVERTSTRING, "IMR_RECONVERTSTRING"),
    (IMR_CONFIRMRECONVERTSTRING, "IMR_CONFIRMRECONVERTSTRING"),
    (IMR_QUERYCHARPOSITION, "IMR_QUERYCHARPOSITION"),
    (IMR_DOCUMENTFEED, "IMR_DOCUMENTFEED"),
];

/// A TSF context's static status bits (`TS_SS_*`).
const STATIC_STATUS_FLAGS: &[(u32, &str)] = &[
    (TS_SS_DISJOINTSEL, "DISJOINTSEL"),
    (TS_SS_REGIONS, "REGIONS"),
    (TS_SS_TRANSITORY, "TRANSITORY"),
    (TS_SS_NOHIDDENTEXT, "NOHIDDENTEXT"),
    (TS_SS_TKBAUTOCORRECTENABLE, "TKBAUTOCORRECTENABLE"),
    (TS_SS_TKBPREDICTIONENABLE, "TKBPREDICTIONENABLE"),
    (TS_SS_UWPCONTROL, "UWPCONTROL"),
];

/// An input processor profile's flags (`TF_IPP_FLAG_*`).
const PROFILE_FLAGS: &[(u32, &str)] = &[
    (TF_IPP_FLAG_ACTIVE, "ACTIVE"),
    (TF_IPP_FLAG_ENABLED, "ENABLED"),
    (
        TF_IPP_FLAG_SUBSTITUTEDBYINPUTPROCESSOR,
        "SUBSTITUTEDBYINPUTPROCESSOR",
    ),
];

/// Where `GetCurrentInputMessageSource` says the message being dispatched came
/// from, as `device/origin`.
fn input_source() -> String {
    let mut source = INPUT_MESSAGE_SOURCE::default();
    if let Err(error) = unsafe { GetCurrentInputMessageSource(&mut source) } {
        return format!("<{}>", hresult(&error));
    }
    let device = match source.deviceType {
        IMDT_UNAVAILABLE => "unavailable".to_owned(),
        IMDT_KEYBOARD => "keyboard".to_owned(),
        IMDT_MOUSE => "mouse".to_owned(),
        IMDT_TOUCH => "touch".to_owned(),
        IMDT_PEN => "pen".to_owned(),
        IMDT_TOUCHPAD => "touchpad".to_owned(),
        other => format!("{:#x}", other.0),
    };
    let origin = match source.originId {
        IMO_UNAVAILABLE => "unavailable".to_owned(),
        IMO_HARDWARE => "hardware".to_owned(),
        IMO_INJECTED => "injected".to_owned(),
        IMO_SYSTEM => "system".to_owned(),
        other => format!("{:#x}", other.0),
    };
    format!("{device}/{origin}")
}

/// A failed call's `HRESULT` and message. A call that succeeded with a null
/// out-pointer (which `windows` reports as an error carrying `S_OK`) reads
/// as `null`.
fn hresult(error: &windows::core::Error) -> String {
    let code = error.code();
    if code.is_ok() {
        return "null".to_owned();
    }
    format!("{:#010x} {}", code.0 as u32, error.message().trim_end())
}

// ---------------------------------------------------------------------------
// The IMM probe
// ---------------------------------------------------------------------------

/// Read the window's current composition and result strings.
fn probe_imm(hwnd: HWND, seq: u64) {
    let himc = unsafe { ImmGetContext(hwnd) };
    if himc.is_invalid() {
        tap_trace!("#{seq}   imm: the window has no input context");
        return;
    }
    let composition = composition_string(himc, GCS_COMPSTR);
    let result = composition_string(himc, GCS_RESULTSTR);
    let cursor = unsafe { ImmGetCompositionStringW(himc, GCS_CURSORPOS, None, 0) };
    let delta = unsafe { ImmGetCompositionStringW(himc, GCS_DELTASTART, None, 0) };
    unsafe {
        let _ = ImmReleaseContext(hwnd, himc);
    }
    tap_trace!("#{seq}   imm: comp={composition} result={result} cursor={cursor} delta={delta}");
}

/// One of an input context's strings, with its UTF-16 units in hex, so a lone
/// surrogate shows. Reading does not consume it.
fn composition_string(himc: HIMC, which: IME_COMPOSITION_STRING) -> String {
    // The length is in bytes; a negative one is `IMM_ERROR_NODATA` (-1) or
    // `IMM_ERROR_GENERAL` (-2).
    let bytes = unsafe { ImmGetCompositionStringW(himc, which, None, 0) };
    if bytes < 0 {
        return format!("<error {bytes}>");
    }
    if bytes == 0 {
        return "<none>".to_owned();
    }
    let mut units = vec![0u16; bytes as usize / 2];
    let read = unsafe {
        ImmGetCompositionStringW(
            himc,
            which,
            Some(units.as_mut_ptr().cast()),
            (units.len() * 2) as u32,
        )
    };
    if read < 0 {
        return format!("<error {read}>");
    }
    units.truncate(read as usize / 2);
    let hex: Vec<String> = units.iter().map(|unit| format!("{unit:04x}")).collect();
    format!("{:?} [{}]", String::from_utf16_lossy(&units), hex.join(" "))
}

// ---------------------------------------------------------------------------
// The TSF probe
// ---------------------------------------------------------------------------

/// How much of TSF a probe reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// After a composition message: the focused context, its compositions and
    /// the active keyboard TIP.
    Message,
    /// At attach: the same, and the compartments at every level.
    Baseline,
}

/// Ask TSF about the focused context and its compositions, the compartments
/// at a baseline, and which keyboard TIP is active.
fn probe_tsf(seq: u64, reach: Reach) {
    let apartment = match com_apartment() {
        Ok(apartment) => apartment,
        Err(problem) => {
            tap_trace!("#{seq}   tsf: {problem}; not probed");
            return;
        }
    };
    match existing_thread_manager() {
        Ok(manager) => probe_thread_manager(seq, apartment, &manager, reach),
        Err(why) => tap_trace!("#{seq}   tsf: apartment={apartment}, no thread manager: {why}"),
    }
    probe_keyboard_profile(seq);
}

/// This thread's COM apartment, or why COM cannot be used here. Never
/// initialises COM.
fn com_apartment() -> Result<&'static str, String> {
    let mut kind = APTTYPE::default();
    let mut qualifier = APTTYPEQUALIFIER::default();
    match unsafe { CoGetApartmentType(&mut kind, &mut qualifier) } {
        Ok(()) => Ok(match kind {
            APTTYPE_STA => "STA",
            APTTYPE_MAINSTA => "main STA",
            APTTYPE_MTA => "MTA",
            APTTYPE_NA => "NA",
            _ => "unknown",
        }),
        Err(error) if error.code() == CO_E_NOTINITIALIZED => {
            Err("COM is not initialised on this thread".to_owned())
        }
        Err(error) => Err(format!("CoGetApartmentType failed: {}", hresult(&error))),
    }
}

/// The thread manager this thread already has.
///
/// msctf exports `TF_GetThreadMgr` with no import library, hence
/// `GetProcAddress`. When msctf is not loaded, nothing in the process is using
/// TSF.
fn existing_thread_manager() -> Result<ITfThreadMgr, String> {
    /// `HRESULT WINAPI TF_GetThreadMgr(ITfThreadMgr **pptim)`.
    type GetThreadMgr = unsafe extern "system" fn(*mut *mut c_void) -> HRESULT;

    let module = unsafe { GetModuleHandleW(w!("msctf.dll")) }
        .map_err(|_| "msctf.dll is not loaded".to_owned())?;
    let Some(export) = (unsafe { GetProcAddress(module, s!("TF_GetThreadMgr")) }) else {
        return Err("msctf.dll exports no TF_GetThreadMgr".to_owned());
    };
    // SAFETY: the export has the signature `GetThreadMgr` spells out.
    let get = unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, GetThreadMgr>(export)
    };
    let mut raw: *mut c_void = std::ptr::null_mut();
    let result = unsafe { get(&mut raw) };
    if result.is_err() {
        return Err(format!(
            "TF_GetThreadMgr failed: {}",
            hresult(&windows::core::Error::from_hresult(result))
        ));
    }
    if raw.is_null() {
        return Err("this thread has none".to_owned());
    }
    // SAFETY: `TF_GetThreadMgr` hands over a reference it has added;
    // `from_raw` takes it over and releases it on drop.
    Ok(unsafe { ITfThreadMgr::from_raw(raw) })
}

/// The focused document and its top context, the owner of each composition
/// in it, and at a baseline the compartments at every level.
fn probe_thread_manager(seq: u64, apartment: &str, manager: &ITfThreadMgr, reach: Reach) {
    let thread_focus = match unsafe { manager.IsThreadFocus() } {
        Ok(focus) => focus.as_bool().to_string(),
        Err(error) => format!("<{}>", hresult(&error)),
    };
    let prefix = format!("tsf: apartment={apartment}, thread focus={thread_focus}");
    let document = match unsafe { manager.GetFocus() } {
        Ok(document) => Some(document),
        Err(error) => {
            tap_trace!("#{seq}   {prefix}, focused document: {}", hresult(&error));
            None
        }
    };
    let context = document
        .as_ref()
        .and_then(|document| match unsafe { document.GetTop() } {
            Ok(context) => Some(context),
            Err(error) => {
                tap_trace!("#{seq}   {prefix}, top context: {}", hresult(&error));
                None
            }
        });
    if let Some(context) = &context {
        match unsafe { context.GetStatus() } {
            Ok(status) => tap_trace!(
                "#{seq}   {prefix}, top context dynamic={:#x} static={:#x} [{}]",
                status.dwDynamicFlags,
                status.dwStaticFlags,
                flag_names(status.dwStaticFlags, STATIC_STATUS_FLAGS)
            ),
            Err(error) => {
                tap_trace!("#{seq}   {prefix}, top context status: {}", hresult(&error))
            }
        }
        probe_compositions(seq, context);
    }
    if reach != Reach::Baseline {
        return;
    }
    probe_compartments(seq, "global", unsafe { manager.GetGlobalCompartment() });
    probe_compartments(seq, "thread", manager.cast());
    if let Some(document) = &document {
        probe_compartments(seq, "document", document.cast());
    }
    if let Some(context) = &context {
        probe_compartments(seq, "context", context.cast());
    }
}

/// The owner of each composition in `context`.
fn probe_compositions(seq: u64, context: &ITfContext) {
    let views = match context
        .cast::<ITfContextComposition>()
        .and_then(|composition| unsafe { composition.EnumCompositions() })
    {
        Ok(views) => views,
        Err(error) => {
            tap_trace!("#{seq}   tsf: compositions: {}", hresult(&error));
            return;
        }
    };
    /// A context holds one composition, rarely more; the cap only bounds a
    /// misbehaving enumerator.
    const MAX_COMPOSITIONS: usize = 16;
    let mut count = 0;
    while count < MAX_COMPOSITIONS {
        let mut slot = [None];
        let mut fetched = 0u32;
        if let Err(error) = unsafe { views.Next(&mut slot, &mut fetched) } {
            tap_trace!("#{seq}   tsf: composition {count}: {}", hresult(&error));
            break;
        }
        let Some(view) = slot[0].take().filter(|_| fetched > 0) else {
            break;
        };
        match unsafe { view.GetOwnerClsid() } {
            Ok(owner) => tap_trace!("#{seq}   tsf: composition {count} owner={{{owner:?}}}"),
            Err(error) => {
                tap_trace!(
                    "#{seq}   tsf: composition {count} owner: {}",
                    hresult(&error)
                )
            }
        }
        count += 1;
    }
    if count == 0 {
        tap_trace!("#{seq}   tsf: no composition in the top context");
    }
}

/// The compartments the trace always reads, at every level.
const WATCHED_COMPARTMENTS: &[(GUID, &str)] = &[
    (GUID_COMPARTMENT_SPEECH_OPENCLOSE, "SPEECH_OPENCLOSE"),
    (GUID_COMPARTMENT_SPEECH_DISABLED, "SPEECH_DISABLED"),
    (GUID_COMPARTMENT_SPEECH_GLOBALSTATE, "SPEECH_GLOBALSTATE"),
    (
        GUID_COMPARTMENT_HANDWRITING_OPENCLOSE,
        "HANDWRITING_OPENCLOSE",
    ),
    (GUID_COMPARTMENT_KEYBOARD_OPENCLOSE, "KEYBOARD_OPENCLOSE"),
    (GUID_COMPARTMENT_KEYBOARD_DISABLED, "KEYBOARD_DISABLED"),
];

/// Other compartments TSF defines, named when a level lists them.
const OTHER_KNOWN_COMPARTMENTS: &[(GUID, &str)] = &[
    (GUID_COMPARTMENT_CONVERSIONMODEBIAS, "CONVERSIONMODEBIAS"),
    (GUID_COMPARTMENT_EMPTYCONTEXT, "EMPTYCONTEXT"),
    (
        GUID_COMPARTMENT_ENABLED_PROFILES_UPDATED,
        "ENABLED_PROFILES_UPDATED",
    ),
    (GUID_COMPARTMENT_KEYBOARD_INPUTMODE, "KEYBOARD_INPUTMODE"),
    (
        GUID_COMPARTMENT_KEYBOARD_INPUTMODE_CONVERSION,
        "KEYBOARD_INPUTMODE_CONVERSION",
    ),
    (
        GUID_COMPARTMENT_KEYBOARD_INPUTMODE_SENTENCE,
        "KEYBOARD_INPUTMODE_SENTENCE",
    ),
    (GUID_COMPARTMENT_SAPI_AUDIO, "SAPI_AUDIO"),
    (GUID_COMPARTMENT_SPEECH_CFGMENU, "SPEECH_CFGMENU"),
    (GUID_COMPARTMENT_SPEECH_UI_STATUS, "SPEECH_UI_STATUS"),
    (GUID_COMPARTMENT_TIPUISTATUS, "TIPUISTATUS"),
    (GUID_COMPARTMENT_TRANSITORYEXTENSION, "TRANSITORYEXTENSION"),
    (
        GUID_COMPARTMENT_TRANSITORYEXTENSION_DOCUMENTMANAGER,
        "TRANSITORYEXTENSION_DOCUMENTMANAGER",
    ),
    (
        GUID_COMPARTMENT_TRANSITORYEXTENSION_PARENT,
        "TRANSITORYEXTENSION_PARENT",
    ),
];

/// The most compartments one level is expected to list; the cap only bounds
/// a misbehaving enumerator.
const MAX_LISTED_COMPARTMENTS: usize = 64;

/// Read one level's compartments: the watched ones whether or not the level
/// lists them, then every other one it lists.
fn probe_compartments(seq: u64, level: &str, manager: windows::core::Result<ITfCompartmentMgr>) {
    let manager = match manager {
        Ok(manager) => manager,
        Err(error) => {
            tap_trace!("#{seq}   tsf: {level} compartments: {}", hresult(&error));
            return;
        }
    };
    // Listed before anything is read, so the list is what existed before the
    // probe looked.
    let (listed, listing) = match listed_compartments(&manager) {
        Ok(listed) => {
            let listing = format!("{} listed", listed.len());
            (listed, listing)
        }
        Err(error) => (Vec::new(), format!("listing: {}", hresult(&error))),
    };
    let watched: Vec<String> = WATCHED_COMPARTMENTS
        .iter()
        .map(|(guid, name)| {
            let mark = if listed.contains(guid) {
                ""
            } else {
                " (unlisted)"
            };
            format!("{name}={}{mark}", read_compartment(&manager, guid))
        })
        .collect();
    tap_trace!(
        "#{seq}   tsf: {level} compartments ({listing}): {}",
        watched.join(", ")
    );
    let others: Vec<String> = listed
        .iter()
        .filter(|guid| {
            !WATCHED_COMPARTMENTS
                .iter()
                .any(|(watched, _)| watched == *guid)
        })
        .map(|guid| {
            format!(
                "{}={}",
                compartment_name(guid),
                read_compartment(&manager, guid)
            )
        })
        .collect();
    if !others.is_empty() {
        tap_trace!(
            "#{seq}   tsf: {level} compartments, others: {}",
            others.join(", ")
        );
    }
}

/// The compartments a level lists, in its order.
fn listed_compartments(manager: &ITfCompartmentMgr) -> windows::core::Result<Vec<GUID>> {
    let guids = unsafe { manager.EnumCompartments() }?;
    let mut listed = Vec::new();
    while listed.len() < MAX_LISTED_COMPARTMENTS {
        let mut batch = [GUID::zeroed(); 16];
        let mut fetched = 0u32;
        let result = unsafe { guids.Next(&mut batch, Some(&mut fetched)) };
        result.ok()?;
        let fetched = (fetched as usize).min(batch.len());
        listed.extend_from_slice(&batch[..fetched]);
        // `S_FALSE` (fewer than asked for) ends the list.
        if result != S_OK || fetched == 0 {
            break;
        }
    }
    Ok(listed)
}

/// A compartment's name, or its GUID when TSF does not define it.
fn compartment_name(guid: &GUID) -> String {
    WATCHED_COMPARTMENTS
        .iter()
        .chain(OTHER_KNOWN_COMPARTMENTS)
        .find(|(known, _)| known == guid)
        .map_or_else(|| format!("{{{guid:?}}}"), |(_, name)| (*name).to_owned())
}

/// One compartment's value, or why it could not be read.
fn read_compartment(manager: &ITfCompartmentMgr, guid: &GUID) -> String {
    let compartment = match unsafe { manager.GetCompartment(guid) } {
        Ok(compartment) => compartment,
        Err(error) => return format!("<{}>", hresult(&error)),
    };
    match unsafe { compartment.GetValue() } {
        Ok(mut value) => describe_variant(&mut value),
        Err(error) => format!("<{}>", hresult(&error)),
    }
}

/// A `VARIANT`'s type and value, then cleared (the caller owns it, and
/// `VARIANT` has no `Drop`).
fn describe_variant(value: &mut VARIANT) -> String {
    // SAFETY: `vt` says which member of the union is live, and only that
    // member is read.
    let text = unsafe {
        let tagged = &value.Anonymous.Anonymous;
        let data = &tagged.Anonymous;
        match tagged.vt {
            VT_EMPTY => "empty".to_owned(),
            VT_I4 => format!("I4 {}", data.lVal),
            VT_INT => format!("INT {}", data.intVal),
            VT_UI4 => format!("UI4 {}", data.ulVal),
            VT_UINT => format!("UINT {}", data.uintVal),
            VT_I2 => format!("I2 {}", data.iVal),
            VT_BOOL => format!("BOOL {}", data.boolVal.0 != 0),
            VT_BSTR => format!("BSTR {:?}", (*data.bstrVal).to_string()),
            VT_UNKNOWN => {
                let set = if data.punkVal.is_some() {
                    "non-null"
                } else {
                    "null"
                };
                format!("UNKNOWN {set}")
            }
            other => format!("vt={:#x}", other.0),
        }
    };
    unsafe {
        let _ = VariantClear(value);
    }
    text
}

/// The active keyboard input processor profile: a TIP (an IME, voice
/// typing's, if it has one) or a plain keyboard layout.
fn probe_keyboard_profile(seq: u64) {
    let profiles: ITfInputProcessorProfileMgr = match unsafe {
        CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)
    } {
        Ok(profiles) => profiles,
        Err(error) => {
            tap_trace!("#{seq}   tsf: profile manager: {}", hresult(&error));
            return;
        }
    };
    let mut profile = TF_INPUTPROCESSORPROFILE::default();
    if let Err(error) = unsafe { profiles.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &mut profile) }
    {
        tap_trace!("#{seq}   tsf: active keyboard profile: {}", hresult(&error));
        return;
    }
    let kind = match profile.dwProfileType {
        TF_PROFILETYPE_INPUTPROCESSOR => "TIP".to_owned(),
        TF_PROFILETYPE_KEYBOARDLAYOUT => "keyboard layout".to_owned(),
        other => format!("type {other:#x}"),
    };
    tap_trace!(
        "#{seq}   tsf: active keyboard profile: {kind} langid={:#06x} clsid={{{:?}}} profile={{{:?}}} hkl={:#x} flags={:#x} [{}]",
        profile.langid,
        profile.clsid,
        profile.guidProfile,
        profile.hkl.0 as usize,
        profile.dwFlags,
        flag_names(profile.dwFlags, PROFILE_FLAGS)
    );
}

// ---------------------------------------------------------------------------
// The foreground probe
// ---------------------------------------------------------------------------

/// The foreground window, its class, and the image of the process that owns
/// it. `hwnd` is the tapped window, so the line says whether the foreground is
/// it.
fn probe_foreground(seq: u64, hwnd: HWND) {
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_invalid() {
        tap_trace!("#{seq}   fg: no foreground window");
        return;
    }
    let mut class = [0u16; 256];
    let length = unsafe { GetClassNameW(foreground, &mut class) };
    let class = match usize::try_from(length) {
        Ok(length) if length > 0 => format!(
            "{:?}",
            String::from_utf16_lossy(&class[..length.min(class.len())])
        ),
        _ => "<unknown>".to_owned(),
    };
    let mut pid = 0u32;
    let tid = unsafe { GetWindowThreadProcessId(foreground, Some(&mut pid)) };
    let this_process = pid == unsafe { GetCurrentProcessId() };
    tap_trace!(
        "#{seq}   fg: hwnd={:#x} class={class} pid={pid} tid={tid} this window={} this process={this_process} image={}",
        foreground.0 as usize,
        foreground == hwnd,
        process_image(pid)
    );
}

/// The full path of a process's image, or why it could not be read (a
/// protected process refuses even the limited query).
fn process_image(pid: u32) -> String {
    let process = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(process) => process,
        Err(error) => return format!("<{}>", hresult(&error)),
    };
    let mut path = [0u16; 1024];
    let mut length = path.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        )
    };
    unsafe {
        let _ = CloseHandle(process);
    }
    match result {
        Ok(()) => format!(
            "{:?}",
            String::from_utf16_lossy(&path[..(length as usize).min(path.len())])
        ),
        Err(error) => format!("<{}>", hresult(&error)),
    }
}

// ---------------------------------------------------------------------------
// Ending a composition
// ---------------------------------------------------------------------------

/// End the window's composition without the input method delivering its text:
/// the focused field has already kept what the composition showed (a click
/// elsewhere, a focus change).
///
/// It cancels the composition (`ImmNotifyIME(NI_COMPOSITIONSTR, CPS_CANCEL)`)
/// rather than completing it: `CPS_COMPLETE` would hand the composition's text
/// over as a result, and winit would commit it a second time. What winit then
/// receives, and why it commits nothing:
///
/// 1. The input method clears the composition and sends `WM_IME_COMPOSITION`
///    with `lParam` 0. winit turns that into an empty `Ime::Preedit` and stays
///    in its preedit state (only a result moves it out).
/// 2. It sends `WM_IME_ENDCOMPOSITION`. winit, still in its preedit state,
///    reads `GCS_RESULTSTR` and commits it (a workaround for Hangul input
///    methods, which send their last result after this message). winit counts
///    an empty string as text, so it sends an empty `Ime::Preedit` and an
///    empty `Ime::Commit`, then `Ime::Disabled`.
///
/// The result string is empty there: a cancel produces no result, and CUAS
/// (which serves every TSF input method to an IMM32 window like winit's)
/// writes the context's composition strings afresh at each update, so a
/// result an earlier message carried does not survive into the cancel's. An
/// empty commit inserts nothing in any Teksilo text field. An input method
/// that kept an earlier result through a cancel would have it committed
/// again; the trace logs the strings before and after the cancel, and after
/// winit has handled `WM_IME_ENDCOMPOSITION`, to show it.
///
/// The field must stop treating the composition as live before it calls this:
/// winit delivers the empty preedit after the call (it holds events while the
/// application handles one), and a field still holding the preedit's range
/// would delete the text it meant to keep.
///
/// Returns whether the window had an input context to cancel in. `false` when
/// the window is not attached or has none (its IME is not allowed, so nothing
/// is composing), and the caller's fallback, toggling winit's IME, then finds
/// nothing to reset either.
pub(super) fn finish_composition(window_id: TeksiloWindowId) -> bool {
    let hwnd = WINDOWS
        .try_with(|windows| windows.borrow().get(&window_id).copied())
        .ok()
        .flatten();
    let seq = next_sequence();
    let Some(hwnd) = hwnd else {
        tap_trace!("#{seq} {window_id} finish composition: the window is not attached");
        return false;
    };
    let himc = unsafe { ImmGetContext(hwnd) };
    if himc.is_invalid() {
        tap_trace!("#{seq} {window_id} finish composition: the window has no input context");
        return false;
    }
    let tracing = super::trace_enabled();
    if tracing {
        tap_trace!(
            "#{seq} {window_id} finish composition: comp={} result={}",
            composition_string(himc, GCS_COMPSTR),
            composition_string(himc, GCS_RESULTSTR)
        );
    }
    // The input method sends its messages to the window from inside this
    // call; winit handles them as they come and holds the events they make
    // until the application is done with the one it is handling.
    let cancelled = unsafe { ImmNotifyIME(himc, NI_COMPOSITIONSTR, CPS_CANCEL, 0) };
    if tracing {
        tap_trace!(
            "#{seq}   cancel returned {}; now comp={} result={}",
            cancelled.as_bool(),
            composition_string(himc, GCS_COMPSTR),
            composition_string(himc, GCS_RESULTSTR)
        );
    }
    unsafe {
        let _ = ImmReleaseContext(hwnd, himc);
    }
    true
}
