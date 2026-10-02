<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Status and limitations

<!-- BEGIN README: Status -->
## Status

Expect breaking changes between 0.x versions.

The test suite is roughly 9,400 tests in teksilo and over 12,000 across the whole stack. Tests target behavior (event dispatch, layout output, accessibility-tree structure), not implementation snapshots. The same widget tree runs under tests without a window, a GPU, or winit, and a simulated clock makes time-dependent behavior deterministic.

Teksilo builds on two earlier MPL-2.0 crates already at v1.x: [text-document](https://github.com/ferntech-eu/text-document) (rich-text document model) and [text-typeset](https://github.com/ferntech-eu/text-typeset) (typesetting engine).

Known gaps are listed below.

Project. Architecture, design reviews, code review and final acceptance were human; code generation and routine refactoring were LLM-assisted (Claude Opus and Mistral Medium) under that review.

**Scale:** 40+ framework crates · 700k+ lines of Rust · 100+ widgets · 1600+ builder methods.
<!-- END README: Status -->

<!-- BEGIN README: Known gaps -->
## Known gaps

- **CJK IME composition.** Latin and BiDi input compose correctly; Chinese, Japanese, and Korean input methods need to be tested by actual users.
- **X11 verification breadth.** The X11 custom title bar and drag-and-drop backends ship and are covered by protocol tests, but live verification has been done against KWin (via XWayland) and, in CI, Openbox. Other window managers are untested, and there is no run against a standalone Xorg server. A window manager without `_NET_WM_MOVERESIZE` is detected up front and keeps native decorations rather than producing an immovable window.
- **Mobile and web.** Linux, Windows and macOS are the primary targets. No mobile or web targets.
- **API stability.** Pre-1.0; breaking changes are expected between minor versions.
<!-- END README: Known gaps -->

## WebView prototype

The wry backend is functional. On Linux it requires WebKitGTK and uses XWayland
in a Wayland session. The Servo backend is work in progress and is not yet
frame-driven. See [WebView](web-view.md) for backend requirements and restrictions.

## Accessibility verification

Toolkit support does not establish application conformance. Review the
[accessibility issues](a11y/a11y_issues.md), then test your application's
keyboard and assistive-technology workflows.
