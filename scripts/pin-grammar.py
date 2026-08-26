#!/usr/bin/env python3
"""The five artifact-pin grammars (gate-tax 5, single source).

These are EXACTLY the patterns scripts/chain-walk-repin.py rewrites, with
their per-pattern path classes preserved verbatim:
  A  "path", "hash"            (ratchets|vendor|crates/oracle|.github)
  B  "path":\n    "hash"       (ratchets|vendor|crates/oracle|.github)
  C  "path": "hash"            (ratchets|vendor|crates/oracle|.github)
  D  const X_RELATIVE_PATH = "path" ... const [EXPECTED_]X_SHA256 = "hash"
     (path unrestricted)
  E  const NAME = "path" ... [NAME]: "hash"   (ratchets|vendor|crates|.github)

Consumers: chain-walk-repin.py (refresh), pin-index.py (classification),
pin-normalize (receipt-key masking; mirrored in
crates/oracle/pin-normalize.mjs — the cross-check test in
.github/ci/gate-tax-5.test.mjs asserts python/mjs agreement on every
chain script), walk-topology-audit.py (producer ordering).

Extraction is a pure function of the text: no disk reads, no existence
checks (repin's path-existence filter is refresh-side only — an
existence-dependent receipt key would differ across machines).
"""
import hashlib
import json
import re
import sys

PLACEHOLDER = "<extracted-pin>"

PAT_A = re.compile(
    r'"((?:ratchets|vendor|crates/oracle|\.github)/[^"\n]+)",\s*"([0-9a-f]{64})"'
)
PAT_B = re.compile(
    r'"((?:ratchets|vendor|crates/oracle|\.github)/[^"\n]+)":\s*\n(\s*)"([0-9a-f]{64})"'
)
PAT_C = re.compile(
    r'"((?:ratchets|vendor|crates/oracle|\.github)/[^"\n]+)": "([0-9a-f]{64})"'
)
PAT_D_PAIRS = re.compile(r'const (\w+?)_RELATIVE_PATH =\s*\n?\s*"([^"]+)"')
PAT_E_CONSTS = re.compile(
    r'const (\w+) =\s*\n?\s*"((?:ratchets|vendor|crates|\.github)/[^"\n]+)"'
)


def d_hash_pattern(const_name):
    return re.compile(
        r'(const ' + re.escape(const_name) + r' =\s*\n?\s*")([0-9a-f]{64})(")'
    )


def e_hash_pattern(name):
    return re.compile(r'(\[' + re.escape(name) + r'\]:\s*\n?\s*")([0-9a-f]{64})(")')


def d_relative_path_pairs(text):
    """(base-name, path) for every const X_RELATIVE_PATH = "path"."""
    return dict(PAT_D_PAIRS.findall(text))


def e_path_constants(text):
    """(name, path) for every const NAME = "<pin-class path>"."""
    return dict(PAT_E_CONSTS.findall(text))


def extract_pins(text):
    """Every grammar hit as {path, grammar, start, end} where start/end
    span the 64 hex digits only. Deduplicated by span, sorted, and
    refused (ValueError) on overlapping distinct spans."""
    pins = []

    def add(path, grammar, start, end):
        for pin in pins:
            if pin["start"] == start and pin["end"] == end:
                return
        pins.append({"path": path, "grammar": grammar, "start": start, "end": end})

    for match in PAT_A.finditer(text):
        add(match.group(1), "A", *match.span(2))
    for match in PAT_B.finditer(text):
        add(match.group(1), "B", *match.span(3))
    for match in PAT_C.finditer(text):
        add(match.group(1), "C", *match.span(2))
    for base, path in d_relative_path_pairs(text).items():
        for const_name in (base + "_SHA256", "EXPECTED_" + base + "_SHA256"):
            match = d_hash_pattern(const_name).search(text)
            if match:
                add(path, "D", *match.span(2))
    for name, path in e_path_constants(text).items():
        for match in e_hash_pattern(name).finditer(text):
            add(path, "E", *match.span(2))

    pins.sort(key=lambda pin: (pin["start"], pin["end"]))
    for left, right in zip(pins, pins[1:]):
        if left["end"] > right["start"]:
            raise ValueError(
                f"overlapping extracted pin spans at {left['start']} and {right['start']}"
            )
    return pins


def group_pins(pins):
    """{(path, grammar): count} over extracted pins."""
    groups = {}
    for pin in pins:
        key = (pin["path"], pin["grammar"])
        groups[key] = groups.get(key, 0) + 1
    return groups


def classify(text, rows):
    """Positive selection of maskable spans against classification rows.

    rows = {"pins": [{path, grammar, count}], "semantic": [{path, grammar}]}
    Returns (masked_spans, refusals): a grammar hit whose (path, grammar)
    is enumerated under `pins` with a matching current count is masked; a
    `semantic` pair is left unmasked without refusal; anything else —
    unclassified pair or count drift — is a refusal row.
    """
    pins = extract_pins(text)
    enumerated = {
        (row["path"], row["grammar"]): row["count"] for row in rows.get("pins", [])
    }
    semantic = {(row["path"], row["grammar"]) for row in rows.get("semantic", [])}
    groups = group_pins(pins)
    masked, refusals = [], []
    for (path, grammar), count in sorted(groups.items()):
        key = (path, grammar)
        if key in semantic:
            if key in enumerated:
                refusals.append(
                    {"path": path, "grammar": grammar, "reason": "both-pin-and-semantic"}
                )
            continue
        if key not in enumerated:
            refusals.append({"path": path, "grammar": grammar, "reason": "unclassified"})
            continue
        if enumerated[key] != count:
            refusals.append(
                {
                    "path": path,
                    "grammar": grammar,
                    "reason": f"count-drift ({enumerated[key]} classified, {count} present)",
                }
            )
            continue
        masked.extend(
            pin for pin in pins if pin["path"] == path and pin["grammar"] == grammar
        )
    for row in rows.get("pins", []):
        if (row["path"], row["grammar"]) not in groups:
            refusals.append(
                {
                    "path": row["path"],
                    "grammar": row["grammar"],
                    "reason": "classified-but-absent",
                }
            )
    masked.sort(key=lambda pin: pin["start"])
    return masked, refusals


def normalize(text, rows):
    """(normalized_text, refusals). Masks classified spans with PLACEHOLDER."""
    masked, refusals = classify(text, rows)
    if refusals:
        return None, refusals
    out = []
    cursor = 0
    for pin in masked:
        out.append(text[cursor : pin["start"]])
        out.append(PLACEHOLDER)
        cursor = pin["end"]
    out.append(text[cursor:])
    return "".join(out), []


def normalized_sha256(text, rows):
    normalized, refusals = normalize(text, rows)
    if refusals:
        return None, refusals
    return hashlib.sha256(normalized.encode("utf-8")).hexdigest(), []


def load_index_rows(index_path, consumer):
    index = json.load(open(index_path, encoding="utf-8"))
    return index.get("consumers", {}).get(consumer, {"pins": [], "semantic": []})


def main(argv):
    if len(argv) >= 2 and argv[0] == "--extract":
        text = open(argv[1], encoding="utf-8").read()
        print(json.dumps(extract_pins(text), indent=2))
        return 0
    if len(argv) >= 2 and argv[0] == "--normalize":
        script = argv[1]
        index_path = ".github/ci/pin-index.v1.json"
        consumer = script
        rest = argv[2:]
        while rest:
            flag = rest.pop(0)
            if flag == "--index":
                index_path = rest.pop(0)
            elif flag == "--consumer":
                consumer = rest.pop(0)
            else:
                print(f"unknown flag {flag}", file=sys.stderr)
                return 2
        text = open(script, encoding="utf-8").read()
        digest, refusals = normalized_sha256(
            text, load_index_rows(index_path, consumer)
        )
        if refusals:
            print(json.dumps({"refusals": refusals}, indent=2))
            return 3
        print(digest)
        return 0
    print(
        "usage: pin-grammar.py --extract <file> | "
        "--normalize <file> [--index <pin-index>] [--consumer <relpath>]",
        file=sys.stderr,
    )
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
