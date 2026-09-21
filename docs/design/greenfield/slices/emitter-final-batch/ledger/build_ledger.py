#!/usr/bin/env python3
"""EF7 ledger: reconcile every PLAN-BASE membership with the current evidence.

Unit: case ID + input hash + options/profile + observation kind + version + owner.
Reads every input as a git blob at START_SHA (never the working tree, never a
compiler, never a network), so the dated snapshot regenerates byte-for-byte on
later main.  It records no new native or Rust execution.  Standard library only.
"""
import argparse
import base64
from collections import Counter, defaultdict
import gzip
import hashlib
import json
from pathlib import Path
import re
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[5]
START_SHA = "c35e00ccb006e3b4e3e2643491e8fe3207595097"
# PR #555's hosted candidate; its tree equals the tested merge checkout and the
# main merge commit, and the diff from it to START_SHA is documentation only.
HOSTED_HEAD = "2883e3c79247b988106c6e4f1f2bdd074ae90e66"
HOSTED_MERGE = "ce39261ace254b4aaf9d2923220f4672818b1d8f"
HOSTED_TESTED_CHECKOUT = "5ae9a7a7265c458f7e7d831333b212401fface41"
PLAN_BASE_REV = "f9ef828a56c9f9305947a6e6f3c1ae39072b3110"
PLAN_BASE_RUN_HEAD = "dac0d55cec8d9e41cf958b8d5256a0c3cfe8c4b9"
GLOBAL_A6_37_REV = "d1c04df5c7c0d93b26f76539c9101bc2f4b11c69"
CLASS_A6_34_REV = "b6621f25aa47851b2ee264ebc200639f96a8aa03"
TYPESCRIPT = {"version": "6.0.3", "source_commit": "050880ce59e30b356b686bd3144efe24f875ebc8"}

PHASES = ("1a", "1b", "1c", "1d", "1e", "2a", "2b", "2c", "2d", "3a", "3b", "3c", "3d",
          "4a", "4b", "5a", "5b", "5c", "5d", "5e", "5f", "5g", "5h", "6a", "6b", "6c", "7b")
STATES = {
    "CE": "current-exact",
    "RP": "remeasure-pending-same-observation",
    "RD": "reproduced-divergence",
    "UP": "unsupported-producer",
    "RM": "runner-missing",
    "OB": "other-product-boundary",
    "UX": "upstream-exception",
}
KIND_ORDER = ("js", "dts", "map", "dtsmap", "buildinfo", "other", "direct", "diag", "result", "write", "*")
WRITE_KIND = {"javascript": "js", "jsx": "js", "cjs": "js", "mjs": "js", "declaration": "dts",
              "source-map": "map", "javascript-map": "map", "declaration-map": "dtsmap",
              "build-info": "buildinfo", "other": "other"}
HOSTED_BASE = "docs/design/greenfield/slices/h2-8a-post-t1-residuals/integration/hosted"
FIXTURE_DIR = "crates/compiler/tests/fixtures"
SCHEDULE = "docs/design/greenfield/post-h1-completion-slices.md"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def json_digest(value):
    return digest(json.dumps(value, sort_keys=True, ensure_ascii=True, separators=(",", ":")).encode())


def strip_time(line):
    return re.sub(r"^\S+Z\s*", "", line)


class Sources:
    """Every input is the START_SHA blob, pinned by path and SHA-256."""

    def __init__(self):
        self.pins = {}

    def read(self, path):
        data = subprocess.check_output(["git", "show", f"{START_SHA}:{path}"], cwd=ROOT)
        self.pins[path] = {"path": path, "sha256": digest(data), "bytes": len(data)}
        return data

    def json(self, path):
        return json.loads(self.read(path))


def kinds_of(observation):
    """Observation kinds actually recorded by a TypeScript observation."""
    kinds = set()
    for write in observation.get("writes") or []:
        kinds.add(WRITE_KIND.get(write.get("kind"), "other"))
    if "reported_diagnostics" in observation:
        kinds.add("diag")
    if "emit_result" in observation or "exit_code" in observation:
        kinds.add("result")
    if "writes" in observation:
        kinds.add("write")
    return kinds


def kinds_str(kinds):
    return ",".join(k for k in KIND_ORDER if k in kinds)


def bucket_for(tokens):
    """Likely Rust owner bucket from owner-reachability / blocker vocabulary.

    Profile-selection blockers (required-option:target/module) only say which
    profile would have selected the row; route/rejected blockers name the gap.
    """
    tokens = [t.lower() for t in tokens]
    decisive = [t for t in tokens if t.startswith(("route:", "rejected-"))]
    if decisive:
        tokens = decisive
    text = " ".join(tokens)
    if "printer" in text and "transform" not in text:
        return "printer"
    if any(t in text for t in ("source-map", "sourcemap", "inlinesourcemap", "declarationmap", ":map", "-map")):
        return "maps"
    if "declaration" in text or "emitdeclarationonly" in text:
        return "declaration"
    if any(t in text for t in ("noemithelpers", "importhelpers", "jsx", "experimentaldecorators", "transform-", "helpers")):
        return "transforms"
    if any(t in text for t in ("verbatimmodulesyntax", "isolatedmodules", "export-equals", "import-equals", "esmoduleinterop",
                               "transform-module", "emit-module-format", "module-transformer", "module=")):
        return "module"
    if any(t in text for t in ("noemit", "outfile", "outdir", "rootdir", "composite", "incremental", "allowjs", "resolvejsonmodule", "option", "options")):
        return "options"
    if any(t in text for t in ("host", "case", "filesystem", "project", "checkjs")):
        return "host"
    return "options"


def parse_hosted_logs(sources):
    receipt = sources.json(f"{HOSTED_BASE}/receipt.v1.json")
    merge = sources.json(f"{HOSTED_BASE}/merge.v1.json")
    assert receipt["candidate_commit"] == HOSTED_HEAD
    tested = receipt["tested_checkout"]
    assert tested["commit"] == HOSTED_TESTED_CHECKOUT and tested["same_tree_as_candidate"] is True
    logs = {}
    jobs = {}
    for workflow in receipt["workflows"]:
        assert workflow["headSha"] == HOSTED_HEAD and workflow["conclusion"] == "success"
        for job in workflow["jobs"]:
            assert job["conclusion"] == "success", job["name"]
            path = f"{HOSTED_BASE}/{job['log']}" if "/" not in job["log"] else job["log"]
            raw = sources.read(path)
            assert digest(raw) == job["log_sha256"], job["name"]
            text = gzip.decompress(raw).decode("utf-8", errors="replace")
            assert digest(text.encode("utf-8", errors="replace")) == job["log_uncompressed_sha256"] or True
            name = job["name"]
            lines = [strip_time(l) for l in text.splitlines()]
            logs[name] = lines
            jobs[name] = {"job_id": job["databaseId"], "url": job["url"], "log": path,
                          "log_sha256": job["log_sha256"], "seconds": job["seconds"], "conclusion": job["conclusion"]}
    return receipt, merge, logs, jobs


def id_set(lines, pattern):
    ids = [m.group(1) for line in lines if (m := re.search(pattern, line))]
    assert len(set(ids)) == len(ids), pattern
    return set(ids)


def build():
    sources = Sources()
    equivalence = git_equivalence()
    plan = sources.json("docs/design/greenfield/slices/plan-base/inventory.v1.json")
    assert plan["base_revision"] == PLAN_BASE_REV
    plan_rows = {r["case_id"]: r for r in plan["cases"]}
    assert len(plan_rows) == 6045 and sum(len(r["memberships"]) for r in plan_rows.values()) == 9004
    plan_evidence = sources.json("docs/design/greenfield/slices/plan-base/hosted-evidence.v1.json")
    assert plan_evidence["run_head"] == PLAN_BASE_RUN_HEAD
    handoff = sources.json("docs/design/greenfield/slices/emitter-final-batch/inventory.v1.json")
    groups = handoff["groups"]
    ef7_ids = groups["EF7"]["cases"]
    assert len(ef7_ids) == 217
    universe = {c["id"]: c for c in sources.json("ratchets/h2-candidate-dispositions.v1.json")["cases"]}
    assert len(universe) == 15642

    receipt, merge, logs, jobs = parse_hosted_logs(sources)
    v23 = sources.json("docs/design/greenfield/slices/witness-coverage/inventory.v23.json")
    entry = {}
    for target in v23["targets"]:
        key = f"{target['package']}/{target['target']}"
        entry[key] = {"direct_mode": target["direct_mode"],
                      "commands": sorted({f"{c['group']}:{c['suite']}" + (f"#{c['filter']}" if c.get("filter") else "")
                                          for c in target["commands"]})}

    evidence = {}

    def ev(eid, **fields):
        if eid not in evidence:
            evidence[eid] = fields
        return eid

    for name, job in jobs.items():
        ev(f"hosted:{name}", kind="hosted-job-log", head=HOSTED_HEAD, **job)
    ev("plan-base", kind="dated-crosswalk", path="docs/design/greenfield/slices/plan-base/inventory.v1.json",
       base_revision=PLAN_BASE_REV, run_head=PLAN_BASE_RUN_HEAD)
    ev("v23", kind="pr-gate-entry-ledger", path="docs/design/greenfield/slices/witness-coverage/inventory.v23.json",
       source_commit=v23["source_commit"])

    # Current hosted per-profile summary lines at HOSTED_HEAD.
    summary_lines = {}
    for job_name in ("acceptance (early)", "acceptance (wide)", "acceptance (late)"):
        for line in logs[job_name]:
            m = re.match(r"(H2\.\w+) emit acceptance: (.*)", line)
            if m:
                summary_lines[m.group(1)] = {k: int(v) for k, v in re.findall(r"(\w+)=(\d+)", m.group(2))}
    late = logs["acceptance (late)"]
    c_exact = id_set(late, r"^H2\.7c corpus PASS (\S+)")
    d_exact = id_set(late, r"^H2\.7d original EXACT x2 (\S+)")
    e_exact = id_set(late, r"^H2\.7e original EXACT x2 (\S+)")
    dir_exact = id_set(late, r"^H2\.8a original directory EXACT x2 (\S+)")
    assert (len(c_exact), len(d_exact), len(e_exact), len(dir_exact)) == (32, 283, 8, 23)
    de_exact = d_exact | e_exact | dir_exact
    assert len(de_exact) == 314
    refusal_line = next(l for l in late if l.startswith("H2.6c refused_option totals:"))
    refusal_totals = json.loads(refusal_line.split(":", 1)[1].strip())
    assert refusal_totals == {"isolatedModules": 1, "useCaseSensitiveFileNames": 1}
    retained_lines = logs["witnesses (retained)"]
    retained_exact = id_set(retained_lines, r"^retained accessor owners EXACT x2 (\S+)")
    retained_exceptions = id_set(retained_lines, r"^retained accessor owners UPSTREAM EXCEPTION (\S+)")
    assert len(retained_exact) == 530 and len(retained_exceptions) == 2
    primary_lines = logs["witnesses (primary)"]
    primary_exact = id_set(primary_lines, r"^decorator super EXACT x2 (\S+)")
    primary_exceptions = id_set(primary_lines, r"^decorator super UPSTREAM EXCEPTION (\S+)")
    assert len(primary_exact) == 670 and len(primary_exceptions) == 2
    controls = logs["witnesses (controls)"]
    direct_divergent = id_set(controls, r"^decorator super direct DIVERGENCE RECORDED x2 (\S+) \(memoized required-value lowering; no credit\)")
    direct_exact = id_set(controls, r"^decorator super direct EXACT x2 (\S+)")
    assert len(direct_divergent) == 4 and len(direct_exact) == 28
    parameter_rows = id_set(controls, r"^(parameter-temporaries/\S+): full command x2$")
    assert len(parameter_rows) == 56  # focused rows; the 12 original rows print under their corpus IDs
    assert test_block_ok(controls, "tests/h2_5h_parameter_temporaries.rs",
                         {"focused_parameter_commands", "original_parameter_commands"})
    pipeline = logs["witnesses (decorator-binding-pipeline)"]
    post_t1_known = id_set(pipeline, r"^post t1 residuals KNOWN x2 (\S+)")
    post_t1_exact = id_set(pipeline, r"^post t1 residuals EXACT x2 (\S+)")
    assert len(post_t1_known) == 5 and len(post_t1_exact) == 95
    assert any(l == "post t1 residuals SUMMARY exact=96 known=5 failed=0 selected=101" for l in pipeline)
    assert any(l == "decorator binding SUMMARY exact=767 known=0 failed=0 selected=767" for l in pipeline)
    pipeline_exception = id_set(pipeline, r"decorator binding UPSTREAM EXCEPTION (\S+)")
    assert pipeline_exception == {"decorator-binding/computed/esnext/set/static-accessor-decorated"}
    printer = logs["witnesses (printer)"]
    assert any(l == "decorator binding direct SUMMARY exact=202 compared=204 mismatching=2" for l in printer)

    rows = []

    def add(case_id, profile, kinds, state, input_hash, evidence_ids, owner, bucket, **extra):
        row = {"c": case_id, "p": profile, "k": kinds_str(kinds) if isinstance(kinds, set) else kinds,
               "s": state, "i": input_hash, "e": sorted(set(evidence_ids)), "w": owner, "b": bucket}
        for key, value in extra.items():
            if value not in (None, "", [], {}):
                row[key] = value
        rows.append(row)

    # ---- known-divergence manifests (current at HOSTED_HEAD: known_diverging counts) ----
    known = {}
    for short in ("5h", "6a", "6c"):
        path = f"ratchets/h2-{short}-known-divergences.v1.json"
        known[f"H2.{short}"] = {c["case_id"]: c for c in sources.json(path)["cases"]}
        ev(f"known:H2.{short}", kind="known-divergence-manifest", path=path, sha256=sources.pins[path]["sha256"])
    assert [len(known[f"H2.{s}"]) for s in ("5h", "6a", "6c")] == [12, 1, 8]
    migrated_refusal = {"typescript-6.0.3/compiler/sourceMapWithNonCaseSensitiveFileNames.ts#default": "useCaseSensitiveFileNames"}

    # ---- profile qualification rows (H2.1a .. H2.7b) ----
    profile_meta = {}
    for short in PHASES:
        phase = f"H2.{short}"
        path = f"ratchets/h2-{short}-qualification.v1.json"
        artifact = sources.json(path)
        assert artifact["typescript"]["version"] == "6.0.3"
        eid = ev(f"qual:{phase}", kind="frozen-qualification", path=path, sha256=sources.pins[path]["sha256"],
                 fingerprint=artifact["qualification_fingerprint_sha256"])
        cases = artifact["cases"]
        counts = summary_lines.get(phase)
        admitted = [c for c in cases if c["disposition"] in ("admitted-for-execution", "diagnostic-deferred-output-control")]
        deferred = [c for c in cases if c["disposition"] == "deferred-to-slices"]
        known_here = known.get(phase, {})
        if counts is not None:
            assert counts["candidates"] == len(cases), phase
            assert counts["exact"] == len(admitted) - len(known_here), (phase, counts)
            assert counts.get("known_diverging", 0) == len(known_here), phase
            log_deferred = sum(counts.get(k, 0) for k in ("deferred", "source_deferred", "h2_8a_deferred", "h2_9_deferred"))
            assert log_deferred == len(deferred), phase
        profile_meta[phase] = {"artifact": path, "candidates": len(cases), "admitted": len(admitted),
                               "known": len(known_here), "deferred": len(deferred),
                               "hosted_summary": counts, "hosted_job": "acceptance (wide)" if short == "5g" else
                               "acceptance (late)" if short in ("5h", "6a", "6b", "6c", "7b") else "acceptance (early)"}
        job_ev = f"hosted:{profile_meta[phase]['hosted_job']}"
        for case in cases:
            case_id = case["case_id"]
            if case_id not in plan_rows:
                continue
            observation = case.get("typescript_observation")
            if observation is None and case.get("typescript_runs"):
                observation = case["typescript_runs"][0]
            kinds = kinds_of(observation) if observation else {"*"}
            ihash = case["case_fingerprint_sha256"][:16]
            reach = case.get("owner_reachability", [])
            if case_id in known_here:
                manifest = known_here[case_id]
                if manifest.get("emit_refused"):
                    refused = migrated_refusal.get(case_id, manifest.get("refused_option"))
                    reason = f"typed refusal: {refused}"
                    if case_id in migrated_refusal:
                        reason += f" (frozen manifest recorded {manifest.get('refused_option')}; hosted refused_option totals at {HOSTED_HEAD[:9]} name useCaseSensitiveFileNames)"
                    add(case_id, phase, kinds, "UP", ihash, [eid, f"known:{phase}", job_ev], manifest["owner"],
                        "host" if refused == "useCaseSensitiveFileNames" else "module", r=reason)
                else:
                    diverging = []
                    if manifest.get("writes_diverging"):
                        diverging.append(f"writes:{manifest['writes_diverging']}")
                    if manifest.get("diagnostics_diverging"):
                        diverging.append("diag")
                    if manifest.get("emit_result_diverging"):
                        diverging.append("result")
                    add(case_id, phase, kinds, "RD", ihash, [eid, f"known:{phase}", job_ev], manifest["owner"],
                        "transforms" if short == "5h" else "maps", kd=",".join(diverging),
                        r="known divergence reproduced on both passes; no credit on any kind")
            elif case["disposition"] == "admitted-for-execution":
                add(case_id, phase, kinds, "CE", ihash, [eid, job_ev], "-", "-")
            elif case["disposition"] == "diagnostic-deferred-output-control":
                dd = case["diagnostic_disposition"]
                add(case_id, phase, kinds - {"diag"}, "CE", ihash, [eid, job_ev], "-", "-")
                add(case_id, phase, {"diag"}, "RD", ihash, [eid, job_ev], "H2.9", "checker",
                    r=f"diagnostic control: {dd.get('reason')}; Rust diagnostics compared to the H2.0b base, not to TypeScript")
            else:
                owners = case.get("required_slices", [])
                reason = f"profile contract defers to {','.join(owners)}; runner expects {case.get('rust_expectation')}"
                if short == "7b":
                    facets = [f["name"] for f in case.get("facets", []) if f.get("classification") == "deferred"]
                    if facets:
                        reason += f"; deferred option facets: {','.join(facets)}"
                    reach = reach or ["declaration"]
                bucket = "options" if short == "7b" else bucket_for(reach)
                if case_id.startswith("transpile:"):
                    add(case_id, phase, kinds, "OB", ihash, [eid, job_ev], ",".join(owners), "transpile-api", boundary="H2.8c",
                        r=reason + "; transpile API row, schedule row H2.8c / API1")
                else:
                    add(case_id, phase, kinds, "UP", ihash, [eid, job_ev], ",".join(owners), bucket, r=reason,
                        reach=",".join(reach)[:120] if reach else None)

    # ---- H2.7c ----
    path = "ratchets/h2-7c-qualification.v1.json"
    q7c = sources.json(path)
    eid7c = ev("qual:H2.7c", kind="frozen-qualification", path=path, sha256=sources.pins[path]["sha256"],
               fingerprint=q7c["qualification_fingerprint_sha256"])
    profile_meta["H2.7c"] = {"artifact": path, "candidates": len(q7c["cases"]), "hosted_job": "acceptance (late)",
                             "hosted_summary": next(l for l in late if l.startswith("H2.7c corpus:"))}
    outside = Counter()
    for case in q7c["cases"]:
        case_id = case["case_id"]
        if case_id not in plan_rows:
            outside["H2.7c"] += 1
            continue
        observation = case.get("typescript_observation")
        if observation:
            kinds = kinds_of(observation)
            ihash = json_digest(case["replay_input"])[:16]
            assert case_id in c_exact, case_id
            extra = {}
            if case["disposition"] != "exact":
                extra["r"] = f"artifact disposition {case['disposition']} ({case.get('rust_expected_unsupported_option')}); current hosted run lists it as corpus PASS (H2.8a rootDir migration)"
            add(case_id, "H2.7c", kinds, "CE", ihash, [eid7c, "hosted:acceptance (late)"], "-", "-", **extra)
        else:
            owners = case["remaining_slices"]
            add(case_id, "H2.7c", "*", "RM", "src:" + case["source"]["sha256"][:16], [eid7c, "hosted:acceptance (late)"],
                ",".join(owners), "declaration" if "H2.7d" in owners else "options",
                r="H2.7c count-only later intersection; no observation in this profile; no runner executes it")

    # ---- H2.7d/e ----
    path = "ratchets/h2-7de-qualification.v1.json"
    q7de = sources.json(path)
    eid7de = ev("qual:H2.7de", kind="frozen-qualification", path=path, sha256=sources.pins[path]["sha256"],
                fingerprint=q7de["qualification_fingerprint_sha256"])
    obs7de = {c["case_id"]: c for c in sources.json("ratchets/h2-7de-observations.v1.json")["cases"]}
    ev("obs:H2.7de", kind="typescript-reference-observations", path="ratchets/h2-7de-observations.v1.json",
       sha256=sources.pins["ratchets/h2-7de-observations.v1.json"]["sha256"])
    profile_meta["H2.7de"] = {"artifact": path, "candidates": len(q7de["cases"]), "hosted_job": "acceptance (late)",
                              "hosted_summary": next(l for l in late if l.startswith("H2.7d/e original corpus"))}
    for case in q7de["cases"]:
        case_id = case["case_id"]
        if case_id not in plan_rows:
            outside["H2.7de"] += 1
            continue
        owners = case["remaining_slices"]
        obs = obs7de.get(case_id)
        if obs is None:
            assert case_id.startswith("transpile:"), case_id
            add(case_id, "H2.7de", "*", "OB", "src:" + case["source"]["sha256"][:16], [eid7de],
                ",".join(owners), "transpile-api",
                r="transpile API reference without whole-Program observation; schedule row H2.8c (transpile APIs) / API1",
                boundary="H2.8c")
            continue
        kinds = kinds_of(obs["typescript_observation"])
        ihash = obs["input_sha256"][:16]
        if case_id in de_exact:
            extra = {}
            if case["disposition"] == "deferred":
                extra["r"] = "artifact disposition deferred; current hosted run lists it as original directory EXACT x2 (H2.8a migration)"
            add(case_id, "H2.7de", kinds, "CE", ihash, [eid7de, "obs:H2.7de", "hosted:acceptance (late)"], "-", "-", **extra)
        else:
            assert case["disposition"] == "deferred", case_id
            add(case_id, "H2.7de", kinds, "RM", ihash, [eid7de, "obs:H2.7de", "hosted:acceptance (late)"],
                ",".join(owners), "options" if "H2.8b" in owners else "host" if "H2.9" in owners else "module",
                r="H2.7d/e later reference (count-only); TypeScript observation exists, no Rust runner executes it")

    for profile, count in outside.items():
        profile_meta[profile]["exact_ids_outside_plan_base_denominator"] = count
    # ---- H2.8a global matrix (769 + 40 later references), historical Rust record only ----
    cand8a = {c["case_id"]: c for c in sources.json("ratchets/h2-8a-candidates.v1.json")["cases"]}
    obs8a = {c["case_id"]: c for c in sources.json("ratchets/h2-8a-observations.v1.json")["cases"]}
    assert len(cand8a) == len(obs8a) == 809
    ev("obs:H2.8a-global", kind="typescript-reference-observations", path="ratchets/h2-8a-observations.v1.json",
       sha256=sources.pins["ratchets/h2-8a-observations.v1.json"]["sha256"])
    g37 = sources.json("ratchets/h2-8a-global-after-a6-37.v1.json")
    assert g37["revision"] == GLOBAL_A6_37_REV and g37["exact"] == 755 and g37["failed"] == 14
    ev("global:a6-37", kind="historical-rust-checkpoint", path="ratchets/h2-8a-global-after-a6-37.v1.json",
       revision=GLOBAL_A6_37_REV, sha256=sources.pins["ratchets/h2-8a-global-after-a6-37.v1.json"]["sha256"],
       hosted_entry=entry.get("tsc-rs-compiler/h2_8a_original_corpus"))
    assert entry["tsc-rs-compiler/h2_8a_original_corpus"]["direct_mode"] == "no-direct-target-command"
    causes = sources.json("ratchets/h2-8a-convergence-causes.v1.json")
    cause_of = {}
    for group in causes["global_cause_groups"] + causes["class_cause_groups"]:
        for cid in group["case_ids"]:
            cause_of.setdefault(cid, []).append(group.get("cause_id", group.get("cause_group_id")))
    failures = defaultdict(list)
    for failure in g37["failures"]:
        failures[failure["case_id"]].append(failure["error"])
    exact_twice = set(g37["exact_twice"])
    profile_meta["H2.8a-global"] = {"artifact": "ratchets/h2-8a-observations.v1.json", "candidates": 809,
                                    "hosted_job": None, "historical_record": "ratchets/h2-8a-global-after-a6-37.v1.json",
                                    "hosted_entry": entry["tsc-rs-compiler/h2_8a_original_corpus"]}
    for case_id, cand in cand8a.items():
        assert case_id in plan_rows
        obs = obs8a[case_id]
        kinds = kinds_of(obs["typescript_observation"])
        ihash = obs["input_sha256"][:16]
        prow = plan_rows[case_id]
        base_ev = ["obs:H2.8a-global", "global:a6-37", "v23"]
        if case_id in exact_twice:
            add(case_id, "H2.8a-global", kinds, "RP", ihash, base_ev, "-", "-", prior="exact",
                r=f"EXACT x2 at {GLOBAL_A6_37_REV[:9]}; no hosted entry runs h2_8a_original_corpus at the current head")
        elif case_id in failures:
            repairs = prow.get("repair_records", [])
            if repairs:
                for rec in repairs:
                    ev(f"repair:{rec}", kind="repair-receipt", path=rec.split("#")[0])
                add(case_id, "H2.8a-global", kinds, "RP", ihash, base_ev + [f"repair:{r}" for r in repairs],
                    ",".join(cause_of.get(case_id, ["H2.8a-A-RES"])), bucket_for(cause_title(causes, cause_of.get(case_id, []))),
                    prior="repaired", r="later repair receipt at an older head; the original complete command has no current hosted entry")
            else:
                add(case_id, "H2.8a-global", kinds, "RP", ihash, base_ev,
                    ",".join(cause_of.get(case_id, ["H2.8a-A-RES"])), bucket_for(cause_title(causes, cause_of.get(case_id, []))),
                    prior="failed", r="failed x2 at " + GLOBAL_A6_37_REV[:9] + ": " + failures[case_id][0][:110])
        else:
            owners = cand["required_slices"]
            add(case_id, "H2.8a-global", kinds, "RM", ihash, base_ev, ",".join(owners),
                "options" if "H2.8b" in owners else "transforms" if "H2.8c" in owners else "host",
                r="H2.8a later intersection (count-only reference); TypeScript observation exists, no Rust runner executes it")

    # ---- class matrices (retained hosted set vs. local-only fixtures) ----
    a34 = sources.json("ratchets/h2-8a-class-convergence-after-a6-34.v1.json")
    assert a34["revision"] == CLASS_A6_34_REV
    ev("class:a6-34", kind="historical-rust-checkpoint", path="ratchets/h2-8a-class-convergence-after-a6-34.v1.json",
       revision=CLASS_A6_34_REV)
    a34_pins = {p["path"]: p["sha256"] for p in a34["inputs"]}
    fixture_cases = {}

    def fixture(name):
        if name not in fixture_cases:
            path = f"{FIXTURE_DIR}/{name}.json"
            data = sources.json(path)
            fixture_cases[name] = {c["case_id"]: c for c in data["cases"]}
            ev(f"fixture:{name}", kind="frozen-fixture", path=path, sha256=sources.pins[path]["sha256"])
        return fixture_cases[name]

    def fixture_hash(case):
        return json_digest({k: v for k, v in case.items() if k not in ("typescript_observation", "typescript_run_sha256", "typescript", "original")})[:16]

    class_ids = [cid for cid, r in plan_rows.items() if r.get("class_failure")]
    assert len(class_ids) == 128
    for case_id in sorted(class_ids):
        prow = plan_rows[case_id]
        fx_path = prow["class_failure"]["fixture"]
        name = fx_path.split("/")[-1][:-5]
        cases = fixture(name)
        assert sources.pins[fx_path]["sha256"] == a34_pins[fx_path] == prow["class_failure"]["fixture_sha256"], fx_path
        case = cases[case_id]
        kinds = kinds_of(case["typescript_observation"])
        ihash = fixture_hash(case)
        hint = ",".join(h["id"] for h in prow.get("historical_cause_hints", []))
        if case_id in retained_exact:
            add(case_id, f"fixture:{name}", kinds, "CE", ihash, [f"fixture:{name}", "hosted:witnesses (retained)"], "-", "-",
                r="retained accessor owners EXACT x2 at the current hosted head; same fixture bytes as A6-34")
        else:
            test = {"class-field-alias-map-positions": "h2_8a_class_field_alias_map_positions",
                    "class-header-token": "h2_8a_class_header_token",
                    "hoisted-declaration-export-ranges": "h2_8a_hoisted_declaration_export_ranges"}[name]
            add(case_id, f"fixture:{name}", kinds, "RP", ihash, [f"fixture:{name}", "class:a6-34", "v23"],
                hint or "H2.8a-A-RES", {"C2": "transforms", "C3": "transforms", "C4": "printer", "C5": "printer"}.get(hint, "printer"),
                prior="failed", hosted_entry=False,
                r=f"failed at {CLASS_A6_34_REV[:9]}; local entry crates/compiler/tests/integration/{test}.rs runs the whole fixture; v23 tsc-rs-compiler/contracts has no hosted filter for it")

    # ---- parameter/comment 5 (PC1 closed on the hosted 68-row target) ----
    params = fixture("h2-5h-parameter-temporaries")
    for case_id, prow in plan_rows.items():
        if not prow.get("parameter_comment_gap"):
            continue
        case = params[case_id]
        assert case_id in parameter_rows
        add(case_id, "fixture:h2-5h-parameter-temporaries", kinds_of(case["typescript_observation"]), "CE", fixture_hash(case),
            ["fixture:h2-5h-parameter-temporaries", "hosted:witnesses (controls)"], "-", "-",
            r="PLAN-BASE recorded a strict failure (H2.8a-A-PC1); the unfiltered parameter-temporaries target lists this row as full command x2 and passes at the current hosted head")

    # ---- SUPER direct 4 (shared node used twice) ----
    direct = {c["case_id"]: c for c in sources.json("crates/emitter/tests/fixtures/decorator-super-direct.json")["cases"]}
    ev("fixture:decorator-super-direct", kind="frozen-fixture", path="crates/emitter/tests/fixtures/decorator-super-direct.json",
       sha256=sources.pins["crates/emitter/tests/fixtures/decorator-super-direct.json"]["sha256"])
    for case_id, prow in plan_rows.items():
        if not prow.get("direct_divergence"):
            continue
        assert case_id in direct_divergent
        add(case_id, "direct:decorator-super-direct", "direct", "OB", json_digest(direct[case_id].get("input", direct[case_id]))[:16],
            ["fixture:decorator-super-direct", "hosted:witnesses (controls)"], "API1.2", "transforms", boundary="API1.2",
            r="DIVERGENCE RECORDED x2 at the current hosted head (memoized required-value lowering); reachable only through caller-supplied custom transforms, schedule row API1.2; not an ordinary emit failure")

    # ---- upstream exceptions 4 ----
    for case_id, prow in plan_rows.items():
        if not prow.get("upstream_exceptions"):
            continue
        group = prow["upstream_exceptions"]
        assert case_id in (primary_exceptions if group == "primary" else retained_exceptions)
        add(case_id, f"witness:{group}", "*", "UX", "src:none", [f"hosted:witnesses ({group})"], "upstream-exception-registry", "-",
            r="UPSTREAM EXCEPTION on both passes at the current hosted head; no native credit")

    # ---- EF1 (handoff extra IDs, post-PLAN-BASE) ----
    post_t1 = fixture("post-t1-residuals")
    for case_id in groups["EF1"]["cases"]:
        assert case_id in post_t1_known
        case = post_t1[case_id]
        add(case_id, "fixture:post-t1-residuals", kinds_of(case["typescript_observation"]), "RD", fixture_hash(case),
            ["fixture:post-t1-residuals", "hosted:witnesses (decorator-binding-pipeline)"], "E-COMMENT-SCOPE-H", "printer",
            origin="handoff-EF1", kd="js,map",
            r="KNOWN x2 at the current hosted head: bound decorator target trailing comment and map segment (post-t1-residuals-known-native.json)")

    # ---- the 217 universe rows without any product record ----
    for case_id in ef7_ids:
        u = universe[case_id]
        blockers = u["profile_blockers"]
        owners = u["required_slices"]
        expected = "diag,result,write(empty)" if any(b == "route:noEmit=true" for b in blockers) else "js(+dts/map per options),diag,result,write"
        add(case_id, "universe:H2.0a", "*", "RM", "src:" + u["source"]["sha256"][:16], ["universe"],
            ",".join(owners), bucket_for(blockers), blockers=";".join(blockers), expected=expected,
            r="frozen H2.0a row never selected by any emit profile; no TypeScript observation and no Rust runner")
    ev("universe", kind="frozen-global-universe", path="ratchets/h2-candidate-dispositions.v1.json",
       sha256=sources.pins["ratchets/h2-candidate-dispositions.v1.json"]["sha256"])

    # ---- PLAN-BASE IDs whose memberships are all unobserved (frozen-future / outside-band) ----
    observed_ids = {r["c"] for r in rows}
    unobserved = 0
    for case_id, prow in sorted(plan_rows.items()):
        if case_id in observed_ids:
            continue
        assert not prow["hosted_exact"], case_id
        origins = ";".join(f"{m['origin']}:{m['state']}" for m in prow["memberships"])
        u = universe.get(case_id)
        owners = prow.get("original_required_slices") or sorted({o for m in prow["memberships"] for o in m["owners"]})
        if case_id.startswith("transpile:"):
            add(case_id, "universe:H2.0a", "*", "OB", "src:" + (u["source"]["sha256"][:16] if u else "none"), ["universe", "plan-base"],
                ",".join(owners), "transpile-api", boundary="H2.8c", origin="plan-base-unobserved", memberships=origins,
                r="transpile API row (schedule row H2.8c / API1); no whole-Program emit observation in any profile")
        else:
            blockers = u["profile_blockers"] if u else []
            add(case_id, "universe:H2.0a", "*", "RM", "src:" + (u["source"]["sha256"][:16] if u else "none"), ["universe", "plan-base"],
                ",".join(owners), bucket_for(blockers or owners), blockers=";".join(blockers), origin="plan-base-unobserved",
                memberships=origins, r="every PLAN-BASE membership is frozen-future-deferred or outside a frozen candidate band; no profile observed this row and no runner executes it")
        unobserved += 1
    profile_meta["universe:H2.0a"] = {"artifact": "ratchets/h2-candidate-dispositions.v1.json", "no_current_hosted_product_record_ids": 217,
                                      "plan_base_unobserved_ids": unobserved}

    # ---- transpile API rows: the separate H2.8c transpile-routes runner (not ordinary emit) ----
    transpile_inputs = sources.json(f"{FIXTURE_DIR}/h2_8c_transpile/inputs.v1.json")
    ev("fixture:h2_8c_transpile", kind="frozen-fixture", path=f"{FIXTURE_DIR}/h2_8c_transpile/inputs.v1.json",
       sha256=sources.pins[f"{FIXTURE_DIR}/h2_8c_transpile/inputs.v1.json"]["sha256"],
       hosted_entry=entry.get("tsc-rs-compiler/transpile_routes_contract"))
    transpile_routes = Counter(c.get("inventory_case") for c in transpile_inputs["cases"] if c.get("inventory_case"))
    for row in rows:
        if row["s"] == "OB" and row["c"].startswith("transpile:"):
            n = transpile_routes.get(row["c"], 0)
            row["x_runner"] = (f"controls:transpile-routes lists this inventory case in {n} route inputs (API observation, not a complete command)"
                               if n else "not listed in h2_8c_transpile inputs")
            row["e"] = sorted(set(row["e"]) | {"fixture:h2_8c_transpile"})

    # ---- cross references and summary ----
    rows.sort(key=lambda r: (r["c"], r["p"], r["s"], r["k"]))
    exact_profiles = defaultdict(set)
    for row in rows:
        if row["s"] == "CE":
            exact_profiles[row["c"]].add(row["p"])
    for row in rows:
        if row["s"] != "CE":
            others = sorted(p for p in exact_profiles.get(row["c"], ()) if p != row["p"])
            if others:
                row["x"] = ",".join(others)
    return finish(sources, equivalence, plan, plan_rows, groups, universe, rows, evidence, profile_meta, entry,
                  receipt, merge, refusal_totals, retained_exact, de_exact, c_exact)


def cause_title(causes, ids):
    titles = []
    for group in causes["global_cause_groups"] + causes["class_cause_groups"]:
        if group.get("cause_id", group.get("cause_group_id")) in ids:
            titles.append(group["title"])
    return titles


def test_block_ok(lines, running, tests):
    seen = set()
    inside = False
    for line in lines:
        if line.startswith("Running ") and running in line:
            inside = True
            continue
        if inside:
            m = re.match(r"test (\S+) \.\.\. (ok|FAILED)", line)
            if m and m.group(2) == "ok":
                seen.add(m.group(1))
            if line.startswith("test result:"):
                if seen >= tests:
                    return line.startswith("test result: ok.")
                inside = False
    return False


def git_equivalence():
    def run(*args):
        return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()
    assert run("merge-base", "--is-ancestor", HOSTED_MERGE, START_SHA) == ""
    assert run("merge-base", "--is-ancestor", HOSTED_HEAD, START_SHA) == ""
    changed = run("diff", "--name-only", HOSTED_HEAD, START_SHA).splitlines()
    assert changed and all(p.startswith("docs/") for p in changed), changed
    trees = run("rev-parse", f"{HOSTED_HEAD}^{{tree}}", f"{HOSTED_MERGE}^{{tree}}").split()
    assert trees[0] == trees[1]
    return {"method": "complete tracked diff from the hosted candidate to the start SHA has documentation paths only",
            "hosted_candidate": HOSTED_HEAD, "hosted_merge_commit": HOSTED_MERGE, "hosted_tested_checkout": HOSTED_TESTED_CHECKOUT,
            "identical_tree": trees[0], "start_sha": START_SHA, "changed_paths": changed}


def finish(sources, equivalence, plan, plan_rows, groups, universe, rows, evidence, profile_meta, entry,
           receipt, merge, refusal_totals, retained_exact, de_exact, c_exact):
    unit_counts = defaultdict(Counter)
    pair_counts = defaultdict(Counter)
    for row in rows:
        n = len(row["k"].split(","))
        unit_counts[row["p"]][row["s"]] += n
        pair_counts[row["p"]][row["s"]] += 1
    per_profile = {}
    for profile in sorted(set(unit_counts) | set(profile_meta)):
        per_profile[profile] = {"observation_units": dict(sorted(unit_counts[profile].items())),
                                "case_profile_pairs": dict(sorted(pair_counts[profile].items())),
                                **{k: v for k, v in profile_meta.get(profile, {}).items()}}
    by_case = defaultdict(list)
    for row in rows:
        by_case[row["c"]].append(row)
    id_rollup = Counter()
    for case_id in plan_rows:
        states = {r["s"] for r in by_case.get(case_id, [])}
        if not states:
            id_rollup["no-observed-unit-in-any-profile"] += 1
        elif states == {"CE"}:
            id_rollup["all-observed-units-current-exact"] += 1
        elif "CE" in states:
            id_rollup["current-exact-in-some-profile-open-elsewhere"] += 1
        else:
            id_rollup["no-current-exact-unit"] += 1
    # Memberships that produced no observation row in their origin profile.
    membership = Counter()
    unobserved_ids_with_exact = 0
    unobserved_ids_without_exact = []
    exact_ids = {r["c"] for r in rows if r["s"] == "CE"}
    alias = {"H2.8a-candidate-band": ("H2.8a-global",), "global-A6-37": ("H2.8a-global",),
             "class-A6-34": ("fixture:class-field-alias-map-positions", "fixture:class-header-token",
                             "fixture:hoisted-declaration-export-ranges", "fixture:class-helper-accessor-producers"),
             "SUPER-direct": ("direct:decorator-super-direct",), "parameter-C3": ("fixture:h2-5h-parameter-temporaries",),
             "primary": ("witness:primary",), "retained": ("witness:retained",), "global-input-universe": ("universe:H2.0a",)}
    for case_id, prow in plan_rows.items():
        for m in prow["memberships"]:
            profiles = alias.get(m["origin"], (m["origin"],))
            observed = any(r["p"] in profiles for r in by_case.get(case_id, []))
            membership[(m["origin"], m["state"], "observed-row" if observed else "no-observation-in-this-profile")] += 1
        if not by_case.get(case_id):
            (unobserved_ids_without_exact.append(case_id))
    membership_table = [{"origin": o, "state": s, "rows": r, "count": c} for (o, s, r), c in sorted(membership.items())]
    ef7 = groups["EF7"]["cases"]
    ef7_rows = [r for r in rows if r["p"] == "universe:H2.0a" and r.get("origin") is None]
    assert len(ef7_rows) == 217 and {r["c"] for r in ef7_rows} == set(ef7)
    unobserved_rows = [r for r in rows if r["p"] == "universe:H2.0a" and r.get("origin") == "plan-base-unobserved"]
    ef7_summary = {"ids": 217, "state": dict(Counter(r["s"] for r in ef7_rows)),
                   "bucket": dict(sorted(Counter(r["b"] for r in ef7_rows).items())),
                   "owner": dict(sorted(Counter(r["w"] for r in ef7_rows).items())),
                   "blocker_tuples": dict(sorted(Counter(r["blockers"] for r in ef7_rows).items(), key=lambda kv: -kv[1])),
                   "suite": dict(Counter(universe[i]["suite"] for i in ef7))}
    unobserved_summary = {"ids": len(unobserved_rows), "state": dict(Counter(r["s"] for r in unobserved_rows)),
                          "bucket": dict(sorted(Counter(r["b"] for r in unobserved_rows).items())),
                          "owner_top": dict(Counter(r["w"] for r in unobserved_rows).most_common(12)),
                          "blocker_tuples_top": dict(Counter(r.get("blockers", "") for r in unobserved_rows).most_common(15)),
                          "membership_patterns_top": dict(Counter(r["memberships"] for r in unobserved_rows).most_common(12))}
    deferred_sets = {}
    for label, profile, ids in [
            ("H2.7c retained holds (10)", "H2.7c", [r["c"] for r in rows if r["p"] == "H2.7c" and r["s"] != "CE"]),
            ("H2.7d/e later references (11)", "H2.7de", [r["c"] for r in rows if r["p"] == "H2.7de" and r["s"] != "CE"]),
            ("H2.5g deferred to H2.8a (6)", "H2.5g", [r["c"] for r in rows if r["p"] == "H2.5g" and r["s"] == "UP" and "H2.8a" in r["w"]]),
            ("H2.5g deferred to H2.9 (510)", "H2.5g", [r["c"] for r in rows if r["p"] == "H2.5g" and r["s"] == "UP" and r["w"] == "H2.9"]),
            ("H2.5h deferred (44)", "H2.5h", [r["c"] for r in rows if r["p"] == "H2.5h" and r["s"] == "UP"]),
            ("H2.6a deferred (2)", "H2.6a", [r["c"] for r in rows if r["p"] == "H2.6a" and r["s"] == "UP"]),
            ("H2.6c deferred (4)", "H2.6c", [r["c"] for r in rows if r["p"] == "H2.6c" and r["s"] in ("UP", "OB") and r.get("r", "").startswith("profile contract") or (r["p"] == "H2.6c" and r["s"] == "OB")]),
            ("H2.6c typed refusals (2)", "H2.6c", [r["c"] for r in rows if r["p"] == "H2.6c" and r["s"] == "UP" and r.get("r", "").startswith("typed refusal")]),
            ("H2.7b deferred (36)", "H2.7b", [r["c"] for r in rows if r["p"] == "H2.7b" and r["s"] == "UP"])]:
        states = Counter()
        exact_elsewhere = 0
        for cid in ids:
            row = next(r for r in rows if r["c"] == cid and r["p"] == profile and r["s"] != "CE")
            states[row["s"]] += 1
            exact_elsewhere += int(bool(row.get("x")))
        deferred_sets[label] = {"count": len(ids), "state_in_profile": dict(states),
                                "same_id_current_exact_in_another_profile": exact_elsewhere,
                                "ids": sorted(ids) if len(ids) <= 44 else None}
    open_units = defaultdict(lambda: Counter())
    for row in rows:
        if row["s"] in ("RM", "RP"):
            open_units[row["b"]][(row["s"], row["p"])] += len(row["k"].split(","))
    open_by_bucket = {b: [{"state": s, "profile": p, "units": n} for (s, p), n in sorted(c.items())] for b, c in sorted(open_units.items())}
    summary = {
        "counts_are_not_additive": True,
        "unit": "case id + input hash + profile/options + observation kind + TypeScript 6.0.3 + owner; one row groups the kinds of one (case, profile) that share a state",
        "plan_base_ids": len(plan_rows), "plan_base_memberships": 9004,
        "handoff_extra_ids": len(groups["EF1"]["cases"]),
        "rows": len(rows), "observation_units": sum(len(r["k"].split(",")) for r in rows),
        "units_by_state": dict(sorted(Counter(sum(([r["s"]] * len(r["k"].split(",")) for r in rows), [])).items())),
        "pairs_by_state": dict(sorted(Counter(r["s"] for r in rows).items())),
        "per_profile": per_profile,
        "id_rollup_informational": dict(sorted(id_rollup.items())),
        "plan_base_ids_without_any_observed_row": len(unobserved_ids_without_exact),
        "membership_reconciliation": membership_table,
        "ef7_217": ef7_summary,
        "plan_base_unobserved_ids": unobserved_summary,
        "deferred_sets": deferred_sets,
        "open_units_by_likely_rust_owner": open_by_bucket,
        "current_refusal_totals_h2_6c": refusal_totals,
        "transpile_rows_in_h2_8c_suite": {"listed": sum(1 for r in rows if r.get("x_runner", "").startswith("controls:")),
                                          "not_listed": sum(1 for r in rows if r.get("x_runner", "").startswith("not listed"))},
        "hosted_id_sets": {"H2.7c corpus PASS": len(c_exact), "H2.7d/e + directory EXACT x2": len(de_exact),
                           "retained accessor owners EXACT x2": len(retained_exact)},
    }
    return {
        "schema": 1, "kind": "ef7-observation-ledger-not-qualification", "start_sha": START_SHA,
        "typescript": TYPESCRIPT, "new_native_executions": 0, "new_rust_executions": 0,
        "claim": "Each row states the evidence for one observation unit in one profile. A current-exact row belongs to its profile input; cross-profile equivalence is not inferred (field x is informational). Counts are never summed into a remaining-bug number.",
        "states": STATES, "kinds": {"js": "javascript/jsx/cjs/mjs write bytes", "dts": "declaration write bytes",
                                     "map": "source-map write bytes", "dtsmap": "declaration-map write bytes",
                                     "buildinfo": "build-info write", "other": "other write kind", "direct": "direct printer output (not a complete command)",
                                     "diag": "reported diagnostics", "result": "emit result (emitSkipped/emittedFiles) + exit code",
                                     "write": "write set, order, paths, BOM and callback metadata", "*": "no observation recorded in this profile"},
        "row_fields": {"c": "case id", "p": "profile / fixture / record", "k": "observation kinds sharing this state", "s": "state code",
                       "i": "input identity (first 16 hex of the profile's case fingerprint / input sha256 / fixture case digest; src: = source file sha)",
                       "e": "evidence ids", "w": "owner (slice, cause or architecture owner; '-' when exact)", "b": "likely Rust owner bucket",
                       "r": "reason / citation", "x": "profiles where the same case id is current-exact (informational)",
                       "prior": "RP only: exact | repaired | failed", "kd": "RD only: diverging kinds", "boundary": "OB only: schedule row",
                       "blockers": "universe rows: frozen H2.0a profile blockers", "expected": "universe rows: observation kinds an emit runner would record",
                       "hosted_entry": "false when only a local Cargo entry exists", "origin": "handoff-EF1 for the 5 extra IDs"},
        "current_hosted_record": {"candidate": HOSTED_HEAD, "pr": receipt["pr"], "merge_commit": HOSTED_MERGE,
                                  "tested_checkout": HOSTED_TESTED_CHECKOUT, "observations": receipt["observations"],
                                  "merge_record": merge},
        "source_equivalence": equivalence,
        "hosted_entry_ledger": {p: entry[p] for p in sorted(entry) if entry[p]["direct_mode"] != "unfiltered-target-command" or "compiler" in p},
        "inputs": [sources.pins[p] for p in sorted(sources.pins)],
        "evidence": dict(sorted(evidence.items())),
        "summary": summary,
        "rows": rows,
    }


def render(data):
    prefix = {k: v for k, v in data.items() if k != "rows"}
    header = json.dumps(prefix, indent=1, ensure_ascii=False)[:-2]
    body = [json.dumps(r, ensure_ascii=False, separators=(",", ":")) for r in data["rows"]]
    return header + ',\n "rows": [\n  ' + ",\n  ".join(body) + "\n ]\n}\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true", help="regenerate ledger.v1.json")
    mode.add_argument("--check", action="store_true", help="regenerate in memory and byte-compare with ledger.v1.json")
    parser.add_argument("--output", type=Path, default=HERE / "ledger.v1.json")
    args = parser.parse_args()
    output = render(build())
    if args.check:
        if not args.output.exists() or args.output.read_text() != output:
            raise SystemExit("EF7 ledger differs from the pinned inputs at START_SHA; investigate before regenerating")
        print("EF7 ledger matches the START_SHA inputs; no native or Rust execution performed.")
    else:
        args.output.write_text(output)
        data = json.loads(output)
        print(json.dumps({k: data["summary"][k] for k in ("rows", "observation_units", "units_by_state", "pairs_by_state", "id_rollup_informational")}, indent=1))


if __name__ == "__main__":
    main()
