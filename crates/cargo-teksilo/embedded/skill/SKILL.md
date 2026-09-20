---
name: teksilo
description: Build, modify, or debug a Rust desktop GUI app that DEPENDS ON the `teksilo` crate (FernTech's pure-Rust GUI framework) — questions about teksilo widgets, layout, Signal/Prop reactivity, events, theming, settings, i18n, animations, reactive data models, accessibility overrides, the `teksu!` DSL (writing a widget tree in it, translating to or from builder calls, or debugging a `teksu!` compile error), and driving or testing the running app through the automation bridge / MCP server / probe harness. Use any time a Cargo.toml in scope lists `teksilo` as a dependency, or the user types `/teksilo`. SKIP when editing the teksilo framework itself (a workspace that DEFINES teksilo-core / teksilo-widgets — use that repo's CLAUDE.md and its in-repo skills instead).
user_invocable: true
---

# teksilo

Teksilo is a retained-widget-tree, SwiftUI-layout, signal-reactive, wgpu-rendered
pure-Rust GUI framework. This skill helps you write app code that **compiles
against the exact version the app pins** — not against stale assumptions.

**Slash invocation.** `/teksilo` with no context → ask what they are building or
which part they need help with. If they name a type (`/teksilo ComboBox`), go
straight to `cargo teksilo symbol ComboBox`. If they paste a `teksu!` block or
ask about the DSL, read `reference/teksu.md` first.

## Sources of truth — when they disagree, trust in THIS order

1. **`cargo check`** — the compiler is the territory. A clean check is the
   definition of "this uses the API correctly."
2. **Live API extraction** — `cargo teksilo symbol <Name>`, `cargo doc`, or
   docs.rs, all read from the version the app actually pins.
3. **The bundled reference prose** — the files under `reference/`. A *map*, not
   the territory: verified against teksilo **0.12.1**, and it MAY lag the version
   this app pins. Never let it override the compiler or live extraction.

## The tool: `cargo teksilo`

A cargo subcommand that answers for **this app's** teksilo. It resolves the
framework through `cargo metadata` — the resolved dependency graph, never a
parsed manifest — so it works with a crates.io, git or path dependency, with
workspace inheritance, and with no framework checkout anywhere.

Install it at the version your app pins:

```bash
cargo install cargo-teksilo --version <the teksilo version your app pins>
```

**There is a floor: this tool's first release is 0.13.0.** It is newer than the
framework it serves, so an app on teksilo 0.12 or older has no matching tool —
that version of it was never published, and `--version 0.12.1` will not resolve.
Such an app has to move to teksilo 0.13 before `cargo teksilo` can answer for
it; until then, fall back to `cargo doc -p teksilo-widgets --no-deps --open`,
docs.rs at the pinned version, and the compiler — and say the version is
unserved rather than answering from memory.

That version match is load-bearing and enforced: the tool **refuses** to answer
when its minor or major differs from the app's resolved teksilo, and warns when
only the patch differs. Serving 0.12 answers to an app on 0.9 is worse than
serving nothing — `SplitView` was deleted outright in favour of `Splitter` with
no back-compat between them, and a wrong answer reads exactly like a right one.

```bash
cargo teksilo symbol Button HStack Dialog   # exact public API + docs for the pinned version
cargo teksilo symbol ListModel              # data / settings / scene types too
cargo teksilo search "virtualized table with sortable columns"
cargo teksilo search "how do I persist window size"
cargo teksilo show docs/scroll-area.md      # read a search result in full, offline
cargo teksilo show docs/scroll-area.md --lines 166-172
cargo teksilo probe                         # materialise the Python automation harness
cargo teksilo setup                         # the harness, plus a teksilo brief for every agent here
```

- **`symbol`** replaces reading the widget source by hand. It emits the type's
  `//!` module header, its `pub` declarations with their `///` docs, and the
  builder methods from its inherent `impl` blocks — skipping trait plumbing and
  `pub(crate)` items. Names may be a type (`Button`) or a module (`button`).
- **`search`** is hybrid BM25 + semantic retrieval over the framework's
  hand-written guides (69 of them) and worked examples (56 crates). Neither reaches a
  consumer app any other way: the guides live in no crate, and every example is
  `publish = false`. Reach for it whenever the question is conceptual ("which
  data model do I want?", "how does drag-and-drop escalate to the OS?") rather
  than "what is this method's signature?".
- **`show`** prints one of those documents in full — the whole file, or just the
  lines a hit cited (`--lines 166-172`, 1-based and inclusive, exactly as
  `search` prints them). **Read a search result this way, and do not fetch the
  file from GitHub.** The path a hit prints is real in the *framework*
  repository and absent here, so the two tempting moves are both wrong: opening
  it locally finds nothing, and `blob/main/` (or the published book) serves
  `main`, which is a different Teksilo from the one this app pinned — which is
  the exact confusion this tool exists to prevent. `show` is offline and
  version-matched. `cargo teksilo show --list` prints every available path.
- **`probe`** writes a Python automation harness into `scripts/teksilo_probe/`
  for driving and asserting on the running GUI. See `reference/automation.md`.
- **`setup`** is `probe` plus the teksilo briefing for every coding agent this
  project already configures — this skill where Claude Code looks for one, and a
  self-contained condensed brief in `.cursor/rules/`, `.windsurf/rules/`,
  `.github/copilot-instructions.md` or `AGENTS.md` where the format is not a
  skill — plus a one-off fetch of the search encoder. It prints a plan and asks
  first; `-y` skips the question (needed in CI, where a prompt is an error rather
  than a wait), `--no-model` skips the download, and `--user` is the only mode
  that writes `$HOME`. It only writes where a marker already exists, and lists
  what it skipped.

`cargo teksilo --help` and `cargo teksilo <command> --help` are authoritative for
flags; this file names the commands, not their whole flag surface.

**If `cargo teksilo` is not installed and the user does not want it**, fall back
to `cargo doc -p teksilo-widgets --no-deps --open` (or docs.rs at the pinned
version) for signatures, and to the `reference/` files for concepts. Both are
slower and the second is version-approximate — say so rather than guessing.

## Workflow

1. **Orient.** `reference/teksilo_app_guide.md` is the app-author surface — entry
   point, Widget trait, layout, Signal/Prop, events, theming, settings, i18n,
   widget catalog, testing. Treat it as directional, not authoritative on exact
   signatures. For a conceptual question, `cargo teksilo search "<question>"`
   first — it reaches material this skill does not carry — then
   `cargo teksilo show <path>` to read the hit in full rather than working from
   its snippet or fetching it from GitHub.
2. **Extract the exact API before using a type** — never invent builder methods.
   `cargo teksilo symbol <Name>`. For full generics and trait bounds, or if the
   tool is unavailable, use `cargo doc` / docs.rs at the pinned version.
3. **Write** the code. If it is a `teksu!` block, read `reference/teksu.md` first.
4. **Compile and fix:** `cargo check -p <app-crate>` (or `--workspace`). **If it
   fails twice on the same item, re-extract that type's API before a third
   attempt** — your mental model of the API is wrong, not the compiler.
5. **Verify behaviour** where it matters: headless widget tests for layout and
   state logic, a probe for "the user clicks this and that happens on screen"
   (`reference/automation.md`).

## Triage

| The question is about | Do this |
|---|---|
| A specific type's builder methods / signature | `cargo teksilo symbol <Name>` |
| "Which widget / model / approach do I want?" | `cargo teksilo search "<question>"`, then the guide's catalog section |
| Layout, Signal/Prop, events, theming, settings, i18n, a11y, data models | `reference/teksilo_app_guide.md` |
| Writing / translating / debugging a `teksu!` block | `reference/teksu.md` |
| Driving, screenshotting or testing the running app | `reference/automation.md` |
| A compile error you have already tried once | Re-extract the API (step 2) before the next attempt |
| Framework internals, why something is built this way | `cargo teksilo search` — the guides are the only place that answers it |
| Reading a guide or example a search cited | `cargo teksilo show <path>` — **not** GitHub, which tracks `main` rather than the pinned version |

## Version & imports

Use whatever version the app's `Cargo.toml` already pins — do **not** assume a
number (`teksilo` is pre-1.0 and changes fast). The umbrella crate re-exports
everything: `use teksilo::prelude::*;` brings core + app + theme + settings +
i18n + geometry, and `use teksilo::widgets::*;` brings the widget builders —
**the prelude does NOT pull the widget builders in**.

## High-leverage gotchas

Verified against **teksilo 0.12.1** — re-verify with `cargo teksilo symbol` if
the app pins something newer.

- **No `Theme::default()`** — pick a preset: `intui::light()` / `intui::dark()`.
- **Charts and Scene are separate crates** (`teksilo-charts`, `teksilo-scene`),
  NOT re-exported by the umbrella — add them as direct dependencies and import
  from those crate paths.
- **`AppIntent::from_intent(i)` returns `Option<&Self>`** (a borrow). `.cloned()`
  works only if the enum derives `Clone`; otherwise destructure the reference in
  place.
- **`ctx.set_locale(...)` takes `impl Into<String>`** — pass `"fr-FR"`, not a
  parsed `LanguageIdentifier`.
- **Prefer roles over `Color`** so the UI follows the theme —
  `SurfaceRole` / `TextRole` / `BorderRole` and their typed `Signal<…>` forms
  (there is no generic `Signal<Role>`).
- **Data models are the whole `teksilo::data` layer, not just `ListView`.** A
  dynamic list / tree / table is a data-driven widget bound to a *model you own*
  (`ListModel` / `TreeModel`) or a *`ListDataSource` / `TreeDataSource` you
  implement over your domain* — never a hand-rolled `for`-loop of children.
  Decide that ownership shape first; see the guide's *Reactive data models*
  section. Doc caveat as of 0.12.1: the `data-models` guide's §3
  `ListDataSource` snippet is stale (it omits `type Key: ItemKey`) — trust the
  `data-source` guide and the extracted API.
- **Composing-widget invariant:** the id returned from `build()`, the root id
  used by `layout_response`, and `children()` must all be the same root child.
- **Testing is headless:** `teksilo::core::{WidgetTree, LayoutContext::for_testing}`
  plus `teksilo::canvas::MockTextBackend` — `MockTextBackend` is under `canvas`,
  not `core`.
- **A value-bound control has a callback, and it is not the signal.** `Checkbox`,
  `Toggle`, `RadioButton` and `Slider` take `.on_change(|value, ctx| …)` —
  `bool`, `bool`, `usize` (the index) and `f32` respectively, each with an
  `&mut EventContext`. Use it only when the change must reach the ambient context
  (`ctx.send_intent`, `ctx.set_theme`, open a window); the bound `Signal` is
  still the state and still the notification. It fires on **user activation
  only**, never on a programmatic write to the signal — observe the signal for
  that direction. A radio reports a *real* change (re-activating the selected
  button says nothing); a slider is continuous through a drag and has no
  commit-on-release.
- **Slot methods: id-taking is common, not universal, and the name does not tell
  you.** Most structural slots (`child`, `content`, `header`, `pane`, `tab`) take
  `impl IntoTeksiChild`, so a `WidgetId` or a widget both work, and the old
  `*_id` twins are gone (including `TabWidget::tab_id` and `ToolBox::item_id`).
  But 65 slot declarations still take `impl Widget + 'static` and reject an id —
  including **`PopoverWidget::content`**, which is a named slot on a widget you
  would expect to take one. Also `TextInput::leading_slot` / `trailing_slot`,
  every `StandardListItem` / `StandardTreeItem` slot, `MenuList::item` / `header`,
  `Banner::action`, `Cycle::child`, `RadioGroup::child`, and the whole
  `composite_tooltip` family. Extract the signature rather than assuming.
- **Renames landing between 0.9 and 0.12** (a stale call site fails to resolve,
  so the compiler names them — this list is only so you recognise the fix):
  `Checkbox::labels_hidden(bool)` → `labelled_externally()` (no argument, and
  `Toggle` already spelled it that way); `StandardTreeItem::on_toggle` /
  `on_toggle_rc` → `on_chevron_toggle` / `on_chevron_toggle_rc` (the chevron, not
  the checkbox — `on_checkbox_toggle` is the box); the `.dim_when_inactive(..)`
  builder method is gone, wrap the subtree in the `DimWhenInactive` widget
  instead.

## `teksu!` in one paragraph

`teksu!` is an **optional** block-structured macro for widget trees that desugars
one-to-one to the builder calls you would otherwise write — no runtime, no
virtual tree, byte-for-byte the same output. It earns its keep on deep nested
trees and is pointless on a flat one; the two forms nest in either direction. The
single most common mistake is writing a handler as a value:
`on_activate_fn: |ctx| ctx.send_intent(AppIntent::Save)` is a **closure**, not
`on_activate: AppIntent::Save`. Before writing anything non-trivial in it, read
**`reference/teksu.md`** — the routing rules there (which slots reject a
`WidgetId`, the Category B widgets that have no `.child()`, the three rejected
heads, the 4-arm cap) exist because each of them was got wrong.

## Driving and testing the running app

Teksilo ships an automation layer that observes (accessibility tree, full
layout tree with bounds, screenshots) and drives (AT actions, synthetic pointer /
key / IME input) a live app in-process, with no OS accessibility layer — plus a
Python probe harness (`cargo teksilo probe`) to make any of it repeatable. Two
rules that cost the most time when missed: a **structural rebuild invalidates
every node id**, so re-find after one; and for an accelerator chord inject
**`command: true`, not `ctrl`** — on macOS `ctrl` injects a key that matches no
binding and still reports success. Full catalog, error codes and probe workflow:
**`reference/automation.md`**.

## Reference files

| File | Read it when |
|---|---|
| `reference/teksilo_app_guide.md` | Anything about the app-author surface: entry point, Widget trait, layout model, Signal/Prop, events, intents & shortcuts, theming, animation, accessibility overrides, i18n, settings, data models, widget catalog, toasts, drag-and-drop, `EventContext`, breaking changes 0.9 → 0.12. The default first read. |
| `reference/teksu.md` | Writing, reading, translating or debugging a `teksu!` block. Routing rules, slot arity, diagnostics, limitations, the formatter. |
| `reference/automation.md` | Driving or testing the running app: wiring the bridge, attaching, the 34-tool catalog by job, error codes, and the probe harness workflow. |
