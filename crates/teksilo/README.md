<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Teksilo

A pure-Rust, batteries-included GUI framework for desktop applications.
Accessibility, internationalization, rich text, themes, persistent settings,
drag-and-drop, charts, and a scene canvas all ship as first-class citizens.

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

State is reactive through `Signal<T>` and `Prop<T>`; a derived signal
(`count.map(|n| format!("Count: {n}"))`) re-renders only what depends on it.

## What you get

- **Widgets** — over a hundred, from `Button` to `TreeTableView`, `CodeEditor`,
  `Terminal` and a dockable IDE shell.
- **Accessibility** — AccessKit at the trait level, plus `.access_*` overrides
  on any widget. Not an afterthought layer.
- **Layout** — SwiftUI-style negotiation with grow *and* shrink weights, and
  correct height-for-width.
- **Theming** — tokens → variants → recipes → style protocols, with Int-UI,
  Material 3, Fluent and macOS Aqua presets.
- **i18n** — Fluent messages with compile-time key checking, plus ICU-backed
  number and date formatting.
- **Headless tests** — the same widget tree runs with no window, no GPU and no
  winit; a simulated clock makes time-dependent behaviour deterministic.

## Feature flags

`widgets`, `text`, `i18n`, `inspector`, `toast`, `file-dialog` and `clipboard`
are on by default. Opt in to `theme-material3`, `theme-fluent`, `theme-macos`,
`terminal`, `web-view`, `async` (with `tokio` or `async-std`), `automation`,
`telemetry`, and the bundled script fonts (`fonts-*`).

## Status

Pre-1.0: expect breaking changes between 0.x releases. Production deployment is
currently limited to FernTech's own applications, which is what the 0.x label
reflects. Teksilo builds on two MPL-2.0 crates already at 1.x:
[text-document](https://github.com/ferntech-eu/text-document) and
[text-typeset](https://github.com/ferntech-eu/text-typeset).

## Documentation

Guides, the widget catalog and the API reference live at
<https://github.com/ferntech-eu/teksilo>, which also documents the authorship
and review rules the project is built under.

## License

Mozilla Public License 2.0. Teksilo can be used in commercial and closed-source
software; modifications to the Teksilo files themselves must be shared under
MPL-2.0 if distributed, while application code that merely uses Teksilo is under
its own license.
