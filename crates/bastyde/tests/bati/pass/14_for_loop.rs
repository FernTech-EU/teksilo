//! Spec §5.2: `for pat in iter { Elem }` lowers to
//! `.children(iter.map(|pat| Elem::new()))`. Optional `let` bindings
//! inside the for-body are emitted as statements inside the closure.

use bastyde::prelude::*;

#[derive(Clone, Debug)]
struct Item {
    id: u32,
    title: String,
}

#[derive(Debug)]
struct ListItem {
    label: String,
    tag: u32,
}

impl ListItem {
    fn new(label: String) -> Self {
        Self { label, tag: 0 }
    }

    fn tag(mut self, tag: u32) -> Self {
        self.tag = tag;
        self
    }
}

impl Widget for ListItem {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug, Default)]
struct VLike {
    items: std::cell::RefCell<Vec<(String, u32)>>,
}

impl VLike {
    fn new() -> Self {
        Self::default()
    }

    fn children<I>(self, iter: I) -> Self
    where
        I: IntoIterator<Item = ListItem>,
    {
        for li in iter {
            self.items.borrow_mut().push((li.label, li.tag));
        }
        self
    }

    fn child(self, li: ListItem) -> Self {
        self.items.borrow_mut().push((li.label, li.tag));
        self
    }
}

impl Widget for VLike {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let items = vec![
        Item { id: 1, title: "alpha".to_string() },
        Item { id: 2, title: "beta".to_string() },
        Item { id: 3, title: "gamma".to_string() },
    ];

    let v: VLike = bati!(
        VLike {
            ListItem("head".to_string())
            for item in items.into_iter() {
                let id = item.id;
                let title = item.title.clone();
                ListItem(title) { tag: id }
            }
            ListItem("tail".to_string())
        }
    );

    let got = v.items.borrow();
    assert_eq!(got.len(), 5);
    assert_eq!(got[0], ("head".to_string(), 0));
    assert_eq!(got[1], ("alpha".to_string(), 1));
    assert_eq!(got[2], ("beta".to_string(), 2));
    assert_eq!(got[3], ("gamma".to_string(), 3));
    assert_eq!(got[4], ("tail".to_string(), 0));
}
