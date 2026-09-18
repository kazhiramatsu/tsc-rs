#!/usr/bin/env python3
"""Archive completed PR jobs; qualify only a complete, successful, identical tree."""
import argparse
import datetime
import gzip
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[6]
OUT = Path(__file__).resolve().parent / "hosted"
REPO = "kazhiramatsu/tsc-rs"
BASE = "3b1f5fe87fd31e3b303bb44bd257342735452ed9"
sys.path.insert(0, str(ROOT / "scripts"))
import emitter_final_witnesses as final


def gh(*args):
    return subprocess.check_output(["gh", *map(str, args)], cwd=ROOT)


def seconds(job):
    date = lambda value: datetime.datetime.fromisoformat(value.replace("Z", "+00:00"))
    return int((date(job["completedAt"]) - date(job["startedAt"])).total_seconds())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--head", required=True)
    parser.add_argument("--runs", type=int, nargs=2, required=True)
    args = parser.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    workflows = []
    for run in args.runs:
        data = json.loads(gh("run", "view", run, "--json",
                             "databaseId,headSha,headBranch,event,status,conclusion,createdAt,updatedAt,url,jobs"))
        assert data["headSha"] == args.head and data["event"] == "pull_request", data
        for job in data["jobs"]:
            if job["status"] != "completed":
                continue
            job["seconds"] = seconds(job)
            file = OUT / f'{job["databaseId"]}.log.gz'
            if not file.exists():
                result = subprocess.run(["gh", "api", f'repos/{REPO}/actions/jobs/{job["databaseId"]}/logs'],
                                        cwd=ROOT, capture_output=True)
                if result.returncode:
                    job["log_pending"] = result.stderr.decode().strip()
                    continue
                file.write_bytes(gzip.compress(result.stdout, mtime=0))
            raw = gzip.decompress(file.read_bytes())
            job.update(log=file.name, log_sha256=hashlib.sha256(file.read_bytes()).hexdigest(),
                       log_uncompressed_sha256=hashlib.sha256(raw).hexdigest())
            checkouts = re.findall(r"git log -1 --format=%H\r?\n\S+ ([0-9a-f]{40})", raw.decode())
            assert len(checkouts) == 1, (job["name"], checkouts)
            job["checkout_commit"] = checkouts[0]
        workflows.append(data)
    jobs = [job for run in workflows for job in run["jobs"]]
    record = {"version": 1, "candidate_commit": args.head, "base_commit": BASE,
              "pr": f"https://github.com/{REPO}/pull/561", "workflows": workflows}
    progress = {"jobs": len(jobs), "success": sum(j["conclusion"] == "success" for j in jobs),
                "failure": [(j["name"], j["conclusion"]) for j in jobs if j["conclusion"] not in ("", "success")],
                "running": [j["name"] for j in jobs if j["status"] != "completed"],
                "logs": sum("log" in j for j in jobs)}
    (OUT / f"progress-{args.head[:12]}.json").write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps(progress), flush=True)
    if not all(run["status"] == "completed" for run in workflows):
        return
    assert len(jobs) == 23 and all(j["conclusion"] == "success" and "log" in j for j in jobs), progress
    checkouts = {job["checkout_commit"] for job in jobs}
    assert len(checkouts) == 1, checkouts
    checkout = next(iter(checkouts))
    commit = json.loads(gh("api", f"repos/{REPO}/git/commits/{checkout}"))
    tree = gh("api", f"repos/{REPO}/git/commits/{args.head}", "--jq", ".tree.sha").decode().strip()
    assert commit["tree"]["sha"] == tree
    parents = [p["sha"] for p in commit["parents"]]
    assert parents == [BASE, args.head], parents
    record.update(candidate_tree=tree, tested_checkout={"commit": checkout, "tree": tree,
                  "parents": parents, "same_tree_as_candidate": True})

    def log(name):
        job = next(j for j in jobs if j["name"] == name)
        return gzip.decompress((OUT / job["log"]).read_bytes()).decode()

    required = {
        "witnesses (decorator-binding-pipeline)": [
            "decorator binding SUMMARY exact=767 known=0 failed=0 selected=767",
            "post t1 residuals SUMMARY exact=101 known=0 failed=0 selected=101",
            "post t1 residuals PACKET SUMMARY exact=79 known=0 failed=0 probed=79"],
        "witnesses (controls)": [
            "bundle metadata t1 SUMMARY exact=18 known=0 failed=0 selected=18",
            "bundle metadata t1 PACKET SUMMARY exact=15 known=0 failed=0 probed=15"],
        "witnesses (emitter-final)": [
            "external helper imports SUMMARY exact=539 known=12 failed=0 selected=551",
            "emitter final rows SUMMARY exact=21 known=0 failed=0 selected=21",
            "EF4/EF5 SUMMARY exact=40 failed=0 selected=40", "EF6 SUMMARY exact=14 selected=14",
            "selected 217/217 / exact 216 / known 1 / failed 0"],
    }
    for name, snippets in required.items():
        for snippet in snippets:
            assert snippet in log(name), (name, snippet)
    plan_counts = []
    for suite in final.SUITES:
        raw = log(f"witnesses ({suite})")
        # Pair each dispatched command with its actual result and completion marker.
        events = []
        for line in raw.splitlines():
            if "{" not in line:
                continue
            try:
                item = json.loads(line[line.index("{"):])
            except json.JSONDecodeError:
                continue
            if isinstance(item, dict) and item.get("suite") == suite and "event" in item:
                events.append(item)
        commands = final.commands(suite)
        assert len(events) == 2 * len(commands), (suite, events)
        for index, (command, _) in enumerate(commands):
            start, finish = events[2 * index:2 * index + 2]
            assert start["event"] == "start" and start["argv"] == command
            assert finish["event"] == "passed" and finish["seconds"] >= 0
        if suite.startswith("emitter-plan-base-"):
            rows = re.findall(r"selected (\d+)/1798 / exact (\d+) / known (\d+) / failed (\d+)", raw)
            assert len(rows) == 1, (suite, rows)
            selected, exact, known, failed = map(int, rows[0])
            assert selected == len(final.case_ids(suite)) and exact + known == selected and failed == 0
            plan_counts.append((exact, known))
        if suite == "emitter-global" or suite.startswith("emitter-class-"):
            observed = re.findall(r"EXACT x2 (\S+)", raw)
            assert sorted(observed) == sorted(final.case_ids(suite)), suite
    assert tuple(map(sum, zip(*plan_counts))) == (1763, 35), plan_counts
    replay = [j for j in jobs if j["name"].startswith(("acceptance (", "witnesses ("))]
    assert len(replay) == 19
    record.update(replay_jobs=19, replay_total_seconds=sum(j["seconds"] for j in replay),
                  replay_longest_seconds=max(j["seconds"] for j in replay), workers=2,
                  required_observations=required, plan_base={"exact": 1763, "known": 35, "failed": 0},
                  timing_excludes=["plans", "gates", "main push"],
                  additional_command_controls={"selected": 551, "exact": 539, "typed_parse_boundaries": 12},
                  remaining_known={"parse_recovery": 36, "resolution": 0, "helper_collision": 0, "checker": 0})
    (OUT / "receipt.v1.json").write_text(json.dumps(record, indent=2) + "\n")
    print("FINAL_RECEIPT_VERIFIED")


if __name__ == "__main__":
    main()
