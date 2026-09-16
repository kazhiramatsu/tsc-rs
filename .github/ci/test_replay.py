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

    def test_emitter_direct_inputs_select_only_their_target_and_share_printer_build(self):
        for suite in witness.EMITTER_DIRECT:
            for path in witness.emitter_inputs(suite):
                with self.subTest(suite=suite, path=path):
                    self.assertTrue((ROOT / path).is_file(), path)
                    plan = replay.selection([path])
                    self.assertEqual(plan["acceptance"], [])
                    self.assertEqual(plan["witnesses"], [suite])
                    self.assertEqual(replay.matrices(plan)["witnesses"], {
                        "include": [{"group": "printer", "suites": [suite]}],
                    })

    def test_changed_direct_inputs_union_with_other_owners_without_full_replay(self):
        plan = replay.selection([
            "crates/emitter/tests/fixtures/template-raw-provenance.json",
            "ratchets/h2-8a-list-cursor-lifecycle.v1.json",
            "crates/emitter/tests/fixtures/printer-failure-hooks.json",
            "crates/compiler/tests/fixtures/decorator-super-followup3-inputs.json",
        ])
        self.assertEqual(plan["acceptance"], [])
        self.assertEqual(plan["witnesses"], ["followup3", "printer", "literal-value-provenance", "comma-argument-factory"])
        self.assertEqual(len(replay.matrices(plan)["witnesses"]["include"]), 2)

    def test_compiler_direct_inputs_select_only_their_target_in_controls(self):
        for suite in witness.COMPILER_DIRECT:
            for path in witness.compiler_direct_inputs(suite):
                with self.subTest(suite=suite, path=path):
                    self.assertTrue((ROOT / path).is_file(), path)
                    plan = replay.selection([path])
                    self.assertEqual(plan["acceptance"], [])
                    self.assertEqual(plan["witnesses"], [suite])
                    self.assertEqual(replay.matrices(plan)["witnesses"], {
                        "include": [{"group": "controls", "suites": [suite]}],
                    })

    def test_compiler_direct_union_and_shared_inputs_retain_other_owners(self):
        plan = replay.selection([
            "crates/compiler/tests/fixtures/utf16-noemit-command-controls.json",
            "crates/compiler/tests/h2_8a_utf16_review_fix_controls.rs",
            "crates/emitter/tests/fixtures/template-raw-provenance.json",
        ])
        self.assertEqual(plan["acceptance"], [])
        self.assertEqual(plan["witnesses"], ["literal-value-provenance", "utf16-identity-recovery", "utf16-review-fix"])
        self.assertEqual(len(replay.matrices(plan)["witnesses"]["include"]), 2)
        for path in ("crates/compiler/tests/fixtures/utf16-literals-adjacent-probes-inputs.json",
                     "vendor/typescript-6.0.3/lib/typescript.js"):
            plan = replay.selection([path])
            self.assertEqual(plan["acceptance"], list(replay.GROUPS))
            self.assertEqual(plan["witnesses"], list(witness.SUITES))

    def test_witness_groups_cover_every_suite_once(self):
        suites = [suite for group in replay.WITNESS_GROUPS.values() for suite in group]
        self.assertCountEqual(suites, witness.SUITES)

    def test_declaration_map_cli_wrapper_and_shared_program_helper_have_distinct_owners(self):
        plan = replay.selection(["crates/compiler/tests/h2_7e_original_corpus.rs"])
        self.assertEqual(plan["acceptance"], [])
        self.assertEqual(plan["witnesses"], ["declaration-map-cli"])
        self.assertEqual(replay.matrices(plan)["witnesses"], {
            "include": [{"group": "controls", "suites": ["declaration-map-cli"]}],
        })
        shared = replay.selection(["crates/compiler/tests/integration/h2_7e_original_corpus_shared.rs"])
        self.assertEqual(shared["acceptance"], ["late"])
        self.assertEqual(shared["witnesses"], ["declaration-map-cli"])
        for path in ("ratchets/h2-7de-observations.v1.json", "ratchets/h2-7de-candidate-inputs.v1.json"):
            # These immutable joins also serve D283, directories and other slices.
            self.assertEqual(replay.selection([path])["acceptance"], list(replay.GROUPS))

    def test_shared_comparator_is_an_acceptance_input(self):
        plan = replay.selection(["crates/compiler/tests/integration/h2_7c_declaration_blocking.rs"])
        self.assertEqual(plan["acceptance"], ["late"])
        self.assertEqual(plan["witnesses"], ["retained", "utf16-literal-witnesses"])

    def test_library_snapshot_keeps_all_fresh_program_consumers(self):
        plan = replay.selection(["crates/compiler/tests/support/witness_libraries.rs"])
        self.assertEqual(plan["acceptance"], ["late"])
        self.assertEqual(plan["witnesses"], [*witness.SUPER, "retained", "utf16-literal-witnesses"])

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
            "followup3": 48, "retained": 530, "direct": 32, "printer": 70, "bundle-sinks": 10,
            "declaration-map-cli": 8, "transpile-routes": 301,
            "literal-parent-provenance": 128, "literal-value-provenance": 540,
            "string-literal-identifier-source": 72, "utf16-literal-escaping": 296,
            "class-header-token-metadata": 32, "comma-argument-factory": 519,
            "ellipsis-comment-metadata": 144, "import-type-attributes": 84,
            "mapped-type-members": 328, "token-comment-phase-metadata": 96,
            "utf16-identity-recovery": 79, "utf16-review-fix": 25, "utf16-tagged-template": 16,
            "utf16-literal-witnesses": 64, "utf16-original-commands": 4,
        })

    def test_transpile_runner_requires_all_nine_tests_after_both_oracles(self):
        summary = "test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"
        def fake_run(command, **kwargs):
            return subprocess.CompletedProcess(command, 0, summary)
        with patch.object(witness.subprocess, "run", side_effect=fake_run) as run:
            witness.run_compiler_direct(["transpile-routes"])
            self.assertEqual(run.call_args_list[0].args[0], ["node", "scripts/observe-transpile-routes.mjs", "--check"])
            self.assertIn("transpile_routes_contract", run.call_args_list[-1].args[0])
        for output in (summary.replace("9 passed", "5 passed"), summary.replace("9 passed", "0 passed"), summary.replace("0 ignored", "1 ignored")):
            with patch.object(witness.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, output)):
                with self.assertRaises(ValueError):
                    witness.run_compiler_direct(["transpile-routes"])

    def test_direct_catalog_rejects_missing_duplicate_and_empty_ids(self):
        for rows in ([], [{"case_id": "a"}] * 128, [{"case_id": ""}] * 128):
            with patch.object(witness, "read_cases", return_value=rows):
                with self.assertRaises(ValueError):
                    witness.case_ids("literal-parent-provenance")

    def test_direct_runner_batches_only_selected_targets_and_rejects_partial_success(self):
        selected = ["literal-value-provenance", "import-type-attributes"]
        summary = "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"
        full = summary * 2
        def run_with(output, status=0):
            with patch.object(witness.subprocess, "run", return_value=subprocess.CompletedProcess([], status, output)) as run:
                witness.run_emitter_direct(selected)
                return run.call_args_list
        calls = run_with(full)
        self.assertEqual([call.args[0] for call in calls[:-1]], [
            ["node", "scripts/observe-template-raw-provenance.mjs", "--check"],
            ["node", "scripts/observe-string-property-provenance.mjs", "--check"],
            ["node", "scripts/observe-import-type-attributes.mjs", "--check"],
        ])
        command = calls[-1].args[0]
        self.assertEqual([command[i + 1] for i, arg in enumerate(command) if arg == "--test"],
                         ["literal_value_provenance_contract", "import_type_attributes_contract"])
        self.assertNotIn("--exact", command)
        for output in ("", summary, full + summary, full.replace("2 passed", "0 passed", 1),
                       full.replace("0 ignored", "1 ignored", 1), full.replace("0 filtered out", "1 filtered out", 1)):
            with self.subTest(output=output), self.assertRaises(ValueError):
                run_with(output)
        with self.assertRaises(subprocess.CalledProcessError):
            run_with(full, 101)

    def test_direct_runner_propagates_observer_failure_before_cargo(self):
        with patch.object(witness.subprocess, "run", side_effect=subprocess.CalledProcessError(1, ["node"])) as run:
            with self.assertRaises(subprocess.CalledProcessError):
                witness.run_emitter_direct(["import-type-attributes"])
            self.assertEqual(run.call_count, 1)
        for suites in ([], ["printer"], ["import-type-attributes"] * 2):
            with self.assertRaises(ValueError):
                witness.emitter_command(suites)

    def test_compiler_direct_runner_batches_only_selected_targets_and_rejects_partial_success(self):
        selected = ["utf16-identity-recovery", "utf16-tagged-template"]
        summary = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"
        full = summary.replace("1 passed", "2 passed") + summary
        def run_with(output, status=0):
            with patch.object(witness.subprocess, "run", return_value=subprocess.CompletedProcess([], status, output)) as run:
                witness.run_compiler_direct(selected)
                return run.call_args_list
        calls = run_with(full)
        self.assertEqual([call.args[0] for call in calls[:-1]], [
            ["node", "scripts/observe-utf16-identity-recovery-controls.mjs", "--check"],
            ["node", "scripts/observe-utf16-noemit-command-controls.mjs", "--check"],
            ["node", "scripts/observe-utf16-tagged-template-controls.mjs", "--check"],
        ])
        command = calls[-1].args[0]
        self.assertEqual(command, ["cargo", "test", "--manifest-path", "crates/compiler/Cargo.toml",
                                  "--test", "h2_8a_utf16_identity_recovery_controls",
                                  "--test", "h2_8a_utf16_tagged_template_controls",
                                  "--", "--nocapture", "--test-threads=1"])
        for output in ("", summary, full + summary, full.replace("2 passed", "0 passed", 1),
                       full.replace("2 passed", "1 passed", 1), full.replace("0 ignored", "1 ignored", 1),
                       full.replace("0 filtered out", "1 filtered out", 1)):
            with self.subTest(output=output), self.assertRaises(ValueError):
                run_with(output)
        with self.assertRaises(subprocess.CalledProcessError):
            run_with(full, 101)

    def test_compiler_direct_observer_failure_stops_before_cargo(self):
        with patch.object(witness.subprocess, "run", side_effect=subprocess.CalledProcessError(1, ["node"])) as run:
            with self.assertRaises(subprocess.CalledProcessError):
                witness.run_compiler_direct(["utf16-review-fix"])
            self.assertEqual(run.call_count, 1)
        for suites in ([], ["printer"], ["utf16-review-fix"] * 2):
            with self.assertRaises(ValueError):
                witness.compiler_direct_command(suites)

    def test_literal_witnesses_keep_shared_helpers_and_qualification_owners(self):
        plan = replay.selection(["crates/compiler/tests/integration/h2_7c_declaration_blocking.rs"])
        self.assertEqual(plan["acceptance"], ["late"])
        self.assertEqual(plan["witnesses"], ["retained", "utf16-literal-witnesses"])
        for path in ("crates/compiler/tests/integration/h2_7b_w4a_controls.rs",
                     "ratchets/h2-5h-qualification.v1.json",
                     "crates/oracle/vfs-directory-overlay.mjs",
                     "crates/compiler/tests/fixtures/utf16-literals-adjacent-probes.json"):
            plan = replay.selection([path])
            self.assertEqual(plan["acceptance"], list(replay.GROUPS))
            self.assertEqual(plan["witnesses"], list(witness.SUITES))

    def test_literal_runner_checks_each_group_and_only_the_dedicated_test(self):
        selected = ["utf16-literal-witnesses", "utf16-original-commands"]
        summary = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"
        filtered = summary.replace("0 filtered out", "9 filtered out")
        def run_with(output=filtered, status=0):
            def result(command, **kwargs):
                if command[0] == "node":
                    return subprocess.CompletedProcess(command, 0)
                return subprocess.CompletedProcess(command, status, output if "--exact" in command else summary)
            with patch.object(witness.subprocess, "run", side_effect=result) as run:
                witness.run_compiler_direct(selected)
                return run.call_args_list
        calls = run_with()
        self.assertEqual([call.args[0] for call in calls[:4]], [
            ["node", "scripts/observe-utf16-literal-witnesses.mjs", group, "--check"]
            for group in ("string-literals", "template-literals", "bundle-prologues")
        ] + [["node", "scripts/observe-utf16-original-rows-complete.mjs", "--check"]])
        self.assertEqual(len(calls), 6)
        command = calls[-1].args[0]
        self.assertEqual(command, ["cargo", "test", "--manifest-path", "crates/compiler/Cargo.toml",
                                  "--test", "h2_5h_utf16_literal_witnesses",
                                  "utf16_literal_witnesses_match_complete_typescript_observations",
                                  "--", "--exact", "--nocapture", "--test-threads=1"])
        for output in ("", summary, filtered.replace("1 passed", "0 passed"),
                       filtered.replace("9 filtered", "8 filtered"),
                       filtered.replace("9 filtered", "10 filtered"),
                       filtered.replace("0 ignored", "1 ignored"), filtered * 2):
            with self.subTest(output=output), self.assertRaises(ValueError):
                run_with(output)
        with self.assertRaises(subprocess.CalledProcessError):
            run_with(status=101)
        with self.assertRaises(ValueError):
            witness.compiler_direct_command(selected)

    def test_literal_suite_clears_inherited_internal_case_selectors(self):
        poisoned = {"TSC_RS_UTF16_LITERAL_WITNESS_SET": "adjacent-probes",
                    "TSC_RS_UTF16_LITERAL_WITNESS_FILTER": "string-escape",
                    "CARGO_BUILD_JOBS": "2"}
        for suite in witness.COMPILER_DIRECT:
            _, env = witness.invocation(suite, [], poisoned)
            self.assertNotIn("TSC_RS_UTF16_LITERAL_WITNESS_SET", env)
            self.assertNotIn("TSC_RS_UTF16_LITERAL_WITNESS_FILTER", env)
        self.assertEqual(poisoned["TSC_RS_UTF16_LITERAL_WITNESS_SET"], "adjacent-probes")

    def test_compiler_direct_catalog_rejects_empty_duplicate_and_changed_memberships(self):
        for rows in ([], [{"id": "a"}] * 25, [{"id": ""}] * 25, [{"id": str(i)} for i in range(24)]):
            with patch.object(witness, "read_cases", return_value=rows):
                with self.assertRaises(ValueError):
                    witness.case_ids("utf16-review-fix")

    def test_compiler_direct_list_and_dry_run_never_start_processes(self):
        with patch.object(witness.subprocess, "run") as run:
            for suite in witness.COMPILER_DIRECT:
                self.assertEqual(witness.main([suite, "--list"]), 0)
                self.assertEqual(witness.main([suite, "--all", "--dry-run"]), 0)
            run.assert_not_called()

    def test_declaration_map_cli_runs_only_its_exact_test_and_rejects_zero_success(self):
        command, env = witness.invocation("declaration-map-cli", [], {})
        self.assertEqual(command, ["cargo", "test", "--manifest-path", "crates/compiler/Cargo.toml",
                                  "--test", "h2_7e_original_corpus",
                                  "h2_7e_original_cli_matches_outputs_diagnostics_and_exit_twice",
                                  "--", "--exact", "--nocapture", "--test-threads=1"])
        summary = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out;"
        for output, code in ((summary, 0), ("", 0), (summary.replace("1 passed", "0 passed"), 0),
                             (summary.replace("0 ignored", "1 ignored"), 0), (summary, 101)):
            with self.subTest(output=output, code=code):
                with patch.object(witness.subprocess, "run", return_value=subprocess.CompletedProcess(command, code, output)):
                    if output == summary and code == 0:
                        witness.run_declaration_map_cli(command, env)
                    else:
                        with self.assertRaises((ValueError, subprocess.CalledProcessError)):
                            witness.run_declaration_map_cli(command, env)

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
                         ["printer", "--case", "recover-same"],
                         ["declaration-map-cli"], ["declaration-map-cli", "--case", "declarationMaps"],
                         ["literal-value-provenance"], ["literal-value-provenance", "--case", "template"]):
                with self.assertRaises(SystemExit) as exit:
                    witness.main(argv)
                self.assertEqual(exit.exception.code, 2)
            for suite in witness.COMPILER_DIRECT:
                for argv in ([suite], [suite, "--case", witness.case_ids(suite)[0]]):
                    with self.assertRaises(SystemExit) as exit:
                        witness.main(argv)
                    self.assertEqual(exit.exception.code, 2)
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
