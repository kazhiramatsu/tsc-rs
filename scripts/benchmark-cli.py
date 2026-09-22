#!/usr/bin/env python3
"""Compare fresh compiler processes on fixed inputs (macOS/Linux, stdlib only)."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import shutil
import signal
import statistics
import subprocess
import sys
import time


def digest(data):
    return hashlib.sha256(data).hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def inventory(root, excluded=None):
    return {str(path.relative_to(root)): digest(path.read_bytes())
            for path in sorted(root.rglob("*"))
            if path.is_file() and (excluded is None or excluded not in path.parents)}


def measure(command, cwd, env, stdout, stderr, timeout):
    """One wait4 reaps the child; resource use excludes the Python parent."""
    def expired(_signum, _frame):
        raise TimeoutError("child deadline exceeded")

    child = None
    reaped = False
    timed_out = False
    previous = signal.signal(signal.SIGALRM, expired)
    try:
        with stdout.open("wb") as out, stderr.open("wb") as err:
            started = time.perf_counter_ns()
            child = subprocess.Popen(command, cwd=cwd, env=env, stdout=out, stderr=err,
                                     start_new_session=True)
            signal.setitimer(signal.ITIMER_REAL, timeout)
            try:
                _, status, usage = os.wait4(child.pid, 0)
            except TimeoutError:
                timed_out = True
                signal.setitimer(signal.ITIMER_REAL, 0)
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                _, status, usage = os.wait4(child.pid, 0)
            signal.setitimer(signal.ITIMER_REAL, 0)
            reaped = True
            elapsed = time.perf_counter_ns() - started
            child.returncode = os.waitstatus_to_exitcode(status)
            return {"wall_ns": elapsed, "exit_code": child.returncode, "timed_out": timed_out,
                    "user_seconds": usage.ru_utime, "system_seconds": usage.ru_stime,
                    "rss_bytes": usage.ru_maxrss * (1 if sys.platform == "darwin" else 1024)}
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)
        if child is not None and not reaped:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            _, status, _ = os.wait4(child.pid, 0)
            child.returncode = os.waitstatus_to_exitcode(status)


def orders(names, rounds, seed):
    # For three compilers this is all six permutations; for larger sets use
    # every rotation in both directions, balancing each compiler's position.
    patterns = list(dict.fromkeys(tuple(order[i:] + order[:i])
                                 for order in [names, list(reversed(names))]
                                 for i in range(len(names))))
    if rounds <= 0 or rounds % len(patterns):
        raise ValueError(f"rounds must be a positive multiple of {len(patterns)}")
    result = patterns * (rounds // len(patterns))
    random.Random(seed).shuffle(result)
    return result


def distribution(values):
    median = statistics.median(values)
    quartiles = statistics.quantiles(values, n=4, method="inclusive")
    return {"n": len(values), "median": median, "min": min(values), "max": max(values),
            "q1": quartiles[0], "q3": quartiles[2],
            "mad": statistics.median(abs(value - median) for value in values)}


def paired_interval(ratios, seed):
    rng = random.Random(seed)
    medians = sorted(statistics.median(rng.choices(ratios, k=len(ratios))) for _ in range(10000))
    return {"median": statistics.median(ratios), "bootstrap_95_percent": [medians[250], medians[9749]],
            "resamples": len(medians), "unit": "paired round within one session"}


def summarize(rows, plan):
    measured = [row for row in rows if row["phase"] == "sample"]
    tables = []
    for session in range(plan["sessions"]):
        for case in plan["cases"]:
            table = {"session": session, "case": case["id"], "variants": {}}
            selected = [row for row in measured if row["session"] == session and row["case"] == case["id"]]
            for name in plan["comparison"]:
                samples = [row for row in selected if row["variant"] == name]
                table["variants"][name] = {
                    "wall_ms": distribution([row["wall_ns"] / 1e6 for row in samples]),
                    "rss_mib": distribution([row["rss_bytes"] / 1024**2 for row in samples]),
                    "cpu_ms": distribution([(row["user_seconds"] + row["system_seconds"]) * 1000 for row in samples])}
            if "pair" in plan:
                first, second = plan["pair"]
                by_round = {(row["round"], row["variant"]): row["wall_ns"] for row in selected}
                ratios = [by_round[(index, first)] / by_round[(index, second)] for index in range(plan["rounds"])]
                table["paired_speedup"] = paired_interval(ratios, plan["seed"] + session)
            tables.append(table)
    return tables


def execute(plan, output):
    if sys.platform not in ("darwin", "linux"):
        raise RuntimeError("resource units are defined only for macOS and Linux")
    if os.getpriority(os.PRIO_PROCESS, 0) != 0:
        raise RuntimeError("run timed measurements at nice=0, separately from builds/tests")
    names = plan["comparison"]
    if not names or len(set(names)) != len(names) or plan["sessions"] < 1 or plan["warmups"] < 0:
        raise ValueError("invalid comparison, session or warmup counts")
    orders(names, plan["rounds"], plan["seed"])
    output.mkdir(parents=True, exist_ok=False)
    (output / "logs").mkdir()
    save(output / "plan.json", plan)
    variants = plan["variants"]
    executable_hashes = {}
    for variant in variants.values():
        binary = Path(shutil.which(variant["command"][0]) or variant["command"][0]).resolve(strict=True)
        variant["command"][0] = str(binary)
        executable_hashes[str(binary)] = digest(binary.read_bytes())
    outputs = {}
    inputs = {}
    for case in plan["cases"]:
        root = Path(case["cwd"]).resolve(strict=True)
        case["cwd"] = str(root)
        name = case.get("output_dir")
        if name is not None and (Path(name).name != name or name in ("", ".", "..")):
            raise ValueError("output_dir must name one disposable direct child directory of cwd")
        excluded = root / name if name else None
        if excluded is not None and excluded.is_symlink():
            raise ValueError("output directory must not be a symlink")
        outputs[case["id"]] = excluded
        inputs[str(root)] = inventory(root, excluded)
    for path in plan.get("extra_inputs", []):
        root = Path(path).resolve(strict=True)
        inputs[str(root)] = inventory(root)
    metadata = {"started_unix": time.time(), "platform": platform.platform(),
                "python": sys.version, "cpu_count": os.cpu_count(),
                "harness_sha256": digest(Path(__file__).read_bytes()), "binaries": executable_hashes,
                "inputs": inputs, "environment": {key: os.environ.get(key) for key in
                    ["PATH", "GOMAXPROCS", "GOGC", "GOMEMLIMIT", "RUST_MIN_STACK", "LANG", "LC_ALL"]}}
    save(output / "metadata.json", metadata)
    rows = []
    expected = {}

    def check_inputs():
        for root, wanted in inputs.items():
            excluded = next((outputs[case["id"]] for case in plan["cases"] if case["cwd"] == root), None)
            if inventory(Path(root), excluded) != wanted:
                raise RuntimeError(f"input changed: {root}")
        for binary, wanted in executable_hashes.items():
            if digest(Path(binary).read_bytes()) != wanted:
                raise RuntimeError(f"binary changed: {binary}")

    with (output / "samples.jsonl").open("x") as log:
        def run(case, name, phase, session, round_index, position):
            destination = outputs[case["id"]]
            if destination is not None and destination.exists():
                if destination.is_symlink():
                    raise RuntimeError("output directory became a symlink")
                shutil.rmtree(destination)
            variant = variants[name]
            env = os.environ.copy()
            for key, value in variant.get("env", {}).items():
                if value is None:
                    env.pop(key, None)
                else:
                    env[key] = value
            label = f"{len(rows):05d}"
            stdout, stderr = (output / "logs" / (label + suffix) for suffix in (".stdout", ".stderr"))
            command = variant["command"] + case["args"]
            row = measure(command, case["cwd"], env, stdout, stderr, plan.get("timeout_seconds", 120))
            row.update(case=case["id"], variant=name, phase=phase, session=session,
                       round=round_index, position=position, command=command,
                       stdout=str(stdout.relative_to(output)), stderr=str(stderr.relative_to(output)),
                       stdout_sha256=digest(stdout.read_bytes()), stderr_sha256=digest(stderr.read_bytes()),
                       outputs=inventory(destination) if destination is not None and destination.exists() else {})
            rows.append(row)
            log.write(json.dumps(row, ensure_ascii=False) + "\n")
            log.flush()
            if row["timed_out"] or row["exit_code"] != case.get("expected_exit", 0):
                raise RuntimeError(f"unexpected exit/timeout; see sample {label}")
            identity = {key: row[key] for key in ("exit_code", "stdout_sha256", "stderr_sha256", "outputs")}
            if phase == "oracle":
                expected[case["id"]] = identity
            elif identity != expected[case["id"]]:
                raise RuntimeError(f"oracle mismatch; see sample {label} (retained, not omitted)")

        for case in plan["cases"]:
            run(case, plan["oracle"], "oracle", -1, 0, 0)
            for position, name in enumerate(names):
                run(case, name, "preflight", -1, 0, position)
        check_inputs()
        save(output / "preflight.json", expected)
        for session in range(plan["sessions"]):
            # This measures harness + process startup, without subtracting it
            # from compiler timings. The compiler's own startup is real work.
            floor = []
            for index in range(32):
                row = measure(["/usr/bin/true"], output, os.environ.copy(),
                              output / "logs/floor.stdout", output / "logs/floor.stderr", 10)
                if row["exit_code"] != 0 or row["timed_out"]:
                    raise RuntimeError("harness floor probe failed")
                row["warmup"] = index < 2
                floor.append(row)
            save(output / f"floor-{session}.json", floor)
            cases = plan["cases"].copy()
            random.Random(plan["seed"] + session).shuffle(cases)
            for case_index, case in enumerate(cases):
                for index in range(plan["warmups"]):
                    for position, name in enumerate(names):
                        run(case, name, "warmup", session, index, position)
                for index, order in enumerate(orders(names, plan["rounds"], plan["seed"] + session * 100 + case_index)):
                    for position, name in enumerate(order):
                        run(case, name, "sample", session, index, position)
                check_inputs()
                print(f"session {session + 1}/{plan['sessions']}: {case['id']} complete", flush=True)
        check_inputs()
    save(output / "summary.json", summarize(rows, plan))
    save(output / "complete.json", {"finished_unix": time.time(), "samples": len(rows)})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plan", type=Path)
    parser.add_argument("output", type=Path, help="new output directory; existing evidence is never overwritten")
    args = parser.parse_args()
    execute(json.loads(args.plan.read_text()), args.output.resolve())


if __name__ == "__main__":
    main()
