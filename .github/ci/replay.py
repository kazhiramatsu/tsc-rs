#!/usr/bin/env python3
"""Conservative hosted replay planning and bounded acceptance groups."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import witness

GROUPS = {
    "early": ("conformance", "h1", "h2-1a", "h2-1b", "h2-1c", "h2-1d", "h2-1e",
              "h2-2a", "h2-2b", "h2-2c", "h2-2d", "h2-3a", "h2-3b", "h2-3c", "h2-3d",
              "h2-4a", "h2-4b", "h2-5a", "h2-5b", "h2-5c", "h2-5d", "h2-5e", "h2-5f"),
    "wide": ("h2-5g",),
    "late": ("h2-5h", "h2-6a", "h2-6b", "h2-6c", "h2-7b", "h2-7c", "h2-7de"),
}
# One compiler build serves all four short collections. A changed fixture may
# select just one collection within this job; direct controls share its build.
WITNESS_GROUPS = {
    "primary": ("primary",),
    "controls": ("extra", "followup", "followup2", "followup3", "direct", "bundle-sinks", "declaration-map-cli",
                 *witness.COMPILER_DIRECT),
    "retained": ("retained",),
    # Reuse this short job's emitter build. Fixture changes select individual
    # direct suites without running unrelated printer failure/owner controls.
    "printer": ("printer", *witness.EMITTER_DIRECT),
}
PRINTER_TARGETS = (
    "printer_failure_contract", "emit_pipeline_phases_contract",
    "comma_list_printer_contract", "list_format_flags_contract",
    "list_comment_flags_contract", "source_comment_topology_contract",
    "utf16_writer_contract",
)
PRINTER_INPUTS = {
    *(f"crates/emitter/tests/{target}.rs" for target in PRINTER_TARGETS),
    *(f"crates/emitter/tests/fixtures/{name}.json" for name in (
        "printer-failure-hooks", "printer-failure-probes", "printer-failure-review",
        "printer-failure-known-native", "printer-comment-carry",
        "printer-hook-hints", "emit-pipeline-phases", "comma-list-printer",
        "list-format-flags", "list-comment-flags", "utf16-writer")),
    "scripts/observe-printer-failures.mjs", "scripts/observe-printer-failure-review.mjs",
    "scripts/observe-printer-comment-carry.mjs",
    "scripts/observe-printer-hook-hints.mjs",
}
SUPER_MODULES = {
    "crates/compiler/tests/integration/h2_8a_decorator_super.rs",
    "crates/compiler/tests/decorator_super_contract.rs",
}
RETAINED_ONLY = {
    "retained-accessor-owners", "retained-accessor-inputs",
    "retained-lexical-environments", "retained-lexical-inputs",
    "retained-lexical-edges", "retained-lexical-edge-inputs",
    "retained-constructor-references", "retained-constructor-reference-inputs",
    "retained-comma-factory", "retained-comma-factory-inputs",
    "decorator-receiver-context", "decorator-receiver-context-inputs",
    "class-helper-accessor-producers",
}


def full_selection(reason):
    return {"acceptance": list(GROUPS), "witnesses": list(witness.SUITES), "reason": reason}


def selection(paths):
    if paths is None or not paths:
        return full_selection("missing or empty change range")
    acceptance, witnesses = set(), set()
    for file in paths:
        if file.startswith("docs/") or file in ("README.md", "CONTRIBUTING.md", "LICENSE"):
            continue
        direct_owners = {suite for suite in witness.EMITTER_DIRECT if file in witness.emitter_inputs(suite)}
        if direct_owners:
            witnesses.update(direct_owners)
            continue
        compiler_owners = {suite for suite in witness.COMPILER_DIRECT if file in witness.compiler_direct_inputs(suite)}
        if compiler_owners:
            witnesses.update(compiler_owners)
            continue
        if file in PRINTER_INPUTS:
            witnesses.add("printer")
            continue
        if file in ("crates/compiler/tests/h2_7d_bundle_sinks.rs",
                    "crates/compiler/tests/fixtures/bundle-sinks.json"):
            witnesses.add("bundle-sinks")
            continue
        if file == "crates/compiler/tests/h2_7e_original_corpus.rs":
            witnesses.add("declaration-map-cli")
            continue
        if file == "crates/compiler/tests/integration/h2_7e_original_corpus_shared.rs":
            acceptance.add("late")
            witnesses.add("declaration-map-cli")
            continue
        if file in SUPER_MODULES:
            witnesses.update(witness.SUPER)
            continue
        if file.startswith("crates/compiler/tests/fixtures/"):
            name = file.rsplit("/", 1)[1]
            matched = False
            for suite, (suffix, _) in witness.SUPER.items():
                stem = f"decorator-super{suffix}"
                if name in (f"{stem}.json", f"{stem}.json.zst", f"{stem}-inputs.json"):
                    witnesses.add(suite)
                    matched = True
                    break
            if matched:
                continue
            if name.endswith(".json") and name[:-5] in RETAINED_ONLY:
                witnesses.add("retained")
                continue
        if file in (
            "crates/emitter/tests/decorator_super_direct_contract.rs",
            "crates/emitter/tests/fixtures/decorator-super-direct.json",
        ):
            witnesses.add("direct")
            continue
        if file == "crates/compiler/tests/integration/h2_8a_retained_accessor_owners.rs":
            witnesses.add("retained")
            continue
        if file == "crates/compiler/tests/integration/h2_7c_declaration_blocking.rs":
            acceptance.add("late")
            witnesses.update(("retained", "utf16-literal-witnesses"))
            continue
        if file == "crates/compiler/tests/support/witness_libraries.rs":
            acceptance.add("late")
            witnesses.update((*witness.SUPER, "retained", "utf16-literal-witnesses"))
            continue
        # Shared product code, manifests, vendor, CI, TS corpus, and unknown
        # inputs keep complete coverage. Never infer that tests/ is disconnected:
        # several compiler comparators are imported by xtask via #[path].
        return full_selection(f"shared or unknown input: {file}")
    return {
        "acceptance": [group for group in GROUPS if group in acceptance],
        "witnesses": [suite for suite in witness.SUITES if suite in witnesses],
        "reason": "explicit disconnected or owning-input rules",
    }


def changed_paths(event_name, event, root=ROOT):
    if event_name == "pull_request":
        base = event.get("pull_request", {}).get("base", {}).get("sha")
    elif event_name == "merge_group":
        base = event.get("merge_group", {}).get("base_sha")
    elif event_name == "push":
        base = event.get("before")
    else:
        return None
    if not isinstance(base, str) or not re.fullmatch(r"[0-9a-f]{40}", base) or base == "0" * 40:
        return None
    try:
        result = subprocess.run(
            ["git", "diff", "--no-renames", "--name-only", "-z", base, "HEAD", "--"],
            cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True,
        )
        # Include both old and new paths for renames; no API pagination/300-file
        # path-filter limit. Decode strictly: unfamiliar input selects everything.
        paths = result.stdout.decode("utf-8").split("\0")
        return [file for file in paths if file]
    except (subprocess.CalledProcessError, UnicodeError, OSError):
        return None


def validate_partition(root=ROOT):
    slices = [item for group in GROUPS.values() for item in group]
    if len(slices) != len(set(slices)):
        raise ValueError("acceptance partition contains duplicate slices")
    registry = (root / "crates/xtask/src/acceptance_plan.rs").read_text()
    registry = registry.split('pub(crate) const SLICE_IDS: &[&str] = &[', 1)[1].split('];', 1)[0]
    if slices != re.findall(r'"([a-z0-9-]+)"', registry):
        raise ValueError("acceptance partition differs from the Rust slice registry")
    # Check dispatch against the entire canonical acceptance call sequence, not
    # just names/counts. Missing late slices or owner-control wrappers must fail.
    source = (root / "crates/xtask/src/main.rs").read_text()
    body = source.split("fn acceptance(", 1)[1].split("\n}\n", 1)[0]
    canonical_calls = re.findall(r"^    ([a-z0-9_:]+)\(&workspace\)", body, re.MULTILINE)
    dispatch = (root / "crates/xtask/src/acceptance_slices.rs").read_text()
    rows = re.findall(r'"([a-z0-9-]+)" => crate::([a-z0-9_:]+)\(workspace\)', dispatch)
    if [item for item, _ in rows] != slices[1:]:
        raise ValueError("acceptance dispatch contains reordered, duplicate or missing slices")
    targets = dict(rows)
    if [targets.get(item) for item in slices[1:]] != canonical_calls:
        raise ValueError("acceptance dispatch differs from canonical complete coverage")
    if '"conformance" => crate::conformance(std::iter::empty())' not in dispatch:
        raise ValueError("conformance dispatch changed")


def matrices(plan):
    return {
        "acceptance": {"include": [{"group": group} for group in plan["acceptance"]]},
        "witnesses": {"include": [
            {"group": group, "suites": [suite for suite in suites if suite in plan["witnesses"]]}
            for group, suites in WITNESS_GROUPS.items() if any(suite in plan["witnesses"] for suite in suites)
        ]},
    }


def verify_gate(needs, kind):
    if set(needs) != {"plan", kind} or needs["plan"].get("result") != "success":
        raise ValueError("replay planning did not succeed")
    selected = needs["plan"].get("outputs", {}).get(f"has_{kind}")
    if selected not in ("true", "false"):
        raise ValueError("missing replay selection")
    expected = "success" if selected == "true" else "skipped"
    if needs[kind].get("result") != expected:
        raise ValueError(f"{kind}: expected {expected}, got {needs[kind].get('result')}")


def printer_witnesses():
    """Direct failure/reuse and adjacent owner controls; no compiler/oracle chain."""
    for observer in ("observe-printer-failures.mjs", "observe-printer-failure-review.mjs",
                     "observe-printer-comment-carry.mjs", "observe-printer-hook-hints.mjs"):
        subprocess.run(["node", f"scripts/{observer}", "--check"], cwd=ROOT, check=True)
    command = ["cargo", "test", "--manifest-path", "crates/emitter/Cargo.toml"]
    for target in PRINTER_TARGETS:
        command.extend(("--test", target))
    command.extend(("--", "--nocapture", "--test-threads=1"))
    # These small direct targets finish in milliseconds after compilation.
    # Preserve their output while checking that a removed/misnamed target did
    # not turn a selected job into a silent zero-test success.
    result = subprocess.run(command, cwd=ROOT, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, check=False)
    print(result.stdout, end="", flush=True)
    result.check_returncode()
    counts = re.findall(r"test result: ok\. (\d+) passed; 0 failed;", result.stdout)
    if len(counts) != len(PRINTER_TARGETS) or any(int(count) == 0 for count in counts):
        raise ValueError("printer witness target omitted or selected zero tests")
    gate = subprocess.run([
        "cargo", "test", "--manifest-path", "crates/emitter/Cargo.toml", "--test", "contracts",
        "output_plan_contract::duplicate_output_preflight_reaches_no_sink_and_obeys_no_emit_on_error",
        "--", "--exact", "--nocapture", "--test-threads=1",
    ], cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
    print(gate.stdout, end="", flush=True)
    gate.check_returncode()
    if "test result: ok. 1 passed; 0 failed;" not in gate.stdout:
        raise ValueError("noEmitOnError owner control selected zero tests")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("plan", "acceptance", "witnesses", "gate"))
    parser.add_argument("value", nargs="?")
    args = parser.parse_args()
    if args.command == "plan":
        subprocess.run(["node", ".github/ci/qualification.mjs", "check-policy"], cwd=ROOT, check=True)
        subprocess.run([sys.executable, "-m", "unittest", "discover", "-s", ".github/ci",
                        "-p", "test_replay.py"], cwd=ROOT, check=True)
        validate_partition()
        event_path = os.environ.get("GITHUB_EVENT_PATH")
        try:
            event = json.loads(Path(event_path).read_text()) if event_path else {}
        except (OSError, ValueError):
            event = {}
        plan = selection(changed_paths(os.environ.get("GITHUB_EVENT_NAME"), event))
        print(json.dumps(plan, indent=2), flush=True)
        matrix = matrices(plan)
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            for kind in ("acceptance", "witnesses"):
                output.write(f"has_{kind}={str(bool(plan[kind])).lower()}\n")
                output.write(f"{kind}_matrix={json.dumps(matrix[kind], separators=(',', ':'))}\n")
    elif args.command == "acceptance":
        validate_partition()
        if args.value not in GROUPS:
            parser.error("unknown acceptance group")
        for item in GROUPS[args.value]:
            print(f"acceptance slice: {item}", flush=True)
            subprocess.run(["cargo", "xtask", "acceptance-slice", item], cwd=ROOT, check=True)
    elif args.command == "witnesses":
        suites = json.loads(os.environ["WITNESS_SUITES"])
        if not isinstance(suites, list) or not suites or len(set(suites)) != len(suites) or any(suite not in witness.SUITES for suite in suites):
            parser.error("invalid witness suite selection")
        for suite in suites:
            if suite == "printer":
                printer_witnesses()
            elif suite in witness.EMITTER_DIRECT or suite in witness.COMPILER_DIRECT:
                continue  # Batch selected small targets below, sharing one build.
            else:
                subprocess.run([sys.executable, "scripts/witness.py", suite, "--all"], cwd=ROOT, check=True)
        direct = [suite for suite in suites if suite in witness.EMITTER_DIRECT]
        if direct:
            witness.run_emitter_direct(direct)
        compiler = [suite for suite in suites if suite in witness.COMPILER_DIRECT]
        if compiler:
            witness.run_compiler_direct(compiler)
    else:
        if args.value not in ("acceptance", "witnesses"):
            parser.error("gate requires acceptance or witnesses")
        verify_gate(json.loads(os.environ["REPLAY_NEEDS"]), args.value)
        print(f"{args.value} replay gate passed")


if __name__ == "__main__":
    main()
