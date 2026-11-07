#!/usr/bin/env python3
"""Run coarse phase ledgers; native measurements belong to a quiet window.

Example: python3 -I scripts/phase_ledger.py /abs/path/tsc-rs hono/full \
    --native-total 1234567890
Supply the instruction total from /usr/bin/time -l on the feature-off build
of the same source/configuration. This tool never runs time or scales rows.
Use --tsrs for --singleThreaded --extendedDiagnostics (phase wall times only).
"""

import argparse
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import sys
import tempfile

# Same case mapping as target/claude-tools/perf11/serial.py (2026-11-07).
ROOTS = {
    "hono": "hono", "zod": "zod", "effect": "effect.noindex/packages/effect",
    "ts": "TypeScript-6.0/src/compiler", "next": "next.js/packages/next",
    "vscode": "vscode/src", "playwright": "playwright", "vue": "vue",
}
PHASES = ("config", "load", "bind", "checker-init", "semantic", "finalization")
COUNTERS = ("ri_instructions", "ri_user_time", "ri_system_time", "wall_ns")
PEAK = "ri_lifetime_max_phys_footprint"


def sample(row):
    if not isinstance(row, dict):
        raise ValueError("counter sample must be an object")
    values = {}
    for key in (*COUNTERS, PEAK):
        value = row.get(key)
        if type(value) is not int or value < 0:
            raise ValueError(f"missing or invalid {key}")
        values[key] = value
    return values


def follows(current, previous):
    return all(current[key] >= previous[key] for key in (*COUNTERS, PEAK))


def reconcile(phases, total, native_total=None):
    """Add raw disjoint intervals; never normalize to the native control."""
    sums = {key: sum(row[key] for row in phases.values()) for key in COUNTERS}
    remainder = {key: total[key] - sums[key] for key in COUNTERS}
    if any(value < 0 for value in remainder.values()):
        raise ValueError("phase sum exceeds process total")
    result = {"sum": sums, "remainder": remainder}
    if native_total is not None:
        if native_total <= 0:
            raise ValueError("native total must be positive")
        result["native_delta"] = total["ri_instructions"] - native_total
        result["native_delta_percent"] = 100 * result["native_delta"] / native_total
    return result


def parse_ledger(text, wall_only=False):
    """Parse version 1 JSONL, reject probe failures and overlapping intervals."""
    phases = {}
    metadata = None
    calibration = None
    previous = dict.fromkeys((*COUNTERS, PEAK), 0)
    total = None
    for line in text.splitlines():
        if not line.strip():
            continue
        row = json.loads(line)
        if not isinstance(row, dict):
            raise ValueError("ledger rows must be objects")
        if "error" in row:
            raise ValueError(f"probe disabled: {row['error']} (errno {row.get('errno')})")
        if total is not None:
            raise ValueError("rows after final total")
        if row.get("kind") == "metadata":
            if metadata is not None or row.get("version") != 1:
                raise ValueError("duplicate metadata or unsupported ledger version")
            if any(type(row.get(key)) is not int or row[key] < 1 for key in ("workers", "checkers")):
                raise ValueError("invalid worker/checker budgets")
            metadata = row
            continue
        if row.get("kind") == "calibration":
            if calibration is not None:
                raise ValueError("duplicate calibration")
            for key in ("boundaries", "instructions", "wall_ns"):
                if type(row.get(key)) is not int or row[key] < 0:
                    raise ValueError(f"invalid calibration {key}")
            if row["boundaries"] == 0:
                raise ValueError("empty calibration")
            calibration = row
            continue
        phase = row.get("phase")
        end = sample(row)
        if phase == "total":
            if not follows(end, previous):
                raise ValueError("non-monotonic total")
            total = end
            continue
        if phase not in PHASES:
            raise ValueError(f"unknown phase: {phase}")
        start = sample(row.get("start", {}))
        if not follows(start, previous) or not follows(end, start):
            raise ValueError("non-monotonic or overlapping phase intervals")
        previous = end
        accumulated = phases.setdefault(phase, dict.fromkeys((*COUNTERS, PEAK), 0))
        for key in COUNTERS:
            accumulated[key] += end[key] - start[key]
        accumulated[PEAK] = max(accumulated[PEAK], end[PEAK])
    if metadata is None or total is None:
        raise ValueError("incomplete ledger (metadata or total missing)")
    serialized = (metadata.get("serialized") is True
                  and metadata.get("workers") == 1 and metadata.get("checkers") == 1)
    if not serialized and not wall_only:
        raise ValueError("process instruction attribution requires one worker and one checker")
    reconcile(phases, total)
    return {"phases": phases, "total": total, "calibration": calibration}


def parse_tsrs(text):
    """tsrs provides parse/bind/check wall time, no phase instructions/peak."""
    rows = {}
    for label, value in re.findall(
            r"^\s*(Parse|Bind|Check|Total) time:\s*([0-9]+(?:\.[0-9]+)?)s\s*$",
            text, re.MULTILINE | re.IGNORECASE):
        label = label.lower()
        if label in rows:
            raise ValueError(f"duplicate tsrs {label} time")
        rows[label] = float(value) * 1e9
    if any(label not in rows for label in ("parse", "bind", "check", "total")):
        raise ValueError("tsrs extendedDiagnostics is missing parse/bind/check/total times")
    phases = {phase: {"wall_ns": rows[label]} for phase, label in
              (("load", "parse"), ("bind", "bind"), ("semantic", "check"))}
    return {"phases": phases, "total": {"wall_ns": rows["total"]}}


def median_runs(runs):
    # Missing phases are unavailable, not zero work. Require matching coverage.
    names = set(runs[0]["phases"])
    if any(set(run["phases"]) != names for run in runs):
        raise ValueError("phase coverage changed between rounds")
    return {
        "phases": {name: {key: statistics.median(run["phases"][name][key] for run in runs)
                           for key in runs[0]["phases"][name]} for name in names},
        "total": {key: statistics.median(run["total"][key] for run in runs)
                  for key in runs[0]["total"]},
    }


def print_table(runs, tsrs=False, wall_only=False, native_total=None):
    med = median_runs(runs)
    print("tsrs: wall only; phase instructions and footprint are unavailable."
          if tsrs else "tsc-rs: medians of raw disjoint phase intervals; peak is a lifetime gauge.")
    if tsrs:
        print("load shows parse time only; other loading and checker initialization are unavailable.")
    if wall_only and not tsrs:
        print("Wall-only mode: instruction attribution and native reconciliation suppressed.")
    if not tsrs and set(med["phases"]) != set(PHASES):
        print("Unavailable phases (skipped by this run): " +
              ", ".join(phase for phase in PHASES if phase not in med["phases"]))
    print(f"{'phase':16} {'instructions G':>16} {'wall s':>12} {'peak MiB':>12}")
    for phase in (*PHASES, "total"):
        row = med["total"] if phase == "total" else med["phases"].get(phase, {})
        instructions = (f"{row['ri_instructions'] / 1e9:.6f}"
                        if not tsrs and not wall_only and "ri_instructions" in row else "—")
        wall = f"{row['wall_ns'] / 1e9:.6f}" if "wall_ns" in row else "—"
        peak = f"{row[PEAK] / 2**20:.2f}" if PEAK in row else "—"
        print(f"{phase:16} {instructions:>16} {wall:>12} {peak:>12}")
    if tsrs:
        remainder = med["total"]["wall_ns"] - sum(row["wall_ns"] for row in med["phases"].values())
        print(f"remainder (total − available phase medians): {remainder / 1e9:.6f}s (times are rounded by tsrs)")
        return
    # Reconcile each run before taking medians: medians are not additive.
    checks = [reconcile(run["phases"], run["total"],
                        None if wall_only else native_total) for run in runs]
    summed = statistics.median(check["sum"]["ri_instructions"] for check in checks)
    remainder = statistics.median(check["remainder"]["ri_instructions"] for check in checks)
    wall_remainder = statistics.median(check["remainder"]["wall_ns"] for check in checks)
    if not wall_only:
        print(f"sum(phases), median per run: {summed / 1e9:.6f} G")
        print(f"remainder = total − sum(phases), median per run: {remainder / 1e9:.6f} G; {wall_remainder / 1e9:.6f}s")
        print("Remainder includes pre-main startup, calibration, observed probe gaps and sidecar I/O; no correction applied.")
        if native_total is not None:
            delta = statistics.median(check["native_delta_percent"] for check in checks)
            print(f"feature-off native total supplied: {native_total / 1e9:.6f} G; ledger total delta {delta:+.4f}%")
            print(f"0.3% reconciliation gate: {'PASS' if abs(delta) <= 0.3 else 'FAIL'}")
        else:
            print("Native reconciliation pending: supply --native-total from the feature-off control.")
    else:
        print(f"remainder wall (median of total − sum(phases) per run): {wall_remainder / 1e9:.6f}s")
    for run in runs:
        cal = run.get("calibration")
        if cal:
            print(f"calibration envelope/boundary: {cal['instructions'] / cal['boundaries']:.1f} instructions, "
                  f"{cal['wall_ns'] / cal['boundaries']:.1f} ns (upper estimate; not subtracted)")


def positive(value):
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError("must be positive")
    return number


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("case", help="project/variant from serial.py's ROOTS")
    parser.add_argument("--bench", type=Path, default=Path.home() / "dev/bench-corpora")
    parser.add_argument("--warmup", type=int, default=2)
    parser.add_argument("--rounds", type=positive, default=9)
    parser.add_argument("--native-total", type=positive, help="raw feature-off instructions retired")
    parser.add_argument("--tsrs", action="store_true")
    parser.add_argument("--wall-only", action="store_true")
    parser.add_argument("--output-dir", type=Path, help="retain raw sidecars and compiler streams")
    args = parser.parse_args(argv)
    if args.warmup < 0:
        parser.error("--warmup must be nonnegative")
    if args.tsrs and args.native_total is not None:
        parser.error("--tsrs has no phase instructions; omit --native-total")
    parts = args.case.split("/")
    if len(parts) != 2 or parts[0] not in ROOTS or not re.fullmatch(r"[A-Za-z0-9_-]+", parts[1]):
        parser.error("case must be project/variant; project in " + ", ".join(ROOTS))
    env = dict(os.environ)
    for key in ("TSRS_WORKERS", "TSRS_CHECKERS"):
        value = env.get(key, "1")
        if not re.fullmatch(r"[1-9][0-9]*", value):
            parser.error(f"{key} must be a positive integer")
        if int(value) > 1 and not args.wall_only:
            parser.error(f"{key}>1 requires --wall-only; process counters cannot attribute parallel phases")
        env[key] = value if args.wall_only else "1"
    cwd = args.bench.expanduser().resolve() / ROOTS[parts[0]]
    config = f"tsconfig.bench-{parts[1]}.json"
    if not (cwd / config).is_file():
        parser.error(f"case config missing: {cwd / config}")
    command = [str(args.binary.expanduser().resolve()), "-p", config, "--pretty", "false"]
    if args.tsrs:
        command += ["--singleThreaded", "--extendedDiagnostics"]
    runs = []
    with tempfile.TemporaryDirectory(prefix="phase-ledger-") as temporary:
        destination = args.output_dir.expanduser().resolve() if args.output_dir else Path(temporary)
        destination.mkdir(parents=True, exist_ok=True)
        for index in range(args.warmup + args.rounds):
            name = (f"warmup-{index + 1}" if index < args.warmup
                    else f"round-{index - args.warmup + 1}")
            path = destination / f"{name}.jsonl"
            path.unlink(missing_ok=True)  # never accept a stale run's ledger
            env.pop("TSRS_PHASE_LEDGER", None)
            if not args.tsrs:
                env["TSRS_PHASE_LEDGER"] = str(path)
            completed = subprocess.run(command, cwd=cwd, env=env, capture_output=True, text=True)
            if args.output_dir:
                (destination / f"{name}.stdout").write_text(completed.stdout)
                (destination / f"{name}.stderr").write_text(completed.stderr)
            # TypeScript diagnostics normally exit 1 or 2. Crashes and driver
            # failures must not become apparent measurement successes.
            if completed.returncode not in (0, 1, 2):
                raise ValueError(f"{name}: compiler exited {completed.returncode}: {completed.stderr[:500]}")
            result = (parse_tsrs(completed.stdout) if args.tsrs else
                      parse_ledger(path.read_text(), wall_only=args.wall_only))
            if index >= args.warmup:
                runs.append(result)
            print(f"{name}: rc={completed.returncode}", file=sys.stderr, flush=True)
    print(f"{args.case}: {args.rounds} rounds after {args.warmup} warm-up runs")
    print_table(runs, args.tsrs, args.wall_only, args.native_total)


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError) as error:
        sys.exit(f"phase_ledger: {error}")
