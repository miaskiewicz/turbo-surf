#!/usr/bin/env python3
"""Union two or more tarpaulin JSON slice reports into one (max-hits merge)."""
import json
import sys


def union_reports(reports):
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


def main():
    if len(sys.argv) < 4:
        print("Usage: merge-render-slices.py <in1.json> <in2.json> [<inN.json>...] <out.json>", file=sys.stderr)
        return 2
    inputs, output = sys.argv[1:-1], sys.argv[-1]
    reports = []
    for path in inputs:
        with open(path) as f:
            reports.append(json.load(f))
    merged = union_reports(reports)
    with open(output, "w") as f:
        json.dump(merged, f)
    print(f"merged {len(reports)} slices: {merged['coverage']:.2f}% ({merged['covered']}/{merged['coverable']})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
