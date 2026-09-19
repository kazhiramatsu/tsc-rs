#!/usr/bin/env python3
"""Compare complete selected-corpus commands only after exact input/option checks."""
import argparse
import hashlib
import json
from pathlib import Path


def load(path):
    data = path.read_bytes()
    return json.loads(data), hashlib.sha256(data).hexdigest()


def compare(selection, selection_sha, native, oracle):
    assert native["schema"] == oracle["schema"] == 1
    assert native["kind"] == "emitter-recovery-native-observations"
    assert oracle["kind"] == "emitter-recovery-typescript-observations"
    assert native["selection_sha256"] == oracle["selection_sha256"] == selection_sha
    assert native["input_workspace"] == oracle["input_workspace"]
    assert native["library_root"] == oracle["library_root"]
    assert native["load_failures"] == oracle["load_failures"] == selection["load_failures"]
    assert native["repetitions"] == oracle["repetitions"] == 2
    if "successor" in selection:
        assert native["syntax_tree_hash"] == selection["successor"]["syntax_tree_hash"], "native observations used a different successor parser"
        assert native["successor_source_files_sha256"] == selection["successor"]["source_files_sha256"], "native successor dependency pins differ"
    indices = []
    for artifact in [selection, native, oracle]:
        index = {row["case_id"]: row for row in artifact["cases"]}
        assert len(index) == len(artifact["cases"]), "duplicate selected observation"
        indices.append(index)
    rows, actuals, expecteds = indices
    assert rows.keys() == actuals.keys() == expecteds.keys(), "selected corpus coverage differs"
    cases, counts = [], {}
    for id, row in rows.items():
        actual, expected = actuals[id], expecteds[id]
        # serde_json and these protocol fields use the same Unicode JSON
        # escaping, sorted object keys, and integer-valued numeric fields.
        input_sha = hashlib.sha256(json.dumps(row["command_input"], ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
        assert actual["input_sha256"] == expected["input_sha256"] == input_sha, id
        if "typescript_repetition_error" in expected:
            disposition = "typescript-repetition-mismatch; emit-not-qualified"
            assert expected["disposition"] == disposition, id
            assert len(expected["observations"]) == 2 and expected["observations"][0] != expected["observations"][1], id
            counts[disposition] = counts.get(disposition, 0) + 1
            cases.append({"case_id": id, "loader": row["loader"], "reasons": row["reasons"],
                          "disposition": disposition, "typescript_observations": expected["observations"]})
            continue
        if "input_reconstruction_error" in expected:
            assert expected["disposition"] == "input-reconstruction-mismatch; emit-not-qualified", id
            assert expected["complete_command_runs"] == [], id
            assert isinstance(expected["input_reconstruction_error"], str) and expected["input_reconstruction_error"], id
            disposition = "input-reconstruction-mismatch; emit-not-qualified"
            counts[disposition] = counts.get(disposition, 0) + 1
            cases.append({"case_id": id, "loader": row["loader"], "reasons": row["reasons"],
                          "disposition": disposition, "typescript_input_error": expected["input_reconstruction_error"]})
            continue
        for observation in [actual, expected]:
            assert len(observation["complete_command_runs"]) == 2, id
            assert observation["complete_command_runs"][0] == observation["complete_command_runs"][1], id
        a, e = actual["complete_command_runs"][0], expected["complete_command_runs"][0]
        fallback = row["loader"] in ["load_compiler_no_emit", "load_project_no_emit"]
        if fallback:
            assert actual["disposition"] == expected["disposition"] == "parse-admission-only; emit-not-qualified", id
        if actual["options"] != expected["options"]:
            disposition = "input-option-mismatch; emit-not-qualified"
            details = {"native_options": actual["options"], "typescript_options": expected["options"]}
        elif "observer_unsupported" in a or "observer_unsupported" in e:
            disposition = "observer-unsupported; emit-not-qualified"
            details = {"native": a, "typescript": e}
        elif "production_error" in a:
            disposition = "production-refusal; emit-not-qualified"
            details = {"native": a}
        elif a != e:
            disposition = "complete-command-mismatch; emit-not-qualified"
            details = {"native": a, "typescript": e}
        else:
            disposition = "no-emit-command-exact; emit-not-qualified" if fallback else "complete-command-exact"
            details = {}
        counts[disposition] = counts.get(disposition, 0) + 1
        cases.append({"case_id": id, "loader": row["loader"], "reasons": row["reasons"],
                      "disposition": disposition, **details})
    return {"schema": 1, "kind": "emitter-recovery-complete-command-comparison", "selection_sha256": selection_sha,
            "census_head": selection["head"], "native_head": native["head"], "oracle_head": oracle["head"],
            "summary": {"selected": len(rows), "dispositions": counts, "unloaded": len(selection["load_failures"])},
            "load_failures": selection["load_failures"], "cases": cases}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["selection", "native", "oracle", "out"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    selection, selection_sha = load(args.selection)
    native, native_sha = load(args.native)
    oracle, oracle_sha = load(args.oracle)
    result = compare(selection, selection_sha, native, oracle)
    result["native_sha256"] = native_sha
    result["oracle_sha256"] = oracle_sha
    result["comparator_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("x") as file:
        json.dump(result, file, ensure_ascii=False, separators=(",", ":"))
        file.write("\n")
    print(json.dumps(result["summary"]))
    raise SystemExit(any(case["disposition"] not in ["complete-command-exact", "no-emit-command-exact; emit-not-qualified"] for case in result["cases"]))


if __name__ == "__main__":
    main()
