<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Roadmap

Updated 2 October 2026.

## Toward V1

The **V1 API freeze is targeted for January 2027**. The remaining work focuses
on live image display, hardware validation, and feedback from applications
beyond the maintainer's own use cases. These tracks can proceed in parallel.

| Status | Goal | Remaining work |
| --- | --- | --- |
| Planned | LiveImage | Add a widget displaying frames produced by another thread for video, VM screens, and camera previews, while keeping the UI responsive. |
| Pending validation | Touchscreen and stylus hardware | Exercise touch and pen input on real hardware, record device and platform results, and fix discovered defects. |
| Ongoing | Wider real-world use | Reproduce and fix bugs reported by other users, add regression coverage, and resolve API friction before the freeze. |
| Target: January 2027 | Public API freeze | Complete intended breaking changes, align documentation and examples, and establish the compatibility baseline for 1.x. |

Breaking changes remain possible between 0.x releases. The V1 release date is
not yet set. Bug reports are welcome through the
[issue tracker](https://github.com/ferntech-eu/teksilo/issues), with a
reproduction, Teksilo version, platform, and expected behavior where possible.

## Delivered

These groups summarize the Git history by theme through 2 October 2026.
Periods overlap because the areas developed together; improvements continue
in each area. All dates below are in 2026.

| Theme | Main achievements | Period |
| --- | --- | --- |
| Framework foundations | Retained widget tree, reactive state, layout, GPU rendering, focus, events, scrolling, and animation. Later work improved API consistency and GPU/backend fallbacks. | March to April; refinements through September |
| Desktop widgets and application integration | Controls, menus, dialogs, tabs, pickers, actions and shortcuts, multiple windows, settings, async execution, native desktop integration, and OS drag-and-drop. | April to July |
| Rich text and internationalization | Rich-text editing, clipboard interchange, IME integration, bidirectional caret movement, search, inline images, translation checks, reactive language changes, and locale-aware formatting. | April to August |
| Styling and themes | Reactive themes, semantic colors, configurable paint recipes, and widget style protocols. Material 3, Fluent, and macOS presets alongside Int UI. | April to August |
| Data views, docking, and graphics | Virtualized data views, keyed selection, pluggable sources, editable tables, docking, charts, and a scene canvas with selection, snapping, transforms, and ink. Specialized surfaces include code editing, terminal, and log viewing. | April to July; scene extensions in September |
| Developer tooling and automation | Widget previewer, runtime inspector, DSL formatter and language server, screenshots, cross-platform MCP automation, and cargo teksilo. Documentation and onboarding consolidated in September and October. | April to October |
| Accessibility and input | Keyboard navigation, focus scopes, contrast checks, accessible text and actions, and screen-reader corrections. Unified touch, pen, and trackpad support shipped in 0.10.0; physical hardware validation remains pending. | Foundations in April; major extensions June to September |

Teksilo began on 31 March as FernUI, became Bastyde in May, and adopted the
Teksilo name on 8 August. See [status and limitations](status-and-limitations.md)
for current restrictions, including the prototype web view and verification gaps.

## Release history

As of 2 October 2026, the latest tagged release is **0.14.2**, dated
28 September. See the
[changelog](https://github.com/ferntech-eu/teksilo/blob/main/CHANGELOG.md)
for individual versions and detailed changes. The
[Git history](https://github.com/ferntech-eu/teksilo/commits/main/)
also covers development before the changelog was introduced.
