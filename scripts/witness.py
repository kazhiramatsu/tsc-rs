#!/usr/bin/env python3
"""Run a focused witness selection without building xtask first."""
import argparse
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "crates/compiler/tests/fixtures"
SUPER = {
    "primary": ("", "TSC_RS_DECORATOR_SUPER_CASE_SET"),
    **{name: (f"-{name}", f"TSC_RS_DECORATOR_SUPER_{name.upper()}_CASE_SET")
       for name in ("extra", "followup", "followup2", "followup3")},
}
# Small direct targets share the hosted printer build, but retain individual
# selection. Each fixture tuple is (path, row count, ID key). These counts are
# input memberships, not a claim about complete compiler command equivalence.
EMITTER_DIRECT = {
    "compact-body-comments": {
        "target": "compact_body_comments_contract",
        "fixtures": (("crates/emitter/tests/fixtures/compact-body-comments.json", 240, "case_id"),),
        "observers": ("scripts/observe-compact-body-comments.mjs",),
    },
    "literal-parent-provenance": {
        "target": "literal_parent_provenance_contract",
        "fixtures": (("crates/emitter/tests/fixtures/literal-parent-provenance-utf16.json", 128, "case_id"),),
        "observers": ("scripts/observe-literal-parent-provenance-utf16.mjs",),
        "inputs": ("scripts/observe-literal-parent-provenance.mjs",
                   "crates/emitter/tests/fixtures/literal-parent-provenance.json"),
    },
    "literal-value-provenance": {
        "target": "literal_value_provenance_contract",
        "fixtures": (("crates/emitter/tests/fixtures/template-raw-provenance.json", 480, "case_id"),
                     ("crates/emitter/tests/fixtures/string-property-provenance.json", 60, "case_id")),
        "observers": ("scripts/observe-template-raw-provenance.mjs", "scripts/observe-string-property-provenance.mjs"),
    },
    "string-literal-identifier-source": {
        "target": "string_literal_identifier_source_contract",
        "fixtures": (("crates/emitter/tests/fixtures/string-literal-identifier-source.json", 72, "case_id"),),
        "observers": ("scripts/observe-string-literal-identifier-source.mjs",),
    },
    "utf16-literal-escaping": {
        "target": "utf16_literal_escaping_contract",
        "fixtures": (("crates/emitter/tests/fixtures/utf16-literal-escaping.json", 288, "case_id"),
                     ("crates/emitter/tests/fixtures/utf16-declaration-literal-printer.json", 8, "id")),
        "observers": ("scripts/observe-utf16-literal-escaping.mjs", "scripts/observe-utf16-declaration-literal-printer.mjs"),
    },
    "class-header-token-metadata": {
        "target": "class_header_token_metadata_contract",
        "fixtures": (("crates/emitter/tests/fixtures/class-header-token-printer-metadata.json", 32, "case_id"),),
        "observers": ("scripts/observe-class-header-token-printer-metadata.mjs",),
    },
    "comma-argument-factory": {
        "target": "comma_argument_factory_contract",
        "fixtures": (("crates/emitter/tests/fixtures/comma-argument-factory.json", 44, "case_id"),
                     ("crates/emitter/tests/fixtures/list-intervening-owners.json", 96, "case_id"),
                     ("crates/emitter/tests/fixtures/list-trailing-token-owners.json", 104, "case_id"),
                     ("crates/emitter/tests/fixtures/list-boundary-lines.json", 264, "case_id"),
                     ("ratchets/h2-8a-list-cursor-lifecycle.v1.json", 11, "case_id")),
        "observers": tuple(f"scripts/observe-{name}.mjs" for name in (
            "comma-argument-factory", "list-intervening-owners", "list-trailing-token-owners",
            "list-boundary-lines", "list-cursor-lifecycle")),
    },
    "ellipsis-comment-metadata": {
        "target": "ellipsis_comment_metadata_contract",
        "fixtures": (("crates/emitter/tests/fixtures/ellipsis-comment-printer-metadata.json", 144, "case_id"),),
        "observers": ("scripts/observe-ellipsis-comment-printer-metadata.mjs",),
    },
    "import-type-attributes": {
        "target": "import_type_attributes_contract",
        "fixtures": (("crates/emitter/tests/fixtures/import-type-attributes.json", 84, "case_id"),),
        "observers": ("scripts/observe-import-type-attributes.mjs",),
    },
    "mapped-type-members": {
        "target": "mapped_type_members_contract",
        "fixtures": (("crates/emitter/tests/fixtures/mapped-type-members.json", 328, "case_id"),),
        "observers": ("scripts/observe-mapped-type-members.mjs",),
    },
    "token-comment-phase-metadata": {
        "target": "token_comment_phase_metadata_contract",
        "fixtures": (("crates/emitter/tests/fixtures/token-comment-phase-printer-metadata.json", 96, "case_id"),),
        "observers": ("scripts/observe-token-comment-phase-printer-metadata.mjs",),
    },
}
# These compiler witnesses have dedicated inputs or additional command fields.
# Shared helper tests stay in acceptance; select the dedicated test where needed.
COMPILER_DIRECT = {
    "declaration-specifiers": {
        "target": "h2_8a_declaration_specifiers",
        "test": ("focused_declaration_specifiers_match_complete_commands",
                 "composition_declaration_specifiers_match_complete_commands"),
        "tests": 2,
        "filtered_tests": 9,
        "fixtures": tuple((f"crates/compiler/tests/fixtures/h2-8a-{group}.json", count, "case_id")
                          for group, count in (("declaration-specifiers", 24), ("declaration-specifiers-composition", 6))),
        "observers": tuple((f"scripts/observe-h2-8a-{group}.mjs", group)
                           for group in ("declaration-specifiers", "declaration-specifiers-composition")),
        "inputs": tuple(f"crates/compiler/tests/fixtures/h2-8a-{group}-inputs.json"
                        for group in ("declaration-specifiers", "declaration-specifiers-composition")),
    },
    "declaration-comments": {
        "target": "h2_8a_declaration_comment_ranges",
        "test": ("declaration_comment_range_focused_complete_commands",
                 "declaration_comment_detached_prefix_complete_commands",
                 "declaration_comment_parameter_tags_complete_commands"),
        "tests": 3,
        "filtered_tests": 12,
        "fixtures": tuple((f"crates/compiler/tests/fixtures/declaration-comment-{group}.json", count, "case_id")
                          for group, count in (("ranges", 17), ("detached-prefixes", 12), ("parameter-tags", 12))),
        "observers": ("scripts/observe-declaration-comment-commands.mjs",),
        # The reused observer serves other owners too; its path keeps the
        # planner's conservative shared-input rule, rather than owning it here.
    },
    "jsdoc-return": {
        "target": "h2_8a_jsdoc_return",
        "test": "jsdoc_return_controls_match_complete_commands_twice",
        "tests": 1,
        "filtered_tests": 1,
        "fixtures": (("crates/compiler/tests/fixtures/h2-8a-jsdoc-return.json", 58, "id"),),
        "observers": ("scripts/observe-h2-8a-jsdoc-return.mjs",),
    },
    "parameter-temporaries": {
        "target": "h2_5h_parameter_temporaries",
        "tests": 2,
        "fixtures": (("crates/compiler/tests/fixtures/h2-5h-parameter-temporaries.json", 68, "case_id"),),
        "observers": ("scripts/observe-h2-5h-parameter-temporaries.mjs",),
        "inputs": ("docs/design/greenfield/slices/h2-5h-parameter-temporaries-selection.v1.json",),
    },
    "transpile-routes": {
        "target": "transpile_routes_contract",
        "tests": 9,
        "fixtures": (("crates/compiler/tests/fixtures/h2_8c_transpile/inputs.v1.json", 287, "id"),
                     ("crates/compiler/tests/fixtures/h2_8c_transpile/review-inputs.v1.json", 14, "id")),
        "observers": ("scripts/observe-transpile-routes.mjs",),
        "inputs": tuple(f"crates/compiler/tests/fixtures/h2_8c_transpile/{name}.v1.json"
                        for name in ("expected", "known-open", "known-native", "review-expected")),
    },
    "utf16-identity-recovery": {
        "target": "h2_8a_utf16_identity_recovery_controls",
        "tests": 2,
        "fixtures": (("crates/compiler/tests/fixtures/utf16-identity-recovery-controls.json", 65, "id"),
                     ("crates/compiler/tests/fixtures/utf16-noemit-command-controls.json", 14, "id")),
        "observers": ("scripts/observe-utf16-identity-recovery-controls.mjs",
                      "scripts/observe-utf16-noemit-command-controls.mjs"),
    },
    "utf16-review-fix": {
        "target": "h2_8a_utf16_review_fix_controls",
        "tests": 1,
        "fixtures": (("crates/compiler/tests/fixtures/utf16-review-fix-controls.json", 25, "id"),),
        "observers": ("scripts/observe-utf16-review-fix-controls.mjs",),
    },
    "utf16-tagged-template": {
        "target": "h2_8a_utf16_tagged_template_controls",
        "tests": 1,
        "fixtures": (("crates/compiler/tests/fixtures/utf16-tagged-template-controls.json", 16, "id"),),
        "observers": ("scripts/observe-utf16-tagged-template-controls.mjs",),
        "inputs": ("crates/compiler/tests/fixtures/utf16-tagged-template-review-v1.json",
                   "crates/compiler/tests/fixtures/utf16-tagged-template-review-v2.json"),
    },
    "utf16-literal-witnesses": {
        "target": "h2_5h_utf16_literal_witnesses",
        "test": "utf16_literal_witnesses_match_complete_typescript_observations",
        "tests": 1,
        "filtered_tests": 9,
        "fixtures": tuple((f"crates/compiler/tests/fixtures/utf16-literals-{group}.json", count, "case_id")
                          for group, count in (("string-literals", 36), ("template-literals", 26), ("bundle-prologues", 2))),
        "observers": tuple(("scripts/observe-utf16-literal-witnesses.mjs", group)
                           for group in ("string-literals", "template-literals", "bundle-prologues")),
        "inputs": tuple(f"crates/compiler/tests/fixtures/utf16-literals-{group}-inputs.json"
                        for group in ("string-literals", "template-literals", "bundle-prologues")),
    },
    "utf16-original-commands": {
        "target": "h2_5h_utf16_original_rows_complete",
        "tests": 1,
        "fixtures": (("crates/compiler/tests/fixtures/utf16-original-rows-complete.json", 4, "case_id"),),
        "observers": ("scripts/observe-utf16-original-rows-complete.mjs",),
    },
}
SUITES = (*SUPER, "retained", "direct", "printer", "bundle-sinks", "declaration-map-cli",
          *EMITTER_DIRECT, *COMPILER_DIRECT, "resolution-cache")
RESOLUTION_INPUTS = {
    "crates/program/tests/resolution_cache_contract.rs",
    "crates/program/tests/fixtures/resolution_cache/manifest.v1.json",
    "crates/program/tests/fixtures/resolution_cache/expected.v1.json",
    "scripts/observe-resolution-cache.mjs",
}
RESOLUTION_OBSERVER = [
    "node", "scripts/observe-resolution-cache.mjs", "--manifest",
    "crates/program/tests/fixtures/resolution_cache/manifest.v1.json", "--check",
    "crates/program/tests/fixtures/resolution_cache/expected.v1.json",
]
RETAINED_FIXTURES = (
    "retained-accessor-owners", "class-helper-accessor-producers",
    "class-field-alias-map-positions", "decorator-receiver-context",
    "retained-lexical-environments", "retained-constructor-references",
    "retained-lexical-edges", "retained-comma-factory",
)


def read_cases(file):
    return json.loads(file.read_text())["cases"]


def case_ids(suite):
    if suite == "resolution-cache":
        manifest = json.loads((ROOT / "crates/program/tests/fixtures/resolution_cache/manifest.v1.json").read_text())
        families = manifest["families"]
        ids = [family["id"] for family in families]
        generations = [generation for family in families for generation in family["generations"]]
        if (len(ids) != 26 or len(set(ids)) != 26 or any(not item.strip() for item in ids)
                or len(generations) + len(families) != 112
                or sum(len(family["requests"]) * (1 + len(family["generations"])) for family in families) != 197):
            raise ValueError("resolution-cache: changed family/generation/request membership")
        return ids
    if suite in EMITTER_DIRECT or suite in COMPILER_DIRECT:
        spec = EMITTER_DIRECT[suite] if suite in EMITTER_DIRECT else COMPILER_DIRECT[suite]
        ids = []
        for file, expected, key in spec["fixtures"]:
            rows = read_cases(ROOT / file)
            local_ids = [row[key] for row in rows]
            if (len(rows) != expected or not rows
                    or any(not isinstance(item, str) or not item.strip() for item in local_ids)
                    or len(set(local_ids)) != len(local_ids)):
                raise ValueError(f"{suite}: empty, duplicate or changed fixture membership: {file}")
            ids.extend(f"{Path(file).stem}/{item}" for item in local_ids)
        if len(set(ids)) != len(ids):
            raise ValueError(f"{suite}: duplicate fixture membership")
        return ids
    if suite == "declaration-map-cli":
        cases = [row for row in read_cases(ROOT / "ratchets/h2-7de-observations.v1.json")
                 if row["required_slices"] == ["H2.7e"]]
        if len(cases) != 8:
            raise ValueError("declaration-map-cli: changed eight-case membership")
    elif suite in SUPER:
        suffix, _ = SUPER[suite]
        # Input IDs include the two primary upstream exceptions. The Rust
        # comparator reports those separately and requires a native match.
        cases = read_cases(FIXTURES / f"decorator-super{suffix}-inputs.json")
    elif suite == "retained":
        cases = []
        for name in RETAINED_FIXTURES:
            rows = read_cases(FIXTURES / f"{name}.json")
            if name == "class-field-alias-map-positions":
                rows = [row for row in rows if row["options"]["target"] == 9]
            cases.extend(rows)
    elif suite == "bundle-sinks":
        cases = read_cases(FIXTURES / "bundle-sinks.json")
    elif suite == "printer":
        cases = []
        for name in ("printer-failure-hooks", "printer-failure-review", "printer-comment-carry",
                     "printer-hook-hints"):
            cases.extend(read_cases(ROOT / f"crates/emitter/tests/fixtures/{name}.json"))
    else:
        cases = read_cases(ROOT / "crates/emitter/tests/fixtures/decorator-super-direct.json")
    ids = [case["case_id"] for case in cases]
    if not ids or len(set(ids)) != len(ids):
        raise ValueError(f"{suite}: empty or duplicate case IDs")
    return ids


def select_cases(ids, needles):
    if any(not needle.strip() or "," in needle or needle.strip() == "all" for needle in needles):
        raise ValueError("--case requires a nonempty substring (no comma or reserved 'all')")
    needles = [needle.strip() for needle in needles]
    selected = [case for case in ids if not needles or any(needle in case for needle in needles)]
    if not selected:
        raise ValueError("selection matched no cases")
    return selected


def invocation(suite, needles, environ=None):
    env = dict(os.environ if environ is None else environ)
    # Explicit CLI selection owns the entire selection, including --all.
    for _, key in SUPER.values():
        env.pop(key, None)
    env.pop("TSC_RS_RETAINED_ACCESSOR_CASE_FILTER", None)
    env.pop("TSC_RS_RETAINED_ACCESSOR_CASE_SET", None)
    env.setdefault("CARGO_BUILD_JOBS", "2")
    if suite == "resolution-cache":
        if needles:
            raise ValueError("resolution-cache: complete trace and controls run together; use --all")
        return ["cargo", "test", "--manifest-path", "crates/program/Cargo.toml",
                "--lib", "--test", "resolution_cache_contract", "--", "--nocapture", "--test-threads=1"], env
    if suite in COMPILER_DIRECT:
        if needles:
            raise ValueError(f"{suite}: compiler target runs together; use --all")
        # Internal selectors can narrow cases without changing Cargo's test
        # count. Registered suites always own all their frozen inputs.
        env.pop("TSC_RS_UTF16_LITERAL_WITNESS_SET", None)
        env.pop("TSC_RS_UTF16_LITERAL_WITNESS_FILTER", None)
        env.pop("TSC_RS_H2_5H_PARAMETER_FILTER", None)
        env.pop("TSC_RS_H2_5H_PARAMETER_CAPTURE_DIR", None)
        for key in ("TSC_RS_DECL_COMMENT_FILTER", "TSC_RS_DECL_COMMENT_CAPTURE_DIR",
                    "TSC_RS_JSDOC_RETURN_FILTER", "TSC_RS_JSDOC_RETURN_CAPTURE_DIR",
                    "TSC_RS_DECLARATION_SPECIFIER_CAPTURE_DIR"):
            env.pop(key, None)
        return compiler_direct_command([suite]), env
    if suite in EMITTER_DIRECT:
        if needles:
            raise ValueError(f"{suite}: small direct target runs together; use --all")
        return emitter_command([suite]), env
    if suite == "printer":
        if needles:
            raise ValueError("printer failure controls run together; use --all (142 small direct rows)")
        return ["cargo", "test", "--manifest-path", "crates/emitter/Cargo.toml",
                "--test", "printer_failure_contract", "--", "--nocapture", "--test-threads=1"], env
    if suite in SUPER:
        suffix, key = SUPER[suite]
        name = f"decorator_super{suffix.replace('-', '_')}_forms_match_complete_typescript_observations"
        target, test = "decorator_super_contract", f"h2_8a_decorator_super::{name}"
        env[key] = ",".join(needle.strip() for needle in needles) if needles else "all"
    elif suite == "bundle-sinks":
        if needles:
            raise ValueError("bundle sink controls run together; use --all (10 complete commands)")
        target, test = "h2_7d_bundle_sinks", "ordinary_bundle_sink_commands_match_complete_typescript_twice"
    elif suite == "declaration-map-cli":
        if needles:
            raise ValueError("declaration map CLI controls run together; use --all (8 CLI cases)")
        target, test = "h2_7e_original_corpus", "h2_7e_original_cli_matches_outputs_diagnostics_and_exit_twice"
    elif suite == "retained":
        target = "contracts"
        test = "h2_8a_retained_accessor_owners::retained_accessor_owners_match_complete_typescript_observations"
        env["TSC_RS_RETAINED_ACCESSOR_CASE_SET"] = "all"
        if needles:
            env["TSC_RS_RETAINED_ACCESSOR_CASE_FILTER"] = ",".join(needle.strip() for needle in needles)
    else:
        if needles:
            raise ValueError("direct controls run together; use --all")
        target, test = "decorator_super_direct_contract", "decorator_super_direct_controls_match_typescript"
    owner = "emitter" if suite == "direct" else "compiler"
    command = ["cargo", "test", "--manifest-path", f"crates/{owner}/Cargo.toml",
               "--test", target, test, "--", "--exact", "--nocapture", "--test-threads=1"]
    return command, env


def emitter_inputs(suite):
    spec = EMITTER_DIRECT[suite]
    return {f"crates/emitter/tests/{spec['target']}.rs", *spec["observers"],
            *(file for file, _, _ in spec["fixtures"]), *spec.get("inputs", ())}


def compiler_direct_inputs(suite):
    spec = COMPILER_DIRECT[suite]
    # The identity observer also reads utf16-literals-adjacent-probes-inputs.
    # That shared input keeps full replay via the planner's unknown-input rule.
    return {f"crates/compiler/tests/{spec['target']}.rs",
            *(observer[1] for observer in compiler_direct_observers([suite])),
            *(file for file, _, _ in spec["fixtures"]), *spec.get("inputs", ())}


def compiler_direct_observers(suites):
    # An observer may take a group before --check. Deduplicate commands, not
    # paths: the three literal groups use the same script with distinct inputs.
    return list(dict.fromkeys(
        ("node", *((observer,) if isinstance(observer, str) else observer), "--check")
        for suite in suites for observer in COMPILER_DIRECT[suite]["observers"]))


def compiler_direct_command(suites):
    if not suites or len(set(suites)) != len(suites) or any(suite not in COMPILER_DIRECT for suite in suites):
        raise ValueError("invalid compiler direct selection")
    command = ["cargo", "test", "--manifest-path", "crates/compiler/Cargo.toml"]
    for suite in suites:
        command.extend(("--test", COMPILER_DIRECT[suite]["target"]))
    filtered = [suite for suite in suites if "test" in COMPILER_DIRECT[suite]]
    if filtered:
        if len(suites) != 1:
            raise ValueError("filtered compiler target requires its own invocation")
        names = COMPILER_DIRECT[filtered[0]]["test"]
        names = (names,) if isinstance(names, str) else names
        if (not names or any(not isinstance(name, str) or not name.strip() for name in names)
                or len(set(names)) != len(names)
                or len(names) != COMPILER_DIRECT[filtered[0]]["tests"]):
            raise ValueError("invalid exact compiler test names")
        # Cargo accepts one TESTNAME; libtest accepts additional exact names
        # after `--`. Their union runs once without replaying imported tests.
        return [*command, names[0], "--", "--exact", *names[1:], "--nocapture", "--test-threads=1"]
    return [*command, "--", "--nocapture", "--test-threads=1"]


def run_compiler_direct(suites):
    """Check selected frozen oracles and replay their complete standalone targets."""
    if not suites or len(set(suites)) != len(suites) or any(suite not in COMPILER_DIRECT for suite in suites):
        raise ValueError("invalid compiler direct selection")
    unfiltered = [suite for suite in suites if "test" not in COMPILER_DIRECT[suite]]
    batches = ([unfiltered] if unfiltered else []) + [[suite] for suite in suites if "test" in COMPILER_DIRECT[suite]]
    for suite in suites:
        print(f"{suite}: {len(case_ids(suite))} fixture rows (including any typed refusal controls)", flush=True)
    started = time.monotonic()
    for command in compiler_direct_observers(suites):
        subprocess.run(list(command), cwd=ROOT, check=True)
    oracle_seconds = time.monotonic() - started
    _, env = invocation(suites[0], [])
    started = time.monotonic()
    tests_passed = 0
    for batch in batches:
        result = subprocess.run(compiler_direct_command(batch), cwd=ROOT, env=env, text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
        print(result.stdout, end="", flush=True)
        result.check_returncode()
        counts = re.findall(r"test result: ok\. (\d+) passed; 0 failed; (\d+) ignored;.*? (\d+) filtered out;", result.stdout)
        expected = sorted((COMPILER_DIRECT[suite]["tests"], 0, COMPILER_DIRECT[suite].get("filtered_tests", 0)) for suite in batch)
        if sorted(tuple(map(int, row)) for row in counts) != expected:
            raise ValueError("compiler direct target omitted, ignored, filtered or selected zero tests")
        tests_passed += sum(int(row[0]) for row in counts)
    print(json.dumps({"compiler_direct": suites, "targets": len(suites),
                      "tests_passed": tests_passed,
                      "observer_seconds": round(oracle_seconds, 3),
                      "cargo_build_and_replay_seconds": round(time.monotonic() - started, 3)}), flush=True)


def emitter_command(suites):
    if not suites or len(set(suites)) != len(suites) or any(suite not in EMITTER_DIRECT for suite in suites):
        raise ValueError("invalid emitter direct selection")
    command = ["cargo", "test", "--manifest-path", "crates/emitter/Cargo.toml"]
    for suite in suites:
        command.extend(("--test", EMITTER_DIRECT[suite]["target"]))
    return [*command, "--", "--nocapture", "--test-threads=1"]


def run_emitter_direct(suites):
    """Check selected oracles, then build/replay only their targets in one Cargo call."""
    command = emitter_command(suites)
    for suite in suites:
        print(f"{suite}: {len(case_ids(suite))} fixture rows (each compared twice)", flush=True)
    started = time.monotonic()
    observers = dict.fromkeys(observer for suite in suites for observer in EMITTER_DIRECT[suite]["observers"])
    for observer in observers:
        subprocess.run(["node", observer, "--check"], cwd=ROOT, check=True)
    oracle_seconds = time.monotonic() - started
    _, env = invocation(suites[0], [])
    started = time.monotonic()
    result = subprocess.run(command, cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, check=False)
    print(result.stdout, end="", flush=True)
    result.check_returncode()
    counts = re.findall(r"test result: ok\. (\d+) passed; 0 failed; (\d+) ignored;.*? (\d+) filtered out;", result.stdout)
    if (len(counts) != len(suites)
            or any(int(passed) == 0 or int(ignored) or int(filtered) for passed, ignored, filtered in counts)):
        raise ValueError("emitter direct target omitted, ignored, filtered or selected zero tests")
    print(json.dumps({"emitter_direct": suites, "targets": len(suites),
                      "tests_passed": sum(int(row[0]) for row in counts),
                      "observer_seconds": round(oracle_seconds, 3),
                      "cargo_build_and_replay_seconds": round(time.monotonic() - started, 3)}), flush=True)


def run_resolution_cache(command, env):
    case_ids("resolution-cache")
    started = time.monotonic()
    subprocess.run(RESOLUTION_OBSERVER, cwd=ROOT, check=True)
    oracle_seconds = time.monotonic() - started
    started = time.monotonic()
    result = subprocess.run(command, cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, check=False)
    print(result.stdout, end="", flush=True)
    result.check_returncode()
    counts = re.findall(r"test result: ok\. (\d+) passed; 0 failed; (\d+) ignored;.*? (\d+) filtered out;", result.stdout)
    if sorted(tuple(map(int, row)) for row in counts) != [(11, 0, 0), (56, 0, 0)]:
        raise ValueError("resolution-cache: missing, ignored, filtered or changed target results")
    print(json.dumps({"resolution_cache": {"families": 26, "generations": 112, "requests": 197},
                      "contract_tests": 11, "program_unit_tests": 56,
                      "observer_seconds": round(oracle_seconds, 3),
                      "cargo_build_and_replay_seconds": round(time.monotonic() - started, 3)}), flush=True)


def run_declaration_map_cli(command, env):
    started = time.monotonic()
    result = subprocess.run(command, cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, check=False)
    print(result.stdout, end="", flush=True)
    result.check_returncode()
    if "test result: ok. 1 passed; 0 failed; 0 ignored;" not in result.stdout:
        raise ValueError("declaration-map-cli: missing or zero-test CLI comparison")
    print(json.dumps({"declaration_map_cli_cases": 8, "repetitions": 2,
                      "cargo_build_and_replay_seconds": round(time.monotonic() - started, 3)}), flush=True)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("suite", choices=SUITES)
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--case", action="append", default=[], metavar="ID_SUBSTRING",
                           help="repeat to select the union of matching case IDs")
    selection.add_argument("--all", action="store_true", help="explicit full replay (normally hosted)")
    parser.add_argument("--list", action="store_true", help="list matching input IDs without Cargo")
    parser.add_argument("--dry-run", action="store_true", help="print selection and command without running")
    args = parser.parse_args(argv)
    if not args.list and not (args.case or args.all):
        parser.error("choose --case or --all; use --list to inspect IDs")
    try:
        selected = select_cases(case_ids(args.suite), args.case)
        command, env = invocation(args.suite, args.case)
    except (ValueError, KeyError, OSError) as error:
        parser.error(str(error))
    print(f"{args.suite}: selected {len(selected)} input cases", flush=True)
    if args.suite == "primary":
        print("Primary upstream exceptions are reported separately by the comparator.", flush=True)
    if args.list:
        print("\n".join(selected))
        return 0
    assignments = {key: value for key, value in env.items()
                   if key.startswith("TSC_RS_") and (key.endswith("CASE_SET") or key.endswith("CASE_FILTER"))}
    print(shlex.join(["env", *[f"{key}={value}" for key, value in sorted(assignments.items())], *command]), flush=True)
    if args.dry_run:
        if args.suite == "resolution-cache":
            print(shlex.join(RESOLUTION_OBSERVER))
        if args.suite in EMITTER_DIRECT:
            for observer in EMITTER_DIRECT[args.suite]["observers"]:
                print(shlex.join(["node", observer, "--check"]))
        if args.suite in COMPILER_DIRECT:
            for command in compiler_direct_observers([args.suite]):
                print(shlex.join(command))
        return 0
    if args.suite == "resolution-cache":
        run_resolution_cache(command, env)
        return 0
    if args.suite in COMPILER_DIRECT:
        run_compiler_direct([args.suite])
        return 0
    if args.suite in EMITTER_DIRECT:
        run_emitter_direct([args.suite])
        return 0
    if args.suite == "declaration-map-cli":
        run_declaration_map_cli(command, env)
        return 0
    return subprocess.run(command, cwd=ROOT, env=env, check=False).returncode


if __name__ == "__main__":
    sys.exit(main())
