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
live `--attach` mode (debug builds) drives your actual app, while the headless
mode is the toolkit's CI harness / a build-your-own-harness kit (see below).

## Two modes

| Mode | Command | What it drives |
| --- | --- | --- |
| **Headless** (default) | `teksilo-automation-mcp --headless` | A built-in demo app owned entirely in-process on a dedicated thread. No display, GPU daemon, or AT layer needed. The right mode for CI and agent test-authoring. |
| **Live (attach)** | `teksilo-automation-mcp --attach` | A *running* app that opted into the debug-only in-app bridge, found from the descriptor it publishes. The agent drives the real window the user sees. `--connect <endpoint> --token <uuid>` names one by hand. |

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
- **Use the live `--attach` mode** below — the turnkey "drive my real app"
  path today (needs a display and a running debug build).

### Live (attach)

A debug build opts in with one line:

```rust
use teksilo::prelude::*;

TeksiloAppBuilder::new()
    .theme(intui::light())
    .install_automation_bridge_in_debug()   // debug-only; a no-op in release
    .initial_window(/* ... */)
    .run();
```

On startup it binds this platform's private endpoint, publishes a small
**endpoint descriptor** naming it, and prints how to attach:

```text
teksilo-automation: bridge endpoint = /run/user/1000/teksilo-automation/12345.d/s
teksilo-automation: descriptor = /run/user/1000/teksilo-automation/12345.json
TEKSILO_AUTOMATION_TOKEN=8f3c…
teksilo-automation: attach with `teksilo-automation-mcp --attach-pid 12345` (or --connect /run/user/1000/teksilo-automation/12345.d/s --token 8f3c…)
```

On Windows the endpoint is a named pipe and the descriptor lives under
`%LOCALAPPDATA%`, but nothing else changes:

```text
teksilo-automation: bridge endpoint = \\.\pipe\teksilo-automation-12828
teksilo-automation: descriptor = C:\Users\you\AppData\Local\Teksilo\teksilo-automation\12828.json
```

Attach with any of:

```sh
teksilo-automation-mcp --attach              # the newest live app
teksilo-automation-mcp --attach-pid 12345    # one specific process
teksilo-automation-mcp --list                # what is live right now
teksilo-automation-mcp --connect <endpoint> --token <uuid>   # explicit escape hatch
teksilo-automation-mcp --help                # usage
teksilo-automation-mcp --version             # the binary's version
```

`--token` may be omitted when `$TEKSILO_AUTOMATION_TOKEN` holds it — which is
the form the startup banner prints it in, so the line above can be exported
verbatim. A value-taking flag whose value is missing is an error, not a shrug:
`--connect` with nothing after it used to fall through and start the *demo*
server while the caller believed it was driving their app.

`--attach` reads the descriptor, so the same command works on all three
platforms and there is nothing to copy out of stderr. It and `--list` probe
each descriptor before offering it, and remove only one with *nothing listening
behind it* — left by an app that exited without unwinding (`exit()`, a crash, a
kill). A bridge that is merely busy is kept; the probe answers in three states
for exactly that reason (see the limitations below).

Each rmcp tool handler writes the op to the endpoint and reads one reply; the
in-app bridge thread reads it and posts an `AutomationPayload` (carrying a
`Send` reply channel) through the existing `AppEvent::External` path, and the
winit **main thread** runs the op against the real window — the settle runs
synchronously on the main thread, never across a frame boundary.

If the main thread does not answer within 15 s — it is inside a native modal
loop (a file dialog, menu tracking, a window drag) — the bridge replies
`BRIDGE_TIMEOUT` and keeps serving, rather than blocking forever and holding
the single connection slot for the life of the process.

#### The endpoint descriptor

```jsonc
// <runtime dir>/teksilo-automation/<pid>.json — 0600, or owner-only by ACL
{
  "version": 1,
  "pid": 12345,
  "transport": "unix",              // or "named_pipe"
  "address": "/run/user/1000/teksilo-automation/12345.d/s",
  "token": "8f3c…",
  "app": "widget-catalog",
  "started_unix_ms": 1772000000000
}
```

| OS | Runtime directory | Endpoint |
| --- | --- | --- |
| Linux | `$XDG_RUNTIME_DIR` if set, else `$TMPDIR` | Unix socket, `0700` dir + `0600` socket |
| macOS | `$XDG_RUNTIME_DIR` if set, else `$TMPDIR` (per-user, per-boot) | Unix socket, `0700` dir + `0600` socket |
| Windows | `%LOCALAPPDATA%\Teksilo` | Named pipe with an owner-only DACL |

The Unix rows are one rule, not two: `$XDG_RUNTIME_DIR` is honoured wherever
it is set, and only when it is not does the directory fall back to
`std::env::temp_dir()` — `$TMPDIR`, which on Darwin is the per-user per-boot
`/var/folders/…/T/`. macOS never sets `$XDG_RUNTIME_DIR` itself, but a nix
shell, a container, or a remote session can, and then the descriptor is there
rather than under `$TMPDIR`. Read the path out of the descriptor (or out of
`--list`) rather than reconstructing it.

## Install

Add the `automation` feature to the umbrella crate (debug-only by design):

```toml
[dependencies]
teksilo = { version = "0.9", features = ["automation"] }
```

`install_automation_bridge_in_debug()` is gated on `debug_assertions`: a
**release** build with the feature on still contains no socket, token, or
bridge — the method is the identity. The GUI-free DTO toolkit is available as
`teksilo::automation` for writing harnesses against the same protocol.

The server binary builds from `cargo build -p teksilo-automation-mcp`.

## Tool surface (27 tools)

Every tool that changes the UI accepts an optional `settle` argument (see the
settle model below), and so does `screenshot` — it is catalogued non-mutating,
but it settles before capturing, because a PNG of a half-run animation answers
no question anyone asked. The two exceptions to the pattern are `advance_clock`,
which takes only `millis` because it *is* the clock op, and the read-only query
tools — `snapshot_tree`, `read_node`, `layout_tree`, `inspect_node`,
`find_node`, `assert_node`, `list_windows`, `get_overlays`, `get_shortcuts`,
`list_live_regions`, `pull_announcements` — which observe without touching the
tree. Every tool's parameters are `deny_unknown_fields`, so sending `settle`
where it is not accepted is a hard error, not a silent no-op; that is
deliberate, since the alternative is a script that believes it settled and
never did.

Node ids come from `snapshot_tree` / `find_node` and are the raw AccessKit
`NodeId` values — derived deterministically from the widget id. An id is
stable for the **lifetime of the widget instance** (across
relayout, repaint, theme, and locale changes, which mutate widgets in place),
but a *structural rebuild* that destroys and recreates the widget — a
data-model change, a `Switcher` swap, a `Rebuild`-level binding — allocates a
new id. So caching an id is the payoff over an OS handle for *in-place*
changes; after a structural change, re-`find_node` (by role/label, which
carries the usual label fragility) rather than reuse a possibly-stale id.

**Query** — `snapshot_tree {window_id?, max_depth?}`, `read_node {node}`,
`find_node {role?, label?}`, `assert_node {node, kind, value?/flag?}`,
`list_windows {}`

`max_depth` bounds the walk from the root, and on a real app it is usually the
difference between a reply you can read and one you cannot: take a shallow
snapshot first, then descend from a node you picked out of it.

`assert_node`'s `kind` is one of `role_equals`, `label_equals`,
`label_contains`, `value_equals` (each taking the expected string in `value`),
`toggled`, `expanded`, `selected`, `disabled` (each taking the expected bool in
`flag`), or `exists` / `focused`, which take neither. An unrecognised `kind` is
refused rather than defaulted — the same reason every op's parameters are
`deny_unknown_fields`: a probe that asks a question the server does not
understand must not be quietly answered with a different one.

A failed `assert_node` is a **tool error** (`isError = true`) carrying
`code: "ASSERTION_FAILED"` and a message with the actual and expected values.
A node reference that names nothing comes back as `code: "NOT_FOUND"` instead,
because "the button is not focused" and "there is no such button" are different
bugs and a probe that cannot tell them apart chases the wrong one. The one
exception is `kind: "exists"`, where a missing node is a genuine answer and so
reports `ASSERTION_FAILED`.

The decision is made in the toolkit, not in this server, so the socket bridge
and any direct `execute` caller behave identically.

**Layout / geometry** — `layout_tree {max_depth?, include_debug?}`,
`inspect_node {node}`

**Drive (AT actions)** — `invoke_action`, `focus_node`, `set_value`,
`expand`, `collapse`, `scroll`

`scroll` takes `ctrl` / `shift` / `alt` / `meta` / `command` alongside `dx` /
`dy`, all defaulting to false. A modifier-held wheel is a *different gesture*
from a plain one — `WidgetEvent::Scroll` carries modifiers precisely so an app
can implement Ctrl-wheel-to-zoom — so a probe for such a feature must be able
to send one, not merely a bare wheel.

**Synthetic input** — `inject_pointer`, `right_click`, `inject_key`,
`type_text`, `type_ime`, `drag_node`

`inject_pointer` and `inject_key` take the same five modifier flags, and
**`command` is the one to reach for whenever the chord means "the
accelerator"** — save, copy, select-all, accelerator-click to extend a
selection. It is Control on Windows and Linux and Command (⌘) on macOS, which is
what a shortcut *declared* `Ctrl+S` resolves to there (see
[Actions, Intents & Shortcuts](shortcut-intent-action.md)). So on macOS
`ctrl: true` injects a key that matches no binding — and *reports success*,
because the key really was injected, it just bound to nothing — while
`command: true` is the same script on all three platforms. `ctrl` stays literal
Control everywhere, for the chords that genuinely are Control on macOS too
(Ctrl+Tab, a terminal's Ctrl+C).

**Introspection** — `get_overlays`, `get_shortcuts`, `list_live_regions`,
`pull_announcements`

**Time / settle** — `advance_clock`, `settle`, `wait_for_condition`

**Visual** — `screenshot` (returns an MCP image content block)

`snapshot_tree` returns `{ root, focus, nodes: [SemanticNode…] }`, where each
`SemanticNode` carries `id`, `role`, `label`, `value`, `description`,
`toggled`, `expanded`, `selected`, `level`, `disabled`, `focused`, `live`,
`numeric_value`, `bounds`, `actions`, and `children`. Optional fields are
omitted from the JSON when absent, so a node is as small as what it actually
declares.

Two of those are easy to overlook and load-bearing. `description` is the
accessible description — the sentence an AT reads after the name — and it is
where a whole tier of the tooltip system lives: a plain tooltip is never
auto-shown on focus, so its text reaches a screen reader *only* as the
described control's description, and a probe that reads only `label` cannot
tell a wired-up hint from a missing one. `level` is a tree row's 1-based
hierarchy depth; a client aiming a synthetic pointer at a row's disclosure
chevron cannot compute its x without it, because the chevron sits one indent
step per level in from the row's leading edge.

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
| `settle_timeout_ms` | `500` | The budget. For a settle it is a hard **wall-clock** cap; exceeding it ends the settle (the live bridge reports `SETTLE_TIMEOUT`). For `wait_for_condition` the same field is a **simulated-time** budget — see below. |

The settle loop is **simulation-clock-driven** (`tick_animations` doesn't wait
on VSync or OS events), so it can't deadlock — it progresses to quiescence or
the cap. `wait_for_condition` polls `snapshot → predicate` on the same clock
until a `node_exists` / `node_value` / `node_gone` / `at_version_at_least`
condition holds, else `WAIT_TIMEOUT`. Those are the values of the `kind` tag a
client sends, not Rust variant names: `node_exists` takes `role?` / `label?`,
`node_value` takes `node` + `expected`, `node_gone` takes `node`, and
`at_version_at_least` takes `version`.

Its budget is spent as **simulated 16 ms frames**, not as wall clock, so the
same wait resolves identically on every host. A wall-clock bound made the
outcome depend on the platform's timer granularity: the loop slept 1 ms per
poll, and a 1 ms sleep costs up to 15.6 ms on Windows, so the same budget bought
roughly a fifteenth of the frames and a fifteenth of the simulated time — a wait
that passed on Linux timed out on Windows, with nothing in the reply to say why.
Nothing else can move the tree while the loop runs (it is `!Send` and this
thread owns it), so simulated frames are the only thing that can make the
predicate true, and the sleep bought no progress at all. A wall-clock backstop
remains at **1×** the budget (floor 250 ms), and only so a pathological tree —
one that rebuilds unboundedly every frame — cannot spin forever; it must never
be what ends an ordinary wait, which is why it is not a larger multiple. The
live bridge clamps `settle_timeout_ms` to 2 s precisely so no op can freeze the
winit main thread for longer, and a 10× backstop quietly turned that into 20 s —
past even the bridge's own 15 s reply deadline, so the client was told its
request had timed out while the UI stayed frozen.

### Live regions & announcements

Teksilo has no OS AT layer in headless mode, and no in-process way to observe
what the platform *spoke*. So the `WidgetTree` diffs the live (`Live::Polite` /
`Live::Assertive`) nodes of each freshly-built `TreeUpdate` and records the
changes into a ring buffer. `pull_announcements { since_seq }` drains it — a
faithful, in-process model of the live-region stream. `list_live_regions`
reports the live nodes themselves.

## Screenshots

`screenshot` renders the window (or, with `node`, that node's bounds) to a PNG
and returns it as an MCP **image content block**, alongside a metadata block:

```jsonc
{ "width": 1600, "height": 1200, "scale": 2.0, "warnings": [] }
```

**`scale` is not decoration.** Pixel dimensions are physical, and a live window
on a HiDPI display is not laid out at that size: an 800×600 logical window
captures as 1600×1200 at `scale: 2.0`. Every coordinate elsewhere in the
toolkit — node `bounds`, `inject_pointer` — is *logical*, so without `scale` a
caller cannot relate a pixel it can see to a point it can click, and a script
written against one display silently mis-aims on another. Headless always
reports `scale: 1.0`, where the two coincide.

- **Headless:** an offscreen `RENDER_ATTACHMENT | COPY_SRC` texture in a fixed
  `Rgba8UnormSrgb` format, rendered via the test renderer, read back,
  PNG-encoded — identical bytes on every backend.
- **Live:** `PlatformWindow::capture_offscreen` renders the live frame into an
  offscreen texture in the window's own surface format (the swapchain texture
  lacks `COPY_SRC`), swizzling BGRA→RGBA as needed — that branch is the one
  DX12 and Metal take. The bridge base64-encodes the PNG and the attach client
  rehydrates it to an image block.

Adapter selection is a search, not a single request: the preferred adapter
first, then an explicit software fallback if it yields no device. A host can
enumerate an adapter it cannot actually open — a VM's OpenGL driver is the
common case — while a perfectly good software device (DX12 WARP, llvmpipe) sits
behind `force_fallback_adapter`. Treating the first failure as fatal reported
"no GPU" on machines that have one, which is what made screenshots unavailable
on GPU-less Windows hosts and CI runners. When *nothing* yields a device the
tool returns `GPU_UNAVAILABLE`; when a device is lost mid-readback it returns
`GPU_READBACK_FAILED`. Both are non-fatal and the session survives.

That device is opened **once per process** and shared by every offscreen
renderer. Two D3D12 WARP devices rasterizing at the same time fault inside
`d3d10warp.dll` — Microsoft's software rasterizer, which is what a GPU-less
Windows host and the CI runners actually use — so a device per caller made any
two concurrent screenshots a coin-flip on the process surviving, with the crash
landing in WARP rather than anywhere we could catch it. Each caller still gets
its own `Renderer`, and therefore its own glyph and path atlases, so nothing
leaks between them.

**WebView blind spot:** a native `WebView` subview composites *on top of* the
wgpu surface and is invisible to the readback (a transparent hole). When the
AT tree contains a `WebView` node the `screenshot` reply adds
`warnings: ["webview_hole_possible"]`. There is no platform-capture workaround
in scope.

## Security model

The live bridge is defence-in-depth:

- **Feature- and debug-gated.** Every item that binds the endpoint, generates
  the token, or spawns the bridge thread is `#[cfg(debug_assertions)]`; a
  release build's `install_automation_bridge_in_debug()` is the identity. A
  shipped release binary contains no endpoint, token, or bridge.
- **A private local endpoint, never TCP.** Each platform uses the mechanism it
  actually provides, and each refuses to bind rather than fall back to a weaker
  mode:
  - **Linux / macOS** — a Unix-domain socket in a per-process directory created
    `0700` by `mkdir`'s own mode (so it is never briefly world-reachable — that
    ordering is what closes the bind→chmod TOCTOU), with the socket itself
    `0600`. Removed on startup and on bridge-thread exit. `sockaddr_un::sun_path`
    is 104 bytes on macOS, and Darwin's `$TMPDIR` already spends about half of
    that, so the path is measured against a 100-byte ceiling before use and a
    short `/tmp/tka-<pid>` is tried if the preferred one will not fit —
    overflow is not truncation, `bind` simply fails. The fallback keeps the same
    per-process `0700` discipline, so it is a shorter path and not a weaker one;
    and its parent runs the same not-taken-on-trust check as the descriptor
    directory two bullets down, which a stock `/tmp` fails — world-writable,
    and not something a non-root process may tighten — rather than settling
    into a shared home. It is a guard against an unbindable path, not a routine
    location.
  - **Windows** — a named pipe with an explicit owner-only DACL
    (`D:P(A;;GA;;;<user SID>)`). This is not the default: a pipe created with a
    null security descriptor grants read access to *Everyone and the anonymous
    account*, so the descriptor is always built from the process token's user
    SID. `PIPE_REJECT_REMOTE_CLIENTS` is set in the same `dwPipeMode` bitmask as
    a second layer against the SMB path — it does **not** keep out other local
    users, so it is never a substitute for the DACL.
- **An owner-only endpoint descriptor.** It carries the token, so *how* it is
  written is part of the boundary, not housekeeping. On Unix the staging file is
  opened with `create_new` **and the `0600` mode in the same `open`** — never
  create-then-`chmod`, which would leave the token at the umask default (0644 as
  a rule) for the window in between: the same TOCTOU the socket's `mkdir` mode
  exists to close. `create_new` also refuses to follow a symlink planted at the
  staging path, so a writable parent cannot redirect the token elsewhere. The
  directory's owner is then checked **before the token reaches the disk** — the
  empty `0600` file just created is itself proof of our uid, which is how the
  check is made with no `libc` — and only then is the file written and renamed
  into place, so a reader never sees it half-written. On Windows both the
  directory and the file inherit the per-user `%LOCALAPPDATA%` ACL.
- **A descriptor directory that is not taken on trust.** An already-existing one
  must be a real directory — `symlink_metadata`, so a symlink pointing at a
  directory we *do* own is still refused, because the target is not what we
  would be protecting — and is tightened to `0700` if anything else can reach
  it. `$XDG_RUNTIME_DIR` is per-user by spec and Darwin's `$TMPDIR` is per-user
  per-boot, but the fallback for a Unix with neither is the **shared `/tmp`**,
  where another local user can create `teksilo-automation/` first. Accepting
  `AlreadyExists` blindly there would publish a token-bearing descriptor into a
  directory somebody else controls, and let the listing pick up descriptors they
  planted. That fallback is the case this bullet and the one above it are
  written for.
- **Single connection at a time**, single in-flight request, with a 16 MiB cap
  on a request frame (256 MiB on a reply, which can carry a screenshot). A reply
  too large to frame is replaced by a typed error rather than desyncing the
  stream.
- **A handshake bounded end to end, not just per read.** The transport gets a
  10 s read deadline *and* the token read gets a 10 s deadline for the handshake
  **as a whole**, checked between bytes. Both are needed. The transport's
  deadline is per-`read`, and the token is read one byte at a time — a buffered
  reader would pull the first *frame* into its buffer along with the token line,
  and `try_clone`-ing the connection to recover has no clean equivalent on a
  Windows pipe — so on its own a per-read bound lets a peer drip one byte just
  under the timeout and hold the single connection slot for the 512-byte token
  cap × 10 s, which is the exact denial the deadline exists to prevent. Once the
  peer is authenticated the deadline is cleared again: requests arrive
  sporadically over a long-lived connection.
- **Per-process UUID token**: the client must send the token as the first line
  or the connection is dropped. Compared without early-exit, so the comparison
  leaks neither length nor prefix.
- **Bounded main-thread settle.** Because the live settle runs on the winit
  main thread, the bridge clamps `max_anim_frames ≤ 120` and
  `settle_timeout_ms ≤ 2000` so no op can freeze the UI for more than ~2 s.
  Headless keeps the caller's values (no UI to freeze).
- **A bridge that always answers.** The reply wait is bounded (15 s,
  `BRIDGE_TIMEOUT`), so a main thread stuck in a native modal loop costs one
  request rather than the connection slot for the life of the process.

The threat model is a *trusted local user*: the OS refuses the connection to
anyone else, and the token is a second gate. The token is printed to stderr and
stored in the descriptor, so anyone who can read the app's stderr (or
`/proc/<pid>/environ` when it's pinned) can drive the UI; this is a dev-tool
stance, kept out of production by the debug gate above. **Regression guard:**
the debug-only banner string only exists in the gated `spawn_bridge_thread`, so
a release binary with the feature on must not contain it. The `test-automation`
CI job runs exactly this, on all three OSes, against
`automation_bridge_smoke` — the cheapest binary that actually wires the bridge:

```sh
banner="teksilo-automation: bridge endpoint"

cargo build -p automation_bridge_smoke
grep -qa "$banner" target/debug/automation_bridge_smoke \
  || { echo "the canary string is absent from the DEBUG build"; exit 1; }

cargo build --release -p automation_bridge_smoke
! grep -qa "$banner" target/release/automation_bridge_smoke \
  || { echo "BRIDGE LEAKED INTO RELEASE"; exit 1; }
```

Both directions matter. Absent-from-release is the assertion, but a grep for a
string that no longer exists anywhere passes for the wrong reason, so the debug
build is checked to still *contain* it — otherwise renaming the banner would
retire the canary in silence and nobody would find out until a release shipped
a live bridge. (On Windows both paths take a `.exe` suffix.)

Headless mode has no endpoint at all.

## Documented limitations

- **WebView pixels** in screenshots (compositor hole — warned, not captured).
- **Release-build automation**: debug-gated by design.
- **Golden screenshots are Linux-only**: font rasterisation differs per OS, so
  cross-platform screenshot assertions are structural (size, scale, non-blank),
  not pixel-exact.
- **One client at a time.** The bridge serves strictly one connection and one
  in-flight request. A second client is not refused: it waits — in the kernel
  backlog on Unix, on the second pipe instance on Windows (the pipe allows two
  so a client arriving the instant the previous one leaves still finds something
  listening) — until the accept loop comes back round. On Windows a further
  client that finds no free instance retries for 5 s and then fails with "the
  automation bridge is already serving another client"; on Unix the wait is
  unbounded.
- **A busy bridge is still a live bridge.** A probe answers in three states, not
  two: `Live` (a bridge answered), `Busy` (something is listening but could not
  take us — the single slot is occupied, or the server is between accepts) and
  `Dead` (an unambiguous nothing-is-there: no socket or pipe, or the
  `ECONNREFUSED` of the classic stale-socket corpse). `--list` and `--attach`
  prune **only** on `Dead`, because a caller acts on that answer by *deleting
  the descriptor*: conflating busy with dead meant a read-only-looking `--list`
  permanently unregistered a perfectly healthy app because somebody else got
  there first, after which `--attach-pid` could never find it again for the life
  of the process.
- **macOS App Sandbox**: a sandboxed build cannot bind outside its container.
  Structurally moot (the bridge is debug-gated and a sandboxed release build
  can never contain it), but worth knowing.
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
| `teksilo-automation` | GUI-free toolkit: DTOs, `execute`, `RecordingWindowOps`, the tool catalog, and `wire` — the framing, token handshake and endpoint descriptor, shared verbatim by both ends. Mirrors `teksilo-data`'s core-only-peer design. |
| `teksilo-automation-mcp` | The rmcp server binary (`--headless` / `--attach`) + offscreen screenshots. `tokio` / `rmcp` are confined here. |
| `teksilo-app` (`automation` feature) | The debug-only in-app bridge: policy only — token, accept loop, main-thread routing. No `cfg` on the OS, no async runtime in the framework. |
| `teksilo-platform` (`automation` feature) | `automation_transport` — `bind`/`accept`/`connect` and their per-OS access control (Unix socket, Windows named pipe). The live-window readback, `PlatformWindow::capture_offscreen`, is **not** behind the feature: it is an ordinary method on the window, usable by anything that wants an offscreen frame. |

The split between the last two is the same one the crate already draws for
`external_dnd` and `native_menu`: the OS primitive lives in `teksilo-platform`
behind a backend trait, and the subsystem that uses it stays platform-agnostic.
The protocol lives one level lower still, in `teksilo-automation`, because both
ends speak it and it needs no OS at all — which is what makes it exhaustively
testable over an in-memory buffer on every platform.

`RecordingWindowOps` is why an AT action that opens a window (a menu item, a
"New window" button) never crashes the headless server: instead of panicking
on `open_window`, it records the request and returns a synthetic id.

## Run snippets

```text
# Headless MCP server (CI / agent test-authoring; no display or GPU daemon needed —
#  screenshots try a real adapter, then a software one, and return GPU_UNAVAILABLE
#  only if neither yields a device):
teksilo-automation-mcp --headless

# Drive a live running app (after `install_automation_bridge_in_debug()`):
teksilo-automation-mcp --attach              # the newest one
teksilo-automation-mcp --attach-pid 12345    # a specific process
teksilo-automation-mcp --list                # what is live right now

# Live-bridge smoke test (needs a display; works on Linux, macOS and Windows):
cargo run -p automation_bridge_smoke
#   — or keep it alive for an external client:
cargo run -p automation_bridge_smoke -- --serve
```

## Testing

- **Toolkit** (`teksilo-automation`): unit tests driven through `execute()`,
  each validating the produced `TreeUpdate` with the real `accesskit_consumer`.
- **Wire protocol** (`teksilo-automation::wire`): framing round-trips, empty and
  truncated frames, oversize refusal on both read and write, the token-line cap,
  and that the handshake leaves the first frame intact — all in memory, so the
  whole protocol is covered on every platform with no socket. Plus, on Unix, that
  the published descriptor **and its staging file** are `0600` from the moment
  they exist: an after-the-fact permissions assertion is one a
  create-then-`chmod` would also pass, so the staging file is what the test
  actually looks at.
- **Transport** (`teksilo-platform::automation_transport`): bind → connect →
  round-trip over whatever this platform actually uses, plus the read deadline
  (it must report `TimedOut` and leave the stream usable, not kill it). On Unix
  it also asserts the owner-only permissions — `0600` socket in a `0700`
  directory — and that dropping the listener takes the per-process directory
  with it. On Windows the coverage is narrower and worth stating plainly: that
  the process token's SID resolves at all (if it does not, `bind` must fail
  rather than fall back to the permissive default descriptor) and that the
  security descriptor built from it is non-null and non-inheritable. The DACL's
  own ACEs are **not** walked back and asserted, so "owner-only" rests on the
  construction, not on a test. What *is* tested there is the instance
  behaviour a busy bridge depends on: that a second pipe instance can exist
  while the first is in use, and that a client can connect immediately after
  the previous one is released.
- **MCP conformance** (`teksilo-automation-mcp`): the tool router, the
  async-handler ⇄ tree-thread marshaling, and a screenshot that decodes to PNG
  magic bytes (skipped when no GPU).
- **Server end-to-end** (`teksilo-automation-mcp/tests/stdio_smoke.rs`): spawns
  the real binary and speaks JSON-RPC to it — argument parsing, the rmcp stdio
  transport, the `!Send` tree thread and the serde round-trip. Needs no display
  and no GPU, so it runs on all three OSes in the ordinary `cargo test`.
- **Golden screenshots** behind the `golden-tests` feature
  (`cargo test -p teksilo-automation-mcp --features golden-tests`), inline
  per-channel pixel compare with tolerance ≤ 2, `UPDATE_GOLDENS=1` to refresh.
  Linux-only by policy: font rasterisation differs per OS, so the cross-platform
  assertions are structural rather than pixel-exact.
- **Bridge round-trip** (`examples/automation_bridge_smoke`): a real app +
  `install_automation_bridge_in_debug()`, a client that discovers the endpoint
  from the published descriptor and runs
  `snapshot → invoke → re-snapshot → screenshot`, asserting the descriptor and
  socket are owner-only. Run per-OS in CI by the `test-automation` job, which
  also carries the release canary.

See also: [Accessibility overrides](accessibility-overrides.md),
[Scene accessibility](teksilo-scene-a11y.md).
