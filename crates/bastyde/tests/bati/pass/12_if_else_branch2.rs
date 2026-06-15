// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spec §5.1: `if cond { A } else { B }` lowers via `BatiBranch<L, R>`
//! so the two arms can produce different widget types from the same
//! child position.

use bastyde::prelude::*;

#[derive(Debug)]
struct YesBanner;
impl YesBanner {
    fn new() -> Self {
        Self
    }
}
impl Widget for YesBanner {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug)]
struct NoBanner {
    label: &'static str,
}
impl NoBanner {
    fn new(label: &'static str) -> Self {
        Self { label }
    }
}
impl Widget for NoBanner {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug, Default)]
struct Holder;

impl Holder {
    fn new() -> Self {
        Self
    }

    fn child<W: Widget + 'static>(self, _w: W) -> Self {
        self
    }
}

impl Widget for Holder {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let is_logged_in = true;
    let _yes: Holder = bati!(
        Holder {
            if is_logged_in {
                YesBanner
            } else {
                NoBanner("sign in please")
            }
        }
    );

    let is_logged_in = false;
    let _no: Holder = bati!(
        Holder {
            if is_logged_in {
                YesBanner
            } else {
                NoBanner("sign in please")
            }
        }
    );
}
