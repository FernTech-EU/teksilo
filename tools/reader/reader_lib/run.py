# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""One run of one scenario: the application, the listener, Orca, and the acts.

A scenario is a function that drives an example through a sequence of acts:

    def body(run: Run) -> None:
        with run.act("Tab to the Info button",
                     expect=[focused(role="push button", name="Info"), said("Info")]):
            run.key("Tab")

Each act is a window of time. Entering it leaves the application alone for
`settle` seconds, so whatever the scene setting set moving is over; then the
start is stamped, the act's steps run, and `record` seconds after the last one
the end is stamped. Everything the application emitted on the AT-SPI bus in
that window, and everything Orca logged in it, belongs to the act.

## How an act is done

Three ways, and a scenario says which each time, because they are not the same
thing to a screen reader:

* `run.key(...)` / `run.type(...)`: **real keys**, pressed through the private
  KWin's `org_kde_kwin_fake_input` (`fake_key.c`). They reach the application
  through the compositor and winit, as a keyboard's do, so window-level keys
  (F10, Alt, Caps Lock) work. Orca does not hear them as key presses: it reads
  the keyboard of its own X display (see `orca.py`), so its key-press speech
  interrupt is not exercised.
* `run.action(...)`: an **AT-SPI action** on a node, which is what a screen
  reader's own activation does. It reaches the handler through the
  application's event loop.
* `run.bridge()`: the **automation bridge**, through Teksilo's own
  `teksilo_probe` harness. A bridge request runs an accessibility sync of its
  own, which no adapter is handed, and its `inject_key` goes straight into the
  widget tree. It is for setting a scene the other two cannot, and an act that
  uses it says so in its record.

## What is judged

The bus and Orca's log, never the bridge's announcement ring: the ring is not
what any platform received. See `checks.py`.
"""

from __future__ import annotations

import datetime as dt
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import uuid
from contextlib import contextmanager
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Iterator

from . import keys as keymap
from .orca import Orca, OrcaLine, lines_between, utterances

HERE = Path(__file__).resolve().parent
READER_DIR = HERE.parent
REPO = READER_DIR.parents[1]
PROBE_DIR = REPO / "crates" / "cargo-teksilo" / "embedded" / "probe"

#: How long the scene is left alone before an act starts.
SETTLE = 1.2
#: How long an act is recorded after its last step.
RECORD = 2.5
#: How long a launch is given to put its window on the bus.
LAUNCH_TIMEOUT = 60.0
#: The least and the most a launch is recorded for, from the application
#: reaching the bus: it ends once the window has been activated and the bus has
#: been quiet for `STARTUP_QUIET`, between the two.
STARTUP = 2.0
STARTUP_MAX = 20.0
STARTUP_QUIET = 1.0
#: How long the listener may take to answer a command.
REPLY_TIMEOUT = 30.0
#: How long Orca is waited for to catch up with an act before the run moves
#: on without it. Orca's main loop stalls for seconds on a loaded machine.
ORCA_CATCH_UP = 45.0
#: How long Orca's log must show no work before Orca counts as idle.
ORCA_QUIET = 0.5
#: How long Orca is given to go idle once it has received every event of an
#: act. An application that never stops (a live chart, a streaming log) keeps
#: Orca busy for good; the act ends after this rather than at `ORCA_CATCH_UP`.
ORCA_TAIL = 2.0
#: Where the pointer is parked at the start of a run: the bottom-right corner
#: of the private output (1280 x 900), away from the window. KWin puts it at
#: the output's centre, over the window, and an overlay that opened there got
#: a hover tooltip nobody asked for.
POINTER_PARK = (1279, 899)


class RunError(RuntimeError):
    """The run could not do what the scenario asked; not a finding."""


def wall() -> str:
    return dt.datetime.now().strftime("%H:%M:%S.%f")


def inside_private_session() -> str | None:
    """Why this is not a private session, or `None` when it is."""
    if os.environ.get("TEKSILO_READER_INNER") != "1":
        return "not started by tools/reader/private_session.sh"
    runtime = os.environ.get("XDG_RUNTIME_DIR", "")
    if not Path(runtime).name.startswith("teksilo-reader-run."):
        return (f"the runtime directory {runtime!r} is not the private one "
                "private_session.sh makes, so the AT-SPI bus here could be the desktop's")
    if os.environ.get("DISPLAY"):
        return "DISPLAY is set, so the desktop's X server is reachable"
    if os.environ.get("GSETTINGS_BACKEND") != "memory":
        return "settings are not kept in memory, so accessibility would be turned on for good"
    return None


def target_dir() -> Path:
    override = os.environ.get("TEKSILO_READER_TARGET_DIR") or os.environ.get("CARGO_TARGET_DIR")
    return Path(override) if override else REPO / "target"


def binary_for(package: str, binary: str | None = None) -> Path:
    name = binary or package
    path = target_dir() / "debug" / name
    if not path.exists():
        raise RunError(f"{path} does not exist: build it first with "
                       f"`cargo build -p {package}`")
    return path


def build_fake_key() -> Path:
    """`fake_key`, compiled from `fake_key.c` with the protocol code generated
    from the system's `fake-input.xml`, once per version of the two.

    The binary lives in `target/reader/.bin/`, named after a hash of its
    sources, so concurrent runs share it and a changed source builds anew. The
    compiler's temporaries go there too: `/tmp` is a small shared tmpfs, and
    runs failed with "No space left on device" when something else filled it.
    """
    import hashlib

    xml = Path("/usr/share/plasma-wayland-protocols/fake-input.xml")
    if not xml.exists():
        raise RunError(f"{xml} is not installed (plasma-wayland-protocols), "
                       "so there is no way to press real keys")
    source = READER_DIR / "fake_key.c"
    digest = hashlib.sha256(source.read_bytes() + xml.read_bytes()).hexdigest()[:16]
    bin_dir = target_dir() / "reader" / ".bin"
    out = bin_dir / f"fake_key-{digest}"
    if out.exists():
        return out
    # Made first, so that the temporaries beside it are on the same file
    # system and the rename below is not a copy across two.
    bin_dir.mkdir(parents=True, exist_ok=True)
    build = Path(tempfile.mkdtemp(prefix="fake_key-", dir=str(bin_dir.parent)))
    try:
        env = dict(os.environ, TMPDIR=str(build))
        header = build / "fake-input-client-protocol.h"
        code = build / "fake-input-protocol.c"
        for kind, dest in (("client-header", header), ("private-code", code)):
            subprocess.run(["wayland-scanner", kind, str(xml), str(dest)], check=True, env=env)
        flags = subprocess.run(["pkg-config", "--cflags", "--libs", "wayland-client"],
                               check=True, capture_output=True, text=True).stdout.split()
        built = build / "fake_key"
        subprocess.run(["gcc", "-O1", "-Wall", "-I", str(build), "-o", str(built),
                        str(source), str(code), *flags], check=True, env=env)
        # A rename, so a concurrent run never executes a half-written binary.
        os.replace(built, out)
    finally:
        shutil.rmtree(build, ignore_errors=True)
    return out


class Keyboard:
    """One `fake_key --stdin` client for the whole run.

    KWin adds a fake-input device, with a pointer, for each client and removes
    it when the client leaves; a client per key made the seat's pointer come
    and go under the application, and winit 0.30 panicked on it ("failed to get
    pointer data", `wayland/seat/pointer/mod.rs:409`). One client keeps the
    seat still.
    """

    def __init__(self, binary: Path) -> None:
        self.proc = subprocess.Popen([str(binary), "--stdin"], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)

    def send(self, steps: list[str], timeout: float = 60.0) -> None:
        import select

        if self.proc.poll() is not None:
            raise RunError(f"fake_key exited: {self.proc.stderr.read() if self.proc.stderr else ''}")
        assert self.proc.stdin is not None and self.proc.stdout is not None
        self.proc.stdin.write(" ".join(steps) + "\n")
        self.proc.stdin.flush()
        ready, _, _ = select.select([self.proc.stdout], [], [], timeout)
        reply = self.proc.stdout.readline().strip() if ready else ""
        if reply != "ok":
            raise RunError(f"fake_key did not take {' '.join(steps)!r}: {reply or 'no answer'}")

    def stop(self) -> None:
        if self.proc.poll() is None:
            try:
                assert self.proc.stdin is not None
                self.proc.stdin.close()
                self.proc.wait(timeout=5)
            except (OSError, subprocess.TimeoutExpired):
                self.proc.kill()


# ---------------------------------------------------------------------------
# The listener, from the driver's side
# ---------------------------------------------------------------------------


class ListenerClient:
    def __init__(self, out: Path, pid: int) -> None:
        self.out = out
        # Its stderr carries libatspi's warnings, hundreds a run: kept in a
        # file beside the events rather than on the terminal. See
        # `libatspi_warnings`.
        self.log = out.with_name("listener.log")
        self.proc = subprocess.Popen(
            [sys.executable, "-W", "ignore::DeprecationWarning", "-m",
             "reader_lib.listener", "--out", str(out), "--pid", str(pid)],
            cwd=str(READER_DIR), stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=open(self.log, "w"), text=True)
        reply = self.recv()
        if not reply.get("ready"):
            self.stop()
            raise RunError(f"the AT-SPI listener did not start: {reply}")

    def recv(self, timeout: float = REPLY_TIMEOUT) -> dict:
        import select

        assert self.proc.stdout is not None
        ready, _, _ = select.select([self.proc.stdout], [], [], timeout)
        if not ready:
            raise RunError("the AT-SPI listener did not answer in time")
        line = self.proc.stdout.readline()
        if not line:
            raise RunError("the AT-SPI listener exited")
        return json.loads(line)

    def call(self, timeout: float = REPLY_TIMEOUT, **message: Any) -> dict:
        assert self.proc.stdin is not None
        self.proc.stdin.write(json.dumps(message, ensure_ascii=False) + "\n")
        self.proc.stdin.flush()
        return self.recv(timeout)

    def events(self) -> list[dict]:
        events = []
        try:
            for line in self.out.read_text(encoding="utf-8").splitlines():
                if line.strip():
                    events.append(json.loads(line))
        except (OSError, ValueError):
            pass
        return events

    def stop(self) -> None:
        if self.proc.poll() is None:
            try:
                assert self.proc.stdin is not None
                self.proc.stdin.write('{"cmd": "quit"}\n')
                self.proc.stdin.flush()
                self.proc.wait(timeout=10)
            except (OSError, subprocess.TimeoutExpired):
                self.proc.kill()

    def libatspi_warnings(self) -> dict[str, int]:
        """How often libatspi refused a cache signal from the application.

        `accesskit_unix` sends `AddAccessible` and `RemoveAccessible` with the
        cache item flattened into separate arguments, and libatspi 2.52 accepts
        only the struct form, so it discards every one: a libatspi client then
        never learns that a removed node came back. Counted rather than
        printed, as the evidence it is."""
        try:
            text = self.log.read_text(errors="replace")
        except OSError:
            return {}
        return {"AddAccessible refused": text.count("AddAccessible with unknown signature"),
                "RemoveAccessible refused": text.count("for RemoveAccessible")}


# ---------------------------------------------------------------------------
# An act
# ---------------------------------------------------------------------------


@dataclass
class Act:
    label: str
    expect: list = field(default_factory=list)
    #: Why the act exists, in the reader's terms: what they should get.
    should: str = ""
    start_mono: float = 0.0
    end_mono: float = 0.0
    start_wall: str = ""
    end_wall: str = ""
    steps: list[str] = field(default_factory=list)
    events: list[dict] = field(default_factory=list)
    orca: list[OrcaLine] = field(default_factory=list)
    tree: dict | None = None
    error: str | None = None
    bridge_used: bool = False
    #: Where in Orca's log this act's events were handled: from Orca's receipt
    #: of the act's first event it listens to, to where it went quiet after
    #: the last. `None` when Orca heard no event of the act, and the act's own
    #: window is used.
    orca_start: str | None = None
    orca_end: str | None = None
    #: How far behind the bus Orca took in the act's first event, in ms.
    orca_lag_ms: float | None = None
    #: Events of the act Orca listens to that it never logged receiving.
    orca_missing: int = 0
    #: The application's name on the bus, as Orca's log names it.
    app_name: str = ""
    results: list[dict] = field(default_factory=list)
    #: Every event of the run up to this act's end, for checks that look back.
    history: list[dict] = field(default_factory=list)

    def rel_ms(self, event: dict) -> float:
        return (event["mono"] - self.start_mono) * 1000.0

    def to_json(self) -> dict:
        return {
            "label": self.label,
            "should": self.should,
            "start_wall": self.start_wall,
            "end_wall": self.end_wall,
            "steps": self.steps,
            "bridge_used": self.bridge_used,
            "error": self.error,
            "orca_window": [self.orca_start, self.orca_end],
            "orca_lag_ms": self.orca_lag_ms,
            "orca_missing": self.orca_missing,
            "events": self.events,
            "orca": [line.to_json() for line in self.orca],
            "utterances": [u.to_json() for u in utterances(self.orca)],
            "results": self.results,
            "tree": self.tree,
        }


# ---------------------------------------------------------------------------
# The run
# ---------------------------------------------------------------------------


@dataclass
class Options:
    orca: bool = False
    binary: str | None = None
    out_dir: Path | None = None
    keep: bool = False
    settle: float = SETTLE
    record: float = RECORD
    #: Take a tree after every act, not only for the acts that check one.
    trees: bool = False


class Run:
    """One launch of one example, driven through one scenario."""

    def __init__(self, scenario: Any, options: Options) -> None:
        self.scenario = scenario
        self.options = options
        stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
        base = options.out_dir or (target_dir() / "reader")
        # Absolute: the listener runs from `tools/reader`, and a relative
        # directory would name somewhere else to it.
        self.out_dir = Path(base).resolve() / f"{scenario.name}-{stamp}-{os.getpid()}"
        self.out_dir.mkdir(parents=True, exist_ok=True)
        self.work = Path(tempfile.mkdtemp(prefix="reader-", dir=str(self.out_dir)))
        self.acts: list[Act] = []
        self.current: Act | None = None
        self.orca: Orca | None = None
        self.app: subprocess.Popen | None = None
        self.listener: ListenerClient | None = None
        self.fake_key: Path | None = None
        self.keyboard: Keyboard | None = None
        #: The run stopped before its acts were done (set by `reader.py`).
        self.stopped: str | None = None
        self.token = str(uuid.uuid4())
        self._bridge = None
        self._orca_cursor = 0
        self._orca_types: set[str] = set()
        self.app_name = ""
        self.app_log = self.out_dir / "app.log"
        self.notes: list[str] = []

    # -- Lifecycle ----------------------------------------------------------

    def start(self) -> None:
        reason = inside_private_session()
        if reason:
            raise RunError(f"refusing to run: {reason}")
        self.fake_key = build_fake_key()
        self.keyboard = Keyboard(self.fake_key)
        self.keyboard.send([f"move:{POINTER_PARK[0]},{POINTER_PARK[1]}"])
        if self.options.orca:
            self.orca = Orca(self.work / "orca")
            self.orca.start(language=self.scenario.orca_lang)
            deadline = time.monotonic() + 30
            while not self.orca.ready() and time.monotonic() < deadline:
                if not self.orca.alive():
                    raise RunError("Orca exited as it started; its log:\n"
                                   + self.orca.text()[-3000:])
                time.sleep(0.25)
            if not self.orca.ready():
                raise RunError("Orca did not start; its log:\n" + self.orca.text()[-3000:])
            if self.orca.factory() != "teksilo_reader_null_speech":
                factory = self.orca.factory()
                self.orca.stop()
                raise RunError(f"Orca chose the speech server {factory!r}, not the silent "
                               "one; stopped it before it could speak")
            # Orca settles into its default script before a window arrives.
            time.sleep(1.0)
        binary = Path(self.options.binary) if self.options.binary else \
            binary_for(self.scenario.package, self.scenario.binary)
        env = dict(os.environ)
        env["RUST_BACKTRACE"] = "1"
        env["TEKSILO_AUTOMATION_TOKEN"] = self.token
        if self.scenario.lang:
            env["LANG"] = self.scenario.lang
        env.update(self.scenario.env)
        launch = Act("launch", should="the window's first reading")
        launch.start_mono = time.monotonic()
        launch.start_wall = wall()
        launch.steps.append(f"launch {binary.name} {' '.join(self.scenario.args)}".strip())
        self.app = subprocess.Popen([str(binary), *self.scenario.args], env=env,
                                    stdout=open(self.app_log, "w"),
                                    stderr=subprocess.STDOUT, cwd=str(REPO))
        self.listener = ListenerClient(self.out_dir / "events.jsonl", self.app.pid)
        deadline = time.monotonic() + LAUNCH_TIMEOUT
        while True:
            if self.app.poll() is not None:
                raise RunError(f"the application exited ({self.app.returncode}) before "
                               "reaching the bus:\n" + self.app_log_tail())
            if self.listener.call(cmd="ping").get("app"):
                break
            if time.monotonic() > deadline:
                raise RunError("the application never reached the private AT-SPI bus:\n"
                               + self.app_log_tail())
            time.sleep(0.25)
        self._wait_for_first_reading(launch)
        launch.end_mono = time.monotonic()
        launch.end_wall = wall()
        self.app_name = (self.listener.call(cmd="find", role="application")
                         .get("node") or {}).get("name") or binary.name
        self._orca_catch_up(launch)
        launch.tree = self.tree_now()
        self.acts.append(launch)
        if self.app.poll() is not None:
            # A tree walked from an application that has gone says nothing
            # about it, and a run with nothing left to act on has passed
            # nothing.
            raise RunError(f"the application exited ({self.app.returncode}) during launch:\n"
                           + self.app_log_tail())

    def _wait_for_first_reading(self, launch: Act) -> None:
        """Wait until the window has been activated and the bus has gone quiet,
        between `STARTUP` and `STARTUP_MAX` from the application reaching it.

        A fixed wait ended before a slow launch had published its tree on a
        loaded machine, and the launch's tree then held nothing but the
        application."""
        assert self.listener is not None
        started = time.monotonic()
        while True:
            now = time.monotonic()
            events = [e for e in self.listener.events() if e["mono"] >= launch.start_mono]
            activated = any(e["type"] in ("window:activate", "object:state-changed:focused")
                            for e in events)
            last = max((e["mono"] for e in events), default=started)
            quiet = now - last >= STARTUP_QUIET
            if now - started >= STARTUP_MAX or (
                    now - started >= STARTUP and activated and quiet):
                return
            time.sleep(0.2)

    def stop(self) -> None:
        if self.keyboard is not None:
            self.keyboard.stop()
        if self.listener is not None:
            self.listener.stop()
        if self._bridge is not None:
            try:
                self._bridge.close()
            except Exception:
                pass
        if self.app is not None and self.app.poll() is None:
            self.app.terminate()
            try:
                self.app.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.app.kill()
        if self.orca is not None:
            shutil.copy(self.orca.log, self.out_dir / "orca-debug.out") \
                if self.orca.log.exists() else None
            self.orca.stop()
        shutil.rmtree(self.work, ignore_errors=True)

    def app_log_tail(self, lines: int = 30) -> str:
        try:
            return "\n".join(self.app_log.read_text(errors="replace").splitlines()[-lines:])
        except OSError:
            return ""

    def alive(self) -> bool:
        return self.app is not None and self.app.poll() is None

    # -- Acts -----------------------------------------------------------------

    @contextmanager
    def act(self, label: str, expect: list | None = None, *, should: str = "",
            settle: float | None = None, record: float | None = None,
            tree: bool | None = None) -> Iterator[Act]:
        """Record one act: a quiet `settle`, the steps inside the block, then
        `record` seconds of what followed them.

        An exception inside the block is the scenario failing to do the act,
        recorded as the act's error, and the run goes on to the next act.
        """
        if self.current is not None:
            raise RunError("acts do not nest")
        act = Act(label, list(expect or []), should=should)
        self.acts.append(act)
        time.sleep(self.options.settle if settle is None else settle)
        self._orca_settle()
        act.start_mono = time.monotonic()
        act.start_wall = wall()
        self.current = act
        try:
            yield act
        except Exception as exc:  # noqa: BLE001 - a failed act is data
            act.error = f"{type(exc).__name__}: {exc}"
        finally:
            self.current = None
        time.sleep(self.options.record if record is None else record)
        act.end_mono = time.monotonic()
        act.end_wall = wall()
        self._orca_catch_up(act)
        wants_tree = tree if tree is not None else (
            self.options.trees or any(getattr(c, "needs_tree", False) for c in act.expect))
        if wants_tree and self.alive():
            act.tree = self.tree_now()
        if not self.alive():
            act.error = (act.error or "") + f" the application exited ({self.app.returncode})"

    def _orca_catch_up(self, act: Act) -> None:
        """Wait until Orca has handled what the act made the application send,
        and mark where in Orca's log that was.

        Orca's main loop stalls, for seconds on a loaded machine, and then
        works through its backlog: speech measured by the clock would be
        credited to whichever act was running by then. So the act's events that
        Orca listens to are matched in order to Orca's receipt of each, and the
        act is over for Orca once it has received the last of them and its log
        has been quiet for `ORCA_QUIET`. The next act starts only then, so
        each act meets an idle Orca, as a user who waits for speech does.
        """
        from .orca import hears, last_busy, listened, receipts, seconds

        if self.orca is None or not self.orca.alive():
            return
        assert self.listener is not None
        events = [e for e in self.listener.events()
                  if act.start_mono <= e["mono"] <= act.end_mono
                  and not e["type"].startswith("harness:")]
        deadline = time.monotonic() + ORCA_CATCH_UP
        tail_deadline: float | None = None
        while True:
            text = self.orca.text()
            if not self._orca_types:
                self._orca_types = listened(text)
            lines = text.splitlines()
            wanted = [e for e in events if hears(self._orca_types, e["type"])]
            got = receipts(lines, self._orca_cursor, getattr(self, "app_name", ""))
            # In order: each wanted event is received after the one before it.
            matched: list = []
            position = 0
            for event in wanted:
                while position < len(got) and got[position].type != event["type"]:
                    position += 1
                if position == len(got):
                    break
                matched.append((event, got[position]))
                position += 1
            caught_up = len(matched) == len(wanted)
            busy = last_busy(lines, self._orca_cursor)
            quiet_since = None
            if busy is not None:
                stamp = lines[busy][:15]
                try:
                    quiet_since = seconds(stamp)
                except ValueError:
                    quiet_since = None
            now = seconds(wall())
            idle = quiet_since is None or now - quiet_since >= ORCA_QUIET
            if caught_up and tail_deadline is None:
                tail_deadline = time.monotonic() + ORCA_TAIL
            if (caught_up and (idle or time.monotonic() > tail_deadline)) \
                    or time.monotonic() > deadline:
                break
            time.sleep(0.1)
        act.orca_missing = len(wanted) - len(matched)
        if matched:
            act.orca_start = matched[0][1].stamp
            act.orca_lag_ms = round((seconds(matched[0][1].stamp)
                                     - seconds(matched[0][0]["wall"])) * 1000, 1)
        end = len(lines)
        act.orca_end = wall() if act.orca_start else None
        if act.orca_missing:
            self.note(f"{act.label}: Orca never logged receiving {act.orca_missing} of the "
                      f"{len(wanted)} events of this act it listens to, "
                      f"after {ORCA_CATCH_UP:g}s")
        self._orca_cursor = end

    def _orca_settle(self) -> None:
        """Before an act starts: let Orca finish with whatever happened since
        the last act, and start the act's part of its log after it.

        A step taken between acts (a focus request, a key that sets the next
        scene, a toast the application raised on its own) makes the
        application send events Orca then handles; matched to the next act's
        own events by type, their speech was credited to that act. Waiting for
        Orca to go idle and moving the log cursor past it keeps each act's part
        of the log to what the act caused. Bounded by `ORCA_TAIL` beyond
        `ORCA_QUIET`, for an application that never stops.
        """
        from .orca import last_busy, seconds

        if self.orca is None or not self.orca.alive():
            return
        deadline = time.monotonic() + ORCA_QUIET + ORCA_TAIL
        while True:
            lines = self.orca.text().splitlines()
            busy = last_busy(lines, self._orca_cursor)
            idle = True
            if busy is not None:
                try:
                    idle = seconds(wall()) - seconds(lines[busy][:15]) >= ORCA_QUIET
                except ValueError:
                    idle = True
            if idle or time.monotonic() > deadline:
                break
            time.sleep(0.1)
        self._orca_cursor = len(lines)

    def note(self, text: str) -> None:
        """Something the scenario wants in the report beside the acts."""
        self.notes.append(text)

    def _step(self, text: str) -> None:
        if self.current is not None:
            self.current.steps.append(text)

    # -- Doing things ---------------------------------------------------------

    def key(self, *chords: str, gap: float = 0.15) -> None:
        """Press real keys, one chord after another: `run.key("Tab", "Tab")`."""
        assert self.keyboard is not None
        for chord in chords:
            self._step(f"key {chord}")
            self.keyboard.send(keymap.chord_steps(chord))
            time.sleep(gap)

    def type(self, text: str) -> None:
        """Type ASCII text on a US layout, key by key."""
        assert self.keyboard is not None
        self._step(f"type {text!r}")
        self.keyboard.send(keymap.text_steps(text))

    def pointer_to(self, x: float, y: float) -> None:
        """Put the pointer at a position of the private output (1280 x 900).
        It is parked at `POINTER_PARK` when the run starts."""
        assert self.keyboard is not None
        self._step(f"pointer to {x:g},{y:g}")
        self.keyboard.send([f"move:{x:g},{y:g}"])

    def wait(self, seconds: float) -> None:
        self._step(f"wait {seconds:g}s")
        time.sleep(seconds)

    def action(self, action: str | int = 0, **spec: Any) -> dict:
        """Do an AT-SPI action on a node, as a screen reader's activation does.

        `spec` finds the node: `focused=True`, or `role=` / `name=` /
        `name_contains=` / `name_startswith=` / `nth=`.
        """
        assert self.listener is not None
        self._step(f"AT-SPI action {action!r} on {spec}")
        reply = self.listener.call(cmd="action", action=action, **spec)
        if "error" in reply:
            raise RunError(reply["error"])
        return reply

    def grab_focus(self, **spec: Any) -> dict:
        """Ask for focus through AT-SPI's Component interface."""
        assert self.listener is not None
        self._step(f"AT-SPI grab_focus on {spec}")
        reply = self.listener.call(cmd="grab_focus", **spec)
        if "error" in reply:
            raise RunError(reply["error"])
        return reply

    def find(self, **spec: Any) -> dict | None:
        """The first node matching `spec` on the bus now, without its children."""
        assert self.listener is not None
        return self.listener.call(cmd="find", **spec).get("node")

    def find_all(self, **spec: Any) -> list[dict]:
        assert self.listener is not None
        return self.listener.call(cmd="find_all", **spec).get("nodes", [])

    def wait_for(self, timeout: float = 10.0, **spec: Any) -> dict:
        """Poll the bus until a node matching `spec` is there."""
        deadline = time.monotonic() + timeout
        while True:
            node = self.find(**spec)
            if node:
                return node
            if time.monotonic() > deadline:
                raise RunError(f"no node matching {spec} reached the bus in {timeout:g}s")
            time.sleep(0.25)

    def tree_now(self, limit: int = 6000) -> dict | None:
        assert self.listener is not None
        reply = self.listener.call(timeout=120, cmd="tree", limit=limit)
        return reply.get("tree")

    def snapshot(self, label: str) -> dict | None:
        """A tree outside any act, kept in the report under `label`."""
        tree = self.tree_now()
        act = Act(f"tree: {label}", should="a snapshot, not an act")
        act.start_mono = act.end_mono = time.monotonic()
        act.start_wall = act.end_wall = wall()
        act.tree = tree
        self.acts.append(act)
        return tree

    def close_window(self) -> None:
        """Ask the private KWin to close this application's windows, as the
        close button or Alt+F4 would."""
        assert self.app is not None
        self._step("close the window through KWin")
        script = self.work / f"close-{self.app.pid}.js"
        script.write_text(
            "const wins = workspace.windowList ? workspace.windowList() : workspace.clientList();\n"
            f"wins.forEach(w => {{ if (w.pid === {self.app.pid}) w.closeWindow(); }});\n",
            encoding="utf-8")
        self._kwin_script(script)

    def resize_window(self, width: int, height: int) -> None:
        """Ask the private KWin to resize this application's windows, keeping
        their top-left corner."""
        assert self.app is not None
        self._step(f"resize the window to {width}x{height} through KWin")
        script = self.work / f"resize-{self.app.pid}-{width}x{height}.js"
        script.write_text(
            "const wins = workspace.windowList ? workspace.windowList() : workspace.clientList();\n"
            f"wins.forEach(w => {{ if (w.pid === {self.app.pid}) {{ const g = w.frameGeometry; "
            f"w.frameGeometry = {{x: g.x, y: g.y, width: {int(width)}, height: {int(height)}}}; }} }});\n",
            encoding="utf-8")
        self._kwin_script(script)

    def _kwin_script(self, script: Path) -> None:
        loaded = subprocess.run(
            ["gdbus", "call", "--session", "--dest", "org.kde.KWin", "--object-path",
             "/Scripting", "--method", "org.kde.kwin.Scripting.loadScript", str(script)],
            capture_output=True, text=True, timeout=10)
        ids = re.findall(r"-?\d+", loaded.stdout)
        if loaded.returncode != 0 or not ids or int(ids[0]) < 0:
            raise RunError("KWin would not load a script: "
                           + (loaded.stderr.strip() or loaded.stdout.strip()))
        for method in ("run", "stop"):
            subprocess.run(
                ["gdbus", "call", "--session", "--dest", "org.kde.KWin", "--object-path",
                 f"/Scripting/Script{ids[0]}", "--method", f"org.kde.kwin.Script.{method}"],
                capture_output=True, text=True, timeout=10)

    # -- The bridge -------------------------------------------------------------

    def bridge(self):
        """The automation bridge, through `teksilo_probe`, connected on first use.

        Using it inside an act marks the act: a bridge request syncs the
        accessibility tree itself, which no adapter is handed.
        """
        if self.current is not None:
            self.current.bridge_used = True
        if self._bridge is None:
            if str(PROBE_DIR) not in sys.path:
                sys.path.insert(0, str(PROBE_DIR))
            from teksilo_probe import bridge as probe_bridge  # noqa: E402
            from teksilo_probe.session import connect  # noqa: E402

            mcp = target_dir() / "debug" / "teksilo-automation-mcp"
            if not mcp.exists():
                raise RunError(f"{mcp} does not exist: `cargo build -p teksilo-automation-mcp`")
            os.environ["TEKSILO_MCP_BIN"] = str(mcp)
            assert self.app is not None
            found = probe_bridge.wait_for_bridge(self.app.pid, timeout=30,
                                                 log=str(self.app_log), proc=self.app,
                                                 token=self.token)
            self._bridge = connect(found, client_name=f"reader-{self.scenario.name}")
        return self._bridge

    # -- Collecting ---------------------------------------------------------------

    def collect_events(self, act: Act) -> list[dict]:
        """Hand one finished act its events now, for a scenario that decides
        what to do next from what the last act did."""
        assert self.listener is not None
        act.events = [e for e in self.listener.events()
                      if act.start_mono <= e["mono"] <= act.end_mono]
        return act.events

    def last_focus(self) -> dict:
        """The node the most recent focus change on the bus named."""
        from .checks import _focus_node, _is_focus

        assert self.listener is not None
        moves = [e for e in self.listener.events() if _is_focus(e)]
        return _focus_node(moves[-1]) if moves else {}

    def collect(self) -> None:
        """Hand each act its events and Orca lines, and run its checks.

        Orca is stopped first, so its log is whole: whatever it was still
        processing when the last act ended is written before it exits.
        """
        from .checks import evaluate, observations

        assert self.listener is not None
        if self.orca is not None and self.orca.alive():
            time.sleep(0.5)
            self.orca.stop()
        recorded = self.listener.events()
        orca_text = self.orca.text() if self.orca is not None else ""
        for act in self.acts:
            if act.start_mono == act.end_mono:
                continue
            act.app_name = self.app_name
            act.events = [e for e in recorded if act.start_mono <= e["mono"] <= act.end_mono]
            act.history = [e for e in recorded if e["mono"] <= act.end_mono]
            if self.orca is not None:
                act.orca = lines_between(orca_text, act.orca_start or act.start_wall,
                                         act.orca_end or act.end_wall)
            act.results = [evaluate(check, act, self.orca is not None) for check in act.expect]
            act.results.extend(observations(act))
