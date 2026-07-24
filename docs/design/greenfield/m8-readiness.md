# M8 scope and readiness contract

This page is the executable M8-entry contract. It states the two metric
views and the ten machine rows. Supporting formats are defined once in
[measurement-integrity.md](measurement-integrity.md) and
[evidence-and-steady-state.md](evidence-and-steady-state.md). The final
end state remains [definition-of-done.md](definition-of-done.md).

## Two views of one corpus

Every conformance run reports both:

1. **All corpus** — the versioned oracle-input universe. It owns the set
   ratchet, top-FN visibility, and absolute `FP=0`.
2. **Supported scope** — the same universe after subtracting only exact,
   reviewed oracle diagnostic occurrences from `tsrs2/m8-scope.json`.
   M8's T1-T4 target uses this denominator.

Scope identity is A2 schema 2:

```text
fixture + matrix_key + pass + file + start + length + code + category
+ message-chain hash + related-information hash + occurrence
```

Line/column are verified review fields, not keys. Syntactic records are
never excludable. There are no fixture, directory, code, or glob
exclusions. The only reasons are `host-resolution`, `jsdoc-semantics`,
and `emit-dependent`, each with non-empty evidence.

The [A2 contract](measurement-integrity.md#3-a2--exact-scope-state)
defines canonical hashes, the draft `2xxx` band pin used by §4 row 9 of
the convergence plan, the two-step global freeze at M7 close, and A1
tombstones for resolved exclusions. Schema 1, a stale anchor, a
post-freeze addition/edit, or an unresolved duplicate-bucket proof fails.
Conformance writes both views to mismatch JSON.

## Declaration converse

Run:

```sh
cargo xtask codegen band-inventory --by-function --band all
cargo xtask codegen band-inventory --by-function --band all --check
cargo xtask port-plan --declaration <d2:id>
cargo xtask port-plan --diagnostic-json <exact-identity.json>
```

`m8-emitter-inventory.json` pins the vendor and uses D2 exact declaration
identities for named and anonymous functions. Names are aliases;
`tsc-span` plus `tsc-hash` selects a port. Lexical calls resolve exactly;
property calls may over-approximate but keep candidates separate.

`m8-emitter-dispositions.json` pins the generated inventory and classifies
every closure identity as ported, deferred, or not applicable with
evidence. Schema-1 name-collapsed files are draft migration input only.
The earlier D2a exact planning report and `port-plan` view are also
report-only: neither an unanchored D2a inventory nor an incomplete
disposition set can satisfy readiness. This section consumes only the
frozen D2b inventory/dispositions and their reviewed snapshot anchor.
The complete contract is
[D2 declaration identity](measurement-integrity.md#6-d2--declaration-identity-and-closure).

## Produced evidence

`m8-evidence.json` configures producers; it does not contain editable
readiness claims. Runtime, fuzz-smoke, performance, and RSS artifacts are
generated under `target/` and consumed in the same workspace. Readiness
recomputes summaries and requires current input fingerprints. Missing,
dirty, stale, malformed, or hand-authored evidence fails.

Runtime coverage is declaration-level, the fuzzer runs every generated
case against tsrs and the pinned oracle with reducer/dedupe smoke, and
wall/RSS observations must pass on an approved reference runner. See the
[evidence contract](evidence-and-steady-state.md). M9 strengthens the
already-running fuzzer; it does not introduce it.

### Recorded tsc 6.0.3 crash deviations (differential classification)

The pinned oracle can crash where the port reports. A crashed oracle
run has no classifiable output, so these shapes cannot carry corpus
goldens, and the shadow/fuzzer differential must classify an
oracle-side crash matching a recorded row as that deviation — the
port's report stands; it is not a mismatch. Recorded rows
(M4-review B29, both re-executed 2026-07-19):

1. `for await` over a **sync** iterable whose yield type is a
   non-promise thenable (callable `then`, non-callable callback
   param).
2. `yield*` of such a sync iterable inside an async generator. (The
   non-delegated `yield thenable;` does NOT crash — tsc reports 1321
   normally; only the async-from-sync synthesis path is affected.)

   Shared root: `getAsyncFromSyncIterationTypes` (84113-84128) passes
   an errorNode to `getAwaitedType` WITHOUT a diagnosticMessage, and
   `getAwaitedTypeNoAlias` (82435) hits
   `Debug.assertIsDefined(diagnosticMessage)` (82486) on the thenable
   arm — Debug Failure. The port's synthesis
   (`get_async_from_sync_iteration_types`, checker/src/iterate.rs)
   passes the 1320 pair explicitly, so it reports where tsc dies.
   Under the corpus lib regime both trigger shapes currently contain
   upstream as conditional-type (`Awaited`/`BuiltinIteratorReturn`
   machinery) M8-stub partials, so the deviation is corpus-inert
   today and goes observable when M8 ports conditional types.

3. The static-block `strictPropertyInitialization` probe under
   `strictNullChecks: false` — `getOptionalType` Debug assert; the
   port keeps the pre-swap declared-type reduction for that regime
   only. Recorded at m5-flow-steps.md (post-close review).

4. Tuple inference through a variadic/rest (or rest/variadic) middle
   pair whose empty middle slice meets a rest element that CONTAINS A
   TYPE VARIABLE — e.g.
   `declare function f<T extends [any, any], U>(x: [...T, ...U[]]): [T, U];
   f(["a"] as [string]);` (recorded 2026-07-20, M6 7.2d;
   probe-tuple.mjs f6). `inferFromObjectTypes` (69121/69130) passes
   `getElementTypeOfSliceOfTupleType`'s undefined straight into
   `inferFromTypes`, which dereferences `source.aliasSymbol` (68657)
   — TypeError — unless the target's `couldContainTypeVariables` early
   return fires first. The port's `infer_from_middle_slice`
   (checker/src/inference.rs) skips the harmless shape exactly like
   the early return and reports Unsupported (recovery-class escape)
   where tsc dies. Same-shape calls whose rest element is variable-free
   (probe f2/f3) do not crash in either implementation and infer
   identically.

## Machine gate

```sh
cargo xtask m8 readiness
cargo xtask m8 readiness --require-ready
```

The first command reports. The second closes M7 and opens M8 only when
all ten rows are green:

1. M7 conformance: `T0 >= 63%`, `FP=0`, configured exact T1 ratchet;
2. live T1-T3 shadow metrics;
3. globally identity-anchored, frozen, fresh exact scope;
4. zero undispositioned Rust checker functions;
5. fresh schema-2 all-band declaration inventory;
6. frozen and complete dependency dispositions;
7. current declaration-level runtime coverage;
8. current differential-fuzzer smoke with reducer and dedupe;
9. current wall/RSS observations within ceilings on an approved runner;
10. every M7-owned A5 family has all canaries and supported FN=0.

Row 10 is recomputed from current full conformance after exact A2 scope,
then grouped by the frozen A5 map. A1 is its monotonic guard, not a
substitute for current supported grading. The aggregate 63% in row 1
cannot hide a red family; `--require-ready` names it.

Required regression coverage includes: row 1 green with one M7 family
red; a wholly excluded bucket; a duplicate bucket with one excluded
neighbor; a frozen owner moved to a later milestone; and stale
conformance/scope/evidence fingerprints. Each must fail the responsible
row rather than change the denominator silently.

The report is `target/m8/readiness.json`. It describes remaining work; it
does not claim the current branch is ready.

## Escape end state

`Unsupported` recovery may remain separately ratcheted through M7, but
it is not a Done exception. Final completion requires `sites=0` and an
empty `escapes.toml`.
