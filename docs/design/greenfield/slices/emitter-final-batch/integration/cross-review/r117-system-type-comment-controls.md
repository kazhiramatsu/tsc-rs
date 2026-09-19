# System variable type-comment controls

Twelve complete TypeScript 6.0.3 commands cover initialized and uninitialized exports, initialized locals, multiple declarations, destructuring names and function-local declarations, each with removeComments false and true. All include JS/source maps and declarations/declaration maps; all twelve have zero diagnostics and two identical oracle observations. Native qualification is pending.

The cases follow the concrete round116 Opus review of System parsed-name reuse, which also corrected its previous namespace call-chain inference. No new KNOWN is introduced and no production fix is claimed by this fixture commit. The existing r113 namespace, generator clone, parameter property and CommonJS cases remain separate controls.

Receipts: `../records/local/system-type-comment-oracle-r117.json` and `../records/local/system-type-comment-registration-r117.json` (seven registration tests pass). The oracle observer is `scripts/observe-emitter-r117-type-comment-controls.mjs`, and the fixture is `crates/compiler/tests/fixtures/emitter-r117-type-comment-controls.json`, both relative to the repository root.
