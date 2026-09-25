// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The gate against a fake registry, and the X11 terms each key is reported in.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use winit::event::ElementState::{Pressed, Released};
use winit::keyboard::{
    Key, KeyCode, KeyLocation, ModifiersState, NamedKey, NativeKey, PhysicalKey,
};

use super::{
    DeviceEvent, DeviceEventKind, KeyDisposition, KeyEventReporter, KeyInput, KeyReportGate,
    KeyTarget, KeyboardState, ReportOutcome,
};

/// A key landing where a reader is attached, and where none is.
const READER: KeyTarget = KeyTarget {
    reader_attached: true,
    secure_field: false,
};
const NO_READER: KeyTarget = KeyTarget {
    reader_attached: false,
    secure_field: false,
};
/// A key landing in a secure field (a `PasswordField`) with a reader attached.
const SECURE: KeyTarget = KeyTarget {
    reader_attached: true,
    secure_field: true,
};

/// A registry that records every report and answers presses from a script,
/// `NotConsumed` once the script runs out.
#[derive(Default, Clone)]
struct FakeRegistry {
    seen: Rc<RefCell<Vec<DeviceEvent>>>,
    answers: Rc<RefCell<VecDeque<ReportOutcome>>>,
}

impl FakeRegistry {
    fn answering(answers: &[ReportOutcome]) -> Self {
        let fake = Self::default();
        fake.answers.borrow_mut().extend(answers.iter().copied());
        fake
    }

    fn seen(&self) -> Vec<DeviceEvent> {
        self.seen.borrow().clone()
    }
}

impl KeyEventReporter for FakeRegistry {
    fn report_press(&mut self, event: &DeviceEvent) -> ReportOutcome {
        self.seen.borrow_mut().push(event.clone());
        self.answers
            .borrow_mut()
            .pop_front()
            .unwrap_or(ReportOutcome::NotConsumed)
    }

    fn report_release(&mut self, event: &DeviceEvent) {
        self.seen.borrow_mut().push(event.clone());
    }
}

fn named(key: NamedKey) -> Key {
    Key::Named(key)
}

fn character(text: &str) -> Key {
    Key::Character(text.into())
}

fn input(logical: &Key, code: KeyCode, state: winit::event::ElementState) -> KeyInput<'_> {
    KeyInput {
        physical_key: PhysicalKey::Code(code),
        logical_key: logical,
        location: KeyLocation::Standard,
        state,
        repeat: false,
        synthetic: false,
    }
}

/// The same key pressed again by the keyboard's autorepeat.
fn repeated(input: KeyInput<'_>) -> KeyInput<'_> {
    KeyInput {
        repeat: true,
        ..input
    }
}

fn at(input: KeyInput<'_>, location: KeyLocation) -> KeyInput<'_> {
    KeyInput { location, ..input }
}

fn plain() -> KeyboardState {
    KeyboardState::default()
}

fn holding(modifiers: ModifiersState) -> KeyboardState {
    KeyboardState {
        modifiers,
        caps_lock: false,
    }
}

fn event(key: KeyInput<'_>, keyboard: KeyboardState) -> DeviceEvent {
    DeviceEvent::for_key(&key, keyboard, false, false, 0)
}

// ---------------------------------------------------------------------------
// The struct each key is reported as
// ---------------------------------------------------------------------------

/// The key of the defect: Down in a text editor, which Orca reads as "the
/// next line" only when it knows the key was Down.
#[test]
fn down_is_reported_as_the_x_keysym_down() {
    let down = named(NamedKey::ArrowDown);
    let press = event(input(&down, KeyCode::ArrowDown, Pressed), plain());
    assert_eq!(press.kind, DeviceEventKind::Pressed);
    assert_eq!(press.keysym, 0xff54, "XK_Down");
    assert_eq!(press.modifiers, 0);
    assert_eq!(
        press.event_string, "",
        "a key with no text is named by its keysym"
    );
    assert!(!press.is_text);

    let release = event(input(&down, KeyCode::ArrowDown, Released), plain());
    assert_eq!(release.kind, DeviceEventKind::Released);
    assert_eq!(release.keysym, 0xff54);
}

#[test]
fn named_keys_carry_their_x_keysyms() {
    use KeyLocation::{Left, Numpad, Right, Standard};
    let rows: &[(NamedKey, KeyLocation, bool, u32, &str)] = &[
        (NamedKey::ArrowUp, Standard, false, 0xff52, "Up"),
        (NamedKey::ArrowLeft, Standard, false, 0xff51, "Left"),
        (NamedKey::ArrowRight, Standard, false, 0xff53, "Right"),
        (NamedKey::Home, Standard, false, 0xff50, "Home"),
        (NamedKey::End, Standard, false, 0xff57, "End"),
        (NamedKey::PageUp, Standard, false, 0xff55, "Page_Up"),
        (NamedKey::PageDown, Standard, false, 0xff56, "Page_Down"),
        (NamedKey::Enter, Standard, false, 0xff0d, "Return"),
        (NamedKey::Tab, Standard, false, 0xff09, "Tab"),
        (NamedKey::Tab, Standard, true, 0xfe20, "ISO_Left_Tab"),
        (NamedKey::Backspace, Standard, false, 0xff08, "BackSpace"),
        (NamedKey::Delete, Standard, false, 0xffff, "Delete"),
        (NamedKey::Escape, Standard, false, 0xff1b, "Escape"),
        (NamedKey::Insert, Standard, false, 0xff63, "Insert"),
        (NamedKey::F1, Standard, false, 0xffbe, "F1"),
        (NamedKey::F10, Standard, false, 0xffc7, "F10"),
        (NamedKey::F12, Standard, false, 0xffc9, "F12"),
        (NamedKey::CapsLock, Standard, false, 0xffe5, "Caps_Lock"),
        (NamedKey::NumLock, Numpad, false, 0xff7f, "Num_Lock"),
        (NamedKey::Shift, Left, false, 0xffe1, "Shift_L"),
        (NamedKey::Shift, Right, false, 0xffe2, "Shift_R"),
        (NamedKey::Control, Left, false, 0xffe3, "Control_L"),
        (NamedKey::Control, Right, false, 0xffe4, "Control_R"),
        (NamedKey::Alt, Left, false, 0xffe9, "Alt_L"),
        (NamedKey::Super, Left, false, 0xffeb, "Super_L"),
        (NamedKey::AltGraph, Right, false, 0xfe03, "ISO_Level3_Shift"),
        (NamedKey::ContextMenu, Standard, false, 0xff67, "Menu"),
        (NamedKey::Space, Standard, false, 0x20, "space"),
        (NamedKey::Enter, Numpad, false, 0xff8d, "KP_Enter"),
        (NamedKey::Insert, Numpad, false, 0xff9e, "KP_Insert"),
        (NamedKey::End, Numpad, false, 0xff9c, "KP_End"),
        (NamedKey::ArrowDown, Numpad, false, 0xff99, "KP_Down"),
    ];
    for &(key, location, shift, want, name) in rows {
        let logical = named(key);
        let keyboard = if shift {
            holding(ModifiersState::SHIFT)
        } else {
            plain()
        };
        let got = event(
            at(input(&logical, KeyCode::KeyA, Pressed), location),
            keyboard,
        );
        assert_eq!(
            got.keysym, want,
            "{key:?} at {location:?} (shift {shift}) should be {name} {want:#x}, got {:#x}",
            got.keysym
        );
    }
}

#[test]
fn characters_are_latin_1_or_unicode_keysyms_and_carry_their_text() {
    let rows: &[(&str, u32)] = &[
        ("a", 0x61),
        ("A", 0x41),
        ("é", 0xe9),
        ("ÿ", 0xff),
        ("€", 0x0100_20ac),
        ("ж", 0x0100_0436),
    ];
    for &(text, want) in rows {
        let logical = character(text);
        let got = event(input(&logical, KeyCode::KeyA, Pressed), plain());
        assert_eq!(
            got.keysym, want,
            "{text:?} should be {want:#x}, got {:#x}",
            got.keysym
        );
        assert_eq!(got.event_string, text);
        assert!(got.is_text, "{text:?} types itself");
    }
}

#[test]
fn keypad_characters_are_keypad_keysyms() {
    let rows: &[(&str, u32)] = &[("1", 0xffb1), ("0", 0xffb0), (".", 0xffae), ("+", 0xffab)];
    for &(text, want) in rows {
        let logical = character(text);
        let got = event(
            at(
                input(&logical, KeyCode::Numpad1, Pressed),
                KeyLocation::Numpad,
            ),
            plain(),
        );
        assert_eq!(
            got.keysym, want,
            "keypad {text:?} should be {want:#x}, got {:#x}",
            got.keysym
        );
        assert_eq!(got.event_string, text);
    }
}

#[test]
fn dead_keys_are_x_dead_keysyms() {
    let rows: &[(Option<char>, u32)] = &[
        (Some('`'), 0xfe50),
        (Some('´'), 0xfe51),
        (Some('^'), 0xfe52),
        (Some('~'), 0xfe53),
        (Some('¨'), 0xfe57),
        (Some('ˇ'), 0xfe5a),
        (None, 0x00ff_ffff),
    ];
    for &(ch, want) in rows {
        let logical = Key::Dead(ch);
        let got = event(input(&logical, KeyCode::BracketLeft, Pressed), plain());
        assert_eq!(
            got.keysym, want,
            "dead {ch:?} should be {want:#x}, got {:#x}",
            got.keysym
        );
        assert!(!got.is_text, "a dead key types nothing by itself");
    }
}

/// winit names only some keysyms; the rest arrive as the raw keysym.
#[test]
fn a_keysym_winit_has_no_name_for_passes_through() {
    let begin = Key::Unidentified(NativeKey::Xkb(0xff9d));
    let got = event(
        at(
            input(&begin, KeyCode::Numpad5, Pressed),
            KeyLocation::Numpad,
        ),
        plain(),
    );
    assert_eq!(got.keysym, 0xff9d, "XK_KP_Begin");
}

#[test]
fn modifiers_are_the_x_modifier_mask() {
    let right = named(NamedKey::ArrowRight);
    let ctrl_right = event(
        input(&right, KeyCode::ArrowRight, Pressed),
        holding(ModifiersState::CONTROL),
    );
    assert_eq!(ctrl_right.modifiers, 1 << 2, "ControlMask");

    let a = character("a");
    let all = event(
        input(&a, KeyCode::KeyA, Pressed),
        KeyboardState {
            modifiers: ModifiersState::SHIFT
                | ModifiersState::CONTROL
                | ModifiersState::ALT
                | ModifiersState::SUPER,
            caps_lock: true,
        },
    );
    assert_eq!(
        all.modifiers,
        (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 6),
        "Shift, Lock, Control, Mod1, Mod4"
    );
}

/// Num Lock, once known to be on, is Mod2 on the event, whatever the key.
#[test]
fn a_known_num_lock_is_mod2() {
    let end = named(NamedKey::End);
    let key = at(input(&end, KeyCode::Numpad1, Pressed), KeyLocation::Numpad);
    assert_eq!(
        DeviceEvent::for_key(&key, plain(), true, false, 0).modifiers,
        1 << 4
    );
    assert_eq!(
        DeviceEvent::for_key(&key, plain(), false, false, 0).modifiers,
        0
    );
}

#[test]
fn a_key_typed_with_control_is_not_text() {
    let a = character("a");
    let got = event(
        input(&a, KeyCode::KeyA, Pressed),
        holding(ModifiersState::CONTROL),
    );
    assert_eq!(got.keysym, 0x61);
    assert_eq!(got.event_string, "a");
    assert!(!got.is_text, "Ctrl+A types nothing");

    let shifted = character("A");
    let got = event(
        input(&shifted, KeyCode::KeyA, Pressed),
        holding(ModifiersState::SHIFT),
    );
    assert!(got.is_text, "Shift+A types A");
}

#[test]
fn space_types_a_space() {
    let space = named(NamedKey::Space);
    let got = event(input(&space, KeyCode::Space, Pressed), plain());
    assert_eq!(got.keysym, 0x20);
    assert_eq!(got.event_string, " ");
    assert!(got.is_text);
}

/// On Linux the physical key is winit's evdev code, and the X keycode is that
/// plus 8. Orca matches its commands on it (Orca+T is a grab on T's keycode).
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn the_hardware_code_is_the_x_keycode() {
    let rows: &[(KeyCode, u32)] = &[
        (KeyCode::ArrowDown, 116),
        (KeyCode::KeyA, 38),
        (KeyCode::KeyT, 28),
        (KeyCode::Insert, 118),
        (KeyCode::Numpad1, 87),
        (KeyCode::Space, 65),
    ];
    let any = named(NamedKey::ArrowDown);
    for &(code, want) in rows {
        let got = event(input(&any, code, Pressed), plain());
        assert_eq!(got.hw_code, want, "{code:?}");
    }
}

/// The keysym table is written in hex; this reads each row back against the
/// named constants of xkbcommon's keysym list.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn the_keysym_table_agrees_with_xkb() {
    use KeyLocation::{Left, Numpad, Right, Standard};
    use xkeysym::key as xk;
    let rows: &[(NamedKey, KeyLocation, bool, u32)] = &[
        (NamedKey::Backspace, Standard, false, xk::BackSpace),
        (NamedKey::Tab, Standard, false, xk::Tab),
        (NamedKey::Tab, Standard, true, xk::ISO_Left_Tab),
        (NamedKey::Clear, Standard, false, xk::Clear),
        (NamedKey::Enter, Standard, false, xk::Return),
        (NamedKey::Pause, Standard, false, xk::Pause),
        (NamedKey::ScrollLock, Standard, false, xk::Scroll_Lock),
        (NamedKey::Escape, Standard, false, xk::Escape),
        (NamedKey::Delete, Standard, false, xk::Delete),
        (NamedKey::Compose, Standard, false, xk::Multi_key),
        (NamedKey::CodeInput, Standard, false, xk::Codeinput),
        (
            NamedKey::SingleCandidate,
            Standard,
            false,
            xk::SingleCandidate,
        ),
        (
            NamedKey::AllCandidates,
            Standard,
            false,
            xk::MultipleCandidate,
        ),
        (
            NamedKey::PreviousCandidate,
            Standard,
            false,
            xk::PreviousCandidate,
        ),
        (NamedKey::KanjiMode, Standard, false, xk::Kanji),
        (NamedKey::NonConvert, Standard, false, xk::Muhenkan),
        (NamedKey::Convert, Standard, false, xk::Henkan_Mode),
        (NamedKey::Romaji, Standard, false, xk::Romaji),
        (NamedKey::Hiragana, Standard, false, xk::Hiragana),
        (
            NamedKey::HiraganaKatakana,
            Standard,
            false,
            xk::Hiragana_Katakana,
        ),
        (NamedKey::Zenkaku, Standard, false, xk::Zenkaku),
        (NamedKey::Hankaku, Standard, false, xk::Hankaku),
        (
            NamedKey::ZenkakuHankaku,
            Standard,
            false,
            xk::Zenkaku_Hankaku,
        ),
        (NamedKey::KanaMode, Standard, false, xk::Kana_Lock),
        (NamedKey::Alphanumeric, Standard, false, xk::Eisu_toggle),
        (NamedKey::Home, Standard, false, xk::Home),
        (NamedKey::ArrowLeft, Standard, false, xk::Left),
        (NamedKey::ArrowUp, Standard, false, xk::Up),
        (NamedKey::ArrowRight, Standard, false, xk::Right),
        (NamedKey::ArrowDown, Standard, false, xk::Down),
        (NamedKey::PageUp, Standard, false, xk::Page_Up),
        (NamedKey::PageDown, Standard, false, xk::Page_Down),
        (NamedKey::End, Standard, false, xk::End),
        (NamedKey::Select, Standard, false, xk::Select),
        (NamedKey::PrintScreen, Standard, false, xk::Print),
        (NamedKey::Execute, Standard, false, xk::Execute),
        (NamedKey::Insert, Standard, false, xk::Insert),
        (NamedKey::Undo, Standard, false, xk::Undo),
        (NamedKey::Redo, Standard, false, xk::Redo),
        (NamedKey::ContextMenu, Standard, false, xk::Menu),
        (NamedKey::Find, Standard, false, xk::Find),
        (NamedKey::Cancel, Standard, false, xk::Cancel),
        (NamedKey::Help, Standard, false, xk::Help),
        (NamedKey::ModeChange, Standard, false, xk::Mode_switch),
        (NamedKey::NumLock, Numpad, false, xk::Num_Lock),
        (NamedKey::Tab, Numpad, false, xk::KP_Tab),
        (NamedKey::Enter, Numpad, false, xk::KP_Enter),
        (NamedKey::F1, Numpad, false, xk::KP_F1),
        (NamedKey::F4, Numpad, false, xk::KP_F4),
        (NamedKey::Home, Numpad, false, xk::KP_Home),
        (NamedKey::ArrowLeft, Numpad, false, xk::KP_Left),
        (NamedKey::ArrowUp, Numpad, false, xk::KP_Up),
        (NamedKey::ArrowRight, Numpad, false, xk::KP_Right),
        (NamedKey::ArrowDown, Numpad, false, xk::KP_Down),
        (NamedKey::PageUp, Numpad, false, xk::KP_Page_Up),
        (NamedKey::PageDown, Numpad, false, xk::KP_Page_Down),
        (NamedKey::End, Numpad, false, xk::KP_End),
        (NamedKey::Insert, Numpad, false, xk::KP_Insert),
        (NamedKey::Delete, Numpad, false, xk::KP_Delete),
        (NamedKey::F1, Standard, false, xk::F1),
        (NamedKey::F12, Standard, false, xk::F12),
        (NamedKey::F13, Standard, false, xk::F13),
        (NamedKey::F24, Standard, false, xk::F24),
        (NamedKey::F35, Standard, false, xk::F35),
        (NamedKey::Shift, Left, false, xk::Shift_L),
        (NamedKey::Shift, Right, false, xk::Shift_R),
        (NamedKey::Control, Left, false, xk::Control_L),
        (NamedKey::Control, Right, false, xk::Control_R),
        (NamedKey::CapsLock, Standard, false, xk::Caps_Lock),
        (NamedKey::Meta, Left, false, xk::Meta_L),
        (NamedKey::Meta, Right, false, xk::Meta_R),
        (NamedKey::Alt, Left, false, xk::Alt_L),
        (NamedKey::Alt, Right, false, xk::Alt_R),
        (NamedKey::Super, Left, false, xk::Super_L),
        (NamedKey::Super, Right, false, xk::Super_R),
        (NamedKey::Hyper, Left, false, xk::Hyper_L),
        (NamedKey::Hyper, Right, false, xk::Hyper_R),
        (NamedKey::AltGraph, Right, false, xk::ISO_Level3_Shift),
        (NamedKey::GroupNext, Standard, false, xk::ISO_Next_Group),
        (NamedKey::GroupPrevious, Standard, false, xk::ISO_Prev_Group),
        (NamedKey::GroupFirst, Standard, false, xk::ISO_First_Group),
        (NamedKey::GroupLast, Standard, false, xk::ISO_Last_Group),
        (NamedKey::Space, Standard, false, xk::space),
    ];
    for &(key, location, shift, want) in rows {
        let logical = named(key);
        let keyboard = if shift {
            holding(ModifiersState::SHIFT)
        } else {
            plain()
        };
        let got = event(
            at(input(&logical, KeyCode::KeyA, Pressed), location),
            keyboard,
        );
        assert_eq!(
            got.keysym, want,
            "{key:?} at {location:?} (shift {shift}): table says {:#x}, xkb {want:#x}",
            got.keysym
        );
    }
    let digits = [
        xk::KP_0,
        xk::KP_1,
        xk::KP_2,
        xk::KP_3,
        xk::KP_4,
        xk::KP_5,
        xk::KP_6,
        xk::KP_7,
        xk::KP_8,
        xk::KP_9,
    ];
    for (digit, want) in digits.into_iter().enumerate() {
        let logical = character(&digit.to_string());
        let got = event(
            at(
                input(&logical, KeyCode::Numpad0, Pressed),
                KeyLocation::Numpad,
            ),
            plain(),
        );
        assert_eq!(got.keysym, want, "keypad {digit}");
    }
    for (text, want) in [
        ("*", xk::KP_Multiply),
        ("+", xk::KP_Add),
        (",", xk::KP_Separator),
        ("-", xk::KP_Subtract),
        (".", xk::KP_Decimal),
        ("/", xk::KP_Divide),
        ("=", xk::KP_Equal),
    ] {
        let logical = character(text);
        let got = event(
            at(
                input(&logical, KeyCode::NumpadAdd, Pressed),
                KeyLocation::Numpad,
            ),
            plain(),
        );
        assert_eq!(got.keysym, want, "keypad {text:?}");
    }
    for (dead, want) in [
        ('`', xk::dead_grave),
        ('´', xk::dead_acute),
        ('^', xk::dead_circumflex),
        ('~', xk::dead_tilde),
        ('¯', xk::dead_macron),
        ('˘', xk::dead_breve),
        ('˙', xk::dead_abovedot),
        ('¨', xk::dead_diaeresis),
        ('°', xk::dead_abovering),
        ('˝', xk::dead_doubleacute),
        ('ˇ', xk::dead_caron),
        ('¸', xk::dead_cedilla),
        ('˛', xk::dead_ogonek),
    ] {
        let logical = Key::Dead(Some(dead));
        let got = event(input(&logical, KeyCode::BracketLeft, Pressed), plain());
        assert_eq!(got.keysym, want, "dead {dead:?}");
    }
}

// ---------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------

#[test]
fn nothing_is_reported_while_no_assistive_technology_is_attached() {
    let fake = FakeRegistry::answering(&[ReportOutcome::Consumed]);
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let down = named(NamedKey::ArrowDown);
    let got = gate.filter(
        &input(&down, KeyCode::ArrowDown, Pressed),
        plain(),
        NO_READER,
    );
    assert_eq!(got, KeyDisposition::Deliver);
    assert!(
        fake.seen().is_empty(),
        "reported with no AT attached: {:?}",
        fake.seen()
    );
}

/// The fix itself: with a reader attached, the registry hears of the press
/// and the release, in that order, by the time the gate lets each through.
#[test]
fn each_press_and_release_is_reported_before_it_is_delivered() {
    let fake = FakeRegistry::default();
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let down = named(NamedKey::ArrowDown);

    let pressed = gate.filter(&input(&down, KeyCode::ArrowDown, Pressed), plain(), READER);
    assert_eq!(pressed, KeyDisposition::Deliver);
    assert_eq!(
        fake.seen().len(),
        1,
        "the press was not reported before delivery"
    );

    let released = gate.filter(&input(&down, KeyCode::ArrowDown, Released), plain(), READER);
    assert_eq!(released, KeyDisposition::Deliver);
    let kinds: Vec<_> = fake.seen().iter().map(|e| (e.kind, e.keysym)).collect();
    assert_eq!(
        kinds,
        vec![
            (DeviceEventKind::Pressed, 0xff54),
            (DeviceEventKind::Released, 0xff54)
        ]
    );
}

/// Orca+T: Orca takes the T, and the text field must not get a "t".
#[test]
fn a_press_a_listener_took_is_dropped_and_so_is_its_release() {
    let fake = FakeRegistry::answering(&[ReportOutcome::Consumed]);
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let t = character("t");

    let pressed = gate.filter(&input(&t, KeyCode::KeyT, Pressed), plain(), READER);
    assert_eq!(
        pressed,
        KeyDisposition::Drop,
        "a key Orca took reached the application"
    );

    let released = gate.filter(&input(&t, KeyCode::KeyT, Released), plain(), READER);
    assert_eq!(
        released,
        KeyDisposition::Drop,
        "the release of a taken key was delivered"
    );
    assert_eq!(fake.seen().len(), 2, "the release must still be reported");
}

#[test]
fn the_release_of_a_delivered_press_is_delivered() {
    let fake = FakeRegistry::answering(&[ReportOutcome::NotConsumed]);
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let t = character("t");
    assert_eq!(
        gate.filter(&input(&t, KeyCode::KeyT, Pressed), plain(), READER),
        KeyDisposition::Deliver
    );
    assert_eq!(
        gate.filter(&input(&t, KeyCode::KeyT, Released), plain(), READER),
        KeyDisposition::Deliver
    );
}

#[test]
fn an_unanswered_press_is_delivered() {
    let fake = FakeRegistry::answering(&[ReportOutcome::Unanswered]);
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let down = named(NamedKey::ArrowDown);
    assert_eq!(
        gate.filter(&input(&down, KeyCode::ArrowDown, Pressed), plain(), READER),
        KeyDisposition::Deliver
    );
    assert_eq!(fake.seen().len(), 1);
}

/// A held key repeats as presses; each is reported and answered on its own,
/// and the release is delivered if any press of the key was.
#[test]
fn a_held_key_is_reported_repeat_by_repeat() {
    let fake = FakeRegistry::answering(&[
        ReportOutcome::Consumed,
        ReportOutcome::Consumed,
        ReportOutcome::NotConsumed,
    ]);
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let down = named(NamedKey::ArrowDown);
    let press = input(&down, KeyCode::ArrowDown, Pressed);
    let got: Vec<_> = (0..3)
        .map(|_| gate.filter(&press, plain(), READER))
        .collect();
    assert_eq!(
        got,
        vec![
            KeyDisposition::Drop,
            KeyDisposition::Drop,
            KeyDisposition::Deliver
        ]
    );
    assert_eq!(
        gate.filter(&input(&down, KeyCode::ArrowDown, Released), plain(), READER),
        KeyDisposition::Deliver,
        "a repeat reached the application, so its release must too"
    );
    let kinds: Vec<_> = fake.seen().iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![
            DeviceEventKind::Pressed,
            DeviceEventKind::Pressed,
            DeviceEventKind::Pressed,
            DeviceEventKind::Released
        ]
    );
}

/// winit replays keys already held when focus arrives (X11). Nobody pressed
/// them: they are not reported, and nothing about them is dropped.
#[test]
fn synthetic_events_are_neither_reported_nor_dropped() {
    let fake = FakeRegistry::answering(&[ReportOutcome::Consumed]);
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let shift = named(NamedKey::Shift);
    let replay = KeyInput {
        synthetic: true,
        ..input(&shift, KeyCode::ShiftLeft, Pressed)
    };
    assert_eq!(
        gate.filter(&replay, plain(), READER),
        KeyDisposition::Deliver
    );
    assert!(fake.seen().is_empty());
}

/// A reader that leaves between a press it took and the release still gets
/// the release dropped: the application never saw the key go down.
#[test]
fn a_taken_press_keeps_its_release_after_the_reader_leaves() {
    let fake = FakeRegistry::answering(&[ReportOutcome::Consumed]);
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let t = character("t");
    assert_eq!(
        gate.filter(&input(&t, KeyCode::KeyT, Pressed), plain(), READER),
        KeyDisposition::Drop
    );
    assert_eq!(
        gate.filter(&input(&t, KeyCode::KeyT, Released), plain(), NO_READER),
        KeyDisposition::Drop
    );
    assert_eq!(
        fake.seen().len(),
        2,
        "the release of a reported press is reported, reader or not"
    );
}

/// A key pressed where a reader is attached can come up in a window whose
/// adapter the reader has not reached yet (accesskit_unix activates each
/// adapter on its own thread). The registry heard the press, so it must hear
/// the release: libatspi's legacy device sets the Orca modifier from the one
/// and clears it from the other.
#[test]
fn the_release_of_a_reported_press_is_reported_wherever_it_lands() {
    let fake = FakeRegistry::default();
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let insert = named(NamedKey::Insert);
    gate.filter(&input(&insert, KeyCode::Insert, Pressed), plain(), READER);
    gate.filter(
        &input(&insert, KeyCode::Insert, Released),
        plain(),
        NO_READER,
    );
    let kinds: Vec<_> = fake.seen().iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![DeviceEventKind::Pressed, DeviceEventKind::Released]
    );
}

/// Another key's release is not mistaken for the taken one's.
#[test]
fn only_the_taken_key_loses_its_release() {
    let fake = FakeRegistry::answering(&[ReportOutcome::NotConsumed, ReportOutcome::Consumed]);
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let insert = named(NamedKey::Insert);
    let t = character("t");
    let press_insert = input(&insert, KeyCode::Insert, Pressed);
    let press_t = input(&t, KeyCode::KeyT, Pressed);
    assert_eq!(
        gate.filter(&press_insert, plain(), READER),
        KeyDisposition::Deliver
    );
    assert_eq!(gate.filter(&press_t, plain(), READER), KeyDisposition::Drop);
    assert_eq!(
        gate.filter(&input(&t, KeyCode::KeyT, Released), plain(), READER),
        KeyDisposition::Drop
    );
    assert_eq!(
        gate.filter(&input(&insert, KeyCode::Insert, Released), plain(), READER),
        KeyDisposition::Deliver
    );
}

/// The application saw the key go down on the first press, so it must see it
/// come up, even when the reader took a later repeat of it.
#[test]
fn the_release_is_delivered_when_any_press_of_the_hold_was() {
    let fake = FakeRegistry::answering(&[ReportOutcome::NotConsumed, ReportOutcome::Consumed]);
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let down = named(NamedKey::ArrowDown);
    let press = input(&down, KeyCode::ArrowDown, Pressed);
    assert_eq!(
        gate.filter(&press, plain(), READER),
        KeyDisposition::Deliver
    );
    assert_eq!(
        gate.filter(&repeated(press), plain(), READER),
        KeyDisposition::Drop
    );
    assert_eq!(
        gate.filter(&input(&down, KeyCode::ArrowDown, Released), plain(), READER),
        KeyDisposition::Deliver,
        "the application got KeyDown and would never get KeyUp"
    );
}

/// A press made before a reader attached reached the application unreported;
/// a repeat the reader then takes does not take the release with it.
#[test]
fn a_press_from_before_the_reader_keeps_its_release() {
    let fake = FakeRegistry::answering(&[ReportOutcome::Consumed]);
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let down = named(NamedKey::ArrowDown);
    let press = input(&down, KeyCode::ArrowDown, Pressed);
    assert_eq!(
        gate.filter(&press, plain(), NO_READER),
        KeyDisposition::Deliver
    );
    assert_eq!(
        gate.filter(&repeated(press), plain(), READER),
        KeyDisposition::Drop
    );
    assert_eq!(
        gate.filter(&input(&down, KeyCode::ArrowDown, Released), plain(), READER),
        KeyDisposition::Deliver
    );
}

/// A new press of a key, not a repeat, starts a new hold: a delivered press
/// whose release was lost (it came up in another application) does not make
/// the release of a later, taken press reach the application.
#[test]
fn a_new_press_starts_a_new_hold() {
    let fake = FakeRegistry::answering(&[ReportOutcome::NotConsumed, ReportOutcome::Consumed]);
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let t = character("t");
    let press = input(&t, KeyCode::KeyT, Pressed);
    assert_eq!(
        gate.filter(&press, plain(), READER),
        KeyDisposition::Deliver
    );
    assert_eq!(gate.filter(&press, plain(), READER), KeyDisposition::Drop);
    assert_eq!(
        gate.filter(&input(&t, KeyCode::KeyT, Released), plain(), READER),
        KeyDisposition::Drop
    );
}

// ---------------------------------------------------------------------------
// Num Lock
// ---------------------------------------------------------------------------

const MOD2: u32 = 1 << 4;

fn keypad<'a>(logical: &'a Key, code: KeyCode) -> KeyInput<'a> {
    at(input(logical, code, Pressed), KeyLocation::Numpad)
}

/// Every press the fake saw, as (keysym, whether Mod2 was set).
fn mod2_by_keysym(fake: &FakeRegistry) -> Vec<(u32, bool)> {
    fake.seen()
        .iter()
        .filter(|e| e.kind == DeviceEventKind::Pressed)
        .map(|e| (e.keysym, e.modifiers & MOD2 != 0))
        .collect()
}

/// libatspi keeps Num Lock significant on the keypad's own keycodes
/// (`_atspi_key_is_on_keypad`), so Orca's keypad commands, grabbed with no
/// modifier, take keypad Enter, +, -, * and / unless the event says Num Lock
/// is on. Once a key has shown it on, every key says so, as an X client's
/// state does.
#[test]
fn num_lock_known_on_rides_on_every_key() {
    let fake = FakeRegistry::default();
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let one = character("1");
    let plus = character("+");
    let enter = named(NamedKey::Enter);
    let a = character("a");
    for key in [
        keypad(&one, KeyCode::Numpad1),
        keypad(&plus, KeyCode::NumpadAdd),
        keypad(&enter, KeyCode::NumpadEnter),
        input(&a, KeyCode::KeyA, Pressed),
    ] {
        gate.filter(&key, plain(), READER);
    }
    assert_eq!(
        mod2_by_keysym(&fake),
        vec![(0xffb1, true), (0xffab, true), (0xff8d, true), (0x61, true)]
    );
}

/// A keypad key that moves the caret shows Num Lock off.
#[test]
fn a_keypad_motion_shows_num_lock_off() {
    let fake = FakeRegistry::default();
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let one = character("1");
    let end = named(NamedKey::End);
    let plus = character("+");
    for key in [
        keypad(&one, KeyCode::Numpad1),
        keypad(&end, KeyCode::Numpad1),
        keypad(&plus, KeyCode::NumpadAdd),
    ] {
        gate.filter(&key, plain(), READER);
    }
    assert_eq!(
        mod2_by_keysym(&fake),
        vec![(0xffb1, true), (0xff9c, false), (0xffab, false)]
    );
}

/// The Num Lock key toggles a known state; its own event carries the state
/// before it, as an X key event's does.
#[test]
fn the_num_lock_key_toggles_a_known_state() {
    let fake = FakeRegistry::default();
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let one = character("1");
    let num_lock = named(NamedKey::NumLock);
    let plus = character("+");
    for key in [
        keypad(&one, KeyCode::Numpad1),
        keypad(&num_lock, KeyCode::NumLock),
        keypad(&plus, KeyCode::NumpadAdd),
        keypad(&num_lock, KeyCode::NumLock),
        keypad(&plus, KeyCode::NumpadAdd),
    ] {
        gate.filter(&key, plain(), READER);
        let release = KeyInput {
            state: Released,
            ..key
        };
        gate.filter(&release, plain(), READER);
    }
    assert_eq!(
        mod2_by_keysym(&fake),
        vec![
            (0xffb1, true),
            (0xff7f, true),
            (0xffab, false),
            (0xff7f, false),
            (0xffab, true)
        ]
    );
}

/// Until a key has shown the state, nothing claims Num Lock is on: the state
/// an Orca user with a desktop layout keeps, since Orca's keypad commands
/// need it off.
#[test]
fn num_lock_unknown_sets_no_mod2() {
    let fake = FakeRegistry::default();
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let plus = character("+");
    let num_lock = named(NamedKey::NumLock);
    gate.filter(&keypad(&plus, KeyCode::NumpadAdd), plain(), READER);
    gate.filter(&keypad(&num_lock, KeyCode::NumLock), plain(), READER);
    gate.filter(&keypad(&plus, KeyCode::NumpadAdd), plain(), READER);
    assert_eq!(
        mod2_by_keysym(&fake),
        vec![(0xffab, false), (0xff7f, false), (0xffab, false)]
    );
}

// ---------------------------------------------------------------------------
// Secure fields
// ---------------------------------------------------------------------------

/// What a key typed into a secure field tells the registry: not the
/// character. GTK 3 and Qt 6 report it in full; Orca needs none of it there
/// (it echoes nothing in a password text and obscures such keys in its own
/// log), and the field promises its plaintext never reaches assistive
/// technology. The keycode and modifiers stay: Orca matches its own commands
/// on them.
#[test]
fn a_secure_field_keeps_its_characters_from_the_registry() {
    let fake = FakeRegistry::default();
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let p = character("P");
    let space = named(NamedKey::Space);
    let acute = Key::Dead(Some('´'));
    for (key, code) in [
        (&p, KeyCode::KeyP),
        (&space, KeyCode::Space),
        (&acute, KeyCode::BracketLeft),
    ] {
        gate.filter(
            &input(key, code, Pressed),
            holding(ModifiersState::SHIFT),
            SECURE,
        );
        gate.filter(
            &input(key, code, Released),
            holding(ModifiersState::SHIFT),
            SECURE,
        );
    }
    let seen = fake.seen();
    assert_eq!(seen.len(), 6);
    for event in &seen {
        assert_eq!(event.keysym, 0x00ff_ffff, "keysym withheld: {event:?}");
        assert_eq!(event.event_string, "", "text withheld: {event:?}");
        assert!(!event.is_text);
        assert_eq!(event.modifiers, 1, "Shift is still reported");
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    assert_eq!(
        seen[0].hw_code, 33,
        "the keycode is still reported (KEY_P + 8)"
    );
}

/// The keys that type nothing keep their names in a secure field: Orca
/// needs Tab, BackSpace or its own modifier for what it does there.
#[test]
fn a_secure_field_still_names_its_other_keys() {
    let fake = FakeRegistry::default();
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let tab = named(NamedKey::Tab);
    let backspace = named(NamedKey::Backspace);
    let insert = named(NamedKey::Insert);
    for (key, code) in [
        (&tab, KeyCode::Tab),
        (&backspace, KeyCode::Backspace),
        (&insert, KeyCode::Insert),
    ] {
        gate.filter(&input(key, code, Pressed), plain(), SECURE);
    }
    let keysyms: Vec<_> = fake.seen().iter().map(|e| e.keysym).collect();
    assert_eq!(keysyms, vec![0xff09, 0xff08, 0xff63]);
}

/// A character pressed in a secure field stays withheld on its release,
/// wherever focus has gone meanwhile.
#[test]
fn a_withheld_press_keeps_its_release_withheld() {
    let fake = FakeRegistry::default();
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let p = character("p");
    gate.filter(&input(&p, KeyCode::KeyP, Pressed), plain(), SECURE);
    gate.filter(&input(&p, KeyCode::KeyP, Released), plain(), READER);
    let seen = fake.seen();
    assert_eq!(seen[1].kind, DeviceEventKind::Released);
    assert_eq!(seen[1].keysym, 0x00ff_ffff);
    assert_eq!(seen[1].event_string, "");
}

/// Outside a secure field, nothing is withheld.
#[test]
fn an_ordinary_field_reports_its_characters() {
    let fake = FakeRegistry::default();
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let p = character("p");
    gate.filter(&input(&p, KeyCode::KeyP, Pressed), plain(), READER);
    assert_eq!(fake.seen()[0].keysym, 0x70);
    assert_eq!(fake.seen()[0].event_string, "p");
}

// ---------------------------------------------------------------------------
// A release names the key its press named
// ---------------------------------------------------------------------------

/// winit composes on presses only: the key that completes a dead-key
/// sequence is `é` on its press and `e` on its release. Shift can come or go
/// during a hold too. The release is of the key that went down, so it is
/// reported as its press was, which is what Orca pairs them by
/// (`KeyboardEvent.isReleaseFor`: keysym, string, key name).
#[test]
fn a_release_names_the_key_its_press_named() {
    let fake = FakeRegistry::default();
    let mut gate = KeyReportGate::with_reporter(fake.clone());
    let composed = character("é");
    let bare = character("e");
    gate.filter(&input(&composed, KeyCode::KeyE, Pressed), plain(), READER);
    gate.filter(&input(&bare, KeyCode::KeyE, Released), plain(), READER);
    let lower = character("a");
    let upper = character("A");
    gate.filter(&input(&lower, KeyCode::KeyA, Pressed), plain(), READER);
    gate.filter(
        &input(&upper, KeyCode::KeyA, Released),
        holding(ModifiersState::SHIFT),
        READER,
    );
    let pairs: Vec<_> = fake
        .seen()
        .iter()
        .map(|e| (e.kind, e.keysym, e.event_string.clone()))
        .collect();
    assert_eq!(
        pairs,
        vec![
            (DeviceEventKind::Pressed, 0xe9, "é".to_owned()),
            (DeviceEventKind::Released, 0xe9, "é".to_owned()),
            (DeviceEventKind::Pressed, 0x61, "a".to_owned()),
            (DeviceEventKind::Released, 0x61, "a".to_owned()),
        ]
    );
}
