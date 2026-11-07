# SPEC-gen2 — the record generator v2: node tails and checker records from layout JSON (phase 0.3)

Revision 2 (2026-11-07, after the reviews): the checker phase (A) now comes before the front-end phase (B), so **deliverable 2
(checker/binder records) is the priority and deliverable 1 (node tails) may stop at a compiling preview**; the Type header carries
a dense per-checker `id`; the REPORT must give a per-kind byte table of Type tails with B-3 occupancy; hot zero-copy accessors are
required, and `data()`'s cost is to be priced by the integrator (ceiling (iv) of SPEC-0b).

Track: [ts71-arch-arena](../README.md). Depends on [SPEC-arena](SPEC-arena.md) only through the crate API named there. The arena
crate is being written by another lane at the same time, so **do not depend on it**: give the preview crate a small safe stub
`crates/records-preview/src/arena_stub.rs` (Vec-backed `Record`, `Ref<T>`, `Span<T>`, `List<T>`, `ArenaCell<T>`, a bump `Arena`
with the same method names as SPEC-arena) that the generated code compiles against; the integrator swaps the stub for `tsc_arena`
at integration. Keep the stub under 300 lines and say in `STATUS.md` which SPEC-arena signatures it mirrors.
This lane migrates **no consumer**: it proves the generator on a preview crate; phase 1 switches the real crates to its output.

## Lane rules

Same as SPEC-arena: no commits (patches under `target/lane-gen2/patches/`, `STATUS.md`), `nice -n 20 cargo … -j 2`,
`--test-threads=1`, one cargo command at a time, build/test only `-p tsc-rs-records-preview` (and `-p tsc-rs-arena` if needed),
no network, no benchmarks, claims with runnable demonstrations, time box three working days. Changes only under
`scripts/generate_records.py`, `crates/syntax/nodes.layout2.json`, `crates/records.layout.json`, `crates/records-preview/`,
the root `Cargo.toml` members line and `Cargo.lock`. Do not touch `scripts/generate_nodes.py`, `crates/syntax/nodes.layout.json`
or any generated file in `crates/syntax/src` (the live compiler keeps building from them until phase 1).

## What to read first

`scripts/generate_nodes.py` (the v1 generator: its `Kind`/`Field` classes, `render_payload_codec`, `render_nodes_rs`,
`render_for_each_child_rs`, `render_identity_walk`; reuse its parsing and rendering helpers by importing it as a module),
`crates/syntax/nodes.schema.json` (195 kinds: `kindName`, `dataName`, `fields[{name, rustName, type, optional, child}]`,
`children[{name, array}]`), `crates/syntax/nodes.layout.json` (v1: `record`, `stringTypes`, `rustOptional`, `visitOrder`,
`headerObservables`, `equalitySkips`, `fieldDocs`, `hotSlots`), `crates/syntax/src/nodes.rs` (the shape consumers depend on:
`Node`, `NodeData` with owned `XxxData` structs, `data()`, `as_xxx()`, `missing()`), `crates/syntax/src/for_each_child.rs`,
`crates/syntax/src/observable_fields.rs`, the design document §0 and §2 (段 0.3) and §3 (段 1.2, 1.4) for the target shapes,
tsgo's `internal/ast/ast.go` under `target/typescript7/upstream/tsc/` (which node structs embed `FlowNodeBase`,
`DeclarationBase`, `LocalsContainerBase`, `FunctionLikeBase`, `ModuleDeclaration`… — the authority for which kinds carry which
binder fields), and the current record definitions the JSON must transcribe (file:line in deliverable 2).

## Deliverable 1: `crates/syntax/nodes.layout2.json` and the node records

Vocabulary (every entry carries a one-line `why`, as v1 does):

- `record`: `{ "headerBytes": 24, "handleUnitBytes": 8 }`. Header fields in order: `kind: u16`, `tflags: u16` (literal/token facts
  as today's `literal_flags` plus `HAS_JSDOC`), `flags: u32` (NodeFlags; the binder writes it in place), `pos: u32`, `end: u32`,
  `parent: Option<NodeId>`. Word 5 belongs to the kind's tail layout.
- `tails.<Kind>`: the tail word list in order. Default derivation (the generator does it; the JSON only records overrides):
  every schema field is one word (`Node`/`NodeArray` → `Option<NodeId>`/`Option<ListId>`; text → `TextRef` u32; scalars —
  bools, `SyntaxKind` operators, numeric facts — packed into one `scalars` word), ordered by v1's hot-slot rule (`hotSlots`),
  then the kind's binder words. Total record bytes = 24 + 8 × ceil(words / 2) when the first tail word is word 5… make the
  rule explicit in the generator and assert the size per kind.
- `binderFields.<Kind>`: derived from tsgo's `ast.go` embeds, written out by the generator into `binder-fields.derived.json`
  (kind → Go struct name → fields) so the integrator can audit it: `flow` (FlowNodeBase), `symbol` (DeclarationBase),
  `locals` + `nextContainer` (LocalsContainerBase), `endFlow` + `returnFlow` (FunctionLikeBase / bodies), `possiblyExhaustive`
  (SwitchStatement), `fallthroughFlow` (CaseClause/DefaultClause), `flowWhenTrue/False` where tsgo keeps them, the module's
  `externalModuleIndicator`/`commonJsModuleIndicator` on SourceFile. Where tsgo keeps a binder fact in a per-file map rather than
  on the node, say so in the derived JSON and keep it out of the tail.
- `jsDoc`: no field; `HAS_JSDOC` in `tflags`; the per-file side map is the parser's business (phase 1).
- `texts`: `TextRef` is a u32: tag bit 31 = 0 → an index into the file's source-span table (pos/len pairs for escape-free string
  values); 1 → an arena `Ref<Bytes>`. Only `StringLiteral`/`NoSubstitutionTemplateLiteral`/template parts/`Identifier` escaped
  text/numeric/regex/JsxText texts use it (list them under `stringTypes` as v1 does).
- `rare`: optional per kind: words that are present on < 1% of nodes (from `H/node-layout/vscode-counts.json` field masks) may
  go to a `Option<Ref<RareTail>>` word instead of inline; the generator emits the rare struct and transparent accessors. Use it
  only where it saves ≥ 8 B on the common path (CallExpression's `questionDotToken` + `typeArguments`, for example); record the
  decision per kind in the JSON.

Generated Rust (into `crates/records-preview/src/nodes.rs` for phase 0; the same renderer targets `crates/syntax/src/nodes.rs` in
phase 1):

- `#[repr(C)] pub struct Node { kind, tflags, flags, pos, end, parent, w5 }` with `unsafe impl Record`.
- `#[repr(C)] pub struct <Kind>Tail { … }` per kind, `unsafe impl Record`, `const _: () = assert!(size_of::<…>() == N)`.
- `pub enum NodeData { Token, <Kind>(<Kind>Data), … }` and `pub struct <Kind>Data { … }` **unchanged in shape from v1**
  (owned decoded structs with the same field names and types: `Option<NodeId>`, `Option<ListId>`, `TextId`→`TextRef`, scalars),
  except that boxed payloads are no longer boxed. `Node::data(&self) -> NodeData` decodes from the tail; `as_<kind>()` as today;
  `missing()` as today.
- Zero-copy access for the hot paths: `Node::tail_<kind>(&self) -> &<Kind>Tail` (kind-checked, panics in debug on mismatch).
- Builders on a `FileArena` facade (preview: a struct wrapping `tsc_arena::SharedArena`): `alloc_<kind>(pos, end, flags, &<Kind>Data)
  -> NodeId`, `alloc_token(kind, pos, end) -> NodeId`, `alloc_list(pos, end, has_trailing_comma, modifier_flags, &[NodeId]) ->
  ListId`, `node(id) -> &Node`, `node_mut(id) -> &mut Node` (unfrozen only), `list(id) -> NodeArray<'_>`.
- `for_each_child` and the child-slot walks (`try_visit`, `child_slots`, `apply_child_slots`) reading tails directly, with v1's
  `visitOrder` conditionals.
- `observable_fields` against the tails.
- `binder_fields` accessors: `flow(&self) -> Option<FlowId>`, `set_flow(&mut self, …)`, `symbol()`, `locals()`, … generated only for
  the kinds that have them, plus a kind-independent `node.flow()` that returns `None` for kinds without the word (one match on
  kind; keep it `#[inline]`).

## Deliverable 2: `crates/records.layout.json` and the checker/binder records

Transcribe the field lists from the current definitions and lay them out as the design says; flag anything you cannot place in
`STATUS.md` with the reason. Sources (main `aa1992224`): `crates/binder/src/symbols.rs:29-95` (Symbol, SymbolExtras),
`crates/binder/src/flow.rs:55-76` (FlowNode, keep 16 B), `crates/types/src/ty.rs` (`TypeData` variants and the 40 B `Type`),
`crates/checker/src/links.rs:294-330` (NodeLinks + cold), `:450-530` (SymbolLinks, PersistentSymbolLinks + cold), `:767-830`
(TypeLinks + cold), `crates/checker/src/state.rs:94-144` (Signature 136 B), `:179-200` (IndexInfo), `:326-370`
(ResolvedMembers/MembersExtras), `crates/checker/src/instantiate.rs:100-140` (TypeMapper), `crates/checker/src/inference.rs:64-150`
(InferenceInfo, InferenceContext). tsrs's shapes for comparison: `target/claude-tools/handover/perf-tsrs/tsrs-layouts-report.md`.

Record families and the target shapes (sizes are the design's; adjust only with a reason in `STATUS.md`):

| family | shape |
| --- | --- |
| `Symbol` (shared, file arena) | 32 B: `flags u32, name u32, parent Option<SymbolId>, value_declaration Option<NodeId>, decl0 Option<NodeId>, more Option<Span<NodeId>> (8 B), tables Option<Ref<SymbolTables>>` — count the words; if they exceed 32 B say which goes to the tail |
| `SymbolTables` (shared tail) | `members, exports, global_exports: Option<TableId>`, `export_symbol`, `const_enum_only_module: u8`, `is_replaceable_by_method: u8`, `assignment_declaration_members: Option<Span<(NodeId, NodeId)>>` |
| `SymbolTable` (shared) | `ListHeader{len, flags}` + entries `(name u32, symbol u32)` 8 B; `flags` says whether a hash index follows the entries (entries > 16) |
| `TransientSymbol` (local) | two shapes by a flag: instantiated 32 B `{flags, origin: SymbolId, check_flags, target, mapper, type_of_symbol: ArenaCell, declared_type: ArenaCell, name_type}`; full = `Symbol` 32 B + links tail 24 B |
| `Flow` (shared) | today's 16 B record, `FlowId = Ref<Flow>`, label lists `Span<FlowId>` |
| `Type` (local) | 24 B header `{flags, object_flags, id (dense per-checker creation number; tsgo `t.id`; the API's type handle), symbol, alias: Option<Ref<Alias>>, w5}` + per-kind tail carrying the `TypeData` payload AND the `TypeLinks` fields of that kind (cells), per the design §2.1; where a kind's tail would exceed today's 40 + 24 B, record it in REPORT.md and keep those links fields out of line (a links table for that kind) — decide per kind from the byte table, not globally; `Alias` record `{symbol, type_arguments: Option<TypeListId>}` |
| `TypeList` (local) | `Ref<List<TypeId>>` |
| `Signature` (local) | 48 B `{flags, min_argument_count, declaration, parameters: Span<SymbolId>, type_parameters: Option<TypeListId>, this_parameter, resolved_return: ArenaCell, target, mapper, rare: Option<Ref<SignatureRare>>}` + `SignatureRare` with the caches, composite kind/signatures, optional-call pair, isolated kind/type, from_method if it does not fit the flags |
| `StructuredMembers` (local) | 32 B `{members: TableId, properties: Span<SymbolId>, call: Span<SignatureId>, construct: Span<SignatureId>, index_infos: Span<IndexInfoId>}` |
| `IndexInfo` (local) | 20 B `{key_type, value_type, declaration, components: Option<Span<NodeId>>, flags u8 (readonly, enum-number marker, any-base marker)}` |
| `Mapper` (local) | 16 B `{kind u8 + pad, a u32, b u32, c u32}` covering Simple/Array/Deferred/Function/Composite/Merged; lists as `Span` |
| `InferenceContext` / `InferenceInfo` (local) | 48 B / 24 B per the design, with `Span` for the vectors and two inline candidate slots per direction + overflow `Option<Span>` (B-3 census: 99.88% ≤ 2) |
| `NodeLinks` (local, page table) | 16 B as today; `PersistentSymbolLinks` 12 B as today; the cold maps stay maps (list them, do not generate) |

Generated Rust (into `crates/records-preview/src/records.rs`): `#[repr(C)]` structs with `unsafe impl Record`, `ArenaCell` for the
fields the design marks mutable, `const` size assertions, `Default`/zero constructors, and a `memory_rows()` function listing
`(family, bytes_per_record)` for the memory report.

## Deliverable 3: the preview crate and its tests

`crates/records-preview` (package `tsc-rs-records-preview`, workspace member; deleted in phase 1): depends on `tsc-rs-arena` and
`tsc-rs-types` (for `SyntaxKind`, flags). Tests:

1. Per kind (all 195 + tokens): build a node through the builder from a `<Kind>Data` with every field set to a distinct dummy
   handle, decode with `data()`, compare equal; size assertion holds; `for_each_child` visits exactly the schema's children in
   order (compare against a list derived from `nodes.schema.json` in the test).
2. `observable_fields` yields the schema's fields in order for three kinds.
3. Binder-field accessors: `flow()` on Identifier reads back what `set_flow` wrote; kinds without the field return `None`.
4. Records: size assertions for every family; a round trip per family; `TransientSymbol` instantiated-shape reads name/
   declarations through `origin`.
5. The generator is deterministic: `scripts/generate_records.py --check` exits 0 after a fresh `generate`; running it twice
   produces identical files.

## Deliverable 4: `REPORT.md`

Per node kind: words, record bytes, rare tail decision, binder words with the tsgo struct that justifies them; the VS Code
byte total of the AST under this layout using `target/claude-tools/handover/perf-tsrs/node-layout/vscode-counts.json`
(`kinds.<Kind>.count`, scaled by 1.033 to tsc-rs's node count as `node-layout-report.md` does), split into header, tails, rare
tails, lists (`3,119,430` lists, `5,407,345` items on VS Code per `handover-2026-11-03.md` §4.2); the checker/binder record
families with bytes per record; and for `Type`, a per-kind table (kind, payload words, links words, tail bytes, record bytes,
VS Code population from `target/claude-tools/handover/perf-tsrs/b3-census/` and the memory report rows, bytes today = 40 + 24,
bytes proposed) so the integrator can see where a tail would grow a kind (review M5). This is the integrator's phase 0.5 input, so keep the arithmetic reproducible (a script under
`crates/records-preview/tools/`).

## Out of scope

Touching the live generator or any live crate; migrating the parser/binder/checker (phase 1/2); the checker's cold maps; JSDoc
side maps; wasm32 specifics beyond compiling.

## Acceptance (integrator)

Preview crate tests green under the load rules; `--check` deterministic; `binder-fields.derived.json` audited against three
kinds by hand (Identifier, FunctionDeclaration, SourceFile); `REPORT.md` arithmetic reproduced from the script.
