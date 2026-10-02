<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Teksilo

Teksilo is a pure-Rust GUI framework for desktop applications. It combines a
retained widget tree, reactive state, rich text, accessibility, internationalization,
and GPU rendering.

[Website](https://teksilo.rs) | [Source repository](https://github.com/ferntech-eu/teksilo)

Linux, Windows, and macOS are the primary targets. The API is pre-1.0.
Read [status and limitations](status-and-limitations.md) before adopting it.

## Start here

- [Your first application](first-application.md): install Teksilo, open a window, and add reactive state.
- [Features](features.md): widgets, text, layout, themes, data, and platform integration.
- [Examples and tooling](examples-and-tooling.md): run the catalog and configure `cargo teksilo`.
- [Guide index](guide-index.md): subsystem guides and API references.
- [Contributing and project information](project-information.md): development setup, review policy, licensing, and support.

<!-- BEGIN README: Who is this for -->
## Who is this for

Built primarily for professional desktop applications (writing tools, IDEs, dispatcher consoles, admin panels) where users spend hours and expect full keyboard navigation, screen-reader support, and locale-aware formatting. Small tools and one-off utilities are equally well served: the batteries-included surface means a "window with a list and a few buttons" needs little more than the [first application example](first-application.md).

Default styles are inspired by JetBrains' Int UI, with a light and dark theme that meet WCAG 2.1 AA contrast out of the box. No Win95-style "classic" theme is provided; the framework is intended for modern desktop applications.

Particularly relevant to projects with regulatory accessibility or internationalization requirements (EU Accessibility Act, US Section 508, France RGAA, government procurement, regulated industries such as healthcare and finance). Accessibility and localization are architectural, not retrofitted: a real AccessKit bridge binds on every window on Linux, Windows, and macOS; every widget declares its role, name, and value at the trait level, with a per-widget override surface for labels, descriptions, and relationships; and Fluent-backed translations are checked at compile time. The default light and dark themes meet WCAG 2.1 AA contrast out of the box, enforced by a CI gate; an opt-in high-contrast variant follows the OS "increase contrast" setting, re-queried on window focus; and keyboard alternatives cover the primary drag interactions. Conformance obligations attach to your application, not the toolkit: Teksilo's role is to supply correct primitives and stay out of the way.

Also useful as a shelf of ready-to-use widgets if you're shopping the Rust GUI ecosystem for a specific component (rich text editor, table view, tree view, scene canvas, calendar, color picker) to drop into your app.
<!-- END README: Who is this for -->

<!-- BEGIN README: Design priorities -->
## Design priorities

Teksilo is a retained-tree framework inspired by Qt and the SwiftUI layouting.

- **Composition with painting layered on top.** The `Widget` trait offers both `build()` (compose children) and `paint()` (draw chrome); both methods are optional, and a single widget can do both. Most of the widgets are pure compositions of primitives (`RectWidget`, `TextWidget`, `HStack`, `Padding`); `Card`, `Panel`, overlays, and custom chrome layer paint on top of their composed children.
- **Accessibility at the trait level.** Every widget declares its role and name in an `accessibility()` method that sits beside `layout_response` and `paint`. The AT tree is built alongside the widget tree, not reconstructed from it.
- **Compile-time-checked i18n.** The `tr!` macro reads `.ftl` files at proc-macro expansion time and rejects missing keys, missing arguments, and unknown arguments at build time.
- **Rich-text stack as foundation.** The document model, shaping, BiDi, color emoji, and undo/redo are the foundation, not a widget on top. Even the plain `TextWidget` routes through it.
- **Two API surfaces, one semantics.** A fluent builder API for everything, plus an optional `teksu!` macro for SwiftUI-style declarative syntax. The macro desugars one-to-one to builder calls, so you can mix both in one file.
<!-- END README: Design priorities -->

<!-- BEGIN README: Real-world example -->
## Real-world example

Skribisto, a rich-text writing tool built with Teksilo, was ported from C++/Qt to Rust/Teksilo. It is available [here](https://github.com/jacquetc/skribisto/).

A widget catalog example is available if you run `cargo run -p widget-catalog` in the Teksilo repo. It shows most of the widgets. Dozens of runnable examples are available in the `examples/` directory of the Teksilo repo.
<!-- END README: Real-world example -->
