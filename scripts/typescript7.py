#!/usr/bin/env python3
"""Run pinned native TypeScript reference tests; never update accepted baselines."""

import argparse
import datetime
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
WORK = ROOT / "target/typescript7"
UPSTREAM = WORK / "upstream"
PIN = "1f70213d4922b434345f639b441681e470c7cfc1"
REMOTE = "https://github.com/microsoft/TypeScript.git"
TOOLCHAIN = "go1.26.0"


def git(*args):
    return subprocess.check_output(
        ["git", "-C", str(UPSTREAM), *args], text=True
    ).strip()


def environment():
    env = os.environ.copy()
    env.update(
        GOTOOLCHAIN=TOOLCHAIN,
        GOCACHE=str(WORK / "go-build"),
        GOMODCACHE=str(WORK / "go-mod"),
        GOPATH=str(WORK / "go"),
        GOMAXPROCS="2",
        CGO_ENABLED="0",
        GOFLAGS="",
        GOWORK=str(UPSTREAM / "go.work"),
    )
    return env


def setup():
    WORK.mkdir(parents=True, exist_ok=True)
    if not UPSTREAM.exists():
        subprocess.run(
            ["git", "clone", "--filter=blob:none", "--no-checkout", "--depth", "1",
             "--single-branch", REMOTE, str(UPSTREAM)], check=True
        )
        found = subprocess.run(
            ["git", "-C", str(UPSTREAM), "cat-file", "-e", PIN + "^{commit}"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        if found.returncode:
            subprocess.run(
                ["git", "-C", str(UPSTREAM), "fetch", "--depth", "1", "origin", PIN],
                check=True,
            )
        subprocess.run(
            ["git", "-C", str(UPSTREAM), "checkout", "--detach", PIN], check=True
        )
    check_source()
    subprocess.run(["go", "version"], env=environment(), check=True)
    print(f"Reference: {UPSTREAM}\nCommit: {PIN}")


def check_source():
    if not (UPSTREAM / "tsc/go.mod").is_file():
        raise ValueError("Run `python3 scripts/typescript7.py setup` first.")
    if git("rev-parse", "HEAD") != PIN:
        raise ValueError(f"Reference HEAD differs from {PIN}; preserve it and use a separate checkout.")


def run_recorded(args, test_prefix=None):
    WORK.mkdir(parents=True, exist_ok=True)
    with (WORK / "runner.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise ValueError("Another reference run is active in this checkout.") from None
        return _run_recorded(args, test_prefix)


def _run_recorded(args, test_prefix):
    check_source()
    env = environment()
    command = ["go", "-C", str(UPSTREAM / "tsc"), *args]
    # Bound both compiler workers and tests; demote locally on macOS.
    if sys.platform == "darwin" and shutil.which("taskpolicy"):
        command = ["taskpolicy", "-b", "nice", "-n", "15", *command]
    logs = WORK / "logs"
    logs.mkdir(parents=True, exist_ok=True)
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ-")
    record = Path(tempfile.mkdtemp(prefix=stamp, dir=logs))
    tracking = record / "baseline-tracking"
    tracking.mkdir()
    env["TSGO_BASELINE_TRACKING_DIR"] = str(tracking)
    local = UPSTREAM / "tsc/testdata/baselines/local"
    before = {p: p.stat().st_mtime_ns for p in local.rglob("*") if p.is_file()}
    version = subprocess.check_output(["go", "version"], env=env, text=True).strip()
    metadata = {
        "upstream_commit": PIN, "toolchain": version, "command": command,
        "source_status": git("status", "--porcelain"),
        "trace_file": env.get("TSRS_TRACE_FILE"),
        "trace_stack": env.get("TSRS_TRACE_STACK"),
        "environment": {key: env[key] for key in (
            "GOTOOLCHAIN", "GOCACHE", "GOMODCACHE", "GOPATH", "GOMAXPROCS",
            "CGO_ENABLED", "GOFLAGS", "GOWORK",
            "TSGO_BASELINE_TRACKING_DIR",
        )},
    }
    (record / "source.patch").write_bytes(subprocess.check_output(
        ["git", "-C", str(UPSTREAM), "diff", "--binary", "HEAD"]
    ))
    print(f"Log: {record}", flush=True)
    started = time.monotonic()
    terminal = {}
    with (record / "output.log").open("w") as log:
        proc = subprocess.Popen(command, env=env, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, text=True)
        for line in proc.stdout:
            log.write(line)
            if test_prefix is None:
                print(line, end="", flush=True)
                continue
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                print(line, end="", flush=True)
                continue
            if "Output" in event:
                print(event["Output"], end="", flush=True)
            name = event.get("Test", "")
            if name.startswith(test_prefix) and event.get("Action") in ("pass", "fail", "skip"):
                terminal[name] = event["Action"]
        result = proc.wait()
    # A passing parent with zero matched children is not a passing reference case.
    if test_prefix is not None and (not terminal or any(v != "pass" for v in terminal.values())):
        result = result or 1
        print("Reference selection failed: zero cases, a skip, or a failed case.", file=sys.stderr)
    # The native baseline writer can emit a .delete marker without failing
    # the Go test. Keep changed outputs, including deletions, visible and red.
    baselines = {name for file in tracking.glob("*.txt") for name in file.read_text().splitlines()}
    changes = []
    for name in sorted(baselines):
        for relative in (name, name + ".delete"):
            path = local / relative
            if path.is_file() and before.get(path) != path.stat().st_mtime_ns:
                destination = record / "baseline-diffs" / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, destination)
                changes.append(relative)
    if changes:
        result = result or 1
        print(f"Reference baseline changes: {changes}", file=sys.stderr)
    cases = [name for name in terminal if "/" not in name[len(test_prefix):]] if test_prefix else []
    metadata.update(exit_code=result, elapsed_seconds=round(time.monotonic() - started, 3),
                    cases=cases, tests=terminal, baseline_changes=changes)
    (record / "result.json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(f"Result: exit={result}, cases={len(cases)}, elapsed={metadata['elapsed_seconds']}s")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    sub.add_parser("setup", help="fetch the fixed source and Go toolchain")
    build = sub.add_parser("build", help="build a compiler with embedded standard libraries")
    build.add_argument("--debug", action="store_true", help="disable optimization/inlining for Delve")
    compiler = sub.add_parser("compiler", help="run all variants of one official .ts/.tsx fixture")
    compiler.add_argument("case", help="exact basename, including .ts or .tsx")
    fourslash = sub.add_parser("fourslash", help="run one native FourSlash Go test")
    fourslash.add_argument("case", help="exact Go function name, e.g. TestBasicEdit")
    args = parser.parse_args()
    if args.action == "setup":
        setup()
        return 0
    if args.action == "build":
        binary = WORK / "bin" / ("tsc-debug" if args.debug else "tsc")
        binary.parent.mkdir(parents=True, exist_ok=True)
        flags = ["-gcflags=all=-N -l"] if args.debug else []
        return run_recorded(["build", "-p=2", *flags, "-o", str(binary), "./cmd/tsc"])
    check_source()
    if args.action == "compiler":
        if Path(args.case).name != args.case or not args.case.endswith((".ts", ".tsx")):
            raise ValueError("Specify one fixture basename with its .ts/.tsx extension.")
        fixtures = [p for group in ("compiler", "conformance")
                    for p in (UPSTREAM / "tsc/testdata/tests/cases" / group).rglob("*")
                    if p.is_file() and p.name == args.case]
        if len(fixtures) != 1:
            raise ValueError(f"Expected one official fixture, found {len(fixtures)}: {args.case}")
        # Go replaces the space before the configuration name with an underscore.
        # Let the upstream parser expand every option combination.
        pattern = "^TestLocal$/^" + re.escape(args.case) + "(_.*)?$"
        package, prefix = "./internal/testrunner", "TestLocal/"
    else:
        if not re.fullmatch(r"Test[A-Za-z0-9_]+", args.case):
            raise ValueError("Specify an exact Go test function name, not a regular expression.")
        pattern = "^" + args.case + "$"
        package, prefix = "./internal/fourslash/tests", args.case
    return run_recorded(
        ["test", "-p=2", "-parallel=1", "-count=1", "-timeout=5m", "-json",
         "-run=" + pattern, package], test_prefix=prefix,
    )


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"typescript7: {error}", file=sys.stderr)
        sys.exit(1)
