# Toast Notification Reference

`bastyde_widgets::toast` ships the Bastyde notification system: stackable,
action-rich, severity-aware floating notifications backed by a
persistent archive with a log + bell-button + dialog UI.

Mental model in one line:

```
BastydeAppBuilder.install_toast_default() → ctx.show_toast(Toast::…) from any handler
```

Distinct from siblings:

| Widget | Shape | Lifetime | Stackable | Archive |
|---|---|---|---|---|
| [`Snackbar`](../crates/bastyde-widgets/src/snackbar.rs) | Bottom-center, single-instance | Auto-dismiss (4s default) | No (calls `dismiss_all_except_hosts`) | No |
| [`Banner`](../crates/bastyde-widgets/src/banner.rs) | Inline persistent strip | Until user dismisses | No (inline) | No |
| [`MessageBox`](../crates/bastyde-widgets/src/message_box.rs) | Modal dialog | Until user picks a button | No (one modal at a time) | No |
| **`Toast`** | Corner-anchored, stackable | Auto-dismiss (10s default) or persistent | **Yes** | **Yes** (persistent across restarts) |

End-to-end demo: `cargo run -p toast-demo`. Source:
[examples/toast_demo/src/main.rs](../examples/toast_demo/src/main.rs).

---

## Quickstart

```rust
use bastyde::prelude::*;
use bastyde::settings::AppPaths;

fn main() {
    BastydeAppBuilder::new()
        .theme(intui::light())
        .app_paths(AppPaths::new("com", "FernTech", "MyApp").unwrap())
        .install_toast_default()                    // ← one-line install
        .initial_window(WindowConfig::new()
            .id("main")
            .root(|tree, _state| tree.add(MyRoot::new())))
        .run();
}
```

Anywhere inside a handler:

```rust
fn save_handler(ctx: &mut EventContext) {
    save_to_disk();
    ctx.show_toast(
        Toast::success(tr!("saved"))
            .body(tr!("save_details", path = path.display()))
    );
}
```

For errors with replayable actions:

```rust
ctx.show_toast(
    Toast::error(tr!("build_failed"))
        .body(tr!("build_error_count", n = 3))
        .action(ToastAction::primary(tr!("retry"), |c| c.send_intent(AppIntent::BuildRetry)))
        .action(ToastAction::new(tr!("show_log"), |c| c.send_intent(AppIntent::ShowLog)))
);
```

For long-running operations that update in place:

```rust
let h = ctx.show_toast(Toast::loading(tr!("uploading_start")).id("upload"));
// later, with the same id:
ctx.show_toast(Toast::loading(tr!("uploading_progress", n = 4)).id("upload"));
// completion replaces the same entry:
ctx.show_toast(
    Toast::success(tr!("upload_complete"))
        .id("upload")
        .auto_dismiss_after(Duration::from_secs(5))
);
```

---

## Installing the toast system

One line at the builder. The `BastydeAppBuilderToastExt` extension trait
is re-exported from the umbrella prelude (`bastyde::prelude::*`) so
`install_toast(...)` / `install_toast_default()` are callable without
an extra import:

```rust
.install_toast_default()                          // BottomTrailing, persistent
.install_toast(ToastInstallOptions {              // explicit override
    corner: Corner::TopTrailing,
    archive: Some(NotificationArchive::in_memory()),
    ..ToastInstallOptions::default()
})
```

The toast subsystem ships behind the umbrella's `toast` feature
(default-on). To drop it (and the bell-icon SVG + archive code),
depend on `bastyde` with `default-features = false` and re-add only
the features you need.

What `install_toast` does, internally:

1. Opens the [`NotificationArchiveModel`](#notificationarchivemodel)
   — `InMemory` or `Persistent`. `Persistent` requires `app_paths(…)`
   to be set on the builder first; the install panics with a helpful
   message otherwise.
2. Constructs a shared [`ToastRegistry`](#toastregistry) bound to the
   archive (or naked if no archive is configured).
3. Registers a [`DefaultPostRoot`](../crates/bastyde-app/src/default_post_root.rs)
   closure that wraps every window's root with
   `ZStack { user_root, ToastHost::new(registry, options) }`. The
   `DefaultPostRoot` fires for every window the app opens — initial
   AND runtime-opened — so the host installs everywhere automatically.
4. Registers the `ToastRegistry` + `Rc<NotificationArchiveModel>`
   into `app_state` so `NotificationLog` /
   `NotificationCenterButton` / `NotificationLogDialog` can look
   them up.

## Install options

```rust
pub struct ToastInstallOptions {
    pub corner: Corner,                       // BottomTrailing (default)
    pub margin: Vec2,                         // (24.0, 24.0)
    pub gap: f32,                             // 8.0 between stacked toasts
    pub max_visible: usize,                   // 5
    pub entry_width: f32,                     // 380.0 (matches IntelliJ)
    pub pause_on_hover_group: bool,           // true (any hover pauses all)
    pub archive: Option<NotificationArchive>, // Persistent("notifications") (default)
}
```

| Field | Notes |
|---|---|
| `corner` | RTL-aware: `Trailing` flips to the physical left edge. Top corners stack downward, bottom corners upward. |
| `max_visible` | Normal-priority overflow drops with cause `SlotPoolFull`. High / Urgent priority evicts the oldest Normal to make room. |
| `pause_on_hover_group: true` | Hovering ANY live toast pauses every timer. `false` pauses only the hovered toast (libadwaita behaviour). |
| `archive: None` | Toasts work; just not archived. `NotificationLog` / bell button still load but show empty state. |
| `archive: Some(InMemory)` | Session-only ring buffer. Cleared on app exit. |
| `archive: Some(Persistent)` | TOML file at `<config>/notifications.toml` via `PersistedListModel`. Survives app restarts. |

---

## The `Toast` request

`Toast` is NOT a `Widget` — it's a present-able request, like
`MessageBox::present(ctx)`. Construct with a severity-named
constructor, configure via builder methods, then call
`ctx.show_toast(toast)` (or `toast.present(ctx)`).

### Constructors

| Constructor | Severity | Default behaviour |
|---|---|---|
| `Toast::info(title)` | `Info` | Status / confirmation |
| `Toast::success(title)` | `Success` | "Saved", "Deploy complete" |
| `Toast::warning(title)` | `Warning` | Non-fatal issue |
| `Toast::error(title)` | `Error` | Failure (forces `Role::Alert` + `Live::Assertive`) |
| `Toast::loading(title)` | `Info` | Persistent, with a `Spinner` leading widget |

Each has a `_literal` `#[doc(hidden)]` shim (`info_literal`, etc.)
for untranslated strings. Use during scaffolding; the grep marker
keeps "find me before localizing" findable.

### Builder methods

```rust
Toast::warning(tr!("unsaved"))
    .body(tr!("close_anyway"))                          // optional second line
    .leading(MyAppIcon::new())                          // override severity glyph
    .action(ToastAction::primary(tr!("save"), on_save)) // 0..N actions
    .primary_action(tr!("retry"), on_retry)             // shorthand for ToastAction::primary
    .auto_dismiss_after(Duration::from_secs(5))         // override the 10s default
    .persistent()                                       // disable auto-dismiss
    .priority(ToastPriority::High)                      // Normal | High | Urgent
    .id("unique-key")                                   // update-in-place key (see below)
    .on_click(|ctx| ctx.send_intent(AppIntent::Open))   // click-anywhere callback
    .on_dismiss(|cause, ctx| log_dismiss(cause))        // fires once
    .show_close_button(false)                           // default: true
    .closable_on_escape(false)                          // default: true
    .announcement(tr!("custom_at_text"))                // override AT name
    .archive(false)                                     // opt out of archive mirror
    .style(MyToastStyle)                                // per-call style override
```

### `ToastAction`

```rust
ToastAction::new(label, on_invoke)              // Link (default)
ToastAction::primary(label, on_invoke)          // Filled button
ToastAction::destructive(label, on_invoke)      // Destructive button
ToastAction::new(label, on_invoke)
    .style(ToastActionStyle::Button { variant: ButtonVariant::Plain })
    .closes_toast(false)                        // default: true (IntelliJ "expiring")
    .shortcut_id("app.save")                    // for archive replay
    .tooltip(tr!("save_explainer"))
```

Link actions render inline in the body row; Button actions go in a
footer row below the body. Mixed within the same toast is allowed.

### Update-in-place

`Toast::id("…")` is the dedup key. When a second toast carrying a
matching id is presented:

- **Live side**: the existing entry's fields mutate in place (title,
  body, severity, actions, …) and the auto-dismiss timer resets.
  The original `entry_id` is preserved — the first call's
  `ToastHandle` keeps working.
- **Archive side**: the existing archived entry merges (an
  `UpdateRecord` appended to its `updates` Vec); no new row appears
  in `NotificationLog`.

Preserved across updates that don't re-specify them:

| Field | Preserved when update omits it |
|---|---|
| `on_dismiss` | Yes (the original callback survives) |
| `leading` | Yes (a `Toast::loading` spinner survives `Toast::info` updates) |
| Other fields | No (always overwritten by the update) |

When the update DOES specify `on_dismiss` / `leading`, the
replacement installs and the previous values drop silently.

Contract: `on_dismiss` fires exactly once per entry, with the
most-recently-supplied callback.

### Setting `archive(false)` on an update

The update's `archive` flag is honored. If the original was
archived but a subsequent update sets `archive(false)`, the existing
archive record stays in place but no further updates are mirrored.
Apps that want continuous archive capture should leave `archive` at
default `true` across updates.

### `ToastHandle`

Returned by `ctx.show_toast(...)` and `toast.present(ctx)`. Cheap
to clone (`Rc<Inner>`).

```rust
let h = ctx.show_toast(Toast::loading_literal("Working…"));

// Some time later, in another handler:
if h.is_alive() {
    h.dismiss(ctx);            // ToastDismissCause::Programmatic
}
```

Dropping the handle does NOT dismiss the toast — toasts have their
own lifecycle (timer + manual paths). The handle is OPTIONAL
"I want to control this later" wiring.

### Severity × priority → AT role × live region

The AT role / live region is computed from severity + priority:

| Severity | Priority | `Role` | `aria-live` |
|---|---|---|---|
| `Info`, `Success` | any | `Status` | `Polite` |
| `Warning` | `Normal` | `Status` | `Polite` |
| `Warning` | `High` / `Urgent` | `Alert` | `Assertive` |
| `Error` | any | `Alert` | `Assertive` |
| any | `Urgent` | (forces) | `Assertive` |

All toasts call `set_live_atomic()` so the entire title+body
announces as one unit. The `announcement(...)` builder overrides
the spoken text without changing the visible title (useful when
the visible title is iconic but the spoken text needs context).

### `ToastDismissCause`

| Cause | When |
|---|---|
| `Timeout` | `auto_dismiss_after` reached zero (timer expired naturally) |
| `ActionInvoked` | A `ToastAction` with `closes_toast(true)` (the default) fired |
| `CloseClicked` | User clicked the X |
| `EscapePressed` | User pressed Escape while focused into the toast |
| `Programmatic` | `ToastHandle::dismiss()` or `ctx.dismiss_toast(handle)` |
| `HostShutdown` | Window is being torn down |
| `SlotPoolFull` | Pool full + Normal-priority drop, OR evicted by a higher-priority arrival |

---

## `ToastRegistry`

Cheap to clone (`Rc<RefCell<…>>`). Holds the queue + per-entry
state, registered in app-state by `install_toast`. Apps don't usually
construct one directly — use the install hook.

```rust
pub struct ToastRegistry { … }

impl ToastRegistry {
    pub fn new(opts: ToastInstallOptions) -> Self                      // no archive
    pub fn with_archive(opts, archive: Rc<NotificationArchiveModel>) -> Self
    pub fn archive(&self) -> Option<Rc<NotificationArchiveModel>>
    pub fn version_signal(&self) -> &Signal<u64>                       // bumps on every change
    pub fn hover_count_signal(&self) -> Signal<usize>                  // shared pause refcount
    pub fn live_count(&self) -> usize                                  // test helper
}
```

The `version_signal` is what `ToastHost` binds to at
`BindingLevel::Rebuild`; bumps on enqueue / in-place merge / timer
expiry / programmatic dismiss / close-click / action-invoked.

`hover_count` is shared between every `ToastSurface`: each surface's
outer wrapper has an `on_hover` handler that increments / decrements
this refcount. The host's frame-tick effect reads `count > 0` as
"pause all timers" (when `pause_on_hover_group: true`).

---

## `ToastHost`

Invisible sibling widget owning the queue + per-frame timer. The
install hook wraps every window with
`ZStack { user_root, ToastHost::new(registry, options) }`. Direct
construction is needed only when apps want manual mounting
control:

```rust
pub fn new(registry: ToastRegistry, options: ToastInstallOptions) -> Self
```

Renders zero of its own chrome. Each live entry from the registry
becomes a `ToastSurface` child placed at the configured corner with
stacked offset. Newer entries are placed closer to the corner
anchor (matches IntelliJ + Windows convention).

Per-frame-tick effect (subscribed via `ctx.subscribe_frame_tick()`)
decrements every entry's `time_left` by the wall-clock dt — unless
hover-pause is active. Expired entries are dismissed deferred (the
user callback fires on the next pointer event).

---

## `NotificationArchiveModel`

The persistent layer behind the live toast queue. Two backends:

| Backend | Use case |
|---|---|
| `InMemory { limit }` | Session-only. No disk I/O. Cleared on exit. |
| `Persistent { file_name, limit }` | `<config>/<file>.toml` via [`PersistedListModel`](settings.md#listfile-and-persistedlistmodel). Survives restarts. |

Default `limit`: `DEFAULT_ARCHIVE_LIMIT = 200`. IntelliJ keeps
hundreds; Bastyde picks a pragmatic cap so persistent files don't
grow unbounded.

### `NotificationEntry`

The persistent shape — closures dropped, `intent_name` retained for
archive replay:

```rust
pub struct NotificationEntry {
    pub id: u64,                         // per-archive stable id, monotonic
    pub severity: BannerSeverity,
    pub priority: ToastPriority,
    pub title: String,
    pub body: Option<String>,
    pub actions: Vec<ArchivedAction>,
    pub timestamp: jiff::Timestamp,
    pub group: Option<String>,           // optional visual grouping key
    pub source: Option<String>,          // optional originating-feature tag
    pub read: bool,                      // flipped by mark_all_read
    pub dedup_id: Option<String>,        // sourced from Toast::id
    pub updates: Vec<NotificationUpdate>,// appended by in-place merges
}

pub struct ArchivedAction {
    pub label: String,
    pub intent_name: Option<String>,     // sourced from ToastAction::shortcut_id
    pub style: ArchivedActionStyle,      // Link | PrimaryButton | SecondaryButton | Destructive
    pub closes_on_invoke: bool,
}

pub struct NotificationUpdate {
    pub timestamp: jiff::Timestamp,
    pub title: Option<String>,           // None if unchanged
    pub body: Option<String>,
    pub progress: Option<f32>,
}
```

### Methods

```rust
pub fn open(archive, paths, debounce) -> Result<Self, NotificationArchiveError>
pub fn in_memory() -> Self                                  // convenience
pub fn entries(&self) -> &ListModel<NotificationEntry>      // observable
pub fn unread_count(&self) -> &Signal<usize>                // drives the badge
pub fn version_signal(&self) -> &Signal<u64>                // drives the log rebuild
pub fn push(&self, entry: NotificationEntry)                // bounded; deduplicates by dedup_id
pub fn mark_all_read(&self)                                 // resets unread to 0
pub fn clear(&self)
pub fn remove(&self, index: usize)
pub fn flush_now(&self) -> Result<(), SettingsFileError>    // persist immediately (test helper)
```

`push` semantics:
- New entry → inserts at index 0 (newest first), evicts oldest past `limit`, bumps `unread_count` + `version`.
- Existing `dedup_id` → updates the matching entry's title/body in place, appends a `NotificationUpdate` to its `updates`, flips it back to unread, bumps `unread_count` + `version`.

`version_signal` bumps on every actual mutation; no-ops (mark_all_read
on a fully-read archive, clear on empty) don't bump.

---

## `NotificationLog`

Widget rendering the archive as a filterable list. Composition:

```
VStack {
    Toolbar { Spacer, "Mark all read" button, "Clear all" button },
    ScrollArea {
        VStack {
            "Today" header,
            row, row, …
            "Yesterday" header,
            row, row, …
            "This week" header,
            …
            "Earlier" header,
            …
        }
    },
    // OR empty-state hint when the archive is empty
}
```

Each row is a [`StandardListItem`](../crates/bastyde-widgets/src/standard_item.rs):
severity glyph leading, title in `BodyBold` (unread) / `Body`
(read), body as subtitle, action buttons trailing. Outer node
carries `Role::List` + `set_name("Notifications")`.

### Builder

```rust
NotificationLog::new(archive: Rc<NotificationArchiveModel>)
    .show_toolbar(true)                                       // default true
    .empty_state(my_empty_widget)                             // override default text
    .on_entry_invoked(|entry, ctx| open_details(entry, ctx))  // click-row callback
    .on_action_invoked(|entry, action, ctx| {                 // archive-replay callback
        match action.intent_name.as_deref() {
            Some("app.build.retry") => ctx.send_intent(AppIntent::BuildRetry),
            Some(name) => log::warn!("unknown archived intent: {name}"),
            None => {}
        }
    })
```

### Archive-action replay model

`ArchivedAction::intent_name` is captured from
`ToastAction::shortcut_id`. The log uses it to decide whether an
action is clickable:

| `intent_name` | `on_action_invoked` set | Renders as |
|---|---|---|
| Present | Yes | Clickable Link / Button → fires the callback |
| Present | No | Inert text tag |
| `None` | any | Inert text tag with "(no longer available)" suffix |

The "no longer available" tag matches IntelliJ's event-log
semantics: actions that depended on live closures can't be replayed
once the closure has torn down. Actions that go through the Intent
system survive archival — apps map the archived `intent_name` to
their typed `AppIntent` variants in the callback.

### Day-bucket section headers

Entries are grouped into four buckets, computed against the user's
local timezone via [`jiff::Zoned`](https://docs.rs/jiff):

| Bucket | Range |
|---|---|
| **Today** | Same local-calendar date |
| **Yesterday** | `today - 1` day |
| **This week** | 2..=6 days ago |
| **Earlier** | 7+ days ago |

Future timestamps (clock skew, peer sync) bucket as **Today** so
they don't silently slip into Earlier.

Buckets recompute on every archive mutation (the log binds to
`archive.version_signal()` at Rebuild). A log left open across
midnight keeps stale labels until the next mutation OR until the
user reopens — acceptable for the popover-shaped UX.

---

## `NotificationCenterButton`

Bell-icon trigger + live unread badge + popover containing a
`NotificationLog`. The de-facto status-bar widget.

```rust
NotificationCenterButton::new(archive: Rc<NotificationArchiveModel>)
    .size(IconButtonSize::Toolbar)                  // default Toolbar (30dp)
    .show_badge_when_zero(false)                    // default: hide badge at 0
    .max_badge_count(99)                            // default 99; counts above show "99+"
    .placement(OverlayPlacement::BelowPreferred)    // popover anchor
    .on_action_invoked(|entry, action, ctx| { … }) // forwarded to the embedded log
```

The badge label binds reactively to `archive.unread_count()`. On
popover open, the archive's `mark_all_read` runs (the user is
presumed to have seen the toasts now) — the badge resets, the
badge widget disappears.

```
┌─────┐
│ 🔔3 │  ← bell icon + badge with unread count
└─────┘
   ↓ click
┌──────────────────────────────────────┐
│                  Mark all read │ Clear │
│ Today                                  │
│ • [⚠] Build #42 failed   [Retry][Log] │
│ • [✓] File saved                       │
│ Yesterday                              │
│ • […] Upload complete                  │
│ Earlier                                │
│ …                                       │
└──────────────────────────────────────┘
```

A custom `BellButton` example wrapper that pulls the archive from
app-state lives in the demo:
[examples/toast_demo/src/main.rs](../examples/toast_demo/src/main.rs).

---

## `NotificationLogDialog`

One-liner modal preset wrapping `NotificationLog` in a
`ModalContainer`:

```rust
NotificationLogDialog::show(archive, ctx);
NotificationLogDialog::show_with(archive, ctx, |log| {
    log.on_action_invoked(|entry, action, ctx| { … })
});
```

Default presentation: `ModalPresentation::Auto`, 720×520 size,
title "Notifications", dismissed via Escape or click-outside.

Apps wire this to a menu item ("Window → Notification Log…") or a
shortcut.

---

## Accessibility checklist

| Concern | How it's handled |
|---|---|
| Role mapping | Status / Alert per the severity × priority table above. |
| Live region | `Live::Polite` / `Live::Assertive` per the same table; `set_live_atomic()` so title+body is one announcement. |
| Custom announcement text | `Toast::announcement(text)` overrides the AT name without changing the visible title. |
| Body description | `Toast::body(text)` → `set_description(body)` on the surface node. |
| Severity glyph | `set_hidden()` — presentational, not in the AT tree (the surface itself carries the role + name). |
| Close button | `IconButton` with its own `Role::Button` + tooltip / a11y label. Reachable by Tab once focus is inside the toast. |
| Action buttons | Standard Button / Link a11y. |
| Escape | Dismisses with cause `EscapePressed` while focus is inside the toast. |
| Bell button | `IconButton` with the localized "Notifications" tooltip. Badge widget contributes its own `Role::Label` with the count. |
| Log | `Role::List` outer + `Role::ListItem` rows + section headers as plain `TextWidget` with `Secondary` color. |

`Urgent` priority forces `Live::Assertive` regardless of severity —
the escape hatch for "this Info-level toast is actually
time-critical" cases.

Per the WAI-ARIA spec gotcha: the live-region node must exist in
the AT tree before its content is populated, otherwise ATs miss the
announcement. `ToastSurface::build` handles this — the surface's AT
node is constructed empty, then bound to the title signal one frame
later (via `ctx.subscribe_frame_tick()` one-shot).

---

## Reduced motion

`prefers-reduced-motion` is consulted at the host's enqueue path.
When set: no fade-in / fade-out on the overlay, no slide-in. The
surface appears in place, instantly. The auto-dismiss timer still
runs normally — reduced motion affects only the visual transition.

---

## i18n keys

Built-in keys (en-US source + fr-FR translation shipped):

| Key | Default text |
|---|---|
| `a11y-builtin-bell` | "Notifications" |
| `notifications-title` | "Notifications" |
| `notifications-empty` | "No notifications" |
| `notifications-mark-all-read` | "Mark all read" |
| `notifications-clear` | "Clear all" |
| `notifications-filter-placeholder` | "Search notifications" (reserved) |
| `notifications-bucket-today` | "Today" |
| `notifications-bucket-yesterday` | "Yesterday" |
| `notifications-bucket-this-week` | "This week" |
| `notifications-bucket-earlier` | "Earlier" |
| `notifications-archive-replay-disabled` | "(no longer available)" |

Apps that need more locales register them through the framework's
standard `I18nConfig::framework_locales(bastyde_widgets::framework_locales())`.

---

## Where the code lives

| Concern | File |
|---|---|
| `Toast` request + `ToastAction` + `ToastDismissCause` + `ToastHandle` + `ToastInstallOptions` | [crates/bastyde-widgets/src/toast.rs](../crates/bastyde-widgets/src/toast.rs) |
| `ToastRegistry` (queue + archive bridge + in-place merge) | [crates/bastyde-widgets/src/toast/registry.rs](../crates/bastyde-widgets/src/toast/registry.rs) |
| `EventContextToastExt` (`ctx.show_toast` / `ctx.dismiss_toast`) | [crates/bastyde-widgets/src/toast/ext.rs](../crates/bastyde-widgets/src/toast/ext.rs) |
| `ToastSurface` (chrome + a11y + custom actions) | [crates/bastyde-widgets/src/toast/surface.rs](../crates/bastyde-widgets/src/toast/surface.rs) |
| `ToastHost` (queue display + timer + hover-pause) | [crates/bastyde-widgets/src/toast/host.rs](../crates/bastyde-widgets/src/toast/host.rs) |
| `RecipeToastStyle` (default chrome) | [crates/bastyde-widgets/src/styles/recipe_toast_style.rs](../crates/bastyde-widgets/src/styles/recipe_toast_style.rs) |
| `ToastStyle` trait + `ToastPriority` + `ToastStyleConfig` | [crates/bastyde-core/src/styles/toast_style.rs](../crates/bastyde-core/src/styles/toast_style.rs) |
| `NotificationEntry` + `ArchivedAction` + `NotificationUpdate` | [crates/bastyde-widgets/src/notification.rs](../crates/bastyde-widgets/src/notification.rs) |
| `NotificationArchiveModel` (`InMemory` / `Persistent`) | [crates/bastyde-widgets/src/notification/archive.rs](../crates/bastyde-widgets/src/notification/archive.rs) |
| `NotificationLog` (toolbar + buckets + rows) | [crates/bastyde-widgets/src/notification/log.rs](../crates/bastyde-widgets/src/notification/log.rs) |
| `NotificationCenterButton` (bell + badge + popover) | [crates/bastyde-widgets/src/notification/center_button.rs](../crates/bastyde-widgets/src/notification/center_button.rs) |
| `NotificationLogDialog` (modal preset) | [crates/bastyde-widgets/src/notification/log_dialog.rs](../crates/bastyde-widgets/src/notification/log_dialog.rs) |
| `install_toast` extension trait (`BastydeAppBuilderToastExt`) | [crates/bastyde/src/toast_install.rs](../crates/bastyde/src/toast_install.rs) |
| Runnable demo | [examples/toast_demo/src/main.rs](../examples/toast_demo/src/main.rs) |

## Related core API additions

The toast system landed alongside small bastyde-core extensions reusable
by other overlay-shaped widgets:

- `bastyde_tokens::Corner` (`TopLeading` / `TopTrailing` / `BottomLeading` /
  `BottomTrailing`) with RTL-aware `resolve(content, viewport, margin, rtl)`.
- `OverlayPlacement::ViewportCorner { corner, margin }` —
  anchor-independent corner-snapped overlay placement.
- `OverlayManager::pause_auto_dismiss(id)` /
  `resume_auto_dismiss(id)` — stash `auto_dismiss_after - elapsed`
  and restore with a fresh `shown_at_*`. Available to any overlay
  caller; ToastHost owns its own timer so doesn't use these, but
  Snackbar refinements or future hover-pause-able overlays can.
- `EventContext::pause_overlay_auto_dismiss(id)` /
  `resume_overlay_auto_dismiss(id)` — handler-side queue that
  forwards to `OverlayManager` after the dispatcher returns.
- `BastydeAppBuilder::configured_app_paths()` — read-side companion to
  the existing `app_paths(paths)` setter. Builder-extension traits
  use it to open persistent files at install time before `run`.
- `bastyde_core::styles::ToastStyle` slot + `ComponentStyleSlots::toast`.

## Limitations & explicit non-goals

- **Multi-window**: a single shared `ToastRegistry` per app. Toasts
  show up in whichever window's host last bound — apps with multiple
  windows share one queue. Per-window routing is a planned
  refinement.
- **OS-level notifications** (libnotify / NSUserNotification / Windows
  toast): out of scope. Toast is an *in-app* widget. Apps wanting
  OS notifications wire that through a separate crate.
- **Inline reply / combo / picker inside a toast** (Windows toast
  text-box / combo): non-portable, out of pattern for Bastyde.
- **Cross-window deduplication**: `Toast::id` is per-app (the
  registry is app-singleton), but the `NotificationLog` rebuild
  signal is per-archive — multi-window apps see the SAME log in
  every bell button.
- **Sticky promotion via long hover** (the tooltip dwell pattern):
  not needed — a Toast is either sticky from the start
  (`persistent()`) or timed; there's no progressive promotion.
- **Live update-in-place visual transition**: the surface mutates
  in place (no fade-out + fade-in), which matches the IntelliJ
  pattern. Apps that want a visual handoff can dismiss + show
  separately instead of using `Toast::id`.
- **`rich-text`-gated SearchField filter in NotificationLog**:
  documented as a future refinement. Apps can compose external
  filtering with their own toolbar above the log.
- **Severity-chip filter in NotificationLog**: same — composable
  with `SegmentedControl`.
