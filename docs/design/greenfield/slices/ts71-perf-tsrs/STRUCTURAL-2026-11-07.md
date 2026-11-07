# 構造変更の試作と評価（2026-11-07）

[README の計画](README.md#次のアルゴリズムデータ構造変更の計画2026-11-07) と、所有者の「1 から 6 を実装」に従って、
#913 後の profile、引数と型推論の census、構文共有と flow 索引の独立した試作を実施した。
**両試作は通常版の実時間で継続目安の 5% 改善に届かず、不採用。compiler の main は変更しない。tsrs より速いという目標も未達。**
型・symbol の生成を遅らせる拡張、arrow の共通 template、型推論の recipe cache は未実装で、条件付き段階を見送った。
6 段階すべての実装を採用・完了したという記録ではない。

## 段階ごとの結果

| 段階 | 実施した内容 | 判断 |
| --- | --- | --- |
| 1 | #913 後の source を固定し、VS Code / TypeScript compiler の 1 / 4-thread profile と引数・生成物の census | 完了。全 thread の CPU を記録。依存関係を含む critical path 自体は未計測 |
| 2 | call の間だけ共有する object member の構文配列 | 実装・局所テスト・8 案件測定まで実施し、不採用 |
| 3 | 共有する構文と毎回新しく生成する semantic state を分離し、fresh な型・property symbol の同一性をテスト | この範囲は段階 2 に実装。生成の遅延・arrow の拡張は見送り。escape / lifetime の完全な証明は未実施 |
| 4 | 解決済み symbol を使う、最大 16 代入の直線 block の索引 | 実装・shadow・局所テスト・8 案件測定まで実施し、不採用 |
| 5 | property の列挙・名前照合・候補への適用、visited hit / miss と再訪理由を追加観測 | 小さい構造走査費用と既存 visited の効果を確認。recipe cache は実装しない判断 |
| 6 | A/A、独立した 2 試作それぞれの通常 release 比較、全出力・peak footprint・source/binary/input identity を保存 | 不採用判断まで完了。採用候補がないため、組合せと追加の独立確認、全適合性・hosted code gates は実施していない |

## 固定したものと観測

基準 source は `8e573a1fd7a0b689506203961e136bad397914f7`。通常 binary は #913 採用版
`932e975f6141199123372a4a05e6c1baa8036abe` の保存済み CLI（SHA-256
`79e68a70641ebb3395759815320cb78d1477b5c7954333d1863b918bc729fd9d`）を使用した。
両 source 間の変更は profiler と文書で、compiler の実装は同じ。profile 用 binary は `8e573a1fd` から line tables 付きで別途 build した。
固定 tsgo は `aa8149273b23401f0a79a5f0384c42de51888693`。

各 thread 数で VS Code を 1 回、TypeScript compiler を 12 回記録。最初の VS Code 1-thread 記録には整形処理が冒頭で重なったため、
その原本を除外理由とともに保存し、重複処理なしの `vscode-1-clean` に置き換えた。結果を見て選び直したものではない。
self line の解決率は CPU の 73.78〜77.71%、project owner の帰属率は 99.90〜100.00%。未解決・system の分も分母に残した。

| inclusive CPU（親子は重なる） | VS Code 1 / 4 | TypeScript compiler 1 / 4 |
| --- | ---: | ---: |
| `choose_overload` | 40.58% / 42.10% | 35.01% / 38.10% |
| `check_expression_with_contextual_type` | 32.21% / 32.47% | 25.29% / 26.54% |
| `get_type_at_flow_node` | 10.94% / 10.13% | 21.48% / 20.50% |
| `infer_from_properties` | 0.92% / 1.65% | 4.08% / 3.90% |

この表は sample ごとに同じ関数名を一度だけ数え、source location が分割された inline frame の重複を除いた値。
元の location 別 counter をそのまま足した値は採用しない（`function-union.json`）。CPU 比率は wall の削減率ではなく、最適化された行への帰属にも限界がある。
4-thread の各 thread の CPU と active interval は保存したが、thread 間の依存 trace がないため critical path と同一視しない。

1-thread の call 内の引数検査は VS Code 1,816,047 回中 404,308 回、TypeScript compiler 112,611 回中 22,132 回が同じ引数への再訪だった。
同じ明示 key や node の再訪は、文脈・副作用まで同じことの証明ではない。
object の member 配列の列挙・複製に付けた scope は、それぞれ観測時間で約 9.60 ms / 0.057 ms。
これは構文処理の一部であり、全構文費用の上限とは扱わない。
観測版の経過時間は通常版の約 1.5 倍（1 thread）、約 2.8〜3.5 倍（4 threads）になったため、性能結果には使っていない。

## 実装と正しさの確認

構文共有は [`f9d1a1250`](https://github.com/kazhiramatsu/tsc-rs/commit/f9d1a1250a6f7f112e9b8e1b9dc0e2f955f7293c)。
`resolve_call` の scope に、object member の node、種類、computed name の node、method 判定を順序付き配列で保持する。
再入した call は別 scope を持ち、成功・失敗・`CheckAbort` の通常の return 経路で破棄する。
computed name の評価、grammar、contextual type、推論、診断、型と symbol の生成は元の順序で実行する。
共有する配列には型・symbol を保存せず、`ObjectLiteralAcc` は検査ごとに作る。
同じ構文を使っても object type と各 property symbol が別 identity になるテスト、nested call の失敗後に scope が残らないテストを追加した。

`cargo test --offline -j 2 -p tsc-rs-checker --lib literals::tests -- --test-threads=1` は 36 passed。
追加した診断ケースの TS2322 は固定 tsgo に bundled `lib.es5.d.ts` を与えて確認した。
最初の `--noLib` だけの oracle 実行は global type 不足で対象診断まで進まなかったため、失敗記録を残し、判定には使っていない。

flow 索引は [`f0bb1c3f1`](https://github.com/kazhiramatsu/tsc-rs/commit/f0bb1c3f1ee35d6708a9bee0145d88efde9644f3)。
checker 内に 512 枠の表を遅延作成し、file / entry flow ごとに最大 16 個の代入先 symbol と距離を整列して持つ。
最も近い関係する代入の手前まで、少なくとも 2 節点をまとめて飛ばす。表の payload は checker ごとに 110,592 bytes。
identifier の解決済み link と宣言 symbol だけを読み、索引を作るための名前解決や型生成は行わない。
未知の名前、speculation、active mapper、loop / reduce の可変状態では通常の走査へ戻る。
CALL、SHARED、condition、join、loop、reduce label、array mutation、START は barrier。for-in 宣言も対象外。
binder の graph と metadata は変更していない。

既存の `flow-memo-shadow` は条件を緩めず維持した。加えて、飛ばす代入を元の worker で実行して `None` であることと、
型・symbol・signature の生成数、instantiation count、flow register / epoch、frame・mapper・loop stack と reduce map の長さ、shared flow hash、query の flags・depth・key が同じことを照合した。
これは明示した状態と純粋な経路の検証であり、checker 全メモリの byte 比較ではない。
8 案件・1 / 4 threads の 16 実行は stdout / stderr / exit が各 thread 数の基準と一致。
報告がある実行では 1,179,027 jumps / 4,887,110 代入節点を照合し、assertion 失敗なし。
既存 memo の 20,876,251 hits の照合も disagreement 0。Vue 1-thread は報告がないので件数の根拠から外し、出力だけを照合した。
shadow は memo の置換を無効にするため、この件数は通常版の skip coverage を示さない。
`cargo test --offline -j 2 -p tsc-rs-checker --lib flow -- --test-threads=1` は 61 passed。

## 通常 release の実時間とメモリ

各試作を基準と独立に比較した。nice 0、warm files、fresh process、noEmit、worker / checker を 1 / 4 に固定。
各案件・thread 数・版で warm-up 2 回の後、AB / BA 6 組。各試作の前に VS Code / TypeScript compiler で A/A 4 組も実施した。
測定窓に local build・test・解析を重ねず、process snapshot と各 run の load average を保存した。機械全体が完全に無負荷だったとは主張しない。
下表は各組の比の中央値。負が改善。peak footprint は OS の報告値で、索引や構文配列だけの heap 使用量ではない。

| project | syntax wall 1 / 4 | flow wall 1 / 4 | syntax peak footprint 1 / 4 | flow peak footprint 1 / 4 |
| --- | ---: | ---: | ---: | ---: |
| VS Code | +0.28% / +0.09% | +0.61% / +0.83% | +0.12% / -0.17% | +0.03% / -0.10% |
| TypeScript compiler | -0.17% / +1.78% | +0.88% / +0.90% | +0.02% / -0.30% | +0.12% / +0.32% |
| Hono | -0.04% / +0.76% | +0.12% / -0.33% | +0.03% / -0.29% | +0.20% / +0.96% |
| Zod | +0.07% / +0.05% | +0.20% / -0.02% | -0.07% / +0.19% | -0.01% / -0.09% |
| Effect | +0.03% / -0.29% | +0.10% / +0.12% | +0.06% / -0.36% | -0.09% / +0.03% |
| Next.js | -0.18% / +0.54% | +0.44% / -0.88% | +0.00% / -0.19% | +0.09% / -0.25% |
| Playwright | -0.09% / -0.34% | +0.46% / +0.26% | +0.63% / +0.03% | +0.79% / +0.22% |
| Vue | +0.12% / +1.57% | +0.34% / -0.64% | +0.01% / -1.22% | +0.04% / -0.19% |

全 512 実行（2 試作 × 8 案件 × 2 thread 数 × 各 16 実行）の stdout / stderr / exit が基準と一致し、
別枠の A/A 96 実行も一致。記録した config / git 状態 / binary hash は各測定窓の前後で不変だった。
外れたサンプルも削除していない。各条件 6 組と少なく、bootstrap 95% 区間は同一窓内の探索的なもの（多重比較補正なし）。
全区間・絶対値・命令数・raw sample は各 `summary.json` と `runs.jsonl` にある。

構文共有の VS Code wall 区間は 1 thread −1.59〜+0.64%、4 threads −0.56〜+1.23%。
flow の VS Code は +0.36〜+1.12% / −0.38〜+1.06%、TypeScript compiler は −0.32〜+1.26% / −6.22〜+5.13%。
A/A でも短い TypeScript compiler 4-thread は大きく揺れた（構文共有の窓 −7.27〜+4.49%、flow の窓 −2.62〜+1.08%）。
小さい差を安定した高速化やメモリ削減と扱わない。flow の VS Code / TypeScript compiler の命令数中央値も約 +0.10〜+0.15% だった。
両試作とも対象 workload の通常 wall で 5% 改善を示さず、組合せと追加の独立確認には進めなかった。

## 条件付き拡張を見送った根拠

候補の不採用は、その候補内で生成した型・symbol の全廃棄を意味しない。推論 arena、once-state、診断、後続の cache からの参照が残る。
今回の生成数は semantic object の数で、malloc 数や生存期間の完全な分析ではない。
型・symbol をまとめて遅延生成する設計の正しさを、この数から推定しない。構文共有の通常測定が継続目安を満たさなかったため、
arrow template と生成の遅延はこの段階では拡張しない。

推論の追加 census は [`177252b00`](https://github.com/kazhiramatsu/tsc-rs/commit/177252b002100c53906e925a6ac7563d56dd02a0)。
VS Code 1-thread では visited hit 3,533,651、miss 4,198,329。同じ walk 内の型 pair 再訪は 1,952 で、分散変更 1,905、priority 変更 129
（重なりがあるため合算しない）。TypeScript compiler は hit 22,128、miss 75,104、pair 再訪 1,200、すべて分散変更を伴った。
property 列挙 / 名前照合の観測時間は VS Code 10.31 / 14.35 ms、TypeScript compiler 1.49 / 2.63 ms。
名前照合の一部では lazy な型・symbol の生成も起きた。構造だけの recipe として無条件に保存できる処理ではない。
追加観測版も 1 / 4 threads の両案件で出力一致。

これらは walk 内の観測で、複数の `infer_types` にまたがる inference context の共有可能性を証明していない。
signature / type argument の recipe、member revision の新設、候補更新の全状態ログ比較は未実施。
今回確認した小さい構造走査費用を根拠に大きな cache を追加する段階には進めず、recipe 実装は保留した。

## 保存先と検証の限界

**2026-11-07 夜の追記**：下記の local 保存先（`target/codex-structural-20261107/`、`target/codex-line-profile-20261107/RESUME.json`）は、
同日夜の統合者の事故（main repository の誤削除）で失われた。source は archive branch（PROTOTYPES.md）に、hash と結果はこの記録と README に残る。


local `target/codex-structural-20261107/` に通常 / 観測 / shadow binary、profile + dSYM、行別 HTML、raw counter、全 sample、失敗ログ、
source patch、command / exit / hash の記録を保持した。主要入口は `STATE.json`、`VALIDATION.json`、`MANIFEST.json`、
`candidate-sources.json`、`syntax-review.json`、`flow-review.json`、`inference-review.json`。
再開入口は `target/codex-line-profile-20261107/RESUME.json`。source worktree も削除していない。
入力は config hash と git 状態を記録したが、依存ファイル全体の snapshot ではない。

不採用の試作に full checker / workspace test、全 4-checker conformance、全 types / symbols / emit oracle、hosted code gates は実施していない。
採用版として検証済みとは扱わない。今回 main へ入れるのは実施記録だけで、compiler の動作変更はない。
archive branch の一覧は [PROTOTYPES.md](PROTOTYPES.md)。

| binary | source | SHA-256 |
| --- | --- | --- |
| `syntax-plan` | `f9d1a1250` | `32ef676f561df81f89d275206be69eda93a97a7da74a33675fd889d6575634ea` |
| `flow-index` | `f0bb1c3f1ee35d6708a9bee0145d88efde9644f3` | `e9036d8d9b9cbcbb2dde2fa1bb197b0da39e4292e7e8094f1f0d3fc831442fc2` |
| `flow-shadow` | `f0bb1c3f1` | `40b61fbda32a4e757a5d650909ab7991dc1db3c10dee6a522c8794cd4a194f4c` |
| `inference-observe` | `177252b00` | `ac13a8b9cde98e7a17c767ef4cb0d6ecd0e99309f873419e01ccfff1b06b3032` |
