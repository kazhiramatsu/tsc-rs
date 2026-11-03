#!/usr/bin/env python3
"""Build offline tsgo entry counters, run serial benchmarks, or compare JSON counts."""

import argparse
import csv
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shlex
import shutil
import stat
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "scripts/tsgo_census"
WORK = ROOT / "target/tsgo-census"
UPSTREAM = WORK / "upstream"
BINARY = WORK / "tsc"
MANIFEST = ASSETS / "functions.tsv"
PIN = "aa8149273b23401f0a79a5f0384c42de51888693"
REFERENCE = Path("/Users/hiramatsu/dev/tsc-rs/target/typescript7")
BENCH = Path("/Users/hiramatsu/dev/bench-corpora")
PROJECTS = {
    "hono": "hono",
    "zod": "zod",
    "effect": "effect.noindex/packages/effect",
    "ts": "TypeScript-6.0/src/compiler",
    "next": "next.js/packages/next",
    "vscode": "vscode/src",
    "playwright": "playwright",
    "vue": "vue",
}


def local_work():
    # Never follow an existing target symlink into another checkout.
    if not WORK.resolve().is_relative_to(ROOT):
        raise ValueError(f"work directory escapes this clone: {WORK.resolve()}")
    WORK.mkdir(parents=True, exist_ok=True)
    for path in WORK.iterdir():
        # Upstream may have legitimate internal links; only write destinations
        # are checked here, rather than traversing the whole 1.5 GB copy.
        if path.is_symlink():
            raise ValueError(f"unexpected symlink in work directory: {path}")


def manifest_rows():
    rows = []
    with MANIFEST.open(newline="") as source:
        for cols in csv.reader(source, delimiter="\t"):
            if not cols or cols[0].startswith("#"):
                continue
            if len(cols) != 3:
                raise ValueError(f"expected three columns in {MANIFEST}: {cols}")
            symbol = cols[0].split(":", 1)[1].split(".")[-1]
            rows.append((symbol[0].lower() + symbol[1:], *cols))
    return rows


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def remove_private_tree(path):
    if path.parent != WORK or path.is_symlink():
        raise ValueError(f"refusing to remove unexpected path: {path}")
    if path.exists():
        # Go's extracted module directories can be read-only.
        for parent, dirs, _ in os.walk(path):
            os.chmod(parent, os.stat(parent).st_mode | stat.S_IWUSR)
        shutil.rmtree(path)


def environment():
    env = os.environ.copy()
    env.pop("TSGO_CENSUS", None)
    env.update(
        GOTOOLCHAIN="go1.27.1",
        CGO_ENABLED="0",
        GOFLAGS="",
        GOMAXPROCS="2",
        GOWORK=str(UPSTREAM / "go.work"),
        GOCACHE=str(WORK / "go-build"),
        GOMODCACHE=str(WORK / "go-mod"),
        GOPATH=str(WORK / "go"),
        # All dependencies and the toolchain must come from the copied cache.
        GOPROXY="off",
        # Go refuses to authenticate a downloaded toolchain with GOSUMDB=off,
        # even when its zip is already present. The copied cache has its signed
        # sum.golang.org lookup and tiles, so verification is entirely local.
        GOSUMDB="sum.golang.org",
    )
    return env


def nice_prefix(records):
    command = ["nice", "-n", "20", "true"]
    try:
        proc = subprocess.run(command, capture_output=True)
        records.append({"command": command, "exit_code": proc.returncode,
                        "stderr": proc.stderr.decode(errors="replace")})
        if proc.returncode == 0 and not proc.stderr:
            return command[:-1]
    except FileNotFoundError:
        records.append({"command": command, "error": "nice not available"})
    print("census: nice unavailable/refused; running directly", file=sys.stderr)
    return []


def recorded(command, cwd, env, stdout, stderr, records):
    print(f"census: cd {shlex.quote(str(cwd))} && {shlex.join(map(str, command))}",
          file=sys.stderr, flush=True)
    started = time.monotonic()
    with stdout.open("wb") as out, stderr.open("wb") as err:
        proc = subprocess.run(command, cwd=cwd, env=env, stdout=out, stderr=err)
    result = {
        "command": list(map(str, command)), "cwd": str(cwd),
        "exit_code": proc.returncode,
        "elapsed_seconds": round(time.monotonic() - started, 3),
        "stdout": str(stdout), "stderr": str(stderr),
    }
    records.append(result)
    print(f"census: exit={proc.returncode}, elapsed={result['elapsed_seconds']}s",
          file=sys.stderr, flush=True)
    return result


def build(args):
    started = time.monotonic()
    records = []
    metadata = {"pin": PIN, "source": str(args.source),
                "module_cache_source": str(args.module_cache), "commands": records}
    try:
        for command in (["git", "-C", str(args.source), "rev-parse", "HEAD"],
                        ["git", "-C", str(args.source), "status", "--porcelain"]):
            proc = subprocess.run(command, capture_output=True, text=True)
            records.append({"command": command, "exit_code": proc.returncode,
                            "stdout": proc.stdout, "stderr": proc.stderr})
            if proc.returncode:
                raise ValueError(proc.stderr.strip())
        if records[0]["stdout"].strip() != PIN or records[1]["stdout"].strip():
            raise ValueError(f"source must be a clean checkout at {PIN}")
        if not (args.module_cache / "golang.org/toolchain@v0.0.1-go1.27.1.darwin-arm64").exists():
            # The path varies by platform; GOTOOLCHAIN validates the exact version.
            if not list(args.module_cache.glob("golang.org/toolchain@v0.0.1-go1.27.1.*")):
                raise ValueError("source module cache does not contain go1.27.1")
        # Remove the previous binary before doing anything that could fail.
        BINARY.unlink(missing_ok=True)
        for name in ("upstream", "go-build"):
            remove_private_tree(WORK / name)
        print("census: copying clean upstream working tree (without .git)", file=sys.stderr, flush=True)
        shutil.copytree(args.source, UPSTREAM, symlinks=True,
                        ignore=shutil.ignore_patterns(".git"))
        cache = WORK / "go-mod"
        if not cache.exists():
            print("census: copying offline module/toolchain cache", file=sys.stderr, flush=True)
            staging = WORK / "go-mod-copying"
            remove_private_tree(staging)
            shutil.copytree(args.module_cache, staging)
            staging.rename(cache)
        (WORK / "go-build").mkdir()
        env = environment()
        metadata["environment"] = {k: env[k] for k in (
            "GOTOOLCHAIN", "CGO_ENABLED", "GOFLAGS", "GOMAXPROCS", "GOWORK",
            "GOCACHE", "GOMODCACHE", "GOPATH", "GOPROXY", "GOSUMDB")}
        metadata["manifest_sha256"] = hashlib.sha256(MANIFEST.read_bytes()).hexdigest()
        prefix = nice_prefix(records)
        commands = [
            ["go", "run", "-p=2", str(ASSETS / "rewrite.go"),
             "--root", str(UPSTREAM / "tsc"), "--manifest", str(MANIFEST),
             "--package-source", str(ASSETS / "census.go"),
             "--report", str(WORK / "rewrite.json")],
            ["go", "build", "-p=2", "-o", str(BINARY), "./cmd/tsc"],
        ]
        for label, command in zip(("rewrite", "build"), commands):
            result = recorded(prefix + command, UPSTREAM / "tsc", env,
                              WORK / f"{label}.stdout", WORK / f"{label}.stderr", records)
            if result["exit_code"]:
                print((WORK / f"{label}.stderr").read_text(errors="replace"), file=sys.stderr)
                return result["exit_code"]
        metadata["rewrite"] = json.loads((WORK / "rewrite.json").read_text())
        print(json.dumps(metadata["rewrite"], indent=2))
        return 0
    finally:
        metadata["elapsed_seconds"] = round(time.monotonic() - started, 3)
        metadata["binary_exists"] = BINARY.is_file()
        write_json(WORK / "build.json", metadata)
        print(f"census: build total={metadata['elapsed_seconds']}s", file=sys.stderr)


def read_counts(path):
    data = json.loads(path.read_text())
    if not isinstance(data, dict) or any(not isinstance(k, str) or type(v) is not int or v < 0
                                         for k, v in data.items()):
        raise ValueError(f"expected an object of nonnegative integer entry counts: {path}")
    return data


def run(args):
    if not BINARY.is_file():
        raise ValueError("run build first")
    build_info = json.loads((WORK / "build.json").read_text())
    if build_info["manifest_sha256"] != hashlib.sha256(MANIFEST.read_bytes()).hexdigest():
        raise ValueError("functions.tsv changed since build; rebuild first")
    report = build_info["rewrite"]
    expected = {entry["name"] for entry in report["instrumented"]}
    counts_dir, logs = WORK / "counts", WORK / "runs"
    counts_dir.mkdir(exist_ok=True)
    logs.mkdir(exist_ok=True)
    records = []
    prefix = nice_prefix(records)
    summary = {"pin": PIN, "commands": records, "projects": {}, "missing": report["missing"]}
    failed = False
    try:
        for project in args.projects or PROJECTS:
            root = args.bench_root / PROJECTS[project]
            variant = "stable" if project == "vscode" else "noemit"
            command_args = ["-p", f"tsconfig.bench-{variant}.json", "--pretty", "false", "--singleThreaded"]
            if not (root / command_args[1]).is_file():
                raise ValueError(f"benchmark config missing: {root / command_args[1]}")
            output = counts_dir / f"{project}.json"
            output.unlink(missing_ok=True)
            env = os.environ.copy()
            env["TSGO_CENSUS"] = str(output)
            result = recorded(prefix + [str(BINARY), *command_args], root, env,
                              logs / f"{project}.stdout", logs / f"{project}.stderr", records)
            summary["projects"][project] = dict(result, counts=str(output))
            counts = read_counts(output)
            if set(counts) != expected or not any(counts.values()):
                raise ValueError(f"incomplete or empty census: {output}")
            # Nonzero tsc status normally means benchmark diagnostics; retain it
            # in metadata, keep collecting, and return the first compiler failure.
            if args.check_stdout and project == "hono":
                checks = []
                env.pop("TSGO_CENSUS")
                for label, binary in (("disabled", BINARY), ("reference", args.reference_binary)):
                    check = recorded(prefix + [str(binary), *command_args], root, env,
                                     logs / f"hono.{label}.stdout", logs / f"hono.{label}.stderr", records)
                    identical = {"exit_code": check["exit_code"] == result["exit_code"]}
                    for stream in ("stdout", "stderr"):
                        content = Path(result[stream]).read_bytes()
                        identical[stream] = content == Path(check[stream]).read_bytes()
                        identical[f"{stream}_sha256"] = hashlib.sha256(content).hexdigest()
                        identical[f"{stream}_bytes"] = len(content)
                    checks.append({"against": label, **identical})
                    failed |= not all(identical[k] for k in ("exit_code", "stdout", "stderr"))
                summary["stdout_identity_hono"] = checks
                print(json.dumps(checks, indent=2))
            print(f"{project}: {len(counts)} counters, compiler exit={result['exit_code']}, "
                  f"{result['elapsed_seconds']}s, {output}", flush=True)
        if failed:
            return 1
        return next((r["exit_code"] for r in summary["projects"].values() if r["exit_code"]), 0)
    finally:
        write_json(WORK / "run.json", summary)


def normalize(data):
    rows = manifest_rows()
    aliases = {}
    patterns = []
    for name, source, rust, _ in rows:
        symbol = source.split(":", 1)[1].split(".")[-1]
        for alias in (name, source, symbol):
            aliases[alias] = name
        if rust.startswith("re:"):
            patterns.append((name, re.compile(rust[3:])))
        elif rust:
            aliases[rust] = name
            # Accept demangled LLVM profile names, including monomorphizations
            # and the optional legacy Rust hash, but never closure child entries.
            patterns.append((name, re.compile(r"(?:^|::)" + re.escape(rust)
                                               + r"(?:::<.*>)?(?:::h[0-9a-f]{16})?$")))
    result = {}
    for symbol, count in data.items():
        if symbol in aliases:
            name = aliases[symbol]
        else:
            matches = {name for name, pattern in patterns if pattern.search(symbol)}
            if len(matches) > 1:
                raise ValueError(f"ambiguous function mapping: {symbol}: {sorted(matches)}")
            name = next(iter(matches), symbol)
        result[name] = result.get(name, 0) + count
    return result


def compare(args):
    a, b = normalize(read_counts(args.a)), normalize(read_counts(args.b))
    rows = []
    for name in a.keys() | b.keys():
        x, y = a.get(name), b.get(name)
        if x is None or y is None:
            ratio, score = "missing", math.inf
        else:
            value = y / x if x else (1.0 if y == 0 else math.inf)
            ratio, score = ("inf" if math.isinf(value) else f"{value:.6f}"), abs(value - 1)
        rows.append((score, name, x, y, ratio))
    width = max([len("function")] + [len(row[1]) for row in rows])
    print(f"{'function':<{width}} {'count A':>16} {'count B':>16} {'B / A':>12}")
    for _, name, x, y, ratio in sorted(rows, key=lambda row: (-row[0], row[1])):
        print(f"{name:<{width}} {str(x) if x is not None else '-':>16} "
              f"{str(y) if y is not None else '-':>16} {ratio:>12}")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest="action", required=True)
    builder = actions.add_parser("build", help="copy pinned source and offline cache, rewrite, build")
    builder.add_argument("--source", type=Path, default=REFERENCE / "upstream")
    builder.add_argument("--module-cache", type=Path, default=REFERENCE / "go-mod")
    runner = actions.add_parser("run", help="run all eight benchmarks serially with --singleThreaded")
    runner.add_argument("--projects", nargs="+", choices=PROJECTS, help="default: all eight")
    runner.add_argument("--bench-root", type=Path, default=BENCH)
    runner.add_argument("--check-stdout", action="store_true", help="compare hono with census disabled and pinned reference")
    runner.add_argument("--reference-binary", type=Path, default=REFERENCE / "bin/tsc-aa814927")
    comparer = actions.add_parser("compare", help="compare JSON counts; ratio is B/A, descending |ratio-1|")
    comparer.add_argument("a", type=Path)
    comparer.add_argument("b", type=Path)
    args = parser.parse_args()
    if args.action == "compare":
        return compare(args)
    if args.action == "run" and args.check_stdout and args.projects and "hono" not in args.projects:
        parser.error("--check-stdout requires hono in --projects")
    local_work()
    # One compiler/build at a time, even if two invocations use this same clone.
    with (WORK / "runner.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise ValueError("another census build/run is active in this clone") from None
        return build(args) if args.action == "build" else run(args)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError) as error:
        print(f"census: {error}", file=sys.stderr)
        sys.exit(1)
