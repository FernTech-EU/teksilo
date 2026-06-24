// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Shared state and helpers for the widget catalog.
//!
//! `Signals` is the single bundle of reactive values that every tab
//! reads from and writes to. Both the classic and `bati!` builds of a
//! tab share this struct, so flipping the mode toggle preserves all
//! interactive state (slider positions, checkbox states, etc.).

use bastyde::prelude::*;
use bastyde::widgets::tooltip::TooltipContent;
use bastyde::widgets::{CheckState, Panel, TabId};

// ── Cascading-tooltip registry keys ───────────────────────────────────
// Three-deep cascade: each tip's body links into the next via the
// `[label](:key)` markup understood by RichTooltipWidget. Used by the
// "Cascading tooltip showcase" section in the Layout tab; mirrors the
// `tooltips_showcase` example.

pub const KEY_TIP_A: &str = "tip-a";
pub const KEY_TIP_B: &str = "tip-b";
pub const KEY_TIP_C: &str = "tip-c";

pub const KEY_STAT_FOOD: &str = "stat-food";
pub const KEY_STAT_TRADE: &str = "stat-trade";
pub const KEY_STAT_HAPPINESS: &str = "stat-happiness";

/// Build the catalog's tooltip registry — every key referenced by a
/// `.rich_tooltip(KEY_…)` call site or a `[label](:key)` cascade link
/// inside a tooltip body must appear here. Single source of truth so
/// the layout tab and any future cascading demos stay in sync.
pub fn build_tooltip_registry() -> Vec<TooltipContent> {
    vec![
        TooltipContent::new(KEY_TIP_A, tr!(tip_a_body()))
            .with_more(tr!(tip_a_more()))
            .with_shortcut_label("F1"),
        TooltipContent::new(KEY_TIP_B, tr!(tip_b_body())).with_more(tr!(tip_b_more())),
        TooltipContent::new(KEY_TIP_C, tr!(tip_c_body())).with_shortcut_label("Esc"),
        TooltipContent::new(KEY_STAT_FOOD, tr!(tip_stat_food_body())).with_shortcut_label("F"),
        TooltipContent::new(KEY_STAT_TRADE, tr!(tip_stat_trade_body())).with_shortcut_label("T"),
        TooltipContent::new(KEY_STAT_HAPPINESS, tr!(tip_stat_happiness_body())),
    ]
}

/// Bundle of reactive signals shared across every tab. Tabs that
/// don't need a particular signal simply ignore it.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Signals {
    // ── Inputs tab ────────────────────────────────────────────────
    pub checkbox_checked: Signal<bool>,
    pub tristate: Signal<CheckState>,
    pub radio_selected: Signal<usize>,
    pub toggle_on: Signal<bool>,
    pub toggle_label_on: Signal<bool>,
    pub slider_value: Signal<f32>,
    pub slider_v_value: Signal<f32>,
    pub slider_stepped: Signal<f32>,
    pub segment_selected: Signal<usize>,
    pub combo_selected: Signal<Option<String>>,
    pub spin_value: Signal<f64>,

    // ── Containers tab ────────────────────────────────────────────
    pub accordion_expanded: Signal<bool>,
    pub accordion2_expanded: Signal<bool>,
    pub group_box_notifications_on: Signal<bool>,
    pub tool_box_selected: Signal<usize>,

    // ── Buttons / chrome / etc. ───────────────────────────────────
    pub cb_disabled_state: Signal<bool>,
    pub cb_sounds: Signal<bool>,
    pub toggle_disabled_state: Signal<bool>,
    pub slider_disabled_state: Signal<f32>,

    // ── Tabs / overlays ───────────────────────────────────────────
    pub inner_tabs_selected: Signal<Option<TabId>>,
    pub styled_tabs_selected: Signal<Option<TabId>>,
    pub visibility_signal: Signal<bool>,
    pub pinned_signal: Signal<bool>,

    // ── Text inputs ───────────────────────────────────────────────
    pub search_text: Signal<String>,
    pub username_text: Signal<String>,
    pub readonly_text: Signal<String>,
}

impl Signals {
    pub fn new(ctx: &mut BuildContext) -> Self {
        Self {
            checkbox_checked: ctx.signal(false),
            tristate: ctx.signal(CheckState::Unchecked),
            radio_selected: ctx.signal(0_usize),
            toggle_on: ctx.signal(false),
            toggle_label_on: ctx.signal(true),
            slider_value: ctx.signal(50.0_f32),
            slider_v_value: ctx.signal(0.3_f32),
            slider_stepped: ctx.signal(25.0_f32),
            segment_selected: ctx.signal(0_usize),
            combo_selected: ctx.signal(None::<String>),
            spin_value: ctx.signal(0.0_f64),

            accordion_expanded: ctx.signal(false),
            accordion2_expanded: ctx.signal(true),
            group_box_notifications_on: ctx.signal(true),
            tool_box_selected: ctx.signal(0_usize),

            cb_disabled_state: ctx.signal(true),
            cb_sounds: ctx.signal(true),
            toggle_disabled_state: ctx.signal(false),
            slider_disabled_state: ctx.signal(30.0_f32),

            inner_tabs_selected: ctx.signal(None),
            styled_tabs_selected: ctx.signal(None),
            visibility_signal: ctx.signal(false),
            pinned_signal: ctx.signal(false),

            search_text: ctx.signal(String::new()),
            username_text: ctx.signal("cyril".to_string()),
            readonly_text: ctx.signal("Read-only value".to_string()),
        }
    }
}

/// Common header for every tab body: bold title + secondary subtitle
/// pointing to the dedicated example crate(s) for deep-dive material.
///
/// `title` and `refs` are translated `LocalizedString`s produced by
/// `tr!`. The subtitle uses `TextStyleRole::Small`; both lines
/// get standard role-driven coloring so theme switches retint live.
pub fn tab_header(
    ctx: &mut BuildContext,
    title: impl Into<LocalizedString>,
    refs: impl Into<LocalizedString>,
) -> WidgetId {
    use bastyde::widgets::{TextWidget, VStack};
    ctx.add(
        VStack::new()
            .spacing(4.0)
            .child(
                TextWidget::new(title)
                    .style(TextStyleRole::BodyBold)
                    .color(TextRole::Primary),
            )
            .child(
                TextWidget::new(refs)
                    .style(TextStyleRole::Small)
                    .color(TextRole::Secondary),
            ),
    )
}

/// Reusable colored cell — used by layout primitives demos to stand in
/// for "real" content. Uses `TextRole::Primary` for the label, which
/// renders correctly on any non-saturated surface (Raised / Sunken /
/// AccentSubtle / AltRow). Pass a strong fill like `SurfaceRole::Accent`
/// only when you want emphasis; the text will still resolve via the
/// theme's contrast tokens.
pub fn color_cell(role: impl Into<ColorProp>, label: &'static str) -> impl Widget + 'static {
    use bastyde::widgets::TextWidget;
    Panel::new()
        .background(role)
        .corner_radius(4.0)
        .padding(8.0)
        .child(
            TextWidget::new(lit!(label))
                .style(TextStyleRole::SmallBold)
                .color(TextRole::Primary),
        )
}

/// Maximum width for free-text entry widgets in the catalog — ones with
/// no fixed-length content (TextInput, SearchField, PasswordField,
/// FilePickerField). These are flex-fill by default, so inside a wide
/// tab they stretch edge-to-edge and stop the ScrollArea content from
/// shrinking sensibly. Wrap them in `MaxSize::width(FIELD_MAX_WIDTH)` —
/// the field never grows past the cap but still shrinks into a narrow
/// viewport — mirroring the built-in width cap on `SpinBox` (see the
/// `spin_box` example).
///
/// Fixed-format fields (hex colors, dates, times) don't use this cap:
/// `MaxSize` makes the flex field *fill* the cap, so a 7-char hex field
/// at 360 dp reads as far too wide. Those are capped near their own
/// content width at each call site instead.
pub const FIELD_MAX_WIDTH: f32 = 360.0;

/// Per-widget showcase section: bold mono title + the widget itself.
///
/// `title` accepts either a plain `&str` (a **widget name** that stays
/// untranslated since it identifies a Rust type, e.g. `"HStack"`,
/// `"Button"`) or a `tr!(...)` `LocalizedString` for headings whose
/// descriptive part should localize (e.g. `"ScrollBar (standalone)"`).
/// `body` is the demo widget itself.
pub fn section(
    ctx: &mut BuildContext,
    title: impl Into<LocalizedString>,
    body: impl Widget + 'static,
) -> WidgetId {
    use bastyde::widgets::{TextWidget, VStack};
    ctx.add(
        VStack::new()
            .spacing(6.0)
            .child(
                TextWidget::new(title)
                    .style(TextStyleRole::SmallBold)
                    .color(TextRole::Accent),
            )
            .child(body),
    )
}
