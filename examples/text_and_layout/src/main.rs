//! Milestone 2: Text and Layout Fundamentals
//!
//! A window with multiple widgets arranged in nested layouts, demonstrating
//! the layout engine, text rendering, and theme switching.
//!
//! Run with: `cargo run -p text-and-layout`
//!
//! Demonstrates:
//! - HStack, VStack, ZStack, Padding, Spacer layout primitives
//! - TextWidget with different TextStyle tokens (heading, body, caption)
//! - Theme switching at runtime (light/dark toggle via a button)
//! - Nested HStack-in-VStack arrangements
//! - Composite widget rebuild on theme change

use std::cell::Cell;
use std::rc::Rc;

use fern_ui::prelude::*;
use fern_ui::tokens::{FontWeight, TextStyle};
use fern_ui::widgets::{Button, ButtonStyle, HStack, Padding, Panel, Spacer, TextWidget, VStack};

// ---------------------------------------------------------------------------
// Application commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Cmd {
    ToggleDarkMode,
}

impl AppCommand for Cmd {}

// ---------------------------------------------------------------------------
// Root composite — rebuilds on theme change
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct RootContent {
    root_child_id: Option<WidgetId>,
}

impl RootContent {
    fn new() -> Self {
        Self {
            root_child_id: None,
        }
    }
}

impl Widget for RootContent {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let theme = ctx.theme().clone();
        let t = &theme.typography;
        let c = &theme.colors;

        let root = ctx.add(
            Padding::uniform(24.0).child(
                VStack::new()
                    .spacing(20.0)
                    // Toolbar
                    .child(
                        HStack::new()
                            .child(
                                TextWidget::new("Text & Layout")
                                    .style(t.heading_2.clone())
                                    .color(c.on_surface),
                            )
                            .child(Spacer::new())
                            .child(
                                Button::new("Toggle Dark Mode")
                                    .style(ButtonStyle::Outlined)
                                    .on_activate(Cmd::ToggleDarkMode),
                            ),
                    )
                    // Typography showcase
                    .child(
                        VStack::new()
                            .spacing(6.0)
                            .child(
                                TextWidget::new("Typography Styles")
                                    .style(t.heading_3.clone())
                                    .color(c.on_surface),
                            )
                            .child(
                                TextWidget::new(
                                    "Body text (14px) — the default reading style for content.",
                                )
                                .style(t.body.clone())
                                .color(c.on_surface),
                            )
                            .child(
                                TextWidget::new(
                                    "Body small (12px) — secondary information and descriptions.",
                                )
                                .style(t.body_small.clone())
                                .color(c.on_surface),
                            )
                            .child(
                                TextWidget::new(
                                    "Caption (11px) — timestamps, footnotes, and fine print.",
                                )
                                .style(t.caption.clone())
                                .color(c.on_surface),
                            )
                            .child(
                                TextWidget::new(
                                    "LABEL (12px medium, +0.5 tracking) — form labels and tags.",
                                )
                                .style(t.label.clone())
                                .color(c.on_surface),
                            ),
                    )
                    // Layout showcase
                    .child(
                        VStack::new()
                            .spacing(6.0)
                            .child(
                                TextWidget::new("Layout Primitives")
                                    .style(t.heading_3.clone())
                                    .color(c.on_surface),
                            )
                            .child(
                                HStack::new()
                                    .spacing(8.0)
                                    .child(build_color_box(c.primary, "A"))
                                    .child(build_color_box(c.secondary, "B"))
                                    .child(build_color_box(c.error, "C")),
                            )
                            .child(
                                TextWidget::new("HStack with spacing — three colored boxes")
                                    .style(t.caption.clone())
                                    .color(c.on_surface),
                            )
                            .child(
                                HStack::new()
                                    .child(
                                        TextWidget::new("Leading")
                                            .style(t.body.clone())
                                            .color(c.on_surface),
                                    )
                                    .child(Spacer::new())
                                    .child(
                                        TextWidget::new("Trailing")
                                            .style(t.body.clone())
                                            .color(c.on_surface),
                                    ),
                            )
                            .child(
                                TextWidget::new("Spacer pushing items to edges")
                                    .style(t.caption.clone())
                                    .color(c.on_surface),
                            ),
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

/// Helper — returns a widget value, not a WidgetId.
/// Works with the inline child() pattern.
fn build_color_box(color: Color, label: &str) -> Panel {
    Panel::new()
        .background(color)
        .corner_radius(6.0)
        .padding(8.0)
        .child(
            TextWidget::new(label)
                .style(TextStyle {
                    family: "sans-serif".into(),
                    size: 14.0,
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
        .window_title("FernUI — Text & Layout")
        .window_size(600, 500)
        .on_command(move |cmd: &Cmd, ctx| match cmd {
            Cmd::ToggleDarkMode => {
                let dark = !is_dark_clone.get();
                is_dark_clone.set(dark);
                println!("Theme: {}", if dark { "dark" } else { "light" });
                if dark {
                    ctx.set_theme(Theme::dark_default());
                } else {
                    ctx.set_theme(Theme::light_default());
                }
            }
        })
        .root(|tree| tree.add(RootContent::new()))
        .run();
}

// ---------------------------------------------------------------------------
// Tests — headless layout and theme validation
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use fern_ui::core::WidgetTree;
    use fern_ui::prelude::*;
    use fern_ui::widgets::{HStack, Spacer, TextWidget, VStack};

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn size_that_fits(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
            Size::new(self.0, self.1)
        }
    }

    #[test]
    fn nested_hstack_in_vstack_produces_correct_positions() {
        let mut tree = WidgetTree::new();
        let a = tree.add(FixedLeaf(60.0, 25.0));
        let b = tree.add(FixedLeaf(40.0, 25.0));
        let row = tree.add(HStack::new().spacing(5.0).add_child(a).add_child(b));
        let c = tree.add(FixedLeaf(80.0, 30.0));
        let _col = tree.add(VStack::new().spacing(10.0).add_child(row).add_child(c));
        tree.layout(SizeProposal::exact(400.0, 300.0));

        assert!((tree.bounds(a).width - 60.0).abs() < 0.01);
        assert!((tree.bounds(b).x - 65.0).abs() < 0.01);
        assert!((tree.bounds(c).y - 35.0).abs() < 0.01);
    }

    #[test]
    fn text_widget_measures_correctly_without_backend() {
        let theme = Theme::light_default();
        let w = TextWidget::new("Hello World").style(theme.typography.body.clone());
        let ctx = LayoutContext::for_testing(&theme);
        let size = w.size_that_fits(SizeProposal::unspecified(), &ctx);
        assert!((size.width - 88.0).abs() < 0.01);
        assert!(size.height > 0.0);
    }

    #[test]
    fn theme_swap_changes_color_tokens() {
        let light = Theme::light_default();
        let dark = Theme::dark_default();
        assert_ne!(
            light.colors.surface.to_array(),
            dark.colors.surface.to_array()
        );
        assert_ne!(
            light.colors.on_surface.to_array(),
            dark.colors.on_surface.to_array()
        );
    }

    #[test]
    fn spacer_pushes_widgets_apart_in_hstack() {
        let mut tree = WidgetTree::new();
        let left = tree.add(FixedLeaf(50.0, 20.0));
        let spacer = tree.add(Spacer::new());
        let right = tree.add(FixedLeaf(50.0, 20.0));
        let _row = tree.add(
            HStack::new()
                .add_child(left)
                .add_child(spacer)
                .add_child(right),
        );
        tree.layout(SizeProposal::exact(300.0, 40.0));

        assert!((tree.bounds(left).x - 0.0).abs() < 0.01);
        assert!((tree.bounds(right).x - 250.0).abs() < 0.01);
    }

    #[test]
    fn button_keyboard_activates_in_composite() {
        use super::{Cmd, RootContent};
        use std::cell::Cell;
        use std::rc::Rc;

        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let clicked = Rc::new(Cell::new(false));
        let c = clicked.clone();
        tree.on_command(move |cmd: &Cmd| {
            if matches!(cmd, Cmd::ToggleDarkMode) {
                c.set(true);
            }
        });
        let _root = tree.add(RootContent::new());
        tree.layout(SizeProposal::exact(600.0, 500.0));

        // Tab to the button (first focusable) and activate via Space
        tree.press_key(Key::Tab, Modifiers::NONE);
        tree.press_key(Key::Space, Modifiers::NONE);

        assert!(
            clicked.get(),
            "ToggleDarkMode command should have been emitted"
        );
    }

    #[test]
    fn button_pointer_click_in_composite() {
        use super::{Cmd, RootContent};
        use std::cell::Cell;
        use std::rc::Rc;

        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let clicked = Rc::new(Cell::new(false));
        let c = clicked.clone();
        tree.on_command(move |cmd: &Cmd| {
            if matches!(cmd, Cmd::ToggleDarkMode) {
                c.set(true);
            }
        });
        let _root = tree.add(RootContent::new());
        tree.layout(SizeProposal::exact(600.0, 500.0));

        // Tab to button to discover its ID, then click it
        tree.press_key(Key::Tab, Modifiers::NONE);
        let focused = tree.focused();
        assert!(focused.is_some(), "should have focused the button");

        let btn_id = focused.unwrap();
        let bounds = tree.bounds(btn_id);
        eprintln!("Button bounds: {:?}", bounds);
        tree.click(btn_id);

        assert!(clicked.get(), "pointer click should emit ToggleDarkMode");
    }

    #[test]
    fn button_click_produces_pending_command() {
        // Simulates the windowed app path: commands go to drain_pending_commands,
        // NOT through tree.on_command (which is the headless test path).
        use super::{Cmd, RootContent};

        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let _root = tree.add(RootContent::new());
        tree.layout(SizeProposal::exact(600.0, 500.0));

        // Tab to button and click
        tree.press_key(Key::Tab, Modifiers::NONE);
        let btn_id = tree.focused().expect("button should be focused");
        tree.click(btn_id);

        // In the windowed app, commands are drained by the event loop
        let pending = tree.drain_pending_commands();
        eprintln!("Pending commands after click: {}", pending.len());
        assert!(
            !pending.is_empty(),
            "click should produce a pending command"
        );

        // Verify it's the right command type
        let cmd = pending[0].downcast_ref::<Cmd>();
        assert!(cmd.is_some(), "command should be Cmd type");
        assert_eq!(cmd.unwrap(), &Cmd::ToggleDarkMode);
    }

    #[test]
    fn composite_rebuild_on_theme_change() {
        use super::RootContent;

        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let root = tree.add(RootContent::new());
        tree.layout(SizeProposal::exact(600.0, 500.0));
        let frame_light = tree.render();

        // Switch to dark theme — triggers composite rebuild
        tree.set_theme(Theme::dark_default());
        tree.layout(SizeProposal::exact(600.0, 500.0));
        let frame_dark = tree.render();

        // The frames should differ (different colors)
        assert_ne!(
            frame_light.shapes, frame_dark.shapes,
            "theme switch should produce different render output"
        );
    }
}
