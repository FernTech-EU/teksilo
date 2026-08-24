<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Tooltip Reference

Tooltips are hover-/focus-triggered overlays that surface ancillary information
about a control. Teksilo ships **three tiers** that share one attachment pipeline:

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
| Plain content widget | [`TooltipWidget`](../crates/teksilo-widgets/src/tooltip.rs) | `teksilo-widgets` | Themed rounded-rect with one line of text |
| Rich content widget | [`RichTooltipWidget`](../crates/teksilo-widgets/src/tooltip/rich.rs) | `teksilo-widgets` | Body + shortcut chip + "more" disclosure + dwell indicator |
| Composite content widget | [`CompositeTooltipWidget`](../crates/teksilo-widgets/src/tooltip/composite.rs) | `teksilo-widgets` | Surface hosting an arbitrary widget tree (TabWidget, charts, progress bars, conditional rows, dynamic values) with dwell-to-sticky promotion |
| Registry | [`TooltipRegistry`](../crates/teksilo-widgets/src/tooltip/registry.rs) / [`TooltipContent`](../crates/teksilo-widgets/src/tooltip/registry.rs) | `teksilo-widgets` | Thread-local catalog keyed by short stable ids |
| Attach helpers | [`attach_rich_tooltip*`](../crates/teksilo-widgets/src/tooltip/attach.rs) / [`attach_composite_tooltip*`](../crates/teksilo-widgets/src/tooltip/attach.rs) | `teksilo-widgets` | Wire a tooltip onto an anchor inside `build()` |
| Tree machinery | [`WidgetTree::attach_tooltip*`](../crates/teksilo-core/src/widget_tree/overlay_impl.rs) | `teksilo-core` | Hover/focus tracking, dwell promotion, overlay lifetime |
| Visual progress | [`DwellIndicator`](../crates/teksilo-widgets/src/tooltip/dwell_indicator.rs) | `teksilo-widgets` | Pie-wedge / pin glyph for sticky-on-dwell |
| Tokens | [`TooltipStyle`](../crates/teksilo-core/src/styles/tooltip_style.rs) (trait) / constants in [`recipe_tooltip_style.rs`](../crates/teksilo-widgets/src/styles/recipe_tooltip_style.rs) | `teksilo-core` / `teksilo-widgets` | `TOOLTIP_PADDING_HORIZONTAL`, `TOOLTIP_PADDING_VERTICAL`, `TOOLTIP_CORNER_RADIUS`, `TOOLTIP_MAX_WIDTH`; composite variants prefixed `COMPOSITE_TOOLTIP_*` |

---

## Quick start

### Plain tooltip on any widget

```rust
use teksilo::prelude::*;

Button::new(tr!(save()))
    .tooltip(tr!(save_hint()))                 // i18n
    .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Save));
```

`.tooltip(...)` accepts `impl Into<LocalizedString>`. The grep-marker
`.tooltip(lit!("..."))` exists as a `#[doc(hidden)]` shim for tests and
scaffolding.

### Rich tooltip from the registry

```rust
use teksilo::prelude::*;
use teksilo_widgets::tooltip::TooltipContent;

fn main() {
    TeksiloAppBuilder::new()
        .register_tooltips(vec![
            TooltipContent::new("save-as", tr!(save_as_tooltip()))
                .for_shortcut("app.save_as"),
            TooltipContent::new("autosave", tr!(autosave_tooltip()))
                .with_more(tr!(autosave_tooltip_more())),
        ])
        .initial_window(WindowConfig::new().root(|tree, _| {
            tree.add(Button::new(tr!(save_as())).rich_tooltip("save-as"))
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
use teksilo::prelude::*;

Button::new(tr!(province_info()))
    .composite_tooltip(
        VStack::new()
            .spacing(8.0)
            .child(TextWidget::new(tr!(province_header())).style(TextStyleRole::BodyBold))
            .child(ProgressBar::new(prosperity_signal))
            .child(
                Grid::new()
                    .columns(vec![TrackSize::Auto, TrackSize::Auto])
                    .child(TextWidget::new(tr!(food())))
                    .child(TextWidget::new(lit!(food_value)))
                    .child(TextWidget::new(tr!(trade())))
                    .child(TextWidget::new(lit!(trade_value))),
            ),
    );
```

The dwell-to-sticky machinery is reused from rich tooltips: at 2 s the role
flips `Tooltip → Dialog`, dismiss swaps to `EscapeOrClickOutside`, and the
surface becomes Tab-reachable. Rare interactive descendants (a "Pin"
button, an internal `TabWidget`) work cleanly post-promotion.

The default delay is `theme.motion.tooltip_delay_heavy` (700 ms — slower than
the 500 ms `tooltip_delay` used by plain/rich tooltips, because composite
surfaces are heavier and shouldn't pop on transient hover). Default
`max_width` × `max_height` are
both 480 dp (`COMPOSITE_TOOLTIP_MAX_WIDTH` / `COMPOSITE_TOOLTIP_MAX_HEIGHT`
constants in `teksilo-widgets/src/styles/recipe_tooltip_style.rs`),
configurable per-instance with `.max_width(f32)` / `.max_height(f32)` on
`CompositeTooltipWidget`.

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
| `.tooltip(text)` / `.tooltip(lit!(text))` | plain text | rich source, composite body |
| `.rich_tooltip(key)` / `.rich_tooltip_content(c)` | rich source | plain text, composite body |
| `.composite_tooltip(w)` | composite body | plain text, rich source |

This is preserved across every widget that exposes the tooltip flavors —
which is now essentially every interactive control:

- **Buttons:** `Button`, `IconButton`, `CommandLinkButton`, `SplitButton`
  (separate `.tooltip(...)` and `.chevron_tooltip(...)` matrices),
  `PopoverWidget` / `Popover`, `NotificationCenterButton`.
- **Inputs:** `TextInput`, `PasswordField`, `SearchField`, `SpinBox`,
  `TextScaleControl`, `ComboBox`, `HexColorInput`, `FilePickerField`,
  `DateEdit` / `TimeEdit` / `DateTimeEdit` / `DateRangeEdit`,
  `ColorEdit` / `ColorPicker` / `ColorSwatch`.
- **Selection controls:** `Checkbox`, `RadioButton`, `Toggle`, `Slider`,
  `SegmentedControl` (per-`Segment`).
- **Misc controls & rows:** `Avatar`, `Badge`, `Breadcrumb`, `Stepper`,
  `StandardListItem` / `StandardTreeItem`, `ToolBox`, `Link`, `MenuItem`.
- **Presets that forward to an inner control:** `ThemeSwitcher`,
  `LanguageSwitcher` (both forward onto their inner `ComboBox`).
- **Data / command delegates:** `TabInfo` / `TabDelegate` for tab strips,
  `ToolbarAction` for `Toolbar` commands.

**`Clone` value types** (`Segment` via `SegmentedControl`, `ToolbarAction`)
are stored by value in a `Vec` and cloned, so they cannot hold a
`Box<dyn Widget>` (which is not `Clone`). Their `.composite_tooltip(...)`
therefore takes a **factory closure** — `impl Fn() -> Box<dyn Widget>`
(stored as an `Rc`, invoked once per build to produce a fresh body) —
rather than an `impl Widget` instance. The plain and rich setters are
unaffected (`LocalizedString` and `RichTooltipSource` are both `Clone`).

**Not applicable:** `Toast` is a presentable *request builder*, not a
`Widget` — it has no `build()` or visible root of its own. Its tooltip is
stored as data and rendered by `toast/surface.rs`; the multi-flavor
setters don't apply to it.

### Tooltips and a control's own overlay

A control that opens an overlay (the `ComboBox` dropdown, a `Popover`, a
date picker's calendar) keeps that overlay's content as an *arena child*
of the trigger. A tooltip on the trigger (or any ancestor) is suppressed
while the pointer is over that overlay's content: `tooltip_pointer_enter`
only fires when the hovered widget is within the anchor's scope **and** no
active-overlay boundary separates them (`WidgetTree::tooltip_hover_targets_anchor`).
So opening a dropdown and hovering its rows never re-triggers the combo's
tooltip — while a tooltip attached to a widget *inside* the overlay (e.g. a
dropdown row's own tooltip) still fires.

---

## Registration: `TeksiloAppBuilder::register_tooltips`

The application's tooltip catalog is a single `Vec<TooltipContent>` registered
once at boot. The bundle is frozen into a thread-local `TooltipRegistry` *before
the first frame builds* — both `run()` and `build_headless()` install it
before invoking the root builder, so tooltip widgets created during the very
first build can resolve their content immediately.

```rust
TeksiloAppBuilder::new()
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
strings only show up in tests and demos via `lit!(...)`.

### URL scheme inside the body

Inline links inside body text use the `:key` prefix to address other tooltip
entries:

```rust
TooltipContent::new(
    "autosave",
    tr!(autosave_with_link()),    // "Teksilo autosaves. See [details](:autosave-details)…"
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
`menu_item.rs`). Each child is marked a *cascade child*, which **suppresses
its dwell-to-sticky indicator** and reads as a persistent, focusable
`Role::Dialog` (advertising `Focus`) straight away — it's opened by an
explicit click and is already persistent, so the hover-to-sticky affordance
and the ephemeral `Role::Tooltip` don't apply.
Clicking a link activates the matching child and calls `ctx.show_overlay(...)`
anchored to the parent tooltip's own widget id, positioned 8 px below it,
with dismiss behavior `EscapeOrClickOutside`.

The request passes `parent_overlay: None`, but the event dispatcher fills it
in with the containing tooltip's overlay (`overlay_ancestor_for_widget`), so
the child is linked to its parent: dismissing the parent cascade-closes the
whole subtree (`OverlayManager::dismiss_immediate`'s BFS), while Escape
dismisses the top-most level first. A runaway cascade — e.g. a cyclic
`A → B → A` `:key` loop — is bounded by `MAX_OVERLAY_NESTING_DEPTH`, so the
overlay stack can't grow without limit.

---

## Attaching tooltips inside `build()`

Most widgets expose `.tooltip(...)` / `.rich_tooltip(...)` builder methods
that wire everything internally. When you author a custom anchor you reach
the same machinery through `BuildContext`:

```rust
// Plain tooltip — caller-managed delay.
let tooltip_id = ctx.add(TooltipWidget::new(tr!(save_hint())));
let delay = ctx.theme().motion.tooltip_delay;
ctx.attach_tooltip(anchor_id, tooltip_id, delay);

// Rich tooltip from the registry — recommended path.
let delay = ctx.theme().motion.tooltip_delay;
crate::tooltip::attach_rich_tooltip(ctx, anchor_id, "save-as", delay);

// Rich tooltip from inline content (no registry lookup).
let delay = ctx.theme().motion.tooltip_delay;
crate::tooltip::attach_rich_tooltip_content(
    ctx,
    anchor_id,
    TooltipContent::new("inline", tr!(inline_body())),
    delay,
);

// Source-driven — accepts either a key or an inline content.
let delay = ctx.theme().motion.tooltip_delay;
crate::tooltip::attach_rich_tooltip_source(
    ctx,
    anchor_id,
    source, // RichTooltipSource
    delay,
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

All tooltip dwell delays are theme-defined on `MotionTokens`
([motion.rs](../crates/teksilo-tokens/src/motion.rs)), so apps retune the feel
in one place. Each widget reads the value at `build()` time via
`ctx.theme().motion.*`.

| Path | Field | Default |
|------|-------|---------|
| Plain + rich tooltips (all widgets) | `motion.tooltip_delay` | `500 ms` |
| Composite tooltips + scene-item tips | `motion.tooltip_delay_heavy` | `700 ms` |
| Subsequent tip while a tip is open / just dismissed | `motion.tooltip_reshow_delay` | `100 ms` |

Defaults match desktop OS norms (Windows `TTDT_INITIAL` / GTK
`gtk-tooltip-timeout` ≈ 500 ms; Windows `TTDT_RESHOW` ≈ 100 ms). Themes
(including Material 3) inherit `MotionTokens::default()` unless they
override `motion`.

The reshow shortening is **proportional, not a floor**. Windows derives
`TTDT_RESHOW` as `TTDT_INITIAL / 5`, and the two tokens encode exactly that
ratio (100 ms of 500 ms); `effective_tooltip_delay` applies the ratio rather
than clamping to the absolute value. So on the warm path a 500 ms entry
reshows at 100 ms and a 700 ms *heavy* entry at 140 ms — a heavier surface
keeps the proportionally longer statement of intent it exists for, instead of
collapsing to the light tier's 100 ms.

Two things deliberately do **not** keep a session warm: a pinned (sticky)
tooltip, which would otherwise hold every other anchor on the 100 ms path for
as long as it stays up, and — since the grace is 1 s — any hover that starts
more than a second after the last tip closed.

Widgets that need a custom value pass an explicit `Duration` to
`attach_tooltip`.

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

1. **Hover enter** (`tooltip_pointer_enter`). The **innermost** entry whose
   `anchor_id` contains the entered widget records `hover_start = now` and the
   pointer position as `hover_origin`. No overlay yet. Only one entry arms: a
   row inside a panel that both carry tooltips would otherwise mature two tips
   and stack them on top of each other, so the most specific anchor — measured
   by arena depth, not attach order — wins.
2. **Stationary filter** (`tooltip_pointer_moved`). While the tip is still
   pending, moving more than ~4 logical px from `hover_origin` restarts the
   timer (Windows-style hover-tracking slop). Intentional pause, not
   fly-by, shows the tip.
3. **Delay tick** (`process_tooltips` / `process_tooltips_real`). Each entry
   whose elapsed time since `hover_start` ≥ the *effective* delay is shown.
   Effective delay is the entry's `delay`, scaled by the theme's
   `tooltip_reshow_delay : tooltip_delay` ratio while a tooltip session is
   active (any non-sticky tip currently shown, or within `TOOLTIP_SESSION_GRACE`
   = 1 s of the last dismiss). An entry whose content announces no text is
   skipped rather than opening an empty bubble. On show: `arena.activate` on
   the dormant content, `show_overlay` with placement
   `NearAnchor { offset: (0, 8) }` and dismiss behavior
   `PointerLeave { delay: 100 ms }`. The `shown_at_sim` / `shown_at_real`
   timestamps are recorded; the optional `shown_at_sink` is updated.
4. **Fade-in.** Tooltips fade in over `MotionTokens::duration_fast` (~120 ms).
   Reduced-motion users get an instant snap (no fade animation), and so does
   the warm reshow path — a 120 ms fade would cost more than the ~100 ms the
   shortened delay just saved.
5. **Hover leave** (`tooltip_pointer_leave`). Pending timers are cancelled.
   Shown non-sticky tips stay until the overlay stack's 100 ms
   leave-grace (WCAG 1.4.13 Hoverable). Sticky tooltips (post-promotion)
   survive — the user dismisses them via `EscapeOrClickOutside`.

### Suppression and dismissal

Beyond hover-leave, four things retire a tooltip:

| Trigger | Pending dwell | Shown non-sticky | Sticky |
|---|---|---|---|
| `PointerDown` anywhere (`tooltip_pointer_press`) | cancelled | dismissed | kept |
| Drag session active (`tooltip_cancel_pending_dwell`) | cancelled | — | kept |
| Window deactivated (`tooltip_window_deactivated`) | cancelled | dismissed | kept |
| <kbd>Escape</kbd> (`try_dismiss_top_on_escape`) | — | dismissed | dismissed |

A press means the user already knows what the control does, so a tip must not
pop *after* the click that answered it, nor sit over what was just clicked —
Windows and GTK both behave this way. A drag owns the pointer, and
`process_tooltips_real` runs from the layout pass the drag keeps driving, so
dwells are cleared each pass rather than left to ripen into a stray overlay.

<kbd>Escape</kbd> scans the overlay stack top-down for the first
Escape-dismissible overlay instead of consulting only the top. That satisfies
WCAG 2.2 SC 1.4.13(a) *Dismissible* for hover content, and stops a tooltip
raised over an open menu from swallowing the keystroke meant for the menu
underneath. `Manual` overlays remain opaque to the scan.

### Waking the event loop

`WidgetTree::next_timer_deadline()` folds every timing source the tooltip
machinery needs the loop to wake for: the pending-tooltip delay, the per-step
dwell wake-ups, delayed overlays, auto-dismiss, **and the `PointerLeave`
leave-grace**. That last one matters as much as the first: the pointer's final
motion event only *starts* the 100 ms grace, so without a deadline for its end
the loop would sit in `ControlFlow::Wait` and the tooltip would stay on screen
until some unrelated input redrew the window. See
[idle-and-animation.md](idle-and-animation.md).

### Entry lifetime

`attach_tooltip*` is called from `build()`, so it re-runs on every rebuild.
**An anchor owns at most one tooltip**: attaching retires any previous entry
for that anchor and destroys its content subtree, and destroying a widget
reaps the tooltip it anchored. Without both, the entry table would grow by one
dead row (plus one orphaned arena node, since `ctx.add` creates a parentless
one that the rebuild teardown never reaches) on every rebuild — and that table
is scanned on every pointer move, four times per layout pass, on every
event-loop wake, and once per widget during the accessibility walk.

---

## Sticky-on-dwell (rich tooltips)

Rich tooltips opt into a 2-second dwell timer that promotes a hover-shown
tooltip into a focusable, click-through Dialog. Promotion advertises focus
but does **not** steal it: the panel becomes focusable and AT-reachable and
the user Tabs in (the correct non-modal-panel pattern) — whatever the user
was doing keeps keyboard focus. The threshold lives in
[`DWELL_PROMOTION`](../crates/teksilo-widgets/src/tooltip/rich.rs) and is
`Duration::from_secs(2)`; it's split into 4 visible quarters of 500 ms each,
driving the [`DwellIndicator`](../crates/teksilo-widgets/src/tooltip/dwell_indicator.rs)
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
reaches assistive tech through the described control's accessible
**description**, wired in the AccessKit pass, which is the W3C-recommended
pattern for supplementary hints. That is the whole of what makes the
asymmetry acceptable, so it has to actually land on the control.

### Which node the description lands on

Not necessarily the anchor. A composing control (`Button`, `Toggle`, and
the two dozen widgets shaped like them) hangs the tooltip *overlay* off an
inner chrome node — the thing with the right bounds to open against — while
its role, its name and its focusability stay on its own outer node.
Emitting the description on the anchor put it on an unnamed box beside the
control: present in the tree, attached to nothing anyone reads. Since a
plain tooltip is never auto-shown on focus, that description **is** the
entire non-pointer path for the tier, and it reached nobody.

So every `BuildContext::attach_tooltip*` records the widget that was
building as the tooltip's `description_owner_id`, and the AccessKit pass
honours it — but only where that node is *unambiguously* the one being
described. Three ways a claim is declined, each falling back to the anchor,
which is where the description sat before any of this and so is always safe:

- **Contested.** One `build()` can attach many tooltips, and `self_id()` is
  the same for all of them: a list body pane attaches one per visible row,
  every one naming the pane. Granting that would put one row's text on the
  pane and lose every other row's outright. A contested claim is no claim.
  `SplitButton`, which tooltips its main region and its chevron separately,
  declines for the same reason and keeps both on their own regions.
- **Anonymous.** A widget whose own `accessibility()` leaves it a
  content-free `GenericContainer` is not a node a description can be read
  from — `TextInput` says so out loud, keeping the real role on an inner
  field — so it does not get to hold one.
- **Already spoken for.** A description the widget wrote itself
  (`MenuItem::trailing_hint`) is specific where a tooltip's is
  supplementary, and both land in the one scalar field. The specific wins.

Overlay placement, hover hit-testing, dwell, focus promotion and retirement
all go on reading `anchor_id`. Only the accessibility walk reads the owner.

The AccessKit pass emits one of two forms, depending on whether the tooltip
is currently on screen:

- **Shown** — `described_by` pointing at the live tooltip content node, the
  richer relation, since the node is genuinely in the tree and navigable.
- **Not shown** — the content's announced text copied onto the anchor as a
  static `description`, harvested from the content widget's own
  `accessibility()` (all three tiers publish their body as the node *name*).

The second form is what actually carries the majority tier. A `described_by`
relation can only reference a node that exists in the emitted tree, and a
dormant tooltip's content is excluded from it — so gating the wiring on "is
the overlay shown" left plain tooltips, which are never auto-shown on focus,
with no screen-reader path at all. The text is read at walk time, so a locale
change or a `Signal<String>` swap is picked up by the same AT re-walk that
already tracks both.

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

Tooltip layout constants are defined in
[`teksilo-widgets/src/styles/recipe_tooltip_style.rs`](../crates/teksilo-widgets/src/styles/recipe_tooltip_style.rs):

```rust
pub const TOOLTIP_PADDING_HORIZONTAL: f32 = 10.0;
pub const TOOLTIP_PADDING_VERTICAL: f32   = 6.0;
pub const TOOLTIP_CORNER_RADIUS: f32      = 8.0;
pub const TOOLTIP_MAX_WIDTH: f32          = 320.0;
pub const TOOLTIP_SHADOW_DENSITY: f32     = 1.0;

// Composite variant:
pub const COMPOSITE_TOOLTIP_PADDING_HORIZONTAL: f32 = 12.0;
pub const COMPOSITE_TOOLTIP_PADDING_VERTICAL: f32   = 12.0;
pub const COMPOSITE_TOOLTIP_CORNER_RADIUS: f32      = 8.0;
pub const COMPOSITE_TOOLTIP_MAX_WIDTH: f32          = 480.0;
pub const COMPOSITE_TOOLTIP_MAX_HEIGHT: f32         = 480.0;
pub const COMPOSITE_TOOLTIP_SHADOW_DENSITY: f32     = 0.7;
```

Per-instance overrides on `CompositeTooltipWidget`: `.max_width(f32)` /
`.max_height(f32)`. Apps that need different global defaults can install a
custom `impl TooltipStyle` via `theme.style_slots.tooltip`.

Color tokens (`Theme::colors`):

| Token | Role | Default (light + dark — both intentionally dark) |
|-------|------|-----------------|
| `tooltip_bg` | `SurfaceRole::TooltipBg` | `#1E1F22` |
| `tooltip_text` | `TextRole::TooltipText` | `#DFE1E5` |
| `tooltip_border` | `BorderRole::TooltipBorder` | `#393B40` |
| `tooltip_shortcut` | `TextRole::TooltipShortcut` | `#9DA0A8` |

Int UI's house style: tooltip surfaces stay dark in both light and dark themes
for high-contrast popups (also reused by `Snackbar`). The OS-theme bridge
([`theme.rs`](../crates/teksilo-tokens/src/theme.rs)) lets a host OS override
`tooltip_bg` / `tooltip_text` if the platform exposes corresponding values.

Motion knobs come from `MotionTokens`:

| Token | Default | Used for |
|-------|---------|----------|
| `duration_fast` | 120 ms | Tooltip fade-in (and matching fade-out for sticky dismiss) |

The `RichTooltipWidget` clamps its proposal width to `TOOLTIP_MAX_WIDTH` in
`layout_response` so long bodies wrap rather than stretching the surface
horizontally. Layout uses a `Grid` (Fractional + Auto columns) so the body
text receives a width proposal that excludes the trailing shortcut chip and
the dwell indicator — `HStack + Spacer` would propose the body's natural
single-line width and the chip / indicator would overflow.

---

## Builder API surface (per-widget)

The canonical surface is the **same four methods on every widget** listed
under [Supported widgets](#last-call-wins-setter-matrix) above:

```rust
.tooltip(text)                  // plain — impl Into<LocalizedString>
.rich_tooltip(key)              // rich — registry key, impl Into<String>
.rich_tooltip_content(content)  // rich — inline TooltipContent
.composite_tooltip(widget)      // composite — impl Widget + 'static
```

The last setter wins — calling `.tooltip(...)` after `.rich_tooltip(...)`
clears the rich source, and vice versa.

Exceptions and extras worth knowing:

| Widget(s) | Difference |
|-----------|------------|
| [`SplitButton`](../crates/teksilo-widgets/src/split_button.rs) | Mirrors all four onto its chevron with a parallel `chevron_tooltip` / `chevron_rich_tooltip` / `chevron_rich_tooltip_content` / `chevron_composite_tooltip` matrix. |
| [`SegmentedControl`](../crates/teksilo-widgets/src/segmented_control.rs) (per-`Segment`), [`ToolbarAction`](../crates/teksilo-widgets/src/toolbar.rs) | `Clone` value types: `.composite_tooltip(...)` takes a **factory** `impl Fn() -> Box<dyn Widget>` (not an `impl Widget` instance), since `Box<dyn Widget>` isn't `Clone`. |
| [`TextInput`](../crates/teksilo-widgets/src/text_input.rs), [`PasswordField`](../crates/teksilo-widgets/src/password_field.rs) | Also keep a legacy `rich_tooltip_key(key)` alias predating the canonical `rich_tooltip(key)`; prefer the canonical name. |
| [`TabDelegate`](../crates/teksilo-widgets/src/tab_widget/delegate.rs) | Closure-driven per-tab delegate — `rich_tooltip_key` / `rich_tooltip_content_with` / `composite_tooltip_with` take `Fn(&T) -> …` closures rather than fixed values. |
| [`ThemeSwitcher`](../crates/teksilo-widgets/src/theme_switcher.rs), [`LanguageSwitcher`](../crates/teksilo-widgets/src/language_switcher.rs) | Thin `ComboBox` presets; the four setters **forward** onto the inner `ComboBox`. |
| [`Toast`](../crates/teksilo-widgets/src/toast.rs) | Not applicable — a request builder, not a `Widget`; tooltip is data rendered by `toast/surface.rs`. |

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
        let tooltip = ctx.add(TooltipWidget::new(lit!(text)));
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

- [Overlays in architecture.md](architecture.md) — overlay
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
