# new-CI evidence-DAG — design packet (successor of the lost 2026-08-18 draft)

Status: NORMATIVE DESIGN + verified prototype. This document is the
in-repo successor of the 2026-08-18 'tsc-rs 証拠DAG設計' artifact,
whose published page and scratchpad source were both lost — design
documents now live here (in-repo principle, ratified 2026-08-26).

## Provenance and verification

- Drafted 2026-08-26 by Codex CLI (gpt-5-codex) under a written mission
  spec (new-ci/SPEC.md), in an isolated worktree, network-blocked,
  boundaries verified (only new-ci/ touched).
- Independently verified by the session owner-agent: new-ci/ cargo test
  9/9 green (re-run), clippy clean, shadow adapter re-run against the
  live repository: 55 nodes, 113 projection-labelled edges, incident
  a8aa644b classified 44/44 envelope-only with 0 unclassified
  path-adjacent 64-hex literals — i.e. under these keys the incident's
  89-minute observation node receipt HITs while printer.rs correctly
  invalidates its true core dependents.
- The reference implementation lives in new-ci/ (standalone Cargo
  project, NOT a workspace member — zero ladder tax; see
  new-ci/README.md). The shadow adapter is the standing reporter.
- gate-tax 5 consumes this prototype's pin-span extraction as the
  bootstrap of its typed pin-index (ratified 2026-08-26).

## Governing question set

The ten normative questions below came out of the 2026-08-26 external
design review (Codex consult #2); this packet answers them. Related
review outcomes recorded in the gate-tax-5 packet: receipt key formula
correction (projection-labelled dependency edges), G2 SLA split by
change class, hosted-cache safety constraints.

## Sequencing (RATIFIED 2026-08-27)

Land this substrate + a current-oracle adapter in shadow mode FIRST;
the tsgo producer is added under the same receipt/store/key contract
AFTER the substrate is trusted — never both migrations at once
(running two full systems during an oracle migration doubles the
expensive sweeps and makes cache/key failures indistinguishable from
legitimate compiler divergence). The design remains unified with the
tsgo Phase 0 oracle rebuild; only the LANDING order is staged. Shadow
mode requires an explicit budget, a sampling/full-run schedule, a
semantic-versus-byte comparison rule, an expected-difference taxonomy,
and promotion + rollback criteria.

# Evidence-DAG CI redesign: first-slice design

## Divergences

These are deliberate first-slice boundaries, rather than unspecified
behavior:

1. The library uses a small, audited-in-this-directory SHA-256 implementation
   instead of `sha2`, so `cargo test` works without network or a populated
   crates.io cache.
2. Receipt records are uncompressed binary records in an append-only
   directory.  Content blobs, compression, remote signatures, and active
   leases have explicit interfaces and policy below, but only the local
   receipt mint and GC-root stub are implemented in this slice.
3. `shadow` is a report-only adapter.  It parses the repository's five
   checked-in pin grammars and uses read-only `git show`; it does not execute
   an oracle or materialize an oracle artifact.  The 9,027-case observation is
   represented as a derived core consumer for the incident replay.

The divergences do not change the receipt-key contract or the kill-safety
guarantee.

## Normative answers to the review questions

### 1. Producer, input, child-process, and host digests

The semantic receipt key is:

```text
H(
  schema/version,
  action-definition + implementation digest,
  semantic input manifest,
  sorted labelled dependency output/projection digests,
  explicit baseline digest-or-null
)
```

An action is typed as `(tool, tool-version, action-definition-digest,
implementation-digest)`.  The implementation digest covers the complete
producer: executable or script bytes, loaded plugins, declared toolchain
files, and the version selected by the action.  The action definition digest
covers the operation name only as part of a complete definition: normalized
argv, declared environment allowlist, resource/timeout policy when it can
change the result, output schema, and child-process policy.  A bare action
name is never sufficient.

The semantic input manifest is a sorted set of labelled `(logical-name,
digest)` entries.  Each entry identifies the bytes or canonical structured
value that can affect evidence.  Paths are labels, not identity: the digest
is over canonical bytes.  The manifest includes declared configuration,
toolchains, source inputs, generated inputs, and target/platform facts when
they affect the result.

Dynamic children are not silently folded into a parent's ambient process
state.  A producer declares each permitted child executable and invocation
contract in its action definition.  At execution time each child gets its
own typed action and receipt; the parent records a labelled dependency on
the child's `core` and/or `envelope` projection.  A newly discovered child,
different argv, executable, environment allowlist value, or child output
therefore changes the parent key or makes the execution invalid rather than
creating an untracked cache hit.  Network services are either content
addressed and declared as inputs or are forbidden for a hermetic action.

Host state is split explicitly.  Output-affecting facts (target triple,
selected CPU features, OS/ABI, filesystem semantics, and relevant runtime
versions) are canonical semantic manifest entries.  Worker hostname, PID,
workspace pathname, wall clock, load, retry count, scheduling priority, and
other execution observations are receipt metadata only.  If an otherwise
unlisted host fact can affect bytes, the action is not cacheable until that
fact is promoted into the semantic manifest.

### 2. Versioned baseline and ratchet state

Baselines and ratchets are immutable objects addressed by their full digest
and a schema version.  A versioned record contains its kind, schema version,
parent digest (if any), canonical payload, producer identity, and promotion
provenance.  Names such as `h2-5g-profile.v1.json` are human-facing labels;
the receipt and graph always carry the digest.  There is no mutable `latest`
pointer in a key path.

The key encodes baseline as a tagged option: `Some(digest)` and `None` have
different bytes, including when the baseline payload happens to be empty.
Promotion creates a new immutable root manifest that points to the chosen
baseline/ratchet generation.  A reader pins that root manifest ID for its
whole run.  Ratcheting is append-only: a new schema or interpretation gets a
new version and migration receipt, never an in-place rewrite of an old
object.

### 3. Root-graph transaction and promotion

Per-node receipt minting and artifact publication are separate operations.
The node receipt is a complete, checksummed record whose final filename is
created by write-temp, `sync_all`, and same-directory rename.  Materialized
files are disposable views and may be overwritten in a private staging
directory.

For a graph run, the coordinator writes a transaction manifest containing
the pinned root input, every node key and receipt record ID, every output
projection digest, and the artifact blob IDs.  It validates all edges and
closes the manifest with a complete marker.  Only then does it publish a new
immutable root-generation record by an atomic create operation.  Readers
open a generation ID supplied by the caller; they never follow a mutable
latest pointer.  A crash before close leaves an unpromoted transaction and
staging files.  A crash after close but before publication is recoverable by
replaying the idempotent publication.  An old promoted generation remains
valid during either case.

### 4. Shards, batches, ordering, duplicates, and kill recovery

The coordinator first canonicalizes the complete expected item-ID sequence.
Each batch receipt contains the sweep ID, batch index/count, contiguous
global range, exact expected IDs for that range, ordered `(ID, output
digest)` items, a union digest over that ordered list, and `complete=true`.
The verifier requires:

1. every expected ID is non-empty and globally unique;
2. all batch indices are present exactly once and agree on batch count;
3. ranges are contiguous from zero with no overlap or gap;
4. each item ID equals its expected ID at the same ordinal and item IDs are
   unique; and
5. each batch union digest and the final ordered union digest recompute
   exactly.

Workers mint a batch only after those checks.  A kill can leave an ignored
   temporary file, but cannot leave a valid partial batch; recovery resumes
   missing ranges and validates already minted batches before adoption.
   Shard count is therefore an execution choice, not semantic identity.

### 5. Size, compression, retention, roots, leases, GC, and recovery

The local prototype bounds a receipt record at 64 MiB and an individual
record string at 16 MiB.  Production policy bounds a transaction and batch
according to the configured store quota, rejects oversized records before
mint, and stores large output bytes separately in a digest-addressed blob
CAS.  Receipts retain only projection digests and small provenance.  Blob
compression is by content type and is keyed by the uncompressed digest;
identical content deduplicates before compression, so compression choices
cannot change identity.

GC roots are promoted graph generations, active transaction manifests,
unexpired leases, audit/legal holds, and explicitly retained diagnostic
receipts.  Retention first removes unrooted generations after the policy
grace period, then unreferenced receipts/blobs after a second grace period.
The first slice exposes an explicit no-op GC-root hook so accidental policy
omission cannot delete evidence.  It never follows a mutable latest link.

Recovery ignores `.tmp` files, truncated records, bad magic/version, invalid
fields, checksum failures, and records with trailing bytes.  A valid old
receipt is never deleted or replaced by recovery.  A directory scan is
deterministically ordered and returns all valid versions for a key, allowing
the coordinator to choose a successful, policy-approved attempt.

### 6. Cross-machine receipts, hosted trust, poisoning, and PR namespaces

The portable unit is a receipt plus its immutable input/output objects and
provenance: schema, complete action identity, semantic manifest, labelled
projection edges, baseline option, producer platform facts, and source
namespace.  A remote worker may upload it only after local verification of
all referenced digests.  Hosted trust additionally requires a signature over
the canonical receipt and an attestation/provenance policy accepted by the
host; an unsigned receipt is a local diagnostic, never a hosted success.

Remote caches are append-only and verify every object digest before serving
it.  A receipt whose action, input namespace, or dependency provenance is
not authorized is ignored rather than merged.  Pull requests use an
untrusted, repository-and-commit-scoped namespace with no write access to a
trusted branch namespace.  A PR result can be promoted only by a trusted
job that revalidates it under the destination namespace and signature
policy.  Thus a malicious PR cannot poison a shared trusted key or replace a
baseline.

### 7. Concurrency, locks, leases, CAS, and stale owners

Receipt mint is a CAS operation: a worker writes a unique temporary object
and creates a never-before-used final object name.  It does not overwrite a
key's existing version.  Multiple successful versions for one key are
allowed and are selected by transaction policy.

Coordination locks are advisory and scoped to `(namespace, key)`.  A lease
has a random owner token, monotonic deadline, renewal record, and fencing
epoch.  Mutations include the owner token and epoch; a stale owner loses the
CAS even if its clock is wrong.  Expiry is confirmed by the coordinator's
monotonic lease service, not by deleting another worker's receipt.  A stale
lock may be reclaimed only by creating a higher fencing epoch.  If a worker
dies, its temp file and lease eventually expire; existing receipts remain
untouched and another worker may mint a new version.

### 8. Failed, cancelled, timed-out, and diagnostic receipts

Every attempt has the same semantic key and an explicit status.  The local
model carries `success`, `failed(error-code,message)`,
`cancelled(reason)`, `timed-out(deadline)`, or
`diagnostic(code,message)`.  Only `success` with verified `core` and
`envelope` outputs is eligible for a cache HIT.  Failure and cancellation
records are retained for observability and retry policy but never satisfy a
success lookup.  A timeout records the deadline and worker provenance;
diagnostic records can preserve partial measurements without pretending
that an output is complete.  A root transaction may promote only the
statuses allowed by its policy, normally successful nodes and explicitly
reviewed diagnostics.

### 9. Semantic versus execution-only fields

Semantic fields are schema/version, complete action definition and producer
implementation, canonical input labels/digests, sorted labelled dependency
projection/digests, explicit baseline digest-or-null, and output projection
content.  They can change evidence and therefore change a key.

Execution-only fields include shard count, batch assignment, scheduling
priority, retry/attempt number, worker ID, workspace path, PID, start/end
times, hostname, load, temporary filenames, and ordinary host fingerprints
used only for diagnosis.  They are serialized in the receipt's execution
metadata but are absent from the key preimage.  In particular, changing
shard count, priority, or workspace path cannot create needless cache
versions; the batch proof still validates the chosen execution layout.
Host facts move into the semantic manifest only when they affect output, as
specified in question 1.

### 10. Canonicalization, migration, rollback, and observability

The key and batch preimages use UTF-8 strings with explicit big-endian
lengths, fixed-width digest bytes, tagged optional values, and explicit
projection tags.  Input entries sort by label.  Dependency entries sort by
label, projection, then digest.  Arrays whose order is semantic retain their
order; sets are sorted.  There is no locale, path spelling, map iteration,
JSON whitespace, or filesystem-order dependency.

The schema/version is part of the first key field.  A schema migration
reads and validates an old canonical record, writes a new versioned record,
and records old-to-new provenance; it never guesses that old bytes have new
meaning.  Rollback selects an earlier immutable root generation and leaves
new receipts available for later promotion.  It does not mutate or delete
the newer generation.

Observability emits the key, schema, action identity, input labels (with
secret values redacted), dependency labels/projections, status, receipt
version, batch ranges, and timing separately.  It records cache HIT/MISS,
invalidated projection, rejected/torn record count, lease/fencing events,
promotion transaction ID, and reason-coded failures.  It never logs secret
input bytes or treats execution metadata as semantic identity.  The shadow
report follows the same rule: it records each masked pin span and both
projection digests so a reviewer can reproduce the incident classification.
