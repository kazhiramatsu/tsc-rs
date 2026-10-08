#!/usr/bin/env python3
"""Hosted CI for the TypeScript 7.1 line: planning, the two jobs and the gate.

usage: replay.py plan | rust | conformance-ts71 | gate ci

`plan` reads the changed paths of the event (pull request, merge group or
push) and selects the jobs. A change under docs/ or to the root README.md,
CONTRIBUTING.md or LICENSE selects nothing; any other change, and an unknown
change range, selects both `rust` (formatting, Clippy and the Rust test
targets) and `conformance-ts71` (the TypeScript 7.1 conformance comparison:
the error, emit, type, symbol, source-map and trace baselines on one
checker, then the suites outside the compiler runner (transpile), each
checked against its ratchet in ratchets/ts71/). `gate ci` requires every
selected job to have succeeded and every unselected job to have been
skipped.
"""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DOCUMENTATION = ("README.md", "CONTRIBUTING.md", "LICENSE")
CONFORMANCE_TS71_WORKERS = "4"
JOBS = {"rust": "has_rust", "conformance-ts71": "has_conformance_ts71"}
# The Rust checks the hosted job runs: formatting, Clippy over every target,
# every workspace test target, and the generated diagnostic catalog.
RUST_CHECKS = (
    ["cargo", "fmt", "--all", "--", "--check"],
    ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
    ["cargo", "test", "--workspace"],
    ["cargo", "xtask", "codegen", "diagnostics-check"],
)


def changed_paths(event_name, event, root=ROOT):
    """The paths the event changed, or None when the range is unknown."""
    if event_name == "pull_request":
        base = event.get("pull_request", {}).get("base", {}).get("sha")
    elif event_name == "merge_group":
        base = event.get("merge_group", {}).get("base_sha")
    elif event_name == "push":
        base = event.get("before")
    else:
        return None
    if not isinstance(base, str) or not re.fullmatch(r"[0-9a-f]{40}", base) or base == "0" * 40:
        return None
    try:
        result = subprocess.run(
            ["git", "diff", "--no-renames", "--name-only", "-z", base, "HEAD", "--"],
            cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True,
        )
        # Both old and new paths of renames; decoded strictly, so unfamiliar
        # input selects everything.
        return [file for file in result.stdout.decode("utf-8").split("\0") if file]
    except (subprocess.CalledProcessError, UnicodeError, OSError):
        return None


def is_documentation(path):
    return path.startswith("docs/") or path in DOCUMENTATION


def selection(paths):
    """Which jobs the changed paths select."""
    if paths is None:
        return {"rust": True, "conformance_ts71": True, "reason": "unknown change range"}
    selecting = [path for path in paths if not is_documentation(path)]
    if not selecting:
        return {"rust": False, "conformance_ts71": False, "reason": "documentation only"}
    return {"rust": True, "conformance_ts71": True, "reason": f"changed: {selecting[0]}"}


def verify_gate(needs):
    if set(needs) != {"plan", *JOBS} or needs["plan"].get("result") != "success":
        raise ValueError("planning did not succeed")
    for job, output in JOBS.items():
        selected = needs["plan"].get("outputs", {}).get(output)
        if selected not in ("true", "false"):
            raise ValueError(f"missing selection for {job}")
        expected = "success" if selected == "true" else "skipped"
        if needs[job].get("result") != expected:
            raise ValueError(f"{job}: expected {expected}, got {needs[job].get('result')}")


def run(command):
    print("+ " + " ".join(command), flush=True)
    subprocess.run(command, cwd=ROOT, check=True)


def rust():
    for command in RUST_CHECKS:
        run(command)


def conformance_ts71():
    """Build the release runners and compare lane A, then the other suites,
    with the vendored 7.1 baselines."""
    run(["cargo", "build", "--release", "-p", "tsc-rs-conformance",
         "--bin", "conformance-ts71", "--bin", "suites-ts71"])
    run([sys.executable, "scripts/conformance_ts71.py", "--workers", CONFORMANCE_TS71_WORKERS, "--check"])
    run([sys.executable, "scripts/suites_ts71.py", "--check"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("plan", "rust", "conformance-ts71", "gate"))
    parser.add_argument("value", nargs="?")
    args = parser.parse_args()
    if args.command == "plan":
        subprocess.run([sys.executable, "-m", "unittest", "discover", "-s", ".github/ci",
                        "-p", "test_replay.py"], cwd=ROOT, check=True)
        event_path = os.environ.get("GITHUB_EVENT_PATH")
        try:
            event = json.loads(Path(event_path).read_text()) if event_path else {}
        except (OSError, ValueError):
            event = {}
        plan = selection(changed_paths(os.environ.get("GITHUB_EVENT_NAME"), event))
        print(json.dumps(plan, indent=2), flush=True)
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            output.write(f"has_rust={str(plan['rust']).lower()}\n")
            output.write(f"has_conformance_ts71={str(plan['conformance_ts71']).lower()}\n")
    elif args.command == "rust":
        rust()
    elif args.command == "conformance-ts71":
        conformance_ts71()
    else:
        if args.value != "ci":
            parser.error("gate takes the workflow name ci")
        verify_gate(json.loads(os.environ["REPLAY_NEEDS"]))
        print("ci gate passed")


if __name__ == "__main__":
    main()
