<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# NotificationLog

`NotificationLog` — a scrollable, day-bucketed list of archived notifications.

Renders a [`NotificationArchiveModel`] as a scrollable column of
[`StandardListItem`] rows grouped under section headers (Today /
Yesterday / This week / Earlier), computed against the user's local
timezone on every archive mutation. An optional toolbar row provides
mark-all-read and clear buttons. Unread rows show the title in
`BodyBold`; read rows use `Body`. An empty-state hint is shown when the
archive is empty.

## When to use

- Embed directly inside a side panel or settings page for an in-app
  notification centre.
- Wrap in `NotificationCenterButton`
  for the standard bell-icon-with-popover pattern.
- Call `NotificationLogDialog::show`
  for a one-line modal presentation.

```ignore
let archive: Rc<NotificationArchiveModel> = ctx.app_state().unwrap();
let log = NotificationLog::new(archive)
    .on_action_invoked(|_entry, action, ctx| {
        if let Some(name) = &action.intent_name {
            ctx.send_intent(bastyde_core::Intent::new(name));
        }
    });
```

## Builder methods at a glance

`show_toolbar`, `empty_state`, `on_entry_invoked`, `on_action_invoked`

## API reference

📖 [Full rustdoc API for this module](../api/bastyde_widgets/notification/index.html)

## `pub struct NotificationLog`

Configurable archive log. Shipped chrome:
- mark-all-read + clear buttons in a toolbar row;
- empty-state hint when the archive is empty;
- day-bucket section headers (Today / Yesterday / This week /
  Earlier) above the rows for each bucket — computed against the
  user's local timezone, recomputed on every archive mutation;
- [`StandardListItem`] rows with unread-as-bold differentiation.

A SearchField filter and a severity-chip filter can be composed by
apps using the existing widget toolkit.

```rust
pub struct NotificationLog { /* fields */ }
```

### Methods

#### `pub fn new(archive: Rc<NotificationArchiveModel>) -> Self`

Construct a log bound to the shared archive. The archive is
expected to outlive the log (typically held in `app_state`).

#### `pub fn show_toolbar(mut self, show: bool) -> Self`

Whether to render the toolbar row (mark-all-read + clear).
Default `true`. Apps that want a chrome-less log (e.g. inside
a custom panel that supplies its own toolbar) pass `false`.

#### `pub fn empty_state(mut self, widget: impl Widget + 'static) -> Self`

Override the empty-state hint widget. Default: a centered
"No notifications" text. Pass any widget for a custom empty
view (illustration, call-to-action, …).

#### `pub fn on_entry_invoked( mut self, f: impl Fn(&NotificationEntry, &mut EventContext) + 'static, ) -> Self`

Called when the user clicks anywhere on an archived entry's
row body (outside any specific action button). The default
behaviour is no-op — the log is read-only display unless
callers wire this hook.

#### `pub fn on_action_invoked( mut self, f: impl Fn(&NotificationEntry, &ArchivedAction, &mut EventContext) + 'static, ) -> Self`

Called when an archived action button is clicked. Apps wire
this hook to replay the action — typically by mapping the
`ArchivedAction::intent_name` to one of the app's registered
`Action`s via `ctx.send_intent(...)`. Without this hook
configured the action buttons are inert (the log keeps them
visible for archival context).

Actions without an `intent_name` render as non-clickable
past-action tags regardless of this hook — there's nothing
for the framework to dispatch against once the live closure
has torn down.

```ignore
log.on_action_invoked(|_entry, action, ctx| {
    // Bridge the dynamic intent_name to one of the app's
    // typed AppIntent variants:
    match action.intent_name.as_deref() {
        Some("app.build.retry") => ctx.send_intent(AppIntent::BuildRetry),
        Some(name) => log::warn!("unknown archived intent: {name}"),
        None => {}
    }
})
```
