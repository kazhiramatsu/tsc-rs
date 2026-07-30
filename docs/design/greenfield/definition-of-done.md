# tsrs2 definition of done

One page, normative. If any other doc disagrees with this one about
WHAT "done" means, this doc wins; file a doc fix. (Milestone HOW
lives in [greenfield.md](../greenfield.md) §§7-8 and the steps docs.)
Adopted 2026-07-14 after the external full-project review; the
review's checkpoint table is folded in below.

## What tsrs2 is

A batch **diagnostics checker** for TypeScript, byte-compatible with
**TypeScript 6.0.3 exactly** (the vendored
`tsrs2/vendor/typescript-6.0.3/lib/_tsc.js` bundle and its lib set —
the same artifact the oracle runs). "tsc compatibility" in any tsrs2
context means THIS bundle, not tsc-at-large.

## Done means

On the versioned conformance corpus (`ts-tests/tests/cases/conformance`,
matrix-expanded by the harness — 5,908 fixtures at adoption), fixed for
each gate run and growable only through
[A1's append-only reviewed universe transition](measurement-integrity.md#2-a1--accepted-conformance-state), using the
two-view contract in [m8-readiness.md](m8-readiness.md):

1. **Supported-scope T3 = 100%** for the syntactic AND semantic oracle passes:
   every diagnostic matches on file/code/line/col (T0), category
   (T1), full span + top message text (T2), and message chain +
   relatedInformation (T3). The full-corpus FN residue remains
   visible, and **all-corpus FP = 0 is absolute at every point in
   between** (the standing invariant, not just at the end). Scope
   exclusions are exact reviewed diagnostic identities; no
   fixture/code/glob exclusion exists.
2. **Supported-scope T4 per case**: replaying the complete oracle records
   through the Rust formatter first reproduces the genuine
   `oracle_cli_hash` stored in the schema-3 golden. After that formatter
   anchor verifies, exact A2 scope is applied and the supported oracle and
   current tsrs rendered bytes are byte-identical — ordering and dedupe
   included. The golden stores oracle output evidence only; it never
   persists a tsrs output or `tsrs_cli_hash` baseline.
3. **Suggestion-pass rows are in scope** insofar as they are
   emit-free (the M7 band: unused/grammar/suggestion). They already
   sit in the All-band denominator (band filtering is code-based);
   emit-DEPENDENT suggestions (if any surface) get exact
   `emit-dependent` scope dispositions plus the corresponding
   not-applicable function-ledger disposition instead of silent
   omission.
4. **Zero escapes**: no `Unsupported` containment sites remain
   (`cargo xtask escapes` reports sites=0), the site manifest
   (`tsrs2/escapes.toml`) is empty, and every ledger entry is
   hash-fresh against the vendored bundle. Every checker
   pub/pub(crate) function carries a DISPOSITION (the tsc-port
   header family / tsrs-native / tsc-deferred / tsc-not-applicable
   — the `fn-dispositions.toml` backlog allowlist admits ONLY
   deletions and empties before M8 starts); the tsc-side converse
   (emitter inventory + dependency closure over the SAME exact
   tsc-span/tsc-hash declaration identities; function names are aliases,
   not keys) is the M8-start checkpoint. Parse-recovery guards may
   be separately ratcheted through M7, but must move off the
   `Unsupported` channel before this final gate. The escape ledger's
   `dormant-assumption` entries are constructibility debt rather than
   runtime `Unsupported`, but share the same final zero/empty-manifest
   requirement.
5. **Determinism invariants green** at full scope: idempotence,
   jobs-independence, prefix-determinism, encodings,
   matrix-independence.
6. **Corpus-external confidence** is CLAIMED ONLY as: the
   differential fuzzer introduced by the M8-start gate (generator +
   oracle comparison + reducer + signature dedupe) reaches M9's CI
   steady state — new divergence signatures < 1/night — with no
   known-open divergence class.
   This is an engineering bar, not a formal guarantee.

## Milestone gates vs slice fidelity

The implementation is designed for full T0-T4 parity from the first
ported branch, but corpus-wide activation remains staged. These are
different obligations:

- **Milestone gate**: the active corpus gate starts at T0 + absolute
  all-corpus FP=0; an exact-count T1 aggregate joins it at M7 8.4.
  M8 replaces that aggregate authority with exact A1 T1-T3 bucket
  sets through `tier1-3-input-schema-extension`, while A3/T4 activates
  only under
  [measurement-integrity.md §4](measurement-integrity.md#4-a3--t4-activation).
- **Touched-family fidelity**: a slice that makes a diagnostic family
  observable follows that family vertically through every tier whose
  prerequisites are live. T1 category, T2 span/top message, and T3
  chain/related information are oracle-pinned in the same slice when
  available; the slice may not knowingly choose a T0-equivalent but
  structurally wrong reporter merely because the higher tier is still
  shadow-only.
- **Pre-identity-diff evidence history**: before phase 9.3c, conformance
  exposed T1/T2/T3 only as band-level shadow rates plus row data in
  `mismatches.json`. Slices therefore recorded before/after rates and
  reviewed target-family rows directly; that historical procedure was
  review evidence, never an automated identity-level ratchet. Phase 9.3c
  added exact per-tier matched/lost/gained identities to
  `conformance --out-json`, and
  `cargo xtask conformance-diff <old> <new>` now compares two observations.
  The report never changes A1 or a ratchet artifact. Before the formal
  `tier1-3-input-schema-extension` it is shadow/report-only; after that
  transition it remains supplemental slice evidence while A1 is the
  enforcement authority. Do not reconstruct tier identities from aggregate
  rates.
- **Pre-A3 T4**: local formatter goldens and
  `conformance --tier t4 --report-only` are validation evidence, not
  accepted T4 identities and not corpus-wide T4 activation. The A3
  schema transition and T4 accepted-set ratchet still wait for the
  globally frozen A2 scope and zero live `resolved` entries.
- **Blocked upper tier**: if a shared prerequisite (diagnostic-chain
  builder, program/global assembly, formatter, or an unconstructible
  type family) prevents vertical completion, the slice records the
  exact blocker, owner, retirement milestone, and oracle anchors. A
  bare "T2/T3/T4 later" note is not sufficient evidence. The slice
  must not expand that debt beyond the rows it unlocks.
- **Prerequisite-only slice**: a data-model or cache-discipline change
  may legitimately add no accepted diagnostic. It names the downstream
  family, carries direct semantic pins, leaves every active accepted
  set unchanged, and is followed by the consuming slice rather than
  being counted as parity progress.

M8 performs the formal cross-family tier sweeps and closes every
recorded upper-tier blocker. This policy keeps the causal clarity of
staged gates without creating a T0-only reporting architecture that
must be replaced at the end.

Checked JavaScript, including tsc-compatible JSDoc parsing, arena nodes,
binding, type construction, relations, and diagnostics, is part of the M8
batch-diagnostics surface. `plainJSErrors` is a tsc dispatch rule, not a
scope boundary. Historical exact `jsdoc-semantics` exclusions return through
A2 tombstones when their dependencies are implemented; a broad JSDoc
exclusion is not permitted.

## Explicitly out of scope

- **Emitter** (no JS/d.ts output; emit-dependent diagnostics get
  dispositions, per [2xxx-first-order.md](2xxx-first-order.md)).
- **Module HOST resolution**: node_modules, package.json,
  `paths`/baseUrl, project references, `/// <reference>` redirects
  ([program-and-modules.md](program-and-modules.md)) — the checker
  resolves only in-program files + ambient/pattern-ambient modules
  (m4-58 §9). Exact affected oracle diagnostics receive
  `host-resolution` scope dispositions. They stay FN in the
  all-corpus visibility metric and are not chased.
- **LSP / watch / incremental** ([lsp-and-incremental.md](
  lsp-and-incremental.md) is design-only). Preconditions if ever
  started: owned lib cache (no `Box::leak`), collision-safe keys.
- **Public TypeChecker API** surface.
- **Upstream tracking** (>6.0.3): a separate project with its own
  re-vendor + goldens-regeneration + ledger-refresh loop; nothing
  here promises forward compatibility.

Emitter, LSP/watch/incremental, and a public TypeChecker API are separate
follow-on design tracks, not deferred M8 slices. Each requires its own goal,
compatibility surface, oracle/reference contract, performance bounds, and
definition of done. Work in those tracks may reuse the batch checker but may
not alter this scope denominator or claim M8 acceptance credit. See
[M8 execution and close](m8-execution-and-close.md#separate-follow-on-design-tracks).

## Performance / memory bounds

- Full-corpus conformance stays at or below **60 s wall** on an approved
  runner profile.
- Peak RSS stays within the reviewed ceiling declared for that approved
  profile in `tsrs2/m8-evidence.json`. Completion consumes a fresh B4
  observation against those ceilings; this document does not embed a
  transient current measurement.

## Go / no-go checkpoints (external review, 2026-07-14)

| Gate | Bar |
|---|---|
| M4 close | T0 ≥ 35% (All band), untagged escapes 0, stale 0 — first real go/no-go |
| M5 close | flow landed with idempotence + jobs-independence still green |
| M6 start | speculation scoped-transaction API + failed-candidate rollback tests exist ([m6-inference-calls-steps.md](m6-inference-calls-steps.md) precondition) |
| M8 start | `cargo xtask m8 readiness --require-ready`: M7 gate, globally identity-anchored frozen exact scope, T1-T3 shadow metrics, declaration-identity all-band emitter inventory + dependency closure + runtime coverage, current-fingerprint fuzzer evidence, and current performance/RSS evidence on an approved reference runner |
| Done | this page's §"Done means", all six clauses |

Shadow T1/T2/T3 rates are measured (non-gating) from pre-5.8a
onward; a fixture family that reaches completion may ratchet its
tier early, but the GATES stay T0+FP=0 until M8 activates the
higher tiers corpus-wide.
