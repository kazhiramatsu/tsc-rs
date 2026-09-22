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
import hashlib
import json
import sys
import contextlib
import io
import tempfile
from pathlib import Path

MOD_BRACED = re.compile(r"^(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{")
MOD_DECL = re.compile(r"^(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*;")
CFG_TEST_EXACT = "#[cfg(test)]"
CFG_TEST_LOOSE = re.compile(r"^#\[cfg\(.*\btest\b.*\)\]$")
FROZEN_SOURCE = "crates/xtask/src/recovery_parse_snapshot.rs"
CONTRACT = json.loads(Path(__file__).with_name("frozen-test-layout.json").read_bytes())


def frozen_module_allowed(relative, raw, violations, coupling):
    """One existing census producer is byte-frozen; no path-only exemption."""
    if relative != FROZEN_SOURCE:
        return False
    if (CONTRACT["schema"], CONTRACT["source"], CONTRACT["module"], CONTRACT["inline_modules"]) != (1, FROZEN_SOURCE, "tests", 1):
        raise ValueError("frozen test layout descriptor scope changed")
    if not violations:
        raise ValueError("frozen layout exception no longer needed; retire it")
    if hashlib.sha256(raw).hexdigest() != CONTRACT["sha256"] or len(raw) != CONTRACT["bytes"]:
        raise ValueError("frozen census producer changed; migrate its identity and retire the layout exception together")
    if violations != [(CONTRACT["cfg_line"], "inline-body")]:
        raise ValueError("frozen layout exception covers exactly one existing inline module")
    if raw.decode().splitlines()[CONTRACT["cfg_line"]].strip() != "mod tests {":
        raise ValueError("frozen layout module identity changed")
    if CONTRACT["coupling_expression"] not in coupling:
        raise ValueError("frozen census coupling removed; retire its layout exception")
    return True


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
    root = Path(__file__).resolve().parent.parent
    raw = (root / FROZEN_SOURCE).read_bytes()
    with tempfile.TemporaryDirectory(prefix="tsc-rs-frozen-layout-") as directory:
        scratch = Path(directory)
        frozen = scratch / FROZEN_SOURCE
        frozen.parent.mkdir(parents=True)
        coupling = scratch / CONTRACT["coupling_source"]
        other = frozen.with_name("other.rs")

        def check(expected_code, message):
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                code = scan_root(scratch)
            if code != expected_code or message not in output.getvalue():
                raise AssertionError((expected_code, message, code, output.getvalue()))

        frozen.write_bytes(raw)
        coupling.write_text(CONTRACT["coupling_expression"])
        check(0, "one frozen reference module")
        for changed in [
            raw + b"\n",
            raw.replace(b"mod tests {", b"mod renamed_tests {"),
            raw + b"\n#[cfg(test)]\nmod extra {}\n",
        ]:
            frozen.write_bytes(changed)
            check(1, "frozen census producer changed")
        frozen.write_bytes(b'#[cfg(test)]\n#[path = "../tests/unit/recovery_parse_snapshot/tests.rs"]\nmod tests;\n')
        check(1, "frozen layout exception no longer needed")
        frozen.write_bytes(raw)
        other.write_bytes(raw)
        check(1, "other.rs:380: inline-body")
        other.unlink()
        coupling.write_text("")
        check(1, "frozen census coupling removed")
        coupling.write_text(CONTRACT["coupling_expression"])
        frozen.unlink()
        check(1, "missing source")
        frozen.write_bytes(raw)
        check(0, "one frozen reference module")
    print(f"self-test: {len(cases)} layout fixtures + frozen boundary controls ok")
    return 0


def main(argv):
    if "--self-test" in argv:
        return self_test()
    return scan_root(Path(__file__).resolve().parent.parent)


def scan_root(root):
    if not (root / FROZEN_SOURCE).is_file():
        print("frozen layout exception names a missing source; retire it with the census migration")
        return 1
    coupling = (root / CONTRACT["coupling_source"]).read_text()
    frozen_seen = False
    hits = []
    for src_dir in sorted(root.glob("crates/*/src")):
        for path in sorted(src_dir.rglob("*.rs")):
            raw = path.read_bytes()
            violations = scan_text(raw.decode("utf-8"))
            try:
                allowed = frozen_module_allowed(path.relative_to(root).as_posix(), raw, violations, coupling)
            except ValueError as error:
                print(f"{path.relative_to(root)}: {error}")
                return 1
            if allowed:
                frozen_seen = True
                # Only the single checked layout finding is exempted.
                violations = [(line, kind) for line, kind in violations
                              if (line, kind) != (CONTRACT["cfg_line"], "inline-body")]
            for line, kind in violations:
                hits.append(f"{path.relative_to(root)}:{line}: {kind}")
    if hits:
        print("test-module layout violations (move bodies below tests/unit/,")
        print("declare with '#[cfg(test)] #[path = ...] mod tests;'):")
        for hit in hits:
            print(f"  {hit}")
        return 1
    if not frozen_seen:
        print("frozen layout source was not scanned; retire the exception with its census migration")
        return 1
    print(f"inline-tests scan: clean (one frozen reference module: {FROZEN_SOURCE} @ {CONTRACT['sha256'][:12]})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
