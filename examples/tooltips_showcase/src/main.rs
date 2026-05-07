//! Tooltips showcase — three-tier cascading demo.
//!
//! Run with: `cargo run -p tooltips-showcase`
//!
//! Three columns, each demonstrating one tooltip tier and cascade
//! depth:
//!
//! 1. **Plain** — single-line text via `.tooltip_literal(...)`.
//! 2. **Rich** — `[label](:key)` cascade three levels deep, plus
//!    `.with_more(...)` Accordion bodies and shortcut chips.
//! 3. **Composite** — `VStack` with header / `ProgressBar` / stat
//!    grid (each stat has its own `.rich_tooltip(...)` cascading
//!    *into* the rich-tooltip chain — multi-tier mixing). One sample
//!    embeds an internal `TabWidget`. One sample includes a rare
//!    `Button` to prove keyboard-focus reachability post-promotion.

use fern_ui::prelude::*;
use fern_ui::widgets::tooltip::TooltipContent;
use fern_ui::widgets::{
    Button, Expand, HStack, Padding, Panel, ProgressBar, Spacer, TabInfo, TabWidget, TextWidget,
    VStack,
};

// ── Rich-tooltip registry keys ───────────────────────────────────────────
//
// Three-deep cascade: KEY_A links to KEY_B links to KEY_C. Each level
// also has a `.with_more(...)` body so the Accordion disclosure adds
// a fourth interaction surface.

const KEY_A: &str = "tip-a";
const KEY_B: &str = "tip-b";
const KEY_C: &str = "tip-c";

// Per-stat keys cascaded *from* the composite tooltips.
const KEY_FOOD: &str = "stat-food";
const KEY_TRADE: &str = "stat-trade";
const KEY_HAPPINESS: &str = "stat-happiness";

fn build_tooltip_registry() -> Vec<TooltipContent> {
    vec![
        TooltipContent::new(
            KEY_A,
            LocalizedString::literal(
                "Level 1 of the cascade. Hover the [next link](:tip-b) to open level 2.",
            ),
        )
        .with_more(LocalizedString::literal(
            "Open the Accordion to read this long-form body without leaving the tooltip.",
        ))
        .with_shortcut_label("F1"),
        TooltipContent::new(
            KEY_B,
            LocalizedString::literal(
                "Level 2 of the cascade. Hover the [final link](:tip-c) for one more.",
            ),
        )
        .with_more(LocalizedString::literal(
            "Each nested tooltip parents its overlay to the previous one (OverlayLayer::InTree).",
        )),
        TooltipContent::new(
            KEY_C,
            LocalizedString::literal(
                "Level 3 — end of the cascade. Press Esc or click outside to dismiss.",
            ),
        )
        .with_shortcut_label("Esc"),
        TooltipContent::new(
            KEY_FOOD,
            LocalizedString::literal(
                "**Food** modifies your population's growth rate. Linked to [trade](:stat-trade).",
            ),
        )
        .with_shortcut_label("F"),
        TooltipContent::new(
            KEY_TRADE,
            LocalizedString::literal(
                "**Trade** routes affect coin income. Linked to [happiness](:stat-happiness).",
            ),
        )
        .with_shortcut_label("T"),
        TooltipContent::new(
            KEY_HAPPINESS,
            LocalizedString::literal(
                "**Happiness** caps unrest. End of the inside-composite cascade.",
            ),
        ),
    ]
}

// ── Columns ──────────────────────────────────────────────────────────────

fn plain_column() -> impl Widget {
    Panel::new().child(
        VStack::new()
            .spacing(8.0)
            .child(TextWidget::new_literal("Plain tooltips").style(TextStyleRole::BodyBold))
            .child(TextWidget::new_literal("(single-line, ephemeral)"))
            .child(Button::new_literal("Save").tooltip_literal("Save the current document"))
            .child(Button::new_literal("Open").tooltip_literal("Open a file"))
            .child(Button::new_literal("Close").tooltip_literal("Close the tab"))
            .child(Spacer::new()),
    )
}

fn rich_column() -> impl Widget {
    Panel::new().child(
        VStack::new()
            .spacing(8.0)
            .child(TextWidget::new_literal("Rich tooltips").style(TextStyleRole::BodyBold))
            .child(TextWidget::new_literal("(:key cascade, dwell-to-sticky)"))
            .child(Spacer::new())
            .child(Button::new_literal("Hover for level 1").rich_tooltip(KEY_A))
            .child(Button::new_literal("Hover for level 2").rich_tooltip(KEY_B))
            .child(Button::new_literal("Hover for level 3").rich_tooltip(KEY_C))
            .child(Button::new_literal("Plain among rich").tooltip_literal(
                "Plain tooltip living in the rich column — diagnostic.",
            ))
            .child(Spacer::new())
            .child(TextWidget::new_literal(
                "Tip: dwell ~2 s to pin, then click links to chain.",
            )),
    )
}

/// Build the body of a "province info" composite tooltip — a stat
/// grid with each row carrying its own rich tooltip cascading into
/// the registry.
fn province_composite_body() -> impl Widget {
    VStack::new()
        .spacing(8.0)
        .child(TextWidget::new_literal("Iberia").style(TextStyleRole::BodyBold))
        .child(TextWidget::new_literal("Province overview"))
        .child(ProgressBar::new(0.65))
        .child(
            HStack::new()
                .spacing(12.0)
                .child(Button::new_literal("Food: 42").rich_tooltip(KEY_FOOD))
                .child(Button::new_literal("Trade: 18").rich_tooltip(KEY_TRADE))
                .child(Button::new_literal("Happiness: 71%").rich_tooltip(KEY_HAPPINESS)),
        )
}

/// Build the body of a tabbed composite tooltip — proves `TabWidget`
/// works inside a tooltip surface.
fn tabbed_composite_body() -> impl Widget {
    let selected: Signal<Option<fern_ui::widgets::tab_widget::TabId>> = Signal::new(None);
    let body = TabWidget::new(selected)
        .static_tab(
            TabInfo::new().title(LocalizedString::literal("Stats")),
            VStack::new()
                .spacing(4.0)
                .child(TextWidget::new_literal("Population: 12,400"))
                .child(TextWidget::new_literal("Garrison: 320")),
        )
        .static_tab(
            TabInfo::new().title(LocalizedString::literal("History")),
            TextWidget::new_literal("Founded 1247 • 3 sieges • 1 plague"),
        );
    VStack::new()
        .spacing(8.0)
        .child(TextWidget::new_literal("Tabbed details").style(TextStyleRole::BodyBold))
        .child(body)
}

/// Build the body of an interactive composite tooltip — includes a
/// `Button` to prove keyboard-focus reachability post-promotion.
fn interactive_composite_body() -> impl Widget {
    VStack::new()
        .spacing(8.0)
        .child(TextWidget::new_literal("Treasury report").style(TextStyleRole::BodyBold))
        .child(TextWidget::new_literal("This quarter: +423 coins"))
        .child(ProgressBar::new(0.42))
        .child(
            Button::new_literal("Open ledger")
                .on_activate_fn(|_ctx| {
                    println!("Open ledger pressed from inside a composite tooltip!");
                }),
        )
}

fn composite_column() -> impl Widget {
    Panel::new().child(
        VStack::new()
            .spacing(8.0)
            .child(TextWidget::new_literal("Composite tooltips").style(TextStyleRole::BodyBold))
            .child(TextWidget::new_literal("(arbitrary widget tree, CK3-style)"))
            .child(Spacer::new())
            .child(Button::new_literal("Province info").composite_tooltip(province_composite_body()))
            .child(Button::new_literal("Tabbed details").composite_tooltip(tabbed_composite_body()))
            .child(
                Button::new_literal("With internal Button")
                    .composite_tooltip(interactive_composite_body()),
            )
            .child(Spacer::new())
            .child(TextWidget::new_literal(
                "Tip: dwell ~2 s, then Tab into the surface, then activate the inner Button.",
            )),
    )
}

fn root() -> impl Widget {
    VStack::new()
        .spacing(12.0)
        .child(
            Padding::uniform(12.0).child(
                TextWidget::new_literal("FernUI — Tooltips Showcase")
                    .style(TextStyleRole::BodyBold),
            ),
        )
        .child(
            Expand::new().child(
                HStack::new()
                    .spacing(12.0)
                    .child(Expand::new().flex(1.0).child(plain_column()))
                    .child(Expand::new().flex(1.0).child(rich_column()))
                    .child(Expand::new().flex(1.0).child(composite_column())),
            ),
        )
}

fn main() {
    FernAppBuilder::new()
        .theme(Theme::dark_default())
        .register_tooltips(build_tooltip_registry())
        .initial_window(
            WindowConfig::new()
                .title("FernUI — Tooltips Showcase")
                .size(1200, 720)
                .root(|tree, _state| tree.add(root())),
        )
        .run();
}
