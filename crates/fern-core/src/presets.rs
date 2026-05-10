//! Built-in [`Theme`](crate::styles::Theme) presets.
//!
//! Each submodule offers a `light()` and `dark()` constructor returning
//! a fully populated `Theme`. Apps pick a preset explicitly:
//!
//! ```ignore
//! use fern_core::presets::intui;
//! let theme = intui::light();
//! ```
//!
//! IntUI ships in core. Sibling preset crates (`fern-theme-material3`,
//! `fern-theme-macos`, `fern-theme-fluent`) live behind opt-in Cargo
//! features in the `fern-ui` umbrella crate.

pub mod intui;
