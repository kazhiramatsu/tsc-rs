#!/usr/bin/env python3
"""Collect pinned PR #557 logs and verify the real replay step records."""
from pathlib import Path
import datetime
import gzip
import hashlib
import importlib.util
import json
import re
import subprocess

ROOT = Path(__file__).resolve().parents[6]
OUT = Path(__file__).resolve().parent / "hosted"
HEAD = "bc4fa2c295dc20e45814f44ce8f460a6d959be0f"
BASE = "c35e00ccb006e3b4e3e2643491e8fe3207595097"
RUNS = (35190703849, 35190703827)
REPO = "kazhiramatsu/tsc-rs"


def gh(*args):
    return subprocess.check_output(["gh", *map(str, args)], cwd=ROOT)


def seconds(job):
    def date(value):
        return datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
    return int((date(job["completedAt"]) - date(job["startedAt"])).total_seconds())


def load_module(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def timing_records(raw, expected, witness):
    # Plan jobs run negative unit cases that deliberately print failed records.
    # This parser is only called for the three actual compiler replay jobs.
    records = []
    summaries = []
    for line in raw.splitlines():
        if "{" not in line:
            continue
        try:
            item = json.loads(line[line.index("{"):])
        except json.JSONDecodeError:
            continue
        if not isinstance(item, dict):
            continue
        if item.get("witness_step") == "compiler-direct":
            records.append(item)
        if "compiler_direct" in item:
            summaries.append(item)
    assert len(summaries) == 1, summaries
    summary = summaries[0]
    assert summary["compiler_direct"] == expected, summary
    assert summary["targets"] == len(expected), summary
    assert summary["tests_passed"] == sum(witness.COMPILER_DIRECT[s]["tests"] for s in expected), summary
    commands = [("observer", list(command),
                 [s for s in expected if command in witness.compiler_direct_observers([s])])
                for command in witness.compiler_direct_observers(expected)]
    unfiltered = [s for s in expected if "test" not in witness.COMPILER_DIRECT[s]]
    batches = ([unfiltered] if unfiltered else []) + [[s] for s in expected if "test" in witness.COMPILER_DIRECT[s]]
    commands.extend(("cargo-build-and-replay", witness.compiler_direct_command(batch), list(batch))
                    for batch in batches)
    assert len(records) == 2 * len(commands), (len(records), len(commands))
    finishes = []
    for index, (phase, command, suites) in enumerate(commands):
        start, finish = records[index * 2:index * 2 + 2]
        for record in (start, finish):
            assert record["phase"] == phase and record["argv"] == command and record["suites"] == suites, record
        assert start["event"] == "start", start
        assert finish["event"] == "finish" and finish["status"] == "passed" and finish["seconds"] >= 0, finish
        finishes.append(finish)
    return {"summary": summary, "steps": finishes,
            "observer_step_seconds": round(sum(r["seconds"] for r in finishes if r["phase"] == "observer"), 3),
            "cargo_batch_step_seconds": round(sum(r["seconds"] for r in finishes if r["phase"] == "cargo-build-and-replay"), 3)}


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    workflows = []
    for run in RUNS:
        data = json.loads(gh("run", "view", run, "--json", "databaseId,headSha,headBranch,event,status,conclusion,createdAt,updatedAt,url,jobs"))
        assert data["headSha"] == HEAD and data["event"] == "pull_request", data
        for job in data["jobs"]:
            if job["status"] != "completed":
                continue
            job["seconds"] = seconds(job)
            path = OUT / f'{job["databaseId"]}.log.gz'
            if not path.exists():
                result = subprocess.run(["gh", "api", f'repos/{REPO}/actions/jobs/{job["databaseId"]}/logs'], cwd=ROOT, capture_output=True)
                if result.returncode:
                    job["log_pending"] = result.stderr.decode().strip()
                    continue
                path.write_bytes(gzip.compress(result.stdout, mtime=0))
            raw = gzip.decompress(path.read_bytes())
            job.update(log=path.name, log_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
                       log_uncompressed_sha256=hashlib.sha256(raw).hexdigest())
            checkouts = re.findall(r"git log -1 --format=%H\r?\n\S+ ([0-9a-f]{40})", raw.decode())
            assert len(checkouts) == 1, (job["name"], checkouts)
            job["checkout_commit"] = checkouts[0]
        workflows.append(data)
    jobs = [job for run in workflows for job in run["jobs"]]
    record = {"version": 1, "candidate_commit": HEAD, "base_commit": BASE,
              "pr": f"https://github.com/{REPO}/pull/557", "workflows": workflows}
    progress = {"jobs": len(jobs), "success": sum(j["conclusion"] == "success" for j in jobs),
                "failure": [(j["name"], j["conclusion"]) for j in jobs if j["conclusion"] not in ("", "success")],
                "running": [j["name"] for j in jobs if j["status"] != "completed"],
                "logs": sum("log" in j for j in jobs)}
    progress_path = ROOT / "target/emitter-final-ci-hosted-progress.json"
    progress_path.parent.mkdir(parents=True, exist_ok=True)
    progress_path.write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps(progress), flush=True)
    if not all(run["status"] == "completed" for run in workflows):
        return
    assert len(jobs) == 14 and all(j["conclusion"] == "success" and "log" in j for j in jobs), progress
    checkouts = {job["checkout_commit"] for job in jobs}
    assert len(checkouts) == 1, checkouts
    checkout = next(iter(checkouts))
    commit = json.loads(gh("api", f"repos/{REPO}/git/commits/{checkout}"))
    record["candidate_tree"] = gh("api", f"repos/{REPO}/git/commits/{HEAD}", "--jq", ".tree.sha").decode().strip()
    assert commit["tree"]["sha"] == record["candidate_tree"], commit
    parents = [p["sha"] for p in commit["parents"]]
    assert parents == [BASE, HEAD], parents
    record["tested_checkout"] = {"commit": checkout, "tree": commit["tree"]["sha"],
                                 "parents": parents, "same_tree_as_candidate": True}

    def log(name):
        job = next(j for j in jobs if j["name"] == name)
        return gzip.decompress((OUT / job["log"]).read_bytes()).decode()

    pipeline = log("witnesses (decorator-binding-pipeline)")
    controls = log("witnesses (controls)")
    required = [("h2-5g", "H2.5g emit acceptance: candidates=9027 exact=8511 h2_8a_deferred=6 h2_9_deferred=510 exact_diagnostics=26815 exact_writes=9466 repetitions=2", log("acceptance (wide)")),
                ("super-primary", "decorator super SUMMARY exact=670 failed=0 selected=670", log("witnesses (primary)")),
                ("pipeline", "decorator binding SUMMARY exact=767 known=0 failed=0 selected=767", pipeline),
                ("post-t1", "post t1 residuals SUMMARY exact=96 known=5 failed=0 selected=101", pipeline),
                ("post-t1-packet", "post t1 residuals PACKET SUMMARY exact=79 known=0 failed=0 probed=79", pipeline),
                ("t1", "bundle metadata t1 SUMMARY exact=18 known=0 failed=0 selected=18", controls),
                ("t1-packet", "bundle metadata t1 PACKET SUMMARY exact=15 known=0 failed=0 probed=15", controls)]
    for name, expected, raw in required:
        assert expected in raw, (name, expected)
    replay = load_module("emitter_budget_replay", ROOT / ".github/ci/replay.py")
    compiler = {}
    for group in ("controls", "module-output", "decorator-binding-pipeline"):
        expected = [s for s in replay.WITNESS_GROUPS[group] if s in replay.witness.COMPILER_DIRECT]
        compiler[group] = timing_records(log(f"witnesses ({group})"), expected, replay.witness)
    replay_jobs = [j for j in jobs if j["name"].startswith(("acceptance (", "witnesses ("))]
    assert len(replay_jobs) == 10
    record.update(replay_jobs=10, replay_total_seconds=sum(j["seconds"] for j in replay_jobs),
                  replay_longest_seconds=max(j["seconds"] for j in replay_jobs), workers=2,
                  observations={name:expected for name,expected,_ in required}, compiler_direct=compiler,
                  timing_excludes=["plans", "gates", "main push"],
                  timing_limits=["Whole-job comparisons include host/cache variability.",
                                 "Cargo batch seconds include build and replay and are not divided among suites.",
                                 "Baseline per-observer timing was not recorded; no isolated speedup is inferred."],
                  remaining_known={"post_t1_complete_commands":5,"post_t1_packet_probes":0,
                                   "owner":"E-COMMENT-SCOPE-H bound decorator target trailing comments"})
    baseline = json.loads((OUT.parent / "baseline.v1.json").read_text())
    baseline_path = ROOT / baseline["source_receipt"]
    assert hashlib.sha256(baseline_path.read_bytes()).hexdigest() == baseline["source_receipt_sha256"]
    baseline_receipt = json.loads(baseline_path.read_text())
    baseline_summaries = []
    for name in ("witnesses (controls)", "witnesses (declaration-maps)"):
        job = next(j for run in baseline_receipt["workflows"] for j in run["jobs"] if j["name"] == name)
        path = baseline_path.parent / job["log"]
        assert hashlib.sha256(path.read_bytes()).hexdigest() == job["log_sha256"]
        lines = [line for line in gzip.decompress(path.read_bytes()).decode().splitlines() if '{"compiler_direct":' in line]
        assert len(lines) == 1
        baseline_summaries.append(json.loads(lines[0][lines[0].index("{"):]))
    new_summaries = [compiler[group]["summary"] for group in ("controls", "module-output")]
    old_suites = sorted(s for summary in baseline_summaries for s in summary["compiler_direct"])
    new_suites = sorted(s for summary in new_summaries for s in summary["compiler_direct"])
    assert old_suites == new_suites and len(new_suites) == len(set(new_suites)) == 23
    assert sum(s["tests_passed"] for s in baseline_summaries) == sum(s["tests_passed"] for s in new_summaries) == 81
    record["baseline_comparison"] = {
        "source_receipt": baseline["source_receipt"], "source_receipt_sha256": baseline["source_receipt_sha256"],
        "compiler_membership_preserved": {"suites": new_suites, "targets": 23, "tests_passed": 81},
        "before": {"controls_seconds": baseline["controls_job"]["seconds"],
                   "declaration_maps_seconds": baseline["declaration_maps_seconds"],
                   "total_replay_seconds": baseline["baseline_total_replay_seconds"]},
        "after": {"controls_seconds": next(j["seconds"] for j in jobs if j["name"] == "witnesses (controls)"),
                  "module_output_seconds": next(j["seconds"] for j in jobs if j["name"] == "witnesses (module-output)"),
                  "total_replay_seconds": record["replay_total_seconds"]}}
    (OUT / "receipt.v1.json").write_text(json.dumps(record, indent=2) + "\n")
    print("FINAL_RECEIPT_VERIFIED")


if __name__ == "__main__":
    main()
