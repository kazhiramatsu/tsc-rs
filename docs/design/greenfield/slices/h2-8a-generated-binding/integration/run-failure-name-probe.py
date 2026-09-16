#!/usr/bin/env python3
"""Compare seven failure/reuse controls with pinned TypeScript; mismatch exits 1."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent


def run(command, cwd, env, log):
    started = time.monotonic()
    result = subprocess.run(command, cwd=cwd, env=env, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    log.write_text(result.stderr)
    result.check_returncode()
    return result.stdout, {"command": command, "exit": result.returncode,
                           "seconds": round(time.monotonic() - started, 3)}


def rows(text):
    values = [json.loads(line) for line in text.splitlines() if line.strip()]
    assert len(values) == 7, "seven controls must execute"
    keys = [row.get("second_source", row.get("same_binding")) for row in values]
    assert len(set(keys)) == 7, "duplicate observation"
    for row in values:
        assert row["first"]["status"] == "threw", "the injected hook must fail"
        assert row["second"]["status"] == row["fresh"]["status"] == "returned"
    return values


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True,
                        help="read-only source checkout to probe")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root, output = args.root.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    package = output / "package"
    (package / "src").mkdir(parents=True, exist_ok=True)
    shutil.copyfile(HERE / "failure-name-probe.rs", package / "src/main.rs")
    shutil.copyfile(HERE / "probe.Cargo.lock", package / "Cargo.lock")
    manifest = ('[package]\nname="c02-integration-review"\nversion="0.0.0"\n'
                'edition="2021"\n[workspace]\n[dependencies]\nserde_json="1.0"\n')
    for name in ("emitter", "program", "syntax", "types"):
        path = json.dumps(str(root / "crates" / name))
        manifest += f'tsc-{name}={{package="tsc-rs-{name}",path={path}}}\n'
    manifest += "[profile.dev]\ndebug=0\n"
    for name in ("diagnostics", "emitter", "syntax", "types"):
        manifest += f"[profile.dev.package.tsc-rs-{name}]\nopt-level=3\n"
    (package / "Cargo.toml").write_text(manifest)
    env = os.environ.copy()
    env.update(CARGO_BUILD_JOBS="2", CARGO_INCREMENTAL="0", CARGO_PROFILE_DEV_DEBUG="0",
               CARGO_TARGET_DIR=str(root / "target"))
    prefix = ["taskpolicy", "-b", "nice", "-n", "15"] if sys.platform == "darwin" else []
    observed = {}
    commands = []
    # Sequential commands; two builds are never scheduled concurrently.
    for index in (1, 2):
        for name, command in (
            ("typescript", ["node", str(HERE / "observe-failure-name-probe.mjs"), str(root)]),
            ("native", ["cargo", "run", "--offline", "--locked", "--manifest-path",
                        str(package / "Cargo.toml")]),
        ):
            text, receipt = run(prefix + command, root, env, output / f"{name}-{index}.log")
            (output / f"{name}-{index}.jsonl").write_text(text)
            value = rows(text)
            if name in observed:
                assert value == observed[name], f"{name}: repeat drift"
            observed[name] = value
            commands.append(receipt)
    comparison = []
    for expected, actual in zip(observed["typescript"], observed["native"]):
        key = expected.get("second_source", expected.get("same_binding"))
        assert key == actual.get("second_source", actual.get("same_binding"))
        # The injected error is typed on Rust and a JS exception on TypeScript.
        # Compare both returned texts and statuses, including retained writer bytes.
        exact = all(expected[field] == actual[field] for field in ("second", "fresh"))
        comparison.append({"key": key, "exact": exact,
                           "expected": expected["second"], "actual": actual["second"]})
    mismatches = sum(not row["exact"] for row in comparison)
    receipt = {"root": str(root), "rows": 7, "repetitions": 2,
               "exact": 7 - mismatches, "mismatches": mismatches,
               "typescript_sha256": hashlib.sha256(
                   (root / "vendor/typescript-6.0.3/lib/typescript.js").read_bytes()).hexdigest(),
               "commands": commands, "comparison": comparison}
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({key: receipt[key] for key in ("rows", "repetitions", "exact", "mismatches")}))
    return int(mismatches != 0)


if __name__ == "__main__":
    sys.exit(main())
