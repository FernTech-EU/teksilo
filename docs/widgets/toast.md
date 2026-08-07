<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Toast

Toast notification — stackable, action-rich, severity-aware floating
notification (the "upgrade path" from `Snackbar`).

Distinct from siblings:
- `Snackbar` — single-instance, message-only.
  Calling `present_snackbar` dismisses all other overlays first.
- `Banner` — persistent inline strip, not a floating
  overlay.
- `MessageBox` — modal dialog. Blocks
  interaction with the rest of the UI.

A `Toast` is built with one of the four severity constructors
(`info` / `success` / `warning` / `error`) plus a `loading` variant,
configured via builder methods, and presented with
`ctx.show_toast(toast)` (see
`toast::ext::EventContextToastExt`)
or `toast.present(ctx)`. A `ToastHost`
installed via `TeksiloAppBuilder.install_toast(opts)` from the `teksilo`
umbrella accepts the request, picks a free slot from its pool, and
mounts a `ToastSurface` at the
configured viewport corner using the
`OverlayPlacement::ViewportCorner`
variant.

```ignore
ctx.show_toast(
    Toast::warning(tr!(unsaved_changes()))
        .body(tr!(close_anyway_question()))
        .action(ToastAction::primary(tr!(save()), |c| c.send_intent(AppIntent::Save)))
        .action(ToastAction::new(tr!(discard()), |c| c.send_intent(AppIntent::Discard)))
);
```

## Builder methods at a glance

`info`, `success`, `warning`, `error`, `loading`, `body`, `leading`, `action`, `primary_action`, `auto_dismiss_after`, `persistent`, `priority`, `id`, `on_click`, `on_dismiss`, `show_close_button`, `closable_on_escape`, `announcement`, `archive`, `style`, `present`

## API reference

📖 [Full rustdoc API for this module](../api/teksilo_widgets/toast/index.html)

## `pub const DEFAULT_TOAST_AUTO_DISMISS`

Default auto-dismiss duration when the caller does not override
it (matches IntelliJ `BALLOON` and Material Snackbar maximum).

```rust
pub const DEFAULT_TOAST_AUTO_DISMISS: Duration = Duration::from_secs(10);
```

## `pub enum ToastDismissCause`

Why a toast was dismissed — delivered to the `on_dismiss` callback.

```rust
pub enum ToastDismissCause { /* variants */ }
```

### Variants

- **`Timeout`** — `auto_dismiss_after` reached zero (timer expired naturally).
- **`ActionInvoked`** — A `ToastAction` with `closes_toast(true)` (the default) fired.
- **`CloseClicked`** — The user clicked the close (X) button.
- **`EscapePressed`** — The user pressed Escape while focus was inside the toast.
- **`Programmatic`** — `ToastHandle::dismiss` was called from app code.
- **`HostShutdown`** — The host's window is being torn down.
- **`SlotPoolFull`** — The host's slot pool was at `max_visible` and this toast was dropped (Normal priority overflow) or was evicted by a higher-priority arrival. Reported synthetically so `on_dismiss` always fires once per toast — apps that track outstanding toasts via the callback don't leak.

## `pub enum ToastActionStyle`

How a `ToastAction` should be rendered inside the toast surface.

```rust
pub enum ToastActionStyle { /* variants */ }
```

### Variants

- **`Link`** — JetBrains-style hyperlink. Rendered inline with the body row. Default — minimal visual weight, scales to many actions.
- **`Button`** — Material / Windows-style button. Rendered in a dedicated row below the body. Use for primary calls-to-action ("Retry", "Save", "Discard").

## `pub type ToastActionCallback`

Type-erased callback for a `ToastAction`. `Fn` (not `FnMut`) so
the same callback can be wrapped in an `Rc` and dispatched from
multiple paths (tap, keyboard, AT custom action).

```rust
pub type ToastActionCallback = Rc<dyn Fn(&mut EventContext)>;
```

## `pub struct ToastAction`

One actionable element inside a `Toast` — a button or hyperlink
the user can click to drive a domain action.

```rust
pub struct ToastAction { /* fields */ }
```

### Methods

#### `pub fn new( label: impl Into<LocalizedString>, on_invoke: impl Fn(&mut EventContext) + 'static, ) -> Self`

Build an action with the default `Link` style and
`closes_toast = true` (IntelliJ "expiring action" semantics).

#### `pub fn primary( label: impl Into<LocalizedString>, on_invoke: impl Fn(&mut EventContext) + 'static, ) -> Self`

Shorthand for `ToastAction::new(label, on_invoke).style(Button { Filled })`.
The visual-weight default for primary calls-to-action.

#### `pub fn destructive( label: impl Into<LocalizedString>, on_invoke: impl Fn(&mut EventContext) + 'static, ) -> Self`

Shorthand for the destructive button variant — red-tinted for
confirm-style "Delete" / "Discard" actions.

#### `pub fn style(mut self, style: ToastActionStyle) -> Self`

Override the action's visual style. Default is `Link`.

#### `pub fn closes_toast(mut self, closes: bool) -> Self`

Whether invoking this action also dismisses the toast. Default
is `true` — matches IntelliJ's "expiring action" semantics.
Set to `false` for actions that toggle state without closing
(e.g. "Show details" disclosure inside a sticky toast).

#### `pub fn shortcut_id(mut self, id: impl Into<String>) -> Self`

Associate the action with a registered `Shortcut` id. Two
effects: the keystroke label is shown as a chip on the action,
and the archived form of this action (in
`NotificationLog`)
is re-invokable by name through the existing Intent
dispatcher.

#### `pub fn tooltip(mut self, text: impl Into<LocalizedString>) -> Self`

Optional tooltip text shown when the pointer hovers the action.

#### `pub fn label(&self) -> String`

Resolve the action label to a plain string using the current locale.

#### `pub fn style_ref(&self) -> &ToastActionStyle`

Return the action's rendering style (link vs button variant).

#### `pub fn closes_toast_flag(&self) -> bool`

Return `true` when invoking this action also dismisses the toast.

#### `pub fn shortcut_id_ref(&self) -> Option<&str>`

Return the associated `Shortcut` id, if any.

#### `pub fn tooltip_ref(&self) -> Option<&LocalizedString>`

Return the optional tooltip text, if one was set via `tooltip`.

#### `pub fn callback(&self) -> ToastActionCallback`

Clone the invocation callback — cheap because the underlying closure is `Rc`-wrapped.

## `pub struct ToastHandle`

Returned by `Toast::present` (and `ctx.show_toast(toast)`). Cheap
to clone (`Rc<Inner>`). Lets app code dismiss the toast
programmatically or check whether it is still alive.

Dropping the handle does NOT dismiss the toast — toasts have their
own lifecycle managed by the host (timer + manual paths). The
handle is the OPTIONAL "I want to control this toast later" hook.

```rust
pub struct ToastHandle { /* fields */ }
```

### Methods

#### `pub fn entry_id(&self) -> u64`

Stable per-toast id. Two `ToastHandle`s pointing at the same
underlying toast share the same `entry_id`. The id is unique
per `ToastRegistry` (per app) — it doesn't survive across app
restarts.

#### `pub fn is_alive(&self) -> bool`

Whether the toast is still in the registry's live set (timer
hasn't expired, user hasn't dismissed, host hasn't shut down).
Always `false` for overflow-dropped toasts.

#### `pub fn dismiss(&self, ctx: &mut EventContext)`

Programmatically dismiss the toast with cause
`ToastDismissCause::Programmatic`. No-op if the toast is
already dismissed (timer, user, host shutdown).

## `pub type ToastDismissCallback`

Type-erased on_dismiss callback receiving the cause + context.

```rust
pub type ToastDismissCallback = Rc<dyn Fn(ToastDismissCause, &mut EventContext)>;
```

## `pub struct Toast`

Toast — a present-able request (NOT a `Widget`). Construct with one
of the severity-named constructors, configure via builder methods,
then call `.present(ctx)` or `ctx.show_toast(self)`. Internally the
builder is consumed and its data is moved into a slot on the
installed `ToastHost`.

See the module docs for the full conceptual overview.

```rust
pub struct Toast { /* fields */ }
```

### Methods

#### `pub fn info(title: impl Into<LocalizedString>) -> Self`

Info-severity toast (status confirmation, neutral notice).

#### `pub fn success(title: impl Into<LocalizedString>) -> Self`

Success-severity toast ("Saved", "Connected", "Build finished").

#### `pub fn warning(title: impl Into<LocalizedString>) -> Self`

Warning-severity toast.

#### `pub fn error(title: impl Into<LocalizedString>) -> Self`

Error-severity toast. Defaults to `Live::Assertive`.

#### `pub fn loading(title: impl Into<LocalizedString>) -> Self`

Loading-style toast — Info severity with a
`Spinner` as the leading widget.
Persistent by default; the app calls
`ToastHandle::dismiss` (typically from the operation's
completion callback) or replaces it with a success/error toast.

#### `pub fn body(mut self, text: impl Into<LocalizedString>) -> Self`

Optional secondary text below the title.

**Long bodies clamp and disclose.** A title is one line by construction, but a body is
whatever the app passes — and apps pass error text, which has no length bound. A body
needing more than `TOAST_BODY_COLLAPSED_LINES` (3) lines is clamped there and grows a
thin row offering **Show more** and **Copy**:

- *Show more / Show less* unfolds and refolds it. Unfolding also cancels the toast's
  auto-dismiss timer — asking to see more is a statement that the toast is being read,
  and a 10-second timer firing mid-sentence is exactly what the disclosure exists to
  prevent. The close button (and the notification archive) remain the way out.
- *Copy* puts the **whole** body on the clipboard, not the visible three lines. A clamped
  body is usually an error chain; pasting its truncation would be worse than useless. The
  label switches to *Copied* on success, and stays a live link so a second click re-copies.
  A failed `set_text` (no clipboard, denied access) leaves the label alone rather than
  claiming a copy that did not happen.

A body short enough to fit gains none of this — no clamp, no row, no extra height. The
accessibility tree always carries the full text as the alert's description, clamped or
not, so a screen reader is never given the truncation.

Nothing about this is configurable yet; if you need a different ceiling, say so rather
than working around it with a pre-truncated string.

#### `pub fn leading(mut self, widget: impl Widget + 'static) -> Self`

Replace the default severity glyph with a custom leading
widget (spinner, app icon, avatar). Boxes the widget so the
toast remains object-safe.

#### `pub fn action(mut self, action: ToastAction) -> Self`

Append a `ToastAction` (link or button) to the toast.

#### `pub fn primary_action( self, label: impl Into<LocalizedString>, on_invoke: impl Fn(&mut EventContext) + 'static, ) -> Self`

Shorthand for appending a filled-button primary action — equivalent to
`.action(ToastAction::primary(label, on_invoke))`.

#### `pub fn auto_dismiss_after(mut self, duration: Duration) -> Self`

Override the auto-dismiss countdown. Pass `Duration::ZERO` for immediate dismissal
on the next timer tick; call `persistent` to disable the timer entirely.

#### `pub fn persistent(mut self) -> Self`

Disable auto-dismiss — the toast persists until the user
clicks the close X, invokes a `closes_toast` action, or the
app calls `ToastHandle::dismiss`.

#### `pub fn priority(mut self, priority: ToastPriority) -> Self`

Set the queue priority. `High` / `Urgent` entries evict the oldest `Normal` entry
when the slot pool is full; `Urgent` also forces `Live::Assertive` regardless of severity.

#### `pub fn id(mut self, id: impl Into<String>) -> Self`

Stable identity for the "progress toast updates in place"
pattern. Two toasts presented with the same id reuse the same
slot (mutation hooks are not yet implemented; the id is captured
and archived but each present allocates a new slot).

#### `pub fn on_click(mut self, f: impl Fn(&mut EventContext) + 'static) -> Self`

Treat a click on the toast body as a meaningful action — the
callback fires on tap. Cursor changes to `Pointer` over the body.

#### `pub fn on_dismiss( mut self, f: impl Fn(ToastDismissCause, &mut EventContext) + 'static, ) -> Self`

Notification of dismissal. Fires exactly once per toast on any
dismiss path (timer, action invocation, close click, escape,
programmatic, host shutdown, slot-pool overflow).

#### `pub fn show_close_button(mut self, show: bool) -> Self`

Show or hide the trailing close (×) button. Default `true`.

#### `pub fn closable_on_escape(mut self, allow: bool) -> Self`

Whether pressing Escape while the toast is focused dismisses
it. Default true. Set to false in apps that have a custom
Escape-handling story (focus trap, modal-style toast).

#### `pub fn announcement(mut self, text: impl Into<LocalizedString>) -> Self`

Override the screen-reader announcement text without changing
the visible title. Useful when the visible title is iconic
("3") but the spoken text needs context ("3 unread messages").

#### `pub fn archive(mut self, archive: bool) -> Self`

Whether this toast is added to the persistent archive that
drives `NotificationLog`.
Default `true`. Set `false` for noise-suppressing
transient notifications like quick "Copied!" feedback.

#### `pub fn style(mut self, style: impl teksilo_core::styles::ToastStyle) -> Self`

Override the visual chrome for this toast instance. Takes precedence over the
theme-wide `style_slots.toast` slot and the built-in `RecipeToastStyle` default.

#### `pub fn present(self, ctx: &mut EventContext) -> ToastHandle`

Submit the toast through the installed
`ToastHost`. Equivalent to
`ctx.show_toast(self)`. Returns a `ToastHandle` for
programmatic control. If `install_toast` was not called the
returned handle is in the "dropped" state (`is_alive` returns
`false`) and a one-shot stderr warning fires explaining the omission.
