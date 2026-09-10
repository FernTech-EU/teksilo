// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spec §9.2 for the popover family, which is Category B by NAME.
//!
//! The family has four names, all aliases of `PopoverWidget<T>`, and none
//! of them is spelled `Popover` — which is what the diagnostic's list used
//! to say, so a bare child in any real popover fell through to the generic
//! "no method named `child`" error instead of the slot hint. The predicate
//! is a name match, so pinning one of the four is enough to keep the list
//! naming types that exist.

use teksilo::prelude::*;

#[derive(Debug)]
struct PopoverButton;
impl PopoverButton {
    fn new() -> Self {
        Self
    }
}
impl Widget for PopoverButton {
    fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
        p.resolve(0.0, 0.0).into()
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
    fn layout_response(&self, p: SizeProposal, _: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
        p.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let _: PopoverButton = teksu!(
        PopoverButton {
            TextWidget("hi")
        }
    );
}
