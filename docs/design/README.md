# Design index

`docs/design` is the place for durable design: north-star architecture,
deep checker seams, and the active greenfield build. Tactical
workstream plans, old snapshot-specific roadmaps, completed step
guides, and v1-era operating instructions live under `archive/`.

**The authoritative execution docs for active work are under
[greenfield/](greenfield/README.md)** — implementers start from the
stage step docs referenced there.

## How to Use This Directory

- Start with the smallest design that owns the subsystem you are about
  to change.
- Read the referenced tsc anchors and probe before changing behavior.
- Keep implementation checklists close to the design while they are
  active, then move stale or completed checklists into `archive/`.

## Core Architecture

- [greenfield.md](greenfield.md): from-scratch north-star architecture
  and rebuild trigger conditions.
- [core-interfaces.md](core-interfaces.md): data contracts for nodes,
  symbols, types, signatures, flow, diagnostics, and options.
- [syntax-and-binder.md](syntax-and-binder.md): scanner, parser,
  recovery, binder, symbol merge, and flow graph construction.
- [checker-foundations.md](checker-foundations.md): lazy resolution,
  check ordering, contextual typing, type construction, widening,
  instantiation, and member access.
- [checker-key-functions.md](checker-key-functions.md): relation,
  inference, overload, and flow algorithms.

## Active Execution (greenfield)

- [greenfield/](greenfield/README.md): the execution companion to the
  five docs above — milestone-by-milestone (M0-M9) step guides that
  sequence the from-scratch build for implementing agents, with
  verified tsc anchors and per-stage acceptance gates.
- [greenfield/completion-convergence-plan.md](greenfield/completion-convergence-plan.md):
  the active cross-milestone execution plan — workstreams, required
  landing order, and stop conditions from the current state through M9.
- [greenfield/measurement-integrity.md](greenfield/measurement-integrity.md):
  the A1/A2/A3/A5 + D2 measurement contracts — artifact schemas,
  anchors, and adversarial tests.
- [greenfield/evidence-and-steady-state.md](greenfield/evidence-and-steady-state.md):
  the B1-B4 evidence contracts, required CI topology, and the M9
  steady-state window.
- [greenfield/2xxx-first-order.md](greenfield/2xxx-first-order.md):
  first-order decomposition of the 2XXX band with measured baselines;
  owns the M5/M6-before-sweep phase plan.
- [greenfield/non-2xxx-first-order.md](greenfield/non-2xxx-first-order.md):
  the non-2XXX family map — owner-based decomposition of the bands
  outside 2000-2999; feeds the A5 family rollup and M7 stage gates.

## Reference (v1-era, kept in place)

These were written against the paused v1 codebase (tag `v1-final`) and
are still cited from the docs above for durable facts; their command
lines and `src/` paths only work at that tag.

- [knowledge-base.md](knowledge-base.md): pinned non-obvious facts and
  standing pitfalls (oracle behavior, corpus quirks, tsc internals).
- [tsc-source-guide.md](tsc-source-guide.md): how to navigate the
  vendored `_tsc.js` source.
- [stall-playbook.md](stall-playbook.md): how to detect an architecture
  stall and choose the right deeper migration; the refactor house
  style.
- [EXECUTION-GUIDE.md](EXECUTION-GUIDE.md): the v1 implementation loop
  and FP/FN triage procedure (the greenfield equivalents are the
  per-stage gates in `greenfield/`).
- [non-2xxx-blockers.md](non-2xxx-blockers.md): the v1 blocker map for
  the bands outside 2XXX; provenance input to
  `greenfield/non-2xxx-first-order.md`.
- [architectural-debt.md](architectural-debt.md): v1 debt items,
  referenced by `checker-key-functions.md` for context.

## Archive

- [archive/README.md](archive/README.md): archived roadmaps,
  workstreams (v1 and superseded), and v1 operating instructions.

Archived documents are preserved for context, not treated as the
current source of design truth.
