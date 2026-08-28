#!/usr/bin/env python3
"""Pre-walk scan for test-module layout violations in crates/*/src.

Mirrors and EXTENDS xtask's workspace_maintenance::audit_unit_test_layout
(the gate's first phase) so violations surface BEFORE a ladder walk, not
after it (the gt6 lesson: a post-walk layout fix re-stales the whole
ladder). Unlike the audit, this scan reports ALL hits (the audit
fail-fasts on the first) and also covers two known audit gaps:

  A. audit mirror — an exact `#[cfg(test)]` line followed (skipping other
     attributes; stopped by `#[path`) by `mod <name> {` (pub variants
     included): the inline test-module body the audit rejects.
  B. compound cfg — any `#[cfg(...)]` whose argument mentions `test`
     (e.g. `#[cfg(all(test, target_os = "macos"))]`) followed by
     `mod <name> {`: invisible to the audit's exact-line match. The
     corpus has no `cfg(not(test))` today; if one ever appears
     legitimately, refine the matcher rather than deleting the rule.
  C. src-resident declaration — `#[cfg(test)]` (exact or compound) +
     `mod <name>;` with NO `#[path` attribute between them: the body
     resolves to a file INSIDE src/, which the layout convention also
     forbids (audit gap: it only rejects braced bodies).

Exit 0 = clean, 1 = violations listed on stdout, 2 = usage/self-test
failure. `--self-test` runs the embedded fixtures.
"""

import re
import sys
from pathlib import Path

MOD_BRACED = re.compile(r"^(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{")
MOD_DECL = re.compile(r"^(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*;")
CFG_TEST_EXACT = "#[cfg(test)]"
CFG_TEST_LOOSE = re.compile(r"^#\[cfg\(.*\btest\b.*\)\]$")


def scan_text(text):
    """Return [(line_number, kind)] violations in one file's text."""
    violations = []
    lines = [line.strip() for line in text.split("\n")]
    for index, line in enumerate(lines):
        exact = line == CFG_TEST_EXACT
        loose = bool(CFG_TEST_LOOSE.match(line))
        if not (exact or loose):
            continue
        saw_path = False
        for candidate in lines[index + 1 :]:
            if not candidate:
                continue
            if candidate.startswith("#[path"):
                saw_path = True
                continue
            if candidate.startswith("#["):
                continue
            if MOD_BRACED.match(candidate):
                violations.append(
                    (index + 1, "inline-body" if exact else "inline-body-compound-cfg")
                )
            elif MOD_DECL.match(candidate) and not saw_path:
                violations.append((index + 1, "src-resident-declaration"))
            break
    return violations


def self_test():
    cases = [
        # (fixture, expected kinds)
        ("#[cfg(test)]\nmod tests {\n}", ["inline-body"]),
        ("#[cfg(test)]\npub mod tests {\n}", ["inline-body"]),
        ("#[cfg(test)]\npub(crate) mod checks {\n}", ["inline-body"]),
        ("#[cfg(all(test, unix))]\nmod tests {\n}", ["inline-body-compound-cfg"]),
        ("#[cfg(test)]\nmod tests;\n", ["src-resident-declaration"]),
        ("#[cfg(test)]\n#[path = \"../tests/unit/x/tests.rs\"]\nmod tests;\n", []),
        ("#[cfg(test)]\n#[allow(dead_code)]\nmod tests {\n}", ["inline-body"]),
        ("#[cfg(test)]\nfn helper() {}\n", []),
        ("mod plain {\n}", []),
        ("// #[cfg(test)]\nmod tests {\n}", []),
        ("#[cfg(test)]\n\n\nmod tests {\n}", ["inline-body"]),
    ]
    for fixture, expected in cases:
        got = [kind for (_, kind) in scan_text(fixture)]
        if got != expected:
            print(f"self-test FAILED: {fixture!r}: expected {expected}, got {got}")
            return 2
    print(f"self-test: {len(cases)} fixtures ok")
    return 0


def main(argv):
    if "--self-test" in argv:
        return self_test()
    root = Path(__file__).resolve().parent.parent
    hits = []
    for src_dir in sorted(root.glob("crates/*/src")):
        for path in sorted(src_dir.rglob("*.rs")):
            for line, kind in scan_text(path.read_text(encoding="utf-8")):
                hits.append(f"{path.relative_to(root)}:{line}: {kind}")
    if hits:
        print("test-module layout violations (move bodies below tests/unit/,")
        print("declare with '#[cfg(test)] #[path = ...] mod tests;'):")
        for hit in hits:
            print(f"  {hit}")
        return 1
    print("inline-tests scan: clean")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
