# A-PC1: compact function-body comment ownership

2026-09-16. Integrator: Codex. Base: `e677f87385d87e97d972ad8423d0670778596a9b`.
Branch: `work/compact-body-comments`. Status: local validation passed; hosted validation pending.

The ES2015 parameter transform inserts temporary declarations and default-value
guards before the original body. The compact printer visits the list-owned
comment boundary only at index zero, dropping `/* body */` before the retained
return statement. This owner is separate from C03 failure-state restoration and
API1.2-HINT hook dispatch.

## Pinned source contract

TypeScript 6.0.3 `vendor/typescript-6.0.3/lib/_tsc.js`:

- `emitNodeListItems`, 120068–120140: visit each child's explicit comment range
  before emitting the item, unless a separating line terminator skips that phase.
- `emitTrailingCommentsOfPosition`, 121191–121217: the list phase owns its comments
  independently of node `NoLeadingComments` / `NoComments` flags.
- `forEachTrailingCommentToEmit`, 121233–121237: source/container end ownership.

The previous sibling's ordinary trailing phase can emit the same source comment.
The pinned printer actually repeats `/* body */` in `a(); /* body */ return x;`.
Deduplication would change TypeScript output. Global removal and enclosing
`NoNestedComments` still suppress the list phase.

The repair uses the existing typed comment-range/resume helpers at the compact
function-body list call site. It does not widen raw source fallback or change
transform admission, parameter lowering, the node pipeline, or source-map models.

## Frozen inputs and comparison

The existing `h2-5h-parameter-temporaries.json` artifact and observer stay byte
identical. Five existing focused IDs are reproduced before editing production:

- `parameter-temporaries/comments-lf/es2015`
- `parameter-temporaries/comments-crlf/es2015`
- `parameter-temporaries/source-map/es2015`
- `parameter-temporaries/bom/es2015`
- `parameter-temporaries/no-emit-on-error/es2015`

Each uses two fresh complete commands: writes/bytes/BOM/callback metadata,
diagnostics, emit results, maps, status, exit and partial-write/error fields.
Adjacent ES5 inputs and the existing 68-case target retain the same comparator.

`observe-compact-body-comments.mjs` independently freezes 240 printer outputs
twice before the production edit: first/middle/non-BMP body layouts; no prefix,
two synthetic statements, explicit comment-range prefix, and an explicit
comment-range prefix after a forced line break; five flag variants; LF/CRLF;
retain/remove comments. Explicit comment metadata does not mutate raw synthetic
positions. Rust compares full text, UTF-8 bytes and UTF-16 length twice.

The new printer target and existing parameter target get explicit hosted entries.
Local checks are focused; all related hosted acceptance and witness jobs must pass
before landing, following [witness testing](../../../witness-testing.md).

## Local results

The five pre-existing commands move from **0/5 to 5/5 complete exact twice**.
The source-map case repairs both emitted map bytes and the recorded URL position;
all other command fields retain the upstream expectation. The original 12 ES5
commands remain **12/12 complete exact twice**. The frozen parameter artifact,
observer and test comparator are byte-identical to the base.

New direct controls move from **152/240 to 240/240 exact twice** at unchanged
fixture bytes. Seven adjacent printer targets pass all **34 tests**, including
C03 failure/reuse, comment carry, hook hints, source topology and list flags.
These safety/control tests are not added to the complete-command denominator.
The CI planner passes 36 tests and the selected policy suite passes 10 tests.
Formatting, diff checks and inventory regeneration pass. Targeted Clippy exits
zero with no changed-line diagnostic; existing program145/emitter16 warnings
remain and are preserved in the log.

The [before receipt](h2-8a-compact-body-comments/records/before.v1.json) preserves
source, input and binary hashes, complete observations, differing fields and
native failure captures. The [local receipt](h2-8a-compact-body-comments/records/local.v1.json)
records final source hashes, commands, results and compressed captures/logs.
The initial direct run used a relative capture path that did not exist; it was
repeated with an absolute path before editing production. Both paths are recorded.

The explicit hosted entries cover all 240 new printer cases and all 68 existing
parameter commands. [Inventory v8](witness-coverage/inventory.v8.json) records
66 standalone targets, 24 unfiltered, seven filtered and 35 without a direct
entry. Hosted execution and timings will be recorded separately from this local
and static-entry evidence.
