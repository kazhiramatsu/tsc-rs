# Post-H1 TypeScript 6.0.3 completion slices

Status: execution schedule approved on 2026-08-08. H0, L0/L1, H1, H2.0a,
H2.0b, and H2.1a are complete. **H2.1b is the next slice.**

This document turns the audited post-H1 residual into branch-sized execution
slices. It owns post-H1 slice IDs, dependency order, and slice-specific
acceptance. The
[compiler compatibility residual](compiler-compatibility-residual.md) owns the
surface inventory, [H1](h1-emit.md) remains the frozen bounded-emit contract,
and the [incremental/LSP design](lsp-and-incremental.md) owns the L2-L5 data and
lifetime architecture. The project-wide
[definition of done](definition-of-done.md) still wins on completion claims.

The target is the pinned TypeScript 6.0.3 compiler and tooling surface. LSP is
listed because it is an intended Rust-native product, but it is not an
upstream TypeScript protocol and never counts toward a TypeScript parity
percentage.

`BLD1`, `W1`, and `API1` are intentionally verbose track IDs. They do not
reuse the existing A1-A5 accepted-state/measurement names or B1-B4 evidence
protocol names.

## 1. Slice rule: keep the existing loop

The existing evidence-led loop remains mandatory. One slice is one
dependency-closed behavior change, one short-lived branch, one merge-commit
PR, and one reviewable before/after result. Every slice:

1. records a trusted base and immutable before observation;
2. pins the exact TypeScript declarations, bodies, hashes, callers, and
   dependency closure it owns;
3. freezes a versioned admitted profile plus explicit `not-run`, unsupported,
   failed, and adjacent-control dispositions;
4. captures positive, adjacent-negative, and applicable fault-injection oracle
   witnesses before implementing behavior;
5. ports the pinned TypeScript control flow and adds `tsc-port`, `tsc-span`,
   and `tsc-hash` ledger entries at port time;
6. compares every applicable observable exactly: diagnostics, text bytes,
   output paths and order, callback metadata, result presence, status, and
   failure boundary;
7. proves repeated-run and legal-worker determinism, plus H0/H1/L1
   non-regression for every shared producer it touches;
8. runs the complete local gate against the recorded trusted base for the
   final runtime candidate and records the result in the PR; and
9. lets ordinary GitHub Actions run only the fixed, unsplit
   `cargo xtask acceptance` boundary sourced from `ts-tests`.

Each scheduled row is an upper bound, not permission for a mega-PR. Its
inventory slice must add suffixes before runtime work when it finds multiple
independent owner SCCs or independently observable protocol/query families.
Runtime rows may be split; they may not be silently coalesced.

Expected output is oracle-produced, never hand-authored or normalized to make
a comparison pass. A structural inventory, recognized option, reserved enum
arm, or successful smoke test is not compatibility. Only an executed exact
observation is a pass.

Documentation-only changes retain the repository's exact Markdown exception.
Evidence, schema, golden, workflow, or generated-artifact changes are not
documentation-only.

Runtime slices that can affect diagnostic accepted state continue to use the
repository `slice-evidence snapshot`/`verify` protocol. Track-specific emit,
state, build, service, or protocol observations supplement that evidence; they
do not replace its FP/loss checks or the complete local gate.

## 2. Additional contracts required after H1

The loop is the same, but later products expose state that one-shot H1 could
not observe. The following additions are mandatory when their surface first
becomes reachable.

| Surface | Additional per-slice evidence |
| --- | --- |
| Persistent Program and resolution reuse | A machine-readable generation trace; fresh-versus-reused equality after every transition; exact parse/bind/check/resolution reuse and invalidation counts; old-generation release; bounded registry/cache state |
| Builder and `.tsbuildinfo` | Deterministic signatures and bytes across restarts; schema/version and incompatible-input behavior; atomic/partial-write and read-failure precedence; affected-file and unchanged-write decisions |
| Watch and Project Service | A virtual-clock event trace; watcher registration/removal, coalescing, timer, missing/failed-lookup, config/package, and close behavior; repeated churn with bounded RSS and handles |
| Cancellation | Named safe points; bounded cancellation latency; no publication of partial Program, builder, query, output, or cache state; an exact subsequent uncancelled result |
| Public API and custom transforms | An explicit Rust-native versus JavaScript-compatible contract; signature inventory; callback presence/absence and lifetime; object identity/mutation; panic/error/exception mapping; thread-safety and semver policy |
| Language Service and tsserver | Exact request/query/event traces over open/edit/close and project transitions; cache invalidation; stale-result suppression; request-ID cancellation; restart and resource evidence |
| LSP | Independent protocol/capability and URI/path/UTF-16 contracts; document-version synchronization; concurrent scheduling; cancellation; diagnostics/workspace edits/progress/errors; protocol tests separate from TypeScript evidence |
| Persistent or external schemas | A named owner, version, canonical encoding, migration/unknown-version policy, corruption behavior, and reproducibility proof |

Every stateful trace names the initial state, input event, host/project/document
version, expected invalidation set, observable outputs/events, live cache and
watch counts, and final released state. Wall-clock sleeps are not oracle
evidence; watch/server tests use a controlled scheduler or virtual clock.

Every runtime slice preserves these frozen boundaries:

- H0 `--noEmit` constructs no emit-only component and writes nothing;
- H1's admitted profile remains byte-, callback-, failure-, and
  resource-exact;
- L1 incremental parsing remains fresh-equivalent and within its approved
  large-edit and reclamation budgets; and
- no old frozen artifact is silently reinterpreted. A broader profile gets a
  new versioned artifact and explicit lineage.

## 3. Dependency waves

The explicit dependency column below is authoritative; numeric IDs group
owners and do not authorize skipping a dependency. Read-only inventories may
run early, but runtime publication follows this order:

| Wave | Track | Runtime dependency | Finish line |
| --- | --- | --- | --- |
| 1 | H2 broad one-shot compiler | Frozen H0/L0/L1/H1 | Full one-shot compiler/config/emit observations for the pinned 6.0.3 suites |
| 2 | L2 shared Program/resolution reuse | H2 complete, so Program/options/file-kind keys are stable | Exact old-Program, registry, resolution-cache, invalidation, release, and fresh-equivalence behavior |
| 3 | BLD1 builder/project references and W1 watch | H2 declarations/maps plus L2 reuse/invalidation | Deterministic builder/build-info/solution state and qualified watch state machines |
| 4 | API1 public API and cancellation | H2, L2, and builder contracts stable | Deliberate public ownership/callback/identity contract rather than exposed internals |
| 5 | L3 Language Service and L4 tsserver | L2 and cancellation; applicable API1 APIs | Upstream service, FourSlash, project-system, and server-protocol observations |
| 6 | L5 Rust-native LSP | Qualified L3 engine | Independent LSP mapping and protocol/resource qualification |
| 7 | M9/release | Shared checker producers and claimed products stable | Confidence freeze, platform/locale/package qualification, reproducible 6.0.3 release |

H2 source-map work and H2 declaration-owner inventory may proceed in parallel
with JavaScript transformer slices. L2/L3/L4/L5 owner and suite inventories may
also proceed read-only. They may not publish a runtime or parity claim before
their dependencies close.

## 4. H2 — broad one-shot compiler

H2 is a sequence of monotonic profile expansions, not one mega-PR. Each
transform slice owns its factory, helpers, resolver/host facts, printer arms,
options, output planning, positive witnesses, and typed adjacent controls.
Helper behavior is never deferred to a generic cleanup tail after its first
transform becomes reachable.

### 4.1 Evidence and transition

| Slice | Scope | Dependencies and close evidence |
| --- | --- | --- |
| H2.0a — complete | Generate the full post-H1 owner/converse inventory, profile-transition manifest, oracle schemas, and exact compiler/conformance/project/transpile candidate dispositions. Freeze the current 94 compiler and 201 conformance option-level one-module-blocker candidates without claiming source compatibility. | H1.6. Zero unresolved/undispositioned owners and cases; old H1 artifacts byte-identical; all rows remain explicit until source analysis and execution. |
| H2.0b — complete | Freeze post-H1 no-emit, H1 emit, L1 edit, binary/startup, output-fault, and resource baselines; add H2 constructor/activity canaries without changing ordinary CI. | H2.0a. Eight alternating approved-runner base/candidate pairs per workload, two exact sink-fault observations, positive H1 controls, zero activity across all 37 H2 runtime slices, and complete local regression gates before H2 runtime changes. |

H2.0a closed on 2026-08-08 without changing the runtime profile or admitting a
new case:

- the [owner/converse inventory](../../../ratchets/h2-owner-inventory.v1.json)
  freezes 50 one-shot compiler owner roots, 46 exact inter-owner references,
  and 14 current Rust converse rows. One root is closed by H1, 22 have an H1
  path plus explicit H2 residual, 27 are deferred, and no owner or Rust row is
  undispositioned;
- the [runner dispositions](../../../ratchets/h2-candidate-dispositions.v1.json)
  retain all 15,642 compiler, conformance, project, and transpile rows. The
  sole executed row is the already-frozen H1 compiler case. The 94 compiler
  and 201 conformance module-only rows are `pending-source-analysis`, while
  every other row names its earliest required H2 slice and remains `not-run`;
- the [profile transition](../../../ratchets/h2-profile-transition.v1.json)
  fixes all 39 H2 rows, keeps H1 as the only runtime profile, records zero H2
  admissions, closes both evidence rows, and records H2.1a as the then-next
  and first runtime candidate; and
- strict contracts now exist for the
  [owner inventory](../../../.github/ci/contracts/h2-owner-inventory.schema.json),
  [candidate dispositions](../../../.github/ci/contracts/h2-candidate-dispositions.schema.json),
  [profile transition](../../../.github/ci/contracts/h2-profile-transition.schema.json),
  [source-reachability oracle](../../../.github/ci/contracts/h2-source-reachability.schema.json),
  and [exact emit observation](../../../.github/ci/contracts/h2-emit-observation.schema.json).

The single producer is
[`crates/oracle/h2-transition.mjs`](../../../crates/oracle/h2-transition.mjs).
`node crates/oracle/h2-transition.mjs --check` regenerates all three manifests
in memory, checks every frozen H1/input hash, and byte-compares the results.
The independent Rust contract repeats the lineage, identity, count, monotonic
slice-order, and zero-undispositioned checks. Neither check treats the 295
option-level candidates as source-compatible or executed.

H2.0b closed on 2026-08-08 without admitting an H2 runtime path. The
[pre-runtime baseline](../../../ratchets/h2-runtime-baseline.v1.json) compares
the exact H2.0a merge with the final H2.0b runtime candidate on the same
approved macOS arm64 runner. Each of the three H0 no-emit workloads, the sole
H1-compatible emit case, and the L1 large-edit fresh/incremental workload has
one cold plus seven warm alternating AB/BA pairs. The same artifact freezes
production/compiler and qualification-observer binary sizes, cold startup,
allocation and RSS ceilings, exact output bytes, and the immutable lineage of
the older H1/L1 performance artifacts. The recorded candidate is
`2894d167b336c6c8039f23f71d31bef223c40ef5`: the largest no-emit warm-median,
warm-p95, and RSS ratios are 1.008095, 1.006398, and 1.008474; H1 emit records
1.014961, 1.009044, and 0.997680; and the largest candidate/base L1 operation
ratio is 1.038047. Compiler and H0 observer size ratios are 1.000166 and
1.000164, while the L1 observer is byte-identical.

The production session now carries one
[`H2ActivityCanary`](../../../crates/emitter/src/activity.rs). Its positive H1
controls prove that the session, plan, resolver, three active transformer
constructors, transform context, printer, JavaScript artifact, and sink paths
are actually observed. Three no-emit executions remain all-zero, both output
failure positions retain exact partial-output behavior, and all 37 reserved
H2 runtime-slice counters remain zero and fail closed before admission. The
strict [baseline schema](../../../.github/ci/contracts/h2-runtime-baseline.schema.json),
generator, and independent Rust contract bind those facts. After H2.1a
admission this H2.0b artifact and its generator are historical: the generator
remains syntax-checked and the Rust contract checks the exact recorded bytes,
but it is no longer reinterpreted against the current runtime tree. Current
ownership is content-addressed by the H2.1a profile below. The historical H1
no-emit and emit generators are handled the same way. Ordinary GitHub Actions remains
the single `gates` job running only `cargo xtask acceptance`; evidence
production and the complete regression gate remain local qualification work.

### 4.2 Module formats at `target=ESNext`

| Slice | Scope | Dependencies and close evidence |
| --- | --- | --- |
| H2.1a — complete | Port `transformImpliedNodeFormatDependentModule`, `getEmitModuleFormatOfFile`, and both ESM/CJS constructor and hook-composition closures. Admit only files proven to select the already-closed ESM path; an incomplete CJS selection fails before the first sink call. | H2.0b. All 295 candidates execute twice: 241 complete observations are exact, 5 output-exact diagnostic controls remain deferred to H2.9, and 49 source-deferred rows fail before the first sink callback. |
| **H2.1b — next** | Close `transformModule` for CommonJS, including prologues, interop, substitutions/notifications, helpers, resolver facts, and printer/output dependencies. | H2.1a. CJS positive and ESM-adjacent controls, multi-file ordering, helper de-duplication, and failure parity are exact. |
| H2.1c | Activate AMD and UMD branches that reuse `transformModule`, including dependency arrays, wrappers, names, and option interactions. | H2.1b. AMD/UMD runner observations are exact; System and bundle-only rows remain controls. |
| H2.1d | Port and qualify `transformSystemModule` and its resolver/helper/output closure. | H2.1c. System-specific execute/setter/export ordering and diagnostics are exact. |
| H2.1e | Close Node16/18/20/Next implied-format behavior, package type, `.mts`/`.cts` output extensions, import attributes, and relative-extension rewriting. | H2.1a-H2.1d plus the required host facts. Mixed-format projects prove per-file dispatch, per-run package-format separation, path casing, and fresh-run behavior after package changes; persistent invalidation remains L2. |

H2.1a closed on 2026-08-08. The
[qualification](../../../ratchets/h2-1a-qualification.v1.json) starts from all
94 compiler and 201 conformance option-level candidates and records source,
module-format, transform-root, parse-depth, comment, diagnostic, and exact
TypeScript observations. Every row is executed twice by Rust as well:

- 241 rows are complete exact admissions, covering 499 reported diagnostics
  and 251 byte-, path-, order-, BOM-, provenance-, result-, exit-, and
  activity-exact writes;
- 5 rows have exact JavaScript output but retain pre-existing H2.0b checker,
  option, or scanner diagnostic differences. They are executed controls, not
  compatibility admissions, and their Rust diagnostic counts and hashes are
  frozen until H2.9;
- 49 rows reach a later source owner and deterministically fail with a typed
  error before the first sink callback; and
- the two-worker control, both ESNext sink-failure positions, the H1 Preserve
  adjacent path, all H0 no-emit constructors, and every other H2 activity
  counter retain their owned boundaries.

The [current runtime profile](../../../ratchets/h2-1a-profile.v1.json) and its
[strict schema](../../../.github/ci/contracts/h2-1a-profile.schema.json)
content-address the production and acceptance inputs, preserve the H2.0a and
H2.0b artifacts byte-for-byte as lineage, mark only H2.1a active, and name
H2.1b as next. Freshness is checked with:

```text
node crates/oracle/h2-1a-qualification.mjs --check
node crates/oracle/h2-1a-profile.mjs --check
```

The ordinary hosted boundary remains only `cargo xtask acceptance`; source
generation, fault/worker controls, schema checks, and the complete H0/H1/L1
regression gate remain local.

### 4.3 TypeScript, source-kind, JSX, and decorator families

| Slice | Scope | Dependencies and close evidence |
| --- | --- | --- |
| H2.2a | Runtime and const enum branches of `transformTypeScript`, including resolver constant values and helper/printer closure. | H2.1b. Enum preservation/inlining/runtime output and adjacent type-only erasure are exact. |
| H2.2b | Namespace/module-declaration runtime transforms and export-container behavior. | H2.2a. Nested/merged/global/module cases and resolver ownership are exact. |
| H2.2c | Parameter properties and remaining class TypeScript syntax reachable at ESNext. | H2.2a. Constructor ordering, modifiers, declarations, and class-field interaction controls are exact. |
| H2.2d | `import =`, `export =`, import elision/value preservation, and module-transform interaction. | H2.1b and H2.2a-H2.2c. Resolver alias/value decisions and module-specific outputs are exact. |
| H2.3a | `.js`/`.mjs`/`.cjs` input and output families, `allowJs`/`checkJs` emit routing, shebang/directive/comment preservation, and extension planning. | H2.1e. Checked and unchecked JavaScript emit uses the production Program without a JS-only AST. |
| H2.3b | Classic JSX/TSX transform, factory/fragment facts, pragmas, namespaces, and `.jsx` output. | H2.3a. Classic React/Preserve/ReactNative observations and UTF-16/source-range controls are exact. |
| H2.3c | Automatic and development JSX runtimes, import source, helper imports, and file-kind interactions. | H2.3b and H2.1b. Runtime import de-duplication/order and diagnostics are exact. |
| H2.3d | JSON source eligibility/copying and `resolveJsonModule` output/path behavior. | H2.3a. Text/BOM/newline, collision, and module-format controls are exact. |
| H2.4a | Legacy decorators plus decorator metadata and referenced-value/check-flag resolver facts. | H2.2c. Evaluation order, metadata helpers, class/member cases, and failure behavior are exact. |
| H2.4b | Standard decorators, `transformClassFields`, `useDefineForClassFields` modes, private/static elements, and their shared helpers. | H2.4a. ESNext and first-downlevel reachability is closed before lowering the target. |

### 4.4 Target ladder

The target profile moves newest to oldest. A row activates only after all
transformers above it in `getScriptTransformers` are closed. Each row is a
separate runtime slice even when a corpus fixture exercises several already
closed transforms.

| Slice | Newly closed owner | Dependencies |
| --- | --- | --- |
| H2.5a | `transformESNext` | H2.4b and H2.1 module closure |
| H2.5b | `transformES2021` | H2.5a |
| H2.5c | `transformES2020` | H2.5b |
| H2.5d | `transformES2019` | H2.5c |
| H2.5e | `transformES2018` | H2.5d |
| H2.5f | `transformES2017` | H2.5e |
| H2.5g | `transformES2016` | H2.5f |
| H2.5h | `transformES2015` plus `transformGenerators`, which activate together | H2.5g |

Every target row closes its exact syntax gates, helper graph, generated-name
collisions, substitution/notification composition, resolver calls, source-map
ranges when that track is available, and the newly admitted upstream runner
observations. Merely accepting the target enum is forbidden.

### 4.5 Maps, declarations, output/config, and broad qualification

| Slice | Scope | Dependencies and close evidence |
| --- | --- | --- |
| H2.6a | Source-map generator/recorder, original/synthetic/source-switch ranges, external single-file `.js.map`, callback metadata, and path planning. | H2.0b and H1 printer hooks. May run in parallel with H2.1; exact map JSON and callback/order observations. |
| H2.6b | Inline maps/sources, `sourceRoot`, `mapRoot`, transformed/multi-source ranges, source-map URL placement, and map failure behavior. | H2.6a plus each transform whose ranges become observable. |
| H2.6c | Close every applicable compiler/conformance/project source-map and source-map-record observation. | H2.6b and all applicable H2.1-H2.5 transforms. No map row is inferred from JavaScript byte parity. |
| H2.7a | Generate declaration/NodeBuilder/resolver/diagnostic owner inventory and port the declaration transform/printer foundation without activating output. | H2.0a. Zero unresolved owners and typed declaration controls. May run read-only/foundation work in parallel. |
| H2.7b | Non-bundle `.d.ts` emit, callback metadata, declaration-only routing, output paths, and exact resolver/NodeBuilder results. | H2.7a and stable Program/emit ownership. |
| H2.7c | Declaration diagnostics and options, including `stripInternal`, `declarationDir`, `isolatedDeclarations`, and forced/targeted declaration axes. | H2.7b. Diagnostic, partial-output, and emitSkipped behavior is exact. |
| H2.7d | JavaScript/declaration bundles, `outFile`, source ordering, and collision/failure behavior; retain prepend/project-reference inputs as typed BLD1 controls. | H2.1c-H2.1d, H2.7b, and applicable map support. |
| H2.7e | Declaration maps and declaration-to-source mapping. | H2.6b and H2.7b-H2.7d. Exact `.d.ts.map` bytes and metadata. |
| H2.8a | Full output directory/root/common-source-directory matrix for the existing JavaScript artifact, overwrite/case collisions, BOM/newline/`removeComments`, emitted-file lists, and filesystem faults. Later artifact slices own their additional path axes. | H2.0b. Exact Memory/Fs sink equivalence and pre-first-write collision/failure behavior. |
| H2.8b | Remaining config/host/System/library-replacement and optional-host-capability behavior for one-shot compilation. | H2.8a and the relevant output tracks. Memory/Fs host equivalence and fallback/diagnostic precedence are exact. |
| H2.8c | `noCheck`, transpile APIs, and their smaller linked-reference/diagnostic/built-in-transform pipelines; retain caller-supplied custom transforms as API1 controls. | Required transforms/maps/declarations. They receive distinct performance and API-route evidence rather than being forced through full checking. |
| H2.8d | Targeted `Program.emit`, ordinary emit-only/declaration-only axes, cancellation, and callback precedence; retain builder-signature runtime as a BLD1 control. | Applicable H2.6a-H2.8c rows. Whole-Program H1 evidence is not substituted for per-file requests. |
| H2.8e | Remaining one-shot CLI modes and observations: help/version/init/show/list, trace/diagnostics/profile, English-profile locale validation/fallback, exits, and terminal/System capabilities. Non-vendored locale catalogs remain REL1 controls. | H2.8b and generated locale/CLI inventories. |
| H2.9 | Broad one-shot compiler qualification. Execute and disposition every applicable compiler, conformance, project, and transpile observation for the approved H2 profile; freeze resource and release-candidate evidence. | All H2 rows. No hidden unsupported success, implicit pass, normalization, or borrowed H1 evidence. Build/watch, public API, services, and LSP remain separate claims. |

If an H2.0 owner graph proves that a listed row contains independent owner
SCCs, split it with letter suffixes before runtime work. If it proves two rows
are inseparable, stop and amend this schedule with the exact owner edges; do
not silently merge them in an implementation PR.

## 5. L2, BLD1, and W1 — shared reuse, build, and watch

### 5.1 L2 shared Program/resolution substrate

| Slice | Scope | Close evidence |
| --- | --- | --- |
| L2.0 | Generate the full registry/old-Program/resolution/cache state-surface inventory and a multi-generation fresh-versus-reused oracle harness. | Exact event and state schemas; no runtime claim. Read-only work may start during H2. |
| L2.1 | Complete `DocumentRegistry` buckets, script-kind/implied-format variants, acquire/update/release counts, overlap/orphan/open policy, statistics, and bounded eviction. | Multi-project overlap and open/edit/close/release traces; identity and RSS bounds. |
| L2.2 | Port `isProgramUptoDate`, all three `StructureIsReused` states, root/options/references/missing/import/lib/package comparisons, and old-Program publication. | Fresh equality after every transition; unchanged Parsed/Bound Arc identity and exact parse/bind/check counts. |
| L2.3 | Add versioned module/type/lib/config/package-json/directory/failed-lookup caches with explicit dependency sets and invalidation. | Positive reuse plus adjacent invalidation for every dependency kind; no lifetime extension of a per-run cache without dependency tracking. |
| L2.4 | Close publication/release ordering, cancellation-safe refresh, stale-candidate discard, service/builder cache interfaces, and long-running qualification. | New Program published before old release, no partial state after cancellation, deterministic generations, bounded memory, H0/H1/H2/L1 regression green. |

### 5.2 BLD1 builder and project references

| Slice | Scope | Close evidence |
| --- | --- | --- |
| BLD1.0 | Generate builder/build-info/project-reference owner, schema, option, and upstream-runner inventories. | Exact converse inventory and restart oracle; no build claim. |
| BLD1.1 | Builder state, semantic/emit affected-file queues, dependency/signature comparison, unchanged-output suppression, and pull/done discipline. | Fresh full build equality, deterministic affected order, cancellation and failure continuation. |
| BLD1.2 | Canonical `.tsbuildinfo` read/write, version/corruption handling, incremental CLI, builder signature/build-info-only output, and restart parity. | Byte-deterministic build info and identical second-process decisions; exact atomic/partial failure behavior. |
| BLD1.3 | Project-reference graph, redirects, cycles, ordering, status/up-to-date checks, clean/dry/force/verbose, timestamp-only work, and solution pull APIs. | Exact solution-builder/project runner observations, collision behavior, partial graph state, and exits. |
| BLD1.4 | Full builder/incremental/solution qualification and resource freeze. | Every admitted build observation exact; long graph/restart determinism and bounded state. |

### 5.3 W1 watch

| Slice | Scope | Close evidence |
| --- | --- | --- |
| W1.0 | Controlled scheduler/clock, watch host, registration inventory, polling/fallback policy, event coalescing, timers, and close. | Deterministic event trace with zero wall-clock sleeps and exact watch cleanup. |
| W1.1 | Single-project watch compilation, root/missing/failed-lookup/type-root/config/package changes, screen/status output, and `afterProgramCreate`. | Exact upstream watch traces; one change causes only its declared invalidation set. |
| W1.2 | Solution build-with-watch and cross-project invalidation/rebuild/timestamp-only behavior. | Exact project/event order across graph edits, errors, cancellation, and recovery. |
| W1.3 | Watch qualification under repeated churn, filesystem faults, cancellation, and platform profiles. | Bounded RSS/watch/timer/cache counts, prompt close, deterministic output, no stale diagnostics or writes. |

## 6. API1 and L3-L5 — APIs and interactive products

### 6.1 API1 public compiler API and custom transforms

| Slice | Scope | Close evidence |
| --- | --- | --- |
| API1.0 | Generate the complete `typescript.d.ts` signature-to-implementation-to-Rust converse inventory and choose explicit Rust-native and optional JavaScript-compatible product profiles. | Every public signature has a disposition; similar names or internal methods do not count. |
| API1.1 | Stabilize public AST/source/factory/printer/Program/TypeChecker/host ownership, errors, cancellation token, thread-safety, and semver contracts. | Signature and behavior witnesses, lifetime compile tests, cancellation safety, no raw internal ID exposure. |
| API1.2 | Custom `before`, `after`, and `afterDeclarations` transforms plus callback/write precedence and clone/original rules. | Exact callback order/presence, mutation/identity, exception/error, repeated emit, and cancellation behavior. |
| API1.3 | If claimed, JavaScript binding/package exports, objects, arrays/maps, `undefined`, exceptions, callbacks, identity/mutation, and entry points. | Direct `typescript` API compatibility suite and package smoke tests; a Rust facade alone is not this claim. |

### 6.2 L3 Language Service

| Slice | Scope | Close evidence |
| --- | --- | --- |
| L3.0 | Complete Language Service/FourSlash owner and query inventory; service host, snapshots, registry integration, modes, cancellation, and multi-generation harness. | Every projected operation has an owner/disposition and fresh/reused oracle trace. |
| L3.1 | Syntactic/semantic/partial-semantic diagnostics, classifications, outlining, indentation, and formatting. | Exact query results/spans after open/edit/close and option/project changes. |
| L3.2 | Definitions, references, rename, navigation, call/type hierarchy, document symbols, and file-rename edits. | Exact cross-file/project results, source mapping, invalidation, and cancellation. |
| L3.3 | Completions, auto-imports, quick info, signature help, module specifiers, package-json and auto-import-provider caches. | Exact entries/details/order and cache invalidation with bounded retained state. |
| L3.4 | Code fixes, refactors, organize imports, paste edits, inlay hints, and workspace edits. | Exact text changes, applicability, fix-all/refactor identity, conflict handling, and cancellation. |
| L3.5 | Per-file emit/source mapping plus complete FourSlash/service-suite and long-running qualification. | No whole-Program substitution; repeated edit/query resource bounds and exact fresh equality. |

### 6.3 L4 tsserver and Project Service

| Slice | Scope | Close evidence |
| --- | --- | --- |
| L4.0 | Generate protocol/project-system/session/typings inventories and a framed request/response/event oracle. | Exact request/event denominator and virtualized time/I/O harness. |
| L4.1 | Configured/inferred/external projects, open-file overlays, project selection, config discovery, and lifecycle. | Exact project graph and open/edit/close events with release bounds. |
| L4.2 | Watches/timers, background and region diagnostics, request-ID cancellation, stale-event suppression, and project reload. | Deterministic event ordering and no partial/stale publication. |
| L4.3 | Protocol commands, preferences, logging, performance, telemetry, session errors, and transport/framing behavior. | Exact server suite observations and fault behavior. |
| L4.4 | Plugins, package installation, automatic type acquisition, typings installer, and security/capability boundaries. | Exact mocked external interactions, cancellation, failure, cache, and cleanup behavior. |
| L4.5 | Full tsserver/project-system qualification, restart/resource/platform freeze, and package entry point. | Every admitted server observation exact; bounded long-running state. |

### 6.4 L5 independent Rust-native LSP adapter

| Slice | Scope | Close evidence |
| --- | --- | --- |
| L5.0 | Freeze supported LSP version/capabilities and the explicit Language Service-to-LSP mapping; build protocol and synchronization harnesses. | Independent capability/request/error manifest; no TypeScript parity borrowing. |
| L5.1 | Initialize/shutdown, URI/path/workspace folders, UTF-16 positions, text-document synchronization, versioning, and configuration changes. | Protocol tests for Unicode, casing, symlinks, stale versions, reconnect, and close. |
| L5.2 | Map navigation, completion, hover/signature, rename, symbols, hierarchy, code actions, formatting, semantic tokens, and inlay hints. | Exact mapped results and capability-dependent absence/presence. |
| L5.3 | Concurrent scheduling, cancellation, progress, diagnostics publication, workspace edits, partial results, and error mapping. | Deterministic race/cancel traces with no stale diagnostics or partial engine state. |
| L5.4 | Protocol, interoperability, latency, memory, churn, fault, and platform qualification. | Independent LSP product claim; these tests remain local/manual under the current `ts-tests`-only hosted-CI policy. |

## 7. Final confidence, platform, and release slices

| Slice | Scope | Close evidence |
| --- | --- | --- |
| M9.1c-M9.7 | Resume the existing M9 execution contract only after shared checker producers are stable. | Production generator, incident/owner closure, burn-in, fingerprint freeze, and 14-window qualification exactly as already specified. |
| REL1.0 | Locale catalogs/fallback, Windows/POSIX path/case/drive/UNC/symlink/permission/timestamp/watch profiles, terminal capabilities, and filesystem failures for every claimed product. | Exact platform/locale matrices; unavailable profiles remain explicit. |
| REL1.1 | `tsc`, compiler-library, tsserver, and optional LSP entry points; stock libs, licenses, package metadata, install/upgrade smoke tests, and reproducible artifacts. | Clean-environment execution, byte-reproducible packages, exact 6.0.3 version and entry behavior. |
| REL1.2 | Final union-of-finish-lines report. | Each claimed compiler/build/API/service/server/LSP row points to its own evidence; no aggregate hides an unimplemented product. |
| VER1.0 | Post-6.0.3 transition, only if separately approved. | New source/lib/locale/package pins, generated data, inventories, oracles, accepted sets, and explicit compatibility transition. It is never a routine dependency bump. |

## 8. Opening and closing a slice

Before implementation:

- confirm every dependency row is closed on `main`;
- create a fresh branch named for exactly one slice;
- record the trusted base and immutable before evidence outside the worktree;
- regenerate the relevant owner/converse and candidate inventories in memory;
- freeze the admitted profile, exact observables, adjacent controls, resource
  budget, and stop conditions; and
- stop for a design amendment if the dependency closure crosses another row.

Before merge:

- every in-slice owner and observation is closed or explicitly dispositioned;
- all new outputs/state transitions match the correct upstream or protocol
  oracle exactly, including failures and cancellation;
- frozen earlier profiles and resource gates remain green;
- final-candidate focused tests and the complete local gate pass against the
  recorded trusted base;
- versioned artifacts, ledgers, status docs, and PR evidence are updated in the
  same slice; and
- the fixed hosted acceptance check succeeds before merge-commit landing.

## 9. Stop and re-slice conditions

Stop and amend this plan before implementation continues if:

- a row needs two unrelated transformer/query/protocol owner groups;
- an admitted branch needs a resolver, host, helper, printer, map, declaration,
  cache, or schema owner assigned to another unfinished row;
- a test needs normalization, sleep-based timing, hand-authored expected
  output, or a process-global leak to pass;
- a cache reuses data without a dependency set and invalidation event;
- cancellation can publish partial state or consume a valid prior snapshot;
- an external schema lacks version/corruption/restart behavior;
- a Rust internal object is exposed as public compatibility without an
  ownership/identity contract;
- FourSlash, tsserver, and LSP results are substituted for one another; or
- ordinary GitHub Actions would need a phase-specific job or non-`ts-tests`
  suite under the current CI policy.

Hard implementation work, a large upstream owner, or a small current corpus
denominator is not permission to broaden a slice, fabricate a dependency, or
count an unexecuted row as complete.
