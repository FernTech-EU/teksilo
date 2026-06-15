// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Built-in [`Theme`](crate::styles::Theme) presets.
//!
//! Each submodule offers a `light()` and `dark()` constructor returning
//! a fully populated `Theme`. Apps pick a preset explicitly:
//!
//! ```
//! use bastyde_core::presets::intui;
//! let theme = intui::light();
//! ```
//!
//! IntUI ships in core. Sibling preset crates (`bastyde-theme-material3`,
//! `bastyde-theme-macos`, `bastyde-theme-fluent`) live behind opt-in Cargo
//! features in the `bastyde` umbrella crate.

pub mod intui;
