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

## prefix-determinism invariant: reformulated (2026-07-10)

The original diagnostic-level formulation (diagnostics strictly before
a cut unchanged by truncation) is unsatisfiable for a tsc-faithful
parser. Counterexample against tsc 6.0.3 itself:

- full text `declare module "*.foo" {\r\n  export interface OhNo ... }`
  → no diagnostics before offset 36;
- prefix `declare module "*.foo" {\r\n  export i` →
  tsc reports 1128 at 28..34 (`export`), i.e. BEFORE the cut.

Recovery attributes errors to earlier tokens depending on later text.
The invariant was therefore split into the two properties that DO
hold (greenfield §7.6 updated):

- **prefix-determinism** (redefined, stays in `--suite all`):
  token-level — `scan_tokens(file[..k])` agrees with
  `scan_tokens(file)` for tokens strictly before the cut. Pure Rust,
  catches scanner statefulness bugs, GREEN on the standard sample.
- **prefix-conformance** (new, opt-in — needs the node oracle):
  our syntactic T0 diagnostics for `file[..k]` must equal the tsc
  oracle's getSyntacticDiagnostics on the same truncated program —
  diagnostic fidelity on truncated inputs, which is the strongest
  honest version of the original intent. GREEN on 200-fixture
  (299 cases) and 1000-fixture (1858 cases) samples at the M1 gate.
  Truncated .json files are excluded: tsc's module resolution injects
  package.json validation diagnostics (e.g. 1328 for a malformed
  typesVersions) into parseDiagnostics — unported program machinery.

The suite immediately earned its keep: the 1000-fixture run exposed
that fixtures redefining the same @filename produce programs where
tsc's name-keyed host map keeps only the LAST file while we checked
all of them; check_program now applies the same later-shadows-earlier
rule.

With this, every M1 final-gate line is green except the classified
1453 pragma residual above.
