//! Spec §5.6 side-effect form: `rust { ...; }` (last stmt ends with `;`)
//! runs for its side effects and produces no children. Forces
//! statement-sequence lowering on the enclosing element.

use fern_ui::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

#[derive(Debug)]
struct Tag {
    name: &'static str,
}

impl Tag {
    fn new(name: &'static str) -> Self {
        Self { name }
    }
}

impl Widget for Tag {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        proposal.resolve(0.0, 0.0)
    }
}

#[derive(Debug, Default)]
struct Container {
    tags: std::cell::RefCell<Vec<&'static str>>,
}

impl Container {
    fn new() -> Self {
        Self::default()
    }

    fn child(self, t: Tag) -> Self {
        self.tags.borrow_mut().push(t.name);
        self
    }
}

impl Widget for Container {
    fn size_that_fits(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> Size {
        proposal.resolve(0.0, 0.0)
    }
}

fn main() {
    let side_effect_ran = Rc::new(Cell::new(0_u32));
    let probe = side_effect_ran.clone();

    let c: Container = fern!(
        Container {
            Tag("before")
            rust {
                probe.set(probe.get() + 1);
                probe.set(probe.get() + 10);
            }
            Tag("after")
        }
    );

    assert_eq!(side_effect_ran.get(), 11);
    assert_eq!(*c.tags.borrow(), vec!["before", "after"]);
}
