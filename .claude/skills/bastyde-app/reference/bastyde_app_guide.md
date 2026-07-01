<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Bastyde — App Developer Reference (for Claude)

> Drop this file into your app repo (rename it `CLAUDE.md`, or reference it from your
> own `CLAUDE.md`) so Claude has accurate context when helping you build a GUI with
> Bastyde. It documents the **public API and usage patterns** you need as a consumer
> of the `bastyde` crate — not the framework internals.

## What Bastyde is

Bastyde is a pure-Rust GUI framework for serious desktop applications: a **retained
widget tree** with SwiftUI-style layout negotiation, signal-based reactivity, AccessKit
accessibility, and a wgpu renderer. You write widgets with one unified `Widget` trait,
wire behavior with attached event handlers, and drive state with `Signal<T>`.

- Rust edition 2024, resolver 3.
- Cross-platform: macOS, Windows, Linux (Wayland + X11).
- Tests run **headless** — no GPU, display server, or Xvfb needed.

## Adding Bastyde

Depend on the **umbrella crate** `bastyde`. It re-exports everything and gates optional
subsystems behind feature flags. Don't depend on the individual `bastyde-*` crates
directly.

```toml
[dependencies]
bastyde = "0.7"
```

Then in code:

```rust,ignore
use bastyde::prelude::*;       // core types, app builder, theme, settings, i18n, geometry
use bastyde::widgets::*;       // Button, VStack, HStack, TextWidget, ListView, ... (NOT in prelude)
```

The widget *builders* (Button, VStack, TextWidget, ListView, …) live in `bastyde::widgets`
and are re-exported flat — the prelude deliberately does **not** pull them in, so you import
the widget set explicitly. (The prelude *does* bring the app-builder install-hook traits —
`install_toast_default()`, `install_inspector_in_debug()`, … — and, with the default `toast`
feature, the `Toast` notification types; `tr!` / `lit!` arrive only with the default `i18n`
feature.)

### Feature flags

The default set is sensible for most apps: `widgets`, `text`, `i18n`, `inspector`,
`toast`, `file-dialog`, `clipboard`, plus Arabic/Hebrew fallback fonts. Notable opt-ins
and opt-outs:

| Feature | Effect |
| --- | --- |
| `widgets` (default) | The whole widget catalog. Text/rich-text widgets compile unconditionally — there is no separate `rich-text` feature |
| `i18n` (default) | `tr!`/`tr_widget!`, `LocalizedString`, locale-aware formatters |
| `inspector` (default) | Debug-only in-app inspector (F12). No-op in release. Pulls in `widgets` |
| `toast` (default) | Toast notifications + notification log |
| `file-dialog` (default) | Native open/save/pick-folder via `rfd`; `file-dialog-trait` for the trait surface only |
| `clipboard` (default) | System clipboard for text widgets — text input is unusable without it |
| `web-view` / `web-view-servo` / `web-view-headless` | Embeddable `WebView` widget (wry by default; Servo additive for Wayland). Off by default |
| `async` / `tokio` / `async-std` | Optional main-thread async executor (`ctx.spawn_local`) + reactor adapters. Off by default |
| `telemetry` | Privacy-respecting analytics wiring + `PrivacySettings` widget |
| `fonts-cjk-sc` / `fonts-thai` / `fonts-all` / `system-emoji` | Extra bundled script fonts / runtime color-emoji fallback |

For a Latin-only minimal build: `bastyde = { version = "0.7", default-features = false, features = ["widgets", "text", "clipboard"] }`. Note this drops the default `i18n` feature, so `tr!` / `lit!` and the `bastyde::i18n` module are **unavailable** — add `"i18n"` to the list if you use them.

## App entry point

Every window — initial or runtime-opened — is described by a `WindowConfig`. There is
no `.window_title` / `.size` / `.root` directly on the builder.

```rust,ignore
use bastyde::prelude::*;

fn main() {
    BastydeAppBuilder::new()
        .theme(intui::light())                 // or intui::dark(); no Theme::default()
        .install_inspector_in_debug()          // F12 inspector in debug, no-op in release
        .initial_window(
            WindowConfig::new()
                .title("My App")
                .size(1200, 800)
                .root(|tree, _state| tree.add(RootWidget::new())),
        )
        .run();
}
```

Open secondary windows from handler code with `ctx.open_window(WindowConfig::new()...)`.
See app-wide behavior (shortcuts, actions) below — it lives **inside the root widget**,
not on the builder.

If you use persistence, add `.app_paths(...)` (or `.application(qualifier, org, app)`) and
`.settings(SettingsBundle::new()...)`. **Builder-call order is irrelevant** — these methods
just store config; the only rule is that `.app_paths`/`.application` must be set before
`.run()` (it panics there if settings need a config dir and none was given).

## The unified Widget trait

One trait for every widget — leaf, container, composite, hybrid. `layout_response` is the
only required method.

```rust,ignore
pub trait Widget: std::fmt::Debug + std::any::Any {   // Any implies 'static
    fn build(&mut self, _ctx: &mut BuildContext) -> Vec<WidgetId> { vec![] }
    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse; // required
    fn place_children(&self, _bounds: Rect, _proposal: SizeProposal,
                      _children: &mut [WidgetPlacement], _ctx: &LayoutContext) {}
    fn paint(&self, _bounds: Rect, _canvas: &mut Canvas, _ctx: &PaintContext) {}
    fn accessibility(&self, _builder: &mut AccessNodeBuilder) {}
    fn children(&self) -> Vec<WidgetId> { vec![] }
    fn clips_children(&self) -> bool { false }
}
```

- **Leaf** (text, rect): `layout_response` + `paint`.
- **Container** (VStack, HStack): `layout_response` + `place_children` + `children`.
- **Composing** (most app widgets): `build` (creates the child subtree) + `layout_response`
  (usually delegates to the root child) + `accessibility`.

`layout_response` returns `LayoutResponse { size, flex, min, shrink }` — wanted `size`, a
grow weight (`flex`), a compression floor (`min`), and a shrink weight (`shrink`). Most
widgets just return a `Size` (auto-converts to fully rigid: `flex = 0`, `shrink = 0`,
`min = size`); use `LayoutResponse::flexible(size, flex)` for grow-bearing widgets and
`LayoutResponse::shrinkable(size, min, shrink)` to opt content into compression under
over-constraint. See the **Layout model** section for the grow/shrink rules.

> **Critical invariant:** in a composing widget, the id you return from `build()`, the id
> you store for `layout_response` delegation, and what `children()` reports must all
> reference the **same** root child. A mismatch silently breaks layout.

### Composing widget skeleton

```rust,ignore
#[derive(Debug)]
struct MyWidget { root: Option<WidgetId> }

impl Widget for MyWidget {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        let root = ctx.add(
            VStack::new().spacing(8.0)
                .child(TextWidget::new(lit!("Hello")).style(TextStyleRole::BodyBold))
                .child(Button::new(lit!("Click")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::Go)))
        );
        self.root = Some(root);
        vec![root]
    }

    fn layout_response(&self, proposal: SizeProposal, ctx: &LayoutContext) -> LayoutResponse {
        self.root
            .and_then(|id| ctx.child_size(id, proposal))
            .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
            .into()
    }
}
```

### Widget insertion APIs

- `tree.add(w)` / `tree.add_child(parent, w)` — at the tree level.
- `ctx.add(w)` / `ctx.add_boxed(Box<dyn Widget>)` — inside `build()`.

## Widget construction patterns

```rust,ignore
// Inline children — .child() takes impl Widget + 'static
VStack::new().spacing(10.0)
    .child(TextWidget::new(lit!("Title")).style(TextStyleRole::BodyBold))
    .child(Button::new(lit!("Save")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::Save)))

// Iterator children
VStack::new().children(items.iter().map(|it| TextWidget::new(lit!(it.name.clone()))))

// Conditional child
container.child_opt(show_extra.then(|| TextWidget::new(lit!("Extra"))))

// Pre-registered child when you need the id
let label = ctx.add(TextWidget::new(lit!("Status")).bind_text(status_signal));
HStack::new().add_child(label)

// Switcher — show one child at a time, driven by Signal<usize>
let page = ctx.signal(0usize);
ctx.add(Switcher::new(page.clone())
    .child(TextWidget::new(lit!("Page 0")))
    .child(TextWidget::new(lit!("Page 1"))))
```

## Layout model

SwiftUI-style two-phase negotiation: parent proposes a size → child responds with its
wanted size → parent places it. All units are logical pixels. Use `Leading`/`Trailing`
(RTL-aware), never Left/Right.

**Flex in stacks:** `HStack`/`VStack` treat each child's wanted size as a floor, then
distribute slack (`bounds − Σ wanted − spacing`) proportional to each child's `flex`
weight (default `0.0` = rigid). `Spacer` and `Expand` carry flex `1.0`.

```rust,ignore
HStack::new()
    .child(Expand::new().flex(1).child(panel_a))   // 1/3 of slack
    .child(Expand::new().flex(2).child(panel_b))   // 2/3 of slack
```

`Expand::new()` defaults to `flex(1)` and stretches its child; default basis is zero
(CSS flex-basis: 0). Call `.respect_intrinsic()` to use the child's natural size as a
floor. `.align_child(Alignment::X)` opts out of fill. `Center::new()` is **not** a
synonym for `Expand::new().align_child(CENTER)`: a bare `Center` sizes to its child on an
unbounded axis (it reports `flex = 0` and does **not** claim a stack's slack), and fills a
bounded one. Wrap it in `Expand` to center *within* leftover space
(`Expand::horizontal { Center { w } }`).

**Shrink (over-constraint):** when children exceed the bounds, the deficit is distributed
across children with `shrink > 0`, never below their `min`. Shrink is opt-in (rigid by
default → overflow). Single-line `TextWidget` opts in natively (truncates with an
ellipsis); wrap arbitrary content with `Shrinkable::new().min_width(40.0).child(w)` to make
it compressible. Controls (`Button`, `Badge`, `ComboBox`, …) are deliberately rigid and
overflow rather than truncate.

**Layout primitives:** `HStack`, `VStack`, `ZStack`, `Grid`, `Wrap`, `Padding`, `Spacer`,
`Center`, `Expand`, `Shrinkable`, `FixedSize`, `MinSize`, `MaxSize`, `AspectRatio`,
`Switcher`, `Divider`, `IconWidget`, `ImageWidget`, `MasonryLayout`, `FormLayout`.

## Signals & reactivity

- `Signal<T>` — the unified reactive type. `Signal::new(v)` for mutable state;
  `signal.map(|v| ...)` for derived. `signal.set(v)` / `signal.get()`.
- Combinators: `a.zip(&b)`, `a.zip3(&b, &c)`; `a.and(&b)` / `a.or(&b)` / `s.not()` on
  `Signal<bool>`. Derived signals dirty-track every upstream root.
- `Prop<T>` — widget property: `Prop::Static(T)` or `Prop::Bound(Signal<T>)`. Builder
  methods accept `impl Into<Prop<T>>`, so you can pass a value or a signal.
- `ColorProp` / `TextStyleProp` — theme-aware prop types. Color-accepting methods take
  `impl Into<ColorProp>`: a `Color`, a role (`TextRole`/`SurfaceRole`/`BorderRole`), a
  `Signal<Color>`, or a typed role signal (`Signal<TextRole>` / `Signal<SurfaceRole>` /
  `Signal<BorderRole>` — there is no generic `Signal<Role>`). **Prefer roles** for
  theme-driven colors; a bare `Color` is frozen.
- Inside `build()`: `ctx.signal(value)` to create, `ctx.effect(&signal, |v| ...)` for a
  scoped effect (auto-cleaned on rebuild).
- `ObserverHandle` is an RAII guard — dropping it removes the callback (no leak).

```rust,ignore
let count = ctx.signal(0i32);
let label = TextWidget::new(lit!("")).bind_text(count.map(|n| format!("Count: {n}")));
let inc = Button::new(lit!("+1")).on_activate_fn({
    let count = count.clone();
    move |_ctx| count.set(count.get() + 1)
});
```

## Event system (attached handlers)

Dispatch is a **preview pass** (root → strict ancestors) then a **bubble pass**
(target → root). Handlers attach via the `WidgetBuilder` blanket impl on any widget:

- `.on_tap()`, `.on_double_tap()`, `.on_triple_tap()`, `.on_long_press()` — take
  `&TapEvent { position, button, modifiers }`. Default acceptance is primary button only;
  widen with `.accept_tap_buttons(...)`.
- `.on_hover()`, `.on_scroll()`, `.on_pointer_event()`.
- `.on_key()`, `.on_key_preview()` (ancestors claim chords before a focused descendant),
  `.on_focus()`, `.on_access_action()`.
- `.focusable(true)`, `.cursor(CursorIcon::Pointer)`.
- `.focus_within(Signal<bool>)` / `.hover_within(Signal<bool>)` — framework writes `true`
  when a **strict descendant** is focused/hovered (drives unified halos around composites).

```rust,ignore
ctx.add(
    MinSize::new(48.0, 48.0).child(content)
        .on_tap(|_ev, ctx| ctx.send_intent(AppIntent::Clicked))
        .on_hover(move |entered, _ctx| interaction.set(if entered { Hovered } else { Idle }))
        .focusable(true)
        .cursor(CursorIcon::Pointer)
)
```

Inside a composing widget you can attach handlers to `self` in `build()` via a
`HandlerSet` + `ctx.apply_self_handlers(handlers)`.

## Actions, Intents & Shortcuts

The input-to-behavior pipeline. Widgets fire **`Intent`s**, ancestor widgets register
**`Action`s** keyed by intent name, and **`Shortcut`s** bind rebindable keystrokes to
intent names. (There is no `AppCommand`/`on_command`.)

```rust,ignore
use bastyde::IntentKind;
use bastyde::prelude::*;

#[derive(Debug, IntentKind)]
enum AppIntent {
    #[name = "app.save"] Save,
    #[name = "app.open"] Open(String),
}

impl Widget for Root {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        ctx.register_shortcut_global(
            Shortcut::new("app.save").name("Save")
                .primary(KeyStroke::ctrl(Key::S)).build(),
        );
        // Unit intent: name match is enough.
        ctx.register_action(Action::new("app.save").on_invoke(|_i, _c| save()));
        // Data-bearing intent: extract the typed variant.
        ctx.register_action(Action::new("app.open").on_invoke(|i, _c| {
            if let Some(AppIntent::Open(path)) = AppIntent::from_intent(i) { open(path); }
        }));

        let btn = Button::new(lit!("Save")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::Save));
        vec![ctx.add(btn)]
    }
}
```

- `ctx.register_shortcut(s)` (widget-scoped) / `register_shortcut_global(s)` (app-wide).
- `ctx.send_intent(AppIntent::X)` from any handler — the enum variant converts to `Intent`.
- Call `AppIntent::from_intent(intent)` only when you need typed fields; unit intents react
  on name alone (so the same handler fires from a shortcut or a `send_intent`). It returns
  `Option<&AppIntent>` (a borrow — no `Clone` bound), so fields come as references. `.cloned()`
  yields an owned value only if `AppIntent` derives `Clone`; otherwise destructure the
  reference in place (e.g. `if let Some(AppIntent::Open(path)) = …` binds `path: &String`).
- `ShortcutSettings::new()` is a pre-built rebind UI widget; menu labels/tooltips use
  `MenuItem::for_shortcut("id")` / `TooltipContent::for_shortcut("id")` to re-render on rebinds.

## Theming

Apps explicitly pick a preset — there is **no** `Theme::default()`:

```rust,ignore
use bastyde::prelude::intui;
let theme = intui::light();   // or intui::dark()
```

`ThemeAppearance::{Light, Dark}` is a required field on every theme. **Theme is reactive**:
`ctx.set_theme(...)` updates an internal signal and dirty-marks nodes — no rebuild, and
focus/scroll/interaction state survive.

**Variants** select a widget's look (Tier 1):

```rust,ignore
Button::new(lit!("Save")).variant(ButtonVariant::Filled)   // Filled/Tinted/Outlined/Plain/Ghost/Link/Destructive
Toggle::new(on).variant(ToggleVariant::Switch)
```

**Roles** name what a value represents and resolve against the active theme at paint time —
use them instead of hard-coded colors:

```rust,ignore
let bg = interaction.map(|s| match s {
    InteractionState::Hovered => SurfaceRole::Hover,
    InteractionState::Pressed => SurfaceRole::Pressed,
    _ => SurfaceRole::Transparent,
});
RectWidget::new().background(bg)
```

For full restyling, every themable widget supports a **style protocol** (escape hatch).
Override per-call (`Button::new(lit!("X")).style(MyStyle)`) or theme-wide
(`theme.style_slots.button = Some(Rc::new(MyStyle))`). `ctx.theme_signal()` /
`ctx.locale_signal()` exist for cases no role covers — use sparingly.

## Animation

Prefer the fluent `ctx.animate()` spec builder — it captures motion tokens and the
reduced-motion preference at build time.

```rust,ignore
// In build():
let width = ctx.animated_signal(300.0);
let slide = ctx.animate().normal().standard();   // duration + easing from theme

// In a handler:
slide.to_or_snap(&width, 0.0);   // tweens, but snaps under prefers-reduced-motion
```

Presets: `.instant()/.fast()/.normal()/.slow()/.collapse()/.sweep()/.duration(d)` for
timing; `.standard()/.linear()/.ease_in_out()` for easing; `.looping()` for continuous.
Apply with `.to(&sig, target)` (always tween) or `.to_or_snap(&sig, target)` (respects
accessibility — use this for almost all UI transitions).

**Ready-made animated wrappers** (in `bastyde::widgets`): `Collapse`, `Fade`, `Pulse`,
`Cycle`, `SmoothSize`, `Crossfade`, `Slide`, `Shake`, `Scale`, `Rotate`, `Blur`,
`Spinner`, `Unroll` (the horizontal sibling of `Collapse` — `Unroll::new(expanded)`,
`UnrollFrom::{Leading, Trailing}`). For overlays, `OverlayRequest::with_fade(duration)`
wires fade-in/out automatically.

## Accessibility overrides

Augment any widget's accessibility from the outside with builder-level `.access_*`
methods (analogous to SwiftUI's `.accessibility*`):

```rust,ignore
use accesskit::{Role, Live};   // import accesskit::Action *qualified* — the prelude also exports an `Action`

Button::new(tr!(save_icon()))
    .access_label(tr!(save()))                 // user-visible strings take impl Into<Prop<String>>
    .access_description(tr!(save_explanation()))
    .access_role(Role::Button)
    .access_shortcut_id("app.save")            // tracks user rebinds
    .access_action(accesskit::Action::ShowContextMenu, |ctx| ctx.send_intent(AppIntent::Menu));

card.access_merge_subtree();        // collapse a composite into one AT element
logo.access_exclude_subtree();      // hide descendants from AT
toast.access_live(Live::Polite);    // status region
```

With the `i18n` feature, `tr!(...)` strings stay locale-reactive in the AT tree. For
intentionally-untranslated AT strings use `lit!("…")` — a bare `&str` won't compile for
these methods.

## Internationalization & formatting

```rust,ignore
use bastyde::prelude::*;   // brings tr, tr_widget, lit, localized, LocalizedString

let label = tr!(welcome(name = user_name));     // compile-time .ftl key + arg checking
ctx.set_locale("fr-FR");                        // reactive: UI + AT re-render; takes impl Into<String>
```

- `tr!` / `tr_widget!` — translated strings; compile-time validated against `.ftl` files.
- `lit!("...")` — an intentionally-untranslated string.
- `tr_signal!` / `tr_signal_widget!` (from `bastyde::i18n`) — `Signal<T>` args inside a
  translated sentence; returns `Signal<String>` re-rendering on arg/locale/hot-reload change.
- Locale-aware formatters: `NumberFormatter` / `BastydeDateTimeFormatter` turn a value (or
  `Signal<T>`) into a `Signal<String>`. `{ NUMBER(...) }` / `{ DATETIME(...) }` inside
  `.ftl` messages render correctly across locales automatically.

## Settings & persistence

In-memory `Signal<T>` / `*Model<T>` handles are the source of truth; disk is a debounced
atomic projection. Three shapes:

- `SettingsStore` — dotted-key K/V for scalars. `store.signal::<T>(key, default)` or
  `store.signal_for(&KEY)` returns a cached `Signal<T>` (same key → same signal).
- `SettingsFile<T>` — typed single-struct persistence with versioned migrations.
- `PersistedListModel<T>` / `PersistedTreeModel<T>` — collections (fine for <1k items).

Plus `MruList<T: MruEntry>` (recents) and `WindowStateService` (per-window geometry,
auto save/restore when a `WindowConfig` has `.id(...)` and the bundle has
`with_window_state(true)`).

```rust,ignore
const FONT_SIZE: SettingsKey<f32> = SettingsKey::new("editor.font_size", || 14.0);

BastydeAppBuilder::new()
    .application("com", "FernTech", "MyApp")               // sets the config dir (any order; needed by .run())
    .settings(SettingsBundle::new().with_window_state(true))
    .initial_window(WindowConfig::new().id("main").title("My App").size(1200, 800))
    .run();

// In a handler/build:
let size = ctx.settings().signal_for(&FONT_SIZE);   // Signal<f32>
size.set(18.0);                                      // schedules a debounced flush
```

`application(...)` panics if no home dir resolves; tests use `AppPaths::for_testing(path)`
and `Duration::ZERO` debounce.

## Reactive data models (`bastyde::data`)

A GUI-agnostic peer layer (the `bastyde-data` crate). Concrete generic typing throughout (no
`QVariant`, no role integers); all handles are `Rc<RefCell<…>>` so `.clone()` = share-by-handle.
Mutations notify observers **after** dropping the borrow (no reactive deadlock). Full reference:
`docs/data-models.md` + `docs/data-source.md` in the framework repo.

**Decide the ownership shape first** — this is the main design choice, not just "which model":

- **View-model owns the collection** (Qt `QStandardItemModel` shape) — hold a built-in
  `ListModel<T>` / `TreeModel<T>` and keep it in sync (`push`/`insert`/`remove`/`move_item`/
  `replace_all`). Right for bounded, in-memory, fully-resident data.
- **Domain owns the data** (Qt `QAbstractItemModel` shape) — implement `ListDataSource` /
  `TreeDataSource` **directly** over your store (DB cursor, Qleany entity store, paged feed);
  no second in-memory copy to sync, identity is your own domain key. Bind with
  `ListView::from_source` / `TreeView::from_source` (add `_keyed` for `KeyedSelectionModel`).
  A first-class path, **not** a mere "escape hatch for huge sources" — reach for it whenever
  the truth lives elsewhere. The built-in models *are* sources, so both shapes feed the same
  widgets.

The pieces:

- `ListModel<T>` (flat, in-memory) / `TreeModel<T>` (shape) + `TreeSlice<T>` (per-view
  flattening + independent expand state — `TreeModel` is **not** itself a `TreeDataSource`,
  wrap it). Projections: `SortFilterListModel<T>` / `SortFilterTreeModel<T>` (`TreeFilterMode`:
  `HideNonMatching` / `KeepAncestors` / `KeepDescendants`).
- **Source traits** `ListDataSource` / `TreeDataSource` — `type Item` + `type Key: ItemKey`; a
  small required read surface plus **defaulted** capability methods for drag-and-drop (`drag` /
  `can_accept` / `accept_drop`) and lazy/windowed loading (`row_state` / `request_window` /
  `fetch_more`, `DataChange::WindowLoaded`). Non-object-safe → consumed generically via `from_source`.
- **Selection is a separate concern:** `SelectionModel` (index-based; `Single`/`Multi`/`None`,
  `selection_signal()`, Shift+click anchor) vs `KeyedSelectionModel<K>` (identity-based —
  survives reorder/filter/window-slide, consistent across two views of one source).
- **Checkedness is another orthogonal axis:** `CheckedModel` / `TreeCheckedModel<T>` — per-row
  checkbox state with descendant→ancestor tristate aggregation. `CheckState` is
  `Unchecked`/`Checked`/`Indeterminate`.
- Change notifications: `DataChange` / `TreeChange`; `NodeId` is stable across mutations —
  store IDs/keys, never indices, in long-lived state.

Feed these into `ListView` (virtualized) / `Repeater` (bounded, non-virtualized, ≤~100) /
`TreeView` / `TableView` / `TreeTableView` / `GridView`; rows via `StandardListItem` /
`StandardTreeItem`. **A dynamic list/tree/table is always one of these widgets bound to a model
or source — never a hand-rolled `for … .child(…)` loop.**

## Widget catalog (quick reference)

Import from `bastyde::widgets`. The main families:

- **Controls:** Button, IconButton, CommandLinkButton, PopoverButton, SplitButton,
  Checkbox, RadioButton, RadioTile / RadioTileGroup (selectable-card radios; N-ary group with
  `TileLayout::{Row, Grid, Column, Vertical}` — the last is a compact fixed-height settings list),
  Toggle, Slider, ComboBox, SegmentedControl, ProgressBar, Spinner, Link, Badge, SpinBox, Avatar.
- **Containers:** Panel, Card, Accordion, ToolBox, ScrollArea, ScrollBar, Splitter,
  DockingLayout, TabWidget / TabBar, Dialog, Popover, Snackbar, GroupBox, Wizard,
  Breadcrumb, MessageBox, DropZone, DropTarget, Toast.
- **Menus:** MenuBar, MenuList, MenuItem (Plain/Check/Radio modes, `&`-mnemonics). Context
  menus are wired via `.context_menu(...)` builder methods / `ContextMenuFactory`, plus the
  declarative `MenuModel` (shared by the in-window bar and the native OS menu bar).
- **Chrome:** Toolbar, StatusBar, TitleBar, GroupHeader.
- **Data-driven:** ListView, TreeView, Repeater, GridView, TableView (multi-column,
  virtualized, sort/filter, drag-resize/reorder columns, pinned columns, keyboard nav, cell
  edit hooks), TreeTableView, StandardListItem, StandardTreeItem.
- **Text:** TextInput, PasswordField (secure entry), RichTextEditor — call
  `RichTextEditor::read_only(doc)` for a read-only viewer (there is no separate
  `RichTextViewer` type; `RichTextEditor::editor(doc)` is the editable constructor).
- **Rendering primitives:** RectWidget, TextWidget.
- **Tooltips:** three tiers — plain (`.tooltip(...)`), rich (`.rich_tooltip(key)`),
  composite (`.composite_tooltip(widget)`). Setters are mutually exclusive (last wins).
- **Charts** (`bastyde-charts`): BarChart, LineChart, PieChart.
- **Scene** (`bastyde-scene`): pannable/zoomable viewport for corkboards, mind maps,
  node graphs, CAD-style canvases.

> **Charts and Scene are separate crates, NOT re-exported by the `bastyde` umbrella.** Add
> `bastyde-charts` / `bastyde-scene` as direct dependencies (version them alongside `bastyde`)
> and import from those crates — they are *not* reachable via a `bastyde::` path.

To inspect a widget's exact public surface, ask Claude to read the widget source in the
Bastyde repo, or use the framework's `tools/extract_widget_api.py` if you have the checkout.

## Toasts & notifications

With the `toast` feature (default), one line wires the host + archive + bell glyph:

```rust,ignore
BastydeAppBuilder::new()
    .install_toast_default()
    /* ... */;

// From any handler:
ctx.show_toast(Toast::success(lit!("Saved")).action(ToastAction::new(lit!("Undo"), |c| c.send_intent(AppIntent::Undo))));
```

Severities: `info`/`success`/`warning`/`error`/`loading`. Action constructors are
`ToastAction::new(label, cb)` (default Link style), `::primary`, and `::destructive`.
`Toast::id(...)` updates in place. A persistent `NotificationLog` / `NotificationCenterButton` / `NotificationLogDialog`
family backs the bell.

## Drag-and-drop

In-app and OS-level drops share one pipeline. Attach `.on_drag_hover` / `.on_drag_leave` /
`.on_drop`, or use the ready-made `DropZone` (standalone "drop files here") and `DropTarget`
(wraps any child, keeps it visible). External-OS DnD needs
`BastydeAppBuilder::install_external_dnd()`. An OS drop arrives as a `DragPayload` with
`origin() == DragOrigin::External` carrying `files()` / `text()` / `uris()`.

## The `bati!` DSL

`bati!` is an **optional** block-structured macro for widget trees. It desugars 1:1 to the
builder calls you'd otherwise write — no runtime, no virtual tree, byte-for-byte the same
output. It earns its keep on **deep, nested trees** where builder chains (`.child(...).child(...)`)
get hard to scan; for a flat two- or three-widget tree the plain builder API is just as
clear. The two are fully interchangeable and nest in either direction, so adopt it where it
helps and ignore it where it doesn't.

**Two invocation forms** (the `ctx =>` preamble names your `BuildContext` local; `=>` is
literal):

```rust,ignore
bati!(ctx => VStack { /* … */ })   // inserts the root via ctx.add → returns WidgetId
bati!(VStack { /* … */ })          // returns a widget value → pass to .child(...) / a slot
```

**The same tree, both ways** — identical expansion:

```rust,ignore
// Plain builder
VStack::new().spacing(10.0)
    .child(TextWidget::new(lit!("Title")).style(TextStyleRole::BodyBold))
    .child(Button::new(lit!("Save")).on_activate_fn(|ctx| ctx.send_intent(AppIntent::Save)))

// bati!
bati!(ctx =>
    VStack {
        spacing: 10.0
        TextWidget(lit!("Title")) { style: TextStyleRole::BodyBold }
        Button(lit!("Save")) { on_activate_fn: |ctx| ctx.send_intent(AppIntent::Save) }
    }
)
```

**Reading rules:**

- **Element head** — `Button(lit!("Save"))` → `Button::new(lit!("Save"))`. An UpperCamel head
  gets `::new` appended automatically; a bare type (`VStack`) → `VStack::new()`; an explicit
  lowercase constructor is emitted as-is (`Padding::uniform(24.0)`, `ProgressBar::indeterminate()`).
  Whatever sits in `(…)` is passed **verbatim** to the constructor, so the head form (bare vs
  explicit `::new`) is independent of the argument. Note labels are `LocalizedString` under the
  default `i18n` feature: wrap them in `tr!(key())` (translated) or `lit!("text")` (untranslated)
  — a bare `&str` does **not** convert (see Internationalization above).
- **`name: value`** → `.name(value)`. Multi-arg `border: color, 2.0` → `.border(color, 2.0)`.
  A bare lowercase word is a zero-arg call: `fills_stack` → `.fills_stack()`.
- **Bare child** → `.child(...)`, for the layout/container widgets that have a `.child()`
  (VStack, HStack, ZStack, Padding, Expand, Panel, ScrollArea, Toolbar, GroupBox, …).
  Properties and children interleave freely; body items are **newline-separated** (commas
  between them are optional).
- **Handlers are closures, verbatim** — `on_tap: |ev, ctx| …`, `on_activate_fn: |ctx| …`,
  `on_hover: move |entered, ctx| …`. `move`, capture, and arity stay exactly as written, and
  handler methods are auto-moved to the end of the chain so you can place them in any order.

> The single most common mistake: it's `on_activate_fn: |ctx| ctx.send_intent(AppIntent::Save)`
> — a **closure that fires the intent**, not `on_activate: AppIntent::Save`. Handlers are
> always closures.

**Named-slot widgets.** Card, Dialog, TabWidget, Popover, Snackbar, Accordion, Breadcrumb,
TitleBar (and friends) take content by **named slot**, not by bare child — a bare child there
is a compile error that names the slot you meant:

```rust,ignore
bati!(ctx =>
    Card {
        header: TextWidget(lit!("Title")) { style: TextStyleRole::BodyBold }
        content: VStack {
            spacing: 8.0
            TextWidget(lit!("Line one"))
            TextWidget(lit!("Line two"))
        }
        footer: Button(lit!("OK")) { on_activate_fn: |ctx| ctx.send_intent(AppIntent::Ok) }
        padding: 16.0
    }
)
```

**Bind an id** with `name = Element` when a later item — usually a handler closure — needs to
reference that widget. It hoists a `let name = ctx.add(Element)` and stays in scope for the
rest of the block (in a slot it routes to the slot's `*_id` twin):

```rust,ignore
bati!(ctx =>
    Card {
        header: title = TextWidget(lit!("Manuscript")) { style: TextStyleRole::BodyBold }
        content: Button(lit!("Focus title")) {
            on_tap: move |_, ctx| ctx.focus(title)     // `title` is in scope here
        }
    }
)
```

To splice an **already-registered** id, use the `#{ id }` escape (`→ .add_child(id)`), or the
plain property forms `add_child: id` / `child_id: id` / `<slot>_id: id`.

This is also the standard escape hatch when a child needs a builder chain `bati!` can't
express inline (a multi-arg constructor plus method calls — `bati!` has no method-chain form
at a property/child head): pre-register it with `ctx.add(...)`, then splice the `WidgetId`.
The stock `widget-catalog` example does exactly this for its richer controls:

```rust,ignore
let confirm = ctx.add(
    Button::new(tr!(confirm()))
        .icon(IconWidget::checkmark(16.0), IconLocation::Leading)
        .variant(ButtonVariant::Filled),
);
bati!(ctx =>
    HStack {
        spacing: 8.0
        #{ confirm }                                      // → .add_child(confirm)
        Button(lit!("Cancel")) { on_activate_fn: |ctx| ctx.send_intent(AppIntent::Cancel) }
    }
)
```

**Structural forms** let conditionals and iteration stay inside the block instead of forcing
you back to builder syntax:

| Form | Desugars to |
| --- | --- |
| `if c { E }` | `.child_opt(if c { Some(‹E›) } else { None })` |
| `if c { A } else { B }` (≤ 4 arms) | `.child(BatiBranch::…)` |
| `match x { p => E, … }` (2–4 arms) | `.child(match x { … })` |
| `for p in it { let …; E }` | `.children(it.map(\|p\| ‹E›))` |
| `let x = …;` at body | a local for the following body items |
| `..ids` | splices an iterator of `WidgetId` as children |
| `rust { … }` | imperative escape — trailing expr → child, trailing `;` → side-effect |

```rust,ignore
bati!(ctx =>
    VStack {
        if items.is_empty() {
            TextWidget(lit!("Nothing here yet"))
        }
        for item in items.iter() {
            let id = item.id;                          // owned capture for the move closure
            Button(lit!(item.title.clone())) {
                on_tap: move |_, ctx| ctx.send_intent(AppIntent::Select(id))
            }
        }
    }
)
```

**Gotchas worth knowing up front:**

- `if` / `match` cap at **4 arms** — beyond that, call a helper returning `Box<dyn Widget>`.
- `if signal { … }` does **not** auto-wire reactive visibility (the macro never invents
  reactivity the builder doesn't have). Bind `visible_when(id, signal)` on a registered child
  yourself.
- A Rust **struct literal** as a property value must be parenthesized — `prop: (MyStruct { … })`
  — because an unparenthesized `{ … }` is parsed as a `bati!` element. Enum variants
  (`prop: Type::Variant(x)`) need no parens.
- No method chains in property-arg position: write `item: MenuItem::new(lit!("x")) { on_activate_fn: cb }`
  (body form), not `item: MenuItem::new(lit!("x")).on_activate(cb)`.

The best way to learn `bati!` is to read real trees: the framework ships large, runnable
`bati!` examples (`cargo run -p widget-catalog` is the densest; `simple_button` /
`text-and-layout` are the gentle ones). The `/bati-macro` skill (in the framework repo)
handles read/write/translate/debug requests; the full grammar and desugaring cheat sheet
live in `docs/bati-macro-reference.md` and `docs/bati-language-spec-v3.md` there.

## EventContext capabilities

Inside any handler, `EventContext` (`ctx`) offers ambient mutations:

- `ctx.send_intent(...)`, `ctx.set_theme(...)`, `ctx.set_locale(...)`.
- `ctx.open_window(WindowConfig...)`, `ctx.close_window()`.
- `ctx.with_widget_mut::<W>(id, BindingLevel, |w| ...)` — typed deferred mutation of a
  mounted widget (applied after the handler, then dirty-marked at the given level).
- `ctx.request_accessibility_update()` — force an AccessKit re-walk after restructuring.
- `ctx.settings()`, `ctx.window_state()`, `ctx.mru::<T>()` (with the `SettingsExt` trait).
- `ctx.pick_file(...)` / `ctx.save_file(...)` / `ctx.pick_folder(...)` (file-dialog feature).
- `ctx.spawn_local(...)` / `spawn_blocking(...)` (async feature).

## Testing patterns

Headless — no GPU or display needed.

```rust,ignore
use bastyde::core::WidgetTree;   // test-only types live under bastyde::core, not the prelude

let mut tree = WidgetTree::new();
let id = tree.add(MyWidget::new());
tree.layout(SizeProposal::exact(400.0, 300.0));
assert!((tree.bounds(id).width - expected).abs() < 0.01);
```

Test helpers (not in the prelude): `WidgetTree` and `LayoutContext::for_testing(&theme)`
live under `bastyde::core`; `MockTextBackend::new()` (fixed 8px char width) lives under
`bastyde::canvas`.

### Agent / CI automation (MCP)

Beyond unit tests, an app can be **observed and driven by an AI agent or a CI harness**
through a Model Context Protocol server — in-process, with no OS accessibility layer. It
exposes the live AccessKit tree (so node ids are stable across rebuilds), AT actions,
synthetic pointer/key/IME input, and screenshots.

- **Headless (CI / agent-authored tests):** `bastyde-automation-mcp --headless` — a
  self-contained MCP server, no display or GPU daemon needed (screenshots need a GPU else
  return `GPU_UNAVAILABLE`). Works on every platform.
- **Live app:** enable the `automation` feature on the `bastyde` dependency and add one
  builder call:

  ```rust,ignore
  BastydeAppBuilder::new()
      .install_automation_bridge_in_debug()   // debug-only; no-op in release / on Windows
      // … the rest of the chain …
      .run();
  ```

  At startup it prints a socket path + token to stderr; drive it with
  `bastyde-automation-mcp --connect <sock> --token <uuid>`. The bridge is a debug-only,
  `0600`, per-process Unix socket (Linux/macOS) — a release build contains no socket.

On connect the server hands the client a "how to drive this app" briefing plus a JSON
schema per tool, so a capable agent self-guides through the snapshot → act → settle →
assert loop. Full reference: `docs/automation-mcp.md` in the framework repo.

## Conventions when writing Bastyde code

- **Builder pattern everywhere** — fluent `.child()`, `.spacing()`, `.style()`, `.on_tap()`.
- **Prefer roles over raw colors** so the UI follows theme changes.
- **Keep `build()` return, the stored root id, and `children()` in sync** in composing widgets.
- **Use `Leading`/`Trailing`**, not Left/Right (RTL-aware).
- **Use `thiserror`** for error types (`#[derive(thiserror::Error)]`).
- **`Signal<T>` for state, `Prop<T>` for widget inputs.** Methods accept `impl Into<Prop<T>>`.
- snake_case fns, CamelCase types — standard Rust.

---

*This guide is abridged from Bastyde's internal `CLAUDE.md` and targets app developers
consuming `bastyde` 0.7. For framework internals, source layout, and implementation
status, see the Bastyde repository's own docs (`docs/SUMMARY.md`) and `CLAUDE.md`.*
