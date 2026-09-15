# Claude 実装依頼：A6-41-SUPER — decorator の static super と評価順序

作成日：2026-09-14。種別：**隔離候補の実装依頼・research**。
親スライスは H2.8a / A6-41。`A6-41-SUPER` はこの依頼の識別子であり、
新しい runtime admission や ready packet を表すものではありません。

## Claude に渡す依頼文

この資料に従い、standard decorator 変換の **static `super` の read / call /
tag / assignment / update / destructuring と、それを支える訪問・評価順序**を
実装・検証してください。固定 TypeScript 6.0.3 の source owner から再現例を
作り、必要な設計、隔離候補パッチ、修正前後の完全な観測、回帰検証を提出してください。
調査報告だけで終了せず、原因ごとにレビューできる実装候補まで仕上げてください。

開始点は下記の v18 + テスト修正です。root checkout の production source に
候補は適用されていません。新しい worktree で作業し、§2 の入力照合を済ませてから
変更してください。全体の design/readiness gate は開いているため、成果物は
隔離候補として提出します。root への適用・profile 更新・PR/merge はこの依頼の
完了条件に含めません。

## 1. 選定理由と境界

前回の実測は **530/530 完全コマンド一致 × 2、退行 0** です。ただしこれは既存の
観測集合に対する結果で、A6-41 の全経路を網羅した証明ではありません。
今回確認した v18 の `DecoratorReceiverFrame` は `class_this` だけを保持し、
`visit` に `super` の read / call / assignment / update 専用分岐がありません。
一方、upstream の frame は `classThis` と `classSuper` の両方を運びます。
後続の class-fields 変換で扱える範囲を含め、どこで差が観測されるかを固定します。

このスライスは receiver、基底の identity、computed key の評価回数、値を使うか
捨てるか、一時変数、複数変換 pass、source map が同時に関わります。
単純な `this` の置換では閉じず、Claude に依頼する高難度のまとまりとして選定しました。
**新規 witness の失敗件数は未計測**です。ソース上の差を既存 530 件の退行と
言い換えないでください。

今回閉じる範囲：

- standard decorators の static property initializer / static block から到達する
  `super` の各式形と、arrow・入れ子 class をまたぐ receiver/base の選択。
- 上記の再現例に必要な class decorator → heritage → non-constructor members →
  constructors の訪問順、pending expression と生成 binding の順序。
- 書き込み式の値の使用・破棄、computed key の保存、destructuring target wrapper、
  helper と source-map provenance、変更した状態の復元。

親 A6-41 に残す独立項目：FileLevel census / global-name oracle 全体の刷新、
全 producer を対象にした memoization・identity の監査。今回作る `super` 用 binding や
訪問 mode がこれらに依存する場合、その依存部分の witness と修正は今回に含めます。
依存して失敗した行を「別項目」として成功件数から外さないでください。
独立な未完了行は親スライスに明記し、A6-41 全体や H2.8 全体の完了とは報告しません。

## 2. 正確な開始点と復元

証跡・パッチを保持するコミット：

```text
repository: /Users/hiramatsu/dev/tsc-rs
branch at preparation: work/h2-8-output-matrix
evidence commit: 6e298cda8dbf362f61ce6a8e690ec145270681df
```

履歴と測定の基準：

| 記録 | 意味 |
| --- | --- |
| [attempt62 receipt](../../../../ratchets/h2-8a-decorator-receiver-design-experiment.v1.json) | v18 production、530/530 × 2、旧 477 件不変、53 件修復、退行 0、実 exit 0 |
| [attempt65 receipt](../../../../ratchets/h2-8a-decorator-receiver-emitter-checks.v1.json) | 同一 production + テスト修正。494 library tests / 452 contracts 成功。1350 declaration reprints、8 template controls、static-accessor 4 条件は重複を含むため加算しない |
| [前回の実装・レビュー記録](h2-8a-decorator-receiver-frames.md) | 初期 v17、外部候補、独立再計測、テスト訂正の経緯と未解決 owner |

Receipt の SHA-256：

```text
attempt62: 77f5440820c9c2ab3e9933533021684b67252b79c30e0545f3d1ac981578cf7d
attempt65: 186ed40ee40b7b41df4e60a23a33d8f340ef39c0da02a575df74eeab8b994580
```

**2026-09-14 の現物確認**：attempt62 / 65 の一時保存先に
`source-and-inputs.tar.gz` は存在せず、attempt62 の `captures/` にファイルはありません。
履歴の成功記録と、生キャプチャを現在再監査できることは区別してください。
一方、`target/h2-8a-retained-lexical-design-workspace` は attempt65 の
`prelaunch.inputs` **1014/1014 ファイルで SHA 一致**、root の vendor 入力も
**109/109 一致**でした。既存 `../tsc-rs-dec53` は accessor fixture がなく、
`active_transform_contract.rs` も異なるため、そのまま開始点にしません。

共有 workspace は後続 runner が上書きするので、次の復元経路を使ってください。
このパッチ適用順は本資料の作成時に新規一時ディレクトリで実行し、
**復元後の 1014 入力すべてが attempt65 manifest に一致**することを確認済みです。
Cargo / Node の実行結果を新たに主張する確認ではありません。

1. 上記 evidence commit から新規 worktree を作る。

   ```sh
   git worktree add -b draft/h2-8a-decorator-super ../tsc-rs-dec-super 6e298cda8dbf362f61ce6a8e690ec145270681df
   ```

2. 新 worktree 内で、`crates/emitter/src/builtins/class_fields.rs` を
   [retained lexical candidate v2](h2-8a-retained-lexical-owners.candidate-v2.rs.txt)
   の内容に置き換える。
3. 以下を **各パッチにつき `git apply --check` → `git apply`** の順で適用する。

   | 順序 | パッチ |
   | --- | --- |
   | 1 | [comma printer](h2-8a-comma-printer.candidate.patch) |
   | 2 | [comma argument factory](h2-8a-comma-argument-factory.candidate.patch) |
   | 3 | [literal property observation](h2-8a-literal-property-observation.candidate.patch) |
   | 4 | [統合済み list owner v18](h2-8a-list-intervening-printer.candidate-v18.patch) |
   | 5 | [UTF16 tests v4](h2-8a-utf16-writer-tests.candidate-v4.patch) |
   | 6 | [static-accessor tests v2](h2-8a-decorator-static-accessor-test.candidate-v2.patch) |

   v18 は receiver と static-accessor production の修正を含みます。
   それらの個別 candidate patch や旧 v17 を追加適用しないでください。
4. vendor が新 worktree にない場合は、root から attempt65 の
   `prelaunch.vendor_inputs` に列挙された相対パスをコピーし、SHA を照合する。
   `prelaunch.inputs` の不足ファイルも source checkout から補い、1014 件を照合する。
   シンボリックリンクで可変な別 workspace の production を参照しないこと。
5. 復元差分を候補ベースとして保存し、以後の実装差分と分ける。
   最終成果物には evidence commit → 復元ベースと、復元ベース → 新候補の両方を残す。

新 worktree の root で使う入力確認コマンド：

```sh
python3 - <<'PY'
from pathlib import Path
import hashlib, json
receipt = Path('ratchets/h2-8a-decorator-receiver-emitter-checks.v1.json')
assert hashlib.sha256(receipt.read_bytes()).hexdigest() == '186ed40ee40b7b41df4e60a23a33d8f340ef39c0da02a575df74eeab8b994580'
pre = json.loads(receipt.read_text())['prelaunch']
for group, count in [('inputs', 1014), ('vendor_inputs', 109)]:
    rows = pre[group]
    assert len(rows) == count
    bad = [r['path'] for r in rows if not Path(r['path']).is_file()
           or hashlib.sha256(Path(r['path']).read_bytes()).hexdigest() != r['sha256']]
    assert not bad, (group, bad)
    print(f'{group}: {count}/{count} exact')
PY
```

主要な復元後 production SHA-256：

```text
standard_decorators.rs: 7872d2f6eb27e03aa8bf74fc0affe0f30cca00ecd7dbd412240c475f80fbfdfe
class_fields.rs:        c423c24425c14a1201920514db3d9dd7ea235ffafc24ea8b803eab9552148081
class_fields/downlevel.rs: 506ea9864f9e71cf58b991d05beff14ced5c3fdcd1ed93a53af74579dd1bc0b9
```

## 3. Source owner と Rust の入口

権威の順序は [design index](../../README.md)、
[emitter architecture](../emitter-architecture.md)、
[schedule §1.1](../post-h1-completion-slices.md#11-mandatory-implementation-ready-design-gate)
を参照してください。研究候補を `active-qualified` と扱わないこと。

Pinned source は [vendor `_tsc.js`](../../../../vendor/typescript-6.0.3/lib/_tsc.js)。
全体 SHA-256 は
`1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3`。
以下の行番号はこのファイル用で、observer が実行する `typescript.js` の行番号では
ありません。observer は実際に読み込む `typescript.js` 自体も hash 固定します。

| Source 入口 | 読み取って設計に固定するもの | 対応する Rust 候補の入口 |
| --- | --- | --- |
| `updateState` 98973、`enterClassElement` 99007、`shouldVisitNode` 99058 | `classThis` / `classSuper` の取得条件、Name の第三先行 frame、lexical flags による到達性 | `DecoratorReceiverFrame`、`update_receiver_state`、`enter_receiver_class_element`、`visit` |
| `transformClassLike` 99319 | class decorator → heritage → class entry → member 二巡 → pending expression → exit。匿名 heritage の named evaluation 抑止も追う | `transform_class_like`、`prepare_class_super`、`prepare_decorators_and_computed_names` |
| `visitor` 99061、`discardedValueVisitor` 99183 | 使用/破棄 mode の伝播、通常関数と arrow、visit の省略条件 | `StandardDecoratorVisitor::visit` と追加する typed value-use 経路 |
| `visitCallExpression` 100154、tag 100165、property 100182、element 100192 | read の receiver、`.call` / `.bind`、各 node の original/text range | standard-decorator の専用式訪問を追加。後続 downlevel の同種処理と区別 |
| `visitBinaryExpression` 100249、unary 100312、comma list 100341 | setter key / getter key の共有、結果 temp、prefix/postfix、comma の末尾以外を破棄 | typed value-use、generated binding、既存 class-fields の `visit_discarded_value` 等を比較 |
| `visitDestructuringAssignmentTarget` 100394 と pattern/rest 群～100488 | setter wrapper の parameter、default/rest/computed name の訪問順、入れ子 pattern | class-fields の `create_assignment_target_wrapper` を調査。下流へ丸投げしない |
| `createReflectGetCall` 24601、`createReflectSetCall` 24604、`createAssignmentTargetWrapper` 24754 | helper の AST 形状、引数、parameter scope | factory / typed AST 構築と emit helper の既存境界 |
| `expandPreOrPostfixIncrementOrDecrementExpression` 27499、`visitCommaListElements` 91306、`isSimpleInlineableExpression` 93030、`isCompoundAssignment` 93033 | callee と predicate の分岐。値の使用と key 再評価を自己流に簡略化しない | 新しい update lowering と既存 `ExpressionValueUse` / binding owner |

行番号は入口です。実装前に必要な callee/predicate の範囲と hash、
source → Rust → witness の対応を実装メモに固定してください。
特に compound/logical assignment は演算子名から期待値を推測せず、
前後の pass と固定 upstream の実際の出力を確認します。

Rust の具体的な参照ファイル（リンク先は root、編集は復元済み worktree 内）：

- [standard_decorators.rs](../../../../crates/emitter/src/builtins/standard_decorators.rs)：主要な変更 owner。
  v18 の `prepare_class_super` は 1829 行、frame は 619 行付近。
  `prepare_class_super` 呼出が class decorator の変換より先にある点を検証する。
- [class_fields.rs](../../../../crates/emitter/src/builtins/class_fields.rs) と
  [downlevel.rs](../../../../crates/emitter/src/builtins/class_fields/downlevel.rs)：
  `StaticBindingFrames`、`StaticSuperPolicy`、`StaticSuperAccessResolution`、
  `visit_discarded_value`、`create_reflect_get/set`、assignment/update/wrapper の既存処理。
  `E-CAPTURE-CLASS-G` の legacy-decorated recovery と standard decorator を混同しない。
- [target_bindings.rs](../../../../crates/emitter/src/builtins/target_bindings.rs)：
  temp の宣言・参照を同じ binding に結びつける既存 owner。
- [factory.rs](../../../../crates/emitter/src/factory.rs)、
  [metadata.rs](../../../../crates/emitter/src/metadata.rs)：
  factory-time flags、original、text/map/comment range の境界。

## 4. 実装順序と設計上の条件

1. **復元と before 固定。** §2 の照合後、新規 witness の source 観測を採取し、
   復元ベースで native before を実行する。過去の生キャプチャが欠けているため、
   既存 530 件も一度取り直す。履歴 full62 と同じ 530/530 を確認し、capture を保持する。
2. **state / 訪問順。** `classThis` と `classSuper` を source の同じ境界で選択する。
   raw name の検索で receiver を復元しない。class decorators と heritage は
   外側の frame で source 順に訪問し、必要な member 二巡と pending の順を保つ。
   `_outerThis` が必要になる場合は生成条件・挿入順・復元も witness に含める。
3. **read / call / tag。** static initializer / block を先に閉じ、arrow と入れ子の
   computed name / heritage / body へ拡張する。property-get と invocation の range は
   source の設定先が違うため、JS 一致だけで止めない。
4. **write / update と value-use。** `Used` / `Discarded` 相当の typed 状態を導入する。
   expression statement、`for` の initializer/incrementor、parenthesized /
   partially-emitted expression、binary comma / comma list の伝播を扱う。
   key、RHS、結果 temp の生成順・実行回数を source に合わせる。
5. **destructuring。** array/object、default、rest、入れ子の target を区別し、
   property read に変換してしまわない。wrapper parameter は hoisted result temp と
   同一 scope にしない。後続 ES2015 / object-rest 等との合成を観測する。
6. **最終候補の固定と検証。** 新規 battery と既存 530 件、隣接 emitter suites を
   最終 production bytes に対して実行し、§7 の成果物をそろえる。

`nodes: BTreeMap<NodeId, ...>` の memo に新しい value-use mode を無条件に混ぜないこと。
同じ node の別 mode / receiver 訪問が到達するなら key・寿命の設計と direct control を
追加します。到達しないなら producer と呼出順の根拠を残します。
「念のため」の全体 cache 改修や classThis clone の機械的撤去はしません。
早期 `Result::Err` で継続する経路には frame・pending・temp scope の復元を保証し、
visitor 全体が破棄される経路とは分けて記録します。

## 5. Witness の設計

共通の options 軸は **target = ES2015 / ES2022 / ESNext ×
useDefineForClassFields = false / true**、module = ESNext、sourceMap = true。
standard decorators を使い、`experimentalDecorators` は legacy 対照だけで切り替えます。
complete-command observer の host、lib、診断・callback 契約は既存方式を使います。

新規 case ID は `decorator-super/<target>/<set|define>/<family>/<variant>`。
以下の表を実行前に具体的な入力 manifest に展開し、全 ID と予定件数を固定します。
family 一行を一ケースとは数えず、property/element、使用/破棄等の差は別 ID にします。

| Family | 必須 variant と確認点 |
| --- | --- |
| `read` | `super.x`、`super[key()]`。static field と block、getter 内の `this` |
| `call-tag` | property/element method call、property/element tagged template。key → args/template の順と receiver |
| `assign` | `super.x = rhs()` / `super[key()] = rhs()`、値を `record(...)` へ渡す場合と statement として捨てる場合 |
| `compound` | property/element の `+=`、代表の算術・bitwise・指数、key が literal / identifier / call。get/set と key/RHS の回数 |
| `logical` | `&&=` / `||=` / `??=`、RHS が必要/不要な値、使用/破棄。upstream の pass 合成も記録 |
| `update` | prefix/postfix `++` / `--`、property/element、使用/破棄。Number と BigInt を分離 |
| `discard` | 二重括弧、comma の左/右、`for` initializer/incrementor、arrow の expression body と block body |
| `array-target` | `[super.x] = values`、default、hole、`[...super.x] = values`、nested array、effectful element key |
| `object-target` | `({x: super.x} = value)`、default、computed property、`({...super.x} = value)`、nested object/array |
| `receiver` | static arrow、object method / 通常関数の own receiver、instance field/method、static method、入れ子 class の heritage / computed name / body |
| `phase-order` | decorator expression と heritage の両方に decorated class を含め、member decorator / computed name / constructor を混在させる。生成名割当と実行順を別々に見る |
| `handoff` | class decorator による class replacement、static private field/accessor の有無、member-only decorator、undecorated / legacy 対照 |
| `fault` | `noEmitOnError` と出力 callback failure。実診断・失敗 boundary・partial writes を保持 |

source input の出発例（**未計測。golden ではありません**）：

```ts
const events: unknown[] = [];
function key() { events.push("key"); return "x"; }
function rhs() { events.push("rhs"); return 3; }
function record(value: unknown) { events.push(value); }
function dec<T extends new (...args: any[]) => any>(value: T) { return value; }
class Base {
    static stored = 1;
    static get x() { events.push(["get", this.name]); return this.stored; }
    static set x(value: number) { events.push(["set", this.name, value]); this.stored = value; }
}
@dec
class Derived extends Base {
    static {
        record(super[key()] += rhs());
        super[key()]++;
        [super[key()]] = [rhs()];
        record((() => super.x)());
    }
}
record(Derived.stored);
```

この複合例を操作ごとの最小ケースへ分けます。診断があれば source 観測に残し、
正常例・エラー回復例を混ぜないこと。通常関数への receiver 非伝播は有効な own
`this` / own `super` を持つ対照で確かめ、無効な lexical `super` だけに依存しません。

新しい期待値は [既存の complete-command observer](../../../../scripts/observe-decorator-receiver-context.mjs)
を参考に、固定 vendor から **2 回一致**で採取します。新規 observer のファイル名、
入力/出力先、件数 assertion は新規集合用に定義し、旧 observer の 36 件を変更しません。
JS、map、reported diagnostics、emit-result diagnostics / emitted files / maps、
ordered write callback、status、exit、exception、partial writes を比較します。
upstream exception は独立に数え、完全コマンド一致に加点しません。

評価回数の主張には、実行可能な ES2015 / ES2022 出力を同じ runtime で動かした
event log の source/native 比較も付けます。これは補助的な runtime control であり、
compiler の完全タプルや ESNext の出力一致の代わりにはしません。
共有 node / partially-emitted / synthetic comma-list の control も、通常の Program
コマンド数とは別に報告します。

## 6. 変更面と検証コマンド

主な隔離候補の編集対象は `crates/emitter/src/builtins/standard_decorators.rs`。
依存部分に必要な変更は `class_fields.rs`、`class_fields/downlevel.rs`、
`target_bindings.rs`、`factory.rs`、`metadata.rs` を owner 調査後に扱います。
変更前に file / symbol / source owner / 必要な witness を設計メモへ記載し、
変更理由のない共有 owner の整理を混ぜないでください。

新規 test は独立した `crates/compiler/tests/decorator_super_contract.rs`、
`crates/emitter/tests/decorator_super_contract.rs`、
`crates/compiler/tests/fixtures/decorator-super-*.json`、
`scripts/observe-decorator-super.mjs` 等として追加できます（いずれも今回作成するファイル）。
既存 compiler contract の観測 adapter を参考にし、比較項目を減らさないこと。
共有 `contracts.rs` の登録変更は独立 integration target が適さない場合に理由を記録します。
既存 fixture・過去 patch・過去 receipt は保持します。ratchet、profile、pin、
admission、hosted acceptance、canonical runner の書換えはこの候補の作業面に含めません。

次は **復元済み worktree** で実行する既存回帰の正確な入口です。
`DEC_SUPER_RUN_DIR` は実行ごとに作る空の絶対パス、`DEC_SUPER_TARGET_DIR` は
今回専用の Cargo target パスを指定します。実行前 manifest、argv/env、時刻を保存し、
コマンドの実 exit code を直後に記録する runner で実行してください。

```sh
env CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$DEC_SUPER_TARGET_DIR" \
  TSC_RS_RETAINED_ACCESSOR_CASE_SET=all \
  TSC_RS_H2_8A_CAPTURE_WRITES_DIR="$DEC_SUPER_RUN_DIR/captures" \
  TSC_RS_H2_8A_FAILURE_DIR="$DEC_SUPER_RUN_DIR/failures" \
  taskpolicy -b nice -n 15 cargo test --offline -p tsc-rs-compiler \
  --test contracts -- retained_accessor_owners_match_complete_typescript_observations \
  --nocapture --test-threads=1

env CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$DEC_SUPER_TARGET_DIR" \
  taskpolicy -b nice -n 15 cargo test --offline --no-fail-fast \
  -p tsc-rs-emitter --lib --test contracts -- --test-threads=1
```

最初のコマンドは既存 **530 件 × 2** です。編集ループでは同じ test の selector を
`context`（36 件）/ `additional`（46 件、upstream exception 2 件は別）に変えられます。
新規集合は別 integration target で実行し、530 の件数 assertion を薄めません。
2 番目は既存 **494 / 452** が基準。追加 test 数や影響した隣接 suite を別記します。
ESNext static accessor の既存 4 条件は `SourceFileTextMode::Canonical` を使います。

既存の [隔離 runner](../../../../scripts/run-comma-printer-design-experiment.py) は
共有 workspace を root + 固定 patch から作り直します。今回の編集をそこへ置いて
同 runner を再実行すると変更が上書きされるため、上記の専用 worktree と実行記録を
使ってください。新規 ID と保存先を使い、attempt1–65 を再利用しません。
重いコマンドは一度に一つ、`taskpolicy -b` / `nice -n 15` / jobs=2。
実行中の handle を最後まで追い、入力を変更しません。

## 7. 完了条件と提出物

完了は次の全条件を満たした隔離候補の提出です。

1. §5 を展開した全 witness に source owner と disposition が付き、新規の source/native
   完全観測が 2 回一致する。対象を減らしたり typed refusal を成功扱いしない。
2. 最終 candidate で既存 **530/530 × 2** を維持する。今回再採取した before と
   完全観測を比較し、case ID ごとの regression 0 を確認する。
3. 隣接 emitter regression、変更 owner の focused tests が成功する。
   production 編集後の古い binary の結果を最終結果として再利用しない。
4. source → Rust → witness の対応、state/順序/名前/range の設計、変更ファイルの理由、
   復元ベースとの差分 patch と再適用手順を提出する。
5. before/after 入力 SHA、期待 fixture SHA、実行 binary SHA、実 exit、ログ、
   全 capture、比較 script、source snapshot を永続的な成果物ディレクトリに保存する。
   receipt が存在しない一時パスだけを指す状態にしない。
6. 成否を `完全コマンド / runtime control / direct metadata control / test suite` に
   分け、未計測項目と親 A6-41 に残る独立項目を明記する。

新規 before に差がなければ、観測で到達した owner と分岐を記録し、残る未到達経路を
追加測定します。それでも既存処理が十分だと分かった部分は、不要な修正を作らず
到達性・同値性の根拠と回帰 control を成果物にします。反対に、scope 外の owner が
修正に不可欠なら依存と再現例を保存し、このスライスを完了扱いしません。

本資料の準備作業では production の実装・新規 witness の実行をしていません。
確認済みなのは source / 候補の入口、保存物の現存状態、復元手順と入力 SHA、文書リンクです。

## 8. 完了記録（2026-09-15、Claude 実装）

隔離候補として提出済み。作業 worktree は `~/dev/tsc-rs-dec-super`（ブランチ
`draft/h2-8a-decorator-super`、復元ベース commit `13f5767ec`）。root production、
profile、ratchet 値、pin、hosted acceptance は変更していない。

| 母集団 | before | after（最終候補、`after-16`） |
| --- | --- | --- |
| 新規 witness 672（完全コマンド、upstream exception 2 件は加点なし） | 152 exact ×2 / 518 失敗（復元ベース） | 670 exact ×2 / 0 失敗 |
| 追加 control 42 | 7 exact / 35 失敗（復元ベース） | 42 exact ×2 |
| 追補 witness 156（2026-09-15 追加、before = `after-10` 候補バイト） | 31 exact / 125 失敗 | 156 exact ×2 |
| 追補 2 witness 162（named evaluation 21 variant + private 名 6 variant、before = `after-13` 候補バイト） | 38 exact / 124 失敗 | 150 exact ×2 / 12 失敗（すべて ES2015 の隣接 owner、§8.2） |
| 既存 530 | 530 exact | 530 exact ×2、退行 0 |
| runtime control（event log） | — | primary 553/553、extra 35/35、followup は receipt 参照 |
| direct control（synthetic comma-list / shared node、JS テキスト） | — | 28 exact ×2 + required 2 回到達 4 件は記録済み divergence（加点なし） |
| emitter 隣接 suite（lib / contracts / direct） | 494 / 452 | 494 / 452 / 1、exit 0 |

### 8.2 追補 2（2026-09-15 夜）: 「残る未計測」3 件

1. **parameter default の named evaluation**: 実装した範囲は parameter だけでなく
   tsc の esDecorators が扱う全 source（parameter / binding element の default、変数宣言、
   `=`・`&&=`・`||=`・`??=` 代入、property assignment（識別子・string・numeric・private・
   computed literal・computed 式 = `[_a = __propKey(k)]`）、`export default`）。outer
   expression（括弧・partially emitted）を剥がして判定（設計メモ決定 19）。派生で
   helper 要求順（class decorator の `__esDecorate`/`__runInitializers` と宣言名の
   `__setFunctionName` を member 訪問後へ、決定 21）、`__propKey` temp の nested-scope
   予約（決定 22）、ES2021 論理代入結果の範囲撤去（決定 23、`es2021.rs` 独立 patch）を修正。
2. **割り当て名・private 名の escape**: 割り当て名の source（識別子/literal node、
   `__propKey` temp）を `__setFunctionName` へ配線、private descriptor の
   `__setFunctionName(function, "#\\u006d")`・context `name`、printer の private 名の
   source 綴り（`printer.rs` 独立 patch、決定 20）、private auto-accessor storage 名の
   source 綴り（決定 24）。
3. **required 2 回到達する shared node**: direct control `shared-node-used-twice` を追加。
   tsc は 2 回 lowering（`var _a, _b;`）、port は memo により 1 回目を再利用（`var _a;`）。
   テストは port の出力を tsc 出力の書換えとして pin し、加点なしの記録済み divergence
   として扱う（producer が存在しないため memo 方針は変更せず、親 A6-41 の
   memoization 監査に残す）。

witness: `decorator-super-followup2` 27 variant × 6 = 162（fixture 4 本目、zstd）。
残 12 件（ES2015 のみ）は隣接 owner: `param-default-undecorated-control` の map
（class-fields / ES2015 の parameter default lowering）と `private-name` 5 variant の
`__classPrivateFieldIn`/`__classPrivateFieldGet` helper 要求順（`class_fields/downlevel.rs`）。
いずれも設計メモ §6 に証跡付きで記録。
**→ §8.3 で全て閉じた（2026-09-15 深夜）。**

### 8.3 追補 3（2026-09-15 深夜）: 残 12 件（ES2015）を閉じた

§8.2 の残 12 件は全て exact になった（設計メモ §5.2、決定 25–28）。

1. **`param-default-undecorated-control` ×2（map のみ）**: owner は
   `class_fields/downlevel.rs` 自身の parameter default lowering
   （`lower_parameter_default`: 代入全体に `NoSourceMap`、代入と block に text range なし）と
   `install_function_bindings`（concise body の return statement に range なし）。tsc の
   `addDefaultValueAssignmentForInitializer` / `convertToFunctionBlock` と同じ range/flag に
   揃えた（決定 25、standard_decorators 側の決定 14 と同形）。
2. **`private-name` 5 variant ×2（helper 順）**: downlevel の要求順は最初から tsc と同じ
   （In → Get → Get）で、原因は **printer 側の並べ替え** `order_private_field_helpers`
   （w5 lane S `6e4516d38`、ES2022 未満で `__classPrivateFieldIn` を get/set の後ろへ移動）。
   tsc は relocated static（static block / static field initializer）を member pass と
   constructor の**後**に訪問する（`addPropertyOrClassStaticBlockStatements`）ので、
   downlevel は static 訪問中の helper 要求を parking（`visit_relocated_static` →
   `TransformationContext::defer/resume_emit_helper_requests`、入れ子は stack）し、
   instance operations の後で再発行（`request_relocated_static_helpers`）。printer の
   並べ替えは撤去（決定 26、`helper-request-order` patch = `transform.rs` + `helpers.rs`）。
   撤去前の並べ替えは、plain class の `static { #x in this } static { this.#x }` のような
   relocated static 同士の順を反転させていた（followup3 で観測）。
3. **新規 witness `decorator-super-followup3`**（family `helper-order`、8 variant × 6 =
   48、tsc 観測 48 完全 / diag 0 / 例外 0）。plain 5 形 + decorated 3 形。この集合が
   隣接 2 件を露出させ、同じ train で修正: lowered `#x in obj` の余分な map 2 本
   （`transformPrivateIdentifierInInExpression` は `setOriginalNode` のみ、決定 27）、
   ES2022+ set-mode で synthetic constructor が class-this / named-evaluation transport
   block より前に出る・pending static block が constructor より前に出る
   （`transformClassMembers` の並び、`arrange_synthetic_members`、決定 28）。

| 母集団 | before | after（最終 `after-18`） |
| --- | --- | --- |
| 追補 2 witness 162 | 150 exact / 12 失敗（`after-16`） | **162 exact ×2 / 0 失敗** |
| 追補 3 witness 48（before = `after-16` production + 同一 test file、base worktree） | 26 exact / 22 失敗（JS 差 8: helper 順 4 + synthetic 順 4、map のみ 14） | **48 exact ×2 / 0 失敗** |
| 新規 672 / 追加 42 / 追補 156 / 既存 530 | — | 670 (+例外 2) / 42 / 156 / 530、退行 0 |
| runtime control | — | primary 553/553、extra 35/35、followup 130/130、followup2 115/115、followup3 40/40 |
| emitter 隣接 suite / clippy / fmt | 494 / 452 / 1 | 494 / 452 / 1、clippy `-D warnings` green、fmt clean |

検証の経緯: `after-17`（決定 25–26 のみ）で全 chain green → followup3 追加 → 決定 27–28 で
production 変更 → `after-18` で全母集団を再 replay（`run-after-18-chain.sh`、`run-followup3.sh`、
`finalize-4.sh`）。receipt `ratchets/h2-8a-decorator-super-design-experiment.v1.json`
（SHA-256 `1cd9b70839fe032933cc5ddc2cd3f79c2f4685c9e1568879da56e650c245bfbc`）。
候補 patch は 8 本（`candidate-helper-request-order.patch` を追加、`witness-tooling` は
followup3 の manifest / 観測 / test / script を含む）。root production・profile・ratchet 値・
pin・hosted acceptance は変更していない。

方針メモ（ユーザー指示、2026-09-15）: 重い replay（530 / 672）は hosted CI へ。今回は
after-17 が green になった後に隣接 2 件を同乗させたため全 chain を二周した。以後は
focused set（対象行 + 追補 witness + emitter suite）で編集し、大きな replay は最終バイトで
一回、または hosted に委ねる。なお hosted 入口 `cargo xtask acceptance` は ts-tests 由来の
suite のみで、`decorator_super_contract` 等の witness target は含まれない（workflow 変更は
ユーザー判断）。

### 8.1 追補（2026-09-15 後半）で閉じた残項目

前回報告の「未計測・残項目」はすべて実装・計測した（設計メモ §3 決定 14–18、§4、§6）。

1. **es2015 `object-target/rest` の map 差 2 件**: owner は `es2018.rs` の plan 型
   `flatten_destructuring_assignment` が結果に代入式全体の範囲を再設定していたこと
   （tsc の `inlineExpressions` は範囲を付けない）。再設定を撤去（決定 18、独立 patch
   `candidate-es2018-flatten-range.patch`）。primary 670/670。
2. **arrow の parameter default 内の super update（VariablesHoistedInParameters）**:
   `visit_arrow_function` が parameter 訪問後に flag を見て
   `addDefaultValueAssignmentsIfNeeded` を適用（決定 14）。加えて
   `visitParameterDeclaration` 自体が未移植で、更新された parameter 名の
   `NoTrailingSourceMap` が欠けていた（決定 15、read-only control でも観測）。
   witness family `param-default` 10 variant × 6 = 60、before 10/60 → after 60/60。
3. **unicode escape 付き識別子名**: `createStringLiteralFromNode` の text source を
   super key / decorator context `name` / `has` / literal 名の `get`/`set` /
   `__setFunctionName` に配線（決定 16）。string / numeric member 名の helper 変数名
   （`getHelperVariableName` → `member`）も修正（決定 17）。family `unicode-name`
   15 variant × 6 = 90、before 15/90 → after 90/90。
4. **decorated computed field + lexical `this` + 後続 computed name なし**: arrow 形式は
   extra witness で既に exact（memo §6 の記述は古かった）。class declaration 形の
   witness を追加（6/6 exact、修正不要）。
5. **synthetic comma-list / shared node の direct control**: tsc 側は
   `customTransformers.before` で注入、Rust 側は parse 済み arena を組み替えて
   `get_script_transformers` を実行し JS を比較（`crates/emitter/tests/decorator_super_direct_contract.rs`、
   7 形 × 2 target × 2 mode = 28 exact ×2）。memo 方針（required は node id で memo、
   discarded は迂回）で tsc と一致。required 2 回到達は producer が存在しない旨を記録。
6. **clippy 3 件**（`class_fields.rs` の cast 2 件・type complexity）: 修正、
   `-D warnings` green。
7. **fixture 16 MB**: 観測 fixture 3 本を zstd 圧縮（`decorator-super.json.zst` 84 KB、
   extra 19 KB、followup 28 KB）。replay は実行時に復号、capture の `fixture_sha256` と
   receipt の `fixtures_decoded_sha256` は復号後 JSON の SHA（前回 receipt の pin と一致）。

成果物:

- 設計・実装記録: [h2-8a-decorator-super.md](h2-8a-decorator-super.md)。
- 候補パッチ（復元ベースとの差分、順に `git apply --check` → `git apply`）:
  `candidate-standard-decorators`、`candidate-class-fields-downlevel`、
  `candidate-class-fields-visitor`、`candidate-es2018-flatten-range`、
  `candidate-printer-private-name`、`candidate-es2021-logical-range`、
  `candidate-witness-tooling`（`h2-8a-decorator-super.candidate-*.patch`）。
- receipt: `ratchets/h2-8a-decorator-super-design-experiment.v1.json`。
- 新規 test target `crates/compiler/tests/decorator_super_contract.rs`（3 set）、
  `crates/emitter/tests/decorator_super_direct_contract.rs`、fixture
  `crates/compiler/tests/fixtures/decorator-super{,-extra,-followup}{-inputs.json,.json.zst}`、
  `crates/emitter/tests/fixtures/decorator-super-direct.json`、observer
  `scripts/observe-decorator-super{,-direct}.mjs`、manifest 生成
  `scripts/generate-decorator-super-inputs.mjs`（`--extra` / `--followup`）、
  runner / 解析 / runtime control / receipt の `scripts/decorator-super-*.{py,mjs}`。
- 生 capture・log・実行 binary: worktree の `target/h2-8a-decorator-super/`
  （`runs/after-13-*`、`runs/before-followup`、`bin/`、`logs/`）。

親 A6-41 残項目: FileLevel census / global-name oracle の刷新と producer 横断の
memoization・identity 監査（shared node の required 2 回到達はその監査に含める）。
隣接 owner の open row（ES2015 の 12 件）は §8.3 で閉じた（設計メモ §6）。
