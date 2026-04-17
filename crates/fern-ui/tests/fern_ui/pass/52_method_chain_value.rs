//! A method chain as a property value parses as a Rust expression
//! rather than as a fern element. Detection: after an element-shaped
//! path parses, the presence of `.method()` tokens means we have a
//! chain on a constructor call, not a fern element.
//!
//! Covers the `item: MenuItem::new(...).on_activate(...).tooltip(...)`
//! and `tab_literal: "Overview", Panel::new().padding(16.0).child(...)`
//! patterns.

use fern_ui::prelude::*;

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
    fn size_that_fits(&self, p: SizeProposal, _: &LayoutContext) -> Size {
        p.resolve(0.0, 0.0)
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
    fn size_that_fits(&self, p: SizeProposal, _: &LayoutContext) -> Size {
        p.resolve(0.0, 0.0)
    }
}

fn main() {
    let m: Menu = fern!(
        Menu {
            item: MenuItem::new_literal("Run")
                .on_activate(1)
                .tooltip_literal("Runs the thing")
            item: MenuItem::new_literal("Stop").on_activate(2)
        }
    );
    let items = m.items.borrow();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0], ("Run".to_string(), true, Some("Runs the thing")));
    assert_eq!(items[1], ("Stop".to_string(), true, None));
}
