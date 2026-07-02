#!/usr/bin/env python3
"""Phase-2 diagnostic comparison: tsrs --diag-json vs diag_oracle.js (tsc).

Runs both engines on a single .ts/.tsx file (compiled alongside the curated
lib, exactly like cmp.sh) and reports structured differences:

  MISSING   tsc reports it, tsrs does not
  EXTRA     tsrs reports it, tsc does not (the Phase-1 "false positive" of the
            semantic layer)
  FIELDDIFF same (code, position) but a compared field differs

By default diagnostics are matched on (code, startLine, startCol) and compared
on the message chain (text + nested structure). Use --strict to additionally
compare the full span end, category, source, and the reportsUnnecessary /
reportsDeprecated hint flags (these are not tracked by tsrs yet, so --strict
will surface them as FIELDDIFFs until that compat work lands).

Usage:
  python3 difftest/diag_cmp.py <file.ts> [--strict] [--all-files]
  echo 'code' | python3 difftest/diag_cmp.py - [--strict]
"""
import json, subprocess, sys, tempfile, os, shutil
from pathlib import Path

ROOT = Path(os.environ.get("TSRS_ROOT", Path(__file__).resolve().parents[1]))
TSRS = os.environ.get("TSRS_BIN_RELEASE", str(ROOT / "target" / "release" / "tsrs"))
LIB = os.environ.get("TSRS_LIB", str(ROOT / "lib" / "lib.tsrs.d.ts"))
ORACLE = os.environ.get("TSRS_DIAG_ORACLE", str(ROOT / "difftest" / "diag_oracle.js"))

def chain_key(m):
    if m is None:
        return None
    k = [m.get("text")]
    for c in m.get("next", []) or []:
        k.append(chain_key(c))
    return tuple(k)

def run(path, strict, all_files):
    work = tempfile.mkdtemp()
    try:
        base = "main.tsx" if path.endswith(".tsx") else "main.ts"
        if path == "-":
            data = sys.stdin.read()
            with open(os.path.join(work, base), "w") as f:
                f.write(data)
        else:
            base = os.path.basename(path)
            shutil.copy(path, os.path.join(work, base))
        shutil.copy(LIB, os.path.join(work, "lib.tsrs.d.ts"))
        main = os.path.join(work, base)
        lib = os.path.join(work, "lib.tsrs.d.ts")

        # tsc oracle
        oc = subprocess.run(["node", ORACLE, main, lib] + (["--all-files"] if all_files else []),
                            capture_output=True, text=True)
        tsc = json.loads(oc.stdout or '{"diagnostics":[]}')

        # tsrs
        env = dict(os.environ, TSRS_VIRTUAL_CWD=work)
        tr = subprocess.run([TSRS, "--strict", "--diag-json", base],
                            capture_output=True, text=True, cwd=work, env=env)
        tsrs = json.loads(tr.stdout or '{"diagnostics":[]}')
    finally:
        shutil.rmtree(work, ignore_errors=True)

    # Scope to the main file unless --all-files (the oracle already scopes;
    # tsrs may additionally report on the prepended lib).
    def scope(ds):
        return [d for d in ds if all_files or (d.get("file") == base)]

    return scope(tsc["diagnostics"]), scope(tsrs["diagnostics"]), tsc, tsrs

def index(ds):
    out = {}
    for d in ds:
        out.setdefault((d["code"], d.get("startLine"), d.get("startCol")), []).append(d)
    return out

def main():
    args = sys.argv[1:]
    strict = "--strict" in args
    all_files = "--all-files" in args
    paths = [a for a in args if not a.startswith("--")]
    if not paths:
        print("usage: diag_cmp.py <file.ts|-> [--strict] [--all-files]"); sys.exit(2)
    tsc, tsrs, tscFull, tsrsFull = run(paths[0], strict, all_files)

    ti, ri = index(tsc), index(tsrs)
    missing, extra, fielddiff = [], [], []

    for key, tds in ti.items():
        rds = ri.get(key, [])
        for i, td in enumerate(tds):
            if i >= len(rds):
                missing.append(td); continue
            rd = rds[i]
            diffs = []
            if chain_key(td["message"]) != chain_key(rd["message"]):
                diffs.append("message")
            # relatedInformation is core diagnostic content; compare by default
            # as a list of (code, startLine, startCol, messageChain).
            def relkey(d):
                return [
                    (r["code"], r.get("startLine"), r.get("startCol"), chain_key(r["message"]))
                    for r in d.get("related", [])
                ]
            if relkey(td) != relkey(rd):
                diffs.append("related")
            if strict:
                for fld in ("endLine", "endCol", "category", "source",
                            "reportsUnnecessary", "reportsDeprecated", "length"):
                    if td.get(fld) != rd.get(fld):
                        diffs.append(fld)
            if diffs:
                fielddiff.append((td, rd, diffs))
    for key, rds in ri.items():
        tds = ti.get(key, [])
        for i in range(len(tds), len(rds)):
            extra.append(rds[i])

    def fmt(d):
        loc = f"{d.get('startLine')}:{d.get('startCol')}" if d.get("file") else "global"
        return f"TS{d['code']} {loc} {d['message'].get('text','')[:70]}"

    print(f"emittedFiles(tsc): {tscFull.get('emittedFiles')}  emitSkipped: {tscFull.get('emitSkipped')}")
    print(f"diagnostics: tsc={len(tsc)} tsrs={len(tsrs)}  "
          f"MISSING={len(missing)} EXTRA={len(extra)} FIELDDIFF={len(fielddiff)}")
    for d in missing:   print("  MISSING  ", fmt(d))
    for d in extra:     print("  EXTRA    ", fmt(d))
    for td, rd, fl in fielddiff:
        print("  FIELDDIFF", fmt(td), "->", ",".join(fl))
        if "message" in fl:
            print("      tsc :", chain_key(td["message"]))
            print("      tsrs:", chain_key(rd["message"]))
        if "related" in fl:
            print("      tsc.related :", [(r["code"], r.get("startLine"), r.get("startCol")) for r in td.get("related", [])])
            print("      tsrs.related:", [(r["code"], r.get("startLine"), r.get("startCol")) for r in rd.get("related", [])])

    ok = not (missing or extra or fielddiff)
    print("MATCH" if ok else "DIFF")
    sys.exit(0 if ok else 1)

if __name__ == "__main__":
    main()
