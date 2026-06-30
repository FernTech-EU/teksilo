---
name: bastyde-app
description: Build or modify a Rust desktop GUI app that DEPENDS ON the `bastyde` crate (FernTech's pure-Rust GUI framework) — questions about bastyde widgets, layout, Signal/Prop reactivity, events, theming, settings, i18n, animations, or the `bati!` DSL, or any time a Cargo.toml in scope lists `bastyde` as a dependency. Also handles `/bastyde-app`. SKIP when editing the bastyde framework itself (a workspace defining bastyde-core / bastyde-widgets — use that repo's CLAUDE.md); when working inside the framework repo, the extract-widget-api / bati-macro skills are better for single-widget API dumps or bati! questions.
user_invocable: true
---

# bastyde-app

Bastyde is a retained-widget-tree, SwiftUI-layout, signal-reactive, wgpu-rendered
pure-Rust GUI framework. This skill helps you write app code that **compiles against the
exact version the app pins** — not against stale assumptions.

**Slash invocation.** `/bastyde-app` with no context → ask what they're building or which
part they need help with. If they name a widget (`/bastyde-app ComboBox`), go straight to
`scripts/bastyde-api.sh ComboBox` (step 2 below).

## Sources of truth — when they disagree, trust in THIS order

1. **`cargo check`** — the compiler is the territory. A clean check is the definition of
   "this uses the API correctly."
2. **Live API extraction** — `scripts/bastyde-api.sh`, `cargo doc`, or docs.rs, all read
   from the version the app actually pins.
3. **The bundled guide prose** — `reference/bastyde_app_guide.md`. A *map*, not the
   territory: verified against bastyde **0.7**, and it MAY lag the version this app pins.
   Never let it override the compiler or live extraction.

## Workflow

1. **Read** `reference/bastyde_app_guide.md` for the app-author surface (entry point,
   Widget trait, layout, Signal/Prop, events, theming, settings, i18n, catalog, testing).
   Treat it as directional — not authoritative on exact signatures.
2. **Extract the exact API before using a widget** — never invent builder methods:
   - `scripts/bastyde-api.sh Button HStack Dialog` — compact API + docs for THIS app's
     pinned version (works for crates.io / git / path deps; no checkout required).
   - `scripts/bastyde-api.sh --list` — every widget.
   - For full type signatures (generics, trait bounds), or if the script errors, use
     `cargo doc -p bastyde-widgets --no-deps --open`, or docs.rs for the pinned version.
3. **Write** the code.
4. **Compile and fix:** `cargo check -p <app-crate>` (or `--workspace`). If it fails
   twice on the same item, re-extract that widget's API (step 2) before a third attempt —
   your mental model of the API is wrong, not the compiler.

## Version & imports

Use whatever version the app's `Cargo.toml` already pins — do **not** assume a number
(`bastyde` is pre-1.0 and changes fast). The umbrella crate re-exports everything:
`use bastyde::prelude::*;` brings core + app + theme + settings + i18n + geometry, and
`use bastyde::widgets::*;` brings the widget builders — **the prelude does NOT pull the
widget builders in**.

## High-leverage gotchas (verified against 0.7 source — re-verify via step 2 if newer)

- **No `Theme::default()`** — pick a preset: `intui::light()` / `intui::dark()`.
- **Charts and Scene are separate crates** (`bastyde-charts`, `bastyde-scene`) NOT
  re-exported by the umbrella — add them as direct dependencies.
- **`AppIntent::from_intent(i)` returns `Option<&Self>`** (a borrow). `.cloned()` works
  only if the enum derives `Clone`; otherwise destructure the reference in place.
- **`ctx.set_locale(...)` takes `impl Into<String>`** — pass `"fr-FR"`, not a parsed
  `LanguageIdentifier`.
- **Prefer roles over `Color`** so the UI follows the theme — `SurfaceRole`/`TextRole`/
  `BorderRole` and their typed `Signal<…>` forms (there is no generic `Signal<Role>`).
- **Composing-widget invariant:** the id from `build()`, the root id used by
  `layout_response`, and `children()` must all be the same root child.
- **Testing is headless:** `bastyde::core::{WidgetTree, LayoutContext::for_testing}` +
  `bastyde::canvas::MockTextBackend` — `MockTextBackend` is under `canvas`, not `core`.
- **Agent/CI automation (optional):** an MCP server can observe (accessibility tree +
  screenshots) and drive (AT actions + synthetic pointer/key/IME input) the app in-process,
  no OS accessibility layer. `bastyde-automation-mcp --headless` is a self-contained MCP
  server for deterministic CI / agent-authored tests; to drive a *live* app, enable the
  `automation` feature on the `bastyde` dep and add `.install_automation_bridge_in_debug()`
  to the `BastydeAppBuilder` chain (debug-only Unix socket, Linux/macOS; no-op in release /
  on Windows), then `bastyde-automation-mcp --connect <sock> --token <uuid>` (both printed
  to stderr at startup). Built on the AccessKit tree every widget already declares, so node
  ids are stable. See `docs/automation-mcp.md` in the framework repo.

## Maintenance (skill owner only)

`reference/bastyde_app_guide.md` and `scripts/extract_widget_api.py` are **snapshots**.
After framework changes, refresh: `cp <bastyde-repo>/tools/extract_widget_api.py
scripts/` and re-verify the guide against source. The `bastyde-api.sh` *extraction* is
always version-matched (it reads the pinned source); only the prose and the bundled
extractor's parser logic are frozen at copy time.
