<!-- SPDX-License-Identifier: MPL-2.0 -->
<!-- SPDX-FileCopyrightText: 2026 FernTech -->

# Testing what a screen reader gets

`tools/reader/` runs a Teksilo example in a private, invisible desktop session
and records what a screen reader receives from it: the AT-SPI events the
application emits, the tree a reader walks, and what Orca 46.1 would say. It
exists because every check that reads the tree as Teksilo *builds* it (the
automation snapshot, the announcement ring, a unit test on a `TreeUpdate`)
passed over defects that a screen reader hit at once: dialogs whose content no
adapter could reach, announcements Orca dropped, descriptions no platform
read. This reads what the platform adapter hands a reader, on Linux.

```bash
tools/reader/reader.py list                          # the scenarios
tools/reader/reader.py run announcer-calendar-arrows --orca
tools/reader/reader.py tabwalk spin-box --orca       # Tab through any example
tools/reader/reader.py tree widget-catalog -- --tab overlays
```

`tabwalk` and `tree` take `--lang`, `--env NAME=VALUE` (repeatable) for the
example's environment, and the example's own arguments after `--`.

Build the example first (`cargo build -p <package>`); the harness runs
`target/debug/<package>` and never builds. Each run writes a directory under
`target/reader/` (or `--out DIR`):

| file | what it holds |
|---|---|
| `report.txt` | per act: the events and Orca's speech on one timeline, then each check and observation |
| `run.json` | everything, for tools: every event, every Orca log line, every result, the trees |
| `events.jsonl` | the raw AT-SPI event stream, one event a line, monotonic time stamps |
| `orca-debug.out` | Orca's whole debug log (with `--orca`) |
| `tree-<act>.txt` | the tree a reader walks, as an outline, after the acts that took one |
| `app.log` | the example's own output |

Exit status: `0` when every check passed and nothing was observed, `2` when a
check failed or an observation was made (a finding), `1` when something could
not be run.

## What it needs

Linux with KDE's `kwin_wayland` 6, `dbus-run-session`, `gdbus`, Python 3 with
`gi.repository.Atspi` (libatspi 2.52), `gcc`, `wayland-scanner`, the
`wayland-client` development files and `plasma-wayland-protocols`
(`/usr/share/plasma-wayland-protocols/fake-input.xml`). With `--orca`, Orca
46.1 at `/usr/bin/orca` and `Xwayland`. It was written against Orca 46.1 and
KWin 6.6; another Orca may log differently.

## The private session

`private_session.sh` wraps every run; `reader.py` re-runs itself inside one
when started from the desktop, and refuses to act in any session it did not
make. The session has:

- its **own D-Bus session bus**, and so its own AT-SPI bus and registry;
- its **own runtime directory**, made with `mktemp -d`, mode `0700`, set before
  the bus starts. With the desktop's, a private `at-spi-bus-launcher` once took
  the desktop's own socket (`at-spi/bus_0`) and deleted it on exit, leaving
  the user's screen reader with no accessibility bus. The script checks that
  socket before and after every run and fails loudly if it went missing;
- its **own KWin** (`kwin_wayland --virtual`), which draws to no screen but
  sends frame callbacks, so everything that waits on a frame runs as on a
  desktop; **no `DISPLAY`**;
- its **own configuration and data homes**, a US keyboard layout and English
  messages (`TEKSILO_READER_LANG` overrides the language);
- **settings in memory** (`GSETTINGS_BACKEND=memory`): turning accessibility
  on is a settings write, and through dconf it would land in the user's own;
- **no sound**: PulseAudio, PipeWire and speech-dispatcher point at sockets
  that do not exist, and Orca speaks to a null speech server.

Every process on the private bus is killed when the run ends.

## What it records

**The event stream.** A listener process (`reader_lib/listener.py`) records
every `object:`, `window:`, `document:` and `focus:` event the application
emits on the private AT-SPI bus, which is every event the AccessKit adapter
can emit (`accesskit_atspi_common/src/events.rs`), stamped with the system's
monotonic clock.

**The tree.** The listener walks the application on AT-SPI and records, for
every node, what a reader can ask: role, name, description, states,
attributes (`posinset`, `setsize`, `placeholder-text`, …), relations,
actions, value, text, locale, extents. This is the tree *after*
`accesskit_consumer`'s filter, as every adapter lists children: a hidden node
and its whole subtree are gone, a `GenericContainer` is replaced by its
children.

**What Orca says** (`--orca`). Orca 46.1 runs in the private session with a
null speech server that writes every utterance and every stop into its debug
log; the report reads `SPEECH OUTPUT`, `NULL SPEECH: stop` and Orca's event
manager lines out of it. A stop cuts what was being said. The null server
cannot tell how long an utterance lasts, so the report estimates it (about 14
characters a second, Orca's default rate, each utterance queued behind the
last) and marks an utterance **cut** when a stop falls inside that estimate.
Orca is started through a small shim over its own launcher that leaves its
process name alone (so the user's own Orca does not refuse to start beside
it), limits its single-instance check to the private bus, and line-buffers
its log; nothing in Orca itself is changed.

## How an act is done

A scenario is a Python function over a `Run`. Each act is a window of time:
entering it leaves the application alone for a moment (`settle`), then the
steps run, then what followed them is recorded for `record` seconds.

```python
from reader_lib.checks import announced, focused, said
from reader_lib.scenario import Scenario

def body(run):
    run.grab_focus(role="push button", name="Next month")
    with run.act("Space on Next month", [announced("June 2026"), said("June 2026")],
                 should="the calendar moves on and the reader hears the new month"):
        run.key("space")

SCENARIOS = [Scenario("my-scenario", "datetime-pickers", body, "what it covers")]
```

Three ways to act, which are not the same thing to a screen reader:

- `run.key("Tab")`, `run.key("Alt+F", "Down")`, `run.type("abc")`: **real
  keys**, pressed through the private KWin's `org_kde_kwin_fake_input` by a
  small client (`fake_key.c`, compiled once into `target/reader/.bin/`), one
  client for the whole run so the seat stays still. They reach the example
  through the compositor and winit, so window-level keys (F10, Alt, Caps Lock)
  work. Whether Orca hears them is up to the application: see "What it does
  not show".
- `run.action("click", role=..., name=...)`: an **AT-SPI action**, what a
  screen reader's own activation does. `run.grab_focus(...)` asks for focus
  the same way.
- `run.bridge()`: the **automation bridge** (`teksilo_probe`). A bridge request
  runs an accessibility sync of its own, which no adapter is handed, and its
  `inject_key` goes into the widget tree, past winit. Use it only to set a
  scene the other two cannot reach; an act that uses it is marked so.

`run.find(...)`, `run.find_all(...)`, `run.wait_for(...)` and `run.snapshot(label)`
read the bus; `run.close_window()` and `run.resize_window(w, h)` ask the
private KWin to close or resize the window, as its decorations would, and
`run.pointer_to(x, y)` moves the pointer (it is parked in the output's
bottom-right corner at the start, so nothing opens a hover tooltip by itself).

Keep every step inside an act, scene setting included (label it "scene: ..."):
before each act the harness waits for Orca to finish with whatever came
before, and starts the act's part of Orca's log after it, but an act's checks
only see its own window.

## What is judged

**Checks** are what a scenario states a reader should get
(`reader_lib/checks.py`): `said`, `not_said`, `said_once`, `announced`,
`not_announced`, `focused`, `focus_stays`, `event`, `no_event`, `in_tree`,
`not_in_tree`, `custom`. A check on speech is skipped without `--orca`. Text
checks match a substring with spaces and case ignored, so `said("checked")`
also passes on "not checked"; say what you mean.

**Orca's speech is credited by cause, not by the clock.** Orca's main loop
stalls for seconds on a loaded machine. After each act the harness matches the
act's events that Orca listens to (read from Orca's own "registering listener"
lines) to Orca's receipt of each, in order, and waits until Orca has received
the last and gone quiet (at most `ORCA_TAIL` longer for an application that
never goes quiet, such as a live chart). A report line says when Orca lagged.

**Observations** are made on every act whatever the scenario says, because
each is wrong for any reader anywhere:

- an announcement from a node the bus had already been told was **defunct**
  (Orca drops it);
- an announcement that reached the bus **with the act's first focus change**,
  in the same accessibility update (within 25 ms before it): Orca stops speech
  to read a new focus, so it is cut;
- Orca **ignoring an event** because its source was defunct, except the focus
  loss of a node just removed, which Orca drops on every close and a reader
  never misses;
- Orca's speech **cut** by a stop. An act that presses several keys in a row
  cuts its own speech the way a fast typist does. Orca's echo of a key (the
  null speech server logs it as `key event`) is marked `echo` and never
  counted as cut: the next key cuts it for any typist.

The launch act also runs a **tree audit** (`reader_lib/audit.py`): a window
with no name, a focusable control with no name, a dialog with no name, an
unnamed scroll pane, a node the adapter could not answer for.

## What it proves, and what it does not

It proves what the Linux adapter (`accesskit_atspi_common` through
`accesskit_unix`) hands a reader, and what Orca 46.1 does with it, for the
acts a scenario performs, on the machine it runs on.

It does not show:

- **Orca's answer to keys, unless the application reports them.** In a Wayland
  session, libatspi 2.52 gives Orca its legacy keyboard device, which learns of
  a key only when the application reports it to the AT-SPI registry
  (`DeviceEventController.NotifyListenersSync`), as the GTK and Qt bridges do.
  What Orca says about a caret move, "selected" after Space, key echo, and
  Orca's own commands all depend on it. KWin 6.6 offers the newer
  `org.freedesktop.a11y.KeyboardMonitor`, but this libatspi does not use it.
  The harness presses keys through the compositor, so Orca hears exactly what
  the application reports, as it would on the user's own Wayland desktop.
- **Windows and macOS.** The UIA and macOS adapters are different code; a
  finding here is a Linux finding until their source says otherwise, and a
  claim about NVDA, JAWS or VoiceOver rests on reading their source.
- **Orca's key-press speech**, and anything Orca does in answer to a key it
  sees itself (its own navigation commands, flat review, key echo).
- **Timing on another machine.** A cut is estimated from Orca's log; a race
  that loses here may win on a faster machine, and the other way round. Run a
  scenario more than once before trusting a result that depends on timing.
- **Braille.** Braille output is off.
- **Orca in another language.** `Scenario(orca_lang=...)` sets Orca's `LANG`,
  but where the distribution installs Orca's translations under
  `/usr/share/locale-langpack` (Ubuntu), Orca 46.1 looks only in
  `/usr/share/locale` and stays English.

The tree the harness walks is read fresh from the application each time
(libatspi's cache is cleared first). Orca's own view may differ: libatspi
refuses `accesskit_unix`'s cache signals (each report counts them), so a
client that read a node before it left keeps what it read, and a node that
comes back under an id the adapter removed is dead to it.

The automation bridge's own record (its node snapshot, its announcement ring)
is not what a platform received, and the harness never judges by it.
