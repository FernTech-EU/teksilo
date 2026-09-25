# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Orca 46.1, in the private session, speaking nowhere, and what its log says.

Ported from CalendrierAccessible's `scripts/probes/speech_order.py`.

## How it is kept silent

Orca is given a settings directory of its own, created for the run, whose
speech server is a module written into it that speaks nothing and writes every
utterance and every stop into Orca's debug log (`NULL_SPEECH_MODULE`). On top
of that the private session points PulseAudio, PipeWire and speech-dispatcher
at sockets that do not exist (`private_session.sh`), and braille, the sound
effects, the main window and the splash window are off. The run stops Orca at
once if it reports any speech server factory but the null one.

## How it is kept off the desktop

Orca 46.1 reads and rewrites the X keymap as it starts (`xkbcomp $DISPLAY`,
`orca_modifier_manager.py`) and stops without an X display, so it is given an
Xwayland of its own on the private KWin. Only Orca is told of it; the
application runs with no `DISPLAY`.

Orca's launcher (`/usr/bin/orca`) does two things that reach outside the
session, and the harness runs it through `ORCA_SHIM`, which executes that very
launcher's source with those two functions replaced and calls its `main()`:

* `setProcessName('orca')` renames the process to `orca`. The user's own Orca,
  started on the desktop later, looks for other Orcas with
  `pgrep -u <uid> -x orca` (`otherOrcas`, `/usr/bin/orca:198`) and refuses to
  start beside one, so a harness Orca under that name would stop the user's
  screen reader from starting for as long as a sweep runs. The shim leaves the
  process named `python3`.
* `otherOrcas()` would see every harness Orca in every other private session
  by the same `pgrep`, and refuse to start. The shim answers with the Orcas on
  this run's own private bus, which is none.

It also opens Orca's debug file line-buffered rather than block-buffered, so
the log on disk is complete up to the line just written.

Nothing in Orca itself is changed: `orca.main()` runs as it does for a user.
The harness never passes `--replace`, which kills other Orcas.

## What the log is read for

`SPEECH OUTPUT: '<text>' {voice}` lines are what Orca would have said.
`NULL SPEECH: stop` is a stop, which cuts whatever was being said.
`EVENT MANAGER: Ignoring defunct object` follows an event Orca dropped because
its source was already gone. `EVENT MANAGER: <event> ... is not obsoleted` is
an event Orca took from its queue to process; an event it replaced with a
later one of the same kind is logged as obsoleted instead.
"""

from __future__ import annotations

import json
import os
import re
import select
import subprocess
import time
from dataclasses import dataclass, field
from pathlib import Path

ORCA_LAUNCHER = "/usr/bin/orca"

NULL_SPEECH_MODULE = '''\
"""A speech server for Orca that speaks nothing and records what it is given.

Written by Teksilo's tools/reader/reader_lib/orca.py into a settings directory
of its own, for one run.
"""

from orca import debug, speechserver


class SpeechServer(speechserver.SpeechServer):
    _server = None

    @staticmethod
    def getFactoryName():
        return "teksilo reader null speech"

    @staticmethod
    def getSpeechServers():
        return [SpeechServer._get()]

    @staticmethod
    def getSpeechServer(info=None):
        return SpeechServer._get()

    @staticmethod
    def shutdownActiveServers():
        SpeechServer._server = None

    @staticmethod
    def _get():
        if SpeechServer._server is None:
            SpeechServer._server = SpeechServer()
        return SpeechServer._server

    def getInfo(self):
        return ["teksilo reader null speech", "null"]

    def getVoiceFamilies(self):
        return []

    def _note(self, what):
        debug.printMessage(debug.LEVEL_SEVERE, "NULL SPEECH: " + what, True)

    def speak(self, text=None, acss=None, interrupt=True):
        self._note("speak %r interrupt=%r" % (text, interrupt))

    def speakCharacter(self, character, acss=None):
        self._note("character %r" % (character,))

    def speakKeyEvent(self, event, acss=None):
        self._note("key event")

    def sayAll(self, utteranceIterator, progressCallback):
        for context, _acss in utteranceIterator:
            self._note("say all %r" % (context.utterance,))

    def stop(self):
        self._note("stop")

    def shutdown(self):
        self._note("shutdown")

    def reset(self, text=None, acss=None):
        self._note("reset")

    def updateCapitalizationStyle(self):
        pass

    def updatePunctuationLevel(self):
        pass

    def increaseSpeechRate(self, step=5):
        pass

    def decreaseSpeechRate(self, step=5):
        pass

    def increaseSpeechPitch(self, step=0.5):
        pass

    def decreaseSpeechPitch(self, step=0.5):
        pass

    def increaseSpeechVolume(self, step=0.5):
        pass

    def decreaseSpeechVolume(self, step=0.5):
        pass
'''

ORCA_SHIM = '''\
"""Run Orca's own launcher, kept to this private session.

Written by Teksilo's tools/reader/reader_lib/orca.py. See that module's
docstring for why each of the two functions below is replaced.
"""

import os
import subprocess
import sys

LAUNCHER = sys.argv.pop(1)
BUS = os.environ["DBUS_SESSION_BUS_ADDRESS"]

with open(LAUNCHER, encoding="utf-8") as source:
    code = compile(source.read(), LAUNCHER, "exec")
host = {"__name__": "teksilo_reader_orca_host", "__file__": LAUNCHER}
exec(code, host)


def on_this_bus(pid):
    try:
        with open(f"/proc/{pid}/environ", "rb") as env:
            return f"DBUS_SESSION_BUS_ADDRESS={BUS}".encode() in env.read().split(b"\\0")
    except OSError:
        return False


def other_orcas_on_this_bus():
    mine = os.getpid()
    found = subprocess.run(["pgrep", "-u", str(os.getuid()), "-f", "teksilo_reader_orca_shim"],
                           capture_output=True, text=True, check=False).stdout.split()
    return [int(p) for p in found if int(p) != mine and on_this_bus(int(p))]


def line_buffered_open(file, mode="r", *args, **kwargs):
    """The launcher opens its debug file block-buffered, so the last few
    kilobytes of the log are still in memory whenever the harness reads it
    while Orca runs. Line-buffered, every line is on disk as it is logged."""
    if "w" in mode and "b" not in mode and not args and "buffering" not in kwargs:
        kwargs["buffering"] = 1
    return open(file, mode, *args, **kwargs)


host["setProcessName"] = lambda name: None
host["otherOrcas"] = other_orcas_on_this_bus
host["open"] = line_buffered_open
sys.argv[0] = LAUNCHER
sys.exit(host["main"]())
'''


def read_line(stream, timeout: float) -> str:
    ready, _, _ = select.select([stream], [], [], timeout)
    return stream.readline() if ready else ""


@dataclass
class Orca:
    """Orca 46.1 in the private session, speaking nowhere."""

    root: Path
    proc: subprocess.Popen | None = None
    xwayland: subprocess.Popen | None = None
    display: str | None = None
    #: Orca's debug level for events; `ALL` records every event it received.
    log_events: bool = True

    @property
    def log(self) -> Path:
        return self.root / "orca-debug.out"

    def start_x(self) -> None:
        read_end, write_end = os.pipe()
        self.xwayland = subprocess.Popen(
            ["Xwayland", "-displayfd", str(write_end), "-noreset", "-geometry", "640x480"],
            pass_fds=(write_end,), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        os.close(write_end)
        number = b""
        deadline = time.monotonic() + 10
        while not number.endswith(b"\n") and time.monotonic() < deadline:
            ready, _, _ = select.select([read_end], [], [], 0.2)
            if ready:
                chunk = os.read(read_end, 16)
                if not chunk:
                    break
                number += chunk
        os.close(read_end)
        if not number.strip().isdigit():
            raise RuntimeError("the private Xwayland reported no display")
        self.display = f":{number.strip().decode()}"

    def start(self, *, language: str | None = None) -> None:
        if not Path(ORCA_LAUNCHER).exists():
            raise RuntimeError(f"{ORCA_LAUNCHER} is not installed")
        prefs = self.root / "prefs"
        prefs.mkdir(parents=True, exist_ok=True)
        # Orca puts its settings directory on `sys.path`, and the speech
        # module is imported by name from there when `orca.<name>` fails.
        (prefs / "teksilo_reader_null_speech.py").write_text(NULL_SPEECH_MODULE,
                                                             encoding="utf-8")
        shim = self.root / "teksilo_reader_orca_shim.py"
        shim.write_text(ORCA_SHIM, encoding="utf-8")
        quiet = {
            "speechServerFactory": "teksilo_reader_null_speech",
            "speechServerInfo": None,
            "enableSpeech": True,
            "enableBraille": False,
            "enableBrailleMonitor": False,
            "enableSound": False,
            "enableTutorialMessages": False,
            "firstStart": False,
            "startingProfile": ["Default", "default"],
            "activeProfile": ["Default", "default"],
        }
        settings = {
            "general": dict(quiet),
            "profiles": {"default": dict(quiet, profile=["Default", "default"])},
            "pronunciations": {},
            "keybindings": {},
        }
        (prefs / "user-settings.conf").write_text(json.dumps(settings, indent=2),
                                                  encoding="utf-8")
        runtime = os.environ["XDG_RUNTIME_DIR"]
        self.start_x()
        env = dict(os.environ)
        assert self.display is not None
        env["DISPLAY"] = self.display
        env["SPEECHD_ADDRESS"] = f"unix_socket:{runtime}/no-speech-dispatcher"
        env["SPEECHD_CMD"] = "/bin/false"
        env["PULSE_SERVER"] = f"unix:{runtime}/no-pulseaudio"
        env["PIPEWIRE_REMOTE"] = f"{runtime}/no-pipewire"
        env["GST_AUDIO_SINK"] = "fakesink"
        if language:
            env["LANG"] = language
        argv = ["python3", str(shim), ORCA_LAUNCHER, "--debug-file", str(self.log),
                "-u", str(prefs), "-d", "braille", "-d", "braille-monitor",
                "-d", "main-window", "-d", "splash-window"]
        self.proc = subprocess.Popen(argv, env=env, stdout=subprocess.DEVNULL,
                                     stderr=subprocess.DEVNULL)

    def ready(self) -> bool:
        return "SPEECH: Using speech server factory" in self.text()

    def text(self) -> str:
        try:
            return self.log.read_text(encoding="utf-8", errors="replace")
        except OSError:
            return ""

    def factory(self) -> str | None:
        match = re.search(r"SPEECH: Using speech server factory: (\S+)", self.text())
        return match.group(1) if match else None

    def alive(self) -> bool:
        return self.proc is not None and self.proc.poll() is None

    def stop(self) -> None:
        for proc in (self.proc, self.xwayland):
            if proc is not None and proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    proc.kill()


# ---------------------------------------------------------------------------
# Reading the log
# ---------------------------------------------------------------------------

ORCA_LINE = re.compile(r"^(\d\d:\d\d:\d\d\.\d{6}) - (.*)$")
SPEECH_OUTPUT = re.compile(r"^SPEECH OUTPUT: '(.*)' \{")


@dataclass
class OrcaLine:
    stamp: str
    text: str

    @property
    def spoken(self) -> str | None:
        """The text of a `SPEECH OUTPUT` line, or `None`."""
        match = SPEECH_OUTPUT.match(self.text)
        return match.group(1) if match else None

    @property
    def is_stop(self) -> bool:
        return self.text.startswith("NULL SPEECH: stop")

    @property
    def is_key_echo(self) -> bool:
        """The null speech server's note that the utterance just before was
        Orca echoing a key (`speakKeyEvent`)."""
        return self.text.startswith("NULL SPEECH: key event")

    @property
    def is_defunct_drop(self) -> bool:
        return self.text.startswith("EVENT MANAGER: Ignoring defunct")

    @property
    def is_event(self) -> bool:
        return self.text.startswith("EVENT MANAGER: ") and (
            self.text.endswith("is not obsoleted") or " is obsoleted by " in self.text)

    @property
    def is_locus(self) -> bool:
        """Orca moving its locus of focus, which is what it will read."""
        return self.text.startswith("FOCUS MANAGER: Changing locus of focus")

    @property
    def is_silent_result(self) -> bool:
        """Orca's speech generator finding nothing to say for an object."""
        return self.text.startswith("SPEECH GENERATOR: Results for") and \
            self.text.endswith("are pauses only")

    def to_json(self) -> dict:
        kind = ("speech" if self.spoken is not None else "stop" if self.is_stop
                else "key-echo" if self.is_key_echo
                else "defunct-drop" if self.is_defunct_drop else "locus" if self.is_locus
                else "silent" if self.is_silent_result else "event")
        record = {"stamp": self.stamp, "kind": kind, "text": self.text}
        if self.spoken is not None:
            record["spoken"] = self.spoken
        return record


KEEP = ("SPEECH OUTPUT:", "NULL SPEECH: stop", "EVENT MANAGER: Ignoring defunct",
        "EVENT MANAGER: ")

LISTENER = re.compile(r"EVENT MANAGER: registering listener for: (\S+)")
RECEIPT = re.compile(r"^(\d\d:\d\d:\d\d\.\d{6}) - EVENT MANAGER: ([a-z-]+:[a-z:-]*) for \[")


def listened(text: str) -> set[str]:
    """The event types Orca registered listeners for, from its log."""
    return set(LISTENER.findall(text))


def hears(types: set[str], event_type: str) -> bool:
    """Whether Orca, listening to `types`, receives an event of `event_type`."""
    return any(event_type == t or event_type.startswith(t if t.endswith(":") else t + ":")
               for t in types)


@dataclass
class Receipt:
    index: int
    stamp: str
    type: str
    app: str


def receipts(lines: list[str], start: int, app: str) -> list[Receipt]:
    """Orca's receipt of each event from `app`, from line `start` on: the first
    line it logs for an event, before it queues, obsoletes or ignores it."""
    found = []
    marker = f"in [application: '{app}']"
    for index in range(start, len(lines)):
        line = lines[index]
        match = RECEIPT.match(line)
        if not match:
            continue
        # A source whose name holds a newline spans lines of the log, and the
        # application marker is on a later one, before the next stamped line.
        whole = line
        follow = index + 1
        while marker not in whole and follow < len(lines) and not ORCA_LINE.match(lines[follow]):
            whole += "\n" + lines[follow]
            follow += 1
        # The same event taken from the queue is logged again with "is not
        # obsoleted" or "is obsoleted by": not a second receipt.
        if marker in whole and " obsoleted" not in whole:
            found.append(Receipt(index, match.group(1), match.group(2), app))
    return found


def last_busy(lines: list[str], start: int) -> int | None:
    """The index of the last time-stamped line from `start` on that shows Orca
    doing something, or `None`.

    Every time-stamped line counts but Orca's idle heartbeat, the
    `_onNoFocus` check it logs about every second and a quarter with nothing
    to do. The `PROCESS OBJECT EVENT` markers carry no time stamp; the lines
    between them do.
    """
    for index in range(len(lines) - 1, start - 1, -1):
        line = lines[index]
        if ORCA_LINE.match(line) and IDLE_HEARTBEAT not in line:
            return index
    return None


IDLE_HEARTBEAT = "event_manager._onNoFocus"


def lines_between(text: str, start: str, end: str) -> list[OrcaLine]:
    """Orca's log lines between two wall-clock stamps (`HH:MM:SS.ffffff`) that
    say what it would have spoken, what stopped it, and which events it took
    or dropped."""
    lines = []
    for raw in text.splitlines():
        match = ORCA_LINE.match(raw.strip())
        if not match:
            continue
        stamp, rest = match.groups()
        if not (start <= stamp <= end):
            continue
        line = OrcaLine(stamp, rest)
        if line.spoken is not None or line.is_stop or line.is_key_echo or line.is_defunct_drop \
                or line.is_event or line.is_locus or line.is_silent_result:
            lines.append(line)
    return lines


def normalized(text: str) -> str:
    """A sentence reduced to what Orca's log and the bus agree on: no spaces
    at all (the bus carries a no-break space where Orca's lines have a plain
    one or none) and no case."""
    return re.sub(r"\s+", "", text).casefold()


def fate(lines: list[OrcaLine], sentence: str) -> str:
    """What Orca did with one sentence within a window of its log.

    `spoken` when it said something containing the sentence and no stop came
    while that was being said (see `utterances`); `cut` when one did;
    `dropped` when the announcement event carrying it was followed by
    "Ignoring defunct"; `unheard` when none of these is in the log. A sentence
    said more than once is `spoken` if any saying of it was heard whole.
    """
    wanted = normalized(sentence)
    matches = [u for u in utterances(lines) if wanted in normalized(u.text)]
    if matches:
        return "spoken" if any(not u.cut for u in matches) else "cut"
    for i, line in enumerate(lines):
        if line.text.startswith("EVENT MANAGER: object:announcement") \
                and wanted in normalized(line.text):
            following = lines[i + 1:i + 3]
            if any(f.is_defunct_drop for f in following):
                return "dropped"
    return "unheard"


#: How fast a voice at Orca's default rate reads, for estimating when an
#: utterance would have ended. Orca 46.1's default rate 50 is around 180 words
#: a minute through speech-dispatcher, about 15 characters a second; this is
#: the slow side of that, so an estimate errs towards "still speaking".
CHARS_PER_SECOND = 14.0
#: What a voice takes to start and stop, on top of the characters.
UTTERANCE_OVERHEAD = 0.2


def seconds(stamp: str) -> float:
    hours, minutes, rest = stamp.split(":")
    return int(hours) * 3600 + int(minutes) * 60 + float(rest)


@dataclass
class Utterance:
    stamp: str
    text: str
    #: When it would have started and ended, in seconds since midnight, with
    #: every utterance queued behind the one before it, as speech-dispatcher
    #: queues them.
    start: float = 0.0
    end: float = 0.0
    #: A stop came while it was being said, or while it waited to be.
    cut: bool = False
    #: How far into it the stop came, in seconds, when it was cut.
    cut_after: float | None = None
    #: Orca echoing a key, not the application telling the reader anything.
    echo: bool = False

    def to_json(self) -> dict:
        record = {"stamp": self.stamp, "text": self.text, "cut": self.cut}
        if self.echo:
            record["echo"] = True
        if self.cut_after is not None:
            record["cut_after_ms"] = round(self.cut_after * 1000)
        return record


def utterances(lines: list[OrcaLine]) -> list[Utterance]:
    """What Orca said within a window, and which of it a stop cut.

    Orca hands each utterance to the speech server as it produces it, and the
    server queues it behind the one being spoken, so an utterance starts when
    the one before it ends. A stop discards what is being spoken and all that
    is queued. This is a model: the null speech server says nothing, so each
    utterance's length is estimated (`CHARS_PER_SECOND`), and an utterance
    whose estimated end falls after a stop is reported cut.
    """
    said: list[Utterance] = []
    busy_until = 0.0
    for line in lines:
        at = seconds(line.stamp)
        if line.spoken is not None:
            start = max(at, busy_until)
            end = start + UTTERANCE_OVERHEAD + len(line.spoken) / CHARS_PER_SECOND
            said.append(Utterance(line.stamp, line.spoken, start, end))
            busy_until = end
        elif line.is_key_echo:
            if said:
                said[-1].echo = True
        elif line.is_stop:
            for utterance in said:
                if not utterance.cut and utterance.end > at:
                    utterance.cut = True
                    utterance.cut_after = max(0.0, at - utterance.start)
            busy_until = at
    return said


@dataclass
class OrcaWindow:
    lines: list[OrcaLine] = field(default_factory=list)

    def spoken(self) -> list[str]:
        return [u.text for u in utterances(self.lines)]
