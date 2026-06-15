// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Tri-state checkbox value, used by `Checkbox` and `TreeCheckedModel`.
//!
//! Lives in `bastyde-data` (not `bastyde-widgets`) so the data-layer tree
//! checking model can produce `Signal<CheckState>` without inverting
//! the dependency graph.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CheckState {
    Unchecked,
    Checked,
    Indeterminate,
}

impl CheckState {
    /// Whether the box shows a filled background (checked or indeterminate).
    pub fn is_filled(self) -> bool {
        self != CheckState::Unchecked
    }

    /// Cycle to the next state: Unchecked → Checked → Indeterminate → Unchecked.
    pub fn next_tristate(self) -> Self {
        match self {
            CheckState::Unchecked => CheckState::Checked,
            CheckState::Checked => CheckState::Indeterminate,
            CheckState::Indeterminate => CheckState::Unchecked,
        }
    }
}

impl From<bool> for CheckState {
    fn from(checked: bool) -> Self {
        if checked {
            CheckState::Checked
        } else {
            CheckState::Unchecked
        }
    }
}
