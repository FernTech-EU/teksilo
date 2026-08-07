// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use slotmap::new_key_type;

new_key_type! {
    /// Unique identifier for a widget in the arena.
    pub struct WidgetId;
}
