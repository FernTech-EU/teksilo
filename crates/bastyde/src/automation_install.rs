// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Automation bridge install hook.
//!
//! Re-exports [`BastydeAppBuilderAutomationExt`](bastyde_app::automation_bridge::BastydeAppBuilderAutomationExt) from `bastyde-app` so that
//! `use bastyde::prelude::*` makes
//! [`install_automation_bridge_in_debug()`](bastyde_app::automation_bridge::BastydeAppBuilderAutomationExt::install_automation_bridge_in_debug)
//! callable on a [`BastydeAppBuilder`](bastyde_app::BastydeAppBuilder).
//! Mirrors `toast_install` / `webview_install`.
//!
//! The bridge lets `bastyde-automation-mcp --connect <sock> --token <uuid>`
//! drive a *live* running app. It is debug-only by construction: the install
//! method is a no-op in a release build (see the `bastyde-app` crate). The
//! GUI-free DTO toolkit is available as `bastyde::automation`.

pub use bastyde_app::automation_bridge::BastydeAppBuilderAutomationExt;
