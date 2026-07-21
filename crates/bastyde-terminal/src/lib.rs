// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `bastyde-terminal` — an embeddable, fully-accessible terminal-emulator
//! (Console) widget for Bastyde.
//!
//! Bastyde owns the **view** (grid rendering, input encoding, selection,
//! `Role::Terminal` accessibility, theming, lifecycle); the actual VT emulation
//! and the pseudo-terminal are delegated to proven crates behind the
//! [`engine::TerminalEngine`] trait. The default backend
//! ([`alacritty_engine`], enabled by the `alacritty` feature) pairs
//! `portable-pty` (the PTY) with `alacritty_terminal` (the VT model) — nothing
//! in this crate re-implements escape-code handling.
//!
//! Sits at the `bastyde-widgets` tier but depends only on `bastyde-core` /
//! `bastyde-tokens` / `bastyde-canvas`, so apps that don't embed a terminal pay
//! nothing.

mod a11y;
pub mod color_scheme;
pub mod engine;
mod input;
pub mod memory;
mod mouse;
mod render;
mod state;
pub mod style;
mod terminal;

#[cfg(feature = "alacritty")]
mod pty;

#[cfg(feature = "alacritty")]
pub mod alacritty_engine;

pub use color_scheme::{ColorScheme, TermColor};
pub use engine::{
    Cell, CellAttrs, CursorInfo, GridSnapshot, PtyGeom, Scroll, SelectionKind, SelectionSpan,
    SpawnedEngine, TermCursorShape, TermEvent, TermMode, TerminalCommand, TerminalEngine,
    TerminalEngineFactory, TerminalExit,
};
pub use memory::{MemoryEngine, MemoryEngineFactory, MemoryShared};
pub use render::CellMetrics;
pub use style::{RecipeTerminalStyle, TerminalChrome, TerminalStyle};
pub use terminal::{BellStyle, CursorStyle, Terminal, TerminalClosePolicy, TerminalController};

#[cfg(feature = "alacritty")]
pub use alacritty_engine::AlacrittyEngineFactory;
