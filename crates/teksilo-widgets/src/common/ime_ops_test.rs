// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A window-operations sink that counts what a text field asks of the
//! platform's input method, for the composition tests of the text widgets.

use teksilo_core::WindowConfig;
use teksilo_core::window::{TeksiloWindowId, WindowState};

/// Counts [`finish_ime_composition`](teksilo_core::WindowOps::finish_ime_composition)
/// calls; every other operation is inert.
#[derive(Default)]
pub(crate) struct ImeOps {
    /// How many times a field told the platform its composition is over.
    pub finished: usize,
}

impl teksilo_core::WindowOps for ImeOps {
    fn open_window(&mut self, _config: WindowConfig) -> TeksiloWindowId {
        TeksiloWindowId::new(1)
    }

    fn find_window(&self, _string_id: &str) -> Option<TeksiloWindowId> {
        None
    }

    fn window_state(&self, _id: TeksiloWindowId) -> Option<WindowState> {
        None
    }

    fn windows(&self) -> Vec<WindowState> {
        Vec::new()
    }

    fn focus_window(&mut self, _id: TeksiloWindowId) {}

    fn close_window_by_id(&mut self, _id: TeksiloWindowId) {}

    fn finish_ime_composition(&mut self) {
        self.finished += 1;
    }
}
