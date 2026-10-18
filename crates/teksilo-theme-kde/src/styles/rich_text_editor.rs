// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze frame of an editable `RichTextEditor`: the line edit's.
//!
//! The specification frames a text area, as it frames every scroll area,
//! with a fill of the view background in the input-frame outline stroked at
//! radius 4.5 (k.5.20), the outline the line edit is drawn in (k.5.3). So
//! an editable editor is framed by the line edit's own painter,
//! [`BreezeFieldChrome`]: a 1 px outline on the node inset by half the pen,
//! in `border_strong` at rest and `border_focused` while the editor has
//! focus, its width unchanged by focus. A `RichTextEditor::background`
//! replaces the view fill and keeps the outline.
//!
//! A read-only viewer is frameless, as the shipped recipe style leaves it:
//! a viewer sits on a surface that owns its chrome. The style hands it to
//! [`RecipeRichTextEditorStyle`], which honours the viewer's
//! `content_padding` and `background`.
//!
//! ## Where this departs from Breeze
//!
//! - **D6, the outline at rest**, as on the line edit
//!   ([`crate::styles::text_input`]): the frame outline is 1.5:1 on the
//!   Breeze Light window and the view fill 1.1:1, so the outline at rest is
//!   `border_strong`, lifted to 3:1; focus is `border_focused`, lifted to
//!   3:1 (D5).
//! - **The text inset** (`[derived]`). The specification gives a text
//!   area's frame its 2 px frame width and is silent on where the text
//!   starts inside it. The text lies 6 px inside the frame on every side,
//!   the line edit's text inset (k.5.3), through the density ladder, so an
//!   editor's text lines up with a line edit's above it. An app's
//!   `RichTextEditor::content_padding` replaces it.
//! - **No hover outline.** The editor gives its style no hover state, so
//!   the outline does not turn to the hover colour under the pointer.

use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{
    RichTextEditorStyle, RichTextEditorStyleConfig, TextInputValidationLevel,
};
use teksilo_core::widget_id::WidgetId;
use teksilo_widgets::primitives::{Padding, ZStack};
use teksilo_widgets::styles::RecipeRichTextEditorStyle;

use crate::styles::metrics::breeze_field_metrics_for;
use crate::styles::text_input::BreezeFieldChrome;

/// Breeze `RichTextEditorStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct BreezeRichTextEditorStyle;

impl RichTextEditorStyle for BreezeRichTextEditorStyle {
    fn make_body(&self, cfg: &RichTextEditorStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        if cfg.is_read_only {
            return RecipeRichTextEditorStyle.make_body(cfg, ctx);
        }
        let theme = ctx.theme();
        let inset = breeze_field_metrics_for(&theme.input, &theme.typography).padding_horizontal;
        let mut chrome = BreezeFieldChrome::new(
            cfg.is_focused.clone(),
            Signal::new(false),
            Signal::new(false),
            Signal::new(TextInputValidationLevel::None),
            false,
        );
        if let Some(background) = cfg.background.clone() {
            chrome = chrome.fill(background);
        }
        let chrome = ctx.add(chrome);
        let (top, right, bottom, left) =
            cfg.content_padding.unwrap_or((inset, inset, inset, inset));
        let padded = ctx.add(Padding::new(top, right, bottom, left).child(cfg.viewport));
        ctx.add(ZStack::new().child(chrome).child(padded))
    }
}
