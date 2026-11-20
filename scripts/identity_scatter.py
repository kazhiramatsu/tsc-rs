#!/usr/bin/env python3
"""Run the suites under the identity scatter.

usage: identity_scatter.py [--seeds 1,2] [--packages <name>,...] [--jobs N]
                           [--test-threads N] [--logs <directory>]

No output may depend on the order of identities across files: the loader
gives the sources it finds through imports their identities in the order
its workers reach them, which differs from run to run. The cargo feature
`identity-scatter` of tsc-rs-types (crates/types/src/identity.rs) makes that
order follow a seed: with TSRS_IDENTITY_SCATTER=<seed> every range of a
tree's or a symbol table's identities lands where the seed puts it, and
every source has a room of its own.

Builds the suites of the packages with the feature (in target/, as an
ordinary `cargo test` build of its own feature set) and runs them once
without the variable and once per seed. A test that passes without the
variable and fails with it depends on the order: the script lists such
tests and exits 1. crates/compiler/tests/identity_scatter.rs fails in a run
that is not in the state the script runs it in, so "passes under the
scatter" cannot come from a run that did not scatter.

Not for the suites of tsc-rs-types, tsc-rs-syntax, tsc-rs-binder and
tsc-rs-program: their placement tests assert where ranges land.
"""

import argparse
import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
PACKAGES = ("tsc-rs-compiler", "tsc-rs-incremental", "tsc-rs-project",
            "tsc-rs-ls", "tsc-rs-lsp", "tsc-rs-cli")
RESULT = re.compile(r"^test (\S+) \.\.\. (ok|FAILED|ignored)", re.MULTILINE)
BINARY = re.compile(r"^\s+Running (\S+)", re.MULTILINE)


def run_suites(packages, jobs, test_threads, seed, log):
    """The (binary, test) names that passed and failed in one run."""
    command = ["cargo", "test", "--locked", "-j", str(jobs)]
    for package in packages:
        command += ["-p", package]
    command += ["--features", "tsc-types/identity-scatter", "--no-fail-fast",
                "--", f"--test-threads={test_threads}"]
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith("TSRS_IDENTITY_SCATTER")}
    environment["TSRS_IDENTITY_SCATTER_EXPECTED"] = "off" if seed is None else "on"
    if seed is not None:
        environment["TSRS_IDENTITY_SCATTER"] = str(seed)
    completed = subprocess.run(command, cwd=ROOT, env=environment, text=True,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    log.write_text(completed.stdout)
    passed, failed = set(), set()
    binary = ""
    for line in completed.stdout.splitlines():
        match = BINARY.match(line)
        if match:
            binary = match.group(1)
            continue
        match = RESULT.match(line)
        if match and match.group(2) != "ignored":
            (passed if match.group(2) == "ok" else failed).add((binary, match.group(1)))
    return completed.returncode, passed, failed


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--seeds", default="1,2")
    parser.add_argument("--packages", default=",".join(PACKAGES))
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--test-threads", type=int, default=4)
    parser.add_argument("--logs", type=pathlib.Path,
                        default=ROOT / "target/identity-scatter")
    arguments = parser.parse_args()
    packages = arguments.packages.split(",")
    seeds = [int(seed) for seed in arguments.seeds.split(",")]
    arguments.logs.mkdir(parents=True, exist_ok=True)

    status, passed, failed = run_suites(packages, arguments.jobs, arguments.test_threads,
                                        None, arguments.logs / "plain.log")
    print(f"without the variable: exit {status}, {len(passed)} passed, {len(failed)} failed")
    if not passed:
        print(f"no test ran: see {arguments.logs / 'plain.log'}")
        return 1
    verdict = 0
    if failed:
        verdict = 1
        for binary, test in sorted(failed):
            print(f"  FAILED without the scatter: {test} ({binary})")
    for seed in seeds:
        log = arguments.logs / f"seed-{seed}.log"
        status, scattered, scattered_failed = run_suites(
            packages, arguments.jobs, arguments.test_threads, seed, log)
        only = sorted(scattered_failed & passed)
        missing = sorted(passed - scattered - scattered_failed)
        print(f"TSRS_IDENTITY_SCATTER={seed}: exit {status}, {len(scattered)} passed, "
              f"{len(scattered_failed)} failed, {len(only)} of them pass without the variable, "
              f"{len(missing)} did not run")
        for binary, test in only:
            print(f"  DEPENDS ON THE ORDER OF IDENTITIES: {test} ({binary})")
        for binary, test in missing:
            print(f"  did not run under the scatter: {test} ({binary})")
        if only or missing or scattered_failed - passed:
            verdict = 1
    print("no test depends on the order of identities" if verdict == 0
          else f"see the logs in {arguments.logs}")
    return verdict


if __name__ == "__main__":
    sys.exit(main())
