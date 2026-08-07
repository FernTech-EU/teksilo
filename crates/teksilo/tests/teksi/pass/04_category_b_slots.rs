// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Spec §4.2: Category B widgets use named slot methods rather than
//! `.child()`. Slot values can be full elements with their own body —
//! no special syntax needed, just `slot: Element { ... }`.

use teksilo::prelude::*;

#[derive(Debug)]
struct Header {
    text: &'static str,
}

impl Header {
    fn new(text: &'static str) -> Self {
        Self { text }
    }
}

impl Widget for Header {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug)]
struct Footer {
    label: &'static str,
}

impl Footer {
    fn new(label: &'static str) -> Self {
        Self { label }
    }
}

impl Widget for Footer {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug, Default)]
struct CardLike {
    header_text: Option<String>,
    content_kind: Option<&'static str>,
    footer_label: Option<String>,
    padding: Option<f32>,
}

impl CardLike {
    fn new() -> Self {
        Self::default()
    }

    fn header(mut self, h: Header) -> Self {
        self.header_text = Some(h.text.to_string());
        self
    }

    fn content(mut self, c: Content) -> Self {
        self.content_kind = Some(c.kind);
        self
    }

    fn footer(mut self, f: Footer) -> Self {
        self.footer_label = Some(f.label.to_string());
        self
    }

    fn padding(mut self, p: f32) -> Self {
        self.padding = Some(p);
        self
    }
}

impl Widget for CardLike {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

#[derive(Debug)]
struct Content {
    kind: &'static str,
}

impl Content {
    fn new(kind: &'static str) -> Self {
        Self { kind }
    }
}

impl Widget for Content {
    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

fn main() {
    let c: CardLike = teksu!(
        CardLike {
            header: Header("Title")
            content: Content("body")
            footer: Footer("OK")
            padding: 16.0
        }
    );
    assert_eq!(c.header_text.as_deref(), Some("Title"));
    assert_eq!(c.content_kind, Some("body"));
    assert_eq!(c.footer_label.as_deref(), Some("OK"));
    assert_eq!(c.padding, Some(16.0));
}
