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

### Batch 3 ✅ — Audited and fixed

| Widget | Role | Issues found | Status |
|---|---|---|---|
| ScrollArea | `ScrollView` | Already conventional (clips_children, offsets/min/max, conditional ScrollUp/Down/Left/Right actions) | ✅ No changes |
| ScrollBar | (hidden) | Already conventional — deliberately `set_hidden()` so AT scrolls via the parent ScrollView instead | ✅ No changes |
| SplitView | `Splitter` | `ClipPane` wrappers leaked `Role::Unknown` nodes between the splitter and real pane content | ✅ Fixed |
| Wizard / WizardStep | `GenericContainer` + `Region` | `WizardHeader` rendered "Step 2 of 4" visually but never exposed `position_in_set` / `size_of_set`; `WizardFlow` was an untitled `GenericContainer` instead of a navigable `Region` landmark | ✅ Fixed |
| MessageBox | `AlertDialog` | Already conventional (modal, Live::Assertive, Focus action, Enter/Escape shortcuts) | ✅ No changes |
| Toolbar | `Toolbar` | No accessible name; inner `Panel` wrapper emitted a dead `Role::Group` between the toolbar and its items | ✅ Fixed |
| StatusBar | `Status` + `Live::Polite` | Was a plain `GenericContainer` with no live region; dynamic status text announced incorrectly; same Panel-wrapper dead node as Toolbar | ✅ Fixed |
| TitleBar / WindowControls | `Banner` landmark + `Group` + `Button`s | TitleBar had no `accessibility()` impl; ControlButton exposed **glyph characters** (`—`, `□`, `×`) as names — AT pronounced "em dash / white square / multiplication sign"; DragRegion leaked a blank `Role::Unknown` node | ✅ Fixed |
| Tooltip / RichTooltip | `Tooltip` (`Dialog` when sticky) | Anchor→tooltip `described_by` wiring already existed in the a11y post-processing pass; sticky rich tooltips were not focusable and offered no keyboard-focus path to the promoted surface | ✅ Fixed |
| Card | `Group` | Already conventional | ✅ No changes |

Fix details:
- `SplitView` — `ClipPane::accessibility` now calls `set_hidden()`; the real pane content keeps its own semantics, and no `Role::Unknown` nodes show up between the splitter and the panes.
- `Wizard` — `WizardHeader` now emits `set_position_in_set(current+1)` / `set_size_of_set(total)` and binds `current_step` at `BindingLevel::AccessibilityOnly` so AT announces step progression reactively without rebuild/relayout. `WizardFlow` upgraded from `GenericContainer` to `Role::Region` with the localised name "Wizard content".
- `Panel` — added `a11y_presentational()` builder that flips the wrapper to `set_hidden()`; used by both `Toolbar` and `StatusBar` to flatten their a11y trees.
- `Toolbar` — added `.label(LocalizedString)` / `.label_literal(&str)` builders; defaults to the localised "Toolbar" string when unset. Inner `Panel` is now presentational.
- `StatusBar` — role flipped to `Role::Status`, `set_live(Live::Polite)` so dynamic status text announces without interrupting; inner `Panel` is presentational.
- `TitleBar` — top-level now has `accessibility()` emitting `Role::Banner` (landmark) with the localised name "Window title bar". `WindowControls` emits `Role::Group` with "Window controls". `ControlButton` now carries a `Signal<String>` a11y name (bound at `AccessibilityOnly`), populated by `WindowControls` from localised strings ("Minimize" / "Maximize" / "Restore" / "Close"); the glyph is kept for paint but hidden from AT. `DragRegion::accessibility` calls `set_hidden()` — pointer-only affordance with no AT analogue.
- `RichTooltip` — sticky promotion now adds `Action::Focus` + `focusable(true)`; keyboard users can Tab into the promoted Dialog to reach inline links and the "more" disclosure. The sticky signal is bound at `AccessibilityOnly` so the role flip (Tooltip → Dialog) and new focus action reach AT without a repaint.
- Tooltip focus-promotion — new `tooltip_focus_enter` / `tooltip_focus_leave_outside` wired into `focus_with_origin`. When a keyboard user focuses an anchor with a rich tooltip attached, the tooltip appears immediately and is pre-promoted to sticky (bypassing the 2 s pointer dwell). The tooltip dismisses automatically when focus moves outside both the anchor's subtree and the tooltip's content subtree, preventing sticky-tooltip accumulation as the user Tabs through a form. Plain tooltips are unaffected — their text still reaches AT via the existing `described_by` wiring, which is the W3C-recommended pattern for supplementary hints.
- New FTL keys in fern-widgets: `a11y_toolbar_name`, `a11y_title_bar_name`, `a11y_window_controls_name`, `a11y_window_minimize_name`, `a11y_window_maximize_name`, `a11y_window_restore_name`, `a11y_window_close_name`, `a11y_wizard_progress_name`, `a11y_wizard_content_name` (en-US + fr-FR).

### Batch 4 ✅ — Audited and fixed

| Widget | Role | Issues found | Status |
|---|---|---|---|
| Panel | `Group` (or hidden via `a11y_presentational`) | Already conventional (presentational mode already exists from Batch 3) | ✅ No changes |
| GroupBox | `Group` | Checkable-but-unchecked state wasn't reflected as `set_disabled()` — AT announced the group as interactive while dispatcher blocked its content | ✅ Fixed |
| GroupHeader | `Label` | Already conventional (`set_name` from label, adjacent `TextWidget` marked `a11y_hidden`, has dedicated a11y tests) | ✅ No changes |
| IconButton (then BuiltInButton) | `Button` (+ `set_toggled` for toggle mode) | No tooltip ⇒ unnamed button (silent footgun); `toggled` signal was only bound `RepaintOnly`, so AT never observed toggle flips | ✅ Fixed |
| SplitButton | `Button` with dropdown | Missing `set_has_popup(Menu)` + `set_expanded` — screen readers announced a plain button with no hint that a menu was attached or currently open | ✅ Fixed |
| SpinBox | `SpinButton` | Already conventional (full numeric API, `Increment`/`Decrement`/`SetValue`/`Focus`, value signal bound `AccessibilityOnly`) | ✅ No changes |
| TextInput / TextInputField | `TextInput` on the field + `GenericContainer` on the composite | Field was missing `Action::ReplaceSelectedText` — password managers / voice-control "replace 'foo' with 'bar'" commands had nothing to target | ✅ Fixed |
| RadioGroup | `RadioGroup` | Already conventional (container name, radio children push each sibling via `push_to_radio_group`) | ✅ No changes |
| Repeater | (hidden by default; `Role` on opt-in) | Unconditionally emitted `Role::List`, wrong for the widget's documented uses (toolbar buttons, tag chips, chapter lists) — AT announced a dead "list" wrapper around non-list content | ✅ Fixed |
| ShortcutSettings | `Group` + `Role::Status` live region on capture | No `accessibility()` impl at all — top-level role defaulted to `Unknown`; "Press any key…" capture hint was silent to AT | ✅ Fixed |

Fix details:
- `GroupBox` — `accessibility()` now calls `builder.set_disabled()` when `checked == Some(false)`, matching the dispatcher-level enabled-propagation that already blocks descendant events. The `checked` signal is bound to the group's own a11y node at `BindingLevel::AccessibilityOnly` so the state flips without a relayout.
- `IconButton` (then `BuiltInButton`) — added `debug_assert!(self.tooltip_text.is_some(), …)` mirroring the Checkbox/Toggle footgun from Batch 1; release builds fall back to the string "Button" so AT is never completely silent. When `toggle(...)` mode is active, the `toggled` signal is now bound `AccessibilityOnly` in addition to `RepaintOnly`, so `set_toggled()` refreshes every flip.
- `SplitButton` — added a private `menu_open: Signal<bool>` bound `AccessibilityOnly`. Both overlay-open paths (chevron click, ArrowDown key) set it `true` and register an `on_dismiss` callback that flips it back to `false`. `accessibility()` now emits `set_has_popup(HasPopup::Menu)` + `set_expanded(menu_open.get())`. `selected` is also bound `AccessibilityOnly` so the main-region name (driven by the promoted item) refreshes in AT.
- `TextInputField` — added `builder.add_action(Action::ReplaceSelectedText)` alongside the existing `SetValue`/`SetTextSelection`, guarded by `!read_only`.
- `Repeater` — default `accessibility()` now emits `set_hidden()`; items therefore attach semantically to the Repeater's parent instead of a generic wrapper. Two opt-in builders — `.a11y_role(Role)` and `.a11y_label(impl Into<LocalizedString>)` (+ `a11y_label_literal` shim) — let callers surface a genuine `List`/`Menu`/`Toolbar` when the children really do form one.
- `ShortcutSettings` — added a top-level `accessibility()` impl emitting `Role::Group` with the localised name "Shortcut settings"/"Paramètres des raccourcis". During capture, the row's keystroke cell is promoted to a new private `LiveStatusText` widget that emits `Role::Status` + `set_live(Live::Polite)`, so screen readers announce the "Press any key…" hint the instant Rebind is clicked. Static keystroke cells stay plain labels. Moved the hint text to an FTL key so it translates alongside the widget name.
- New FTL keys in fern-widgets: `a11y-shortcut-settings-name`, `a11y-shortcut-settings-capture-hint` (en-US + fr-FR).

### Batch 5 ✅ — Audited and fixed

The original Batch 5 list included three stale entries (**GroupBox** and **SplitButton** were already fixed in Batch 4; **MenuContext** is an internal `pub(crate)` coordinator struct, not a `Widget`). Those are dropped from the table below.

| Widget | Role | Issues found | Status |
|---|---|---|---|
| RichTextEditor | `MultilineTextInput` / `Document` | Already emits the right role, walks its flow snapshot into Paragraph/TextRun children, handles read-only vs editor naming. Missing `Action::Focus` (AT-initiated focus), and editor mode didn't register `Action::ReplaceSelectedText` (voice-control / password-manager "replace 'foo' with 'bar'" had no target) | ✅ Fixed |
| MasonryLayout | `GenericContainer` | Already conventional — layout-only wrapper for independent items; no inherent list/grid semantics | ✅ No changes |
| ImageWidget | `Image` (+ `set_name` from alt) | Silent-footgun when `.alt()` was never called — unnamed image announced as bare "image"; no way to opt out for purely decorative images | ✅ Fixed |
| FormLayout | `Form` landmark / `GenericContainer` | Emitted an unnamed `Role::Form` landmark — AT announced a bare "form" landmark indistinguishable from any other, and landmark navigation lost its value | ✅ Fixed |
| Switcher | (hidden) | Emitted a `GenericContainer` wrapper around an internal `ZStack` wrapper — dead node chain between parent and the one visible child | ✅ Fixed |
| AspectRatio | (hidden) | Empty `accessibility()` impl → `Role::Unknown` dead node between parent and the constrained child | ✅ Fixed |
| MaxSize | (hidden) | No `accessibility()` impl at all → default `Role::Unknown` dead node | ✅ Fixed |

Fix details:
- **RichTextEditor** — `accessibility()` now emits `Action::Focus` unconditionally, and (in editor mode) `Action::ReplaceSelectedText` alongside the existing `SetValue`/`SetTextSelection`. No changes to the sophisticated flow-snapshot → Paragraph/TextRun walk or the `read_only` / role flip that were already correct.
- **ImageWidget** — added `a11y_hidden: bool` field + `.a11y_hidden()` builder mirroring [TextWidget::a11y_hidden at primitives/text_widget.rs:232](../../crates/fern-widgets/src/primitives/text_widget.rs#L232). `accessibility()` now `debug_assert!`s that either `.alt(...)` or `.a11y_hidden()` was called (matches the Checkbox/Toggle/BuiltInButton footgun pattern from Batches 1 and 4); when `a11y_hidden` is set the node is dropped via `set_hidden()`.
- **FormLayout** — added `.label(LocalizedString)` / `.label_literal(&str)` builders matching the Toolbar / Breadcrumb pattern. When labelled, emits `Role::Form` + `set_name`; when unlabelled, demotes to `Role::GenericContainer` rather than leak an unnamed landmark. No FTL additions — the fallback is *structural* (drop the landmark), not a default name.
- **Switcher / AspectRatio / MaxSize** — all three now call `builder.set_hidden()`. Semantics attach to the wrapper's parent. Matches the ClipPane (Batch 3), Snackbar-outer (Batch 2), and Repeater-default (Batch 4) precedent for layout-only wrappers.

### Layout / Decorative Primitives (no interactive a11y requirements)

These set `Role::GenericContainer` or have no `accessibility()` impl — correct behavior, no audit needed unless they gain interactive semantics:

`HStack`, `VStack`, `ZStack`, `Grid`, `Wrap`, `Padding`, `Spacer`, `Center`, `Expand`, `FixedSize`, `MinSize`, `Divider`, `IconWidget`, `RectWidget`, `TextWidget`

Layout wrappers that now call `set_hidden()` to avoid emitting dead `Role::Unknown` / presentational wrapper nodes (see Batches 3–5): `AspectRatio`, `MaxSize`, `Switcher`, `ClipPane` (Batch 3), `Snackbar` outer wrapper (Batch 2), `Repeater` default (Batch 4), `DragRegion` (Batch 3), `Panel::a11y_presentational()` (Batch 3).

Not audited (not a `Widget`): `MenuContext` is an internal `pub(crate)` coordinator struct shared between the MenuBar trigger and the overlay host; it has no `Widget` impl and no accessibility surface.
