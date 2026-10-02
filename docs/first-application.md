<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Your first application

Use a stable Rust toolchain. Linux builds also need the
[system libraries listed in the development setup](project-information.md#development-environment).

## Create the project

<!-- BEGIN README: install -->
```sh
cargo new my-app
cd my-app
cargo add teksilo
```
<!-- END README: install -->

## Open a window

Replace `src/main.rs` with:

<!-- BEGIN README: hello -->
```rust
use teksilo::prelude::*;
use teksilo::widgets::Button;

fn main() {
    TeksiloAppBuilder::new()
        .theme(intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Hello Teksilo")
                .size(400, 300)
                .root(|tree, _state| {
                    tree.add(
                        Button::new(lit!("Click Me"))
                            .on_activate_fn(|_ctx| println!("Clicked!")),
                    )
                }),
        )
        .run();
}
```
<!-- END README: hello -->

Run the application:

```sh
cargo run
```

The window contains a button. Activating it prints `Clicked!` in the terminal.

## Add reactive state

Replace `src/main.rs` with this counter:

<!-- BEGIN README: counter -->
```rust
use teksilo::prelude::*;
use teksilo::widgets::{Button, TextWidget, VStack};

fn main() {
    TeksiloAppBuilder::new()
        .theme(intui::light())
        .initial_window(
            WindowConfig::new()
                .title("Counter")
                .size(300, 150)
                .root(|tree, _state| {
                    let count = Signal::new(0_i32);
                    let label = count.map(|n| format!("Count: {n}"));
                    tree.add(
                        VStack::new()
                            .spacing(12.0)
                            .child(TextWidget::new(lit!("")).text(label))
                            .child(
                                Button::new(lit!("Increment"))
                                    .on_activate_fn(move |_| {
                                        count.set(count.get() + 1)
                                    }),
                            ),
                    )
                }),
        )
        .run();
}
```
<!-- END README: counter -->

Run `cargo run` again. Each activation increments the count and updates the label.
`Signal` stores the count; `map` derives the label from its current value.

Continue with [examples and tooling](examples-and-tooling.md),
[layout](layout-primitives.md), or [events and gestures](events-and-gestures.md).
