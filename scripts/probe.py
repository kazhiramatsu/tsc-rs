#!/usr/bin/env python3
"""Probe: run a fixture through both tsrs and the tsc oracle, print both diag sets.
Usage: probe.py <fixture.ts> [more.ts...]"""
import json
import os
import subprocess
import sys

sys.path.insert(0, "/Users/hiramatsu/dev/tsc-rs")
import scripts.parallel_classify as classify

ROOT = "/Users/hiramatsu/dev/tsc-rs"
LIB = os.path.join(ROOT, "lib", "lib.tsrs.d.ts")
TSRS = os.path.join(ROOT, "target", "release", "tsrs")


def tsrs_diags(path):
    listfile = path + ".list"
    with open(listfile, "w") as fh:
        fh.write(path + "\n")
    out = subprocess.run(
        [TSRS, "--check-batch", listfile], capture_output=True, text=True
    ).stdout
    os.unlink(listfile)
    for line in out.splitlines():
        if "\x01" in line:
            p, js = line.split("\x01", 1)
            try:
                data = json.loads(js)
            except json.JSONDecodeError:
                return [("PARSE-FAIL", js[:80], (0, 0))]
            res = []
            for d in data.get("diagnostics", []):
                f = os.path.basename(d.get("file") or "?")
                res.append((f, d["code"], d.get("category"), (d.get("startLine"), d.get("startCol")),
                            (d.get("message") or {}).get("text", "")[:70]))
            return res
    return []


def tsc_diags(path):
    import shutil
    import tempfile
    work = tempfile.mkdtemp()
    try:
        with open(path, encoding="utf8", errors="replace") as fh:
            fixture = classify.parse_fixture(fh.read())
        if path.endswith(".tsx") and len(fixture["files"]) == 1 and fixture["files"][0][0] == "main.ts":
            fixture["files"][0] = ("main.tsx", fixture["files"][0][1])
        opts = classify.compiler_options_from_directives(fixture["options"])
        if opts is None:
            return [("OPTS-FAIL", 0, 0, (0, 0), "")]
        roots = [classify.write_fixture_file(work, name, text) for name, text in fixture["files"]]
        roots.extend(classify.extra_root_path(work, name) for name in fixture["extra_root_files"])
        lib_path = os.path.join(work, "lib.tsrs.d.ts")
        shutil.copy(LIB, lib_path)
        roots.append(lib_path)
        opts_path = os.path.join(work, "compiler-options.json")
        with open(opts_path, "w", encoding="utf8") as fh:
            json.dump(opts, fh)
        r = subprocess.run(
            ["node", classify.ORACLE, "--options-json", opts_path, "--all-files", *roots],
            capture_output=True, text=True, timeout=45,
        )
        if r.returncode != 0:
            return [("TSC-FAIL", 0, 0, (0, 0), r.stderr[:100])]
        data = json.loads(r.stdout)
        res = []
        for d in data.get("diagnostics", []):
            f = os.path.basename(d.get("file") or "?")
            res.append((f, d["code"], d.get("category"), (d.get("startLine"), d.get("startCol")),
                        (d.get("message") or {}).get("text", "")[:70]))
        return res
    finally:
        shutil.rmtree(work, ignore_errors=True)


for path in sys.argv[1:]:
    path = os.path.abspath(path)
    print(f"\n########## {os.path.basename(path)}")
    ts_r = tsrs_diags(path)
    ts_c = tsc_diags(path)
    set_r = {(f, c, loc) for f, c, _, loc, _ in ts_r}
    set_c = {(f, c, loc) for f, c, _, loc, _ in ts_c}
    print("--- tsrs:")
    for f, c, cat, loc, msg in sorted(ts_r, key=lambda x: (x[0], x[3])):
        mark = " " if (f, c, loc) in set_c else "*"  # * = tsrs-only (FP)
        print(f"  {mark} {f}:{loc[0]}:{loc[1]} TS{c} [{cat}] {msg}")
    print("--- tsc:")
    for f, c, cat, loc, msg in sorted(ts_c, key=lambda x: (x[0], x[3])):
        mark = " " if (f, c, loc) in set_r else "*"  # * = tsc-only (FN)
        print(f"  {mark} {f}:{loc[0]}:{loc[1]} TS{c} [{cat}] {msg}")
