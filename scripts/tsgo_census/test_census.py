"""Checks for cross-port count normalization and comparison edge cases."""

import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch


sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location(
    "tsgo_census", Path(__file__).resolve().parents[1] / "tsgo_census.py"
)
census = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(census)


class CensusTests(unittest.TestCase):
    def test_go_and_rust_names_share_keys(self):
        self.assertEqual(census.normalize({
            "CompareTypes": 3,
            "internal/checker/checker.go:Checker.getPropertyOfTypeEx": 4,
            "<tsc_checker::state::CheckerState>::get_effects_signature": 5,
            "<tsc_checker::state::CheckerState>::get_property_of_type_ex_with_include_type_only_members::<Name>": 6,
        }), {"compareTypes": 3, "getPropertyOfTypeEx": 10, "getEffectsSignature": 5})

    def test_monomorphizations_and_closures_sum(self):
        closure = "tsc_checker::State::infer_types::{{closure}}"
        # Plain-name mappings retain their legacy aggregation semantics.
        with patch.object(census, "manifest_rows", return_value=[
            ("inferTypes", "a.go:inferTypes", "infer_types", ""),
        ]):
            self.assertEqual(census.normalize({
                "tsc_checker::State::infer_types::<First>": 7,
                "tsc_checker::State::infer_types::<Second>": 11,
                closure: 100,
            }), {"inferTypes": 118})

    def test_manifest_counts_only_selected_worker_entries(self):
        state = "<tsc_checker::state::CheckerState>"
        worker = state + "::get_property_of_type_ex_with_include_type_only_members"
        symbols = {
            worker + "::<tsc_types::escaped_name::EscapedName>": 7,
            worker + "::<&alloc::string::String>": 11,
            worker + "::<Name>::{closure#0}": 101,
            worker + "::<Name>::{{closure}}": 103,
            worker + "::<State::{closure#0}>": 107,
            "<tsc_checker::links::LinksTables>::read_symbol::<Type, " + worker + "::{closure#0}>": 109,
            state + "::get_union_type": 113,
            state + "::get_union_type_ex_with_origin": 13,
            state + "::instantiate_type_with_alias": 211,
            "<tsc_types::tables::TypeTables>::get_union_type": 127,
            "<tsc_types::tables::TypeTables>::get_union_type_with_origin": 17,
            "<tsc_types::tables::TypeTables>::get_union_type_worker": 199,
            "<tsc_checker::engine::RelationChecker>::is_related_to_with_head": 17,
            "<tsc_checker::inference::InferTypesWalker>::infer_from_types": 19,
        }
        counts, matched = census.map_rust_counts(symbols)
        self.assertEqual(counts["getPropertyOfTypeEx"], 18)
        self.assertEqual(len(matched["getPropertyOfTypeEx"]), 2)
        # Pure table construction and checker construction are disjoint full
        # entries. Their forwarding wrappers and downstream workers are not.
        self.assertEqual(counts["getUnionTypeEx"], 30)
        self.assertEqual(len(matched["getUnionTypeEx"]), 2)
        self.assertEqual(counts["isRelatedToEx"], 17)
        self.assertEqual(counts["inferFromTypes"], 19)
        self.assertIn("getPropertyOfType", counts["_unmapped"])
        self.assertIn("instantiateTypeWithAlias", counts["_unmapped"])
        self.assertNotIn("instantiateTypeWithAlias", counts)

    def test_profdata_fixture_uses_first_block_not_function_count(self):
        fixture = Path(__file__).parent / "fixtures/profdata-show.txt"
        with fixture.open() as source:
            self.assertEqual(census.parse_profdata(source), {
                "_ZN7checker11infer_types17h0123456789abcdefE": 24,
                "_ZN7checker11infer_types17hfedcba9876543210E": 11,
                "zero_entry": 0,
            })
        for text in ("", "Counters:\n", "  absent:\n    Block counts: []\n"):
            with self.subTest(text=text), self.assertRaisesRegex(ValueError, "no first-block"):
                census.parse_profdata(text.splitlines())
        with self.assertRaisesRegex(ValueError, "not entry-first"):
            census.parse_profdata(["  f:", "    Block counts: [17]", "Instrumentation level: IR entry_first = 0"])

    def test_mapping_own_segment_generics_closures_regex_and_absence(self):
        rows = [
            ("inferTypes", "a.go:inferTypes", "infer_types", ""),
            ("regex", "a.go:regex", r"re:^other::special::<.*>", ""),
            ("zero", "a.go:zero", "zero", ""),
            ("missing", "a.go:missing", "not_present", ""),
            ("empty", "a.go:empty", "", ""),
        ]
        symbols = {
            "checker::State::infer_types::<First>": 7,
            "checker::State::infer_types::<other::Type<inner::Type>>::h0123456789abcdef": 11,
            "checker::State::infer_types::<First>::{{closure}}::{{closure}}::h0123456789abcdef": 13,
            "checker::State::infer_types_extra": 1000,
            "checker::State::infer_types::nested_function": 2000,
            "checker::State::<other::infer_types>::unrelated": 3000,
            "other::special::<T>": 5,
            "other::special::<U>::{{closure}}": 17,
            "checker::State::zero": 0,
        }
        with patch.object(census, "manifest_rows", return_value=rows):
            counts, matched = census.map_rust_counts(symbols)
        self.assertEqual(counts, {"inferTypes": 31, "regex": 22, "zero": 0,
                                  "_unmapped": ["missing", "empty"]})
        self.assertEqual(len(matched["inferTypes"]), 3)
        self.assertEqual(matched["zero"], [("checker::State::zero", 0)])
        self.assertEqual(matched["missing"], [])

    def test_mapping_ambiguity_is_fatal(self):
        with patch.object(census, "manifest_rows", return_value=[
            ("one", "a.go:one", "infer_types", ""),
            ("two", "a.go:two", "re:infer_types", ""),
        ]), self.assertRaisesRegex(ValueError, "ambiguous"):
            census.map_rust_counts({"checker::infer_types": 1})

    def test_wrapper_and_body_mapping_stay_separate(self):
        state = "<tsc_checker::state::CheckerState>::"
        self.assertEqual(census.normalize({
            state + "get_property_of_type_ex_with_include_type_only_members::<Name>": 9,
            state + "check_expression_worker": 12,
            state + "check_expression_with_force_tuple": 15,
        }), {"getPropertyOfTypeEx": 9, "checkExpressionWorker": 12,
             "checkExpressionEx": 15})

    def test_skip_build_requires_identical_compiler_inputs_and_binary(self):
        for changed_sources, changed_binary in ((False, False), (True, False), (False, True)):
            with self.subTest(changed_sources=changed_sources, changed_binary=changed_binary), \
                    tempfile.TemporaryDirectory() as directory:
                work = Path(directory)
                binary = work / "instrumented/release/tsc-rs"
                binary.parent.mkdir(parents=True)
                binary.write_bytes(b"instrumented compiler")
                census.write_json(work / "build.json", {
                    "binary_commit": "build-commit", "entry_first": True,
                    "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                })
                if changed_binary:
                    binary.write_bytes(b"changed compiler")
                commands = []

                def checked(command, stdout, stderr, records, **kwargs):
                    commands.append(command)
                    if command == ["git", "rev-parse", "HEAD"]:
                        stdout.write_text("tooling-commit\n")
                    if command[:5] == ["git", "diff", "--exit-code", "build-commit", "tooling-commit"] and changed_sources:
                        raise ValueError("compiler inputs changed")

                with patch.object(census, "RS_WORK", work), \
                        patch.object(census, "checked_record", side_effect=checked):
                    args = SimpleNamespace(skip_build=True)
                    if changed_sources or changed_binary:
                        with self.assertRaises(ValueError):
                            census.rs_build(args, [], [], None)
                    else:
                        actual, metadata = census.rs_build(args, [], [], None)
                        self.assertEqual(actual, binary)
                        self.assertEqual(metadata["binary_commit"], "build-commit")
                        self.assertIn(["git", "diff", "--exit-code", "build-commit", "tooling-commit", "--",
                                       "crates", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo"], commands)

    def test_explicit_regex_and_ambiguity(self):
        with patch.object(census, "manifest_rows", return_value=[
            ("one", "a.go:one", r"re:^mangled_one_[0-9]+$", ""),
        ]):
            self.assertEqual(census.normalize({"mangled_one_123": 4}), {"one": 4})
        with patch.object(census, "manifest_rows", return_value=[
            ("one", "a.go:one", "re:shared", ""),
            ("two", "b.go:two", "re:shared", ""),
        ]):
            with self.assertRaisesRegex(ValueError, "ambiguous"):
                census.normalize({"shared": 4})

    def test_ratio_direction_order_zero_and_absence(self):
        with tempfile.TemporaryDirectory() as directory:
            a, b = Path(directory) / "a.json", Path(directory) / "b.json"
            a.write_text(json.dumps({"same": 5, "up": 10, "down": 10,
                                     "zero": 0, "infinite": 0, "absent": 7}))
            b.write_text(json.dumps({"same": 5, "up": 30, "down": 5,
                                     "zero": 0, "infinite": 1}))
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                self.assertEqual(census.compare(SimpleNamespace(a=a, b=b)), 0)
            rows = [line.split() for line in output.getvalue().splitlines()[1:]]
            self.assertEqual([r[0] for r in rows],
                             ["absent", "infinite", "up", "down", "same", "zero"])
            self.assertEqual(rows[0], ["absent", "7", "-", "missing"])
            self.assertEqual(rows[1][-1], "inf")
            self.assertEqual(rows[2][-1], "3.000000")
            self.assertEqual(rows[-1][-1], "1.000000")

    def test_invalid_entry_counts_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "counts.json"
            for value in ([1], {"f": True}, {"f": -1}, {"f": 1.5}, {"f": "3"},
                          {"_meta": []}, {"_unmapped": "f"}, {"_unmapped": [1]},
                          {"_unmapped": ["f", "f"]}, {"f": 0, "_unmapped": ["f"]}):
                with self.subTest(value=value):
                    path.write_text(json.dumps(value))
                    with self.assertRaises(ValueError):
                        census.read_counts(path)

    def test_unmapped_and_metadata_excluded_from_ratios(self):
        a = {"up": 10, "absent": 7, "zero": 0}
        b = {"up": 30, "zero": 0, "_unmapped": ["absent"], "_meta": {"exit_code": 2}}
        rows, unmapped = census.comparison_rows(a, b)
        self.assertEqual(unmapped, ["absent"])
        self.assertEqual(rows, [(2.0, "up", 10, 30, "3.000000"), (0.0, "zero", 0, 0, "1.000000")])

    def test_summary_ratios_direction_order_and_unmapped(self):
        with tempfile.TemporaryDirectory() as directory:
            a, b = Path(directory) / "go", Path(directory) / "rs"
            a.mkdir()
            b.mkdir()
            for project in ("hono", "zod"):
                census.write_json(a / f"{project}.json", {"same": 5, "up": 10, "absent": 7})
                census.write_json(b / f"{project}.json", {"same": 5, "up": 30 if project == "hono" else 20,
                                                        "_unmapped": ["absent"], "_meta": {"exit_code": 2}})
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                self.assertEqual(census.compare(SimpleNamespace(a=a, b=b, summary=True,
                                                                projects=["hono", "zod"])), 0)
            lines = output.getvalue().splitlines()
            self.assertEqual(lines[0].split(), ["function", "hono", "zod"])
            self.assertEqual(lines[1].split(), ["up", "3.000000", "2.000000"])
            self.assertEqual(lines[2].split(), ["same", "1.000000", "1.000000"])
            self.assertIn("  absent: hono, zod", lines)

    def test_pgo_build_and_profile_environment_reused_offline(self):
        pgo = census.pgo_helpers()
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(pgo.sys, "platform", "darwin"), patch.dict(pgo.os.environ, {
                "LLVM_PROFILE_FILE": "train-leaked.profraw", "CARGO_ENCODED_RUSTFLAGS": "-Copt-level=0",
                "CFLAGS": "-DKEEP_NATIVE_FLAG", "CXXFLAGS": "-DKEEP_CXX_FLAG",
            }):
                commands = []

                def runner(command, **kwargs):
                    commands.append((command, kwargs["env"]))
                    return SimpleNamespace(returncode=0)

                binary = pgo.instrumented_build(directory, 1, entry_counts=True, runner=runner,
                                                offline=True, prefix=["nice", "-n", "20"])
                self.assertTrue(binary.endswith("instrumented/release/tsc-rs"))
                command, env = commands[0]
                self.assertEqual(command[:8], ["nice", "-n", "20", "cargo", "build", "--release", "-j", "1"])
                self.assertEqual(command[-2:], ["--offline", "--locked"])
                self.assertNotIn("LLVM_PROFILE_FILE", env)
                self.assertNotIn("CARGO_ENCODED_RUSTFLAGS", env)
                self.assertEqual(env["CFLAGS"], "-DKEEP_NATIVE_FLAG -fno-profile-generate -fno-profile-use")
                self.assertEqual(env["CXXFLAGS"], "-DKEEP_CXX_FLAG -fno-profile-generate -fno-profile-use")
                self.assertEqual(env["CARGO_NET_OFFLINE"], "true")
                self.assertEqual(env["RUSTFLAGS"], f"-Cprofile-generate={directory}/profiles -Cllvm-args=-pgo-instrument-entry=true")
                self.assertEqual(pgo.profile_environment(directory, "train-%p.profraw")["LLVM_PROFILE_FILE"],
                                 f"{directory}/train-%p.profraw")

    def test_run_rs_diagnostics_retained_and_missing_profile_fatal(self):
        pgo = census.pgo_helpers()
        for produce_profile in (True, False):
            with self.subTest(produce_profile=produce_profile), tempfile.TemporaryDirectory() as directory:
                root = Path(directory).resolve()
                work = root / "work"
                (work / "profiles").mkdir(parents=True)
                project = root / "hono"
                project.mkdir()
                (project / "tsconfig.bench-noemit.json").touch()
                binary = work / "instrumented/release/tsc-rs"
                commands = []

                def recorded(command, cwd, env, stdout, stderr, records, **kwargs):
                    commands.append(command)
                    result = {"command": command, "exit_code": 2 if command[0] == str(binary) else 0,
                              "elapsed_seconds": 0.1, "stdout": str(stdout), "stderr": str(stderr)}
                    if command[0] == str(binary) and produce_profile:
                        Path(env["LLVM_PROFILE_FILE"].replace("%p", "123")).write_bytes(b"profile")
                    records.append(result)
                    return result

                args = SimpleNamespace(projects=["hono"], bench_root=root, llvm_profdata=Path("llvm-profdata"), skip_build=True)
                with patch.object(census, "ROOT", root), patch.object(census, "RS_WORK", work), \
                        patch.object(census, "nice_prefix", return_value=[]), \
                        patch.object(census, "pgo_helpers", return_value=pgo), \
                        patch.object(census, "recorded", side_effect=recorded), \
                        patch.object(census, "rs_build", return_value=(binary, {"binary_commit": "commit", "binary_sha256": "sha"})), \
                        patch.object(census, "rs_profile_counts", return_value=({"f": 3, "_unmapped": []}, {"f": [["State::f", 3]]})), \
                        patch.object(census, "write_mapping"), contextlib.redirect_stdout(io.StringIO()):
                    if produce_profile:
                        self.assertEqual(census.run_rs(args), 0)
                        counts = census.read_counts(work / "counts/hono.json")
                        self.assertEqual(counts["_meta"]["exit_code"], 2)
                        self.assertEqual(counts["_meta"]["binary_commit"], "commit")
                        self.assertIn("--singleThreaded", counts["_meta"]["command"])
                    else:
                        with self.assertRaisesRegex(ValueError, "expected one nonempty training profile"):
                            census.run_rs(args)
                        self.assertFalse((work / "counts/hono.json").exists())
                self.assertTrue((work / "run.json").exists())


if __name__ == "__main__":
    unittest.main()
