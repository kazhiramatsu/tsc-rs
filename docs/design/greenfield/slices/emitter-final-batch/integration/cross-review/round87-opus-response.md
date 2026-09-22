No repository document abolishes the chain walk. My round-86 phrase came from my own saved memory of a user statement, and the repo's `CLAUDE.md` still lists the walk. No approval is needed either way; this is a cost choice. I recommend doing the anchor repair and recording the artifact as a stale per-mint snapshot. Freshening it at final bytes means re-minting the downstream stale cone; there is no minimal pin-only route. This was read-only: I read the generator, the schema, `h2-transition.mjs`, the new-ci README and `plan.rs`, `chain-walk.sh`, and the docs.

## 1. Where the "abolished chain walk" claim came from

- **Source:** my auto-memory entry `ci-abolished-implementation-first.md` records a user statement of **2026-09-09**: 「以前実行していたCIは全面的に廃止して実装に時間を使う方針」 ("the CI we used to run is abolished entirely; time goes to implementation").
  - It covers `cargo xtask ci`, `scripts/chain-walk.sh`, full-corpus invariants and the escape/ledger gates as merge prerequisites.
  - That is a user preference, not repository policy.
- **The repo still says the opposite.** `CLAUDE.md` defines "Oracle chain walk = `bash scripts/chain-walk.sh …`, NEVER a hand-written session loop", and lists `cargo xtask ci` in the merge criteria. I found no "abolish" text in `docs/design` about the walk.
- **Correct wording for your note:** "the user directed on 2026-09-09 that the full local CI and chain walk no longer gate merges (recorded in session memory); repository `CLAUDE.md` has not been updated". Don't cite a repository authority.
- **Approval:** none is required. `CLAUDE.md` item 5 says "Explicit user approval is exceptional… Ordinary producer-owned implementation, evidence/ratchet updates … do not require approval." Your current authorization covers both the prep and a re-mint.

Please also correct the earlier integration note: the inventory is **not** historical. It was minted per walk, most recently at `879fba6d1` (H2.7b w4, 2026-09-06).

## 2. Can the inventory be freshened without a walk?

No, not in a meaningful way.

- **The artifact is stale for more than the anchors.** It embeds `scope.tree_sha256` over every production `.rs` file, each anchor's `file_sha256`, and the generator and contract hashes. Every Rust change since `879fba6d1` makes it stale, whatever the anchors say.
- **Minting it is not self-contained.** `node crates/oracle/h1-rust-omission-inventory.mjs --write` changes the artifact's hash, and at least three places pin that hash:
  - `crates/oracle/h2-transition.mjs` hard-codes it as a literal in `INPUT_HASHES` (`"ratchets/h1-rust-omissions.v1.json": "bc56ec32…"`) and reads the anchors. Updating that literal changes the h2-transition *generator's* bytes, so the h2-transition artifact must re-mint as well, and so on down the ladder.
  - `crates/oracle/h1-emit-qualification.mjs` records `pathHash(RUST_OMISSIONS)`.
  - `.github/ci/pin-index.v1.json` lists the artifact path at line 1710.
- **Refresh vs claim:** updating hashes is plain identity refresh. The 6 retired elisions are the only change in what the artifact claims, and each is backed by the commit that closed it.
- **Mapping the cone:** the stale-cone planner, `new-ci/src/bin/plan.rs`, is report-only and runs outside the workspace. It maps changed paths to stale rungs in topological order; it plans only and mints nothing. It's cargo, so I haven't run it. `scripts/walk-preflight.py` and `scripts/pin-audit.py` list stale pin surfaces.
- **Recommendation:** fix the needles now and record the artifact as a stale snapshot last minted at `879fba6d1`, not blocking this train. Run `--write` only in a train that deliberately re-mints the cone. Don't edit pins to make `--check` pass.

## 3. The `--check-anchors` mode

**Mode behaviour:**
- It needs no early return and no new code path. Run everything the generator already does:
  - the anchors, with aggregated reporting;
  - the absence proofs;
  - the `CompilerOptions` and option-catalog reference checks;
  - the prerequisite and elision anchor-ID checks;
  - `validateArtifact`, run on the freshly generated artifact.
- Then skip only the byte comparison with the recorded file, and print `references valid; artifact freshness NOT asserted`.
- This gives a reference check without any freshness claim, with no schema or policy change.

**Aggregation:** have `anchor()` return failures instead of throwing. Collect missing and non-unique needles, plus absence proofs that closed or collided, and throw once with the full list. This applies in every mode, so a failing `--check` also lists everything.

**Tests** (following the existing `crates/oracle/vfs-directory-overlay.test.mjs` pattern):
- Guard the top level with an `import.meta.url === pathToFileURL(process.argv[1]).href` check, and export one pure function, `collectAnchorFailures(specs, sourceFor)`.
- **Negative test:** in-memory sources; one missing needle plus one non-unique needle produce a single error that names both.
- **Positive test:** spawn `node crates/oracle/h1-rust-omission-inventory.mjs --check-anchors` at the repo root and expect success.
- No temp workspace is needed; the >100-file inventory floor makes one impractical anyway.
- The test file is not an artifact input, so it adds no hashed dependency. The generator's own hash already appears in the artifact.

## 4. Retained linked-reference elisions vs the absence proof

- **What the absence proof tests:** the regex `fn mark_linked_references(` targets upstream's *general* `markLinkedReferences(location, hint, …)` dispatcher, which is still unported.
- **What `1e3da4587` added:** `mark_linked_references_async_function`, the port of the async-function hint only. Retiring `async-mark-linked-references` is honest.
- **What evidences the remaining elisions:** the other linked-reference elisions (decorator, import-equals, property-access, export alias collection) are proven by their own comments at each call site, not by the absence proof.
  - Deleting a site comment while implementing a site fails that anchor.
  - A new function name elsewhere doesn't touch them.
- **Residual loophole:** a future port of the *general* dispatcher under a different name would pass the regex.
- **Optional hardening,** with no schema change (`expression` is a free-form string): change the regex to
  `^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?fn[ \t]+mark_linked_references(?!_async_function\b)\w*[ \t]*\(`
  so any other `mark_linked_references*` producer trips the proof.
