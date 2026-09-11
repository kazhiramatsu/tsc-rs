# 作業報告書：A6-41 literal computed name・lexical prologue（2026-09-11）

依頼：[貼り付け用プロンプト](h2-8a-decorator-literal-prologue-claude-prompt.md)・
[詳細 handoff](h2-8a-decorator-literal-prologue-handoff.md)・
[開始 manifest](h2-8a-decorator-literal-prologue-start.v1.json)・
[提案入力 120 件](h2-8a-decorator-literal-prologue-proposed-inputs.v1.json)。
設計記録（source→Rust owner→witness 対応表、後続 pass の読者と到達性、原因別の所有権判断、
全計測）は [h2-8a-decorator-literal-prologue.md](h2-8a-decorator-literal-prologue.md)。
本書はその要約と結論です。

## 1. 結論

- 提案 120 件（literal-member-kinds 48 / literal-key-spelling 48 / lexical-prologue 24）を
  既存 observer で TypeScript 6.0.3 に各 2 回観測（診断 0・例外 0・exit 0）し、独立した新規
  group として登録。production 無変更の Rust baseline は 60/120 一致（18+28+14）。
- 到達性拡張として `lexical-prologue-readers`（18 件：CommonJS/AMD/UMD/System の top-level
  hoist と parameter initializer）を追加観測。baseline は 2/18（native の 2 行のみ）。
  System の 2 行は JS 一致・source map は System module transform 側（Rust 出力に mapping が
  皆無）で decorator の所有ではないため、それを除く 16 件を `lexical-prologue-readers-v2`
  として通常実行 group に登録（入力・観測は v1 と byte 同一、v1 は保持）。
- 不一致が再現した原因は 6 つ。すべて通常 source の完全タプル比較で witness され、原因別に
  commit・focused 検証。**最終 production `8884fcc05` で新規 4 group 136 件は 136/136 完全
  タプル一致 ×2、通常実行 exit 0**。既存 5 group 192 件・既存 530 件は期待値不変で一致 ×2、
  full62 比較は変更 0・欠落 0・追加 0、emitter suites 通過（§5）。
- 「提案 120 件＋読者 16 件が通る」ことの主張であり、「標準 decorator 全 source 経路／H2.8 全体の
  完了」ではありません（§6）。

## 2. 開始点と前作 PR の状態

- worktree `/Users/hiramatsu/dev/tsc-rs-dec-literal-prologue`、branch
  `prep/h2-8a-decorator-literal-prologue`、開始 HEAD `e8281f286`（production 基準 `5134bb018`
  ＋資料 commit）。production 12 ファイル・observer・harness・`_tsc.js` の SHA-256 は着手時に
  再計算し manifest と一致。
- PR #513：着手時 OPEN（head `5134bb018`、hosted gates 実行中）。作業中に Codex により
  マージ済み（10:50:45 UTC、hosted gates success、merge commit `9806a4cb9` = `origin/main`）。
  merge commit の tree `1e632db73…` は production 基準の tree と byte 同一、`5134bb018` は
  `origin/main` の祖先。差分なしのため同一性のみ記録し、production 再検証扱いにはしていません。
  本 branch の reset/rebase は行わず、main・PR #513 への反映もしていません。
- root・dec-next・dec53・dec-followup・dec-merge の worktree・target・capture は未変更。
  専用 target `target/decorator-literal-prologue-acceptance`、run dir
  `target/decorator-literal-prologue-runs/`（tools、prelaunch、入力 archive、log、captures、
  binaries、receipt）。重い実行は 1 つずつ、`taskpolicy -b nice -n 15`、`CARGO_BUILD_JOBS=2`。

## 3. Baseline（修正前）と分類

test 登録のみ、1 つのバイナリ `d1476afb7499…`（読者 group は `30322e5e1217…`）で実行。
受領証 `ratchets/h2-8a-decorator-literal-prologue-baseline-*.v1.json`。

| group | 一致 ×2 / 件数 | 失敗の家族 | 原因（source → Rust owner） |
| --- | --- | --- | --- |
| literal-member-kinds | 18 / 48 | method/getter/setter × {`["x"]`, `[key]` 対照} × lowered 5 構成（auto-accessor 12 件と ESNext/define は一致） | 原因 1：`__runInitializers(_a, _staticExtraInitializers);` の statement に class 名の source map range が無い（tsc は call と statement の両方に `setSourceMapRange`）。JS は一致、map のみ |
| literal-key-spelling | 28 / 48 | template key 3 source × 5 構成、substitution 対照 × 5（numeric 18 件・string 対照 6 件は一致） | 原因 2：template source の `textSourceNode` 未設定＋printer の text-source 分岐が Identifier/String source のみ（`"xa"` vs `` `xa` ``）。substitution 対照は原因 1 の map 差 |
| lexical-prologue | 14 / 24 | function/arrow directives × 5 構成 | 原因 3：`merge_block_environment` が index 0 へ挿入（tsc は directive の後） |
| lexical-prologue-readers | 2 / 18 | CommonJS 5・AMD 2・UMD 2・parameter 5・System 2 | 原因 4：hoisted `var` に `CUSTOM_PROLOGUE` が無く CommonJS の custom prologue copy が読めない（`__esModule` marker の後に出る）。parameter 行は原因 4＋5＋6。System は JS 一致・map は System owner |

## 4. 原因別コミット（production 最終 `8884fcc05`）

| commit | 内容 | focused 検証 |
| --- | --- | --- |
| `aeffc5fb9` | 136 件（120＋readers 18／v2 16）を harness に登録、baseline 受領証、設計記録 | — |
| `a41921f1a` | 原因 1：pending static initializer の statement に initializer の source map range を複写（`materialize_pending_initializer_statements`） | member-kinds 48/48 |
| `60a6a8a94` | 原因 2：`create_string_literal_from_property_literal` が numeric/template source にも text source を設定、printer が numeric source（cooked text を quote）と template source（`getLiteralTextOfNode` 委譲：source 範囲があれば verbatim、無ければ template token writer）を追加。quote 正規化はしない | key-spelling 48/48 |
| `5bcb2c600` | 原因 3：`merge_block_environment` が standard prologue directive と hoisted function の後（`leftHoistedFunctionsEnd`）へ挿入（source file と共通の `hoisted_declaration_insertion_index`） | lexical-prologue 24/24 |
| `940d62e4a` | 原因 4：`create_hoisted_declarations` が declaration に `NO_NESTED_SOURCE_MAPS`、statement に `CUSTOM_PROLOGUE`（`hoistVariableDeclaration`／`endLexicalEnvironment`） | readers-v2 11/16（module 9 行＋native 2 行が一致、parameter 5 行は残存） |
| `8df67d1c1` | 原因 5：class-fields downlevel の `prepend_function_prelude_to_block` を `mergeLexicalEnvironment` の span（directive → hoisted function → hoisted var）に合わせ、custom prologue を既存 hoisted `var` の後へ | readers-v2 11/16（parameter 4 行の JS が一致、map 差のみ残る。ESNext/set は default 未 lowering） |
| `8884fcc05` | 原因 6：decorator pass に `visitParameterList` を移植（InParameters／VariablesHoistedInParameters／`addDefaultValueAssignmentsIfNeeded`、tsc と同じ flag・range、custom prologue の merge） | readers-v2 16/16 |

候補 patch：`scripts/export-decorator-literal-prologue-candidates.sh`（開始 `5134bb018`・終点
`8884fcc05` 固定）→ `docs/design/greenfield/slices/h2-8a-decorator-literal-prologue-01..07-*.candidate.patch`
＋ `…-production.cumulative.patch`。既存 exporter（`306930ab7`、`0fda49509..a4c089c7b`）は未変更。

原因 4 の調査：tsc の `getScriptTransformers` 順（ESNext → ESDecorators → ClassFields →
ES2021…ES2015 → module）に沿って `CustomPrologue` の読者を対応付け（設計記録 §「Reachability
extension」）。読者は CommonJS/AMD/UMD/System の `copyCustomPrologue`（source-file hoist）と
class-fields `mergeLexicalEnvironment`（関数 body hoist、parameter initializer 経由）。関数
directive の挿入順（原因 3）と `CUSTOM_PROLOGUE`（原因 4）は別 witness・別 commit で検証。
一時 probe（未 commit）で flag の付与と class-fields downlevel が `merge_lexical_environment`
を通らないことを確認し、原因 5 を特定。

## 5. 最終計測（production `8884fcc05`、1 つのバイナリ）

| suite | 件数 | exact ×2 | exit | 受領証 |
| --- | --- | --- | --- | --- |
| 新規 literal-member-kinds | 48 | 48 | 0 | `…-final-literal-member-kinds.v1.json` |
| 新規 literal-key-spelling | 48 | 48 | 0 | `…-final-literal-key-spelling.v1.json` |
| 新規 lexical-prologue | 24 | 24 | 0 | `…-final-lexical-prologue.v1.json` |
| 新規 lexical-prologue-readers-v2 | 16 | 16 | 0 | `…-final-lexical-prologue-readers-v2.v1.json` |
| 既存 transform-order | 60 | 60 | 0 | `…-final-transform-order.v1.json` |
| 既存 super-paths | 42 | 42 | 0 | `…-final-super-paths.v1.json` |
| 既存 name-owners | 24 | 24 | 0 | `…-final-name-owners.v1.json` |
| 既存 source-followup | 48 | 48 | 0 | `…-final-source-followup.v1.json` |
| 既存 source-followup-top-level | 18 | 18 | 0 | `…-final-source-followup-top-level.v1.json` |
| 既存 530 件 | 530 | 530 | 0 | `…-final530.v1.json`（full62 比 変更 0 / 欠落 0 / 追加 0、`…-final530-full62.v1.json`） |
| emitter lib / contracts | 495 / 452 pass | — | 0 | `…-final-emitter-suites.v1.json` |

実行バイナリはすべて `4a8e07b7ea03…`（1 つ）。受領証は `ratchets/h2-8a-decorator-literal-prologue-final-*.v1.json`、run dir は `target/decorator-literal-prologue-runs/final-*/`。

各受領証は head・production/fixture/test の SHA-256・環境・実 exit・ログ SHA・実行バイナリ SHA・
全 capture SHA・実行後の source/fixture 不変を記録。full62 比較元 manifest（`badaa9c02d9e…`）と
比較器（`23507016ab00…`）は使用前に受領証と照合し、書き換えていません。

### acceptance

`cargo xtask acceptance`（handoff の環境、専用 target、chain 完了後に単独実行、head `98666f2b7`、2026-09-11T12:22:37Z〜2026-09-11T13:57:41Z）：実 exit 0、ログ SHA `ad74840235b5…`、受領証 `ratchets/h2-8a-decorator-literal-prologue-acceptance.v1.json`。

- `conformance band=all fixtures=5908 cases=7691 T0=100.0000% matched=49024/49024 FP=0 FN=0 mismatches=0`
- `H1 emit acceptance: compatible=1 executed=1 exact=1 diagnostics=2 writes=1`
- `H2.1a emit acceptance: candidates=295 exact=246 diagnostic_controls=0 source_deferred=49 exact_diagnostics=704 exact_writes=256 control_writes=0 repetitions=2`
- `H2.1b emit acceptance: candidates=15 exact=10 source_deferred=5 exact_diagnostics=2 exact_writes=15 repetitions=2`
- `H2.1c emit acceptance: candidates=8 exact=6 source_deferred=2 exact_diagnostics=6 exact_writes=12 repetitions=2`
- `H2.1d emit acceptance: candidates=6 exact=5 source_deferred=1 exact_diagnostics=5 exact_writes=11 repetitions=2`
- `H2.1e emit acceptance: candidates=6 exact=4 source_deferred=2 exact_diagnostics=6 exact_writes=8 repetitions=2`
- `H2.2a emit acceptance: candidates=11 exact=6 source_deferred=5 exact_diagnostics=8 exact_writes=9 repetitions=2`
- `H2.2b emit acceptance: candidates=18 exact=15 source_deferred=3 exact_diagnostics=59 exact_writes=72 repetitions=2`
- `H2.2c emit acceptance: candidates=6 exact=6 source_deferred=0 exact_diagnostics=12 exact_writes=6 repetitions=2`
- `H2.2d emit acceptance: candidates=9 exact=9 source_deferred=0 exact_diagnostics=36 exact_writes=13 repetitions=2`
- `H2.3a emit acceptance: candidates=1 exact=1 source_deferred=0 exact_diagnostics=1 exact_writes=1 repetitions=2`
- `H2.3b emit acceptance: candidates=6 exact=2 source_deferred=4 exact_diagnostics=4 exact_writes=2 repetitions=2`
- `H2.3c emit acceptance: candidates=4 exact=4 source_deferred=0 exact_diagnostics=42 exact_writes=4 repetitions=2`
- `H2.3d emit acceptance: candidates=0 exact=0 future_deferred=695 source_deferred=0 repetitions=2`
- `H2.4a emit acceptance: candidates=10 exact=9 source_deferred=1 exact_diagnostics=8 exact_writes=9 repetitions=2`
- `H2.4b emit acceptance: candidates=44 exact=42 source_deferred=2 exact_diagnostics=150 exact_writes=56 repetitions=2`
- `H2.5a emit acceptance: candidates=172 exact=167 h2_8a_deferred=1 h2_9_deferred=4 exact_diagnostics=335 exact_writes=287 repetitions=2`
- `H2.5b emit acceptance: candidates=72 exact=68 h2_9_deferred=4 exact_diagnostics=48 exact_writes=93 repetitions=2`
- `H2.5c emit acceptance: candidates=15 exact=14 h2_9_deferred=1 exact_diagnostics=19 exact_writes=14 repetitions=2`
- `H2.5d emit acceptance: candidates=24 exact=23 h2_9_deferred=1 exact_diagnostics=47 exact_writes=57 repetitions=2`
- `H2.5e emit acceptance: candidates=41 exact=40 h2_9_deferred=1 exact_diagnostics=88 exact_writes=46 repetitions=2`
- `H2.5f emit acceptance: candidates=8 exact=8 exact_diagnostics=20 exact_writes=8 repetitions=2`
- `H2.5g ordered acceptance pipeline: cases=9027 workers=2`
- `H2.5g emit acceptance: candidates=9027 exact=8511 h2_8a_deferred=6 h2_9_deferred=510 exact_diagnostics=26815 exact_writes=9466 repetitions=2`
- `H2.5h ordered acceptance pipeline: cases=932 workers=2`
- `H2.5h emit acceptance: candidates=932 exact=860 known_diverging=28 deferred=44 repetitions=2`
- `H2.6a ordered acceptance pipeline: cases=177 workers=2`
- `H2.6a emit acceptance: candidates=177 exact=171 known_diverging=4 deferred=2 repetitions=2`
- `H2.6b ordered acceptance pipeline: cases=6 workers=2`
- `H2.6b emit acceptance: candidates=6 exact=6 known_diverging=0 deferred=0 repetitions=2`
- `H2.6c ordered acceptance pipeline: cases=643 workers=2`
- `H2.6c emit acceptance: candidates=643 exact=631 known_diverging=8 deferred=4 current_migrated_refusals=1 repetitions=2`
- `H2.7b ordered acceptance pipeline: cases=1593 workers=2 pre_flip=false`
- `H2.7b emit acceptance: candidates=1593 exact=1557 known_diverging=0 deferred=36 repetitions=2 planned_declaration_members=2414 zero_member_controls=6 transform_blocked=28 preflight_blocked=5`

stale／新規 divergence／facet／error 行：なし。H2.6c の migrated refusal 1 件・suite 要約・H2.7d の reference 3 行は開始 head の hosted run と同じ情報行で、全 band の計数も同一（H2.5g 9027/8511/6/510、H2.5h 932/860/28/44、H2.6a 177/171/4/2、H2.6b 6/6/0/0、H2.6c 643/631/8/4、H2.7b 1593/1557/0/36）。6 原因は band の disposition を変えていません。

## 6. 未判定・未主張の行

| 行 | 状態 |
| --- | --- |
| `lexical-prologue-readers` v1 の System 2 行（`es2022/system/{set,define}`） | 到達済み・JS は開始 head から一致。`main.js.map` は Rust の System 出力に mapping が皆無で不一致。System module transform の owner で decorator 経路ではないため未修正・通常実行 group 外（v1 に観測を保持） |
| class-fields downlevel `lower_parameter_default` の source map（block/assignment に range を付けず、initializer に `NoSourceMap` を付けない） | 原因 5 の focused run で観測（parameter 4 行の map 差）。原因 6 で decorator pass が lowering を所有するため今回の witness は通らなくなり、class-fields 自身が lowering する通常 source の witness は未作成。class-fields owner の観察として記録、未修正 |
| 原因 6 の binding-pattern 分岐（`addDefaultValueAssignmentForBindingPattern`） | 同 callee の未 witness 分岐。実装はしたが主張しない |
| numeric key（decimal/hex/separator）、string 対照 | baseline から一致。理由（scanner の cooking、printer の separator 規則）を記録、未修正 |
| 前作 follow-up が記録した H2.5h 残 28 行、H2.5g deferred（h2_8a 6 / h2_9 510） | 対象外 |

## 7. 環境と運用

- 重い実行は 1 つずつ（build → group、chain 単位）。walk・chain-walk・`cargo xtask ci`・歴史的
  証跡の一括再生成は未実行。前作 PR #513 の run の停止・再 dispatch、CI 設定や既知差分 manifest の
  変更はしていません。
- 修正候補はこの branch にのみ保持（未 push の場合は push のみ、PR 作成・main 反映はしない）。
