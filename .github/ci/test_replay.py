"""Selection and gate contracts; no Rust build or witness replay."""
from contextlib import redirect_stdout
import io
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("replay", ROOT / ".github/ci/replay.py")
replay = importlib.util.module_from_spec(spec)
spec.loader.exec_module(replay)
witness = replay.witness


class CompilerBudgetTests(unittest.TestCase):
    def test_module_output_partition_preserves_all_suites_and_dedicated_selection(self):
        members = [suite for suites in replay.WITNESS_GROUPS.values() for suite in suites]
        self.assertCountEqual(members, witness.SUITES)
        self.assertEqual(len(members), len(set(members)))
        self.assertEqual(len(replay.WITNESS_GROUPS), 7)
        self.assertEqual(replay.WITNESS_GROUPS["module-output"],
                         ("declaration-map-apis", "declaration-maps", "require-rewrite", "declaration-specifiers"))
        workflow = (ROOT / ".github/workflows/witness.yml").read_text()
        for suite in ("require-rewrite", "declaration-specifiers"):
            plan = replay.selection([f"crates/compiler/tests/fixtures/h2-8a-{suite}.json"])
            self.assertEqual(plan["acceptance"], [])
            self.assertEqual(replay.matrices(plan)["witnesses"], {
                "include": [{"group": "module-output", "suites": [suite]}]})
            self.assertNotIn(suite, replay.WITNESS_GROUPS["controls"])
            self.assertIn(f"contains(matrix.suites, '{suite}')", workflow)
        self.assertEqual(replay.selection(["crates/emitter/src/printer.rs"])["witnesses"],
                         list(witness.SUITES))

    def test_step_records_deduplicate_shared_observers_and_keep_shared_cargo_cost(self):
        suites = ["declaration-map-apis", "declaration-maps"]
        observers = witness.compiler_direct_observers(suites)
        good = ("test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"
                "test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n")
        output = io.StringIO()

        def run(command, **kwargs):
            events = [json.loads(line) for line in output.getvalue().splitlines() if line.startswith('{"witness_step"')]
            self.assertEqual(events[-1]["event"], "start")
            self.assertEqual(events[-1]["argv"], list(command))
            return subprocess.CompletedProcess(command, 0, good)

        with patch.object(witness.subprocess, "run", side_effect=run) as calls, redirect_stdout(output):
            witness.run_compiler_direct(suites)
        self.assertEqual([call.args[0] for call in calls.call_args_list],
                         [list(command) for command in observers] + [witness.compiler_direct_command(suites)])
        events = [json.loads(line) for line in output.getvalue().splitlines() if line.startswith('{"witness_step"')]
        self.assertEqual(len(events), 2 * (len(observers) + 1))
        for start, finish in zip(events[::2], events[1::2]):
            self.assertEqual(finish["argv"], start["argv"])
            self.assertEqual(finish["status"], "passed")
            self.assertGreaterEqual(finish["seconds"], 0)
        shared = next(event for event in events if event["phase"] == "observer"
                      and event["argv"][1] == "scripts/observe-declaration-map-apis.mjs")
        self.assertEqual(shared["suites"], suites)
        self.assertEqual(events[-1]["phase"], "cargo-build-and-replay")
        self.assertEqual(events[-1]["suites"], suites)

    def test_observer_failure_is_timed_and_prevents_cargo_and_success_summary(self):
        output = io.StringIO()
        error = subprocess.CalledProcessError(7, ["node"])
        with patch.object(witness.subprocess, "run", side_effect=error) as calls, redirect_stdout(output):
            with self.assertRaises(subprocess.CalledProcessError) as raised:
                witness.run_compiler_direct(["require-rewrite"])
        self.assertIs(raised.exception, error)
        self.assertEqual(calls.call_count, 1)
        events = [json.loads(line) for line in output.getvalue().splitlines() if line.startswith('{"witness_step"')]
        self.assertEqual(len(events), 2)
        self.assertEqual(events[-1]["status"], "failed")
        self.assertEqual(events[-1]["exit_code"], 7)
        self.assertNotIn('"tests_passed"', output.getvalue())

    def test_cargo_failure_or_invalid_membership_never_gets_passed_timing(self):
        good = "test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"
        for status, text, error in ((101, good, subprocess.CalledProcessError),
                                    (0, good.replace("9 passed", "0 passed"), ValueError)):
            output = io.StringIO()
            def run(command, **kwargs):
                return subprocess.CompletedProcess(command, status if command[0] == "cargo" else 0, text)
            with self.subTest(status=status), patch.object(witness.subprocess, "run", side_effect=run), redirect_stdout(output):
                with self.assertRaises(error):
                    witness.run_compiler_direct(["transpile-routes"])
            events = [json.loads(line) for line in output.getvalue().splitlines() if line.startswith('{"witness_step"')]
            self.assertEqual(events[-1]["phase"], "cargo-build-and-replay")
            self.assertEqual(events[-1]["status"], "failed")
            self.assertEqual(events[-1]["error"], error.__name__)
            self.assertNotIn('"tests_passed"', output.getvalue())


class FoundationTests(unittest.TestCase):
    def output(self, suites):
        foundation = witness.foundation_witnesses
        blocks = []
        for suite in suites:
            target = foundation.SUITES[suite]["target"]
            names = foundation.test_names(suite)
            blocks.append(f"     Running tests/{target}.rs (target/debug/deps/{target}-012345)\n" +
                          "\n".join(f"test {name} ... ok" for name in names) +
                          f"\ntest result: ok. {len(names)} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n")
        return "\n".join(blocks)

    def test_foundation_sources_have_one_bounded_job_and_all_targets(self):
        foundation = witness.foundation_witnesses
        self.assertEqual(len(foundation.SUITES), 16)
        self.assertEqual(len({(s["crate"], s["target"]) for s in foundation.SUITES.values()}), 16)
        for suite in foundation.SUITES:
            plan = replay.selection([foundation.source(suite)])
            self.assertEqual(plan["acceptance"], [])
            self.assertEqual(plan["witnesses"], [suite])
            self.assertEqual(replay.matrices(plan)["witnesses"], {
                "include": [{"group": "foundations", "suites": [suite]}]})
            for file in foundation.inputs(suite):
                self.assertTrue((ROOT / file).is_file(), file)
                self.assertIn(suite, replay.selection([file])["witnesses"], file)
        self.assertIn("matrix.group == 'foundations'", (ROOT / ".github/workflows/witness.yml").read_text())

    def test_foundation_shared_dependencies_preserve_existing_consumers(self):
        for file in ("crates/compiler/tests/fixtures/utf16-literals-adjacent-probes-inputs.json",
                     "crates/host/tests/support/scalar_path.rs", "scripts/foundation_witnesses.py",
                     "crates/program/src/program.rs", "crates/syntax/src/parser.rs"):
            self.assertEqual(replay.selection([file])["acceptance"], list(replay.GROUPS))
            self.assertEqual(replay.selection([file])["witnesses"], list(witness.SUITES))
        for file in ("crates/emitter/tests/fixtures/bundle-plan.json", "scripts/observe-bundle-plan.mjs"):
            selected = replay.selection([file])["witnesses"]
            self.assertIn("bundle-program", selected)
            self.assertIn("program-bundle-facts", selected)

    def test_foundation_platform_names_are_explicit(self):
        foundation = witness.foundation_witnesses
        for platform, memory, filesystem in (("linux", 14, 9), ("darwin", 14, 8), ("win32", 13, 7)):
            self.assertEqual(len(foundation.test_names("host-memory", platform)), memory)
            self.assertEqual(len(foundation.test_names("host-filesystem", platform)), filesystem)
        with self.assertRaises(ValueError):
            foundation.test_names("host-memory", "unreviewed")

    def test_foundation_missing_duplicate_filtered_ignored_and_wrong_tests_fail(self):
        foundation = witness.foundation_witnesses
        suites = ["syntax-entity-names", "syntax-template-flags"]
        output = self.output(suites)
        self.assertEqual(foundation.verify_output(suites, output), 3)
        for broken in ("", self.output(suites[:1]), output + output,
                       output.replace("0 filtered out", "1 filtered out", 1),
                       output.replace("0 ignored", "1 ignored", 1),
                       output.replace("test entity_and_identifier_predicates_match_typescript_utf16_values", "test wrong_name"),
                       output.replace("2 passed", "0 passed", 1)):
            with self.subTest(output=broken), self.assertRaises(ValueError):
                foundation.verify_output(suites, broken)

    def test_foundation_cli_selection_and_dry_run_never_execute(self):
        for suite in witness.foundation_witnesses.SUITES:
            with self.assertRaises(ValueError):
                witness.invocation(suite, ["anything"])
            with patch.object(witness.subprocess, "run") as run, redirect_stdout(io.StringIO()):
                self.assertEqual(witness.main([suite, "--all", "--dry-run"]), 0)
                run.assert_not_called()

    def test_foundation_batches_selected_targets_and_checks_oracles_first(self):
        foundation = witness.foundation_witnesses
        suites = ["syntax-entity-names", "syntax-template-flags"]
        fake = subprocess.CompletedProcess([], 0, self.output(suites))
        with patch.object(foundation.subprocess, "run", return_value=fake) as run, redirect_stdout(io.StringIO()):
            foundation.run(suites, {})
        self.assertEqual(run.call_count, 3)
        self.assertEqual([call.args[0][0] for call in run.call_args_list], ["node", "node", "cargo"])
        self.assertEqual(run.call_args_list[-1].args[0], foundation.command(suites))
        with self.assertRaises(ValueError):
            foundation.command(["syntax-entity-names", "host-memory"])
        for suites in ([], ["host-memory", "host-memory"], ["unknown"]):
            with self.assertRaises(ValueError):
                foundation.run(suites, {})

    def test_foundation_observer_and_native_failures_propagate(self):
        foundation = witness.foundation_witnesses
        with patch.object(foundation.subprocess, "run", side_effect=subprocess.CalledProcessError(1, ["node"])) as run:
            with self.assertRaises(subprocess.CalledProcessError):
                foundation.run(["syntax-entity-names"], {})
            self.assertEqual(run.call_count, 1)
        with patch.object(foundation.subprocess, "run", return_value=subprocess.CompletedProcess([], 101, "failure")), redirect_stdout(io.StringIO()):
            with self.assertRaises(subprocess.CalledProcessError):
                foundation.run(["host-memory"], {})

    def test_foundation_unreviewed_attributes_and_empty_declarations_fail(self):
        for text in ("", "#[ignore]\n#[test]\nfn omitted() {}", "#[cfg(feature = \"hidden\")]\n#[test]\nfn hidden() {}"):
            with patch.object(Path, "read_text", return_value=text), self.assertRaises(ValueError):
                witness.foundation_witnesses.test_names("host-memory")


class SelectionTests(unittest.TestCase):
    def test_post_t1_inputs_use_pipeline_job_and_pinned_node_alone_or_combined(self):
        suite = "post-t1-residuals"
        self.assertNotIn(suite, replay.WITNESS_GROUPS["controls"])
        for path in witness.compiler_direct_inputs(suite):
            plan = replay.selection([path])
            self.assertEqual(plan["acceptance"], [])
            self.assertEqual(plan["witnesses"], [suite])
            self.assertEqual(replay.matrices(plan)["witnesses"], {
                "include": [{"group": "decorator-binding-pipeline", "suites": [suite]}],
            })
        plan = replay.selection([
            "crates/compiler/tests/post_t1_residuals_contract.rs",
            "crates/compiler/tests/decorator_binding_pipeline_contract.rs",
        ])
        self.assertEqual(set(plan["witnesses"]), {suite, "decorator-binding-pipeline"})
        matrix = replay.matrices(plan)["witnesses"]["include"]
        self.assertEqual(len(matrix), 1)
        self.assertEqual(matrix[0]["group"], "decorator-binding-pipeline")
        self.assertEqual(set(matrix[0]["suites"]), set(plan["witnesses"]))
        workflow = (ROOT / ".github/workflows/witness.yml").read_text()
        self.assertIn(f"contains(matrix.suites, '{suite}')", workflow)

    def test_post_t1_selection_and_dump_environment_are_cleared(self):
        dirty = {key: "stale" for key in (
            "TSC_RS_POST_T1_RESIDUALS_CASE_SET", "TSC_RS_POST_T1_RESIDUALS_DUMP_DIR",
            "TSC_RS_BUNDLE_METADATA_T1_CASE_SET", "TSC_RS_BUNDLE_METADATA_T1_DUMP_DIR",
        )}
        for suite in witness.COMPILER_DIRECT:
            _, env = witness.invocation(suite, [], dirty)
            self.assertTrue(set(dirty).isdisjoint(env), suite)

    def test_bundle_metadata_t1_uses_controls_and_pinned_node_when_selected_alone(self):
        suite = "bundle-metadata-t1"
        for path in witness.compiler_direct_inputs(suite):
            plan = replay.selection([path])
            self.assertEqual(plan["acceptance"], [])
            self.assertEqual(plan["witnesses"], [suite])
            self.assertEqual(replay.matrices(plan)["witnesses"], {
                "include": [{"group": "controls", "suites": [suite]}],
            })
        workflow = (ROOT / ".github/workflows/witness.yml").read_text()
        self.assertIn(f"contains(matrix.suites, '{suite}')", workflow)

    def test_module_facets_cover_shared_targets_without_narrowing_shared_inputs(self):
        suites = ["module-identities", "bundle-original-javascript"]
        plan = replay.selection([
            "crates/compiler/tests/h2_7d_module_identities.rs",
            "scripts/observe-bundle-original-javascript.mjs",
        ])
        self.assertEqual(plan["acceptance"], [])
        self.assertEqual(plan["witnesses"], suites)
        self.assertEqual(replay.matrices(plan)["witnesses"], {
            "include": [{"group": "controls", "suites": suites}],
        })
        self.assertTrue(set(suites).issubset(replay.WITNESS_GROUPS["controls"]))
        self.assertEqual(replay.selection(["crates/compiler/tests/h2_7d_declaration_bundles.rs"])["witnesses"],
                         ["bundle-original-javascript", "bundle-declarations"])
        for path in ("crates/emitter/tests/fixtures/bundle-module-identities.json",
                     "ratchets/h2-7de-candidate-inputs.v1.json", "ratchets/h2-7de-observations.v1.json",
                     "crates/oracle/h2-7de-observations.mjs", "crates/oracle/h2-7de-candidates.mjs",
                     "crates/oracle/vfs-directory-overlay.mjs", "crates/host/tests/support/scalar_path.rs"):
            self.assertEqual(replay.selection([path])["witnesses"], list(witness.SUITES))
            self.assertEqual(replay.selection([path])["acceptance"], list(replay.GROUPS))
        workflow = (ROOT / ".github/workflows/witness.yml").read_text()
        for suite in suites:
            self.assertIn(f"contains(matrix.suites, '{suite}')", workflow)

    def test_binding_dependencies_and_dedicated_pipeline_job(self):
        suite = "decorator-binding-pipeline"
        for path in witness.binding_inputs(suite):
            self.assertTrue((ROOT / path).is_file(), path)
            expected = ["decorator-binding", suite] if path == "scripts/observe-decorator-bindings.mjs" else [suite]
            plan = replay.selection([path])
            self.assertEqual(plan["acceptance"], [])
            self.assertEqual(plan["witnesses"], expected)
        plan = replay.selection(["scripts/observe-decorator-bindings.mjs"])
        self.assertEqual(replay.matrices(plan)["witnesses"]["include"], [
            {"group": suite, "suites": [suite]},
            {"group": "printer", "suites": ["decorator-binding"]},
        ])
        workflow = (ROOT / ".github/workflows/witness.yml").read_text()
        for selected in plan["witnesses"]:
            self.assertIn(f"contains(matrix.suites, '{selected}')", workflow)

    def test_binding_runner_requires_requested_membership_and_propagates_failure(self):
        suite = "decorator-binding-pipeline"
        command, env = witness.invocation(suite, [], {})
        # The frozen known-native fixture holds no rows after
        # H2.8a-A-RES-POST-T1 retired the four R9 / R12 rows (T1 had retired
        # its five rows before).
        good = "decorator binding SUMMARY exact=767 known=0 failed=0 selected=767\n" \
               "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"
        def invoke(output=good, status=0, observer_failure=False, needles=()):
            def run(argv, **kwargs):
                if argv[0] == "node":
                    if observer_failure:
                        raise subprocess.CalledProcessError(1, argv)
                    return subprocess.CompletedProcess(argv, 0, "")
                return subprocess.CompletedProcess(argv, status, output)
            with patch.object(witness.subprocess, "run", side_effect=run) as calls, redirect_stdout(io.StringIO()):
                witness.run_binding(suite, command, env, needles)
            return calls
        calls = invoke()
        self.assertEqual(calls.call_args_list[0].args[0],
                         ["node", "scripts/observe-decorator-bindings.mjs", "pipeline", "--check"])
        self.assertEqual(calls.call_args_list[0].kwargs["env"], env)
        self.assertEqual(calls.call_args_list[1].args[0], command)
        for output in ("", good.replace("1 passed", "0 passed"), good.replace("0 ignored", "1 ignored"),
                       good.replace("failed=0", "failed=1"), good.replace("exact=767", "exact=766"),
                       good.replace("exact=767 known=0", "exact=766 known=1"),
                       good.replace("exact=767", "exact=1").replace("selected=767", "selected=10")):
            with self.subTest(output=output), self.assertRaises(ValueError):
                invoke(output)
        with self.assertRaises(subprocess.CalledProcessError):
            invoke(status=101)
        with self.assertRaises(subprocess.CalledProcessError):
            invoke(observer_failure=True)
        focused = good.replace("exact=767 known=0", "exact=28 known=0").replace("selected=767", "selected=28")
        invoke(focused, needles=("/reserved/esnext/set/",))
        with self.assertRaises(ValueError), patch.object(witness.subprocess, "run") as calls:
            witness.run_binding(suite, command, env, [witness.BINDING[suite]["upstream_exceptions"][0]])
        calls.assert_not_called()

    def test_binding_selection_and_capture_environment_cannot_narrow_other_suites(self):
        dirty = {key: "stale" for key in ("TSC_RS_H2_8A_CAPTURE_WRITES_DIR",
                 "TSC_RS_DECORATOR_BINDING_REPORT_DIR", "TSC_RS_H2_8A_KNOWN_NATIVE_DUMP_DIR",
                 "TSC_RS_DECORATOR_BINDING_CASE_SET")}
        for suite in witness.SUITES:
            _, env = witness.invocation(suite, [], dirty)
            for key in dirty:
                self.assertEqual(env.get(key), "all" if suite in witness.BINDING and key.endswith("CASE_SET") else None)

    def test_declaration_map_dependencies_keep_shared_and_adjacent_owners(self):
        for path in ("crates/compiler/tests/fixtures/declaration-map-apis.json",
                     "scripts/observe-declaration-map-apis.mjs"):
            plan = replay.selection([path])
            self.assertEqual(plan["witnesses"], ["declaration-map-apis", "declaration-maps"])
            self.assertEqual(plan["acceptance"], [])
            self.assertEqual(replay.matrices(plan)["witnesses"], {
                "include": [{"group": "module-output", "suites": list(replay.DECLARATION_MAP_SUITES)}],
            })
        self.assertTrue(set(replay.DECLARATION_MAP_SUITES).isdisjoint(replay.WITNESS_GROUPS["controls"]))
        for path in ("crates/compiler/tests/fixtures/declaration-reference-paths.json",
                     "ratchets/h2-7de-candidate-inputs.v1.json",
                     "ratchets/h2-7de-observations.v1.json",
                     "crates/compiler/tests/integration/h2_7c_forced_declarations.rs",
                     "crates/oracle/vfs-directory-overlay.mjs"):
            self.assertEqual(replay.selection([path])["witnesses"], list(witness.SUITES))
            self.assertEqual(replay.selection([path])["acceptance"], list(replay.GROUPS))
        workflow = (ROOT / ".github/workflows/witness.yml").read_text()
        for suite in ("declaration-map-apis", "declaration-maps"):
            self.assertIn(f"contains(matrix.suites, '{suite}')", workflow)

    def test_declaration_map_runner_checks_every_observer_mode_and_both_targets(self):
        suites = ["declaration-map-apis", "declaration-maps"]
        expected_observers = [
            ["node", "scripts/observe-declaration-map-apis.mjs", "--check"],
            ["node", "scripts/observe-declaration-reference-paths.mjs", "--check"],
            ["node", "scripts/observe-declaration-maps.mjs", "--check"],
            ["node", "scripts/observe-declaration-maps.mjs", "--runtime", "--check"],
            ["node", "scripts/observe-declaration-maps.mjs", "--disabled-declaration", "--check"],
            ["node", "scripts/observe-declaration-maps.mjs", "--bundle-boundary", "--check"],
        ]
        good = "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n" \
               "test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"
        for output, status in ((good, 0), (good.splitlines()[0], 0),
                               (good.replace("8 passed", "0 passed"), 0),
                               (good.replace("0 ignored", "1 ignored"), 0),
                               (good.replace("0 filtered", "1 filtered"), 0), (good, 101)):
            with self.subTest(output=output, status=status), patch.object(
                witness.subprocess, "run", return_value=subprocess.CompletedProcess([], status, output)
            ) as run, redirect_stdout(io.StringIO()):
                if output == good and status == 0:
                    witness.run_compiler_direct(suites)
                else:
                    with self.assertRaises((ValueError, subprocess.CalledProcessError)):
                        witness.run_compiler_direct(suites)
                self.assertEqual([call.args[0] for call in run.call_args_list[:-1]], expected_observers)
                self.assertEqual(run.call_args_list[-1].args[0], witness.compiler_direct_command(suites))
        for observer in expected_observers:
            def fail_selected(command, **kwargs):
                if command == observer:
                    raise subprocess.CalledProcessError(1, command)
                return subprocess.CompletedProcess(command, 0, "")
            with self.subTest(observer=observer), patch.object(
                witness.subprocess, "run", side_effect=fail_selected
            ) as run, redirect_stdout(io.StringIO()):
                with self.assertRaises(subprocess.CalledProcessError):
                    witness.run_compiler_direct(suites)
                self.assertTrue(all(call.args[0][0] == "node" for call in run.call_args_list))

    def test_bundle_dependencies_select_every_consumer_and_keep_shared_coverage(self):
        for path in ("crates/emitter/tests/fixtures/bundle-declarations.json",
                     "scripts/observe-bundle-declarations.mjs"):
            plan = replay.selection([path])
            self.assertEqual(plan["witnesses"], ["bundle-program", "bundle-declarations"])
            self.assertEqual(plan["acceptance"], [])
        for path in ("crates/emitter/tests/fixtures/bundle-maps.json", "scripts/observe-bundle-maps.mjs"):
            self.assertEqual(replay.selection([path])["witnesses"], ["bundle-declarations"])
        for path in ("crates/emitter/tests/fixtures/bundle-plan.json",
                     "crates/emitter/tests/fixtures/bundle-module-identities.json",
                     "ratchets/h2-7de-candidate-inputs.v1.json", "ratchets/h2-7de-observations.v1.json",
                     "crates/oracle/vfs-directory-overlay.mjs", "vendor/typescript-6.0.3/lib/typescript.js"):
            self.assertEqual(replay.selection([path])["witnesses"], list(witness.SUITES))
        workflow = (ROOT / ".github/workflows/witness.yml").read_text()
        for suite in ("bundle-program", "bundle-declarations"):
            self.assertIn(f"contains(matrix.suites, '{suite}')", workflow)

    def test_recovery_census_archive_matches_frozen_selection(self):
        spec = witness.COMPILER_DIRECT["utf16-recovery-corpus"]
        source, destination, digest = spec["staged_inputs"][0]
        fixture = json.loads((ROOT / spec["fixtures"][0][0]).read_text())
        archive = (ROOT / source).read_bytes()
        self.assertEqual(witness.hashlib.sha256(archive).hexdigest(), digest)
        self.assertEqual(fixture["census"]["sha256"], digest)
        self.assertEqual(fixture["census"]["path"], destination)
        self.assertEqual([row["case_id"] for row in json.loads(archive)["newly_admitted"]],
                         [row["case_id"] for row in fixture["cases"]])
        self.assertEqual(fixture["skipped"], [])
        self.assertIn(destination, witness.compiler_direct_observers(["utf16-recovery-corpus"])[0])

    def test_staged_input_preserves_existing_files_and_cleans_up_after_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            data = b"frozen input"
            source = root / "archive.json"
            source.write_bytes(data)
            target = root / "target/staged.json"
            spec = {"staged_inputs": ((source.name, "target/staged.json", witness.hashlib.sha256(data).hexdigest()),)}
            with patch.object(witness, "ROOT", root), patch.dict(witness.COMPILER_DIRECT, {"stage-test": spec}):
                with self.assertRaisesRegex(RuntimeError, "observer failed"):
                    with witness.staged_compiler_inputs(["stage-test"]):
                        self.assertEqual(target.read_bytes(), data)
                        raise RuntimeError("observer failed")
                self.assertFalse(target.exists())
                target.write_bytes(data)
                with witness.staged_compiler_inputs(["stage-test"]):
                    self.assertEqual(target.read_bytes(), data)
                self.assertEqual(target.read_bytes(), data)
                target.write_bytes(b"user data")
                with self.assertRaisesRegex(ValueError, "existing staged input differs"):
                    with witness.staged_compiler_inputs(["stage-test"]):
                        self.fail("mismatched existing input was accepted")
                self.assertEqual(target.read_bytes(), b"user data")
                target.unlink()
                source.write_bytes(b"drift")
                with self.assertRaisesRegex(ValueError, "archived input hash drift"):
                    with witness.staged_compiler_inputs(["stage-test"]):
                        self.fail("drifted archive was accepted")
                self.assertFalse(target.exists())

    def test_recovery_observer_failure_stops_before_cargo_and_removes_staged_census(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            spec = witness.COMPILER_DIRECT["utf16-recovery-corpus"]
            source, destination, _ = spec["staged_inputs"][0]
            (root / source).parent.mkdir(parents=True)
            (root / source).write_bytes((ROOT / source).read_bytes())
            fixture = spec["fixtures"][0][0]
            (root / fixture).write_bytes((ROOT / fixture).read_bytes())
            with patch.object(witness, "ROOT", root), patch.object(
                    witness.subprocess, "run", side_effect=subprocess.CalledProcessError(9, "observer")) as run:
                with self.assertRaises(subprocess.CalledProcessError):
                    witness.run_compiler_direct(["utf16-recovery-corpus"])
                self.assertEqual(run.call_count, 1)
                self.assertEqual(run.call_args.args[0][0], "node")
                self.assertFalse((root / destination).exists())

    def test_map_projection_runs_exact_tests_and_keeps_shared_artifacts_broad(self):
        spec = witness.COMPILER_DIRECT["map-option-projection"]
        source = (ROOT / f"crates/compiler/tests/{spec['target']}.rs").read_text()
        self.assertEqual(set(re.findall(r"#\[test\]\s*fn (\w+)", source)), set(spec["test"]))
        command, env = witness.invocation("map-option-projection", [], {"TSRS_MAP_OPTION_CAPTURE": "/bad"})
        self.assertIn("--exact", command)
        self.assertTrue(all(name in command for name in spec["test"]))
        self.assertNotIn("existing_witness_route_census", command)
        self.assertNotIn("TSRS_MAP_OPTION_CAPTURE", env)
        for path in ("ratchets/h2-6a-qualification.v1.json", "ratchets/h2-5h-qualification.v1.json",
                     "crates/harness/src/upstream_suites/execution.rs", "crates/oracle/vfs-directory-overlay.mjs"):
            self.assertEqual(replay.selection([path])["acceptance"], list(replay.GROUPS))
            self.assertEqual(replay.selection([path])["witnesses"], list(witness.SUITES))
        self.assertEqual(len(witness.case_ids("map-option-projection")), 31)
        good = "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out;"
        with patch.object(witness.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, good)) as run:
            witness.run_compiler_direct(["map-option-projection"])
            self.assertEqual(run.call_count, 2)
            self.assertEqual(run.call_args.args[0], command)
        for output in ("", "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out;",
                       good.replace("2 filtered", "1 filtered"),
                       "test result: ok. 3 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out;"):
            with patch.object(witness.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, output)):
                with self.assertRaises(ValueError):
                    witness.run_compiler_direct(["map-option-projection"])

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
                if path in ("scripts/observe-literal-update.mjs", "scripts/observe-decorator-bindings.mjs"):
                    continue  # covered by the explicit cross-crate ownership contract below
                with self.subTest(suite=suite, path=path):
                    self.assertTrue((ROOT / path).is_file(), path)
                    plan = replay.selection([path])
                    self.assertEqual(plan["acceptance"], [])
                    self.assertEqual(plan["witnesses"], [suite])
                    self.assertEqual(replay.matrices(plan)["witnesses"], {
                        "include": [{"group": "printer", "suites": [suite]}],
                    })

    def test_literal_update_shared_observer_selects_both_crates(self):
        plan = replay.selection(["scripts/observe-literal-update.mjs"])
        self.assertEqual(plan["acceptance"], [])
        self.assertEqual(plan["witnesses"], ["literal-update", "literal-update-pipeline"])
        self.assertEqual(replay.matrices(plan)["witnesses"], {"include": [
            {"group": "controls", "suites": ["literal-update-pipeline"]},
            {"group": "printer", "suites": ["literal-update"]},
        ]})
        workflow = (ROOT / ".github/workflows/witness.yml").read_text()
        for suite in plan["witnesses"]:
            self.assertIn(f"contains(matrix.suites, '{suite}')", workflow)
        for path in ("crates/emitter/src/factory.rs", "crates/emitter/src/builtins/tagged_template.rs",
                     "crates/emitter/src/builtins/relative_imports.rs"):
            self.assertEqual(replay.selection([path])["witnesses"], list(witness.SUITES))

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

    def test_compiler_direct_inputs_select_only_their_target_in_owning_job(self):
        for suite in witness.COMPILER_DIRECT:
            for path in witness.compiler_direct_inputs(suite):
                if path in ("scripts/observe-literal-update.mjs", "scripts/observe-decorator-bindings.mjs"):
                    continue  # covered by the explicit cross-crate ownership contract below
                if path in ("scripts/observe-bundle-declarations.mjs",
                            "crates/emitter/tests/fixtures/bundle-declarations.json",
                            "crates/emitter/tests/fixtures/bundle-module-identities.json",
                            "crates/compiler/tests/h2_7d_declaration_bundles.rs"):
                    continue  # shared bundle consumers have an explicit contract above
                if path in ("scripts/observe-declaration-map-apis.mjs",
                            "crates/compiler/tests/fixtures/declaration-map-apis.json",
                            "crates/compiler/tests/fixtures/declaration-reference-paths.json"):
                    continue  # declaration map cross-target/acceptance ownership above
                with self.subTest(suite=suite, path=path):
                    self.assertTrue((ROOT / path).is_file(), path)
                    plan = replay.selection([path])
                    self.assertEqual(plan["acceptance"], [])
                    self.assertEqual(plan["witnesses"], [suite])
                    group = ("module-output" if suite in replay.MODULE_OUTPUT_SUITES
                             else "decorator-binding-pipeline" if suite in replay.POST_T1_SUITES
                             else "controls")
                    self.assertEqual(replay.matrices(plan)["witnesses"], {
                        "include": [{"group": group, "suites": [suite]}],
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

    def test_config_library_exact_selection_preserves_shared_contract_coverage(self):
        for path in ("crates/compiler/tests/contracts.rs",
                     "crates/compiler/tests/integration/h2_7b_w4a_controls.rs"):
            self.assertEqual(replay.selection([path])["acceptance"], list(replay.GROUPS))
            self.assertEqual(replay.selection([path])["witnesses"], list(witness.SUITES))
        spec = witness.COMPILER_DIRECT["config-library"]
        names = set()
        for source in spec["sources"]:
            names.update(f"{Path(source).stem}::{name}" for name in re.findall(
                r"#\[test\]\s*fn (\w+)", (ROOT / source).read_text()))
        self.assertEqual(set(spec["test"]), names)
        self.assertEqual(len(names), 24)
        self.assertEqual(len(witness.compiler_direct_observers(["config-library"])), 12)
        for path in ("crates/compiler/tests/integration/h2_7c_declaration_blocking.rs",
                     "crates/compiler/tests/support/witness_libraries.rs"):
            plan = replay.selection([path])
            self.assertEqual(plan["acceptance"], ["late"])
            self.assertIn("config-library", plan["witnesses"])

    def test_config_library_runner_rejects_incomplete_contract_selection(self):
        spec = witness.COMPILER_DIRECT["config-library"]
        good = (f"test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; "
                f"{spec['filtered_tests']} filtered out;\n")
        for output in (good, "", good.replace("24 passed", "23 passed"),
                       good.replace("0 ignored", "1 ignored"),
                       good.replace(f"{spec['filtered_tests']} filtered", "9999 filtered")):
            with patch.object(witness.subprocess, "run",
                              return_value=subprocess.CompletedProcess([], 0, output)) as run:
                if output != good:
                    with self.assertRaises(ValueError):
                        witness.run_compiler_direct(["config-library"])
                    continue
                witness.run_compiler_direct(["config-library"])
                self.assertEqual(len(run.call_args_list), 13)
                command = run.call_args_list[-1].args[0]
                self.assertIn("--exact", command)
                self.assertTrue(all(name in command for name in spec["test"]))
                self.assertEqual(command[command.index("--test") + 1], "contracts")

    def test_transpile_oracle_runtime_matches_frozen_receipts(self):
        version = (ROOT / ".node-version").read_text().strip()
        for name in ("expected", "review-expected"):
            fixture = json.loads((ROOT / f"crates/compiler/tests/fixtures/h2_8c_transpile/{name}.v1.json").read_text())
            self.assertEqual(fixture["node"], "v" + version)
        workflow = (ROOT / ".github/workflows/witness.yml").read_text()
        self.assertIn("contains(matrix.suites, 'transpile-routes')", workflow)
        self.assertIn("actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020", workflow)
        self.assertIn("node-version-file: .node-version", workflow)

    def test_resolution_cache_owns_only_dedicated_inputs(self):
        for path in witness.RESOLUTION_INPUTS:
            self.assertTrue((ROOT / path).is_file(), path)
            plan = replay.selection([path])
            self.assertEqual(plan["acceptance"], [])
            self.assertEqual(plan["witnesses"], ["resolution-cache"])
            self.assertEqual(replay.matrices(plan)["witnesses"], {
                "include": [{"group": "controls", "suites": ["resolution-cache"]}],
            })
        for path in ("crates/program/src/resolution_cache.rs", "crates/program/src/loader.rs"):
            self.assertEqual(replay.selection([path])["witnesses"], list(witness.SUITES))
        self.assertEqual(len(witness.case_ids("resolution-cache")), 26)
        with self.assertRaises(ValueError):
            witness.invocation("resolution-cache", ["module/"])
        workflow = (ROOT / ".github/workflows/witness.yml").read_text()
        self.assertIn("contains(matrix.suites, 'resolution-cache')", workflow)

    def test_resolution_cache_runner_checks_both_targets_and_observer(self):
        command, env = witness.invocation("resolution-cache", [], {})
        self.assertIn("--lib", command)
        self.assertIn("resolution_cache_contract", command)
        good = "\n".join(f"test result: ok. {n} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;" for n in (56, 11))
        for output in (good, good.split("\n")[0], good.replace("11 passed", "0 passed"),
                       good.replace("0 ignored", "1 ignored", 1), good.replace("0 filtered", "1 filtered", 1)):
            with patch.object(witness.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, output)) as run:
                if output == good:
                    witness.run_resolution_cache(command, env)
                    self.assertEqual(run.call_args_list[0].args[0], witness.RESOLUTION_OBSERVER)
                else:
                    with self.assertRaises(ValueError):
                        witness.run_resolution_cache(command, env)
        with patch.object(witness.subprocess, "run", side_effect=subprocess.CalledProcessError(1, "observer")) as run:
            with self.assertRaises(subprocess.CalledProcessError):
                witness.run_resolution_cache(command, env)
            self.assertEqual(run.call_count, 1)

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
        self.assertEqual(plan["witnesses"], ["retained", "config-library", "require-rewrite", "declaration-comments", "utf16-literal-witnesses"])

    def test_library_snapshot_keeps_all_fresh_program_consumers(self):
        plan = replay.selection(["crates/compiler/tests/support/witness_libraries.rs"])
        self.assertEqual(plan["acceptance"], ["late"])
        self.assertEqual(plan["witnesses"], [*witness.SUPER, "retained", "config-library", "require-rewrite", "declaration-comments", "utf16-literal-witnesses", *witness.BINDING])

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
    def test_module_facet_runner_rejects_partial_results_and_oracle_failures(self):
        suites = ["module-identities", "bundle-original-javascript"]
        observers = [["node", f"scripts/{name}.mjs", "--check"] for name in (
            "observe-bundle-module-identities", "observe-system-generated-names",
            "observe-module-alias-underscores", "observe-bundle-original-javascript")]

        def invoke(bad_target=None, bad_output=None, status=0, failing_observer=None):
            def result(command, **kwargs):
                if command[0] == "node":
                    if command == failing_observer:
                        raise subprocess.CalledProcessError(1, command)
                    return subprocess.CompletedProcess(command, 0)
                target = command[command.index("--test") + 1]
                passed, filtered = (3, 0) if target == "h2_7d_module_identities" else (1, 3)
                output = f"test result: ok. {passed} passed; 0 failed; 0 ignored; 0 measured; {filtered} filtered out;\n"
                if target == bad_target:
                    output = bad_output
                return subprocess.CompletedProcess(command, status, output)
            with patch.object(witness.subprocess, "run", side_effect=result) as calls, redirect_stdout(io.StringIO()):
                witness.run_compiler_direct(suites)
                return calls.call_args_list

        calls = invoke()
        self.assertEqual([call.args[0] for call in calls[:4]], observers)
        self.assertEqual([call.args[0] for call in calls[4:]],
                         [witness.compiler_direct_command([suite]) for suite in suites])
        for target, passed, filtered in (("h2_7d_module_identities", 3, 0), ("h2_7d_declaration_bundles", 1, 3)):
            good = f"test result: ok. {passed} passed; 0 failed; 0 ignored; 0 measured; {filtered} filtered out;\n"
            for output in ("", good * 2, good.replace(f"{passed} passed", "0 passed"),
                           good.replace("0 ignored", "1 ignored"),
                           good.replace(f"{filtered} filtered", f"{filtered + 1} filtered")):
                with self.subTest(target=target, output=output), self.assertRaises(ValueError):
                    invoke(target, output)
        with self.assertRaises(subprocess.CalledProcessError):
            invoke(status=101)
        for observer in observers:
            with self.subTest(observer=observer), self.assertRaises(subprocess.CalledProcessError):
                invoke(failing_observer=observer)

    def test_original_bundle_manifest_and_observer_preserve_pinned_predecessor(self):
        suite = "bundle-original-javascript"
        manifest_path = witness.COMPILER_DIRECT[suite]["fixtures"][0][0]
        manifest = json.loads((ROOT / manifest_path).read_text())
        originals = witness.read_cases(ROOT / "ratchets/h2-7de-candidate-inputs.v1.json")
        names = ("jsDeclarationsImportTypeBundled", "jsdocAccessibilityTagsDeclarations",
                 "jsdocReadonlyDeclarations", "uniqueSymbolsDeclarationsInJs")
        expected = [next(row["case_id"] for row in originals if row["case_id"].endswith(f"/{name}.ts#default"))
                    for name in names]
        self.assertEqual([row["case_id"] for row in manifest["cases"]], expected)
        source = (ROOT / "scripts/observe-bundle-original-javascript.mjs").read_text()
        original = (ROOT / "crates/oracle/h2-7de-observations.mjs").read_text()
        body = original.split("function diagnostic(d) {", 1)[1].split("\nconst cases = [];", 1)[0]
        self.assertIn("function diagnostic(d) {" + body, source)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / manifest_path
            target.parent.mkdir(parents=True)
            for rows in ([], manifest["cases"][:-1], [manifest["cases"][0]] * 4):
                target.write_text(json.dumps({**manifest, "cases": rows}))
                with patch.object(witness, "ROOT", root), patch.object(witness.subprocess, "run") as run:
                    with self.assertRaises(ValueError):
                        witness.run_compiler_direct([suite])
                    run.assert_not_called()

    def test_bundle_section_membership_preserves_modes_and_rejects_drift_before_replay(self):
        ids = witness.case_ids("bundle-declarations")
        self.assertIn("bundle-maps/helpers/shared-es5", ids)
        self.assertIn("bundle-maps/metadata_lifetime_references/helpers/shared-es5", ids)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for file, _, _ in witness.COMPILER_DIRECT["bundle-declarations"]["fixtures"]:
                target = root / file
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes((ROOT / file).read_bytes())
            file = root / "crates/emitter/tests/fixtures/bundle-maps.json"
            fixture = json.loads(file.read_text())
            original = fixture["metadata_lifetime_references"]
            for rows in ([], original[:-1], [original[0]] * 6,
                         [{"case_id": ""}] * 6, [{"case_id": None}] * 6):
                fixture["metadata_lifetime_references"] = rows
                file.write_text(json.dumps(fixture))
                with patch.object(witness, "ROOT", root), patch.object(witness.subprocess, "run") as run:
                    with self.assertRaises(ValueError):
                        witness.run_compiler_direct(["bundle-declarations"])
                    run.assert_not_called()

    def test_bundle_runner_shares_observers_and_requires_both_target_results(self):
        selected = ["bundle-program", "bundle-declarations"]
        def run_with(bad_output=None, status=0, observer_failure=False):
            def result(command, **kwargs):
                if command[0] == "node":
                    if observer_failure:
                        raise subprocess.CalledProcessError(1, command)
                    return subprocess.CompletedProcess(command, 0)
                target = command[command.index("--test") + 1]
                passed, filtered = (4, 0) if target == "h2_7d_bundle_program" else (3, 1)
                output = f"test result: ok. {passed} passed; 0 failed; 0 ignored; 0 measured; {filtered} filtered out;\n"
                if target == "h2_7d_declaration_bundles" and bad_output is not None:
                    output = bad_output
                return subprocess.CompletedProcess(command, status, output)
            with patch.object(witness.subprocess, "run", side_effect=result) as run:
                witness.run_compiler_direct(selected)
                return run.call_args_list
        calls = run_with()
        self.assertEqual(len(calls), 4)
        self.assertEqual([call.args[0] for call in calls[:2]], [
            ["node", "scripts/observe-bundle-declarations.mjs", "--check"],
            ["node", "scripts/observe-bundle-maps.mjs", "--check"],
        ])
        names = witness.COMPILER_DIRECT["bundle-declarations"]["test"]
        self.assertEqual(calls[-1].args[0], ["cargo", "test", "--manifest-path", "crates/compiler/Cargo.toml",
                                          "--test", "h2_7d_declaration_bundles", names[0], "--", "--exact",
                                          *names[1:], "--nocapture", "--test-threads=1"])
        good = "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out;\n"
        for output in ("", good * 2, good.replace("3 passed", "0 passed"),
                       good.replace("3 passed", "2 passed"), good.replace("0 ignored", "1 ignored"),
                       good.replace("1 filtered", "0 filtered")):
            with self.subTest(output=output), self.assertRaises(ValueError):
                run_with(output)
        with self.assertRaises(subprocess.CalledProcessError):
            run_with(status=101)
        with self.assertRaises(subprocess.CalledProcessError):
            run_with(observer_failure=True)

    def test_frozen_input_catalog_counts(self):
        self.assertEqual({suite: len(witness.case_ids(suite)) for suite in witness.SUITES}, {
            "syntax-entity-names": 2, "syntax-meta-property": 1, "syntax-literal-values": 1,
            "syntax-recovery": 1, "syntax-scanner-escapes": 1, "syntax-template-escapes": 5,
            "syntax-template-flags": 1, "binder-symbol-names": 2, "types-option-numbers": 3,
            "host-memory": 13 if sys.platform == "win32" else 14,
            "host-filesystem": {"linux": 9, "darwin": 8, "win32": 7}[sys.platform],
            "program-bundle-facts": 1, "program-host-platform": 1, "program-config-paths": 2,
            "program-module-paths": 3, "program-raw-source": 1,
            "primary": 672, "extra": 42, "followup": 156, "followup2": 162,
            "followup3": 48, "retained": 530, "direct": 32, "printer": 142, "bundle-sinks": 10,
            "declaration-map-cli": 8, "transpile-routes": 301, "resolution-cache": 26,
            "compact-body-comments": 240, "parameter-temporaries": 68,
            "config-library": 96, "prologue-comments": 8,
            "utf16-recovery-corpus": 50, "map-option-projection": 31,
            "bundle-program": 27, "bundle-declarations": 56, "bundle-metadata-t1": 18,
            "post-t1-residuals": 101,
            "module-identities": 56, "bundle-original-javascript": 4,
            "declaration-map-apis": 75, "declaration-maps": 84,
            "literal-update": 1396, "literal-update-pipeline": 22, "require-rewrite": 74,
            "decorator-binding": 156, "decorator-binding-pipeline": 768,
            "declaration-specifiers": 30, "declaration-comments": 41, "jsdoc-return": 58,
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

    def test_literal_update_observer_groups_are_all_checked_before_cargo(self):
        summary = "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n"
        commands = [["node", "scripts/observe-literal-update.mjs", group, "--check"]
                    for group in ("factory", "transform", "lifetime")]
        with patch.object(witness.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, summary)) as run:
            witness.run_emitter_direct(["literal-update"])
            self.assertEqual([call.args[0] for call in run.call_args_list[:-1]], commands)
            self.assertIn("literal_update_contract", run.call_args_list[-1].args[0])
        for failed_group in ("factory", "transform", "lifetime"):
            def result(command, **kwargs):
                if failed_group in command:
                    raise subprocess.CalledProcessError(1, command)
                return subprocess.CompletedProcess(command, 0)
            with patch.object(witness.subprocess, "run", side_effect=result) as run:
                with self.assertRaises(subprocess.CalledProcessError):
                    witness.run_emitter_direct(["literal-update"])
                self.assertTrue(all(call.args[0][0] == "node" for call in run.call_args_list))
        self.assertEqual(witness.direct_observers(witness.EMITTER_DIRECT, ["literal-update"] * 2),
                         [tuple(command) for command in commands])
        _, env = witness.invocation("literal-update", [], {"TSC_RS_LITERAL_UPDATE_REPORT_DIR": "/tmp/stale"})
        self.assertNotIn("TSC_RS_LITERAL_UPDATE_REPORT_DIR", env)

    def test_require_rewrite_runs_only_four_dedicated_commands_and_rejects_missing_results(self):
        summary = "test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 10 filtered out;\n"
        for output in (summary, "", summary.replace("4 passed", "3 passed"),
                       summary.replace("10 filtered", "9 filtered"), summary.replace("0 ignored", "1 ignored")):
            with patch.object(witness.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, output)) as run:
                if output != summary:
                    with self.assertRaises(ValueError):
                        witness.run_compiler_direct(["require-rewrite"])
                    continue
                witness.run_compiler_direct(["require-rewrite"])
                calls = [call.args[0] for call in run.call_args_list]
                self.assertEqual(calls[:-1], [["node", f"scripts/observe-require-rewrite{suffix}.mjs",
                                              f"require-rewrite{suffix}", "--check"]
                                             for suffix in ("", "-composition", "-substitution", "-dynamic")])
                self.assertEqual(calls[-1], ["cargo", "test", "--manifest-path", "crates/compiler/Cargo.toml",
                    "--test", "h2_8a_require_rewrite", "require_rewrite_focused_complete_commands", "--", "--exact",
                    "require_rewrite_composition_complete_commands", "require_rewrite_substitution_complete_commands",
                    "require_rewrite_dynamic_complete_commands", "--nocapture", "--test-threads=1"])

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
        self.assertEqual(plan["witnesses"], ["retained", "config-library", "require-rewrite", "declaration-comments", "utf16-literal-witnesses"])
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

    def test_compiler_suites_clear_inherited_internal_case_selectors(self):
        poisoned = {"TSC_RS_UTF16_LITERAL_WITNESS_SET": "adjacent-probes",
                    "TSC_RS_UTF16_LITERAL_WITNESS_FILTER": "string-escape",
                    "TSC_RS_H2_5H_PARAMETER_FILTER": "comments-lf/es2015",
                    "TSC_RS_H2_5H_PARAMETER_CAPTURE_DIR": "/tmp/stale-captures",
                    "TSC_RS_DECL_COMMENT_FILTER": "module-exports",
                    "TSC_RS_DECL_COMMENT_CAPTURE_DIR": "/tmp/stale-comment-captures",
                    "TSC_RS_JSDOC_RETURN_FILTER": "no-match",
                    "TSC_RS_JSDOC_RETURN_CAPTURE_DIR": "/tmp/stale-jsdoc-captures",
                    "TSC_RS_DECLARATION_SPECIFIER_CAPTURE_DIR": "/tmp/stale-specifier-captures",
                    "TSC_RS_REQUIRE_REWRITE_FILTER": "no-match",
                    "TSC_RS_H2_8A_CAPTURE_WRITES_DIR": "/tmp/stale-rewrite-captures",
                    "TSC_RS_LITERAL_UPDATE_REPORT_DIR": "/tmp/stale-update-reports",
                    "TSC_RS_BUNDLE_METADATA_T1_CASE_SET": "no-match",
                    "TSC_RS_BUNDLE_METADATA_T1_DUMP_DIR": "/tmp/stale-t1-dumps",
                    "CARGO_BUILD_JOBS": "2"}
        for suite in witness.COMPILER_DIRECT:
            _, env = witness.invocation(suite, [], poisoned)
            self.assertEqual(env, {"CARGO_BUILD_JOBS": "2"})
            self.assertNotIn("TSC_RS_UTF16_LITERAL_WITNESS_SET", env)
            self.assertNotIn("TSC_RS_UTF16_LITERAL_WITNESS_FILTER", env)
            self.assertNotIn("TSC_RS_H2_5H_PARAMETER_FILTER", env)
            self.assertNotIn("TSC_RS_H2_5H_PARAMETER_CAPTURE_DIR", env)
        self.assertEqual(poisoned["TSC_RS_UTF16_LITERAL_WITNESS_SET"], "adjacent-probes")

    def test_declaration_runner_keeps_exact_names_and_checks_all_result_counts(self):
        selected = ["declaration-specifiers", "declaration-comments", "jsdoc-return"]
        expected = {
            "h2_8a_declaration_specifiers": (2, 9),
            "h2_8a_declaration_comment_ranges": (3, 12),
            "h2_8a_jsdoc_return": (1, 1),
        }
        def run_with(bad_target=None, bad_output=None, status=0):
            def result(command, **kwargs):
                if command[0] == "node":
                    return subprocess.CompletedProcess(command, 0)
                target = command[command.index("--test") + 1]
                passed, filtered = expected[target]
                output = f"test result: ok. {passed} passed; 0 failed; 0 ignored; 0 measured; {filtered} filtered out;\n"
                if target == bad_target:
                    output = bad_output
                return subprocess.CompletedProcess(command, status, output)
            with patch.object(witness.subprocess, "run", side_effect=result) as run:
                witness.run_compiler_direct(selected)
                return run.call_args_list
        calls = run_with()
        self.assertEqual(len(calls), 7)  # four observers, three exact target invocations
        self.assertEqual([call.args[0] for call in calls[:4]], [
            ["node", "scripts/observe-h2-8a-declaration-specifiers.mjs", "declaration-specifiers", "--check"],
            ["node", "scripts/observe-h2-8a-declaration-specifiers-composition.mjs", "declaration-specifiers-composition", "--check"],
            ["node", "scripts/observe-declaration-comment-commands.mjs", "--check"],
            ["node", "scripts/observe-h2-8a-jsdoc-return.mjs", "--check"],
        ])
        for suite, call in zip(selected, calls[4:]):
            spec = witness.COMPILER_DIRECT[suite]
            names = spec["test"]
            names = (names,) if isinstance(names, str) else names
            self.assertEqual(call.args[0], ["cargo", "test", "--manifest-path", "crates/compiler/Cargo.toml",
                                          "--test", spec["target"], names[0], "--", "--exact",
                                          *names[1:], "--nocapture", "--test-threads=1"])
            self.assertEqual(len(witness.case_ids(suite)), {"declaration-specifiers": 30, "declaration-comments": 41, "jsdoc-return": 58}[suite])
            with self.assertRaises(ValueError):
                witness.invocation(suite, ["a"])
        good = "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 12 filtered out;\n"
        for output in ("", good * 2, good.replace("3 passed", "0 passed"),
                       good.replace("3 passed", "2 passed"), good.replace("0 ignored", "1 ignored"),
                       good.replace("12 filtered", "11 filtered"), good.replace("12 filtered", "13 filtered")):
            with self.subTest(output=output), self.assertRaises(ValueError):
                run_with("h2_8a_declaration_comment_ranges", output)
        with self.assertRaises(subprocess.CalledProcessError):
            run_with(status=101)
        with self.assertRaises(ValueError):
            witness.compiler_direct_command(selected)

    def test_declaration_shared_observer_and_helpers_keep_all_consumers(self):
        for path in ("scripts/observe-jsdoc-block-scope-container.mjs",
                     "crates/oracle/vfs-directory-overlay.mjs",
                     "crates/compiler/tests/integration/h2_7b_w4a_controls.rs",
                     "crates/compiler/tests/integration/h2_7d_original_corpus_shared.rs"):
            self.assertEqual(replay.selection([path])["witnesses"], list(witness.SUITES))
        plan = replay.selection(["crates/compiler/tests/fixtures/declaration-comment-ranges.json",
                                 "crates/compiler/tests/fixtures/h2-8a-jsdoc-return.json"])
        self.assertEqual(plan["acceptance"], [])
        self.assertEqual(plan["witnesses"], ["declaration-comments", "jsdoc-return"])

    def test_compiler_exact_name_lists_reject_empty_duplicate_or_count_drift(self):
        spec = witness.COMPILER_DIRECT["declaration-comments"]
        for names in ((), ("one", "one", "two"), ("one", "", "two"), ("one", "two")):
            with patch.dict(spec, {"test": names}), self.assertRaises(ValueError):
                witness.compiler_direct_command(["declaration-comments"])

    def test_coverage_inventory_retains_every_exact_name_in_a_command(self):
        spec = importlib.util.spec_from_file_location(
            "coverage_inventory", ROOT / "docs/design/greenfield/slices/witness-coverage/inventory.py")
        inventory = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(inventory)
        rows = inventory.command_rows(replay)
        for suite in ("declaration-specifiers", "declaration-comments", "jsdoc-return", "config-library"):
            names = witness.COMPILER_DIRECT[suite]["test"]
            names = (names,) if isinstance(names, str) else names
            self.assertEqual([row["filter"] for row in rows if row["suite"] == suite], list(names))

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
