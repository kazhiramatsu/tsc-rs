# tsrs に追いつく構造変更：設計と工程（2026-11-07）

状態：**計画**（所有者の方向承認待ち。code は未着手）。所有者の指示（2026-11-07）：
「tsrs の性能に追いつくために scanner/parser/binder/checker の構造を大きく変更したい。まずは設計を」
「過去にとらわれずゼロベースで」「小手先のデータ構造変更は全て試して無意味だとわかったので、大きな変更が必要」
「各フェーズをより詳細に計画。変更量が大きいのでさらなる具体化が必要」。

根拠にした記録：[tsrs 比較 packet](../ts71-perf-tsrs/README.md)（M1 の段階別命令数台帳、終了節、構造試作の記録）、
[handover-2026-11-03](../ts71-perf-tsrs/handover-2026-11-03.md)（data model の現状と tsrs の形）、
`target/claude-tools/handover/perf-tsrs/consult-{codex,opus}-2026-11-04.md`（率直な相談の答え）。
数字はすべて VS Code `src` 1 thread、特記なければ #913 時点。

## 0. 設計の要約

**診断。** 仕事の量は tsgo と同じ（型・symbol・instantiation の数、主要関数の入口回数が 1.00〜1.05 倍）なのに、同名関数が一様に
1.2〜2.2 倍遅い。原因は関数の中ではなく、関数が触る record の形：id から record までの routing、binder の答えが副表、checker の答えが
links 表（4 段の依存 load）、getter が `Vec`/`Arc` を返す（確保 1.55 億回対 0.22 億回）、parse 後の relocate と publish と adopt
（4.1 G、tsrs に対応物なし）、request の別走査（10 倍）。これらは 1 操作ごとに掛かる税で、record を 1 家族ずつ縮めても足し算にしか
ならない。構造を替えれば一斉に消える。tsrs の 85.5 G がその上限の実測。

**原則 4 つ。**

1. 参照はすべて `Ref<T>`（`NonZeroU32`、8 byte 単位 offset）。deref は定数 base + shift の 1 命令。プロセスに 1 つの仮想予約
   （native 32 GiB、wasm32 は linear memory）。id 空間・region・lease・relocation・publish・routing は存在しない。handle は確保した
   瞬間に最終値。
2. arena は所有者ごと：file arena（parse + bind の出力。凍結後は全 checker が読む）、checker arena（型・signature・transient
   symbol・member 表・list・links 表）、scratch arena（式検査の一時物、mark と reset）、emit arena（file ごと、print 後に解放）。
   個別 free はない。
3. record は header + kind 別 tail を同じ allocation に。binder の出力（symbol/locals/flow）は node の tail、型の links は型の tail。
   副表、Box、別 region の tail はない。
4. getter は arena 上の span を借用して返す。arena は thread 専有なので `&self` で alloc でき、可変 field は `ArenaCell`。借用の
   衝突が消えるので `to_vec`/`clone`/`Arc` がホットパスから消える。

**層ごとの形。**

| 層 | 今 | 新設計 |
| --- | --- | --- |
| 記憶 | 8 region の store、u32 id、lease と relocate | 1 予約、`Ref<T>`、所有者ごとの bump arena、64 KiB block と owner 表 |
| scanner | source 値は offset 済み。識別子の run は 1 byte ずつ、keyword は文字列 `match`、状態 11 word | ASCII run を SWAR、keyword は perfect hash、状態 6 word |
| AST | 32 B 固定 node + 3 word payload + 別 region の tail と list、親は後付け | header 24 B + kind 別 tail を parse 順に bump。list は `{pos, end, len+flags, items…}`。失敗した speculation は巻き戻す |
| binder | symbol/flow/flags が region 4〜6 の副表、locals は hash map、表は Box と Arc の 6 段 | symbol/locals/flow/end_flow/next_container は node tail の field。Symbol 32 B。表は 8 B entry の平らな slice。flow は global handle |
| loader | parse → relocate → publish → adopt → bind、request は別走査 | worker が parse 直後に bind。request は parse 中に記録。program 順は file 配列が持つ |
| checker record | Type 40 B + TypeLinks 表、Signature 136 B、ResolvedMembers 80 B + Box + Arc、transient は bit routing | Type 24 B header + tail に payload と links 同居。Signature 48 B + rare。StructuredMembers 32 B と span。transient は checker arena に 32 B（instantiated）か 56 B |
| checker links | PagedTable 4 段、cold map 37 | 永続 symbol/node は 2 段 page 表（12 B / 16 B）、transient は inline |
| checker 一時物 | mapper 24 B 解放なし、InferenceContext 152 B + Vec 5 本、error state を Vec で clone、String key | mapper 16 B、context 48 B + info 24 B、scratch arena、error state は不変 chain、整数 key |
| 並列と所有 | parse と bind の間に直列の publish | file arena は書き手 1 thread で凍結後 read-only。checker は自分の arena だけ書く。checker は P-core に 4 本 |

**Rust だからできること（tsrs を上回る余地）。** 名前は u32 で intern したまま（symbol 表と member 照合 5,400 万回が u32 比較）、
scanner の SWAR と parse+bind の融合（front end は tsrs の 17.6 G を下回れる唯一の段）、scratch の寿命を型で縛る（tsrs の実行時
escape bit なし）、生成した kind 別 visitor、PGO（tsrs は −13〜14%、こちらは未実施）。

**不変条件。** 出力は handle の値に依存しない（順序は file 順 index、pos、checker 内の生成順）。型の生成順 = checker arena の
address 順。`unsafe` は `crates/arena` 1 つ。tsgo の論理・生成順・診断順・cache の鍵の意味・名前の intern・checker 4 本と DET-1 の
file 割り当ては変えない。

**却下した案。** lib の型を 1 回作って 4 checker に写す（型 id の順が tsgo と変わり union の並びが出力に出る）、lazy member 表
（depth 付近の診断が変わる）、checker の tsgo からの再移植（2 年分の適合を捨てる）。

## 1. 工程の全体像

| 段 | 期間 | 成果物 | 出口の gate（go/no-go） |
| --- | --- | --- | --- |
| 0 基盤 | 1 週 | `crates/arena`、生成器 v2、shuffle 決定性 harness、byte 予測表 | 単体 test、生成器が全 kind を通る、予測表のレビュー |
| 1 front end | 2.5〜3 週 | scanner/AST/parser/binder/loader の arena 化、relocate 削除、消費側の compile 修正、incremental | fingerprint 全 corpus 一致、8 案件出力一致、conformance、shuffle 2 seed 一致、**front end ≤ 20 G**（今 ≈ 29 G） |
| 2 checker data 層 | 3.5〜4 週 | 型・symbol・links・signature・members・mapper・scratch・key の arena 化、`to_vec`/`clone` の掃除 | checker tests、4-checker conformance 15,339、8 案件、`.types`/`.symbols`、Types/Symbols/Instantiations 行不変、**semantic ≤ 75 G**（今 ≈ 95 G） |
| 3 消費側と配布 | 1.5 週 | emit arena、LS/API、診断 finalization、PGO pipeline、thread 配置 | 全 suite hosted、LS suite 4,583、API suite |
| 4 計測と判定 | 3 日 | 製品指標の表（8 案件 × 1/4/既定 × wall/peak/命令数、tsrs 通常と PGO） | 達否の記録 |

合計 ≈ 99 人日。Codex lane 3 本 + 統合者 1 人で **8〜10 週**。その間 main は修正だけ（feature freeze）。

## 2. 段 0：基盤（1 週）

### 0.1 `crates/arena`（`crates/store` を置き換える唯一の `unsafe` crate）— 3 人日、lane

- 予約：`Reservation` が native で 32 GiB の virtual 空間を 1 回 `mmap(PROT_NONE)`。使う block を commit。wasm32 は `memory.grow`。
- `Ref<T>`：`NonZeroU32`、8 byte 単位 offset。`Ref::get(self) -> &'static T`（record は動かず、解放は arena 単位。arena の生存は
  持ち主の `Arc` が保証）。`Option<Ref<T>>` は 4 B（niche）。
- block：64 KiB 単位の `BlockAllocator`（atomic bump）。`block_owner: [u32; 2^19]` で `owner_of(Ref) = block_owner[offset >> 13]` が
  1 load（file の特定、`source_of_node` の実装）。
- `Arena`：所有者 1 つの bump。`alloc<T: Record>(&self, T) -> Ref<T>`、`alloc_slice<T>(&self, &[T]) -> Span<T>`。`&self` で確保
  （thread 専有、`Cell<u32>` の bump）。`Arena::contiguous(bytes)` は checker 用に連続 virtual 範囲を予約し handle 順 = 生成順を
  保証する。`Arena::freeze()` 後の書き込みは debug で panic。
- `Span<T>` `{start: Ref<T>, len: u32}` 8 B、`List<T>` record `{len: u32, items: [T]}`。
- `ArenaCell<T: Copy>`：record 内の可変 field。`get/set` は `&self`。
- `ScratchArena::scope(&self) -> ScratchScope<'s>`、`ScratchVec<'s, T>`。`'s` が scope を出られないので永続 record に入らない
  （trybuild で compile error を test）。
- `Arena::release()`：block を `madvise(MADV_FREE)`（emit arena、LS の旧 tree）。
- memory report：arena ごとの committed / used を `TSRS_MEMORY_REPORT` に。

### 0.2 handle 型 — 1 人日

`tsc-types` の `NodeId`、`NodeArrayId`（`ListId` の別名）、`SymbolId`、`TypeId`、`SignatureId`、`FlowId` を `Ref<…>` の newtype に。
名前は残す（消費側 30 万行が id を受け渡す code をそのまま compile するため）。`IdentitySpace`/`IdentityLease`/`IdentityDomain`/
`TRANSIENT_SYMBOL_BIT` は `#[deprecated]` を付け、段 1 で消す。

### 0.3 生成器 v2（`scripts/generate_nodes.py` → `generate_records.py`）— 4 人日、lane

- 入力：`nodes.schema.json`（既存）、`nodes.layout.json` v2（kind ごとの tail layout：field 順、hot/cold、binder field）、新設
  `records.layout.json`（Symbol、TransientSymbol、Type の kind 別 tail、Signature、StructuredMembers、IndexInfo、Mapper、
  InferenceContext、Flow）。
- 出力：`crates/syntax/src/nodes.rs`（header、kind ごとの `#[repr(C)]` tail、`NodeData<'a>` view enum（variant は `&'a XxxTail`）、
  `node.data()` の jump table、builder `FileArena::alloc_xxx(fields…) -> NodeId`、`size_of` assertion）、`for_each_child.rs`、binder の
  `bind_children`、`observable_fields.rs`、`crates/checker/src/records.rs`、memory 行。`relocate.rs` は生成しない。
- `NodeData` の variant 名と field 名を保つ：`NodeData::CallExpression(d)` と `d.expression` がそのまま compile する。Box だった 52
  variant の deref 除去が消費側の主な compile 修正。

### 0.4 決定性 harness — 1 人日

cargo feature `arena-shuffle`（test build のみ）で block の割り当て順を seed で乱す。CI job：hono/zod/TypeScript compiler と
conformance 1,000 case を seed 2 つで走らせ、stdout/stderr/exit と `--extendedDiagnostics` 行を比較。relocate が担っていた
「handle 値に依存しない出力」の保証を置き換える。

### 0.5 byte 予測表 — 1 人日、統合者

`node-census`（kind 別個数）と B-3 census から、新 layout の AST / symbol / 型 / signature / members / links の live MiB を
今の memory report の行と並べる。段 1・2 の出口 gate の予想値。

## 3. 段 1：front end（2.5〜3 週、lane 3 本）

### 1.1 scanner（`scanner.rs` 3.7 K、`keywords.rs`、`chars.rs`）— 3 人日

source 値は既に offset（`TokenValue{start, end, cooked}`）。残り：(a) 識別子・空白・行コメントの ASCII run を `u64` の byte class
判定で 8 byte ずつ進める、(b) keyword は長さ + 先頭 2 byte の perfect hash 表（生成）、(c) `ScannerState` 11 word → 6 word、
(d) `scan_identifier_parts` の複製経路は escape のある識別子だけ。gate：scanner の test、fingerprint の `parse_hash` と
`diagnostics_hash` が全 corpus で一致。

### 1.2 AST record（生成物）— 2 人日

- header 24 B：`kind u16 | tflags u16`（literal flags、`HAS_JSDOC`）、`flags u32`（NodeFlags。binder が bind 中に書く）、`pos`、
  `end`、`parent: Option<NodeId>`、`w5`（kind layout が使う）。
- tail の例：Identifier `{name: EscapedName, flow: Option<FlowId>}`（計 32 B）、PropertyAccessExpression `{expression, name,
  question_dot, flow}`（40 B）、CallExpression `{expression, type_arguments: Option<ListId>, arguments: ListId, flow}` + rare
  （48 B）、StringLiteral `{text: TextRef}`（escape なしは source slice、32 B）、Block `{statements, locals: Option<TableId>, …}`、
  関数系 `{…, locals, end_flow, return_flow, next_container, symbol}`。
- list：`List<NodeId>` record `{pos, end, len | has_trailing_comma | modifier_flags, items…}`。`NodeArray<'a>` view は残す
  （`node_array(id)` の 862 か所がそのまま動く）。
- JSDoc：`HAS_JSDOC` + `FileArena.js_doc: FxHashMap<NodeId, ListId>`（JS / JSDoc mode の file のみ）。`js_doc()` は file 経由。
- text：`TextRef::Source{pos, end}` か arena の bytes。`NodeLookup::text()` の契約は保つ。

### 1.3 parser（`parser.rs` 11.4 K、`parser/jsdoc.rs` 2 K、`recovery*.rs`）— 6 人日、lane A

- `finish_node_*`（10082〜10412）と `alloc_node/alloc_token/alloc_missing` を builder に：tail の field を直接 arena に書く。
  `NodeData` 値を組み立てて `set_data` する経路は消える。
- list：既存の element stack から `alloc_slice` で 1 回写す。`alloc_array/empty_array/alloc_synthetic_array/missing_array` を `List`
  builder に。
- speculation：`try_parse`（1465）/`look_ahead`（1491）の失敗経路で `FileArena::mark()/reset()`。巻き戻す node を誰も参照しない
  ことを debug で確認（parent 未設定 = 未 finish）。
- request 収集：`import`/`export … from`/`import()`/`require()`/`/// <reference>` を parse 中に `SourceFile.module_requests` へ。
  `program/src/module_requests.rs` の別走査は削除。
- 入口：`parse_source_file(file_name, text, options, arena: FileArena) -> SourceFile`。削除：`parse_*_in_identity_domain`、
  `with_bases`、`SyntaxLeases`、`published_*`、`Published`、`relocate.rs`（4.6 K）、`remap_node_data_ids`。
- gate：fingerprint 全 corpus 一致、syntax の 279 test、conformance の parse 診断。

### 1.4 binder（`bind.rs` 4.0 K、`declare.rs` 2.0 K、`containers.rs`、`symbols.rs`、`symbols/table.rs`、`flow.rs`）— 5 人日、lane B

- Symbol 32 B を file arena に：`{flags, name, parent, value_declaration, decl0, more: Option<Span<NodeId>>, tables:
  Option<Ref<SymbolTables>>, spare}`。`SymbolTables` tail `{members, exports, global_exports, export_symbol,
  const_enum_only_module, is_replaceable_by_method, assignment_declaration_members: Option<Span<(NodeId, NodeId)>>}`。
  `SymbolExtras` の Box と `Arc<SymbolTable>` × 3 を削除。
- `SymbolTable`：`{len, entries: [(EscapedName, SymbolId)]}` の arena record。bind 中は thread scratch で育て、container の bind
  終了時に凍結（16 entry 超は hash index を後ろに付ける）。checker が後から足す経路（late-bound、merge）は checker arena に新しい
  table を作って symbol の `tables` cell を差し替える（tsgo の merge と同じ）。挿入順は保つ。
- BindData の per-node map を tail に：`node_symbol`→`symbol`、`locals`、`node_local_symbol`、`next_container`、`node_flow`→`flow`、
  `node_end_flow`/`node_return_flow`→関数 tail、`node_flow_when_true/false`/`possibly_exhaustive`/`node_fallthrough_flow`→該当
  kind、`node_flags_mut`→header `flags` に直接。BindData に残るのは `bind_diagnostics`、`classifiable_names`、
  `pattern_ambient_modules`、`common_js_module_indicator`、`emit_flags`。
- flow：`FlowId = Ref<Flow>`（global）。16 B record は維持、label の antecedent は `Span<FlowId>`。
- 削除：`published_node_symbol/flow/flags`、`DenseNodeMap`、`NodeFlowMap`、`BinderNodeFlags`、`TreeClaim`、`bind_reserved`、
  `attach_reserved_leases`、`relocate_with_leases`、`lease_identities`、`SymbolArena` の base と transient 分岐。
- gate：fingerprint の `bind_hash` 一致、binder の 86 test、conformance の bind 診断。

### 1.5 loader と Program（`loader.rs` 8.7 K、`prepared.rs`、`parse_cache.rs`、`workers.rs`、`program.rs`）— 5 人日、lane B

- `load_task` → `admit_source` で parse した直後に同じ worker で `bind_source_file`。`bind_existing_source` の後段 bind、relocation、
  publication、adopt を削除。
- `FileDocument { arena: FileArena, source: SourceFile, bind: BindData }` を `Arc` で保持。`Program.files` の順が program 順。
- `try_file_index_of_node`/`source_of_node`（checker 264、ls 189 か所）の実装を `arena::owner_of(node)` の 1 load に。API は残す。
  `DenseOwners` 削除。
- impliedNodeFormat / module detection は parse 前に決まる既存経路を使い、bind が要る options を task に渡す。
- gate：program の 704 test、8 案件出力一致、`TSRS_PHASE_TRACE` に adopt 行がない。

### 1.6 消費側の compile 修正（checker、emitter、ls、api、project、lsp）— 5 人日、lane D

`NodeData::X(d)` の match（checker 2,810、emitter 3,443、ls 1,370、api 229、binder 524、program 29）は view enum でそのまま
compile。手修正は (a) Box variant の deref、(b) `js_doc()`、(c) `published_*`、(d) `NodeArena` の変更 API（emitter 8、api 12、
ls 2 か所）、(e) `&Node` → header + `data()`。見積 500〜800 か所。checker の意味は変えない（`kind_of` 1,758、`.node(` 454 は
無変更）。

### 1.7 incremental（`incremental.rs` 1 K）— 2 人日

再利用 node は旧 file arena を `Arc` で保持したまま in-place で `pos/end/parent` を更新（旧 tree は LS が専有）。保持 byte が
新 source の 2 倍を超えたら全 parse。`collect_node_data_ids`/`node_data_structurally_equal` は `observable_fields` 版に。

### 段 1 の出口 gate（PR #1）

fingerprint 全 corpus `--compare` 一致、`cargo test -p tsc-syntax -p tsc-binder -p tsc-program`、8 案件 `diffout.py` 一致、
conformance（hosted）、`arena-shuffle` 2 seed 一致、M1 台帳で front end（load + bind、adopt なし）**≤ 20 G**、syntax + binder の
live が予測表の帯、hosted `gates`。**22 G を超えたら段 2 に進まず、profile で原因を書いて所有者に判断を仰ぐ。**

## 4. 段 2：checker の data 層（3.5〜4 週、lane 3 本）

前提：checker arena は `&self` で alloc、可変 field は `ArenaCell`。links の書き込みは今の `LinksTables` の `set_*`（speculation
journal の入口 117 か所）を経由したままにし、journal の論理は変えない。

### 2.1 型（`types/src/ty.rs`、`tables.rs` 3.8 K、`type_order.rs`）— 7 人日、lane E

- `Type` header 24 B `{flags, object_flags, symbol, alias: Option<Ref<Alias>>, …}` + kind 別 tail に `TypeData` の payload と
  `TypeLinks` の field を同居：Object `{members: Cell<Option<MembersId>>, deferred_node, instantiated_target, mapper, …}`、
  Reference `{target, type_arguments: Cell, …}`、Union/Intersection `{types: Span<TypeId>, regular_type, origin, key_property, …}`、
  TypeParameter `{constraint, default, base_constraint, target, mapper, …}`、Literal/Intrinsic `{value, fresh, regular}`、
  Mapped/Conditional/IndexedAccess/Substitution/ReverseMapped/GenericType。`TypeLinks` 表と `type_cold` の大半を削除。
- 生成順 = handle 順を checker arena の連続予約で保証。`compare_types` の tie-break と `type_order.rs` は handle 比較。Types 行は
  counter。
- list：`TypeListId = Ref<List<TypeId>>`、`intern_type_list` は content hash の表で `Ref` を返す、`list(id) -> &[TypeId]` は arena
  借用。`get_type_list_id`/`get_alias_id`（String key）は `(TypeListId, AliasId)` の整数 key。
- API 名は保つ：`flags_of`（909 か所）、`object_flags_of`（182）、`type_of`（659）、`TypeData::` の match（465）は view enum。
- gate：types の test、Types 行不変。

### 2.2 symbol と links（`links.rs` 3.9 K、`state.rs`、`symbols.rs` の transient 部分）— 5 人日、lane F

- transient symbol を checker arena に：instantiated 形 32 B `{flags, origin: SymbolId, check_flags, target, mapper,
  type_of_symbol, declared_type, name_type}`（name/declarations/value_declaration/parent は origin 経由）、非 instantiated 形は
  Symbol 32 B + links tail 24 B。`self.binder.symbol(id)`（489 か所）は `id.get()`。
- 永続 symbol の links：2 段 page 表（256 handle/page、12 B `{type_of_symbol, declared_type, flags}`）。`SymbolLinksCold` 37 map は
  transient 専用を tail に、残りは sparse のまま。
- node links：2 段 page 表、16 B record。`NodeLinksCold` は sparse のまま。
- symbol の順序 id（R29、`getSymbolId`）は links に残す。
- gate：Symbols 行不変、`.symbols` baseline。

### 2.3 signature・members・index info（`state.rs`、`structural.rs` 9.3 K、`functions.rs` 5.7 K）— 6 人日、lane E

- `Signature` 48 B `{flags, min_argument_count, declaration, parameters: Span<SymbolId>, type_parameters: Option<TypeListId>,
  this_parameter, resolved_return: Cell, target, mapper, rare: Option<Ref<SignatureRare>>}`。rare に composite / predicate /
  erased / canonical / base / optional-call の cache。`signature_of`（242 か所）は by-ref view。
- `StructuredMembers` 32 B `{members: TableId, properties: Span<SymbolId>, call: Span<SignatureId>, construct: Span<SignatureId>,
  index_infos: Span<IndexInfoId>}`、`IndexInfo` 20 B。`get_properties_of_type`（61）/`get_signatures_of_type`（90）は `&[…]` を
  返す。`Arc<…>` 99 か所の members 系を削除。
- Resolving の sentinel と「空だが存在」の区別は `LinkSlot` の意味を cell で再現。
- gate：`structural`/`functions` の test、Instantiations 行不変。

### 2.4 mapper・inference・scratch（`instantiate.rs` 3.7 K、`inference.rs` 4.0 K、`calls.rs` 7.2 K）— 5 人日、lane G

- `TypeMapper` 16 B record `{kind: u8, a, b}`、list は `Span`。`mapper_lists` の append-only Vec を arena に。
- `InferenceContext` 48 B + `InferenceInfo` 24 B × n を checker arena に（escape があるので永続。recycle は段 4 後の実験）。
- scratch：`choose_overload`/`resolve_call`/`infer_types`/relation walk の一時 `Vec`（候補、引数、visited、walk buffer）を
  `ScratchScope` に。永続 record に入れる値は `alloc` し直す（型で強制）。
- gate：`calls`/`inference` の test、Instantiations 行不変。

### 2.5 relation と key（`engine.rs` 5.1 K、`relate.rs`）— 4 人日、lane G

- relation cache key：`(source, target)` の u64 + `(relation, intersection_state)` で表を選ぶ。10 進文字列 key を削除。
- `RelationErrorState` の clone を scratch の不変 chain `{parent, entry}` + depth に。capture は head の複製、reset は head の復元。
- `format!/String::new` 206 か所のうち cache key 用途を整数 key に、診断文は `#[cold]` 関数へ。
- gate：診断の順序と文面の byte 一致（8 案件、conformance）。

### 2.6 `to_vec`/`clone` の掃除 — 5 人日、lane F

`.to_vec()` 296、`.clone()` 531、`Vec<TypeId>` 236、`Vec<SymbolId>` 98、`Vec<SignatureId>` 46 を profile の self time 上位から、
`&[…]` 借用か `Span` に。「複製してから mutate」は残す。gate：serial.py の命令数（進捗確認）。

### 2.7 統合と oracle debugging — 6 人日、統合者

2.1〜2.6 を `arch/arena` に順に merge し、各段で checker 1,891 test と 8 案件を回す。差が出たら `.types`/`.symbols` の walk で最初の
差分 node を特定。

### 段 2 の出口 gate（PR #2）

checker tests、`conformance_ts71.py --workers 1 --checkers 4` の 15,339 行が既知 4 差分のみ、8 案件 1 / 4 thread 出力一致、
`.types`/`.symbols` suite（hosted）、Types/Symbols/Instantiations 行が 8 案件で不変、M1 台帳で semantic check **≤ 75 G**、checker
live が予測表の帯、hosted `gates`。**80 G を超えたら段 3 の前に profile で原因を書き、所有者に判断を仰ぐ。**

## 5. 段 3：消費側と配布（1.5 週、lane 2 本）

- **3.1 emitter**（62 K 行：`factory.rs`、`transform.rs`、`printer.rs`、`declarations/`）— 3 人日。transform が作る node は file
  ごとの emit arena に `alloc` し、print 後に `release()`。`detached_clone`/`into_detached`/`copy_subtree`/`transform_flags` の
  detached 表を emit arena の side table に。
- **3.2 LS・API・project・lsp** — 3 人日。`source_of_node` 189 か所は 1.5 の実装で動く。API の node/symbol handle は登録制の opaque
  値なので wire 不変。LS suite 4,583 と API suite で確認。
- **3.3 診断 finalization** — 2 人日。M1 で 1.2 G 対 0.03 G。診断 record `{code, span, args: Span<Arg>}` の遅延描画と
  `(file index, pos)` の安定 sort。
- **3.4 PGO と配布** — 2 人日。`pgo.yml`（DRAFT）と `scripts/pgo_build.py` を release pipeline に。学習 corpus は hono +
  conformance batch + 2 案件。PGO 対 PGO で tsrs と比較できるようにする。
- **3.5 thread 配置** — 1 人日。checker thread に macOS の QoS user-interactive、既定 checker 4、loader worker = P-core 数。

出口：全 suite hosted、LS / API suite、`gates`。PR #3。

## 6. 段 4：計測と判定（3 日）

8 案件 × {1 thread, 4 thread, 既定} × {wall, peak, 命令数}。tsrs 通常（`tsrs-sym-nopgo`）と npm PGO の両方。tsgo との出力一致は
DET-1 の窓と同じ手順。静音窓、warm-up 2、AB/BA 12 組、A/A 付き。packet と README の性能表に記録し、「tsrs より速い」の達否を
案件ごとに書く。

## 7. 体制・branch・gate の運用

- branch：`arch/arena` を main から切る。lane は `arch/arena` から sub-branch を isolated clone で作業し、SPEC・patch・自己 check
  を返す。統合者が毎日 `arch/arena` に merge し `cargo check --workspace` と focused test。main へは段 1・2・3 の 3 本の PR
  （merge commit）。
- main の freeze：段 1〜2 の間は修正だけ。統合者が週 1 回 `arch/arena` を main に rebase。
- 役割：統合者（Claude）= SPEC、統合、gate、台帳。Codex lane = 実装（xhigh）。Opus = 設計レビューと段 2 の oracle debugging の
  併走。着手前に設計の敵対レビュー（Codex max + Opus max、独立）を 1 回。
- 負荷：heavy job は同時に 1 つ、lane の build は `-j 1`、計測は静音窓。
- SPEC：`SPEC-arena`、`SPEC-gen2`、`SPEC-scanner`、`SPEC-parser`、`SPEC-binder`、`SPEC-loader`、`SPEC-consumers-fe`、`SPEC-types`、
  `SPEC-symbols-links`、`SPEC-sig-members`、`SPEC-scratch`、`SPEC-keys`、`SPEC-copies`、`SPEC-emit-arena`、`SPEC-ls-api`、
  `SPEC-pgo`。各 SPEC：範囲、API 契約、不変条件、削除する物、gate、却下条件。

## 8. 見込みと停止規則

| 指標（VS Code 1 thread） | 今（#913） | tsrs 通常 / PGO | 段 1 後 | 段 2 後 | 段 3 後（PGO 込み） |
| --- | ---: | ---: | ---: | ---: | ---: |
| 命令数 | 127.7 G | 85.5 / 75 G | ≈ 115 G | 85〜95 G | 76〜85 G |
| 内訳 front end | ≈ 29 G | 17.6 G | 15〜18 G | 同 | 同 |
| 内訳 semantic | ≈ 95 G | 66.8 G | ≈ 95 G | 68〜74 G | 61〜67 G |
| peak footprint | 2,886 MiB | 2,008 MiB | ≈ 2,650 MiB | 1,900〜2,200 MiB | 同 |
| 4 thread wall | 3.13 s | 1.97 s | — | ≈ 2.1 s | ≈ 1.9〜2.0 s |

正直な見立て：この構造は「追いつく」ための必要条件で、1 thread の命令数は tsgo 忠実のまま並ぶところまで。tsrs は tsgo より型を
8% 少なく作る近道を持ち、その分は per-operation 効率で埋める。明確に上回る材料は front end の融合と SWAR、名前の u32、PGO、
thread 配置の 4 つで合計 5〜10%。tsrs の近道を結果同一の証明付きで採るかは別の所有者判断。

停止規則：段 1 の front end が 22 G 超、段 2 の semantic が 80 G 超なら、次の段に進まず原因を書いて判断を仰ぐ。

## 9. リスク台帳

| リスク | 対策 |
| --- | --- |
| 段 2 の出力同一性 debugging が工数の大半を占める | accessor と `set_*` の名前を保って変更を機械的に。段 2 を 2 PR（2.1〜2.3 / 2.4〜2.7）に分ける退路。Opus を debugging に併走 |
| handle 順序への暗黙依存 | `arena-shuffle` の CI job。handle 比較は型の生成順だけに限定し、symbol は順序 id |
| cell 化で speculation journal が漏れる | 書き込みは `LinksTables` の `set_*` 経由のみ（lint で直接 `set` を禁止） |
| 小 file の block 端数と untouched page | 64 KiB block は virtual、resident は触った page だけ。memory report で committed を出す |
| LS の incremental で旧 arena が溜まる | 保持 2 倍で全 parse。LS suite で確認 |
| API/LS の handle 公開 | 登録制の opaque 値で wire 不変。suite で確認 |
| wasm32 | handle は 32 bit で収まる。予約は `memory.grow`、単 thread。段 3 で build を確認 |
| main の freeze と rebase 費用 | 週 1 回の rebase、修正以外は main に入れない |

## 10. 捨てるもの・残すもの

捨てる：`crates/store` の region / identity space / lease / claim / relocation / publication、`relocate.rs`、`PagedTable` と
`LinkSlot`/`IdSlot` と cold map の大半、binder の region 4〜6 と `BindData` の per-node map、`SymbolExtras` の Box、
`ResolvedMembers` の Box と Arc、`Signature` の Vec、`TypeMapper` の enum arena、`InferenceContext` の Vec、`RelationErrorState`
の clone、`get_type_list_id` の String、`ScannerState` の余分な word、`DenseOwners`、`module_requests.rs` の別走査。

残す：tsgo の論理と生成順、`NodeData`/`TypeData` の variant と field の名前、`kind_of`/`flags_of`/`type_of`/`node_array`/
`source_of_node` の API 名、名前の intern（`EscapedName`）、`bind_children`/`for_each_child` の生成、fingerprint と conformance の
oracle、perf11 の計測道具、DET-1 の checker 割り当て。
