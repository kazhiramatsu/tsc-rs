#!/usr/bin/env python3
"""Bounded hosted entries for the emitter final integration's frozen row sets."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = "crates/compiler/tests/fixtures/"
PACKET = "docs/design/greenfield/slices/emitter-final-batch/"
SHARDS = 4
CLASS_BANDS = (
    ("class-field-alias-map-positions", 384),
    ("transformed-class-assigned-names", 108),
    ("class-field-initializer-comments", 64),
    ("promoted-class-export-maps", 144),
    ("one-sided-class-comments", 128),
    ("class-helper-accessor-producers", 144),
    ("hoisted-declaration-export-ranges", 168),
    ("class-header-token", 88),
)
SUITES = ("emitter-final", "emitter-universe-oracle",
          *(f"emitter-plan-base-{i}" for i in range(SHARDS)),
          "emitter-global", "emitter-class-0", "emitter-class-1")
SELECTORS = ("TSC_RS_EMITTER_FINAL_CASE_FILTER", "TSC_RS_EMITTER_FINAL_CASE_SET",
             "TSC_RS_EMITTER_FINAL_SHARD", "TSC_RS_EMITTER_FINAL_CAPTURE_DIR",
             "TSC_RS_EMITTER_FINAL_FAILURE_DIR", "TSC_RS_H2_8A_CAPTURE_WRITES_DIR",
             "TSC_RS_IMPORT_HELPERS_CASE_FILTER")


def read(name):
    file = ROOT / name
    data = (subprocess.check_output(["zstd", "-d", "-c", str(file)])
            if file.suffix == ".zst" else file.read_bytes())
    return json.loads(data)


def ids(name, count):
    rows = read(name)["cases"]
    result = sorted(row["case_id"] for row in rows)
    if len(result) != count or len(set(result)) != count:
        raise ValueError(f"{name}: changed or duplicate membership")
    return result


def class_bands(suite):
    return CLASS_BANDS[:4] if suite == "emitter-class-0" else CLASS_BANDS[4:]


def case_ids(suite):
    if suite not in SUITES:
        raise ValueError("unknown emitter final suite")
    if suite.startswith("emitter-plan-base-"):
        part = int(suite.rsplit("-", 1)[1])
        return ids(FIXTURES + "emitter-final-universe-plan-base.json.zst", 1798)[part::SHARDS]
    if suite.startswith("emitter-class-"):
        return [case for name, count in class_bands(suite)
                for case in ids(FIXTURES + name + ".json", count)]
    if suite == "emitter-global":
        rows = read("ratchets/h2-8a-candidates.v1.json")["cases"]
        result = sorted(row["case_id"] for row in rows if row["required_slices"] == ["H2.8a"])
        if len(result) != 769 or len(set(result)) != 769:
            raise ValueError("global output matrix membership changed")
        return result
    universe = ids(FIXTURES + "emitter-final-universe.json", 217)
    if suite == "emitter-universe-oracle":
        return universe + ids(FIXTURES + "emitter-final-universe-plan-base.json.zst", 1798)
    # These are observation memberships, not a deduplicated compatibility total.
    groups = read(PACKET + "inventory.v1.json")["groups"]
    historical = [f"{group}/{case}" for group in ("EF2", "EF3", "EF4", "EF5", "EF6")
                  for case in groups[group]["cases"]]
    historical.append("EF3-shared-H2.6a/typescript-6.0.3/compiler/sourceMapValidationDestructuringForArrayBindingPattern.ts#target%3Des2015")
    return (historical + universe + ids(FIXTURES + "output-matrix.json", 22)
            + ids(FIXTURES + "output-matrix-filesystem.json", 4)
            + ids(FIXTURES + "output-filesystem.json", 24) + ids(FIXTURES + "import-helpers.json", 480)
            + ids(FIXTURES + "emitter-audit-class-regressions.json", 24)
            + ids(FIXTURES + "emitter-cli-options.json", 58)
            + ["typescript-6.0.3/compiler/jsFileCompilationAwaitModifier.ts#default",
               "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsTypeAliases.ts#default"])


def inputs(suite):
    common = {"scripts/emitter_final_witnesses.py"}
    universe = {"crates/compiler/tests/emitter_final_universe.rs",
                FIXTURES + "emitter-final-known-native.json",
                PACKET + "integration/records/retired-checker-known.v1.json",
                "scripts/observe-emitter-final-universe.mjs"}
    if suite.startswith("emitter-plan-base-"):
        return common | universe | {FIXTURES + "emitter-final-universe-plan-base.json.zst",
                                    PACKET + "ef7/universe-plan-base.v1.json"}
    if suite == "emitter-universe-oracle":
        return common | universe | {FIXTURES + "emitter-final-universe-plan-base.json.zst",
                                    FIXTURES + "emitter-final-universe.json",
                                    PACKET + "ef7/universe-plan-base.v1.json", PACKET + "ef7/universe-217.v1.json"}
    if suite.startswith("emitter-class-"):
        return common | {FIXTURES + name + ".json" for name, _ in class_bands(suite)} | {
            "crates/compiler/tests/integration/h2_8a_" + name.replace("-", "_") + ".rs"
            for name, _ in class_bands(suite)}
    if suite == "emitter-global":
        return common | {"crates/compiler/tests/h2_8a_original_corpus.rs"}
    return common | universe | {FIXTURES + name for name in (
        "emitter-final-universe.json", "output-matrix.json", "output-matrix-filesystem.json", "emitter-cli-options.json",
        "output-filesystem.json", "import-helpers.json", "emitter-audit-class-regressions.json")} | {
        PACKET + "inventory.v1.json", PACKET + "ef7/universe-217.v1.json",
        "crates/compiler/tests/emitter_final_rows.rs", "crates/compiler/tests/emitter_final_batch.rs",
        "crates/compiler/tests/integration/h2_8a_output_matrix.rs", "scripts/observe-output-matrix.mjs",
        "crates/compiler/tests/integration/h2_8a_output_filesystem.rs",
        "crates/compiler/tests/integration/h2_8a_import_helpers.rs", "scripts/observe-import-helpers.mjs",
        "crates/compiler/tests/integration/emitter_residual_audit.rs",
        "crates/compiler/tests/integration/cli_contract.rs", "scripts/observe-emitter-cli-options.mjs",
        "crates/program/tests/integration/module_request_contract.rs"}


def environment(suite, environ=None):
    env = dict(os.environ if environ is None else environ)
    for key in SELECTORS:
        env.pop(key, None)
    env.setdefault("CARGO_BUILD_JOBS", "2")
    if suite.startswith("emitter-plan-base-"):
        env["TSC_RS_EMITTER_FINAL_SHARD"] = f"{suite.rsplit('-', 1)[1]}/{SHARDS}"
    return env


def cargo(target, names=()):
    command = ["cargo", "test", "--manifest-path", "crates/compiler/Cargo.toml", "--test", target]
    if names:
        command += [names[0], "--", "--exact", *names[1:]]
    else:
        command += ["--"]
    return [*command, "--nocapture", "--test-threads=1"]


def commands(suite):
    if suite == "emitter-universe-oracle":
        return [(["node", "scripts/observe-emitter-final-universe.mjs", "--check", "--set", name], None)
                for name in ("217", "plan-base")]
    if suite.startswith("emitter-plan-base-"):
        return [(cargo("emitter_final_universe", ("plan_base_rows_match_complete_production_commands",)), 1)]
    if suite == "emitter-global":
        return [(cargo("h2_8a_original_corpus", ("original_output_matrix_candidates_match_complete_production_commands",)), 1)]
    if suite.startswith("emitter-class-"):
        names = tuple("h2_8a_" + name.replace("-", "_") + "::" + name.replace("-", "_")
                      + ("_match" if name in ("hoisted-declaration-export-ranges", "transformed-class-assigned-names") else "_matches")
                      + "_complete_typescript_observations" for name, _ in class_bands(suite))
        return [(cargo("contracts", names), len(names))]
    return [(["node", "crates/oracle/h2-6c-qualification.mjs", "--check-cases",
              *("typescript-6.0.3/compiler/" + name + ".ts#default" for name in (
                  "sourceMapWithNonCaseSensitiveFileNames", "sourceMapWithNonCaseSensitiveFileNamesAndOutDir",
                  "sourceMapWithCaseSensitiveFileNames", "sourceMapWithCaseSensitiveFileNamesAndOutDir"))], None),
            (["node", "scripts/observe-output-matrix.mjs", "--check"], None),
            (["node", "scripts/observe-import-helpers.mjs", "--check"], None),
            (["node", "scripts/observe-emitter-cli-options.mjs", "--check"], None),
            (["cargo", "test", "--manifest-path", "crates/program/Cargo.toml", "--test", "contracts",
              "module_request_contract::", "--", "--nocapture", "--test-threads=1"], 40),
            (cargo("emitter_final_rows"), 1), (cargo("emitter_final_batch"), 11),
            (cargo("contracts", ("h2_8a_output_matrix::output_matrix_matches_complete_typescript_observations",
                                 "h2_8a_output_matrix::output_matrix_filesystem_matches_complete_typescript_observations",
                                 "h2_8a_import_helpers::import_helpers_matches_complete_typescript_observations",
                                 "h2_8a_output_filesystem::output_filesystem_matches_complete_typescript_observations",
                                 "emitter_residual_audit::javascript_regressions_match_complete_original_commands",
                                 "emitter_residual_audit::anonymous_class_names_match_complete_original_commands",
                                 "cli_contract::implemented_emit_option_names_match_typescript_cli_and_config")), 7),
            (cargo("emitter_final_universe", ("universe_rows_match_complete_production_commands",
                 "known_checker_divergence_rejects_changed_output_and_diagnostics",
                 "known_refusal_rejects_changed_error_or_partial_writes", "universe_shards_are_disjoint_and_complete")), 4)]


def validate_output(suite, command, expected_tests, output):
    if expected_tests is not None:
        results = re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;", output)
        if results != [(str(expected_tests), "0", "0")]:
            raise ValueError(f"{suite}: missing, ignored or zero-test result: {results}")
    if expected_tests is not None and "emitter_final_universe" in command:
        rows = len(case_ids(suite)) if suite.startswith("emitter-plan-base-") else 217
        total = 1798 if suite.startswith("emitter-plan-base-") else 217
        summaries = re.findall(r"emitter-final universe .*: selected (\d+)/(\d+) / exact (\d+) / known (\d+) / failed (\d+)", output)
        if len(summaries) != 1:
            raise ValueError("missing universe summary")
        selected, denominator, exact, known, failed = map(int, summaries[0])
        if (selected, denominator, failed) != (rows, total, 0) or exact + known != rows:
            raise ValueError("universe selected fewer rows than its registered shard")


def run(suite, environ=None):
    selected = case_ids(suite)
    env = environment(suite, environ)
    print(json.dumps({"suite": suite, "selected_memberships": len(selected)}), flush=True)
    for command, expected_tests in commands(suite):
        started = time.monotonic()
        print(json.dumps({"suite": suite, "argv": command, "event": "start"}), flush=True)
        result = subprocess.run(command, cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, check=False)
        print(result.stdout, end="", flush=True)
        result.check_returncode()
        validate_output(suite, command, expected_tests, result.stdout)
        print(json.dumps({"suite": suite, "event": "passed", "seconds": round(time.monotonic() - started, 3)}), flush=True)


if __name__ == "__main__":
    if len(sys.argv) != 2 or sys.argv[1] not in SUITES:
        raise SystemExit("expected one emitter final suite")
    run(sys.argv[1])
