# Deterministic Checking — Root-Cause & Architecture Design

## Goal / invariant

`checker::check()` must produce **byte-identical output for any `TSRS_JOBS`**
(worker count). The code claims this ("Determinism therefore does not depend on
how files are distributed across workers", `src/checker/mod.rs`), but it does
not hold today.

## Evidence

Full-corpus `--check-batch` at `TSRS_JOBS=1` vs `TSRS_JOBS=16` differs in ≥6
files by *diagnostic set* (plus more by ordering). Representative, each
**deterministic given the worker count**:

| File | J=1 | J≥2 | tsc-correct |
|---|---|---|---|
| `recursiveTypeReferences1.ts` | `2322 @ (72,8)` | `2322 @ (72,11)` | **J=1** (J≥2 is a FP) |
| `externalModules/typeOnly/circular2.ts` | `2456 @ a.ts` only | `2456 @ a.ts + b.ts` | **J≥2** (J=1 misses b.ts) — *fixed* |
| `exportAssignmentCircularModules.ts` | (none) | `2552 @ foo_1.ts` | J≥2 |
| `exportTypeMergedWithExportStarAsNamespace.ts` | (none) | `2702 @ prelude.ts` | J≥2 |

**No single worker count is uniformly correct.** Fixing determinism therefore
cannot mean "pick a worker count"; resolution itself must be made deterministic
*and* complete.

## Root cause

Each worker builds its **own `TypeTable` + resolution caches** (`new_checker`,
`mod.rs`) over a shared immutable `Arc<BindResult>`. Cross-file symbol types are
resolved **redundantly and lazily**, in whatever order each worker happens to
demand them. Two failure families result:

- **P1 — emission suppression.** Diagnostics emitted as a *side effect* of
  declared-type resolution (TS2456 alias cycles, TS2310 base cycles, and
  resolution-time TS2552/2702/2304) are gated by per-worker caches/guards
  (`alias_type_cache`, `res.resolution_failed`, the `reported_*` sets). When one
  worker resolves several related cross-file symbols, the second symbol hits the
  cache and its diagnostic is suppressed; a *different* worker that only touches
  one of them emits it. → the emitted **set** depends on file→worker co-location.
  *(The TS2456 case is already fixed in `aliases.rs` by emitting every alias in
  the cycle, not just the re-entered one — this design generalizes that.)*

- **P2 — type-identity order.** `TypeId`s are assigned in interning order
  (`types.rs`), and **union members are stored sorted by `TypeId`** (load-bearing,
  matches tsc). A worker that processes a different set/order of files interns
  cross-file types in a different order → different `TypeId`s → different union
  member order → different error **elaboration anchor** (the `(72,8)` vs `(72,11)`
  split). At `J=1` the single worker resolves `lib` before `main` (file order);
  at `J≥2` `main`'s worker may resolve `lib` types on demand in a different order.

Both stem from one decision: **independent, lazily-populated per-worker type
tables with no canonical resolution order.**

## Target architecture — two-phase checking

Mirror tsc's structure (resolve once, single identity space), keeping per-file
parallelism for the expensive part.

### Phase R — deterministic global resolution (single-threaded)
- Walk all files in **fixed order** (file index, then declaration order).
- Force-resolve every **declared type** with cross-file reach: exported / global
  (lib) symbols' declared types, member shapes, signatures, enum/alias/class
  instance+static types. (Declared types need no statement-checking context —
  the clean split is tsc's `getTypeOfSymbol` vs `checkExpression`.)
- Emit all declared-type-resolution diagnostics **here, once** (TS2456/2310/…),
  deterministically → fixes **P1**.
- All cross-file `TypeId`s are now assigned in one canonical order → fixes **P2**.
- Freeze the resolved `TypeTable` base + `sym→type` / shape caches into an `Arc`.

### Phase C — parallel per-file statement checking
- Workers share the frozen base (read-only `Arc`) and keep a small **per-worker
  overlay** for *ephemeral* types (expression types, instantiations), with
  `TypeId`s offset above `base.len()`.
- This is the **same two-tier idiom the crate already uses** for checker-minted
  symbols/scopes (`SynthSymbols { base, .. }` + `symbol()`/`scope_at()` in
  `mod.rs`) — apply it to `TypeTable`.
- Declared types read from the frozen base → identical identity across workers.
- Ephemeral types are per-file and never cross files, so their local `TypeId`
  order cannot affect another file's output.

Result: deterministic by construction, **and faster** (each declared type
resolved once, not per worker), with lower memory.

## Staged migration (each stage independently verifiable)

- **Stage 0 — interim (now).** Pin golden capture + `verify.sh` to
  `TSRS_JOBS=1` so the baseline and gate are deterministic; document intra-fixture
  parallelism as unsound-until-Stage-2. (Batch/fixture-level `--jobs` is already
  deterministic and unaffected.)
- **Stage 1 — deterministic resolution order (fixes P2).** Before per-file
  checking, force-resolve all cross-file declared types in fixed order so every
  worker's table assigns identical `TypeId`s. (Can start as per-worker
  replication; no shared-table plumbing yet.) *Verify:* full-corpus byte-identical
  `J=1` vs `J=16`.
- **Stage 2 — complete deterministic emission (fixes P1).** Move declared-type
  cycle/resolution diagnostics to emit during that fixed-order pass, deduped, so
  the set is layout-independent. Generalizes the TS2456 fix. *Verify:* classifier
  vs tsc = 0 new FP, only OK_ADD (recovered FNs).
- **Stage 3 — share the frozen base (perf/memory).** Replace per-worker
  replication with the `Arc`'d base + overlay (the `SynthSymbols` idiom for
  `TypeTable`). Removes redundant resolution. *Verify:* byte-identical vs Stage 2,
  faster wall-clock.

Stages 1+2 deliver full correctness+determinism; Stage 3 is a perf refinement.

## Verification protocol (per stage)

1. **Determinism:** full-corpus `--check-batch` at `TSRS_JOBS=1` vs `16` →
   target **0 differing files** (generalize the `mf` subcommand to the corpus).
2. **Correctness:** `scripts/parallel_classify.py <before> <after>` vs tsc →
   **0 NEW_FP / 0 NEW_FN** (Stage 2 shows OK_ADD for recovered FNs).
3. **Regression:** `cargo test --release` (73 tests) green.
4. Refactor stages that must be behavior-preserving: byte-identical
   `--check-batch` before/after at fixed `TSRS_JOBS=1`.

## Notes

- Reference "correct" point is tsc, not the old `J=1` output (old `J=1` is
  deterministic-but-sometimes-wrong, e.g. the P1 FNs). So verify against the
  oracle, not against the pre-change snapshot.
- The golden baseline is intentionally stale (a standing snapshot); use the
  classifier vs tsc, not raw diff vs golden, to judge correctness.

## Progress log & findings (2026-07)

### Stage 1 — deterministic lib seed — DONE (committed)
Each parallel worker checks the lib file first, so lib globals intern in one
order (same TypeIds) in every worker. Full conformance corpus, `TSRS_JOBS=1`
vs `16`: **byte-level nondeterministic files 22 → 2**. Removed real
production false positives — `recursiveTypeReferences1` TS2322 anchor now
`(72,8)` at all worker counts (was `(72,11)` at `J≥2`, a FP), and the spurious
TS2304/TS2552 the parallel path emitted on `exportAssignImportedIdentifier` /
`exportAssignmentCircularModules` (oracle-confirmed absent in tsc) are gone.
`jobs==1` output unchanged; classifier 0 NEW_FP / 0 NEW_FN; 73 tests pass.

### Stage B/Phase R "resolve all declared types" — ATTEMPTED, REVERTED
A seed that eagerly resolves **every** module-scope declared symbol's type in
fixed order (Phase R) was implemented and reverted. It **broke 5 tests**
(`computed_object_literal_setters_report_implicit_any`,
`named_function_expression_truthiness`, `non_strict_addition_keeps_2365`,
`direct_nullish_values_report_18050`, `accessor_parameter_count`) and shifted
many diagnostics (6133/2339/7022/…), and it *worsened* determinism (traded the
2 residual files for 4 different ones).

**Root finding — the Phase R premise is unsound for value symbols.** A value
symbol's type (`const x = expr`, functions, etc.) is resolved *in context*
during statement checking (initializer inference, usage tracking, flow). A
context-free "resolve everything first" pass produces different results
(implicit-any, truthiness, `+`/2365, nullish/18050) and corrupts expression
diagnostics. Only **type-space** symbols (interface/alias/enum) have a
context-free declared type suitable for a pre-pass; namespaces and value
symbols do not.

**Consequence.** Deterministic TypeId ordering requires a fixed-order type
resolution, but value-symbol resolution is inherently in-context — so parallel
intra-fixture checking and deterministic TypeId order are fundamentally at
odds. The shared-frozen-base architecture (which needs a deterministic
resolve-all Phase R) cannot be made sound without also resolving value symbols
in-context, which defeats the pre-pass.

### Resolution — IMPLEMENTED (default `TSRS_JOBS=1`)
**Status: done.** `check()` now defaults `TSRS_JOBS` to 1, so the default is
serial intra-fixture and fully deterministic. The default full-corpus output is
byte-identical to the serial (`TSRS_JOBS=1`) path and to the Stage-1 result; 73
tests pass; the standing golden was refreshed (parallel → serial). Parallel
intra-fixture (`TSRS_JOBS>1`) remains an opt-in that is not yet deterministic
(and its per-worker lib seed from Stage 1 still reduces, but does not eliminate,
its nondeterminism).

Rationale (as decided):
For this workload the throughput comes from **batch/fixture-level** parallelism
(`--check-batch --jobs`, deterministic); intra-fixture parallelism (`TSRS_JOBS`)
adds negligible benefit (full corpus checks in ~5 s). So the pragmatic,
evidence-based fix is:

> **Default `TSRS_JOBS=1` (serial intra-fixture) → full determinism.** Keep
> batch-level parallelism. Leave parallel intra-fixture as an opt-in that is
> documented as not-yet-deterministic. ~2-line change, low risk.

Note this changes the standing golden (parallel → serial) once, so it needs a
baseline refresh. Alternatives: keep Stage 1 (91%, parallel default) and treat
the residual as known limitations; or a type-space-only seed (fixes type-space
determinism, leaves namespace/value residual such as `TwoInternalModules`).
