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
use fern_ui::widgets::{
    Button, ButtonStyle, HStack, Padding, Panel, RectWidget, Spacer, TextWidget, VStack, ZStack,
};

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
struct RootContent;

impl CompositeWidget for RootContent {
    fn build(&self, ctx: &mut BuildContext) -> WidgetId {
        let theme = ctx.theme().clone();

        // --- Section: heading ---
        let heading = ctx.add(
            TextWidget::new("Text & Layout")
                .style(theme.typography.heading_2.clone())
                .color(theme.colors.on_surface),
        );

        // --- Section: theme toggle button ---
        let toggle_btn = ctx.add(
            Button::new("Toggle Dark Mode")
                .style(ButtonStyle::Outlined)
                .on_click(Cmd::ToggleDarkMode),
        );
        let spacer_top = ctx.add(Spacer::new());
        let toolbar = ctx.add(
            HStack::new()
                .add_child(heading)
                .add_child(spacer_top)
                .add_child(toggle_btn),
        );

        // --- Section: typography showcase ---
        let typo_title = ctx.add(
            TextWidget::new("Typography Styles")
                .style(theme.typography.heading_3.clone())
                .color(theme.colors.on_surface),
        );
        let body_text = ctx.add(
            TextWidget::new("Body text (14px) — the default reading style for content.")
                .style(theme.typography.body.clone())
                .color(theme.colors.on_surface),
        );
        let small_text = ctx.add(
            TextWidget::new("Body small (12px) — secondary information and descriptions.")
                .style(theme.typography.body_small.clone())
                .color(theme.colors.on_surface),
        );
        let caption_text = ctx.add(
            TextWidget::new("Caption (11px) — timestamps, footnotes, and fine print.")
                .style(theme.typography.caption.clone())
                .color(theme.colors.on_surface),
        );
        let label_text = ctx.add(
            TextWidget::new("LABEL (12px medium, +0.5 tracking) — form labels and tags.")
                .style(theme.typography.label.clone())
                .color(theme.colors.on_surface),
        );
        let typo_section = ctx.add(
            VStack::new()
                .spacing(6.0)
                .add_child(typo_title)
                .add_child(body_text)
                .add_child(small_text)
                .add_child(caption_text)
                .add_child(label_text),
        );

        // --- Section: layout showcase ---
        let layout_title = ctx.add(
            TextWidget::new("Layout Primitives")
                .style(theme.typography.heading_3.clone())
                .color(theme.colors.on_surface),
        );

        // HStack row with three colored boxes
        let box_a = build_color_box(ctx, theme.colors.primary, "A", &theme);
        let box_b = build_color_box(ctx, theme.colors.secondary, "B", &theme);
        let box_c = build_color_box(ctx, theme.colors.error, "C", &theme);
        let hstack_row = ctx.add(
            HStack::new()
                .spacing(8.0)
                .add_child(box_a)
                .add_child(box_b)
                .add_child(box_c),
        );
        let hstack_label = ctx.add(
            TextWidget::new("HStack with spacing — three colored boxes")
                .style(theme.typography.caption.clone())
                .color(theme.colors.on_surface),
        );

        // Spacer demo: [Left] --- [Right]
        let left_label = ctx.add(
            TextWidget::new("Leading")
                .style(theme.typography.body.clone())
                .color(theme.colors.on_surface),
        );
        let spacer_mid = ctx.add(Spacer::new());
        let right_label = ctx.add(
            TextWidget::new("Trailing")
                .style(theme.typography.body.clone())
                .color(theme.colors.on_surface),
        );
        let spacer_row = ctx.add(
            HStack::new()
                .add_child(left_label)
                .add_child(spacer_mid)
                .add_child(right_label),
        );
        let spacer_label = ctx.add(
            TextWidget::new("Spacer pushing items to edges")
                .style(theme.typography.caption.clone())
                .color(theme.colors.on_surface),
        );

        let layout_section = ctx.add(
            VStack::new()
                .spacing(6.0)
                .add_child(layout_title)
                .add_child(hstack_row)
                .add_child(hstack_label)
                .add_child(spacer_row)
                .add_child(spacer_label),
        );

        // --- Root: VStack of all sections with padding ---
        let content = ctx.add(
            VStack::new()
                .spacing(20.0)
                .add_child(toolbar)
                .add_child(typo_section)
                .add_child(layout_section),
        );

        ctx.add(Padding::uniform(24.0).set_child(content))
    }
}

fern_ui::core::impl_composite_into_widget_tree!(RootContent);

/// Helper: a small colored box with a centered label.
fn build_color_box(
    ctx: &mut BuildContext,
    color: Color,
    label: &str,
    _theme: &fern_ui::tokens::Theme,
) -> WidgetId {
    let text = ctx.add(
        TextWidget::new(label)
            .style(TextStyle {
                family: "sans-serif".into(),
                size: 14.0,
                weight: FontWeight::BOLD,
                line_height: 1.4,
                letter_spacing: 0.0,
            })
            .color(Color::WHITE),
    );
    ctx.add(
        Panel::new()
            .background(color)
            .corner_radius(6.0)
            .padding(8.0)
            .set_child(text),
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
        .root(|tree| tree.add_widget(RootContent))
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
        let _root = tree.add_widget(RootContent);
        tree.layout(SizeProposal::exact(600.0, 500.0));

        // Tab to the button (first focusable) and activate via Space
        tree.press_key(Key::Tab, Modifiers::NONE);
        tree.press_key(Key::Space, Modifiers::NONE);

        assert!(clicked.get(), "ToggleDarkMode command should have been emitted");
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
        let _root = tree.add_widget(RootContent);
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
        let _root = tree.add_widget(RootContent);
        tree.layout(SizeProposal::exact(600.0, 500.0));

        // Tab to button and click
        tree.press_key(Key::Tab, Modifiers::NONE);
        let btn_id = tree.focused().expect("button should be focused");
        tree.click(btn_id);

        // In the windowed app, commands are drained by the event loop
        let pending = tree.drain_pending_commands();
        eprintln!("Pending commands after click: {}", pending.len());
        assert!(!pending.is_empty(), "click should produce a pending command");

        // Verify it's the right command type
        let cmd = pending[0].downcast_ref::<Cmd>();
        assert!(cmd.is_some(), "command should be Cmd type");
        assert_eq!(cmd.unwrap(), &Cmd::ToggleDarkMode);
    }

    #[test]
    fn composite_rebuild_on_theme_change() {
        use super::RootContent;

        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let root = tree.add_widget(RootContent);
        tree.layout(SizeProposal::exact(600.0, 500.0));
        let frame_light = tree.render();

        // Switch to dark theme — triggers composite rebuild
        tree.set_theme(Theme::dark_default());
        tree.layout(SizeProposal::exact(600.0, 500.0));
        let frame_dark = tree.render();

        // The frames should differ (different colors)
        assert_ne!(frame_light.shapes, frame_dark.shapes,
            "theme switch should produce different render output");
    }
}
