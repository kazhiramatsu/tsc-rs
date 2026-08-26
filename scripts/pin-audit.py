#!/usr/bin/env python3
"""Audit the Rust-side artifact pin literals BEFORE a gate spends ~40
minutes reaching workspace-tests (precedent: the m-1 "walk repair" commit
382c2474 hand-re-pinned these after a walk; forgetting that repair red-ends
a full gate at the workspace-tests phase).

Rules per audited file:
  - RECORDED include: a `"/../../ratchets/X"` include path pairs with the
    `sha256(RECORDED)`-adjacent hex; it must equal the current disk hash.
  - (path, hex) adjacent pair literals: pass when the hex equals the current
    disk sha256 of the path, OR the sha256 the file's own artifact embeds
    for that path under origin/historical (era-frozen predecessor records).
  - --fix rewrites stale pins (embedded value when the owning artifact
    records one for that path, else the disk hash).

A discovery guard refuses (exit 2) when a new Rust file starts holding
ratchet-path + 64-hex literals without being classified below.

Usage: python3 scripts/pin-audit.py [--fix]
Exit: 0 clean, 1 stale pins (reported, or rewritten with --fix — rerun for
0, then run the harness integration tests as the targeted check), 2
unclassified pin-holding file.
"""
import hashlib, json, re, subprocess, sys

FIX = "--fix" in sys.argv[1:]

AUDITED = [
    "crates/harness/tests/integration/h2_transition.rs",
    "crates/harness/tests/integration/h2_1a_profile.rs",
    "crates/harness/tests/integration/h2_1b_profile.rs",
    "crates/harness/tests/integration/h2_1c_profile.rs",
    "crates/harness/tests/integration/h2_1d_profile.rs",
    "crates/harness/tests/integration/h2_1e_profile.rs",
    "crates/harness/tests/integration/h2_2a_profile.rs",
    "crates/harness/tests/integration/h2_2b_profile.rs",
    "crates/harness/tests/integration/h2_2c_profile.rs",
    "crates/harness/tests/integration/h2_2d_profile.rs",
    "crates/harness/tests/integration/h2_3a_profile.rs",
    "crates/harness/tests/integration/h2_3b_profile.rs",
    "crates/harness/tests/integration/h2_3c_profile.rs",
    "crates/harness/tests/integration/h2_baseline.rs",
    "crates/harness/tests/integration/h1_compiler_profile_classification.rs",
    "crates/harness/tests/integration/h1_conformance_profile_classification.rs",
    "crates/harness/tests/integration/h1_project_profile_classification.rs",
    "crates/harness/tests/integration/h1_fourslash_whole_program_equivalence.rs",
    "crates/harness/tests/integration/transpile_suite_inventory.rs",
]
# Reference ratchet paths + hex but hold no current-tracking artifact pins:
# host_resolution's hex is prose; h2_1a_acceptance's hexes are corpus case
# fingerprints (content-stable, not artifact hashes).
EXEMPT = [
    "crates/conformance/src/host_resolution.rs",
    "crates/xtask/src/h2_1a_acceptance.rs",
]

PAIR = re.compile(r'"((?:ratchets|vendor|goldens|crates/oracle|\.github)/[^"\n]+)",\s*\n?\s*"([0-9a-f]{64})"')
INCLUDE = re.compile(r'"/\.\./\.\./(ratchets/[^"\n]+)"')
RECORDED = re.compile(r'sha256\(RECORDED\),\s*\n?\s*"([0-9a-f]{64})"')

def sha256_file(path):
    try:
        return hashlib.sha256(open(path, "rb").read()).hexdigest()
    except OSError:
        return None

def embedded_hashes(artifact_path):
    """{path: sha256} for every {path, sha256} record inside the artifact."""
    try:
        artifact = json.load(open(artifact_path))
    except (OSError, ValueError):
        return {}
    found = {}
    def walk(node):
        if isinstance(node, dict):
            p, h = node.get("path"), node.get("sha256")
            if isinstance(p, str) and isinstance(h, str) and re.fullmatch(r"[0-9a-f]{64}", h):
                found.setdefault(p, h)
            for value in node.values():
                walk(value)
        elif isinstance(node, list):
            for value in node:
                walk(value)
    walk(artifact)
    return found

def discovery():
    out = subprocess.run(["grep", "-rl", '"ratchets/', "crates/"], capture_output=True, text=True)
    unclassified = []
    for f in out.stdout.split():
        if not f.endswith(".rs") or f in AUDITED or f in EXEMPT:
            continue
        if re.search(r'"[0-9a-f]{64}"', open(f, encoding="utf-8", errors="replace").read()):
            unclassified.append(f)
    return unclassified

def audit_file(path):
    body = open(path).read()
    include = INCLUDE.search(body)
    own_artifact = include.group(1) if include else None
    embedded = embedded_hashes(own_artifact) if own_artifact else {}
    stale, replacements = [], []
    if own_artifact:
        m = RECORDED.search(body)
        disk = sha256_file(own_artifact)
        if m and disk and m.group(1) != disk:
            stale.append((own_artifact, m.group(1), disk))
            replacements.append((m.group(1), disk))
    for m in PAIR.finditer(body):
        target, pinned = m.group(1), m.group(2)
        disk = sha256_file(target)
        candidates = {value for value in (disk, embedded.get(target)) if value}
        if pinned not in candidates:
            new = embedded.get(target) or disk
            stale.append((target, pinned, new))
            if new:
                replacements.append((pinned, new))
    if FIX and replacements:
        for old, new in replacements:
            body = body.replace(f'"{old}"', f'"{new}"')
        open(path, "w").write(body)
    return stale

def main():
    unclassified = discovery()
    if unclassified:
        for f in unclassified:
            print(f"UNCLASSIFIED PIN FILE: {f} — read it and add to AUDITED or EXEMPT in scripts/pin-audit.py")
        return 2
    any_stale = False
    for f in AUDITED:
        for target, old, new in audit_file(f):
            any_stale = True
            action = "fixed ->" if FIX and new else "current ="
            print(f"STALE PIN {f}: {target} pinned {old[:12]}.. {action} {(new or 'MISSING')[:12]}..")
    if any_stale:
        print("pin audit: fixed; rerun to verify, then run the harness integration tests" if FIX
              else "pin audit: STALE (scripts/pin-audit.py --fix, then the harness integration tests)")
        return 1
    print(f"pin audit: clean ({len(AUDITED)} audited files)")
    return 0

sys.exit(main())
