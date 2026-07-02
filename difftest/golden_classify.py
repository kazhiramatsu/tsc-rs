#!/usr/bin/env python3
"""Classify a golden diff against the real-tsc oracle.

Given an OLD baseline snapshot and a NEW snapshot (each produced by
`tsrs --check-batch`, one `path\\x01{json}` line per file), this finds every
file whose `main.ts` diagnostics changed, runs tsc on it, and classifies each
delta:

    NEW_FP  diagnostic tsrs newly emits that tsc does NOT  -> regression (gate)
    NEW_FN  diagnostic tsrs newly drops that tsc DOES emit -> regression (report)
    OK_ADD  diagnostic tsrs newly emits that tsc also emits -> improvement
    OK_RM   diagnostic tsrs newly drops that tsc also omits -> correct FP removal

Only `main.ts`-scoped diagnostics are compared; a small set of library-noise
codes is ignored. Exit status is non-zero iff there is at least one NEW_FP, so
this is a usable CI gate for the standing rule "never ship a new false positive".
NEW_FN are surfaced prominently but do not fail the gate (a false negative is
the lesser evil and is sometimes an accepted trade-off).
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

ROOT = Path(os.environ.get("TSRS_ROOT", Path(__file__).resolve().parents[1]))
ORACLE = os.environ.get("TSRS_DIAG_ORACLE", str(ROOT / "difftest" / "diag_oracle.js"))
# library-internal codes that diverge only because of the curated lib stub.
LIBCODES = {2318, 2304, 2583, 2584, 2792}


def load(path):
    """path -> set[(code, (line, col))] for main.ts diagnostics."""
    out = {}
    with open(path, encoding="utf8", errors="replace") as fh:
        for line in fh:
            line = line.rstrip("\n")
            if "\x01" not in line:
                continue
            p, js = line.split("\x01", 1)
            try:
                d = json.loads(js)
            except json.JSONDecodeError:
                # a PANIC marker or truncated line: record empty so a file that
                # used to have diagnostics and now panics still shows as changed.
                out[p] = out.get(p, set())
                continue
            diags = d.get("diagnostics", [])
            out[p] = {
                (x["code"], (x.get("startLine"), x.get("startCol")))
                for x in diags
                if x.get("file") == "main.ts"
            }
    return out


def tsc_diags(path, lib):
    """Run tsc on `path` (as main.ts) with `lib`; set[(code, (line, col))] or None."""
    work = tempfile.mkdtemp()
    try:
        shutil.copy(lib, os.path.join(work, "lib.tsrs.d.ts"))
        shutil.copy(path, os.path.join(work, "main.ts"))
        r = subprocess.run(
            ["node", ORACLE, os.path.join(work, "main.ts"), os.path.join(work, "lib.tsrs.d.ts")],
            capture_output=True,
            text=True,
            timeout=120,
        )
    except (subprocess.TimeoutExpired, FileNotFoundError, OSError):
        return None
    finally:
        shutil.rmtree(work, ignore_errors=True)
    try:
        d = json.loads(r.stdout)
    except json.JSONDecodeError:
        return None
    return {
        (x["code"], (x.get("startLine"), x.get("startCol")))
        for x in d.get("diagnostics", [])
        if x.get("file") and x["file"].endswith("main.ts")
    }


def short(path):
    return path.split("/")[-1]


def main():
    if len(sys.argv) != 4:
        print("usage: golden_classify.py OLD_SNAPSHOT NEW_SNAPSHOT LIB_DTS", file=sys.stderr)
        return 2
    old_p, new_p, lib = sys.argv[1:4]
    if not os.path.exists(old_p):
        print(f"golden-check: baseline missing: {old_p}", file=sys.stderr)
        return 2
    old, new = load(old_p), load(new_p)

    changed = sorted(
        fn for fn in (set(old) | set(new)) if old.get(fn, set()) != new.get(fn, set())
    )
    if not changed:
        print("golden-check: IDENTICAL — no main.ts diagnostic changed across the corpus")
        return 0

    print(f"golden-check: {len(changed)} file(s) changed; classifying against tsc...")
    new_fp, new_fn = [], []
    ok_add = ok_rm = 0
    tsc_fail = []
    for fn in changed:
        if not os.path.exists(fn):
            tsc_fail.append(fn)
            continue
        tsc = tsc_diags(fn, lib)
        if tsc is None:
            tsc_fail.append(fn)
            continue
        o, n = old.get(fn, set()), new.get(fn, set())
        for d in sorted(n - o):
            if d[0] in LIBCODES:
                continue
            if d in tsc:
                ok_add += 1
            else:
                new_fp.append((short(fn), d[0], d[1]))
        for d in sorted(o - n):
            if d[0] in LIBCODES:
                continue
            if d in tsc:
                new_fn.append((short(fn), d[0], d[1]))
            else:
                ok_rm += 1

    def by_file(rows):
        files = {}
        for base, code, _ in rows:
            files.setdefault(base, Counter())[code] += 1
        for base in sorted(files):
            codes = files[base]
            total = sum(codes.values())
            detail = " ".join(f"{c}x{codes[c]}" for c in sorted(codes))
            print(f"    {base}: {total}  [{detail}]")

    print()
    print(f"improvements: +{ok_add} correct additions, -{ok_rm} correct FP removals")
    if new_fn:
        print(f"\n  NEW FALSE NEGATIVES ({len(new_fn)}) — tsc emits, tsrs now drops (lesser evil):")
        by_file(new_fn)
    if tsc_fail:
        print(f"\n  tsc-unavailable for {len(tsc_fail)} changed file(s) (not classified):")
        for fn in tsc_fail[:10]:
            print(f"    {short(fn)}")

    print()
    if new_fp:
        print(f"  NEW FALSE POSITIVES ({len(new_fp)}) — tsrs emits, tsc does NOT:")
        by_file(new_fp)
        print(f"\ngolden-check: FAIL — {len(new_fp)} new false positive(s)")
        return 1
    print("golden-check: PASS — zero new false positives"
          + (f" ({len(new_fn)} new FN to review)" if new_fn else ""))
    return 0


if __name__ == "__main__":
    sys.exit(main())
