// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `SplitButton`.
//!
//! See `docs/styling-system.md`. The trait is object-safe so
//! `Rc<dyn SplitButtonStyle>` can be stored in a theme slot or attached
//! per-call via `SplitButton::style(...)`.
//!
//! Mirrors [`ButtonStyle`](crate::styles::ButtonStyle): the active style
//! receives a pre-built `content` subtree (the SplitButton's interactive
//! row — default-action region │ divider │ chevron region, with its text
//! already coloured and its tap / menu handlers attached) and arranges the
//! shared frame chrome (background fill, border, corner radius, min size)
//! around it. The widget keeps ownership of text-colour resolution and all
//! event wiring, exactly as `Button` keeps `resolve_text_role`.

use std::rc::Rc;

use crate::build_context::BuildContext;
use crate::signal::Signal;
use crate::styles::ButtonVariant;
use crate::widget_id::WidgetId;

/// Inputs handed to a [`SplitButtonStyle::make_body`] call.
///
/// `content` is the pre-built interactive row (the style only frames it; it
/// never builds the regions or divider itself). The four boolean signals
/// carry the live interaction state — the style can `.zip` / `.map` them to
/// drive a reactive background / border. `variant` is the design-language
/// hint the style may honour or remap (shared with [`ButtonVariant`]).
#[derive(Clone, Debug)]
pub struct SplitButtonStyleConfig {
    /// The assembled, interactive row: main region │ divider │ chevron.
    pub content: WidgetId,
    pub is_pressed: Signal<bool>,
    pub is_hovered: Signal<bool>,
    pub is_focused: Signal<bool>,
    pub is_disabled: Signal<bool>,
    pub variant: ButtonVariant,
}

/// Style protocol for `SplitButton`. The active style owns the shared frame
/// chrome (fill, border, corner radius, overall min size) and arranges it
/// around the pre-built interactive `content` row.
///
/// `'static` (no `Send + Sync`) for the same reason as [`ButtonStyle`](crate::styles::ButtonStyle):
/// the rest of bastyde-core is single-threaded (`Signal` uses `Rc`).
pub trait SplitButtonStyle: 'static {
    fn make_body(&self, cfg: &SplitButtonStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

/// Shared handle for a `SplitButtonStyle` impl. Cheap to clone; one shared
/// `Rc` is used per theme slot and per-call override.
pub type SharedSplitButtonStyle = Rc<dyn SplitButtonStyle>;
