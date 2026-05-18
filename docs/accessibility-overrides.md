# Accessibility Overrides Reference

Bastyde widgets declare their own a11y info via `Widget::accessibility(&self, builder: &mut AccessNodeBuilder)` — Button emits `Role::Button` + label, Slider emits `Role::Slider` + numeric range, Panel marks itself `set_hidden()` when it's `a11y_presentational`, etc. That covers ~95% of cases. The remaining 5% — when an icon-only Button needs an accessible label, when a card composite should read as one AT element, when a status region needs `aria-live`, when a custom action should appear in VoiceOver's Actions rotor — is where **builder-level accessibility overrides** come in.

The override layer is a one-method-per-concern surface (`.access_label`, `.access_role`, `.access_merge_subtree`, …) on `WidgetBuilder` and `WidgetWithHandlers`, analogous to SwiftUI's `.accessibility*` modifiers and Flutter's `Semantics(...)`. App authors annotate widgets from the outside without touching widget internals.

```rust
Button::new(tr!("save_icon"))
    .icon(IconWidget::from_svg_icon(save_icon), IconLocation::IconOnly)
    .access_label(tr!("save"))                  // icon-only button needs a name for AT
    .access_shortcut_id("app.save")             // tracks user rebinds via ShortcutRegistry
    .access_action(Action::ShowContextMenu, |ctx| ctx.send_intent(AppIntent::Menu));
```

Mental model in one line:

```text
HandlerSet (carries) → WidgetNode (owns) → tree walker (applies) → accesskit::Node
```

End-to-end example: [examples/widget_catalog/src/main.rs](../examples/widget_catalog/src/main.rs) — the icon-only buttons in the Controls section show the canonical pattern in both builder and `bati!` macro form.

---

## Where overrides live

The override surface piggy-backs on the existing handler-extraction plumbing — the same path that already mirrors `cursor`, `clips_children`, `focus_within_signal` from `HandlerSet` onto `WidgetNode`.

1. **Builder chain** — `Widget::new(...).access_label(...).access_role(...)` each return a `WidgetWithHandlers<W>` whose `HandlerSet` carries an `Option<Box<AccessibilityOverrides>>`. The first `access_*` call lazily allocates the box; subsequent calls extend it.
2. **Insertion** — `WidgetTree::add(...)` calls `take_handler_set()` on the wrapper and `apply_handler_set` ([crates/bastyde-core/src/arena.rs](../crates/bastyde-core/src/arena.rs)) mirrors the box onto the persistent `WidgetNode::access_overrides` field. After this point, the wrapper has no override state — the source of truth is on the node.
3. **AT tree build** — when the framework calls `WidgetTree::sync_accessibility()`, the walker at [crates/bastyde-core/src/widget_tree/accessibility_impl.rs:139](../crates/bastyde-core/src/widget_tree/accessibility_impl.rs) runs `node.widget.accessibility(builder)` first (so the inner widget emits its defaults), then calls `node.access_overrides.apply(builder)` to layer the overrides on top.

Subsequent `sync_accessibility()` calls re-run the walker if the AT cache is dirty — which now also dirties on `ShortcutRegistry::version()` bumps so `access_shortcut_id` tracks rebinds (see [Shortcuts](#shortcuts) below).

---

## Method reference

Naming: `.access_*` prefix throughout. Three tiers by frequency of use.

### Tier 1 — labeling and state

| Method | Sets | Notes |
|---|---|---|
| `.access_label(s)` | `Node::label` | What screen readers announce. Replaces widget-emitted name. |
| `.access_label_literal(s)` | same | `#[doc(hidden)]` grep marker for explicitly untranslated strings. |
| `.access_description(s)` | `Node::description` | Long-form context. |
| `.access_description_literal(s)` | same | `#[doc(hidden)]` grep marker. |
| `.access_hint(s)` | `Node::description` | Alias for `access_description` (SwiftUI parity — AccessKit has no separate hint slot). |
| `.access_hint_literal(s)` | same | `#[doc(hidden)]` grep marker. |
| `.access_value(s)` | `Node::value` | Current value (sliders, spin boxes, text input). |
| `.access_value_literal(s)` | same | `#[doc(hidden)]` grep marker. |
| `.access_role(role)` | `Node::role` | Replace widget-emitted role. |
| `.access_hidden(bool)` | `Node::hidden` flag | `true` hides from AT, `false` un-hides (clears even widget-emitted hidden). |
| `.access_disabled(bool)` | `Node::disabled` flag | `true` marks disabled, `false` clears even arena-driven disabled. |

### Tier 2 — relationships, live regions, identity

| Method | Sets | Notes |
|---|---|---|
| `.access_identifier(s)` | `Node::author_id` | Stable test/debug id (like `data-testid`). Not user-visible. |
| `.access_controls(target_id)` | `Node::controls` | Append. ARIA `aria-controls`. |
| `.access_described_by(target_id)` | `Node::described_by` | Append. |
| `.access_labelled_by(target_id)` | `Node::labelled_by` | Append. |
| `.access_live(mode)` | `Node::live` | Politeness for status regions (`Polite`, `Assertive`). |
| `.access_current(c)` | `Node::aria_current` | Mark this as the current item in its container (`aria-current`). |
| `.access_has_popup(kind)` | `Node::has_popup` | Disclosure flag — `Menu`, `Listbox`, `Dialog`, … |
| `.access_orientation(o)` | `Node::orientation` | Sliders, scrollbars, separators. |

### Tier 3 — subtree modes, numeric, actions, escape hatch

| Method | Effect |
|---|---|
| `.access_exclude_subtree()` | Prune all descendants from the AT tree. Parent still emitted. |
| `.access_merge_subtree()` | Lift descendant labels / values / actions into parent, prune descendants. |
| `.access_subtree(mode)` | Set explicit `AccessSubtreeMode::{Inherit, Exclude, Merge}`. |
| `.access_numeric_value(v)` | `Node::numeric_value`. |
| `.access_numeric_range(min, max)` | `Node::min_numeric_value` + `max_numeric_value`. |
| `.access_numeric_step(s)` | `Node::numeric_value_step`. |
| `.access_action(action, handler)` | Advertise an AT action AND register a callback. |
| `.access_remove_action(action)` | Suppress an action the widget emitted. |
| `.access_custom_action(label, handler)` | SwiftUI `accessibilityAction(named:)` — appears in VoiceOver's Actions rotor. |
| `.access_custom_action_literal(label, handler)` | `#[doc(hidden)]` grep marker. |
| `.access_shortcut_literal(s)` | Pre-formatted chord string (`"Ctrl+S"`). |
| `.access_shortcut_id(id)` | Bind to a registered `Shortcut` id; tracks user rebinds. |
| `.access_customize(\|builder\| ...)` | Final escape hatch — runs last, full `&mut AccessNodeBuilder` access. |

---

## Subtree modes

By default the AT tree mirrors the widget tree one-to-one — every widget emits one AT node, descendants are visible to AT. `access_subtree` controls how the walker handles descendants of the annotated node.

### `Inherit` (default)

Normal walk. Descendants emit their own nodes. Used implicitly everywhere.

### `Exclude` — `access_exclude_subtree()`

Keep the parent in the AT tree, prune all descendants. Equivalent to Flutter's `excludeSemantics: true`.

```rust
HStack::new()
    .child(IconWidget::from_svg_icon(logo_icon))
    .child(TextWidget::new_literal("Bastyde"))
    .child(TextWidget::new_literal("Pure-Rust GUI"))
    .access_label_literal("Bastyde logo")
    .access_exclude_subtree();
```

Without `access_exclude_subtree`, a screen reader would walk all three children individually: "graphic", "Bastyde", "Pure-Rust GUI". With it, AT sees one node named "Bastyde logo".

Use for purely decorative composites: animated logos, icon clusters, splash content.

### `Merge` — `access_merge_subtree()`

Keep the parent, but **lift descendants' a11y info into the parent** before pruning. The whole composite reads as one AT element. Equivalent to Flutter's `mergeAllDescendants: true` and SwiftUI's `.accessibilityElement(children: .combine)`.

```rust
Card::new()
    .child(TextWidget::new_literal("New message"))
    .child(TextWidget::new_literal("From Alice"))
    .child(TextWidget::new_literal("Hey, are we still on for…"))
    .access_merge_subtree();
```

VoiceOver announces the card as one element: "New message · From Alice · Hey, are we still on for…". Tab-stops collapse, so a keyboard user moves card-by-card instead of line-by-line within the card.

**Merge accumulator rules:**

| Source | Merged into parent | Rule |
|---|---|---|
| descendant `name` | parent `name` | Append with single space; existing parent name kept first if any. |
| descendant `value` | parent `value` | First non-empty wins. |
| descendant supported actions | parent action set | Union, deduplicated. |
| descendant `role` | — | Discarded. Parent's role wins. |
| descendant `numeric_value` / range / step | — | Discarded. |
| descendant `hidden` / `disabled` | — | Discarded. Parent's state governs the merged element. |
| descendant `description` / `controls` / `described_by` / `labelled_by` | — | Currently dropped (no `AccessNodeBuilder` getters); use `access_customize` on the parent if you need them. |

**Nested subtree modes:**

- `Merge` containing `Exclude` somewhere — Exclude wins for that subtree (descendants of the excluded node contribute nothing to the merge).
- `Merge` containing `Merge` — the inner merge runs first into a temp builder, the outer merge then absorbs the inner's already-merged label as one element.
- `Exclude` containing anything — outer Exclude prunes everything; inner modes never run.

**What merge can't reach.** Widgets that don't expose their internals to the arena (e.g. a hand-rolled `paint()`-only widget that draws its own icon + label without inserting child `WidgetNode`s) have no descendants for the merge walker to find. Those widgets' authors should set `accessibility()` correctly internally; consumers can still use `.access_label(...)` to override the parent. This is an inherent property of the arena-based tree, not a deferred feature.

---

## Action callbacks

AT-invoked actions arrive as `WidgetEvent::AccessAction { action, target, target_node, data }` on the widget's `on_access_action` handler. The override system layers an additional callback path on top of any user-installed handler.

### Standard actions — `access_action`

```rust
use bastyde::core::accesskit::Action;

let widget = my_widget
    .access_action(Action::ShowContextMenu, |ctx| {
        ctx.send_intent(AppIntent::OpenMenu);
    })
    .access_action(Action::Increment, |ctx| {
        ctx.send_intent(AppIntent::StepUp);
    });
```

Both calls advertise the action on the AT node AND register the callback. Multiple `access_action` calls register separate callbacks for distinct actions; the dispatcher routes each invoked action to the matching callback.

**Layering with `on_access_action`.** If the developer also calls `.on_access_action(|action, ctx| …)` directly, both fire for the same dispatched event — the override-registered callback first, then the user's catch-all. Builder ordering doesn't matter; the dispatcher reads `node.access_overrides.actions` directly.

### Action suppression — `access_remove_action`

A widget like Button emits `Action::Click` and `Action::Focus` unconditionally. To neutralize one (e.g. a Button used purely as a layout shim that shouldn't appear clickable to AT):

```rust
my_button.access_remove_action(Action::Click);
```

Applied after the widget's `accessibility()` runs but before override-advertised actions are added — so a subsequent `.access_action(Action::Click, …)` call re-advertises Click with the override's callback.

### Custom-named actions — `access_custom_action`

SwiftUI's `accessibilityAction(named:)` parity. The label is exposed verbatim by AT software (e.g. VoiceOver's Actions rotor reads "Reply to message").

```rust
my_message
    .access_custom_action(tr!("reply_now"), |ctx| {
        ctx.send_intent(AppIntent::Reply);
    })
    .access_custom_action(tr!("delete"), |ctx| {
        ctx.send_intent(AppIntent::Delete);
    });
```

Each entry is assigned a stable `i32` id in declaration order. AT triggers a custom action via `WidgetEvent::AccessAction { action: Action::CustomAction, data: Some(ActionData::CustomAction(idx)), .. }` and the dispatcher routes by `idx` into `access_overrides.custom_actions`.

---

## Shortcuts

Two variants for announcing a chord on the AT node — pick by where the binding lives.

### `.access_shortcut_id("app.save")` — the production path

Bind to a `Shortcut` registered in [`ShortcutRegistry`](../crates/bastyde-core/src/shortcut.rs). The walker resolves the current effective primary keystroke at AT-build time and writes it via `KeyStroke::Display` (`"Ctrl+S"`). On a user rebind via `ShortcutSettings`, the registry's `version()` signal bumps and `sync_accessibility` dirties the AT cache automatically — the announcement updates without any explicit signaling from the settings UI.

```rust
// Somewhere in your root widget's build(), register the Shortcut.
ctx.register_shortcut_global(
    Shortcut::new("app.save").name("Save")
        .primary(KeyStroke::ctrl(Key::S))
        .build(),
);
ctx.register_action(Action::new("app.save").on_invoke(|_, ctx| save(ctx)));

// On the Save button, bind the AT announcement to the same id.
Button::new(tr!("save"))
    .on_activate_fn(|ctx| ctx.send_intent(AppIntent::Save))
    .access_shortcut_id("app.save");
```

If the registry has no entry for `id` yet (registration hasn't happened, or the app spelled the id wrong), the announcement is silently omitted — same fallback as `MenuItem::for_shortcut(...)` and `TooltipContent::for_shortcut(...)`.

### `.access_shortcut_literal("Ctrl+S")` — the explicit-string path

Frozen pre-formatted string. Use for chords NOT going through the `Shortcut` system: platform-native keys (Tab, Esc), app-internal hotkeys not exposed to user rebinding, or stand-alone demos.

```rust
my_button.access_shortcut_literal("Ctrl+Shift+P");
```

Does NOT track rebinds — that's the literal variant's tradeoff. For chords routed through `Shortcut`, prefer `access_shortcut_id` or the announcement and the actual binding will drift.

See [shortcut-intent-action.md](shortcut-intent-action.md) for the full Shortcut/Intent/Action pipeline.

---

## Internationalization

User-visible string methods (`access_label`, `access_description`, `access_hint`, `access_value`, `access_custom_action`) accept `impl Into<String>`. With the `i18n` feature enabled, [`bastyde_i18n::LocalizedString`](../crates/bastyde-i18n/src/localized_string.rs) (the type produced by `tr!(...)`) implements `From<LocalizedString> for String`, so:

```rust
button
    .access_label(tr!("save"))                       // Fluent-translated
    .access_description(tr!("save_explanation"))
    .access_custom_action(tr!("publish_now"), |ctx| ctx.send_intent(AppIntent::Publish));
```

flows through unchanged. Translation is resolved eagerly at builder time. Locale changes rebuild the composite, which re-runs the builder chain and stores the new translation. Same model as `Button::new(impl Into<LocalizedString>)`.

The `_literal` twins (`access_label_literal`, etc.) are `#[doc(hidden)]` grep markers for explicitly-untranslated call sites. Same convention as `Button::new_literal`/`tooltip_literal`. Use literal variants in tests, demo scaffolding, and any string the app explicitly does not translate.

---

## State clearing

`AccessNodeBuilder::set_hidden()` and `set_disabled()` flip flags on. Some widgets call those setters unconditionally (e.g. [Panel](../crates/bastyde-widgets/src/panel.rs) calls `set_hidden()` when `a11y_presentational`). To **un-set** widget-emitted state, the override system exposes:

- `.access_hidden(false)` — clears even widget-emitted `set_hidden()`. Full clear (no framework re-application of hidden).
- `.access_disabled(false)` — clears widget-emitted disabled AND arena-driven disabled. The framework's gate at [`accessibility_impl.rs`](../crates/bastyde-core/src/widget_tree/accessibility_impl.rs) respects the override, so even a `.disabled(true)` set on the widget's enabled-state can be overridden for AT purposes.

Real use cases:

1. App author wraps a `Panel` configured as decorative but a screen reader user *does* need to know about it ("Settings panel — collapsed").
2. App author force-disables a Button visually pending a save, but wants AT to keep announcing it as enabled because the disabled state is transient.
3. Test scaffolding asserts a widget is exposed regardless of internal `a11y_presentational` plumbing.

---

## Synthetic children — `access_customize`

Widgets like `RichTextEditor` emit synthetic AT children (paragraphs, text-runs) via `push_paragraph_child` / `push_text_run_child` — these live inside the parent's emitted Node, not as separate `WidgetNode`s in the arena. The override system can't reach them through `.access_*` modifiers (which target whole widgets, not sub-nodes).

The supported path is `access_customize`, which runs **last** in the apply pipeline with full `&mut AccessNodeBuilder` access:

```rust
my_widget.access_customize(|builder| {
    // builder.inner_mut() exposes the underlying accesskit::Node — any
    // AccessKit field the typed surface doesn't cover is reachable.
    builder.inner_mut().set_class_name("custom-widget");
    builder.inner_mut().set_role_description("special panel");
});
```

Same escape-hatch model AccessKit itself uses internally. The closure runs every time the AT tree is built, so it has consistent re-render semantics with the rest of the override layer.

---

## Apply order — the full pipeline

For each widget the AT walker visits, the sequence is:

1. **`node.widget.accessibility(&mut builder)`** — inner widget emits role, name, value, actions, hidden/disabled, etc.
2. **`overrides.apply(&mut builder)`** in this order:
    1. Scalars: `label`, `description`, `value`, `role` (replace if `Some`).
    2. State flags: `hidden`/`disabled` set or clear based on `Some(true)`/`Some(false)`.
    3. Identity: `identifier` → `set_author_id`.
    4. Relationships: `controls`, `described_by`, `labelled_by` (append).
    5. Live region / `aria_current` / `keyboard_shortcut` literal / `has_popup` / `orientation`.
    6. Numeric: `numeric_value`, `min`, `max`, `step`.
    7. Action suppression: `removed_actions` (call `remove_action`).
    8. Action advertisement: `actions` (call `add_action`).
    9. Custom actions: write `Vec<accesskit::CustomAction>` with sequential ids.
    10. **`customize`** closure runs last with full `inner_mut()` access.
3. **Walker post-processing**:
    1. Resolve `access_shortcut_id` against `ShortcutRegistry` (this needs tree access, so it lives outside `apply()`).
    2. Subtree dispatch — for `Merge`, walk descendants and absorb into the current builder; for `Exclude`, prune.
4. **Framework finalization**:
    1. Push child NodeIds (skipped for Exclude / Merge).
    2. Inject layout bounds.
    3. Re-apply `set_disabled()` from the arena's enabled-flag UNLESS the override has `disabled: Some(false)`.
    4. Tooltip → `push_described_by`.
    5. `builder.build(id)` → produces `(NodeId, accesskit::Node, synthetic_children)`.

---

## Testing patterns

All headless. Assertions go through `WidgetTree::accessibility_node(id)` (synthetic snapshot) or `WidgetTree::sync_accessibility()` (full TreeUpdate, useful when checking pruning, custom_actions, controls relationships, etc.).

```rust
use bastyde::core::accesskit::{Action, Role, HasPopup};

// Scalar override
let mut tree = WidgetTree::new();
let id = tree.add(MyWidget.access_label_literal("Publish"));
tree.layout(SizeProposal::exact(100.0, 40.0));
assert_eq!(tree.accessibility_node(id).name(), Some("Publish"));

// Action callback
let flag = Signal::new(false);
let cb = flag.clone();
let id = tree.add(MyWidget.access_action(Action::ShowContextMenu, move |_| cb.set(true)));
tree.layout(...);
tree.dispatch_event(WidgetEvent::AccessAction {
    action: Action::ShowContextMenu,
    target: Some(id),
    target_node: widget_id_to_node_id(id),
    data: None,
});
assert!(flag.get());

// Subtree merge
let title = tree.add(FillWidget::new().label("Title"));
let body = tree.add(FillWidget::new().label("Body"));
let card = tree.add(StackWidget::new().add_child(title).add_child(body).access_merge_subtree());
tree.layout(...);
let update = tree.sync_accessibility();
assert_eq!(tree.text_content(card), Some("Title Body".to_string()));
// Children pruned from output:
assert!(find_node(&update, title).is_none());
assert!(find_node(&update, body).is_none());

// Shortcut id auto-tracks rebinds
tree.shortcut_registry_mut().register(
    Shortcut::new("app.save").name("Save").primary(KeyStroke::ctrl(Key::S)).build(),
);
let id = tree.add(MyButton.access_shortcut_id("app.save"));
let node = find_node(&tree.sync_accessibility(), id).unwrap();
assert_eq!(node.keyboard_shortcut(), Some("Ctrl+S"));
tree.shortcut_registry_mut().rebind_primary("app.save", Some(KeyStroke::ctrl(Key::Q)));
let node = find_node(&tree.sync_accessibility(), id).unwrap();
assert_eq!(node.keyboard_shortcut(), Some("Ctrl+Q"));
```

The 36 in-crate tests at [`crates/bastyde-core/src/widget_tree/accessibility_impl.rs`](../crates/bastyde-core/src/widget_tree/accessibility_impl.rs) cover every method in this reference, including all subtree-mode edge cases (nested Exclude-in-Merge, Merge-in-Merge), action-callback layering with `on_access_action`, custom-action dispatch by index, state-clearing for both hidden and disabled, and the `i18n` `Into<String>` conversion path.

---

## End-to-end demo

[`examples/widget_catalog`](../examples/widget_catalog/src/main.rs) — the icon-only buttons in the Controls section show the canonical pattern in both builder and `bati!` macro form. To verify a11y output against a real assistive tech stack:

```bash
cargo run -p widget-catalog
# In another terminal:
accerciser   # Linux AT-SPI inspector
```

Navigate to the icon-only Save button; confirm the announced name is "Save", the keyboard shortcut field reads "Ctrl+S", and the chevron-down sibling reads "More options" with `has_popup = Menu`.

---

## Styling never touches accessibility

The Tier-3 styling system (see [styling-system.md](styling-system.md)) lets an app swap a widget's entire chrome — `Button::style(MyGlassButton)`, `theme.style_slots.toggle = Some(...)`, an image-backed theme — but **style trait impls do not participate in the accessibility tree**. A `*Style::make_body` return is decoration only; the widget owns its `accessibility(builder)` output and all `.access_*` overrides regardless of which style is installed. A glassmorphism button and the default `RecipeButtonStyle` button announce identically. This keeps AT identity stable across theme swaps and reskins — switching themes at runtime never disturbs a screen-reader's cursor or the AccessKit node ids.

## Related references

- [styling-system.md](styling-system.md) — the four-tier styling ladder; style traits decorate, they do not annotate.
- [shortcut-intent-action.md](shortcut-intent-action.md) — the `Shortcut` / `Intent` / `Action` pipeline that `.access_shortcut_id` binds to.
- [events-and-gestures.md](events-and-gestures.md) — `on_access_action` and `on_access_action_request` event handlers (what `.access_action` layers on top of).
- [reactive-theme.md](reactive-theme.md) — how locale and theme changes propagate via composite rebuilds (the same mechanism keeps `.access_label(tr!(...))` translations current).
- [bati-macro-reference.md](bati-macro-reference.md) — `bati!` DSL syntax for `name: value` body items, used by the catalog demo's `controls_bati` block.
- [crates/bastyde-core/src/widget_builder.rs](../crates/bastyde-core/src/widget_builder.rs) — `AccessibilityOverrides` struct, `AccessSubtreeMode` enum, every `access_*` method definition.
- [crates/bastyde-core/src/widget_tree/accessibility_impl.rs](../crates/bastyde-core/src/widget_tree/accessibility_impl.rs) — walker integration, `merge_descendants_into` helper, the 36 unit tests.
