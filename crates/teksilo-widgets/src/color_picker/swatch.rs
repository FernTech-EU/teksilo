// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `ColorSwatch` — single clickable color cell with `Role::ColorWell`.
//!
//! Public widget so apps can compose their own swatch rows or palettes
//! outside of the bundled `SwatchGrid`. Renders an optional checkerboard
//! underlay when `color.a() < 1.0` so transparent swatches read correctly.
//! The displayed color is a `Prop<Color>` — pass a static `Color` for a
//! fixed palette entry or a `Signal<Color>` for a live preview that
//! re-paints whenever the bound value changes (used by `ColorPicker`'s
//! current-color preview and `ColorEdit`'s trigger swatch).
//!
//! ## Accessibility
//!
//! Declares `Role::ColorWell`; `set_color_value` carries the RGBA value
//! and `set_value` carries the formatted hex string so braille and
//! voice output both have a human-readable form. Selected swatches
//! append a localized "selected" suffix to their announced name.
//!
//! ```rust
//! # use teksilo_widgets::color_picker::ColorSwatch;
//! # use teksilo_tokens::Color;
//! let _swatch = ColorSwatch::new(Color::new(0.21, 0.52, 0.89, 1.0))
//!     .size(24.0)
//!     .corner_radius(4.0);
//! ```

use std::cell::Cell;
use std::rc::Rc;

use teksilo_canvas::{Canvas, Rect, Size, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::accesskit::{Action, Role};
use teksilo_core::build_context::BuildContext;
use teksilo_core::event::{EventResponse, Key, WidgetEvent};
use teksilo_core::focus::FocusOrigin;
use teksilo_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::{LocalizedString, resolve_message_widget};
use teksilo_tokens::{Color, CornerRadius};

use super::alpha_strip::paint_checkerboard;

type ActivateFn = Rc<dyn Fn(&mut EventContext)>;

/// Single-cell color swatch.
///
/// The displayed color is a `Prop<Color>` — pass a `Color` for a
/// static palette entry (the common case in `SwatchGrid`) or a
/// `Signal<Color>` for a live preview that re-paints when the bound
/// value changes (used by `ColorPicker`'s current-color preview and
/// `ColorEdit`'s trigger).
pub struct ColorSwatch {
    color: teksilo_core::signal::Prop<Color>,
    selected: bool,
    label: Option<LocalizedString>,
    size: Option<f32>,
    corner_radius: Option<f32>,
    /// Enabled state, static or reactive; forwarded to the arena at
    /// build time.
    enabled: teksilo_core::signal::Prop<bool>,
    on_activate: Option<ActivateFn>,
    focus_origin: Rc<Cell<Option<FocusOrigin>>>,
    /// Optional plain tooltip text shown after a hover delay. Mutually exclusive
    /// with the rich / composite slots — every setter clears the other two so
    /// the last call wins.
    tooltip_text: Option<LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body (arbitrary widget tree).
    composite_tooltip_content: Option<Box<dyn Widget>>,
}

impl ColorSwatch {
    /// Create a swatch displaying `color`. Accepts a static `Color` or a
    /// `Signal<Color>` (via `impl Into<Prop<Color>>`); a reactive value
    /// re-paints the cell whenever the signal changes.
    pub fn new(color: impl Into<teksilo_core::signal::Prop<Color>>) -> Self {
        Self {
            color: color.into(),
            selected: false,
            label: None,
            size: None,
            corner_radius: None,
            enabled: teksilo_core::signal::Prop::Static(true),
            on_activate: None,
            focus_origin: Rc::new(Cell::new(None)),
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
        }
    }

    /// Mark the swatch as currently selected, which paints an accent
    /// border and appends a localized "selected" suffix to the AT name.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Override the accessible label. Default is a localized "Color: #RRGGBB"
    /// string derived from the displayed color's hex value.
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the swatch cell size in logical pixels (square). Defaults to
    /// the theme's `recipe_color_picker_style::SWATCH_SIZE`.
    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size.max(0.0));
        self
    }

    /// Set the corner radius of the swatch cell in logical pixels.
    /// Defaults to `recipe_color_picker_style::SWATCH_CORNER_RADIUS`.
    pub fn corner_radius(mut self, r: f32) -> Self {
        self.corner_radius = Some(r.max(0.0));
        self
    }

    /// Set the enabled state, statically or reactively. Forwarded to the
    /// arena at build time.
    pub fn enabled(mut self, enabled: impl Into<teksilo_core::signal::Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Register an activation callback invoked on tap, Enter, Space, or
    /// the `Action::Click` accessibility action.
    pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_activate = Some(Rc::new(f));
        self
    }

    /// Attach a plain single-line tooltip shown after a hover delay.
    ///
    /// Mutually exclusive with [`Self::rich_tooltip`], [`Self::rich_tooltip_content`],
    /// and [`Self::composite_tooltip`] — this call clears the other slots.
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip looked up from the tooltip registry by key.
    ///
    /// Mutually exclusive with [`Self::tooltip`], [`Self::rich_tooltip_content`],
    /// and [`Self::composite_tooltip`] — this call clears the other slots.
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip with inline content (no registry lookup required).
    ///
    /// Mutually exclusive with [`Self::tooltip`], [`Self::rich_tooltip`],
    /// and [`Self::composite_tooltip`] — this call clears the other slots.
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip whose body is an arbitrary widget tree.
    ///
    /// Mutually exclusive with [`Self::tooltip`], [`Self::rich_tooltip`],
    /// and [`Self::rich_tooltip_content`] — this call clears the other slots.
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }
}

impl std::fmt::Debug for ColorSwatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColorSwatch")
            .field("color", &self.color.get())
            .field("selected", &self.selected)
            .field("enabled", &self.enabled.get())
            .finish_non_exhaustive()
    }
}

impl Widget for ColorSwatch {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Forward the enabled state into the arena; see IconButton.
        ctx.enabled_when(self_id, self.enabled.clone());
        let on_activate = self.on_activate.clone();
        // Framework gates events on `arena.is_enabled` and the focus
        // walker skips disabled subtrees.
        let mut handlers = HandlerSet::new()
            .focusable(true)
            .cursor(CursorIcon::Pointer);

        if let Some(cb) = on_activate.clone() {
            handlers = handlers.on_tap(move |_pos, ctx_evt| {
                cb(ctx_evt);
            });
        }
        if let Some(cb) = on_activate.clone() {
            handlers = handlers.on_key(move |event, ctx_evt| {
                let WidgetEvent::KeyDown { key, .. } = event else {
                    return EventResponse::Ignored;
                };
                match key {
                    Key::Enter | Key::Space => {
                        cb(ctx_evt);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }
        if let Some(cb) = on_activate {
            handlers = handlers.on_access_action(move |action, ctx_evt| match action {
                Action::Click => {
                    cb(ctx_evt);
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            });
        }

        {
            let focus_origin = self.focus_origin.clone();
            handlers = handlers.on_focus(move |gained, _ctx| {
                focus_origin.set(if gained {
                    Some(FocusOrigin::Keyboard)
                } else {
                    None
                });
            });
        }

        ctx.apply_self_handlers(handlers);

        // Tooltip attachment — mutually exclusive slots, last setter wins.
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, self_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, self_id, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let tooltip_widget = crate::tooltip::TooltipWidget::new(text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(self_id, tooltip_id, delay);
        }

        // Reactive: when `color` is bound to a Signal, re-paint and
        // refresh the AT color value whenever it changes. Static
        // colors register nothing (Prop::Static).
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.color.register_if_bound(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::AccessibilityOnly,
        );
        self.color.register_if_bound(
            self_id,
            registry,
            teksilo_core::binding::BindingLevel::RepaintOnly,
        );

        Vec::new()
    }

    fn layout_response(&self, _proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
        use crate::styles::recipe_color_picker_style as cp;
        let size = self.size.unwrap_or(cp::SWATCH_SIZE);
        Size::new(size, size).into()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        use crate::styles::recipe_color_picker_style as cp;
        let radius = CornerRadius::uniform(self.corner_radius.unwrap_or(cp::SWATCH_CORNER_RADIUS));
        let color = self.color.get();

        // Checkerboard underlay if the swatch is partly transparent.
        if color.a() < 1.0 {
            paint_checkerboard(
                canvas,
                bounds,
                cp::CHECKER_CELL,
                cp::CHECKER_COLOR_A,
                cp::CHECKER_COLOR_B,
            );
        }

        canvas.fill_rounded_rect(bounds, radius, color);

        // Selection ring.
        if self.selected {
            canvas.stroke_rounded_rect(
                bounds,
                radius,
                ctx.theme.colors.accent,
                cp::SWATCH_SELECTED_STROKE_WIDTH,
            );
        } else {
            // Always draw a hairline border so light swatches don't
            // disappear into a light surface.
            canvas.stroke_rounded_rect(bounds, radius, ctx.theme.colors.border, 1.0);
        }

        // Focus ring (keyboard).
        if self.focus_origin.get() == Some(FocusOrigin::Keyboard) {
            let offset = ctx.theme.shape.focus_ring_offset;
            let half = ctx.theme.shape.focus_ring_width * 0.5;
            let inset = offset + half;
            let ring = Rect::new(
                bounds.x - inset,
                bounds.y - inset,
                bounds.width + inset * 2.0,
                bounds.height + inset * 2.0,
            );
            canvas.stroke_rounded_rect(
                ring,
                CornerRadius::uniform(
                    self.corner_radius.unwrap_or(cp::SWATCH_CORNER_RADIUS) + inset,
                ),
                ctx.theme.colors.focus_ring,
                ctx.theme.shape.focus_ring_width,
            );
        }
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::ColorWell);
        let color = self.color.get();
        builder.set_color_value(color);
        let hex = color.to_hex_upper(color.a() < 1.0);
        let name = match &self.label {
            Some(ls) => ls.resolve_now(),
            None => {
                resolve_message_widget("color-picker-swatch-label", &[("hex", hex.clone().into())])
            }
        };
        let display = if self.selected {
            let suffix = resolve_message_widget("color-picker-swatch-selected-suffix", &[]);
            format!("{}{}", name, suffix)
        } else {
            name
        };
        builder.set_name(display);
        builder.set_value(hex);
        if self.selected {
            builder.set_selected(true);
        }
        // Framework a11y walker sets `set_disabled` from arena state.
        builder.add_action(Action::Click);
        builder.add_action(Action::Focus);
    }

    fn place_children(
        &self,
        _bounds: Rect,
        _proposal: SizeProposal,
        _children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
    }
}
