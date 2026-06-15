// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Demonstrates the four-tier styling system in action — particularly
//! the typed slot bag (`theme.style_slots.<widget>`) and per-call
//! `.style(...)` overrides that landed with the styling refactor.
//!
//! Run with: `cargo run -p theme-styles`
//!
//! Three columns side-by-side:
//!
//! 1. **Default IntUI** — buttons painted by the bundled
//!    `RecipeButtonStyle` reading IntUI tokens. Toggle and Checkbox
//!    use their built-in defaults too (their slots are None).
//! 2. **Theme-wide override** — a custom `GlowButton` style installed
//!    on `theme.style_slots.button`. EVERY button — and any other
//!    button in the same theme — picks it up automatically.
//! 3. **Per-call override** — a one-off `.style(BrutalistButton)` on
//!    a single button. Wins over the theme-slot install; sibling
//!    buttons in the same column still get the GlowButton install.
//!
//! See `docs/styling-system.md` for the full reference.

use std::rc::Rc;

use bastyde::core::styles::{ButtonStyle, ButtonStyleConfig, CardVariant};
use bastyde::core::{BuildContext, WidgetId};
use bastyde::prelude::*;
use bastyde::tokens::{Color, CornerRadius, SurfaceRole, TextRole, TextStyleRole};
use bastyde::widgets::primitives::{HStack, Padding, RectWidget, TextWidget, VStack, ZStack};
use bastyde::widgets::{Button, ButtonVariant, Card, Toggle};

// ── Custom ButtonStyle #1: a soft glow + pill shape ─────────────────────────
struct GlowButton;

impl ButtonStyle for GlowButton {
    fn make_body(&self, cfg: &ButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // Bg switches to the accent family on hover/press; idle is a
        // muted accent-subtle that hints at the call-to-action without
        // shouting.
        let bg = cfg.is_pressed.zip3(&cfg.is_hovered, &cfg.is_disabled).map(
            |(pressed, hovered, disabled)| {
                if *disabled {
                    SurfaceRole::AccentDisabled
                } else if *pressed {
                    SurfaceRole::AccentPressed
                } else if *hovered {
                    SurfaceRole::AccentHover
                } else {
                    SurfaceRole::AccentSubtle
                }
            },
        );

        let body = ctx.add(
            RectWidget::new()
                .bind_background(bg)
                .corner_radius(CornerRadius::uniform(20.0)), // pill-ish
        );
        let padded_label = ctx.add(Padding::new(8.0, 24.0, 8.0, 24.0).child_id(cfg.label));
        ctx.add(ZStack::new().add_child(body).add_child(padded_label))
    }
}

// ── Custom ButtonStyle #2: brutalist — sharp corners, hot-pink fill ─────────
struct BrutalistButton;

impl ButtonStyle for BrutalistButton {
    fn make_body(&self, cfg: &ButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        // Static hot-pink fill regardless of hover state — brutalist
        // convention is "ignore interaction theatre". A small darken
        // on press keeps the affordance honest.
        let bg = cfg.is_pressed.map(|pressed| {
            if *pressed {
                Color::new(0.85, 0.0, 0.45, 1.0)
            } else {
                Color::new(1.0, 0.0, 0.5, 1.0) // hot pink
            }
        });
        let body = ctx.add(
            RectWidget::new()
                .bind_background(bg)
                .corner_radius(CornerRadius::uniform(0.0)) // sharp
                .border_width(2.0)
                .border_color(Color::BLACK),
        );
        let padded_label = ctx.add(Padding::new(10.0, 20.0, 10.0, 20.0).child_id(cfg.label));
        ctx.add(ZStack::new().add_child(body).add_child(padded_label))
    }
}

fn main() {
    // Build a theme with the GlowButton installed theme-wide.
    let mut theme = intui::light();
    theme.style_slots.button = Some(Rc::new(GlowButton));

    BastydeAppBuilder::new()
        .theme(theme)
        .install_inspector_in_debug()
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Styling System Demo")
                .size(900, 500)
                .root(|tree, _state| tree.add(Demo)),
        )
        .run();
}

#[derive(Debug)]
struct Demo;

impl Widget for Demo {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Three columns of cards. Cards themselves are also style-aware
        // (they go through `RecipeCardStyle` by default — proves the
        // migration is working), but we don't override the card style
        // here so the chrome stays IntUI-default.
        let column1 = column_card(
            "Default IntUI",
            "Toggle and Checkbox use their built-in RecipeFooStyle \
             defaults — the GlowButton install only touches the \
             button slot.",
            VStack::new()
                .spacing(12.0)
                .child(TextWidget::new(lit!("Title")).style(TextStyleRole::BodyBold))
                .child(Toggle::new(Signal::new(false)).label(lit!("Notifications")))
                .child(Toggle::new(Signal::new(true)).label(lit!("Dark mode"))),
        );

        let column2 = column_card(
            "Theme-wide GlowButton",
            "Both buttons are picked up by the GlowButton style \
             installed on `theme.style_slots.button` — no per-call \
             `.style()` needed.",
            VStack::new()
                .spacing(12.0)
                .child(TextWidget::new(lit!("Both glowing")).style(TextStyleRole::BodyBold))
                .child(
                    Button::new(lit!("Save"))
                        .variant(ButtonVariant::Filled)
                        .on_activate_fn(|_| println!("save")),
                )
                .child(
                    Button::new(lit!("Cancel"))
                        .variant(ButtonVariant::Plain)
                        .on_activate_fn(|_| println!("cancel")),
                ),
        );

        let column3 = column_card(
            "Per-call BrutalistButton",
            "The first button has `.style(BrutalistButton)` — \
             overrides BOTH the slot AND the recipe default. The \
             sibling button still gets the theme-wide GlowButton.",
            VStack::new()
                .spacing(12.0)
                .child(TextWidget::new(lit!("Mixed")).style(TextStyleRole::BodyBold))
                .child(
                    Button::new(lit!("BRUTAL"))
                        .style(BrutalistButton)
                        .on_activate_fn(|_| println!("brutal")),
                )
                .child(
                    Button::new(lit!("glow"))
                        .variant(ButtonVariant::Plain)
                        .on_activate_fn(|_| println!("glow")),
                ),
        );

        let root = ctx.add(
            HStack::new()
                .spacing(20.0)
                .child(column1)
                .child(column2)
                .child(column3),
        );
        vec![root]
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &bastyde::core::widget::LayoutContext,
    ) -> bastyde::core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn column_card(
    title: &'static str,
    description: &'static str,
    body: impl Widget + 'static,
) -> impl Widget {
    Padding::uniform(16.0).child(
        VStack::new()
            .spacing(10.0)
            .child(TextWidget::new(lit!(title)).style(TextStyleRole::BodyBold))
            .child(
                TextWidget::new(lit!(description))
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
            )
            .child(Card::new().variant(CardVariant::Plain).content(body)),
    )
}
