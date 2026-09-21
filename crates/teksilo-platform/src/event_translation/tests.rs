// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use super::*;

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// The pre-existing suite. Mouse and keyboard translation are byte-for-byte what
// they were; the one assertion edited since is `rotation_gesture_translates`'s,
// whose own doc comment states the contract change that forced it.
// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------

#[test]
fn cursor_moved_to_pointer_move() {
    let mut state = TranslationState::new();
    let event = translate_cursor_moved(100.0, 50.0, &mut state).unwrap();
    if let WidgetEvent::PointerMove { position, .. } = event {
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
    if let WidgetEvent::PointerMove { position, .. } = event {
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

/// A rotation gesture leaves the seam in radians.
///
/// *Contract change.* This test used to assert the payload was `15.0` for a
/// 15-degree twist — winit's number passed through unconverted into a field
/// [`GestureEvent::PinchChanged`] defines as radians. Every consumer read it as
/// radians (the scene feeds it straight to `Transform2D::rotate`), so a
/// one-degree trackpad twist turned the content by a radian: ~57× too far. The
/// unit is known here and nowhere downstream, so the conversion is here and the
/// assertion is now the converted value.
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
        assert!(
            (rotation - 15.0f32.to_radians()).abs() < 0.001,
            "15 degrees of twist is 0.2618 rad, got {rotation}"
        );
        assert!(
            (rotation - 15.0).abs() > 0.001,
            "…and emphatically not winit's degrees passed through, got {rotation}"
        );
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
    // Radians, not winit's degrees — see `rotation_gesture_translates` for the
    // assertion this replaced and why.
    assert!(matches!(
        rotate[0].as_gesture(),
        Some(GestureEvent::PinchChanged { rotation, .. })
            if (rotation - 15.0f32.to_radians()).abs() < 1e-6
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

// ---------------------------------------------------------------------------
// Pen (P16). The digitizer shims are exercised in `crate::pen`; these drive the
// translator's proximity state machine over synthetic packets, so they need no
// tablet, no compositor and no Windows.
// ---------------------------------------------------------------------------

use crate::pen::{PenButtons, PenCaps, PenPacket, PenSource};
use teksilo_canvas::Size;
use teksilo_tokens::{PenKind, PointerKind};

/// A pen source that hands over a scripted list of packets.
#[derive(Debug, Default)]
struct ScriptedPen {
    packets: Vec<PenPacket>,
    contact: Option<Size>,
    caps: PenCaps,
}

impl ScriptedPen {
    fn with(packets: Vec<PenPacket>) -> Self {
        Self {
            packets,
            contact: None,
            caps: PenCaps::FULL_PEN,
        }
    }
}

impl PenSource for ScriptedPen {
    fn poll(&mut self, out: &mut Vec<PenPacket>) {
        out.append(&mut self.packets);
    }

    fn capabilities(&self) -> PenCaps {
        self.caps
    }

    fn touch_contact(&self, _os_contact_id: u64) -> Option<Size> {
        self.contact
    }
}

/// A pen hovering at `(x, y)`.
fn hover(x: f32, y: f32) -> PenPacket {
    PenPacket::hovering(PenKind::Pen, Point::new(x, y))
}

/// Every pointer sample in `samples`.
fn pointers(samples: &[InputSample]) -> Vec<&PointerSample> {
    samples.iter().filter_map(InputSample::as_pointer).collect()
}

fn state_with_pen(packets: Vec<PenPacket>) -> TranslationState {
    let mut state = TranslationState::new();
    state.set_pen_source(Box::new(ScriptedPen::with(packets)));
    state
}

#[test]
fn a_hovering_pen_moves_with_no_buttons_down() {
    let mut state = state_with_pen(vec![hover(10.0, 20.0), hover(11.0, 21.0)]);
    let samples = state.poll_pen(EventTime::from_millis(5));
    let pointers = pointers(&samples);

    assert_eq!(pointers.len(), 2, "an enter and a move");
    for sample in &pointers {
        assert_eq!(sample.phase, PointerPhase::Move);
        assert!(
            sample.pointer.buttons.is_empty(),
            "a hovering pen holds nothing"
        );
        assert!(matches!(
            sample.pointer.kind,
            PointerKind::Pen(PenKind::Pen)
        ));
        assert!(
            sample.pointer.kind.hovers(),
            "a pen is the one direct pointer that hovers"
        );
        assert_eq!(sample.button, None);
    }
    assert_eq!(pointers[0].position, Point::new(10.0, 20.0));
    assert_eq!(pointers[1].position, Point::new(11.0, 21.0));
    assert!(state.pen_in_proximity());
}

#[test]
fn leaving_proximity_ends_the_pointer() {
    let mut state = state_with_pen(vec![
        hover(10.0, 20.0),
        PenPacket::out_of_proximity(PenKind::Pen, Point::new(10.0, 20.0)),
    ]);
    let samples = state.poll_pen(EventTime::from_millis(5));
    let pointers = pointers(&samples);

    assert_eq!(pointers.len(), 2);
    assert_eq!(pointers[0].phase, PointerPhase::Move);
    assert_eq!(
        pointers[1].phase,
        PointerPhase::Cancel,
        "a tool going out of range completes nothing"
    );
    assert!(pointers[1].pointer.buttons.is_empty());
    assert!(!state.pen_in_proximity());

    // Both samples belong to one pointer.
    assert_eq!(pointers[0].pointer.id, pointers[1].pointer.id);
}

#[test]
fn a_packet_out_of_range_for_a_session_that_never_started_says_nothing() {
    let mut state = state_with_pen(vec![PenPacket::out_of_proximity(
        PenKind::Pen,
        Point::new(1.0, 1.0),
    )]);
    assert!(state.poll_pen(EventTime::from_millis(1)).is_empty());
}

#[test]
fn the_tip_is_the_primary_button() {
    let mut state = state_with_pen(vec![
        hover(10.0, 10.0),
        hover(10.0, 10.0).down_at(0.75),
        hover(20.0, 20.0).down_at(1.0),
        hover(20.0, 20.0),
    ]);
    let samples = state.poll_pen(EventTime::from_millis(7));
    let pointers = pointers(&samples);
    let phases: Vec<_> = pointers.iter().map(|s| s.phase).collect();
    assert_eq!(
        phases,
        vec![
            PointerPhase::Move, // proximity enter
            PointerPhase::Down, // tip contact
            PointerPhase::Move, // drawing
            PointerPhase::Up,   // tip lift
        ]
    );

    let down = pointers[1];
    assert_eq!(down.button, Some(PointerButton::Primary));
    assert!(
        down.pointer.buttons.contains(PointerButton::Primary),
        "every accept_buttons() recognizer gates on PRIMARY"
    );
    assert_eq!(down.pointer.axes.pressure, Some(0.75));

    // The move while drawing keeps the tip held and carries the new pressure.
    assert!(pointers[2].pointer.buttons.contains(PointerButton::Primary));
    assert_eq!(pointers[2].pointer.axes.pressure, Some(1.0));

    // The lift releases it.
    assert_eq!(pointers[3].button, Some(PointerButton::Primary));
    assert!(pointers[3].pointer.buttons.is_empty());
}

#[test]
fn the_barrel_button_is_secondary() {
    let mut barrel = hover(5.0, 5.0);
    barrel.buttons = PenButtons::BARREL;
    let mut both = barrel;
    both.buttons = PenButtons::BARREL.union(PenButtons::SECONDARY_BARREL);

    let mut state = state_with_pen(vec![hover(5.0, 5.0), barrel, both, hover(5.0, 5.0)]);
    let samples = state.poll_pen(EventTime::from_millis(9));
    let pointers = pointers(&samples);

    // enter, barrel down, second barrel down, then both released.
    assert_eq!(pointers[1].phase, PointerPhase::Down);
    assert_eq!(pointers[1].button, Some(PointerButton::Secondary));
    assert_eq!(pointers[2].button, Some(PointerButton::Middle));
    assert!(
        pointers[2]
            .pointer
            .buttons
            .contains(PointerButton::Secondary)
    );

    let releases: Vec<_> = pointers
        .iter()
        .filter(|s| s.phase == PointerPhase::Up)
        .filter_map(|s| s.button)
        .collect();
    assert_eq!(
        releases,
        vec![PointerButton::Secondary, PointerButton::Middle]
    );
    assert!(pointers.last().unwrap().pointer.buttons.is_empty());
}

#[test]
fn the_eraser_end_is_a_new_pointer_not_a_button() {
    let mut state = state_with_pen(vec![
        hover(1.0, 1.0),
        PenPacket::hovering(PenKind::Eraser, Point::new(1.0, 1.0)),
    ]);
    let samples = state.poll_pen(EventTime::from_millis(3));
    let pointers = pointers(&samples);

    assert_eq!(
        pointers.len(),
        3,
        "enter, the old tool's end, the new enter"
    );
    assert!(matches!(
        pointers[0].pointer.kind,
        PointerKind::Pen(PenKind::Pen)
    ));
    assert_eq!(pointers[1].phase, PointerPhase::Cancel);
    assert!(matches!(
        pointers[2].pointer.kind,
        PointerKind::Pen(PenKind::Eraser)
    ));
    assert_ne!(
        pointers[0].pointer.id, pointers[2].pointer.id,
        "flipping the stylus is a different pointer, not a mutated one"
    );
    assert!(
        pointers.iter().all(|s| s.button.is_none()),
        "the eraser is never announced as a button"
    );
}

#[test]
fn a_press_is_preceded_by_the_move_that_positions_it() {
    // Position and tip change in the same packet: the consumer must see the
    // new position before the press lands on it.
    let mut state = state_with_pen(vec![hover(0.0, 0.0), hover(30.0, 40.0).down_at(0.5)]);
    let samples = state.poll_pen(EventTime::from_millis(2));
    let pointers = pointers(&samples);
    assert_eq!(pointers.len(), 3);
    assert_eq!(pointers[1].phase, PointerPhase::Move);
    assert_eq!(pointers[1].position, Point::new(30.0, 40.0));
    assert_eq!(pointers[2].phase, PointerPhase::Down);
    assert_eq!(pointers[2].position, Point::new(30.0, 40.0));
}

#[test]
fn tilt_and_twist_ride_every_sample() {
    let mut packet = hover(1.0, 2.0);
    packet.tilt = Some((-30.0, 45.0));
    packet.twist = Some(120.0);
    let mut state = state_with_pen(vec![packet]);
    let samples = state.poll_pen(EventTime::from_millis(1));
    let sample = pointers(&samples)[0];
    assert_eq!(sample.pointer.axes.tilt, Some((-30.0, 45.0)));
    assert_eq!(sample.pointer.axes.twist, Some(120.0));
}

#[test]
fn a_pen_stamps_the_polls_clock_when_the_device_has_none() {
    let mut state = state_with_pen(vec![hover(1.0, 1.0)]);
    let now = EventTime::from_millis(1234);
    let samples = state.poll_pen(now);
    assert_eq!(pointers(&samples)[0].pointer.time, now);
    assert_eq!(state.now(), now);
}

// ---------------------------------------------------------------------------
// A drained batch is not one instant
// ---------------------------------------------------------------------------

/// **The defect this section exists for.** A poll drains everything the shim
/// buffered since the last one, and those packets are separate digitizer
/// frames at separate times. Stamping them all with the poll's single `now` —
/// which is what this did — collapses a whole stroke onto one instant and
/// destroys every velocity, smoothing and time-offset computation downstream.
///
/// Delete the back-dating in `poll_pen` and this reddens on the first
/// assertion.
#[test]
fn a_drained_batch_carries_the_devices_own_times_not_the_polls() {
    // Six packets 4 ms apart on the device's clock: a 250 Hz stylus drawing
    // through one 20 ms event-loop turn.
    let packets: Vec<PenPacket> = (0..6)
        .map(|i| hover(10.0 + i as f32, 20.0).at_device_ms(7_000 + i * 4))
        .collect();
    let mut state = state_with_pen(packets);

    let now = EventTime::from_millis(50_000);
    let samples = state.poll_pen(now);
    let times: Vec<EventTime> = pointers(&samples)
        .iter()
        .map(|sample| sample.pointer.time)
        .collect();

    assert_eq!(times.len(), 6, "one sample per packet");
    for pair in times.windows(2) {
        assert!(
            pair[1] > pair[0],
            "every sample must be strictly later than the one before it: {times:?}"
        );
    }
    assert_eq!(
        times
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        times.len(),
        "and therefore all distinct: {times:?}"
    );

    // The spacing is the digitizer's, exactly.
    for pair in times.windows(2) {
        assert_eq!(pair[1].saturating_since(pair[0]), Duration::from_millis(4));
    }
    // The newest packet is the one the poll's clock actually describes.
    assert_eq!(*times.last().unwrap(), now);
    assert_eq!(
        now.saturating_since(times[0]),
        Duration::from_millis(20),
        "the batch spans the 20 ms the device says it does"
    );
}

/// The same run through a source that reports no device clock. The batch is
/// still spread — the packets are still separate frames — but over one poll
/// interval rather than over a span nobody measured.
#[test]
fn a_drained_batch_with_no_device_clock_is_still_not_one_instant() {
    let mut state = state_with_pen(vec![hover(1.0, 1.0), hover(2.0, 2.0), hover(3.0, 3.0)]);
    let now = EventTime::from_millis(9_000);
    let samples = state.poll_pen(now);
    let times: Vec<EventTime> = pointers(&samples)
        .iter()
        .map(|sample| sample.pointer.time)
        .collect();

    assert_eq!(times.len(), 3);
    for pair in times.windows(2) {
        assert!(pair[1] > pair[0], "{times:?} must strictly increase");
    }
    assert_eq!(*times.last().unwrap(), now);
    assert!(
        now.saturating_since(times[0]) < crate::pen::PEN_POLL_INTERVAL,
        "a batch nobody timed still fits in the window it was buffered in"
    );
}

/// Every transition inside one packet shares that packet's time — the `Move`
/// that positions a press and the `Down` itself are one digitizer frame, and
/// splitting them across two instants would be inventing a gesture.
#[test]
fn the_transitions_of_one_packet_share_its_time() {
    let mut state = state_with_pen(vec![
        hover(0.0, 0.0).at_device_ms(100),
        hover(30.0, 40.0).down_at(0.5).at_device_ms(108),
    ]);
    let now = EventTime::from_millis(2_000);
    let samples = state.poll_pen(now);
    let pointers = pointers(&samples);

    // enter (packet 1), then move + down (packet 2).
    assert_eq!(pointers.len(), 3);
    assert_eq!(pointers[0].pointer.time, EventTime::from_millis(1_992));
    assert_eq!(pointers[1].pointer.time, now);
    assert_eq!(pointers[2].pointer.time, now);
}

/// Invariant 3 of the conformance suite — time is monotone — is a promise to
/// every consumer downstream. A shim filling its buffer from its own thread
/// can hand over a packet the device stamped before the last drain returned,
/// and this window has already said time had reached that point.
#[test]
fn back_dating_never_reaches_behind_a_time_already_reported() {
    let mut state = state_with_pen(vec![hover(1.0, 1.0).at_device_ms(1_000)]);
    let first = EventTime::from_millis(10_000);
    assert_eq!(pointers(&state.poll_pen(first))[0].pointer.time, first);

    // The next drain is 2 ms later by the tree's clock but claims 50 ms of
    // device time: back-dating it honestly would put its oldest sample 48 ms
    // before a time this window has already reported.
    state.set_pen_source(Box::new(ScriptedPen::with(vec![
        hover(2.0, 2.0).at_device_ms(1_002),
        hover(3.0, 3.0).at_device_ms(1_052),
    ])));
    let second = EventTime::from_millis(10_002);
    let times: Vec<EventTime> = pointers(&state.poll_pen(second))
        .iter()
        .map(|sample| sample.pointer.time)
        .collect();

    assert!(
        times.iter().all(|&t| t >= first),
        "no sample may precede {first:?}, which was already reported: {times:?}"
    );
    assert_eq!(*times.last().unwrap(), second);
}

/// `translate_pen_packet` is the public door for a shim Teksilo has not met,
/// and the tree's clock is the caller's to supply — so a packet fed through it
/// lands exactly where the caller says, device stamp or no device stamp.
#[test]
fn the_public_packet_door_stamps_what_the_caller_says() {
    let mut state = TranslationState::new();
    let at = EventTime::from_millis(4_242);
    let samples = state.translate_pen_packet(&hover(5.0, 6.0).at_device_ms(77), at);
    assert_eq!(pointers(&samples)[0].pointer.time, at);
}

#[test]
fn a_pen_shim_raises_the_windows_capability_row() {
    let mut bare = TranslationState::new();
    bare.set_window_system(WindowSystem::Wayland);
    let before = bare.capabilities();
    assert!(!before.reports_tilt && !before.reports_pen_kind);

    let mut with_pen = TranslationState::new();
    with_pen.set_window_system(WindowSystem::Wayland);
    with_pen.set_pen_source(Box::new(ScriptedPen::with(Vec::new())));
    let after = with_pen.capabilities();
    assert!(after.reports_tilt && after.reports_twist && after.reports_pen_kind);
    assert!(after.reports_pressure);
    // Everything that is not a pen capability is untouched.
    assert_eq!(after.reports_cancel, before.reports_cancel);
    assert_eq!(after.osk, before.osk);
}

#[test]
fn cancel_all_ends_a_hovering_pen() {
    let mut state = state_with_pen(vec![hover(3.0, 4.0).down_at(1.0)]);
    let _ = state.poll_pen(EventTime::from_millis(1));
    assert!(state.pen_in_proximity());

    let samples = state.cancel_all(EventTime::from_millis(2));
    let pointers = pointers(&samples);
    assert_eq!(pointers.len(), 1);
    assert_eq!(pointers[0].phase, PointerPhase::Cancel);
    assert_eq!(pointers[0].position, Point::new(3.0, 4.0));
    assert!(!state.pen_in_proximity());

    // And a second cancel has nothing left to end.
    assert!(state.cancel_all(EventTime::from_millis(3)).is_empty());
}

#[test]
fn a_touch_contact_patch_reaches_the_axes() {
    let source = ScriptedPen {
        contact: Some(Size::new(24.0, 18.0)),
        ..Default::default()
    };
    let mut state = TranslationState::new();
    state.set_pen_source(Box::new(source));

    let event = touch(winit::event::TouchPhase::Started, 7, 100.0, 100.0);
    let samples = feed(&mut state, &event, 1);
    let sample = pointers(&samples)[0];
    assert_eq!(
        sample.pointer.axes.contact,
        Some(Size::new(24.0, 18.0)),
        "WM_TOUCH carries no contact area; the WM_POINTER shim does"
    );
}

#[test]
fn a_window_with_no_pen_source_produces_no_pen_samples() {
    let mut state = TranslationState::new();
    assert!(!state.has_pen_source());
    assert!(state.poll_pen(EventTime::from_millis(1)).is_empty());
    assert!(!state.pen_in_proximity());
    // …and taking the source back is how a caller uninstalls one.
    state.set_pen_source(Box::new(ScriptedPen::with(vec![hover(1.0, 1.0)])));
    assert!(state.take_pen_source().is_some());
    assert!(state.poll_pen(EventTime::from_millis(2)).is_empty());
}

// ---------------------------------------------------------------------------
// Pen batching (`PenBatching::Coalesce`). The contract an ink surface stands
// on: whichever mode is set, the same positions reach a consumer that reads
// both `PointerSample::position` and `PointerSample::coalesced` — with their
// own times and their own axes.
// ---------------------------------------------------------------------------

use crate::pen::PenBatching;

/// A pressure-varying contact packet at `(x, y)`.
fn contact(x: f32, y: f32, pressure: f32, ms: u32) -> PenPacket {
    PenPacket::hovering(PenKind::Pen, Point::new(x, y))
        .down_at(pressure)
        .at_device_ms(ms)
}

/// Every position a consumer would draw through, oldest first: each sample's
/// batched positions, then its own.
fn drawn_positions(samples: &[InputSample]) -> Vec<(Point, Option<f32>)> {
    let mut out = Vec::new();
    for sample in samples.iter().filter_map(InputSample::as_pointer) {
        for c in &sample.coalesced {
            out.push((c.window_position, c.axes.pressure));
        }
        out.push((sample.position, sample.pointer.axes.pressure));
    }
    out
}

fn stroke_packets() -> Vec<PenPacket> {
    let mut packets = vec![hover(0.0, 0.0).at_device_ms(0)];
    for i in 0..8u32 {
        packets.push(contact(
            i as f32,
            i as f32 * 2.0,
            0.1 + i as f32 * 0.1,
            4 + i * 3,
        ));
    }
    packets
}

#[test]
fn coalescing_a_pen_batch_keeps_every_position_its_axes_and_its_time() {
    let per_packet = {
        let mut state = state_with_pen(stroke_packets());
        state.poll_pen(EventTime::from_millis(40))
    };
    let mut state = state_with_pen(stroke_packets());
    state.set_pen_batching(PenBatching::Coalesce);
    let coalesced = state.poll_pen(EventTime::from_millis(40));

    assert!(
        coalesced.len() < per_packet.len(),
        "Coalesce must actually fold: {} samples against {}",
        coalesced.len(),
        per_packet.len()
    );
    assert_eq!(
        drawn_positions(&coalesced),
        drawn_positions(&per_packet),
        "a consumer that reads `coalesced` then `position` must see the same \
         stroke under either mode — same points, same per-point pressure"
    );

    // And the times are per-position, not the drain's single `now`.
    let times: Vec<_> = coalesced
        .iter()
        .filter_map(InputSample::as_pointer)
        .flat_map(|s| {
            s.coalesced
                .iter()
                .map(|c| c.time)
                .chain(std::iter::once(s.pointer.time))
                .collect::<Vec<_>>()
        })
        .collect();
    let distinct: std::collections::BTreeSet<_> = times.iter().collect();
    assert!(
        distinct.len() > 1,
        "the batched positions must keep the device's own spacing, got {times:?}"
    );
    assert!(
        times.windows(2).all(|w| w[0] <= w[1]),
        "oldest first, and monotone: {times:?}"
    );
}

#[test]
fn coalescing_never_folds_a_transition() {
    // hover, hover, down, move, move, up-by-lifting, hover
    let packets = vec![
        hover(0.0, 0.0).at_device_ms(0),
        hover(1.0, 0.0).at_device_ms(3),
        contact(2.0, 0.0, 0.4, 6).at_device_ms(6),
        contact(3.0, 0.0, 0.5, 9),
        contact(4.0, 0.0, 0.6, 12),
        hover(5.0, 0.0).at_device_ms(15),
        hover(6.0, 0.0).at_device_ms(18),
    ];
    let phases = |mode: PenBatching| {
        let mut state = state_with_pen(packets.clone());
        state.set_pen_batching(mode);
        let samples = state.poll_pen(EventTime::from_millis(20));
        samples
            .iter()
            .filter_map(InputSample::as_pointer)
            .filter(|s| s.phase != PointerPhase::Move || s.button.is_some())
            .map(|s| (s.phase, s.button))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        phases(PenBatching::Coalesce),
        phases(PenBatching::PerPacket),
        "Down / Up / button changes keep their own samples under either mode"
    );
    assert!(
        !phases(PenBatching::Coalesce).is_empty(),
        "the fixture must contain transitions, or this asserts nothing"
    );
}

/// The proximity **enter** keeps its own sample under `Coalesce`, and that is
/// the half of "transitions are never folded" the fold cannot see for itself:
/// [`PointerPhase`] has no *enter*, so a tool coming into range rides as a
/// `Move` with nothing held — which is `coalesce_pen_moves`'s own definition of
/// foldable pure motion.
///
/// What would be lost is not a `PointerMove`. The tree derives the hover owner,
/// the cursor and the tooltip dwell from a sample's `position` and never from
/// its batched list, so a pen that came into range over one widget and hovered
/// onto another inside one drain would never enter the first at all — a hover
/// affordance that silently stops firing under a batching mode.
///
/// Stop pinning it — pass `&[]` in place of `&pinned` at `poll_pen`'s call to
/// `coalesce_pen_moves` — and the drain's first position becomes `(30, 30)`.
#[test]
fn coalescing_keeps_the_proximity_enters_own_sample() {
    let packets = || {
        vec![
            hover(0.0, 0.0).at_device_ms(0),
            hover(10.0, 10.0).at_device_ms(3),
            hover(20.0, 20.0).at_device_ms(6),
            hover(30.0, 30.0).at_device_ms(9),
        ]
    };

    let per_packet = {
        let mut state = state_with_pen(packets());
        state.poll_pen(EventTime::from_millis(12))
    };
    let mut state = state_with_pen(packets());
    state.set_pen_batching(PenBatching::Coalesce);
    let coalesced = state.poll_pen(EventTime::from_millis(12));

    let folded = pointers(&coalesced);
    assert_eq!(
        folded[0].position,
        Point::new(0.0, 0.0),
        "the entering position must be dispatched on its own, not batched into \
         the hover that followed it: got {:?}",
        folded.iter().map(|s| s.position).collect::<Vec<_>>()
    );
    assert!(
        folded[0].coalesced.is_empty(),
        "…and it is the sample, not a passenger on one"
    );
    assert_eq!(
        folded.len(),
        2,
        "the enter plus one folded hover run: {:?}",
        folded.iter().map(|s| s.position).collect::<Vec<_>>()
    );

    // Still a fold, or the assertion above is satisfied by doing nothing.
    assert!(
        coalesced.len() < per_packet.len(),
        "Coalesce must still fold the run behind the enter: {} samples against \
         {}",
        coalesced.len(),
        per_packet.len()
    );
    assert_eq!(
        drawn_positions(&coalesced),
        drawn_positions(&per_packet),
        "and every position still reaches a consumer that reads `coalesced` \
         then `position`"
    );
}

#[test]
fn a_hover_run_and_a_contact_run_do_not_fold_together() {
    let packets = vec![
        hover(0.0, 0.0).at_device_ms(0),
        hover(1.0, 0.0).at_device_ms(3),
        contact(2.0, 0.0, 0.4, 6),
        contact(3.0, 0.0, 0.5, 9),
    ];
    let mut state = state_with_pen(packets);
    state.set_pen_batching(PenBatching::Coalesce);
    let samples = state.poll_pen(EventTime::from_millis(12));

    for sample in samples.iter().filter_map(InputSample::as_pointer) {
        let down = !sample.pointer.buttons.is_empty();
        for c in &sample.coalesced {
            let batched_down = c.axes.pressure.is_some_and(|p| p > 0.0);
            assert_eq!(
                batched_down, down,
                "a batched position must belong to the same contact state as \
                 the sample carrying it"
            );
        }
    }
}

#[test]
fn per_packet_is_the_default_and_batches_nothing() {
    let state = TranslationState::new();
    assert_eq!(state.pen_batching(), PenBatching::PerPacket);

    let mut state = state_with_pen(stroke_packets());
    let samples = state.poll_pen(EventTime::from_millis(40));
    assert!(
        samples
            .iter()
            .filter_map(InputSample::as_pointer)
            .all(|s| s.coalesced.is_empty()),
        "the default must not populate `coalesced` — a consumer that reads only \
         `position` sees every packet, which is the whole point of PerPacket"
    );
}

#[test]
fn a_single_packet_drain_is_unchanged_by_coalescing() {
    let mut state = state_with_pen(vec![hover(3.0, 4.0).at_device_ms(7)]);
    state.set_pen_batching(PenBatching::Coalesce);
    let samples = state.poll_pen(EventTime::from_millis(9));
    let pointers = pointers(&samples);
    assert_eq!(pointers.len(), 1);
    assert!(pointers[0].coalesced.is_empty());
    assert_eq!(pointers[0].position, Point::new(3.0, 4.0));
}
