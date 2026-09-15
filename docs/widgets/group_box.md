<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# GroupBox

![GroupBox preview](img/group_box.png)

GroupBox — titled cluster of controls in Int UI / Jewel style.

A bold title (optionally preceded by a checkbox) sits above an indented
content area. No border, no frame — pure composition. The standard use
is grouping related settings controls on a preferences sheet or
form — the IntelliJ "group" pattern.

In checkable mode, unchecking disables event dispatch to every descendant
of the content area (via `ctx.enabled_when` with ancestor propagation) AND
paints a translucent surface overlay over the content so it reads as
greyed-out. The title checkbox itself stays interactive.

## When to use

- **GroupBox** — logical cluster with a title; optional enable/disable
  toggle for the whole cluster. Use for settings sections.
- `GroupHeader` — lighter-weight "soft divider +
  caption" without a content slot; use to label regions that are not
  collapsed or disabled as a unit.

## Accessibility

The box node carries `Role::Group` and its `name` is set to the title
string. When checkable and unchecked, `set_disabled()` is set on the
group node so assistive technology announces the cluster as unavailable.

```rust
# use teksilo_widgets::GroupBox;
# use teksilo_widgets::primitives::TextWidget;
# use teksilo_i18n::lit;
let _w = GroupBox::new(lit!("Indentation"))
    .child(TextWidget::new(lit!("Tab width: 4")));
```

## Density

The picture above is the widget at `TargetDensity::Compact`, the mouse-and-keyboard ladder. Below is the same subject on the same canvas with only the ladder changed, so what moves is the density and nothing else — where the subject no longer fits, that is what the denser targets cost it at that size. See `docs/density-and-targets.md`.

**Touch**

![GroupBox at Touch density](img/group_box-touch.png)

## Builder methods at a glance

`checkable`, `child`, `child_opt`, `child_id`

## API reference

📖 [Full rustdoc API for this module](https://docs.rs/teksilo-widgets/latest/teksilo_widgets/group_box/index.html)

## `pub const GROUP_BOX_CONTENT_INDENT`

Horizontal indent of the content area below the title (dp).

```rust
pub const GROUP_BOX_CONTENT_INDENT: f32 = 24.0;
```

## `pub fn group_box_content_indent(...)`

`GROUP_BOX_CONTENT_INDENT` scaled by the density's `spacing_factor`
(1.00 / 1.15 / 1.30).

```rust
pub fn group_box_content_indent(tokens: &InputTokens) -> f32;
```

## `pub const GROUP_BOX_TITLE_CONTENT_SPACING`

Vertical gap between the title row and the content area (dp).

```rust
pub const GROUP_BOX_TITLE_CONTENT_SPACING: f32 = 8.0;
```

## `pub fn group_box_title_content_spacing(...)`

`GROUP_BOX_TITLE_CONTENT_SPACING` scaled by the density's `spacing_factor`
(1.00 / 1.15 / 1.30).

```rust
pub fn group_box_title_content_spacing(tokens: &InputTokens) -> f32;
```

## `pub const GROUP_BOX_CHECKBOX_GAP`

Gap between the checkbox and the adjacent title label in checkable mode (dp).

```rust
pub const GROUP_BOX_CHECKBOX_GAP: f32 = 6.0;
```

## `pub fn group_box_checkbox_gap(...)`

`GROUP_BOX_CHECKBOX_GAP` scaled by the density's `spacing_factor`
(1.00 / 1.15 / 1.30).

```rust
pub fn group_box_checkbox_gap(tokens: &InputTokens) -> f32;
```

## `pub struct GroupBox`

A titled cluster of controls with optional enable/disable toggle.

See the `module documentation` for the checkable-mode details and
the `GroupHeader` sibling.

```rust
pub struct GroupBox { /* fields */ }
```

### Methods

#### `pub fn new(title: impl Into<LocalizedString>) -> Self`

Create a non-checkable group box with the given `title`.

#### `pub fn checkable(mut self, checked: Signal<bool>) -> Self`

Turn this into a checkable GroupBox. When the signal is `false`, events
to descendants of the content area are blocked via effective-enabled
ancestor propagation. The title checkbox itself stays interactive.

#### `pub fn child(mut self, widget: impl Widget + 'static) -> Self`

Set the content widget inline (deferred insertion).

#### `pub fn child_opt(self, widget: Option<impl Widget + 'static>) -> Self`

Attach `widget` when it is `Some`, and do nothing when it is `None`.

The conditional-child form. `teksu!`'s `if` without an `else` lowers to
this, and it is what `cond.then(|| w)` is for in a builder chain. `None`
adds no arena node, so nothing is laid out, painted, or published to the
accessibility tree, and a stack applies no spacing around it.

#### `pub fn child_id(mut self, id: WidgetId) -> Self`

Set the content widget by pre-registered ID.
