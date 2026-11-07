# Reproduction

Run from the repository root, with no network and one Cargo command at a time:

```sh
python3 scripts/generate_records.py
python3 scripts/generate_records.py --check
python3 crates/records-preview/tools/memory_report.py
python3 crates/records-preview/tools/memory_report.py --check
python3 crates/records-preview/tools/test_generator.py
nice -n 20 cargo test --offline -p tsc-rs-records-preview -j 2 -- --test-threads=1
nice -n 20 cargo clippy --offline -p tsc-rs-records-preview -j 2 --all-targets --no-deps -- -D warnings
```

`test_generator.py` computes generator output twice without modifying it, checks
byte identity against disk, invokes `--check`, verifies source transcription
(including multiline field types), audits three Go embedding paths against
independent expected facts, checks the per-kind 64-byte rule, and reproduces report
arithmetic. The Rust tests exercise field round trips, both visitOrder branches,
all token kinds, zero-copy tails, candidate inline/spill behavior, mapper forms,
packed sentinel states, declaration origins and thread-bound rejection.

Input snapshots have SHA-256 checksums in `inputs/manifest.json`. Original paths
and source hashes are in `inputs/provenance.json`. `go-structs.json` retains
verbatim struct bodies and original line numbers from ast.go/ast_generated.go,
plus Binder and checker SwitchStatementLinks evidence. Generation and reporting
never open the original external paths.

The census lane should emit a single flat JSON object with exactly the following
22 keys (case-sensitive). This is a **shape example with zero placeholders**, not
measured data; replace them with counts before running the report:

```json
{
  "Object": 0,
  "Intrinsic": 0,
  "Literal": 0,
  "Enum": 0,
  "UniqueESSymbol": 0,
  "Union": 0,
  "Intersection": 0,
  "EvolvingArray": 0,
  "Reference": 0,
  "TupleTarget": 0,
  "TypeParameter": 0,
  "TemplateLiteral": 0,
  "StringMapping": 0,
  "Index": 0,
  "IndexedAccess": 0,
  "Mapped": 0,
  "ReverseMapped": 0,
  "Conditional": 0,
  "Substitution": 0,
  "GenericType": 0,
  "mapped_cold_materialized": 0,
  "conditional_cold_materialized": 0
}
```

Every value is a nonnegative integer (JSON booleans/floats are rejected). The 20
TypeData-kind counts, **excluding the two cold counters**, must sum to the pinned
Type memory row, **2,575,521**. Count each resident Type once under its TypeData
variant at the same census point as that memory row. `Object` and `GenericType`,
for example, are distinct exact variant names, not aggregate categories.

`mapped_cold_materialized` counts resident Mapped types whose generated cold
links block exists; `conditional_cold_materialized` counts the analogous
Conditional blocks. Each must be at most its own kind's population. Count blocks
actually retained, not nonzero fields or cumulative allocation attempts. Both
blocks cost 16 arena-rounded bytes each (Mapped has 12 repr(C) bytes). If the
census runs on the old representation, derive materialization from the proposed
lazy allocation policy for the fields listed under Mapped/Conditional in REPORT;
do not substitute the mere presence of a universal TypeLinks slot.

```sh
python3 crates/records-preview/tools/memory_report.py --type-populations /path/to/type-populations.json
```

The existing `--type-populations` flag and the original 20-kind-only JSON remain
supported. Supply either both cold counters or neither. With both, the report
also prints common + cold allocated bytes; without them it reports common bytes
only and leaves cold occupancy unmeasured. Unknown keys, incomplete kind lists,
a single cold counter, or inconsistent counts fail validation. This command
regenerates REPORT.md with the supplied census; default generation still uses the
bundled B-3 input and labels absent measurements honestly.

`binder-fields.derived.json` also includes `rustWriters.maps`: every per-node map,
writer file:line, method/function, kind bound (`any` if not syntactically bounded),
and separate dispatch evidence. `tools/binder_census.py` discovers mutable sites;
source hashes in nodes.layout2.json pin the manual guard audit. Go embeds remain
the preview tail authority. Every missing/uncertain Rust placement explicitly
retains the original per-file map; no consumer or map is removed here.

The owned `data()` section is reproduced from the generated Rust method bodies
for the ten highest raw node populations. It counts source field reads and
distinct words, plus the debug-only kind check in `tail()`. These are static
accounting inputs for ceiling (iv), not benchmark or assembly results.

The `repr(C)` layouts are the proof artifact. The Vec-backed arena is a safe
shape model with explicit borrow guards; its timings, host allocations and
committed_bytes() are not native-arena measurements. Read STATUS.md for the
integration differences and the permitted node-preview boundary.
