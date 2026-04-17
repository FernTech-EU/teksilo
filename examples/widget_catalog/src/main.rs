//! Milestone 3: Widget Catalog
//!
//! A window showcasing every Milestone 3 widget with all their options.
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
//! - Theme switching (light/dark)

use std::cell::Cell;
use std::rc::Rc;

use fern_ui::prelude::*;
use fern_ui::tokens::{FontWeight, Orientation, TextStyle};
use fern_ui::widgets::{
    Accordion, Badge, BuiltInButton, BuiltInButtonSize, Button, ButtonVariant, Card, CheckState,
    Checkbox, ComboBox, Divider, Expand, FixedSize, Grid, GroupBox, GroupHeader, HStack,
    IconLocation, IconWidget, ImageFit, ImageWidget, Link, MaxSize, MenuItem, MenuList, Padding, Panel, ProgressBar,
    RadioButton, ScrollArea, SegmentedControl, Slider, Spacer, SplitButton, StatusBar, TabItem,
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
    Cancel,
    LearnMore,
    Run,
    RunTests,
    RunCoverage,
    Debug,
}

impl AppCommand for Cmd {}

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

        // --- Shared signals ---
        let checkbox_checked = ctx.signal(false);
        let tristate = ctx.signal(CheckState::Unchecked);
        let radio_selected = ctx.signal(0_usize);
        let toggle_on = ctx.signal(false);
        let toggle_label_on = ctx.signal(true);
        let slider_value = ctx.signal(50.0_f32);
        let slider_v_value = ctx.signal(0.3_f32);
        let slider_stepped = ctx.signal(25.0_f32);
        let segment_selected = ctx.signal(0_usize);
        let accordion_expanded = ctx.signal(false);
        let accordion2_expanded = ctx.signal(true);
        let group_box_notifications_on = ctx.signal(true);

        // =====================================================================
        // Section 0: Theme Palette
        //
        // A visual reference for every surface and text role in the theme.
        // Side-by-side swatches make it obvious when a widget is using the
        // wrong color role for a given surface (e.g. text_primary on a
        // caption label, or surface_main where surface_content is expected).
        // =====================================================================

        // Helper: one surface swatch — a colored box containing the surface
        // token name rendered in the text role that normally pairs with it,
        // with the role name as a caption below. This makes it immediately
        // visible whether the semantically-correct combination actually
        // reads cleanly (e.g. `text_primary` on `surface_main`, or
        // `selection_text_active` on `surface_selected`).
        let surface_swatch = |bg: Color,
                              name: &str,
                              text_role: &str,
                              text_color: Color|
         -> VStack {
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
        };

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
                .child(surface_swatch(
                    c.surface_main,
                    "surface_main",
                    "text_primary",
                    c.text_primary,
                ))
                .child(surface_swatch(
                    c.surface_content,
                    "surface_content",
                    "text_primary",
                    c.text_primary,
                ))
                .child(surface_swatch(
                    c.surface_raised,
                    "surface_raised",
                    "text_primary",
                    c.text_primary,
                ))
                .child(surface_swatch(
                    c.surface_sunken,
                    "surface_sunken",
                    "text_secondary",
                    c.text_secondary,
                ))
                .child(surface_swatch(
                    c.surface_hover,
                    "surface_hover",
                    "text_primary",
                    c.text_primary,
                ))
                .child(surface_swatch(
                    c.surface_pressed,
                    "surface_pressed",
                    "text_primary",
                    c.text_primary,
                ))
                .child(surface_swatch(
                    c.surface_selected,
                    "surface_selected",
                    "selection_text_active",
                    c.selection_text_active,
                ))
                .child(surface_swatch(
                    c.surface_selected_inactive,
                    "surface_selected_inactive",
                    "selection_text_inactive",
                    c.selection_text_inactive,
                )),
        );

        // Text samples: rendered on surface_main so contrast matches real
        // usage. Each row shows the label in its actual color + the role
        // name in text_secondary on the right.
        let text_sample = |name: &str, color: Color, description: &str| -> HStack {
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
        };

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
                        .child(text_sample("text_primary", c.text_primary, "body, main labels"))
                        .child(text_sample(
                            "text_secondary",
                            c.text_secondary,
                            "hints, captions, placeholders",
                        ))
                        .child(text_sample(
                            "text_disabled",
                            c.text_disabled,
                            "disabled labels",
                        ))
                        .child(text_sample("text_link", c.text_link, "hyperlinks"))
                        .child(text_sample(
                            "text_error",
                            c.text_error,
                            "validation errors",
                        ))
                        .child(text_sample(
                            "text_warning",
                            c.text_warning,
                            "validation warnings",
                        ))
                        .child(text_sample(
                            "text_success",
                            c.text_success,
                            "success messages",
                        )),
                ),
        );

        // text_on_accent needs its own row because it's drawn on an accent
        // background, not on surface_main. Show it inline on an accent fill.
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

        // =====================================================================
        // Editor palette
        //
        // The editor is architecturally a separate color scheme layered on
        // top of the UI theme — IntelliJ / CLion treat it the same way. The
        // foreground is intentionally dimmer than `text_primary`, the
        // selection is more saturated than the list-row selection, and
        // `editor_current_line_bg` has no UI equivalent.
        //
        // This section renders a small mock editor using the real
        // `theme.colors.editor_*` tokens, plus a row of named swatches.
        // =====================================================================

        // Mock editor — a Panel filled with `editor_bg`, containing a
        // faux gutter (line numbers in `editor_gutter_fg`) and four code
        // lines drawn in `editor_fg`. Line 2 is wrapped in a full-width
        // ZStack of `editor_current_line_bg` to stand in for the caret-row
        // highlight. A small `editor_selection_bg` rectangle after "let" on
        // line 2 hints at what a text selection looks like, and a thin
        // `editor_caret` rectangle stands in for the blinking caret.
        let mono = t.mono.clone();
        let editor_line = |line_no: &str, code: &str| -> HStack {
            HStack::new()
                .spacing(12.0)
                .child(
                    FixedSize::new()
                        .bind_width(24.0_f32)
                        .child(
                            TextWidget::new_literal(line_no)
                                .style(mono.clone())
                                .color(c.editor_gutter_fg),
                        ),
                )
                .child(
                    TextWidget::new_literal(code)
                        .style(mono.clone())
                        .color(c.editor_fg),
                )
        };

        // Line 2 is the "current" (caret) line — it gets the full-width
        // background highlight and carries a selection marker + caret.
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
                // `selection_bg` stand-in: a soft rectangle under a word.
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
                // Caret — a 1 dp tall vertical rectangle in editor_caret.
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
                        .child(editor_line("1", "fn main() {"))
                        .add_child(current_line_bg)
                        .child(editor_line("3", "    println!(\"{}\", x);"))
                        .child(editor_line("4", "}")),
                ),
        );

        // Individual editor swatches, same format as the surface grid.
        // Each swatch's interior text uses the text role that semantically
        // belongs on it (editor_fg for the backgrounds, editor_bg for the
        // foregrounds).
        let editor_swatch = |bg: Color,
                             name: &str,
                             sample_color: Color|
         -> VStack {
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
        };

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
                .child(editor_swatch(c.editor_bg, "editor_bg", c.editor_fg))
                .child(editor_swatch(c.editor_fg, "editor_fg", c.editor_bg))
                .child(editor_swatch(
                    c.editor_caret,
                    "editor_caret",
                    c.editor_bg,
                ))
                .child(editor_swatch(
                    c.editor_current_line_bg,
                    "editor_current_line_bg",
                    c.editor_fg,
                ))
                .child(editor_swatch(
                    c.editor_gutter_fg,
                    "editor_gutter_fg",
                    c.editor_bg,
                ))
                .child(editor_swatch(
                    c.editor_selection_bg,
                    "editor_selection_bg",
                    c.editor_fg,
                )),
        );

        let palette_section = ctx.add(
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
        );

        // =====================================================================
        // Section 1: Primitives
        // =====================================================================

        // --- Divider ---
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

        // --- IconWidget ---
        let icon_row = ctx.add(
            HStack::new()
                .spacing(12.0)
                .child(IconWidget::checkmark(24.0).color(c.accent))
                .child(IconWidget::chevron_down(24.0).color(c.accent_subtle_bg))
                .child(IconWidget::chevron_right(24.0).color(c.text_error)),
        );

        let primitives_section = ctx.add(
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
        );

        // =====================================================================
        // Section 2: Layout Primitives
        // =====================================================================

        let layout_section = ctx.add(
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
        );

        // =====================================================================
        // Section 3: Form Controls
        // =====================================================================

        // --- Button variants (Int UI: Default, Regular, Flat) ---
        //
        // One row of enabled buttons followed by one row of disabled
        // buttons, so the three silhouettes and their disabled-state
        // rendering can be compared side-by-side.
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
        // --- Icon buttons: demonstrate icons from SVG with all IconLocation variants.
        //
        // Primary path: res!() macro — compile-time validated, lazy-decoded.
        // SVG icons (tintable — color follows button theme state)
        let save_icon = fern_ui::res!("resources/icons/save.svg");
        let home_icon = fern_ui::res!("resources/icons/home.svg");
        // PNG icon (tintable — white-on-transparent, used as alpha mask)
        let star_icon = fern_ui::res!("resources/icons/star.png");
        // WebP icon (tintable)
        let clock_icon = fern_ui::res!("resources/icons/clock.webp");
        //
        // Alternative: include_str! + from_svg (no compile-time validation,
        // parses every time — kept here for reference).
        // let save_svg = include_str!("../resources/icons/save.svg");
        // IconWidget::from_svg(save_svg)
        let icon_buttons_row = ctx.add(
            HStack::new()
                .spacing(8.0)
                // SVG, leading
                .child(
                    Button::new_literal("Save")
                        .icon(
                            IconWidget::from_svg_icon(save_icon),
                            IconLocation::Leading,
                        )
                        .style(ButtonVariant::Default)
                        .on_activate(Cmd::Save),
                )
                // SVG, leading
                .child(
                    Button::new_literal("Home")
                        .icon(
                            IconWidget::from_svg_icon(home_icon),
                            IconLocation::Leading,
                        )
                        .style(ButtonVariant::Regular)
                        .on_activate(Cmd::Cancel),
                )
                // PNG, leading
                .child(
                    Button::new_literal("Star")
                        .icon(
                            IconWidget::from_raster(star_icon, 24.0),
                            IconLocation::Leading,
                        )
                        .style(ButtonVariant::Regular)
                        .on_activate(Cmd::Save),
                )
                // WebP, leading
                .child(
                    Button::new_literal("Clock")
                        .icon(
                            IconWidget::from_raster(clock_icon, 24.0),
                            IconLocation::Leading,
                        )
                        .style(ButtonVariant::Regular)
                        .on_activate(Cmd::Cancel),
                )
                // SVG, icon-only
                .child(
                    Button::new_literal("Save")
                        .icon(
                            IconWidget::from_svg_icon(save_icon),
                            IconLocation::IconOnly,
                        )
                        .style(ButtonVariant::Flat)
                        .on_activate(Cmd::Save),
                )
                // Built-in programmatic, icon-only
                .child(
                    Button::new_literal("")
                        .icon(
                            IconWidget::chevron_down(16.0),
                            IconLocation::IconOnly,
                        )
                        .style(ButtonVariant::Regular)
                        .on_activate(Cmd::Cancel),
                ),
        );

        // --- SplitButton: default action on the left, chevron dropdown on the
        //     right. Reuses MenuItem directly for the dropdown rows so icons,
        //     shortcut labels, per-item tooltips and separators come for free.
        //
        // First control: promoting mode — picking an item from the dropdown
        // updates the main region's label and default action.
        // Second control: static mode — main region stays pinned to "Save"
        // even after you pick "Save As…".
        // Third control: disabled.
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

        // --- Checkbox ---
        let cb_disabled_state = ctx.signal(true);
        let cb_sounds = ctx.signal(true);
        let checkbox_group = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    Checkbox::new(checkbox_checked.clone())
                        .label_literal("Accept terms")
                        .tooltip_literal("Click to accept the terms and conditions"),
                )
                .child(
                    Checkbox::new(cb_sounds)
                        .label_literal("Play notification sounds")
                        .caption_literal(
                            "Play a short chime when a new message arrives. \
                             Muted automatically while you're in a call.",
                        ),
                )
                .child(
                    Checkbox::new(cb_disabled_state)
                        .label_literal("Always on (disabled)")
                        .enabled(false),
                )
                .child(
                    Checkbox::tristate(tristate.clone())
                        .label_literal("Select all (tristate)")
                        .caption_literal("Cycles unchecked → checked → indeterminate")
                        .tooltip_literal("Cycles: unchecked, checked, indeterminate"),
                ),
        );

        // --- RadioButton ---
        let radio_group = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    RadioButton::new(0, radio_selected.clone())
                        .label_literal("Standard")
                        .caption_literal("Recommended for most users — sensible defaults.")
                        .tooltip_literal("First option"),
                )
                .child(
                    RadioButton::new(1, radio_selected.clone())
                        .label_literal("Advanced")
                        .caption_literal(
                            "Expose every option, including experimental features \
                             that may change between releases.",
                        ),
                )
                .child(RadioButton::new(2, radio_selected.clone()).label_literal("Custom")),
        );

        // --- Toggle ---
        let toggle_disabled_state = ctx.signal(false);
        let toggle_group = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(Toggle::new(toggle_on.clone()))
                .child(Toggle::new(toggle_label_on.clone()).label_literal("Notifications"))
                .child(Toggle::new(toggle_disabled_state).enabled(false)),
        );

        // --- Slider ---
        let slider_disabled_state = ctx.signal(30.0_f32);
        let slider_vert = ctx
            .add(Slider::new(slider_v_value.clone(), 0.0, 1.0).orientation(Orientation::Vertical));
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
                        .child(Slider::new(slider_value.clone(), 0.0, 100.0))
                        .child(
                            TextWidget::new_literal("Stepped (25)")
                                .style(t.tiny.clone())
                                .color(c.text_secondary),
                        )
                        .child(Slider::new(slider_stepped.clone(), 0.0, 100.0).step(25.0))
                        .child(
                            TextWidget::new_literal("Disabled")
                                .style(t.tiny.clone())
                                .color(c.text_secondary),
                        )
                        .child(Slider::new(slider_disabled_state, 0.0, 100.0).enabled(false)),
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

        // Layout: Checkboxes | Radios | Toggles in a grid
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

        let controls_section = ctx.add(
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
                    segment_selected.clone(),
                )),
        );

        // =====================================================================
        // Section 4: Display Widgets
        // =====================================================================

        // --- ProgressBar ---
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

        let display_section = ctx.add(
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
        );

        // =====================================================================
        // Section 4.5: Text overflow modes
        //
        // TextWidget defaults to wrap; other modes are opt-in via
        // .overflow(TextOverflow::...) / .single_line().
        // =====================================================================

        const LOREM: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing \
                             elit. Sed do eiusmod tempor incididunt ut labore \
                             et dolore magna aliqua. Ut enim ad minim veniam, \
                             quis nostrud exercitation ullamco laboris nisi ut \
                             aliquip ex ea commodo consequat.";

        const LONG_TITLE: &str =
            "A somewhat verbose section title that almost certainly will not fit";

        let text_overflow_section = ctx.add(
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
                    // Paragraph wraps inside a narrow FixedSize column so the
                    // effect is obvious regardless of window size.
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
        );

        // =====================================================================
        // Section 5: Containers
        // =====================================================================

        // --- Accordion (needs pre-registered content children) ---
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

        let containers_section = ctx.add(
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
                    Accordion::new_literal("Click to expand", accordion_expanded.clone())
                        .content_id(acc_content1),
                )
                .child(
                    Accordion::new_literal("Already expanded", accordion2_expanded.clone())
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
                        .checkable(group_box_notifications_on.clone())
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
        );

        // =====================================================================
        // Section 6: Navigation
        // =====================================================================

        // TabWidget demo — three tabs, a trailing flat button slot, and
        // enough intrinsic content to prove the tab bar stays on top and
        // the content doesn't bleed over it. The whole thing is wrapped
        // in a FixedSize so the catalog's scrolling column gives it a
        // concrete height instead of asking for intrinsic size.
        let tabs_selected = ctx.signal(0_usize);
        let tabs = ctx.add(
            TabWidget::new(tabs_selected)
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

        let nav_section = ctx.add(
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
        );

        // =====================================================================
        // Section 6.5: Rich Tooltips
        //
        // Hover any of the buttons below for ~500ms to see a registered
        // tooltip appear. Body text supports inline markup
        // (`[label](url)`, `*italic*`, `**bold**`); links to `:key` URLs
        // open nested tooltips, and links to `https://` URLs hand off
        // to the OS default browser via the `open` crate. Shortcut
        // chips and "more" long-form bodies are pulled from each
        // entry's `TooltipContent` registered at app boot.
        // =====================================================================

        let rich_tooltips_section = ctx.add(
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
        );

        // =====================================================================
        // Section 7: Menus & Dropdowns (Milestone 4)
        // =====================================================================

        let combo_selected = ctx.signal(None::<usize>);
        let menus_section = ctx.add(
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
                        ComboBox::new_literal(
                            vec!["Apple", "Banana", "Cherry", "Date", "Elderberry"],
                            combo_selected.clone(),
                        )
                        .placeholder_literal("Select a fruit..."),
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
        );

        // =====================================================================
        // Section 8: ImageWidget
        // =====================================================================

        let tree_img = fern_ui::res!("resources/icons/tree.webp");
        let image_section = ctx.add(
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
        );

        // =====================================================================
        // Section 9: Built-in Buttons
        // =====================================================================

        let visibility_signal = ctx.signal(false);
        let vis_label = visibility_signal.map(|v| if *v { "Visible".to_string() } else { "Hidden".to_string() });

        let builtin_section = ctx.add(
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
                        .child(BuiltInButton::visibility_toggle(visibility_signal))
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
        );

        // --- Text Input ---
        let search_text = ctx.signal(String::new());
        let username_text = ctx.signal("cyril".to_string());
        let readonly_text = ctx.signal("Read-only value".to_string());

        let text_input_section = ctx.add(
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
                    TextInput::new(search_text)
                        .placeholder("Search...")
                        .show_clear_button(true)
                        .leading_slot(
                            IconWidget::checkmark(14.0).color(c.text_secondary),
                        ),
                )
                .child(
                    TextInput::new(username_text)
                        .placeholder("Username")
                        .label("Username")
                        .trailing_slot(
                            BuiltInButton::browse().on_activate(Cmd::Save),
                        ),
                )
                .child(
                    TextInput::new(readonly_text)
                        .read_only(true),
                ),
        );

        // =====================================================================
        // Assemble all sections
        // =====================================================================

        // Toolbar at top
        let toolbar = ctx.add(
            Toolbar::new().child(
                HStack::new()
                    .child(
                        TextWidget::new_literal("Widget Catalog")
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

        // Main content. Section dividers use the Int UI default 1 dp
        // (no thickness override) — the 24 dp VStack spacing above carries
        // the visual separation; the divider is a single hairline, as in
        // IntelliJ's Settings / Preferences panels.
        let content_col = ctx.add(
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
        let padded = ctx.add(Padding::uniform(24.0).child_id(content_col));
        let scroll = ctx.add(ScrollArea::from_id(padded));

        // Root: Toolbar | ScrollArea (fills remaining space) | StatusBar.
        // Migrated to the fern! DSL per spec §7.8. Expand is a
        // single-child wrapper (uses `.child_id(id)`, not
        // `.add_child`) so `scroll` goes through a property call;
        // VStack and StatusBar are multi-child containers and take
        // `.add_child(id)` via the `#{ }` id-escape.
        let root = fern!(ctx =>
            VStack {
                #{ toolbar }
                Expand {
                    fills_stack
                    child_id: scroll
                }
                StatusBar {
                    TextWidget::new_literal("Milestone 4 -- All widgets demonstrated") {
                        style: t.tiny.clone()
                        color: c.text_secondary
                    }
                }
            }
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
// Helpers
// ---------------------------------------------------------------------------

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
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    let is_dark = Rc::new(Cell::new(false));
    let is_dark_clone = is_dark.clone();

    FernAppBuilder::new()
        .theme(Theme::light_default())
        .window_title("FernUI -- Widget Catalog (Milestone 3)")
        .window_size(900, 700)
        .register_tooltips(vec![
            // Plain registered entry — body is a single line, no markup.
            TooltipContent::new(
                "save-as",
                LocalizedString::literal(
                    "Save the current file under a new name",
                ),
            )
            .with_shortcut_label("Ctrl+Shift+S"),

            // Rich body with *italic*, **bold**, an `https://` link
            // (delegated to the OS via the `open` crate on click) and a
            // nested `:key` link that opens another tooltip.
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

            // Nested entry reachable from `autosave`'s body via the
            // `:autosave-details` link.
            TooltipContent::new(
                "autosave-details",
                LocalizedString::literal(
                    "Autosave runs on a debounced timer: it only writes \
                     to disk after typing pauses for 500ms.",
                ),
            ),

            // Nested entry reachable from `autosave`'s "more" body via
            // the `:prefs-general` link.
            TooltipContent::new(
                "prefs-general",
                LocalizedString::literal(
                    "Open Preferences → General to toggle autosave and \
                     change its interval.",
                ),
            ),

            // Inline-content path: bound with a shortcut hint and a
            // trailing "more" disclosure.
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
        tree.layout(SizeProposal::exact(900.0, 700.0));
        let b = tree.bounds(root);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn catalog_renders_without_crash() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(900.0, 700.0));
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
        tree.layout(SizeProposal::exact(900.0, 700.0));
        let frame_light = tree.render();

        tree.set_theme(Theme::dark_default());
        tree.layout(SizeProposal::exact(900.0, 700.0));
        let frame_dark = tree.render();

        assert_ne!(
            frame_light.shapes, frame_dark.shapes,
            "theme switch should produce different output"
        );
    }

    /// Regression test: second layout+render at same proposal must produce
    /// the same number of draw commands as the first.
    #[test]
    fn second_render_same_output() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(900.0, 700.0));
        let frame1 = tree.render();
        let cmds1 = frame1.draw_order.len();

        // Second pass without resize — should be identical
        tree.layout(SizeProposal::exact(900.0, 700.0));
        let frame2 = tree.render();
        let cmds2 = frame2.draw_order.len();

        assert_eq!(
            cmds1, cmds2,
            "second render must produce same draw commands (got {cmds1} vs {cmds2})"
        );
    }

    /// Regression test: after set_theme, render must still contain draw commands.
    #[test]
    fn theme_switch_preserves_draw_commands() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(900.0, 700.0));
        let frame1 = tree.render();
        let cmds_before = frame1.draw_order.len();

        tree.set_theme(Theme::dark_default());
        tree.layout(SizeProposal::exact(900.0, 700.0));
        let frame2 = tree.render();
        let cmds_after = frame2.draw_order.len();

        // After theme switch we expect roughly the same number of commands
        assert!(
            cmds_after > cmds_before / 2,
            "draw commands after theme switch ({cmds_after}) should be \
             close to before ({cmds_before})"
        );
    }

    /// Verify the ScrollArea fills the space between toolbar and status bar.
    #[test]
    fn scroll_area_fills_remaining_space() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let root = tree.add(WidgetCatalog::new());
        tree.layout(SizeProposal::exact(900.0, 700.0));

        // root is WidgetCatalog → VStack → [Toolbar, Expand, StatusBar]
        let vstack_children = {
            let adapter_children = tree.children(root);
            assert_eq!(
                adapter_children.len(),
                1,
                "composite adapter has one child (VStack)"
            );
            tree.children(adapter_children[0])
        };
        assert_eq!(
            vstack_children.len(),
            3,
            "VStack must have toolbar, expand, statusbar"
        );

        let toolbar_bounds = tree.bounds(vstack_children[0]);
        let expand_bounds = tree.bounds(vstack_children[1]);
        let status_bounds = tree.bounds(vstack_children[2]);

        // Expand (containing ScrollArea) should fill remaining space
        assert!(
            expand_bounds.height > 400.0,
            "Expand wrapping ScrollArea should fill >400px, got {}",
            expand_bounds.height
        );

        // StatusBar should be at the bottom
        assert!(
            (status_bounds.y + status_bounds.height - 700.0).abs() < 1.0,
            "StatusBar should reach bottom of window: y={}, h={}",
            status_bounds.y,
            status_bounds.height
        );

        // No overlap
        assert!(
            expand_bounds.y >= toolbar_bounds.y + toolbar_bounds.height - 0.1,
            "Expand should start below toolbar"
        );
        assert!(
            status_bounds.y >= expand_bounds.y + expand_bounds.height - 0.1,
            "StatusBar should start below Expand"
        );
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
        // Render at the max 2 K texture size (2048 rows) instead of 700 so
        // the labels this test probes ("Rust", "A1", "Standard") still fit
        // in the captured viewport regardless of how many sections are
        // stacked above them. wgpu's default max texture dimension is 2048.
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
        tree.layout(SizeProposal::exact(900.0, TEST_HEIGHT as f32));
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
                width: 900,
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
            900,
            TEST_HEIGHT,
            tree.theme().colors.surface_main.to_array(),
        );
        let pixels =
            test_support::read_texture_rgba(&device, &queue, &texture, 900, TEST_HEIGHT);

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
            let (brightest, darkest) = pixel_extrema(&pixels, 900, TEST_HEIGHT, bounds);
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
