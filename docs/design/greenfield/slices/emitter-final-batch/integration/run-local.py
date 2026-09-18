#!/usr/bin/env python3
"""Record one focused integration command, preserving its exit and source state."""
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
OUT = Path(__file__).resolve().parent / "records/local"


def main():
    label, *command = sys.argv[1:]
    if not command or not label.replace("-", "").replace("_", "").isalnum():
        raise SystemExit("usage: run-local.py LABEL COMMAND [ARG ...]")
    OUT.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env["CARGO_BUILD_JOBS"] = "2"
    env.setdefault("CARGO_TARGET_DIR", "/Users/hiramatsu/dev/tsc-rs-emitter-final/target")
    prefix = ["taskpolicy", "-b", "nice", "-n", "15"] if sys.platform == "darwin" else []
    receipt = {
        "argv": prefix + command,
        "cwd": str(ROOT),
        "started_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "tracked_clean": not subprocess.check_output(["git", "diff", "HEAD", "--name-only"], cwd=ROOT).strip(),
        "diff_sha256": hashlib.sha256(subprocess.check_output(["git", "diff", "HEAD"], cwd=ROOT)).hexdigest(),
        "env": {key: value for key, value in env.items()
                if key.startswith(("CARGO_", "TSC_RS_", "TSRS_"))},
    }
    started = time.monotonic()
    log = OUT / f"{label}.log"
    with log.open("wb") as output:
        process = subprocess.Popen(prefix + command, cwd=ROOT, env=env,
                                   stdout=output, stderr=subprocess.STDOUT)
        print(f"{label}: pid={process.pid}; log={log}", flush=True)
        code = process.wait()
    content = log.read_bytes()
    (OUT / f"{label}.log.gz").write_bytes(gzip.compress(content, mtime=0))
    receipt.update(exit=code, seconds=round(time.monotonic() - started, 3),
                   log_sha256=hashlib.sha256(content).hexdigest(), log_bytes=len(content))
    (OUT / f"{label}.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(content.decode(errors="replace")[-5000:], flush=True)
    print(json.dumps({"label": label, "exit": code, "seconds": receipt["seconds"]}), flush=True)
    raise SystemExit(code)


if __name__ == "__main__":
    main()
