// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Breeze Tier-3 widget styles, installed into the theme's `style_slots` by
//! [`crate::light`], [`crate::dark`] and every constructor built on
//! [`crate::from_scheme`].
//!
//! Widgets whose Breeze counterpart differs in **structure** get a real
//! `impl FooStyle` here: the push button, the default button and the flat
//! tool button, with their outline that turns to the highlight under the
//! pointer and their one-pixel shadow. They share one painter,
//! [`chrome::BreezeButtonChrome`]. The dimensions they are built with, all
//! derived from [`crate::geometry`] and the density ladder, are collected in
//! [`metrics`].

pub mod button;
pub mod chrome;
pub mod icon_button;
pub mod metrics;
pub mod split_button;
