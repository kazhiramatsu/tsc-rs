# Design archive

This directory holds snapshot-specific or tactical documents that should
not define the current shape of `docs/design`.

Archived documents are still useful for provenance, old probes, and
implementation notes, but current work should start from the top-level
design index.

## Archived Roadmaps

- [convergence-roadmap-2026-07.md](convergence-roadmap-2026-07.md):
  old convergence roadmap, working protocol, and priority table from
  the July 2026 sweep.

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
  by `../type-checking-2xxx-roadmap.md` as the relation/2339 entry
  point; mapped sub-designs remain useful provenance.
- [workstreams/lib-gap-2304.md](workstreams/lib-gap-2304.md)
  and [steps](workstreams/lib-gap-2304-steps.md) — **parked, not
  completed**; mostly buys the raw metric (2304 is gate-filtered).
- [workstreams/u6-unused-fp.md](workstreams/u6-unused-fp.md) —
  **parked, not completed**; ~173 unused-family FPs remained at
  @3242fdc.
