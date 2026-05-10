//! SpinBox demo — exercises every `SpinBox` feature in one window.
//!
//! Run with: `cargo run -p spin-box`.
//!
//! What's on screen:
//!
//! - An integer SpinBox for a font size in `pt` (clamp mode).
//! - A float SpinBox for gain in `dB` with 1-decimal display and a
//!   custom `value_from_text` that accepts Unicode minus (`−`) in
//!   addition to ASCII `-`.
//! - A percentage SpinBox with suffix, wrap mode enabled, and a
//!   custom single/page step combo.
//! - An "Auto" SpinBox whose minimum shows
//!   [`special_value_text`](fern_widgets::SpinBox::special_value_text).
//! - A scientific-notation adaptive-step SpinBox over six orders of
//!   magnitude.
//! - A read-only SpinBox that mirrors one of the other values.
//!
//! A `Reset all` button below returns every value to a sensible
//! default so the demo can be re-played without restarting.

use fern_ui::core::WidgetPlacement;
use fern_ui::prelude::*;
use fern_ui::widgets::{
    Button, ButtonVariant, Expand, HStack, Padding, Panel, Spacer, SpinBox, StepType, TextWidget,
    Toolbar, VStack, WheelMode, WrapMode,
};

fn dark_mode_toolbar() -> impl Widget {
    let is_dark = Signal::new(false);
    fern!(
        Toolbar {
            HStack {
                Spacer
                Button::new_literal("Toggle Dark Mode") {
                    on_activate_fn: move |ctx| {
                        let next = !is_dark.get();
                        is_dark.set(next);
                        ctx.set_theme(if next {
                            fern_ui::presets::intui::dark()
                        } else {
                            fern_ui::presets::intui::light()
                        });
                    }
                }
            }
        }
    )
}

#[derive(Debug)]
struct Values {
    font_size: Signal<i32>,
    gain_db: Signal<f64>,
    opacity: Signal<i32>,
    timeout: Signal<i32>,
    frequency: Signal<f64>,
    mirror: Signal<i32>,
}

impl Values {
    fn new(font_size: Signal<i32>) -> Self {
        Self {
            font_size: font_size.clone(),
            gain_db: Signal::new(0.0),
            opacity: Signal::new(50),
            timeout: Signal::new(0),
            frequency: Signal::new(440.0),
            mirror: font_size,
        }
    }
}

#[derive(Debug)]
struct Root {
    values: Values,
    root_child_id: Option<WidgetId>,
}

impl Root {
    fn new() -> Self {
        let font_size = Signal::new(12);
        Self {
            values: Values::new(font_size),
            root_child_id: None,
        }
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let _theme = ctx.theme_signal().get();

        // Live readouts so the demo visibly confirms each signal.
        let font_size_text = self.values.font_size.map(|v| format!("{} pt", v));
        let gain_text = self.values.gain_db.map(|v| format!("{:.1} dB", v));
        let opacity_text = self.values.opacity.map(|v| format!("{} %", v));
        let timeout_text = self.values.timeout.map(|v| {
            if *v == 0 {
                "Auto".to_string()
            } else {
                format!("{} s", v)
            }
        });
        let frequency_text = self.values.frequency.map(|v| format!("{:.2} Hz", v));

        let reset_font = self.values.font_size.clone();
        let reset_gain = self.values.gain_db.clone();
        let reset_opacity = self.values.opacity.clone();
        let reset_timeout = self.values.timeout.clone();
        let reset_frequency = self.values.frequency.clone();

        let root = fern!(ctx => Padding::uniform(24.0) {
                Panel {
                    Padding::uniform(20.0) {
                        VStack {
                            spacing: 14.0
                            // Heading.
                            TextWidget::new_literal("SpinBox gallery") {
                                style: TextStyleRole::BodyBold
                                color: TextRole::Primary
                            }
                            TextWidget::new_literal("Every SpinBox feature on one page. \
                                            Use arrow keys, Page↑/Page↓, mouse wheel, \
                                            or the ± buttons; press Enter or Tab to commit typed input.") {
                                style: TextStyleRole::Body
                                color: TextRole::Secondary
                            }
                            // Row 1 — font size (integer, clamp, narrow width).
                            child: row(
                                "Font size (narrow 80 dp)",
                                SpinBox::new(self.values.font_size.clone(), 4, 96)
                                    .single_step(1)
                                    .page_step(10)
                                    .suffix(" pt")
                                    .width(80.0)
                                    .label("Font size"),
                                font_size_text,
                            )
                            // Row 2 — gain dB (float, 1 decimal).
                            child: row(
                                "Gain",
                                SpinBox::new(self.values.gain_db.clone(), -60.0, 12.0)
                                    .single_step(0.5)
                                    .page_step(6.0)
                                    .decimals(1)
                                    .suffix(" dB")
                                    .value_from_text(|s| {
                                        // Accept Unicode minus `−` (U+2212) as well as ASCII.
                                        s.replace('\u{2212}', "-").trim().parse::<f64>().ok()
                                    })
                                    .label("Gain"),
                                gain_text,
                            )
                            // Row 3 — opacity (integer, wrap mode for fun).
                            child: row(
                                "Opacity",
                                SpinBox::new(self.values.opacity.clone(), 0, 100)
                                    .single_step(5)
                                    .page_step(25)
                                    .suffix(" %")
                                    .wrap_mode(WrapMode::Wrap)
                                    .label("Opacity"),
                                opacity_text,
                            )
                            // Row 4 — timeout with special value "Auto".
                            child: row(
                                "Timeout",
                                SpinBox::new(self.values.timeout.clone(), 0, 3600)
                                    .single_step(1)
                                    .page_step(60)
                                    .suffix(" s")
                                    .special_value_text("Auto")
                                    .label("Timeout"),
                                timeout_text,
                            )
                            // Row 5 — frequency (adaptive step, wider width).
                            child: row(
                                "Frequency (wider 180 dp)",
                                SpinBox::new(self.values.frequency.clone(), 0.1, 20_000.0)
                                    .single_step(1.0)
                                    .decimals(2)
                                    .suffix(" Hz")
                                    .step_type(StepType::Adaptive)
                                    .wheel_mode(WheelMode::Hover)
                                    .width(180.0)
                                    .label("Frequency"),
                                frequency_text,
                            )
                            // Row 6 — Int UI-style dense field: buttons hidden.
                            child: row(
                                "Font size (no buttons, Int UI)",
                                SpinBox::new(self.values.font_size.clone(), 4, 96)
                                    .single_step(1)
                                    .page_step(10)
                                    .suffix(" pt")
                                    .show_buttons(false)
                                    .label("Font size, no buttons"),
                                self.values.font_size.map(|v| format!("{} pt", v)),
                            )
                            // Row 7 — read-only mirror of font_size.
                            child: row(
                                "Font size (mirror, read-only)",
                                SpinBox::new(self.values.mirror.clone(), 4, 96)
                                    .suffix(" pt")
                                    .read_only(true)
                                    .label("Font size mirror"),
                                self.values.mirror.map(|v| format!("{} pt", v)),
                            )
                            // ── Width gallery ──────────────────────
                            //
                            // Four SpinBoxes bound to the same `opacity`
                            // signal so the different widths are easy to
                            // compare side by side. Each row labels its
                            // width policy.
                            TextWidget::new_literal("Width control") {
                                style: TextStyleRole::BodyBold
                                color: TextRole::Primary
                            }
                            child: row(
                                "Narrow — .width(64)",
                                SpinBox::new(self.values.opacity.clone(), 0, 100)
                                    .suffix(" %")
                                    .width(64.0)
                                    .label("Opacity (narrow)"),
                                self.values.opacity.map(|v| format!("{} %", v)),
                            )
                            child: row(
                                "Default — 120 dp cap",
                                SpinBox::new(self.values.opacity.clone(), 0, 100)
                                    .suffix(" %")
                                    .label("Opacity (default)"),
                                self.values.opacity.map(|v| format!("{} %", v)),
                            )
                            child: row(
                                "Wider — .width(220)",
                                SpinBox::new(self.values.opacity.clone(), 0, 100)
                                    .suffix(" %")
                                    .width(220.0)
                                    .label("Opacity (wide)"),
                                self.values.opacity.map(|v| format!("{} %", v)),
                            )
                            child: row(
                                "Chars — .width_chars(3) (fits \"100 %\")",
                                SpinBox::new(self.values.opacity.clone(), 0, 100)
                                    .suffix(" %")
                                    .width_chars(3)
                                    .label("Opacity (3 chars)"),
                                self.values.opacity.map(|v| format!("{} %", v)),
                            )
                            // `.fill_width()` needs a flex parent to
                            // stretch into, so this row wraps the
                            // SpinBox in `Expand::horizontal()`
                            // instead of the normal `row` helper.
                            HStack {
                                spacing: 12.0
                                MinSizeForLabel::new(TextWidget::new_literal(
                                                "Fill — .fill_width()",
                                            )) {
                                    width: 220.0
                                }
                                Expand::horizontal() {
                                    SpinBox::new(self.values.opacity.clone(), 0, 100) {
                                        suffix: " %"
                                        fill_width
                                        label: "Opacity (fill)"
                                    }
                                }
                                TextWidget::new_literal("") {
                                    bind_text: self.values.opacity.map(|v| format!("{} %", v))
                                }
                            }
                            // Reset button.
                            HStack {
                                spacing: 8.0
                                Button::new_literal("Reset all") {
                                    variant: ButtonVariant::Plain
                                    on_activate_fn: move |_ctx| {
                                        reset_font.set(12);
                                        reset_gain.set(0.0);
                                        reset_opacity.set(50);
                                        reset_timeout.set(0);
                                        reset_frequency.set(440.0);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }
}

/// Single demo row: [label | SpinBox | live readout].
fn row(
    label: &str,
    spin: SpinBox<impl fern_ui::widgets::SpinValue>,
    readout: Signal<String>,
) -> impl Widget {
    fern!(
        HStack {
            spacing: 12.0
            MinSizeForLabel::new(TextWidget::new_literal(label)) {
                width: 220.0
            }
            child: spin
            TextWidget::new_literal("") {
                bind_text: readout
            }
        }
    )
}

/// Fixed-width label wrapper so the grid columns line up without a
/// full-blown `Grid`. The example stays single-file this way.
#[derive(Debug)]
struct MinSizeForLabel {
    child: Option<Box<dyn Widget>>,
    width: f32,
    child_id: Option<WidgetId>,
}
impl MinSizeForLabel {
    fn new(child: impl Widget + 'static) -> Self {
        Self {
            child: Some(Box::new(child)),
            width: 160.0,
            child_id: None,
        }
    }
    fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }
}
impl Widget for MinSizeForLabel {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let child = self
            .child
            .take()
            .expect("MinSizeForLabel: child already consumed");
        let id = ctx.add_boxed(child);
        self.child_id = Some(id);
        vec![id]
    }
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        let Some(id) = self.child_id else {
            return (proposal.resolve(self.width, 0.0)).into();
        };
        let mut measured = ctx
            .child_size(id, proposal)
            .unwrap_or_else(|| proposal.resolve(self.width, 0.0));
        if measured.width < self.width {
            measured.width = self.width;
        }
        measured.into()
    }
    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        for p in children.iter_mut() {
            p.origin = bounds.origin();
            p.size = bounds.size();
        }
    }
    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

fn main() {
    FernAppBuilder::new()
        .install_inspector_in_debug()
        .theme(fern_ui::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("FernUI — SpinBox gallery")
                .size(720, 560)
                .root(|tree, _state| {
                    fern!(tree => VStack {
                            child: dark_mode_toolbar()
                            Expand {
                                Root::new()
                            }
                        }
                    )
                }),
        )
        .run();
}
