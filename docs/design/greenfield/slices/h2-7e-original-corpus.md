# H2.7e original-corpus comparison

## Current result and close addendum

The final E-only comparison remains exact for all eight original Program
tuples, twice, plus 16 CLI runs; its final correction rerun took 18.94s. The three
unchanged D/E Bundle inputs now also pass complete production comparison twice
through the [D original comparator](h2-7d-original-corpus.md), including full
`.d.ts.map` bytes and observations. E therefore covers 11 exact original cases,
with its two original transpile references still H2.8c-owned. Those three shared
cases are counted once within the joint 291-case band; D 283 and E 11 cannot be
summed to 294. The formal ordered deltas are D 280 then E 11.

The [shared close](h2-7de-ca.md) links accepted runtime candidate
`7db1e4d595d5f5ca25c569a80190ecdfddd8a8c9`, local regressions, passing hosted run
34123778139 and the 2026-09-07 adoption through runtime merge
`ad84a7e6e46bd7c5ccd9ac1eb78580dab18151a2`. The m3 Bundle-refusal
table and validation below remain historical evidence, superseded only for the
three shared production routes by the complete D/E comparison. Their original
inputs, TS expected tuples and observation artifacts remain unchanged.

## M3 comparison record (historical)

This local comparison joins the unchanged H2.7d/e candidate census, original
inputs, and complete TypeScript 6.0.3 observations from `239e51ed`. It does not
change a qualification artifact, runtime profile, CI configuration, or formal
adoption record. The three artifacts are SHA256-pinned by
`crates/compiler/tests/h2_7e_original_corpus.rs`; their case IDs, source identities,
input hashes, owner sets, and two-observation requirement must agree.

| Original membership | Count | This comparator |
| --- | ---: | --- |
| H2.7e only, declarations enabled | 7 | Complete ordinary Program tuple and real CLI, twice |
| H2.7e only, declaration absent | 1 | Complete ordinary Program tuple and real CLI, twice, including TS5069 |
| H2.7d + H2.7e | 3 | `outFile` typed refusal, twice |
| H2.7e + H2.8c | 2 | Original transpile API inputs, count-only reference |
| Union | 13 | 8 E-only + 3 compound + 2 later intersections |

The seven declaration-enabled ordinary rows are `declarationMaps.ts`,
`declarationMapsMultifile.ts`, the two `classStaticBlock25.ts` targets, and the
three `esDecorators-classDeclaration-sourceMap.ts` targets. They keep their
original source bytes, source ordering, roots, effective options, virtual
`/.src` current directory, and case-sensitive host. The pinned standard libraries
mount at `/lib`; the original observation already names libraries by basename.
There is no declaration-only control transformation or option floor.

The Program comparison covers program source order and ordered libraries,
reported/emit diagnostics including related information, callback order and
paths, every callback/materialized UTF-8 byte and BOM, callback source files and
metadata, `emitSkipped`, and exact `emittedFiles`/`sourceMaps` absence or contents.
Map JSON remains a byte-exact string. Artifact kinds independently distinguish
JavaScript maps from declaration maps before projecting the observer's common
`source-map` label. Activity counters are separate runtime instrumentation and
are deliberately outside this expectation tuple.

The real CLI additionally compares exit status, stdout/stderr, and all
materialized output files in a temporary input tree. This supplementary check
relocates inputs for the physical filesystem; the Program comparison keeps the
original virtual paths. The seven declaration-enabled rows have no reported
diagnostics or status writes, and no location-dependent output options. No
expected output is rewritten.

`declarationMapsWithoutDeclaration.ts` retains its absent `declaration` option.
TS6 reports TS5069, writes its JavaScript, returns `emitSkipped: false`, and leaves
`sourceMaps` absent. The m3 ordinary map packet now matches this eighth original
tuple, including the reported diagnostic and exit status 2. The test does not
manufacture `declaration: true` to make it pass. The Program observation has no
config file and keeps the diagnostic's absent location. For the additional real
CLI check, the same relocated config is executed with the pinned TS6 CLI and
stdout/stderr/exit are compared, including the config diagnostic position.
All Rust output bytes and the absence of extra files are checked against the
frozen observation before TS can write any files into that temporary tree.
The three bundle rows preserve the D-to-E handoff, and the two five-unit
transpile API inputs are never substituted with whole-Program runs.

Eight complete runtime comparisons do not mean eight new corpus members.
Five of the eight IDs already occur in the frozen H2.6c membership (the two
static-block and three decorator cases); three IDs are new relative to that set.
The m3 change adds one exact runtime comparison to the previous seven and one
additional unique ID relative to H2.6c; it does not add a candidate ID.
Across all 13 E candidates the H2.6c overlap is six, including the compound
`declarationMapsWithSourceMap.ts`. This historical membership is not inherited
Rust success. The candidate union remains D315 + E13 - compound3 = 325, and this
test adds no IDs or admissions to it. Formal admission remains the root packet's
separate decision; all three historical artifacts retain their original status.

Reproduce the focused comparison with one worker and a target directory owned
by the current worktree. The test verifies that Cargo's runtime package directory
matches its compile-time manifest directory; build targets must not be shared
between worktrees.

```sh
CARGO_TARGET_DIR=target/h2-7e-original-check CARGO_BUILD_JOBS=1 cargo test -p tsc-rs-compiler --test h2_7e_original_corpus -- --test-threads=1
```

Validation against m3 runtime `5d25aac9` (local cherry-pick `bfe99221`): all eight
Program and CLI comparisons pass twice in 36.00 seconds after a 9m26s rebuild
in the same worktree's dedicated target. The previous seven exact rows remain
unchanged; the three typed bundle refusals and two transpile references remain.
The run logs its own compiler manifest and binary paths and checks their
workspace identity. The candidate preparer `--check`, focused
`cargo clippy --test h2_7e_original_corpus -- -D warnings` (33.36s), Rust
formatting and patch whitespace checks pass. The existing three census/oracle
artifacts remain byte-for-byte unchanged.
