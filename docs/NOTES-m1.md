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

## ast-diff status (2026-07-10)

`files=5908 compared=5242 excluded=666 differing=1`

The 666 exclusions are error fixtures (parse errors on at least one
side), per impl-nodes.md §5.

| file | classification |
|---|---|
| externalModules/topLevelAwait.1.ts | unported production: reparseTopLevelAwait (module files re-parse possible-top-level-await statements in AwaitContext; needs ContainsPossibleTopLevelAwait aggregation + externalModuleIndicator). `await [x]` parses as ElementAccess instead of AwaitExpression. |

## syntactic-diagnostic gate status (2026-07-10)

`T0-syntactic 73.33% (1647/2246), exact cases 6959/7691, FP 4275, FN 599`

Top FP codes: 1005 (3098), 1128 (333), 1109 (267), 1003 (220),
1434 (87) — recovery-order differences in statement/declaration error
attribution; the 1434-vs-1128 case below is a confirmed instance.
Top FN codes: 8002/8006/8010 (~301 total) — grammar errors tsc emits
only for JS files; .js inputs are currently parsed as TS (JS handling
unported), so these are out of M1 scope. 1109/1127/1125/1005 FN —
recovery-order differences, burn down with the FP list.

Known fidelity bug (drives part of 1434/1128 FP): for
`declare module "m" { export i` (identifier after `export` at EOF)
tsc emits 1128 "Declaration or statement expected" at `export`;
we emit 1434 "Unexpected keyword or identifier".

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
