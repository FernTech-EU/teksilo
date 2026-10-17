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

/// The window and accessibility state a frame is painted under: the inputs of
/// [`Theme::for_paint_state`].
///
/// A fresh tree paints with `window_active: true, high_contrast: false`,
/// under which [`Theme::for_paint_state`] returns the theme unchanged when no
/// [`PaintProjection`] is registered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaintState {
    /// The host window is focused and not occluded.
    pub window_active: bool,
    /// The OS "increase contrast" preference is set.
    pub high_contrast: bool,
}

/// How a theme re-colours itself for a [`PaintState`]: registered in the
/// theme's [extension registry](ThemeExtensions) and consulted by
/// [`Theme::for_paint_state`].
///
/// Absent, which is the default and what every raw-token theme and the IntUI
/// preset get, means the generic chain: [`Theme::for_inactive_window`] when
/// the window is inactive, then [`Theme::for_high_contrast`] when the
/// high-contrast preference is set. That chain is a fixed transform of the
/// tokens (desaturate the accent family, then lift the accent, the ring, the
/// secondary text and the selection until they reach the high-contrast
/// floors), which is right for a palette that defines no inactive or
/// high-contrast colours of its own.
///
/// A preset whose source palette **does** define them (a desktop colour
/// scheme with an inactive colour group, say) registers this to paint those
/// colours instead. The function receives the *active* theme and the whole
/// state at once, so it decides how the two flags combine; it replaces the
/// generic chain entirely, and calling [`Theme::for_inactive_window`] or
/// [`Theme::for_high_contrast`] from it is how it keeps a part of that chain.
/// Only colours should change: the paint walker swaps the result in without a
/// relayout, so geometry a hook changes is not honoured.
///
/// It lives in the extension registry for the reason [`DensityProjection`]
/// does: it is optional, typed, non-serializable state only some themes carry.
#[derive(Clone, Copy)]
pub struct PaintProjection(pub fn(&Theme, PaintState) -> Theme);

impl std::fmt::Debug for PaintProjection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PaintProjection(<fn>)")
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

    /// Register how this theme re-colours itself for an inactive window and
    /// the high-contrast preference; see [`PaintProjection`].
    pub fn with_paint_projection(self, project: fn(&Theme, PaintState) -> Theme) -> Self {
        self.with_extension(PaintProjection(project))
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

    /// The theme a frame is painted with under `state`: what the paint walker
    /// swaps in, and what a recipe that bakes a colour at build time must
    /// resolve against so it agrees with the paint-resolved controls.
    ///
    /// A theme that carries a [`PaintProjection`] gets that function's
    /// answer. Otherwise this is the generic chain:
    /// [`for_inactive_window`](Self::for_inactive_window) when
    /// `!state.window_active`, then
    /// [`for_high_contrast`](Self::for_high_contrast) when
    /// `state.high_contrast` (so the desaturated accent and ring of an
    /// inactive window are lifted to the high-contrast floors too), and an
    /// unchanged copy when neither applies.
    pub fn for_paint_state(&self, state: PaintState) -> Theme {
        if let Some(PaintProjection(project)) = self.extension::<PaintProjection>().copied() {
            return project(self, state);
        }
        Theme {
            colors: generic_paint_colors(&self.colors, state),
            ..self.clone()
        }
    }

    /// The colours of [`for_paint_state`](Self::for_paint_state), without
    /// cloning the rest of the theme when no [`PaintProjection`] is
    /// registered. For a colour resolved on every paint (a recipe's
    /// `ColorProp::Bound`), where only the palette is read.
    pub fn paint_colors(&self, state: PaintState) -> ColorTokens {
        if let Some(PaintProjection(project)) = self.extension::<PaintProjection>().copied() {
            return project(self, state).colors;
        }
        generic_paint_colors(&self.colors, state)
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

/// The generic chain behind [`Theme::for_paint_state`] when no
/// [`PaintProjection`] is registered: the token halves of
/// [`Theme::for_inactive_window`] and [`Theme::for_high_contrast`], which
/// change only the colours.
fn generic_paint_colors(colors: &ColorTokens, state: PaintState) -> ColorTokens {
    match (state.window_active, state.high_contrast) {
        (true, false) => colors.clone(),
        (false, false) => colors.for_inactive_window(),
        (true, true) => colors.for_high_contrast(),
        (false, true) => colors.for_inactive_window().for_high_contrast(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::intui;
    use teksilo_tokens::TargetDensity;

    const ALL_PAINT_STATES: [PaintState; 4] = [
        PaintState {
            window_active: true,
            high_contrast: false,
        },
        PaintState {
            window_active: false,
            high_contrast: false,
        },
        PaintState {
            window_active: true,
            high_contrast: true,
        },
        PaintState {
            window_active: false,
            high_contrast: true,
        },
    ];

    /// The paint walker's projection as it was written before
    /// `for_paint_state` existed, kept verbatim as the oracle.
    fn pre_hook_chain(theme: &Theme, state: PaintState) -> Theme {
        let base = if state.window_active {
            theme.clone()
        } else {
            theme.for_inactive_window()
        };
        if state.high_contrast {
            base.for_high_contrast()
        } else {
            base
        }
    }

    const HOOK_INACTIVE: &str = "#FF00FF";
    const HOOK_HIGH_CONTRAST: &str = "#00FF00";

    /// A projection that marks each state with a colour no generic transform
    /// produces, so a test can tell which path painted.
    fn marking_projection(theme: &Theme, state: PaintState) -> Theme {
        let mut out = theme.clone();
        if !state.window_active {
            out.colors.accent = teksilo_tokens::Color::from_hex(HOOK_INACTIVE);
        }
        if state.high_contrast {
            out.colors.accent = teksilo_tokens::Color::from_hex(HOOK_HIGH_CONTRAST);
        }
        out
    }

    /// Without a hook, the default output is the old chain exactly, for every
    /// state and both IntUI appearances.
    #[test]
    fn for_paint_state_without_a_hook_is_the_pre_hook_chain() {
        for theme in [intui::light(), intui::dark()] {
            assert!(theme.extension::<PaintProjection>().is_none());
            for state in ALL_PAINT_STATES {
                let expected = pre_hook_chain(&theme, state);
                assert_eq!(theme.for_paint_state(state), expected, "{state:?}");
                assert_eq!(theme.paint_colors(state), expected.colors, "{state:?}");
            }
            // Active and not high-contrast is the identity.
            assert_eq!(theme.for_paint_state(ALL_PAINT_STATES[0]), theme);
        }
    }

    /// A registered hook replaces the generic chain, in both entry points.
    #[test]
    fn a_paint_projection_replaces_the_generic_chain() {
        let theme = intui::light().with_paint_projection(marking_projection);
        for state in ALL_PAINT_STATES {
            let expected = marking_projection(&theme, state);
            assert_eq!(theme.for_paint_state(state), expected, "{state:?}");
            assert_eq!(theme.paint_colors(state), expected.colors, "{state:?}");
        }
        assert_eq!(
            format!("{:?}", theme.extension::<PaintProjection>().unwrap()),
            "PaintProjection(<fn>)"
        );
    }

    /// `for_inactive_window` and `for_high_contrast` stay the documented
    /// generic transforms: colours only, and a hook does not reach them.
    #[test]
    fn the_named_projections_keep_their_documented_behaviour() {
        let plain = intui::light();
        let hooked = plain.clone().with_paint_projection(marking_projection);
        for theme in [&plain, &hooked] {
            let inactive = theme.for_inactive_window();
            assert_eq!(inactive.colors, theme.colors.for_inactive_window());
            assert_ne!(inactive.colors.accent, theme.colors.accent);
            let high = theme.for_high_contrast();
            assert_eq!(high.colors, theme.colors.for_high_contrast());
            for projected in [inactive, high] {
                assert_eq!(projected.id, theme.id);
                assert_eq!(projected.appearance, theme.appearance);
                assert_eq!(projected.layout, theme.layout);
                assert_eq!(projected.typography, theme.typography);
                assert_eq!(projected.shape, theme.shape);
                assert_eq!(projected.motion, theme.motion);
                assert_eq!(projected.input, theme.input);
                assert_eq!(
                    projected.extension::<PaintProjection>().is_some(),
                    theme.extension::<PaintProjection>().is_some(),
                );
            }
        }
    }

    /// A leaf painting the theme's accent, read from the paint context the
    /// walker hands it.
    #[derive(Debug)]
    struct AccentProbe;

    impl crate::widget::Widget for AccentProbe {
        fn layout_response(
            &self,
            proposal: teksilo_canvas::SizeProposal,
            _ctx: &crate::widget::LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }

        fn paint(
            &self,
            bounds: teksilo_canvas::Rect,
            canvas: &mut teksilo_canvas::Canvas,
            ctx: &crate::widget::PaintContext,
        ) {
            canvas.fill_rounded_rect(
                bounds,
                teksilo_tokens::CornerRadius::ZERO,
                ctx.theme.colors.accent,
            );
        }
    }

    fn painted_accent(tree: &mut crate::widget_tree::WidgetTree) -> [f32; 4] {
        let frame = tree.render();
        assert_eq!(frame.shapes.len(), 1);
        frame.shapes[0].color
    }

    /// The paint walker asks the theme's hook, for window activation and for
    /// the high-contrast preference.
    #[test]
    fn the_paint_walker_paints_the_hooks_colours() {
        let theme = intui::light().with_paint_projection(marking_projection);
        let active = theme.colors.accent.to_array();
        let mut tree = crate::widget_tree::WidgetTree::new().with_theme(theme);
        tree.add(AccentProbe);
        tree.layout(teksilo_canvas::SizeProposal::exact(40.0, 20.0));
        assert_eq!(painted_accent(&mut tree), active);

        tree.set_window_active(false);
        assert_eq!(
            painted_accent(&mut tree),
            teksilo_tokens::Color::from_hex(HOOK_INACTIVE).to_array(),
            "an inactive window paints the hook's colours, not the generic desaturation"
        );

        tree.set_window_active(true);
        tree.set_accessibility_preferences(true, false, 1.0);
        assert_eq!(
            painted_accent(&mut tree),
            teksilo_tokens::Color::from_hex(HOOK_HIGH_CONTRAST).to_array(),
        );
        assert_eq!(
            tree.paint_state(),
            PaintState {
                window_active: true,
                high_contrast: true,
            }
        );
    }

    /// Without a hook the walker paints what it painted before the hook.
    #[test]
    fn the_paint_walker_without_a_hook_paints_the_generic_chain() {
        let theme = intui::dark();
        let mut tree = crate::widget_tree::WidgetTree::new().with_theme(theme.clone());
        tree.add(AccentProbe);
        tree.layout(teksilo_canvas::SizeProposal::exact(40.0, 20.0));
        tree.set_window_active(false);
        tree.set_accessibility_preferences(true, false, 1.0);
        assert_eq!(
            painted_accent(&mut tree),
            pre_hook_chain(&theme, ALL_PAINT_STATES[3])
                .colors
                .accent
                .to_array()
        );
    }

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
