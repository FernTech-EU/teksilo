#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""The KDE Breeze preset as it renders in the real widget catalog, measured.

    tools/reader/kde_visual_check.sh                     # all three modes
    tools/reader/kde_visual_check.sh --modes system      # one of them
    tools/reader/kde_visual_check.sh --out DIR           # keep logs and PNGs

Run it through `kde_visual_check.sh`, which builds the debug `widget-catalog`
and `teksilo-automation-mcp` into this checkout's `target/` and then starts
this driver inside `private_session.sh`: a private D-Bus bus, runtime
directory, `kwin_wayland --virtual` and configuration home, so neither the
catalog nor `plasma-apply-colorscheme` touches the desktop or the user's
`~/.config`. Started any other way, the driver refuses to run.

For each mode, `kde-light`, `kde-dark` and `system`:

1. The private configuration home is given a Breeze colour scheme by the
   installed `plasma-apply-colorscheme` (BreezeLight for `kde-light` and
   `system`, BreezeDark for `kde-dark`), and the catalog runs with
   `XDG_CURRENT_DESKTOP=KDE` and `KDE_FULL_SESSION=true`, so the native path
   takes the session for a Breeze desktop.
2. `widget-catalog --theme <mode>` starts; the driver reads its automation
   bridge's endpoint descriptor under the private runtime directory and
   attaches `teksilo-automation-mcp --connect`.
3. It opens the catalog tabs the controls live on and measures each control
   with `layout_tree`, found by its Rust type name: a push button, a line
   edit's frame, a combo box, a spin box, a check box row, a list row and a
   tree row, a table header and a table row, a row of a menu opened from a
   menu bar, a tab of a horizontal tab bar and a scroll bar's extent. Every
   height is printed beside the Breeze value Plasma 6.8 reports
   (`tools/kde_golden/vectors/sizes.json`, the entry named in the table), the
   specification's computed size (`engineering/docs/kde-breeze-spec.md`
   k.4, at Plasma's default 10 pt font) and the height the preset means to
   give it, with the documented deviation where there is one. A control
   passes when it is within 1 px of the preset's height. The catalog is built
   at the Compact density, where the preset's density ladder leaves every
   Breeze height as it is (`[unpublished]`: KDE publishes no touch ladder).
   It also reads which entry the title-bar theme switcher shows (the bridge
   reports no theme id; the switcher names `kde.light` "KDE Light",
   `kde.dark` "KDE Dark" and follow-the-desktop "System"), and checks that
   the push button is painted by the preset's own chrome
   (`teksilo_theme_kde::styles::chrome::BreezeButtonChrome`), which in
   `system` mode is what tells Breeze's chrome from Int UI's (Int UI's push
   button is 24 px high, Breeze's 34).
4. In `system` mode only, it then changes the desktop while the catalog
   runs: `plasma-apply-colorscheme BreezeDark`, then the accent `#e93a9a`.
   After each it polls, with a deadline, screenshots of a push button and of
   the selected tab of the catalog's tab bar until they change (the desktop
   watch reports a change 150 ms after the signal), and checks that the
   button turned darker and that the selected tab's changed pixels moved
   toward the new accent. Then it checks that nothing was rebuilt: the
   `layout_tree` node ids and the bounds of every active node are the ones
   measured before the change.

Some heights are not the preset's to give, and are printed as INFO rather
than judged: the catalog's `TableView` and `TreeView` set their row heights
themselves (28 px), and a `ListView` lays its rows out at its own uniform
item height (32 px) whatever the style. The preset's table row height (24,
D7) therefore has no table in the catalog to show it, and no list or tree
row in the catalog has an icon or a check box; `NOT_IN_CATALOG` names both.

Exit status: 0 when every judged control passes, 1 on a failed check (the
reason is printed), 2 when the driver cannot run (not a private session,
binaries missing).
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import select
import shutil
import subprocess
import sys
import tempfile
import time
import zlib
from dataclasses import dataclass
from pathlib import Path

READER_DIR = Path(__file__).resolve().parent
REPO = READER_DIR.parents[1]
TARGET = REPO / "target" / "debug"
CATALOG = TARGET / "widget-catalog"
MCP = TARGET / "teksilo-automation-mcp"
SIZES = REPO / "tools" / "kde_golden" / "vectors" / "sizes.json"

MODES = ("kde-light", "kde-dark", "system")
#: The scheme each mode's desktop is seeded with.
SEED_SCHEME = {"kde-light": "BreezeLight", "kde-dark": "BreezeDark", "system": "BreezeLight"}
#: What the title-bar theme switcher shows for each mode's theme.
SWITCHER_ENTRY = {"kde-light": "KDE Light", "kde-dark": "KDE Dark", "system": "System"}
#: The accent the system mode applies while the catalog runs: Plasma's pink
#: swatch (specification, section h).
LIVE_ACCENT = "#e93a9a"

#: How long a launch, a tab switch, a menu or a repaint is waited for.
LAUNCH_DEADLINE = 90.0
STEP_DEADLINE = 20.0
REPAINT_DEADLINE = 15.0
POLL = 0.1
#: The tolerance of a judged height, in logical pixels.
TOLERANCE = 1.0

BUTTON = "teksilo_widgets::button::Button"
BREEZE_BUTTON_CHROME = "teksilo_theme_kde::styles::chrome::BreezeButtonChrome"
TEXT_INPUT = "teksilo_widgets::text_input::TextInput"
FIELD_CHROME = "teksilo_theme_kde::styles::text_input::BreezeFieldChrome"
COMBO = "teksilo_widgets::combo_box::ComboBox<"
SPIN = "teksilo_widgets::spin_box::SpinBox<"
CHECKBOX = "teksilo_widgets::checkbox::Checkbox"
LIST_ITEM = "teksilo_widgets::standard_item::StandardListItem"
TREE_ITEM = "teksilo_widgets::standard_item::StandardTreeItem"
LIST_ROW_WRAPPER = "teksilo_widgets::list_item_a11y::ListItemWrapper"
TREE_ROW_WRAPPER = "teksilo_widgets::list_item_a11y::TreeItemWrapper"
TABLE_VIEW = "teksilo_widgets::table_view::TableView<"
TABLE_HEADER = "teksilo_widgets::table_view::table_header::TableHeader"
TABLE_ROW = "teksilo_widgets::table_view::body::BodyRow"
MENU_BAR_TRIGGER = "teksilo_widgets::menu_bar::MenuBarTrigger"
MENU_ITEM = "teksilo_widgets::menu_item::MenuItem"
TAB_HEADER = "teksilo_widgets::tab_widget::header::TabHeader"
SCROLL_AREA = "teksilo_widgets::scroll_area::ScrollArea"
SCROLL_BAR = "teksilo_widgets::scroll_bar::ScrollBar"
MAX_SIZE = "teksilo_widgets::primitives::max_size::MaxSize"
TAB_CONTENT = "widget_catalog::TabContent"

#: Controls the catalog has no instance of, with the reason. A control listed
#: here is reported, never silently skipped; any other control that cannot be
#: found fails the check.
NOT_IN_CATALOG = {
    "list row with an icon or a check box": (
        "no StandardListItem or StandardTreeItem in the widget catalog has a leading "
        "icon or a check box (tabs/data.rs and tabs/touch.rs build text-only rows), so "
        "Breeze's 26 px row (list_view.icon, list_view.check) has nothing to compare with"
    ),
    "table row at the preset's height": (
        "both tables in the catalog set their own row height (tabs/data.rs: TableView "
        ".row_height(28.0), TreeTableView .row_height(26.0)), so the style's 24 px row "
        "(D7, against Breeze's 23 with its grid line) is never laid out there; the "
        "TableView row is measured as INFO at the height the catalog asks for"
    ),
}


class CheckFailed(Exception):
    """A failed check, with the reason."""


class CannotRun(Exception):
    """The driver cannot run here."""


# ---------------------------------------------------------------------------
# The session
# ---------------------------------------------------------------------------


def user_config_home() -> Path:
    import pwd

    return Path(pwd.getpwuid(os.getuid()).pw_dir) / ".config"


def check_private_session() -> Path:
    """The private configuration home, after refusing anything else."""
    if os.environ.get("TEKSILO_READER_INNER") != "1":
        raise CannotRun("not inside tools/reader/private_session.sh; run kde_visual_check.sh")
    runtime = os.environ.get("XDG_RUNTIME_DIR", "")
    if not Path(runtime).name.startswith("teksilo-reader-run."):
        raise CannotRun(f"the runtime directory {runtime!r} is not the private session's")
    if os.environ.get("DISPLAY"):
        raise CannotRun("DISPLAY is set: this is not the private session")
    raw = os.environ.get("XDG_CONFIG_HOME", "")
    if not raw:
        raise CannotRun("XDG_CONFIG_HOME is unset: plasma-apply-colorscheme would write ~/.config")
    config = Path(raw).resolve()
    if config == user_config_home().resolve():
        raise CannotRun(f"XDG_CONFIG_HOME is the user's own {config}")
    if not config.is_relative_to(Path(runtime).resolve()):
        raise CannotRun(f"XDG_CONFIG_HOME {config} is not inside the private runtime directory")
    return config


def stat_or_none(path: Path) -> tuple | None:
    try:
        st = path.stat()
    except FileNotFoundError:
        return None
    return (st.st_mtime_ns, st.st_size, st.st_ino)


@dataclass
class Desktop:
    """The private desktop's KDE configuration, written by Plasma's own tool."""

    config: Path
    user_kdeglobals: Path
    user_stat: tuple | None

    @classmethod
    def open(cls, config: Path) -> "Desktop":
        if shutil.which("plasma-apply-colorscheme") is None:
            raise CannotRun("plasma-apply-colorscheme is not installed")
        user = user_config_home() / "kdeglobals"
        return cls(config, user, stat_or_none(user))

    @property
    def kdeglobals(self) -> Path:
        return self.config / "kdeglobals"

    def env(self) -> dict[str, str]:
        env = dict(os.environ, XDG_CONFIG_HOME=str(self.config))
        # The tool needs no window; it would otherwise try the private
        # compositor and an X display that does not exist.
        env["QT_QPA_PLATFORM"] = "offscreen"
        return env

    def run_tool(self, *args: str) -> None:
        proc = subprocess.run(["plasma-apply-colorscheme", *args], env=self.env(),
                              capture_output=True, text=True, timeout=60)
        if proc.returncode != 0:
            raise CheckFailed(f"plasma-apply-colorscheme {' '.join(args)} failed "
                              f"({proc.returncode}): {proc.stdout.strip()} {proc.stderr.strip()}")
        self.guard_user_config()

    def guard_user_config(self) -> None:
        if stat_or_none(self.user_kdeglobals) != self.user_stat:
            raise CheckFailed(f"{self.user_kdeglobals} changed during the run")

    def general(self) -> dict[str, str]:
        group: dict[str, str] = {}
        current = None
        try:
            text = self.kdeglobals.read_text()
        except FileNotFoundError:
            return group
        for line in text.splitlines():
            line = line.strip()
            if line.startswith("["):
                current = line
            elif current == "[General]" and "=" in line:
                key, value = line.split("=", 1)
                group[key.strip()] = value.strip()
        return group

    def seed(self, scheme: str) -> None:
        """Start the desktop afresh on `scheme`, at its own accent."""
        for name in ("kdeglobals", "kdedefaults"):
            path = self.config / name
            if path.is_dir():
                shutil.rmtree(path)
            elif path.exists():
                path.unlink()
        # With no kdeglobals the tool takes the system default (the Look and
        # Feel's scheme, from XDG_CONFIG_DIRS, which differs between machines)
        # for the scheme in use, and writes nothing when asked for that one. Going
        # through the other Breeze scheme first writes `scheme` whatever the
        # default is.
        self.run_tool("BreezeDark" if scheme == "BreezeLight" else "BreezeLight")
        self.run_tool(scheme)
        self.expect_scheme(scheme)

    def expect_scheme(self, scheme: str) -> None:
        found = self.general().get("ColorScheme")
        if found != scheme:
            raise CheckFailed(f"the private {self.kdeglobals} names the scheme {found!r}, "
                              f"not {scheme!r}")


# ---------------------------------------------------------------------------
# The catalog and its bridge
# ---------------------------------------------------------------------------


class Bridge:
    """The catalog and the MCP server attached to its automation bridge."""

    def __init__(self, args: list[str], env: dict[str, str], log_dir: Path, name: str):
        self.log_path = log_dir / f"{name}-catalog.log"
        self.mcp_log_path = log_dir / f"{name}-mcp.log"
        self.mcp: subprocess.Popen | None = None
        self._id = 0
        with open(self.log_path, "w") as log:
            self.app = subprocess.Popen([str(CATALOG), *args], stdout=log,
                                        stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL,
                                        env=env)
        try:
            self._attach(env)
        except BaseException:
            self.close()
            raise

    def _attach(self, env: dict[str, str]) -> None:
        descriptor = Path(env["XDG_RUNTIME_DIR"]) / "teksilo-automation" / f"{self.app.pid}.json"
        deadline = time.monotonic() + LAUNCH_DEADLINE
        desc = None
        while desc is None:
            if self.app.poll() is not None:
                raise CheckFailed(f"the catalog exited ({self.app.returncode}) before "
                                  f"publishing its bridge; its log is {self.log_path}")
            if time.monotonic() > deadline:
                raise CheckFailed(f"no readable bridge descriptor at {descriptor} within "
                                  f"{LAUNCH_DEADLINE:.0f} s (is the catalog a debug build?)")
            try:
                desc = json.loads(descriptor.read_text())
            except (OSError, ValueError):
                time.sleep(POLL)
        with open(self.mcp_log_path, "w") as log:
            self.mcp = subprocess.Popen(
                [str(MCP), "--connect", desc["address"], "--token", desc["token"]],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log,
                text=True, bufsize=1, env=env)
        self._send("initialize", {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "kde_visual_check", "version": "1"},
        })
        self._recv(30)
        self._send("notifications/initialized", notify=True)

    def close(self) -> None:
        for proc in (self.mcp, self.app):
            if proc is not None and proc.poll() is None:
                proc.terminate()
        for proc in (self.mcp, self.app):
            if proc is None:
                continue
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()

    def _send(self, method: str, params=None, notify: bool = False) -> None:
        msg: dict = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        if not notify:
            self._id += 1
            msg["id"] = self._id
        assert self.mcp is not None and self.mcp.stdin is not None
        self.mcp.stdin.write(json.dumps(msg) + "\n")
        self.mcp.stdin.flush()

    def _recv(self, timeout: float) -> dict:
        assert self.mcp is not None and self.mcp.stdout is not None
        end = time.monotonic() + timeout
        while True:
            left = end - time.monotonic()
            if left <= 0 or self.mcp.poll() is not None:
                raise CheckFailed(f"no reply from the MCP server; its log is {self.mcp_log_path}")
            ready, _, _ = select.select([self.mcp.stdout], [], [], left)
            if not ready:
                continue
            line = self.mcp.stdout.readline()
            if not line:
                raise CheckFailed(f"the MCP server closed its output; its log is "
                                  f"{self.mcp_log_path}")
            if not line.strip():
                continue
            msg = json.loads(line)
            if msg.get("id") == self._id:
                return msg

    def call(self, name: str, args: dict | None = None, timeout: float = 60) -> tuple[dict, dict]:
        """One tool call: the raw result and its JSON payload."""
        self._send("tools/call", {"name": name, "arguments": args or {}})
        msg = self._recv(timeout)
        if "error" in msg:
            raise CheckFailed(f"{name}: {msg['error']}")
        result = msg.get("result", {})
        payload = result.get("structuredContent")
        if payload is None:
            text = "".join(c.get("text", "") for c in result.get("content", [])
                           if c.get("type") == "text")
            try:
                payload = json.loads(text) if text.strip() else {}
            except ValueError:
                payload = {"text": text}
        if result.get("isError"):
            raise CheckFailed(f"{name} {args or {}}: {payload}")
        return result, payload

    # -- views of the tree ----------------------------------------------------

    def layout(self) -> "Layout":
        _, payload = self.call("layout_tree")
        return Layout(payload.get("nodes", []))

    def at_nodes(self) -> list[dict]:
        _, payload = self.call("snapshot_tree")
        return payload.get("nodes", [])

    def at_node(self, role: str, label: str) -> dict:
        found = [n for n in self.at_nodes() if n.get("role") == role and n.get("label") == label]
        if not found:
            raise CheckFailed(f"no {role} node labelled {label!r} in the accessibility tree")
        return found[0]

    def screenshot(self, node: int | None = None) -> "Image":
        res, _ = self.call("screenshot", {"node": node} if node is not None else {})
        for c in res.get("content", []):
            if c.get("type") == "image" and c.get("data"):
                return Image.from_png(base64.b64decode(c["data"]))
        raise CheckFailed(f"the screenshot of node {node} carried no image")


class Layout:
    """One `layout_tree` reply, indexed."""

    def __init__(self, nodes: list[dict]):
        self.nodes = nodes
        self.by_id = {n["id"]: n for n in nodes}

    def descendants(self, root: int):
        stack = [root]
        while stack:
            node = self.by_id[stack.pop()]
            yield node
            stack.extend(reversed(node.get("children", [])))

    def ancestors(self, node: dict):
        parent = node.get("parent")
        while parent is not None and parent in self.by_id:
            node = self.by_id[parent]
            yield node
            parent = node.get("parent")

    def active_tab_content(self) -> dict:
        found = [n for n in self.nodes if n["type"] == TAB_CONTENT and n["active"]]
        if len(found) != 1:
            raise CheckFailed(f"{len(found)} active catalog tab contents, expected one")
        return found[0]

    def find(self, root: int | None, pred, what: str) -> list[dict]:
        """Every active, laid-out node under `root` (or anywhere) matching `pred`."""
        pool = self.descendants(root) if root is not None else iter(self.nodes)
        found = [n for n in pool if n["active"] and n["bounds"]["height"] > 0
                 and n["bounds"]["width"] > 0 and pred(n)]
        if not found:
            raise CheckFailed(f"no {what} in the layout tree")
        return found


def is_type(prefix: str):
    return lambda n: n["type"] == prefix or (prefix.endswith("<") and n["type"].startswith(prefix))


# ---------------------------------------------------------------------------
# Screenshots
# ---------------------------------------------------------------------------


class Image:
    """An 8-bit RGB or RGBA PNG, decoded."""

    def __init__(self, width: int, height: int, pixels: list[tuple[int, int, int]]):
        self.width, self.height, self.pixels = width, height, pixels

    @classmethod
    def from_png(cls, data: bytes) -> "Image":
        if data[:8] != b"\x89PNG\r\n\x1a\n":
            raise CheckFailed("the screenshot is not a PNG")
        pos, idat, header = 8, b"", None
        while pos < len(data):
            length = int.from_bytes(data[pos:pos + 4], "big")
            kind = data[pos + 4:pos + 8]
            body = data[pos + 8:pos + 8 + length]
            pos += 12 + length
            if kind == b"IHDR":
                header = body
            elif kind == b"IDAT":
                idat += body
            elif kind == b"IEND":
                break
        if header is None:
            raise CheckFailed("the screenshot PNG has no header")
        width = int.from_bytes(header[0:4], "big")
        height = int.from_bytes(header[4:8], "big")
        depth, color, interlace = header[8], header[9], header[12]
        channels = {2: 3, 6: 4}.get(color)
        if depth != 8 or channels is None or interlace != 0:
            raise CheckFailed(f"unsupported screenshot PNG (depth {depth}, colour {color})")
        raw = zlib.decompress(idat)
        stride = width * channels
        prev = bytearray(stride)
        pixels: list[tuple[int, int, int]] = []
        at = 0
        for _ in range(height):
            kind = raw[at]
            line = bytearray(raw[at + 1:at + 1 + stride])
            at += 1 + stride
            for i in range(stride):
                left = line[i - channels] if i >= channels else 0
                up = prev[i]
                ul = prev[i - channels] if i >= channels else 0
                if kind == 1:
                    line[i] = (line[i] + left) & 255
                elif kind == 2:
                    line[i] = (line[i] + up) & 255
                elif kind == 3:
                    line[i] = (line[i] + ((left + up) >> 1)) & 255
                elif kind == 4:
                    p = left + up - ul
                    pa, pb, pc = abs(p - left), abs(p - up), abs(p - ul)
                    pred = left if pa <= pb and pa <= pc else (up if pb <= pc else ul)
                    line[i] = (line[i] + pred) & 255
            pixels.extend(tuple(line[i:i + 3]) for i in range(0, stride, channels))
            prev = line
        return cls(width, height, pixels)

    def changed(self, other: "Image", threshold: int = 24) -> list[int]:
        """The indices of the pixels that differ by more than `threshold` levels."""
        if (self.width, self.height) != (other.width, other.height):
            return list(range(len(self.pixels)))
        return [i for i, (a, b) in enumerate(zip(self.pixels, other.pixels))
                if max(abs(a[k] - b[k]) for k in range(3)) > threshold]

    def mean(self, indices: list[int] | None = None) -> tuple[float, float, float]:
        pool = [self.pixels[i] for i in indices] if indices else self.pixels
        n = max(len(pool), 1)
        return tuple(sum(p[k] for p in pool) / n for k in range(3))  # type: ignore[return-value]


def luma(rgb: tuple[float, float, float]) -> float:
    return 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]


# ---------------------------------------------------------------------------
# References
# ---------------------------------------------------------------------------


class Breeze:
    """The heights Plasma 6.8 reports, from the Docker oracle's sizes.json."""

    def __init__(self, path: Path):
        data = json.loads(path.read_text())
        self.widgets = {w["id"]: w for w in data["widgets"] if w.get("font") == "Noto Sans 10"}

    def widget(self, wid: str) -> dict:
        if wid not in self.widgets:
            raise CannotRun(f"{SIZES} has no {wid!r} entry at Noto Sans 10")
        return self.widgets[wid]

    def hint_height(self, wid: str) -> tuple[float, str]:
        return self.widget(wid)["size_hint"][1], f"{wid} size_hint h"

    def hint_width(self, wid: str) -> tuple[float, str]:
        return self.widget(wid)["size_hint"][0], f"{wid} size_hint w"

    def row(self, wid: str) -> tuple[float, str]:
        return self.widget(wid)["size_hint_for_row"], f"{wid} size_hint_for_row"

    def menu_item(self) -> tuple[float, str]:
        rows = [r for r in self.widget("menu.with_icon")["rows"] if r["kind"] == "item"]
        return rows[0]["geometry"][3], "menu.with_icon rows[item] h"

    def tab(self) -> tuple[float, str]:
        return self.widget("tab_bar")["tab_rects"][0][3], "tab_bar tab_rects[0] h"


@dataclass
class Spec:
    """One control's expectations."""

    control: str
    breeze: tuple[float, str]
    #: The specification's k.4 size at H = 18, as text (it can carry a note).
    spec_k4: str
    #: The height the preset gives it, or None for an INFO row.
    intended: float | None
    note: str


@dataclass
class Row:
    spec: Spec
    node: dict
    measured: float

    @property
    def status(self) -> str:
        if self.spec.intended is None:
            return "INFO"
        return "PASS" if abs(self.measured - self.spec.intended) <= TOLERANCE else "FAIL"


def fmt(v: float) -> str:
    return f"{v:g}" if v == int(v) else f"{v:.2f}"


# ---------------------------------------------------------------------------
# The check
# ---------------------------------------------------------------------------


class ModeCheck:
    def __init__(self, mode: str, desktop: Desktop, breeze: Breeze, out: Path):
        self.mode, self.desktop, self.breeze, self.out = mode, desktop, breeze, out
        self.rows: list[Row] = []
        self.notes: list[str] = []
        self.failures: list[str] = []
        self.bridge: Bridge | None = None

    # -- driving ---------------------------------------------------------------

    def wait_for(self, what: str, probe, deadline: float = STEP_DEADLINE):
        """`probe()`'s first value that is not None, polled until `deadline`."""
        end = time.monotonic() + deadline
        last: Exception | None = None
        while time.monotonic() < end:
            try:
                value = probe()
            except CheckFailed as e:
                last = e
                value = None
            if value is not None:
                return value
            time.sleep(POLL)
        raise CheckFailed(f"{what} within {deadline:.0f} s" + (f" ({last})" if last else ""))

    def open_tab(self, label: str) -> Layout:
        """Select the catalog tab `label` and return the layout once it shows."""
        b = self.bridge
        assert b is not None
        tab = b.at_node("Tab", label)
        b.call("invoke_action", {"node": tab["id"], "action": "click"})

        def shown():
            layout = b.layout()
            content = layout.active_tab_content()
            # The tab header the click selected sits in the bar; the content
            # it shows is laid out when its ScrollArea has a size.
            areas = [n for n in layout.descendants(content["id"])
                     if n["type"] == SCROLL_AREA and n["active"]
                     and n.get("parent") == content["id"] and n["bounds"]["height"] > 0]
            selected = b.at_node("Tab", label).get("selected")
            return layout if areas and selected else None

        return self.wait_for(f"the {label} tab did not show", shown)

    def measure(self, spec: Spec, node: dict) -> None:
        self.rows.append(Row(spec, node, node["bounds"]["height"]))

    # -- one mode --------------------------------------------------------------

    def run(self) -> None:
        self.desktop.seed(SEED_SCHEME[self.mode])
        env = dict(os.environ, XDG_CURRENT_DESKTOP="KDE", KDE_FULL_SESSION="true")
        self.bridge = Bridge(["--theme", self.mode, "--tab", "buttons"], env, self.out, self.mode)
        try:
            self.measure_all()
            if self.mode == "system":
                self.live_change()
        finally:
            self.bridge.close()
            self.bridge = None
        self.desktop.guard_user_config()

    def measure_all(self) -> None:
        b = self.bridge
        assert b is not None
        br = self.breeze

        # The theme switcher's entry, and Breeze's chrome on the push button.
        combo = self.wait_for("the title-bar theme switcher did not appear",
                              lambda: next((n for n in b.at_nodes()
                                            if n.get("role") == "ComboBox"
                                            and n.get("label") == "Theme"), None),
                              LAUNCH_DEADLINE)
        entry = combo.get("value")
        self.notes.append(f"theme switcher shows {entry!r} (expected {SWITCHER_ENTRY[self.mode]!r})")
        if entry != SWITCHER_ENTRY[self.mode]:
            self.failures.append(f"the theme switcher shows {entry!r}, not "
                                 f"{SWITCHER_ENTRY[self.mode]!r}")

        layout = self.open_tab("Buttons")
        tab = layout.active_tab_content()["id"]
        button = layout.find(tab, is_type(BUTTON), "push button on the Buttons tab")[0]
        chrome = [n for n in layout.descendants(button["id"]) if n["type"] == BREEZE_BUTTON_CHROME]
        self.notes.append(f"push button painted by {BREEZE_BUTTON_CHROME}: "
                          f"{'yes' if chrome else 'NO'}")
        if not chrome:
            self.failures.append("the push button is not painted by the Breeze button chrome")
        self.measure(Spec("push button", br.hint_height("push_button"), "34", 34,
                          "Breeze's H + 16"), button)

        layout = self.open_tab("Text")
        tab = layout.active_tab_content()["id"]
        field = layout.find(tab, is_type(TEXT_INPUT), "TextInput on the Text tab")[0]
        frames = layout.find(field["id"], is_type(FIELD_CHROME), "Breeze line-edit frame "
                             "in the TextInput")
        self.measure(Spec("line edit (its frame)", br.hint_height("line_edit"), "32", 32,
                          f"TextInput node {fmt(field['bounds']['height'])} with its "
                          "validation line"), frames[0])
        spin = layout.find(tab, is_type(SPIN), "SpinBox on the Text tab")[0]
        self.measure(Spec("spin box", br.hint_height("spin_box.0_99"), "32", 32, ""), spin)

        layout = self.open_tab("Inputs")
        tab = layout.active_tab_content()["id"]
        combo_node = layout.find(tab, is_type(COMBO), "ComboBox on the Inputs tab")[0]
        self.measure(Spec("combo box", br.hint_height("combo_box"), "32", 32, ""), combo_node)
        check = layout.find(tab, is_type(CHECKBOX), "Checkbox on the Inputs tab")[0]
        self.measure(Spec("check box row", br.hint_height("check_box"), "22", 24,
                          "D8: the 24 px target, the 20 px indicator unchanged"), check)

        layout = self.open_tab("Data")
        tab = layout.active_tab_content()["id"]

        def outside(wrapper: str):
            return lambda n: not any(a["type"] == wrapper for a in layout.ancestors(n))

        item = layout.find(tab, lambda n: is_type(LIST_ITEM)(n) and outside(LIST_ROW_WRAPPER)(n),
                           "StandardListItem outside a ListView on the Data tab")[0]
        self.measure(Spec("list row, text", br.row("list_view"), "24", 24,
                          "a view's first row is not one taller"), item)
        tree = layout.find(tab, lambda n: is_type(TREE_ITEM)(n) and outside(TREE_ROW_WRAPPER)(n),
                           "StandardTreeItem outside a TreeView on the Data tab")[0]
        self.measure(Spec("tree row, text", br.row("tree_view"), "24", 24, ""), tree)
        lv_row = layout.find(tab, is_type(LIST_ROW_WRAPPER), "ListView row on the Data tab")[0]
        self.measure(Spec("ListView row", br.row("list_view"), "24", None,
                          "the ListView's own uniform item height, not the style's"), lv_row)
        tv_row = layout.find(tab, is_type(TREE_ROW_WRAPPER), "TreeView row on the Data tab")[0]
        self.measure(Spec("TreeView row", br.row("tree_view"), "24", None,
                          "set by the catalog: .item_height(28.0)"), tv_row)
        table = layout.find(tab, is_type(TABLE_VIEW), "TableView on the Data tab")[0]
        header = layout.find(table["id"], is_type(TABLE_HEADER), "TableView header")[0]
        self.measure(Spec("table header", br.hint_height("header.horizontal"), "30", 30, ""),
                     header)
        table_row = layout.find(table["id"], is_type(TABLE_ROW), "TableView row")[0]
        self.measure(Spec("table row", br.row("table_view"), "22 (23 with the grid)", None,
                          "set by the catalog: .row_height(28.0); the preset's is 24 (D7)"),
                     table_row)

        layout = self.open_tab("Containers")
        tab = layout.active_tab_content()["id"]
        headers = layout.find(tab, lambda n: is_type(TAB_HEADER)(n)
                              and n["bounds"]["width"] > n["bounds"]["height"],
                              "horizontal tab on the Containers tab")
        self.measure(Spec("tab", br.tab(), "30", 30, "Compact; Breeze's static-bar 34 is "
                          "never used"), headers[0])
        demo_area = layout.find(tab, lambda n: is_type(SCROLL_AREA)(n)
                                and layout.by_id.get(n.get("parent"), {}).get("type") == MAX_SIZE,
                                "ScrollArea demo on the Containers tab")[0]
        bar = layout.find(demo_area["id"], lambda n: is_type(SCROLL_BAR)(n)
                          and n.get("parent") == demo_area["id"],
                          "scroll bar of the ScrollArea demo")[0]
        self.rows.append(Row(Spec("scroll bar extent (ScrollArea)",
                                  br.hint_width("scroll_bar.vertical"), "21", 12,
                                  "documented: a ScrollArea lays its bars out 12 px"),
                             bar, min(bar["bounds"]["width"], bar["bounds"]["height"])))
        standalone = layout.find(tab, lambda n: is_type(SCROLL_BAR)(n)
                                 and layout.by_id.get(n.get("parent"), {}).get("type") == MAX_SIZE,
                                 "standalone ScrollBar on the Containers tab")[0]
        self.rows.append(Row(Spec("scroll bar extent (ScrollBar)",
                                  br.hint_height("scroll_bar.horizontal"), "21", 8,
                                  "documented: a ScrollBar is 8 px unless the app sizes it"),
                             standalone, min(standalone["bounds"]["width"],
                                             standalone["bounds"]["height"])))

        layout = self.open_tab("Menus")
        tab = layout.active_tab_content()["id"]
        trigger = layout.find(tab, is_type(MENU_BAR_TRIGGER), "menu bar trigger on the Menus tab")[0]
        b.call("inject_pointer", {"node": trigger["id"],
                                  "x": trigger["bounds"]["width"] / 2,
                                  "y": trigger["bounds"]["height"] / 2})

        def popup_rows():
            lay = b.layout()
            inside = {n["id"] for n in lay.descendants(lay.active_tab_content()["id"])}
            rows = [n for n in lay.nodes if n["type"] == MENU_ITEM and n["active"]
                    and n["id"] not in inside and n["bounds"]["height"] > 0]
            return rows or None

        rows = self.wait_for("no menu opened from the Menus tab's menu bar", popup_rows)
        self.measure(Spec("menu item (opened menu)", br.menu_item(), "29", 29,
                          "Teksilo reserves the icon column on every row"), rows[0])
        b.call("inject_key", {"key": "Escape"})
        self.wait_for("the menu did not close", lambda: None if popup_rows() else True)

    # -- live change (system mode) ---------------------------------------------

    def live_change(self) -> None:
        b = self.bridge
        assert b is not None
        layout = self.open_tab("Buttons")
        tab = layout.active_tab_content()["id"]
        button = layout.find(tab, is_type(BUTTON), "push button on the Buttons tab")[0]
        selected = b.at_node("Tab", "Buttons")
        before = b.layout()
        shot_button = b.screenshot(button["id"])
        shot_tab = b.screenshot(selected["id"])

        def repainted(node: int, reference: Image):
            def probe():
                img = b.screenshot(node)
                return img if len(reference.changed(img)) >= 20 else None
            return probe

        start = time.monotonic()
        self.desktop.run_tool("BreezeDark")
        self.desktop.expect_scheme("BreezeDark")
        dark_button = self.wait_for("the push button was not repainted after the scheme "
                                    "changed to BreezeDark", repainted(button["id"], shot_button),
                                    REPAINT_DEADLINE)
        took = time.monotonic() - start
        l0, l1 = luma(shot_button.mean()), luma(dark_button.mean())
        self.notes.append(f"BreezeDark: push button repainted after {took:.2f} s, mean luma "
                          f"{l0:.1f} -> {l1:.1f}, {len(shot_button.changed(dark_button))} of "
                          f"{len(dark_button.pixels)} pixels changed")
        if not l1 < l0 - 30:
            self.failures.append(f"the push button did not turn dark (mean luma {l0:.1f} -> "
                                 f"{l1:.1f})")
        dark_tab = b.screenshot(selected["id"])

        start = time.monotonic()
        self.desktop.run_tool("-a", LIVE_ACCENT)
        accent_tab = self.wait_for("the selected tab was not repainted after the accent "
                                   f"changed to {LIVE_ACCENT}", repainted(selected["id"], dark_tab),
                                   REPAINT_DEADLINE)
        took = time.monotonic() - start
        moved = dark_tab.changed(accent_tab)
        r0, g0, _ = dark_tab.mean(moved)
        r1, g1, _ = accent_tab.mean(moved)
        self.notes.append(f"accent {LIVE_ACCENT}: selected tab repainted after {took:.2f} s, "
                          f"{len(moved)} pixels changed, their mean red {r0:.0f} -> {r1:.0f}, "
                          f"green {g0:.0f} -> {g1:.0f}")
        if not (r1 > r0 and g1 < g0):
            self.failures.append("the selected tab's changed pixels did not move toward the "
                                 f"accent {LIVE_ACCENT}")
        if not len(shot_tab.changed(accent_tab)) >= 20:
            self.failures.append("the selected tab looks as it did before both changes")

        after = b.layout()
        ids0, ids1 = set(before.by_id), set(after.by_id)
        if ids0 != ids1:
            self.failures.append(f"the layout tree was rebuilt: {len(ids0 - ids1)} nodes gone, "
                                 f"{len(ids1 - ids0)} new")
        moved_nodes = [i for i, n in before.by_id.items()
                       if n["active"] and after.by_id.get(i, {}).get("bounds") != n["bounds"]]
        if moved_nodes:
            self.failures.append(f"{len(moved_nodes)} active nodes changed bounds, first "
                                 f"{moved_nodes[0]}")
        self.notes.append(f"no rebuild: {len(ids1)} layout node ids before and after "
                          f"({'same' if ids0 == ids1 else 'DIFFERENT'}), "
                          f"{len(moved_nodes)} active nodes with other bounds")

    # -- report ----------------------------------------------------------------

    def report(self) -> str:
        head = ("control", "measured", "Breeze 6.8 (sizes.json entry)", "spec k.4",
                "preset", "deviation / note", "result")
        lines = []
        for r in self.rows:
            lines.append((
                r.spec.control,
                fmt(r.measured),
                f"{fmt(r.spec.breeze[0])} ({r.spec.breeze[1]})",
                r.spec.spec_k4,
                "-" if r.spec.intended is None else fmt(r.spec.intended),
                r.spec.note,
                r.status,
            ))
        # A mode that failed before measuring anything still prints its head.
        widths = [max([len(h)] + [len(row[i]) for row in lines]) for i, h in enumerate(head)]
        sep = "  "
        out = [f"== {self.mode} ==",
               sep.join(h.ljust(w) for h, w in zip(head, widths)),
               sep.join("-" * w for w in widths)]
        out += [sep.join(c.ljust(w) for c, w in zip(row, widths)).rstrip() for row in lines]
        out += [f"note: {n}" for n in self.notes]
        for control, reason in NOT_IN_CATALOG.items():
            out.append(f"not in catalog: {control}: {reason}")
        return "\n".join(out)

    def ok(self) -> bool:
        return not self.failures and all(r.status != "FAIL" for r in self.rows)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--modes", default=",".join(MODES),
                    help="comma-separated modes, from: " + ", ".join(MODES))
    ap.add_argument("--out", type=Path,
                    help="keep the catalog and MCP logs here (default: discarded with the "
                         "private session)")
    args = ap.parse_args(argv)
    modes = [m for m in args.modes.split(",") if m]
    unknown = [m for m in modes if m not in MODES]
    if unknown:
        print(f"kde_visual_check: unknown mode {', '.join(unknown)}", file=sys.stderr)
        return 2
    try:
        config = check_private_session()
        missing = [str(p) for p in (CATALOG, MCP) if not p.exists()]
        if missing:
            raise CannotRun("missing binaries (run kde_visual_check.sh, which builds them): "
                            + ", ".join(missing))
        desktop = Desktop.open(config)
        breeze = Breeze(SIZES)
    except CannotRun as e:
        print(f"kde_visual_check: cannot run: {e}", file=sys.stderr)
        return 2
    out = args.out.resolve() if args.out else Path(tempfile.mkdtemp(
        prefix="kde-visual-check.", dir=os.environ["XDG_RUNTIME_DIR"]))
    out.mkdir(parents=True, exist_ok=True)

    failed = []
    for mode in modes:
        check = ModeCheck(mode, desktop, breeze, out)
        try:
            check.run()
        except CheckFailed as e:
            check.failures.append(str(e))
        print(check.report())
        for f in check.failures:
            print(f"FAIL: {f}")
        for r in check.rows:
            if r.status == "FAIL":
                print(f"FAIL: {r.spec.control} is {fmt(r.measured)} px, the preset gives "
                      f"{fmt(r.spec.intended or 0)} (node {r.node['id']}, {r.node['type']})")
        print()
        if not check.ok():
            failed.append(mode)
    if failed:
        where = f"logs in {out}" if args.out else "run again with --out DIR to keep the logs"
        print(f"kde_visual_check: FAILED in {', '.join(failed)}; {where}")
        return 1
    print(f"kde_visual_check: all {len(modes)} modes pass")
    return 0


if __name__ == "__main__":
    sys.exit(main())
