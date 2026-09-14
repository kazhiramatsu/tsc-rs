# H2.5h parameter temporary: implementation design

2026-09-14。状態: **原4件と関連回帰 verified、共通 producer は C3 printer 残差を持つ部分閉包、統合待ち**。
[依頼資料](h2-5h-parameter-temporaries.md) の production 許可2ファイルを維持する。
§5 に全68ケースの native before と Rust trace、§6 と report に final after を記録する。

## 1. 固定した入力と比較

開始 source は `9ec083e9e1471e27b0558f1db65420a14f9dcc16`、runtime は準備 base
`96037be2c876621d59ebf82cda1967cf8c59ac83` と同一。
source/上流15関数/原8行の pins は [selection](h2-5h-parameter-temporaries-selection.v1.json) を保持する。

新規 observer `scripts/observe-h2-5h-parameter-temporaries.mjs` が作る
`crates/compiler/tests/fixtures/h2-5h-parameter-temporaries.json` に68件を固定した。
TS Program の command を各2回、計136回観測し、完全 tuple の決定性を確認した。
原8行は input/settings と既存 qualification の全 observed fields が一致することを assert する。
callback metadata、materialized bytes、related diagnostics 等の追加観測は別 fixture に持ち、元 artifact は変更しない。

| 区分 | 件数 | provenance / route |
| --- | --- | --- |
| 原 ES5 | 4 | H2.5h frozen input、`qualified-vfs` / Established |
| 原 ES2015 | 4 | H2.5g frozen input、同じ route/floor |
| 原 ESNext | 4 | 元 ES2015 input の target だけを元 source directive の ESNext に変更 |
| P2–P6 controls | 56 | `/.src/main.ts`、明示 settings、MapFamilyWithDeclarationOnly。noEmit 2件のみ完全 options を保持した `load_program`。map/BOM 等を落とさない |

元の command exit 2 と TS5107 を保持する。strict runner の成否とは区別する。
比較対象は writes の配列全体（順序/path/kind/bytes/hash/長さ/BOM/materialized bytes/onError/sourceFiles/data）、
reported/emit diagnostics と related information、emit/result collection の有無と内容、status、exit。
scalar-only の固定入力に対し、TS callback の非 scalar 値と Rust の非 scalar 観測は fail-closed とする。
出力 path や bytes は正規化しない。host lookup の path handling は元 qualification の VFS policy に従う。

## 2. 上流 trace による観測

`target/parameter-temporaries-runs/upstream/trace.mjs` は、vendored source のコピーをメモリ上で読み込み、
`getTransformers` が返す factory と context の lexical methods を記録用に包む。
vendored source は書き換えない。node の kind/range/original chain/parameter/initializer/flags と
hoist/initialization/env の前後を記録し、printer を追加実行して generated naming を動かさない。
instrumented な全12 original command の結果が、無改造 compiler による fixture と完全一致した。
trace は期待値の代用ではない。

| target / source shape | ES2020 pass 終了時 | ES2015 pass 終了時 |
| --- | --- | --- |
| ES5、initializer（optional/nullish 各1） | parameter の initializer は ConditionalExpression のまま。body に hoisted var | initializer を除去、body は hoisted var → default の if |
| ES5、computed binding（各1） | parameter 名は ObjectBindingPattern のまま。body に hoisted var | generated Identifier に更新、body は hoisted var → destructuring var |
| ES2015、initializer（各1） | parameter initializer を除去、body に var → if | この pass は実行されない |
| ES2015、computed binding（各1） | parameter 名は generated Identifier、body に var → binding var | この pass は実行されない |
| ESNext（4） | ES2020 pass は実行されない | ES2015 pass も実行されない |

原12件で340 trace events。`trace.json` の hash は
`977afc9879615d7736adc2317a84008bdc78b0110cc12cb22a3bc2b56207acf6`。
完全 upstream fixture の hash は
`8cdf5349118b856c7488e1d35a2901acbc1193526c95102309c8cba7873f7145`。

### Target 条件だけを外す切り分け

`target/parameter-temporaries-runs/upstream/counterfactual.mjs` は pinned compiler の
`visitParameterList` にある target >= ES2015 の条件だけをメモリ上のコピーから除き、
原12件を各2回実行する調査用 script。無改造 compiler の fixture をそのまま比較基準に使う。
結果は ES5 の4件だけが変化し、ES2015/ESNext の8件は全 tuple が不変だった。

- initializer 2件: var/default-if を含む関数本体が1行になり、後段の default 処理が立てる multiline が失われる。
- binding-pattern 2件: 同じ改行差に加え、`var _c = _a` の余分な binding が生じ、後続 temp の名前がずれる。
- 差分はいずれも write 0 の callback/materialized bytes・hash・length の6 fields に限定される。
  path、BOM、callback metadata、診断、result、status、exit は不変。

この結果は target 条件が必要であることを上流内で示す。
Rust の実測ではないため、次の native before と一致するかを確認してから修正原因と結び付ける。
記録は `target/parameter-temporaries-runs/upstream/counterfactual.json`。

## 3. Rust に対応させる契約

`TargetTransformer` が保持する `ScriptTarget` は `CompilerOptions::emit_script_target()` に由来する。
`TargetVisitor` に同じ typed target を渡し、source ごとに値として保持する方式を第一候補とする。
別の文字列 setting や pass 名から target を再推定しない。

| 型/関数 | 現行責務 | 検証後の予定変更 |
| --- | --- | --- |
| `TargetTransformer::transform_root` | source/root、context、pass を visitor に渡す | 自分の effective target も渡す |
| `TargetVisitor::new` / field | context の exclusive borrow と SourceId、pass、typed name scopes | `target: ScriptTarget` を追加。borrow/lifetime と source identity は維持 |
| `plan_parameter_hoists` | subtree の temp 必要性を調べ、binding-pattern alias を予約 | target が native parameters を保持する場合だけ alias 用の必要性判定を有効化。ES5 でも parameter と alias-vector の長さを対応させる |
| `visit_parameter_list` | parameter visit 後、hoisted flag に応じて defaults を移す | 上流どおり `target >= ES2015` も要求する |
| `allocate_hoisted_temp` | stable GeneratedBindingId を lexical owner に hoist | 維持。ES5 でも optional/nullish の temp は必要 |
| `merge_function_lexical_environment` | 関数に var/initialization を materialize | 維持。ES5 ではこの時点の body に hoisted var を残す |
| `es2015.rs::visit_parameter_list` / `transform_function_body` | ES5 parameter を最終形へ展開、custom prologue と body を合流 | 最初は維持。元4件と maps/順序の差分が残った場合にこの許可ファイル内で因果を確認する |

`ParameterHoistPlan` は owned な alias vector、`TargetBinding` は typed identity を持つ。
既存の `GeneratedBindingScopes` と最終 traversal による名前割当を保持する。
ES5 の不要な alias 予約と defaults の先行移動を別々に検証し、片方だけで閉じたと推測しない。
source-wide text scan、printer の temp 名置換、global mutable target、scope の共有は追加しない。

上流の根拠は selection の `visitParameterList`（91168–91181）と default workers、
ES2020 visitor、ES2015 parameter/body workers。production の ledger は既存の
`visitParameterList` の `tsc-hash` 算法/値と正確な span を使う。
新たな owner を変更する場合はここに body/caller/callee を追加する。

### 共通 visitor を使う他 pass の閉包

準備時の15 owner pins は保持する。追加で読解した以下の body は同じ `_tsc.js` の
AST `getStart`〜`end` の UTF-8 SHA-256。詳細 inventory は
`target/parameter-temporaries-runs/upstream/shared-pass-owners.json` に保存した。

| owner | span | SHA-256 |
| --- | --- | --- |
| `transformES2021/visitor` | 103217–103225 | `2417b5d01aa4d4a286dd071e0aaf47d04b76531feb5440be80c50481c95943ca` |
| `transformES2021/transformLogicalAssignment` | 103226–103274 | `074f1f1a189b018ca1587693664f783c9912d6d8252ea2709b523af01db5b213` |
| `transformES2016/visitor` | 104658–104668 | `9823071d2d7df2c8752aeddea43c911e73b5f6342d2e2f66bdeeb79ef194ac6c` |
| `transformES2016/visitBinaryExpression` | 104669–104678 | `91f299e45664e7fe29dcbd6e0e67614b8f0a8dcdf88f0b075e55b7c0d7996bc7` |
| `transformES2016/visitExponentiationAssignmentExpression` | 104679–104728 | `f24991ddc8f7bda8ce2a626e68f1619310d9f042ca48a799cab9630b6813220d` |
| `transformES2019/visitor` | 102916–102926 | `636a8b2e2c136373f836ccbcb26c641029ebeb53365df73e5c63215ec4a315de` |
| `transformES2019/visitCatchClause` | 102927–102939 | `4d8ad35b3a76ff356ca808a361fab50d31c294f77175451839b44174998d49d0` |

ES2021 は assignment の receiver/key が単純でなければ `hoistVariableDeclaration` へ渡す。
既存 control の `get().value` はこの receiver 分岐に入る。`??=` は後続 ES2020 も通る。
ES2016 の property `**=` は receiver が identifier でも必ず hoist するため、既存 control の
`holder.value **= get()` が引数内 hoist を実際に要求する。element access では receiver と key の2個、
identifier への assignment では0個となるが、その expression lowering 自体は変更しない。
両 pass とも他 node は `visitEachChild` を通じて同じ `visitParameterList` 条件に達する。
ES2019 の optional catch は `createTempVariable(undefined)` で宣言し、hoist callback を呼ばない。
したがって同 pass が単独で parameter-hoisted flag を立てる分岐はない。
これらの upstream body は修正対象の共通 target 条件の適用範囲を示し、native 成功の代用にはしない。

architecture は `E-CONTEXT` の per-unit context と env lifetime、`E-NAMES-BASE` / `E-NAMES-H` の
typed identity と completed-tree naming を維持する。今回の修復は既存の target 条件を正しく適用するもの。
共有 architecture ファイルの編集は統合段階に予約し、実装中に Claude と共有ファイルを同時編集しない。

## 4. 具体的 witness 対応

下の ID は `parameter-temporaries/<name>/<target>`。明記がなければ ES5 と ES2015 の2件。
すべて fixture 内の完全 input/options/observation が正本。

| packet 群 | name | branch |
| --- | --- | --- |
| P2 | `simple-nullish`, `call-nullish`, `simple-optional` | temp 不要/必要 |
| P2 | `optional-call-receiver`, `optional-chain-receiver` | receiver を保持する call の違い |
| P3 | `pattern-default-computed` | computed key と pattern default が両方 temp を要求 |
| P3 | `multiple-rest`, `empty-pattern` | 複数 default の順序、rest、要素なし pattern |
| P4 | `arrow-concise-body`, `arrow-block-body` | body 側 temp と parameter 側 temp、body 変換 |
| P4 | `nested-functions`, `nested-parameter-function` | 子関数が親 parameter の hoist 計画に混入しない |
| P4 | `function-expression-collision`, `object-method`, `class-method` | 関数種別、同名 local、method |
| P5 | `exponentiation-assignment` | 同じ visitor の ES2016 pass |
| P5 | `logical-nullish-assignment`, `logical-and-assignment`, `logical-or-assignment`（ES5/ES2020） | ES2021 pass |
| P5 | `optional-target-boundary`, `nullish-target-boundary`（ES2019/ES2020） | ES2020 lowering と保持境界 |
| P6 | `comments-lf`, `comments-crlf`, `comments-removed` | parameter comments、prologue、line endings |
| P6 | `source-map`, `bom`, `no-emit`, `no-emit-on-error` | map、BOM、command 境界と診断 |

## 5. Native before と因果 gate

専用 strict test は `crates/compiler/tests/h2_5h_parameter_temporaries.rs`。
`original_parameter_commands` は12件、`focused_parameter_commands` は56件を各2回実行する。
失敗しても全 cases の actual/error/partial-writes/expected を保存してから group を失敗させる。
filter は調査専用で空選択を拒否する。完了証拠は filter なしの全68件。

`before-complete-20260914-1656` は全136 captures、37 exact / 31差分、exit 101。
原 ES2015/ESNext 8件は exact。原 ES5 4件の全 actual は §2 の counterfactual と完全一致した。
最初の run の test adapter 不備と修正は [report §3](h2-5h-parameter-temporaries-report.md#3-native-before) に残す。
修正後も先に取得した128 captures は全 field が不変だった。

`before-trace-20260914-1700` は許可2ファイルに一時的な読み取り用 trace を加え、同じ全68件を各2回実行。
exit 101、155.46秒。全136 captures が uninstrumented before と完全一致した。
trace は stderr と `trace-excerpts.json`、instrumentation は `working.patch` に保存した。
instrumentation を除去後、準備時27 source pins と再一致したことを確認した。

### C1: ES5 の parameter lowering を後続 ES2015 へ渡す

Rust の ES2020 pass は flag を `0 → 3 (IN_PARAMETERS | VARIABLES_HOISTED_IN_PARAMETERS) → 2 → 0`
と遷移させ、ES5 でも initializer を除去する。binding pattern の原ケースでは、local alias を
`GeneratedBindingId(0)` / provisional `_a` として予約し、その後の hoisted temp が id 1 / `_b` となる。
parameter は ObjectBindingPattern から Identifier に変わる。ES2015 の body 合流時は
`multiline=false` のまま。これが上流 ES5 trace と異なる最初の分岐である。

§3 の typed target の伝播と2つの guard を C1 として実装する。lowering の guard は上流の条件を復元し、
alias-plan の guard は ES5 で実行しなくなる lowering 用の予約を止める。hoisted temp 自体の
identity / lexical owner / materialization は維持する。両 guard を含む修正を全68件で確認し、
ES2015 側の C2 と原因別に記録する。ES2015 本体の production 変更は予定しない。

### C2: native parameter initializer の clone/range/flags

before には ES2015 の `comments-lf`、`comments-crlf`、`source-map`、`bom`、
`no-emit-on-error` にも差分がある。元 parameter の `/* after */` が initializer の末尾でも出力され、
body 先頭の `/* body */` が消える。同じ source の map positions にも差分がある。
これは C1 の ES5 target 条件とは別で、同じ `es2021.rs::lower_parameter_default` の initializer arm が owner。

上流 `range-trace.mjs` は comments-lf/source-map の ES5/ES2015 4件を記録し、無改造 TS と全 tuple が一致。
`range-trace.json` SHA-256 は `6c6448a566e8cf6ffff79f3fe75e52fadb44f15caebd7f34315c8e8cb4adddf7`。
`comments-lf/es2015` の parameter range は 111..156、initializer は 130..156（この input は ASCII）。

| node | TS trace | Rust before trace | C2 の変更 |
| --- | --- | --- | --- |
| 条件の name | positionless clone、original は元 Identifier | source identity のない新規 Identifier | `factory.clone_node(name)` |
| 代入の name | 同 clone、flags 96 (NoSourceMap) | 新規 Identifier、flags なし | clone + `set_flags(NO_SOURCE_MAP)` |
| initializer | range 130..156、flags 3168 (NoSourceMap + NoComments) | 同 range、flags なし | 既存 flags に両 flags を追加 |
| assignment | parameter range 111..156、flags 3072 (NoComments) | synthetic range、flags 3168 | parameter の text range + NoComments のみ |
| expression statement | synthetic range、flags なし | synthetic range、NoComments | 追加 flags を除去 |
| then block | parameter range、flags 3905 | synthetic range、同 flags | parameter の text range を付与 |

Rust の `u32::MAX` は TS の synthetic `-1` sentinel に対応する。trace はこの差を値の正規化で隠さない。
変更は pinned `addDefaultValueAssignmentForInitializer`（91239–91276、selection に既存 pin）に従う。
rest / binding-pattern の arm、context、factory、printer は変更しない。
共有 factory の `clone_node` と `set_text_range`、metadata の `set_flags` / `add_flags` をそのまま使う。
`standard_decorators.rs` の同 owner port も同じ clone/range 配置を使用していることを read-only で確認した。

追加 read-only helpers の body pins（AST getStart〜end）:

| owner | span | SHA-256 |
| --- | --- | --- |
| `createNodeFactory/cloneNode` | 24436–24466 | `04e6cc6ac32172f263e59d0ef1b89774f0e310425f3f99b6923286b5a3be59a7` |
| `setEmitFlags` | 25318–25321 | `74360b7b9b8d9725d16ddd1d9fd7c263293aed62f0d34ff196598a63893624ec` |
| `setTextRange` | 28256–28258 | `641fccddbba39e0b73a702f725eb9b0ffc3fa470e7ae811cac817d9323d8cbb4` |

`cloneIdentifier`（24409–24420）と `cloneGeneratedIdentifier`（24401–24408）の original / generated identity
維持も読解した。Rust factory は既存の typed original/metadata copy を持ち、その実装を置き換えない。
ledger hash は行 span の改行込み bytes を用いる。C2 helper は
`40b44ee01a36e04f01c1117674832a11a865d16bb40a5fc6c1da6395ad60d4a2`、
C1 の visitParameterList は既存 ledger 値と一致した。

**実装前 gate: 未解決0。** C1 と C2 の順に別 commit で実装し、それぞれ全68件の完全 command ×2 を確認する。
各中間結果は保持し、最終 after と区別する。新たな残差の owner が許可2ファイル外にある場合は
OUT-OF-SCOPE として記録し、期待値・比較を緩めない。

## 6. After / regression / handoff

原12件と追加56件の全 tuple ×2、許可範囲内の Rust instrumentation の除去、最終 source での再検証が必要。
既存 emitter の parameter/logical/optional/nullish/exponentiation、context/name/comment/token scope と
emitter library、H2.5h static-this/super、UTF-16 original rows、必要な map controls を確認する。
既存失敗や範囲外 owner は元の期待を保持して別記録し、成功件数に算入しない。
原因別 commit、完全 source/evidence hash、実 command/exit を report に残す。
global manifests、共有 index/architecture、PR/hosted は合流段階の担当とする。

2026-09-14 after 更新: C1 によって原4件を含む26件が exact、元37 positives は維持された。
C2 の clone/range/flags 修復後も ES2015 の5 control は compact body の `/* body */` 欠落を持つ。
これは上流 `emitNodeListItems` の intervening comment phase に対し、Rust printer が先頭 child
だけを拾う別 owner（C3）。合流後の元 statement/block/array range は上流と一致している。
[report §4](h2-5h-parameter-temporaries-report.md#4-原因別の変更と-after) に実測と OUT-OF-SCOPE を記録した。
全68件を strict 比較した結果は63 exact / 5差分で、runner の exit 101 を維持する。
この残差を expected value の書換えや ignore で通さない。原4件の exact と共通 producer の部分閉包を区別する。
