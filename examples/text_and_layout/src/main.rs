// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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

use bastyde::prelude::*;
use bastyde::tokens::{FontWeight, TextStyle};
use bastyde::widgets::{Button, ButtonVariant, HStack, Padding, Panel, Spacer, TextWidget, VStack};

// ---------------------------------------------------------------------------
// Application commands
// ---------------------------------------------------------------------------

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
        // Snapshot typography (static tokens not expressible as a role);
        // colors use roles and resolve reactively at paint time.

        let root = ctx.add(
            Padding::uniform(24.0).child(
                VStack::new()
                    .spacing(20.0)
                    // Toolbar
                    .child(
                        HStack::new()
                            .child(
                                TextWidget::new(lit!("Text & Layout"))
                                    .style(TextStyleRole::BodyBold)
                                    .color(TextRole::Primary),
                            )
                            .child(Spacer::new())
                            .child({
                                let is_dark = std::rc::Rc::new(std::cell::Cell::new(false));
                                Button::new(lit!("Toggle Dark Mode"))
                                    .variant(ButtonVariant::Plain)
                                    .on_activate_fn(move |ctx: &mut EventContext| {
                                        let next = !is_dark.get();
                                        is_dark.set(next);
                                        ctx.set_theme(if next {
                                            bastyde::presets::intui::dark()
                                        } else {
                                            bastyde::presets::intui::light()
                                        });
                                    })
                            }),
                    )
                    // Typography showcase
                    .child(
                        VStack::new()
                            .spacing(6.0)
                            .child(
                                TextWidget::new(lit!("Typography Styles"))
                                    .style(TextStyleRole::BodyBold)
                                    .color(TextRole::Primary),
                            )
                            .child(
                                TextWidget::new(lit!(
                                    "Body text (14px) — the default reading style for content."
                                ))
                                .style(TextStyleRole::Body)
                                .color(TextRole::Primary),
                            )
                            .child(
                                TextWidget::new(lit!(
                                    "Body small (12px) — secondary information and descriptions."
                                ))
                                .style(TextStyleRole::Small)
                                .color(TextRole::Primary),
                            )
                            .child(
                                TextWidget::new(lit!(
                                    "Caption (11px) — timestamps, footnotes, and fine print."
                                ))
                                .style(TextStyleRole::Tiny)
                                .color(TextRole::Primary),
                            )
                            .child(
                                TextWidget::new(lit!(
                                    "LABEL (12px medium, +0.5 tracking) — form labels and tags."
                                ))
                                .style(TextStyleRole::Small)
                                .color(TextRole::Primary),
                            ),
                    )
                    // Layout showcase
                    .child(
                        VStack::new()
                            .spacing(6.0)
                            .child(
                                TextWidget::new(lit!("Layout Primitives"))
                                    .style(TextStyleRole::BodyBold)
                                    .color(TextRole::Primary),
                            )
                            .child(
                                HStack::new()
                                    .spacing(8.0)
                                    .child(build_color_box(SurfaceRole::Accent, "A"))
                                    .child(build_color_box(SurfaceRole::AccentSubtle, "B"))
                                    .child(build_color_box(TextRole::Error, "C")),
                            )
                            .child(
                                TextWidget::new(lit!("HStack with spacing — three colored boxes"))
                                    .style(TextStyleRole::Tiny)
                                    .color(TextRole::Primary),
                            )
                            .child(
                                HStack::new()
                                    .child(
                                        TextWidget::new(lit!("Leading"))
                                            .style(TextStyleRole::Body)
                                            .color(TextRole::Primary),
                                    )
                                    .child(Spacer::new())
                                    .child(
                                        TextWidget::new(lit!("Trailing"))
                                            .style(TextStyleRole::Body)
                                            .color(TextRole::Primary),
                                    ),
                            )
                            .child(
                                TextWidget::new(lit!("Spacer pushing items to edges"))
                                    .style(TextStyleRole::Tiny)
                                    .color(TextRole::Primary),
                            ),
                    ),
            ),
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_size(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0)),
            None => proposal.resolve(0.0, 0.0),
        }
        .into()
    }
}

/// Helper — returns a widget value, not a WidgetId.
/// Works with the inline child() pattern.
fn build_color_box(color: impl Into<bastyde::core::ColorProp>, label: &str) -> Panel {
    Panel::new()
        .background(color)
        .corner_radius(6.0)
        .padding(8.0)
        .child(
            TextWidget::new(lit!(label))
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
    BastydeAppBuilder::new()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Text & Layout")
                .size(600, 500)
                .root(|tree, _state| tree.add(RootContent::new())),
        )
        .run();
}

// ---------------------------------------------------------------------------
// Tests — headless layout and theme validation
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use bastyde::core::WidgetTree;
    use bastyde::prelude::*;
    use bastyde::widgets::{HStack, Spacer, TextWidget, VStack};

    #[derive(Debug)]
    struct FixedLeaf(f32, f32);
    impl Widget for FixedLeaf {
        fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            Size::new(self.0, self.1).into()
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
        let theme = bastyde::presets::intui::light();
        let w = TextWidget::new(lit!("Hello World")).style(theme.typography.body.clone());
        let ctx = LayoutContext::for_testing(&theme);
        let size = w.layout_response(SizeProposal::unspecified(), &ctx).size;
        assert!((size.width - 88.0).abs() < 0.01);
        assert!(size.height > 0.0);
    }

    #[test]
    fn theme_swap_changes_color_tokens() {
        let light = bastyde::presets::intui::light();
        let dark = bastyde::presets::intui::dark();
        assert_ne!(
            light.colors.surface_main.to_array(),
            dark.colors.surface_main.to_array()
        );
        assert_ne!(
            light.colors.text_primary.to_array(),
            dark.colors.text_primary.to_array()
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
    fn composite_rebuild_on_theme_change() {
        use super::RootContent;

        let mut tree = WidgetTree::new().with_theme(bastyde::presets::intui::light());
        let _root = tree.add(RootContent::new());
        tree.layout(SizeProposal::exact(600.0, 500.0));
        let frame_light = tree.render();

        // Switch to dark theme — triggers composite rebuild
        tree.set_theme(bastyde::presets::intui::dark());
        tree.layout(SizeProposal::exact(600.0, 500.0));
        let frame_dark = tree.render();

        // The frames should differ (different colors)
        assert_ne!(
            frame_light.shapes, frame_dark.shapes,
            "theme switch should produce different render output"
        );
    }
}
