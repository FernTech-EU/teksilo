#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Measure what a live picture costs in a real window: the release-host
acceptance criteria of `engineering/docs/live-image.md` (AC6 to AC14, G.1,
G.2), on Linux.

Runs `live-image-bench` (a release build with the `live-image-timings`
feature) once per scenario, with `TEKSILO_IDLE_TRACE=1`, and reads:

- the idle trace's `teksilo_idle_trace_live` lines: uploads, copies of
  another window's texture and contended frames each second, and the
  percentiles of the live pass (`prepare`),
  lock holds and commit-to-upload delays, in microseconds;
- the trace's main lines: rendered frames each second;
- the UI thread's CPU time, `utime + stime` of `/proc/<pid>/task/<pid>/stat`
  (winit runs the event loop on the main thread);
- the process's GPU memory, `drm-total-gtt` and `drm-total-vram` (or the
  older `drm-memory-*`) summed over its DRM clients in
  `/proc/<pid>/fdinfo`, once a second, or every 50 ms around a window close
  and a churn.

It needs a display. Run it in a session nothing else draws in, on a quiet
host, never on a desktop someone is using: `tools/reader/private_session.sh`
hosts one. Each scenario prints its figures and its verdict, and `--json`
writes them all.

    python3 tools/live_image_measure.py                 # every scenario
    python3 tools/live_image_measure.py --only ac7 ac8  # some
    python3 tools/live_image_measure.py --no-build --json out.json
    python3 tools/live_image_measure.py --list
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BINARY = ROOT / "target" / "release" / "live-image-bench"
CLK_TCK = os.sysconf("SC_CLK_TCK")
MIB = 1024 * 1024


@dataclass
class Scenario:
    name: str
    what: str
    args: list[str]
    seconds: float
    # The part of the run measured: (from, to) seconds after start.
    window: tuple[float, float]
    # Sample GPU memory every 50 ms instead of every second.
    fine_memory: bool = False


SCENARIOS = [
    Scenario(
        "ac7",
        "AC6, AC7, AC12: four rects (8 %) of 720x1280 at 60 Hz",
        ["--workload", "rects"],
        75, (10, 70),
    ),
    Scenario(
        "ac8",
        "AC6, AC8, AC12, AC13: full 1920x1080 frames at 60 Hz",
        ["--workload", "full", "--size", "1920x1080"],
        75, (10, 70),
    ),
    # From 30 s: a window that copies the other's texture takes few locks,
    # so its latest 1,024 lock holds can reach back to the start-up's first
    # upload long after the warm-up; 20 s of the other window's 60 holds a
    # second refresh its ring whole.
    Scenario(
        "ac13",
        "AC13: full 1920x1080 frames, rotated every 5 s, in two windows",
        ["--workload", "full", "--size", "1920x1080", "--rotate-every", "5",
         "--second-window-at", "0"],
        75, (30, 70),
    ),
    Scenario(
        "ac14",
        "AC14: a copy-only producer (copy_within + write_rect) at 60 Hz",
        ["--workload", "copy"],
        75, (10, 70),
    ),
    Scenario(
        "ac11",
        "AC11: a second window opened and closed every 5 s, four times",
        ["--workload", "full", "--size", "1920x1080", "--second-window-at", "10",
         "--second-window-every", "5"],
        48, (5, 48), fine_memory=True,
    ),
    Scenario(
        "g1-720",
        "AC9, G.1: full 720x1280 frames at 60 Hz for 300 s",
        ["--workload", "full", "--size", "720x1280"],
        315, (10, 310),
    ),
    Scenario(
        "g1-1080",
        "AC9, G.1: full 1920x1080 frames at 60 Hz for 300 s",
        ["--workload", "full", "--size", "1920x1080"],
        315, (10, 310),
    ),
    Scenario(
        "g2",
        "G.2: the picture mounted and unmounted every 0.5 s for 300 s",
        ["--workload", "full", "--size", "720x1280", "--churn", "0.5"],
        305, (2, 302), fine_memory=True,
    ),
]

LIVE = re.compile(
    r"^teksilo_idle_trace_live t=(?P<t>[\d.]+) window=(?P<window>\S+) "
    r"textures=(?P<textures>\d+) bytes=(?P<bytes>\d+) uploads=(?P<uploads>\d+) "
    r"(?:copies=(?P<copies>\d+) )?"
    r"contended=(?P<contended>\d+) prepare_us=(?P<prepare>\{[^}]*\}) "
    r"lock_hold_us=(?P<lock_hold>\{[^}]*\}) commit_to_upload_us=(?P<c2u>\{[^}]*\})"
)
MAIN = re.compile(r"^teksilo_idle_trace t=(?P<t>[\d.]+) .*?rendered_frames=(?P<frames>\d+)")
NOTE = re.compile(r"^live_image_bench t=(?P<t>[\d.]+) (?P<event>.*)$")


def percentiles(text: str) -> dict[str, int]:
    return {k: int(v) for k, v in re.findall(r"(\w+):(\d+)", text)}


def thread_cpu(pid: int) -> float | None:
    """The main thread's CPU seconds, user and system."""
    try:
        fields = Path(f"/proc/{pid}/task/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    except OSError:
        return None
    # Fields after the command: state is field 3, utime 14, stime 15.
    return (int(fields[11]) + int(fields[12])) / CLK_TCK


def gpu_memory(pid: int) -> dict[str, int] | None:
    """GTT and VRAM bytes over the process's DRM clients, each counted once."""
    clients: dict[str, dict[str, int]] = {}
    try:
        fds = os.listdir(f"/proc/{pid}/fd")
    except OSError:
        return None
    for fd in fds:
        try:
            if not os.readlink(f"/proc/{pid}/fd/{fd}").startswith("/dev/dri/"):
                continue
            info = Path(f"/proc/{pid}/fdinfo/{fd}").read_text()
        except OSError:
            continue
        keys = dict(re.findall(r"^([\w-]+):\s*(.*)$", info, re.M))
        client = keys.get("drm-client-id")
        if client is None:
            continue
        mem = {}
        for region in ("gtt", "vram"):
            raw = keys.get(f"drm-total-{region}") or keys.get(f"drm-memory-{region}")
            if raw is None:
                continue
            number, *unit = raw.split()
            scale = {"KiB": 1024, "MiB": MIB, "GiB": 1024 * MIB}.get(unit[0] if unit else "", 1)
            mem[region] = int(number) * scale
        clients[client] = mem
    if not clients:
        return None
    return {
        region: sum(mem.get(region, 0) for mem in clients.values())
        for region in ("gtt", "vram")
    }


@dataclass
class Run:
    scenario: Scenario
    started: float = 0.0
    lines: list[tuple[float, str]] = field(default_factory=list)
    cpu: list[tuple[float, float]] = field(default_factory=list)
    memory: list[tuple[float, dict[str, int]]] = field(default_factory=list)
    returncode: int | None = None


def run(scenario: Scenario) -> Run:
    env = dict(os.environ, TEKSILO_IDLE_TRACE="1")
    result = Run(scenario)
    args = [str(BINARY), *scenario.args, "--seconds", str(scenario.seconds)]
    proc = subprocess.Popen(args, env=env, stderr=subprocess.PIPE, stdout=subprocess.DEVNULL,
                            text=True, bufsize=1)
    result.started = time.monotonic()

    def read() -> None:
        assert proc.stderr is not None
        for line in proc.stderr:
            result.lines.append((time.monotonic() - result.started, line.rstrip("\n")))

    reader = threading.Thread(target=read, daemon=True)
    reader.start()
    period = 0.05 if scenario.fine_memory else 1.0
    next_cpu = 0.0
    while proc.poll() is None:
        now = time.monotonic() - result.started
        if now >= next_cpu:
            cpu = thread_cpu(proc.pid)
            if cpu is not None:
                result.cpu.append((now, cpu))
            next_cpu = now + 1.0
        mem = gpu_memory(proc.pid)
        if mem is not None:
            result.memory.append((now, mem))
        time.sleep(period)
    reader.join(timeout=2)
    result.returncode = proc.returncode
    return result


def within(samples, window):
    return [s for s in samples if window[0] <= s[0] <= window[1]]


def note_times(run_: Run, event: str) -> list[float]:
    """When the bench printed `event`, on the script's clock."""
    return [t for t, line in run_.lines if (m := NOTE.match(line)) and m["event"] == event]


def analyse(run_: Run) -> dict:
    s = run_.scenario
    w = s.window
    out: dict = {"scenario": s.name, "what": s.what, "returncode": run_.returncode}
    live = [(t, LIVE.match(line)) for t, line in run_.lines]
    live = [(t, m) for t, m in live if m]
    main = [(t, MAIN.match(line)) for t, line in run_.lines]
    main = [(t, m) for t, m in main if m]
    measured_live = [(t, m) for t, m in live if w[0] <= t <= w[1]]
    measured_main = [(t, m) for t, m in main if w[0] <= t <= w[1]]

    cpu = within(run_.cpu, w)
    if len(cpu) >= 2:
        (t0, c0), (t1, c1) = cpu[0], cpu[-1]
        out["ui_thread_cpu_percent"] = round(100.0 * (c1 - c0) / (t1 - t0), 2)

    windows = sorted({m["window"] for _, m in measured_live})
    per_window = {}
    for window in windows:
        rows = [(t, m) for t, m in measured_live if m["window"] == window]
        uploads = [int(m["uploads"]) for _, m in rows]
        copies = [int(m["copies"] or 0) for _, m in rows]
        # The commits a window's texture took in, uploaded or copied.
        updates = [u + c for u, c in zip(uploads, copies)]
        contended = [int(m["contended"]) for _, m in rows]
        worst = {}
        for key in ("prepare", "lock_hold", "c2u"):
            # Lock holds and commit-to-upload delays are timed by uploads: a
            # line of a window that only copied the other window's texture
            # repeats what its rings held when it last uploaded.
            timed = rows if key == "prepare" else [(t, m) for t, m in rows if int(m["uploads"])]
            stats = [percentiles(m[key]) for _, m in timed]
            stats = [p for p in stats if p.get("n", 0) > 0]
            if stats:
                worst[key] = {
                    "p50_max": max(p["p50"] for p in stats),
                    "p99_max": max(p["p99"] for p in stats),
                    "max": max(p["max"] for p in stats),
                    "last": stats[-1],
                }
        per_window[window] = {
            "lines": len(rows),
            "uploads_per_line_min": min(uploads[1:], default=None),
            "uploads_per_line_median": sorted(uploads)[len(uploads) // 2] if uploads else None,
            "uploads": sum(uploads),
            "copies": sum(copies),
            "updates_per_line_min": min(updates[1:], default=None),
            "contended": sum(contended),
            "textures": sorted({int(m["textures"]) for _, m in rows}),
            "bytes": sorted({int(m["bytes"]) for _, m in rows}),
            "timings_us": worst,
        }
    out["windows"] = per_window
    frames = sum(int(m["frames"]) for _, m in measured_main)
    out["rendered_frames"] = frames
    out["rendered_frames_per_s"] = round(frames / (w[1] - w[0]), 1) if frames else 0

    mem = within(run_.memory, w)
    if mem:
        for region in ("gtt", "vram"):
            values = [m[region] for _, m in mem]
            out[f"{region}_mib"] = {
                "first": round(values[0] / MIB, 1),
                "last": round(values[-1] / MIB, 1),
                "min": round(min(values) / MIB, 1),
                "max": round(max(values) / MIB, 1),
            }
        total = [m["gtt"] + m["vram"] for _, m in mem]
        out["gpu_total_drift_mib"] = round((max(total) - min(total)) / MIB, 1)
        out["gpu_total_end_minus_start_mib"] = round((total[-1] - total[0]) / MIB, 1)

    out["raw"] = {
        "notes": [(round(t, 3), m["event"]) for t, line in run_.lines if (m := NOTE.match(line))],
        "memory": [(round(t, 3), m["gtt"], m["vram"]) for t, m in run_.memory],
        "cpu": [(round(t, 3), c) for t, c in run_.cpu],
        "live": [(round(t, 3), m["window"], int(m["textures"]), int(m["uploads"]),
                  int(m["copies"] or 0), int(m["contended"]), percentiles(m["prepare"]),
                  percentiles(m["lock_hold"]), percentiles(m["c2u"])) for t, m in live],
        "frames": [(round(t, 3), int(m["frames"])) for t, m in main],
    }
    if s.name == "ac11":
        out.update(analyse_close(run_))
    if s.name == "g2":
        out.update(analyse_churn(run_))
    return out


def total_at(run_: Run, t: float) -> int | None:
    before = [m for at, m in run_.memory if at <= t]
    return before[-1]["gtt"] + before[-1]["vram"] if before else None


def analyse_close(run_: Run) -> dict:
    """Each close against the level just before its own open, and against
    the level before the first: a pool the first window filled and keeps
    shows as a one-off step, a leak as a step at every cycle."""
    opened = note_times(run_, "opened second window")
    closed = note_times(run_, "closed second window")
    if not opened or not closed:
        return {"ac11": "the bench did not open and close its second window"}
    first = total_at(run_, opened[0] - 0.05)
    cycles = []
    for open_at, close_at in zip(opened, closed):
        before = total_at(run_, open_at - 0.05)
        peak = max((m["gtt"] + m["vram"] for t, m in run_.memory if open_at <= t <= close_at),
                   default=None)
        after = [(t - close_at, m["gtt"] + m["vram"]) for t, m in run_.memory if t >= close_at]
        back = next((dt for dt, total in after if before is not None and total <= before + MIB),
                    None)
        one_s = total_at(run_, close_at + 1.0)
        cycles.append({
            "before_open_mib": round(before / MIB, 1) if before else None,
            "open_peak_mib": round(peak / MIB, 1) if peak else None,
            "after_close_1s_mib": round(one_s / MIB, 1) if one_s else None,
            "back_within_1mib_of_before_open_s": round(back, 3) if back is not None else None,
        })
    return {
        "ac11_first_baseline_mib": round(first / MIB, 1) if first else None,
        "ac11_cycles": cycles,
    }


def analyse_churn(run_: Run) -> dict:
    unmounted = note_times(run_, "unmounted")
    mounted = note_times(run_, "mounted")
    # Just before each mount: the picture has been gone for most of 0.5 s.
    levels = [total_at(run_, t - 0.05) for t in mounted if any(u < t for u in unmounted)]
    levels = [v for v in levels if v is not None]
    if not levels:
        return {"g2": "no unmount and mount pair seen"}
    return {
        "g2_cycles": len(levels),
        "g2_unmounted_first_mib": round(levels[0] / MIB, 1),
        "g2_unmounted_min_mib": round(min(levels) / MIB, 1),
        "g2_unmounted_max_mib": round(max(levels) / MIB, 1),
        "g2_unmounted_last_mib": round(levels[-1] / MIB, 1),
    }


def verdicts(result: dict) -> list[str]:
    """The acceptance criteria a scenario's figures decide, as lines."""
    name = result["scenario"]
    lines = []
    windows = list(result.get("windows", {}).values())

    def p99(key):
        return max((w["timings_us"][key]["p99_max"] for w in windows if key in w["timings_us"]),
                   default=None)

    cpu = result.get("ui_thread_cpu_percent")
    if name == "ac7":
        lines.append(f"AC6  prepare p99 {p99('prepare')} us (goal <= 500)")
        lines.append(f"AC7  UI thread {cpu} % of a core (provisional goal <= 10)")
    if name in ("ac8", "ac13"):
        lines.append(f"AC6  prepare p99 {p99('prepare')} us (goal <= 1500)")
        lines.append(f"AC13 lock hold p99 {p99('lock_hold')} us (goal <= 1500)")
    if name == "ac13":
        for label, w in result.get("windows", {}).items():
            lines.append(f"AC13 window {label}: {w['uploads']} uploads, {w['copies']} copies, "
                         f"least taken in a second {w['updates_per_line_min']}")
        lines.append(f"AC13 UI thread {cpu} % of a core")
    if name == "ac8":
        least = min((w["uploads_per_line_min"] or 0 for w in windows), default=None)
        lines.append(f"AC8  uploads per trace line, least {least} (goal >= 59 a second)")
        lines.append(f"AC8  UI thread {cpu} % of a core (provisional goal <= 20)")
    if name in ("ac7", "ac8"):
        c2u = [w["timings_us"].get("c2u") for w in windows]
        c2u = [c for c in c2u if c]
        if c2u:
            lines.append(
                f"AC12 commit to upload p50 {max(c['p50_max'] for c in c2u)} us, "
                f"p99 {max(c['p99_max'] for c in c2u)} us (reported)"
            )
    if name == "ac14":
        contended = sum(w["contended"] for w in windows)
        frames = result.get("rendered_frames") or 0
        share = 100.0 * contended / frames if frames else None
        lines.append(f"AC14 contended {contended} of {frames} frames "
                     f"({share:.2f} %) (goal < 1 %)" if share is not None
                     else "AC14 no frames counted")
    if name.startswith("g1"):
        size = (720 * 1280) if name == "g1-720" else (1920 * 1080)
        limit = (3 * size * 4 + 8 * MIB) / MIB
        lines.append(f"AC9  GPU memory drift {result.get('gpu_total_drift_mib')} MiB "
                     f"(goal <= {limit:.0f}), textures {[w['textures'] for w in windows]} "
                     f"(goal: constant)")
    if name == "ac11":
        for i, cycle in enumerate(result.get("ac11_cycles", []), 1):
            lines.append(
                f"AC11 cycle {i}: {cycle['before_open_mib']} MiB before the open, "
                f"{cycle['open_peak_mib']} open, {cycle['after_close_1s_mib']} 1 s after the "
                f"close; within 1 MiB of before the open after "
                f"{cycle['back_within_1mib_of_before_open_s']} s (goal <= 1)")
    if name == "g2":
        lines.append(f"G.2  unmounted GPU memory over {result.get('g2_cycles')} cycles: "
                     f"{result.get('g2_unmounted_min_mib')} to {result.get('g2_unmounted_max_mib')} MiB")
    return lines


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--only", nargs="+", metavar="NAME")
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--json", metavar="PATH")
    args = parser.parse_args()
    if args.list:
        for s in SCENARIOS:
            print(f"{s.name:8} {s.seconds:4.0f} s  {s.what}")
        return 0
    if sys.platform != "linux":
        print("live_image_measure: Linux only (it reads /proc)", file=sys.stderr)
        return 2
    chosen = [s for s in SCENARIOS if not args.only or s.name in args.only]
    unknown = set(args.only or []) - {s.name for s in SCENARIOS}
    if unknown:
        print(f"live_image_measure: no scenario {sorted(unknown)}", file=sys.stderr)
        return 2
    if not args.no_build:
        subprocess.run(["cargo", "build", "--release", "-p", "live-image-bench",
                        "--features", "live-image-timings"], cwd=ROOT, check=True)
    results = []
    for scenario in chosen:
        print(f"== {scenario.name}: {scenario.what} ({scenario.seconds:.0f} s)", flush=True)
        result = analyse(run(scenario))
        results.append(result)
        for line in verdicts(result):
            print("   " + line, flush=True)
        if result["returncode"] not in (0, None):
            print(f"   the bench exited with {result['returncode']}", flush=True)
    if args.json:
        Path(args.json).write_text(json.dumps(results, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
