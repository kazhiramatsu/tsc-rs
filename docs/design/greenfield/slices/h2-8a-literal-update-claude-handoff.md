# Claude 実装依頼①：A40-LITERAL-UPDATE — UTF-16 更新と template 消費

作成日：2026-09-14。親：H2.8a / A6-40。状態：隔離候補の research。


**2026-09-15 更新**：開始点と検証分担は[共通手順](claude-high-difficulty-handoffs.md)の最新版に従います。
SUPER 統合後の main の SHA を固定し、ローカルは新規失敗・関連 owner の focused set、
重い全件 replay は hosted で実行します。以下の技術要件は現行実装と照合し、既実装部分を再実装しません。


**2026-09-16 次スライス**：C05はPR #539で統合済み。Claudeへ次に送る依頼は本C01。
最新mainから新しいworktreeを作り、開始SHAを固定してください。C05の作業treeへ重ねません。
統合担当のAPI1.2-HINT（PR #537）とA-PC1（PR #538）も開始点に含めます。
C05 merge `5f6e681af9dd338d816b00582a39021e3dbc0741`を含むことを確認してください。

再確認した入口は `factory::update_node`（同値なら同一node、変更時はcloneしてpayload更新）、
`clone_node`（literal propertiesとtemplate flagsを保持）、`LiteralNodeProperties::raw_template_text`
（正確なraw UTF-16を所有）、`tagged_template::template_fragment_texts`（owned rawを優先）、
`create_template_cooked`（`IS_INVALID`を消費）です。これらの存在自体は新規失敗の証明ではありません。
cooked / raw projection / owned raw / flagsの変更操作が交差する境界を追加観測してください。

既存の `literal-value-provenance`、`literal-parent-provenance`、`utf16-literal-escaping`、
`string-literal-identifier-source`、`utf16-review-fix`、`utf16-tagged-template` は
`python3 scripts/witness.py <suite> --list`で入口を確認できます。
新規fixtureはこれらの凍結expectedを書き換えず追加し、最終提出時に専用CI入口の所有path・
実行件数・想定時間も示してください。全件hostedは統合担当が登録・実行します。


**2026-09-16 提出状態**：本依頼の隔離候補を [h2-8a-literal-update/](h2-8a-literal-update/DESIGN.md) に提出済み
（[REPORT.md](h2-8a-literal-update/REPORT.md)、[INTEGRATION.md](h2-8a-literal-update/INTEGRATION.md)、
[inputs.v1.json](h2-8a-literal-update/inputs.v1.json)、[records/](h2-8a-literal-update/records/)）。
開始 SHA `6c41a03888b66bb6781e5ef39c253b6200ff44c6`、worktree `~/dev/tsc-rs-literal-update`、branch `draft/h2-8a-literal-update`（未 commit）。
commit / PR / hosted 入口の登録は統合担当（A-INT1）。

## Claudeへの送付用要約

次のスライスとして **C01 / A40-LITERAL-UPDATE：UTF-16リテラル更新とtemplate伝播の残経路監査・修復** をお願いします。
最新の `origin/main` をfetchし、開始SHAを記録した専用branch/worktreeで進めてください。
API1.2-HINT（PR #537）、A-PC1（PR #538）、C05統合（PR #539）を含むmainが開始点です。
既存のC05 worktreeは使い回さず、旧UTF-16 patchも再適用しません。

**目的**：literalの値を更新した後、cooked値・raw値・templateFlags・quote/textSourceNode・
originalの関係が固定TypeScript 6.0.3と同じ条件で保持または更新され、
後続printer / tagged-template変換へ正しく届くことを確認し、再現した差を修復すること。
`JsString`移行、templateFlagsの保持、`IS_INVALID`の消費は既に実装済みです。

進める順序は次のとおりです。

1. **現状の対応表**：旧要求、現行owner/caller、既存witness、追加確認が必要な境界を整理する。
   「既存観測済み／追加対照が必要／差を再現／到達前提が未成立」を分ける。
2. **beforeの固定**：同値更新・異値更新、cookedのみ・rawのみ・flagsのみの変更、raw未指定と空文字列、
   clone/update/cross-source/disposeの操作列について入力IDと件数を先に固定し、nativeを2回観測する。
   孤立surrogate同士と実U+FFFDを区別し、raw projectionとowned rawを別々に確認する。
3. **必要な修復**：差が出たproducer/update/consumerのownerを修復する。
   新しいtyped updateが必要なら、その根拠・同値判定・保持/更新規則・実caller移行を示す。
   差のない経路は根拠と対照を残し、不要なAPIや再実装は追加しない。
4. **afterと隣接回帰**：direct factory/printerと、到達するtagged-template経路を最終bytesで検証する。
   source Programへ到達する経路は完全command観測も比較する。Rust-only errorとnative例外は別集計にする。

**提出物**：`DESIGN.md`、現状対応表、入力/期待値と全ID・件数、before/afterのログ・観測・argv/env/exit、
必要なcandidate patchとSHA-256、`REPORT.md`、`INTEGRATION.md`。
残件と未到達の前提、新規CI入口のtarget/所有path/実行件数/想定時間も記載してください。
既存の凍結expectedは維持し、新規対照は追加ファイル/追加IDで保存してください。

ローカルは新規失敗と変更ownerのfocused setを低優先度・2 workers以下で確認します。
重い全件replay、commit/PR、CI入口の登録・hosted確認・本番統合は統合担当が進めます。
Unicode writer全体、checker/syntaxの無関係な改修、全custom-transform APIのadmissionは対象外です。
以下の詳細仕様と[共通手順](claude-high-difficulty-handoffs.md)を併せて参照してください。

## 依頼

UTF-16 literal の値を変更したときの AST 更新と、後続 tagged-template 変換への
cooked/raw/templateFlags の伝播を実装・検証してください。
固定 TypeScript の factory・printer・transformer を調べ、型と更新規則を定義し、
source 由来の再現例、隔離 candidate patch、修正前後の観測と回帰検証を提出してください。
単なる文字列の上書きではなく、値・provenance・flags の所有者を明確にしてください。

[共通手順](claude-high-difficulty-handoffs.md)の開始点で
`draft/h2-8a-literal-update` / `../tsc-rs-literal-update` を作り、現在の source/input manifest を保存します。
旧 v18 の復元や旧 UTF-16 patch の再適用は、新規依頼の開始手順に含めません。

## 現在の開始状態と残経路の確認

main の UTF-16 統合（[PR #521](https://github.com/kazhiramatsu/tsc-rs/pull/521)）で、
literal/template の `NodeData` は cooked 値を `JsString` で所有し、parser/factory が
`template_flags` を保持しています。`tagged_template::create_template_cooked` も
`TokenFlags::IS_INVALID` を読みます。旧依頼の「lossy な String の同値判定」や
「tagged-template が flags を読まない」という前提は、現在の開始状態ではありません。

`factory::update_node` は `NodeData` と transform flags が同じなら元 identity を返し、
変更時は clone して payload を更新します。`LiteralNodeProperties` には quote/text-source/raw
等の property が残るため、値・raw・flags・provenance を別々に変える操作列と
clone/cross-source/dispose の連鎖を現行コードに対して調べます。

最初に旧要求・現行 owner・既存 witness の対応表を作り、既に観測済みの経路を区別してください。
追加対照で差が再現した部分について必要な typed update や consumer 修復を行います。
新規の失敗数は未計測です。新 API の追加や既存 consumer の再実装を先に結論としません。

## 対象と source/Rust 対応

Source の行番号は pinned `_tsc.js` 用です。共通手順の SHA を確認してください。

| Source 入口 | 意味 | Rust 入口 |
| --- | --- | --- |
| `createStringLiteral` 21529、`createStringLiteralFromNode` 21535 | 値と quote/text-source の所有者 | [factory.rs](../../../../crates/emitter/src/factory.rs)、[metadata.rs](../../../../crates/emitter/src/metadata.rs) の `LiteralNodeProperties` |
| template constructors 22877–22903、`updateTemplateExpression` 22840 | checked/unchecked constructor、raw の absent/empty、templateFlags、update identity | `create_template_literal_like_from_code_units`、`finish_update`、`update_node` |
| `cloneNode` 24436、`setOriginalNode` 25208 | own property と emit metadata の違い | `clone_node`、`clone_node_to_source`、`set_original_node` |
| `getLiteralTextOfNode` 120467 | source-text eligibility、textSourceNode、escaping | [printer.rs](../../../../crates/emitter/src/printer.rs) の literal worker |
| `processTaggedTemplateExpression` 93972、`createTemplateCooked` 94019、`getRawLiteral` 94022 | invalid cooked → void zero、raw fallback、CR/CRLF → LF、range | [tagged_template.rs](../../../../crates/emitter/src/builtins/tagged_template.rs)、[es2018.rs](../../../../crates/emitter/src/builtins/es2018.rs)、[es2015.rs](../../../../crates/emitter/src/builtins/es2015.rs) |

callee と predicate の範囲/hash を実装メモへ追加します。
`getRawLiteral` の改行変換は upstream 自身の操作です。比較結果を後処理で正規化しません。
scope は literal の更新と上記 consumer です。Unicode writer 全体の再実装や
新 public API の安定化、全 custom transform の admission は含めません。

## 実装手順

1. parsed/synthetic/clone/update/cross-source clone の literal property と raw syntax
   projection を別々に観測する adapter を作り、before を固定する。
2. 既存 update で表現できる操作を確認し、必要なら値を変更する typed update と、
   子・flags だけを変更する generic update の契約を定義する。
   明示的 UTF-16 値、raw の absent/empty、flags を表現する。型名は Rust 側で決めてよい。
   一致判定に `String::from_utf16_lossy` を使って異なる値を同一視しない。
3. 実 caller を列挙し、新しい値を作る更新と既存 property を保持する更新に分類して移行する。
   synthetic overlay を一律消去したり、すべて `setOriginalNode` からコピーしたりしない。
4. 既存 template_flags の scanner/parser/factory producer と更新経路を追い、
   clone/update 後も source と同じ条件で消費されることを確認する。
5. ES2015/ES2018 の既存 cooked/raw consumer を検証し、再現した差をその owner で修復する。
   source が invalid cooked を `void 0` にする条件、raw fallback と改行処理を区別する。
6. 同一値 update、別値 update、clone、dispose、再 print、後続 pass の連鎖を最終 bytes で検証する。

編集候補：`factory.rs`、`metadata.rs`、`printer.rs`、`builtins/tagged_template.rs`、
`builtins/es2015.rs`、`builtins/es2018.rs`。必要な公開 re-export は `emitter/src/lib.rs`。
syntax/checker に変更が必要なら、具体的な caller・source owner・影響テストを設計へ記載して
別差分にします。新しい node property を session metadata に戻さないでください。

## 必須 witness

case ID は `literal-update/<kind>/<origin>/<operation>/<variant>`。
表を具体的入力に展開し、実行前に全 ID と件数を固定します。

| 軸 | 必須の対照 |
| --- | --- |
| 値 | ASCII、BMP、補助平面、孤立 high/low surrogate、実 U+FFFD。異なる孤立 surrogate 同士が同じ lossy UTF-8 になる対照 |
| kind | StringLiteral、NoSubstitutionTemplateLiteral、TemplateHead/Middle/Tail |
| update | 同値、cooked のみ変更、raw のみ変更、raw absent↔empty、flags のみ変更、子のみ変更 |
| provenance | parsed、fresh synthetic、clone、setOriginalNode、textSourceNode の連鎖、cross-source clone |
| lifetime | arena clone、TransformationResult dispose、同じ literal の再 print、update 後の別 pass |
| template | valid/invalid escape、CR/LF/CRLF、escaped delimiter、tagged/untagged、expression span と nested tag |
| route | direct factory/printer、ES5 の ES2015 lowering、ES2015/ES2018/ESNext の該当経路、sourceMap |

最小 direct witness の出発点は、`[0xD800]` を持つ literal を `[0xD801]` に変更し、
UTF-16 の値、UTF-8 projection、node identity、出力、generated column を比較する操作列です。
これは未計測の入力案です。named TypeScript factory API と Rust generic update は形が違うので、
同じ意味の更新操作を定義して比較し、TypeScript にない API の完全一致とは呼びません。

source input から到達しない factory 操作は direct control として数えます。
source Program に到達する consumer は完全 command 観測も追加します。
invalid handle の typed error は Rust-only control に分離します。

## 検証と提出

新規 target 案：`crates/emitter/tests/literal_update_contract.rs`、
`crates/compiler/tests/literal_update_pipeline_contract.rs`。
新規 observer/fixture は `scripts/observe-literal-update.mjs` と
`crates/emitter/tests/fixtures/literal-update-*.json` 等。いずれも今回作るものです。

既存 focused test の入口（開始 SHA を固定した worktree、共通の低優先度/env で実行）：

```sh
cargo test --offline --manifest-path crates/emitter/Cargo.toml \
  --test literal_value_provenance_contract --test utf16_writer_contract \
  --test utf16_literal_escaping_contract --test literal_parent_provenance_contract \
  --test string_literal_identifier_source_contract -- --test-threads=1
```

ローカルは新規 target の focused set と変更 consumer の該当対照を実行します。
必要な全件 regression は共通手順に従って hosted で実行します。
旧 2155 / 530 / 494 / 452 は履歴の母集団であり、現在の実行数として引き継ぎません。

完了条件は全新規 row の disposition、選定 scope の観測一致 × 2、regression 0、
typed update の不変条件と caller 移行の説明、再適用可能な patch と保存済み観測です。
変わらないことが正しい経路は source 根拠と control を残し、不要な修正を作りません。
未到達 consumer や未設計の flags producer が残る場合は、その境界を未完了として報告します。
