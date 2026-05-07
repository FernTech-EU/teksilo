# Tooltip Reference

Tooltips are hover-/focus-triggered overlays that surface ancillary information
about a control. FernUI ships **three tiers** that share one attachment pipeline:

- **Plain tooltips** — a single localized string in a themed rounded-rect
  surface. Pure-text, ephemeral, no interaction.
- **Rich tooltips** — a registry-driven content surface that may carry inline
  markup (`*italic*`, `**bold**`, `[label](url)`), a shortcut hint, an
  Accordion-revealed "more" body, and a sticky-on-dwell promotion to a
  focusable, click-through Dialog.
- **Composite tooltips** — host an arbitrary widget tree (Crusader Kings 3
  style: tabbed sections, charts, progress bars, conditional rows, dynamic
  numeric values). Same dwell-to-sticky machinery as rich tooltips. "Primary
  only" by construction — has no inline-markup body and no registry key, so
  it cannot be the target of a `[label](:key)` cascade from a rich tooltip.
  Child widgets *inside* the composite body keep their own
  `.tooltip(...)` / `.rich_tooltip(...)` setters and cascade normally.

All three ride the same `WidgetTree` machinery (hover/focus tracking, delay
scheduling, overlay show/dismiss, fade-in animation). The choice of tier is
made per-anchor by which builder method you call on the host widget. The
three setters are mutually exclusive (last-call-wins): each setter clears
the other two.

| Layer | Type | Crate | What it does |
|-------|------|-------|--------------|
| Plain content widget | [`TooltipWidget`](../crates/fern-widgets/src/tooltip.rs) | `fern-widgets` | Themed rounded-rect with one line of text |
| Rich content widget | [`RichTooltipWidget`](../crates/fern-widgets/src/tooltip/rich.rs) | `fern-widgets` | Body + shortcut chip + "more" disclosure + dwell indicator |
| Composite content widget | [`CompositeTooltipWidget`](../crates/fern-widgets/src/tooltip/composite.rs) | `fern-widgets` | Surface hosting an arbitrary widget tree (TabWidget, charts, progress bars, conditional rows, dynamic values) with dwell-to-sticky promotion |
| Registry | [`TooltipRegistry`](../crates/fern-widgets/src/tooltip/registry.rs) / [`TooltipContent`](../crates/fern-widgets/src/tooltip/registry.rs) | `fern-widgets` | Thread-local catalog keyed by short stable ids |
| Attach helpers | [`attach_rich_tooltip*`](../crates/fern-widgets/src/tooltip/attach.rs) / [`attach_composite_tooltip*`](../crates/fern-widgets/src/tooltip/attach.rs) | `fern-widgets` | Wire a tooltip onto an anchor inside `build()` |
| Tree machinery | [`WidgetTree::attach_tooltip*`](../crates/fern-core/src/widget_tree/overlay_impl.rs) | `fern-core` | Hover/focus tracking, dwell promotion, overlay lifetime |
| Visual progress | [`DwellIndicator`](../crates/fern-widgets/src/tooltip/dwell_indicator.rs) | `fern-widgets` | Pie-wedge / pin glyph for sticky-on-dwell |
| Tokens | [`TooltipStyle`](../crates/fern-tokens/src/components.rs) / [`CompositeTooltipStyle`](../crates/fern-tokens/src/components.rs) | `fern-tokens` | `padding_*`, `corner_radius`, `max_width`, `max_height` (composite only), `shadow_density` |

---

## Quick start

### Plain tooltip on any widget

```rust
use fern_ui::prelude::*;

Button::new(tr!("save"))
    .tooltip(tr!("save_hint"))                 // i18n
    .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Save));
```

`.tooltip(...)` accepts `impl Into<LocalizedString>`. The grep-marker
`.tooltip_literal("...")` exists as a `#[doc(hidden)]` shim for tests and
scaffolding.

### Rich tooltip from the registry

```rust
use fern_ui::prelude::*;
use fern_widgets::tooltip::TooltipContent;

fn main() {
    FernAppBuilder::new()
        .register_tooltips(vec![
            TooltipContent::new("save-as", tr!("save_as_tooltip"))
                .for_shortcut("app.save_as"),
            TooltipContent::new("autosave", tr!("autosave_tooltip"))
                .with_more(tr!("autosave_tooltip_more")),
        ])
        .initial_window(WindowConfig::new().root(|tree, _| {
            tree.add(Button::new(tr!("save_as")).rich_tooltip("save-as"))
        }))
        .run();
}
```

Two attachment paths once registered:

- `.rich_tooltip("save-as")` — registry key lookup at build time.
- `.rich_tooltip_content(TooltipContent::new(...))` — inline content; bypasses
  the registry. Useful for one-off tooltips, tests, and per-row tips on
  data-driven widgets.

### Composite tooltip (CK3-style)

For tooltips that need a full widget tree — tabs, charts, progress bars,
conditional rows, dynamic numeric values — use `.composite_tooltip(content)`:

```rust
use fern_ui::prelude::*;

Button::new(tr!("province_info"))
    .composite_tooltip(
        VStack::new()
            .spacing(8.0)
            .child(TextWidget::new(tr!("province_header")).style(TextStyleRole::BodyBold))
            .child(ProgressBar::new(prosperity_signal))
            .child(
                Grid::new()
                    .columns(vec![TrackSize::Auto, TrackSize::Auto])
                    .child(TextWidget::new(tr!("food")))
                    .child(TextWidget::new_literal(food_value))
                    .child(TextWidget::new(tr!("trade")))
                    .child(TextWidget::new_literal(trade_value)),
            ),
    );
```

The dwell-to-sticky machinery is reused from rich tooltips: at 2 s the role
flips `Tooltip → Dialog`, dismiss swaps to `EscapeOrClickOutside`, and the
surface becomes Tab-reachable. Rare interactive descendants (a "Pin"
button, an internal `TabWidget`) work cleanly post-promotion.

The default delay is `DEFAULT_COMPOSITE_TOOLTIP_DELAY` (400 ms — slower than
the 200 ms rich-tooltip delay because composite surfaces are heavier and
shouldn't pop on transient hover). Default `max_width` × `max_height` are
both 480 dp, configurable per-instance with `.max_width(f32)` /
`.max_height(f32)` on `CompositeTooltipWidget`, or globally via
`Theme::components.composite_tooltip`.

**No registry, no `:key` cascade target.** Composite tooltips are widget
trees, not data — they don't fit the `TooltipRegistry`'s
`Vec<TooltipContent>` model and have no stable id to address in markup. The
"primary-only" constraint is structural, not enforced at runtime: there is
simply no key to write in `[label](:key)`.

#### Cascading from a composite tooltip

A child widget *inside* the composite body (e.g. a stat row's
`Button::rich_tooltip("modifier-detail")`) keeps working as ordinary widget
composition — its own `build()` runs the existing rich-attach path, and the
nested overlay opens via `OverlayLayer::InTree` parented to the composite
tooltip's overlay. Mix tiers freely.

### Last-call-wins setter matrix

The three setters are mutually exclusive — every setter clears the other two.

| Setter | Sets | Clears |
| --- | --- | --- |
| `.tooltip(text)` / `.tooltip_literal(text)` | plain text | rich source, composite body |
| `.rich_tooltip(key)` / `.rich_tooltip_content(c)` | rich source | plain text, composite body |
| `.composite_tooltip(w)` | composite body | plain text, rich source |

This is preserved across all widgets that expose multiple tooltip flavors:
`Button`, `Link`, `MenuItem`, `TextInput`, `IconButton`, `Checkbox`,
`RadioButton`, `SplitButton` (separate `.tooltip(...)` and
`.chevron_tooltip(...)` matrices), and `TabInfo` / `TabDelegate` for tab
strips.

---

## Registration: `FernAppBuilder::register_tooltips`

The application's tooltip catalog is a single `Vec<TooltipContent>` registered
once at boot. The bundle is frozen into a thread-local `TooltipRegistry` *before
the first frame builds* — both `run()` and `build_headless()` install it
before invoking the root builder, so tooltip widgets created during the very
first build can resolve their content immediately.

```rust
FernAppBuilder::new()
    .register_tooltips(vec![ /* ... */ ])
    .run();
```

Calling `register_tooltips` more than once on the same builder simply replaces
the previous catalog (the registry only sees the final list when `run` /
`build_headless` fires). Calling `install_tooltip_registry` directly twice on
the same thread panics in debug builds; release builds keep the first
installation. Tests reset between cases via the crate-internal
`_reset_tooltip_registry` helper.

### `TooltipContent` builder

```rust
pub struct TooltipContent {
    pub key: String,
    pub text: LocalizedString,
    pub more: Option<LocalizedString>,
    pub shortcut_label: Option<String>,        // literal override
    pub shortcut_id: Option<&'static str>,     // ShortcutRegistry binding
}
```

| Method | Behavior |
|--------|----------|
| `TooltipContent::new(key, text)` | Construct with body only |
| `.with_more(LocalizedString)` | Long-form body revealed by the Accordion disclosure inside a sticky tooltip |
| `.with_shortcut_label("Ctrl+Shift+S")` | Manual shortcut hint — used verbatim, takes precedence over `for_shortcut` |
| `.for_shortcut("app.save_as")` | Bind the chip to a registered `Shortcut` id; the effective primary keystroke is read from the tree's `ShortcutRegistry` and tracks user rebinds (the registry's `version` signal triggers a `Rebuild`-level rebind on the tooltip widget) |
| `.has_more()` / `.has_shortcut()` | Predicates used by the layout |

The body is a `LocalizedString`, so production code uses `tr!(...)`; literal
strings only show up in tests and demos via `LocalizedString::literal(...)`.

### URL scheme inside the body

Inline links inside body text use the `:key` prefix to address other tooltip
entries:

```rust
TooltipContent::new(
    "autosave",
    tr!("autosave_with_link"),    // "FernUI autosaves. See [details](:autosave-details)…"
)
```

`TooltipRegistry::parse_url(":autosave-details")` returns `Some("autosave-details")`.
Every other URL scheme — `http://`, `https://`, `mailto:`, bare paths — passes
through unchanged and is dispatched to `open::that(url)` (the OS default
handler) when clicked, so production code spawns a browser / mail client
without extra wiring. The `open::that` call is suppressed under `cfg(test)`
so unit tests don't actually launch external apps.

When a body contains `[label](:key)` links, the rich tooltip widget
**pre-creates** dormant `RichTooltipWidget` children for every registered
target during its `build()` (matching the menu-submenu pattern in
`menu_item.rs`). Clicking a link activates the matching child and calls
`ctx.show_overlay(...)` anchored to the parent tooltip's own widget id,
positioned 8 px below it, with dismiss behavior `EscapeOrClickOutside`.
Nested overlays are registered with `parent_overlay: None`, so each is
independent from the manager's perspective — Escape / click-outside
dismisses each level on its own behavior, and a deep cascade unwinds as
the user clicks away.

---

## Attaching tooltips inside `build()`

Most widgets expose `.tooltip(...)` / `.rich_tooltip(...)` builder methods
that wire everything internally. When you author a custom anchor you reach
the same machinery through `BuildContext`:

```rust
// Plain tooltip — caller-managed delay.
let tooltip_id = ctx.add(TooltipWidget::new(tr!("save_hint")));
ctx.attach_tooltip(anchor_id, tooltip_id, Duration::from_millis(500));

// Rich tooltip from the registry — recommended path.
crate::tooltip::attach_rich_tooltip(
    ctx,
    anchor_id,
    "save-as",
    crate::tooltip::DEFAULT_RICH_TOOLTIP_DELAY,
);

// Rich tooltip from inline content (no registry lookup).
crate::tooltip::attach_rich_tooltip_content(
    ctx,
    anchor_id,
    TooltipContent::new("inline", tr!("inline_body")),
    crate::tooltip::DEFAULT_RICH_TOOLTIP_DELAY,
);

// Source-driven — accepts either a key or an inline content.
crate::tooltip::attach_rich_tooltip_source(
    ctx,
    anchor_id,
    source,                                       // RichTooltipSource
    crate::tooltip::DEFAULT_RICH_TOOLTIP_DELAY,
);
```

`RichTooltipSource` is the union type accepted by builder methods that want
to take either form:

```rust
pub enum RichTooltipSource {
    Key(String),
    Content(TooltipContent),
}

impl<T: Into<String>> From<T> for RichTooltipSource { /* … */ }
```

The attach helpers do three things:

1. Construct the content widget (`TooltipWidget` / `RichTooltipWidget`).
2. Insert it into the arena via `ctx.add(...)` and immediately mark it dormant
   — it has no parent on the visible scene; the overlay manager activates it
   on show.
3. Register a `TooltipEntry` on the tree with the anchor id, content id,
   delay, and (for rich) the dwell threshold + a shared `shown_at` sink.

### Default delays

| Path | Delay | Where it's defined |
|------|-------|--------------------|
| Rich tooltip | `200 ms` | `DEFAULT_RICH_TOOLTIP_DELAY` in [tooltip/attach.rs](../crates/fern-widgets/src/tooltip/attach.rs) |
| `Button` plain tooltip | `200 ms` | inline literal in [button.rs](../crates/fern-widgets/src/button.rs) |
| `Checkbox`, `RadioButton`, `MenuItem`, `SplitButton`, `Link`, `IconButton`, `TextInput` plain tooltip | `500 ms` | inline literal in each widget |

There is no token for plain-tooltip delay yet; widgets that need a custom
value pass an explicit `Duration` to `attach_tooltip`.

### `BuildContext` surface

| Method | Use |
|--------|-----|
| `attach_tooltip(anchor, content, delay)` | Plain hover-only tooltip |
| `attach_tooltip_with_sticky(anchor, content, delay, sticky_after)` | Tooltip that auto-promotes after `sticky_after` of visible time |
| `attach_tooltip_with_sticky_sink(anchor, content, delay, sticky_after, shown_at_sink)` | Same plus a shared `Rc<Cell<Option<Instant>>>` the tree updates on show/dismiss; the rich widget reads it from `paint()` to drive its dwell indicator without a paint-gap heuristic |
| `promote_tooltip_to_sticky(content_id)` | Manual promotion — flag the entry sticky and swap the overlay to `EscapeOrClickOutside` |

---

## Lifecycle: hover, delay, show, fade

The `WidgetTree` keeps a `Vec<TooltipEntry>` and visits it once per processed
event batch. The state-machine is:

1. **Hover enter** (`tooltip_pointer_enter`). Every entry whose `anchor_id`
   contains the entered widget records `hover_start = now`. No overlay yet.
2. **Delay tick** (`process_tooltips` / `process_tooltips_real`). Each entry
   whose elapsed time since `hover_start` ≥ `delay` is shown — `arena.activate`
   on the dormant content, `show_overlay` with placement `NearAnchor { offset: (0, 8) }`
   and dismiss behavior `PointerLeave { delay: 100 ms }`. The
   `shown_at_sim` / `shown_at_real` timestamps are recorded; the optional
   `shown_at_sink` is updated.
3. **Fade-in.** Tooltips fade in over `MotionTokens::duration_fast` (~120 ms).
   Reduced-motion users get an instant snap (no fade animation).
4. **Hover leave** (`tooltip_pointer_leave`). Plain tooltips dismiss
   immediately and clear `hover_start`. Sticky tooltips (post-promotion)
   survive — the user dismisses them via `EscapeOrClickOutside`.

`WidgetTree::next_timer_deadline()` returns the earliest pending tooltip /
delayed-overlay deadline so the idle-event loop knows when to wake (see
[idle-and-animation.md](idle-and-animation.md)).

---

## Sticky-on-dwell (rich tooltips)

Rich tooltips opt into a 2-second dwell timer that promotes a hover-shown
tooltip into a focusable, click-through Dialog. The threshold lives in
[`DWELL_PROMOTION`](../crates/fern-widgets/src/tooltip/rich.rs) and is
`Duration::from_secs(2)`; it's split into 4 visible quarters of 500 ms each,
driving the [`DwellIndicator`](../crates/fern-widgets/src/tooltip/dwell_indicator.rs)
in the tooltip's top-right corner.

Visual progression of the indicator:

| Step | Glyph |
|------|-------|
| 0 | Empty 14×14 circle outline (just shown) |
| 1 | 25 % pie wedge filled (12 → 3 o'clock) |
| 2 | 50 % wedge |
| 3 | 75 % wedge |
| 4 / sticky | Filled pin icon (head + downward triangle tail) |

The dwell mechanism wires up two reactive signals (`Signal<u32>` step,
`Signal<bool>` sticky) inside `RichTooltipWidget`. On every paint the
widget recomputes both from the authoritative `shown_at` sink — the tree
writes `Some(now)` on show and `None` on dismiss, so the widget never
needs to track its own visibility heuristically.

When the dwell threshold elapses, `WidgetTree::process_tooltips_impl`
sweeps the active rich tooltips and calls
`promote_tooltip_to_sticky(content_id)`:

- The entry's `is_sticky` flag flips to `true` so `tooltip_pointer_leave` no
  longer auto-dismisses it.
- The overlay's dismiss behavior is swapped to `EscapeOrClickOutside`.
- The tooltip's a11y role flips from `Role::Tooltip` to `Role::Dialog` and
  the AT node advertises `Action::Focus` (the rebind happens at
  `BindingLevel::AccessibilityOnly`, so no relayout / repaint cost).
- The widget itself is `focusable(true)` unconditionally (avoiding a
  rebuild on every sticky flip), but only the sticky form is meaningfully
  reachable — ephemeral tooltips dismiss on pointer-leave so users can't
  realistically Tab into them.

The auto-promote sweep marks the entire tooltip subtree `needs_paint` on
every dwell-window frame, so the indicator's pie wedge advances visibly as
the user keeps hovering.

### Accordion "more" disclosure

When `TooltipContent::with_more(...)` is set, the rich tooltip's footer row
contains an `Accordion` whose title is the literal string `"More"` and
whose content is the long-form body (also markup-aware). The Accordion's
expand state is a `ctx.signal(false)` owned by the tooltip widget — the
disclosure animates open in place once the user clicks the chevron, which
is only practically reachable after the tooltip has gone sticky (clicks
on a non-sticky tooltip would otherwise dismiss it via `PointerLeave`).

---

## Keyboard / a11y promotion

Pointer users reach the rich-tooltip interactive surface via the 2-second
dwell. Keyboard and screen-reader users get the same access via *focus
promotion* — `WidgetTree::tooltip_focus_enter` is called when a widget
gains keyboard focus and immediately shows + promotes any rich tooltip
whose `anchor_id` is in the focused subtree (in either direction —
composite widgets like `Button` attach the tooltip to an inner subtree
root but keep focus on the outer node, so the check accepts an
ancestor-or-descendant relationship).

Plain tooltips are deliberately **not** auto-shown on focus — their text
reaches assistive tech via the anchor's `aria-describedby` / `Tooltip`
role link wired in the AccessKit pass, which is the W3C-recommended
pattern for supplementary hints.

When focus moves away from a focus-promoted tooltip,
`tooltip_focus_leave_outside` dismisses it unless the new focus is
inside either the anchor's subtree or the tooltip-content subtree (so
Tab-into the tooltip to click an inline link keeps it open). Pointer-
dwelled stickies survive focus changes intact — they're only dismissed
via `Escape` or click-outside, matching the existing mouse UX.

### Accessibility roles

| State | Role | Notes |
|-------|------|-------|
| `TooltipWidget` (always) | `Role::Tooltip` with `set_name(text)` | Plain text, no interaction |
| `RichTooltipWidget` (ephemeral) | `Role::Tooltip` with `set_name(body_text_resolved)` | Body and shortcut child `TextWidget`s are `a11y_hidden` so the parent owns the announcement |
| `RichTooltipWidget` (sticky) | `Role::Dialog` + `Action::Focus` | Tab-reachable, click-through |
| `DwellIndicator` | `Role::GenericContainer` | Decorative — content meaning lives on the tooltip itself |

Inline shortcut chips are bound to the `ShortcutRegistry` so user rebinds
re-render the chip on the next pass (the registry's version signal is
bound at `BindingLevel::Rebuild`).

---

## Theming knobs

`Theme::components.tooltip` is a [`TooltipStyle`](../crates/fern-tokens/src/components.rs):

```rust
pub struct TooltipStyle {
    pub padding_horizontal: f32,    // default 10.0
    pub padding_vertical: f32,      // default 6.0
    pub corner_radius: f32,         // default 8.0
    pub max_width: f32,             // default 320.0
}
```

Color tokens (`Theme::colors`):

| Token | Role | Default (light + dark — both intentionally dark) |
|-------|------|-----------------|
| `tooltip_bg` | `SurfaceRole::TooltipBg` | `#1E1F22` |
| `tooltip_text` | `TextRole::TooltipText` | `#DFE1E5` |
| `tooltip_border` | `BorderRole::TooltipBorder` | `#393B40` |
| `tooltip_shortcut` | `TextRole::TooltipShortcut` | `#9DA0A8` |

Int UI's house style: tooltip surfaces stay dark in both light and dark themes
for high-contrast popups (also reused by `Snackbar`). The OS-theme bridge
([`theme.rs`](../crates/fern-tokens/src/theme.rs)) lets a host OS override
`tooltip_bg` / `tooltip_text` if the platform exposes corresponding values.

Motion knobs come from `MotionTokens`:

| Token | Default | Used for |
|-------|---------|----------|
| `duration_fast` | 120 ms | Tooltip fade-in (and matching fade-out for sticky dismiss) |

The `RichTooltipWidget` clamps its proposal width to `tooltip.max_width` in
`layout_response` so long bodies wrap rather than stretching the surface
horizontally. Layout uses a `Grid` (Fractional + Auto columns) so the body
text receives a width proposal that excludes the trailing shortcut chip and
the dwell indicator — `HStack + Spacer` would propose the body's natural
single-line width and the chip / indicator would overflow.

---

## Builder API surface (per-widget)

Every visible interactive widget exposes one or both of these patterns. The
last setter wins — calling `.tooltip(...)` after `.rich_tooltip(...)` clears
the rich source, and vice versa.

| Widget | Plain | Rich (key) | Rich (inline) |
|--------|-------|------------|---------------|
| [`Button`](../crates/fern-widgets/src/button.rs) | `tooltip(text)` | `rich_tooltip(key)` | `rich_tooltip_content(content)` |
| [`Link`](../crates/fern-widgets/src/link.rs) | `tooltip(text)` | `rich_tooltip(key)` | `rich_tooltip_content(content)` |
| [`MenuItem`](../crates/fern-widgets/src/menu_item.rs) | `tooltip(text)` | `rich_tooltip(key)` | `rich_tooltip_content(content)` |
| [`Checkbox`](../crates/fern-widgets/src/checkbox.rs) | `tooltip(text)` | — | — |
| [`RadioButton`](../crates/fern-widgets/src/radio_button.rs) | `tooltip(text)` | — | — |
| [`SplitButton`](../crates/fern-widgets/src/split_button.rs) | `tooltip(text)` | — | — |
| [`IconButton`](../crates/fern-widgets/src/icon_button.rs) | `tooltip(text)` | — | — |
| [`TextInput`](../crates/fern-widgets/src/text_input.rs) | `tooltip_literal(text)` | `rich_tooltip_key(key)` | `rich_tooltip(content)` |
| [`ToolBox`](../crates/fern-widgets/src/tool_box.rs) | — | `tooltip(impl Into<RichTooltipSource>)` | `tooltip_content(content)` |

`tooltip_literal` is a permanent `#[doc(hidden)]` shim that wraps a raw
`String` in `LocalizedString::literal` — same grep marker as
`Button::new_literal`, intended for tests and explicitly-untranslated call
sites.

---

## Authoring tip: pre-create dormant content

The plain `attach_tooltip` API takes a `content_id` you already inserted
into the arena. The pattern inside an anchor widget's `build()` is:

```rust
fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
    let root = ctx.add(/* visible subtree */);

    if let Some(text) = self.tooltip_text.as_deref() {
        let tooltip = ctx.add(TooltipWidget::new_literal(text));
        ctx.attach_tooltip(root, tooltip, Duration::from_millis(500));
    }

    self.root_child_id = Some(root);
    vec![root]
}
```

`attach_tooltip_inner` immediately calls `arena.set_dormant(content_id)`,
so callers don't need their own dormant marker — but they must not place
the tooltip widget under a visible parent. The standard pattern is
`ctx.add(tooltip_widget)` (which inserts at the arena top level) followed
by the `attach_*` call.

---

## See also

- [Overlays in fern-ui-architecture.md](fern-ui-architecture.md) — overlay
  manager, `OverlayRequest`, dismiss behaviors.
- [reactive-theme.md](reactive-theme.md) — `ColorProp`, role-driven colors,
  Signal-bound theme switching.
- [shortcut-intent-action.md](shortcut-intent-action.md) — the
  `ShortcutRegistry` that backs `TooltipContent::for_shortcut`.
- [accessibility-overrides.md](accessibility-overrides.md) — builder-level
  AT augmentation, including `.access_described_by(tooltip_content_id)`
  for explicit `aria-describedby` wiring.
- [idle-and-animation.md](idle-and-animation.md) — how the idle event loop
  uses `next_timer_deadline()` to schedule pending-tooltip wake-ups.
