#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 FernTech
"""Count the screen-reader findings from their per-page summary rows.

``docs/a11y/reader-findings.md`` carries a count table and a totals line.
Both are derived from the findings, never typed by hand:

* a finding's status is the last column of its summary row in
  ``docs/a11y/reader-findings/<page>.md`` (``fixed``, ``partly fixed``,
  ``open``, ``open (example)``, ``upstream``, ``harness``);
* its severity is the fourth column;
* its layer (framework, example, upstream, harness) is the ``**layer:**``
  word on the ``Severity`` line of its entry.

A finding whose status was set by reading the code, with no reader run, has
a ``- **Since `<commit>`:**`` line saying "not yet measured with a reader".
Those are counted separately so the totals say how many statuses are not
measurements.

Run it from the repository root::

    python3 tools/count_reader_findings.py           # print the table and totals
    python3 tools/count_reader_findings.py --check   # exit 1 if the page differs

``--check`` is not wired into CI; run it after editing a status or a
summary row. It compares the count table, the totals line, the sentence naming
the statuses set from the code, and the "What stays open" total with the
pages. A status, severity or layer outside the known sets stops the run.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PAGES = ROOT / "docs" / "a11y" / "reader-findings"
INDEX = ROOT / "docs" / "a11y" / "reader-findings.md"

SEVERITIES = ["critical", "high", "medium", "low"]
STATUSES = ["fixed", "partly fixed", "open", "open (example)", "upstream", "harness"]
LAYERS = ["framework", "example", "upstream", "harness"]

ROW = re.compile(r"^\| \[(?P<id>[^\]]+)\]\(#[^)]*\) \|(?P<rest>.*)\|\s*$")
HEADING = re.compile(r"^### (?P<id>\S+) \{#")
LAYER = re.compile(r"^- \*\*Severity:\*\* (?P<sev>\w+); \*\*layer:\*\* (?P<layer>\w+)")
UNMEASURED = re.compile(r"^- \*\*Since `[0-9a-f]+`:\*\*.*not yet measured with a reader", re.IGNORECASE)


def load() -> list[dict]:
    """Every finding: id, severity, status, layer, set_from_code."""
    findings: dict[str, dict] = {}
    for page in sorted(PAGES.glob("*.md")):
        current = None
        for line in page.read_text(encoding="utf-8").splitlines():
            row = ROW.match(line)
            if row:
                cells = [c.strip() for c in row.group("rest").split("|")]
                # example | finding | severity | platform | status
                findings[row.group("id")] = {
                    "id": row.group("id"),
                    "severity": cells[2],
                    "status": cells[4],
                    "layer": None,
                    "entry_severity": None,
                    "from_code": False,
                }
                continue
            heading = HEADING.match(line)
            if heading:
                current = heading.group("id")
                continue
            if current is None or current not in findings:
                continue
            layer = LAYER.match(line)
            if layer:
                findings[current]["layer"] = layer.group("layer")
                findings[current]["entry_severity"] = layer.group("sev")
            elif UNMEASURED.match(line):
                findings[current]["from_code"] = True
    for f in findings.values():
        if f["layer"] is None:
            sys.exit(f"{f['id']}: no Severity/layer line in its entry")
        if f["layer"] not in LAYERS:
            sys.exit(f"{f['id']}: unknown layer {f['layer']!r}")
        if f["severity"] not in SEVERITIES:
            sys.exit(f"{f['id']}: unknown severity {f['severity']!r}")
        if f["status"] not in STATUSES:
            sys.exit(f"{f['id']}: unknown status {f['status']!r} (known: {', '.join(STATUSES)})")
        if f["entry_severity"] is not None and f["entry_severity"] != f["severity"]:
            sys.exit(
                f"{f['id']}: the row says {f['severity']}, the entry says {f['entry_severity']}"
            )
    return list(findings.values())


def count(findings: list[dict]) -> dict:
    table = {s: {l: 0 for l in LAYERS} | {"fixed": 0, "partly fixed": 0} for s in SEVERITIES}
    status: dict[str, int] = {}
    for f in findings:
        row = table[f["severity"]]
        row[f["layer"]] += 1
        if f["status"] in ("fixed", "partly fixed"):
            row[f["status"]] += 1
        status[f["status"]] = status.get(f["status"], 0) + 1
    assert sum(status.values()) == len(findings)
    stays = {s: 0 for s in SEVERITIES}
    for f in findings:
        if f["layer"] == "framework" and f["status"] in ("open", "partly fixed"):
            stays[f["severity"]] += 1
    return {
        "stays_open": stays,
        "table": table,
        "status": status,
        "total": len(findings),
        "from_code": sorted(f["id"] for f in findings if f["from_code"]),
    }


def table_lines(c: dict) -> list[str]:
    cols = LAYERS + ["all", "fixed", "partly fixed"]
    lines = ["| severity | " + " | ".join(LAYERS[:4]) + " | all | fixed | partly fixed |"]
    lines.append("|---" * (len(cols) + 1) + "|")
    sums = {k: 0 for k in LAYERS + ["all", "fixed", "partly fixed"]}
    for s in SEVERITIES:
        r = c["table"][s]
        r_all = sum(r[l] for l in LAYERS)
        cells = [r[l] for l in LAYERS] + [r_all, r["fixed"], r["partly fixed"]]
        for k, v in zip(cols, cells):
            sums[k] += v
        lines.append(f"| {s} | " + " | ".join(map(str, cells)) + " |")
    lines.append("| all | " + " | ".join(str(sums[k]) for k in cols) + " |")
    return lines


def totals_numbers(c: dict) -> tuple[int, int, int, int, int, int]:
    st = c["status"]
    return (
        c["total"],
        st.get("fixed", 0),
        st.get("partly fixed", 0),
        st.get("open", 0),
        st.get("open (example)", 0),
        st.get("upstream", 0),
    )


def totals_line(c: dict) -> str:
    total, fixed, partly, op, ex, up = totals_numbers(c)
    high = c["table"]["high"]
    line = (
        f"Counted from the finding pages: of {total} findings, {fixed} fixed, {partly} partly "
        f"fixed, {op} open in Teksilo, {ex} open in the examples' own code, and {up} upstream "
        f"(high severity: {high['fixed']} fixed, {high['partly fixed']} partly fixed)."
    )
    if c["from_code"]:
        ids = ", ".join(c["from_code"])
        line += (
            f" Of the fixed and partly fixed, {len(c['from_code'])} were set from the code "
            f"and have not been measured with a reader: {ids}."
        )
    return line


def stays_open_line(c: dict) -> str:
    st = c["stays_open"]
    return (
        f"In Teksilo itself {sum(st.values())} findings stay open or partly fixed: "
        f"{st['critical']} critical, {st['high']} high, {st['medium']} medium, {st['low']} low."
    )


def check(c: dict) -> int:
    text = INDEX.read_text(encoding="utf-8")
    lines = text.splitlines()
    bad = []
    for line in table_lines(c):
        if line not in lines:
            bad.append(f"table line missing or different: {line}")
    for want in (totals_line(c), stays_open_line(c)):
        if want not in text:
            bad.append(f"line missing or different: {want}")
    for b in bad:
        print(b, file=sys.stderr)
    return 1 if bad else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--check", action="store_true", help="exit 1 if reader-findings.md differs")
    args = ap.parse_args()
    c = count(load())
    if args.check:
        return check(c)
    print("\n".join(table_lines(c)))
    print()
    print(totals_line(c))
    print()
    print(stays_open_line(c))
    return 0


if __name__ == "__main__":
    sys.exit(main())
