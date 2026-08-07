// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for the date-edit family (`DateEdit`,
//! `TimeEdit`, `DateRangeEdit`, `DateTimeEdit`). See
//! `docs/styling-system.md`.
//!
//! All four widgets compose a themed `TextInput` with an optional
//! trailing trigger button (calendar or clock popover). The TextInput
//! itself owns the bordered surface + validation strip chrome via
//! `TextInputStyle`; the trigger is themed via `IconButtonStyle`. The
//! remaining family-specific chrome is just the *arrangement* — and in
//! IntUI today the trigger sits inside `TextInput`'s trailing slot,
//! so the default recipe is a passthrough.
//!
//! The thin `make_body(cfg)` surface still earns its keep as a hook
//! for apps that want to *wrap* the field (e.g. add a clear button
//! sibling, attach a help icon, swap the trigger placement). The
//! recipe also owns the family's dimensions — calendar trigger icon
//! size, calendar button width, segment gap — as `pub const`s on
//! `teksilo_widgets::styles::recipe_date_edit_style`.

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::widget_id::WidgetId;

pub struct DateEditStyleConfig {
    /// Pre-assembled body (themed `TextInput` with the optional
    /// trigger already parked in its trailing slot). The recipe may
    /// return this id directly or wrap it.
    pub body: WidgetId,
}

pub trait DateEditStyle: 'static {
    fn make_body(&self, cfg: &DateEditStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedDateEditStyle = Rc<dyn DateEditStyle>;
