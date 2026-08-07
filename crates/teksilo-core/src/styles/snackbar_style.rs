// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `Snackbar`. See `docs/styling-system.md`.
//!
//! Themes the snackbar surface — the high-contrast notification panel
//! that floats up from the bottom of the window. The `Snackbar`
//! widget keeps its `Role::Alert` / `Live::Polite` accessibility node;
//! `SnackbarStyle` only owns the surface chrome.

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::widget_id::WidgetId;

#[derive(Clone, Debug)]
pub struct SnackbarStyleConfig {
    /// Pre-built message + optional action subtree the surface wraps.
    pub content: WidgetId,
}

pub trait SnackbarStyle: 'static {
    fn make_body(&self, cfg: &SnackbarStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedSnackbarStyle = Rc<dyn SnackbarStyle>;
