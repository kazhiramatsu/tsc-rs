# API1.2-HINT: declaration names and initializer hook hints

2026-09-16. Integrator: Codex. Base: `d9cfb664a` (PR #536).
Implementation and focused validation in `work/printer-hook-hints`.

This slice repairs the two already-observed producer differences assigned to
the integrator in [A-INT3-CS](h2-8a-printer-comment-carry/README.md#残る差分)
and the [completion plan](../remaining-completion-slices.md). It does not add
the full custom-transform API or alter runtime admission/profile state.

## Source contract and implementation boundary

Pinned TypeScript 6.0.3 `_tsc.js`:

- `emit` / `emitExpression`, 117145–117161: explicit `Unspecified` / `Expression` hints.
- `emitVariableDeclaration`, 118934–118940: `emit(node.name)`, followed by the initializer.
- `emitInitializer`, 119915–119922: `emitExpression(node, parenthesizerRule)` after `=`.
- Pipeline selection / notification, 117184–117222: substitution is selected before
  invoking the notification handler; the handler retains the original node/hint.

The existing Rust `VariableDeclaration` worker calls the identifier-name helper
for the binding name. Its initializer helper infers `Expression` only for an
Identifier, so a CallExpression receives `Unspecified`. The repair belongs to
these producer call sites in `crates/emitter/src/printer.rs`; the generic helper's
other callers keep their individually owned hint contracts. Comment ownership,
parenthesization and map recording use the existing typed pipeline.

## Observations and validation

`scripts/observe-printer-hook-hints.mjs` reuses the pinned C03 public handler
adapter, adding a hint-dependent substitution predicate. It adds no cleanup.
The new fixture is captured twice and frozen before native execution:
SHA-256 `40d33a2dce1b64959f79ae663f5a3c21a1fd65ed2dd44c31a86c78529310fb70`.

The 72 cases cover const, multiple let declarations, and array binding;
LF/CRLF; binding-name/initializer sites; success, before/substitute/after failure,
replacement and replacement followed by failure. Each case has a fresh control,
a successful seed, the selected operation, and same/other-source recovery.
The replacement is returned only for the expected hint, making hint errors
observable in output as well as the complete event trace. Rust pre-creates the
replacement because factory use during printing is a separate API boundary.

The native contract compares complete ordered events, output bytes, UTF-16
position, status and failure site twice. The existing 24 comment-carry inputs
and their TypeScript expectations remain immutable; four pinned native event
differences are removed only when they match those expectations. Generated-name
failure state remains C02 / A-INT2.

Local checks cover this direct target and adjacent printer owners. The existing
hosted printer job owns the added observer and fixture; common printer source
changes retain full related hosted acceptance/witness selection. Full legacy
local CI remains opt-in under [witness testing](../../../witness-testing.md).

## Results

The initial native execution returned exit 101: **0/72 complete exact**, all 72
event sequences differed, and 20 cases also had different returned output.
After the two producer repairs: **72/72 complete exact twice**. The existing
comment-carry set moves from 20/24 to **24/24 exact twice**, including the separate
replacement-transformation controls. Existing hooks remain 24/25 (one C02
generated-name gap), and the review set remains 21/21. The printer catalog is
therefore **141/142 exact, one explicitly retained gap**. The direct target's
10 tests pass; native-only safety and comparator controls are not added to the
compatibility denominator.

The [before receipt](api1-2-printer-hook-hints/records/before.v1.json) retains
the base, source/input and binary hashes, commands, exit, and compressed captures.
The four retired native-event expectations are preserved as
[historical evidence](api1-2-printer-hook-hints/records/comment-carry-before-events.json);
they no longer authorize runtime mismatches. The original upstream fixture
is byte-identical to the base.

Local validation at the final native bytes:

- Printer failure target: 10 tests; adjacent six targets: 24 tests.
- Exact noEmitOnError owner control: one test (451 intentionally filtered).
- Four upstream observers: frozen expectations reproduced, including the
  original C03 and comment-carry artifacts.
- CI planner: 36 tests; selected qualification/policy tests: 10 tests.
- Workspace fmt and diff checks pass. Targeted Clippy exits 0 with no diagnostic
  on a changed line; the existing program145/emitter16 warnings remain.

The [local receipt](api1-2-printer-hook-hints/records/local.v1.json) records
commands, hashes, exits, captures and compressed logs. Full related acceptance
and witness jobs are required on the PR head; their result is pending.
