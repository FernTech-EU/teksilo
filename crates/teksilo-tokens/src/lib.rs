// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

pub mod alignment;
pub mod color;
pub mod layout;
pub mod motion;
pub mod orientation;
pub mod os_theme_colors;
pub mod roles;
pub mod shape;
pub mod text_style;
pub mod theme;
pub mod typography;

pub use alignment::{Alignment, Corner, HAlignment, VAlignment};
pub use color::Color;
pub use layout::LayoutTokens;
pub use motion::{Easing, MotionTokens, lerp};
pub use orientation::Orientation;
pub use os_theme_colors::{ColorSchemePreference, OsThemeColors};
pub use roles::{BorderRole, SurfaceRole, TextRole, TextStyleRole};
pub use shape::{CornerRadius, Shadow, ShapeTokens};
pub use text_style::{FontWeight, TextStyle};
pub use theme::ColorTokens;
// Theme aggregator lives in `teksilo-core` (so it can co-locate with the
// per-widget style trait protocols and the typed slot bag). Apps reach
// it via `use teksilo_core::Theme` or the umbrella `use teksilo::prelude::*`.
pub use typography::TypographyTokens;
