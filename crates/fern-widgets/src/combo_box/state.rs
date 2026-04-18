//! Internal state types, accessor struct, and pure helpers shared
//! between the public `ComboBox` widget, the `DropdownPanel` overlay
//! content, and the per-row `DropdownItem`.
//!
//! Nothing here is exposed beyond the crate — the public widget lives
//! in [`super::ComboBox`](super::ComboBox).

use std::cell::Cell;
use std::rc::Rc;

use fern_core::ObserverHandle;
use fern_data::{DataChange, ListDataSource, ListModel};
use fern_tokens::Color;

/// Default maximum number of items shown before the dropdown scrolls.
pub(super) const DEFAULT_MAX_VISIBLE_ITEMS: usize = 8;

/// Interaction state for the trigger button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ComboBoxState {
    Idle,
    Hovered,
    Focused,
    Open,
    Disabled,
}

/// Typed accessors shared between the trigger (for keyboard navigation and
/// label resolution) and the dropdown panel (for item rendering).
#[derive(Clone)]
pub(super) struct ItemSource<T: Clone + 'static> {
    len: Rc<dyn Fn() -> usize>,
    item_at: Rc<dyn Fn(usize) -> Option<T>>,
    pub(super) observe: Rc<dyn Fn(Box<dyn Fn(&DataChange)>) -> ObserverHandle>,
}

impl<T: Clone + 'static> ItemSource<T> {
    pub(super) fn from_vec(items: Vec<T>) -> Self {
        Self::from_model(ListModel::from_vec(items))
    }

    pub(super) fn from_model(model: ListModel<T>) -> Self {
        let m_len = model.clone();
        let m_item = model.clone();
        let m_obs = model.clone();
        Self {
            len: Rc::new(move || m_len.len()),
            item_at: Rc::new(move |i| m_item.with_item(i, |t| t.clone())),
            observe: Rc::new(move |f| m_obs.observe_changes(move |c| f(c))),
        }
    }

    pub(super) fn from_data_source<S: ListDataSource<Item = T> + 'static>(source: S) -> Self {
        let s = Rc::new(source);
        let s_len = s.clone();
        let s_item = s.clone();
        let s_obs = s.clone();
        Self {
            len: Rc::new(move || s_len.len()),
            item_at: Rc::new(move |i| s_item.with_item(i, |t| t.clone())),
            observe: Rc::new(move |f| s_obs.observe_changes(move |c| f(c))),
        }
    }

    pub(super) fn len(&self) -> usize {
        (self.len)()
    }

    pub(super) fn get(&self, index: usize) -> Option<T> {
        (self.item_at)(index)
    }
}

pub(super) fn resolve_bg(state: ComboBoxState, colors: &fern_tokens::ColorTokens) -> Color {
    match state {
        ComboBoxState::Idle | ComboBoxState::Focused => colors.surface_main,
        ComboBoxState::Hovered => colors.text_primary.with_alpha(0.04),
        ComboBoxState::Open => colors.text_primary.with_alpha(0.04),
        ComboBoxState::Disabled => colors.accent_disabled,
    }
}

pub(super) fn resolve_border(state: ComboBoxState, colors: &fern_tokens::ColorTokens) -> Color {
    // Int UI convention: the border thickens and switches to the
    // accent color on focus. There is no separate ring.
    match state {
        ComboBoxState::Focused => colors.focus_ring,
        ComboBoxState::Disabled => colors.accent_disabled,
        _ => colors.border,
    }
}

pub(super) fn resolve_text(state: ComboBoxState, colors: &fern_tokens::ColorTokens) -> Color {
    match state {
        ComboBoxState::Disabled => colors.text_disabled,
        _ => colors.text_primary,
    }
}

/// Find the index of `value` in `source`, or `None` if absent.
pub(super) fn index_of<T: Clone + PartialEq + 'static>(
    source: &ItemSource<T>,
    value: &T,
) -> Option<usize> {
    let n = source.len();
    for i in 0..n {
        if source.get(i).as_ref() == Some(value) {
            return Some(i);
        }
    }
    None
}

/// Resolve the index of `value` in `source`, consulting `hint` first.
/// If the hint is still valid (`source[hint] == *value`), returns it
/// in O(1). Otherwise falls back to a linear scan and writes the fresh
/// index back into `hint` (or clears it on miss).
pub(super) fn resolve_index<T: Clone + PartialEq + 'static>(
    source: &ItemSource<T>,
    value: &T,
    hint: &Cell<Option<usize>>,
) -> Option<usize> {
    if let Some(i) = hint.get()
        && source.get(i).as_ref() == Some(value)
    {
        return Some(i);
    }
    let found = index_of(source, value);
    hint.set(found);
    found
}
