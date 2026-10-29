#!/usr/bin/env python3
"""Merge per-shard tarpaulin JSON reports and apply the workspace fail-under.

Usage: merge-coverage-reports.py <tarpaulin.toml> <merged-out.json> <crate,crate=report.json>...

Each report argument names the crates whose tests that shard ran. A crate's files take
their coverable lines from the shard that owns the crate: that shard's binaries carry
the crate's LLVM instrumentation, while other shards that do not compile it fill its
files from tarpaulin's static source analysis, a different line set. A line counts as
covered when any shard's tests hit it, so tests in one crate still credit the code of
another, as in one whole-workspace run. Writes the merged report
(the same JSON shape, for scripts/check-coverage-floors.py), prints the workspace
total, and exits 1 below tarpaulin.toml's [surf] fail-under.
"""
import json
import sys
import tomllib


def crate_of(parts):
    for i, part in enumerate(parts[:-1]):
        if part == "crates":
            return parts[i + 1]
    return None


def merge(shards):
    owner = {}
    for crates, _ in shards:
        for crate in crates:
            if crate in owner:
                raise SystemExit(f"error: {crate} is owned by two shards")
            owner[crate] = crates
    files, hits = {}, {}
    for crates, report in shards:
        for entry in report["files"]:
            key = tuple(entry["path"])
            seen = hits.setdefault(key, {})
            for trace in entry["traces"]:
                seen[trace["line"]] = max(seen.get(trace["line"], 0), trace["stats"].get("Line", 0))
            crate = crate_of(entry["path"])
            if crate in crates or (crate not in owner and key not in files):
                files[key] = {trace["line"] for trace in entry["traces"]}
    files = {key: {line: hits[key].get(line, 0) for line in lines} for key, lines in files.items()}
    merged = []
    for path, lines in sorted(files.items()):
        traces = [{"line": line, "address": [], "length": 0, "stats": {"Line": hits}} for line, hits in sorted(lines.items())]
        covered = sum(1 for hits in lines.values() if hits > 0)
        merged.append({"path": list(path), "content": "", "traces": traces, "covered": covered, "coverable": len(lines)})
    covered = sum(entry["covered"] for entry in merged)
    coverable = sum(entry["coverable"] for entry in merged)
    coverage = 100.0 if coverable == 0 else 100.0 * covered / coverable
    return {"files": merged, "coverage": coverage, "covered": covered, "coverable": coverable}


def main(argv):
    if len(argv) < 4:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    with open(argv[1], "rb") as f:
        fail_under = tomllib.load(f)["surf"]["fail-under"]
    shards = []
    for arg in argv[3:]:
        crates, _, name = arg.partition("=")
        with open(name) as f:
            shards.append((set(crates.split(",")), json.load(f)))
    merged = merge(shards)
    with open(argv[2], "w") as f:
        json.dump(merged, f)
    pct = merged["coverage"]
    print(f"workspace: {pct:.2f}% ({merged['covered']}/{merged['coverable']}) over {len(shards)} shard(s), fail-under {fail_under}")
    if pct + 1e-9 < fail_under:
        print(f"error: workspace coverage {pct:.2f}% is below fail-under {fail_under}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
