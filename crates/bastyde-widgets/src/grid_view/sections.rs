//! Section grouping for `GridView`.
//!
//! A [`SectionProvider`] partitions the flat model into named sections; the
//! grid renders a header above each section's tile band and (optionally)
//! keeps the current section's header pinned to the top while scrolling.
//! Sections compose with the uniform tile layout.

use std::rc::Rc;

use bastyde_data::ListModel;

/// Partitions a flat model into display sections.
pub trait SectionProvider: 'static {
    /// Number of sections.
    fn section_count(&self) -> usize;
    /// Number of items in `section`.
    fn items_in_section(&self, section: usize) -> usize;
    /// Display title for `section`.
    fn section_title(&self, section: usize) -> String;

    /// Per-section item counts, in order. Used by the layout strategy.
    fn section_counts(&self) -> Vec<usize> {
        (0..self.section_count())
            .map(|s| self.items_in_section(s))
            .collect()
    }
}

/// A [`SectionProvider`] built by partitioning consecutive equal-key runs of
/// an (already ordered) model. The titles come from each run's key.
pub struct GroupingSections {
    /// `(title, count)` per section, captured at build time.
    runs: Vec<(String, usize)>,
}

impl GroupingSections {
    fn new(runs: Vec<(String, usize)>) -> Self {
        Self { runs }
    }
}

impl SectionProvider for GroupingSections {
    fn section_count(&self) -> usize {
        self.runs.len()
    }
    fn items_in_section(&self, section: usize) -> usize {
        self.runs.get(section).map(|(_, c)| *c).unwrap_or(0)
    }
    fn section_title(&self, section: usize) -> String {
        self.runs.get(section).map(|(t, _)| t.clone()).unwrap_or_default()
    }
}

/// Build a [`SectionProvider`] by grouping consecutive items of `model` that
/// share a `key_fn` value into one section, titled by the key. The model is
/// not sorted — callers should pre-sort if they want fully-grouped sections.
pub fn grouping_sections<T, K, F>(model: &ListModel<T>, key_fn: F) -> GroupingSections
where
    T: 'static,
    K: ToString + PartialEq + 'static,
    F: Fn(&T) -> K + 'static,
{
    let mut runs: Vec<(String, usize)> = Vec::new();
    let mut last_key: Option<K> = None;
    for i in 0..model.len() {
        let key = model.with_item(i, &key_fn);
        if let Some(key) = key {
            let same = last_key.as_ref().map(|k| *k == key).unwrap_or(false);
            if same {
                if let Some(last) = runs.last_mut() {
                    last.1 += 1;
                }
            } else {
                runs.push((key.to_string(), 1));
                last_key = Some(key);
            }
        }
    }
    GroupingSections::new(runs)
}

/// Internal handle bundling the bits the grid needs from a section provider:
/// the cached counts closure and the title lookup.
#[derive(Clone)]
pub(crate) struct SectionData {
    pub(crate) counts_fn: Rc<dyn Fn() -> Vec<usize>>,
    pub(crate) title_fn: Rc<dyn Fn(usize) -> String>,
}
