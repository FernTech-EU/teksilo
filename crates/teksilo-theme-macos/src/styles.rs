// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! macOS Tier-3 widget styles, installed into the theme's `style_slots`
//! by [`crate::light`] / [`crate::dark`].
//!
//! The split is deliberate. Widgets whose AppKit counterpart differs in
//! **structure** get a real `impl FooStyle` here — the push button's
//! bezel, the switch's near-full-height knob, the checkbox's and radio's
//! bezelled unchecked state, the field's accent focus ring, the slider's
//! shadowed round knob, the menu row's accent highlight, the list row's
//! selection capsule. Widgets that differ only in **dimensions** are the
//! shipped `Recipe*Style` constructed with macOS numbers, collected in
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
