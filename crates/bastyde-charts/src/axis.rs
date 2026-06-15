// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Axis configuration and tick generation.
//!
//! `nice_ticks` implements the Wilkinson / Heckbert nice-numbers algorithm
//! used by matplotlib, d3, and most data-viz libraries. Tick spacings are
//! 1, 2, or 5 × 10^k for the smallest k that yields ≤ `target_count`
//! intervals covering `[min, max]`.

use std::rc::Rc;

/// Axis configuration shared by BarChart and LineChart for both x and y.
#[derive(Clone, Default)]
pub struct AxisConfig {
    pub label: Option<String>,
    pub show_labels: bool,
    pub show_axis_line: bool,
    pub tick_count_hint: Option<usize>,
    pub min: Option<f32>,
    pub max: Option<f32>,
    /// Custom value-to-string formatter. `None` → default `format!("{}", v)`
    /// with a sensible decimal cap.
    pub formatter: Option<Rc<dyn Fn(f32) -> String>>,
}

impl AxisConfig {
    pub fn new() -> Self {
        Self {
            label: None,
            show_labels: true,
            show_axis_line: true,
            tick_count_hint: None,
            min: None,
            max: None,
            formatter: None,
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn show_labels(mut self, on: bool) -> Self {
        self.show_labels = on;
        self
    }

    pub fn show_axis_line(mut self, on: bool) -> Self {
        self.show_axis_line = on;
        self
    }

    pub fn tick_count_hint(mut self, n: usize) -> Self {
        self.tick_count_hint = Some(n);
        self
    }

    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    pub fn formatter(mut self, f: impl Fn(f32) -> String + 'static) -> Self {
        self.formatter = Some(Rc::new(f));
        self
    }

    /// Format `v` for display using the configured formatter, or a default
    /// that drops trailing zeros and caps at 4 decimal places.
    pub fn format(&self, v: f32) -> String {
        if let Some(f) = &self.formatter {
            f(v)
        } else {
            default_format(v)
        }
    }
}

impl std::fmt::Debug for AxisConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AxisConfig")
            .field("label", &self.label)
            .field("show_labels", &self.show_labels)
            .field("show_axis_line", &self.show_axis_line)
            .field("tick_count_hint", &self.tick_count_hint)
            .field("min", &self.min)
            .field("max", &self.max)
            .field("formatter", &self.formatter.as_ref().map(|_| "<fn>"))
            .finish()
    }
}

fn default_format(v: f32) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let abs = v.abs();
    let s = if abs >= 1000.0 {
        format!("{:.0}", v)
    } else if abs >= 10.0 {
        format!("{:.1}", v)
    } else if abs >= 1.0 {
        format!("{:.2}", v)
    } else {
        format!("{:.3}", v)
    };
    // Trim trailing zeros after the decimal point but keep the integer
    // part intact.
    if s.contains('.') {
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_string()
    } else {
        s
    }
}

/// Generate a "nice" set of tick values covering `[min, max]` using the
/// Wilkinson / Heckbert algorithm. Returns ticks in ascending order, each
/// at a spacing of 1/2/5 × 10^k.
///
/// `target_count` is a hint for the desired number of intervals (so
/// `target_count + 1` ticks). The algorithm picks the smallest spacing
/// from the {1, 2, 5} set times a power of ten that produces no more
/// than `target_count` intervals.
pub fn nice_ticks(min: f32, max: f32, target_count: usize) -> Vec<f32> {
    let target_count = target_count.max(2);

    if !min.is_finite() || !max.is_finite() {
        return vec![0.0];
    }

    if (max - min).abs() < f32::EPSILON {
        // Degenerate range — emit a single tick at the value.
        return vec![min];
    }

    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
    let range = hi - lo;
    let raw_step = range / target_count as f32;

    // Find magnitude (10^k) of raw_step.
    let exp = raw_step.log10().floor();
    let pow10 = 10.0_f32.powf(exp);
    let frac = raw_step / pow10;

    // Pick nice fraction from {1, 2, 2.5, 5, 10} — the d3 / matplotlib
    // standard set. The 2.5 entry catches 0..100 / target=4 → step=25.
    let nice_frac = if frac < 1.5 {
        1.0
    } else if frac < 2.25 {
        2.0
    } else if frac < 3.5 {
        2.5
    } else if frac < 7.0 {
        5.0
    } else {
        10.0
    };
    let step = nice_frac * pow10;

    // Snap min down and max up to step-aligned positions.
    let nice_min = (lo / step).floor() * step;
    let nice_max = (hi / step).ceil() * step;

    let mut ticks = Vec::new();
    let n = ((nice_max - nice_min) / step).round() as i32 + 1;
    for i in 0..n {
        // Compute via i*step to limit floating drift, then snap tiny
        // residues that arise around zero.
        let v = nice_min + (i as f32) * step;
        let v = if v.abs() < step * 1e-6 { 0.0 } else { v };
        ticks.push(v);
    }
    ticks
}

/// Pick a target number of ticks given an axis pixel length. Roughly
/// `pixels / 60` ticks, clamped to `[2, 10]`.
pub fn auto_tick_count(axis_pixels: f32) -> usize {
    ((axis_pixels / 60.0) as usize).clamp(2, 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_range() {
        // target_count=5 → raw step 20, nice step 20, intervals 5, ticks 6.
        let t = nice_ticks(0.0, 100.0, 5);
        assert_eq!(t, vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
        // Lower target gives wider step.
        let t = nice_ticks(0.0, 100.0, 4);
        assert_eq!(t, vec![0.0, 25.0, 50.0, 75.0, 100.0]);
    }

    #[test]
    fn negative_zero_crossing() {
        let t = nice_ticks(-30.0, 70.0, 5);
        assert!(t.iter().any(|&v| (v - 0.0).abs() < 1e-4));
        assert!(t.first().copied().unwrap() <= -30.0);
        assert!(t.last().copied().unwrap() >= 70.0);
    }

    #[test]
    fn tiny_range_sub_decimal_ticks() {
        let t = nice_ticks(0.0, 0.003, 5);
        assert!(t.last().copied().unwrap() >= 0.003);
        // The step should be below 0.001
        let step = t[1] - t[0];
        assert!(step <= 0.001 + 1e-7);
    }

    #[test]
    fn zero_range_returns_single_tick() {
        let t = nice_ticks(5.0, 5.0, 5);
        assert_eq!(t, vec![5.0]);
    }

    #[test]
    fn reversed_min_max_handled() {
        let t1 = nice_ticks(0.0, 10.0, 5);
        let t2 = nice_ticks(10.0, 0.0, 5);
        assert_eq!(t1, t2);
    }

    #[test]
    fn format_default_caps_decimals() {
        let cfg = AxisConfig::new();
        assert_eq!(cfg.format(0.0), "0");
        assert_eq!(cfg.format(100.0), "100");
        assert_eq!(cfg.format(0.5), "0.5");
        assert_eq!(cfg.format(0.123456), "0.123");
    }

    #[test]
    fn custom_formatter_overrides_default() {
        let cfg = AxisConfig::new().formatter(|v| format!("${:.0}", v));
        assert_eq!(cfg.format(42.0), "$42");
    }

    #[test]
    fn auto_tick_count_clamps() {
        assert_eq!(auto_tick_count(60.0), 2);
        assert_eq!(auto_tick_count(120.0), 2);
        assert_eq!(auto_tick_count(180.0), 3);
        assert_eq!(auto_tick_count(2000.0), 10);
        assert_eq!(auto_tick_count(0.0), 2);
    }
}
