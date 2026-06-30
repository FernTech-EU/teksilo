// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The complete theme aggregator.
//!
//! `Theme` lives in `bastyde-core` (not `bastyde-tokens`) so the per-widget
//! style trait protocols and the typed `Rc<dyn FooStyle>` slots in
//! [`ComponentStyleSlots`] can sit on the same struct without forcing a
//! dependency cycle. See the `docs/styling-system.md` reference for the
//! four-tier ladder this type anchors.
//!
//! Construct via a preset — there is no `Theme::default()` /
//! `Theme::*_default()`. Apps explicitly pick one:
//!
//! ```
//! use bastyde_core::presets::intui;
//! let theme = intui::light();
//! ```
//!
//! `appearance` is required and drives shadow density, OS-theme
//! matching, and asset variant selection. `extensions` is a typed
//! registry for app-attached extras; see [`ThemeExtensions`].

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use bastyde_tokens::{ColorTokens, LayoutTokens, MotionTokens, ShapeTokens, TypographyTokens};

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

impl Theme {
    /// Build a Theme from raw token data. Most apps go through a
    /// preset constructor (e.g. `bastyde_core::presets::intui::light`)
    /// rather than calling this directly — presets aggregate the
    /// matching `Recipe*Style` defaults under the same call.
    pub fn new(
        appearance: ThemeAppearance,
        colors: ColorTokens,
        layout: LayoutTokens,
        typography: TypographyTokens,
        shape: ShapeTokens,
        motion: MotionTokens,
    ) -> Self {
        Self {
            id: ThemeId::default(),
            appearance,
            colors,
            layout,
            typography,
            shape,
            motion,
            style_slots: ComponentStyleSlots::default(),
            extensions: ThemeExtensions::new(),
        }
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
    /// [`ColorTokens::for_inactive_window`](bastyde_tokens::ColorTokens::for_inactive_window)).
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
