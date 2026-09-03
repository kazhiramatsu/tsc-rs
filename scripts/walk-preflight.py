#!/usr/bin/env python3
"""gate-tax 5-F: report ALL stale pin surfaces at once, in seconds.

Measured on the gt4 landing: three walk runs (~65 min of avoidable
rounds) discovered pin surfaces SEQUENTIALLY — harness pins, then policy
source pins, then a schema promotion const — and a sixth family (fuzz
manifest source references) red-ended the gate ~40 minutes in. Every one
of these is checkable in seconds. The driver runs this right after
fmt/clippy AND at the walk tail (post-mint staleness, e.g. a schema
const pinning a just-re-minted artifact, is then caught at walk end
instead of gate-time); the gate's WALK_DRY structural preflight runs it
too.

Surfaces:
  1. harness integration-test pins        (scripts/pin-audit.py)
  2. pin-index classification             (scripts/pin-index.py --check)
  3. hosted-policy rust_source_sha256 map (.github/ci/qualification-policy.v2.json)
  4. schema-contract embedded {path, sha256} consts (.github/ci/contracts/)
  5. fuzz manifest source references      (ratchets/fuzz-preflight.v1.json,
                                           ratchets/fuzz-domain.v1.toml,
                                           ratchets/fuzz-oracle-deviations.v1.json)
  6. post-5g ORDER/schema registry surface (scripts/chain-walk.sh,
                                            .github/ci/qualification.mjs)

Exit: 0 clean, 1 stale surfaces (ALL reported, never fail-first).
"""
import glob
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
HEX64 = re.compile(r"[0-9a-f]{64}")
POLICY = ".github/ci/qualification-policy.v2.json"
FUZZ_PREFLIGHT = "ratchets/fuzz-preflight.v1.json"
FUZZ_DOMAIN = "ratchets/fuzz-domain.v1.toml"
FUZZ_ORACLE = "ratchets/fuzz-oracle-deviations.v1.json"
CHAIN_WALK = "scripts/chain-walk.sh"
QUALIFICATION = ".github/ci/qualification.mjs"
ORDER_START = "h2-5g-qualification"


def disk_sha256(relative):
    try:
        return hashlib.sha256((ROOT / relative).read_bytes()).hexdigest()
    except OSError:
        return None


def embedded_pairs(node, out):
    if isinstance(node, dict):
        path, digest = node.get("path"), node.get("sha256")
        if (
            isinstance(path, str)
            and isinstance(digest, str)
            and HEX64.fullmatch(digest)
        ):
            out.append((path, digest))
        for value in node.values():
            embedded_pairs(value, out)
    elif isinstance(node, list):
        for value in node:
            embedded_pairs(value, out)


def source_pin_maps(node, out):
    """Every {path: sha256} map stored under a *_sha256 key (the hosted
    policy's rust_source_sha256 shape)."""
    if isinstance(node, dict):
        for key, value in node.items():
            if (
                key.endswith("_sha256")
                and isinstance(value, dict)
                and value
                and all(
                    isinstance(v, str) and HEX64.fullmatch(v)
                    for v in value.values()
                )
            ):
                out.append((key, value))
            else:
                source_pin_maps(value, out)
    elif isinstance(node, list):
        for value in node:
            source_pin_maps(value, out)


def subprocess_surface(problems, label, arguments):
    result = subprocess.run(
        arguments, cwd=ROOT, capture_output=True, text=True
    )
    if result.returncode != 0:
        body = (result.stdout + result.stderr).strip()
        problems.append(f"[{label}] exit {result.returncode}\n{body}")


def pair_surface(problems, label, pairs):
    for path, pinned in pairs:
        current = disk_sha256(path)
        if current != pinned:
            problems.append(
                f"[{label}] {path}: pinned {pinned[:12]}.. current "
                f"{(current or 'MISSING')[:12]}.."
            )


def chain_order():
    source = (ROOT / CHAIN_WALK).read_text(encoding="utf-8")
    match = re.search(r"^ORDER=\(\n(.*?)^\)\n", source, re.MULTILINE | re.DOTALL)
    if not match:
        raise ValueError(f"ORDER block not found in {CHAIN_WALK}")
    return re.findall(r"[a-z0-9-]+", match.group(1))


def registered_schema_rungs():
    source = (ROOT / QUALIFICATION).read_text(encoding="utf-8")
    match = re.search(
        r"^export const ARTIFACT_SCHEMA_CONTRACTS = Object\.freeze\(\[\n(.*?)^\]\);",
        source,
        re.MULTILINE | re.DOTALL,
    )
    if not match:
        raise ValueError(f"ARTIFACT_SCHEMA_CONTRACTS registry not found in {QUALIFICATION}")
    return set(
        re.findall(
            r'\bschema:\s*"\.github/ci/contracts/([a-z0-9-]+)\.schema\.json"',
            match.group(1),
        )
    )


def registry_surface(problems):
    try:
        order = chain_order()
        registered = registered_schema_rungs()
    except (OSError, ValueError) as error:
        problems.append(f"[registry] {error}")
        return
    try:
        start = order.index(ORDER_START)
    except ValueError:
        problems.append(f"[registry] {ORDER_START} is absent from {CHAIN_WALK}")
        return
    for rung in order[start:]:
        schema = f".github/ci/contracts/{rung}.schema.json"
        if not (ROOT / schema).is_file():
            problems.append(f"[registry] missing schema {schema}")
        if rung not in registered:
            problems.append(f"[registry] {schema} is absent from {QUALIFICATION}")


def main():
    problems = []
    subprocess_surface(problems, "harness-pins", ["python3", "scripts/pin-audit.py"])
    # gate-tax 8 S3: the harness pin manifest verifier (dual descriptor
    # anchor -> identity/bijection -> per-value re-derivation). Runs here
    # at startup AND at the walk tail; a stale `values` section is
    # recovery-phase repairable, a descriptor anomaly never is.
    subprocess_surface(
        problems, "harness-manifest", ["python3", "scripts/harness-pins.py", "--check"]
    )
    subprocess_surface(
        problems, "pin-index", ["python3", "scripts/pin-index.py", "--check"]
    )

    maps = []
    source_pin_maps(json.load(open(ROOT / POLICY)), maps)
    if not maps:
        problems.append(f"[policy-source-pins] no *_sha256 map found in {POLICY}")
    for key, mapping in maps:
        pair_surface(
            problems, f"policy-source-pins:{key}", sorted(mapping.items())
        )

    for contract in sorted(glob.glob(str(ROOT / ".github/ci/contracts/*.schema.json"))):
        pairs = []
        embedded_pairs(json.load(open(contract)), pairs)
        pair_surface(
            problems,
            f"schema-const:{pathlib.Path(contract).name}",
            pairs,
        )

    fuzz_pairs = []
    embedded_pairs(json.load(open(ROOT / FUZZ_PREFLIGHT)), fuzz_pairs)
    pair_surface(problems, f"fuzz-manifest:{FUZZ_PREFLIGHT}", fuzz_pairs)
    domain_pairs = []
    embedded_pairs(
        tomllib.load(open(ROOT / FUZZ_DOMAIN, "rb")), domain_pairs
    )
    pair_surface(problems, f"fuzz-manifest:{FUZZ_DOMAIN}", domain_pairs)
    # h2-7a-m-3.5 P7 repair: the oracle-deviations manifest pins checker
    # sources too (state.rs drifted past a clean walk once).
    oracle_pairs = []
    embedded_pairs(json.load(open(ROOT / FUZZ_ORACLE)), oracle_pairs)
    pair_surface(problems, f"fuzz-manifest:{FUZZ_ORACLE}", oracle_pairs)
    registry_surface(problems)

    if problems:
        print(f"walk-preflight: {len(problems)} stale pin surface(s) — fix ALL, then walk ONCE:")
        for problem in problems:
            print(f"  {problem}")
        return 1
    print("walk-preflight: all pin surfaces clean (harness, harness-manifest, pin-index, policy, schema-consts, fuzz-manifests, registry)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
