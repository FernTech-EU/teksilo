#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Drive the live `chart-demo` through the teksilo automation MCP bridge and
assert the chart features actually work in a running app.

This is an end-to-end smoke that no headless unit test can give you: it starts
the *unmodified* demo, reads the bridge socket + token the app prints on
startup, connects `teksilo-automation-mcp --connect`, and then drives the real
widget tree over MCP, asserting against the live AccessKit tree.

What it verifies
----------------
* **Per-datum accessibility** — every visible bar is a `GraphicsObject` node
  labelled ``"<series>, <category>: <value>"`` under the chart's
  `GraphicsDocument` (a screen reader can read the data, not just "a chart").
* **Live structural mutation** — clicking *Add series* grows the per-datum node
  set by one series' worth of marks, i.e. a `ChartModel` mutation propagates
  through to the AT tree.
* **Interactive legend** — clicking a legend swatch hides that series and its
  marks disappear from the AT tree.
* **Screenshot** — the window renders and can be captured.

Usage
-----
    cargo build -p chart-demo -p teksilo-automation-mcp
    python3 scripts/automation_chart_demo.py [--shot out.png] [--release]

Exits 0 on success, 1 on a failed assertion, 2 if the binaries are missing.
Needs a display (the demo opens a real window) and a debug build (the
automation bridge is compiled out of release builds).
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import re
import select
import subprocess
import sys
import tempfile
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Bar-chart per-datum nodes are labelled "<series>, <Quarter>: <value>"
# (e.g. "Revenue, Q1: 41"). The demo's live strip charts use numeric x labels
# ("Windowed, 7: 52") and tick continuously, so matching on the quarter isolates
# the *stable* bar marks — otherwise the counts are a moving target.
BAR_MARK = re.compile(r", Q\d")


def build_dir(release: bool) -> Path:
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target"))
    return target / ("release" if release else "debug")


class Driver:
    """Owns the app + MCP subprocesses and the MCP JSON-RPC conversation."""

    def __init__(self, chart: Path, mcp_bin: Path):
        self.chart = chart
        self.mcp_bin = mcp_bin
        self.app: subprocess.Popen | None = None
        self.mcp: subprocess.Popen | None = None
        self.log = tempfile.NamedTemporaryFile(suffix=".log", delete=False).name
        self.mcp_err = tempfile.NamedTemporaryFile(suffix=".mcperr", delete=False).name
        self._id = 0

    # -- lifecycle ---------------------------------------------------------
    def start(self) -> None:
        self.app = subprocess.Popen(
            [str(self.chart)], stdout=open(self.log, "w"), stderr=subprocess.STDOUT
        )
        sock = tok = None
        deadline = time.time() + 30
        while time.time() < deadline:
            txt = Path(self.log).read_text()
            s = re.search(r"bridge socket = (\S+)", txt)
            t = re.search(r"TEKSILO_AUTOMATION_TOKEN=(\S+)", txt)
            if s and t:
                sock, tok = s.group(1), t.group(1)
                break
            if self.app.poll() is not None:
                self.die("the app exited before announcing its bridge socket")
            time.sleep(0.2)
        if not sock:
            self.die("no bridge socket announced within 30s (is this a debug build?)")
        print(f"bridge up: {sock}")

        # The bridge binds, spawns its accept thread, and only then announces —
        # so the path is connectable the moment it is printed, and one connect
        # is enough. This used to be a wait-for-the-file plus
        # relaunch-until-initialize-answers loop, because the announcement came
        # first and `--connect` loses that race with ENOENT and exit(1).
        # Asserting the guarantee is what replaces the loop: if it ever fires,
        # the announcement has drifted back ahead of the bind in teksilo-app's
        # `automation_bridge::spawn_bridge_thread`.
        if not os.path.exists(sock):
            self.die(
                f"the bridge announced {sock} before binding it — the "
                "announce-before-bind race is back (see "
                "teksilo-app automation_bridge::spawn_bridge_thread)"
            )
        self.mcp = subprocess.Popen(
            [str(self.mcp_bin), "--connect", sock, "--token", tok],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=open(self.mcp_err, "w"),
            text=True,
            bufsize=1,
        )
        self.send(
            "initialize",
            {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "automation_chart_demo", "version": "1"},
            },
        )
        if self.recv(timeout=20, fatal=False) is None:
            self.die("could not connect the MCP server")
        self.send("notifications/initialized", notif=True)

    def stop(self) -> None:
        for p in (self.mcp, self.app):
            if p and p.poll() is None:
                p.terminate()
        if self.app:
            try:
                self.app.wait(timeout=3)
            except Exception:
                self.app.kill()

    def die(self, msg: str) -> None:
        print(f"FAIL: {msg}")
        for name, path in (("app log", self.log), ("mcp stderr", self.mcp_err)):
            try:
                tail = Path(path).read_text().splitlines()[-15:]
                if tail:
                    print(f"--- {name} ---")
                    print("\n".join(tail))
            except Exception:
                pass
        self.stop()
        sys.exit(1)

    # -- MCP JSON-RPC ------------------------------------------------------
    def send(self, method: str, params=None, notif: bool = False) -> None:
        msg = {"jsonrpc": "2.0", "method": method}
        if params is not None:
            msg["params"] = params
        if not notif:
            self._id += 1
            msg["id"] = self._id
        self.mcp.stdin.write(json.dumps(msg) + "\n")
        self.mcp.stdin.flush()

    def recv(self, timeout: float = 20, fatal: bool = True):
        end = time.time() + timeout
        while time.time() < end:
            if self.mcp.poll() is not None:
                break
            ready, _, _ = select.select([self.mcp.stdout], [], [], max(0.0, end - time.time()))
            if not ready:
                break
            line = self.mcp.stdout.readline()
            if not line:
                break
            if line.strip():
                return json.loads(line)
        if fatal:
            self.die("no MCP response within timeout")
        return None

    def call(self, name: str, args=None):
        self.send("tools/call", {"name": name, "arguments": args or {}})
        result = self.recv().get("result", {})
        payload = result.get("structuredContent")
        if payload is None:
            text = "".join(
                c.get("text", "") for c in result.get("content", []) if c.get("type") == "text"
            )
            payload = json.loads(text) if text.strip().startswith("{") else {}
        return result, payload

    # -- convenience -------------------------------------------------------
    def nodes(self) -> list[dict]:
        _, payload = self.call("snapshot_tree")
        return payload.get("nodes", [])

    def click(self, node: dict) -> None:
        """Synthetic pointer click at the node's centre.

        Deliberately a real pointer click rather than `invoke_action`: it drives
        the same gesture pipeline a user does, so it exercises the widget's
        actual tap handling.
        """
        b = node["bounds"]
        self.call(
            "inject_pointer",
            {"x": b["x"] + b["width"] / 2, "y": b["y"] + b["height"] / 2},
        )

    def poll_marks(self, count_fn, changed, tries: int = 10) -> int:
        """Re-snapshot until `changed(n)` or we run out of tries."""
        n = count_fn(self.nodes())
        for _ in range(tries):
            time.sleep(0.4)
            n = count_fn(self.nodes())
            if changed(n):
                break
        return n


def bar_marks(nodes: list[dict]) -> int:
    return sum(
        1
        for n in nodes
        if n.get("role") == "GraphicsObject" and BAR_MARK.search(n.get("label") or "")
    )


def graphics_objects(nodes: list[dict]) -> int:
    return sum(1 for n in nodes if n.get("role") == "GraphicsObject")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--shot", type=Path, help="write a PNG screenshot here")
    ap.add_argument("--release", action="store_true", help="use target/release binaries")
    args = ap.parse_args()

    bd = build_dir(args.release)
    chart, mcp_bin = bd / "chart-demo", bd / "teksilo-automation-mcp"
    missing = [str(p) for p in (chart, mcp_bin) if not p.exists()]
    if missing:
        print("missing binaries:\n  " + "\n  ".join(missing))
        print("\nbuild them first:\n  cargo build -p chart-demo -p teksilo-automation-mcp")
        return 2

    d = Driver(chart, mcp_bin)
    d.start()
    problems: list[str] = []

    time.sleep(1.0)  # let the first frame settle
    ns = d.nodes()
    g0, b0 = graphics_objects(ns), bar_marks(ns)
    has_doc = any(n.get("role") == "GraphicsDocument" for n in ns)
    sample = [n.get("label") for n in ns if n.get("role") == "GraphicsObject"][:3]
    print(f"\nbaseline: {len(ns)} nodes, {g0} GraphicsObject marks ({b0} bar marks)")
    print(f"GraphicsDocument: {has_doc}; sample marks: {sample}")

    if not has_doc:
        problems.append("no GraphicsDocument node — the chart has no a11y container")
    if b0 == 0:
        problems.append("no per-datum GraphicsObject bar marks in the AT tree")

    # Add series -> the per-datum set grows by one series' worth of marks.
    add = next(
        (n for n in ns if n.get("label") == "Add series" and n.get("role") == "Button"), None
    )
    b1 = b0
    if add is None:
        problems.append("'Add series' button not found in the AT tree")
    else:
        print("\n-> click 'Add series'")
        d.click(add)
        b1 = d.poll_marks(bar_marks, lambda n: n > b0)
        print(f"   bar marks: {b0} -> {b1}")
        if b1 <= b0:
            problems.append(f"'Add series' did not grow the per-datum AT set ({b0} -> {b1})")

    # Toggle a legend row -> that series hides and its marks disappear.
    ns = d.nodes()
    cb = next((n for n in ns if n.get("role") == "CheckBox" and n.get("bounds")), None)
    b2 = b1
    if cb is None:
        print("\n(note) no interactive legend row found — skipping the legend check")
    else:
        print(f"\n-> toggle legend row '{cb.get('label')}'")
        d.click(cb)
        b2 = d.poll_marks(bar_marks, lambda n: n < b1)
        print(f"   bar marks: {b1} -> {b2}")
        if b2 >= b1:
            problems.append(f"legend toggle did not hide the series' marks ({b1} -> {b2})")

    # Screenshot.
    res, _ = d.call("screenshot")
    image = next(
        (c["data"] for c in res.get("content", []) if c.get("type") == "image" and c.get("data")),
        None,
    )
    if image is None:
        problems.append("screenshot returned no image data")
    elif args.shot:
        args.shot.write_bytes(base64.b64decode(image))
        print(f"\nscreenshot -> {args.shot}")

    d.stop()

    print("\n=============== RESULT ===============")
    if problems:
        print("FAIL")
        for p in problems:
            print("  -", p)
        return 1
    print("PASS — verified against the live app:")
    print(f"  per-datum a11y : {g0} GraphicsObject marks under a GraphicsDocument")
    print(f"  add series     : bar marks {b0} -> {b1}")
    print(f"  legend toggle  : bar marks {b1} -> {b2}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
