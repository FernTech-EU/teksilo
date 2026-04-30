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
    Button, ButtonVariant, Collapse, Divider, Fade, HStack, Padding, ProgressBar, ScrollArea,
    Spinner, TextWidget, Toggle, VStack,
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
