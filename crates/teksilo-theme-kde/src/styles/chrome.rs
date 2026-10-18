// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The paint vocabulary the Breeze button styles share: the button frame,
//! its one-pixel shadow and the keyboard focus frame.
//!
//! All three are painted by [`BreezeButtonChrome`], a leaf widget that
//! resolves every colour from [`PaintContext::theme`] at paint time, so a
//! live theme swap repaints with the new palette and the theme's inactive
//! and high-contrast projections reach the frame with no code here.
//!
//! ## The frame
//!
//! A Breeze button is a rounded rectangle on the node inset by one pixel on
//! every side (room for the shadow), stroked with a 1 px pen on that
//! rectangle inset by half the pen, so the outline lies inside it and its
//! radius is 4.5 ([`BREEZE_STROKED_FRAME_RADIUS`](crate::shape::BREEZE_STROKED_FRAME_RADIUS)).
//! Three buttons share it:
//!
//! - the **push button**: a flat fill of the button colour, outlined in the
//!   button colour moved toward the button text by the frame contrast, the
//!   outline turning to the highlight under the pointer or keyboard focus,
//!   and the fill moving 30 % toward the highlight while pressed;
//! - the **default button**, in an active window only: the button colour
//!   moved 20 % toward the highlight, outlined half-way between the
//!   highlight and the button outline;
//! - the **flat button** (a tool button or a flat push button): nothing at
//!   rest, a highlight outline with no fill under the pointer, the highlight
//!   at 30 % while pressed and the button text at 12.5 % while checked.
//!
//! The push and default buttons cast a 1 px shadow, black at 12.5 % one
//! pixel below the frame (`shadow_xs`), only in an active window and never
//! while pressed, checked or disabled; a flat button never casts one.
//!
//! ## Which highlight
//!
//! Breeze outlines and fills a button with the **active** window's
//! highlight whatever the window state, so a button hovered in a backdrop
//! window still lights up in the accent, not in the muted selection colour
//! an inactive Breeze window paints its selections with. While the theme
//! paints its [`KdePalette`]'s own accent, that highlight comes from the
//! palette's active tokens for the same contrast preference; a theme whose
//! accent the app changed paints its own accent, as projected. A disabled
//! button uses the disabled highlight.
//!
//! ## Without the palette
//!
//! The neutral button colours come from the [`KdePalette`] extension, which
//! carries the button colour, its text and its outline, three colours
//! `ColorTokens` has no slot for. A theme without the extension paints the
//! frame from its own roles instead: the content surface, the primary text
//! and the border, so a Breeze button dropped into another theme still
//! paints a sane frame.
//!
//! ## The focus frame
//!
//! Keyboard focus adds Breeze's 2 px focus frame, from 1 px inside the node
//! to 1 px outside it, its inner edge at the control radius, which is what
//! the shape tokens say (`focus_ring_width` 2, `focus_ring_offset` -1).
//! Breeze paints it in the highlight at 30 %; this paints the `focus_ring`
//! token, the focus colour lifted to 3:1 and opaque (D5 of the colour
//! mapping), so the indicator meets WCAG SC 1.4.11 on its own.

use teksilo_canvas::{Canvas, Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::binding::BindingLevel;
use teksilo_core::build_context::BuildContext;
use teksilo_core::signal::Signal;
use teksilo_core::styles::{FocusRingStroke, PaintState, Theme};
use teksilo_core::widget::{LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement};
use teksilo_core::widget_id::WidgetId;
use teksilo_kde_scheme::ColorState;
use teksilo_kde_scheme::KdeColor;
use teksilo_kde_scheme::math::mix;
use teksilo_tokens::{Color, CornerRadius, ShapeTokens};

use crate::color::face_under_primary_text;
use crate::palette::KdePalette;

/// `[Hp]` How far a default button's fill moves from the button colour
/// toward the highlight.
pub const DEFAULT_BUTTON_FILL: f64 = 0.2;
/// `[Hp]` How far a default button's outline moves from the highlight toward
/// the button outline.
pub const DEFAULT_BUTTON_OUTLINE: f64 = 0.5;
/// `[Hp]` The alpha of a pressed flat button's highlight fill.
pub const FLAT_PRESSED_ALPHA: f32 = 0.3;
/// `[Hp]` How far a checked button's fill moves toward the button text, and
/// the alpha of a checked flat button's button-text fill.
pub const CHECKED_FILL: f32 = 0.125;
/// `[Hp]` The inset of a button's frame inside its node, which leaves room
/// for the 1 px shadow below it.
pub const SHADOW_INSET: f32 = 1.0;

/// Which Breeze button a surface is painted as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreezeButtonKind {
    /// The push button: button-colour fill, outline, shadow.
    Push,
    /// The default button: the push button with a fill moved toward the
    /// highlight, in an active window. In an inactive window it is a push
    /// button.
    Default,
    /// The flat button, a tool button or a flat push button: nothing at
    /// rest and never a shadow.
    Flat,
    /// `[unpublished]` A push button filled with the accent and its hover
    /// and press shades, for a control whose label is the on-accent text and
    /// whose style has no say over it (a `SplitButton`'s filled variants).
    /// Breeze has no accent-filled button; see
    /// [`crate::styles::split_button`]. Its frame is outlined in its own
    /// face colour, so it covers the push button's band, from 1 px inside
    /// the node, and hides the shadow's sides as the push button's outline
    /// does.
    Accent,
}

impl BreezeButtonKind {
    /// Whether this kind can cast the 1 px shadow.
    pub fn casts_shadow(self) -> bool {
        !matches!(self, BreezeButtonKind::Flat)
    }
}

/// A button's interaction state, in Breeze's precedence order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BreezeButtonState {
    /// At rest.
    #[default]
    Rest,
    /// Hovered or showing keyboard focus: Breeze paints both the same.
    Highlighted,
    /// Pressed.
    Pressed,
    /// Disabled.
    Disabled,
}

impl BreezeButtonState {
    /// Collapse the style configuration's four booleans into a state:
    /// disabled first, then pressed, then hovered or focused.
    ///
    /// Each input may be a derived signal, so this only composes with `zip`
    /// and `map`.
    pub fn derive(
        is_disabled: &Signal<bool>,
        is_pressed: &Signal<bool>,
        is_hovered: &Signal<bool>,
        is_focused: &Signal<bool>,
    ) -> Signal<BreezeButtonState> {
        is_disabled
            .zip3(is_pressed, is_hovered)
            .zip(is_focused)
            .map(|((disabled, pressed, hovered), focused)| {
                if *disabled {
                    BreezeButtonState::Disabled
                } else if *pressed {
                    BreezeButtonState::Pressed
                } else if *hovered || *focused {
                    BreezeButtonState::Highlighted
                } else {
                    BreezeButtonState::Rest
                }
            })
    }
}

/// The colours a Breeze button is painted from, in one paint state.
///
/// The faces a label is set on (the face at rest, pressed, checked and the
/// default button's) carry the primary text the label is painted in at the
/// text floor of the paint state, 4.5:1 or 7:1 under high contrast: the
/// button colour is the surface the colour tokens lift that text on, and
/// a face mixed from it is moved, as an accent wash is (D1 of the colour
/// mapping), until the text reaches the floor on it. In stock Breeze none
/// moves. A disabled button's faces are Breeze's, its text being exempt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreezeButtonColors {
    /// The button colour, a push button's face.
    pub button: Color,
    /// The button text, which a checked button's fill moves toward.
    pub button_text: Color,
    /// The push button's outline.
    pub button_outline: Color,
    /// The highlight a hovered, focused or pressed button is outlined in:
    /// the active window's (see the module documentation), or the disabled
    /// one for a disabled button.
    pub highlight: Color,
    /// A pressed push button's face: the button colour 30 % of the way to
    /// the highlight, the theme's `surface_pressed`.
    pub pressed: Color,
    /// `[Hp]` The default button's face: the button colour moved toward the
    /// highlight.
    pub default_face: Color,
    /// `[Hp]` A checked push button's face: the button colour moved toward
    /// the button text.
    pub checked_face: Color,
    /// The accent family, for [`BreezeButtonKind::Accent`].
    pub accent: Color,
    /// The hovered accent.
    pub accent_hover: Color,
    /// The pressed accent.
    pub accent_pressed: Color,
    /// The disabled accent.
    pub accent_disabled: Color,
}

impl BreezeButtonColors {
    /// The colours of `theme` as it paints in a window that is or is not
    /// active, with or without the high-contrast preference, for an enabled
    /// or a disabled button.
    ///
    /// `theme` is the theme the paint walker hands a widget, already
    /// projected for that state; see the module documentation for where each
    /// colour comes from.
    pub fn resolve(
        theme: &Theme,
        window_active: bool,
        high_contrast: bool,
        disabled: bool,
    ) -> BreezeButtonColors {
        let c = &theme.colors;
        let accent_family =
            |button: Color, button_text: Color, outline: Color| BreezeButtonColors {
                button,
                button_text,
                button_outline: outline,
                highlight: if disabled {
                    c.accent_disabled
                } else {
                    c.accent
                },
                pressed: c.surface_pressed,
                default_face: button,
                checked_face: button,
                accent: c.accent,
                accent_hover: c.accent_hover,
                accent_pressed: c.accent_pressed,
                accent_disabled: c.accent_disabled,
            };
        let mut colors = match theme.extension::<KdePalette>() {
            // No KDE palette: the theme's own roles stand in for the button
            // colours it has no slot for.
            None if disabled => {
                accent_family(c.surface_disabled, c.text_primary, c.border_disabled)
            }
            None => accent_family(c.surface_content, c.text_primary, c.border),
            Some(palette) => {
                let state = if disabled {
                    ColorState::Disabled
                } else if window_active {
                    ColorState::Active
                } else {
                    ColorState::Inactive
                };
                let b = palette.breeze_colors(state, high_contrast);
                let mut colors = accent_family(b.button, b.button_text, b.button_outline);
                // `[Hp]` The active window's highlight, while the theme
                // paints the palette's own accent for this state.
                let painted = PaintState {
                    window_active,
                    high_contrast,
                };
                if c.accent == palette.tokens(painted).accent {
                    let active = palette.tokens(PaintState {
                        window_active: true,
                        high_contrast,
                    });
                    if !disabled {
                        colors.highlight = active.accent;
                    }
                    colors.pressed = active.surface_pressed;
                }
                colors
            }
        };
        // `[Hp]` The default button's face moves toward the highlight, a
        // checked one's toward the button text.
        colors.default_face = kde_mix(colors.button, colors.highlight, DEFAULT_BUTTON_FILL);
        colors.checked_face = kde_mix(colors.button, colors.button_text, f64::from(CHECKED_FILL));
        if !disabled {
            // `[wcag]` D1: each face mixed from the button colour carries the
            // label's primary text, as the button colour itself does.
            let under_label = |face: Color| face_under_primary_text(face, c, high_contrast);
            colors.pressed = under_label(colors.pressed);
            colors.default_face = under_label(colors.default_face);
            colors.checked_face = under_label(colors.checked_face);
        }
        colors
    }

    /// The default button's outline: half-way from the highlight to the
    /// button outline.
    pub fn default_outline(&self) -> Color {
        kde_mix(self.highlight, self.button_outline, DEFAULT_BUTTON_OUTLINE)
    }
}

/// KDE's mix of two colours, in its 16-bit arithmetic.
pub(crate) fn kde_mix(a: Color, b: Color, t: f64) -> Color {
    mix(KdeColor::from_color(a), KdeColor::from_color(b), t).to_color()
}

/// What a button paints in one state: a face, an outline, a shadow, each
/// optional.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ButtonPaint {
    /// The fill of the frame, or nothing.
    pub face: Option<Color>,
    /// The 1 px outline of the frame, or nothing.
    pub outline: Option<Color>,
    /// Whether the 1 px shadow is cast below the frame.
    pub shadow: bool,
}

impl ButtonPaint {
    const NOTHING: ButtonPaint = ButtonPaint {
        face: None,
        outline: None,
        shadow: false,
    };
}

/// `[Hp]` What a `kind` button paints in `state`, checked or not, in a
/// window that is or is not active, from `c`.
///
/// The rules of the module documentation. Pressing wins over checking,
/// which wins over hover and focus; a checked button keeps its plain outline
/// under the pointer.
pub fn button_paint(
    kind: BreezeButtonKind,
    state: BreezeButtonState,
    checked: bool,
    window_active: bool,
    c: &BreezeButtonColors,
) -> ButtonPaint {
    use BreezeButtonState::{Disabled, Highlighted, Pressed, Rest};
    // The default button is one only in an active window.
    let kind = match kind {
        BreezeButtonKind::Default if !window_active => BreezeButtonKind::Push,
        other => other,
    };
    let shadow =
        kind.casts_shadow() && window_active && !checked && matches!(state, Rest | Highlighted);
    let paint = |face: Option<Color>, outline: Option<Color>| ButtonPaint {
        face,
        outline,
        shadow,
    };
    match kind {
        BreezeButtonKind::Push | BreezeButtonKind::Default => {
            let (rest_face, rest_outline) = if kind == BreezeButtonKind::Default {
                (c.default_face, c.default_outline())
            } else {
                (c.button, c.button_outline)
            };
            match state {
                Pressed => paint(Some(c.pressed), Some(c.highlight)),
                _ if checked => paint(Some(c.checked_face), Some(c.button_outline)),
                Highlighted => paint(Some(rest_face), Some(c.highlight)),
                Rest | Disabled => paint(Some(rest_face), Some(rest_outline)),
            }
        }
        BreezeButtonKind::Flat => match state {
            Pressed => paint(
                Some(c.highlight.with_alpha(FLAT_PRESSED_ALPHA)),
                Some(c.highlight),
            ),
            _ if checked => paint(
                Some(c.button_text.with_alpha(CHECKED_FILL)),
                Some(c.button_outline),
            ),
            Highlighted => paint(None, Some(c.highlight)),
            Rest | Disabled => ButtonPaint::NOTHING,
        },
        BreezeButtonKind::Accent => {
            let face = match state {
                Rest => c.accent,
                Highlighted => c.accent_hover,
                Pressed => c.accent_pressed,
                Disabled => c.accent_disabled,
            };
            paint(Some(face), Some(face))
        }
    }
}

/// `[Hp]` The rectangle a button's frame is filled and stroked on: the node
/// inset by [`SHADOW_INSET`], then by half the pen, so the outline lies
/// inside the inset rectangle.
pub fn frame_rect(bounds: Rect, pen: f32) -> Rect {
    let d = SHADOW_INSET + pen * 0.5;
    Rect::new(
        bounds.x + d,
        bounds.y + d,
        (bounds.width - d * 2.0).max(0.0),
        (bounds.height - d * 2.0).max(0.0),
    )
}

/// The radius the frame of a theme's controls is stroked with: the control
/// radius less half the pen, 4.5 for Breeze.
pub fn frame_radius(shape: &ShapeTokens) -> f32 {
    (shape.radius_control - shape.border_width * 0.5).max(0.0)
}

/// Paint `paint` into the node `bounds`: the shadow, then the face, then the
/// outline, all on [`frame_rect`].
pub fn paint_button(canvas: &mut Canvas, bounds: Rect, paint: &ButtonPaint, theme: &Theme) {
    let shape = &theme.shape;
    let pen = shape.border_width;
    let rect = frame_rect(bounds, pen);
    let corner = CornerRadius::uniform(frame_radius(shape));
    if paint.shadow {
        // `[Hp]` The frame's outline in the shadow colour, moved down by the
        // shadow's offset: the face covers all of it but the line below.
        let s = &shape.shadow_xs;
        let moved = Rect::new(
            rect.x + s.offset_x,
            rect.y + s.offset_y,
            rect.width,
            rect.height,
        );
        canvas.stroke_rounded_rect(moved, corner, s.color, pen);
    }
    if let Some(face) = paint.face
        && face.a() > 0.0
    {
        canvas.fill_rounded_rect(rect, corner, face);
    }
    if let Some(outline) = paint.outline
        && outline.a() > 0.0
        && pen > 0.0
    {
        canvas.stroke_rounded_rect(rect, corner, outline, pen);
    }
}

/// The stroke of the focus frame around the node `bounds`: its inner edge
/// `focus_ring_offset` from the node edge with the control radius, the
/// stroke as wide as `focus_ring_width`.
pub fn focus_frame(bounds: Rect, shape: &ShapeTokens) -> FocusRingStroke {
    // At its inner edge the ring has the control radius, so at the node edge
    // it has that radius less the (signed) offset.
    let edge_radius = (shape.radius_control - shape.focus_ring_offset).max(0.0);
    FocusRingStroke::around(bounds, edge_radius, shape)
}

/// Paint the focus frame around the node `bounds` in the theme's
/// `focus_ring` colour.
pub fn paint_focus_frame(canvas: &mut Canvas, bounds: Rect, theme: &Theme) {
    let shape = &theme.shape;
    let color = theme.colors.focus_ring;
    if color.a() <= 0.0 || shape.focus_ring_width <= 0.0 {
        return;
    }
    let ring = focus_frame(bounds, shape);
    canvas.stroke_rounded_rect(
        ring.rect,
        CornerRadius::uniform(ring.corner_radius),
        color,
        shape.focus_ring_width,
    );
}

/// The chrome layer of a Breeze button: its frame, shadow and focus frame,
/// painted behind whatever content the style stacks on top.
///
/// Sized like `RectWidget`: it takes whatever the proposal offers and
/// reports zero when unbounded, so it can sit in a `ZStack` without changing
/// the control's size.
pub struct BreezeButtonChrome {
    kind: BreezeButtonKind,
    state: Signal<BreezeButtonState>,
    /// `true` while the button shows its focus frame.
    show_focus_frame: Signal<bool>,
    /// The checked state of a checkable button.
    checked: Option<Signal<bool>>,
}

impl BreezeButtonChrome {
    /// A chrome of `kind` following `state`, showing the focus frame while
    /// `show_focus_frame` is set.
    pub fn new(
        kind: BreezeButtonKind,
        state: Signal<BreezeButtonState>,
        show_focus_frame: Signal<bool>,
    ) -> Self {
        Self {
            kind,
            state,
            show_focus_frame,
            checked: None,
        }
    }

    /// Paint the checked state while `checked` is set.
    pub fn checked(mut self, checked: Option<Signal<bool>>) -> Self {
        self.checked = checked;
        self
    }
}

impl std::fmt::Debug for BreezeButtonChrome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BreezeButtonChrome")
            .field("kind", &self.kind)
            .finish()
    }
}

impl Widget for BreezeButtonChrome {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.state.bind_to(id, registry, BindingLevel::RepaintOnly);
        self.show_focus_frame
            .bind_to(id, registry, BindingLevel::RepaintOnly);
        if let Some(checked) = &self.checked {
            checked.bind_to(id, registry, BindingLevel::RepaintOnly);
        }
        vec![]
    }

    fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        let state = self.state.get();
        let checked = self.checked.as_ref().is_some_and(Signal::get);
        let colors = BreezeButtonColors::resolve(
            ctx.theme,
            ctx.window_active,
            ctx.prefers_high_contrast,
            state == BreezeButtonState::Disabled,
        );
        let paint = button_paint(self.kind, state, checked, ctx.window_active, &colors);
        paint_button(canvas, bounds, &paint, ctx.theme);
        if self.show_focus_frame.get() {
            paint_focus_frame(canvas, bounds, ctx.theme);
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Presentational: the owning control emits the real node.
        builder.set_hidden();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_kde_scheme::PaletteRole;

    fn rgba8(c: Color) -> [u8; 4] {
        KdeColor::from_color(c).to_rgba8()
    }

    fn colors(theme: &Theme, window_active: bool, disabled: bool) -> BreezeButtonColors {
        BreezeButtonColors::resolve(theme, window_active, false, disabled)
    }

    #[test]
    fn the_state_follows_breezes_precedence() {
        let disabled = Signal::new(false);
        let pressed = Signal::new(false);
        let hovered = Signal::new(false);
        let focused = Signal::new(false);
        let state = BreezeButtonState::derive(&disabled, &pressed, &hovered, &focused);
        assert_eq!(state.get(), BreezeButtonState::Rest);
        focused.set(true);
        assert_eq!(
            state.get(),
            BreezeButtonState::Highlighted,
            "focus lights it"
        );
        focused.set(false);
        hovered.set(true);
        assert_eq!(state.get(), BreezeButtonState::Highlighted, "so does hover");
        pressed.set(true);
        assert_eq!(state.get(), BreezeButtonState::Pressed, "press beats hover");
        disabled.set(true);
        assert_eq!(
            state.get(),
            BreezeButtonState::Disabled,
            "disabled beats all"
        );
    }

    /// The Breeze Light values of the specification's rules, and the
    /// colours Plasma 6.8 paints them in (the golden pixel samples).
    #[test]
    fn breeze_light_paints_the_specifications_colours() {
        let t = crate::light();
        let c = colors(&t, true, false);
        assert_eq!(rgba8(c.button), [252, 252, 252, 255]);
        assert_eq!(rgba8(c.button_outline), [209, 209, 210, 255]);
        assert_eq!(rgba8(c.highlight), [61, 174, 233, 255]);
        assert_eq!(rgba8(c.pressed), [195, 229, 246, 255]);
        assert_eq!(rgba8(c.default_face), [214, 236, 248, 255]);
        assert_eq!(rgba8(c.default_outline()), [135, 192, 221, 255]);

        let d = colors(&crate::dark(), true, false);
        assert_eq!(rgba8(d.button), [41, 44, 48, 255]);
        assert_eq!(rgba8(d.button_outline), [83, 86, 89, 255]);
        assert_eq!(rgba8(d.default_face), [45, 70, 85, 255]);
        assert_eq!(rgba8(d.default_outline()), [72, 130, 161, 255]);
        assert_eq!(rgba8(d.pressed), [47, 83, 104, 255]);
    }

    #[test]
    fn a_hovered_button_in_a_backdrop_window_is_outlined_in_the_active_highlight() {
        let t = crate::light();
        let inactive = t.for_paint_state(PaintState {
            window_active: false,
            high_contrast: false,
        });
        assert_ne!(inactive.colors.accent, t.colors.accent, "premise: it mutes");
        let c = colors(&inactive, false, false);
        assert_eq!(c.highlight, t.colors.accent);
        assert_eq!(c.pressed, t.colors.surface_pressed);
        // A disabled button uses the disabled highlight.
        assert_eq!(colors(&t, true, true).highlight, t.colors.accent_disabled);
    }

    #[test]
    fn an_accent_the_app_changed_is_painted_as_projected() {
        let mut t = crate::light();
        t.colors.accent = Color::from_hex("#FF0000");
        let c = colors(&t, true, false);
        assert_eq!(c.highlight, t.colors.accent);
    }

    #[test]
    fn the_high_contrast_outline_is_the_button_text() {
        let t = crate::light();
        let hc = t.for_paint_state(PaintState {
            window_active: true,
            high_contrast: true,
        });
        let c = BreezeButtonColors::resolve(&hc, true, true, false);
        assert_eq!(c.button_outline, c.button_text);
        assert_eq!(c.highlight, hc.colors.accent, "the lifted accent");
    }

    #[test]
    fn a_theme_without_the_palette_paints_from_its_roles() {
        let t = teksilo_core::presets::intui::light();
        let c = colors(&t, true, false);
        assert_eq!(c.button, t.colors.surface_content);
        assert_eq!(c.button_outline, t.colors.border);
        assert_eq!(c.highlight, t.colors.accent);
        let d = colors(&t, true, true);
        assert_eq!(d.button, t.colors.surface_disabled);
        assert_eq!(d.button_outline, t.colors.border_disabled);
    }

    #[test]
    fn the_disabled_push_button_paints_the_disabled_button_colours() {
        let t = crate::light();
        let p = t.extension::<KdePalette>().unwrap();
        let c = colors(&t, true, true);
        assert_eq!(c.button, p.role(ColorState::Disabled, PaletteRole::Button));
        assert_eq!(
            c.button_outline,
            p.colors(ColorState::Disabled).button_outline
        );
        // Plasma 6.8 paints 240,240,240 outlined in 226,226,226.
        assert_eq!(rgba8(c.button), [240, 240, 240, 255]);
        assert_eq!(rgba8(c.button_outline), [226, 226, 226, 255]);
        let paint = button_paint(
            BreezeButtonKind::Push,
            BreezeButtonState::Disabled,
            false,
            true,
            &c,
        );
        assert_eq!(paint.face, Some(c.button));
        assert!(!paint.shadow, "a disabled button casts no shadow");
    }

    #[test]
    fn the_paint_rules_for_each_kind_and_state() {
        use BreezeButtonKind::{Accent, Default, Flat, Push};
        use BreezeButtonState::{Disabled, Highlighted, Pressed, Rest};
        let c = colors(&crate::light(), true, false);
        let p = |kind, state, checked, active| button_paint(kind, state, checked, active, &c);

        assert_eq!(
            p(Push, Rest, false, true),
            ButtonPaint {
                face: Some(c.button),
                outline: Some(c.button_outline),
                shadow: true
            }
        );
        assert_eq!(p(Push, Highlighted, false, true).outline, Some(c.highlight));
        assert!(p(Push, Highlighted, false, true).shadow);
        let pressed = p(Push, Pressed, false, true);
        assert_eq!(
            (pressed.face, pressed.outline, pressed.shadow),
            (Some(c.pressed), Some(c.highlight), false)
        );
        let checked = p(Push, Highlighted, true, true);
        assert_eq!(
            (checked.face, checked.outline, checked.shadow),
            (Some(c.checked_face), Some(c.button_outline), false)
        );
        // The shadow is an active window's only.
        assert!(!p(Push, Rest, false, false).shadow);

        let default = p(Default, Rest, false, true);
        assert_eq!(default.face, Some(c.default_face));
        assert_eq!(default.outline, Some(c.default_outline()));
        assert!(default.shadow);
        // In an inactive window the default button is a push button.
        assert_eq!(p(Default, Rest, false, false), p(Push, Rest, false, false));

        assert_eq!(p(Flat, Rest, false, true), ButtonPaint::NOTHING);
        assert_eq!(p(Flat, Disabled, false, true), ButtonPaint::NOTHING);
        assert_eq!(
            p(Flat, Highlighted, false, true),
            ButtonPaint {
                face: None,
                outline: Some(c.highlight),
                shadow: false
            }
        );
        let flat_pressed = p(Flat, Pressed, false, true);
        assert_eq!(
            flat_pressed.face,
            Some(c.highlight.with_alpha(FLAT_PRESSED_ALPHA))
        );
        assert!(!flat_pressed.shadow);
        let flat_checked = p(Flat, Rest, true, true);
        assert_eq!(
            flat_checked.face,
            Some(c.button_text.with_alpha(CHECKED_FILL))
        );
        assert_eq!(flat_checked.outline, Some(c.button_outline));

        assert_eq!(p(Accent, Rest, false, true).face, Some(c.accent));
        assert_eq!(
            p(Accent, Highlighted, false, true).face,
            Some(c.accent_hover)
        );
        assert_eq!(p(Accent, Pressed, false, true).face, Some(c.accent_pressed));
        assert_eq!(
            p(Accent, Disabled, false, true).face,
            Some(c.accent_disabled)
        );
        assert!(p(Accent, Rest, false, true).shadow);
        // The accent frame is outlined in its own face, every state.
        for state in [Rest, Highlighted, Pressed, Disabled] {
            let accent = p(Accent, state, false, true);
            assert_eq!(accent.outline, accent.face, "{state:?}");
        }
    }

    #[test]
    fn the_flat_and_checked_fills_are_breezes_samples() {
        // Over the Breeze Light window, 239,240,241, as Plasma 6.8 paints a
        // pressed and a checked tool button: within one step a channel, the
        // margin by which Qt's raster painter rounds an 8-bit blend its own
        // way.
        let c = colors(&crate::light(), true, false);
        let window = Color::from_hex("#EFF0F1");
        let over = |top: Color| {
            let a = top.a();
            let ch = |f: f32, g: f32| ((f * a + g * (1.0 - a)) * 255.0).round() as i32;
            [
                ch(top.r(), window.r()),
                ch(top.g(), window.g()),
                ch(top.b(), window.b()),
            ]
        };
        let near = |got: [i32; 3], want: [i32; 3]| {
            assert!(
                got.iter().zip(want).all(|(g, w)| (g - w).abs() <= 1),
                "{got:?} is not within one step of {want:?}"
            );
        };
        near(
            over(c.highlight.with_alpha(FLAT_PRESSED_ALPHA)),
            [185, 220, 238],
        );
        near(
            over(c.button_text.with_alpha(CHECKED_FILL)),
            [213, 215, 216],
        );
    }

    #[test]
    fn the_frame_lies_inside_the_node_with_room_for_the_shadow() {
        let t = crate::light();
        let node = Rect::new(0.0, 0.0, 84.0, 34.0);
        let r = frame_rect(node, t.shape.border_width);
        assert_eq!(r, Rect::new(1.5, 1.5, 81.0, 31.0));
        assert_eq!(frame_radius(&t.shape), 4.5);
        // A degenerate node does not invert.
        let tiny = frame_rect(Rect::new(0.0, 0.0, 2.0, 2.0), 1.0);
        assert!(tiny.width >= 0.0 && tiny.height >= 0.0);
    }

    #[test]
    fn the_focus_frame_runs_from_one_pixel_inside_to_one_pixel_outside() {
        let t = crate::light();
        let node = Rect::new(10.0, 10.0, 84.0, 34.0);
        let ring = focus_frame(node, &t.shape);
        // Centred on the node edge, 2 px wide: from 1 px inside to 1 px
        // outside, inner radius 5 and outer radius 7.
        assert_eq!(ring.rect, node);
        assert_eq!(ring.corner_radius, 6.0);
        assert_eq!(
            ring.corner_radius - t.shape.focus_ring_width * 0.5,
            t.shape.radius_control
        );
        assert_eq!(
            ring.corner_radius + t.shape.focus_ring_width * 0.5,
            crate::shape::BREEZE_FOCUS_RING_OUTER_RADIUS
        );
    }

    #[test]
    fn only_the_flat_kind_never_casts_a_shadow() {
        assert!(!BreezeButtonKind::Flat.casts_shadow());
        for k in [
            BreezeButtonKind::Push,
            BreezeButtonKind::Default,
            BreezeButtonKind::Accent,
        ] {
            assert!(k.casts_shadow());
        }
    }
}
