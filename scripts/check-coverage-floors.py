#!/usr/bin/env python3
"""Per-crate line-coverage floors over a tarpaulin JSON report.

Usage: check-coverage-floors.py <tarpaulin-report.json> <coverage-floors.json>

The floors file is {"floors": {crate: percent}, "excluded": {crate: reason}}, crates
named by their directory under rust/crates/. Every file in the report is attributed to
the crate whose directory holds it; a crate's coverage is covered/coverable over its
files. Exit 1 when any crate is under its floor, when a crate in the report has neither
a floor nor an exclusion (a new crate must declare one), when a floor names a crate the
report lacks, or when a crate is both floored and excluded. Excluded crates are skipped.
"""
import json
import sys


def crate_of(parts):
    for i, part in enumerate(parts[:-1]):
        if part == "crates":
            return parts[i + 1]
    return None


def per_crate(report):
    totals = {}
    for entry in report["files"]:
        crate = crate_of(entry["path"])
        if crate is None:
            continue
        covered, coverable = totals.get(crate, (0, 0))
        totals[crate] = (covered + entry["covered"], coverable + entry["coverable"])
    return totals


def percent(covered, coverable):
    return 100.0 if coverable == 0 else 100.0 * covered / coverable


def check(report, config):
    floors, excluded = config["floors"], config.get("excluded", {})
    totals = per_crate(report)
    failures, lines = [], []
    for crate in sorted(set(floors) & set(excluded)):
        failures.append(f"{crate}: both floored and excluded")
    for crate in sorted(set(excluded)):
        lines.append(f"skip {crate}: excluded ({excluded[crate]})")
    for crate in sorted((set(totals) | set(floors)) - set(excluded)):
        if crate not in floors:
            failures.append(f"{crate}: no floor in the floors file")
            continue
        if crate not in totals:
            failures.append(f"{crate}: has a floor but no files in the report")
            continue
        covered, coverable = totals[crate]
        pct = percent(covered, coverable)
        ok = pct + 1e-9 >= floors[crate]
        lines.append(f"{'ok  ' if ok else 'FAIL'} {crate}: {pct:.2f}% ({covered}/{coverable}), floor {floors[crate]}")
        if not ok:
            failures.append(f"{crate}: {pct:.2f}% is below its floor {floors[crate]}")
    return lines, failures


def main(argv):
    if len(argv) != 3:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    with open(argv[1]) as f:
        report = json.load(f)
    with open(argv[2]) as f:
        floors = json.load(f)
    lines, failures = check(report, floors)
    print("\n".join(lines))
    for failure in failures:
        print(f"error: {failure}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
