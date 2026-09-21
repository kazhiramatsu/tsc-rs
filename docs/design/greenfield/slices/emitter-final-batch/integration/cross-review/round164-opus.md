No automated gate runs `inventory.py --check`. `.github/ci/test_replay.py:1296-1304` only imports the module and calls `command_rows`. The witness-coverage README still points at `inventory.v29.json` (line 5), while the generator defaults to v35.

# Round 164: retirement, remaining transpile rows, and final ordering

This is read-only (git show and source reads at frozen `4dfc0fb4d`). No commands run. **I found no hard blocker in source.** There is one ordering hazard to respect (§3), and two small corrections.

## 1. Retirement commit (`4dfc0fb4d`): complete, and the mutation guards are kept

- **`emitter_final_universe.rs:18,22`.** `KNOWN` and `KNOWN_PLAN_BASE` are now empty, and the live fixture `emitter-final-known-native.json` has `"cases": []`.
  - The membership check ("native observations and KNOWN IDs must have identical membership", ~99-103) therefore holds as ∅ = ∅.
  - With nothing known, every one of the 217 and 1,798 rows must now replay exact. This strengthens the test.
- **Refusal mutation guard.** `known_refusal_rejects_changed_error_or_partial_writes` now reads the tracked historical record `integration/records/parse-known-before-retirement-r122.json`. So the "changed error or partial writes" guard still runs against real prior refusals.
- **`transpile_routes_contract.rs`:**
  - Live known-open is exactly 2 rows, with the duplicate-ID assert.
  - `known_open_rejects_changed_output_refusal_and_panic` iterates the tracked historical `transpile-known-before-retirement-r161/known-native.v1.json`, all 8 prior observations, so the comparator's rejection logic is still exercised.
- **Witness selection.**
  - `scripts/emitter_final_witnesses.py:207-211` adds the r122 record to the universe inputs.
  - `scripts/witness.py:379-384` adds both r161 historical files to the transpile inputs.
  - Editing any guard input therefore selects the right suites.
- **Tracking.** All three historical files are tracked (`git ls-files --error-unmatch`).
- **Nothing weakened.** The six stale-known assertions were resolved by retiring the rows (r213), not by relaxing the assertions.

**Minor, not a blocker:** the emptied live fixture keeps its old `producer_diff_sha256`. Nothing I found validates it, but consider noting in the retirement README that it is historical.

## 2. The two remaining transpile rows: the finite dispositions still hold

**`transpile-js/text/unicode`:**
- **Input:** `const \u{1F600}x = …; export { \u{1F600}x as 😀 };` with target ES5.
- **TS output** (decoded from `expected.v1.json`): `var u, F600 = (void 0)[1], x = "\uD83D\uDE00"…`, then `{ 1; F600; }`, `x;` and `;`.
- **Why it isn't a bounded repair:** the invalid escape recovers into an identifier `u`, an array binding pattern `[1, F600]` lowered by ES5 destructuring, a block with two expression statements, and a stray empty statement. That is 9 diagnostics and 19 events across several recovery families (invalid character, binding-pattern elements, statement splits). None of the admitted context-profile rules covers them, and admitting them needs several new structural claimers.
- **Disposition:** keep it attributed to H2.9.

**`transpile-js/numeric-target/transform-100`:**
- **TS observation:** `"exception": "Debug Failure. Output generation failed"`, with internal `script_kind: "JSON"`.
  - `target: 100` is `ScriptTarget.JSON`, so TS parses the `.ts` text as JSON and asserts during emit.
- **Native:** a typed refusal, "unsupported emit compiler option: target".
- **Why not repair it:** matching would mean either reproducing an internal TS assertion (fabricated behaviour) or adding filename-independent JSON source-kind handling across Program and emitter, which is outside this batch.
- **Disposition:** keep `rust-unsupported`.

## 3. Pins, walk and inventory ordering

**What the inventory reads** (`witness-coverage/inventory.py`, the `READ` set and `source_commit`):
- every crate manifest, `Cargo.toml`, `Cargo.lock` and `crates/xtask/src/acceptance_plan.rs`;
- the Rust **test-target sources**, following each target's full `#[path]` closure;
- `.github/ci/replay.py`, `.github/ci/test_replay.py`, the witness scripts and the two workflows;
- the acceptance driver closure under `crates/xtask/src` (from `acceptance_slices.rs`).

Fixture *paths* are recorded, but their bytes aren't hashed. `source_commit` is `git rev-parse HEAD` at `--write` time, and `generator_sha256` hashes `inventory.py` itself.

**What the walk writes:**
- **Automatically, in `scripts/chain-walk.sh`:**
  - `ratchets/*.json` (oracle rungs);
  - `.github/ci/contracts/h2-5g-profile.schema.json` (`schema-const-repin.py --fix`, lines 103 and 428);
  - `ratchets/pins/harness-expected.v1.json` (`harness-pins.py --write`, lines 116 and 453);
  - `crates/oracle/*.mjs` pins (`chain-walk-repin.py`).

  **None of these is in `READ`.**
- **The recorded post-walk repair `pin-audit.py --fix`** *does* rewrite `AUDITED` Rust test sources, e.g. `crates/harness/tests/integration/h2_baseline.rs` and `transpile_suite_inventory.rs` (`pin-audit.py:38-44`). Those files **are** in `READ`.

  So a v36 minted before the walk can go stale.

**Recommended order** (no masking, and no overwrite: `--write` opens with `'x'`):
1. **Before the walk:**
   - set `inventory.py`'s default `--output` to `inventory.v36.json` (line 258). It must be first because `generator_sha256` is captured at mint time;
   - fix README line 5's link from v29 to v36;
   - make sure every `emitter-architecture.md` edit is already merged into the Repair tree: the 18 delegated rows, `E-RECOVERY-FACTS`, and the 58-entry disposition table. The H2.5h trio hashes that file, and a post-walk edit would make it stale.
2. **One canonical walk.** Apply any `pin-audit.py --fix` it reports, re-run the affected harness tests, then fmt and clippy. Commit, leaving a clean tree.
3. **On that clean commit,** run `inventory.py --write`. `source_commit` then names exactly the bytes it hashed. Commit the new `inventory.v36.json`. It's a docs-path JSON file and not an input to any gate or rung.
4. **That commit is V:** run the unsplit `cargo xtask ci` there, and `inventory.py --check` passes at V.

## 4. Operational sequence: no contradiction, with two conditions

The sequence is consistent with CLAUDE.md §3-4 and the r143 decision:
1. Walk and gate in Repair.
2. Fast-forward the Integration branch to the exact V.
3. Push and update PR 561, with the gate summary recorded at V in the body.
4. Hosted acceptance.
5. `gh pr merge --merge`.
6. D: a Markdown-only commit changing only `architecture-validation.md` for the 18 delegated rows.

**Conditions:**
- **(a) The architecture doc is final at the walk, not only at the gate** (see §3 step 1). The integration README remains unpinned (no oracle, ratchet or xtask reference), so the dated observations can still be updated before the walk as planned.
- **(b) D must check the merge commit.** D must confirm that the merge commit contains V and that every profile-bound input is byte-identical between V and the merge commit. If `main` moved with such changes, the affected rows stay `active-unqualified`.

**Scope of the record:** keep the qualified scope exactly as recorded. Nothing here supports claims about H2.9 as a whole, build/watch, public re-emit, or TS7 completion.
