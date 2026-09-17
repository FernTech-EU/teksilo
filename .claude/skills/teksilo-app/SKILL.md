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
   territory: verified against teksilo **0.12**, and it MAY lag the version this app pins.
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
- **Live app** (Linux / macOS / Windows, debug builds): enable the `automation` feature on
  the `teksilo` dependency and add one builder call —

  ```rust
  TeksiloAppBuilder::new()
      .install_automation_bridge_in_debug()   // debug-only; a no-op in release
      // … the rest of the chain …
      .run();
  ```

  On startup it binds a private endpoint (Unix socket / Windows named pipe) and publishes an
  owner-only **endpoint descriptor** — `<runtime dir>/teksilo-automation/<pid>.json`, carrying
  the token. Attach with `teksilo-automation-mcp --attach` (the newest live app),
  `--attach-pid <pid>`, or `--list` to see what's live — nothing to scrape out of stderr.
  `--connect <endpoint> --token <uuid>` names one by hand.

On connect the server hands the client a "how to drive this app" briefing plus a JSON
schema per tool, so a capable agent self-guides the **snapshot → find node → act → settle
→ assert** loop. The full tool set (34), by job:

- **Observe:** `snapshot_tree` / `find_node` / `read_node` — semantics: role, label, value,
  toggled/expanded/**selected**, **`bounds {x,y,width,height}`** (widget size lives here),
  and the `actions` a node supports. `assert_node` (a failed assert returns `isError`).
  `inspect_node` (one widget's full record + `Debug` repr / constructor params). `layout_tree`
  — the **full** widget tree incl. layout primitives the AT tree prunes (`Padding`/`Expand`/
  `FixedSize`), each with its **bounds** (position + size = a widget's full **geometry**);
  the tool for size / overlap / off-screen / clipping questions the semantic tree can't
  answer. `screenshot {node?}` — its pixels are **physical** and come with a
  `{width, height, scale}` block; every other coordinate here (node `bounds`,
  `inject_pointer`) is **logical**, so divide by `scale` to click what you can see (headless
  is always `1.0`).
- **Drive:** `invoke_action {node, action}` — **`action` is REQUIRED** (`click` / `focus` /
  `expand` / `collapse` / `set_value` / `increment` / `decrement` / `show_context_menu`);
  omitting it errors and changes nothing. Plus shortcuts `set_value` / `focus_node` / `scroll`
  / **`drag_node {to_node | to_x,to_y}`** (drag-and-drop) / `right_click {node}` (open a
  context menu), and raw input `inject_pointer` / `inject_key` / `type_text` / `type_ime`.
  For an accelerator chord pass **`command: true`**, not `ctrl` — `command` is the platform's
  primary accelerator (Control on Windows/Linux, ⌘ on macOS), and a shortcut *declared*
  `Ctrl+S` resolves to ⌘S there, so `ctrl` injects a key that matches no binding **and still
  reports success**. Keep `ctrl` for chords that really are Control everywhere (Ctrl+Tab).
  `inject_pointer` takes `kind` (`mouse` default / `touch` / `pen`) — the kind reaches the
  hit test, the slop, the hover rules and the arbitration, so a touch is a real touch, not a
  mouse at a point. `pen` carries `pressure` (0..1) and `tilt` ([x, y] degrees). Unknown
  fields are refused rather than defaulted (including `pressure` on a mouse).
- **Touch, pen & multi-pointer:** `inject_touch_sequence` drives a whole multi-touch gesture
  in one call — steps naming a finger **slot**, a phase (`down`/`move`/`up`/`cancel`), a
  point, and how many simulated ms to advance first — and reports the **arbitration after
  every step**: the frozen `touch_action`, every competitor with its role and state, and the
  winner. Identities are minted by the framework, so the reply says which id each slot got.
  A sequence that stops short of its `up` leaves the finger down, which is how a live
  arbitration stays observable. Gesture shorthands: `pinch` (two fingers, span to span — both
  land before either moves, because the recogniser's reference span is the distance between
  the landings), `fling` (one finger released **while still moving**, handing a velocity to
  the scroller — a drag latches on distance, so the duration is the whole difference),
  `long_press` (holds exactly the device's threshold, read off the active input profile, so
  the script needn't know the number). `query_pointers` lists every live pointer with id,
  kind, position, pressure/tilt, capture, frozen `touch_action`, competitors and winner — the
  way to learn the id of a contact left down by anything but `inject_touch_sequence`.
  `cancel_pointer {id}` revokes a pointer the way the system does (a compositor grab, a lost
  capture): **not** an up — no tap completes and every widget working on it is told.
  `set_density {compact | comfortable | touch}` switches the target density; **it rebuilds
  every widget, so every node id captured before it is dead** — re-snapshot afterwards
  (setting the density it already has is a no-op and keeps the ids).
- **Timing (determinism — prefer over `sleep`):** mutating tools auto-settle, but for timed UI
  (tooltips, debounced reactivity, animations) drive the **simulated** clock: `settle.clock_millis`
  on any mutating call, `advance_clock {millis}`, `settle`, or poll with `wait_for_condition`
  (`node_exists` / `node_value` / `node_gone` / `at_version_at_least`).
- **A11y extras:** `get_overlays` (open menus/popovers/dialogs), `list_live_regions` +
  `pull_announcements {since_seq}` (toast/status text a screen reader speaks — the way to assert
  a **toast** fired), `get_shortcuts`, `list_windows` (multi-window; **every tool** takes an
  optional `window_id`).

Error results carry a stable `code` — branch on it, not just `isError`: `NOT_FOUND` /
`BAD_ARGUMENT` / `UNKNOWN_NAME` / `UNHANDLED_ACTION` (the node is real but nothing acted on the
action — it advertises no such action, or its handler declined; the message names the ones it
*does* advertise, so read those and re-call) are real mistakes; `GPU_UNAVAILABLE` (screenshot,
no GPU) / `WAIT_TIMEOUT` (a `wait_for_condition` budget) / `SETTLE_TIMEOUT` (animation budget) /
`BRIDGE_TIMEOUT` (a live app's main thread is in a native modal loop — the op may still land,
so re-read the tree) are benign/environmental. Node ids are stable for a
widget's **lifetime** (across relayout / theme / locale), but a **structural rebuild** (data-model
change, `Switcher` swap, `Rebuild`-level binding) allocates a new id — **re-`find_node` after the
tree structure changes**, never reuse a cached id. Full reference: `docs/automation-mcp.md` in the
framework repo.

## High-leverage gotchas (verified against 0.12 source — re-verify via step 2 if newer)

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
- **A value-bound control now has a callback, and it is not the signal.** `Checkbox`,
  `Toggle`, `RadioButton` and `Slider` take `.on_change(|value, ctx| …)` — `bool`, `bool`,
  `usize` (the index) and `f32` respectively, each with an `&mut EventContext`. Use it only
  when the change has to reach the ambient context (`ctx.send_intent`, `ctx.set_theme`,
  open a window); the bound `Signal` is still the state and still the notification. It fires
  on **user activation only**, never on a programmatic write to the signal — observe the
  signal for that direction. A radio reports a *real* change (re-activating the selected
  button says nothing); a slider is continuous through a drag and has no commit-on-release.
- **Slot methods: id-taking is common, not universal, and the name doesn't tell you.** Most
  structural slots (`child`, `content`, `header`, `pane`, `tab`) take `impl IntoTeksiChild`,
  so a `WidgetId` or a widget both work, and the old `*_id` twins are gone (including
  `TabWidget::tab_id` and `ToolBox::item_id`). But 65 slot declarations still take
  `impl Widget + 'static` and reject an id — including **`PopoverWidget::content`**, which is
  a named slot on a widget you'd expect to take one. Also `TextInput::leading_slot`/
  `trailing_slot`, every `StandardListItem`/`StandardTreeItem` slot, `MenuList::item`/`header`,
  `Banner::action`, `Cycle::child`, `RadioGroup::child`, the `composite_tooltip` family.
  Extract the signature (step 2) rather than assuming.
- **Renames since 0.9** (a stale call site fails to resolve, so the compiler will tell you —
  but these are the ones to expect): `Checkbox::labels_hidden(bool)` → `labelled_externally()`
  (no argument, and `Toggle` already spelled it that way); `StandardTreeItem::on_toggle` /
  `on_toggle_rc` → `on_chevron_toggle` / `on_chevron_toggle_rc` (the chevron, not the
  checkbox — `on_checkbox_toggle` is the box); the `.dim_when_inactive(..)` builder method is
  gone, wrap the subtree in the `DimWhenInactive` widget instead.

## Maintenance (skill owner only)

`reference/teksilo_app_guide.md` and `scripts/extract_widget_api.py` are **snapshots**.
After framework changes, refresh: `cp <teksilo-repo>/tools/extract_widget_api.py
scripts/` and re-verify the guide against source. The `teksilo-api.sh` *extraction* is
always version-matched (it reads the pinned source); only the prose and the bundled
extractor's parser logic are frozen at copy time.
