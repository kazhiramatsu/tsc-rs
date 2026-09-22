# System variable type-comment controls

Twelve complete TypeScript 6.0.3 commands cover initialized and uninitialized exports, initialized locals, multiple declarations, destructuring names and function-local declarations, each with removeComments false and true. All include JS/source maps and declarations/declaration maps; all twelve have zero diagnostics and two identical oracle observations. Native qualification is pending.

The cases follow the concrete round116 Opus review of System parsed-name reuse, which also corrected its previous namespace call-chain inference. No new KNOWN is introduced and no production fix is claimed by this fixture commit. The existing r113 namespace, generator clone, parameter property and CommonJS cases remain separate controls.

Receipts: `../records/local/system-type-comment-oracle-r117.json` and `../records/local/system-type-comment-registration-r117.json` (seven registration tests pass). The oracle observer is `scripts/observe-emitter-r117-type-comment-controls.mjs`, and the fixture is `crates/compiler/tests/fixtures/emitter-r117-type-comment-controls.json`, both relative to the repository root.

A candidate implementation adds a guarded erased-type trailing phase at the two ordinary printer comment exits. It requires an original identifier whose parse parent is a variable declaration and whose metadata retains an erased type. It reuses the name's flags and saved parent container, and preserves the name's own trailing token anchor. Existing declaration/property-access explicit phases are unchanged. Opus round117 found no blocker. This remains a candidate: the preceding r116 measured run must confirm the missing System comment, then the candidate must pass native complete-command and emitter regressions.
