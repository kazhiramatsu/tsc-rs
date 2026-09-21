The proposal is correct and sufficient. No Rust test pins the 5g runtime-input count. The `323`s in `h2_7de_acceptance.rs:122,269` are unrelated case counts, and the readiness envelopes list the schema path without a hash.

# Round 165: H2.5g runtime-input closure proposal

This is read-only: the proposal files, a fresh read-only `git diff` from `TRUSTED_BASE`, and source reads at `4dfc0fb4d`. **Verdict: the two-file proposal is correct and sufficient. There is no hard blocker.** One precondition, a clean tree before the walk, is below.

## 1. The arithmetic, recomputed independently

The closure is the rule in `h2-5g-profile.mjs:488-519`:
- `git diff --name-only --diff-filter=ACMRTUXB 11f5d0ab… -- crates`, plus untracked files under `crates`;
- minus `crates/oracle/*` and the `NON_RUNTIME_SHADOW_INPUTS` set.

It fails closed on **missing** paths and on **stale** `NEW` entries: an entry already in the parent, or no longer in the changed set (741-763).

**Current tree** (my recomputation matches the census):

| Quantity | Value |
|---|---|
| parent (H2.5f) | 86 |
| `NEW_RUNTIME_INPUTS` | 237 |
| union | 323 |
| changed closure | 916 |
| missing | **599** |
| stale | exactly `crates/emitter/tests/unit/tagged_template/tests.rs` |

**The proposal:**
- `added_paths` is exactly the recomputed missing set (599, set-equal).
- The removed path is the one stale entry.
- Result: 86 + 236 + 599 = **921** unique paths, with no duplicate in `NEW`, no overlap between parent and `NEW`, and 0 missing and 0 stale afterwards.
- The five parent inputs outside the changed set are allowed; the rule doesn't require parent paths to have changed.
- The only changes outside the path list are the two `921` pins, generator `size === 921` and schema `minItems`/`maxItems`, plus the explanatory comment. `NON_RUNTIME_SHADOW_INPUTS` is byte-identical.
- The one remaining `323` in the proposed generator is inside an unrelated hash (line 82).

## 2. Including the 535 test paths conservatively: correct, no contract violation

- The existing rule already treats **every** changed crate path as a runtime input unless explicitly shadowed. The prior `NEW` list and parent already contain test paths, for example `crates/harness/tests/integration/*` and `support/pins.rs`.
- Adding the missing test paths applies the existing policy. Adding new exclusions would be a policy change.
- The 599 are all `.rs`, `.json`, `.zst` or `Cargo.toml`. There are no AppleDouble `._*` files: the one committed in `d364a056a` is no longer tracked.
- **Consequence to accept:** any byte change to one of the 921 paths after the mint makes the 5g profile stale. That already holds for the old list, and the pin-audit-rewritable `AUDITED` harness tests were already inputs. So the "final source, then one walk, then no crate edits" order is required, and it is what you planned.

## 3. The deleted path: no lineage violation

- `crates/emitter/tests/unit/tagged_template/tests.rs` was deleted in `d364a056a` ("fix: preserve UTF-16 values across the compiler pipeline", 2026-09-14). It is absent at HEAD.
- The rule's `--diff-filter` excludes deletions, so the entry is stale by construction, and `pathHash` couldn't read it anyway.
- **What stays intact:**
  - the immutable H2.5f parent (86 paths in `ratchets/h2-5f-profile.v1.json`) is untouched;
  - `ratchets/h2-5g-profile.v1.json` is current state that the walk re-mints;
  - other mentions of the path (the `h2-8a-*` design and experiment records, `h2-8b-config-diagnostics-final`) are historical snapshots with no validator on current bytes.
- Dropping it rather than inventing a replacement is correct.

## 4. Downstream obligations, and who handles each

| Obligation | Handled by |
|---|---|
| The 5g profile artifact bytes (generator hash, contract hash, 921 `runtime_inputs`) | the canonical walk (re-mint rung) |
| Downstream rungs and scripts pinning the 5g profile or generator hash | the walk's repin (`chain-walk-repin.py`) |
| The harness manifest (`ratchets/pins/harness-expected.v1.json`) | `harness-pins.py --write` at the walk tail |
| The `h2-5g-profile.schema.json` consts | `schema-const-repin.py` repins only the enumerated five consts, so `minItems`/`maxItems` rightly stay manual in the proposal |
| The `.github/ci` tree digest (`qualification.mjs` `qualification_profile_sha256`, 2090-2097) and policy/fuzz source hashes | your planned refresh of the 23 r226 hashes, **after** these two files are applied |
| Readiness envelopes (`ratchets/fci-readiness/h2-5h-*.v1.json`) | Nothing needed: they list the schema *path* without a hash, and the checker digests only packet `.md` files |
| Rust harness tests for earlier profiles (`h2_1a…h2_3c_profile.rs`) | Nothing needed: they read *their own* profiles' `runtime_inputs` |

No Rust test or xtask code pins the 5g runtime-input count or its list. `xtask/src/main.rs:8999/9019` only name the generator path.

**Re-observation:** only the profile rung changes; `h2-5g-qualification` precedes it in ORDER. The walk's strict "zero 5g re-observation on pin-only cascades" check therefore applies. Confirm with `WALK_DRY=1` that the plan shows a pin-only cascade.

## 5. Precondition: clean tree before the walk

- The closure also includes **untracked** files under `crates/` (`ls-files --others --exclude-standard`). Any stray scratch or AppleDouble file there at walk time would either fail the closure or get hashed as a runtime input.
- **Require** `git status --porcelain -- crates` to be empty before the walk (and before `inventory.py --write`).

## Order (unchanged from r164, now including these files)

1. Source tests pass.
2. Apply the reviewed two files.
3. Refresh the r226 policy/fuzz source hashes.
4. Make the architecture doc final, and point the inventory default and README at v36.
5. `WALK_DRY=1` (expect a pin-only 5g cascade).
6. The one canonical walk, plus any post-walk `pin-audit --fix`.
7. Commit with a clean tree.
8. `inventory.py --write` (v36).
9. Commit V.
10. Unsplit `cargo xtask ci` at V.

Nothing here is merge or qualification evidence.
