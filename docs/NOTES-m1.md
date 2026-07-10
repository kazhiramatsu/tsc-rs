# M1 final-gate triage notes

Per m1-parser-steps.md "Final gate": every residual mismatch gets a
one-line classification here. Nothing is silenced.

## Gate tooling (built 2026-07-10)

- `cargo xtask conformance --syntactic-only` — T0 restricted to the
  oracle's getSyntacticDiagnostics set. Requires schema-2 goldens
  (pass provenance); regenerate with `cargo xtask oracle-refresh`.
- `cargo xtask ast-dump <file>` / `cargo xtask ast-diff --corpus` —
  (kind, pos-utf16, end-utf16) tree diff vs the tsc oracle
  (crates/oracle/ast-dump.mjs). Files with parse errors on either side
  are excluded from the tree gate (covered by the diagnostic gate);
  full per-file first-diffs land in target/ast-diff-failures.txt.

## ast-diff status (2026-07-10, after burndown 8)

`files=5908 compared=5344 excluded=564 differing=0` — CLEAN.

The 564 exclusions are error fixtures (parse errors on at least one
side), per impl-nodes.md §5. reparseTopLevelAwait is ported (module
files re-parse possible-top-level-await statement runs in the Await
context; ContainsPossibleTopLevelAwait is simulated by a subtree scan
whose only source is an identifier spelled `await`, with the factory
strip/clear boundaries), together with
SourceFile.externalModuleIndicator (isFileProbablyExternalModule
incl. the import.meta tree walk).

## syntactic-diagnostic gate status (2026-07-10, after burndown 10)

**GATE MET**: `T0-syntactic 99.82% (2242/2246), exact cases
7687/7691, FP 0 (NEW_FP hard gate green), FN 4`. The ratchet
[t0-syntactic] is locked at 0.998219.

The 4 remaining FN are one fixture:

| fixture | classification |
|---|---|
| node/nodeModulesTripleSlashReferenceModeOverrideModeError.ts (×4 matrix) | unported subsystem: comment pragma processing (processCommentPragmas / processPragmasIntoFields). tsc validates `/// <reference types="..." resolution-mode="...">` and emits 1453 for an invalid resolution-mode value; we do not parse triple-slash pragmas yet. |

(The burndown history — parseJsonText, isStartOfStatement, binding
elements, createMissingNode/createIdentifier, isBinaryOperator,
allowJs gating, Invalid_character, parseSuperExpression, escape
flags, template rescan, JS 8xxx walker, isTypeOnly/isExportEquals
slots, reparseTopLevelAwait, optional chains, 1260, JavaScriptFile
gates, private-name messages, 1010 position — lives in the
`m1 gate burndown 1..10` commit messages.)

## prefix-determinism invariant: unsatisfiable as formulated

`cargo xtask invariants --suite prefix-determinism` fails on
ambient/ambientDeclarationsPatterns_merging3.ts (types.ts truncated at
its midpoint). The invariant requires diagnostics strictly before the
cut to be unchanged by truncation. Counterexample against tsc 6.0.3
itself:

- full text `declare module "*.foo" {\r\n  export interface OhNo ... }`
  → no diagnostics before offset 36;
- prefix `declare module "*.foo" {\r\n  export i` →
  tsc reports 1128 at 28..34 (`export`), i.e. BEFORE the cut.

Recovery attributes errors to earlier tokens depending on later text,
so a tsc-faithful parser cannot satisfy statement-level
prefix-determinism. The scanner-level property (M1a token dumps) does
hold. Decision needed: reformulate the invariant (e.g. token-level
only, or compare prefix parses against the oracle's prefix parse)
rather than weakening the parser to pass it.
