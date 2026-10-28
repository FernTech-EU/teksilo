// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Windows: a TSF text store, Phase 0 of `engineering/docs/tsf-text-store.md`
//! (7.1): a measuring build, behind `TEKSILO_TSF` ([`TextStoreMode`]).
//!
//! What it does:
//!
//! - **The thread** is activated once (`ITfThreadMgrEx::ActivateEx`, else
//!   `ITfThreadMgr::Activate`), on the existing thread manager CUAS made
//!   (`TF_GetThreadMgr`) or a new one, if the UI thread is an STA; it never
//!   initialises COM. Keys follow Chromium's model: TSF processes them
//!   itself, and an `ITfKeyTraceEventSink` on the thread manager says which
//!   it saw (M9). The thread is never deactivated in Phase 0, and its COM
//!   pointers are leaked at thread exit rather than released in an order
//!   nobody controls (4.5).
//! - **Each window** gets its own empty document manager with an empty,
//!   disabled context (`GUID_COMPARTMENT_KEYBOARD_DISABLED` and
//!   `GUID_COMPARTMENT_EMPTYCONTEXT` set to 1), associated with its HWND once
//!   (`AssociateFocus`), as Chromium does (4.6). For the IMM path
//!   ([`StoreFocus::Imm`]) the HWND is associated back with what that call
//!   handed back (nothing, measured), so that CUAS's own document takes the
//!   focus as winit's IMM context activates; with our empty document
//!   associated, TSF focused it instead and the input method stayed off (M2).
//!   It is associated back at detach too.
//! - **Each focus session** on a field that describes its document gets a
//!   new document manager and context over a [`TextStore`]
//!   (`ITextStoreACP`, `ITfContextOwnerCompositionSink`) and an
//!   [`EditSink`] (`ITfTextEditSink`, M4). TSF focus moves only from
//!   [`set_text_store_focus`], which `teksilo-app` calls after every
//!   dispatched event (case 2 of 4.4: TSF calls are allowed there), and only
//!   for the window that has the Win32 focus (`GetFocus`); a change made
//!   while the window is in the background is applied when it comes back
//!   (M14).
//! - **The store** answers from a [`TextMirror`]: the focused document's
//!   text and selection in UTF-16, synced at every call of
//!   [`set_text_store_focus`] while no lock is held and no composition is in
//!   progress, with `OnTextChange`, `OnSelectionChange` and
//!   `OnLayoutChange` sent then, outside any lock. Writes land in the mirror
//!   only; the next sync puts the document back. Locks follow
//!   [`LockArbiter`] (4.7). Geometry is read from the document, through
//!   [`TextInputDocument::is_available`] first, while the mirror holds the
//!   document's text; otherwise the caret rectangle cached at the last sync
//!   is answered, never `TS_E_NOLAYOUT` (4.8).
//!
//! Everything a text service does is traced (`TEKSILO_TEXT_INPUT_TRACE`),
//! with a `tsf:` tag: locks and their flags (M12), every read and write, the
//! compositions and their owners, the properties on every edited range, the
//! keys TSF saw, the focus calls.

use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use teksilo_canvas::{Point, Rect};
use teksilo_core::WidgetId;
use teksilo_core::text_input_document::TextInputDocument;
use teksilo_core::text_surface::TextSurfaces;
use teksilo_core::window::TeksiloWindowId;

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
    CLSID_TF_ThreadMgr, GUID_COMPARTMENT_EMPTYCONTEXT, GUID_COMPARTMENT_KEYBOARD_DISABLED,
    ITextStoreACP, ITextStoreACP_Impl, ITextStoreACPSink, ITfCompartmentMgr, ITfCompositionView,
    ITfContext, ITfContextOwnerCompositionSink, ITfContextOwnerCompositionSink_Impl,
    ITfDocumentMgr, ITfEditRecord, ITfKeyTraceEventSink, ITfKeyTraceEventSink_Impl, ITfProperty,
    ITfRange, ITfSource, ITfTextEditSink, ITfTextEditSink_Impl, ITfThreadMgr, ITfThreadMgrEx,
    TEXT_STORE_LOCK_FLAGS, TEXT_STORE_TEXT_CHANGE_FLAGS, TF_DEFAULT_SELECTION, TF_GTP_INCL_TEXT,
    TS_AE_END, TS_AE_START, TS_ATTRVAL, TS_E_INVALIDPOINT, TS_E_INVALIDPOS, TS_E_NOLOCK,
    TS_E_READONLY, TS_E_SYNCHRONOUS, TS_IAS_QUERYONLY, TS_LC_CHANGE, TS_RT_PLAIN, TS_RUNINFO,
    TS_S_ASYNC, TS_SD_INPUTPANEMANUALDISPLAYENABLE, TS_SD_READONLY, TS_SELECTION_ACP,
    TS_SELECTIONSTYLE, TS_SS_NOHIDDENTEXT, TS_STATUS, TS_TEXTCHANGE,
};
use windows::core::{BOOL, GUID, HRESULT, IUnknown, Interface, PCWSTR, PWSTR, Ref, implement};

use super::super::answers::{self, Utf16Range};
use super::super::tsf_lock::{LockArbiter, LockDecision};
use super::super::tsf_mirror::{TextChange, TextMirror};
use super::super::{StoreFocus, TextStoreMode, text_store_mode};
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

impl ITfKeyTraceEventSink_Impl for KeyTraceSink_Impl {
    fn OnKeyTraceDown(&self, wparam: WPARAM, lparam: LPARAM) -> windows::core::Result<()> {
        tsf_trace!(
            "#{} key trace down vk={:#04x} lparam={:#x}",
            next_sequence(),
            wparam.0,
            lparam.0
        );
        Ok(())
    }

    fn OnKeyTraceUp(&self, wparam: WPARAM, lparam: LPARAM) -> windows::core::Result<()> {
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

pub(super) fn attach(window_id: TeksiloWindowId, hwnd: HWND, surfaces: TextSurfaces) {
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
    if let Some((manager, _)) = thread() {
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
    if imm {
        // CUAS focuses its own document as winit's IMM context activates.
        window.applied = Some(focus);
    } else if has_focus && window.applied != Some(focus) {
        let target = match focus {
            StoreFocus::Document { .. } => window
                .session
                .as_ref()
                .map(|session| session.manager.clone()),
            StoreFocus::Imm | StoreFocus::None => Some(window.empty.clone()),
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
        state.sync();
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
        surfaces: window.surfaces.clone(),
        inner: RefCell::new(StoreInner::default()),
    });
    // The document's text before TSF first reads it.
    state.sync();
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
    surfaces: TextSurfaces,
    inner: RefCell<StoreInner>,
}

#[derive(Default)]
struct StoreInner {
    mirror: TextMirror,
    locks: LockArbiter,
    sink: Option<ITextStoreACPSink>,
    compositions: u32,
    editable: bool,
    /// Window-logical, at the last sync.
    caret: Option<Rect>,
    visible: Option<Rect>,
}

/// What the focused document says, read at a sync.
struct DocumentSnapshot {
    text: String,
    selection: Range<usize>,
    caret_at_start: bool,
    editable: bool,
    caret: Option<Rect>,
    visible: Option<Rect>,
}

impl StoreState {
    /// Bring the mirror up to date with the document, while no lock is held
    /// and no composition is in progress, and tell the sink what changed.
    fn sync(&self) {
        let Some(snapshot) = self
            .surfaces
            .with_focused_input_document(snapshot)
            .flatten()
        else {
            return;
        };
        let (change, moved, layout, sink) = {
            let Ok(mut inner) = self.inner.try_borrow_mut() else {
                return;
            };
            if inner.locks.held().is_some() || inner.compositions > 0 {
                return;
            }
            if inner.mirror.written() {
                tsf_trace!(
                    "#{} {} mirror: the text services' writes go, as {:?}",
                    next_sequence(),
                    self.window_id,
                    inner.mirror.text()
                );
            }
            let (change, moved) = inner.mirror.sync(
                &snapshot.text,
                snapshot.selection.clone(),
                snapshot.caret_at_start,
            );
            let layout = inner.caret != snapshot.caret || inner.visible != snapshot.visible;
            inner.editable = snapshot.editable;
            inner.caret = snapshot.caret;
            inner.visible = snapshot.visible;
            (change, moved, layout, inner.sink.clone())
        };
        let Some(sink) = sink else {
            return;
        };
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
            let _ = unsafe { sink.OnLayoutChange(TS_LC_CHANGE, VIEW) };
        }
    }

    /// Where `range` (UTF-16, the mirror's) is drawn, window-logical: from
    /// the document while the mirror holds its text and it can answer with
    /// no change, else the caret rectangle cached at the last sync.
    fn rect_of(&self, range: Range<usize>) -> Option<Rect> {
        let (in_sync, cached) = {
            let inner = self.inner.try_borrow().ok()?;
            (!inner.mirror.written(), inner.caret)
        };
        if in_sync {
            let measured = self
                .surfaces
                .with_focused_input_document(|document| {
                    if !document.is_available() {
                        return None;
                    }
                    answers::first_rect(document, Utf16Range::from_range(range))
                        .map(|(rect, _)| rect)
                })
                .flatten();
            if measured.is_some() {
                return measured;
            }
        }
        cached
    }
}

/// Read what a sync needs from the focused document.
fn snapshot(document: &dyn TextInputDocument) -> Option<DocumentSnapshot> {
    if document.is_secure() {
        return None;
    }
    let index = document.utf16_index()?;
    let text = document.text_in(0..document.char_count()?)?;
    let selection = document.selection()?;
    let caret = document.caret()?;
    let caret_rect = answers::first_rect(
        document,
        Utf16Range::from_range(index.char_to_utf16(caret)..index.char_to_utf16(caret)),
    )
    .map(|(rect, _)| rect);
    Some(DocumentSnapshot {
        caret_at_start: caret == selection.start && !selection.is_empty(),
        selection: index.char_range_to_utf16(selection),
        text,
        editable: document.is_editable(),
        caret: caret_rect,
        visible: document.visible_text_rect(),
    })
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

    fn writable(&self) -> windows::core::Result<std::cell::RefMut<'_, StoreInner>> {
        let inner = self.inner()?;
        if !inner.locks.can_write() {
            tsf_trace!("write without a read-write lock");
            return Err(TS_E_NOLOCK.into());
        }
        if !inner.editable {
            return Err(TS_E_READONLY.into());
        }
        Ok(inner)
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
            let (decision, sink) = {
                let mut inner = self.inner()?;
                (inner.locks.request(dwlockflags), inner.sink.clone())
            };
            tsf_trace!(
                "#{} RequestLock flags={dwlockflags:#x} -> {decision:?}",
                next_sequence()
            );
            let mut kind = match decision {
                LockDecision::Refused => return Ok(TS_E_SYNCHRONOUS),
                LockDecision::Queued => return Ok(TS_S_ASYNC),
                LockDecision::Grant(kind) => kind,
            };
            let Some(sink) = sink else {
                self.inner()?.locks.release();
                return Err(E_UNEXPECTED.into());
            };
            let mut session = S_OK;
            loop {
                // No borrow is held across the grant: the text service reads
                // and writes from inside it.
                let result = unsafe { sink.OnLockGranted(TEXT_STORE_LOCK_FLAGS(kind.flags())) };
                if session == S_OK {
                    session = result.map_or_else(|error| error.code(), |()| S_OK);
                }
                match self.inner()?.locks.release() {
                    Some(next) => {
                        tsf_trace!("#{} queued lock granted: {next:?}", next_sequence());
                        kind = next;
                    }
                    None => break,
                }
            }
            Ok(session)
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
            let len = self.inner()?.mirror.len();
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
            let inner = self.readable()?;
            if ulindex != 0 && ulindex != TF_DEFAULT_SELECTION {
                return Err(E_INVALIDARG.into());
            }
            let selection = inner.mirror.selection();
            let fetched = u32::from(ulcount > 0);
            if fetched == 1 {
                // SAFETY: TSF passes room for `ulcount` selections.
                unsafe {
                    *pselection = TS_SELECTION_ACP {
                        acpStart: selection.start as i32,
                        acpEnd: selection.end as i32,
                        style: TS_SELECTIONSTYLE {
                            ase: if inner.mirror.caret_at_start() {
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
            tsf_trace!("#{} GetSelection -> {selection:?}", next_sequence());
            Ok(())
        })
    }

    fn SetSelection(
        &self,
        ulcount: u32,
        pselection: *const TS_SELECTION_ACP,
    ) -> windows::core::Result<()> {
        guarded(|| {
            let mut inner = self.writable()?;
            if ulcount == 0 {
                return Ok(());
            }
            // SAFETY: TSF passes `ulcount` selections.
            let selection = unsafe { *pselection };
            let len = inner.mirror.len();
            let range = position(selection.acpStart, len)?..position(selection.acpEnd, len)?;
            tsf_trace!("#{} SetSelection {range:?}", next_sequence());
            inner
                .mirror
                .set_selection(range, selection.style.ase == TS_AE_START)
                .map_err(|_| TS_E_INVALIDPOS.into())
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
            let inner = self.readable()?;
            let len = inner.mirror.len();
            let start = position(acpstart, len)?;
            let end = if acpend == -1 {
                len
            } else {
                position(acpend, len)?
            };
            if end < start {
                return Err(TS_E_INVALIDPOS.into());
            }
            let wanted = (end - start).min(cchplainreq as usize);
            let units = inner.mirror.get(start..start + wanted);
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
                "#{} GetText {start}..{end} (asked {cchplainreq} units) -> {} units",
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
            let mut inner = self.writable()?;
            let len = inner.mirror.len();
            let range = position(acpstart, len)?..position(acpend, len)?;
            let text = units_of(pchtext, cch);
            let change = inner
                .mirror
                .replace(range.clone(), &text)
                .map_err(|_| windows::core::Error::from(TS_E_INVALIDPOS))?;
            tsf_trace!(
                "#{} SetText flags={dwflags:#x} {range:?} <- {:?}; now {:?}",
                next_sequence(),
                String::from_utf16_lossy(&text),
                inner.mirror.text()
            );
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
            let mut inner = if query_only {
                self.readable()?
            } else {
                self.writable()?
            };
            let text = units_of(pchtext, cch);
            let change = inner.mirror.insert_at_selection(&text, query_only);
            tsf_trace!(
                "#{} InsertTextAtSelection flags={dwflags:#x} {:?} -> {change:?}; now {:?}",
                next_sequence(),
                String::from_utf16_lossy(&text),
                inner.mirror.text()
            );
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
        guarded(|| Ok(self.readable()?.mirror.len() as i32))
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
            let written = self.readable()?.mirror.written();
            // SAFETY: TSF passes a valid point.
            let mut point = unsafe { *ptscreen };
            unsafe {
                let _ = ScreenToClient(self.state.hwnd, &mut point);
            }
            let scale = unsafe { GetDpiForWindow(self.state.hwnd) }.max(96) as f32 / 96.0;
            let logical = Point::new(point.x as f32 / scale, point.y as f32 / scale);
            let offset = (!written)
                .then(|| {
                    self.state
                        .surfaces
                        .with_focused_input_document(|document| {
                            if !document.is_available() {
                                return None;
                            }
                            answers::character_index(document, logical)
                        })
                        .flatten()
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
            let len = self.readable()?.mirror.len();
            let range = position(acpstart, len)?..position(acpend, len)?;
            let rect = self.state.rect_of(range.clone());
            let screen = rect
                .map(|rect| screen_rect(self.state.hwnd, rect))
                .unwrap_or_default();
            tsf_trace!(
                "#{} GetTextExt {range:?} -> {rect:?} screen ({}, {})..({}, {})",
                next_sequence(),
                screen.left,
                screen.top,
                screen.right,
                screen.bottom
            );
            // SAFETY: TSF passes valid out pointers.
            unsafe {
                *prc = screen;
                *pfclipped = false.into();
            }
            Ok(())
        })
    }

    fn GetScreenExt(&self, _vcview: u32) -> windows::core::Result<RECT> {
        guarded(|| {
            let visible = self.inner()?.visible;
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
            self.inner()?.compositions += 1;
            tsf_trace!(
                "#{} OnStartComposition owner={}",
                next_sequence(),
                owner(pcomposition.ok().ok())
            );
            Ok(true.into())
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
            let mut inner = self.inner()?;
            inner.compositions = inner.compositions.saturating_sub(1);
            tsf_trace!(
                "#{} OnEndComposition owner={}; mirror {:?}",
                next_sequence(),
                owner(pcomposition.ok().ok()),
                inner.mirror.text()
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
        if !super::super::trace_enabled() {
            return Ok(());
        }
        guarded(|| {
            let context = pic.ok()?;
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

    use std::cell::Cell;

    use teksilo_canvas::SizeProposal;
    use teksilo_core::text_input_document::{FirstLineRect, Utf16Index};
    use teksilo_core::text_surface::TextSurface;
    use teksilo_core::{
        BuildContext, EventContext, HandlerSet, LayoutContext, LayoutResponse, Widget, WidgetTree,
    };
    use windows::Win32::UI::TextServices::{ITextStoreACPSink_Impl, TsLayoutCode};

    use super::super::super::tsf_lock::{LF_READ, LF_READWRITE, LF_SYNC};
    use super::*;

    /// One line of text, the selection a caret at `caret`.
    #[derive(Debug)]
    struct Doc {
        text: RefCell<String>,
        caret: Cell<usize>,
    }

    impl Doc {
        fn len(&self) -> usize {
            self.text.borrow().chars().count()
        }
    }

    impl TextInputDocument for Doc {
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
            Some(self.caret.get()..self.caret.get())
        }
        fn caret(&self) -> Option<usize> {
            Some(self.caret.get())
        }
        fn composition_range(&self) -> Option<Range<usize>> {
            None
        }
        fn first_line_rect(&self, range: Range<usize>) -> Option<FirstLineRect> {
            Some(FirstLineRect {
                rect: Rect::new(range.start as f32 * 10.0, 0.0, 1.0, 20.0),
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
            true
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
            false
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

    /// What the stand-in for TSF heard.
    #[derive(Debug, Default)]
    struct Heard {
        grants: Vec<u32>,
        changes: Vec<(i32, i32, i32)>,
        selections: u32,
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
            self.heard.borrow_mut().changes.push((
                change.acpStart,
                change.acpOldEnd,
                change.acpNewEnd,
            ));
            Ok(())
        }
        fn OnSelectionChange(&self) -> windows::core::Result<()> {
            self.heard.borrow_mut().selections += 1;
            Ok(())
        }
        fn OnLayoutChange(&self, _lcode: TsLayoutCode, _vcview: u32) -> windows::core::Result<()> {
            Ok(())
        }
        fn OnStatusChange(&self, _dwflags: u32) -> windows::core::Result<()> {
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
            self.heard.borrow_mut().grants.push(dwlockflags.0);
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
        _tree: WidgetTree,
        doc: Rc<Doc>,
        state: Rc<StoreState>,
        store: ITextStoreACP,
        heard: Rc<RefCell<Heard>>,
        during_grant: Grant,
        _sink: ITextStoreACPSink,
    }

    /// A store over a focused field holding `text`, its caret at `caret`,
    /// with the test's sink advised.
    fn rig(text: &str, caret: usize) -> Rig {
        let doc = Rc::new(Doc {
            text: RefCell::new(text.to_owned()),
            caret: Cell::new(caret),
        });
        let mut tree = WidgetTree::new();
        let id = tree.add(Field { doc: doc.clone() });
        tree.layout(SizeProposal::exact(300.0, 20.0));
        tree.focus(id);
        let state = Rc::new(StoreState {
            hwnd: HWND::default(),
            window_id: TeksiloWindowId::new(0xF5_0001),
            surfaces: tree.text_surfaces(),
            inner: RefCell::new(StoreInner::default()),
        });
        state.sync();
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
            _tree: tree,
            doc,
            state,
            store,
            heard,
            during_grant,
            _sink: sink,
        }
    }

    impl Rig {
        /// Run `body` inside the next grant.
        fn during_grant(&self, body: impl FnOnce() + 'static) {
            *self.during_grant.borrow_mut() = Some(Box::new(body));
        }
    }

    fn text_of(store: &ITextStoreACP) -> windows::core::Result<String> {
        let mut buffer = [0u16; 64];
        let mut count = 0;
        let mut runs = [TS_RUNINFO::default()];
        let mut run_count = 0;
        let mut next = 0;
        unsafe {
            store.GetText(
                0,
                -1,
                &mut buffer,
                &mut count,
                &mut runs,
                &mut run_count,
                &mut next,
            )
        }?;
        Ok(String::from_utf16_lossy(&buffer[..count as usize]))
    }

    #[test]
    fn reads_and_writes_go_through_a_lock_and_land_in_the_mirror() {
        let rig = rig("abc", 1);
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
                "aXYbc"
            ]
        );
        assert_eq!(rig.heard.borrow().grants, vec![LF_READWRITE]);
        assert_eq!(
            *rig.doc.text.borrow(),
            "abc",
            "the field never sees the writes"
        );
        assert!(rig.state.inner.borrow().mirror.written());
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
    fn the_documents_changes_are_reported_outside_a_lock_and_writes_put_back() {
        let rig = rig("abc", 3);
        *rig.doc.text.borrow_mut() = "abcd".to_owned();
        rig.doc.caret.set(4);
        rig.state.sync();
        assert_eq!(rig.heard.borrow().changes, vec![(3, 3, 4)]);
        assert_eq!(rig.heard.borrow().selections, 1);

        // A text service writes; the next sync gives the document back.
        let store = rig.store.clone();
        rig.during_grant(move || {
            unsafe { store.SetText(0, 0, 0, &[u16::from(b'Z')]) }.unwrap();
        });
        assert_eq!(
            unsafe { rig.store.RequestLock(LF_READWRITE | LF_SYNC) }.unwrap(),
            S_OK
        );
        rig.state.sync();
        assert_eq!(rig.state.inner.borrow().mirror.text(), "abcd");
        assert!(!rig.state.inner.borrow().mirror.written());
        assert_eq!(
            rig.heard.borrow().changes.last(),
            Some(&(0, 1, 0)),
            "the written Z goes: the smallest change"
        );
    }

    #[test]
    fn no_sync_while_a_composition_is_in_progress() {
        let rig = rig("abc", 3);
        let composition: ITfContextOwnerCompositionSink = rig.store.cast().unwrap();
        assert!(
            unsafe { composition.OnStartComposition(None) }
                .unwrap()
                .as_bool()
        );
        *rig.doc.text.borrow_mut() = "abcd".to_owned();
        rig.state.sync();
        assert!(rig.heard.borrow().changes.is_empty());
        unsafe { composition.OnEndComposition(None) }.unwrap();
        rig.state.sync();
        assert_eq!(rig.heard.borrow().changes, vec![(3, 3, 4)]);
    }
}
