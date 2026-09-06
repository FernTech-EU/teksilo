<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Hover-affordance census

Every affordance in the workspace reachable **only** by hovering — it appears,
reveals, opens or dismisses on `on_hover` / `hover_within` / `PointerEnter` /
`PointerLeave`, and has no press, long-press, keyboard or assistive-technology route.

This matters because the framework rule the touch design adopts is blunt: **`on_hover`
and every hover signal are hover-owner-only, and a contact is never a hover owner.**
A finger produces no hover, ever. So every row below is functionality that simply
does not exist under touch unless package **P29** gives it another route.

Measured against `wt-touch` @ `89c7a03c`. Every line number was read.

A hover *tint* is not a census row. Roughly forty widgets swap a `SurfaceRole` on
hover; that is decoration, and losing it on touch costs nothing. A row qualifies only
where hovering is the sole way to reach **functionality**, or the sole way to **see** a
control at all.

## Census

| # | File | Line | Affordance | Hover trigger (dwell) | Non-hover route today | Proposed touch route |
| ---: | --- | ---: | --- | --- | --- | --- |
| 1 | `crates/teksilo-widgets/src/tooltip/attach.rs` | 143 — armed at `crates/teksilo-core/src/widget_tree/pointer_router.rs:786`, shown from `widget_tree/overlay_impl.rs:1450` | **Plain tooltip** (tier 1) | `tooltip_pointer_enter` on a hover-target change; `motion.tooltip_delay` = **500 ms** (`crates/teksilo-tokens/src/motion.rs:183`), warm reshow 100 ms (`:185`) | **AT only.** Focus is explicitly excluded: `overlay_impl.rs:1176` skips any entry whose `sticky_after` is `None`. The text is mirrored onto the anchor as its accessible description. No press, no chord. | **long press** (auto-dismiss at 5 s) |
| 2 | `crates/teksilo-widgets/src/tooltip/attach.rs` | 276 (`sticky_after = tooltip.sticky_enabled().then_some(DWELL_PROMOTION)`) | **Composite tooltip built with `.sticky(false)`** | `motion.tooltip_delay_heavy` = **700 ms** (`motion.rs:184`) | **none.** With `sticky_after == None` the focus arm at `overlay_impl.rs:1176` skips it, and pointer-leave is the only retire path. | **long press** |
| 3 | `crates/teksilo-core/src/widget_tree/pointer_router.rs` | 786 | **A tooltip on a DISABLED control.** `tooltip_pointer_enter` is *not* behind the `arena.is_enabled` gate that `dispatch_to_widget` applies (`pointer_router.rs:910`), so a disabled anchor still shows its tooltip — by hover and by nothing else. | the same 500 / 700 ms dwell | **none.** Disabled subtrees are dropped from focus traversal (`widget_tree/focus_impl.rs:501-509`), so even a *rich* tooltip's focus route is unreachable on a disabled anchor. | **long press.** This is the single most load-bearing hover-only path in the framework — it is the "why is this greyed out?" answer, and on touch there is currently no way to ask. |
| 4 | `crates/teksilo-scene/src/view/gestures_impl.rs` | 216 (`show_overlay_after`), triggered at 195; delay resolved at `crates/teksilo-scene/src/view/build_impl.rs:425` | **Lightweight `SceneItem` tooltip** | hover transition on pointer-move; `tooltip_delay_heavy` = 700 ms | **none.** Lightweight items have no `WidgetId`, so no focus and no `.tooltip()` attach path, and the synthetic AT node carries no tooltip text. Worse, a `PointerDown` *dismisses* it (`gestures_impl.rs:205`) — a tap can never surface it. | **long press** on the item |
| 5 | `crates/teksilo-widgets/src/tab_widget/header.rs` | 547-548 (`visible_when` on the close button) | **Tab close `×` button** — hidden entirely while the header is Idle. `visible_when` culls it from paint *and* from the AT tree. | `on_hover` at `header.rs:709`, **no dwell** (instant) | **partial.** `Delete` on the focused tab header (`header.rs:781-788`) and middle-click on `PointerUp` (`header.rs:915-917`). The button itself is `.focusable(false)` (`header.rs:539`), and the tab's AT custom actions are reorder-only (`header.rs:1090-1104`) — there is **no** AT "Close". | **always-visible at Touch density** (`RevealPolicy::Always`), plus an AT "Close" custom action |
| 6 | `crates/teksilo-widgets/src/styles/recipe_scroll_bar_style.rs` | 216 (`set_opacity(full_id, revealed)`; effects at 174 and 183); hover source `crates/teksilo-widgets/src/scroll_bar.rs:466` | **The overlay scroll bar's full interactive track** — at rest only the 4 dp passive indicator is painted (`recipe_scroll_bar_style.rs:37-38`); the 8 dp thumb and track fade in on hover | `on_hover` or `is_dragging`; `duration_fast` 120 ms, **no dwell** | **press works blind.** The hit slot is the full `scroll_bar_thickness` (12 dp default, `scroll_area.rs:204`), so a drag or track-click lands even while invisible. Keyboard is dead: `ScrollBar` is `.focusable(false)` (`scroll_bar.rs:372`) despite its `on_key` at `:477`. `ScrollBarMode::Overlay` is the **default** (`scroll_area.rs:55`). | **always-visible at Touch density** — render the full thickness rather than the thin indicator |
| 7 | `crates/teksilo-widgets/src/toast/surface.rs` | 303 (`on_hover` bumps `hover_count`); consumed at `crates/teksilo-widgets/src/toast/host.rs:215` and `:354` | **Toast auto-dismiss pause** — the only way to hold a toast open long enough to read or act on it | `on_hover`, **no dwell**; ref-counted and group-wide by default (`host.rs:112`) | **none.** Focus does not pause, despite the comment at `toast/registry.rs:668` — `hover_count` has exactly one writer. Escape dismisses (`surface.rs:316-321`); the persistent `NotificationLog` is the durable read-later route. | **exempt — no touch analogue for "a pointer resting".** Give Touch density a longer toast-duration token and lean on `NotificationLog`. If a route is wanted, press-and-hold can bump the same refcount. |
| 8 | `crates/teksilo-charts/src/bar_chart.rs` | 316 (`PointerMove` → `hover.set`), painted 627-641 | **Bar datum tooltip card + hover mark** (cross-crate) | `PointerMove` + `rect_hit`, **no dwell** | **AT only** — a per-datum synthetic node with `set_numeric_value` (`crates/teksilo-charts/src/hit.rs:243-246`). `on_tap` (`bar_chart.rs:355`) drives *selection*, not the readout. | **tap** — reuse the existing `on_tap` to also set `hover` |
| 9 | `crates/teksilo-charts/src/line_chart.rs` | 290 (`PointerMove` → `hover.set`), painted 631-652 | **Line nearest-point tooltip + marker** (cross-crate) | `PointerMove` + `nearest_point`, **no dwell** | AT only (`hit.rs:243-246`); `on_tap` at :334 is selection | **tap** |
| 10 | `crates/teksilo-charts/src/pie_chart.rs` | 338 (`PointerMove` → `hover.set`), painted 603-633 | **Slice tooltip card** (cross-crate) | `PointerMove` + `slice_hit`, **no dwell** | AT only (`hit.rs:243-246`); `on_tap` at :390 is selection | **tap** |
| 11 | `crates/teksilo-widgets/src/standard_item.rs` | 338 (`StandardListItem::interaction_signal`), 886 (`StandardTreeItem` forward); written by `on_hover` at 722 | **The API contract for hover-revealed row actions** — the framework's blessed handle for "a row that shows its actions only while the pointer is over it" (documented at :327-337) | row `on_hover`, **no dwell** | **none.** It is a raw `Signal<InteractionState>` with no focus or AT contribution, so every app built on it is hover-only by construction. | **always-visible at Touch density** — the signal must read as permanently revealed at Touch, or the documented contract has to say it is mouse-only |

Eleven rows: **nine** need a touch route, **one** is exempt (row 7), and **one** is an
API contract rather than a widget (row 11) but has to be settled the same way.

## Corrections — named in the audit, verified NOT hover-only

Six of the affordances the pre-flight audit flagged turn out to have a non-hover route
already. They are recorded here so P29 does not spend effort on them, and so the
audit's numbers are not carried forward uncorrected.

| Audit claim | Verdict |
| --- | --- |
| MenuItem's ~400 ms submenu hover-open is hover-only | **Constant right, conclusion wrong.** `DEFAULT_SUBMENU_OPEN_DELAY = 400 ms` (`menu_item.rs:114`) applies only on the hover path (`:1264-1266`). The same submenu opens **immediately** on tap (`menu_item.rs:1223-1246`), on ArrowRight / Enter / Space (`:1382-1465`), and through `on_access_action` (`:1479`). Not a census row. |
| The `PointerLeave` submenu dismissal strands an open submenu on touch | `DEFAULT_SUBMENU_CLOSE_DELAY = 150 ms` (`menu_item.rs:115`), but a `DismissBehavior::PointerLeave` overlay is *also* dismissed by Escape (`crates/teksilo-core/src/overlay.rs:922-925`) and by press-outside (`overlay.rs:1175`). Touch produces both. Not a census row. |
| The safe triangle is a hover-only mechanism | `crates/teksilo-core/src/overlay/safe_triangle.rs` + `SAFE_REGION_BUDGET = 600 ms` (`overlay.rs:36`) is pure hover-trajectory *suppression*: it reveals nothing and gates no functionality. It goes inert on touch, harmlessly. Exempt. |
| **The Splitter handle is invisible until hovered, so touch has no grab affordance** | **Wrong, and this was a blocker-severity claim.** `crates/teksilo-widgets/src/styles/recipe_splitter_style.rs:146-147` paints a resting divider line *unconditionally* — the comment is literally "Resting line — always present". The 400 ms dwell (`splitter/handle.rs:45`, alpha ramp `HOVER_DWELL_DELAY_FRAC = 0.75` at `recipe_splitter_style.rs:33`, applied at `:157`) only fades in a **thicker focus-coloured** line, and that same line appears *instantly* on keyboard focus or on drag (`recipe_splitter_style.rs:151-154`). The handle is `.focusable(true)` with arrows / Home / End / Enter (`splitter/handle.rs:232`, `:474-553`) and carries `Role::Splitter` + `on_access_action` (`:555`, `:667`). No theme preset overrides `SplitterStyle`. Hover *emphasis* only — excluded. |
| Docking resize handle, same | Identical: `docking/resize_handle.rs:196-205` drives the same `hover_progress` into the shared `SplitterStyle::make_handle` (`:171`); `.focusable(true)` at `:194`, `on_key` at `:293`, `Role::Splitter` at `:382`. Excluded. |
| Toolbar overflow chevron is hover-revealed | **Wrong.** `crates/teksilo-widgets/src/toolbar.rs` contains **zero** hover references (`grep -c 'on_hover\|is_hovered' → 0`). The chevron is gated on the layout-derived `is_overflowing` signal (`toolbar.rs:796`). Excluded. |
| Tab-bar scroll arrows and the overflow dropdown are hover-revealed | Gated on scroll state, not hover: `tab_widget/bar.rs:1698`, `:1726`, `:1758`. Excluded. |
| Row hover chrome in the five data views | **No such thing exists.** `list_view/body_pane.rs` and `grid_view/body_pane.rs` have zero hover references; `table_view/body_pane.rs:334` and `tree_table_view/body_pane.rs:344` hard-code `is_hovered: false`; `TableStyle::make_row_background` (`styles/recipe_table_style.rs:182`) is never called by the stock views. The only row hover is the *optional* `StandardListItem` / `StandardTreeItem` delegate — a background tint (`styles/recipe_standard_item_style.rs:198-215`) — plus row 11 above. |
| The TableView column-resize grip is hover-discovered | Hover only changes the cursor (`table_view/header.rs:663`, `:697`, `:747`). Press-drag works blind, and the header paints the column separators "that make those grips findable" (`table_view/header.rs:12-13`). There is no keyboard column resize, but that is a 2.5.7 gap — see [the drag census](drag-operation-census.md) row 7 — not a hover-only one. |
| Rich tooltips (tier 2) are hover-only | They have a real keyboard route: focus arms the same delay and the same 2 s `DWELL_PROMOTION` (`tooltip/rich.rs:74`; arming at `overlay_impl.rs:1149-1213`). **Caveat P29 must honour:** that route exists only on a *focusable* anchor. A rich tooltip on a `Badge`, an `Avatar` or any non-focusable decoration is hover-only in practice — and on a disabled anchor, always (row 3). |
| Composite tooltips (tier 3) are hover-only | Same focus route (`tooltip/composite.rs:109`, `attach.rs:276`) for the default `.sticky(true)` form. Only the `.sticky(false)` form is hover-only, which is row 2. |
| The rich-tooltip `[label](:key)` cascade is hover-driven | It opens on a **link click** (`tooltip/rich.rs:383`, `:420`). Excluded. |
| MenuBar hover-switching between open top-level menus | `menu_bar.rs:655-665` fires only when a menu is *already* open; tap (`:645`) and ArrowLeft / ArrowRight (`:684-690`) both cover it. Excluded. |
| Inspector cursor-following bounds tooltip | `crates/teksilo-inspector/src/highlight.rs:115-116` is hover-only, but it is a `cfg(debug_assertions)` developer tool. Exempt. |
| TreeView / ListView drag spring-load ("dwell to expand") | `tree_view/widget_impl.rs:804-807` fires during an **active drag**, which touch reaches through a long-press drag. Exempt. |

## Hover tints — looked at and excluded

No reveal and no functionality behind them; every one is a colour swap or an
interaction-state mirror:

`button.rs:106` · `checkbox.rs:448` · `radio_button.rs:304` · `radio_tile.rs:608` ·
`toggle.rs:296` · `slider.rs:380` · `link.rs:252` · `combo_box.rs:834` ·
`combo_box/item.rs:151` · `search_field.rs:907` · `segmented_control.rs:1065` and
`segmented_control/cell.rs:216` · `breadcrumb.rs:296` · `tool_box.rs:764` ·
`calendar/zoom_grid.rs:344` · `spin_box/step_button.rs:293` ·
`primitives/text_input_field.rs:1281` · `table_view/header.rs:532` ·
`title_bar/controls.rs:203` (the glyph is always painted, `:181-187`) ·
`split_button.rs:732` (`hover_within` → tint) · `standard_item.rs:722` (the tint half) ·
`tab_widget/header.rs:660` · every `is_hovered` consumer in
`crates/teksilo-widgets/src/styles/recipe_*_style.rs` and in
`crates/teksilo-theme-{fluent,macos,material3}/src/styles/*.rs`.

Swept with zero hover-reveal hits: `crates/teksilo-preview-ui/src` (no `on_hover` at
all), `crates/teksilo-webview/src`, `crates/teksilo-terminal/src`,
`crates/teksilo-widgets/src/code_editor/`, `log_view`, `rich_text/`, `accordion.rs`,
`grid_view.rs`, `docking/activity_bar.rs`, `notification/`.

## Cross-references

- The files these affordances live in are rows in
  [the widget pointer inventory](widget-pointer-inventory.md).
- Reveal geometry (the 4 dp vs 8 dp scroll bar, the 6 dp splitter gutter) is in
  [the density inventory](density-inventory.md).
