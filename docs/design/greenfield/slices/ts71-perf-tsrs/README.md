# tsrs より速く（2026-11-01〜）

引き継ぎ（2026-11-03、Fable 5.1 → Opus）：[handover-2026-11-03.md](handover-2026-11-03.md)（slice 11 の残作業、計測の作法、
data model の現状と費用の証拠、slice 12〜17 の設計、tsrs との対応、利用者判断が要る項目）。

利用者の指示（2026-11-01）：「パフォーマンスを改善してください。ピークメモリも削減してください。CIは1プロセスCPU100%まで許可します」
「目標はtsrsより高速になることです」「tsrsは我々のプロジェクトと似ていますが完全に関数レベルで移植しています。われわれは完全には
移植していない分、rustならではの最適化ができると思っています。もし速度差があるなら同じ関数を調べてパフォーマンスを比較して
ください」。試作の数字を見た後の判断：「いちいち聞かなくていいので進めてください」。`unsafe` は新しい小さな crate 1 つに閉じ、
他の crate は `#![forbid(unsafe_code)]` のまま。

**tsrs** = [oxc-project/tsrs](https://github.com/oxc-project/tsrs)（tsgo の Rust 移植。npm `tsrs` 0.20.1、TypeScript 7.1.0-beta
`3e9236361` を移植。tsc-rs の参照 `aa8149273` の 53 commit 後）。この repo の環境変数の接頭辞 `TSRS_` とは無関係。既定の mode が
tsgo と同じ答え（checker 4 つ、eager な member）で、`--fast` は答えが変わりうる。

## 比べ方

- **負荷**：local は 1 process が CPU 100% まで（`cargo -j 1`、conformance `--workers 1`、test は `--test-threads=1`、重い job は
  一度に一つ、`nice -n 20`）。
- **数字**：1 thread の命令数（`/usr/bin/time -l` の instructions retired）と peak footprint で判断する。別の job が同じ machine を
  使うので wall time は参考にしない。tsc-rs は `TSRS_WORKERS=1 TSRS_CHECKERS=1`、tsrs・tsgo は `--singleThreaded`。
- **道具**（`target/claude-tools/perf11/`、git 管理外）：`serial.py`（binary の A/B を 1 case ずつ）、`run-samply.sh`（samply の
  記録。install は利用者が実行）、`sym.py`・`analyze.py`・`cmp2.py`・`drill.py`（`atos -i` で inline を展開した関数別の比較）、
  `fcmp.py`・`callers.py`（symbol 単位の比較と呼び出し元）、`disasm.py`（同じ関数の機械語）、`tsrs-notes-digest.md`（tsrs の
  `notes/` の要約：効果の測られた技法 R1〜R33、却下された案、hot な関数）。
- **tsrs の binary**：npm の binary は PGO 込みで symbol なし。関数単位の比較は同じ tag を `cargo build --release`（PGO なし、
  symbol あり、toolchain 1.99.1）で作って使う（VS Code 1 thread で 86 G。PGO 込みは 75 G）。

## 差（main `7d517bbd7` と tsrs、1 thread）

| project | 命令数 tsc-rs ÷ tsrs | peak footprint tsc-rs ÷ tsrs |
|---|---:|---:|
| hono | 2.2〜2.6 | 1.8 |
| zod | 2.4〜2.6 | 2.0 |
| TypeScript compiler（`src/compiler`） | 2.5〜3.8 | 2.0〜2.2 |
| Next.js / Effect | 2.6〜2.9 | 2.0〜2.1 |
| VS Code（`src`、10,196 file） | 3.1（229 G 対 75 G） | 2.2（4.4 GB 対 2.0 GB） |

出力（診断の byte 列）は全 project で一致。関数単位で見て分かったこと：

- **差は一様**。VS Code の段階別は parse 2.18 s 対 0.81 s、bind 0.56 対 0.29、check 10.5 対 4.8 で、どの関数も約 2.2 倍。
  1 か所が遅いのではない。
- **主因は memory model**。tsrs は固定 address の arena と 32-bit handle で、`node.parent`・`kind` を 1 word で読む。tsc-rs は
  `NodeId` から record に届くまでに file の区間表を引いていた（`node_record` の routing が sample の 13%、symbol の routing 3%、
  親を 1 段たどるのに 23 命令超 対 8 命令）。allocator は 7%。
- `Result` の伝播は主因ではない（checker の `Err` を消す実験で −1.5〜−2.2%）。
- `node.symbol`・`locals` が hash lookup、`get_signatures_of_type` などが `Vec` を複製して返す（tsrs の R18 と同じ指摘）。

## slice 1：固定 address の store（branch `perf/ts71-fixed-stores`）

**設計。** 新 crate `tsc-store`（`crates/store`、compiler で唯一 `unsafe` を持つ crate）が、process に一つの address 範囲を
予約する（`0x4000_0000_0000` から 8 region × 2^38 byte。`mmap(MAP_NORESERVE)`／Windows は `VirtualAlloc(MEM_RESERVE)`。触らない
page は無料）。record の種類ごとに region が一つあり、identity（`NodeId`・`NodeArrayId`・`SymbolId`）の値がそのまま record の
位置：`base + id * size` の 1 回の積和と 1 回の load で届き、base も表も境界も読まない。

- `StoreVec<T>`：heap の `Vec` と同じ 3 word（ptr・len・capacity）に置き場所の印を添えた list。parse・bind は heap（64 KiB 以上の
  buffer は専用の mmap：解放で即 OS に返る。allocator の page 共有で残らない）に作り、`publish(position)` で lease した位置へ
  移す（以後 final）。bind の symbol は予約済みの範囲の slot に直接作る（`claim_room`。予約を超えたら heap に移って relocate）。
  per-node 表（`node_symbol`・`node_flow`・binder の flag）は木の位置を `claim` する。drop は record を drop して slot を 0 に
  戻し、1 MiB 以上は page ごと OS に返す（全 0 はどの record 型でも有効な値なので、古い identity は空の record を読む）。
- `SmallList<T>`：4 byte record を 2 つまで inline に持つ 16 byte の list（`Symbol::declarations`。`Symbol` 56 → 48 byte）。
- **identity は process で一意**：`IdentityDomain::new()` は process に一つの状態を共有する（domain は document の所有者の印で
  あり、番号の空間ではない）。`NodeArrayItem` の空間を足し、array の要素も store に置く。provisional 予約（EphemeralBump）と
  policy の分岐は削除した。
- **読み口**：`tsc_syntax::published_node`／`published_node_array`、`tsc_binder::published_symbol`／`published_node_symbol`／
  `published_node_flow`／`published_node_flags`。`ProgramBinder` の `node_record`・`node_array`・`symbol`・`node_symbol`・
  `flags_of`・`flow_node_of` はこれらを呼ぶだけになり、区間の route 表（`node_routes`・`symbol_routes`・`array_owners`）と
  hint は消えた。file を知る必要のある `source_of_node` だけ page 表（`DenseOwners`）を引く。
- `CheckAbort` を uninhabited にした（`Err` の唯一の生成元は tsc 6.0.3 の crash の再現で、tsgo にはない）。

**計測**（1 thread、`serial.py`。a1 = main + `CheckAbort`、a8 = この branch）：

| case | 命令数 main → a8 | peak footprint main → a8 |
|---|---:|---:|
| TypeScript compiler noEmit | 15.72 → 13.40 G（−15%） | 236 → 217 MB |
| Next.js noEmit | 46.05 → 40.54 G（−12%） | 977 → 916 MB |
| VS Code | 228.3 → 195.9 G（−14%） | 4,657 → 4,346 MB |

残りの corpus（noEmit、main → a9 = merge 直前の binary）：hono 4.29 → 3.89 G（−9%、135 → 133 MB）、zod 30.8 → 27.8 G
（−10%、868 → 858 MB）、effect 27.4 → 23.9 G（−13%、679 → 662 MB）、playwright 21.8 → 19.0 G（−13%、511 → 502 MB）、
vue 23.9 → 21.4 G（−11%、387 → 385 MB）。

途中の段階：route 表を store の読みに替えただけ（a3）は −5%、per-node 表を store へ（a4）−6%、`StoreVec` を `Vec` と同じ
3 word に（a5）−15%（`Deref` の分岐が消えた）。a5 は peak が +380 MB 増えた（解放した parse 用 buffer を mimalloc が page 共有で
保持する）ので、64 KiB 以上の buffer を mmap に、symbol を予約 slot に直接作るようにして（a7）main より下に戻した。
`mi_collect` を adoption の後に呼ぶ案（a6）は効かなかった。owner の hint の除去（a8）は ±1% で差なし。

**記録。** PR [#842](https://github.com/kazhiramatsu/tsc-rs/pull/842) → merge `bda16f32a`（2026-11-02。commit：store crate `b0cfed5ca`、
本体 `26bb13732`、文書 `56d062bc7`、test の修正 `c3a8e2ded`）。local の `scripts/conformance_ts71.py --workers 1 --check`
（`56d062bc7`）：12,829 case、lane A 13,577 full、mismatch 0（診断・emit・types・symbols・sourcemap・trace・content mapper）、
harness error 0、ratchet の退行 0。`scripts/suites_ts71.py --check`：退行 0（既知の 3 件のみ）。hosted run
[41323763901](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41323763901)（`c3a8e2ded`）：rust・conformance・plan・gates 全て
pass。最初の hosted run 41320897830 は syntax の unit test 1 件（process 共有の identity の active range 数を絶対値で assert して
いて、並列 test で他の test の lease を数えた）で失敗し、`c3a8e2ded` で `IdentityLease::watch`（lease の生死を数えずに見る）に
替えた。main との出力（stdout・終了 code）は 7 case で byte 一致（`target/claude-tools/perf11/diffout.py`）。

**不変条件。** 公開済みの木は同時に一度しか bind できない（per-node 表が木の node 位置を占める）：binder は木の node lease に
`IdentityClaim` を取り、二度目は panic する（debug build では store の範囲の二重 claim も検査する）。production では document は
一度だけ bind され（`BoundDocument`）、API の作成 file も parse 直後に一度 bind する。

## slice 2：routing なしの node 読み（branch `perf/ts71-published-lookup`）

store の後も checker は node を読む前に file を探していた（`node_util` の関数が `&SourceFile` を取り `source.arena` を引くため、
812 か所が `source_of_node` の page 表を通る。a9 の profile で self 3.3%）。

- `node_util` の木だけを読む関数（141 中 104、さらに `crate::assignment` の 23 と、それに阻まれていた関数）は
  `tree: &impl NodeLookup` を取る。残りは file の text・root・JSDoc の hosted 表（`crate::hosted::jsdoc_hosted`、
  `reparsed_overload_host`）が要る：`get_name_of_declaration`・`has_dynamic_name`・`type_node_of`・
  `get_combined_modifier_flags`・`get_effective_modifier_flags`・`declaration_name_to_string` など。
- `tsc_syntax::Published` は store を読む lookup（file を探さない）。`SourceFile` も lookup なので binder・emitter・
  言語 service の呼び出し元はそのまま。
- `assignment` が file の root で「JS file か」を見ていた 2 か所は、tsc の `isInJSFile` と同じく手元の node の flag を読む
  （JS file の全 node が `JAVA_SCRIPT_FILE` を持つ）。
- checker：`node_util::f(self.binder.source_of_node(x), ..)` → `node_util::f(&Published, ..)`、`source.arena.node(..)` →
  `Published.node(..)`、`for_each_child(&source.arena, ..)` → `for_each_child(&Published, ..)`（script で 574 か所）。
  それだけのために `source` を束縛していた 193 の `let` と、引数だけになった helper の仮引数 6 つを削除。

**計測**（1 thread、a9 = merge 後の main の binary → b2）：

| case | 命令数 | peak footprint |
|---|---:|---:|
| TypeScript compiler noEmit | 13.45 → 12.93 G（−3.9%） | 217 → 217 MB |
| Next.js noEmit | 41.13 → 39.44 G（−4.1%） | 915 → 923 MB |
| VS Code | 197.4 → 188.4 G（−4.6%） | 4,347 → 4,347 MB |

第一段（node_util のみ、b1）で −4.2〜−4.6%、第二段（assignment、b2）で −0.3〜−1.0%。出力は 7 case で a9 と byte 一致。

**記録。** PR [#843](https://github.com/kazhiramatsu/tsc-rs/pull/843) → merge `2c00eaf87`（2026-11-02。commit：checker `e1f637219`、
assignment `c4b95ac95`、文書 `c8da31120`、main の取り込み `a110d31e9`）。local の conformance（`c8da31120`）：12,829 case、
mismatch 0、退行 0。suites：退行 0。hosted run [41331926493](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41331926493)
（`a110d31e9`）：全 job pass。
残る `source_of_node`（350 か所）は `file_name`・`root`・`text`・`is_declaration_file` と、hosted 表の要る関数、
`locals_of`（`resolve_name` の scope walk：file ごとの `FxHashMap<NodeId, SymbolTable>`）。

## slice 3：cache key を値に（branch `perf/ts71-cache-keys`）

b1 の profile では hashbrown の get/insert/rehash が 3.3%、`format!` が 0.8%、malloc/free が 2.4%。その一部は tsc が文字列で
作る intern 用の key（`getTypeListId`・`getAliasId`：union・intersection・indexed access の map）で、lookup ごとに
String を組み立て、hash していた。

- `UnionKey { list: TypeListId, origin, alias }`・`UnionOfUnionKey`・`IntersectionKey { list, alias | NoConstraintReduction }`・
  `IndexedAccessKey`、`AliasKey`（tsc の getAliasId：引数は symbol があるときだけ数える）。list は既存の
  `intern_type_list`（instantiation の key と同じ）。subtype reduction の cache も `TypeListId` で引く。
- flow cache の key は tsc の文字列のまま `push_decimal` で組む（`format!` を消す）。
- inference の walk の visited map は checker に持ち回り、walk の後に clear する（tsgo は inferTypes ごとに確保。
  rehash で育てる分が消える）。

**計測**（1 thread、b2 = #843 の merge → c1）：

| case | 命令数 | peak footprint |
|---|---:|---:|
| TypeScript compiler noEmit | 12.93 → 12.54 G（−3.0%） | 217 → 218 MB |
| Next.js noEmit | 39.94 → 38.15 G（−4.5%） | 916 → 921 MB |
| VS Code | 188.9 → 185.8 G（−1.6%） | 4,347 → 4,353 MB |

残る文字列 key：tuple（element flag と名前の宣言）、template literal（text）、`cached_types`（conditional・contextual の
`D{node},{type}` など）、string literal の map（text が key。必要）。

**記録。** PR [#844](https://github.com/kazhiramatsu/tsc-rs/pull/844) → merge `260f0638a`（2026-11-02。commit：`f785403fd`、文書
`c09de3634`、main の取り込み `c90c03064`）。出力は 7 case で b2 と byte 一致。local の conformance（`c09de3634`）：12,829 case、
mismatch 0、退行 0。suites：退行 0。hosted run [41335155225](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41335155225)
（`c90c03064`）：全 job pass。

## slice 4：48 byte の node（branch `perf/ts71-node-48`）

`Node` は header 24 byte（kind・literal flag・flags・pos・end・parent・jsDoc）+ `NodeData` 32 byte = 56 byte だった。`NodeData` が
32 なのは inline の payload の最大が 24 byte（text を持つ literal 5 kind と、child slot が 6 つある宣言 9 kind、
NumericLiteral）だから（JSDoc の tag・template・JSX text は既に Box）。その 15 kind の payload を Box にして inline の最大を
20 byte（5 slot の宣言）にし、`NodeData` 24・`Node` 48 byte。構築側は `Box::new`、emitter の visitor で payload を値や参照で
受ける所は deref（どちらも compile error の span から script で書き換え）。

**計測**（1 thread、c1 = #844 → d1）：peak footprint は全 corpus で減る（VS Code 4,350 → 4,279 MB（−1.6%）、TypeScript
218 → 210（−3.7%）、Next.js 921 → 905、zod 859 → 850、effect 663 → 648、playwright 500 → 492、vue 384 → 377、hono 132 → 131）。
命令数は ±1% の揺れの中（VS Code −0.05%、hono −0.5%、zod −0.5%、playwright −0.5%、effect +0.4%、vue +1.0%、Next.js +0.9%）で、
TypeScript compiler だけ +2.3%（12.59 → 12.88 G。Box の kind は Parameter 36k = node の 3.4% で確保の費用では説明がつかず、
build ごとの code 配置の揺れと見る）。これは memory の slice。

40 byte まで下げるには Box の pointer（8 byte 整列）をやめて kind ごとの store の u32 index にする必要があり（tsrs の形）、
`node.data` の match を全部書き換えることになるので、別の slice。

**記録。** PR [#845](https://github.com/kazhiramatsu/tsc-rs/pull/845) → merge `54fd568b6`（2026-11-02。commit `f6fde16e6`、main の
取り込み `0cf097ff2`、文書）。merge 後の byte（d2 = node 48 + slice 3）で測り直すと命令数は c1 と同じ（TypeScript 12.50 → 12.49 G、
Next.js 38.27 → 38.15、VS Code 185.90 → 185.91）で、上の +2.3% は build の配置の揺れだった。peak：TypeScript 218 → 212、
Next.js 912 → 907、VS Code 4,350 → 4,285 MB。出力は 7 case で一致、local の conformance（merge 後の byte）：12,829 case、
mismatch 0、退行 0、suites 退行 0。hosted run [41338627681](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41338627681)
（`24c4e3e01`）：全 job pass。

## slice 5：resolved members の memory（branch `perf/ts71-shared-members`）

checker の memory で最大の項目 resolved members（VS Code で 1,224,722 件、464 MB）を縮める。

- **小さい member table は線形探索**：`MemberTable` は member の `Vec<SymbolId>` と名前の hash 索引（`HashTable<u32>`）を持っていた。
  8 件以下の table は索引を作らず（空の `HashTable` が印。確保なし）順に比べる。tsrs の小 table と同じ（R21）。
- **signature と index info は別 record**：call/construct signature と index info の 3 つの `Vec` は大半の型で空なのに、record の
  152 byte のうち 72 byte を占めていた。`Option<Box<MembersExtras>>` にして最初の空でない書き込みで確保（`extras_mut`、
  `set_call_signatures` 等。tsc と同じ順に一つずつ書く）。読み手は `call_signatures()` 等の slice。record は 88 byte。
- **試して捨てた案**：symbol 自身の `Arc<SymbolTable>` を members table として共有する（tsc は同じ Map を持つ）。生きている
  table をそのまま使える型は 15k 件しかなく、checker が作る一時 `SymbolTable` を Arc で持つと MemberTable の複製より大きい
  （entry 8 byte + 索引 対 4 byte + 索引）。enum の 8 byte × 1.2M 件も加わり、正味で悪化した（VS Code +49 MB）。

**計測**（1 thread、d2 = #845 の merge → e6）：

| case | 命令数 | peak footprint | peak RSS |
|---|---:|---:|---:|
| VS Code | 187.2 → 186.3 G（−0.5%） | 4,283 → 4,307 MB（+0.6%） | 4,495 → 4,354 MB（−3.1%） |
| TypeScript compiler noEmit | 12.59 → 12.56 G | 212 → 211 MB | 224 → 225 MB |
| Next.js noEmit | 38.67 → 38.30 G（−1.0%） | 907 → 893 MB | 936 → 909 MB |
| zod | 27.17 → 26.88 G（−1.1%） | 854 → 844 MB | 884 → 866 MB |
| effect | 23.09 → 23.03 G | 654 → 644 MB | 671 → 662 MB |

`TSRS_MEMORY_REPORT` の resolved members は 464 → 364 MB、`TSRS_PHASE_TRACE` の mimalloc の peak committed は
3,397 → 3,107 MiB、peak RSS 4,493 → 4,316 MiB。VS Code の `/usr/bin/time -l` の peak footprint だけ +22 MB になるのは、
解放した page が lazy に返るまで footprint に数えられるため（RSS と committed は減る）。以後は footprint と RSS の両方を記す。

**記録。** PR [#846](https://github.com/kazhiramatsu/tsc-rs/pull/846) → merge `2d8f95e67`（2026-11-02。commit `690e93f59`、文書
`ccef290b5`）。出力は 5 case で一致、local の conformance：12,829 case、mismatch 0、退行 0、suites 退行 0。hosted run
[41347213089](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41347213089)（`ccef290b5`）：全 job pass。

## slice 6：64 byte の type（branch `perf/ts71-type-48`）

`Type` は 80 byte だった（flag 2 つ・symbol・alias symbol・fresh/regular 4 × 4 byte、alias type argument の
`Option<Box<[TypeId]>>` 16 byte、`TypeData` 40 byte）。VS Code で 2.58M 件、`Vec` の容量 4M × 80 = 320 MiB。二段で 64 byte にする。

- **alias type argument を intern した list に**：`alias_type_arguments: Option<TypeListId>`（4 byte）。list の interner は
  list を一度だけ持つ（`lists: Vec<Box<[TypeId]>>` + `HashTable<TypeListId>`。以前は map の key にも複製があった）。読み手は
  `tables.alias_type_arguments(id)`（`Option<&[TypeId]>`）、書き手は `intern_alias_arguments`。subtype cache の key（slice 3）と
  同じ list。`Type` 72 byte。
- **少数の kind だけが持つ payload を Box に**：inline の最大 40 byte は intrinsic の名前 2 つ（`&'static str` × 2）、template
  literal の texts と types、bigint literal の `PseudoBigInt`、generic type の `usize` の outer parameter count。intrinsic と
  template literal は `Box<IntrinsicData>`・`Box<TemplateLiteralData>`、bigint は `Box<PseudoBigInt>`、count は `u32`
  （読み手で `as usize`）。`TemplateText` も `Vec<u16>` から `Box<[u16]>`。inline の最大は 24 byte（list を持つ kind）で
  `TypeData` 32・`Type` 64 byte（`const` の assert で固定）。読み手の書き換え（checker で Intrinsic 7 か所、TemplateLiteral
  10 か所、count 13 か所、api で 4 か所）は compile error の span から。

**計測**（1 thread、d2 = #845 の merge → f2。両段の合計）：

| case | 命令数 | peak footprint | peak RSS |
|---|---:|---:|---:|
| VS Code | 187.31 → 184.62 G（−1.4%） | 4,284 → 4,232 MB（−1.2%） | 4,476 → 4,418 MB |
| TypeScript compiler noEmit | 12.58 → 12.37 G（−1.7%） | 212 → 208 MB | 224 → 220 MB |
| Next.js noEmit | 38.71 → 37.79 G（−2.4%） | 907 → 892 MB | 936 → 920 MB |
| zod | 27.22 → 26.54 G（−2.5%） | 867 → 841 MB | 894 → 869 MB |
| effect | 23.13 → 22.75 G（−1.6%） | 654 → 637 MB | 671 → 661 MB |

一段目だけ（f1）は VS Code 187.37 → 185.87 G、4,285 → 4,253 MB。`TSRS_MEMORY_REPORT` の type record は 320 → 256 MiB、
type list map は 31.4 → 26.8 MiB（entry は 619k → 625k。alias list が加わり、複製が消えた）。命令数の減りは record が
cache line 1 本（64 byte 整列）に収まることと alias list の複製が消えたことによると見る。

main（#846 込み）を取り込んだ byte（f3）では VS Code 186.84 → 185.85 G、peak footprint 4,284 → 4,180 MB、peak RSS 4,493 → 4,287 MB
（d2 比。#846 の分を含む）。出力は 7 case で d2 と一致。

48 byte までの残り：fresh/regular の 8 byte を literal と union だけの side table に（56）、`alias_symbol` + `alias_type_arguments`
の組を `Option<AliasId>` に（48）。どちらも読み手が多いので別の slice。

**記録。** PR [#847](https://github.com/kazhiramatsu/tsc-rs/pull/847) → merge `a3232b54d`（2026-11-02。commit `3288e5202` + `3f9fb51b3`、
main の取り込み `a2da70d1a`、文書 `05794d4e1`）。hosted run
[41353036842](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41353036842)（`05794d4e1`）：全 job pass。

## slice 7：profile の小さな切り口（branch `perf/ts71-profile-cuts`）

#847 の byte で VS Code（1 thread）の samply profile を取り直した（`perf11/s-f3.json.gz`。`flat2.py` が 1 本の profile の
self/inclusive、`chains.py` が関数名を切らずに呼び出し連鎖を出す）。work 11,984 sample のうち上位は memmove 5.0%、
`PagedTable::get` 2.3%、`try_file_index_of_node` 1.8%、malloc/free 系 4.5%、scanner 5%、`__open` 2.1%（`std::fs::read` と
`read_dir`。file ごとに 1 回で正常）。driver の側に、tsc の動作と無関係に消せる項目が 4 つあった。

1. **file ごとの diagnostic の組み立て**（`semantic_diagnostics_for_program_file`、self 142 sample = 1.2%）：file ごとに checker の
   全 row を file 名で filter していた（file 1 万 × row 1 万）。`checker_diagnostics_by_file` が row を一度だけ file 別に分け、
   組み立てはその slice を読む（serial・sharded の両 driver、library prefix の完了後にもう一度）。1 file だけの getter は従来のまま。
2. **行表の遅延**（`DensePositionIndex::new`、92 sample）：parser の `to_utf16`（parse diagnostic の位置）と loader の import span の
   変換が、全 file の行表（byte と UTF-16 の行頭）まで作っていた。変換に要るのは非 ASCII 文字の表だけなので、行表は
   `OnceLock<LineTables>` にして最初の行の問い合わせで作る（static な index は従来どおり構築時）。
3. **resolved file name の正規化**（90 sample）：`resolveExternalModule` が型付きの import でも、untyped JS の拡張子判定のために
   resolved file name を正規化していた。判定は拡張子だけなので正規化前の名前で行い、untyped JS のときだけ正規化する。
4. **終了時の drop**（約 190 sample = 1.6%）：CLI は報告後に `_exit` するのに、checker の state・program の document・lib の
   document を drop してから戻っていた。`AuthoritativeRun::leak_at_end`（CLI の batch 経路が `leak_program` から渡す。
   harness・LSP・API は false）で `mem::forget` に任せる。

試して捨てた：loader の `resolve_with_facts` が request ごとに containing file を正規化する（56 sample）ので、最後の file の
正規化結果を resolver に持たせた。命令数は VS Code・Next.js・zod・TypeScript のどれも変わらず（±0.1%）、捨てた。

**記録。** PR [#848](https://github.com/kazhiramatsu/tsc-rs/pull/848) → merge `954211217`（2026-11-02。commit `b7f30d5a6` + `8c917d15d`、
main の取り込み `147192bdb`、文書 `e205ca209` + `ed6ae31ee`）。hosted run
[41359453613](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41359453613)（`ed6ae31ee`）：全 job pass。

**計測**（1 thread、f3 = #847 の byte → g3 = main を取り込んだ最終 byte）：

| case | 命令数 | peak footprint | peak RSS |
|---|---:|---:|---:|
| VS Code | 185.83 → 180.05 G（−3.1%） | 4,179 → 4,159 MB | 4,278 → 4,264 MB |
| TypeScript compiler noEmit | 12.37 → 12.27 G（−0.8%） | 207 → 206 MB | 221 → 218 MB |
| Next.js noEmit | 38.31 → 37.75 G（−1.5%） | 882 → 871 MB | 907 → 929 MB |
| zod | 26.91 → 26.59 G（−1.2%） | 831 → 824 MB | 859 → 852 MB |
| effect | 22.91 → 22.80 G（−0.5%） | 630 → 631 MB | 644 → 654 MB |
| hono | 3.74 → 3.68 G（−1.6%） | 129 → 126 MB | 143 → 141 MB |
| vue | 20.49 → 20.34 G（−0.7%） | 370 → 374 MB | 398 → 394 MB |
| playwright | 18.34 → 18.10 G（−1.3%） | 480 → 481 MB | 496 → 499 MB |

出力は 8 case で f3 と一致。1〜4 を merge 前の byte で測った g1 も同じ（VS Code 185.15 → 180.06 G）。peak は項目 4 で変わらない
（forget は確保を増やさず、peak は check の途中）。


## slice 8：PGO（branch `perf/ts71-pgo`）

利用者の指示（2026-11-02）「PGOもやってください」。tsrs の npm binary は PGO + fat LTO 込み（R7：−13〜14%。VS Code 1 thread で
PGO なし 86 G、PGO 込み 75 G）なので、比較の土俵を揃える意味もある。`scripts/pgo_build.py`：`-Cprofile-generate` の build
（`target/pgo/instrumented`）→ 与えた project を `--noEmit` で 1 process ずつ走らせて `.profraw` を集める → toolchain の
`llvm-tools` の `llvm-profdata merge` → `-Cprofile-use` の build（`target/pgo/tsc-rs`）。通常の `target/release` は触らない。
学習は bench corpora の 6 project（VS Code stable、zod、Next.js、effect、TypeScript compiler、hono）、1 thread。最初の試みは
学習 run が profile を書かなかった：CLI は `_exit` で終わるので atexit の profile writer が走らず、集まったのは build script と
proc macro の profile（`syn` が最上位）だった。`LLVM_PROFILE_FILE` が設定されているときだけ CLI は C runtime の exit を通る。

**計測**（1 thread、g3 = #848 の byte の通常 build → 同じ byte の PGO build）：

| case | 命令数 | | peak footprint | peak RSS |
|---|---:|---:|---:|---:|
| VS Code | 181.33 → 143.58 G | −20.8% | 4,159 → 4,160 MB | 4,264 → 4,266 MB |
| TypeScript compiler noEmit | 12.29 → 9.65 G | −21.5% | 206 → 206 MB | 218 → 219 MB |
| Next.js noEmit | 38.15 → 31.13 G | −18.4% | 871 → 874 MB | 929 → 924 MB |
| zod | 26.85 → 22.05 G | −17.9% | 824 → 825 MB | 862 → 862 MB |
| effect | 22.89 → 18.71 G | −18.3% | 631 → 631 MB | 654 → 654 MB |
| hono | 3.73 → 3.01 G | −19.3% | 126 → 127 MB | 141 → 142 MB |
| vue（学習外） | 20.42 → 16.90 G | −17.2% | 375 → 373 MB | 394 → 395 MB |
| playwright（学習外） | 18.27 → 14.64 G | −19.9% | 481 → 481 MB | 499 → 497 MB |

出力は 8 case で g3 と一致。peak は変わらない（同じ code、配置だけが違う）。学習に入れなかった vue と playwright も同じ幅で速く、
profile は project を跨いで効く。tsrs（PGO 込み 75 G）との比は VS Code で 1.9×。

**記録。** PR [#849](https://github.com/kazhiramatsu/tsc-rs/pull/849) → merge `5a40ff03d`（2026-11-03。commit `825eae345`）。hosted run
[41368494860](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41368494860)（`825eae345`）：全 job pass（gates は通常 build）。
以後の計測は PGO 同士（`scripts/pgo_build.py` で両方を build）か通常 build 同士で行い、混ぜない。tsrs の学習 workload は
`.github/scripts/pgo-train.sh`：conformance suite（in-process）、Go fourslash suite（LSP worker）、公開 project 2 つ（xstate-main、
webpack）。CI で再現できる同じ形は次の候補 0。

## slice 9：tsgo の統計行で仕事量を比べる（branch `perf/ts71-statistics`）

tsrs の `--extendedDiagnostics` は VS Code で Types 2,361,713 と出し、我々の `TSRS_MEMORY_REPORT` は 2,575,538 型だった（+9%）。
「同じ診断で余計な型を作っている」のが疑われたので、tsgo と同じ統計行（Identifiers・Symbols・Types・Instantiations）を
`--diagnostics` / `--extendedDiagnostics` に足し、参照の tsgo（`target/typescript7/bin/tsc-aa814927 --singleThreaded`）と比べた。
識別子は parser が割り当て時に数え（`NodeArena::identifier_count`、tsc の `identifierCount`）、driver が program の binder symbol を
一度、checker ごとに transient symbol・型・`instantiationCount` を `CheckWorkCounters` に記録し、compiler が表に出す。

| project（1 thread） | Identifiers tsc-rs / tsgo | Symbols | Types | Instantiations |
|---|---:|---:|---:|---:|
| VS Code | 5,945,718 / 5,867,751 | 8,160,599 / 8,205,379 | 2,575,538 / 2,570,757（+0.2%） | 3,387,455 / 3,389,287 |
| zod | 711,636 / 713,350 | 2,129,557 / 2,149,435 | 716,427 / 713,472（+0.4%） | 2,135,031 / 2,139,310 |
| effect | 600,838 / 605,222 | 1,268,355 / 1,273,816 | 534,541 / 532,879（+0.3%） | 1,907,749 / 1,908,646 |
| TypeScript compiler | 434,206 / 435,926 | 258,286 / 258,542 | 108,059 / 107,889（+0.2%） | 168,788 / 168,802 |
| hono | 135,761 / 134,596 | 264,611 / 264,887 | 148,022 / 147,905（+0.1%） | 416,907 / 417,231 |

**結論：仕事量は tsgo と同じ**（型 ±0.4%、instantiation ±0.2%、symbol −0.5%）。+9% は我々の余計ではなく、tsrs が既定 mode でも
tsgo より 8% 少ない型で同じ診断を出している（tsrs の R1 tuple target の共有、R12 union の構築など）。残る差（PGO 同士で
VS Code 143.6 G 対 75 G = 1.9×）は仕事の量ではなく 1 操作あたりの費用：id から record への経路（bounds check 付きの表、
page 経由の links）、hash 探索、複製と確保、parser の 2×。tsgo 本体（single thread）とは既に同等以下（check 9.5 s 対 9.8 s）。
識別子の ±1% は数え方の差（missing node・JSDoc）で、仕事量の指標ではない。

**記録。** PR [#850](https://github.com/kazhiramatsu/tsc-rs/pull/850) → merge `684e19cc1`（2026-11-02。commit `83871ec41`、docs `2063acbaa`、
main の merge `c9484e989`）。hosted run [41382242447](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41382242447)（`c9484e989`）：
全 job pass。local（`2063acbaa`）：conformance 12,829 case、mismatch 0、regressions 0、harness errors 0（`--workers 1` で 1,919 s）、
suites regressions 0、8 case で出力一致、命令数は g3 比 ±0.1%。

## slice 10：hot path の小さな切り口 2（branch `perf/ts71-hot-path-cuts-2`）

PGO の学習 profile（slice 8）は関数ごとの呼び出し回数を持つ（`llvm-profdata show --all-functions --counts`、`census.py`）。
6 project 合計の上位：`TypeTables::flags_of` 11.0 億回（VS Code 7.3 億）、`object_flags_of` 3.8 億、`__rust_alloc` 2.5 億、
`SymbolArena::symbol` 2.1 億、`type_of` 2.1 億、`PagedTable::get` 1.5 + 0.95 億、`scan_identifier_parts` 1.1 億、
`get_reduced_type` 1.1 億、`get_merged_symbol` 0.8 億、`get_apparent_type` 0.67 億。この回数から、答えが決まっている呼び出しを
入口で返す（tsrs の R19）、確保を使い回す（R16）の 2 形で切った。出力は変えない。

1. **入口で返す**（`#[inline]` の入口 + `#[inline(never)]` の本体）：`get_reduced_type`（union・intersection 以外は入口で
   そのまま返す）、`get_apparent_type`（flag と mapped の object flag を見て、plain な object 型は自分自身）、
   `could_contain_type_variables`（計算済みの bit）、`resolve_structured_type_members`（resolved な links）、
   `get_type_of_symbol`（links の `type` が入っていれば入口で返す。どの種類の getter も最初にそれを返す）。
2. **`get_merged_symbol` の dense bit**（`merged_symbol_bits`、`record_merged_symbol` が立てる）：tsgo は `symbol.MergeId != 0`
   を先に見る。VS Code の 5,200 万回のうち 4,850 万回は merge されたことのない symbol の hash 探索だった。
3. **関係判定の作業領域を state に**（`RelationScratch`：`maybe_keys` とその `HashSet`、stack 2 本）：`RelationChecker` を作る
   2 か所で `mem::take` し、`into_scratch()` で空にして返す。`checkTypeRelatedTo` ごとの確保（VS Code で `HashSet` の drop
   640 万回）が消えた。
4. `maybe_type_of_kind` は union の member slice をその場で読む（`to_vec` なし）。

**計測**（1 thread、g3 = #848 の byte → h1 = `e88615fb6`、通常 build 同士）：

| case | 命令数 | peak footprint | peak RSS |
|---|---:|---:|---:|
| VS Code | 181.00 → 173.25 G（−4.3%） | 4,160 → 4,163 MB | 4,246 → 4,254 MB |
| TypeScript compiler noEmit | 12.30 → 11.89 G（−3.3%） | 206 → 207 MB | 218 → 219 MB |
| Next.js noEmit | 38.08 → 36.31 G（−4.6%） | 871 → 871 MB | 929 → 928 MB |
| zod | 26.86 → 25.52 G（−5.0%） | 825 → 825 MB | 858 → 851 MB |
| effect | 22.88 → 21.42 G（−6.4%） | 631 → 630 MB | 654 → 649 MB |
| hono | 3.72 → 3.61 G（−3.0%） | 127 → 127 MB | 141 → 141 MB |
| vue | 20.43 → 19.04 G（−6.8%） | 373 → 370 MB | 394 → 394 MB |
| playwright | 18.27 → 17.28 G（−5.4%） | 481 → 482 MB | 499 → 498 MB |

出力は 8 case で g3 と一致。peak は変わらない（確保が減っただけで、生きている record は同じ）。累計（main `7d517bbd7` → h1、
通常 build 同士）：VS Code 228.3 → 173.2 G（−24%）。

**記録。** PR [#851](https://github.com/kazhiramatsu/tsc-rs/pull/851) → merge `526466556`（2026-11-02。commit `e88615fb6`、main の取り込み
`f3a580e6a`、docs `b696305a3`）。hosted run [41406256306](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41406256306)（`b696305a3`）：
全 job pass。local（`b696305a3`）：fmt・clippy clean、test 4,993 pass、conformance 12,829 case、mismatch 0、regressions 0、harness errors 0
（`--workers 1` で 1,041 s）、suites regressions 0、8 case で出力一致、最終 byte の h2 は g3 比 VS Code 181.43 → 172.80 G（−4.8%）。

## slice 11：型を store に、Type を 40 byte に（branch `perf/ts71-type-40`）

利用者の指示（2026-11-02）：「scanner/parser/binder で作ったデータ構造を最後まで引き継ぐ、フラグの類はビットで保持、不必要な
データはヒープに保持しない」「必要なオブジェクトはまとめて確保する」「unsafe はできるだけ避けたいが高速化には避けられないかも
しれない」。この slice はその規則を型の record に適用する。

**step 1：型を固定 address の store に**（commit `85e8305da`）。`TypeTables` の型 list は `Vec<Type>`（境界 check 付き）だった。
checker ごとに identity domain から `IdentitySpace::Type` の範囲（2^26）を lease し、`StoreVec::claim_room` でその場に作る（domain に
余りがなければ heap。読み方は同じ）。`type_of` は `StoreVec::record`（長さ以下の index を check なしで読む。store crate の中の
unsafe）で 1 load。`TypeData` は `#[repr(u8)]` で先頭の variant を `Object` にし、全 0 の record を有効な型にする（store の契約）。

**step 2：Type 64 → 40 byte、align 4**（commit `a6ff5ab82`）。record に残すのは word だけにした。

- `TypeData` は `Copy` の 24 byte：union・intersection の member list、reference の type argument、generic type と tuple target の
  type parameter、template literal の span type は、cache key 用に intern 済みだった `TypeListId`（`Box<[TypeId]>` の複製を消した）。
  tuple target と template literal の payload、intrinsic の名前、bigint の値、string literal の文字列は tables 側の list への id。
  数値は `NumberBits`（f64 の bit 列を 2 word で持ち、align 8 を避ける）。
- fresh/regular の対は header から freshable な variant（`Intrinsic`・`Literal`・`Enum`）と `Union` の payload に移した。object
  literal の regular type は record に置き場がないので、tsgo の `cachedTypes[RegularObjectLiteral]` と同じく side map
  （`regular_object_literal_types`）。
- alias symbol と alias type arguments の対は `Option<AliasId>` 1 word にし、alias を持つ型だけが tables の alias list に 8 byte
  持つ。header は `flags`・`object_flags`・`symbol`・`alias` の 16 byte。`const` assert で 40 byte と align 4 を固定。
- intern した型 list は list ごとの `Box<[TypeId]>` をやめ、一つの平らな list に「長さ、要素…」の形で並べ、id は長さの位置にした
  （list ごとの malloc が消え、読みは長さの load とその隣の要素）。
- literal の値は `LiteralValue<'_>`（借用）で読む。値を持ち帰る language service だけ `LiteralValueOwned`。memory report の型の行は
  `capacity` でなく `len` で数える。
- 書き換えは checker 約 450 か所、ls・api・test 約 60 か所（機械的な部分は script で、残りは手で）。出力は変えない。

**step 2 の命令数の戻りとその回収。** 最初の版（k1）は memory を減らした（VS Code の型の行 256 → 98 MiB、peak −156 MB）が、命令数が
step 1（i1）より VS Code +0.3%、Next.js +2.9%、vue +3.0% 戻った。vue の profile（4 回ずつ、check 区間で正規化）で増分の大半は union
を作るときの並べ替えだった：`compare_types` が string literal の text を `literal_value()`（out-of-line の関数と値の enum）経由で読み、
`compare_type_lists` が list の span 表を引く。直したこと（k2・k3）：list を「長さ、要素」の形にして span 表の間接参照を消した；
`compare_types` は record の `LiteralData` を直接比べ、同じ text id・bigint id なら読まない（`literal_data_equal` も同じ）；
`getRecursionIdentityTarget` と `maybeTypeOfKind` は入口で答えて worker を out-of-line に（R19）；checker の transient symbol は
`StoreVec::record` の 1 load（`SymbolArena::symbol` は 329 か所から out-of-line で呼ばれていた。API と emit resolver の外部 id は
checked な `SymbolArena::get` のまま）。

| case | i1（step 1） | k1（step 2 の最初） | k3（最終） | k3 対 i1 |
|---|---:|---:|---:|---:|
| Next.js noEmit | 35.55 G | 36.63 G | 35.61 G | +0.2% |
| vue | 18.53 G | 19.18 G | 18.72 G | +1.0% |
| zod | 24.94 G | 25.33 G | 24.89 G | −0.2% |

（1 thread、通常 build 同士、i1 と k3 は 2 回の平均。i1 は回ごとに ±0.5〜1% 動く。）

**計測**（最終 byte。1 thread、通常 build 同士、各 1 回。h2 = slice 10 最終 `b696305a3`、i1 = step 1、k9 = `8393ff411`）：

| case | 命令数 h2 → i1 → k9 | k9 対 h2 | peak footprint h2 → k9 | peak RSS h2 → k9 |
|---|---:|---:|---:|---:|
| VS Code | 174.19 → 169.93 → 168.94 G | −3.0% | 4,162 → 4,077 MB | 4,262 → 4,106 MB |
| TypeScript compiler noEmit | 11.92 → 11.64 → 11.70 G | −1.8% | 207 → 203 MB | 219 → 215 MB |
| Next.js noEmit | 36.72 → 35.43 → 35.65 G | −2.9% | 872 → 851 MB | 928 → 881 MB |
| zod | 25.84 → 24.87 → 24.93 G | −3.5% | 826 → 802 MB | 866 → 833 MB |
| effect | 21.54 → 20.98 → 20.99 G | −2.6% | 630 → 603 MB | 649 → 648 MB |
| hono | 3.66 → 3.56 → 3.56 G | −2.7% | 127 → 122 MB | 141 → 137 MB |
| vue | 19.15 → 18.49 → 18.72 G | −2.2% | 370 → 360 MB | 394 → 390 MB |
| playwright | 17.48 → 16.95 → 16.97 G | −2.9% | 482 → 471 MB | 498 → 486 MB |

出力は 8 case で h2 と一致。VS Code の memory report：型 256.0 → 98.2 MiB、型所有の list と text 45.5 → 26.5、type list map
26.8 → 20.4、checker 合計 2,128.8 → 1,946.2 MiB。step 1 に対する命令数は VS Code −0.6%、vue +1.2%、Next.js・TypeScript
+0.5〜0.6%、他は ±0.2% 以内で、step 2 の戻りはほぼ回収した（残りは record の値の読み方の差。memory の減り分と引き換え）。
累計（main `7d517bbd7` → k9、通常 build 同士）：VS Code 228.3 → 168.9 G（−26%）、peak footprint 4,657 → 4,077 MB。

**記録。** PR [#852](https://github.com/kazhiramatsu/tsc-rs/pull/852) → merge `819cb6e6c`（2026-11-03。step 1 `85e8305da`、step 2 `a6ff5ab82`、
main の取り込み `ddd654b78`、docs `da14acb78`、clippy の修正 `8393ff411`、計測の docs `3f8769c58`）。hosted run
[41584476366](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41584476366)（`3f8769c58`）：全 job pass。local（`8393ff411`）：fmt・clippy clean、
test 4,993 pass（language service は修正後に 256 pass）、conformance 12,829 case、mismatch 0、regressions 0（`--workers 1` で
1,028 s）、suites regressions 0、`--checkers 4` は既知の 4 件（15,335 一致）、8 case で出力一致、k9 は h2 比 VS Code −3.0%、
peak footprint −85 MB。

## slice 12：複製と確保（branch `perf/ts71-copies`）

handover §6.4。profile の memmove 635 sample（tsrs 85）と malloc/free 約 540（tsrs 60）の呼び出し元を、借用か共有か使い回しに
替えた。出力は変えない（memo の値と順序はそのまま）。

1. **返す list を共有に**：`get_properties_of_type`・`get_signatures_of_type` は呼ぶたびに `Vec` を複製して返していた。
   `SharedList<T>`（`Option<Arc<[T]>>`。clone は参照数、空は確保なし）の `SymbolIds`・`SignatureIds` を返し、resolved members と
   links の `resolved_properties`・`array_fallback_signatures`・variances も同じ list を持つ。
2. **型 list を複製せずに読む**：`TypeListCopy`（8 個まで inline）、tables を借りない `ListCursor`、`create_type_reference_with_last`
   （再利用 buffer）。union の member を数えるだけの箇所は `union_member_count`、inference の `types_of` は inline copy、`map_type`
   は cursor、`filter_type` は全部残るなら元の型を返す（tsgo の `core.Same`）。`list(..).to_vec()` の大半を `list_copy` に。
3. **式ごとの文字列と node list**：`nodes_of` は program の寿命の slice（`NodeIds<'a>`）。プロパティ名・`escaped_text_of` は
   `&'a str`。
4. **call 解決**：`choose_overload` は引数と型引数の node list を借りる。
5. **signature をその場で読む**：`is_top_signature`（tsgo の順に、型 parameter があれば this 型を解決しない）、
   `try_get_type_at_position`、`instantiate_signature`、`clone_signature`。signature の instantiation cache の key は
   `(SignatureId, TypeListId)`。
6. **instantiation**：`SmallList::clone` は inline なら word の copy。外側の型 parameter の cache は `TypeListId`。
   `create_type_mapper` などは slice を受け取る。alias と conditional の instantiation cache は値の `InstantiationKey`。
7. **inference の作業領域を state に**（`InferenceWalkBuffers`）。
8. **arena の予約**：型 table の上限を 2^20 → 2^22、transient symbol 2^22 → 2^24、inference arena も予約（VS Code で倍増のたびの
   複製と旧新 buffer の同時 resident をなくす）。memory report の signature・members・mapper・inference・symbol の行は `len` で数える。

**計測**（1 thread、通常 build 同士、各 1 回。k9 = slice 11 最終 `8393ff411`、m1 = この slice の code `26de7bce1`）：

| case | 命令数 k9 → m1 | 差 | peak footprint k9 → m1 |
|---|---:|---:|---:|
| VS Code | 170.16 → 162.36 G | −4.6% | 4,077 → 4,064 MB |
| TypeScript compiler noEmit | 11.74 → 11.32 G | −3.6% | 203 → 202 MB |
| Next.js noEmit | 36.09 → 34.10 G | −5.5% | 843 → 841 MB |
| zod | 25.29 → 23.50 G | −7.1% | 802 → 794 MB |
| effect | 21.12 → 19.54 G | −7.5% | 603 → 596 MB |
| hono | 3.61 → 3.41 G | −5.5% | 122 → 122 MB |
| vue | 18.85 → 17.92 G | −4.9% | 360 → 353 MB |
| playwright | 17.23 → 16.42 G | −4.7% | 471 → 468 MB |

出力は 8 case で k9 と一致。peak はほぼ変わらない（生きている record は同じで、減ったのは一時的な複製と確保）。

**記録。** PR [#853](https://github.com/kazhiramatsu/tsc-rs/pull/853) → merge `d998b4194`（2026-11-03。code `26de7bce1`、main の取り込み
`39daa9a51`、docs `db54d3a7f`、clippy の修正 `bb8635ef4`）。hosted run
[41590768534](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41590768534)（`bb8635ef4`）：全 job pass。local（`bb8635ef4`）：fmt・clippy
clean、test 4,993 pass、conformance 12,829 case、mismatch 0、regressions 0（`--workers 1` で 999 s）、suites regressions 0、
`--checkers 4` は既知の 4 件（15,335 一致）、8 case で出力一致。最終 byte（m9）の k9 比：VS Code 169.40 → 162.23 G（−4.2%）、
TypeScript compiler −3.6%、Next.js −5.3%、zod −6.3%、effect −7.3%、hono −4.5%、vue −4.7%、playwright −3.9%。peak footprint は
VS Code 4,076 → 4,064 MB。

## slice 13：文字列（branch `perf/ts71-strings`）

handover §6.1。同じ text を何度も複製・再符号化していた箇所を、source の範囲か 1 回だけ持つ text への id に替えた。出力は 5 の
順序の修正（tsgo に合わせた）を除いて変えない。

1. **scanner の token value を source の範囲に**：`token_value: JsString` に識別子・文字列・数値ごとに複製していた（VS Code で
   `scan_identifier_parts` 1.1 億回、15 byte を超えると heap、`save()` ごとに clone）。value は `TokenValue { start, end, cooked }`
   になり、escape・行継続・数値の正規化（separator、先頭 0、小数、指数、16 進、bigint の基数変換）があるときだけ scanner の
   scratch（`cooked: JsString`）に組み立てる。識別子は `pos` を進めるだけで、最初の `\u` escape で `text[start..escape]` を
   cooked に移す。`finish_identifier_token` は source slice で keyword を引く（以前は value を source と `memcmp` で照合していた）。
   `save()` は cooked のときだけ文字列を clone する。数値・regex・JSX text・JSDoc は `token_value_str()`（source slice なら UTF-8
   検証なし）。cooked の組み立ては今と同じ `push_js`（surrogate の結合）を通るので、同じ canonical byte になる。
2. **JSDoc の comment 片を source の借用に**：`token_text()`・`token_value()` が token ごとに `String` を作っていた。comment 片は
   `Cow<'text, str>`（escape した識別子だけ owned）。
3. **名前表を平らに**（`crates/store/src/names.rs`）：名前 1 つに `OnceLock` の slot 32 B + leak した `JsString` 24 B（+ 15 B 超の
   heap）を持ち、読みは chunk の `OnceLock` 2 段と Inline/Heap の分岐だった。名前の record（8 B：text の開始位置と長さ、lone
   surrogate の flag）を region 8 に、text を region 9 の append-only な byte 列に置き、読みは record の 1 load と slice。
   `as_str()` は intern 時に検証した flag を見るだけ（`from_utf8_unchecked` は store crate の中）。`JsStr` には canonical な byte の
   複製を包む `from_canonical_bytes`（debug build で検証）を足した。shard の選択を hash の上位 bit から中位 bit に直した（上位
   bit は hashbrown の 7 bit tag と重なり、shard 内の tag がほぼ 2 値になっていた）。
4. **string literal 型の表を 1 つに**：同じ text を UTF-16 key の map、UTF-8 key の map、`literal_texts`（UTF-16）の 3 か所に持って
   いた（VS Code で 2 map 56 MiB）。text は 1 本の byte 列（`LiteralTexts`、canonical WTF-8、text ごとの (start, end)）に 1 回だけ
   置き、表は WTF-8 byte の hash で引く `HashTable<(LiteralTextId, TypeId)>`。`LiteralValue::String` は `JsStr` の view になり、
   `to_js_string()`（UTF-16 から WTF-8 への再符号化）・`to_utf8()`（`String` の確保）をしていた読み手は借用で済む。template
   literal の text は UTF-16 のまま（`TemplateText`）で、union の縮約で string literal と template literal を総当たりする端の test は
   literal をその場で読む（複製しない）。
5. **文字列の順序を tsgo に合わせる**：tsgo は string literal 型（`compareTypes`）、symbol 名（`compareSymbols`・
   `compareTypeNames`）、tuple の label（`compareElementLabels`）を Go の string の `strings.Compare`（byte 順、つまり code point
   順）で比べる。tsc-rs は tsc（JavaScript）の UTF-16 の code unit 順で比べていたので、U+FFFF を超える文字と U+E000〜U+FFFF の
   文字の順が逆になり、union の表示順が tsgo と違った（`const u: "\u{10000}" | "\uE000" = 1` の message で tsgo は `"\uE000"` を
   先に出す）。canonical WTF-8 の byte 順で比べる（lone surrogate も code point の位置で、tsgo と同じ）。比較は 1 回の `memcmp`
   になる。単体 test（`type_order`）は tsgo 7.1.0-dev `aa814927` の message で pin。

入れていないもの（handover §8。利用者の判断で後回し、2026-11-03）：known name の id を静的表から seed する（全 `EscapedName` の
id がずれる、§8-6。見込みは命令数 −0.1〜−0.2%）、string literal の text を process 全体の名前表に intern する（LSP で表が育つ、
§8-7。主に memory、命令数 −0.2〜−0.5% の見込み）。

**計測**（1 thread、通常 build 同士、各 1 回。k9 = slice 11 最終 `8393ff411`、n5 = この slice の code `3d5bd9555`。slice 12 は
含まない）：

| case | 命令数 k9 → n5 | 差 | peak footprint k9 → n5 |
|---|---:|---:|---:|
| VS Code | 169.78 → 164.74 G | −3.0% | 4,076 → 4,003 MB |
| TypeScript compiler noEmit | 11.73 → 11.40 G | −2.8% | 203 → 199 MB |
| Next.js noEmit | 36.05 → 34.77 G | −3.6% | 843 → 840 MB |
| zod | 25.18 → 24.24 G | −3.7% | 801 → 800 MB |
| effect | 21.05 → 20.30 G | −3.6% | 603 → 581 MB |
| hono | 3.61 → 3.46 G | −4.2% | 122 → 122 MB |
| vue | 18.82 → 18.72 G | −0.5% | 361 → 357 MB |
| playwright | 17.17 → 16.47 G | −4.1% | 471 → 461 MB |

出力は 8 case で k9 と一致（5 の順序が変わる文字はこれらの project にない）。最初の版（n1）は hono +14%、vue +11% だった。
samply（10 kHz、3 回ずつ）で見ると、増分は string literal（WTF-8）と template literal の text（UTF-16）の境目の変換で、union の
縮約が全 string literal を全 template literal と突き合わせる端の test が literal を code unit ごとに decode していた
（`CodeUnits::next` 25 → 980 sample）。template の text が ASCII の間は byte で比べる（ASCII の byte は WTF-8 で 1 code point・
1 code unit なので、先頭は前から、末尾は後ろから比べて最初の差で決まる）、`TemplateText::from_js` と UTF-16 からの lookup に
ASCII の近道、literal の text を 1 本の byte 列に置く（読みに inline/heap の分岐がない）、で消した。

**記録。** PR [#854](https://github.com/kazhiramatsu/tsc-rs/pull/854) → merge `39058c48c`（2026-11-03。code `3d5bd9555`、main の取り込み
`87366ac30`、順序の修正 `635132d4a`、clippy の修正 `6fe2c46a4`、docs `87864cb63`、fmt `d0152ba8e`）。hosted run
[41604591267](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41604591267)（`d0152ba8e`）：attempt 2 で全 job pass（attempt 1 の `rust` の
失敗は LSP の `exit` の race で、この slice より古い。#855 で直す）。local（`d0152ba8e`）：fmt・clippy clean、test 4,999 pass、
conformance 12,829 case、mismatch 0、regressions 0（`--workers 1` で 890 s）、suites regressions 0、`--checkers 4` は既知の 4 件
（15,335 一致）、8 case で出力が m9 と一致。最終 byte（n9）の m9（slice 12 最終 `bb8635ef4`）比：VS Code 163.08 → 157.35 G
（−3.5%）、TypeScript compiler −3.0%、Next.js −4.4%、zod −4.2%、effect −4.3%、hono −4.4%、vue −1.5%、playwright −4.5%。
peak footprint は VS Code 4,065 → 3,998 MB、effect 596 → 573 MB。VS Code の memory report（1 thread）：string literal の表
56.1 → 3.9 MiB（text は型が持つ byte 列の行に入る）、名前表の行 4.9 → 6.7 MiB（record 8 B × 23.8 万を数えるようになった分。以前の
行は text の byte だけで、名前ごとの slot 32 B と `JsString` 24 B は数えていなかった）。

## slice 14：links と routing、profile の小さな切り口 3（branch `perf/ts71-links-routing`）

handover §6.5 と戦略 v2 の B-1、M0 の「profile の小さな切り口 3」。links の読み書きの段数と、node から file を引く routing を
減らした。出力は変えない。

1. **symbol links を persistent と transient に分ける**：`PagedTable<SymbolId, SymbolLinks>`（28 B、persistent と transient の
   id を交互に置いた 4 段の load）を、binder の symbol 用の `PagedTable<PersistentSymbol, PersistentSymbolLinks>`（12 B：
   declared type、type、is_referenced。key は index そのまま）と、checker の transient symbol 用の `Vec<SymbolLinks>`（位置は
   transient bit を外した id、1 回の境界 check と load、arena と同じ量を予約）に分けた。check flags・target・mapper・name type は
   transient symbol にしかない（tsc の `TransientSymbol` の links）ので、persistent symbol では未設定として読む（`getCheckFlags` が
   0 を返すのと同じ）。`symbol(id)` は record の値（Copy）を返す。
2. **routing を node の flag で**：`is_in_js_file` は node の `JAVA_SCRIPT_FILE`、hosted JSDoc の表は flag が無ければ引かない。
   binder の `get_name_of_declaration`・`get_non_assigned_name_of_declaration`・`has_dynamic_name`・overload の host と、checker
   で `F(self.binder.source_of_node(X), X)` の形の 47 か所を、file を引かない `&Published`（`NodeLookup`）に。modifier flag の
   合成（`effective_modifier_flags`・`combined_modifier_flags`）も JS でなければ syntactic な flag を `Published` で読む。
3. **flow の arena を bind の終わりに詰める**（`shrink_to_fit`）。
4. **type links を dense に**：`TypeLinks` 36 → 24 B（type parameter の target・mapper・default は使う型が少ないので sparse 表へ）、
   checker の型 id は 0 から密なので 1 本の `Vec`（`DenseLinks`、型 table と同じ量を予約）。
5. **`resolve_name` の locals**：binder の node flag byte の bit 6 を `HAS_LOCALS` にし（binder が locals の表を作る箇所で
   立て、消す箇所で消す。hash 表は走査しない）、`locals_of` は bit を見てから file の表を引く。locals を持たない scope（大半）は routing も hash も無し。
6. `PagedTable` の far path（dense な範囲の外の page）を `#[cold] #[inline(never)]` に。
7. 小物：`set_node_resolved_symbol` の使っていない計算と `instantiation_root`、`DenseOwners::lookup_file` は owner の file を直接。

profile の小さな切り口（戦略 v2 §1.3。VS Code の self ms は n5）：

8. **arrow の行末**：`checkGrammarArrowFunction` は `=>` の token の開始と終わりの行を比べるために file の行表（byte と UTF-16 の
   2 本）を作っていた（`build_lines` 38 ms、ほぼ全 file）。token の text は前の trivia と `=>` なので、行が違うのは text に行末
   文字（`\n`・`\r`・U+2028・U+2029）があるときに限る。text を走査する（tsrs と同じ）。
9. **`Cannot find name` の message**：`getResolvedSymbol`・`resolveEntityName` は識別子ごとに 40 本の文字列の `match` で message を
   選んでから名前を引いていた（41 ms）。選ぶのは名前・親・option だけを読む純粋な関数なので、引けなかったときに選ぶ。
10. **keyword**：`keyword_kind` は 85 本の文字列の `match` だった（scanner の識別子ごと、binder の strict mode の識別子ごと：
    22 + 16 ms）。2〜11 byte の小文字 ASCII だけを先頭の文字で分けて比べる（最大 9 本）。全 keyword の単体 test。
11. **Node の core module**：`is_node_core_module` は 55 本の線形探索（module 参照ごと、20 ms）。list は byte 順なので二分探索
    （順序と全 module の単体 test）。
12. **binder の JS だけの入口**（`bind_jsdoc` 18、`bind_reparsed_jsdoc_namespaces` 28 ms）：node ごとの呼び出しで root の flag を
    読んでいた。file の JS flag は binder を作るときに 1 回読み、入口は inline の分岐、本体は別関数。
13. **tracer の sample**（`trace_node_sample` 13 ms）：式ごとの呼び出し。tracer が無いときは inline の分岐だけ、本体は cold。
14. **speculation の journal**（`journal_node_resolution` 23 ms）：重複の確認が journal 全体の線形探索だった。transaction は LIFO で
    終わり、終わるときに自分の entry を外すので、今の transaction の entry は末尾に並ぶ。末尾の同じ深さの entry だけを見る。

**計測**（1 thread、通常 build 同士、各 1 回。n9 = slice 13 最終 `d0152ba8e`、o1 = 1〜7、o2 = 1〜14。命令数は 2 回の A/B
（n9 対 o1、o1 対 o2。o1 の 2 回の差は VS Code で 0.6%）、peak footprint は n9 → o1 → o2）：

| case | 命令数 n9 → o1 | 差 | 命令数 o1 → o2 | 差 | peak footprint |
|---|---:|---:|---:|---:|---:|
| VS Code | 157.16 → 148.15 G | −5.7% | 149.10 → 145.65 G | −2.3% | 3,998 → 3,901 → 3,889 MB |
| TypeScript compiler noEmit | 11.01 → 10.36 G | −6.0% | 10.42 → 10.23 G | −1.8% | 198 → 195 → 193 MB |
| Next.js noEmit | 33.11 → 31.00 G | −6.4% | 31.31 → 30.65 G | −2.1% | 829 → 814 → 810 MB |
| zod | 22.94 → 21.24 G | −7.4% | 21.49 → 21.00 G | −2.3% | 794 → 775 → 774 MB |
| effect | 18.84 → 17.46 G | −7.3% | 17.54 → 17.22 G | −1.8% | 573 → 558 → 555 MB |
| hono | 3.33 → 3.19 G | −4.4% | 3.22 → 3.14 G | −2.7% | 120 → 119 → 119 MB |
| vue | 17.77 → 16.82 G | −5.3% | 16.89 → 16.68 G | −1.3% | 345 → 339 → 339 MB |
| playwright | 15.95 → 14.94 G | −6.3% | 15.11 → 14.72 G | −2.6% | 459 → 449 → 448 MB |

出力は 8 case で n9 と一致。VS Code の memory report（1 thread、n9 → o2）：symbol links 253.6 MiB（交互配置の page 表）→
persistent 31.1 + transient 157.3（570 万 × 28 B）、type links 60.9 → 58.9 MiB（36 → 24 B、dense）、binder の flow node 105.2 →
72.9 MiB（`shrink_to_fit`）、binder 合計 490.5 → 458.2 MiB。予約した transient symbol と type の links は長さで数える（slice 12 と
同じ。未使用の予約は address 空間だけ）。memory report の型 table の行は、union・intersection・indexed access の map を数えて
いなかった（代入で上書きしていた）ので直した。

**記録。** PR [#857](https://github.com/kazhiramatsu/tsc-rs/pull/857) → merge `d7cf969e0`（2026-11-03。links と routing `4a1c35081`・`f29c5307b`、
小さな切り口 `c856331ea`〜`96897a0cc`、memory report `95a8df81d`・`8bf5cbbe9`、locals の bit `6ec99de98`・`44f20edcd`・`7e04bbfa5`、docs
`6e964f99c`、main の取り込み `327de251b`）。hosted run [41636415458](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41636415458)（`327de251b`）：
全 job pass。最初の hosted run（`44f20edcd`）は conformance で regression 2 件（`compiler/jsDocGenericOverloads`、
`conformance/jsdocVariadicInOverload`：JS の `@overload` で template の名前が引けない）。clippy の `iter_over_hash_type` を避けて
locals の bit を表を作る箇所で立てる形にしたとき、`@overload` の template comment の表を漏らしていた。`7e04bbfa5` で直し、bit の
無い表を debug build で検出する check を足した。local（利用者の 2026-11-03 の規則：hosted がしないことだけ。`7e04bbfa5`）：
fmt・clippy clean、変えた crate の test（syntax・binder・types・checker、debug）2,224 pass、parser・binder の fingerprint は
baseline と一致（143,190 行）、`--checkers 4` は既知の 4 件（15,335 一致）、8 case で出力が n9 と一致。最終 byte（o9）の n9 比
（他の lane の小さな build と同時に測ったので ±1%）：VS Code 159.04 → 147.68 G（−7.1%）、TypeScript compiler −7.4%、Next.js
−8.2%、zod −9.8%、effect −9.2%、hono −7.6%、vue −6.4%、playwright −9.2%。静かな run では o9 の VS Code は 145.33 G。peak
footprint は VS Code 3,995 → 3,881 MB。

## toolchain 1.99.1（branch `perf/ts71-toolchain-1.99`）

M0 の終わり（戦略 v2 §9-6、利用者の 2026-11-03 の決定：M0 の終わりに pin を上げ、全 gate を回す PR 一つで）。
`rust-toolchain.toml` を 1.93.0 から 1.99.1（tsrs が build に使う toolchain）に。rustfmt 1.99.1 は何も変えない。clippy 1.99.1 の
新しい lint（`collapsible_match` 28、`while_let_loop` 8、`chunks_exact_to_as_chunks` 8 ほか。test target を含む）を書き換えた。
`collapsible_match` の提案どおりにすると `match` が網羅でなくなる 1 か所（`crates/program/src/config_matcher.rs`）は明示の腕を足した。
挙動は変えない。

**計測**（1 thread、通常 build 同士、各 1 回。o9 = slice 14 の最終 byte を 1.93.0 で、q1 = この branch）：VS Code 145.33 →
143.37 G（−1.4%）、TypeScript compiler −1.6%、Next.js −1.3%、zod −1.2%、effect −1.2%、hono −1.3%、vue −1.8%、playwright
−1.2%。peak footprint は変わらない（VS Code 3,879 → 3,877 MB）。8 case で出力が o9 と一致。

**記録。** PR [#859](https://github.com/kazhiramatsu/tsc-rs/pull/859) → merge `c6c088301`（2026-11-03。bump と lint `0b2a0d5c7`、
main の取り込み `e77b7add7`・`8132165cb`）。hosted run [41644118804](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41644118804)
（`8132165cb`、1.99.1 の cache なし）：全 job pass（rust 45 分、conformance 25 分）。local（hosted がしないことだけ）：fmt・clippy
clean（1.99.1）、`generate_nodes.py --check`、書き換えた crate の test（types・syntax・program・emitter）1,342 pass、8 case の出力一致。

## M1 の下準備：front end の oracle・byte scanner・tsgo の census（PR #856・#858・#862・#860・#861）

戦略 v2（`target/claude-tools/handover/perf-tsrs/strategy-2026-11-03.md`、利用者の決定 2026-11-03）の M1（front end の
書き直し）に先立つ道具と、その最初の slice。Fable 5.1 の session が Opus（M0 の writer）と並行して進めた。Codex（`gpt-6.1-sol`）
の lane は隔離した clone で実装し、Fable が主張を全て再実行してから commit した。

- **node の生成器**（#856、branch `perf/ts71-node-generator`）：`scripts/generate_nodes.py` が `crates/syntax/nodes.schema.json` と
  `nodes.layout.json` から `nodes.rs`・`for_each_child.rs`・`relocate.rs`・`observable_fields.rs` を byte 単位で再生成する
  （`--check` を `.github/ci/replay.py` の RUST_CHECKS に追加）。`scripts/node_layout.py` は record の配置の報告（195 種のうち 152 種が
  3 word に収まる。record 787 → 525 MiB、tail 5.8 MiB）。store の test の隔離修正（`ROOM = 8_000_000`）を含む。
- **fingerprint oracle**（#858、branch `perf/ts71-syntax-fingerprint`）：`crates/binder/examples/syntax_fingerprint.rs` と
  `scripts/syntax_fingerprint.py` が parser と binder の出力の構造 hash（id に依存しない。両 JSDoc mode）を conformance corpus と
  8 project の source について出す。baseline は `target/claude-tools/handover/perf-tsrs/syntax-baseline/main-39058c48c.tsv`
  （143,190 行、panic 0）。front end を変える slice は `--compare` で差分 0 を示す（新しい local check の作法、§3.3）。
- **byte scanner**（#862、branch `perf/ts71-byte-scanner`、A-1b）：scanner を tsgo の `scanner.go` と同じく byte で動かす。先頭 byte の
  256 entry の表で分岐、コメント・文字列・template 本体は byte run（`memchr`）、keyword は長さ別の完全 hash（#857 の先頭文字分岐を
  置換）、`ScannerState` は `Copy`（cooked 値は snapshot pool）、稀な経路は `#[cold]`、ASCII 識別子表。public API と token・位置・値・
  flag の出力は変えない（oracle 差分 0。Codex の read-only review でも tokenization の差なし。指摘のうち U+2028 で終わるコメント列の
  2 次関数的な走査と、parser が毎回呼ぶ `re_scan_greater_token` の `#[cold]` は直した）。scan_bench
  （`crates/syntax/examples/scan_bench.rs`、10 rounds、`/usr/bin/time -l`、各 3 回の中央値）で main 対 branch：VS Code `src`
  543.2 → 203.8 命令/token（0.375×）、25.25 → 11.91 ns/token；conformance corpus 608.1 → 392.1（0.645×）、23.59 → 16.65 ns/token。
- **tsgo の census**（#860、branch `perf/ts71-tsgo-census`）：`scripts/tsgo_census.py build|run|compare` が vendored tsgo（aa8149273）の
  checker の関数（`scripts/tsgo_census/functions.tsv`、50 行）に入口 counter を Go の AST 書き換えで入れて offline で build し、8 project
  を `--singleThreaded` で走らせて `{関数: 入口回数}` を出す（D 柱の census。hono の stdout・stderr・exit は reference と一致。
  `compareTypes` だけ run ごとに数回ぶれる）。結果は `target/claude-tools/handover/perf-tsrs/census/`。
- **構造比較の漏れ**（#861、branch `fix/module-declaration-attributes-equality`）：`ModuleDeclaration.attributes`（2517f726b が手で
  足した field）を `node_data_structurally_equal` に。生成器の `equalitySkips` を消して再生成、回帰 test。呼び手は workspace にないので
  出力は変わらない。

**記録。** #855（LSP の `exit` の race。slice 13 の hosted failure の原因）→ merge `5f05fdbed`（2026-11-03。hosted run
[41623049972](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41623049972) 全 job pass）。
PR [#856](https://github.com/kazhiramatsu/tsc-rs/pull/856) → merge `d83d222a9`（2026-11-03。hosted run
[41629176798](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41629176798) `301214e1f` 全 job pass）。
PR [#858](https://github.com/kazhiramatsu/tsc-rs/pull/858) → merge `a8d70a1dc`（2026-11-03。hosted run
[41652954413](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41652954413) `bff28494f` 全 job pass）。local：baseline の作成と
`--compare` の再実行（差分 0）。
PR [#862](https://github.com/kazhiramatsu/tsc-rs/pull/862) → merge `49912cdfa`（2026-11-03。hosted run
[41659808528](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41659808528) `e230e75e6` 全 job pass）。local（hosted がしないことだけ）：
syntax crate の test 260 pass、fmt・clippy clean（1.93.0。1.99.1 は変更箇所に lint なし）、fingerprint `--compare` 143,190 行 差分 0
（rebase の前後と review 修正の後）、scan_bench は上記。
PR [#860](https://github.com/kazhiramatsu/tsc-rs/pull/860) → merge `6cfc00be1`（hosted run
[41668205298](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41668205298) 全 job pass。local：`test_census.py` 6 件 pass）。
PR [#861](https://github.com/kazhiramatsu/tsc-rs/pull/861) → merge `be038cd52`（hosted run
[41674368256](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41674368256) 全 job pass。local：syntax crate の test 255 pass、
`generate_nodes.py --check`）。
PR [#863](https://github.com/kazhiramatsu/tsc-rs/pull/863) → merge `a4d0c5297`（hosted run
[41683028311](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41683028311) 全 job pass。census の tsc-rs 側：`run-rs` と同一境界の行。local：
unit test 15 件、hono の再実行で counter 一致、`compare --summary` の再現）。
PR [#864](https://github.com/kazhiramatsu/tsc-rs/pull/864) → merge `df52e9967`（hosted run
[41692409823](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41692409823) 全 job pass。B-5 step 1+2：投機 transaction の余分な診断複製と
結果を取り消せない 19 個の links mark の削除（−1,640 行）。local（`2d0359f42`）：checker test 1,836 pass（lane）、`--checkers 4` は前回の
4-checker 報告と 15,339 構成一致・one-checker 報告とは既知の 4 件、8 case で出力が q1 と一致）。

## A-1a part 1：payload の accessor と literal text の id（branch `perf/ts71-node-records`）

M1 の A-1a（戦略 v2 §4.1、32 byte の node record）の前半。record を詰める前に、node の payload を読み書きする全箇所が
通る形にした。record はまだ 48 byte。

1. **accessor**：`Node` の payload を private にし、`data()`・`data_mut()`・`set_data()`・`Node::new` を通す（generator の
   template。全 crate の読み書きを compile error から機械的に書き換え）。
2. **literal text を id に**：19 個の text field（文字列・template・数値・bigint・正規表現・JSX の literal、template の raw
   text、JSDoc の text と link）と `JSDocComment::Text` は `String`/`JsString` ではなく `TextId`。tree ごとの text store
   （store の region 12、長さの word と 4 byte ずつの本体。長さの word の最上位 bit が「UTF-8 でない」）に置き、新しい
   identity space `NodeText` で node と一緒に lease・publish・relocate する。program の tree は
   `tsc_syntax::published_text_js`/`_utf8`、emit session の tree は `TransformArena::text_js`/`text_utf8` で読む。UTF-8 として
   読むのは数値・bigint・正規表現・JSX・JSDoc・raw text だけで、文字列と template の値は JS の view（lone surrogate）。
   別の tree へ payload を写すとき（incremental parse の subtree copy、emitter の source をまたぐ reuse clone）は text も写し
   先の store に入れ直す（後者は emitter の literal update contract test で見つかった。`remap_node_data_ids` を公開）。
   構造比較は text を中身で比べる。

**計測**（1 thread、通常 build 同士、各 1 回、静かな machine。main `2b006e078` → part 1 `1dc438ee4`）：VS Code 138.39 →
138.79 G（+0.3%）、TypeScript compiler −0.2%、Next.js −0.9%、zod −1.3%、effect −0.1%、hono −1.1%、vue −0.4%、playwright
+0.2%。peak footprint は変わらない（VS Code 3,879 → 3,886 MB）。下準備なので中立。8 case で出力が main と一致、fingerprint
（143,190 行）は baseline と一致。

**次（part 2、branch `perf/ts71-node-packing`）の基準**：32 byte の record と tree ごとの tail、kind ごとの decoder を
inline した by-value の `data()`。part 1 比（#862 込みの main 上、静かな machine）：命令数 VS Code +2.0%（140.59 →
143.36 G）、ts +2.9%、effect +2.0%、vue +0.4%、playwright +0.1%、next −0.4%、hono −0.7%、zod −1.3%。peak footprint は
8 case すべて −5〜−9%（VS Code 3,887 → 3,623 MB）、CPU 時間は同じか減。最初の by-value decode（関数呼び出し）は
+5〜+11% だったので、kind ごとの decoder・`as_<kind>()`・共有 field の slot 読み（`<field>_field()`）・walk の kind 分岐・
`#[inline(always)]` で詰めた。

**記録。** PR [#866](https://github.com/kazhiramatsu/tsc-rs/pull/866) → merge `fe0d0fc28`（2026-11-04。phase 1 `eee5c76a7`・
`8f15a48ed`、phase 2 `9527742d8`〜`c51736979`、修正 `991714abd`・`022b9d424`・`ab35f2fa9`・`ee022b4e1`、main の取り込み）。hosted run
[41728193170](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41728193170)（`ee022b4e1`）：全 job pass。それまでの 2 回の
hosted run が 1 件ずつ回帰を見つけた。(1) `conformance/jsDeclarationsPackageJson` の `.types`：node builder の `reuseName`（と module
specifier の override）が、別の file から再利用した node（text は写し先の tree に入れ直した copy）の text を published store から読んで
いた → node 自身の tree から読む。(2) `lsp/TestOrganizeImports_dtsUnusedImportWithAugmentation`：organize imports が module
augmentation の名前と import の specifier を `TextId` 同士で比べていた（id は置き場所の名前で、同じ中身でも違う）→ 中身で比べる。
legacy decorator の比較と、emitter の「変わらない update」判定（`TransformArena::same_payload`）も同じ。fingerprint は構造と各 tree での
中身を hash するのでこの種類は見えない。local（hosted がしないことだけ）：変えた 8 crate の test 3,815 pass（`ee022b4e1` で checker・
syntax・emitter・ls を再実行）、fmt・clippy・4 つの生成器の `--check`、fingerprint 143,190 行 差分 0、全 suite の `--check`（0 regressions）、
8 case で出力が main と一致、serial は上の通り。

## A-1a part 2：32 byte の node record（branch `perf/ts71-node-packing`）

A-1a の後半（戦略 v2 §4.1）。`Node` を 48 → 32 byte に詰め、payload を `Copy` の値として record から kind ごとに読む。

1. **record**：`kind`・`literal_flags`（bit 15 が `HAS_TAIL`。`literal_flags()`/`set_literal_flags()` は見せない）・`flags`・`pos`・
   `end`・`parent` と payload の 3 word。生成器（`scripts/generate_nodes.py`、data は `nodes.layout.json`）が kind ごとに payload を
   slot に並べる（node・array・text・名前の field が 1 word ずつ、boolean と `SyntaxKind` が 1 word。省略可能な boolean は 2 bit）。
   3 slot 以下か、4 番目以降が空なら record に全部入り、JSDoc の付属は空いた word へ。そうでなければ word 0 が tail を指し、tail に
   3 番目以降の slot・JSDoc の付属・JSDoc の comment（4 word）。4 slot 以上の kind は hot な 3 slot を先に置く（`hotSlots`、
   node-layout-report.md の規則：必須を先に、残りは VS Code での出現の多い順）ので tail は稀。
2. **tail**（`tsc_store::tails`、store の region 13）：payload は record だけから読む（組み立て中の tree でも公開後の tree でも）ので、
   tail は動かない位置に置く。tree が process 全体の allocator から chunk（16 word から倍々に 4,096 まで）で持ち、drop で 0 に戻して
   返す。`NodeArena::clone` は tail も写す。書き込みは arena から（`alloc_node`、`set_data`・`replace_js_doc` は新しい tail、組み立て
   中の `set_js_doc` と relocation は同じ場所、API decoder の kind 替えは `reshape`）。
3. **view**：`data()` は `NodeData` を値で返し、常に inline（kind で分岐して kind ごとの decoder へ。呼び手の `match` は自分が名指す
   kind だけを読む）。`Node::as_<kind>()` は 1 kind だけを読む（873 か所の単一 kind の判定を `a1/as_kind.py` で置換）。3 kind 以上が共有する
   29 field は `Node::<field>_field()` が slot を直接読む（node_util の field helper）。`for_each_child` は record の kind で分岐。encode も
   kind ごとに特化（`store::<N, COMMENT>`）。relocation は生成した slot mask で identity の word を直接写す。field の無い 7 kind
   （EmptyStatement・JSDocAllType ほか）は parser が token として作るものがあるので、印の word で `Token` と区別する。

**計測**（1 thread、通常 build 同士、各 1 回、静かな machine。part 1 = main `fe0d0fc28` 相当の `ee022b4e1`、part 2 = `51ee6e123`）：
命令数 VS Code 138.57 → 141.97 G（+2.5%）、TypeScript compiler +3.1%、effect +2.1%、playwright +1.2%、vue +0.9%、Next.js +0.4%、
zod −0.2%、hono −0.7%。peak footprint は 8 case すべて減（VS Code 3,893 → 3,624 MB、−269 MB。ts 193 → 176、playwright 446 →
414、vue 339 → 317）。CPU 時間はほぼ同じ。VS Code の memory report の node record 787.0 → 533.4 MiB。最初の by-value の view
（inline されない `data()`）は命令数 +5〜+11% だった。kind ごとの decoder・`as_<kind>()`・slot 読み・walk の kind 分岐・inline で
この表まで詰めた。8 case で出力が part 1 と一致、fingerprint（143,190 行）は baseline と一致。release binary は 30.9 → 33.2 MB。
命令数の +0.4〜+3.1%（6 project）と peak の −3〜−9%（8 project）の交換が、A-1c/A-1d（その場での組み立て、relocation の融合）の
出発点で、以後の slice はこの表を基準にする。

**記録。** PR [#867](https://github.com/kazhiramatsu/tsc-rs/pull/867) → merge `8e587f401`（2026-11-04。record と tail `85e05d3ca`〜
`b878fa2cc`、kind ごとの decoder・共有 field の slot 読み・1 kind の読み `c6aacebb7`〜`e1fa86175`、decode の inline と kind ごとの encode
`e84d3c012`、test `95249bfbf`、part 1 と main の取り込み）。hosted run
[41736334002](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41736334002)（`ea0793b14`）：全 job pass。local（hosted がしないこと
だけ）：変えた 8 crate の test（store 15・syntax 267・binder 80・program 684・emitter 352・checker 1,837・ls 256・api 332、0 failed。
syntax・emitter・ls は part 1 の最後の修正を取り込んだ `51ee6e123` で再実行）、fmt・clippy・4 つの生成器の `--check`、fingerprint
143,190 行 差分 0、8 case で出力が part 1 と一致、serial は上の通り。

## 次の候補（効果の大きい順の見込み）

利用者の指示（2026-11-02）：「arena の改造が必要な気がします。やはり間接参照が入る分遅くなる気がします」「JsString と JsStr の
二つも見直しが必要かと思います」「peak memory があまり変わらないですねえ」。調べた現状：

- **間接参照**：node・symbol は既に固定 address（`store::get` = base + id × size。tsrs の `P<T>` の u32 offset と同じ形）。残るのは
  型（境界 check 付きの `Vec<Type>`：`flags_of` が VS Code で 7.3 億回）、transient symbol（分岐 + `StoreVec` の境界 check）、
  links の `PagedTable`（4 load。tsrs の `IdLinkStore` も同じ形）、file routing（`try_file_index_of_node` 約 1.2%、440 か所）で、
  sample の約 7%。
- **record の大きさ**：Node 48 / Symbol 48 / Type 64 byte 対 tsrs の 24 / 16 / 24 byte の header + 種類別の tail。cache miss の差。
- **文字列**：識別子と symbol 名は intern 済み（`EscapedName` u32、member 表は u32 比較）。残るのは scanner の `token_value: JsString`
  （識別子ごとに複製、6 project で 1.1 億回。15 byte 超は heap、`save()` が clone）、名前表の読み（2 段の `OnceLock` chunk +
  Inline/Heap の分岐：`as_js` 5,200 万回、`JsString == str` 3,000 万回）、`JsString` の Inline/Heap enum（`Option` で 24 byte）。
- **複製と確保**：sample の約 12% で、上の 2 つより大きい。
- **memory の内訳**（VS Code 1 thread、#847 の byte、check 終了時、MiB）：node 787（1,720 万 × 48）、binder 490（symbol 104、
  per-node 表 148、flow 116、表 85）、checker 2,129（型 256 + list 45、文字列 literal の表 56、node links 150、symbol links
  254 + 28、type links 61 + 11、transient symbol 384、signature 147、resolved members 364、mapper 96、関係 cache 64、inference の
  arena 120）、source text 136。合計約 3,640 MiB。peak footprint 4,160 との差 540 は check 途中の一時物・`Vec` の余り・hashbrown
  の load（mimalloc の peak committed 約 3.1 GB 対 live 約 2.5 GB。store の region 約 1.25 GB）。tsrs は同じ machine で VS Code
  2,021 MB、zod 445、effect 332、Next.js 456、TypeScript 110、hono 74。tsrs の lazy な member 表（R2、−23%）は既定 mode では
  off で、診断が変わりうるので採らない。

以後は peak memory を命令数と同格の目標にし、slice ごとに peak footprint と（説明のつかない分のために）peak 時の mimalloc の
統計を報告する。順序：

0. PGO の学習を CI で再現できる形に（tsrs と同じ：conformance suite を in-process で走らせる `conformance-ts71`、LSP の
   `suites-ts71`、pin した公開 project 2 つ）。release binary を CI で作るならこの job が要る。
1. **型を固定 address の store に**（slice 11、branch `perf/ts71-type-store`）：checker ごとに範囲を claim してその場で作り、
   `type_of` を 1 load に（`TypeData` の tag 0 は全 0 で有効な `Object`、`Type` は 64 byte 以下）。transient symbol も `store::get`
   で読む。見込み −2〜3%。
2. **Type 64 → 40 byte**：union・intersection・reference の list を `Box<[TypeId]>` から `TypeListId` に（match 231 か所）、
   literal の文字列を id に、fresh/regular を links に、f64 は bit 列（align 4）。型 256 MiB → 160。
3. **文字列**（slice 12）：escape のない識別子の token value は source の slice、名前表は薄い `(ptr, len)`、hot な `== "literal"`
   は intern した id の比較、`JsString` は不変の薄い `(ptr, len)` + `Vec<u8>` の builder。
4. **Node 48 → 32 byte、Symbol 48 → 32 byte**（tsrs の header + tail 形、binder と transient の両方）：node 787 → 525 MiB。
5. resolved members の `properties` Vec を member 表の挿入順の filter view に（364 MiB の重複）、signature（243 byte → header +
   tail、147 MiB）。
6. inference context・mapper の使い回し（tsrs R16）、per-node の binder 表を疎に（1,720 万 node に dense で 148 MiB）。
7. symbol links を symbol record に inline（`PagedTable::get` 1.5 億回。tsrs は inline field）：transient symbol は checker の
   arena に、binder symbol は per-program の dense 表に。
8. `resolve_name` の `locals` は file ごとの `FxHashMap<NodeId, SymbolTable>`（`locals_of` が残る routing の最大の呼び出し元）。
   program 全体で一つの map にすれば routing は消える（VS Code で約 36 万 entry）。
9. JSDoc の hosted 表（`get_name_of_declaration` 18 か所など）：JS file の node だけが要るので、node の flag で先に分岐する。
10. parser の relocate（`relocate_with_leases` + `relocate_node_data` 97 sample）：parse 後に store の slot へ複製している。
11. 複製して返す API（`get_signatures_of_type`・`get_properties_of_type`、tsrs R18：−1〜−4%）、関係判定の error state の
    capture/reset ごとの clone（`capture_error_calculation_state`）。
