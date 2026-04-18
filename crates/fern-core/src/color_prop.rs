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

use fern_tokens::{BorderRole, Color, SurfaceRole, TextRole, Theme};

use crate::binding::{BindingLevel, BindingRegistry};
use crate::signal::{Prop, Signal};
use crate::widget_id::WidgetId;

/// A color value that may be static, reactive, or driven by a theme role.
#[derive(Clone)]
pub enum ColorProp {
    /// Frozen literal color.
    Static(Color),
    /// Reactive signal.
    Bound(Signal<Color>),
    /// Resolved against the theme's foreground palette at paint time.
    TextRole(TextRole),
    /// Resolved against the theme's surface / background palette at paint time.
    SurfaceRole(SurfaceRole),
    /// Resolved against the theme's border palette at paint time.
    BorderRole(BorderRole),
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
        }
    }

    /// Register dirty-tracking for the Bound variant so signal updates
    /// trigger a widget repaint. Role variants need no registration — theme
    /// changes already dirty-mark everything.
    pub fn register_if_bound(
        &self,
        widget_id: WidgetId,
        registry: &BindingRegistry,
        level: BindingLevel,
    ) {
        if let ColorProp::Bound(signal) = self {
            signal.bind_to(widget_id, registry, level);
        }
    }

    /// Whether this prop carries a signal binding. Used by callers that want
    /// to skip registration work when the prop is a static literal.
    pub fn is_bound(&self) -> bool {
        matches!(self, ColorProp::Bound(_))
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
