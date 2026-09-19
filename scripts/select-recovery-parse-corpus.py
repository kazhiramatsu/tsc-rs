#!/usr/bin/env python3
"""Select the union of parser changes and all five recovery-admission deltas."""
import argparse
import hashlib
import json
from pathlib import Path

PROFILES = ["missing-await", "missing-declaration", "parameter-gaps", "statement-gaps", "context-recovery"]


def load(path):
    data = path.read_bytes()
    return json.loads(data), hashlib.sha256(data).hexdigest()


def select(snapshot, snapshot_sha, current, baselines, reports):
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
        assert row["command_input"] is not None, f"selected row has no exact loader input: {id}"
        collect_documents(row["command_input"])
        cases.append({**row, "reasons": reasons[id]})
    return {"schema": 1, "kind": "emitter-recovery-corpus-selection", "head": snapshot["head"],
            "snapshot_sha256": snapshot_sha, "digest_code_sha256": snapshot["digest_code_sha256"],
            "summary": {"loaded_rows": len(rows), "selected_rows": len(cases), "unchanged_rows": len(rows) - len(cases),
                        "load_failures": len(snapshot["load_failures"]),
                        "selected_no_emit_fallbacks": sum(row["loader"] in ["load_compiler_no_emit", "load_project_no_emit"] for row in cases), "changed_inputs": {k: len(v) for k, v in changed_inputs.items()}},
            # Failures remain explicit: this artifact never claims complete
            # qualification until each failed load has a separate disposition.
            "load_failures": snapshot["load_failures"], "changed_inputs": changed_inputs,
            "documents": documents, "cases": cases}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ["snapshot", "current", "projection", "merge-base", "profiles-dir", "out"]:
        p.add_argument("--" + name, type=Path, required=True)
    args = p.parse_args()
    snapshot, snapshot_sha = load(args.snapshot)
    current, current_sha = load(args.current)
    projection, projection_sha = load(args.projection)
    merge, merge_sha = load(args.merge_base)
    reports, hashes = {}, {}
    for name in PROFILES:
        reports[name], hashes[name] = load(args.profiles_dir / (name + ".json"))
    result = select(snapshot, snapshot_sha, current, {"projection": projection, "merge-base": merge}, reports)
    result["evidence"] = {"current_sha256": current_sha, "projection_sha256": projection_sha,
                          "merge_base_sha256": merge_sha, "profiles_sha256": hashes,
                          "selector_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("x") as file:
        json.dump(result, file, ensure_ascii=False, separators=(",", ":"))
        file.write("\n")
    print(json.dumps(result["summary"]))


if __name__ == "__main__":
    main()
