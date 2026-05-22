//! Spec §3.4: multi-argument property where the second arg is a full
//! bati element — the TabWidget `tab: "name", Card { ... }`
//! pattern. The `Card { ... }` body uses DSL syntax (named slots via
//! `:`), not Rust struct-literal syntax, so the parser must commit to
//! element parsing on the `UpperCamel { ... }` prefix.

use bastyde::prelude::*;

#[derive(Debug)]
struct Page {
    caption: &'static str,
}

impl Page {
    fn new(caption: &'static str) -> Self {
        Self { caption }
    }

    fn label(mut self, _label: &'static str) -> Self {
        self
    }
}

impl Widget for Page {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug, Default)]
struct TabLike {
    tabs: std::cell::RefCell<Vec<(String, String)>>,
}

impl TabLike {
    fn new() -> Self {
        Self::default()
    }

    fn tab(self, name: &'static str, page: Page) -> Self {
        self.tabs
            .borrow_mut()
            .push((name.to_string(), page.caption.to_string()));
        self
    }
}

impl Widget for TabLike {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let t: TabLike = bati!(
        TabLike {
            tab: "Overview", Page("overview body") {
                label: "a"
            }
            tab: "Inspector", Page("inspector body")
        }
    );
    let tabs = t.tabs.borrow();
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs[0], ("Overview".to_string(), "overview body".to_string()));
    assert_eq!(tabs[1], ("Inspector".to_string(), "inspector body".to_string()));
}
