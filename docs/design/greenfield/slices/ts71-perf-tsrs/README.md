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

**本来の指標（multi-thread の wall-clock と peak、2026-11-04 18:38、lane 停止、warm-up + 3 round 中央値、両 binary とも既定の thread 数、
tsrs = `tsrs-sym-nopgo`）。** main `ed001ed92` 対 tsrs：VS Code wall 2.77 s 対 2.14 s（tsc-rs が **1.29 倍**遅い；CPU 18.05 対 8.39 s = 2.15 倍；
peak 4,382 対 2,311 MB = 1.90 倍）、Next.js 1.11 倍（CPU 1.97、peak 1.77）、zod 1.13（1.93、1.84）、Effect 1.09（1.99、1.75）、hono 1.22（2.2、1.93）、
vue 1.27（2.7、1.96）、playwright 1.21（2.1、1.75）、TypeScript compiler 1.65（1.9、1.52）。wall の差が命令数の差より小さいのは tsc-rs が VS Code で
≈ 6.5 core、tsrs が ≈ 3.9 core を使うため；tsrs の npm 配布 binary（PGO）はさらに ≈ 13% 速い。script：
`target/claude-tools/handover/perf-tsrs/wallclock.sh`。

**B0 の凍結と anchor（M0、2026-11-05 07:00、静音、nice 20、warm-up + 3 round 中央値）。** B0 = main `c3485397a`（#887 まで；#885 は queue の
3 周目で後から着地）、binary の sha256 と環境の receipt は `target/claude-tools/perf-plan-20261104/`（`manifest.json`、`runs/`）。A/A（同じ B0 binary を
2 つの label で）：命令数の差は 8 project すべてで ±0.03% 以内、peak ±4 MB — 雑音床。

| project（1 thread） | B0 命令数 | tsrs | 比 | B0 peak | tsrs peak | 比 |
|---|---:|---:|---:|---:|---:|---:|
| VS Code | 138.24 G | 85.48 G | 1.62 | 3,522 MB | 2,008 MB | 1.75 |
| TypeScript compiler | 9.72 | 4.89 | 1.99 | 172 | 110 | 1.56 |
| Next.js | 29.06 | 17.27 | 1.68 | 744 | 453 | 1.64 |
| zod | 19.14 | 13.20 | 1.45 | 716 | 440 | 1.63 |
| Effect | 15.97 | 10.98 | 1.45 | 521 | 328 | 1.59 |
| hono | 2.84 | 1.73 | 1.64 | 111 | 71 | 1.56 |
| vue | 15.11 | 8.67 | 1.74 | 301 | 182 | 1.65 |
| playwright | 13.08 | 7.54 | 1.73 | 404 | 249 | 1.62 |

**製品指標の前線（同じ窓、`frontier.sh`、VS Code；wall / CPU / 使用 core / peak / 命令数）。**

| 構成 | tsc-rs | tsrs |
|---|---|---|
| single thread（worker 1・checker 1 対 `--singleThreaded`） | 8.74 s / 8.77 s / 1.0 / 3,520 MB / 138.2 G | 5.65 s / 5.81 s / 1.0 / 2,005 MB / 85.5 G |
| 4 thread 予算（`TSRS_WORKERS=4 TSRS_CHECKERS=4` 対 `RAYON_NUM_THREADS=4 --checkers 4`） | 2.95 s / 10.46 s / 3.5 / 3,973 MB / 148.4 G | 1.96 s / 6.64 s / 3.4 / 2,306 MB / 92.3 G |
| 既定 | 2.22 s / 15.15 s / 6.8 / 4,313 MB / 155.0 G | 1.85 s / 7.40 s / 4.0 / 2,306 MB / 92.4 G |
| checker 数別（worker 1）：1 / 4 / 8 | 138.2 / 147.5 / 153.6 G（+6.7% / +11.1%）、peak 3,520 / 4,082 / 4,394 MB | 1 / 4：85.5 / 92.3 G（+8.0%）、2,005 / 2,306 MB |

読み：(1) **4 thread 予算での差 1.51 倍は 1 thread の差 1.55 倍と同じ**で、並列効率は両者とも同じ（85% 対 87%）。固定予算での負けは scheduler ではなく thread
あたりの命令数と CPI の差。(2) 既定どうしの差が 1.20 倍と小さいのは tsc-rs が 6.8 core（E core へ溢れ、CPU +73%）を使うからで、tsrs は 4 core に留めている。
(3) checker 間の重複は 4 checker で +6.7%、8 で +11.1%（tsrs は既定の 4 で +8.0%）で、tsrs と同程度。(4) 4 thread での直列・不均衡の wall 比率は ≈ 11%
（tsrs ≈ 15%）で、symbol/bind model の分岐の規則（< 10% 維持、> 20% 代替案）では**維持**。(5) checker あたりの memory は ≈ +125〜140 MB（tsrs ≈ +100）。
結論：並列の経路は命令数の差を迂回しない。4 thread で勝つには thread あたりの効率（record の形・確保・CPI）を詰める必要があり、SCHED-1 の利得は不均衡分
（wall の 1 割前後）と既定での配置（E core の CPU）に限られる。

**M1：段階別の命令数台帳（2026-11-05 10:07、静音、1 thread、nice 20）。** Codex lane の観測 patch（`proc_pid_rusage` の対 scope 19 種 + 集計 tool、
`handover/perf-tsrs/m1-lane-record/`）に統合者の修正（package.json/implied format を解決の行へ、coarse mode、footprint > 遅延更新 peak を警告に）を当てた
B0 の binary と、Opus が同じ sidecar 形式で instrument した tsrs（`m1-tsrs-ledger.patch`）を、case ごとに warm-up → coarse → fine（両 engine 交互、
`/usr/bin/time -l` 併走）で走らせた。coarse（呼び出し単位の scope を切る）の仕事合計は native の anchor と +0.15%（gate 0.2% の内）、fine は probe の
残差（校正 × 境界数）を行の横に示す。集計の差分は全 run で 0。run と表：`target/claude-tools/perf-plan-20261104/runs/m1/`。

| VS Code（coarse、exclusive、M 命令） | tsc-rs | tsrs | 差 | 比 |
|---|---:|---:|---:|---:|
| config/driver | 1,417 | 898 | +519 | 1.58 |
| load 合計（fs + parse + request 収集 + 解決） | 24,691 | 12,342 | +12,350 | 2.00 |
| publication/adopt（tsrs に対応物なし） | 4,120 | 0 | +4,120 | — |
| bind | 9,287 | 5,278 | +4,008 | 1.76 |
| checker init | 60 | 8 | +52 | 7.4 |
| semantic check | 97,640 | 66,831 | +30,809 | 1.46 |
| finalization（診断の整列・描画・emit・後始末） | 1,234 | 27 | +1,207 | 46 |
| 仕事合計（probe 除く） | 138,450 | 85,385 | +53,065 | 1.62 |

load の fine 内訳（VS Code）：fs/loader 4,177 対 2,992（1.40）、scan/parse 16,003 対 8,427（**1.90**）、request 収集 1,975 対 193（**10.2**）、解決 4,079 対
1,877（2.17）。**front end の超過（load + publication/adopt + bind）= 20.5 G = 差の 38.6%**、semantic = 58%、config + init + finalization = 3.4%。
next：仕事 29,105 対 17,242（1.69）、front 27%、semantic 1.60；hono：2,856 対 1,696（1.68）、front 54%、semantic 1.41。
判断（計画 §3 M2 の引き金：front end ≥ 12 G または差の ≥ 25%）：**引き金が引かれた**ので front end の stream（Opus）を checker の stream と同格にする。
目立つ行：request 収集 10 倍（FE2 の対象、VS Code で 1.8 G）、publication/adopt 4.1 G（tsrs にない仕事＝FE4 の価格）、finalization 1.2 G 対 0.03 G（要調査）、
parse 1.9 倍（7.6 G）、bind 1.76 倍（4.0 G）。

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
  8 project の source について出す。baseline は `target/claude-tools/handover/perf-tsrs/syntax-baseline/main-b8a54e83a.tsv`
  （143,190 行、panic 0。#883 の後、2026-11-05 に `main-39058c48c.tsv` から更新した。差分は #883 の 40 input・80 行だけで、#883 の
  run の差分と行単位で一致し、#887 からの差分はない）。front end を変える slice は `--compare` で差分 0 を示す（新しい local check
  の作法、§3.3）。
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

## checker と types の最初の slice 群：B-5 3a〜3c・C-0 step 1・R29 1〜3・M4 の小さな切り口（PR #868・#871・#872・#873）と PGO の workflow（#869）

Opus が A-1（front end）を書く間、Fable 5.1 の session が Codex（`gpt-6.1-sol`）の lane で checker と types 側の slice を並行して
進めた。lane は隔離した clone で実装し（`cargo -j 1`、`--test-threads=1`、一度に 1 つ）、Fable が主張を再実行してから（lane の
test と A/B の確認、A-1a part 2 の上への rebase、release build、`--checkers 4` の報告を main の新しい 4-checker 報告と比較、
`diffout.py` で 8 case の出力を main の binary と比較）PR にした。4 つの branch は同じ `serial.py` の run で main（`1a05993fb`、
label `p2main`）の binary と比べた（1 thread、通常 build、各 1 回、10:36〜10:38。lane の test が 1 本走っていたので絶対値は
静かな計測より +1.2%。差は同じ run の中の比）。

| case | main | C-0 step 1 | R29 1〜3 | M4 cuts | B-5 3a〜3c |
|---|---:|---:|---:|---:|---:|
| VS Code | 143.66 G / 3,618 MB | +0.3% / 3,618 | −0.6% / 3,618 | **−1.8%** / **3,588** | −0.4% / 3,615 |
| TypeScript compiler noEmit | 9.95 G / 176 MB | +1.5% | −0.6% | −1.1% / 173 | −0.5% |
| Next.js noEmit | 29.99 G / 776 MB | −0.9% | −1.4% | −2.6% / 755 | −1.8% |
| zod | 20.10 G / 737 MB | −1.7% | −1.9% | −2.7% / 726 | −2.1% |
| effect | 16.47 G / 531 MB | −0.2% | −0.8% | −1.6% / 526 | −0.7% |
| hono | 2.90 G / 113 MB | −1.4% | −2.0% | −2.5% / 112 | −2.0% |
| vue | 16.05 G / 319 MB | −0.2% | −1.9% | −1.6% / 305 | −0.7% |
| playwright | 13.83 G / 414 MB | −0.9% | −3.3% | −2.3% / 409 | −1.7% |

（peak footprint は変化のある cell だけ書いた。生 data は `serial-p2.jsonl`。）

- **C-0 step 1**（#868、branch `perf/ts71-c0-step1-p2`）：flow memo（tsrs R17、hook map
  `target/claude-tools/handover/perf-tsrs/flow-memo-hook-map-report.md`）の帳簿だけ。`crates/checker/src/flow_memo.rs` の frame
  register・serial・epoch と taint（hook 3〜11：shared cache の出自、loop の fixpoint、resolution target、signature の `resolving`、
  未解決 member、walk の位置、inline/reduce の上限）。memo の key・表・lookup・checkpoint は step 2（lane、build 時の
  `flow-memo-shadow` feature の裏で tsgo と同じ答えを shadow で検証）。unit test 13。予想は費用 <0.3%、実測は VS Code +0.3%
  （他 7 case は −1.7〜+1.5% で配置の揺れの中）：帯の端。step 2 の shadow 計測で memo の純益が出なければ step 3（既定 on）には
  進まない。
- **R29 steps 1〜3**（#871、branch `perf/ts71-r29-p2`、C2 監査 §3・§7.1）：union builder の入力を tsgo と同じに（traversal 順に
  append、重複は stable sort まで保持、隣接の同一性で除去。`addTypeToUnion`/`removeDuplicateTypes` の形）し、content の
  contains/insert を Go の lower-bound search（`crates/types/src/goslices.rs`。GST/GQ の移設先、最後の等価比較まで同じ）に。
  `perf-counters` feature の counter 32 本（collection の長さ分布、GST の比較回数、`compare_types` の outer/recursive、search の
  hit/miss）。test：全長さの hit/insert 順位、Go の probe trace 9 本、旧 pipeline との全 vector 比較、外部の Go 1.27.1 ↔ Rust の
  sort trace 180 本（比較 59,874 回）。予想は credit なし（fidelity の slice）、実測は VS Code −0.6%、他 −0.6〜−3.3%：上振れ。
  ID 順の二分探索と途中挿入（要素の移動）を append と 1 回の sort に替えた分と見る（counter で追える）。steps 4〜7 は計測で退行したので止めた（台帳の行、原因は symbol の順序 id の側表）。
- **M4 の小さな切り口**（#872、branch `perf/ts71-m4-cuts-p2`、instruction audit の rows 6・8〜12・15・16）：`FlowQuery` が
  flow arena の借用を file index と並べて 1 回で検証、`visit_nodes` と NodeArena の array view の入口を inline、`page_mut` の成長
  tail を `#[cold]` の `create_page` に、resolver の snapshot 複製・suggestion・診断を cold に、`advance_walk` の JSDoc 遷移を分離、
  `is_matching_reference` の This/Super/終端を worker の前で判定、inference の alias 腕を `#[inline(never)]` に、`RelationCache` を
  `Pair (u32, u32, i32)` の表と `Generic Box<str>` の表に（旧 mixed-key map との全 bit の回帰 test）。予想は P11 0〜0.81 G + P8
  0〜0.05 G（≤0.6%）、実測は VS Code −1.8%（141.09 G）と peak −30 MB、他 −1.1〜−2.7%（peak も next −21、zod −11、vue −14 MB）：
  3 倍の上振れ。memory の分は row 16（文字列 key と整数 key の表の分離）。
- **B-5 steps 3a〜3c**（#873、branch `perf/ts71-b5-steps-3abc-p2`、transaction 監査 §5.1）：overload 候補の transaction から、
  tsgo が候補をまたいで保持する 3 種の完了事実（type-only alias、node check flag、base type の cache）の journal・mark・restore を
  削除し、直接公開。3b は fidelity の修正を含む：overload 引数の class expression の static initializer が自分の class を参照する
  場合の emit が `_a.owner = _a`（tsgo と同じ。保持される `CONSTRUCTOR_REFERENCE` flag）になる（以前は `_a.owner = Named`）。
  conformance corpus は覆わないので unit test で固定（3 本追加）。16 か所の `or_node_check_flags` の呼び手は lane の記録で分類
  （once-state・emit state・scoped latch。tsgo はどれも巻き戻さない）。予想 ≈0、実測は VS Code −0.4%、他 −0.5〜−2.1%。
- **PGO / 命令数の workflow**（#869、branch `ci/pgo-workflow`、設計 `pgo-ci-design-report.md`）：`.github/workflows/pgo.yml`。
  PR の label `perf:instructions`（通常 build の命令数）/ `perf:pgo`（instrumented build → 学習 → PGO build）か手動起動で、
  hosted runner が main と PR の両側を測って artifact に報告する（build 300 分・measure 150 分の cap）。repository variable
  `PGO_CI_ENABLED` が `true` になるまで inert（利用者の決定 2026-11-04：runner の支出は要求時のみ）。action は SHA で固定。

**記録。** PR [#868](https://github.com/kazhiramatsu/tsc-rs/pull/868) → merge `a4663d27e`（2026-11-04。hosted run
[41748881477](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41748881477) 全 job pass。local（hosted がしないことだけ）：lane の
checker test 1,850 pass、release build、`--checkers 4` が main `1a05993fb` の新しい 4-checker 報告と 15,339 構成一致、8 case で出力が
main の binary と一致、serial は上の表）。

## A-1c C1：array が要素の modifier flags を持つ（branch `perf/ts71-modifier-flags`）

A-1c（戦略 v2 §4.1、`a1c-parser-inventory-report.md` §3）の最初の step。tsgo の `ModifierList` は作るときに
`ModifiersToFlags(nodes)` を一度だけ計算して持つ（internal/ast/ast.go:160-166）。同じことを node array でする。

1. **record**：`NodeArrayRecord` の空いていた 2 byte に `modifier_flags: u16`（20 byte のまま）。要素の modifier と decorator の
   flag（`ModifierFlags::SYNTACTIC_MODIFIERS` の bit 0〜15）の OR で、view（`NodeArray::modifier_flags`）は `ModifierFlags` に
   0 拡張して返す。array を作る唯一の口 `NodeArena::alloc_array`（parser・JSDoc parser・emitter の factory・API の decoder・
   synthetic file）が全 array について計算するので、あとで `modifiers` field に入る array はどれも正しい値を持つ（VS Code で
   3.1 M array・5.4 M 要素の kind 読み）。incremental parse の subtree copy は要素がまだ古い tree の id なので、古い array の値を
   写す。clone・relocation・publish は record ごと写す。`modifier_to_flag` は `tsc_syntax::utilities` に移した（tsgo
   `ast.ModifierToFlag`、binder は re-export）。
2. **読み手**：`node_util::modifiers_to_flags`（`get_syntactic_modifier_flags`・`get_effective_modifier_flags`・
   `get_combined_modifier_flags` の下）は要素を歩かず array の値を読む。debug build では歩いた値と比べる assert を置いたので、
   宣言の modifier flags を問う全 test が保存値を検査する。checker の単一 modifier の判定（`has_static_modifier`、abstract の
   class と constructor type、`has_readonly_modifier`、static scope、decorator の有無）、parser の `modifiers_contain` と外部
   module 判定、recovery と module request の `declare` 判定、hover・snippets・JavaScript の宣言 emit の transform node 用
   helper も値を読む。modifier の node そのものが要る箇所（grammar の検査、decorator の式、印字、範囲）はそのまま。
   API decoder の kind 替え（`reshape`）は debug build で modifier flag が変わらないことを assert する。

**計測**（1 thread、通常 build 同士、各 1 回、静かな machine。part 2 = main `8e587f401` のコードの `q52f`、C1 = `9bb9e1b10`（`8ae334593` + main の docs））：
命令数 VS Code 144.58 → 143.54 G（−0.7%）、TypeScript compiler −0.3%、Next.js −1.5%、zod −2.2%、effect −0.7%、hono −2.1%、vue −0.6%、
playwright −1.7%。peak footprint は変わらない（VS Code 3,611 → 3,620 MB、他は同じ）。CPU 時間も減（VS Code 9.74 → 9.41 s）。絶対値は #867 の
静かな計測より約 1.8% 高い（止めた process が memory を持ったまま）。比は同じ session の値。array record は 20 byte のまま（VS Code 59.5 MiB）。
**予想との差**：見込みは n5 の modifier 群（157 対 tsrs 14 ms、≈2.0 G 相当）だったが、実測は VS Code −1.04 G。原因：157 ms は part 2 より
前の n5 の値で、part 2 の slot 読み（`modifiers_field()`）が `modifiers_of_data` の分（38 ms）を既に除いていた。C1 が除けたのは要素の walk だけ。
c1a の profile（`perf11/prof-c1/vs-c1a.json.gz`、9,153 ms）には残りがある：checker の `get_declaration_modifier_flags_from_symbol_write`
66 ms、`modifiers_of::<Published>` 51、`combined_modifier_flags` 28、binder の `get_combined_modifier_flags` 22（いずれも包含）。同じ値宣言の
combined flags を繰り返し計算する分で、A-1c の完了後に別の見込みを立てて扱う。

**記録。** PR [#870](https://github.com/kazhiramatsu/tsc-rs/pull/870) → merge `5ed9e073c`（2026-11-04。`d7d9e4eed`・`8ae334593`、main の
取り込み 2 回（#867 の記録、#868））。hosted run [41756034323](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41756034323)（`7c9acc395`）：
全 job pass。local（hosted がしないことだけ、`9bb9e1b10`）：変えた・通る 7 crate の test（syntax 268・binder 80・api 332・program 684・emitter 352・
ls 256・checker 1,837、0 failed）、fmt・clippy・4 つの生成器の `--check`、fingerprint 143,190 行 差分 0、8 case で出力が main と一致、
`--checkers 4` は既知の 4 件（15,335 一致）、serial は上の通り。

## A-1c C7+C8：parent を finish 時に付ける（branch `perf/ts71-parents-at-finish`）

A-1c（戦略 v2 §4.1、`a1c-parser-inventory-report.md` §2）の parent の step。tsgo の `finishNode` と同じく、node を作り終えた時点で
その子に parent を付け、完成した tree を歩き直す pass（`finalize_tree` の parent walk）をやめた。

1. **finish 時**：`Parser::finish_node_at` の最後で `NodeArena::attach_children`（`LastToken` より上の kind だけ。token・identifier・
   literal には子が無く、`for_each_child` にもその arm が無い）。子の parent は上書きする（parse が node を動かしたとき、tsgo と同じ）。
   finish を通らずに node を作る・変える箇所は自分で付ける：`T?` を包み直す optional の tuple 要素、finish 後に modifier を受け取る
   `MissingDeclaration` 2 か所、host に JSDoc を付けたとき（`attach_js_doc`、tsgo の `parsed.Parent`）、incremental parse が古い tree
   から写した node（subtree copier そのものは parent なしの契約のまま）。`finalize_tree` は root の子と error の集約（印の付いた node から
   上へ）だけになった。debug build は従来の 2 段の reference walk を copy に走らせ、到達する全 node の parent と error 集約を比べる。
2. **到達性**：失敗した先読みの node も finish されるので、その中で parent を持つ。arena 全体を走査して `parent.is_some()` を「tree の中」
   の意味に使っていた 3 か所（module specifier 生成の module name literal、宣言 emit の可視性、ES module 変換の import-equals /
   `export =` の入れ子判定）は `NodeArena::reachable_mask(root)` を聞く。他の parent の読み手は tree の node だけを読む。

**計測**（1 thread、通常 build、静かな machine、同じ run。C1 = `c1a`、最初の版 = `ccb9fb179`（全 finish で付ける）、本 PR = `2991eed8c`
（token を飛ばす））：VS Code（3 回の中央値）142.36 → 141.98 G（−0.3%、最初の版は 142.51）。他の 7 project は 2 回目の静かな窓（16:45）で
3 回を交互に測った中央値で TypeScript compiler −0.05%、Next.js −0.21%、zod −0.28%、effect −0.26%、hono −0.33%、vue −0.18%、playwright −0.26%
（最初に PR に書いた 1 回測定の −0.6〜−2.7% は、base を先に走らせたため base が各 project の cold start を負った分の水増し。serial.py は
warm-up と 3 回の中央値に直した）。peak footprint は変わらない。
**予想との差**：予想は VS Code −0.6〜−0.9 G（C1 の profile で walk が 77 ms ≈ 1.2 G、finish で付ける分が一部戻る）。最初の版は +0.1%：
VS Code の 17.2 M node のうち約 10 M が子の無い token で、その全部に呼び出し・scratch・`for_each_child` の dispatch を払い、失敗した先読みの
node にも付けていた（旧 walk は tree だけを歩いた）。token を飛ばして −0.38 G だが、VS Code では予想を下回る（他の 7 project は 0.05〜0.33%
減）：composite node ごとの付与は walk と同じ子ごとの仕事（dispatch、array の子の訪問、境界検査付きの書き込み）をするので、消えたのは
token の分と walk の stack だけ（`perf11/prof-c8/vs-c8b.json.gz`）。kind ごとの子の slot を生成して parent を直接書く案（`c8c`）は
VS Code +0.98 G の悪化で中止した（生成した visitor も payload を全部 decode する）。

**記録。** PR [#874](https://github.com/kazhiramatsu/tsc-rs/pull/874) → merge `4199f017e`（2026-11-04。code `ccb9fb179`・`2991eed8c`、最終 head `30ae5b558`）。
hosted run [41810437491](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41810437491)：全 job pass。local（hosted がしないことだけ、`c3bd12f8f`）：test
（`ccb9fb179` で syntax 269・binder 80・api 332・program 684・emitter 352・ls 256・checker 1,837、`2991eed8c` で syntax 269、0 failed）、
fmt・clippy・4 つの生成器の `--check`、fingerprint 143,190 行 差分 0（両方の byte）、8 case で出力が C1 と一致、`--checkers 4` は既知の 4 件
（15,335 一致）、計測は上の通り。

## A-1d D1：identity を差分で動かし、node record を写しながら store に置く（branch `perf/ts71-relocation-deltas`）

A-1d（戦略 v2 §4.1、`a1d-relocation-design-report.md` §2.1・§2.5）の最初の step。loader が parse した tree を program 順の lease に
移す relocation を軽くした。

1. **差分**：`identity_relocation` が lease ごとに space の数を確かめているので、identity は space ごとの始点の差だけ動く。
   `RelocationShift` が 3 つの差を持ち、各 word は `new + (word − old)`（wrapping 演算）、元の範囲に入っているかの検査は flag を 1 本
   落とすだけ。node と item を移し終えてから flag を見て、外れていれば従来と同じ `InvalidLease` を返す。それまでは word ごとに
   space の `match`・checked な引き算と足し算・失敗時の error 構築を `?` で返していた。`Node::map_identity_words` は失敗しない mapper で
   呼ぶので `?` は消える。
2. **写しながら置く**：`StoreVec::publish_mapped`（store、`unsafe` はここだけ）が record を 1 つずつ読み、`map` を通して store の slot に
   書く。node record は以前、buffer の中で書き換えてから store へ memcpy していた（各 record を 2 回読み書き）。結果は commit する
   まで公開されず、範囲外の identity があれば `PendingPublish` が書いた slot を scrub して tree はそのまま（従来どおり error のあと tree は
   使われない）。array と item は量が小さいので従来の書き換え + publish。

**計測**（1 thread、通常 build、静かな machine。base = main `cf50870c1`（`m870`）、本 PR = `9f760804d`（`d1b`））：VS Code（3 回の
中央値）144.43 → 144.18 G（−0.2%）、peak footprint 3,617 → 3,623 MB、memory report は同一（node record 533.4 MiB、syntax 641.4、binder
458.2、checker 1,519.8 MiB）。他の 7 project は 2 回目の静かな窓（16:45）で 3 回を交互に測った中央値で TypeScript compiler −0.10%、
Next.js −0.05%、zod −0.08%、effect −0.18%、hono −0.29%、vue −0.01%、playwright −0.12%（最初に PR に書いた 1 回測定の −0.7〜−2.4% は、
base を先に走らせたため base が各 project の cold start を負った分の水増し。serial.py は warm-up と 3 回の中央値に直した）。
**予想との差**：予想は VS Code −1.0〜−1.3 G。C1 の profile の relocation の時間（self 91 ms + `memmove` 42 ms）を program 平均の命令率で
命令数に換算したが、この仕事は memory を流す。word ごとの写像の単純化で消えるのは約 30 M の identity word × 約 10 命令（≈ 0.3 G、
VS Code で見えた分）で、融合した publish が消す 533 MiB の copy の費用は memory の転送であって命令ではない（vectorize された copy は
≈ 0.02 G）。以後、memory を流す仕事の予想は CPU 時間か word あたりの命令数で出す（計測台帳の規則）。

**記録。** PR [#875](https://github.com/kazhiramatsu/tsc-rs/pull/875) → merge `5c1ed6230`（2026-11-04。code `9f760804d`、最終 head `fcdc0d171`）。
hosted run [41814981400](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41814981400)：全 job pass。local（hosted がしないことだけ、`9f760804d`）：
relocation が届く 8 crate の test（store 17・syntax 269・binder 80・api 332・program 684・emitter 352・ls 256・checker 1,850、0 failed）、
fmt・clippy・4 つの生成器の `--check`、fingerprint 143,190 行 差分 0、8 case で出力が main と一致（checker は変えていないので
`--checkers 4` はなし）、計測は上の通り。

## A-1c C2+C3：parser の list を 1 本の element stack で作る（branch `perf/ts71-list-stack`）

A-1c（戦略 v2 §4.1、parser inventory 報告 §1.2〜§1.5・§5.2 の C2・C3）。parser は list ごとに一時の `Vec<NodeId>` に要素を集めてから
arena の item store に写していた（VS Code で 3.1 M array・5.4 M item、一時領域の確保に約 31 ms。tsrs の stack は約 1 ms）。tsrs と同じく
parser が `node_stack` を 1 本持ち、list は stack の長さを mark に取り（`start_node_list`）、完成した要素を積み、`stack[mark..]` から
従来の `alloc_array` で array を作って（item store への copy は 1 回）mark まで戻す（`finish_node_list` / `truncate_node_list`）。入れ子の
list は外側の要素の上の suffix を使い、戻る前に解放する。C2 は `parse_list` と `parse_delimited_list_worker`（先読みの parameter 解析が
失敗したら `parsing_context` を戻してから mark まで切る）、C3 は残りの producer（modifier と decorator、2 種の template span、union /
intersection、JSX の子（並べ替えた子は新しい frame で作り item store は縮めない）、JSON の回復 list、top-level await の置き換え文、host の
JSDoc root、JSDoc の全 list）。JSDoc の root の comment part と root tag は重なる（part を持ったまま tag を足し、comment の array が先に
終わる）ので、root tag だけは parser が持つ再利用の scratch `Vec` に置く。debug build は list が順に閉じて mark に戻ることを検査する。
出力（array・確保順・範囲・trailing comma・missing list・modifier flags・回復・診断）は変わらない。`crates/syntax/examples/parse_bench.rs`
（parse だけの corpus benchmark）を足した。実装は Codex の lane（隔離した clone、SPEC は `codex/SPEC-list-stack.md`）で、4 つの patch を
そのまま commit にし、lane の計測用 cargo feature（`parser-list-metrics`）は最後の commit で外した（数値は PR に記録）。

**計測**（1 thread、通常 build、静かな machine（17:27〜17:29）、`serial.py` の warm-up 1 回 + 3 回を交互に測った中央値。base = main の
code `ed001ed92`（`m5main`）、本 PR = `5088a7078`（`ls1`））：VS Code 141.93 → 141.53 G（−0.28%）、TypeScript compiler −0.25%、Next.js
−0.21%、zod −0.32%、effect −0.26%、hono −0.45%、vue −0.13%、playwright −0.81%、peak footprint は ±4 MB。lane の parse_bench（VS Code
の source 9,482 file、3 回の中央値）は 882.6 → 833.8 ms（−5.5%）、stack の最大長 11,159、同時に開いた list は最大 23。
**予想との差**：予想（計測前）は VS Code −0.4〜−0.8 G、footprint ±0、他は parse の比率で −0.3〜−1%。VS Code は −0.40 G で下端、他は
下半分。−49 ms を parse の平均命令率で換算すると少し多めに出る（一時領域の費用の一部は memory の待ちと重なる allocator の時間）。lane の
負荷の下で測った最初の run は VS Code −0.54% と出たが、その負荷では 1 回ごとに ±0.6 G ばらつくので記録は静かな run だけ。

**記録。** PR [#879](https://github.com/kazhiramatsu/tsc-rs/pull/879) → merge `26afd2d30`（2026-11-04。lane の 4 commit `cd536e7e1`〜`cb4b176bd` と
`5088a7078`、最終 head `ff4fda9aa`）。hosted run [41844366290](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41844366290)：全 job pass。local（hosted が
しないことだけ、`5088a7078`）：syntax の test 271、fmt・clippy・4 つの生成器の `--check`、fingerprint 143,190 行 差分 0、8 case で出力が main と
一致（checker は変えていないので `--checkers 4` はなし）、計測は上の通り。

## tsgo 7.1 の async arrow の先読みと Identifiers の数え方、先読みの巻き戻しと comment directive（branch `perf/ts71-async-lookahead`）

`--extendedDiagnostics` の `Identifiers` を tsgo（`aa814927`）と file ごとに比べて見つけた、7.1 の parser の振る舞いの 2 つの取り残し。
どちらも tree は変えない。

1. **async arrow の先読み**：tsgo の `nextIsUnParenthesizedAsyncArrowFunction`（parser.go:4594）は token だけを見る（`async`・
   識別子・`=>` が同じ行）。6.0.3 の先読みは `async` の後ろの二項式を丸ごと parse していたので、`async function …` 式の本体を毎回
   2 度 parse していた（hono の `method-override/index.ts` は識別子を 441 個作って tree に残るのは 241、tsgo は 254）。同じ commit で
   tsgo の `parseElementAccessExpressionRest`（parser.go:5527）が引数の前に作って捨てる missing identifier を数に入れる（node は
   作らない）。8 project のうち 7 で `Identifiers` が tsgo と一致し、effect だけ 293 少ない（7.1 の top-level await の印
   `statementHasAwaitIdentifier`・`possibleAwaitSpans` が未移植。M2 の監査に入れた）。
2. **巻き戻した先読みの comment directive**：tsgo の `ScannerState` は comment directive を持つので、巻き戻した先読みが読んだ
   `// @ts-ignore` / `// @ts-expect-error` は残らない。`ScannerState` に directive の数を持たせ、restore で切る。以前は失敗した
   括弧付き arrow の試行などで 1 つの directive が 2 回並んでいた（fingerprint の 40 input・41 個。tree は同一）。

**計測**：1 thread、通常 build、静かな machine（M0 の窓、06:58〜）。`serial.py` の warm-up 1 回 + 交互 3 回の中央値。main の code
（`m5main`）対この branch（`al2`）。生の行は `asynclookahead/serial.jsonl`。

| project | 命令数 | 備考 |
|---|---:|---|
| VS Code | **−0.10%**（141.99 → 141.84 G、−0.15 G） | 3 回とも減る（−0.06〜−0.15%）。peak footprint 3,587 → 3,560 MB（−27） |
| hono | −0.16%（2.84 G） | 3 回とも −0.16% |
| 他の 6 project | +0.00〜+0.13% | noise の幅（playwright の +0.13% は 1 回だけの +0.17% による） |

VS Code 以外の peak footprint は ±4 MB 以内。exit code と出力の大きさは 8 case とも同じ。

**予想との差**：予想は VS Code −0.2〜−0.5 G（−0.15〜−0.35%）で、0.5〜0.8 M の node が 2 度 parse されなくなるという見積もり
だった。実測の −0.15 G は帯の下に外れた。予想と同じ node あたりの命令数（0.4〜0.6 K）なら 0.24〜0.37 M node 分にあたる。VS Code の
Identifiers は 77,967 減った（5,945,718 → 5,867,751、tsgo と同じ）。2 度目の parse は見積もりより小さかった。hono が他より大きく
減るという予想は当たった。この slice の価値は fidelity にある（Identifiers が 7 project で tsgo と一致し、directive の重複が消える）。
計測では命令数の増加はなく、VS Code はわずかに減る。

**記録。** PR [#883](https://github.com/kazhiramatsu/tsc-rs/pull/883) → merge `55f65b0b6`（2026-11-05。`80849312f`・`cecdb2fa8`、最終 head `5823e1f1c`）。
hosted run [41932861516](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41932861516)：全 job pass。local（hosted がしないことだけ、`cecdb2fa8`）：
syntax の test 270、fmt・clippy・4 つの生成器の `--check`、fingerprint は 40 input・80 行が差分で全部が directive の重複の削除（tree の
dump は同一）、8 case で出力が main と一致、`Identifiers` は 7 project で tsgo と一致、計測は上の通り。

## checker の data 層の書き直し（利用者の決定 2026-11-04）

計測台帳の結果（front end と memo では VS Code の差が縮まらず、差は checker の 1 操作の費用と record の形にある）を受けて、利用者が
checker の data 層の書き直しを決めた。範囲・record ごとの契約・順序・予想・gate は
[checker-data-layer-plan.md](checker-data-layer-plan.md)。C-0 / C-1 の memo は停止（C-0 step 2 は branch `perf/ts71-c0-step2`
のまま、PR なし）。

**見込みの相談と計画（2026-11-04 夜）。** 本来の指標（上の「差」の節の wall-clock）と命令数の見通し（計画の phase 1〜2 で ≈ 1.4〜1.5 倍、
peak 1.3 倍）を受けて、利用者の指示で Opus と Codex（effort max）に「率直にどう変えるか」を相談した：
`target/claude-tools/handover/perf-tsrs/consult-opus-2026-11-04.md`（段階別 load+parse 2.2 倍・bind 1.6 倍・check 1.3 倍、対応する 67 関数の中央値
1.22 倍、確保回数 6.4 倍、tsrs は型が 8.1% 少ない、8 checker で CPU 2 倍（E-core）、未計上 692 MiB、天井 build の提案）と
`consult-codex-2026-11-04.md`（parity は hot core の大きな変更の後にしか plausible でない；memory の計画としては良いが命令数の予想は過大；
S8 を先に、S4 は後回し、C-1 は bounded な qualification、まず段階別の命令数台帳）。利用者の求めで Codex が計画書
`PLAN-codex-2026-11-04.md` を書いた：bounded milestone 11/18 18:00 JST に ≤ 132 G / ≤ 3,100 MiB（最低 gate < 135 G）、計測 program M0〜M8
（B0 の凍結と ABBA の harness、段階別の native 命令数台帳、front end の仕事量監査、天井の感度、確保の census、C-1 の bounded な
qualification、checker/worker の frontier、692 MiB、対応 hot path）、card 順 W0 → I0 → L1/L2（S8）→ Y0/Y1/Y2（S1/S2）→ R5/R6 → N3 → F10 →
Q7 → H1、writer は最大 2。Opus の訂正 3 点（F10 = S10 の option b、Y0/L3 = S1a-2 の client、M2 = tsrs の pin 3e9236361 で比べる）は
`PLAN-codex-2026-11-04-addendum.md`。利用者の決定待ち：目標の再設定の時期（今か 11/18 の gate か）、writer を 2 本に戻すか（S3 と S1b-1 の
lane は patch を保存して中断）、FE4（identity の rewrite）の方針、merge queue。

**利用者の決定（2026-11-04 20:20、「OKです」）。** 計画を採用：writer は Codex lane 1 本 + Opus（統合は Fable）、S3 lane は手渡し（patch 01〜04）で終了、
S1b-1 lane は中断して working tree を patch に保存（`target/claude-tools/handover/perf-tsrs/s1b-1-parked/`、Y2 の入力）、M0〜M8 は 11/5 から。
parity は 11/18 の gate（判断 packet 15:00、決定 18:00 JST；≤ 132 G / ≤ 3,100 MiB、最低 < 135 G）まで研究目標として残す。merge queue か
2 card の train を採用（GitHub の merge queue は ruleset の作成が要り、利用者の操作；それまでは 2 card の train）。FE4 は引き続き利用者の明示の
方針決定が要る（M2/M8 で残る pass の価格を付けてから提示）。Opus の M1 の seam の review は `m1-mapping-review-opus.md`：parse は request
planning の worker の中で走る（scope は worker 内の parse 呼び出しに）、tsrs は「parallel parse + resolve」1 行 + job ごとの sub-row、bind は
走った場所から取る。

**製品指標の決定（2026-11-04 夜）。** 利用者：「1 thread で届かなくても 4 thread で勝つ pattern もアリ」。合意した指標：8 project の wall-clock と
peak footprint を **両者 4 thread の予算**と**それぞれの既定**で（warm-up + 3 round 中央値、tsrs は normal と npm PGO の両方を明記）、11/18 の
決定表の最上段に置く。1 thread の命令数台帳は雑音床が最も低いので機構の台帳として残す（固定 thread 予算では命令数の削減がそのまま wall に
効く）。根拠：既定どうしで VS Code は wall 2.77 対 2.14 s（1.29 倍）に対し命令数は 1.6 倍。tsc-rs は 1 thread から 4.8 倍伸びる（tsrs 2.8 倍）が、
それは P core 4 + E core 6 のうち E core へ溢れて稼いでいる（CPU 18.05 対 8.39 s）。並列で勝つための構造の変更は record の形ではなく所有・寿命・
scheduling の層（直列区間 = loader の finish/publish・identity の relocation・binder の consume・診断の順序；checker ごとの lib/共有 module
の型の重複；P core/QoS の配置）で、record の card は計画どおり。唯一の大きな分岐 = symbol/bind の model（tsc-rs：並列 bind → 全体 id 空間へ
relocation、id 再現可能；tsgo/tsrs：file ごとの symbol、checker ごとに global merge）は 11/11 の checkpoint で 4 thread の直列区間の wall 比率で
決める（< 10% で維持、> 20% で代替案に価格を付ける）。M6（前線）は D1 に前倒し：`handover/perf-tsrs/frontier.sh`（既定・4 thread・1 worker で
checker 1/4/8 = 重複を命令数で、tsrs 既定・`--singleThreaded`・4 thread の knob）。WASM は単 thread なので 1 thread の効率は製品価値のまま。
利用者の追加（21:25）：**WASM 用に single thread の行**（worker 1・checker 1 対 tsrs `--singleThreaded`、wall と peak）も製品指標に入れる。
WASM は溢れる core がなく wasm32 の linear memory が peak を縛る（VS Code の 1 thread peak は今 ≈ 3.6 GB 対 tsrs ≈ 2 GB）ので、1 thread の wall と
peak は機構の台帳ではなく製品値。packet の表は project ごとに single・4 thread・既定の 3 行。tsrs 側の knob は patch 不要（Opus の確認：
`RAYON_NUM_THREADS=N` が worker pool、`--checkers N` が checker 数、既定は pool 10・checker 4）。

**scheduler の決定（2026-11-04 23:35）。** 利用者：「rayon 級の scheduler を自前で書く方針でいいです。Mac 寄りに特化した内容を入れるのは賛成です。
そもそも tsc は Mac 環境で動くのが多いでしょうし」。経緯：tsrs の並列は parse+resolve が rayon の pool（`RAYON_NUM_THREADS`）、checker は自前 thread +
親和性つきの work stealing（sticky thieves）、tsc-rs は std の thread と自前の queue で stealing も親和性もない。tokio は async I/O の runtime で CPU-bound
な compile には向かず、rayon は配置（P/E core）・idle 方針・親和性・単 thread/WASM の面で足りない。方針：自前の work-stealing scheduler（card **SCHED-1**、
要件と段階は `target/claude-tools/handover/perf-tsrs/SCHED-1-brief.md`）：fork-join の scope/join と動的 DAG 用の queue、thread ごとの deque + injector、
親和性優先の steal、P core 数（`hw.perflevel0.physicalcpu`）を既定にした配置と QoS、bounded spin → park の idle 方針、予算 1 では inline 実行（WASM の
mode）、実行順に依らない決定的な merge、worker ごとの busy/idle/steal の counter。deque の primitive は crossbeam-deque を借り policy 層を自前で書く
（deque まで自前なら loom の検証を工程に入れる）。段階：loader（動的 DAG）→ checker の file 配分（親和性で重複削減）→ 配置と idle → 直列区間は M6 の
比率次第。予想式は phase ごとの (max − mean busy) − steal の overhead + 重複の削減で、M6（D1）の数字が順序と予想を決める。設計書は M6 の後（D2）に
Codex（軽作業）、Opus と Fable が review、実装は Codex lane 2〜3 日、各段階を B0 上で計測。

PR [#871](https://github.com/kazhiramatsu/tsc-rs/pull/871) → merge `43a78fc08`（2026-11-04。hosted run
[41762963850](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41762963850) 全 job pass。local：lane の types test 57・checker 1,837、
`--checkers 4` が main の報告と 15,339 構成一致、8 case で出力一致、serial は上の表）。
PR [#873](https://github.com/kazhiramatsu/tsc-rs/pull/873) → merge `fcdd24e5b`（2026-11-04。hosted run
[41769776478](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41769776478) 全 job pass。local：lane の checker test 1,839、3 段の A/B（3b の emit の修正を含む）、
`--checkers 4` が main の報告と 15,339 構成一致、8 case で出力一致、serial は上の表）。

PR [#872](https://github.com/kazhiramatsu/tsc-rs/pull/872) → merge `15d095c97`（2026-11-04。hosted run
[41777167903](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41777167903) 全 job pass。#868 の flow.rs と衝突したので local で解決
（両方の追加を残す。`cargo check` と flow の test 9 件で確認）。local：lane の checker test 1,838・syntax 260、`--checkers 4` が main の
報告と 15,339 構成一致、8 case で出力一致、serial は上の表）。

PR [#869](https://github.com/kazhiramatsu/tsc-rs/pull/869) → merge `49467d9de`（2026-11-04。PGO / 命令数の workflow。merge 後に
repository variable `PGO_CI_ENABLED=true` を設定、label `perf:instructions` / `perf:pgo` を作成、#880 に label を付けて smoke run を要求）。smoke run（workflow_dispatch、pr 880、build_kind normal、cachegrind false）= run
[41806174056](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41806174056) 成功、runner ≈ 20 分（request 5 s、corpus 10 s、base/head の build
≈ 9.5 分ずつ、計測 hono 22 s + typescript 34 s、report 7 s）。hosted runner に hardware counter はなく（"I refs unavailable"）、命令数は
`cachegrind=true` が要る；label を付けても run は作られなかった（workflow_dispatch は動く）。

## S1a step 1：symbol の export link を extras へ（Symbol 48 → 40 B、branch `perf/ts71-symbol-record`）

checker の data 層の書き直し（[checker-data-layer-plan.md](checker-data-layer-plan.md)、S1）の binder 側の最初の step。`Symbol` の
`export_symbol: Option<SymbolId>`（tag 込み 8 B）を、member / export の table がすでに住む extras の box に移した。VS Code では binder の
symbol 2,270,113 のうち設定するのは 48,811（2.2%、`declareModuleMember` が export に結ぶ local symbol）、checker が作る 5,890,486 は
どれも設定しない。`Symbol::export_symbol()` が box を通して読み（extras がなければ `None`）、唯一の writer（`containers.rs`）は
`set_export_symbol` で extras を初回に確保する。relocation は extras の中の link を写す。reader 24 か所は accessor を呼ぶ。symbol の
id・順序・table は変わらない（id 順の audit は S1b のもの）。binder と checker の symbol は同じ record なので 8 B は両方に効く。

**計測**（1 thread、通常 build、静かな machine（16:42〜16:49）。base = main の code `5093f0204`（`m873`）、本 PR = `39317816d`
（`s1a1`）。各 case 3 回を交互に測った中央値）：VS Code 144.01 → 143.83 G（−0.13%）、peak footprint 3,620 → 3,553 MB（−67 MB）、
CPU 9.62 → 9.38 s。memory report は binder の symbol 103.9 → 86.6 MiB、binder 計 458.2 → 445.8 MiB（export link を持つ 2.2% は extras の
box を新たに持つ）、serial checker 計 1,517.7 → 1,472.8 MiB（checker の symbol 5,890,486 × 8 B）、syntax は不変。他の 7 project は
命令 −0.0〜−0.35%、footprint −2〜−21 MB。**予想との差**：予想（計測前）は VS Code の live memory −59〜−65 MB、命令 0 ± 0.2%。
計上で −57.3 MiB（−60.1 MB）、peak footprint で −67 MB、命令は予想どおり。この計測で serial.py の偏りが見つかった（1 回測定で base を
先に走らせると base が cold start を負う。warm-up と 3 回の中央値に直した）。

**記録。** PR [#878](https://github.com/kazhiramatsu/tsc-rs/pull/878) → merge `41e609234`（2026-11-04。code `71049d3e5`、最終 head `b3ff25b8d`）。
hosted run [41836928888](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41836928888)：全 job pass。local（hosted がしないことだけ）：test（`71049d3e5`
で binder 80・api 332・program 684・emitter 352・ls 256・checker 1,850、0 failed）、`39317816d` で fmt・clippy・4 つの生成器の `--check`、
fingerprint 143,190 行 差分 0、8 case で出力と統計行（Files〜Instantiations）が main と一致、`--checkers 4` は既知の 4 件（15,335 一致）で
C8 の 4-checker 報告とは 15,339 すべて一致、計測は上の通り。

## M2 の fidelity 修正 2：型引数の list が空の alias は non-generic（branch `perf/ts71-nongeneric-empty-alias`、tsgo isNonGenericTopLevelType）

M2 の item 1b。#885 の後も vue の instantiation は tsgo より 19,725 多かった。両方の engine で数え方を比べて原因を特定した（`m2/vue-instantiations.md`
の「Item 1b」）。

**比べ方**
- 最上位の instantiation の入口は一致していた（123,204 対 123,611）。差は展開の中にある。
- 各 property の展開を比べると、差は二つの property に集中していた：`__asyncResolved?: ConcreteComponent`（tsgo は 1 回の使用につき 1、
  tsc-rs は 8）と `components?: LC & Record<string, Component>`。

**原因**：vue は type parameter がすべて default を持つ generic alias（`ConcreteComponent`・`Component`）を import し、型引数なしで書く。
- tsgo は alias を `{symbol, []}` にする（getTypeArgumentsFromNode）。isNonGenericTopLevelType は `len(alias.typeArguments) == 0` なので
  non-generic と判定し、instantiate しない。
- tsc-rs も alias を `Some(vec![])` で作るが、`is_non_generic_top_level_type` は `None` だけを見ていた。そのため毎回 union とその member を
  instantiate していた。

修正は 1 行で、tsgo と同じく空かどうかを見る。

**統計行**（親 main `22c755af5` 比、1 thread）
- vue −32,627。#885 の上では 569,481 → 548,963（tsgo 549,756）。getTypeOfInstantiatedSymbol・getInferredType・createMarkerType などが
  tsgo と完全に一致する。
- zod −459・next −42・hono −7。変わった property はすべて tsgo と一致するか、tsgo に近づく。
- 他は 0。出力は 8 project とも一致。

**退けた案**：型引数を書かない場合に list を持たない表現にすると、vue は同じく一致する。しかし playwright で tsgo が行う 5,856 の
instantiation が消える。原因は型の順序付け（`type_order.rs:497`）で、tsgo の compareTypeNames は list を常に比べるのに、tsc-rs は
最初の型が list を持つときだけ比べる。None と空を区別する読み手の監査は別の項目にした。

**計測**：1 thread、通常 build、静かな machine（02:30〜02:32）。`serial.py` の warm-up 1 回 + 3 回の中央値。

| project | 命令数 | 備考 |
|---|---:|---|
| vue | **−0.65%**（15.17 → 15.07 G） | 3 回とも同じ |
| 他の 7 project | −0.02〜+0.03% | |

peak footprint はすべて ±1 MB 以内。

**予想との差**：予想は vue −0.2〜−0.4%（N = 32,627 × c = 1〜2 K 命令）だった。実測は良い側に外れ、1 個あたり約 3 K になった。展開を
省くと、型と symbol を作る仕事も消える（vue の Types −906、Symbols −2,755）。この仕事は instantiation の数には入っていなかった。

**記録。** PR [#886](https://github.com/kazhiramatsu/tsc-rs/pull/886) → merge `75019a7e2`（2026-11-05。`970d0c301`、最終 head `69dd90377`）。hosted run
[41900857035](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41900857035)：全 job pass。local（hosted がしないことだけ、`970d0c301`）：
- tsgo に合わせた unit test（修正なしでは 11 対 9 で失敗）
- checker の test 1,863 + 1、fmt、release build
- 8 case で出力（exit code・stdout・stderr）が main と一致
- `--checkers 4` は既知の 4 件のみ。最新の 4-checker 報告とは 15,339 構成すべて一致
- 計測は上の通り

## syntax の relocation test を独自の process で走らせる（branch `test/ts71-relocation-own-process`、#885 の hosted run で出た flake）

#885 の最初の hosted rust job（run 41863955264）が `tsc-rs-syntax --lib` の `generated_relocation_matches_a_direct_forced_nonzero_parse`
で失敗した。#885 の変更（checker のみ）とは関係がなく、この test は隣で別の test が走るときだけ失敗する。

**原因**
- identity は process 全体で共有される。`IdentityDomain::new` はすべて一つの状態から割り当てる。
- c3a8e2ded（11-02）で、この test は node・array・item について木の置き場所に依存しなくなった。
- #866（A-1a step 2）が NodeText の空間を加えた。`NodeArena` の等価性は texts の base と node の text identity を絶対値で比べ、直接
  parse した木の texts は常に 0 から始まる。隣の test が NodeText 空間の先頭を持っていると、relocate した木の texts が後ろにずれ、比較が
  失敗する。
- hosted の log は、同時に走った `detached_arena_preserves_ids_without_extending_published_leases`（`const value = 1;`）一つでちょうど
  説明できる：node_base 18 = 7 + 11、array_base 9 = 2 + 7、text_base 2 対 0。

test 専用の domain（`IdentityDomain::with_limits`）は修正にならない。relocation は固定番地の store に木を公開するので、その domain の
identity は process の生きた木と衝突する。

**修正**：test を独自の integration binary（`crates/syntax/tests/relocation_direct_parse.rs`）に移し、独自の process で走らせる。前提
（texts が空間の先頭から始まる）は assert で確かめる。`src/` は変わらない。

**確認**
- 2 word の NodeText lease を先に持たせる一時的な変更で、hosted と同じ diff を再現した。前提の assert があれば、失敗は 1 行になる。
- lib の test binary を `--test-threads=8` で 100 回走らせた。移す前は 35 回失敗し（40 回の標本では失敗 13 回すべてがこの test）、移した
  後は 0 回。

**記録。** PR [#887](https://github.com/kazhiramatsu/tsc-rs/pull/887) → merge `37a326f94`（2026-11-05。`16629932c`、最終 head `42865efb1`）。hosted run
[41913026714](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41913026714)：全 job pass。local（`16629932c`）：
- `cargo test -p tsc-rs-syntax`：lib 248（移す前は 249）、`relocation_direct_parse` 1、他の integration binary 11 本と doc test がすべて pass
- 再現と、並列 100 回の前後比較は上の通り。fmt
- `src/` に変更がないので、release build・8 project の出力比較・計測は対象外

## M2 の fidelity 修正：alias と reference の型引数からの推論を invokeOnce に通す（branch `perf/ts71-alias-inference-once`、tsgo #64553）

M2（仕事量の監査）の最初の結果。vue の instantiation が tsgo より 10% 多い（605,322 対 549,756）。両方の engine で呼び出し元ごとに数えて、
原因を特定した（`m2/vue-instantiations.md`）。tsgo `c8e9b7d25`（microsoft/TypeScript#64553、2026-09-30）は inferFromTypes の二つの腕を
invokeOnce に通した。shared-alias の腕（inference.go:82-87）と matching-references の腕（228-230）である。tsc-rs はこの commit の
InferenceKey（#841）だけを取り込み、二つの腕は 6.0.3 の形のままだった。import した generic alias は書かれたとおりの型引数を持つ。そのため
同じ alias の組を訪れるたびに、default を新しい mapper で埋め直して instantiate していた。修正では二つの腕を `invoke_once` に通す
（`InferAction::FromAliasTypeArguments`・`FromReferenceTypeArguments`）。alias の腕の条件は tsgo と同じく、どちらかの側に型引数があること
（either-side）。inferFromObjectTypes の reference の場合は、tsgo と同じく同じ helper を直接呼ぶ。

**統計行**（1 thread、`--extendedDiagnostics`）
- vue：Instantiations 605,322 → 569,481（tsgo 549,756）、Types 205,054 → 204,175、Symbols 619,373 → 616,641。alias の default の fill は tsgo
  と同じ数（5,438）。
- ts +3・next +3・VS Code +5・zod +12：いずれも cache される getModifiersTypeFromMappedType での増加。depth guard は instantiated mapped type
  の recursion identity を modifiers type から取るので、走査する stack が深くなると計算が増える。どれも tsgo に近づく方向。
- hono 0。
- effect −56・playwright −37：reference の腕から来る。どちらも元から tsgo より約 1 K 少ない。

腕と条件を分けた build で、すべての増減の出どころを確かめた。vue の残り +19,725（union の member list で +13.7 K、
getObjectTypeInstantiation の外側の型引数で +6.2 K）は別の原因で、M2 の次の項目。

**計測**：1 thread、通常 build、静かな machine（00:55〜01:05）。`serial.py` の warm-up 1 回 + 3 回の中央値（VS Code と effect は 5 回）。
base = main `6e20adc06`、本 PR = `907890feb`。

| project | 命令数 | 備考 |
|---|---:|---|
| vue | −0.32%（15.61 → 15.56 G） | peak footprint −6 MB |
| effect | **+0.46%** | |
| VS Code | +0.06% | |
| zod | +0.13% | |
| playwright | +0.09% | |
| ts | +0.01% | |
| hono | −0.04% | |
| next | −0.12% | 雑音 |

**予想との差**：予想は vue −0.2〜−0.7%（N = 35,841 × c = 1〜3 K 命令）、他は 0 だった。vue は帯の中。effect などの増加は予想に
入っていなかった。invokeOnce は alias と reference の組ごとに bookkeeping を足す。effect では呼び出しが 270 K → 389 K、depth guard の
stack の走査が 0.83 M → 1.33 M 歩に増え、1 歩ごとに recursion identity を計算し直す。tsgo も同じ algorithm の費用を払っている。次の
候補：stack の各要素の recursion identity を、最初に必要になった走査で覚えておく。計算される identity と modifiers type は変わらず、
以前からある走査にも効く。

**記録。** PR [#885](https://github.com/kazhiramatsu/tsc-rs/pull/885) → merge `ddd1bbaa3`（2026-11-05。`907890feb`、最終 head `0c2dbdf36`）。hosted run
[41924151803](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41924151803)：全 job pass。local（hosted がしないことだけ、`907890feb`）：
- tsgo に合わせた unit test（修正なしでは 108 対 100 で失敗）
- checker の test 1,854 + 1、fmt、release build
- 8 case で出力（exit code・stdout・stderr）が main と一致
- `--checkers 4` は既知の 4 件のみ。最新の 4-checker 報告とは 15,339 構成すべて一致
- 計測は上の通り

## M2 の fidelity 修正 3：getLiteralTypeFromProperties を tsgo の形に（branch `perf/ts71-properties-types-cache`、propertiesTypes の cache と alias の origin）

M2 の item 2 で、tsc-rs の型の数が tsgo より多い主な原因は Index（`keyof`）型だとわかった（zod +4,699、next +9,015、VS Code +8,371）。
tsgo の getLiteralTypeFromProperties（checker.go:27267-27294）は結果を (type, include, includeOrigin) ごとに `propertiesTypes` に
cache する。tsc-rs は cache のない 6.0.3 の形のままだった。

**移植で見つかった出力の差**：tsgo の origin の条件は `includeOrigin && classOrRef || t.alias != nil` で、Go では `&&` が先に結び
つく。そのため alias を持つ型は、呼び出し側が origin を求めなくても `keyof t` の origin を持つ。6.0.3 と tsc-rs は括弧で
`includeOrigin && (classOrRef || alias)` にしていた。origin を求めない呼び出し（relater.go:3546 の `keyof T` を制約で比べる経路
など）で差が出る。alias の制約では tsgo が `Type '"c"' is not assignable to type 'keyof C'.` と書くところを、tsc-rs は
`'"a" | "b"'` と書いていた。

**修正**：関数を tsgo の通りに移植した。cache（`properties_types`）、tsgo の条件、含まれない index info を飛ばす処理。unit test は
二つの文面と、`Types` を tsgo の 96 − 1 で固定する（−1 は counting point CP-1：tsgo の `resolvingDefaultType` は型で、tsc-rs は
進行中の集合で持つ）。

**統計行**（B0 比、1 thread）：動くのは `Types` だけ。next −9,009、VS Code −8,376、zod −4,685、playwright −2,139、effect −2,076、
vue −1,187、ts −129、hono −115。どれも「cache が答えるようになった呼び出しで B0 が作っていた origin」から「alias に新しく作る
origin」を引いた差と、それが新しく作る union の残差で説明できる。origin の Index 型は zod（885）と next（1,269）で tsgo と一致する。
出力は 8 project とも一致し、`--checkers 4` は既知の 4 件のみ。

**計測**：1 thread、通常 build、静かな machine（M1 lane を止めて 07:43〜07:45、VS Code は 5 回で再計測）。

| project | 命令数 |
|---|---:|
| next | **−2.79%**（−0.81 G） |
| zod | **−2.69%**（−0.52 G） |
| playwright | −0.99% |
| vue・effect | −0.67%・−0.66% |
| VS Code | −0.26%（−0.36 G、5 回とも ±0.02 G 以内） |
| ts・hono | −0.27%・−0.06% |

peak footprint は ±3 MB 以内。

**予想との差**：予想は zod・next −0.03〜−0.15% で、実測はその 1 桁上（良い側）に外れた。数えた結果：
- N の下限は origin を作る繰り返し（≒ Index の超過）として見積もった。この値は正しかった（next 9,034、zod 4,711、VS Code 8,380）。
  しかし cache は無名型で origin を求める呼び出しと、origin を求めない呼び出しにも答える。実際の N は hit 全体で、next・zod・
  VS Code では下限の 1.5〜1.9 倍、effect・vue では約 7 倍。
- c は 1 回 1〜5 K 命令と見積もった。実測では大きな型への hit が多く（hit 1 回あたり property 22〜38 個）、1 hit で 24〜58 K
  命令になる。property 1 個あたり約 1.2〜2.1 K 命令。

仕組みは card の通りで、母集団の見積もりが小さく、1 回の呼び出しの重さを大きく見誤った。

**記録。** PR [#888](https://github.com/kazhiramatsu/tsc-rs/pull/888) → merge `449869d64`（2026-11-05。`e551de338`、最終 head `a9a8b7088`）。hosted run
[41938861822](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41938861822)：全 job pass。local（hosted がしないことだけ、`e551de338`）：
- tsgo に合わせた unit test（修正前は alias の文面が union になる）
- checker の test 1,868 + 1、fmt、release build
- 8 case で出力（exit code・stdout・stderr）が B0 と一致
- `--checkers 4` は既知の 4 件のみ。直前の 4-checker 報告とは 15,339 構成すべて一致
- 数え上げの build で N を測った（`m2/card3-counting-hooks.diff`）。計測は上の通り

## M2 の fidelity 修正 4：同じ alias の variance 経路は source に型引数があるときだけ（branch `perf/ts71-alias-variance-nonempty`、tsgo relater.go:3424）

None と空 list を区別する読み手の監査で見つかった出力の差。tsgo は同じ alias どうしの比較で variance の近道を
`len(source.alias.typeArguments) != 0` のときだけ使う。tsc-rs は空の list でも使っていた。空の list は、default をすべて持つ
generic alias を import して型引数なしで書いたときにできる（#886 が前提にしている表現）。そのため tsc-rs は default を埋めて
variance で比べていた。tsgo はここで型を構造的に比べる。

**差の出方**：import した `type Box<T = string> = { value: T }` を型引数なしで書いた `Box` を `Box<number>` に代入すると、tsgo は
`Types of property 'value' are incompatible.` を挟んで説明する。tsc-rs はこの行を落とし、型引数の組に直接進んでいた。さらに
tsgo が測らない `Box` の variance を測っていた（tsgo Types 83・Instantiations 5、tsc-rs 84・11）。

**修正**：tsgo と同じ条件 `!source_arguments.is_empty()` を足す 1 行。unit test は 3 行の説明、`Types` 83 − 1（CP-1）、
`Instantiations` 5 を固定する。

**統計行**（B0 比）：vue Instantiations −46、next −10（Types −2・Symbols −2）、ts −5、hono +4。他の 4 project は同じ。B0 と
この PR の数え上げ build で、呼び出し元ごとにすべて説明できる。
- 減ったもの：空の list の default 埋め（ts −61、`Visitor` の比較 61 回に 1 回ずつ）、alias の variance 測定とその marker の
  instantiate（vue −45 など。next の Types −2・Symbols −2 はこの marker の型と symbol）、型引数の variance 比較（vue −1）。
- 増えたもの：構造的な比較での member の instantiate（ts +39 +39、hono +2 +2 など）。
構造的な経路に移った比較は ts 61・vue 8・hono 5・next 3・VS Code 3・playwright 1・zod と effect 0。出力は 8 project とも一致し、
`--checkers 4` は既知の 4 件のみ。

**計測**：1 thread、静かな machine（M1 lane を止めて 08:39〜08:42、next は 09:03 に 5 回で再計測）。8 project とも noise の幅
（−0.02〜+0.03%、next −0.01%）。予想（0 ± 0.05%、vue の Instantiations が少し減る）の通り。価値は fidelity にある。

**記録。** PR [#889](https://github.com/kazhiramatsu/tsc-rs/pull/889) → merge `e4f0e8f02`（2026-11-05。`4690e7169`、最終 head `995bee118`）。hosted run
[41946302576](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41946302576)：全 job pass。local（hosted がしないことだけ、`4690e7169`）：
- tsgo に合わせた unit test（修正前は 2 行の説明、Types 84・Instantiations 11）
- checker の test 1,868 + 1、fmt、release build
- 8 case で出力（exit code・stdout・stderr）が B0 と一致
- `--checkers 4` は既知の 4 件のみ。直前の 4-checker 報告とは 15,339 構成すべて一致
- B0 とこの PR の数え上げ build（`m2/card4-counting-hooks-B0-tree.diff`）。計測は上の通り

## S1a step 2：symbol の declarations を pool の 8 byte の span に（Symbol 40 → 32 B、branch `perf/ts71-declaration-spans-2`）

checker の data 層の書き直し（[checker-data-layer-plan.md](checker-data-layer-plan.md)、S1）の binder 側の 2 つ目の step。
`Symbol.declarations`（`SmallList<NodeId>`、16 byte、2 つまで inline）を、symbol arena が持つ pool の `Copy` な span
（`{start, len}`、8 byte）にした。

- **store**（最初の commit、#881 に cherry-pick 済み）：`ClaimedRecords::push_slice`（1 つの部屋の連続した位置に置く。入らなければ
  次の部屋、長い slice には専用の部屋）、`Tagged<T, R>`（`repr(transparent)`、同じ zero-valid な型を別の region に）と
  `slice_tagged`（`&[T]` で読み返す。store の外に cast を書かない）、region 37（declaration）・38（flow node）・39（flow item）。
- **pool**：`SymbolArena` ごとの `DeclarationPool`（`ClaimedRecords<Tagged<NodeId, 37>>`）。document の binder symbol には 128 item
  の部屋（document どうしが page を共有する）、checker の transient arena には 4,096。list は pool の最後の list である間はその場で
  伸び、そうでなければ写す。別の symbol の declarations を持つ symbol は span を写す（以前は list を clone し、3 個以上なら確保）。
  書き手は arena に移った（`push_declaration`・`set_declarations_from`・`extend_declarations`・`declarations_from`。checker は
  `ProgramBinder` 経由で transient だけ）。読み手は accessor の commit から変わらない。

**計測**（1 thread、通常 build、静かな machine（21:21〜21:24、Spotlight の索引が CPU ≈ 56% で CPU・wall は雑音あり）、`serial.py` の
warm-up 1 回 + 3 回を交互に測った中央値。base = この branch の親 `bde0ecd10`、本 PR = `2615cbb1e`）：VS Code 141.75 → 141.97 G（+0.16%）、
peak footprint 3,524 → 3,469 MB（−55 MB）。他の 7 project は命令 −0.02〜+0.35%、footprint −2〜−13 MB。memory report は binder の
`Symbol` 86.6 → 69.3 MiB、symbol 名と declarations 34.3 → 43.1 MiB（pool。`SmallList` が inline に持っていた 1 個の list も pool に
入る）、binder 計 445.8 → 437.3 MiB、serial checker 計 1,442.2 → 1,395.5 MiB（checker の symbol 5,890,486 × 8 B）、計上 −55.2 MiB。
**予想との差**：予想（計測前）は S1a-1 からさらに live −60〜−70 MiB、命令 0 ± 0.3%。memory は帯の少し下、命令は VS Code で帯の中、
他は −0.02〜+0.35%。原因はどちらも同じで、`SmallList` が inline に持っていた 1 個の list が pool の slot と store 経由の load になった。
span の word に `NodeId` を 1 つ inline に持つ（field 表の inline-1）が次の段で、この run がその値段を ≈ 9 MiB と 1 個の list ≈ 220 万の
load と出した。

**記録。** PR [#884](https://github.com/kazhiramatsu/tsc-rs/pull/884) → merge `c7e880283`（2026-11-05。store `9a84b9cb3`（#881 にも同じ変更）・accessor の
取り込み `bde0ecd10`・span `2615cbb1e`、最終 head `3eb8235bd`）。hosted run [41953364593](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41953364593)：
全 job pass。local（hosted がしないことだけ、`2615cbb1e`）：test（store 45・binder 80・api 332・program 684・emitter 352・ls 256・
checker 1,854、0 failed）、fmt・clippy・4 つの生成器の `--check`、fingerprint 143,190 行 差分 0、8 case で出力と統計行が親と一致、
`--checkers 4` は既知の 4 件（15,335 一致）で C8 の 4-checker 報告とは 15,339 すべて一致、計測は上の通り。

## 計測台帳：予想と実測

利用者の指示（2026-11-04）：「常に計測をして予想からずれている場合は戦略の変更、中止をしてさらに改善案を考えてくださいね」。
以後、slice ごとに着地前の予想（戦略 v2.1 の再見積もり
`target/claude-tools/handover/perf-tsrs/strategy-rebaseline-2026-11-04.md` §3.1 の credit P0〜P11、または slice の設計報告・
profile から出した帯）と着地後の `serial.py` の実測（VS Code 1 thread、通常 build、命令数と peak footprint）をここに並べる。帯を
下回った項目は後続の lane を始めず、profile の差で原因を書き、計画を変えるか止める。節目の帯：M1（front end）126.55〜136.63 G、
M2（checker の data 層）124.57〜136.63 G。通常 build で tsrs（87.3 G）に明確に勝つ（0.90× = 78.6 G）には **M2 の時点で
87.3 G 以下**が必要（§3.3）で、現在の credit ではそこに届かない：差を埋めるのは値段のついていない項目（A-1 の実測、C-0/C-1 の
memo の shadow 計測、checker の store の形）。

**計測の作法の訂正（2026-11-04 夕、Opus の発見）。** `serial.py` の「base を先に 1 回ずつ」は偏っていた：各 case の 1 回目の実行は cold start で
命令数が膨らみ（Next.js / zod / playwright +0.3 G、小さい project +1〜2.5%、VS Code の 1 回目 +0.86 G）、base が常に 1 回目なので branch が
常に有利に見えた。以後は case × binary ごとに計測前の warm-up 1 回、3 round の中央値（`serial.py` 既定）、lane の負荷も中央値を 0.1% 単位で
動かすので台帳の値は静音窓で取る。下の表で「1 回測り」と注記した行はその偏りを含む。修正後の手順で測り直した **merge 済み 5 本
（#868・#870・#871・#872・#873）の累積**：main `ed001ed92` 対 `1a05993fb`、VS Code 143.50 → 142.63 G（**−0.61%**）、peak 3,616 → 3,586 MB
（−30）、ts −0.65%、next −0.16%、zod −0.79%、effect −0.89%、hono −0.27%、vue −1.94%、playwright −2.44%。1 回測りの行を足すと VS Code ≈ −3%
に見えていたが、実体は −0.6%。memory-bound な code の予想は CPU 時間か word あたりの命令数で（D1 の教訓）、小さな差は静音窓の中央値で。

| 項目 | PR | 予想（VS Code 1 thread） | 実測 | 判定 |
|---|---|---|---|---|
| toolchain 1.99.1 | #859 | （P0 の anchor） | 145.33 → 143.37 G（−1.4%）、peak 同じ | 着地 |
| byte scanner（A-1b）と同時期の下準備（node 生成器・census・B-5 1〜2） | #856〜#864 | P2 0〜4.15 G（microbench 0.375× の移転は未計測なので下限 0）、他は 0 | 143.37 → 138.39 G（−4.98 G、−3.5%）、peak 同じ | 帯の上端を超えた：scanner の microbench は whole-program に移った |
| A-1a part 1（accessor と literal text の id） | #866 | 0（下準備） | 138.39 → 138.79 G（+0.3%）、peak 同じ | 予想どおり |
| A-1a part 2（32 byte の node record） | #867 | P1 0 G（符号は未計測）、memory −256 MiB | 138.57 → 141.97 G（**+3.40 G、+2.5%**）、peak 3,893 → 3,624 MB（−269） | memory は予想どおり、命令数は帯の外（費用）。A-1c + A-1d の帯（3.39〜5.02 G）がこれを取り返す前提：A-1c 完了時に再判定 |
| A-1c C1（NodeArray の modifier flags） | #870 → 5ed9e073c | P3 の一部（A-1c 全体で 3.39〜3.63 G。n5 の modifier 群 157 対 14 ms ≈ 2.0 G 相当） | −1.04 G（−0.7%）、peak 同じ（Opus の計測） | 見積の半分。原因（Opus の c1a profile）：157 ms は part 2 前の n5 の値で、part 2 の slot 読み（`modifiers_field()`）が dispatch の半分（`modifiers_of_data` 38 ms の大半）を既に取っていた。残る群（symbol write 経由の `get_declaration_modifier_flags` 66 ms、`modifiers_of::<Published>` 51、`combined_modifier_flags` 28 ほか ≈ 80〜100 ms）は C1 の対象外。改善案（未着手、要予想）：同じ value declaration の combined flag の per-symbol memo（≲ 0.8 G）。A-1c 完了後に判断 |
| A-1c C7+C8（parent を finish 時に付ける） | #874 | Opus の予想 −0.6〜−0.9 G（c1a の finish 時の parent walk 77 ms ≈ 1.21 G を消し、finish での付与が一部戻る）。v2.1 の parent 行 0.94〜1.19 G より下 | 最初の版 +0.1%、token 以下の kind を attach しない修正（c8b）で VS Code −0.38 G（−0.27%、静かな 3 round の中央値）、他 −0.6〜−2.7%、peak 不変 | VS Code は帯の下、他 7 は帯の上。原因：node ごとの attach の dispatch + scratch + bounds check（profile で 62 ms）。c8b として着地（Opus の判断）。node 種類ごとの直接 attach を別 slice で試し **+0.98 G（悪化）→ 中止**（生成した visitor が for_each_child と同じく payload を全部 decode する） |
| C-0 step 1（flow memo の帳簿） | #868 | 費用 <0.3%（hook map §5） | VS Code +0.3%、他 −1.7〜+1.5%（1 回測り） | 帯の端（偏りを除くと +0.5〜+0.9% の費用の可能性）。step 2 の shadow 計測が gate |
| R29 1〜3（union builder の入力と二分探索） | #871 | credit なし（fidelity） | VS Code −0.6%、他 −0.6〜−3.3%（1 回測り） | 偏りを除くと VS Code ≈ 0：帯の中 |
| M4 の小さな切り口（rows 6・8〜12・15・16） | #872 | P11 0〜0.81 G + P8 0〜0.05 G（≤0.6%） | VS Code −1.8%、peak −30 MB、他 −1.1〜−2.7%（1 回測り） | peak −30 MB は実在（累積の値と一致）。命令数は偏りを除くと ≈ −1%：帯の上端 |
| B-5 3a〜3c（journal 3 本の削除） | #873 | ≈0 | VS Code −0.4%、他 −0.5〜−2.1%（1 回測り） | 偏りを除くと ≈ 0：予想どおり |
| D1（publication の relocation を差分で、map と publish の融合。Opus） | #875 | −1.0〜−1.3 G（profile の relocation 91 ms + memmove 42 ms を平均の命令率で換算） | VS Code −0.25 G（−0.2%、3 round の中央値）、他 −0.7〜−2.4%、peak と memory 行は不変 | **帯の外（大きく下）**。原因：memory 帯域に縛られる streaming で、word あたり ≈10 命令 × 3,000 万 word ≈ 0.3 G が実体、memcpy は帯域の費用（≈0.02 G）。教訓を §0 の予想の作法に追加（CPU 時間か word あたりの命令数で見積もる）。8 project で改善なので計測値のまま着地 |
| R29 4〜7（残りの sort site の Go の履歴、symbol の順序 id） | なし（branch `perf/ts71-r29b-p2` deb3eeeab） | credit なし（fidelity） | VS Code **+3.6%**（150.27 対 145.00 G）、peak **+342 MB**（3,962）；zod +5.3%、effect +5.5%、hono +4.7%、next +3.9% / +34 MB（同じ run。1〜3 単独は −1.2%）。出力は main と一致 | **止めた**。原因：`SymbolOrderIds` が `Mutex<FxHashMap<SymbolId, u64>>` で、value link を読むたびに `touch_links`（mutex + hash、symbol あたり ≈40 B）。tsgo は id を `ast.Symbol` の field に持つ。改善案は data 層の書き直しの S1/S2（順序 id を checker ごとの dense な表に）に吸収 |
| C-0 step 2 + memo on（flow memo。step 2 = 鍵・表・lookup/store・shadow 検証、memo off；step 3 = on） | なし（branch `perf/ts71-c0-step2` 6dd36c387） | P9：VS Code 0〜2.2% | memo on で出力は main と一致（4-checker 15,339、8 project）。命令数：TypeScript compiler **−10.2%**、playwright −3.8%、vue −2.4%、Next.js −0.7〜−2.0%、zod −1.3%、hono −1.5%、effect ±0、**VS Code ±0**（表を 1<<14・1<<16 にしても同じ。hit 率は 36%：1,577 万 lookup / 568 万 hit / 1,245 万 store）。shadow 実行で VS Code に不一致 1 件（型は同じ、frame flag の bit 違い） | VS Code は帯の下端。tsrs の R17 は VS Code では移らない（飛ばせる walk が短く lookup/store と相殺）。不一致の修正と eligibility の実験を lane に出した直後、利用者が data 層の書き直しを決めたので **停止**（memo を戻すなら新しい data 層の上で） |
| call routes を tsgo と同じに（census の差の行：alias の gate、relation/signature の callback、index lookup、apparent type の cache、resolver の経路） | #876 | fidelity、≈ 0〜−1% | 1 回測り：VS Code +0.2%、他 −0.4〜−4.3%。**修正後の手順**（main `ed001ed92` を取り込んだ head 対 main の binary、3 round 中央値、lane 稼働中）：VS Code **−0.3%**、vue −2.4%、zod −1.0%、ts −0.6%、next −0.6%、effect −0.5%、playwright −0.5%、hono −0.1%；peak 不変；出力一致 | 帯の中（8 project で改善） |
| copy census の切り口 5 件（inference visited map の key 単位の clear、symbol 表の成長、JsString の一括 span、flow antecedent の snapshot、member sort の key/range buffer） | #877 | census の見積（walk / call ごとの確保数。G では未価格） | 1 回測り：VS Code −0.9%、他 −0.7〜−4.0%。**修正後の手順**（main 取り込み版 対 main の binary、3 round 中央値、lane 稼働中）：VS Code **−0.3%**、hono −0.5%、ts −0.3%、playwright −0.3%、zod −0.2%、vue −0.2%、next −0.1%、effect −0.1%；peak 不変；出力一致 | 帯の中（小さく改善。#876 の後に chain） |
| S10 flow node 16 B + antecedent の span + global な flow id（Opus、予想行） | — | Opus の census（10,270 file、flow node 239 万、file あたり中央値 73・p90 565・最大 17,232）：record 72.9 → 36.5 MiB、list 10.6 → 3.8 MiB、net ≈ −40 MiB live；命令数 −0.2〜−0.5%。設計変更（option b、2026-11-04 夜）：bind 中は binder の local `Vec`、完了時に document ごとの exact room（region 38）と list の room（39）へ `push_slice`、link・label span・`node_flow`・FlowId の側表を 1 回 rebase（≈ 0.03〜0.07 G）。理由：32 record の chunk 案では `ClaimedRecords::get_mut` の所有権検索が binder の REFERENCED/SHARED 書き込み 240 万回で ≈ 0.1 G（予想の 3 分の 1）。端数なし、id は document ごとに連続、LSP は room 単位で解放 | 着地待ち（S1a-2 の後） | — |
| A-1c C2/C3 list-stack（parser の list を stack で、JSDoc 含む。Opus） | #879 | VS Code −0.4〜−0.8 G（parse_bench −5.5% の 49 ms を parse の命令率で換算）、peak ±0、他 −0.3〜−1% | 静音窓、3 round 中央値、main `ed001ed92` 比：VS Code 141.93 → 141.53 G（**−0.28%**）、peak −3 MB；ts −0.25%、next −0.21%、zod −0.32%、effect −0.26%、hono −0.45%、vue −0.13%、playwright −0.81% | 帯の下端（parse の費用の一部は allocator と memory stall の重なりで、命令数には出ない）。lane 稼働中の計測では −0.54% に見えた（負荷の影響） |
| B-5 3d〜3e（deferred 診断の登録と satisfies の elaboration を候補をまたいで保持） | #880 | ≈0 | 出力一致（c4 15,339、8 project）；3 round 中央値（lane 稼働中、`1a05993fb` 比）VS Code −1.5%・peak −29 MB — 旧 base なので累積 −0.61% / −30 MB を含む；main 直接比較は静音窓で | 予想どおり以上（静音窓の値で確定） |
| A-3 loader（module 解決の resolver-local cache・所有権の整理・counters feature。Opus の SPEC、lane 実装） | #882 | lane の操作回数ベース予想 ≈ 0.1〜0.2 G（wrapper と buffer の数のみ：VS Code で hit 414,911 / miss 77,285、scoped heap 構築 1,548,943 回減）；v2.1 の P6 3.36〜7.66 G | 3 round 中央値（lane 稼働中、main `ed001ed92` 比）：VS Code **−1.4%**（142.48 → 140.54 G）、hono −0.6%、next −0.5%、zod −0.5%、playwright −0.5%、effect −0.2%、ts・vue ±0；peak 不変；出力一致 | lane の予想の上、credit の下。lane の操作回数予想は cache の効果（解決の walk そのもの）を数え落とした。front end の比重（Opus 35〜45%）を支持する材料 |
| tsgo 7.1 の async arrow の lookahead と identifier の数、巻き戻しで重複する comment directive の修正（Opus） | #883 | VS Code −0.2〜−0.5 G（−0.15〜−0.35%：二度 parse していた `async function …` 式 0.5〜0.8 M node × parse の ≈ 750 命令/node）。gate：Identifiers 行が 8 project で tsgo と一致 + fingerprint + diffout + syntax test | 静音（2026-11-05 07:00、main `ed001ed92` の code 対 `cecdb2fa8`、warm-up + 3 round 中央値）：VS Code 141.99 → 141.84 G（**−0.10%**、−0.15 G）、footprint 3,587 → 3,560 MB（−27）；ts +0.06%、next ±0、zod +0.03%、effect +0.05%、hono −0.16%（3 round とも、予想どおり）、vue +0.02%、playwright +0.13%（雑音の範囲）；exit と出力 size は全行一致。Identifiers は 7/8 で tsgo と一致（VS Code 5,945,718 → 5,867,751 = tsgo、−77,967；effect −293 = tsgo 7.1 の `statementHasAwaitIdentifier` 未移植、次の slice）。fingerprint が 2 つ目の fidelity bug を見つけた：`ScannerState` が comment directive を持たず（tsgo の Mark/Rewind は持つ）、巻き戻した speculation が `// @ts-ignore` を重複登録（40 入力で AST 一致、重複 41 件除去、unit test）。merge 後に fingerprint の baseline を更新 | 帯のすぐ下（−0.15 G は下端 −0.2 G の 4 分の 3）：予想の node あたり 0.4〜0.6 K 命令なら 0.24〜0.37 M node 分で、二度 parse していた node は見積の 0.5〜0.8 M より少なかった。価値は忠実性（Identifiers の一致、directive の重複除去）。VS Code では改善、他は中立 |
| S1a-2 declarations を 8 B の span に（`ClaimedRecords<Tagged<NodeId,37>>` に `push_slice`、store の `slice_tagged` で読む。Symbol 40 → 32 B。Opus） | branch `perf/ts71-declaration-spans-2` | memo §7 の records −130.6 MB の残り半分 ≈ −63 MiB + declaration pool ≈ ±5 MB；命令数 ≈ 0〜−0.3%（間接参照 1 段が消える） | 単発・負荷あり（参考値）：binder Symbol 86.6 → 69.3 MiB、binder 合計 445.8 → 437.3、checker 合計 1,472.8 → 1,395.5（−77：8 B × 5.9 M + 3 件以上の宣言の heap clone が消えた）、peak 3,730 → 3,637 MB（−93）；diffout 8/8、fingerprint 0 | 着地待ち（focused test → battery → 静音窓の serial）。参考値は帯の上。S1b-1 と transient constructor + forwarding 4 本で衝突（Opus が両方の着地時に解く） |
| S3 lane as built：node flag と `NodeLookup` で routing を消す（47 file）、`resolve_name` の locals bit、`NodeLinks` の 2 段表、cold id slot の 4 B 化ほか（Codex lane、base `43a78fc08`、patch 01〜04） | なし（`handover/perf-tsrs/s3-lane-record/`、N3 の入力） | lane の予想：step 1 −0.4〜−0.7%、2 −0.4〜−0.8%（+9 MB/snapshot）、3 −0.2〜−0.4%、4 小 | 静音、lane の基準 binary 対 最終 build、warm-up + 3 round 中央値：VS Code 143.57 → 142.27 G（**−0.91%**）、peak ±0；ts −0.80%、effect −0.67%、next −0.39%、hono −0.32%、vue −0.30%、playwright −0.10%、zod ±0；footprint は全 project で ±7 MB 以内 | 帯の下端（予想の合計 −1.0〜−1.9% に対し −0.9%）。出力は lane の 120 case + hono/zod で一致。B0 には入れず、価格つきの候補 card（N3 の枠）として rebase → 全 battery → B0 上で再計測 |
| B0 anchor（M0、2026-11-05 07:00）：main `c3485397a` = 直前 9 本（#875・#878〜#882・#886・#887 + docs）の累積 | — | （予想なし：anchor） | 1 thread VS Code **138.24 G**（`1a05993fb` の 141.97 から −2.6%）、peak 3,522 MB（3,586 から −64）；tsrs 85.48 G / 2,008 MB → 命令数 1.62 倍、peak 1.75 倍。A/A ±0.03% | 以後の予想はこの anchor から |
| M1 段階別の命令数台帳（観測 patch：Codex lane + 統合者の修正；tsrs 側：Opus） | — | 観測の overhead = 境界数 × probe の単価、最適化の credit 0 | coarse の仕事合計は native anchor +0.15%；VS Code の差 53.1 G の内訳：semantic 30.8 G（58%）、front end 20.5 G（38.6%：load 12.3 G、pub/adopt 4.1 G、bind 4.0 G）、他 1.8 G | gate 内（coarse ≤ 0.2%）。M2 の引き金が引かれ front end stream を同格に |
| 1 checker の CLI 経路の後始末（M1 の finalization 1,234 M の内訳。Opus の sub-scope 計測）：`check_snapshot_serially` の末尾で `CheckerState` を drop（580.8 M；CLI の noEmit 呼び出し `lib.rs:5310` が `leak_state=false` 固定、shard 実行 `:4857` は `checkers.leaks_states()`）＋ 戻り道で `ProgramSnapshot` と store を drop（342.6 M；shard 経路は `:4874` で forget）；per-file 診断 loop 2 回 64+55 M、assemble 48、render 65、session 32、run_config 21 | 予定（Opus、card：1 checker の CLI 経路でも shard と同じ leaked-states の予算で leak、watch/API は drop のまま、unit test つき） | VS Code 1 thread −0.92 G（−0.67%）、next ≈ −0.2 G、zod ≈ −0.15 G；既定と 4 thread は不変（shard）、single thread/WASM の行はそのまま得る | 計測待ち | follow-up：2 回目の per-file loop は 1 回目の行を再計算（≈ −55 M、別行） |
| tsgo の `getLiteralTypeFromProperties` の cache を移植（`propertiesTypes`：(type, include, includeOrigin) の key、speculation 区分 C、tsgo の origin 条件 `includeOrigin && classOrRef \|\| alias != nil`（Go の優先順位：alias 型は origin を必ず持つ）、含まれる index info だけを push。M2 item 2 の Index 型超過の原因；忠実性も：alias の `keyof` 表示が tsgo と同じに）（Opus、card 3） | #888 | origin を作る繰り返しだけを価格付け：N = next 9,034 / zod 4,711 / VS Code 8,380、c = 1〜5 K → zod/next −0.03〜−0.15% | 静音、B0 比：**next −2.79%**（−0.81 G）、**zod −2.69%**（−0.52 G）、playwright −0.99%、vue −0.67%、effect −0.66%、ts −0.27%、VS Code −0.26%（−0.36 G、5 round で ±0.02 G）、hono −0.06%；peak ±3 MB；出力 8/8 一致（stderr 含む）、c4 = 既知 4 件・15,339 一致；count は Types だけが動き（next −9,009、VS Code −8,376、zod −4,685）、origin の Index 型は zod 885・next 1,269 で tsgo と一致；unit test は tsgo の 2 文言と Types 96−1（tsgo だけが作る `resolvingDefaultType` の番兵：全 program で定数 −1） | **帯の外（良い側、1 桁上）**。原因は母集団と単価の両方：N は origin を作る繰り返しではなく cache hit の全部（next 16,354、zod 8,819、VS Code 12,380 = 1.5〜1.9 倍、effect/vue は ≈ 7 倍）、c は 1〜5 K ではなく大きな型で hit あたり 24〜58 K（property 1 つの走査 ≈ 1.2〜2.1 K；hit の型は next で平均 38 property）。計測 build の counter で確定（`m2/card3-counting-hooks.diff`） |
| 同じ alias 同士の variance 比較の経路を tsgo の条件（source の alias 型引数が非空）に（`structural.rs:527`；tsc-rs は import した全既定 alias の空 list でも通り、既定値を埋めて variance で関係付け、variance の測定まで行っていた。witness：`Box` 対 `Box<number>` の TS2322 で中間行が落ちる。M2 の None-vs-empty 監査の 1 件目）（Opus、card 4、忠実性） | #889 | 0 ± 0.05%（忠実性。N = 飛ばす関係：ts 61（Visitor）、vue 8、hono 5、next 3、VS Code 3、playwright 1） | 静音、B0 比：8 project すべて雑音の範囲（VS Code −0.002% … playwright +0.026%；next は 3 round +0.075% → 5 round −0.01% で雑音）；peak ±4 MB；出力 8/8 一致（stderr 含む）、c4 = 既知 4 件・15,339 一致；count は site 別に帰属（既定値の埋め込み回避 ts −61、alias-variance の marker instantiation 回避 next Types −2 / Symbols −2、構造比較で増える member の instantiation ts +39+39、hono +2+2）；unit test は tsgo の 3 行 chain と Types 83−1・Instantiations 5 に pin | 予想どおり（0）。価値は忠実性：診断の chain が tsgo と同じに |
| tsgo #64553 の移植：`inferFromTypes` の alias 分岐と matching-references 分岐を `invokeOnce` で（visited key の重複排除と循環の優先度、either-side 条件。tsgo `c8e9b7d25`「Cache inferences made from type arguments」の残り半分；#841 は InferenceKey だけだった。M2 の仕事量監査で発見：vue の Instantiations +10.1% のうち 64%）（Opus） | #885 | N = vue で 35,841 の instantiation 減、c = 1〜3 K 命令 → vue −0.2〜−0.7%、他 0 | 静音、親 `6e20adc06` 比、warm-up + 3 round（VS Code と effect は 5 round）：vue **−0.32%**（15.61 → 15.56 G、footprint −6 MB）、effect **+0.46%**（16.14 → 16.21 G）、VS Code +0.06%、zod +0.13%、playwright +0.09%、ts +0.01%、hono −0.04%、next −0.12%。count：vue Instantiations 605,322 → 569,481（tsgo 549,756）、診断 byte 一致、c4 = 既知 4 件・15,339 一致、diffout 8/8（stderr 含む） | vue は帯の中、effect は帯の外（費用）。原因：予想は消えた instantiation だけを価格付けし、`invokeOnce` の帳簿（effect で呼び出し 270 K → 389 K、深さ guard の stack step 0.83 M → 1.33 M、step ごとに再帰 identity を再計算）を数えていなかった。tsgo 自身の algorithm なので忠実性の価格として着地。follow-up の候補 card（価格つき、未着手）：inference stack の entry の再帰 identity を最初に必要な scan で lazy に cache（≈ 100 命令/step：effect −0.1 G、VS Code −0.1〜−0.2 G；relater の isDeeplyNestedType の stack も同形）。vue の残り +19.7K（union の member list +13.7K、outer 引数の mapping +6.2K）は次の M2 項目 |
| `is_non_generic_top_level_type` を tsgo の空判定に（alias の型引数 list が空なら非 generic。tsc-rs は (Some, None) だけを見ていたので、引数を書かずに使う全既定の generic alias の union を使うたびに instantiate していた。M2 item 1b：vue の残り +19.7K の原因）（Opus） | #886 | N = 32,627 × c = 1〜2 K（#885 の vue の率）→ vue −0.2〜−0.4%、他 0 | 静音、親 `22c755af5` 比、warm-up + 3 round：vue **−0.65%**（15.17 → 15.07 G、全 round 同じ）、他 −0.02〜+0.03%、footprint ±1 MB。count：vue 569,481 → 548,963（tsgo 549,756）、zod −459、next −42、hono −7（変わった property は全部 tsgo と一致か tsgo 側へ）；出力 8/8 一致（stderr 含む）、c4 = 既知 4 件・15,339 一致 | 帯の外（良い側）：消えた expansion は型と symbol の生成も止める（vue Types −906、Symbols −2,755）ので instantiation あたり ≈ 3 K。監査の手がかり（別項目）：`type_order.rs:497` は先頭の型が alias 引数を持つ時だけ引数 list を比べるが tsgo の `compareTypeNames` は常に比べる → 「list なし」表現では union の member 順が tsgo から外れる（playwright −5,856 の説明） |

**合算（2026-11-04 時点）。** 静かな計測の main `1a05993fb` は 141.97 G。4 つの checker/types の branch が全部入ると ≈ −2.5%
（加算できれば ≈ 138.4 G。merge 後に静かな machine で測り直す）。A-1c の残り + A-1d + A-3 の帯（3.39〜3.63 + 0〜1.39 + 3.36〜7.66 G）を
引くと M1 は ≈ 126〜132 G で v2.1 の帯（126.55〜136.63）の中：part 2 の +3.4 G を scanner の上振れ（−5.0 G）と checker 側の
上振れ（−2.8%）が埋めている形で、front end 単独では v2.1 の見積を下回っている。勝敗を決める I_M2 ≤ 87.3 G には、この帯の
どこに着地しても届かない：次の判断材料は C-0 step 2 の shadow 計測（memo の hit 率と純益）、C-1 の shadow、B-3/B-4 の store の
形の設計の実測。

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
