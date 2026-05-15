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

pub use alignment::{Alignment, HAlignment, VAlignment};
pub use color::Color;
pub use layout::LayoutTokens;
pub use motion::{Easing, MotionTokens, lerp};
pub use orientation::Orientation;
pub use os_theme_colors::{ColorSchemePreference, OsThemeColors};
pub use roles::{BorderRole, SurfaceRole, TextRole, TextStyleRole};
pub use shape::{CornerRadius, Shadow, ShapeTokens};
pub use text_style::{FontWeight, TextStyle};
pub use theme::ColorTokens;
// Theme aggregator lives in `fern-core` (so it can co-locate with the
// per-widget style trait protocols and the typed slot bag). Apps reach
// it via `use fern_core::Theme` or the umbrella `use fern_ui::prelude::*`.
pub use typography::TypographyTokens;
