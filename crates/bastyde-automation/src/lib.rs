// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! # bastyde-automation
//!
//! GUI-free runtime-introspection & automation toolkit for Bastyde apps.
//!
//! A Bastyde app exposes a rich semantic surface — the AccessKit
//! accessibility tree — plus an AT-action dispatch path, both of which are
//! queryable and drivable **in-process, without the OS accessibility
//! layer**. This crate turns that latent capability into a small set of
//! serde DTOs plus one core function, [`execute`], that an MCP server (or
//! any other harness) marshals operations to.
//!
//! ```text
//! execute(tree: &mut WidgetTree, ops: &mut dyn WindowOps,
//!         op: &AutomationOp, settle: &SettleSpec) -> AutomationReply
//! ```
//!
//! The wire protocol is **serde DTOs, never closures or `!Send` handles**
//! ([`dto`]). [`WidgetTree`](bastyde_core::WidgetTree) is `!Send`, so it
//! lives on exactly one thread; async / socket layers marshal `Send` DTOs
//! to it. Two operations need context [`execute`] doesn't hold —
//! `list_windows` (the window manager) and `screenshot` (a GPU / platform
//! window) — and return [`dto::codes::HOST_REQUIRED`]; the headless server
//! and the live bridge serve those themselves.
//!
//! This crate has no GUI, render, platform, or async dependency. It mirrors
//! `bastyde-data`'s "core-only peer" design so a CI harness, a headless
//! test, or the live in-app bridge can all share it.
//!
//! ## Modules
//! - [`dto`] — the serde wire protocol.
//! - [`executor`] — [`execute`] and the settle model.
//! - [`recording_ops`] — a non-panicking [`WindowOps`](bastyde_core::WindowOps)
//!   for the headless server.
//! - [`mcp_schema`] — the canonical tool catalog.

pub mod dto;
pub mod executor;
pub mod mcp_schema;
pub mod recording_ops;

pub use dto::{
    AnnouncementDto, Assertion, AssertionResult, AutomationOp, AutomationReply, AutomationRequest,
    NodeBounds, NodeRef, PointerAction, PointerButtonDto, SemanticNode, SettleSpec, ShortcutInfo,
    WaitCondition, WindowInfo, codes,
};
pub use executor::{execute, run_settle};
pub use mcp_schema::{TOOL_CATALOG, TOOL_COUNT, ToolDescriptor};
pub use recording_ops::{RecordedWindow, RecordingWindowOps};

#[cfg(test)]
mod tests;
