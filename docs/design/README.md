# Design index

`docs/design` is the place for durable design: north-star architecture,
deep checker seams, and the active greenfield build. Tactical
workstream plans, old snapshot-specific roadmaps, completed step
guides, and v1-era operating instructions normally live under `archive/`.
The completed/paused M-stage guides retained in `greenfield/` are a documented
historical exception and are classified by that directory's index.

## Document roles and precedence

Do not infer authority from detail or file age. Follow this order:

| Class | Owns | Examples |
| --- | --- | --- |
| Pinned upstream authority | TypeScript semantics and observable phase behavior | vendored TypeScript 6.0.3 declarations/bodies pinned by each slice |
| Active architecture | Current validated Rust ownership, types, invariants, and integration seams | core designs below; [current emitter architecture](greenfield/emitter-architecture.md) |
| Execution schedule | Dependency order, slice boundaries, and readiness/close gates | [post-H1 completion slices](greenfield/post-h1-completion-slices.md) |
| Slice packet | Exact bounded change, current symbols, commands, and expected results | the [slice-packet index](greenfield/slices/README.md) and active linked packet; H2.5g uses the indexed sole legacy-closure exception |
| Frozen contract/evidence | A predecessor claim and its immutable observations | [H1 emit](greenfield/h1-emit.md), ratchets, schemas, tests |
| Historical/reference | Rationale, navigation techniques, or superseded implementation instructions | v1 references and completed landing histories |

A historical document never overrides current code-validated architecture.
Conversely, current architecture does not redefine tsc semantics or turn a
dormant seam into compatibility. When a required architecture row is stale,
validate and update it before writing production code.

**The authoritative execution docs for active work are under
[greenfield/](greenfield/README.md)**. Post-H1 emitter work follows its current
architecture, schedule, and slice-packet route; historical M-stage work uses
the referenced stage step docs. The in-progress H2.5g slice alone follows its
[existing-contract closure route](greenfield/slices/README.md#h25g-legacy-closure-route)
instead of retroactively creating a packet; this exception ends with H2.5g.

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
- [greenfield/emitter-architecture.md](greenfield/emitter-architecture.md):
  the current code-validated emitter pipeline, ownership map, lifecycle state,
  extension seams, and open architectural constraints.

## Active Execution (greenfield)

- [greenfield/](greenfield/README.md): the execution companion to the
  five M/core documents above, plus the entry route for active post-H1 work.
  Its M0-M9 step guides are completed or paused history; its post-H1 route
  points to the current subsystem architecture, schedule, and
  [slice packet](greenfield/slices/README.md).
- [greenfield/completion-convergence-plan.md](greenfield/completion-convergence-plan.md):
  the historical/paused M4-M9 cross-milestone plan — workstreams, landing
  order, and stop conditions for that claim, not the current H2 schedule.
- [greenfield/measurement-integrity.md](greenfield/measurement-integrity.md):
  the A1/A2/A3/A5 + D2 measurement contracts — artifact schemas,
  anchors, and adversarial tests.
- [greenfield/evidence-and-steady-state.md](greenfield/evidence-and-steady-state.md):
  the B1-B4 evidence contracts, required CI topology, and the M9
  steady-state window.
- [greenfield/m9-execution-and-close.md](greenfield/m9-execution-and-close.md):
  the paused-after-M9.1b preflight, bounded fuzzer/domain implementation,
  exact owner-triage and burn-in loop, fingerprint freeze, 14-window
  qualification, and close contract.
- [greenfield/noemit-cli.md](greenfield/noemit-cli.md):
  the completed H0 contract that turns the prepared-program checker into a
  filesystem-hosted `--noEmit` compiler through exact host-owner closure,
  program/config loading, and CLI behavior.
- [greenfield/h1-emit.md](greenfield/h1-emit.md):
  the frozen H1 JavaScript-emit contract and its design/qualification history.
  It preserves predecessor invariants but is not the current broad-emitter
  implementation map.
- [greenfield/post-h1-completion-slices.md](greenfield/post-h1-completion-slices.md):
  the active post-H1 execution schedule and mandatory implementation-ready
  slice gate after H2.5g. It links bounded work to current architecture and
  pinned tsc owners without duplicating either; H2.5g's sole non-retroactive
  exception is confined to the existing-contract closure route above.
- [greenfield/compiler-compatibility-residual.md](greenfield/compiler-compatibility-residual.md):
  an audited surface/owner inventory and historical gap analysis. Recompute
  implementation-state gaps from current code before using it in a slice.
- [greenfield/lsp-and-incremental.md](greenfield/lsp-and-incremental.md):
  the persistent-source and incremental architecture: frozen L0/L1 lineage
  plus the current L2-L5 Program/resolution, Language Service, tsserver, and
  LSP design targets. Revalidate implementation-state claims before an L2+
  slice begins.
- [greenfield/terminal-residue-protocol.md](greenfield/terminal-residue-protocol.md):
  the last-mile parity-sweep protocol — pipeline-layer classification,
  exact shape/provenance proof, and terminal identity-diff gates.
- [greenfield/2xxx-first-order.md](greenfield/2xxx-first-order.md):
  first-order decomposition of the 2XXX band with measured baselines;
  owns the M5/M6-before-sweep phase plan.
- [greenfield/non-2xxx-first-order.md](greenfield/non-2xxx-first-order.md):
  the non-2XXX family map — owner-based decomposition of the bands
  outside 2000-2999; feeds the A5 family rollup and M7 stage gates.
- [greenfield/m7-band-and-owner-strategy.md](greenfield/m7-band-and-owner-strategy.md):
  the M7 pre-implementation survey and virtual-band strategy — exact
  `(code, pass)` family queues, D2 owner tracing, and the 8.1a-f
  checker-grammar split.

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
