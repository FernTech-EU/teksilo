// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The two tuning panels: the density toggle, and the kinetic constants.
//!
//! # Why a density switch is two steps and not one
//!
//! A density lives in the theme *and* is decided inside `build()` — a `MinSize`
//! wrapper, a recipe's metrics, how many commands a toolbar fits. So switching
//! it needs the re-projected theme (`ctx.set_theme`, which the app layer applies
//! before the next layout pass) **and** a rebuild. `WidgetTree::set_input_density`
//! does exactly those two things in one call, and nothing on `EventContext`
//! reaches it — so an application does them itself: these panels call
//! `ctx.set_theme(theme.with_density(d))` and write a signal that
//! [`crate::Root`] binds at `BindingLevel::Rebuild`. Order matters: the theme
//! first, or the rebuild bakes the old tokens.
//!
//! The kinetic panel is the same shape for the same reason. Every scrollable
//! snapshots `theme.input.scroll_physics` in its own `build()`, and the tree's
//! fling driver reads it off the effective theme, so a changed constant reaches
//! a surface only through a rebuild.
//!
//! A density switch throws away every widget id in the tree, which is why the
//! toggle guards on "already there" rather than re-applying: a redundant rebuild
//! would discard the scroll offsets and the selection a tester had just set up.

use teksilo::core::Signal;
use teksilo::core::build_context::BuildContext;
use teksilo::core::widget::EventContext;
use teksilo::core::widget_id::WidgetId;
use teksilo::i18n::lit;
use teksilo::tokens::{ScrollPhysics, TargetDensity, TextRole, TextStyleRole};
use teksilo::widgets::primitives::Padding;
use teksilo::widgets::{
    Button, ButtonVariant, Checkbox, HStack, Segment, SegmentedControl, SpinBox, TextWidget, VStack,
};

use crate::state::{KineticKnobs, PlaygroundState};

/// The three rungs, in the order the toggle shows them.
const DENSITIES: [(TargetDensity, &str); 3] = [
    (TargetDensity::Compact, "Compact"),
    (TargetDensity::Comfortable, "Comfortable"),
    (TargetDensity::Touch, "Touch"),
];

/// The preset the playground themes from. Re-projected on every apply rather
/// than mutated in place, so the kinetic panel's reset is "drop the knobs"
/// instead of "remember what the theme said".
fn base_theme() -> teksilo::core::Theme {
    teksilo::presets::intui::light()
}

/// Fold the knob values into a theme's input tokens.
fn with_knobs(mut theme: teksilo::core::Theme, knobs: KineticKnobs) -> teksilo::core::Theme {
    theme.input.scroll_physics = knobs.apply(theme.input.scroll_physics);
    theme
}

/// Re-theme for the current density and knobs, and ask for the rebuild that
/// makes them reach a scrollable's `build()`.
fn reapply(ctx: &mut EventContext, state: &PlaygroundState) {
    let theme = with_knobs(
        base_theme().with_density(state.density.get()),
        state.kinetic.get(),
    );
    ctx.set_theme(theme);
    state.bump_rebuild();
}

/// Apply `density` to the tree and to the playground, both halves in order.
fn set_density(ctx: &mut EventContext, state: &PlaygroundState, density: TargetDensity) {
    if state.density.get() == density {
        return;
    }
    state.density.set(density);
    reapply(ctx, state);
    state.log(format!("density → {density:?}"));
}

/// The density toggle.
pub fn density_toggle(ctx: &mut BuildContext, state: &PlaygroundState) -> WidgetId {
    let index = Signal::new(
        DENSITIES
            .iter()
            .position(|(d, _)| *d == state.density.get())
            .unwrap_or(0),
    );
    let mut control = SegmentedControl::indexed(index).label(lit!("Target density"));
    for (_, label) in DENSITIES {
        control = control.segment(Segment::new(lit!(label)));
    }
    // The change handler is given the segment's id, not its position, so the id
    // list is captured to map one to the other. Taken after every segment is
    // added and before `on_change` consumes the builder.
    let ids = control.segment_ids();
    let for_change = state.clone();
    let control = control.on_change(move |id, ctx| {
        if let Some(i) = ids.iter().position(|candidate| *candidate == id) {
            set_density(ctx, &for_change, DENSITIES[i].0);
        }
    });

    let ladder = state.density.map(|density| {
        let tokens = teksilo::tokens::InputTokens::for_density(*density);
        format!(
            "target {} dp · grab {} dp · slop budget {} dp · spacing x{:.2} · reveal {:?}",
            tokens.target_size,
            tokens.grab_size,
            tokens.slop_budget,
            tokens.spacing_factor,
            tokens.reveal,
        )
    });

    ctx.add(
        Padding::uniform(8.0).child(
            VStack::new()
                .spacing(4.0)
                .child(control)
                .child(
                    TextWidget::new(lit!(""))
                        .text(ladder)
                        .style(TextStyleRole::Tiny)
                        .color(TextRole::Secondary),
                )
                .child(
                    TextWidget::new(lit!(
                        "Switching rebuilds the whole tree: scroll offsets and selections reset, \
                         and a screen reader is told once."
                    ))
                    .style(TextStyleRole::Tiny)
                    .color(TextRole::Secondary),
                ),
        ),
    )
}

/// The kinetic tuning panel.
///
/// Every control writes one field of [`KineticKnobs`]. The numbers start at the
/// shipped token values, read from `ScrollPhysicsTokens::DEFAULT` rather than
/// retyped here, so the panel cannot drift from the framework's own defaults.
pub fn kinetic_panel(ctx: &mut BuildContext, state: &PlaygroundState) -> WidgetId {
    let knobs = state.kinetic.get();
    let friction = Signal::new(knobs.clamping_friction);
    let decay = Signal::new(knobs.bouncing_decay_per_second);
    let band = Signal::new(knobs.rubber_band_factor);
    let rubber = Signal::new(knobs.rubber_band);

    // The controls write into the knobs struct; Apply is what re-themes. Editing
    // without applying is deliberate: a SpinBox writes on every step, and
    // re-theming on each one would rebuild the tree under the finger holding the
    // step button down.
    {
        let state = state.clone();
        let (f, d, b, r) = (
            friction.clone(),
            decay.clone(),
            band.clone(),
            rubber.clone(),
        );
        let collect = move || {
            let mut knobs = state.kinetic.get();
            knobs.clamping_friction = f.get();
            knobs.bouncing_decay_per_second = d.get();
            knobs.rubber_band_factor = b.get();
            knobs.rubber_band = r.get();
            state.kinetic.set(knobs);
        };
        for signal in [&friction, &decay, &band] {
            let collect = collect.clone();
            ctx.effect(signal, move |_| collect());
        }
        ctx.effect(&rubber, move |_| collect());
    }

    let summary = state.kinetic.map(|k| {
        format!(
            "{:?} · friction {:.3} · decay {:.3}/s · band x{:.2} · past-the-end {}",
            k.physics,
            k.clamping_friction,
            k.bouncing_decay_per_second,
            k.rubber_band_factor,
            if k.rubber_band { "on" } else { "off" },
        )
    });

    let clamping = state.clone();
    let bouncing = state.clone();
    let platform = state.clone();
    let apply = state.clone();
    let reset = state.clone();

    let column = VStack::new()
        .spacing(8.0)
        .child(TextWidget::new(lit!("Kinetic tuning")).style(TextStyleRole::SmallBold))
        .child(
            TextWidget::new(lit!(
                "Flick a surface below and let go. Edit, then Apply — a scrollable \
                 snapshots these constants in its build(), so nothing moves until \
                 the rebuild."
            ))
            .style(TextStyleRole::Tiny)
            .color(TextRole::Secondary),
        )
        .child(
            TextWidget::new(lit!(""))
                .text(summary)
                .style(TextStyleRole::Small),
        )
        .child(
            HStack::new()
                .spacing(8.0)
                .child(Button::new(lit!("Clamping")).on_activate_fn(move |ctx| {
                    set_physics(ctx, &clamping, ScrollPhysics::Clamping);
                }))
                .child(Button::new(lit!("Bouncing")).on_activate_fn(move |ctx| {
                    set_physics(ctx, &bouncing, ScrollPhysics::Bouncing);
                }))
                .child(Button::new(lit!("Platform")).on_activate_fn(move |ctx| {
                    set_physics(ctx, &platform, ScrollPhysics::Platform);
                })),
        )
        .child(
            SpinBox::new(friction, 0.001, 0.2)
                .label(lit!("clamping friction"))
                .single_step(0.001)
                .decimals(3),
        )
        .child(
            SpinBox::new(decay, 0.01, 1.0)
                .label(lit!("bouncing decay per second"))
                .single_step(0.005)
                .decimals(3),
        )
        .child(
            SpinBox::new(band, 0.05, 2.0)
                .label(lit!("rubber-band factor"))
                .single_step(0.01)
                .decimals(2),
        )
        .child(Checkbox::new(rubber).label(lit!("follow the finger past the end")))
        .child(
            HStack::new()
                .spacing(8.0)
                .child(
                    Button::new(lit!("Apply"))
                        .variant(ButtonVariant::Filled)
                        .on_activate_fn(move |ctx| reapply(ctx, &apply)),
                )
                .child(
                    Button::new(lit!("Reset to shipped")).on_activate_fn(move |ctx| {
                        reset.kinetic.set(KineticKnobs::shipped());
                        reapply(ctx, &reset);
                    }),
                ),
        );

    ctx.add(Padding::uniform(8.0).child(column))
}

/// Switch the simulation family and apply it in one gesture — the family is the
/// one knob a tester changes to feel a difference rather than to measure one.
fn set_physics(ctx: &mut EventContext, state: &PlaygroundState, physics: ScrollPhysics) {
    let mut knobs = state.kinetic.get();
    knobs.physics = physics;
    state.kinetic.set(knobs);
    reapply(ctx, state);
}
