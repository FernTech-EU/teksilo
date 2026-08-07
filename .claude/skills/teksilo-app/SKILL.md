---
name: teksilo-app
description: Build or modify a Rust desktop GUI app that DEPENDS ON the `teksilo` crate (FernTech's pure-Rust GUI framework) — questions about teksilo widgets, layout, Signal/Prop reactivity, events, theming, settings, i18n, animations, the `teksu!` DSL, or driving/testing the app via the teksilo-automation MCP (agent/CI automation), or any time a Cargo.toml in scope lists `teksilo` as a dependency. Also handles `/teksilo-app`. SKIP when editing the teksilo framework itself (a workspace defining teksilo-core / teksilo-widgets — use that repo's CLAUDE.md); when working inside the framework repo, the extract-widget-api / teksu-macro skills are better for single-widget API dumps or teksu! questions.
user_invocable: true
---

# teksilo-app

Teksilo is a retained-widget-tree, SwiftUI-layout, signal-reactive, wgpu-rendered
pure-Rust GUI framework. This skill helps you write app code that **compiles against the
exact version the app pins** — not against stale assumptions.

**Slash invocation.** `/teksilo-app` with no context → ask what they're building or which
part they need help with. If they name a widget (`/teksilo-app ComboBox`), go straight to
`scripts/teksilo-api.sh ComboBox` (step 2 below).

## Sources of truth — when they disagree, trust in THIS order

1. **`cargo check`** — the compiler is the territory. A clean check is the definition of
   "this uses the API correctly."
2. **Live API extraction** — `scripts/teksilo-api.sh`, `cargo doc`, or docs.rs, all read
   from the version the app actually pins.
3. **The bundled guide prose** — `reference/teksilo_app_guide.md`. A *map*, not the
   territory: verified against teksilo **0.7**, and it MAY lag the version this app pins.
   Never let it override the compiler or live extraction.

## Workflow

1. **Read** `reference/teksilo_app_guide.md` for the app-author surface (entry point,
   Widget trait, layout, Signal/Prop, events, theming, settings, i18n, catalog, testing).
   Treat it as directional — not authoritative on exact signatures.
2. **Extract the exact API before using a widget** — never invent builder methods:
   - `scripts/teksilo-api.sh Button HStack Dialog` — compact API + docs for THIS app's
     pinned version (works for crates.io / git / path deps; no checkout required).
   - `scripts/teksilo-api.sh --list` — every widget.
   - For full type signatures (generics, trait bounds), or if the script errors, use
     `cargo doc -p teksilo-widgets --no-deps --open`, or docs.rs for the pinned version.
3. **Write** the code.
4. **Compile and fix:** `cargo check -p <app-crate>` (or `--workspace`). If it fails
   twice on the same item, re-extract that widget's API (step 2) before a third attempt —
   your mental model of the API is wrong, not the compiler.

## Version & imports

Use whatever version the app's `Cargo.toml` already pins — do **not** assume a number
(`teksilo` is pre-1.0 and changes fast). The umbrella crate re-exports everything:
`use teksilo::prelude::*;` brings core + app + theme + settings + i18n + geometry, and
`use teksilo::widgets::*;` brings the widget builders — **the prelude does NOT pull the
widget builders in**.

## Driving & testing the app (automation MCP)

Teksilo ships **`teksilo-automation`** + **`teksilo-automation-mcp`** — a Model Context
Protocol server that lets an AI agent / CI harness **observe** (the live accessibility
tree, the full widget/layout tree with bounds + per-widget parameters, and screenshots)
and **drive** (accessibility actions + synthetic pointer/key/IME input) the app
**in-process, with no OS accessibility layer**. Reach for it to author UI tests, reproduce
a bug, debug layout, or let an agent exercise the running app.

- **Headless** (CI / agent-authored tests, every platform): `teksilo-automation-mcp
  --headless` — a self-contained MCP server; no display or GPU daemon needed. (The stock
  binary drives a built-in demo; to headlessly drive *your* app, build a tiny harness over
  `teksilo_automation::execute` — the toolkit is GUI-free.)
- **Live app** (Linux/macOS, debug builds): enable the `automation` feature on the
  `teksilo` dependency and add one builder call —

  ```rust
  TeksiloAppBuilder::new()
      .install_automation_bridge_in_debug()   // debug-only; no-op in release / on Windows
      // … the rest of the chain …
      .run();
  ```

  It prints a socket path + token to stderr; drive it with `teksilo-automation-mcp
  --connect <sock> --token <uuid>`.

On connect the server hands the client a "how to drive this app" briefing plus a JSON
schema per tool, so a capable agent self-guides the **snapshot → find node → act → settle
→ assert** loop. The full tool set (~26), by job:

- **Observe:** `snapshot_tree` / `find_node` / `read_node` — semantics: role, label, value,
  toggled/expanded/**selected**, **`bounds {x,y,width,height}`** (widget size lives here),
  and the `actions` a node supports. `assert_node` (a failed assert returns `isError`).
  `inspect_node` (one widget's full record + `Debug` repr / constructor params). `layout_tree`
  — the **full** widget tree incl. layout primitives the AT tree prunes (`Padding`/`Expand`/
  `FixedSize`), each with its **bounds** (position + size = a widget's full **geometry**);
  the tool for size / overlap / off-screen / clipping questions the semantic tree can't
  answer. `screenshot {node?}`.
- **Drive:** `invoke_action {node, action}` — **`action` is REQUIRED** (`click` / `focus` /
  `expand` / `collapse` / `set_value` / `increment` / `decrement` / `show_context_menu`);
  omitting it errors and changes nothing. Plus shortcuts `set_value` / `focus_node` / `scroll`
  / **`drag_node {to_node | to_x,to_y}`** (drag-and-drop), and raw input `inject_pointer` /
  `inject_key` / `type_text` / `type_ime`.
- **Timing (determinism — prefer over `sleep`):** mutating tools auto-settle, but for timed UI
  (tooltips, debounced reactivity, animations) drive the **simulated** clock: `settle.clock_millis`
  on any mutating call, `advance_clock {millis}`, `settle`, or poll with `wait_for_condition`
  (`node_exists` / `node_value` / `node_gone` / `at_version_at_least`).
- **A11y extras:** `get_overlays` (open menus/popovers/dialogs), `list_live_regions` +
  `pull_announcements {since_seq}` (toast/status text a screen reader speaks — the way to assert
  a **toast** fired), `get_shortcuts`, `list_windows` (multi-window; **every tool** takes an
  optional `window_id`).

Error results carry a stable `code` — branch on it, not just `isError`: `NOT_FOUND` /
`BAD_ARGUMENT` / `UNKNOWN_NAME` are real mistakes; `GPU_UNAVAILABLE` (screenshot, no GPU) /
`SETTLE_TIMEOUT` (poll/animation budget) are benign/environmental. Node ids are stable for a
widget's **lifetime** (across relayout / theme / locale), but a **structural rebuild** (data-model
change, `Switcher` swap, `Rebuild`-level binding) allocates a new id — **re-`find_node` after the
tree structure changes**, never reuse a cached id. Full reference: `docs/automation-mcp.md` in the
framework repo.

## High-leverage gotchas (verified against 0.7 source — re-verify via step 2 if newer)

- **No `Theme::default()`** — pick a preset: `intui::light()` / `intui::dark()`.
- **Charts and Scene are separate crates** (`teksilo-charts`, `teksilo-scene`) NOT
  re-exported by the umbrella — add them as direct dependencies.
- **`AppIntent::from_intent(i)` returns `Option<&Self>`** (a borrow). `.cloned()` works
  only if the enum derives `Clone`; otherwise destructure the reference in place.
- **`ctx.set_locale(...)` takes `impl Into<String>`** — pass `"fr-FR"`, not a parsed
  `LanguageIdentifier`.
- **Prefer roles over `Color`** so the UI follows the theme — `SurfaceRole`/`TextRole`/
  `BorderRole` and their typed `Signal<…>` forms (there is no generic `Signal<Role>`).
- **Data models = the whole `teksilo::data` layer, not just `ListView`.** A dynamic
  list/tree/table is a data-driven widget bound to a *model you own* (`ListModel`/`TreeModel`)
  or a *`ListDataSource`/`TreeDataSource` you implement over your domain* — never a hand-rolled
  `for`-loop of children. Decide that ownership shape first (see the guide's *Reactive data
  models* section). Doc caveat: `docs/data-models.md` §3's `ListDataSource` snippet is stale
  (omits `type Key: ItemKey`); trust `docs/data-source.md` + the source.
- **Composing-widget invariant:** the id from `build()`, the root id used by
  `layout_response`, and `children()` must all be the same root child.
- **Testing is headless:** `teksilo::core::{WidgetTree, LayoutContext::for_testing}` +
  `teksilo::canvas::MockTextBackend` — `MockTextBackend` is under `canvas`, not `core`.
  (For agent/CI-driven testing of the running app, see *Driving & testing the app* above.)

## Maintenance (skill owner only)

`reference/teksilo_app_guide.md` and `scripts/extract_widget_api.py` are **snapshots**.
After framework changes, refresh: `cp <teksilo-repo>/tools/extract_widget_api.py
scripts/` and re-verify the guide against source. The `teksilo-api.sh` *extraction* is
always version-matched (it reads the pinned source); only the prose and the bundled
extractor's parser logic are frozen at copy time.
