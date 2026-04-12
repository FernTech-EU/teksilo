//! First Milestone Demo: a single button in a window.
//!
//! Run with: `cargo run -p simple-button`
//!
//! Uses the production-quality `Button` widget from fern-widgets with:
//! - Themed rounded rectangle background
//! - 5 visual states (Idle, Hovered, Pressed, Focused, Disabled)
//! - 4 styles (Filled, Outlined, Flat, Tonal)
//! - Typed command emission on click and keyboard activation
//! - Full accessibility (Role::Button, name, actions)
//! - Focus-visible behavior (focus ring only on keyboard focus)

use fern_ui::prelude::*;
use fern_ui::widgets::{Button, ButtonVariant, tooltip};

// ---------------------------------------------------------------------------
// Application command
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum DemoCmd {
    ButtonClicked,
}

impl AppCommand for DemoCmd {}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() {
    FernAppBuilder::new()
        .theme(Theme::light_default())
        .window_title("FernUI — Simple Button")
        .window_size(400, 300)
        .on_command(|cmd: &DemoCmd, _ctx| match cmd {
            DemoCmd::ButtonClicked => {
                println!("Button clicked!");
            }
        })
        .root(|tree| {
            tree.add(
                Button::new("Click Me")
                    .style(ButtonVariant::Default)
                    .on_activate(DemoCmd::ButtonClicked)
                    .tooltip("This is a simple button. Click it to see a message in the console."),
            )
        })
        .run();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use fern_ui::core::WidgetTree;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn button_has_correct_accessibility() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let button = tree.add(Button::new("Click Me").on_activate(DemoCmd::ButtonClicked));
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let info = tree.accessibility_node(button);
        assert_eq!(info.role(), fern_ui::core::accesskit::Role::Button);
        assert_eq!(info.name(), Some("Click Me"));
        assert!(
            info.actions()
                .contains(&fern_ui::core::accesskit::Action::Click)
        );
    }

    #[test]
    fn button_click_emits_command() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let clicked = Rc::new(Cell::new(false));
        let c = clicked.clone();
        tree.on_command(move |cmd: &DemoCmd| {
            if matches!(cmd, DemoCmd::ButtonClicked) {
                c.set(true);
            }
        });
        let button = tree.add(Button::new("Click Me").on_activate(DemoCmd::ButtonClicked));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.click(button);
        assert!(clicked.get());
    }

    #[test]
    fn button_hover_changes_render_output() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let button = tree.add(Button::new("Click Me").on_activate(DemoCmd::ButtonClicked));
        tree.layout(SizeProposal::exact(400.0, 300.0));

        let frame_idle = tree.render();
        let idle_color = frame_idle.shapes[0].color;

        let center = tree.bounds(button).center();
        tree.pointer_move(center);
        tree.mark_needs_paint(button);
        let frame_hovered = tree.render();
        let hover_color = frame_hovered.shapes[0].color;

        assert_ne!(idle_color, hover_color, "color should change on hover");
    }

    #[test]
    fn button_renders_shape() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        tree.add(Button::new("Click Me").on_activate(DemoCmd::ButtonClicked));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let frame = tree.render();
        assert!(!frame.shapes.is_empty(), "button should render a shape");
    }

    #[test]
    fn keyboard_activates_button() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let clicked = Rc::new(Cell::new(false));
        let c = clicked.clone();
        tree.on_command(move |cmd: &DemoCmd| {
            if matches!(cmd, DemoCmd::ButtonClicked) {
                c.set(true);
            }
        });
        let button = tree.add(Button::new("Click Me").on_activate(DemoCmd::ButtonClicked));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.focus(button);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert!(clicked.get());
    }
}
