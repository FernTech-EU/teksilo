//! `ColorEdit` — compact field-style color picker trigger that opens
//! a popover containing a [`ColorPicker`].
//!
//! Direct analog of [`DateEdit`](crate::date_edit::DateEdit). The
//! trigger renders a small `ColorSwatch` plus an optional hex readout
//! plus an optional chevron, inside a focusable bordered frame. Click,
//! Enter, Space, or Alt+Down opens the popover; Escape or click-outside
//! closes it. The inner picker writes through the same bound
//! `Signal<Color>`, so external observers see live updates as the user
//! drags within the popover (no commit step).
//!
//! Overlay wiring mirrors DateEdit verbatim: the picker is added in
//! `build()` and immediately marked dormant via `ctx.set_dormant(...)`,
//! then woken via `ctx_evt.activate(...)` + `ctx_evt.show_overlay(...)`
//! when the trigger fires. The dismiss callback flips a `popover_open`
//! signal back to `false` so accessibility's `set_expanded(...)` stays
//! truthful and the trigger button's visual state matches.
//!
//! # Accessibility
//!
//! Trigger: [`Role::Button`](accesskit::Role::Button) with
//! `set_color_value(...)`, `set_value(hex)`, `set_has_popup(Dialog)`,
//! and `set_expanded(popover_open)`. Default name resolves to
//! "Color #RRGGBB" via the `color-edit-trigger-name` Fluent key (apps
//! override via `.label(...)`).

use std::cell::Cell;
use std::rc::Rc;

use fern_canvas::{Canvas, Rect, SizeProposal};
use fern_core::accessibility::AccessNodeBuilder;
use fern_core::accesskit::{Action, HasPopup, Role};
use fern_core::build_context::BuildContext;
use fern_core::event::{EventResponse, Key, WidgetEvent};
use fern_core::focus::FocusOrigin;
use fern_core::overlay::{
    DismissBehavior, OverlayDismissCallback, OverlayLayer, OverlayPlacement, OverlayRequest,
};
use fern_core::signal::Signal;
use fern_core::widget::{
    CursorIcon, EventContext, LayoutContext, LayoutResponse, PaintContext, Widget, WidgetPlacement,
};
use fern_core::widget_builder::HandlerSet;
use fern_core::widget_id::WidgetId;
use fern_i18n::{LocalizedString, resolve_message_widget};
use fern_tokens::{Color, CornerRadius};

use crate::color_picker::{ColorPicker, ColorPickerLayout, ColorSwatch};
use crate::primitives::{HStack, IconWidget, Padding, TextWidget};

type OnVoid = Rc<dyn Fn(&mut EventContext)>;

#[derive(Clone)]
enum ColorBinding {
    Required(Signal<Color>),
    Nullable {
        source: Signal<Option<Color>>,
        proxy: Signal<Color>,
    },
}

impl ColorBinding {
    fn proxy(&self) -> Signal<Color> {
        match self {
            Self::Required(s) => s.clone(),
            Self::Nullable { proxy, .. } => proxy.clone(),
        }
    }

    fn external_is_some(&self) -> bool {
        match self {
            Self::Required(_) => true,
            Self::Nullable { source, .. } => source.get().is_some(),
        }
    }
}

/// Compact color cell that opens a [`ColorPicker`] in a popover.
pub struct ColorEdit {
    binding: ColorBinding,

    // Picker pass-through.
    alpha_enabled: bool,
    swatches: Option<Vec<Color>>,
    swatches_signal: Option<Signal<Vec<Color>>>,
    swatch_columns: usize,
    picker_layout: ColorPickerLayout,
    show_rgb_spinners: bool,
    show_hsv_spinners: bool,
    show_hex_input: bool,

    // Trigger appearance.
    show_hex_in_trigger: bool,
    show_chevron: bool,
    trigger_swatch_size: Option<f32>,

    // Popover.
    placement: OverlayPlacement,
    dismiss_behavior: DismissBehavior,

    // Composite.
    label: Option<LocalizedString>,
    enabled: bool,
    on_open: Option<OnVoid>,
    on_close: Option<OnVoid>,

    // Internal state.
    popover_open: Signal<bool>,
    focused_within: Signal<bool>,
    focus_origin: Rc<Cell<Option<FocusOrigin>>>,
    picker_id: Option<WidgetId>,
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for ColorEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColorEdit")
            .field("alpha_enabled", &self.alpha_enabled)
            .field("picker_layout", &self.picker_layout)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

impl ColorEdit {
    pub fn new(value: Signal<Color>) -> Self {
        Self::from_binding(ColorBinding::Required(value))
    }

    pub fn nullable(value: Signal<Option<Color>>) -> Self {
        let proxy = Signal::new(value.get().unwrap_or(Color::TRANSPARENT));
        Self::from_binding(ColorBinding::Nullable { source: value, proxy })
    }

    fn from_binding(binding: ColorBinding) -> Self {
        Self {
            binding,
            alpha_enabled: false,
            swatches: None,
            swatches_signal: None,
            swatch_columns: 6,
            picker_layout: ColorPickerLayout::Compact,
            show_rgb_spinners: true,
            show_hsv_spinners: false,
            show_hex_input: true,
            show_hex_in_trigger: true,
            show_chevron: true,
            trigger_swatch_size: None,
            placement: OverlayPlacement::BelowPreferred,
            dismiss_behavior: DismissBehavior::EscapeOrClickOutside,
            label: None,
            enabled: true,
            on_open: None,
            on_close: None,
            popover_open: Signal::new(false),
            focused_within: Signal::new(false),
            focus_origin: Rc::new(Cell::new(None)),
            picker_id: None,
            root_child_id: None,
        }
    }

    pub fn alpha_enabled(mut self, enabled: bool) -> Self {
        self.alpha_enabled = enabled;
        self
    }

    pub fn swatches(mut self, s: Vec<Color>) -> Self {
        self.swatches = Some(s);
        self
    }

    pub fn swatches_signal(mut self, s: Signal<Vec<Color>>) -> Self {
        self.swatches_signal = Some(s);
        self
    }

    pub fn swatch_columns(mut self, n: usize) -> Self {
        self.swatch_columns = n.max(1);
        self
    }

    pub fn picker_layout(mut self, l: ColorPickerLayout) -> Self {
        self.picker_layout = l;
        self
    }

    pub fn show_rgb_spinners(mut self, s: bool) -> Self {
        self.show_rgb_spinners = s;
        self
    }

    pub fn show_hsv_spinners(mut self, s: bool) -> Self {
        self.show_hsv_spinners = s;
        self
    }

    pub fn show_hex_input(mut self, s: bool) -> Self {
        self.show_hex_input = s;
        self
    }

    pub fn show_hex_in_trigger(mut self, s: bool) -> Self {
        self.show_hex_in_trigger = s;
        self
    }

    pub fn show_chevron(mut self, s: bool) -> Self {
        self.show_chevron = s;
        self
    }

    pub fn trigger_swatch_size(mut self, size: f32) -> Self {
        self.trigger_swatch_size = Some(size.max(0.0));
        self
    }

    pub fn placement(mut self, p: OverlayPlacement) -> Self {
        self.placement = p;
        self
    }

    pub fn dismiss_behavior(mut self, b: DismissBehavior) -> Self {
        self.dismiss_behavior = b;
        self
    }

    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn on_open(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_open = Some(Rc::new(f));
        self
    }

    pub fn on_close(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }
}

impl Widget for ColorEdit {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // ── Bridge nullable binding ↔ proxy (mirror ColorPicker) ──
        if let ColorBinding::Nullable { source, proxy } = &self.binding {
            {
                let proxy = proxy.clone();
                ctx.effect(source, move |new| {
                    let resolved = new.unwrap_or(Color::TRANSPARENT);
                    if proxy.get() != resolved {
                        proxy.set(resolved);
                    }
                });
            }
            {
                let source = source.clone();
                ctx.effect(proxy, move |c| {
                    if source.get() != Some(*c) {
                        source.set(Some(*c));
                    }
                });
            }
        }

        let value = self.binding.proxy();
        let style_snapshot = ctx.theme_signal().get().components.color_picker;
        let alpha_enabled = self.alpha_enabled;
        let enabled = self.enabled;
        let popover_open = self.popover_open.clone();

        // ── Pre-build the picker (dormant) ──
        let mut picker = ColorPicker::new(value.clone())
            .alpha_enabled(alpha_enabled)
            .layout(self.picker_layout)
            .show_rgb_spinners(self.show_rgb_spinners)
            .show_hsv_spinners(self.show_hsv_spinners)
            .show_hex_input(self.show_hex_input)
            .swatch_columns(self.swatch_columns)
            .enabled(enabled);
        if let Some(s) = self.swatches.clone() {
            picker = picker.swatches(s);
        }
        if let Some(s) = self.swatches_signal.clone() {
            picker = picker.swatches_signal(s);
        }
        let picker_id = ctx.add(picker);
        ctx.set_dormant(picker_id);
        self.picker_id = Some(picker_id);

        // ── Trigger ──
        // The trigger is the entire ColorEdit footprint: a swatch +
        // optional hex + optional chevron, inside a focusable bordered
        // frame. We render the frame ourselves in paint(); the children
        // are arranged by an inner HStack we add as the only structural
        // child.
        let preview_color = value.get();
        let swatch_size = self
            .trigger_swatch_size
            .unwrap_or(style_snapshot.preview_height);

        let swatch_widget = ColorSwatch::new(preview_color)
            .size(swatch_size)
            .corner_radius(style_snapshot.preview_corner_radius)
            // Disable the swatch's own a11y so the trigger frame owns
            // the Role::Button + color value declaration.
            .enabled(false);

        let mut row = HStack::new()
            .spacing(8.0)
            .child(swatch_widget);

        if self.show_hex_in_trigger && self.binding.external_is_some() {
            let hex = value.get().to_hex_upper(self.alpha_enabled);
            row = row.child(TextWidget::new(hex));
        } else if !self.binding.external_is_some() {
            // Nullable + None: render the localized empty-placeholder
            // glyph (default "—", overridable per locale).
            row = row.child(TextWidget::new(resolve_message_widget(
                "color-edit-trigger-empty-placeholder",
                &[],
            )));
        }

        if self.show_chevron {
            row = row.child(IconWidget::chevron_down(12.0));
        }

        let trigger_inner = Padding::uniform(6.0).child(row);
        let trigger_id = ctx.add(trigger_inner);
        self.root_child_id = Some(trigger_id);

        // ── Trigger handlers ──
        let dismiss_cb: OverlayDismissCallback = {
            let popover_open = popover_open.clone();
            let on_close = self.on_close.clone();
            Rc::new(move || {
                popover_open.set(false);
                let _ = on_close.as_ref();
            })
        };

        let self_ref = ctx.self_id();
        let placement = self.placement.clone();
        let dismiss_behavior = self.dismiss_behavior.clone();
        let on_open = self.on_open.clone();
        let activate: OnVoid = {
            let popover_open = popover_open.clone();
            let dismiss_cb = dismiss_cb.clone();
            Rc::new(move |ctx_evt: &mut EventContext| {
                if popover_open.get() {
                    popover_open.set(false);
                    ctx_evt.dismiss_all_overlays();
                } else {
                    popover_open.set(true);
                    ctx_evt.activate(picker_id);
                    ctx_evt.show_overlay(OverlayRequest {
                        content_id: picker_id,
                        anchor: self_ref,
                        placement: placement.clone(),
                        dismiss: dismiss_behavior.clone(),
                        layer: OverlayLayer::InTree,
                        parent_overlay: None,
                        on_dismiss: Some(dismiss_cb.clone()),
                        fade_duration: None,
                    });
                    ctx_evt.request_focus(picker_id);
                    if let Some(cb) = on_open.as_ref() {
                        cb(ctx_evt);
                    }
                }
            })
        };

        let mut handlers = HandlerSet::new()
            .focusable(enabled)
            .focus_within(self.focused_within.clone())
            .cursor(CursorIcon::Pointer);

        {
            let activate = activate.clone();
            handlers = handlers.on_tap(move |_pos, ctx_evt| {
                if !enabled {
                    return;
                }
                activate(ctx_evt);
            });
        }
        {
            let activate = activate.clone();
            handlers = handlers.on_key(move |event, ctx_evt| {
                if !enabled {
                    return EventResponse::Ignored;
                }
                let WidgetEvent::KeyDown { key, modifiers, .. } = event else {
                    return EventResponse::Ignored;
                };
                match key {
                    Key::Enter | Key::Space => {
                        activate(ctx_evt);
                        EventResponse::Handled
                    }
                    Key::ArrowDown if modifiers.alt() => {
                        activate(ctx_evt);
                        EventResponse::Handled
                    }
                    _ => EventResponse::Ignored,
                }
            });
        }
        {
            let activate = activate.clone();
            handlers = handlers.on_access_action(move |action, ctx_evt| match action {
                Action::Click => {
                    activate(ctx_evt);
                    EventResponse::Handled
                }
                _ => EventResponse::Ignored,
            });
        }
        {
            let focus_origin = self.focus_origin.clone();
            handlers = handlers.on_focus(move |gained, _ctx| {
                focus_origin.set(if gained { Some(FocusOrigin::Keyboard) } else { None });
            });
        }
        ctx.apply_self_handlers(handlers);

        // Bind value + popover_open at AccessibilityOnly so the trigger's
        // a11y node refreshes its color_value / set_value / set_expanded
        // when the bound color changes or the popover toggles.
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        value.bind_to(self_id, registry, fern_core::binding::BindingLevel::AccessibilityOnly);
        self.popover_open
            .bind_to(self_id, registry, fern_core::binding::BindingLevel::AccessibilityOnly);

        vec![trigger_id]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        match self.root_child_id {
            Some(id) => ctx
                .child_layout_response(id, proposal)
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0).into()),
            None => proposal.resolve(0.0, 0.0).into(),
        }
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn children(&self) -> Vec<WidgetId> {
        self.root_child_id.into_iter().collect()
    }

    fn paint(&self, bounds: Rect, canvas: &mut Canvas, ctx: &PaintContext) {
        // Frame border + subtle background, matching the TextInput field
        // chrome. The inner HStack child paints the swatch, hex, chevron.
        let theme = &ctx.theme;
        let radius = CornerRadius::uniform(theme.shape.radius_control);
        let bg = if self.popover_open.get() {
            theme.colors.surface_sunken
        } else {
            theme.colors.surface_content
        };
        canvas.fill_rounded_rect(bounds, radius, bg);
        let border_color = if self.focused_within.get() {
            theme.colors.focus_ring
        } else {
            theme.colors.border
        };
        let border_width = if self.focused_within.get() {
            theme.shape.focus_ring_width
        } else {
            1.0
        };
        canvas.stroke_rounded_rect(bounds, radius, border_color, border_width);
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::Button);
        let current = self.binding.proxy().get();
        let hex = current.to_hex_upper(self.alpha_enabled);

        // Default name: localized "Color #RRGGBB" (or "Color, none" when nullable+empty).
        let name = match &self.label {
            Some(ls) => ls.resolve_now(),
            None => {
                if !self.binding.external_is_some() {
                    resolve_message_widget("color-edit-trigger-name-empty", &[])
                } else {
                    resolve_message_widget(
                        "color-edit-trigger-name",
                        &[("hex", hex.clone().into())],
                    )
                }
            }
        };
        builder.set_name(name);

        // Color value advertised even on a Button — AccessKit allows
        // color_value on any node and macOS VoiceOver announces it
        // appropriately.
        if self.binding.external_is_some() {
            builder.set_color_value(current);
            builder.set_value(hex);
        }
        builder.set_has_popup(HasPopup::Dialog);
        builder.set_expanded(self.popover_open.get());
        if !self.enabled {
            builder.set_disabled();
        }
        builder.add_action(Action::Click);
        builder.add_action(Action::Focus);
    }
}

