//! Light vs Dark surface declaration.
//!
//! Required field on every [`Theme`](crate::styles::Theme). Drives:
//!
//! - Per-component shadow density (dark themes need higher shadow alphas
//!   to read on dark surfaces).
//! - OS-theme matching (light/dark mode auto-switching reads this off
//!   the active theme).
//! - Asset variant selection (logos, icons that ship light/dark pairs).
//!
//! Independent of which preset built the theme: an `intui::light()`
//! theme and a hypothetical `material3::light()` theme both report
//! [`ThemeAppearance::Light`].

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ThemeAppearance {
    Light,
    Dark,
}

impl ThemeAppearance {
    pub fn is_dark(self) -> bool {
        matches!(self, ThemeAppearance::Dark)
    }

    pub fn is_light(self) -> bool {
        matches!(self, ThemeAppearance::Light)
    }
}
