//! Tier-2 paint-recipe primitives — Shape, Fill, Border, Shadow,
//! per-state envelope, and the [`WidgetState`] enum the envelope
//! resolves against.
//!
//! These types are **pure data**: every field is plain (no signals, no
//! closures, no `Rc`-wrapped reactive state). That gives them three
//! properties the rest of the styling system relies on:
//!
//! - **`Send + Sync + 'static`** so the default `Recipe*Style` impls
//!   stored as `Arc<dyn FooStyle>` on `Theme.components` satisfy the
//!   `Send + Sync + 'static` trait bound.
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

use fern_canvas::Vec2;
use fern_tokens::{BorderRole, Color, ColorTokens, CornerRadius, SurfaceRole, TextRole};

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

/// What the inside of the [`ShapeRecipe`] is filled with. Solid is the
/// only variant the renderer paints today; gradient variants exist so
/// recipe data can describe them, but until the renderer grows
/// `LinearGradient` / `RadialGradient` paint pipelines they fall
/// through to `None` (transparent) at draw time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FillRecipe {
    /// Single flat color.
    Solid(RecipeColor),
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

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BorderRecipe {
    pub width: f32,
    pub color: RecipeColor,
    pub style: BorderStyle,
    pub position: BorderPosition,
}

impl BorderRecipe {
    pub fn solid(width: f32, color: impl Into<RecipeColor>) -> Self {
        Self {
            width,
            color: color.into(),
            style: BorderStyle::Solid,
            position: BorderPosition::Inside,
        }
    }

    /// Convenience for "no border" — width 0, transparent color.
    pub fn none() -> Self {
        Self::solid(0.0, RecipeColor::Static(Color::TRANSPARENT))
    }
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
/// explicit fallback chain. FernUI's answer to Flutter's
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
        // Compile-time check: the `Arc<dyn FooStyle: Send + Sync>` slot
        // bag relies on every recipe being storable inside one.
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
