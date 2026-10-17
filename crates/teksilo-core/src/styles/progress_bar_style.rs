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

use teksilo_tokens::{InputTokens, Orientation};

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

    /// The bar's narrow dimension, in logical pixels, at the density `tokens`
    /// describe, when the app gives it none (`ProgressBar::thickness`): the
    /// height of a horizontal bar, the width of a vertical one. `None` (the
    /// default) keeps the widget's own 4 px.
    ///
    /// A metrics accessor because the bar measures itself in
    /// `layout_response`, where the chrome `make_body` built is out of reach.
    /// A design language whose bar reserves room around a thinner groove (Qt
    /// sizes a KDE Breeze bar for its text and centres a 6 px groove in it)
    /// says so here and paints the groove inside the bounds it is given.
    fn thickness(&self, _tokens: &InputTokens) -> Option<f32> {
        None
    }

    /// Whether this style paints an indeterminate bar's motion itself.
    ///
    /// `false` (the default): the bar draws its own sweep, a shader-driven
    /// quad for a horizontal bar and a moving fill over
    /// [`make_body`](Self::make_body)'s track for a vertical one, and the
    /// style supplies only the stationary chrome. `true`: the bar mounts
    /// `make_body` alone for an indeterminate bar in either orientation, with
    /// [`ProgressKind::Indeterminate`], and the style draws the whole busy
    /// indicator, animating it (and holding it still under the reduced-motion
    /// preference, `BuildContext::prefers_reduced_motion`) itself. For a
    /// design language whose busy bar is not a sweep, such as KDE Breeze's
    /// moving stripes.
    fn paints_busy_indicator(&self) -> bool {
        false
    }
}

pub type SharedProgressBarStyle = Rc<dyn ProgressBarStyle>;
