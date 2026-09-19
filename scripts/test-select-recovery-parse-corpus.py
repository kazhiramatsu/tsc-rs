#!/usr/bin/env python3
"""Guard against silently dropping changed commands from recovery qualification."""
import copy
import importlib.util
import json
import hashlib
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("selector", Path(__file__).with_name("select-recovery-parse-corpus.py"))
selector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(selector)


class SelectionTests(unittest.TestCase):
    def setUp(self):
        self.snapshot = {"schema": 1, "kind": "emitter-recovery-parse-snapshot", "head": "head",
            "syntax_tree_hash": "syntax", "vendor_tree_hash": "vendor", "plan_manifest_sha256": "manifest",
            "digest_code_sha256": "digest", "inputs": {"emit": {}, "module": {}},
            "digests": {key: {"core": key, "profiles": {"literal": True}} for key in ["emit", "module"]},
            "input_manifest": {"manifest": "sha"}, "documents": {"doc": "base64"},
            "load_failures": [{"case_id": "unloaded", "error": "explicit failure"}],
            "rows": [{"case_id": "clean-command", "universe": "compiler", "loader": "emit",
                "emit_load_error": None, "emit_disposition": "pending-complete-command-comparison",
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

    def test_command_input_numbers_must_have_a_safe_integer_encoding(self):
        selector.validate_input_numbers({"good": [9007199254740991, -9007199254740991, True]})
        for number in [1.0, 0.5, 9007199254740992, -9007199254740992]:
            with self.assertRaises(AssertionError):
                selector.validate_input_numbers({"bad": number})

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

    def test_no_emit_fallback_cannot_lose_its_refusal_disposition(self):
        self.baselines["projection"]["digests"]["module"]["core"] = "before"
        row = self.snapshot["rows"][0]
        row["loader"] = "load_compiler_no_emit"
        row["emit_load_error"] = None
        row["emit_disposition"] = "pending-complete-command-comparison"
        with self.assertRaisesRegex(AssertionError, "lost emit refusal"):
            self.select()
        row["emit_load_error"] = "unsupported emit option"
        row["emit_disposition"] = "parse-admission-only; emit-not-qualified"
        self.assertEqual(self.select()["summary"]["selected_no_emit_fallbacks"], 1)

    def test_emit_loader_cannot_absorb_a_fallback_disposition(self):
        self.baselines["projection"]["digests"]["emit"]["core"] = "before"
        self.snapshot["rows"][0]["emit_load_error"] = "refused"
        with self.assertRaisesRegex(AssertionError, "carried a fallback"):
            self.select()

    def test_selected_command_requires_exact_loader_input(self):
        self.baselines["projection"]["digests"]["emit"]["core"] = "before"
        self.snapshot["rows"][0]["command_input"] = None
        with self.assertRaisesRegex(AssertionError, "no exact loader input"):
            self.select()

    def test_without_successor_preserves_the_original_selection_bytes(self):
        rendered = json.dumps(self.select(), ensure_ascii=False, separators=(",", ":")).encode()
        self.assertEqual(hashlib.sha256(rendered).hexdigest(),
                         "82a4c1e3d3d30bf198cd2dccdb1ff56d6af8cdd80fd12bd31dd402ca0d018089")

    def successor(self):
        for replay in [self.snapshot, self.current, *self.baselines.values()]:
            for digest in replay["digests"].values():
                digest["profiles"] = {key: key == "literal" for key in selector.PROFILE_KEYS}
        self.current["build"].update(parser_head=self.snapshot["head"], syntax_tree_hash=self.snapshot["syntax_tree_hash"],
            source_files_sha256={"Cargo.lock": "a" * 64,
            "crates/syntax/src/parser.rs": "a" * 64, "crates/syntax/src/recovery.rs": "a" * 64},
            probe_files_sha256={"probe.rs": "a" * 64})
        self.current["recovery_facts_format"] = "rust-debug-ParseRecovery-v1"
        self.current["recovery_facts_sha256"] = {id: "a" * 64 for id in self.snapshot["inputs"]}
        successor = copy.deepcopy(self.current)
        successor["build"].update(baseline_kind="successor", parser_head="b" * 40,
            syntax_tree_hash="c" * 40, binary_sha256="d" * 64, predicate_diff_sha256="e" * 64,
            reference_source_files_sha256=copy.deepcopy(self.current["build"]["source_files_sha256"]),
            changed_source_paths=["crates/syntax/src/recovery.rs"])
        successor["build"]["source_files_sha256"]["crates/syntax/src/recovery.rs"] = "f" * 64
        return successor

    def select_successor(self, successor):
        return selector.select(self.snapshot, "snapshot", self.current, self.baselines, self.reports, successor)

    def test_successor_module_profile_change_adds_rows_without_reclassifying_them(self):
        successor = self.successor()
        successor["digests"]["module"]["profiles"]["statement_gaps"] = True
        successor["digests"]["module"]["profiles"]["context_recovery"] = True
        result = self.select_successor(successor)
        self.assertEqual(result["head"], self.snapshot["head"])
        self.assertEqual(result["syntax_tree_hash"], self.snapshot["syntax_tree_hash"])
        self.assertEqual(result["successor"]["syntax_tree_hash"], "c" * 40)
        self.assertEqual(result["summary"]["successor_changed_inputs"], 1)
        self.assertEqual(result["cases"][0]["reasons"][0]["role"], "module-request-parse")
        self.assertFalse(result["cases"][0]["reasons"][0]["before"]["context_recovery"])
        self.assertTrue(result["cases"][0]["reasons"][0]["after"]["context_recovery"])
        self.assertNotIn("verdict", result["cases"][0]["reasons"][0])
        self.assertEqual(result["load_failures"], self.snapshot["load_failures"])
        self.baselines["merge-base"]["digests"]["emit"]["core"] = "old"
        self.assertEqual(len(self.select_successor(successor)["cases"][0]["reasons"]), 2)

    def test_successor_cannot_omit_inputs_drift_core_or_change_recovery_facts(self):
        original = self.successor()
        mutations = [
            lambda s: s["digests"]["emit"].update(core="changed"),
            lambda s: s["digests"].pop("emit"),
            lambda s: s["digests"].update(extra=s["digests"]["emit"]),
            lambda s: s.update(input_artifact_sha256="other"),
            lambda s: s["recovery_facts_sha256"].update(emit="b" * 64),
            lambda s: s["recovery_facts_sha256"].pop("emit"),
        ]
        for mutate in mutations:
            successor = copy.deepcopy(original)
            mutate(successor)
            with self.assertRaises(AssertionError): self.select_successor(successor)
        self.current["digests"]["emit"]["core"] = "changed"
        with self.assertRaisesRegex(AssertionError, "differs from actual census"):
            self.select_successor(original)

    def test_successor_requires_all_boolean_profiles_and_only_monotonic_statement_changes(self):
        original = self.successor()
        for key, value in [("literal", False), ("parameter_gaps", True), ("missing_await", True),
                           ("statement_gaps", None), ("context_recovery", 1), ("extra", True)]:
            successor = copy.deepcopy(original)
            successor["digests"]["emit"]["profiles"][key] = value
            with self.assertRaises(AssertionError): self.select_successor(successor)
        successor = copy.deepcopy(original)
        del successor["digests"]["emit"]["profiles"]["context_recovery"]
        with self.assertRaises(AssertionError): self.select_successor(successor)
        self.snapshot["digests"]["emit"]["profiles"]["context_recovery"] = True
        self.current["digests"]["emit"]["profiles"]["context_recovery"] = True
        with self.assertRaisesRegex(AssertionError, "removed admission"):
            self.select_successor(original)

    def test_successor_style_change_requires_the_exact_reviewed_pair(self):
        path, pair = next(iter(selector.SUCCESSOR_STYLE_SOURCE_PAIRS.items()))
        before, after = {path: pair[0]}, {path: pair[1]}
        self.assertEqual(selector.successor_source_changes(before, after), [path])
        for left, right in [(after, before), ({}, after), (before, {}),
                            ({path: "0" * 64}, after), (before, {path: "0" * 64})]:
            with self.assertRaisesRegex(AssertionError, "outside reviewed predicates"):
                selector.successor_source_changes(left, right)

    def test_successor_source_changes_are_limited_to_the_reviewed_predicate_and_test(self):
        original = self.successor()
        for path in ["Cargo.lock", "rust-toolchain.toml", "crates/syntax/src/parser.rs",
                     "crates/syntax/src/recovery/context.rs", "crates/types/src/lib.rs"]:
            successor = copy.deepcopy(original)
            successor["build"]["source_files_sha256"][path] = "9" * 64
            with self.assertRaisesRegex(AssertionError, "outside reviewed predicates"):
                self.select_successor(successor)
        successor = copy.deepcopy(original)
        successor["build"]["probe_files_sha256"]["probe.rs"] = "other"
        with self.assertRaisesRegex(AssertionError, "digest producer differs"):
            self.select_successor(successor)


if __name__ == "__main__":
    unittest.main()
