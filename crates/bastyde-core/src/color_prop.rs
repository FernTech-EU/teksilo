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

use bastyde_tokens::{BorderRole, Color, SurfaceRole, TextRole};

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
}

impl ColorProp {
    /// Produce the color value for the supplied theme. Callers invoke this in
    /// paint (where `ctx.theme` is in scope). Static and Bound variants
    /// ignore the theme; role variants resolve against it.
    pub fn resolve(&self, theme: &Theme) -> Color {
        match self {
            ColorProp::Static(c) => *c,
            ColorProp::Bound(s) => s.get(),
            ColorProp::TextRole(role) => role.resolve(&theme.colors),
            ColorProp::SurfaceRole(role) => role.resolve(&theme.colors),
            ColorProp::BorderRole(role) => role.resolve(&theme.colors),
            ColorProp::DynamicTextRole(s) => s.get().resolve(&theme.colors),
            ColorProp::DynamicSurfaceRole(s) => s.get().resolve(&theme.colors),
            ColorProp::DynamicBorderRole(s) => s.get().resolve(&theme.colors),
        }
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
            _ => {}
        }
    }

    /// Whether this prop carries any signal binding.
    pub fn is_bound(&self) -> bool {
        matches!(
            self,
            ColorProp::Bound(_)
                | ColorProp::DynamicTextRole(_)
                | ColorProp::DynamicSurfaceRole(_)
                | ColorProp::DynamicBorderRole(_)
        )
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

// ---------------------------------------------------------------------------
// TextStyleProp — static or role-based TextStyle
// ---------------------------------------------------------------------------

use bastyde_tokens::{TextStyle, TextStyleRole};

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
    pub fn resolve(&self, typography: &bastyde_tokens::TypographyTokens) -> TextStyle {
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
