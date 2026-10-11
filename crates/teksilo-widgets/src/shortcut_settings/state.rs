// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The state a [`ShortcutSettings`](super::ShortcutSettings) shares between
//! its toolbar, its list and the key captures it arms, and the rules that
//! decide what a captured chord does.
//!
//! Everything that changes what the list shows is a [`Signal`] the list
//! rebuilds on. What must survive that rebuild (which control holds focus,
//! the armed capture) lives in plain cells beside them.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use teksilo_core::announcer::Politeness;
use teksilo_core::event::{Key, Modifiers};
use teksilo_core::shortcut::{CaptureHandle, KeyStroke, ShortcutRegistry};
use teksilo_core::signal::Signal;
use teksilo_core::widget::EventContext;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::tr_widget;

use super::ShortcutConflict;
use crate::keystroke_format::{command_modifier_labels, format_keystroke};

/// Which keystroke slot of a shortcut a control edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum SlotKind {
    Primary,
    Secondary,
}

/// One slot of one shortcut: what a recording writes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct CaptureTarget {
    pub(super) id: &'static str,
    pub(super) slot: SlotKind,
}

impl CaptureTarget {
    pub(super) fn new(id: &'static str, slot: SlotKind) -> Self {
        Self { id, slot }
    }
}

/// A focusable control of the list, named by what it does rather than by
/// its widget id, so focus can find it again after the list rebuilds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum FocusKey {
    /// The chord control of one slot (its chips, its "Not set", its `+`,
    /// or its recording field: all one control, in different states).
    Slot(CaptureTarget),
    /// A row's reset button.
    Reset(&'static str),
    /// "Reassign" in a row's conflict line.
    Reassign(&'static str),
    /// "Cancel" in a row's conflict line.
    KeepBinding(&'static str),
    /// "Reset all" in the reset-all confirmation line.
    ResetAllConfirm,
    /// "Cancel" in the reset-all confirmation line.
    ResetAllCancel,
}

/// A rebind held back until the user confirms it, because the chord already
/// belongs to another shortcut.
#[derive(Debug, Clone)]
pub(super) struct PendingRebind {
    pub(super) target: CaptureTarget,
    pub(super) ks: KeyStroke,
    pub(super) conflict_id: String,
    pub(super) conflict_slot: Option<SlotKind>,
    pub(super) conflict_name: String,
}

/// A chord the recording refused, and why. The recording stays armed.
#[derive(Debug, Clone)]
pub(super) struct Rejection {
    pub(super) target: CaptureTarget,
    pub(super) message: String,
}

/// State shared by the widget, its list and the captures it arms.
pub(super) struct Shared {
    /// The slot being recorded, if any.
    pub(super) capturing: Signal<Option<CaptureTarget>>,
    /// Whether "Find by keys" is waiting for a chord.
    pub(super) finding: Signal<bool>,
    /// A conflicting rebind awaiting confirmation.
    pub(super) pending: Signal<Option<PendingRebind>>,
    /// The last chord the recording refused.
    pub(super) rejection: Signal<Option<Rejection>>,
    /// Whether the reset-all confirmation is showing.
    pub(super) confirm_reset_all: Signal<bool>,
    /// The search text.
    pub(super) filter: Signal<String>,
    /// The chord "Find by keys" captured. It only filters while the search
    /// text still reads as that chord, so editing the text drops it.
    pub(super) chord_filter: Signal<Option<KeyStroke>>,
    /// Show only the shortcuts the user changed.
    pub(super) modified_only: Signal<bool>,
    /// Whether any shortcut carries a user override. Written by the list,
    /// read by the toolbar's "Reset all".
    pub(super) has_overrides: Signal<bool>,
    /// The list's one Tab stop.
    pub(super) roving: Signal<Option<FocusKey>>,
    /// The list control holding focus. Cleared when focus leaves it, and
    /// *not* when the rebuild destroys it (a destroyed node gets no focus
    /// event), which is how the rebuild knows to give focus back.
    pub(super) focus: Cell<Option<FocusKey>>,
    /// The ids of the overridden shortcuts, as of the list's last build.
    pub(super) overridden_ids: RefCell<Vec<&'static str>>,
    /// Set by "Find by keys"; the next list build speaks the match count.
    pub(super) announce_find_result: Cell<bool>,
    /// The toolbar's search field, where focus goes after "Reset all".
    pub(super) search_field: Cell<Option<WidgetId>>,
    /// The toolbar's "Reset all", where focus goes when its confirmation
    /// is cancelled.
    pub(super) reset_all_button: Cell<Option<WidgetId>>,
    /// The armed capture. Dropping it cancels the capture.
    pub(super) active_handle: RefCell<Option<CaptureHandle>>,
    /// Ask before taking a chord from another shortcut.
    pub(super) confirm: bool,
    /// Told of every chord taken (or about to be taken) from another shortcut.
    pub(super) on_conflict: Option<Rc<dyn Fn(&ShortcutConflict)>>,
}

impl Shared {
    pub(super) fn new(
        filter: Signal<String>,
        modified_only: Signal<bool>,
        confirm: bool,
        on_conflict: Option<Rc<dyn Fn(&ShortcutConflict)>>,
    ) -> Self {
        Self {
            capturing: Signal::new(None),
            finding: Signal::new(false),
            pending: Signal::new(None),
            rejection: Signal::new(None),
            confirm_reset_all: Signal::new(false),
            filter,
            chord_filter: Signal::new(None),
            modified_only,
            has_overrides: Signal::new(false),
            roving: Signal::new(None),
            focus: Cell::new(None),
            overridden_ids: RefCell::new(Vec::new()),
            announce_find_result: Cell::new(false),
            search_field: Cell::new(None),
            reset_all_button: Cell::new(None),
            active_handle: RefCell::new(None),
            confirm,
            on_conflict,
        }
    }

    /// Drop any armed capture and leave recording and finding.
    pub(super) fn cancel_capture(&self) {
        self.active_handle.borrow_mut().take();
        if self.capturing.get().is_some() {
            self.capturing.set(None);
        }
        if self.finding.get() {
            self.finding.set(false);
        }
        if self.rejection.get().is_some() {
            self.rejection.set(None);
        }
    }
}

/// What a chord captured while recording asks for.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Recorded {
    /// Bare Escape, or Tab / Shift+Tab: stop recording, change nothing.
    Cancel,
    /// Bare Delete or Backspace: remove the slot's chord.
    Clear,
    /// A chord that cannot be a shortcut; the message says why.
    Reject(String),
    /// A chord to assign.
    Chord(KeyStroke),
}

/// Classify a chord captured while recording.
pub(super) fn classify_recorded(ks: KeyStroke) -> Recorded {
    let bare = ks.modifiers == Modifiers::NONE;
    if bare && ks.key == Key::Escape {
        return Recorded::Cancel;
    }
    // Tab is how a keyboard user leaves the control, so it never records.
    if ks.key == Key::Tab && (bare || ks.modifiers == Modifiers::SHIFT) {
        return Recorded::Cancel;
    }
    if bare && matches!(ks.key, Key::Delete | Key::Backspace) {
        return Recorded::Clear;
    }
    match rejection_message(ks) {
        Some(message) => Recorded::Reject(message),
        None => Recorded::Chord(ks),
    }
}

/// Why `ks` cannot be a shortcut, or `None` when it can.
pub(super) fn rejection_message(ks: KeyStroke) -> Option<String> {
    let chord = format_keystroke(ks);
    if matches!(ks.key, Key::CapsLock | Key::ContextMenu) {
        return Some(tr_widget!(shortcut_settings_unusable(chord = chord.clone())).resolve_now());
    }
    let m = ks.modifiers;
    // Shift alone does not make a shortcut: Shift+A types a capital A.
    if !(m.ctrl() || m.alt() || m.super_key()) && !is_function_key(ks.key) {
        let (first, second) = command_modifier_labels();
        return Some(
            tr_widget!(shortcut_settings_needs_modifier(
                chord = chord.clone(),
                first = first,
                second = second
            ))
            .resolve_now(),
        );
    }
    if is_reserved(ks) {
        return Some(tr_widget!(shortcut_settings_reserved(chord = chord)).resolve_now());
    }
    None
}

fn is_function_key(key: Key) -> bool {
    matches!(
        key,
        Key::F1
            | Key::F2
            | Key::F3
            | Key::F4
            | Key::F5
            | Key::F6
            | Key::F7
            | Key::F8
            | Key::F9
            | Key::F10
            | Key::F11
            | Key::F12
            | Key::F13
            | Key::F14
            | Key::F15
            | Key::F16
            | Key::F17
            | Key::F18
            | Key::F19
            | Key::F20
            | Key::F21
            | Key::F22
            | Key::F23
            | Key::F24
    )
}

/// Chords the system or the framework keeps for itself. Binding one would
/// either never fire or take away something the user relies on.
pub(super) fn is_reserved(ks: KeyStroke) -> bool {
    let m = ks.modifiers;
    let (ctrl, shift, alt, sup) = (
        Modifiers::CTRL,
        Modifiers::SHIFT,
        Modifiers::ALT,
        Modifiers::SUPER,
    );
    // Ctrl+Tab and Ctrl+Shift+Tab leave a control that keeps Tab for itself
    // (WCAG 2.1.2), on every platform, macOS included (they stay literal).
    if ks.key == Key::Tab && (m == ctrl || m == ctrl | shift) {
        return true;
    }
    if cfg!(target_os = "macos") {
        // App switcher, Spotlight, Force Quit, Lock Screen.
        (ks.key == Key::Tab && (m == sup || m == sup | shift))
            || (ks.key == Key::Space && m == sup)
            || (ks.key == Key::Escape && m == sup | alt)
            || (ks.key == Key::Q && m == ctrl | sup)
    } else {
        // Close window, window switcher, window menu, secure attention,
        // lock screen.
        (ks.key == Key::F4 && m == alt)
            || (ks.key == Key::Tab && (m == alt || m == alt | shift))
            || (ks.key == Key::Space && m == alt)
            || (ks.key == Key::Delete && m == ctrl | alt)
            || (ks.key == Key::L && m == sup)
    }
}

/// Start recording `target`, or stop when it is already recording.
pub(super) fn start_recording(shared: &Rc<Shared>, ctx: &mut EventContext, target: CaptureTarget) {
    if shared.capturing.get() == Some(target) {
        shared.cancel_capture();
        return;
    }
    shared.cancel_capture();
    if shared.pending.get().is_some() {
        shared.pending.set(None);
    }
    shared.focus.set(Some(FocusKey::Slot(target)));
    shared.roving.set(Some(FocusKey::Slot(target)));
    shared.capturing.set(Some(target));
    arm_recording(Rc::downgrade(shared), ctx, target);
}

/// Arm the capture that records `target`. Re-armed from inside itself when
/// it refuses a chord, so the user can try another.
fn arm_recording(weak: Weak<Shared>, ctx: &mut EventContext, target: CaptureTarget) {
    // The capture holds the state weakly: the state holds the capture's
    // handle, and a strong reference back would keep both alive forever.
    let for_callback = weak.clone();
    let handle = ctx.begin_key_capture(move |ks, reg, cctx| {
        let Some(shared) = for_callback.upgrade() else {
            return;
        };
        match classify_recorded(ks) {
            Recorded::Cancel => shared.cancel_capture(),
            Recorded::Clear => {
                shared.cancel_capture();
                rebind(reg, target.id, target.slot, None);
            }
            Recorded::Reject(message) => {
                shared.rejection.set(Some(Rejection { target, message }));
                arm_recording(for_callback.clone(), cctx, target);
            }
            Recorded::Chord(ks) => {
                shared.cancel_capture();
                assign(&shared, reg, cctx, target, ks);
            }
        }
    });
    if let Some(shared) = weak.upgrade() {
        *shared.active_handle.borrow_mut() = Some(handle);
    }
}

/// Write `ks` into `target`'s slot, after resolving any conflict with
/// another shortcut that holds it.
///
/// In confirm mode a conflict writes nothing: the rebind is parked in
/// `pending` and the row offers Reassign / Cancel. Otherwise the other
/// shortcut loses the chord at once, so a chord always has one owner.
pub(super) fn assign(
    shared: &Shared,
    reg: &mut ShortcutRegistry,
    ctx: &mut EventContext,
    target: CaptureTarget,
    ks: KeyStroke,
) {
    shared.focus.set(Some(FocusKey::Slot(target)));
    // Assigning a chord the shortcut already has changes nothing.
    if let Some(eff) = reg.effective(target.id)
        && (eff.primary == Some(ks) || eff.secondary == Some(ks))
    {
        return;
    }
    let conflict = reg.find_conflict(ks, Some(target.id)).map(str::to_string);
    let Some(conflict_id) = conflict else {
        rebind(reg, target.id, target.slot, Some(ks));
        return;
    };
    let conflict_slot = reg.effective(&conflict_id).and_then(|eff| {
        if eff.primary == Some(ks) {
            Some(SlotKind::Primary)
        } else if eff.secondary == Some(ks) {
            Some(SlotKind::Secondary)
        } else {
            None
        }
    });
    let conflict_name = reg
        .effective(&conflict_id)
        .map(|e| e.shortcut.name.get())
        .unwrap_or_else(|| conflict_id.clone());

    if let Some(cb) = &shared.on_conflict {
        cb(&ShortcutConflict {
            displaced_id: conflict_id.clone(),
            displaced_name: conflict_name.clone(),
            keystroke: ks,
        });
    }

    if shared.confirm {
        shared.focus.set(Some(FocusKey::Reassign(target.id)));
        shared.pending.set(Some(PendingRebind {
            target,
            ks,
            conflict_id,
            conflict_slot,
            conflict_name,
        }));
        return;
    }

    if let Some(slot) = conflict_slot {
        rebind(reg, &conflict_id, slot, None);
    }
    rebind(reg, target.id, target.slot, Some(ks));
    announce_removed(ctx, ks, &conflict_name);
}

/// Speak which shortcut lost a chord to the one being edited.
pub(super) fn announce_removed(ctx: &mut EventContext, ks: KeyStroke, from: &str) {
    ctx.announce_with(
        tr_widget!(a11y_shortcut_settings_removed_from(
            chord = format_keystroke(ks),
            command = from.to_string()
        ))
        .resolve_now(),
        Politeness::Polite,
    );
}

/// Write one slot of the registry directly (inside a capture).
pub(super) fn rebind(reg: &mut ShortcutRegistry, id: &str, slot: SlotKind, ks: Option<KeyStroke>) {
    match slot {
        SlotKind::Primary => reg.rebind_primary(id.to_string(), ks),
        SlotKind::Secondary => reg.rebind_secondary(id.to_string(), ks),
    }
}

/// Queue a write of one slot from an ordinary handler.
pub(super) fn rebind_deferred(
    ctx: &mut EventContext,
    id: &str,
    slot: SlotKind,
    ks: Option<KeyStroke>,
) {
    match slot {
        SlotKind::Primary => ctx.rebind_shortcut_primary(id.to_string(), ks),
        SlotKind::Secondary => ctx.rebind_shortcut_secondary(id.to_string(), ks),
    }
}

/// Confirm a parked conflict: the other shortcut loses the chord, the
/// edited one takes it.
pub(super) fn confirm_pending(shared: &Shared, ctx: &mut EventContext) {
    let Some(p) = shared.pending.get() else {
        return;
    };
    if let Some(slot) = p.conflict_slot {
        rebind_deferred(ctx, &p.conflict_id, slot, None);
    }
    rebind_deferred(ctx, p.target.id, p.target.slot, Some(p.ks));
    announce_removed(ctx, p.ks, &p.conflict_name);
    shared.focus.set(Some(FocusKey::Slot(p.target)));
    shared.pending.set(None);
}

/// Drop a parked conflict, leaving both shortcuts as they were.
pub(super) fn cancel_pending(shared: &Shared) {
    let Some(p) = shared.pending.get() else {
        return;
    };
    shared.focus.set(Some(FocusKey::Slot(p.target)));
    shared.pending.set(None);
}

/// Start (or stop) "Find by keys": the next chord becomes the filter.
pub(super) fn start_finding(shared: &Rc<Shared>, ctx: &mut EventContext) {
    if shared.finding.get() {
        shared.cancel_capture();
        return;
    }
    shared.cancel_capture();
    shared.finding.set(true);
    let weak = Rc::downgrade(shared);
    let handle = ctx.begin_key_capture(move |ks, _reg, _cctx| {
        let Some(shared) = weak.upgrade() else {
            return;
        };
        shared.finding.set(false);
        let bare = ks.modifiers == Modifiers::NONE;
        if (bare && ks.key == Key::Escape) || (ks.key == Key::Tab && bare) {
            return;
        }
        shared.chord_filter.set(Some(ks));
        shared.filter.set(format_keystroke(ks));
        shared.announce_find_result.set(true);
    });
    *shared.active_handle.borrow_mut() = Some(handle);
}
