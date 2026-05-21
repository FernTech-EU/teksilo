//! Styling tab — the four-tier styling ladder in one place.
//!
//! - **Tier 1 — Variants.** Every themable widget exposes a closed
//!   `*Variant` enum; `.variant(...)` picks the design-language shape.
//!   The grids below show Button / Toggle / Checkbox / Card side by
//!   side across all their variants.
//! - **Tier 3 — Style protocols.** A per-call `.style(impl FooStyle)`
//!   replaces the widget's chrome wholesale. `GlowButton` and
//!   `SquareToggle` below are ~10-line custom impls — no widget source
//!   forked.
//!
//! Tier 0 (tokens) is covered by the Palette tab; Tier 2 (recipes) is
//! the data-driven default impl behind every widget and isn't
//! separately demoed here. See `docs/styling-system.md`.

use bastyde::core::styles::{
    ButtonStyle, ButtonStyleConfig, CardVariant, CheckboxVariant, ToggleStyle, ToggleStyleConfig,
    ToggleVariant,
};
use bastyde::prelude::*;
use bastyde::widgets::{
    Button, ButtonVariant, Card, Checkbox, Divider, FixedSize, HStack, Padding, RectWidget,
    TextWidget, Toggle, VStack, ZStack,
};

use crate::shared::{Signals, section, tab_header};

pub fn title() -> LocalizedString {
    tr!(tab_styling_title())
}

pub fn refs() -> LocalizedString {
    tr!(tab_styling_refs())
}

// ── Tier-3 custom styles ────────────────────────────────────────────
//
// Two tiny `impl FooStyle` blocks. They receive a `*StyleConfig`
// carrying interaction signals (+ the pre-built label subtree for
// buttons) and return the WidgetId of a composed chrome subtree.

/// Soft-glow pill button — accent-subtle at rest, brightens through the
/// accent family on hover/press. Mirrors the `theme_styles` example's
/// `GlowButton`.
struct GlowButton;

impl ButtonStyle for GlowButton {
    fn make_body(&self, cfg: &ButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId {
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
                .corner_radius(CornerRadius::uniform(16.0)),
        );
        let padded_label = ctx.add(Padding::new(6.0, 20.0, 6.0, 20.0).child_id(cfg.label));
        ctx.add(ZStack::new().add_child(body).add_child(padded_label))
    }
}

/// Square indicator toggle — a 36×20 rounded rect that fills with the
/// accent when on, sits in the sunken surface when off. No sliding
/// knob; the whole body IS the state.
struct SquareToggle;

impl ToggleStyle for SquareToggle {
    fn make_body(&self, cfg: &ToggleStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let bg =
            cfg.is_on
                .zip3(&cfg.is_hovered, &cfg.is_disabled)
                .map(|(on, hovered, disabled)| {
                    if *disabled {
                        SurfaceRole::AccentDisabled
                    } else if *on {
                        SurfaceRole::Accent
                    } else if *hovered {
                        SurfaceRole::Hover
                    } else {
                        SurfaceRole::Sunken
                    }
                });
        let rect = ctx.add(
            RectWidget::new()
                .bind_background(bg)
                .corner_radius(CornerRadius::uniform(3.0)),
        );
        ctx.add(
            FixedSize::new()
                .bind_width(36.0_f32)
                .bind_height(20.0_f32)
                .child_id(rect),
        )
    }
}

// ── Reusable labelled-cell helper ───────────────────────────────────
//
// A small column: the variant's name on top, the live widget below.
// Keeps the variant grids self-documenting without a legend.

fn labelled(label: &'static str, body: impl Widget + 'static) -> impl Widget + 'static {
    VStack::new()
        .spacing(4.0)
        .child(
            TextWidget::new(lit!(label))
                .style(TextStyleRole::Small)
                .color(TextRole::Secondary),
        )
        .child(body)
}

pub fn classic(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // All the variant-grid widgets need their own bound signal;
    // `ctx.signal(...)` can't be called inside a `section(ctx, …)`
    // argument list (that would borrow `ctx` twice), so the signals
    // are minted up front.
    let tog_switch = ctx.signal(true);
    let tog_pill = ctx.signal(true);
    let tog_square = ctx.signal(true);
    let tog_inset = ctx.signal(true);
    let cb_square = ctx.signal(true);
    let cb_rounded = ctx.signal(true);
    let cb_circle = ctx.signal(true);
    let tog_default = ctx.signal(true);
    let tog_custom = ctx.signal(true);

    let header = tab_header(ctx, title(), refs());

    // ── Tier 1: Button variants ─────────────────────────────────────
    // Seven variants; split across two rows so the grid stays legible.
    let button_variants = section(
        ctx,
        tr!(sty_tier1_button_variant_heading()),
        VStack::new()
            .spacing(8.0)
            .child(
                HStack::new()
                    .spacing(8.0)
                    .child(labelled(
                        "Filled",
                        Button::new(lit!("Filled")).variant(ButtonVariant::Filled),
                    ))
                    .child(labelled(
                        "Tinted",
                        Button::new(lit!("Tinted")).variant(ButtonVariant::Tinted),
                    ))
                    .child(labelled(
                        "Outlined",
                        Button::new(lit!("Outlined")).variant(ButtonVariant::Outlined),
                    ))
                    .child(labelled(
                        "Plain",
                        Button::new(lit!("Plain")).variant(ButtonVariant::Plain),
                    )),
            )
            .child(
                HStack::new()
                    .spacing(8.0)
                    .child(labelled(
                        "Ghost",
                        Button::new(lit!("Ghost")).variant(ButtonVariant::Ghost),
                    ))
                    .child(labelled(
                        "Link",
                        Button::new(lit!("Link")).variant(ButtonVariant::Link),
                    ))
                    .child(labelled(
                        "Destructive",
                        Button::new(lit!("Delete")).variant(ButtonVariant::Destructive),
                    )),
            ),
    );

    // ── Tier 1: Toggle variants ─────────────────────────────────────
    let toggle_variants = section(
        ctx,
        tr!(sty_tier1_toggle_variant_heading()),
        HStack::new()
            .spacing(16.0)
            .child(labelled(
                "Switch",
                Toggle::new(tog_switch)
                    .variant(ToggleVariant::Switch)
                    .label(lit!("Switch")),
            ))
            .child(labelled(
                "Pill",
                Toggle::new(tog_pill)
                    .variant(ToggleVariant::Pill)
                    .label(lit!("Pill")),
            ))
            .child(labelled(
                "Square",
                Toggle::new(tog_square)
                    .variant(ToggleVariant::Square)
                    .label(lit!("Square")),
            ))
            .child(labelled(
                "Inset",
                Toggle::new(tog_inset)
                    .variant(ToggleVariant::Inset)
                    .label(lit!("Inset")),
            )),
    );

    // ── Tier 1: Checkbox variants ───────────────────────────────────
    let checkbox_variants = section(
        ctx,
        tr!(sty_tier1_checkbox_variant_heading()),
        HStack::new()
            .spacing(16.0)
            .child(labelled(
                "Square",
                Checkbox::new(cb_square)
                    .variant(CheckboxVariant::Square)
                    .label(lit!("Square")),
            ))
            .child(labelled(
                "Rounded",
                Checkbox::new(cb_rounded)
                    .variant(CheckboxVariant::Rounded)
                    .label(lit!("Rounded")),
            ))
            .child(labelled(
                "Circle",
                Checkbox::new(cb_circle)
                    .variant(CheckboxVariant::Circle)
                    .label(lit!("Circle")),
            )),
    );

    // ── Tier 1: Card variants ───────────────────────────────────────
    let card_variants = section(
        ctx,
        tr!(sty_tier1_card_variant_heading()),
        HStack::new()
            .spacing(12.0)
            .child(labelled(
                "Plain",
                Card::new()
                    .variant(CardVariant::Plain)
                    .content(TextWidget::new(lit!("Plain")).style(TextStyleRole::Small)),
            ))
            .child(labelled(
                "Elevated",
                Card::new()
                    .variant(CardVariant::Elevated)
                    .content(TextWidget::new(lit!("Elevated")).style(TextStyleRole::Small)),
            ))
            .child(labelled(
                "Outlined",
                Card::new()
                    .variant(CardVariant::Outlined)
                    .content(TextWidget::new(lit!("Outlined")).style(TextStyleRole::Small)),
            ))
            .child(labelled(
                "Filled",
                Card::new()
                    .variant(CardVariant::Filled)
                    .content(TextWidget::new(lit!("Filled")).style(TextStyleRole::Small)),
            )),
    );

    // ── Tier 3: per-call custom styles ──────────────────────────────
    // The `.style(...)` override swaps the entire chrome. Each custom
    // widget sits next to its IntUI-default sibling for comparison.
    let custom_button = section(
        ctx,
        tr!(sty_tier3_button_style_heading()),
        HStack::new()
            .spacing(12.0)
            .child(labelled(
                "default",
                Button::new(lit!("Default")).variant(ButtonVariant::Filled),
            ))
            .child(labelled(
                ".style(GlowButton)",
                Button::new(lit!("Glow")).style(GlowButton),
            )),
    );

    let custom_toggle = section(
        ctx,
        tr!(sty_tier3_toggle_style_heading()),
        HStack::new()
            .spacing(16.0)
            .child(labelled(
                "default",
                Toggle::new(tog_default).label(lit!("Default")),
            ))
            .child(labelled(
                ".style(SquareToggle)",
                Toggle::new(tog_custom)
                    .style(SquareToggle)
                    .label(lit!("Square")),
            )),
    );

    ctx.add(
        VStack::new()
            .spacing(20.0)
            .add_child(header)
            .child(Divider::new())
            .add_child(button_variants)
            .add_child(toggle_variants)
            .add_child(checkbox_variants)
            .add_child(card_variants)
            .add_child(custom_button)
            .add_child(custom_toggle),
    )
}

pub fn bati(ctx: &mut BuildContext, _sigs: &Signals) -> WidgetId {
    // `bati!` borrows `ctx` for the whole block, so anything needing
    // `&mut ctx` — fresh signals, the `.style(...)`-carrying widgets
    // whose value is a plain Rust expr the DSL can't take inline —
    // is created up front and spliced via `#{ … }`.
    let tog_switch = ctx.signal(true);
    let tog_pill = ctx.signal(true);
    let tog_square = ctx.signal(true);
    let tog_inset = ctx.signal(true);
    let cb_square = ctx.signal(true);
    let cb_rounded = ctx.signal(true);
    let cb_circle = ctx.signal(true);
    let tog_default = ctx.signal(true);
    let tog_custom = ctx.signal(true);

    // Tier-3 rows: the per-call `.style(impl FooStyle)` override takes
    // a value the `bati!` property grammar can't express directly, so
    // these are built with the plain builder API and spliced in.
    let glow_button = ctx.add(Button::new(lit!("Glow")).style(GlowButton));
    let square_toggle = ctx.add(
        Toggle::new(tog_custom)
            .style(SquareToggle)
            .label(lit!("Square")),
    );

    bati!(ctx => VStack {
            spacing: 20.0
            VStack {
                spacing: 4.0
                TextWidget::new(tr!(tab_styling_title())) {
                    style: TextStyleRole::BodyBold
                    color: TextRole::Primary
                }
                TextWidget::new(tr!(tab_styling_refs())) {
                    style: TextStyleRole::Small
                    color: TextRole::Secondary
                }
            }
            Divider

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(sty_tier1_button_variant_heading())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 8.0
                    Button::new(lit!("Filled")) {
                        variant: ButtonVariant::Filled
                    }
                    Button::new(lit!("Tinted")) {
                        variant: ButtonVariant::Tinted
                    }
                    Button::new(lit!("Outlined")) {
                        variant: ButtonVariant::Outlined
                    }
                    Button::new(lit!("Plain")) {
                        variant: ButtonVariant::Plain
                    }
                }
                HStack {
                    spacing: 8.0
                    Button::new(lit!("Ghost")) {
                        variant: ButtonVariant::Ghost
                    }
                    Button::new(lit!("Link")) {
                        variant: ButtonVariant::Link
                    }
                    Button::new(lit!("Delete")) {
                        variant: ButtonVariant::Destructive
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(sty_tier1_toggle_variant_heading())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 16.0
                    Toggle::new(tog_switch) {
                        variant: ToggleVariant::Switch
                        label_literal: "Switch"
                    }
                    Toggle::new(tog_pill) {
                        variant: ToggleVariant::Pill
                        label_literal: "Pill"
                    }
                    Toggle::new(tog_square) {
                        variant: ToggleVariant::Square
                        label_literal: "Square"
                    }
                    Toggle::new(tog_inset) {
                        variant: ToggleVariant::Inset
                        label_literal: "Inset"
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(sty_tier1_checkbox_variant_heading())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 16.0
                    Checkbox::new(cb_square) {
                        variant: CheckboxVariant::Square
                        label_literal: "Square"
                    }
                    Checkbox::new(cb_rounded) {
                        variant: CheckboxVariant::Rounded
                        label_literal: "Rounded"
                    }
                    Checkbox::new(cb_circle) {
                        variant: CheckboxVariant::Circle
                        label_literal: "Circle"
                    }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(sty_tier3_button_style_heading())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 12.0
                    Button::new(lit!("Default")) {
                        variant: ButtonVariant::Filled
                    }
                    #{ glow_button }
                }
            }

            VStack {
                spacing: 6.0
                TextWidget::new(tr!(sty_tier3_toggle_style_heading())) {
                    style: TextStyleRole::SmallBold
                    color: TextRole::Accent
                }
                HStack {
                    spacing: 16.0
                    Toggle::new(tog_default) {
                        label_literal: "Default"
                    }
                    #{ square_toggle }
                }
            }
        }
    )
}
