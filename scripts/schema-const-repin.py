#!/usr/bin/env python3
"""gate-tax 8 S2: the enumerated schema-constant repin.

Single source for the ONE sanctioned schema-const pin family: the five
`/properties/current_exact_promotions/const/{0..4}/historical_qualification/sha256`
leaves of the H2.5g profile contract, all carrying the sha256 of
ratchets/h2-1a-qualification.v1.json. The 2026-08-30 W4 converge paid a
second full walk (~69 min) because this repair happened OUTSIDE the walk
transaction; the driver now applies it in-round right after the
h2-1a-qualification rung writes (repin-early, gt5-D extended) and the
recovery phase applies it idempotently after a kill window.

Extending coverage to ANY other schema constant is a new enumerated
entry in this module, landed by the slice introducing it — never a
generic pattern.

Modes:
  --check   report; exit 0 current, 1 stale, 2 invalid (asserts failed)
  --fix     apply; exit 0 applied-or-current, 2 refused (no partial
            write persists; atomic same-directory temp+rename)

Procedure (--fix): parse; assert schema identity, the five case
identities in order, per-row artifact path, and value count; compute
the new value from the on-disk artifact; text-replace the old value
(occurrence count must be exactly five); reparse; prove the change is
confined to the five pointers (model-level masked compare) AND to five
64-hex byte spans (byte-level diff); write atomically; emit a journal
line (pointers, old/new, pre/post file digests) on stdout.
"""
import hashlib
import json
import os
import sys

SCHEMA_PATH = ".github/ci/contracts/h2-5g-profile.schema.json"
ARTIFACT_PATH = "ratchets/h2-1a-qualification.v1.json"
POINTER_TEMPLATE = (
    "/properties/current_exact_promotions/const/{i}/historical_qualification/sha256"
)
EXPECTED_SCHEMA_ID = "h2-5g-profile"
EXPECTED_CASE_IDS = [
    "typescript-6.0.3/compiler/arrayFromAsync.ts#default",
    "typescript-6.0.3/compiler/arrayIterationLibES5TargetDifferent.ts"
    "#nolib%3Dtrue%2Ctarget%3Desnext",
    "typescript-6.0.3/compiler/mapGroupBy.ts#default",
    "typescript-6.0.3/compiler/objectGroupBy.ts#default",
    "typescript-6.0.3/compiler/regularExpressionScanning.ts#target%3Desnext",
]
ROW_COUNT = 5


def sha256_file(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


def refuse(message):
    print(f"schema-const-repin REFUSED: {message}")
    sys.exit(2)


def load_rows():
    text = open(SCHEMA_PATH, encoding="utf-8").read()
    document = json.loads(text)
    identity = document.get("$id", "")
    if EXPECTED_SCHEMA_ID not in identity:
        refuse(f"schema $id {identity!r} does not name {EXPECTED_SCHEMA_ID}")
    rows = (
        document.get("properties", {})
        .get("current_exact_promotions", {})
        .get("const")
    )
    if not isinstance(rows, list) or len(rows) != ROW_COUNT:
        refuse(f"promotion const rows: expected {ROW_COUNT}, found "
               f"{len(rows) if isinstance(rows, list) else 'none'}")
    for index, (row, expected_case) in enumerate(zip(rows, EXPECTED_CASE_IDS)):
        if row.get("case_id") != expected_case:
            refuse(f"row {index} case_id {row.get('case_id')!r} != "
                   f"{expected_case!r}")
        qualification = row.get("historical_qualification", {})
        if qualification.get("path") != ARTIFACT_PATH:
            refuse(f"row {index} historical_qualification.path "
                   f"{qualification.get('path')!r} != {ARTIFACT_PATH!r}")
        value = qualification.get("sha256", "")
        if not (isinstance(value, str) and len(value) == 64):
            refuse(f"row {index} sha256 is not a 64-hex value")
    values = {
        rows[index]["historical_qualification"]["sha256"]
        for index in range(ROW_COUNT)
    }
    if len(values) != 1:
        refuse(f"the five pinned values differ: {sorted(values)}")
    return text, document, values.pop()


def masked(document):
    clone = json.loads(json.dumps(document))
    rows = clone["properties"]["current_exact_promotions"]["const"]
    for row in rows:
        row["historical_qualification"]["sha256"] = "<masked>"
    return clone


def main():
    arguments = sys.argv[1:]
    root = None
    if "--root" in arguments:
        at = arguments.index("--root")
        root = arguments[at + 1]
        del arguments[at:at + 2]
    mode = arguments[0] if arguments else "--check"
    if mode not in ("--check", "--fix"):
        refuse(f"unknown mode {mode!r}")
    os.chdir(
        root
        if root
        else os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
    )

    text, document, pinned = load_rows()
    current = sha256_file(ARTIFACT_PATH)
    if pinned == current:
        print(f"schema-const: current ({SCHEMA_PATH} -> {ARTIFACT_PATH} "
              f"{current[:12]}..)")
        return 0
    if mode == "--check":
        print(f"schema-const STALE: {SCHEMA_PATH} pins {pinned[:12]}.. "
              f"but {ARTIFACT_PATH} is {current[:12]}..")
        return 1

    occurrences = text.count(pinned)
    if occurrences != ROW_COUNT:
        refuse(f"stale value appears {occurrences} times, expected "
               f"{ROW_COUNT} — the file diverges from the enumerated shape")
    replaced = text.replace(pinned, current)
    new_document = json.loads(replaced)
    if masked(new_document) != masked(document):
        refuse("the replacement changed bytes outside the five pointers "
               "(model-level masked compare failed)")
    for index in range(ROW_COUNT):
        row = new_document["properties"]["current_exact_promotions"]["const"][index]
        if row["historical_qualification"]["sha256"] != current:
            refuse(f"row {index} did not adopt the new value")
    spans = []
    cursor = 0
    for old_char, new_char in zip(text, replaced):
        if old_char != new_char and (not spans or cursor > spans[-1][1]):
            spans.append([cursor, cursor + 64])
        cursor += 1
    if len(text) != len(replaced) or len(spans) != ROW_COUNT:
        refuse(f"byte diff is not five equal-length 64-hex spans "
               f"(found {len(spans)})")

    pre_digest = hashlib.sha256(text.encode()).hexdigest()
    temporary = os.path.join(
        os.path.dirname(SCHEMA_PATH), f".{os.path.basename(SCHEMA_PATH)}.tmp"
    )
    with open(temporary, "w", encoding="utf-8") as handle:
        handle.write(replaced)
    os.replace(temporary, SCHEMA_PATH)
    post_digest = sha256_file(SCHEMA_PATH)
    pointers = ";".join(
        POINTER_TEMPLATE.format(i=index) for index in range(ROW_COUNT)
    )
    print(
        "schema-const-repin APPLIED: "
        f"pointers={pointers} old={pinned} new={current} "
        f"pre={pre_digest} post={post_digest}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
