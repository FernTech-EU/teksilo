// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The complete theme aggregator.
//!
//! `Theme` lives in `teksilo-core` (not `teksilo-tokens`) so the per-widget
//! style trait protocols and the typed `Rc<dyn FooStyle>` slots in
//! [`ComponentStyleSlots`] can sit on the same struct without forcing a
//! dependency cycle. See the `docs/styling-system.md` reference for the
//! four-tier ladder this type anchors.
//!
//! Construct via a preset — there is no `Theme::default()` /
//! `Theme::*_default()`. Apps explicitly pick one:
//!
//! ```
//! use teksilo_core::presets::intui;
//! let theme = intui::light();
//! ```
//!
//! `appearance` is required and drives shadow density, OS-theme
//! matching, and asset variant selection. `extensions` is a typed
//! registry for app-attached extras; see [`ThemeExtensions`].

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use teksilo_tokens::{
    ColorTokens, InputTokens, LayoutTokens, MotionTokens, ShapeTokens, TargetDensity,
    TypographyTokens,
};

use crate::styles::component_style_slots::ComponentStyleSlots;
use crate::styles::theme_appearance::ThemeAppearance;
use crate::styles::theme_extension::ThemeExtensions;

/// Stable identity for a [`Theme`], independent of its token values.
///
/// Two themes that share a `ThemeId` are "the same theme" even after a
/// token tweak, and two distinct themes are always distinguishable even
/// if they happen to share an appearance (Light/Dark). This is what lets
/// UI like `ThemeSwitcher` reliably match the active theme back to a list
/// entry, where `appearance` alone would be ambiguous.
///
/// Preset constructors stamp a `family.variant` id (e.g. `"intui.light"`,
/// `"fluent.dark"`). OS-driven themes (follow-system / native) carry the
/// id `"system"`. A theme built from raw tokens defaults to `"custom"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ThemeId(Cow<'static, str>);

impl ThemeId {
    /// Construct from a static string (`ThemeId::new("intui.light")`) or an
    /// owned `String` for app-supplied custom themes.
    pub fn new(id: impl Into<Cow<'static, str>>) -> Self {
        Self(id.into())
    }

    /// The id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ThemeId {
    fn default() -> Self {
        Self(Cow::Borrowed("custom"))
    }
}

impl std::fmt::Display for ThemeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    /// Stable identity of this theme — see [`ThemeId`]. Serde-defaulted so
    /// older serialized themes (which predate the field) still deserialize.
    #[serde(default)]
    pub id: ThemeId,
    pub appearance: ThemeAppearance,
    pub colors: ColorTokens,
    pub layout: LayoutTokens,
    pub typography: TypographyTokens,
    pub shape: ShapeTokens,
    pub motion: MotionTokens,
    /// Input and density tokens — target sizes, gesture slop, scroll physics
    /// and the touch kill switch. See [`InputTokens`] and
    /// `docs/density-and-targets.md`.
    ///
    /// Serde-defaulted for the same reason [`Theme::id`](Self::id) is: `Theme`
    /// derives `Deserialize`, and a theme serialized before this field existed
    /// must still load. The default is the Compact ladder — today's behaviour.
    #[serde(default)]
    pub input: InputTokens,
    /// Typed `Rc<dyn FooStyle>` slot bag for theme-wide style
    /// installations. `None` per slot means "use the widget's local
    /// `Recipe*Style` default"; apps install per-theme overrides via
    /// `theme.style_slots.button = Some(Rc::new(MyButton))`. Per-call
    /// `.style(...)` on a widget always wins over the slot.
    #[serde(skip, default)]
    pub style_slots: ComponentStyleSlots,
    #[serde(skip, default)]
    pub extensions: ThemeExtensions,
}

/// How a theme re-derives itself for another [`TargetDensity`], registered as
/// a [`ThemeExtensions`] entry.
///
/// Absent — the default, and what every raw-token theme gets — means
/// [`Theme::with_density`] swaps [`Theme::input`] and changes nothing else.
/// That is right for a theme whose `style_slots` are all `None`, because each
/// widget then builds its own `Recipe*Style` from `ctx.theme().input` at its
/// next build.
///
/// A **preset that installs Tier-3 slots of its own** (Fluent, macOS,
/// Material 3) needs more: its slots are `Some(..)`, so they would ride across
/// a density switch still carrying the dimensions they were built with, and a
/// Fluent button would stay 32 dp tall under Touch. Such a preset registers
/// this — a function taking the *current* theme, so it can recover the palette
/// it was built from (`theme.extension::<FluentPalette>()`) and rebuild only
/// the slots it owns, leaving colours, id, other extensions and any slot the
/// app installed itself alone.
///
/// It lives in the extension registry rather than as a `Theme` field for the
/// reason the registry exists: it is optional, typed, non-serializable state
/// that only some themes carry.
#[derive(Clone, Copy)]
pub struct DensityProjection(pub fn(&Theme, TargetDensity) -> Theme);

impl std::fmt::Debug for DensityProjection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DensityProjection(<fn>)")
    }
}

impl Theme {
    /// Build a Theme from raw token data. Most apps go through a
    /// preset constructor (e.g. `teksilo_core::presets::intui::light`)
    /// rather than calling this directly — presets aggregate the
    /// matching `Recipe*Style` defaults under the same call.
    pub fn new(
        appearance: ThemeAppearance,
        colors: ColorTokens,
        layout: LayoutTokens,
        typography: TypographyTokens,
        shape: ShapeTokens,
        motion: MotionTokens,
        input: InputTokens,
    ) -> Self {
        Self {
            id: ThemeId::default(),
            appearance,
            colors,
            layout,
            typography,
            shape,
            motion,
            input,
            style_slots: ComponentStyleSlots::default(),
            extensions: ThemeExtensions::new(),
        }
    }

    /// Register how this theme re-derives itself for another density. Preset
    /// constructors call this; see [`DensityProjection`].
    pub fn with_density_projection(self, project: fn(&Theme, TargetDensity) -> Theme) -> Self {
        self.with_extension(DensityProjection(project))
    }

    /// Set this theme's [`ThemeId`] and return self for chaining. Used by
    /// preset constructors and apps building custom themes.
    pub fn with_id(mut self, id: impl Into<Cow<'static, str>>) -> Self {
        self.id = ThemeId::new(id);
        self
    }

    /// Whether this theme paints on a dark background. Convenience for
    /// `theme.appearance.is_dark()`.
    pub fn is_dark(&self) -> bool {
        self.appearance.is_dark()
    }

    /// A copy of this theme projected for an **inactive window** — the accent
    /// family and focus indicators desaturated toward graphite (see
    /// [`ColorTokens::for_inactive_window`](teksilo_tokens::ColorTokens::for_inactive_window)).
    /// The paint walker swaps this in when the host window loses focus, so every
    /// accent-coloured control greys out with no per-widget code. Only the
    /// colours change; typography / layout / shape / motion are untouched, so
    /// this never affects layout.
    pub fn for_inactive_window(&self) -> Theme {
        Theme {
            colors: self.colors.for_inactive_window(),
            ..self.clone()
        }
    }

    /// Project into a high-contrast variant (WCAG 1.4.6 Enhanced / EN 301 549
    /// §11.7), applied at paint time when the OS "increase contrast" preference
    /// is set. See
    /// [`ColorTokens::for_high_contrast`](teksilo_tokens::ColorTokens::for_high_contrast).
    pub fn for_high_contrast(&self) -> Theme {
        Theme {
            colors: self.colors.for_high_contrast(),
            ..self.clone()
        }
    }

    /// A copy of this theme projected onto another [`TargetDensity`].
    ///
    /// This is a **token projection only**: it replaces [`Self::input`] with
    /// [`InputTokens::for_density`] and carries `style_slots` and `extensions`
    /// across verbatim. It deliberately does *not* re-run any recipe
    /// constructor, because there is nothing in such a theme to re-run — every
    /// `ComponentStyleSlots` slot is `None` in a raw-token theme and in the
    /// IntUI preset, and each
    /// widget builds its `Recipe*Style` lazily at its own build site, in
    /// `teksilo-widgets`, from `ctx.theme().input`. Changing the tokens here is
    /// therefore sufficient; the widgets read the new values on their next
    /// build.
    ///
    /// A slot an app has installed itself is `Some(..)` and is preserved as it
    /// is, so a custom Tier-3 style keeps whatever dimensions it was written
    /// with. That is intentional (a hand-written style owns its own metrics)
    /// but it means a custom style is not density-aware unless its author made
    /// it so.
    ///
    /// A theme that carries a [`DensityProjection`] — every shipped preset that
    /// installs slots — takes that function's answer instead, so its own chrome
    /// does follow the density.
    ///
    /// Use `WidgetTree::set_input_density` rather than calling this and
    /// `set_theme` by hand: a density change must rebuild, not merely relayout.
    pub fn with_density(&self, density: TargetDensity) -> Theme {
        if let Some(DensityProjection(project)) = self.extension::<DensityProjection>().copied() {
            return project(self, density);
        }
        Theme {
            input: InputTokens::for_density(density),
            ..self.clone()
        }
    }

    /// Look up a typed theme extension. See [`ThemeExtensions`].
    pub fn extension<T: std::any::Any + Send + Sync>(&self) -> Option<&T> {
        self.extensions.get::<T>()
    }

    /// Attach a typed extension and return self for chaining. See
    /// [`ThemeExtensions`].
    pub fn with_extension<T: std::any::Any + Send + Sync>(mut self, value: T) -> Self {
        self.extensions.insert(value);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::intui;
    use teksilo_tokens::TargetDensity;

    /// The whole point of `#[serde(default)]` on `input`: a theme serialized
    /// before the field existed must still load. The fixture is built the only
    /// way that cannot go stale — serialize a real theme, then delete the key.
    #[test]
    fn a_theme_without_an_input_key_still_deserializes() {
        let mut value: serde_json::Value =
            serde_json::to_value(intui::light()).expect("theme serializes");
        assert!(
            value
                .as_object_mut()
                .expect("theme is a JSON object")
                .remove("input")
                .is_some(),
            "the fixture must actually have had an `input` key to remove"
        );

        let restored: Theme = serde_json::from_value(value).expect("pre-programme theme loads");
        assert_eq!(restored.input, teksilo_tokens::InputTokens::default());
        assert_eq!(restored.input.density, TargetDensity::Compact);
        assert_eq!(restored.colors, intui::light().colors);
    }

    /// `with_density` swaps the token group and nothing else.
    #[test]
    fn with_density_projects_only_the_input_tokens() {
        let base = intui::light();
        let touch = base.with_density(TargetDensity::Touch);

        assert_eq!(touch.input.density, TargetDensity::Touch);
        assert_eq!(touch.input.target_size, 44.0);
        // Everything else rides across verbatim.
        assert_eq!(touch.id, base.id);
        assert_eq!(touch.appearance, base.appearance);
        assert_eq!(touch.colors, base.colors);
        assert_eq!(touch.typography, base.typography);
        assert_eq!(touch.shape, base.shape);
        assert_eq!(touch.motion, base.motion);
    }

    /// A theme built by a preset is Compact, i.e. today's behaviour.
    #[test]
    fn presets_start_compact() {
        assert_eq!(intui::light().input, teksilo_tokens::InputTokens::default());
        assert_eq!(intui::dark().input.density, TargetDensity::Compact);
    }

    /// An app-installed Tier-3 style slot survives a density projection —
    /// documented behaviour (a hand-written style owns its own metrics), not
    /// an accident of `..self.clone()`.
    #[test]
    fn with_density_preserves_installed_style_slots() {
        #[derive(Debug)]
        struct TestButtonStyle;
        impl crate::styles::ButtonStyle for TestButtonStyle {
            fn make_body(
                &self,
                _cfg: &crate::styles::ButtonStyleConfig,
                ctx: &mut crate::BuildContext,
            ) -> crate::WidgetId {
                ctx.add(crate::test_widgets::FillWidget::new())
            }
        }

        let mut base = intui::light();
        base.style_slots.button = Some(std::rc::Rc::new(TestButtonStyle));
        let touch = base.with_density(TargetDensity::Touch);
        assert!(touch.style_slots.button.is_some());
        assert_eq!(touch.input.density, TargetDensity::Touch);
    }
}
