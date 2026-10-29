// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Windows: a TSF text store, Phases 0 to 3 of
//! `engineering/docs/tsf-text-store.md` (7.1): reads, writes, compositions,
//! notifications and the lock model, behind `TEKSILO_TSF` ([`TextStoreMode`]).
//!
//! What it does:
//!
//! - **The thread** is activated once (`ITfThreadMgrEx::ActivateEx`, else
//!   `ITfThreadMgr::Activate`), on the existing thread manager CUAS made
//!   (`TF_GetThreadMgr`) or a new one, if the UI thread is an STA; it never
//!   initialises COM. Keys follow Chromium's model: TSF processes them
//!   itself, and an `ITfKeyTraceEventSink` on the thread manager says which
//!   it saw (M9); a key traced with no `WM_KEYDOWN` for it was taken
//!   ([`KeyTrace`]), which the bare-Alt menu asks (4.3). The thread is never
//!   deactivated, and its COM pointers are leaked at thread exit rather than
//!   released in an order nobody controls (4.5).
//! - **Each window** gets its own empty document manager with an empty,
//!   disabled context (`GUID_COMPARTMENT_KEYBOARD_DISABLED` and
//!   `GUID_COMPARTMENT_EMPTYCONTEXT` set to 1), associated with its HWND
//!   (`AssociateFocus`), as Chromium does (4.6). For the IMM path
//!   ([`StoreFocus::Imm`]) the HWND is associated back with what that call
//!   handed back (nothing, measured), so that CUAS's own document takes the
//!   focus as winit's IMM context activates (M2). It is associated back at
//!   detach too. A secure field ([`StoreFocus::Secure`]) gets the empty
//!   context, and speech is turned off on the thread while it has the focus
//!   (section 6).
//! - **Each focus session** on a widget that describes its document gets a
//!   new document manager and context over a [`TextStore`]
//!   (`ITextStoreACP`, `ITfContextOwnerCompositionSink`) and an
//!   [`EditSink`] (`ITfTextEditSink`). The store serves that widget, found by
//!   its id, until the session ends, so a text service finishing a
//!   composition a focus change ended writes into it. TSF focus moves only
//!   from [`set_text_store_focus`], which `teksilo-app` calls after every
//!   dispatched event (case 2 of 4.4), never while a `RequestLock` is on the
//!   stack, and only for the window that has the Win32 focus (M14).
//! - **Reads** answer from the document (4.8), in UTF-16 through its index,
//!   and `GetTextExt` with the range's own lines.
//! - **Writes** (4.9) go through the field's write half
//!   ([`TextInputEditor`]): `SetText` and `InsertTextAtSelection` replace a
//!   range (`TS_ST_CORRECTION` keeps the replaced run's format, a query is a
//!   dry run), `SetSelection` sets the selection, and each answers what
//!   landed. Every grant is a batch of the field's (`begin_input_batch`), so
//!   what the application sees of the writes follows as the grant ends.
//! - **Compositions** are ranges of the field's text: `OnStartComposition`
//!   marks the range the text service names (`mark_composition`), the edit
//!   sink hands over its clauses, each styled by its display attribute
//!   (`GUID_PROP_ATTRIBUTE`, `ITfDisplayAttributeMgr`), and
//!   `OnEndComposition` ends it as one undo step (`end_composition`).
//!   [`terminate_composition`] (`TerminateComposition`) ends one before a
//!   press is dispatched and when a field ends its composition (4.11).
//! - **Notifications** (4.10): at every [`set_text_store_focus`], typing the
//!   field holds for its frame is flushed, and the changes since the last
//!   report go out, the journal's range followed into a [`TextMirror`] (the
//!   text as the text services last heard of it); changes the text services
//!   made themselves, and the selection their writes left, are not told
//!   back. At the tail of `RequestLock`, the changes a message made during
//!   the grant.
//! - **Locks** follow [`LockArbiter`] (4.7): while the field is busy or has
//!   changes not reported yet, a synchronous request is refused and an
//!   asynchronous one deferred, and a [`TextStorePayload`] asks
//!   `teksilo-app` for a settle, which reports, then grants it. A read-only
//!   field's store says so (`TS_SD_READONLY`) and refuses writes and
//!   compositions.
//!
//! Everything a text service does is traced (`TEKSILO_TEXT_INPUT_TRACE`),
//! with a `tsf:` tag: locks and their flags (M12), every read and write, the
//! compositions and their owners, the properties on every edited range, the
//! keys TSF saw, the focus calls.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;

use teksilo_canvas::{Point, Rect};
use teksilo_core::text_input_document::{
    ChangeCause, CompositionSegment, InputRevisions, SegmentStyle, TextInputDocument,
    TextInputEditor, Utf16Index, WriteOptions, WriteRefused,
};
use teksilo_core::text_surface::{TextSurface, TextSurfaces};
use teksilo_core::window::TeksiloWindowId;
use teksilo_core::{AppEventPoster, ImeOrigin, WidgetId};

use windows::Win32::Foundation::{
    E_FAIL, E_INVALIDARG, E_NOTIMPL, E_UNEXPECTED, HWND, LPARAM, POINT, RECT, S_OK, WPARAM,
};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, FORMATETC, IDataObject};
use windows::Win32::System::Ole::{CONNECT_E_ADVISELIMIT, CONNECT_E_CANNOTCONNECT};
use windows::Win32::System::Variant::{VARIANT, VT_EMPTY, VT_I4};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::TextServices::{
    CLSID_TF_CategoryMgr, CLSID_TF_DisplayAttributeMgr, CLSID_TF_ThreadMgr,
    GUID_COMPARTMENT_EMPTYCONTEXT, GUID_COMPARTMENT_KEYBOARD_DISABLED,
    GUID_COMPARTMENT_SPEECH_DISABLED, GUID_PROP_ATTRIBUTE, ITextStoreACP, ITextStoreACP_Impl,
    ITextStoreACPSink, ITfCategoryMgr, ITfCompartmentMgr, ITfCompositionView, ITfContext,
    ITfContextComposition, ITfContextOwnerCompositionServices, ITfContextOwnerCompositionSink,
    ITfContextOwnerCompositionSink_Impl, ITfDisplayAttributeMgr, ITfDocumentMgr, ITfEditRecord,
    ITfKeyTraceEventSink, ITfKeyTraceEventSink_Impl, ITfProperty, ITfRange, ITfRangeACP, ITfSource,
    ITfTextEditSink, ITfTextEditSink_Impl, ITfThreadMgr, ITfThreadMgrEx, TEXT_STORE_LOCK_FLAGS,
    TEXT_STORE_TEXT_CHANGE_FLAGS, TF_ATTR_CONVERTED, TF_ATTR_FIXEDCONVERTED,
    TF_ATTR_TARGET_CONVERTED, TF_ATTR_TARGET_NOTCONVERTED, TF_DA_ATTR_INFO, TF_DEFAULT_SELECTION,
    TF_DISABLE_COMMANDING, TF_DISABLE_DICTATION, TF_DISABLE_SPEECH, TF_DISPLAYATTRIBUTE,
    TF_GTP_INCL_TEXT, TS_AE_END, TS_AE_START, TS_ATTRVAL, TS_E_INVALIDPOINT, TS_E_INVALIDPOS,
    TS_E_NOLOCK, TS_E_READONLY, TS_E_SYNCHRONOUS, TS_IAS_QUERYONLY, TS_LC_CHANGE, TS_RT_PLAIN,
    TS_RUNINFO, TS_S_ASYNC, TS_SD_INPUTPANEMANUALDISPLAYENABLE, TS_SD_READONLY, TS_SELECTION_ACP,
    TS_SELECTIONSTYLE, TS_SS_NOHIDDENTEXT, TS_ST_CORRECTION, TS_STATUS, TS_TEXTCHANGE,
};
use windows::core::{BOOL, GUID, HRESULT, IUnknown, Interface, PCWSTR, PWSTR, Ref, implement};

use super::super::answers::{self, Utf16Range};
use super::super::key_trace::KeyTrace;
use super::super::tsf_lock::{LockArbiter, LockDecision, LockKind};
use super::super::tsf_mirror::{TextChange, TextMirror};
use super::super::{StoreFocus, TextStoreMode, TextStorePayload, text_store_mode};
use super::{
    com_apartment, describe_variant, existing_thread_manager, hresult, next_sequence, to_screen,
};

/// One `tsf:` trace line.
macro_rules! tsf_trace {
    ($($arg:tt)*) => {
        super::super::trace(format_args!("tsf: {}", format_args!($($arg)*)))
    };
}

/// The one view a store has (`GetActiveView`).
const VIEW: u32 = 1;

/// `TF_POP_ALL`, which the `windows` crate does not define.
const TF_POP_ALL: u32 = 1;

// ---------------------------------------------------------------------------
// The thread
// ---------------------------------------------------------------------------

/// The activated thread manager. Leaked at thread exit (see the module
/// documentation).
struct Thread {
    manager: ITfThreadMgr,
    client_id: u32,
}

thread_local! {
    /// The activated thread, or why it is not.
    static THREAD: RefCell<Option<Result<ManuallyDrop<Thread>, String>>> = const { RefCell::new(None) };
    /// Each attached window's TSF state, by HWND.
    static WINDOWS: RefCell<HashMap<isize, Rc<RefCell<WindowTsf>>>> = RefCell::new(HashMap::new());
    /// How many of the stores' `RequestLock` calls (and grants of a deferred
    /// request) are on the stack. While any is, focus does not move and
    /// nothing is reported (4.7, rules 3 and 4).
    static LOCK_DEPTH: Cell<u32> = const { Cell::new(0) };
    /// The thread's speech compartment as it was before a secure field took
    /// the focus, while one has it.
    static SPEECH_SAVED: Cell<Option<i32>> = const { Cell::new(None) };
}

/// One `RequestLock` (or deferred grant) on the stack, for [`LOCK_DEPTH`].
struct LockDepth;

impl LockDepth {
    fn enter() -> Self {
        let _ = LOCK_DEPTH.try_with(|depth| depth.set(depth.get() + 1));
        Self
    }

    /// Whether a lock call is on the stack.
    fn active() -> bool {
        LOCK_DEPTH.try_with(|depth| depth.get() > 0).unwrap_or(true)
    }
}

impl Drop for LockDepth {
    fn drop(&mut self) {
        let _ = LOCK_DEPTH.try_with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// The thread manager and the client id, activating the thread the first
/// time. `None` when it cannot be (no STA, no TSF).
fn thread() -> Option<(ITfThreadMgr, u32)> {
    THREAD
        .try_with(|slot| {
            let mut slot = slot.try_borrow_mut().ok()?;
            if slot.is_none() {
                let activated = activate();
                match &activated {
                    Ok(thread) => tsf_trace!("thread activated, client id {}", thread.client_id),
                    Err(problem) => tsf_trace!("thread not activated: {problem}"),
                }
                *slot = Some(activated.map(ManuallyDrop::new));
            }
            match slot.as_ref()? {
                Ok(thread) => Some((thread.manager.clone(), thread.client_id)),
                Err(_) => None,
            }
        })
        .ok()
        .flatten()
}

fn activate() -> Result<Thread, String> {
    let apartment = com_apartment()?;
    if !apartment.contains("STA") {
        return Err(format!("the UI thread is an {apartment}, not an STA"));
    }
    let manager = match existing_thread_manager() {
        Ok(manager) => manager,
        Err(_) => unsafe { CoCreateInstance(&CLSID_TF_ThreadMgr, None, CLSCTX_INPROC_SERVER) }
            .map_err(|error| format!("no thread manager: {}", hresult(&error)))?,
    };
    let mut client_id = 0;
    match manager.cast::<ITfThreadMgrEx>() {
        Ok(ex) => unsafe { ex.ActivateEx(&mut client_id, 0) }
            .map_err(|error| format!("ActivateEx failed: {}", hresult(&error)))?,
        Err(_) => {
            client_id = unsafe { manager.Activate() }
                .map_err(|error| format!("Activate failed: {}", hresult(&error)))?;
        }
    }
    // Which keys TSF saw (M9).
    let sink: ITfKeyTraceEventSink = KeyTraceSink.into();
    match manager
        .cast::<ITfSource>()
        .and_then(|source| unsafe { source.AdviseSink(&ITfKeyTraceEventSink::IID, &sink) })
    {
        Ok(_) => {}
        Err(error) => tsf_trace!("no key trace sink: {}", hresult(&error)),
    }
    // The sink stays advised for the thread's life.
    std::mem::forget(sink);
    Ok(Thread { manager, client_id })
}

/// Learns which keys TSF processed: Chromium's model (4.3, M9).
#[implement(ITfKeyTraceEventSink)]
struct KeyTraceSink;

thread_local! {
    /// Which keys a text service took (4.3), from the key trace and the
    /// window's key-downs.
    static KEY_TRACE: RefCell<KeyTrace> = RefCell::new(KeyTrace::default());
}

/// The window got `WM_KEYDOWN` or `WM_SYSKEYDOWN` for `vk`, which the text
/// services therefore passed on. From the subclass, ahead of winit: no TSF
/// call.
pub(super) fn note_window_key_down(vk: u32) {
    let _ = KEY_TRACE.try_with(|trace| {
        if let Ok(mut trace) = trace.try_borrow_mut() {
            trace.window_key_down(vk);
        }
    });
}

/// Whether a text service took a key since the last call.
pub(super) fn take_key_taken() -> bool {
    KEY_TRACE
        .try_with(|trace| {
            trace
                .try_borrow_mut()
                .is_ok_and(|mut trace| trace.take_taken())
        })
        .unwrap_or(false)
}

impl ITfKeyTraceEventSink_Impl for KeyTraceSink_Impl {
    fn OnKeyTraceDown(&self, wparam: WPARAM, lparam: LPARAM) -> windows::core::Result<()> {
        let _ = KEY_TRACE.try_with(|trace| {
            if let Ok(mut trace) = trace.try_borrow_mut() {
                trace.trace_down(wparam.0 as u32);
            }
        });
        tsf_trace!(
            "#{} key trace down vk={:#04x} lparam={:#x}",
            next_sequence(),
            wparam.0,
            lparam.0
        );
        Ok(())
    }

    fn OnKeyTraceUp(&self, wparam: WPARAM, lparam: LPARAM) -> windows::core::Result<()> {
        let _ = KEY_TRACE.try_with(|trace| {
            if let Ok(mut trace) = trace.try_borrow_mut() {
                trace.trace_up(wparam.0 as u32);
            }
        });
        tsf_trace!(
            "#{} key trace up vk={:#04x} lparam={:#x}",
            next_sequence(),
            wparam.0,
            lparam.0
        );
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Windows and focus
// ---------------------------------------------------------------------------

/// A window's TSF state.
struct WindowTsf {
    hwnd: HWND,
    window_id: TeksiloWindowId,
    surfaces: TextSurfaces,
    /// Asks `teksilo-app` to settle the window again ([`Service`]).
    service: Rc<Service>,
    /// The window's empty document manager, with its disabled context.
    empty: ITfDocumentMgr,
    /// What `AssociateFocus` handed back: CUAS's document manager.
    previous: Option<ITfDocumentMgr>,
    /// The focus session on a text field, if any.
    session: Option<Session>,
    /// The focus last given to TSF, while the window had the Win32 focus.
    applied: Option<StoreFocus>,
    /// Whether the HWND is associated with `empty` (true) or with what
    /// `AssociateFocus` handed back (false, for the IMM path).
    associated: bool,
}

/// One focus session on a field that describes its document.
struct Session {
    widget: WidgetId,
    manager: ITfDocumentMgr,
    context: ITfContext,
    state: Rc<StoreState>,
    edit_sink_cookie: Option<u32>,
}

/// Asks `teksilo-app`, through the window's poster, to call
/// [`set_text_store_focus`] again between dispatches: a lock request waits
/// for changes that could not be reported, or a call was skipped while a
/// lock call was on the stack. Posted once until that call runs.
struct Service {
    window_id: TeksiloWindowId,
    poster: Arc<dyn AppEventPoster>,
    posted: Cell<bool>,
}

impl Service {
    fn post(&self, why: &str) {
        if self.posted.replace(true) {
            return;
        }
        tsf_trace!(
            "#{} {} service posted: {why}",
            next_sequence(),
            self.window_id
        );
        self.poster.post_external(Box::new(TextStorePayload {
            window_id_owner: self.window_id,
        }));
    }
}

pub(super) fn attach(
    window_id: TeksiloWindowId,
    hwnd: HWND,
    poster: Arc<dyn AppEventPoster>,
    surfaces: TextSurfaces,
) {
    if text_store_mode() == TextStoreMode::Off {
        return;
    }
    let Some((manager, client_id)) = thread() else {
        return;
    };
    let empty = match empty_document(&manager, client_id) {
        Ok(empty) => empty,
        Err(error) => {
            tsf_trace!("{window_id} no empty document: {}", hresult(&error));
            return;
        }
    };
    let previous = unsafe { manager.AssociateFocus(hwnd, &empty) }.ok();
    tsf_trace!(
        "#{} {window_id} associated with its empty document (CUAS's before: {})",
        next_sequence(),
        if previous.is_some() { "one" } else { "none" }
    );
    let window = WindowTsf {
        hwnd,
        window_id,
        surfaces,
        service: Rc::new(Service {
            window_id,
            poster,
            posted: Cell::new(false),
        }),
        empty,
        previous,
        session: None,
        applied: None,
        associated: true,
    };
    let _ = WINDOWS.try_with(|windows| {
        windows
            .borrow_mut()
            .insert(hwnd.0 as isize, Rc::new(RefCell::new(window)))
    });
}

pub(super) fn detach(hwnd: HWND) {
    let Some(window) = WINDOWS
        .try_with(|windows| windows.borrow_mut().remove(&(hwnd.0 as isize)))
        .ok()
        .flatten()
    else {
        return;
    };
    let Ok(mut window) = window.try_borrow_mut() else {
        return;
    };
    if let Some(session) = window.session.take() {
        end_session(session);
    }
    if let Some((manager, client_id)) = thread() {
        if window.applied == Some(StoreFocus::Secure) {
            disable_speech(&manager, client_id, false);
        }
        let previous = window.previous.take();
        let _ = unsafe { manager.AssociateFocus(hwnd, previous.as_ref()) };
    }
    unsafe {
        let _ = window.empty.Pop(TF_POP_ALL);
    }
    tsf_trace!("#{} {} detached", next_sequence(), window.window_id);
}

/// An empty document manager with an empty, disabled context pushed.
fn empty_document(manager: &ITfThreadMgr, client_id: u32) -> windows::core::Result<ITfDocumentMgr> {
    let document = unsafe { manager.CreateDocumentMgr() }?;
    let mut context = None;
    let mut cookie = 0;
    unsafe { document.CreateContext(client_id, 0, None::<&IUnknown>, &mut context, &mut cookie) }?;
    let context = context.ok_or_else(|| windows::core::Error::from(E_FAIL))?;
    if let Ok(compartments) = context.cast::<ITfCompartmentMgr>() {
        for guid in [
            GUID_COMPARTMENT_KEYBOARD_DISABLED,
            GUID_COMPARTMENT_EMPTYCONTEXT,
        ] {
            let set =
                unsafe { compartments.GetCompartment(&guid) }.and_then(|compartment| unsafe {
                    compartment.SetValue(client_id, &int_variant(1))
                });
            if let Err(error) = set {
                tsf_trace!("compartment {guid:?} not set: {}", hresult(&error));
            }
        }
    }
    unsafe { document.Push(&context) }?;
    Ok(document)
}

/// A `VT_I4` variant.
fn int_variant(value: i32) -> VARIANT {
    let mut variant = VARIANT::default();
    // SAFETY: `vt` and the member it names are written together.
    unsafe {
        let tagged = &mut *variant.Anonymous.Anonymous;
        tagged.vt = VT_I4;
        tagged.Anonymous.lVal = value;
    }
    variant
}

/// See [`super::super::set_text_store_focus`].
pub(super) fn set_text_store_focus(hwnd: HWND, focus: StoreFocus) {
    let Some(window) = WINDOWS
        .try_with(|windows| windows.borrow().get(&(hwnd.0 as isize)).cloned())
        .ok()
        .flatten()
    else {
        return;
    };
    // A message delivered by a TSF call below can run the application,
    // which calls this again: that call does nothing.
    let Ok(mut window) = window.try_borrow_mut() else {
        tsf_trace!("set focus: re-entered; skipped");
        return;
    };
    // Inside a lock call (a message a text service's call delivered): no
    // `SetFocus`, `Push` or `Pop`, nothing reported (4.7, rule 4). Done
    // again between dispatches.
    if LockDepth::active() {
        tsf_trace!(
            "#{} {} set focus inside a lock call; deferred",
            next_sequence(),
            window.window_id
        );
        window.service.post("focus asked inside a lock call");
        return;
    }
    window.service.posted.set(false);
    let Some((manager, client_id)) = thread() else {
        return;
    };
    // The session follows the focused widget, whether or not the window has
    // the Win32 focus.
    let wanted_widget = match focus {
        StoreFocus::Document { widget } => Some(widget),
        _ => None,
    };
    if window.session.as_ref().map(|session| session.widget) != wanted_widget {
        if let Some(session) = window.session.take() {
            end_session(session);
            window.applied = None;
        }
        if let Some(widget) = wanted_widget {
            match start_session(&window, client_id, widget) {
                Ok(session) => window.session = Some(session),
                Err(error) => tsf_trace!(
                    "#{} {} no store for {widget:?}: {}",
                    next_sequence(),
                    window.window_id,
                    hresult(&error)
                ),
            }
        }
    }
    // TSF focus: only for the window the user types in.
    // The IMM path needs the HWND free of our empty document: an
    // associated document manager is what TSF focuses as the window's IMM
    // context activates, so CUAS's own document never took the focus and the
    // input method stayed off (measured, M2). Before winit allows its IME,
    // which `settle_ime` does after this call.
    let imm = focus == StoreFocus::Imm;
    if window.associated == imm {
        let document = if imm {
            window.previous.clone()
        } else {
            Some(window.empty.clone())
        };
        let result = unsafe { manager.AssociateFocus(window.hwnd, document.as_ref()) };
        tsf_trace!(
            "#{} {} {}",
            next_sequence(),
            window.window_id,
            if imm {
                "associated with CUAS's document again, for the IMM path"
            } else {
                "associated with its empty document again"
            }
        );
        drop(result);
        window.associated = !imm;
        window.applied = None;
    }
    let has_focus = unsafe { GetFocus() } == window.hwnd;
    if has_focus {
        // Speech stays off while a secure field has the focus (section 6).
        disable_speech(&manager, client_id, focus == StoreFocus::Secure);
    }
    if imm {
        // CUAS focuses its own document as winit's IMM context activates.
        window.applied = Some(focus);
    } else if has_focus && window.applied != Some(focus) {
        let target = match focus {
            StoreFocus::Document { .. } => window
                .session
                .as_ref()
                .map(|session| session.manager.clone()),
            StoreFocus::Imm | StoreFocus::None | StoreFocus::Secure => Some(window.empty.clone()),
        };
        let result = unsafe { manager.SetFocus(target.as_ref()) };
        tsf_trace!(
            "#{} {} SetFocus({focus:?}) {}",
            next_sequence(),
            window.window_id,
            match &result {
                Ok(()) => "ok".to_owned(),
                Err(error) => hresult(error),
            }
        );
        window.applied = Some(focus);
    } else if !has_focus && window.applied.is_some() {
        // Applied again when the window comes back.
        window.applied = None;
    }
    let state = window.session.as_ref().map(|session| session.state.clone());
    drop(window);
    if let Some(state) = state {
        // Typing waiting for the frame goes in now, so a text service never
        // asks for a lock with typing unreported (4.10); then the changes are
        // reported and a lock request that waited for them is granted.
        state.report(true);
        state.grant_deferred(true);
    }
}

/// End the composition a text service holds in the window's store, outside
/// any lock (4.11): before a press is dispatched, or as a field ends its
/// composition. Whether one was open. Never while a lock call is on the
/// stack.
pub(super) fn terminate_composition(hwnd: HWND) -> bool {
    if LockDepth::active() {
        return false;
    }
    let Some(window) = WINDOWS
        .try_with(|windows| windows.borrow().get(&(hwnd.0 as isize)).cloned())
        .ok()
        .flatten()
    else {
        return false;
    };
    let session = window.try_borrow().ok().and_then(|window| {
        window
            .session
            .as_ref()
            .map(|session| (session.state.clone(), session.context.clone()))
    });
    // No borrow of the window across the TSF call.
    drop(window);
    session.is_some_and(|(state, context)| state.terminate(&context))
}

/// Turn speech off on the thread (`GUID_COMPARTMENT_SPEECH_DISABLED`, a
/// bitmask on the thread manager's compartment), or give it back the value
/// it had before.
fn disable_speech(manager: &ITfThreadMgr, client_id: u32, disable: bool) {
    let saved = SPEECH_SAVED.try_with(Cell::get).ok().flatten();
    if disable == saved.is_some() {
        return;
    }
    let compartment = manager
        .cast::<ITfCompartmentMgr>()
        .and_then(|compartments| unsafe {
            compartments.GetCompartment(&GUID_COMPARTMENT_SPEECH_DISABLED)
        });
    let compartment = match compartment {
        Ok(compartment) => compartment,
        Err(error) => {
            tsf_trace!("no speech compartment: {}", hresult(&error));
            return;
        }
    };
    let result = if disable {
        let before = unsafe { compartment.GetValue() }.unwrap_or_default();
        let before = int_of(&before).unwrap_or(0);
        let _ = SPEECH_SAVED.try_with(|saved| saved.set(Some(before)));
        let mask = (TF_DISABLE_SPEECH | TF_DISABLE_DICTATION | TF_DISABLE_COMMANDING) as i32;
        unsafe { compartment.SetValue(client_id, &int_variant(before | mask)) }
    } else {
        let _ = SPEECH_SAVED.try_with(|saved| saved.set(None));
        unsafe { compartment.SetValue(client_id, &int_variant(saved.unwrap_or(0))) }
    };
    tsf_trace!(
        "#{} speech {}: {}",
        next_sequence(),
        if disable { "disabled" } else { "given back" },
        match result {
            Ok(()) => "ok".to_owned(),
            Err(error) => hresult(&error),
        }
    );
}

/// A `VT_I4` variant's value.
fn int_of(variant: &VARIANT) -> Option<i32> {
    // SAFETY: the member read is the one `vt` names.
    unsafe {
        let tagged = &variant.Anonymous.Anonymous;
        (tagged.vt == VT_I4).then(|| tagged.Anonymous.lVal)
    }
}

fn start_session(
    window: &WindowTsf,
    client_id: u32,
    widget: WidgetId,
) -> windows::core::Result<Session> {
    let (manager, _) = thread().ok_or_else(|| windows::core::Error::from(E_FAIL))?;
    let state = Rc::new(StoreState {
        hwnd: window.hwnd,
        window_id: window.window_id,
        widget,
        surfaces: window.surfaces.clone(),
        service: window.service.clone(),
        inner: RefCell::new(StoreInner::default()),
    });
    // The document's text before TSF first reads it.
    state.report(true);
    let store: ITextStoreACP = TextStore {
        state: state.clone(),
    }
    .into();
    let document = unsafe { manager.CreateDocumentMgr() }?;
    let mut context = None;
    let mut cookie = 0;
    unsafe { document.CreateContext(client_id, 0, &store, &mut context, &mut cookie) }?;
    let context = context.ok_or_else(|| windows::core::Error::from(E_FAIL))?;
    let edit_sink: ITfTextEditSink = EditSink {
        state: state.clone(),
    }
    .into();
    let edit_sink_cookie = context
        .cast::<ITfSource>()
        .and_then(|source| unsafe { source.AdviseSink(&ITfTextEditSink::IID, &edit_sink) })
        .inspect_err(|error| tsf_trace!("no edit sink: {}", hresult(error)))
        .ok();
    unsafe { document.Push(&context) }?;
    tsf_trace!(
        "#{} {} store for {widget:?}: {} units",
        next_sequence(),
        window.window_id,
        state.inner.borrow().mirror.len()
    );
    Ok(Session {
        widget,
        manager: document,
        context,
        state,
        edit_sink_cookie,
    })
}

fn end_session(session: Session) {
    if let Some(cookie) = session.edit_sink_cookie
        && let Ok(source) = session.context.cast::<ITfSource>()
    {
        let _ = unsafe { source.UnadviseSink(cookie) };
    }
    let _ = unsafe { session.manager.Pop(TF_POP_ALL) };
    tsf_trace!(
        "#{} {} store for {:?} ended; its text was {:?}",
        next_sequence(),
        session.state.window_id,
        session.widget,
        session.state.inner.borrow().mirror.text()
    );
}

// ---------------------------------------------------------------------------
// The store's state
// ---------------------------------------------------------------------------

/// What the store and its sinks share.
struct StoreState {
    hwnd: HWND,
    window_id: TeksiloWindowId,
    /// The field the store is for.
    widget: WidgetId,
    surfaces: TextSurfaces,
    service: Rc<Service>,
    inner: RefCell<StoreInner>,
}

/// The store's own state.
///
/// The mirror is the text and selection as the text services last heard of
/// them, in UTF-16: what a change is reported against. Reads are answered
/// from the field's document, and the text services' writes go to its
/// write half (`TextInputEditor`); the mirror answers a read only while the
/// document cannot.
#[derive(Default)]
struct StoreInner {
    mirror: TextMirror,
    locks: LockArbiter,
    sink: Option<ITextStoreACPSink>,
    compositions: u32,
    editable: bool,
    /// The document's revisions at the last report; `None` until the first,
    /// and for a document that counts none (a full comparison each time).
    reported: Option<InputRevisions>,
    /// The UTF-16 index of the text at the last report, where the journal's
    /// old offsets are converted.
    reported_index: Option<Rc<Utf16Index>>,
    /// The selection (UTF-16, and whether the caret is at its start) a text
    /// service's last write left: the text service knows it, so it is not
    /// reported back to it.
    store_selection: Option<(Range<usize>, bool)>,
    /// Window-logical, at the last report: answered while the document
    /// cannot place a range.
    caret: Option<Rect>,
    visible: Option<Rect>,
}

/// What a report tells the text services, once no borrow is held.
struct Notifications {
    sink: ITextStoreACPSink,
    change: Option<TextChange>,
    moved: bool,
    layout: bool,
    status: bool,
}

impl StoreState {
    /// The field the store is for, focused or not: a text service finishing
    /// a composition a focus change ended writes its last text there.
    fn surface(&self) -> Option<Rc<dyn TextSurface>> {
        self.surfaces.surface(self.widget)
    }

    /// Run `f` on the field's document.
    fn document<R>(&self, f: impl FnOnce(&dyn TextInputDocument) -> R) -> Option<R> {
        let surface = self.surface()?;
        let document = surface.input_document()?;
        Some(f(document))
    }

    /// Run `f` on the field's write half; `None` for a field that takes no
    /// writes from the input system (read-only, secure, or a widget that
    /// implements none).
    fn editor<R>(&self, f: impl FnOnce(&dyn TextInputEditor) -> R) -> Option<R> {
        let surface = self.surface()?;
        let editor = surface.input_editor()?;
        Some(f(editor))
    }

    /// Report what changed in the document since the last report:
    /// `OnTextChange` with the journal's range, `OnSelectionChange`,
    /// `OnLayoutChange`, `OnStatusChange` (4.10). Not while a lock is held.
    /// Changes the text services made themselves are followed, not reported.
    ///
    /// At a `settle` (between dispatches), typing the widget holds for its
    /// frame goes in first. At the tail of `RequestLock`, where no widget code
    /// may run, it does not.
    fn report(&self, settle: bool) {
        let held = |state: &Self| {
            state
                .inner
                .try_borrow()
                .map_or(true, |inner| inner.locks.held().is_some())
        };
        if held(self) {
            return;
        }
        if settle {
            let _ = self.document(|document| document.flush_pending_input());
        }
        let Some(notifications) = self.document(|document| self.compare(document)).flatten() else {
            return;
        };
        let Notifications {
            sink,
            change,
            moved,
            layout,
            status,
        } = notifications;
        if let Some(change) = change {
            tsf_trace!("#{} OnTextChange {change:?}", next_sequence());
            let _ =
                unsafe { sink.OnTextChange(TEXT_STORE_TEXT_CHANGE_FLAGS(0), &text_change(change)) };
        }
        if moved {
            tsf_trace!("#{} OnSelectionChange", next_sequence());
            let _ = unsafe { sink.OnSelectionChange() };
        }
        if layout {
            tsf_trace!("#{} OnLayoutChange", next_sequence());
            let _ = unsafe { sink.OnLayoutChange(TS_LC_CHANGE, VIEW) };
        }
        if status {
            tsf_trace!("#{} OnStatusChange", next_sequence());
            let _ = unsafe { sink.OnStatusChange(0) };
        }
    }

    /// Bring the mirror and the reported revisions up to the document, and
    /// say what to tell the text services. `None` when nothing is to be told
    /// or the document cannot be read now.
    fn compare(&self, document: &dyn TextInputDocument) -> Option<Notifications> {
        if document.is_secure() || !document.is_available() {
            return None;
        }
        let editable = document.is_editable() && self.editor(|_| ()).is_some();
        let mut inner = self.inner.try_borrow_mut().ok()?;
        if inner.locks.held().is_some() {
            return None;
        }
        let revisions = document.revisions();
        if revisions.is_some() && revisions == inner.reported && editable == inner.editable {
            return None;
        }
        let index = document.utf16_index()?;
        // Where the text services last heard the selection was: a selection
        // that is elsewhere now is told, even one a text change carried.
        let before = (inner.mirror.selection(), inner.mirror.caret_at_start());
        // The text: the journal's range where it can say, the whole text
        // compared otherwise.
        let followed = match (revisions, inner.reported, inner.reported_index.clone()) {
            (Some(now), Some(then), Some(old)) => {
                if now.text == then.text {
                    Some(None)
                } else {
                    follow_journal(document, &mut inner.mirror, then.text, &old, &index)
                }
            }
            _ => None,
        };
        let selection = document.selection()?;
        let caret = document.caret()?;
        let selection16 = index.char_range_to_utf16(selection.clone());
        let caret_at_start = caret == selection.start && !selection.is_empty();
        let change = match followed {
            Some(change) => change,
            None => {
                let text = document.text_in(0..index.char_count())?;
                let selection = inner.mirror.selection();
                let at_start = inner.mirror.caret_at_start();
                // The selection is compared below, after the text.
                let (change, _) = inner.mirror.sync(&text, selection, at_start);
                change
            }
        };
        // The one a text service's write left is its own and not told back.
        let known = inner.store_selection.take() == Some((selection16.clone(), caret_at_start));
        inner
            .mirror
            .follow_selection(selection16.clone(), caret_at_start);
        let moved = (selection16, caret_at_start) != before && !known;
        // The layout: by revision, or by the caret's and the visible text's
        // rectangles for a document that counts none.
        let layout_changed = match (revisions, inner.reported) {
            (Some(now), Some(then)) => now.layout != then.layout,
            _ => true,
        };
        let mut layout = false;
        if layout_changed {
            let caret_rect = answers::first_rect(
                document,
                Utf16Range::from_range(index.char_to_utf16(caret)..index.char_to_utf16(caret)),
            )
            .map(|(rect, _)| rect);
            let visible = document.visible_text_rect();
            layout = revisions.is_some() || inner.caret != caret_rect || inner.visible != visible;
            inner.caret = caret_rect;
            inner.visible = visible;
        }
        let status = inner.reported_index.is_some() && inner.editable != editable;
        inner.editable = editable;
        inner.reported = revisions;
        inner.reported_index = Some(index);
        let sink = inner.sink.clone()?;
        (change.is_some() || moved || layout || status).then_some(Notifications {
            sink,
            change,
            moved,
            layout,
            status,
        })
    }

    /// Whether a lock waits for the next settle (4.7, rule 1): the field
    /// holds typing not inserted yet, or has changes the text services have
    /// not heard of. Reads only. A document that counts no revisions is
    /// compared at each report instead, and never holds a lock back.
    fn dirty(&self) -> bool {
        self.document(|document| {
            if !document.is_available() {
                return true;
            }
            let Ok(inner) = self.inner.try_borrow() else {
                return false;
            };
            match (document.revisions(), inner.reported) {
                (Some(now), Some(then)) => now.text != then.text || now.selection != then.selection,
                _ => false,
            }
        })
        .unwrap_or(false)
    }

    /// Grant `kind`, then each request queued meanwhile, then report what
    /// changed during the grants and grant a request deferred meanwhile
    /// (4.7, rule 3). The first grant's result.
    ///
    /// Each grant is a batch of the field's (`begin_input_batch`,
    /// `end_input_batch`): the text service's writes change only the
    /// field's own state while it holds the lock, and what the application
    /// sees of them (the document's events, `on_text_inserted`, the caret's
    /// signals) follows as the grant ends (4.9).
    fn grant(&self, mut kind: LockKind) -> windows::core::Result<HRESULT> {
        let _depth = LockDepth::enter();
        let Some(sink) = self
            .inner
            .try_borrow()
            .ok()
            .and_then(|inner| inner.sink.clone())
        else {
            if let Ok(mut inner) = self.inner.try_borrow_mut() {
                inner.locks.release();
            }
            return Err(E_UNEXPECTED.into());
        };
        let mut session = S_OK;
        loop {
            let _ = self.editor(|editor| editor.begin_input_batch());
            // No borrow is held across the grant: the text service reads
            // and writes from inside it.
            let result = unsafe { sink.OnLockGranted(TEXT_STORE_LOCK_FLAGS(kind.flags())) };
            let _ = self.editor(|editor| editor.end_input_batch());
            if session == S_OK {
                session = result.map_or_else(|error| error.code(), |()| S_OK);
            }
            let next = self
                .inner
                .try_borrow_mut()
                .map_err(|_| windows::core::Error::from(E_UNEXPECTED))?
                .locks
                .release();
            match next {
                Some(next) => {
                    tsf_trace!("#{} queued lock granted: {next:?}", next_sequence());
                    kind = next;
                }
                None => break,
            }
        }
        // The tail: what the grants changed that the text services did not
        // write themselves, a message delivered meanwhile among them.
        self.report(false);
        self.grant_deferred(false);
        Ok(session)
    }

    /// Grant the request deferred for changes to report, once they are.
    /// At a `settle`, after its report, it is granted whatever [`dirty`]
    /// says: what could be reported was, and a request that waited any
    /// longer would cost the text service its keys. At the tail of
    /// `RequestLock`, only once nothing is to be reported.
    ///
    /// [`dirty`]: Self::dirty
    fn grant_deferred(&self, settle: bool) {
        if self.dirty() {
            if !settle {
                return;
            }
            tsf_trace!(
                "#{} {} deferred lock granted with changes still unreported",
                next_sequence(),
                self.window_id
            );
        }
        let kind = match self.inner.try_borrow_mut() {
            Ok(mut inner) => inner.locks.take_deferred(),
            Err(_) => return,
        };
        if let Some(kind) = kind {
            tsf_trace!("#{} deferred lock granted: {kind:?}", next_sequence());
            let _ = self.grant(kind);
        }
    }

    /// Run `read` on the field's document, with its UTF-16 index, while it
    /// can be read with no change; `None` otherwise, and the mirror answers.
    fn read_document<T>(
        &self,
        read: impl FnOnce(&dyn TextInputDocument, &Utf16Index) -> Option<T>,
    ) -> Option<T> {
        self.document(|document| {
            if document.is_secure() || !document.is_available() {
                return None;
            }
            let index = document.utf16_index()?;
            read(document, &index)
        })
        .flatten()
    }

    /// The field's selection now, in UTF-16, and whether the caret is at
    /// its start.
    fn selection16(&self) -> Option<(Range<usize>, bool)> {
        self.read_document(|document, index| {
            let selection = document.selection()?;
            let caret = document.caret()?;
            Some((
                index.char_range_to_utf16(selection.clone()),
                caret == selection.start && !selection.is_empty(),
            ))
        })
    }

    /// Note the selection a text service's write left, which it knows.
    fn note_store_selection(&self) {
        let selection = self.selection16();
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.store_selection = selection;
        }
    }

    /// Where `range` (UTF-16) is drawn, window-logical, and whether it was
    /// cut off: its lines from the document, else the caret rectangle at the
    /// last report.
    fn rect_of(&self, range: Range<usize>) -> Option<(Rect, bool)> {
        let measured = self.read_document(|document, index| {
            answers::range_rect(document, index.utf16_range_to_chars(range))
        });
        measured.or_else(|| {
            self.inner
                .try_borrow()
                .ok()?
                .caret
                .map(|caret| (caret, false))
        })
    }

    /// Write `text` over `range` (UTF-16) through the field's write half,
    /// and say what changed, in UTF-16: `SetText` and
    /// `InsertTextAtSelection`. With `dry_run`, nothing changes.
    fn write(
        &self,
        range: Range<usize>,
        text: &[u16],
        options: WriteOptions,
    ) -> windows::core::Result<TextChange> {
        let old = self
            .read_document(|_, index| Some(Rc::new(index.clone())))
            .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
        if range.end > old.utf16_len() || range.start > range.end {
            return Err(TS_E_INVALIDPOS.into());
        }
        let chars = old.utf16_range_to_chars(range);
        let text = String::from_utf16_lossy(text);
        let landed = self
            .editor(|editor| editor.replace_text(chars, &text, options))
            .unwrap_or(Err(WriteRefused::ReadOnly))
            .map_err(refusal)?;
        let start = old.char_to_utf16(landed.replaced.start);
        // A dry run changes nothing to index: the units of what would land.
        let new_end = if options.dry_run {
            start
                + text
                    .chars()
                    .take(landed.inserted.len())
                    .map(char::len_utf16)
                    .sum::<usize>()
        } else {
            self.read_document(|_, index| Some(index.char_to_utf16(landed.inserted.end)))
                .unwrap_or(start)
        };
        Ok(TextChange {
            start,
            old_end: old.char_to_utf16(landed.replaced.end),
            new_end,
        })
    }

    /// The UTF-16 range of `range` (an `ITfRange` the text service hands
    /// over), as characters of the field's text.
    fn chars_of(&self, range: &ITfRange) -> Option<Range<usize>> {
        let (start, len) = acp_extent(range)?;
        self.read_document(|_, index| Some(index.utf16_range_to_chars(start..start + len)))
    }

    /// End the composition the text services hold in this store
    /// (`TerminateComposition`), outside any lock, so that the text service
    /// writes its last text now (a Japanese trailing `n` becomes `ん`).
    /// Whether one was open.
    fn terminate(&self, context: &ITfContext) -> bool {
        if self
            .inner
            .try_borrow()
            .map_or(true, |inner| inner.compositions == 0)
        {
            return false;
        }
        // Its edit session must not wait for changes to report.
        self.report(true);
        let result = context
            .cast::<ITfContextOwnerCompositionServices>()
            .and_then(|services| unsafe { services.TerminateComposition(None) });
        tsf_trace!(
            "#{} {} TerminateComposition {}",
            next_sequence(),
            self.window_id,
            match &result {
                Ok(()) => "ok".to_owned(),
                Err(error) => hresult(error),
            }
        );
        true
    }
}

/// An `ITfRange`'s extent in ACP offsets (start, length).
fn acp_extent(range: &ITfRange) -> Option<(usize, usize)> {
    let acp: ITfRangeACP = range.cast().ok()?;
    let (mut start, mut len) = (0, 0);
    unsafe { acp.GetExtent(&mut start, &mut len) }.ok()?;
    Some((usize::try_from(start).ok()?, usize::try_from(len).ok()?))
}

/// What a refused write answers TSF.
fn refusal(refused: WriteRefused) -> windows::core::Error {
    match refused {
        WriteRefused::InvalidRange => TS_E_INVALIDPOS.into(),
        WriteRefused::Busy => E_FAIL.into(),
        _ => TS_E_READONLY.into(),
    }
}

/// Follow the journal's change since `since` into the mirror: the old
/// offsets through `old`, the text's index at the last report, the new ones
/// through `new`. `Some(None)` for a change the text services made (not
/// reported, 4.10), `None` when the journal cannot say or the result would
/// not match the document, and the whole text is compared.
fn follow_journal(
    document: &dyn TextInputDocument,
    mirror: &mut TextMirror,
    since: u64,
    old: &Utf16Index,
    new: &Utf16Index,
) -> Option<Option<TextChange>> {
    let changed = document.text_changed_since(since)?;
    let start = old.char_to_utf16(changed.start);
    let old_end = old.char_to_utf16(changed.old_end);
    let new_end = new.char_to_utf16(changed.new_end);
    let units: Vec<u16> = document
        .text_in(changed.start..changed.new_end)?
        .encode_utf16()
        .collect();
    let fits = old.utf16_len() == mirror.len()
        && old_end <= mirror.len()
        && start + units.len() == new_end
        && mirror.len() - (old_end - start) + units.len() == new.utf16_len();
    if !fits {
        tsf_trace!("journal {changed:?} does not fit the mirror; compared whole");
        return None;
    }
    let change = mirror.follow(start..old_end, &units)?;
    Some((changed.cause == ChangeCause::Other).then_some(change))
}

fn text_change(change: TextChange) -> TS_TEXTCHANGE {
    TS_TEXTCHANGE {
        acpStart: change.start as i32,
        acpOldEnd: change.old_end as i32,
        acpNewEnd: change.new_end as i32,
    }
}

/// A window-logical rectangle on the screen.
fn screen_rect(hwnd: HWND, rect: Rect) -> RECT {
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let scale = dpi as f32 / 96.0;
    let top_left = to_screen(hwnd, rect.x, rect.y, scale);
    let bottom_right = to_screen(hwnd, rect.right(), rect.bottom(), scale);
    RECT {
        left: top_left.x,
        top: top_left.y,
        right: bottom_right.x,
        bottom: bottom_right.y,
    }
}

/// Run a COM method body: no panic may unwind out of an `extern "system"`
/// function, so a panic answers `E_UNEXPECTED`.
fn guarded<T>(body: impl FnOnce() -> windows::core::Result<T>) -> windows::core::Result<T> {
    catch_unwind(AssertUnwindSafe(body)).unwrap_or_else(|_| Err(E_UNEXPECTED.into()))
}

/// An ACP offset (`i32`, `-1` for the end where allowed) as a mirror
/// offset, checked against `len`.
fn position(acp: i32, len: usize) -> windows::core::Result<usize> {
    usize::try_from(acp)
        .ok()
        .filter(|&offset| offset <= len)
        .ok_or_else(|| TS_E_INVALIDPOS.into())
}

// ---------------------------------------------------------------------------
// The store
// ---------------------------------------------------------------------------

/// `ITextStoreACP` over the mirror, and the composition sink TSF asks the
/// store for.
#[implement(ITextStoreACP, ITfContextOwnerCompositionSink)]
struct TextStore {
    state: Rc<StoreState>,
}

impl TextStore_Impl {
    fn inner(&self) -> windows::core::Result<std::cell::RefMut<'_, StoreInner>> {
        self.state
            .inner
            .try_borrow_mut()
            .map_err(|_| E_UNEXPECTED.into())
    }

    fn readable(&self) -> windows::core::Result<std::cell::RefMut<'_, StoreInner>> {
        let inner = self.inner()?;
        if !inner.locks.can_read() {
            tsf_trace!("read without a lock");
            return Err(TS_E_NOLOCK.into());
        }
        Ok(inner)
    }

    /// The text's length in UTF-16 units: the document's, or the mirror's
    /// while the document cannot be read.
    fn end(&self) -> windows::core::Result<usize> {
        match self.state.read_document(|_, index| Some(index.utf16_len())) {
            Some(len) => Ok(len),
            None => Ok(self.inner()?.mirror.len()),
        }
    }

    /// Whether a write may go through now: a read-write lock is held and
    /// the field takes writes. No borrow is kept: the write goes to the
    /// field.
    fn writable(&self) -> windows::core::Result<()> {
        let inner = self.inner()?;
        if !inner.locks.can_write() {
            tsf_trace!("write without a read-write lock");
            return Err(TS_E_NOLOCK.into());
        }
        if !inner.editable {
            return Err(TS_E_READONLY.into());
        }
        Ok(())
    }
}

/// The units of a text a text service hands over.
fn units_of(text: &PCWSTR, count: u32) -> Vec<u16> {
    if text.is_null() || count == 0 {
        return Vec::new();
    }
    // SAFETY: TSF hands `count` units at `text` for the length of the call.
    unsafe { std::slice::from_raw_parts(text.0, count as usize) }.to_vec()
}

impl ITextStoreACP_Impl for TextStore_Impl {
    fn AdviseSink(
        &self,
        riid: *const GUID,
        punk: Ref<IUnknown>,
        dwmask: u32,
    ) -> windows::core::Result<()> {
        guarded(|| {
            // SAFETY: TSF passes a valid IID.
            if unsafe { *riid } != ITextStoreACPSink::IID {
                return Err(E_INVALIDARG.into());
            }
            let sink: ITextStoreACPSink = punk.ok()?.cast()?;
            let mut inner = self.inner()?;
            if inner.sink.is_some() {
                return Err(CONNECT_E_ADVISELIMIT.into());
            }
            tsf_trace!("#{} AdviseSink mask={dwmask:#x}", next_sequence());
            inner.sink = Some(sink);
            Ok(())
        })
    }

    fn UnadviseSink(&self, _punk: Ref<IUnknown>) -> windows::core::Result<()> {
        guarded(|| {
            let mut inner = self.inner()?;
            if inner.sink.take().is_none() {
                return Err(CONNECT_E_CANNOTCONNECT.into());
            }
            tsf_trace!("#{} UnadviseSink", next_sequence());
            Ok(())
        })
    }

    fn RequestLock(&self, dwlockflags: u32) -> windows::core::Result<HRESULT> {
        guarded(|| {
            // Rule 1 of 4.7: with changes the text services have not heard
            // of, nothing is granted; they are reported first.
            let held = self.inner()?.locks.held().is_some();
            let dirty = !held && self.state.dirty();
            let decision = {
                let mut inner = self.inner()?;
                if dirty {
                    inner.locks.defer(dwlockflags)
                } else {
                    inner.locks.request(dwlockflags)
                }
            };
            tsf_trace!(
                "#{} RequestLock flags={dwlockflags:#x}{} -> {decision:?}",
                next_sequence(),
                if dirty { " (changes to report)" } else { "" }
            );
            match decision {
                LockDecision::Refused => Ok(TS_E_SYNCHRONOUS),
                LockDecision::Queued => {
                    if dirty {
                        self.state.service.post("a lock request waits for a report");
                    }
                    Ok(TS_S_ASYNC)
                }
                LockDecision::Grant(kind) => self.state.grant(kind),
            }
        })
    }

    fn GetStatus(&self) -> windows::core::Result<TS_STATUS> {
        guarded(|| {
            let editable = self.inner()?.editable;
            let mut dynamic = TS_SD_INPUTPANEMANUALDISPLAYENABLE;
            if !editable {
                dynamic |= TS_SD_READONLY;
            }
            Ok(TS_STATUS {
                dwDynamicFlags: dynamic,
                dwStaticFlags: TS_SS_NOHIDDENTEXT,
            })
        })
    }

    fn QueryInsert(
        &self,
        acpteststart: i32,
        acptestend: i32,
        cch: u32,
        pacpresultstart: *mut i32,
        pacpresultend: *mut i32,
    ) -> windows::core::Result<()> {
        guarded(|| {
            let len = self.end()?;
            let start = position(acpteststart, len)?;
            let end = position(acptestend, len)?;
            tsf_trace!("#{} QueryInsert {start}..{end} cch={cch}", next_sequence());
            // SAFETY: TSF passes valid out pointers.
            unsafe {
                *pacpresultstart = start as i32;
                *pacpresultend = end as i32;
            }
            Ok(())
        })
    }

    fn GetSelection(
        &self,
        ulindex: u32,
        ulcount: u32,
        pselection: *mut TS_SELECTION_ACP,
        pcfetched: *mut u32,
    ) -> windows::core::Result<()> {
        guarded(|| {
            let mirrored = {
                let inner = self.readable()?;
                (inner.mirror.selection(), inner.mirror.caret_at_start())
            };
            if ulindex != 0 && ulindex != TF_DEFAULT_SELECTION {
                return Err(E_INVALIDARG.into());
            }
            let from_document = self.state.selection16();
            let source = if from_document.is_some() {
                "document"
            } else {
                "mirror"
            };
            let (selection, caret_at_start) = from_document.unwrap_or(mirrored);
            let fetched = u32::from(ulcount > 0);
            if fetched == 1 {
                // SAFETY: TSF passes room for `ulcount` selections.
                unsafe {
                    *pselection = TS_SELECTION_ACP {
                        acpStart: selection.start as i32,
                        acpEnd: selection.end as i32,
                        style: TS_SELECTIONSTYLE {
                            ase: if caret_at_start {
                                TS_AE_START
                            } else {
                                TS_AE_END
                            },
                            fInterimChar: false.into(),
                        },
                    };
                }
            }
            // SAFETY: as above.
            unsafe { *pcfetched = fetched };
            tsf_trace!(
                "#{} GetSelection -> {selection:?} from the {source}",
                next_sequence()
            );
            Ok(())
        })
    }

    fn SetSelection(
        &self,
        ulcount: u32,
        pselection: *const TS_SELECTION_ACP,
    ) -> windows::core::Result<()> {
        guarded(|| {
            self.writable()?;
            if ulcount == 0 {
                return Ok(());
            }
            // SAFETY: TSF passes `ulcount` selections.
            let selection = unsafe { *pselection };
            let len = self.end()?;
            let range = position(selection.acpStart, len)?..position(selection.acpEnd, len)?;
            if range.end < range.start {
                return Err(TS_E_INVALIDPOS.into());
            }
            let chars = self
                .state
                .read_document(|_, index| Some(index.utf16_range_to_chars(range.clone())))
                .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
            let caret_at_start = selection.style.ase == TS_AE_START;
            let result = self
                .state
                .editor(|editor| editor.set_selection(chars.clone(), caret_at_start))
                .unwrap_or(Err(WriteRefused::ReadOnly));
            tsf_trace!(
                "#{} SetSelection {range:?} (characters {chars:?}) -> {result:?}",
                next_sequence()
            );
            result.map_err(refusal)?;
            self.state.note_store_selection();
            Ok(())
        })
    }

    fn GetText(
        &self,
        acpstart: i32,
        acpend: i32,
        pchplain: PWSTR,
        cchplainreq: u32,
        pcchplainret: *mut u32,
        prgruninfo: *mut TS_RUNINFO,
        cruninforeq: u32,
        pcruninforet: *mut u32,
        pacpnext: *mut i32,
    ) -> windows::core::Result<()> {
        guarded(|| {
            drop(self.readable()?);
            let len = self.end()?;
            let start = position(acpstart, len)?;
            let end = if acpend == -1 {
                len
            } else {
                position(acpend, len)?
            };
            if end < start {
                return Err(TS_E_INVALIDPOS.into());
            }
            let wanted = start..start + (end - start).min(cchplainreq as usize);
            // From the document, its characters widened to whole ones and
            // cut back to the units asked (4.8); from the mirror while the
            // document cannot be read.
            let from_document = self.state.read_document(|document, index| {
                let chars = index.utf16_range_to_chars(wanted.clone());
                let first = index.char_to_utf16(chars.start);
                let units: Vec<u16> = document.text_in(chars)?.encode_utf16().collect();
                let skip = wanted.start.checked_sub(first)?;
                units.get(skip..skip + wanted.len()).map(<[u16]>::to_vec)
            });
            let source = if from_document.is_some() {
                "document"
            } else {
                "mirror"
            };
            let units = match from_document {
                Some(units) => units,
                None => self.inner()?.mirror.get(wanted.clone()).to_vec(),
            };
            if !pchplain.is_null() && !units.is_empty() {
                // SAFETY: TSF passes room for `cchplainreq` units.
                unsafe { std::ptr::copy_nonoverlapping(units.as_ptr(), pchplain.0, units.len()) };
            }
            // SAFETY: TSF passes valid out pointers, and room for
            // `cruninforeq` runs.
            unsafe {
                *pcchplainret = units.len() as u32;
                if cruninforeq > 0 && !prgruninfo.is_null() && !units.is_empty() {
                    *prgruninfo = TS_RUNINFO {
                        uCount: units.len() as u32,
                        r#type: TS_RT_PLAIN,
                    };
                    *pcruninforet = 1;
                } else {
                    *pcruninforet = 0;
                }
                *pacpnext = (start + units.len()) as i32;
            }
            tsf_trace!(
                "#{} GetText {start}..{end} (asked {cchplainreq} units) -> {} units from the {source}",
                next_sequence(),
                units.len()
            );
            Ok(())
        })
    }

    fn SetText(
        &self,
        dwflags: u32,
        acpstart: i32,
        acpend: i32,
        pchtext: &PCWSTR,
        cch: u32,
    ) -> windows::core::Result<TS_TEXTCHANGE> {
        guarded(|| {
            self.writable()?;
            let len = self.end()?;
            let range = position(acpstart, len)?..position(acpend, len)?;
            let text = units_of(pchtext, cch);
            let options = WriteOptions {
                correction: dwflags & TS_ST_CORRECTION.0 != 0,
                ..WriteOptions::default()
            };
            let change = self.state.write(range.clone(), &text, options);
            tsf_trace!(
                "#{} SetText flags={dwflags:#x} {range:?} <- {:?} -> {change:?}",
                next_sequence(),
                String::from_utf16_lossy(&text),
            );
            let change = change?;
            self.state.note_store_selection();
            Ok(text_change(change))
        })
    }

    fn GetFormattedText(&self, _acpstart: i32, _acpend: i32) -> windows::core::Result<IDataObject> {
        Err(E_NOTIMPL.into())
    }

    fn GetEmbedded(
        &self,
        _acppos: i32,
        _rguidservice: *const GUID,
        _riid: *const GUID,
    ) -> windows::core::Result<IUnknown> {
        Err(E_NOTIMPL.into())
    }

    fn QueryInsertEmbedded(
        &self,
        _pguidservice: *const GUID,
        _pformatetc: *const FORMATETC,
    ) -> windows::core::Result<BOOL> {
        Ok(false.into())
    }

    fn InsertEmbedded(
        &self,
        _dwflags: u32,
        _acpstart: i32,
        _acpend: i32,
        _pdataobject: Ref<IDataObject>,
    ) -> windows::core::Result<TS_TEXTCHANGE> {
        Err(E_NOTIMPL.into())
    }

    fn InsertTextAtSelection(
        &self,
        dwflags: u32,
        pchtext: &PCWSTR,
        cch: u32,
        pacpstart: *mut i32,
        pacpend: *mut i32,
        pchange: *mut TS_TEXTCHANGE,
    ) -> windows::core::Result<()> {
        guarded(|| {
            let query_only = dwflags & TS_IAS_QUERYONLY != 0;
            if query_only {
                drop(self.readable()?);
            } else {
                self.writable()?;
            }
            let text = units_of(pchtext, cch);
            let (selection, _) = self
                .state
                .selection16()
                .ok_or_else(|| windows::core::Error::from(E_FAIL))?;
            let options = WriteOptions {
                dry_run: query_only,
                ..WriteOptions::default()
            };
            let change = self.state.write(selection, &text, options);
            tsf_trace!(
                "#{} InsertTextAtSelection flags={dwflags:#x} {:?} -> {change:?}",
                next_sequence(),
                String::from_utf16_lossy(&text),
            );
            let change = change?;
            if !query_only {
                self.state.note_store_selection();
            }
            // SAFETY: TSF passes valid out pointers, or null for those it
            // does not want.
            unsafe {
                if !pacpstart.is_null() {
                    *pacpstart = change.start as i32;
                }
                if !pacpend.is_null() {
                    *pacpend = change.new_end as i32;
                }
                if !pchange.is_null() && !query_only {
                    *pchange = text_change(change);
                }
            }
            Ok(())
        })
    }

    fn InsertEmbeddedAtSelection(
        &self,
        _dwflags: u32,
        _pdataobject: Ref<IDataObject>,
        _pacpstart: *mut i32,
        _pacpend: *mut i32,
        _pchange: *mut TS_TEXTCHANGE,
    ) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn RequestSupportedAttrs(
        &self,
        dwflags: u32,
        cfilterattrs: u32,
        pafilterattrs: *const GUID,
    ) -> windows::core::Result<()> {
        trace_attrs(
            "RequestSupportedAttrs",
            dwflags,
            cfilterattrs,
            pafilterattrs,
        );
        Ok(())
    }

    fn RequestAttrsAtPosition(
        &self,
        acppos: i32,
        cfilterattrs: u32,
        pafilterattrs: *const GUID,
        dwflags: u32,
    ) -> windows::core::Result<()> {
        trace_attrs(
            &format!("RequestAttrsAtPosition {acppos}"),
            dwflags,
            cfilterattrs,
            pafilterattrs,
        );
        Ok(())
    }

    fn RequestAttrsTransitioningAtPosition(
        &self,
        acppos: i32,
        cfilterattrs: u32,
        pafilterattrs: *const GUID,
        dwflags: u32,
    ) -> windows::core::Result<()> {
        trace_attrs(
            &format!("RequestAttrsTransitioningAtPosition {acppos}"),
            dwflags,
            cfilterattrs,
            pafilterattrs,
        );
        Ok(())
    }

    fn FindNextAttrTransition(
        &self,
        _acpstart: i32,
        acphalt: i32,
        _cfilterattrs: u32,
        _pafilterattrs: *const GUID,
        _dwflags: u32,
        pacpnext: *mut i32,
        pffound: *mut BOOL,
        plfoundoffset: *mut i32,
    ) -> windows::core::Result<()> {
        // SAFETY: TSF passes valid out pointers.
        unsafe {
            *pacpnext = acphalt;
            *pffound = false.into();
            *plfoundoffset = 0;
        }
        Ok(())
    }

    fn RetrieveRequestedAttrs(
        &self,
        _ulcount: u32,
        _paattrvals: *mut TS_ATTRVAL,
        pcfetched: *mut u32,
    ) -> windows::core::Result<()> {
        // SAFETY: TSF passes a valid out pointer.
        unsafe { *pcfetched = 0 };
        Ok(())
    }

    fn GetEndACP(&self) -> windows::core::Result<i32> {
        guarded(|| {
            drop(self.readable()?);
            Ok(self.end()? as i32)
        })
    }

    fn GetActiveView(&self) -> windows::core::Result<u32> {
        Ok(VIEW)
    }

    fn GetACPFromPoint(
        &self,
        _vcview: u32,
        ptscreen: *const POINT,
        dwflags: u32,
    ) -> windows::core::Result<i32> {
        guarded(|| {
            drop(self.readable()?);
            // SAFETY: TSF passes a valid point.
            let mut point = unsafe { *ptscreen };
            unsafe {
                let _ = ScreenToClient(self.state.hwnd, &mut point);
            }
            let scale = unsafe { GetDpiForWindow(self.state.hwnd) }.max(96) as f32 / 96.0;
            let logical = Point::new(point.x as f32 / scale, point.y as f32 / scale);
            let offset = self
                .state
                .document(|document| {
                    if !document.is_available() {
                        return None;
                    }
                    answers::character_index(document, logical)
                })
                .flatten()
                .filter(|&offset| offset < answers::NOT_FOUND);
            tsf_trace!(
                "#{} GetACPFromPoint {logical:?} flags={dwflags:#x} -> {offset:?}",
                next_sequence()
            );
            offset
                .map(|offset| offset as i32)
                .ok_or_else(|| TS_E_INVALIDPOINT.into())
        })
    }

    fn GetTextExt(
        &self,
        _vcview: u32,
        acpstart: i32,
        acpend: i32,
        prc: *mut RECT,
        pfclipped: *mut BOOL,
    ) -> windows::core::Result<()> {
        guarded(|| {
            drop(self.readable()?);
            let len = self.end()?;
            let range = position(acpstart, len)?..position(acpend, len)?;
            if range.end < range.start {
                return Err(TS_E_INVALIDPOS.into());
            }
            let measured = self.state.rect_of(range.clone());
            let screen = measured
                .map(|(rect, _)| screen_rect(self.state.hwnd, rect))
                .unwrap_or_default();
            tsf_trace!(
                "#{} GetTextExt {range:?} -> {measured:?} screen ({}, {})..({}, {})",
                next_sequence(),
                screen.left,
                screen.top,
                screen.right,
                screen.bottom
            );
            // SAFETY: TSF passes valid out pointers.
            unsafe {
                *prc = screen;
                *pfclipped = measured.is_some_and(|(_, clipped)| clipped).into();
            }
            Ok(())
        })
    }

    fn GetScreenExt(&self, _vcview: u32) -> windows::core::Result<RECT> {
        guarded(|| {
            let visible = self
                .state
                .document(|document| document.visible_text_rect())
                .flatten()
                .or(self.inner()?.visible);
            Ok(visible
                .map(|rect| screen_rect(self.state.hwnd, rect))
                .unwrap_or_default())
        })
    }

    fn GetWnd(&self, _vcview: u32) -> windows::core::Result<HWND> {
        Ok(self.state.hwnd)
    }
}

/// Trace the attributes a text service asks for (`GUID_PROP_INPUTSCOPE`
/// among them, which the store does not answer yet).
fn trace_attrs(what: &str, flags: u32, count: u32, attrs: *const GUID) {
    if !super::super::trace_enabled() {
        return;
    }
    let attrs = if attrs.is_null() || count == 0 {
        &[][..]
    } else {
        // SAFETY: TSF passes `count` GUIDs.
        unsafe { std::slice::from_raw_parts(attrs, count as usize) }
    };
    tsf_trace!(
        "#{} {what} flags={flags:#x} attrs={attrs:?}",
        next_sequence()
    );
}

impl ITfContextOwnerCompositionSink_Impl for TextStore_Impl {
    fn OnStartComposition(
        &self,
        pcomposition: Ref<ITfCompositionView>,
    ) -> windows::core::Result<BOOL> {
        guarded(|| {
            let view = pcomposition.ok().ok();
            // A read-only field takes no composition (section 6).
            let editable = self.inner()?.editable;
            // The composition is the text service's range: empty at the
            // caret, or the text a reconversion takes up.
            let range = view
                .and_then(|view| unsafe { view.GetRange() }.ok())
                .and_then(|range| self.state.chars_of(&range));
            let accepted = editable
                && range.clone().is_some_and(|range| {
                    self.state
                        .editor(|editor| editor.mark_composition(range, &[]))
                        .is_some_and(|marked| marked.is_ok())
                });
            if accepted {
                self.inner()?.compositions += 1;
            }
            tsf_trace!(
                "#{} OnStartComposition owner={} characters {range:?} -> {}",
                next_sequence(),
                owner(view),
                if accepted {
                    "accepted"
                } else if editable {
                    "refused (the field took no composition)"
                } else {
                    "refused (read-only)"
                }
            );
            Ok(accepted.into())
        })
    }

    fn OnUpdateComposition(
        &self,
        pcomposition: Ref<ITfCompositionView>,
        _prangenew: Ref<ITfRange>,
    ) -> windows::core::Result<()> {
        tsf_trace!(
            "#{} OnUpdateComposition owner={}",
            next_sequence(),
            owner(pcomposition.ok().ok())
        );
        Ok(())
    }

    fn OnEndComposition(&self, pcomposition: Ref<ITfCompositionView>) -> windows::core::Result<()> {
        guarded(|| {
            {
                let mut inner = self.inner()?;
                inner.compositions = inner.compositions.saturating_sub(1);
            }
            // The text the text service left is the commit: one undo step,
            // through the field's rules (4.9).
            let ended = self
                .state
                .editor(|editor| editor.end_composition(ImeOrigin::Composition));
            self.state.note_store_selection();
            tsf_trace!(
                "#{} OnEndComposition owner={} -> {ended:?}",
                next_sequence(),
                owner(pcomposition.ok().ok()),
            );
            Ok(())
        })
    }
}

/// A composition's owner, as a GUID (M4).
fn owner(view: Option<&ITfCompositionView>) -> String {
    view.and_then(|view| unsafe { view.GetOwnerClsid() }.ok())
        .map_or_else(|| "?".to_owned(), |guid| format!("{guid:?}"))
}

// ---------------------------------------------------------------------------
// The edit sink
// ---------------------------------------------------------------------------

/// Logs every edit a text service makes in the context, with the text and
/// every property on each changed range (M4, M5).
#[implement(ITfTextEditSink)]
struct EditSink {
    state: Rc<StoreState>,
}

impl ITfTextEditSink_Impl for EditSink_Impl {
    fn OnEndEdit(
        &self,
        pic: Ref<ITfContext>,
        ecreadonly: u32,
        peditrecord: Ref<ITfEditRecord>,
    ) -> windows::core::Result<()> {
        guarded(|| {
            let context = pic.ok()?;
            // The clauses of the composition, for drawing it (display
            // attributes, `GUID_PROP_ATTRIBUTE`).
            if self
                .state
                .inner
                .try_borrow()
                .is_ok_and(|inner| inner.compositions > 0)
            {
                self.mark_clauses(context, ecreadonly);
            }
            if !super::super::trace_enabled() {
                return Ok(());
            }
            let record = peditrecord.ok()?;
            let seq = next_sequence();
            let selection_changed =
                unsafe { record.GetSelectionStatus() }.is_ok_and(|changed| changed.as_bool());
            tsf_trace!(
                "#{seq} {} OnEndEdit selection changed={selection_changed}",
                self.state.window_id
            );
            let ranges = unsafe { record.GetTextAndPropertyUpdates(TF_GTP_INCL_TEXT, &[]) }?;
            let properties = all_properties(context);
            for _ in 0..64 {
                let mut slot = [None];
                let mut fetched = 0;
                if unsafe { ranges.Next(&mut slot, &mut fetched) }.is_err() || fetched == 0 {
                    break;
                }
                let Some(range) = slot[0].take() else {
                    break;
                };
                let text = range_text(&range, ecreadonly);
                tsf_trace!("#{seq}   changed {text:?}");
                for property in &properties {
                    let Ok(kind) = (unsafe { property.GetType() }) else {
                        continue;
                    };
                    let Ok(mut value) = (unsafe { property.GetValue(ecreadonly, &range) }) else {
                        continue;
                    };
                    let empty = unsafe { value.Anonymous.Anonymous.vt } == VT_EMPTY;
                    let described = describe_variant(&mut value);
                    if !empty {
                        tsf_trace!("#{seq}     {} = {described}", property_name(&kind));
                    }
                }
            }
            Ok(())
        })
    }
}

impl EditSink_Impl {
    /// Hand the field the clauses of the context's composition, each styled
    /// by its display attribute (`mark_composition`).
    fn mark_clauses(&self, context: &ITfContext, cookie: u32) {
        let Some((composition, clauses)) = composition_clauses(context, cookie) else {
            return;
        };
        let marked = self.state.read_document(|_, index| {
            let chars = |range: Range<usize>| index.utf16_range_to_chars(range);
            let segments: Vec<CompositionSegment> = clauses
                .iter()
                .map(|(range, style)| CompositionSegment {
                    range: chars(range.clone()),
                    style: *style,
                })
                .collect();
            Some((chars(composition.clone()), segments))
        });
        let Some((range, segments)) = marked else {
            return;
        };
        let result = self
            .state
            .editor(|editor| editor.mark_composition(range.clone(), &segments));
        tsf_trace!(
            "#{} composition {range:?} clauses {:?} -> {result:?}",
            next_sequence(),
            segments
                .iter()
                .map(|segment| (segment.range.clone(), segment.style))
                .collect::<Vec<_>>()
        );
    }
}

/// A clause of a composition: its UTF-16 range and style.
type Clause = (Range<usize>, SegmentStyle);

/// The first composition in `context` (UTF-16), and its clauses: the runs
/// of `GUID_PROP_ATTRIBUTE` inside it, each with the style its display
/// attribute names.
fn composition_clauses(context: &ITfContext, cookie: u32) -> Option<(Range<usize>, Vec<Clause>)> {
    let compositions: ITfContextComposition = context.cast().ok()?;
    let views = unsafe { compositions.EnumCompositions() }.ok()?;
    let mut slot = [None];
    let mut fetched = 0;
    unsafe { views.Next(&mut slot, &mut fetched) }.ok()?;
    let view: ITfCompositionView = slot[0].take()?;
    let range = unsafe { view.GetRange() }.ok()?;
    let (start, len) = acp_extent(&range)?;
    let composition = start..start + len;
    let mut clauses = Vec::new();
    let property = unsafe { context.GetProperty(&GUID_PROP_ATTRIBUTE) }.ok()?;
    let mut runs = None;
    unsafe { property.EnumRanges(cookie, &mut runs, &range) }.ok()?;
    let runs = runs?;
    for _ in 0..256 {
        let mut slot = [None];
        let mut fetched = 0;
        if unsafe { runs.Next(&mut slot, &mut fetched) }.is_err() || fetched == 0 {
            break;
        }
        let Some(run) = slot[0].take() else {
            break;
        };
        let Some((run_start, run_len)) = acp_extent(&run) else {
            continue;
        };
        let Ok(value) = (unsafe { property.GetValue(cookie, &run) }) else {
            continue;
        };
        let Some(atom) = int_of(&value) else {
            continue;
        };
        let start = run_start.clamp(composition.start, composition.end);
        let end = (run_start + run_len).clamp(start, composition.end);
        if start < end {
            clauses.push((start..end, display_style(atom as u32)));
        }
    }
    Some((composition, clauses))
}

thread_local! {
    /// The thread's category and display attribute managers, made at the
    /// first clause read, and each atom's style as they named it.
    static DISPLAY: RefCell<Option<DisplayAttributes>> = const { RefCell::new(None) };
}

/// What reads a display attribute.
struct DisplayAttributes {
    categories: Option<ITfCategoryMgr>,
    attributes: Option<ITfDisplayAttributeMgr>,
    styles: HashMap<u32, SegmentStyle>,
}

/// The style of the display attribute `atom` names (a `TfGuidAtom` of
/// `GUID_PROP_ATTRIBUTE`): its `bAttr`. Typed input when it cannot be read.
fn display_style(atom: u32) -> SegmentStyle {
    DISPLAY
        .try_with(|display| {
            let mut display = display.try_borrow_mut().ok()?;
            let display = display.get_or_insert_with(|| DisplayAttributes {
                categories: unsafe {
                    CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)
                }
                .ok(),
                attributes: unsafe {
                    CoCreateInstance(&CLSID_TF_DisplayAttributeMgr, None, CLSCTX_INPROC_SERVER)
                }
                .ok(),
                styles: HashMap::new(),
            });
            if let Some(style) = display.styles.get(&atom) {
                return Some(*style);
            }
            let guid = unsafe { display.categories.as_ref()?.GetGUID(atom) }.ok()?;
            let mut info = None;
            let mut owner = GUID::zeroed();
            unsafe {
                display
                    .attributes
                    .as_ref()?
                    .GetDisplayAttributeInfo(&guid, &mut info, &mut owner)
            }
            .ok()?;
            let mut attribute = TF_DISPLAYATTRIBUTE::default();
            unsafe { info?.GetAttributeInfo(&mut attribute) }.ok()?;
            let style = style_of(attribute.bAttr);
            display.styles.insert(atom, style);
            Some(style)
        })
        .ok()
        .flatten()
        .unwrap_or(SegmentStyle::Input)
}

/// A display attribute's meaning as a clause style.
fn style_of(attribute: TF_DA_ATTR_INFO) -> SegmentStyle {
    match attribute {
        TF_ATTR_TARGET_CONVERTED => SegmentStyle::Target,
        TF_ATTR_TARGET_NOTCONVERTED => SegmentStyle::TargetInput,
        TF_ATTR_CONVERTED | TF_ATTR_FIXEDCONVERTED => SegmentStyle::Converted,
        _ => SegmentStyle::Input,
    }
}

/// Every property the context knows (`EnumProperties`).
fn all_properties(context: &ITfContext) -> Vec<ITfProperty> {
    let mut properties = Vec::new();
    let Ok(all) = (unsafe { context.EnumProperties() }) else {
        return properties;
    };
    for _ in 0..128 {
        let mut slot = [None];
        let mut fetched = 0;
        if unsafe { all.Next(&mut slot, &mut fetched) }.is_err() || fetched == 0 {
            break;
        }
        if let Some(property) = slot[0].take() {
            properties.push(property);
        }
    }
    properties
}

/// The text of a range, at most 256 units.
fn range_text(range: &ITfRange, cookie: u32) -> String {
    let mut buffer = [0u16; 256];
    let mut count = 0;
    match unsafe { range.GetText(cookie, 0, &mut buffer, &mut count) } {
        Ok(()) => String::from_utf16_lossy(&buffer[..(count as usize).min(buffer.len())]),
        Err(error) => format!("<{}>", hresult(&error)),
    }
}

/// A property's name where TSF documents it, its GUID otherwise.
fn property_name(guid: &GUID) -> String {
    use windows::Win32::UI::TextServices::{
        GUID_PROP_ATTRIBUTE, GUID_PROP_COMPOSING, GUID_PROP_INPUTSCOPE, GUID_PROP_LANGID,
        GUID_PROP_READING, GUID_PROP_TEXTOWNER, GUID_PROP_TKB_ALTERNATES,
    };
    let known = [
        (GUID_PROP_TEXTOWNER, "TEXTOWNER"),
        (GUID_PROP_COMPOSING, "COMPOSING"),
        (GUID_PROP_ATTRIBUTE, "ATTRIBUTE"),
        (GUID_PROP_READING, "READING"),
        (GUID_PROP_LANGID, "LANGID"),
        (GUID_PROP_INPUTSCOPE, "INPUTSCOPE"),
        (GUID_PROP_TKB_ALTERNATES, "TKB_ALTERNATES"),
    ];
    known
        .iter()
        .find(|(known, _)| known == guid)
        .map_or_else(|| format!("{guid:?}"), |(_, name)| (*name).to_owned())
}

#[cfg(test)]
mod tests {
    //! The store against its own `ITextStoreACP` interface, with a sink of
    //! the test's standing in for TSF: no thread manager, no text service,
    //! so it runs in session 0.

    use std::sync::atomic::{AtomicUsize, Ordering};

    use teksilo_canvas::SizeProposal;
    use teksilo_core::text_input_document::{FirstLineRect, Landed, TextChanged};
    use teksilo_core::text_surface::TextSurface;
    use teksilo_core::{
        BuildContext, EventContext, HandlerSet, LayoutContext, LayoutResponse, SubscriptionId,
        Widget, WidgetTree,
    };
    use windows::Win32::UI::TextServices::{ITextStoreACPSink_Impl, TsLayoutCode};

    use super::super::super::tsf_lock::{LF_READ, LF_READWRITE, LF_SYNC};
    use super::*;

    /// One line of text, laid out 10 wide per character. With `journal`,
    /// it counts revisions and keeps its changes as the widgets do.
    #[derive(Debug)]
    struct Doc {
        text: RefCell<String>,
        selection: RefCell<Range<usize>>,
        editable: Cell<bool>,
        /// Typing held for the frame: not available until flushed.
        pending: RefCell<String>,
        journal: bool,
        revisions: Cell<InputRevisions>,
        changes: RefCell<Vec<(u64, TextChanged)>>,
        /// Every call of its write half, in order.
        written: RefCell<Vec<String>>,
        /// The composition the input system marked, if one is open.
        composition: RefCell<Option<Range<usize>>>,
        /// Whether its journal merges several changes.
        merges: Cell<bool>,
    }

    /// The widgets' merge (`common::input_journal::merge`).
    fn merge(first: TextChanged, then: TextChanged) -> TextChanged {
        let then_old_end = if then.old_end >= first.new_end {
            then.old_end - first.new_end + first.old_end
        } else {
            first.old_end
        };
        let first_new_end = if first.new_end >= then.old_end {
            first.new_end + then.new_end - then.old_end
        } else {
            then.new_end
        };
        TextChanged {
            start: first.start.min(then.start),
            old_end: first.old_end.max(then_old_end),
            new_end: first_new_end.max(then.new_end),
            cause: if first.cause == ChangeCause::InputSystem
                && then.cause == ChangeCause::InputSystem
            {
                ChangeCause::InputSystem
            } else {
                ChangeCause::Other
            },
        }
    }

    impl Doc {
        fn new(text: &str, caret: usize, journal: bool) -> Self {
            Self {
                text: RefCell::new(text.to_owned()),
                selection: RefCell::new(caret..caret),
                editable: Cell::new(true),
                pending: RefCell::new(String::new()),
                journal,
                revisions: Cell::new(InputRevisions {
                    text: 0,
                    selection: 0,
                    layout: 0,
                }),
                changes: RefCell::new(Vec::new()),
                written: RefCell::new(Vec::new()),
                composition: RefCell::new(None),
                merges: Cell::new(true),
            }
        }

        fn written(&self) -> Vec<String> {
            self.written.borrow().clone()
        }

        fn len(&self) -> usize {
            self.text.borrow().chars().count()
        }

        /// Replace `range` (characters) with `text`, the caret after it, as
        /// `cause`.
        fn edit(&self, range: Range<usize>, text: &str, cause: ChangeCause) {
            let mut chars: Vec<char> = self.text.borrow().chars().collect();
            chars.splice(range.clone(), text.chars());
            *self.text.borrow_mut() = chars.into_iter().collect();
            let end = range.start + text.chars().count();
            *self.selection.borrow_mut() = end..end;
            let mut revisions = self.revisions.get();
            revisions.text += 1;
            revisions.selection += 1;
            revisions.layout += 1;
            self.revisions.set(revisions);
            self.changes.borrow_mut().push((
                revisions.text,
                TextChanged {
                    start: range.start,
                    old_end: range.end,
                    new_end: end,
                    cause,
                },
            ));
        }
    }

    impl Doc {
        /// Move the field's own selection, as a key it handles does.
        fn select(&self, range: Range<usize>) {
            *self.selection.borrow_mut() = range;
            let mut revisions = self.revisions.get();
            revisions.selection += 1;
            self.revisions.set(revisions);
        }
    }

    impl TextInputDocument for Doc {
        fn is_available(&self) -> bool {
            self.pending.borrow().is_empty()
        }
        fn flush_pending_input(&self) -> bool {
            let typed = std::mem::take(&mut *self.pending.borrow_mut());
            if typed.is_empty() {
                return false;
            }
            let caret = self.selection.borrow().end;
            self.edit(caret..caret, &typed, ChangeCause::Other);
            true
        }
        fn revisions(&self) -> Option<InputRevisions> {
            self.journal.then(|| self.revisions.get())
        }
        /// One change since `revision`; two or more are merged as the
        /// widgets' journal does, unless `merges` is off, when the journal
        /// cannot say.
        fn text_changed_since(&self, revision: u64) -> Option<TextChanged> {
            let changes = self.changes.borrow();
            let after: Vec<TextChanged> = changes
                .iter()
                .filter(|(at, _)| *at > revision)
                .map(|(_, change)| *change)
                .collect();
            match after.len() {
                0 => None,
                1 => Some(after[0]),
                _ if self.merges.get() => after.into_iter().reduce(merge),
                _ => None,
            }
        }
        fn char_count(&self) -> Option<usize> {
            Some(self.len())
        }
        fn text_in(&self, range: Range<usize>) -> Option<String> {
            let end = range.end.min(self.len());
            let start = range.start.min(end);
            Some(
                self.text
                    .borrow()
                    .chars()
                    .skip(start)
                    .take(end - start)
                    .collect(),
            )
        }
        fn selection(&self) -> Option<Range<usize>> {
            Some(self.selection.borrow().clone())
        }
        fn caret(&self) -> Option<usize> {
            Some(self.selection.borrow().end)
        }
        fn composition_range(&self) -> Option<Range<usize>> {
            None
        }
        fn first_line_rect(&self, range: Range<usize>) -> Option<FirstLineRect> {
            Some(FirstLineRect {
                rect: Rect::new(
                    range.start as f32 * 10.0,
                    0.0,
                    (range.len() as f32 * 10.0).max(1.0),
                    20.0,
                ),
                covered: range,
            })
        }
        fn offset_at_point(&self, _: Point) -> Option<usize> {
            None
        }
        fn visible_text_rect(&self) -> Option<Rect> {
            Some(Rect::new(0.0, 0.0, 300.0, 20.0))
        }
        fn is_editable(&self) -> bool {
            self.editable.get()
        }
        fn is_secure(&self) -> bool {
            false
        }
        fn writing_tools_allowed(&self) -> bool {
            false
        }
        fn utf16_index(&self) -> Option<Rc<Utf16Index>> {
            Some(Rc::new(Utf16Index::new(&self.text.borrow())))
        }
    }

    /// A write half that writes what it is given: no rules, no undo.
    impl TextInputEditor for Doc {
        fn replace_text(
            &self,
            range: Range<usize>,
            text: &str,
            options: WriteOptions,
        ) -> Result<Landed, WriteRefused> {
            if range.end > self.len() {
                return Err(WriteRefused::InvalidRange);
            }
            let count = text.chars().count();
            let landed = Landed {
                inserted: range.start..range.start + count,
                replaced: range.clone(),
            };
            if !options.dry_run {
                self.written
                    .borrow_mut()
                    .push(format!("replace {range:?} {text:?}"));
                self.edit(range, text, ChangeCause::InputSystem);
            }
            Ok(landed)
        }
        fn set_selection(
            &self,
            selection: Range<usize>,
            _caret_at_start: bool,
        ) -> Result<(), WriteRefused> {
            self.written
                .borrow_mut()
                .push(format!("select {selection:?}"));
            self.select(selection);
            Ok(())
        }
        fn mark_composition(
            &self,
            range: Range<usize>,
            segments: &[CompositionSegment],
        ) -> Result<(), WriteRefused> {
            self.written
                .borrow_mut()
                .push(format!("mark {range:?} {}", segments.len()));
            *self.composition.borrow_mut() = Some(range);
            Ok(())
        }
        fn end_composition(&self, origin: ImeOrigin) -> Result<(), WriteRefused> {
            self.written.borrow_mut().push(format!("end {origin:?}"));
            *self.composition.borrow_mut() = None;
            Ok(())
        }
        fn begin_input_batch(&self) {
            self.written.borrow_mut().push("begin".to_owned());
        }
        fn end_input_batch(&self) {
            self.written.borrow_mut().push("end batch".to_owned());
        }
    }

    impl TextSurface for Doc {
        fn can_undo(&self) -> bool {
            false
        }
        fn can_redo(&self) -> bool {
            false
        }
        fn undo(&self) {}
        fn redo(&self) {}
        fn has_selection(&self) -> bool {
            false
        }
        fn is_read_only(&self) -> bool {
            !self.editable.get()
        }
        fn allows_copy(&self) -> bool {
            true
        }
        fn cut(&self, _: &EventContext<'_>) {}
        fn copy(&self, _: &EventContext<'_>) {}
        fn paste(&self, _: &EventContext<'_>) {}
        fn paste_plain(&self, _: &EventContext<'_>) {}
        fn select_all(&self) {}
        fn input_document(&self) -> Option<&dyn TextInputDocument> {
            Some(self)
        }
        fn input_editor(&self) -> Option<&dyn TextInputEditor> {
            self.editable.get().then_some(self as &dyn TextInputEditor)
        }
    }

    #[derive(Debug)]
    struct Field {
        doc: Rc<Doc>,
    }

    impl Widget for Field {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            ctx.apply_self_handlers(HandlerSet::new().focusable(true));
            ctx.register_text_surface(self.doc.clone());
            vec![]
        }
        fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
            proposal.resolve(300.0, 20.0).into()
        }
    }

    /// Counts the payloads posted.
    #[derive(Default)]
    struct Poster {
        posted: AtomicUsize,
    }

    impl AppEventPoster for Poster {
        fn post_subscription_event(&self, _: SubscriptionId, _: Box<dyn std::any::Any + Send>) {}
        fn post_external(&self, payload: Box<dyn std::any::Any + Send>) {
            assert!(payload.downcast_ref::<TextStorePayload>().is_some());
            self.posted.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// What the stand-in for TSF heard, in order.
    #[derive(Debug, Default)]
    struct Heard {
        grants: Vec<u32>,
        changes: Vec<(i32, i32, i32)>,
        selections: u32,
        layouts: u32,
        statuses: u32,
        /// Every call, in order: `grant`, `text`, `selection`, `layout`,
        /// `status`.
        order: Vec<&'static str>,
    }

    /// The sink TSF would advise, running `during_grant` inside each grant.
    #[implement(ITextStoreACPSink)]
    struct Sink {
        heard: Rc<RefCell<Heard>>,
        during_grant: Grant,
    }

    /// What the sink runs inside its next grant.
    type Grant = Rc<RefCell<Option<Box<dyn FnOnce()>>>>;

    impl ITextStoreACPSink_Impl for Sink_Impl {
        fn OnTextChange(
            &self,
            _dwflags: TEXT_STORE_TEXT_CHANGE_FLAGS,
            pchange: *const TS_TEXTCHANGE,
        ) -> windows::core::Result<()> {
            let change = unsafe { *pchange };
            let mut heard = self.heard.borrow_mut();
            heard
                .changes
                .push((change.acpStart, change.acpOldEnd, change.acpNewEnd));
            heard.order.push("text");
            Ok(())
        }
        fn OnSelectionChange(&self) -> windows::core::Result<()> {
            let mut heard = self.heard.borrow_mut();
            heard.selections += 1;
            heard.order.push("selection");
            Ok(())
        }
        fn OnLayoutChange(&self, _lcode: TsLayoutCode, _vcview: u32) -> windows::core::Result<()> {
            let mut heard = self.heard.borrow_mut();
            heard.layouts += 1;
            heard.order.push("layout");
            Ok(())
        }
        fn OnStatusChange(&self, _dwflags: u32) -> windows::core::Result<()> {
            let mut heard = self.heard.borrow_mut();
            heard.statuses += 1;
            heard.order.push("status");
            Ok(())
        }
        fn OnAttrsChange(
            &self,
            _acpstart: i32,
            _acpend: i32,
            _cattrs: u32,
            _paattrs: *const GUID,
        ) -> windows::core::Result<()> {
            Ok(())
        }
        fn OnLockGranted(&self, dwlockflags: TEXT_STORE_LOCK_FLAGS) -> windows::core::Result<()> {
            {
                let mut heard = self.heard.borrow_mut();
                heard.grants.push(dwlockflags.0);
                heard.order.push("grant");
            }
            if let Some(body) = self.during_grant.borrow_mut().take() {
                body();
            }
            Ok(())
        }
        fn OnStartEditTransaction(&self) -> windows::core::Result<()> {
            Ok(())
        }
        fn OnEndEditTransaction(&self) -> windows::core::Result<()> {
            Ok(())
        }
    }

    struct Rig {
        tree: WidgetTree,
        doc: Rc<Doc>,
        state: Rc<StoreState>,
        store: ITextStoreACP,
        heard: Rc<RefCell<Heard>>,
        during_grant: Grant,
        poster: Arc<Poster>,
        _sink: ITextStoreACPSink,
    }

    /// A store over a focused field holding `text`, its caret at `caret`,
    /// with the test's sink advised; with `journal`, the field counts
    /// revisions.
    fn rig_with(text: &str, caret: usize, journal: bool) -> Rig {
        let doc = Rc::new(Doc::new(text, caret, journal));
        let mut tree = WidgetTree::new();
        let id = tree.add(Field { doc: doc.clone() });
        tree.layout(SizeProposal::exact(300.0, 20.0));
        tree.focus(id);
        let window_id = TeksiloWindowId::new(0xF5_0001);
        let poster = Arc::new(Poster::default());
        let state = Rc::new(StoreState {
            hwnd: HWND::default(),
            window_id,
            widget: id,
            surfaces: tree.text_surfaces(),
            service: Rc::new(Service {
                window_id,
                poster: poster.clone(),
                posted: Cell::new(false),
            }),
            inner: RefCell::new(StoreInner::default()),
        });
        state.report(true);
        let store: ITextStoreACP = TextStore {
            state: state.clone(),
        }
        .into();
        let heard = Rc::new(RefCell::new(Heard::default()));
        let during_grant = Grant::default();
        let sink: ITextStoreACPSink = Sink {
            heard: heard.clone(),
            during_grant: during_grant.clone(),
        }
        .into();
        unsafe { store.AdviseSink(&ITextStoreACPSink::IID, &sink, 0) }.expect("advised");
        Rig {
            tree,
            doc,
            state,
            store,
            heard,
            during_grant,
            poster,
            _sink: sink,
        }
    }

    fn rig(text: &str, caret: usize) -> Rig {
        rig_with(text, caret, false)
    }

    impl Rig {
        /// Run `body` inside the next grant.
        fn during_grant(&self, body: impl FnOnce() + 'static) {
            *self.during_grant.borrow_mut() = Some(Box::new(body));
        }

        fn posted(&self) -> usize {
            self.poster.posted.load(Ordering::Relaxed)
        }

        /// Read the whole text inside a read lock.
        fn read_text(&self) -> String {
            let store = self.store.clone();
            let text = Rc::new(RefCell::new(String::new()));
            let out = text.clone();
            self.during_grant(move || *out.borrow_mut() = text_of(&store).unwrap());
            assert_eq!(
                unsafe { self.store.RequestLock(LF_READ | LF_SYNC) }.unwrap(),
                S_OK
            );
            text.take()
        }
    }

    fn text_of(store: &ITextStoreACP) -> windows::core::Result<String> {
        text_in(store, 0, -1)
    }

    fn text_in(store: &ITextStoreACP, start: i32, end: i32) -> windows::core::Result<String> {
        units_in(store, start, end).map(|units| String::from_utf16_lossy(&units))
    }

    fn units_in(store: &ITextStoreACP, start: i32, end: i32) -> windows::core::Result<Vec<u16>> {
        let mut buffer = [0u16; 64];
        let mut count = 0;
        let mut runs = [TS_RUNINFO::default()];
        let mut run_count = 0;
        let mut next = 0;
        unsafe {
            store.GetText(
                start,
                end,
                &mut buffer,
                &mut count,
                &mut runs,
                &mut run_count,
                &mut next,
            )
        }?;
        Ok(buffer[..count as usize].to_vec())
    }

    #[test]
    fn reads_and_writes_go_through_a_lock_and_land_in_the_field() {
        let rig = rig_with("abc", 1, true);
        assert_eq!(
            text_of(&rig.store).map_err(|error| error.code()),
            Err(TS_E_NOLOCK),
            "no read without a lock"
        );
        let store = rig.store.clone();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        rig.during_grant(move || {
            let mut log = log.borrow_mut();
            log.push(format!("end {}", unsafe { store.GetEndACP() }.unwrap()));
            log.push(text_of(&store).unwrap());
            let mut selection = [TS_SELECTION_ACP::default()];
            let mut fetched = 0;
            unsafe { store.GetSelection(TF_DEFAULT_SELECTION, &mut selection, &mut fetched) }
                .unwrap();
            log.push(format!(
                "selection {}..{}",
                selection[0].acpStart, selection[0].acpEnd
            ));
            let change = unsafe { store.SetText(0, 1, 1, &[u16::from(b'X')]) }.unwrap();
            log.push(format!(
                "set {} {} {}",
                change.acpStart, change.acpOldEnd, change.acpNewEnd
            ));
            let (mut start, mut end) = (0, 0);
            unsafe {
                store.InsertTextAtSelection(
                    0,
                    &[u16::from(b'Y')],
                    &mut start,
                    &mut end,
                    std::ptr::null_mut(),
                )
            }
            .unwrap();
            log.push(format!("inserted {start}..{end}"));
            log.push(text_of(&store).unwrap());
            log.push(format!("end {}", unsafe { store.GetEndACP() }.unwrap()));
        });
        let session = unsafe { rig.store.RequestLock(LF_READWRITE | LF_SYNC) }.unwrap();
        assert_eq!(session, S_OK);
        assert_eq!(
            *seen.borrow(),
            vec![
                "end 3",
                "abc",
                "selection 1..1",
                "set 1 1 2",
                "inserted 2..3",
                "aXYbc",
                "end 5"
            ],
            "each write is read back at once"
        );
        assert_eq!(*rig.doc.text.borrow(), "aXYbc", "the field has them");
        assert_eq!(
            rig.doc.written(),
            vec![
                "begin",
                "replace 1..1 \"X\"",
                "replace 2..2 \"Y\"",
                "end batch"
            ],
            "one batch for the grant"
        );
        assert!(
            rig.heard.borrow().changes.is_empty(),
            "the text service's own writes are not told back"
        );
        assert_eq!(rig.heard.borrow().selections, 0, "nor the caret they left");
        assert_eq!(rig.state.inner.borrow().mirror.text(), "aXYbc", "followed");
    }

    #[test]
    fn a_write_lands_in_utf16_and_a_query_changes_nothing() {
        // "a😀b": the emoji is units 1..3.
        let rig = rig_with("a\u{1F600}b", 3, true);
        let store = rig.store.clone();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        rig.during_grant(move || {
            let mut log = log.borrow_mut();
            let change = unsafe { store.SetText(0, 1, 3, &[u16::from(b'e')]) }.unwrap();
            log.push((change.acpStart, change.acpOldEnd, change.acpNewEnd));
            let (mut start, mut end) = (0, 0);
            unsafe {
                store.InsertTextAtSelection(
                    TS_IAS_QUERYONLY,
                    &[0xD83D, 0xDE00],
                    &mut start,
                    &mut end,
                    std::ptr::null_mut(),
                )
            }
            .unwrap();
            log.push((start, start, end));
        });
        assert_eq!(
            unsafe { rig.store.RequestLock(LF_READWRITE | LF_SYNC) }.unwrap(),
            S_OK
        );
        assert_eq!(*seen.borrow(), vec![(1, 3, 2), (2, 2, 4)]);
        assert_eq!(*rig.doc.text.borrow(), "aeb", "the query wrote nothing");
    }

    #[test]
    fn reads_answer_from_the_document_in_utf16_cut_to_the_units_asked() {
        // "a😀b": characters 0..3, UTF-16 units 0..4.
        let rig = rig("a\u{1F600}b", 3);
        let store = rig.store.clone();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        // A document that counts no revisions changes behind the mirror's
        // back: the reads still answer what the document holds.
        *rig.doc.text.borrow_mut() = "a\u{1F600}bc".to_owned();
        rig.during_grant(move || {
            let mut log = log.borrow_mut();
            log.push(format!("end {}", unsafe { store.GetEndACP() }.unwrap()));
            log.push(text_of(&store).unwrap());
            log.push(format!("{:x?}", units_in(&store, 1, 2).unwrap()));
            log.push(text_in(&store, 3, 5).unwrap());
            let (mut rect, mut clipped) = (RECT::default(), BOOL::default());
            unsafe { store.GetTextExt(VIEW, 3, 5, &mut rect, &mut clipped) }.unwrap();
            log.push(format!("clipped {}", clipped.as_bool()));
        });
        assert_eq!(
            unsafe { rig.store.RequestLock(LF_READ | LF_SYNC) }.unwrap(),
            S_OK
        );
        assert_eq!(
            *seen.borrow(),
            vec![
                "end 5".to_owned(),
                "a\u{1F600}bc".to_owned(),
                "[d83d]".to_owned(),
                "bc".to_owned(),
                "clipped false".to_owned(),
            ],
            "half of a pair is the unit asked for, not the whole character"
        );
    }

    #[test]
    fn a_ranges_box_comes_from_its_own_lines() {
        let rig = rig("abcdef", 0);
        let measured = rig.state.rect_of(2..5);
        assert_eq!(
            measured,
            Some((Rect::new(20.0, 0.0, 30.0, 20.0), false)),
            "the range's own box, not the caret's"
        );
    }

    #[test]
    fn a_write_needs_a_read_write_lock() {
        let rig = rig("abc", 0);
        let store = rig.store.clone();
        let result = Rc::new(Cell::new(S_OK));
        let out = result.clone();
        rig.during_grant(move || {
            let code = unsafe { store.SetText(0, 0, 0, &[u16::from(b'X')]) }
                .map_or_else(|error| error.code(), |_| S_OK);
            out.set(code);
        });
        assert_eq!(
            unsafe { rig.store.RequestLock(LF_READ | LF_SYNC) }.unwrap(),
            S_OK
        );
        assert_eq!(result.get(), TS_E_NOLOCK);
    }

    #[test]
    fn a_lock_asked_inside_a_grant_is_refused_if_synchronous_and_queued_if_not() {
        let rig = rig("abc", 0);
        let store = rig.store.clone();
        let answers = Rc::new(RefCell::new(Vec::new()));
        let out = answers.clone();
        rig.during_grant(move || {
            let mut out = out.borrow_mut();
            out.push(unsafe { store.RequestLock(LF_READ | LF_SYNC) }.unwrap());
            out.push(unsafe { store.RequestLock(LF_READWRITE) }.unwrap());
        });
        assert_eq!(unsafe { rig.store.RequestLock(LF_READ) }.unwrap(), S_OK);
        assert_eq!(*answers.borrow(), vec![TS_E_SYNCHRONOUS, TS_S_ASYNC]);
        assert_eq!(
            rig.heard.borrow().grants,
            vec![LF_READ, LF_READWRITE],
            "the queued request granted as the first one ends"
        );
    }

    #[test]
    fn the_documents_changes_are_reported_outside_a_lock() {
        let rig = rig("abc", 3);
        *rig.doc.text.borrow_mut() = "abcd".to_owned();
        *rig.doc.selection.borrow_mut() = 4..4;
        rig.state.report(true);
        assert_eq!(rig.heard.borrow().changes, vec![(3, 3, 4)]);
        assert_eq!(rig.heard.borrow().selections, 1);
        assert_eq!(rig.state.inner.borrow().mirror.text(), "abcd");
    }

    #[test]
    fn the_journals_range_is_reported_in_utf16_and_the_input_systems_own_is_not() {
        let rig = rig_with("\u{1F600}\u{1F600}abc", 5, true);
        rig.heard.borrow_mut().order.clear();
        // "x" after the two emoji: characters 2..2, UTF-16 units 4..4.
        rig.doc.edit(2..2, "x", ChangeCause::Other);
        rig.state.report(true);
        assert_eq!(rig.heard.borrow().changes, vec![(4, 4, 5)]);
        assert_eq!(
            rig.heard.borrow().order,
            vec!["text", "selection", "layout"],
            "text, then selection, then layout"
        );
        assert_eq!(
            rig.state.inner.borrow().mirror.text(),
            "\u{1F600}\u{1F600}xabc"
        );

        // A change the input system made itself is followed, not reported.
        rig.doc.edit(0..1, "", ChangeCause::InputSystem);
        rig.state.report(true);
        assert_eq!(rig.heard.borrow().changes.len(), 1, "not reported");
        assert_eq!(rig.state.inner.borrow().mirror.text(), "\u{1F600}xabc");
        assert_eq!(rig.read_text(), "\u{1F600}xabc");

        // Two changes the journal cannot merge: the whole text is compared
        // instead, and the smallest change reported.
        rig.doc.merges.set(false);
        rig.doc.edit(5..5, "!", ChangeCause::Other);
        rig.doc.edit(0..0, "?", ChangeCause::Other);
        rig.state.report(true);
        assert_eq!(rig.heard.borrow().changes.last(), Some(&(0, 6, 8)));
        assert_eq!(rig.state.inner.borrow().mirror.text(), "?\u{1F600}xabc!");

        // Nothing changed: nothing reported.
        let heard = rig.heard.borrow().order.len();
        rig.state.report(true);
        assert_eq!(rig.heard.borrow().order.len(), heard);
    }

    #[test]
    fn a_lock_waits_while_changes_are_unreported_then_follows_their_report() {
        let rig = rig_with("abc", 3, true);
        rig.heard.borrow_mut().order.clear();
        rig.doc.edit(3..3, "d", ChangeCause::Other);
        assert_eq!(
            unsafe { rig.store.RequestLock(LF_READ | LF_SYNC) }.unwrap(),
            TS_E_SYNCHRONOUS,
            "a synchronous request is refused"
        );
        assert_eq!(
            unsafe { rig.store.RequestLock(LF_READ) }.unwrap(),
            TS_S_ASYNC
        );
        assert_eq!(
            unsafe { rig.store.RequestLock(LF_READWRITE) }.unwrap(),
            TS_S_ASYNC
        );
        assert!(rig.heard.borrow().grants.is_empty(), "nothing granted yet");
        assert!(
            rig.heard.borrow().changes.is_empty(),
            "nothing reported inside RequestLock"
        );
        assert_eq!(rig.posted(), 1, "asked for a settle, once");

        // What the settle does.
        rig.state.report(true);
        rig.state.grant_deferred(true);
        assert_eq!(
            rig.heard.borrow().order,
            vec!["text", "selection", "layout", "grant"],
            "reported, then granted"
        );
        assert_eq!(
            rig.heard.borrow().grants,
            vec![LF_READWRITE],
            "the two requests are one, read-write"
        );
        assert_eq!(rig.read_text(), "abcd");
    }

    #[test]
    fn a_change_made_during_a_grant_is_reported_at_the_tail_of_request_lock() {
        let rig = rig_with("abc", 3, true);
        rig.heard.borrow_mut().order.clear();
        // A message delivered during the grant runs the application, which
        // edits the field (4.4, case 3).
        let doc = rig.doc.clone();
        rig.during_grant(move || doc.edit(0..0, "x", ChangeCause::Other));
        assert_eq!(
            unsafe { rig.store.RequestLock(LF_READ | LF_SYNC) }.unwrap(),
            S_OK
        );
        assert_eq!(
            rig.heard.borrow().order,
            vec!["grant", "text", "selection", "layout"],
            "after the grant, before RequestLock returns"
        );
        assert_eq!(rig.heard.borrow().changes, vec![(0, 0, 1)]);
    }

    /// 変換 with nothing selected: the IME selects the word at the caret and
    /// opens a composition over it in one lock. The selection it set is the
    /// field's, read back as such and not told back; the field's own
    /// selection moving afterwards (Shift+Right) is reported, and holds no
    /// later lock back (measured stall, Phase 1 round 1).
    #[test]
    fn a_selection_the_store_sets_is_the_fields_and_a_later_move_is_reported() {
        let rig = rig_with("left right", 0, true);
        let store = rig.store.clone();
        let read = Rc::new(RefCell::new(None));
        let out = read.clone();
        rig.during_grant(move || {
            let selection = [TS_SELECTION_ACP {
                acpStart: 0,
                acpEnd: 4,
                style: TS_SELECTIONSTYLE {
                    ase: TS_AE_END,
                    fInterimChar: false.into(),
                },
            }];
            unsafe { store.SetSelection(&selection) }.unwrap();
            let mut got = [TS_SELECTION_ACP::default()];
            let mut fetched = 0;
            unsafe { store.GetSelection(TF_DEFAULT_SELECTION, &mut got, &mut fetched) }.unwrap();
            *out.borrow_mut() = Some((got[0].acpStart, got[0].acpEnd));
        });
        assert_eq!(
            unsafe { rig.store.RequestLock(LF_READWRITE | LF_SYNC) }.unwrap(),
            S_OK
        );
        assert_eq!(*read.borrow(), Some((0, 4)), "the IME's own selection");
        assert_eq!(*rig.doc.selection.borrow(), 0..4, "set in the field");
        assert_eq!(rig.heard.borrow().selections, 0, "not told back");

        // Shift+Right in the field, then the IME's next keys.
        rig.doc.select(0..1);
        rig.state.report(true);
        assert_eq!(rig.heard.borrow().selections, 1, "the field's move is told");
        assert_eq!(
            unsafe { rig.store.RequestLock(LF_READWRITE) }.unwrap(),
            S_OK
        );
        assert_eq!(rig.posted(), 0, "no settle asked for");
    }

    /// The composition sink: a composition the text service starts is the
    /// field's, over the range it names, and its end is the field's commit.
    #[test]
    fn a_composition_starts_and_ends_in_the_field() {
        let rig = rig_with("ab", 2, true);
        let store = rig.store.clone();
        rig.during_grant(move || {
            let composition: ITfContextOwnerCompositionSink = store.cast().unwrap();
            // No view: TSF hands one, whose range the test cannot make, so
            // the composition is refused.
            assert!(
                !unsafe { composition.OnStartComposition(None) }
                    .unwrap()
                    .as_bool()
            );
            unsafe { store.SetText(0, 2, 2, &[0x65E5, 0x672C]) }.unwrap();
            unsafe { composition.OnEndComposition(None) }.unwrap();
        });
        assert_eq!(
            unsafe { rig.store.RequestLock(LF_READWRITE) }.unwrap(),
            S_OK
        );
        assert_eq!(
            rig.doc.written(),
            vec![
                "begin",
                "replace 2..2 \"\u{65E5}\u{672C}\"",
                "end Composition",
                "end batch"
            ]
        );
        assert!(rig.heard.borrow().changes.is_empty());
    }

    /// A key the field handles while a composition is open (Down passed on
    /// by the IME) is reported at the next settle, and no later lock waits.
    #[test]
    fn a_key_the_field_handles_during_a_composition_is_reported() {
        let rig = rig_with("ab", 2, true);
        rig.state.inner.borrow_mut().compositions = 1;
        rig.heard.borrow_mut().order.clear();
        rig.doc.select(0..0);
        assert_eq!(
            unsafe { rig.store.RequestLock(LF_READWRITE) }.unwrap(),
            TS_S_ASYNC,
            "it waits for the report"
        );
        rig.state.report(true);
        rig.state.grant_deferred(true);
        assert_eq!(
            rig.heard.borrow().order,
            vec!["selection", "grant"],
            "reported, then granted"
        );
    }

    /// Typing that waits for the frame while a composition is open is
    /// inserted at the settle all the same, and the lock that waited for it
    /// is granted.
    #[test]
    fn typing_waiting_during_a_composition_is_flushed_and_the_lock_granted() {
        let rig = rig_with("abc", 3, true);
        rig.state.inner.borrow_mut().compositions = 1;
        rig.doc.pending.borrow_mut().push('d');
        assert_eq!(
            unsafe { rig.store.RequestLock(LF_READWRITE) }.unwrap(),
            TS_S_ASYNC
        );
        rig.state.report(true);
        rig.state.grant_deferred(true);
        assert_eq!(rig.heard.borrow().grants, vec![LF_READWRITE]);
        assert_eq!(*rig.doc.text.borrow(), "abcd");
    }

    #[test]
    fn typing_held_for_the_frame_goes_in_before_a_lock_is_granted() {
        let rig = rig_with("abc", 3, true);
        rig.doc.pending.borrow_mut().push_str("de");
        assert_eq!(
            unsafe { rig.store.RequestLock(LF_READ) }.unwrap(),
            TS_S_ASYNC,
            "the field is busy"
        );
        // Inside RequestLock nothing may flush: the next settle does.
        rig.state.report(false);
        rig.state.grant_deferred(false);
        assert!(rig.heard.borrow().grants.is_empty());
        rig.state.report(true);
        rig.state.grant_deferred(true);
        assert_eq!(rig.heard.borrow().changes, vec![(3, 3, 5)]);
        assert_eq!(rig.heard.borrow().grants, vec![LF_READ]);
    }

    #[test]
    fn a_read_only_field_takes_no_write_and_no_composition() {
        let rig = rig_with("abc", 0, true);
        rig.doc.editable.set(false);
        rig.state.report(true);
        assert_eq!(rig.heard.borrow().statuses, 1, "the status change reported");
        let status = unsafe { rig.store.GetStatus() }.unwrap();
        assert_ne!(status.dwDynamicFlags & TS_SD_READONLY, 0);
        let composition: ITfContextOwnerCompositionSink = rig.store.cast().unwrap();
        assert!(
            !unsafe { composition.OnStartComposition(None) }
                .unwrap()
                .as_bool()
        );
        let store = rig.store.clone();
        let result = Rc::new(Cell::new(S_OK));
        let out = result.clone();
        rig.during_grant(move || {
            let code = unsafe { store.SetText(0, 0, 0, &[u16::from(b'X')]) }
                .map_or_else(|error| error.code(), |_| S_OK);
            out.set(code);
        });
        assert_eq!(
            unsafe { rig.store.RequestLock(LF_READWRITE | LF_SYNC) }.unwrap(),
            S_OK
        );
        assert_eq!(result.get(), TS_E_READONLY);
        assert_eq!(rig.read_text(), "abc", "still read");
    }

    #[test]
    fn the_store_still_serves_its_field_once_focus_has_left() {
        let mut rig = rig_with("abc", 0, true);
        rig.tree.clear_focus();
        assert_eq!(
            rig.state.read_document(|_, index| Some(index.utf16_len())),
            Some(3)
        );
        assert_eq!(rig.read_text(), "abc");
    }

    #[test]
    fn a_display_attribute_names_a_clause_style() {
        assert_eq!(style_of(TF_ATTR_TARGET_CONVERTED), SegmentStyle::Target);
        assert_eq!(
            style_of(TF_ATTR_TARGET_NOTCONVERTED),
            SegmentStyle::TargetInput
        );
        assert_eq!(style_of(TF_ATTR_CONVERTED), SegmentStyle::Converted);
        assert_eq!(style_of(TF_ATTR_FIXEDCONVERTED), SegmentStyle::Converted);
        assert_eq!(style_of(TF_DA_ATTR_INFO(0)), SegmentStyle::Input);
        assert_eq!(style_of(TF_DA_ATTR_INFO(-1)), SegmentStyle::Input);
    }
}
