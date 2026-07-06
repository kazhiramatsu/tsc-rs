# Greenfield: if tsc-rs were rebuilt from scratch

Purpose: (1) the north-star architecture against which retrofits are
judged, (2) an honest answer to "should the core mirror tsc, or is
there a better architecture?", (3) the test-harness design we would
build on day 1 knowing what we know now. **Recommendation up front: do
NOT rebuild.** §6 lists what to adopt incrementally without a rewrite.

## 1. The architecture question, answered

**For a parity-targeted checker, tsc's semantic architecture IS the
correct architecture.** This is an empirical conclusion from this
project, not deference:

- Every subsystem where tsrs invented its own approximation eventually
  had to be torn out and replaced with a tsc mirror: the fact-stack
  flow engine (replaced by a tsc-shaped FlowNode graph, Tier-2), the
  numeric-operand comparison rule (replaced by tsc's comparable
  relation, operator sweep), adopt-the-RHS assignment narrowing
  (replaced by getTypeAtFlowAssignment, relation-core 1), first-decl
  method typing (replaced by tsc's merged overload symbols). The
  rework cost dominated the project's timeline.
- Every subsystem that mirrored tsc's decision structure from the
  start converged fast and monotonically (unused-locals grouping
  engine, checkUnusedClassMembers, definite assignment on the CFG).
- The reason is structural: the success metric is byte-exact
  diagnostics, and tsc's architecture is OBSERVABLE in its output —
  through resolution order, type identity, literal freshness,
  elaboration chains, cache policy (Ternary/Maybe), and error
  suppression sites. Any architectural innovation in an observable
  layer is a permanent divergence liability that mining will
  eventually charge you for.

**Where better-than-tsc architecture is legitimate**: layers the
output cannot observe — memory model, id-based arenas vs GC pointers,
explicit side tables vs monkey-patched node properties, determinism
discipline, and the entire test harness. That is exactly where a Rust
rebuild should spend its freedom.

**If the goal were NOT parity** (a fast independent TS checker), a
genuinely better architecture exists: demand-driven incremental
computation (salsa-style), eager normalized type representations,
constraint-based inference, file-parallel checking. Each is REJECTED
here explicitly because each breaks parity: eager normalization
changes type identity and display; parallel checking reorders
resolution side-effects (this project's determinism saga proved the
diagnostics move); constraint solving diverges from tsc's imperative
inference order precisely in the underspecified cases where tsc's
order IS the de-facto spec. A "modern mode" could be layered onto a
parity core later; the reverse migration is impossible.

## 2. Prime directive: port, don't reinvent

Every semantic decision is a PORT of a named tsc function.

- Module layout mirrors checker.ts's section order; function names
  keep tsc's names in snake_case (`get_type_at_flow_assignment`, not
  `assigned_type`) so cross-codebase grep is 1:1. (Today's tsrs
  requires a mental mapping table; a rebuild makes grep the map.)
- **Flag bit-compatibility**: TypeFlags / SymbolFlags / ObjectFlags /
  NodeFlags / CheckMode use tsc's EXACT bit values, so masks in ported
  code are copied verbatim and inline comments in `_tsc.js` remain
  directly usable. (tsrs's homegrown flag sets forced re-derivation at
  every port site.)
- **Port ledger** (new invention, unobservable → allowed): a generated
  file mapping every ported function → (tsc name, vendored version,
  content hash of the tsc source snippet it mirrors). Re-vendoring a
  newer tsc diffs the ledger and emits the exact list of ports whose
  upstream changed. This converts "tsc upgraded, what broke?" from
  archaeology into a checklist. Ledger entries are doc-comments parsed
  by a build script:
  `/// tsc-port: getAssignmentReducedType @6.0.3 sha256:ab12…`

## 3. Core data model (the things tsrs got structurally wrong)

- **Type identity = allocation identity, interning only where tsc
  interns.** tsc dedupes literal types per value, and unions/
  intersections by member-id-list keys — mirror exactly those maps
  (`literalTypes`, `unionTypes`, `intersectionTypes` with
  getTypeListId-style keys) and NOTHING else. Two `{}` type literals
  written in two places are two type objects, as in tsc. This makes
  the anon-identity class of divergence (unknownControlFlow fx2/fx4,
  2403 mapped-identity family) unrepresentable-by-construction.
- **Freshness is a paired-object property** (`fresh_type` /
  `regular_type` fields on literal types), never inferred from cache
  arrival order. Widening happens only at ported rule sites
  (getWidenedLiteralType, getInferredType's widenLiteralTypes, …).
- **Three links tables, explicit**: `NodeLinks`, `SymbolLinks`,
  `TypeLinks` — id-indexed side tables holding every memo tsc hangs
  off its objects (resolvedType, resolvedSignature, resolvedSymbol,
  outerTypeParameters, …). One memo policy, one invalidation story,
  instead of tsrs's ~dozen ad-hoc caches with individually discovered
  pollution rules (expr_type_cache/quiet, sig_ret_cache order effects,
  relation cache modes).
- **Relations**: one `check_type_related_to` engine, `Relation` enum ×
  5, per-relation caches keyed by tsc's `getRelationKey` (which
  includes alias/instantiation context!), `Ternary` results with the
  Maybe-stack cache policy. Ported wholesale on day 1 — this project's
  single-bool engine is the root of an entire stall-playbook section.
- **Symbols**: tsc's merge model (flags-driven mergeability, decl
  lists, members/exports separation, getMergedSymbol chain). The class
  -method-overload orphaning bug this week was exactly a hand-rolled
  divergence from that model.
- **AST**: parser produces tsc-shaped recovery: missing-node
  placeholders, `ThisNodeHasError`/`ThisNodeOrAnySubNodesHasError`
  node flags from day 1 (the parse-error gate is then free), and
  recovery decisions ported from parser.ts (the non-LHS `=` rule is
  one instance of a general class).
- **Flow**: bind-time FlowNode graph (tsrs's Tier-2 design is already
  correct; keep it).
- **Diagnostics**: message table generated from tsc's
  diagnosticMessages.json (tsrs already does this — keep), plus
  chain/relatedInformation construction ported so strict-mode
  comparison is achievable without a later retrofit.

Mechanical layer (Rust-native, all unobservable): arena allocation
with u32 ids for Node/Symbol/Type/Signature; no interior mutability
beyond the links tables; iteration order deterministic everywhere
(IndexMap or sorted iteration — HashMap order leaks were a real bug
class here); checker strictly single-threaded per program, parallelism
only ACROSS programs (the determinism saga's conclusion, adopted as a
design axiom rather than a discovered fix).

## 4. Test harness design (day-1, not accreted)

The current harness grew organically and its weak points cost real
time (ephemeral /tmp goldens, dual fixture parsers, classifier
blindness to category). The rebuild's harness:

### 4.1 Single-source fixture expansion

ONE implementation (Rust library) parses fixture directives and
expands multi-file/option matrices; it emits a canonical
`program.json` (files + options). BOTH the checker harness and the
oracle driver consume `program.json`. The BOM/double-blind class of
bug (two parsers drifting) becomes impossible. The oracle driver is a
thin node script: read program.json → run tsc API → emit
`diagnostics.json` with FULL structure (code, category, span, message
chain, relatedInformation, file) — strict-ready from day 1.

### 4.2 Goldens in the repository

Per-fixture diagnostic snapshots live in-repo (sharded files,
compressed; or per-file content hashes with bulk storage in git-lfs).
/tmp is a cache, never the source of truth. `git bisect` then works
over conformance history — impossible today.

### 4.3 The gate, mechanized

CI job per PR: run corpus, classify vs golden + oracle, FAIL on any
NEW_FP; auto-render the NEW_FN/OK_ADD/OK_RM summary into the PR
description. The "0 NEW_FP" discipline stops being convention and
becomes infrastructure. Local `verify` = the same code path.

### 4.4 Comparison tiers with ratchets

- T0: (file, code, line, col) set equality — today's metric.
- T1: + category (error/suggestion band).
- T2: + full span (length), message TEXT.
- T3: + message chains + relatedInformation.
- T4: byte-exact CLI output (incl. ordering and formatting).

A `ratchet.toml` in-repo records the current % per tier; CI fails if
any tier regresses. Workstreams declare which tier they target. (Today
only T0 exists and T1+ facts — like the suggestion band — leak into
T0 debugging as "artifacts"; see knowledge-base §1–2.)

### 4.5 Invariant (metamorphic) suite

The invariants discovered ad hoc become permanent harness commands:

- **Prefix determinism**: check(file[..k statements]) agrees with
  check(file) on the shared prefix's diagnostics, for sampled k —
  kills the check-order-sensitivity class (stall-playbook §2.2).
- **Jobs independence**: byte-identical output across
  `--jobs 1..16` (today's `verify.sh mf`, promoted).
- **Encoding**: BOM/no-BOM, CRLF/LF equivalence.
- **Idempotence**: checking the same program twice in one process
  yields identical output (cache-pollution detector — would have
  caught the quiet/expr_type_cache class instantly).

### 4.6 Differential fuzzing loop

Grammar-based TS generator (biased toward the constructs the corpus
under-covers: recovery-heavy inputs, deep generics, template
patterns) → run both engines → any T0 difference is auto-minimized
(delta-debugging reducer over statements, then expressions) and filed
as `fuzz/NNNN.ts` into a triage directory with both outputs. The
conformance corpus is a fixed 5,907-file sample; the fuzzer is how the
blind spots (knowledge-base §7's "corpus can't see it" caveat) get
coverage.

### 4.7 Coverage accounting

The port ledger (§2) cross-links each ported function to the fixtures
that exercise it (instrument the checker with a per-function hit
counter in a coverage build). "Which ports have zero corpus coverage"
becomes a query — those are exactly where fuzzing effort aims.

## 5. Versioning against upstream tsc

- Vendor a pinned tsc (as today); the pin is part of the conformance
  claim ("byte-exact vs tsc 6.0.3").
- Upgrades: re-vendor → port-ledger diff → work the checklist →
  re-baseline goldens in one commit. Never track upstream
  continuously; jump pin-to-pin.

## 6. What to adopt WITHOUT rebuilding (ordered, all retrofittable)

The greenfield above is a direction, not a plan. Retrofit order for
the current codebase, cheapest-first, each independently valuable:

1. **Goldens in-repo** (§4.2) + gate CI (§4.3) — pure infra, no
   checker changes. Kills the /tmp fragility permanently.
2. **Idempotence + prefix-determinism harness commands** (§4.5) —
   small scripts; they detect two known bug classes automatically.
3. **Single-source fixture expansion** (§4.1) — refactor
   parallel_classify.py to consume the Rust harness's expansion (add
   `tsrs --expand-fixture` emitting program.json). Prereq for lib-gap
   Stage 2 anyway (lib layering needs both sides switching together).
4. **Port ledger, incrementally** — add `/// tsc-port:` doc-comments
   to functions AS THEY ARE TOUCHED; a script collects them. No
   big-bang annotation pass.
5. **Ternary × 5 relations** (stall-playbook §2.1) — the first
   OBSERVABLE-layer retrofit, do when relation mining stalls.
6. **Freshness de-ordering** (stall-playbook §2.2) via getInferredType
   fidelity (relation-core-2-steps STAGE I is the first slice).
7. **Declaration-identity types** (stall-playbook §2.3) — last, only
   with attribution evidence; it is the closest thing to a partial
   rebuild and §3's identity model is its specification.
8. **Fuzzing loop** (§4.6) — once the mapped workstreams are done and
   the corpus tail thins, this becomes the main FP/FN discovery
   engine.

A full rewrite would only be justified if items 5–7 TOGETHER were
assessed as more expensive in-place than a fresh port around them —
re-evaluate only after all three have attribution evidence, and even
then prefer strangler-style replacement (new relation engine module
consumed by the old checker) over a green repo.
