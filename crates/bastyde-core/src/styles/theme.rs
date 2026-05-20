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
//! ```ignore
//! use bastyde_core::presets::intui;
//! let theme = intui::light();
//! ```
//!
//! `appearance` is required and drives shadow density, OS-theme
//! matching, and asset variant selection. `extensions` is a typed
//! registry for app-attached extras; see [`ThemeExtensions`].

use serde::{Deserialize, Serialize};

use bastyde_tokens::{ColorTokens, LayoutTokens, MotionTokens, ShapeTokens, TypographyTokens};

use crate::styles::component_style_slots::ComponentStyleSlots;
use crate::styles::theme_appearance::ThemeAppearance;
use crate::styles::theme_extension::ThemeExtensions;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Theme {
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

    /// Whether this theme paints on a dark background. Convenience for
    /// `theme.appearance.is_dark()`.
    pub fn is_dark(&self) -> bool {
        self.appearance.is_dark()
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
