# Design index

`docs/design` is the place for durable design: north-star architecture,
deep checker seams, and active designs that should guide future
implementation. Tactical workstream plans, old snapshot-specific
roadmaps, and completed step guides live under `archive/`.

## How to Use This Directory

- Start with the smallest design that owns the subsystem you are about
  to change.
- Read the referenced tsc anchors and probe before changing behavior.
- Keep implementation checklists close to the design while they are
  active, then move stale or completed checklists into `archive/`.
- Do not add broad local patches to 2XXX behavior without reading the
  2XXX roadmap first.

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
- [greenfield/](greenfield/README.md): the execution companion to the
  five docs above — milestone-by-milestone (M0-M7) step guides that
  sequence the from-scratch build for implementing agents, with
  verified tsc anchors and per-stage acceptance gates.
- [greenfield/completion-convergence-plan.md](greenfield/completion-convergence-plan.md):
  the active cross-milestone execution plan for set-monotone ratchets,
  exact scope identities, T4/completion gates, produced M8 evidence, and
  the ordered path from the current M4 state through M9.
- [greenfield/non-2xxx-first-order.md](greenfield/non-2xxx-first-order.md):
  the non-2XXX family map — owner-based decomposition of the bands
  outside 2000-2999 with measured baselines and per-family acceptance;
  feeds the convergence plan's A5 slice and M7 stage gates.

## Active Deep Designs

- [type-checking-2xxx-roadmap.md](type-checking-2xxx-roadmap.md):
  cross-workstream design for 2XXX diagnostics and the local-vs-deep
  architecture decision boundary.
- [type-checking-2xxx-execution-plan.md](type-checking-2xxx-execution-plan.md):
  readiness checks, dependency order, and stop conditions for entering
  central 2XXX behavior work.
- [non-2xxx-blockers.md](non-2xxx-blockers.md): the blocker map for
  everything outside the 2XXX band — parser recovery/parse-error gate
  residue, implicit-any 7XXX, unused band, 18XXX sub-families,
  override 4XXX, 17XXX, suggestion families, lib axis, and infra.
- [non-2xxx-quick-wins-steps.md](non-2xxx-quick-wins-steps.md):
  implementation steps for the five N0 quick wins (enum constant
  evaluation, regex-validator wiring, scanner dedup, static-block
  grammar, `new.target`). Move to `archive/` as workstreams land.
- [relation-kind-facade-steps.md](relation-kind-facade-steps.md):
  byte-identical implementation steps for the `RelationKind` facade
  scaffold. Move to `archive/` when it lands.
- [candidate-boundary-steps.md](candidate-boundary-steps.md):
  byte-identical implementation steps for the call-candidate
  speculation boundary scaffold. Move to `archive/` when it lands.
- [candidate-call-resolution.md](candidate-call-resolution.md):
  transactional call-candidate design for `TS2345`, `TS2554`,
  `TS2769`, and `TS2349`.
- [destructuring-parameter-implicit-any.md](destructuring-parameter-implicit-any.md):
  leaf-level `TS7031` design for destructuring parameters.
- [destructuring-parameter-implicit-any-steps.md](destructuring-parameter-implicit-any-steps.md):
  current implementation steps for the `TS7031` design. Move this to
  `archive/` when the workstream lands or becomes stale.
- [architectural-debt.md](architectural-debt.md): targeted debt items
  that should be implemented only when a workstream proves they block
  meaningful progress.
- [stall-playbook.md](stall-playbook.md): how to detect an architecture
  stall and choose the right deeper migration.

## Reference and Process

- [EXECUTION-GUIDE.md](EXECUTION-GUIDE.md): implementation loop,
  verification rules, and FP/FN triage procedure.
- [knowledge-base.md](knowledge-base.md): pinned non-obvious facts and
  standing pitfalls.
- [tsc-source-guide.md](tsc-source-guide.md): how to navigate the
  vendored `_tsc.js` source.

## Archive

- [archive/README.md](archive/README.md): archived roadmaps and
  snapshot-specific workstreams.
- [archive/convergence-roadmap-2026-07.md](archive/convergence-roadmap-2026-07.md):
  old 2026-07 convergence roadmap and priority table.

Archived documents are preserved for context, not treated as the current
source of design truth.
