//! `ColorEdit` — compact field-style color picker trigger that opens
//! a popover containing a [`ColorPicker`].
//!
//! Direct analog of [`DateEdit`](crate::date_edit::DateEdit). The
//! trigger is a [`Button`] with a reactive [`ColorSwatch`] in its
//! leading slot, the current hex as the label, and an optional
//! chevron in its trailing slot. Click, Enter, Space, or Alt+Down
//! opens the popover; Escape or click-outside closes it. The inner
//! picker writes through the same bound `Signal<Color>`, so external
//! observers see live updates as the user drags within the popover
//! (no commit step).
//!
//! Built on [`PopoverButton`]:
//! the overlay wiring (dormant content + show / dismiss + AT
//! `has_popup` + `expanded`) lives there. This file is just the
//! ColorEdit-specific assembly — picker config pass-through, the
//! reactive trigger, and the nullable-binding bridge.
//!
//! # Accessibility
//!
//! The trigger declares `Role::Button`
//! (via Button), `HasPopup::Dialog`
//! (via PopoverButton), and tracks the popover open state through
//! `set_expanded`. The label binds reactively to the hex value so
//! AT name updates as the picker mutates the bound color.

use bastyde_i18n::lit;
use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::build_context::BuildContext;
use bastyde_core::overlay::{DismissBehavior, OverlayPlacement};
use bastyde_core::signal::Signal;
use bastyde_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_builder::WidgetBuilder;
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::{LocalizedString, resolve_message_widget};
use bastyde_tokens::Color;

use crate::button::Button;
use crate::color_picker::{ColorPicker, ColorPickerLayout, ColorSwatch};
use crate::popover_widget::PopoverButton;
use crate::primitives::IconWidget;

type OnVoid = Rc<dyn Fn()>;

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
    /// Initial enabled-state; forwarded to the arena at build time.
    initial_enabled: bool,
    on_open: Option<OnVoid>,
    on_close: Option<OnVoid>,

    // Internal state.
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for ColorEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColorEdit")
            .field("alpha_enabled", &self.alpha_enabled)
            .field("picker_layout", &self.picker_layout)
            .field("initial_enabled", &self.initial_enabled)
            .finish_non_exhaustive()
    }
}

impl ColorEdit {
    pub fn new(value: Signal<Color>) -> Self {
        Self::from_binding(ColorBinding::Required(value))
    }

    pub fn nullable(value: Signal<Option<Color>>) -> Self {
        let proxy = Signal::new(value.get().unwrap_or(Color::TRANSPARENT));
        Self::from_binding(ColorBinding::Nullable {
            source: value,
            proxy,
        })
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
            initial_enabled: true,
            on_open: None,
            on_close: None,
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

    /// Set the initial enabled state. Forwarded to the arena at build time.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.initial_enabled = enabled;
        self
    }

    /// Install a callback fired when the color-picker popover opens.
    ///
    /// The signature is `Fn()` (no [`EventContext`](bastyde_core::widget::EventContext))
    /// because `on_close` is invoked from the overlay-dismiss path,
    /// which has no ctx in scope. To keep the open/close pair
    /// symmetric, `on_open` matches. If you need ctx in a
    /// color-editing-mode callback, attach an `on_tap` on a sibling
    /// trigger that wakes the editor explicitly.
    pub fn on_open(mut self, f: impl Fn() + 'static) -> Self {
        self.on_open = Some(Rc::new(f));
        self
    }

    /// Install a callback fired when the color-picker popover closes.
    /// See [`on_open`](Self::on_open) for why this is `Fn()` and not
    /// `Fn(&mut EventContext)`.
    pub fn on_close(mut self, f: impl Fn() + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }
}

impl Widget for ColorEdit {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Forward initial-enabled into the arena; see IconButton.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }

        // Bridge nullable binding ↔ proxy. The picker writes the
        // proxy; we mirror that to the source as Some(c). External
        // changes to source flow back into proxy. Empty (None) state
        // is purely visual on the trigger — there is no in-trigger
        // "clear" affordance (apps compose a separate Clear button).
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
        let alpha_enabled = self.alpha_enabled;
        use crate::styles::recipe_color_picker_style as cp;

        // Snapshot of the bound color at popover-open time. Cancel
        // restores this; Done leaves the picker's writes intact. The
        // initial value seeds the snapshot for the first open before
        // the open-transition effect has a chance to refresh it.
        let snapshot = ctx.signal(value.get());

        // ── Build the picker (handed to PopoverButton as content) ──
        let mut picker = ColorPicker::new(value.clone())
            .alpha_enabled(alpha_enabled)
            .layout(self.picker_layout)
            .show_rgb_spinners(self.show_rgb_spinners)
            .show_hsv_spinners(self.show_hsv_spinners)
            .show_hex_input(self.show_hex_input)
            .swatch_columns(self.swatch_columns)
            .show_footer(true)
            .on_done(|ctx_evt| {
                ctx_evt.dismiss_self_overlay_chain();
            })
            .on_cancel({
                let value = value.clone();
                let snapshot = snapshot.clone();
                move |ctx_evt| {
                    let prior = snapshot.get();
                    if value.get() != prior {
                        value.set(prior);
                    }
                    ctx_evt.dismiss_self_overlay_chain();
                }
            })
            .enabled(self.initial_enabled);
        if let Some(s) = self.swatches.clone() {
            picker = picker.swatches(s);
        }
        if let Some(s) = self.swatches_signal.clone() {
            picker = picker.swatches_signal(s);
        }

        // ── Build the trigger ──
        let swatch_size = self.trigger_swatch_size.unwrap_or(cp::PREVIEW_HEIGHT);

        // ColorSwatch accepts `impl Into<Prop<Color>>` — pass the
        // bound signal so it re-paints whenever the picker mutates
        // the value. `.access_hidden(true)` so the swatch's own
        // ColorWell role doesn't appear as a redundant child of the
        // trigger Button's Role::Button.
        let swatch = ColorSwatch::new(value.clone())
            .size(swatch_size)
            .corner_radius(cp::PREVIEW_CORNER_RADIUS)
            .enabled(false)
            .access_hidden(true);

        // Reactive hex / placeholder for the Button label. For the
        // nullable variant, None → localized "—" placeholder; Some →
        // formatted hex. For the required variant, just formatted hex.
        let label_signal = match &self.binding {
            ColorBinding::Required(s) => {
                let alpha = alpha_enabled;
                if self.show_hex_in_trigger {
                    s.map(move |c| c.to_hex_upper(alpha))
                } else {
                    s.map(|_| String::new())
                }
            }
            ColorBinding::Nullable { source, .. } => {
                let alpha = alpha_enabled;
                let show_hex = self.show_hex_in_trigger;
                source.map(move |opt| match opt {
                    Some(c) if show_hex => c.to_hex_upper(alpha),
                    Some(_) => String::new(),
                    None => resolve_message_widget("color-edit-trigger-empty-placeholder", &[]),
                })
            }
        };

        // App-supplied `.label(...)` replaces the entire visible text
        // (and therefore the AT name) with a static localized string.
        // Apps that want their custom label PLUS a visible swatch can
        // pair `.label(...)` with `.show_hex_in_trigger(false)`. When
        // no label is set, the bound hex signal feeds the Button label
        // — every value mutation refreshes the visible text and the
        // AT name reactively via Button's `bind_label` plumbing.
        let trigger = if let Some(ls) = self.label.take() {
            Button::new(ls)
        } else {
            Button::new(lit!("")).bind_label(label_signal)
        };
        let mut trigger = trigger.enabled(self.initial_enabled).leading(swatch);
        if self.show_chevron {
            trigger = trigger.trailing(IconWidget::chevron_down(12.0).access_hidden(true));
        }

        // ── Wrap in PopoverButton ──
        let pb = PopoverButton::new(trigger)
            .content(picker)
            .placement(self.placement.clone())
            .dismiss_behavior(self.dismiss_behavior.clone());

        // Refresh the cancel-snapshot whenever the popover transitions
        // to open. This must run BEFORE the user has a chance to
        // mutate the value through the picker — `open_signal()` flips
        // synchronously inside the activate handler, before any drag
        // events reach the canvas / strips, so the snapshot captures
        // the value that was bound at the moment of open.
        {
            let open_signal = pb.open_signal();
            let snapshot = snapshot.clone();
            let value = value.clone();
            ctx.effect(&open_signal, move |opened| {
                if *opened {
                    let current = value.get();
                    if snapshot.get() != current {
                        snapshot.set(current);
                    }
                }
            });
        }

        let mut pb = pb;
        if let Some(cb) = self.on_open.take() {
            pb = pb.on_open(move || cb());
        }
        if let Some(cb) = self.on_close.take() {
            pb = pb.on_close(move || cb());
        }

        let pb_id = ctx.add(pb);
        self.root_child_id = Some(pb_id);
        vec![pb_id]
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

    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {
        // Transparent — the inner Button (via PopoverButton) declares
        // Role::Button + has_popup + expanded + name. Adding anything
        // here would create a duplicate AT element above the trigger.
    }
}
