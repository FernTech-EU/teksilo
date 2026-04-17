//! Spec §9.2: a bare child element inside a Category B widget (Card
//! here) produces a targeted hint pointing at a slot instead of the
//! generic "no method named `child`" compiler error.

use fern_ui::prelude::*;

#[derive(Debug, Default)]
struct Card;
impl Card {
    fn new() -> Self {
        Self
    }
}
impl Widget for Card {
    fn size_that_fits(&self, p: SizeProposal, _: &LayoutContext) -> Size {
        p.resolve(0.0, 0.0)
    }
}

#[derive(Debug)]
struct TextWidget {
    _text: &'static str,
}
impl TextWidget {
    fn new(text: &'static str) -> Self {
        Self { _text: text }
    }
}
impl Widget for TextWidget {
    fn size_that_fits(&self, p: SizeProposal, _: &LayoutContext) -> Size {
        p.resolve(0.0, 0.0)
    }
}

fn main() {
    let _: Card = fern!(
        Card {
            TextWidget("hi")
        }
    );
}
