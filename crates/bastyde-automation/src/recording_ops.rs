// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A [`WindowOps`] implementation for the headless server.
//!
//! [`NoopWindowOps`](bastyde_core::NoopWindowOps) *panics* on `open_window`.
//! That is the right behaviour for a standalone test tree, but it is wrong
//! for automation: an AT action (a menu item, a "New window" button) can
//! legitimately call `open_window`, and crashing the whole server on it
//! would be a terrible failure mode. [`RecordingWindowOps`] instead
//! **records a summary of the requested window and returns a synthetic
//! id**, so the action completes and the harness can observe that a window
//! was requested via [`RecordingWindowOps::opened`].

use bastyde_core::{BastydeWindowId, WindowConfig, WindowOps, WindowState};

/// A summary of an `open_window` request. `WindowConfig` is not `Clone`
/// (it holds `FnOnce` root builders), so the recorder keeps only its
/// inspectable scalar fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedWindow {
    /// The requested window title.
    pub title: String,
    /// The requested stable `string_id`, if any.
    pub string_id: Option<String>,
    /// The requested `(width, height)`.
    pub size: (u32, u32),
    /// The synthetic id this recorder returned for the request.
    pub assigned_id: BastydeWindowId,
}

/// A non-panicking `WindowOps` for the headless automation server: it
/// records `open_window` requests and hands back synthetic ids; every
/// other method no-ops (matching `NoopWindowOps`).
#[derive(Debug, Default)]
pub struct RecordingWindowOps {
    next_id: u64,
    /// Every `open_window` request seen, in order.
    pub opened: Vec<RecordedWindow>,
    /// Window ids passed to `focus_window`, in order.
    pub focused: Vec<BastydeWindowId>,
    /// Window ids passed to `close_window_by_id`, in order.
    pub closed: Vec<BastydeWindowId>,
}

impl RecordingWindowOps {
    pub fn new() -> Self {
        Self::default()
    }
}

impl WindowOps for RecordingWindowOps {
    fn open_window(&mut self, config: WindowConfig) -> BastydeWindowId {
        // Synthetic ids start at a high base so they never collide with a
        // real window manager's allocator in a mixed test.
        let id = BastydeWindowId::new(0x1000_0000 + self.next_id);
        self.next_id += 1;
        self.opened.push(RecordedWindow {
            title: config.title.clone(),
            string_id: config.string_id.clone(),
            size: config.size,
            assigned_id: id,
        });
        id
    }

    fn find_window(&self, string_id: &str) -> Option<BastydeWindowId> {
        self.opened
            .iter()
            .find(|w| w.string_id.as_deref() == Some(string_id))
            .map(|w| w.assigned_id)
    }

    fn window_state(&self, _id: BastydeWindowId) -> Option<WindowState> {
        None
    }

    fn windows(&self) -> Vec<WindowState> {
        Vec::new()
    }

    fn focus_window(&mut self, id: BastydeWindowId) {
        self.focused.push(id);
    }

    fn close_window_by_id(&mut self, id: BastydeWindowId) {
        self.closed.push(id);
    }
}
