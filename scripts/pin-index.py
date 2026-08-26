#!/usr/bin/env python3
"""The typed pin-index (gate-tax 5-A): the checked-in classification of
every artifact-hash pin literal in the oracle scripts.

`.github/ci/pin-index.v1.json` enumerates, per consumer script, the
classified pin sites as {path, grammar, count} plus a hand-curated
`semantic` exclusion list (hash-shaped constants that are program logic,
never masked, never auto-repinned). Classification only — no volatile
hash values or byte offsets; spans are re-derived from current bytes at
every use (scripts/pin-grammar.py) and verified against the
classification. The receipt-key normalizer masks ONLY enumerated pin
spans (positive selection); an unclassified site is a refusal there and
a red here.

  --write   regenerate the index from current bytes (git diff = the
            review surface; `semantic` entries are preserved)
  --check   regenerate in memory and refuse on ANY drift, plus refuse on
            path-adjacent 64-hex literals not covered by a classified
            span (the new-ci M1 oracle-audit rule, validated at 0
            findings over the whole corpus)

Exit: 0 clean, 1 drift/unclassified (reported in full, never
fail-first), 2 usage.
"""
import importlib.util
import json
import os
import pathlib
import re
import sys

_spec = importlib.util.spec_from_file_location(
    "pin_grammar", pathlib.Path(__file__).with_name("pin-grammar.py")
)
pin_grammar = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(pin_grammar)

INDEX_RELATIVE_PATH = ".github/ci/pin-index.v1.json"
SCOPE_GLOB = "crates/oracle"
PATH_ROOTS = ("ratchets/", "vendor/", "crates/oracle/", ".github/")
QUOTED = re.compile(r'"(?:[^"\\\n]|\\.)*"')
HEX_RUN = re.compile(r"[0-9a-fA-F]{64}")


def repo_root():
    return pathlib.Path(__file__).resolve().parent.parent


def consumers_on_disk(root):
    return sorted(
        str(path.relative_to(root))
        for path in (root / SCOPE_GLOB).glob("*.mjs")
    )


def path_adjacent(text, start):
    """Port of the new-ci adjacency heuristic: the literal's line, or the
    ±256-byte context, contains a quoted string holding a pin path root."""

    def any_pin_path(segment):
        return any(
            any(root in quoted.group(0) for root in PATH_ROOTS)
            for quoted in QUOTED.finditer(segment)
        )

    line_start = text.rfind("\n", 0, start) + 1
    line_end = text.find("\n", start)
    if line_end == -1:
        line_end = len(text)
    if any_pin_path(text[line_start:line_end]):
        return True
    context_start = max(0, start - 256)
    context_end = min(len(text), start + 64 + 256)
    return any_pin_path(text[context_start:context_end])


def unclassified_literals(text, pins):
    covered = {(pin["start"], pin["end"]) for pin in pins}
    findings = []
    for match in HEX_RUN.finditer(text):
        start, end = match.span()
        if start > 0 and text[start - 1] in "0123456789abcdefABCDEF":
            continue
        if end < len(text) and text[end] in "0123456789abcdefABCDEF":
            continue
        if (start, end) in covered:
            continue
        if path_adjacent(text, start):
            findings.append({"start": start, "literal": match.group(0)})
    return findings


def build_rows(text, semantic_rows):
    pins = pin_grammar.extract_pins(text)
    semantic = {(row["path"], row["grammar"]) for row in semantic_rows}
    groups = pin_grammar.group_pins(pins)
    rows = [
        {"path": path, "grammar": grammar, "count": count}
        for (path, grammar), count in sorted(groups.items())
        if (path, grammar) not in semantic
    ]
    stale_semantic = [
        row
        for row in semantic_rows
        if (row["path"], row["grammar"]) not in groups
    ]
    return pins, rows, stale_semantic


def curated_lists(stored_entry):
    """The hand-curated classification lists --write must preserve:
    `semantic` = grammar-SHAPED sites that are program logic (excluded
    from masking and from repin); `unmatched` = path-adjacent 64-hex
    literals OUTSIDE the five grammars, stored by literal value so a
    frozen anchor changing value goes red for review."""
    return (
        stored_entry.get("semantic", []),
        stored_entry.get("unmatched", []),
    )


def generate(root, stored):
    stored_consumers = stored.get("consumers", {})
    consumers = {}
    problems = []
    for consumer in consumers_on_disk(root):
        text = (root / consumer).read_text(encoding="utf-8")
        semantic_rows, unmatched_rows = curated_lists(
            stored_consumers.get(consumer, {})
        )
        try:
            pins, rows, stale_semantic = build_rows(text, semantic_rows)
        except ValueError as error:
            problems.append(f"{consumer}: {error}")
            continue
        for row in stale_semantic:
            problems.append(
                f"{consumer}: semantic entry ({row['path']}, {row['grammar']}) "
                "matches no current site — retire it"
            )
        allowed_literals = {row["literal"] for row in unmatched_rows}
        found_literals = set()
        for finding in unclassified_literals(text, pins):
            if finding["literal"] in allowed_literals:
                found_literals.add(finding["literal"])
                continue
            problems.append(
                f"{consumer}: path-adjacent 64-hex literal at byte "
                f"{finding['start']} ({finding['literal'][:12]}..) is covered by "
                "no grammar — classify it (pin? extend the grammar; frozen "
                "semantic anchor? add an `unmatched` entry) before it can "
                "hide a pin"
            )
        for row in unmatched_rows:
            if row["literal"] not in found_literals:
                problems.append(
                    f"{consumer}: unmatched entry {row['literal'][:12]}.. no "
                    "longer appears path-adjacent — the frozen anchor changed "
                    "or moved; review and update the entry"
                )
        if rows or semantic_rows or unmatched_rows:
            consumers[consumer] = {
                "family": "oracle-script",
                "pins": rows,
                "semantic": semantic_rows,
                "unmatched": unmatched_rows,
            }
    index = {
        "schema": 1,
        "kind": "pin-index",
        "scope": f"{SCOPE_GLOB}/*.mjs",
        "grammar_source": "scripts/pin-grammar.py",
        "consumers": consumers,
    }
    return index, problems


def render(index):
    return json.dumps(index, indent=2, sort_keys=True) + "\n"


def main(argv):
    if argv not in (["--write"], ["--check"]):
        print("usage: pin-index.py --write | --check", file=sys.stderr)
        return 2
    root = repo_root()
    index_path = root / INDEX_RELATIVE_PATH
    try:
        stored = json.loads(index_path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        stored = {}
    index, problems = generate(root, stored)
    for problem in problems:
        print(f"pin-index: {problem}")
    if argv == ["--write"]:
        if problems:
            print("pin-index: refusing to write while problems stand")
            return 1
        rendered = render(index)
        temporary = index_path.with_name(f".{index_path.name}.tmp")
        temporary.write_text(rendered, encoding="utf-8")
        os.replace(temporary, index_path)
        total = sum(
            row["count"]
            for consumer in index["consumers"].values()
            for row in consumer["pins"]
        )
        print(
            f"pin-index: wrote {INDEX_RELATIVE_PATH} "
            f"({len(index['consumers'])} consumers, {total} classified pin sites) "
            "— the git diff is the review surface"
        )
        return 0
    if not stored:
        print(f"pin-index: {INDEX_RELATIVE_PATH} is absent or unreadable — run --write")
        return 1
    drift = render(index) != render(stored)
    if drift:
        current = {
            (consumer, row["path"], row["grammar"]): row["count"]
            for consumer, entry in index["consumers"].items()
            for row in entry["pins"]
        }
        recorded = {
            (consumer, row["path"], row["grammar"]): row["count"]
            for consumer, entry in stored.get("consumers", {}).items()
            for row in entry.get("pins", [])
        }
        for key in sorted(set(current) | set(recorded)):
            if current.get(key) != recorded.get(key):
                consumer, path, grammar = key
                print(
                    f"pin-index: DRIFT {consumer}: ({path}, {grammar}) "
                    f"classified={recorded.get(key)} current={current.get(key)}"
                )
        print(
            "pin-index: STALE — review the sites above, then "
            "scripts/pin-index.py --write and review the diff"
        )
    if problems or drift:
        return 1
    total = sum(
        row["count"]
        for consumer in index["consumers"].values()
        for row in consumer["pins"]
    )
    print(
        f"pin-index: clean ({len(index['consumers'])} consumers, "
        f"{total} classified pin sites)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
