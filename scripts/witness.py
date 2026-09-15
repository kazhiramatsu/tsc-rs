#!/usr/bin/env python3
"""Run a focused witness selection without building xtask first."""
import argparse
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "crates/compiler/tests/fixtures"
SUPER = {
    "primary": ("", "TSC_RS_DECORATOR_SUPER_CASE_SET"),
    **{name: (f"-{name}", f"TSC_RS_DECORATOR_SUPER_{name.upper()}_CASE_SET")
       for name in ("extra", "followup", "followup2", "followup3")},
}
SUITES = (*SUPER, "retained", "direct", "printer", "bundle-sinks")
RETAINED_FIXTURES = (
    "retained-accessor-owners", "class-helper-accessor-producers",
    "class-field-alias-map-positions", "decorator-receiver-context",
    "retained-lexical-environments", "retained-constructor-references",
    "retained-lexical-edges", "retained-comma-factory",
)


def read_cases(file):
    return json.loads(file.read_text())["cases"]


def case_ids(suite):
    if suite in SUPER:
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
        for name in ("printer-failure-hooks", "printer-failure-review", "printer-comment-carry"):
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
    if suite == "printer":
        if needles:
            raise ValueError("printer failure controls run together; use --all (70 small direct rows)")
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
        return 0
    return subprocess.run(command, cwd=ROOT, env=env, check=False).returncode


if __name__ == "__main__":
    sys.exit(main())
