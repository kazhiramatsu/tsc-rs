# new-ci overnight mission (2026-08-26 night): pin-index, prospective
# concordance, substrate hardening

Same hard boundaries as SPEC.md: work ONLY inside `new-ci/`; the
repository is read-only input data; never run cargo against the root
workspace; no network (keep the zero-dependency constraint). Commit
your work with `git add new-ci && git commit` AFTER EACH milestone
(the worktree branch is yours); if you must stop early, commit what is
done plus `new-ci/STATUS.md` describing exactly what remains.

Execute milestones IN ORDER — each is independently valuable.

## M1 — six-family typed pin-index generator (highest value)

`cargo run --bin pin-index` emits `new-ci/pin-index.json` (generated
output, never committed — it is in .gitignore territory; add the
filename to new-ci/.gitignore) enumerating EVERY pin literal in the
repository as {family, producer, consumer_file, pinned_path, role,
hash, byte_span}. The six families, discovered the hard way this week:

1. Oracle script pin constants — the five grammars of
   `scripts/chain-walk-repin.py` over `crates/oracle/*.mjs` (you
   already extract these in shadow.rs; reuse).
2. Harness integration-test consts — `crates/harness/tests/**/*.rs`
   hash literals pinning `ratchets/*.json` artifacts; the curated list
   in `scripts/pin-audit.py` is the seed (read it), but SCAN, don't
   trust the list alone.
3. Hosted policy source pins —
   `.github/ci/qualification-policy.v2.json` rust_source_sha256
   entries (16 files).
4. Schema contract consts — `.github/ci/contracts/*.schema.json`
   `const` values that embed artifact content containing {path,
   sha256} records (e.g. current_exact_promotions'
   historical_qualification).
5. Fuzz manifests — `ratchets/fuzz-domain.v1.toml` source_references
   (path+sha256 of Rust sources) and `ratchets/fuzz-preflight.v1.json`
   source references (Rust sources AND the domain toml itself).
6. Artifact-internal pins — every `ratchets/*.json` artifact's
   `generator` {path,sha256} and its `inputs` records that carry
   {path, sha256/hash} of other files.

Audit mode: any path-adjacent 64-hex literal that no family classifies
is REPORTED (file, span, context line) — a report entry, not a fatal
error, except inside `crates/oracle/*.mjs` where SPEC.md already
established zero.

**Acceptance for M1** (put results in `new-ci/pin-index-report.md`,
also generated/uncommitted):
- Counts per family, with cross-checks: family 3 finds exactly 16;
  family 2 covers at least every file named in scripts/pin-audit.py;
  family 5 finds the m8_evidence.rs references.
- Incident validation: for commits `e8e32f61` and `e1957f77` (this
  repository's history), every changed 64-hex literal in their diffs
  is an indexed pin span under the index built from each commit's
  PARENT. This proves the index would have predicted this week's
  sequential pin-surface discoveries in one report.

## M2 — prospective concordance (the plan primitive)

Extend the shadow machinery into `cargo run --bin plan -- <base-ref>
<head-ref>`: given two commits, predict the FULL consequence of the
tree diff BEFORE any walk runs:

- which ladder rungs' cores change (real logic) vs envelope-only;
- which rungs go stale transitively (their pinned inputs' producers
  re-mint), in topological order, including rungs whose pins are
  currently green and only become stale AFTER an upstream re-mint;
- which non-ladder pin surfaces (families 2-6) will need re-pinning
  after those re-mints.

Output `new-ci/plan-report.md`. **Acceptance:** running it for
`9bacb97e..e1957f77` (the gate-tax-4 landing) predicts a superset of
the six surfaces actually hit this week: full-ladder staleness from
xtask byte changes, harness pins in h2_3c_profile.rs, the policy
main.rs pin, the h2-5g-profile schema const, and both fuzz manifests.
State precision/recall against that measured list explicitly.

## M3 — substrate hardening (design §3/§7/§8 into code)

Implement in the library what DESIGN (now at
docs/design/greenfield/new-ci-evidence-dag.md, identical content to
your draft) specifies but the first slice stubbed:

- transaction manifest + immutable root-generation promotion with the
  two crash windows recoverable (before-close, after-close);
- advisory leases with owner tokens and fencing epochs; stale-owner
  CAS loss; reclamation only via higher epoch;
- typed status receipts (success/failed/cancelled/timed-out/
  diagnostic) with only verified success eligible for HIT;
- adversarial tests: kill-simulation for both promotion crash windows,
  concurrent CAS mint race (threads), lease fencing (stale owner loses
  even with a wrong clock), status-eligibility.

## Review contract

Morning review will re-run everything independently. Claims without a
runnable demonstration are treated as absent. Keep `cargo clippy`
clean inside new-ci/.
