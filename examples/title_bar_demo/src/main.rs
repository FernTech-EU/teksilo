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

use fern_ui::prelude::*;
use fern_ui::widgets::{
    Expand, RectWidget, TextWidget, TitleBar, VStack, WindowFrame, ZStack,
};

fn main() {
    FernAppBuilder::new()
        .theme(Theme::dark_default())
        .window_title("FernUI — Title Bar Demo")
        .window_size(900, 600)
        .custom_chrome(true)
        .root(|tree| {
            let theme = tree.theme().clone();

            // ----- Title bar + body content (the inner window content) -----
            let title_bar: Box<dyn Widget> = match tree.title_bar_host() {
                Some(host) => Box::new(
                    TitleBar::new(host)
                        .height(40.0)
                        // surface_pressed (#43454A) is clearly lighter
                        // than surface_main (#2B2D30) in the dark default
                        // theme. Plus a 2 px border in text_secondary so
                        // the separation is unambiguous regardless of
                        // theme.
                        .background(theme.colors.surface_pressed)
                        .border(theme.colors.text_secondary, 2.0)
                        .leading(
                            TextWidget::new_literal("  FernUI — Title Bar Demo")
                                .style(theme.typography.body_bold.clone())
                                .color(theme.colors.text_primary),
                        )
                        .center(
                            TextWidget::new_literal(
                                "drag · double-click maximize · right-click for menu  ",
                            )
                            .style(theme.typography.small.clone())
                            .color(theme.colors.text_secondary),
                        )
                        .close_action(|ctx| ctx.close_window()),
                ),
                None => Box::new(
                    TextWidget::new_literal(
                        "(custom chrome unsupported on this platform — \
                         falling back to native decorations)",
                    )
                    .color(theme.colors.text_error),
                ),
            };

            let body = ZStack::new()
                .child(RectWidget::new().background(theme.colors.surface_main))
                .child(
                    Expand::new().child(
                        TextWidget::new_literal("body content goes here")
                            .style(theme.typography.body.clone())
                            .color(theme.colors.text_primary),
                    ),
                );

            // Wrap the body in `Expand::fills_stack()` so the inner VStack
            // sees it as a spacer and gives it all the leftover vertical
            // space. Without this the body would collapse to its 16-px
            // text intrinsic height and leave a huge unused area below
            // the title bar.
            let body_filling = Expand::new().fills_stack().child(body);

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
            match tree.title_bar_host() {
                Some(host) if host.needs_custom_resize_handles() => {
                    tree.add(WindowFrame::new(host).thickness(6.0).content_id(inner))
                }
                _ => inner,
            }
        })
        .run();
}
