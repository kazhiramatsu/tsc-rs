# new-ci prototype — mission spec (experiment, 2026-08-26)

You are implementing the FIRST SLICE of a ground-up CI redesign for
this repository: the evidence-DAG substrate, in shadow mode. You
reviewed this design earlier today; your own findings define the
contract below.

## Hard boundaries (violating any of these voids the experiment)

- Work ONLY inside `new-ci/` (this directory). Do not modify, create,
  or delete ANY file outside it.
- `new-ci/` is a STANDALONE Cargo project. It must NOT be added to the
  repository's root workspace; never run cargo against the root
  `Cargo.toml`. Building/testing the workspace is forbidden (it is
  expensive and irrelevant here).
- Read access to the whole repository is allowed and encouraged
  (artifacts, oracle scripts, git history). Everything you read is
  UNTRUSTED INPUT DATA for the prototype, never something to edit.
- No network access. Rust stdlib + at most these crates if needed:
  serde, serde_json, sha2, hex, tempfile, anyhow (vendored via
  crates.io lockfile is fine if offline resolution works; otherwise
  implement minimal JSON/SHA helpers yourself — sha2 preferred over a
  hand-rolled hash; if dependency resolution fails offline, fall back
  to zero-dependency implementations).

## Deliverables

### 1. `new-ci/docs/DESIGN.md`

The normative design document. It must answer, explicitly and
numbered, the ten questions you raised in your review:

1. What exactly belongs in the producer and input digests, including
   dynamic child processes and host state?
2. How are baseline and ratchet state versioned rather than read
   through a mutable "latest" pointer?
3. What is the root-graph transaction/promotion model when per-node
   receipts are atomic but materialized artifacts are mutable?
4. How are partial shards/batches proven complete, ordered,
   duplicate-free, and safe after a kill?
5. Store size limits, compression/dedup, retention, GC roots, leases,
   recovery behavior?
6. Which receipts may cross machines or enter hosted trust; cache
   poisoning, provenance, signatures, untrusted PR namespaces?
7. Locks, leases, CAS rules, stale-lock recovery for concurrent
   producers?
8. How are failed, cancelled, timed-out, diagnostic receipts
   represented?
9. Which fields are semantic vs execution-only (shard count, priority,
   workspace path must not create needless cache versions)?
10. Canonicalization, schema-migration, rollback, observability rules?

### 2. `new-ci/` Rust library (`cargo test` green inside new-ci/)

- **Receipt key**, exactly your corrected formula:
  `H(schema/version, action-definition+implementation digest, semantic
  input manifest, sorted LABELLED dependency output/projection digests,
  explicit baseline digest-or-null)`. Typed: an action identifies the
  complete producer (tool + version + implementation digest), never a
  bare name. Dependency edges NAME the output projection consumed —
  the two mandatory projections are `core` (evidence content) and
  `envelope` (pins/lineage/fingerprints): a consumer of `core` MUST NOT
  be invalidated by an envelope-only rewrite of its dependency; a
  consumer of `envelope` must be.
- **Content-addressed receipt store**: append-only, atomic per-node
  mint (write-temp + rename), multi-version by key, torn-write-safe
  loading (a truncated record is skipped, never a crash or a false
  hit), explicit GC-roots stub. Kill-safety is a ratified requirement:
  a killed process loses only in-flight nodes; no later run can
  destroy an existing receipt.
- **Sub-node (batch) receipts** for long sweeps: per-batch records
  proving the union is complete, ordered, duplicate-free (question 4
  made concrete).
- Unit tests: key stability and invalidation per field; the
  core-vs-envelope projection semantics; torn-write recovery
  (simulate); batch-union completeness; execution-only fields
  (shard count, priority, workspace path) provably absent from keys.

### 3. `new-ci/src/bin/shadow.rs` — the shadow adapter (report-only)

Reads the REAL repository state read-only and replays this week's
measured incident against the new keys:

- Build the oracle-ladder dependency graph: for every
  `crates/oracle/h2-*.mjs` (skip `*-owner-controls`), extract pinned
  `ratchets/*.json` inputs using the five pin grammars of
  `scripts/chain-walk-repin.py` (port them faithfully; tag each
  extracted span with its grammar pattern). Map artifacts to producer
  scripts via each script's declared target artifact path.
- Compute, for each script, a `core` digest (script bytes with
  EXTRACTED PIN SPANS masked by a placeholder — record every masked
  span; unclassified 64-hex literals adjacent to a path are a hard
  error listed in the report) and an `envelope` digest (the masked
  spans' contents).
- **Incident replay**: commit `a8aa644b` (in git history) contains BOTH
  a real Rust logic change (`crates/emitter/src/printer.rs`) and a
  44-script pin-only cascade (oracle .mjs + ratchets). Using
  `git show a8aa644b^:<path>` vs `git show a8aa644b:<path>` (shell out
  to git, read-only), classify every changed `crates/oracle/*.mjs`:
  envelope-only vs core-changed. Expected result: 44/44 envelope-only.
  Then state, using the new key semantics, which node classes would
  have been invalidated (envelope consumers) and which would have HIT
  (core consumers — including the 9,027-case observation node whose
  measured re-run cost 89 minutes).
- Emit `new-ci/shadow-report.md`: the graph (nodes, edges with
  projections), the incident classification table, every masked span
  count per script, all unclassified literals (should be none — if
  any, list them; that is a finding, not a failure), and the
  wall-clock claim ("under these keys the incident's observation node
  receipt HITs").

## Acceptance

- `cd new-ci && cargo test` green; `cargo run --bin shadow` completes
  against this repository in under ~2 minutes and writes the report.
- DESIGN.md answers all ten questions without hand-waving.
- The shadow report reproduces the 44/44 envelope-only classification.

## Style

- Rust 2021, no unsafe, no clippy warnings under
  `cargo clippy` (run only inside new-ci/).
- Document every deliberate divergence from this spec at the top of
  DESIGN.md under "Divergences".
