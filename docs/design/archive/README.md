# Design archive

This directory holds snapshot-specific or tactical documents that should
not define the current shape of `docs/design`.

Archived documents are still useful for provenance, old probes, and
implementation notes, but current work should start from the top-level
design index. Everything below was written against the paused v1
codebase (`src/`), which was removed from the working tree on
2026-07-15 and is preserved in full at tag `v1-final`; check out that
tag to run any command or path these documents mention.

## Archived Roadmaps

- [convergence-roadmap-2026-07.md](convergence-roadmap-2026-07.md):
  old convergence roadmap, working protocol, and priority table from
  the July 2026 sweep.

## Archived v1 Operation

The current setup and verification instructions live in
[../../setup.md](../../setup.md) and the repository `CLAUDE.md`;
these are the v1 equivalents, kept for provenance.

- [v1-setup.md](v1-setup.md): v1 bootstrap requirements
  (`scripts/bootstrap.sh`, `verify.sh`), generated fixtures, and
  verification commands.
- [v1-phase1-status.md](v1-phase1-status.md): v1 Phase 1 status report
  (`fn_stack` bracketing restoration).
- [v1-determinism-design.md](v1-determinism-design.md): why the v1
  checker was single-threaded per program and how its CFG flow
  resolver replaced the fact stack.

## Archived Workstreams

Status legend: **parked** = not completed, archived to keep the
top-level index small; revive the doc (and refresh its snapshot
numbers) when scheduling it. **superseded** = its role as a design
entry point moved elsewhere; keep for provenance.

- [workstreams/parse-error-gate.md](workstreams/parse-error-gate.md)
  and [steps](workstreams/parse-error-gate-steps.md) — **parked,
  first tranche landed**: commit `5412cb1` (2026-07-07) shipped the
  paired non-LHS `=` recovery + statement-level un-gating, so the
  doc's "tsrs drops ALL semantics on any parse error" premise and its
  yield numbers are STALE — refresh before reviving. The residue
  (recovery-profile parity, node-level gate granularity, FN-only
  grammar checks) is mapped in `../non-2xxx-blockers.md` owner 1;
  still the prerequisite for comprehensive 2XXX FN coverage.
- [workstreams/relation-core-2.md](workstreams/relation-core-2.md)
  and [steps](workstreams/relation-core-2-steps.md) — **superseded**
  by [workstreams/type-checking-2xxx-roadmap.md](workstreams/type-checking-2xxx-roadmap.md)
  as the relation/2339 entry point; mapped sub-designs remain useful
  provenance.
- [workstreams/lib-gap-2304.md](workstreams/lib-gap-2304.md)
  and [steps](workstreams/lib-gap-2304-steps.md) — **parked, not
  completed**; mostly buys the raw metric (2304 is gate-filtered).
- [workstreams/u6-unused-fp.md](workstreams/u6-unused-fp.md) —
  **parked, not completed**; ~173 unused-family FPs remained at
  @3242fdc.
- [workstreams/type-checking-2xxx-roadmap.md](workstreams/type-checking-2xxx-roadmap.md)
  and [execution plan](workstreams/type-checking-2xxx-execution-plan.md)
  — **superseded** by the greenfield rebuild: 2XXX ownership now lives
  in [../greenfield/2xxx-first-order.md](../greenfield/2xxx-first-order.md)
  and the [convergence plan](../greenfield/completion-convergence-plan.md).
  The v1 local-vs-deep boundary analysis remains useful provenance.
- [workstreams/candidate-call-resolution.md](workstreams/candidate-call-resolution.md)
  and [steps](workstreams/candidate-boundary-steps.md) — **superseded**:
  the transactional call-candidate design for v1; the greenfield
  equivalent is the M6 speculation transaction
  ([../greenfield/m6-inference-calls-steps.md](../greenfield/m6-inference-calls-steps.md)).
- [workstreams/destructuring-parameter-implicit-any.md](workstreams/destructuring-parameter-implicit-any.md)
  and [steps](workstreams/destructuring-parameter-implicit-any-steps.md)
  — **parked, v1**: leaf-level `TS7031` design; the greenfield
  implicit-any family is owned by M6
  ([../greenfield/non-2xxx-first-order.md](../greenfield/non-2xxx-first-order.md)).
- [workstreams/relation-kind-facade-steps.md](workstreams/relation-kind-facade-steps.md)
  — **superseded, v1**: the `RelationKind` facade scaffold; the
  greenfield relation engine was built with explicit relation kinds
  from M3.
- [workstreams/non-2xxx-quick-wins-steps.md](workstreams/non-2xxx-quick-wins-steps.md)
  — **parked, v1**: the five N0 quick wins against the v1 tree; the
  greenfield owners for these codes are mapped in
  [../greenfield/non-2xxx-first-order.md](../greenfield/non-2xxx-first-order.md).
