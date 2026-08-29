<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# StandardListItem

Canonical row layout for `ListView` / `TreeView` delegates.

Two widgets:
- `StandardListItem` — primary line `[checkbox?] [leading_slot?]
  [center_slot?] [label] [Spacer] [trailing_slot?]` with optional
  subtitle line `[subtitle_leading_slot?] [subtitle] [Spacer]
  [subtitle_trailing_slot?]`.
- `StandardTreeItem` — same plus depth-driven indent + chevron
  column (always reserved, even for leaves, so labels at the same
  depth align).

Selection / hover / pressed background mirrors `MenuItem` /
`ComboBox`: rounded `RectWidget` (`item_corner_radius: 8.0`),
horizontally inset so corners are visible, theme-driven via
`SurfaceRole` so light/dark/custom themes propagate without
rebuild.

## Canonical TreeView wiring

```ignore
use teksilo::data::{TreeCheckedModel, TreeModel};
use teksilo::widgets::{StandardTreeItem, TreeView};

let tree: TreeModel<Item> = ...;
let checks = TreeCheckedModel::new(tree.clone());

TreeView::new_with_context(tree, move |item, entry, selected, ctx| {
    let mut row = StandardTreeItem::new(lit!(item.title.clone()))
        .from_entry(entry)
        .selected(selected)
        .leading_slot(IconWidget::from_svg(FOLDER_ICON).icon_size(16.0))
        .on_toggle_rc(ctx.toggle_callback());
    if entry.has_children {
        row = row.tristate_checkbox(checks.signal_for(entry.node_id));
    } else {
        row = row.checkbox(checks.bool_signal_for(entry.node_id));
    }
    Box::new(row)
})
.row_click_expands(false)   // chevron is the only toggle target
```

Wiring rules:
- `TreeView::new_with_context` exposes a `TreeRowContext` that
  yields `toggle_callback()` for chevron clicks. Pair with
  `.row_click_expands(false)` so body clicks don't also toggle.
- For tristate parent rows, bind to `signal_for(node)`. For
  leaves, prefer `bool_signal_for(node)` — the model's bool ↔
  tristate bridge runs ancestor recompute on writes either way.
- `from_entry(&FlatEntry)` is shorthand for
  `.depth(entry.depth).has_children(entry.has_children)
  .is_expanded(entry.is_expanded)`.

## Accessibility

`StandardListItem.accessibility()` sets the row's `name` (label
only) and `description` (subtitle, if any) — structural role +
position/level/expanded/selected come from the parent's
`ListItemA11y` / `TreeRowA11y` wrapper. The embedded `Checkbox`
receives an `access_label*` override carrying the row label so
screen readers announce "checkbox, checked, `[label]`" rather than
a nameless `Role::CheckBox`. The chevron's `TwistArrow` is
decorative (`set_hidden`); the row's expanded state is owned by
the wrapper.

## Builder methods at a glance

`style`, `subtitle`, `leading_slot`, `leading_slot_boxed`, `center_slot`, `center_slot_boxed`, `trailing_slot`, `trailing_slot_boxed`, `subtitle_leading_slot`, `subtitle_leading_slot_boxed`, `subtitle_trailing_slot`, `subtitle_trailing_slot_boxed`, `checkbox`, `tristate_checkbox`, `selected`, `enabled`, `label_style`, `subtitle_style`, `label_color`, `subtitle_color`, `interaction_signal`, `label_slot`, `label_overflow`, `subtitle_overflow`, `tooltip`, `rich_tooltip`, `rich_tooltip_content`, `composite_tooltip`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/standard_item/index.html)

## `pub struct StandardListItem`

Canonical single-line or two-line row layout for use in a `ListView`.

See the `module-level documentation` for the full slot layout and
wiring rules.

```rust
pub struct StandardListItem { /* fields */ }
```

### Methods

#### `pub fn new(label: impl Into<LocalizedString>) -> Self`

Create a list item with the given primary label.

#### `pub fn style(mut self, style: impl teksilo_core::styles::StandardItemStyle) -> Self`

Per-call style override. Replaces the theme-wide default
`StandardItemStyle` for just this row instance.

#### `pub fn subtitle(mut self, text: impl Into<LocalizedString>) -> Self`

Set an optional secondary line below the primary label.

#### `pub fn leading_slot(mut self, widget: impl Widget + 'static) -> Self`

Leading slot — placed AFTER the optional checkbox, BEFORE the
center slot. Typical: `IconWidget`, avatar, color swatch.

#### `pub fn leading_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self`

`Box<dyn Widget>` variant of `leading_slot`.

#### `pub fn center_slot(mut self, widget: impl Widget + 'static) -> Self`

Center slot — placed BETWEEN the leading slot and the label.
Typical: status dot, colored category bar, drag-handle gripper,
key-binding chip. Distinct from `leading_slot`: leading is the
row's icon identity, center is label-adjacent decoration.

#### `pub fn center_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self`

`Box<dyn Widget>` variant of `center_slot`.

#### `pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self`

Trailing slot — placed AFTER the flex Spacer on the primary
line. Typical: badge, count, status pill, secondary IconButton.

#### `pub fn trailing_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self`

`Box<dyn Widget>` variant of `trailing_slot`.

#### `pub fn subtitle_leading_slot(mut self, widget: impl Widget + 'static) -> Self`

Leading slot for the subtitle line. No-op without `subtitle(...)`.

#### `pub fn subtitle_leading_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self`

`Box<dyn Widget>` variant of `subtitle_leading_slot`.

#### `pub fn subtitle_trailing_slot(mut self, widget: impl Widget + 'static) -> Self`

Trailing slot for the subtitle line. No-op without `subtitle(...)`.

#### `pub fn subtitle_trailing_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self`

`Box<dyn Widget>` variant of `subtitle_trailing_slot`.

#### `pub fn checkbox(mut self, checked: Signal<bool>) -> Self`

Optional two-state checkbox at the start of the row.
Mutually exclusive with `tristate_checkbox` — last call wins.

#### `pub fn tristate_checkbox(mut self, state: Signal<CheckState>) -> Self`

Optional tri-state checkbox bound to `Signal<CheckState>`.
Cycles `Unchecked → Checked → Indeterminate`. Mutually
exclusive with `checkbox` — last call wins.

#### `pub fn selected(mut self, selected: impl Into<Prop<bool>>) -> Self`

Set the selection state, statically or reactively via a bound
`Signal<bool>`.

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Set the enabled state, statically or reactively via a bound
`Signal<bool>` / `Prop<bool>`.

#### `pub fn label_style( mut self, style: impl Into<teksilo_core::color_prop::TextStyleProp>, ) -> Self`

Override the label's text style (font, size, weight). Accepts a
`TextStyleRole`, a `TextStyle`, or a `Signal` of either. Default is
`TextStyleRole::Body`.

#### `pub fn subtitle_style( mut self, style: impl Into<teksilo_core::color_prop::TextStyleProp>, ) -> Self`

Override the subtitle's text style. Default is `TextStyleRole::Small`.

#### `pub fn label_color(mut self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Override the label's text color. Accepts `Color`, a role, or a
`Signal` of either. Default (unset) is enabled-derived
(`Primary` / `Disabled`); setting this replaces that cascade.

#### `pub fn subtitle_color(mut self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Override the subtitle's text color. Default (unset) is
`TextRole::Secondary`.

#### `pub fn interaction_signal(mut self, signal: Signal<InteractionState>) -> Self`

Truncate the primary label instead of wrapping it. Default (unset) is
`TextOverflow::Wrap`.

A wrapping label reports its full intrinsic width, so on a row too
narrow to hold it the primary `HStack` is over-constrained and the
`trailing_slot` is pushed past the row's edge.
Set `TextOverflow::Ellipsis(..)` on rows whose trailing actions must
stay reachable: the label then shrinks and truncates within the row.
**Share the row's interaction state**, so a caller can reveal controls on
hover.

A row that shows its actions only while the pointer is over it is a standard
pattern — a search result offering *replace* and *dismiss*, a list offering
*remove* — and it cannot be built from outside without knowing when the row
is hovered. The row already tracks that; this is the handle on it.

The signal is written by the row, not read: pass one in, watch it, and gate
a trailing slot on it. Reserve the space the controls will take, or the row
reflows under the pointer that is trying to hit them.

#### `pub fn label_slot(mut self, widget: impl Widget + 'static) -> Self`

**Draw this instead of the label's text**, keeping the label as the row's
accessible name.

For a row whose label is not plain text: a search result with the matched
run picked out of its excerpt, a diff line, anything built from runs rather
than from a string. The label passed to `new` is still what
`accessibility` reports, so the row keeps a name a screen reader can read —
which is the whole reason this is a *replacement for the drawing* and not a
replacement for the label.

The widget is laid out where the text would have been, so it inherits the
row's spacing and its place beside the leading and trailing slots.
`label_style`, `label_color` and
`label_overflow` do not reach it: it draws itself.

#### `pub fn label_overflow(mut self, overflow: TextOverflow) -> Self`

#### `pub fn subtitle_overflow(mut self, overflow: TextOverflow) -> Self`

Truncate the subtitle instead of wrapping it. Default (unset) is
`TextOverflow::Wrap`.

Same rationale as `label_overflow` — and the
usual culprit, since subtitles carry long secondary text (file paths,
URLs). `TextOverflow::Ellipsis(EllipsisMode::Middle)` suits a path: it
keeps both the root and the file name legible.

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Attach a plain tooltip shown after the standard hover delay.

Mutually exclusive with `rich_tooltip`,
`rich_tooltip_content`, and
`composite_tooltip` — the last setter called
wins and clears the other slots.

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Attach a rich tooltip looked up from the global tooltip registry by key.

Mutually exclusive with `tooltip`,
`rich_tooltip_content`, and
`composite_tooltip` — the last setter called
wins and clears the other slots.

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Attach a rich tooltip from an inline `TooltipContent`
value (no registry lookup required).

Mutually exclusive with `tooltip`,
`rich_tooltip`, and
`composite_tooltip` — the last setter called
wins and clears the other slots.

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Attach a composite tooltip whose body is an arbitrary widget tree.

Mutually exclusive with `tooltip`,
`rich_tooltip`, and
`rich_tooltip_content` — the last setter
called wins and clears the other slots.

## `pub struct StandardTreeItem`

Canonical row layout for a `TreeView` — `StandardListItem` plus
a depth-driven indent column and an always-reserved chevron column.

See the `module-level documentation` for the canonical `TreeView`
wiring pattern and wiring rules.

```rust
pub struct StandardTreeItem { /* fields */ }
```

### Methods

#### `pub fn new(label: impl Into<LocalizedString>) -> Self`

Create a tree item with the given primary label.

#### `pub fn interaction_signal(mut self, signal: Signal<InteractionState>) -> Self`

Forwarded to the inner `StandardListItem` — see its
`subtitle`.
See [`StandardListItem::interaction_signal`]: the row's own hover/press
state, for a caller revealing controls on hover.

#### `pub fn label_slot(mut self, widget: impl Widget + 'static) -> Self`

See [`StandardListItem::label_slot`]: draw this instead of the label's
text, keeping the label as the row's accessible name.

#### `pub fn subtitle(mut self, text: impl Into<LocalizedString>) -> Self`

#### `pub fn leading_slot(mut self, widget: impl Widget + 'static) -> Self`

Forwarded to the inner `StandardListItem` — see its
`leading_slot`.

#### `pub fn leading_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self`

`Box<dyn Widget>` variant of `leading_slot`.

#### `pub fn center_slot(mut self, widget: impl Widget + 'static) -> Self`

Forwarded to the inner `StandardListItem` — see its
`center_slot`.

#### `pub fn center_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self`

`Box<dyn Widget>` variant of `center_slot`.

#### `pub fn trailing_slot(mut self, widget: impl Widget + 'static) -> Self`

Forwarded to the inner `StandardListItem` — see its
`trailing_slot`.

#### `pub fn trailing_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self`

`Box<dyn Widget>` variant of `trailing_slot`.

#### `pub fn subtitle_leading_slot(mut self, widget: impl Widget + 'static) -> Self`

Forwarded to the inner `StandardListItem` — see its
`subtitle_leading_slot`.

#### `pub fn subtitle_leading_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self`

`Box<dyn Widget>` variant of
`subtitle_leading_slot`.

#### `pub fn subtitle_trailing_slot(mut self, widget: impl Widget + 'static) -> Self`

Forwarded to the inner `StandardListItem` — see its
`subtitle_trailing_slot`.

#### `pub fn subtitle_trailing_slot_boxed(mut self, widget: Box<dyn Widget>) -> Self`

`Box<dyn Widget>` variant of
`subtitle_trailing_slot`.

#### `pub fn checkbox(mut self, checked: Signal<bool>) -> Self`

Forwarded to the inner `StandardListItem` — see its
`checkbox`.

#### `pub fn tristate_checkbox(mut self, state: Signal<CheckState>) -> Self`

Forwarded to the inner `StandardListItem` — see its
`tristate_checkbox`.

#### `pub fn selected(mut self, selected: impl Into<Prop<bool>>) -> Self`

Set the selection state, statically or reactively via a bound
`Signal<bool>`. Forwarded to the inner `StandardListItem` — see
its `selected`.

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Set the enabled state, statically or reactively via a bound
`Signal<bool>` / `Prop<bool>`. Forwarded to the inner
`StandardListItem`.

#### `pub fn label_style( mut self, style: impl Into<teksilo_core::color_prop::TextStyleProp>, ) -> Self`

Override the label's text style. Forwarded to the inner
`StandardListItem` — see its
`label_style`.

#### `pub fn subtitle_style( mut self, style: impl Into<teksilo_core::color_prop::TextStyleProp>, ) -> Self`

Override the subtitle's text style. Forwarded to the inner
`StandardListItem` — see its
`subtitle_style`.

#### `pub fn label_color(mut self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Override the label's text color. Forwarded to the inner
`StandardListItem` — see its `label_color(...)`.

#### `pub fn subtitle_color(mut self, color: impl Into<teksilo_core::color_prop::ColorProp>) -> Self`

Override the subtitle's text color. Forwarded to the inner
`StandardListItem` — see its `subtitle_color(...)`.

#### `pub fn label_overflow(mut self, overflow: TextOverflow) -> Self`

Truncate the primary label instead of wrapping it. Forwarded to the
inner `StandardListItem` — see its
`label_overflow`.

#### `pub fn subtitle_overflow(mut self, overflow: TextOverflow) -> Self`

Truncate the subtitle instead of wrapping it. Forwarded to the inner
`StandardListItem` — see its
`subtitle_overflow`.

#### `pub fn style(mut self, style: impl teksilo_core::styles::StandardItemStyle) -> Self`

Per-call style override for the row chrome. Forwarded to the
inner `StandardListItem` — see its `style(...)` for the
precedence rules (per-call > theme.style_slots.standard_item >
`RecipeStandardItemStyle`).

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Attach a plain tooltip shown after the standard hover delay.
Forwarded to the inner `StandardListItem` — see its
`tooltip`.

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Attach a rich tooltip looked up from the global tooltip registry by key.
Forwarded to the inner `StandardListItem` — see its
`rich_tooltip`.

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Attach a rich tooltip from an inline
`TooltipContent` value.
Forwarded to the inner `StandardListItem` — see its
`rich_tooltip_content`.

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Attach a composite tooltip whose body is an arbitrary widget tree.
Forwarded to the inner `StandardListItem` — see its
`composite_tooltip`.

#### `pub fn depth(mut self, depth: usize) -> Self`

Set the indent depth (0 = root level). Each level adds one
`STANDARD_ITEM_TREE_INDENT_STEP` of leading whitespace.

#### `pub fn has_children(mut self, has: bool) -> Self`

Declare whether the node has children, which determines whether the
chevron column is interactive or decorative-only.

#### `pub fn is_expanded(mut self, expanded: impl Into<Prop<bool>>) -> Self`

Set the expanded state, statically or reactively via a bound
`Signal<bool>`.

#### `pub fn from_entry(self, entry: &FlatEntry) -> Self`

Convenience for the TreeView delegate path:
`.from_entry(entry)` sets depth + has_children + is_expanded.

#### `pub fn on_toggle( mut self, f: impl Fn(&mut teksilo_core::widget::EventContext) + 'static, ) -> Self`

Click handler for the chevron. Wired only when `has_children`
is true. Typical use: `.on_toggle(ctx.toggle_callback())` from
a `TreeRowContext` (see `TreeView::new_with_context`).

The callback receives the firing `EventContext` so apps can
dispatch an intent (e.g. lazy-load children on expand), open
a dialog, or otherwise route the toggle through the framework
before mutating model state.

#### `pub fn on_toggle_rc(mut self, f: Rc<dyn Fn(&mut teksilo_core::widget::EventContext)>) -> Self`

Variant accepting an already-`Rc`'d callback. Useful when the
same callback is shared across multiple call sites without an
extra clone — e.g. `TreeRowContext::toggle_callback()` returns
this shape directly.
