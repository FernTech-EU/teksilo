//! Milestone 3: Widget Catalog — side-by-side builder vs `fern!`
//!
//! A 1600×900 window split by a vertical [`SplitView`] into two panes.
//! Each pane renders the *exact same* widget tree — a toolbar, a
//! scrollable 13-section catalog of every Milestone 3 widget, and a
//! status bar. The left pane is assembled with builder-style
//! `.child()/.add_child()` chains; the right pane is assembled with the
//! `fern!` DSL. Both panes are driven by a single shared `Signals`
//! bundle, so toggling a checkbox, dragging a slider, or changing a
//! tab on one side mirrors on the other — visual proof that the two
//! authorings lower to identical trees.
//!
//! Run with: `cargo run -p widget-catalog`
//!
//! Demonstrates:
//! - Divider (horizontal, vertical, custom thickness/color)
//! - IconWidget (checkmark, chevrons, custom path)
//! - Grid (fixed, fractional, auto tracks with gaps)
//! - Wrap (flow layout with line breaking)
//! - AspectRatio (16:9, square)
//! - ProgressBar (determinate, indeterminate, vertical)
//! - Badge (custom colors)
//! - Checkbox (two-state, tristate, disabled, with tooltip)
//! - RadioButton (mutual exclusion group)
//! - Toggle (with/without label, disabled)
//! - Slider (horizontal, vertical, stepped)
//! - SegmentedControl (position-based click selection)
//! - Card (header/content/footer, shadow)
//! - Toolbar (compact action bar)
//! - StatusBar (bottom info bar)
//! - Accordion (animated expand/collapse)
//! - Link (clickable text with underline)
//! - ScrollArea (wrapping all content)
//! - SplitView (twin-pane demo)
//! - Theme switching (light/dark)

use std::cell::Cell;
use std::rc::Rc;

use fern_ui::prelude::*;
use fern_ui::tokens::{FontWeight, Orientation, TextStyle};
use fern_ui::widgets::{
    Accordion, Badge, BuiltInButton, BuiltInButtonSize, Button, ButtonVariant, Card, CheckState,
    Checkbox, ComboBox, Divider, Expand, FixedSize, Grid, GroupBox, GroupHeader, HStack,
    IconLocation, IconWidget, ImageFit, ImageWidget, Link, MaxSize, MenuItem, MenuList, Padding, Panel, ProgressBar,
    RadioButton, ScrollArea, SegmentedControl, Slider, Spacer, SplitButton, SplitView, StatusBar, TabItem,
    TabWidget, TextInput, TextWidget, Toggle, Toolbar, TrackSize, VStack, Wrap,
};
use fern_ui::widgets::tooltip::TooltipContent;

// ---------------------------------------------------------------------------
// Application commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Cmd {
    ToggleDarkMode,
    LinkClicked,
    Cut,
    Copy,
    Paste,
    Save,
    SaveAs,
    Cancel,
    LearnMore,
    Run,
    RunTests,
    RunCoverage,
    Debug,
}

impl AppCommand for Cmd {}

// ---------------------------------------------------------------------------
// Shared signal bundle
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Signals {
    checkbox_checked: Signal<bool>,
    tristate: Signal<CheckState>,
    radio_selected: Signal<usize>,
    toggle_on: Signal<bool>,
    toggle_label_on: Signal<bool>,
    slider_value: Signal<f32>,
    slider_v_value: Signal<f32>,
    slider_stepped: Signal<f32>,
    segment_selected: Signal<usize>,
    accordion_expanded: Signal<bool>,
    accordion2_expanded: Signal<bool>,
    group_box_notifications_on: Signal<bool>,
    cb_disabled_state: Signal<bool>,
    cb_sounds: Signal<bool>,
    toggle_disabled_state: Signal<bool>,
    slider_disabled_state: Signal<f32>,
    tabs_selected: Signal<usize>,
    combo_selected: Signal<Option<String>>,
    visibility_signal: Signal<bool>,
    search_text: Signal<String>,
    username_text: Signal<String>,
    readonly_text: Signal<String>,
}

impl Signals {
    fn new(ctx: &mut BuildContext) -> Self {
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
            accordion_expanded: ctx.signal(false),
            accordion2_expanded: ctx.signal(true),
            group_box_notifications_on: ctx.signal(true),
            cb_disabled_state: ctx.signal(true),
            cb_sounds: ctx.signal(true),
            toggle_disabled_state: ctx.signal(false),
            slider_disabled_state: ctx.signal(30.0_f32),
            tabs_selected: ctx.signal(0_usize),
            combo_selected: ctx.signal(None::<String>),
            visibility_signal: ctx.signal(false),
            search_text: ctx.signal(String::new()),
            username_text: ctx.signal("cyril".to_string()),
            readonly_text: ctx.signal("Read-only value".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

fn surface_swatch(theme: &Theme, bg: Color, name: &str, text_role: &str, text_color: Color) -> VStack {
    let t = &theme.typography;
    let c = &theme.colors;
    VStack::new()
        .spacing(4.0)
        .child(
            Panel::new()
                .background(bg)
                .border_color(c.border_strong)
                .border_width(1.0)
                .corner_radius(4.0)
                .padding(10.0)
                .child(MaxSize::new(f32::INFINITY, 44.0).child(
                    TextWidget::new_literal(name)
                        .style(t.small.clone())
                        .color(text_color),
                )),
        )
        .child(
            TextWidget::new_literal(text_role)
                .style(t.tiny.clone())
                .color(c.text_secondary),
        )
}

fn text_sample(theme: &Theme, name: &str, color: Color, description: &str) -> HStack {
    let t = &theme.typography;
    let c = &theme.colors;
    HStack::new()
        .spacing(12.0)
        .child(
            TextWidget::new_literal("The quick brown fox jumps over the lazy dog")
                .style(t.body.clone())
                .color(color),
        )
        .child(Spacer::new())
        .child(
            TextWidget::new_literal(name)
                .style(t.tiny.clone())
                .color(c.text_secondary),
        )
        .child(
            TextWidget::new_literal(description)
                .style(t.tiny.clone())
                .color(c.text_secondary),
        )
}

fn editor_line(theme: &Theme, line_no: &str, code: &str) -> HStack {
    let t = &theme.typography;
    let c = &theme.colors;
    HStack::new()
        .spacing(12.0)
        .child(
            FixedSize::new()
                .bind_width(24.0_f32)
                .child(
                    TextWidget::new_literal(line_no)
                        .style(t.mono.clone())
                        .color(c.editor_gutter_fg),
                ),
        )
        .child(
            TextWidget::new_literal(code)
                .style(t.mono.clone())
                .color(c.editor_fg),
        )
}

fn editor_swatch(theme: &Theme, bg: Color, name: &str, sample_color: Color) -> VStack {
    let t = &theme.typography;
    let c = &theme.colors;
    VStack::new()
        .spacing(4.0)
        .child(
            Panel::new()
                .background(bg)
                .border_color(c.border_strong)
                .border_width(1.0)
                .corner_radius(4.0)
                .padding(10.0)
                .child(MaxSize::new(f32::INFINITY, 44.0).child(
                    TextWidget::new_literal("Aa Bb 123")
                        .style(t.mono.clone())
                        .color(sample_color),
                )),
        )
        .child(
            TextWidget::new_literal(name)
                .style(t.tiny.clone())
                .color(c.text_secondary),
        )
}

fn build_color_cell(color: Color, label: &str) -> Panel {
    Panel::new()
        .background(color)
        .corner_radius(4.0)
        .padding(8.0)
        .child(
            TextWidget::new_literal(label)
                .style(TextStyle {
                    family: "sans-serif".into(),
                    size: 12.0,
                    weight: FontWeight::BOLD,
                    line_height: 1.4,
                    letter_spacing: 0.0,
                })
                .color(Color::WHITE),
        )
}

// ---------------------------------------------------------------------------
// Root composite
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct WidgetCatalog {
    root_child_id: Option<WidgetId>,
}

impl WidgetCatalog {
    fn new() -> Self {
        Self {
            root_child_id: None,
        }
    }
}

impl Widget for WidgetCatalog {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let t = &theme.typography;
        let c = &theme.colors;
        let sigs = Signals::new(ctx);

        // ============================ LEFT PANE (builder) ============================
        let palette_section = self.palette_builder(ctx, &theme, &sigs);
        let primitives_section = self.primitives_builder(ctx, &theme, &sigs);
        let layout_section = self.layout_builder(ctx, &theme, &sigs);
        let controls_section = self.controls_builder(ctx, &theme, &sigs);
        let display_section = self.display_builder(ctx, &theme, &sigs);
        let text_overflow_section = self.text_overflow_builder(ctx, &theme, &sigs);
        let containers_section = self.containers_builder(ctx, &theme, &sigs);
        let nav_section = self.nav_builder(ctx, &theme, &sigs);
        let rich_tooltips_section = self.rich_tooltips_builder(ctx, &theme, &sigs);
        let menus_section = self.menus_builder(ctx, &theme, &sigs);
        let image_section = self.image_builder(ctx, &theme, &sigs);
        let builtin_section = self.builtin_builder(ctx, &theme, &sigs);
        let text_input_section = self.text_input_builder(ctx, &theme, &sigs);

        let left_toolbar = ctx.add(
            Toolbar::new().child(
                HStack::new()
                    .child(
                        TextWidget::new_literal("Widget Catalog -- builder")
                            .style(t.body_bold.clone())
                            .color(c.text_primary),
                    )
                    .child(Spacer::new())
                    .child(
                        Button::new_literal("Toggle Dark Mode")
                            .style(ButtonVariant::Regular)
                            .on_activate(Cmd::ToggleDarkMode),
                    ),
            ),
        );

        let left_content_col = ctx.add(
            VStack::new()
                .spacing(24.0)
                .add_child(palette_section)
                .child(Divider::new())
                .add_child(primitives_section)
                .child(Divider::new())
                .add_child(layout_section)
                .child(Divider::new())
                .add_child(controls_section)
                .child(Divider::new())
                .add_child(display_section)
                .child(Divider::new())
                .add_child(text_overflow_section)
                .child(Divider::new())
                .add_child(containers_section)
                .child(Divider::new())
                .add_child(nav_section)
                .child(Divider::new())
                .add_child(rich_tooltips_section)
                .child(Divider::new())
                .add_child(menus_section)
                .child(Divider::new())
                .add_child(image_section)
                .child(Divider::new())
                .add_child(builtin_section)
                .child(Divider::new())
                .add_child(text_input_section),
        );
        let left_padded = ctx.add(Padding::uniform(24.0).child_id(left_content_col));
        let left_scroll = ctx.add(ScrollArea::from_id(left_padded));
        let left_expand = ctx.add(Expand::new().fills_stack().child_id(left_scroll));
        let left_root = ctx.add(
            VStack::new()
                .add_child(left_toolbar)
                .add_child(left_expand)
                .child(
                    StatusBar::new().child(
                        TextWidget::new_literal("Builder -- .child() / .add_child() chains")
                            .style(t.tiny.clone())
                            .color(c.text_secondary),
                    ),
                ),
        );

        // ============================ RIGHT PANE (fern!) =============================
        let r_palette = self.palette_fern(ctx, &theme, &sigs);
        let r_primitives = self.primitives_fern(ctx, &theme, &sigs);
        let r_layout = self.layout_fern(ctx, &theme, &sigs);
        let r_controls = self.controls_fern(ctx, &theme, &sigs);
        let r_display = self.display_fern(ctx, &theme, &sigs);
        let r_text_overflow = self.text_overflow_fern(ctx, &theme, &sigs);
        let r_containers = self.containers_fern(ctx, &theme, &sigs);
        let r_nav = self.nav_fern(ctx, &theme, &sigs);
        let r_rich_tooltips = self.rich_tooltips_fern(ctx, &theme, &sigs);
        let r_menus = self.menus_fern(ctx, &theme, &sigs);
        let r_image = self.image_fern(ctx, &theme, &sigs);
        let r_builtin = self.builtin_fern(ctx, &theme, &sigs);
        let r_text_input = self.text_input_fern(ctx, &theme, &sigs);

        let right_content_col = fern!(ctx =>
            VStack {
                spacing: 24.0
                add_child: r_palette
                Divider { }
                add_child: r_primitives
                Divider { }
                add_child: r_layout
                Divider { }
                add_child: r_controls
                Divider { }
                add_child: r_display
                Divider { }
                add_child: r_text_overflow
                Divider { }
                add_child: r_containers
                Divider { }
                add_child: r_nav
                Divider { }
                add_child: r_rich_tooltips
                Divider { }
                add_child: r_menus
                Divider { }
                add_child: r_image
                Divider { }
                add_child: r_builtin
                Divider { }
                add_child: r_text_input
            }
        );
        let right_padded = ctx.add(Padding::uniform(24.0).child_id(right_content_col));
        let right_scroll = ctx.add(ScrollArea::from_id(right_padded));
        let right_root = fern!(ctx =>
            VStack {
                Toolbar {
                    HStack {
                        TextWidget::new_literal("Widget Catalog -- fern!") {
                            style: t.body_bold.clone()
                            color: c.text_primary
                        }
                        Spacer { }
                        Button::new_literal("Toggle Dark Mode") {
                            style: ButtonVariant::Regular
                            on_activate: Cmd::ToggleDarkMode
                        }
                    }
                }
                Expand {
                    fills_stack
                    child_id: right_scroll
                }
                StatusBar {
                    TextWidget::new_literal("fern! -- DSL body items") {
                        style: t.tiny.clone()
                        color: c.text_secondary
                    }
                }
            }
        );

        // ============================ SPLIT ROOT =====================================
        let split = ctx.signal(0.5_f32);
        let root = ctx.add(
            SplitView::new(split)
                .orientation(Orientation::Horizontal)
                .min_first_size(400.0)
                .min_second_size(400.0)
                .first_id(left_root)
                .second_id(right_root),
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
    }
}

// ---------------------------------------------------------------------------
// Builder-style section helpers
// ---------------------------------------------------------------------------

impl WidgetCatalog {
    fn palette_builder(&self, ctx: &mut BuildContext, theme: &Theme, _sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        let surfaces_grid = ctx.add(
            Grid::new()
                .columns(vec![
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                ])
                .column_gap(12.0)
                .row_gap(12.0)
                .rows(vec![TrackSize::Auto, TrackSize::Auto])
                .child(surface_swatch(theme, c.surface_main, "surface_main", "text_primary", c.text_primary))
                .child(surface_swatch(theme, c.surface_content, "surface_content", "text_primary", c.text_primary))
                .child(surface_swatch(theme, c.surface_raised, "surface_raised", "text_primary", c.text_primary))
                .child(surface_swatch(theme, c.surface_sunken, "surface_sunken", "text_secondary", c.text_secondary))
                .child(surface_swatch(theme, c.surface_hover, "surface_hover", "text_primary", c.text_primary))
                .child(surface_swatch(theme, c.surface_pressed, "surface_pressed", "text_primary", c.text_primary))
                .child(surface_swatch(theme, c.surface_selected, "surface_selected", "selection_text_active", c.selection_text_active))
                .child(surface_swatch(theme, c.surface_selected_inactive, "surface_selected_inactive", "selection_text_inactive", c.selection_text_inactive)),
        );

        let text_samples = ctx.add(
            Panel::new()
                .background(c.surface_main)
                .border_color(c.border)
                .border_width(1.0)
                .corner_radius(8.0)
                .padding(16.0)
                .child(
                    VStack::new()
                        .spacing(6.0)
                        .child(text_sample(theme, "text_primary", c.text_primary, "body, main labels"))
                        .child(text_sample(theme, "text_secondary", c.text_secondary, "hints, captions, placeholders"))
                        .child(text_sample(theme, "text_disabled", c.text_disabled, "disabled labels"))
                        .child(text_sample(theme, "text_link", c.text_link, "hyperlinks"))
                        .child(text_sample(theme, "text_error", c.text_error, "validation errors"))
                        .child(text_sample(theme, "text_warning", c.text_warning, "validation warnings"))
                        .child(text_sample(theme, "text_success", c.text_success, "success messages")),
                ),
        );

        let text_on_accent_row = ctx.add(
            Panel::new()
                .background(c.accent)
                .corner_radius(4.0)
                .padding(12.0)
                .child(
                    HStack::new()
                        .spacing(12.0)
                        .child(
                            TextWidget::new_literal("Default button label")
                                .style(t.body.clone())
                                .color(c.text_on_accent),
                        )
                        .child(Spacer::new())
                        .child(
                            TextWidget::new_literal("text_on_accent on accent")
                                .style(t.tiny.clone())
                                .color(c.text_on_accent),
                        ),
                ),
        );

        let mono = t.mono.clone();
        let current_line_content = ctx.add(
            HStack::new()
                .spacing(12.0)
                .child(
                    FixedSize::new()
                        .bind_width(24.0_f32)
                        .child(
                            TextWidget::new_literal("2")
                                .style(mono.clone())
                                .color(c.editor_gutter_fg),
                        ),
                )
                .child(
                    TextWidget::new_literal("    let ")
                        .style(mono.clone())
                        .color(c.editor_fg),
                )
                .child(
                    Panel::new()
                        .background(c.editor_selection_bg)
                        .corner_radius(2.0)
                        .padding(0.0)
                        .border_width(0.0)
                        .child(
                            Padding::symmetric(1.0, 2.0).child(
                                TextWidget::new_literal("x")
                                    .style(mono.clone())
                                    .color(c.editor_fg),
                            ),
                        ),
                )
                .child(
                    TextWidget::new_literal(" = 42;")
                        .style(mono.clone())
                        .color(c.editor_fg),
                )
                .child(
                    FixedSize::new()
                        .bind_width(1.5_f32)
                        .bind_height(16.0_f32)
                        .child(
                            Panel::new()
                                .background(c.editor_caret)
                                .corner_radius(0.0)
                                .border_width(0.0)
                                .padding(0.0)
                                .child(Spacer::new()),
                        ),
                ),
        );
        let current_line_bg = ctx.add(
            Panel::new()
                .background(c.editor_current_line_bg)
                .corner_radius(0.0)
                .border_width(0.0)
                .padding(4.0)
                .child_id(current_line_content),
        );

        let mock_editor = ctx.add(
            Panel::new()
                .background(c.editor_bg)
                .border_color(c.border_strong)
                .border_width(1.0)
                .corner_radius(6.0)
                .padding(8.0)
                .child(
                    VStack::new()
                        .spacing(2.0)
                        .child(editor_line(theme, "1", "fn main() {"))
                        .add_child(current_line_bg)
                        .child(editor_line(theme, "3", "    println!(\"{}\", x);"))
                        .child(editor_line(theme, "4", "}")),
                ),
        );

        let editor_swatches = ctx.add(
            Grid::new()
                .columns(vec![
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                ])
                .column_gap(12.0)
                .row_gap(12.0)
                .rows(vec![TrackSize::Auto, TrackSize::Auto])
                .child(editor_swatch(theme, c.editor_bg, "editor_bg", c.editor_fg))
                .child(editor_swatch(theme, c.editor_fg, "editor_fg", c.editor_bg))
                .child(editor_swatch(theme, c.editor_caret, "editor_caret", c.editor_bg))
                .child(editor_swatch(theme, c.editor_current_line_bg, "editor_current_line_bg", c.editor_fg))
                .child(editor_swatch(theme, c.editor_gutter_fg, "editor_gutter_fg", c.editor_bg))
                .child(editor_swatch(theme, c.editor_selection_bg, "editor_selection_bg", c.editor_fg)),
        );

        ctx.add(
            VStack::new()
                .spacing(12.0)
                .child(
                    TextWidget::new_literal("Theme Palette")
                        .style(t.body_bold.clone())
                        .color(c.text_primary),
                )
                .child(
                    TextWidget::new_literal("Surfaces")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .add_child(surfaces_grid)
                .child(
                    TextWidget::new_literal("Text")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .add_child(text_samples)
                .add_child(text_on_accent_row)
                .child(
                    TextWidget::new_literal("Editor")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .add_child(mock_editor)
                .add_child(editor_swatches),
        )
    }

    fn primitives_builder(&self, ctx: &mut BuildContext, theme: &Theme, _sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        let div_row = ctx.add(
            HStack::new()
                .spacing(16.0)
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new_literal("H")
                                .style(t.tiny.clone())
                                .color(c.text_secondary),
                        )
                        .child(Divider::new()),
                )
                .child(
                    HStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new_literal("V")
                                .style(t.tiny.clone())
                                .color(c.text_secondary),
                        )
                        .child(Divider::vertical().thickness(2.0).color(c.accent)),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new_literal("Thick")
                                .style(t.tiny.clone())
                                .color(c.text_secondary),
                        )
                        .child(Divider::new().thickness(4.0).color(c.text_error)),
                ),
        );

        let icon_row = ctx.add(
            HStack::new()
                .spacing(12.0)
                .child(IconWidget::checkmark(24.0).color(c.accent))
                .child(IconWidget::chevron_down(24.0).color(c.accent_subtle_bg))
                .child(IconWidget::chevron_right(24.0).color(c.text_error)),
        );

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Primitives")
                        .style(t.body_bold.clone())
                        .color(c.text_primary),
                )
                .child(
                    TextWidget::new_literal("Divider (horizontal, vertical, thick, colored)")
                        .style(t.tiny.clone())
                        .color(c.text_secondary),
                )
                .add_child(div_row)
                .child(
                    TextWidget::new_literal("IconWidget (checkmark, chevrons)")
                        .style(t.tiny.clone())
                        .color(c.text_secondary),
                )
                .add_child(icon_row),
        )
    }

    fn layout_builder(&self, ctx: &mut BuildContext, theme: &Theme, _sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Layout Primitives")
                        .style(t.body_bold.clone())
                        .color(c.text_primary),
                )
                .child(
                    TextWidget::new_literal("Grid (Fixed 80px | 1fr | 2fr, with 8px gap)")
                        .style(t.tiny.clone())
                        .color(c.text_secondary),
                )
                .child(
                    Grid::new()
                        .columns(vec![
                            TrackSize::Fixed(80.0),
                            TrackSize::Fractional(1.0),
                            TrackSize::Fractional(2.0),
                        ])
                        .rows(vec![TrackSize::Auto, TrackSize::Auto])
                        .column_gap(8.0)
                        .row_gap(8.0)
                        .child(build_color_cell(c.accent, "A1"))
                        .child(build_color_cell(c.accent_subtle_bg, "B1"))
                        .child(build_color_cell(c.text_error, "C1"))
                        .child(build_color_cell(c.status_info_fg, "A2"))
                        .child(build_color_cell(c.text_success, "B2"))
                        .child(build_color_cell(c.text_warning, "C2")),
                )
                .child(
                    TextWidget::new_literal("Wrap (flow layout, 8px spacing)")
                        .style(t.tiny.clone())
                        .color(c.text_secondary),
                )
                .child(
                    Wrap::new()
                        .spacing(8.0)
                        .line_spacing(8.0)
                        .child(Badge::new_literal("Rust"))
                        .child(Badge::new_literal("GUI"))
                        .child(Badge::new_literal("Accessible"))
                        .child(Badge::new_literal("Reactive"))
                        .child(Badge::new_literal("Fast"))
                        .child(Badge::new_literal("Cross-platform"))
                        .child(Badge::new_literal("Retained"))
                        .child(Badge::new_literal("wgpu")),
                ),
        )
    }

    fn controls_builder(&self, ctx: &mut BuildContext, theme: &Theme, sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        let buttons_row = ctx.add(
            HStack::new()
                .spacing(8.0)
                .child(
                    Button::new_literal("Save")
                        .style(ButtonVariant::Default)
                        .on_activate(Cmd::Save),
                )
                .child(
                    Button::new_literal("Cancel")
                        .style(ButtonVariant::Regular)
                        .on_activate(Cmd::Cancel),
                )
                .child(
                    Button::new_literal("Learn more")
                        .style(ButtonVariant::Flat)
                        .on_activate(Cmd::LearnMore),
                ),
        );
        let buttons_row_disabled = ctx.add(
            HStack::new()
                .spacing(8.0)
                .child(
                    Button::new_literal("Save")
                        .style(ButtonVariant::Default)
                        .enabled(false),
                )
                .child(
                    Button::new_literal("Cancel")
                        .style(ButtonVariant::Regular)
                        .enabled(false),
                )
                .child(
                    Button::new_literal("Learn more")
                        .style(ButtonVariant::Flat)
                        .enabled(false),
                ),
        );

        let save_icon = fern_ui::res!("resources/icons/save.svg");
        let home_icon = fern_ui::res!("resources/icons/home.svg");
        let star_icon = fern_ui::res!("resources/icons/star.png");
        let clock_icon = fern_ui::res!("resources/icons/clock.webp");
        let icon_buttons_row = ctx.add(
            HStack::new()
                .spacing(8.0)
                .child(
                    Button::new_literal("Save")
                        .icon(IconWidget::from_svg_icon(save_icon), IconLocation::Leading)
                        .style(ButtonVariant::Default)
                        .on_activate(Cmd::Save),
                )
                .child(
                    Button::new_literal("Home")
                        .icon(IconWidget::from_svg_icon(home_icon), IconLocation::Leading)
                        .style(ButtonVariant::Regular)
                        .on_activate(Cmd::Cancel),
                )
                .child(
                    Button::new_literal("Star")
                        .icon(IconWidget::from_raster(star_icon, 24.0), IconLocation::Leading)
                        .style(ButtonVariant::Regular)
                        .on_activate(Cmd::Save),
                )
                .child(
                    Button::new_literal("Clock")
                        .icon(IconWidget::from_raster(clock_icon, 24.0), IconLocation::Leading)
                        .style(ButtonVariant::Regular)
                        .on_activate(Cmd::Cancel),
                )
                .child(
                    Button::new_literal("Save")
                        .icon(IconWidget::from_svg_icon(save_icon), IconLocation::IconOnly)
                        .style(ButtonVariant::Flat)
                        .on_activate(Cmd::Save),
                )
                .child(
                    Button::new_literal("")
                        .icon(IconWidget::chevron_down(16.0), IconLocation::IconOnly)
                        .style(ButtonVariant::Regular)
                        .on_activate(Cmd::Cancel),
                ),
        );

        let split_buttons_row = ctx.add(
            HStack::new()
                .spacing(8.0)
                .child(
                    SplitButton::new()
                        .item(
                            MenuItem::new_literal("Run")
                                .on_activate(Cmd::Run)
                                .tooltip_literal("Run the current configuration"),
                        )
                        .item(
                            MenuItem::new_literal("Run Tests")
                                .on_activate(Cmd::RunTests)
                                .tooltip_literal("Run the test suite"),
                        )
                        .item(
                            MenuItem::new_literal("Run with Coverage")
                                .on_activate(Cmd::RunCoverage)
                                .tooltip_literal("Run and collect code coverage"),
                        )
                        .separator()
                        .item(
                            MenuItem::new_literal("Debug")
                                .on_activate(Cmd::Debug)
                                .tooltip_literal("Launch the debugger"),
                        )
                        .tooltip_literal("Run the selected configuration")
                        .chevron_tooltip_literal("Other run configurations")
                        .style(ButtonVariant::Default),
                )
                .child(
                    SplitButton::new_static()
                        .item(
                            MenuItem::new_literal("Save")
                                .on_activate(Cmd::Save)
                                .tooltip_literal("Save the current file"),
                        )
                        .item(
                            MenuItem::new_literal("Save As…")
                                .on_activate(Cmd::Save)
                                .tooltip_literal("Save the current file under a new name"),
                        )
                        .tooltip_literal("Save (main action stays pinned)")
                        .style(ButtonVariant::Regular),
                )
                .child(
                    SplitButton::new()
                        .item(MenuItem::new_literal("Run").on_activate(Cmd::Run))
                        .item(MenuItem::new_literal("Debug").on_activate(Cmd::Debug))
                        .enabled(false),
                ),
        );

        let buttons_group = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(GroupHeader::new_literal("Standard buttons"))
                .add_child(buttons_row)
                .add_child(buttons_row_disabled)
                .child(GroupHeader::new_literal("Icon buttons"))
                .add_child(icon_buttons_row)
                .child(GroupHeader::new_literal("Split buttons"))
                .add_child(split_buttons_row),
        );

        let checkbox_group = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    Checkbox::new(sigs.checkbox_checked.clone())
                        .label_literal("Accept terms")
                        .tooltip_literal("Click to accept the terms and conditions"),
                )
                .child(
                    Checkbox::new(sigs.cb_sounds.clone())
                        .label_literal("Play notification sounds")
                        .caption_literal(
                            "Play a short chime when a new message arrives. \
                             Muted automatically while you're in a call.",
                        ),
                )
                .child(
                    Checkbox::new(sigs.cb_disabled_state.clone())
                        .label_literal("Always on (disabled)")
                        .enabled(false),
                )
                .child(
                    Checkbox::tristate(sigs.tristate.clone())
                        .label_literal("Select all (tristate)")
                        .caption_literal("Cycles unchecked → checked → indeterminate")
                        .tooltip_literal("Cycles: unchecked, checked, indeterminate"),
                ),
        );

        let radio_group = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    RadioButton::new(0, sigs.radio_selected.clone())
                        .label_literal("Standard")
                        .caption_literal("Recommended for most users — sensible defaults.")
                        .tooltip_literal("First option"),
                )
                .child(
                    RadioButton::new(1, sigs.radio_selected.clone())
                        .label_literal("Advanced")
                        .caption_literal(
                            "Expose every option, including experimental features \
                             that may change between releases.",
                        ),
                )
                .child(RadioButton::new(2, sigs.radio_selected.clone()).label_literal("Custom")),
        );

        let toggle_group = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(Toggle::new(sigs.toggle_on.clone()))
                .child(Toggle::new(sigs.toggle_label_on.clone()).label_literal("Notifications"))
                .child(Toggle::new(sigs.toggle_disabled_state.clone()).enabled(false)),
        );

        let slider_vert = ctx.add(
            Slider::new(sigs.slider_v_value.clone(), 0.0, 1.0).orientation(Orientation::Vertical),
        );
        let slider_section = ctx.add(
            HStack::new()
                .spacing(16.0)
                .child(
                    VStack::new()
                        .spacing(8.0)
                        .child(
                            TextWidget::new_literal("Horizontal")
                                .style(t.tiny.clone())
                                .color(c.text_secondary),
                        )
                        .child(Slider::new(sigs.slider_value.clone(), 0.0, 100.0))
                        .child(
                            TextWidget::new_literal("Stepped (25)")
                                .style(t.tiny.clone())
                                .color(c.text_secondary),
                        )
                        .child(Slider::new(sigs.slider_stepped.clone(), 0.0, 100.0).step(25.0))
                        .child(
                            TextWidget::new_literal("Disabled")
                                .style(t.tiny.clone())
                                .color(c.text_secondary),
                        )
                        .child(Slider::new(sigs.slider_disabled_state.clone(), 0.0, 100.0).enabled(false)),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new_literal("Vertical")
                                .style(t.tiny.clone())
                                .color(c.text_secondary),
                        )
                        .child(MaxSize::new(f32::MAX, 120.0).child_id(slider_vert)),
                ),
        );

        let form_row = ctx.add(
            Grid::new()
                .columns(vec![
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                    TrackSize::Fractional(1.0),
                ])
                .column_gap(16.0)
                .rows(vec![TrackSize::Auto])
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new_literal("Checkbox")
                                .style(t.small.clone())
                                .color(c.text_secondary),
                        )
                        .add_child(checkbox_group),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new_literal("RadioButton")
                                .style(t.small.clone())
                                .color(c.text_secondary),
                        )
                        .add_child(radio_group),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new_literal("Toggle")
                                .style(t.small.clone())
                                .color(c.text_secondary),
                        )
                        .add_child(toggle_group),
                ),
        );

        ctx.add(
            VStack::new()
                .spacing(12.0)
                .child(
                    TextWidget::new_literal("Form Controls")
                        .style(t.body_bold.clone())
                        .color(c.text_primary),
                )
                .child(
                    TextWidget::new_literal("Button")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .add_child(buttons_group)
                .child(Divider::new())
                .add_child(form_row)
                .child(Divider::new())
                .child(
                    TextWidget::new_literal("Slider")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .add_child(slider_section)
                .child(Divider::new())
                .child(
                    TextWidget::new_literal("SegmentedControl")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .child(SegmentedControl::new(
                    vec!["Day".into(), "Week".into(), "Month".into(), "Year".into()],
                    sigs.segment_selected.clone(),
                )),
        )
    }

    fn display_builder(&self, ctx: &mut BuildContext, theme: &Theme, _sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        let pb_vert = ctx.add(
            ProgressBar::new(0.7)
                .orientation(Orientation::Vertical)
                .thickness(8.0),
        );
        let progress_section = ctx.add(
            HStack::new()
                .spacing(16.0)
                .child(
                    VStack::new()
                        .spacing(8.0)
                        .child(
                            TextWidget::new_literal("Determinate (65%)")
                                .style(t.tiny.clone())
                                .color(c.text_secondary),
                        )
                        .child(ProgressBar::new(0.65))
                        .child(
                            TextWidget::new_literal("Indeterminate")
                                .style(t.tiny.clone())
                                .color(c.text_secondary),
                        )
                        .child(ProgressBar::indeterminate())
                        .child(
                            TextWidget::new_literal("Custom colors + thick")
                                .style(t.tiny.clone())
                                .color(c.text_secondary),
                        )
                        .child(
                            ProgressBar::new(0.4)
                                .thickness(8.0)
                                .fill_color(c.text_success)
                                .track_color(c.surface_sunken),
                        ),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new_literal("Vertical")
                                .style(t.tiny.clone())
                                .color(c.text_secondary),
                        )
                        .child(MaxSize::new(f32::MAX, 80.0).child_id(pb_vert)),
                ),
        );

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Display Widgets")
                        .style(t.body_bold.clone())
                        .color(c.text_primary),
                )
                .child(
                    TextWidget::new_literal("ProgressBar")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .add_child(progress_section)
                .child(Divider::new())
                .child(
                    TextWidget::new_literal("Badge")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .child(
                    HStack::new()
                        .spacing(8.0)
                        .child(Badge::new_literal("Default"))
                        .child(Badge::new_literal("3").color(c.text_error).text_color(Color::WHITE))
                        .child(Badge::new_literal("New").color(c.text_success).text_color(Color::WHITE))
                        .child(Badge::new_literal("Beta").color(c.text_warning)),
                ),
        )
    }

    fn text_overflow_builder(&self, ctx: &mut BuildContext, theme: &Theme, _sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        const LOREM: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing \
                             elit. Sed do eiusmod tempor incididunt ut labore \
                             et dolore magna aliqua. Ut enim ad minim veniam, \
                             quis nostrud exercitation ullamco laboris nisi ut \
                             aliquip ex ea commodo consequat.";
        const LONG_TITLE: &str =
            "A somewhat verbose section title that almost certainly will not fit";

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Text overflow")
                        .style(t.body_bold.clone())
                        .color(c.text_primary)
                        .single_line(),
                )
                .child(
                    TextWidget::new_literal("Wrap (default) — grows vertically")
                        .style(t.small.clone())
                        .color(c.text_secondary)
                        .single_line(),
                )
                .child(
                    FixedSize::new().bind_width(360.0_f32).child(
                        TextWidget::new_literal(LOREM)
                            .style(t.body.clone())
                            .color(c.text_primary),
                    ),
                )
                .child(
                    TextWidget::new_literal("Wrap capped at 2 lines")
                        .style(t.small.clone())
                        .color(c.text_secondary)
                        .single_line(),
                )
                .child(
                    FixedSize::new().bind_width(360.0_f32).child(
                        TextWidget::new_literal(LOREM)
                            .style(t.body.clone())
                            .color(c.text_primary)
                            .max_lines(2),
                    ),
                )
                .child(
                    TextWidget::new_literal("Ellipsis — trailing / middle / leading")
                        .style(t.small.clone())
                        .color(c.text_secondary)
                        .single_line(),
                )
                .child(
                    FixedSize::new().bind_width(280.0_f32).child(
                        TextWidget::new_literal(LONG_TITLE)
                            .style(t.body.clone())
                            .color(c.text_primary)
                            .overflow(TextOverflow::Ellipsis(EllipsisMode::Trailing)),
                    ),
                )
                .child(
                    FixedSize::new().bind_width(280.0_f32).child(
                        TextWidget::new_literal(LONG_TITLE)
                            .style(t.body.clone())
                            .color(c.text_primary)
                            .overflow(TextOverflow::Ellipsis(EllipsisMode::Middle)),
                    ),
                )
                .child(
                    FixedSize::new().bind_width(280.0_f32).child(
                        TextWidget::new_literal(LONG_TITLE)
                            .style(t.body.clone())
                            .color(c.text_primary)
                            .overflow(TextOverflow::Ellipsis(EllipsisMode::Leading)),
                    ),
                ),
        )
    }

    fn containers_builder(&self, ctx: &mut BuildContext, theme: &Theme, sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        let acc_content1 = ctx.add(
            TextWidget::new_literal("This content is revealed with an animated expand.")
                .style(t.body.clone())
                .color(c.text_primary),
        );
        let acc_content2 = ctx.add(
            TextWidget::new_literal("This section starts expanded and can be collapsed.")
                .style(t.body.clone())
                .color(c.text_primary),
        );

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Containers")
                        .style(t.body_bold.clone())
                        .color(c.text_primary),
                )
                .child(
                    TextWidget::new_literal("Card")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .child(
                    Card::new()
                        .header(
                            TextWidget::new_literal("Card Header")
                                .style(t.small.clone())
                                .color(c.text_secondary),
                        )
                        .content(
                            TextWidget::new_literal("Card content with shadow and themed background.")
                                .style(t.body.clone())
                                .color(c.text_primary),
                        )
                        .footer(
                            TextWidget::new_literal("Footer text")
                                .style(t.tiny.clone())
                                .color(c.text_secondary),
                        ),
                )
                .child(Divider::new())
                .child(
                    TextWidget::new_literal("Accordion")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .child(
                    Accordion::new_literal("Click to expand", sigs.accordion_expanded.clone())
                        .content_id(acc_content1),
                )
                .child(
                    Accordion::new_literal("Already expanded", sigs.accordion2_expanded.clone())
                        .content_id(acc_content2),
                )
                .child(Divider::new())
                .child(
                    TextWidget::new_literal("GroupBox")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .child(
                    GroupBox::new_literal("Appearance").child(
                        VStack::new()
                            .spacing(4.0)
                            .child(
                                TextWidget::new_literal("Indented content under a bold title.")
                                    .style(t.body.clone())
                                    .color(c.text_primary),
                            )
                            .child(
                                TextWidget::new_literal("No border, no frame — Int UI style.")
                                    .style(t.body.clone())
                                    .color(c.text_secondary),
                            ),
                    ),
                )
                .child(
                    GroupBox::new_literal("Notifications")
                        .checkable(sigs.group_box_notifications_on.clone())
                        .child(
                            VStack::new()
                                .spacing(4.0)
                                .child(
                                    TextWidget::new_literal(
                                        "Uncheck the title to disable this whole subtree.",
                                    )
                                    .style(t.body.clone())
                                    .color(c.text_primary),
                                )
                                .child(
                                    Button::new_literal("Inside — tap me")
                                        .style(ButtonVariant::Default),
                                ),
                        ),
                ),
        )
    }

    fn nav_builder(&self, ctx: &mut BuildContext, theme: &Theme, sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        let tabs = ctx.add(
            TabWidget::new(sigs.tabs_selected.clone())
                .tab_literal(
                    "Overview",
                    Panel::new().padding(16.0).child(
                        VStack::new()
                            .spacing(8.0)
                            .child(
                                TextWidget::new_literal("Overview")
                                    .style(t.body_bold.clone())
                                    .color(c.text_primary),
                            )
                            .child(
                                TextWidget::new_literal(
                                    "TabWidget is a retained container with dormant panes: \
                                     only the active tab is built, inactive panes keep \
                                     their state but don't receive layout or paint until \
                                     they're re-activated.",
                                )
                                .style(t.body.clone())
                                .color(c.text_primary),
                            )
                            .child(
                                HStack::new()
                                    .spacing(8.0)
                                    .child(Badge::new_literal("Dormant panes"))
                                    .child(Badge::new_literal("Arrow keys"))
                                    .child(Badge::new_literal("Trailing slot")),
                            ),
                    ),
                )
                .tab_literal(
                    "Usage",
                    Panel::new().padding(16.0).child(
                        VStack::new()
                            .spacing(8.0)
                            .child(
                                TextWidget::new_literal("Usage")
                                    .style(t.body_bold.clone())
                                    .color(c.text_primary),
                            )
                            .child(
                                TextWidget::new_literal(
                                    "Press Tab to move focus into the tab strip, then \
                                     Arrow Left / Arrow Right to switch between tabs. \
                                     Disabled tabs are skipped by keyboard navigation.",
                                )
                                .style(t.body.clone())
                                .color(c.text_primary),
                            ),
                    ),
                )
                .tab_literal(
                    "Structure",
                    Panel::new().padding(16.0).child(
                        VStack::new()
                            .spacing(8.0)
                            .child(
                                TextWidget::new_literal("Structure")
                                    .style(t.body_bold.clone())
                                    .color(c.text_primary),
                            )
                            .child(
                                TextWidget::new_literal(
                                    "Int UI tabs: flat headers, no rounded corners, no \
                                     borders. The selected tab is marked only by a 3 dp \
                                     accent underline at its bottom edge, which \
                                     overpaints the tab bar's own 1 dp separator.",
                                )
                                .style(t.body.clone())
                                .color(c.text_primary),
                            ),
                    ),
                )
                .tab_item(
                    TabItem::new_literal(
                        "Disabled",
                        Panel::new().padding(16.0).child(
                            TextWidget::new_literal(
                                "Disabled panes are still listed in the tab bar but \
                                 cannot be activated by click or keyboard.",
                            )
                            .style(t.body.clone())
                            .color(c.text_primary),
                        ),
                    )
                    .enabled(false),
                )
                .trailing_slot(
                    Button::new_literal("More")
                        .style(ButtonVariant::Flat)
                        .on_activate(Cmd::LinkClicked),
                ),
        );
        let tabs_block = ctx.add(
            FixedSize::new()
                .bind_height(240.0_f32)
                .child_id(tabs),
        );

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Navigation")
                        .style(t.body_bold.clone())
                        .color(c.text_primary),
                )
                .child(
                    TextWidget::new_literal("TabWidget")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .add_child(tabs_block)
                .child(
                    TextWidget::new_literal("Link")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .child(
                    HStack::new()
                        .spacing(16.0)
                        .child(
                            Link::new_literal("Click me")
                                .on_activate(Cmd::LinkClicked)
                                .tooltip_literal("Fires the LinkClicked command"),
                        )
                        .child(
                            Link::new_literal("FernUI Documentation")
                                .url("https://github.com/jacquetc/fern-ui"),
                        ),
                ),
        )
    }

    fn rich_tooltips_builder(&self, ctx: &mut BuildContext, theme: &Theme, _sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Rich Tooltips")
                        .style(t.body_bold.clone())
                        .color(c.text_primary),
                )
                .child(
                    TextWidget::new_literal(
                        "Hover the buttons below. Inline `[label](:key)` links \
                         open nested tooltips; `https://` links open in the browser.",
                    )
                    .style(t.small.clone())
                    .color(c.text_secondary),
                )
                .child(
                    HStack::new()
                        .spacing(12.0)
                        .child(
                            Button::new_literal("Save As…")
                                .rich_tooltip("save-as"),
                        )
                        .child(
                            Button::new_literal("Autosave info")
                                .rich_tooltip("autosave"),
                        )
                        .child(
                            Button::new_literal("Compile")
                                .rich_tooltip("compile"),
                        ),
                ),
        )
    }

    fn menus_builder(&self, ctx: &mut BuildContext, theme: &Theme, sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Menus & Dropdowns")
                        .style(t.body_bold.clone())
                        .color(c.text_primary),
                )
                .child(
                    TextWidget::new_literal("ComboBox")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .child(
                    HStack::new().spacing(16.0).child(
                        ComboBox::new(
                            vec!["Apple", "Banana", "Cherry", "Date", "Elderberry"],
                            sigs.combo_selected.clone(),
                        )
                        .placeholder("Select a fruit..."),
                    ),
                )
                .child(
                    TextWidget::new_literal("Context Menu (right-click the panel below)")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .child(
                    Panel::new()
                        .background(c.surface_main)
                        .corner_radius(8.0)
                        .padding(16.0)
                        .child(
                            TextWidget::new_literal("Right-click here for a context menu")
                                .style(t.body.clone())
                                .color(c.text_primary),
                        )
                        .context_menu(|| {
                            Box::new(
                                MenuList::new()
                                    .item(
                                        MenuItem::new_literal("Cut")
                                            .on_activate(Cmd::Cut)
                                            .shortcut_label("Ctrl+X"),
                                    )
                                    .item(
                                        MenuItem::new_literal("Copy")
                                            .on_activate(Cmd::Copy)
                                            .shortcut_label("Ctrl+C"),
                                    )
                                    .item(
                                        MenuItem::new_literal("Paste")
                                            .on_activate(Cmd::Paste)
                                            .shortcut_label("Ctrl+V"),
                                    )
                                    .separator()
                                    .item(MenuItem::new_literal("Disabled item").enabled(false)),
                            )
                        }),
                ),
        )
    }

    fn image_builder(&self, ctx: &mut BuildContext, theme: &Theme, _sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        let tree_img = fern_ui::res!("resources/icons/tree.webp");
        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Image Widget")
                        .style(t.body_bold.clone())
                        .color(c.text_primary),
                )
                .child(
                    TextWidget::new_literal("Full-color WebP photo, Contain fit, 300x200 display")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .child(
                    ImageWidget::new(tree_img)
                        .size(300.0, 200.0)
                        .fit(ImageFit::Contain)
                        .alt("A tree"),
                ),
        )
    }

    fn builtin_builder(&self, ctx: &mut BuildContext, theme: &Theme, sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        let vis_label = sigs
            .visibility_signal
            .map(|v| if *v { "Visible".to_string() } else { "Hidden".to_string() });

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Built-in Buttons")
                        .style(t.body_bold.clone())
                        .color(c.text_primary),
                )
                .child(
                    TextWidget::new_literal("Predefined (browse, expand, search, copy, clear, add)")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .child(
                    HStack::new()
                        .spacing(4.0)
                        .child(BuiltInButton::browse().on_activate(Cmd::Save))
                        .child(BuiltInButton::expand().on_activate(Cmd::Save))
                        .child(BuiltInButton::search().on_activate(Cmd::Save))
                        .child(BuiltInButton::copy().on_activate(Cmd::Copy))
                        .child(BuiltInButton::clear().on_activate(Cmd::Save))
                        .child(BuiltInButton::add().on_activate(Cmd::Save)),
                )
                .child(
                    TextWidget::new_literal("Visibility toggle")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .child(
                    HStack::new()
                        .spacing(8.0)
                        .child(BuiltInButton::visibility_toggle(sigs.visibility_signal.clone()))
                        .child(
                            TextWidget::new_literal("Hidden")
                                .bind_text(vis_label)
                                .style(t.body.clone())
                                .color(c.text_primary),
                        ),
                )
                .child(
                    TextWidget::new_literal("Size variants (compact, default, large)")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .child(
                    HStack::new()
                        .spacing(8.0)
                        .child(
                            BuiltInButton::search()
                                .size(BuiltInButtonSize::Compact)
                                .on_activate(Cmd::Save),
                        )
                        .child(BuiltInButton::search().on_activate(Cmd::Save))
                        .child(
                            BuiltInButton::search()
                                .size(BuiltInButtonSize::Large)
                                .on_activate(Cmd::Save),
                        ),
                )
                .child(
                    TextWidget::new_literal("Disabled")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .child(
                    HStack::new()
                        .spacing(4.0)
                        .child(BuiltInButton::browse().enabled(false))
                        .child(BuiltInButton::clear().enabled(false)),
                )
                .child(
                    TextWidget::new_literal("Custom icon")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .child(
                    BuiltInButton::new(IconWidget::checkmark(16.0))
                        .tooltip_literal("Custom checkmark")
                        .on_activate(Cmd::Save),
                ),
        )
    }

    fn text_input_builder(&self, ctx: &mut BuildContext, theme: &Theme, sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new_literal("Text Input")
                        .style(t.body_bold.clone())
                        .color(c.text_primary),
                )
                .child(
                    TextWidget::new_literal("Single-line text editing with placeholder, clear button, slots")
                        .style(t.small.clone())
                        .color(c.text_secondary),
                )
                .child(
                    TextInput::new(sigs.search_text.clone())
                        .placeholder("Search...")
                        .show_clear_button(true)
                        .leading_slot(
                            IconWidget::checkmark(14.0).color(c.text_secondary),
                        ),
                )
                .child(
                    TextInput::new(sigs.username_text.clone())
                        .placeholder("Username")
                        .label("Username")
                        .trailing_slot(
                            BuiltInButton::browse().on_activate(Cmd::Save),
                        ),
                )
                .child(
                    TextInput::new(sigs.readonly_text.clone())
                        .read_only(true),
                ),
        )
    }

    // =========================================================================
    // fern! section helpers — exact equivalents of *_builder methods above.
    // =========================================================================

    fn palette_fern(&self, ctx: &mut BuildContext, theme: &Theme, _sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;
        let mono = t.mono.clone();

        fern!(ctx =>
            VStack {
                spacing: 12.0
                TextWidget::new_literal("Theme Palette") {
                    style: t.body_bold.clone()
                    color: c.text_primary
                }
                TextWidget::new_literal("Surfaces") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                Grid {
                    columns: vec![
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                    ]
                    column_gap: 12.0
                    row_gap: 12.0
                    rows: vec![TrackSize::Auto, TrackSize::Auto]
                    child: surface_swatch(theme, c.surface_main, "surface_main", "text_primary", c.text_primary)
                    child: surface_swatch(theme, c.surface_content, "surface_content", "text_primary", c.text_primary)
                    child: surface_swatch(theme, c.surface_raised, "surface_raised", "text_primary", c.text_primary)
                    child: surface_swatch(theme, c.surface_sunken, "surface_sunken", "text_secondary", c.text_secondary)
                    child: surface_swatch(theme, c.surface_hover, "surface_hover", "text_primary", c.text_primary)
                    child: surface_swatch(theme, c.surface_pressed, "surface_pressed", "text_primary", c.text_primary)
                    child: surface_swatch(theme, c.surface_selected, "surface_selected", "selection_text_active", c.selection_text_active)
                    child: surface_swatch(theme, c.surface_selected_inactive, "surface_selected_inactive", "selection_text_inactive", c.selection_text_inactive)
                }
                TextWidget::new_literal("Text") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                Panel {
                    background: c.surface_main
                    border_color: c.border
                    border_width: 1.0
                    corner_radius: 8.0
                    padding: 16.0
                    VStack {
                        spacing: 6.0
                        child: text_sample(theme, "text_primary", c.text_primary, "body, main labels")
                        child: text_sample(theme, "text_secondary", c.text_secondary, "hints, captions, placeholders")
                        child: text_sample(theme, "text_disabled", c.text_disabled, "disabled labels")
                        child: text_sample(theme, "text_link", c.text_link, "hyperlinks")
                        child: text_sample(theme, "text_error", c.text_error, "validation errors")
                        child: text_sample(theme, "text_warning", c.text_warning, "validation warnings")
                        child: text_sample(theme, "text_success", c.text_success, "success messages")
                    }
                }
                Panel {
                    background: c.accent
                    corner_radius: 4.0
                    padding: 12.0
                    HStack {
                        spacing: 12.0
                        TextWidget::new_literal("Default button label") {
                            style: t.body.clone()
                            color: c.text_on_accent
                        }
                        Spacer { }
                        TextWidget::new_literal("text_on_accent on accent") {
                            style: t.tiny.clone()
                            color: c.text_on_accent
                        }
                    }
                }
                TextWidget::new_literal("Editor") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                Panel {
                    background: c.editor_bg
                    border_color: c.border_strong
                    border_width: 1.0
                    corner_radius: 6.0
                    padding: 8.0
                    VStack {
                        spacing: 2.0
                        child: editor_line(theme, "1", "fn main() {")
                        Panel {
                            background: c.editor_current_line_bg
                            corner_radius: 0.0
                            border_width: 0.0
                            padding: 4.0
                            HStack {
                                spacing: 12.0
                                FixedSize {
                                    bind_width: 24.0_f32
                                    TextWidget::new_literal("2") {
                                        style: mono.clone()
                                        color: c.editor_gutter_fg
                                    }
                                }
                                TextWidget::new_literal("    let ") {
                                    style: mono.clone()
                                    color: c.editor_fg
                                }
                                Panel {
                                    background: c.editor_selection_bg
                                    corner_radius: 2.0
                                    padding: 0.0
                                    border_width: 0.0
                                    Padding::symmetric(1.0, 2.0) {
                                        TextWidget::new_literal("x") {
                                            style: mono.clone()
                                            color: c.editor_fg
                                        }
                                    }
                                }
                                TextWidget::new_literal(" = 42;") {
                                    style: mono.clone()
                                    color: c.editor_fg
                                }
                                FixedSize {
                                    bind_width: 1.5_f32
                                    bind_height: 16.0_f32
                                    Panel {
                                        background: c.editor_caret
                                        corner_radius: 0.0
                                        border_width: 0.0
                                        padding: 0.0
                                        Spacer { }
                                    }
                                }
                            }
                        }
                        child: editor_line(theme, "3", "    println!(\"{}\", x);")
                        child: editor_line(theme, "4", "}")
                    }
                }
                Grid {
                    columns: vec![
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                    ]
                    column_gap: 12.0
                    row_gap: 12.0
                    rows: vec![TrackSize::Auto, TrackSize::Auto]
                    child: editor_swatch(theme, c.editor_bg, "editor_bg", c.editor_fg)
                    child: editor_swatch(theme, c.editor_fg, "editor_fg", c.editor_bg)
                    child: editor_swatch(theme, c.editor_caret, "editor_caret", c.editor_bg)
                    child: editor_swatch(theme, c.editor_current_line_bg, "editor_current_line_bg", c.editor_fg)
                    child: editor_swatch(theme, c.editor_gutter_fg, "editor_gutter_fg", c.editor_bg)
                    child: editor_swatch(theme, c.editor_selection_bg, "editor_selection_bg", c.editor_fg)
                }
            }
        )
    }

    fn primitives_fern(&self, ctx: &mut BuildContext, theme: &Theme, _sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        fern!(ctx =>
            VStack {
                spacing: 8.0
                TextWidget::new_literal("Primitives") {
                    style: t.body_bold.clone()
                    color: c.text_primary
                }
                TextWidget::new_literal("Divider (horizontal, vertical, thick, colored)") {
                    style: t.tiny.clone()
                    color: c.text_secondary
                }
                HStack {
                    spacing: 16.0
                    VStack {
                        spacing: 4.0
                        TextWidget::new_literal("H") {
                            style: t.tiny.clone()
                            color: c.text_secondary
                        }
                        Divider { }
                    }
                    HStack {
                        spacing: 4.0
                        TextWidget::new_literal("V") {
                            style: t.tiny.clone()
                            color: c.text_secondary
                        }
                        Divider::vertical() {
                            thickness: 2.0
                            color: c.accent
                        }
                    }
                    VStack {
                        spacing: 4.0
                        TextWidget::new_literal("Thick") {
                            style: t.tiny.clone()
                            color: c.text_secondary
                        }
                        Divider {
                            thickness: 4.0
                            color: c.text_error
                        }
                    }
                }
                TextWidget::new_literal("IconWidget (checkmark, chevrons)") {
                    style: t.tiny.clone()
                    color: c.text_secondary
                }
                HStack {
                    spacing: 12.0
                    IconWidget::checkmark(24.0) { color: c.accent }
                    IconWidget::chevron_down(24.0) { color: c.accent_subtle_bg }
                    IconWidget::chevron_right(24.0) { color: c.text_error }
                }
            }
        )
    }

    fn layout_fern(&self, ctx: &mut BuildContext, theme: &Theme, _sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        fern!(ctx =>
            VStack {
                spacing: 8.0
                TextWidget::new_literal("Layout Primitives") {
                    style: t.body_bold.clone()
                    color: c.text_primary
                }
                TextWidget::new_literal("Grid (Fixed 80px | 1fr | 2fr, with 8px gap)") {
                    style: t.tiny.clone()
                    color: c.text_secondary
                }
                Grid {
                    columns: vec![
                        TrackSize::Fixed(80.0),
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(2.0),
                    ]
                    rows: vec![TrackSize::Auto, TrackSize::Auto]
                    column_gap: 8.0
                    row_gap: 8.0
                    child: build_color_cell(c.accent, "A1")
                    child: build_color_cell(c.accent_subtle_bg, "B1")
                    child: build_color_cell(c.text_error, "C1")
                    child: build_color_cell(c.status_info_fg, "A2")
                    child: build_color_cell(c.text_success, "B2")
                    child: build_color_cell(c.text_warning, "C2")
                }
                TextWidget::new_literal("Wrap (flow layout, 8px spacing)") {
                    style: t.tiny.clone()
                    color: c.text_secondary
                }
                Wrap {
                    spacing: 8.0
                    line_spacing: 8.0
                    Badge::new_literal("Rust")
                    Badge::new_literal("GUI")
                    Badge::new_literal("Accessible")
                    Badge::new_literal("Reactive")
                    Badge::new_literal("Fast")
                    Badge::new_literal("Cross-platform")
                    Badge::new_literal("Retained")
                    Badge::new_literal("wgpu")
                }
            }
        )
    }

    fn controls_fern(&self, ctx: &mut BuildContext, theme: &Theme, sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        let save_icon = fern_ui::res!("resources/icons/save.svg");
        let home_icon = fern_ui::res!("resources/icons/home.svg");
        let star_icon = fern_ui::res!("resources/icons/star.png");
        let clock_icon = fern_ui::res!("resources/icons/clock.webp");

        fern!(ctx =>
            VStack {
                spacing: 12.0
                TextWidget::new_literal("Form Controls") {
                    style: t.body_bold.clone()
                    color: c.text_primary
                }
                TextWidget::new_literal("Button") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                VStack {
                    spacing: 8.0
                    GroupHeader::new_literal("Standard buttons")
                    HStack {
                        spacing: 8.0
                        Button::new_literal("Save") {
                            style: ButtonVariant::Default
                            on_activate: Cmd::Save
                        }
                        Button::new_literal("Cancel") {
                            style: ButtonVariant::Regular
                            on_activate: Cmd::Cancel
                        }
                        Button::new_literal("Learn more") {
                            style: ButtonVariant::Flat
                            on_activate: Cmd::LearnMore
                        }
                    }
                    HStack {
                        spacing: 8.0
                        Button::new_literal("Save") {
                            style: ButtonVariant::Default
                            enabled: false
                        }
                        Button::new_literal("Cancel") {
                            style: ButtonVariant::Regular
                            enabled: false
                        }
                        Button::new_literal("Learn more") {
                            style: ButtonVariant::Flat
                            enabled: false
                        }
                    }
                    GroupHeader::new_literal("Icon buttons")
                    HStack {
                        spacing: 8.0
                        Button::new_literal("Save") {
                            icon: IconWidget::from_svg_icon(save_icon), IconLocation::Leading
                            style: ButtonVariant::Default
                            on_activate: Cmd::Save
                        }
                        Button::new_literal("Home") {
                            icon: IconWidget::from_svg_icon(home_icon), IconLocation::Leading
                            style: ButtonVariant::Regular
                            on_activate: Cmd::Cancel
                        }
                        Button::new_literal("Star") {
                            icon: IconWidget::from_raster(star_icon, 24.0), IconLocation::Leading
                            style: ButtonVariant::Regular
                            on_activate: Cmd::Save
                        }
                        Button::new_literal("Clock") {
                            icon: IconWidget::from_raster(clock_icon, 24.0), IconLocation::Leading
                            style: ButtonVariant::Regular
                            on_activate: Cmd::Cancel
                        }
                        Button::new_literal("Save") {
                            icon: IconWidget::from_svg_icon(save_icon), IconLocation::IconOnly
                            style: ButtonVariant::Flat
                            on_activate: Cmd::Save
                        }
                        Button::new_literal("") {
                            icon: IconWidget::chevron_down(16.0), IconLocation::IconOnly
                            style: ButtonVariant::Regular
                            on_activate: Cmd::Cancel
                        }
                    }
                    GroupHeader::new_literal("Split buttons")
                    HStack {
                        spacing: 8.0
                        SplitButton {
                            item: MenuItem::new_literal("Run") {
                                on_activate: Cmd::Run
                                tooltip_literal: "Run the current configuration"
                            }
                            item: MenuItem::new_literal("Run Tests") {
                                on_activate: Cmd::RunTests
                                tooltip_literal: "Run the test suite"
                            }
                            item: MenuItem::new_literal("Run with Coverage") {
                                on_activate: Cmd::RunCoverage
                                tooltip_literal: "Run and collect code coverage"
                            }
                            separator
                            item: MenuItem::new_literal("Debug") {
                                on_activate: Cmd::Debug
                                tooltip_literal: "Launch the debugger"
                            }
                            tooltip_literal: "Run the selected configuration"
                            chevron_tooltip_literal: "Other run configurations"
                            style: ButtonVariant::Default
                        }
                        SplitButton::new_static() {
                            item: MenuItem::new_literal("Save") {
                                on_activate: Cmd::Save
                                tooltip_literal: "Save the current file"
                            }
                            item: MenuItem::new_literal("Save As…") {
                                on_activate: Cmd::SaveAs
                                tooltip_literal: "Save the current file under a new name"
                            } 
                            tooltip_literal: "Save (main action stays pinned)"
                            style: ButtonVariant::Regular
                        }
                        SplitButton {
                            item: MenuItem::new_literal("Run") { on_activate: Cmd::Run }
                            item: MenuItem::new_literal("Debug") { on_activate: Cmd::Debug }
                            enabled: false
                        }
                    }
                }
                Divider { }
                Grid {
                    columns: vec![
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                        TrackSize::Fractional(1.0),
                    ]
                    column_gap: 16.0
                    rows: vec![TrackSize::Auto]
                    VStack {
                        spacing: 4.0
                        TextWidget::new_literal("Checkbox") {
                            style: t.small.clone()
                            color: c.text_secondary
                        }
                        VStack {
                            spacing: 8.0
                            Checkbox(sigs.checkbox_checked.clone()) {
                                label_literal: "Accept terms"
                                tooltip_literal: "Click to accept the terms and conditions"
                            }
                            Checkbox(sigs.cb_sounds.clone()) {
                                label_literal: "Play notification sounds"
                                caption_literal: "Play a short chime when a new message arrives. Muted automatically while you're in a call."
                            }
                            Checkbox(sigs.cb_disabled_state.clone()) {
                                label_literal: "Always on (disabled)"
                                enabled: false
                            }
                            Checkbox::tristate(sigs.tristate.clone()) {
                                label_literal: "Select all (tristate)"
                                caption_literal: "Cycles unchecked → checked → indeterminate"
                                tooltip_literal: "Cycles: unchecked, checked, indeterminate"
                            }
                        }
                    }
                    VStack {
                        spacing: 4.0
                        TextWidget::new_literal("RadioButton") {
                            style: t.small.clone()
                            color: c.text_secondary
                        }
                        VStack {
                            spacing: 8.0
                            RadioButton(0, sigs.radio_selected.clone()) {
                                label_literal: "Standard"
                                caption_literal: "Recommended for most users — sensible defaults."
                                tooltip_literal: "First option"
                            }
                            RadioButton(1, sigs.radio_selected.clone()) {
                                label_literal: "Advanced"
                                caption_literal: "Expose every option, including experimental features that may change between releases."
                            }
                            RadioButton(2, sigs.radio_selected.clone()) {
                                label_literal: "Custom"
                            }
                        }
                    }
                    VStack {
                        spacing: 4.0
                        TextWidget::new_literal("Toggle") {
                            style: t.small.clone()
                            color: c.text_secondary
                        }
                        VStack {
                            spacing: 8.0
                            Toggle(sigs.toggle_on.clone())
                            Toggle(sigs.toggle_label_on.clone()) {
                                label_literal: "Notifications"
                            }
                            Toggle(sigs.toggle_disabled_state.clone()) {
                                enabled: false
                            }
                        }
                    }
                }
                Divider { }
                TextWidget::new_literal("Slider") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                HStack {
                    spacing: 16.0
                    VStack {
                        spacing: 8.0
                        TextWidget::new_literal("Horizontal") {
                            style: t.tiny.clone()
                            color: c.text_secondary
                        }
                        Slider(sigs.slider_value.clone(), 0.0, 100.0)
                        TextWidget::new_literal("Stepped (25)") {
                            style: t.tiny.clone()
                            color: c.text_secondary
                        }
                        Slider(sigs.slider_stepped.clone(), 0.0, 100.0) {
                            step: 25.0
                        }
                        TextWidget::new_literal("Disabled") {
                            style: t.tiny.clone()
                            color: c.text_secondary
                        }
                        Slider(sigs.slider_disabled_state.clone(), 0.0, 100.0) {
                            enabled: false
                        }
                    }
                    VStack {
                        spacing: 4.0
                        TextWidget::new_literal("Vertical") {
                            style: t.tiny.clone()
                            color: c.text_secondary
                        }
                        slider_vert = Slider(sigs.slider_v_value.clone(), 0.0, 1.0) {
                            orientation: Orientation::Vertical
                        }
                        MaxSize(f32::MAX, 120.0) {
                            child_id: slider_vert
                        }
                    }
                }
                Divider { }
                TextWidget::new_literal("SegmentedControl") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                SegmentedControl(
                    vec!["Day".into(), "Week".into(), "Month".into(), "Year".into()],
                    sigs.segment_selected.clone()
                )
            }
        )
    }

    fn display_fern(&self, ctx: &mut BuildContext, theme: &Theme, _sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        fern!(ctx =>
            VStack {
                spacing: 8.0
                TextWidget::new_literal("Display Widgets") {
                    style: t.body_bold.clone()
                    color: c.text_primary
                }
                TextWidget::new_literal("ProgressBar") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                HStack {
                    spacing: 16.0
                    VStack {
                        spacing: 8.0
                        TextWidget::new_literal("Determinate (65%)") {
                            style: t.tiny.clone()
                            color: c.text_secondary
                        }
                        ProgressBar(0.65)
                        TextWidget::new_literal("Indeterminate") {
                            style: t.tiny.clone()
                            color: c.text_secondary
                        }
                        ProgressBar::indeterminate()
                        TextWidget::new_literal("Custom colors + thick") {
                            style: t.tiny.clone()
                            color: c.text_secondary
                        }
                        ProgressBar(0.4) {
                            thickness: 8.0
                            fill_color: c.text_success
                            track_color: c.surface_sunken
                        }
                    }
                    VStack {
                        spacing: 4.0
                        TextWidget::new_literal("Vertical") {
                            style: t.tiny.clone()
                            color: c.text_secondary
                        }
                        pb_vert = ProgressBar(0.7) {
                            orientation: Orientation::Vertical
                            thickness: 8.0
                        }
                        MaxSize(f32::MAX, 80.0) {
                            child_id: pb_vert
                        }
                    }
                }
                Divider { }
                TextWidget::new_literal("Badge") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                HStack {
                    spacing: 8.0
                    Badge::new_literal("Default")
                    Badge::new_literal("3") {
                        color: c.text_error
                        text_color: Color::WHITE
                    }
                    Badge::new_literal("New") {
                        color: c.text_success
                        text_color: Color::WHITE
                    }
                    Badge::new_literal("Beta") {
                        color: c.text_warning
                    }
                }
            }
        )
    }

    fn text_overflow_fern(&self, ctx: &mut BuildContext, theme: &Theme, _sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        const LOREM: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing \
                             elit. Sed do eiusmod tempor incididunt ut labore \
                             et dolore magna aliqua. Ut enim ad minim veniam, \
                             quis nostrud exercitation ullamco laboris nisi ut \
                             aliquip ex ea commodo consequat.";
        const LONG_TITLE: &str =
            "A somewhat verbose section title that almost certainly will not fit";

        fern!(ctx =>
            VStack {
                spacing: 8.0
                TextWidget::new_literal("Text overflow") {
                    style: t.body_bold.clone()
                    color: c.text_primary
                    single_line
                }
                TextWidget::new_literal("Wrap (default) — grows vertically") {
                    style: t.small.clone()
                    color: c.text_secondary
                    single_line
                }
                FixedSize {
                    bind_width: 360.0_f32
                    TextWidget::new_literal(LOREM) {
                        style: t.body.clone()
                        color: c.text_primary
                    }
                }
                TextWidget::new_literal("Wrap capped at 2 lines") {
                    style: t.small.clone()
                    color: c.text_secondary
                    single_line
                }
                FixedSize {
                    bind_width: 360.0_f32
                    TextWidget::new_literal(LOREM) {
                        style: t.body.clone()
                        color: c.text_primary
                        max_lines: 2
                    }
                }
                TextWidget::new_literal("Ellipsis — trailing / middle / leading") {
                    style: t.small.clone()
                    color: c.text_secondary
                    single_line
                }
                FixedSize {
                    bind_width: 280.0_f32
                    TextWidget::new_literal(LONG_TITLE) {
                        style: t.body.clone()
                        color: c.text_primary
                        overflow: TextOverflow::Ellipsis(EllipsisMode::Trailing)
                    }
                }
                FixedSize {
                    bind_width: 280.0_f32
                    TextWidget::new_literal(LONG_TITLE) {
                        style: t.body.clone()
                        color: c.text_primary
                        overflow: TextOverflow::Ellipsis(EllipsisMode::Middle)
                    }
                }
                FixedSize {
                    bind_width: 280.0_f32
                    TextWidget::new_literal(LONG_TITLE) {
                        style: t.body.clone()
                        color: c.text_primary
                        overflow: TextOverflow::Ellipsis(EllipsisMode::Leading)
                    }
                }
            }
        )
    }

    fn containers_fern(&self, ctx: &mut BuildContext, theme: &Theme, sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        fern!(ctx =>
            VStack {
                spacing: 8.0
                TextWidget::new_literal("Containers") {
                    style: t.body_bold.clone()
                    color: c.text_primary
                }
                TextWidget::new_literal("Card") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                Card {
                    header: TextWidget::new_literal("Card Header") {
                        style: t.small.clone()
                        color: c.text_secondary
                    }
                    content: (TextWidget::new_literal("Card content with shadow and themed background.").style(t.body.clone()).color(c.text_primary))
                    
                    footer: TextWidget::new_literal("Footer text") {
                        style: t.tiny.clone()
                        color: c.text_secondary
                    }
                }
                Divider { }
                TextWidget::new_literal("Accordion") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                acc_content1 = TextWidget::new_literal("This content is revealed with an animated expand.") {
                    style: t.body.clone()
                    color: c.text_primary
                }
                Accordion::new_literal("Click to expand", sigs.accordion_expanded.clone()) {
                    content_id: acc_content1
                }
                acc_content2 = TextWidget::new_literal("This section starts expanded and can be collapsed.") {
                    style: t.body.clone()
                    color: c.text_primary
                }
                Accordion::new_literal("Already expanded", sigs.accordion2_expanded.clone()) {
                    content_id: acc_content2
                }
                Divider { }
                TextWidget::new_literal("GroupBox") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                GroupBox::new_literal("Appearance") {
                    child: VStack {
                        spacing: 4.0
                        TextWidget::new_literal("Indented content under a bold title.") {
                            style: t.body.clone()
                            color: c.text_primary
                        }
                        TextWidget::new_literal("No border, no frame — Int UI style.") {
                            style: t.body.clone()
                            color: c.text_secondary
                        }
                    }
                }
                GroupBox::new_literal("Notifications") {
                    checkable: sigs.group_box_notifications_on.clone()
                    child: VStack {
                        spacing: 4.0
                        TextWidget::new_literal(
                            "Uncheck the title to disable this whole subtree.",
                        ) {
                            style: t.body.clone()
                            color: c.text_primary
                        }
                        Button::new_literal("Inside — tap me") {
                            style: ButtonVariant::Default
                        }
                    }
                }
            }
        )
    }

    fn nav_fern(&self, ctx: &mut BuildContext, theme: &Theme, sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        fern!(ctx =>
            VStack {
                spacing: 8.0
                TextWidget::new_literal("Navigation") {
                    style: t.body_bold.clone()
                    color: c.text_primary
                }
                TextWidget::new_literal("TabWidget") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                FixedSize {
                    bind_height: 240.0_f32
                    TabWidget(sigs.tabs_selected.clone()) {
                        tab_literal: "Overview", Panel {
                            padding: 16.0
                            VStack {
                                spacing: 8.0
                                TextWidget::new_literal("Overview") {
                                    style: t.body_bold.clone()
                                    color: c.text_primary
                                }
                                TextWidget::new_literal(
                                    "TabWidget is a retained container with dormant panes: \
                                     only the active tab is built, inactive panes keep \
                                     their state but don't receive layout or paint until \
                                     they're re-activated."
                                ) {
                                    style: t.body.clone()
                                    color: c.text_primary
                                }
                                HStack {
                                    spacing: 8.0
                                    Badge::new_literal("Dormant panes")
                                    Badge::new_literal("Arrow keys")
                                    Badge::new_literal("Trailing slot")
                                }
                            }
                        }
                        tab_literal: "Usage", Panel {
                            padding: 16.0
                            VStack {
                                spacing: 8.0
                                TextWidget::new_literal("Usage") {
                                    style: t.body_bold.clone()
                                    color: c.text_primary
                                }
                                TextWidget::new_literal(
                                    "Press Tab to move focus into the tab strip, then \
                                     Arrow Left / Arrow Right to switch between tabs. \
                                     Disabled tabs are skipped by keyboard navigation."
                                ) {
                                    style: t.body.clone()
                                    color: c.text_primary
                                }
                            }
                        }
                        tab_literal: "Structure", Panel {
                            padding: 16.0
                            VStack {
                                spacing: 8.0
                                TextWidget::new_literal("Structure") {
                                    style: t.body_bold.clone()
                                    color: c.text_primary
                                }
                                TextWidget::new_literal(
                                    "Int UI tabs: flat headers, no rounded corners, no \
                                     borders. The selected tab is marked only by a 3 dp \
                                     accent underline at its bottom edge, which \
                                     overpaints the tab bar's own 1 dp separator."
                                ) {
                                    style: t.body.clone()
                                    color: c.text_primary
                                }
                            }
                        }
                        tab_item: TabItem::new_literal(
                            "Disabled",
                            Panel::new().padding(16.0).child(
                                TextWidget::new_literal(
                                    "Disabled panes are still listed in the tab bar but \
                                     cannot be activated by click or keyboard."
                                ).style(t.body.clone()).color(c.text_primary)
                            )
                        ) {
                            enabled: false
                        }
                        trailing_slot: Button::new_literal("More") {
                            style: ButtonVariant::Flat
                            on_activate: Cmd::LinkClicked
                        }
                    }
                }
                TextWidget::new_literal("Link") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                HStack {
                    spacing: 16.0
                    Link::new_literal("Click me") {
                        on_activate: Cmd::LinkClicked
                        tooltip_literal: "Fires the LinkClicked command"
                    }
                    Link::new_literal("FernUI Documentation") {
                        url: "https://github.com/jacquetc/fern-ui"
                    }
                }
            }
        )
    }

    fn rich_tooltips_fern(&self, ctx: &mut BuildContext, theme: &Theme, _sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        fern!(ctx =>
            VStack {
                spacing: 8.0
                TextWidget::new_literal("Rich Tooltips") {
                    style: t.body_bold.clone()
                    color: c.text_primary
                }
                TextWidget::new_literal(
                    "Hover the buttons below. Inline `[label](:key)` links \
                     open nested tooltips; `https://` links open in the browser."
                ) {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                HStack {
                    spacing: 12.0
                    Button::new_literal("Save As…") { rich_tooltip: "save-as" }
                    Button::new_literal("Autosave info") { rich_tooltip: "autosave" }
                    Button::new_literal("Compile") { rich_tooltip: "compile" }
                }
            }
        )
    }

    fn menus_fern(&self, ctx: &mut BuildContext, theme: &Theme, sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        fern!(ctx =>
            VStack {
                spacing: 8.0
                TextWidget::new_literal("Menus & Dropdowns") {
                    style: t.body_bold.clone()
                    color: c.text_primary
                }
                TextWidget::new_literal("ComboBox") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                HStack {
                    spacing: 16.0
                    ComboBox(
                        vec!["Apple", "Banana", "Cherry", "Date", "Elderberry"],
                        sigs.combo_selected.clone()
                    ) {
                        placeholder: "Select a fruit..."
                    }
                }
                TextWidget::new_literal("Context Menu (right-click the panel below)") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                Panel {
                    background: c.surface_main
                    corner_radius: 8.0
                    padding: 16.0
                    context_menu: || Box::new(
                        MenuList::new()
                            .item(MenuItem::new_literal("Cut").on_activate(Cmd::Cut).shortcut_label("Ctrl+X"))
                            .item(MenuItem::new_literal("Copy").on_activate(Cmd::Copy).shortcut_label("Ctrl+C"))
                            .item(MenuItem::new_literal("Paste").on_activate(Cmd::Paste).shortcut_label("Ctrl+V"))
                            .separator()
                            .item(MenuItem::new_literal("Disabled item").enabled(false))
                    )
                    TextWidget::new_literal("Right-click here for a context menu") {
                        style: t.body.clone()
                        color: c.text_primary
                    }
                }
            }
        )
    }

    fn image_fern(&self, ctx: &mut BuildContext, theme: &Theme, _sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;
        let tree_img = fern_ui::res!("resources/icons/tree.webp");

        fern!(ctx =>
            VStack {
                spacing: 8.0
                TextWidget::new_literal("Image Widget") {
                    style: t.body_bold.clone()
                    color: c.text_primary
                }
                TextWidget::new_literal("Full-color WebP photo, Contain fit, 300x200 display") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                ImageWidget(tree_img) {
                    size: 300.0, 200.0
                    fit: ImageFit::Contain
                    alt: "A tree"
                }
            }
        )
    }

    fn builtin_fern(&self, ctx: &mut BuildContext, theme: &Theme, sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        let vis_label = sigs
            .visibility_signal
            .map(|v| if *v { "Visible".to_string() } else { "Hidden".to_string() });

        fern!(ctx =>
            VStack {
                spacing: 8.0
                TextWidget::new_literal("Built-in Buttons") {
                    style: t.body_bold.clone()
                    color: c.text_primary
                }
                TextWidget::new_literal("Predefined (browse, expand, search, copy, clear, add)") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                HStack {
                    spacing: 4.0
                    BuiltInButton::browse() { on_activate: Cmd::Save }
                    BuiltInButton::expand() { on_activate: Cmd::Save }
                    BuiltInButton::search() { on_activate: Cmd::Save }
                    BuiltInButton::copy()   { on_activate: Cmd::Copy }
                    BuiltInButton::clear()  { on_activate: Cmd::Save }
                    BuiltInButton::add()    { on_activate: Cmd::Save }
                }
                TextWidget::new_literal("Visibility toggle") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                HStack {
                    spacing: 8.0
                    BuiltInButton::visibility_toggle(sigs.visibility_signal.clone())
                    TextWidget::new_literal("Hidden") {
                        bind_text: vis_label
                        style: t.body.clone()
                        color: c.text_primary
                    }
                }
                TextWidget::new_literal("Size variants (compact, default, large)") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                HStack {
                    spacing: 8.0
                    BuiltInButton::search() {
                        size: BuiltInButtonSize::Compact
                        on_activate: Cmd::Save
                    }
                    BuiltInButton::search() { on_activate: Cmd::Save }
                    BuiltInButton::search() {
                        size: BuiltInButtonSize::Large
                        on_activate: Cmd::Save
                    }
                }
                TextWidget::new_literal("Disabled") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                HStack {
                    spacing: 4.0
                    BuiltInButton::browse() { enabled: false }
                    BuiltInButton::clear()  { enabled: false }
                }
                TextWidget::new_literal("Custom icon") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                BuiltInButton(IconWidget::checkmark(16.0)) {
                    tooltip_literal: "Custom checkmark"
                    on_activate: Cmd::Save
                }
            }
        )
    }

    fn text_input_fern(&self, ctx: &mut BuildContext, theme: &Theme, sigs: &Signals) -> WidgetId {
        let t = &theme.typography;
        let c = &theme.colors;

        fern!(ctx =>
            VStack {
                spacing: 8.0
                TextWidget::new_literal("Text Input") {
                    style: t.body_bold.clone()
                    color: c.text_primary
                }
                TextWidget::new_literal("Single-line text editing with placeholder, clear button, slots") {
                    style: t.small.clone()
                    color: c.text_secondary
                }
                TextInput(sigs.search_text.clone()) {
                    placeholder: "Search..."
                    show_clear_button: true
                    leading_slot: IconWidget::checkmark(14.0) { color: c.text_secondary }
                }
                TextInput(sigs.username_text.clone()) {
                    placeholder: "Username"
                    label: "Username"
                    trailing_slot: BuiltInButton::browse() { on_activate: Cmd::Save }
                }
                TextInput(sigs.readonly_text.clone()) {
                    read_only: true
                }
            }
        )
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let is_dark = Rc::new(Cell::new(false));
    let is_dark_clone = is_dark.clone();

    FernAppBuilder::new()
        .theme(Theme::light_default())
        .window_title("FernUI -- Widget Catalog (Milestone 3)")
        .window_size(1600, 900)
        .register_tooltips(vec![
            TooltipContent::new(
                "save-as",
                LocalizedString::literal(
                    "Save the current file under a new name",
                ),
            )
            .with_shortcut_label("Ctrl+Shift+S"),

            TooltipContent::new(
                "autosave",
                LocalizedString::literal(
                    "FernUI **autosaves** your work every *2 minutes*. \
                     Click [here](:autosave-details) for details, or \
                     read the [full docs](https://github.com/jacquetc/fern-ui).",
                ),
            )
            .with_more(LocalizedString::literal(
                "Autosave uses **debounced writes** so bursts of edits \
                 only hit disk once. Disable it in [Preferences](:prefs-general).",
            )),

            TooltipContent::new(
                "autosave-details",
                LocalizedString::literal(
                    "Autosave runs on a debounced timer: it only writes \
                     to disk after typing pauses for 500ms.",
                ),
            ),

            TooltipContent::new(
                "prefs-general",
                LocalizedString::literal(
                    "Open Preferences → General to toggle autosave and \
                     change its interval.",
                ),
            ),

            TooltipContent::new(
                "compile",
                LocalizedString::literal(
                    "**Compile** the current project. Uses incremental \
                     builds when possible.",
                ),
            )
            .with_more(LocalizedString::literal(
                "The compile step runs `cargo build` under the hood. \
                 Output lands in the Build tab.",
            ))
            .with_shortcut_label("F9"),
        ])
        .on_command(move |cmd: &Cmd, ctx| match cmd {
            Cmd::ToggleDarkMode => {
                let dark = !is_dark_clone.get();
                is_dark_clone.set(dark);
                if dark {
                    ctx.set_theme(Theme::dark_default());
                } else {
                    ctx.set_theme(Theme::light_default());
                }
            }
            Cmd::LinkClicked => {
                println!("Link clicked!");
            }
            Cmd::Cut => println!("Cut"),
            Cmd::Copy => println!("Copy"),
            Cmd::Paste => println!("Paste"),
            Cmd::Save => println!("Save"),
            Cmd::SaveAs => println!("Save As"),
            Cmd::Cancel => println!("Cancel"),
            Cmd::LearnMore => println!("Learn more"),
            Cmd::Run => println!("Run"),
            Cmd::RunTests => println!("Run Tests"),
            Cmd::RunCoverage => println!("Run with Coverage"),
            Cmd::Debug => println!("Debug"),
        })
        .root(|tree| tree.add(WidgetCatalog::new()))
        .run();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use fern_render::test_support;
    use fern_ui::core::WidgetTree;
    use fern_ui::prelude::*;
    use fern_ui::text::SharedTypesetter;
    use fern_ui::widgets::*;

    use super::WidgetCatalog;

    fn tree_with_real_text_backend() -> WidgetTree {
        let typesetter = SharedTypesetter::new_with_default_font();
        WidgetTree::new()
            .with_theme(Theme::light_default())
            .with_text_backend(typesetter.as_text_backend())
    }

    fn tree_and_typesetter() -> (WidgetTree, SharedTypesetter) {
        let typesetter = SharedTypesetter::new_with_default_font();
        let tree = WidgetTree::new()
            .with_theme(Theme::light_default())
            .with_text_backend(typesetter.as_text_backend());
        (tree, typesetter)
    }

    #[test]
    fn catalog_builds_and_layouts() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let root = tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let b = tree.bounds(root);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn catalog_renders_without_crash() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame = tree.render();
        assert!(
            !frame.shapes.is_empty() || !frame.decorations.is_empty(),
            "catalog should produce render output"
        );
    }

    #[test]
    fn catalog_theme_switch() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame_light = tree.render();

        tree.set_theme(Theme::dark_default());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame_dark = tree.render();

        assert_ne!(
            frame_light.shapes, frame_dark.shapes,
            "theme switch should produce different output"
        );
    }

    #[test]
    fn second_render_same_output() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame1 = tree.render();
        let cmds1 = frame1.draw_order.len();

        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame2 = tree.render();
        let cmds2 = frame2.draw_order.len();

        assert_eq!(
            cmds1, cmds2,
            "second render must produce same draw commands (got {cmds1} vs {cmds2})"
        );
    }

    #[test]
    fn theme_switch_preserves_draw_commands() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame1 = tree.render();
        let cmds_before = frame1.draw_order.len();

        tree.set_theme(Theme::dark_default());
        tree.layout(SizeProposal::exact(1600.0, 900.0));
        let frame2 = tree.render();
        let cmds_after = frame2.draw_order.len();

        assert!(
            cmds_after > cmds_before / 2,
            "draw commands after theme switch ({cmds_after}) should be \
             close to before ({cmds_before})"
        );
    }

    #[test]
    fn scroll_area_fills_remaining_space() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let root = tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(1600.0, 900.0));

        // root is WidgetCatalog → SplitView → [first, handle, second],
        // each pane is a VStack → [Toolbar, Expand, StatusBar].
        let split_children = {
            let adapter_children = tree.children(root);
            assert_eq!(
                adapter_children.len(),
                1,
                "composite adapter has one child (SplitView)"
            );
            tree.children(adapter_children[0])
        };
        assert_eq!(
            split_children.len(),
            3,
            "SplitView must have first, handle, second (got {})",
            split_children.len()
        );

        for (label, pane_id) in [("left", split_children[0]), ("right", split_children[2])] {
            // SplitView wraps each pane in a `ClipPane` container so
            // overflowing content is clipped to the pane bounds — the
            // actual VStack is one level deeper.
            let clip_children = tree.children(pane_id);
            assert_eq!(
                clip_children.len(),
                1,
                "{label} pane is a ClipPane with exactly one child"
            );
            let vstack_children = tree.children(clip_children[0]);
            assert_eq!(
                vstack_children.len(),
                3,
                "{label} pane VStack must have toolbar, expand, statusbar"
            );

            let toolbar_bounds = tree.bounds(vstack_children[0]);
            let expand_bounds = tree.bounds(vstack_children[1]);
            let status_bounds = tree.bounds(vstack_children[2]);

            assert!(
                expand_bounds.height > 400.0,
                "{label} pane Expand wrapping ScrollArea should fill >400px, got {}",
                expand_bounds.height
            );

            assert!(
                (status_bounds.y + status_bounds.height - 900.0).abs() < 1.0,
                "{label} pane StatusBar should reach bottom of window: y={}, h={}",
                status_bounds.y,
                status_bounds.height
            );

            assert!(
                expand_bounds.y >= toolbar_bounds.y + toolbar_bounds.height - 0.1,
                "{label} pane Expand should start below toolbar"
            );
            assert!(
                status_bounds.y >= expand_bounds.y + expand_bounds.height - 0.1,
                "{label} pane StatusBar should start below Expand"
            );
        }
    }

    #[test]
    fn badge_renders_text_with_real_text_backend() {
        let mut tree = tree_with_real_text_backend();
        tree.add(Badge::new_literal("Badge text"));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        let frame = tree.render();
        assert!(
            !frame.glyphs.is_empty(),
            "badge should emit glyphs when rendered with the real text backend"
        );
    }

    #[test]
    fn catalog_missing_text_candidates_produce_bright_pixels_offscreen() {
        const TEST_HEIGHT: u32 = 2048;

        fn pixel_extrema(
            pixels: &[u8],
            width: u32,
            height: u32,
            bounds: Rect,
        ) -> ([u8; 3], [u8; 3]) {
            let x0 = bounds.x.floor().max(0.0) as u32;
            let y0 = bounds.y.floor().max(0.0) as u32;
            let x1 = (bounds.x + bounds.width).ceil().max(0.0) as u32;
            let y1 = (bounds.y + bounds.height).ceil().max(0.0) as u32;

            let mut best = [0u8; 3];
            let mut darkest = [255u8; 3];

            for y in y0.min(height)..y1.min(height) {
                for x in x0.min(width)..x1.min(width) {
                    let offset = ((y * width + x) * 4) as usize;
                    let r = pixels.get(offset).copied().unwrap_or(0);
                    let g = pixels.get(offset + 1).copied().unwrap_or(0);
                    let b = pixels.get(offset + 2).copied().unwrap_or(0);
                    if u16::from(r) + u16::from(g) + u16::from(b)
                        > u16::from(best[0]) + u16::from(best[1]) + u16::from(best[2])
                    {
                        best = [r, g, b];
                    }
                    if u16::from(r) + u16::from(g) + u16::from(b)
                        < u16::from(darkest[0]) + u16::from(darkest[1]) + u16::from(darkest[2])
                    {
                        darkest = [r, g, b];
                    }
                }
            }
            (best, darkest)
        }

        fn atlas_max_alpha(
            atlas: &[u8],
            atlas_width: u32,
            atlas_height: u32,
            rect: [f32; 4],
        ) -> u8 {
            let x0 = rect[0].floor().max(0.0) as u32;
            let y0 = rect[1].floor().max(0.0) as u32;
            let x1 = (rect[0] + rect[2]).ceil().max(0.0) as u32;
            let y1 = (rect[1] + rect[3]).ceil().max(0.0) as u32;

            let mut max_alpha = 0u8;
            for y in y0.min(atlas_height)..y1.min(atlas_height) {
                for x in x0.min(atlas_width)..x1.min(atlas_width) {
                    let offset = ((y * atlas_width + x) * 4 + 3) as usize;
                    max_alpha = max_alpha.max(atlas.get(offset).copied().unwrap_or(0));
                }
            }
            max_alpha
        }

        let Some((mut renderer, device, queue)) = pollster::block_on(
            test_support::create_test_renderer("widget_catalog_test_device"),
        ) else {
            return;
        };

        let (mut tree, typesetter) = tree_and_typesetter();
        tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(1600.0, TEST_HEIGHT as f32));
        let mut frame = tree.render();
        let atlas = typesetter.bridge().borrow_mut().atlas_info();
        renderer.upload_atlas(atlas.width, atlas.height, &atlas.pixels);
        if atlas.glyphs_evicted {
            tree.invalidate_all_paints();
            frame = tree.render();
            let atlas2 = typesetter.bridge().borrow_mut().atlas_info();
            renderer.upload_atlas(atlas2.width, atlas2.height, &atlas2.pixels);
        }

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("widget_catalog_test_target"),
            size: wgpu::Extent3d {
                width: 1600,
                height: TEST_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        renderer.render(
            &frame,
            &view,
            1.0,
            1600,
            TEST_HEIGHT,
            tree.theme().colors.surface_main.to_array(),
        );
        let pixels =
            test_support::read_texture_rgba(&device, &queue, &texture, 1600, TEST_HEIGHT);

        for label in ["Rust", "A1", "Standard"] {
            let id = tree
                .find_by_label(label)
                .unwrap_or_else(|| panic!("expected to find text node for {label:?}"));
            let bounds = tree.bounds(id);
            let matching_glyphs: Vec<_> = frame
                .glyphs
                .iter()
                .filter(|glyph| {
                    let [x, y, w, h] = glyph.screen;
                    let glyph_right = x + w;
                    let glyph_bottom = y + h;
                    glyph_right > bounds.x
                        && x < bounds.x + bounds.width
                        && glyph_bottom > bounds.y
                        && y < bounds.y + bounds.height
                })
                .map(|glyph| (glyph.screen, glyph.atlas, glyph.color))
                .collect();
            let (brightest, darkest) = pixel_extrema(&pixels, 1600, TEST_HEIGHT, bounds);
            let atlas_alpha: Vec<_> = matching_glyphs
                .iter()
                .map(|(_, atlas_rect, _)| {
                    atlas_max_alpha(&atlas.pixels, atlas.width, atlas.height, *atlas_rect)
                })
                .collect();
            assert!(
                brightest[0] > 170 && brightest[1] > 170 && brightest[2] > 170,
                "expected bright text pixels for {label:?} in {:?}, brightest was {:?}, darkest was {:?}, matching glyphs were {:?}, atlas alpha was {:?}, total glyphs {}, draw commands {}",
                bounds,
                brightest,
                darkest,
                matching_glyphs,
                atlas_alpha,
                frame.glyphs.len(),
                frame.draw_order.len()
            );
        }
    }
}
