#!/usr/bin/env python3
"""Run the TypeScript 7.1 conformance lanes in restartable worker processes.

usage: conformance_ts71.py [--profile <name>] [--workers <n>] [--timeout <seconds>]
                           [--max-rss-mib <n>] [--filter <path substring>]
                           [--checkers <n>] [--check | --update]

Some cases make tsc-rs (like TypeScript 6.0.3) recurse without bound or expand
types without end: a stack overflow aborts the process, and a runaway case
never finishes. The supervisor therefore lists the cases (`conformance-ts71
--list`), deals them into shards and runs each shard in a `conformance-ts71
--worker` process, which reports every step on stdout: `case <key>` before a
case, `begin <json>` before a lane-A configuration runs and `result <json>`
for every configuration. When a worker aborts, spends longer than `--timeout`
on one step or grows past `--max-rss-mib`, the supervisor records the
configuration it was running (or, outside one, its case) as a harness error
and restarts the worker after it. The merged report is written to
target/conformance-ts71/<profile>/report.json, and each shard's stderr to
shard-<n>.stderr beside it.

Every configuration checks on one checker, the exact reference. `--checkers <n>`
runs the sharded control instead, whose report is compared with the
one-checker report by hand (scripts/conformance_ts71_compare.py); it neither
checks nor updates the ratchet.

A configuration the native runner executes is lane A and is compared; the
ones its SkipUnsupportedCompilerOptions rules leave out are reported as
skipped, the ones it never produces baselines for as not run.

The ratchet ratchets/ts71/<profile>.tsv lists every lane-A configuration that
agrees with its baseline at least on locations, with the deepest tier it
reached (location < category < text < full) and, in a third column, its emit
tier: `js` when tsc-rs's JavaScript emit baseline (the `.js` reference, with
the declaration files) matches byte for byte, `none` otherwise. `--check`
fails when a listed configuration of the run is now below either tier;
`--update` records the run's tiers without lowering any.
"""

import argparse
import json
import pathlib
import signal
import subprocess
import sys
import threading
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/release/conformance-ts71"
TIERS = ("location", "category", "text", "full")
EMIT_TIERS = ("none", "js")
RATCHET_HEADER = (
    "# TypeScript 7.1 lane-A configurations, the deepest error tier each has reached and\n"
    "# its emit tier (scripts/conformance_ts71.py --update). A tier is lowered only by a reviewed edit.\n"
)


def rss_mib(pid):
    out = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True)
    try:
        return int(out.stdout.strip() or 0) // 1024
    except ValueError:
        return 0


def exit_detail(returncode):
    if returncode < 0:
        try:
            return f"killed by {signal.Signals(-returncode).name}"
        except ValueError:
            return f"killed by signal {-returncode}"
    return f"exit status {returncode}"


class Shard:
    """One worker slot: the cases it still has to run and its results."""

    def __init__(self, index, keys, args, out_dir, stop):
        self.index = index
        # A case key and the stems of it to leave out: those a killed worker
        # already reported and the one it failed on.
        self.remaining = [(key, []) for key in keys]
        self.args = args
        self.out_dir = out_dir
        self.stop = stop
        self.results = []
        self.failures = []
        self.error = None

    def run(self):
        try:
            with open(self.out_dir / f"shard-{self.index}.stderr", "w") as stderr:
                while self.remaining and not self.stop.is_set():
                    self.run_worker(stderr)
        except Exception as error:  # reported by main
            self.error = f"shard {self.index}: {error}"

    def run_worker(self, stderr):
        cases_file = self.out_dir / f"shard-{self.index}.cases"
        cases_file.write_text(
            "".join("\t".join([key, *leave_out]) + "\n" for key, leave_out in self.remaining)
        )
        process = subprocess.Popen(
            [str(BINARY), "--profile", self.args.profile, "--checkers", str(self.args.checkers),
             "--worker", str(cases_file)],
            cwd=ROOT, stdout=subprocess.PIPE, stderr=stderr,
            text=True, encoding="utf-8", errors="replace", bufsize=1,
        )
        state = {"case": None, "begin": None, "since": time.monotonic()}
        reported = {}  # case key -> the stems this process reported

        def reader():
            for line in process.stdout:
                if not line.endswith("\n"):
                    break  # cut off when the worker was killed
                event, _, payload = line[:-1].partition(" ")
                state["since"] = time.monotonic()
                if event == "case":
                    state["case"], state["begin"] = payload, None
                elif event == "begin":
                    state["begin"] = json.loads(payload)
                elif event == "result":
                    state["begin"] = None
                    result = json.loads(payload)
                    self.results.append(result)
                    reported.setdefault(state["case"], []).append(result["stem"])

        thread = threading.Thread(target=reader, daemon=True)
        thread.start()
        killed = None
        while process.poll() is None:
            time.sleep(0.5)
            if time.monotonic() - state["since"] > self.args.timeout:
                killed = f"timeout: no progress in {self.args.timeout:.0f} s"
            elif rss_mib(process.pid) > self.args.max_rss_mib:
                killed = f"memory limit: over {self.args.max_rss_mib} MiB resident"
            if killed:
                process.kill()
                process.wait()
                break
        thread.join()
        if process.returncode == 0:
            if state["case"] != self.remaining[-1][0]:
                raise RuntimeError(f"the worker exited after {state['case']}, before the last case")
            self.remaining = []
            return
        reason = killed or f"crash: {exit_detail(process.returncode)}"
        key = state["case"]
        if key is None:
            raise RuntimeError(f"the worker stopped before its first case ({reason})")
        position = [entry[0] for entry in self.remaining].index(key)
        begin = state["begin"]
        if begin is None:
            configuration, stem = "", ""
            reason += " outside a lane-A configuration"
            resume = []
        else:
            configuration, stem = begin["configuration"], begin["stem"]
            leave_out = self.remaining[position][1] + reported.get(key, []) + [stem]
            resume = [(key, leave_out)]
        suite, case = key.split("/", 1)
        self.results.append({
            "suite": suite, "case": case, "configuration": configuration, "stem": stem,
            "status": "harness-error", "reason": f"{reason}; the worker restarted after it",
        })
        self.failures.append(f"{stem or key}: {reason}")
        self.remaining = resume + self.remaining[position + 1:]


def summarize(results):
    summary = {"configurations": len(results), "lane_a": 0, "full": 0, "text": 0, "category": 0,
               "location": 0, "mismatch": 0, "emit_full": 0, "emit_mismatch": 0,
               "emit_not_assessed": 0, "harness_errors": 0, "skipped": 0, "not_run": 0}
    tiers = {"Full": "full", "Text": "text", "Category": "category", "Location": "location",
             "None": "mismatch"}
    for result in results:
        status = result["status"]
        if status == "compared":
            summary["lane_a"] += 1
            summary[tiers[result["agreement"]]] += 1
            summary[{"Full": "emit_full", "NotAssessed": "emit_not_assessed"}
                    .get(result.get("emit"), "emit_mismatch")] += 1
        elif status == "harness-error":
            summary["lane_a"] += 1
            summary["harness_errors"] += 1
        elif status == "skipped":
            summary["skipped"] += 1
        else:
            summary["not_run"] += 1
    return summary


def measured_tiers(results):
    """`suite/stem` -> (error tier, emit tier) of every configuration compared at
    location or deeper."""
    return {f"{result['suite']}/{result['stem']}":
            (result["agreement"].lower(), "js" if result.get("emit") == "Full" else "none")
            for result in results
            if result["status"] == "compared" and result["agreement"] != "None"}


def read_ratchet(path):
    """`suite/stem` -> (error tier, emit tier); a row without the emit column
    (written before the emit comparison existed) counts as `none`."""
    if not path.exists():
        return {}
    accepted = {}
    for line in path.read_text().splitlines():
        if line and not line.startswith("#"):
            key, tier, *rest = line.split("\t")
            accepted[key] = (tier, rest[0] if rest else "none")
    return accepted


def check_ratchet(path, results, filtered):
    """Print the accepted configurations now below their tier; False if any.

    A filtered run checks the configurations it reported; a full run checks
    every accepted configuration, so one missing from the run (its case
    failed before reaching it) counts as a regression."""
    ran = {f"{result['suite']}/{result['stem']}" for result in results}
    measured = measured_tiers(results)
    rank = {tier: index for index, tier in enumerate(TIERS)}
    emit_rank = {tier: index for index, tier in enumerate(EMIT_TIERS)}
    accepted = read_ratchet(path)
    regressions = []
    for key, (tier, emit) in accepted.items():
        if key not in ran and filtered:
            continue
        now_tier, now_emit = measured.get(key, ("none", "none"))
        if rank.get(now_tier, -1) < rank[tier]:
            regressions.append((key, f"accepted {tier}, now {now_tier}"))
        elif emit_rank[now_emit] < emit_rank[emit]:
            regressions.append((key, f"accepted emit {emit}, now {now_emit}"))
    regressions.sort()
    improved = sum(1 for key, (tier, emit) in measured.items()
                   if rank[tier] > rank.get(accepted.get(key, ("none", "none"))[0], -1)
                   or emit_rank[emit] > emit_rank[accepted.get(key, ("none", "none"))[1]])
    for key, detail in regressions:
        print(f"  regression: {key} {detail}")
    print(f"ratchet: {len(regressions)} regressions, {improved} configurations above "
          f"their accepted tiers ({path.relative_to(ROOT)})")
    return not regressions


def update_ratchet(path, results):
    rank = {tier: index for index, tier in enumerate(TIERS)}
    emit_rank = {tier: index for index, tier in enumerate(EMIT_TIERS)}
    accepted = read_ratchet(path)
    for key, (tier, emit) in measured_tiers(results).items():
        old_tier, old_emit = accepted.get(key, ("none", "none"))
        accepted[key] = (
            tier if rank[tier] > rank.get(old_tier, -1) else old_tier,
            emit if emit_rank[emit] > emit_rank[old_emit] else old_emit,
        )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(RATCHET_HEADER + "".join(
        f"{key}\t{accepted[key][0]}\t{accepted[key][1]}\n" for key in sorted(accepted)))
    print(f"ratchet: {len(accepted)} configurations recorded in {path.relative_to(ROOT)}")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--profile", default="7.1.0-dev-19dadef8")
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--timeout", type=float, default=120.0)
    parser.add_argument("--max-rss-mib", type=int, default=3072)
    parser.add_argument("--filter")
    parser.add_argument("--checkers", type=int, default=1)
    ratchet_mode = parser.add_mutually_exclusive_group()
    ratchet_mode.add_argument("--check", action="store_true")
    ratchet_mode.add_argument("--update", action="store_true")
    args = parser.parse_args()
    if not BINARY.exists():
        sys.exit("build first: cargo build --release -p tsc-rs-conformance --bin conformance-ts71")
    listing = [str(BINARY), "--profile", args.profile, "--list"]
    if args.filter:
        listing += ["--filter", args.filter]
    listed = subprocess.run(listing, cwd=ROOT, check=True, capture_output=True,
                            text=True, encoding="utf-8").stdout
    keys = [key for key in listed.split("\n") if key]
    if args.checkers != 1 and (args.check or args.update):
        sys.exit("--check/--update apply to the one-checker run only")
    out_dir = ROOT / "target/conformance-ts71" / args.profile
    if args.checkers != 1:
        out_dir = out_dir / f"checkers-{args.checkers}"
    out_dir.mkdir(parents=True, exist_ok=True)
    stop = threading.Event()
    shards = [Shard(index, keys[index::args.workers], args, out_dir, stop)
              for index in range(args.workers)]
    shards = [shard for shard in shards if shard.remaining]
    started = time.monotonic()
    threads = [threading.Thread(target=shard.run, daemon=True) for shard in shards]
    for thread in threads:
        thread.start()
    try:
        for thread in threads:
            thread.join()
    except KeyboardInterrupt:
        stop.set()
        sys.exit(130)
    errors = [shard.error for shard in shards if shard.error]
    if errors:
        sys.exit("\n".join(errors))
    results = sorted((result for shard in shards for result in shard.results),
                     key=lambda result: (result["suite"], result["case"], result["stem"]))
    summary = summarize(results)
    report = {"profile": args.profile, "summary": summary, "results": results}
    (out_dir / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"{len(keys)} cases in {time.monotonic() - started:.0f} s: {json.dumps(summary)}")
    for failure in sorted(failure for shard in shards for failure in shard.failures):
        print(f"  restarted after {failure}")
    print(f"report: {out_dir / 'report.json'}")
    ratchet = ROOT / "ratchets/ts71" / f"{args.profile}.tsv"
    if args.update:
        update_ratchet(ratchet, results)
    elif args.check and not check_ratchet(ratchet, results, args.filter is not None):
        sys.exit(1)


if __name__ == "__main__":
    main()
