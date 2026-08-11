// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Fluent Tier-3 widget styles, installed into the theme's `style_slots`
//! by [`crate::light`] / [`crate::dark`].
//!
//! The split is deliberate. Widgets whose WinUI counterpart differs in
//! **structure** get a real `impl FooStyle` here — the button's elevation
//! edge, the switch's off-state outline, the checkbox and radio's filled
//! unchecked state, the field's accent focus underline, the slider's
//! two-circle thumb, the menu row's neutral hover, the list row's selection
//! pill. Widgets that differ only in **dimensions** are the shipped
//! `Recipe*Style` constructed with Fluent numbers, collected in
//! [`metrics`].

pub mod button;
pub mod checkbox;
pub mod chrome;
pub mod menu_item;
pub mod metrics;
pub mod radio;
pub mod slider;
pub mod standard_item;
pub mod text_input;
pub mod toggle;
