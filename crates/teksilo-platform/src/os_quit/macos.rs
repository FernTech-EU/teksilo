// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `applicationShouldTerminate:` on winit's application delegate.
//!
//! winit 0.30 declares its delegate class (`WinitApplicationDelegate`) with
//! `applicationDidFinishLaunching:` and `applicationWillTerminate:` only,
//! so AppKit, finding no `applicationShouldTerminate:`, ends the process
//! for every `terminate:`: the Dock menu's Quit, a menu item bound to
//! `terminate:`, AppleScript's `quit`, a logout. The method is added to
//! that class at run time (`class_addMethod`, which adds it only where the
//! class has none), and the delegate is set again so AppKit reads what it
//! responds to anew.
//!
//! The method answers `NSTerminateLater` and posts a
//! [`TerminateRequest`]. AppKit then runs the main run loop in
//! `NSModalPanelRunLoopMode` until it is answered. winit's run-loop
//! observers, its wake timer and its event-loop proxy's source are all
//! scheduled in the common modes, which include that one, so the posted
//! request and every window event (a close guard's dialog, the user's
//! click on it) are still delivered: the observer that delivers them only
//! stands aside while a winit callback is running, and AppKit asks the
//! delegate from its own event dispatch, between callbacks. (What winit
//! queues in the default mode alone is an event raised *during* a
//! callback, such as the effects of a native modal run from a handler.)
//!
//! The reply is handed to the main dispatch queue rather than given on the
//! spot: the quit is decided inside a winit callback, and AppKit, told
//! `YES`, goes on to `applicationWillTerminate:`, which winit answers by
//! calling into the same handler. The main queue is served in the common
//! modes too.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{msg_send, sel};
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply};
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager};

use super::TerminateRequest;

/// Where a request goes, set by [`install_terminate_hook`].
type Post = Box<dyn Fn(TerminateRequest)>;

thread_local! {
    /// The hook, on the main thread, where AppKit calls the delegate.
    static POST: RefCell<Option<Post>> = const { RefCell::new(None) };
}

/// An `NSTerminateLater` is waiting for its reply: a second reply, which
/// AppKit would take as a new decision, is not given.
static AWAITING_REPLY: AtomicBool = AtomicBool::new(false);

/// A four-character Apple Event code.
const fn four_cc(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

/// `keyAEQuitReason`, the attribute of a quit Apple Event that says why.
const QUIT_REASON: u32 = four_cc(b"why?");

/// The reasons that mean the session is ending: `kAELogOut`,
/// `kAEReallyLogOut`, `kAEShowRestartDialog`, `kAEShowShutdownDialog`,
/// `kAERestart`, `kAEShutDown`.
const SESSION_END_REASONS: [u32; 6] = [
    four_cc(b"logo"),
    four_cc(b"rlgo"),
    four_cc(b"rrst"),
    four_cc(b"rsdn"),
    four_cc(b"rest"),
    four_cc(b"shut"),
];

/// Make every `terminate:` ask the application: from now on AppKit's
/// `applicationShouldTerminate:` answers `NSTerminateLater` and hands a
/// [`TerminateRequest`] to `post`, on the main thread. Call it once the
/// event loop has launched (from `resumed`), on the main thread. Returns
/// whether the method is in place; `false` off the main thread, with no
/// delegate, or when the delegate's class already had one (winit declared
/// its own), in which case nothing changes.
pub fn install_terminate_hook(post: impl Fn(TerminateRequest) + 'static) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(delegate) = app.delegate() else {
        return false;
    };
    POST.with(|slot| *slot.borrow_mut() = Some(Box::new(post)));
    let object: &AnyObject = delegate.as_ref();
    let class: &AnyClass = object.class();
    let selector = sel!(applicationShouldTerminate:);
    if class.instance_method(selector).is_some() {
        // Ours from an earlier run, or winit's own: either way it is there.
        return class_has_ours(class, selector);
    }
    let method: extern "C-unwind" fn(&AnyObject, Sel, &AnyObject) -> NSApplicationTerminateReply =
        application_should_terminate;
    // SAFETY: `Imp` is the untyped form of a method implementation; the
    // types string below describes this function's signature: an
    // `NSUInteger` return (`Q`), the receiver (`@`), the selector (`:`)
    // and the sender (`@`).
    let added = unsafe {
        let imp: Imp = std::mem::transmute::<
            extern "C-unwind" fn(&AnyObject, Sel, &AnyObject) -> NSApplicationTerminateReply,
            Imp,
        >(method);
        objc2::ffi::class_addMethod(
            class as *const AnyClass as *mut AnyClass,
            selector,
            imp,
            c"Q@:@".as_ptr(),
        )
    };
    if !added.as_bool() {
        return false;
    }
    // Set the delegate again, so AppKit reads what it responds to anew.
    // SAFETY: the same delegate, on the main thread.
    unsafe {
        let _: () = msg_send![&*app, setDelegate: object];
    }
    true
}

/// Whether the `applicationShouldTerminate:` of `class` is the one added
/// here.
fn class_has_ours(class: &AnyClass, selector: Sel) -> bool {
    let ours: extern "C-unwind" fn(&AnyObject, Sel, &AnyObject) -> NSApplicationTerminateReply =
        application_should_terminate;
    class
        .instance_method(selector)
        .is_some_and(|method| method.implementation() as usize == ours as usize)
}

/// AppKit's question. `NSTerminateNow` with no hook installed, as AppKit
/// would do without the method.
extern "C-unwind" fn application_should_terminate(
    _this: &AnyObject,
    _cmd: Sel,
    _sender: &AnyObject,
) -> NSApplicationTerminateReply {
    let session_end = quitting_for_session_end();
    let posted = POST.with(|slot| match slot.try_borrow() {
        Ok(slot) => match slot.as_ref() {
            Some(post) => {
                AWAITING_REPLY.store(true, Ordering::Release);
                post(TerminateRequest::new(session_end, reply_on_main_queue));
                true
            }
            None => false,
        },
        Err(_) => false,
    });
    if posted {
        NSApplicationTerminateReply::TerminateLater
    } else {
        NSApplicationTerminateReply::TerminateNow
    }
}

/// Give AppKit its answer, from the main dispatch queue, once.
fn reply_on_main_queue(terminate: bool) {
    dispatch2::DispatchQueue::main().exec_async(move || {
        if !AWAITING_REPLY.swap(false, Ordering::AcqRel) {
            return;
        }
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm).replyToApplicationShouldTerminate(terminate);
        }
    });
}

/// Whether the Apple Event being handled is a quit sent because the
/// session is ending: its `keyAEQuitReason` is a logout, a restart or a
/// shutdown. `false` for a quit with no reason (the Dock, AppleScript) and
/// for a `terminate:` that came from no Apple Event (a menu item).
fn quitting_for_session_end() -> bool {
    let manager = NSAppleEventManager::sharedAppleEventManager();
    let Some(event) = manager.currentAppleEvent() else {
        return false;
    };
    // SAFETY: `attributeDescriptorForKeyword:` takes an `AEKeyword`
    // (a 32-bit four-character code) and returns a descriptor or nil.
    let reason: Option<Retained<NSAppleEventDescriptor>> =
        unsafe { msg_send![&*event, attributeDescriptorForKeyword: QUIT_REASON] };
    let Some(reason) = reason else {
        return false;
    };
    // SAFETY: `enumCodeValue` returns an `OSType` (a 32-bit code).
    let code: u32 = unsafe { msg_send![&*reason, enumCodeValue] };
    SESSION_END_REASONS.contains(&code)
}
