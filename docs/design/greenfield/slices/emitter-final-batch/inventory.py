#!/usr/bin/env python3
"""Reproduce the handoff's source inventory; this does not execute or qualify cases."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
BASE = "2883e3c79247b988106c6e4f1f2bdd074ae90e66"


def inventory():
    pins = {}

    def read(path):
        data = subprocess.check_output(["git", "show", f"{BASE}:{path}"], cwd=ROOT)
        pins[path] = hashlib.sha256(data).hexdigest()
        return data

    def load(path):
        return json.loads(read(path))

    plan = load("docs/design/greenfield/slices/plan-base/inventory.v1.json")
    indexed = {row["case_id"]: row for row in plan["cases"]}
    post = load("crates/compiler/tests/fixtures/post-t1-residuals-known-native.json")["cases"]
    known = {}
    for phase in ("5h", "6a", "6c"):
        path = f"ratchets/h2-{phase}-known-divergences.v1.json"
        known[phase] = load(path)["cases"]
    assert [len(known[p]) for p in ("5h", "6a", "6c")] == [12, 1, 8]
    assert len({r["case_id"] for rows in known.values() for r in rows}) == 20
    assert {r["case_id"] for r in known["6a"]} <= {r["case_id"] for r in known["6c"]}
    classes = [r for r in plan["cases"] if r["disposition"] == "historical-class-failure-not-remeasured"]
    field = [r for r in classes if r["class_failure"]["fixture"].endswith("class-field-alias-map-positions.json")]
    token = [r for r in classes if r not in field]
    assert (len(classes), len(field), len(token)) == (40, 28, 12)
    for row in classes:
        path = row["class_failure"]["fixture"]
        read(path)
        assert pins[path] == row["class_failure"]["fixture_sha256"]
    global_rows = [r for r in plan["cases"] if r["disposition"] in (
        "historical-global-failure-not-remeasured", "historical-global-repair-not-remeasured")]
    assert len(global_rows) == 14
    no_record = [r["case_id"] for r in plan["cases"] if any(
        m["origin"] == "global-input-universe" and m["state"] == "no-current-hosted-product-record"
        for m in r["memberships"])]
    assert len(no_record) == 217
    groups = {
        "EF1": {"state": "guarded-known-command-difference", "cases": [r["case_id"] for r in post]},
        "EF2": {"state": "guarded-known-command-difference", "cases": [r["case_id"] for r in known["5h"]]},
        "EF3": {"state": "guarded-known-command-difference", "cases": [r["case_id"] for r in known["6c"]],
                "shared_h2_6a": [r["case_id"] for r in known["6a"]],
                "current_refusal_overrides": {r["case_id"]: indexed[r["case_id"]]["current_refused_option"]
                    for r in known["6c"] if "current_refused_option" in indexed[r["case_id"]]}},
        "EF4": {"state": "historical-unmeasured", "cases": [r["case_id"] for r in field]},
        "EF5": {"state": "historical-unmeasured", "cases": [r["case_id"] for r in token]},
        "EF6": {"state": "historical-not-current-failure-count", "cases": [r["case_id"] for r in global_rows],
                "later_repair_record": [r["case_id"] for r in global_rows if r["disposition"] == "historical-global-repair-not-remeasured"]},
        "EF7": {"state": "coverage-audit-not-bug-count", "cases": no_record,
                "also_reconcile": "All PLAN-BASE memberships against current hosted observations, per profile and observation."},
        "EF8": {"state": "closure-audit-and-integration-proposal", "cases": [],
                "historical_full_replay_denominators": {"global": 769, "class": 1228},
                "full_replay_owner": "integrator-hosted"},
    }
    assert len(post) == 5
    for group in groups.values():
        group["cases"] = sorted(group["cases"])
        assert len(group["cases"]) == len(set(group["cases"]))
        group["count"] = len(group["cases"])
    for path in (
        "vendor/typescript-6.0.3/lib/_tsc.js", "vendor/typescript-6.0.3/lib/typescript.js",
        "ratchets/h2-8a-global-after-a6-37.v1.json",
        "crates/emitter/tests/decorator_binding_contract.rs",
        "crates/emitter/tests/decorator_super_direct_contract.rs",
    ):
        read(path)
    for name in ("decorator-binding-known-native", "bundle-metadata-t1-known-native",
                 "bundle-metadata-t1-known-packet", "post-t1-residuals-known-packet"):
        data = load(f"crates/compiler/tests/fixtures/{name}.json")
        assert data["cases"] == [], name
    return {
        "version": 1, "kind": "handoff-source-inventory-not-runtime-evidence",
        "reference_commit": BASE, "semantics": "TypeScript 6.0.3", "new_native_executions": 0,
        "historical_plan_base": plan["base_revision"], "source_sha256": dict(sorted(pins.items())),
        "groups": groups, "counts_are_not_additive": True,
        "must_remeasure_at_implementation_base": True,
        "separate_later_api_work": {"disposed_transform_print_typed_known": 2,
                                    "custom_shared_node_super_direct_known": 4},
        "not_a_whole_emitter_completion_claim": True,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--write", action="store_true")
    parser.add_argument("--output", type=Path, default=HERE / "inventory.v1.json")
    args = parser.parse_args()
    data = (json.dumps(inventory(), ensure_ascii=False, indent=2) + "\n").encode()
    if args.check:
        assert args.output.read_bytes() == data, "handoff inventory drift"
    else:
        with args.output.open("xb") as stream:
            stream.write(data)
    print("Verified EF1..EF8 source inventory; no native replay or qualification performed.")


if __name__ == "__main__":
    main()
