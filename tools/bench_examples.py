#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech

"""Build all Bastyde examples, run each briefly, and report binary size,
memory footprint, and idle CPU/GPU usage.

Usage:
    python3 tools/bench_examples.py                      # all examples, release
    python3 tools/bench_examples.py --debug              # debug profile
    python3 tools/bench_examples.py --duration 10        # sample for 10s
    python3 tools/bench_examples.py --warmup 3           # 3s warmup before sampling
    python3 tools/bench_examples.py --only simple-button widget-catalog
    python3 tools/bench_examples.py --skip drag-and-drop
    python3 tools/bench_examples.py --output report.md   # write report path
    python3 tools/bench_examples.py --no-build           # reuse existing binaries

Notes:
    * The examples are GUI applications (winit + wgpu). A working DISPLAY /
      WAYLAND_DISPLAY is required. Run from a desktop session.
    * GPU sampling reads /sys/class/drm/cardN/device/gpu_busy_percent
      (AMD/Intel) or falls back to nvidia-smi. Per-process VRAM is not
      attributed by the kernel, so VRAM/GPU figures are *system-wide*
      with the pre-launch baseline subtracted as a rough estimate of
      the example's contribution.
"""

from __future__ import annotations

import argparse
import dataclasses
import datetime
import glob
import os
import shutil
import signal
import statistics
import subprocess
import sys
import time
import tomllib
from pathlib import Path
from typing import Optional

try:
    import psutil
except ImportError:
    sys.stderr.write("psutil is required: pip install psutil\n")
    sys.exit(2)


REPO_ROOT = Path(__file__).resolve().parent.parent
EXAMPLES_DIR = REPO_ROOT / "examples"


@dataclasses.dataclass
class ExampleInfo:
    package_name: str          # e.g. "simple-button"
    manifest_path: Path        # examples/<dir>/Cargo.toml


@dataclasses.dataclass
class RunResult:
    package_name: str
    binary_path: Optional[Path]
    binary_size: Optional[int]
    build_ok: bool
    build_time_s: float
    started: bool
    exit_code: Optional[int]
    rss_peak_b: Optional[int]
    rss_avg_b: Optional[int]
    vms_peak_b: Optional[int]
    cpu_avg_pct: Optional[float]
    cpu_peak_pct: Optional[float]
    gpu_busy_avg_pct: Optional[float]
    gpu_busy_peak_pct: Optional[float]
    vram_delta_b: Optional[int]   # post-baseline; can be negative
    samples: int
    note: str = ""


# ---------- discovery ---------------------------------------------------------

def discover_examples() -> list[ExampleInfo]:
    examples: list[ExampleInfo] = []
    for manifest in sorted(EXAMPLES_DIR.glob("*/Cargo.toml")):
        with manifest.open("rb") as f:
            data = tomllib.load(f)
        pkg = data.get("package", {})
        name = pkg.get("name")
        if not name:
            continue
        examples.append(ExampleInfo(package_name=name, manifest_path=manifest))
    return examples


# ---------- GPU probe ---------------------------------------------------------

class GpuProbe:
    """Probes system-wide GPU busy% and VRAM use.

    AMD/Intel: reads /sys/class/drm/cardN/device/gpu_busy_percent and
    mem_info_vram_used. NVIDIA: nvidia-smi --query-gpu.
    Returns (busy_pct, vram_bytes) or (None, None) if unsupported.
    """

    def __init__(self) -> None:
        self.kind: str = "none"
        self.busy_path: Optional[Path] = None
        self.vram_path: Optional[Path] = None

        for card in sorted(glob.glob("/sys/class/drm/card[0-9]*")):
            busy = Path(card) / "device" / "gpu_busy_percent"
            vram = Path(card) / "device" / "mem_info_vram_used"
            if busy.exists():
                self.kind = "sysfs"
                self.busy_path = busy
                self.vram_path = vram if vram.exists() else None
                return

        if shutil.which("nvidia-smi"):
            self.kind = "nvidia-smi"

    def sample(self) -> tuple[Optional[float], Optional[int]]:
        if self.kind == "sysfs":
            try:
                busy = float(self.busy_path.read_text().strip())
            except (OSError, ValueError):
                busy = None
            vram: Optional[int] = None
            if self.vram_path:
                try:
                    vram = int(self.vram_path.read_text().strip())
                except (OSError, ValueError):
                    vram = None
            return busy, vram
        if self.kind == "nvidia-smi":
            try:
                out = subprocess.check_output(
                    [
                        "nvidia-smi",
                        "--query-gpu=utilization.gpu,memory.used",
                        "--format=csv,noheader,nounits",
                    ],
                    text=True,
                    timeout=2,
                )
                first = out.strip().splitlines()[0]
                util_s, mem_s = (p.strip() for p in first.split(","))
                return float(util_s), int(mem_s) * 1024 * 1024
            except (subprocess.SubprocessError, ValueError, IndexError):
                return None, None
        return None, None

    def label(self) -> str:
        if self.kind == "sysfs":
            return f"AMD/Intel sysfs ({self.busy_path})"
        if self.kind == "nvidia-smi":
            return "NVIDIA (nvidia-smi)"
        return "unavailable"


# ---------- build -------------------------------------------------------------

def cargo_build(pkg: str, release: bool) -> tuple[bool, float, str]:
    args = ["cargo", "build", "-p", pkg]
    if release:
        args.append("--release")
    t0 = time.monotonic()
    proc = subprocess.run(
        args,
        cwd=REPO_ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    dt = time.monotonic() - t0
    if proc.returncode != 0:
        # last few lines of compiler output
        tail = "\n".join(proc.stdout.strip().splitlines()[-12:])
        return False, dt, tail
    return True, dt, ""


def binary_path(pkg: str, release: bool) -> Path:
    profile = "release" if release else "debug"
    return REPO_ROOT / "target" / profile / pkg


# ---------- runtime measurement ----------------------------------------------

def measure_run(
    pkg: str,
    binary: Path,
    duration: float,
    warmup: float,
    sample_period: float,
    gpu: GpuProbe,
) -> RunResult:
    result = RunResult(
        package_name=pkg,
        binary_path=binary,
        binary_size=binary.stat().st_size if binary.exists() else None,
        build_ok=True,
        build_time_s=0.0,
        started=False,
        exit_code=None,
        rss_peak_b=None,
        rss_avg_b=None,
        vms_peak_b=None,
        cpu_avg_pct=None,
        cpu_peak_pct=None,
        gpu_busy_avg_pct=None,
        gpu_busy_peak_pct=None,
        vram_delta_b=None,
        samples=0,
    )

    # Baseline GPU before launch (system-wide, used to attribute delta).
    base_busy: list[float] = []
    base_vram: list[int] = []
    for _ in range(3):
        b, v = gpu.sample()
        if b is not None:
            base_busy.append(b)
        if v is not None:
            base_vram.append(v)
        time.sleep(0.1)
    base_busy_avg = statistics.mean(base_busy) if base_busy else None
    base_vram_avg = statistics.mean(base_vram) if base_vram else None

    env = os.environ.copy()
    env.setdefault("RUST_LOG", "warn")
    try:
        popen = subprocess.Popen(
            [str(binary)],
            cwd=REPO_ROOT,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env=env,
            start_new_session=True,
        )
    except OSError as e:
        result.note = f"failed to spawn: {e}"
        return result

    result.started = True

    rss_samples: list[int] = []
    vms_samples: list[int] = []
    cpu_samples: list[float] = []
    busy_samples: list[float] = []
    vram_samples: list[int] = []

    try:
        ps_proc = psutil.Process(popen.pid)
        # Prime cpu_percent so the next call returns a delta.
        ps_proc.cpu_percent(interval=None)

        # Warmup: let the window open / GPU init.
        warm_end = time.monotonic() + warmup
        while time.monotonic() < warm_end:
            if popen.poll() is not None:
                break
            time.sleep(0.05)

        # Re-prime CPU after warmup so init cost isn't counted.
        try:
            ps_proc.cpu_percent(interval=None)
        except psutil.NoSuchProcess:
            pass

        # Sampling window.
        end = time.monotonic() + duration
        while time.monotonic() < end:
            if popen.poll() is not None:
                break
            try:
                cpu = ps_proc.cpu_percent(interval=None)
                with ps_proc.oneshot():
                    mi = ps_proc.memory_info()
                    # Sum children too — wgpu/winit may spawn helpers.
                    rss = mi.rss
                    vms = mi.vms
                    for ch in ps_proc.children(recursive=True):
                        try:
                            cmi = ch.memory_info()
                            rss += cmi.rss
                            vms += cmi.vms
                        except (psutil.NoSuchProcess, psutil.AccessDenied):
                            pass
                cpu_samples.append(cpu)
                rss_samples.append(rss)
                vms_samples.append(vms)
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                break

            b, v = gpu.sample()
            if b is not None:
                busy_samples.append(b)
            if v is not None:
                vram_samples.append(v)

            time.sleep(sample_period)
    finally:
        # Graceful shutdown of the whole process group.
        try:
            os.killpg(os.getpgid(popen.pid), signal.SIGTERM)
        except (ProcessLookupError, PermissionError):
            pass
        try:
            result.exit_code = popen.wait(timeout=3)
        except subprocess.TimeoutExpired:
            try:
                os.killpg(os.getpgid(popen.pid), signal.SIGKILL)
            except (ProcessLookupError, PermissionError):
                pass
            try:
                result.exit_code = popen.wait(timeout=2)
            except subprocess.TimeoutExpired:
                result.note = "did not exit after SIGKILL"

    # The first cpu_percent() call after priming can be 0.0 even on busy
    # processes; drop it if we have multiple samples.
    if len(cpu_samples) > 1:
        cpu_samples = cpu_samples[1:]

    result.samples = len(rss_samples)
    if rss_samples:
        result.rss_peak_b = max(rss_samples)
        result.rss_avg_b = int(statistics.mean(rss_samples))
    if vms_samples:
        result.vms_peak_b = max(vms_samples)
    if cpu_samples:
        result.cpu_peak_pct = max(cpu_samples)
        result.cpu_avg_pct = statistics.mean(cpu_samples)
    if busy_samples and base_busy_avg is not None:
        result.gpu_busy_peak_pct = max(0.0, max(busy_samples) - base_busy_avg)
        result.gpu_busy_avg_pct = max(0.0, statistics.mean(busy_samples) - base_busy_avg)
    elif busy_samples:
        result.gpu_busy_peak_pct = max(busy_samples)
        result.gpu_busy_avg_pct = statistics.mean(busy_samples)
    if vram_samples and base_vram_avg is not None:
        result.vram_delta_b = int(max(vram_samples) - base_vram_avg)

    if result.exit_code is not None and result.exit_code not in (0, -signal.SIGTERM, 143):
        if not result.note:
            result.note = f"exited early (code {result.exit_code})"

    return result


# ---------- formatting --------------------------------------------------------

def fmt_bytes_simple(n: Optional[int]) -> str:
    if n is None:
        return "—"
    sign = "-" if n < 0 else ""
    a = abs(n)
    if a < 1024:
        return f"{sign}{a} B"
    if a < 1024 ** 2:
        return f"{sign}{a/1024:.1f} KiB"
    if a < 1024 ** 3:
        return f"{sign}{a/1024**2:.1f} MiB"
    return f"{sign}{a/1024**3:.2f} GiB"


def fmt_pct(p: Optional[float]) -> str:
    return "—" if p is None else f"{p:.1f}%"


def fmt_secs(s: float) -> str:
    return f"{s:.1f}s"


def render_report(
    results: list[RunResult],
    *,
    profile: str,
    duration: float,
    warmup: float,
    gpu_label: str,
    started: datetime.datetime,
    finished: datetime.datetime,
) -> str:
    lines: list[str] = []
    lines.append("# Bastyde examples — runtime benchmark")
    lines.append("")
    lines.append(f"- **Date:** {started.strftime('%Y-%m-%d %H:%M:%S')}")
    lines.append(f"- **Duration:** {(finished - started).total_seconds():.1f}s")
    lines.append(f"- **Profile:** `{profile}`")
    lines.append(f"- **Warmup:** {warmup:.1f}s, sampling window: {duration:.1f}s")
    lines.append(f"- **GPU probe:** {gpu_label}")
    lines.append("")
    lines.append("Memory and CPU are per-process (RSS, sum of children). "
                 "GPU busy% and VRAM are *system-wide*; the pre-launch baseline "
                 "is subtracted so the value approximates the example's "
                 "contribution. Idle GUIs typically show ~0% CPU and ~0% GPU.")
    lines.append("")

    # Summary table.
    lines.append("## Summary")
    lines.append("")
    lines.append("| Example | Build | Bin size | RSS avg | RSS peak | CPU avg | CPU peak | GPU avg | GPU peak | VRAM Δ | Note |")
    lines.append("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---|")
    for r in results:
        if not r.build_ok:
            lines.append(
                f"| `{r.package_name}` | ✗ build failed | — | — | — | — | — | — | — | — | {r.note or 'see details'} |"
            )
            continue
        if not r.started:
            lines.append(
                f"| `{r.package_name}` | ok | {fmt_bytes_simple(r.binary_size)} | — | — | — | — | — | — | — | {r.note or 'did not start'} |"
            )
            continue
        lines.append(
            "| `{name}` | ok | {bin} | {ravg} | {rpk} | {cavg} | {cpk} | {gavg} | {gpk} | {vram} | {note} |".format(
                name=r.package_name,
                bin=fmt_bytes_simple(r.binary_size),
                ravg=fmt_bytes_simple(r.rss_avg_b),
                rpk=fmt_bytes_simple(r.rss_peak_b),
                cavg=fmt_pct(r.cpu_avg_pct),
                cpk=fmt_pct(r.cpu_peak_pct),
                gavg=fmt_pct(r.gpu_busy_avg_pct),
                gpk=fmt_pct(r.gpu_busy_peak_pct),
                vram=fmt_bytes_simple(r.vram_delta_b),
                note=r.note or "",
            )
        )
    lines.append("")

    # Build failure details.
    failed = [r for r in results if not r.build_ok]
    if failed:
        lines.append("## Build failures")
        lines.append("")
        for r in failed:
            lines.append(f"### `{r.package_name}`")
            lines.append("")
            lines.append("```")
            lines.append(r.note.rstrip())
            lines.append("```")
            lines.append("")

    # Per-example details.
    lines.append("## Details")
    lines.append("")
    for r in results:
        lines.append(f"### `{r.package_name}`")
        lines.append("")
        lines.append(f"- Build: {'ok' if r.build_ok else 'FAILED'} ({fmt_secs(r.build_time_s)})")
        if r.binary_path:
            lines.append(f"- Binary: `{r.binary_path}`")
        lines.append(f"- Binary size: {fmt_bytes_simple(r.binary_size)}")
        if r.started:
            lines.append(f"- Samples collected: {r.samples}")
            lines.append(f"- RSS avg / peak: {fmt_bytes_simple(r.rss_avg_b)} / {fmt_bytes_simple(r.rss_peak_b)}")
            lines.append(f"- VMS peak: {fmt_bytes_simple(r.vms_peak_b)}")
            lines.append(f"- CPU avg / peak: {fmt_pct(r.cpu_avg_pct)} / {fmt_pct(r.cpu_peak_pct)}")
            lines.append(f"- GPU busy avg / peak (Δ vs baseline): {fmt_pct(r.gpu_busy_avg_pct)} / {fmt_pct(r.gpu_busy_peak_pct)}")
            lines.append(f"- VRAM Δ: {fmt_bytes_simple(r.vram_delta_b)}")
            if r.exit_code is not None:
                lines.append(f"- Exit code: {r.exit_code}")
        if r.note:
            lines.append(f"- Note: {r.note}")
        lines.append("")

    return "\n".join(lines)


# ---------- main --------------------------------------------------------------

def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--debug", action="store_true", help="Build with debug profile (default: release)")
    p.add_argument("--duration", type=float, default=5.0, help="Sampling window seconds (default 5)")
    p.add_argument("--warmup", type=float, default=2.0, help="Warmup before sampling (default 2)")
    p.add_argument("--sample-period", type=float, default=0.25, help="Seconds between samples (default 0.25)")
    p.add_argument("--only", nargs="*", help="Only run these package names")
    p.add_argument("--skip", nargs="*", default=[], help="Skip these package names")
    p.add_argument("--no-build", action="store_true", help="Reuse existing binaries; skip cargo build")
    p.add_argument("--output", default="bench_report.md", help="Markdown report output path (default bench_report.md)")
    args = p.parse_args()

    release = not args.debug
    profile = "release" if release else "debug"

    examples = discover_examples()
    if args.only:
        wanted = set(args.only)
        examples = [e for e in examples if e.package_name in wanted]
        missing = wanted - {e.package_name for e in examples}
        if missing:
            sys.stderr.write(f"warning: --only names not found: {sorted(missing)}\n")
    if args.skip:
        examples = [e for e in examples if e.package_name not in set(args.skip)]

    if not examples:
        sys.stderr.write("no examples to run\n")
        return 1

    if not (os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")):
        sys.stderr.write(
            "warning: neither DISPLAY nor WAYLAND_DISPLAY is set — GUI examples "
            "will likely fail to start. Run from a desktop session.\n"
        )

    gpu = GpuProbe()
    started = datetime.datetime.now()
    print(f"== bench: {len(examples)} example(s), profile={profile}, "
          f"warmup={args.warmup}s, duration={args.duration}s, "
          f"gpu={gpu.label()}", flush=True)

    results: list[RunResult] = []
    for i, ex in enumerate(examples, 1):
        print(f"\n[{i}/{len(examples)}] {ex.package_name}", flush=True)
        if not args.no_build:
            print(f"  building ({profile})…", flush=True, end=" ")
            ok, dt, err = cargo_build(ex.package_name, release)
            print(f"{'ok' if ok else 'FAILED'} in {dt:.1f}s", flush=True)
            if not ok:
                print(err, file=sys.stderr)
                results.append(RunResult(
                    package_name=ex.package_name,
                    binary_path=None,
                    binary_size=None,
                    build_ok=False,
                    build_time_s=dt,
                    started=False,
                    exit_code=None,
                    rss_peak_b=None,
                    rss_avg_b=None,
                    vms_peak_b=None,
                    cpu_avg_pct=None,
                    cpu_peak_pct=None,
                    gpu_busy_avg_pct=None,
                    gpu_busy_peak_pct=None,
                    vram_delta_b=None,
                    samples=0,
                    note=err,
                ))
                continue
        else:
            dt = 0.0

        binary = binary_path(ex.package_name, release)
        if not binary.exists():
            print(f"  binary missing: {binary}", flush=True)
            results.append(RunResult(
                package_name=ex.package_name,
                binary_path=binary,
                binary_size=None,
                build_ok=False,
                build_time_s=dt,
                started=False,
                exit_code=None,
                rss_peak_b=None, rss_avg_b=None, vms_peak_b=None,
                cpu_avg_pct=None, cpu_peak_pct=None,
                gpu_busy_avg_pct=None, gpu_busy_peak_pct=None,
                vram_delta_b=None, samples=0,
                note=f"binary missing at {binary}",
            ))
            continue

        print(f"  running for {args.warmup + args.duration:.1f}s…", flush=True)
        r = measure_run(
            ex.package_name,
            binary,
            duration=args.duration,
            warmup=args.warmup,
            sample_period=args.sample_period,
            gpu=gpu,
        )
        r.build_time_s = dt
        results.append(r)
        print(
            f"  size={fmt_bytes_simple(r.binary_size)} "
            f"rss={fmt_bytes_simple(r.rss_avg_b)} "
            f"cpu={fmt_pct(r.cpu_avg_pct)} "
            f"gpu={fmt_pct(r.gpu_busy_avg_pct)}"
            + (f"  [{r.note}]" if r.note else ""),
            flush=True,
        )

    finished = datetime.datetime.now()
    report = render_report(
        results,
        profile=profile,
        duration=args.duration,
        warmup=args.warmup,
        gpu_label=gpu.label(),
        started=started,
        finished=finished,
    )

    out = Path(args.output)
    if not out.is_absolute():
        out = REPO_ROOT / out
    out.write_text(report)
    print(f"\nreport written to {out}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
