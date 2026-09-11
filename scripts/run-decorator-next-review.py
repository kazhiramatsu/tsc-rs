#!/usr/bin/env python3
"""Run one isolated decorator review check with immutable prelaunch inputs.

Usage: run-decorator-next-review.py <new-run-name> <command> [args ...]
The command's actual exit code is preserved. Runs never reuse a directory.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile

ROOT = Path(__file__).resolve().parent.parent


def sha(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def main():
    name, *command = sys.argv[1:]
    if not re.fullmatch(r"[a-zA-Z0-9_-]+", name) or not command:
        raise SystemExit("expected a new run name and a command")
    run = ROOT / "target/dec-next-runs" / name
    run.mkdir()  # Fail if the run already exists, including a failed run.
    captures = run / "captures"
    captures.mkdir()
    inputs = {
        path for path in git("ls-files").splitlines()
        if path.startswith("crates/")
        or path in {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml"}
        or path.startswith(".cargo/")
        or path.startswith("scripts/")
    }
    inputs.update(str(path.relative_to(ROOT)) for path in (ROOT / "vendor").rglob("*")
                  if path.is_file() and "node_modules" not in path.parts)
    inputs.add(str(Path(__file__).relative_to(ROOT)))
    inputs = sorted(path for path in inputs if (ROOT / path).is_file())
    hashes = {path: sha(ROOT / path) for path in inputs}
    archive = run / "source-and-inputs.tar.gz"
    with tarfile.open(archive, "w:gz") as tar:
        for path in inputs:
            tar.add(ROOT / path, arcname=path, recursive=False)
    env_overrides = {
        "CARGO_TARGET_DIR": str(ROOT / "target/dec-next-review-artifacts"),
        "CARGO_BUILD_JOBS": "2",
        "TSC_RS_H2_8A_CAPTURE_WRITES_DIR": str(captures),
    }
    prelaunch = {
        "head": git("rev-parse", "HEAD"),
        "branch": git("branch", "--show-current"),
        "status": git("status", "--porcelain=v1"),
        "command": command,
        "environment": env_overrides,
        "inputs": hashes,
        "production_inputs": {
            path: hashes[path]
            for path in git("diff", "--name-only", "cb4e5f3e8", "--", "crates").splitlines()
            if "/src/" in path and path in hashes
        },
        "archive": {"path": str(archive), "sha256": sha(archive)},
    }
    (run / "prelaunch.json").write_text(json.dumps(prelaunch, indent=2) + "\n")
    (run / "candidate.patch").write_bytes(subprocess.check_output(
        ["git", "diff", "--binary", "HEAD", "--", "crates", "scripts"], cwd=ROOT))
    log = run / "run.log"
    with log.open("w") as stream:
        stream.write("command: " + json.dumps(command) + "\n")
        stream.flush()
        result = subprocess.run(["taskpolicy", "-b", "nice", "-n", "15", *command],
                                cwd=ROOT, env={**os.environ, **env_overrides},
                                stdout=stream, stderr=subprocess.STDOUT)
        stream.write(f"\nexit: {result.returncode}\n")
    changed = [path for path, expected in hashes.items()
               if not (ROOT / path).is_file() or sha(ROOT / path) != expected]
    binaries = re.findall(r"Running .*?\((.*?)\)", log.read_text(errors="replace"))
    archived_binaries = {}
    if binaries:
        (run / "binaries").mkdir()
    for path in binaries:
        destination = run / "binaries" / Path(path).name
        subprocess.run(["cp", "-c", str(ROOT / path), str(destination)], check=True)
        archived_binaries[str(destination.relative_to(ROOT))] = {
            "executed_path": path, "sha256": sha(destination),
        }
    receipt = {
        "prelaunch_sha256": sha(run / "prelaunch.json"),
        "actual_exit": result.returncode,
        "log_sha256": sha(log),
        "executed_binaries": {path: sha(ROOT / path) for path in binaries},
        "archived_binaries": archived_binaries,
        "inputs_changed_during_run": changed,
    }
    (run / "execution.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"run_dir": str(run), **receipt}), flush=True)
    return result.returncode if not changed else 1


if __name__ == "__main__":
    raise SystemExit(main())
