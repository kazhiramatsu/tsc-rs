#!/usr/bin/env python3
"""Focused checks for the benchmark's measurement and correctness controls."""
import collections
import importlib.util
import json
import os
from pathlib import Path
import signal
import sys
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("benchmark_cli", Path(__file__).with_name("benchmark-cli.py"))
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


class BenchmarkTests(unittest.TestCase):
    def test_balanced_reproducible_order(self):
        for names in [["before", "after", "go"], list("abcde")]:
            orders = bench.orders(names, 30, 42)
            self.assertEqual(orders, bench.orders(names, 30, 42))
            for position in range(len(names)):
                self.assertEqual(collections.Counter(order[position] for order in orders),
                                 dict.fromkeys(names, 30 // len(names)))
        self.assertEqual(len(set(bench.orders(list("abc"), 30, 42))), 6)
        with self.assertRaises(ValueError):
            bench.orders(list("abc"), 10, 42)

    def test_exit_stderr_resources_and_timeout(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            previous = signal.getsignal(signal.SIGALRM)
            row = bench.measure([sys.executable, "-c", "import sys; print('out'); print('err', file=sys.stderr); sys.exit(2)"],
                                root, os.environ.copy(), root / "stdout", root / "stderr", 5)
            self.assertEqual(row["exit_code"], 2)
            self.assertEqual((root / "stdout").read_text(), "out\n")
            self.assertEqual((root / "stderr").read_text(), "err\n")
            self.assertGreater(row["rss_bytes"], 0)
            self.assertFalse(row["timed_out"])
            row = bench.measure([sys.executable, "-c", "import time; time.sleep(10)"], root,
                                os.environ.copy(), root / "stdout", root / "stderr", 0.1)
            self.assertTrue(row["timed_out"])
            self.assertEqual(row["exit_code"], -signal.SIGKILL)
            self.assertLess(row["wall_ns"], 5e9)
            self.assertEqual(signal.getsignal(signal.SIGALRM), previous)

    def test_summary_uses_paired_rounds(self):
        result = bench.paired_interval([2.0] * 30, 42)
        self.assertEqual(result["median"], 2.0)
        self.assertEqual(result["bootstrap_95_percent"], [2.0, 2.0])

    def test_end_to_end_and_retained_mismatch(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            case = root / "case"
            case.mkdir()
            script = case / "compiler.py"
            script.write_text("from pathlib import Path\nimport sys\nPath('out').mkdir()\nPath('out/result.js').write_text('output')\nprint('TS2322')\nif 'bad' in sys.argv: print('unexpected', file=sys.stderr)\nsys.exit(2)\n")
            plan = {"variants": {name: {"command": [sys.executable, str(script)]} for name in ["oracle", "before", "after", "go"]},
                    "oracle": "oracle", "comparison": ["before", "after", "go"], "pair": ["before", "after"],
                    "sessions": 1, "rounds": 6, "warmups": 0, "seed": 42,
                    "cases": [{"id": "negative", "cwd": str(case), "args": [], "output_dir": "out", "expected_exit": 2}]}
            bench.execute(plan, root / "good")
            self.assertTrue((root / "good/complete.json").exists())
            rows = [json.loads(line) for line in (root / "good/samples.jsonl").read_text().splitlines()]
            self.assertEqual(len(rows), 22)
            self.assertTrue(all(row["outputs"] for row in rows))
            plan["variants"]["after"]["command"].append("bad")
            with self.assertRaisesRegex(RuntimeError, "oracle mismatch"):
                bench.execute(plan, root / "bad")
            self.assertFalse((root / "bad/complete.json").exists())
            failed = [json.loads(line) for line in (root / "bad/samples.jsonl").read_text().splitlines()]
            self.assertEqual(len(failed), 3)
            self.assertIn("unexpected", (root / "bad" / failed[-1]["stderr"]).read_text())


if __name__ == "__main__":
    unittest.main()
