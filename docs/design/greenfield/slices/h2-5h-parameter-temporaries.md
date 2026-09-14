# Codex 担当: H2.5h ES5 parameter temporary の引き渡し

2026-09-14。Claude の [G5c JSDoc return](h2-8a-jsdoc-return.md) と並行して進めた、
Codex 側の調査・実装依頼資料。以下は準備時の境界と手順を保持する。
現在の [実装報告](h2-5h-parameter-temporaries-report.md) は原4件 exact ×2 と関連回帰の成功、
追加5 controls に残る別 owner の printer 差分を記録する。
両担当の合流状況は [統合記録](h2-8a-g5c-parameter-integration.md) を参照する。
担当と共有ファイルの扱いは [担当境界表](h2-8a-g5c-h2-5h-parameter-coordination.md)、
短い開始文は [Codex prompt](h2-5h-parameter-temporaries-codex-prompt.md) を参照する。

## 1. 開始点と目的

- worktree: `/Users/hiramatsu/dev/tsc-rs-parameter-temporaries`
- branch: `work/h2-5h-parameter-temporaries`
- main runtime base: `3462ef0e0ca10b90eaed2c93c12baacbec2e628d`（PR #521）。
- 共通準備 commit: `46b9743b525efd6659ff940dfc24b911f9d91946`。
  Claude の資料と既存 AppleDouble 純削除を履歴ごと取り込んだ。Claude の今後の実装には依存しない。
- Codex 開始 base: `96037be2c876621d59ebf82cda1967cf8c59ac83`。
  main から分岐して共通準備を merge した状態。production source は main と同一。
- 固定した source、上流関数、原 artifact 行、対象外12行は
  [selection JSON](h2-5h-parameter-temporaries-selection.v1.json) に記録する。

目的は、ES2020 transform で生じる引数内 temporary と、後続 ES2015 transform の
default/binding-pattern 展開の責務を tsc 6.0.3 と一致させること。
H2.5h の既知16行のうち次の4行だけを修復候補とする。H2.8a 全体の close や新規 admission は主張しない。

case ID の共通 prefix は `typescript-6.0.3/conformance/expressions/`。
完全な ID と元の qualification 行の hash は selection にある。

| ID の残り | 開始 manifest の facet | 完了時の期待 |
| --- | --- | --- |
| `nullishCoalescingOperator/nullishCoalescingOperatorInParameterBindingPattern.ts#target%3Des5` | JS write 1件の差分 | 元の全 command が exact ×2 |
| `nullishCoalescingOperator/nullishCoalescingOperatorInParameterInitializer.ts#target%3Des5` | 同上 | 同上 |
| `optionalChaining/optionalChainingInParameterBindingPattern.ts#target%3Des5` | 同上 | 同上 |
| `optionalChaining/optionalChainingInParameterInitializer.ts#target%3Des5` | 同上 | 同上 |

4行とも既存 owner は `h2-5h-ca-2a-r4`、diagnostics/emit-result の差分と refusal は false。
これは開始時の manifest の分類であり、新たな Rust 実測結果ではない。
残り12行は独立した既知差分として保持する。4行が同一原因であることも trace 前には確定しない。

## 2. 確認済みの source と第一仮説

読解順は `docs/design/README.md` → `emitter-architecture.md` →
`post-h1-completion-slices.md` の現行 header → 本資料 → pinned upstream。
architecture の `E-CONTEXT`、`E-NAMES-BASE`、`E-NAMES-H` と pass 順序を現行コードで照合し、
設計 gate で変更する契約と保持する契約を明示する。古い qualified ラベルだけで閉包完了にしない。

| 層 | 実在する入口 | 確認済み / 調べる値 |
| --- | --- | --- |
| 上流の共通 visitor | `_tsc.js::visitParameterList` 91168–91181 | `VariablesHoistedInParameters` **かつ emit target >= ES2015** のときだけ、この段階で defaults を body に移す |
| Rust target ladder | `builtins/es2021.rs::TargetTransformer` / `TargetVisitor` | ES2021/ES2020/ES2019/ES2016 を同じ visitor で処理。transformer は target を持つが visitor には現在 target field がない |
| 引数の予約と変換 | 同ファイル `plan_parameter_hoists` / `visit_parameter_list` / `lower_parameter_default` | temp が必要なら binding alias を事前予約。lowering の条件には hoisted flag があり、上記 target 条件はない |
| temp の所有 | 同ファイル `allocate_hoisted_temp` / `merge_function_lexical_environment` | typed binding と lexical owner を使い、body に var/initialization を materialize する |
| 後続 ES2015 | `builtins/es2015.rs::visit_parameter_list` / `visit_parameter` / `transform_function_body` | ES5 で default/binding pattern を処理する既存実装。parameter env の suspend/resume、custom prologue、source range を持つ |
| 共通 context | `transform.rs::hoist_variable_declaration` | `IN_PARAMETERS` 中の hoist で flag を立てる既存 producer。初期変更対象ではない |
| 既存の対照実装 | `builtins.rs::visit_owned_module_function_children` | `preserves_native_parameter_defaults` による分岐が既にある。read-only の参考経路 |

**第一仮説**: ES5 にも先行 pass の parameter lowering と alias 予約が適用され、
後続 ES2015 pass に渡す parameter/body/identity が上流と違う。
target 条件を足すだけで十分か、alias 予約・generated name の割当順まで合わせる必要があるかは未確定。
printer の出力置換や temp 名の文字列再番号付けで補正しない。

追跡値は、effective target、各 pass の入力/出力 parameter、original-node chain、
`IN_PARAMETERS` と hoisted flag、binding の予約/割当順、lexical owner、initialization statements、
後続 default/binding-pattern 展開順、comment/source-map range とする。
TS の trace は原因調査用に限り、無改造の pinned TS による完全観測を期待値にする。

## 3. 上流 owner inventory

すべて `vendor/typescript-6.0.3/lib/_tsc.js`。selection に関数の正確な span と
AST `getStart`〜`end` の UTF-8 SHA-256 を記録した。同名関数は enclosing function で区別する。
この hash を既存 ledger の `tsc-hash` 算法と同一とは扱わない。

| owner | 行 | 閉包で調べる branch |
| --- | --- | --- |
| `visitParameterList` / `visitFunctionBody` | 91168–91181 / 91277–91290 | target 条件、env の開始・中断・再開・回収 |
| `addDefaultValueAssignmentsIfNeeded` / `addDefaultValueAssignmentIfNeeded` | 91182–91199 | 更新なし、rest、binding pattern、initializer の分岐 |
| `addDefaultValueAssignmentForBindingPattern` / `...ForInitializer` | 91200–91276 | generated parameter identity、初期化順、range/flags |
| `transformES2020` | 102943–103202 | traversal と shared visitor の接続 |
| その `visitOptionalExpression` / `transformNullishCoalescingExpression` | 103073–103157 / 103173–103192 | 評価回数、receiver、temporary の hoist |
| `transformES2015/visitParameter` | 105672–105722 | 元 parameter を使う後段の binding/default 展開 |
| その `addDefaultValueAssignmentsIfNeeded2` | 105726–105744 | parameter 順の初期化 |
| その `insertDefaultValueAssignmentForBindingPattern` / `...ForInitializer` | 105745–105814 | destructuring、temp と source/comment range |
| その `transformFunctionBody` | 106255–106329 | custom prologue の区分、rest、body、lexical merge |
| `transformNodes/hoistVariableDeclaration` | 116104–116116 | parameter flag の producer |

実装で新たに到達・変更する generated-name、destructuring、range worker は設計に追加する。
大きな transform 全体の hash だけで変更箇所の依存閉包を代用しない。

## 4. 作業範囲

初期 production 許可は次の **2ファイルのみ**。片方で閉じるなら他方は変更しない。

- `crates/emitter/src/builtins/es2021.rs`
- `crates/emitter/src/builtins/es2015.rs`

新規の専用ファイル:

- `scripts/observe-h2-5h-parameter-temporaries.mjs`
- `crates/compiler/tests/fixtures/h2-5h-parameter-temporaries.json`
- `crates/compiler/tests/h2_5h_parameter_temporaries.rs`
- 本 packet 系の `h2-5h-parameter-temporaries-design.md` / `...-report.md`

top-level integration test にして `contracts.rs` 等の共有登録を変更しない。
既存 adapter を参考にしても、fixture selector / observer / capture 環境変数を Claude と共有しない。
capture prefix は `TSC_RS_H2_5H_PARAMETER_`、証拠は `target/parameter-temporaries-runs/` とする。

`transform.rs`、`target_bindings.rs`、`generated_bindings.rs`、`builtins.rs`、printer/factory/metadata は
read-only の参照対象。ここが真の owner なら当該原因を evidence 付き `OUT-OF-SCOPE` とし、
残る範囲を進める。後の別 ticket/統合順序を決めるまで編集範囲を黙って広げない。
binder/checker、syntax、program、compiler production、harness、xtask、oracle、ratchets、CI は編集しない。

## 5. 実行順序と設計 gate

1. HEAD、dirty state、base ancestry を記録し、`node scripts/check-h2-5h-parameter-preparation.mjs` を実行する。
   初期 source 検査であり runtime readiness gate ではない。着手中の worktree を reset しない。
2. selection の ES5 4行と ES2015 4行を、各 artifact の元 input/options/route のまま replay する
   専用 strict test を作る。Rust actual ×2、期待 tuple、全差分、command/exit、source/binary hash を保存する。
   この fresh native before は資料作成時点では **未実行**。既知失敗を成功扱いに変更しない。
3. §2 の trace で4行を関数/branch 単位に分類し、target gating と alias/name allocation を切り分ける。
   元の ES2015 は H2.5g qualification に存在する。ESNext 対照は元 source の directive 展開を使い、
   fresh TS/Rust を観測する。新 controls も production 編集前に具体的入力・期待値を固定する。
4. `...-design.md` に upstream 宣言/body/caller/callee、Rust 型・関数・寿命、順序、編集手順、
   正負対照、out-of-scope、選択した閉包の未解決0を記録する。依頼書の転載だけで gate 完了にしない。
5. owner の境界で修正する。target は既存の effective target から型付きで渡す設計を優先するが、
   未測定の方式を決め打ちしない。temp identity、評価順、lexical scope、原 node/range を保持する。
6. 元8行、新 controls、§6 の回帰を最終 source で検証する。必要な再実行は変更の影響範囲に限定する。
7. `node scripts/check-h2-5h-parameter-preparation.mjs --scope`、`git diff --check` と focused checks の実結果を
   report に残し、原因別 commit を push する。source pin は before のまま保存し、after 証拠は別記録にする。

## 6. 必須対照と検証

| 群 | 必須対照 | 主に保護するもの |
| --- | --- | --- |
| P1 原ケース | ES5 4行、元 ES2015 4行、元 ESNext 4行 | target 境界と完全 command。ES2015 の期待は固定済み、ESNext の fresh 観測は未実行 |
| P2 temp 不要/必要 | 単純 identifier と call、optional property/call と receiver、nullish 左辺の副作用 | 不要な hoist を増やさない、評価回数と receiver |
| P3 parameter 形 | initializer、computed binding key、pattern 自体の default、複数 parameter、rest、空 pattern | 予約の条件、割当順、parameter 間の順序 |
| P4 scope | function declaration/expression、arrow block/concise body、method、入れ子、body 側の temp、同名 local | function 境界と復元、body への二重移動を防ぐ |
| P5 共通 visitor | ES5/ES2015 の `**=`、ES5/ES2020 の `??=`/`&&=`/`||=`、ES2019/ES2020 の optional/nullish | 共有する ES2016/ES2021 pass と保持 target の非退行 |
| P6 出力面 | original の comments、prologue、LF/CRLF、removeComments、sourceMap、emitBOM、noEmit/noEmitOnError | raw bytes、順序、map、BOM、callback metadata、diagnostics と exit |

新 controls の全直積は作らず、到達 branch ごとに入力を割り当て、設計に case ID を固定する。
sourceMap など既存支持外の組合せは別 owner として残し、今回の4行に混ぜて admission を広げない。

比較は `writes` の順序/path/kind、callback bytes・長さ・hash、BOM と materialized bytes、
`on_error_callback_present`、`source_files`、reported/emit diagnostics、emit-skipped、
emitted-files/source-maps の有無と内容、status writes、exit の全項目を扱う。
UTF-16 の比較面で `from_utf8_lossy` / U+FFFD 置換をしない。既存の狭い比較 helper を無条件に流用しない。
元 ES5 4行は frozen TS が **TS5107 と command exit 2** を返す。`ignoreDeprecations` を足さず、
診断を含む全 tuple 一致を成功条件とする。test runner 自身の exit とは区別する。

既存回帰は emitter `active_transform_contract` の parameter/optional/nullish/logical/exponentiation 群、
context lifecycle、generated name、comment/token scope。特に
`es2019_parameter_hoists_share_the_typed_function_scope_plan`、
`es2020_parameter_hoists_move_defaults_into_typed_function_prologues`、
`es2015_class_bindings_created_in_parameters_move_defaults_to_the_body`、
`rest_parameter_loop_reuses_the_dedicated_i_slot_per_function` を保持する。
最後に emitter library/関連 contracts、元 H2.5h static-this/super と UTF-16 rows、
source-map の既存 `h2_6a_map_option_projection` を affected path に応じて実行する。
前 base の既知失敗は元のまま記録し、新規退行と区別する。G5c の実行・修復は Claude が担当する。

専用 test 作成後の実行形（この test/observer は準備コミットではまだ存在しない）:

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/target/parameter-temporaries" \
  taskpolicy -b nice -n 15 cargo test --offline -p tsc-rs-compiler \
  --test h2_5h_parameter_temporaries -- --nocapture --test-threads=1
```

同じ Mac では重い native 実行を1本に限定する。実行枠は担当境界表の手順で明示的に引き渡す。
command の stdout/stderr と returncode を保存し、pipeline の末尾や `|| true` で結果を隠さない。
現行 schedule の軽量運用に従い、full developer CI / certificate walk / 全 profile 再 mint は追加しない。

## 7. 完了と統合

実装完了は、4行の after 全一致 ×2、ES2015 既存4行と ESNext/new controls の非退行、
必要回帰の結果、最終 source/binary/evidence hash、許可範囲の diff、未解決と別 owner の明示が揃った時点。
別 owner が残るなら部分閉包と記録し、4行すべてを閉じたことにしない。

統合担当 Codex が Claude の branch と本 branch を履歴を保持して取り込み、
最終 combined source で両者の focused checks を確認する。4行が本当に exact になった場合だけ
H2.5h known-divergence manifest を **16→12 の純削除**にする。残り12行、TS tuple、比較、profile は維持する。
他 band に同じ修復が波及した場合も evidence で個別に分類する。
共有 architecture/index の更新、必要な manifest の純削除、PR と unchanged hosted acceptance は統合段階で一度に扱う。
本資料の branch だけで新規 PR・hosted run は開始しない。

## 8. 準備資料の検査結果

2026-09-14、production 編集なしで以下を確認した。

- preparation verifier: 27 input pins、15 upstream owners、ES5 4行 + ES2015 4行、保持12 manifest 行が一致。
- scope verifier: 新規5ファイルが許可範囲内、ticket の production 交差0。
- 既存 G5c verifier: 15 pins、12 upstream owners、原1ケースの strict before captures ×2 が引き続き一致。
- verifier の Node syntax check、3文書の local links、staged diff の whitespace check が成功。

fresh native before、実装設計 gate、production 修正、after、今回の hosted acceptance は未実施。
本検査結果は資料と開始点の整合性を示し、runtime の修復を示すものではない。
