# Round 141: finalization sequencing for the architecture doc

Promoting the 14 rows to `active-qualified` in this delivery would break "one canonical walk". Any promotion edit to the doc after the walk makes the pinned artifacts stale and needs a second walk. Promotion before the walk is premature under the lifecycle rules. The honest sequence keeps all 14 rows `active-unqualified` through the walk and records the final validation ref in the integration README, which nothing pins.

## What the source shows

### 1. The lifecycle rules

`emitter-architecture.md` §1, lines 67-115:

- **`active-qualified` requirement:** an "exact profile frozen at an immutable final validation ref" (line 71).
- **Validation ref:** a clean commit where "all runtime gates" passed (lines 83-90).
- **Promotion:** happens only in "a following documentation-only commit" that cites that ref (lines 88-90).
- **Transition gate:** `active-unqualified -> active-qualified` requires that freeze (lines 113-114).

So the complete-command and native evidence finished before the walk is admissible as scoped, candidate evidence recorded in the row text. It does not meet the qualification requirement, because the walk and full CI are still pending. Before the walk, the rows must stay `active-unqualified`.

### 2. What the generators consume: row IDs for validation, the whole file for the pin

| Generator | Reads for validation | Pins |
|---|---|---|
| `h2-5h-a-owner-graph.mjs` | backticked row IDs (245-260) | `lineage.architecture_map = pathHash(doc)` (895) |
| `h2-5h-a-gap-matrix.mjs` | backticked row IDs (485-503) | the same whole-doc hash (565) |
| `h2-5h-a-dispositions.mjs` | row IDs via regex over table rows and `EA-GAP` headings (122-139); none of the lifecycle or status text | the same whole-doc hash (216) |

- `pathHash` is sha256 of the raw file bytes (for example `owner-graph.mjs:462`). So semantically only IDs are consumed, but **any byte change, including a lifecycle edit, makes all three stale**.
- That staleness spreads downstream: `h2-5h-a-es2015-generators-witnesses.mjs:18,1372-1388` checks the owner graph. All of these are in `chain-walk.sh` ORDER (lines 167-170 onward).
- The dispositions table already contains `E-ENTRY`, `E-MAPS` and `E-OUTPUT-FUTURE` (lines 38, 43, 44), so the row inventory matches. Only the hash is affected.

### 3. The trio is already stale now

- All three artifacts record architecture hash `44b2c9ff…`, last minted in the 2026-09-06 walk (`879fba6d1`).
- The current doc is `fdcec92a…`.
- Several doc-only commits since then (`07b7425e9`, `e5a80313a`, `599798019`) changed it without a re-mint, because no walk ran in the meantime.
- The coming walk must re-mint them in any case, against whatever doc bytes exist when it starts.

### 4. Things that are not affected

- The integration records (`slices/emitter-final-batch/integration/README.md`, `residual-audit.md`) are not pinned by any oracle, ratchet or xtask code.
- The only integration-directory pin I found is `recovery-inputs-r20.json`, in `scripts/foundation_witnesses.py:22`.
- The `check-*-readiness.py` scripts do read the doc, but only check row IDs (for example `check-class-order-readiness.py:26-29`). Nothing in xtask or the walk shell scripts calls them, so they don't gate this.

## A narrow, honest sequence

1. **Before the walk, write the final doc text.** All 14 rows stay `active-unqualified`.
   - Each row's evidence cell may cite the immutable pre-walk evidence: the 508 complete commands exact twice, 145 comma, 7 neighbour, emitter 1015, transpile 291, selected44 and project108 once they're done.
   - Label it explicitly as scoped candidate evidence with "canonical walk, unsplit CI and hosted pending".
   - In the same edit, add a pointer from §1 or the row text: "validation ref and qualification decision: [integration README §…]". The README is unpinned.
   - Also finalize the dependency-boundaries note and archive `architecture-before.v1.json` before the walk.
   - Every one of these doc bytes must be final before the walk, because the walk mints the trio from them.
2. **Run the retirements first:** KNOWN 36+6 retirement after the full proof, historical guards kept, integration, fmt/clippy, witness inventory and source pins. Then run a `WALK_DRY=1` preflight. It should list the trio and the es2015-generators cone as stale; if it doesn't, stop before walking.
3. **Run one canonical walk, then unsplit `cargo xtask ci` on the resulting clean commit V.** V, with the walk outputs committed, is the final validation ref.
4. **After V, change only unpinned records.** Record V, the CI result and the hosted result in the integration README. Do **not** edit `emitter-architecture.md`.
   - The rows then read accurately: `active-unqualified` in the map, with the linked record showing that V passed.
   - Nothing is qualified prematurely and nothing goes stale.
5. **The lifecycle promotion to `active-qualified` becomes a separate later step.** It is a doc-only commit that cites V, plus the pin-only re-mint of the trio's cone that its byte change forces. That re-mint needs its own walk, so it belongs to the next slice, not this delivery. Record it there as a named follow-up.

The rule in §1 lines 88-90 ("a following documentation-only commit may … promote") conflicts in practice with the whole-file pins. The sequence above respects both without changing either. Promoting and re-walking in this delivery would be the only other compliant option, and it costs a second walk.

## Unknowns

- **Whether `cargo xtask ci` runs `--check` on the trio directly or only through the walk.** Either way the trio is stale now, and both the walk and CI need the doc to be final first. The `WALK_DRY=1` preflight in step 2 answers it.
- **Whether any other pinned file cites the architecture doc's row text by content** (for example the h2-7a owner inventory). My search was bounded to `crates/oracle`, `ratchets` and `scripts`, and found only whole-file hashes and row-ID checks.
- **The noEmit supplement count (4357) and its rerun are noted.** As you said, no action is needed here.
