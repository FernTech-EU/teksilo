<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Snackbar

Snackbar — a transient, button-triggered floating notification surface.

A `Snackbar` pairs a trigger (a `Button` by default, or any custom
widget via `.trigger(...)`) with a dormant content surface. Activating
the trigger presents the surface as an `OverlayPlacement::BottomCenter`
overlay and dismisses it automatically after a configurable timeout
(default: 4 s). The surface stays until dismissed when `.persistent()`
is set. Only one snackbar can be shown at a time — presenting a second
one dismisses the first.

For richer, stackable, severity-aware notifications see the
`Toast` system, which also maintains a
persistent `NotificationArchiveModel`.

## Accessibility

The content surface exposes `Role::Alert` with `Live::Polite` so
screen readers announce the notification without interrupting the user.
Supply `.announcement(...)` to give the alert a descriptive name
instead of the generic "notification" fallback.

```ignore
use bastyde_widgets::{Snackbar};
use bastyde_i18n::lit;
use bastyde_widgets::primitives::TextWidget;
use bastyde_tokens::TextRole;

// In build():
ctx.add(
    Snackbar::new(lit!("Undo"))
        .content(TextWidget::new(lit!("File deleted.")).color(TextRole::TooltipText))
        .announcement(lit!("File deleted."))
        .auto_dismiss_after(std::time::Duration::from_secs(5)),
);
```

## Builder methods at a glance

`style`, `content`, `content_id`, `variant`, `enabled`, `dismiss_behavior`, `auto_dismiss_after`, `persistent`, `trigger`, `trigger_id`, `announcement`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/snackbar/index.html)

## `pub struct Snackbar`

A button-triggered transient notification surface.

Call `.content(...)` to supply the notification body, then add the
widget to the tree. The trigger label is shown as a `Button` (or a
custom widget via `.trigger(...)`); activating it presents the
content surface at the bottom center of the window.

```rust
pub struct Snackbar { /* fields */ }
```

### Methods

#### `pub fn new(label: impl Into<LocalizedString>) -> Self`

Create a snackbar whose default trigger button shows `label`.

#### `pub fn style(mut self, style: impl bastyde_core::styles::SnackbarStyle) -> Self`

Per-call style override for the snackbar surface chrome.
Replaces the theme-wide default `SnackbarStyle` for just this
instance.

#### `pub fn content(mut self, content: impl Widget + 'static) -> Self`

The snackbar body — the message (and optional inline action)
shown on the floating surface.

The default surface is the high-contrast (dark) `tooltip_bg`,
the same one tooltips use, and it stays dark in light theme.
So any `TextWidget` you pass here must set
`.color(TextRole::TooltipText)` (and actions can use
`TooltipText` / `TooltipShortcut`) — the default `TextRole::Primary`
is dark and renders nearly invisible on the dark surface in light
theme. If you install a light-surface `SnackbarStyle`, color the
content to match that instead.

#### `pub fn content_id(mut self, id: WidgetId) -> Self`

Supply the notification body by `WidgetId` (already added to the
tree). Mutually exclusive with `.content(...)`.

#### `pub fn variant(mut self, variant: ButtonVariant) -> Self`

Override the default trigger [`ButtonVariant`] (default: `Plain`).

#### `pub fn enabled(mut self, enabled: bool) -> Self`

Set the initial enabled state of the trigger.

#### `pub fn dismiss_behavior(mut self, dismiss: DismissBehavior) -> Self`

Override the overlay dismiss behavior (default: `ClickOutside`).

#### `pub fn auto_dismiss_after(mut self, duration: Duration) -> Self`

Set the auto-dismiss timeout. The overlay is removed after this
duration without user interaction (default: 4 s).

#### `pub fn persistent(mut self) -> Self`

Keep the snackbar visible until explicitly dismissed; disables
the auto-dismiss timeout.

#### `pub fn trigger(mut self, trigger: impl Widget + 'static) -> Self`

Replace the default `Button` trigger with a custom widget. The
widget is wired for tap, keyboard (Enter/Space), and AT Click
activation automatically.

#### `pub fn trigger_id(mut self, id: WidgetId) -> Self`

Supply the custom trigger by `WidgetId` (already added to the tree).

#### `pub fn announcement(mut self, text: impl Into<LocalizedString>) -> Self`

Screen-reader announcement string — used as the Alert's
accessible name when the snackbar appears. Without this
the surface falls back to the generic `a11y_snackbar_name`
i18n string, which says "notification" but can't describe
the specific message. Set this whenever the snackbar
conveys information the user needs to hear (errors,
confirmations, status changes).
