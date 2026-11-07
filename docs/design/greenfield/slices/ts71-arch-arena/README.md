# tsrs に追いつく構造変更：設計と工程（2026-11-07）

状態：**第 2.1 版（2026-11-07 夜）。所有者が方向を承認（「承認します。続けてください」）した第 1 版 `aa1992224` に、独立した敵対レビュー 2 本
（Opus 5.5 max、Codex gpt-6.1-sol max。両方とも「書かれたままでは NO-GO、契約と経済性を先に」）の所見を反映した版。** code は段 0 の
lane（arena crate、生成器 v2）まで。所見ごとの採否は [reviews/2026-11-07-dispositions.md](reviews/2026-11-07-dispositions.md)、
レビュー本文は [reviews/2026-11-07-opus.md](reviews/2026-11-07-opus.md) と [reviews/2026-11-07-codex.md](reviews/2026-11-07-codex.md)、
依頼文は [reviews/2026-11-07-brief.md](reviews/2026-11-07-brief.md)。第 1 版からの主な変更：(1) 着手順を checker の data 層 → front end に反転（§1）、
(2) 記憶モデルの健全性の節を追加（§0.5）、(3) 段 0b（B1 anchor・天井実験・PGO・oracle の trait）を freeze の前に置く（§2b）、
(4) gate を計測した B1 からの差分と wall で定義し直す、(5) 見込みをレビューの算術に差し替える（§8：**通常 build で tsrs の 1.2〜1.37 倍**が
現時点の正直な予測で、並ぶ予測ではない）、(6) emitter・API の型 handle・worker 数の決定性・`incremental.rs` の削除を計画に入れる。
所有者の指示（2026-11-07）：
「tsrs の性能に追いつくために scanner/parser/binder/checker の構造を大きく変更したい。まずは設計を」
「過去にとらわれずゼロベースで」「小手先のデータ構造変更は全て試して無意味だとわかったので、大きな変更が必要」
「各フェーズをより詳細に計画。変更量が大きいのでさらなる具体化が必要」。

根拠にした記録：[tsrs 比較 packet](../ts71-perf-tsrs/README.md)（M1 の段階別命令数台帳、終了節、構造試作の記録）、
[handover-2026-11-03](../ts71-perf-tsrs/handover-2026-11-03.md)（data model の現状と tsrs の形）、
`target/claude-tools/handover/perf-tsrs/consult-{codex,opus}-2026-11-04.md`（率直な相談の答え）。
数字はすべて VS Code `src` 1 thread、特記なければ #913 時点。

**状態（2026-11-08 01:30）**：段 0 と段 0b は完了。天井実験 4 本と B1 の測定（§0.9、§8）で、対象機構の上限は VS Code で合計 ≈ 5%
（≈ 6 G）と判明し、所有者は方針を転換した：「instruction の parity は構造書き換えの対象機構では届かない。memory（peak）の削減と、
tsrs との差が集中する phase の algorithm 差の調査に軸を移す」。pilot lane（SPEC-0b.6）は起動せず、この設計は測定済みの天井つきで保留。

## 0. 設計の要約

**診断。** 仕事の量は tsgo と同じ（型・symbol・instantiation の数、主要関数の入口回数が 1.00〜1.05 倍）なのに、同名関数が一様に
1.2〜2.2 倍遅い。原因は関数の中ではなく、関数が触る record の形：id から record までの routing、binder の答えが副表、checker の答えが
links 表（4 段の依存 load）、getter が `Vec`/`Arc` を返す（確保 1.55 億回対 0.22 億回）、parse 後の relocate と publish と adopt
（4.1 G、tsrs に対応物なし）、request の別走査（10 倍）。これらは 1 操作ごとに掛かる税で、record を 1 家族ずつ縮めても足し算にしか
ならない。構造を替えれば一斉に消える。tsrs の 85.5 G がその上限の実測。

**原則 4 つ。**

1. 参照はすべて `Ref<T>`（`NonZeroU32`、8 byte 単位 offset）。deref は定数 base + shift の 1 命令。プロセスに 1 つの仮想予約
   （native 32 GiB = u32 handle が名指せる全空間、wasm32 は linear memory）。id 空間・region・lease・relocation・publish・routing は
   存在しない。handle は確保した瞬間に最終値。**番号が観測される record はすべて密な序数を record 内に持つ**（node header の file 内 `index`、
   Symbol の `ordinal`、Type と Signature の `id`）。順序・API の handle・links の表の key はその序数で、address は決して使わない
   （レビュー R05/R13：address 単位で表を引くと密な id より 4 倍疎になり、node links だけで約 1 GiB になる）。
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

**記憶モデルの健全性（§0.5、レビュー B1 への答え）。**

- `Record` は「あらゆる bit 列が有効な値」の POD（zero だけでなく）。block は所有者の drop 後に zero 化して再利用するので（今の
  `crates/store` の lease と同じ契約）、古い `Ref` は zero か他の所有者の有効な record を読む。kind の decode は網羅的 `match` なので
  論理エラーであって未定義動作ではない。release 専用の `madvise` と再利用なしの「退役」は採らない（u32 handle の 32 GiB で LS の再 check
  が約 20 回で尽きるため）。
- `Ref<T>` は `PhantomData<&'static T>` を持ち、`T: Sync` のときだけ `Send + Sync`。file arena の record は cell を持たず（`SharedArena::alloc`
  は `Record + Sync` を要求、`ArenaCell` は `!Sync`）、binder は freeze 前に `&mut` で書く。checker arena の record は cell を持つので、その
  handle は thread を出られない。checker 全体を thread 間で動かす pool のためだけに `Owned<T>`（監査済みの `unsafe impl Send`、排他使用の
  契約）を arena crate に置く。`ArenaCell` の get/set は debug で所有 thread を検査する。
- 凍結後の file arena には書かない。`incremental.rs` の in-place 再利用は削除（本番の呼び出し元がなく、tsgo にも incremental parser は
  ない）。symbol の表の差し替えは checker 所有の clone にだけ行う（今の `symbol_mut` の `unreachable!` と同じ）。
- 型の順序は Type header の密な id（tsgo の `t.id`、API の `type_handle` もこれ）。`LocalArena::contiguous` は不要。
- parser は失敗した speculation の node を巻き戻さない（tsgo の `rewind` は `identifierCount`/`NodeCount` を戻さず、Nodes/Identifiers 行に
  出る）。`mark/reset` は scratch だけ。
- trybuild の compile-fail test（cell 付き record の `SharedArena` 投入、scratch の値の脱出、`Ref<cell record>` の他 thread への送出）。
- 所有の行列（レビュー R03、Codex「Missing」）：file arena の record は file arena だけを指す。checker arena は file と自分の arena を指す。
  emit arena は file と自分を指す。scratch は scope 内だけ。逆向きの参照（file → checker）は禁止。`owner_of` が返すのは **文書 id** で
  あって Program の file index ではない（同じ文書を複数の Program が別の index で共有する）。Program は `doc → file index` の密な
  配列を持つ（load が 1 つ増えるだけで、区間表の走査はない）。
- `'static` の view が、借用のための複製（Codex の 30 か所 sample で B 11 / S 12 / I 1 / O 6）を消す機構そのもの。`&self` の alloc は
  それを補うだけで、意味的な snapshot（S）は値のまま残す。

**却下した案。** lib の型を 1 回作って 4 checker に写す（型 id の順が tsgo と変わり union の並びが出力に出る）、lazy member 表
（depth 付近の診断が変わる）、checker の tsgo からの再移植（2 年分の適合を捨てる）。

## 0.9 進捗の記録（2026-11-07 夜）

- 所有者の指示（22:30）「段0と0bまで進めてください」。段 0 の成果：**#914 arena crate merge（`a07f200f1`）**、生成器 v2 は branch
  `arch/gen2-preview`（`e930f2955`、round 2 で密な序数と Rust 側 binder 書き込みの行列を追加）。段 0b：#915（oracle の trait、全 corpus
  143,190 行一致）、#916（phase ledger、feature `phase-ledger`）、#917（PGO の release workflow）を PR として提出。TypeData kind の census は
  観測 patch（lane `typecensus`、main には入れない）。天井実験の lane は links と dispatch を 23:05 に起動、bump と fe4 は順番待ち。
- **事故（22:3x）**：統合者の worktree 掃除 script が main repository を削除した（zsh の unquoted 変数が単語分割されず除外が効かず、
  `git worktree remove` 失敗時の `rm -rf` fallback が走った）。GitHub と 11-06 の backup から復旧。**失われた local 記録**：perf-tsrs packet が
  「保持した」と記す `target/codex-structural-20261107/`、`target/codex-line-profile-20261107/`（`report.html` 含む）、
  `target/codex-line-followup-20261107/`、`target/codex-perf-implementation-20261106/` の生 data と binary、`perf11/bin` の binary、
  9 月の worktree 21 個の未 commit 変更。hash と要約は packet と handover に残る。再現は記録された source と手順から。
  今後の規則は memory `disk-hygiene` と `incident-rm-main-repo`（破壊的 script は明示 list + deny-list + dry run、`|| rm -rf` 禁止）。
- 掃除：閉じた lane clone 23 個と旧 worktree 57 個、build cache、Go/npm/Codex log の cache を削除（空き 13 → 491 GiB）。退避は
  `~/dev/tsc-rs-archive/2026-11-07-{worktrees,codex-outputs}/`。
- 23:55：**#915 oracle の trait（`832b43134`）、#916 phase ledger（`7363b3dde`）、#917 PGO release workflow（`46a0e736e`）を merge**。
  release workflow の hosted smoke run を dispatch（結果は次の記録で）。天井 lane は links と dispatch が完了（feature-off byte 同一、
  focused test 通過、未計測）、bump と fe4 を起動。再発防止：Bash の PreToolUse guard hook と毎日の local backup script を導入
  （memory `incident-rm-main-repo`）。
- **2026-11-08 00:30〜01:30：段 0b の天井 4 本と B1 を測定**（integrator、1 thread、retired instructions、warm-up 1 + 3 round の中央値、
  base `b3e0c06c2`、binary と provenance は `perf11/bin/MANIFEST.tsv`、生 data は `handover/arch-arena/ceilings/results/`）。4 lane とも
  feature-off の dev CLI は base と byte 同一、focused test 通過、出力は 8 project で base と一致（`diffout.py`）。結果は §8 の表。
  fe4 は fallback 0（direct 10,321 / 2,876）で、見積もり lease の分だけ遅く・重い。tsrs との差は命令数そのもの（CPI は同じ）。
  TypeData kind の census（VS Code、1 checker）：2,567,002 型 = Object 1,125,919・Literal 458,153・Reference 400,496・TypeParameter 239,261・
  Union 232,499・TupleTarget 25,842・Intersection 20,236・GenericType 17,957・他。`memory_report.py --type-populations` は入力の
  memory 行が旧 build のため不一致で停止（入力の更新が要る）。
- **01:2x 所有者の判断**：「instruction の parity は構造書き換えの対象機構では届かない。memory（peak）の削減と、tsrs との差が集中する
  phase の algorithm 差の調査に軸を移す」。pilot lane は起動しない（価格 gate の「測った bound が有利」を満たさない）。具体案（関数単位の
  差の地図 → algorithm 差の修正、memory は並行）を提示し、承認待ち。承認後の記録先は perf-tsrs packet（この設計は保留）。
- 掃除：計測用 worktree 8 本を削除（binary は perf11/bin に保存）。CI：#918（release workflow の zod pin）、#919（arena test の直列化、
  hosted の flake 修正）が check 中。

## 1. 工程の全体像（第 2 版：checker を先に、front end を後に）

| 段 | 期間 | 成果物 | 出口の gate（go/no-go） |
| --- | --- | --- | --- |
| 0 基盤 | 1 週 | `crates/arena`（健全性の節どおり）、生成器 v2、shuffle 決定性 harness、byte 予測表 | 単体 test、生成器が全 kind を通る、予測表のレビュー |
| 0b 事前の de-risk | 1 週 | phase ledger を main に（feature）、B1 anchor（tsc-rs と tsrs）、天井実験 4 本（確保・links・front end・dispatch）、PGO の release pipeline、fingerprint oracle の trait 化 | B1 と天井の記録。**天井が閾値（front end ≤ B1 − 6 G、同名関数比 ≤ 1.10×）に届かなければ freeze せず所有者判断** |
| A checker の data 層 | 5〜6 週 | 型（密 id 付き header + tail）・transient symbol・links・signature・members・mapper・inference・scratch・整数 key を checker arena に。**NodeId / SymbolId（binder）は今のまま** | checker tests、4-checker conformance 15,339、8 案件、`.types`/`.symbols`、Types/Symbols/Instantiations 行不変、LS/API/jsdoc-corpus suite、semantic ≤ B1_semantic − 天井で決めた差分、1/4 thread の wall AB/BA |
| B front end | 4〜5 週 | scanner/AST/parser/binder/loader の arena 化、relocate・publish・adopt の削除、request の parse 内収集、resolution/loader の予算、emitter の `deep_copy_with_remap`、`incremental.rs` 削除、消費側の compile 修正 | fingerprint 一致、8 案件出力一致、emit 出力一致（bench-emit/decl）、workers 1/4/既定 × 3 回の決定性（Types/Symbols/Instantiations/Identifiers/Nodes 行込み）、shuffle 2 seed、front end ≤ B1_front − 天井で決めた差分、wall AB/BA |
| C 消費側と配布 | 2 週 | LS/API/project/lsp、`--build`・watch の smoke、thread 配置、README の性能表 | 全 suite hosted、`gates` |
| D 計測と判定 | 3 日 | 8 案件 × {1, 4, 既定} × {wall, peak, 命令数}、tsrs 通常と PGO の両方（normal-vs-normal と PGO-vs-PGO を別々に） | 達否の記録 |

合計 ≈ 110〜120 人日。**同時に build する lane は 2 本まで**（heavy job は 1 つの規則）、統合者 1 人。暦で **13〜15 週**。
main の freeze は段 0b の閾値を通ってから（段 A の開始時）。段 0 と 0b は freeze なしで進める。

順序を反転した理由（レビュー M10 / A1）：差の 58% は checker にあり、checker の data 層は `NodeId`/`SymbolId` を変えずに arena 化できる
（emitter 62 K 行・LS・API・fingerprint の消費側に触れない）。front end を先にすると、小さい方の bucket のために 8,400 か所の
`NodeData` match を持つ消費側すべてを最初の PR で動かすことになる。

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
今の memory report の行と並べる。段 A・B の出口 gate の予想値。

**段 0 の lane の結果（2026-11-07 夜、branch `arch/gen2-preview` `0610b17fe` の `crates/records-preview/REPORT.md`）。**
生成器 v2 は 195 node kind と 20 Type kind を生成し、preview crate の test 287 本が統合者の独立実行でも通った。REPORT の算術
（VS Code、16,640,179 node × 1.033）：node header 393 MiB + kind 別 tail（binder word 込み）205 MiB + list 68〜80 MiB = **667〜679 MiB**
（今の node record 525 + tail + array 81 + binder 副表 115 + locals 48 ≈ 770 MiB）。record の大きさは REPORT が正：Symbol 32 B
（`more` を 1 word の list 参照にして `ordinal` を入れる：round 2）、SymbolTables 32、TransientSymbol 32 / Full 56、Flow 16、Type header 24
+ kind 別 tail（Object 40、Literal 48、Union 64、Reference 48、TypeParameter 56、Mapped 72 か 64 + cold 16、Conditional 80 か 64 + cold 16、
GenericType 64）、Signature 48 + rare 40、**StructuredMembers 40**（36 を 8 B 単位に丸め）、**IndexInfo 24**、Mapper 16、
**InferenceInfo 56**（候補 inline 4 個）、InferenceContext 48 + rare 16。Type の kind 別個数は B-3 census にないので未計測（段 0b で
TypeData kind の census を取る：0b.7）。CallExpression の rare tail は共起率 1.76% で条件（< 1%）を満たさず不採用。
tsgo の `ast.go` には `possiblyExhaustive`/`flowWhenTrue/False` の node field がない（checker-local と走査状態）ので、この port の
BindData の map は port 固有として行列に記録する。

## 2b. 段 0b：de-risk の 1 週（freeze の前）

詳細は [specs/SPEC-0b.md](specs/SPEC-0b.md)。レビュー B2/B3/M13/M14 への答え。

- **0b.1 phase ledger を main に**（lane、2 人日）：M1 の `proc_pid_rusage` 計測を #913 に cargo feature `phase-ledger` で移植。境界は構造に
  依存しない定義（`load` = program 生成開始 → binder 開始、`semantic` = checker-init 終了 → finalization 開始）。計測器の PR は対象の
  PR と分ける。
- **0b.2 B1 anchor**（統合者、静音窓 1 つ）：tsc-rs #913 と tsrs を同じ窓で、8 案件 1 thread、warm-up 2、9 round の中央値。以後の gate は
  すべて B1 からの差分。
- **0b.3 天井実験 4 本**（lane、捨てる build）：(i) bump allocator で free なし、(ii) 最も熱い links 5 field を検査なしの密な配列に、
  (iii) parse を最終 id に直接・binder 出力を node に inline・publish なしで parse+bind+fingerprint だけを走らせる、(iv) checker の
  `NodeData` match 上位 50 か所を直接 field 読みに。判定規則は数字が出る前に固定：(iii) が front end ≤ B1_front − 6 G を示さなければ
  段 B を再設計、(i)+(ii) で同名関数比が ≈ 1.22× から ≤ 1.10× に下がらなければ段 A の gate をその値から決め、段 A の価値を所有者が判断。
- **0b.4 PGO の release pipeline**（lane、2 人日）：slice 8 は tsc-rs で −20.8%（VS Code 181.33 → 143.58 G）を実測済み。以後の tsrs 比較は
  normal-vs-normal と PGO-vs-PGO を別々に報告し、PGO を構造変更の効果に数えない。
- **0b.5 fingerprint oracle の trait 化**（lane、1 人日）：密 index の列挙（`SymbolId::new(base + i)`）を trait の背後に置き、今の code で
  baseline を byte 一致で再現してから、段 B で arena 版を実装する。旧 binary と行を保存してから生成器を変える（oracle と生成器が同じ
  field を黙って落とさないように）。
- **0b.6 薄い縦 slice**（lane、1〜2 週、捨てる build。レビュー R17 / Codex §4）：binder field 付きの凍結 node 1 種、本当の instantiated
  symbol と通常の可変 transient clone、再帰する可変 checker 呼び出しの中で借用する TypeList/Signature、可変な emit/API 消費者 1 つ、同じ
  文書を別 index で共有する 2 つの live Program と解放順、小容量での枯渇と再利用、選択的 speculation restore。固定 base と比較してから
  残りの kind に広げる。段 A の工数はこの slice の後に取り直す。

## 3. 段 B：front end（4〜5 週、lane 2 本。第 1 版の「段 1」。段 A の後に行う）

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
- speculation：失敗した `try_parse`/`look_ahead` の node は tsgo と同じく残す（`rewind` は `identifierCount`/`NodeCount` を戻さない。
  第 1 版の arena 巻き戻しは撤回：レビュー M8）。
- request 収集：parser は候補（`import`/`export … from`/`import()`/`require()`/`/// <reference>` の node）を parse 中に記録するだけ。
  正規化（合成 import、static、dynamic/type/JSDoc、augmentation の順序、mode と loadability、DET-1 が使う import 数）は
  `program/src/module_requests.rs` の契約のまま残し、別の木の走査だけをやめる（R07）。
- 入口：`parse_source_file(file_name, text, options, arena: FileArena) -> SourceFile`。削除：`parse_*_in_identity_domain`、
  `with_bases`、`SyntaxLeases`、`published_*`、`Published`、`relocate.rs`（4.6 K）、`remap_node_data_ids`。
- gate：fingerprint 全 corpus 一致、syntax の 279 test、conformance の parse 診断。

### 1.4 binder（`bind.rs` 4.0 K、`declare.rs` 2.0 K、`containers.rs`、`symbols.rs`、`symbols/table.rs`、`flow.rs`）— 5 人日、lane B

- Symbol 32 B を file arena に：`{flags, name, parent, value_declaration, decl0, more: Option<Ref<List<NodeId>>>, tables:
  Option<Ref<SymbolTables>>, ordinal}`（8 word ちょうど。第 1 版の `Span` 8 B + spare は 36 B で誤り：R14）。`ordinal` は file 内の
  生成番号で、API の symbol handle と tie-break（`type_order.rs:138-150`）はこれを使う。`SymbolTables` tail `{members, exports, global_exports, export_symbol,
  const_enum_only_module, is_replaceable_by_method, assignment_declaration_members: Option<Span<(NodeId, NodeId)>>}`。
  `SymbolExtras` の Box と `Arc<SymbolTable>` × 3 を削除。
- `SymbolTable`：`{len, entries: [(EscapedName, SymbolId)]}` の arena record。bind 中は thread scratch で育て、container の bind
  終了時に凍結（16 entry 超は hash index を後ろに付ける）。checker が後から足す経路（late-bound、merge）は checker arena に新しい
  table を作って symbol の `tables` cell を差し替える（tsgo の merge と同じ）。挿入順は保つ。
- BindData の per-node map を tail に。どの kind がどの field を持つかは **field × kind × 書き手/読み手の行列**（gen2 の成果物。tsgo の
  base 埋め込みと、Rust の binder の全書き込みの census の両方から導く：R08。flow は QualifiedName・BindingElement・statement にも
  書かれ、SourceFile は end flow、static block は return flow、ConditionalType と hosted JSDoc は locals を持ち、CallExpression に
  flow の書き手はない）。`node_flags_mut` は header `flags` に直接。BindData に残るのは `bind_diagnostics`、`classifiable_names`、
  `pattern_ambient_modules`、`common_js_module_indicator`、`emit_flags`。
- flow：`FlowId = Ref<Flow>`（global）。16 B record は維持、label の antecedent は `Span<FlowId>`。
- 削除：`published_node_symbol/flow/flags`、`DenseNodeMap`、`NodeFlowMap`、`BinderNodeFlags`、`TreeClaim`、`bind_reserved`、
  `attach_reserved_leases`、`relocate_with_leases`、`lease_identities`、`SymbolArena` の base と transient 分岐。
- gate：fingerprint の `bind_hash` 一致、binder の 86 test、conformance の bind 診断。

### 1.5 loader と Program（`loader.rs` 8.7 K、`prepared.rs`、`parse_cache.rs`、`workers.rs`、`program.rs`）— 5 人日、lane B

- `load_task` → `admit_source` で parse した直後に同じ worker で `bind_source_file`。relocation、publication、adopt を削除。
  `bind_existing_source` は後段の bind ではなく task の重複排除（表示 alias、library/root の class 判定）なので残す：同じ source が複数の
  task/class に付いても bind は 1 回。content mapper の補助出力も同じ経路。
- symbol と private-name serial の割り当て：今は Program 順に直列で予約してから並列 bind する（`checker/src/lib.rs:6214`）。融合後は
  file 内で割り当て、出力に出る serial は Program 順で fix-up するか tsgo と同じく checker で lazy に付ける。どちらにするかは段 B の
  SPEC で決め、gate に workers 1/4/既定 × 3 回の決定性を入れる（レビュー B6）。
- resolution（B0 で 4.08 G 対 tsrs 1.88 G）と fs/loader（4.18 対 2.99 G）も段 B の範囲に入れ、天井 (iii) から予算を切る（レビュー B3）。
- `FileDocument { arena: FileArena, source: SourceFile, bind: BindData }` を `Arc` で保持。`Program.files` の順が program 順。
- `try_file_index_of_node`/`source_of_node`（checker 264、ls 189 か所）の実装を `arena::owner_of(node)` の 1 load に。API は残す。
  `DenseOwners` 削除。
- impliedNodeFormat / module detection は parse 前に決まる既存経路を使い、bind が要る options を task に渡す。
- gate：program の 704 test、8 案件出力一致、`TSRS_PHASE_TRACE` に adopt 行がない。

### 1.6 消費側の compile 修正（checker、emitter、ls、api、project、lsp）— 5 人日、lane D

`NodeData::X(d)` の match（checker 2,810、emitter 3,443、ls 1,370、api 229、binder 524、program 29）は、生成器が今と同じ owned の
`XxxData` を tail から decode する形を保つのでそのまま compile する（`NodeData` の payload は今も Box なしの Copy 値型：レビュー R16。第 1 版の「Box variant 52 個」は古い記述で削除）。
`NodeData::X(XData {…})` で値を組み立てる箇所（emitter 94、checker の node builder 100、api 18、ls 12）も同じ struct で動く。手修正は
(a) `js_doc()`、(b) `published_*`、(c) `NodeArena` の変更 API（`node_mut`/`set_data`/`reshape`：emitter 14/4/0、api 7/1/2、syntax 18/2/0）、
(d) `&Node` → header + `data()`、(e) API decoder の kind 変更（`decoder/mod.rs:676-699`：別 size の record に作り直す）。
見積は段 0b の縦 slice で取り直す（R16/R17）。checker の意味は変えない（`kind_of` 1,758、`.node(` 454 は無変更）。
- **emitter（レビュー B4 / R04）**：emitter は 124 K 行（`src/*.rs` 62 K + `builtins/` + `declarations/`）。`TransformArena::add_source` は
  parse 済み tree を丸ごと複製して detach し、`parsed_node_base` の算術で元の node に戻す（`factory.rs:460-475`、`remap_node_data_ids`
  は `:7937`、`copy_subtree` は `synthetic_file.rs:148`）。global な `Ref` ではこの算術が成り立たないので、生成器が
  `deep_copy_with_remap(src, root, dst) -> (NodeId, OriginalMap)`（tsgo `DeepCloneNode`）を出し、emit arena に元 node の side table を
  持たせる。見積 8〜12 人日（第 1 版の 3 人日は過小）。

### 1.7 incremental（`incremental.rs` 1 K）— 削除

本番の呼び出し元がなく（syntax の test と example だけ）、tsgo にも incremental parser はない（parse cache が file 全体を parse して
bind する）。CLAUDE.md の「tsgo に合わせる」に従い削除する。第 1 版の「凍結 arena の in-place 更新」は撤回（レビュー M1 / B1）。

### 段 B の出口 gate（PR #2）

fingerprint 全 corpus `--compare` 一致（trait 化した oracle）、`cargo test -p tsc-syntax -p tsc-binder -p tsc-program`、8 案件
`diffout.py` 一致、emit 出力の全 file 一致（hono/zod/vue/TypeScript の bench-emit と bench-decl、段 B 前の binary と比較）、
conformance と LS/API/jsdoc-corpus suite（hosted）、workers 1/4/既定 × 3 回 × 8 案件の stdout/stderr/exit と
Types/Symbols/Instantiations/Identifiers/Nodes 行の一致、`arena-shuffle` 2 seed 一致、phase ledger で front end が
**B1_front − 天井 (iii) で決めた差分**以下、1/4 thread の wall AB/BA、hosted `gates`。届かなければ段 C に進まず所有者に判断を仰ぐ。

## 4. 段 A：checker の data 層（5〜6 週、lane 2 本。第 1 版の「段 2」。先に行う）

前提：checker arena は `&self` で alloc、可変 field は `ArenaCell`。`NodeId` と binder の `SymbolId` は今の store のまま（段 B で替える）。
speculation の巻き戻しは `SpeculationCheckpoint`（`speculate.rs:81`：19 本の stack 長 + clone する map 2 つ）と `SpeculativeLinksMarks`
（`resolved_signatures` だけ）で行われており、links の `set_*` の `_speculation_depth` 引数の大半は未使用（レビュー M2）。swap/revert/clear
の経路（`links.rs:1957-2071`）と `LinksTables` 外の signature 書き込み（`state.rs:2244`）もある。段 A の SPEC は **field ごとの
protocol 表**（fresh / 永続 cache / sentinel / scoped restore / accumulator / snapshot）を先に作り、選択的な journal を保つ（R10）。
stack を scratch に移しても truncate-on-rollback を保ち、test する。

### 2.1 型（`types/src/ty.rs`、`tables.rs` 3.8 K、`type_order.rs`）— 7 人日、lane E

- `Type` header 24 B `{flags, object_flags, id（checker ごとの密な生成番号。tsgo `t.id`、API の `type_handle` もこれ）, symbol,
  alias: Option<Ref<Alias>>, w5}` + kind 別 tail に `TypeData` の payload と `TypeLinks` の field を同居：Object `{members: Cell<Option<MembersId>>, deferred_node, instantiated_target, mapper, …}`、
  Reference `{target, type_arguments: Cell, …}`、Union/Intersection `{types: Span<TypeId>, regular_type, origin, key_property, …}`、
  TypeParameter `{constraint, default, base_constraint, target, mapper, …}`、Literal/Intrinsic `{value, fresh, regular}`、
  Mapped/Conditional/IndexedAccess/Substitution/ReverseMapped/GenericType。`TypeLinks` 表と `type_cold` の大半を削除。
- 順序は header の `id`（`compare_types` の tie-break、`type_order.rs`、union の並び、`engine.rs:4809` の再帰検出）。Signature も
  `id` を持つ（API の signature handle は `index + 1`：R05）。address 順は使わない。tail が links を含むと
  kind によっては今の 40 + 24 B を超えうるので、gen2 の REPORT の kind 別 byte 表（B-3 の occupancy 付き）で layout を決め、冷たい
  kind は links 表に残す（レビュー M5）。
- list：`TypeListId = Ref<List<TypeId>>`、`intern_type_list` は content hash の表で `Ref` を返す、`list(id) -> &[TypeId]` は arena
  借用。`get_type_list_id`/`get_alias_id`（String key）は `(TypeListId, AliasId)` の整数 key。
- API 名は保つ：`flags_of`（909 か所）、`object_flags_of`（182）、`type_of`（659）、`TypeData::` の match（465）は view enum。
- gate：types の test、Types 行不変。

### 2.2 symbol と links（`links.rs` 3.9 K、`state.rs`、`symbols.rs` の transient 部分）— 5 人日、lane F

- transient symbol を checker arena に：instantiated 形 32 B `{flags, origin: SymbolId, check_flags, target, mapper,
  type_of_symbol, declared_type, name_type}`（name/declarations/value_declaration/parent は origin 経由）、非 instantiated 形は
  Symbol 32 B + links tail 24 B。生成後に transient symbol を書き換える箇所（`symbol_mut` 57、`set_declarations` 15、`push_declaration` 5）
  を生成経路ごとに census し、形は確保時に決め、origin 由来 field への書き込みは debug で assert する（レビュー M4）。binder の symbol は
  今の `SymbolId` のまま読む。
- 永続 symbol の links：2 段 page 表。**key は密な序数**（段 A では今の `SymbolId`、段 B では file base + `ordinal`）で、handle の
  address ではない（R13）。record は 12 B `{type_of_symbol, declared_type, flags}`。`SymbolLinksCold` 37 map は
  transient 専用を tail に、残りは sparse のまま。
- node links：2 段 page 表、key は密な序数（段 A では今の `NodeId`、段 B では file base + header の `index`）、16 B record。
  `NodeLinksCold` と compound key の map（iteration、union property、properties-type、file 別 export cache）は sparse のまま。
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

- relation cache key：通常 key は既に整数 3 組（`relate.rs:59-77`）。残るのは generic key の正規化（型引数 index の正規化、制約 marker、
  深さ 4 の再帰）で、文字列構築の費用だけを価格付けして置き換える。Pair / Generic の 2 家族と、値ベースの `AliasKey`/`InstantiationKey`
  は意味を変えない（第 1 版の「`(TypeListId, AliasId)` の整数 key」は撤回：`AliasId` は可変 record で正規形ではない。R06）。
- `RelationErrorState` の clone を scratch の不変 chain `{parent, entry}` + depth に。capture は head の複製、reset は head の復元。
- `format!/String::new` 206 か所のうち cache key 用途を整数 key に、診断文は `#[cold]` 関数へ。
- gate：診断の順序と文面の byte 一致（8 案件、conformance）。

### 2.6 `to_vec`/`clone` の掃除 — 5 人日、lane F

`.to_vec()` 296、`.clone()` 531、`Vec<TypeId>` 236、`Vec<SymbolId>` 98、`Vec<SignatureId>` 46 を profile の self time 上位から、
`&[…]` 借用か `Span` に。「複製してから mutate」は残す。gate：serial.py の命令数（進捗確認）。

### 2.7 統合と oracle debugging — 6 人日、統合者

2.1〜2.6 を `arch/arena` に順に merge し、各段で checker 1,891 test と 8 案件を回す。差が出たら `.types`/`.symbols` の walk で最初の
差分 node を特定。

### 段 A の出口 gate（PR #1）

checker tests、`conformance_ts71.py --workers 1 --checkers 4` の 15,339 行が既知 4 差分のみ、8 案件 1 / 4 thread 出力一致、
`.types`/`.symbols` suite と LS/API/jsdoc-corpus suite（hosted）、Types/Symbols/Instantiations 行が 8 案件で不変、API の型 handle が
密 id で不変、phase ledger で semantic が **B1_semantic − 天井 (i)(ii) で決めた差分**以下、checker live が予測表の帯、1/4 thread の
wall AB/BA、hosted `gates`。届かなければ段 B の前に profile で原因を書き、所有者に判断を仰ぐ。

## 5. 段 C：消費側と配布（2 週、lane 2 本。第 1 版の「段 3」）

- **3.1 emitter** は段 B に移った（`deep_copy_with_remap` と元 node の side table、8〜12 人日）。段 C では emit arena の drop による
  解放と `transform_flags` の side table の整理だけ。
- **3.2 LS・API・project・lsp** — 5 人日。`source_of_node` 189 か所は段 B の実装で動く。API の node handle は表の index、symbol handle は
  登録制で wire 不変、型 handle は Type header の密 id（レビュー B5）。LS suite 4,583 と API suite は段 A・B の出口でも走らせる。
  `--build`（project reference）と watch の smoke、共有 lib binder の arena を LS で複数 Program が `Arc` で持つ寿命を確認。
- **3.3 診断 finalization** — 削除。comparator（file 名、start、長さ、code、category、source、message、related info）と重複 merge は
  複雑で簡略化できず（R11）、1.23 G のうち 0.92 G は #890 で既に除かれている（M11）。
- **3.4 PGO** は段 0b に移った。段 C では release tag での配布だけ。
- **3.5 thread 配置** — 1 人日。checker thread に macOS の QoS user-interactive、既定 checker 4、loader worker = P-core 数。

出口：全 suite hosted、LS / API suite、`gates`。PR #3。

## 6. 段 D：計測と判定（3 日）

8 案件 × {1 thread, 4 thread, 既定} × {wall, peak, 命令数}。tsrs 通常（`tsrs-sym-nopgo`）と npm PGO の両方で、normal-vs-normal と
PGO-vs-PGO を別々に。tsgo との出力一致は
DET-1 の窓と同じ手順。静音窓、warm-up 2、AB/BA 12 組、A/A 付き。packet と README の性能表に記録し、「tsrs より速い」の達否を
案件ごとに書く。

## 7. 体制・branch・gate の運用

- branch：`arch/arena` を main から切る。lane は `arch/arena` から sub-branch を isolated clone で作業し、SPEC・patch・自己 check
  を返す。統合者が毎日 `arch/arena` に merge し `cargo check --workspace` と focused test。main へは段 A・B・C の 3 本の PR
  （merge commit）。段 0/0b の成果物（arena crate、生成器、ledger feature、oracle の trait、PGO workflow）は通常の小さな PR で main へ。
- main の freeze：段 A〜B の間は修正だけ。統合者が週 1 回 `arch/arena` を main に rebase。段 0・0b は freeze なし。
- 役割：統合者（Claude）= SPEC、統合、gate、台帳、計測窓。Codex lane = 実装（xhigh）。Opus = 設計レビューと段 A の oracle debugging の
  併走。設計の敵対レビュー（Codex max + Opus max、独立）は 2026-11-07 に実施済み（所見は処理表に）。
- 負荷：build する lane は同時に 2 本まで、heavy job は 1 つ、lane の build は `-j 2`、計測は統合者の静音窓だけ。
- 契約の文書（段 0b の成果物、Codex「Missing」）：所有の行列、snapshot の契約（api/ls/lsp/project、cursor、文書 registry、source lease）、
  front end の全入口（TS/TSX、JS/JSX と hosted JSDoc、宣言/lib、JSON、parse cache、content mapper、project reference の redirect、
  package の重複排除、lib の class 判定）、request の観測契約、wire/serialization の消費者、`--incremental`/`--build`/watch、oracle の
  移行手順、変えた契約ごとの gate 行列、CI の生成器登録（`.github/ci/replay.py` が `generate_nodes.py --check` を走らせる）、byte の台帳。
- 撤退：段 A が gate に届かなければ main は #913 の構造のまま（段 0/0b の成果物だけが main にある）。段 B が届かなければ main は段 A の
  checker arena を保ったまま front end は今の store のまま。
- SPEC：`SPEC-arena`、`SPEC-gen2`、`SPEC-scanner`、`SPEC-parser`、`SPEC-binder`、`SPEC-loader`、`SPEC-consumers-fe`、`SPEC-types`、
  `SPEC-symbols-links`、`SPEC-sig-members`、`SPEC-scratch`、`SPEC-keys`、`SPEC-copies`、`SPEC-emit-arena`、`SPEC-ls-api`、
  `SPEC-pgo`。各 SPEC：範囲、API 契約、不変条件、削除する物、gate、却下条件。

## 8. 見込みと停止規則（第 2 版：レビューの算術に差し替え、B1 で確定）

第 1 版の「今 127.7 G / 2,886 MiB」は #909・#910 の card の和で、#911（peak −7.25%）と #913（命令数 ≈ −0.74%）を含まない。計測器
（phase ledger）も main になく、front end ≈ 29 G・semantic ≈ 95 G は推定だった（レビュー B2）。B1 を測るまでの暫定値はレビューの再構成
（±2 G）：

| 指標（VS Code 1 thread） | 今（#913、導出） | tsrs 通常 / PGO | 段 B 後（front end） | 段 A 後（semantic） | 合計（通常 / PGO） | peak |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 第 1 版の見込み | 127.7 G | 85.5 / 75 G | 15〜18 G | 68〜74 G | 85〜95 / 76〜85 G | 1,900〜2,200 MiB |
| レビューの算術 | ≈ 125〜126.5 G（front ≈ 32.7、semantic ≈ 90〜92、peak ≈ 2,674 MiB） | 同 | 21〜28 G（削れるのは adopt ≤ 4.1、request 0.4〜0.6、parse 0.8〜2.3、bind 0.5〜1.5 G。resolution/fs は別予算） | 78〜87 G（links/routing 2〜3、transient 1〜1.5、複製 1.5〜3、確保 2〜4、key/error state 1〜2 G、着地率 30〜60%。tsgo 忠実の床は ≈ 70〜72 G） | **101〜117 G / 85〜100 G** | 2,100〜2,400 MiB |

正直な見立て：レビューの算術では、この構造変更の到達点は **通常 build で tsrs の 1.2〜1.37 倍、PGO 同士で 1.13〜1.33 倍、peak は tsrs より
5〜20% 多い**。並ぶ予測ではない。第 1 版の数字は、個々に計測済みの機構（S3 の routing 除去 −0.91%、D1 の relocation −0.25 G、A-1a の
小さい node +3.40 G、node_symbol の圧縮 +855 M）を過大に見積もっていた。差の最大の塊（Codex 相談の「relation/member/inference の
制御経路と再問い合わせ」8〜16 G）は data 層では消えない。この見立てを覆せるかどうかを段 0b の天井実験で確かめ、§8 を B1 と天井の
数字で書き直す。

### B1 と天井の数字（2026-11-08、§8 の確定値）

B1（phase ledger、VS Code 1 thread、3 round 中央値、照合 −0.0045%）：config 1.40 G / 0.050 s、load 23.30 G / 1.449 s、bind 8.83 G / 0.436 s、
checker-init 0.06 G、semantic 87.81 G / 6.043 s、finalization 0.19 G、合計 121.63 G / 7.997 s、peak 2,673 MiB。tsrs（`--extendedDiagnostics`、
wall のみ）：parse 0.756、bind 0.285、check 4.561、合計 5.636 s。比は load 1.92×、bind 1.53×、check 1.32×、合計 1.42×；差 2.36 s の内訳は
check 63%、load 29%、bind 6%。8 project すべて同じ形（load 1.9〜2.5×、bind 1.4〜1.6×、check 1.2〜1.45×）。

tsrs 対 main（`serial.py`、1 thread、8 project）：VS Code は命令 85.44 G 対 121.61 G（tsc-rs 1.42×）、cpu 5.98 対 8.37 s（1.40×）、
peak 1,991 対 2,674 MiB（+683 MiB、1.34×）。**wall の比と命令の比が等しいので CPI は同じ：差は命令数であり、cache miss ではない。**
他の 7 project：命令 1.27〜1.58×、peak 1.25〜1.43×（ts +47、next +177、zod +137、effect +113、hono +18、vue +78、playwright +93 MiB）。

| 天井（SPEC-0b.3） | 測るもの | VS Code 命令（base 121.68 G） | 8 project | peak |
| --- | --- | ---: | ---: | ---: |
| (i) `ceiling-bump` | allocator + free の全作業（総確保 5.8 GiB） | **−2.55%** | −2.0〜−5.0% | +3,069 MiB（解放しない） |
| (ii) `ceiling-dense-links` | PagedTable の間接参照税（最熱の 2 表） | **−1.67%** | −0.8〜−2.5% | +91 MiB |
| (iv) `ceiling-direct-data` | 二重 dispatch（最熱 50 箇所） | **−0.81%** | −1.6〜+1.1% | ±0 |
| (iii) `ceiling-fe4` | relocate / publish / copy をなくす（fallback 0） | **+1.96%** | +0.5〜+1.2% | +1,052 MiB |

合計 ≈ −5%（≈ 6 G）→ 115.6 G ≈ tsrs の 1.35 倍。レビューの算術（1.2〜1.37 倍）の悲観側が確定。front end の publish/copy は今の表現では
除いても得がない。cpu 時間も命令と同じ向きで、局所性による追加の利得は観測されない。天井が測っていないもの：gen2 の record 表現そのもの、
型 list の複製（SPEC-0b.6 の対象）、mapper / inference context の生成。所有者の判断（§0.9）により、これらは追わない。

PGO 感度（[release 手順](pgo.md)、構造変更の gain には数えない）：

| 対象 | 通常 build | PGO build | 差分 | 証跡 |
| --- | ---: | ---: | ---: | --- |
| main `a75438da1`、hono 1 thread、hosted Linux x64（cachegrind Ir） | 2,358.1 M | 1,991.7 M | **−15.5%** | run [42395965544](https://github.com/kazhiramatsu/tsc-rs/actions/runs/42395965544) の `release-evidence-Linux-X64`（`results/hono/summary.json`、繰り返し 2 回の差 0.002%）。既定 wall 215 → 167 ms。zod と VS Code は未計測（同 run は繰り返しの完全一致規則で停止、修正 PR 参照） |
| #913+、VS Code 1 thread | 未計測 | 未計測 | 未計測 | integrator：hosted dispatch 成功と assets 確認後、native binary を取得し quiet window で warm-up 2 + 交互 9 round。commit/run URL・binary/corpus/profile hash・peak も記録 |

停止規則：段 0b の天井が閾値（front end ≤ B1_front − 6 G、同名関数比 ≤ 1.10×）に届かなければ freeze せず所有者判断。段 A・B の
出口 gate は「数値を超えたら不合格」で、猶予帯はない（R12）。不合格なら次の段に進まず原因を書いて判断を仰ぐ。

所有者に残る判断（両レビューの問い）：(1) 継続の条件は normal-vs-normal で並ぶ道筋か、memory の大きな改善 + 命令数の小さな改善でも
可か。(2) 32 GiB（u32 handle × 8 B）を live な arena の process 全体の上限として受け入れるか（drop 後の再利用で LS は回る。複数の大きな
Program を同時に持つ LS で超えれば明示的に失敗する。16 byte 単位にすれば 64 GiB）。(3) wasm32 をこの書き直しの間も対応 target と
するか（compile と 4 GiB の明示的な上限、実行時の検証は段 C）。(4) freeze の期間は暫定（縦 slice 後に再見積もり）でよいか。

## 9. リスク台帳

| リスク | 対策 |
| --- | --- |
| 段 A の出力同一性 debugging が工数の大半を占める | accessor と `set_*` の名前を保って変更を機械的に。段 A を 2 PR（2.1〜2.3 / 2.4〜2.7）に分ける退路。Opus を debugging に併走 |
| handle 順序への暗黙依存 | 順序は record の密 id だけ。`arena-shuffle` の CI job と workers 1/4/既定の決定性 gate |
| speculation の巻き戻し漏れ | 巻き戻しは `SpeculationCheckpoint` の stack 長 + map の clone（M2）。stack を scratch に移しても truncate を保ち test |
| 記憶モデルの健全性（stale `Ref`、cell の data race） | §0.5：全 bit 列が有効な `Record`、drop 後の zero 化と再利用、`Ref: Send+Sync iff T: Sync`、`Owned<T>` の監査、trybuild |
| 小 file の block 端数 | 16 KiB block（page と同じ）。memory report で committed を出す |
| emitter の detached copy | 段 B で `deep_copy_with_remap` と元 node の side table（B4）。emit 出力一致を gate に |
| API/LS の handle 公開 | node は表 index、symbol は登録制、型は密 id（B5）。suite を段 A・B の出口でも走らせる |
| 見込みの過大評価（第 1 版） | 段 0b の天井実験と B1 で §8 を書き直し、閾値未達なら freeze しない |
| wasm32 | handle は 32 bit で収まる。予約は `memory.grow`、単 thread。段 C で build を確認 |
| main の freeze と rebase 費用 | 週 1 回の rebase、修正以外は main に入れない。freeze は段 A 開始から |
| lane の並列 build と静音窓の競合 | build する lane は 2 本まで、計測は統合者の窓だけ（M9）。暦 13〜15 週 |

## 10. 捨てるもの・残すもの

捨てる：`crates/store` の region / identity space / lease / claim / relocation / publication、`relocate.rs`、`incremental.rs`、`PagedTable` と
`LinkSlot`/`IdSlot` と cold map の大半、binder の region 4〜6 と `BindData` の per-node map、`SymbolExtras` の Box、
`ResolvedMembers` の Box と Arc、`Signature` の Vec、`TypeMapper` の enum arena、`InferenceContext` の Vec、`RelationErrorState`
の clone、`get_type_list_id` の String、`ScannerState` の余分な word、`DenseOwners`、`module_requests.rs` の別走査。

残す：tsgo の論理と生成順、`NodeData`/`TypeData` の variant と field の名前、`kind_of`/`flags_of`/`type_of`/`node_array`/
`source_of_node` の API 名、名前の intern（`EscapedName`）、`bind_children`/`for_each_child` の生成、fingerprint と conformance の
oracle、perf11 の計測道具、DET-1 の checker 割り当て。
