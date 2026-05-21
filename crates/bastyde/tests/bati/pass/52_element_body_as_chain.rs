//! The bati! body form replaces method chains on widgets. Instead of
//! `item: MenuItem::new_literal("Run").on_activate(cmd).tooltip_literal("...")`,
//! the idiomatic syntax is `item: MenuItem::new_literal("Run") { on_activate: cmd; tooltip_literal: "..." }`
//! — each builder method becomes a body item. The result is a more
//! uniform DSL (the same name-value shape as top-level elements) and
//! avoids the element-vs-expression ambiguity that method chains
//! introduce.
//!
//! This fixture locks in the body-form behavior for element-valued
//! property arguments.

use bastyde::prelude::*;

#[derive(Debug)]
struct MenuItem {
    label: &'static str,
    activated: bool,
    tooltip: Option<&'static str>,
}

impl MenuItem {
    fn new_literal(label: &'static str) -> Self {
        Self {
            label,
            activated: false,
            tooltip: None,
        }
    }

    fn on_activate(mut self, _: u32) -> Self {
        self.activated = true;
        self
    }

    fn tooltip_literal(mut self, t: &'static str) -> Self {
        self.tooltip = Some(t);
        self
    }
}

impl Widget for MenuItem {
    fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        p.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug, Default)]
struct Menu {
    items: std::cell::RefCell<Vec<(String, bool, Option<&'static str>)>>,
}

impl Menu {
    fn new() -> Self {
        Self::default()
    }

    fn item(self, i: MenuItem) -> Self {
        self.items
            .borrow_mut()
            .push((i.label.to_string(), i.activated, i.tooltip));
        self
    }
}

impl Widget for Menu {
    fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        p.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let m: Menu = bati!(
        Menu {
            item: MenuItem::new(lit!("Run")) {
                on_activate: 1
                tooltip_literal: "Runs the thing"
            }
            item: MenuItem::new(lit!("Stop")) {
                on_activate: 2
            }
        }
    );
    let items = m.items.borrow();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0], ("Run".to_string(), true, Some("Runs the thing")));
    assert_eq!(items[1], ("Stop".to_string(), true, None));
}
