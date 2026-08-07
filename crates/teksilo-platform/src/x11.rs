// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! X11 support: the window-manager capability probe behind the custom title
//! bar, and the XDND protocol layer behind external drag-and-drop.
//!
//! Compiled only on unix-not-macOS. Nothing here runs under a Wayland session —
//! callers gate on the live `RawDisplayHandle` (see
//! [`window_system_for_display_handle`]) or, before a window exists, on
//! [`active_window_system`].
//!
//! Layering, deliberately:
//!
//! - [`xdnd`] is **pure** — message codecs, version negotiation, `XdndProxy`
//!   validation, `text/uri-list` parsing, `INCR` assembly. No connection, so it
//!   is exhaustively unit-testable with no display server. This is where the
//!   automated coverage lives, because wire-level behaviour against a real
//!   server cannot be exercised in `cargo test`.
//! - [`connection`] owns our private X connection, the atom cache, and the
//!   property/event helpers.
//! - [`ewmh`] is the pre-window-creation probe that decides whether custom
//!   chrome is safe.
//!
//! [`window_system_for_display_handle`]: crate::window_system::window_system_for_display_handle
//! [`active_window_system`]: crate::window_system::active_window_system

pub mod connection;
pub mod ewmh;
pub mod xdnd;

pub use connection::{Atoms, PropertyValue, X11Connection, X11Error};
pub use ewmh::{WmCapabilities, capabilities};
