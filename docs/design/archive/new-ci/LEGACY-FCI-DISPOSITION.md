# Legacy FCI crate disposition (2026-08-28, user-ratified)

The six `crates/ci-*` workspace crates were the paused "Functional CI"
(FCI) implementation — last substantive commit `3e309a0a` ("chore(fci):
close 3c/5b proofs and stage 5c.1b ready packet", 2026-08-16). They are
scheduled for DELETION in the combined gt6 enforcement/deletion train,
with this file as the durable design record. Decision basis: dual
independent Codex review (gpt-5.6-luna and gpt-5.6-sol, both max
reasoning, read-only), converging on delete-with-record and NO source
absorption; user approval 2026-08-28.

## Why deletion, not porting

- No consumer: no workspace crate, no `.github/ci` validator, and no
  ratified design document references them. The live substrate is this
  out-of-workspace `new-ci/` project (see `README.md`/`STATUS.md`),
  which independently re-derived the concepts under the normative
  packet `docs/design/greenfield/new-ci-evidence-dag.md`.
- Structural incompatibility with the ratified packet (line-verified):
  the FCI graph has unlabelled `Box<[I]>` dependencies where the
  packet requires projection-labelled edges (`TransactionEdge`); FCI
  membership requires lexically sorted values where the packet uses
  canonical expected-ID sequences (`BatchReceipt`); `FixedPlanV1`
  embeds shard layout in semantic plan bytes, conflicting with the
  packet's execution/semantic split (Q4/Q9); the old
  `AdapterDescriptorV1` registry is an unratified authority layer next
  to the packet's `Action(tool, version, definition, implementation)`
  identity.
- Cost: as workspace members they rode every gate's fmt/clippy/
  workspace-test and held ~56 entries in the H2.5g runtime-input
  closure. Consolidating under `crates/ci/` would not help — the
  oracle pin ladder hashes every `crates/**/*.rs` regardless of Cargo
  membership.

**Preserve the reasoning, not a second implementation.** The design
survives here, in the frozen FCI slice packets under
`docs/design/greenfield/slices/fci-*.md`, and in git history at
`3e309a0a`.

## What is NOT deleted

- `.github/ci/fci-h2-5g-membership.mjs` — the REAL H2.5g membership
  shadow is this standalone JS artifact (its packet, FCI-5c.1, states
  it does not modify ci-core); it stays live.
- The FCI readiness envelopes and their qualification-gate validation
  (`.github/ci/qualification.mjs`) — historical packet digests remain
  validated; frozen FCI slice packets are not rewritten.
- Root `sha2`/`libc` workspace dependencies — other crates use them.

## Module dispositions (concept level)

DEFER = "re-evaluate from git history when the named packet is
written," never "copy dormant code forward now."

| Surface | Disposition | Future owner (if any) |
|---|---|---|
| ci-core canonical/digest/graph*/membership/model/ids/input/impact::ImpactPlan/adapter/registry | DROP | superseded by new-ci `Action`/`ReceiptKey`/`TransactionEdge`/`BatchReceipt`/binary canonical preimages |
| ci-core explain::{PlanningBudget, PlanningObservation} | DEFER | Phase 0 shadow budget/comparison/promotion policy packet |
| ci-core impact::{TrustRoot, GraphTransition} | DEFER | post-H2.9 trusted-promotion / hosted-trust packet |
| ci-core inventory | DEFER | tsgo producer / source-snapshot packet (re-derive wire format from the packet, not from this code) |
| ci-core hash | DROP | trusted hashing lands via `sha2` in the promotion-hardening train (new-ci's local SHA-256 caveat) |
| ci-runner bounded/error/resource/snapshot | DEFER | producer-execution / quota packets; snapshot needs a fresh safety review |
| ci-adapter-tsc-rs-protocol invocation/observation framing | DEFER | tsgo child-process protocol packet |
| ci-adapter-tsc-rs-protocol RootReceipt/CaseId/ShardRange/ShardSpec/FixedPlan + JSON codecs | DROP | superseded by transaction close/generation/root publication + Q10 encoding |
| ci-adapter-tsc-rs-control, ci-harness-tsc-rs, ci-testkit | DROP | — |

## Deletion-train checklist (the removal commit set)

1. Delete the six `crates/ci-*` directories; remove their six
   workspace members and six `tsc-ci-*` aliases from root
   `Cargo.toml`; regenerate `Cargo.lock`.
2. `crates/oracle/h2-5g-profile.mjs`: remove the 56 FCI paths from
   `NEW_RUNTIME_INPUTS` (deleted paths do NOT self-heal — the changed-
   path filter excludes deletions); runtime-input cardinality 259→203;
   update the size assertion and the schema `minItems`/`maxItems`;
   regenerate `ratchets/h2-5g-profile.v1.json`.
3. `crates/xtask/src/ci_test_receipts.rs`: drop `CONTROL_INPUTS`,
   `TESTKIT_INPUTS`, `CI_CORE_INPUTS` and the four ci-* receipt rows
   (table 7→3); preserve the input-invalidation unit-test coverage via
   an injected test-only scope, not by widening a production scope.
4. `crates/xtask/src/acceptance_plan.rs`: replace the obsolete
   `crates/ci-` exemption with `new-ci/` (today `new-ci/*` falls
   through to fail-closed all-slices, contradicting the README's
   "invisible to the gate") and update the unit fixture path.
5. `rg` residual-reference audit; final bytes; ONE
   `scripts/chain-walk.sh` walk; full gate.
