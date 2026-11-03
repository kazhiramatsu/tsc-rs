#!/usr/bin/env python3
"""Collect offline tsgo/tsc-rs checker entry counts and compare serial benchmarks."""

import argparse
import csv
import fcntl
import hashlib
import importlib.util
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


# Dynamic loading of the PGO helper must not create files outside the census
# artifact/source directories (notably scripts/__pycache__).
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / "scripts/tsgo_census"
WORK = ROOT / "target/tsgo-census"
RS_WORK = ROOT / "target/tsc-census"
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


def local_work(work=WORK):
    # Never follow an existing target symlink into another checkout.
    if not work.resolve().is_relative_to(ROOT):
        raise ValueError(f"work directory escapes this clone: {work.resolve()}")
    work.mkdir(parents=True, exist_ok=True)
    for path in work.iterdir():
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


def recorded(command, cwd, env, stdout, stderr, records, *, stdin=None):
    print(f"census: cd {shlex.quote(str(cwd))} && {shlex.join(map(str, command))}",
          file=sys.stderr, flush=True)
    started = time.monotonic()
    with stdout.open("wb") as out, stderr.open("wb") as err:
        if stdin is None:
            proc = subprocess.run(command, cwd=cwd, env=env, stdout=out, stderr=err)
        else:
            with stdin.open("rb") as source:
                proc = subprocess.run(command, cwd=cwd, env=env, stdout=out, stderr=err, stdin=source)
    result = {
        "command": list(map(str, command)), "cwd": str(cwd),
        "exit_code": proc.returncode,
        "elapsed_seconds": round(time.monotonic() - started, 3),
        "stdout": str(stdout), "stderr": str(stderr),
    }
    records.append(result)
    if stdin is not None:
        result["stdin"] = str(stdin)
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
    if not isinstance(data, dict):
        raise ValueError(f"expected an object of nonnegative integer entry counts: {path}")
    unmapped = data.get("_unmapped", [])
    if (not isinstance(unmapped, list) or any(not isinstance(k, str) for k in unmapped)
            or len(unmapped) != len(set(unmapped))
            or any(k in data for k in unmapped)
            or not isinstance(data.get("_meta", {}), dict)
            or any(not isinstance(k, str) or type(v) is not int or v < 0
                   for k, v in data.items() if k not in ("_unmapped", "_meta"))):
        raise ValueError(f"expected an object of nonnegative integer entry counts: {path}")
    return data


def project_arguments(project, root):
    variant = "stable" if project == "vscode" else "noemit"
    config = f"tsconfig.bench-{variant}.json"
    if not (root / config).is_file():
        raise ValueError(f"benchmark config missing: {root / config}")
    return ["-p", config, "--pretty", "false", "--singleThreaded"]


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
            command_args = project_arguments(project, root)
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


def parse_profdata(lines):
    """IR instrumentation: the first block counter is the entry count.

    In this format `Function count` is not the entry counter. LLVM may prefix
    local symbols with `source-file;`; multiple records for a name are summed.
    """
    counts, current = {}, None
    for line in lines:
        if re.search(r"\bentry_first\s*=\s*0\b", line):
            raise ValueError("profile is not entry-first; rebuild with -Cllvm-args=-pgo-instrument-entry=true")
        header = re.fullmatch(r"  (\S.*):\s*", line.rstrip("\n"))
        if header:
            current = header[1].split(";")[-1]
            continue
        block = re.match(r"    Block counts: \[(\d+)", line)
        if block and current:
            counts[current] = counts.get(current, 0) + int(block[1])
            current = None
        elif line.startswith("    Block counts:"):
            current = None
    if not counts:
        raise ValueError("llvm-profdata show contained no first-block entry counts")
    return counts


def rust_function_name(symbol):
    # Strip closure children before finding the owning function. Drop generic
    # arguments with balanced brackets: their type paths can contain `::` too.
    symbol = re.sub(r"::h[0-9a-f]{16}$", "", symbol)
    symbol = re.split(r"::(?:\{\{closure\}\}|\{closure#\d+\})", symbol, maxsplit=1)[0]
    plain, depth = [], 0
    for char in symbol:
        if char == "<":
            depth += 1
        elif char == ">" and depth:
            depth -= 1
        elif depth == 0:
            plain.append(char)
    return "".join(plain).rstrip(":").rsplit("::", 1)[-1]


def rust_matchers(rows):
    return [(name, re.compile(rust[3:]) if rust.startswith("re:") else rust)
            for name, _, rust, _ in rows if rust]


def rust_matches(symbol, matchers):
    own_name = rust_function_name(symbol)
    matches = {name for name, pattern in matchers
               if (pattern == own_name if isinstance(pattern, str) else pattern.search(symbol))}
    if len(matches) > 1:
        raise ValueError(f"ambiguous function mapping: {symbol}: {sorted(matches)}")
    return next(iter(matches), None)


def map_rust_counts(data):
    rows = manifest_rows()
    matchers = rust_matchers(rows)
    matched = {name: [] for name, *_ in rows}
    result = {}
    for symbol, count in sorted(data.items()):
        name = rust_matches(symbol, matchers)
        if name is not None:
            result[name] = result.get(name, 0) + count
            matched[name].append((symbol, count))
    result["_unmapped"] = [name for name, *_ in rows if not matched[name]]
    return result, matched


def pgo_helpers():
    spec = importlib.util.spec_from_file_location("pgo_build", ROOT / "scripts/pgo_build.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def checked_record(command, stdout, stderr, records, **kwargs):
    result = recorded(command, ROOT, os.environ.copy(), stdout, stderr, records, **kwargs)
    if result["exit_code"]:
        raise ValueError(f"command failed ({result['exit_code']}): {shlex.join(map(str, command))}; see {stderr}")
    return result


def rs_build(args, prefix, records, pgo):
    metadata = {"commands": records, "manifest_sha256": hashlib.sha256(MANIFEST.read_bytes()).hexdigest()}
    binary = RS_WORK / "instrumented/release/tsc-rs"
    checked_record(["git", "rev-parse", "HEAD"], RS_WORK / "commit.stdout", RS_WORK / "commit.stderr", records)
    commit = (RS_WORK / "commit.stdout").read_text().strip()
    # Tooling-only commits may follow the compiler's instrumented build.
    # Require clean compiler inputs and, on reuse, identical committed inputs.
    compiler_inputs = ["crates", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo"]
    checked_record(["git", "diff", "--exit-code", "HEAD", "--", *compiler_inputs],
                   RS_WORK / "source-diff.stdout", RS_WORK / "source-diff.stderr", records)
    if args.skip_build:
        metadata = json.loads((RS_WORK / "build.json").read_text())
        if (not binary.is_file()
                or not metadata.get("entry_first")
                or metadata["binary_sha256"] != hashlib.sha256(binary.read_bytes()).hexdigest()):
            raise ValueError("instrumented binary/commit changed; run-rs without --skip-build first")
        if metadata["binary_commit"] != commit:
            checked_record(["git", "diff", "--exit-code", metadata["binary_commit"], commit,
                            "--", *compiler_inputs],
                           RS_WORK / "reuse-diff.stdout", RS_WORK / "reuse-diff.stderr", records)
        return binary, metadata
    started = time.monotonic()
    try:
        metadata["binary_commit"] = commit
        checked_record(["rustc", "-Vv"], RS_WORK / "rustc.stdout", RS_WORK / "rustc.stderr", records)

        def cargo_runner(command, **kwargs):
            metadata["rustflags"] = kwargs["env"]["RUSTFLAGS"]
            metadata["environment"] = {key: kwargs["env"][key] for key in
                                       ("CARGO_NET_OFFLINE", "CARGO_TARGET_DIR", "RUSTFLAGS", "CFLAGS", "CXXFLAGS")
                                       if key in kwargs["env"]}
            result = recorded(command, stdout=RS_WORK / "build.stdout", stderr=RS_WORK / "build.stderr",
                              records=records, **kwargs)
            if result["exit_code"]:
                raise ValueError(f"offline cargo build failed ({result['exit_code']}); see {RS_WORK / 'build.stderr'}")
            return subprocess.CompletedProcess(command, result["exit_code"])

        pgo.instrumented_build(str(RS_WORK), 1, entry_counts=True, prefix=prefix, runner=cargo_runner, offline=True)
        metadata["entry_first"] = True
        metadata["binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        return binary, metadata
    finally:
        metadata["elapsed_seconds"] = round(time.monotonic() - started, 3)
        metadata["binary_exists"] = binary.is_file()
        write_json(RS_WORK / "build.json", metadata)


def rs_profile_counts(project, raw, profdata, prefix, records):
    logs = RS_WORK / "runs"
    merged = logs / f"{project}.profdata"
    checked_record([*prefix, str(profdata), "merge", "-o", str(merged), str(raw)],
                   logs / f"{project}.merge.stdout", logs / f"{project}.merge.stderr", records)
    shown = logs / f"{project}.show.txt"
    checked_record([*prefix, str(profdata), "show", "--all-functions", "--counts", str(merged)],
                   shown, logs / f"{project}.show.stderr", records)
    with shown.open() as source:
        counts = parse_profdata(source)
    names = list(counts)
    mangled = logs / f"{project}.mangled.txt"
    mangled.write_text("\n".join(names) + "\n")
    demangled = logs / f"{project}.demangled.txt"
    checked_record([*prefix, "c++filt"], demangled, logs / f"{project}.demangle.stderr", records, stdin=mangled)
    symbols = demangled.read_text().splitlines()
    if len(symbols) != len(names):
        raise ValueError(f"c++filt returned {len(symbols)} names for {len(names)} symbols")
    result = {}
    for name, symbol in zip(names, symbols):
        if symbol.startswith(("_ZN", "_R")):
            raise ValueError(f"c++filt did not demangle Rust symbol: {symbol}")
        result[symbol] = result.get(symbol, 0) + counts[name]
    return map_rust_counts(result)


def write_mapping():
    projects = {project: json.loads(path.read_text()) for project in PROJECTS
                if (path := RS_WORK / "runs" / f"{project}.mapping.json").is_file()}
    with (RS_WORK / "mapping.txt").open("w") as out:
        for name, source, rust, note in manifest_rows():
            out.write(f"{name}\t{source}\t{rust or '(empty Rust mapping)'}\n  {note}\n")
            for project, mapping in projects.items():
                matches = mapping.get(name, [])
                out.write(f"  {project}: " + (f"{sum(count for _, count in matches)} entries" if matches else "UNMAPPED") + "\n")
                for symbol, count in matches:
                    out.write(f"    {count:>16}  {symbol}\n")


def run_rs(args):
    local_work(RS_WORK)
    records = []
    prefix = nice_prefix(records)
    pgo = pgo_helpers()
    summary = {"commands": records, "projects": {}}
    try:
        binary, build_info = rs_build(args, prefix, records, pgo)
        profdata = args.llvm_profdata or Path(pgo.llvm_profdata())
        checked_record([str(profdata), "--version"], RS_WORK / "profdata.stdout", RS_WORK / "profdata.stderr", records)
        summary["binary_commit"] = build_info["binary_commit"]
        counts_dir, logs = RS_WORK / "counts", RS_WORK / "runs"
        counts_dir.mkdir(exist_ok=True)
        logs.mkdir(exist_ok=True)
        for project in args.projects or PROJECTS:
            root = args.bench_root / PROJECTS[project]
            command_args = project_arguments(project, root)
            profiles = RS_WORK / "profiles" / project
            profiles.mkdir(exist_ok=True)
            if profiles.is_symlink():
                raise ValueError(f"unexpected profile directory symlink: {profiles}")
            for stale in profiles.glob("train-*.profraw"):
                stale.unlink()
            output = counts_dir / f"{project}.json"
            output.unlink(missing_ok=True)
            (logs / f"{project}.mapping.json").unlink(missing_ok=True)
            env = pgo.profile_environment(str(profiles), "train-%p.profraw")
            env.pop("TSGO_CENSUS", None)
            result = recorded(prefix + [str(binary), *command_args], root, env,
                              logs / f"{project}.stdout", logs / f"{project}.stderr", records)
            result["environment"] = {"LLVM_PROFILE_FILE": env["LLVM_PROFILE_FILE"]}
            summary["projects"][project] = dict(result, counts=str(output))
            raw = sorted(profiles.glob("train-*.profraw"))
            if len(raw) != 1 or raw[0].stat().st_size == 0:
                raise ValueError(f"expected one nonempty training profile for {project}, found {raw}")
            counts, matched = rs_profile_counts(project, raw[0], profdata, prefix, records)
            counts["_meta"] = dict(result, binary_commit=build_info["binary_commit"],
                                   binary_sha256=build_info["binary_sha256"], profile=str(raw[0]),
                                   manifest_sha256=hashlib.sha256(MANIFEST.read_bytes()).hexdigest(),
                                   llvm_profdata=str(profdata))
            write_json(output, counts)
            read_counts(output)
            write_json(logs / f"{project}.mapping.json", matched)
            write_mapping()
            print(f"{project}: {len(counts) - 2} mapped, {len(counts['_unmapped'])} unmapped, "
                  f"compiler exit={result['exit_code']}, {result['elapsed_seconds']}s, {output}", flush=True)
        # Diagnostics are data, not a runner failure. Missing profiles/tool
        # failures above still fail the invocation, including after a crash.
        return 0
    except (OSError, ValueError, KeyError) as error:
        summary["error"] = str(error)
        raise
    finally:
        # Keep the history when a later invocation measures the remaining projects.
        path = RS_WORK / "run.json"
        history = json.loads(path.read_text()) if path.is_file() else {"invocations": []}
        history.setdefault("invocations", []).append(summary)
        write_json(path, history)


def normalize(data):
    rows = manifest_rows()
    aliases = {}
    patterns = rust_matchers(rows)
    for name, source, rust, _ in rows:
        symbol = source.split(":", 1)[1].split(".")[-1]
        for alias in (name, source, symbol):
            aliases[alias] = name
        if rust and not rust.startswith("re:"):
            aliases[rust] = name
    result = {}
    for symbol, count in data.items():
        if symbol in ("_meta", "_unmapped"):
            continue
        if symbol in aliases:
            name = aliases[symbol]
        else:
            name = rust_matches(symbol, patterns) or symbol
        result[name] = result.get(name, 0) + count
    return result


def ratio_value(x, y):
    if x is None or y is None:
        return "missing", math.inf
    value = y / x if x else (1.0 if y == 0 else math.inf)
    return ("inf" if math.isinf(value) else f"{value:.6f}"), abs(value - 1)


def comparison_rows(a, b):
    unmapped = set(a.get("_unmapped", [])) | set(b.get("_unmapped", []))
    a, b = normalize(a), normalize(b)
    rows = []
    for name in (a.keys() | b.keys()) - unmapped:
        x, y = a.get(name), b.get(name)
        ratio, score = ratio_value(x, y)
        rows.append((score, name, x, y, ratio))
    return sorted(rows, key=lambda row: (-row[0], row[1])), sorted(unmapped)


def compare_summary(args):
    a_dir, b_dir = args.a or WORK / "counts", args.b or RS_WORK / "counts"
    projects = args.projects or [p for p in PROJECTS if (a_dir / f"{p}.json").exists()
                                or (b_dir / f"{p}.json").exists()]
    if not projects:
        raise ValueError("no project counts found")
    table, unmapped, scores = {}, {}, {}
    for project in projects:
        # A missing project is an error, rather than an apparently empty census.
        rows, missing = comparison_rows(read_counts(a_dir / f"{project}.json"),
                                        read_counts(b_dir / f"{project}.json"))
        for score, name, _, _, ratio in rows:
            table.setdefault(name, {})[project] = ratio
            scores[name] = max(score, scores.get(name, 0))
        for name in missing:
            unmapped.setdefault(name, []).append(project)
    width = max([len("function")] + list(map(len, table)))
    print(f"{'function':<{width}}" + "".join(f" {p:>12}" for p in projects))
    for name in sorted(table, key=lambda name: (-scores[name], name)):
        print(f"{name:<{width}}" + "".join(f" {table[name].get(p, 'unmapped'):>12}" for p in projects))
    if unmapped:
        print("\nUnmapped rows (excluded from ratios):")
        for name, affected in sorted(unmapped.items()):
            print(f"  {name}: {', '.join(affected)}")
    return 0


def compare(args):
    if getattr(args, "summary", False):
        return compare_summary(args)
    rows, unmapped = comparison_rows(read_counts(args.a), read_counts(args.b))
    width = max([len("function")] + [len(row[1]) for row in rows])
    print(f"{'function':<{width}} {'count A':>16} {'count B':>16} {'B / A':>12}")
    for _, name, x, y, ratio in rows:
        print(f"{name:<{width}} {str(x) if x is not None else '-':>16} "
              f"{str(y) if y is not None else '-':>16} {ratio:>12}")
    if unmapped:
        print("\nUnmapped rows (excluded from ratios):")
        for name in unmapped:
            print(f"  {name}")
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
    rust_runner = actions.add_parser("run-rs", help="build offline PGO instrumented tsc-rs, then collect serial entry counts")
    rust_runner.add_argument("--projects", nargs="+", choices=PROJECTS, help="default: all eight")
    rust_runner.add_argument("--bench-root", type=Path, default=BENCH)
    rust_runner.add_argument("--skip-build", action="store_true", help="reuse the verified instrumented binary")
    rust_runner.add_argument("--llvm-profdata", type=Path, help="default: pinned Rust toolchain's llvm-profdata")
    comparer = actions.add_parser("compare", help="compare JSON counts; ratio is B/A, descending |ratio-1|")
    comparer.add_argument("a", type=Path, nargs="?")
    comparer.add_argument("b", type=Path, nargs="?")
    comparer.add_argument("--summary", action="store_true", help="compare project directories (default: tsgo vs tsc-rs counts)")
    comparer.add_argument("--projects", nargs="+", choices=PROJECTS, help="select summary columns")
    args = parser.parse_args()
    if args.action == "compare":
        if bool(args.a) != bool(args.b) or (not args.summary and not args.a):
            parser.error("compare needs two JSON paths, or --summary [GO_DIR RS_DIR]")
        if args.projects and not args.summary:
            parser.error("compare --projects requires --summary")
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
        if args.action == "run-rs":
            return run_rs(args)
        return build(args) if args.action == "build" else run(args)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError) as error:
        print(f"census: {error}", file=sys.stderr)
        sys.exit(1)
