#!/usr/bin/env python3
"""gate-tax 8 S3: the harness pin manifest.

Single source for ratchets/pins/harness-expected.v1.json — the
committed manifest that replaces the volatile raw artifact-hash
literals in the converted harness integration tests (the manifest
diff is the review surface, in the escapes/symbol-diff mold; the
walk's tail then has nothing to refuse and certifies in ONE
invocation — the 2026-08-30 W4 baseline paid a second ~69-min walk).

Two sections:
  descriptors  reviewed structural authority (test_file, check_id,
               kind, path); writers never touch a byte of it; its
               sha256 over the canonical serialization is FROZEN in
               the dual anchor (pin-index row + the Rust helper
               const). check_id == path (unique per test_file).
  values       (test_file, check_id, sha256) — the ONLY writable
               section; every value is the CURRENT DISK sha256 of the
               descriptor's path (pure function of on-disk state —
               recovery-phase eligible). For `self-hash` rows the
               path is the artifact the test include_bytes!'s; for
               `file-hash` rows it is the referenced pinned file. In
               both cases the .rs assert then proves the artifact
               RECORDS the current on-disk file — strictly stronger
               than the literal it replaces.

Canonical descriptor serialization (anchor input, ASCII-asserted so
the Rust helper's BTreeMap+serde_json compact form agrees byte-wise):
json.dumps(descriptors, sort_keys=True, separators=(",", ":")).

Modes:
  --check              verifier: anchor(0) -> identity uniqueness ->
                       values/descriptors bijection -> re-derivation.
                       exit 0 clean, 1 stale values, 2 structural.
  --write              regenerate values (descriptors byte-asserted
                       pre/post); atomic temp+rename; journal line on
                       stdout. exit 0 written-or-current, 2 refused.
  --descriptor-digest  print the canonical descriptor sha256.
  --bootstrap          one-time extraction from the pre-conversion
                       .rs literals; refuses if the manifest exists.
"""
import hashlib
import json
import os
import re
import sys

MANIFEST = "ratchets/pins/harness-expected.v1.json"
# The python half of the dual descriptor anchor (the Rust half lives in
# crates/harness/tests/integration/support/pins.rs). Both are reviewed
# structural surfaces; a descriptor-set change updates BOTH in the same
# slice. (The packet named pin-index as the second host; pin-index's
# --check regenerates its fixed key set byte-exactly, so a foreign row
# cannot live there — this const is the same trust class.)
FROZEN_DESCRIPTOR_SHA256 = (
    "360e906e1f2d6e4b21526583c3fcf47dfef11b12f50b298715c363c17820f8e5"
)
RUST_HELPER = "crates/harness/tests/integration/support/pins.rs"
CONVERTED = ["h2_transition"] + [
    f"h2_{stage}_profile"
    for stage in ["1a", "1b", "1c", "1d", "1e",
                  "2a", "2b", "2c", "2d", "3a", "3b", "3c"]
]


def disk_sha256(path):
    try:
        return hashlib.sha256(open(path, "rb").read()).hexdigest()
    except OSError:
        return None


def canonical_descriptor_bytes(descriptors):
    text = json.dumps(descriptors, sort_keys=True, separators=(",", ":"))
    if not text.isascii():
        refuse("descriptor section is not ASCII-only")
    return text.encode()


def refuse(message):
    print(f"harness-pins REFUSED: {message}")
    sys.exit(2)


def load_manifest():
    document = json.load(open(MANIFEST, encoding="utf-8"))
    if document.get("schema") != 1:
        refuse(f"manifest schema {document.get('schema')!r} != 1")
    return document


def anchor_digests():
    found = {"verifier-const": FROZEN_DESCRIPTOR_SHA256}
    try:
        text = open(RUST_HELPER, encoding="utf-8").read()
    except OSError:
        refuse(f"rust anchor host {RUST_HELPER} unreadable")
    match = re.search(
        r'DESCRIPTOR_SHA256: &str =\s*\n?\s*"([0-9a-f]{64})"', text
    )
    if not match:
        refuse(f"no DESCRIPTOR_SHA256 const in {RUST_HELPER}")
    found["rust-helper"] = match.group(1)
    return found


def verify(document, report):
    descriptors = document.get("descriptors")
    values = document.get("values")
    if not isinstance(descriptors, list) or not isinstance(values, list):
        refuse("descriptors/values sections missing")
    digest = hashlib.sha256(canonical_descriptor_bytes(descriptors)).hexdigest()
    anchors = anchor_digests()
    for label, pinned in sorted(anchors.items()):
        if pinned != digest:
            refuse(
                f"descriptor anchor mismatch [{label}]: frozen {pinned[:12]}.. "
                f"!= manifest {digest[:12]}.. — a descriptor change needs its "
                "reviewed same-slice anchor update"
            )
    identities = [(row.get("test_file"), row.get("check_id"))
                  for row in descriptors]
    if len(set(identities)) != len(identities):
        refuse("duplicate descriptor identity")
    for row in descriptors:
        if row.get("kind") not in ("self-hash", "file-hash"):
            refuse(f"descriptor kind {row.get('kind')!r} unknown")
        if row.get("check_id") != row.get("path"):
            refuse("descriptor check_id != path (the v1 identity rule)")
        if row.get("test_file") not in CONVERTED:
            refuse(f"descriptor test_file {row.get('test_file')!r} is not a "
                   "converted test")
    value_identities = [(row.get("test_file"), row.get("check_id"))
                        for row in values]
    if sorted(value_identities) != sorted(identities):
        refuse("values/descriptors are not a bijection")
    stale = []
    for row in values:
        matching = next(
            descriptor for descriptor in descriptors
            if (descriptor["test_file"], descriptor["check_id"])
            == (row["test_file"], row["check_id"])
        )
        current = disk_sha256(matching["path"])
        if current != row.get("sha256"):
            stale.append(
                f"{row['test_file']}:{row['check_id']} pinned "
                f"{str(row.get('sha256'))[:12]}.. current "
                f"{(current or 'MISSING')[:12]}.."
            )
    if stale:
        if report:
            print(f"harness-pins STALE ({len(stale)} rows):")
            for line in stale:
                print(f"  {line}")
        return 1
    if report:
        print(
            f"harness-pins: clean ({len(descriptors)} rows, "
            f"{len(CONVERTED)} converted tests, anchors "
            + ",".join(sorted(anchors)) + ")"
        )
    return 0


def write(document):
    descriptors = document["descriptors"]
    before = canonical_descriptor_bytes(descriptors)
    new_values = [
        {
            "test_file": row["test_file"],
            "check_id": row["check_id"],
            "sha256": disk_sha256(row["path"]) or refuse(
                f"missing pinned file {row['path']}"
            ),
        }
        for row in descriptors
    ]
    if new_values == document["values"]:
        print("harness-pins: current (no value changed)")
        return 0
    updated = {"schema": 1, "descriptors": descriptors, "values": new_values}
    if canonical_descriptor_bytes(updated["descriptors"]) != before:
        refuse("write would alter the descriptor section")
    pre = disk_sha256(MANIFEST)
    temporary = os.path.join(
        os.path.dirname(MANIFEST), f".{os.path.basename(MANIFEST)}.tmp"
    )
    with open(temporary, "w", encoding="utf-8") as handle:
        json.dump(updated, handle, indent=1)
        handle.write("\n")
    os.replace(temporary, MANIFEST)
    changed = sum(
        1 for old, new in zip(document["values"], new_values) if old != new
    )
    print(
        f"harness-pins WRITTEN: rows-updated={changed} "
        f"pre={pre} post={disk_sha256(MANIFEST)}"
    )
    return 0


def bootstrap():
    if os.path.exists(MANIFEST):
        refuse(f"{MANIFEST} already exists — bootstrap is one-time")
    descriptors = []
    for test_file in CONVERTED:
        source = open(
            f"crates/harness/tests/integration/{test_file}.rs",
            encoding="utf-8",
        ).read()
        include = re.search(
            r'include_bytes!\(concat!\(\s*env!\("CARGO_MANIFEST_DIR"\),\s*"(/[^"]+)"',
            source,
        )
        self_hash = re.search(r'sha256\(RECORDED\),\s*"([0-9a-f]{64})"', source)
        rows = []
        if include and self_hash:
            path = include.group(1).replace("/../../", "")
            rows.append({"kind": "self-hash", "path": path})
        for match in re.finditer(
            r'"((?:ratchets|vendor|crates|\.github)/[^"\n]+)",\s*\n?\s*"[0-9a-f]{64}"',
            source,
        ):
            rows.append({"kind": "file-hash", "path": match.group(1)})
        paths = [row["path"] for row in rows]
        if len(set(paths)) != len(paths):
            refuse(f"{test_file}: duplicate pinned path — the check_id==path "
                   "identity rule needs review")
        for row in rows:
            descriptors.append(
                {
                    "test_file": test_file,
                    "check_id": row["path"],
                    "kind": row["kind"],
                    "path": row["path"],
                }
            )
    descriptors.sort(key=lambda row: (row["test_file"], row["check_id"]))
    values = [
        {
            "test_file": row["test_file"],
            "check_id": row["check_id"],
            "sha256": disk_sha256(row["path"]),
        }
        for row in descriptors
    ]
    os.makedirs(os.path.dirname(MANIFEST), exist_ok=True)
    with open(MANIFEST, "w", encoding="utf-8") as handle:
        json.dump(
            {"schema": 1, "descriptors": descriptors, "values": values},
            handle,
            indent=1,
        )
        handle.write("\n")
    digest = hashlib.sha256(canonical_descriptor_bytes(descriptors)).hexdigest()
    print(f"harness-pins BOOTSTRAPPED: rows={len(descriptors)} "
          f"descriptor-digest={digest}")
    return 0


def main():
    arguments = sys.argv[1:]
    if "--root" in arguments:
        at = arguments.index("--root")
        os.chdir(arguments[at + 1])
        del arguments[at:at + 2]
    else:
        os.chdir(
            os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
        )
    mode = arguments[0] if arguments else "--check"
    if mode == "--bootstrap":
        return bootstrap()
    if mode == "--descriptor-digest":
        document = load_manifest()
        print(hashlib.sha256(
            canonical_descriptor_bytes(document["descriptors"])
        ).hexdigest())
        return 0
    if mode == "--check":
        return verify(load_manifest(), report=True)
    if mode == "--write":
        document = load_manifest()
        if verify(document, report=False) == 2:
            return 2
        return write(document)
    refuse(f"unknown mode {mode!r}")


if __name__ == "__main__":
    sys.exit(main())
