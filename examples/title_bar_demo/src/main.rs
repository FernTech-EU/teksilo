// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! M2 demo for the custom `TitleBar` widget.
//!
//! Run with: `cargo run -p title-bar-demo`
//!
//! On Wayland this opens a borderless window with a 40-pixel custom title
//! bar wired to a `WaylandHost`, framed by a 6-pixel ring of resize
//! strips. Behaviors to verify by hand:
//!   - Drag the title bar's middle band to move the window.
//!   - Double-click the title bar to toggle maximize.
//!   - Right-click the title bar to open the system window menu.
//!   - Click the trailing buttons (—, □, ×) to minimize / toggle maximize / close.
//!   - Hover the very edge (6 px) of the window — the cursor should
//!     change to a row / column resize cursor — and drag to resize.
//!
//! On platforms whose host backend is still a stub (Windows, macOS) or
//! that don't support custom chrome at all (X11), `tree.title_bar_host()`
//! returns `None` and the demo falls back to a plain content view with no
//! title bar.

use bastyde::prelude::*;
use bastyde::widgets::{
    Expand, HStack, RectWidget, Spacer, TextWidget, TitleBar, Toolbar, VStack, WindowFrame, ZStack,
};

fn dark_mode_toolbar() -> impl Widget {
    Toolbar::new().child(
        HStack::new()
            .child(Spacer::new())
            .child(bastyde::widgets::ThemeSwitcher::new()),
    )
}

fn main() {
    BastydeAppBuilder::new()
        .install_automation_bridge_in_debug()
        .install_inspector_in_debug()
        .theme(bastyde::presets::intui::dark())
        .initial_window(
            WindowConfig::new()
                .title("Bastyde — Title Bar Demo")
                .size(900, 600)
                .decorations(DecorationsMode::CustomChrome)
                .root(|tree, _state| {
                    let theme = tree.theme().clone();

                    // ----- Title bar + body content (the inner window content) -----
                    let title_bar: Box<dyn Widget> = match tree.title_bar_host() {
                        Some(host) => Box::new(
                            TitleBar::new(host)
                                .height(40.0)
                                // Use *roles*, not frozen `theme.colors.*`
                                // snapshots: roles resolve at paint time, so the
                                // bar retints live when `ctx.set_theme(...)`
                                // swaps light ↔ dark. `SurfaceRole::Pressed` is
                                // clearly lighter than `SurfaceRole::Main`, plus
                                // a 2 px `TextRole::Secondary` border for an
                                // unambiguous separation in either theme.
                                .background(SurfaceRole::Pressed)
                                .border(TextRole::Secondary, 2.0)
                                .leading(
                                    TextWidget::new(lit!("  Bastyde — Title Bar Demo"))
                                        .style(theme.typography.body_bold.clone())
                                        .color(TextRole::Primary),
                                )
                                .center(
                                    TextWidget::new(lit!(
                                        "drag · double-click maximize · right-click for menu  "
                                    ))
                                    .style(theme.typography.small.clone())
                                    .color(TextRole::Secondary),
                                )
                                .close_action(|ctx| ctx.close_window()),
                        ),
                        None => Box::new(
                            TextWidget::new(lit!(
                                "(custom chrome unsupported on this platform — \
            falling back to native decorations)"
                            ))
                            .color(TextRole::Error),
                        ),
                    };

                    let body = ZStack::new()
                        .child(RectWidget::new().background(SurfaceRole::Main))
                        .child(
                            Expand::new().child(
                                TextWidget::new(lit!("body content goes here"))
                                    .style(theme.typography.body.clone())
                                    .color(TextRole::Primary),
                            ),
                        );

                    // Wrap the body in `Expand::fills_stack()` so the inner VStack
                    // sees it as a spacer and gives it all the leftover vertical
                    // space. Without this the body would collapse to its 16-px
                    // text intrinsic height and leave a huge unused area below
                    // the title bar.
                    let body_filling = Expand::new().child(body);

                    let title_bar_id = tree.add_boxed(title_bar);
                    let body_id = tree.add(body_filling);

                    let inner = tree.add(
                        VStack::new()
                            .spacing(0.0)
                            .add_child(title_bar_id)
                            .add_child(body_id),
                    );

                    // Wrap in a 6 px resize frame on the 4 edges *only* when the
                    // active host needs the application to drive edge resize. On
                    // macOS the native NSWindow frame still services edges even
                    // with `titlebarAppearsTransparent + fullSizeContentView`, so
                    // `needs_custom_resize_handles()` returns false and we skip
                    // the overlay — otherwise our strips would fight the OS.
                    //
                    // On platforms where the host is None (X11, unsupported
                    // Unix) the frame can't be created either; we just return
                    // the inner content uncovered.
                    let frame_or_inner = match tree.title_bar_host() {
                        Some(host) if host.needs_custom_resize_handles() => {
                            tree.add(WindowFrame::new(host).thickness(6.0).content_id(inner))
                        }
                        _ => inner,
                    };

                    let toolbar_id = tree.add(dark_mode_toolbar());
                    let expanded_content_id = tree.add(Expand::new().child_id(frame_or_inner));
                    tree.add(
                        VStack::new()
                            .add_child(toolbar_id)
                            .add_child(expanded_content_id),
                    )
                }),
        )
        .run();
}
