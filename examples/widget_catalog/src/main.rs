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
    Accordion, Badge, Button, ButtonStyle, Card, CheckState, Checkbox, Divider, Grid, HStack,
    IconWidget, Link, MaxSize, Padding, Panel, ProgressBar, RadioButton, RectWidget,
    ScrollArea, SegmentedControl, Slider, Spacer, StatusBar, TextWidget, Toggle,
    Toolbar, TrackSize, VStack, Wrap,
};

// ---------------------------------------------------------------------------
// Application commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Cmd {
    ToggleDarkMode,
    LinkClicked,
}

impl AppCommand for Cmd {}

// ---------------------------------------------------------------------------
// Root composite
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct WidgetCatalog;

impl CompositeWidget for WidgetCatalog {
    fn build(&self, ctx: &mut BuildContext) -> WidgetId {
        let theme = ctx.theme().clone();
        let t = &theme.typography;
        let c = &theme.colors;
        let spacing = &theme.spacing;

        // --- Shared state ---
        let checkbox_checked = ctx.state(false);
        let tristate = ctx.state(CheckState::Unchecked);
        let radio_selected = ctx.state(0_usize);
        let toggle_on = ctx.state(false);
        let toggle_label_on = ctx.state(true);
        let slider_value = ctx.state(50.0_f32);
        let slider_v_value = ctx.state(0.3_f32);
        let slider_stepped = ctx.state(25.0_f32);
        let segment_selected = ctx.state(0_usize);
        let progress_value = ctx.state(0.65_f32);
        let accordion_expanded = ctx.state(false);
        let accordion2_expanded = ctx.state(true);

        // =====================================================================
        // Section 1: Primitives
        // =====================================================================

        let sec1_title = ctx.add(
            TextWidget::new("Primitives")
                .style(t.heading_2.clone())
                .color(c.on_surface),
        );

        // --- Divider ---
        let div_label = ctx.add(
            TextWidget::new("Divider (horizontal, vertical, thick, colored)")
                .style(t.caption.clone())
                .color(c.on_surface),
        );
        let div_row = ctx.add(
            HStack::new()
                .spacing(16.0)
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(TextWidget::new("H").style(t.caption.clone()).color(c.on_surface))
                        .child(Divider::new()),
                )
                .child(
                    HStack::new()
                        .spacing(4.0)
                        .child(TextWidget::new("V").style(t.caption.clone()).color(c.on_surface))
                        .child(Divider::vertical().thickness(2.0).color(c.primary)),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(TextWidget::new("Thick").style(t.caption.clone()).color(c.on_surface))
                        .child(Divider::new().thickness(4.0).color(c.error)),
                ),
        );

        // --- IconWidget ---
        let icon_label = ctx.add(
            TextWidget::new("IconWidget (checkmark, chevrons)")
                .style(t.caption.clone())
                .color(c.on_surface),
        );
        let icon_check = ctx.add(IconWidget::checkmark(24.0).color(c.primary));
        let icon_down = ctx.add(IconWidget::chevron_down(24.0).color(c.secondary));
        let icon_right = ctx.add(IconWidget::chevron_right(24.0).color(c.error));
        let icon_row = ctx.add(HStack::new().spacing(12.0).add_child(icon_check).add_child(icon_down).add_child(icon_right));

        let primitives_section = ctx.add(
            VStack::new()
                .spacing(8.0)
                .add_child(sec1_title)
                .add_child(div_label)
                .add_child(div_row)
                .add_child(icon_label)
                .add_child(icon_row),
        );

        // =====================================================================
        // Section 2: Layout Primitives
        // =====================================================================

        let sec2_title = ctx.add(
            TextWidget::new("Layout Primitives")
                .style(t.heading_2.clone())
                .color(c.on_surface),
        );

        // --- Grid ---
        let grid_label = ctx.add(
            TextWidget::new("Grid (Fixed 80px | 1fr | 2fr, with 8px gap)")
                .style(t.caption.clone())
                .color(c.on_surface),
        );
        let grid = ctx.add(
            Grid::new()
                .columns(vec![TrackSize::Fixed(80.0), TrackSize::Fractional(1.0), TrackSize::Fractional(2.0)])
                .rows(vec![TrackSize::Auto, TrackSize::Auto])
                .column_gap(8.0)
                .row_gap(8.0)
                .child(build_color_cell(c.primary, "A1"))
                .child(build_color_cell(c.secondary, "B1"))
                .child(build_color_cell(c.error, "C1"))
                .child(build_color_cell(c.info, "A2"))
                .child(build_color_cell(c.success, "B2"))
                .child(build_color_cell(c.warning, "C2")),
        );

        // --- Wrap ---
        let wrap_label = ctx.add(
            TextWidget::new("Wrap (flow layout, 8px spacing)")
                .style(t.caption.clone())
                .color(c.on_surface),
        );
        let wrap = ctx.add(
            Wrap::new()
                .spacing(8.0)
                .line_spacing(8.0)
                .child(Badge::new("Rust"))
                .child(Badge::new("GUI"))
                .child(Badge::new("Accessible"))
                .child(Badge::new("Reactive"))
                .child(Badge::new("Fast"))
                .child(Badge::new("Cross-platform"))
                .child(Badge::new("Retained"))
                .child(Badge::new("wgpu")),
        );

        let layout_section = ctx.add(
            VStack::new()
                .spacing(8.0)
                .add_child(sec2_title)
                .add_child(grid_label)
                .add_child(grid)
                .add_child(wrap_label)
                .add_child(wrap),
        );

        // =====================================================================
        // Section 3: Form Controls
        // =====================================================================

        let sec3_title = ctx.add(
            TextWidget::new("Form Controls")
                .style(t.heading_2.clone())
                .color(c.on_surface),
        );

        // --- Checkbox ---
        let cb1 = ctx.add(
            Checkbox::new(checkbox_checked.clone())
                .label("Accept terms")
                .tooltip("Click to accept the terms and conditions"),
        );
        let cb_disabled_state = ctx.state(true);
        let cb_disabled = ctx.add(
            Checkbox::new(cb_disabled_state)
                .label("Always on (disabled)")
                .enabled(false),
        );
        let cb_tristate = ctx.add(
            Checkbox::tristate(tristate.clone())
                .label("Select all (tristate)")
                .tooltip("Cycles: unchecked, checked, indeterminate"),
        );
        let checkbox_group = ctx.add(VStack::new().spacing(4.0).add_child(cb1).add_child(cb_disabled).add_child(cb_tristate));

        // --- RadioButton ---
        let rb0 = ctx.add(RadioButton::new(0, radio_selected.clone()).label("Option A").tooltip("First option"));
        let rb1 = ctx.add(RadioButton::new(1, radio_selected.clone()).label("Option B"));
        let rb2 = ctx.add(RadioButton::new(2, radio_selected.clone()).label("Option C"));
        let radio_group = ctx.add(VStack::new().spacing(4.0).add_child(rb0).add_child(rb1).add_child(rb2));

        // --- Toggle ---
        let toggle1 = ctx.add(Toggle::new(toggle_on.clone()));
        let toggle_labeled = ctx.add(Toggle::new(toggle_label_on.clone()).label("Notifications"));
        let toggle_disabled_state = ctx.state(false);
        let toggle_disabled = ctx.add(Toggle::new(toggle_disabled_state).enabled(false));
        let toggle_group = ctx.add(VStack::new().spacing(8.0).add_child(toggle1).add_child(toggle_labeled).add_child(toggle_disabled));

        // --- Slider ---
        let slider_h = ctx.add(Slider::new(slider_value.clone(), 0.0, 100.0));
        let slider_stepped_w = ctx.add(Slider::new(slider_stepped.clone(), 0.0, 100.0).step(25.0));
        let slider_disabled_state = ctx.state(30.0_f32);
        let slider_disabled = ctx.add(Slider::new(slider_disabled_state, 0.0, 100.0).enabled(false));
        let slider_vert = ctx.add(
            Slider::new(slider_v_value.clone(), 0.0, 1.0)
                .orientation(Orientation::Vertical),
        );
        let slider_vert_sized = ctx.add(MaxSize::new(f32::MAX, 120.0).set_child(slider_vert));
        let lbl_horizontal = ctx.add(TextWidget::new("Horizontal").style(t.caption.clone()).color(c.on_surface));
        let lbl_stepped = ctx.add(TextWidget::new("Stepped (25)").style(t.caption.clone()).color(c.on_surface));
        let lbl_disabled = ctx.add(TextWidget::new("Disabled").style(t.caption.clone()).color(c.on_surface));
        let lbl_vertical = ctx.add(TextWidget::new("Vertical").style(t.caption.clone()).color(c.on_surface));
        let slider_col = ctx.add(
            VStack::new()
                .spacing(8.0)
                .add_child(lbl_horizontal)
                .add_child(slider_h)
                .add_child(lbl_stepped)
                .add_child(slider_stepped_w)
                .add_child(lbl_disabled)
                .add_child(slider_disabled),
        );
        let vert_col = ctx.add(VStack::new().spacing(4.0).add_child(lbl_vertical).add_child(slider_vert_sized));
        let slider_section = ctx.add(
            HStack::new()
                .spacing(16.0)
                .add_child(slider_col)
                .add_child(vert_col),
        );

        // --- SegmentedControl ---
        let segmented = ctx.add(SegmentedControl::new(
            vec!["Day".into(), "Week".into(), "Month".into(), "Year".into()],
            segment_selected.clone(),
        ));

        // Layout: Checkboxes | Radios | Toggles in a grid
        let form_row = ctx.add(
            Grid::new()
                .columns(vec![TrackSize::Fractional(1.0), TrackSize::Fractional(1.0), TrackSize::Fractional(1.0)])
                .column_gap(16.0)
                .rows(vec![TrackSize::Auto])
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(TextWidget::new("Checkbox").style(t.label.clone()).color(c.on_surface))
                        .add_child(checkbox_group),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(TextWidget::new("RadioButton").style(t.label.clone()).color(c.on_surface))
                        .add_child(radio_group),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(TextWidget::new("Toggle").style(t.label.clone()).color(c.on_surface))
                        .add_child(toggle_group),
                ),
        );

        let ctrl_div1 = ctx.add(Divider::new());
        let ctrl_div2 = ctx.add(Divider::new());
        let lbl_slider = ctx.add(TextWidget::new("Slider").style(t.label.clone()).color(c.on_surface));
        let lbl_segmented = ctx.add(TextWidget::new("SegmentedControl").style(t.label.clone()).color(c.on_surface));
        let controls_section = ctx.add(
            VStack::new()
                .spacing(12.0)
                .add_child(sec3_title)
                .add_child(form_row)
                .add_child(ctrl_div1)
                .add_child(lbl_slider)
                .add_child(slider_section)
                .add_child(ctrl_div2)
                .add_child(lbl_segmented)
                .add_child(segmented),
        );

        // =====================================================================
        // Section 4: Display Widgets
        // =====================================================================

        let sec4_title = ctx.add(
            TextWidget::new("Display Widgets")
                .style(t.heading_2.clone())
                .color(c.on_surface),
        );

        // --- ProgressBar ---
        let pb_det = ctx.add(ProgressBar::new(0.65));
        let pb_indet = ctx.add(ProgressBar::indeterminate());
        let pb_custom = ctx.add(
            ProgressBar::new(0.4)
                .thickness(8.0)
                .fill_color(c.success)
                .track_color(c.surface_tertiary),
        );
        let pb_vert = ctx.add(
            ProgressBar::new(0.7)
                .orientation(Orientation::Vertical)
                .thickness(8.0),
        );
        let pb_vert_sized = ctx.add(MaxSize::new(f32::MAX, 80.0).set_child(pb_vert));

        let lbl_det = ctx.add(TextWidget::new("Determinate (65%)").style(t.caption.clone()).color(c.on_surface));
        let lbl_indet = ctx.add(TextWidget::new("Indeterminate").style(t.caption.clone()).color(c.on_surface));
        let lbl_custom = ctx.add(TextWidget::new("Custom colors + thick").style(t.caption.clone()).color(c.on_surface));
        let lbl_pv = ctx.add(TextWidget::new("Vertical").style(t.caption.clone()).color(c.on_surface));
        let progress_col = ctx.add(
            VStack::new()
                .spacing(8.0)
                .add_child(lbl_det)
                .add_child(pb_det)
                .add_child(lbl_indet)
                .add_child(pb_indet)
                .add_child(lbl_custom)
                .add_child(pb_custom),
        );
        let pv_col = ctx.add(VStack::new().spacing(4.0).add_child(lbl_pv).add_child(pb_vert_sized));
        let progress_section = ctx.add(
            HStack::new()
                .spacing(16.0)
                .add_child(progress_col)
                .add_child(pv_col),
        );

        // --- Badge ---
        let badge_row = ctx.add(
            HStack::new()
                .spacing(8.0)
                .child(Badge::new("Default"))
                .child(Badge::new("3").color(c.error).text_color(Color::WHITE))
                .child(Badge::new("New").color(c.success).text_color(Color::WHITE))
                .child(Badge::new("Beta").color(c.warning)),
        );

        let lbl_pb = ctx.add(TextWidget::new("ProgressBar").style(t.label.clone()).color(c.on_surface));
        let disp_div = ctx.add(Divider::new());
        let lbl_badge = ctx.add(TextWidget::new("Badge").style(t.label.clone()).color(c.on_surface));
        let display_section = ctx.add(
            VStack::new()
                .spacing(8.0)
                .add_child(sec4_title)
                .add_child(lbl_pb)
                .add_child(progress_section)
                .add_child(disp_div)
                .add_child(lbl_badge)
                .add_child(badge_row),
        );

        // =====================================================================
        // Section 5: Containers
        // =====================================================================

        let sec5_title = ctx.add(
            TextWidget::new("Containers")
                .style(t.heading_2.clone())
                .color(c.on_surface),
        );

        // --- Card ---
        let card = ctx.add(
            Card::new()
                .header(TextWidget::new("Card Header").style(t.label.clone()).color(c.on_surface))
                .content(
                    TextWidget::new("Card content with shadow and themed background.")
                        .style(t.body.clone())
                        .color(c.on_surface),
                )
                .footer(TextWidget::new("Footer text").style(t.caption.clone()).color(c.on_surface)),
        );

        // --- Accordion ---
        let acc_content1 = ctx.add(
            TextWidget::new("This content is revealed with an animated expand.")
                .style(t.body.clone())
                .color(c.on_surface),
        );
        let acc1 = ctx.add(
            Accordion::new("Click to expand", accordion_expanded.clone())
                .set_content(acc_content1),
        );
        let acc_content2 = ctx.add(
            TextWidget::new("This section starts expanded and can be collapsed.")
                .style(t.body.clone())
                .color(c.on_surface),
        );
        let acc2 = ctx.add(
            Accordion::new("Already expanded", accordion2_expanded.clone())
                .set_content(acc_content2),
        );

        let lbl_card = ctx.add(TextWidget::new("Card").style(t.label.clone()).color(c.on_surface));
        let cont_div = ctx.add(Divider::new());
        let lbl_acc = ctx.add(TextWidget::new("Accordion").style(t.label.clone()).color(c.on_surface));
        let containers_section = ctx.add(
            VStack::new()
                .spacing(8.0)
                .add_child(sec5_title)
                .add_child(lbl_card)
                .add_child(card)
                .add_child(cont_div)
                .add_child(lbl_acc)
                .add_child(acc1)
                .add_child(acc2),
        );

        // =====================================================================
        // Section 6: Navigation
        // =====================================================================

        let sec6_title = ctx.add(
            TextWidget::new("Navigation")
                .style(t.heading_2.clone())
                .color(c.on_surface),
        );

        let link1 = ctx.add(
            Link::new("Click me")
                .on_click(Cmd::LinkClicked)
                .tooltip("Fires the LinkClicked command"),
        );
        let link2 = ctx.add(
            Link::new("FernUI Documentation")
                .url("https://github.com/jacquetc/fern-ui"),
        );
        let link_row = ctx.add(HStack::new().spacing(16.0).add_child(link1).add_child(link2));

        let lbl_link = ctx.add(TextWidget::new("Link").style(t.label.clone()).color(c.on_surface));
        let nav_section = ctx.add(
            VStack::new()
                .spacing(8.0)
                .add_child(sec6_title)
                .add_child(lbl_link)
                .add_child(link_row),
        );

        // =====================================================================
        // Assemble all sections
        // =====================================================================

        // Toolbar at top
        let theme_btn = ctx.add(
            Button::new("Toggle Dark Mode")
                .style(ButtonStyle::Outlined)
                .on_click(Cmd::ToggleDarkMode),
        );
        let toolbar_row = ctx.add(
            HStack::new()
                .child(TextWidget::new("Widget Catalog").style(t.heading_1.clone()).color(c.on_surface))
                .child(Spacer::new())
                .add_child(theme_btn),
        );
        let toolbar = ctx.add(Toolbar::new().add_child(toolbar_row));

        // Status bar at bottom
        let status_text = ctx.add(
            TextWidget::new("Milestone 3 -- All widgets demonstrated")
                .style(t.caption.clone())
                .color(c.on_surface),
        );
        let status = ctx.add(StatusBar::new().add_child(status_text));

        // Main content
        let div1 = ctx.add(Divider::new().thickness(2.0));
        let div2 = ctx.add(Divider::new().thickness(2.0));
        let div3 = ctx.add(Divider::new().thickness(2.0));
        let div4 = ctx.add(Divider::new().thickness(2.0));
        let div5 = ctx.add(Divider::new().thickness(2.0));

        let content_col = ctx.add(
            VStack::new()
                .spacing(24.0)
                .add_child(primitives_section)
                .add_child(div1)
                .add_child(layout_section)
                .add_child(div2)
                .add_child(controls_section)
                .add_child(div3)
                .add_child(display_section)
                .add_child(div4)
                .add_child(containers_section)
                .add_child(div5)
                .add_child(nav_section),
        );
        let padded = ctx.add(Padding::uniform(24.0).set_child(content_col));
        let scroll = ctx.add(ScrollArea::from_id(padded));

        // Root: Toolbar | ScrollArea | StatusBar
        ctx.add(
            VStack::new()
                .add_child(toolbar)
                .add_child(scroll)
                .add_child(status),
        )
    }
}

fern_ui::core::impl_composite_into_widget_tree!(WidgetCatalog);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_color_cell(color: Color, label: &str) -> Panel {
    Panel::new()
        .background(color)
        .corner_radius(4.0)
        .padding(8.0)
        .child(
            TextWidget::new(label)
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
        })
        .root(|tree| tree.add_widget(WidgetCatalog))
        .run();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use fern_ui::core::WidgetTree;
    use fern_ui::prelude::*;
    use fern_ui::widgets::*;

    use super::WidgetCatalog;

    #[test]
    fn catalog_builds_and_layouts() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let root = tree.add_widget(WidgetCatalog);
        tree.layout(SizeProposal::exact(900.0, 700.0));
        let b = tree.bounds(root);
        assert!(b.width > 0.0);
        assert!(b.height > 0.0);
    }

    #[test]
    fn catalog_renders_without_crash() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add_widget(WidgetCatalog);
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
        tree.add_widget(WidgetCatalog);
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
}
