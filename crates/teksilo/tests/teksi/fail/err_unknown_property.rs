// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spec §9.1: a property whose name doesn't match a builder method on
//! the type surfaces the compiler's method-resolution error under the
//! property ident.

use teksilo::prelude::*;

#[derive(Debug)]
struct Leaf;
impl Leaf {
    fn new() -> Self {
        Self
    }
}
impl Widget for Leaf {
    fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
        p.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let _ = teksu!(
        Leaf {
            nonexistent_prop: 42
        }
    );
}
