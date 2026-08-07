// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-3 style protocol for `ProgressBar`. See `docs/styling-system.md`.
//!
//! Themes the stationary chrome — the track and (for determinate
//! bars) the proportional fill. The indeterminate sweep itself stays
//! widget-owned (principle 6: motion infrastructure is not chrome) —
//! the `ProgressBar` widget mounts a sibling sweep leaf inside its
//! `build()` and the leaf's `paint()` issues the
//! `draw_animated_quad` call (horizontal-shader path) or the
//! signal-driven moving fill (vertical / reduced-motion path).

use std::rc::Rc;

use teksilo_tokens::Orientation;

use crate::build_context::BuildContext;
use crate::color_prop::ColorProp;
use crate::signal::Prop;
use crate::widget_id::WidgetId;

#[derive(Clone, Debug)]
pub enum ProgressKind {
    /// A determinate bar fills proportionally to a `Prop<f32>` in
    /// `[0.0, 1.0]`.
    Determinate(Prop<f32>),
    /// An indeterminate bar — the recipe paints only the track; the
    /// `ProgressBar` widget mounts the sweep on top.
    Indeterminate,
}

#[derive(Clone, Debug)]
pub struct ProgressBarStyleConfig {
    pub orientation: Orientation,
    pub progress: ProgressKind,
    /// Caller override for the track background — `None` means "use
    /// the recipe default" (`SurfaceRole::Sunken`).
    pub track_color_override: Option<ColorProp>,
    /// Caller override for the determinate fill / indeterminate sweep
    /// tint — `None` means "use the recipe default"
    /// (`SurfaceRole::Accent`).
    pub fill_color_override: Option<ColorProp>,
}

pub trait ProgressBarStyle: 'static {
    fn make_body(&self, cfg: &ProgressBarStyleConfig, ctx: &mut BuildContext) -> WidgetId;
}

pub type SharedProgressBarStyle = Rc<dyn ProgressBarStyle>;
