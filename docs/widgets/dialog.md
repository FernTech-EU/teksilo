<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Dialog

Modal dialogs — a trigger button that presents a centered modal panel.

Three cooperating types cover the common dialog use-case. `Dialog` is the
high-level entry point: a `Button` (or custom trigger) that, on activation,
presents a `ModalContainer` above a full-viewport dimming `ModalScrim`.
`DialogContent` is the convenience body layout — a `VStack` with an
optional title, supporting text, scrollable body slot, and a footer slot
separated by a `Divider`.

## When to use

- `Dialog::new(label).content(|| …)` for the common "button opens dialog" pattern.
- `Dialog::new(label).trigger(my_icon_button).content(|| …)` to use a custom widget
  as the trigger instead of the default `Button`.
- `ModalContainer::new(content)` directly when you need to present a modal from
  handler code via `ctx.present_modal(ModalRequest::…)` rather than a persistent
  trigger.

## Accessibility

`ModalContainer` is a `Role::Dialog` node and announces `set_modal()`.
Its accessible name defaults to the `DialogContent` title (via
`Widget::accessible_title_hint`) or falls back to the localized
`a11y_dialog_name` message; pass `.title(tr!(…))` to the container for an
explicit override. The trigger button advertises `HasPopup::Dialog` and
`set_expanded` tracks whether the modal is currently open.

```ignore
use bastyde_widgets::dialog::{Dialog, DialogContent};
use bastyde_i18n::lit;

let _d = Dialog::new(lit!("Open settings"))
    .content(|| {
        DialogContent::new()
            .title(lit!("Settings"))
            .supporting_text(lit!("Adjust your preferences below."))
    });
```

## Builder methods at a glance

`content`, `variant`, `enabled`, `presentation`, `close_behavior`, `trigger`, `trigger_id`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/dialog/index.html)

## `pub struct ModalContainer`

Rounded panel chrome that wraps a modal dialog's content widget.

All visual dimensions (padding, corner radius, min-width, shadow) are owned
by the active `DialogStyle`; per-instance
overrides are available via `Self::padding` and `Self::min_width`.

```rust
pub struct ModalContainer { /* fields */ }
```

### Methods

#### `pub fn new(content: impl Widget + 'static) -> Self`

Wrap `content` inside a modal panel with default chrome.

#### `pub fn padding(mut self, padding: f32) -> Self`

Override the content padding (logical pixels) from the theme default.

#### `pub fn min_width(mut self, min_width: f32) -> Self`

Override the minimum panel width (logical pixels) from the theme default.

#### `pub fn style(mut self, style: impl bastyde_core::styles::DialogStyle) -> Self`

Per-call style override for the modal panel chrome. Replaces the
theme-wide default `DialogStyle` for just this container.

#### `pub fn title(mut self, title: impl Into<LocalizedString>) -> Self`

Accessible title for the dialog. Screen readers announce this
as the dialog's name. Should match the inner `DialogContent`'s
visible title string.

## `pub struct ModalScrim`

Full-viewport dimming scrim painted behind a `ModalContainer`.

Mounted by the modal-presentation pipeline (bastyde-app) as a separate
`OverlayPlacement::FullViewport` overlay pushed BEFORE the centered
modal overlay so it z-orders below the panel. The chrome itself is
delegated to the active `DialogStyle::make_scrim`; clicking the
scrim dismisses the linked modal when the modal's
`ModalCloseBehavior` permits click-outside dismissal.

The dismissal cascade is wired via
`OverlayManager::set_parent_overlay` AFTER both overlays are
pushed — the scrim's `parent_overlay` is set to the modal's id, so
any dismiss of the modal cascades through `dismiss_immediate` and
also dismisses the scrim. The scrim's own `dismiss` behavior is
`Manual` — it never dismisses itself directly.

```rust
pub struct ModalScrim { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Build a new scrim; wire it with `Self::dismiss_target` and
`Self::click_to_dismiss` after construction.

#### `pub fn style(mut self, style: impl bastyde_core::styles::DialogStyle) -> Self`

Per-call style override for the scrim chrome. Replaces the
theme-wide default `DialogStyle` for just this scrim.

#### `pub fn dismiss_target(mut self, target: Rc<Cell<Option<OverlayId>>>) -> Self`

Handle to the modal-overlay id the scrim dismisses on click.
The framework fills this AFTER the modal is pushed (see the
in-tree modal pipeline in `bastyde-app`).

#### `pub fn click_to_dismiss(mut self, enabled: bool) -> Self`

Enable click-to-dismiss on the scrim. Should mirror whether the
modal's `ModalCloseBehavior` permits click-outside dismissal.

## `pub struct DialogContent`

Convenience body layout for a modal dialog: optional title, supporting text,
scrollable body slot, and a `Divider`-separated footer row.

```rust
pub struct DialogContent { /* fields */ }
```

### Methods

#### `pub fn new() -> Self`

Create an empty dialog body with no sections set.

#### `pub fn title(mut self, title: impl Into<LocalizedString>) -> Self`

Bold title shown at the top of the content area. Also propagated to
the enclosing `ModalContainer` via `accessible_title_hint`.

#### `pub fn supporting_text(mut self, text: impl Into<LocalizedString>) -> Self`

Secondary description text shown below the title.

#### `pub fn body(mut self, body: impl Widget + 'static) -> Self`

Main scrollable content slot (any widget).

#### `pub fn body_id(mut self, id: WidgetId) -> Self`

Main content slot by pre-registered `WidgetId`.

#### `pub fn footer(mut self, footer: impl Widget + 'static) -> Self`

Footer slot separated from the body by a `Divider` (typically action
buttons like "OK" / "Cancel").

#### `pub fn footer_id(mut self, id: WidgetId) -> Self`

Footer slot by pre-registered `WidgetId`.

## `pub struct Dialog`

A trigger button that presents a modal dialog when activated.

Renders as a `Button` by default; call `.trigger(w)` to replace it with any
widget. The content is lazily constructed by a factory closure each time the
dialog opens — no persistent widget subtree is kept while the dialog is closed.

```rust
pub struct Dialog { /* fields */ }
```

### Methods

#### `pub fn new(label: impl Into<LocalizedString>) -> Self`

Build a dialog trigger with `label` as the button text and accessible name.

#### `pub fn content<W, F>(mut self, factory: F) -> Self where W: Widget + 'static, F: Fn() -> W + 'static,`

Factory closure that builds the dialog's content each time it opens.
Required — the dialog panics at build time if no factory is set.

#### `pub fn variant(mut self, variant: ButtonVariant) -> Self`

Visual style of the default trigger button. Has no effect when
`.trigger(…)` replaces the button with a custom widget.

#### `pub fn enabled(mut self, enabled: impl Into<Prop<bool>>) -> Self`

Enable or disable the trigger button, statically or reactively
(default `true`).

#### `pub fn presentation(mut self, presentation: ModalPresentation) -> Self`

Override the modal presentation mode (default `ModalPresentation::Auto`).

#### `pub fn close_behavior(mut self, close_behavior: ModalCloseBehavior) -> Self`

Override how the dialog may be closed (default `EscapeOrClickOutside`).

#### `pub fn trigger(mut self, trigger: impl Widget + 'static) -> Self`

Replace the default `Button` trigger with a custom widget. The widget
receives the same tap / key / AT-action handlers as the button would.

#### `pub fn trigger_id(mut self, id: WidgetId) -> Self`

Custom trigger by pre-registered `WidgetId`.
