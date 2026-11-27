#!/usr/bin/env python3
"""coverage-shard.py chunking and report union."""
import importlib.util
import json
from contextlib import chdir, redirect_stdout, redirect_stderr
import tempfile
import io
from unittest import mock
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
        self.assertEqual(SHARD.parse(["x", "render", "--chunks", "3", "c"]), ("render", 3, 0, 1, ["c"]))
        self.assertEqual(SHARD.parse(["x", "rest", "a", "b"]), ("rest", 1, 0, 1, ["a", "b"]))
        self.assertEqual(SHARD.parse(["x", "r", "--chunks", "4", "--slice", "1/2", "c"]), ("r", 4, 1, 2, ["c"]))
        for bad in (["x", "s", "--chunks", "2", "a", "b"], ["x", "s", "--chunks", "0", "a"], ["x", "s"]):
            with self.assertRaises(SystemExit):
                SHARD.parse(bad)

    def test_unchunked_run_keeps_default_clean_and_propagates_failure(self):
        with mock.patch("builtins.open", mock.mock_open()), \
                mock.patch.object(SHARD.tomllib, "load", return_value={"surf": CONFIG}), \
                mock.patch.object(SHARD.subprocess, "call", return_value=7) as run, \
                redirect_stdout(io.StringIO()):
            self.assertEqual(SHARD.main(["x", "rest", "a", "b"]), 7)
        run.assert_called_once_with(SHARD.tarpaulin_args(CONFIG, "rest", ["a", "b"]))


class ChunkExecutionTests(unittest.TestCase):
    def run_slice(self, statuses, missing_report=None, slice_index=0, clean_status=0):
        with tempfile.TemporaryDirectory() as temporary, chdir(temporary), redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
            tests = Path("crates/c/tests")
            tests.mkdir(parents=True)
            for name in ("a", "b", "c", "d"):
                (tests / f"{name}.rs").touch()
            path = Path("target/cov/report-render/tarpaulin-report.json")
            path.parent.mkdir(parents=True)
            profraws = Path(CONFIG["target-dir"], "tarpaulin", "profraws")
            profraws.mkdir(parents=True)
            (profraws / "stale.profraw").write_text("prior run")
            calls, clean_calls = [], []

            def run(args):
                if args[:2] == ["cargo", "clean"]:
                    clean_calls.append((len(calls), args))
                    return clean_status
                index = len(calls)
                calls.append(args)
                self.assertFalse(path.exists(), "Each chunk must produce its own report")
                self.assertFalse(profraws.exists(), "No chunk may inherit earlier raw counters")
                profraws.mkdir(parents=True)
                (profraws / "current.profraw").write_text(str(index))
                if index != missing_report:
                    path.write_text(json.dumps(report(["a.rs"], [(index + 1, 1)])))
                return statuses[index]

            with mock.patch.object(SHARD.subprocess, "call", side_effect=run):
                status = SHARD.run_chunked(CONFIG, "render", "c", 4, slice_index, 2)
            self.assertFalse(profraws.exists(), "Final raw counters must also be removed")
            self.assertEqual(clean_calls, [(1, ["cargo", "clean", "--target-dir", "target/cov", "-p", "c"])])
            return status, json.loads(path.read_text()), calls

    def test_each_runner_reclaims_completed_crate_artifacts_and_reuses_dependencies(self):
        for slice_index in (0, 1):
            with self.subTest(slice_index=slice_index):
                status, merged, calls = self.run_slice([0, 0], slice_index=slice_index)
                targets = SHARD.chunk_targets(["a", "b", "c", "d"], 4)
                selected = targets[slice_index * 2:(slice_index + 1) * 2]
                expected = [SHARD.tarpaulin_args(CONFIG, "render", ["c"], row) for row in selected]
                self.assertEqual(calls[0], expected[0])
                self.assertEqual(calls[1], expected[1] + ["--skip-clean"])
                self.assertEqual(status, 0)
                self.assertEqual(merged["covered"], 2)

    def test_clean_failure_keeps_completed_reports_and_stops_before_next_chunk(self):
        status, merged, calls = self.run_slice([0, 0], clean_status=5)
        self.assertEqual(status, 5)
        self.assertEqual(len(calls), 1)
        self.assertEqual(merged["covered"], 1)

    def test_sliced_failure_still_exports_merged_reports_and_fails(self):
        status, merged, calls = self.run_slice([0, 3])
        self.assertEqual(status, 3)
        self.assertEqual(len(calls), 2)
        self.assertEqual((merged["covered"], merged["coverable"]), (2, 2))
        self.assertTrue(all("--no-fail-fast" in args for args in calls))

    def test_later_success_does_not_erase_an_earlier_failed_chunk(self):
        status, merged, calls = self.run_slice([7, 0])
        self.assertEqual(status, 7)
        self.assertEqual(len(calls), 2)
        self.assertEqual(merged["covered"], 2)

    def test_successful_slice_unions_every_chunk(self):
        status, merged, calls = self.run_slice([0, 0])
        self.assertEqual(status, 0)
        self.assertEqual(len(calls), 2)
        self.assertEqual([row["line"] for row in merged["files"][0]["traces"]], [1, 2])
        self.assertNotIn("--fail-under", calls[0])
        self.assertEqual(calls[0][calls[0].index("--config") + 1], "/dev/null")

    def test_missing_chunk_report_cannot_reuse_the_previous_chunk(self):
        for missing in (0, 1):
            with self.subTest(missing=missing):
                status, merged, calls = self.run_slice([0, 0], missing_report=missing)
                self.assertEqual(status, 1)
                self.assertEqual(len(calls), 2)
                self.assertEqual((merged["covered"], merged["coverable"]), (1, 1))


if __name__ == "__main__":
    unittest.main()
