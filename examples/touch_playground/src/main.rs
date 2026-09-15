// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Touch playground — what the input layer is being handed, and who wins the
//! press.
//!
//! Run with `cargo run -p touch-playground`. It is useful with a mouse and
//! designed for a touchscreen, a stylus or a trackpad; nothing in it needs a
//! touch device to build or to run.
//!
//! Turn the trace on beside it and the two halves corroborate each other:
//!
//! ```text
//! TEKSILO_TRACE_INPUT=all cargo run -p touch-playground
//! ```
//!
//! `docs/touch-verification.md` is the procedure this window exists to serve:
//! it walks the same panels in order, says what to watch, and gives the trace
//! lines each step should produce.
//!
//! # The four panels
//!
//! **The pointer pad and its readout** (top left). Touch, click or hover the
//! pad; every pointer it is handed is reported beside it with its identity,
//! kind, primary flag, pressure, tilt, twist, contact patch, speed, the
//! `TouchAction` frozen for its press, whether the framework holds a press for
//! the pad, and whether the pad still holds the capture. Above the live rows is
//! the token ladder in force — the three gesture profiles side by side, which is
//! the fastest way to see why a finger needs 18 dp to start a drag where a pen
//! needs 2.
//!
//! The readout can only report **pointers the pad receives**. That is not a
//! shortcut: nothing a widget can reach reports the tree's live pointers. See
//! [`pad`] for the whole argument and for what it costs.
//!
//! **The density toggle.** Compact, Comfortable, Touch. Each switch re-projects
//! the theme and rebuilds, so every target grows or shrinks under your finger.
//!
//! **The kinetic tuning panel.** The fling and overscroll constants, editable.
//! Flick a surface in the scenario column, let go, and watch the coast; then
//! halve the friction and do it again.
//!
//! **The arbitration scenarios** (the scrolling column on the right). Five
//! shapes where two or three contenders want the same press, each showing which
//! one got it. One of them is the text-touch surface: tap for a caret, hold for
//! a word and its handles, drag to pan.
//!
//! # What a session is for
//!
//! Three questions, in order of how often they are the real one:
//!
//! 1. *Is the framework seeing my device at all?* — the pad answers. A stylus
//!    that reports no pressure, a touchscreen whose contacts all arrive with the
//!    same identity, a trackpad whose gestures never reach the tree: all visible
//!    in the first second.
//! 2. *Why did my widget not get the press?* — a scenario answers, and the
//!    frozen `TouchAction` on the pad's row says what the press was allowed to
//!    become before anyone competed for it.
//! 3. *Does it feel right?* — the density toggle and the kinetic panel, which is
//!    the only honest way to tune a feel: change one constant and use it.

mod pad;
mod panels;
mod scenarios;
mod state;

use teksilo::core::binding::BindingLevel;
use teksilo::prelude::*;
use teksilo::tokens::{TargetDensity, TextStyleRole};
use teksilo::widgets::primitives::{Expand, MinSize, Padding};
use teksilo::widgets::scroll_area::ScrollArea;
use teksilo::widgets::{HStack, TextWidget, VStack};

use crate::state::PlaygroundState;

/// The playground root.
///
/// Holds the shared state and binds the one signal that forces a rebuild when a
/// token change has to reach a widget that snapshots it — the density and the
/// kinetic constants both.
#[derive(Debug)]
struct Root {
    state: PlaygroundState,
    child: Option<WidgetId>,
}

impl Root {
    fn new(density: TargetDensity) -> Self {
        Self {
            state: PlaygroundState::new(density),
            child: None,
        }
    }
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // The half of a density switch that `ctx.set_theme` cannot do. See
        // `panels`' module docs for why it takes both.
        self.state
            .rebuild
            .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);

        let left = VStack::new()
            .spacing(6.0)
            .child(
                TextWidget::new(lit!("Pointer pad — touch, click or hover"))
                    .style(TextStyleRole::BodyBold),
            )
            .child(MinSize::new(0.0, 180.0).child(pad::PointerPad::new(self.state.clone())))
            .child(ctx.add(pad::PointerReadout::new(self.state.clone())));
        let left = ctx.add(Padding::uniform(10.0).child(ScrollArea::new().child(left)));

        let controls = VStack::new()
            .spacing(4.0)
            .child(panels::density_toggle(ctx, &self.state))
            .child(panels::kinetic_panel(ctx, &self.state));

        let mut column = VStack::new().spacing(4.0).child(
            Padding::uniform(10.0).child(
                TextWidget::new(lit!(format!(
                    "Arbitration scenarios — a hold is {} ms at the shipped tokens",
                    scenarios::long_press_millis()
                )))
                .style(TextStyleRole::BodyBold),
            ),
        );
        for id in scenarios::all(ctx, &self.state) {
            column = column.child(id);
        }
        let right = ctx
            .add(ScrollArea::new().child(VStack::new().spacing(4.0).child(controls).child(column)));

        let root = ctx.add(
            HStack::new()
                .spacing(4.0)
                .child(Expand::new().flex(1.0).child(left))
                .child(Expand::new().flex(1.0).child(right)),
        );
        self.child = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.child
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }
}

/// `--density compact|comfortable|touch` — start at a rung other than Compact,
/// so a tablet session does not begin by rebuilding the whole tree.
fn density_from_args() -> TargetDensity {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = match arg.strip_prefix("--density=") {
            Some(rest) => Some(rest.to_string()),
            None if arg == "--density" => args.next(),
            None => None,
        };
        if let Some(value) = value {
            return match value.trim().to_ascii_lowercase().as_str() {
                "comfortable" => TargetDensity::Comfortable,
                "touch" => TargetDensity::Touch,
                // An unrecognised value must not silently select a different
                // rung; Compact is both the default and the fail-closed answer.
                _ => TargetDensity::Compact,
            };
        }
    }
    TargetDensity::Compact
}

fn main() {
    let density = density_from_args();
    TeksiloAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(teksilo::presets::intui::light().with_density(density))
        .initial_window(
            WindowConfig::new()
                .title("Teksilo — Touch Playground")
                .size(1180, 820)
                .root(move |tree, _state| tree.add(Root::new(density))),
        )
        .run();
}

#[cfg(test)]
mod tests;
