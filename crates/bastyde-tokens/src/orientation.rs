// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use serde::{Deserialize, Serialize};

/// Orientation for widgets that can be horizontal or vertical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Orientation {
    #[default]
    Horizontal,
    Vertical,
}
