# Accessibility Testing Plan

## Motivation

Three VoiceOver crashes were fixed reactively by running the app manually.
The root causes — duplicate AccessKit children, dangling `controls`/`described_by`
references, and incorrect widget-tree parent pointers — are all detectable
without a display or screen reader. This plan adds the test infrastructure that
would have caught them in CI before they reached a user.

---

## Point 1 — Add `accesskit_consumer` as a `[dev-dependency]` to `fern-core` ✅

Done. Added to both `crates/fern-core/Cargo.toml` and
`crates/fern-widgets/Cargo.toml`:

```toml
[dev-dependencies]
accesskit_consumer = "0.35.0"
```

---

## Point 2 — Write `assert_a11y_tree_valid` ✅

Implemented in `crates/fern-core/src/widget_tree/accessibility_impl.rs`
inside `pub(crate) mod test_helpers`. The helper feeds a `TreeUpdate` into
`accesskit_consumer::Tree::new`, running the same structural validation that
VoiceOver triggers on activation. It panics on:

- duplicate child NodeId across any two nodes in the same `TreeUpdate`
- a `focus` NodeId not present in the tree
- any node referenced in a relationship that is absent from the tree

Additional helpers in the same module:

- `assert_no_dangling_relationships` — checks every `controls()` /
  `described_by()` target is present in the emitted tree, guarding the
  post-processing pass added to `build_accessibility_tree`
- `nodes_with_role` — collects all NodeIds with a given role

---

## Point 3 — Call it from every existing accessibility test + add per-widget tests ✅

### 3a — Retrofit existing tests ✅

Every test in `accessibility_impl.rs` that calls `sync_accessibility()` ends
with `assert_a11y_tree_valid(&update)`.

### 3b — Per-widget semantic correctness tests

**TabWidget** — done in `crates/fern-widgets/src/tab_widget/a11y_tests.rs`.

| Test | What it asserts |
|---|---|
| `has_exactly_one_tab_list` | exactly 1 `TabList` node |
| `tab_count_matches_widget_count` | N `Tab` nodes for N tabs (n = 2, 3, 4) |
| `exactly_one_tab_panel_initially` | exactly 1 `TabPanel` (active pane only) |
| `tabs_are_descendants_of_tab_list` | BFS from `TabList` reaches all `Tab` nodes |
| `active_tab_has_controls_pointing_into_tree` | `controls()` non-empty and target present |
| `switching_tab_keeps_controls_valid` | 1 `TabPanel` + no dangling refs after each switch |
| `inactive_panels_are_absent_from_tree` | only the active panel appears |
| `access_click_on_tab_updates_selected_and_tree_is_valid` | tree valid after `AccessAction::Click` |
| `disabled_tab_has_no_click_action` | disabled tab lacks `Action::Click`; enabled tabs have it |
| `disabled_tab_still_appears_in_tree` | disabled tabs are visible to AT |
| `keyboard_navigation_visits_each_tab_in_order` | Tab key + ArrowRight move focus and selection correctly |
| `no_extra_focusable_nodes_beyond_tab_headers` | focusable count between N and 2N (no leaked wrapper nodes) |

**ComboBox fix** (companion to the plan, done in `combo_box.rs` +
`combo_box/tests.rs`): `push_controlled` is now conditional on the popup
being open. When closed the listbox is absent from the tree; pointing at it
was a dangling reference of exactly the kind that crashes VoiceOver. The
existing `accessibility_trigger_controls_popup` test was updated to verify
both states.

---

## Remaining widgets (not yet done)

Same pattern as TabWidget — prioritised by relationship complexity:

| Widget | Key assertions |
|---|---|
| Popover | trigger has `has_popup` + `expanded`; content enters/leaves on open/close; no dangling `controls` after close |
| Dialog | same as Popover |
| Accordion / ToolBox | each header `controls` its panel; panel enters/leaves on expand/collapse |
| Checkbox / Toggle / RadioButton | `toggled` matches signal value |
| Slider | `value`, `min`, `max` set and correct |
| ListView / TreeView | items have correct roles; only visible items in tree |

---

## Full Widget A11y Audit — Batch Tracking

Widgets reviewed in batches of 10. Layout primitives (no interactive semantics) are listed separately at the bottom.

### Batch 1 ✅ — Audited and fixed

| Widget | Role | Issues found | Status |
|---|---|---|---|
| Popover | `Dialog` (surface), trigger correct | Surface unnamed — needed `set_name` from trigger label | ✅ Fixed |
| Dialog | `Dialog` (modal), surface correct | Trigger missing `has_popup(Dialog)` + `expanded_when` | ✅ Fixed |
| Accordion | `Button` | Missing `aria-controls` → content; content needed `Region` wrapper | ✅ Fixed |
| ToolBox | `Button` + `Region` | Collapsed panels remained in a11y tree — needed `set_hidden()` | ✅ Fixed |
| Checkbox | `CheckBox` | Unlabeled = unnamed (footgun) — now `debug_assert!` | ✅ Fixed |
| Toggle | `Switch` | Same as Checkbox — unlabeled footgun — now `debug_assert!` | ✅ Fixed |
| RadioButton | `RadioButton` | `set_selected()` → `set_toggled()` (correct ARIA `aria-checked`) | ✅ Fixed |
| Slider | `Slider` | Added `.label()` / `.label_literal()` builder methods | ✅ Fixed |
| ListView | `List` → `ListBox` | Role changed; `ListItemWrapper` now `ListBoxOption` + selection | ✅ Fixed |
| TreeView | `Tree` | `TreeItemWrapper` now sets `position_in_set`, `size_of_set`, selection | ✅ Fixed |

### Batch 2 ✅ — Audited and fixed

| Widget | Role | Issues found | Status |
|---|---|---|---|
| Button | `Button` | Already conventional (set_name, set_disabled, has_popup + expanded, Click/Focus actions) | ✅ No changes |
| ComboBox | `ComboBox` | Never registered `Action::Click` / `Action::Focus` — AT press/focus commands saw empty actions list | ✅ Fixed |
| ProgressBar | `ProgressIndicator` | No accessible name — AT announced bare "progress indicator" with no context. Added `.label()` / `.label_literal()` builders | ✅ Fixed |
| Snackbar (outer) | `GenericContainer` | Dead wrapper node sat between ancestors and the real focusable trigger — now `set_hidden()` | ✅ Fixed |
| TabWidget | `TabList` + `Tab` | — | Done ✅ (see Point 3b above) |
| SegmentedControl | `RadioGroup` + `RadioButton` | Already conventional (set_value, set_selected, focus-contained child radios) | ✅ No changes |
| Link | `Link` | Missing `set_disabled`, `set_url`, and disabled-aware `focusable` / actions. Added `.enabled()` builder | ✅ Fixed |
| Badge | `Label` | Already conventional (set_name) — purely presentational | ✅ No changes |
| Breadcrumb / BreadcrumbItem | `Navigation` + `Link` | Navigation landmark had no accessible name; decorative chevrons were enumerable. Added `.label()` + `set_hidden()` on separators | ✅ Fixed |
| MenuItem / MenuList / MenuBar | `MenuItem`, `Menu`, `MenuBar` | Click action unguarded when disabled; shortcut text not exposed to AT; MenuBarTrigger missing `set_expanded`; redundant `Role::Menu` on MenuOverlayHost; unannotated `KeyboardHighlightWrapper` between Menu and MenuItem | ✅ Fixed |

Fix details:
- `AccessNodeBuilder` gained `set_url()` and `set_keyboard_shortcut()` wrappers over `accesskit::Node`.
- `MenuBarTrigger` now registers `menu_ctx.open_index` as a `RepaintOnly` binding on self, so `set_expanded` reflects which top-level entry is currently open.
- `MenuOverlayHost` changed from `Role::Menu` to `Role::GenericContainer` to avoid two nested Menu nodes per dropdown.
- `KeyboardHighlightWrapper` now sets `Role::GenericContainer` — presentational, the real semantics live on the wrapped `MenuItem`.
- `MenuItem` guards `add_action(Click)` on `enabled` and emits `set_keyboard_shortcut` from the resolved shortcut label (manual or registry-derived).

### Batch 3 — Pending

| Widget | Role | Issues | Status |
|---|---|---|---|
| ScrollArea | — | — | Pending |
| ScrollBar | — | — | Pending |
| SplitView | — | — | Pending |
| Wizard / WizardStep | — | — | Pending |
| MessageBox | — | — | Pending |
| Toolbar | — | — | Pending |
| StatusBar | — | — | Pending |
| TitleBar | — | — | Pending |
| Tooltip / TooltipWidget | — | — | Pending |
| Card | — | — | Pending |

### Batch 4 — Pending

| Widget | Role | Issues | Status |
|---|---|---|---|
| Panel | — | — | Pending |
| GroupBox | — | — | Pending |
| GroupHeader | — | — | Pending |
| BuiltInButton | — | — | Pending |
| SplitButton | — | — | Pending |
| SpinBox | — | — | Pending |
| TextInput / TextInputField | — | — | Pending |
| RadioGroup | — | — | Pending |
| Repeater | — | — | Pending |
| ShortcutSettings | — | — | Pending |

### Batch 5 — Pending

| Widget | Role | Issues | Status |
|---|---|---|---|
| RichTextEditor | — | — | Pending (feature-gated) |
| MenuContext | — | — | Pending |
| ImageWidget | — | — | Pending |
| MasonryLayout | — | — | Pending |
| FormLayout | — | — | Pending |
| Switcher | — | — | Pending |
| AspectRatio | — | — | Pending |
| MaxSize | — | — | Pending |
| GroupBox | — | — | Pending |
| SplitButton | — | — | Pending |

### Layout / Decorative Primitives (no interactive a11y requirements)

These set `Role::GenericContainer` or have no `accessibility()` impl — correct behavior, no audit needed unless they gain interactive semantics:

`HStack`, `VStack`, `ZStack`, `Grid`, `Wrap`, `Padding`, `Spacer`, `Center`, `Expand`, `FixedSize`, `MinSize`, `Divider`, `IconWidget`, `RectWidget`, `TextWidget`, `ImageWidget` (decorative), `MasonryLayout`, `FormLayout`
