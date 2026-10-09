// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The macOS session watch: one object observing `NSWorkspace`'s and the
//! distributed notification centre's notifications, and a reading of the
//! session for [`SessionWatch::recheck`](super::SessionWatch::recheck).

use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, Sel};
use objc2::{DefinedClass, MainThreadMarker, define_class, msg_send, sel};
use objc2_app_kit::{
    NSWorkspace, NSWorkspaceScreensDidSleepNotification, NSWorkspaceScreensDidWakeNotification,
    NSWorkspaceSessionDidBecomeActiveNotification, NSWorkspaceSessionDidResignActiveNotification,
};
use objc2_core_foundation::{CFBoolean, CFString, CFType};
use objc2_core_graphics::{CGDisplayIsAsleep, CGMainDisplayID, CGSessionCopyCurrentDictionary};
use objc2_foundation::{
    NSDistributedNotificationCenter, NSNotification, NSNotificationName,
    NSNotificationSuspensionBehavior, ns_string,
};

use super::{Reading, Reason, Reasons};

struct ObserverIvars {
    reasons: Arc<Reasons>,
    changed: Box<dyn Fn() + Send + Sync>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TeksiloSessionObserver"]
    #[ivars = ObserverIvars]
    struct SessionObserver;

    unsafe impl NSObjectProtocol for SessionObserver {}

    impl SessionObserver {
        #[unsafe(method(screensDidSleep:))]
        fn screens_did_sleep(&self, _notification: &NSNotification) {
            self.note(Reason::ScreensAsleep, true);
        }

        #[unsafe(method(screensDidWake:))]
        fn screens_did_wake(&self, _notification: &NSNotification) {
            self.note(Reason::ScreensAsleep, false);
        }

        #[unsafe(method(sessionDidResignActive:))]
        fn session_did_resign_active(&self, _notification: &NSNotification) {
            self.note(Reason::SessionInactive, true);
        }

        #[unsafe(method(sessionDidBecomeActive:))]
        fn session_did_become_active(&self, _notification: &NSNotification) {
            self.note(Reason::SessionInactive, false);
        }

        #[unsafe(method(screenIsLocked:))]
        fn screen_is_locked(&self, _notification: &NSNotification) {
            self.note(Reason::ScreenLocked, true);
        }

        #[unsafe(method(screenIsUnlocked:))]
        fn screen_is_unlocked(&self, _notification: &NSNotification) {
            self.note(Reason::ScreenLocked, false);
        }
    }
);

impl SessionObserver {
    fn note(&self, reason: Reason, on: bool) {
        let ivars = self.ivars();
        if ivars.reasons.set(reason, on) {
            (ivars.changed)();
        }
    }
}

/// The registered observer; dropping it removes it from both centres.
pub(super) struct Observer {
    object: Retained<SessionObserver>,
}

impl Observer {
    /// Register, on the main thread: the distributed centre delivers to the
    /// run loop of the thread that registered. `None` off it.
    pub(super) fn new(reasons: Arc<Reasons>, changed: Box<dyn Fn() + Send + Sync>) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let this = mtm
            .alloc::<SessionObserver>()
            .set_ivars(ObserverIvars { reasons, changed });
        let object: Retained<SessionObserver> = unsafe { msg_send![super(this), init] };

        // SAFETY: AppKit's own notification names, read once.
        let workspace_names: [(Sel, &NSNotificationName); 4] = unsafe {
            [
                (
                    sel!(screensDidSleep:),
                    NSWorkspaceScreensDidSleepNotification,
                ),
                (sel!(screensDidWake:), NSWorkspaceScreensDidWakeNotification),
                (
                    sel!(sessionDidResignActive:),
                    NSWorkspaceSessionDidResignActiveNotification,
                ),
                (
                    sel!(sessionDidBecomeActive:),
                    NSWorkspaceSessionDidBecomeActiveNotification,
                ),
            ]
        };
        let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
        for (selector, name) in workspace_names {
            // SAFETY: `object` implements `selector`, taking the notification.
            unsafe {
                workspace.addObserver_selector_name_object(&object, selector, Some(name), None)
            };
        }

        // Cocoa suspends the distributed centre's delivery while the
        // application is inactive, as it is under the lock screen: without
        // `DeliverImmediately` the lock would be heard at the unlock.
        let distributed = NSDistributedNotificationCenter::defaultCenter();
        for (selector, name) in [
            (
                sel!(screenIsLocked:),
                ns_string!("com.apple.screenIsLocked"),
            ),
            (
                sel!(screenIsUnlocked:),
                ns_string!("com.apple.screenIsUnlocked"),
            ),
        ] {
            // SAFETY: as above.
            unsafe {
                distributed.addObserver_selector_name_object_suspensionBehavior(
                    &object,
                    selector,
                    Some(name),
                    None,
                    NSNotificationSuspensionBehavior::DeliverImmediately,
                )
            };
        }
        Some(Self { object })
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        // SAFETY: removing an observer this registered.
        unsafe {
            NSWorkspace::sharedWorkspace()
                .notificationCenter()
                .removeObserver(&self.object);
            NSDistributedNotificationCenter::defaultCenter().removeObserver(&self.object);
        }
    }
}

/// What the system reports now: the main display's sleep, and the console
/// and lock flags of the current session.
pub(super) fn reading() -> Reading {
    let screens_asleep = Some(CGDisplayIsAsleep(CGMainDisplayID()));
    let Some(session) = CGSessionCopyCurrentDictionary() else {
        return Reading {
            screens_asleep,
            ..Reading::default()
        };
    };
    let flag = |key: &'static str| -> Option<bool> {
        let key = CFString::from_static_str(key);
        // SAFETY: the session dictionary's keys are CFStrings, and the value
        // it returns is borrowed from the dictionary, which outlives it here.
        let value = unsafe { session.value((&*key as *const CFString).cast()) };
        let value = unsafe { value.cast::<CFType>().as_ref() }?;
        value.downcast_ref::<CFBoolean>().map(CFBoolean::as_bool)
    };
    Reading {
        screens_asleep,
        // `kCGSessionOnConsoleKey`.
        on_console: flag("kCGSSessionOnConsoleKey"),
        // Undocumented, and present only while the screen is locked.
        locked: Some(flag("CGSSessionScreenIsLocked").unwrap_or(false)),
    }
}
