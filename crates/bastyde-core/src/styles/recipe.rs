// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tier-2 paint-recipe primitives — Shape, Fill, Border, Shadow,
//! per-state envelope, and the [`WidgetState`] enum the envelope
//! resolves against.
//!
//! These types are **pure data**: every field is plain (no signals, no
//! closures, no `Rc`-wrapped reactive state). That gives them three
//! properties the rest of the styling system relies on:
//!
//! - **`Send + Sync + 'static`** so recipes can be held in
//!   `Arc`-based serialization contexts (Inspector JSON Export,
//!   future `ImageTheme` TOML manifest).
//! - **`Serialize` / `Deserialize`** so themes can round-trip through
//!   the inspector's JSON Export/Import and through the future
//!   `ImageTheme` TOML manifest.
//! - **Cheap `Clone`** so the chrome composer can pull out
//!   `PerStateRecipe::resolve(state).clone()` without thinking about
//!   it.
//!
//! Reactivity is layered on top, not baked in: when a widget's state
//! signal changes, the chrome composer calls
//! [`PerStateRecipe::resolve`] for the new state and re-paints with
//! the resolved recipe. Theme swaps go through
//! [`RecipeColor::resolve`] which reads the live `ColorTokens` from
//! the theme signal — so a recipe holding `RecipeColor::Surface(Hover)`
//! repaints automatically when the theme changes, even though the
//! recipe itself never moved.

use serde::{Deserialize, Serialize};

use bastyde_canvas::{Rect, Vec2};
use bastyde_tokens::{BorderRole, Color, ColorTokens, CornerRadius, SurfaceRole, TextRole};

use crate::styles::Theme;

// ─── WidgetState ────────────────────────────────────────────────────────────

/// Discrete interaction state used to index a [`PerStateRecipe`]. The
/// chrome composer derives the active state from the widget's
/// boolean signals (priority chain: Disabled > Pressed > Focused >
/// Hovered > Idle).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize)]
pub enum WidgetState {
    #[default]
    Idle,
    Hovered,
    Pressed,
    Focused,
    Disabled,
}

// ─── RecipeColor ────────────────────────────────────────────────────────────

/// Send+Sync, serializable color value usable inside a recipe.
///
/// Three flavours, all `Copy`:
/// - **`Static(Color)`** — frozen literal color.
/// - **`Surface(SurfaceRole)`** / `Border(BorderRole)` / `Text(TextRole)` —
///   theme-aware role; resolves against the current `ColorTokens` at
///   paint time (after a theme swap, the same recipe paints the new
///   role-resolved color without rebuilding).
///
/// Distinct from [`crate::ColorProp`] (which carries a `Signal` and is
/// `!Send`). Recipes use `RecipeColor`; widget builders that accept
/// `impl Into<ColorProp>` continue to use `ColorProp` directly.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RecipeColor {
    Static(Color),
    Surface(SurfaceRole),
    Border(BorderRole),
    Text(TextRole),
}

impl RecipeColor {
    pub fn resolve(self, theme: &Theme) -> Color {
        self.resolve_with(&theme.colors)
    }

    /// Variant of [`Self::resolve`] that takes [`ColorTokens`]
    /// directly. Useful for tight inner loops where the caller has a
    /// borrow on the color tokens already.
    pub fn resolve_with(self, colors: &ColorTokens) -> Color {
        match self {
            RecipeColor::Static(c) => c,
            RecipeColor::Surface(r) => r.resolve(colors),
            RecipeColor::Border(r) => r.resolve(colors),
            RecipeColor::Text(r) => r.resolve(colors),
        }
    }
}

impl From<Color> for RecipeColor {
    fn from(c: Color) -> Self {
        Self::Static(c)
    }
}
impl From<SurfaceRole> for RecipeColor {
    fn from(r: SurfaceRole) -> Self {
        Self::Surface(r)
    }
}
impl From<BorderRole> for RecipeColor {
    fn from(r: BorderRole) -> Self {
        Self::Border(r)
    }
}
impl From<TextRole> for RecipeColor {
    fn from(r: TextRole) -> Self {
        Self::Text(r)
    }
}

// ─── ShapeRecipe ────────────────────────────────────────────────────────────

/// The outline a recipe paints into. `Pill` / `Circle` resolve their
/// corner radius against the bounding rect at paint time. `CustomPath`
/// is intentionally absent for now: the path-builder closure can't be
/// `Send + Sync + Serialize`, and the IntUI default recipes don't need
/// it. A future variant will land if app code grows the need.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ShapeRecipe {
    /// Axis-aligned rectangle with per-corner radii. Use
    /// [`CornerRadius::uniform`] for a single-radius rounded rect.
    Rect { corner_radius: CornerRadius },
    /// `corner_radius = min(width, height) / 2` — fully-rounded ends.
    Pill,
    /// Force `width == height` and `corner_radius = width / 2`.
    Circle,
}

impl ShapeRecipe {
    /// Convenience: rounded rect with a uniform corner radius.
    pub fn rounded(radius: f32) -> Self {
        Self::Rect {
            corner_radius: CornerRadius::uniform(radius),
        }
    }

    /// Convenience: sharp-cornered rect.
    pub fn rect() -> Self {
        Self::Rect {
            corner_radius: CornerRadius::uniform(0.0),
        }
    }
}

// ─── FillRecipe ─────────────────────────────────────────────────────────────

/// What the inside of the [`ShapeRecipe`] is filled with.
///
/// `Solid` and `StateLayer` both resolve to a flat [`Color`] (the latter
/// by compositing an `overlay` over a `base` at a given alpha — the
/// Material-3 / Fluent "state layer" model). `LinearGradient` /
/// `RadialGradient` describe true gradients; the renderer paints them via
/// the SDF gradient pipeline once a `PaintProp` carries them (see
/// `resolve_fill_to_paint`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FillRecipe {
    /// Single flat color.
    Solid(RecipeColor),
    /// `overlay` composited over `base` at `alpha` — a translucent
    /// "state layer" (M3 hover = 8 %, pressed = 12 % on-color over the
    /// base fill). Resolves to a flat [`Color`], so it flows through the
    /// solid paint path with no gradient support required.
    StateLayer {
        base: RecipeColor,
        overlay: RecipeColor,
        alpha: f32,
    },
    /// Linear gradient at `angle_deg` (0° = top→bottom, 90° = leading→trailing).
    LinearGradient {
        stops: Vec<GradientStop>,
        angle_deg: f32,
    },
    /// Radial gradient with normalized center (`0.0..=1.0`) and radius
    /// in normalized units of the bounding rect's longer side.
    RadialGradient {
        stops: Vec<GradientStop>,
        center: (f32, f32),
        radius: f32,
    },
    /// No fill (transparent).
    None,
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    /// Position along the gradient axis, `0.0..=1.0`.
    pub offset: f32,
    pub color: RecipeColor,
}

impl FillRecipe {
    pub fn solid(color: impl Into<RecipeColor>) -> Self {
        Self::Solid(color.into())
    }

    /// A translucent state layer: `overlay` composited over `base` at
    /// `alpha` (clamped to `0.0..=1.0`). Resolves to a flat [`Color`].
    pub fn state_layer(
        base: impl Into<RecipeColor>,
        overlay: impl Into<RecipeColor>,
        alpha: f32,
    ) -> Self {
        Self::StateLayer {
            base: base.into(),
            overlay: overlay.into(),
            alpha: alpha.clamp(0.0, 1.0),
        }
    }

    /// Resolve the flat-color variants (`Solid`, `StateLayer`, `None`)
    /// against `colors`. Gradient variants return `None` here — they are
    /// resolved to a `Paint` by `resolve_fill_to_paint`, not to a flat
    /// color. `FillRecipe::None` maps to `Some(Color::TRANSPARENT)`.
    pub fn resolve_flat(&self, colors: &ColorTokens) -> Option<Color> {
        match self {
            FillRecipe::Solid(c) => Some(c.resolve_with(colors)),
            FillRecipe::StateLayer {
                base,
                overlay,
                alpha,
            } => Some(
                base.resolve_with(colors)
                    .mix(overlay.resolve_with(colors), *alpha),
            ),
            FillRecipe::None => Some(Color::TRANSPARENT),
            FillRecipe::LinearGradient { .. } | FillRecipe::RadialGradient { .. } => None,
        }
    }
}

// ─── BorderRecipe ───────────────────────────────────────────────────────────

#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum BorderStyle {
    #[default]
    Solid,
    Dashed {
        dash: f32,
        gap: f32,
    },
    Dotted {
        gap: f32,
    },
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BorderPosition {
    /// Border drawn entirely inside the shape (default — matches the
    /// existing IntUI-style `RectWidget` border).
    #[default]
    Inside,
    /// Centerline of the border lies on the shape edge.
    Center,
    /// Border drawn entirely outside the shape (used for focus rings
    /// that sit in the gap outside the control).
    Outside,
}

/// Per-side border widths (logical px). When a [`BorderRecipe`] carries
/// `sides: Some(BorderSides)`, the four widths override the uniform
/// `BorderRecipe::width` — letting a recipe draw e.g. a bottom-only
/// underline (Material 3 / Fluent / Adwaita filled fields). `Leading` /
/// `Trailing` are RTL-resolved by the paint site, not here.
#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BorderSides {
    pub top: f32,
    pub trailing: f32,
    pub bottom: f32,
    pub leading: f32,
}

impl BorderSides {
    /// All four sides at `w`.
    pub fn uniform(w: f32) -> Self {
        Self {
            top: w,
            trailing: w,
            bottom: w,
            leading: w,
        }
    }

    /// Bottom edge only — the underline case.
    pub fn bottom(w: f32) -> Self {
        Self {
            bottom: w,
            ..Self::default()
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BorderRecipe {
    pub width: f32,
    pub color: RecipeColor,
    pub style: BorderStyle,
    pub position: BorderPosition,
    /// Optional per-side widths. `None` = a uniform `width` border on all
    /// four sides (the common case). `Some(..)` overrides with per-side
    /// widths (e.g. a bottom-only underline).
    #[serde(default)]
    pub sides: Option<BorderSides>,
}

impl BorderRecipe {
    pub fn solid(width: f32, color: impl Into<RecipeColor>) -> Self {
        Self {
            width,
            color: color.into(),
            style: BorderStyle::Solid,
            position: BorderPosition::Inside,
            sides: None,
        }
    }

    /// Convenience for "no border" — width 0, transparent color.
    pub fn none() -> Self {
        Self::solid(0.0, RecipeColor::Static(Color::TRANSPARENT))
    }

    /// A bottom-only underline of `width` in `color` (M3 / Fluent /
    /// Adwaita filled-field underline). `position` is `Inside`.
    pub fn underline(width: f32, color: impl Into<RecipeColor>) -> Self {
        Self {
            width,
            color: color.into(),
            style: BorderStyle::Solid,
            position: BorderPosition::Inside,
            sides: Some(BorderSides::bottom(width)),
        }
    }
}

/// Offset a stroke rect to honour a [`BorderPosition`].
///
/// The SDF stroke is centered on the rect edge ([`BorderPosition::Center`]),
/// so `Inside` shrinks the rect inward by `width / 2` and `Outside`
/// expands it outward by the same — placing the whole stroke inside or
/// outside the original `bounds`. Used by recipe paint sites and
/// `RectWidget` so a focus ring can sit in the gap outside a control.
pub fn apply_border_position(bounds: Rect, width: f32, position: BorderPosition) -> Rect {
    let offset = match position {
        BorderPosition::Inside => width / 2.0,
        BorderPosition::Center => 0.0,
        BorderPosition::Outside => -width / 2.0,
    };
    Rect::new(
        bounds.x + offset,
        bounds.y + offset,
        bounds.width - offset * 2.0,
        bounds.height - offset * 2.0,
    )
}

// ─── ShadowRecipe ───────────────────────────────────────────────────────────

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShadowRecipe {
    pub offset: Vec2,
    pub blur: f32,
    pub spread: f32,
    pub color: RecipeColor,
}

impl ShadowRecipe {
    pub fn drop(offset: Vec2, blur: f32, color: impl Into<RecipeColor>) -> Self {
        Self {
            offset,
            blur,
            spread: 0.0,
            color: color.into(),
        }
    }
}

// ─── PerStateRecipe ─────────────────────────────────────────────────────────

/// Five-slot envelope that maps each [`WidgetState`] to a `T` with an
/// explicit fallback chain. Bastyde's answer to Flutter's
/// `WidgetStateProperty<T>` — no closures, no virtual dispatch, the
/// fallback graph is always knowable from the data.
///
/// Resolution order:
/// - `Idle` → `idle` (always present)
/// - `Hovered` → `hover` ?? `idle`
/// - `Pressed` → `pressed` ?? `hover` ?? `idle`
/// - `Focused` → `focused` ?? `hover` ?? `idle`
/// - `Disabled` → `disabled` ?? `idle`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PerStateRecipe<T> {
    pub idle: T,
    pub hover: Option<T>,
    pub pressed: Option<T>,
    pub focused: Option<T>,
    pub disabled: Option<T>,
}

impl<T> PerStateRecipe<T> {
    /// All five states share the same value.
    pub fn uniform(value: T) -> Self
    where
        T: Clone,
    {
        Self {
            idle: value,
            hover: None,
            pressed: None,
            focused: None,
            disabled: None,
        }
    }

    /// Look up the value for `state`, walking the fallback chain.
    pub fn resolve(&self, state: WidgetState) -> &T {
        match state {
            WidgetState::Idle => &self.idle,
            WidgetState::Hovered => self.hover.as_ref().unwrap_or(&self.idle),
            WidgetState::Pressed => self
                .pressed
                .as_ref()
                .or(self.hover.as_ref())
                .unwrap_or(&self.idle),
            WidgetState::Focused => self
                .focused
                .as_ref()
                .or(self.hover.as_ref())
                .unwrap_or(&self.idle),
            WidgetState::Disabled => self.disabled.as_ref().unwrap_or(&self.idle),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::intui;

    #[test]
    fn recipe_color_static_round_trips() {
        let theme = intui::light();
        let red = RecipeColor::Static(Color::from_hex("#FF0000"));
        assert_eq!(red.resolve(&theme), Color::from_hex("#FF0000"));
    }

    #[test]
    fn recipe_color_surface_resolves_against_theme() {
        let light = intui::light();
        let dark = intui::dark();
        let main = RecipeColor::Surface(SurfaceRole::Main);
        // Same recipe → different colors after theme swap.
        assert_ne!(main.resolve(&light), main.resolve(&dark));
    }

    #[test]
    fn per_state_resolves_idle_fallback() {
        let r = PerStateRecipe::<u32>::uniform(7);
        assert_eq!(*r.resolve(WidgetState::Idle), 7);
        assert_eq!(*r.resolve(WidgetState::Hovered), 7);
        assert_eq!(*r.resolve(WidgetState::Pressed), 7);
        assert_eq!(*r.resolve(WidgetState::Focused), 7);
        assert_eq!(*r.resolve(WidgetState::Disabled), 7);
    }

    #[test]
    fn pressed_falls_back_to_hover_then_idle() {
        let r = PerStateRecipe {
            idle: 1,
            hover: Some(2),
            pressed: None,
            focused: None,
            disabled: None,
        };
        assert_eq!(*r.resolve(WidgetState::Pressed), 2); // → hover
        let r2 = PerStateRecipe {
            idle: 1,
            hover: None,
            pressed: None,
            focused: None,
            disabled: None,
        };
        assert_eq!(*r2.resolve(WidgetState::Pressed), 1); // → idle
    }

    #[test]
    fn focused_falls_back_to_hover_then_idle() {
        let r = PerStateRecipe {
            idle: 1,
            hover: Some(2),
            pressed: None,
            focused: None,
            disabled: None,
        };
        assert_eq!(*r.resolve(WidgetState::Focused), 2);
    }

    #[test]
    fn disabled_falls_back_to_idle_directly() {
        let r = PerStateRecipe {
            idle: 1,
            hover: Some(2), // intentionally NOT used by disabled
            pressed: None,
            focused: None,
            disabled: None,
        };
        assert_eq!(*r.resolve(WidgetState::Disabled), 1);
    }

    #[test]
    fn fill_recipe_solid_constructor() {
        let f = FillRecipe::solid(SurfaceRole::Accent);
        assert!(matches!(f, FillRecipe::Solid(RecipeColor::Surface(_))));
    }

    #[test]
    fn state_layer_composites_overlay_over_base() {
        let colors = intui::light().colors;
        // 50 % white over black = mid-grey.
        let f = FillRecipe::state_layer(
            RecipeColor::Static(Color::BLACK),
            RecipeColor::Static(Color::WHITE),
            0.5,
        );
        let c = f.resolve_flat(&colors).unwrap();
        assert!((c.r() - 0.5).abs() < 1e-6);
        assert!((c.g() - 0.5).abs() < 1e-6);
        assert!((c.b() - 0.5).abs() < 1e-6);
        // alpha 0 → base unchanged.
        let f0 = FillRecipe::state_layer(Color::BLACK, Color::WHITE, 0.0);
        assert_eq!(f0.resolve_flat(&colors).unwrap(), Color::BLACK);
    }

    #[test]
    fn state_layer_clamps_alpha() {
        let f = FillRecipe::state_layer(Color::BLACK, Color::WHITE, 5.0);
        match f {
            FillRecipe::StateLayer { alpha, .. } => assert_eq!(alpha, 1.0),
            _ => panic!("expected StateLayer"),
        }
    }

    #[test]
    fn gradient_has_no_flat_color() {
        let colors = intui::light().colors;
        let g = FillRecipe::LinearGradient {
            stops: vec![],
            angle_deg: 0.0,
        };
        assert!(g.resolve_flat(&colors).is_none());
    }

    #[test]
    fn underline_is_bottom_only() {
        let b = BorderRecipe::underline(2.0, BorderRole::Focused);
        let sides = b.sides.expect("underline sets per-side widths");
        assert_eq!(sides.bottom, 2.0);
        assert_eq!(sides.top, 0.0);
        assert_eq!(sides.leading, 0.0);
        assert_eq!(sides.trailing, 0.0);
    }

    #[test]
    fn solid_border_has_no_per_side() {
        assert!(
            BorderRecipe::solid(1.0, BorderRole::Default)
                .sides
                .is_none()
        );
    }

    #[test]
    fn border_position_offsets_stroke_rect() {
        let bounds = Rect::new(0.0, 0.0, 100.0, 40.0);
        // Inside shrinks by width/2 on each edge.
        let inside = apply_border_position(bounds, 4.0, BorderPosition::Inside);
        assert_eq!(
            (inside.x, inside.y, inside.width, inside.height),
            (2.0, 2.0, 96.0, 36.0)
        );
        // Center is unchanged.
        let center = apply_border_position(bounds, 4.0, BorderPosition::Center);
        assert_eq!((center.x, center.width), (0.0, 100.0));
        // Outside expands.
        let outside = apply_border_position(bounds, 4.0, BorderPosition::Outside);
        assert_eq!(
            (outside.x, outside.y, outside.width, outside.height),
            (-2.0, -2.0, 104.0, 44.0)
        );
    }

    #[test]
    fn shape_recipe_rounded_constructor() {
        let s = ShapeRecipe::rounded(4.0);
        match s {
            ShapeRecipe::Rect { corner_radius } => {
                assert_eq!(corner_radius.top_left, 4.0);
                assert_eq!(corner_radius.bottom_right, 4.0);
            }
            _ => panic!("expected Rect"),
        }
    }

    #[test]
    fn recipes_are_send_sync() {
        // Compile-time check: recipes must be Send + Sync to satisfy
        // serialization contexts (inspector JSON export, future TOML
        // manifest) and to stay storable in Arc-based caches.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ShapeRecipe>();
        assert_send_sync::<FillRecipe>();
        assert_send_sync::<BorderRecipe>();
        assert_send_sync::<ShadowRecipe>();
        assert_send_sync::<PerStateRecipe<FillRecipe>>();
        assert_send_sync::<RecipeColor>();
        assert_send_sync::<WidgetState>();
    }
}
