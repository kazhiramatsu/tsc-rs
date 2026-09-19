#!/usr/bin/env python3
"""Select the union of parser changes and all five recovery-admission deltas."""
import argparse
import hashlib
import json
from pathlib import Path

PROFILES = ["missing-await", "missing-declaration", "parameter-gaps", "statement-gaps", "context-recovery"]
PROFILE_KEYS = {"literal", "missing_await", "missing_declaration", "parameter_gaps", "statement_gaps", "context_recovery"}
# The reviewed nested-parenthesis patch changes predicates and their tests.
# Changes to event production or another parser owner need a separate proof.
SUCCESSOR_SOURCE_PATHS = {"crates/syntax/src/recovery.rs", "crates/syntax/tests/unit/parser/recovery.rs"}
# Reviewed context spelling and additive consumed-keyword parser reporting.
# The independent extension proof must preserve every prior recovery fact.
# Each exception fixes both full file identities; neither admits arbitrary edits.
SUCCESSOR_STYLE_SOURCE_PAIRS = {
    "crates/syntax/src/recovery/context.rs": (
        "780fbb2dba7ab8d686bb5e37792be81c32afce2943fdfc46e6fd32a5929ea30f",
        "b7e382ff51a2193fd23070b667473f41128d9aef32fec541a94a1e4c3a54724e",
    ),
    "crates/syntax/src/parser.rs": (
        "ffe64e0cf029c96b3bc91239a71be2c918f2e6c7803c57f189c0a771ab8f6701",
        "7cbc667b37ec7ac796ef086d1abac8a26b94e680c8a165bd789e2e77d7c4e07d",
    ),
}


def successor_source_changes(before, after):
    changed = {path for path in before.keys() | after.keys() if before.get(path) != after.get(path)}
    unreviewed = changed - SUCCESSOR_SOURCE_PATHS
    for path, pair in SUCCESSOR_STYLE_SOURCE_PAIRS.items():
        if (before.get(path), after.get(path)) == pair:
            unreviewed.discard(path)
    assert not unreviewed, f"successor source changed outside reviewed predicates: {sorted(unreviewed)}"
    return sorted(changed)


def validate_keyword_extension(current, successor, ids):
    assert successor["recovery_extension_format"] == "escaped-keyword-consumed-v1"
    assert successor["legacy_recovery_facts_sha256"] == current["recovery_facts_sha256"], "prior events/origins/actions changed"
    assert set(successor["escaped_keyword_actions"]) == ids, "keyword facts omitted/added inputs"
    assert set(successor["legacy_recovery_facts_sha256"]) == ids
    def relative_probes(artifact):
        probes = artifact["build"]["probe_files_sha256"]
        result = {}
        for path, digest in probes.items():
            prefixes = [p for p in ("/scripts/", "/crates/") if p in path]
            assert len(prefixes) == 1, path
            relative = prefixes[0][1:] + path.split(prefixes[0], 1)[1]
            assert relative not in result
            result[relative] = digest
        return result
    before, after = relative_probes(current), relative_probes(successor)
    assert set(before) == set(after)
    # Only the new replay/guard scripts may change; the parse-graph digest code
    # is still the exact producer used by the immutable original census.
    allowed = {"scripts/replay-recovery-parse.rs", "scripts/replay-recovery-parse.py", "scripts/select-recovery-parse-corpus.py"}
    assert {p for p in before if before[p] != after[p]} <= allowed
    for id in ids:
        actions = successor["escaped_keyword_actions"][id]
        assert isinstance(actions, list)
        seen = {}
        for action in actions:
            assert set(action) == {"token", "start", "length", "statement_start", "matching_report_events"}
            assert all(type(value) is int and value >= 0 for value in action.values())
            assert action["length"] > 0 and action["statement_start"] <= action["start"]
            key = (action["start"], action["length"])
            seen[key] = seen.get(key, 0) + 1
            assert seen[key] == action["matching_report_events"] == 1, "duplicate unreported keyword fact"
        assert (successor["recovery_facts_sha256"][id] != successor["legacy_recovery_facts_sha256"][id]) == bool(actions), "raw/legacy recovery fact accounting differs"


def successor_changes(snapshot, snapshot_sha, current, successor):
    ids = set(snapshot["inputs"])
    assert successor["schema"] == 1 and successor["kind"] == "emitter-recovery-parse-replay"
    assert successor["input_artifact_sha256"] == snapshot_sha, "successor snapshot identity differs"
    assert successor["digest_code_sha256"] == snapshot["digest_code_sha256"]
    assert set(successor["digests"]) == ids, "successor omitted/added parse inputs"
    assert successor["build"]["baseline_kind"] == "successor"
    assert current["build"]["parser_head"] == snapshot["head"], "current replay is not the frozen census head"
    assert current["build"]["syntax_tree_hash"] == snapshot["syntax_tree_hash"], "current parser tree differs from census"
    assert successor["build"]["reference_source_files_sha256"] == current["build"]["source_files_sha256"]
    changed_sources = successor_source_changes(current["build"]["source_files_sha256"], successor["build"]["source_files_sha256"])
    assert changed_sources == successor["build"]["changed_source_paths"]
    if successor.get("recovery_extension_format") == "escaped-keyword-consumed-v1":
        validate_keyword_extension(current, successor, ids)
    else:
        assert current["build"]["probe_files_sha256"] == successor["build"]["probe_files_sha256"], "successor recovery digest producer differs"
    assert current["recovery_facts_format"] == successor["recovery_facts_format"] == "rust-debug-ParseRecovery-v1"
    for replay in [current, successor]:
        assert set(replay["recovery_facts_sha256"]) == ids, "recovery facts omitted/added parse inputs"
        assert all(isinstance(value, str) and len(value) == 64 and all(c in "0123456789abcdef" for c in value)
                   for value in replay["recovery_facts_sha256"].values()), "invalid recovery facts digest"
    if "recovery_extension_format" not in successor:
        assert current["recovery_facts_sha256"] == successor["recovery_facts_sha256"], "successor changed committed recovery facts"
    changed = {}
    for id in sorted(ids):
        before, after = current["digests"][id], successor["digests"][id]
        assert after["core"] == snapshot["digests"][id]["core"], f"successor changed parse core: {id}"
        for flags in [before["profiles"], after["profiles"]]:
            assert isinstance(flags, dict) and set(flags) == PROFILE_KEYS and all(type(value) is bool for value in flags.values()), "successor requires all six boolean profiles"
        for key in PROFILE_KEYS:
            if key not in {"statement_gaps", "context_recovery"}:
                assert before["profiles"][key] == after["profiles"][key], f"successor spread to {key}: {id}"
            assert not before["profiles"][key] or after["profiles"][key], f"successor removed admission: {id}/{key}"
        if before["profiles"] != after["profiles"]:
            changed[id] = {"before": before["profiles"], "after": after["profiles"]}
    return changed


def load(path):
    data = path.read_bytes()
    return json.loads(data), hashlib.sha256(data).hexdigest()


def validate_input_numbers(value):
    if isinstance(value, dict):
        for child in value.values(): validate_input_numbers(child)
    elif isinstance(value, list):
        for child in value: validate_input_numbers(child)
    elif isinstance(value, (int, float)) and not isinstance(value, bool):
        assert isinstance(value, int) and abs(value) <= 9007199254740991, "command input numbers must be safe integers"


def select(snapshot, snapshot_sha, current, baselines, reports, successor=None):
    assert snapshot["schema"] == 1 and snapshot["kind"] == "emitter-recovery-parse-snapshot"
    ids = set(snapshot["inputs"])
    assert ids == set(snapshot["digests"])
    for replay in [current, *baselines.values()]:
        assert replay["schema"] == 1 and replay["kind"] == "emitter-recovery-parse-replay"
        assert replay["input_artifact_sha256"] == snapshot_sha
        assert replay["digest_code_sha256"] == snapshot["digest_code_sha256"]
        assert set(replay["digests"]) == ids, "historical replay omitted/added parse inputs"
    assert current["build"]["baseline_kind"] == "candidate"
    for baseline, replay in baselines.items():
        assert replay["build"]["baseline_kind"] == baseline
    for id in ids:
        recorded = snapshot["digests"][id]
        assert current["digests"][id] == {"core": recorded["core"], "profiles": recorded["profiles"]}, f"current replay differs from actual census input {id}"
    rows = {row["case_id"]: row for row in snapshot["rows"]}
    assert len(rows) == len(snapshot["rows"]), "duplicate census case ID"
    reasons = {id: [] for id in rows}
    changed_inputs = {}
    for baseline, replay in baselines.items():
        changed = {}
        for id in sorted(ids):
            before, after = replay["digests"][id], current["digests"][id]
            differences = []
            if before["core"] != after["core"]:
                differences.append("core")
            if before["profiles"] is not None and before["profiles"] != after["profiles"]:
                differences.append("profiles")
            if differences:
                changed[id] = differences
        changed_inputs[baseline] = changed
        for id, row in rows.items():
            for unit in row["units"]:
                input_id = unit["input_id"]
                assert input_id in ids
                if input_id in changed:
                    reasons[id].append({"baseline": baseline, "path": unit["path"], "role": unit["role"], "input_id": input_id, "changed": changed[input_id]})
    for name in PROFILES:
        report = reports[name]
        assert report["schema"] == 1 and report["head"] == snapshot["head"]
        assert report["inputs"] == snapshot["input_manifest"]
        assert report["summary"]["rows"] == len(rows)
        assert report["load_failures"] == snapshot["load_failures"]
        for verdict in ["newly_admitted", "newly_refused"]:
            for row in report[verdict]:
                id = row["case_id"]
                assert id in rows and row["universe"] == rows[id]["universe"]
                assert row["loader"] == rows[id]["loader"]
                reasons[id].append({"profile": name, "verdict": verdict})
    if successor is not None:
        changes = successor_changes(snapshot, snapshot_sha, current, successor)
        for id, row in rows.items():
            for unit in row["units"]:
                input_id = unit["input_id"]
                if input_id in changes:
                    reasons[id].append({"successor": successor["build"]["parser_head"],
                        "input_id": input_id, "path": unit["path"], "role": unit["role"], **changes[input_id]})
    cases = []
    documents = {}

    def collect_documents(value):
        if isinstance(value, dict):
            for name, child in value.items():
                if name == "content_sha256" and child is not None:
                    documents[child] = snapshot["documents"][child]
                else:
                    collect_documents(child)
        elif isinstance(value, list):
            for child in value:
                collect_documents(child)

    for id, row in rows.items():
        if not reasons[id]:
            continue
        if row["loader"] in ["load_compiler_no_emit", "load_project_no_emit"]:
            assert row["emit_load_error"] and row["emit_disposition"] == "parse-admission-only; emit-not-qualified", f"fallback row lost emit refusal: {id}"
        else:
            assert row["emit_load_error"] is None and row["emit_disposition"] == "pending-complete-command-comparison", f"emit row carried a fallback disposition: {id}"
        assert row["command_input"] is not None, f"selected row has no exact loader input: {id}"
        validate_input_numbers(row["command_input"])
        collect_documents(row["command_input"])
        cases.append({**row, "reasons": reasons[id]})
    result = {"schema": 1, "kind": "emitter-recovery-corpus-selection", "head": snapshot["head"],
            "snapshot_sha256": snapshot_sha, "digest_code_sha256": snapshot["digest_code_sha256"],
            "syntax_tree_hash": snapshot["syntax_tree_hash"], "vendor_tree_hash": snapshot["vendor_tree_hash"],
            "plan_manifest_sha256": snapshot["plan_manifest_sha256"],
            "input_manifest": snapshot["input_manifest"],
            "summary": {"loaded_rows": len(rows), "selected_rows": len(cases), "unchanged_rows": len(rows) - len(cases),
                        "load_failures": len(snapshot["load_failures"]),
                        "selected_no_emit_fallbacks": sum(row["loader"] in ["load_compiler_no_emit", "load_project_no_emit"] for row in cases), "changed_inputs": {k: len(v) for k, v in changed_inputs.items()}},
            # Failures remain explicit: this artifact never claims complete
            # qualification until each failed load has a separate disposition.
            "load_failures": snapshot["load_failures"], "changed_inputs": changed_inputs,
            "documents": documents, "cases": cases}
    if successor is not None:
        result["summary"]["successor_changed_inputs"] = len(changes)
        result["successor"] = {key: successor["build"][key] for key in
            ["parser_head", "syntax_tree_hash", "source_files_sha256", "binary_sha256", "predicate_diff_sha256"]}
    return result


def compare_previous_successor(previous, successor):
    assert previous["input_artifact_sha256"] == successor["input_artifact_sha256"]
    assert previous["digest_code_sha256"] == successor["digest_code_sha256"]
    assert previous["recovery_facts_format"] == successor["recovery_facts_format"] == "rust-debug-ParseRecovery-v1"
    assert previous["recovery_facts_sha256"] == successor["legacy_recovery_facts_sha256"]
    assert set(previous["digests"]) == set(successor["digests"])
    assert set(successor["retained_statement_terminator_reports"]) == set(previous["digests"])
    groups = {"with_consumed_keyword_fact": [], "without_consumed_keyword_fact": []}
    for id, before in previous["digests"].items():
        after = successor["digests"][id]
        assert before["core"] == after["core"], f"previous parse core changed: {id}"
        for key in PROFILE_KEYS:
            assert type(before["profiles"][key]) is bool and type(after["profiles"][key]) is bool
            if key != "context_recovery":
                assert before["profiles"][key] == after["profiles"][key], f"changed non-context profile: {id}/{key}"
            else:
                assert not before["profiles"][key] or after["profiles"][key], f"lost prior admission: {id}"
        if before["profiles"] != after["profiles"]:
            if successor["escaped_keyword_actions"][id]:
                group = "with_consumed_keyword_fact"
            else:
                assert successor["retained_statement_terminator_reports"][id], f"unclassified new admission: {id}"
                group = "without_consumed_keyword_fact"
            groups[group].append(id)
    return groups


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ["snapshot", "current", "projection", "merge-base", "profiles-dir", "out"]:
        p.add_argument("--" + name, type=Path, required=True)
    p.add_argument("--successor", type=Path)
    p.add_argument("--previous-successor", type=Path)
    args = p.parse_args()
    snapshot, snapshot_sha = load(args.snapshot)
    current, current_sha = load(args.current)
    projection, projection_sha = load(args.projection)
    merge, merge_sha = load(args.merge_base)
    reports, hashes = {}, {}
    for name in PROFILES:
        reports[name], hashes[name] = load(args.profiles_dir / (name + ".json"))
    successor, successor_sha = load(args.successor) if args.successor else (None, None)
    result = select(snapshot, snapshot_sha, current, {"projection": projection, "merge-base": merge}, reports, successor)
    result["evidence"] = {"current_sha256": current_sha, "projection_sha256": projection_sha,
                          "merge_base_sha256": merge_sha, "profiles_sha256": hashes,
                          "selector_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    if successor is not None:
        result["evidence"]["successor_sha256"] = successor_sha
        if successor.get("recovery_extension_format") == "escaped-keyword-consumed-v1":
            assert args.previous_successor is not None, "keyword extension requires the prior qualified parser replay"
            previous, previous_sha = load(args.previous_successor)
            result["keyword_extension"] = compare_previous_successor(previous, successor)
            result["evidence"]["previous_successor_sha256"] = previous_sha
            result["summary"]["keyword_extension_changed_inputs"] = {key: len(value) for key, value in result["keyword_extension"].items()}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("x") as file:
        json.dump(result, file, ensure_ascii=False, separators=(",", ":"))
        file.write("\n")
    print(json.dumps(result["summary"]))


if __name__ == "__main__":
    main()
