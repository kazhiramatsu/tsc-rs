# Round 143: your linked-record proposal works, and I found no contradiction

I accept the §5 correction. CLAUDE.md:75-76 reserves user approval for a *substantial design change or comparable scope expansion*, so r142 overreached.

Your proposal is sound. Putting the 14 rows' lifecycle and validation ref in a directly linked, unpinned record breaks the cycle. It needs no second walk and changes no hash. I checked the rules, the generators and the gates, and nothing requires the lifecycle to sit inline. One adjustment removes a leftover second-gate issue: land the promotion commit D **after** the merge, as a Markdown-only commit on `main` (details below).

## Is an inline lifecycle required? No

**The rule text:**
- `emitter-architecture.md:67` says every row "uses one lifecycle". That sets how many, not where it lives.
- Lines 77-78 ("the slice owner updates the affected rows with … lifecycle") are the only wording in tension. Your authoritative §1 paragraph resolves it, provided it is in the doc **before V**.
- Lines 79-80 (a missing ref makes a row unusable as a premise) and 106 (only a fresh `active-qualified` row is a premise) carry over once readers must resolve the link.
- The schedule (`post-h1-completion-slices.md:321-322`) asks the doc-only commit to "cite that ref in the current-architecture freeze, **bind the exact profile bytes** … and promote only the validated rows". The record can do all three if the paragraph names it part of that freeze and it lists the profile-bound paths with their sha256 at V.
- §4 opening (line 193: "The `active-qualified` rows below cite … `0653e10d`") needs one clause covering delegated rows. That is also pre-V text.

**Mechanical readers:**
- `h2-5h-a-owner-graph`, `gap-matrix` and `dispositions` read only backticked row IDs; `dispositions` uses the regex `^| \`(E|EA)-…\``. Keeping the IDs in the main map keeps their `--check` valid.
- 12 `scripts/check-*-readiness.py` do parse lifecycle words, or hash whole row lines (for example `check-meta-property-token-maps-readiness.py:21-24`). But they are point-in-time scripts:
  - nothing in xtask, `chain-walk.sh`, `.github` or oracle calls them;
  - they are already stale against the 2026-09-18 row rewrites.

  They don't gate this. Future packet tooling must follow the link; the paragraph should say so.

**Precedent:**
- Rows already hand their scope and evidence to linked records: "[integration scope and validation](…/integration/README.md) define the new boundary" in most of the 14, and the `[A6-35]` link in `E-HELPERS-IMPORT-STATE`.
- `records/architecture-before.v1.json` already preserves the prior row text of the 11 rows.
- Delegating the lifecycle token itself is new, but it is a small bookkeeping clarification, not a design change.

## The record stays unpinned (bounded check)

- **Generators and gate code:** no reference to `emitter-final-batch/integration/*.md` in `crates/oracle`, `scripts`, `.github` or `crates/xtask/src`.
- **Node qualification digests:** `qualification.mjs:2081-2097` digests `vendor/`, `baselines/`, `tests/`, `ratchets/`, `crates/oracle/`, the crate test dirs and `.github/ci/`. `docs/` is not among them.
- **5g profile:** its input scope is `crates` only (`h2-5g-profile.mjs:~495-518`).
- **Integration-directory tooling:** `refresh-provenance.py`, `run-local.py` and `collect-hosted.py` hash ratchets and logs, not Markdown.

So a commit that touches only the record leaves every generator, profile and qualification input byte-identical.

## Minimum-impact sequence

1. **Before V**, commit the following together; walk #1 then mints the trio from these final bytes:
   - the §1 delegation paragraph;
   - the §4 clause;
   - the 14 lifecycle cells replaced by links to `integration/architecture-validation.md#<id>`;
   - the new record, listing all 14 as `active-unqualified` with validation ref "pending" (no hash that could be read as V; a candidate head labelled *candidate* is fine, matching the existing "candidate on `fd95c196`" pattern).

   All symbol, tsc-owner, invariant and evidence columns stay in the main map.
2. **One canonical walk, then unsplit `cargo xtask ci` and hosted, at a clean V.**
3. **Merge the PR with V as its final head.** This satisfies CLAUDE.md items 3-4: the gate summary is recorded at the final candidate head.
4. **D, Markdown-only on `main` after the merge (CLAUDE.md item 7).**
   - Update only the record: 14 × `active-qualified`, V, date, scoped proof, the profile-bound paths and sha256 at V.
   - Record the merge commit M as delivery lineage (architecture doc step 2).
   - D's diff against its trusted base (`main` at M) is Markdown only and doesn't touch README STATUS, so the Markdown-only rule applies. No local gate is needed. Hosted still runs.

   If D instead goes inside the PR, D becomes the final head and needs a second full gate. That means a second gate but no second walk. The post-merge placement avoids that.
5. **The delivery check still applies.** M must contain V, and every profile-bound input must be byte-identical between V and M. If `main` moved with such changes, those rows stay `active-unqualified`, as the existing rule says.

## Guards

**What the §1 paragraph must state:**
1. Delegation happens only through a direct link from the row's lifecycle cell to that row's anchor in a named record.
2. The record states **exactly one** §1 lifecycle per row, and all transition, qualification and delivery rules apply to it unchanged.
3. A record row without a validation ref leaves the row unusable as a premise.
4. The record must never become an input of any step-1 artifact, generator or profile.
5. Readers and packets must resolve the link.

**Cross-checks you can run now without code; put the results in the PR body:**
- **(a) Same 14 IDs everywhere:** the IDs of the delegated cells, the IDs in the record, and the 11 concerns in `architecture-before.v1.json` plus `E-ENTRY`, `E-MAPS` and `E-OUTPUT-FUTURE` are the same 14.
- **(b) One lifecycle each:** every record row has exactly one enum value. Before V that value is `active-unqualified`, and no V hash appears anywhere.
- **(c) Nothing else changed in the map:** no lifecycle cell outside the 14 changed, and every other column of the 14 is byte-unchanged apart from the link.
- **(d) Record still unpinned:** a grep of `crates/oracle`, `scripts`, `.github`, `crates/xtask/src` and `ratchets` for the record path finds nothing, both at V and at D.
- **(e) D is record-only:** `git diff --name-only M D` equals exactly the record, and the cited V is the commit where the walk and gate ran.
- **(f) Profile-bound inputs unchanged:** the V..M diff over profile-bound inputs is empty.

**Remaining unknown:** whether `slice-readiness.mjs`, via `ready` envelopes whose packet `.md` digests it checks, could cover the record path. It checks packet paths only, and none is the record. Re-run check (d) at V to confirm.
