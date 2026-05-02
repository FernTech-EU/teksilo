//! Animations Kit — interactive showcase for every animation
//! primitive shipped to date. Each section demonstrates one piece of
//! the animation surface and doubles as a visual regression baseline:
//!
//! - **Toggle** — `AnimationSpec::fast().standard()` + `to_or_snap`.
//! - **Collapse** — 0..1 progress signal scaling natural height,
//!   driven by a `Signal<bool>`.
//! - **Fade** — node-level opacity scope (`set_opacity`) animating an
//!   internal `Signal<f32>` via `AnimationSpec`.
//! - **Spinner** — shader-driven `AnimatedQuadKind::SpinnerArc`
//!   (~one uniform write + one `draw_indexed` per frame).
//! - **Indeterminate ProgressBar** — pre-existing
//!   `AnimatedQuadKind::IndeterminateSweep` pipeline; included as a
//!   regression proof that the older shader path coexists with the
//!   new SpinnerArc kind.
//! - **Tooltip with fade** — `OverlayRequest::with_fade(duration)`
//!   coordinated with the overlay manager's deferred dismiss.
//!
//! Run with: `cargo run -p animations-kit`

use std::time::Duration;

use fern_ui::prelude::*;
use fern_ui::widgets::{
    Button, ButtonVariant, Card, Center, Collapse, Crossfade, Cycle, Divider, Fade, FixedSize,
    HStack, Padding, Panel, ProgressBar, Pulse, RectWidget, Rotate, Scale, ScaleOrigin, ScrollArea,
    Shake, Slide, SlideEdge, SmoothSize, Spinner, TextWidget, Toggle, VStack, ZStack,
};

fn main() {
    FernAppBuilder::new()
        .theme(Theme::light_default())
        .initial_window(
            WindowConfig::new()
                .title("FernUI — Animations Kit")
                .size(560, 720)
                .root(|tree, _state| {
                    let toggle_state = Signal::new(false);
                    let collapse_expanded = Signal::new(false);
                    let fade_visible = Signal::new(true);
                    tree.add(
                        ScrollArea::new().child(
                            Padding::uniform(24.0).child(
                                build_kit(toggle_state, collapse_expanded, fade_visible),
                            ),
                        ),
                    )
                }),
        )
        .run();
}

fn build_kit(
    toggle_state: Signal<bool>,
    collapse_expanded: Signal<bool>,
    fade_visible: Signal<bool>,
) -> impl Widget + 'static {
    // New-section signals. Kept here to avoid threading through `main`.
    let smooth_size_long = Signal::new(false);
    let crossfade_key = Signal::new(0_u32);
    let slide_visible = Signal::new(false);
    let shake_trigger = Signal::new(0_u32);
    let scale_visible = Signal::new(true);
    let scale_reflow_visible = Signal::new(true);
    let rotate_angle = Signal::new_animated(0.0_f32);

    VStack::new()
        .spacing(20.0)
        .child(section_header("Toggle"))
        .child(caption(
            "Animated knob slide via AnimationSpec::fast().standard() + to_or_snap.",
        ))
        .child(Toggle::new(toggle_state).label_literal("Animate me"))
        .child(Divider::new())
        .child(section_header("Collapse"))
        .child(caption(
            "Animates child between hidden and natural via a 0..1 progress signal scaling \
             natural height; framework's clip pass crops the overflow.",
        ))
        .child(toggle_button("Toggle Collapse", collapse_expanded.clone()))
        .child(
            Collapse::new(collapse_expanded).child(
                VStack::new()
                    .spacing(6.0)
                    .child(TextWidget::new_literal("Hidden content #1"))
                    .child(TextWidget::new_literal("Hidden content #2"))
                    .child(TextWidget::new_literal("Hidden content #3")),
            ),
        )
        .child(Divider::new())
        .child(section_header("Fade"))
        .child(caption(
            "Animates the child's opacity 0↔1 via the rendering walker's opacity scope \
             (BuildContext::set_opacity). Layout-transparent — the child stays at its \
             natural size at all opacity values.",
        ))
        .child(toggle_button("Toggle Fade", fade_visible.clone()))
        .child(
            Fade::new(fade_visible).child(
                TextWidget::new_literal(
                    "  ●  Faded content — opacity tweens between 0 and 1.",
                ),
            ),
        )
        .child(Divider::new())
        .child(section_header("Spinner"))
        .child(caption(
            "Shader-driven via AnimatedQuadKind::SpinnerArc — ~one uniform write + one \
             draw_indexed per frame, no paint() re-runs.",
        ))
        .child(
            HStack::new()
                .spacing(16.0)
                .child(Spinner::new(20.0))
                .child(Spinner::new(28.0))
                .child(Spinner::new(40.0).period(Duration::from_millis(1400)))
                .child(
                    Spinner::new(28.0)
                        .arc_fraction(0.5)
                        .color(TextRole::Primary),
                ),
        )
        .child(Divider::new())
        .child(section_header("Indeterminate ProgressBar"))
        .child(caption(
            "Pre-existing AnimatedQuadKind::IndeterminateSweep — included as a regression \
             proof that the older shader path still works alongside the new SpinnerArc kind.",
        ))
        .child(ProgressBar::indeterminate().label_literal("Loading"))
        .child(Divider::new())
        .child(section_header("Tooltip with fade"))
        .child(caption(
            "OverlayRequest::with_fade(...) — framework attaches an animated opacity scope \
             on the content, runs the 0→1 fade-in tween at show and 1→0 on dismiss, and \
             defers the actual stack removal until the tween completes. Hover the button.",
        ))
        .child(
            Button::new_literal("Hover me")
                .style(ButtonVariant::Default)
                .tooltip_literal(
                    "I fade in and out over `motion.duration_fast` (~120 ms).",
                ),
        )
        .child(Divider::new())
        .child(section_header("Pulse"))
        .child(caption(
            "Sine-driven opacity oscillation between min and max — the recording-light / \
             attention-beacon pattern. Layout-transparent, like Fade.",
        ))
        .child(
            HStack::new().spacing(12.0).child(
                Pulse::opacity(0.25, 1.0)
                    .period(Duration::from_millis(1100))
                    .child(
                        FixedSize::new()
                            .bind_width(14.0)
                            .bind_height(14.0)
                            .child(
                                RectWidget::new()
                                    .background(Color::from_rgb(0.85, 0.18, 0.20))
                                    .corner_radius(CornerRadius::uniform(7.0)),
                            ),
                    ),
            ).child(TextWidget::new_literal("REC")),
        )
        .child(Divider::new())
        .child(section_header("Cycle"))
        .child(caption(
            "Steps through children on a fixed period — rotating loading tips, status \
             displays. Internally a Switcher driven by a frame-tick effect.",
        ))
        .child(
            Cycle::new()
                .period(Duration::from_secs(2))
                .child(TextWidget::new_literal("Tip 1: hold Shift to multi-select"))
                .child(TextWidget::new_literal("Tip 2: press Cmd-K to search"))
                .child(TextWidget::new_literal("Tip 3: drag the divider to resize")),
        )
        .child(Divider::new())
        .child(section_header("SmoothSize"))
        .child(caption(
            "Auto-sizes the slot to fit the child's intrinsic size, animating the change. \
             Toggle to see the panel grow / shrink as content is added or removed.",
        ))
        .child(toggle_button("Toggle content", smooth_size_long.clone()))
        .child(
            SmoothSize::new()
                .duration(Duration::from_millis(220))
                .child(Card::new().content(Crossfade::new(
                    smooth_size_long.clone(),
                    |&long| -> Box<dyn Widget> {
                        if long {
                            Box::new(
                                VStack::new()
                                    .spacing(4.0)
                                    .child(TextWidget::new_literal("Now there's more content."))
                                    .child(TextWidget::new_literal("The panel grows to fit it."))
                                    .child(TextWidget::new_literal("All animated, no jumps.")),
                            )
                        } else {
                            Box::new(TextWidget::new_literal("Short."))
                        }
                    },
                ))),
        )
        .child(Divider::new())
        .child(section_header("Crossfade"))
        .child(caption(
            "Animated content swap when a key changes. Outgoing fades out while incoming \
             fades in — like Switcher, but smooth.",
        ))
        .child(
            Button::new_literal("Next page")
                .style(ButtonVariant::Regular)
                .on_activate_fn({
                    let key = crossfade_key.clone();
                    move |_| key.set((key.get() + 1) % 3)
                }),
        )
        .child(Crossfade::new(crossfade_key, |k| -> Box<dyn Widget> {
            let label = match k {
                0 => "📄  Page A — overview",
                1 => "📊  Page B — details",
                _ => "🔧  Page C — settings",
            };
            Box::new(Panel::new().child(Padding::uniform(16.0).child(TextWidget::new_literal(label))))
        }))
        .child(Divider::new())
        .child(section_header("Slide"))
        .child(caption(
            "Slides a child in / out from a chosen edge. Layout-stable: siblings don't \
             reflow, the slot stays put. Pair with Fade for the snackbar pattern.",
        ))
        .child(toggle_button("Toggle banner", slide_visible.clone()))
        .child(
            Slide::new(slide_visible.clone())
                .from(SlideEdge::Trailing)
                .child(
                    Fade::new(slide_visible).child(
                        Card::new().content(TextWidget::new_literal(
                            "⚠  Banner — slides + fades.",
                        )),
                    ),
                ),
        )
        .child(Divider::new())
        .child(section_header("Shake"))
        .child(caption(
            "Damped horizontal oscillation, played on each trigger bump — the invalid-input \
             feedback pattern. Click the button to shake the field.",
        ))
        .child(
            VStack::new()
                .spacing(8.0)
                .child(
                    Shake::new(shake_trigger.clone()).child(
                        Card::new()
                            .content(TextWidget::new_literal("incorrect-password-input-field")),
                    ),
                )
                .child(
                    Button::new_literal("Submit")
                        .style(ButtonVariant::Regular)
                        .on_activate_fn(move |_| {
                            shake_trigger.set(shake_trigger.get() + 1);
                        }),
                ),
        )
        .child(Divider::new())
        .child(section_header("Scale (visual-only)"))
        .child(caption(
            "Visual scale 0↔1 around the slot center via BuildContext::set_transform. \
             Wrapper bounds stay at natural — siblings don't reflow. Use for overlay enter/exit \
             and 'boop' feedback on a Card.",
        ))
        .child(toggle_button("Toggle Scale", scale_visible.clone()))
        .child(
            Scale::new(scale_visible).child(
                Card::new().content(TextWidget::new_literal(
                    "I shrink/grow visually around my center.",
                )),
            ),
        )
        .child(Divider::new())
        .child(section_header("Scale (reflow)"))
        .child(caption(
            "Scale with .reflow(true) — the slot itself shrinks, siblings reflow inward. \
             Use TopLeading origin so the visual stays anchored as the slot collapses. \
             The 'card disappears by shrinking' pattern.",
        ))
        .child(toggle_button("Toggle Card", scale_reflow_visible.clone()))
        .child(
            HStack::new()
                .spacing(8.0)
                .child(TextWidget::new_literal("Before:"))
                .child(
                    Scale::new(scale_reflow_visible)
                        .reflow(true)
                        .origin(ScaleOrigin::TopLeading)
                        .child(Card::new().content(TextWidget::new_literal("removable card"))),
                )
                .child(TextWidget::new_literal(":After")),
        )
        .child(Divider::new())
        .child(section_header("Rotate"))
        .child(caption(
            "Bind any Signal<f32> of radians to rotate a child subtree. No internal animation; \
             pair with animate_to for animated rotations. The 80×80 square below rotates around \
             its center — the small black dot marks the expected pivot.",
        ))
        .child(
            // Side-by-side: rotating-cube-with-pivot-dot, then button.
            HStack::new()
                .spacing(20.0)
                .child(
                    // 80×80 ZStack: rotating cube on the bottom layer,
                    // a 6×6 black reference dot centered on top. If
                    // Rotate's pivot matches the slot center, the cube
                    // rotates around the dot.
                    FixedSize::new()
                        .bind_width(80.0)
                        .bind_height(80.0)
                        .child(
                            ZStack::new()
                                .child(
                                    Rotate::new(rotate_angle.clone()).child(
                                        RectWidget::new()
                                            .background(Color::from_rgb(0.30, 0.55, 0.85)),
                                    ),
                                )
                                .child(
                                    Center::new().child(
                                        FixedSize::new()
                                            .bind_width(6.0)
                                            .bind_height(6.0)
                                            .child(
                                                RectWidget::new().background(
                                                    Color::from_rgb(0.0, 0.0, 0.0),
                                                ),
                                            ),
                                    ),
                                ),
                        ),
                )
                .child({
                    let angle = rotate_angle.clone();
                    Button::new_literal("Rotate 90°")
                        .style(ButtonVariant::Regular)
                        .on_activate_fn(move |_| {
                            let target = angle.get() + std::f32::consts::FRAC_PI_2;
                            angle.animate_to(
                                target,
                                Duration::from_millis(400),
                                fern_ui::tokens::Easing::EaseOut,
                            );
                        })
                }),
        )
}

fn section_header(title: &str) -> impl Widget + 'static {
    TextWidget::new_literal(title)
        .style(TextStyleRole::BodyBold)
        .color(TextRole::Primary)
}

fn caption(text: &str) -> impl Widget + 'static {
    TextWidget::new_literal(text)
        .style(TextStyleRole::Small)
        .color(TextRole::Secondary)
}

fn toggle_button(label: &str, signal: Signal<bool>) -> impl Widget + 'static {
    let label = label.to_string();
    Button::new_literal(label)
        .style(ButtonVariant::Regular)
        .on_activate_fn(move |_ctx| {
            signal.set(!signal.get());
        })
}
