//! Spec §9.1: a property whose name doesn't match a builder method on
//! the type surfaces the compiler's method-resolution error under the
//! property ident.

use fern_ui::prelude::*;

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
    let _ = fern!(
        Leaf {
            nonexistent_prop: 42
        }
    );
}
