#!/usr/bin/env python3
"""Run one coverage shard: cargo tarpaulin over a subset of the workspace crates.

Usage (from rust/): python3 ../scripts/coverage-shard.py <shard-name> <crate>...

The settings (engine, include/exclude files, target dir, timeout, no-fail-fast) come
from rust/tarpaulin.toml's [surf] section, so a shard measures exactly what the
whole-workspace run measures, restricted to the named crates' tests. The JSON report
lands in <output-dir>-<shard-name>/tarpaulin-report.json. A shard enforces no
threshold: scripts/merge-coverage-reports.py unions the shards and applies the
workspace fail-under, then scripts/check-coverage-floors.py the per-crate floors.
CI shards the workspace because a single instrumented link of every test binary
exhausts the ubuntu runner (rust-lld bus error, run 40788261228).
"""
import subprocess
import sys
import tomllib


def tarpaulin_args(config, shard, crates):
    args = ["cargo", "tarpaulin", "--locked", "--config", "/dev/null"]
    args += ["--engine", config["engine"]]
    for crate in crates:
        args += ["-p", crate]
    args += ["--include-files", *config["include-files"]]
    args += ["--exclude-files", *config["exclude-files"]]
    args += ["--target-dir", config["target-dir"]]
    args += ["--timeout", config["timeout"].rstrip("s")]
    if config.get("no-fail-fast"):
        args.append("--no-fail-fast")
    args += ["--out", "Json", "Stdout"]
    args += ["--output-dir", f"{config['output-dir']}-{shard}"]
    return args


def main(argv):
    if len(argv) < 3:
        print(__doc__.strip().splitlines()[2], file=sys.stderr)
        return 2
    with open("tarpaulin.toml", "rb") as f:
        config = tomllib.load(f)["surf"]
    args = tarpaulin_args(config, argv[1], argv[2:])
    print(" ".join(args), flush=True)
    return subprocess.call(args)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
