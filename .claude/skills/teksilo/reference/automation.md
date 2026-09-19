<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Driving and testing a Teksilo app

Teksilo ships an automation layer that lets an agent or a CI harness **observe**
the running app (the live accessibility tree, the full widget/layout tree with
bounds, screenshots) and **drive** it (accessibility actions plus synthetic
pointer / key / IME input) **in-process, without the OS accessibility layer**.
Reach for it to author UI tests, reproduce a bug, debug layout, or exercise a
feature end to end.

Two ways in, and they are complements:

| Shape | What it is | Use it for |
|---|---|---|
| **MCP server** — `teksilo-automation-mcp` | An MCP server an agent talks to directly, tool by tool | Exploring, diagnosing, one-off "what does the app actually do here" questions |
| **Python probe** — `cargo teksilo probe` | A script that launches the app, attaches, drives it and asserts | Anything you want to re-run: a regression test, a de-risking experiment, a CI gate |

Verified against **teksilo 0.12.1**.

---

## 1. Wiring the app

Enable the `automation` feature on the `teksilo` dependency and add one builder
call:

```rust,ignore
TeksiloAppBuilder::new()
    .install_automation_bridge_in_debug()   // debug-only; a no-op in release
    // … the rest of the chain …
    .run();
```

Linux, macOS and Windows alike. On startup the app binds a private endpoint — a
`0600` Unix socket in a `0700` per-process directory, or on Windows a named pipe
with an owner-only DACL (a pipe's *default* descriptor grants read to Everyone,
so it is always built explicitly from the process token's SID) — and publishes an
**endpoint descriptor** at `<runtime dir>/teksilo-automation/<pid>.json`,
owner-only because it carries the token.

The bridge binds the endpoint, publishes the descriptor and spawns its accept
thread **before** printing any of it, so the endpoint is connectable the instant
a client reads the announce; no client needs a retry loop. A release build
contains no endpoint, no token and no bridge on any platform.

## 2. Attaching

Install the server matching the framework version your app pins:

```bash
cargo install teksilo-automation-mcp --version <the teksilo version your app pins>
```

Then:

```bash
teksilo-automation-mcp --attach              # the newest live app
teksilo-automation-mcp --attach-pid <pid>    # a specific one
teksilo-automation-mcp --list                # what is live right now
teksilo-automation-mcp --connect <endpoint> --token <uuid>   # name one by hand
```

Discovery goes through the descriptor, so nothing is scraped out of stderr and
`--attach` behaves identically on all three platforms. A descriptor outlives a
process that exits without unwinding, so `--list` / `--attach` prune by probing;
the probe is three-state and only a genuinely dead entry is unregistered, so a
healthy bridge whose single connection slot another client already holds is
reported busy rather than deleted.

**`--headless` does not drive your app.** It is a self-contained server that owns
a small **built-in demo** (a heading, two buttons, a text field, a checkbox) — the
toolkit's own conformance harness. It needs no display or GPU daemon and works
everywhere, which makes it useful for checking that a client speaks the protocol
correctly, and useless for testing your UI. To drive *your* app headlessly, own
its `WidgetTree` on one thread and call `teksilo_automation::execute` per request
(the `teksilo-automation` crate is GUI-free). The turnkey path for a real app is
the live bridge above.

## 3. The tool catalog, by job

34 tools. On connect the server hands the client a "how to drive this app"
briefing plus a JSON schema per tool, so a capable agent can self-guide the
**snapshot → find node → act → settle → assert** loop. What follows is the map,
not a substitute for the schemas.

**Observe.** `snapshot_tree` / `find_node` / `read_node` give semantics: role,
label, value, toggled / expanded / **selected**, **`bounds {x, y, width, height}`**
(a widget's size lives here), and the `actions` a node supports. `assert_node`
turns an expectation into a result (a failed assert returns `isError`).
`inspect_node` returns one widget's full record including its `Debug` repr and
constructor parameters. `layout_tree` returns the **full** widget tree including
the layout primitives the accessibility tree prunes (`Padding` / `Expand` /
`FixedSize`), each with its bounds — position plus size is a widget's full
geometry, and this is the tool for size / overlap / off-screen / clipping
questions the semantic tree cannot answer. `list_windows` enumerates windows, and
**every** tool takes an optional `window_id`.

`screenshot {node?}` returns an image block plus a `{width, height, scale}`
metadata block. **Its pixels are physical; every other coordinate in this API —
node `bounds`, `inject_pointer` — is logical.** Divide by `scale` to click what
you can see. Headless is always `1.0`.

**Drive.** `invoke_action {node, action}` — **`action` is REQUIRED** (`click` /
`focus` / `expand` / `collapse` / `set_value` / `increment` / `decrement` /
`show_context_menu`); omitting it errors and changes nothing. Plus the shortcuts
`set_value` / `focus_node` / `scroll`, **`drag_node {to_node | to_x, to_y}`** for
drag-and-drop, `right_click {node}` to open a context menu, and the raw input
tools `inject_pointer` / `inject_key` / `type_text` / `type_ime`.

> **For an accelerator chord pass `command: true`, not `ctrl`.** `command` is the
> platform's primary accelerator (Control on Windows/Linux, ⌘ on macOS), which is
> what a shortcut *declared* `Ctrl+S` resolves to. `ctrl` stays literal Control
> everywhere — on macOS it injects a key that matches no binding **and still
> reports success**. Keep `ctrl` for chords that really are Control on every
> platform (Ctrl+Tab).

**Touch, pen and multi-pointer.** `inject_pointer` takes `kind` (`mouse` default
/ `touch` / `pen`); the kind reaches the hit test, the slop, the hover rules and
the arbitration, so a touch is a real touch and not a mouse at a point. `pen`
carries `pressure` (0..1) and `tilt` (`[x, y]` degrees). Unknown fields are
refused rather than defaulted — including `pressure` on a mouse.

`inject_touch_sequence` drives a whole multi-touch gesture in one call: steps
naming a finger **slot**, a phase (`down` / `move` / `up` / `cancel`), a point,
and how many simulated milliseconds to advance first — and it reports the
**arbitration after every step**: the frozen `touch_action`, every competitor
with its role and state, and the winner. Identities are minted by the framework,
so the reply says which id each slot got. A sequence that stops short of its `up`
leaves the finger down, which is how a live arbitration stays observable.

Gesture shorthands for the three timings that are easy to get wrong: `pinch` (two
fingers, span to span — both land before either moves, because the recogniser's
reference span is the distance between the landings), `fling` (one finger
released **while still moving**, handing a velocity to the scroller — a drag
latches on distance, so the duration is the whole difference), and `long_press`
(holds exactly the device's threshold, read off the active input profile, so the
script need not know the number).

`query_pointers` lists every live pointer with id, kind, position, pressure/tilt,
capture, frozen `touch_action`, competitors and winner — the way to learn the id
of a contact left down by anything other than `inject_touch_sequence`.
`cancel_pointer {id}` revokes a pointer the way the system does (a compositor
grab, a lost capture): **not** an up — no tap completes, and every widget working
on it is told. `set_density {compact | comfortable | touch}` switches the target
density; **it rebuilds every widget, so every node id captured before it is
dead** — re-snapshot afterwards. Setting the density it already has is a no-op
and keeps the ids.

**Timing — prefer this over `sleep`.** Mutating tools auto-settle, but for timed
UI (tooltips, debounced reactivity, animations) drive the **simulated** clock:
`settle.clock_millis` on any mutating call, `advance_clock {millis}`, `settle`,
or poll with `wait_for_condition` (`node_exists` / `node_value` / `node_gone` /
`at_version_at_least`). `wait_for_condition` spends its budget as simulated
frames rather than wall clock, so it resolves identically on every host.

**Accessibility extras.** `get_overlays` (open menus, popovers, dialogs),
`list_live_regions` + `pull_announcements {since_seq}` (the status text a screen
reader would speak — the way to assert that a **toast** fired), `get_shortcuts`.

## 4. Two rules that cost the most time when missed

**Node ids are stable for a widget's lifetime — not across a rebuild.** They
survive relayout, theme changes and locale changes. A **structural rebuild** (a
data-model change, a `Switcher` swap, a `Rebuild`-level binding, `set_density`)
allocates a new id. **Re-`find_node` after the tree structure changes**; never
reuse a cached id across one.

**Error results carry a stable `code` — branch on it, not just on `isError`.**

| Code | Meaning |
|---|---|
| `NOT_FOUND` | No such node. A real mistake. |
| `BAD_ARGUMENT` | Malformed or unknown argument. A real mistake. |
| `UNKNOWN_NAME` | No such tool. A real mistake. |
| `UNHANDLED_ACTION` | The node is real but nothing acted on the action — it advertises no such action, or its handler declined. **The message names the ones it *does* advertise; read those and re-call.** |
| `GPU_UNAVAILABLE` | Screenshot on a host with no usable adapter. Environmental. |
| `WAIT_TIMEOUT` | A `wait_for_condition` budget ran out. Benign — widen it or fix the predicate. |
| `SETTLE_TIMEOUT` | An animation budget ran out. Benign. |
| `BRIDGE_TIMEOUT` | A live app's main thread is inside a native modal loop. **The op may still have landed — re-read the tree rather than re-issuing blindly.** |

---

## 5. The probe harness

An MCP session is good for asking a question once. A **probe** is the same
question written down so it can be asked again — after a refactor, in CI, or by
whoever picks the bug up next. The harness is a small Python package that removes
the launch-and-attach boilerplate and, more importantly, encodes the invariants
below that are easy to get wrong and silent when you do.

Materialise it into your app:

```bash
cargo teksilo probe    # writes the harness (and worked example probes) into scripts/teksilo_probe/
cargo teksilo setup    # the same, plus the rest of the per-app agent scaffolding
```

Then **read what it wrote** — `ls scripts/teksilo_probe/` — and copy the example
whose shape matches your question. Two shapes are worth recognising, because
almost every useful probe is one of them:

- A **smoke probe** — launch, wait for the bridge, snapshot, assert that the
  expected surface exists, exit. The thing to copy when you want a CI gate that a
  screen still comes up.
- A **single-question probe** — set up one specific state, take a measurement
  before, perform one action, take the measurement after, and print the delta.
  The thing to copy when you are de-risking a design ("when the find banner
  pushes the prose down, does the caret keep its document position, and how far
  does it move on screen?"). It answers on the real screen, before the real
  feature is built.

`cargo teksilo search "automation probe"` pulls further worked examples out of
the version-matched corpus.

### The invariants the harness encodes

Each of these has failed silently in practice, which is why they are in a shared
module rather than copied per probe.

1. **A live-app probe never opens a checked-in fixture.** Driving the real app
   means save, autosave and format migration all happen for real, and any of them
   rewrites the file passed on the command line — corrupting the repo's fixture
   for the next run. A probe that mutates its own fixture still *passes*, so
   nothing catches it. Take a throwaway working copy in a scratch directory and
   open that.

2. **Run in an isolated configuration directory.** Otherwise the probe reads and
   writes the operator's own settings, and a single-instance election may hand
   your launch off to the session they already have open — after which every call
   times out against a process on its way out, and the failure reports a symptom
   several layers from the cause.

3. **Wait for the bridge; never sleep-and-hope.** Poll the app's log until the
   bridge announces its endpoint and token, and fail fast if the process exits
   first — waiting out a full timeout for a process that is already gone only
   delays the log tail you need. Likewise, one snapshot taken the instant the
   bridge connects is not enough for a screen whose content arrives
   asynchronously: poll for a marker with a deadline.

4. **Version-match the MCP binary to the framework the app pins.** A client built
   for a different minor version will answer confidently about tools and
   semantics the app does not have.

### The shape of a probe

```text
isolated config + throwaway working copy
        ↓
launch the app (its own instance), stdout+stderr → a log file
        ↓
poll the log → endpoint + token
        ↓
spawn `teksilo-automation-mcp --connect <endpoint> --token <token>` as a
JSON-RPC-over-stdio child
        ↓
tools/call in a loop:  find_node → invoke_action → settle → read_node / assert_node
        ↓
print a verdict; terminate both children on every exit path
```

Keep a `fail(msg)` that prints the **tail of the app log** before it exits. Most
probe failures are the app saying exactly what went wrong somewhere the probe
never looked.

### Probes and headless tests are different tools

A probe drives the real binary — real windowing, real rendering, real
persistence — and is the only thing that can answer "does this work on screen".
It is also slow and needs a session. Teksilo's own widget tests are **headless**
(`WidgetTree` + `LayoutContext::for_testing` + `MockTextBackend`, no GPU, no
display server) and run in milliseconds. Put layout maths, caret arithmetic and
state machines in headless tests; put "the user clicks this and the dialog opens
over there" in a probe.
