//! First Milestone Demo: a single button in a window.
//!
//! Run with: `cargo run -p simple-button`

use fern_ui::prelude::*;
use fern_ui::widgets::{Button, ButtonVariant};

fn main() {
    FernAppBuilder::new()
        .theme(Theme::light_default())
        .install_inspector_in_debug()
        .initial_window(
            WindowConfig::new()
                .title("FernUI — Simple Button")
                .size(400, 300)
                .root(|tree, _state| {
                    tree.add(
                        Button::new_literal("Click Me")
                            .style(ButtonVariant::Default)
                            .on_activate_fn(|_ctx| {
                                println!("Button clicked!");
                            })
                            .tooltip_literal("This is a simple button. Click it to see a message in the console."),
                    )
                }),
        )
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use fern_ui::core::WidgetTree;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn button_has_correct_accessibility() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let button = tree.add(Button::new_literal("Click Me").on_activate_fn(|_| {}));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let info = tree.accessibility_node(button);
        assert_eq!(info.role(), fern_ui::core::accesskit::Role::Button);
        assert_eq!(info.name(), Some("Click Me"));
    }

    #[test]
    fn button_click_invokes_handler() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let clicked = Rc::new(Cell::new(false));
        let c = clicked.clone();
        let button = tree.add(Button::new_literal("Click Me").on_activate_fn(move |_| c.set(true)));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.click(button);
        assert!(clicked.get());
    }

    #[test]
    fn keyboard_activates_button() {
        let mut tree = WidgetTree::new().with_theme(Theme::light_default());
        let clicked = Rc::new(Cell::new(false));
        let c = clicked.clone();
        let button = tree.add(Button::new_literal("Click Me").on_activate_fn(move |_| c.set(true)));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.focus(button);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert!(clicked.get());
    }
}
