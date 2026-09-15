"""Selection and gate contracts; no Rust build or witness replay."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("replay", ROOT / ".github/ci/replay.py")
replay = importlib.util.module_from_spec(spec)
spec.loader.exec_module(replay)
witness = replay.witness


class SelectionTests(unittest.TestCase):
    def test_partition_covers_canonical_full_acceptance_once(self):
        replay.validate_partition()
        self.assertEqual(sum(map(len, replay.GROUPS.values())), 31)

    def test_docs_do_not_build_or_replay_rust(self):
        plan = replay.selection(["docs/design/a.md", "README.md"])
        self.assertEqual(plan["acceptance"], [])
        self.assertEqual(plan["witnesses"], [])

    def test_followup_fixture_only_runs_its_collection(self):
        plan = replay.selection(["crates/compiler/tests/fixtures/decorator-super-followup2.json.zst"])
        self.assertEqual(plan["acceptance"], [])
        self.assertEqual(plan["witnesses"], ["followup2"])
        self.assertEqual(replay.matrices(plan)["witnesses"], {
            "include": [{"group": "controls", "suites": ["followup2"]}],
        })

    def test_printer_fixture_and_observers_only_select_printer_group(self):
        for path in replay.PRINTER_INPUTS:
            with self.subTest(path=path):
                plan = replay.selection([path])
                self.assertEqual(plan["acceptance"], [])
                self.assertEqual(plan["witnesses"], ["printer"])
                self.assertEqual(replay.matrices(plan)["witnesses"], {
                    "include": [{"group": "printer", "suites": ["printer"]}],
                })

    def test_printer_job_rejects_missing_or_zero_test_target(self):
        full = "\n".join("test result: ok. 1 passed; 0 failed;" for _ in replay.PRINTER_TARGETS)
        for output in (full.replace("1 passed", "0 passed", 1), full.split("\n", 1)[1]):
            fake = subprocess.CompletedProcess([], 0, output)
            with patch.object(replay.subprocess, "run", return_value=fake):
                with self.assertRaises(ValueError):
                    replay.printer_witnesses()
        with patch.object(replay.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, full)):
            replay.printer_witnesses()

    def test_bundle_sink_fixture_selects_only_its_complete_commands(self):
        plan = replay.selection(["crates/compiler/tests/fixtures/bundle-sinks.json"])
        self.assertEqual(plan["acceptance"], [])
        self.assertEqual(plan["witnesses"], ["bundle-sinks"])
        self.assertEqual(replay.matrices(plan)["witnesses"], {
            "include": [{"group": "controls", "suites": ["bundle-sinks"]}],
        })

    def test_shared_comparator_is_an_acceptance_input(self):
        plan = replay.selection(["crates/compiler/tests/integration/h2_7c_declaration_blocking.rs"])
        self.assertEqual(plan["acceptance"], ["late"])
        self.assertEqual(plan["witnesses"], ["retained"])

    def test_library_snapshot_keeps_all_fresh_program_consumers(self):
        plan = replay.selection(["crates/compiler/tests/support/witness_libraries.rs"])
        self.assertEqual(plan["acceptance"], ["late"])
        self.assertEqual(plan["witnesses"], [*witness.SUPER, "retained"])

    def test_common_unknown_and_missing_ranges_keep_complete_coverage(self):
        for paths in (None, [], ["crates/emitter/src/printer.rs"], ["new/tool.rs"],
                      ["crates/xtask/src/h2_7c_acceptance.rs"], ["crates/compiler/tests/new.rs"],
                      ["crates/compiler/data.md"], ["Cargo.lock"]):
            with self.subTest(paths=paths):
                plan = replay.selection(paths)
                self.assertEqual(plan["acceptance"], list(replay.GROUPS))
                self.assertEqual(plan["witnesses"], list(witness.SUITES))

    def test_change_lists_have_no_300_path_truncation(self):
        plan = replay.selection([f"docs/{index}.md" for index in range(500)] + ["crates/types/src/lib.rs"])
        self.assertEqual(plan["acceptance"], list(replay.GROUPS))

    def test_manual_unknown_event_and_invalid_base_force_full_replay(self):
        for event, payload in [("workflow_dispatch", {}), ("unknown", {}), ("pull_request", {}),
                               ("push", {"before": "0" * 40}), ("merge_group", {"merge_group": {"base_sha": "bad"}})]:
            self.assertIsNone(replay.changed_paths(event, payload))

    def test_rename_retains_deleted_old_path(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def git(*args):
                return subprocess.check_output(["git", *args], cwd=root, stderr=subprocess.DEVNULL).decode().strip()
            git("init", "-q")
            (root / "old.rs").write_text("fn example() {}\n")
            git("add", "old.rs")
            git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "base")
            base = git("rev-parse", "HEAD")
            (root / "docs").mkdir()
            git("mv", "old.rs", "docs/new.md")
            git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "rename")
            paths = replay.changed_paths("push", {"before": base}, root)
            self.assertEqual(set(paths), {"old.rs", "docs/new.md"})
            self.assertEqual(replay.selection(paths)["acceptance"], list(replay.GROUPS))
            self.assertIsNone(replay.changed_paths("push", {"before": "f" * 40}, root))

    def test_gate_fails_closed_for_missing_cancelled_or_skipped_selected_jobs(self):
        for selected in ("true", "false"):
            expected = "success" if selected == "true" else "skipped"
            needs = {"plan": {"result": "success", "outputs": {"has_acceptance": selected}},
                     "acceptance": {"result": expected}}
            replay.verify_gate(needs, "acceptance")
            for result in ("failure", "cancelled", "skipped" if selected == "true" else "success"):
                needs["acceptance"]["result"] = result
                with self.assertRaises(ValueError):
                    replay.verify_gate(needs, "acceptance")
            needs["plan"]["result"] = "failure"
            with self.assertRaises(ValueError):
                replay.verify_gate(needs, "acceptance")
        with self.assertRaises(ValueError):
            replay.verify_gate({}, "acceptance")


class WitnessTests(unittest.TestCase):
    def test_frozen_input_catalog_counts(self):
        self.assertEqual({suite: len(witness.case_ids(suite)) for suite in witness.SUITES}, {
            "primary": 672, "extra": 42, "followup": 156, "followup2": 162,
            "followup3": 48, "retained": 530, "direct": 32, "printer": 46, "bundle-sinks": 10,
        })

    def test_focused_selection_is_union_and_never_silent_empty(self):
        ids = witness.case_ids("followup3")
        self.assertEqual(len(witness.select_cases(ids, ["es2015/set/"])), 8)
        self.assertEqual(witness.select_cases(["a", "ab", "b"], ["a", "b"]), ["a", "ab", "b"])
        for needles in ([""], [" "], ["a,"], ["all"], ["this-id-does-not-exist"]):
            with self.assertRaises(ValueError):
                witness.select_cases(ids, needles)

    def test_all_overrides_inherited_focused_environment_and_uses_exact_target(self):
        env = {"TSC_RS_DECORATOR_SUPER_FOLLOWUP3_CASE_SET": "bad", "TSC_RS_RETAINED_ACCESSOR_CASE_SET": "edges",
               "TSC_RS_RETAINED_ACCESSOR_CASE_FILTER": "bad"}
        command, actual = witness.invocation("retained", [], env)
        self.assertEqual(actual["TSC_RS_RETAINED_ACCESSOR_CASE_SET"], "all")
        self.assertNotIn("TSC_RS_RETAINED_ACCESSOR_CASE_FILTER", actual)
        self.assertNotIn("TSC_RS_DECORATOR_SUPER_FOLLOWUP3_CASE_SET", actual)
        self.assertIn("--exact", command)
        self.assertIn("--manifest-path", command)
        self.assertNotIn("xtask", command)
        command, actual = witness.invocation("followup3", ["es2015/set/"], env)
        self.assertIn("h2_8a_decorator_super::decorator_super_followup3_forms_match_complete_typescript_observations", command)
        self.assertEqual(actual["TSC_RS_DECORATOR_SUPER_FOLLOWUP3_CASE_SET"], "es2015/set/")

    def test_cli_rejects_missing_or_invalid_selection_before_cargo(self):
        with patch.object(witness.subprocess, "run") as run:
            for argv in (["retained"], ["retained", "--case", "no-matching-case"], ["direct", "--case", "shared"],
                         ["printer", "--case", "recover-same"]):
                with self.assertRaises(SystemExit) as exit:
                    witness.main(argv)
                self.assertEqual(exit.exception.code, 2)
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
