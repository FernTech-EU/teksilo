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
    Accordion, Badge, Button, ButtonStyle, Card, CheckState, Checkbox, Divider, Expand, Grid,
    HStack, IconWidget, Link, MaxSize, Padding, Panel, ProgressBar, RadioButton, ScrollArea,
    SegmentedControl, Slider, Spacer, StatusBar, TextWidget, Toggle, Toolbar, TrackSize, VStack,
    Wrap,
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
                            TextWidget::new("H")
                                .style(t.caption.clone())
                                .color(c.on_surface),
                        )
                        .child(Divider::new()),
                )
                .child(
                    HStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new("V")
                                .style(t.caption.clone())
                                .color(c.on_surface),
                        )
                        .child(Divider::vertical().thickness(2.0).color(c.primary)),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new("Thick")
                                .style(t.caption.clone())
                                .color(c.on_surface),
                        )
                        .child(Divider::new().thickness(4.0).color(c.error)),
                ),
        );

        // --- IconWidget ---
        let icon_row = ctx.add(
            HStack::new()
                .spacing(12.0)
                .child(IconWidget::checkmark(24.0).color(c.primary))
                .child(IconWidget::chevron_down(24.0).color(c.secondary))
                .child(IconWidget::chevron_right(24.0).color(c.error)),
        );

        let primitives_section = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new("Primitives")
                        .style(t.heading_2.clone())
                        .color(c.on_surface),
                )
                .child(
                    TextWidget::new("Divider (horizontal, vertical, thick, colored)")
                        .style(t.caption.clone())
                        .color(c.on_surface),
                )
                .add_child(div_row)
                .child(
                    TextWidget::new("IconWidget (checkmark, chevrons)")
                        .style(t.caption.clone())
                        .color(c.on_surface),
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
                    TextWidget::new("Layout Primitives")
                        .style(t.heading_2.clone())
                        .color(c.on_surface),
                )
                .child(
                    TextWidget::new("Grid (Fixed 80px | 1fr | 2fr, with 8px gap)")
                        .style(t.caption.clone())
                        .color(c.on_surface),
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
                        .child(build_color_cell(c.primary, "A1"))
                        .child(build_color_cell(c.secondary, "B1"))
                        .child(build_color_cell(c.error, "C1"))
                        .child(build_color_cell(c.info, "A2"))
                        .child(build_color_cell(c.success, "B2"))
                        .child(build_color_cell(c.warning, "C2")),
                )
                .child(
                    TextWidget::new("Wrap (flow layout, 8px spacing)")
                        .style(t.caption.clone())
                        .color(c.on_surface),
                )
                .child(
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
                ),
        );

        // =====================================================================
        // Section 3: Form Controls
        // =====================================================================

        // --- Checkbox ---
        let cb_disabled_state = ctx.signal(true);
        let checkbox_group = ctx.add(
            VStack::new()
                .spacing(4.0)
                .child(
                    Checkbox::new(checkbox_checked.clone())
                        .label("Accept terms")
                        .tooltip("Click to accept the terms and conditions"),
                )
                .child(
                    Checkbox::new(cb_disabled_state)
                        .label("Always on (disabled)")
                        .enabled(false),
                )
                .child(
                    Checkbox::tristate(tristate.clone())
                        .label("Select all (tristate)")
                        .tooltip("Cycles: unchecked, checked, indeterminate"),
                ),
        );

        // --- RadioButton ---
        let radio_group = ctx.add(
            VStack::new()
                .spacing(4.0)
                .child(
                    RadioButton::new(0, radio_selected.clone())
                        .label("Option A")
                        .tooltip("First option"),
                )
                .child(RadioButton::new(1, radio_selected.clone()).label("Option B"))
                .child(RadioButton::new(2, radio_selected.clone()).label("Option C")),
        );

        // --- Toggle ---
        let toggle_disabled_state = ctx.signal(false);
        let toggle_group = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(Toggle::new(toggle_on.clone()))
                .child(Toggle::new(toggle_label_on.clone()).label("Notifications"))
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
                            TextWidget::new("Horizontal")
                                .style(t.caption.clone())
                                .color(c.on_surface),
                        )
                        .child(Slider::new(slider_value.clone(), 0.0, 100.0))
                        .child(
                            TextWidget::new("Stepped (25)")
                                .style(t.caption.clone())
                                .color(c.on_surface),
                        )
                        .child(Slider::new(slider_stepped.clone(), 0.0, 100.0).step(25.0))
                        .child(
                            TextWidget::new("Disabled")
                                .style(t.caption.clone())
                                .color(c.on_surface),
                        )
                        .child(Slider::new(slider_disabled_state, 0.0, 100.0).enabled(false)),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new("Vertical")
                                .style(t.caption.clone())
                                .color(c.on_surface),
                        )
                        .child(MaxSize::new(f32::MAX, 120.0).set_child(slider_vert)),
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
                            TextWidget::new("Checkbox")
                                .style(t.label.clone())
                                .color(c.on_surface),
                        )
                        .add_child(checkbox_group),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new("RadioButton")
                                .style(t.label.clone())
                                .color(c.on_surface),
                        )
                        .add_child(radio_group),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new("Toggle")
                                .style(t.label.clone())
                                .color(c.on_surface),
                        )
                        .add_child(toggle_group),
                ),
        );

        let controls_section = ctx.add(
            VStack::new()
                .spacing(12.0)
                .child(
                    TextWidget::new("Form Controls")
                        .style(t.heading_2.clone())
                        .color(c.on_surface),
                )
                .add_child(form_row)
                .child(Divider::new())
                .child(
                    TextWidget::new("Slider")
                        .style(t.label.clone())
                        .color(c.on_surface),
                )
                .add_child(slider_section)
                .child(Divider::new())
                .child(
                    TextWidget::new("SegmentedControl")
                        .style(t.label.clone())
                        .color(c.on_surface),
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
                            TextWidget::new("Determinate (65%)")
                                .style(t.caption.clone())
                                .color(c.on_surface),
                        )
                        .child(ProgressBar::new(0.65))
                        .child(
                            TextWidget::new("Indeterminate")
                                .style(t.caption.clone())
                                .color(c.on_surface),
                        )
                        .child(ProgressBar::indeterminate())
                        .child(
                            TextWidget::new("Custom colors + thick")
                                .style(t.caption.clone())
                                .color(c.on_surface),
                        )
                        .child(
                            ProgressBar::new(0.4)
                                .thickness(8.0)
                                .fill_color(c.success)
                                .track_color(c.surface_tertiary),
                        ),
                )
                .child(
                    VStack::new()
                        .spacing(4.0)
                        .child(
                            TextWidget::new("Vertical")
                                .style(t.caption.clone())
                                .color(c.on_surface),
                        )
                        .child(MaxSize::new(f32::MAX, 80.0).set_child(pb_vert)),
                ),
        );

        let display_section = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new("Display Widgets")
                        .style(t.heading_2.clone())
                        .color(c.on_surface),
                )
                .child(
                    TextWidget::new("ProgressBar")
                        .style(t.label.clone())
                        .color(c.on_surface),
                )
                .add_child(progress_section)
                .child(Divider::new())
                .child(
                    TextWidget::new("Badge")
                        .style(t.label.clone())
                        .color(c.on_surface),
                )
                .child(
                    HStack::new()
                        .spacing(8.0)
                        .child(Badge::new("Default"))
                        .child(Badge::new("3").color(c.error).text_color(Color::WHITE))
                        .child(Badge::new("New").color(c.success).text_color(Color::WHITE))
                        .child(Badge::new("Beta").color(c.warning)),
                ),
        );

        // =====================================================================
        // Section 5: Containers
        // =====================================================================

        // --- Accordion (needs pre-registered content children) ---
        let acc_content1 = ctx.add(
            TextWidget::new("This content is revealed with an animated expand.")
                .style(t.body.clone())
                .color(c.on_surface),
        );
        let acc_content2 = ctx.add(
            TextWidget::new("This section starts expanded and can be collapsed.")
                .style(t.body.clone())
                .color(c.on_surface),
        );

        let containers_section = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new("Containers")
                        .style(t.heading_2.clone())
                        .color(c.on_surface),
                )
                .child(
                    TextWidget::new("Card")
                        .style(t.label.clone())
                        .color(c.on_surface),
                )
                .child(
                    Card::new()
                        .header(
                            TextWidget::new("Card Header")
                                .style(t.label.clone())
                                .color(c.on_surface),
                        )
                        .content(
                            TextWidget::new("Card content with shadow and themed background.")
                                .style(t.body.clone())
                                .color(c.on_surface),
                        )
                        .footer(
                            TextWidget::new("Footer text")
                                .style(t.caption.clone())
                                .color(c.on_surface),
                        ),
                )
                .child(Divider::new())
                .child(
                    TextWidget::new("Accordion")
                        .style(t.label.clone())
                        .color(c.on_surface),
                )
                .child(
                    Accordion::new("Click to expand", accordion_expanded.clone())
                        .set_content(acc_content1),
                )
                .child(
                    Accordion::new("Already expanded", accordion2_expanded.clone())
                        .set_content(acc_content2),
                ),
        );

        // =====================================================================
        // Section 6: Navigation
        // =====================================================================

        let nav_section = ctx.add(
            VStack::new()
                .spacing(8.0)
                .child(
                    TextWidget::new("Navigation")
                        .style(t.heading_2.clone())
                        .color(c.on_surface),
                )
                .child(
                    TextWidget::new("Link")
                        .style(t.label.clone())
                        .color(c.on_surface),
                )
                .child(
                    HStack::new()
                        .spacing(16.0)
                        .child(
                            Link::new("Click me")
                                .on_click(Cmd::LinkClicked)
                                .tooltip("Fires the LinkClicked command"),
                        )
                        .child(
                            Link::new("FernUI Documentation")
                                .url("https://github.com/jacquetc/fern-ui"),
                        ),
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
                        TextWidget::new("Widget Catalog")
                            .style(t.heading_1.clone())
                            .color(c.on_surface),
                    )
                    .child(Spacer::new())
                    .child(
                        Button::new("Toggle Dark Mode")
                            .style(ButtonStyle::Outlined)
                            .on_click(Cmd::ToggleDarkMode),
                    ),
            ),
        );

        // Main content
        let content_col = ctx.add(
            VStack::new()
                .spacing(24.0)
                .add_child(primitives_section)
                .child(Divider::new().thickness(2.0))
                .add_child(layout_section)
                .child(Divider::new().thickness(2.0))
                .add_child(controls_section)
                .child(Divider::new().thickness(2.0))
                .add_child(display_section)
                .child(Divider::new().thickness(2.0))
                .add_child(containers_section)
                .child(Divider::new().thickness(2.0))
                .add_child(nav_section),
        );
        let padded = ctx.add(Padding::uniform(24.0).set_child(content_col));
        let scroll = ctx.add(ScrollArea::from_id(padded));

        // Root: Toolbar | ScrollArea (fills remaining space) | StatusBar
        let root = ctx.add(
            VStack::new()
                .add_child(toolbar)
                .child(Expand::new().fills_stack().set_child(scroll))
                .child(
                    StatusBar::new().child(
                        TextWidget::new("Milestone 3 -- All widgets demonstrated")
                            .style(t.caption.clone())
                            .color(c.on_surface),
                    ),
                ),
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
        tree.add(Badge::new("Badge text"));
        tree.layout(SizeProposal::exact(200.0, 80.0));

        let frame = tree.render();
        assert!(
            !frame.glyphs.is_empty(),
            "badge should emit glyphs when rendered with the real text backend"
        );
    }

    #[test]
    fn catalog_missing_text_candidates_produce_bright_pixels_offscreen() {
        fn pixel_extrema(pixels: &[u8], width: u32, bounds: Rect) -> ([u8; 3], [u8; 3]) {
            let x0 = bounds.x.floor().max(0.0) as u32;
            let y0 = bounds.y.floor().max(0.0) as u32;
            let x1 = (bounds.x + bounds.width).ceil().max(0.0) as u32;
            let y1 = (bounds.y + bounds.height).ceil().max(0.0) as u32;

            let mut best = [0u8; 3];
            let mut darkest = [255u8; 3];

            for y in y0.min(700)..y1.min(700) {
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
        tree.layout(SizeProposal::exact(900.0, 700.0));
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
                height: 700,
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
            700,
            tree.theme().colors.surface.to_array(),
        );
        let pixels = test_support::read_texture_rgba(&device, &queue, &texture, 900, 700);

        for label in ["Rust", "A1", "Option A"] {
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
            let (brightest, darkest) = pixel_extrema(&pixels, 900, bounds);
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
