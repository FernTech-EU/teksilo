// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Automation bridge install hook.
//!
//! Re-exports [`TeksiloAppBuilderAutomationExt`](teksilo_app::automation_bridge::TeksiloAppBuilderAutomationExt) from `teksilo-app` so that
//! `use teksilo::prelude::*` makes
//! [`install_automation_bridge_in_debug()`](teksilo_app::automation_bridge::TeksiloAppBuilderAutomationExt::install_automation_bridge_in_debug)
//! callable on a [`TeksiloAppBuilder`](teksilo_app::TeksiloAppBuilder).
//! Mirrors `toast_install` / `webview_install`.
//!
//! The bridge lets `teksilo-automation-mcp --connect <sock> --token <uuid>`
//! drive a *live* running app. It is debug-only by construction: the install
//! method is a no-op in a release build (see the `teksilo-app` crate). The
//! GUI-free DTO toolkit is available as `teksilo::automation`.

pub use teksilo_app::automation_bridge::TeksiloAppBuilderAutomationExt;
