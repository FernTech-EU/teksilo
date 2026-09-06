// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;

// ---------------------------------------------------------------------------
// The pre-existing suite. Not one assertion here has been edited: mouse and
// keyboard translation are byte-for-byte what they were.
// ---------------------------------------------------------------------------

#[test]
fn cursor_moved_to_pointer_move() {
    let mut state = TranslationState::new();
    let event = translate_cursor_moved(100.0, 50.0, &mut state).unwrap();
    if let WidgetEvent::PointerMove { position } = event {
        assert_eq!(position.x, 100.0);
        assert_eq!(position.y, 50.0);
    } else {
        panic!("Expected PointerMove");
    }
}

#[test]
fn scale_factor_divides_physical_coords() {
    let mut state = TranslationState::new();
    state.set_scale_factor(2.0);
    let event = translate_cursor_moved(200.0, 100.0, &mut state).unwrap();
    if let WidgetEvent::PointerMove { position } = event {
        assert_eq!(position.x, 100.0);
        assert_eq!(position.y, 50.0);
    } else {
        panic!("Expected PointerMove");
    }
}

#[test]
fn mouse_button_translation() {
    assert_eq!(
        translate_mouse_button(winit::event::MouseButton::Left),
        Some(PointerButton::Primary)
    );
    assert_eq!(
        translate_mouse_button(winit::event::MouseButton::Right),
        Some(PointerButton::Secondary)
    );
}

#[test]
fn mouse_press_to_pointer_down() {
    let mut state = TranslationState::new();
    translate_cursor_moved(50.0, 25.0, &mut state);
    let event = translate_mouse_input(
        winit::event::ElementState::Pressed,
        winit::event::MouseButton::Left,
        &state,
    )
    .unwrap();
    assert!(matches!(
        event,
        WidgetEvent::PointerDown {
            button: PointerButton::Primary,
            ..
        }
    ));
}

#[test]
fn key_translation() {
    let key = winit::keyboard::Key::Named(winit::keyboard::NamedKey::Space);
    assert_eq!(translate_key(&key), Some(Key::Space));
}

#[test]
fn pinch_gesture_started() {
    let mut state = TranslationState::new();
    translate_cursor_moved(100.0, 50.0, &mut state);
    let event = translate_pinch_gesture(0.0, winit::event::TouchPhase::Started, &state).unwrap();
    assert!(matches!(
        event,
        WidgetEvent::Gesture {
            gesture: GestureEvent::PinchStarted { .. }
        }
    ));
}

#[test]
fn pinch_gesture_changed() {
    let mut state = TranslationState::new();
    translate_cursor_moved(100.0, 50.0, &mut state);
    let event = translate_pinch_gesture(0.5, winit::event::TouchPhase::Moved, &state).unwrap();
    if let WidgetEvent::Gesture {
        gesture: GestureEvent::PinchChanged { scale, .. },
    } = event
    {
        assert!((scale - 1.5).abs() < 0.001);
    } else {
        panic!("Expected PinchChanged");
    }
}

#[test]
fn pinch_gesture_ended() {
    let state = TranslationState::new();
    let event = translate_pinch_gesture(0.0, winit::event::TouchPhase::Ended, &state).unwrap();
    assert!(matches!(
        event,
        WidgetEvent::Gesture {
            gesture: GestureEvent::PinchEnded
        }
    ));
}

#[test]
fn rotation_gesture_translates() {
    let mut state = TranslationState::new();
    translate_cursor_moved(50.0, 50.0, &mut state);
    let event = translate_rotation_gesture(15.0, winit::event::TouchPhase::Moved, &state).unwrap();
    if let WidgetEvent::Gesture {
        gesture: GestureEvent::PinchChanged {
            rotation, scale, ..
        },
    } = event
    {
        assert!((rotation - 15.0).abs() < 0.001);
        assert!((scale - 1.0).abs() < 0.001);
    } else {
        panic!("Expected PinchChanged with rotation");
    }
}

#[test]
fn double_tap_gesture_translates() {
    let mut state = TranslationState::new();
    translate_cursor_moved(75.0, 25.0, &mut state);
    let event = translate_double_tap_gesture(&state).unwrap();
    if let WidgetEvent::Gesture {
        gesture: GestureEvent::DoubleTap(tap_event),
    } = event
    {
        assert_eq!(tap_event.position.x, 75.0);
        assert_eq!(tap_event.position.y, 25.0);
        assert_eq!(tap_event.button, PointerButton::Primary);
    } else {
        panic!("Expected DoubleTap");
    }
}

#[test]
fn ime_preedit_to_composition_preserves_cursor_bytes() {
    let evt = translate_ime(winit::event::Ime::Preedit("a".to_string(), Some((1, 1)))).unwrap();
    assert!(matches!(
        evt,
        WidgetEvent::ImeComposition { ref text, cursor: Some(ref r) }
            if text == "a" && *r == (1..1)
    ));
}

#[test]
fn ime_preedit_hide_cursor_maps_to_none() {
    let evt = translate_ime(winit::event::Ime::Preedit(String::new(), None)).unwrap();
    assert!(matches!(
        evt,
        WidgetEvent::ImeComposition { ref text, cursor: None } if text.is_empty()
    ));
}

#[test]
fn ime_commit_translates() {
    let evt = translate_ime(winit::event::Ime::Commit("你".to_string())).unwrap();
    assert!(matches!(evt, WidgetEvent::ImeCommit { ref text } if text == "你"));
}

#[test]
fn ime_enabled_disabled_produce_no_event() {
    assert!(translate_ime(winit::event::Ime::Enabled).is_none());
    assert!(translate_ime(winit::event::Ime::Disabled).is_none());
}

// ---------------------------------------------------------------------------
// Helpers for the new suite
// ---------------------------------------------------------------------------

fn touch(phase: winit::event::TouchPhase, id: u64, x: f64, y: f64) -> winit::event::WindowEvent {
    winit::event::WindowEvent::Touch(winit::event::Touch {
        device_id: winit::event::DeviceId::dummy(),
        phase,
        location: winit::dpi::PhysicalPosition::new(x, y),
        force: None,
        id,
    })
}

fn cursor_moved(x: f64, y: f64) -> winit::event::WindowEvent {
    winit::event::WindowEvent::CursorMoved {
        device_id: winit::event::DeviceId::dummy(),
        position: winit::dpi::PhysicalPosition::new(x, y),
    }
}

fn mouse_input(
    state: winit::event::ElementState,
    button: winit::event::MouseButton,
) -> winit::event::WindowEvent {
    winit::event::WindowEvent::MouseInput {
        device_id: winit::event::DeviceId::dummy(),
        state,
        button,
    }
}

fn feed(
    state: &mut TranslationState,
    event: &winit::event::WindowEvent,
    ms: u64,
) -> Vec<InputSample> {
    state.translate(&BackendEvent::Winit(event), EventTime::from_millis(ms))
}

// ---------------------------------------------------------------------------
// The kill switch
// ---------------------------------------------------------------------------

/// The programme's rollback switch, honoured at the producer. With it off a
/// touch packet yields nothing — no sample, no id minted, no contact tracked,
/// so nothing downstream can even observe that a finger existed.
#[test]
fn touch_disabled_yields_no_samples_at_all() {
    let mut state = TranslationState::new();
    state.set_input_tokens(InputTokens {
        touch_enabled: false,
        ..InputTokens::default()
    });

    for (phase, ms) in [
        (winit::event::TouchPhase::Started, 0),
        (winit::event::TouchPhase::Moved, 10),
        (winit::event::TouchPhase::Ended, 20),
    ] {
        assert!(feed(&mut state, &touch(phase, 1, 40.0, 60.0), ms).is_empty());
    }
    assert_eq!(state.live_contact_count(), 0);
}

/// …and with it off the mouse is untouched: same events, same samples, whether
/// touch is enabled or not.
#[test]
fn touch_disabled_leaves_mouse_translation_identical() {
    let render = |touch_enabled: bool| {
        let mut state = TranslationState::new();
        state.set_input_tokens(InputTokens {
            touch_enabled,
            ..InputTokens::default()
        });

        let mut out = Vec::new();
        for (event, ms) in [
            (cursor_moved(30.0, 40.0), 0),
            (
                mouse_input(
                    winit::event::ElementState::Pressed,
                    winit::event::MouseButton::Left,
                ),
                5,
            ),
            (
                mouse_input(
                    winit::event::ElementState::Released,
                    winit::event::MouseButton::Left,
                ),
                9,
            ),
        ] {
            for sample in feed(&mut state, &event, ms) {
                let s = sample.as_pointer().expect("pointer sample").clone();
                out.push(format!(
                    "{:?} {:?} {:?} {:?}",
                    s.phase, s.position, s.button, s.pointer.buttons
                ));
            }
        }
        out
    };
    assert_eq!(render(true), render(false));
    assert_eq!(render(true).len(), 3);
}

// ---------------------------------------------------------------------------
// A finger reports Primary
// ---------------------------------------------------------------------------

/// Normative. Every `accept_buttons()` recognizer in the framework gates on
/// `ButtonMask::PRIMARY`; a contact that reported an empty mask would be
/// invisible to tap, drag, long-press and multi-tap alike.
#[test]
fn a_finger_holds_the_primary_button_while_it_is_down() {
    let mut state = TranslationState::new();

    let down = feed(
        &mut state,
        &touch(winit::event::TouchPhase::Started, 3, 10.0, 10.0),
        0,
    );
    let down = down[0].as_pointer().unwrap();
    assert_eq!(down.phase, PointerPhase::Down);
    assert_eq!(down.pointer.kind, teksilo_tokens::PointerKind::Touch);
    assert_eq!(down.pointer.buttons, ButtonMask::PRIMARY);
    assert!(down.pointer.buttons.contains(PointerButton::Primary));
    assert_eq!(down.button, Some(PointerButton::Primary));

    let moved = feed(
        &mut state,
        &touch(winit::event::TouchPhase::Moved, 3, 12.0, 10.0),
        8,
    );
    let moved = moved[0].as_pointer().unwrap();
    assert_eq!(moved.pointer.buttons, ButtonMask::PRIMARY);
    assert_eq!(moved.button, None, "a move changes no button");

    let up = feed(
        &mut state,
        &touch(winit::event::TouchPhase::Ended, 3, 12.0, 10.0),
        16,
    );
    let up = up[0].as_pointer().unwrap();
    assert_eq!(up.phase, PointerPhase::Up);
    assert_eq!(
        up.pointer.buttons,
        ButtonMask::NONE,
        "buttons are those held *after* the sample; a lifted finger holds none"
    );
    assert_eq!(up.button, Some(PointerButton::Primary));
}

/// A finger never hovers: no touch sample is ever a buttonless move.
#[test]
fn a_finger_never_hovers() {
    let mut state = TranslationState::new();
    for (phase, ms) in [
        (winit::event::TouchPhase::Started, 0),
        (winit::event::TouchPhase::Moved, 8),
        (winit::event::TouchPhase::Moved, 16),
        (winit::event::TouchPhase::Ended, 24),
    ] {
        for sample in feed(&mut state, &touch(phase, 1, 5.0, 5.0), ms) {
            let s = sample.as_pointer().unwrap();
            assert!(
                s.phase != PointerPhase::Move || !s.pointer.buttons.is_empty(),
                "a buttonless touch move would be a hover"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// The Point::ZERO regression
// ---------------------------------------------------------------------------

/// A press with no known cursor position used to dispatch at the window
/// origin — a click on whatever sat in the top-left corner. It now produces
/// nothing, on both surfaces.
#[test]
fn a_press_with_no_cursor_position_produces_nothing() {
    let state = TranslationState::new();
    assert_eq!(state.cursor_position(), None);
    assert!(
        translate_mouse_input(
            winit::event::ElementState::Pressed,
            winit::event::MouseButton::Left,
            &state,
        )
        .is_none()
    );

    let mut state = TranslationState::new();
    assert!(
        feed(
            &mut state,
            &mouse_input(
                winit::event::ElementState::Pressed,
                winit::event::MouseButton::Left
            ),
            0,
        )
        .is_empty()
    );
}

// ---------------------------------------------------------------------------
// X11 phantom motion
// ---------------------------------------------------------------------------

/// winit's X11 backend warps the virtual core pointer onto the first
/// concurrently-active contact and reports it through the same device a real
/// mouse uses, so while a contact is live the emulated motion has to go.
#[test]
fn the_x11_phantom_cursor_move_beside_a_live_contact_is_dropped() {
    let mut state = TranslationState::new();
    state.set_window_system(WindowSystem::X11);

    feed(
        &mut state,
        &touch(winit::event::TouchPhase::Started, 1, 80.0, 90.0),
        0,
    );
    assert_eq!(state.live_contact_count(), 1);

    assert!(
        feed(&mut state, &cursor_moved(80.0, 90.0), 1).is_empty(),
        "the emulated pointer following the finger must not reach the tree"
    );
    assert_eq!(
        state.cursor_position(),
        None,
        "a suppressed phantom must not move the mouse's own position either"
    );
}

/// The same event with no live contact is a real mouse and is kept.
#[test]
fn the_same_cursor_move_with_no_live_contact_is_kept() {
    let mut state = TranslationState::new();
    state.set_window_system(WindowSystem::X11);

    let samples = feed(&mut state, &cursor_moved(80.0, 90.0), 0);
    assert_eq!(samples.len(), 1);
    let sample = samples[0].as_pointer().unwrap();
    assert_eq!(sample.phase, PointerPhase::Move);
    assert_eq!(sample.position, Point::new(80.0, 90.0));
    assert_eq!(state.cursor_position(), Some(Point::new(80.0, 90.0)));
}

/// After the lift the core pointer stays parked at the lift point for a
/// moment. A move still *at* that point is the ghost; a move anywhere else is
/// a real mouse and gets through at once.
#[test]
fn the_parked_core_pointer_is_suppressed_only_where_the_finger_lifted() {
    let mut state = TranslationState::new();
    state.set_window_system(WindowSystem::X11);

    feed(
        &mut state,
        &touch(winit::event::TouchPhase::Started, 1, 40.0, 40.0),
        0,
    );
    feed(
        &mut state,
        &touch(winit::event::TouchPhase::Ended, 1, 40.0, 40.0),
        10,
    );

    assert!(feed(&mut state, &cursor_moved(40.0, 40.0), 20).is_empty());
    assert_eq!(
        feed(&mut state, &cursor_moved(400.0, 400.0), 30).len(),
        1,
        "a real mouse elsewhere is not the ghost"
    );
    // …and the window closes.
    assert_eq!(feed(&mut state, &cursor_moved(40.0, 40.0), 400).len(), 1);
}

/// The suppressors are scoped: Wayland and the {Windows, macOS, headless} set
/// that `WindowSystem::Unknown` names do not promote touch to mouse, so
/// nothing there is ever dropped.
#[test]
fn only_x11_suppresses_the_mouse_stream() {
    for ws in [WindowSystem::Wayland, WindowSystem::Unknown] {
        let mut state = TranslationState::new();
        state.set_window_system(ws);
        feed(
            &mut state,
            &touch(winit::event::TouchPhase::Started, 1, 40.0, 40.0),
            0,
        );
        assert_eq!(
            feed(&mut state, &cursor_moved(40.0, 40.0), 1).len(),
            1,
            "{ws:?} does not promote touch to mouse"
        );
    }
}

/// A promoted click is suppressed for 500 ms after the lift on a promoting
/// platform, and never anywhere else. Defence in depth: winit 0.30 already
/// filters X11's `XIPointerEmulated` buttons.
#[test]
fn a_promoted_click_is_suppressed_for_half_a_second() {
    let mut state = TranslationState::new();
    state.set_window_system(WindowSystem::X11);
    feed(&mut state, &cursor_moved(10.0, 10.0), 0);

    feed(
        &mut state,
        &touch(winit::event::TouchPhase::Started, 1, 40.0, 40.0),
        10,
    );
    feed(
        &mut state,
        &touch(winit::event::TouchPhase::Ended, 1, 40.0, 40.0),
        20,
    );

    let press = mouse_input(
        winit::event::ElementState::Pressed,
        winit::event::MouseButton::Left,
    );
    assert!(feed(&mut state, &press, 300).is_empty());
    assert_eq!(feed(&mut state, &press, 600).len(), 1);
}

// ---------------------------------------------------------------------------
// Pressure
// ---------------------------------------------------------------------------

#[test]
fn a_normalized_force_becomes_the_pressure_axis() {
    assert_eq!(
        pressure_from_force(winit::event::Force::Normalized(0.25)),
        Some(0.25)
    );
    assert_eq!(
        pressure_from_force(winit::event::Force::Normalized(4.0)),
        Some(1.0),
        "an out-of-range normalized force clamps rather than escaping"
    );
}

/// winit's own `Force::normalized()` divides by `sin(altitude_angle)` and by
/// `max_possible_force`, either of which can be zero — a stylus lying flat on
/// the glass, a device that reports no ceiling. Those must read as "no
/// pressure", not as an infinity in a `PointerAxes`.
#[test]
fn a_degenerate_calibrated_force_reports_no_pressure() {
    assert_eq!(
        pressure_from_force(winit::event::Force::Calibrated {
            force: 1.0,
            max_possible_force: 0.0,
            altitude_angle: None,
        }),
        None
    );
    assert_eq!(
        pressure_from_force(winit::event::Force::Calibrated {
            force: 1.0,
            max_possible_force: 2.0,
            altitude_angle: Some(0.0),
        }),
        None
    );
    let perpendicular = pressure_from_force(winit::event::Force::Calibrated {
        force: 1.0,
        max_possible_force: 2.0,
        altitude_angle: Some(std::f64::consts::FRAC_PI_2),
    });
    assert_eq!(perpendicular, Some(0.5));
}

#[test]
fn a_touch_carries_its_force_as_pressure() {
    let mut state = TranslationState::new();
    let event = winit::event::WindowEvent::Touch(winit::event::Touch {
        device_id: winit::event::DeviceId::dummy(),
        phase: winit::event::TouchPhase::Started,
        location: winit::dpi::PhysicalPosition::new(1.0, 1.0),
        force: Some(winit::event::Force::Normalized(0.75)),
        id: 9,
    });
    let samples = feed(&mut state, &event, 0);
    assert_eq!(
        samples[0].as_pointer().unwrap().pointer.axes.pressure,
        Some(0.75)
    );
}

// ---------------------------------------------------------------------------
// lines_per_notch
// ---------------------------------------------------------------------------

/// The hardcoded `LINES_PER_NOTCH: f32 = 3.0` is gone; the value comes from
/// the installed tokens, whose default is that same 3.0.
#[test]
fn lines_per_notch_comes_from_the_input_tokens() {
    let mut state = TranslationState::new();
    let wheel = winit::event::MouseScrollDelta::LineDelta(0.0, 1.0);

    let default = translate_mouse_wheel(wheel, winit::event::TouchPhase::Moved, &state).unwrap();
    assert!(matches!(
        default,
        WidgetEvent::Scroll { delta: ScrollDelta::Lines { y, .. }, .. } if (y + 3.0).abs() < 1e-6
    ));

    state.set_input_tokens(InputTokens {
        lines_per_notch: 5.0,
        ..InputTokens::default()
    });
    let retuned = translate_mouse_wheel(wheel, winit::event::TouchPhase::Moved, &state).unwrap();
    assert!(matches!(
        retuned,
        WidgetEvent::Scroll { delta: ScrollDelta::Lines { y, .. }, .. } if (y + 5.0).abs() < 1e-6
    ));
}

// ---------------------------------------------------------------------------
// The scroll-phase machine
// ---------------------------------------------------------------------------

fn wheel(
    delta: winit::event::MouseScrollDelta,
    phase: winit::event::TouchPhase,
) -> winit::event::WindowEvent {
    winit::event::WindowEvent::MouseWheel {
        device_id: winit::event::DeviceId::dummy(),
        delta,
        phase,
    }
}

/// A plain wheel notch — `Moved` with no `Started` before it, which is what
/// Windows and X11 always send — stays `Discrete`, exactly as before the
/// programme.
#[test]
fn a_bare_wheel_notch_is_discrete() {
    let mut state = TranslationState::new();
    let samples = feed(
        &mut state,
        &wheel(
            winit::event::MouseScrollDelta::LineDelta(0.0, 1.0),
            winit::event::TouchPhase::Moved,
        ),
        0,
    );
    let scroll = samples[0].as_scroll().unwrap();
    assert_eq!(scroll.phase, ScrollPhase::Discrete);
    assert_eq!(scroll.source, ScrollSource::Wheel);
    assert_eq!(scroll.position, None, "a wheel routes by hover");
}

/// A precise pixel delta is a trackpad, and it still routes by hover: an
/// indirect pointer moves the cursor, so the hovered widget *is* the one under
/// the gesture.
#[test]
fn a_pixel_delta_is_a_trackpad() {
    let mut state = TranslationState::new();
    let samples = feed(
        &mut state,
        &wheel(
            winit::event::MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(
                0.0, 12.0,
            )),
            winit::event::TouchPhase::Moved,
        ),
        0,
    );
    let scroll = samples[0].as_scroll().unwrap();
    assert_eq!(scroll.source, ScrollSource::Trackpad);
    assert_eq!(scroll.position, None);
}

/// The macOS ordering. winit collapses `NSEvent`'s `phase` and `momentumPhase`
/// into one `TouchPhase`, so the OS's momentum arrives as a *second*
/// `Started → Moved → Ended` run. Reading that as a new gesture is what would
/// let a Teksilo fling stack on top of the OS's.
#[test]
fn os_momentum_is_momentum_and_not_a_second_gesture() {
    let mut state = TranslationState::new();
    let pixels =
        winit::event::MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(0.0, 8.0));

    let mut phases = Vec::new();
    for (winit_phase, ms) in [
        (winit::event::TouchPhase::Started, 0),
        (winit::event::TouchPhase::Moved, 16),
        (winit::event::TouchPhase::Moved, 32),
        (winit::event::TouchPhase::Ended, 48),
        // AppKit hands its momentum over in the same run-loop turn as the
        // lift; winit reports it as another `Started`.
        (winit::event::TouchPhase::Started, 49),
        (winit::event::TouchPhase::Moved, 64),
        (winit::event::TouchPhase::Ended, 300),
    ] {
        for sample in feed(&mut state, &wheel(pixels, winit_phase), ms) {
            phases.push(sample.as_scroll().unwrap().phase);
        }
    }

    assert_eq!(
        phases,
        vec![
            ScrollPhase::Began,
            ScrollPhase::Changed,
            ScrollPhase::Changed,
            ScrollPhase::Ended,
            ScrollPhase::Momentum,
            ScrollPhase::Momentum,
            ScrollPhase::MomentumEnded,
        ]
    );
}

/// A `Started` well after the previous `Ended` is a genuinely new gesture.
#[test]
fn a_late_started_is_a_new_gesture_not_momentum() {
    let mut state = TranslationState::new();
    let pixels =
        winit::event::MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(0.0, 8.0));
    feed(
        &mut state,
        &wheel(pixels, winit::event::TouchPhase::Started),
        0,
    );
    feed(
        &mut state,
        &wheel(pixels, winit::event::TouchPhase::Ended),
        10,
    );

    let samples = feed(
        &mut state,
        &wheel(pixels, winit::event::TouchPhase::Started),
        900,
    );
    assert_eq!(samples[0].as_scroll().unwrap().phase, ScrollPhase::Began);
}

#[test]
fn a_cancelled_scroll_reports_cancelled_and_resets() {
    let mut state = TranslationState::new();
    let pixels =
        winit::event::MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(0.0, 8.0));
    feed(
        &mut state,
        &wheel(pixels, winit::event::TouchPhase::Started),
        0,
    );
    let cancelled = feed(
        &mut state,
        &wheel(pixels, winit::event::TouchPhase::Cancelled),
        10,
    );
    assert_eq!(
        cancelled[0].as_scroll().unwrap().phase,
        ScrollPhase::Cancelled
    );
    let after = feed(
        &mut state,
        &wheel(pixels, winit::event::TouchPhase::Moved),
        20,
    );
    assert_eq!(after[0].as_scroll().unwrap().phase, ScrollPhase::Discrete);
}

// ---------------------------------------------------------------------------
// The trackpad gesture translators are reachable
// ---------------------------------------------------------------------------

/// They were dead code: nothing in `teksilo-app` matched the winit events that
/// feed them. They now come out of `translate` as `InputSample::Gesture`.
#[test]
fn the_trackpad_gestures_reach_the_backend_surface() {
    let mut state = TranslationState::new();
    feed(&mut state, &cursor_moved(20.0, 30.0), 0);

    let pinch = feed(
        &mut state,
        &winit::event::WindowEvent::PinchGesture {
            device_id: winit::event::DeviceId::dummy(),
            delta: 0.5,
            phase: winit::event::TouchPhase::Moved,
        },
        1,
    );
    assert!(matches!(
        pinch[0].as_gesture(),
        Some(GestureEvent::PinchChanged { scale, .. }) if (scale - 1.5).abs() < 1e-6
    ));

    let rotate = feed(
        &mut state,
        &winit::event::WindowEvent::RotationGesture {
            device_id: winit::event::DeviceId::dummy(),
            delta: 15.0,
            phase: winit::event::TouchPhase::Moved,
        },
        2,
    );
    assert!(matches!(
        rotate[0].as_gesture(),
        Some(GestureEvent::PinchChanged { rotation, .. }) if (rotation - 15.0).abs() < 1e-6
    ));

    let double_tap = feed(
        &mut state,
        &winit::event::WindowEvent::DoubleTapGesture {
            device_id: winit::event::DeviceId::dummy(),
        },
        3,
    );
    assert!(matches!(
        double_tap[0].as_gesture(),
        Some(GestureEvent::DoubleTap(tap)) if tap.position == Point::new(20.0, 30.0)
    ));
}

// ---------------------------------------------------------------------------
// Multi-touch bookkeeping
// ---------------------------------------------------------------------------

/// The first contact of a sequence is primary; a second one is not; and once
/// the primary lifts, no other contact is promoted (W3C `isPrimary`).
#[test]
fn only_the_first_contact_of_a_sequence_is_primary() {
    let mut state = TranslationState::new();
    let first = feed(
        &mut state,
        &touch(winit::event::TouchPhase::Started, 1, 10.0, 10.0),
        0,
    );
    assert!(first[0].as_pointer().unwrap().pointer.primary);

    let second = feed(
        &mut state,
        &touch(winit::event::TouchPhase::Started, 2, 60.0, 10.0),
        5,
    );
    assert!(!second[0].as_pointer().unwrap().pointer.primary);
    assert_eq!(state.live_contact_count(), 2);

    feed(
        &mut state,
        &touch(winit::event::TouchPhase::Ended, 1, 10.0, 10.0),
        10,
    );
    let still_second = feed(
        &mut state,
        &touch(winit::event::TouchPhase::Moved, 2, 62.0, 10.0),
        15,
    );
    assert!(
        !still_second[0].as_pointer().unwrap().pointer.primary,
        "a lifted primary does not hand the role on mid-sequence"
    );
}

/// A packet for a contact this window never saw go down is dropped rather than
/// invented — emitting an `Up` for a `Down` that never happened would break
/// cancel completeness just as surely as dropping one.
#[test]
fn a_packet_for_an_unknown_contact_is_dropped() {
    let mut state = TranslationState::new();
    assert!(
        feed(
            &mut state,
            &touch(winit::event::TouchPhase::Moved, 7, 1.0, 1.0),
            0
        )
        .is_empty()
    );
    assert!(
        feed(
            &mut state,
            &touch(winit::event::TouchPhase::Ended, 7, 1.0, 1.0),
            1
        )
        .is_empty()
    );
}

/// `cancel_all` terminates every live contact exactly once, oldest first.
#[test]
fn cancel_all_terminates_every_live_contact() {
    let mut state = TranslationState::new();
    for (id, ms) in [(1u64, 0u64), (2, 4), (3, 8)] {
        feed(
            &mut state,
            &touch(winit::event::TouchPhase::Started, id, 10.0, 10.0),
            ms,
        );
    }
    let cancels = state.cancel_all(EventTime::from_millis(20));
    assert_eq!(cancels.len(), 3);

    let mut ids = Vec::new();
    for sample in &cancels {
        let s = sample.as_pointer().unwrap();
        assert_eq!(s.phase, PointerPhase::Cancel);
        assert_eq!(s.pointer.buttons, ButtonMask::NONE);
        ids.push(s.pointer.id);
    }
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted, "oldest contact first");
    assert_eq!(state.live_contact_count(), 0);
    assert!(state.cancel_all(EventTime::from_millis(30)).is_empty());
}

/// A held mouse button is a live pointer too.
#[test]
fn cancel_all_terminates_a_held_mouse_button() {
    let mut state = TranslationState::new();
    feed(&mut state, &cursor_moved(5.0, 5.0), 0);
    feed(
        &mut state,
        &mouse_input(
            winit::event::ElementState::Pressed,
            winit::event::MouseButton::Left,
        ),
        1,
    );
    let cancels = state.cancel_all(EventTime::from_millis(2));
    assert_eq!(cancels.len(), 1);
    let sample = cancels[0].as_pointer().unwrap();
    assert_eq!(sample.phase, PointerPhase::Cancel);
    assert_eq!(sample.pointer.id, teksilo_core::PointerId::MOUSE);
}

/// The mouse's button mask accumulates and drains.
#[test]
fn the_mouse_button_mask_tracks_what_is_held() {
    let mut state = TranslationState::new();
    feed(&mut state, &cursor_moved(5.0, 5.0), 0);

    let left = feed(
        &mut state,
        &mouse_input(
            winit::event::ElementState::Pressed,
            winit::event::MouseButton::Left,
        ),
        1,
    );
    assert_eq!(
        left[0].as_pointer().unwrap().pointer.buttons,
        ButtonMask::PRIMARY
    );

    let right = feed(
        &mut state,
        &mouse_input(
            winit::event::ElementState::Pressed,
            winit::event::MouseButton::Right,
        ),
        2,
    );
    assert_eq!(
        right[0].as_pointer().unwrap().pointer.buttons,
        ButtonMask::PRIMARY.union(ButtonMask::SECONDARY)
    );

    let release = feed(
        &mut state,
        &mouse_input(
            winit::event::ElementState::Released,
            winit::event::MouseButton::Left,
        ),
        3,
    );
    assert_eq!(
        release[0].as_pointer().unwrap().pointer.buttons,
        ButtonMask::SECONDARY
    );
}

/// `set_now` never runs backwards: a batch whose timestamps arrive out of
/// order must not reopen a suppression window that already closed.
#[test]
fn the_translator_clock_never_runs_backwards() {
    let mut state = TranslationState::new();
    state.set_now(EventTime::from_millis(100));
    state.set_now(EventTime::from_millis(50));
    assert_eq!(state.now(), EventTime::from_millis(100));
}
