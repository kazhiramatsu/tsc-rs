#!/usr/bin/env python3
"""Guard against silently dropping changed commands from recovery qualification."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("selector", Path(__file__).with_name("select-recovery-parse-corpus.py"))
selector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(selector)


class SelectionTests(unittest.TestCase):
    def setUp(self):
        self.snapshot = {"schema": 1, "kind": "emitter-recovery-parse-snapshot", "head": "head",
            "digest_code_sha256": "digest", "inputs": {"emit": {}, "module": {}},
            "digests": {key: {"core": key, "profiles": {"literal": True}} for key in ["emit", "module"]},
            "input_manifest": {"manifest": "sha"}, "documents": {"doc": "base64"},
            "load_failures": [{"case_id": "unloaded", "error": "explicit failure"}],
            "rows": [{"case_id": "clean-command", "universe": "compiler", "loader": "emit",
                "units": [{"input_id": key, "path": key + ".ts", "role": role}
                    for key, role in [("emit", "emit-preflight"), ("module", "module-request-parse")]],
                "command_input": {"route": "recorded-compiler", "units": [{"content_sha256": "doc"}]}}]}
        self.current = {"schema": 1, "kind": "emitter-recovery-parse-replay", "input_artifact_sha256": "snapshot",
            "digest_code_sha256": "digest", "build": {"baseline_kind": "candidate"},
            "digests": copy.deepcopy(self.snapshot["digests"])}
        self.baselines = {name: copy.deepcopy(self.current) for name in ["projection", "merge-base"]}
        for name, replay in self.baselines.items():
            replay["build"]["baseline_kind"] = name
        self.reports = {name: {"schema": 1, "head": "head", "inputs": self.snapshot["input_manifest"],
            "summary": {"rows": 1}, "load_failures": self.snapshot["load_failures"],
            "newly_admitted": [], "newly_refused": []} for name in selector.PROFILES}

    def select(self):
        return selector.select(self.snapshot, "snapshot", self.current, self.baselines, self.reports)

    def test_module_only_change_selects_parse_clean_command_and_documents(self):
        self.baselines["projection"]["digests"]["module"]["core"] = "before"
        result = self.select()
        self.assertEqual(result["summary"]["selected_rows"], 1)
        self.assertEqual(result["cases"][0]["reasons"][0]["role"], "module-request-parse")
        self.assertEqual(result["documents"], {"doc": "base64"})
        self.assertEqual(result["load_failures"], self.snapshot["load_failures"])

    def test_merge_base_core_and_projection_profile_changes_are_unioned(self):
        self.baselines["merge-base"]["digests"]["emit"]["core"] = "old"
        self.baselines["projection"]["digests"]["emit"]["profiles"]["literal"] = False
        result = self.select()
        self.assertEqual(len(result["cases"]), 1)
        self.assertEqual({r["baseline"] for r in result["cases"][0]["reasons"]}, {"projection", "merge-base"})

    def test_all_five_profile_directions_select_without_parser_delta(self):
        for name in selector.PROFILES:
            for verdict in ["newly_admitted", "newly_refused"]:
                with self.subTest(profile=name, verdict=verdict):
                    self.reports[name][verdict] = [self.snapshot["rows"][0]]
                    self.assertEqual(self.select()["summary"]["selected_rows"], 1)
                    self.reports[name][verdict] = []

    def test_unchanged_commands_remain_counted_and_failures_remain_visible(self):
        result = self.select()
        self.assertEqual(result["summary"]["unchanged_rows"], 1)
        self.assertEqual(result["summary"]["load_failures"], 1)
        self.assertEqual(result["cases"], [])

    def test_missing_replay_input_and_current_drift_fail_closed(self):
        del self.baselines["projection"]["digests"]["module"]
        with self.assertRaisesRegex(AssertionError, "omitted/added"):
            self.select()
        self.baselines["projection"]["digests"]["module"] = copy.deepcopy(self.current["digests"]["module"])
        self.current["digests"]["emit"]["core"] = "drift"
        with self.assertRaisesRegex(AssertionError, "differs from actual census"):
            self.select()

    def test_selected_command_requires_exact_loader_input(self):
        self.baselines["projection"]["digests"]["emit"]["core"] = "before"
        self.snapshot["rows"][0]["command_input"] = None
        with self.assertRaisesRegex(AssertionError, "no exact loader input"):
            self.select()


if __name__ == "__main__":
    unittest.main()
