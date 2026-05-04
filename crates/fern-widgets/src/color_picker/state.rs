//! Internal state for [`ColorPicker`](super::ColorPicker).
//!
//! `ColorComponents` is constructed once in the picker's `build()` and
//! holds derived `Signal`s for each RGB / HSV channel plus typed
//! setters that recompose the bound `Signal<Color>` when a single
//! channel changes. Centralizing the conversions here keeps every
//! subwidget (HSV canvas, hue strip, alpha strip, RGB/HSV spinners,
//! preview) reading from clean per-channel signals without duplicating
//! the conversion logic.
//!
//! # Hue preservation across grays
//!
//! When saturation drops to 0 or value drops to 0, the underlying
//! sRGB representation has no hue (gray / black). A naive
//! `value.map(|c| c.to_hsv().0)` would clamp the visible hue back to
//! 0° — which makes the HSV canvas snap from "red" back to "red at the
//! top-left" the moment the user drags down to white. We avoid this by
//! caching the last non-degenerate hue in [`ColorComponents`] and
//! returning it whenever the bound color's saturation/value collapses.

use std::cell::Cell;
use std::rc::Rc;

use fern_core::build_context::BuildContext;
use fern_core::signal::Signal;
use fern_tokens::Color;

/// Derived signals + setters for the RGB and HSV channels of a bound
/// `Signal<Color>`.
#[allow(dead_code)]
pub(crate) struct ColorComponents {
    pub value: Signal<Color>,

    // ── RGB ── (each in 0..=1)
    pub red: Signal<f32>,
    pub green: Signal<f32>,
    pub blue: Signal<f32>,
    pub alpha: Signal<f32>,

    // ── HSV ──
    pub hue: Signal<f32>,        // 0..360
    pub saturation: Signal<f32>, // 0..1
    pub value_hsv: Signal<f32>,  // 0..1

    // ── Setters ──
    pub set_red: Rc<dyn Fn(f32)>,
    pub set_green: Rc<dyn Fn(f32)>,
    pub set_blue: Rc<dyn Fn(f32)>,
    pub set_alpha: Rc<dyn Fn(f32)>,
    pub set_hue: Rc<dyn Fn(f32)>,
    pub set_saturation: Rc<dyn Fn(f32)>,
    pub set_value_hsv: Rc<dyn Fn(f32)>,
    /// Batch-update the entire HSV triple in one signal write — used
    /// by the HSV canvas drag handler so the bound signal mutates
    /// once per pointer event instead of three times.
    pub set_hsv: Rc<dyn Fn(f32, f32, f32)>,

    /// Shared mid-drag flag — set by HSV canvas, hue strip, and alpha
    /// strip during a pointer drag, used by the picker's
    /// HexColorInput-feedback effect to skip non-focused reformats.
    /// Mirrors the Slider pattern.
    pub dragging: Rc<Cell<bool>>,

    /// Last hue with non-zero saturation. Returned by `hue` when the
    /// current color is a gray (saturation = 0) so the HSV canvas
    /// keeps its base color as the user drags through white / black.
    last_hue_cell: Rc<Cell<f32>>,
}

impl ColorComponents {
    pub(crate) fn new(ctx: &mut BuildContext, value: Signal<Color>) -> Self {
        let initial = value.get();
        let (init_h, init_s, init_v) = initial.to_hsv();
        let last_hue_cell = Rc::new(Cell::new(if init_s > 1e-6 { init_h } else { 0.0 }));

        // Update last_hue_cell whenever the bound color has a real hue.
        {
            let cell = last_hue_cell.clone();
            ctx.effect(&value, move |c| {
                let (h, s, _v) = c.to_hsv();
                if s > 1e-6 {
                    cell.set(h);
                }
            });
        }

        // RGB derived signals.
        let red = value.map(|c| c.r());
        let green = value.map(|c| c.g());
        let blue = value.map(|c| c.b());
        let alpha = value.map(|c| c.a());

        // HSV derived signals — saturation/value are direct; hue
        // substitutes the cached last-hue when current is degenerate.
        let hue = {
            let cell = last_hue_cell.clone();
            value.map(move |c| {
                let (h, s, _v) = c.to_hsv();
                if s > 1e-6 { h } else { cell.get() }
            })
        };
        let saturation = value.map(|c| c.to_hsv().1);
        let value_hsv = value.map(|c| c.to_hsv().2);

        // Setters — each writes a re-composed Color back to the bound signal.
        let set_red = {
            let v = value.clone();
            Rc::new(move |r: f32| {
                let c = v.get();
                v.set(Color::from_rgba(r.clamp(0.0, 1.0), c.g(), c.b(), c.a()));
            }) as Rc<dyn Fn(f32)>
        };
        let set_green = {
            let v = value.clone();
            Rc::new(move |g: f32| {
                let c = v.get();
                v.set(Color::from_rgba(c.r(), g.clamp(0.0, 1.0), c.b(), c.a()));
            }) as Rc<dyn Fn(f32)>
        };
        let set_blue = {
            let v = value.clone();
            Rc::new(move |b: f32| {
                let c = v.get();
                v.set(Color::from_rgba(c.r(), c.g(), b.clamp(0.0, 1.0), c.a()));
            }) as Rc<dyn Fn(f32)>
        };
        let set_alpha = {
            let v = value.clone();
            Rc::new(move |a: f32| {
                let c = v.get();
                v.set(Color::from_rgba(c.r(), c.g(), c.b(), a.clamp(0.0, 1.0)));
            }) as Rc<dyn Fn(f32)>
        };
        let set_hue = {
            let v = value.clone();
            let cell = last_hue_cell.clone();
            Rc::new(move |h: f32| {
                let c = v.get();
                let (_old_h, s, val) = c.to_hsv();
                let h_norm = h.rem_euclid(360.0);
                cell.set(h_norm);
                v.set(Color::from_hsva(h_norm, s, val, c.a()));
            }) as Rc<dyn Fn(f32)>
        };
        let set_saturation = {
            let v = value.clone();
            let cell = last_hue_cell.clone();
            Rc::new(move |s: f32| {
                let c = v.get();
                let (cur_h, _s, val) = c.to_hsv();
                let h = if c.to_hsv().1 > 1e-6 { cur_h } else { cell.get() };
                v.set(Color::from_hsva(h, s.clamp(0.0, 1.0), val, c.a()));
            }) as Rc<dyn Fn(f32)>
        };
        let set_value_hsv = {
            let v = value.clone();
            let cell = last_hue_cell.clone();
            Rc::new(move |val: f32| {
                let c = v.get();
                let (cur_h, s, _v) = c.to_hsv();
                let h = if s > 1e-6 { cur_h } else { cell.get() };
                v.set(Color::from_hsva(h, s, val.clamp(0.0, 1.0), c.a()));
            }) as Rc<dyn Fn(f32)>
        };
        let set_hsv = {
            let v = value.clone();
            let cell = last_hue_cell.clone();
            Rc::new(move |h: f32, s: f32, val: f32| {
                let c = v.get();
                let h_norm = h.rem_euclid(360.0);
                if s > 1e-6 {
                    cell.set(h_norm);
                }
                v.set(Color::from_hsva(
                    h_norm,
                    s.clamp(0.0, 1.0),
                    val.clamp(0.0, 1.0),
                    c.a(),
                ));
            }) as Rc<dyn Fn(f32, f32, f32)>
        };

        Self {
            value,
            red,
            green,
            blue,
            alpha,
            hue,
            saturation,
            value_hsv,
            set_red,
            set_green,
            set_blue,
            set_alpha,
            set_hue,
            set_saturation,
            set_value_hsv,
            set_hsv,
            dragging: Rc::new(Cell::new(false)),
            last_hue_cell,
        }
    }
}
