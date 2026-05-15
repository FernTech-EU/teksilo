//! `ColorSwatch` — single clickable color cell with `Role::ColorWell`.
//!
//! Public widget so apps can compose their own swatch rows / palettes
//! outside of the bundled [`SwatchGrid`](super::swatch_grid::SwatchGrid).
//! Renders an optional checkerboard underlay when `color.a() < 1.0` so
//! transparent swatches read correctly.

use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Canvas, Rect, Size, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::accesskit::{Action, Role};
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::focus::FocusOrigin;
use fern_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_i18n::{LocalizedString, resolve_message_widget};
use fern_tokens::{Color, CornerRadius};

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
    color: fern_core::signal::Prop<Color>,
    selected: bool,
    label: Option<LocalizedString>,
    size: Option<f32>,
    corner_radius: Option<f32>,
    enabled: bool,
    on_activate: Option<ActivateFn>,
    focus_origin: Rc<Cell<Option<FocusOrigin>>>,
}

impl ColorSwatch {
    pub fn new(color: impl Into<fern_core::signal::Prop<Color>>) -> Self {
        Self {
            color: color.into(),
            selected: false,
            label: None,
            size: None,
            corner_radius: None,
            enabled: true,
            on_activate: None,
            focus_origin: Rc::new(Cell::new(None)),
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size.max(0.0));
        self
    }

    pub fn corner_radius(mut self, r: f32) -> Self {
        self.corner_radius = Some(r.max(0.0));
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn on_activate_fn(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_activate = Some(Rc::new(f));
        self
    }
}

impl std::fmt::Debug for ColorSwatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColorSwatch")
            .field("color", &self.color.get())
            .field("selected", &self.selected)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl Widget for ColorSwatch {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let enabled = self.enabled;
        let on_activate = self.on_activate.clone();
        let mut handlers = HandlerSet::new()
            .focusable(enabled)
            .cursor(CursorIcon::Pointer);

        if let Some(cb) = on_activate.clone() {
            handlers = handlers.on_tap(move |_pos, ctx_evt| {
                if !enabled {
                    return;
                }
                cb(ctx_evt);
            });
        }
        if let Some(cb) = on_activate.clone() {
            handlers = handlers.on_key(move |event, ctx_evt| {
                if !enabled {
                    return EventResponse::Ignored;
                }
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

        // Reactive: when `color` is bound to a Signal, re-paint and
        // refresh the AT color value whenever it changes. Static
        // colors register nothing (Prop::Static).
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        self.color.register_if_bound(
            self_id,
            registry,
            fern_core::binding::BindingLevel::AccessibilityOnly,
        );
        self.color.register_if_bound(
            self_id,
            registry,
            fern_core::binding::BindingLevel::RepaintOnly,
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
        let radius =
            CornerRadius::uniform(self.corner_radius.unwrap_or(cp::SWATCH_CORNER_RADIUS));
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
        if !self.enabled {
            builder.set_disabled();
        }
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
