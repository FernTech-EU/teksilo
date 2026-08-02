<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

> **Status.** Recommendations **A** (Strip bar slots) and **B** (dockless
> `DockAction`s), and the ARIA fix in §4.6, are **implemented and shipped** —
> see `docs/docking.md` for the resulting user-facing API and the CHANGELOG
> entry for the summary. This document is kept for the *reasoning*: why actions
> are not `DockTab`s (§4.1), why they are not spliced into the tab-indexed
> column (§4.2), why they are view config rather than model state (§4.3), and
> the ARIA argument behind the tablist/toolbar split (§4.6).
>
> **§5 is the live backlog item** — the Top/Bottom reopen-affordance scar and
> the horizontal-rail question. §5.1 records a cheap fix that was proposed,
> accepted, and then found unsound; read it before re-proposing one.
>
> §9's phase list is historical. Phases 2-5 are done; Phase 6 (the
> `background_menu` keyboard trap) and Phase 7 (the horizontal rail) are not.

# DockActivityBar Slots + Dockless Actions — Design (Revision 2)

## 1. What already exists (correcting the mental model)

- **`DockRail` already has `top_slot()`/`bottom_slot()`** — `Rc<dyn Fn() -> Box<dyn Widget>>` factories, rebuilt every rail rebuild, placed above the items / after a trailing `Spacer`. [`docking/activity_bar.rs:127-128,149-150,187-203`](crates/bastyde-widgets/src/docking/activity_bar.rs)
- **`TabWidget` already has `bar_leading_slot()`/`bar_trailing_slot()`** — a *different* mechanism: a memoize-once `BarSlot` taking a built `impl Widget + 'static`, not a factory. [`tab_widget.rs:911-935`](crates/bastyde-widgets/src/tab_widget.rs)
- **`docking/panel.rs` already consumes `bar_trailing_slot` internally**, for the "hidden activities" hamburger shown only when every activity on a Strip side is hidden. [`panel.rs:524-535`](crates/bastyde-widgets/src/docking/panel.rs)
- **There is no app-facing way to add a slot to a Strip-presentation tab bar today.** `DockSidePanel` never sets `.orientation(...)`, so the Strip bar is horizontal for *every* `DockSide` including Top/Bottom — "leading/trailing" for that bar is a reading-order axis, unrelated to the Rail's vertical top/bottom axis. [`panel.rs:369-372,438-535`; `tab_widget.rs:361`](crates/bastyde-widgets/src/docking/panel.rs)
- **`DockActivityBar` is vertical-only, end to end** — item column is a `VStack`, overflow capacity is computed from `bounds.height`, `rail_insertion` is y-only, the drop indicator draws a horizontal line, a11y hardcodes `Orientation::Vertical`, Labeled mode uses a 90°-rotated label, tooltips are hardcoded `TooltipPlacement::Side` with an explicit comment explaining why a `Below` tooltip would drop onto the next stacked item. [`activity_bar.rs:236-612,1146-1179`](crates/bastyde-widgets/src/docking/activity_bar.rs)
- **For Top/Bottom sides, the (always-vertical) rail is a column pinned to the leading cross-edge**, excluded from `band_depth()`, and a hidden Top/Bottom band **collapses completely — rail included** ("a vertical rail can't stand alone in a zero-depth band"); the app must supply its own external reopen button. This is an admitted, tested design scar (`hidden_top_with_rail_fully_collapses`). [`geometry.rs:13-26,194-219,759-776`](crates/bastyde-widgets/src/docking/geometry.rs)
- **`DockRail` is per-view builder config on `DockingLayout`, not on `DockingModel`.** It's declared fresh per `DockingLayout::new(model)` call site, the same way `.dock(DockWidget)` is — and dock/rail metadata registration on the model happens **immediately, synchronously, inside the builder chain**, specifically so the app can call `model.import_state(dto)` afterward with all ids already known. [`docking.rs:79-128,156-164,159-164`](crates/bastyde-widgets/src/docking.rs)
- **`DockingModel::register_meta` — the actual metadata-registration mutator — is `pub(crate)`.** App code never calls a model-level `register_*` method directly; it only reaches metadata through the `DockingLayout` builder. [`model.rs:527-531`](crates/bastyde-widgets/src/docking/model.rs)
- **Bastyde already has a live, unfixed ARIA "required owned elements" violation.** `DockActivityBar::accessibility()` sets `Role::TabList` on the rail's *whole* root; `top_slot`/`bottom_slot` widgets and the overflow-trigger `IconButton` are ordinary children of that same `VStack`, so they are already non-`Role::Tab` descendants of a `role=tablist` today, before any of this design ships. [`activity_bar.rs:394-433,601-611`](crates/bastyde-widgets/src/docking/activity_bar.rs)
- **`Role::GenericContainer` is already this codebase's idiom for a presentational, unnamed grouping wrapper** — used by `DockingLayout`'s own root, `menu_bar.rs`, and `splitter.rs`. `accessibility_impl.rs`'s pruning pass only removes such a wrapper when it carries **no** semantic property at all (no name, no orientation) — there's a standing regression test guarding exactly this (`plain_button_is_a_leaf_no_group_node`). [`docking.rs:474`; `menu_bar.rs:917`; `splitter.rs:555`; `bastyde-core/src/widget_tree/accessibility_impl.rs:683-702`; `button.rs:1404-1426`]
- **Skribisto has zero live call sites for `DockRail::top_slot/bottom_slot`** (grep-confirmed) — its only slot consumer is `TabWidget::bar_trailing_slot`, for the editor pane's own split/close button. [`crates/bastyde_ui/src/app.rs:122`]
- **Skribisto's dockless-action need is real but already solved outside the docking system**: `SpellcheckToggleButton`, `ExportSplitButton`, `ProjectSwitcherButton` are hand-built in the window's `TitleBar` (`shell/windows.rs:612-660`), specifically because they are **window-global**, not tied to any `DockSide` — none of them has a coherent side to attach to.

---

## 2. The real gaps, ranked

1. **No app-facing Strip-side slot.** `TabWidget::bar_leading_slot/bar_trailing_slot` exist and are production-tested (the hamburger) but Strip sides have no way for an *app* to inject one — the DockRail-side equivalent (`top_slot`/`bottom_slot`) has no Strip-presentation counterpart at all.
2. **No dockless-action concept exists.** Every rail item today is 1:1 with a `DockTab` (splitter + panes + content). There is no way to put a plain command button in the rail that looks and behaves like an activity button but opens no panel.
3. **A pre-existing ARIA violation** (`top_slot`/`bottom_slot`/the overflow trigger as non-tab children of `role=tablist`) must be fixed *before* adding a second, larger population of non-tab rail content, or the defect compounds. Worth fixing on its own merit even if nothing else here ships.
4. **Top/Bottom sides have no persistent reopen affordance** — a hidden band takes its rail with it, so every app with a Top/Bottom rail must hand-wire an external toggle. Skribisto already does, twice (§5.2). Only a horizontal rail fixes this; there is no cheap version (§5.1).
5. **A pre-existing overflow-capacity approximation** ("one stride per slot" regardless of actual slot height). Not fixed here — Part B's action term is exact by construction, but reconciling the two slot terms needs each `DockRailSlot` to report a measured extent, which is its own small design.
6. **A pre-existing keyboard trap**: `background_menu` — the sole restore path once a side is fully hidden — is reachable only by pointer right-click; nothing in an empty rail is a Tab stop. Genuinely pre-existing and *not* widened by anything in this design (§4.7). Fix separately.

---

## 3. Recommendation A — Strip-presentation bar slots

### 3.1 API

```rust
// docking/activity_bar.rs — DockRail widened
#[derive(Clone)]
pub struct DockRail {
    pub(crate) side: DockSide,
    pub(crate) size: IconButtonSize,
    pub(crate) background: Option<ColorProp>,
    pub(crate) divider: Option<ColorProp>,
    pub(crate) top_slot: Option<DockRailSlot>,      // UNCHANGED — Rail only
    pub(crate) bottom_slot: Option<DockRailSlot>,   // UNCHANGED — Rail only
    /// Pinned at the start of this side's Strip-presentation tab bar via
    /// `TabWidget::bar_leading_slot`. Ignored while the side is Rail
    /// presentation (use `top_slot` there).
    ///
    /// NOT the same contract as `top_slot`. `top_slot`/`bottom_slot` sit on
    /// `DockActivityBar`, which is built unconditionally whenever
    /// `side_has_rail(side)` is true — it survives the side being fully
    /// collapsed. `leading_slot`/`trailing_slot` sit inside `TabWidget`,
    /// which lives *inside* the side's `SideClipPane` and is
    /// `visible_when(progress > COLLAPSED_EPS)` — it disappears the moment
    /// the side is hidden, same as the tab content it sits beside. If your
    /// slot content must survive a hidden side, use Rail presentation with
    /// `top_slot`/`bottom_slot`, or host it outside the docking system
    /// entirely (the pattern Skribisto's title-bar trio already uses).
    pub(crate) leading_slot: Option<DockRailSlot>,
    pub(crate) trailing_slot: Option<DockRailSlot>,
    pub(crate) overflow_icon: Option<DockIconFactory>,
}

impl DockRail {
    pub fn leading_slot<W: Widget + 'static>(mut self, f: impl Fn() -> W + 'static) -> Self {
        self.leading_slot = Some(Rc::new(move || Box::new(f()) as Box<dyn Widget>));
        self
    }
    pub fn trailing_slot<W: Widget + 'static>(mut self, f: impl Fn() -> W + 'static) -> Self {
        self.trailing_slot = Some(Rc::new(move || Box::new(f()) as Box<dyn Widget>));
        self
    }
}
```

### 3.2 Wiring

`DockingLayout::build()` currently reads `self.rails.get(&side)` only at the `DockActivityBar` construction site; `DockSidePanel::new` gets no rail config at all. Fix:

```rust
// docking.rs — DockingLayout::build()
let config = self.rails.get(&side).cloned().unwrap_or_else(|| DockRail::new(side));
let panel = ctx.add(DockSidePanel::new(side, self.model.clone(), self.registry.clone())
    .rail_config(config.clone()));
let rail = self.model.side_has_rail(side)
    .then(|| ctx.add(DockActivityBar::new(side, self.model.clone(), config, self.side_panel_ids.clone())));
```

### 3.3 Hamburger composition — mandatory, exhaustive

`TabWidget`'s `BarSlot` is last-write-wins single-field; `DockSidePanel` already privately calls `bar_trailing_slot` for its hamburger. Compose explicitly, once per edge:

```rust
// docking/panel.rs — Strip-presentation branch
let mut leading = Vec::new();
if let Some(f) = &self.config.leading_slot { leading.push(ctx.add_boxed((f)())); }
let mut tab_widget = tab_widget;
if !leading.is_empty() {
    tab_widget = tab_widget.bar_leading_slot_id(ctx.add(HStack::new().children(leading)));
}

let mut trailing = Vec::new();
if let Some(f) = &self.config.trailing_slot { trailing.push(ctx.add_boxed((f)())); }
if needs_hamburger { trailing.push(ctx.add(hamburger_widget)); }
if !trailing.is_empty() {
    tab_widget = tab_widget.bar_trailing_slot_id(ctx.add(HStack::new().children(trailing)));
}
```

**Invariant A1:** `DockSidePanel` is the sole caller of `TabWidget::bar_leading_slot`/`bar_trailing_slot` in the crate — verify this by grep as a Phase-1 precondition, not an assumption.

### 3.4 Zero-tabs fix

`DockSidePanel::build()` early-returns a bare drop target before the `TabWidget` (and hence any slot) is ever constructed, when the side has zero open docks:

```rust
let all_tabs = self.model.side_tabs(self.side);
if all_tabs.is_empty() {
    let drop = empty_side_drop_target(ctx, &self.model, self.side);
    self.root = Some(drop);
    return vec![drop];
}
```

A side configured with `.leading_slot(...)`/`.trailing_slot(...)` but currently zero docks — a reachable state, not a misuse — renders no slot at all, silently. Thread `self.config.leading_slot`/`trailing_slot` into this branch too (a minimal `HStack`/`TabWidget` around the drop target), so the slot survives an empty-but-configured side. This does **not** fix the collapse-on-hide case documented in §3.1 — that one stays as a stated, weaker contract, not a bug, because fixing it would mean restructuring `docking.rs` to move `TabWidget` outside `SideClipPane`, a change with no current consumer justifying its cost.

### 3.5 Overflow-capacity note

`top_slot`/`bottom_slot`'s "one stride per slot" approximation is a real, pre-existing bug, independent of this feature. Not fixed here — see Phase 5, where Part B needs an *exact* version of the same math anyway and the two should be reconciled together, not duplicated.

---

## 4. Recommendation B — dockless action entries

> **Revision 2.** Cyril's answers to §10 — *"do not hide `DockAction`"*, *"not hidable"* — collapse
> this part substantially. What follows is the revised design; the deleted machinery is
> itemised in §4.9 so the reasoning isn't lost.

### 4.1 Why not a `DockTab` variant (settled, unchanged)

`import_state`'s pane-survival guard (`if !panes.is_empty()`,
[`model.rs`](crates/bastyde-widgets/src/docking/model.rs)) would treat an action's permanently-empty
`panes` as indistinguishable from a fully-pruned dead tab — modelling actions as zero-pane
`DockTab`s would **silently delete every action on the first app restart**. ~10 call sites also
assume `tab.panes.first()` is meaningful (silent panic/blank-panel risk, not a compile error).
**Verdict: actions are a structurally separate concept.**

### 4.2 Why not spliced into the tab-indexed column (settled, unchanged)

`rail_insertion`'s `vpos → model_indices[vpos]` mapping silently resolves to the *wrong tab* if a
non-tab entry shares the indexed sequence. `DockRailActionGroup` is a separate sibling widget that
never registers into `RailItemBounds`/`RailItemIds`, so `rail_insertion`, `model_indices`,
`side_append_index` and the drop machinery are **untouched**. The corruption class is unreachable
by construction, not merely guarded.

### 4.3 Actions are view config, not model state — the decisive consequence of "never hidable"

With hiding gone, an action has **no user-mutable state whatsoever**. Everything about it —
label, icon, tooltip, enabled, toggled, handler — is app-declared and reconstructed each run,
which is the exact definition `state.rs`'s module doc gives for what must *not* be persisted:

> *"Only **user-controllable** values are persisted … App-config — rail thickness, minimum sizes,
> content factories, header actions — is declared each run and reconstructed (Qt `saveState`
> parity)."*

So actions belong on **`DockRail`**, beside `top_slot`/`bottom_slot`, not on `DockingModel`.
This is strictly better than Revision 1's builder-registration design and deletes its entire
registration-ordering problem: there is no id to match at import time because nothing is imported.

```rust
// docking/activity_bar.rs — DockRail gains an ordered action list
impl DockRail {
    /// Append a dockless command button to this side's rail. Declaration
    /// order is render order within a placement.
    ///
    /// **Rail presentation only.** A side in [`TabPresentation::Strip`]
    /// renders no actions at all — and `set_side_rail` can flip
    /// presentation at runtime, so a side that flips Rail → Strip drops
    /// its whole action cluster. If that is reachable in your app, mirror
    /// the cluster with [`trailing_slot`](Self::trailing_slot), which the
    /// same `DockRail` can carry alongside its actions.
    pub fn action(mut self, action: DockAction) -> Self {
        self.actions.push(action);
        self
    }
}
```

**Nothing changes in `state.rs`. Nothing changes in `DockPolicy`.** The latter matters: Revision 1's
`allow_action_hide` was a breaking field addition to a fully-`pub`, non-`#[non_exhaustive]` struct.
That breaking change is now gone — Parts A and B together are **purely additive**.

### 4.4 Identity

```rust
/// Stable identity for a rail action. NOT used for persistence — actions
/// carry no persisted state (§4.3). It exists for AT naming and for the
/// automation bridge, which addresses widgets by stable id; a fresh-per-run
/// id would make every automation script that clicks a rail action flaky.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DockActionId(u64);

impl DockActionId {
    /// Stable across runs, processes and machines — derived from a
    /// caller-chosen name. Prefer this over `from_raw`: it removes the
    /// hand-picked-`u64`-literal collision hazard entirely.
    ///
    /// ```ignore
    /// const SETTINGS: DockActionId = DockActionId::named("skribisto.settings");
    /// ```
    pub const fn named(name: &str) -> Self { /* const FNV-1a over name bytes */ }
    pub const fn from_raw(v: u64) -> Self { Self(v) }
    pub const fn raw(self) -> u64 { self.0 }
}
```

`named()` must be `const fn` so ids can be `const` items at module scope, matching how Skribisto
already declares its `DockWidgetId`s. FNV-1a (not blake3) because it has to run in a `const`
context; collision risk over a handful of app-chosen names is negligible, and unlike
`open_registry`'s namespacing there is no adversarial input here.

### 4.5 `DockAction`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockActionPlacement {
    /// Before the first activity item, in the flowing cluster.
    Start,
    /// After the last activity item and the overflow trigger, still in the
    /// flowing cluster — the group grows downward with the tabs.
    End,
    /// Past the `Spacer`, anchored to the rail's far edge regardless of how
    /// many activities exist. VS Code's Accounts / Manage-gear cluster.
    /// This is where a Settings gear belongs (§4.8).
    Pinned,
}

/// Mirrors `ToolbarAction`'s proven field shape rather than inventing new
/// vocabulary. Never draggable, never hidable, never persisted — a rail
/// action is app chrome that happens to look like an activity button.
pub struct DockAction {
    pub(crate) id: DockActionId,
    pub(crate) placement: DockActionPlacement,
    pub(crate) label: LocalizedString,
    pub(crate) icon: DockIconFactory,
    pub(crate) tooltip: Option<LocalizedString>,
    pub(crate) enabled: Prop<bool>,
    /// `Some` => renders pressed/checked (mirrors `IconButton::toggled`).
    /// With `hidden` gone there is no longer a second checkmark-shaped
    /// concept to confuse this with — Revision 1's open question is void.
    pub(crate) toggled: Option<Signal<bool>>,
    pub(crate) on_activate: Rc<dyn Fn(&mut EventContext)>,
}

impl DockAction {
    pub fn new(
        id: DockActionId,
        label: impl Into<LocalizedString>,
        icon: impl Fn() -> IconWidget + 'static,
        on_activate: impl Fn(&mut EventContext) + 'static,
    ) -> Self { /* placement: End, enabled: true, toggled: None, tooltip: None */ }

    pub fn placement(mut self, p: DockActionPlacement) -> Self { self.placement = p; self }
    pub fn tooltip(mut self, t: impl Into<LocalizedString>) -> Self { self.tooltip = Some(t.into()); self }
    pub fn enabled(mut self, e: impl Into<Prop<bool>>) -> Self { self.enabled = e.into(); self }
    pub fn toggled(mut self, s: Signal<bool>) -> Self { self.toggled = Some(s); self }
}
```

No `hidable`, no `not_hidable()`, no `set_action_hidden`/`is_action_hidden`/`action_hidden_signal`,
no `action_context_menu`, no `background_menu` section. See §4.9.

### 4.6 A11y structure — the argued decision (unchanged, and now cheaper)

**ARIA citation:** the APG Tabs pattern's Required Owned Elements normatively restrict
`role=tablist` children to `role=tab`; command buttons belong in a `role=toolbar`, which carries
its own independent roving-tabindex model.

**Decision: two sibling composites**, neither nested in the other:

- `DockRailTabList` — wraps **only** the `DockRailItem`s. Must provide *real* layout
  (`VStack::spacing(RAIL_ITEM_SPACING)`), not a bare pass-through, or the column's spacing changes
  the moment the wrapper lands. Carries `Role::TabList` + `rail_label(side)` + `Orientation::Vertical`.
- `DockRailActionGroup` — one per `(side, placement)` with ≥1 action; omitted from the `VStack`
  entirely when empty (never an empty `Role::Toolbar`). Carries `Role::Toolbar` +
  `rail_actions_label(side, placement)` + `Orientation::Vertical`, and a local
  `roving: Signal<usize>` mirroring `Toolbar`'s pattern — **not** `DockRailItem`'s model-level
  `selected: Signal<usize>`, since an action group has no "currently selected" concept.
- `DockActivityBar`'s root drops `Role::TabList` for `Role::GenericContainer` — the crate's idiom
  ([`docking.rs:474`](crates/bastyde-widgets/src/docking.rs), `menu_bar.rs:917`, `splitter.rs:555`).
  **Hard requirement:** the wrapper must carry **no** semantic property at all — no `set_name`, no
  `set_orientation` — or `accessibility_impl.rs`'s pruning pass will not prune it and a screen
  reader announces *"Leading activity bar, group"* then *"Leading activity bar, tab list"*. The
  crate's own `plain_button_is_a_leaf_no_group_node` test exists to prevent exactly this.
- The overflow-trigger `IconButton` (a third stray non-tab child today) moves out as its own sibling.

**A rejected shortcut, settled — do not re-litigate.** `Widget::accessibility_children() -> Option<Vec<WidgetId>>`
**does exist** ([`bastyde-core/src/widget.rs:439`](crates/bastyde-core/src/widget.rs), honoured at
`widget_tree/accessibility_impl.rs:413`) and its doc says it can "reorder (**or restrict**)" AT
children — so it looks like a one-line fix: keep `Role::TabList` on the root, return only the item
ids. **It is the wrong fix.** Restricting is not re-parenting: the slots and the overflow trigger
would not move to a valid parent, they would be *dropped from the AT tree entirely*, turning a spec
violation into a WCAG 2.1.1 failure (operable controls with no accessible representation). The
wrapper is correct because it gives non-tab content a valid parent instead of deleting it.

**Composition order inside the rail's padded column:**

```
[top_slot?] → [ActionGroup(Start)?] → [DockRailTabList[…DockRailItem]]
  → [overflow trigger?] → [ActionGroup(End)?] → [Spacer]
  → [ActionGroup(Pinned)?] → [bottom_slot?]
```

**Keyboard model:** `DockRailTabList` and each `DockRailActionGroup` are each a single Tab stop with
their own internal roving Arrow/Home/End cycle. Tab/Shift+Tab crosses between them; arrows never do.
This needs no special implementation — it is the natural consequence of keeping them separate widgets.

**Rendering details that are easy to get wrong:**

- Tooltip: use `ctx.attach_tooltip_with_placement(root, tip, delay, TooltipPlacement::Side)` exactly
  as `DockRailItem` does — **never** `IconButton::tooltip`'s default path, which opens `Below` and
  would drop the tooltip onto the next stacked item.
- Labeled mode: read `side_rail_size(side).shows_label()` and mirror `DockRailItem`'s
  RotatedLabel-vs-tooltip branch. An action showing a hover-only tooltip while the tabs beside it
  show permanent captions is a hover-only-discovery regression.
- Glyph sizing: size the icon to the rail's `item_glyph_size(effective_item_size())`, so an action
  tracks Compact/Default/Labeled like a real item. (An `IconButton` in `bottom_slot` does **not**
  get this for free — the app must bind `rail_size_mode_signal` by hand. That asymmetry is the
  main reason §4.8 recommends `DockAction` over a slot.)

**Overflow capacity** — this *does* require a change to `DockActivityBar::place_children`:

```
reserve = RAIL_PADDING*2
        + stride * (top_slot.is_some() as f32)      // existing, approximate
        + stride * (bottom_slot.is_some() as f32)   // existing, approximate
        + actions.len() as f32 * stride;            // NEW, exact — count is fixed now
```

Because actions can no longer be hidden, the count is a **build-time constant**, so this term is
exact and needs no reactive re-evaluation. Actions are always reserved, never overflow-parked —
matching VS Code's fixed bottom cluster and avoiding its documented issue #46017 (trailing action
icons silently vanishing under space pressure). Keep counts to 1–3 per placement by convention.

### 4.7 The keyboard trap — demoted back to pre-existing

Revision 1 pulled the `background_menu` keyboard trap into scope on the argument that Part B
*widens* it (actions could independently reach zero-visible). With actions never hidable that
argument dies: actions can never disappear, so the set of states reaching an empty rail is
unchanged. The trap is real and still worth fixing — `background_menu` is wired as a `context_menu`
handler on the rail root with **no** accompanying `.focusable(true)`, so a keyboard-only user
cannot reach it via Shift+F10 once every activity is hidden — but it is a **pre-existing defect on
its own merit**, not part of this feature. File it separately; do not let it gate Part B.

### 4.8 Verdict on Skribisto's two concrete cases

**Settings at the bottom of the leading rail → `DockAction` with `Pinned`, not a `bottom_slot` IconButton.**
The two are not equivalent, and four differences all point the same way:

| | `bottom_slot(IconButton)` | `DockAction` + `Pinned` |
|---|---|---|
| a11y | non-tab child of `Role::TabList` — the live violation from §1 | inside the sibling `Role::Toolbar` — correct |
| Roving focus | none; a lone sequential Tab stop | joins the toolbar's Arrow/Home/End cycle |
| Glyph sizing | fixed dp unless the app hand-binds `rail_size_mode_signal` | tracks Compact/Default/Labeled automatically |
| Labeled mode | no caption — looks broken next to captioned tabs | gets the rotated caption like a real item |

Your own framing settles it too: idea #2 asked for something *"similar to the activity buttons"*.
A slot is deliberately opaque chrome; an action is framework-rendered to match. Reserve
`bottom_slot` for what it is genuinely for — non-command chrome (a logo, an avatar, a sync-status
dot, a progress ring) that should *not* look or behave like a button.

```rust
// skribisto — app/project_shell.rs
const SETTINGS_ACTION: DockActionId = DockActionId::named("skribisto.settings");
const ANALYSIS_ACTION: DockActionId = DockActionId::named("skribisto.analysis");

DockRail::new(DockSide::Leading)
    .background(SurfaceRole::Main)
    .divider()
    .action(
        DockAction::new(
            ANALYSIS_ACTION,
            tr!(rail_analysis()),
            || IconWidget::chart_bar(),
            |ctx| ctx.send_intent(Intent::new("analysis.open")),
        )
        .placement(DockActionPlacement::End),
    )
    .action(
        DockAction::new(
            SETTINGS_ACTION,
            tr!(rail_settings()),
            || IconWidget::settings(),
            |ctx| ctx.send_intent(Intent::new("app.settings")),
        )
        .placement(DockActionPlacement::Pinned),
    )
```

Two notes on that call site, both load-bearing:

- **Fire the existing command, don't duplicate it.** `app.settings` is already registered via
  `register_action_global` with Ctrl+, bound to it; `ctx.send_intent(Intent::new("app.settings"))`
  is the idiom Skribisto already uses (`app/commands/file.rs:241`, `app/commands/go.rs:122`). A
  rail action that re-implements the modal would drift from the menu item and the shortcut.
- **"Analysis" is the load-bearing validation of this whole feature.** It opens a *centre editor
  tab*, not a dock — so it can never be an activity, and there is no `DockWidget` to hang it on.
  It is precisely the case `DockRail::action` exists for. Wire the rail half as a new unit
  `AppIntent` variant (handler-driven, payload-free).

  **The rail half is the easy half, and this design only solves that half — say so plainly.**
  An earlier draft said to add the placeholder tab "matching how `tabs/` already dispatches on
  `(role, sub_role)`". That is wrong and would send an implementer down a dead end:
  `tabs.rs::tab_pane()` matches on a **`BinderItem`'s** `(role, sub_role)` drawn from
  `skribisto_model::COMBINATIONS`, and the per-tab payload is a `ContentTab` over a shared
  `Rc<OpenDoc>` — machinery built end to end around a real, `uid`-bearing `BinderItem`. Analysis
  is not a `BinderItem` and has no `(role, sub_role)` to key on, so there is no arm to add.

  Hosting a **non-document tab** in `EditorsViewModel` is therefore a genuine, unsolved
  architectural gap, not a wiring detail — the open questions are: what identifies such a tab in
  the tab list (today every tab is keyed by `BinderItem.uid`, which is also what
  `workspace.toml` v2 persists); whether it survives a session restore or is deliberately
  transient; and whether `EditorsViewModel`'s save/dirty path must learn that some tabs own no
  document. Scope that separately before starting — the `feat/analysis-tab` branch is where it
  belongs, and `DockRail::action` will be waiting for it.

### 4.9 What Revision 1 had that is now deleted

Recorded so the reasoning isn't re-derived later:

| Deleted | Why |
|---|---|
| `DockAction.hidable`, `not_hidable()` | actions are never hidable |
| `DockPolicy.allow_action_hide` | nothing left to gate — **and this un-breaks the API** |
| `set_action_hidden` / `is_action_hidden` / `action_hidden_signal` | no user-mutable state |
| `action_context_menu` | its only item was "Hide" |
| `background_menu`'s action checklist section | nothing to restore |
| `DockActionState`, `DockSideState.actions` | nothing to persist — `state.rs` untouched |
| `DockingLayout::action()` + `register_action_meta` | actions are `DockRail` config (§4.3) |
| the registration-vs-`import_state` ordering contract | no import step exists |
| focus re-homing on hide | actions never vanish |
| the `toggled`-vs-`hidden` visual ambiguity | only `toggled` remains |

---

## 5. Recommendation C — the orientation question

### 5.1 Correction: the cheap fix I proposed does not exist

Revision 1 floated a narrower alternative (OQ5): keep the existing *vertical* rail alive at zero
band depth for Top/Bottom by relaxing `rail_only()`'s `region.height > 0.0` guard. **Cyril accepted
this ("ok, fix"). It is unsound and must not be built.** Closer reading of
[`geometry.rs:434-467`](crates/bastyde-widgets/src/docking/geometry.rs) shows why:

```rust
let rail = Rect::new(rail_x, region.y, rail_w, region.height);
```

For a Top/Bottom band the rail's **height *is* the band's depth**, and
`band_depth() = content_extent() + gutter_extent()` — zero when hidden. The guard is not the cause;
it is a symptom. Keeping a *vertical* rail visible would require permanently reserving
`N × item_extent` of band depth (≈ 3 items → 130 dp of always-present bottom band) purely to host a
column of icons. `SideLayout` models the rail as a single `rail_thickness` scalar, which for
Leading/Trailing is a width with a free height, and for a Top/Bottom vertical rail is a width with a
*constrained* height — a genuine model mismatch, not an oversight. `geometry.rs:20-23`'s comment
("a vertical rail can't stand alone in a zero-depth band") is simply correct.

**There is no cheap fix. The reopen affordance for Top/Bottom requires a horizontal rail.**

### 5.2 The evidence, corrected

Revision 1 also claimed the scar "is not currently hit by any app in this codebase." Also false:

- [`project_shell.rs:205-206`](crates/bastyde_ui/src/app/project_shell.rs) puts `DockSide::Bottom` in
  Rail presentation at 36 dp, `Compact`.
- [`project_shell.rs:311`](crates/bastyde_ui/src/app/project_shell.rs) then calls
  `set_side_visible_immediate(DockSide::Bottom, false)` — **the band ships hidden by default**, so
  the rail is invisible in Skribisto's own default state.
- Skribisto pays for it with two hand-wired workarounds: the
  [`view.rs:63`](crates/bastyde_ui/src/app/commands/view.rs) toggle command and the
  [`project_menus.rs:387`](crates/bastyde_ui/src/shell/project_menus.rs) menu item bound to
  `side_visible_signal(DockSide::Bottom)`. That menu item *is* the "external button"
  `geometry.rs` tells apps to supply.

(`DockSide::Top` remains unreferenced in `bastyde_ui` — that part of the original claim stands.)

### 5.3 Verdict

**Defer, but schedule it.** A horizontal `DockActivityBar` for Top/Bottom is now a justified backlog
item with a named beneficiary, not speculative framework investment. It is still not something to
bundle into Parts A/B: it rewrites `geometry.rs`'s Top/Bottom `split_side` arm, **deletes** rather
than extends four regression tests (`top_rail_is_a_leading_column_not_a_band`,
`top_rail_column_mirrors_to_the_right_in_rtl`, `hidden_top_with_rail_fully_collapses`,
`bottom_rail_column_keeps_handle_inboard`), and changes runtime behaviour for any existing
Top/Bottom-rail consumer. Its payoff is real though: Skribisto would delete a command and a menu
item and get the reopen affordance for free.

Scope notes for when it is taken up, so the next pass starts warm:

- `resize_handle.rs` is **already** fully orientation-generic (branches on `is_horizontal_axis`) —
  no work there.
- Arrow-key nav in `activity_bar.rs` already accepts both axes (Up/Left = Prev, Down/Right = Next) —
  dead flexibility today that becomes correct for free, though it needs an RTL pass for a
  horizontal rail (reading order reverses).
- `RotatedLabel` should be **dropped**, not rotated the other way: it exists because a vertical rail
  has a fixed narrow cross-axis and text runs against the flow. On a horizontal rail the flow axis
  already matches text, so Labeled mode is just icon-above-caption.
- `TooltipPlacement::Side` must become `Below`/`Above` — `bastyde-core`'s existing two-variant enum
  already covers it with edge-flip, so no new variant is needed.
- The one genuinely open design question is whether a horizontal Top/Bottom rail makes
  `TabPresentation::Strip` redundant for those sides, or whether they stay complementary.

---

## 6. Interaction & policy matrix

| Entry kind | Movable | Hidable | Own Tab stop | Overflowable | Drop target | Persisted |
|---|---|---|---|---|---|---|
| `DockTab` (activity item) | yes, if `allow_activity_drag` | yes, if `allow_activity_hide` | yes — roving, in `DockRailTabList` | yes (`DockOverflowMenu`) | yes | yes (`DockTabState`) |
| `top_slot`/`bottom_slot` (Rail) | no — no drag code path exists | no — no hide code path exists | whatever the widget is; sequential, not roving | no — reserved, approximate charge | no | no — app-declared each run |
| `leading_slot`/`trailing_slot` (Strip) | no | no | sequential, not roving | no — reserved | no | no — app-declared each run |
| `DockAction` (Start/End/Pinned) | no — no drag code path exists | **no — by decision** | yes — roving, in its own `Role::Toolbar` | no — reserved, **exact** charge | no | **no — nothing to persist** |

---

## 7. Persistence & migration

**Nothing changes.** This is the headline consequence of §4.3: `state.rs`, `DockLayoutState`,
`DockSideState`, `DockTabState` and the `Versioned`/`Migrator` wiring are all untouched, and
`DockLayoutState::CURRENT_VERSION` stays at 1. Parts A and B together add no persisted field, so
there is no migration to write and no old-layout compatibility question to answer.

One consumer-awareness note worth recording anyway, because it will matter for a *future*
non-additive change: Skribisto embeds `DockLayoutState` as a plain field inside its own separately
`Versioned` `WorkspaceLayoutFile`, via a `lenient_docks` deserializer that bypasses
`DockLayoutState`'s own `Migrator` entirely — only the outer file's version is walked
(`models/workspace_layout_file.rs:129-146`). A future non-additive `DockLayoutState` change
would therefore fail-load and silently blackhole every Skribisto user's whole per-project dock
layout, not just the new field. Nothing to do today; do not let a later contributor assume
"the migrator will handle it".

---

## 8. Prior-art notes

1. **Qt `QTabWidget::setCornerWidget`** — the toolkit precedent for fixed, caller-owned,
   non-draggable end-slots on a horizontal tab strip. Matches Part A directly, and is the source of
   a pitfall worth inheriting awareness of: Qt's corner widget only renders while ≥1 tab exists —
   the exact bug §3.4 fixes.
2. **VS Code's Activity Bar** accepts only View-Container contributions; extensions are explicitly
   forbidden from using an item to open a bare panel-less webview. Validates keeping Part B
   structurally separate from real activity tabs rather than splicing it in as fake tabs.
3. **VS Code's fixed bottom cluster** (Accounts / Manage-gear) is never draggable and never
   individually hideable, and migrates to the title bar on reorientation. Cyril's "do not hide
   `DockAction`" decision lands bastyde **exactly** on this precedent rather than beside it —
   Revision 1's optional-hidability stance was the one part of the design with no precedent in any
   surveyed system, and it is now gone.
4. **IntelliJ's tool-window stripes** — every stripe icon in the docs is a tool-window toggle; the
   only plain-action stripe button ("More tool windows") is IDE-owned chrome. Reinforces: never let
   `DockAction` acquire draggable/reorderable behaviour.
5. **W3C ARIA APG's tablist/toolbar split** — two independent single-Tab-stop composites; arrows
   navigate within, Tab/Shift+Tab crosses between. The load-bearing citation for §4.6.
6. **VS Code panel-actions overflow (issue #46017)** — trailing action icons silently vanishing
   under space pressure with no overflow menu, in a mature funded product. Why §4.6 reserves space
   for actions rather than letting them overflow.

---

## 9. Phased implementation order

**Phase 1 — Preconditions.** No code change. Grep-confirm (a) `DockSidePanel` is the sole caller of
`TabWidget::bar_leading_slot`/`bar_trailing_slot` (Invariant A1); (b) `Role::GenericContainer` usage
at `docking.rs:474`, `menu_bar.rs:917`, `splitter.rs:555`. Record both in the PR description.

**Phase 2 — Part A: Strip slot parity.** `DockRail::leading_slot`/`trailing_slot`; widen
`DockingLayout::build()` to feed rail config into `DockSidePanel`; hamburger composition (§3.3);
zero-tabs fix (§3.4). No model, policy or persistence change.
*Tests:* `strip_leading_slot_renders_with_zero_tabs`; `strip_trailing_slot_composes_with_hamburger`
(both present, neither dropped); `strip_slot_hides_with_side_collapse` (asserts the documented
weaker contract rather than fighting it); regression pass on Skribisto's Strip-side tests.

**Phase 3 — A11y fix.** Independent of Part B; fixes a live defect regardless of whether Part B ever
ships. `DockRailTabList` with real `VStack::spacing(RAIL_ITEM_SPACING)` layout; `DockActivityBar`'s
root drops to a property-free `Role::GenericContainer`; overflow trigger moves out as a sibling.
*Tests:* a11y-tree assertion that `top_slot`/`bottom_slot`/overflow-trigger are no longer descendants
of a `Role::TabList`; update `rail_strip_width_follows_the_size_mode` to query the rail's outer
bounds by `WidgetId` rather than by `Role::TabList`; update `dock_drag_lands_on` to verify its drop
point still lands in the rail's actual outer rect, not the now-narrower `TabList` sub-rect.

**Phase 4 — Part B, whole.** Collapsed from Revision 1's four phases, because §4.3 removed the model,
policy, menu and persistence work. `DockActionId` (+ `const fn named`), `DockAction`,
`DockActionPlacement`, `DockRail::action()`, `DockRailActionGroup`, the `place_children`
overflow-reserve term, and the composition order in `DockActivityBar::build`.
*Tests:* all three placements in one assertion — `Start` before the tab list, `End` after the tab
list **and after the overflow trigger**, `Pinned` after the `Spacer` (the `End`-vs-overflow ordering
is the one most likely to be implemented backwards, since both sit between the tab list and the
`Spacer`); `actions_are_absent_in_strip_presentation` (pins the R2-2 no-op deliberately, §10.1);
roving Tab stop stays within the action group and never enters the tab cycle; tooltip placement
asserted `Side`, not the `IconButton` default; Labeled mode shows an inline caption, not a hover
tooltip; action glyph size follows a Compact↔Labeled flip; overflow reserve shrinks the shown-item
count with 3 actions present; `Role::Toolbar` is a **sibling** of `Role::TabList`, never a
descendant.

**Phase 5 — Skribisto adoption.** `SETTINGS_ACTION` (`Pinned`) firing `Intent::new("app.settings")`;
`ANALYSIS_ACTION` (`End`) firing a new unit `AppIntent` with a placeholder tab. Both locales for
`rail-settings` / `rail-analysis`. Deliberately does **not** migrate the title-bar trio
(Spellcheck/Export/ProjectSwitcher) — those are window-global, have no `DockSide`, and stay where
they are.

**Phase 6 — Separate, pre-existing.** The `background_menu` keyboard trap (§4.7). Its own small PR;
not gated by anything above.

**Phase 7 — Separate proposal.** Horizontal `DockActivityBar` for Top/Bottom (§5). Its own design
doc, its own `geometry.rs` test rewrite, its own behaviour-change sign-off.

---

## 10. Decisions

Every open question is closed. No blocking questions remain; the design is ready to implement as
written.

| # | Question | Decision |
|---|---|---|
| R1-1 | Should `DockAction` be hidable? | **No.** Never hidable — collapses §4 (see §4.9) and un-breaks the API |
| R1-2 | `toggled` vs `hidden` visual disambiguation | **Void** — `hidden` no longer exists |
| R1-3 | Real near-term consumer? | **Yes** — "Analysis" (opens a centre editor tab) and Settings (§4.8) |
| R1-4 | The narrow Top/Bottom reopen fix | **Retracted as unsound** by §5.1 after the accepting decision — do not build |
| R1-5 | `DockActionId::named()` stable-hash ctor | **Yes**, `const fn`, FNV-1a — justified by automation addressing, not persistence (§4.4) |
| R2-1 | Two placements or three? | **Three** — `Start`, `End`, `Pinned` ship together |
| R2-2 | Render actions in `TabPresentation::Strip`? | **No** — Rail-only |

### 10.1 Consequences of R2-2 that must be documented, not left implicit

Actions are Rail-only, but `DockingModel::set_side_rail` can flip a side's presentation **at
runtime**. A side that flips Rail → Strip therefore drops its whole action cluster silently. That is
accepted behaviour, not a defect — but it must be stated at three points or it will be rediscovered
as a bug report:

- `DockRail::action()`'s doc comment: *"Rail presentation only. A side in `TabPresentation::Strip`
  renders no actions — mirror them with `trailing_slot` if the side may flip presentation at
  runtime."*
- `docs/docking.md`'s Activity rail section, beside the existing `top_slot`/`bottom_slot` prose.
- A test — `actions_are_absent_in_strip_presentation` — so the no-op is pinned deliberately rather
  than becoming true by accident and then silently reversing.

The mitigation is already available to apps and costs nothing: a `DockRail` carries both `action()`s
and `leading_slot`/`trailing_slot` for the same side, so an app that genuinely flips presentation
declares the cluster twice — once as actions (Rail), once as a slot widget (Strip). No current app
does this; the escape hatch exists so the Rail-only decision is not a dead end.

### 10.2 Note on shipping all three placements (R2-1)

`Start` has no Skribisto consumer today — `top_slot` already covers the above-the-tabs position for
non-button chrome. Shipping it anyway is the right call for a framework: the three variants are one
coherent, symmetric vocabulary (*before the activities / after the activities / past the spacer*),
and a two-variant enum would make `Start` a later breaking-ish addition that reads as an
afterthought. Cost is a single extra match arm in the composition order — there is no per-variant
machinery, since an empty `DockRailActionGroup` is omitted from the `VStack` entirely.

*Test implication:* the Phase 4 placement test must cover all three (`Start` before the tab list,
`End` after the tab list and the overflow trigger, `Pinned` after the `Spacer`) — the ordering
between `End` and the overflow trigger is the one an implementation is most likely to get backwards,
because both sit between the tab list and the `Spacer`.
