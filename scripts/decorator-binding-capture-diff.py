#!/usr/bin/env python3
"""Summarize decorator-binding pipeline captures: for each captured case, diff the
expected and actual JavaScript/declaration texts and print the first differing
lines. usage: decorator-binding-capture-diff.py <captures-dir> [--all] [--limit N]"""
import base64, difflib, json, os, sys
directory = sys.argv[1]
show_all = "--all" in sys.argv
limit = int(sys.argv[sys.argv.index("--limit") + 1]) if "--limit" in sys.argv else 8
seen = {}
for name in sorted(os.listdir(directory)):
    if not name.endswith(".json"):
        continue
    capture = json.load(open(os.path.join(directory, name)))
    case_id = capture["case_id"]
    if case_id in seen:
        continue
    seen[case_id] = capture
rows = 0
for case_id, capture in sorted(seen.items()):
    expected = {w["path"]: w for w in capture["expected"]["writes"]}
    actual = {w["path"]: w for w in capture["actual"]["writes"]}
    problems = []
    if capture["expected"].get("exit_code") != capture["actual"].get("exit_code"):
        problems.append(f"exit {capture['expected'].get('exit_code')} vs {capture['actual'].get('exit_code')}")
    ed = [(d["code"], d.get("file"), d.get("start")) for d in capture["expected"]["reported_diagnostics"]]
    ad = [(d["code"], d.get("file"), d.get("start")) for d in capture["actual"]["reported_diagnostics"]]
    if ed != ad:
        problems.append(f"diagnostics expected {ed} actual {ad}")
    if sorted(expected) != sorted(actual):
        problems.append(f"paths expected {sorted(expected)} actual {sorted(actual)}")
    for path in sorted(expected):
        if path not in actual:
            continue
        e = base64.b64decode(expected[path]["callback_utf8_base64"]).decode("utf8", "replace")
        a = base64.b64decode(actual[path]["callback_utf8_base64"]).decode("utf8", "replace")
        if e == a:
            continue
        if path.endswith(".map"):
            problems.append(f"{path}: source map bytes differ")
            continue
        diff = [l for l in difflib.unified_diff(e.split("\n"), a.split("\n"), lineterm="", n=0)
                if l[:1] in "+-" and not l.startswith(("+++", "---"))]
        problems.append(f"{path}:\n      " + "\n      ".join(diff[:limit]))
    if problems or show_all:
        rows += 1
        print("==", case_id)
        for p in problems:
            print("   ", p)
print(f"{rows} cases with differences (of {len(seen)} captured)")
