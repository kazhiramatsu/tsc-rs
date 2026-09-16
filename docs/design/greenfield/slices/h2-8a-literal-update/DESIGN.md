# C01 / A40-LITERAL-UPDATE — UTF-16 リテラル更新と template 伝播：現状対応表・契約・修復設計

提出時点の記録。現在の取り込み・CI登録・検証状況は [統合記録](integration/README.md) を参照。

作成日：2026-09-16。状態：隔離候補（research）。
依頼：[h2-8a-literal-update-claude-handoff.md](../h2-8a-literal-update-claude-handoff.md)、
共通手順：[claude-high-difficulty-handoffs.md](../claude-high-difficulty-handoffs.md)。
結果は [REPORT.md](REPORT.md)、統合仕様は [INTEGRATION.md](INTEGRATION.md)。
本書は production readiness や accepted profile の admission を主張しない。

## 1. 開始点と保存物

| 項目 | 値 |
| --- | --- |
| worktree / branch | `~/dev/tsc-rs-literal-update` / `draft/h2-8a-literal-update` |
| 開始 SHA | `6c41a03888b66bb6781e5ef39c253b6200ff44c6`（origin/main。C05 merge `5f6e681af`、SUPER `f6444330`、PR #537/#538 を含むことを `merge-base --is-ancestor` で確認） |
| toolchain / pin | rustc 1.93.0、cargo 1.93.0、node v25.2.1、`_tsc.js` `1c59e77a…ddd3e3`、`typescript.js` `56917765…12be39`（[records/start.txt](records/start.txt)） |
| 入力 manifest | [inputs.v1.json](inputs.v1.json)：全 1,418 ID（factory 987、transform 399、pipeline 22、lifetime 10）を native 実行前に固定 |
| observer | `scripts/observe-literal-update.mjs`（4 group、各 case を同一 process で 2 回観測して一致を要求、`--check` は別 process で byte 一致） |
| 凍結 expected | `crates/emitter/tests/fixtures/literal-update-{factory,transform,lifetime}.json`、`crates/compiler/tests/fixtures/literal-update-pipeline.json` |
| Rust target | `crates/emitter/tests/literal_update_contract.rs`（factory / transform / lifetime、3 tests）、`crates/compiler/tests/literal_update_pipeline_contract.rs`（1 test） |
| 既存の凍結 expected | 変更なし（`template-raw-provenance` 480、`string-property-provenance` 60、`utf16-literal-escaping` 288+8、`literal-parent-provenance` 128、`string-literal-identifier-source` 72、`utf16-tagged-template` 16、`utf16-literal-witnesses` 64、`h2-8a-require-rewrite` 60+6） |

## 2. Source graph（pinned `_tsc.js`、span = 行範囲、hash = span の SHA-256）

| 入口 | 行 | SHA-256 | 意味 |
| --- | --- | --- | --- |
| createStringLiteral | 21529-21534 | 2bf21e80…ed35eb… (`2bf21e80bf4e61e4e1af7273cc968a2d4423ba01535d7cedc31a7ed35ebc1c2e`) | 値・`singleQuote`（undefined 可）・`hasExtendedUnicodeEscape`（真なら ContainsES2015） |
| createStringLiteralFromNode | 21535-21543 | `a2fa6c4e9dd96af89655a0a7d44368bcdd05ad599a3ef7e898a2a64e3e5fe9ee` | `textSourceNode`、singleQuote undefined |
| checkTemplateLiteralLikeNode | 22843-22861 | `9603d422336731c448450113e36065ed0c16bc73e505d1932814d0d2fb41253a` | checked constructor：raw あり時は cooked 一致を assert、text 省略時は cooked を採用 |
| getTransformFlagsOfTemplateLiteralLike | 22862-22868 | `3bf354b5a17106c73ec99978fbd0777ec96d73561293a73283a2682785d44109` | ES2015、templateFlags≠0 なら ES2018 |
| createTemplateLiteralLikeToken / Declaration / Node | 22869-22890 | `7205fd63…`, `51ce17cc…`, `4d36f6cd637eb6babb29850129ab9b8a3bfea4f9e238b375705907258faf9a2b` | unchecked constructor：text / rawText（absent と "" は別）/ `templateFlags & 7176` |
| createTemplateHead … createNoSubstitutionTemplateLiteral | 22891-22906 | `66009daa…`, `3b90057d…` | checked constructor 群 |
| updateTemplateExpression / updateTemplateSpan / updateTaggedTemplateExpression | 22840-22842 / 23037-23039 / 22653-22655 | `dc770076…`, `f362c435…`, `60f6f412…` | 子のみ変更、同一なら同一 node |
| update | 24995-25001 | `384440fe1fa8372895737f3042fe78d813be2d2c8cffa728d419bdfc9dd67707` | `setOriginal` → `setTextRange` |
| cloneNode | 24436-24466 | `d223dcea6ccf14e9212d40d5b8df188197023622ea3e5d624ffb974a25db19d6` | own property（text/rawText/templateFlags/singleQuote/textSourceNode/hasExtendedUnicodeEscape）を複製、`original` を設定、pos/end は -1 |
| setOriginalNode / mergeEmitNode | 25208-25217 / 25218-25277 | `8ef5d40b…`, `6d9f4af1…` | emitNode（flags、comments、ranges…）の merge。`original` は emitNode ではなく node 直属 |
| disposeEmitNodes | 25302-25310 | `0f82231f…` | parse tree に登録された annotated node の emitNode だけを消す |
| getLiteralText / canUseOriginalText | 13647-13688 / 13689-13702 | `35659715…`, `bf211667…` | 範囲＋parent＋非 synthesized なら source verbatim、template は `rawText ?? escape(text)` |
| getLiteralTextOfNode（printer） | 120467-120479 | `43989b908107b6f48eae6547835a82a24937f43ba2ddd8673c48b83018d8201e` | textSourceNode の優先 |
| getSourceTextOfNodeFromSourceFile / getTextOfNodeFromSourceText | 13017-13019 / 13035-13044 | `c2d2a4cb…`, `5b825e21…` | `skipTrivia(pos)..end`。pos<0 は "" |
| processTaggedTemplateExpression / createTemplateCooked / getRawLiteral | 93972-94018 / 94019-94021 / 94022-94032 | `d318d253…`, `1f8f38ee…`, `d4e11c6faf9f995a3cafd841ab9f3aaabfcd7e1c3d56e530c21538f79f1bf2bf` | IsInvalid → `void 0`；raw は `rawText`、無ければ source slice（synthetic かつ range 無しは ""）、`/\r\n?/g`→LF、`setTextRange` |
| hasInvalidEscape | 16270-16272 | `fcf1a345…` | ContainsInvalidEscape の fragment 探索 |
| rewriteModuleSpecifier | 93242-93248 | `f922e640861acb3c4f3e223a052ecf480ebdc989e9c1d4b545efca742e40aace` | 値変更 = `setOriginalNode(setTextRange(createStringLiteral(updated, node.singleQuote), node), node)` |
| visitTemplateLiteral / visitStringLiteral / visitTemplateExpression（es2015） | 107912-107914 / 107915-107920 / 107937-107952 | `fde517b6…`, `681f6439…`, `f5dfead2…` | cooked のみ消費、fresh + setTextRange |
| visitTaggedTemplateExpression（es2018） | 95150-95158 | `5a3b7097…` | LiftRestriction |

固定 TypeScript は、string / template literal の値を**その場で書き換える**操作を持たない
（transformer / printer 範囲 93000-121000 で `.text =` 代入は generators の数値 label 1 箇所のみ）。
「値の更新」は常に fresh node + `update()`（setOriginal + setTextRange）である。
同値の update は `updateX` が同一 node を返すことで表現され、leaf literal には `updateX` が無い。

## 3. 現状対応表（旧要求 → 現行 owner / caller → 既存 witness → 分類）

分類：**A** 既存観測済み、**B** 追加対照が必要（本 slice で追加）、**C** 差を再現（本 slice で修復）、
**D** 到達前提が未成立（direct control のみ、production caller なし）。

| # | 旧要求 | 現行 owner / caller | 既存 witness | 分類 | 本 slice |
| --- | --- | --- | --- | --- | --- |
| 1 | createStringLiteral の値・quote | `NodeFactory::create_string_literal[_from_code_units]`、`LiteralNodeProperties::string_literal_single_quote` | string-property-provenance 60、utf16-literal-escaping 288+8 | A | factory group が origin/op 軸を追加 |
| 2 | createStringLiteralFromNode / textSourceNode | `set_string_literal_text_source`（builtins/es2018/generators/flatten/standard_decorators）、printer 2925-3060 | string-literal-identifier-source 72、literal-parent-provenance 128 | A | — |
| 3 | template constructor（raw absent/empty、UTF-16 raw） | `create_template_literal_like_from_code_units`（owned raw = `LiteralNodeProperties::raw_template_text`、NodeData `raw_text` は lossy projection）、`create_template_head`（projection のみ） | template-raw-provenance 480 | A | — |
| 4 | templateFlags の synthetic producer | なし（factory は常に 0）。upstream の synthetic template producer（111097 import-call、51578 checker）も flags 無し | template_flags.rs（parsed → clone / cross-source の保持） | D | typed update の忠実な `createTemplateLiteralLikeNode(kind,text,raw,flags)` として `create_template_literal_like_node` を追加（transform flags ES2015｜ES2018） |
| 5 | 子のみ変更（updateTemplateSpan / updateTemplateExpression / updateTaggedTemplateExpression） | `update_node` + `update_node_array`（visit_each_child 経由） | 明示的 witness なし | B | pipeline group（ES2020 `??` の span 内 lowering、3 target）＋ transform group `children` op |
| 6 | 同値 update = identity | `update_node` の `record.data == data` | — | B | factory/transform `same` op |
| 7 | 値のみ変更（StringLiteral、generic） | `relative_imports::rewrite_literal`（`update_node` で値差し替え：clone が `textSourceNode` を運ぶ） | h2-8a-require-rewrite 60+6（ASCII、quote/拡張 escape の対照なし） | C（direct）/ A（pipeline） | factory `text-source/cooked` 7 行で再現。`update_node` の property 整合と `update_string_literal` への移行 |
| 8 | raw のみ変更（template、generic） | `update_node`：clone が旧 owned raw を運び、projection だけ更新 → printer / tagged-template は旧 raw を読む | なし | C（direct） | factory 144+12 行で再現。`update_node` の raw 整合（projection 変更 → owned raw を追従、None → 消去） |
| 9 | 正確な UTF-16 raw / flags / quote の更新 | 表現手段なし（NodeData は lossy `String`、flags / quote は property） | なし | C（API 不在。generic 205 行が n/a） | typed update `update_template_literal_like_node` / `update_string_literal`（同値判定は code unit 完全一致） |
| 10 | 孤立 surrogate 同士の lossy 衝突 | `String::from_utf16_lossy` projection の比較 | なし | C | typed update は unit 比較。generic は projection 不変 = raw 不変と定義（§4.3） |
| 11 | cooked のみ変更 | `set_literal_value`（in-place、Rust-only；upstream 対応物なし）と generic/typed update | 単体 test 2 件 | D | factory `cooked` op を unchecked constructor `create(kind, newText, oldRaw)` と対照 |
| 12 | clone / setOriginalNode の own property と emitNode | `clone_node`（NodeData、template_flags、properties、original）、`set_original_node`（`merge_from`） | template-raw-provenance clone / set-original 行 | A | factory `clone`/`set-original` origin、`node-no-ascii` policy で merge 方向を追加 |
| 13 | cross-source clone | `clone_node_to_source`（Rust seam） | template_flags.rs | B | factory `cross-source` origin（cloneNode semantics と比較） |
| 14 | getRawLiteral の raw fallback | `tagged_template::template_fragment_texts`：projection も owned raw も無い fragment を typed error に | utf16-tagged-template 16、utf16-literal-witnesses 26 | C（direct） | transform `raw-absent` 行で再現（upstream は `setTextRange` 済みの fresh fragment を source slice から復元）。source-slice fallback を実装 |
| 15 | createTemplateCooked の IsInvalid → void 0 | `create_template_cooked` | 同上 | A | transform `flags` op で synthetic flags 経路を追加 |
| 16 | CR / CRLF → LF | `get_raw_literal` | utf16-literal-witnesses crlf 行 | A | transform/pipeline crlf 行（synthetic raw の CR も） |
| 17 | 再 print | printer は node state のみ参照 | — | B | 全 factory/lifetime 行で 2 回 print 一致 |
| 18 | TransformationResult dispose | `clear_session_metadata`（全 metadata を消去；literal property と NodeData は arena 所有で残る） | — | B | lifetime group。upstream は parse-tree node の emitNode のみ消去（synthetic は残る）：Rust session model の既知差として記録、修復対象外 |
| 19 | update 後の別 pass | 先行 pass の update を後続 ES2015/ES2018 pass が消費 | — | B | transform group（custom pass → 組込み pass） |
| 20 | sourceMap | 完全 command の map bytes | utf16-literal-witnesses | A | pipeline group（sourceMap:true、declarationMap:true） |

## 4. 契約

### 4.1 所有者

| 事実 | 所有者 | 寿命 |
| --- | --- | --- |
| cooked 値（UTF-16、孤立 surrogate 可） | `NodeData::{StringLiteral,NoSubstitutionTemplateLiteral,TemplateHead,TemplateMiddle,TemplateTail}.text: JsString` | arena（node） |
| raw の正確な UTF-16 | `LiteralNodeProperties::raw_template_text: Option<JavaScriptString>` | arena（node property、dispose を越える） |
| raw の projection | `NodeData::*.raw_text: Option<String>`（parsed は source slice そのもの＝正確、synthetic は lossy） | arena（node） |
| templateFlags | `Node::template_flags`（`TokenFlags::TEMPLATE_LITERAL_LIKE_FLAGS` mask） | arena（node） |
| quote / textSourceNode | `LiteralNodeProperties::{string_literal_single_quote, string_literal_text_source}` | arena（node property） |
| hasExtendedUnicodeEscape | `StringLiteralData::has_extended_unicode_escape` | arena（node） |
| original / emit flags / comments / ranges | `EmitMetadata`（session metadata） | session（dispose で消える） |
| pos / end | `Node::{pos,end}`（synthetic は `u32::MAX`） | arena |

### 4.2 typed update（本 slice で追加）

`NodeFactory::update_template_literal_like_node(original, text: &[u16], raw: Option<&[u16]>, template_flags: TokenFlags)`
= upstream `update(createTemplateLiteralLikeNode(kind, text, rawText, templateFlags), node)`。

- 同値判定：`text` の code unit 完全一致、`raw` の完全一致（`None` と `Some(&[])` は別。原 node の raw は owned raw、無ければ projection の UTF-16）、
  `template_flags & 7176` の一致。すべて一致なら **同一 handle**（`updateX` の identity 規則）。
- 変更時：fresh node（`create_template_literal_like_node`：owned raw と projection を同時に設定、flags mask、
  transform flags = ES2015｜(flags≠0 ? ES2018)）→ `set_original_node(fresh, original)` → `set_text_range(fresh, original)`。
  clone ではないので、原 node の property（textSourceNode 等）は運ばれない。NodeFlags は Synthesized のみ。
- 種類は原 node から取る。4 kind 以外は `FactoryTokenKindExpected`。

`NodeFactory::update_string_literal(original, text: &[u16], single_quote: Option<bool>, has_extended_unicode_escape: Option<bool>)`
= upstream `update(createStringLiteral(text, isSingleQuote, hasExtendedUnicodeEscape), node)`。

- 同値判定：text 完全一致、`single_quote` の `Option<bool>` 一致（parsed / `createStringLiteralFromNode` は `None` = undefined）、
  `has_extended_unicode_escape` の `Option<bool>` 一致。
- 変更時：`create_string_literal_node`（quote は Some のときだけ property を作る、has_ext=Some(true) は ContainsES2015）→ finish_update。
- 実 caller 移行：`relative_imports::rewrite_literal`（rewriteModuleSpecifier）を
  `update_string_literal(node, updated, node.singleQuote, None)` へ。以前の generic 経路は `has_extended_unicode_escape` と
  `textSourceNode` を clone で運んでいた（出力差なし、tree 差あり）。

### 4.3 generic update（`update_node`）の literal 規則（本 slice で整合）

- identity：`NodeData` と transform flags の完全一致（従来どおり）。template の `raw_text` projection は
  「projection が不変なら raw は不変」と定義する。lossy な projection では表現できない raw 変更
  （孤立 surrogate、実 U+FFFD との区別）は typed update の責務であり、generic は使わない。
- StringLiteral の値変更：clone 後に `string_literal_text_source` を消す（fresh `createStringLiteral` に textSourceNode は無い）。
  `single_quote` は保持（`createStringLiteral(text, node.singleQuote)` と同値）、`has_extended_unicode_escape` は payload の値。
- template の raw 変更：payload の `raw_text` projection が原 node の projection と異なるとき、owned raw を
  payload から作り直す（`Some(s)` → `JavaScriptString::from_rust_str(s)`、`None` → 消去）。projection が同じなら owned raw を保持。
- template flags / quote は payload に無いので generic では変更できない（typed update）。

### 4.4 consumer

- `tagged_template::template_fragment_texts`：raw は owned raw → projection → **source slice fallback**（getRawLiteral 13017-13044、
  94022-94032）：node に実 range があれば `skipTrivia(pos)..end` を kind に応じて delimiter 1（先頭）＋1/2（末尾）だけ
  JS `substring` 規則（引数の入替と clamp）で切り出す。range が無い synthetic は ""（upstream と同じ）。typed error は廃止。
- printer `emit_template_literal_token`、`create_template_cooked`、`get_raw_literal`、es2015 `visit_template_*` は変更しない
  （差なし。§3 の A 行と本 slice の after 観測が根拠）。
- `set_literal_value` は変更しない（Rust-only in-place seam；production caller なし。契約：cooked のみ、raw / flags / property は保持）。

## 5. 対象外・未到達

- Unicode writer 全体、checker / syntax の改修、custom transform API の admission。
- checker `node_builder/type_nodes.rs:133` の `create_template_head(target, data.text, data.raw_text)`（parsed の projection をコピー、
  正確）と 2060/2081 の `create_template_literal_like_from_code_units(raw None)`（checker `createTemplateHead(texts[0])` と同じ raw 無し）は
  差の根拠がないため対象外（別差分の候補にもしない）。
- templateFlags を持つ synthetic template の production producer は upstream にも無い（D）。
- dispose 後の synthetic node emitNode 保持（upstream）と Rust の全 session metadata 消去は session model の差：記録のみ。
