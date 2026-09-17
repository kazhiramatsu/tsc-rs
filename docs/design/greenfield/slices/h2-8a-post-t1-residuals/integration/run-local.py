#!/usr/bin/env python3
"""Record the bounded integration replay; output must be a new directory."""
import argparse
import datetime
import gzip
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[6]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
output = args.output.resolve()
output.mkdir(parents=True, exist_ok=False)
env = os.environ.copy()
env["CARGO_BUILD_JOBS"] = "2"
commands = [
    ("emitter-units", ["cargo", "test", "--manifest-path", "crates/emitter/Cargo.toml", "--lib"]),
    ("post-t1", [sys.executable, "scripts/witness.py", "post-t1-residuals", "--all"]),
    ("t1", [sys.executable, "scripts/witness.py", "bundle-metadata-t1", "--all"]),
]
def git(*argv):
    return subprocess.check_output(["git", *argv], cwd=ROOT, text=True).strip()

receipt = {"head": git("rev-parse", "HEAD"), "source_status": git("status", "--short"),
           "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
           "environment": {key: env.get(key) for key in ("CARGO_BUILD_JOBS", "CARGO_TARGET_DIR")},
           "commands": []}
for name, command in commands:
    argv = ["taskpolicy", "-b", "nice", "-n", "15", *command] if sys.platform == "darwin" else command
    start = time.monotonic()
    log = output / (name + ".log")
    with log.open("wb") as stream:
        result = subprocess.run(argv, cwd=ROOT, env=env, stdout=stream, stderr=subprocess.STDOUT)
    compressed = output / (name + ".log.gz")
    compressed.write_bytes(gzip.compress(log.read_bytes(), mtime=0))
    receipt["commands"].append({"name": name, "argv": argv, "exit": result.returncode,
                                "seconds": round(time.monotonic()-start, 3),
                                "log": compressed.name,
                                "sha256": hashlib.sha256(compressed.read_bytes()).hexdigest()})
    (output / "receipt.v1.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt["commands"][-1]), flush=True)
    if result.returncode:
        raise SystemExit(result.returncode)
print("All bounded integration checks passed.", flush=True)
