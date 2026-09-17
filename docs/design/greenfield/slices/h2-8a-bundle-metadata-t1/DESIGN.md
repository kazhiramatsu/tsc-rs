# H2.8a-A-RES-BUNDLE-METADATA-T1 — 設計：bundle の parse-node `commentRange` 可搬性

作成日：2026-09-17。依頼書：[h2-8a-bundle-metadata-t1-claude-handoff.md](../h2-8a-bundle-metadata-t1-claude-handoff.md)。
結果と実行記録：[REPORT.md](REPORT.md)。記録：[records/](records/)。

## 1. Identity / purpose / boundary

| 項目 | 値 |
| --- | --- |
| slice ID / kind | `H2.8a-A-RES-BUNDLE-METADATA-T1` / `runtime`（限定修復候補） |
| 目的 | System bundle の JavaScript transform が parsed node に残す `emitNode.commentRange` を、同じ通常 emit の declaration transform へ引き継ぐ。開始点では `snapshot_parsed_emit_metadata` がこの field を `ParsedEmitMetadataNotPortable` で拒み、T1 5 件が typed error になる |
| 非目標 | C02 の再実装、R9（native decorator の default class 名 2 件）、R12（System bundle 文末 map 2 件）、dispose 後 print の typed 差分 2 件、`comment_range` 以外の未対応 field（synthetic comments / source-map range / helpers / …）の可搬化、SourceFile root の寿命変更、fresh getter / forced emit の寿命変更 |
| 前提 | PR #549（第 1 原因 `InternalEmitFlags` の可搬化）を含む main |
| trusted base（開始 SHA） | `84da0c0278c296fd295f15e48177ada87810f841`（[records/start-state.txt](records/start-state.txt)：toolchain rustc 1.93.0 / cargo 1.93.0 / Node 25.2.1、`_tsc.js` sha256 `1c59e77a…`、`typescript.js` sha256 `56917765…`） |
| activation 前 / 後 | `E-METADATA-BASE` の bundle packet 行：`active-qualified`（flags / internal flags / typeNode / constantValue）→ 候補では `modified-requalify`（`commentRange` を許可項目に追加）。統合担当が最終 validation ref で再 qualify する |
| 次の owner | 統合担当（合成・hosted CI・PR・admission）。残差 R9 / R12 / dispose は既存 owner のまま |
| authority artifact hash | 上流 span は §3（[records/upstream-spans.tsv](records/upstream-spans.tsv)、sha256 = 行を `\n` 連結 + 末尾改行）。入力 / 期待値 / observer の hash は [REPORT.md §2](REPORT.md) |

## 2. Required-reference table

| 参照 | 種別 | 状態 / ref | 現行 Rust symbol（開始 SHA） | 本候補での扱い |
| --- | --- | --- | --- | --- |
| [`E-METADATA-BASE`](../../emitter-architecture.md) | architecture | `active-qualified` @0653e10d | `tsc_emitter::TransformArena::{metadata,metadata_mut}`、`tsc_emitter::{EmitMetadata,CommentRange,CommentSourceRange}`（`crates/emitter/src/metadata.rs:133-149`） | `modified-requalify`：bundle packet の許可項目に `comment_range` を加える。`CommentSourceRange` の 4 状態はそのまま運ぶ |
| [`E-COMMENTS-G`](../../emitter-architecture.md) | architecture | `active-qualified` @6acd5d43 | printer `comment_range_for_node`（`printer.rs:16079-16102`）、`declarations::diagnostics::comment_range`（`diagnostics.rs:1135-1152`） | `premise-unchanged`：consumer は既存のまま。restore 後の値を読む |
| [`E-COMMENT-SCOPE-H`](../../emitter-architecture.md) | architecture | `active-qualified` | `CommentEmissionScope` / `EmitContext` | `premise-unchanged`（本候補は container claim を変えない） |
| H2.7d bundle packet（[REPORT §3 T1](../h2-8a-generated-binding/REPORT.md)、`crates/compiler/tests/h2_7d_declaration_bundles.rs:849/934`） | frozen contract | 既存 3 tests + fixture `bundle-maps.json` | `snapshot_parsed_emit_metadata` / `restore_parsed_emit_metadata`（`factory/parsed_metadata.rs`） | `premise-unchanged`：既存 fixture / 比較対象は不変（§8 隣接検証で再実行） |
| [A-INT3-CS](../h2-8a-printer-comment-carry/README.md) | historical rationale | landed PR #528 | `comment_cursor.rs` の UTF-16 container | rationale のみ（byte / UTF-16 の区別の前例） |
| C02 [REPORT.md §3 / §5](../h2-8a-generated-binding/REPORT.md)、[revised/README.md](../h2-8a-generated-binding/integration/revised/README.md) | frozen evidence | PR #549 | `decorator-binding-known-native.json` の T1 5 行 | 候補側で retire（§7） |
| [`docs/witness-testing.md`](../../../../witness-testing.md) | process | — | `scripts/witness.py` | 新 suite `bundle-metadata-t1` を登録（hosted job 登録は統合担当） |

歴史的文書（M-stage guide、h1-emit.md）は本候補の実装事実として引用しない。

## 3. Pinned upstream map（vendored TypeScript 6.0.3 `_tsc.js`）

hash は `records/upstream-spans.tsv`（`python3 span-hash.py`：関数開始行から brace 対応で終端を求め、行を `\n` 連結し末尾改行を付けた sha256。既存 `tsc-hash` 表記と同一規約、`emitLeadingCommentsOfNode` で照合済み）。

| 関数 | span | sha256 | 役割 / 分岐 |
| --- | --- | --- | --- |
| `setCommentRange` | 25362-25365 | `d6570cb1…` | `getOrCreateEmitNode(node).commentRange = range`：任意の node（parse node を含む）に書く |
| `getCommentRange` | 25358-25361 | `84e06c1d…` | `node.emitNode?.commentRange ?? node` |
| `getOrCreateEmitNode` | 25287-25301 | `973debb5…` | parse node なら所属 SourceFile の `emitNode.annotatedNodes` に登録してから `{}` を作る。`Immutable` の再変更は assert |
| `disposeEmitNodes` | 25302-25310 | `0f82231f…` | `getSourceFileOfNode(getParseTreeNode(sourceFile))?.emitNode?.annotatedNodes` の各 node の `emitNode` を `undefined` にする |
| `getParseTreeNode` | 11426-11437 | `80b5c244…` | synthesized node は `original` chain を辿る；Bundle（`createBundle`、synthesized、original 無し）→ `undefined` |
| `getSourceFileOfNode` | 12867-12872 | `58e6f8a8…` | parent chain で SourceFile を探す；`undefined` 入力 → `undefined` |
| `transformNodes` | 115977-116276 | `ef2079da…` | 開始時 `for (const node of nodes) disposeEmitNodes(getSourceFileOfNode(getParseTreeNode(node)))`（116050-116052）。Bundle root では no-op |
| `transformNodes.dispose` | 116261-116275 | `755ee15b…` | 同じ dispose ループ（116263-116265）。Bundle root では no-op：parse node の emitNode は Program の寿命まで残る |
| `emitJsFileOrBundle` | 116587-116639 | `8ddf52b0…` | `transformNodes(..., [sourceFileOrBundle], scriptTransformers)` → `printSourceFileOrBundle` → `transform.dispose()` |
| `emitDeclarationFileOrBundle` | 116640-116715 | `8275307f…` | `inputListOrBundle = outFile ? [factory.createBundle(filesForEmit)] : filesForEmit`（JSON を除く）→ `transformNodes(declarationTransformers)`；直前の JS transform が parse node に残した emitNode がそのまま見える |
| `printSourceFileOrBundle` | 116744-116804 | `46e5c19d…` | printer は `emitNode` を書かない（`getOrCreateEmitNode` を呼ばない）；probe の `after` 相 = snapshot 相 |
| `createPrivateIdentifierAccess` | 96401-96405 | `c8487979…` | `receiver = visitNode(receiver)`（parsed Identifier / ThisKeyword は同一 node が返る）→ helper へ |
| `createPrivateIdentifierAccessHelper` | 96406-96435 | `d66e2b7f…` | **producer**：`setCommentRange(receiver, moveRangePos(receiver, -1))` = `{pos: -1, end: receiver.end}`（END のみ）を receiver 自身に書く |
| `createPrivateIdentifierAssignment` | 96795-96840 | `50aca93f…` | 同じ producer（96808）。compound 代入は `createCopiableReceiverExpr` で clone してから書くため parse node には残らない |
| `moveRangePos` / `createRange` | 17304-17306 / 17297-17300 | `11da3d6e…` / `520b6961…` | `{pos, end}` の plain object；source identity を持たない |
| `emitCommentsBeforeNode` / `emitCommentsAfterNode` | 120987-120994 / 120995-121006 | `dc59a090…` / `f0baac32…` | **consumer**（printer）：`getCommentRange(node)` の `pos` / `end` で leading / trailing 相を claim |
| `emitLeadingCommentsOfNode` / `emitTrailingCommentsOfNode` | 121007-121032 / 121033-121046 | `ce6bf342…` / `e5c99d84…` | `pos < 0` → leading skip、`end < 0` → trailing skip；`(pos > 0 \|\| end > 0) && pos !== end` で container を claim |
| `forEachLeadingCommentToEmit` / `forEachTrailingCommentToEmit` | 121219-121233 / 121234-121238 | `2e1fb613…` / `bd6612ac…` | 位置は `currentSourceFile.text` の UTF-16 offset として読む（range 自身は source を持たない） |
| `preserveJsDoc` | 114803-114808 | `c7b4e75a…` | **consumer**（declaration transform）：`setCommentRange(updated, getCommentRange(original))` |
| `mergeEmitNode` | 25218-25277 | `6d9f4af1…` | `setOriginalNode` 時に original → 新 node へ `commentRange` を複写（parse node が宛先になる経路は無い） |
| `isIgnorableParen` | 24643-24645 | `8bbd6177…` | `nodeIsSynthesized(getCommentRange(node))`：synthesized 括弧の判定（parse node に無関係、記録のみ） |

`setCommentRange` の全呼出し（`_tsc.js` 内 34 箇所、[records/upstream-spans.tsv](records/upstream-spans.tsv) と `grep -n 'setCommentRange('`）のうち、宛先が parse node になり得るのは上記 receiver の 2 箇所のみ。`setCommentRange(updated, node)` 型は `updated !== node` の guard 付き（95045、98669、98750、99826、100219）、その他は factory が作った synthetic node が宛先（94508、95376、96285、96357-96358、97452、97483、98030、98633、98935、99659-99693、100132、103621-103675、106069-106125、106432、107959、108813、109283、112840、115715）。52400（checker node builder）と 134210（services）は emit 経路外。

失敗順序（T1 の再現）：JS transform で receiver に `{pos:-1,end}` が書かれる → print 完了 → Rust は declaration へ持ち越す packet を作る際に `comment_range` を拒否（`ParsedEmitMetadataNotPortable(TransformNode{source: 1, node})`）。上流に対応する失敗は無い（上流は何も検証せず emitNode を残す）。

## 4. Rust semantic map

| tsc object / field / transition | Rust 型・場所（候補） | producer | owner | updater | consumer | 寿命 / 無効化 | identity 観測 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `emitNode.commentRange`（parse node） | `EmitMetadata::comment_range: Option<CommentRange>`（`metadata.rs:497`） | `ClassFieldsDownlevel::set_private_receiver_comment_range`（`class_fields/downlevel.rs:7636-7657`、`create_private_get` 7673 / `create_private_set` 7700 から） | JS arena の side table | なし（再書込は同値） | printer `comment_range_for_node`（16079）、declaration `comment_range`（1135）/ `preserve_js_doc`（`subtree.rs:958`） | JS arena：`transformation.dispose()`（`execute.rs:1082`）まで。bundle：packet 経由で declaration arena へ（`orchestration.rs:188`）。SourceFile root：持ち越さない | 観測可能（probe の `after_javascript`、§6） |
| `{pos:-1, end}` / `{pos, end:-1}` / `{-1,-1}` / `{pos,end}` | `CommentSourceRange::{EndOnly, StartOnly, Synthesized, Original}`（`metadata.rs:144-149`、`SourceBytePosition` は PositionIndex で検証済み byte 位置） | 同上 | — | — | `has_nonempty_extent` 等 | — | 端点の有無を型で区別（上流の `-1` sentinel を型に写す） |
| range が指す text | `CommentRange::source: TransformSourceId`（上流は `currentSourceFile` で暗黙） | producer | arena | — | printer / declaration | arena 内 | **packet では `SourceFileId` として保存**（新 `ParsedCommentRange { source: SourceFileId, range: CommentSourceRange }`、`parsed_metadata.rs:83-86`） |
| bundle の `annotatedNodes` 残存 | `ParsedEmitMetadata { sources, nodes }`（`parsed_metadata.rs:37-40`） | `snapshot_parsed_emit_metadata`（91-141；`execute.rs:1079-1080`、JS print 後 dispose 前、`TransformRoot::Bundle` かつ declaration path ありのみ） | `execute.rs` の unit loop | — | `restore_parsed_emit_metadata`（143-208；`orchestration.rs:187-189`、mount 後 transform 前） | declaration transform の arena まで | `(SourceFileId, NodeId)` + `ParsedSourceIdentity`（snapshot ptr / lease / node interval） |
| 許可項目の判定 | `remainder` whitelist（`parsed_metadata.rs:100-110`）：flags / internal_flags / type_node / constant_value / **comment_range** | — | — | — | — | — | 他 field は `ParsedEmitMetadataNotPortable` を維持 |
| mount table | `mounted: BTreeMap<SourceFileId, TransformSourceId>`、`mounted_source` closure（173-178；未登録なら `ParsedEmitMetadataSourceMismatch`、panic しない） | restore | restore | — | node / type_node / comment_range の再 mount | restore 中 | mount 順序非依存 |
| 部分書込み禁止 | `restored: Vec<(TransformNode, EmitMetadata)>` を全 resolve 後に `extend`（202-207） | — | — | — | — | — | 失敗時に arena 不変（unit test） |

## 5. Current local-gap matrix（開始 SHA で生成）

| row | 現行 symbol | 分類 | 証拠 |
| --- | --- | --- | --- |
| producer（parse node に END のみ） | `set_private_receiver_comment_range` | `already-exact` | probe：`receiver` 8 行 + decorated 2 行 + utf16 1 行で上流と同じ `{pos:-1,end}`（REPORT §3） |
| snapshot の許可項目 | `snapshot_parsed_emit_metadata` remainder | `missing`（`comment_range` を拒否） | before：T1 5 件 `ParsedEmitMetadataNotPortable`（`records/before/`） |
| range の source identity | `CommentRange::source`（arena-local） | `missing` | packet に `SourceFileId` が無い；別 mount 順序で別 file を指し得る |
| restore の再 mount | `restore_parsed_emit_metadata` | `missing`（comment_range を再構成しない） | — |
| restore の fail-closed lookup | `mounted[&program]` index | `partial-or-stale`（panic 経路） | `.get()` + typed error に置換 |
| consumer（printer / declaration） | `comment_range_for_node`、`comment_range`、`preserve_js_doc` | `already-exact` | arena metadata を読む既存経路；restore 後の値をそのまま消費 |
| byte / UTF-16 | `SourceBytePosition` + `PositionIndex` | `already-exact` | packet identity が同一 `Arc<TextSnapshot>` を要求（`ParsedSourceIdentity::matches`）→ byte 位置は不変；utf16 対照で確認 |
| SourceFile root の寿命 | `execute.rs:1076-1081`（Bundle のみ snapshot） | `already-exact` | `lifetime/source-file-roots` exact ×2 |
| fresh getter / forced の寿命 | `emit_declaration_unit(..., parsed_emit_metadata=None)` | `already-exact`（既存 H2.7d 契約） | `bundle-declarations` 3 tests 再実行；同一 Program の 2 回 emit probe（REPORT §3） |
| synthetic node / original chain | `is_parsed_node` guard（`remember_node`） | `already-exact` | 既存 unit test |
| 他の未対応 field | remainder whitelist | `already-exact`（拒否維持） | unit test に synthetic comment / source-map range の拒否を追加 |
| known-native の T1 5 行 | `decorator-binding-known-native.json` | `obsolete` | 修復で exact → retire（§7） |

## 6. 設計決定

1. **packet の許可項目に `comment_range` を加える（capture / identity / restore / consumer を一組で）。** capture は `metadata.comment_range()` を `remember_comment_range` で `ParsedCommentRange { source: SourceFileId, range }` に写す。identity は range の source を node と独立に `validate_parsed_metadata_source` → `remember_source`（同じ snapshot / lease / node interval の照合）で登録する。restore は `mounted_source(range.source)` で新 `TransformSourceId` に写し `CommentRange::from_parts`（`metadata.rs`、`pub(crate)`）で再構成する。consumer は変更しない。
2. **range の source は node の source と同一と仮定しない。** 上流の range は plain object で source を持たず、印字中の `currentSourceFile` で解釈される。Rust の `CommentRange` は producer 時点の source を型で持つので、packet はそれをそのまま Program identity に落とす。同一 source が普通（T1 の全行）だが、別 source を指す range も同じ経路で再 mount される（unit test）。
3. **位置は byte のまま運ぶ。** packet identity が同一 text snapshot を要求するため、`SourceBytePosition` は declaration arena でもそのまま有効。UTF-16 へ変換して戻す必要は無い（上流の UTF-16 位置との対応は probe で確認）。
4. **端点の 4 状態を維持。** `EndOnly` だけを特別扱いしない：`Original` / `StartOnly` / `Synthesized` も同じ型で往復する（unit test）。
5. **fail-closed。** 未登録 source の lookup は `ParsedEmitMetadataSourceMismatch`、program 無し source（`add_source(_, None)`）の range は `MissingProgramSource`、既存 metadata との衝突は `ParsedEmitMetadataRestoreConflict`。restore は全 resolve 後に一括書込み。
6. **他 field の拒否を維持。** `remainder` whitelist に `comment_range` 以外を足さない（synthetic comments、source-map range 等の拒否を unit test で固定）。
7. **fixture / 入力 / 比較対象は不変。** 既存 767 件、`decorator-binding.json.zst`、既存 `bundle-maps.json` / `bundle-declarations.json` は変更しない。追加対照は新 ID（`bundle-metadata-t1/…`）と新 observer / fixture。

## 7. Implementation sequence（変更ファイル）

| 順 | file | 変更 | 完了確認 |
| --- | --- | --- | --- |
| 1 | `crates/emitter/src/metadata.rs` | `CommentRange::from_parts(source, range)`（`pub(crate) const fn`） | build |
| 2 | `crates/emitter/src/factory/parsed_metadata.rs` | `ParsedCommentRange`；`ParsedNodeMetadata.comment_range`；whitelist に `comment_range`；`remember_source` / `remember_comment_range`；restore の `mounted_source` fail-closed lookup と `comment_range` 再構成；unit tests 2 本追加 + 拒否 test 拡張 | `cargo test --manifest-path crates/emitter/Cargo.toml --lib parsed_metadata_` = 6 passed（既存 4 + 新規 2；`--lib parsed_` なら constants test を含む 7） |
| 3 | `scripts/generate-bundle-metadata-t1-inputs.mjs`（新規）→ `crates/compiler/tests/fixtures/bundle-metadata-t1-inputs.json` | 18 対照の入力（§8） | 生成 18 件 |
| 4 | `scripts/observe-bundle-metadata-t1.mjs`（新規）→ `crates/compiler/tests/fixtures/bundle-metadata-t1.json` | complete command ×2 + emitNode probe（`after` / `afterDeclarations` / after-emit / 2 回 emit） | `--write` 後 `--check` 一致（2 回目の採取） |
| 5 | `crates/compiler/tests/bundle_metadata_t1_contract.rs`（新規）、`fixtures/bundle-metadata-t1-known-native.json`、`fixtures/bundle-metadata-t1-known-packet.json` | 2 tests：complete command ×2（known-native 行は native projection を両 pass で assert）、packet ↔ probe 比較（known-packet 行は対称差を両 pass で assert、comment range 行は常に完全一致） | `cargo test --test bundle_metadata_t1_contract <2 exact names>` |
| 6 | `scripts/witness.py`、`.github/ci/test_replay.py` | `COMPILER_DIRECT["bundle-metadata-t1"]`（exact test 名 2、filtered 8）、内部 selector / dump 環境変数の除去；planner test の件数 18 と retire 後の known 4 | `--list` / `--all --dry-run`、planner 59 tests |
| 7 | `crates/compiler/tests/fixtures/decorator-binding-known-native.json` | T1 5 行を retire（他 4 行不変、整形不変） | `t1_witness` = `exact=5 known=0 failed=0 selected=5` |
| 8 | `docs/design/greenfield/slices/h2-8a-bundle-metadata-t1/{DESIGN,REPORT}.md`、`records/` | 本書と記録 | — |

禁止：`remainder.comment_range = None` のみの変更、`EmitMetadata` の無条件 clone、fixture 文字列 / case ID / NodeId 番号による分岐、既存期待値の書換え、手書き期待出力。

`crates/emitter/src/execute.rs`、`declarations/orchestration.rs`、printer、class-fields producer、既存 fixture / observer は変更しない。

## 8. Frozen witnesses

対象 5 件：依頼書 §3（入力 `decorator-binding-inputs.json`、期待値 `decorator-binding.json.zst` sha256 `be52ec87…`、不変）。

追加対照 18 件（`bundle-metadata-t1/<family>/<target>/<variant>`、outFile + declaration + declarationMap + sourceMap + CRLF、
T1 と同じ option 組。ただし `residual` 以外の bundle 行は **AMD**（`module: 2`）：System bundle は undecorated な top-level class を
`A = class A … };` に hoist し、その `};` 行の map が現状 tsc と異なる（R12 系、JS map のみ、packet と無関係）ため、
その 2 shape を `residual` 行として System のまま known-native に凍結し、他の family は同じ Bundle packet 寿命を持つ AMD で採る。
class は unexported（System の hoist と無関係に producer を観測する）：

| family | variant | 観測点 |
| --- | --- | --- |
| receiver（es2015、decorator 無し） | `static-get-second-file` / `static-get-first-file` / `static-get-both-files` | producer が decorator 経路に依存しないこと、source index 0 / 1 / 両方 |
| receiver | `static-set-second-file` | assignment producer（96808） |
| receiver | `static-compound-second-file` | clone 経路：parse node に range が残らない（negative） |
| receiver | `static-this-second-file` / `instance-this-second-file` | parsed ThisKeyword（static / instance private） |
| receiver | `nested-class-second-file` | 関数内 class の receiver |
| decorated（es2022 / esnext、set） | `static-get-second-file` | 第 1 原因（internal flag 32）と第 2 原因の同居、T1 と別 ID |
| mount-order（es2015、AMD outFile、Node10、`resolveJsonModule`） | `json-before-annotated` / `json-between-annotated` | JS arena と declaration arena で annotated source の index が異なる（JSON は declaration bundle から除外） |
| utf16（es2015） | `non-bmp-before-receiver` | receiver の前に非 BMP 文字とコメント：byte ≠ UTF-16 |
| lifetime（es2015） | `source-file-roots`（ES2015 module、outDir） / `no-declaration` / `emit-declaration-only` | packet を作らない 3 寿命 |
| residual（es2015、System） | `system-export-class-map` / `system-hoisted-class-map` | 本スライス外の JS map 残差（R12 系）を known-native として凍結；JS / d.ts / d.ts.map bytes は一致 |

各対照の上流期待値は observer が 2 回採取して一致を確認し、`--check` で再度全件採取して一致（REPORT §3）。Rust 固有の拒否対照は unit test（parsed_metadata.rs）に置き、互換件数に数えない。
本スライス外の差（residual 2 行の JS map、`bundle-metadata-t1-known-native.json`；packet の flag 差 3 行、`bundle-metadata-t1-known-packet.json`）は
owner 付きで凍結し、両 pass で厳密一致を要求、exact になれば retire を要求して fail する（known-native 規約）。互換件数に数えない。

## 9. Acceptance

- focused：`t1_witness`（依頼書 §5）= `exact=5 known=0 failed=0 selected=5`（retire 後）。
- 新 suite：`python3 scripts/witness.py bundle-metadata-t1 --all` = observer `--check` 一致 + 2 tests passed（complete command 15 exact + 3 known ×2、packet 12 exact + 3 known ×2；REPORT §3.1）。
- unit：`cargo test --manifest-path crates/emitter/Cargo.toml --lib parsed_metadata_` = 6 passed（既存 4 + 新規 2；`--lib parsed_` なら constants test を含む 7）。
- 隣接：`witness.py bundle-declarations --all`（3 tests + 1 filtered）、`witness.py bundle-program --all`（4 tests）。
- 完了条件：上の全てが green で、T1 5 件が complete-command 契約（JS / declaration / map の bytes・path・順序、診断、emit result、status、exit、callback metadata）で exact ×2。typed error の解消や JS 単独一致では完了としない。
- hosted / 全 767 件 / SUPER / acceptance は統合担当（未実行として REPORT に明記）。

## 10. Traceability

| 上流 owner / invariant | Rust | focused test | evidence |
| --- | --- | --- | --- |
| `setCommentRange(receiver, moveRangePos(receiver,-1))` が parse node に残る | `set_private_receiver_comment_range` → packet `ParsedCommentRange` | `bundle_metadata_t1_parsed_packet_matches_typescript_after_javascript_probe` | `bundle-metadata-t1.json` probes |
| Bundle root は dispose されない | `execute.rs` snapshot → `orchestration.rs` restore | `t1_witness`、`bundle_metadata_t1_controls_match_complete_typescript_observations` | REPORT §2 |
| range は現在 source の text で解釈 | `CommentRange::source` を Program identity で再 mount | `parsed_metadata_comment_ranges_survive_reordered_mounts_with_their_own_source_identity` | unit log |
| 失敗時に部分書込み無し | `restored` 一括 extend、`mounted_source` | `parsed_metadata_comment_range_sources_are_validated_and_restore_stays_atomic` | unit log |
| 他 field は拒否 | remainder whitelist | `parsed_metadata_rejects_unimplemented_fields_and_synthetic_type_references` | unit log |
| SourceFile / fresh / forced 寿命 | 既存 H2.7d | `bundle-declarations` / `bundle-program`、`lifetime/*` 対照 | REPORT §2 |

資源：全コマンド `CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15`。単一 writer（本 worktree）。

## 11. Unresolved items

なし（`unresolved = 0`、`undispositioned = 0`）。記録のみの観測：同一 Program で通常 emit を 2 回行うと上流は両方の write tuple が一致する（全 16 対照、probe `second_ordinary_emit.identical = true`）——Rust の「fresh getter / forced は空から始める」契約はこの範囲で出力同値。API 経由の同一 Program 再 emit は H2.9 / API1 の owner。

## 12. Readiness summary

authority hashes：`_tsc.js` `1c59e77a…`、`typescript.js` `56917765…`、上流 span 26（records/upstream-spans.tsv）。reachable upstream rows 21（§3、関数 26）。local-gap rows 12（§5：missing 3、partial-or-stale 1、already-exact 7、obsolete 1）。Rust-map rows 7（§4）。witness rows 5 + 18（§8）。architecture concerns 3（§2：modified-requalify 1、premise-unchanged 2）。lifecycle transitions 1（`E-METADATA-BASE` bundle packet：`active-qualified` → `modified-requalify`、最終 ref での再 qualify は統合担当）。undispositioned 0、unresolved 0。確認コマンド：§9。
