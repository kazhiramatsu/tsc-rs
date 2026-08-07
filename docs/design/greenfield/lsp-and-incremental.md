# LSP and incremental parsing — persistent Program foundation

Status: L0.4 one-shot and registry implementation complete, 2026-08-07;
approved-runner qualification remains before L1. The architecture audit found that the L0.0 one-shot data model was
**not sufficient** for efficient Language Service, tsserver, or LSP operation.
A bounded persistent-source foundation (`L0`) and the incremental-parser proof
(`L1`) must land before H1 emit implementation starts. This does not put
Language Service, tsserver, or LSP behavior inside H1; it prevents H1 from
freezing ownership and identity seams that those products would otherwise
have to replace.

Source authority is TypeScript 6.0.3 commit
`050880ce59e30b356b686bd3144efe24f875ebc8`. The source declarations and tests
were read from the local TypeScript checkout with `git show <pin>:<path>`;
the checkout's working commit is not used as a substitute for the pin. The
load-bearing owners are:

- `src/compiler/parser.ts`: `IncrementalParser.updateSourceFile`,
  `createSyntaxCursor`, parser `currentNode`, and `canReuseNode`;
- `src/services/documentRegistry.ts`: compilation-setting buckets and
  acquire/update/release ownership;
- `src/services/services.ts`: source-file creation/update and Language Service
  Program synchronization;
- `src/compiler/program.ts`: `tryReuseStructureFromOldProgram` and resolution
  reuse;
- `src/compiler/binder.ts`: bind data cached on a reused `SourceFile`;
- `src/server/scriptInfo.ts` and `src/server/scriptVersionCache.ts`: editor
  text, line index, snapshots, and collapsed change ranges; and
- the pinned unit-test owners listed in section 9.

The broader boundary between H1, builder/watch, public API, Language Service,
tsserver, and LSP remains in the
[compiler compatibility residual](compiler-compatibility-residual.md).

## 1. What tsc actually reuses

There are five distinct layers. Treating them as one "incremental parser"
feature hides the ownership work.

1. **Versioned editor text.** tsserver `TextStorage` switches edited files to
   `ScriptVersionCache`. Its persistent `LineIndex` stores line leaves and
   subtree character/line counts, retains a bounded snapshot history, and
   collapses edits into the `TextChangeRange` requested by a later snapshot.
2. **Incremental reparse of one changed file.** `updateSourceFile` extends the
   edit to cover parser lookahead, shifts old node/list ranges, marks
   intersecting elements, builds a syntax cursor, and runs the ordinary
   parser. List parsing asks `canReuseNode` before parsing an element.
3. **Document and bind reuse.** `DocumentRegistry` buckets by the generated
   `sourceFileAffectingCompilerOptions` key plus path, script kind, and implied
   node format. It reference-counts entries and updates only when the script
   version changes. tsc's binder skips work when a reused `SourceFile` already
   has `locals`, so unchanged files reuse parse **and bind** state.
4. **Program and resolution reuse.** Language Service passes the old Program
   to `createProgram`. `tryReuseStructureFromOldProgram` distinguishes no
   reuse, safe-module reuse, and complete reuse after checking roots, options,
   project references, source identity, imports/directives, missing files,
   package state, and invalidated module/type/lib resolutions.
5. **Fresh checker state.** A new Program receives a new TypeChecker. Type,
   signature, transient-symbol, node-link, and relation caches do not migrate
   from the old checker. Lazy Language Service queries make that affordable.

One earlier statement needs explicit correction: tsc's incremental parser
**does mutate the old tree in place**. The source comment says that old nodes
receive new positions and parents and that the original `SourceFile` becomes
unusable after one incremental update. JavaScript object reuse is therefore
not an immutable/persistent-tree algorithm. A Rust implementation may use an
immutable copy-on-reuse adaptation, but it must not describe that adaptation
as tsc's literal mutation behavior or claim JavaScript object-identity parity.

## 2. L0.0 Rust audit

Several entry-state choices were good foundations, but the complete ownership
chain was one-shot before L0.1. Section 8.2 records the accepted replacement.

| Layer | Useful current property | Blocking gap |
| --- | --- | --- |
| Parsed tree | Per-file `NodeArena`; parents are finalized separately; binder/checker mutations already live mostly in side tables | `SourceFile` owns `String` and a deeply cloned arena; there is no shareable source-version owner |
| Cursor | `parse_source_file` already accepts `Option<&SyntaxCursor>` | `SyntaxCursor` is an empty type and the parser names the parameter `_cursor`; no list consults it |
| Node identity | Per-file node/array ranges let `ProgramBinder` route an ID to its source | bases depend on all previously parsed files, and `ProgramBinder` asserts contiguous ranges; editing/reordering an earlier file rebases every later file |
| Text positions | Byte-domain AST ranges and UTF-16 diagnostics are explicit | `LineMap` exposes a `Vec<u32>` entry for every byte and is rebuilt for the full text; it has no UTF-16-to-byte edit conversion or incremental splice |
| Prepared input | canonical paths, implied format, package facts, source order, and resolutions are already explicit | `PreparedSourceFile -> InputFile -> SourceFile` clones full source text, and `SourceFileId` is only a Program-order index |
| Binder | Per-file symbols, document-local `FlowArena`/`FlowId`, flags, locals, and diagnostics are already separated from checker links | `Binder<'a>` borrows both `SourceFile` and `CompilerOptions`; symbol bases and the `next_symbol_id` serial embedded in private names depend on preceding files, so a bound file cannot be cached as an owned entry |
| Checker | `CheckerState` and links are fresh for every execution; this matches tsc | checker construction also owns parsing/binding today, then drops all three layers into `CheckResult` |
| Program | `PreparedProgram` is a validated immutable one-shot snapshot | `ProgramSession` consumes it; there is no old Program, source/project version, acquire/release, or structural-reuse state |
| Resolution | exact resolution tables and a per-run package cache already exist | `ModuleResolver` and its cache die after each load; there are no failed-lookup/package/config dependency sets or invalidation APIs |
| Library reuse | the harness proves that an exact immutable lib prefix can be parsed/bound once | it uses process-lifetime `Box::leak`, fixed prefix bases, and is explicitly not a production or bounded server cache |

The most expensive conflict is numeric identity. Today a cached `SourceFile`
cannot simply enter a new Program: its IDs may collide with another cached
file or leave the contiguous interval layout expected by `ProgramBinder`.
Rebasing every unchanged tree and binder into Program order would preserve
correctness but defeat whole-file zero-copy reuse, invalidate bind maps, and
make every edit proportional to the complete project.

The position index is a second early conflict. The current per-byte table is
excellent for O(1) batch conversion, but a one-character edit reconstructs a
table proportional to the whole file. It also costs roughly four bytes per
UTF-8 byte before vector overhead. Language Service completion requires an
editable/snapshot index, not just the existing table behind another cache.
The repository audit found roughly 128 `line_map`/`byte_to_utf16` references,
so hiding representation behind accessors is a cross-crate migration even
though most call-site edits should be mechanical.

The current document-local `FlowId` model is already the right boundary. It
does not need an identity-domain lease or relocation: the checker resolves a
flow through its owning file/bound document. The required change is to make
that pairing a published API invariant and prevent raw `FlowId` values or
flow-query results from escaping their owner/Program version.

## 3. Decision: add L0 before H1 implementation

Do not start H1's checker-lifetime implementation on the present
`PreparedProgram -> InputFile -> parse -> bind -> check -> drop` chain. Land a
persistent Program foundation first:

```text
filesystem/editor text
        |
        v
VersionedTextStore --snapshot/change range--> DocumentStore
                                               | parsed + bound entries
                                               v
                                         ProgramSnapshot
                                               |
                              +----------------+----------------+
                              v                                 v
                     fresh CheckerSession                 later Program reuse
                       |             |                    and Language Service
                       v             v
                    H0 run        H1 emit
```

Only `VersionedTextStore`, `DocumentStore`, and immutable parsed/bound entries
survive across **Program versions**. A Language Service may retain one
`ProgramSnapshot` and its lazy `CheckerSession` across many queries for that
version, but a source/program update creates a fresh checker rather than
migrating links/types. H0/H1 use a scoped checker. Emit transforms, printer
state, diagnostics work queues, and generated names are always disposable.

Full Project Service, Language Service queries, tsserver protocol, watchers,
and LSP are not prerequisites for H1. The prerequisite is the ownership and
identity substrate they all need.

## 4. L0 persistent-source contract

### 4.1 Text snapshots and position index

Replace public `String`/`LineMap` field coupling with an immutable source-text
view:

```rust
pub struct TextSnapshot {
    document_version: DocumentVersion,
    lineage: SnapshotLineage,
    text: Arc<str>,
    positions: Arc<PositionIndex>,
}
```

`Arc<str>` is provisional for the current Rust-native UTF-8 source contract;
it is not evidence that lone-surrogate JavaScript strings are supported. A
JavaScript-compatible public API must close the separate lossless-code-unit
representation requirement before exposing snapshots. Names are provisional,
but these properties are required:

- `PreparedSourceFile`, checker input, and `SourceFile` share the same
  `Arc<TextSnapshot>` instead of cloning the complete payload at each
  boundary; `SourceFile` is the parsed document's single snapshot authority;
- `DocumentVersion` is the opaque host promise used by the registry, while
  `SnapshotLineage` is the text store's internal identity/revision proof used
  only to collapse edits; equal host versions do not manufacture ancestry;
- parser/scanner still borrow a contiguous `&str`; open-document storage may
  materialize it once per published snapshot, as tsc materializes `newText`;
- `PositionIndex` owns byte line starts, UTF-16 line starts/counts, and
  validated byte<->UTF-16 conversion methods; callers no longer index a
  public `byte_to_utf16` vector;
- the edited form ports the structural idea of tsc's `LineIndex`: persistent
  line leaves with subtree byte, UTF-16, and line counts, rebuilding only the
  changed paths/lines rather than a table for every byte;
- a snapshot retains a bounded history and returns a collapsed change range
  only for an ancestor from the same store; otherwise it returns `None` and
  forces a full parse;
- `Utf16TextChangeRange` and `ByteTextChangeRange` are different types.
  Conversion happens against the old snapshot, insertion validation against
  the new snapshot, and a midpoint of a UTF-8 scalar never rounds; and
- LF, CR, CRLF, LS, and PS match tsc line behavior; NEL remains an adjacent
  non-line-break control.

The initial thresholds/history length should port pinned
`ScriptVersionCache` rather than inventing policy: eight retained versions,
snapshot materialization after the ninth pending edit, or when one deletion
or insertion exceeds 256 UTF-16 code units. A later performance change
requires its own behavior/resource evidence because `getChangeRange`
availability changes whether parsing is incremental or full.

The first static H0 specialization may wrap the existing dense table behind
the new accessors to preserve its O(1) conversion and measured latency. A
compact immutable form is optional only after comparison. The edited form is
separate and uses the persistent line index; batch `--noEmit` must not build
that tree. Both forms implement one accessor contract. H1 source-map hooks
consume that contract; they may not restore direct per-byte-vector
assumptions.

### 4.2 Stable identity without enlarging every AST edge

Keep `NodeId`, `NodeArrayId`, and `SymbolId` compact, but change the meaning of
their bases. An `IdentityDomain` owns leased, non-overlapping ranges for
live parsed/bound document versions, plus the binder's tsc-style symbol
serials used in private-name keys. A lease lasts at least as long as every
`Arc<ParsedDocument>`/`Arc<BoundDocument>` that can expose its IDs or embedded
private names.

The persistent/reusable publication path is:

1. parse or bind into a local range;
2. reserve exact node, array, or symbol intervals after counts are known;
3. apply a schema-generated relocation over every ID-bearing field; and
4. publish the immutable document only after relocation, parent finalization,
   diagnostics, and bind state are complete.

The one-shot H0 allocation policy may avoid that extra relocation traversal.
Because it allocates documents serially and discards the complete domain on
failure, it can open a provisional lease at the current bump tail, parse/bind
directly into those final bases, then seal the lease with the observed counts.
No other allocation may interleave before sealing. Persistent or concurrent
stores still use local construction plus exact reservation/relocation unless
they prove an equally safe allocator. Both policies publish the same lease
contract and must produce identical documents under forced nonzero bases.

`nodes.schema.json` must generate both immutable child visitation and mutable
syntax-ID relocation. The relocation closure also explicitly owns common
`Node.parent`/`Node.js_doc`, every `NodeArray.nodes` element, nested payload IDs
such as `JSDocComment::Nodes`, arena bases, and the `SourceFile`
root/external-module fact wrapper. Binding runs only after that relocation, so
its node-keyed maps and flow payloads already receive final node IDs.

A separate generated/declared `BindData` relocation owner covers every
`SymbolId` occurrence in `SymbolArena`, symbol tables, symbol parent/export
links, node-to-symbol values, assigned-serial keys, and ambient module
records. Hand-maintained kind switches or an informal field list are not
acceptable completeness boundaries. Forced nonzero-base tests compare a
relocated result with a directly based reference.

`ProgramBinder` changes from contiguous-range assertions to sorted,
non-overlapping owner intervals. Symbol owner lookup receives the same change;
it may not assume bind/Program order.

Persistent bound symbols occupy the untagged half of `SymbolId`; the high bit
is reserved for a CheckerSession-local transient arena. Concurrent checker
sessions may reuse those tagged values because no checker-local ID crosses a
session boundary. `ProgramBinder` routes an untagged ID through sorted bound-
document intervals and a tagged ID directly to its own transient arena. This
avoids predicting a transient-symbol count before checking while preserving a
compact ID. Exhausting either partition is a typed resource failure, never
wraparound.

The current `Binder::next_symbol_id` is a second identity source, not the
`SymbolArena` base: it is interpolated into `__#<id>@<name>` keys for private
members. L0 injects a domain serial allocator into `BinderWorker` and retains
the resulting `PrivateNameSerialLease` in `BoundDocument`. H0 uses the
ephemeral bump implementation. A cached bind may not be seeded from the
previous file in the new Program order, and private-name strings may not be
renumbered after publication.

Released intervals may be recycled only after all parsed, bound, Program,
checker, and public-wrapper owners have dropped. Programs may combine entries
only from the same identity domain. This avoids three worse alternatives:

- rebasing every unchanged file on every Program refresh;
- a process-global monotonically growing raw-ID cache with history-dependent
  exhaustion and no release; or
- widening every AST child edge to a file-qualified 64-bit handle and charging
  that memory cost to batch `--noEmit`.

Parsed IDs become **source-version/domain stable**, not Program-order stable.
They are still never serialized, compared across domains, or used as oracle
identity. A changed document version receives new ranges; an unchanged entry
keeps its ranges. Checker `TypeId`, signature IDs, transient symbols, and
links remain CheckerSession-local. Emit synthetic IDs remain emit-session-
local.

The identity-domain implementation may have two allocation policies behind
the same lease contract: an ephemeral bump policy for one-shot H0, released in
one operation with the session, and a reclaiming interval policy for a
long-lived service. H0 must not pay for synchronized free-list maintenance on
each node or symbol, nor for a relocation pass when the sealed-tail rule above
applies. `Arc` ownership is document-granular, never an atomic reference
operation on every AST edge or traversal.

### 4.3 Owned parse and bind records

Split the mutable binder worker from its published result:

```rust
pub struct ParsedDocument {
    address: DocumentAddress,
    // SourceFile owns the sole Arc<TextSnapshot> authority.
    source: SourceFile,
    node_lease: NodeIdentityLease,
}

pub struct BoundDocument {
    parsed: Arc<ParsedDocument>,
    data: BindData,
    symbol_lease: SymbolIdentityLease,
    private_name_lease: PrivateNameSerialLease,
    bind_key: BindKey,
}
```

`BinderWorker<'a>` may borrow the parsed document and effective options while
running; `BindData` may not. The current binder reads only target,
`alwaysStrict`, and `noFallthroughCasesInSwitch`, but the cache key is
generated from the complete pinned `affectsSourceFile ||
affectsBindDiagnostics` owner set so a new read cannot silently reuse stale
state. Publication is all-or-nothing: cancellation, panic containment, or a
typed bind failure leaves no cacheable partial `locals`, flow graph, symbol
table, flags, or diagnostics.

`BindData` contains only completed, checker-consumed results; walk stacks,
current container/flow targets, delayed-work queues, and borrowed inputs stay
in `BinderWorker`. Its `FlowArena` and `FlowId` values remain document-local,
not identity-domain leases. Every Program/checker access carries the owning
`BoundDocument` (currently `(file, FlowId)`), and every checker flow cache is
Program-version-local. Reusing a bound document is therefore safe across a
new file order without relocating FlowIds; caching a raw FlowId without its
owner is forbidden.

`SourceFile.snapshot` is the sole authority for text, version, and position
index. `ParsedDocument`, registry metadata, and Program views may retain or
project that handle but may not store independently mutable copies of those
three facts.

One parsed entry can own zero or more immutable bind variants. The bind cache
is keyed by `(ParsedDocument identity, BindKey)`, not by path/version alone;
a bind-affecting option change reuses the parsed entry but publishes a new
`BoundDocument`. A checker-only option change reuses both records and creates
a fresh CheckerSession. No variant overwrites another project/session's
published bind result.

`ProgramBinder` receives `(Arc<ParsedDocument>, Arc<BoundDocument>)` pairs in
Program order and builds lookup views without owning or mutating them. Global
symbol merging and every checker-created clone remain CheckerSession state;
they do not write back into a cached `BoundDocument`.

This removes the current self-reference pressure. H1 no longer needs the
checker crate to allocate sources and binders inside the emit callback. It
needs only a scoped fresh checker borrowing a `ProgramSnapshot` long enough
to expose its internal `EmitResolver`.

### 4.4 Document keys, versions, and release

Registry lookup and source-version identity are separate. Match pinned
`DocumentRegistry` layering rather than flattening everything into one hash:

- the registry instance is the namespace and fixes the TypeScript pin, JSDoc
  mode, canonicalization profile, and any Rust parser policy affecting tree
  shape;
- `DocumentRegistryBucketKey` is generated from the complete pinned
  source-file-affecting option set and is combined with implied node format;
- the bucket map is addressed by canonical path; and
- a per-path variant map distinguishes exact script kind, including host
  overrides and arbitrary extensions.

`DocumentAddress` names that complete registry namespace/bucket/path/script-
kind route and deliberately excludes the text version. It is the address in
`ParsedDocument`; it is not a public cross-registry identity.

External-module-indicator policy is included wherever the generated source-
affecting option projection places it; it is not an ad hoc secondary key.
Projects using the same registry address and version share the same document
even when they supplied distinct snapshot objects; this is required by the
pinned `documentRegistry` unit test. The version is therefore a host promise
that equal `(address, version)` means equal text, not part of the bucket key. A
debug host-contract assertion may compare text, but production may not
silently fork same-version documents and call that tsc parity. Divergent
unsaved overlays require separate registry namespaces or distinct virtual
paths.

Each published entry obtains its current `DocumentVersion`, text, position
index, and lineage through `ParsedDocument.source.snapshot`; registry metadata
does not keep independently mutable copies. On a new version, an incremental
change range is accepted only when the supplied snapshot's
`SnapshotLineage` proves ancestry from that exact old snapshot/store;
otherwise the entry performs a full parse. Text hashes are accelerators and
exact text is the collision check for content interning or an external cache,
never a replacement for version/lineage rules.

`acquire`, `update`, and `release` are explicit and reference-counted. Zero
references remove an entry unless a separately bounded orphan/open-file
policy owns it. A CLI invocation uses an ephemeral store dropped with
`ProgramSession`; it must not create a hidden process cache.

Cache metadata may be synchronized, but published text, parse, and bind
records are immutable. No checker `RefCell`, lazy link table, mutable emitter
hook, or request-local cancellation state is stored in the registry. The
future concurrency contract can therefore serialize tsc-compatible service
updates or permit immutable snapshot readers without making the parse tree
interior-mutable by accident.

### 4.5 Program snapshot and one-shot adapter

`ProgramSnapshot` owns ordered document handles, ephemeral `SourceFileId`
indexes, root/library/package/config facts, and the exact resolution table for
one Program version. `SourceFileId` remains useful inside that snapshot but is
never a document-cache identity.

The existing loader and `ProgramSession::run` become one-shot adapters:

```text
load PreparedProgram
  -> create ephemeral identity/text/document store
  -> acquire/parse/bind documents
  -> publish ProgramSnapshot
  -> create fresh CheckerSession
  -> produce the unchanged NoEmitOutcome
  -> drop the complete store
```

This route must retain H0 diagnostic, host-operation, ordering, and exit
behavior. It also remains emitter-free. `Arc`, leases, and cache-capable types
are allowed; a global registry, retained entry, background task, or emitter
constructor is not.

The ephemeral `DocumentStore` uses direct session-owned slots for this route;
it does not need synchronized lookup/refcount machinery when no second
Program snapshot is requested. The reusable registry implementation is
exercised separately by the two-snapshot L0 proof. Both implement the same
acquire/publish view seen by `ProgramSnapshot`.

L0.0 freezes a same-runner relative latency/RSS/allocation policy before this
route changes, in addition to the existing absolute H0 ceilings. The L0
candidate must meet both. Replacing full-text projection copies may improve
the result, but unused headroom may not be spent on locks, version trees,
registry statistics, or per-node indirection. H0 calls full parse with no
syntax cursor and never enters the L1 update API.

## 5. L1 incremental parser

After L0 can reuse an unchanged parsed/bound document, port the pinned parser
owners in this order:

1. snapshot change-range validation and `extendToAffectedRange`;
2. old-tree range adjustment and intersecting-element marking;
3. `createSyntaxCursor` highest-list-element lookup;
4. parser `currentNode`, reusable parsing contexts, `canReuseNode`, and
   `consumeNode` hooks in ordinary list parsing;
5. copied-subtree relocation, parent/error restamping, JSDoc attachments,
   directives/pragmas, external-module facts, and top-level-await reparse; and
6. `createLanguageServiceSourceFile`/`updateLanguageServiceSourceFile`
   snapshot/version fields and disposal behavior.

The Rust-native path keeps published source versions immutable. A reusable
old subtree is copied into the new arena, receives the new arena's IDs and
shifted byte ranges, and records test-only reuse lineage. The changed file is
bound again; unchanged `Arc<BoundDocument>` entries are reused wholesale.
No checker, transform, printer, or generated-name state follows a subtree.

Copy-on-reuse is correct but remains O(number of copied nodes), unlike tsc's
object reuse. L1 therefore has a mandatory large-file edit benchmark. If the
copy cost misses the editor latency/memory target, stop before H1 and choose a
linear consumed-old-source fast path or a chunked/persistent arena design.
Do not defer that representation decision until Language Service queries or a
public AST have been built on top.

For a Rust-native LSP product, structural/diagnostic/protocol equality and
measured reuse are the contract; old and new public node objects need not be
identical. A JavaScript-compatible `typescript` API is stricter because the
pinned implementation visibly reuses and mutates node objects. It needs a
separate object-identity/mutation binding contract and cannot claim parity
from reuse-lineage counters.

## 6. L2 Program and resolution reuse

L0/L1 are not old-Program reuse by themselves. L2 ports:

- `DocumentRegistry` bucket overlap, script-kind variants, acquire/update/
  release counts, orphan/open-file policy, and statistics;
- `isProgramUptoDate` and the three `StructureIsReused` outcomes;
- unchanged whole `ParsedDocument`/`BoundDocument` identity across Program
  versions;
- root, option, project-reference, missing-file, import/directive, package,
  automatic-type, and lib comparisons;
- module/type/lib resolution reuse only when the containing document identity
  is unchanged and the corresponding invalidation set is clear;
- per-directory/package-json/failed-lookup/config caches with watcher-owned
  dependency sets and explicit invalidation; and
- old Program/resource release after the new snapshot has been published.

The current `PreparedProgram::resolutions()` remains the immutable observation
for one snapshot. The reusable cache lives above `ModuleResolver`; it is
project/service-owned, versioned, bounded, and invalidated. Extending the
resolver's current per-run `package_cache` lifetime without dependency
tracking would return stale results and is forbidden.

## 7. Later product tracks

- **L3 — Language Service:** semantic, partial-semantic, and syntactic modes;
  diagnostics/classifications; definitions/references/navigation;
  completions/auto-imports; quick info/signature help; rename; formatting;
  code fixes/refactors; hierarchy/inlay hints; source mapping; and per-file
  emit. FourSlash and service unit tests own this layer.
- **L4 — tsserver/Project Service:** configured/inferred/external projects,
  open-file overlays, background diagnostics, watches/timers, request-ID
  cancellation, preferences, plugins, logging/telemetry, package install,
  automatic type acquisition, and typings installer. Server/project-system
  unit and event baselines own this layer.
- **L5 — LSP adapter:** a separate product mapping capabilities, URI/path,
  UTF-16 positions, synchronization, requests, cancellation, diagnostics,
  workspace changes, progress, and errors onto engine APIs. TypeScript does
  not provide this protocol, so it requires its own protocol tests.

Cancellation safe points enter the fresh CheckerSession before L3. A no-op
hook may reserve call shape but is not cancellation support. Cancellation
must discard partial checker/query state without evicting valid immutable
document/program snapshots.

## 8. Required landing order

The recommended order is now:

1. **L0.0 — evidence freeze (complete):** add parse/bind/text-copy counters, current H0
   latency/RSS/allocation measurements and a same-runner relative regression
   policy, large-file edit fixtures, source/options key inventories, and the
   CI lane/receipt/failure-artifact schemas required below;
2. **L0.1 — text ownership (complete):** introduce shared text and accessor-only static
   position indexes, remove full-text projection copies, then port the
   versioned line/snapshot store;
3. **L0.2 — identity leases (complete):** generate node/array/symbol relocation, admit
   non-contiguous owner ranges, and prove release/no-overlap/exhaustion
   behavior;
4. **L0.3 — owned bind state:** split `BinderWorker` from `BindData`, construct
   `ProgramSnapshot`, and make the fresh checker borrow it;
5. **L0.4 — one-shot and registry proof:** move H0 through an ephemeral store,
   add a minimal reference-counted registry, and prove unchanged-file parse
   and bind reuse across two Program snapshots;
6. **L1 — incremental parser closure:** port the owners in section 5 and pass
   the pinned incremental parser, version-cache, Unicode-edit, randomized
   edit, and large-file performance gates;
7. **H1 implementation:** build resolver/transform/printer/output work on the
   stable `ProgramSnapshot -> CheckerSession` boundary; and
8. **L2-L5:** close Program/resolution reuse, then Language Service, tsserver,
   and the independent LSP adapter.

### 8.1 L0.0 frozen record

L0.0 completed on 2026-08-06. The runtime observation is bound to commit
`330827c5808d8fe2ebff9d893b176910d5de9605`; later documentation, policy, and
evidence-file commits do not change that observed runtime tree. Its checked-in
authorities are:

- `CheckWorkCounters` and `NoEmitWorkCounters`, propagated through the
  production check/program/CLI path, expose parsed documents, bound documents,
  full-text projections, and projected bytes. The qualification example alone
  installs the allocation observer; the production binary keeps its allocator
  and entry path unchanged.
- [the source/options inventory](../../../ratchets/l0-source-options.v1.json),
  generated by
  [`l0-option-inventory.mjs`](../../../crates/oracle/l0-option-inventory.mjs),
  binds the vendored TypeScript source-affecting option set, Rust option fields,
  document-address components, parse/bind/semantic/resolution/structure key
  partitions, current text owners, and every full-text projection edge to
  source hashes.
- [the fixture manifest](../../../ratchets/l0-fixtures.v1.json), generated by
  [`l0-fixtures.mjs`](../../../crates/oracle/l0-fixtures.mjs), freezes explicit,
  project, and 32-file scale workloads plus a 1,073,676-byte/1,073,664-UTF-16-
  unit edit source. Its edit replaces 17 bytes/UTF-16 units with Japanese text
  and an astral character, recording distinct 25-byte and 19-UTF-16-unit
  insertion lengths and exact before/after hashes.
- [the performance evidence](../../../ratchets/l0-evidence.v1.json), produced
  and revalidated by
  [`l0-performance.mjs`](../../../crates/oracle/l0-performance.mjs), contains
  one first-process observation and eight warm observations for each workload
  on the approved macOS arm64 runner. `cold` here means the first fresh process
  for that workload after materialization, not an operating-system page-cache
  purge; the existing H0 absolute ratchet retains its own cold-run contract.

| Workload | First process | Warm median / p95 | Peak RSS | Allocations / allocated bytes | Parse / bind / text copies |
| --- | ---: | ---: | ---: | ---: | ---: |
| explicit root | 526.137 ms | 100.784 / 101.771 ms | 99,024,896 B | 1,574,997 / 159,081,958 B | 83 / 83 / 249 |
| project | 96.435 ms | 96.701 / 98.010 ms | 92,323,840 B | 1,503,874 / 151,160,917 B | 67 / 67 / 203 |
| scale | 211.198 ms | 212.066 / 213.623 ms | 177,405,952 B | 5,140,959 / 455,897,892 B | 95 / 95 / 287 |

Future comparisons use at least seven paired samples on the same approved
runner in alternating AB/BA order. Candidate/base ceilings are 1.10 for warm
median wall time, 1.15 for warm p95 wall time, 1.10 for peak RSS, 1.02 for
allocation count, 1.03 for allocated bytes, and 1.00 for every parse, bind,
text-copy, and copied-byte counter. A moving hosted runner cannot mint or relax
these ratios, and the existing absolute H0 ceilings remain independently
required.

The common PR lane, fail-closed selector, stable aggregate, scheduled frozen-
fixture input, and strict receipt/failure-artifact schemas live under
`.github/ci` and `.github/workflows/ci.yml`. Receipt validation requires the
exact candidate/base and immutable inputs plus trusted-runner OIDC or a
registered signer; unsigned files, artifacts, and PR comments are rejected.
L0.0 froze and tested that contract without treating an unsigned summary as
acceptance. L0.1 activated the exact status producer, text-store stress, and
approved-runner comparison described in section 8.2. L0.2 adds identity-range
reclamation and its chained evidence in section 8.3. L0.3 adds owned bind
publication and fresh-checker snapshot borrowing in section 8.4. Registry/
Program reuse and incremental-parser exactness remain L0.4-L1 work.

### 8.2 L0.1 accepted text-ownership record

L0.1 completed on 2026-08-06. Its qualified runtime is commit
`3ed0304704d73312d72ddbf1edfaae103adf8d34`, compared with exact base
`298705ef79525dd50c888af013202b4505520435`. Later evidence and documentation
commits qualify only when their runtime-tree fingerprint remains identical to
that candidate. The accepted ownership boundary is:

- `TextSnapshot` owns one `Arc<str>` and one `Arc<PositionIndex>` together
  with an opaque host `DocumentVersion` and private same-store lineage.
  Prepared/config/auxiliary/package inputs, checker `InputFile`, and syntax
  `SourceFile` retain that exact snapshot Arc; production projection no
  longer clones complete source text.
- Batch H0 snapshots use the accessor-only dense position index and do not
  instantiate the edited tree. The edited representation is an immutable
  persistent line tree with subtree byte, UTF-16, and line counts; local edits
  rebuild the bounded neighboring-line path needed to preserve CRLF while
  sharing untouched subtrees.
- Byte and UTF-16 edit ranges are distinct types. Conversion rejects UTF-8 or
  surrogate midpoints rather than rounding, and line accounting recognizes
  LF, CR, CRLF, LS, and PS while intentionally excluding NEL.
- Scanner dump token offsets remain UTF-16-relative by contract. Binder and
  checker diagnostic helpers convert those deltas through `PositionIndex`
  before slicing byte text; BOM/astral canaries and the 7,691-program encoding
  invariant prevent the old midpoint interpretation from returning.
- `VersionedTextStore` separates host versions from internal ancestry,
  materializes at the TypeScript-compatible ninth-pending-edit or greater-
  than-256-UTF-16 threshold, retains at most eight published ancestors, and
  returns a collapsed change range only for a retained same-store ancestor.
- The parser still borrows a contiguous `&str`; an edited store materializes
  exactly once when it publishes a snapshot. The H0 adapter therefore keeps
  its one-shot parse/bind behavior and dense lookup cost without entering the
  future registry or incremental-parser path.

The checked-in
[source/options inventory](../../../ratchets/l0-source-options.v1.json)
enumerates every snapshot owner, Arc-sharing edge, position representation,
and now-empty production full-text-copy edge. Diagnostics owns deterministic
Unicode/property tests, syntax/checker/compiler own exact Arc identity tests,
and the scheduled `text-stress` authority runs 20,000 byte/UTF-16 edits over
the frozen 1 MiB input with a flat-string/dense-index oracle, bounded history,
and a 512 MiB RSS ceiling. The acceptance run published 1,564 snapshots and
observed 44,580,864 bytes maximum RSS.

The approved macOS arm64 comparison is checked in as
[L0.1 performance evidence](../../../ratchets/l0-text-ownership-performance.v1.json).
It contains one cold plus seven warm pairs per workload in alternating AB/BA
order. Every ratio is candidate/base and remains below its frozen ceiling:

| Workload | Warm median | Warm p95 | Peak RSS | Allocations | Allocated bytes | Candidate text copies / bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| explicit root | 0.994584 | 1.001815 | 1.054921 | 0.999846 | 0.907713 | 0 / 0 |
| project | 0.993084 | 0.994539 | 1.064048 | 0.999873 | 0.906290 | 0 / 0 |
| scale | 0.995711 | 0.990133 | 1.003220 | 0.999944 | 0.963931 | 0 / 0 |

The active policy binds selected L0/L1 and H1 runtime candidates to the exact
unsplit full-gate result, immutable inputs, GitHub OIDC attestation, verified
signer workflow, and final receipt. Protected-main scheduled stress and the
manual approved-runner performance workflow publish only bounded,
content-addressed evidence. L0.2 builds identity leases on this boundary
without exposing snapshot lineage as a public revision or replacing these
authorities with the aggregate hosted sentinel.

### 8.3 L0.2 accepted identity-lease record

L0.2 completed on 2026-08-06. Its qualified runtime is commit
`f03be30d4c581ec432b059b7f133d4439b3b1902`, compared with exact base
`2b814b19902a49ffe8964c0fe9d56ea87687095e`. The base runtime-tree fingerprint
is the accepted L0.1 candidate fingerprint, so the evidence forms a checked
L0.1-to-L0.2 chain. Later evidence and documentation commits qualify only
while their runtime-tree fingerprint remains identical to the L0.2 candidate.
The accepted identity boundary is:

- `IdentityDomain` leases node, node-array, persistent-symbol, and private-name
  serial intervals through either an ephemeral bump policy or a reclaiming,
  coalescing interval policy. Batch reservation is atomic, provisional H0
  reservations seal exactly or cancel, the last lease clone releases a range,
  and all limit, overflow, partition, and allocator failures are typed.
- `NodeArena` and `SourceFile` retain node/array leases. The node schema now
  generates mutable relocation for every ID-bearing node payload alongside
  child visitation, including parents, JSDoc, arrays, roots, external-module
  indicators, and arena bases. Reclaiming parses construct locally and
  relocate after exact reservation; ephemeral H0 parses use sealed final bases
  directly. Forced-nonzero tests prove both paths logically identical.
- Binder publication leases and relocates persistent symbols plus the serials
  embedded in private-name keys. L0.3 now consumes the worker through an
  exhaustive `BinderWorker::into_bind_data` move; symbol tables, links, node
  maps, assigned serials, and ambient-module records all move together.
- `ProgramBinder` accepts independently sorted, non-contiguous node, array,
  and symbol owner intervals, rejects overlaps, unmanaged mixtures, and
  cross-domain Programs, and does not assume bind order. The untagged symbol
  half remains persistent; the high bit identifies checker-session-local
  transient symbols, with typed exhaustion on both sides.
- Every production H0 parse/bind path now supplies a domain. One-shot work
  uses direct ephemeral bases, while process-lifetime prepared libraries retain
  reclaiming leases and release them when the bundle drops.

The scheduled `identity-stress` authority deterministically exercised 10,000
open/edit/close operations with at most 64 active documents across four
projects, TS/TSX/JS/JSON, and eight option variants. It checked non-overlap on
every iteration, ended with zero active ranges, kept maximum bumps at 1,541
nodes, 371 arrays, 488 symbols, and 49 private-name serials, and observed
6,537,216 bytes RSS under the 512 MiB ceiling.

The approved macOS arm64 comparison is checked in as
[L0.2 performance evidence](../../../ratchets/l0-identity-leases-performance.v1.json).
It contains one cold plus seven warm pairs per workload in alternating AB/BA
order. All parse, bind, text-copy, and copied-byte ratios remain exactly 1;
the remaining candidate/base ratios are:

| Workload | Warm median | Warm p95 | Peak RSS | Allocations | Allocated bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| explicit root | 0.986083 | 0.988231 | 1.000969 | 1.005749 | 1.012714 |
| project | 0.993702 | 0.985400 | 0.999834 | 1.004077 | 1.008805 |
| scale | 0.989254 | 0.991974 | 0.999817 | 1.002155 | 1.005240 |

L0.3 is complete: worker state is split from owned `BindData`,
`ProgramSnapshot` retains ordered Arc handles, and each fresh checker borrows
that snapshot without weakening this lease or qualification boundary.

Thus full Language Service work should not precede H1, but L0 and L1 should.
Completing them first is cheaper than retrofitting persistent identities,
owned bind state, and editable text beneath an already-landed emitter.

### 8.4 L0.3 accepted owned-bind-state record

L0.3 completed on 2026-08-06. Its qualified runtime is commit
`d78bf23f73b341e0a7ba840367f515b5ec521e04`, compared with exact base
`f03be30d4c581ec432b059b7f133d4439b3b1902`. The base runtime-tree fingerprint
is the accepted L0.2 candidate fingerprint, so the evidence forms a checked
L0.2-to-L0.3 chain. Later evidence and documentation commits qualify only
while their runtime-tree fingerprint remains identical to the L0.3 candidate.
The accepted ownership boundary is:

- `BinderWorker` is the concrete borrowed walk worker and `Binder` remains a
  compatibility alias. `BinderWorker::into_bind_data` exhaustively moves the
  completed checker-facing tables, flow graph, flags, diagnostics, and leases
  into `BindData`; container cursors, active labels, delayed queues, and all
  other walk state are discarded before publication.
- `ParsedDocument` owns an `Arc<SourceFile>` and `BoundDocument` pairs it with
  the owned `BindData`. `ProgramSnapshot` owns the ordered `Arc` document
  handles and library-prefix boundary. It validates identity domains and
  independently sorted non-contiguous node/array/symbol owner intervals when
  the checker view is constructed.
- `ProgramBinder::from_snapshot` borrows only the snapshot's immutable handles.
  `CheckerState::from_snapshot` creates fresh transient symbols and checker
  caches for every session; no links, flow results, or checker arena migrate
  between sessions. Production fixture binds and process-lifetime library
  bundles publish `BoundDocument` records before checking.
- Legacy raw-Binder unit-test adapters remain available, but the production
  checker path no longer stores a `BinderWorker` in `ProgramBinder` or
  `LibBundle`. Library cache reuse shares the same parsed/bound Arc handles.

The focused checker program tests prove worker publication, snapshot source
identity, two fresh sessions over one snapshot, and session-local transient
symbol arenas. The complete checker library suite (1,540 tests) and binder
library suite (71 tests) remain green.

The approved macOS arm64 comparison is checked in as
[L0.3 performance evidence](../../../ratchets/l0-owned-bind-state-performance.v1.json).
It contains one cold plus seven warm pairs per workload in alternating AB/BA
order. Parse, bind, full-text-copy, and copied-byte counters remain exactly
equal to the L0.2 base (and full-text copies remain zero); the candidate/base
ratios are:

| Workload | Warm median | Warm p95 | Peak RSS | Allocations | Allocated bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| explicit root | 0.992898 | 0.960340 | 1.001290 | 1.000158 | 1.000731 |
| project | 0.999751 | 1.000813 | 1.000998 | 1.000134 | 1.000622 |
| scale | 1.010421 | 0.943605 | 1.000639 | 1.000056 | 1.000257 |

L0.4 implementation is landed: the one-shot H0 adapter publishes completed
binds through an ephemeral document store, and the minimal registry proves
refcounted unchanged-file parse/bind reuse across two Program snapshots.
Approved-runner qualification and the corresponding chained performance
receipt remain required before this becomes an accepted runtime record.

### 8.5 L0.4 implementation record

The L0.4 implementation adds the following ownership boundaries:

- `EphemeralDocumentStore` owns the identity domain and direct immutable
  `BoundDocument` slots for one H0 run. It publishes only completed bind data,
  then transfers the slots into `ProgramSnapshot`; it is not global and does
  not retain a version after the session drops.
- `DocumentAddress` includes the registry namespace, path, script kind, full
  source/bind option bucket, and module-format facts. The synchronous
  `DocumentRegistry` keeps one entry per live `(address, host version, text)`
  variant, rejects equal-version text replacement, and removes a variant when
  its final explicit lease is released.
- registry metadata retains the source's `Arc<TextSnapshot>` rather than a
  second text or position-index projection. A new version may coexist with an
  older live Program, while a same-version request reuses the exact
  `Arc<BoundDocument>`.
- the production checker path now moves fixture bind workers through the
  ephemeral store before creating its fresh checker. H0's library bundle is
  still the separately authorized harness-only cache; production sessions do
  not acquire a global document registry.

The focused Program tests construct parse and bind records inside the registry
builder and observe one parse/bind for two unchanged ProgramSnapshots, two
after a new version, exact Arc reuse, equal-version text rejection, and zero
active entries after all releases. The compiler contract suite remains green
with the H0 diagnostic and work-counter behavior unchanged. These are local
implementation proofs; the approved-runner qualification is intentionally
still pending.

## 9. Evidence and tests

Pinned upstream owners to import or mirror are:

| Source | Required evidence |
| --- | --- |
| `src/testRunner/unittests/incrementalParser.ts` | Fresh/incremental structural equality, exact parse diagnostics, invariants, reusable-context controls, comments/JSDoc, regex/lookahead/context changes, and reuse counts/lineage |
| `src/testRunner/unittests/services/documentRegistry.ts` | Cross-project sharing, source-affecting option buckets, version updates, and acquire/update sequencing |
| `src/testRunner/unittests/tsserver/versionCache.ts` | Line-index edits, unusual line endings, position conversion, range reads, and randomized/stress edits |
| `src/testRunner/unittests/tsserver/textStorage.ts` | Text-to-version-cache transitions, line/offset equality, reload, large-file, and file-size behavior |
| `src/testRunner/unittests/tsserver/documentRegistry.ts` | Orphan entry reuse/change, project ownership, and script-kind changes |
| `src/testRunner/unittests/reuseProgramStructure.ts` | `StructureIsReused` transitions, `isProgramUptoDate`, root/option/source changes, missing files, and old-Program release behavior |
| `src/testRunner/unittests/tsserver/resolutionCache.ts` and `tscWatch/resolutionCache.ts` | Successful/failed lookup reuse, invalidation, package/type changes, project-reference sharing, and watcher interaction |

Rust owners should remain adjacent to the implementation:

- `crates/diagnostics`/text-store tests for byte/UTF-16 conversions,
  versioned line indexes, and host-version versus snapshot-lineage
  separation;
- `crates/syntax` tests for relocation, cursor/reuse rules, fresh-versus-
  incremental trees, and malformed/random edit scripts;
- `crates/binder` tests for owned `BindData`, stable symbol identity, tagged
  checker-transient separation, private-name serial stability, bind-key
  changes, document-local flow ownership, exhaustion, and all-or-nothing
  publication;
- `crates/checker` tests for a fresh session per Program version, owner-paired
  `(document, FlowId)` lookup/cache keys, lazy same-version query reuse, and no
  checker/flow-result migration to a replacement Program;
- `crates/program` tests for registry keys/refcounts, parse/bind option
  variants, Program snapshots, structure states, and resolution invalidation;
- `crates/compiler` tests proving the H0 adapter is behaviorally identical and
  leaves no retained registry state; and
- long-running integration tests for repeated open/edit/close, multiple
  projects/options/script kinds, cancellation, range reclamation, and bounded
  RSS.

### 9.1 CI and qualification topology

L0/L1 use the same four authorities as H1, but select persistent-program and
edit-specific evidence:

1. **Required PR guardrail.** Every non-documentation change runs formatting,
   a locked all-target workspace check, and focused tests selected
   fail-closed from the owner graph. L0/L1 changes select byte/UTF-16 boundary
   tests, text/snapshot/version contracts, forced-nonzero identity relocation,
   owner-range exhaustion/reclamation, parse/bind option variants,
   document-owned flow keys, deterministic fresh-versus-incremental cases,
   and the unchanged H0 adapter/no-emit canaries. Small fixed seeds and
   checked-in fixtures keep this lane bounded.
2. **Exact merge qualification.** The unsplit full gate runs against an exact
   base commit, and its machine-readable summary is bound to HEAD, base,
   toolchain and Node pins, `Cargo.lock`, suite/oracle inventories, commands,
   profiles, and result hashes. A new commit, moving base, changed pin, or
   missing/unknown lane invalidates the summary. Merge-queue composition is a
   new candidate and must not reuse a PR-head result. Only the trusted-runner
   or registered-signer authentication defined by the
   [cross-track CI contract](compiler-compatibility-residual.md#114-cross-track-ci-and-qualification-topology)
   may post the required status; a hash or PR comment alone is insufficient.
3. **Scheduled stress.** At L0.2, protected-main runs long deterministic
   randomized byte/UTF-16 edit scripts plus repeated open/edit/close and
   multi-project identity churn, option and script-kind changes, range
   reclamation, bounded snapshot history, and bounded RSS. L0.4-L1 extend that
   authority with registry reuse, cancellation, and fresh-versus-incremental
   exactness. A failure publishes a bounded reproducer
   containing the available initial-text hash, ordered edits, seed, option/
   version keys, reuse counters, owner ranges, diagnostics, and resource
   observations.
4. **Approved-runner performance.** The H0 relative guard and L1 large-file
   edit latency/allocation/RSS qualification run with alternating baseline and
   candidate samples on the frozen runner/profile. A moving hosted image may
   smoke the behavior but cannot mint or relax a performance ratchet.

L0.2 has extended the schema-bound fail-closed classifier and common non-
document format/locked-all-target lane with exact identity-owner tests,
scheduled 10,000-operation reclamation stress, a chained approved-runner H0
comparison, and strict bounded failure evidence while retaining the L0.1
Arc/text-store authorities. A green `gates` sentinel or unsigned summary remains
insufficient: a selected runtime candidate requires the exact qualification.
Windows selection expands in later slices with program, registry, path,
toolchain, and compiler adapters that exercise platform-specific paths or file
identity. Third-party Actions use reviewed full commit SHAs and Cargo
resolution is locked.

Language Service, tsserver, and LSP later add their query, protocol,
cancellation, event-ordering, and platform matrices to this topology; their
future checks are not substituted for the L0/L1 engine gates, and L0/L1 does
not claim those products merely by reserving their lanes.

Acceptance requires all of these quantitative observations:

- an unchanged Program refresh performs zero parses and zero binds;
- a one-file edit reparses and rebinds only that document unless an explicit
  source-affecting key changes;
- a bind-only option change reuses parsed identity but selects/rebuilds the
  correct bind variants, while a checker-only change reuses both document
  layers and creates a fresh checker;
- every unchanged parsed/bound entry retains the same internal Arc identity;
- fresh and reused Programs produce exact source order, resolutions,
  diagnostics, and later emit bytes;
- incremental and fresh parse trees/diagnostics are exact, with reuse lineage
  meeting the pinned case expectations or an explicitly adapted Rust metric;
- open/edit/close returns registry entries and identity ranges to the declared
  bound, with no harness-style leak;
- large-file edit latency and allocation meet the L1 target; and
- H0 cold/warm latency, peak RSS, host observations, diagnostics, exits, and
  emitter-constructor/write-zero evidence remain within their frozen gates.

## 10. Stop conditions

Stop and amend this design if:

- cached documents are rebased into every new Program order;
- a Program combines overlapping ID ranges or entries from unrelated identity
  domains without explicit relocation;
- a persistent `SymbolId` enters the checker-transient partition, a transient
  ID is written into `BoundDocument`, or private-name serials are reseeded by
  new Program order;
- a raw `FlowId` is compared or cached without its owning bound document/file,
  or a checker flow result migrates to a new Program version;
- raw Node/Symbol IDs become serialized cache keys or public protocol values;
- `Binder<'a>` or checker links are stored through a self-referential or leaked
  registry object;
- a global cache retains arbitrary source versions after project/session
  release;
- a version or option-key change reuses stale parse/bind/resolution state;
- a host `DocumentVersion` is used as proof of snapshot ancestry, or a text-
  store revision is exposed as the registry's host-version contract;
- `SourceFile`, `ParsedDocument`, and registry metadata can disagree about
  one document version's text or position index;
- UTF-16 edits are applied directly to byte ranges or invalid boundaries are
  rounded;
- a one-character edit rebuilds a per-byte position table for every unchanged
  file;
- copy-on-reuse misses the L1 large-file budget and H1 proceeds before the
  representation is corrected;
- an L0/L1 runtime change can merge from classifier/platform success alone,
  from a full-gate claim not bound to its exact HEAD/base, or without the
  selected incremental owner tests;
- reused parsed nodes carry checker, transform, printer, or generated-name
  state; or
- FourSlash, tsserver, and LSP results are substituted for one another.

## 11. Relationship to H0, H1, build, and public API

L0 changes internal ownership used by H0, so it must requalify H0 before H1
starts. It does not change H0's `--noEmit` product scope or permit a cache to
survive the CLI session. L1 existing in the workspace does not make
incremental parsing part of the H1 compatibility claim.

H1 may borrow immutable parsed/bound entries and the fresh checker resolver;
its transform/printer/output state remains session-local. A later builder can
reuse `ProgramSnapshot` and resolution-cache seams, but `.tsbuildinfo`,
affected-file queues, and watch scheduling remain separate.

A Rust-native LSP can retain immutable snapshots. A JavaScript-compatible
public `typescript` API must additionally decide how to expose tsc's
in-place-updated/reused node object identity. Neither product may expose the
compact internal IDs as source positions or stable cross-version handles.
