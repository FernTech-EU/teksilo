---
name: extract-widget-api
description: Extract the public API and inline documentation of a type from any teksilo crate with a public API (30 of them, including teksilo-core, teksilo-tokens, teksilo-canvas and teksilo-charts; four are also mdBook-cataloged). Use when the user wants to see a type's public surface (struct, builder methods, enums, module doc) without opening the file, or asks things like "show me Button's API", "what are HStack's builder methods", "what's on ListModel", "list teksilo-widgets", or "/extract-widget-api <Name>". Also use when packing widget docs into context for a downstream task, or when regenerating the mdBook catalog pages.
user_invocable: true
---

# extract-widget-api

> **Where to run this.** Every path below is relative to the root of a **Teksilo framework
> checkout** — the workspace containing `tools/extract_widget_api.py` and `crates/teksilo-widgets/`.
> Locate it with `git rev-parse --show-toplevel` from anywhere inside it, confirm the marker
> file is there, and `cd` to that root. If the working directory is not in such a checkout, ask
> the user where it is rather than guessing.
>
> A **consumer app** that merely pins the `teksilo` crate has no checkout and no `tools/`
> directory. Use the `teksilo-app` skill's `scripts/teksilo-api.sh` there instead — it reads the
> pinned source through `cargo metadata`, and takes the same arguments as the script below.
>
> The `../../../` links in this file resolve only when it is read from inside the checkout. When
> the skill is installed elsewhere (a user-level skills directory), they are dead links — the
> commands still work once you have cd'd to a real checkout.

Run [tools/extract_widget_api.py](../../../tools/extract_widget_api.py) to emit a type's public API with `///` docs and its `//!` module header. Skips `impl Widget for Foo` trait plumbing and `pub(crate)` items.

## Usage

Run from the repository root via Bash:

```bash
python3 tools/extract_widget_api.py <Name> [<Name> ...]
```

Names accept type names (`Button`, `HStack`, `Dialog`) or module names (`button`, `hstack`), case-insensitive.

### Common invocations

```bash
python3 tools/extract_widget_api.py --list                        # List every widget file
python3 tools/extract_widget_api.py Button                        # One widget
python3 tools/extract_widget_api.py Button HStack Dialog          # Several at once
python3 tools/extract_widget_api.py --all                         # Every widget (large)
python3 tools/extract_widget_api.py Button -f json                # JSON for tooling
python3 tools/extract_widget_api.py Button -o /tmp/button.md      # Write to file
python3 tools/extract_widget_api.py Button -f text                # Plain text, no markdown
```

## It covers every crate with a public API, not just widgets

`--crate` selects the source tree. **30 crates are queryable**; the four in the
table below are additionally *cataloged* — only they generate mdBook pages under
`docs/`, which is what `--catalog-all` and `--md-dir` act on. `--md-dir` on a
queryable-but-not-cataloged crate refuses rather than writing pages.

A name reached through the `teksilo` umbrella prelude resolves to its owning
crate automatically: `Theme` looked up with the default `--crate widgets` prints
a note and answers from `teksilo-core`, rather than reporting "not found".

**The default is `widgets`, so a lookup for a
non-widget type fails with "did you mean" noise until you pass the right crate** —
check the table before concluding a type doesn't exist.

| `--crate` | Crate | Holds |
|---|---|---|
| `widgets` (default) | `teksilo-widgets` | `Button`, `HStack`, `ListView`, `Dialog`, the layout primitives, … |
| `data` | `teksilo-data` | `ListModel`, `TreeModel`, `TreeSlice`, `SelectionModel`, `SortFilterListModel`, `CheckedModel`, `ChartModel`, … |
| `settings` | `teksilo-settings` | `SettingsStore`, `SettingsFile`, `MruList`, `WindowStateService`, … |
| `scene` | `teksilo-scene` | `Scene`, `SceneModel`, `SceneView`, `SceneItem`, `SceneCard`, `Magnet`, … |

```bash
python3 tools/extract_widget_api.py --crate data ListModel        # a data model
python3 tools/extract_widget_api.py --crate data --list           # everything in teksilo-data
python3 tools/extract_widget_api.py --crate core Theme            # a queryable-only crate
python3 tools/extract_widget_api.py --crate tokens Color          # …and another
python3 tools/extract_widget_api.py --crate core --list           # what teksilo-core exposes
python3 tools/extract_widget_api.py --crate scene SceneModel      # a scene type
python3 tools/extract_widget_api.py --crate settings MruList      # a settings service
```

Everything else (`--list`, `--all`, `-f`, `-o`) composes with `--crate`.

Four crates are **not** covered — `teksilo-core`, `teksilo-tokens`, `teksilo-canvas` and
`teksilo-charts` have no spec entry. For those, read the source or use `cargo doc`.

## Regenerating the mdBook catalog

The committed catalog pages under `docs/widgets/`, `docs/data-collections/`,
`docs/settings/` and `docs/scene/` come from this same script. Regenerate after
changing a public API:

```bash
cargo doc -p teksilo-widgets --no-deps                                        # build /api first
python3 tools/extract_widget_api.py --all --md-dir docs/widgets --api-dir target/doc
python3 tools/extract_widget_api.py --catalog-all                             # all four crates at once
```

`--md-dir` also patches the `<!-- BEGIN/END GENERATED WIDGETS -->` region of the
matching `SUMMARY.md` — but **only when DIR is inside `docs/`**, so a scratch-directory
run leaves the book's table of contents alone. It ignores positional names (always emits
all). `--api-dir` points at a built rustdoc tree so deep links to private / cfg-gated
modules fall back to the nearest real page instead of 404ing; it is auto-detected from
`target/doc` when present. `--api-base` overrides the rustdoc link root (default: the
crate's docs.rs base, so a committed page resolves when read on GitHub).

Preview images are a separate step (needs a GPU) — see the documentation-site section of
the repo `CLAUDE.md`.

## Argument parsing

If the user invoked `/extract-widget-api` with arguments, pass them straight through:

- `/extract-widget-api Button` → `python3 tools/extract_widget_api.py Button`
- `/extract-widget-api HStack VStack` → `python3 tools/extract_widget_api.py HStack VStack`
- `/extract-widget-api --list` → `python3 tools/extract_widget_api.py --list`

If no arguments were given, ask the user which type(s) they want, or suggest `--list`.

## Output handling

- For 1–2 types, print the full output directly to the user — it's already markdown-formatted.
- For `--all` or more than ~3 types, write to a file with `-o` and tell the user the path, since the dump is large.
- If the user asks for JSON (for tooling or downstream LLM context), use `-f json`.

## Errors

- Unknown names exit with code 2 and print `Did you mean: X, Y, Z?` suggestions. Relay the suggestions to the user — **but first check whether the type simply lives in another crate** (`--crate data` / `settings` / `scene`), which is the more common cause of a miss than a typo.
- If the script reports "src not found", you're likely not at the repo root — `cd` to the checkout root (see the note at the top) or invoke the script by absolute path. In a consumer app with no checkout, use `teksilo-api.sh` from the `teksilo-app` skill.
- `python3 tools/extract_widget_api.py --test` runs the generator's own smoke tests (a pre-commit gate).
