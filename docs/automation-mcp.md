<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Automation MCP — Drive a Bastyde App from an Agent

A Bastyde app exposes a rich semantic surface — the **AccessKit accessibility
tree** — plus an **AT-action dispatch path**, both of which are queryable and
drivable **in-process, without the OS accessibility layer**. The
`bastyde-automation-mcp` server turns that latent capability into a
[Model Context Protocol](https://modelcontextprotocol.io) (MCP) server so an
AI agent (or any MCP client) can **observe** (semantic tree + screenshots) and
**drive** (AT actions + synthetic input) a Bastyde app.

This is the capability an agent can't get otherwise: the `TreeUpdate` lives
inside private platform state with no external channel except the OS AT layer;
the AT-action channel is OS-AT-only; headless operation needs no display
server; and `WidgetId`-stable node identity beats fragile OS handles. It
**complements** (does not replace) a real screen-reader OS smoke test — use it
for deterministic CI / agent test-authoring, and (debug-only) against a live
running app.

## Two modes

| Mode | Command | What it drives |
| --- | --- | --- |
| **Headless** (default) | `bastyde-automation-mcp --headless` | A built-in demo app owned entirely in-process on a dedicated thread. No display, GPU daemon, or AT layer needed. The right mode for CI and agent test-authoring. |
| **Live (connect)** | `bastyde-automation-mcp --connect <sock> --token <uuid>` | A *running* app that opted into the debug-only in-app bridge. The agent drives the real window the user sees. |

Both speak MCP over **stdio**.

### Headless

```text
bastyde-automation-mcp --headless
```

A dedicated `std::thread` owns a `HeadlessApp` (a small demo UI: a heading,
two buttons, a text field, a checkbox). The async rmcp handlers marshal `Send`
DTOs to that thread and await a reply; the `!Send` `WidgetTree` never leaves
it. Screenshots render offscreen on the tree thread via `pollster::block_on`
(reusing `bastyde_render::test_support::create_test_renderer` — the same
offscreen path the widget previewer's PNG export uses).

### Live (connect)

A debug build opts in with one line:

```rust
use bastyde::prelude::*;

BastydeAppBuilder::new()
    .theme(intui::light())
    .install_automation_bridge_in_debug()   // debug-only; a no-op in release
    .initial_window(/* ... */)
    .run();
```

On startup it prints the socket path and token to stderr:

```text
bastyde-automation: bridge socket = /run/user/1000/bastyde-automation-12345.sock
BASTYDE_AUTOMATION_TOKEN=8f3c…
bastyde-automation: connect with `bastyde-automation-mcp --connect /run/user/1000/bastyde-automation-12345.sock --token 8f3c…`
```

Then point the server at it:

```text
bastyde-automation-mcp --connect /run/user/1000/bastyde-automation-12345.sock --token 8f3c…
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
bastyde = { version = "0.6", features = ["automation"] }
```

`install_automation_bridge_in_debug()` is gated on `debug_assertions`: a
**release** build with the feature on still contains no socket, token, or
bridge — the method is the identity. The GUI-free DTO toolkit is available as
`bastyde::automation` for writing harnesses against the same protocol.

The server binary builds from `cargo build -p bastyde-automation-mcp`.

## Tool surface (24 tools)

Mutating tools accept an optional `settle` argument (see the settle model
below); query tools don't. Node ids come from `snapshot_tree` / `find_node`
and are the raw AccessKit `NodeId` values — derived deterministically from the
widget id, so they survive rebuilds.

**Query** — `snapshot_tree`, `read_node`, `find_node`, `assert_node`,
`list_windows`

**Drive (AT actions)** — `invoke_action`, `focus_node`, `set_value`,
`expand`, `collapse`, `scroll`

**Synthetic input** — `inject_pointer`, `inject_key`, `type_text`,
`type_ime`, `drag_node`

**Introspection** — `get_overlays`, `get_shortcuts`, `list_live_regions`,
`pull_announcements`

**Time / settle** — `advance_clock`, `settle`, `wait_for_condition`

**Visual** — `screenshot` (returns an MCP image content block)

`snapshot_tree` returns `{ root, focus, nodes: [SemanticNode…] }`, where each
`SemanticNode` carries `id`, `role`, `label`, `value`, `toggled`, `expanded`,
`selected`, `disabled`, `focused`, `live`, `numeric_value`, `bounds`,
`actions`, and `children`.

### Multi-window routing

Every tool accepts an optional `window_id` (the `BastydeWindowId` raw value
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

Bastyde has no OS AT layer in headless mode, and no in-process way to observe
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

- **Feature- and debug-gated.** It exists only with the `automation` feature
  **and** `debug_assertions`. A shipped release binary contains none of it.
- **Unix-domain socket only** (never TCP), mode `0600`, PID-unique path under
  `$XDG_RUNTIME_DIR`, removed on startup and on bridge-thread exit.
- **Single connection at a time**, single in-flight request.
- **Per-process UUID token**: the client must send the token (printed to
  stderr, or pinned via `BASTYDE_AUTOMATION_TOKEN`) as the first line, or the
  connection is rejected.

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
core function does the work, in the GUI-free `bastyde-automation` crate:

```rust
bastyde_automation::execute(
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
| `bastyde-automation` | GUI-free toolkit: DTOs, `execute`, `RecordingWindowOps`, the tool catalog. Mirrors `bastyde-data`'s core-only-peer design. |
| `bastyde-automation-mcp` | The rmcp server binary (`--headless` / `--connect`) + offscreen screenshots. `tokio` / `rmcp` are confined here. |
| `bastyde-app` (`automation` feature) | The debug-only in-app bridge — `std::os::unix::net` + the existing `send_external` path; no async runtime in the framework. |
| `bastyde-platform` | `PlatformWindow::capture_offscreen` — the live-window readback. |

`RecordingWindowOps` is why an AT action that opens a window (a menu item, a
"New window" button) never crashes the headless server: instead of panicking
on `open_window`, it records the request and returns a synthetic id.

## Run snippets

```text
# Headless MCP server (CI / agent test-authoring):
bastyde-automation-mcp --headless

# Drive a live running app (after `install_automation_bridge_in_debug()`):
bastyde-automation-mcp --connect <sock> --token <uuid>

# Live-bridge smoke test (needs a display):
cargo run -p automation_bridge_smoke
#   — or keep it alive for an external client:
cargo run -p automation_bridge_smoke -- --serve
```

## Testing

- **Toolkit** (`bastyde-automation`): unit tests driven through `execute()`,
  each validating the produced `TreeUpdate` with the real `accesskit_consumer`.
- **MCP conformance** (`bastyde-automation-mcp`): the 24-tool router, the
  async-handler ⇄ tree-thread marshaling, and a screenshot that decodes to PNG
  magic bytes (skipped when no GPU).
- **Golden screenshots** behind the `golden-tests` feature
  (`cargo test -p bastyde-automation-mcp --features golden-tests`), inline
  per-channel pixel compare with tolerance ≤ 2, `UPDATE_GOLDENS=1` to refresh.
- **Bridge round-trip** (`examples/automation_bridge_smoke`): a real app +
  `install_automation_bridge_in_debug()`, an in-process client running
  `snapshot → invoke → re-snapshot` and asserting the `0600` socket.

See also: [Accessibility overrides](accessibility-overrides.md),
[Scene accessibility](bastyde-scene-a11y.md).
