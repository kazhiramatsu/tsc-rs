# H1: filesystem-hosted JavaScript emit execution contract

Status: H1.0a now has the generated, report-only
[`h1-owner-inventory.v1.json`](../../../ratchets/h1-owner-inventory.v1.json)
owner-graph draft plus the frozen
[`h1-emit-profile.v1.json`](../../../ratchets/h1-emit-profile.v1.json) and
callback-level [`h1-emit-oracle.v1.json`](../../../ratchets/h1-emit-oracle.v1.json),
and the current-Rust
[`h1-rust-omissions.v1.json`](../../../ratchets/h1-rust-omissions.v1.json)
baseline, and the complete 22-file upstream `transpile` source tree is now
content-addressed in the additive suite pin v2 without fabricating expansion
or runner results. Additive suite pin v3 now also binds the complete
6,568-file FourSlash tree identity and vendors the exact 38-file batch-emit
witness projection, again with zero expansion or execution rows.
The shared L0/L1 prerequisite and its evidence/CI-contract freeze are complete.
The graph's unresolved dynamic calls and conservative property dispatch still
need review, and the corpus classification portion of H1.0a remains open. No
H1 runtime implementation or compatibility claim exists until the complete
H1.0 inventory and post-L0/L1 no-emit performance baseline described below are
frozen.
H0 remains the released, frozen single-project `--noEmit` profile. M9 remains a
separate paused batch-diagnostics qualification track.

The persistent-source `L0` foundation and incremental-parser `L1` proof in
[the LSP/incremental design](lsp-and-incremental.md) are workspace
prerequisites for H1 runtime implementation. H1 owner/oracle design may
continue in parallel, but H1.1 must not freeze checker, arena, or source-file
lifetimes before L0/L1 and the resulting H0 requalification are complete.
This prerequisite does not add incremental behavior to the H1 compatibility
claim.

The audited implementation gap and the roadmap from bounded H1 through the
broader TypeScript 6.0.3 compiler/tooling surfaces are maintained in
[the compiler compatibility residual](compiler-compatibility-residual.md).
That inventory is a companion to this execution contract, not an expansion
of H1 scope.

Compatibility target: the vendored TypeScript 6.0.3 compiler only.

## 1. Purpose and outcome

H1 adds bounded JavaScript output to the production compiler without changing
the behavior or cost model of the completed H0 `--noEmit` command. It ports
the TypeScript emit pipeline through the same evidence-led method that closed
the scanner, parser, binder, checker, program, host, and CLI work:

```text
command/config/filesystem
  -> PreparedProgram
  -> ephemeral/persistent DocumentStore
  -> ProgramSnapshot (parsed + bound documents)
  -> fresh CheckerSession
  -> checker-owned EmitResolver
  -> ordered transformer selection
  -> transformNodes
  -> printer
  -> output planning
  -> OutputSink
  -> diagnostics, emitted-file observations, and exit status
```

H1 is not an independently designed TypeScript-to-JavaScript formatter. The
vendored compiler's phase boundaries, option gates, transformer order,
resolver queries, printer behavior, output paths, write order, diagnostics,
and exit behavior are the specification. Rust ownership may differ only where
the difference is not observable.

The completed H0 path is a hard product boundary because editors, linters,
build systems, and CI commonly invoke `tsc --noEmit`. H1 may make an emitting
invocation do more work; it may not make a no-emit invocation initialize or
pay for the emitter.

## 2. Prime directive and source authority

Port, never improvise. Before implementing an H1 function:

1. relocate the exact declaration in
   `vendor/typescript-6.0.3/lib/_tsc.js`;
2. record its declaration span and body hash;
3. identify its complete in-profile dependency closure;
4. capture an emitting oracle witness and an adjacent non-emitting control;
5. port the function in TypeScript control-flow order; and
6. attach the ordinary `tsc-port`, `tsc-span`, and `tsc-hash` ledger comment.

`_tsc.js` is the production owner for H1. Vendored `typescript.js` and the
pinned upstream runner sources are additional authorities only when H1 builds
a Language Service/FourSlash cross-control or reproduces runner expansion and
observation rules. Such a control must not pull retained service-owned
`DocumentRegistry`, old-Program reuse, or Language Service state into the
production H1 path.

An H1 branch may be deferred only with a constructibility proof for the frozen
profile and a canary that fails when the branch becomes reachable. A missing
printer, resolver, transform, or output-planning branch is a typed unsupported
result, never a partially emitted success.

"Same architecture" means the following observable boundaries match tsc:

- diagnostic gates run before emission in the same order;
- `noEmit`/`noEmitOnError` choose the same early-exit behavior;
- the checker supplies the same semantic resolver answers to transforms;
- script transformers are selected and applied in the same order;
- transformation lexical environments, helper requests, substitutions, and
  notifications obey the same lifetimes;
- the printer sees the same transformed tree and handler sequence;
- output paths, file order, bytes, BOM choice, diagnostics, and exit status
  match the oracle.

The following Rust representations may differ:

- arena IDs instead of JavaScript object references;
- session-owned side tables instead of mutable `node.emitNode` fields;
- a separate synthetic-node arena instead of GC-owned node objects;
- a typed `OutputSink` instead of an untyped `writeFile` callback; and
- a one-shot borrowing session instead of a long-lived self-referential
  `Program`/`TypeChecker` object graph.

Those substitutions are valid only while the oracle-visible result and the
lifetime rules in this contract remain exact.

## 3. Audited entry state

The pre-L0 workspace audit starts from the completed H0 profile:

- the production `CompilerHost` is read-only;
- `PreparedProgram` owns ordered source, library, option, path, package, and
  resolution facts;
- `ProgramSession::run(self)` performs one no-emit check and returns only
  diagnostic buckets;
- parser, binder, checker, and their borrowed arenas are dropped inside that
  one-shot run;
- no production transformer, JavaScript printer, output planner, or output
  sink exists; and
- checker branches used only by runtime transformation or declaration emit
  were deliberately elided or empty-gated where they were unobservable to
  the frozen batch-diagnostics surface.

The frozen H0 macOS arm64 explicit-root workload measured:

| Observation | H0 value | Frozen absolute ceiling |
| --- | ---: | ---: |
| Cold wall time | 0.70 s | 2.0 s |
| Warm wall time | 0.14 s | 1.0 s |
| Cold maximum RSS | 101,072,896 bytes | 268,435,456 bytes |
| Warm maximum RSS | 97,648,640 bytes | 268,435,456 bytes |

These values come from `ratchets/h0-qualification.v1.json`. The ceilings are
release safety bounds, not permission for H1 to consume their unused margin.
H1.0 freezes a tighter same-runner before/after regression policy before any
emit implementation lands.

## 4. Scope and explicit non-scope

H1 includes:

- one filesystem-hosted TypeScript project per invocation;
- the existing H0 root, config, library, and module-resolution profile;
- JavaScript output for a frozen, versioned option and syntax profile;
- the checker-to-emitter resolver boundary required by that profile;
- TypeScript syntax transformation, printer behavior, output-path selection,
  file-write ordering, and emit diagnostics for that profile;
- an in-memory output sink used by every oracle and differential test;
- a filesystem output sink used only by the production emitting command;
- exact `emitSkipped`, emitted-file, diagnostic, and exit-status behavior;
  and
- continued exact H0 `--noEmit` behavior and resource bounds.

H1 does not include:

- declaration (`.d.ts`) or declaration-map emission;
- JavaScript source maps or inline source maps;
- build-info emission, `--incremental`, `--build`, project-reference
  orchestration, or solution builds;
- watch operation or filesystem event scheduling;
- incremental parsing or `SourceFile`/program reuse as an H1 product
  behavior (the workspace-level L0/L1 prerequisite is still required);
- LSP, language-service queries, or a public `TypeChecker` API;
- plugins or custom transformers;
- `outFile`/bundle emission unless a later H1 inventory amendment explicitly
  admits and qualifies it; or
- TypeScript versions other than 6.0.3.

Source maps, declaration emit, build/watch, incremental parsing, LSP, and the
public checker API retain separate goals, or separately reviewed later H1
profiles. An output option outside the frozen H1 profile fails before writing
any file.

"Does not include" is an implementation and compatibility boundary, not
permission to erase a load-bearing tsc seam. Before H1.1 freezes Rust types,
H1.0 records the dormant declaration, source-map, bundle, targeted-emit, and
build-info axes described below. The bootstrap profile cannot execute those
axes, but the JavaScript-only implementation must not choose an intermediate
representation or callback contract that makes them require a replacement
emitter later.

## 5. Production API and lifetime boundaries

### 5.1 Preserve the H0 entry

After L0, the existing no-emit API remains the ordinary production adapter:

```rust
impl ProgramSession {
    pub fn run(self) -> Result<NoEmitOutcome, DriverError>;
}
```

H1 adds an emitting entry without routing `run` through an emitter facade:

```rust
impl ProgramSession {
    pub fn emit(
        self,
        sink: &mut dyn OutputSink,
    ) -> Result<EmitOutcome, DriverError>;
}
```

Names may be refined during H1.0, but the type-level separation is
load-bearing. A no-emit call does not receive an output sink, transformer
factory, printer factory, or emit configuration object, so it cannot invoke
them accidentally.

`ProgramSession` owns or constructs a `ProgramSnapshot` containing shared
immutable `ParsedDocument`/`BoundDocument` handles. The emitting entry creates
a fresh scoped `CheckerSession` borrowing that snapshot and keeps checker
links and the checker-owned resolver alive through transform and print. It
does not first collapse the checker into `CheckResult`, reconstruct semantic
state, or parse/bind the program a second time. The H0 adapter uses an
ephemeral document store and drops it with the invocation.

### 5.2 Read host and write sink stay separate

`CompilerHost` remains read-only. H1 must not add `write_file` to that trait.
The emitter receives a separate sink:

```rust
pub trait OutputSink {
    fn write(
        &mut self,
        artifact: EmitArtifact,
    ) -> Result<EmitWriteDisposition, EmitIoError>;
}

pub struct EmitArtifact {
    path: PathBuf,
    bytes: Vec<u8>,
    write_byte_order_mark: bool,
    kind: EmitArtifactKind,
    source_files: Option<Vec<PathBuf>>,
    metadata: EmitWriteMetadata,
}
```

The concrete field visibility and names remain an H1.0 decision. Callers do
not construct artifacts by struct literal: accessors and typed constructors
keep later artifact kinds and callback metadata additive rather than a public
layout break.

`bytes` is the UTF-8 encoding of the callback text without a BOM;
`write_byte_order_mark` records the separate tsc callback decision. The
materialized filesystem bytes prepend the UTF-8 BOM exactly when that flag is
true. Keeping both observations prevents a sink from hiding a wrong callback
contract behind coincidentally matching final bytes.

The canonical artifact identity is path, exact bytes, BOM decision, kind,
ordered source-provenance presence/content, and normalized write-callback
metadata presence/content. It never contains `NodeId`, `SymbolId`, `TypeId`,
addresses, or process-local cache keys. H1's text metadata carries the
transform diagnostics and optional source-map URL position that tsc passes to
`writeFile`; that position is a generated UTF-16 text offset, not a byte
index. A future build-info variant remains typed and versioned rather than an
unstructured property bag. Presence is observable: tsc passes no
`sourceFiles` for build info rather than an empty source array.

`MemoryOutputSink` is the acceptance authority. `FsOutputSink` applies the
same ordered artifacts to the filesystem and reports typed write failures.
The emitter never calls `std::fs` directly. Both H1 sinks return only
`EmitWriteDisposition::Written`. The return type is nevertheless
load-bearing: tsc's builder write callback can suppress an unchanged
declaration write and feeds that decision back into `emittedFiles`. A future
builder adapter may return a typed skipped-unchanged disposition without
changing the emitter/sink ABI; H1 does not implement that policy.

A sink `Err` is consumed at the ported `writeFile` boundary. It becomes tsc's
`Could not write file ...` diagnostic and emission continues or stops exactly
where the vendored callback does; it is not automatically promoted to the
outer `DriverError`. `FsOutputSink` owns parent-directory creation, retry, and
the final stable error message. Driver errors remain for unsupported requests,
host/program failures before emission, cancellation when later admitted, and
internal contract violations. Multi-output tests inject an error at each
write index so partial files, later callbacks, diagnostics, `emitSkipped`, and
exit status cannot drift together accidentally.

### 5.3 Outcome boundary

The emitting command returns a typed, non-publicly-constructible outcome at
least equivalent to:

```rust
pub struct EmitOutcome {
    diagnostics: Vec<Diagnostic>,
    emit_skipped: bool,
    emitted_files: Option<Vec<PathBuf>>,
    source_maps: Option<Vec<SourceMapObservation>>,
}
```

Read-only accessors expose the admitted observations. The output sink owns
written artifact bytes. The outcome owns the observable driver result. H1
must preserve TypeScript's distinction between diagnostics with outputs
generated, diagnostics with outputs skipped, and success. Its optional
collections also distinguish tsc's absent observation from a present empty
list: `emittedFiles` exists only when `listEmittedFiles` is active, and
`sourceMaps` exists only when a map product is requested. H1 leaves the
source-map slot absent, but retaining it now avoids changing the result shape
when maps are separately admitted.

Callback order and `emitted_files` order are separate observations. They must
not share one vector or be derived from each other: tsc can invoke `writeFile`
for an external map before its JavaScript/declaration text while reporting the
text path before the map path in `emittedFiles`.

### 5.4 Dormant emit axes fixed before implementation

The internal H1 request and output plan retain these independent axes even
though the bootstrap profile admits only the first value in each list:

- selection: whole Program, or one target `SourceFile`;
- emitted root: one `SourceFile`, or a `Bundle`;
- product: JavaScript, JavaScript map, declaration, declaration map, or build
  info; and
- mode: ordinary script emit, declaration-only/builder-signature emit, or
  build-info-only emit.

Rust uses enums and optional typed slots rather than copying tsc's optional
parameters and booleans literally. The internal output-unit plan combines
`getOutputPathsFor`'s distinct `js`, `js_map`, `declaration`, and
`declaration_map` slots with `forEachEmittedFile`'s separate `build_info`
slot. In the first H1 profile every slot except `js` is absent, the selection
is whole Program, and the root is a single source file. No inactive slot is
allocated or inspected on the H0 no-emit path.

Targeted emit, bundles, and non-JavaScript products are not public H1 APIs.
Retaining their discriminants prevents the initial implementation from baking
"one Program always produces one kind of file per source" into transformer,
printer, output-order, or sink code. Reaching any inactive discriminant in H1
is a typed unsupported result before the first sink call.

### 5.5 Crate dependency and test ownership

H1.0 is expected to add `crates/emitter` as the acyclic owner of emit protocol
types, the transform context and synthetic factory, printer/writer, output
planning, `MemoryOutputSink`, and the internal `EmitResolver`/emit-host traits.
The dependency direction is load-bearing:

```text
emitter  -> syntax + types + diagnostics + program
checker  -> emitter protocols + its existing dependencies
compiler -> checker + emitter + program + host
```

`crates/emitter` never depends on `crates/checker`; the checker implements the
consumer-owned resolver trait for its program-borrowing session. This avoids a
checker/emitter cycle and prevents the compiler driver from reconstructing
semantic facts after checking. If H1.0 evidence requires a different crate
name, the same dependency direction still applies and the amendment is
recorded before H1.1.

There is no new top-level `tests/` directory. Broad integration contracts keep
the repository's one-aggregator-per-crate pattern:

- `crates/emitter/tests/contracts.rs` owns factory, transform lifecycle,
  printer, output-plan, artifact, and memory-sink modules;
- `crates/compiler/tests/contracts.rs` adds no-emit fast-path, emit-session,
  CLI, sink-failure, and H1-qualification modules;
- `crates/harness/tests/contracts.rs` adds transpile expansion and FourSlash
  emit-inventory contracts; and
- oracle/conformance unit tests own record encoding, exact callback
  comparison, suite-pin, and ratchet behavior.

Individual integration modules live below each crate's `tests/integration/`
and are included by its `contracts.rs`, rather than becoming dozens of Cargo
test executables. Fixture bytes, oracle observations, and qualification data
remain under the pinned suite, `pins/`, `vendor/`, or `ratchets/` owners; they
are not hand-authored expected files under a Rust test directory.

### 5.6 Read-only emit-host projection

The emitter consumes a borrowing, read-only `EmitHost` projection generated
from the frozen owner graph. It exposes only reached Program facts such as
compiler options, ordered source lookup, current/common source directory,
canonical-name policy, source eligibility/blocking, redirects, and implied
module format. It neither owns the filesystem nor embeds a write callback;
all writes still cross `OutputSink`.

Future-only `getBuildInfo`, hashing, project-reference, bundle, and targeted
emit host queries remain typed inactive capabilities. They are not added to
the production `CompilerHost`, and an H0 call never constructs the projection.
This keeps source-map path calculation and module transforms from reaching
back into the CLI while preserving a place for later builder evidence.

## 6. Mandatory zero-cost `--noEmit` path

### 6.1 Early-exit rule

Effective `noEmit=true` selects the existing H0 execution before any H1
component is constructed:

```text
parse/config/program preparation
  -> existing H0 ProgramSession::run
  -> existing diagnostic render/exit path
```

It must not follow this shape:

```text
construct emitter -> construct transforms/printer -> discover noEmit -> skip
```

This mirrors tsc's `handleNoEmitOptions` early return before
`getEmitResolver`, `getTransformers`, and `emitFiles`.

For every invocation in the frozen H0/H1 single-project profile with effective
`noEmit=true`, all of the following counts are exactly zero:

- emit-resolver construction;
- script-transformer selection and initialization;
- transformation-context and synthetic-node-arena construction;
- emit-only node/symbol side-table allocation;
- printer and text-writer construction;
- H1 JavaScript output-path and source-map planning;
- emit artifact creation; and
- output-sink writes.

The profile qualification matters. Upstream tsc may write `.tsbuildinfo` for
an incremental/build invocation even when ordinary JavaScript/declaration
output is disabled. H0 and H1 admit no incremental or build mode, so their
zero-write rule remains absolute. A later builder track must define and
qualify its build-info exception explicitly; it cannot silently weaken this
contract or route H0 `--noEmit` through an emitter.

Focused contracts use factories that panic if any forbidden constructor or
sink is reached from `ProgramSession::run` or the CLI `--noEmit` route. This
structural canary complements timing measurements; a fast accidental call is
still a contract violation.

### 6.2 No unconditional data-model tax

H1 must not enlarge every parsed `Node`, persistent bind record, or checker
link with emit-only storage merely because emission is compiled into the
binary.

Parsed trees use the L0 persistent-source representation, which H0 reaches
through its one-shot adapter. Emit-only observations live in lazily allocated,
session-owned sparse or indexed side tables. A dense transform-flag table is
permitted only in an emitting session and must be measured against a sparse
alternative. Synthetic nodes and their transform flags live in the emitting
session's transform arena. H1 stores no transform/printer state in a cached
`ParsedDocument` or `BoundDocument`.

The H0 adapter uses L0's serial sealed-tail identity allocation and direct
session-owned document slots. It does not perform the persistent store's
post-parse relocation pass or construct synchronized registry machinery just
to satisfy the shared `ProgramSnapshot` interface.

Emit-only checker bookkeeping is selected at the checker entry or at a
bounded semantic owner, not through an unpredictable branch on every generic
node or type operation. When two entry points share an implementation, the
no-emit specialization must be demonstrably free of emit allocations and hot
loop work.

Large generated printer tables remain immutable static data and require no
runtime initialization on the no-emit path. Binary-size and cold-start page
fault effects are measured rather than assumed away.

### 6.3 Performance evidence and ratchet

H1.0 creates a versioned no-emit performance artifact before H1.1 changes
runtime behavior. The artifact records:

- the exact trusted pre-H1 commit and candidate commit;
- OS, architecture, toolchain, build profile, job count, and workload hash;
- alternating baseline/candidate cold and warm samples;
- median and tail wall time, peak RSS, executable size, and output-write
  count;
- sample count and observed runner variance; and
- a reviewed relative regression ceiling derived from that variance.

Every H1 semantic candidate must satisfy both:

1. the existing absolute ceilings in
   `ratchets/h0-qualification.v1.json`; and
2. the tighter frozen H1 relative-regression ceiling against the trusted H0
   route.

The relative ceiling is measured and frozen before implementation; it is not
chosen after observing an emitting candidate. A candidate outside the ceiling
is optimized or redesigned. The H0 ceiling is not loosened to accept it.

The performance matrix retains the embedded-ES2025 explicit-root workload and
adds two H0-compatible no-emit workloads before the relative budget freezes:

- a representative `--noEmit -p` project with config discovery, imports, and
  package/type resolution, matching editor/linter invocation shape; and
- a stable multi-file scale project with enough parsed and checked nodes to
  expose a per-node field, allocation, or hot-loop branch tax that startup
  timing would hide.

All three record cold/warm wall time and peak RSS. The explicit-root row guards
startup and binary-layout regressions, the project row guards ordinary linter
use, and the scale row guards asymptotic no-emit regressions. Output-option
parsing cannot hide a regression outside the original small canary.

### 6.4 CI and qualification topology

Ordinary GitHub CI owns one stable acceptance boundary: the `gates` job runs
only `cargo xtask acceptance`. The command accepts no partial selectors and
draws test cases from `ts-tests`; it currently executes the complete diagnostic
conformance corpus. H1 extends that same command with the compatible
transpile/compiler/project/FourSlash emit projections as they become executable.
It does not add emitter-focused jobs, static phase checks, platform matrices,
scheduled stress, or evidence producers to Actions.

The complete local `cargo xtask ci --baseline <trusted-base>` remains required
before opening and merging every non-documentation runtime PR. It owns
formatting, Clippy, workspace tests, owner/oracle freshness, exact trusted-base
history, conformance/recovery/invariants, constructor/write-zero canaries,
stress, and same-process evidence production and consumption. The exact base,
conformance counts, FP/FN state, and gate result are recorded in the PR body.
A green hosted acceptance result never substitutes for that local proof.

No-emit and emit performance ratchets continue to use alternating
baseline/candidate samples on the approved frozen runner. Long stress and
resource qualification run locally or through an explicitly dispatched
qualification workflow; they are not ordinary GitHub CI. Third-party Actions
remain pinned to reviewed full commit SHAs, and the hosted Cargo build uses at
most two jobs. The retained receipt/failure-artifact schemas are local evidence
utilities and do not authorize additional hosted lanes.

## 7. Source tree, transform state, and future reuse

The parse tree becomes immutable after the existing parent/error stamping
pass. H1 does not erase types, rewrite imports, attach helpers, or install
substitution state by mutating the cached parse tree.

An emitting session owns:

- transform flags for parsed nodes;
- `emitNode`-equivalent comments, original-node links, source-map/token ranges,
  helper, and substitution metadata;
- synthetic nodes and arrays;
- lexical-environment stacks;
- requested helpers;
- printer substitution/notification handlers; and
- output-local generated-name state.

Parsed `NodeId`/`NodeArrayId` values remain valid for the lifetime of their
source version inside one L0 identity domain; an unchanged shared document
may therefore retain them across Program snapshots. Bound-document
`SymbolId`s follow the same domain/lease rule. Checker-created types,
signatures, transient symbols, links, and all synthetic emit IDs remain valid
only for the current checker or emitting session. Cross-run evidence and
document keys use canonical source identity, versions/text, options, and
spans, never raw arena IDs.

Parsed `pos`/`end` and source text remain the authority for original nodes.
Synthetic nodes record their original-node association and emitted source
range in the emitting session. Source-map support may later consume those
facts, but it may not add a mapping field to every persistent parse node or
retrofit locations after printing has already lost them.

### 7.1 Position domains are explicit

The Rust and JavaScript representations use different position units. Parsed
Rust node and array `pos`/`end` values are UTF-8 byte offsets, with the current
all-ones synthetic sentinel. In the vendored compiler, source-map source
positions and `TextWriter` output positions are JavaScript string offsets and
therefore UTF-16 code-unit offsets. A raw `u32` must never cross that boundary
without its position domain being known.

Keeping byte offsets in the parse tree is deliberate. Rust source slicing,
scanner/parser movement, and trivia/comment rescanning require UTF-8 byte
boundaries. Storing only UTF-16 positions would add a reverse conversion to
those common operations; storing both positions would enlarge every parsed
node and charge that memory/cache cost to `--noEmit`. The byte-to-UTF-16 map
already exists in the H0 implementation; L0 moves that behavior behind each
snapshot's `PositionIndex` accessor. An emitting session reuses that index
without a second map or persistent node field. UTF-16 AST positions would not
remove the other domain in any case: generated source-map columns must still
be counted from the Rust writer's UTF-8 output as UTF-16 code units.

A future TypeScript-compatible public AST or LSP surface must not expose the
raw internal `u32` as a protocol position. It converts through a per-file
accessor to the contract's requested unit. That is an API-boundary rule, not a
reason to change the parse-tree storage used by every batch invocation.

H1 keeps parsed nodes unchanged and uses emitter-local typed positions/ranges:

- source slicing, trivia scanning, and parsed/original ranges use validated
  UTF-8 byte boundaries;
- an emitting source borrows its L0 snapshot `PositionIndex`, whose static H0
  specialization may wrap the existing dense table, then derives the exact
  UTF-16 source line/column only at the map-recorder boundary;
- synthesized positions are represented as a distinct state and are never
  indexed into text or silently converted from `u32::MAX`;
- a range that switches its source file carries that source identity together
  with its byte-domain range, matching tsc's source-map-source switching; and
- the text writer stores exact UTF-8 output bytes while independently tracking
  generated line, column, and text position in UTF-16 code units. It does not
  infer map coordinates by rescanning or counting output bytes afterward.

The concrete newtypes are frozen in H1.0a before implementation. Conversions
reject out-of-bounds and non-character-boundary byte positions instead of
falling back to an identity conversion. Direct pins cover an astral character
before and inside a mapped token, combining characters, escaped and unescaped
identifiers/literals, LF/CR/CRLF/LS/PS, and NEL as an adjacent non-line-break
control, in both source and generated text. Production H1 uses the disabled
recorder; focused printer tests install a test-only coordinate recorder that
captures dormant hook inputs without encoding or serializing a source map.

This ownership rule is inherited from L0/L1: an unchanged parsed document can
be shared, and an incremental parse can copy reusable syntax, without carrying
stale transform, printer, checker, or generated-name state. H1 does not invoke
those update APIs or claim their behavior as emit compatibility.

### 7.2 Source text and cooked JavaScript-string values are separate

Position units are not the only encoding boundary. A JavaScript string is a
sequence of UTF-16 code units and can contain an unpaired surrogate produced
by an escape such as `"\\uD800"`. Rust `String` cannot represent that value.
The current scanner therefore exposes U+FFFD on its general token-value path;
only template processing has a lossless `Vec<u16>` side channel reconstructed
from raw text.

H1.0 must classify every active factory/transform/printer read of cooked
identifier, string, template, regexp-related, pragma, and module-specifier
values. For each in-profile read it must do one of the following before the
first write:

- prove that tsc prints the validated original raw slice and never observes
  the lossy cooked value on that route;
- derive a lossless emitter-session JavaScript-string value from raw source
  and use the same representation for synthetic factory nodes; or
- reject the exact construct in the frozen profile with an adjacent oracle
  canary.

A replacement character is never an acceptable fallback. The broad emitter
eventually needs a lossless code-unit value for every relevant parsed and
synthetic field. Keeping that value in the emitting session avoids adding a
vector to every parsed node and preserves the H0 memory contract. A later
JavaScript-compatible public API has the wider requirement that source text
and snapshots themselves preserve arbitrary JavaScript strings; broader
UTF-16LE/BE filesystem profiles must also match tsc on lone code units. H1.0
therefore freezes admitted input encodings and rejects any unrepresentable
input before emit unless a lossless source container has landed. Neither case
is solved by the parsed-literal side table alone.

Direct tests include paired astral escapes, each lone high/low surrogate,
escaped backslashes that must not decode, source-copy versus synthesized
printing, helper/generated literals, and source-map generated-column counts.

## 8. Checker-owned `EmitResolver`

Transforms consume a narrow internal resolver whose behavior is ported from
tsc's `getEmitResolver`/`createResolver`. It is not a public `TypeChecker`
API. Its initial method inventory is generated from every call reachable from
the frozen H1 transformer set.

Expected owner families include:

- value/reference classification for import and export aliases;
- referenced declarations and export containers;
- node check flags used by downlevel transforms;
- constant and enum-member values;
- lexical `this`, `new.target`, `super`, and captured-binding facts;
- JSX factory/fragment facts when a later profile admits JSX;
- generated-name collision and global-name queries; and
- declaration visibility/type construction only when declaration emit is
  separately admitted.

The H1 JavaScript profile must not accidentally require declaration-only
resolver methods. Each unavailable method returns a typed unsupported error
with a reachability canary; it never returns a fabricated conservative value.

Emit-only checker producers currently elided by the no-emit implementation
are restored only through this inventory. Each restoration has:

- an exact tsc owner and dependency closure;
- an emitting positive probe;
- an adjacent no-emit probe proving diagnostics remain unchanged; and
- a performance observation proving the no-emit entry did not begin
  collecting the new state.

## 9. Vendored TypeScript owner spine

H1.0 starts its dependency inventory from these TypeScript 6.0.3 roots:

| Owner | Vendored line | H1 responsibility |
| --- | ---: | --- |
| `computeLineStarts` / `getLineAndCharacterOfPosition` | 8250 / 8328 | existing UTF-16 line/column boundary |
| `getOriginalNode` / `setOriginalNode` | 11400 / 25208 | original-node association across transforms |
| `createTextWriter` | 16365 | exact bytes plus generated UTF-16 positions |
| `getOwnEmitOutputFilePath` | 16567 | per-source output relocation |
| `getDeclarationEmitOutputFilePath` | 16577 | dormant declaration-path slot |
| `getSourceFilesToEmit` / `sourceFileMayBeEmitted` | 16600 / 16617 | exact output eligibility and targeted selection |
| `writeFile` | 16644 | write/BOM/error boundary |
| `writeFileEnsuringDirectories` | 16663 | filesystem parent creation and retry boundary |
| `getSourceMapRange` / `setSourceMapRange` / `setTokenSourceMapRange` | 25336 / 25340 / 25344 | dormant node/token mapping ranges |
| `getEmitResolver` | 47561 | force/check diagnostics then expose resolver |
| `createResolver` | 88545 | semantic transformer query surface |
| `createSourceMapGenerator` | 92365 | dormant map generator and mapping contract |
| `transformTypeScript` | 94036 | TypeScript syntax transformation |
| `transformDeclarations` | 114265 | dormant declaration-transform root |
| `getTransformers` | 115897 | script/declaration transformer split |
| `getScriptTransformers` | 115903 | exact script-transformer ordering |
| `getDeclarationTransformers` | 115950 | dormant declaration-transform ordering |
| `transformNodes` | 115977 | transformation context and lifetime |
| `forEachEmittedFile` | 116312 | emitted-unit/output ordering |
| `getTsBuildInfoEmitOutputFilePath` | 116342 | dormant build-info path slot |
| `getOutputPathsForBundle` | 116365 | dormant bundle output shape |
| `getOutputPathsFor` | 116373 | per-source output path plan |
| `getSourceMapFilePath` / `getOutputExtension` | 116388 / 116391 | map-path and JS-family extension selection |
| `getOutputJSFileName` | 116409 | config-root-aware JavaScript path |
| `emitFiles` | 116530 | transform/print/write orchestration |
| nested `printSourceFileOrBundle` | 116744 | printer/map/write ordering seam |
| `createPrinter` | 116912 | exact text emission |
| `createWriteFileMeasuringIO` | 121960 | final filesystem-error-to-callback conversion |
| Program `getEmitHost` | 123482 | program-to-emitter host projection |
| Program `emit` / `emitWorker` | 123568 / 123595 | public emit and early gates |
| `verifyCompilerOptions` / nested `verifyEmitFilePath` | 124750 / 125028 | emit-active option and output-collision preflight |
| `handleNoEmitOptions` | 125636 | zero-cost no-emit/noEmitOnError exit |
| `emitFilesAndReportErrors` | 129412 | CLI diagnostic and exit orchestration |

Line numbers are navigation anchors, not identity. The generated H1 inventory
pins declaration body hashes and the complete reachable call graph. Any vendor
drift fails the inventory before code runs.

The inventory distinguishes "emitter" here from the repository's historical
diagnostic-emitter inventory. H1 emits JavaScript artifacts; D2 inventories
functions that emit diagnostics. Neither artifact may substitute for the
other.

## 10. Transform and printer boundaries

### 10.1 Transformer selection

H1 ports the real `getTransformers`/`getScriptTransformers` spine even when
the first frozen profile activates only a small transformer subset. It does
not call a bespoke "strip TypeScript" formatter.

The transformer plan always distinguishes script and declaration pipelines.
The H1 profile constructs and executes only its admitted script factories;
the declaration list is a dormant typed slot whose sole built-in root is
recorded as `transformDeclarations`. H1 does not port that root, declaration
NodeBuilder, symbol-accessibility diagnostics, or declaration-only resolver
methods.

Within those lists, the plan records tsc's `before`, built-in script,
`after`, built-in declaration, and `afterDeclarations` ordering positions.
Only built-in script factories are constructible in H1. The other positions
do not expose a Rust plugin ABI or accept callbacks; their typed provenance
prevents a later custom-transformer track from having to replace an
unstructured fixed list merely to reproduce tsc's ordering.

The profile manifest records every transformer in canonical tsc order as:

- active and ported;
- inactive with an option/target reachability proof and control; or
- unsupported, causing the invocation to fail before writes.

### 10.2 Transformation context

The `transformNodes` port owns the same categories of state as tsc:

- lexical and block-scope environments;
- hoisted declarations and initialization statements;
- helper requests;
- substitution and emit-notification enablement;
- per-node transform-feature gates; and
- transform diagnostics and disposal.

State is initialized once per emitted unit and disposed before the next
session can observe it. Repeated emission of the same prepared input must be
byte-identical regardless of prior emitted programs.

### 10.3 Printer

The printer is a reusable syntax-layer consumer with explicit options,
handlers, source context, and writer. It owns no filesystem or checker. All
semantic decisions enter through the transformed tree or explicit handlers.

Its internal root surface follows tsc's `Printer`: node, node list,
`SourceFile`, and `Bundle`. H1 acceptance initially reaches only whole-source
printing, but the implementation cannot be a function specialized to a
JavaScript `SourceFile` string. `printNode`/`printList` remain internal until a
separate public-printer or L-track contract exists, and `Bundle` fails closed
until bundling is admitted.

Printer acceptance includes tokens, precedence/parenthesization, trivia and
comments, indentation, line endings, literals/escaping, generated names,
substitution, and helper placement. Expected bytes always come from the
vendored oracle.

The printer may later serve LSP code actions, but H1 does not expose a public
printer API or accept LSP requirements that are not reached by the H1 emit
profile.

### 10.4 Declaration and source-map seams required now

Declaration and source-map behavior remains outside H1 compatibility, but the
following structure is fixed before H1.2 starts:

- `getTransformers` produces separate script/declaration plans;
- `noEmitOnError` retains the declaration-diagnostics position after
  options/syntax/global/semantic diagnostics without executing it in H1;
- transformed roots preserve source/original-node associations needed by
  either JavaScript or declaration printing;
- the printer accepts an optional source-map generator/recorder and performs
  source-map before-node, after-node, and token hooks at the same pipeline
  phases as tsc;
- the no-map implementation is the explicit disabled recorder path, not a
  second printer with mapping phases deleted;
- output planning retains separate JS-map and declaration-map paths without
  assuming every declaration ends in `.d.ts`; and
- `EmitArtifactKind`, callback metadata, and sink ordering can represent JS,
  JS map, declaration, declaration map, and build info independently.

This is a structural port, not permission to implement maps or declarations
inside H1. In particular, H1 does **not** implement VLQ encoding,
`sourcesContent`, `sourceRoot`, `mapRoot`, inline map serialization,
`transformDeclarations`, declaration type construction, or declaration
visibility diagnostics. Every corresponding option remains rejected before
writes.

The distinction prevents two known rewrites. Adding source maps after a
printer has discarded node/token source positions would require changing its
entire emission pipeline. Adding declarations after script and declaration
transforms have been collapsed into one list would require replacing
transform selection, resolver lifetime, output planning, and partial-write
behavior together.

### 10.5 Output topology, feedback, and future consumers

`forEachEmittedFile` and `emitFiles` operate on an internal
source-file-or-bundle root plus the full typed output-path shape from section
5.4. Whole-program JavaScript emit is the only admitted request, but the
function boundary retains a dormant target-source selection. This lets a
future Language Service investigate per-file `getEmitOutput` without changing
the batch H1 contract; H1 itself neither exposes nor qualifies that operation.

Write callback order, callback metadata, sink disposition, and the outcome's
`emitted_files` list stay independent. For example, when maps are eventually
admitted, tsc writes an external map before its associated text but records
the text before the map in `emittedFiles`. A future incremental builder may
also suppress an unchanged declaration through callback feedback. H1's two
sinks always write, but their typed result cannot make that later feedback an
ABI-breaking retrofit.

Build info uses the same generic artifact/sink boundary but not the
transformer or printer. Reserving its artifact/path discriminant does not
implement `--incremental`, Program reuse, signature comparison, or
`.tsbuildinfo` serialization; those remain a separate builder track.

## 11. Initial profile and fail-closed expansion

H1.0 measures the upstream compiler/project emit inventory and freezes the
first executable profile before H1.1. The intended bootstrap profile is:

- single project and one-shot execution;
- JavaScript output only;
- explicit `target: ESNext` (99), explicit `module: Preserve` (200), and
  absent/true `useDefineForClassFields`;
- no JSX output;
- no legacy or standard decorators;
- no source maps, declarations, build info, bundling, custom transforms, or
  plugins; and
- an initial erasable-TypeScript vertical slice before runtime TypeScript
  constructs are admitted owner by owner.

This exact option choice makes the canonical built-in script-transformer list
`transformTypeScript`, `transformClassFields`, then
`transformECMAScriptModule`. `transformClassFields` is still constructed even
though its rewriting branches are inactive for ESNext plus standard class
fields. Its context/hook lifecycle is therefore in profile; each rewrite arm
needs an explicit reachability proof and canary rather than omission. Choosing
`module: ESNext` instead would select
`transformImpliedNodeFormatDependentModule` and construct both the ESM and CJS
module transformer closures, so it is not an equivalent bootstrap shortcut.

H1.0 machine-freezes the target/module choices above. The exact accepted
extensions, config combinations, and syntax constructs are facts produced by
that inventory, not silently inferred from this prose. The frozen profile is
a machine manifest. A program that reaches enum,
namespace, decorators, parameter-property, JSX, module-downlevel, helper, or
other unported behavior fails before the first sink write.

The bootstrap choices are now machine-frozen in
[`h1-emit-profile.v1.json`](../../../ratchets/h1-emit-profile.v1.json). Its
generator executes the vendored `getTransformers` selection and rejects drift
from `transformTypeScript`, `transformClassFields`, then
`transformECMAScriptModule`; it also binds the exact H0 base profile, TypeScript
bundles, schemas, Node pin, admitted options, rejected feature roots, and
dormant axes.

Profile expansion is monotonic and evidence-backed. One slice adds one
dependency-complete transformer/resolver/printer owner group plus exact
oracle output. It may not trade away a previously matching output.

### 11.1 Emit-active option and output preflight

An H0-valid prepared configuration is not automatically valid after
`noEmit` becomes false. H1 adds a separate emitting loader/validation entry;
it does not weaken `load_config_program`'s mandatory no-emit gate or send H0
through a generalized emit mode.

The H1.0 option inventory includes every `verifyCompilerOptions` branch and
checker/transform entry mode whose reachability changes when output is
enabled, including `noEmitOnError`, `noCheck`, `isolatedModules`,
`verbatimModuleSyntax`, and `rewriteRelativeImportExtensions`. For example,
`allowImportingTsExtensions` is valid with `noEmit` but requires
`emitDeclarationOnly` or `rewriteRelativeImportExtensions` when emitting.
The oracle freezes the effective command/config precedence and exact option
diagnostics for both sides of each such relationship.

Before the first sink call, the emitting program also ports tsc's output
preflight:

- determine exact source-file eligibility through `getSourceFilesToEmit` and
  `sourceFileMayBeEmitted`;
- select `.js`/`.jsx`/`.mjs`/`.cjs`/`.json` output behavior through
  `getOutputExtension`, with every non-admitted input/output family rejected;
- compute output paths with the host's current directory, common source
  directory, and case profile;
- reject a path that overwrites an input file;
- reject case-aware duplicate outputs produced by distinct inputs;
- preserve per-path emit-blocking diagnostics; and
- reject unsupported products/options before a successful artifact exists.

These are program/config diagnostics, not filesystem-sink errors. Parent
directory creation and the write/retry/error conversion owned by
`writeFileEnsuringDirectories` remain an `FsOutputSink` concern after the
preflight succeeds.

### 11.2 Upstream emit-suite inventory

H1.0a inventories upstream tests by the API path they exercise rather than
treating every file containing output as interchangeable.

The audited pre-transition state was explicit: suite pin v1 contained only
`compiler`, `project`, and `projects`, and the local `ts-tests` tree contained
neither `transpile` nor `fourslash`. The reviewed additive suite pin v2 binds
v1 by path and SHA-256, preserves its three entries exactly, and pins and
vendors all 22 `transpile` files plus the `transpileRunner` Git blob from
source commit `050880ce59e30b356b686bd3144efe24f875ebc8`. Additive suite pin
v3 in turn binds v2 byte-for-byte, preserves all four complete suite entries,
and pins the complete FourSlash source tree as 6,568 files, 14,198,525 bytes,
and Git tree `775c30f57c0638a180e7ac2e38b2581976620ca5`.

The checked-in
[`fourslash-emit-projection.v1.json`](../../../vendor/typescript-6.0.3/fourslash-emit-projection.v1.json)
is the mechanically extracted batch-emit projection of that full tree. It
vendors only 38 fixtures/31,051 bytes: 31
`baselineGetEmitOutput`, five `getEmitOutput`, one
`verifyGetEmitOutputForCurrentFile`, and one
`verifyGetEmitOutputContentsForCurrentFile` call. It also freezes all 49
ordered `emitThisFile` directives, the two declaration/comment false-positive
controls, the extractor hash, the projection Git tree/blob inventory, and the
`fourslashImpl`/`fourslashInterfaceImpl` runner-source blobs. The producer's
ordinary `--check` is offline; `--source-root` additionally reconstructs and
re-scans the full pinned upstream tree. These counts and source pins are
inventory facts, not executed-test or compatibility claims.

The existing expansion v1 remains byte-identical: it inventories 7,086
`compiler`, `project`, and `projects` sources, expands exactly 7,276 compiler
cases and 632 project cases, and retains initial state `not-run` for all 7,908
rows.
The newly pinned transpile sources have no expansion or execution rows until
H1 reproduces and classifies their runner matrices. H0 structural load/session
qualification does not change that upstream-runner state. At the pinned
commit, `compilerRunner` separately observes diagnostics, module-resolution
traces, source-map records, JavaScript/declaration output, source-map output,
and type/symbol baselines. H1 records a separate result for every row and
observation it admits and leaves all other observations explicitly deferred;
it never promotes Program construction alone to an emit pass.

The inventory then classifies these sources:

1. `tests/cases/compiler`, `tests/cases/project`,
   `tests/cases/projects`, and the existing conformance emitter families are
   the primary whole-Program input universe. An admitted row must pass through
   production program construction and `ProgramSession::emit`.
2. The complete TypeScript 6.0.3 `tests/cases/transpile` tree has been added
   through a reviewed additive suite-pin-v2 transition. H1 next reproduces
   `transpileRunner` unit partitioning and option matrices. In-profile
   JavaScript rows directly pin
   transform/printer behavior; declaration and map rows remain inventoried
   unsupported controls. `transpileModule` is a component oracle and never
   substitutes for whole-Program emit acceptance.
3. H1.0a has scanned the complete upstream `tests/cases/fourslash` tree at the
   same source commit and written a versioned manifest of every direct
   DSL/API operation whose runner reaches Language Service `getEmitOutput`,
   including
   `getEmitOutput`, `baselineGetEmitOutput`,
   `verifyGetEmitOutputForCurrentFile`,
   `verifyGetEmitOutputContentsForCurrentFile`, and `emitThisFile` metadata.
   The manifest pins the upstream tree, extractor, paths, blob hashes,
   operation line/class, and metadata order. The 38 selected fixture bytes
   are checked in; the other 6,530 FourSlash files are represented only by
   the complete source-tree identity.
   A one-shot case may become a non-gating H1 cross-control only after the
   oracle proves that its Language Service observation is equivalent to the
   H1 whole-Program request. Per-file, edit/version, formatting, server, or
   other Language Service state remains deferred to the L-track.

The full FourSlash runner is not an H1 dependency, and H1 does not claim a
FourSlash pass rate. Any promoted input bytes are checked in with their pinned
provenance so ordinary verification is offline; expected JavaScript is still
captured from the vendored compiler rather than copied or hand-authored.

## 12. Oracle, comparison, and acceptance evidence

The H1 oracle creates the same TypeScript 6.0.3 program and captures its
`writeFile` calls in memory. One canonical record contains:

- ordered write-callback index and output path;
- exact output bytes and BOM decision;
- source-file-provenance presence and ordered content;
- output kind;
- normalized write-callback metadata presence/content and sink disposition;
- emit diagnostics including chains and related information;
- `emitSkipped`, emitted-file-list presence and independent order, and
  source-map-observation presence; and
- process exit status.

The first callback-level producer is
[`h1-emit-oracle.mjs`](../../../crates/oracle/h1-emit-oracle.mjs), checked with
`node crates/oracle/h1-emit-oracle.mjs --check`. It executes every frozen case
twice and records exact callback text bytes separately from BOM-materialized
sink bytes. The initial controls make map-before-text callback order visibly
independent from text-before-map `emittedFiles` order without admitting maps
to H1.

Comparison is exact. Line-normalized or formatter-normalized JavaScript is
not acceptance evidence. Paths may be canonicalized only through the same
typed virtual-root mapping on both sides; contents remain byte-for-byte.

Every slice carries:

- an immutable before observation;
- the exact tsc owner/dependency inventory;
- positive and adjacent-negative fixtures;
- output-set and byte diffs;
- H0 diagnostic accepted-set/FP non-regression;
- a `--noEmit` constructor/write-zero canary; and
- the no-emit performance result required by section 6.3.

The upstream project suite's currently classified emit cases provide an
initial inventory, not automatic acceptance. Each admitted case must execute
through the production Rust host, checker, emitter, and in-memory sink and
match its oracle outputs.

Out-of-profile declaration, source-map, bundle, build-info, transpile, and
FourSlash rows still retain their upstream observation and classification.
They prove that the unsupported gate is intentional and adjacent to a real
future behavior; they do not count as H1 output parity and cannot force that
future implementation into the JavaScript slice.

## 13. Relationship to incremental parsing, build reuse, and LSP

H1 and the L-track have a deliberate implementation dependency without
sharing a compatibility claim. L0 and L1 land first to validate the source,
arena, bind, and checker lifetime boundary; full old-Program/resolution reuse,
Language Service, tsserver, and LSP behavior remain later tracks.

The shared invariants are:

- every published Rust source version is immutable; incremental parsing may
  copy from it but does not attach emit or checker state to it;
- unchanged parsed and bound documents may be shared across Program snapshots
  inside one identity domain;
- parsed/bound IDs are source-version/domain stable, while checker-created
  semantic state is disposable per Program version;
- transform and printer state is emit-session-local;
- serialized output/evidence uses paths and spans, never raw IDs; and
- no unbounded process-global mutable cache is keyed by an internal ID.

L1 turns the currently empty `SyntaxCursor` seam into real ordinary-parser
reuse and proves the chosen arena strategy against fresh parses and a
large-file edit budget before H1.1. H1 consumes only the resulting immutable
`ProgramSnapshot`; it neither invokes incremental update APIs during emit nor
reuses transform nodes across source versions.

`--incremental`/`.tsbuildinfo` is distinct from incremental parsing. A future
builder may compare deterministic `EmitArtifact` identities and suppress
unchanged writes through the typed sink disposition, but H1 neither creates
build graphs nor stores build info. That builder must also qualify tsc's
build-info write under `noEmit`; it is outside the frozen H0/H1 no-build
profile and cannot become an exception to H0's zero-write contract by
accident.

An LSP consumer may later reuse the printer and node factory for code edits.
It still creates a fresh checker per Program version and defines its own query,
cancellation, protocol, latency, and memory contracts. H1 does not make its
internal `EmitResolver` a public language-service API.

Likewise, retaining an internal target-source discriminant and inventorying
FourSlash `getEmitOutput` cases is only an architectural seam and evidence
map. It does not expose per-file emit, construct a Language Service, or claim
incremental/FourSlash compatibility in H1.

## 14. Required landing order

H1 and its workspace prerequisite execute in dependency order:

The H1.0a producers now regenerate the active-root closure, exact
declaration/body/ledger hashes, callback-nesting edges, unresolved calls, and
dormant declaration/map/bundle/targeted/build-info anchors with
`node crates/oracle/h1-owner-inventory.mjs --check`, and freeze the bootstrap
profile plus callback observations with
`node crates/oracle/h1-emit-oracle.mjs --check`. The current production Rust
scope is independently hashed and its 11 missing production boundaries, 32
effective option-projection omissions, and 25 explicit checker emit elision
and control rows
are frozen with
`node crates/oracle/h1-rust-omission-inventory.mjs --check`. The owner artifact
deliberately remains `draft/report-only`. The complete transpile source tree is
also pinned in the additive v2 source universe without expansion or execution
rows. The additive v3 source universe now freezes the complete FourSlash tree
identity and exact 38-file emit projection, also with zero expansion or
execution rows; corpus classification and reviewed unresolved/property-
dispatch dispositions still keep item 1 open.

1. **H1.0a — inventory and oracle:** generate the JavaScript-emitter owner
   graph, classify the compiler/project/conformance/transpile corpus plus the
   FourSlash emit projection, freeze dormant declaration/map/bundle/targeted/
   build-info seams, land the in-memory oracle, and record every current
   emit-only Rust omission. The inventory
   and design portion may proceed in parallel with the next item.
2. **L0/L1 prerequisite — persistent source and parser proof:** land shared
   text/position snapshots, identity leases, owned parse/bind records,
   `ProgramSnapshot`, the ephemeral H0 adapter, minimal registry reuse, and
   the incremental parser plus its large-file performance gate. Requalify H0
   diagnostics, host observations, exits, latency, and RSS; do not accept a
   new baseline by weakening the frozen H0 ceilings.
3. **H1.0b — no-emit performance and CI freeze:** collect alternating
   post-L0/L1 pre-H1 baseline/candidate measurements, freeze the relative
   regression policy, freeze ordinary GitHub CI to the `ts-tests`-only
   `cargo xtask acceptance` boundary, and land constructor/write-zero canaries
   in the complete local gate before H1 runtime behavior changes.
4. **H1.1 — typed execution spine:** add the non-publicly-constructible
   `EmitArtifact`, callback metadata, sink disposition, full typed output-path
   shape, `OutputSink`, typed failures, and the separate emitting session
   entry. Unsupported emission reaches no sink. H0 results and performance
   remain unchanged.
5. **H1.2 — factory, transform context, and printer foundation:** port the
   synthetic/original-node ownership model, `transformNodes` lifecycle,
   dual-domain writer/position conversion, disabled source-map hook phases,
   and the generic printer pipeline, with direct Unicode/newline oracle pins.
   Only whole-source printing is active; node-list, standalone-node, bundle,
   map, and declaration requests remain unreachable typed controls.
6. **H1.3 — active transformer and resolver slice:** port the exact
   `transformTypeScript` -> `transformClassFields` ->
   `transformECMAScriptModule` list selected by the frozen profile, including
   each transform's context/hook setup. Port the reachable
   `transformTypeScript` resolver producers for the first erasable-TypeScript
   profile; close inactive class-field/module branches with generated
   reachability evidence rather than fabricated resolver answers.
7. **H1.4 — output planning and in-memory emit:** port transformer selection,
   source eligibility, emit-active option/output-collision preflight,
   `emitFiles`, output paths, callback versus emitted-file ordering,
   diagnostics, and repeated-run determinism through `MemoryOutputSink`.
8. **H1.5 — filesystem/CLI connection:** connect the same artifacts to
   `FsOutputSink`, match CLI diagnostics and exit behavior, and prove that a
   failure before or during writing has the oracle's exact parent-retry,
   diagnostic-and-continue, and partial-write boundary.
9. **H1.6 — profile closure and qualification:** close every owner in the
   frozen profile, execute every compatible upstream emit case, freeze output
   and resource summaries, and publish the expanded binary only after all H0
   and H1 gates are green.

No slice may move emit work into `ProgramSession::run`, relax an H0 ceiling,
or start M9 qualification history. M9 fingerprint freeze waits until shared
checker behavior is stable.

## 15. Definition of done

H1 is complete only when:

- the frozen JavaScript-emit owner inventory has no open in-profile row;
- every admitted output path, byte sequence, BOM decision, source-provenance
  presence/content, callback-metadata presence/content and sink disposition,
  diagnostic, write order, `emitSkipped`, emitted-file-list/source-map presence,
  independently ordered emitted-file content, and exit status matches the
  vendored compiler;
- emit-active option validation, source eligibility, input-overwrite checks,
  and case-aware duplicate-output checks match the vendored compiler before
  the first sink call;
- unsupported options and constructs fail before their oracle-equivalent
  write boundary and never report partial success;
- the production CLI requires no Node or repository `vendor/` lookup at
  runtime;
- repeated runs and legal worker/job counts produce byte-identical outputs;
- `MemoryOutputSink` and `FsOutputSink` observe the same ordered artifacts;
- every injected sink failure produces the exact write diagnostic, continuation
  behavior, partial output set, `emitSkipped`, and exit status;
- `ProgramSession::run` and the CLI `--noEmit` route construct zero H1
  components and perform zero output writes;
- every H1 runtime PR has passed the hosted `ts-tests` acceptance command and
  the complete local gate against its trusted base; the local result and exact
  base are recorded in the PR body, and hosted acceptance alone is rejected;
- all H0 diagnostics, renderings, exit statuses, host identities, and
  accepted sets remain exact with full-corpus FP=0;
- no-emit cold/warm wall time, peak RSS, and executable-startup effects remain
  within both the frozen H0 ceilings and the H1 relative regression budget;
- original parse trees remain immutable and no program-local ID escapes into
  artifacts or process-global caches;
- parsed byte positions, source UTF-16 coordinates, and generated UTF-16
  coordinates never cross as untyped integers, including synthetic and
  source-switching ranges;
- every admitted cooked JavaScript-string value is lossless in UTF-16 code
  units or is proven unread on the raw-copy route; no lone surrogate becomes
  U+FFFD during transform or print;
- the dormant declaration/map/bundle/targeted/build-info axes retain their
  typed slots and reachability canaries without becoming H1 compatibility
  claims; and
- declaration emit, maps, build/watch/incremental, LSP, plugins, and newer
  TypeScript versions remain explicitly unclaimed.

## 16. Stop conditions

Stop and review the H1 design if:

- a proposed transformer mutates the original parsed tree;
- emission requires reparsing or rebuilding semantic state after diagnostics;
- a no-emit invocation constructs an emit resolver, transform arena, printer,
  output plan, artifact, or sink write;
- an emit-only field is added to every node/link without no-emit memory and
  performance evidence;
- a branch is declared unreachable without a constructibility proof and
  adjacent canary;
- expected output is hand-authored instead of captured from the oracle;
- output comparison needs whitespace or line-ending normalization to pass;
- the JavaScript slice introduces a JS-only intermediate tree, source-file-
  only printer, single-product output record, or sink API that cannot express
  callback metadata and write disposition;
- printer implementation deletes source-map hook phases or loses original/
  synthetic source ranges even though the map generator itself is deferred;
- a writer or map seam counts UTF-8 bytes or Unicode scalar values where tsc
  observes UTF-16 code units, or treats a synthetic position as a text index;
- an active transform/printer/factory branch consumes the scanner replacement
  character for a JavaScript string value that contained a lone surrogate;
- callback order and `emittedFiles` order are collapsed into one observation;
- H0 config/program loading is generalized by removing its mandatory
  `noEmit` gate instead of adding a separate emitting entry;
- one slice spans unrelated transformer/resolver/printer owner groups;
- a candidate exceeds the H1 no-emit relative performance budget;
- an H1 runtime change can merge with hosted `ts-tests` acceptance alone,
  with a stale or text-only local-gate claim, or without its internal focused
  tests in the complete local gate;
- H0's resource ceiling or accepted state would need weakening; or
- L0/L1 state is bypassed, copied into an H1-private source model, or polluted
  with checker/transform/printer state; or
- old-Program/resolution reuse, Language Service, tsserver, LSP, build/watch,
  public API, custom-transformer, or declaration behavior is pulled into the
  H1 compatibility claim to solve a JavaScript-emit mismatch.

Hard or slow implementation is not a reason to bypass the vendored owner
spine, fabricate a resolver answer, or charge emitter work to `--noEmit`.
