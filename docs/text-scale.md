<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Global text scale (accessibility "grow all text")

A single app-wide setting that magnifies **all** text for low-vision users —
persisted across launches and exposed through a ready-made settings control. It
is the framework analogue of the "Text size" slider in an OS accessibility
panel.

- **One source of truth.** The user factor (`1.0` = 100 %) multiplies the OS
  accessibility text-scale preference; the product is the *effective text
  scale*.
- **No rebuild.** Changing the scale marks the tree dirty (relayout + repaint);
  focus, scroll offsets, and interaction state survive.
- **Persisted + restored automatically.** Any app that installs settings gets
  startup restore for free.

---

## For app developers

### Drop in the control

`TextScaleControl` is a specialized `SpinBox` (80 %–200 %, step 10 %). Bind it to
the persisted key and place it in a settings window — it both persists the value
and applies it app-wide on edit. No other wiring.

```rust
use teksilo::prelude::*;            // re-exports TEXT_SCALE_KEY
use teksilo::widgets::TextScaleControl;

// inside build():
let scale = ctx.settings().signal_for(&TEXT_SCALE_KEY);
ctx.add(TextScaleControl::new(scale).label(tr!(text_size())));
```

Requirements: the app must install settings (`.application(...)` /
`.app_paths(...)` + `.settings(SettingsBundle::new())`). With settings present,
`teksilo-app` reads `accessibility.text_scale` at startup and seeds every
window; apps without settings simply stay at `1.0`.

### Apply / read it programmatically

- From any handler: `ctx.set_text_scale(factor)` applies app-wide (every window)
  after the handler returns — same model as `ctx.set_theme` / `ctx.set_locale`.
  Persist alongside it via `ctx.settings().signal_for(&TEXT_SCALE_KEY).set(...)`
  (the `TextScaleControl` does both for you).
- Read the current factor at build via `ctx.text_scale()`, or bind the reactive
  `ctx.text_scale_signal()` (`Signal<f32>`) for values that must update without
  a rebuild.

---

## How it works

Font sizes flow through `Theme.typography` (`TypographyTokens`), resolved on
every layout/paint pass. The tree keeps a cached **`effective_theme`** =
the active theme with its `typography` scaled by `user_scale × OS_factor`, and
the layout + paint walkers read it. So **every widget that sizes text from
`ctx.theme.typography` scales for free** — `TextWidget`, `Button`, `Badge`,
`ListItem`, `MenuItem`, `TableView` cells, and so on — with zero per-widget code.

The same combined factor is published two more ways for surfaces that size text
from a source *other* than typography:

- `LayoutContext::text_scale` / `PaintContext::text_scale` — the `f32` factor,
  read during layout/paint.
- `WidgetTree::text_scale_signal()` (and `BuildContext::text_scale_signal()`) —
  a reactive `Signal<f32>` for build-time binders.

`effective_text_scale` and the signal are written in one place
(`WidgetTree::recompute_effective_theme`), so theme, OS-pref, and user-scale
changes all stay consistent.

### Editable text and the rich-text engine

Editable widgets (`TextInput`, `SpinBox`, `DateEdit`, hex color input) and
`RichTextEditor` shape text through a per-widget `RichTextEngine`, whose size
does **not** come from the theme. They scale via a true per-engine **logical
font scale** in `text-typeset`: `RichTextEngine::set_font_scale(f)` multiplies
the resolved font size *before* shaping, so advances, line heights, content
height, and wrapping all grow correctly. Driven automatically from
`ctx.text_scale` at layout/paint.

This is distinct from two pre-existing factors — see the comparison below.

### `font_scale` vs `scale_factor`

| | `scale_factor` | `font_scale` |
|---|---|---|
| Question | physical px per logical px (HiDPI) | how big, *logically* (a11y + per-editor size) |
| Acts at | rasterization | **shaping** (size before layout) |
| Changes logical metrics? | No (cancels out) | **Yes** (grows + reflows) |
| Glyph sharpness | densifies | densifies (larger ppem) |
| Scope | global service | per-engine |

They are orthogonal and never double-count: physical shaping size =
`base_pt × font_scale × scale_factor`; logical metrics = `base_pt × font_scale`.
The standalone single-line shapers used by `TextWidget`/`Canvas` pass
`font_scale = 1.0` (their size is already theme-scaled), which is what keeps
label text from being scaled twice.

### Per-editor text size

`RichTextEditor` / `CodeEditor` / `PlainTextEditor::font_size_scale(f)` (and
`set_font_size_scale` on the rich-text handle) multiplies into the engine font
scale alongside a11y:

```text
engine.font_scale = (follow_text_scale ? ctx.text_scale : 1.0) × font_size_scale
```

Use that for a "Text size 125%" preference. Editors no longer expose page zoom.

---

## Opt-in / opt-out surfaces

A few surfaces don't follow the scale automatically, by design:

| Surface | Default | Knob |
|---|---|---|
| `IconWidget` | **off** (fixed-footprint glyphs) | `.follow_text_scale(true)` |
| Severity badges (`Banner`/`Toast`/`MessageBox`/`NotificationLog`) | **on** | (built in — enabled on the badge's icons) |
| `RichTextEditor` | **on** | `.follow_text_scale(false)` to opt out (e.g. a WYSIWYG editor whose font sizes are document content) |
| `teksilo-scene` `TextItem` | **off** (the scene has its own pan/zoom) | `.follow_text_scale(true)` |
| `Calendar` | **on** (rebuilds with scaled cell/header constants) | — |

`IconWidget::follow_text_scale(true)` multiplies the reported size by
`ctx.text_scale`; paint fills the enlarged bounds automatically.

---

## Reference

- Persisted key: `teksilo_settings::TEXT_SCALE_KEY`
  (`"accessibility.text_scale"`, default `1.0`).
- Widget: [`TextScaleControl`](../crates/teksilo-widgets/src/text_scale_control.rs).
- Core: `WidgetTree::{set_user_text_scale, effective_text_scale, text_scale_signal}`
  ([widget_tree.rs](../crates/teksilo-core/src/widget_tree.rs)); `text_scale` on
  [`LayoutContext`](../crates/teksilo-core/src/widget/layout_context.rs) /
  [`PaintContext`](../crates/teksilo-core/src/widget/paint_context.rs);
  `EventContext::set_text_scale`.
- App fan-out + startup seed: `WindowManager::{set_text_scale,
  set_initial_text_scale, drain_pending_text_scale_requests}`
  ([window_manager.rs](../crates/teksilo-app/src/window_manager.rs)).
- Engine font scale: `RichTextEngine::set_font_scale`
  ([teksilo-text](../crates/teksilo-text/src/rich_text_engine.rs)) →
  `DocumentFlow::set_font_scale` (text-typeset).
- Demo: `cargo run -p widget-catalog` — the `TextScaleControl` in the title bar
  next to the language buttons grows the whole catalog live.
