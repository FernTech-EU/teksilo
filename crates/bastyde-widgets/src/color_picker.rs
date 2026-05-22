//! `ColorPicker` — embeddable composite color selector.
//!
//! Combines a 2D HSV canvas, 1D hue and alpha strips, RGB and HSV
//! component spinners, a hex input, a current-color preview, and an
//! optional preset swatch grid into a single bound widget. Driven by a
//! `Signal<Color>` (or `Signal<Option<Color>>`) source of truth — every
//! subcomponent reads from / writes to the same signal so the various
//! representations stay in lockstep.
//!
//! # Layouts
//!
//! - [`ColorPickerLayout::Compact`] — HSV canvas + hue strip + hex
//!   input. Minimal vertical footprint, suitable for popovers.
//! - [`ColorPickerLayout::Standard`] (default) — HSV canvas + hue
//!   strip + alpha strip (when enabled), with RGB spinners, hex
//!   input, and preset swatches stacked beneath. The everything-on
//!   layout for inspector panes and settings dialogs.
//! - [`ColorPickerLayout::Wide`] — HSV canvas with strips on the
//!   right, spinners stacked vertically alongside the swatch grid.
//!   For wide property pages.
//!
//! # Accessibility
//!
//! Root: [`Role::Group`](accesskit::Role::Group) with a localized
//! label and `Live::Polite` so screen readers announce committed color
//! changes. The HSV canvas's subtree is excluded from the AT tree
//! (no ARIA precedent for 2D pointer gestures); the hue strip, alpha
//! strip, RGB / HSV spinners, hex input, current-color preview, and
//! swatch grid each carry their own appropriate role and value.

pub mod alpha_strip;
pub mod hsv_canvas;
pub mod hue_strip;
pub mod state;
pub mod swatch;
pub mod swatch_grid;

#[cfg(test)]
mod tests;

use bastyde_i18n::lit;
use bastyde_i18n::localized;
use std::cell::RefCell;
use std::rc::Rc;

use bastyde_canvas::{Rect, SizeProposal};
use bastyde_core::accessibility::AccessNodeBuilder;
use bastyde_core::accesskit::{Action, Live, Role};
use bastyde_core::build_context::BuildContext;
use bastyde_core::signal::Signal;
use bastyde_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget, WidgetPlacement};
use bastyde_core::widget_id::WidgetId;
use bastyde_i18n::{LocalizedString, resolve_message_widget};
use bastyde_tokens::{Color, Orientation};

use self::alpha_strip::AlphaStrip;
use self::hsv_canvas::HsvCanvas;
use self::hue_strip::HueStrip;
use self::state::ColorComponents;
use self::swatch_grid::SwatchGrid;
use crate::button::{Button, ButtonVariant};
use crate::hex_color_input::HexColorInput;
use crate::primitives::{HStack, Spacer, TextWidget};
use crate::spin_box::SpinBox;

pub use self::swatch::ColorSwatch;

/// Default 12-color preset palette (Int UI–flavored). Apps can use
/// this verbatim or pass their own via [`ColorPicker::swatches`].
pub const DEFAULT_SWATCHES: [Color; 12] = [
    Color::new(0.91, 0.30, 0.24, 1.0), // red
    Color::new(0.95, 0.60, 0.20, 1.0), // orange
    Color::new(0.96, 0.83, 0.27, 1.0), // yellow
    Color::new(0.42, 0.70, 0.35, 1.0), // green
    Color::new(0.20, 0.66, 0.61, 1.0), // teal
    Color::new(0.21, 0.52, 0.89, 1.0), // blue
    Color::new(0.36, 0.36, 0.83, 1.0), // indigo
    Color::new(0.66, 0.40, 0.85, 1.0), // purple
    Color::new(0.92, 0.45, 0.68, 1.0), // pink
    Color::new(0.55, 0.36, 0.20, 1.0), // brown
    Color::new(0.06, 0.06, 0.06, 1.0), // near-black
    Color::new(0.96, 0.96, 0.96, 1.0), // near-white
];

pub use bastyde_core::styles::ColorPickerLayout;

/// Internal binding to either a non-nullable `Signal<Color>` or a
/// nullable `Signal<Option<Color>>`. The picker always operates on a
/// concrete `Color` internally — the nullable case treats `None` as
/// "transparent black" for picker math, then writes back `Some(color)`
/// on every commit.
#[derive(Clone)]
enum ColorBinding {
    Required(Signal<Color>),
    Nullable {
        source: Signal<Option<Color>>,
        proxy: Signal<Color>,
    },
}

impl ColorBinding {
    fn value(&self) -> Signal<Color> {
        match self {
            Self::Required(s) => s.clone(),
            Self::Nullable { proxy, .. } => proxy.clone(),
        }
    }
}

/// Embeddable HSV+RGB+hex+alpha+swatches color picker.
pub struct ColorPicker {
    binding: ColorBinding,
    alpha_enabled: bool,
    show_hsv_canvas: bool,
    show_hue_strip: bool,
    show_alpha_strip: Option<bool>,
    show_rgb_spinners: bool,
    show_hsv_spinners: bool,
    show_hex_input: bool,
    show_preview: bool,
    show_swatches: bool,
    show_footer: bool,
    on_done: Option<Rc<dyn Fn(&mut EventContext)>>,
    on_cancel: Option<Rc<dyn Fn(&mut EventContext)>>,
    swatches: Vec<Color>,
    swatches_signal: Option<Signal<Vec<Color>>>,
    swatch_columns: usize,
    layout: ColorPickerLayout,
    label: Option<LocalizedString>,
    /// Initial enabled-state; forwarded to the arena at build time.
    initial_enabled: bool,
    /// Cache of the most recent color formatted as a hex string. The
    /// live-region effect updates this whenever the bound color changes;
    /// `accessibility()` reads it (via `binding.value().get()` then
    /// `to_hex_upper`) — keeping the cell here means the effect's
    /// dirty-marking is what triggers re-resolution, instead of every
    /// AT walk allocating a fresh string.
    last_announced_hex: Rc<RefCell<Option<String>>>,
    /// Per-call style override. Higher precedence than the theme-wide
    /// `style_slots.color_picker` slot, which in turn beats the default
    /// `RecipeColorPickerStyle`.
    style_override: Option<bastyde_core::styles::SharedColorPickerStyle>,
    root_child_id: Option<WidgetId>,
}

impl ColorPicker {
    /// Bind to a non-nullable color signal.
    pub fn new(value: Signal<Color>) -> Self {
        Self::from_binding(ColorBinding::Required(value))
    }

    /// Bind to a nullable color signal. `None` is treated as
    /// transparent black for picker math; any commit produces a
    /// concrete `Some(color)`. Apps that want a "clear to None"
    /// affordance should expose a separate Clear button alongside
    /// the picker.
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
            show_hsv_canvas: true,
            show_hue_strip: true,
            show_alpha_strip: None, // defaults to alpha_enabled
            show_rgb_spinners: true,
            show_hsv_spinners: false,
            show_hex_input: true,
            show_preview: true,
            show_swatches: true,
            show_footer: false,
            on_done: None,
            on_cancel: None,
            swatches: DEFAULT_SWATCHES.to_vec(),
            swatches_signal: None,
            swatch_columns: 6,
            layout: ColorPickerLayout::Standard,
            label: None,
            initial_enabled: true,
            last_announced_hex: Rc::new(RefCell::new(None)),
            style_override: None,
            root_child_id: None,
        }
    }

    /// Per-call style override. Higher precedence than the theme-wide
    /// `style_slots.color_picker` slot.
    pub fn style(mut self, style: impl bastyde_core::styles::ColorPickerStyle) -> Self {
        self.style_override = Some(Rc::new(style));
        self
    }

    pub fn alpha_enabled(mut self, e: bool) -> Self {
        self.alpha_enabled = e;
        self
    }

    pub fn show_hsv_canvas(mut self, s: bool) -> Self {
        self.show_hsv_canvas = s;
        self
    }

    pub fn show_hue_strip(mut self, s: bool) -> Self {
        self.show_hue_strip = s;
        self
    }

    pub fn show_alpha_strip(mut self, s: bool) -> Self {
        self.show_alpha_strip = Some(s);
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

    pub fn show_preview(mut self, s: bool) -> Self {
        self.show_preview = s;
        self
    }

    pub fn show_swatches(mut self, s: bool) -> Self {
        self.show_swatches = s;
        self
    }

    /// Show a Done / Cancel footer at the bottom of the picker.
    /// Default `false` for embedded use (the bound signal is the
    /// commit channel — there is no "uncommitted" state). Wrappers
    /// that present the picker as a popover (e.g. [`ColorEdit`])
    /// flip this to `true` so the user has explicit accept / dismiss
    /// affordances; the buttons fire [`Self::on_done`] /
    /// [`Self::on_cancel`] respectively.
    pub fn show_footer(mut self, s: bool) -> Self {
        self.show_footer = s;
        self
    }

    /// Callback fired when the user activates the footer's Done
    /// button. The picker has already been writing through to the
    /// bound signal as the user dragged / typed, so Done's job is
    /// purely to dismiss the surrounding surface (popover, sheet,
    /// dialog). Only meaningful when `show_footer(true)`.
    pub fn on_done(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_done = Some(Rc::new(f));
        self
    }

    /// Callback fired when the user activates the footer's Cancel
    /// button. The picker itself does **not** restore any value —
    /// that's the caller's responsibility (e.g. ColorEdit captures a
    /// snapshot at popover-open time and writes it back here). The
    /// callback's typical implementation is
    /// `value.set(snapshot.get()); ctx.dismiss_self_overlay_chain();`.
    /// Only meaningful when `show_footer(true)`.
    pub fn on_cancel(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self {
        self.on_cancel = Some(Rc::new(f));
        self
    }

    pub fn swatches(mut self, s: Vec<Color>) -> Self {
        self.swatches = s;
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

    pub fn layout(mut self, l: ColorPickerLayout) -> Self {
        self.layout = l;
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

    /// Read the current bound color. Convenience for tests / apps that
    /// hold a `ColorPicker` reference; otherwise prefer reading the
    /// `Signal<Color>` you passed in.
    pub fn current(&self) -> Color {
        self.binding.value().get()
    }
}

impl std::fmt::Debug for ColorPicker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ColorPicker")
            .field("alpha_enabled", &self.alpha_enabled)
            .field("layout", &self.layout)
            .field("initial_enabled", &self.initial_enabled)
            .finish_non_exhaustive()
    }
}

impl Widget for ColorPicker {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // ── Bridge nullable binding ↔ proxy ──
        if let ColorBinding::Nullable { source, proxy } = &self.binding {
            // source → proxy (external writes update internal proxy)
            {
                let proxy = proxy.clone();
                ctx.effect(source, move |new| {
                    let resolved = new.unwrap_or(Color::TRANSPARENT);
                    if proxy.get() != resolved {
                        proxy.set(resolved);
                    }
                });
            }
            // proxy → source (internal commits update external source)
            {
                let source = source.clone();
                ctx.effect(proxy, move |c| {
                    if source.get() != Some(*c) {
                        source.set(Some(*c));
                    }
                });
            }
        }

        let value = self.binding.value();
        let components = Rc::new(ColorComponents::new(ctx, value.clone()));

        // ── Live-region announcement on commit ──
        // Only fires when dragging is false (avoids per-frame chatter
        // mid-drag). The last-announced cache prevents repeating
        // identical announcements when other channels change.
        // Live-region hex cache — refreshed when the bound color settles
        // (i.e. not mid-drag). `accessibility()` reads the cached string
        // when present and falls back to a fresh format otherwise.
        {
            let dragging = components.dragging.clone();
            let last_announced = self.last_announced_hex.clone();
            let alpha = self.alpha_enabled;
            ctx.effect(&value, move |c| {
                if dragging.get() {
                    return;
                }
                let hex = c.to_hex_upper(alpha);
                let needs_update = last_announced.borrow().as_deref() != Some(hex.as_str());
                if needs_update {
                    *last_announced.borrow_mut() = Some(hex);
                }
            });
        }

        let self_id = ctx.self_id();
        // Forward initial-enabled into the arena; see IconButton. Inner
        // sub-widgets (HsvCanvas, HueStrip, AlphaStrip, SwatchGrid)
        // inherit disabled via the ancestor walk — their per-widget
        // `initial_enabled` is only consulted at build time and then
        // they too forward into the arena, so the AND semantics fall
        // out for free.
        if !self.initial_enabled {
            ctx.enabled_when(self_id, false);
        }

        // ── Resolve flags ──
        let alpha_enabled = self.alpha_enabled;
        let show_alpha_strip = self.show_alpha_strip.unwrap_or(alpha_enabled);
        let layout = self.layout;
        let enabled = self.initial_enabled;
        use crate::styles::recipe_color_picker_style as cp;

        // ── Build subcomponents ──

        // Top row: HSV canvas + hue strip + alpha strip
        let mut top_row = HStack::new().spacing(cp::GAP);
        if self.show_hsv_canvas {
            let canvas = HsvCanvas::new(
                components.hue.clone(),
                components.saturation.clone(),
                components.value_hsv.clone(),
                components.set_hsv.clone(),
                components.dragging.clone(),
            )
            .enabled(enabled);
            // The HSV canvas is a 2D pointer surface with no ARIA
            // precedent — exclude its subtree from the AT tree.
            use bastyde_core::widget_builder::WidgetBuilder;
            top_row = top_row.child(canvas.access_exclude_subtree());
        }
        if self.show_hue_strip {
            let hue = HueStrip::new(
                components.hue.clone(),
                components.set_hue.clone(),
                components.dragging.clone(),
            )
            .orientation(Orientation::Vertical)
            .enabled(enabled)
            .label(resolve_message_widget("color-picker-hue-label", &[]));
            top_row = top_row.child(hue);
        }
        if alpha_enabled && show_alpha_strip {
            let alpha = AlphaStrip::new(
                value.clone(),
                components.alpha.clone(),
                components.set_alpha.clone(),
                components.dragging.clone(),
            )
            .orientation(Orientation::Vertical)
            .enabled(enabled)
            .label(resolve_message_widget("color-picker-alpha-label", &[]));
            top_row = top_row.child(alpha);
        }
        let top_row_id = ctx.add(top_row);

        // Preview + hex row — Standard / Wide only. Empty row in
        // Compact (Compact uses the compact-hex slot instead).
        let preview_row_id: Option<WidgetId> =
            if layout != ColorPickerLayout::Compact && (self.show_preview || self.show_hex_input) {
                let mut row = HStack::new().spacing(cp::GAP);
                if self.show_preview {
                    row = row.child(
                        ColorSwatch::new(value.clone())
                            .size(cp::PREVIEW_HEIGHT)
                            .corner_radius(cp::PREVIEW_CORNER_RADIUS)
                            .label(localized(move || {
                                resolve_message_widget("color-picker-current-color-label", &[])
                            })),
                    );
                }
                if self.show_hex_input {
                    let hex = HexColorInput::new(value.clone())
                        .alpha_enabled(alpha_enabled)
                        .label(localized(move || {
                            resolve_message_widget("color-picker-hex-label", &[])
                        }))
                        .width(cp::HEX_FIELD_WIDTH);
                    row = row.child(hex);
                }
                Some(ctx.add(row))
            } else {
                None
            };

        // RGB spinners row — Standard / Wide only (the Compact layout
        // doesn't include them, so creating one in Compact would leak
        // an orphan root in the arena and absorb hit-tests at the
        // pre-layout fallback bounds). Built eagerly so closures don't
        // fight over &mut ctx. Bridges observe the mutable `value`
        // signal (not the derived `components.red` etc., which are
        // ReadOnly and don't support `ctx.effect`).
        let rgb_row_id: Option<WidgetId> =
            if layout != ColorPickerLayout::Compact && self.show_rgb_spinners {
                let r_spin = make_byte_spinner_from_value(
                    ctx,
                    value.clone(),
                    |c| c.r(),
                    components.set_red.clone(),
                    enabled,
                    cp::SPINNER_FIELD_WIDTH,
                );
                let g_spin = make_byte_spinner_from_value(
                    ctx,
                    value.clone(),
                    |c| c.g(),
                    components.set_green.clone(),
                    enabled,
                    cp::SPINNER_FIELD_WIDTH,
                );
                let b_spin = make_byte_spinner_from_value(
                    ctx,
                    value.clone(),
                    |c| c.b(),
                    components.set_blue.clone(),
                    enabled,
                    cp::SPINNER_FIELD_WIDTH,
                );
                let mut row = HStack::new()
                    .spacing(cp::GAP)
                    .child(spinner_cell("color-picker-red-short", r_spin))
                    .child(spinner_cell("color-picker-green-short", g_spin))
                    .child(spinner_cell("color-picker-blue-short", b_spin));
                if alpha_enabled {
                    let a_spin = make_byte_spinner_from_value(
                        ctx,
                        value.clone(),
                        |c| c.a(),
                        components.set_alpha.clone(),
                        enabled,
                        cp::SPINNER_FIELD_WIDTH,
                    );
                    row = row.child(spinner_cell("color-picker-alpha-short", a_spin));
                }
                Some(ctx.add(row))
            } else {
                None
            };

        // HSV spinners row — same pattern. Standard / Wide only.
        let hsv_row_id: Option<WidgetId> =
            if layout != ColorPickerLayout::Compact && self.show_hsv_spinners {
                let h_spin = make_hue_spinner_from_value(
                    ctx,
                    value.clone(),
                    components.set_hue.clone(),
                    enabled,
                    cp::SPINNER_FIELD_WIDTH,
                );
                let s_spin = make_percent_spinner_from_value(
                    ctx,
                    value.clone(),
                    |c| c.to_hsv().1,
                    components.set_saturation.clone(),
                    enabled,
                    cp::SPINNER_FIELD_WIDTH,
                );
                let v_spin = make_percent_spinner_from_value(
                    ctx,
                    value.clone(),
                    |c| c.to_hsv().2,
                    components.set_value_hsv.clone(),
                    enabled,
                    cp::SPINNER_FIELD_WIDTH,
                );
                Some(
                    ctx.add(
                        HStack::new()
                            .spacing(cp::GAP)
                            .child(spinner_cell("color-picker-hue-short", h_spin))
                            .child(spinner_cell("color-picker-saturation-short", s_spin))
                            .child(spinner_cell("color-picker-value-short", v_spin)),
                    ),
                )
            } else {
                None
            };

        // Compact-layout hex row.
        let compact_hex_id: Option<WidgetId> =
            if layout == ColorPickerLayout::Compact && self.show_hex_input {
                Some(
                    ctx.add(
                        HexColorInput::new(value.clone())
                            .alpha_enabled(alpha_enabled)
                            .label(localized(move || {
                                resolve_message_widget("color-picker-hex-label", &[])
                            }))
                            .width(cp::HEX_FIELD_WIDTH),
                    ),
                )
            } else {
                None
            };

        // Swatch grid — Standard / Wide only (Compact doesn't surface
        // a swatch grid; building one anyway would orphan it in the
        // arena and absorb hit-tests inside the trigger).
        let swatches_id: Option<WidgetId> = if layout != ColorPickerLayout::Compact
            && (self.show_swatches && !self.swatches.is_empty() || self.swatches_signal.is_some())
        {
            let swatches_signal = self
                .swatches_signal
                .clone()
                .unwrap_or_else(|| Signal::new(self.swatches.clone()));
            let on_select: Rc<dyn Fn(Color, &mut EventContext)> = {
                let value = value.clone();
                Rc::new(move |c, _ctx_evt| {
                    value.set(c);
                })
            };
            Some(ctx.add(SwatchGrid::new(
                swatches_signal,
                value.clone(),
                self.swatch_columns,
                on_select,
            )))
        } else {
            None
        };

        // Footer row (Cancel + Spacer + Done) — only when show_footer
        // is set. Built once per layout. The buttons fire user-supplied
        // callbacks; the picker doesn't dismiss anything itself (it
        // doesn't own the surrounding surface).
        let footer_id: Option<WidgetId> = if self.show_footer {
            let mut row = HStack::new().spacing(cp::GAP).child(Spacer::new());
            if let Some(cb) = self.on_cancel.clone() {
                let cancel_btn = Button::new(localized(move || {
                    resolve_message_widget("color-picker-cancel-label", &[])
                }))
                .variant(ButtonVariant::Plain)
                .enabled(enabled)
                .on_activate_fn(move |ctx_evt| cb(ctx_evt));
                row = row.child(cancel_btn);
            }
            if let Some(cb) = self.on_done.clone() {
                let done_btn = Button::new(localized(move || {
                    resolve_message_widget("color-picker-done-label", &[])
                }))
                .variant(ButtonVariant::Filled)
                .enabled(enabled)
                .on_activate_fn(move |ctx_evt| cb(ctx_evt));
                row = row.child(done_btn);
            }
            Some(ctx.add(row))
        } else {
            None
        };

        // ── Delegate body assembly + surface wrap to the active style.
        let style = resolve_color_picker_style(&self.style_override, ctx);
        let cfg = bastyde_core::styles::ColorPickerStyleConfig {
            layout,
            top_row: top_row_id,
            preview_row: preview_row_id,
            rgb_row: rgb_row_id,
            hsv_row: hsv_row_id,
            swatches: swatches_id,
            footer: footer_id,
            compact_hex: compact_hex_id,
        };
        let root_id = style.make_body(&cfg, ctx);
        self.root_child_id = Some(root_id);

        // Bind the value signal so the wrapper's accessibility() re-runs
        // whenever the color changes (Live::Polite + set_value churns).
        let self_id = ctx.self_id();
        let registry = ctx.binding_registry();
        value.bind_to(
            self_id,
            registry,
            bastyde_core::binding::BindingLevel::AccessibilityOnly,
        );

        vec![root_id]
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

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        builder.set_role(Role::Group);
        let name = self
            .label
            .as_ref()
            .map(|ls| ls.resolve_now())
            .unwrap_or_else(|| resolve_message_widget("color-picker-name", &[]));
        builder.set_name(name);
        builder.set_live(Live::Polite);
        let hex = self
            .last_announced_hex
            .borrow()
            .clone()
            .unwrap_or_else(|| self.binding.value().get().to_hex_upper(self.alpha_enabled));
        builder.set_value(resolve_message_widget(
            "color-picker-changed-announcement",
            &[("hex", hex.into())],
        ));
        // Framework a11y walker sets `set_disabled` from arena state.
        builder.add_action(Action::Focus);
    }
}

fn resolve_color_picker_style(
    override_: &Option<bastyde_core::styles::SharedColorPickerStyle>,
    ctx: &BuildContext,
) -> bastyde_core::styles::SharedColorPickerStyle {
    if let Some(s) = override_.clone() {
        return s;
    }
    ctx.theme_signal()
        .get()
        .style_slots
        .color_picker
        .clone()
        .unwrap_or_else(|| {
            Rc::new(crate::styles::recipe_color_picker_style::RecipeColorPickerStyle)
                as bastyde_core::styles::SharedColorPickerStyle
        })
}

// ── Helpers ───────────────────────────────────────────────────────────

/// Bridge a mutable `Signal<Color>` channel → `SpinBox<u8>` (0..255).
/// Observes the mutable source signal so `ctx.effect` works (derived
/// signals are ReadOnly).
fn make_byte_spinner_from_value(
    ctx: &mut BuildContext,
    value: Signal<Color>,
    accessor: fn(Color) -> f32,
    setter: Rc<dyn Fn(f32)>,
    enabled: bool,
    width: f32,
) -> SpinBox<u8> {
    let initial = (accessor(value.get()) * 255.0).round().clamp(0.0, 255.0) as u8;
    let bridge = ctx.signal(initial);
    // value → bridge
    {
        let bridge = bridge.clone();
        ctx.effect(&value, move |c| {
            let new_u = (accessor(*c) * 255.0).round().clamp(0.0, 255.0) as u8;
            if bridge.get() != new_u {
                bridge.set(new_u);
            }
        });
    }
    // bridge → setter — guard against re-entrance from the value→bridge
    // effect by no-oping when the current value's projection already
    // equals the bridge value (i.e. this bridge change came from a
    // value-driven update, not user input on the SpinBox).
    {
        let setter = setter.clone();
        let value = value.clone();
        ctx.effect(&bridge, move |new_u| {
            let current_u = (accessor(value.get()) * 255.0).round().clamp(0.0, 255.0) as u8;
            if *new_u == current_u {
                return;
            }
            (setter)((*new_u) as f32 / 255.0);
        });
    }
    SpinBox::new(bridge, 0u8, 255u8)
        .single_step(1u8)
        .page_step(16u8)
        .enabled(enabled)
        .width(width)
}

/// Bridge `Signal<Color>` (HSV hue) → `SpinBox<u32>` (0..359).
fn make_hue_spinner_from_value(
    ctx: &mut BuildContext,
    value: Signal<Color>,
    setter: Rc<dyn Fn(f32)>,
    enabled: bool,
    width: f32,
) -> SpinBox<u32> {
    let initial = value.get().to_hsv().0.round().clamp(0.0, 359.0) as u32;
    let bridge = ctx.signal(initial);
    {
        let bridge = bridge.clone();
        ctx.effect(&value, move |c| {
            let new_u = c.to_hsv().0.round().clamp(0.0, 359.0) as u32;
            if bridge.get() != new_u {
                bridge.set(new_u);
            }
        });
    }
    {
        let setter = setter.clone();
        let value = value.clone();
        ctx.effect(&bridge, move |new_u| {
            let current_u = value.get().to_hsv().0.round().clamp(0.0, 359.0) as u32;
            if *new_u == current_u {
                return;
            }
            (setter)(*new_u as f32);
        });
    }
    SpinBox::new(bridge, 0u32, 359u32)
        .single_step(1u32)
        .page_step(15u32)
        .enabled(enabled)
        .width(width)
}

/// Bridge `Signal<Color>` (HSV channel) → `SpinBox<u8>` displayed as 0..100 percent.
fn make_percent_spinner_from_value(
    ctx: &mut BuildContext,
    value: Signal<Color>,
    accessor: fn(Color) -> f32,
    setter: Rc<dyn Fn(f32)>,
    enabled: bool,
    width: f32,
) -> SpinBox<u8> {
    let initial = (accessor(value.get()) * 100.0).round().clamp(0.0, 100.0) as u8;
    let bridge = ctx.signal(initial);
    {
        let bridge = bridge.clone();
        ctx.effect(&value, move |c| {
            let new_u = (accessor(*c) * 100.0).round().clamp(0.0, 100.0) as u8;
            if bridge.get() != new_u {
                bridge.set(new_u);
            }
        });
    }
    {
        let setter = setter.clone();
        let value = value.clone();
        ctx.effect(&bridge, move |new_u| {
            let current_u = (accessor(value.get()) * 100.0).round().clamp(0.0, 100.0) as u8;
            if *new_u == current_u {
                return;
            }
            (setter)((*new_u) as f32 / 100.0);
        });
    }
    SpinBox::new(bridge, 0u8, 100u8)
        .single_step(1u8)
        .page_step(10u8)
        .suffix(" %")
        .enabled(enabled)
        .width(width)
}

/// Wrap a spinner with a small leading label cell ("R", "G", "B", …).
fn spinner_cell(label_key: &str, spinner: impl Widget + 'static) -> HStack {
    let label = resolve_message_widget(label_key, &[]);
    HStack::new()
        .spacing(4.0)
        .child(TextWidget::new(lit!(label)))
        .child(spinner)
}
