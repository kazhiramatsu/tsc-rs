#!/usr/bin/env python3
"""gate-tax 8 S5 (report-only): per-event output capture.

Copies a MODELED producer's declared outputs into the run directory
before (pre) and after (post) its write invocation — the event's own
truth baseline (a later round overwrites minted bytes; without the
post copy an earlier event's ground truth is unrecoverable, review
round 8). Unmodeled producers declare no outputs and are captured
nowhere: they abstain by construction.

Usage: capture.py pre|post <round> <producer> <run_dir>
Emits one jsonl row per copied file on stdout; exit 0 always on
missing manifest/producer (report-only path — never reds a walk).
"""
import json
import os
import shutil
import sys
import time

MANIFEST = "scripts/shadow/selector-manifest.v1.json"


def main():
    stage, round_number, producer, run_directory = sys.argv[1:5]
    try:
        manifest = json.load(open(MANIFEST, encoding="utf-8"))
    except (OSError, ValueError):
        return 0
    declared = manifest.get("producers", {}).get(producer, {})
    outputs = declared.get("outputs", [])
    for output in outputs:
        path = output.get("output_path")
        if not path or not os.path.exists(path):
            continue
        destination = os.path.join(
            run_directory, stage, round_number, path.replace("/", "__")
        )
        os.makedirs(os.path.dirname(destination), exist_ok=True)
        shutil.copyfile(path, destination)
        print(
            json.dumps(
                {
                    "capture": stage,
                    "round": int(round_number),
                    "rung": producer,
                    "output": path,
                    "at": int(time.time()),
                }
            )
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
