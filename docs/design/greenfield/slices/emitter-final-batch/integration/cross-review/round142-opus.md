# Round 142: re-evaluated finalization sequence

I accept both corrections: row updates at slice close are mandatory, and "one invocation per converge" does not forbid a second converge. So all 14 rows get updated in this delivery; my r141 deferral is withdrawn.

With that constraint, a **second canonical walk and a second full gate after the doc freeze are unavoidable**. That part is sound. What the current rules can't settle on their own is which validation ref the rows then cite. I explain why below. That needs an explicit decision from you or the owner; I haven't assumed one.

## Confirmed mechanism (source-based)

**1. What a doc-only edit makes stale.**
- Directly: `h2-5h-a-owner-graph`, `h2-5h-a-gap-matrix` and `h2-5h-a-dispositions`. Each pins `pathHash(doc)`, raw sha256, and its `--check` compares the whole rendered artifact.
- Transitively: one more, `h2-5h-a-es2015-generators-witnesses`, which pins the owner-graph hash.
- Nothing else in `crates/oracle` pins these four, and they sit consecutively in ORDER (`chain-walk.sh:167-172`).
- Things I checked that pin none of them:
  - Rust source: the three `builtins/*.rs` mentions are doc comments only.
  - `ratchets/pins/harness-expected.v1.json`.
  - Schema constants: the four schemas carry no hash consts.
  - `qualification.mjs`: it checks their schemas only (`validateArtifactSchemaContracts`, 895).

**2. The unsplit gate cannot detect this staleness.**
- `xtask ci` runs the walk only in dry mode (`main.rs:8405-8410`), which compares crate-tree hashes only (`chain-walk.sh:320-332`).
- None of the 79 explicit oracle `--check` calls in `main.rs` names h2-5h-a.
- So a doc-promotion commit would pass `cargo xtask ci` while leaving the ladder stale. Only a real walk catches it, which is why the second walk is required rather than optional.

**3. The approved automation handles it without a manual loop.**
- `scripts/chain-walk.sh`, per stale rung: `chain-walk-repin.py`, then `--write` (386-417).
- 5g enforcement stays strict with zero re-observation. 5g comes before h2-5h-a in ORDER and is unaffected.
- The witnesses rung on `--write` reuses the stored TypeScript observations when its generator bytes and TS record are unchanged (`reusableStoredObservations`, 905-925). Its lineage and fingerprints are re-derived.
- The witnesses rung's `--check` always re-observes, so the walk still verifies it fresh.
- Expected cascade: those 4 rungs, zero new oracle runs on write.

**4. Side effects.**
- 56 h2-8a record files (118 pins) snapshot the *current* witnesses hash in `prelaunch` inputs.
- I found no validator for them in oracle, scripts, xtask, tests or `.github/ci`. `slice-readiness.mjs` digests only the fci-readiness envelope packet paths.
- A re-mint will leave them historical, exactly like the 286 doc pins that already are.

**5. The trio is already stale today.**
- It pins the doc as of `0382bb3a7` (2026-08-23).
- The doc has been edited since 2026-09-07 with no walk.
- Walk #1 has to re-mint the trio regardless.

## Why no finite sequence closes under the literal rules

- **What step 1 contains.** It explicitly includes "qualification/**owner**/profile artifacts, schemas, and generators" (`post-h1-completion-slices.md:317-320`). The owner graph, gap matrix, dispositions and witnesses are step-1 artifacts.
- **What a later change triggers.** "Any change to runtime or evidence inputs after step 1 requires a new final validation ref" (schedule 328-330; architecture doc 100-102).
- **The loop:**
  1. The step-2 doc commit necessarily changes the doc bytes.
  2. Those four artifacts hash the doc, so walk #2 must change them.
  3. Read literally, that makes walk #2's commit a new validation ref, V2.
  4. The doc must then cite V2, which changes the doc again, which makes the trio stale again. The cycle never ends.
- **No written exclusion.** The rules contain no exclusion for lineage fields derived from the step-2 doc. The only "profile-bound" statements are at architecture doc 93 and schedule 327 and 361, and none of them defines one.
- **What that means.** For rows whose step-1 artifacts hash the step-2 doc, the literal wording cannot close. This comes from the rules themselves, not from a sequencing choice. Deciding it is a process-rule interpretation, which CLAUDE.md §5 reserves for the user, so I haven't treated it as settled.

## Honest sequence (every step required)

1. **Before V:** make every doc byte final except the 14 rows' validation ref and lifecycle, which stay `active-unqualified` with their scoped evidence. Run the KNOWN 36+6 retirement after the full proof, integration, fmt/clippy and the inventory/pins work. Then `WALK_DRY=1`.
2. **Walk #1, then unsplit CI and hosted at a clean commit V.** V is the validation ref.
3. **D1, documentation only:** update all 14 rows per lines 77-80 (V, date, symbols, tsc owners, tests, `active-qualified`), and record the post-merge lineage plan.
4. **Walk #2 (one canonical invocation).** Before starting, a `WALK_DRY=1` plan must predict **exactly** the 4 rungs above. Any other stale rung means a real input changed, and you stop. The walk must end with 5g re-observation at zero and the witnesses rung adopting its stored observations. Commit the result as D2.
5. **Full unsplit `cargo xtask ci` plus hosted at D2.** This is needed in any case, because the merge head must pass the complete gate.
6. **Record a mechanical proof** that V..D2 changes only:
   - Markdown files;
   - the four ratchets, where every changed JSON path is a doc or owner-graph lineage pin or a fingerprint over those pins (verify with a masked JSON diff);
   - the witnesses observations are byte-identical to V's.

   Everything else (Rust, tests, other evidence, profiles) must be byte-identical.
7. **Decision point:**
   - **(i)** Ratify that doc-derived lineage fields are not step-1 "evidence changes". The rows then cite V, D2 is delivery-side lineage, and post-merge verification compares V's profile-bound inputs with those fields excluded, as proven in step 6.
   - **(ii)** Keep the literal reading. Then the rows cannot close under the current rules, and the rules need an owner fix.
   - Leaving the rows unqualified, or skipping walk #2, is not acceptable under lines 77-80 or green-main.

## Unknowns

- Whether walk #2's tail refreshes anything beyond the 4 rungs, such as the pin-index or harness manifest. The step-4 dry plan and `walk-preflight.py` report this before any write.
- Whether the witnesses adoption actually covers every case (`adopted_cases` equal to all). If it doesn't, step 6 fails and (i) cannot be claimed.
- Whether any packet checker outside the paths I searched re-verifies the 56 prelaunch snapshots.
