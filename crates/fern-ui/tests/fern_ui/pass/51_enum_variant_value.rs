//! `Type::Variant` at a property-value position is parsed as a Rust
//! expression (enum variant access), not as a fern element. The two-
//! UpperCamel-segment shape is the syntactic signal.
//!
//! Tuple-variant construction `Type::Variant(args)` follows the same
//! rule — also parsed as an expression.

use fern_ui::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Fit {
    Contain,
    Cover,
}

#[derive(Debug, Clone, PartialEq)]
enum Overflow {
    Clip,
    Ellipsis(EllipsisMode),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum EllipsisMode {
    Head,
    Trailing,
}

#[derive(Debug, Default)]
struct Image {
    fit: Option<Fit>,
    overflow: Option<Overflow>,
}

impl Image {
    fn new() -> Self {
        Self::default()
    }

    fn fit(mut self, f: Fit) -> Self {
        self.fit = Some(f);
        self
    }

    fn overflow(mut self, o: Overflow) -> Self {
        self.overflow = Some(o);
        self
    }
}

impl Widget for Image {
    fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> fern_core::widget::LayoutResponse {
        p.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let i: Image = fern!(
        Image {
            fit: Fit::Contain
            overflow: Overflow::Ellipsis(EllipsisMode::Trailing)
        }
    );
    assert_eq!(i.fit, Some(Fit::Contain));
    assert_eq!(i.overflow, Some(Overflow::Ellipsis(EllipsisMode::Trailing)));
}
