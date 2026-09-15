#!/usr/bin/env python3
"""Reproduce the dated PLAN-BASE crosswalk; never run a compiler or change a ratchet."""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import re
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
BASE = "f9ef828a56c9f9305947a6e6f3c1ae39072b3110"
RUN_HEAD = "dac0d55cec8d9e41cf958b8d5256a0c3cfe8c4b9"
PHASES = (
    "1a", "1b", "1c", "1d", "1e", "2a", "2b", "2c", "2d",
    "3a", "3b", "3c", "3d", "4a", "4b", "5a", "5b", "5c",
    "5d", "5e", "5f", "5g", "5h", "6a", "6b", "6c", "7b", "7c", "7de",
)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def json_digest(value):
    return digest(json.dumps(value, sort_keys=True, ensure_ascii=True,
                             separators=(",", ":")).encode())


def indexed(cases, key="case_id"):
    result = {row[key]: row for row in cases}
    if len(result) != len(cases):
        raise ValueError(f"duplicate {key}")
    return result


class Sources:
    """Read the immutable base, so the dated report remains reproducible on later main."""
    def __init__(self):
        self.pins = {}

    def read(self, path):
        data = subprocess.check_output(["git", "show", f"{BASE}:{path}"], cwd=ROOT)
        self.pins[path] = {"path": path, "sha256": digest(data), "bytes": len(data)}
        return data

    def json(self, path):
        return json.loads(self.read(path))


def identifiers(lines, pattern):
    ids = [match.group(1) for line in lines if (match := re.fullmatch(pattern, line))]
    if len(set(ids)) != len(ids):
        raise ValueError(f"duplicate hosted marker: {pattern}")
    return set(ids)


def load_evidence():
    path = HERE / "hosted-evidence.v1.json"
    data = json.loads(path.read_text())
    assert data["base_revision"] == BASE and data["run_head"] == RUN_HEAD
    changed = subprocess.check_output(
        ["git", "diff", "--name-only", RUN_HEAD, BASE], cwd=ROOT, text=True).splitlines()
    assert changed == data["source_equivalence"]["changed_paths"]
    assert all(path.startswith("docs/") for path in changed)
    jobs = indexed(data["jobs"], "group")
    assert set(jobs) == {"early", "wide", "late", "primary", "controls", "retained"}
    for job in jobs.values():
        assert job["status"] == "completed" and job["conclusion"] == "success"
        assert job["run_head"] == RUN_HEAD
    return jobs, {"path": str(path.relative_to(ROOT)), "sha256": digest(path.read_bytes())}


def future_ids(global_cases, phase, candidates, expected=None):
    ids = {case_id for case_id, row in global_cases.items()
           if phase in row["required_slices"]} - set(candidates)
    if expected is not None and len(ids) != expected:
        raise ValueError(f"{phase}: future denominator {len(ids)} != {expected}")
    return ids


def disposition(row):
    # An exact observation in another context cannot erase a known difference.
    if row.get("upstream_exceptions"):
        return "upstream-exception-no-native-credit"
    if row.get("known_divergences"):
        return "current-known-divergence"
    if row.get("direct_divergence"):
        return "current-direct-divergence"
    if row.get("parameter_comment_gap"):
        return "recorded-printer-gap-not-remeasured"
    if row.get("class_failure"):
        return ("class-repair-confirmed-hosted" if row.get("same_fixture_hosted_exact")
                else "historical-class-failure-not-remeasured")
    if row.get("global_failure"):
        return ("historical-global-repair-not-remeasured" if row.get("repair_records")
                else "historical-global-failure-not-remeasured")
    if row.get("hosted_exact"):
        return "hosted-exact-in-listed-profile-only"
    if row.get("historical_global_exact"):
        return "historical-global-exact-not-remeasured"
    return "later-owned-no-current-exact-record"


def build():
    sources = Sources()
    jobs, evidence_pin = load_evidence()
    global_cases = indexed(sources.json("ratchets/h2-candidate-dispositions.v1.json")["cases"], "id")
    assert len(global_cases) == 15642
    rows = {}
    seeds = set()
    profile_summaries = []

    def row(case_id):
        if case_id not in rows:
            rows[case_id] = {"case_id": case_id, "memberships": [], "hosted_exact": []}
            if case_id in global_cases:
                original = global_cases[case_id]
                rows[case_id]["original_required_slices"] = original["required_slices"]
        return rows[case_id]

    def membership(case_id, origin, state, owners=()):
        seeds.add(case_id)
        row(case_id)["memberships"].append(
            {"origin": origin, "state": state, "owners": list(owners)})

    known = {}
    for short in ("5h", "6a", "6c"):
        phase = f"H2.{short}"
        known[phase] = indexed(sources.json(f"ratchets/h2-{short}-known-divergences.v1.json")["cases"])
        for case_id, details in known[phase].items():
            membership(case_id, phase, "known-divergence", [details["owner"]])
            row(case_id).setdefault("known_divergences", {})[phase] = details
    assert [len(known[f"H2.{s}"]) for s in ("5h", "6a", "6c")] == [12, 1, 8]

    c_exact = identifiers(jobs["late"]["observations"], r"H2\.7c corpus PASS (\S+)")
    de_exact = set()
    for pattern, count in [(r"H2\.7d original EXACT x2 (\S+)", 283),
                           (r"H2\.7e original EXACT x2 (\S+)", 8),
                           (r"H2\.8a original directory EXACT x2 (\S+)", 23)]:
        ids = identifiers(jobs["late"]["observations"], pattern)
        assert len(ids) == count, (pattern, len(ids))
        assert de_exact.isdisjoint(ids)
        de_exact.update(ids)
    assert len(c_exact) == 32 and len(de_exact) == 314

    for short in PHASES:
        phase = f"H2.{short}"
        path = f"ratchets/h2-{short}-qualification.v1.json"
        artifact = sources.json(path)
        cases = indexed(artifact["cases"])
        observed_exact = 0
        observed_deferred = 0
        migrated = 0
        for case_id, case in cases.items():
            recorded = case["disposition"]
            owners = case.get("remaining_slices", case.get("required_slices", []))
            deferred = recorded in ("deferred-to-slices", "deferred")
            if short == "7c":
                exact = case_id in c_exact
            elif short == "7de":
                exact = case_id in de_exact
            else:
                assert recorded in ("admitted-for-execution", "diagnostic-deferred-output-control",
                                    "deferred-to-slices"), (phase, recorded)
                exact = not deferred and case_id not in known.get(phase, {})
            if deferred:
                membership(case_id, phase, "historical-deferred-migrated" if exact else "source-deferred", owners)
                migrated += int(exact)
                observed_deferred += int(not exact)
            if exact:
                observed_exact += 1
                # This digest identifies the input in this profile only. Equality
                # of IDs or digests never promotes another profile automatically.
                data = {"profile": phase, "artifact": path,
                        "input_sha256": json_digest(case.get("replay_input", case.get("input", {})))}
                row(case_id)["hosted_exact"].append(data)
        selector = artifact.get("selection_contract", {})
        future = None
        if isinstance(selector, dict) and "future_deferred_rows" in selector:
            future = future_ids(global_cases, phase, cases, selector["future_deferred_rows"])
            for case_id in sorted(future):
                membership(case_id, phase, "frozen-future-deferred", global_cases[case_id]["required_slices"])
        if short == "7b":
            future = future_ids(global_cases, phase, cases, 863)
            for case_id in sorted(future):
                membership(case_id, phase, "outside-frozen-candidate-band", global_cases[case_id]["required_slices"])
        group = "wide" if short == "5g" else "late" if short in ("5h", "6a", "6b", "6c", "7b", "7c", "7de") else "early"
        if short == "7c":
            assert observed_exact == len(c_exact), "unmatched H2.7c exact ID"
        elif short == "7de":
            assert observed_exact == len(de_exact), "unmatched H2.7d/e exact ID"
        if short not in ("7c", "7de"):
            log = next(line for line in jobs[group]["observations"] if line.startswith(f"{phase} emit acceptance:"))
            counts = {key: int(value) for key, value in re.findall(r"(\w+)=(\d+)", log)}
            assert counts["candidates"] == len(cases), phase
            assert counts["exact"] == observed_exact, (phase, observed_exact, counts)
            assert counts.get("known_diverging", 0) == len(known.get(phase, {})), phase
            log_deferred = sum(counts.get(key, 0) for key in ("deferred", "source_deferred", "h2_8a_deferred", "h2_9_deferred"))
            assert log_deferred == observed_deferred, (phase, log_deferred, observed_deferred)
        profile_summaries.append({"profile": phase, "artifact": path, "candidates": len(cases),
                                  "hosted_job": group, "hosted_exact": observed_exact,
                                  "known_divergences": len(known.get(phase, {})),
                                  "current_deferred": observed_deferred, "migrated_deferred": migrated,
                                  "frozen_future_or_outside_band": len(future) if future is not None else None})

    # Remaining sources, historical repairs and output are added below.
    return finish(sources, jobs, evidence_pin, global_cases, rows, seeds,
                  profile_summaries, row, membership)


def finish(sources, jobs, evidence_pin, global_cases, rows, seeds,
           profile_summaries, row, membership):
    output_candidates = sources.json("ratchets/h2-8a-candidates.v1.json")
    for case in output_candidates["cases"]:
        membership(case["case_id"], "H2.8a-candidate-band", "candidate-not-admission", case["required_slices"])
    old_global = sources.json("ratchets/h2-8a-global-after-a6-37.v1.json")
    failures = defaultdict(list)
    for failure in old_global["failures"]:
        failures[failure["case_id"]].append(failure)
    assert len(failures) == old_global["failed"] == 14
    for case_id, records in sorted(failures.items()):
        assert sorted(f["repetition"] for f in records) == [0, 1]
        membership(case_id, "global-A6-37", "historical-failure", ["H2.8a"])
        row(case_id)["global_failure"] = records
    for case_id in old_global["exact_twice"]:
        row(case_id)["historical_global_exact"] = old_global["revision"]

    causes = sources.json("ratchets/h2-8a-convergence-causes.v1.json")
    for group in causes["global_cause_groups"] + causes["class_cause_groups"]:
        for case_id in group["case_ids"]:
            row(case_id).setdefault("historical_cause_hints", []).append(
                {"id": group.get("cause_id", group.get("cause_group_id")),
                 "title": group["title"]})

    # Repair receipts are dated evidence, not a rerun at BASE.
    rewrite_path = "ratchets/h2-8a-require-rewrite-final.v1.json"
    rewrite = sources.json(rewrite_path)
    assert rewrite["execution"]["actual_exit"] == 0 and rewrite["original_exact_twice"] == 2
    rewrite_test = sources.read("crates/compiler/tests/h2_8a_require_rewrite.rs").decode()
    original_body = rewrite_test.split("fn require_rewrite_original_complete_commands()", 1)[1].split("let exact", 1)[0]
    rewrite_originals = set(re.findall(r'"(typescript-6\.0\.3/[^"\n]+)"', original_body))
    assert len(rewrite_originals) == rewrite["original_exact_twice"]
    assert rewrite_originals == set(next(g["case_ids"] for g in causes["global_cause_groups"] if g["cause_id"] == "G2"))
    for case_id in rewrite_originals:
        row(case_id).setdefault("repair_records", []).append(rewrite_path)
    spec_path = "ratchets/h2-8a-declaration-specifiers-after.v1.json"
    spec = sources.json(spec_path)
    assert spec["actual_exit"] == 0 and spec["original_case"]["exact"] is True
    row(spec["original_case"]["case_id"]).setdefault("repair_records", []).append(spec_path)
    comments_path = "ratchets/h2-8a-declaration-comment-ranges-after.v1.json"
    comments = sources.json(comments_path)
    commands = indexed(comments["stages"][-1]["receipt"]["commands"], "name")
    for test, cause in [("original-g4a", "G4a"), ("original-g4b", "G4b")]:
        assert commands[test]["exit_code"] == 0
        group = next(g for g in causes["global_cause_groups"] if g["cause_id"] == cause)
        for case_id in group["case_ids"]:
            # G4a's shared G5c case explicitly still failed in this receipt.
            if "jsDeclarationsFunctionsCjs.ts" not in case_id:
                row(case_id).setdefault("repair_records", []).append(comments_path + "#final/" + test)
    g5c_path = "docs/design/greenfield/slices/h2-8a-g5c-parameter-integration.md"
    g5c_report = sources.read(g5c_path).decode()
    assert "combined-g5c" in g5c_report and "ac4986f13536a517ab5711e8425993821db8033ec0d00bf808986b3e70a17faa" in g5c_report
    g5c_id = next(g["case_ids"][0] for g in causes["global_cause_groups"] if g["cause_id"] == "G5c")
    row(g5c_id).setdefault("repair_records", []).append(g5c_path + "#combined-g5c")
    property_path = "ratchets/h2-8a-object-property-owners-after.v1.json"
    properties = sources.json(property_path)["original"]
    assert properties["actual_exit"]["exit_code"] == 0 and not properties["failed_twice"]
    assert len(properties["exact_twice"]) == 4
    for case_id in set(properties["exact_twice"]) & set(failures):
        row(case_id).setdefault("repair_records", []).append(property_path + "#original")

    retained_exact = identifiers(jobs["retained"]["observations"], r"retained accessor owners EXACT x2 (\S+)")
    assert len(retained_exact) == 530
    for case_id in retained_exact:
        row(case_id)["hosted_exact"].append({"profile": "retained", "hosted_job": "retained"})
    old_class = sources.json("ratchets/h2-8a-class-convergence-after-a6-34.v1.json")
    old_pins = {p["path"]: p["sha256"] for p in old_class["inputs"]}
    class_failures = set(old_class["failed_once"])
    assert len(class_failures) == 128
    for band in old_class["bands"]:
        fixture = f"crates/compiler/tests/fixtures/{band['fixture']}.json"
        raw = sources.read(fixture)
        assert digest(raw) == old_pins[fixture], f"class fixture changed: {fixture}"
        fixture_ids = indexed(json.loads(raw)["cases"])
        for case_id in band["failed_once"]:
            assert case_id in fixture_ids
            membership(case_id, "class-A6-34", "historical-failure", ["H2.8a"])
            row(case_id)["class_failure"] = {"revision": old_class["revision"], "fixture": fixture,
                                            "fixture_sha256": digest(raw)}
            if case_id in retained_exact:
                row(case_id)["same_fixture_hosted_exact"] = "retained"
    assert len(class_failures & retained_exact) == 88

    direct_path = "crates/emitter/tests/fixtures/decorator-super-direct.json"
    direct = sources.json(direct_path)
    direct_ids = {c["case_id"] for c in direct["cases"] if c["shape"] == "shared-node-used-twice"}
    logged = identifiers(jobs["controls"]["observations"],
                         r"decorator super direct DIVERGENCE RECORDED x2 (\S+) \(memoized required-value lowering; no credit\)")
    assert direct_ids == logged and len(direct_ids) == 4
    for case_id in direct_ids:
        membership(case_id, "SUPER-direct", "recorded-divergence", ["H2.8a-A-RES"])
        row(case_id)["direct_divergence"] = "required-value lowering memoized per node; explicit native-output control, not TS exact"

    retained = sources.json("crates/compiler/tests/fixtures/retained-accessor-owners.json")
    retained_exceptions = {c["case_id"] for c in retained["upstream_failures"]}
    primary_inputs = indexed(sources.json("crates/compiler/tests/fixtures/decorator-super-inputs.json")["cases"])
    for group, pattern in [("primary", r"decorator super UPSTREAM EXCEPTION (\S+)"),
                           ("retained", r"retained accessor owners UPSTREAM EXCEPTION (\S+)")]:
        ids = identifiers(jobs[group]["observations"], pattern)
        assert len(ids) == 2
        if group == "retained":
            assert ids == retained_exceptions
        else:
            exact = identifiers(jobs[group]["observations"], r"decorator super EXACT x2 (\S+)")
            assert len(exact) == 670 and exact.isdisjoint(ids) and exact | ids == set(primary_inputs)
        for case_id in ids:
            membership(case_id, group, "upstream-exception", [])
            row(case_id)["upstream_exceptions"] = group

    parameter_path = "crates/compiler/tests/fixtures/h2-5h-parameter-temporaries.json"
    parameters = indexed(sources.json(parameter_path)["cases"])
    report_path = "docs/design/greenfield/slices/h2-5h-parameter-temporaries-report.md"
    report = sources.read(report_path).decode()
    assert "{comments-lf,comments-crlf,source-map,bom,no-emit-on-error}/es2015" in report
    for name in ("comments-lf", "comments-crlf", "source-map", "bom", "no-emit-on-error"):
        case_id = f"parameter-temporaries/{name}/es2015"
        assert case_id in parameters
        membership(case_id, "parameter-C3", "recorded-strict-failure", ["H2.8a-A-PC1"])
        row(case_id)["parameter_comment_gap"] = report_path

    # Preserve known refusal migration instead of copying the historical option.
    migrated_id = "typescript-6.0.3/compiler/sourceMapWithNonCaseSensitiveFileNames.ts#default"
    row(migrated_id)["current_refused_option"] = "useCaseSensitiveFileNames"
    assert any('"useCaseSensitiveFileNames": 1' in line for line in jobs["late"]["observations"])

    # Pin source consumers used to interpret per-profile logs, without running them.
    for path in ("crates/xtask/src/h2_1a_acceptance.rs", "crates/xtask/src/h2_2c_acceptance.rs",
                 "crates/xtask/src/h2_7c_acceptance.rs", "crates/xtask/src/h2_7de_acceptance.rs",
                 "crates/compiler/tests/integration/h2_7c_corpus.rs",
                 "crates/compiler/tests/integration/h2_7d_original_corpus_shared.rs",
                 "crates/compiler/tests/integration/h2_8a_retained_accessor_owners.rs",
                 "crates/emitter/tests/decorator_super_direct_contract.rs"):
        sources.read(path)
    # Reconcile the complete frozen corpus too: dependency lists alone can omit
    # rows never selected by any current emit runner. Keep those explicitly owned.
    exact_global = {case_id for case_id, value in rows.items()
                    if value["hosted_exact"] and case_id in global_cases}
    closed_h1 = {case_id for case_id, value in global_cases.items()
                 if value["disposition"] == "closed-h1-exact"}
    assert len(closed_h1) == 1
    unrepresented = set(global_cases) - seeds - exact_global - closed_h1
    for case_id in sorted(unrepresented):
        membership(case_id, "global-input-universe", "no-current-hosted-product-record",
                   global_cases[case_id]["required_slices"])
    assert set(global_cases) <= seeds | exact_global | closed_h1
    result = []
    for case_id in sorted(seeds):
        value = row(case_id)
        value["disposition"] = disposition(value)
        value["triage_owner"] = triage_owner(value)
        value["memberships"].sort(key=lambda m: (m["origin"], m["state"]))
        value["hosted_exact"].sort(key=lambda e: e["profile"])
        result.append(value)
    summary = {
        "global_input_universe": len(global_cases),
        "inventory_unique_ids": len(result),
        "membership_count": sum(len(r["memberships"]) for r in result),
        "global_ids_in_inventory": len(set(global_cases) & seeds),
        "global_ids_with_hosted_exact_profile": len(exact_global),
        "global_ids_added_outside_profile_queues": len(unrepresented),
        "closed_h1_ids": sorted(closed_h1),
        "global_ids_without_a_disposition": 0,
        "dispositions": dict(sorted(Counter(r["disposition"] for r in result).items())),
        "triage_owners": dict(sorted(Counter(r["triage_owner"] for r in result).items())),
        "known_divergence_memberships": 21, "known_divergence_unique_ids": 20,
        "old_global_failures": 14,
        "old_global_with_later_repair_record": sum(bool(rows[c].get("repair_records")) for c in failures),
        "old_class_failures": 128, "same_fixture_hosted_repaired": 88,
        "old_class_without_current_replay": 40,
        "parameter_comment_recorded_gaps": 5, "direct_divergences": 4,
        "upstream_exceptions_no_credit": 4,
    }
    return {"schema": 1, "kind": "planning-crosswalk-not-qualification", "base_revision": BASE,
            "semantics": "TypeScript 6.0.3", "new_native_executions": 0,
            "claim": "Membership and evidence inventory only. An exact result belongs to its listed profile; cross-profile input/observation equivalence is not inferred.",
            "evidence": evidence_pin, "inputs": [sources.pins[p] for p in sorted(sources.pins)],
            "summary": summary, "profiles": profile_summaries, "cases": result}


def triage_owner(row):
    if row.get("upstream_exceptions"):
        return "upstream-exception-registry"
    if row.get("parameter_comment_gap"):
        return "H2.8a-A-PC1"
    if row.get("current_refused_option"):
        return "H2.8b-SYS1"
    if any(d.get("refused_option") == "isolatedModules" for d in row.get("known_divergences", {}).values()):
        return "H2.8c-MOD1"
    if row.get("class_failure") or row.get("global_failure") or row.get("known_divergences") or row.get("direct_divergence"):
        return "H2.8a-A-RES"
    # A historical dependency is a routing hint. A later profile's exact result
    # remains explicit, and H2.9-INV owns any cross-profile closure decision.
    candidates = set()
    for member in row["memberships"]:
        if member["state"] in ("source-deferred", "candidate-not-admission"):
            candidates.update(member["owners"])
    for owner in ("H2.8c", "H2.8b", "H2.8d", "H2.8a", "H2.9"):
        if owner in candidates and not row["hosted_exact"]:
            return owner
    return "H2.9-INV"


def render(data):
    # One stable case per line keeps this planning artifact compact and reviewable.
    prefix = {k: v for k, v in data.items() if k != "cases"}
    header = json.dumps(prefix, indent=2, ensure_ascii=False)[:-2]
    cases = [json.dumps(row, ensure_ascii=False, separators=(",", ":")) for row in data["cases"]]
    return header + ',\n  "cases": [\n    ' + ',\n    '.join(cases) + '\n  ]\n}\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="compare with the committed dated snapshot")
    mode.add_argument("--list", action="store_true", help="query saved IDs without regenerating")
    parser.add_argument("--case", help="case ID substring; requires --list")
    parser.add_argument("--disposition", help="exact disposition name; requires --list")
    args = parser.parse_args()
    path = HERE / "inventory.v1.json"
    if (args.case is not None or args.disposition is not None) and not args.list:
        parser.error("--case and --disposition require --list")
    if args.list:
        data = json.loads(path.read_text())
        selected = [r for r in data["cases"]
                    if (args.case is None or args.case in r["case_id"])
                    and (args.disposition is None or args.disposition == r["disposition"])]
        if not selected:
            parser.error("selection matched no inventory IDs")
        print(f"Dated base {data['base_revision']}; selected {len(selected)} IDs; no replay")
        for value in selected:
            print(f"{value['case_id']}\t{value['disposition']}\t{value['triage_owner']}")
        return
    data = build()
    output = render(data)
    if args.check:
        if not path.exists() or path.read_text() != output:
            raise SystemExit("PLAN-BASE inventory differs; investigate before regenerating")
    else:
        path.write_text(output)
    print(json.dumps(data["summary"], indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
