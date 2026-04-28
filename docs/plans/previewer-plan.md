# Widget Previewer — Plan

**Status:** Proposed — no code landed yet. This document is the
implementation plan for a Storybook-style component previewer for
FernUI. Authored 2026-04-28.

**Companion docs:**
- [docs/fern-ui-architecture.md](../fern-ui-architecture.md) — overall framework
- [docs/reactive-theme.md](../reactive-theme.md) — `Signal` / `Prop` / `BindingLevel`
- [docs/fern-macro-reference.md](../fern-macro-reference.md) — `fern!` DSL (the source-emitter follow-on)
- [docs/plans/widgets-plan.md](widgets-plan.md) — the catalog of widgets the previewer will host

---

## 1. Goal

Provide a tool that lets developers iterate on widgets — both the
ones shipped in `fern-widgets` and the ones any FernUI-consuming
application builds on top — in **isolation** from any host
application. The tool must:

1. Enumerate widgets from a registry populated at link time.
2. Render any widget at any one of a handful of *named variants* on a
   live canvas, using the same renderer the host application uses.
3. Expose each widget's tweakable properties as a *knobs panel* on the
   side, with mutations driving live repaint via `Signal`s — no
   `cargo build` between knob edits.
4. Switch theme, locale, zoom, and background colour on the canvas
   without leaving the tool.
5. Be launchable both as a standalone browser and targeted at a
   specific widget via CLI args (so editors can wire keybindings to
   "preview the widget defined in the current file").

The model is **Storybook 1.x for FernUI** — variants per component,
auto-generated controls panel, theme/locale toggles, snapshot export.
Not Storybook 8: no addon ecosystem, no visual-regression infra, no
hot-reload of structural code edits in v1.

### 1.1 Audience: every FernUI consumer, not just one app

This is **framework infrastructure**, not application infrastructure.
The `fern-widgets-previewer` binary handles FernUI's own widgets, but
the real point of the architecture is that the GUI lives in the
reusable `fern-preview-ui` library. Any application built on FernUI
— writing apps, IDEs, dashboards, design tools, internal tools, games
with rich UI — gets a previewer for its own composite widgets by:

1. Adding a `preview` feature to its widget crate.
2. Implementing `WidgetCatalog` for each widget behind that feature.
3. Shipping a thin binary crate (`<app>-previewer`) that depends on
   the app's widgets + `fern-preview-ui` and calls
   `run_previewer(PreviewerOptions::from_args())`.

The cost per consumer is one `Cargo.toml` feature line, one binary
crate with a ~10-line `main.rs`, plus the per-widget catalog impls
authored alongside the widget code. No fork, no build-system
contortions, no per-app variant of the previewer GUI.

This shape is also why the `fern-preview` / `fern-preview-ui` /
`fern-widgets-previewer` split (§4.1) is non-negotiable: a monolithic
previewer binary tied to `fern-widgets` would be useless to every
other consumer.

### 1.2 Benefits

This section makes the case for spending the v1 budget (~3 weeks).
Benefits are framed for any FernUI consumer; specific applications
inherit them automatically.

#### Day-to-day development wins

- *Iteration speed on visual details.* Knob change → instant repaint
  via `Signal` (no `cargo build`). The difference between a 5-second
  rebuild loop and a 16ms paint loop is ~300×. Most visual polish
  (colour, size, label, role) happens in this loop.
- *Component isolation.* No need to launch the host app and navigate
  to where a widget lives in order to look at it. A bug like "ComboBox
  dropdown clips at panel boundary" becomes a 30-second reproduction
  in the previewer instead of a 5-minute click-path through the host.
- *Edge-case enumeration as a side effect.* Authoring variants forces
  the developer to enumerate states by name — "disabled", "loading",
  "long label", "empty", "RTL". States that would otherwise only
  surface by accident in production show up in the catalog explicitly.
- *Discoverability.* A widget vocabulary of dozens of components is
  past the size where everyone keeps it in their head. Browsing the
  navigator surfaces widgets developers forgot existed (or never knew
  about because someone else added them).

#### Force-multiplier benefits — the framework itself improves

This is the underrated category.

- *Drives `Prop<T>` coverage across widgets.* For `disabled` to be a
  live knob, `Button::disabled` must accept `impl Into<Prop<bool>>`,
  not a static `bool`. Many widgets aren't fully `Prop`-ified yet.
  Building knob coverage forces the builder API to be uniformly
  reactive — and every consumer reaps that directly.
- *Eats its own dogfood.* The previewer is itself a FernUI app using
  TreeView, SplitView, Accordion, ComboBox, Toggle, Slider, Toolbar.
  Rough edges that no individual application's usage would surface
  show up while the FernUI team uses the previewer daily. Each
  annoyance becomes a framework fix.
- *Theme stress-testing at scale.* Light → Dark → Custom on every
  widget, every variant, in seconds. A large fraction of theme bugs
  (low-contrast hovers, wrong border roles, transparent surfaces over
  wrong backgrounds) only show up under specific role combinations.
  Found once, fixed once, every consumer benefits.
- *Locale stress-testing.* Long-word languages, RTL, and short
  languages exercise layout edge cases that English alone never hits.
- *The `KnobSpec` is a structured property surface.* Once it exists
  per widget, it can feed auto-generated rustdoc supplements,
  design-system documentation, or the `#[derive(WidgetCatalog)]`
  macro later (§9 Phase 7). That's free downstream value for the
  whole ecosystem.

#### Application-side benefits (every consumer)

- *Composite widgets get the same treatment.* `TagManager`,
  `ManuscriptOutline`, `PaymentForm`, `ChartLegend` — whatever an
  application defines on top of FernUI — gets isolated previewing
  through the same machinery. The library split (§4.2) is what makes
  this universal.
- *Designer / non-engineer collaboration.* Show a stakeholder "here
  are the variants we have, here's dark mode, here's RTL" without
  them launching the host app or knowing Rust.
- *PR review artefacts.* Paste PNG snapshots in PR descriptions.
  "Before / After" images in commit messages. Far easier review than
  asking the reviewer to check out the branch and reproduce a state.

#### Compounding effects (why the cost amortises well)

- *Front-loaded cost.* The ~16-day v1 cost is one-time. After that,
  each new widget pays a small ~30-minute catalog tax during normal
  development. No accumulating debt — steady-state maintenance.
- *Variants double as fixture functions for headless tests.* A
  `WidgetCatalog::variants()` function returning sample widgets is
  exactly what layout/render tests want as input. Phase 7
  visual-regression infra will reuse the same fixtures.
- *Cultural norm.* "Every widget has a canonical knob surface" gets
  established alongside the tool. New widgets get scrutinised for
  which properties are tweakable, which are reactive, which have
  sensible defaults. Hard to introduce that discipline retroactively;
  easy to introduce alongside a tool that requires it.

#### What the previewer does *not* do (honest)

- *Won't catch interaction bugs.* Drag-drop, focus traversal across a
  real form, keyboard shortcuts in context — those still need a host
  application.
- *Won't catch performance issues at scale.* Single widget at a time;
  a 10k-row `ListView` won't reveal its slowness here.
- *Won't replace integration tests.* Variants are isolated fixtures,
  not flows.
- *Adds a small maintenance tax.* New builder methods on widgets
  with catalog impls need a corresponding `KnobSpec` update if you
  want them tweakable. Forgettable; not catastrophic.
- *Doesn't reload structural code edits live.* New variant or new
  builder method still needs a rebuild + restart. Knob *value* edits
  are live; *structural* edits are not.

## 2. Non-goals (and why)

- **No QtDesigner-style visual tree editing.** Source remains the
  authority; the previewer reads code, never writes it. A `fern!`
  visual editor is a separate, multi-month project.
- **No click-into-canvas-to-edit-children ("Flavor B").** Inspecting
  live widget instances back to their knob values needs a per-widget
  introspection trait we don't have. Defer until the v1 inspector
  exists and we know whether users actually reach for it.
- **No structural hot-reload.** Adding a builder method, a new
  variant, or a new widget still requires `cargo build`. Knob edits
  within an existing variant are instant. Hot-reload via `libloading`
  or interpreted `fern!` is its own project.
- **No Source panel (paste-back `fern!`) in v1.** The reverse
  generator (builder state → `fern!` text) does not exist
  in [crates/fern-ui-macros/src/lower.rs](../../crates/fern-ui-macros/src/lower.rs);
  the lowering is one-way. A builder-API source emitter is a fallback,
  but a real `fern!` emitter is a separate effort.
- **No visual-regression / Chromatic-style infra.** PNG export is the
  building block, not the diff/baseline/CI workflow.

## 3. Substrate audit (what we depend on, verified)

These were verified against the codebase before writing the plan.

| Claim | Status | Evidence |
|---|---|---|
| Off-screen rendering to texture works | **Confirmed** | `Renderer::render(view: &wgpu::TextureView, ...)` at [crates/fern-render/src/renderer.rs:191](../../crates/fern-render/src/renderer.rs#L191) accepts a generic `TextureView`, not a `Surface`. Format is set at construction in [renderer.rs:67](../../crates/fern-render/src/renderer.rs#L67). |
| Signal change → repaint without rebuild | **Confirmed** | `BindingLevel` variants `RepaintOnly`, `Relayout`, `Rebuild`, `AccessibilityOnly` defined at [crates/fern-core/src/binding.rs:18-35](../../crates/fern-core/src/binding.rs#L18). `Signal::bind_to` at [signal.rs:430](../../crates/fern-core/src/signal.rs#L430) registers per-widget at the chosen level. Test at [signal.rs:928-937](../../crates/fern-core/src/signal.rs#L928) confirms `RepaintOnly` does not invoke `build()`. |
| All target widgets exist | **Confirmed** | TreeView at [tree_view.rs:86](../../crates/fern-widgets/src/tree_view.rs#L86); SplitView at [split_view.rs:500](../../crates/fern-widgets/src/split_view.rs#L500); Accordion at [accordion.rs:100](../../crates/fern-widgets/src/accordion.rs#L100); ComboBox at [combo_box.rs:120](../../crates/fern-widgets/src/combo_box.rs#L120); SegmentedControl at [segmented_control.rs:1](../../crates/fern-widgets/src/segmented_control.rs#L1); Toggle at [toggle.rs:21](../../crates/fern-widgets/src/toggle.rs#L21); Slider at [slider.rs:24](../../crates/fern-widgets/src/slider.rs#L24); Toolbar at [toolbar.rs:13](../../crates/fern-widgets/src/toolbar.rs#L13); Switcher at [switcher.rs:38](../../crates/fern-widgets/src/primitives/switcher.rs#L38). |
| Cargo `feature` is the workspace idiom for opt-in code | **Confirmed** | Existing features: `rich-text` in `fern-widgets/Cargo.toml`, font-bundle features in `fern-text` and `fern-ui`. No `cfg(debug_assertions)` gating in use. |
| `inventory` / `linkme` not present | **Absent** | First-time introduction. Adding `inventory = "0.3"` is uncontroversial but novel for this workspace. |
| Mid-run root swap | **Refuted** | `FernAppBuilder::run()` at [crates/fern-app/src/app.rs:1478](../../crates/fern-app/src/app.rs#L1478) consumes `self`. No public `set_root` / `replace_subtree`. Resolution: previewer architects its root as a custom rebuilding widget that owns the canvas — see §6. |
| Reverse `fern!` generation | **Refuted** | [crates/fern-ui-macros/src/lower.rs](../../crates/fern-ui-macros/src/lower.rs) is one-way (IR → `quote!`). No reverse pretty-printer. Resolution: drop Source panel from v1. |
| `Switcher` supports dynamic content | **Partial** | Children are fixed at `build()` time — see [switcher.rs:38-79](../../crates/fern-widgets/src/primitives/switcher.rs#L38). Resolution: canvas is *not* a `Switcher`; it's a custom widget whose `build()` reads the selected widget+variant signals and constructs the child from the catalog entry on each rebuild. |

## 4. Architecture

### 4.1 Crate layout

Three new crates plus modifications to `fern-widgets`:

```
crates/
  fern-preview/               NEW   trait + types + registry (lib only, no UI)
    src/
      lib.rs
      catalog.rs              WidgetCatalog trait, CatalogEntry erased trait
      variant.rs              PreviewVariant
      knob.rs                 KnobSpec, KnobValues, KnobKind, ColorKnob
      registry.rs             inventory glue, iter_entries(), find_by_id()
      source_loc.rs           SourceLoc { file: &'static str, line: u32 }

  fern-preview-ui/            NEW   the 3-pane GUI as a reusable library
    src/
      lib.rs                  pub fn run_previewer(opts: PreviewerOptions)
      app_state.rs            shared Signals, knob persistence map
      navigator.rs            left pane (TreeView over registry)
      canvas.rs               center pane (custom rebuilding widget)
      inspector.rs            right pane (variant radios + knob form)
      knob_form.rs            KnobSpec → widget tree (Toggle / Slider / etc.)
      toolbar.rs              top bar (theme, locale, zoom, background)
      knob_signals.rs         typed signal pool, `KnobValues` runtime view
      cli.rs                  argument parsing for --widget / --variant / --file

  fern-widgets/               MOD   `preview` feature; per-widget catalog modules
    Cargo.toml                + [features] preview = ["dep:fern-preview"]
    src/
      button.rs               + #[cfg(feature = "preview")] mod catalog;
      ... (every widget)

  fern-widgets-previewer/     NEW   binary; links fern-widgets + fern-preview-ui
    Cargo.toml
    src/main.rs
```

Any FernUI-consuming application replicates the same shape inside
its own workspace:

```
<consuming-app>/
  crates/
    <app>-widgets/            WidgetCatalog impls behind `preview`
    <app>-previewer/          binary linking <app>-widgets + fern-preview-ui
```

Concrete examples — the same shape applies identically:

- A writing app: `skribisto-widgets/`, `skribisto-previewer/`
- A note-taking app: `notes-widgets/`, `notes-previewer/`
- An IDE: `myide-widgets/`, `myide-previewer/`
- An internal tool: `dashboard-widgets/`, `dashboard-previewer/`

Each binary is ~10 lines of `main.rs` calling `run_previewer`. The
inventory is **per-binary**: each previewer sees only the catalog
entries linked into it. That's the correct isolation property — the
writing app's previewer doesn't show the IDE's widgets, and vice
versa — and it's what makes `fern-preview-ui` a true reusable
library.

### 4.2 Why the `lib` / `binary` split

The split (`fern-preview-ui` library + thin binary per consumer) is
the only way an application can preview *its own* widgets. A
monolithic `fern-previewer` binary would hard-link `fern-widgets` and
could never see any other application's catalog impls — making the
tool useless for everyone except FernUI's own widget development.

Cost of the split: one extra crate plus a ~10-line `main.rs` per
consumer. The wrapper binary calls `fern_preview_ui::run_previewer(
PreviewerOptions::from_args() )` and that's it. Cheap for FernUI to
maintain, free for downstream consumers to adopt.

### 4.3 Cargo feature propagation

`fern-widgets` exposes `preview = ["dep:fern-preview"]`. The catalog
modules are `#[cfg(feature = "preview")]`. The previewer binary's
`Cargo.toml`:

```toml
[dependencies]
fern-widgets = { path = "../fern-widgets", features = ["preview"] }
fern-preview-ui = { path = "../fern-preview-ui" }
```

Cargo's feature unification means that whenever the previewer is in
the build graph, `fern-widgets` is compiled with the catalog impls
present. Normal `cargo build` (no previewer) leaves them out. Phase 0
acceptance verifies this works end-to-end before any widget impls are
written.

## 5. Core types

### 5.1 The trait (in `fern-preview`)

```rust
pub trait WidgetCatalog: 'static {
    fn id() -> &'static str;             // "button", "tag_manager"
    fn group() -> &'static str;          // "Controls", "Containers", "Composites"
    fn display_name() -> &'static str;   // human label, may be localised later

    fn variants() -> Vec<PreviewVariant>;
    fn knobs() -> KnobSpec { KnobSpec::empty() }

    fn build(variant: &str, knobs: &KnobValues) -> Box<dyn Widget>;
}

pub trait CatalogEntry: Sync {
    fn id(&self) -> &'static str;
    fn group(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn source(&self) -> SourceLoc;       // file!() + line!() captured at submit
    fn variants(&self) -> Vec<PreviewVariant>;
    fn knobs(&self) -> KnobSpec;
    fn build(&self, variant: &str, knobs: &KnobValues) -> Box<dyn Widget>;
}

inventory::collect!(&'static dyn CatalogEntry);
```

`WidgetCatalog` is the user-facing static-method trait; `CatalogEntry`
is the object-safe erased version `inventory` actually iterates over.
A small macro hides the boilerplate of writing both:

```rust
catalog_entry!(Button);   // generates ButtonEntry + inventory::submit!
```

### 5.2 `KnobSpec` and `KnobValues`

```rust
pub enum KnobKind {
    Bool        { default: bool },
    I32         { default: i32, min: i32, max: i32, step: i32 },
    F32         { default: f32, min: f32, max: f32, step: f32 },
    Text        { default: &'static str },
    Choice      { options: &'static [&'static str], default: usize },
    TextRole    { default: TextRole },
    SurfaceRole { default: SurfaceRole },
    BorderRole  { default: BorderRole },
    TextStyle   { default: TextStyleRole },
    Color       { default: ColorProp },
    Optional    { inner: Box<KnobKind> },          // adds an enable checkbox
}

pub struct KnobSpec {
    knobs:  Vec<KnobDecl>,                          // ordered for stable layout
    groups: Vec<KnobGroup>,                         // for "add_button: { ... }"
}
```

`KnobValues` at runtime owns one typed `Signal` per declared knob.
Constructed once when the user navigates to a widget; reused as the
user flips between variants of the same widget (variant change
mutates signal *values*, not the signal set).

```rust
pub struct KnobValues { /* opaque */ }

impl KnobValues {
    pub fn bool(&self, id: &str)     -> Signal<bool>;
    pub fn i32(&self, id: &str)      -> Signal<i32>;
    pub fn f32(&self, id: &str)      -> Signal<f32>;
    pub fn text(&self, id: &str)     -> Signal<String>;
    pub fn choice(&self, id: &str)   -> Signal<usize>;
    pub fn role_text(&self, id: &str)-> Signal<TextRole>;
    pub fn color(&self, id: &str)    -> Signal<ColorProp>;
    pub fn opt_bool(&self, id: &str) -> Signal<Option<bool>>;
    // ... one accessor per KnobKind
}
```

Each accessor panics if the id wasn't declared in the spec — this is
developer-facing tooling, panic on misuse is fine. A debug
implementation also asserts the kind matches.

### 5.3 `PreviewVariant`

```rust
pub enum PreviewVariant {
    /// Knob preset — knobs() drives the build, this just supplies
    /// override values for them.
    Knobs {
        name: &'static str,
        overrides: VariantOverrides,
    },
    /// Hand-authored fixture — for composites where flat knobs don't
    /// describe the shape (Wizard, Form, ListView with sample data).
    /// `build()` ignores knobs() entirely for this variant.
    Scenario {
        name: &'static str,
        builder: fn() -> Box<dyn Widget>,
    },
}
```

Tier A widgets (flat knob surface) use `Knobs` variants. Tier B/C
widgets (composites, data-driven) use `Scenario`. A widget can mix
both if useful.

## 6. The canvas pane (the design correction)

`Switcher` cannot host arbitrary children swapped at runtime — its
child set is fixed at `build()` time. The canvas is therefore a
**custom widget**, roughly:

```rust
struct PreviewCanvas {
    selected_widget:   Signal<&'static str>,
    selected_variant:  Signal<&'static str>,
    knobs_for_current: Signal<Rc<KnobValues>>,    // rebuilt on widget change
    background_mode:   Signal<BackgroundMode>,
    zoom:              Signal<f32>,

    // cached child id; rebuilt when widget or variant changes.
    child_id: Option<WidgetId>,
}

impl Widget for PreviewCanvas {
    fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
        // Rebuild on widget+variant change. Knob value mutations
        // do NOT trigger this — they propagate through the child's
        // Prop::Bound bindings at RepaintOnly / Relayout level.
        ctx.bind_to(&self.selected_widget, BindingLevel::Rebuild);
        ctx.bind_to(&self.selected_variant, BindingLevel::Rebuild);

        let entry = registry::find_by_id(self.selected_widget.get())
            .expect("registry holds selected entry");
        let knobs = self.knobs_for_current.get();
        let child = entry.build(&self.selected_variant.get(), &knobs);

        let zoom_wrap   = FixedSize::new()
            .bind_size(self.zoom_signal_to_size())
            .child(child);
        let bg_wrap     = self.wrap_in_background(zoom_wrap);
        self.child_id   = Some(ctx.add_boxed(Box::new(bg_wrap)));
        vec![self.child_id.unwrap()]
    }

    fn size_that_fits(&self, p: SizeProposal, c: &LayoutContext) -> Size { /* delegate */ }
}
```

Knob change → existing widget's `Prop::Bound` repaints/relayouts only
the affected node, no rebuild. Widget-or-variant change → canvas
rebuilds, constructs a fresh widget instance from the catalog entry.
This is the only correct shape given the substrate.

## 7. Launch UX

Three modes; v1 ships A and B. C is a follow-on.

### 7.1 Mode A — Standalone (always works)

```bash
# In the FernUI workspace:
cargo run -p fern-widgets-previewer

# In any consuming-app workspace:
cargo run -p <app>-previewer
```

Opens with the navigator focused, no widget selected. User browses.
Pair with `cargo watch -x 'run -p fern-widgets-previewer'` (or the
equivalent in the consuming app) for restart-on-save.

### 7.2 Mode B — CLI targeting

```bash
# By widget id:
cargo run -p fern-widgets-previewer -- --widget=button --variant=disabled

# By source file (looks up which entry was registered from this file):
cargo run -p fern-widgets-previewer -- --file=crates/fern-widgets/src/button.rs
```

The `--file=PATH` form uses `entry.source()` (populated by
`file!() / line!()` captured at `inventory::submit!` time). The
previewer matches by suffix to handle path canonicalisation.

VS Code integration is a workspace task, no extension required:

```jsonc
// .vscode/tasks.json
{
  "label": "Preview current widget",
  "type": "shell",
  "command": "cargo run -p fern-widgets-previewer -- --file=${file}",
  "presentation": { "panel": "dedicated", "reveal": "silent" }
}
```

Bound to a keystroke (e.g. `Ctrl+Alt+P`). Each consuming-app
workspace ships its own task pointing at its own `<app>-previewer`
binary. The user has one task per workspace, and the same keystroke
"works on the file in front of me" regardless of which project is
open.

### 7.3 Mode C — VS Code extension with CodeLens (deferred)

A CodeLens "▶ Preview" above each `impl WidgetCatalog for X` block,
clicking spawns the previewer with `--widget=X`. Polish; defer until
the previewer is indispensable.

## 8. UI layout

```
┌────────────────────────────────────────────────────────────────────────┐
│ [Theme: Dark ▾] [Locale: en-US ▾] [Background: Themed ▾] [Zoom 100% ▾] │
├──────────────────┬───────────────────────────────────┬─────────────────┤
│ 🔍 filter...     │                                   │ ▸ Variant       │
│                  │                                   │   ◉ disabled    │
│ ▾ Controls       │                                   │   ○ default     │
│   ▾ Button       │                                   │   ○ loading     │
│     • default    │      ┌──────────────────┐         │                 │
│     • disabled ◀ │      │      [ Save ]    │         │ ▸ Knobs         │
│     • loading    │      └──────────────────┘         │   label  [Save] │
│   ▸ Checkbox     │                                   │   disabled  ☑   │
│   ▸ Toggle       │                                   │   role  [Prim▾] │
│ ▸ Containers     │                                   │   icon  [None▾] │
│ ▸ Composites     │                                   │                 │
│                  │                                   │ ▸ Export        │
│                  │  ─── 280 × 36 px · 0.4ms ───      │   [Save PNG]    │
└──────────────────┴───────────────────────────────────┴─────────────────┘
```

### 8.1 Knob → editor widget mapping

| `KnobKind`     | Editor widget                                  |
|----------------|------------------------------------------------|
| `Bool`         | `Toggle`                                       |
| `I32` / `F32`  | `Slider` + numeric label                       |
| `Text`         | `TextField` (placeholder once it lands)        |
| `Choice` (≤5)  | `SegmentedControl`                             |
| `Choice` (>5)  | `ComboBox`                                     |
| `TextRole` …   | `ComboBox` with role chip rendering            |
| `Color`        | role-chip `SegmentedControl` + custom hex row  |
| `Optional<T>`  | `Checkbox` + nested editor, disabled when off  |

Composite editors (`Color`, `Optional`) live in `knob_form.rs`. They
combine existing widgets — no upstream additions to `fern-widgets`.

### 8.2 Knob persistence

A `HashMap<(WidgetId, VariantId), KnobValues>` in app state caches
the user's edits. Behaviour:

- **Variant switch within widget:** load that variant's preset from
  `KnobSpec`, then apply any saved overrides for `(widget, variant)`.
- **Widget switch:** save current `(widget, variant)` snapshot;
  restore destination's snapshot if cached, else build from preset.
- **"Reset" button** in the inspector clears the override map for
  `(widget, variant)` and reloads the preset.

The cache is in-memory; not persisted to disk in v1. (Phase 7 may
add a JSON sidecar if it proves useful.)

## 9. Phasing

Each phase is independently shippable and produces a binary that can
be exercised by hand. Phases 0–4 plus Tier A coverage = v1.

### Phase 0 — Substrate *(~1.5 days)*

1. Create `crates/fern-preview` (lib only) with `WidgetCatalog`,
   `CatalogEntry`, `PreviewVariant`, `KnobSpec`, `KnobKind`,
   `KnobValues`, `SourceLoc`, registry helpers. Unit tests cover
   `KnobSpec` round-trip, `KnobValues` accessor panics, registry
   iteration order.
2. Create `crates/fern-preview-ui` as an empty library crate. Just a
   `pub fn run_previewer(opts: PreviewerOptions) { todo!() }`
   placeholder so the binary compiles.
3. Add `inventory = "0.3"` to workspace `[workspace.dependencies]`.
4. Add `preview` feature to `fern-widgets/Cargo.toml`, gated `mod
   catalog;` placeholder in one widget (Button) so we can prove the
   feature path compiles end-to-end.
5. Create `crates/fern-widgets-previewer` binary with a 5-line
   `main.rs` calling `run_previewer(PreviewerOptions::default())`.

**Acceptance:**
- `cargo test -p fern-preview` green.
- `cargo build -p fern-widgets-previewer` green; the resulting binary
  exits cleanly (`run_previewer` is allowed to print "todo" and quit).
- `cargo build -p fern-widgets` (no features) green and *does not*
  pull in `fern-preview` as a dependency.

### Phase 1 — Vertical slice (Button only) *(~2.5 days)*

1. `WidgetCatalog` impl for `Button` with three knob-style variants
   (default / disabled / loading) and a flat `KnobSpec` covering
   label, disabled, role, optional icon.
2. Implement `PreviewCanvas` (the custom rebuilding widget from §6).
3. Implement minimal `PreviewerApp` root: a `VStack` containing a
   hardcoded `RadioButton` group ("Button" only) for widget pick,
   another for variant pick, the canvas, and a hardcoded knob form
   (Toggle for `disabled`, TextField stub for `label`, etc.).
4. Wire `--widget=button --variant=disabled` CLI args to set the
   initial signals.

**Acceptance:**
- Launch previewer → see Button render.
- Flip "disabled" toggle → instant visual change. Verify in
  `RUST_LOG=debug` that no `build()` runs on Button when only
  `disabled` flips (RepaintOnly path).
- Switch variant → Button rebuilds, knob values reset to that
  variant's preset.
- `cargo run -p fern-widgets-previewer -- --variant=loading` opens
  with "loading" preselected.

### Phase 2 — Navigator (left pane) *(~2 days)*

1. Build a `TreeModel<CatalogNode>` where `CatalogNode` is `Group |
   Widget | Variant`, populated from `inventory::iter`.
2. Render via `TreeView` with a delegate emitting label + selection
   indicator per node kind.
3. Filter `TextField` at the top: case-insensitive substring match on
   widget id, display name, and variant name.
4. Wire selection → `selected_widget` + `selected_variant` signals.
5. Implement `--file=PATH` resolution: walk
   `inventory::iter::<&dyn CatalogEntry>()`, match by `entry.source()
   .file` suffix, set initial selection.

**Acceptance:**
- TreeView lists Button (only widget registered) under "Controls".
- Filter narrows the tree.
- `cargo run -- --file=crates/fern-widgets/src/button.rs` opens with
  Button focused and the navigator scrolled to it.

### Phase 3 — Inspector (right pane) *(~3 days)*

1. Variant section: `RadioButton` group bound to `selected_variant`.
2. `knob_form.rs` walks `KnobSpec` and emits one row per knob using
   the editor table from §8.1. Composite editors for `Color` and
   `Optional` live here.
3. Group sections: each `KnobGroup` becomes an `Accordion`,
   default-expanded.
4. Implement `KnobValues` persistence map (§8.2). Wire the "Reset"
   button in the inspector header.
5. Move the hardcoded knob form from Phase 1 into `knob_form.rs`.

**Acceptance:**
- Every `KnobKind` round-trips through its editor.
- Switching variants resets to that variant's preset.
- Switching widgets preserves the previous widget's overrides; coming
  back restores them.
- "Reset" returns the current `(widget, variant)` to its preset.

### Phase 4 — Toolbar (top) *(~1.5 days)*

1. Theme dropdown (Light / Dark / Custom) — drives a
   `Signal<Theme>` scoped to the canvas sub-tree only. The
   previewer's own chrome stays on its own theme so a buggy theme
   doesn't render the controls invisible.
2. Locale dropdown driving the canvas's locale signal (use
   `fern-i18n::I18nManager`).
3. Zoom presets: 50% / 75% / 100% / 125% / 150% / 200%. Wraps the
   canvas in a `FixedSize` with size = preview-natural-size × zoom.
4. Background mode: Themed Surface / Transparent (checkered) /
   Custom Color. Drives the canvas's background `RectWidget`.

**Acceptance:**
- All four toolbar controls take effect without rebuilding the
  canvas tree (theme/locale go through `Signal`s).
- Switching theme on the canvas does *not* affect the previewer
  chrome.

### Phase 5 — Widget coverage *(rolling, ~3-5 widgets/day)*

Order by complexity, not alphabetically. Tier A ships in v1; Tier B/C
roll in afterwards.

#### Tier A — flat knob surface *(targets v1)*

`Button`, `Checkbox`, `RadioButton`, `Toggle`, `Slider`,
`ProgressBar`, `Badge`, `Link`, `Divider`, `IconWidget`, `ComboBox`,
`SegmentedControl`. Each gets a `mod catalog` with a flat `KnobSpec`
and 2–4 knob-style variants.

#### Tier B — composites with fixture variants *(post-v1)*

`Card`, `Panel`, `Accordion`, `ToolBox`, `Tooltip`, `Snackbar`,
`Breadcrumb`, `StatusBar`, `Toolbar`, `TitleBar`. `knobs()` is empty
for most; `variants()` is hand-authored scenarios.

#### Tier C — data-driven / structural *(post-v1)*

`ListView`, `TreeView`, `Repeater`, `MenuBar`, `MenuList`,
`MenuContext`, `Dialog`, `Popover`, `Wizard`, `SplitView`,
`TabWidget`, `ScrollArea`, `ShortcutSettings`. Scenario-only.
Fixtures share helpers in
`crates/fern-widgets/src/preview_fixtures.rs` (e.g. sample
`ListModel<String>`, sample `TreeModel<&str>`, etc.) so each variant
is short.

**Acceptance per widget:** at least one variant renders; any
properties that are obviously interesting (disabled, role, density)
are exposed as knobs if the widget is Tier A.

### Phase 6 — PNG snapshot export *(~1.5 days)*

1. Add a "Save PNG" button in the inspector's Export accordion.
2. Implementation: instantiate a one-off `Renderer` against an
   off-screen `wgpu::Texture` (using
   [renderer.rs:191](../../crates/fern-render/src/renderer.rs#L191)),
   render the current `RenderFrame`, copy texture to buffer via
   `CommandEncoder::copy_texture_to_buffer`, encode as PNG via the
   `image` crate.
3. Output filename: `<widget_id>__<variant>__<theme>.png` in a
   user-chosen directory.

**Acceptance:**
- "Save PNG" produces a pixel-correct snapshot of the current
  canvas content (theme, zoom, background respected).

### Phase 7 — Deferred items *(out of v1)*

- **Source panel** (`fern!` snippet emit). Needs a builder→source
  pretty-printer mirroring [crates/fern-ui-macros/src/lower.rs](../../crates/fern-ui-macros/src/lower.rs).
  Investigate a builder-API string emitter as a stop-gap.
- **`#[derive(WidgetCatalog)]`** proc macro that synthesises
  `KnobSpec` from inherent `impl Foo { pub fn x(...) }` builder
  methods. Strong recommendation: write 10+ hand impls first to feel
  out the shape before committing to the macro.
- **File-watcher hot-reload** of structural edits. Requires either
  `libloading` reload of a `cdylib` or a runtime `fern!` interpreter.
  Each is a multi-week project on its own.
- **Visual regression infra.** Snapshot baselines, diff renderer,
  CI wiring, PR comment integration.
- **Click-to-introspect** ("Flavor B"). Needs a per-widget
  introspection trait recovering current `KnobValues` from a live
  instance.

## 10. Risks

| Risk | Mitigation |
|---|---|
| `inventory` × Cargo features interaction is novel for this workspace. If feature unification leaks `preview` into release builds, `fern-widgets` ships unused catalog code. | Verify in Phase 0 that `cargo build -p fern-widgets` (no features) emits no `inventory::submit!` symbols. Use `cargo expand` and a binary-size diff. |
| Knob form widget churn for `Color` and `Optional`. These need composite editors that don't exist as single fern-widgets components. | Build them in `knob_form.rs` from existing primitives. Don't upstream — they're tooling-specific. |
| TreeView re-mount cost when switching widgets. Each switch rebuilds the canvas's child subtree from scratch. | Acceptable for ≤50 widgets. Measure in Phase 2. If problematic, cache `Box<dyn Widget>` per `(widget, variant)` keyed off knob-snapshot hash. |
| Tier B/C scenario polish is a rabbit hole. Wizards and Dialogs invite "let me make this preview perfect" effort. | Cap variants at 2–4 per widget. Reuse fixtures across widgets. Ship the rough version; iterate when needed. |
| Theme scope leakage: if the canvas's theme override accidentally mutates the previewer chrome, the user can render the chrome unusable. | The canvas owns a child subtree with its own `Signal<Theme>`; the chrome reads from a separate signal. Verify in Phase 4 with a deliberately broken theme. |
| Source-loc capture from `inventory::submit!` may be brittle if the macro doesn't run in the call site's `file!()` scope. | Confirm in Phase 0 by registering Button and asserting `entry.source().file` ends with `button.rs`. |

## 11. Reading list

For implementers picking this up:

- [docs/reactive-theme.md](../reactive-theme.md) — `Signal`, `Prop`,
  `BindingLevel`, why a knob change repaints without rebuild.
- [docs/fern-macro-reference.md](../fern-macro-reference.md) — the
  `fern!` syntax we eventually want to emit (Phase 7).
- [crates/fern-render/src/renderer.rs](../../crates/fern-render/src/renderer.rs)
  for the off-screen render API used by Phase 6.
- [crates/fern-widgets/src/primitives/switcher.rs](../../crates/fern-widgets/src/primitives/switcher.rs)
  to confirm why the canvas can't be a `Switcher` (§6).
- [crates/fern-widgets/src/tree_view.rs](../../crates/fern-widgets/src/tree_view.rs)
  for the navigator's underlying widget.
- [examples/widget_catalog/src/main.rs](../../examples/widget_catalog/src/main.rs) —
  closest existing pattern (hand-coded Signals struct, no registry).
  Useful as inspiration but not a reusable scaffold.

## 12. Estimated total scope

| Phase | Days |
|---|---|
| 0 — Substrate | 1.5 |
| 1 — Vertical slice (Button) | 2.5 |
| 2 — Navigator | 2 |
| 3 — Inspector | 3 |
| 4 — Toolbar | 1.5 |
| 5a — Tier A coverage (12 widgets) | 4 |
| 6 — PNG export | 1.5 |
| **v1 total** | **~16 days (~3.2 weeks)** |
| 5b — Tier B coverage (10 widgets) | 4 |
| 5c — Tier C coverage (13 widgets) | 6 |
| **v1 + full coverage** | **~26 days (~5 weeks)** |

Phase 7 items are individually scoped follow-ons: Source panel
(~1 week), `#[derive(WidgetCatalog)]` (~3 days once the impl shape is
known), hot-reload (multi-week), VR infra (multi-week).
