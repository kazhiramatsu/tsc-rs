# SPEC-gen2 phase 0.3 status — 2026-11-07

Base: `aa1992224`. No commits, consumer migration, network access or benchmarks.
Checker/binder generation (priority A) is implemented as an isolated layout proof.
Front-end B is the permitted compiling preview, with the explicit integration gaps below.
Integrator acceptance is **not complete**: the supplied census lacks per-TypeData-kind populations.

## Delivered

- `scripts/generate_records.py` imports v1's `Kind`/`Field`/layout loader,
  scalar codec, conditional child-order renderer, snake_case and rustfmt helpers.
  Its seven outputs are confined to `crates/records-preview/`. It never writes the
  live generator, v1 layout or syntax source. `nodes.layout2.json` stores v2 rules;
  `records.layout.json` carries source field/type/line transcriptions, explicit
  destinations, family layouts, Type-kind payload/links choices and cold-map inventory.
- Generated repr(C) families include Symbol/Tables/TableEntry, both transient
  shapes, 16 B Flow and reduce payload, 24 B Type header with dense creation `id`,
  20 Type tails/combined records, Alias/TypeList, Signature/Rare,
  StructuredMembers, IndexInfo, Mapper, InferenceInfo/Context and auxiliary records.
  Every generated record has size/alignment/no-Drop assertions and zero defaults.
  `memory_rows()` includes each generated checker family and Type allocation shape.
- `CheckerArena` assigns dense ids across all 20 Type allocation methods; raw bump
  handles can have gaps. Hot `TypeKindRecord::tail()` borrows the resident tail;
  cells retain vacant/resolving/resolved-none/resolved-handle distinctions.
  Zero-sized !Sync markers keep Type header and raw-tagged Mapper handles local too.
- Mapped: fully inline 72 B; common record 64 B plus a 12 B cold block (16 B
  allocated). Conditional: fully inline 80 B; common 64 B plus 16 B cold.
  Hot resolved_members remains inline. The extra allocation does not make the
  populated total smaller; REPORT shows both costs. Other kinds stay inline.
- 195 node Data/Tail/Record shapes plus tokens. Data enums contain owned structs
  without boxed variants. Typed records decode `data()`, expose borrowed `tail()`,
  and implement direct child_slots/apply_child_slots/for_each_child/try_visit and
  schema-order observable field words. Both visitOrder branches are tested.
  `NodeView` is a borrowed preview dispatcher for data/flow. Flow fields on the five
  keyword-expression kinds are included; ordinary token kinds have none.
- Binder fields are recursively derived from snapshotted Go embeds, including
  local_symbol. `binder-fields.derived.json` records struct names, embedding paths
  and source locations. Inputs were found read-only in the adjacent main clone;
  `inputs/provenance.json` records original paths/hashes and `inputs/manifest.json`
  checks the bundled snapshots. Reproduction needs no external clone.
- `crates/records-preview/REPORT.md` gives all node layouts, AST arithmetic, family
  sizes, all 20 Type byte rows and the 18/41/42 Node/Symbol/Type cold fields.
  `tools/memory_report.py` reproduces it; `tools/README.md` lists commands.

## Verified

All Cargo commands used `nice -n 20`, `--offline`, `-j 2`, this preview package
only, and ran sequentially. The environment prints `nice: setpriority: Operation
not permitted`; the requested command is retained. No other package was selected
for a build/test (normal transitive dependencies compile).

```text
python3 scripts/generate_records.py
  generated 7 files; 195 node kinds, 20 Type kinds
python3 scripts/generate_records.py --check
  checked 7 files; 195 node kinds, 20 Type kinds
python3 crates/records-preview/tools/memory_report.py --check
  REPORT.md arithmetic reproduced
python3 crates/records-preview/tools/test_generator.py
  Ran 6 tests ... OK
nice -n 20 cargo test --offline -p tsc-rs-records-preview -j 2 -- --test-threads=1
  nodes_generated: 196 passed (195 kinds + every-token fixture)
  records_generated: 74 passed
  semantics: 14 passed
  compile-fail doctests: 3 passed
  total: 287 passed, 0 failed
nice -n 20 cargo clippy --offline -p tsc-rs-records-preview -j 2 --all-targets --no-deps -- -D warnings
  Finished dev profile ... (exit 0)
```

Logs: `target/lane-gen2/{test,generator-tests,clippy}.log`.
The Python test computes all generated output twice, compares bytes with disk,
runs --check, checks multiline source-field coverage, audits three Go kinds,
checks the per-kind ceiling, verifies rare occupancy and reproduces arithmetic.
Rust tests cover every field with distinct dummy identities, list expansion,
ordering, missing values, zero-text optionality, binder access, origin borrowing,
two inline candidates/overflow/deep copy, all mapper shapes/subkinds, slot states,
mutable shared records and rejection of shared cells / sending Type handles.

## Three-kind Go audit

Audited by reading the original struct bodies, independently asserted in the
Python test (not an expectation copied from the generated derived JSON):

- Identifier: ast_generated.go:621 -> FlowNodeBase:144 -> flow only.
- FunctionDeclaration: ast_generated.go:1989 -> StatementBase -> FlowNodeBase;
  DeclarationBase -> symbol; ExportableBase -> local_symbol;
  FunctionLikeWithBodyBase -> FunctionLikeBase -> LocalsContainerBase -> locals,
  next_container; BodyBase -> end_flow; own ReturnFlowNode -> return_flow.
- SourceFile: ast.go:2446 -> DeclarationBase -> symbol; LocalsContainerBase ->
  locals/next_container; own CommonJSModuleIndicator/ExternalModuleIndicator.

The pinned Go has no node possiblyExhaustive or flowWhenTrue/False fields.
SwitchStatementLinks.exhaustiveState is checker-local (checker/types.go:273);
Binder.currentTrueTarget/currentFalseTarget are traversal state, not per-file maps.
The derived JSON records these exclusions and SourceFile.jsdocCache as a side map.
Legacy schema kinds without an exact Go struct use explicit compatibility aliases;
these are visible in the generator and should be re-audited when B is integrated.

## Size and representation decisions

- Symbol: flags/name/parent/value_declaration/decl0 = 5 words, more = 2,
  tables = 1, exactly 32 B. No spare word fits.
- Signature has 44 named bytes plus 4 padding to reach 48 B. from_method is rare.
- StructuredMembers fields add to 36 B (40 allocated), not 32; IndexInfo adds
  to 24 B, not 20. Neither target can hold the stated 8 B span fields as written.
- InferenceInfo is 52 B (56 allocated): type_parameter/inferred_type 8,
  four inline candidates 16, two optional overflow spans 16, priority/flags/arity
  12. The 24 B target is impossible while preserving those requested fields.
  Priority presence is a separate flags bit so None, Some(0), Some(-1) remain
  distinct. Arity conversion is checked into the u32 arena-length domain.
  Context is 48 B, with its four late fields in a 16 B lazy rare record.
- Records use nullable raw `Id<T>` (4 B) and integer enum/boolean codecs, avoiding
  invalid Rust discriminants in stale records. `OptionalSpan<T>` uses two raw
  words and len+1 so absent and present-empty differ. A literal SPEC Span starting
  with NonZero Ref cannot itself accept every bit pattern. Conversely a zeroable
  two-word Span makes Rust Option<Span> 12 B. These are integration contract issues,
  not silently assumed 8-byte Options. Slot writers reject MAX/MAX-1 handle values
  reserved for resolving / resolved-none. TextRef rejects arena handles >= 2^31.
- Node preview reserves w5 and starts the typed tail at byte 24 to obey the stated
  `24 + 8*ceil(words/2)` formula. Actually overlapping the first tail word with w5
  changes that formula and needs an arena projection ABI decision.
- Full-range TextRef has no optional niche. Four template kinds add a presence
  word. JSDocComment (schema comment content, distinct from node jsDoc attachments)
  uses five words: tag plus four words of data. This preserves TextRef(0), full
  ranges and the Nodes/Text/None distinction. Both exceptions are in layout2.
- No rare extraction. CallExpression's pair would save 8 rounded bytes, but its
  joint occupancy is 16,951/962,167 = 1.761752%, above the <1% condition.

## Safe arena stub contract (155 lines)

No tsc-rs-arena dependency is declared: the lane was explicitly told not to depend
on the concurrently developed arena crate. The real tsc-types dependency supplies
flag reexports. SyntaxKind actually lives in tsc-syntax at this base, so its enum
is mirrored into generated preview syntax_kind.rs without adding a live syntax
crate dependency.

Mirrored names and signatures:

- Record trait; Ref<T>::raw/from_raw; ListHeader {len, extra}; List/Bytes markers;
  ArenaCell<T: Copy>::new/get/set; four-byte optional Ref and eight-byte Span.
- SharedArena::new(owner), alloc<T:Record+Sync>(&self,T),
  get_mut<T:Record>(&mut self,Ref<T>)->&mut T, freeze(self),
  alloc_bytes(&self,&[u8]), used_bytes/committed_bytes.
- Arena/LocalArena::new(owner), alloc<T:Record>(&self,T), alloc_slice,
  alloc_list, alloc_bytes, adopt_current_thread. Slice/list allocation adds Copy
  to the element bound to remain safe Rust. Handles advance in 8-byte units.
- Ref::get, Span::as_slice, Ref<List<T>>::items/header and Ref<Bytes>::as_bytes
  keep the names but **add an explicit &Arena argument and return RefCell borrow
  guards**, rather than unconstrained static references. Arena::get is an extra
  preview helper. Shared/Frozen wrappers add list_items/bytes helpers.

Differences: storage is a Vec of boxed Any values / padding slots, not adjacent
native bytes. Holding a read guard prevents allocation through the same arena;
overflow deep cloning releases its read before allocating a copied backing.
Frozen/Local/Shared thread mobility, native alignment, ownership checks, release/
reuse/zeroing, global reservation and physical commitment are not simulated.
committed_bytes reports logical used bytes only. freeze consumes the writable
wrapper; no native memory or performance claims follow from these stub tests.

## Open / integrator work

1. **Missing measurement**: the supplied B-3 stream has an aggregate Type count
   of 2,575,521 but no TypeData-kind populations or per-kind cold-cache occupancy.
   All 20 population cells are honestly unmeasured. Pass a complete kind-count
   JSON to memory_report.py --type-populations; it checks the aggregate. Cold-link
   occupancy is additionally needed to total proposed Type resident bytes. No
   compiler/census run was attempted under the no-benchmark/package restrictions.
2. **Permitted B preview boundary**: typed per-kind builders/records are implemented,
   but the exact FileArena facade (alloc_kind -> uniform NodeId, node/node_mut,
   alloc_list/list), Node::data()/tail_kind()/as_kind()/flow() on a bare 24 B header,
   and production typed observable_fields are not implemented. NodeView and typed
   KindRecord methods demonstrate decoding, zero-copy access and direct traversal.
   Header-to-adjacent-tail projection and variable-sized allocation require a
   reviewed native arena API; safe Vec Any storage cannot provide that projection.
   The stub is not a drop-in replacement with identical borrowing signatures.
3. Map the raw zeroable id/span codecs and reserved states to the final arena API;
   retain all-bit-pattern validity and !Sync propagation. Auxiliary literal/value
   identities are retained as payload words; semantic text/bigint tables are not
   migrated in this lane. The checker cold maps remain existing maps.
4. The exact node-list allocation total also needs the odd-length list count;
   REPORT gives a 4 .. 12,477,716 B padding range instead of an invented exact sum.
5. The integrator must price owned data() decoding with SPEC-0b ceiling (iv).
   No timing or instruction-count claim was made here. wasm32 was not built.

## Patches

Apply both numbered patches in order, with no commits made by this lane:

- `target/lane-gen2/patches/01-records-preview.patch`: generator, layouts, stub,
  generated records/nodes, bundled inputs, tests, workspace member and lock entry.
- `target/lane-gen2/patches/02-report-status.patch`: report, reproduction guide and
  this STATUS. User-owned README/SPEC edits present at entry are excluded.
