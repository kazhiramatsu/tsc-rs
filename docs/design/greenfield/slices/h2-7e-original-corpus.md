# H2.7e original-corpus comparison

This local comparison joins the unchanged H2.7d/e candidate census, original
inputs, and complete TypeScript 6.0.3 observations from `239e51ed`. It does not
change a qualification artifact, runtime profile, CI configuration, or formal
adoption record. The three artifacts are SHA256-pinned by
`crates/compiler/tests/h2_7e_original_corpus.rs`; their case IDs, source identities,
input hashes, owner sets, and two-observation requirement must agree.

| Original membership | Count | This comparator |
| --- | ---: | --- |
| H2.7e only, declarations enabled | 7 | Complete ordinary Program tuple and real CLI, twice |
| H2.7e only, declaration absent | 1 | `declarationMap` typed refusal, twice |
| H2.7d + H2.7e | 3 | `outFile` typed refusal, twice |
| H2.7e + H2.8c | 2 | Original transpile API inputs, count-only reference |
| Union | 13 | 8 E-only + 3 compound + 2 later intersections |

The seven ordinary rows are `declarationMaps.ts`,
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
original virtual paths. These seven rows have no reported diagnostics or status
writes, and no location-dependent output options. No expected output is rewritten.

`declarationMapsWithoutDeclaration.ts` retains its absent `declaration` option.
TS6 reports TS5069, writes its JavaScript, returns `emitSkipped: false`, and leaves
`sourceMaps` absent. The current ordinary map packet instead retains its prior
typed `declarationMap` refusal with no writes. This is the one E-only residual;
the test does not manufacture `declaration: true` to make it pass.
The three bundle rows preserve the D-to-E handoff, and the two five-unit
transpile API inputs are never substituted with whole-Program runs.

Seven new complete runtime comparisons do not mean seven new corpus members.
Five of the seven IDs already occur in the frozen H2.6c membership (the two
static-block and three decorator cases); two IDs are new relative to that set.
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

Validation against ordinary runtime `acf34ad2`: the focused test passes in
32.02 seconds after a fresh build in its own worktree target, with all Program
and CLI comparisons repeated twice. The run logs its own compiler manifest and
binary paths and passes the runtime-directory assertion. The candidate preparer
`--check`, focused `cargo clippy --test h2_7e_original_corpus -- -D warnings`,
Rust formatting, and patch whitespace checks also pass. The existing three
census/oracle artifacts remain byte-for-byte unchanged.
