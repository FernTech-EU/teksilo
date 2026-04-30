//! `ChartSeries<T>` and `ChartDatum<T>` — the data shapes consumed by every
//! chart in this crate.
//!
//! `T` is the **category / x-axis** type — typically `String` for textual
//! labels, an enum for fixed buckets, or a date type later. The numeric
//! `value` is always `f32`. Bar and line charts both bind to
//! `Prop<Vec<ChartSeries<T>>>`.

use fern_core::color_prop::ColorProp;
use fern_core::signal::Signal;

/// One numeric data point at a category/x-axis position.
#[derive(Debug, Clone)]
pub struct ChartDatum<T> {
    pub category: T,
    pub value: f32,
}

impl<T> ChartDatum<T> {
    pub fn new(category: T, value: f32) -> Self {
        Self { category, value }
    }
}

/// One named series of data points with an optional explicit color and a
/// reactive visibility flag (toggleable from a legend).
pub struct ChartSeries<T> {
    pub name: String,
    pub color: Option<ColorProp>,
    pub visible: Signal<bool>,
    pub data: Vec<ChartDatum<T>>,
}

impl<T> std::fmt::Debug for ChartSeries<T>
where
    T: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChartSeries")
            .field("name", &self.name)
            .field("len", &self.data.len())
            .finish()
    }
}

impl<T> Clone for ChartSeries<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            color: self.color.clone(),
            visible: self.visible.clone(),
            data: self.data.clone(),
        }
    }
}

impl<T> ChartSeries<T> {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            color: None,
            visible: Signal::new(true),
            data: Vec::new(),
        }
    }

    pub fn color(mut self, color: impl Into<ColorProp>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn data(mut self, data: Vec<ChartDatum<T>>) -> Self {
        self.data = data;
        self
    }

    pub fn push(&mut self, category: T, value: f32) {
        self.data.push(ChartDatum::new(category, value));
    }

    /// Bind the series visibility to an externally-owned signal (so a
    /// legend or a settings UI can toggle multiple series in sync).
    pub fn visibility(mut self, signal: Signal<bool>) -> Self {
        self.visible = signal;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_chain() {
        let mut s = ChartSeries::<String>::new("Revenue").data(vec![
            ChartDatum::new("Q1".into(), 10.0),
            ChartDatum::new("Q2".into(), 20.0),
        ]);
        s.push("Q3".into(), 30.0);
        assert_eq!(s.name, "Revenue");
        assert_eq!(s.data.len(), 3);
        assert!(s.visible.get());
    }

    #[test]
    fn visibility_signal_externally_owned() {
        let vis = Signal::new(false);
        let s = ChartSeries::<String>::new("X").visibility(vis.clone());
        assert!(!s.visible.get());
        vis.set(true);
        assert!(s.visible.get());
    }
}
