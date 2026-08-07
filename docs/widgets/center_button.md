<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# NotificationCenterButton

`NotificationCenterButton` — bell icon with an unread-count badge that
opens a `NotificationLog` popover when clicked.

Composed as a `ZStack { PopoverIconButton(bell), Badge }`. The badge
shows the current unread count and is hit-transparent so clicks always
reach the bell beneath. On popover close the archive's `mark_all_read`
is called and the badge resets — matching the GitHub / Slack / JetBrains
convention. Most apps mount this in a `StatusBar` or `TitleBar` trailing
slot; all popover behaviour is self-managed with no further wiring.

## Accessibility

The inner `IconButton` carries the bell `Role::Button` label; the outer
container is `set_hidden` (presentational). The badge count is not
separately announced — the button label and badge label together convey
the state to sighted users; AT users interact through the button itself.

```ignore
// Typical setup — archive comes from install_toast_default():
let archive: Rc<NotificationArchiveModel> = ctx.app_state().unwrap();
let bell = NotificationCenterButton::new(archive)
    .on_action_invoked(|_entry, action, ctx| {
        if let Some(name) = &action.intent_name {
            ctx.send_intent(teksilo_core::Intent::new(name));
        }
    });
```

## Builder methods at a glance

`size`, `show_badge_when_zero`, `max_badge_count`, `placement`, `on_action_invoked`, `tooltip`, `rich_tooltip`, `rich_tooltip_content`, `composite_tooltip`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/notification/center_button/index.html)

## `pub struct NotificationCenterButton`

Bell-icon trigger + unread-count badge + popover that contains a
`NotificationLog`. On popover open the archive's `mark_all_read`
runs (the user is presumed to have seen the toasts now).

```rust
pub struct NotificationCenterButton { /* fields */ }
```

### Methods

#### `pub fn new(archive: Rc<NotificationArchiveModel>) -> Self`

Construct bound to a shared archive. The archive is typically
held in `app_state` and cloned to every consumer.

#### `pub fn size(mut self, size: IconButtonSize) -> Self`

Bell-icon size. Default `IconButtonSize::Toolbar` (30 dp) —
matches the JetBrains status-bar density.

#### `pub fn show_badge_when_zero(mut self, show: bool) -> Self`

Whether to keep the badge visible when the unread count is
zero. Default `false` (badge hidden when no unread). Apps
that want a persistent "0" indicator pass `true`.

#### `pub fn max_badge_count(mut self, max: u32) -> Self`

Cap the displayed badge count. Default `99` — counts above
the cap display as `"99+"`. Set to `u32::MAX` to disable the
cap.

#### `pub fn placement(mut self, p: OverlayPlacement) -> Self`

Popover placement relative to the bell. Default
`BelowPreferred` — flips above when the button is near the
viewport bottom edge.

#### `pub fn on_action_invoked( mut self, f: impl Fn(&NotificationEntry, &ArchivedAction, &mut EventContext) + 'static, ) -> Self`

Threaded into the embedded `NotificationLog` —
see `NotificationLog::on_action_invoked` for the contract.
Wire this to dispatch archived actions; without it the
action buttons in the log are inert.

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Attach a plain single-line tooltip shown after a hover delay.

Mutually exclusive with `rich_tooltip`,
`rich_tooltip_content`, and
`composite_tooltip` — the last setter
called wins.

#### `pub fn rich_tooltip(mut self, key: impl Into<String>) -> Self`

Attach a rich tooltip identified by a registry key.

Mutually exclusive with `tooltip`,
`rich_tooltip_content`, and
`composite_tooltip`.

#### `pub fn rich_tooltip_content(mut self, content: crate::tooltip::TooltipContent) -> Self`

Attach a rich tooltip from inline `crate::tooltip::TooltipContent`.

Mutually exclusive with `tooltip`,
`rich_tooltip`, and
`composite_tooltip`.

#### `pub fn composite_tooltip(mut self, content: impl Widget + 'static) -> Self`

Attach a composite tooltip containing an arbitrary widget tree.

Mutually exclusive with `tooltip`,
`rich_tooltip`, and
`rich_tooltip_content`.
