// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Breeze scroll bar.
//!
//! Breeze paints no track. A permanent bar has a 1 px separator line on its
//! leading edge, the edge toward the content (the top of a horizontal bar,
//! the left of a vertical one, the right of a vertical one in a
//! right-to-left window), in the view's text at the frame contrast. The
//! handle is 8 px wide, centred across what the separator leaves, and
//! stroked with a 1 px pen on itself inset by half the pen, so it is painted
//! 7 px wide, at a radius of half its width, 4. Along the bar it is the
//! widget's own thumb, at least 20 px long, so the handle painted is the
//! one a press and a drag find.
//!
//! The handle is filled with the `scrollbar_thumb` token at rest and
//! `scrollbar_thumb_hover` while the pointer is over the bar or the handle
//! is dragged, and outlined in the window text at rest and the hover colour
//! otherwise, each at the frame contrast. An overlay or thin bar, which
//! lies over the content, has no separator. A bar with nothing to scroll
//! paints nothing. A thumb colour the app sets
//! (`ScrollBar::thumb_color`) takes the place of the window text and the
//! hover colour.
//!
//! ## Where this departs from Breeze
//!
//! - **D9, the handle's fill.** Breeze fills the idle handle with the window
//!   text at an effective 17.5 % over the window (25 % while the pointer is
//!   over the bar), 1.4:1, and a hovered one with the hover colour at 50 %;
//!   the tokens lift both to 3:1, keeping their hue (D9 of the colour
//!   mapping), and the handle is painted in them. Lifted, the idle and the
//!   pointer-over-the-bar handles are one colour.
//! - **The hover colour follows the bar, not the handle.** Breeze turns the
//!   handle to the hover colour when the pointer is over the handle itself;
//!   the style is told only that the pointer is over the bar.
//! - **The handle runs to the bar's ends.** Breeze keeps 3 px free at each
//!   end of a bar without buttons and runs the handle over what is left;
//!   the widget hit-tests its thumb over the whole bar and gives its style
//!   no say over the track, so the handle painted here starts at the top of
//!   the bar, where a press finds it.
//! - **The bar is as thick as the widget lays it out**, not Breeze's 21 px:
//!   the handle keeps its 8 px width and the separator its 1 px in whatever
//!   thickness the bar is given (see the crate's known limitations).

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::color_prop::ColorProp;
use teksilo_core::environment::LayoutDirection;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{
    ScrollBarOrientation, ScrollBarStyle, ScrollBarStyleConfig, ScrollBarVariant, Theme,
};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget};
use teksilo_core::widget_id::WidgetId;
use teksilo_kde_scheme::{ColorState, PaletteRole};
use teksilo_tokens::{Color, CornerRadius, InputTokens};

use crate::palette::KdePalette;
use crate::shape::BREEZE_SCROLL_HANDLE_RADIUS;
use crate::styles::metrics::{BreezeScrollBarMetrics, breeze_scroll_bar_metrics_for};

/// `[Hp]` The alpha a handle colour the app sets is painted at, at rest and
/// while the pointer is over the bar: Breeze's 0.5 x 0.7 x 0.5 and 0.5, the
/// effective alphas of its own handle.
pub const OVERRIDE_THUMB_ALPHA: [f32; 2] = [0.175, 0.5];

/// Breeze `ScrollBarStyle`.
///
/// [`Default`] is the Compact density; the preset installs
/// [`BreezeScrollBarStyle::for_tokens`] of the theme it builds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeScrollBarStyle {
    /// The scroll-bar dimensions this style paints and reports.
    pub metrics: BreezeScrollBarMetrics,
}

impl BreezeScrollBarStyle {
    /// The style at the density of `tokens`.
    pub fn for_tokens(tokens: &InputTokens) -> Self {
        Self {
            metrics: breeze_scroll_bar_metrics_for(tokens),
        }
    }
}

impl Default for BreezeScrollBarStyle {
    fn default() -> Self {
        Self::for_tokens(&InputTokens::default())
    }
}

impl ScrollBarStyle for BreezeScrollBarStyle {
    fn make_body(&self, cfg: &ScrollBarStyleConfig, ctx: &mut BuildContext) -> WidgetId {
        ctx.add(BreezeScrollBarBody {
            orientation: cfg.orientation,
            separator: matches!(cfg.variant, ScrollBarVariant::Permanent),
            min_thumb_length: cfg.min_thumb_length,
            scroll_ratio: cfg.scroll_ratio.clone(),
            viewport_ratio: cfg.viewport_ratio.clone(),
            is_hovered: cfg.is_hovered.clone(),
            is_dragging: cfg.is_dragging.clone(),
            is_idle: cfg.is_idle.clone(),
            thumb_color: cfg.thumb_color.clone(),
            metrics: self.metrics,
        })
    }

    /// [`BreezeScrollBarMetrics::min_handle_length`], 20 at every density.
    fn min_thumb_length(&self, _tokens: &InputTokens) -> f32 {
        self.metrics.min_handle_length
    }
}

/// The colours a Breeze scroll bar is painted in, in one paint state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeScrollBarColors {
    /// `[wcag]` D9: the handle at rest, `scrollbar_thumb`.
    pub thumb: Color,
    /// `[wcag]` D9: the handle with the pointer over the bar or while it is
    /// dragged, `scrollbar_thumb_hover`.
    pub thumb_hover: Color,
    /// `[Hp]` The handle's outline at rest: the window text at the frame
    /// contrast.
    pub outline: Color,
    /// `[Hp]` Its outline otherwise: the hover colour at the frame contrast.
    pub outline_hover: Color,
    /// `[Hp]` The separator: the view's text at the frame contrast.
    pub separator: Color,
}

impl BreezeScrollBarColors {
    /// The colours of `theme` as it paints in a window that is or is not
    /// active, with or without the high-contrast preference.
    ///
    /// The thumbs are the theme's projected tokens; the outlines and the
    /// separator come from [`KdePalette`]. Without it the outlines are the
    /// border and `border_focused`, and the separator the divider.
    pub fn resolve(
        theme: &Theme,
        window_active: bool,
        high_contrast: bool,
    ) -> BreezeScrollBarColors {
        let c = &theme.colors;
        let Some(palette) = theme.extension::<KdePalette>() else {
            return BreezeScrollBarColors {
                thumb: c.scrollbar_thumb,
                thumb_hover: c.scrollbar_thumb_hover,
                outline: c.border,
                outline_hover: c.border_focused,
                separator: c.divider,
            };
        };
        let state = if window_active {
            ColorState::Active
        } else {
            ColorState::Inactive
        };
        let fc = frame_contrast(palette, high_contrast);
        BreezeScrollBarColors {
            thumb: c.scrollbar_thumb,
            thumb_hover: c.scrollbar_thumb_hover,
            outline: palette.role(state, PaletteRole::WindowText).with_alpha(fc),
            outline_hover: palette
                .breeze_colors(state, high_contrast)
                .hover
                .with_alpha(fc),
            separator: palette.role(state, PaletteRole::Text).with_alpha(fc),
        }
    }

    /// The handle's fill and outline: hovered or dragged, or at rest.
    pub fn handle(&self, active: bool) -> (Color, Color) {
        if active {
            (self.thumb_hover, self.outline_hover)
        } else {
            (self.thumb, self.outline)
        }
    }
}

/// `[derived]` The frame contrast: 1 in the high-contrast scheme the palette
/// builds, as `KdePalette::breeze_colors` computes it.
fn frame_contrast(palette: &KdePalette, high_contrast: bool) -> f32 {
    if high_contrast {
        1.0
    } else {
        palette.frame_contrast() as f32
    }
}

/// The parts of a Breeze scroll bar across its thickness, in a bar whose
/// bounds are `bounds`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollBarParts {
    /// The separator line, when the bar has one.
    pub separator: Option<Rect>,
    /// The band the handle runs in: its offset across the bar and its width.
    pub handle_across: (f32, f32),
}

/// `[Hp]` Where the separator and the handle lie across a bar: the separator
/// on the leading edge, the handle centred in what it leaves, at most the
/// handle width wide.
pub fn scroll_bar_parts(
    bounds: Rect,
    orientation: ScrollBarOrientation,
    direction: LayoutDirection,
    separator: bool,
    m: &BreezeScrollBarMetrics,
) -> ScrollBarParts {
    let sep = if separator { m.separator } else { 0.0 };
    let (start, thickness) = match orientation {
        ScrollBarOrientation::Vertical => (bounds.x, bounds.width),
        ScrollBarOrientation::Horizontal => (bounds.y, bounds.height),
    };
    // The separator is on the edge toward the content: the top of a
    // horizontal bar, the left of a vertical one, its right in a
    // right-to-left window, where the bar sits left of the content.
    let separator_first = !(orientation == ScrollBarOrientation::Vertical
        && direction == LayoutDirection::RightToLeft);
    let (sep_at, region_start) = if separator_first {
        (start, start + sep)
    } else {
        (start + thickness - sep, start)
    };
    let region = (thickness - sep).max(0.0);
    let width = m.handle_width.min(region);
    let offset = ((region - width) * 0.5).floor();
    let separator = separator.then(|| match orientation {
        ScrollBarOrientation::Vertical => Rect::new(sep_at, bounds.y, sep, bounds.height),
        ScrollBarOrientation::Horizontal => Rect::new(bounds.x, sep_at, bounds.width, sep),
    });
    ScrollBarParts {
        separator,
        handle_across: (region_start + offset, width),
    }
}

/// The along-the-bar extent of the thumb in a track `track` long: its offset
/// and its length, the widget's own arithmetic, so the handle painted is the
/// one the bar hit-tests.
pub fn thumb_along(track: f32, viewport_ratio: f32, scroll_ratio: f32, min: f32) -> (f32, f32) {
    let len = (track * viewport_ratio.clamp(0.0, 1.0)).max(min).min(track);
    let offset = scroll_ratio.clamp(0.0, 1.0) * (track - len);
    (offset, len)
}

/// Paints a Breeze scroll bar: its separator and its handle.
struct BreezeScrollBarBody {
    orientation: ScrollBarOrientation,
    separator: bool,
    min_thumb_length: f32,
    scroll_ratio: Signal<f32>,
    viewport_ratio: Signal<f32>,
    is_hovered: Signal<bool>,
    is_dragging: Signal<bool>,
    is_idle: Signal<bool>,
    thumb_color: Option<ColorProp>,
    metrics: BreezeScrollBarMetrics,
}

impl std::fmt::Debug for BreezeScrollBarBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeScrollBarBody")
            .field("orientation", &self.orientation)
            .finish()
    }
}

impl Widget for BreezeScrollBarBody {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        for s in [&self.scroll_ratio, &self.viewport_ratio] {
            s.bind_to(id, registry, BindingLevel::RepaintOnly);
        }
        for s in [&self.is_hovered, &self.is_dragging, &self.is_idle] {
            s.bind_to(id, registry, BindingLevel::RepaintOnly);
        }
        Vec::new()
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        // The bar owns its thickness; this fills what it is given.
        proposal.resolve(0.0, 0.0).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        if self.is_idle.get() {
            return;
        }
        let theme = ctx.theme;
        let mut colors =
            BreezeScrollBarColors::resolve(theme, ctx.window_active, ctx.prefers_high_contrast);
        if let Some(prop) = &self.thumb_color {
            let tint = prop.resolve(theme, true);
            let fc = theme.extension::<KdePalette>().map_or(
                teksilo_kde_scheme::scheme::DEFAULT_FRAME_CONTRAST as f32,
                |p| frame_contrast(p, ctx.prefers_high_contrast),
            );
            colors.thumb = tint.with_alpha(OVERRIDE_THUMB_ALPHA[0]);
            colors.thumb_hover = tint.with_alpha(OVERRIDE_THUMB_ALPHA[1]);
            colors.outline = tint.with_alpha(fc);
            colors.outline_hover = colors.outline;
        }
        let parts = scroll_bar_parts(
            bounds,
            self.orientation,
            ctx.layout_direction,
            self.separator,
            &self.metrics,
        );
        if let Some(line) = parts.separator {
            canvas.fill_rect(line, colors.separator);
        }
        let (across, width) = parts.handle_across;
        if width <= 0.0 {
            return;
        }
        let track = match self.orientation {
            ScrollBarOrientation::Vertical => bounds.height,
            ScrollBarOrientation::Horizontal => bounds.width,
        };
        let (offset, len) = thumb_along(
            track,
            self.viewport_ratio.get(),
            self.scroll_ratio.get(),
            self.min_thumb_length,
        );
        let handle = match self.orientation {
            ScrollBarOrientation::Vertical => Rect::new(across, bounds.y + offset, width, len),
            ScrollBarOrientation::Horizontal => {
                // The start of the content is the right-hand end of the
                // track in a right-to-left window, where the bar's own hit
                // test puts the thumb.
                let x = if ctx.layout_direction == LayoutDirection::RightToLeft {
                    bounds.x + bounds.width - offset - len
                } else {
                    bounds.x + offset
                };
                Rect::new(x, across, len, width)
            }
        };
        let (fill, outline) = colors.handle(self.is_hovered.get() || self.is_dragging.get());
        let pen = theme.shape.border_width;
        let half = pen * 0.5;
        let stroked = Rect::new(
            handle.x + half,
            handle.y + half,
            (handle.width - pen).max(0.0),
            (handle.height - pen).max(0.0),
        );
        // `[Hp]` Half the handle's narrower side, at most 4.
        let radius = (0.5 * handle.width.min(handle.height)).min(BREEZE_SCROLL_HANDLE_RADIUS);
        let corner = CornerRadius::uniform(radius);
        canvas.fill_rounded_rect(stroked, corner, fill);
        if pen > 0.0 && outline.a() > 0.0 {
            canvas.stroke_rounded_rect(stroked, corner, outline, pen);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational: the bar announces itself.
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_kde_scheme::KdeColor;
    use teksilo_kde_scheme::math::mix;
    use teksilo_tokens::TargetDensity;

    fn rgba8(c: Color) -> [u8; 4] {
        KdeColor::from_color(c).to_rgba8()
    }

    fn m() -> BreezeScrollBarMetrics {
        crate::styles::metrics::breeze_scroll_bar_metrics()
    }

    /// Breeze's 21 px vertical bar: the separator in the leftmost column,
    /// the handle from 7 to 15, as Plasma painted it.
    #[test]
    fn breezes_own_bar_puts_the_handle_six_pixels_from_the_separator() {
        let p = scroll_bar_parts(
            Rect::new(0.0, 0.0, 21.0, 200.0),
            ScrollBarOrientation::Vertical,
            LayoutDirection::LeftToRight,
            true,
            &m(),
        );
        let sep = p.separator.unwrap();
        assert_eq!((sep.x, sep.width, sep.height), (0.0, 1.0, 200.0));
        assert_eq!(p.handle_across, (7.0, 8.0));
        // Right to left the separator moves to the right edge.
        let p = scroll_bar_parts(
            Rect::new(0.0, 0.0, 21.0, 200.0),
            ScrollBarOrientation::Vertical,
            LayoutDirection::RightToLeft,
            true,
            &m(),
        );
        assert_eq!(p.separator.unwrap().x, 20.0);
        assert_eq!(p.handle_across, (6.0, 8.0));
        // A horizontal bar's separator is its top line in either direction.
        let p = scroll_bar_parts(
            Rect::new(0.0, 0.0, 200.0, 21.0),
            ScrollBarOrientation::Horizontal,
            LayoutDirection::RightToLeft,
            true,
            &m(),
        );
        let sep = p.separator.unwrap();
        assert_eq!((sep.y, sep.height, sep.width), (0.0, 1.0, 200.0));
        assert_eq!(p.handle_across, (7.0, 8.0));
    }

    #[test]
    fn a_thinner_bar_keeps_the_handle_and_an_overlay_has_no_separator() {
        let p = scroll_bar_parts(
            Rect::new(100.0, 0.0, 12.0, 200.0),
            ScrollBarOrientation::Vertical,
            LayoutDirection::LeftToRight,
            true,
            &m(),
        );
        assert_eq!(p.handle_across, (102.0, 8.0));
        let p = scroll_bar_parts(
            Rect::new(100.0, 0.0, 12.0, 200.0),
            ScrollBarOrientation::Vertical,
            LayoutDirection::LeftToRight,
            false,
            &m(),
        );
        assert!(p.separator.is_none());
        assert_eq!(p.handle_across, (102.0, 8.0));
        // Narrower than the handle: the handle fills it.
        let p = scroll_bar_parts(
            Rect::new(0.0, 0.0, 4.0, 200.0),
            ScrollBarOrientation::Vertical,
            LayoutDirection::LeftToRight,
            false,
            &m(),
        );
        assert_eq!(p.handle_across, (0.0, 4.0));
    }

    #[test]
    fn the_thumb_is_the_widgets_and_never_shorter_than_twenty() {
        assert_eq!(thumb_along(200.0, 0.5, 0.0, 20.0), (0.0, 100.0));
        assert_eq!(thumb_along(200.0, 0.5, 1.0, 20.0), (100.0, 100.0));
        assert_eq!(thumb_along(200.0, 0.01, 0.5, 20.0), (90.0, 20.0));
        assert_eq!(thumb_along(10.0, 0.01, 0.5, 20.0), (0.0, 10.0));
        for density in [
            TargetDensity::Compact,
            TargetDensity::Comfortable,
            TargetDensity::Touch,
        ] {
            let tokens = InputTokens::for_density(density);
            assert_eq!(
                BreezeScrollBarStyle::for_tokens(&tokens).min_thumb_length(&tokens),
                20.0
            );
        }
    }

    /// The separator Plasma painted, the view's text at the frame contrast
    /// over the window, and the thumbs are the lifted tokens.
    #[test]
    fn the_colours_are_plasmas_lines_and_the_lifted_thumbs() {
        for (theme, separator) in [
            (crate::light(), [198, 200, 201, 255]),
            (crate::dark(), [76, 78, 80, 255]),
        ] {
            let c = BreezeScrollBarColors::resolve(&theme, true, false);
            let window = KdeColor::from_color(theme.colors.surface_main);
            let over = |x: Color| {
                rgba8(
                    mix(
                        window,
                        KdeColor::from_color(x.with_alpha(1.0)),
                        f64::from(x.a()),
                    )
                    .to_color(),
                )
            };
            let got = over(c.separator);
            for (g, w) in got.iter().zip(separator) {
                assert!(g.abs_diff(w) <= 1, "{got:?} against {separator:?}");
            }
            assert_eq!(c.thumb, theme.colors.scrollbar_thumb);
            assert_eq!(c.thumb_hover, theme.colors.scrollbar_thumb_hover);
            assert_eq!(c.handle(false), (c.thumb, c.outline));
            assert_eq!(c.handle(true), (c.thumb_hover, c.outline_hover));
            assert!((c.outline.a() - 0.2).abs() < 1e-6);
        }
    }

    #[test]
    fn the_bar_paints_from_the_roles_without_the_palette() {
        let mut bare = crate::light();
        bare.extensions.remove::<KdePalette>();
        let c = BreezeScrollBarColors::resolve(&bare, true, false);
        assert_eq!(c.outline, bare.colors.border);
        assert_eq!(c.separator, bare.colors.divider);
    }
}
