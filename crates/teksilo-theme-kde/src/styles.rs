// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Breeze Tier-3 widget styles, installed into the theme's `style_slots` by
//! [`crate::light`], [`crate::dark`] and every constructor built on
//! [`crate::from_scheme`].
//!
//! Widgets whose Breeze counterpart differs in **structure** get a real
//! `impl FooStyle` here:
//!
//! - the push button, the default button and the flat tool button, with
//!   their outline that turns to the highlight under the pointer and their
//!   one-pixel shadow, sharing one painter, [`chrome::BreezeButtonChrome`];
//! - the check box and the radio button, a 15 px box or disc in a 20 px
//!   indicator marked in the view's text, and the desktop QtQuick style's
//!   switch, a handle as tall as its track's indicator, which share their
//!   colours through [`indicator`];
//! - the line edit, a view-coloured frame whose outline turns to the focus
//!   colour, in [`text_input`].
//!
//! The dimensions they are built with, all derived from [`crate::geometry`]
//! and the density ladder, are collected in [`metrics`].

pub mod button;
pub mod checkbox;
pub mod chrome;
pub mod icon_button;
pub mod indicator;
pub mod metrics;
pub mod radio;
pub mod split_button;
pub mod text_input;
pub mod toggle;
