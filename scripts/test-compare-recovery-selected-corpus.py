#!/usr/bin/env python3
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("compare", Path(__file__).with_name("compare-recovery-selected-corpus.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ComparisonTests(unittest.TestCase):
    def inputs(self, fallback=False):
        command_input = {"route": "recorded-compiler", "settings": [["target", "es5"]]}
        sha = hashlib.sha256(json.dumps(command_input, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        row = {"case_id": "a", "loader": "load_compiler_no_emit" if fallback else "load_compiler_emit",
               "command_input": command_input, "reasons": [{"baseline": "projection"}]}
        command = {"writes": [{"callback": {"utf16": [0xd800], "utf8_base64": "77+9"}}]}
        observed = {"case_id": "a", "input_sha256": sha, "options": {"sourceMap": None},
                    "disposition": "parse-admission-only; emit-not-qualified" if fallback else "observed",
                    "complete_command_runs": [copy.deepcopy(command), copy.deepcopy(command)]}
        shared = {"schema": 1, "selection_sha256": "selection-sha", "input_workspace": "/input", "library_root": "/input/lib",
                  "load_failures": [], "repetitions": 2, "head": "code-head", "cases": [observed]}
        native = {**copy.deepcopy(shared), "kind": "emitter-recovery-native-observations"}
        oracle = {**copy.deepcopy(shared), "kind": "emitter-recovery-typescript-observations"}
        selection = {"head": "census-head", "load_failures": [], "cases": [row]}
        return selection, native, oracle

    def compare(self, values):
        selection, native, oracle = values
        return module.compare(selection, "selection-sha", native, oracle)["cases"][0]["disposition"]

    def test_utf16_diff_cannot_hide_behind_equal_utf8_projection(self):
        values = self.inputs()
        self.assertEqual(self.compare(values), "complete-command-exact")
        for run in values[2]["cases"][0]["complete_command_runs"]:
            run["writes"][0]["callback"]["utf16"] = [0xfffd]
        self.assertEqual(self.compare(values), "complete-command-mismatch; emit-not-qualified")

    def test_equal_output_cannot_qualify_different_options(self):
        values = self.inputs()
        values[2]["cases"][0]["options"]["sourceMap"] = False
        self.assertEqual(self.compare(values), "input-option-mismatch; emit-not-qualified")

    def test_no_emit_exact_is_never_emit_qualification(self):
        self.assertEqual(self.compare(self.inputs(True)), "no-emit-command-exact; emit-not-qualified")

    def test_input_reconstruction_error_blocks_only_that_row(self):
        values = self.inputs()
        observation = values[2]["cases"][0]
        observation.update(disposition="input-reconstruction-mismatch; emit-not-qualified",
                           input_reconstruction_error="loaded sources differ", complete_command_runs=[], options=None)
        self.assertEqual(self.compare(values), "input-reconstruction-mismatch; emit-not-qualified")

    def test_typescript_nondeterminism_is_not_an_input_reconstruction_failure(self):
        values = self.inputs()
        observation = values[2]["cases"][0]
        observation.update(disposition="typescript-repetition-mismatch; emit-not-qualified",
                           typescript_repetition_error="different repetitions", observations=[{"writes": []}, {"writes": ["different"]}])
        self.assertEqual(self.compare(values), "typescript-repetition-mismatch; emit-not-qualified")

    def test_missing_coverage_or_input_drift_is_fatal(self):
        values = self.inputs()
        values[1]["cases"] = []
        with self.assertRaises(AssertionError): self.compare(values)
        values = self.inputs()
        values[1]["cases"][0]["input_sha256"] = "other"
        with self.assertRaises(AssertionError): self.compare(values)


if __name__ == "__main__":
    unittest.main()
