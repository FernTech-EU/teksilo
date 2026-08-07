<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Automation MCP — Drive a Teksilo App from an Agent

A Teksilo app exposes a rich semantic surface — the **AccessKit accessibility
tree** — plus an **AT-action dispatch path**, both of which are queryable and
drivable **in-process, without the OS accessibility layer**. The
`teksilo-automation-mcp` server turns that latent capability into a
[Model Context Protocol](https://modelcontextprotocol.io) (MCP) server so an
AI agent (or any MCP client) can **observe** (semantic tree + screenshots) and
**drive** (AT actions + synthetic input) a Teksilo app.

This is the capability an agent can't get otherwise: the `TreeUpdate` lives
inside private platform state with no external channel except the OS AT layer;
the AT-action channel is OS-AT-only; headless operation needs no display
server; and a `WidgetId`-derived node id (stable across *in-place* changes —
see the tool surface) is steadier to cache than a fragile OS handle. It
**complements** (does not replace) a real screen-reader OS smoke test — the
live `--connect` mode (debug builds) drives your actual app, while the headless
mode is the toolkit's CI harness / a build-your-own-harness kit (see below).

## Two modes

| Mode | Command | What it drives |
| --- | --- | --- |
| **Headless** (default) | `teksilo-automation-mcp --headless` | A built-in demo app owned entirely in-process on a dedicated thread. No display, GPU daemon, or AT layer needed. The right mode for CI and agent test-authoring. |
| **Live (connect)** | `teksilo-automation-mcp --connect <sock> --token <uuid>` | A *running* app that opted into the debug-only in-app bridge. The agent drives the real window the user sees. |

Both speak MCP over **stdio**.

### Headless

```text
teksilo-automation-mcp --headless
```

A dedicated `std::thread` owns a `HeadlessApp` and the async rmcp handlers
marshal `Send` DTOs to it; the `!Send` `WidgetTree` never leaves that thread.
Screenshots render offscreen on the tree thread via `pollster::block_on`
(reusing `teksilo_render::test_support::create_test_renderer` — the same
offscreen path the widget previewer's PNG export uses).

**What the stock binary drives.** `teksilo-automation-mcp --headless` builds a
small *built-in demo* (a heading, two buttons, a text field, a checkbox) — it
is the toolkit's own conformance harness and a worked reference, **not** your
app. To headlessly automate *your* app there are two paths:

- **Build a tiny harness** with the GUI-free `teksilo-automation` crate: own
  your app's `WidgetTree` on one thread (or reuse a headless test tree) and
  call `teksilo_automation::execute(&mut tree, &mut ops, &op, &settle)` per
  request. `execute` works against any `WidgetTree`, so this is ~a screenful of
  glue — but it is a kit, not a turnkey "point it at my app" binary.
- **Use the live `--connect` mode** below — the turnkey "drive my real app"
  path today (needs a display and a running debug build).

### Live (connect)

A debug build opts in with one line:

```rust
use teksilo::prelude::*;

TeksiloAppBuilder::new()
    .theme(intui::light())
    .install_automation_bridge_in_debug()   // debug-only; a no-op in release
    .initial_window(/* ... */)
    .run();
```

On startup it prints the socket path and token to stderr:

```text
teksilo-automation: bridge socket = /run/user/1000/teksilo-automation-12345.sock
TEKSILO_AUTOMATION_TOKEN=8f3c…
teksilo-automation: connect with `teksilo-automation-mcp --connect /run/user/1000/teksilo-automation-12345.sock --token 8f3c…`
```

Then point the server at it:

```text
teksilo-automation-mcp --connect /run/user/1000/teksilo-automation-12345.sock --token 8f3c…
```

Each rmcp tool handler writes the op to the Unix socket and reads one reply;
the in-app bridge thread reads the socket and posts an `AutomationPayload`
(carrying a `Send` reply channel) through the existing `AppEvent::External`
path, and the winit **main thread** runs the op against the real window — the
settle runs synchronously on the main thread, never across a frame boundary.

## Install

Add the `automation` feature to the umbrella crate (debug-only by design):

```toml
[dependencies]
teksilo = { version = "0.6", features = ["automation"] }
```

`install_automation_bridge_in_debug()` is gated on `debug_assertions`: a
**release** build with the feature on still contains no socket, token, or
bridge — the method is the identity. The GUI-free DTO toolkit is available as
`teksilo::automation` for writing harnesses against the same protocol.

The server binary builds from `cargo build -p teksilo-automation-mcp`.

## Tool surface (27 tools)

Mutating tools accept an optional `settle` argument (see the settle model
below); query tools don't. Node ids come from `snapshot_tree` / `find_node`
and are the raw AccessKit `NodeId` values — derived deterministically from the
widget id. An id is stable for the **lifetime of the widget instance** (across
relayout, repaint, theme, and locale changes, which mutate widgets in place),
but a *structural rebuild* that destroys and recreates the widget — a
data-model change, a `Switcher` swap, a `Rebuild`-level binding — allocates a
new id. So caching an id is the payoff over an OS handle for *in-place*
changes; after a structural change, re-`find_node` (by role/label, which
carries the usual label fragility) rather than reuse a possibly-stale id.

**Query** — `snapshot_tree`, `read_node`, `find_node`, `assert_node`,
`list_windows`

**Layout / geometry** — `layout_tree`, `inspect_node`

**Drive (AT actions)** — `invoke_action`, `focus_node`, `set_value`,
`expand`, `collapse`, `scroll`

**Synthetic input** — `inject_pointer`, `right_click`, `inject_key`,
`type_text`, `type_ime`, `drag_node`

**Introspection** — `get_overlays`, `get_shortcuts`, `list_live_regions`,
`pull_announcements`

**Time / settle** — `advance_clock`, `settle`, `wait_for_condition`

**Visual** — `screenshot` (returns an MCP image content block)

`snapshot_tree` returns `{ root, focus, nodes: [SemanticNode…] }`, where each
`SemanticNode` carries `id`, `role`, `label`, `value`, `toggled`, `expanded`,
`selected`, `disabled`, `focused`, `live`, `numeric_value`, `bounds`,
`actions`, and `children`.

`layout_tree` and `inspect_node` expose the **full widget/layout (arena)
tree** — the same data the debug inspector's Tree + Properties tabs show, and
strictly richer than the accessibility snapshot: it includes widgets the AT
tree *prunes* (layout primitives like `HStack`/`Padding`/`Spacer`, dormant
`Switcher` branches, presentational / `access_exclude` widgets), so an agent
can debug layout (overlap, clipping, off-screen, wrong size) — not just
semantics. Each `LayoutNode` carries `id`, `type` (the concrete Rust type
name), `bounds`, `active`, `clips_children`, `parent`, `children`, and — when
requested (`include_debug`, or always for `inspect_node`) — `debug`, the
widget's `Debug` repr (its constructor parameters). Layout nodes are keyed by
the **same** node-id space as the AT tools, so when a widget appears in both,
the two records share an `id` and can be correlated. (Coordinates are logical
window-relative pixels, identical to the AT `bounds`.)

### Right-click & context menus

Teksilo context menus are attached with the `.context_menu(factory)` builder
and open on a **Secondary `PointerDown`** — a real right-click. To open one from
automation, use the node-based **`right_click`** tool:

```jsonc
// 1. Find the row / cell / item you want the menu for.
find_node { "role": "Row", "label": "report.pdf" }   // → { "node": 123 }
// 2. Right-click it — injects a secondary press+release at its centre.
right_click { "node": 123 }
// 3. Read the menu that opened, then pick an item.
get_overlays                                          // → { "count": 1, … }
find_node { "role": "MenuItem", "label": "Rename" }  // → { "node": 456 }
invoke_action { "node": 456, "action": "click" }
```

Equivalent alternatives, in order of preference:

- **`right_click(node)`** — the clearest verb; no coordinate math. Preferred.
- **`invoke_action(node, "show_context_menu")`** — the `ShowContextMenu` AT
  action. It routes to the same `.context_menu(..)` factory (unless the widget
  wires its *own* `ShowContextMenu` handler, which then wins). This is exactly
  what a screen reader's "show context menu" does.
- **`inject_pointer(x, y, button="secondary")`** — the low-level path. Only
  reach for it when you need a menu at a *specific point* rather than a node's
  centre; you must supply coordinates yourself (from a node's `bounds`).

After any of these, **settle** runs automatically, so the very next
`snapshot_tree` / `get_overlays` / `find_node` sees the mounted menu. Dismiss it
with `inject_key { "key": "escape" }` or by clicking elsewhere.

### Multi-window routing

Every tool accepts an optional `window_id` (the `TeksiloWindowId` raw value
from `list_windows`). `window_id: None` resolves to the focused window, else
the primary. Headless is single-tree, so routing is always unambiguous there.

## The settle model

After a mutating op, the executor settles the tree so the next snapshot
reflects the change, then re-syncs the AT tree. The `settle` argument (all
fields optional) is:

| Field | Default | Meaning |
| --- | --- | --- |
| `clock_millis` | `0` | Advance the simulation clock first (drives tooltip / overlay timers). |
| `max_anim_frames` | `60` | Cap on 16 ms animation ticks (~1 s). A perpetually-looping animation hits the cap — expected. |
| `layout_after` | `true` | Run a layout pass after ticking, so height-for-width / reflow settles before the AT re-walk. |
| `settle_timeout_ms` | `500` | Hard wall-clock budget; exceeding it ends the settle (the live bridge reports `SETTLE_TIMEOUT`). |

The settle loop is **simulation-clock-driven** (`tick_animations` doesn't wait
on VSync or OS events), so it can't deadlock — it progresses to quiescence or
the cap. `wait_for_condition` polls `snapshot → predicate` on the same clock
until a `NodeExists` / `NodeValue` / `NodeGone` / `AtVersionAtLeast` condition
holds or `settle_timeout_ms` elapses (`WAIT_TIMEOUT`).

### Live regions & announcements

Teksilo has no OS AT layer in headless mode, and no in-process way to observe
what the platform *spoke*. So the `WidgetTree` diffs the live (`Live::Polite` /
`Live::Assertive`) nodes of each freshly-built `TreeUpdate` and records the
changes into a ring buffer. `pull_announcements { since_seq }` drains it — a
faithful, in-process model of the live-region stream. `list_live_regions`
reports the live nodes themselves.

## Screenshots

`screenshot` renders the window (or, with `node`, that node's bounds) to a PNG
and returns it as an MCP **image content block**.

- **Headless:** an offscreen `RENDER_ATTACHMENT | COPY_SRC` texture, rendered
  via the test renderer, read back, PNG-encoded. If no GPU backend is present
  (CI without a GPU), the tool returns `GPU_UNAVAILABLE` (non-fatal).
- **Live:** `PlatformWindow::capture_offscreen` renders the live frame into an
  offscreen texture in the window's own surface format (the swapchain texture
  lacks `COPY_SRC`), swizzling BGRA→RGBA as needed; the bridge base64-encodes
  the PNG over the socket and the `--connect` client rehydrates it to an image
  block.

**WebView blind spot:** a native `WebView` subview composites *on top of* the
wgpu surface and is invisible to the readback (a transparent hole). When the
AT tree contains a `WebView` node the `screenshot` reply adds
`warnings: ["webview_hole_possible"]`. There is no platform-capture workaround
in scope.

## Security model

The live bridge is defence-in-depth:

- **Feature- and debug-gated.** Every item that binds the socket, generates the
  token, or spawns the bridge thread is `#[cfg(debug_assertions)]`; a release
  build's `install_automation_bridge_in_debug()` is the identity. A shipped
  release binary contains no socket, token, or bridge.
- **Unix-domain socket only** (never TCP), in a `0700` per-process directory
  under `$XDG_RUNTIME_DIR` with a `0600` socket (so it isn't world-connectable
  even during the bind→chmod window), removed on startup and on bridge-thread
  exit.
- **Single connection at a time**, single in-flight request, with a 10 s
  read-timeout on the token handshake and a 16 MiB cap on a request frame.
- **Per-process UUID token**: the client must send the token (printed to
  stderr, or pinned via `TEKSILO_AUTOMATION_TOKEN`) as the first line, or the
  connection is rejected.
- **Bounded main-thread settle.** Because the live settle runs on the winit
  main thread, the bridge clamps `max_anim_frames ≤ 120` and
  `settle_timeout_ms ≤ 2000` so no op (including a long `wait_for_condition`)
  can freeze the UI for more than ~2 s. Headless keeps the caller's values (no
  UI to freeze).

The threat model is a *trusted local user with a non-shared `$XDG_RUNTIME_DIR`*
— the same-uid socket plus the per-process token. The token is printed to
stderr, so anyone who can read the app's stderr (or `/proc/<pid>/environ` when
it's pinned) can drive the UI; this is a dev-tool stance, kept out of
production by the debug gate above. **Regression guard:** the debug-only banner
string only exists in the gated `install`, so a release binary with the feature
on must not contain it — a CI canary:

```sh
cargo build --release -p widget-catalog          # has the bridge wired + feature
! grep -qa "teksilo-automation: bridge socket" target/release/widget-catalog \
  || { echo "BRIDGE LEAKED INTO RELEASE"; exit 1; }
```

Headless mode has no socket at all.

## Documented limitations

- **WebView pixels** in screenshots (compositor hole — warned, not captured).
- **Windows live bridge**: the socket is Unix-only;
  `install_automation_bridge_in_debug()` is a no-op on Windows (headless mode
  works everywhere — it has no socket).
- **Release-build automation**: debug-gated by design.
- **Software-GPU fallback** for CI screenshots: a clean `GPU_UNAVAILABLE`
  error, not a fallback.
- This complements, **not replaces**, a real screen-reader OS round-trip.

## Architecture

The wire protocol is **serde DTOs, never closures or `!Send` handles**. One
core function does the work, in the GUI-free `teksilo-automation` crate:

```rust
teksilo_automation::execute(
    tree: &mut WidgetTree,
    ops: &mut dyn WindowOps,
    op: &AutomationOp,
    settle: &SettleSpec,
) -> AutomationReply
```

`WidgetTree` is `Rc/RefCell`-based and therefore `!Send`, so it lives on
exactly one thread; the async / socket layers marshal `Send` DTOs to it.
`list_windows` and `screenshot` are the only ops `execute` can't serve (they
need the window manager / a GPU); both return `HOST_REQUIRED`, and the
headless tree thread and the live bridge serve them with the context they
alone hold.

| Crate | Role |
| --- | --- |
| `teksilo-automation` | GUI-free toolkit: DTOs, `execute`, `RecordingWindowOps`, the tool catalog. Mirrors `teksilo-data`'s core-only-peer design. |
| `teksilo-automation-mcp` | The rmcp server binary (`--headless` / `--connect`) + offscreen screenshots. `tokio` / `rmcp` are confined here. |
| `teksilo-app` (`automation` feature) | The debug-only in-app bridge — `std::os::unix::net` + the existing `send_external` path; no async runtime in the framework. |
| `teksilo-platform` | `PlatformWindow::capture_offscreen` — the live-window readback. |

`RecordingWindowOps` is why an AT action that opens a window (a menu item, a
"New window" button) never crashes the headless server: instead of panicking
on `open_window`, it records the request and returns a synthetic id.

## Run snippets

```text
# Headless MCP server (CI / agent test-authoring):
teksilo-automation-mcp --headless

# Drive a live running app (after `install_automation_bridge_in_debug()`):
teksilo-automation-mcp --connect <sock> --token <uuid>

# Live-bridge smoke test (needs a display):
cargo run -p automation_bridge_smoke
#   — or keep it alive for an external client:
cargo run -p automation_bridge_smoke -- --serve
```

## Testing

- **Toolkit** (`teksilo-automation`): unit tests driven through `execute()`,
  each validating the produced `TreeUpdate` with the real `accesskit_consumer`.
- **MCP conformance** (`teksilo-automation-mcp`): the 24-tool router, the
  async-handler ⇄ tree-thread marshaling, and a screenshot that decodes to PNG
  magic bytes (skipped when no GPU).
- **Golden screenshots** behind the `golden-tests` feature
  (`cargo test -p teksilo-automation-mcp --features golden-tests`), inline
  per-channel pixel compare with tolerance ≤ 2, `UPDATE_GOLDENS=1` to refresh.
- **Bridge round-trip** (`examples/automation_bridge_smoke`): a real app +
  `install_automation_bridge_in_debug()`, an in-process client running
  `snapshot → invoke → re-snapshot` and asserting the `0600` socket.

See also: [Accessibility overrides](accessibility-overrides.md),
[Scene accessibility](teksilo-scene-a11y.md).
