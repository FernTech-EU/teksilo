// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `ColorProp` — the unified input type for widget color builders.
//!
//! A `ColorProp` is one of:
//! - a static `Color` — frozen, non-reactive
//! - a `Signal<Color>` — reactive state (e.g. an interaction-dependent color)
//! - a [`TextRole`] / [`SurfaceRole`] / [`BorderRole`] — resolved against the
//!   current theme at paint time, reactive via `WidgetTree::set_theme`
//!
//! Widget builders accept `impl Into<ColorProp>`, so callers write:
//!
//! ```ignore
//! TextWidget::new("Hello")                              // default role
//! TextWidget::new("Error!").color(TextRole::Error)      // role-based (reactive)
//! Panel::new().background(SurfaceRole::Sunken)          // role-based (reactive)
//! ChipWidget::new().color(Color::from_hex("#FF00FF"))   // frozen custom
//! ChipWidget::new().color(hover_signal)                 // reactive interaction state
//! ```
//!
//! Role variants resolve against `PaintContext.theme` at each paint tick, so
//! no explicit dirty-tracking registration is required — the tree-wide
//! `mark_all_dirty` call inside `set_theme` already forces a repaint on every
//! node.

use teksilo_tokens::{BorderRole, Color, SurfaceRole, TextRole};

use crate::binding::{BindingLevel, BindingRegistry};
use crate::signal::{Prop, Signal};
use crate::styles::Theme;
use crate::widget_id::WidgetId;

/// A color value that may be static, reactive, or driven by a theme role.
///
/// Role variants (both static and dynamic) resolve against the current theme
/// at paint time, so a `Signal<TextRole>` + runtime theme switch yields the
/// expected color without any explicit `theme_signal` plumbing in the caller.
#[derive(Clone)]
pub enum ColorProp {
    /// Frozen literal color.
    Static(Color),
    /// Reactive signal carrying a color directly.
    Bound(Signal<Color>),
    /// Fixed theme-foreground role — resolved against the current theme at paint time.
    TextRole(TextRole),
    /// Fixed theme-surface role — resolved against the current theme at paint time.
    SurfaceRole(SurfaceRole),
    /// Fixed theme-border role — resolved against the current theme at paint time.
    BorderRole(BorderRole),
    /// Reactive text role — the role itself changes with state (e.g. interaction).
    DynamicTextRole(Signal<TextRole>),
    /// Reactive surface role — the role itself changes with state.
    DynamicSurfaceRole(Signal<SurfaceRole>),
    /// Reactive border role — the role itself changes with state.
    DynamicBorderRole(Signal<BorderRole>),
    /// A colour that keeps its role in a **disabled** subtree instead of dimming
    /// to the disabled counterpart — see [`ColorProp::undimmed`].
    Undimmed(Box<ColorProp>),
}

impl ColorProp {
    /// Produce the color value for the supplied theme. Callers invoke this in
    /// paint (where `ctx.theme` and `ctx.effective_enabled` are in scope).
    /// Static and Bound variants ignore both the theme and `enabled`; role
    /// variants resolve against the theme.
    ///
    /// When `enabled == false`, role variants substitute their disabled
    /// counterpart before resolving — the single hook that makes every
    /// role-derived color in a disabled subtree dim automatically (leaves
    /// like `IconWidget` / `TextWidget` / `RectWidget` pass
    /// `ctx.effective_enabled` through verbatim). `TextRole` →
    /// [`TextRole::Disabled`]; the accent `SurfaceRole` / `BorderRole`
    /// family → their `AccentDisabled` counterpart; and the *neutral
    /// interactive* [`SurfaceRole::Field`] / [`BorderRole::Field`] → their
    /// `Disabled` counterpart.
    ///
    /// Every other surface / border role passes through unchanged — a
    /// disabled panel keeps its surface; only interactive chrome dims. That
    /// is why a field paints `Field` rather than `Content`: the two resolve
    /// identically while enabled, and the distinction exists solely so this
    /// substitution can dim the field without also greying every `Panel` and
    /// `Card` in the disabled subtree.
    ///
    /// Resolving here — at paint, from `PaintContext::effective_enabled` —
    /// rather than from a build-time `Signal` is deliberate and load-bearing:
    /// `effective_enabled_signal` captures the ancestor chain when it is
    /// called, and a widget's `parent` is not yet wired during its own
    /// `build()`, so such a signal only ever reflects the widget's *own*
    /// `enabled` prop. The paint walker, by contrast, ANDs the live arena
    /// chain, so this hook is the only thing that dims a control sitting
    /// inside a disabled *ancestor*.
    pub fn resolve(&self, theme: &Theme, enabled: bool) -> Color {
        match self {
            ColorProp::Static(c) => *c,
            ColorProp::Bound(s) => s.get(),
            ColorProp::TextRole(role) => {
                let role = if enabled { *role } else { TextRole::Disabled };
                role.resolve(&theme.colors)
            }
            ColorProp::SurfaceRole(role) => disabled_surface(*role, enabled).resolve(&theme.colors),
            ColorProp::BorderRole(role) => disabled_border(*role, enabled).resolve(&theme.colors),
            ColorProp::DynamicTextRole(s) => {
                let role = if enabled { s.get() } else { TextRole::Disabled };
                role.resolve(&theme.colors)
            }
            ColorProp::DynamicSurfaceRole(s) => {
                disabled_surface(s.get(), enabled).resolve(&theme.colors)
            }
            ColorProp::DynamicBorderRole(s) => {
                disabled_border(s.get(), enabled).resolve(&theme.colors)
            }
            // The one opt-out of the substitution above: resolve as if enabled.
            ColorProp::Undimmed(inner) => inner.resolve(theme, true),
        }
    }

    /// Mark a colour as semantic **state** rather than interactive chrome: it keeps
    /// its role in a disabled subtree instead of dimming.
    ///
    /// The disabled substitution in [`Self::resolve`] exists because a disabled
    /// control must *look* unavailable — its label, its icon, its border. But a
    /// widget's colour sometimes carries information that is true regardless of
    /// whether the control can be clicked: a sync/save state, a validation result,
    /// a severity badge. Dimming those to `Disabled` doesn't say "you can't press
    /// this", it destroys the very thing the colour was there to communicate — and
    /// the caller has no way to get it back, because the substitution happens at
    /// paint, below any role they can pass in.
    ///
    /// Reach for this only when the colour is a *statement*, not an affordance:
    ///
    /// ```ignore
    /// // A save indicator that is disabled (autosave owns the saving) but must
    /// // still show whether the manuscript is on disk.
    /// IconButton::new(check)
    ///     .icon_role(ColorProp::undimmed(TextRole::Success))
    ///     .enabled(false)
    /// ```
    ///
    /// Composes with every variant, including the dynamic ones (their signal is
    /// still registered — see [`Self::register_if_bound`]).
    pub fn undimmed(color: impl Into<ColorProp>) -> ColorProp {
        ColorProp::Undimmed(Box::new(color.into()))
    }

    /// Register dirty-tracking for signal-bearing variants so updates trigger
    /// a widget repaint. Static role variants need no registration — theme
    /// changes already dirty-mark everything.
    pub fn register_if_bound(
        &self,
        widget_id: WidgetId,
        registry: &BindingRegistry,
        level: BindingLevel,
    ) {
        match self {
            ColorProp::Bound(s) => s.bind_to(widget_id, registry, level),
            ColorProp::DynamicTextRole(s) => s.bind_to(widget_id, registry, level),
            ColorProp::DynamicSurfaceRole(s) => s.bind_to(widget_id, registry, level),
            ColorProp::DynamicBorderRole(s) => s.bind_to(widget_id, registry, level),
            // Wrapping a reactive colour must not silently unsubscribe it.
            ColorProp::Undimmed(inner) => inner.register_if_bound(widget_id, registry, level),
            _ => {}
        }
    }

    /// Whether this prop carries any signal binding.
    pub fn is_bound(&self) -> bool {
        match self {
            ColorProp::Bound(_)
            | ColorProp::DynamicTextRole(_)
            | ColorProp::DynamicSurfaceRole(_)
            | ColorProp::DynamicBorderRole(_) => true,
            ColorProp::Undimmed(inner) => inner.is_bound(),
            _ => false,
        }
    }
}

/// Substitute the disabled counterpart of an accent surface role when the
/// subtree is disabled. Non-accent roles pass through (a disabled panel
/// keeps its surface).
fn disabled_surface(role: SurfaceRole, enabled: bool) -> SurfaceRole {
    if enabled {
        return role;
    }
    match role {
        SurfaceRole::Accent | SurfaceRole::AccentHover | SurfaceRole::AccentPressed => {
            SurfaceRole::AccentDisabled
        }
        // The neutral counterpart: a *field* dims, a passive `Panel` (which
        // paints `Content`) does not. Both resolve to the same colour while
        // enabled — `Field` exists precisely so this substitution can tell
        // them apart.
        SurfaceRole::Field => SurfaceRole::Disabled,
        other => other,
    }
}

/// Substitute the disabled counterpart of an accent border role when the
/// subtree is disabled.
fn disabled_border(role: BorderRole, enabled: bool) -> BorderRole {
    if enabled {
        return role;
    }
    match role {
        BorderRole::Accent => BorderRole::AccentDisabled,
        BorderRole::Field => BorderRole::Disabled,
        other => other,
    }
}

impl std::fmt::Debug for ColorProp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColorProp::Static(c) => f.debug_tuple("Static").field(c).finish(),
            ColorProp::Bound(_) => f.write_str("Bound(..)"),
            ColorProp::TextRole(r) => f.debug_tuple("TextRole").field(r).finish(),
            ColorProp::SurfaceRole(r) => f.debug_tuple("SurfaceRole").field(r).finish(),
            ColorProp::BorderRole(r) => f.debug_tuple("BorderRole").field(r).finish(),
            ColorProp::DynamicTextRole(_) => f.write_str("DynamicTextRole(..)"),
            ColorProp::DynamicSurfaceRole(_) => f.write_str("DynamicSurfaceRole(..)"),
            ColorProp::DynamicBorderRole(_) => f.write_str("DynamicBorderRole(..)"),
            ColorProp::Undimmed(inner) => f.debug_tuple("Undimmed").field(inner).finish(),
        }
    }
}

// Coercions — one From impl per accepted input shape.
impl From<Color> for ColorProp {
    fn from(c: Color) -> Self {
        ColorProp::Static(c)
    }
}

impl From<Signal<Color>> for ColorProp {
    fn from(s: Signal<Color>) -> Self {
        ColorProp::Bound(s)
    }
}

impl From<Prop<Color>> for ColorProp {
    fn from(p: Prop<Color>) -> Self {
        match p {
            Prop::Static(c) => ColorProp::Static(c),
            Prop::Bound(s) => ColorProp::Bound(s),
        }
    }
}

impl From<TextRole> for ColorProp {
    fn from(r: TextRole) -> Self {
        ColorProp::TextRole(r)
    }
}

impl From<SurfaceRole> for ColorProp {
    fn from(r: SurfaceRole) -> Self {
        ColorProp::SurfaceRole(r)
    }
}

impl From<BorderRole> for ColorProp {
    fn from(r: BorderRole) -> Self {
        ColorProp::BorderRole(r)
    }
}

impl From<Signal<TextRole>> for ColorProp {
    fn from(s: Signal<TextRole>) -> Self {
        ColorProp::DynamicTextRole(s)
    }
}

impl From<Signal<SurfaceRole>> for ColorProp {
    fn from(s: Signal<SurfaceRole>) -> Self {
        ColorProp::DynamicSurfaceRole(s)
    }
}

impl From<Signal<BorderRole>> for ColorProp {
    fn from(s: Signal<BorderRole>) -> Self {
        ColorProp::DynamicBorderRole(s)
    }
}

impl From<crate::styles::RecipeColor> for ColorProp {
    fn from(c: crate::styles::RecipeColor) -> Self {
        use crate::styles::RecipeColor;
        match c {
            RecipeColor::Static(color) => ColorProp::Static(color),
            RecipeColor::Surface(r) => ColorProp::SurfaceRole(r),
            RecipeColor::Border(r) => ColorProp::BorderRole(r),
            RecipeColor::Text(r) => ColorProp::TextRole(r),
        }
    }
}

// ---------------------------------------------------------------------------
// TextStyleProp — static or role-based TextStyle
// ---------------------------------------------------------------------------

use teksilo_tokens::{TextStyle, TextStyleRole};

/// A text-style input that is either a frozen `TextStyle` or a
/// [`TextStyleRole`] resolved against the current theme typography at
/// paint/layout time. Widgets that accept `impl Into<TextStyleProp>` follow
/// theme typography changes without the caller having to thread
/// `theme_signal` through their build.
#[derive(Clone, Debug)]
pub enum TextStyleProp {
    Static(TextStyle),
    Role(TextStyleRole),
}

impl TextStyleProp {
    /// Resolve the text style against the supplied typography tokens.
    pub fn resolve(&self, typography: &teksilo_tokens::TypographyTokens) -> TextStyle {
        match self {
            TextStyleProp::Static(s) => s.clone(),
            TextStyleProp::Role(role) => role.resolve(typography),
        }
    }
}

impl Default for TextStyleProp {
    fn default() -> Self {
        TextStyleProp::Role(TextStyleRole::Body)
    }
}

impl From<TextStyle> for TextStyleProp {
    fn from(s: TextStyle) -> Self {
        TextStyleProp::Static(s)
    }
}

impl From<TextStyleRole> for TextStyleProp {
    fn from(r: TextStyleRole) -> Self {
        TextStyleProp::Role(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::intui;

    #[test]
    fn accent_surface_dims_when_disabled() {
        let theme = intui::light();
        let prop = ColorProp::SurfaceRole(SurfaceRole::Accent);
        assert_eq!(prop.resolve(&theme, true), theme.colors.accent);
        assert_eq!(prop.resolve(&theme, false), theme.colors.accent_disabled);
    }

    // ── `undimmed` — colour that is state, not chrome ────────────────────────

    /// The default, for contrast: *any* text role dims in a disabled subtree,
    /// including the semantic ones. That is right for a label or an affordance,
    /// and wrong for a colour that is saying something still true.
    #[test]
    fn a_plain_text_role_dims_when_disabled_even_a_semantic_one() {
        let theme = intui::light();
        let prop = ColorProp::TextRole(TextRole::Success);
        assert_eq!(prop.resolve(&theme, true), theme.colors.text_success);
        assert_eq!(prop.resolve(&theme, false), theme.colors.text_disabled);
    }

    #[test]
    fn undimmed_keeps_its_role_in_a_disabled_subtree() {
        let theme = intui::light();
        let prop = ColorProp::undimmed(TextRole::Success);
        assert_eq!(prop.resolve(&theme, true), theme.colors.text_success);
        assert_eq!(
            prop.resolve(&theme, false),
            theme.colors.text_success,
            "a save indicator that can't be clicked still has to say whether the \
             work is on disk"
        );
    }

    #[test]
    fn undimmed_composes_with_surface_and_border_roles() {
        let theme = intui::light();
        let surface = ColorProp::undimmed(SurfaceRole::Accent);
        let border = ColorProp::undimmed(BorderRole::Accent);
        assert_eq!(surface.resolve(&theme, false), theme.colors.accent);
        assert_eq!(border.resolve(&theme, false), theme.colors.accent);
    }

    /// Wrapping a reactive colour must not silently unsubscribe it — the wrapper
    /// has to forward both the binding registration and the live value.
    #[test]
    fn undimmed_keeps_a_dynamic_role_reactive() {
        let theme = intui::light();
        let role = Signal::new(TextRole::Success);
        let prop = ColorProp::undimmed(role.clone());
        assert!(prop.is_bound(), "the wrapper must still report its binding");
        assert_eq!(prop.resolve(&theme, false), theme.colors.text_success);

        role.set(TextRole::Warning);
        assert_eq!(prop.resolve(&theme, false), theme.colors.text_warning);
    }

    #[test]
    fn accent_border_dims_when_disabled() {
        let theme = intui::light();
        let prop = ColorProp::BorderRole(BorderRole::Accent);
        assert_eq!(prop.resolve(&theme, true), theme.colors.accent);
        assert_eq!(prop.resolve(&theme, false), theme.colors.accent_disabled);
    }

    #[test]
    fn non_accent_surface_unchanged_when_disabled() {
        let theme = intui::light();
        let prop = ColorProp::SurfaceRole(SurfaceRole::Main);
        assert_eq!(prop.resolve(&theme, false), theme.colors.surface_main);
    }

    /// `Field` is the neutral twin of `Accent`: it dims to a *neutral* grey,
    /// not to the accent-tinted `accent_disabled`, so a greyed-out text field
    /// never renders as washed-out cyan.
    #[test]
    fn field_surface_dims_to_the_neutral_disabled_token() {
        let theme = intui::light();
        let prop = ColorProp::SurfaceRole(SurfaceRole::Field);
        assert_eq!(prop.resolve(&theme, true), theme.colors.surface_content);
        assert_eq!(prop.resolve(&theme, false), theme.colors.surface_disabled);
        assert_ne!(prop.resolve(&theme, false), theme.colors.accent_disabled);
    }

    #[test]
    fn field_border_dims_to_the_neutral_disabled_token() {
        let theme = intui::light();
        let prop = ColorProp::BorderRole(BorderRole::Field);
        assert_eq!(prop.resolve(&theme, true), theme.colors.border);
        assert_eq!(prop.resolve(&theme, false), theme.colors.border_disabled);
        assert_ne!(prop.resolve(&theme, false), theme.colors.accent_disabled);
    }

    /// The reason `Field` exists at all. A field and a passive `Panel` both
    /// used to paint `Content`, so the substitution could not dim one without
    /// greying the other — and so dimmed neither. They must still resolve
    /// identically while *enabled*, or the split would be a visible change.
    #[test]
    fn field_and_content_are_indistinguishable_until_disabled() {
        let theme = intui::light();
        let field = ColorProp::SurfaceRole(SurfaceRole::Field);
        let panel = ColorProp::SurfaceRole(SurfaceRole::Content);
        assert_eq!(field.resolve(&theme, true), panel.resolve(&theme, true));

        // Disabled: the field dims, the panel keeps its surface.
        assert_eq!(field.resolve(&theme, false), theme.colors.surface_disabled);
        assert_eq!(panel.resolve(&theme, false), theme.colors.surface_content);
    }

    #[test]
    fn new_cross_language_roles_resolve() {
        let c = &intui::light().colors;
        assert_eq!(TextRole::OnError.resolve(c), c.text_on_error);
        assert_eq!(
            TextRole::OnErrorContainer.resolve(c),
            c.text_on_error_container
        );
        assert_eq!(
            SurfaceRole::ErrorContainer.resolve(c),
            c.surface_error_container
        );
        assert_eq!(SurfaceRole::Container.resolve(c), c.surface_container);
        assert_eq!(
            SurfaceRole::ContainerRaised.resolve(c),
            c.surface_container_raised
        );
        assert_eq!(
            SurfaceRole::ContainerSunken.resolve(c),
            c.surface_container_sunken
        );
    }
}
