#!/usr/bin/env python3
"""check-coverage-floors.py over synthetic tarpaulin reports."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("floors", Path(__file__).with_name("check-coverage-floors.py"))
FLOORS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FLOORS)


def entry(crate, name, covered, coverable):
    return {"path": ["/", "w", "rust", "crates", crate, "src", name], "covered": covered, "coverable": coverable}


REPORT = {"files": [entry("a", "x.rs", 99, 100), entry("a", "y.rs", 100, 100), entry("b", "z.rs", 0, 0),
                    {"path": ["/", "w", "rust", "build.rs"], "covered": 0, "coverable": 9}]}


class CheckTests(unittest.TestCase):
    def test_sums_files_per_crate_and_compares_to_the_floor(self):
        lines, failures = FLOORS.check(REPORT, {"floors": {"a": 99.5, "b": 100}})
        self.assertEqual(lines, ["ok   a: 99.50% (199/200), floor 99.5", "ok   b: 100.00% (0/0), floor 100"])
        self.assertEqual(failures, [])

    def test_below_floor_missing_floor_and_stale_floor_fail(self):
        lines, failures = FLOORS.check(REPORT, {"floors": {"a": 99.6, "c": 50}})
        self.assertEqual(lines, ["FAIL a: 99.50% (199/200), floor 99.6"])
        self.assertEqual(failures, ["a: 99.50% is below its floor 99.6", "b: no floor in the floors file",
                                    "c: has a floor but no files in the report"])

    def test_excluded_crates_are_skipped_but_not_also_floored(self):
        lines, failures = FLOORS.check(REPORT, {"floors": {"a": 99.5}, "excluded": {"b": "binding"}})
        self.assertEqual(lines, ["skip b: excluded (binding)", "ok   a: 99.50% (199/200), floor 99.5"])
        self.assertEqual(failures, [])
        _, failures = FLOORS.check(REPORT, {"floors": {"a": 99.5, "b": 0}, "excluded": {"b": "binding"}})
        self.assertEqual(failures, ["b: both floored and excluded"])

    def test_main_exit_codes(self):
        with tempfile.TemporaryDirectory() as tmp:
            report, floors = Path(tmp, "r.json"), Path(tmp, "f.json")
            report.write_text(json.dumps(REPORT))
            floors.write_text(json.dumps({"floors": {"a": 99.5, "b": 0}}))
            self.assertEqual(FLOORS.main(["x", str(report), str(floors)]), 0)
            floors.write_text(json.dumps({"floors": {"a": 100, "b": 0}}))
            self.assertEqual(FLOORS.main(["x", str(report), str(floors)]), 1)
            self.assertEqual(FLOORS.main(["x"]), 2)


if __name__ == "__main__":
    unittest.main()
