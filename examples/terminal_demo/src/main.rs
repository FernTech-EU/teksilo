// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `terminal-demo` — showcase for the embeddable [`Terminal`] (Console) widget.
//!
//! A real shell runs over a pseudo-terminal (ConPTY on Windows, openpty on
//! Unix); the VT emulation is `alacritty_terminal`, the view is Bastyde's. Try
//! `ls --color`, `vim`, `htop`, `tmux`; `Ctrl+C` interrupts (proving the
//! terminal owns keyboard input), `Shift+PageUp`/`Shift+PageDown` and the wheel
//! scroll the scrollback, `Ctrl+Shift+C` / `Ctrl+Shift+V` (⌘C / ⌘V on macOS)
//! copy and paste, and the toolbar drives the terminal through its controller.
//!
//! Run: `cargo run -p terminal-demo`
//!
//! Click the terminal to give it keyboard focus.

use bastyde::prelude::*;
use bastyde::terminal::{BellStyle, CursorStyle, Terminal};
use bastyde::widgets::{Button, Divider, HStack, Spacer, TextWidget, VStack};

fn main() {
    BastydeAppBuilder::new()
        .install_inspector_in_debug()
        .theme(intui::dark())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Terminal")
                .size(920, 620)
                .root(|tree, _state| {
                    let terminal = Terminal::new()
                        .label("Demo shell")
                        .scrollback_lines(5000)
                        .cursor_shape(CursorStyle::Beam)
                        .bell(BellStyle::Visual)
                        .on_title_changed(|title| println!("[title] {title}"))
                        .on_child_exited(|exit| {
                            println!("[exit] success={} code={:?}", exit.success, exit.code);
                        });
                    let ctrl = terminal.controller();

                    // Reactive status line, fed by the controller's signals.
                    let title = ctrl.title_signal();
                    let running = ctrl.child_running_signal();
                    let status = title.zip(&running).map(|(title, running)| {
                        let dot = if *running {
                            "\u{25cf} running"
                        } else {
                            "\u{25cb} exited"
                        };
                        let title = if title.is_empty() { "shell" } else { title };
                        format!("{dot}   \u{00b7}   {title}")
                    });
                    let dims = ctrl
                        .columns_signal()
                        .zip(&ctrl.rows_signal())
                        .map(|(cols, rows)| format!("{cols}\u{00d7}{rows}"));

                    let clear = ctrl.clone();
                    let reset = ctrl.clone();
                    let bottom = ctrl.clone();
                    let toolbar = HStack::new()
                        .spacing(8.0)
                        .child(Button::new(lit!("Clear")).on_activate_fn(move |_ctx| clear.clear()))
                        .child(Button::new(lit!("Reset")).on_activate_fn(move |_ctx| reset.reset()))
                        .child(
                            Button::new(lit!("Scroll to bottom"))
                                .on_activate_fn(move |_ctx| bottom.scroll_to_bottom()),
                        )
                        .child(Spacer::new())
                        .child(TextWidget::new(lit!("")).text(status))
                        .child(TextWidget::new(lit!("")).text(dims));

                    tree.add(
                        VStack::new()
                            .spacing(6.0)
                            .child(toolbar)
                            .child(Divider::new())
                            .child(terminal),
                    )
                }),
        )
        .run();
}
