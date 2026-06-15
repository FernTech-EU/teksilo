// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Handler-attachment properties (`on_tap`, `context_menu`, `cursor`,
//! `focusable`, …) wrap the widget in `WidgetWithHandlers<T>`, which
//! doesn't expose per-widget methods like `.child()` or `.spacing()`.
//! The macro therefore reorders handler properties to come AFTER every
//! widget-specific item, letting users write them in any source order.
//!
//! Here: a `context_menu` property sits BEFORE bare children and a
//! spacing property. Without reorder, the `.spacing(...)` after
//! `.context_menu(...)` wouldn't resolve.

use bastyde::prelude::*;

#[derive(Debug, Default)]
struct Leaf;
impl Leaf {
    fn new() -> Self {
        Self
    }
}
impl Widget for Leaf {
    fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> bastyde_core::widget::LayoutResponse {
        p.resolve(0.0, 0.0).into()
    }
}

fn build(ctx: &mut BuildContext) -> WidgetId {
    bati!(ctx => bastyde::widgets::primitives::VStack {
            on_tap: move |_, _ctx| { /* handler */ }
            spacing: 12.0
            Leaf
            focusable: true
            Leaf
            cursor: CursorIcon::Pointer
        }
    )
}

fn main() {
    let _build: fn(&mut BuildContext) -> WidgetId = build;
}
