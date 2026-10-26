// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Where a window's state says it is, against where the window system put
//! it, on macOS and Windows, where a window can be created on the main thread
//! only (hence `harness = false`). Elsewhere it does nothing: on X11 the
//! window manager places a window after it is created and reports it with
//! `Moved`, and the winit loop test covers that.
//!
//! - A window opened with no position, never moved, holds the position the
//!   window system gave it (macOS centres it, Windows cascades it), not the
//!   config's default (0, 0), which its saved geometry would record.
//! - A window opened at a position has its frame there, as `Moved` and the
//!   saved geometry mean it: on macOS winit places the content's corner, a
//!   title bar lower, so a restored window opened higher each time.

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn main() {
    use std::cell::RefCell;
    use std::rc::Rc;

    use teksilo_app::TeksiloAppBuilder;
    use teksilo_canvas::SizeProposal;
    use teksilo_core::build_context::BuildContext;
    use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
    use teksilo_core::{WidgetId, WindowConfig};

    /// What the probe saw, read once the loop has ended.
    #[derive(Debug, Default)]
    struct Seen {
        unplaced: Option<(i32, i32)>,
        placed: Option<(i32, i32)>,
    }

    #[derive(Debug)]
    struct Probe(Rc<RefCell<Seen>>);

    impl Widget for Probe {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let seen = self.0.clone();
            ctx.run_after_mount(move |ctx| {
                let unplaced = ctx.window().map(|state| state.position().get());
                let id = ctx.open_window(
                    WindowConfig::new()
                        .title("teksilo test: placed by the application")
                        .size(240, 160)
                        .position(PLACED.0, PLACED.1),
                );
                let placed = ctx.window_state(id).map(|state| state.position().get());
                *seen.borrow_mut() = Seen { unplaced, placed };
                ctx.quit_forced();
            });
            Vec::new()
        }

        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
    }

    /// Whole logical points at the usual scales (1, 1.25, 1.5, 2).
    const PLACED: (i32, i32) = (240, 180);

    let seen = Rc::new(RefCell::new(Seen::default()));
    let probe = seen.clone();
    TeksiloAppBuilder::new()
        .theme(teksilo_core::presets::intui::light())
        .initial_window(
            WindowConfig::new()
                .title("teksilo test: placed by the window system")
                .size(240, 160)
                .root(move |tree, _state| tree.add(Probe(probe.clone()))),
        )
        .run();

    let seen = seen.borrow();
    let unplaced = seen.unplaced.expect("the probe ran");
    assert_ne!(
        unplaced,
        (0, 0),
        "a window opened with no position holds where the window system put it"
    );
    assert_eq!(
        seen.placed,
        Some(PLACED),
        "a window opened at a position has its frame there"
    );
    println!("os_window_position: ok ({seen:?})");
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() {
    println!("os_window_position: macOS and Windows only");
}
