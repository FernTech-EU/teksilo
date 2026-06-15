// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Method chains inside a property value must be wrapped in parens
//! when the receiver is an UpperCamel-rooted path — the outer `(`
//! bypasses `peek_element_start` (which requires an Ident as the
//! first token), so the whole parenthesized expression goes through
//! the Expr parse path. This is the documented escape hatch for rare
//! cases where the idiomatic bati! body form doesn't fit.
//!
//! The `deny(unused_parens)` lint guards against the macro emitting
//! the user's outer parens verbatim into the `.prop((expr))` slot,
//! which rustc would otherwise warn about. Lowering strips one layer
//! of `Expr::Paren` before splicing.

#![deny(unused_parens)]

use bastyde::prelude::*;

struct Helper(u32);
impl Helper {
    fn from(n: u32) -> Self {
        Self(n)
    }
    fn finalize(self) -> u32 {
        self.0 * 2
    }
}

#[derive(Debug, Default)]
struct Probe {
    value: Option<u32>,
}

impl Probe {
    fn new() -> Self {
        Self::default()
    }
    fn value(mut self, v: u32) -> Self {
        self.value = Some(v);
        self
    }
}

impl Widget for Probe {
    fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        p.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let p: Probe = bati!(
        Probe {
            value: (Helper::from(10).finalize())
        }
    );
    assert_eq!(p.value, Some(20));
}
