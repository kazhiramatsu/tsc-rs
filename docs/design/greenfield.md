# Greenfield: the from-scratch design (v2, implementation-grade)

Purpose: (1) the north-star architecture against which retrofits are
judged, (2) the answer to "mirror tsc or do better?", (3) a design
detailed enough that a rebuild could START from this document alone:
crate layout, core types, wire schemas, harness, and a milestone plan
with acceptance gates. **Recommendation unchanged: do NOT rebuild
today** — §10 maps which pieces the current repo adopts incrementally,
and §11 states the only conditions under which a rebuild wins.

---

## 1. The architecture verdict (condensed; evidence in v1 history)

For byte-exact diagnostic parity, tsc's semantic architecture is the
design, because the parity metric makes that architecture OBSERVABLE
(resolution order, type identity, literal freshness, message chains,
cache policy, suppression sites). Empirical record from this repo:
every invented approximation was eventually torn out for a tsc mirror
(fact stack → CFG; numeric comparison rule → comparable relation;
RHS-adoption narrowing → getTypeAtFlowAssignment; first-decl method
typing → merged overload symbols); every day-1 mirror converged
monotonically. Architectural freedom is spent ONLY in unobservable
layers: memory model, side tables, determinism discipline, tooling,
harness. Genuinely different architectures (salsa incrementality,
eager normalization, constraint inference, file-parallel checking) are
each rejected because each one is observable through diagnostics.

## 2. Workspace layout

```
tsrs2/
  Cargo.toml                 # workspace
  crates/
    syntax/        # scanner + parser + AST arena + recovery (no deps on sema)
    binder/        # symbols, scopes, flow-graph construction
    types/         # type objects, arenas, interning maps, flags
    checker/       # the ported checker (largest crate, checker.ts-ordered)
    diags/         # message tables (generated), chains, span utils
    harness/       # fixture expansion, program.json, batch runner
    oracle/        # node driver + rust client for diagnostics.json
    conformance/   # classifier, tiers, ratchets, goldens
    fuzz/          # generator + reducer + triage
    xtask/         # ledger, codegen (flags/messages/syntaxkind), CI entry
  vendor/typescript-6.0.3/   # pinned tsc (source of ports + oracle impl)
  goldens/                   # in-repo golden shards (see §7.3)
  ratchet.toml
```

Dependency direction: `syntax ← binder ← checker`, `types ← checker`,
everything ← `diags`. `harness`/`conformance`/`fuzz` depend on the
public `check_program` API only. No crate depends on `oracle` at
runtime (test-only).

## 3. Generated foundations (xtask codegen; never hand-written)

All generated from `vendor/typescript-*/`, committed, and re-generated
on re-vendor. Bit/value compatibility is what lets `_tsc.js` masks and
switch arms be ported verbatim.

- `types::flags`: `TypeFlags`, `ObjectFlags`, `SymbolFlags`,
  `NodeFlags`, `ModifierFlags`, `CheckFlags`, `InferencePriority`,
  `Ternary`, `CheckMode`, `SignatureFlags`, `ElementFlags` — bitflags
  with tsc's EXACT numeric values, extracted by parsing the const-enum
  tables in the vendored source. Each constant carries a doc-comment
  with the tsc name.
- `syntax::kind`: `SyntaxKind` as a `#[repr(u16)]` enum with tsc's
  numbering (extracted the same way). Parser ports then keep tsc's
  kind-based dispatch literally.
- `diags::gen`: the full message table from `diagnosticMessages.json`
  (code, category, text template, reportsUnnecessary/Deprecated,
  elidedInCompatabilityPyramid) — the current repo already proves this
  works; keep the mechanism.
- `ledger.toml`: see §8.

## 4. Core data model (concrete)

### 4.1 Ids and arenas

```rust
// every id is a u32 newtype; Default = INVALID sentinel (u32::MAX)
pub struct NodeId(u32);   pub struct SymbolId(u32);
pub struct TypeId(u32);   pub struct SignatureId(u32);
pub struct FlowId(u32);   pub struct ScopeId(u32);

pub struct Arena<Id, T> { items: Vec<T>, _m: PhantomData<Id> } // push/get only
```

No cross-run stability is promised for any id (lesson: node_key).
Serialization always goes through spans/paths, never ids.

### 4.2 Types: allocation identity + tsc's exact interning surface

```rust
pub struct Type {
    pub flags: TypeFlags,            // tsc-bit-compatible
    pub object_flags: ObjectFlags,   // incl. FreshLiteral, Instantiated, Reference…
    pub symbol: SymbolId,            // INVALID for intrinsics
    pub alias: Option<(SymbolId, Box<[TypeId]>)>,   // aliasSymbol/typeArguments
    pub data: TypeData,
}
pub enum TypeData {
    Intrinsic { name: &'static str },                 // any, unknown, string…
    Literal   { value: LiteralValue, fresh_of: Option<TypeId>,
                regular: TypeId, wide: TypeId },      // freshness is STRUCTURE
    Union     { members: Box<[TypeId]>, origin: Option<TypeId> },
    Intersection { members: Box<[TypeId]> },
    ObjectAnon   { decl: Option<NodeId>, shape: OnceCell<ShapeId> },  // decl = IDENTITY
    ObjectReference { target: SymbolId, args: Box<[TypeId]> },
    Interface { decl_symbol: SymbolId },
    TypeParameter { symbol: SymbolId },
    IndexedAccess { obj: TypeId, index: TypeId },
    Conditional { decl: NodeId, mapper: MapperId },
    Mapped      { decl: NodeId, mapper: MapperId },
    TemplateLiteral { texts: Box<[Box<str>]>, types: Box<[TypeId]> },
    StringMapping   { case: IntrinsicCase, inner: TypeId },   // day-1, not a retrofit
    // …substitution, unique symbol, enum member, etc.
}
```

Interning maps exist ONLY where tsc has them, with tsc's keys:

```rust
pub struct TypeTables {
    arena: Arena<TypeId, Type>,
    literal_types: HashMap<LiteralKey, TypeId>,          // per-value literals
    union_types: HashMap<Box<[TypeId]>, TypeId>,         // getTypeListId keyed
    intersection_types: HashMap<(Box<[TypeId]>, AliasKey), TypeId>,
    string_mappings: HashMap<(IntrinsicCase, TypeId), TypeId>,
    // NO general structural interning: two written `{}` literals differ
}
```

This makes the anon-identity divergence class (fx2/fx4, the 2403
mapped-identity family) unrepresentable, and it makes tsc's
`typeMembershipMap` dedup semantics fall out for free.

> The foundational MACHINERY (lazy type computation + the
> `pushTypeResolution` cycle stack, the eager/deferred check-driver
> ordering, contextual typing, `getUnionType`/`getIntersectionType`
> construction, widening, `instantiateType`/TypeMapper, member access)
> has its own porting notes in
> [checker-foundations.md](checker-foundations.md). This section (§4)
> gives the data-model shapes those functions read/write; that doc gives
> the functions.

### 4.3 Links tables (the memo policy, in one place)

```rust
pub struct NodeLinks   { resolved_type: Option<TypeId>, resolved_signature: Option<SignatureId>,
                         resolved_symbol: Option<SymbolId>, flags: NodeCheckFlags, … }
pub struct SymbolLinks { declared_type: Option<TypeId>, type_of_symbol: Option<TypeId>,
                         type_parameters: Option<Box<[TypeId]>>, target: Option<SymbolId>,
                         mapper: Option<MapperId>, … }
pub struct TypeLinks   { apparent: Option<TypeId>, base_constraint: Option<TypeId>,
                         resolved_shape: Option<ShapeId>, … }
```

Rules (enforced by construction, reviewed at port time):
- A links slot is written ONCE (OnceCell semantics); "resolving"
  in-progress states are explicit enum values, mirroring tsc's
  `resolvingSignature`/`resolvingDefaultType` sentinels, so cycle
  handling is ported rather than improvised.
- SPECULATIVE checking never writes links: the checker has ONE
  `speculation_depth: u32`; all links writes assert `depth == 0` or go
  through an explicit transaction that is dropped on rollback. (This
  single rule replaces the discovered-by-bug `fresolve.quiet` /
  `expr_type_cache` pollution family.)

### 4.4 Symbols

tsc's merge model verbatim: `flags: SymbolFlags` (bit-compatible),
`declarations: Vec<NodeId>`, `value_declaration: Option<NodeId>`,
`members`/`exports` as ordered tables (IndexMap), `parent`,
`merged_into: Option<SymbolId>` with `get_merged_symbol` chasing.
Binder merge rules ported from binder.ts's `declareSymbol`
(excludes/includes masks — the class-overload orphaning bug becomes
impossible because the merge table IS tsc's).

### 4.5 AST + recovery

Nodes carry `flags: NodeFlags` including `ThisNodeHasError` /
`ThisNodeOrAnySubNodesHasError`, set by the parser exactly where
parser.ts sets them; `Missing` placeholder nodes exist as real nodes
with zero-width spans. Every parser production is a port with a ledger
entry; recovery behavior (what token sets abort which lists —
`abortParsingListOrMoveToNextToken`, `isListTerminator`) is ported,
not approximated. This buys: parse-error gating for free, spans that
match tsc's `getErrorSpanForNode`, and the non-LHS `=` class never
existing.

### 4.6 Flow

Keep the current repo's proven design (it already mirrors tsc):
FlowNodes built at bind time, `FlowFlags` bit-compatible, resolver =
`getFlowTypeOfReference` port. The one upgrade: `getTypeAtFlow*` arms
live in ONE module ordered as in checker.ts, each with a ledger entry.

> The load-bearing algorithms (`isTypeRelatedTo`/`recursiveTypeRelatedTo`,
> `getInferredType`/`getCovariantInference`, `resolveCall`/`chooseOverload`,
> and control-flow analysis `getFlowTypeOfReference`/`getTypeAtFlowNode`/
> `narrowType`/`isReachableFlowNode`) have implementation-grade porting
> notes — Rust-shaped skeletons mirroring the real control flow, with line
> anchors — in [checker-key-functions.md](checker-key-functions.md).
> §4.6 here gives the flow data-model shape; that doc §4 gives the flow
> algorithms.

### 4.7 Relations engine (day-1 shape)

```rust
pub enum Relation { Identity, Subtype, StrictSubtype, Assignable, Comparable }
pub enum Ternary { False = 0, Unknown = 1, Maybe = 3, True = -1i8 as isize } // tsc values

pub struct RelationCaches { per_relation: [HashMap<RelationKey, RelationResult>; 5] }
// RelationKey = tsc getRelationKey: source/target ids + alias context +
// intersection-state; RelationResult = Succeeded|Failed|…Reported flags
```

`check_type_related_to` ported with: maybe-stack (`maybeKeys`,
re-check on Maybe), expanding-type depth limits
(`isDeeplyNestedType`, recursion identity, depth 5 per side),
`IntersectionState`, error-chain capture (§6). Public bool API wraps
it. All five relations exist from day 1 even if Subtype call sites
arrive later — the engine cost is identical and the retrofit cost in
the current repo is a whole stall-playbook section.

### 4.8 Instantiation

`TypeMapper` as tsc's closed set of mapper kinds (simple, array,
deferred, merged, composite) with an arena and `MapperId`;
`instantiate_type` ported including `instantiationDepth`/`Count`
guards and the instantiation caches on SymbolLinks/Signature. No
ad-hoc `HashMap<SymbolId, TypeId>` mappers (the current repo's Mapper)
— composite mappers are where subtle divergence hides.

## 5. Checker organization

- One module per checker.ts region, IN FILE ORDER (grammar checks,
  types-from-nodes, relations, inference, narrowing, expression
  checks, statement checks, unused, declaration checks). A CONTRIBUTING
  table maps module → checker.ts line range of the vendored pin.
- Function granularity = tsc function granularity. If tsc has
  `getTypeOfVariableOrParameterOrProperty` → we have
  `get_type_of_variable_or_parameter_or_property`. No coalescing "for
  elegance" — coalesced functions are what made the current repo's
  ports drift.
- Every diagnostic emission site names its tsc counterpart in the
  ledger comment. `error_at` requires a `&'static DiagnosticMessage`
  from the generated table — no ad-hoc strings, so T2 text parity is
  structural.
- Suppression/dedup surfaces (tsc's implicit ones) are centralized:
  errorType-silences-cascade, `reportedError` node marks, once-per-
  symbol reports — one module, ported rules only.

## 6. Diagnostics pipeline

```rust
pub struct Diagnostic {
    file: Option<FileId>, start: u32, length: u32,
    message: MessageChain,            // code/category/args + children
    related: Vec<RelatedInfo>,
    category_override: Option<Category>,  // suggestion-band moves (unused etc.)
}
```

- Chains built by the ported `chainDiagnosticMessages` discipline;
  elaboration (property chains, signature mismatch reporting) ported
  from relation-error reporting.
- Ordering & dedup: one final sort identical to tsc's
  (`compareDiagnostics`: file, start, length, code, message text) plus
  tsc's dedup of identical adjacent diagnostics — this is T4's floor.
- Suggestion band: `getSuggestionDiagnostics` semantics ported,
  INCLUDING the emit interaction: greenfield oracle driver does NOT
  call emit (unlike the current diag_oracle.js), and the checker
  implements the emit-marking rules directly (the current repo's
  `module_body_instance_state` mirror) — removing the knowledge-base
  §1 artifact class entirely by making both sides emit-free.

## 7. Test harness (day-1, the part the current repo aches for)

### 7.1 Single-source fixture expansion

`harness` crate owns directive parsing (`// @option:`, multi-file
`// @filename:`, BOM, matrix options) and emits canonical
**program.json**:

```json
{ "schema": 1,
  "cwd": "/",
  "options": { "strict": true, "target": "es2015", "module": null, … },
  "libs": ["lib.es5.d.ts", "lib.es2015.core.d.ts"],
  "files": [ { "name": "main.ts", "textB64": "…" } ],
  "matrixKey": "target=es2015"       // one program.json per matrix point
}
```

- `tsrs2 expand <fixture.ts> --out-dir …` produces N program.json
  files (matrix expansion INCLUDED — the current repo bails 6054).
- The oracle driver (`oracle/driver.mjs`) consumes program.json — the
  Rust side never re-implements expansion, node never parses
  directives. The BOM/double-blind class is structurally dead.
- Libs: layered lib set VERBATIM from the vendored tsc (no curated
  single-file lib — the entire lib-gap axis never exists; the corpus
  cost of full libs is performance, paid once in §9).

### 7.2 Oracle protocol

`driver.mjs`: createProgram from program.json (in-memory host) →
collect syntactic + semantic + suggestion diagnostics (NO emit) →
**diagnostics.json**:

```json
{ "schema": 1, "tscVersion": "6.0.3",
  "diags": [ { "file": "main.ts", "start": 123, "length": 5,
               "code": 2322, "category": "error",
               "chain": { "text": "...", "code": 2322, "next": [ … ] },
               "related": [ {"file": "...", "start": 1, "length": 2,
                              "code": 2728, "text": "..."} ] } ] }
```

Rust `oracle` crate: typed client + a persistent node PROCESS POOL
(stdin/stdout JSONL, N workers) — no per-fixture process spawn (the
current 15-minute serial runs become ~1 minute) and no OS-level
process storms (the incident that motivated the no-parallel rule).

### 7.3 Goldens in-repo

`goldens/{fixture-relpath}.json.zst` — per fixture, containing BOTH
sides at T3 fidelity + the T4 output hash:

```json
{ "fixture": "conformance/…/foo.ts", "matrixKey": "…",
  "tsrs": [ …diag records… ], "oracle": [ … ],
  "tsrsCliHash": "…", "oracleCliHash": "…" }
```

Committed. `git bisect run cargo xtask conformance --files <list>`
works across history. Size control: zstd shards by directory,
regenerate-don't-diff workflow, and CI verifies shard freshness.

### 7.4 Comparison tiers + ratchet

- T0 (file, code, line, col) set equality — bring-up metric.
- T1 + category (error vs suggestion).
- T2 + full span + top message text.
- T3 + chains + relatedInformation.
- T4 byte-exact CLI output.

`ratchet.toml`:

```toml
[t0] rate = 0.6235  allowed_regression = 0.0
[t1] rate = 0.0     allowed_regression = 0.0   # activates when measured
```

CI: any tier's measured rate < recorded rate ⇒ fail; improvements
auto-bump via a bot commit. Workstreams DECLARE their tier.

### 7.5 Classifier gate

Ported from the current scripts but running on diagnostics.json pairs:
NEW_FP hard-fails CI; NEW_FN, OK_ADD, OK_RM rendered into the PR body.
LIBCODES-style ignore lists DO NOT EXIST (full libs remove the need) —
every code counts, which also retires the "2304 partially invisible"
caveat.

### 7.6 Invariant suite (metamorphic; each is a bug-class detector)

```
xtask invariants --suite all
  prefix-determinism   # check(file[..k]) prefix-agrees with check(file)
  idempotence          # same program twice in-process, byte-identical
  jobs-independence    # jobs 1..16 byte-identical
  encodings            # BOM/no-BOM, CRLF/LF agree modulo spans
  matrix-independence  # matrix points don't leak state across programs
```

Run on a 200-fixture rotating sample per PR; full corpus nightly.

### 7.7 Differential fuzzer

- Generator: grammar-based, weighted toward historically divergent
  constructs (error recovery, generics depth, template literals,
  overloads, narrowing chains); seeds mutated from corpus fixtures.
- Executor: both engines via program.json; any T0 diff → reducer.
- Reducer: statement-level ddmin, then expression-hole shrinking
  (replace subtrees with `0`/`""`/`x`), fixpoint; emits minimal repro.
- Triage: dedup by signature (sorted one-sided (code, msg-head)
  pairs); new signatures filed as `fuzz/pending/NNNN.ts` with both
  outputs; a resolved repro graduates into the conformance corpus.
- Budget: nightly hours, not per-PR.

### 7.8 Unit pins

Checker-internal unit tests are allowed ONLY as ports of tsc's own
compiler tests or oracle-probed pins (expected strings from the
oracle, never hand-derived) — the current repo's rule, kept.

## 8. Port ledger (tooling detail)

Doc-comment format, enforced by `xtask ledger check`:

```rust
/// tsc-port: getAssignmentReducedType @6.0.3
/// tsc-hash: 6b0e…   (sha256 of the exact vendored source slice)
/// tsc-span: checker.ts:69675-69699
fn get_assignment_reduced_type(…)
```

- `xtask ledger check`: every pub fn in checker/binder/syntax hot
  modules has an entry; hashes match the vendored source (drift ⇒
  re-vendor happened ⇒ listed as STALE).
- `xtask ledger coverage`: coverage-build hit counts per ported fn
  joined against the corpus → "ports with zero corpus coverage" feeds
  the fuzzer's weight table.
- Re-vendor procedure: swap vendor/ pin → `ledger check` emits the
  changed-port checklist → work it → re-baseline goldens in ONE
  commit.

## 9. Performance & determinism budget

- Checker single-threaded per program; parallelism ACROSS programs
  only (rayon over fixtures). Axiom, not option.
- Full-libs cost: mitigate with a lib-check cache — libs are identical
  across programs; bind+check them once per (lib-set, options-subset)
  and snapshot the symbol/type arenas for reuse (tsc does the moral
  equivalent with its own lib reuse). Budget: full corpus T0 compare
  < 60s on 8 cores including oracle reuse of cached runs.
- All iteration deterministic (IndexMap or sort-before-iterate;
  clippy lint deny on std HashMap iteration in checker crates).

## 10. Strangler adoption map (greenfield pieces → current repo)

Adoptable WITHOUT rebuild, in value order:
1. §7.2 oracle process pool + §7.1 `--expand-fixture` emitting
   program.json (kills dual expansion; 10× faster oracle runs).
2. §7.3 in-repo goldens + §7.5 CI gate.
3. §7.6 invariant suite (idempotence + prefix-determinism first).
4. §8 ledger comments, added as code is touched.
5. §4.7 relations engine as a NEW module consumed by the old checker
   (stall-playbook §2.1's R-stages are exactly this strangler).
6. §6 suggestion-band emit-free oracle (retires knowledge-base §1
   artifact) — requires implementing emit-marking rules first.
7. §7.7 fuzzer (post-workstream backlog).

## 11. When would a rebuild actually win?

Only if stall-playbook attribution shows the THREE structural items
(Ternary relations §2.1, order-dependence §2.2, declaration identity
§2.3) are all binding simultaneously AND their in-place migrations are
each assessed as more expensive than a fresh port of their layer. Even
then: strangle layer-by-layer inside this repo (checker crates are
separable) rather than a green repo — the corpus, harness, goldens,
and ledger carry over unchanged, and the conformance ratchet protects
every step of the migration exactly as it protected the sweeps.

## 12. Milestone plan for a true greenfield (if §11 ever triggers)

Each milestone has a MEASURABLE acceptance gate; no milestone starts
before the previous gate is green. Estimates assume one strong agent
with review.

| M | Deliverable | Acceptance gate |
|---|-------------|-----------------|
| M0 | xtask codegen: flags/SyntaxKind/messages from vendor; harness expand + oracle driver + pool; goldens format; T0 classifier | oracle side of goldens generated for full corpus; invariant runner skeleton green on empty engine |
| M1 | scanner + parser + recovery + syntactic diags | T0 parity of SYNTACTIC diagnostics vs oracle ≥ 99.5% corpus-wide; prefix-determinism green |
| M2 | binder (symbols/merging/flow graph), no checker | crash-free bind of corpus; symbol-table spot-audit vs tsc on 50 fixtures (script comparing exported symbol names via oracle `program.getTypeChecker().getSymbolsInScope`) |
| M3 | types crate + relations engine + intrinsics + literals/unions | ported relation unit-pins green (oracle-probed pairs, ~200 cases incl. Ternary/Maybe edge pins) |
| M4 | checker: expressions + statements MINUS narrowing/inference (declared types only) | T0 ≥ 35% (calibration: current repo pre-Tier-2 was ~48% with more subsystems) |
| M5 | flow narrowing (resolver port) + checkNonNull family | T0 ≥ 50%; jobs/idempotence invariants green |
| M6 | inference (inferTypes/getInferredType full port) + generics instantiation caches | T0 ≥ 58% |
| M7 | unused/grammar/suggestion band (emit-free rules) | T0 ≥ 63% = parity with current repo; T1 measured and ratcheted |
| M8 | long tail by classifier mining (this playbook's normal loop) | T0 ratchet climbs; T2/T3 activated |
| M9 | fuzzer + coverage ledger in CI | new-signature rate < 1/night before declaring steady state |

The single most load-bearing scheduling fact, learned here: **M1's
parser-with-tsc-recovery is the foundation everything else prices
in.** The current repo did M1 approximately and has been paying the
parse-error-gate + recovery-profile tax ever since. In a rebuild it is
the first thing done exactly.
