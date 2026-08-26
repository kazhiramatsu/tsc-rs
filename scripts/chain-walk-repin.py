#!/usr/bin/env python3
"""Refresh stale artifact-hash pins inside an oracle script.
The five pin grammars live in scripts/pin-grammar.py (gate-tax 5 single
source; the extraction is behavior-preserving vs the pre-gt5 inline
patterns). Only replaces when the path exists and its current sha256
differs. Writes are same-directory temp+rename (atomic)."""
import hashlib
import importlib.util
import os
import pathlib
import sys

_spec = importlib.util.spec_from_file_location(
    "pin_grammar", pathlib.Path(__file__).with_name("pin-grammar.py")
)
pin_grammar = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(pin_grammar)


def cur(p):
    try:
        return hashlib.sha256(open(p, "rb").read()).hexdigest()
    except OSError:
        return None


def write_atomic(path, contents):
    temporary = os.path.join(
        os.path.dirname(path) or ".", f".{os.path.basename(path)}.tmp"
    )
    with open(temporary, "w") as handle:
        handle.write(contents)
    os.replace(temporary, path)


script = sys.argv[1]
s = open(script).read()
changed = []


# patterns A/B/C: quoted path + adjacent quoted hash forms
def sub_pair(m, rebuild):
    path, old = m.group(1), m.group(m.lastindex)
    now = cur(path)
    if now and now != old:
        changed.append((path, old[:8], now[:8]))
        return rebuild(m, now)
    return m.group(0)


s = pin_grammar.PAT_A.sub(
    lambda m: sub_pair(m, lambda mm, now: f'"{mm.group(1)}", "{now}"'), s
)
s = pin_grammar.PAT_B.sub(
    lambda m: sub_pair(m, lambda mm, now: f'"{mm.group(1)}":\n{mm.group(2)}"{now}"'), s
)
s = pin_grammar.PAT_C.sub(
    lambda m: sub_pair(m, lambda mm, now: f'"{mm.group(1)}": "{now}"'), s
)
write_atomic(script, s)
for path, old, new in changed:
    print(f"re-pinned {path}: {old}.. -> {new}..")
if not changed:
    print("no stale pins found (failure may be a count const or deeper)")

# pattern D: const X_RELATIVE_PATH = "path"; ... const X_SHA256 = "hash";
s = open(script).read()
changedD = []
for name, path in pin_grammar.d_relative_path_pairs(s).items():
    now = cur(path)
    if not now:
        continue
    for const_name in (name + "_SHA256", "EXPECTED_" + name + "_SHA256"):
        patD = pin_grammar.d_hash_pattern(const_name)
        m = patD.search(s)
        if m and m.group(2) != now:
            s = patD.sub(lambda mm: mm.group(1) + now + mm.group(3), s)
            changedD.append((path, m.group(2)[:8], now[:8]))
if changedD:
    write_atomic(script, s)
    for path, old, new in changedD:
        print(f"re-pinned(const) {path}: {old}.. -> {new}..")

# pattern E: computed keys — [PATH_CONST]:\n  "hash" where const PATH_CONST = "path"
s = open(script).read()
changedE = []
for name, path in pin_grammar.e_path_constants(s).items():
    now = cur(path)
    if not now:
        continue
    patE = pin_grammar.e_hash_pattern(name)
    for m in list(patE.finditer(s)):
        if m.group(2) != now:
            s = patE.sub(lambda mm: mm.group(1) + now + mm.group(3), s)
            changedE.append((path, m.group(2)[:8], now[:8]))
if changedE:
    write_atomic(script, s)
    for path, old, new in changedE:
        print(f"re-pinned(key) {path}: {old}.. -> {new}..")
