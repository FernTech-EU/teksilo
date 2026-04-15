//! Rich text editor widget. Feature-gated behind the `rich-text` feature.
//!
//! See [`§27.10` of the architecture doc](../../../../../docs/fern-ui-architecture.md)
//! for the design rationale. This crate ships `RichTextEditor` with two
//! construction presets — M8a provides [`RichTextEditor::read_only`]
//! (view documents, select/copy, click links). M8b will add
//! [`RichTextEditor::editor`] (full editing).
//!
//! The widget owns its own `fern_text::RichTextEngine` (per-widget
//! typesetter — see gap 5 of the plan), subscribes to document events
//! via `on_change` so multiple editors can share a `TextDocument` like
//! QTextEdit views, and drives its own scroll bars outside of
//! `ScrollArea` to break the wrap/scrollbar circular dependency of
//! §27.10.5.

mod clipboard;
mod frame_loop;
mod hit_test;
mod image_cache;
mod keyboard;
mod mouse;
mod paint;
mod policy;
mod state;
mod widget;

#[cfg(test)]
mod tests;

pub use hit_test::ContextTarget;
pub use policy::{
    AccessibilityRole, CaretPolicy, ClipboardPolicy, CommandFilter, EditCommandKind,
    PolicyBundle, EDITOR_PRESET, READ_ONLY_PRESET,
};
pub use widget::{RichTextEditor, ScrollPolicy};
