//! Spec §9.2: `,` between body items produces the "newlines, not
//! commas" diagnostic. Triggered when a comma sits between completed
//! body items. Commas INSIDE a property-arg list are a different story
//! — multi-arg properties consume them, so `spacing: 8.0, Foo` hits
//! the compiler's arity mismatch instead.

use fern_ui::prelude::*;

#[derive(Debug, Default)]
struct Stack;
impl Stack {
    fn new() -> Self {
        Self
    }
    fn child<W: Widget + 'static>(self, _: W) -> Self {
        self
    }
}
impl Widget for Stack {
    fn size_that_fits(&self, p: SizeProposal, _: &LayoutContext) -> Size {
        p.resolve(0.0, 0.0)
    }
}

#[derive(Debug)]
struct Leaf;
impl Leaf {
    fn new() -> Self {
        Self
    }
}
impl Widget for Leaf {
    fn size_that_fits(&self, p: SizeProposal, _: &LayoutContext) -> Size {
        p.resolve(0.0, 0.0)
    }
}

fn main() {
    let _: Stack = fern!(
        Stack {
            Leaf,
            Leaf
        }
    );
}
