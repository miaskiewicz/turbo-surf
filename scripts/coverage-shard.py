#!/usr/bin/env python3
"""Run one coverage shard: cargo tarpaulin over a subset of the workspace crates.

Usage (from rust/): python3 ../scripts/coverage-shard.py <shard-name> [--chunks N] <crate>...

The settings (engine, include/exclude files, target dir, timeout, no-fail-fast) come
from rust/tarpaulin.toml's [surf] section, so a shard measures exactly what the
whole-workspace run measures, restricted to the named crates' tests. The JSON report
lands in <output-dir>-<shard-name>/tarpaulin-report.json. A shard enforces no
threshold: scripts/merge-coverage-reports.py unions the shards and applies the
workspace fail-under, then scripts/check-coverage-floors.py the per-crate floors.
CI shards the workspace because a single instrumented link of every test binary
exhausts the ubuntu runner (rust-lld bus error, run 40788261228).

--chunks N (one crate only) splits the crate's tests/ targets into N runs over the same
instrumented build (the first also runs the lib tests), deleting the LLVM profraws
between runs and unioning the N reports into the shard report. Every test process
writes a profraw holding the counters of all instrumented code, so one run of every
turbo-surf-render test binary filled the runner's 145G disk (run 40899746815).
"""
import json
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path


def tarpaulin_args(config, shard, crates, targets=None):
    args = ["cargo", "tarpaulin", "--locked", "--config", "/dev/null"]
    args += ["--engine", config["engine"]]
    for crate in crates:
        args += ["-p", crate]
    if targets is not None:
        lib, tests = targets
        if lib:
            args.append("--lib")
        for test in tests:
            args += ["--test", test]
    args += ["--include-files", *config["include-files"]]
    args += ["--exclude-files", *config["exclude-files"]]
    args += ["--target-dir", config["target-dir"]]
    args += ["--timeout", config["timeout"].rstrip("s")]
    if config.get("no-fail-fast"):
        args.append("--no-fail-fast")
    args += ["--out", "Json", "Stdout"]
    args += ["--output-dir", f"{config['output-dir']}-{shard}"]
    return args


def chunk_targets(tests, chunks):
    """Split sorted test target names into `chunks` contiguous groups; the first runs the lib."""
    tests = sorted(tests)
    size = -(-len(tests) // chunks)
    groups = [tests[i * size:(i + 1) * size] for i in range(chunks)]
    return [(i == 0, group) for i, group in enumerate(groups) if i == 0 or group]


def union_reports(reports):
    """Union tarpaulin JSON reports of the same build: every line any report lists, max hits."""
    files = {}
    for report in reports:
        for entry in report["files"]:
            lines = files.setdefault(tuple(entry["path"]), {})
            for trace in entry["traces"]:
                hits = trace["stats"].get("Line", 0)
                lines[trace["line"]] = max(lines.get(trace["line"], 0), hits)
    merged = []
    for path, lines in sorted(files.items()):
        traces = [{"line": line, "address": [], "length": 0, "stats": {"Line": hits}} for line, hits in sorted(lines.items())]
        covered = sum(1 for hits in lines.values() if hits > 0)
        merged.append({"path": list(path), "content": "", "traces": traces, "covered": covered, "coverable": len(lines)})
    covered = sum(entry["covered"] for entry in merged)
    coverable = sum(entry["coverable"] for entry in merged)
    coverage = 100.0 if coverable == 0 else 100.0 * covered / coverable
    return {"files": merged, "coverage": coverage, "covered": covered, "coverable": coverable}


def parse(argv):
    shard, rest = argv[1], argv[2:]
    chunks = 1
    if rest[:1] == ["--chunks"]:
        chunks, rest = int(rest[1]), rest[2:]
    if not rest or chunks < 1 or (chunks > 1 and len(rest) != 1):
        raise SystemExit(__doc__.strip().splitlines()[2])
    return shard, chunks, rest


def run_chunked(config, shard, crate, chunks):
    tests = [p.stem for p in Path("crates", crate, "tests").glob("*.rs")]
    report_path = Path(f"{config['output-dir']}-{shard}", "tarpaulin-report.json")
    profraws = Path(config["target-dir"], "tarpaulin", "profraws")
    reports, status = [], 0
    for i, targets in enumerate(chunk_targets(tests, chunks)):
        shutil.rmtree(profraws, ignore_errors=True)
        args = tarpaulin_args(config, shard, [crate], targets)
        print(f"chunk {i + 1}: " + " ".join(args), flush=True)
        status = subprocess.call(args) or status
        with open(report_path) as f:
            reports.append(json.load(f))
    shutil.rmtree(profraws, ignore_errors=True)
    merged = union_reports(reports)
    with open(report_path, "w") as f:
        json.dump(merged, f)
    print(f"{shard}: {merged['coverage']:.2f}% ({merged['covered']}/{merged['coverable']}) over {len(reports)} chunk(s)")
    return status


def main(argv):
    if len(argv) < 3:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    shard, chunks, crates = parse(argv)
    with open("tarpaulin.toml", "rb") as f:
        config = tomllib.load(f)["surf"]
    if chunks > 1:
        return run_chunked(config, shard, crates[0], chunks)
    args = tarpaulin_args(config, shard, crates)
    print(" ".join(args), flush=True)
    return subprocess.call(args)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
