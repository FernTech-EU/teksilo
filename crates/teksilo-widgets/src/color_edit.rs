// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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
//!
//! # Example
//!
//! ```ignore
//! use teksilo_core::signal::Signal;
//! use teksilo_tokens::Color;
//!
//! let color = ctx.signal(Color::new(0.21, 0.52, 0.89, 1.0));
//! let _edit = ColorEdit::new(color)
//!     .alpha_enabled(true)
//!     .show_chevron(true);
//! ```

use std::rc::Rc;
use teksilo_i18n::lit;

use teksilo_canvas::{Rect, SizeProposal};
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::build_context::BuildContext;
use teksilo_core::overlay::{DismissBehavior, OverlayPlacement};
use teksilo_core::signal::{Prop, Signal};
use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use teksilo_core::widget_builder::WidgetBuilder;
use teksilo_core::widget_id::WidgetId;
use teksilo_i18n::{LocalizedString, resolve_message_widget};
use teksilo_tokens::Color;

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

/// Compact color cell that opens a full [`ColorPicker`] in a popover when activated.
pub struct ColorEdit {
    binding: ColorBinding,

    // Picker pass-through.
    alpha_enabled: bool,
    swatches: Option<Prop<Vec<Color>>>,
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
    /// Enabled state, static or reactive; forwarded to the arena at
    /// build time.
    enabled: Prop<bool>,
    on_open: Option<OnVoid>,
    on_close: Option<OnVoid>,

    // Tooltip slots (mutually exclusive; last setter wins).
    /// Optional plain tooltip text shown after a hover delay. Mutually exclusive
    /// with the rich / composite slots — every setter clears the other two so
    /// the last call wins.
    tooltip_text: Option<LocalizedString>,
    /// Optional rich tooltip source (registry key or inline content).
    rich_tooltip_source: Option<crate::tooltip::RichTooltipSource>,
    /// Optional composite tooltip body (arbitrary widget tree).
    composite_tooltip_content: Option<Box<dyn Widget>>,

    // Internal state.
    root_child_id: Option<WidgetId>,
}

impl std::fmt::Debug for ColorEdit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColorEdit")
            .field("alpha_enabled", &self.alpha_enabled)
            .field("picker_layout", &self.picker_layout)
            .field("enabled", &self.enabled.get())
            .finish_non_exhaustive()
    }
}

impl ColorEdit {
    /// Bind to a non-nullable color signal. The trigger and the picker
    /// both read from and write to the same signal.
    pub fn new(value: Signal<Color>) -> Self {
        Self::from_binding(ColorBinding::Required(value))
    }

    /// Bind to a nullable color signal. `None` is treated as transparent
    /// black for picker math; any user interaction produces a concrete
    /// `Some(color)`. To clear back to `None`, compose a separate
    /// Clear button alongside the `ColorEdit`.
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
            enabled: Prop::Static(true),
            on_open: None,
            on_close: None,
            tooltip_text: None,
            rich_tooltip_source: None,
            composite_tooltip_content: None,
            root_child_id: None,
        }
    }

    /// Enable or disable the alpha channel in the picker and the hex trigger label.
    pub fn alpha_enabled(mut self, enabled: bool) -> Self {
        self.alpha_enabled = enabled;
        self
    }

    /// Provide a palette of preset swatches shown in the popover —
    /// statically, or reactively via a bound `Signal<Vec<Color>>` so the
    /// palette updates without reopening the popover.
    pub fn swatches(mut self, s: impl Into<Prop<Vec<Color>>>) -> Self {
        self.swatches = Some(s.into());
        self
    }

    /// Number of columns in the preset swatch grid. Defaults to 6;
    /// clamped to at least 1.
    pub fn swatch_columns(mut self, n: usize) -> Self {
        self.swatch_columns = n.max(1);
        self
    }

    /// Select a popover layout variant — [`ColorPickerLayout::Compact`]
    /// (default, minimal height) or `Standard` / `Wide` for richer controls.
    pub fn picker_layout(mut self, l: ColorPickerLayout) -> Self {
        self.picker_layout = l;
        self
    }

    /// Show or hide the RGB (0–255) component spinners in the popover.
    pub fn show_rgb_spinners(mut self, s: bool) -> Self {
        self.show_rgb_spinners = s;
        self
    }

    /// Show or hide the HSV (hue/saturation/value) component spinners in the popover.
    pub fn show_hsv_spinners(mut self, s: bool) -> Self {
        self.show_hsv_spinners = s;
        self
    }

    /// Show or hide the hex string input in the popover.
    pub fn show_hex_input(mut self, s: bool) -> Self {
        self.show_hex_input = s;
        self
    }

    /// Show or hide the formatted hex value as the trigger button label.
    pub fn show_hex_in_trigger(mut self, s: bool) -> Self {
        self.show_hex_in_trigger = s;
        self
    }

    /// Show or hide the trailing chevron glyph on the trigger button.
    pub fn show_chevron(mut self, s: bool) -> Self {
        self.show_chevron = s;
        self
    }

    /// Override the size of the color swatch thumbnail in the trigger button (logical pixels).
    pub fn trigger_swatch_size(mut self, size: f32) -> Self {
        self.trigger_swatch_size = Some(size.max(0.0));
        self
    }

    /// Override where the popover appears relative to the trigger.
    /// Default is [`OverlayPlacement::BelowPreferred`].
    pub fn placement(mut self, p: OverlayPlacement) -> Self {
        self.placement = p;
        self
    }

    /// Override how the popover is dismissed. Default is
    /// `DismissBehavior::EscapeOrClickOutside`.
    pub fn dismiss_behavior(mut self, b: DismissBehavior) -> Self {
        self.dismiss_behavior = b;
        self
    }

    /// Replace the trigger button's visible label with a static localized
    /// string. When set, the hex value is no longer displayed in the trigger
    /// (combine with `.show_hex_in_trigger(false)` if needed).
    pub fn label(mut self, label: impl Into<LocalizedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the enabled state, statically or reactively. Forwarded to the
    /// arena at build time.
    pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self {
        self.enabled = enabled.into();
        self
    }

    /// Install a callback fired when the color-picker popover opens.
    ///
    /// The signature is `Fn()` (no [`EventContext`](teksilo_core::widget::EventContext))
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

    /// Attach a plain single-line tooltip shown after a hover delay.
    ///
    /// Mutually exclusive with [`rich_tooltip`](Self::rich_tooltip),
    /// [`rich_tooltip_content`](Self::rich_tooltip_content), and
    /// [`composite_tooltip`](Self::composite_tooltip) — calling this
    /// clears the other slots (last setter wins).
    pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self {
        self.tooltip_text = Some(text.into());
        self.rich_tooltip_source = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip identified by a registry key.
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip),
    /// [`rich_tooltip_content`](Self::rich_tooltip_content), and
    /// [`composite_tooltip`](Self::composite_tooltip) — calling this
    /// clears the other slots (last setter wins).
    pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Key(key.into()));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a rich tooltip from an inline [`TooltipContent`](crate::tooltip::TooltipContent) value.
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip),
    /// [`rich_tooltip`](Self::rich_tooltip), and
    /// [`composite_tooltip`](Self::composite_tooltip) — calling this
    /// clears the other slots (last setter wins).
    pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self {
        self.rich_tooltip_source = Some(crate::tooltip::RichTooltipSource::Content(content));
        self.tooltip_text = None;
        self.composite_tooltip_content = None;
        self
    }

    /// Attach a composite tooltip whose body is an arbitrary widget tree.
    ///
    /// Mutually exclusive with [`tooltip`](Self::tooltip),
    /// [`rich_tooltip`](Self::rich_tooltip), and
    /// [`rich_tooltip_content`](Self::rich_tooltip_content) — calling
    /// this clears the other slots (last setter wins).
    pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self {
        self.composite_tooltip_content = Some(Box::new(content));
        self.tooltip_text = None;
        self.rich_tooltip_source = None;
        self
    }
}

impl Widget for ColorEdit {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let self_id = ctx.self_id();
        // Forward the enabled state into the arena; see IconButton.
        ctx.enabled_when(self_id, self.enabled.clone());

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
            });
        if let Some(s) = self.swatches.clone() {
            picker = picker.swatches(s);
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
                // Zip the locale signal so the "no color" placeholder
                // re-resolves on a live locale switch — `source.map` alone
                // only re-fires when the color changes.
                source
                    .zip(&ctx.locale_signal())
                    .map(move |(opt, _)| match opt {
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
        // AT name reactively via Button's `label` plumbing.
        let trigger = if let Some(ls) = self.label.take() {
            Button::new(ls)
        } else {
            Button::new(lit!("")).label(label_signal)
        };
        let mut trigger = trigger.leading(swatch);
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

        // Tooltip attachment — anchored on the trigger (pb_id), not the popover content.
        if let Some(content) = self.composite_tooltip_content.take() {
            let delay = ctx.theme().motion.tooltip_delay_heavy;
            crate::tooltip::attach_composite_tooltip_boxed(ctx, pb_id, content, delay);
        } else if let Some(source) = self.rich_tooltip_source.clone() {
            let delay = ctx.theme().motion.tooltip_delay;
            crate::tooltip::attach_rich_tooltip_source(ctx, pb_id, source, delay);
        } else if let Some(text) = self.tooltip_text.clone() {
            let tooltip_widget = crate::tooltip::TooltipWidget::new(text);
            let tooltip_id = ctx.add(tooltip_widget);
            let delay = ctx.theme().motion.tooltip_delay;
            ctx.attach_tooltip(pb_id, tooltip_id, delay);
        }

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
