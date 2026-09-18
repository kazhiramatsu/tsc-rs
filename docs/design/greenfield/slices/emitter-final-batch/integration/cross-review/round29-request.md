# r29: ordinary emitter CI now exposes two more issues

Read-only review, no edits, Cargo or subagents. Inspect the CI-repair worktree
/Users/hiramatsu/dev/tsc-rs-emitter-final-ci-repair, which is free for fixes.
Do not change audit or integration: their Rust validations are frozen/running.
Fable unless rate limited, then we switch to Opus.

1. H2.1a strict promotion now verifies all diagnostics but finds a JS byte
difference in privateNameInInExpression: three empty for-in blocks print
`{  /**/}` instead of TS `{ /**/ }`. The original whitespace/private-name
section is exact; we are preserving the historical expected observations.
Full CI log /tmp/emitter-ci-105710436367.log.
Codex traced printer.rs emit_empty_block_comments: the single-line branch
writes a list space before emit_empty_node_array_boundary_comments, which
also writes the open-brace trailing comment's prefix space and never writes
the list space after that comment. TS emitBlockStatements first emits open
brace token + trailing comments, then SingleLineBlockStatements (empty list
writes one space), then close-brace leading comments. Proposed small fix:
generalize existing emit_empty_multiline_block_boundary_comments with a
multi_line argument. Preserve its trailing comment loop. For multiline keep
line/indent behavior; for single line write the list space between trailing
and leading phases. Use it for both empty block layouts; leave generic empty
array/list comment helper unchanged. Add complete commands/maps for empty
loop/if/while blocks and function bodies, one/multiple/multiline comments,
removeComments and ES5/ESNext. Check subtle function-body suppression behavior.

2. H2.6c now gets past the absent empty manifest but
h2_6c_refusal_migrations::validate fails on the retired case-insensitive
sourceMapWithNonCaseSensitiveFileNames row: it requires its prior outFile
refusal vector in the live manifest. This row is already exact in EF3 x2
after the earlier oracle case-sensitivity correction. Its old-input and
current D/E inputs/identities differ, so do not silently substitute inputs.
Full CI log /tmp/emitter-ci-105710436512.log. Review the best retirement
mechanism, expected request counters and refusal totals. CURRENT has one
remaining migration and de_registry_contracts.rs requires nonempty current
registries; simply emptying CURRENT would make those mutation tests fail.
We want strict current full comparison plus original historical evidence,
not keeping a fake live refusal or reactivating an old mismatch vector.
Recommend the smallest robust change that keeps nonvacuous negative guards,
and identify other stale promotion pins from the two corrected oracle rows.

Report concrete code paths and proposed checks, about 50 lines. Codex will
independently implement/measure; do not declare either native result green.
