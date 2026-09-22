# Comment source-map residual after agreed guard experiment

Please independently review the minimal repair, read-only, no edits/builds/delegation. Scope only printer comment/source-map handling. Worktree /Users/hiramatsu/dev/tsc-rs-emitter-final-audit. Pinned upstream vendor/typescript-6.0.3/lib/_tsc.js.

The agreed guard removal experiment passed 221/227 complete commands. Exactly six source-map mismatches remain: comment-boundary/{es2015,es2022,esnext}/remove-false/{optional-as,optional-non-null}. All JS, diagnostics, exit, and comment bytes match. removeComments=true and optional-angle pass. Full log integration/records/local/audit-expanded.log under docs/design/greenfield/slices/emitter-final-batch.

Inputs are in scripts/observe-import-helpers.mjs lines 145-149, e.g. `declare const object: { x: number } | undefined; export const value = (object?.x /* value */ as number);`. TS mapping suffix has `CAAC,CAAC,WAAsB,CAAC`, Rust has `CAAwB,CAAC`: missing mappings at emitted comment boundaries.

Codex initial hypothesis: printer.rs emit_partially_emitted_boundary_comments calls free emit_source_leading_comments_of_position at the erased wrapper child end, writing identical comment bytes without recording source maps. Need verify against TS emitPartiallyEmittedExpression / emitLeadingCommentsOfPosition / emitComment / emitPos path. Identify minimal shared typed comment map repair and any risk of duplicate ownership/maps; do not propose text-pattern special cases. I will inspect CLI failure and numeric target controls independently while you inspect this. Provide concise concrete functions and implementation advice.
