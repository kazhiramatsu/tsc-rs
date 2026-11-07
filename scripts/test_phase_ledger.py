"""Unit tests use synthetic counters; no corpus or measurement window needed."""
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
import unittest.mock
from contextlib import redirect_stdout

spec = importlib.util.spec_from_file_location("phase_ledger", Path(__file__).with_name("phase_ledger.py"))
ledger = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ledger)


def sample(value):
    return {"ri_instructions": value, "ri_user_time": value * 2,
            "ri_system_time": value, "wall_ns": value * 4,
            ledger.PEAK: 4096 + value}


def fixture():
    rows = [{"kind": "metadata", "version": 1, "serialized": True, "workers": 1, "checkers": 1}]
    # Repeated load around a child bind: no nested/overlapping intervals.
    for phase, start, end in (("config", 10, 20), ("load", 22, 40),
                              ("bind", 42, 50), ("load", 52, 60),
                              ("checker-init", 62, 70), ("semantic", 72, 90),
                              ("finalization", 92, 100)):
        rows.append({"phase": phase, "start": sample(start), **sample(end)})
    rows.append({"phase": "total", **sample(120)})
    return rows


def encode(rows):
    return "\n".join(json.dumps(row) for row in rows) + "\n"


class PhaseLedgerTests(unittest.TestCase):
    def test_parse_and_reconcile_disjoint_children(self):
        result = ledger.parse_ledger(encode(fixture()))
        self.assertEqual(set(result["phases"]), set(ledger.PHASES))
        self.assertEqual(result["phases"]["load"]["ri_instructions"], 26)
        self.assertEqual(result["phases"]["load"][ledger.PEAK], 4156)
        check = ledger.reconcile(result["phases"], result["total"], 125)
        self.assertEqual(check["sum"]["ri_instructions"], 78)
        self.assertEqual(check["remainder"]["ri_instructions"], 42)
        self.assertEqual(check["remainder"]["wall_ns"], 168)
        self.assertEqual(check["native_delta"], -5)
        self.assertEqual(check["native_delta_percent"], -4)

    def test_reject_corrupt_or_incomplete_ledgers(self):
        for transform in (
            lambda rows: rows[:-1],
            lambda rows: rows[1:],
            lambda rows: rows + [{"phase": "total", **sample(121)}],
            lambda rows: rows + [{"error": "probe failed", "errno": 5}],
            lambda rows: rows[:2] + [{"phase": "bind", "start": sample(15), **sample(30)}] + rows[2:],
            lambda rows: rows[:-1] + [{"phase": "total", **sample(99)}],
            lambda rows: rows[:1] + [{"phase": "config", "start": sample(20), **sample(10)}] + rows[2:],
        ):
            with self.subTest(transform=transform), self.assertRaises(ValueError):
                ledger.parse_ledger(encode(transform(fixture())))

    def test_parallel_instructions_require_wall_only(self):
        rows = fixture()
        rows[0].update(serialized=False, checkers=4)
        with self.assertRaisesRegex(ValueError, "one worker and one checker"):
            ledger.parse_ledger(encode(rows))
        ledger.parse_ledger(encode(rows), wall_only=True)

    def test_skipped_phases_remain_unavailable(self):
        rows = [row for row in fixture() if row.get("phase") != "semantic"]
        result = ledger.parse_ledger(encode(rows))
        self.assertNotIn("semantic", result["phases"])
        output = io.StringIO()
        with redirect_stdout(output):
            ledger.print_table([result])
        self.assertIn("Unavailable phases (skipped by this run): semantic", output.getvalue())

    def test_runner_refuses_parallel_environment_before_running(self):
        with unittest.mock.patch.dict(ledger.os.environ, {"TSRS_CHECKERS": "4"}), \
                redirect_stdout(io.StringIO()), unittest.mock.patch("sys.stderr", io.StringIO()):
            with self.assertRaises(SystemExit) as error:
                ledger.main(["/not/run", "hono/full"])
            self.assertEqual(error.exception.code, 2)

    def test_medians_reconcile_each_run_before_reduction(self):
        runs = []
        for config, load in ((1, 100), (100, 1), (100, 100)):
            phases = {"config": {**sample(config), ledger.PEAK: 4096},
                      "load": {**sample(load), ledger.PEAK: 4096}}
            runs.append({"phases": phases, "total": sample(300)})
        checks = [ledger.reconcile(run["phases"], run["total"]) for run in runs]
        self.assertEqual(ledger.statistics.median(c["sum"]["ri_instructions"] for c in checks), 101)
        med = ledger.median_runs(runs)
        self.assertEqual(sum(row["ri_instructions"] for row in med["phases"].values()), 200)

    def test_tsrs_has_only_wall_and_does_not_invent_missing_phases(self):
        run = ledger.parse_tsrs("Parse time: 0.12s\nBind time: 0.04s\nCheck time: 0.50s\nTotal time: 0.75s\n")
        self.assertEqual(run["phases"]["load"], {"wall_ns": 120000000})
        self.assertNotIn("checker-init", run["phases"])
        output = io.StringIO()
        with redirect_stdout(output):
            ledger.print_table([run], tsrs=True)
        self.assertIn("phase instructions and footprint are unavailable", output.getvalue())
        self.assertIn("load shows parse time only", output.getvalue())
        with self.assertRaises(ValueError):
            ledger.parse_tsrs("Total time: 0.75s\n")

    def test_runner_uses_serial_case_and_drops_warmup(self):
        # Exercise the actual runner with a tiny fake executable, never a corpus.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            project = root / "hono"
            project.mkdir()
            (project / "tsconfig.bench-test.json").write_text("{}")
            binary = root / "fake-compiler"
            binary.write_text("#!/usr/bin/env python3\nimport os\nfrom pathlib import Path\n"
                              "assert os.environ['TSRS_WORKERS'] == '1'\n"
                              "assert os.environ['TSRS_CHECKERS'] == '1'\n"
                              f"Path(os.environ['TSRS_PHASE_LEDGER']).write_text({encode(fixture())!r})\n")
            binary.chmod(0o755)
            output = io.StringIO()
            with unittest.mock.patch.dict(ledger.os.environ, {"TSRS_WORKERS": "1", "TSRS_CHECKERS": "1"}), redirect_stdout(output):
                ledger.main([str(binary), "hono/test", "--bench", tmp, "--warmup", "1", "--rounds", "2", "--native-total", "120"])
            self.assertIn("2 rounds after 1 warm-up", output.getvalue())
            self.assertIn("reconciliation gate: PASS", output.getvalue())


if __name__ == "__main__":
    unittest.main()
