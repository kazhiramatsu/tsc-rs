# h2-7a-ca — Phase-E/P register (values recorded as minted; `__X__` = TO-FILL at the named step)

## Design gate
- Packet rev 2 RATIFIED 2026-09-03 (sol r1 REVISE 7+4 → r2 AGREE; F1 withdrawn by mechanical count 69/69). Authority file: `docs/design/greenfield/slices/h2-7a-ca.md`.
- Trusted base for the train: the PR #500 merge sha `424ff3e1` (main).

## Frozen baselines the close generator carries (measured 2026-09-03 at 96a33a6e; byte-identical to the H2.6c close 5b4c626a)
| baseline | value | classification |
| --- | --- | --- |
| `crates/emitter/src/plan.rs` sha256 | ac36b2be2a480b0fce8b25a3983a9ec4e0c63661f138b45fcf31db1124c4fa3e | pin-index `semantic` (grammar D) |
| `crates/emitter/src/execute.rs` sha256 | e4b1e42504e5af8fc49433b4e2bf2ee3255f6e40e264e38e7743f01170f7a1ae | pin-index `semantic` (grammar D) |
| `crates/emitter/src/activity.rs` sha256 | 3542317425e47a3fa90d657d419008feb9eb32864144c7356c00b54fd35a09b3 | pin-index `semantic` (grammar D) |
| `h2-candidate-dispositions` `cases` roll | ed0036eb9d22227c3fba7980d852509a6aba42566c9a39329c761d1b4c61a79b | pin-index `unmatched` (frozen literal) |

## Eligible-domain denominators (re-derived from the probe artifact; schema consts)
116 eligible cases (120 − `F6/references-first`, `S2/entityname-1`, `S3/typeofexpr-1` (outFile → H2.7d) − `S2/latebound-1` (isolatedDeclarations → H2.7c)); transformSeed 202; declBlocked 202; transformTopLevelDeclaration.changed 742; visitDeclarationSubtree.changed 496; trackSymbol 533; reportInferenceFallback 362; reportInaccessibleUniqueSymbolError 1; reportLikelyUnsafeImportRequiredError 1.

## Candidate band (from `ratchets/h2-candidate-dispositions.v1.json`, 15,642 rows)
H2.7a first-blocker 0 / chain 0. H2.7b forecast (count-only, recorded not const): first-blocker 1 / chain 2,456.

## Close artifact — first mint (the walk)
- generator sha256: `__GEN__`; contract sha256: `__CONTRACT__`; `close_fingerprint_sha256`: `__FP__`
- `--check` exit 0 twice: `__YES__`; `--selftest` (lane) output recorded in `target/session-notes/ca/lanes/STATUS.md`

## Registration surfaces (same commit)
chain-walk.sh ORDER 69 → 70; plan.rs LADDER_ORDER 70; qualification.mjs + qualification.test.mjs 18 → 19 pairs; pin-index consumer entry (3 semantic + 1 unmatched); contracts.rs `mod h2_7a_ca_controls`; h2-5g-profile.mjs NON_RUNTIME_SHADOW_INPUTS +1 (runtime count stays 241).

## Landing (separate LAST commit)
h2-5g-profile.mjs:634-636 (`H2.7b` / `non-bundle-declaration-output` / `H2.7b`) + comment; h2-5g-profile.schema.json:228-230; h2-5h-a-foundation.mjs:1296-1304 + comment. Commit: `__LANDING__`.

## Walk / gate / PR
- walk cert `__WALK__` (rounds: `__ROUNDS__`; 5g observations 0: `__YES__`); final-head artifact fingerprints: owner-inventory `__`, witnesses `__`, probe-traces `__`, printer-reprint `__`, close `__`
- `cargo xtask ci --baseline 424ff3e1`: `__GATE_LINE__`; hosted `gates`: `__HOSTED__`; PR `__CA_PR__` merged `__CA_MERGE__` → H2.7a CLOSED.
