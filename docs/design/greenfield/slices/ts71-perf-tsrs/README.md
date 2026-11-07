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

## 方針転換：tsrs との差の地図と「移植の税」（2026-11-08、進行中）

arena 書き換え計画（[ts71-arch-arena](../ts71-arch-arena/README.md)）の段 0b で測った 4 本の天井は合計 ≈ 5% で、所有者は 2026-11-08 01:2x に
方針を転換した：「instruction の parity は構造書き換えの対象機構では届かない。memory（peak）の削減と、tsrs との差が集中する phase の
algorithm 差の調査に軸を移す」。続いて「tsc と tsgo から移植した関数を徹底的にトレース、プロファイルして差を確認」、tsrs の設計文書の
確認、そして「(2) の移植の税から先に片付けてください」。記録は `target/claude-tools/handover/arch-arena/`（`gapmap/` に lane の報告一式、
`tsrs-notes/` に tsrs の docs と notes の複製、rev `fc38431`）。

**測った事実（VS Code、1 thread）。** 統計：Types は tsc-rs 2,567,002 / tsgo 2,570,757 / tsrs 2,163,379、Instantiations 3,386,791 /
3,389,287 / 3,116,848（TypeScript compiler と next.js も同じ形：tsc-rs ≈ tsgo、tsrs は型 −7% / −2%）。tsc-rs は tsgo と同じ型の仕事をし、
tsrs は既定 mode でも tsgo より仕事が少ない（tuple の要素 type parameter の共有、空の index-info list で名前 literal 型を作らない、ほか未特定）。
命令 85.44 G 対 121.61 G（1.42×）、cpu 5.98 対 8.37 s（1.40×、CPI は同じ）、peak 1,991 対 2,674 MiB。worker thread の sample（4 kHz）では
parse 663 対 701 ms（tsc-rs が速い）、bind 413 対 288（1.43×）、check 6,252 対 4,751（1.32×、+1.5 s）、loader 周辺の「other」903 対 213
（4.2×、+0.69 s：module request の計画 245 ms、program 入力の parse 163、relocate 144、module 解決 190）。呼び出し回数（entry
instrumentation、`-pgo-instrument-entry=true` と `CFLAGS=-fno-profile-generate`）：resolve_call 1,067,207、choose_overload 1,069,920、
invoke_once 7,731,980 は両者で完全一致。多いのは get_contextual_type 1.18×、get_apparent_type_of_contextual_type 1.26×、
get_type_of_parameter 1.28×、get_contextual_type_for_object_literal_element 1.42×、get_object_type_instantiation 1.44×、
get_effects_signature 1.12×、get_type_at_flow_node 1.06×（tsrs が答えを持ち回る箇所）。同じ回数で self が重いのは
recursive_type_related_to（maybe stack の線形走査 vs hash）、invoke_once（hash の get/insert）、next_token（scanner 診断の cold 分割なし）、
properties_related_to、check_expression_for_mutable_location（contextual type の再問い合わせ、incl +319 ms）、is_reachable_flow_node_worker。

**tsrs の記録が答え。** tsrs の `notes/`（約 180 本）は出力同一のまま命令数を削った工程記録で、tsrs 自身の VS Code 1 thread は 10 月初めに
121.4 G（今の tsc-rs と同じ）、今 85.4 G。種類は 2 つ：(1) algorithm の変更（union の sorted run merge #250、Function/Object fallback の
filter #187、contextual type / property lookup / instantiation の再利用 #301/#304/#327、#299 の union/indexed access cache、lazy member 表は
本家 microsoft/TypeScript#64475/#64526 へ）— tsgo でも効く；(2) 移植の税の除去（round 1〜3：格納済み slice を返す、inference の visited map、
mapper cache の pool、keyBuilder の inline、Go の sort、hash の二重操作）— Rust 移植固有で tsgo には無い。tsrs は tsgo の Go 版にも counter を
入れて「関数単位で Go より仕事が多い箇所は sort 以外にない」と確認している。lane `gapmap` の表「tsrs catalog vs tsc-rs」（164 項目、
`handover/arch-arena/gapmap/CATALOG.md`）は各項目の tsc-rs での有無と、我々の時間・回数から見た上限（条件付き、加算不可、合計 ≈ 22%）を並べる。
上位：inference visited map（部分）、isConstContext からの contextual type 再利用（無し、−1.3%）、inline early return（部分）、格納済み
slice（部分）、member 表の walk、Function/Object fallback filter（無し）、contextual union constituent の cache（無し）、temporary list の
pool（部分）、maybe stack ≤ 32 の走査（無し）、enum member の simple relation（部分）。

**実施（所有者の順序：(2) を先に）。** lane `tax-a`（checker の複製・文字列・`format!` key・確保する述語：`is_const_type_variable`、綴り提案の
lowercase 確保、hot path の `JsString` 構築、`get_apparent_type_of_contextual_type` と `js_number_to_string` の `format!`、
`RelationChecker::union_members` の複製、`get_type_arguments` / `get_index_infos_of_type` の `Vec` 返し）と lane `tax-b`（hash 表の
rehash 2.1% と `Vec` 成長 2.1% の事前確保・再利用；hashbrown の反復順が出力に届かない証明を要求）を 03:1x に起動。1 項目 1 patch、
統合者が `serial.py`（1 thread、8 project）で項目ごとに測り、`diffout.py` の出力同一と focused test と（binder に触れれば）fingerprint
oracle を gate に、払う項目だけ PR にする。次は loader の「other」（4.2×）と、catalog の上位（reuse と inline）。

## 構造変更の試作と評価（2026-11-07、採用なし）

[下記の計画](#次のアルゴリズムデータ構造変更の計画2026-11-07) に沿って、call-scoped な object 構文共有と
直線 flow block の索引を独立に実装・測定した。**両案とも対象 workload の通常 wall で 5% 改善に届かず、不採用。**
VS Code の 1 / 4 threads は構文共有 +0.28% / +0.09%、flow 索引 +0.61% / +0.83%。
型・symbol の生成を遅らせる拡張と型推論 recipe は、追加観測と継続条件に基づき見送った。全 6 段階を実装・採用済みとは扱わない。

通常版の 8 案件・1 / 4 threads・2 試作の全 512 実行は出力一致。局所テストは構文共有 36、flow 関連 61 が成功し、
flow の 16 実行の shadow でも観測した区間と既存 memo の不一致は 0。全適合性・hosted code gates は不採用試作には実施していない。
実装・全案件の時間とメモリ・条件付き段階の判断・保存先は [構造変更の実施記録](STRUCTURAL-2026-11-07.md)。
compiler の main は #913 のままで、tsrs より速いという目標は未達。

## 行別プロファイルからの追加改善（2026-11-07、#912・#913）

`RESUME.json` から再開し、プロファイラを [#912](https://github.com/kazhiramatsu/tsc-rs/pull/912)、
flow memo の依存条件の変更を [#913](https://github.com/kazhiramatsu/tsc-rs/pull/913) として採用した。
merge はそれぞれ `144e71bca139875267b6d446e92980dee53a1f68`、`ace63ee1f1ab59b57b2fedbae55731e423bb3a38`。
**tsrs より速いという目標は引き続き未達。今回、tsrs との直接比較は追加していない。**

**観測から選んだ変更。** 選択した argument identity の key では、VS Code の overload 選択は 1,069,920 calls 中、同じ key の再訪が 563 回、
contextual type は 1,816,050 calls 中 11,294 回だった。一方、flow memo の key hit 2,593,111 に対して epoch による拒否が 1,610,903 あった。
これは key の同一性の観測であり、同じ意味の仕事の全量や、再利用の正しさを証明する数字ではない。観測した生成数も Rust の malloc 回数ではない。
この結果から、単純な overload/contextual cache の追加よりも、scoped expression cache と flow memo の依存関係を調べた。

式キャッシュに触れていない sub-walk の結果だけを、キャッシュの scoped reset 後も再利用する。キャッシュの hit だけを依存として扱った
試案 `755bf9b43` は、VS Code の strict shadow 比較で 2 件の不一致を出した。同じ型を返しても transient/shared state が一致しなかったため不採用。
miss とキャッシュ自体の不在も依存として記録した `4987c5a40` では、1 / 4 threads・8 案件の記録で候補 hit 合計 21,504,938、不一致 0。
Vue serial は counter report がないため hit の根拠に含めず、出力を照合した。16 実行すべて同じ thread 数の基準と stdout・stderr・exit が一致。
初期の 4-thread Zod 比較には誤って serial 基準を使った記録があるが、基準自体に thread 数による既存の差がある。
採用判断には同じ thread 数で取り直した `epoch-negative-shadow-matched-4` を使い、誤った比較も削除していない。

最終 source `932e975f6141199123372a4a05e6c1baa8036abe` は shadow 検証済み実装にテストとコメントを追加したもの。
creation / instantiation / transient / reference / shared-flow / mapper / loop / depth / count-reset の既存 guard と strict verifier は維持した。
memo hit が親へ伝える cache dependency も保守的なまま。新しい mode flag、観測用コード、ratchet の変更は採用版に含めない。

**通常 release の本測定。** 基準は #911 最終版 `7706d25a3`、候補は上記最終 source。nice 0、warm files、fresh process、noEmit、
worker/checker 数を 1 / 4 に固定。各案件・各 thread 数で各版 2 warm-ups 後、順序を均衡させた AB/BA 12 組を測定した。
下表は各組の比の中央値（負が改善）。local build・test・解析を測定窓に重ねていない。外部 process の snapshot と各 run の load average を保存しており、
機械全体が完全に無負荷だったとは主張しない。事前の 4-pair pilot は別に保存し、下表に混ぜていない。

| project | wall 1 thread | wall 4 threads | peak footprint 1 / 4 threads |
| --- | ---: | ---: | ---: |
| VS Code | -3.61% | +0.07% | -0.12% / -0.13% |
| TypeScript compiler | -3.00% | +1.32% | -0.02% / +0.07% |
| Hono | -0.07% | +2.07% | +0.00% / -0.38% |
| Zod | +0.94% | +0.35% | +0.00% / -0.07% |
| Effect | -0.76% | -0.45% | -0.03% / -0.10% |
| Next.js | -0.62% | +0.22% | -0.02% / -0.14% |
| Playwright | +0.23% | +0.41% | -0.08% / +0.09% |
| Vue | -0.15% | -1.49% | -0.03% / -0.30% |

wall の探索的 bootstrap 95% 区間（1 / 4 threads）は VS Code: -4.08〜-0.87% / -0.63〜+2.99%; TypeScript compiler: -5.60〜+3.34% / -1.82〜+4.45%。
同一窓内の区間で、多重比較補正はしていない。全 sample を保持し、結果による除外や選び直しはしなかった。
8 案件の wall 中央値の合計は -1.95% / -0.19%。各案件 1 回ずつを数えた合計で、一般的な workload の重みではない。
メモリの微小差を一律の削減とは扱わない。詳細な区間・絶対値は `final-review.json` と両 `final-*/summary.json` にある。
今回の wall は多くの区間がゼロをまたぐ。VS Code 1 thread の −3.61% も pilot の −0.56%、命令数の約 −0.74% と開きがあり、
この小変更の安定した高速化率とは扱わない。TypeScript compiler の最終 wall は両 thread 数とも未確定。
Zod / Playwright などの正の中央値も保持した。命令数は全条件で約 0.21〜3.11% 減ったが、広範囲の大幅な wall 改善を実証したという採用ではない。

**検証。** 固定 tsgo は `aa8149273b23401f0a79a5f0384c42de51888693`。

- 最終 source で checker 1,887 unit + 1 integration、release CLI/conformance build が成功。全 workspace test と 1-checker 全適合性は hosted CI で実施し、local では重複実行していない。
- local `conformance_ts71.py --workers 1 --checkers 4` の 15,339 rows は既存 4-checker report と全 field 同一。
  1-checker reference との差は既存の 4 構成だけ。4 checker は types / symbols walk を評価しないため、別途 flow 関連 14 cases・15 configurations を
  1 checker で実行し、types / symbols を含む全 result fields が reference と一致した（compared 15、types/symbols Full 15/15）。
- 最終 wall 測定の 448 実行はすべて対応する基準と stdout・stderr・exit 同一。別枠で調べた 1 thread の 8 案件の Types / Symbols / Instantiations の統計値も一致。
  4 threads は Vue の統計値が一致し、ほかの 7 案件は両版ともこれらの統計値を出力しないため未評価。初回の比較 script は統計の存在を前提として停止したが、失敗記録を保持し、未評価を明示する手順で確認した。
- [tool CI 42332326407](https://github.com/kazhiramatsu/tsc-rs/actions/runs/42332326407)、
  [checker CI 42334410486](https://github.com/kazhiramatsu/tsc-rs/actions/runs/42334410486) は各最終 head の `gates` 成功後に merge。

最終 CLI SHA-256 `79e68a70641ebb3395759815320cb78d1477b5c7954333d1863b918bc729fd9d`。
ソース・通常版/観測版 binary・raw counter・全測定 sample・失敗した試案・各 check の command/exit/hash は local
`target/codex-line-followup-20261107/` に保持した。再開状態は `STATE.json` と `target/codex-line-profile-20261107/RESUME.json`。
元の source worktree も残してある。小さい実測改善を、未計測の大きな構造変更の効果に外挿しない。

### 次のアルゴリズム・データ構造変更の計画（2026-11-07）

所有者の追加指示：「プロファイラで重い処理の候補を絞り込んで、そのアルゴリズム、データ構造の変更を計画」。
以下は着手前の **計画**。その後の実装・効果検証と不採用判断は [実施記録](STRUCTURAL-2026-11-07.md) にある。着手順は **引数検査の構造分離 → flow graph の索引化 → 型推論の再利用単位の再設計**。
最初の 2 案を主対象とし、3 案目は追加観測で費用が残っている場合に進める。微小な table 調整を次々に足す進め方にはしない。

**絞り込みの根拠。** #911 の固定 source を観測した VS Code 1 run / TypeScript compiler 12 runs の CPU 比率。
以下の親・子は重なるため、行や列を合算しない。4-thread の critical path は未観測で、この比率を wall の削減率に置き換えない。
抽出値は local `target/codex-line-followup-20261107/architecture-evidence.json`、元データは `codex-line-profile-20261107/reviewed/`。

| 観測する処理・呼び出し行 | VS Code | TypeScript compiler | 計画への判断 |
| --- | ---: | ---: | --- |
| `choose_overload` inclusive | 40.01% | 35.96% | 本体のループより、その配下の仕事を対象にする |
| `get_signature_applicability_error` inclusive | 30.26% | 26.03% | 引数を文脈付きで検査する経路を優先 |
| applicability → `check_effective_arg_with_contextual_type`（`calls.rs:3728`） | 25.70% | 21.88% | 案 1 の中心。文脈依存の仕事と構文共通の仕事を分ける |
| inference → 同じ引数検査（`calls.rs:3162`） | 10.71% | 8.13% | 推論・適用性・通常 mode 間の再処理を測る。上の行とは重なる |
| `infer_type_arguments` inclusive | 16.22% | 13.17% | 引数検査以外に残る推論処理を案 3 で調べる |
| `get_type_at_flow_node` inclusive | 10.85% | 23.04% | TypeScript compiler では特に案 2 を優先する理由 |
| `begin_speculation` inclusive | 0.095% | 0.048% | checkpoint の map clone を主対象にしない |

`speculate` の scope は VS Code の約 28.3% に現れるが、ほぼすべて候補の検査を実行している時間。
transaction の保存・復元全体にその費用が掛かっているという解釈はしない。
観測した明示 key の再訪は VS Code の choose が 563 / 1,069,920 calls、contextual が 11,294 / 1,816,050 calls。
この key は mutable state を網羅せず、同じ値を返せても副作用が一致するとは限らない。関数入口に一般的な結果 cache を足す根拠には使わない。

**案 1：文脈付き引数検査を、構文の共通構造と候補ごとの状態に分ける。**
対象は [calls.rs](../../../../../crates/checker/src/calls.rs) の `ResolveCallCtx` / `infer_type_arguments` / `get_signature_applicability_error` と
[expr.rs](../../../../../crates/checker/src/expr.rs) の `check_expression_with_contextual_type`、object literal / function の検査経路。
現在は `EffectiveArg` を共有しても、推論・適用性・mode の再試行が同じ式の検査へ入り、文脈を push して再び構造を歩く。
既存の `check_expression_cached` は通常 mode の resolved type を保存する仕組みであり、文脈付きの結果を自由に流用できるものではない。

- 最初に call / argument node ごとに、候補数・relation pass・check mode・contextual type・inference context の組合せと再訪回数を分離する。
  AST の分類・member 列挙、文脈からの parameter 対応、実際の型生成、診断の exclusive 費用と生成物の生存期間を測る。
  現在の counter は argument identity の組合せまでで、「同じ node のどの部分を共通化できるか」は未確定。
- 試作する構造は、式ごとの不変な構文情報を順序付き配列にまとめた template と、候補ごとの contextual type / mapper / inference slots を持つ小さな overlay。
  `ResolveCallCtx` がまずその call の間だけ保持し、プロパティ名・spread/arity・context-sensitive 部分の判定を各 pass で組み立て直さない。
  object literal / arrow の型付けを最初の対象にし、文脈と無関係と証明できた部分だけを共通化する。候補に依存する型と symbol は overlay 側で生成し、
  既存の外部参照へ公開する境界で materialize する。異なる候補の semantic identity を同一化しない。checker 全体の無制限 cache は作らない。
- 候補の順序、Subtype → Assignable の順序、SKIP_* → NORMAL の再試行、literal の regular/fresh 判定、intra-expression inference の消去、
  rejected trial から残る once-state / 診断を保存条件に含める。単に「候補が不採用なら全状態を捨てる」設計にはしない。
- 共通化できる exclusive work が小さければ、この構造を導入する費用の方が大きい。node 単位の再処理・生成物を測った段階で見切る。
  25.70% の領域を仮に 20% 減らせて初めて CPU 全体で約 5.1% という規模であり、これは必要な改善幅の目安であって効果予測ではない。

**案 2：参照に無関係な flow 節点を、結果を計算する前に飛ばせる graph にする。**
対象は [flow.rs](../../../../../crates/checker/src/flow.rs) の `get_type_at_flow_node` / `get_type_at_flow_assignment` と binder の flow metadata。
現在は query ごとに antecedent を遡り、参照の一致・包含を調べてから `None` で次の節点へ進む。VS Code の観測は 16,735,837 steps、
shared-flow の線形比較は 9,380,298 回に対して 287,648 hits。これらの回数だけから wall 効果は算出しない。

- まず query ごとに「結果を変えずに通過した連続区間」と CALL / SHARED / LOOP / REDUCE_LABEL などで止まる区間を記録し、
  処理時間を参照照合・式の再検査・narrowing / join・memo 管理へ分解する。#913 後の残存費用を再測定してから試作する。
- immutable な直線 block に「触る root symbol / property path と不明な効果の有無」をまとめる。小さな sorted index と jump edge を持ち、
  query と無関係と判定できた block を一度で飛ばす。全 reference × 全 flow node の表は作らず、container 単位で必要になった索引を保持する。
  すでに確定した binding identity だけを使い、索引を作るために名前解決や型生成を先行させない。
- 最初はキャッシュ済みの単純な identifier / this に限定し、alias・computed property・captured reference が不明なら従来の walk に戻す。
  CALL（asserts / never を含む）、shared state、loop fixpoint、try/finally の label 差替え、array mutation、outer-container resume は barrier とする。
  branch の join 順序・incomplete type・depth-limit 診断・transient flag の伝播を変えない。#913 の strict shadow は緩めず、戻り型だけでなく状態も照合する。
- 過去の C-0 table 拡大や今回の epoch 緩和とは異なり、計算する節点数自体を減らす案。VS Code の flow 領域が 10.85% なので、
  flow だけで CPU 全体 5% を狙うにはその領域を約 46% 減らす必要がある。TypeScript compiler では約 22%。
  安全に飛ばせる区間がこの規模に届かないなら、graph の大改造は進めない。

**案 3：型推論の構造走査と、推論候補への更新を分ける（追加観測後）。**
対象は [inference.rs](../../../../../crates/checker/src/inference.rs) の `InferTypesWalker` / `invoke_once` / `infer_from_properties`。
visited map と buffer の再利用は実装済みであり、再提案しない。推論 slot は可変で、priority・contravariant・bivariant・recursive stack に依存するため、
source / target だけの結果 memo は不可。C1 の checker 全体 generation で hit が消えた問題も、そのまま持ち込まない。

同じ型構造を歩いて得る「対応する property / signature / type argument と処理順」の recipe と、その場の inference slots に候補を適用する処理を分離する。
初版は元の走査と同じ時点で解決済みの immutable な shape pair に限り、型や symbol の生成、副作用、未完の再帰に触れる recipe は保存しない。
member 構造の revision を照合し、保持範囲も inference context / 対象 container 内に制限する。cache のために lazy member を先行展開しない。
優先順位・分散・候補の順序を保ち、候補更新のログと全状態を通常 walk と照合する。先に pair ごとの再訪数、visited miss の理由、
構造走査と候補更新の exclusive 費用を記録し、案 1 と重なる費用を二重に見積もらない。十分な構造走査が残らなければ保留する。

**実行と採否の順序。**

1. #913 採用後の通常 source/binary を固定し、1 / 4-thread profile を取得。4 threads では全 thread の CPU と critical path を区別する。
   上記の不足 counter を 1 案ずつ足し、観測 overhead を通常版との照合で把握する。観測時間を性能結果として使わない。
2. 案 1 の call-scoped prototype、次に案 2 の単純な直線 block prototype を別々に評価する。新設構造の memory と作成費用も計上する。
   source / binary / 入力を固定し、8 案件・1 / 4 threads の wall と peak footprint で判断する。単独で効いた案だけを組み合わせ、効果を足し算せず再測定する。
3. 各案で固定 tsgo の診断・emit・types/symbols、局所の shadow state、checker tests、local 4-checker の既知 4 差分、hosted gates を確認する。
   binder metadata を変える案 2 は syntax fingerprint も確認する。観測可能な型・symbol の同一性や diagnostic order を変えて速度を得ない。
4. 大きな構造変更の継続目安は、狙った workload の通常 wall で再現する 5% 以上の改善を示せること。
   これは利用者へ約束する予測値ではなく、小さな効果に大改造を重ねないための作業上の目安。メモリ増と他案件の退行は併記して採否を判断する。
   A/A を含む測定計画と一度だけの独立確認を先に固定し、今回のように揺れが大きい場合は未確定と記録する。

当初の着手点は **案 1 の argument-node ごとの再処理と生成物の census**。実施後の状態は上記の実施記録を参照。

## 再評価（2026-11-07、#911）

所有者の追加指示により、不採用案と構造変更を実時間で再評価した。下の「終了」は 11/06 時点の判断を残した記録。
今回の採用は [#911](https://github.com/kazhiramatsu/tsc-rs/pull/911)、merge `f31ba6a4bacb25f66853c32f3625a01c477cd419`。
**tsrs より速いという目標は未達だが、改善余地は残っていた。**

- generic reference の member を必要な名前だけ instantiate し、signature / index の問い合わせは property 全体を生成せずに答える。
  全列挙時は先に生成した symbol を再利用する。元の eager 境界で「宣言 symbol をそのまま使えるか」を固定し、後から型の cache が温まっても
  identity が変わらないようにした。利用側の weak/empty/function/index-only 判定も合わせて変更した。
- C-0（`6dd36c387`、有効化試作 `77c2aa34d`）を再利用した。VS Code の既知の shadow 不一致を再現し、以前の共有 flow 計算が持つ
  effects を `impure_shared` が数えていなかったことを修正した。新しい型や symbol を作らない再訪でも、この情報の伝播は必要だった。
  store と admission に同じ条件を使い、照合基準は緩めていない。通常の identifier / this の key を詰めて、key 構築と複製の費用も減らした。
  memo table は checker ごとに 128 KiB。実行時の切替 flag は追加していない。

**最終版の実時間とメモリ。** 基準 `48c8e0a0d46ff09698a146897bf6a05ab39a0d08`、候補
`7706d25a361af52708b1f4be695b459d50b6bdf2`。各 project・thread 数で warm-up 2 回ずつの後、順序を均衡させた AB/BA 12 組。
表は各組の比の中央値（負が改善）。nice 0、worker/checker 数を明示、warm filesystem cache、fresh process、noEmit。
計測中に local の build・test・整形は重ねていない。計測前の snapshot では iTerm2 が CPU 約 35% を使っており、完全な無負荷は主張しない。

| project | wall 1 thread | wall 4 threads | peak footprint 1 / 4 threads |
| --- | ---: | ---: | ---: |
| VS Code | -1.22% | -0.83% | -7.25% / -7.92% |
| TypeScript compiler | -8.28% | -11.94% | -1.54% / -1.74% |
| Zod | -2.26% | -1.03% | -6.93% / -8.12% |
| Playwright | -1.54% | +0.84% | -5.29% / -4.19% |
| Hono | -2.80% | -2.87% | -9.70% / -9.22% |
| Effect | +0.09% | -1.55% | -5.69% / -7.77% |
| Next.js | -0.63% | -0.63% | -4.63% / -4.18% |
| Vue | -2.48% | -1.35% | -3.05% / -4.01% |

TypeScript compiler の wall の探索的 bootstrap 95% 区間は 1 thread −8.49〜−8.04%、4 threads −13.54〜−8.80%。
VS Code は −1.52〜−0.81%、−1.31〜−0.57%。Effect の 1 thread と短い 4-thread 案件の一部は差が未確定。
Playwright 4 threads は逆行の可能性を残す：初回 +0.84%、あらかじめ 1 回と決めた別順序の追加 24 組で +0.56%
（区間 −0.02〜+1.11%）。両窓を残し、改善には数えない。8 案件の wall 中央値の合計は 1 thread −1.53%、4 threads −1.56%。
これは各案件 1 回ずつを想定した合計で、利用者の workload 全般を代表する重みではない。メモリ削減も含めて採用した。
サンプルの除外はなく、区間は同一窓内の探索的なもの（多重比較補正なし）。

**tsrs との差。** 固定した `tsrs-sym-nopgo`（checkout `e18fb617ee516917342dcda27063ab67acc79e49`、通常の eager member、
`--fast` なし）と、同じ thread 予算で別途 6 組ずつ比較した。tsc-rs / tsrs の wall 比は VS Code 1.422 / 1.545、
TypeScript compiler 1.550 / 1.663（1 / 4 threads）。この 2 案件では出力が一致する。参照 tsgo revision は異なり、npm の PGO 配布版との比較ではない。

**検証。** 固定版 tsgo は `aa8149273`。checker は 1,885 unit + 1 integration が通過（`8070d31f0`）。その後の変更は shadow-only branch の
不要な `return` の除去だけで、最終版の通常 CLI と conformance binary は検証済みの前版と byte 同一だった。最終ソースで shadow の focused 28 tests、
Clippy（shadow）、fmt、release build を通した。通常 Clippy は前版で通過。通常 unit 実行の最初の `nice` は sandbox に拒否されたため、低優先度実行や計測結果とは扱わない。

- 4 checker の 15,339 report rows は既存 report と全 field 同一（report SHA-256 も同一）。1 checker との差は既知の 4 構成だけ。
  4 checker では types / symbols の walk は評価しない。影響のある 14 case・15 構成の 1-checker 確認では、types / symbols を含めて参照 row と一致
  （14 compared、ES5 1 構成は既存どおり skipped）。ratchet は変更していない。
- 新しい型付けの確認例は tsgo・基準・最終版でいずれも exit 0、診断なし。8 project の 1 / 4-thread 計測 448 実行（warm-up 込み）は stdout・stderr・exit 同一。
  Playwright 追加 52 実行と tsrs 比較 64 実行も、それぞれの基準と出力一致。
- 整理前の観測版 `deferred-c0-v3` は 1 / 4-thread の記録で候補 hit 合計 16,824,168、不一致 0。
  Vue の serial では counter report が出ず、出力だけを照合した（4-thread Vue の hit は記録されている）。観測版の数字は通常実行の wall と区別する。
- hosted [ci run 42326260148](https://github.com/kazhiramatsu/tsc-rs/actions/runs/42326260148) は最終 commit で `gates` が成功してから merge した。

最終 CLI SHA-256 `4203d7439352bd25973ff33fce050b3afeb05986cfa1181f0b9cec27f268a8ea`、conformance
`d78f2b8bd44011fbce178ace4517f115a8988b9071f86ee2bc126cab2ae18522`。source patch SHA-256
`c1853858d0a0434ae5babe1a0ba4812ea1f8b39540fcc6b0f23f78e7098d461c`。
raw output、binary/source の identity、全 sample、区間、実行 command は local の
`target/codex-perf-implementation-20261106/`（`adoption-final-{1,4}-review.json`、`adoption-playwright-confirm-review.json`、
`adoption-tsrs-{1,4}-review.json` ほか）。最終 A/B の harness は `recheck-adoption-final-used.py` として保存した。
以前の汚染を含む計測窓や中間版は最終結果と混ぜていない。

**次に残る候補。** C1 の conservative observer が 0 hit だったのは、無関係な mutable access でも checker 全体の generation を変える条件での結果。
古い反復 step 数だけから高速化率は出せないが、read dependency と実際の mutation の扱いを変えた構造で再検討する余地がある。
今回、90 本すべての実時間を再計測したという意味ではなく、台帳から選んだ候補と実装の比較を行った。C0 の復活と member 構造変更の効果を記録し、
小さい命令数削減だけで採用判断しない。

### 再開メモ：行別プロファイル（2026-11-07）

前セッションではプロファイラと HTML の完成で停止した。今回は `RESUME.json` から再開し、ツールのレビュー・CI・採用と上記の追加改善まで実施した。
以下の profile が観測している compiler source は引き続き #911 最終版であり、#913 採用後の profile ではない。

- HTML は local `target/codex-line-profile-20261107/report.html`（約 27 MB、ソース内包、単独でブラウザ表示）。
  呼び出し先を含む inclusive CPU time を既定に、self time、行別の色付け、呼び出し先・元への移動、検索、比較を表示する。
  NYTProf のような全行の直接計時・呼び出し回数ではなく、サンプリングに基づく CPU 時間の推定。行に対応する全 address を `atos` で解決する。
- ソースは [scripts/perf_line_profile](https://github.com/kazhiramatsu/tsc-rs/tree/7f79468ecc144517b5989c5e8dc84b290a7440bc/scripts/perf_line_profile)
  の `scripts/perf_line_profile/`、commit `7f79468ecc144517b5989c5e8dc84b290a7440bc`。`USAGE.txt` に再実行方法。
  local worktree は `/private/tmp/tsc-rs-line-profile-report-20261107`。[#912](https://github.com/kazhiramatsu/tsc-rs/pull/912) で採用済み。
- 観測ソースは #911 最終版 `7706d25a3`。release の fat LTO / codegen-units 1 を保ち、`line-tables-only` を追加した。
  VS Code 1 run と TypeScript compiler 12 runs、いずれも `--singleThreaded`、1000 Hz、nice 20。
  VS Code は診断による exit 2 で、要求した 2 回のうち 1 回のみ記録された。HTML は実際の process 数で正規化している。
  観測の経過時間は採用判定の wall 計測には使わない。
- 集計の 4 tests（inclusive/self、再帰、同じ行の inline 重複、CPU ゼロの待機、全 thread、binary UUID の不一致、外部 stdarch source の除外）と
  headless Chrome での操作確認を通した。`browser-check.json`、画面画像、全 profile、source/binary hash は同じ artifact directory にある。
  この版の samply sidecar は symbol 内の address をまとめるため、sidecar の line 情報だけで集計しない。
  レビューで外部 `core_arch` の path を project source と誤分類する問題を修正し、`reviewed/` 配下の解析を使って HTML を再生成した。
  元の profile/analysis は保持し、CPU 総量・thread 集計・行帰属率が変わらないことも確認した。
- 初期の手掛かり：VS Code の `choose_overload` は自身約 8.7 ms、呼び出し先込み約 3.50 s（観測 CPU の約 40%）。
  TypeScript compiler の `get_type_at_flow_node` は呼び出し先込み約 23%。親子の時間は重なるので合算しない。
  呼び出し行から overload 候補の試行・文脈付き式チェック・flow の再訪と cache / mapper / symbol の構造を調べた結果は、上の追加改善節に記録した。
  これらの時間がそのまま除去可能な費用であるとはまだ判断していない。
- 詳細な再開状態：local `target/codex-line-profile-20261107/RESUME.json`。元の性能改修の記録は
  `target/codex-perf-implementation-20261106/adoption-status.json`。次のエージェントはまず上の追加改善節と最新の `RESUME.json` / `STATE.json` を読み、完了した検証を重複実行しない。

## 終了（2026-11-06、所有者の決定）

所有者の判断（2026-11-06）：「FE4を実行しても3%程度ならやる価値はないですね。何か大きな構造変更をするのも限界でしょうか。であれば
そろそろ打ち切ろうかと思います。小手先の対応をしても意味がないので」→ 最後の card（#910）を入れて終了。PGO の試行と最後の計測も
取りやめた。

**到達点（VS Code 1 thread）**

| 時点 | 命令数 | peak footprint |
| --- | ---: | ---: |
| B0（2026-11-05 07:00、`c3485397a`） | 138.27 G | 3,520 MiB |
| main `d640a5852`（anchor #4、静音の 1 窓、9 round の low-mode 中央値） | 129.43 G（−6.4%） | 2,898 MiB（−17.7%） |
| ＋ #909・#910（各 card の実測の和。anchor ではない） | ≈ 127.7 G（≈ −7.7%） | ≈ 2,886 MiB（≈ −18%） |
| tsrs（PGO なしの build） | 85.5 G | 2,008 MiB |

- **tsrs との差**：命令数 ≈ 1.49 倍、peak ≈ 1.44 倍。
- **product の wall**（anchor #4、#909・#910 の前）：既定 2.97 s 対 1.85 s、4 thread 3.13 s 対 1.97 s、1 thread 8.29 s 対 5.70 s。
  - #909 は 4 thread と既定の wall を ≈ −1.5% 下げた。
  - #910 は 4 thread で −2.6%、既定で −1.2% 下げた。
- **約束と努力目標**（2026-11-18 の線：< 135 G・≤ 3,300 MiB と ≤ 132 G・≤ 3,100 MiB）：どちらも満たした。
- **tsgo との比較**（既定、VS Code）：tsc-rs 2.97 s・3,431 MB 対 tsgo 4.72 s・6,201 MB（tsgo は DET-1 の窓の値）。出力は byte
  単位で同一（DET-1、#905）。

**打ち切った理由**
- **差は散らばっている**：VS Code の profile では、差の内訳は checker 49%、binder 12%、allocator 7%、読み込み 5%、adopt 5%、parse 4%、
  その他 18%。1 つの関数で差の 2〜3% を超えるものはなく、名前の対応する仕事（名前解決、symbol の型、instantiation）は同等だった。
- **残る手は小さい**：
  - 値段の付いた data 層の計画（S2〜S11）は命令数 −5〜9% で、tsrs 比 ≈ 1.4 倍まで。
  - FE4（identity の書き換えをやめる）は ≈ 3%。
- **scheduler に得はない**：rayon への置き換え（r0）は速くならなかった。既存の pool はすでに偏りが小さい。
- **build の設定は同じ**：LTO は tsrs と同じ（fat、codegen-units 1）。tsrs は配布 binary に PGO（自身の記録で命令数 −13〜14%）を
  かけているが、試していない。

**やらずに残した候補**（値段は付いている）
- (b) include 理由の文字列（上限 23.7 MiB）
- 副表を record の field に（≈ 0.6〜0.75 G）
- phase の境目での purge（1 thread で ≈ 20〜25 MiB。ただし card なしの main では +4.9 MiB だった）
- `reduce_label_overrides` の空のときの省略（≈ 0.1 G）
- data 層 S2〜S11
- FE4
- PGO
- 忠実性の小さな修正：`conditional.rs:664-665/679-680` が、`any` の判定の前に restrictive な instantiation を作る（tsgo は省く。
  effect で型 3 つ）

**記録**
- 試作の一覧：[PROTOTYPES.md](PROTOTYPES.md)。merge しなかった branch は GitHub の `archive/2026-11/` の下にある。
- gate・lane・計測窓の記録：`target/claude-tools/handover/perf-tsrs/`（local）と、その backup
  `~/dev/tsc-rs-archive/2026-11-06/`（local）。
- 下の計測台帳に、card ごとの予想と実測がある。

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

## 1 checker の command check も checker state と program を process の終了に任せる（branch `perf/ts71-serial-leak`、M1 の finalization 行）

M1 の phase ledger で、tsc-rs の diagnostics/render/emit/finalization 行は VS Code で 1,234 M 命令、tsrs は 27 M だった。
sub-scope を足した ledger build を 2 つ作って中身を分けた（`m1-tsrs/finalization-split.md`）。大部分は後始末だった。
- `check_snapshot_serially` の最後での `CheckerState` の drop：580.8 M（直接 scope で測った）
- check から戻った後の `ProgramSnapshot`（全 tree と binder の record）と store の drop：約 342.6 M（他に大きな仕事のない
  隙間から推定）

**原因**：一度きりの command は program と checker state をわざと leak する（`with_leaked_program(!watch)`・
`with_leaked_states(!watch)`。`--generateTrace` では state を drop して types file を書く）。sharded driver はこの budget
に従い、state と snapshot を process の終了に任せる。1 checker の no-emit 入口は `leak_at_end = false` を渡していた。
その eager な serial 経路も state に `false` を直書きし、snapshot を残さずに戻っていた。そのため 1 thread の run だけ、終了の
直前に全部を drop していた。

**修正**：入口に budget の leaked-states を渡し、eager な serial 経路はそれに従って state と snapshot を残す（sharded
と同じ）。watch・API・trace の session は今まで通り drop する。出力・統計行は変わらない。unit test は text snapshot の Arc
の数で leak と drop を見分ける（drop で 1、leak で 3）。

**計測**：1 thread、静かな machine（M4 lane を止めて 11:04〜11:08）、親 `3f336c690` 比。
| project | 命令数 |
|---|---:|
| VS Code | **−0.66%**（−0.91 G、5 回。wall 8.69 → 8.58 s） |
| zod | **−0.91%**（−169 M） |
| effect・hono・playwright | −0.75%・−0.75%・−0.70% |
| next | −0.60%（−168 M） |
| ts | −0.32% |
| vue | 0.00% |

予想（VS Code −0.92 G など）の通り。vue だけ 0 なのは、bench の config が declaration diagnostics を求め
（`declaration: true` と `noEmit`）、declaration 診断の経路（`..._for_emit_with_checkers`、`leak_at_end = false` のまま）
を通るため。これは別の follow-up にした。4 thread と既定の構成は sharded driver（すでに leak している）なので変わらない。
1 thread（WASM）の構成はこの分をすべて得る。

**記録。** PR [#890](https://github.com/kazhiramatsu/tsc-rs/pull/890) → merge `ad0fbe716`（2026-11-05。`67f853a38`、最終 head `7db7063cb`）。hosted run
[41959000445](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41959000445)：全 job pass。local（hosted がしないことだけ、`67f853a38`）：
- unit test（Arc の数で leak と drop を見分ける。修正なしでは失敗）
- checker の test 1,870 + 1、compiler の test 54 + 188、fmt、release build
- 8 case で出力（exit code・stdout・stderr）と統計行が親と一致
- `--checkers 4` は既知の 4 件のみ。直前の 4-checker 報告とは 15,339 構成すべて一致
- 計測は上の通り

## loader の request の計画は tsgo と同じ条件でだけ file 全体を歩く（branch `perf/ts71-request-walk`、M1 の request collection 行）

M1 の phase ledger で、tsc-rs の module-request collection は VS Code で 1,975 M 命令、tsrs は 193 M だった。
`plan_module_requests_worker` は dynamic import を探す file 全体の walk を、tsgo の条件に加えて、本文に `/**` か `require`
という文字列があるだけの file でも行っていた。tsgo の条件は、parser が見た `PossiblyContainsDynamicImport` か JavaScript の
file（parser/references.go:16）。VS Code ではほぼすべての file に JSDoc がある。walk した 5,062 file（99.3 MB）のうち、
4,843 file（89.8 MB）は何も見つからない walk だった。

loader の計画（expanded）では、TypeScript の file の walk は tsgo の条件の外で何も記録しない。TypeScript では JSDoc に
降りず、require() は JavaScript でだけ request になる。修正では、二つの文字列の目印を static-import slice（`!expanded`）
にだけ残す。この slice は JSDoc の import と require 呼び出しで fail closed する。request は一つも変わらない。8 project の
`--traceResolution` の出力は byte 単位で同一だった（VS Code は 922,795 行）。contract test は、TypeScript の JSDoc
`@import` と `require()` が request にならないこと（tsgo と同じ）と、static slice が fail closed のままであることを固定する。

**計測**：1 thread、静かな machine（M4 lane を止めて 11:36〜11:40）、親 `03623ed12` 比。
| project | 命令数 | 無駄に歩いた本文 |
|---|---:|---:|
| VS Code | **−0.85%**（−1.17 G、5 回） | 89.8 MB |
| playwright・hono・ts | −1.20%・−1.10%・−1.03% | 15.1・2.4・7.0 MB |
| effect・zod | −0.86%・−0.75% | 15.3・14.5 MB |
| vue・next | −0.49%・−0.47% | 8.3・14.1 MB |

予想（c は VS Code の M1 行から 16 M/MB、±30%）に対し、VS Code・ts・hono は帯の中。effect・zod・next・playwright・vue は
35〜44% 下に外れた。1 MB あたりの walk の命令数が codebase で違うため（実測 9〜13 M/MB。byte あたりの node の数が
異なる）。教訓：walk の費用の予想では、c をその project 自身の ledger の行から取り、VS Code だけから取らない。

**記録。** PR [#891](https://github.com/kazhiramatsu/tsc-rs/pull/891) → merge `589fc4e81`（2026-11-05。`7a003d671`、最終 head `7a003d671`）。hosted run
[41957895758](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41957895758)：全 job pass。local（hosted がしないことだけ、`7a003d671`）：
- contract test と program の test 691、fmt、release build
- 8 case で出力（exit code・stdout・stderr）が親と一致。`--traceResolution` の出力は 8 project とも byte 単位で同一
- `--checkers 4` は既知の 4 件のみ。直前の 4-checker 報告とは 15,339 構成すべて一致
- 計測は上の通り

## 1 checker で emit operation を持つ check（declaration 診断の経路）も checker state と program を process の終了に任せる（branch `perf/ts71-declaration-route-leak`、#890 の follow-up）

#890 の計測で vue だけ 0 だった。vue の bench の config は declaration diagnostics を求める（`declaration: true` と
`noEmit`）ので、1 checker の declaration 診断の経路（`check_program_with_authoritative_modules_at_for_emit_with_workers`）
を通る。この入口は `leak_at_end` を受け取るが、compiler は `false` を渡していた。driver の inline 経路も state や session は
forget しても、snapshot は drop していた。

**修正**：compiler はこの経路にも budget の `leaks_states()` を渡す（`--generateTrace` では false なので、trace の state は
今まで通り drop して types file を書く）。driver の inline 経路は、state や session を forget する所で snapshot も forget する
（eager 経路と sharded driver と同じ）。副作用：1 checker の CLI emit 経路はもともと `leak_program` で session を残していた。
これからは snapshot も残す（hono の bench-full で出力 748 file が byte 単位で一致）。unit test は operation のある経路が
no-emit の経路と同じだけ残すことを Arc の数で見る（driver の修正なしでは 2 と 3 で失敗）。

**計測**：1 thread、静かな machine（12:45〜12:48）、親 `67f853a38`（#890 の head）比。
| project | 命令数 |
|---|---:|
| vue | **−0.38%**（−56.7 M、5 回。3 回では −57.8 M） |
| 他の 7 project | 0（中央値の差 ±2.8 M 以内） |

予想（vue −66 M、帯 −53〜−77 M。#890 で測った、peak footprint 1 MB あたりの後始末 0.18〜0.26 M 命令から）の帯の中の
下側。予想に書いた注意の通り、vue の peak には declaration 診断の一時的な memory も入っていて、それは state の一部ではない。

**記録。** PR [#892](https://github.com/kazhiramatsu/tsc-rs/pull/892) → merge `468faebd0`（2026-11-05。`67f853a38` の上、最終 head `b43580fac`）。hosted run
[41963996636](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41963996636)：全 job pass。local（hosted がしないことだけ、`b43580fac`）：
- unit test（operation のある経路も no-emit の経路と同じだけ残す。driver の修正なしでは失敗）
- checker の test 1,871 + 1、compiler の test 54 + 188、fmt、release build
- 8 case で出力（exit code・stdout・stderr）が親と一致。vue の統計行も一致
- 1 checker の CLI emit 経路：hono の bench-full で 748 file が byte 単位で一致
- `--checkers 4` は既知の 4 件のみ。#890 の 4-checker 報告とは 15,339 構成すべて一致
- 計測は上の通り

## 式の中の型引数は tsgo と同じ所でだけ試す（branch `perf/ts71-type-args-precondition`、M8 の parse 表の最初の card）

M8 の parse 表（`m8-parse/README.md`：VS Code、1 thread、engine ごとに 3 つの samply profile と観測用の counting build）
で、parse 行は tsc-rs 16.0 G 命令、tsrs 8.4 G だった。token（22.13 M 対 21.96 M）・node（17.02 M 対 16.94 M）・
identifier（5.87 M）の数は同じなので、差のほとんどは 1 個あたりの費用。speculation だけは逆で、1 回あたりの費用は
ほぼ同じ（84 対 74 命令）だが、回数が 10.92 M 対 2.42 M だった。tsc-rs の `try_parse` 9.45 M 回のうち 8.54 M 回は
1 か所で、その 8.50 M 回は tsgo の前提条件で外れる。

**原因**：tsgo の `tryParseTypeArgumentsInExpression`（parser.go:5332、`aa814927`）は parser の状態を保存する前に
安い前提条件を見る（JavaScript の file か、token が `<`・`<<` 以外なら mark/rewind せず nil）。tsc-rs は古い形のまま、
member expression の終わり毎・call chain の `?.` の後・`super` では二重に、状態全体を保存して `<` の rescan で失敗する
closure を試し、戻していた。

**修正**：tsgo の形（前提条件を先に見てから speculation）にし、`super` からは直接呼ぶ（tsgo と同じ）。`re_scan_less_than_token`
は `<<` 以外の token を変えないので、飛ばした speculation は必ず失敗して何も残さなかった：tree は変わらない。unit test は
境界の 3 例（`<<` を分けた generic function type の型引数、`f<string>(g)`、JavaScript では関係演算子の連鎖）を tsgo の
tree で固定する（`<<` の腕を消すと失敗）。fingerprint oracle は 143,190 行すべて一致。

**計測**：1 thread、静かな machine（13:24〜13:28）、親 `c234075b2` 比。
| project | 命令数 | 飛ばした呼び出し（N） | 1 回あたり |
|---|---:|---:|---:|
| VS Code | **−0.81%**（−1.10 G、5 回） | 8,495,809 | 129 |
| ts | **−0.70%**（−67 M） | 512,610 | 131 |
| playwright・effect・vue | −0.45%・−0.34%・−0.33% | 409 K・403 K・378 K | 138・131・129 |
| zod・next・hono | −0.26%・−0.24%・−0.17% | 351 K・518 K・39 K | 133・127・124 |

予想（N は counter で正確、c は profile から 80〜115 命令、VS Code −0.85 G・帯 −0.68〜−0.98 G）より 8 project すべてで
節約が大きく、帯の外（有利側）。1 回あたりは 124〜138 命令でどの project も揃い、c だけが 4 分の 1 ほど小さく見積もら
れていた。予想の c は profile の時間の割合を phase 平均の命令/sample で換算したもので、分岐の当たる直線的な保存・復元の
code では命令数を少なく数える。**教訓**：時間の割合からの c は IPC の高い経路では下限。次の parse の値付けは、経路の上で
c を測る（対の batch か、命令で重みを付けた sample）。

**記録。** PR [#893](https://github.com/kazhiramatsu/tsc-rs/pull/893) → merge `9f38bba14`（2026-11-05。`c234075b2` の上、最終 head `1135109c2`）。hosted run
[41968137800](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41968137800)：全 job pass。local（hosted がしないことだけ、`1135109c2`）：
- unit test（境界の 3 例を tsgo の tree で固定。`<<` の腕を消すと失敗）
- syntax の test 276、fmt、release build
- 8 case で出力（exit code・stdout・stderr）が親と一致
- fingerprint oracle（`main-b8a54e83a.tsv` 比）：143,190 行すべて一致、panic 0
- 計測は上の通り

## 終えた node の子に、node の payload から親を付ける（branch `perf/ts71-attach-from-data`、M8 の parse 表の 3 枚目）

tsgo の `finishNode`（`aa814927` の parser.go:5994）は `overrideParentInImmediateChildren`（parser.go:6005）で node の
`ForEachChild` の子に親を付ける。tsc-rs は node を格納した後に同じことをしていた：`attach_children` が格納した record を
`for_each_child` で decode し、子を scratch の `Vec` に集め、二度目の pass で親を書く。

**修正**：parser と JSDoc parser の finish（`finish_node_with_flags`、`finish_with_flags`）は `alloc_and_finish` を通る。
node が受け取る id（`next_node_id`）を親にして、格納する前の payload を生成済みの `try_visit_each_child` で歩き
（子は `for_each_child` と同じ：どちらも `scripts/generate_nodes.py` が同じ schema から生成。生成 code の変更なし）、
子ごとに bounds check 一つで親を書いてから node を格納する。decode も scratch も二度目の pass もない。`unsafe` なし。
root、missing node、直接 allocate する少数の node（ほとんど token）、incremental parse で複写した subtree は
`attach_children` のまま。speculation（`try_parse`/`look_ahead`）は tsgo の `mark`/`rewind` と同じく親を戻さず、
親を付ける時点（finish）と対象の子は前後で同じ。

**親の一致の確かめ方**：debug build では `alloc_and_finish` で終えた node ごとに、格納した record を `for_each_child`
で歩き直し、子の親がその node であることを assert する（release には入らない。syntax の test 276 で全 parse に掛かる）。
fingerprint oracle は reach した各 node の親と、それが walk の親かを hash する（`syntax_fingerprint.rs:435–438`）：
143,190 行のうち差は 0。8 project の出力・統計も一致。

**計測**：1 thread、静かな machine（15:10〜15:13、M5 lane を止めて）、親 `167f488a8` 比。
| project | 命令数 | 予想（帯） |
|---|---:|---:|
| VS Code | **−0.25%**（−333 M、7 回。今日の ≈ +0.9 G の雑音の 1 組を除く組は −263〜−409 M） | −350 M（−260〜−440）：帯の中央 |
| playwright | −0.34%（−42.8 M） | −39 M：帯の中 |
| ts | −0.25%（−24.0 M） | −22 M：帯の中 |
| effect | −0.22%（−33.8 M） | −32 M：帯の中 |
| next | −0.19%（−53.4 M） | −47 M：帯の中 |
| zod / vue / hono | −28.8 M / −14.9 M / +1.0 M | −41 / −24 / −8 M：節約が 6〜12 M 小さい |

予想は release build の前に書いた（N＝parse が allocate する node 数、VS Code 17,020,093。c は vscode/src の parse batch
で経路の上で測った：attach 全体 81.3 命令/node、この card の正味 −20.5/node、足す側 ≈ 61 も新しい経路で測った＝
#894 の教訓）。VS Code は帯の中央。footprint は ±3 MiB で不変。

**記録。** PR [#895](https://github.com/kazhiramatsu/tsc-rs/pull/895) → merge `80c8629b8`（2026-11-05。`167f488a8` の上、最終 head `0538d5bf4`）。hosted run
[41979180165](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41979180165)：全 job pass。local（hosted がしないことだけ、`0538d5bf4`）：
- syntax の test 276（debug build の親の assert 付き）、fmt、clippy（`-p tsc-rs-syntax --all-targets -- -D warnings`）、release build
- 8 case で出力（exit code・stdout・stderr）が親と一致。Identifiers・Symbols・Types・Instantiations も 8 project とも一致
- fingerprint oracle：`main-b8a54e83a.tsv` に対して 143,190 行のうち差は 0（parse の hash は各 node の親を含む）
- 計測は上の通り

## module の top-level await の文を、tsgo と同じく parse しながら見つける（branch `perf/ts71-top-level-await-spans`、M8 の parse 表の 2 枚目）

tsgo は parse しながら `statementHasAwaitIdentifier` を記録する（`aa814927` の parser.go）：
- `await` の identifier がこれを立てる（`newIdentifier`、`parseJSDocIdentifierName`）；
- `mark`/`rewind` と 15 の構文（function の block、class とその名前、enum、module、import と import clause、
  `require(...)`、export、property 名、binding identifier、identifier 名でない dot の右側）が保存して戻す；
- `parseToplevelStatement` が立った top-level の文の番号を `possibleAwaitSpans` に集め、module はその span だけを
  Await context で parse し直す（`reparseTopLevelAwait`、parser.go:516）。

tsc-rs は parse の後で module の全部の文の subtree を歩いていた（tsc 6.0.3 の `containsPossibleTopLevelAwait` の
射影。訪れた node 毎に children の `Vec`）。bench の 8 project ではこの walk は VS Code で 954,174 node を訪れ、
parse し直す文は一つも選ばない。

**修正**：tsgo と同じ所で同じ記録をし、`parse_toplevel_statement` で span を集め、tsgo の span による reparse
（parse し直した文が span を越えたら次の span まで続ける）にする。残す範囲の診断と recovery の記録はそのまま。walk は
削除。language service の incremental parse（本番の呼び出し元はなく、契約 test だけ）は text に `await` を含む node を
再利用しない（再利用した node は自分の identifier を flag に通さないため。incremental と full の parse が同じ文を
parse し直す）。

**選び方が tsgo になる**：dot の後の名前が `await` の文も parse し直す（tsgo は `export const x = a.await;` の
module で Identifiers 12、`a.b` なら 9）。bench では effect の Identifiers の差（tsc-rs 604,929、tsgo 605,222）が
消え、8 project とも tsgo と一致。tsc 6.0.3 のままだった test の期待値を tsgo の出力に移した：
- `export {}; interface I extends await<string> {}` は tsgo と同じ TS1109・TS1109・TS1005（walk は消える宣言を飛ばしていた）；
- `export {}; let a = await /1; b; c; x/;` は tsgo と同じく診断なし（後に `await (1,);` が続けば TS1109 一つ、JS の
  emit も tsgo と byte 単位で同じ）；
- await-flag-boundary の fixture（285 例）は tsgo の parser（`aa814927`）の dump を新しい oracle にした。

**計測**：1 thread、静かな machine（14:38〜14:41、M5 lane を止めて）、親 `fec97ea85` 比。
| project | 命令数 |
|---|---:|
| VS Code | **−0.08%**（−103 M、7 回。今日の ≈ +0.9 G の雑音の 2 組を除く組は −76〜−130 M） |
| playwright | −0.11%（−13.5 M） |
| 他の 6 project | ≈ 0（−4.7〜+6.7 M） |

予想（VS Code −0.17 G、帯 −0.10〜−0.34 G）の帯の下端。小さい project は帯より節約が小さい。pilot（walk を飛ばした
main）は walk の費用を正しく測っていたが、tracking が足す分を少なく見積もっていた（identifier あたり 1〜2 命令と
したが、小さい project からは 10〜15：binding identifier と property 名ごとの保存と、token の種類の比較。VS Code で
≈ 0.08 G）。**教訓**：card が足す構文ごとの仕事も、消す経路と同じ注意で値付けする。この card は主に fidelity（tsgo と
同じ Identifiers、同じ診断、fixture は tsgo 自身の parse）で、VS Code に小さな利得。

**記録。** PR [#894](https://github.com/kazhiramatsu/tsc-rs/pull/894) → merge `096cbed53`（2026-11-05。`fec97ea85` の上、最終 head `2bbefe4e5`）。hosted run
[41978497797](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41978497797)：全 job pass。最初の hosted run（`42bb6e7db`）は rust job の clippy
（`manual_is_multiple_of`、debug_assert! の中の `% 2 == 0`）で落ち、`2bbefe4e5` で直した。local（hosted がしないことだけ、`42bb6e7db`、
release は `fe1266e1b`＝test 以外同じ。`2bbefe4e5` は debug_assert! の条件一つだけが違い、release build には入らない）：
- unit test（`a.await` の module で Identifiers 12、`a.b` で 9：tsgo と同じ。main では両方 9 で失敗）
- syntax の test 276、fmt、release build
- clippy（`-p tsc-rs-syntax --all-targets -- -D warnings`、`2bbefe4e5`）：exit 0。`42bb6e7db` の parser.rs では hosted と同じく 101。
  以後の battery には変えた crate の clippy を入れる
- 8 case で出力（exit code・stdout・stderr）が親と一致。Identifiers は 8 project とも tsgo と一致（effect 604,929 → 605,222）
- fingerprint oracle：104 行（52 input）が違い、すべて想定どおり（effect 49 input と tsgo 自身の regression 3 例。3 例は
  この PR の conformance binary で tsgo の baseline と完全一致）
- await-flag-boundary の fixture：tsgo の parser（`aa8149273`）の dump を oracle にし、285 例すべてでこの PR と一致
  （6.0.3 との差は両方とも同じ 81 例）
- 計測は上の通り

## parser が identifier の名前を、parse の間だけ持つ名前の cache で引く（branch `perf/ts71-parser-name-cache`、gate を通った 2 枚目）

parser が作る identifier はそれぞれ escape した text を intern する（`current_token_name` → `EscapedName::escape`）。
その経路は毎回 `names::intern` を通っていた：Fx hash、thread-local の access、`RefCell` の borrow、32 byte の slot の
copy、当たりの前の `memcmp` の呼び出し（vscode/src で名前あたり ≈ 177 命令、うち cache の当たりの経路だけで ≈ 156）。
tsgo と tsrs は parse では source の slice を持ち、名前は後で hash する。

**修正**：thread ごとに escape した名前の cache を持ち、parse はそれを parse の間だけ持つ（`names::HeldNameCache`：
parser を作る時に thread から取り、捨てる時に返す。別の parse が持っている間に始まった parse は新しい cache を得る）。
16 byte 以下の名前は長さと重なり合う 2 つの little-endian word で正確に key になり、当たりは乗算 1 つと word の比較
3 つで、thread-local の access も borrow も呼び出しもない。長い名前は hash・長さ・text で引く。外れは今まで通り共有の
表で intern する（`EscapedName::escape` と共有する `escaped_id`）ので、id はすべて変わらない。memory は parse する
thread あたり 512 KiB。

**gate**：
- micro-batch（vscode/src の identifier token 2,775,905 個、11 round の合計、3 rep）：`escape` は 1 呼び出し ≈ 177。
  `escape` と全く同じ id を返す prototype（5,551,810 呼び出しで確認）の節約は、thread-local・RefCell・copy を除くだけ
  なら 31.7、この設計の key・hash・比較で **78.5**。
- 独立した判定：A 0.46 G（0.38〜0.53）、B 0.47 G（0.35〜0.56。参照の 2 engine も後で払う仕事は identifier あたり
  ≈ 20〜30 命令だけ）、C（反証役）は ≈ 0.40 G に弱める（0.30〜0.50。cache を file ごとに 1 回取ることが条件：
  呼び出しごとの thread-local の access なら ≈ 0.28 G で gate を通らない）。
- 予想（branch の前に台帳に記入）：VS Code −0.40〜−0.47 G（帯 −0.30〜−0.53）。この設計の CLI の pilot：low mode の
  median −527 M、同じ mode の組 −515〜−608 M（7 組）。

**計測**：1 thread、静かな machine（16:52〜16:55、M7 lane を止めて）、親 `d4d9d0364` 比。VS Code は 9 回で、run は
low mode とそれより ≈ 0.9 G 高い mode に分かれる（親は low 6・high 3、この PR は low 5・high 4。`perf11/modes.py`）。
| project | 命令数 |
|---|---:|
| VS Code | **−0.40%**（low mode の median −535 M、high mode −560 M、同じ mode の組 −537 / −515 / −565 / −522 M。全 run の median は mode の混ざり方で −633 M と読める） |
| playwright / hono / ts / zod / effect | −0.46% / −0.43% / −0.42% / −0.40% / −0.39% |
| next / vue | −0.31% / −0.22% |

予想（−0.40〜−0.47 G、帯 −0.30〜−0.53）の帯の上端。CLI の pilot（−527 M）と一致。footprint は ±4 MiB。

**記録。** PR [#897](https://github.com/kazhiramatsu/tsc-rs/pull/897) → merge `2508f2c5c`（2026-11-05。`e8ee6147e` の上、最終 head `d74d3374e`）。hosted run
[41990745045](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41990745045)：全 job pass。local（hosted がしないことだけ、`d74d3374e`）：
- unit test（cache が `escape` と同じ名前を返す：1〜40 byte の全長と各位置の 1 byte 違い、`__` の名前、空、multi-byte、衝突する 150,000 の text を 2 round、parse の中の parse）
- types の test 60、syntax の test 277、fmt、clippy（`-D warnings`）、release build
- 8 case で出力（exit code・stdout・stderr）が親と一致。Identifiers・Symbols・Types・Instantiations も 8 project とも一致
- fingerprint oracle：`main-d4d9d0364.tsv` に対して 143,190 行のうち差は 0
- gate の記録：`gate-card11/VERDICTS.md`、CLI の pilot `gate-card11/cli/`
- 計測は上の通り

## parser の node の finish を全呼び出し箇所に inline し、子の walk と encode を組み立てた variant に畳む（branch `perf/ts71-inline-finish`、M8 の parse 表の 4 枚目、価格の gate を通った最初の card）

parser は node の payload（`NodeData`：40 byte、約 300 variant）を、variant が分かっている呼び出し箇所で組み立てる。
それを out-of-line の汎用関数の連鎖（`finish_node_data` → `finish_node_with_flags` → `alloc_and_finish` →
`alloc_node` → `push_node` → `Node::encode` → `encode_payload`）で終えていたので、親の walk（`try_visit_each_child`）と
record の encode（`encode_payload`）がそれぞれ variant でもう一度分岐していた。tsgo と tsrs は型付きの constructor で
node を作り、どちらの分岐も払わない。

**修正**：finish の経路を record の encode まで `#[inline(always)]` にした。呼び出し箇所ごとに walk と encode が組み立てた
variant の腕だけに畳まれる。生成される関数（`Node::encode`、`encode_payload`、`try_visit_each_child`）は
`scripts/generate_nodes.py` で属性を付けて再生成、手書きの 6 関数は直接。論理の変更はない。

**gate（owner の 2026-11-05 15:25 の規則：code の前に値付け）**：
- 実際の parser（vscode/src の parse batch、1 round 16,256,039 node、3 rep）で、`finish_node_data` 以外を inline した
  pilot は node あたり −66.9 命令、`finish_node_data` も inline すると −92.7（差 −25.8、3 rep が ±0.1 で一致）。
- 独立した subagent の判定（data と code を渡し、こちらの結論は渡さない）：A（件数 × 経路の値）1.07〜1.13 G、
  B（静的な経路と両 binary の逆 assemble、tsgo・tsrs と比較）≈ 1.1 G（Identifier 88 → 29 命令、CallExpression
  ≈ 209 → 59）、C（反証役）は「立つ」（≈ 1.0〜1.1 G、下限 ≈ 0.9 G）。
- 予想（branch を作る前に台帳の gate 行に記入）：VS Code −1.58 G（帯 −1.2〜−1.75）。#895 の head での CLI の
  pilot は −1.58 G。

**計測**：1 thread、静かな machine（16:26〜16:29、M5 lane を止めて）、親 `d4d9d0364` 比。
| project | 命令数 | CPU |
|---|---:|---:|
| VS Code | **−1.16%**（−1,550 M、9 回。今日の ≈ +0.9 G の雑音の 1 組を除く組は −1,321〜−1,598 M） | 8.45 → 8.37 s |
| playwright | −1.23%（−156 M） | 0.74 → 0.73 s |
| hono / ts / zod / effect | −1.06% / −0.99% / −0.87% / −0.84% | ほぼ同じ |
| next / vue | −0.68% / −0.67% | ほぼ同じ |

予想（−1.58 G）の帯の中央。footprint は ±3 MiB で不変。code は +97.7 KB。PGO の build ではこの連鎖が既に
inline されて利得が小さい可能性があり、製品側の残る risk として記録（gate の指標は通常の build）。

**記録。** PR [#896](https://github.com/kazhiramatsu/tsc-rs/pull/896) → merge `9cc53e56f`（2026-11-05。`d4d9d0364` の上、最終 head `e3ddc0073`）。hosted run
[41987844512](https://github.com/kazhiramatsu/tsc-rs/actions/runs/41987844512)：全 job pass。local（hosted がしないことだけ、`e3ddc0073`）：
- `scripts/generate_nodes.py --check`、fmt、syntax の test 277（debug build の親の assert を inline した全箇所で）、clippy（`-D warnings`）、release build
- 8 case で出力（exit code・stdout・stderr）が親と一致。Identifiers・Symbols・Types・Instantiations も 8 project とも一致
- fingerprint oracle：main 自身の binary で `d4d9d0364` に書き直した baseline（`main-d4d9d0364.tsv`）に対して 143,190 行のうち差は 0
- gate の記録：`gate-inline/VERDICTS.md`（pilot の値、3 つの判定、CLI の pilot）
- 計測は上の通り

## 名前は名前のまま：定数の key は場所ごとに 1 回だけ intern し、union の合成は持っている名前を渡す（branch `perf/ts71-names-stay-names`、gate を通った 3 枚目）

text で引く symbol table は、引くたびにその text を intern する（`&str`・`JsStr` などの `NameKey`：
`EscapedName::from_escaped_text`）。VS Code（1 thread）では 4.41 M 回（`names.key.text_lookups`）、1 回 ≈ 166 命令で、
一度 intern した名前で引けば ≈ 25 命令。

**修正**：
- `InternalSymbolName` の定数を key にする lookup（module の解決ごとの `export=`、`__index`、`__call`、`__new`、
  `__constructor`、`export*`、`default`：26 箇所）と、literal の key を渡す helper の呼び出し（`"then"`・`"length"`・
  `"0"`・`"prototype"`・`"value"` など 16 箇所）を `known_name!` にした。場所ごとに定数を 1 回だけ intern する。
  `known_name!` は定数の text（literal か定数の path）を受け、まず `const` に束ねるので、変数を渡すと compile error。
- union・intersection の property の合成（`create_union_or_intersection_property`）は名前を text で受け取り、構成型ごとの
  property の lookup、index info の lookup で intern し直し、新しい symbol の名前には `String` も作っていた。2 つの呼び
  出し元はどちらも `EscapedName` を持っているので、それを受け取るようにした。
- id は変わらない（`known_name!` は text の lookup が intern していたのと同じ text を intern する。定数ごとに test で固定）。

**gate**：
- 上限：text の lookup 4,413,088 × c 141.5（micro-batch で実測：text 166.0、`known_name!` 24.5）＝ 0.62 G。
- 呼び出し元の特定：`SymbolTable` の lookup と汎用の `impl NameKey` helper に counting build でだけ `track_caller` を
  付けた（per-site の file は 1 回の lookup を 2 回数えると判定 B が見つけたので半分にした）。定数の key 2,341,148 回
  ＝ 0.33 G、union の合成が持っている名前を intern し直す分 1,177,333 回 ＝ +0.17 G。
- 独立した判定：A 0.37 G（union の修正込みで +0.14）、B 0.26 G（helper の呼び出し元を特定する前）、C ≈ 0.34 G
  （union の修正込みで ≈ 0.47）。予想（branch の前に台帳へ）：VS Code −0.45〜−0.50 G（帯 −0.35〜−0.60）。

**計測**：1 thread、静かな machine（17:57〜18:01）、親 `d4d9d0364` 比。VS Code は 9 回（親は low 7・high 2、この PR は
low 8・high 1）。
| project | 命令数 |
|---|---:|
| VS Code | **−0.36%**（low mode の median −478 M、high mode −496 M、同じ mode の組 −458〜−505 M の 6 組） |
| vue | −1.26%（−186 M） |
| playwright / zod / next | −0.70% / −0.63% / −0.60% |
| hono / ts / effect | −0.41% / −0.32% / −0.27% |

予想の帯の中。footprint は ±3 MiB。checker の変更なので 4 checker の conformance も回した：前回の 4 checker の
report と比べて 15,339 の構成すべてが同じ。

**記録。** PR [#898](https://github.com/kazhiramatsu/tsc-rs/pull/898) → merge `26042a78d`（2026-11-05。`065e00f39` の上、最終 head `754dfdf5d`）。hosted run
[42000714432](https://github.com/kazhiramatsu/tsc-rs/actions/runs/42000714432)：全 job pass。local（hosted がしないことだけ、`754dfdf5d`）：
- unit test（`known_name!` が定数ごとに `from_escaped_text`・`internal` と同じ名前を返す）
- types の test 58、binder の test 80、checker の test 1,873、fmt、clippy（`-D warnings`）、release build
- 8 case で出力（exit code・stdout・stderr）が親と一致。Identifiers・Symbols・Types・Instantiations も 8 project とも一致
- fingerprint oracle：`main-d4d9d0364.tsv` に対して 143,190 行のうち差は 0
- conformance（4 checker、`--workers 1 --checkers 4`）：前回の 4 checker の report に対して 15,339 の構成すべてが同じ
- gate の記録：`textkey/VERDICTS.md`
- 計測は上の通り

## binder の node ごとの helper を inline し、bindChildren は Unreachable を消さない（tsgo）（branch `perf/ts71-bind-inline`、gate を通った 4 枚目、bind 表の 1 枚目）

binder は node ごとに小さな helper（`get_container_flags`、`is_potentially_executable_node`、`is_narrowable_reference`）を、
宣言ごとに修飾子と名前の helper と `get_assignment_declaration_kind` を呼ぶ。どれも node の id を受け取り、record を引き直し
（id から index、bounds check）、kind で分岐していた（呼び出しの向こうで）。これを `#[inline(always)]` にし、呼び出し元が
読んだ kind で分岐するようにした。

また bindChildren は tsc 6.0.3 と同じく、実行されうる node を bind する前に Unreachable を毎回消していた。tsgo の
bindChildren（binder.go:1645-1660）は到達しない node にこの flag を立てるだけ。binder の flag の overlay は bind のたびに
parse の flag から作り直され（`BinderNodeFlags::new`）、Unreachable を立てるのは binder だけなので、消すことは何もしない
仕事だった。これと helper の呼び出しを除いた。test で、`function f() { a(); return; b(); }` の到達しない文だけが flag を持つ
こと（同じ木の 1 回目と 2 回目の bind で）を固定した。

**gate**：
- bind batch（vscode/src の 1 つおきの file を binder に通す：4,741 file、8,493,068 node。基準は node あたり 645 命令/round、
  CLI の 546 に近い）：9 つの helper の inline で node あたり −28.3〜−28.7（low mode の基準 run と pilot の全 run）。
  さらに 8 つを inline した v2 は効果なし（`declare_symbol` に押し込むと別の helper が out of line に戻った）。clear を
  除くと −12（−15.6 / −11.9 / −12.2、pilot の run の差は 25 M）。
- inline への独立した判定：A 0.48 G（0.40〜0.55）、B 0.48 G（0.37〜0.53。tsgo の bindChildren との違いは B が見つけた）、
  C（反証役）は「立つ」（0.41〜0.49 G、下限 0.36、CLI の決着の基準つき）。
- 予想（branch の前に台帳へ）：VS Code −0.59〜−0.70 G（帯 −0.50〜−0.75）。CLI の pilot（main `1cdae0b1b` の上、9 回）：
  low mode −706 M（−0.53%）、同じ mode の組 −542〜−725 M。C の基準をすべて満たした。

**計測**：1 thread、静かな machine（19:38〜19:42）、親 `9037c4ba9` 比。VS Code は 9 回（親は low 9・high 0、この PR は
low 8・high 1。`perf11/modes.py`）。
| project | 命令数 |
|---|---:|
| VS Code | **−0.53%**（low mode の median −703 M。同じ mode の組 −671〜−730 M の 8 組） |
| playwright / hono / zod | −0.72% / −0.69% / −0.52% |
| effect / ts / vue / next | −0.44% / −0.44% / −0.37% / −0.36% |

予想の帯の上端で、CLI の pilot（−706 M）と一致。CPU は同じ、footprint は ±4 MiB。checker も使う helper なので、4 checker
の conformance も回した：前回の report と比べて 15,339 の構成すべてが同じ。

**記録。** PR [#900](https://github.com/kazhiramatsu/tsc-rs/pull/900) → merge `a2bc727f0`（2026-11-05。`9037c4ba9` の上、最終 head `4dc1f8956`）。hosted run
[42012925586](https://github.com/kazhiramatsu/tsc-rs/actions/runs/42012925586)：全 job pass。local（hosted がしないことだけ、`4dc1f8956`）：
- unit test（`a(); return; b();` で到達しない文だけが Unreachable を持つ：同じ木の 1 回目と 2 回目の bind）
- binder の test 81、checker の test 1,873、fmt、clippy（`-D warnings`）、release build
- 8 case で出力（exit code・stdout・stderr）が親と一致。Identifiers・Symbols・Types・Instantiations も 8 project とも一致
- fingerprint oracle：`main-d4d9d0364.tsv` に対して 143,190 行のうち差は 0（bind の hash は node の binder flag を含む）
- conformance（4 checker）：前回の report に対して 15,339 の構成すべてが同じ
- gate の記録：`gate-bind/VERDICTS.md`
- 計測は上の通り

## symbol の部屋を parser が数えた上限で予約する（branch `perf/ts71-symbol-rooms`、gate を通った 5 枚目、memory の 1 枚目）

checker は bind の前に、source ごとの symbol の id を Program の順に予約する。source はその番地へ並列に、store に直接
bind される。部屋の大きさは source の node 数で、bind する symbol の約 7.4 倍あった。そのため source ごとの symbol が
それぞれ別の store の page から始まり、VS Code では 69 MiB の symbol に 178 MiB の dirty page を使っていた。

いまは parser が、bind が宣言しうる symbol の上限を木を作りながら数える（`NodeArena::symbol_bound`）。
- node ごとに kind の重み（`alloc_node` の中。parser の呼び出し箇所では定数に畳まれる）を足す。
- kind だけでは決まらないものは parser が足す：
  - export された宣言と ambient な宣言の export の片割れ（declareModuleMember の local と export）
  - parameter property の member
  - TypeScript で entity の member への代入（`f.a = …`）の expando member
  - JavaScript の代入と呼び出し
  - file ごとの 3 つ（JSON の `export=`、CommonJS の `module`・`module.exports`・`exports`）
- 数えないものがある：JavaScript の `@overload`（2 回 bind される）と、ドットを含む JSDoc typedef の名前空間。これらを
  持つ bind は、既存の relocation の経路で動く。
- 別の木から subtree を写した木（incremental parse。本番ではまだ使わない）は、従来どおり node 数を予約する。
- tsgo は binder ごとに symbol を確保し、予約しない。この変更が変えるのは tsc-rs の symbol の置き場所だけ。

**gate**：
- **計測した上限**：
  - region 3（binder の symbol）は、ピーク時点の `mincore` で 178.4 MiB が dirty だった。
  - source ごとの部屋の log から page の touch を数える model が、この値をそのまま再現した。部屋を正確に取れば
    69.3 MiB になる。
  - 最初の数え方が足りなかった file は、3 つの源ですべて説明がついた：export の片割れ 48,811、parameter property
    19,717、関数型と constructor 型 13,675。
- **pilot**（`58ac74f04`）、VS Code 1 thread、9 回：
  - footprint −117.2 MiB。
  - 命令数は −47 M（low mode。並べ替え検定 p = 0.0013）。
  - relocation は 7 project で 0、vue で 1（TypeScript の expando）。
  - 出力は 8 project で同じ。
- **独立した判定**（データとコードを渡し、結論は渡さない）：
  - A（件数 × 費用）：GO WITH CONDITIONS。model は −114.4 MiB：region 3 が −103.3、symbol の id の幅で大きさが決まる表
    （persistent symbol の links の index page、merge の bitset、DenseOwners）が −11.1。命令数は −29〜−47 M。
  - B（静的な網羅）：SOUND。symbol を確保するすべての箇所を、それを数える node に対応させた。overflow の経路も正しい。
  - C（反証役）：STANDS WITH FIXES。隣り合う部屋の false sharing は否定（10,209 の境界のうち 128 B の line を共有する
    のは 1）。
- **修正 3 つ**：TypeScript の expando を数える、写した subtree を持つ木は node 数を予約する、それらと overflow の
  relocation の test。
- **予想**（branch の前に台帳へ）：VS Code −110〜−117 MiB、命令数 −10〜−60 M。

**計測**：静かな machine（22:27〜22:36、他の lane は停止）、VS Code、`d613e1bb8` 比。

| | 基準 | この PR |
|---|---:|---:|
| footprint、1 thread（serial 2、9 回の median） | 3,449.9 MiB | 3,333.4 MiB（**−116.5 MiB**） |
| footprint、4 thread（7 回） | 3,886.4 MiB | 3,760.7 MiB（**−125.7 MiB**） |
| wall、4 thread | 2.95 s（最小 2.82） | 2.96 s（最小 2.81） |

- 1 thread の footprint：
  - 範囲は重ならない。
  - page reclaim は 7,455 減った。16 KiB の page で −116.5 MiB にあたる。
- 1 thread の命令数（serial 2、両 binary とも low 8・high 1）：
  - low mode の median は −33 M、high mode は −72 M。
  - 同じ mode の組は −9〜−71 M の 7 組。
- serial 1 は mode が混ざった：この PR は 6 回が high mode に入り、mode の間の run も 3 回あった。
  - modes.py の low mode の median は −5 M、同じ mode の組は −58 / +368 / +316 / −64 / −18 M。正の 2 組は、どちらも
    mode の間の run を含む。
  - mode の中心にある run だけで比べると −35 M（low）と −55 M（high）。
- 4 thread の命令数は +2 M、CPU 時間は 10.51 → 10.57 s。

relocation は 8 project とも 0。出力は 8 project で同じで、統計も同じ。`full` 構成（JS・宣言・map）の出力の木も、
hono（748 file）と effect（1,984 file）で byte 単位で同じ。予想の帯の中で、C の決着の基準をすべて満たした。

**記録。** PR [#901](https://github.com/kazhiramatsu/tsc-rs/pull/901) → merge `2164a93d9`（2026-11-05。`dfea63ac6` の上、最終 head `ac1d0a007`）。hosted run
[42037193177](https://github.com/kazhiramatsu/tsc-rs/actions/runs/42037193177)：全 job pass。local（hosted がしないことだけ、`ac1d0a007`）：
- unit test 3 つ：
  - 上限は 6 つの宣言の形でちょうど symbol + 3 になる（関数と const の arrow function の expando を含む）。
  - 2 symbol の部屋に 7 symbol を bind すると relocation され、同じ番地への直接の bind と一致する。
  - subtree を写した incremental parse の木は node 数を予約する。
- syntax の test 278、binder 83、checker 1,873、fmt、clippy（`-D warnings`）、release build。
- 8 case で出力（exit code・stdout・stderr）が基準と一致。Identifiers・Symbols・Types・Instantiations も 8 project とも一致。
- `full` 構成の出力の木：hono（748 file）と effect（1,984 file）で byte 単位で一致。
- fingerprint oracle：`main-d4d9d0364.tsv` に対して 143,190 行のうち差は 0。
- conformance（4 checker）：12,829 case、harness error は 0。
  - 前回の 4 checker の report に対して 15,339 の構成すべてが同じ。
  - 1 checker の report に対しては、記録済みの 4 構成だけが違う。
- relocation の trace：8 project とも 0。
- gate の記録：`gate-rooms/VERDICTS.md`。
- 計測は上の通り。

## flow の記録を 16 byte にし、label の list は bind 中は linked cell、凍結後は file ごとの正確な pool 1 つにする（branch `perf/ts71-flow16`、gate を通った 6 枚目、memory の 2 枚目）

binder の flow graph は、1 node 32 byte の payload enum で、label ごとに `Vec`、reduce label ごとに `Box` を持っていた
（VS Code で node 72.9 MiB、label の list 10.6 MiB）。

いまは `FlowNode` を 4 つの u32 語にした：flags、`a`、antecedent、`c`（kind ごとの配置は doc comment の表）。
- **bind 中**：label の list は per-thread scratch の cell の鎖（tsgo の `FlowList`）。label ごとの確保はない。
- **bind の終わり**（`FlowArena::freeze`）：すべての list を file ごとの正確な pool 1 つへ一度だけ書き、cell は thread へ
  返す。
- **bind の結果**（`FlowArena::rehome`）：全 file の bind の後で、凍結した node と pool を正確な大きさの新しい確保へ移す。
  長く生きる list が、bind の worker の一時領域と allocator の page を共有しないようにするため。
- **読み手**：`FlowArena::antecedents`・`payload`（借用の `FlowData`）と `FlowNode::payload_node`・`antecedent`（kind で
  特化）。
- flow graph は変わらない（fingerprint oracle の flow の hash で差 0）。

**gate**：
- **store 版**（F10b：領域 38/39 に global id で置く）：命令数 −69 / −86 M だが footprint −12 MiB。
  - `mincore` で store はちょうど 40.3 MiB。heap は 83.5 MiB のうち ≈ 56 しか戻らない。
  - そこで heap に置く。
- **pilot の variant**（`main-e84278c49` 比、VS Code 9 round）：
  - per-label Vec：−34.8 MiB / +195〜+216 M
  - cell：−30.6〜−30.7 / +29〜+75 M
  - cell + 正確な pool：−31.7〜−32.2 / +98〜+107 M
  - **re-home つき：−36.1 / +112〜+120 M**
- **独立した判定**：
  - A（件数 × 費用）GO WITH CONDITIONS。mimalloc は倍々の block を保ち、heap から抜いた byte は ≈ 0.7〜0.8 で戻る、とする
    model が全 variant に合う。配置の仮説を出し、re-home がそれを確かめた。
  - B（静的）SOUND。
  - C（反証）STANDS WITH FIXES。修正（正確な pool、凍結後の成長は release assert、空の lookup の guard は外す）を入れた。
- **予想**：−34〜−37 MiB、+0.05〜+0.15 G。

**計測**：夜間、他の lane は止めていない。load ≈ 2.2〜2.7、base と交互、`e84278c49` 比。

| | 基準 | この PR |
|---|---:|---:|
| VS Code 1 thread の footprint（9 回の median） | 3,333.5 MiB | **3,297.3 MiB（−36.2）** |
| 命令数（low mode の median） | 130.384 G | 130.465 G（**+80 M**） |
| VS Code 4 thread の footprint | 3,777.5 MiB | **3,686.4 MiB（−91.1）** |
| 4 thread の命令数 | 141.361 G | 141.510 G |

- VS Code 1 thread の footprint は範囲が重ならない。
- 同じ mode の組は +64〜+114 M（ほかに mode の間の run を含む 1 組 +415 M）。
- 他 7 project：命令数 −0.03〜+0.10%、footprint −5〜+1 MiB。

予想の帯の中。VS Code 1 thread は ≈ 3,297 MiB で、**約束 ≤ 3,300 MiB の内側**。

**記録。** PR [#902](https://github.com/kazhiramatsu/tsc-rs/pull/902) → merge `681fc89fc`（2026-11-06。`1d00eda8b` の上、最終 head `1c7070d28`）。hosted run
[42081813862](https://github.com/kazhiramatsu/tsc-rs/actions/runs/42081813862)：全 job pass。local（hosted がしないことだけ、`1c7070d28`）：
- unit test：凍結後にすべての kind を作ったとおりに読む（0・1・多の label、置き換え、clause の組、list の有無の reduce label、pool の
  最後の空 label、凍結した写しは元を bind 可能なまま残す）。cell での 0/1/多・到達しない/重複の除外・Referenced/Shared。
- binder の test 84、checker 1,873、api 332、fmt、clippy（`-D warnings`）、release build。
- 8 case で出力（exit code・stdout・stderr）が基準と一致。Identifiers・Symbols・Types・Instantiations も 8 project とも一致。
- `full` 構成の出力の木：hono（748 file）と effect（1,984 file）で byte 単位で一致。
- fingerprint oracle：`main-d4d9d0364.tsv` に対して 143,190 行のうち差は 0（bind の hash は flow graph を含む）。
- conformance（4 checker）：12,829 case、harness error は 0。
  - #901 の 4 checker の report に対して 15,339 の構成すべてが同じ。
  - 1 checker の report に対しては、記録済みの 4 構成だけが違う。
- gate の記録：`gate-flow/`（DOSSIER.md・README.md・窓の log）。
- 計測は上の通り。

## node の flowNode を 2 byte の slot にし、大きな file だけ overflow の map に置く（branch `perf/ts71-node-flow-slots`、memory の 3 枚目）

binder は node ごとに、その node を bind した時点の flow node（tsc の `node.flowNode`）を記録する。published な木では
store の region 5 に、node の位置で 1 node 4 byte の slot を持っていた（VS Code で 16,880,227 slot、64.4 MiB、51% が
埋まる）。

flow の id は file ごとの `FlowArena` の添字で、file の中だけの番号である。そこで slot を 2 byte にした
（`NodeFlowSlot`）。
- 0 は flow node なし、`0xFFFF` 未満は flow の位置（`FlowId::position`）、`0xFFFF` は「file の overflow の map を見よ」。
- overflow（`FxHashMap`）を使うのは、flow node が 65,535 以上ある file の、それ以降の flow だけ。空の map は確保しない。
- checker の読み（`flow_node_of`）は、store の 1 load と 1 比較のまま inline。overflow は `#[cold]` で out of line。
- binder の書き込みは、置き換えた値を返さない（どの呼び出し箇所も使わない）。overflow は「slot がそう言う node だけ」
  を保つ。

**試したもの**（`main-681fc89fc` 比、VS Code 1 thread、9 回。記録は `gate-nodeflow/`）：
- **file ごとの compact な形**（presence bit・rank・値を heap に置き、持ち主の file を引いて読む）：footprint −45.8 MiB、
  命令数 +860 M。
  - 診断 build（phase の mark ごとに retired instructions を出す。merge しない）で分けると：
    - bind +150 M（heap の作業用 map）
    - compaction +305 M（slot あたり 18 命令）
    - 読み +320〜+450 M（out of line で ≈ 42 命令。元は store の 4 命令）
  - 採らない。
- **2 byte の slot**（この PR の前身 pilot 3）：−32.1 MiB、+124 M（同じ mode の組 8 つで +112〜+155 M）。
  - 診断 build：bind +105 M、check +41 M。
  - 書き込みが古い slot を読まないようにして bind は +59 M。
- **予想**：region 5 が半分になる（64.4 → 32.2 MiB）ので ≈ −32 MiB。命令数は ≈ +0.01〜0.05 G と見たが、pilot は
  +0.12 G だった（書き込みの比較と分岐）。

**計測**：朝（05:57〜06:02）、他の lane は止めていない。load ≈ 3、base と交互、`681fc89fc` 比。

| | 基準 | この PR |
|---|---:|---:|
| VS Code 1 thread の footprint（9 回の median） | 3,297.3 MiB | **3,262.8 MiB（−34.5）** |
| 命令数（low mode の median） | 130.981 G | 131.105 G（**+124 M**） |
| VS Code 4 thread の footprint | 3,693.8 MiB | **3,675.7 MiB（−18.1）** |
| 4 thread の命令数 | 141.357 G | 141.474 G |

- VS Code 1 thread の footprint は範囲が重ならない。
- footprint は binary ごとに ≈ 3.5 MiB 離れた 2 つの水準を持つ（中央値は違う水準どうしを比べうる）。同じ水準どうしで
  比べると low 同士 −33.0 MiB、high 同士 −31.9 MiB。**≈ −32 MiB で記帳する**（予想 −32.2 のとおり）。
- 同じ mode の組は +94〜+172 M（ほかに 1 組 −54 M）。
- 他 7 project：命令数 +0.05〜+0.16%、footprint −1〜−7 MiB。

memory は予想どおり（同じ水準どうしで −32〜−33 MiB）。命令数は予想の上端（+0.05 G）の ≈ 2.5 倍。診断 build で、書き込み（8.67 M 回の比較と overflow
の guard、+59 M）と読み（5.03 M 回、+40〜60 M）に分かれる。VS Code 1 thread は ≈ 3,263〜3,265 MiB、≈ 131.1 G になる。

**副産物**（計測の方法）：VS Code の ≈ 0.9 G の mode は、診断 build のどの run でも「config: file names」（+0.18 G）と
「load: parse tasks」（+0.7 G）に出る。どちらも file system の syscall が主なので、compiler の code ではなく、kernel
側の file system の仕事が process の命令数に入ったものとみられる。check の区間の幅は 25〜41 M しかない。

**記録。** PR [#903](https://github.com/kazhiramatsu/tsc-rs/pull/903) → merge `38a260d6e`（2026-11-06。`1a875e73d` の上、最終 head `98d19f147`）。hosted run
[42109074264](https://github.com/kazhiramatsu/tsc-rs/actions/runs/42109074264)：全 job pass。local（hosted がしないことだけ、`98d19f147`）：
- unit test：slot の上限の flow とその次の flow、slot と overflow の間の置き換え（両向き）、削除、node の順（binder）。
  66,000 回の代入の後の参照が最後の代入で絞られる（checker。overflow を読まない変異では落ちる）。
- binder の test 85、checker 1,874、api 332、fmt、clippy（`-D warnings`）、release build。
- 8 case で出力（exit code・stdout・stderr）が基準と一致。Identifiers・Symbols・Types・Instantiations も 8 project とも一致。
- `full` 構成の出力の木：hono（748 file）と effect（1,984 file）で byte 単位で一致。
- fingerprint oracle：`main-d4d9d0364.tsv` に対して 143,190 行のうち差は 0（bind の hash は node ごとの flow node を含む）。
- conformance（4 checker）：12,829 case、harness error は 0。
  - #902 の 4 checker の report に対して 15,339 の構成すべてが同じ。
  - 1 checker の report に対しては、記録済みの 4 構成だけが違う。
- 記録：`gate-nodeflow/`（README.md・窓の log・診断 build の log）。
- 計測は上の通り。

## transient symbol の links を 20 byte にし、declared type と参照の意味は持つ少数だけを疎な map に置く（branch `perf/ts71-transient-links-20`、gate を通った 7 枚目、memory の 4 枚目、Y1/S2）

checker は transient（checker が作った）symbol ごとに links の記録を、transient の番号で密に持つ（VS Code で 5,885,636）。
記録は 28 byte だった：declared type、型、check flags、参照の意味、target、mapper、name type。

census（観測用 build で check の終わりに記録の field を数えた、VS Code 1 thread）：
- `declared_type` を持つのは **450**、`is_referenced` は **163**。
- `name_type` は 310,315、`type_of_symbol` は 1.94 M、`check_flags` は 4.80 M、`mapper` は 4.62 M、`target` は 5.40 M。

いまは記録を **20 byte** にした。
- transient symbol の declared type と参照の意味は、transient の番号を key とする疎な map に置く。
- 読みは `LinksTables::symbol_declared_type` と `symbol_is_referenced`（17 箇所）。field は共有の view から外したので、
  古い既定値を読む reader は作れない。
- persistent symbol は従来どおり 12 byte の paged record に両方を持つ。
- map は古くならない：transient symbol は追加だけで、speculation も切り詰めない（`speculate.rs` の category C）。
  番号が 2 つの symbol を指すことはない。まれな書き込みは dense の記録も伸ばすので、記録の数（trace）は変わらない。
- Y0/Y1 の memo の案（target と mapper を 32 byte の Symbol の末尾 2 語へ）は、binder と checker が `unsafe` を
  禁じるので extras の id 化が先に要る。同じ 8 byte を、Symbol と binder に触れずに取った。

**gate**（owner の 2026-11-05 15:25 の規則。台帳の行はこの branch より前に main へ）：
- **pilot**（`main-681fc89fc` 比、VS Code 9 round、warm-up 2、1 つの静かな窓）：
  - 20 byte（この PR）：footprint −45.9 MiB（水準を揃えて。中央値の比は −49.5）、命令数 +40 M（同じ mode の組 +29〜+87）
  - 16 byte（name type も bit 付きの map へ）：−67.4 MiB、命令数 **+1,059 M** → 不採用
- **独立した判定**：
  - A（件数 × 費用）GO WITH CONDITIONS：≈ −45 MiB・+40 M で記帳。7 project の停止条件（命令数 > +0.15%、
    VS Code の low mode > +0.1%、zod・next・effect の節約が予想の半分未満）。
  - B（静的）SOUND：Resolving を読む reader はなく、writer はすべて map へ、記録の丸ごとの複写もない。
    小修正 3 つ（doc link、memory 報告の bucket 係数、まれな書き込みで記録を伸ばす）を入れた。
  - C（反証）STANDS WITH FIXES：複数 checker・speculation・Resolving・他 crate・panic の攻撃はすべて失敗。
    footprint は水準を揃えて記帳。

**計測**：朝（07:44〜07:50）、他の lane は止めていない。load ≈ 3.5、base と交互、`main-903` 比。

| | 基準 | この PR |
|---|---:|---:|
| VS Code 1 thread の footprint（9 回の median） | 3,265.4 MiB | **3,215.9 MiB（−49.5）** |
| 命令数（low mode の median） | 131.079 G | 131.096 G（**+18 M**） |
| VS Code 4 thread の footprint | 3,664.1 MiB | **3,626.7 MiB（−37.4）** |
| 4 thread の命令数 | 141.889 G | 142.123 G |

- footprint は範囲が重ならない。binary ごとの 2 水準（≈ 3.5 MiB 差）を揃えると **≈ −46〜−49.5 MiB**
  （pilot の low 同士 −45.9、census の上限 −44.9）。
- 他 7 project：命令数 −0.03〜+0.07%、footprint next −7、zod −10、playwright −4、effect −10、vue −3、ts −1、hono −1 MiB。
- 判定 A の停止条件はすべて満たした。

予想（−45〜−50 MiB、+0.02〜+0.06 G）の帯の中。VS Code 1 thread は ≈ 3,216〜3,219 MiB になる。

**記録。** PR [#904](https://github.com/kazhiramatsu/tsc-rs/pull/904) → merge `e97586d70`（2026-11-06。`4d0cee783` の上、最終 head `42b8ad8a5`）。hosted run
[42124914961](https://github.com/kazhiramatsu/tsc-rs/actions/runs/42124914961)：全 job pass。local（hosted がしないことだけ、`42b8ad8a5`）：
- unit test：2 つの transient symbol で参照の意味は OR で積もり symbol ごとに分かれる、直接の印は `ALL`、declared type は片方だけ、
  記録の他の field は変わらない。speculate と modules の test は新しい accessor を通る。
- checker の test 1,875、api 332、fmt、clippy（`-D warnings`）、release build。
- 8 case で出力（exit code・stdout・stderr）が基準と一致。Identifiers・Symbols・Types・Instantiations も 8 project とも一致。
- `full` 構成の出力の木：hono（748 file）と effect（1,984 file）で byte 単位で一致。
- conformance（4 checker）：12,829 case、harness error は 0。
  - #903 の 4 checker の report に対して 15,339 の構成すべてが同じ。
  - 1 checker の report に対しては、記録済みの 4 構成だけが違う。
- gate の記録：`gate-links20/`（DOSSIER.md・判定・窓の log）。
- 計測は上の通り。

## checker の割り当てを tsgo と同じにし、既定の出力を決定的で tsgo と同じにする（branch `fix/ts71-tsgo-checker-association`、DET-1）

所有者の決定（2026-11-05 23:15）：既定の出力を tsgo と byte 単位で同じにし、実行ごとに変わらないようにする。それまでの既定
（8 checker まで、shard stealing と共有 queue）は実行ごとに出力が変わった（zod で 6 回中 4 通り：TS5115 の位置が動き、TS2344 が
一部の回だけ出る）。tsgo の既定は何回走らせても同じ。

変更（Codex lane DET-1、patch `3cb747705bc65b6d`。記録は `target/claude-tools/handover/perf-tsrs/det1-lane-record/`）：
- tsgo の `compiler/checkerpool.go` の割り当てを既存の driver に移植した：
  - 既定は 4 checker。`--singleThreaded` が `--checkers` に、`--checkers` が `TSRS_CHECKERS` に勝つ。数は
    `max(min(N, files, 256), 1)`。
  - source 優位と宣言優位の方針、重み `NodeCount + bytes/100` と import による正規化。
  - 解決済み import の graph の上の重み付き FENNEL（tsgo の定数と同点の扱い）。各 checker は自分の file を Program 順に見る。
- 取り除いたもの：shard stealing、共有 queue、directory と cost による分割、CPU・宣言・支配的な file による checker 数の自動
  調整とそれらの環境変数、checker の `TSRS_FILE_TRACE`。mode flag や別の経路は残さない。
- node 数と構文上の import 数を tsgo の factory と text scan と同じに数える（別の統計。arena の物理的な id は変えない）。
- merged interface・function と constructor・enum の precheck を checker ごとに行い（tsgo の checker-local flag）、file ごとの
  診断を getter が返る時点で確定する（tsgo）。どちらも旧来の「最後に全 checker の ledger を merge する」形が隠していた。
- Go の割り当て関数そのもの（upstream の部分文字列）で作った oracle が、bench 8 project の 19,064 file すべてで同じ checker を
  返す。

確認（local、main checkout、head `0f593a02a`、tsc-rs `66d6b4a66b73ebb9…`、tsgo `tsc-aa814927` `5fd89a44ad3d2656…`。
`gate-det1/battery.log`）：
- test：syntax 279、program 693、checker 1,863、compiler 245（native watch を含めすべて pass）、fmt、clippy（`-D warnings`）。
- 既定 5 回 × 8 project：project ごとに 1 通りで、tsgo の既定と stdout・stderr・exit が一致。`--checkers 2`・`8`（zod・next・vue）、
  `TSRS_WORKERS=1`・`8`（zod・vscode）、`--singleThreaded`（8 project）も tsgo と一致（`gate-det1/identity.py`）。
- 1 thread の診断は main（#903 の binary）と 8/8 同じ。fingerprint は 143,190 行のうち差 0。conformance（4 checker）：#902 の
  4 checker の report と 15,339 構成すべて同じ、1 checker との差は記録済みの 4 構成だけ。
- 1 thread の統計は next・playwright・vue・vscode で動く（Symbols +52〜+267、Types −166〜+118。診断は同じ）。tsgo の checker
  ごとの precheck（最初の宣言が library にあっても、checker が最初に見た宣言から調べる）による。Symbols は 3 project で tsgo の
  値に近づく。

計測（2026-11-06 08:42〜08:50、lane を止め writer は休止。`gate-det1/window.log`）：

| VS Code | wall s | CPU s | peak MB | 命令数 G |
|---|---:|---:|---:|---:|
| main 既定（8 checker、stealing） | 2.32 | 15.60 | 3,975 | 148.24 |
| **DET-1 既定（4 checker、tsgo の割り当て）** | **3.28** | 12.13 | **3,669** | 140.74 |
| main 4 worker / 4 checker | 3.15 | 11.11 | 3,682 | 142.10 |
| **DET-1 4 worker / 4 checker** | **3.45** | 10.95 | 3,606 | 140.47 |
| tsgo 既定 | 4.72 | 24.54 | 6,201 | 264.67 |
| tsrs 既定（4 checker、Go の割り当て） | 1.99 | 8.05 | 2,311 | 92.42 |

- 1 thread（VS Code、warm-up 2 + 9）：命令数 −9 M、footprint +5.4 MiB（3,265.3 → 3,270.7）。他 7 project：命令数
  −0.02〜+0.29%、footprint −1〜+5 MiB。
- 既定の wall は 7 project でも伸びる（next 0.58 → 0.80 s、zod 0.42 → 0.50、effect 0.40 → 0.50）。既定の peak は 8 project の
  うち 7 で下がる（ts は +2 MB）。
- VS Code の段階時間（`--extendedDiagnostics`）：4 thread で Parse 0.627・Check 2.673・Total 3.442 s、tsrs は 0.350・1.670・
  2.053 s。check の構造は tsrs の既定と同じなので、差は checker ごとの処理量（命令数 140 G 対 92 G）と、parse と pool の外の時間
  （lane r0 の対象）にある。

所有者の判断（2026-11-06、この計測を見て）：「このまま merge」。既定の wall は rayon 化（lane r0）と命令数の削減で取り返す。

**記録。** PR [#905](https://github.com/kazhiramatsu/tsc-rs/pull/905) → merge `454b6bbe3`（2026-11-06。`4d0cee783` の上、最終 head
`0f593a02a`）。hosted run [42133926294](https://github.com/kazhiramatsu/tsc-rs/actions/runs/42133926294)：全 job pass（rust 57 分、
conformance 24 分）。残る差：
- JS の割り当ての入力：JSDoc の reparse で Go が作る clone を数え切れていない。JS の多い project では割り当ての一致を主張しない
  （lane の 6 file の反例。bench の 19,064 file は TS・TSX・MTS・CTS・宣言・JSON だけ）。
- 4 checker の既知の 4 構成（再帰の error の位置が、先に計算した checker で決まる）。
- 過去に報告された static four の揺れは、138 回の対照で再現しなかった。

## symbol 表は 9 entry 未満なら hash の索引を持たず entry を走査し、索引は作るときに box にする（branch `perf/ts71-small-symbol-maps`、memory の 5 枚目、命令数も減る）

symbol 表（`SymbolTable`：binder の locals・members・exports と、checker が instance 化した型の members）は挿入順の map で、
entry の vector と、その位置の hash 表を持っていた。

census（観測用 build で各大きさに達した map を数えた、VS Code 1 thread）：1,276,677 個の map のうち **95% は 8 entry を
超えない**（9 に達するのは 59,229、16 を超えるのは 18,281）。

いまは 9 entry 未満の map は索引を持たない。
- 引くときは entry を走査し、4 byte の名前の id を比べる。
- 9 entry から位置の索引を作り、box に置く。索引のない map は vector と null の 1 語だけ。
- 挿入順、置き換えはその場、順序を保つ削除、は変わらない。
- unit test：20 個の名前を入れ、挿入のたびにすべての名前を引く（閾値の両側）。置き換えと削除を両側で確かめ、同じ entry を
  持つ走査の map と索引の map が等しいことも確かめる。

1〜8 entry に索引を持つと、hash 表の確保（32〜64 B 以上）と、map ごとに 32 byte の表の header がかかる。
8 個までの 4 byte の id を走査する方が、名前を hash して group を探るより安い。

**計測**：朝（09:54〜09:59）、他の lane は止めていない。load ≈ 3.5、base と交互、`main-904` 比。

| | 基準 | この PR |
|---|---:|---:|
| VS Code 1 thread の footprint（9 回の median） | 3,215.8 MiB | **3,179.1 MiB（−36.7）** |
| 命令数（low mode の median） | 131.205 G | 130.809 G（**−396 M、−0.30%**） |
| VS Code 4 thread の footprint | 3,600.3 MiB | **3,577.7 MiB（−22.6）** |
| 4 thread の命令数 | 142.033 G | 141.860 G |

- VS Code 1 thread の footprint は範囲が重ならない。pilot の窓（同じ基準）は low 同士 −36.7。
- 同じ mode の組は −352〜−446 M（ほかに 1 組 +73 M、mode の間の run を含む）。pilot は −526 M。
- 他 7 project：命令数 −0.07〜−0.30%（すべて減る）、footprint −1〜−11 MiB。

重い card の閾値（≥ 50 MiB、命令数 +0.3 G 以上、1 日以上）の下で、両方の軸で減るので、複数の判定の gate はない。
VS Code 1 thread は ≈ 3,179 MiB、≈ 130.8 G になる。

**記録。** PR [#906](https://github.com/kazhiramatsu/tsc-rs/pull/906) → merge `c18662e1a`（2026-11-06。`8b10dbb51` の上、最終 head `1ee8d5e67`）。hosted run
[42140898175](https://github.com/kazhiramatsu/tsc-rs/actions/runs/42140898175)：全 job pass。local（hosted がしないことだけ、`1ee8d5e67`）：
- unit test：閾値の両側で、すべての読み・置き換え・削除が一致し、走査の map と索引の map が等しい。
- binder の test 86、checker 1,875、api 332、fmt、clippy（`-D warnings`。最初の実行で新しい test の `.clone()` と index の
  loop を直した）、release build（test だけの修正の後も tsc-rs の sha256 は同じ）。
- 8 case で出力（exit code・stdout・stderr）が基準と一致。Identifiers・Symbols・Types・Instantiations も 8 project とも一致。
- `full` 構成の出力の木：hono（748 file）と effect（1,984 file）で byte 単位で一致。
- fingerprint oracle：`main-d4d9d0364.tsv` に対して 143,190 行のうち差は 0（bind の hash はすべての symbol 表を順に含む）。
- conformance（4 checker）：12,829 case、harness error は 0。
  - #904 の 4 checker の report に対して 15,339 の構成すべてが同じ。
  - 1 checker の report に対しては、記録済みの 4 構成だけが違う。
- 記録：`gate-smallmaps/`（battery・窓の log）、census は `gate-extras/after-det1.log`。
- 計測は上の通り。

## program の source text を専用の allocator heap に置き、adopt が解放する parse の出力と page を分ける（branch `perf/ts71-source-text-heap`、gate を通った 8 枚目、memory の 6 枚目、FE-T1）

loader は file ごとに、parse しながら source text を確保していた。一般の heap では、text は同じ大きさの級の page を、その file
の heap にある parse の出力（store の 64 KiB の mapping の閾値より小さい木）と共有した。adopt はそれを store へ移して解放し、
page は text のためだけに確保されたまま残った。

**場所の特定**（VS Code 1 thread）：
- **heap walk**（4 つの時点）：空き slot は、load 後 173 MiB、**adopt 後 438 MiB**、bind 後 331 MiB、footprint の peak で **293 MiB**。
- **確保元の census**（M4b lane の collector、標本）：それらの page に生き残る最大の確保元は source text だった
  （`TextSnapshot::new` ← `PreparedSourceFile::new` ← `StagedGraph::admit_source`、136.5 MiB）。

いまは `tsc_store::SharedText` が text を持つ。
- text は process に 1 つの mimalloc v3 の heap（`mi_heap_new`、消さない）に置く。v3 の first-class heap は、自分の確保を
  他と別の page に置き、どの thread からも確保・解放できる。
- `SharedText` は 16 B の handle（data pointer と長さを inline）。block は参照数と text の bytes を 1 つに持つので、読みは
  `Arc<str>` と同じ費用、確保は text ごとに 1 回（以前と同じ）。持ち主は file ごと（snapshot と位置の索引）で、node ごとではない。
- 作り手はみな text heap へ写す（`TextSnapshot::new`、`from_shared_text`、`from_store`、LSP の編集、package.json、一時の text）。
  置き換えた text は、最後の持ち主が手放すと解放される。
- v3 でないときや heap を作れないときは、呼んだ thread の既定の heap に置く。libmimalloc-sys の `v2` では compile できない。
  Miri では `std::alloc`、WASM では diagnostics が `Arc<str>` の代わりを使う（store は 64 bit 専用）。
- `custom/runGC` は text heap も collect する（呼んだ thread の page と arena の purge）。
- allocator の方針（判定 C の F3、統合役が受け入れ）：library の利用者は自分の global allocator のままで、source の text だけが
  mimalloc の text heap に入る（`crates/cli/src/main.rs` と `docs/rust-api.md` に記した）。
- adopt が解放する parse の出力は page ごと空き、bind と check がそれを再利用する。

**関門**：重い card（上限 ≥ 50 MiB）なので、関門を全部通した。ledger の gate row は PR の前に main に入れた（`d81090bca`）。
- 判定：A（件数 × 費用）GO WITH CONDITIONS、B（静的）FEASIBLE WITH CONDITIONS、C（反証）STANDS WITH FIXES。統合役の決定は GO WITH CONDITIONS。
- 帯（A と C の共通部分）：VS Code 1 thread −205〜−215 MiB・命令数 0〜−0.10 G、4/4 −10〜−40、既定 −35〜−72、他の 7 project の
  1 thread 合計 −70〜−120 MiB、WASM 0。
- 条件はすべて満たした（PR の表）。表現は 1 つで text ごとに確保 1 回・16 B、v2 の guard と既定の heap への fallback、heap は
  消さない、WASM の代わり、LSP の編集の test、runGC、census などの道具への注記。

**計測**：昼（13:04〜13:14）、lane は止めた。load 1.6〜4.1、`nice -n 20`、基準と交互に実行、`main-ba4240a55` 比。

| | 基準 | この PR |
|---|---:|---:|
| VS Code 1 thread の footprint（9 回の median） | 3,188.3 MiB | **2,976.9 MiB（−211.4）** |
| 命令数（low mode の median） | 130.631 G | 130.649 G（+18 M） |
| VS Code 4/4 の footprint（10 組の対の median） | — | **−33.7 MiB**（wall +0.22%、95% −0.09〜+0.53%） |
| VS Code 既定の footprint（10 組） | — | **−60.6 MiB**（wall −0.02%、95% −0.97〜+0.92%） |

- 1 thread の footprint は範囲が重ならない（基準 3,184.4〜3,194.1、card 2,971.2〜2,978.3）。閾値の pilot の窓でも、同じ
  binary は −211.9。
- 他 7 project の 1 thread：footprint は next −27、zod −26、playwright −16、effect −14、vue −8、ts −2、hono −2 MiB（合計 −95）。
  命令数は −0.01〜+0.12%、emit の構成では −0.19〜+0.09%。
- **hono は 4 worker 以上で増える**（4/4 と既定は 6 組の ABBA）：4/4 で +1.5 MiB、既定で +2.2 MiB（6 組とも正）。他の project は減る。
  - 原因：worker ごとに、小さな size の bin ごとに text heap の page を開く。既定の 10 worker では、hono の小さな text 241 個
    （0.6 MiB）が ≈ 206 の page を開き、最初の touch が ≈ 3 MiB になる。
  - 8 KiB 以下を既定の heap に残す pilot（`3aed63493`）は hono を +0.1 にした。しかし VS Code 1 thread で 25 MiB を失い（−186.8）、
    関門の 190 MiB の線を下回ったので採らない（統合役の判断）。残る差として記録する。
- D1：(workers, checkers) = (1, 4) は −192.3 MiB、(4, 1) は −69.3 MiB。4 つの checker は空いた page を吸わない。並列の読み込み
  （大きい順の read-ahead）が、基準でも text を parse の出力から既に分けている。そのため product metric（4/4 と既定）の得は
  34 と 61 MiB にとどまる。
- VS Code 1 thread は ≈ 2,977 MiB・≈ 130.6 G になり、努力目標の線（≤ 3,100 MiB・≤ 132 G）を越える。WASM は 0。

**記録。** PR [#907](https://github.com/kazhiramatsu/tsc-rs/pull/907) → merge `7ce379bc6`（2026-11-06。`ba4240a55` の上、最終 head `e64b5c622`）。hosted run
[42163943251](https://github.com/kazhiramatsu/tsc-rs/actions/runs/42163943251)：全 job pass。local（hosted がしないことだけ、`e64b5c622`）：
- crate の test：store 52（text heap への所属、thread をまたぐ持ち主の数、空の text の address、等しさ）、diagnostics 86、
  program 693、syntax 279、checker 1,864、compiler 246（live program の 20 回の編集で text heap が有界）。
- cli の test（binary が text heap を使う）、fmt、clippy（`-D warnings`）、release build。
- WASM：`cargo check -p tsc-rs-diagnostics --target wasm32-unknown-unknown` は、main と同じ既存の error 1 つだけ。
- 8 case で出力（exit code・stdout・stderr）が基準と一致。Identifiers・Symbols・Types・Instantiations も 8 project とも一致。
- `full` 構成の出力の木：7 project の 16,164 file（source map と declaration map 8,081 を含む）が byte 単位で一致。
- fingerprint oracle：`main-d4d9d0364.tsv` に対して 143,190 行のうち差は 0。
- 記録：`gate-frag/`（dossier、判定 A・B・C、battery・窓・M1・t8k の log）。census などの道具への注記は `INSTRUMENTS-text-heap.md`。
- 計測は上の通り。

## module の解決表を tsgo の形にする（source ごとの範囲を持つ 1 本の行の vector）（branch `perf/ts71-resolution-table`、gate を通った 9 枚目、memory の 7 枚目、FE-R1）

`ResolutionTable` は `BTreeMap<ResolutionKey, ModuleResolution>` を持っていた。
- 1 項目 368 B が B-tree の node（4 KiB の葉、5 KiB の内部 node）に入っていた。
- 項目ごとに、import する file の canonical path と、解決先の 2 つの綴りの複製を持っていた。
- VS Code では 112,780 項目：項目 41.5 MB と文字列 36.2 MB、それに node がかかる。
- checker の解決は、引くたびに source の path を key に複製し、長い path を比べながら木を下りていた。

いまは tsgo の `resolvedModules[path][specifier, mode]` の形：
- 1 本の vector に 64 B の行を並べる。順は (import する source の canonical path、specifier、mode) で、旧 key と同じ。source ごとの
  行の範囲は `SourceFileId` で引く。
- 行が持つもの：source の id、specifier、mode、解決先（`SourceFileId`、読み込まない解決先は box にした path）、1 byte の拡張子、
  flag、intern した `u32` の package id、まれな事実の box。box は中身があるときだけ作る。
- 読み込んだ解決先の path は、source 自身の path と表示か canonical の綴りが違うときだけ持つ。
- 引くとき（`PreparedProgram::module_resolution`）は、source の範囲を借用した specifier で二分探索し、確保しない。
- path で引くとき（language service と API）は path の持ち主の source に限り、alias を拒む。API は tsgo の API と同じく、alias を
  まず持ち主の source に直す。
- builder は source id ごとに行を集め（加えるときに衝突を検出し、alias の key を拒む）、vector を直接作る。build の後に表は
  変わらない。
- 実装は Codex lane（fer1、phase 1）で、Opus が検証・統合した。

**関門**：重い card（上限 ≥ 50 MiB）なので、関門を全部通した。ledger の gate row は PR の前に main に入れた（`d10f79206`）。
- 判定：A GO WITH CONDITIONS、B FEASIBLE WITH CONDITIONS、C STANDS WITH FIXES、決定は GO。予想は VS Code footprint −40〜−65 MiB
  （中心 −45）、命令数 −0.3〜−0.7 G。
- 条件はすべて満たした（PR の表）：
  - 行 ≤ 88 B（64 B を assert）、引くときに確保しない（確保の test）。
  - package id の数を数えた（6 project で 10% 超なので `u32` に intern）。
  - alias の key を拒む、表示と canonical の両方で比べる、box は中身があるときだけ。
  - `with_replaced_source` で path が変わらないことを assert、順序は旧と同じ、表は変わらない。
  - tsgo と比べる fixture（大文字小文字の違う import、package の重複除去の redirect、symlink（preserveSymlinks の有無）、alias
    path での API の読み）は byte 単位で一致。
- heap walk（前後、VS Code 1 thread、既定の heap）：
  - live は 4 時点とも −93.1 MiB、committed は check の後で −73.3 MiB。
  - B-tree の 4 KiB の葉は adopt の後 62.5 MiB → 4 MiB 未満、5 KiB の内部 node は 22.7 → 6.5 MiB。
  - 80〜128 B の級は page が残り、解放した文字列は空き slot のまま残る（≈ +20 MiB、phase 2 の分）。

**計測**：昼（15:02〜15:14）、lane は止めた。load 1.0〜3.4、`nice -n 20`、交互に実行。基準は `main-7ce379bc6`（#907 の merge から
build、`text-heap-final` と同じ bytes）。

| | 基準 | この PR |
|---|---:|---:|
| VS Code 1 thread の footprint（7 回の median） | 2,976.8 MiB | **2,900.3 MiB（−76.5）** |
| 命令数（low mode の median） | 130.756 G | **129.459 G（−1,296 M、−0.99%）** |
| VS Code 4/4（10 組の対の median） | — | **−67.6 MiB**、−1,446 M、wall **−1.01%**（95% −1.53〜−0.48%） |
| VS Code 既定（10 組） | — | **−89.3 MiB**、−1,454 M、wall **−1.27%**（95% −1.96〜−0.63%） |

- 同じ mode の組は −1,281〜−1,346 M。予想（−0.3〜−0.7 G）との差を数で示す（診断用 build、phase の mark ごとの命令数と表を引く回数、
  VS Code 1 thread、3 回ずつ）：
  - **check で −1.39 G**：check は表を **594,290 回**引く（load は 0 回）ので、1 回あたり ≈ **2,330 命令**減る。
    - 旧：source の canonical path を key に複製し（確保・≈ 100 byte の copy・解放）、`BTreeMap` を ≈ 5 段下りる。各段は 11 key
      までの線形走査で、共通の前置きの長い path（VS Code の path は 60〜120 byte）を ≈ 25〜30 回比べる。
    - 新：source の範囲を読み、その ≈ 11 行を specifier で二分探索する（短い比較 ≈ 4 回、確保なし）。
  - **load で +0.13 G**：表を作るときに 112,780 行を source の canonical path で並べ替える。解決の walk は誤差の内（low mode
    で 19.75 対 19.73 G）。
  - **page fault**：reclaim が −4,899（16 KiB で ≈ 76.5 MiB、footprint の減りと同じ）、kernel の仕事で ≈ −0.02〜−0.03 G。
  - 予想は ≈ 0.45 M 回 × path の比較 ≈ 25 回と見積もっていた。実際は 594 k 回で、共通の前置きの長い path の比較と複製が
    見積もりより高い。
- 他 7 project の 1 thread：footprint は next −5、zod −6、playwright −4、effect +1、vue −2、ts −1、hono −1 MiB、命令数は
  −0.42〜0.00%。
- 4/4 と既定で増える project はない（中央値 +2 MiB 超かつ 6 組中 5 組以上が正、はない）。emit の構成は −0.20〜−0.04%。
- FE-T1 との相互作用（判定 C の M4、VS Code 1 thread、`main-ba4240a55` の 3,188.4 MiB 比）：FE-T1 −212.2、FE-R1 だけ −62.1、
  両方 −288.1 MiB。I = −13.8 MiB で、和より大きく減る（15 MiB の線の内）。
- 予想（−40〜−65 MiB、−0.3〜−0.7 G）を両方の軸で上回る。VS Code 1 thread は ≈ 2,900 MiB・≈ 129.5 G になる。

**記録。** PR [#908](https://github.com/kazhiramatsu/tsc-rs/pull/908) → merge `d640a5852`（2026-11-06。`7ce379bc6` の上、最終 head `73665ae47`）。hosted run
[42173257254](https://github.com/kazhiramatsu/tsc-rs/actions/runs/42173257254)：全 job pass。local（hosted がしないことだけ、`73665ae47`）：
- crate の test：program 702（新しい表の test 8 を含む）、compiler 247（引くときの確保が 0 の test を含む）、checker 1,864、
  api 333（alias での API の読み）、ls 256、project 235。
- fmt、clippy（`-D warnings`）、release build。
- 8 case で出力（exit code・stdout・stderr）が基準と一致。Identifiers・Symbols・Types・Instantiations も 8 project とも一致。
- fingerprint oracle：143,190 行のうち差は 0。
- conformance（4 checker）：12,829 case。
  - #906 の 4 checker の report に対して、15,339 の構成すべてが同じ。
  - 1 checker の report に対しては、記録済みの 4 構成だけが違う。
- lane の tsgo の fixture（大文字小文字、package の重複除去、symlink 2 種、alias の API）：CLI の出力と API の JSON が byte 単位で一致。
- 記録：`gate-resolutions/`（dossier、census、card の battery・heap walk・窓の log）。lane の記録は `fer1-lane-record/`。
- 計測は上の通り。

## 推論の identity probe：conditional 型の分岐を小さな cache から読み、identity の判定を relation の枠の前で返す（branch `perf/ts71-identity-probes`、gate を通った 10 枚目）

tsgo の `inferFromMatchingTypes` は、union の要素を `isTypeOrBaseIdenticalTo` で組にして調べる。VS Code ではこの probe の大半が
1 つの file（`src/vs/platform/agentHost/test/node/mapSessionEvents.test.ts`）から来る。この file は project の関係判定の 35% を持ち、
4 checker で最も遅い shard に入る。
- その file を縮めた project（`tsconfig.bench-mse.json`）では、identity の probe 3.43 M が conditional 型と他の種類を組にし、その
  96% は正規化の直後の identity の flag の判定で終わる。
- conditional 型の正規化は両方の分岐を読み、分岐は hash map の links にあるので 13.9 M 回引く。1 probe あたり ≈ 2,300 命令で、
  tsrs は ≈ 1,330。

いまは次の 3 つ（どれも tsgo の意味のまま）：
1. `getTrue/FalseTypeFromConditionalType` の links の前に、checker ごとの direct-mapped cache（4,096 slot × 12 B）を置く。分岐は
   一度書かれたら変わらず巻き戻されない（setter は speculation を無視し、呼び手は getter の miss の道だけ）ので、hit は links の答えと同じ。
2. identity で片側だけが conditional 型の組は、`checkTypeRelatedTo` が予算（`relation_count`）を読んだ直後、枠を作る前に、正規化の後の
   判定を返す：同じ型なら真、flag が違えば偽、同じ singleton の flag なら真。これらの判定は cache を書かず報告もしない。
   - 落ちた組は完全な検査で同じ型に正規化し直す。他の組（conditional 同士、object）は正規化 1 回のまま。最初の広い版は zod +0.75%
     だったので狭めた。
   - 飛ばすのは line profiler の counter（`profile_ops[OP_RELATIONS]`）だけで、出力には出ない。
3. `getNormalizedType` は簡約できる種類（index・indexed access・conditional）を先に見る。先の腕はこれらに当てはまらない。

**関門**：重い card（VS Code ≈ −1.4 G、0.3 G 超）なので lean な関門を通した。gate row は PR の前に main に入れた（`b1f9af8d1`）。
- 判定：A GO WITH CONDITIONS、B EQUIVALENT WITH CONDITIONS、C STANDS WITH FIXES、決定は GO WITH CONDITIONS。
- 帯：VS Code 1 thread −1.3〜−1.5 G（計上 −1.4）、縮めた project −11.5%、4/4 と既定の wall ≈ −1.5%（下限 −1.0 / −0.7%）、footprint 0。
- 持続性：VS Code の節約の ≈ 93%（1.42 G のうち 1.33）は test file 1 つなので、「VS Code、test file 1 つ」として固定の corpus
  vscode `29b68000` 上で計上する。
- B が見つけた既存の差（conditional.rs:664-665 と 679-680 は `any` を調べる前に restrictive instantiation を作るが、tsgo は作らない）
  は別の忠実性の card に回す。

**計測**：夕方（19:23〜19:33）、lane は止めた。load 1.5〜4.1、`nice -n 20`、交互に実行。基準は `main-d640a5852`（#908 の merge から
build）。

| | 基準 | この PR |
|---|---:|---:|
| VS Code 1 thread の命令数（low mode の median） | 129.488 G | **128.136 G（−1,352 M、−1.04%）** |
| VS Code 1 thread の footprint（9 回の median） | 2,898.3 MiB | 2,899.7 MiB（+1.4、水準の内） |
| VS Code 4/4（10 組の対） | — | wall **−1.59%**（平均、95% −2.16〜−1.01%）、−1,376 M、footprint −0.2 MiB（計上 0） |
| VS Code 既定（10 組） | — | wall **−1.47%**（平均、95% −2.06〜−0.88%）、−1,337 M、footprint は下 |

- 同じ mode の組は −1,280〜−1,390 M。
- 縮めた project（1 thread）：11.54 → **10.32 G（−10.6%）**、wall 0.54 → 0.47 s。計上した −11.5% より 0.9 point 小さい。
  - abc′ の試作（fast path は `isTypeRelatedTo` の中）は −11.55% だった。
  - abc′ との差は test を除けば条件 1 の移動（予算を読んだ後、`checkTypeRelatedTo` の頭）だけなので、tsgo と同じ順の代価は
    ここで ≈ 1 point（≈ 0.12 G）。VS Code 1 thread は abc′ の −1,423 M に対して −1,352 M で、帯の内。
- 4/4 の出力：vscode/stable（66,899 byte）と zod（5,533 byte）が基準と byte 単位で一致。
- **VS Code 既定の footprint**：最初の 10 組で中央値 +3.5 MiB（10 組中 9 組が正）と規則 C(b) に当たった。統合役の規則で 10 組を
  足し、20 組で見た。
  - 組（MiB）：+16.8 +7.0 +1.7 +4.4 +2.5 +1.7 +0.2 +20.7 −0.1 +31.4 −15.0 +10.2 +15.2 +47.9 +1.1 −22.9 +31.0 −18.0
    +17.9 +22.6。
  - 中央値 **+5.7 MiB**、20 組中 16 組が正（平均 +8.8）。繰り返したので、範囲の決まった差として記録する（3,424 MiB の 0.17%）。
  - 仕組み（時間の差）：card が持つのは checker ごとに 48 KiB（既定で ≈ 0.2 MiB）。−1.4 G の大半は最も遅い shard を縮めるので、
    4 つの shard の終わりが近づき、peak の重なりが増える。
  - 反証：4/4 は −0.2 MiB（10 組中 4 組が正）、(4,1) は −5.8、(1,4) は −1.6、1 thread は +1.4（水準の内）。
- M2：(worker, checker) = (1, 4) で footprint −1.6 MiB、(4, 1) で −5.8 MiB。命令数はどちらも ≈ −1.3 G。
- 他 7 project の 1 thread：命令数 −0.08〜0.00%（effect −0.08、next −0.07、vue −0.05、ts −0.03、hono −0.03、playwright
  −0.02、zod 0.00）、footprint −1〜+2 MiB（next +2、zod −1、他は 0）。
- 4/4 と既定で、他 7 project に規則に当たるもの（中央値 +2 MiB 超かつ 6 組中 5 組以上が正）はない。最大は既定の zod +3.0、
  4/4 の next +2.6、既定の playwright +2.6 MiB で、どれも 6 組中 4 組が正。
- 縮めた project はまだ tsrs より重い（10.32 対 7.18 G）。probe の道の残り（union の要素の複製、残る組の relation の枠）と
  推論自身の構造が次で、lane rel1 と sl1 の数で値を付ける。

**記録。** PR [#909](https://github.com/kazhiramatsu/tsc-rs/pull/909) → merge `49b73e4ba`（2026-11-06。`d640a5852` の上、最終 head `e98b98dab`）。hosted run
[42204006355](https://github.com/kazhiramatsu/tsc-rs/actions/runs/42204006355)：全 job pass。local（hosted がしないことだけ、`e98b98dab`）：
- crate の test：checker 1,869（新しい test 5 を含む：identity の判定 4 種を両方向で完全な枠と比べるもの 4 つ、slot を共有する
  2 つの conditional 型 1 つ）、compiler 247。
- fmt、clippy（`-D warnings`）、release build。
- 9 case（8 project と縮めた project）で出力（exit code・stdout・stderr）が基準と一致。Identifiers・Symbols・Types・
  Instantiations も 9 case とも一致。4 worker / 4 checker でも vscode/stable と zod の出力が byte 単位で一致。
- fingerprint oracle：143,190 行のうち差は 0。
- conformance（4 checker）：12,829 case。
  - #906 の 4 checker の report に対して、15,339 の構成すべてが同じ。
  - 1 checker の report に対しては、記録済みの 4 構成だけが違う。
- 記録：`gate-rel/`（dossier、判定 A・B・C、profile、battery・窓・既定の再計測の log）。
- 計測は上の通り。

## path の bytes を共有する：`CanonicalPath` と `ProgramPath` を tsgo の Go string と同じく複製で bytes を写さない形にする（branch `perf/ts71-shared-paths`、gate を通った 11 枚目、tsrs の track の最後の card）

tsgo の `tspath.Path` は Go の string で、複製は header だけを写す。tsc-rs では path の複製が毎回 bytes を確保して写していた。
VS Code の load で同じ path が何度も複製される：
- module の request・row・map の key（`source.path().canonical().clone()`、112,822 個ずつ）；
- parse task の target（`resolved_file().clone()`、import の出現ごとに 1 つ、118,942 個 × 表示と canonical の 2 本）；
- directory の cache から渡す host（`host.clone()`）。

いまは `tsc_diagnostics::SharedJsString`（`Arc<[u8]>` の後ろの不変な canonical WTF-8）を `CanonicalPath` と `ProgramPath` の表示が持つ。
- 複製は数を 1 つ増やすだけで、bytes を写さない。呼び出し側は変えていない。
- 等価・順序・hash・`Debug` は `JsString` と同じ bytes 上のもの。`Hash` は `JsString` と同じく bytes を書くので、hash 値も map の反復順も
  変わらない。等価はまず同じ bytes を共有しているかを見る。
- 表示が canonical と同じ bytes なら、表示は canonical の bytes を共有する（macOS では canonical を小文字にするので起きない）。
- `into_js_string()` と `into_parts()` は `JsString` へ写す（呼ぶのは test だけ）。15 B 以下の path も `Arc` を確保する。

**関門**：命令数で重い card（VS Code ≈ −0.42 G、0.3 G 超）なので lean な関門を通した。gate row は PR の前に main に入れた（`2d7822024`）。
- 判定：A GO WITH CONDITIONS、B EQUIVALENT WITH CONDITIONS、C STANDS WITH FIXES、決定は GO WITH CONDITIONS。
- 帯（C の最も低く守れる値）：VS Code 1 thread −0.40〜−0.45 G、footprint −6〜−9 MiB；他 7 project は next −0.15%、playwright −0.19%、
  hono −0.13%、effect −0.07%、zod・vue・ts は 0；check は 0〜+0.1 G の追加。
- 件数（診断用 build、phase の mark ごとの命令数と allocation の呼び出し数、3 回ずつ、数は決定的）：allocation の呼び出し **−1,682,447
  （−2.3%）**（parse task −1,273,341、graph の finish −127,652、依存の symlink −30,559、load program −248,695、check −2,143）。load は
  ≈ −0.47 G で 1 回 ≈ 280 命令（確保・≈ 100 byte の複写・解放）。check の +0.04〜0.06 G は allocation の変化を伴わない（code の配置）。
- memory の上限は 32.3 MiB だったが、footprint は −8.5 MiB。heap の page は peak で −50.0 MiB（80〜128 B の page 343/481/828/505 →
  98/205/802/267）なのに、同じ時点の footprint は −13.2：
  - purge を各段の境で強いると −37.1 になる。≈ 24 MiB は mimalloc の purge の遅れ（解放した arena の memory は 10 ms の遅延の後、
    遅延して purge されるまで footprint に残る。推定で、追跡はしていない）；
  - ≈ 13 MiB は歩いた page の外にあり、場所を特定していない。

**計測**：夜（22:03〜22:20）、lane は止めた。`nice -n 20`、交互に実行。基準は `main-d640a5852`。

| | 基準 | この PR |
|---|---:|---:|
| VS Code 1 thread の命令数（low mode の median） | 129.535 G | **129.119 G（−417 M）** |
| VS Code 1 thread の footprint（9 回の median） | 2,898.6 MiB | **2,886.0 MiB（−12.6）** |
| VS Code 4/4（10 組の対） | — | footprint **−37.5 MiB**（95% −49.1〜−21.3、10 組とも負）、−493 M、wall の対の median −0.07 s（−2.62%） |
| VS Code 既定（10 組） | — | footprint **−47.7 MiB**（95% −57.2〜−35.9、10 組とも負）、−463 M、wall −0.04 s（−1.24%） |

- 同じ mode の組は −396〜−465 M。thread が多いほど heap の節約が footprint に出る（peak での purge の遅れの比重が小さい）。
- 他 7 project の 1 thread（5 組）：命令数は next −0.21%、zod −0.23%、playwright −0.25%、effect −0.09%、vue −0.09%、hono −0.19%、
  ts +0.03%。footprint は −6〜0 MiB。
- 4/4 と既定の footprint：next −3.9 / −9.5、zod −5.8 / −3.9、playwright −4.7 / −6.2、effect −0.7 / −1.6、vue −2.4 / −2.6、
  ts +0.5 / +0.3、hono +0.5 / −0.4 MiB。規則（中央値 +2 MiB 超かつ 6 組中 5 組以上が正）に当たるものはない。
- 出力：M1 の全実行の stdout の sha256 が 18 の構成と project で基準と同じ。4/4 で vscode/stable と zod は byte 単位で一致。
- thread の下の loader（VS Code、phase trace、6 組、card − 基準）：(4,1) と既定で read-ahead（並列）−18.4 / −10.3 ms、parse task −15.1 / −14.2、
  graph の finish −20.2 / −19.8、load program −50.2 / −42.9 ms。共有の数えで競合の費用は見えない。check は対ごとに ±100 ms の揺れ。

**記録。** PR [#910](https://github.com/kazhiramatsu/tsc-rs/pull/910) → merge `c50adfcdf`（2026-11-06。`b1f9af8d1` の上、最終 head `4a79b2302`）。hosted run
[42224481118](https://github.com/kazhiramatsu/tsc-rs/actions/runs/42224481118)：全 job pass。local（hosted がしないことだけ、`4a79b2302`）：
- crate の test：diagnostics 87（`SharedJsString` と `JsString` の bytes・hash の入力・`Debug`・順序、surrogate の sample、複製の共有）、
  program 705（表示と canonical の共有、`into_parts`・`into_js_string`、hash と順序）、compiler 247、api 333、ls 256、project 235。
- fmt、clippy（`-D warnings`）、release build。16 B の const assert。
- 9 case（8 project と縮めた project）で出力（exit code・stdout・stderr）が基準と一致。Identifiers・Symbols・Types・
  Instantiations も 9 case とも一致。M1 の全実行の stdout の sha256 も一致。
- fingerprint oracle：143,190 行のうち差は 0。
- conformance（4 checker）：12,829 case。
  - #906 の 4 checker の report に対して、15,339 の構成すべてが同じ。
  - 1 checker の report に対しては、記録済みの 4 構成だけが違う。
- 記録：`gate-paths/`（dossier、判定 A・B・C、battery・窓・phase trace の log）と `gate-mem/`（上限、account、heap walk と footprint）。
- 計測は上の通り。

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

**計測の作法の訂正 2（2026-11-06、Opus の発見、統合役が採用）。**
- **mode の場所**：VS Code の命令数の ≈ 0.9 G の mode は、compiler の code ではなく、config と parse の file system の仕事にある。
  - 診断 build（phase の mark ごとに process の retired instructions を出す。merge しない）の 32 run で、高い mode の run はどれも
    「config: file names」が +0.18 G（1.25 対 1.07 G）、「load: parse tasks」が ≈ +0.7 G（20.2 対 19.5 G）。低い mode の run は両方
    低い。
  - check の区間の幅は 25〜41 M しかない。
- **温めの実験**：計測の直前に config の file（`--listFilesOnly` の 10,196 file）を読んで温めた。low の水準は ≈ 0.1 G 下がるが、重い
  job の直後の 1〜2 process は温めても高い。
- **規則**：以後、VS Code の serial は warm-up を 2 回にする（`serial.py --warmup 2`）。記録は
  `target/claude-tools/handover/perf-tsrs/gate-nodeflow/README.md`。

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
| 1 checker の CLI 経路の後始末（M1 の finalization 1,234 M の内訳。Opus の sub-scope 計測）：`check_snapshot_serially` の末尾で `CheckerState` を drop（580.8 M；CLI の noEmit 呼び出し `lib.rs:5310` が `leak_state=false` 固定、shard 実行 `:4857` は `checkers.leaks_states()`）＋ 戻り道で `ProgramSnapshot` と store を drop（342.6 M；shard 経路は `:4874` で forget）；per-file 診断 loop 2 回 64+55 M、assemble 48、render 65、session 32、run_config 21 | #890（Opus、card 6：1 checker の CLI 経路でも shard と同じ leaked-states の予算で leak、watch/API は drop のまま；unit test は text snapshot の Arc 数で leak 3 / drop 1 を pin） | VS Code 1 thread −0.92 G（−0.67%）、next ≈ −0.2 G、zod ≈ −0.15 G；既定と 4 thread は不変（shard）、single thread/WASM の行はそのまま得る | 静音、親 `3f336c690` 比：VS Code **−0.67%**（−923 M；5 round −0.66%、wall 8.69 → 8.58 s）、zod −0.91%（−169 M）、effect −0.75%、hono −0.75%、playwright −0.70%、next −0.60%（−168 M）、ts −0.32%、**vue 0.00%**；exit と出力 size 一致、peak ±2 MB；c4 = 既知 4 件・15,339 一致；count は（drop/leak の前に記録されるので）構造上不変 | 予想どおり（vue 以外）。vue は declaration diagnostics の経路（`check_program_with_authoritative_modules_at_for_emit_with_checkers` が `leak_at_end=false` のまま）を通るので効かない → 同じ 1 flag の follow-up card。もう 1 つの follow-up：2 回目の per-file loop の再計算（≈ −55 M） |
| request 収集の dynamic import 探索を tsgo の条件に（text に `/**` か `require` があるだけで file 全体を walk していた `module_requests.rs:657-662` を、expanded では tsgo と同じ「PossiblyContainsDynamicImport か JS file」に。M1 の request 収集 10 倍の原因）（Opus、card 7） | #891 | N = 無駄に walk した file：VS Code 4,843 file / 89.8 MB、c = 16 M/MB（M1 の行から）±30% → VS Code −1.44 G（−1.05%）、playwright −1.9%、effect −1.5%、zod −1.25%、ts −1.1%、vue −0.9%、next −0.8%、hono −1.4% | 静音、親 `03623ed12` 比：VS Code **−0.85%**（−1,165 M；5 round −0.82〜−0.85%、wall 8.73 → 8.67 s）、playwright −1.20%、hono −1.10%、ts −1.03%、effect −0.86%、zod −0.75%、vue −0.49%、next −0.47%；exit と出力 size 一致、peak ±2 MB；`--traceResolution` が 8 project で親と byte 一致（VS Code 922,795 行）、contract test（TS file の JSDoc @import と require() は request を出さず、static import の slice は fail-closed のまま）、program test 691；c4 = 既知 4 件・15,339 一致 | VS Code・ts・hono は帯の中（13 M/MB）、他 5 つは 35〜44% 下（9〜10 M/MB）：walk の単価は codebase の byte あたりの node 密度で変わる → 以後の walk 系の予想は project ごとの単価をその project の台帳の行から取る |
| #890 の follow-up：declaration diagnostics の経路（`check_program_with_authoritative_modules_at_for_emit_with_checkers`）と inline driver の経路でも同じ leaked-states の予算で CheckerState と snapshot を leak（1 checker の CLI emit 経路も snapshot を残す）。unit test は Arc 数で #890 の noEmit 経路と同値を pin（Opus、card 6b） | #892 | vue −66 M（帯 −53〜−77 M、−0.44%；c = #890 の footprint 1 MB あたりの teardown 0.18〜0.26 M）、他 7 つは 0 | 静音、親 `67f853a38`（#890 の head）比：**vue −0.38%**（5 round −56.7 M、3 round −57.8 M）、他 7 つは 0 ± 2.8 M（VS Code +1.9 M）；footprint ±2 MiB；出力 8/8 一致、hono の emit 経路 748 file byte 一致、vue の統計は親と同じ；c4 = 既知 4 件・15,339 一致 | 帯の中（低めの側：vue の peak には declaration diagnostics の一時 object も含まれる）。これで finalization の teardown（#890 + #892）は閉じた |
| parse の最初の card：式中の型引数の speculation に tsgo の事前条件（`tryParseTypeArgumentsInExpression`、parser.go:5332：JS でないこと、次が `<` か `<<` であることを mark() の前に確認）。tsc-rs は Strada の形で member expression の末尾ごと・`?.` の後・super で二重に `try_parse` していた（M8 の parse 対応表：speculation は 1 回の費用は同じで回数が 10.92 M 対 2.42 M）（Opus、card 8） | #893 | N = counter で正確（飛ばせる呼び出し数、project 別）、c = 80〜115 命令/回（profile の時間比から）→ VS Code −0.75〜−1.0 G | 静音、親 `c234075b2` 比：**VS Code −1.10 G（−0.81%）**（5 round；3 round は −1.39 G だが雑音大）、ts −0.70%、playwright −0.45%、effect −0.34%、vue −0.33%、zod −0.26%、next −0.24%、hono −0.17%；footprint ±3 MiB；syntax test 276、diffout 8/8、fingerprint oracle（`main-b8a54e83a.tsv`）143,190 行で差 0；期待される木は tsgo `aa814927` で確認、`<<` 以外の token は `re_scan_less_than_token` で不変（出力は構造上不変） | **8 project すべて帯の外（良い側）**：N は正確で c が一貫して 124〜138 命令/回（25〜30% の過小）。教訓：profile の時間比から出す単価は、分岐予測の効く直線的な（IPC の高い）save/restore の code では下限にしかならない → 次の parse の価格付けは命令数で重みづけした単価か micro-batch の計測で |
| tsgo の `getLiteralTypeFromProperties` の cache を移植（`propertiesTypes`：(type, include, includeOrigin) の key、speculation 区分 C、tsgo の origin 条件 `includeOrigin && classOrRef \|\| alias != nil`（Go の優先順位：alias 型は origin を必ず持つ）、含まれる index info だけを push。M2 item 2 の Index 型超過の原因；忠実性も：alias の `keyof` 表示が tsgo と同じに）（Opus、card 3） | #888 | origin を作る繰り返しだけを価格付け：N = next 9,034 / zod 4,711 / VS Code 8,380、c = 1〜5 K → zod/next −0.03〜−0.15% | 静音、B0 比：**next −2.79%**（−0.81 G）、**zod −2.69%**（−0.52 G）、playwright −0.99%、vue −0.67%、effect −0.66%、ts −0.27%、VS Code −0.26%（−0.36 G、5 round で ±0.02 G）、hono −0.06%；peak ±3 MB；出力 8/8 一致（stderr 含む）、c4 = 既知 4 件・15,339 一致；count は Types だけが動き（next −9,009、VS Code −8,376、zod −4,685）、origin の Index 型は zod 885・next 1,269 で tsgo と一致；unit test は tsgo の 2 文言と Types 96−1（tsgo だけが作る `resolvingDefaultType` の番兵：全 program で定数 −1） | **帯の外（良い側、1 桁上）**。原因は母集団と単価の両方：N は origin を作る繰り返しではなく cache hit の全部（next 16,354、zod 8,819、VS Code 12,380 = 1.5〜1.9 倍、effect/vue は ≈ 7 倍）、c は 1〜5 K ではなく大きな型で hit あたり 24〜58 K（property 1 つの走査 ≈ 1.2〜2.1 K；hit の型は next で平均 38 property）。計測 build の counter で確定（`m2/card3-counting-hooks.diff`） |
| 同じ alias 同士の variance 比較の経路を tsgo の条件（source の alias 型引数が非空）に（`structural.rs:527`；tsc-rs は import した全既定 alias の空 list でも通り、既定値を埋めて variance で関係付け、variance の測定まで行っていた。witness：`Box` 対 `Box<number>` の TS2322 で中間行が落ちる。M2 の None-vs-empty 監査の 1 件目）（Opus、card 4、忠実性） | #889 | 0 ± 0.05%（忠実性。N = 飛ばす関係：ts 61（Visitor）、vue 8、hono 5、next 3、VS Code 3、playwright 1） | 静音、B0 比：8 project すべて雑音の範囲（VS Code −0.002% … playwright +0.026%；next は 3 round +0.075% → 5 round −0.01% で雑音）；peak ±4 MB；出力 8/8 一致（stderr 含む）、c4 = 既知 4 件・15,339 一致；count は site 別に帰属（既定値の埋め込み回避 ts −61、alias-variance の marker instantiation 回避 next Types −2 / Symbols −2、構造比較で増える member の instantiation ts +39+39、hono +2+2）；unit test は tsgo の 3 行 chain と Types 83−1・Instantiations 5 に pin | 予想どおり（0）。価値は忠実性：診断の chain が tsgo と同じに |
| tsgo #64553 の移植：`inferFromTypes` の alias 分岐と matching-references 分岐を `invokeOnce` で（visited key の重複排除と循環の優先度、either-side 条件。tsgo `c8e9b7d25`「Cache inferences made from type arguments」の残り半分；#841 は InferenceKey だけだった。M2 の仕事量監査で発見：vue の Instantiations +10.1% のうち 64%）（Opus） | #885 | N = vue で 35,841 の instantiation 減、c = 1〜3 K 命令 → vue −0.2〜−0.7%、他 0 | 静音、親 `6e20adc06` 比、warm-up + 3 round（VS Code と effect は 5 round）：vue **−0.32%**（15.61 → 15.56 G、footprint −6 MB）、effect **+0.46%**（16.14 → 16.21 G）、VS Code +0.06%、zod +0.13%、playwright +0.09%、ts +0.01%、hono −0.04%、next −0.12%。count：vue Instantiations 605,322 → 569,481（tsgo 549,756）、診断 byte 一致、c4 = 既知 4 件・15,339 一致、diffout 8/8（stderr 含む） | vue は帯の中、effect は帯の外（費用）。原因：予想は消えた instantiation だけを価格付けし、`invokeOnce` の帳簿（effect で呼び出し 270 K → 389 K、深さ guard の stack step 0.83 M → 1.33 M、step ごとに再帰 identity を再計算）を数えていなかった。tsgo 自身の algorithm なので忠実性の価格として着地。follow-up の候補 card（価格つき、未着手）：inference stack の entry の再帰 identity を最初に必要な scan で lazy に cache（≈ 100 命令/step：effect −0.1 G、VS Code −0.1〜−0.2 G；relater の isDeeplyNestedType の stack も同形）。vue の残り +19.7K（union の member list +13.7K、outer 引数の mapping +6.2K）は次の M2 項目 |
| `is_non_generic_top_level_type` を tsgo の空判定に（alias の型引数 list が空なら非 generic。tsc-rs は (Some, None) だけを見ていたので、引数を書かずに使う全既定の generic alias の union を使うたびに instantiate していた。M2 item 1b：vue の残り +19.7K の原因）（Opus） | #886 | N = 32,627 × c = 1〜2 K（#885 の vue の率）→ vue −0.2〜−0.4%、他 0 | 静音、親 `22c755af5` 比、warm-up + 3 round：vue **−0.65%**（15.17 → 15.07 G、全 round 同じ）、他 −0.02〜+0.03%、footprint ±1 MB。count：vue 569,481 → 548,963（tsgo 549,756）、zod −459、next −42、hono −7（変わった property は全部 tsgo と一致か tsgo 側へ）；出力 8/8 一致（stderr 含む）、c4 = 既知 4 件・15,339 一致 | 帯の外（良い側）：消えた expansion は型と symbol の生成も止める（vue Types −906、Symbols −2,755）ので instantiation あたり ≈ 3 K。監査の手がかり（別項目）：`type_order.rs:497` は先頭の型が alias 引数を持つ時だけ引数 list を比べるが tsgo の `compareTypeNames` は常に比べる → 「list なし」表現では union の member 順が tsgo から外れる（playwright −5,856 の説明） |
| B0 からの累積（anchor の再計測、2026-11-05 14:27〜14:34、統合者 tsc-rs-4e）：main `eb65df82f` = B0 + #885・#886・#887・#888・#889・#884・#890・#891・#892・#893（+ docs） | — | 各 card の実測の和：VS Code −2.37%（#888 −0.26・#884 +0.16・#890 −0.67・#891 −0.85・#893 −0.81・#885 +0.06、他 0）、zod −4.5%、next −4.2%、playwright −3.3%、vue −2.8%、ts −2.3%、effect −2.2%、hono −2.1% | warm-up + 3 round の中央値、同じ run の B0 binary 比（静音ではない：Spotlight の索引付けで load 4.6）：VS Code 138.05 → **135.36 G（−1.95%）**、footprint 3,520 → **3,446 MiB（−74）**。VS Code は 0.9 G の二峰性（B0 138.05 / 138.96 / 137.91、main 135.38 / 135.36 / 134.50）で −1.95〜−2.47%。zod −4.40%、next −4.39%、playwright −2.95%、effect −2.32%、ts −2.13%、vue −2.02%、hono −1.73%；exit と出力 size は全行一致。tsrs 86.40 G / 2,008 MiB → 命令数 **1.57 倍**（同じ run の B0 1.60）、peak **1.72 倍**（1.75）。frontier（main、同じ負荷なので wall は参考値）：VS Code wall 既定 2.36 s 対 tsrs 1.94 s（1.22 倍）、4 thread 3.03 対 2.01 s（1.51 倍）、1 thread 9.15 対 5.85 s（1.56 倍）；peak 既定 4,218 対 2,314 MiB、4 thread 3,899 対 2,306、1 thread 3,446 対 2,008；4 checker の重複 +9.99 G（+7.4%、B0 +9.31 G）対 tsrs +6.77 G | VS Code は和より 0.4 pt 小さいが二峰性の幅の中（5 round は次の静音窓で）。vue は和 −2.8 に対し −2.0（#885・#886・#890 の vue 行の親がそれぞれ別）。2026-11-18 の線（VS Code 1 thread ≤ 132 G / ≤ 3,100 MiB）まで **−3.4 G・−346 MiB**；4 thread と既定の wall は 1.5 倍・1.2 倍の差。**訂正（15:20）**：code が同じ `167f488a8` を静音 7 round で測ると **134.69 G**（Opus の #895 の base、pair が揃う low mode）→ B0 比 **−2.43%**、card の和 −2.37% と一致；3 round の 135.36 G は +0.9 G の mode を拾っていた。#894・#895 の後は ≈ 134.26 G、線まで **≈ −2.3 G**（memory −346 MiB は再現）。以後、gate に使う VS Code の値は 7 round の low-mode 中央値（pair を示す） |
| top-level await を parse 中に見つける（tsgo の `statementHasAwaitIdentifier`・`possibleAwaitSpans`・span 駆動の `reparseTopLevelAwait`、parser.go:516；tsc-rs は parse 後に module の文を全部歩いていた（tsc 6.0.3 の `containsPossibleTopLevelAwait` の射影、node ごとに子の `Vec`）。effect の Identifiers −293 を閉じる。fixture `await-flag-boundary.json` の 285 件を tsgo `aa814927` の parser の dump で作り直し（6.0.3 の期待と 81 件違い、tsgo と 285/285 一致））（Opus、card 9） | #894 | 消す walk：N = 訪問 node（VS Code 954,174、effect 90,645、playwright 78,953、zod 48,439 ほか）× c（walk を飛ばした pilot で実測、45〜360 命令/node）→ VS Code −0.17 G（帯 −0.10〜−0.34）、zod −16 M、effect −15 M、playwright −10 M、ts −7 M、他 ≈ 0；足す追跡は identifier あたり 1〜2 命令と見積もり | 静音、親 `fec97ea85` 比（VS Code 7 round）：VS Code **−102.7 M（−0.08%）**（pair −130 / −107 / −87 / −130 / −76 と 0.9 G の mode の +791 / +960）、playwright −13.5 M、effect −4.7、ts −2.0、zod −1.4、hono −0.2、vue +5.3、next +6.7；footprint ±2 MB；Identifiers は 8 project で tsgo と一致（effect 604,929 → 605,222）、Symbols/Types/Instantiations 不変、出力 8/8 一致、fingerprint の差 104 行（52 入力：effect 49 + tsgo の回帰 case 3）は parse/bind hash だけ | VS Code は帯の下端、小さい project は帯の外（節約が小さい側）。walk の値段は pilot どおり、外れは足す側：binding identifier と property name ごとの save と token-kind の比較で identifier あたり ≈ 10〜15 命令（VS Code ≈ 0.08 G）。忠実性の card として着地。教訓：足す側の構文ごとの仕事も、消す側と同じ精度（N × c）で値段を付ける |
| 完成した node の子の parent を、record を encode する前に payload から設定（tsgo の `finishNode` と同じ所で。生成済みの `try_visit_each_child` で 1 回だけ歩き、`attach_children` の decode・scratch `Vec`・二度目の pass をなくす；JSDoc の finish も同じ経路）（Opus、card 10） | #895 | knock-out batch で attach 全体は 81.3 命令/node だが、新しい経路を同じ batch で先に測った差は 20.5/node（v1 12.9、v2 = inline + bounds check 1 回の `get_mut`、unsafe なし）× N（parse.nodes：VS Code 17.02 M）→ VS Code −0.35 G（帯 −0.26〜−0.44）、next −47 M、zod −41 M、playwright −39 M、effect −32 M、vue −24 M、ts −22 M、hono −8 M | 静音、親 `167f488a8` 比（VS Code 7 round）：VS Code **−333 M（−0.25%）**（pair −364 / −409 / −368 / −263 / −389 / −349 と 0.9 G の mode の +384）、next −53.4 M、playwright −42.8、effect −33.8、zod −28.8、ts −24.0、vue −14.9、hono +1.0；footprint 不変；出力 8/8 一致、統計 8 project 一致、fingerprint 143,190 行で差 0（parent は bind hash に入る）、syntax 276（debug の parent 照合つき）、clippy rc 0 | VS Code・next・playwright・effect・ts は帯の中、zod は帯の端、vue・hono は小さい側。足す側を先に測る手順（card 9 の教訓）が効いた：消す側の 81/node だけなら 1.38 G と見積もり 4 倍外れていた。zod・vue・hono が小さいのは c を vscode/src の batch だけで値段付けしたため（project ごとの node の構成の差） |
| **関門** card 11：parser の識別子の interning を安く（P2：16 byte 以下の名前は 2 word の正確な key + 乗算 hash + 3 word の比較、worker thread ごとの cache を parser が file ごとに 1 回取って `&mut` で渡す、miss は `names::intern`）（Opus、2026-11-05 の関門の最初の適用） | —（実装前） | 上限：escape 177 命令/呼び出し × parser の 5.87 M = 1.04 G（全呼び出し 12.2 M では ≈ 2.2 G）。足す側：試作 P2 は −78.5/呼び出し（11 round の batch、555 万呼び出しで id 完全一致；P1（caller 所有の cache）は −31.7、P2 を 2^16 slot で −92） | 判定 A（回数 × 経路の費用）0.46 G（0.38〜0.53）、B（静的な経路 + 逆 assemble + tsgo/tsrs：両者は parse で intern せず lookup ごとに hash、前倒しは識別子あたり 20〜30 だけ）0.47 G（0.35〜0.56）、反証 C は ≈ 0.40 G（0.30〜0.50）に弱めた：呼び出しごとの TLS アクセスは ≈ 0.28 G で関門を通らない | **GO（設計の条件つき）**：予想 VS Code −0.40〜−0.47 G（帯 −0.30〜−0.53）。code の前にこの設計そのものの CLI A/B pilot（静音 7 round の対）。記録 `target/claude-tools/handover/perf-tsrs/gate-card11/VERDICTS.md` |
| **関門** inline-finish：parser の finish 経路（`finish_node_with_flags`・`alloc_and_finish`・`attach_children_of_data`・`try_visit_each_child`・`alloc_node`・`push_node`・`encode`・`encode_payload`、v2 は 148 か所から呼ぶ `finish_node_data` も）を `#[inline(always)]` にし、呼び出し側で NodeData の variant が決まるので attach と encode の汎用 dispatch が 1 本に畳まれる（生成部分は `generate_nodes.py` 経由、論理の変更なし）（Opus） | —（実装前） | 上限 = 正味（実 parser の batch、vscode/src 16.26 M node/round、3 rep）：v1 −66.9 命令/node、v2 −92.7/node（v2 − v1 = −25.8、rep の差 ±0.1）；code +48.8 KB | 判定 A 1.07〜1.13 G（0.77〜1.22）、B ≈ 1.1 G（0.9〜1.2；逆 assemble で Identifier 88 → 29、CallExpression ≈ 209 → 59、PropertyAccess ≈ 166 → 71、token 86 → 47）、反証 C は「成立」（≈ 1.0〜1.1 G、下限 ≈ 0.9） | **GO（v2、CLI での確認の後）**：予想 VS Code −1.58 G（帯 −1.2〜−1.75、#895 の上なので #895 が先）。未解決の製品 risk：PGO build が既にこの連鎖を inline している可能性（必要なら `pgo_build.py` で対を測る）。記録 `target/claude-tools/handover/perf-tsrs/gate-inline/VERDICTS.md` |
| **関門** buffer pool の試作：store で解放した list の mapping を thread ごとに ≤ 8 個再利用し、publication の後の parse が新しい page を fault せずに済むように（`crates/store` のみ、試作で merge しない）（Opus） | —（試作） | 上限：B0 の peak 以外の fault ≈ 33 K × 8.4 K 命令（実測した fault 1 回の費用）≈ 0.28 G | CLI pilot（静音、#895 の head 比、VS Code 9 round）：VS Code −0.03%（−46 M、pair −939〜+820 = 雑音）、footprint +18 MB（他 +6〜+20 MB）、出力 8/8 一致。同じ窓の発見：+0.9 G の雑音 mode は page reclaim では分かれない（mode の run も 252.6〜253.7 K で同じ）→ fault 由来ではなく、mimalloc の purge 仮説も支持されない | **PARK**（命令数の効果なし、memory は悪化）。推定理由：publication と解放が連続する parse の間に挟まらない、または buffer の多くが 64 KB の mapping 閾値の下。追わない |
| parser の finish 経路の inline 化（関門を通した最初の card：`finish_node_with_flags`・`alloc_and_finish`・`attach_children_of_data`・`try_visit_each_child`・`alloc_node`・`push_node`・`encode`・`encode_payload` と 148 か所から呼ぶ `finish_node_data` に `#[inline(always)]`。生成部分は `generate_nodes.py` 経由、論理の変更なし）（Opus） | #896 | 関門：実 parser の batch で −92.7 命令/node（v1 −66.9）、判定 A 1.07〜1.13 G・B ≈ 1.1 G・反証 C 成立（下限 ≈ 0.9）→ VS Code −1.58 G（帯 −1.2〜−1.75）；CLI pilot（#895 の head 比）−1.58 G | 静音、親 `d4d9d0364` 比（VS Code 9 round）：VS Code 134.11 → **132.56 G（−1.16%、−1,550 M）**（pair −1,321〜−1,598 と mode の −685）、playwright −1.23%、hono −1.06%、ts −0.99%、zod −0.87%、effect −0.84%、next −0.68%、vue −0.67%（pair ±6 M）；CPU 8.45 → 8.37 s、footprint ±3 MiB、code +97.7 KB；出力 8/8 一致、統計 8 project 一致、fingerprint は新しい baseline `main-d4d9d0364.tsv`（main 自身の binary で生成）に対し 0 行 | 帯の中央付近。関門の予想・CLI pilot・card の実測が揃った：上限 → 足す側 → 複数の判定 → CLI pilot の手順が初めて全段で働いた例。2026-11-18 の努力目標（≤ 132 G）まで ≈ −0.56 G |
| parser の識別子の名前を、parse の間だけ持つ name cache で引く（card 11：worker thread の cache を parse の開始で取り `&mut` で持ち、終われば返す（入れ子の parse は自分の cache）；16 byte 以下は 2 word の正確な key + 乗算 hash + 3 word 比較、miss は共有の `escaped_id`；hit も `names.escape.calls` を数えて counting build と比較可能）（Opus） | #897 | 関門：上限 1.04 G、試作 P2 −78.5/呼び出し、判定 A 0.46・B 0.47・反証 C 0.40 G → VS Code −0.40〜−0.47 G（帯 −0.30〜−0.53）；設計どおりの CLI pilot は low mode で −0.53 G | 静音、親 `d4d9d0364` 比（`perf11/modes.py`）：VS Code low-mode 中央値 134.093 → **133.558 G（−535 M、−0.40%）**（main 6 low / 3 high、card 5 / 4；high mode −560 M、同 mode の pair −515 / −522 / −537 / −565 M；全 run の中央値は mode の混ざりで −633 M）、playwright −0.46%、hono −0.43%、ts −0.42%、zod −0.40%、effect −0.39%、next −0.31%、vue −0.22%；footprint +0〜+4 MB（cache ≈ 0.5 MB/thread）；出力 8/8 一致、統計一致、fingerprint 0 行（`main-d4d9d0364.tsv`）、types 60・syntax 277、clippy rc 0 | 帯の上端（予想より少し大きい節約）。CLI pilot（−527 M）とも一致。計測の規則を追加：VS Code は binary ごとに mode を分け（各 binary の最低 run + 0.45 G で分割）、low-mode の中央値と同 mode の pair を示す（9 round でも mode の偏りで全体の中央値は歪む） |
| **関門** names stay names：symbol 表の text key の検索（`NameKey` for `&str` は検索ごとに完全な intern）を、定数 key は site ごとに 1 度だけ intern する `known_name!` に（各々 `InternalSymbolName` の定数に test で pin）、union の合成は持っている `EscapedName` をそのまま渡す（`structural.rs` の再 intern と String の確保をやめる）；id と出力は不変（Opus） | —（実装前） | 上限：4,413,088 回 × c 141.5（実測：text 166.0 対 `known_name!` 24.5/回）= 0.62 G。**数え直し**：track_caller の site 集計は 1 回を 2 度数えていた（`NameKey` の impl と `from_escaped_text` の両方が同じ caller を記録；合計 8.83 M = 2 × counter）→ 半分にして読む | 判定 A（回数）0.37 G（全定数 site）+ union の修正 0.14、下限 0.31；B（静的経路 + 逆 assemble）helper 未帰属で 0.26 G（二重計数を発見）；反証 C は ≈ 0.34 G（union 込み ≈ 0.47）に弱めた。決着の counting build（helper にも track_caller、半分にして）：定数 key 2,341,148 回 = 0.331 G（直接 1.65 M、helper 経由 0.69 M）、union 合成の再 intern 1,177,333 回 = +0.167 G；届かないもの：contextual の index text 0.55 M、engine の is_known_property 0.29 M；後回し：`globals.rs:193` 0.25 M（≈ 60 caller の API 変更） | **GO（1 枚の card）**：予想 VS Code −0.45〜−0.50 G（帯 −0.35〜−0.60）。checker の変更なので local の検査に `--checkers 4` の conformance を含める。教訓：site 集計は合計を counter と突き合わせてから N に使う。記録 `target/claude-tools/handover/perf-tsrs/textkey/VERDICTS.md` |
| names stay names：symbol 表の定数 key の検索を site ごとに 1 度だけ intern する `known_name!` に（`InternalSymbolName` の定数 26 か所 + helper の literal key 16 か所、各 key は test で `from_escaped_text`/`internal` と同値に pin）、union の合成は持っている `EscapedName` を渡す（`create_union_or_intersection_property`）；id と出力は不変（Opus） | #898 | 関門（数え直し後）：定数 key 2.34 M 回 × 141.5 = 0.331 G + union 合成の再 intern 1.18 M 回 = 0.167 G → VS Code −0.45〜−0.50 G（帯 −0.35〜−0.60） | 静音、親 `d4d9d0364` 比（`perf11/modes.py`）：VS Code low-mode 中央値 134.316 → **133.838 G（−478 M、−0.36%）**（main 7 low / 2 high、card 8 / 1；high mode −496 M、同 mode の pair −458〜−505）、vue −1.26%、playwright −0.70%、zod −0.63%、next −0.60%、hono −0.41%、ts −0.32%、effect −0.27%；footprint ±3 MiB；checker の変更なので c4：12,829 case・harness error 0、card 6b の c4 report と 15,339 構成すべて一致；test types 58・binder 80・checker 1,873、clippy rc 0、diffout 8/8、統計一致、fingerprint 0 行 | 帯の中（下端寄り）。関門の数え直し（site 集計の二重計数を counter で検出）がなければ 0.53〜0.62 G と見積もっていた。#896 + #897 + #898 が独立に足せれば VS Code low-mode ≈ 131.7 G（努力目標 ≤ 132 G の内側）：3 枚の merge 後に 9 round の累積で確かめる |
| **関門** bind：node ごとに呼ぶ binder の helper 9 個（`get_container_flags`・`is_potentially_executable_node`・`is_narrowable_reference`・`modifiers_of`・`get_syntactic_modifier_flags`・`name_field_of`・`get_name_of_declaration`・`get_escaped_text_of_identifier_or_literal`・`get_assignment_declaration_kind`）を `#[inline(always)]` に（v1）＋ tsgo の `bindChildren` に合わせ、到達可能な node で UNREACHABLE を消す tsc 6.0.3 の仕事をやめる（`binder.go:1645-1660`；flag の overlay は bind ごとに parse の flag から作り直すので死んだ仕事）（v3）（Opus、判定 B が忠実性の項目を発見） | —（実装前） | bind の batch（vscode/src の半分、8.49 M node・1.10 M symbol/round；bind = 645 命令/node、CLI の M1 は 546）：v1 −28〜−35/node、v2（次の段の helper 8 個）は 0 で捨て、v3 は v1 にさらに −12/node（rep の差 25 M）→ v3 は base 比 −41〜−44/node → CLI 予想 −0.59〜−0.70 G（帯 −0.50〜−0.75） | 判定 A 0.48 G（0.40〜0.55、v1）、B 0.48 G（0.37〜0.53、静的 21〜29/node；UNREACHABLE の消去は ≈ 0.28 G で inline と ≈ 0.11 G 重なる）、反証 C は「成立」0.41〜0.49 G（下限 ≈ 0.36、決着の条件：low-mode の差 ≤ −0.30 G・各 binary ≥ 3 low run・同 mode の pair すべて ≤ −0.30 G）。**CLI pilot**（静音、main `1cdae0b1b` 比、`modes.py`）：VS Code low-mode 133.228 → 132.522 G（**−706 M、−0.53%**、main 8/1・pilot 9/0、同 mode の pair −542〜−725）、playwright −0.68%、hono −0.67%、zod −0.47%、effect −0.44%、ts −0.44%、vue −0.38%、next −0.35%；CPU 同じか −0.01 s、footprint ±1 MB、diffout 8/8 | **GO（v3）**：C の条件をすべて満たし、予想の帯の上端。card は main（#896 込み）から：9 個の属性 + tsgo の `bindChildren`、到達可能な実行され得る node が bind 後に UNREACHABLE を持たないことを unit test で pin；binder の変更なので c4 と fingerprint を含める。注：窓ごとの漂い ≈ 0.4 G（同じ binary の low median が窓で違う）→ 累積の anchor は全点を 1 つの窓で測る |
| B0 からの累積 #2（anchor、2026-11-05 19:31〜19:38、統合者 tsc-rs-4e；**1 つの窓**で B0・main `d4d9d0364`（#893〜#895 まで）・main `0ab9da2a6`（+ #896・#897・#898）・tsrs を測る；VS Code 9 round、`perf11/modes.py` で mode を分類（tsc-rs の 3 binary は各 8 low / 1 high、tsrs 9 / 0）） | — | card の実測の和：#896 −1.55 G + #897 −0.53 G + #898 −0.48 G = −2.56 G | VS Code low-mode 中央値：B0 137.846 → `d4d9d0364` 133.808 G（−4.04 G、−2.93%）→ main **131.264 G（B0 比 −6.58 G、−4.77%）**；main − `d4d9d0364` = −2.54 G（同 mode の pair −2.54〜−2.73 G 相当、B0 比の pair −6.54〜−6.68 G）；footprint 3,520 → 3,450 MiB（−70）；tsrs 85.276 G / 2,006 MiB → 命令数 **1.54 倍**、peak 1.72 倍。他 7 project の B0 比：zod −6.42%、next −5.99%、playwright −5.71%、vue −4.30%、ts −4.08%、hono −4.00%、effect −3.94% | 3 枚は独立に足し合わさった（−2.54 対 和 −2.56 G）。**努力目標 ≤ 132 G の内側（131.26 G）**、約束 < 135 G は余裕。memory は 3,450 MiB で、約束 ≤ 3,300 MiB まで −150、努力目標 ≤ 3,100 MiB まで −350 MiB（M7 の棚卸しから card を選ぶ）。bind の card（関門 GO、CLI pilot −0.53%）が入れば ≈ 130.6 G |
| binder の node ごとの helper 9 個を inline し、`bindChildren` は tsgo と同じく到達可能な node の UNREACHABLE を消さない（`binder.go:1645-1660`；tsc 6.0.3 の死んだ仕事）（Opus、関門を通した card、v3） | #900 | 関門：batch で v3 −41〜−44 命令/node、判定 A・B 0.48 G、反証 C 成立 → VS Code −0.59〜−0.70 G（帯 −0.50〜−0.75）；CLI pilot −706 M | 静音、親 `9037c4ba9` 比（`modes.py`、VS Code 9 round：main 9 low / 0、card 8 / 1）：VS Code low-mode 131.935 → **131.232 G（−703 M、−0.53%）**（同 mode の pair −671〜−730）、playwright −0.72%、hono −0.69%、zod −0.52%、effect −0.44%、ts −0.44%、vue −0.37%、next −0.36%；CPU 同じか −0.01 s、footprint ±4 MB；binder 81（新しい UNREACHABLE の test 込み）・checker 1,873、clippy rc 0、diffout 8/8、統計一致、fingerprint 0 行（bind hash は binder の node flag を含む）、c4 15,339 構成一致 | 帯の上端、CLI pilot と一致。累積 #2 の 131.264 G に足すと main ≈ 130.56 G（low mode） |
| **関門** binder の node ごとの表（memory の card の最初の候補：dense な `NodeSymbolMap` と `node_flow` を publication で file ごとに compact な形（存在 bitmap + 64 bit word ごとの u32 rank + 詰めた値）に）（Opus） | —（値段付け） | VS Code の counting build（next は M7 の 15.6% / 40.0% と一致）：node_symbol 2,181,337 / 16,880,227 slot = 12.9%、読み 19.24 M；node_flow 8,669,416 = 51.4%、読み 5.03 M；dense は各 16.88 M × 4 B = 67.5 MB → compact で node_symbol 11.9 MB（−55.6）、node_flow 37.4 MB（−30） | 足す側（実測した fill の合成 micro-benchmark）：読み 1 回 +8.44 命令（12.9%）/ +12.63（51.4%）（dense ≈ 2.0、compact 10.4 / 14.7）→ node_symbol +0.162 G − fault の credit 0.029 = **−56 MB に対し ≈ +0.13 G**、両方で −86 MB / ≈ +0.18 G | **PARK（値段付きで保留）**：M7 の VS Code の棚卸し（next では footprint 770 MB のうち初期化済みの既知の live 573 MB、残り 197 MB が未説明）でより大きい memory の梃子があるかを見てから選ぶ。記録 `target/claude-tools/handover/perf-tsrs/m8-maps/README.md` |
| FE4 の内訳（Opus、朝の CLI profile 3 本の時間比、publication/adoption の根の下）：633 sample = run の 2.4% ≈ 3.8 G（M1 の行 4.12 G と整合）；**60% が `relocate_with_leases` 自身**（node の identity の書き換え：record ごとの `publish_mapped` + `map_identity_words`）≈ 2.3 G（Codex の leaf adoption 2.36 G と一致）、memmove 9.5%、`validate_canonical_case` 5.5%、`add_module_resolution` 4.9%、memcmp 4.7%、munmap 4.6% | —（値段付け） | 2.3 G ÷ ≈ 17 M node ≈ 135 命令/node（32 B の record の複製と数個の identity word の shift に対して）。`map_identity_words` は node ごとに `slot_spaces(kind)`・`payload_shape(kind)` を引き、≤ 3 の inline word（か tail）+ JSDoc word を closure で回し、word ごとに space の match・範囲 check・shift | 方針の変更なしの安い案：kind ごとの事前計算した記述子（identity word の mask + word ごとの space）と直線的な書き換え。135 → 50〜60/node なら ≈ 1.3〜1.4 G（FE4 の上限の半分以上、id は再現可能のまま）。relocation は直列の区間なので 4 thread・既定の wall にも効く見込み | **値段付けへ GO**（relocation の batch + 表駆動の pilot、関門の手順、CLI pilot では rs-w4c4・rs-def の wall も測る）。通れば FE4 の方針判断（再現性を手放すか）は不要になる見込み |
| **関門** relocation の表駆動の書き換え（FE4 の方針変更なしの代替案：node の種類ごとの定数表 `INLINE_IDENTITY_LAYOUTS[360]`（inline word ごとの 2 bit の space）と直線的な shift）（Opus） | —（試作） | relocation の batch（vscode/src 全 9,482 file、16.26 M node/round、publish − parse）：**109 命令/node**（1 round の冷えた run の 54 は誤り）→ CLI の上限 ≈ 1.86 G；knock-out：`map_identity_words` 78.5、parent shift 6.7、array-item shift 3.2、残り（record の複製・lease・buffer の解放）≈ 20 | 試作 v1（tail のない record だけ）：−20.5/node（安定）→ CLI ≈ 0.31〜0.35 G；v2（tail の record も）：−12.7/node で v1 より悪い（tail の経路の書き換えが汎用の経路より高い）。K1 の 78.5 は過大（関数を消すと publish の loop 全体が単純化された分も含む） | **PARK**（値段付き）：閾値 0.3 G すれすれ、0.5 G の基準に届かない。FE4 の代替にはならない（relocation 109/node のうち表で取れるのは ≈ 20；残りは record の複製と record ごとの仕事で、配置を変える FE4 でしか消えない）。記録 `target/claude-tools/handover/perf-tsrs/m8-reloc/` |
| **関門** symbol の room を parse 時の上限で確保（memory の最初の card：binder の symbol 領域（region 3）は file ごとに node 数ぶんの room を取り、使うのは 13.4%（10,270 room、16.88 M slot に 2,270,113 symbol）→ 各 room の端の page が dirty。parse の `alloc_node` で kind ごとの symbol 重みを数え、export の双子・parameter property の member・dotted namespace・JS の Binary/Call を足した上限 c0（+3/file）で room を確保；溢れたら既存の relocation の経路）（Opus） | —（実装前） | 上限：region 3 は mincore で 178.4 MiB（page-touch の模型 `sim_rooms.py` が完全に再現）、正確な room なら 69.3 MiB。c0 + 2 の模型で 74.8 MiB・溢れ 0（−103.6 MiB）。最初の仮説（store の解放範囲のその場の 0 埋め）は counting build で否定（CLI は store の範囲を一度も解放しない） | **試作**（p51 `58ac74f04`、main `d613e1bb8` 比、VS Code warm-up 1 + 9）：**footprint 3,449.9 → 3,332.7 MiB（−117.2 MiB）**、page reclaim −7,517 ≈ −117.5 MiB；命令数 low-mode −47 M（置換検定 p = 0.0013、bootstrap 95% [−105, −15] M）；溢れは 7 project で 0、vue で 1（TS の expando）；diffout 8/8、統計一致。判定 A：条件つき GO（−114.4 MiB：region 3 −103.3、persistent links の index page −6.5〜−7.3、merged_symbol_bits −3.4）、B：健全（全 alloc site を対応付け；不足は TS expando・JS の @overload・dotted JSDoc typedef と、本番の呼び出し元がない incremental reparse）、反証 C：修正つきで成立（false sharing は否定） | **GO（修正 3 点つき）**：(1) TS の expando 候補を数える（≈ +2 MiB、vue の溢れ → 0）、(2) 複製した subtree を持つ tree（incremental）は従来どおり node 数を確保、(3) test（expando の正確な一致、強制した溢れの relocation が直接の bind と同値）。予想 VS Code −110〜−117 MiB、命令数 −10〜−60 M（C の否定条件：footprint の変化が −50 MiB より小さい、または命令数 ≥ +100 M）。これで約束 ≤ 3,300 MiB まで ≈ −33 MiB |
| symbol の room を parse 時の上限で確保（memory の最初の card：parser が source ごとに binder の symbol の上限を数え（kind ごとの重み + export の双子・parameter property・dotted namespace・JS の Binary/Call・TS の expando、+3/file）、Program は node 数ではなくこの上限で symbol の room を予約；複製した subtree を持つ tree は従来どおり node 数、溢れは既存の relocation）（Opus、関門を通した card） | #901 | 関門：試作で VS Code footprint −117.2 MiB、命令数 −47 M → 予想 −110〜−117 MiB、−10〜−60 M（反証 C の否定条件：−50 MiB より小さい変化、または +100 M 以上） | 静音、親 `d613e1bb8` 比（VS Code 9 round × 2 回；2 回目は 8 low / 1 high で clean）：**footprint 3,449.9 → 3,333.4 MiB（−116.5 MiB）**（範囲は重ならず、page reclaim −7,455 = −116.5 MiB と一致）；命令数 low-mode −33 M（同 mode の pair −9〜−71 M）；**4 thread**：footprint 3,886.4 → 3,760.7 MiB（−125.7）、wall 2.95 → 2.96 s・CPU 10.51 → 10.57 s・命令数同じ；溢れは 8 project すべて 0（vue は expando の数え方で解消）；test syntax 278・binder 83・checker 1,873、clippy rc 0、diffout 8/8、統計一致、fingerprint 0 行、emit `full`（JS + .d.ts + map）の木が hono（748 file）と effect（1,984）で byte 一致、c4 15,339 構成一致（1 checker 比は既知の 4 件だけ） | 帯の中（上端寄り）、C の決着条件をすべて満たす。memory の約束 ≤ 3,300 MiB まで ≈ −33 MiB。最初の仮説（store の範囲のその場の 0 埋め）は否定され、測った page-touch の模型で原因（room の端の page）を特定した例 |
| **関門** binder の `node_symbol` の compact 化 V1（Program ごとの索引：存在 bitmap + rank + 値、document の compact form から program ごとに 1 回作る；flag の表は published の読み ≈ 52 M で対象外）（Opus） | —（試作） | counting build 7：node_symbol は published の読み 17.96 M（99.1% hit）・binder の get 1.28 M、埋まり 12.9%；変種 V1 ≈ −42 MiB / +0.17〜0.21 G、V2 −41 / +0.13〜0.16（node の lease に触る）、V3 −53 / +0.25；関門の基準：footprint ≤ −30 MiB、命令数 ≤ +0.25 G | 試作（main `d613e1bb8` 比、VS Code warm-up 1 + 9）：footprint −40.0 MiB（範囲は重ならず）だが**命令数 +855 M**（low mode、同 mode の pair 9 本すべて +759〜+902 M）。原因：索引は密（10,270 owner、cold path は 0 回 — guard 説は否定）、読み 1 回 ≈ 48 命令増（rank ≈ 15、残りは ≈ 180 か所の呼び出し元に inline された本体が推論を重くした分）；細い読み（word と rank の交互配置、unsafe の crate で unchecked、out of line）でも ≈ +0.25〜0.3 G | **PARK**（基準 +0.25 G を超える）。memory の約束までの残り ≈ 33 MiB は別の梃子（M7b の heap の内訳、F10 の flow）で。記録 `target/claude-tools/handover/perf-tsrs/gate-nodesym/PARKED.md` |
| **関門** F10a：flow node を 16 B に（flags・a・antecedent・c の 4 word）+ document ごとの正確な antecedent pool（binder 局所、global id と rebase は F10b に）（Opus） | —（試作、予備） | M7 の行（binder.flow_nodes 72.9 MiB、flow_antecedents 3.8 / 10.6）から予想 ≈ −43 MiB、命令数 0 ± 0.1 G；関門の基準：footprint ≤ −30 MiB（範囲が重ならない）、命令数 ±0.15 G、flow fingerprint 0 差、8/8 + c4 | 試作 v1（main `d613e1bb8` 比、VS Code warm-up 1 + 9）：−33.7 MiB、**+695 M**（汎用の `payload()`/`antecedents()` が flag を最大 4 回調べ、24 B の view を値で返す）；v2（kind ごとに特化した inline の reader）：**−34.1 MiB**（範囲は重ならず、reclaim −1,984）、**+237 M**（同 mode の pair 8 本すべて +140〜+291 M）、diffout 8/8・統計一致。残りの費用の場所は未特定（frozen pool の分岐を通る label の antecedent の読み、bind 時の staging、freeze ≈ 30 M が候補） | **PARK（予備として保持）**：±0.15 G の外。branch `m8/pilot-flow16` `f021f6c23` は card の検査（c4・fingerprint・test）を通せば使える状態。M7b の heap の内訳を見て判断し、F10b（global id と routing の除去：命令数の側）と合わせて 1 枚にする案も残す |
| F10b の値段付け（flow を store の領域に global id で置き、checker の `(file, FlowId)` の振り分けをなくす：命令数の側）と F10 全体の見込み（Opus） | —（値段付け） | counting build 8（main `000eac25c`、#901 込み、VS Code 1 thread）：`binder.file(file)` 経由の読み 11.58 M（flags 5.75・antecedent 4.05・payload_node 1.60・label list 0.18）、query arena の読み 120.2 M（node 76.8・payload_node 23.4・antecedent 20.0）、`reduce_label_overrides` の引き 4.00 M（label の読みごと、ほぼ常に空の map）、他の key 操作 1.56 M + `last_flow_node` の比較 6.75 M | 1 回あたりの差（code の経路からの推定）：振り分け −0.09〜−0.12 G、query arena を `store::get` に −0.12〜−0.36 G、空の override の引きを飛ばす −0.04〜−0.06 G、4 B の key −0.02 G → 総 −0.27〜−0.56 G、memo の rebase（+0.05〜0.07）を引いて **−0.20〜−0.51 G**。F10a の +0.24 G と、F10a の staging の解放（freeze 時の label ごとの Vec 0.7 M 回、−0.05〜0.07 G）を合わせて **F10 全体 ≈ −0.34〜+0.04 G、memory ≈ −34 MiB** | F10 全体なら ±0.15 G の基準に入る見込み → F10a + F10b を 1 枚の card として試作（関門）。F10a 単独の費用は F10b の土台の費用（オーナーの指摘どおり、後の最適化が効く）。記録 `target/claude-tools/handover/perf-tsrs/m8-flow/README.md` |
| B0 からの累積 #3（anchor、2026-11-06 00:50〜00:57、Opus が統合者の `anchor3.sh` で実行；**1 つの窓**で B0・main `0ab9da2a6`（anchor #2）・main `e84278c49`（+ #900・#901）・tsrs `tsrs-sym-nopgo` を測る；夜間のため lane は止めていない（命令数と footprint は process ごとの数）；VS Code 9 round、`perf11/modes.py`（B0 8 low / 1 high、`0ab9da2a6` 6 / 3、main 6 / 3、tsrs 8 / 1）） | — | card の実測の和：#900 −0.70 G + #901 −0.03 G = −0.73 G；footprint は #901 の −116.5 MiB | VS Code low-mode 中央値：B0 138.055 → `0ab9da2a6` 131.398 → main **130.626 G（B0 比 −7.43 G、−5.38%）**；main − `0ab9da2a6` = −0.77 G；footprint B0 3,520 → `0ab9da2a6` 3,450 → main **3,334 MiB（B0 比 −186）**；tsrs 85.444 G / 2,008 MiB → 命令数 **1.53 倍**、peak **1.66 倍**。他 7 project（main 対 tsrs：命令数、footprint）：ts 9.32 / 4.90 G（1.90 倍）・169 / 110 MiB、next 27.25 / 17.31（1.57）・712 / 452、zod 17.83 / 13.21（1.35）・681 / 440、effect 15.28 / 11.00（1.39）・506 / 328、hono 2.71 / 1.73（1.57）・107 / 71、vue 14.44 / 8.69（1.66）・286 / 182、playwright 12.28 / 7.55（1.63）・388 / 249；B0 比の命令数は zod −7.02%、playwright −6.39%、next −6.37%、hono −4.69%、vue −4.60%、effect −4.52%、ts −4.23%。log：`perf-plan-20261104/runs/b0cum/anchor3.log` | #900 と #901 は和どおり（−0.77 対 −0.73 G）。**努力目標 ≤ 132 G の内側（130.63 G）**、約束 < 135 G は余裕。memory は 3,334 MiB：約束 ≤ 3,300 MiB まで −34、努力目標 ≤ 3,100 MiB まで −234 MiB。tsrs との差は命令数 1.53 倍・peak 1.66 倍 |
| **関門** F10a‴（flow の記録 32 → 16 B、label の list は bind 中は per-thread scratch の linked cell（tsgo の FlowList）で、bind の終わりに file ごとの正確な大きさの pool 1 つへ凍結、`into_bind_data` で node と pool を正確な新しい確保へ移す（re-home））（Opus、夜間） | —（実装前） | 上限と仕組み：旧 flow 72.9 + label list 10.6 MiB。store 版（F10a+F10b、global id、領域 38/39）は命令数 −69/−86 M だが footprint −12 MiB：mincore で store はちょうど 40.3 MiB、heap は 83.5 のうち ≈ 56 しか戻らない（mimalloc は倍々の block を保ち、heap から抜いた byte は ≈ 0.7〜0.8 で戻る）→ heap 配置を選ぶ。pilot（`main-e84278c49` 比、VS Code 9 round、夜間で lane は止めていない、全 run が clean）：v2（per-label Vec）−34.8 MiB / +195〜+216 M、F10a′（cell）−30.6〜−30.7 / +29〜+75 M、F10a″（cell + 正確な pool）−31.7〜−32.2 / +98〜+107 M、**F10a‴（+ re-home）−36.1 MiB / +112〜+120 M** | 判定 A（件数 × 費用）GO WITH CONDITIONS（memory の model が全 variant に合う；v2 との差 ≈ 4 MiB は pool の倍々 1.3〜1.7 + 配置 2.5〜3 と予想 → re-home で確認）；B（静的）SOUND（全 kind で凍結後の読みが旧と同じ、cell は Vec の順序・重複除去・置換・0/1/多を再現、checker の読む前に必ず凍結；round-trip test・debug assert・doc を追加）；C（反証）STANDS WITH FIXES（4 MiB は実在、override の空 skip は ≈ 0 → 外した、凍結後の成長は release assert に）。fingerprint 0 行 / 143,190、diffout 8/8・統計一致 | **GO**（card は F10a‴）。予想：VS Code footprint −34〜−37 MiB（→ ≈ 3,297 MiB、**約束 ≤ 3,300 MiB の内側**）、命令数 +0.05〜+0.15 G。card の battery：binder/checker/syntax test、clippy、fingerprint、c4、diffout 8/8・統計、7 project、4 thread。F10b（global id）は store の配置が memory を返さないので保留（命令数の利得は別の形で） |
| **関門** Y1/S2：transient symbol の links を 28 → 20 B に（`declared_type` と `is_referenced` を transient index の sparse map へ移し、読みは専用の accessor 17 箇所）（Opus、統合役の割り当て 06:20） | —（実装前） | 上限：census（観測用 build、VS Code 1 thread）で transient record 5,885,636 のうち `declared_type` は 450、`is_referenced` は 163（`name_type` 310 K、他の field は 33〜92%）→ 8 B × 5.89 M = −44.9 MiB。memo の案（target と mapper を Symbol の末尾 2 語へ）は binder と checker が `unsafe` を禁じるので extras の id 化が先に要る → 同じ 8 B をこの 2 field で取る（Symbol と binder は変えない；transient symbol は単調で index は再利用されない）。pilot（`main-681fc89fc` 比、VS Code 9 round、warm-up 2、1 つの静かな窓、`gate-links20/window-ab.log`）：**A（20 B）footprint −45.9 MiB**（main の footprint の low mode 比。中央値の比 −49.5 は main の 2 水準の混ざり）/ **命令数 +40 M**（同じ mode の組 8 つで +29〜+87 M）；B（16 B、`name_type` も bit 付きの map へ）−67.4 MiB / **+1,059 M** → 不採用（mapped type の member の読みが多い）。出力は 8 project で同じ、統計も一致 | 判定 A（件数 × 費用）GO WITH CONDITIONS（≈ −45 MiB・+40 M で記帳；7 project の停止条件：命令数 > +0.15%、VS Code の low mode > +0.1%、zod・next・effect の節約が予想の半分未満）；B（静的）SOUND（Resolving を読む reader なし、writer はすべて map へ、記録の丸ごとの複写なし；小修正 3 つ：doc link、memory 報告の bucket 係数、まれな書き込みで dense の記録も伸ばし trace の数を同じに）；C（反証）STANDS WITH FIXES（複数 checker・speculation・Resolving・他 crate・panic の攻撃はすべて失敗；footprint は mode を揃えて ≈ −46 MiB で記帳、doc link）。修正は入れた | **GO**（card は A + 修正）。予想：VS Code footprint −45〜−50 MiB（#903 の後で ≈ 3,218 MiB）、命令数 +0.02〜+0.06 G；zod −13.5〜−14.6、next −10.0〜−11.2、effect −7.4〜−8.3 MiB、命令数は各 project 0〜+0.06%。card の battery：checker/api test、clippy、c4 と比較、diffout 8/8・統計、7 project、4 thread、emit |
| **関門（保留）** binder の `SymbolExtras` を 104 → 40 B（JS の assignment-declaration members の `IndexMap` を最初の entry で box に）（Opus） | —（pilot、保留） | 上限の見込み：VS Code の extras box 348,548（M7 の account）× 64 B（mimalloc の 112 → 48 B 級）≈ −21 MiB、命令数 ≈ 0。pilot（p53 `m8/pilot-extras-box` `8c38102b1`、`main-904` 比、VS Code 9 round・warm-up 2、小さな map の pilot と同じ窓、`gate-extras/after-det1.log`）：footprint **−2.6 MiB**（low 同士）、命令数 −87 M（low mode、組は −131〜+64 M）。出力 8/8 同じ・統計一致 | 予想の 1/8。live の extras box は M7 の数より少ないか、解放された 112 B 級の空きが他の確保に再利用されて footprint に出ない（未確認） | **保留**（card にしない；記録 `gate-extras/`） |
| DET-1：checker の割り当てを tsgo と同じにし、既定の出力を決定的に（所有者の決定 2026-11-05） | [#905](https://github.com/kazhiramatsu/tsc-rs/pull/905) | 既定の checker 8 → 4 で既定の wall は伸び peak は下がる；1 thread ≈ 0 | VS Code 既定 wall 2.32 → 3.28 s（+41%）・peak 3,975 → 3,669 MB；4 worker / 4 checker 3.15 → 3.45 s；1 thread 命令数 −9 M・footprint +5.4 MiB；出力は既定 5 回 × 8 project で tsgo と一致 | merge（所有者の判断：このまま merge、wall は r0 と命令数の削減で取り返す） |
| **関門** FE-R1：module の解決表を tsgo の形に（source ごとの範囲を持つ 1 本の行の vector、行は ≤ 88 B、読みは借用した specifier の二分探索、読み込んだ target は `SourceFileId` だけで path は redirect のときだけ box）（Opus、Codex lane で実装） | —（実装前） | 上限（観測用 build、VS Code 1 thread）：112,780 行、行の inline 39.6 MiB（368 B × 行。B-tree の node は数えていない：葉 11 行 ≈ 4,064 B → 4 KiB、内部 → 5 KiB、挿入順で 55〜72% の充填 → 実際は ≈ 57〜75 MiB）、文字列 34.5 MiB（key の source 11.8、読み込んだ target 18.4、specifier 4.2）。読み込んだ target の 108,666 のうち source 自身の path と違うのは 33（他 project は 0〜1）。checker は解決を cache せず、引くたびに path を複製して長い文字列を ≈ 25 回比べる（≈ 0.45 M 回） | 判定 A（件数 × 費用）GO WITH CONDITIONS（−45 MiB（帯 −35〜−70）、−0.4 G（帯 −0.3〜−0.7）。行 ≤ 88 B（96 B だと葉が 1,280 B 級）、引くときに key を確保しない、package id の数を測る）；B（静的）FEASIBLE WITH CONDITIONS（key の source は source 自身の path に限る（alias を拒む）、「違う」は display と canonical の両方で比べる、rare の box は中身があるときだけ、`with_replaced_source` で path 不変をassert、#905 の利用者も直す；順序は rank で旧と同じ、build の後に表は変わらない、どこにも直列化されない）；C（反証）STANDS WITH FIXES（−40〜−65 MiB。文字列の戻りは少ない（walk の高水位）、B-tree の node が本体、太い木を作ってから変換しない、API・LS の path からの読みに alias の guard） | **GO**（card は Codex lane で実装し、Opus が検証・統合）。予想：VS Code footprint −40〜−65 MiB（中心 −45）、命令数 −0.3〜−0.7 G；他 project は next −2.5〜−6、zod −2.5〜−5.5 MiB。条件：rare field の census、4 KiB/5 KiB と 80〜128 B 級の heap walk（前後）、tsgo と比べる test（大文字小文字の違う import、package の重複除去の redirect、symlink の target（preserveSymlinks の有無）、alias path での API の読み）、8 project の出力一致と serial |
| **試作（不採用）** r0：rayon の常駐 pool で SpawningPool を置き換える（P1）、binder の consume を bind の job の中で行う（P2）（Codex lane r0、計測は統合役の窓） | —（試作） | 予想なし（所有者の決定 2026-11-06 07:45「rayon でまず実装し、余地があれば自作」の最初の試作。survey：VS Code 4/4 の marked serial 7.7%、pool の外 13%（DET-1 以前の観測 build）） | VS Code（窓 11:31〜11:41、lane は停止、base は lane の `0f593a02a` の build、`binprov.py pair` 済み、warm-up 1 + 5 round）：P1 は既定の wall 3.325 → 3.439 s（**+3.4%**、5 pair すべて +0.08〜+0.18 s）、peak **+78 MiB**；4 worker / 4 checker 3.452 → 3.616 s（**+4.8%**）、peak **+54 MiB**、命令数 +0.95 G；1 thread は差なし。P2 は P1 比で wall +0.04〜+0.05 s、peak −11 MiB。他 7 project の既定と 4/4：P1 は wall +3〜+10%、peak +12〜+61 MiB。出力は VS Code の既定と 4/4 で同じ（lane は候補ごとに 40 例で一致）。同じ窓の tsrs：既定 1.99 s / 2,313 MiB、4 thread 2.12 s / 2,303 MiB、1 thread 6.10 s / 2,008 MiB；tsc-rs の base は 3.33 s / 3,654、3.45 s / 3,589、8.85 s / 3,271 | **不採用**。lane r0 は 11:43 に停止（P3 は未検証）、記録は `handover/perf-tsrs/r0-lane-record/`。**訂正（12:12 の確認、VS Code 4/4、3 round + phase trace 2 回）**：page の仮説は外れ（page reclaims は +4.2 k で ≈ 35 M・数 ms 分しかない）。遅れ +0.20 s はすべて check session の前段にあり、rayon の順序付き map に載せた並列の bind が 126 → 272〜276 ms（2.2 倍）、adopt の並列の書き換えが 52 → 106〜112 ms（2 倍）。読み込みの DAG（read-ahead 381〜388 → 368〜393 ms）と 4 本の shard の check は同じ。P1 の順序付き map の割り振りを直しても互角までで、rayon への置き換えそのものに wall の得はない。4/4 の差は check の中身（checker ごとの仕事）にある。**所有者の決定（2026-11-06 12:31）**：rayon も自前の scheduler（SCHED-1）も性能の項目から外し、今の pool を使い続ける。4 thread の wall は checker 1 本の仕事を減らして詰める（lane dup4：自分の file・重複・偏りの内訳）。steal（検査中の動的な割り当て）は出力の決定性とかかわるので所有者の判断として保留。直列部分の書き直しは trace で具体的な塊が見えたときだけ |
| **関門** FE-T1：source の text を parse の出力と別の allocator heap に置く（mimalloc v3 の first-class heap を 1 つ（`mi_heap_new`、process の間は消さない）。`tsc_store::SharedText` は 16 B（data pointer と長さを inline、参照数は text の block の先頭に置き、text ごとに確保は 1 回）。作り手はみな text heap へ copy する（LSP の編集・package.json・一時の text も）。v3 でないときや heap が作れないときは既定の heap に置き、WASM では `Arc<str>`）（Opus） | —（実装前） | 上限：heap walk（VS Code 1 thread、4 時点、`gate-frag/heap-walk4-vscode.txt`）。adopt が heap に置いた parse の出力 264 MiB を解放し、空き slot は 173 → 438 MiB になる。check の後（peak）にも 10〜64 KiB 級に 293 MiB が残る。M4b の確保地点 census（`gate-frag/m4b`）では、peak に生きている最大の地点が source の text（`TextSnapshot::new` ← `PreparedSourceFile::new` ← `StagedGraph::admit_source`、136.5 MiB）で、load の間に parse の出力と同じ size 級へ確保される。試作（thread-local の hint で text だけを別 heap に、`serial-textheap.*`）：footprint は main 比 −201.1 MiB、同じ binary で heap を切った場合との比で −210.4 MiB。pilot (a)（main `ba4240a55` 比、`window-pilot-a.log`）：1 thread −214.1 MiB・命令数 −79 M（low mode）、4/4 −27.0 MiB、既定 −57.4 MiB、他の 7 project −2〜−26 MiB | 判定 A（件数 × 費用）GO WITH CONDITIONS：1 thread −195〜−225 MiB・−0.02〜−0.10 G、4/4 −10〜−50、既定 −35〜−90、7 project 合計 −70〜−120 MiB。2 worker 以上では read-ahead が大きい順に parse するので、base でも text は parse の出力と既に分かれており、どの置き方でも 4 thread の得は戻らない（D1 で確かめる）。条件は `SharedText` = 16 B の assert と、mimalloc を library に入れることへの承認の記録。B（静的）FEASIBLE WITH CONDITIONS：表現は 1 つ（enum の分岐なし）、v2 では compile できない guard と version の確認、heap は消さない。WASM では diagnostics が store に依存しない。LSP の編集も `TextSnapshot::new` を通るので、編集を繰り返しても text heap が有界であることを test で示す。emit の構成も測り、runGC の範囲を文書にする。C（反証）STANDS WITH FIXES：1 thread −205〜−215（最低 −195）、4/4 は n = 4 では 0 と区別できない（最低 0）、既定 −35〜−72、WASM 0。修正：v3 でないときや NULL の heap では既定の heap に戻る、text ごとに確保は 1 回、runGC は text heap も collect する、census と heap walk に text heap を見せる、行は構成ごとに付ける、post-check の空き slot 293 MiB のうち ≈ 210 MiB をこの card が使ったと記す。M1：8 project を 4/4 と既定で 1 + 6 組の ABBA で測る | **GO WITH CONDITIONS**（統合役の決定 2026-11-06。card は Opus が実装）。帯（A と C の共通部分）：VS Code 1 thread −205〜−215 MiB・命令数 0〜−0.10 G、4/4 −10〜−40 MiB、既定 −35〜−72 MiB、他の 7 project は 1 thread で合計 −70〜−120 MiB、WASM 0。**product metric（4/4 と既定）の得は ≈ 20〜60 MiB にとどまり、この card が満たすのは努力目標の線（1 thread ≤ 3,100 MiB）である。**F3（allocator の方針）：`tsc_store` に mimalloc の text heap を入れることを統合役が受け入れた（library の利用者は自分の global allocator のままで、source の text だけが text heap に入る。所有者は覆せる）。止める条件：1 thread の得 < 180 MiB、命令数が VS Code で +0.10 G 超か 7 project で +0.10% 超、4/4 か既定が 10 MiB を超えて悪化、出力か統計の差、どれかの project の 4/4 か既定で対の中央値が +2 MiB 超かつ 6 組中 5 組以上が正。PR の前に M1 と D1/M2（(workers, checkers) = (1, 4) と (4, 1)）を測る。後続の断片化 card は、空き slot ≈ 83 MiB（293 − 210）から予想を立て直す |
| B0 からの累積 #4（anchor、2026-11-06 16:34〜16:44、統合者 tsc-rs-4e；**1 つの窓**、lane は停止、Opus は休止；binary はすべて `binprov.py check` 済み：B0 `c3485397a`・main `ba4240a55`（FE-T1 の前）・main `d640a5852`（+ #907 FE-T1・#908 FE-R1）・tsrs `tsrs-sym-nopgo`；VS Code は warm-up 2 + 9 round、全 run が low mode） | — | card の実測の和：#907 −211.4 MiB・+18 M、#908 −76.5 MiB・−1.30 G（相互作用 −13.8 MiB） | VS Code 1 thread：B0 138.271 G / 3,519.9 MiB → `ba4240a55` 130.69 G / 3,188.4 → main **129.43 G（B0 比 −8.84 G、−6.39%、pair −8.74〜−8.89 G）/ 2,898.3 MiB（−621.6、−17.7%）**；tsrs 85.51 G / 2,007.7 MiB → 命令数 **1.51 倍**、peak **1.44 倍**。他 7 project（main、B0 比の命令数、tsrs 比の命令数・footprint）：next 27.10 G（−6.7%、1.57・1.46）、zod 17.81（−6.9%、1.35・1.41）、playwright 12.25（−6.4%、1.63・1.42）、effect 15.22（−4.8%、1.39・1.45）、vue 14.38（−4.8%、1.66・1.47）、ts 9.27（−4.6%、1.90・1.45）、hono 2.71（−4.6%、1.57・1.41）。product の frontier（VS Code、warm-up 1 + 3）：既定 2.97 s / 3,431 MB 対 tsrs 1.85 s / 2,306（1.61 倍・1.49 倍）、4 worker / 4 checker 3.13 s / 3,413 対 4 thread 1.97 s / 2,304（1.59 倍・1.48 倍）、1 thread 8.29 s / 2,903 対 5.70 s / 2,008（1.45 倍・1.45 倍）。log：`handover/perf-tsrs/checkpoint-1111/anchor4.log` | **約束（< 135 G・≤ 3,300 MiB）も努力目標（≤ 132 G・≤ 3,100 MiB）も満たす**（VS Code 1 thread）。#907 と #908 は和どおり。tsrs との差は命令数 1.51 倍・peak 1.44 倍、4 thread の wall 1.59 倍：差の本体は check（checker 1 本の仕事）で、次の track は関係判定と推論の経路（lane rel1・Opus の profile） |
| **試作（不採用）** mimalloc の purge の遅延 10 → 100 ms（`main.rs` の `mi_option_set` の定数 1 つ；profile では madvise が tsrs 比 +101 sample ≈ 0.5%）（Opus の build `purge-100` `af27aebc3`、統合者の窓 16:51〜16:57、lane 停止、`binprov pair` 済み） | —（試作） | 命令数が少し減る、peak は未知 | VS Code 1 thread：footprint 2,898.3 → 2,898.9 MiB、page reclaims 同じ、命令数 low mode −40 M（同 mode の pair −379〜+434 M で雑音）；既定 wall 3.15 → 3.12 s・peak +5 MB・−0.17 G；4/4 3.38 → 3.40 s・peak +13 MB・−0.07 G；他 7 project の 1 thread は命令数 −0.03〜−0.15%・+0〜+3 MiB、既定と 4/4 の peak は next +17 / +25 MB、hono +6 / +5、vue +2 / +8、他は ±2 | **不採用**（10 ms のまま）。得る命令数が小さく、multi thread の peak が少し増える。所有者の判断を要する trade-off ではない |
| **関門** 推論の identity probe：conditional 型の分岐を小さな cache から読み、identity の判定を relation の枠の前で返す（`inferFromMatchingTypes` が union の要素を `isTypeOrBaseIdenticalTo` で組にする probe。(a) `getTrue/FalseTypeFromConditionalType` の links（hash map）の前に checker ごとの direct-mapped cache（4,096 slot × 12 B）、(b) identity で片側だけが conditional 型の組は、正規化の後の判定（同じ型 → 真、flag が違う → 偽、singleton → 真）を `checkTypeRelatedTo` の予算を読んだ直後・枠を作る前に返す、(c) `getNormalizedType` は簡約できる種類（index・indexed access・conditional）を先に見る）（Opus） | —（実装前） | 上限（数えた）：VS Code の mapSessionEvents.test.ts（project の関係判定の 35%、4 checker で最も遅い shard）を縮めた project `tsconfig.bench-mse.json` で、top-level の関係判定 3.65 M のうち identity が 3.49 M、片側が conditional 型のものが 3.43 M、その 96% は正規化の直後の identity の flag 判定で終わる（lane rel1）。呼び手の 80.7% は `infer_from_matching_types`。正規化ごとに conditional 型の両分岐を読むので、hash map を 13.9 M 回引く（`diag-count`）。1 probe あたり ≈ 2,300 命令（tsrs ≈ 1,330）。試作（main `d640a5852` 比）：縮めた project 11.52 → 10.19 G（−11.5%）、VS Code 1 thread −1,423 M（low mode）、4/4 と既定の wall −1.4〜−1.9%（abc の段階で測定）、出力と統計は 9 case とも同じ | 判定 A（件数 × 費用）GO WITH CONDITIONS；B（静的な同値性）EQUIVALENT WITH CONDITIONS（判定は cache を書かず報告もしない、(a) の書き手は一度だけ書く setter 2 つで巻き戻しなし、(c) の種類は先の腕と交わらない）；C（反証）STANDS WITH FIXES（予算 `relation_count` を fast path の前に読む、wall は abc でなく最終の bytes で測る） | **GO WITH CONDITIONS**（統合役の決定 2026-11-06）。帯：VS Code 1 thread −1.3〜−1.5 G（計上 −1.4）、縮めた project −11.5%、**4/4 と既定の wall ≈ −1.5%**（下限 −1.0 / −0.7%）、footprint 0、他の 7 project ±0.1%。**持続性：VS Code の節約の ≈ 93%（1.42 G のうち 1.33）は test file 1 つ**なので、「VS Code、test file 1 つ」として固定の corpus vscode `29b68000` 上で計上する。条件：予算を fast path の前に読む（TS2859 の境界で tsgo と同じ順）、test（(b) の 4 種の判定を両方向で、判定・identity cache の大きさ・型の数を完全な検査と比べる；(a) は 2 つの conditional 型が slot を共有する場合）、最終の bytes の窓（M1 は D1 = 1 で M2 も、7 project の 1 thread、8 project の統計、vscode/stable と zod の 4 checker での出力比較、4 checker の conformance を 1 checker の report と比べる）、PR 本文に飛ばした profiler の counter（`profile_ops[OP_RELATIONS]`）・1 file の割合・B が見つけた既存の差（conditional.rs:664-665 と 679-680 は `any` を調べる前に restrictive instantiation を作るが tsgo は作らない：別の忠実性の card）を書く |
| **関門** 共有の path：`CanonicalPath` と `ProgramPath` の bytes を `SharedJsString`（`Arc<[u8]>`）で共有し、request・row・map の key、parse task の target、cache した解決の複製は bytes を写さない（tsgo の `tspath.Path` は Go string で、複製は header だけ）。等価・順序・hash・`Debug` は `JsString` と同じ bytes 上のもの（hash 値も map の反復順も変わらない）（Opus） | —（実装前） | 上限（数えた、VS Code 1 thread）：collection の直後に request・row・map の key が source path を 112,822 個ずつ、parse task が target の path を 118,942 × 2 個、それぞれ自前で持つ（80/96/128 B の page の上限 32.3 MiB）。試作（main `d640a5852` 比）：key・task・host の私有の複製は 0。allocation の呼び出し **−1,682,447（−2.3%）**、load で ≈ −0.47 G（1 回 ≈ 280 命令）、check +0.04〜0.06 G（allocation の変化なし：code の配置）。VS Code 1 thread **−0.42 G**、footprint −8.5 MiB。heap の page は peak で −50.0 MiB だが、footprint は −13.2（同じ時点）：purge を各段の境で強いると −37.1 で、≈ 24 MiB は mimalloc の purge の遅れ（推定、追跡はしていない）、≈ 13 MiB は場所を特定していない | 判定 A（件数 × 費用）GO WITH CONDITIONS；B（静的な同値性）EQUIVALENT WITH CONDITIONS（hash・順序・`Debug`・公開の signature・thread の trait は不変、共有は bytes が不変なので観測できない）；C（反証）STANDS WITH FIXES（−0.42 G、footprint は水準ごとに −6〜−9、check は 0〜+0.1 G、ts の footprint +0.2 MiB、表示と canonical の共有は macOS では起きない） | **GO WITH CONDITIONS**（統合役の決定 2026-11-06）。帯（C の最も低く守れる値）：VS Code 1 thread −0.40〜−0.45 G、footprint −6〜−9 MiB（低い水準 −7.7、高い水準 −5.7）；他 7 project は next −0.15%、playwright −0.19%、hono −0.13%、effect −0.07%、zod・vue・ts は 0（ts の footprint +0.2 MiB は 3 組とも）；check は 0〜+0.1 G の追加（符号は未確定）；4/4 と既定は測るまで 0。条件：最終の窓（VS Code と 7 project の 4/4 と既定を 6 組の ABBA で、stdout の sha256 付き。wall +0.05 s 超か peak +10 MiB 超で止める；(4,1) と既定で phase trace を取り load の ms で Arc の競合を見る；VS Code 1 thread 7 組、7 project は 5 組以上；8 project の diffout）、test（B の surrogate の sample、16 B の const assert、表示と canonical の共有の unit test：macOS では実行で通らない）、PR 本文（`into_*` は複写になる・呼ぶのは test だけ、15 B 以下の path は確保するようになる、census の `unloaded_path_bytes`、check の費用、≈ 13 MiB の未特定、purge の遅れは推定）。**この card で tsrs の性能の track を終える**（owner の決定 2026-11-06 21:16：(b) の理由、side table の束、段の境の purge、reduce_label の skip はしない） |

**2026-11-18 の線の見直し（オーナーの決定、2026-11-05 15:35）。** Codex（effort max）と Opus への相談（`target/claude-tools/handover/perf-tsrs/consult-2026-11-05/`）を受けて、線を約束と努力目標に分ける：**約束 = VS Code 1 thread < 135 G・≤ 3,300 MiB**、**努力目標 = ≤ 132 G・≤ 3,100 MiB**。努力目標は、11/11 の点検で ≈ ≤ 133.5 G（7 round の low-mode 中央値）かつ裏付けのある残り ≥ 1.7 G、≈ ≤ 3,250 MiB かつ数え終えた残り ≥ 170 MiB を示せたときだけ続ける。同じ日のオーナーの指示（15:25）：予想 0.3 G・50 MiB・1 日以上の card は、上限の実測・足す側の実測・方法の違う 2 つ以上の subagent の判定と反証役 1 つを経て、その結果を台帳に書いてから実装する。memory の card は MiB と G の両方を予想する（重複・複製・確保を消す card は両方が減るが、record を詰める card はここでは命令数が増えてきた：Node 48 → 32 B は −269 MB・+3.40 G、Symbol 40 → 32 B は −55 MB・+0.16%）。

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
