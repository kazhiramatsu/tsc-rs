"""Selection and gate contracts of .github/ci/replay.py; no build."""
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("replay", ROOT / ".github/ci/replay.py")
replay = importlib.util.module_from_spec(spec)
spec.loader.exec_module(replay)


class SelectionTests(unittest.TestCase):
    def test_documentation_only_changes_select_nothing(self):
        plan = replay.selection(["docs/design/README.md", "README.md", "CONTRIBUTING.md", "LICENSE"])
        self.assertEqual((plan["rust"], plan["conformance_ts71"]), (False, False))

    def test_any_other_change_selects_both_jobs(self):
        for path in ("crates/checker/src/engine.rs", "CLAUDE.md", ".github/workflows/ci.yml",
                     "scripts/conformance_ts71.py", "vendor/typescript-native/x/manifest.json",
                     "ratchets/ts71/7.1.0-dev-19dadef8.tsv", "docs-not-a-directory.md"):
            plan = replay.selection(["docs/design/README.md", path])
            self.assertEqual((plan["rust"], plan["conformance_ts71"]), (True, True), path)

    def test_an_unknown_change_range_selects_both_jobs(self):
        plan = replay.selection(None)
        self.assertEqual((plan["rust"], plan["conformance_ts71"]), (True, True))
        self.assertEqual(replay.changed_paths("workflow_dispatch", {}), None)
        self.assertEqual(replay.changed_paths("push", {"before": "0" * 40}), None)


class GateTests(unittest.TestCase):
    def needs(self, rust, conformance, selected):
        return {
            "plan": {"result": "success",
                     "outputs": {"has_rust": selected, "has_conformance_ts71": selected}},
            "rust": {"result": rust},
            "conformance-ts71": {"result": conformance},
        }

    def test_selected_jobs_must_succeed(self):
        replay.verify_gate(self.needs("success", "success", "true"))
        with self.assertRaises(ValueError):
            replay.verify_gate(self.needs("failure", "success", "true"))
        with self.assertRaises(ValueError):
            replay.verify_gate(self.needs("success", "skipped", "true"))

    def test_unselected_jobs_must_be_skipped(self):
        replay.verify_gate(self.needs("skipped", "skipped", "false"))
        with self.assertRaises(ValueError):
            replay.verify_gate(self.needs("success", "skipped", "false"))

    def test_the_plan_must_succeed_and_name_every_job(self):
        needs = self.needs("success", "success", "true")
        needs["plan"]["result"] = "failure"
        with self.assertRaises(ValueError):
            replay.verify_gate(needs)
        with self.assertRaises(ValueError):
            replay.verify_gate({"plan": {"result": "success", "outputs": {}}})


if __name__ == "__main__":
    unittest.main()
