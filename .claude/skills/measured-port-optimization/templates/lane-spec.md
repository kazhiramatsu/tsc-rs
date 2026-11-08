# Lane `<name>` — <one-line goal>

Base: this clone's HEAD (`<commit>`, = main [+ which merged lanes]). Policy: outputs (diagnostics, emit, conformance) byte-identical; the
`--extendedDiagnostics` work counters may change and are recorded. Reference: tsgo at `/Users/hiramatsu/dev/tsc-rs/target/typescript7/upstream/tsc/internal/`
(read-only) [and tsrs at `tsrs-src/` if present]. Evidence in `target/lane-<name>/evidence/` (profiles, `callers.py`, notes, catalog rows).

## Evidence (why these rows)

<the measured facts: phase ledger buckets, same-name excess, entry-count ratios, callers two levels up; cite the files>

## Rows (one patch each)

| # | row | site (file:line) | reference change / delta | expected |
| --- | --- | --- | --- | --- |
| R1 | … | … | … | … |

## Rules (mandatory; the integrator verifies every claim)

- Work only in this clone; the sandbox cannot commit. One patch per row under `target/lane-<name>/patches/R1-….patch` …, plus `ALL.patch`;
  patches independent where possible (say so when they are a series).
- Output identity is the law; no mode flags, env knobs or parallel variants; no `unsafe` outside `store`/`arena`; no new dependencies.
- Load: `nice -n 20 cargo … -j 2`, `CARGO_INCREMENTAL=0`, `--offline`, one cargo command at a time, tests `-- --test-threads=1`.
  Allowed: `cargo check` of touched crates, focused `cargo test -p <crate> --lib <modules>`, `cargo fmt`, `cargo clippy -p <crate> -- -D warnings`,
  **one** dev build of `tsc-rs-cli` for a smoke run on `~/dev/bench-corpora/hono` (`-p tsconfig.bench-noemit.json --pretty false`,
  `TSRS_WORKERS=1 TSRS_CHECKERS=1`; stdout, exit code and emitted files identical to the unmodified base build you make first; record
  `--extendedDiagnostics` Symbols/Types/Instantiations before/after). No release builds, no benchmarks, no performance claims.
- Read tsgo's function for every site you touch and the reference's version; state which side you followed and why the result cannot differ.
- `STATUS.md` at the clone root: per row — done / verified (command + quoted output) / open / blocked, with before/after counts from
  temporary counters you remove before the patch. Time box: one working day. Stop at the box and write STATUS.md.
