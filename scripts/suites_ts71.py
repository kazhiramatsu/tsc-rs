#!/usr/bin/env python3
"""Compare the TypeScript 7.1 suites outside the compiler runner and check the ratchet.

usage: suites_ts71.py [--profile <name>] [--filter <path substring>]
                      [--dump <directory>] [--check | --update]

Runs target/release/suites-ts71 (build it first: cargo build --release -p
tsc-rs-conformance --bin suites-ts71), which renders every baseline of the
transpile suite (testrunner/transpile_runner.go) and of the command-line and
tsconfig parsing tests (tsoptions/commandlineparser_test.go,
tsoptions/tsconfigparsing_test.go), compares it with the vendored reference
byte for byte and writes target/suites-ts71/<profile>/report.json. --dump writes the produced
baselines that differ under <directory>/<suite>/.

The ratchet ratchets/ts71/suites-<profile>.tsv lists every baseline of the
suites as `<suite>/<baseline>` with its tier: `full` when tsc-rs's baseline
matches the reference byte for byte, `none` otherwise. `--check` fails when
a listed baseline the run produced is now below its tier and, in a run
without --filter, when a `full` baseline is missing from the run; `--update`
records the run's tiers without lowering any.
"""

import argparse
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/release/suites-ts71"
TIERS = ("none", "full")
RATCHET_HEADER = (
    "# TypeScript 7.1 suites outside the compiler runner: every baseline and its tier, `full` when tsc-rs's\n"
    "# baseline matches the reference byte for byte (scripts/suites_ts71.py --update). A tier is lowered only by a reviewed edit.\n"
)


def measured_tiers(results):
    """`suite/baseline` -> tier of every baseline the run produced or expected."""
    return {f"{result['suite']}/{result['baseline']}":
            "full" if result["outcome"] == "full" else "none"
            for result in results}


def read_ratchet(path):
    if not path.exists():
        return {}
    accepted = {}
    for line in path.read_text().splitlines():
        if line and not line.startswith("#"):
            key, tier = line.split("\t")
            accepted[key] = tier
    return accepted


def check_ratchet(path, results, filtered):
    """Print the accepted baselines now below their tier; False if any."""
    rank = {tier: index for index, tier in enumerate(TIERS)}
    measured = measured_tiers(results)
    accepted = read_ratchet(path)
    regressions = []
    for key, tier in sorted(accepted.items()):
        if key not in measured:
            if not filtered and tier != "none":
                regressions.append((key, f"accepted {tier}, missing from the run"))
            continue
        if rank[measured[key]] < rank[tier]:
            regressions.append((key, f"accepted {tier}, now {measured[key]}"))
    improved = sum(1 for key, tier in measured.items()
                   if rank[tier] > rank.get(accepted.get(key, "none"), 0))
    unlisted = sum(1 for key in measured if key not in accepted)
    for key, detail in regressions:
        print(f"  regression: {key} {detail}")
    print(f"ratchet: {len(regressions)} regressions, {improved} baselines above their accepted "
          f"tiers, {unlisted} not listed ({path.relative_to(ROOT)})")
    return not regressions


def update_ratchet(path, results):
    rank = {tier: index for index, tier in enumerate(TIERS)}
    accepted = read_ratchet(path)
    for key, tier in measured_tiers(results).items():
        old = accepted.get(key, "none")
        accepted[key] = tier if rank[tier] > rank[old] else old
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(RATCHET_HEADER + "".join(
        f"{key}\t{accepted[key]}\n" for key in sorted(accepted)))
    print(f"ratchet: {len(accepted)} baselines recorded in {path.relative_to(ROOT)}")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--profile", default="7.1.0-dev-19dadef8")
    parser.add_argument("--filter")
    parser.add_argument("--dump", help="write the differing baselines under this directory")
    ratchet_mode = parser.add_mutually_exclusive_group()
    ratchet_mode.add_argument("--check", action="store_true")
    ratchet_mode.add_argument("--update", action="store_true")
    args = parser.parse_args()
    if not BINARY.exists():
        sys.exit("build first: cargo build --release -p tsc-rs-conformance --bin suites-ts71")
    if args.update and args.filter:
        sys.exit("--update records a run without --filter only")
    command = [str(BINARY), "--profile", args.profile]
    if args.filter:
        command += ["--filter", args.filter]
    if args.dump:
        command += ["--dump", str(pathlib.Path(args.dump).resolve())]
    run = subprocess.run(command, cwd=ROOT)
    if run.returncode != 0:
        sys.exit(f"suites-ts71 exited with status {run.returncode}")
    report_path = ROOT / "target/suites-ts71" / args.profile / "report.json"
    results = json.loads(report_path.read_text())["results"]
    ratchet = ROOT / "ratchets/ts71" / f"suites-{args.profile}.tsv"
    if args.update:
        update_ratchet(ratchet, results)
    elif args.check and not check_ratchet(ratchet, results, args.filter is not None):
        sys.exit(1)


if __name__ == "__main__":
    main()
