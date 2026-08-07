#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Build every Teksilo example, launch each one, screenshot its window, and
bundle the captures into a single archive.

This is the visual companion to ``bench_examples.py`` (runtime metrics) and
``package_examples.py`` (binary bundle): instead of numbers or binaries it
produces one PNG per example window — plus one PNG per *page* of the
``widget-catalog`` example, which has 21 tabs.

    python3 tools/screenshot_examples.py                 # all examples, debug
    python3 tools/screenshot_examples.py --release       # release profile
    python3 tools/screenshot_examples.py --only simple-button widget-catalog
    python3 tools/screenshot_examples.py --skip web-view-demo
    python3 tools/screenshot_examples.py --no-build      # reuse existing binaries
    python3 tools/screenshot_examples.py --out shots     # output dir (default dist/screenshots)
    python3 tools/screenshot_examples.py --no-package    # leave PNG files loose, skip the tarball
    python3 tools/screenshot_examples.py --catalog cycle # use widget-catalog --cycle (see below)

The Wayland / KDE catch
-----------------------
This machine runs a **KDE Plasma / Wayland** session. The examples open native
winit/Wayland surfaces, which X11 tools (xdotool, scrot, ``import -window``)
cannot see or drive, and the window opens *behind* the focused IDE/terminal so
a naive ``spectacle -a`` would grab the wrong thing.

The fix (same as ``.claude/skills/run-app`` in the Skribisto repo):

  1. Launch the example and remember its **PID**.
  2. Raise + focus *exactly that window* via KWin's D-Bus scripting API,
     matching on ``window.pid`` (teksilo sets the Wayland app_id to the binary
     name but PID matching is unambiguous — it can never grab the user's other
     open windows, e.g. a running Skribisto).
  3. Capture the now-active window with ``spectacle -b -n -a`` (Wayland
     screencopy, sees occluded surfaces).

Requires: ``gdbus`` + ``spectacle`` (KDE), a live Wayland session. ``convert``
(ImageMagick) is used only for the optional contact sheet.

widget-catalog pages
--------------------
``widget-catalog`` packs every widget into 21 tabs. Two capture modes:

  * ``--catalog tabs`` (default) — relaunch the binary once per tab with
    ``--tab <name>`` and capture. Deterministic: every PNG is guaranteed to
    show exactly the named page.
  * ``--catalog cycle`` — launch once with ``--cycle-ms <ms>`` (the explicit
    interval flag the example ships) and capture each page as the tab
    auto-advances. Honours the flag literally but is timing-sensitive.
  * ``--catalog off`` — treat widget-catalog like any other single window.
"""

from __future__ import annotations

import argparse
import datetime
import glob
import os
import re
import shutil
import signal
import subprocess
import sys
import tarfile
import tempfile
import time
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

REPO_ROOT = Path(__file__).resolve().parent.parent
EXAMPLES_DIR = REPO_ROOT / "examples"

# widget-catalog is captured page-by-page, so the generic single-window loop
# skips it and the dedicated catalog routine handles it.
CATALOG_PKG = "widget-catalog"

# Fallback tab list if parsing `widget-catalog --help` fails for any reason.
CATALOG_TABS_FALLBACK = [
    "palette", "layout", "visuals", "containers", "chrome", "buttons",
    "styling", "inputs", "indicators", "charts", "scene", "text", "richtext",
    "datetime", "color", "menus", "overlays", "data", "dragdrop",
    "animations", "settings",
]


# ── discovery ────────────────────────────────────────────────────────────────

def workspace_excludes() -> set[str]:
    """Directories the root Cargo.toml excludes from the workspace.

    Excluded examples (e.g. ``examples/telemetry_teksilo``, which needs a
    system cmake/C++ toolchain for protobuf-src) are not built by a default
    ``cargo build`` and can't be reached with ``cargo build -p``, so we skip
    them rather than mislabel them as failures.
    """
    try:
        with (REPO_ROOT / "Cargo.toml").open("rb") as f:
            data = tomllib.load(f)
    except (OSError, tomllib.TOMLDecodeError):
        return set()
    return set(data.get("workspace", {}).get("exclude", []))


def discover_examples() -> list[str]:
    excluded = workspace_excludes()
    names: list[str] = []
    for manifest in sorted(EXAMPLES_DIR.glob("*/Cargo.toml")):
        rel = manifest.parent.relative_to(REPO_ROOT).as_posix()
        if rel in excluded:
            continue
        with manifest.open("rb") as f:
            data = tomllib.load(f)
        name = data.get("package", {}).get("name")
        if name:
            names.append(name)
    return names


def binary_path(pkg: str, release: bool) -> Path:
    profile = "release" if release else "debug"
    base = REPO_ROOT / "target" / profile
    cand = base / pkg
    if cand.exists():
        return cand
    alt = base / pkg.replace("-", "_")
    if alt.exists():
        return alt
    return cand  # canonical name for error messages


def cargo_build(pkg: str, release: bool) -> tuple[bool, str]:
    args = ["cargo", "build", "-p", pkg]
    if release:
        args.append("--release")
    proc = subprocess.run(
        args, cwd=REPO_ROOT,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    if proc.returncode != 0:
        tail = "\n".join(proc.stdout.strip().splitlines()[-12:])
        return False, tail
    return True, ""


# ── KWin / spectacle plumbing ────────────────────────────────────────────────

class KWin:
    """Drives KWin's scripting D-Bus interface via gdbus.

    Each call writes a transient .js file, loads it (``loadScript``), runs and
    stops it. Script ``print()`` output lands in the user journal under
    ``kwin_wayland_wrapper`` — that's the only channel back, so readiness/result
    is reported by printing a uniquely-tokened sentinel and grepping the journal.
    """

    JOURNAL_TAG = "kwin_wayland_wrapper"

    def __init__(self) -> None:
        self._token = 0

    def _run_script(self, js: str) -> None:
        with tempfile.NamedTemporaryFile("w", suffix=".js", delete=False) as f:
            f.write(js)
            path = f.name
        try:
            out = subprocess.run(
                ["gdbus", "call", "--session", "--dest", "org.kde.KWin",
                 "--object-path", "/Scripting",
                 "--method", "org.kde.kwin.Scripting.loadScript", path],
                capture_output=True, text=True, timeout=10,
            ).stdout
            m = re.search(r"\d+", out)
            if not m:
                return
            rid = m.group(0)
            obj = f"/Scripting/Script{rid}"
            for method in ("org.kde.kwin.Script.run", "org.kde.kwin.Script.stop"):
                subprocess.run(
                    ["gdbus", "call", "--session", "--dest", "org.kde.KWin",
                     "--object-path", obj, "--method", method],
                    capture_output=True, text=True, timeout=10,
                )
        finally:
            try:
                os.unlink(path)
            except OSError:
                pass

    def _journal_has(self, needle: str, since: str) -> bool:
        try:
            out = subprocess.run(
                ["journalctl", "--user", "-t", self.JOURNAL_TAG,
                 "--since", since, "--no-pager"],
                capture_output=True, text=True, timeout=10,
            ).stdout
        except subprocess.SubprocessError:
            return False
        return needle in out

    def activate_by_pid(self, pid: int) -> Optional[bool]:
        """Raise + focus the window whose process is ``pid``.

        Returns True if such a window currently exists (and was activated),
        False if none exists, or None if the result couldn't be read back.
        """
        self._token += 1
        tok = self._token
        sentinel = f"TEKSILO_SHOT tok={tok} pid={pid} found="
        js = f"""
const wins = workspace.windowList ? workspace.windowList() : workspace.clientList();
let hit = false;
wins.forEach(w => {{
  if (w.pid === {pid}) {{
    w.minimized = false;
    if ("activeWindow" in workspace) workspace.activeWindow = w;
    else workspace.activeClient = w;
    hit = true;
  }}
}});
print("{sentinel}" + hit);
"""
        since = "8 seconds ago"
        self._run_script(js)
        # The journal write lags the D-Bus return by a fraction of a second.
        for _ in range(6):
            time.sleep(0.25)
            if self._journal_has(sentinel + "true", since):
                return True
            if self._journal_has(sentinel + "false", since):
                return False
        return None


def capture_active(out: Path) -> bool:
    """Capture the active window to ``out``. Returns True on a non-trivial PNG."""
    out.parent.mkdir(parents=True, exist_ok=True)
    if out.exists():
        out.unlink()
    try:
        subprocess.run(
            ["spectacle", "-b", "-n", "-a", "-o", str(out)],
            capture_output=True, text=True, timeout=20,
        )
    except subprocess.SubprocessError:
        return False
    # spectacle saves in the background; give it a moment to flush.
    for _ in range(8):
        if out.exists() and out.stat().st_size > 2048:
            return True
        time.sleep(0.25)
    return out.exists() and out.stat().st_size > 2048


# ── launch / capture one window ──────────────────────────────────────────────

@dataclass
class ShotResult:
    name: str               # logical name (== output stem)
    pkg: str
    png: Optional[Path] = None
    ok: bool = False
    note: str = ""


def _spawn(binary: Path, extra_args: list[str]) -> subprocess.Popen:
    env = os.environ.copy()
    env.setdefault("RUST_LOG", "warn")
    return subprocess.Popen(
        [str(binary), *extra_args],
        cwd=REPO_ROOT,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        env=env, start_new_session=True,
    )


def _kill(popen: subprocess.Popen) -> None:
    if popen.poll() is not None:
        return
    try:
        os.killpg(os.getpgid(popen.pid), signal.SIGTERM)
    except (ProcessLookupError, PermissionError):
        return
    try:
        popen.wait(timeout=3)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(os.getpgid(popen.pid), signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass


def shoot_window(
    kwin: KWin,
    name: str,
    pkg: str,
    binary: Path,
    out: Path,
    *,
    extra_args: list[str] | None = None,
    map_timeout: float = 25.0,
    settle: float = 1.8,
    keep_alive: subprocess.Popen | None = None,
) -> ShotResult:
    """Launch (or reuse ``keep_alive``), wait for the window, raise it, capture.

    ``keep_alive`` lets the caller own the process (used by --catalog cycle,
    where one process is captured many times). When None, the process is
    spawned and killed here.
    """
    res = ShotResult(name=name, pkg=pkg)
    extra_args = extra_args or []

    popen = keep_alive or _spawn(binary, extra_args)
    own = keep_alive is None
    try:
        # Poll until KWin reports a window for this PID, or it gives up / dies.
        deadline = time.monotonic() + map_timeout
        found: Optional[bool] = None
        while time.monotonic() < deadline:
            if popen.poll() is not None:
                res.note = f"process exited early (code {popen.returncode}) — no window"
                return res
            found = kwin.activate_by_pid(popen.pid)
            if found:
                break
            time.sleep(0.5)
        if not found:
            res.note = "no window appeared within timeout (headless / CLI example?)"
            return res

        # Let the first frame render, then re-activate to make sure it's frontmost.
        time.sleep(settle)
        kwin.activate_by_pid(popen.pid)
        time.sleep(0.4)

        if capture_active(out):
            res.png = out
            res.ok = True
        else:
            res.note = "spectacle capture failed / empty"
        return res
    finally:
        if own:
            _kill(popen)


# ── widget-catalog page enumeration ──────────────────────────────────────────

def catalog_tabs(binary: Path) -> list[str]:
    try:
        out = subprocess.run(
            [str(binary), "--help"], capture_output=True, text=True, timeout=10,
            cwd=REPO_ROOT,
        ).stdout
    except subprocess.SubprocessError:
        return list(CATALOG_TABS_FALLBACK)
    m = re.search(r"TABS:\s*\n\s*(.+)", out)
    if not m:
        return list(CATALOG_TABS_FALLBACK)
    tabs = [t.strip() for t in m.group(1).split(",") if t.strip()]
    return tabs or list(CATALOG_TABS_FALLBACK)


def shoot_catalog_tabs(
    kwin: KWin, binary: Path, out_dir: Path, settle: float,
) -> list[ShotResult]:
    tabs = catalog_tabs(binary)
    print(f"  widget-catalog: {len(tabs)} pages -> {', '.join(tabs)}", flush=True)
    results: list[ShotResult] = []
    for i, tab in enumerate(tabs):
        name = f"{CATALOG_PKG}__{i:02d}_{tab}"
        out = out_dir / f"{name}.png"
        print(f"    [{i + 1}/{len(tabs)}] page '{tab}'", flush=True, end=" ")
        r = shoot_window(
            kwin, name, CATALOG_PKG, binary, out,
            extra_args=["--tab", tab], settle=settle,
        )
        print("ok" if r.ok else f"FAILED ({r.note})", flush=True)
        results.append(r)
    return results


def shoot_catalog_cycle(
    kwin: KWin, binary: Path, out_dir: Path, period_ms: int, settle: float,
) -> list[ShotResult]:
    """Literal --cycle capture: one process, tab auto-advances on a timer.

    Captures are timed against the launch epoch. Generous period so a slow
    capture still lands inside the right tab's window.
    """
    tabs = catalog_tabs(binary)
    period = period_ms / 1000.0
    print(f"  widget-catalog --cycle-ms {period_ms}: {len(tabs)} pages", flush=True)
    results: list[ShotResult] = []
    popen = _spawn(binary, ["--cycle-ms", str(period_ms)])
    try:
        # Wait for the window to map (tab 0 is showing at this point).
        deadline = time.monotonic() + 25.0
        epoch = None
        while time.monotonic() < deadline:
            if kwin.activate_by_pid(popen.pid):
                epoch = time.monotonic()
                break
            time.sleep(0.4)
        if epoch is None:
            print("    catalog window never appeared", flush=True)
            return results
        time.sleep(settle)  # first frame

        for i, tab in enumerate(tabs):
            # Aim for the middle of tab i's [i*period, (i+1)*period) window.
            target = epoch + i * period + period * 0.45
            now = time.monotonic()
            if target > now:
                time.sleep(target - now)
            kwin.activate_by_pid(popen.pid)
            name = f"{CATALOG_PKG}__{i:02d}_{tab}"
            out = out_dir / f"{name}.png"
            ok = capture_active(out)
            print(f"    [{i + 1}/{len(tabs)}] '{tab}': {'ok' if ok else 'FAILED'}", flush=True)
            results.append(ShotResult(
                name=name, pkg=CATALOG_PKG,
                png=out if ok else None, ok=ok,
                note="" if ok else "cycle capture failed",
            ))
    finally:
        _kill(popen)
    return results


# ── packaging ────────────────────────────────────────────────────────────────

def write_manifest(out_dir: Path, results: list[ShotResult], meta: dict) -> Path:
    lines = ["# Teksilo example screenshots", ""]
    for k, v in meta.items():
        lines.append(f"- **{k}:** {v}")
    lines.append("")
    ok = [r for r in results if r.ok]
    bad = [r for r in results if not r.ok]
    lines.append(f"## Captured ({len(ok)})")
    lines.append("")
    for r in sorted(ok, key=lambda r: r.name):
        lines.append(f"- `{r.png.name}` — {r.pkg}")
    if bad:
        lines.append("")
        lines.append(f"## Skipped / failed ({len(bad)})")
        lines.append("")
        for r in sorted(bad, key=lambda r: r.name):
            lines.append(f"- `{r.name}` — {r.pkg}: {r.note}")
    lines.append("")
    path = out_dir / "MANIFEST.md"
    path.write_text("\n".join(lines))
    return path


def make_contact_sheet(out_dir: Path, results: list[ShotResult]) -> Optional[Path]:
    if not shutil.which("montage"):
        return None
    pngs = [str(r.png) for r in results if r.ok and r.png]
    if not pngs:
        return None
    sheet = out_dir / "_contact_sheet.png"
    try:
        subprocess.run(
            ["montage", *pngs, "-tile", "4x", "-geometry", "400x300+6+6",
             "-background", "white", str(sheet)],
            capture_output=True, timeout=120,
        )
    except subprocess.SubprocessError:
        return None
    return sheet if sheet.exists() else None


def package(out_dir: Path, archive: Path) -> int:
    archive.parent.mkdir(parents=True, exist_ok=True)
    root = archive.name.removesuffix(".tar.gz").removesuffix(".tgz")
    total = 0
    with tarfile.open(archive, "w:gz") as tar:
        for f in sorted(out_dir.iterdir()):
            if f.is_file():
                tar.add(f, arcname=f"{root}/{f.name}")
                total += f.stat().st_size
    return total


# ── main ─────────────────────────────────────────────────────────────────────

def main() -> int:
    p = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--release", action="store_true", help="Release profile (default: debug)")
    p.add_argument("--only", nargs="+", metavar="PKG", help="Only these packages")
    p.add_argument("--skip", nargs="+", metavar="PKG", default=[], help="Skip these packages")
    p.add_argument("--no-build", action="store_true", help="Reuse existing binaries")
    p.add_argument("--out", default="dist/screenshots", help="Output dir (default dist/screenshots)")
    p.add_argument("--no-package", action="store_true", help="Skip the .tar.gz bundle")
    p.add_argument("--catalog", choices=["tabs", "cycle", "off"], default="tabs",
                   help="widget-catalog page capture mode (default tabs)")
    p.add_argument("--cycle-ms", type=int, default=6000,
                   help="Period for --catalog cycle (default 6000)")
    p.add_argument("--settle", type=float, default=1.8,
                   help="Seconds to wait for the first frame after a window maps")
    args = p.parse_args()

    if not os.environ.get("WAYLAND_DISPLAY") and not os.environ.get("DISPLAY"):
        sys.stderr.write("error: no WAYLAND_DISPLAY/DISPLAY — run from a desktop session\n")
        return 2
    for tool in ("gdbus", "spectacle"):
        if not shutil.which(tool):
            sys.stderr.write(f"error: required tool '{tool}' not found\n")
            return 2

    examples = discover_examples()
    if args.only:
        wanted = set(args.only)
        examples = [e for e in examples if e in wanted]
        missing = wanted - set(examples)
        if missing:
            sys.stderr.write(f"warning: --only names not found: {sorted(missing)}\n")
    if args.skip:
        examples = [e for e in examples if e not in set(args.skip)]
    if not examples:
        sys.stderr.write("no examples selected\n")
        return 1

    out_dir = Path(args.out)
    if not out_dir.is_absolute():
        out_dir = REPO_ROOT / out_dir
    out_dir.mkdir(parents=True, exist_ok=True)

    profile = "release" if args.release else "debug"
    kwin = KWin()
    started = datetime.datetime.now()
    print(f"== screenshot: {len(examples)} example(s), profile={profile}, "
          f"catalog={args.catalog}, out={out_dir}", flush=True)

    results: list[ShotResult] = []
    catalog_selected = CATALOG_PKG in examples and args.catalog != "off"

    for i, pkg in enumerate(examples, 1):
        is_catalog_special = pkg == CATALOG_PKG and catalog_selected
        print(f"\n[{i}/{len(examples)}] {pkg}", flush=True)

        if not args.no_build:
            print("  building...", flush=True, end=" ")
            ok, err = cargo_build(pkg, args.release)
            print("ok" if ok else "FAILED", flush=True)
            if not ok:
                print(err, file=sys.stderr)
                results.append(ShotResult(name=pkg, pkg=pkg, note="build failed"))
                continue

        binary = binary_path(pkg, args.release)
        if not binary.exists():
            results.append(ShotResult(name=pkg, pkg=pkg, note=f"binary missing: {binary}"))
            print(f"  binary missing: {binary}", flush=True)
            continue

        # Clear any stale instance from a prior interrupted run. Anchor the
        # path with a trailing space-or-end so "animations" doesn't also match
        # "animations-kit" (comm is truncated at 15 chars, so match the path).
        subprocess.run(["pkill", "-f", f"{binary}([[:space:]]|$)"], capture_output=True)
        time.sleep(0.4)

        if is_catalog_special:
            if args.catalog == "tabs":
                results.extend(shoot_catalog_tabs(kwin, binary, out_dir, args.settle))
            else:  # cycle
                results.extend(
                    shoot_catalog_cycle(kwin, binary, out_dir, args.cycle_ms, args.settle))
            continue

        out = out_dir / f"{pkg}.png"
        r = shoot_window(kwin, pkg, pkg, binary, out, settle=args.settle)
        print(f"  -> {'ok' if r.ok else 'FAILED'}"
              + (f" ({r.note})" if r.note else ""), flush=True)
        results.append(r)

    finished = datetime.datetime.now()
    ok = [r for r in results if r.ok]
    bad = [r for r in results if not r.ok]

    meta = {
        "Date": started.strftime("%Y-%m-%d %H:%M:%S"),
        "Duration": f"{(finished - started).total_seconds():.0f}s",
        "Profile": profile,
        "Catalog mode": args.catalog,
        "Captured": len(ok),
        "Skipped/failed": len(bad),
    }
    manifest = write_manifest(out_dir, results, meta)
    sheet = make_contact_sheet(out_dir, results)

    print(f"\n== done: {len(ok)} captured, {len(bad)} skipped/failed", flush=True)
    print(f"   screenshots: {out_dir}", flush=True)
    print(f"   manifest:    {manifest}", flush=True)
    if sheet:
        print(f"   contact sheet: {sheet}", flush=True)
    if bad:
        print("   skipped/failed:", flush=True)
        for r in bad:
            print(f"     - {r.name}: {r.note}", flush=True)

    if not args.no_package:
        stamp = started.strftime("%Y-%m-%d")
        archive = REPO_ROOT / "dist" / f"teksilo-screenshots-{stamp}.tar.gz"
        total = package(out_dir, archive)
        size = archive.stat().st_size
        print(f"\n   packaged -> {archive} "
              f"({total / 1_048_576:.1f} MiB raw, {size / 1_048_576:.1f} MiB gz)", flush=True)

    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
