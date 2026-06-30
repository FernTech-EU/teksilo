// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! First Milestone Demo: a single button in a window.
//!
//! Run with: `cargo run -p simple-button`

use bastyde::prelude::*;
use bastyde::widgets::{Button, ButtonVariant};

fn main() {
    BastydeAppBuilder::new()
        .install_automation_bridge_in_debug()
        .theme(bastyde::presets::intui::light())
        .install_inspector_in_debug()
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Simple Button")
                .size(400, 300)
                .root(|tree, _state| {
                    tree.add(
                        Button::new(lit!("Click Me"))
                            .variant(ButtonVariant::Filled)
                            .on_activate_fn(|_ctx| {
                                println!("Button clicked!");
                            })
                            .tooltip(lit!(
                                "This is a simple button. Click it to see a message in the console."
                            )),
                    )
                }),
        )
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use bastyde::core::WidgetTree;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    fn button_has_correct_accessibility() {
        let mut tree = WidgetTree::new().with_theme(bastyde::presets::intui::light());
        let button = tree.add(Button::new(lit!("Click Me")).on_activate_fn(|_| {}));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        let info = tree.accessibility_node(button);
        assert_eq!(info.role(), bastyde::core::accesskit::Role::Button);
        assert_eq!(info.name(), Some("Click Me"));
    }

    #[test]
    fn button_click_invokes_handler() {
        let mut tree = WidgetTree::new().with_theme(bastyde::presets::intui::light());
        let clicked = Rc::new(Cell::new(false));
        let c = clicked.clone();
        let button = tree.add(Button::new(lit!("Click Me")).on_activate_fn(move |_| c.set(true)));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.click(button);
        assert!(clicked.get());
    }

    #[test]
    fn keyboard_activates_button() {
        let mut tree = WidgetTree::new().with_theme(bastyde::presets::intui::light());
        let clicked = Rc::new(Cell::new(false));
        let c = clicked.clone();
        let button = tree.add(Button::new(lit!("Click Me")).on_activate_fn(move |_| c.set(true)));
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.focus(button);
        tree.press_key(Key::Space, Modifiers::NONE);
        assert!(clicked.get());
    }
}
