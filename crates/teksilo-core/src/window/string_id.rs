// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Why a window's string id could not be changed; see
//! [`WindowOps::set_window_string_id`](super::WindowOps::set_window_string_id).

use super::id::TeksiloWindowId;

/// Why [`EventContext::set_window_string_id`](crate::widget::EventContext::set_window_string_id)
/// left a window's string id as it was.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WindowStringIdError {
    /// Another open window already has this string id. Two windows with
    /// one id would leave [`find_window`](super::WindowOps::find_window)
    /// able to name only one of them, and their saved geometry would be
    /// one row that both overwrite, so the rename is refused. Close the
    /// other window, or rename it first.
    #[error("the window id `{string_id}` is already held by another open window ({holder:?})")]
    HeldByAnotherWindow {
        /// The string id that was asked for.
        string_id: String,
        /// The open window that has it.
        holder: TeksiloWindowId,
    },
    /// No open window has this id: it was never opened, or it has closed.
    #[error("no open window has the id {0:?}")]
    NoSuchWindow(TeksiloWindowId),
    /// The window operations in use cannot rename a window: a standalone
    /// tree with no application behind it, or a custom
    /// [`WindowOps`](super::WindowOps) that does not implement it.
    #[error("these window operations cannot change a window's string id")]
    Unsupported,
}
