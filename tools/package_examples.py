#!/usr/bin/env python3
"""Build every Bastyde example in release mode and bundle the resulting
binaries into a single .tar.gz archive.

Usage:
    python3 tools/package_examples.py
    python3 tools/package_examples.py --output dist/bastyde-examples.tar.gz
    python3 tools/package_examples.py --only simple-button widget-catalog
    python3 tools/package_examples.py --skip drag-and-drop
    python3 tools/package_examples.py --no-build      # reuse existing binaries
    python3 tools/package_examples.py --jobs 4        # cargo -j N
"""

from __future__ import annotations

import argparse
import datetime
import subprocess
import sys
import tarfile
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
EXAMPLES_DIR = REPO_ROOT / "examples"
TARGET_RELEASE = REPO_ROOT / "target" / "release"


def discover_examples() -> list[str]:
    names: list[str] = []
    for manifest in sorted(EXAMPLES_DIR.glob("*/Cargo.toml")):
        with manifest.open("rb") as f:
            data = tomllib.load(f)
        name = data.get("package", {}).get("name")
        if name:
            names.append(name)
    return names


def binary_path(pkg: str) -> Path:
    # Cargo replaces dashes with underscores, but for [package] name the
    # default bin name is the package name unchanged on disk.
    candidate = TARGET_RELEASE / pkg
    if candidate.exists():
        return candidate
    alt = TARGET_RELEASE / pkg.replace("-", "_")
    if alt.exists():
        return alt
    return candidate  # return canonical for error messages


def cargo_build(pkg: str, jobs: int | None) -> bool:
    args = ["cargo", "build", "-p", pkg, "--release"]
    if jobs is not None:
        args.extend(["-j", str(jobs)])
    print(f"  building {pkg} ...", flush=True)
    proc = subprocess.run(args, cwd=REPO_ROOT)
    return proc.returncode == 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output", "-o",
        type=Path,
        default=None,
        help="Output archive path (default: dist/bastyde-examples-<date>.tar.gz)",
    )
    parser.add_argument("--only", nargs="+", metavar="PKG", help="Only these packages")
    parser.add_argument("--skip", nargs="+", metavar="PKG", default=[], help="Skip these packages")
    parser.add_argument("--no-build", action="store_true", help="Skip cargo build, reuse existing binaries")
    parser.add_argument("--jobs", "-j", type=int, default=None, help="Parallel cargo jobs")
    args = parser.parse_args()

    if args.output is None:
        stamp = datetime.date.today().isoformat()
        args.output = REPO_ROOT / "dist" / f"bastyde-examples-{stamp}.tar.gz"

    examples = discover_examples()
    if args.only:
        wanted = set(args.only)
        examples = [e for e in examples if e in wanted]
        missing = wanted - set(examples)
        if missing:
            print(f"error: unknown packages: {', '.join(sorted(missing))}", file=sys.stderr)
            return 2
    if args.skip:
        examples = [e for e in examples if e not in set(args.skip)]

    if not examples:
        print("no examples to package", file=sys.stderr)
        return 1

    print(f"packaging {len(examples)} example(s):")
    for name in examples:
        print(f"  - {name}")

    if not args.no_build:
        print("\nbuilding (release):")
        failures: list[str] = []
        for pkg in examples:
            if not cargo_build(pkg, args.jobs):
                failures.append(pkg)
        if failures:
            print(f"\nbuild failed for: {', '.join(failures)}", file=sys.stderr)
            return 1

    missing_bins: list[str] = []
    binaries: list[tuple[str, Path]] = []
    for pkg in examples:
        path = binary_path(pkg)
        if not path.exists():
            missing_bins.append(pkg)
        else:
            binaries.append((pkg, path))

    if missing_bins:
        print(f"\nerror: missing binaries (use without --no-build to compile): {', '.join(missing_bins)}", file=sys.stderr)
        return 1

    args.output.parent.mkdir(parents=True, exist_ok=True)
    archive_root = args.output.name.removesuffix(".tar.gz").removesuffix(".tgz")

    print(f"\nwriting {args.output} ...")
    total_bytes = 0
    with tarfile.open(args.output, "w:gz") as tar:
        for pkg, path in binaries:
            arcname = f"{archive_root}/{pkg}"
            tar.add(path, arcname=arcname)
            total_bytes += path.stat().st_size

    out_size = args.output.stat().st_size
    print(f"  {len(binaries)} binaries, {total_bytes / 1_048_576:.1f} MiB raw -> {out_size / 1_048_576:.1f} MiB compressed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
