#!/usr/bin/env python3
"""coverage-shard.py chunking and report union."""
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("shard", Path(__file__).with_name("coverage-shard.py"))
SHARD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SHARD)
CONFIG = {"engine": "Llvm", "include-files": ["i"], "exclude-files": ["e"], "target-dir": "target/cov",
          "timeout": "600s", "no-fail-fast": True, "output-dir": "target/cov/report"}


def report(path, lines):
    return {"files": [{"path": path, "traces": [{"line": l, "stats": {"Line": h}} for l, h in lines]}]}


class ShardTests(unittest.TestCase):
    def test_chunks_split_sorted_tests_and_only_the_first_runs_the_lib(self):
        self.assertEqual(SHARD.chunk_targets(["e", "a", "d", "b", "c"], 2), [(True, ["a", "b", "c"]), (False, ["d", "e"])])
        self.assertEqual(SHARD.chunk_targets(["a"], 3), [(True, ["a"])])
        self.assertEqual(SHARD.chunk_targets([], 2), [(True, [])])

    def test_targets_become_lib_and_test_flags(self):
        args = SHARD.tarpaulin_args(CONFIG, "render", ["c"], (True, ["x", "y"]))
        self.assertEqual(args[args.index("-p"):args.index("--include-files")], ["-p", "c", "--lib", "--test", "x", "--test", "y"])
        args = SHARD.tarpaulin_args(CONFIG, "render", ["c"], (False, ["x"]))
        self.assertNotIn("--lib", args)
        self.assertEqual(args[-2:], ["--output-dir", "target/cov/report-render"])
        self.assertNotIn("--lib", SHARD.tarpaulin_args(CONFIG, "rest", ["c"]))

    def test_union_keeps_every_line_and_the_max_hits(self):
        merged = SHARD.union_reports([report(["a.rs"], [(1, 0), (2, 3)]), report(["a.rs"], [(1, 2), (3, 0)]),
                                      report(["b.rs"], [(9, 0)])])
        self.assertEqual([(f["path"], f["covered"], f["coverable"]) for f in merged["files"]],
                         [(["a.rs"], 2, 3), (["b.rs"], 0, 1)])
        self.assertEqual([t["stats"]["Line"] for t in merged["files"][0]["traces"]], [2, 3, 0])
        self.assertEqual((merged["covered"], merged["coverable"]), (2, 4))
        self.assertAlmostEqual(merged["coverage"], 50.0)
        self.assertEqual(SHARD.union_reports([])["coverage"], 100.0)

    def test_parse_reads_chunks_and_rejects_multi_crate_chunking(self):
        self.assertEqual(SHARD.parse(["x", "render", "--chunks", "3", "c"]), ("render", 3, ["c"]))
        self.assertEqual(SHARD.parse(["x", "rest", "a", "b"]), ("rest", 1, ["a", "b"]))
        for bad in (["x", "s", "--chunks", "2", "a", "b"], ["x", "s", "--chunks", "0", "a"], ["x", "s"]):
            with self.assertRaises(SystemExit):
                SHARD.parse(bad)


if __name__ == "__main__":
    unittest.main()
