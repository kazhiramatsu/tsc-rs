#!/usr/bin/env python3
"""Compare separate project supplement artifacts without changing census proof."""
import argparse
import hashlib
import json
from pathlib import Path


def validate_roster(native, roster_bytes):
    roster = json.loads(roster_bytes)
    assert roster["schema"] == 1 and roster["kind"] == "emitter-project-projection-roster"
    assert native["roster_sha256"] == hashlib.sha256(roster_bytes).hexdigest(), "project roster pin differs"
    assert native["case_ids"] == roster["case_ids"], "project roster coverage differs"


def compare(native, native_sha, oracle):
    assert native["schema"] == oracle["schema"] == 1
    assert native["kind"] == "emitter-project-projection-native"
    assert oracle["kind"] == "emitter-project-projection-typescript"
    assert oracle["native_sha256"] == native_sha
    for key in ["head", "input_head", "roster_sha256", "compiler_sha256", "library_root", "input_workspace"]:
        assert native[key] == oracle[key], key
    assert native["repetitions"] == oracle["repetitions"] == 2
    ids = native["case_ids"]
    assert ids and len(ids) == len(set(ids))
    indices = []
    for artifact in [native, oracle]:
        index = {row["case_id"]: row for row in artifact["cases"]}
        assert len(index) == len(artifact["cases"]) and set(index) == set(ids), "project coverage differs"
        indices.append(index)
    cases, counts = [], {}
    for id in ids:
        a, e = (index[id] for index in indices)
        if "load_error" in a:
            assert a["disposition"] == "not-loaded; emit-not-qualified" and a["load_error"]
            assert e.get("disposition") == "native-input-unavailable; emit-not-qualified"
            disposition = "not-loaded; emit-not-qualified"
        else:
            assert a["row"]["case_id"] == id and a["row"]["loader"] == "load_project_emit"
            raw = json.dumps(a["row"]["command_input"], ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
            assert a["input_sha256"] == e["input_sha256"] == hashlib.sha256(raw).hexdigest(), id
            if "input_reconstruction_error" in e or "typescript_repetition_error" in e:
                disposition = e["disposition"]
                assert disposition in ["input-reconstruction-mismatch; emit-not-qualified", "typescript-repetition-mismatch; emit-not-qualified"]
            elif any(not isinstance(obs.get("options"), dict) or not isinstance(obs.get("complete_command_runs"), list) for obs in [a, e]):
                disposition = "command-observation-invalid; emit-not-qualified"
            else:
                for observation in [a, e]:
                    assert len(observation["complete_command_runs"]) == 2, id
                    assert observation["complete_command_runs"][0] == observation["complete_command_runs"][1], id
                ac, ec = a["complete_command_runs"][0], e["complete_command_runs"][0]
                if not isinstance(ac, dict) or not isinstance(ec, dict):
                    disposition = "command-observation-invalid; emit-not-qualified"
                elif a["options"] != e["options"]:
                    disposition = "input-option-mismatch; emit-not-qualified"
                elif "observer_unsupported" in ac or "observer_unsupported" in ec:
                    disposition = "observer-unsupported; emit-not-qualified"
                elif "production_error" in ac:
                    disposition = "production-refusal; emit-not-qualified"
                elif any(not {"writes", "reported_diagnostics", "status_writes", "exit_code", "emit_result"} <= command.keys() for command in [ac, ec]):
                    disposition = "command-observation-invalid; emit-not-qualified"
                elif ac != ec:
                    disposition = "complete-command-mismatch; emit-not-qualified"
                else:
                    disposition = "complete-command-exact"
        counts[disposition] = counts.get(disposition, 0) + 1
        row = {"case_id": id, "disposition": disposition}
        if disposition != "complete-command-exact":
            row.update(native=a, typescript=e)
        cases.append(row)
    return {"schema": 1, "kind": "emitter-project-projection-comparison", "head": native["head"],
            "native_sha256": native_sha, "roster_sha256": native["roster_sha256"],
            "summary": {"selected": len(ids), "dispositions": counts}, "cases": cases}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["native", "oracle", "out"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    native, oracle = args.native.read_bytes(), args.oracle.read_bytes()
    parsed_native = json.loads(native)
    roster_path = Path(__file__).resolve().parents[1] / "docs/design/greenfield/slices/emitter-final-batch/integration/records/project-projection-roster-r139.json"
    validate_roster(parsed_native, roster_path.read_bytes())
    assert len(parsed_native["case_ids"]) == 108, "the original project load-failure set has 108 IDs"
    result = compare(parsed_native, hashlib.sha256(native).hexdigest(), json.loads(oracle))
    result["oracle_sha256"] = hashlib.sha256(oracle).hexdigest()
    result["comparator_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("x") as file:
        json.dump(result, file, ensure_ascii=False, separators=(",", ":")); file.write("\n")
    print(json.dumps(result["summary"]))
    raise SystemExit(any(row["disposition"] != "complete-command-exact" for row in result["cases"]))


if __name__ == "__main__":
    main()
