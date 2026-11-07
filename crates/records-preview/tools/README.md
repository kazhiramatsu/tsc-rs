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

A future TypeData-kind census can be supplied as a JSON object with all 20 variant
names and nonnegative integer populations:

```sh
python3 crates/records-preview/tools/memory_report.py --type-populations /path/to/type-populations.json
```

Its sum must equal the recorded Type memory row (2,575,521). The supplied B-3
stream has no such distribution, so the default report labels every per-kind
population unmeasured. Materialized per-kind cold-link counts and node-list
length parity are also missing; the report exposes those costs separately.

The `repr(C)` layouts are the proof artifact. The Vec-backed arena is a safe
shape model with explicit borrow guards; its timings, host allocations and
committed_bytes() are not native-arena measurements. Read STATUS.md for the
integration differences and the permitted node-preview boundary.
