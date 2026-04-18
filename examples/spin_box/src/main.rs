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
    Button, ButtonVariant, Expand, HStack, Padding, Panel, SpinBox, StepType, TextWidget, VStack,
    WheelMode, WrapMode,
};

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
        let theme = ctx.theme().clone();
        let t = &theme.typography;
        let c = &theme.colors;

        // Live readouts so the demo visibly confirms each signal.
        let font_size_text = self.values.font_size.map(|v| format!("{} pt", v));
        let gain_text = self.values.gain_db.map(|v| format!("{:.1} dB", v));
        let opacity_text = self.values.opacity.map(|v| format!("{} %", v));
        let timeout_text = self.values.timeout.map(|v| if *v == 0 {
            "Auto".to_string()
        } else {
            format!("{} s", v)
        });
        let frequency_text = self.values.frequency.map(|v| format!("{:.2} Hz", v));

        let reset_font = self.values.font_size.clone();
        let reset_gain = self.values.gain_db.clone();
        let reset_opacity = self.values.opacity.clone();
        let reset_timeout = self.values.timeout.clone();
        let reset_frequency = self.values.frequency.clone();

        let root = ctx.add(
            Padding::uniform(24.0).child(
                Panel::new().child(
                    Padding::uniform(20.0).child(
                        VStack::new()
                            .spacing(14.0)
                            // Heading.
                            .child(
                                TextWidget::new_literal("SpinBox gallery")
                                    .style(t.body_bold.clone())
                                    .color(c.text_primary),
                            )
                            .child(
                                TextWidget::new_literal(
                                    "Every SpinBox feature on one page. \
                                    Use arrow keys, Page↑/Page↓, mouse wheel, \
                                    or the ± buttons; press Enter or Tab to commit typed input.",
                                )
                                .style(t.body.clone())
                                .color(c.text_secondary),
                            )
                            // Row 1 — font size (integer, clamp, narrow width).
                            .child(row(
                                "Font size (narrow 80 dp)",
                                SpinBox::new(self.values.font_size.clone(), 4, 96)
                                    .single_step(1)
                                    .page_step(10)
                                    .suffix(" pt")
                                    .width(80.0)
                                    .label("Font size"),
                                font_size_text,
                            ))
                            // Row 2 — gain dB (float, 1 decimal).
                            .child(row(
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
                            ))
                            // Row 3 — opacity (integer, wrap mode for fun).
                            .child(row(
                                "Opacity",
                                SpinBox::new(self.values.opacity.clone(), 0, 100)
                                    .single_step(5)
                                    .page_step(25)
                                    .suffix(" %")
                                    .wrap_mode(WrapMode::Wrap)
                                    .label("Opacity"),
                                opacity_text,
                            ))
                            // Row 4 — timeout with special value "Auto".
                            .child(row(
                                "Timeout",
                                SpinBox::new(self.values.timeout.clone(), 0, 3600)
                                    .single_step(1)
                                    .page_step(60)
                                    .suffix(" s")
                                    .special_value_text("Auto")
                                    .label("Timeout"),
                                timeout_text,
                            ))
                            // Row 5 — frequency (adaptive step, wider width).
                            .child(row(
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
                            ))
                            // Row 6 — Int UI-style dense field: buttons hidden.
                            .child(row(
                                "Font size (no buttons, Int UI)",
                                SpinBox::new(self.values.font_size.clone(), 4, 96)
                                    .single_step(1)
                                    .page_step(10)
                                    .suffix(" pt")
                                    .show_buttons(false)
                                    .label("Font size, no buttons"),
                                self.values.font_size.map(|v| format!("{} pt", v)),
                            ))
                            // Row 7 — read-only mirror of font_size.
                            .child(row(
                                "Font size (mirror, read-only)",
                                SpinBox::new(self.values.mirror.clone(), 4, 96)
                                    .suffix(" pt")
                                    .read_only(true)
                                    .label("Font size mirror"),
                                self.values.mirror.map(|v| format!("{} pt", v)),
                            ))
                            // ── Width gallery ──────────────────────
                            //
                            // Four SpinBoxes bound to the same `opacity`
                            // signal so the different widths are easy to
                            // compare side by side. Each row labels its
                            // width policy.
                            .child(
                                TextWidget::new_literal("Width control")
                                    .style(t.body_bold.clone())
                                    .color(c.text_primary),
                            )
                            .child(row(
                                "Narrow — .width(64)",
                                SpinBox::new(self.values.opacity.clone(), 0, 100)
                                    .suffix(" %")
                                    .width(64.0)
                                    .label("Opacity (narrow)"),
                                self.values.opacity.map(|v| format!("{} %", v)),
                            ))
                            .child(row(
                                "Default — 120 dp cap",
                                SpinBox::new(self.values.opacity.clone(), 0, 100)
                                    .suffix(" %")
                                    .label("Opacity (default)"),
                                self.values.opacity.map(|v| format!("{} %", v)),
                            ))
                            .child(row(
                                "Wider — .width(220)",
                                SpinBox::new(self.values.opacity.clone(), 0, 100)
                                    .suffix(" %")
                                    .width(220.0)
                                    .label("Opacity (wide)"),
                                self.values.opacity.map(|v| format!("{} %", v)),
                            ))
                            .child(row(
                                "Chars — .width_chars(3) (fits \"100 %\")",
                                SpinBox::new(self.values.opacity.clone(), 0, 100)
                                    .suffix(" %")
                                    .width_chars(3)
                                    .label("Opacity (3 chars)"),
                                self.values.opacity.map(|v| format!("{} %", v)),
                            ))
                            // `.fill_width()` needs a flex parent to
                            // stretch into, so this row wraps the
                            // SpinBox in `Expand::horizontal().fills_stack()`
                            // instead of the normal `row` helper.
                            .child(
                                HStack::new()
                                    .spacing(12.0)
                                    .child(
                                        MinSizeForLabel::new(TextWidget::new_literal(
                                            "Fill — .fill_width()",
                                        ))
                                        .width(220.0),
                                    )
                                    .child(
                                        Expand::horizontal().fills_stack().child(
                                            SpinBox::new(
                                                self.values.opacity.clone(),
                                                0,
                                                100,
                                            )
                                            .suffix(" %")
                                            .fill_width()
                                            .label("Opacity (fill)"),
                                        ),
                                    )
                                    .child(
                                        TextWidget::new_literal("").bind_text(
                                            self.values.opacity.map(|v| format!("{} %", v)),
                                        ),
                                    ),
                            )
                            // Reset button.
                            .child(
                                HStack::new().spacing(8.0).child(
                                    Button::new_literal("Reset all")
                                        .style(ButtonVariant::Regular)
                                        .on_activate_fn(move |_ctx| {
                                            reset_font.set(12);
                                            reset_gain.set(0.0);
                                            reset_opacity.set(50);
                                            reset_timeout.set(0);
                                            reset_frequency.set(440.0);
                                        }),
                                ),
                            ),
                    ),
                ),
            ),
        );
        self.root_child_id = Some(root);
        vec![root]
    }

    fn size_that_fits(
        &self,
        proposal: SizeProposal,
        ctx: &LayoutContext,
    ) -> Size {
        self.root_child_id
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
    }
}

/// Single demo row: [label | SpinBox | live readout].
fn row(
    label: &str,
    spin: SpinBox<impl fern_ui::widgets::SpinValue>,
    readout: Signal<String>,
) -> impl Widget {
    HStack::new()
        .spacing(12.0)
        .child(
            MinSizeForLabel::new(TextWidget::new_literal(label)).width(220.0),
        )
        .child(spin)
        .child(TextWidget::new_literal("").bind_text(readout))
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
        let child = self.child.take().expect("MinSizeForLabel: child already consumed");
        let id = ctx.add_boxed(child);
        self.child_id = Some(id);
        vec![id]
    }
    fn size_that_fits(&self, proposal: SizeProposal, ctx: &LayoutContext) -> Size {
        let Some(id) = self.child_id else {
            return proposal.resolve(self.width, 0.0);
        };
        let mut measured = ctx
            .child_size(id, proposal)
            .unwrap_or_else(|| proposal.resolve(self.width, 0.0));
        if measured.width < self.width {
            measured.width = self.width;
        }
        measured
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
        .theme(Theme::light_default())
        .window_title("FernUI — SpinBox gallery")
        .window_size(720, 560)
        .root(|tree| tree.add(Root::new()))
        .run();
}
