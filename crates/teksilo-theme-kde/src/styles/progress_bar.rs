// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze progress bar.
//!
//! A 6 px groove centred across the bar, filled and stroked with a 1 px pen
//! on itself inset by half the pen at radius 3: the window text at 14 % over
//! the window, outlined in the window text at the frame contrast. A
//! determinate bar's contents run from the start of the groove, `floor(value
//! x length)` long, in the highlight at 70 % over the window, outlined in the
//! highlight at the frame contrast; they start at the right-hand end in a
//! right-to-left window, and at the bottom of a vertical bar. A disabled bar
//! paints the disabled colours.
//!
//! A busy (indeterminate) bar fills its groove with stripes 28 px apart, a
//! 14 px stripe of the highlight and a 14 px stripe of the highlight moved
//! 70 % toward the window, which move one period along the groove every
//! 800 ms, linearly and looping. The style paints this itself
//! ([`ProgressBarStyle::paints_busy_indicator`]), so the bar draws no sweep
//! of its own; under the reduced-motion preference the stripes stand still.
//! The desktop's animation speed does not change the period, as in Breeze;
//! its `breezerc` can, and can stop the stripes ([`busy_step`]).
//!
//! The bar is 26 px thick at Plasma's default font, Qt's size hint, the
//! groove centred in it ([`BreezeProgressMetrics`]); a thickness the app
//! sets wins. A track or fill colour the app sets takes the place of the
//! groove's or the highlight's.
//!
//! ## Where this departs from Breeze
//!
//! - **A short contents keeps its length.** Breeze draws contents shorter
//!   than the groove's thickness 6 px long and clips them; here they are
//!   drawn at their length, the radius shrinking to half of it.

use std::time::Duration;

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::environment::LayoutDirection;
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::styles::{ProgressBarStyle, ProgressBarStyleConfig, ProgressKind, Theme};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use teksilo_core::widget_id::WidgetId;
use teksilo_kde_scheme::{ColorState, PaletteRole};
use teksilo_tokens::{Color, CornerRadius, InputTokens, Orientation, TypographyTokens};

use crate::breeze_config::BreezeStyleConfig;
use crate::palette::KdePalette;
use crate::styles::chrome::kde_mix;
use crate::styles::metrics::{
    BreezeProgressMetrics, PROGRESS_BUSY_LIGHT_MIX, PROGRESS_CONTENTS_ALPHA, PROGRESS_GROOVE_ALPHA,
    breeze_progress_metrics_for,
};

/// Breeze `ProgressBarStyle`.
///
/// [`Default`] is the Breeze typography; the preset installs
/// [`BreezeProgressBarStyle::for_theme`] of the theme it builds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeProgressBarStyle {
    /// The progress-bar dimensions this style paints and reports.
    pub metrics: BreezeProgressMetrics,
}

impl BreezeProgressBarStyle {
    /// The style for `typography`.
    pub fn for_typography(typography: &TypographyTokens) -> Self {
        Self {
            metrics: breeze_progress_metrics_for(typography),
        }
    }

    /// The style for the typography of `theme`.
    pub fn for_theme(theme: &Theme) -> Self {
        Self::for_typography(&theme.typography)
    }
}

impl Default for BreezeProgressBarStyle {
    fn default() -> Self {
        Self::for_typography(&crate::typography::breeze_typography())
    }
}

impl ProgressBarStyle for BreezeProgressBarStyle {
    fn make_body(&self, cfg: &ProgressBarStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        let (value, phase) = match &cfg.progress {
            ProgressKind::Determinate(p) => (Some(p.clone()), None),
            ProgressKind::Indeterminate => {
                let phase = ctx.animated_signal(0.0);
                if let Some(step) = busy_step(ctx.theme()) {
                    let spec = ctx.animate().duration(step).linear().looping();
                    if !spec.reduced_motion() {
                        spec.to(&phase, 1.0);
                    }
                }
                (None, Some(phase))
            }
        };
        ctx.add(BreezeProgressBody {
            orientation: cfg.orientation,
            value,
            phase,
            track: cfg.track_color_override.clone(),
            fill: cfg.fill_color_override.clone(),
            metrics: self.metrics,
        })
    }

    /// [`BreezeProgressMetrics::thickness`], 26 at Plasma's default font.
    fn thickness(&self, _tokens: &InputTokens) -> Option<f32> {
        Some(self.metrics.thickness)
    }

    /// The busy stripes are this style's.
    fn paints_busy_indicator(&self) -> bool {
        true
    }
}

/// The period a busy bar's stripes move in under `theme`, or `None` when
/// they stand still.
///
/// `[S]` The desktop's `breezerc` decides whether they move
/// ([`BreezeStyleConfig::busy_step`]); the period is the theme's
/// `duration_indeterminate_sweep`, which the preset sets to
/// `ProgressBarBusyStepDuration`, 800 ms by default. A theme without the
/// config moves them with its sweep duration; a sweep of no length stands
/// still.
pub fn busy_step(theme: &Theme) -> Option<Duration> {
    let moves = theme
        .extension::<BreezeStyleConfig>()
        .is_none_or(|config| config.busy_step().is_some());
    let step = theme.motion.duration_indeterminate_sweep;
    (moves && !step.is_zero()).then_some(step)
}

/// The colours a Breeze progress bar is painted in, in one paint state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeProgressColors {
    /// `[Hp]` The groove: the window text at 14 % over the window.
    pub groove: Color,
    /// `[Hp]` Its outline: the window text at the frame contrast.
    pub groove_outline: Color,
    /// `[Hp]` The contents: the highlight at 70 % over the window.
    pub contents: Color,
    /// `[Hp]` Their outline: the highlight at the frame contrast.
    pub contents_outline: Color,
    /// `[Hp]` A busy bar's highlight stripe.
    pub busy: Color,
    /// `[Hp]` A busy bar's other stripe: the highlight moved 70 % toward the
    /// window.
    pub busy_light: Color,
}

impl BreezeProgressColors {
    /// The colours of `theme` as it paints with or without the high-contrast
    /// preference, for an enabled or a disabled bar, with the track and fill
    /// colours an app set, if any.
    ///
    /// The window, its text and the highlight are the theme's projected
    /// tokens (the disabled palette's window and window text for a disabled
    /// bar); the frame contrast comes from [`KdePalette`], Breeze's 0.2
    /// without it.
    pub fn resolve(
        theme: &Theme,
        high_contrast: bool,
        disabled: bool,
        track: Option<Color>,
        fill: Option<Color>,
    ) -> BreezeProgressColors {
        let c = &theme.colors;
        let palette = theme.extension::<KdePalette>();
        let (window, window_text) = match (disabled, palette) {
            (true, Some(p)) => (
                p.role(ColorState::Disabled, PaletteRole::Window),
                p.role(ColorState::Disabled, PaletteRole::WindowText),
            ),
            (true, None) => (c.surface_main, c.text_disabled),
            (false, _) => (c.surface_main, c.text_primary),
        };
        let highlight = fill.unwrap_or(if disabled {
            c.accent_disabled
        } else {
            c.accent
        });
        // `[derived]` The frame contrast: 1 in the high-contrast scheme the
        // palette builds, as `KdePalette::breeze_colors` computes it.
        let fc = match palette {
            Some(_) if high_contrast => 1.0,
            Some(p) => p.frame_contrast(),
            None => teksilo_kde_scheme::scheme::DEFAULT_FRAME_CONTRAST,
        } as f32;
        BreezeProgressColors {
            groove: track.unwrap_or_else(|| kde_mix(window, window_text, PROGRESS_GROOVE_ALPHA)),
            groove_outline: window_text.with_alpha(fc),
            contents: kde_mix(window, highlight, PROGRESS_CONTENTS_ALPHA),
            contents_outline: highlight.with_alpha(fc),
            busy: highlight,
            busy_light: kde_mix(highlight, window, PROGRESS_BUSY_LIGHT_MIX),
        }
    }
}

/// `[Hp]` The groove in a bar's `bounds`: `thickness` across, centred and
/// rounded down to a whole pixel, the bar's full length along it.
pub fn groove_rect(bounds: Rect, orientation: Orientation, thickness: f32) -> Rect {
    match orientation {
        Orientation::Horizontal => {
            let t = thickness.min(bounds.height);
            let y = bounds.y + ((bounds.height - t) * 0.5).floor();
            Rect::new(bounds.x, y, bounds.width, t)
        }
        Orientation::Vertical => {
            let t = thickness.min(bounds.width);
            let x = bounds.x + ((bounds.width - t) * 0.5).floor();
            Rect::new(x, bounds.y, t, bounds.height)
        }
    }
}

/// `[Hp]` A determinate bar's contents in `groove`: `floor(value x length)`
/// long from the start, the right-hand end in a right-to-left window and
/// the bottom of a vertical bar.
pub fn contents_rect(
    groove: Rect,
    orientation: Orientation,
    direction: LayoutDirection,
    value: f32,
) -> Rect {
    let value = value.clamp(0.0, 1.0);
    match orientation {
        Orientation::Horizontal => {
            let len = (value * groove.width).floor();
            let x = if direction == LayoutDirection::RightToLeft {
                groove.x + groove.width - len
            } else {
                groove.x
            };
            Rect::new(x, groove.y, len, groove.height)
        }
        Orientation::Vertical => {
            let len = (value * groove.height).floor();
            Rect::new(groove.x, groove.y + groove.height - len, groove.width, len)
        }
    }
}

/// `[Hp]` The highlight stripes of a busy bar's `groove`, moved `phase` of a
/// period along it: each `(start, end)` along the groove, clipped to it.
pub fn busy_stripes(length: f32, period: f32, phase: f32) -> Vec<(f32, f32)> {
    if length <= 0.0 || period <= 0.0 {
        return Vec::new();
    }
    let half = period * 0.5;
    let shift = phase.rem_euclid(1.0) * period;
    let mut stripes = Vec::new();
    let mut start = shift - period;
    while start < length {
        let a = start.max(0.0);
        let b = (start + half).min(length);
        if b > a {
            stripes.push((a, b));
        }
        start += period;
    }
    stripes
}

/// The `(along, length)` part of `groove` as a rectangle.
fn along(groove: Rect, orientation: Orientation, from: f32, to: f32) -> Rect {
    match orientation {
        Orientation::Horizontal => Rect::new(groove.x + from, groove.y, to - from, groove.height),
        Orientation::Vertical => Rect::new(groove.x, groove.y + from, groove.width, to - from),
    }
}

/// Paints a Breeze progress bar: its groove and contents, or its stripes.
struct BreezeProgressBody {
    orientation: Orientation,
    value: Option<Prop<f32>>,
    /// The busy stripes' position in their period, for an indeterminate
    /// bar.
    phase: Option<Signal<f32>>,
    track: Option<ColorProp>,
    fill: Option<ColorProp>,
    metrics: BreezeProgressMetrics,
}

impl std::fmt::Debug for BreezeProgressBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeProgressBody")
            .field("orientation", &self.orientation)
            .finish()
    }
}

impl Widget for BreezeProgressBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        if let Some(v) = &self.value {
            v.register_if_bound(id, registry, BindingLevel::RepaintOnly);
        }
        if let Some(p) = &self.phase {
            p.bind_to(id, registry, BindingLevel::RepaintOnly);
        }
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // The bar owns its size; this fills what it is given.
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let theme = ctx.theme;
        let enabled = ctx.effective_enabled;
        let colors = BreezeProgressColors::resolve(
            theme,
            ctx.prefers_high_contrast,
            !enabled,
            self.track.as_ref().map(|p| p.resolve(theme, enabled)),
            self.fill.as_ref().map(|p| p.resolve(theme, enabled)),
        );
        let groove = groove_rect(bounds, self.orientation, self.metrics.groove);
        let pen = theme.shape.border_width;
        let radius = CornerRadius::uniform(self.metrics.radius);
        let inset = |r: Rect| {
            Rect::new(
                r.x + pen * 0.5,
                r.y + pen * 0.5,
                (r.width - pen).max(0.0),
                (r.height - pen).max(0.0),
            )
        };

        if let Some(phase) = &self.phase {
            // Busy: the lighter stripes fill the groove and the highlight
            // stripes are laid over them.
            let length = match self.orientation {
                Orientation::Horizontal => groove.width,
                Orientation::Vertical => groove.height,
            };
            canvas.fill_rounded_rect(groove, radius, colors.busy_light);
            for (a, b) in busy_stripes(length, self.metrics.busy_period, phase.get()) {
                let stripe = along(groove, self.orientation, a, b);
                // A stripe at an end of the groove keeps the groove's
                // rounded corners there.
                let mut corners = CornerRadius::ZERO;
                let r = self.metrics.radius;
                let (at_start, at_end) = (a <= 0.0, b >= length);
                match self.orientation {
                    Orientation::Horizontal => {
                        if at_start {
                            corners.top_left = r;
                            corners.bottom_left = r;
                        }
                        if at_end {
                            corners.top_right = r;
                            corners.bottom_right = r;
                        }
                    }
                    Orientation::Vertical => {
                        if at_start {
                            corners.top_left = r;
                            corners.top_right = r;
                        }
                        if at_end {
                            corners.bottom_left = r;
                            corners.bottom_right = r;
                        }
                    }
                }
                canvas.fill_rounded_rect(stripe, corners, colors.busy);
            }
            return;
        }

        let stroked = inset(groove);
        canvas.fill_rounded_rect(stroked, radius, colors.groove);
        if pen > 0.0 {
            canvas.stroke_rounded_rect(stroked, radius, colors.groove_outline, pen);
        }
        let value = self.value.as_ref().map_or(0.0, Prop::get);
        let contents = contents_rect(groove, self.orientation, ctx.layout_direction, value);
        if contents.width > 0.0 && contents.height > 0.0 {
            let stroked = inset(contents);
            canvas.fill_rounded_rect(stroked, radius, colors.contents);
            if pen > 0.0 {
                canvas.stroke_rounded_rect(stroked, radius, colors.contents_outline, pen);
            }
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational: the `ProgressBar` announces its value.
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_kde_scheme::KdeColor;

    fn rgba8(c: Color) -> [u8; 4] {
        KdeColor::from_color(c).to_rgba8()
    }

    /// Whether two colours are within one step a channel: Qt's raster
    /// painter rounds an 8-bit blend its own way.
    fn near(a: [u8; 4], b: [u8; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 1)
    }

    #[test]
    fn the_style_sizes_the_bar_and_paints_its_busy_indicator() {
        let s = BreezeProgressBarStyle::default();
        assert_eq!(s.thickness(&InputTokens::default()), Some(26.0));
        assert!(s.paints_busy_indicator());
        assert_eq!(BreezeProgressBarStyle::for_theme(&crate::light()), s);
    }

    /// The groove Qt laid out in a 26 px bar, 10 px down, and the one
    /// Plasma painted in an 18 px sample, 6 px down.
    #[test]
    fn the_groove_is_centred_across_the_bar() {
        let g = groove_rect(
            Rect::new(0.0, 0.0, 160.0, 26.0),
            Orientation::Horizontal,
            6.0,
        );
        assert_eq!((g.x, g.y, g.width, g.height), (0.0, 10.0, 160.0, 6.0));
        let g = groove_rect(
            Rect::new(0.0, 0.0, 200.0, 18.0),
            Orientation::Horizontal,
            6.0,
        );
        assert_eq!(g.y, 6.0);
        let g = groove_rect(Rect::new(0.0, 0.0, 26.0, 91.0), Orientation::Vertical, 6.0);
        assert_eq!((g.x, g.width, g.height), (10.0, 6.0, 91.0));
    }

    #[test]
    fn the_contents_run_from_the_start_and_mirror() {
        let g = Rect::new(0.0, 6.0, 200.0, 6.0);
        let c = contents_rect(
            g,
            Orientation::Horizontal,
            LayoutDirection::LeftToRight,
            0.5,
        );
        assert_eq!((c.x, c.width), (0.0, 100.0));
        let c = contents_rect(
            g,
            Orientation::Horizontal,
            LayoutDirection::RightToLeft,
            0.5,
        );
        assert_eq!((c.x, c.width), (100.0, 100.0));
        let c = contents_rect(
            g,
            Orientation::Horizontal,
            LayoutDirection::LeftToRight,
            0.333,
        );
        assert_eq!(c.width, 66.0, "rounded down");
        let v = Rect::new(0.0, 0.0, 6.0, 100.0);
        let c = contents_rect(v, Orientation::Vertical, LayoutDirection::LeftToRight, 0.25);
        assert_eq!((c.y, c.height), (75.0, 25.0));
    }

    #[test]
    fn the_stripes_are_fourteen_of_twenty_eight_and_move_with_the_phase() {
        assert_eq!(
            busy_stripes(60.0, 28.0, 0.0),
            vec![(0.0, 14.0), (28.0, 42.0), (56.0, 60.0)]
        );
        assert_eq!(
            busy_stripes(60.0, 28.0, 0.5),
            vec![(14.0, 28.0), (42.0, 56.0)]
        );
        let shifted = busy_stripes(60.0, 28.0, 0.25);
        assert_eq!(shifted[0], (7.0, 21.0));
        assert_eq!(shifted[1], (35.0, 49.0));
        // A whole period is the same picture.
        assert_eq!(busy_stripes(60.0, 28.0, 1.0), busy_stripes(60.0, 28.0, 0.0));
        assert!(busy_stripes(0.0, 28.0, 0.3).is_empty());
    }

    /// The groove and contents Plasma painted in Breeze Light.
    #[test]
    fn breeze_lights_progress_colours_are_plasmas() {
        let c = BreezeProgressColors::resolve(&crate::light(), false, false, None, None);
        assert!(near(rgba8(c.groove), [210, 211, 213, 255]), "#d2d3d5");
        assert!(near(rgba8(c.contents), [114, 194, 235, 255]), "#72c2eb");
        assert_eq!(rgba8(c.busy), [61, 174, 233, 255]);
        assert!((c.groove_outline.a() - 0.2).abs() < 1e-6);
        assert!((c.contents_outline.a() - 0.2).abs() < 1e-6);
        let light = rgba8(c.busy_light);
        // 70 % of the way from the highlight to the window.
        assert_eq!(light, [186, 220, 239, 255]);
    }

    #[test]
    fn overrides_and_the_disabled_state_change_the_colours() {
        let theme = crate::light();
        let c = BreezeProgressColors::resolve(
            &theme,
            false,
            false,
            Some(Color::RED),
            Some(Color::BLUE),
        );
        assert_eq!(c.groove, Color::RED);
        assert_eq!(c.busy, Color::BLUE);
        let d = BreezeProgressColors::resolve(&theme, false, true, None, None);
        assert_eq!(d.busy, theme.colors.accent_disabled);
        let mut bare = theme.clone();
        bare.extensions.remove::<KdePalette>();
        let b = BreezeProgressColors::resolve(&bare, false, false, None, None);
        assert_eq!(b.busy, bare.colors.accent);
    }
}
