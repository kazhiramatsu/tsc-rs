# A40-PRINT-FAILURE：printer の失敗順序と継続状態（隔離候補・設計と記録）

作成日：2026-09-15。親：H2.8a / A6-40、依頼 C03（[handoff](../h2-8a-printer-failure-claude-handoff.md)、
[共通手順](../claude-high-difficulty-handoffs.md)）。状態：**隔離候補の提出**。本番統合・admission は統合担当（A-INT3）。

## 1. 開始点

| 項目 | 値 |
| --- | --- |
| worktree / branch | `../tsc-rs-printer-failure` / `draft/h2-8a-printer-failure` |
| 開始 `HEAD`（= `origin/main`） | `f9ef828a56c9f9305947a6e6f3c1ae39072b3110`（PR #525 merge。SUPER `f64443300`・CI 改修 `bb2d51c89` を祖先に含む） |
| 開始時 `git status --short` | 空（新規 worktree） |
| toolchain | rustc 1.93.0 (254b59607 2026-01-19)、cargo 1.93.0、node v25.2.1 |
| vendor | `_tsc.js` `1c59e77a…eddd3e3`、`typescript.js` `569177652…2bb12e39`（共通手順の値と一致） |
| 旧 v18 patch の再適用 | なし。本書の候補は main の現行 printer/writer/UTF-16 API に対する新規差分 |

環境：`taskpolicy -b nice -n 15`、`CARGO_BUILD_JOBS=2`、`--offline`、一度に一つの重い実行。

## 2. 対象と非対象

対象：既存 printer（`crates/emitter/src/printer.rs`、`printer/bundle.rs`）の
public entry（`Printer::print` の StandaloneNode / SourceFile / Bundle）で、`Transformer` hook
（substitute / before / after）が失敗したときの「書き出し済み prefix・callback 順序・保持される状態・
次の print への影響」。固定 TypeScript 6.0.3 の `createPrinter(options, handlers)` +
`printNode / printFile / printBundle` を期待値の唯一の出典とする。

非対象（本書 §7 に境界として記録）：公開 custom transform API 全体、全 sink の atomic write 改修、
新 printer の再実装、`TextWriter` 全 write の `Result` 化、上流の内部 API（`writeNode/writeFile/writeBundle` に
呼出側 writer/generator を渡す形）に対する Rust 側の新設。

## 3. Source 入口と失敗し得る callee

固定 `_tsc.js` の範囲と本体 SHA-256（先頭 16 桁。全桁は
`records/upstream-spans.json`）。「失敗」= 例外がそこから、またはそこで呼ぶ handler/writer/generator から伝播し得るか。

| Source | span | sha256 | 失敗し得るか | Rust の対応 |
| --- | --- | --- | --- | --- |
| `createPrinter` 状態 | 116912-121378（既存 pin） | b227b66a… | — | `Printer` struct |
| `printNode` / `printFile` / `printBundle` / `printList` | 116985-117005 / 117019-117027 / 117010-117018 / 117006-117009 | 624ff5c9… / 17ef8275… / a4a09f11… / 494c03cc… | 伝播のみ（`beginPrint` は投げない） | `print(StandaloneNode{MultiLine})` / `print(SourceFile)`（Canonical） / `print(Bundle)` / **なし**（`NodeList` は typed Unsupported） |
| `writeNode` / `writeList` / `writeBundle` / `writeFile2` | 117028-117038 / 117039-117057 / 117058-117071 / 117072-117081 | f6450be8… / 87715851… / bfb6e699… / c4d8a035… | 伝播。**正常終了時のみ `reset()` と `writer = previousWriter`** | 呼出側 writer を受ける入口は Rust に無い（§7） |
| `beginPrint` / `endPrint` | 117082-117084 / 117085-117089 | 8d487851… / b2a3b084… | 投げない。`endPrint` は正常終了時のみ到達 | 候補 `begin_print` / `end_print` |
| `print` / `setSourceFile` / `setWriter` / `reset` | 117090-117100 / 117101-117108 / 117109-117116 / 117117-117141 | b6934e15… / 01175a1e… / 3e58d0fb… / fd783bac… | 投げない。`reset` が触らない状態は §5 | `start_print` / `finish_print` |
| `pipelineEmit` / `getPipelinePhase` | 117173-117178 / 117185-117215 | 6a43b2cf… / 0e29bfcc… | `substituteNode` handler が投げる | `emit_node_with_hint_and_source_comments` の `substitute_node` |
| `pipelineEmitWithNotification` / `pipelineEmitWithHint` / `pipelineEmitWithSubstitution` | 117219-117222 / 117223-117235 / 117712-117718 | 3e319f7f… / 78e89abc… / 143322c3… | `onEmitNode` の前半・後半、`onBefore/AfterEmitNode` が投げる。**`finally` 無し** | `before_emit_node` / `after_emit_node` |
| `pipelineEmitWithComments` / `emitCommentsBeforeNode` / `emitCommentsAfterNode` | 120978-120986 / 120987-120994 / 120995-121006 | 263af529… / dc59a090… / f0baac32… | comment writer 経由で投げ得る。`commentsDisabled` の set/reset に `finally` 無し | `EmitContext`（不変・threaded）と候補 `comments_disabled_after_failure` |
| `emitLeadingCommentsOfNode` / `emitTrailingCommentsOfNode` | 121007-121032 / 121033-121046 | ce6bf342… / e5c99d84… | 同上。container 3 値の set/restore に `finally` 無し | `CommentEmissionScope` claim + 候補 `root_comment_scope` |
| `forEachLeadingCommentToEmit` / `forEachTrailingCommentToEmit` | 121219-121233 / 121234-121238 | 2e1fb613… / bd6612ac… | 投げない（guard） | `parent_comment_container_owned_prefix_for_owner` / `retains_end` |
| `emitBodyWithDetachedComments` | 121075-121104 | b07b0634… | 伝播。`commentsDisabled` の一時 set に `finally` 無し | node 単位の `NO_NESTED_COMMENTS` 反転で包含 |
| `emitNodeList` / `emitNodeListItems` | 120029-120067 / 120068-120155 | 9286227d… / ebeb65a7… | `onBefore/AfterEmitNodeArray`、item pipeline、comment 出力から伝播。`nextListElementPos = child.pos` は item の前に代入 | `emit_formatted_node_list` / `record_list_element_position`（A6-40 既存） |
| `writeTokenNode` / `emitTokenWithComment` | 120213-120221 / 118731-118764 | 04ee5d98… / d7df39bb… | `onBefore/AfterEmitToken`、writer | token hook は Rust に無し（§7） |
| `emitSourceFile` / `emitSourceFileWorker` / `emitPrologueDirectives` / `emitShebangIfNeeded` | 119710-119719 / 119753-119769 / 119789-119811 / 119823-119838 | cea241c6… / 8dfb3b4d… / e59768a4… / 0447f0d6… | 伝播。`pushNameGenerationScope` の pop に `finally` 無し | `write_transformed_source_file`（候補で順序と改行位置を修正） |
| `pushNameGenerationScope` / `popNameGenerationScope` / `generateName` / `makeTempVariableName` / `makeUniqueName` / `makeName` | 120480-120492 / 120493-120502 / 120624-120632 / 120703-120740 / 120741-120779 / 120943-120977 | 75e640ef… / a2f730a9… / 5418a174… / 9b0f57d6… / 24cb46aa… / e8a91f80… | 投げない。**`reset()` が消すのは正常終了時のみ** | `finalize_generated_binding_names_for_print`（transformation 所有・eager。§7 の境界） |
| `pipelineEmitWithSourceMaps` / `emitSourceMapsBeforeNode` / `emitSourceMapsAfterNode` / `emitPos` / `emitSourcePos` / `emitTokenWithSourceMap` / `setSourceMapSource` | 121277-121282 / 121283-121293 / 121294-121303 / 121307-121321 / 121322-121332 / 121333-121351 / 121352-121370 | 0c57cdce… / ac346b41… / 6ca767d4… / 0f8f04ad… / c3f07966… / 1f4c5a04… / 9d5ddceb… | generator の `addMapping/addSource` が投げる。`sourceMapsDisabled` の一時 set に `finally` 無し（次の `setWriter` で再計算）。`mostRecentlyAddedSourceMapSource` は `reset()` 対象外 | `SourceMapRecording`（print 所有・不可謬）。§7 |
| `createTextWriter` | 16365-16461 | 468df403… | writer は投げないが、呼出側 writer は任意 | `TextWriter`（不可謬） |
| `emitHelpers` | 117719-117755 | 27b8a8ef… | `bundledHelpers` は `reset()` 対象外 | bundle print 内の `emitted_helpers`（print 単位） |

## 4. 観測

### 4.1 observer（`scripts/observe-printer-failures.mjs`）

- 入力は固定 `vendor/typescript-6.0.3/lib/typescript.js`（`createPrinter` の public handlers：
  `isEmitNotificationEnabled`、`substituteNode`、`onEmitNode`）。**adapter は `try/finally` を追加しない**：
  `onEmitNode` の後半（`after`）は `emit(hint, node)` が返った場合にのみ実行・記録する。
- 失敗点は `phase / kind / occurrence`（op 内で phase×kind ごとに 1 始まりで数えた handler 呼出回数）で指定する。
  実行順が変わっても同じ指定で同じ失敗点を選べる。
- 1 case = `sources` + `tracked` kinds + `ops`。各 op は `fresh` printer か共有 printer で
  `printNode / printFile / printBundle` を 1 回実行し、`status`（`returned` の text・UTF-8 base64・bytes・
  `end_utf16`、または `threw` の message）を保存する。全 hook 呼出は `{op, phase, hint, kind, pos, end}` で保存する。
- 全 case を 2 回観測して deepEqual を確認してから固定する（`repetitions: 2`）。
  `--write` は新規作成のみ（`wx`）、`--check` は byte 一致を検証する。
- 出力：`crates/emitter/tests/fixtures/printer-failure-hooks.json`（25 case、Rust replay 対象）と
  `crates/emitter/tests/fixtures/printer-failure-probes.json`（14 case、上流のみの probe）。

### 4.2 hook fixture の 25 row（ID `printer-failure/<entry>/<phase>/<site>/<sequence>`）

| entry | rows | 軸 |
| --- | --- | --- |
| printNode | `before/identifier-2/recover-same`（CRLF/LF）、`…/recover-other-statement`、`…/recover-other-source-same-positions`、`before/statement-1/recover-same`、`substitute/identifier-2/recover-same`、`after/identifier-1/recover-same`、`after/statement-1/recover-same`、`before/nested-comments/identifier-2/recover-same`、`before/remove-comments/identifier-2/recover-same`、`substitution-y/after/identifier-2/recover-same`、`substitution-y/success/recover-same`、`before/non-bmp/identifier-2/recover-same`、`before/block-indent/identifier-2/recover-other-block-{crlf,lf}`、`before/list-item-3/cursor-array`、`unique-name/after/statement-1/recover-new-unique`、`unique-name/success/twice` | hook 3 phase × 初回/後続、代替 node、NoComments/NoNestedComments/removeComments、LF/CRLF、非 BMP、indent、list 前半/後半 + A6-40 cursor、generated names、成功→成功 / 成功→失敗→成功 / 失敗→同一 node / 失敗→別 statement / 失敗→別 source / 失敗→失敗後 2 回目 |
| printFile | `before/statement-2/recover-same`、`before/identifier-2/recover-same`、`before/source-file-1/prologue/recover-same`、`after/source-file-1/recover-same`、`before/statement-2/recover-other-file` | SourceFile hook、statement 途中、shebang/prologue と通知順、別 file |
| printBundle | `before/statement-2/recover-same`、`success/twice` | 二つ目の source、再利用 |

`text` 軸の「孤立 surrogate の direct writer」は本 slice では追加していない（UTF-16 writer の既存 contract
`utf16_writer_contract` が writer 単体を覆う。失敗経路との合成は §10）。

### 4.3 probe fixture の 14 row（`rust_counterpart: "absent"`）

writer fault（`writeNode` に呼出側 writer：`writeComment/writeKeyword/writeStringLiteral/writeLine` の 1 回目で投げる + 無 fault 対照）、
map fault（`writeFile` に呼出側 generator：`addMapping` 1 回目 / 3 回目 + 無 fault 再利用）、
`onBefore/AfterEmitNodeArray`・`onBeforeEmitToken`、`printList`、compiler command
（custom transformer の `onEmitNode` が 2 file 目で投げる partial write、`noEmitOnError`）。
上流の observable の記録であり、Rust の到達前提が無いことを row ごとに `reason` に明記する。

## 5. 上流の失敗時状態の機構（測定で確認した事実）

`writeNode/writeFile2/writeBundle` は正常終了時にだけ `reset()` を呼び、`printNode/printFile/printBundle` は
正常終了時にだけ `endPrint()`（`ownWriter.getText()` + `clear()`）を呼ぶ。例外時はどちらも走らない。
`reset()` が消すのは生成名の表・temp flag stack・reserved names・`currentSourceFile`・`currentLineMap`・
`detachedCommentsInfo`・writer/generator 参照だけである。したがって失敗後に残るもの：

| 状態 | 失敗後 | 次の print への観測（fixture の row） |
| --- | --- | --- |
| `ownWriter` の text・indent・lineStart・行数 | 残る（`endPrint` 未到達） | 次の `print*` の戻り値が **partial prefix + 継続した indent/行** になる（例 `"/*a*/ f(f(x); //t\r\n"`、block 内失敗後は `"{\r\n    f({\r\n        g(y);\r\n    }"`）。成功で消える |
| `containerPos/End/declarationListContainerEnd` | 例外時点の値（最内の comments-before が claim した node）のまま | 次の print で `pos === containerPos` の leading comment、`end === containerEnd` の trailing comment が **以後ずっと**抑止される（`recover-same` の op3 `"f(x); //t\r\n"`）。位置は素の整数なので **別 source でも一致すれば抑止**（`recover-other-source-same-positions`） |
| `commentsDisabled` | `NoNestedComments` subtree 内で失敗すると true のまま | 以後の全 print が comment 無し（`nested-comments` row の op2-op4） |
| 生成名の表・`tempFlags`・`generatedNames` | 残る | 次の unique name が `x_2`（`unique-name/after…` op2）。成功なら `x_1` に戻る |
| `nextListElementPos` | 残る（成功でも残る） | A6-40 lifecycle と同じ（`cursor-array` op2 の `[/*c1*/ b, c]`） |
| `lastSubstitution` / `currentParenthesizerRule` | 残る | 次の `pipelineEmit` で上書きされ観測不能 |
| `currentSourceFile` / `detachedCommentsInfo` | 残る | 次の print が sourceFile を渡せば `setSourceFile` で置換。渡さない呼出（型上は必須）だけが観測し得る。未測定 |
| `sourceMapsDisabled` | true のまま | 次の `setWriter` が再計算。観測不能 |
| `mostRecentlyAddedSourceMapSource(Index)` / `bundledHelpers` | 残る（成功でも残る） | 内部 API `writeFile` を別 generator で再利用すると **`addSource` が呼ばれず `sources: []`**（probe `map-fault/none-reuse-new-generator`、失敗の有無に依らない） |
| transformer 側 `onEmitNode` wrapper の閉包状態 | 残る（after 半が走らない） | callback 順序：失敗 node と祖先の `after` は一切発火しない（全 row の `events`） |

hook が投げる位置による差：`before(N)`/`substitute(N)` は N の comments 相の前なので container は親の値、
`after(N)` は N の trailing の後なので restore 済みの親の値、N の worker 内（子の hook）は N の claim 値が残る。
`printNode` の root の comment 相は notification の内側で走る（`before/statement-1` の partial は `""`、
`after/statement-1` の partial は trailing comment 込み）。`printFile` は shebang → 各 prologue（各自の
pipeline と hook）→ `SourceFile` の hook → `emitSourceFile`（先頭で `writeLine()`）の順である
（`source-file-1/prologue` row の events と partial `"#!…\r\n\"use strict\";"`）。

## 6. state lifetime 表（Rust 候補での扱い）

| 状態 | 上流の寿命 | Rust 候補 | 失敗時 |
| --- | --- | --- | --- |
| own writer（text/indent/line） | printer 持続、成功で clear | `Printer.own_writer: Option<TextWriter>`：`begin_print` で取り出し、`end_print` で成功時のみ clear して戻す | **保持**（partial prefix を次の print が継続） |
| comment container 3 値 | printer 持続（`reset` 対象外）、node ごとに save/restore | `Printer.root_comment_scope`：各 entry の root `EmitContext` に seed。失敗した最内 frame の `expression_context.comments()` を `failure_comment_scope` に記録し、entry 終了時に昇格 | **失敗時点の値を保持** |
| `commentsDisabled` | printer 持続 + 動的 extent | `Printer.comments_disabled_after_failure`；`comments_disabled()` = option ∨ sticky。`NO_NESTED_COMMENTS` extent に入った frame の worker が失敗したとき set | **保持** |
| list cursor | printer 持続、成功でも残る | 既存 `next_list_element`（変更なし） | 保持（A6-40 と一致を確認） |
| 生成名 | printer 持続、成功で reset | transformation 所有・print ごとに eager 再確定（変更なし） | **境界**（§7） |
| transformer 通知状態 | transformer 閉包、after 半で復元 | `after_emit_node` は成功時のみ呼ぶ（候補 P1） | 上流と同じく **未復元** |
| `EmissionPlan` | （Rust 固有）print 単位 | 変更なし | 破棄 |
| source-map recording | print 単位（Rust）/ 呼出側 generator（上流） | 変更なし | 破棄（§7） |

## 7. 設計判断・境界

1. **P1 失敗時に after hook を呼ばない**（`emit_node_with_hint_and_source_comments`）。main は失敗時にも
   `after_emit_node` を「復元 guard」として呼んでいた（`printer.rs` 旧 13800 行付近）。上流には `finally` が無く、
   全 row の `events` が一致しなかった。production transformer（es2015 / class_fields）の before/after 対は
   上流の `onEmitNode` wrapper と同じく失敗後は未復元になる。compiler 経路では print 失敗が emit 全体の
   `Err` になり transformation は破棄されるので影響しない。Rust 固有 error（identity arm の
   `TransformedNodeWorkerUnavailable`）の after 呼出しは Rust-only の復元 guard としてそのまま残し、
   別 battery（`rust_only_typed_errors_keep_the_printer_usable`）に pin した。
2. **P2 own writer の保持**（`begin_print`/`end_print`）。recording 付き（compiler の writeFile 相当）は呼出側
   writer の意味なので従来どおり呼出ごとに新規。`StandaloneWriter::SingleLine` は checker の
   `usingSingleLineStringWriter`（`finally` で clear）に対応するため呼出ごと。
3. **P3 comment container の carry**。`CommentEmissionScope` は source 識別付き `CommentCursor` を持つため、
   上流の「素の整数一致」による **別 source の偶然一致は再現しない**（`recover-other-source-same-positions`
   の op2/op3 を既知差分として登録）。素の整数比較に落とすのは typed cursor の設計（A6-40 comment cursor
   の contract）を崩すので採らない。統合担当が上流互換を優先する場合は `parent_comment_container_owned_prefix_for_owner`
   の比較を position のみに緩める 1 箇所で切替可能。
4. **P4 `commentsDisabled` の sticky 化**：`self.options.remove_comments` の読み出し 33 箇所を
   `self.comments_disabled()` に機械置換。option の値自体は変更しない。
5. **P5 standalone root の comment 相を notification の内側へ**：`print_standalone_node` は
   `emit_leading_comments_for_node` → pipeline → `emit_trailing_comments_for_node` の順で、root の leading が
   `before` hook より先に書かれていた（main の既存差）。`DeferredExpressionSourceComments::nested(carried_scope, LeadingAndTrailing)`
   を渡して既存 pipeline の phase で処理する。
6. **P6 改行の書き出し位置**：上流は各 item の**前**に `writeLine()`（prologue、`emitSourceFile` 先頭、list の leading/separating）を
   書き、Rust は各 statement の**後**に書いていた。通常は同じ bytes だが、partial prefix の後では
   上流だけが改行を書く（`printFile/before/identifier-2` の `"/*a*/ f(\r\nf(x)…"`）。上流の位置に合わせた
   （行頭では no-op）。
7. **P7 `SourceFile` hook の発火位置**：上流は shebang と prologue（各自の hook 込み）の後に `print(SourceFile)` の
   hook を出す。Rust は先頭で出していた（`prologue` row の events と partial）。`notify_source_file_root` を
   最初の body statement／body 無しの各点で 1 回呼ぶ。
8. **境界（再現しない・できない）**
   - 生成名：上流は print 中に lazily 生成し失敗後も `generatedNames` を保つ（`x_2`）。Rust は transformation 所有の
     表を print ごとに eager 確定する（`x_1`）。printer 所有・lazy 化は "新 printer の再実装" に近く本 slice の範囲外。
   - `substitute_node` で新規 node を作れない：`TransformationContext` は完了後の factory 使用を
     `InvalidLifecycle` で拒否する（上流の custom transformer は print 中に `factory.createIdentifier` を呼ぶ）。
     replay は代替 node を事前生成する。API1.1a の項目。
   - 呼出側 writer / generator の fault（probe 5+3 row）：Rust の `TextWriter`・`SourceMapRecording` は不可謬で
     print 所有。全 write の `Result` 化は開始しない（handoff の指示）。上流の `mostRecentlyAddedSourceMapSource`
     fast path による `sources: []` は内部 API 再利用時の上流の癖であり、Rust の recording は print 単位なので発生しない。
   - node-array / token hook、`printList`：Rust の `Transformer` に対応 hook が無く、`NodeList` は typed Unsupported。
   - compiler command：custom transformer が無いため hook fault が到達しない。上流は 1 file 目を書いてから投げる
     （probe）。Rust は全 artifact 構築後に sink へ書く（E-OUTPUT-SCRIPT）。`noEmitOnError` は上流 `emitSkipped: true`
     で write 無し（probe）。Rust の diagnostic gate の等価性は本 slice では再検証していない。
   - `currentSourceFile` の stale 参照（sourceFile を渡さない `printNode`）：未測定。

## 8. 候補 patch

`records/h2-8a-printer-failure.candidate.patch`（`printer.rs` + `printer/bundle.rs` のみ、base `f9ef828a5`）。
閉包でくくった entry 本体の再インデントを含むため行数は大きいが、`git diff -w`
（`records/h2-8a-printer-failure.candidate.ignore-whitespace.patch`）では `printer.rs` 415 行・`bundle.rs` 36 行
（363 追加 / 88 削除）。変更点は §7 P1〜P7 と `Printer` の 4 field。新規 test/fixture/script は tree 上のファイルとして
提出する（§10）。patch の SHA-256 は §10 の表に記す。

## 9. 検証記録

### 9.1 before（無変更 main `f9ef828a5`）

- 既存 focused 5 suite（handoff 指定）：`comma_list_printer_contract` 1/1、`emit_pipeline_phases_contract` 1/1、
  `list_format_flags_contract` 1/1、`source_comment_topology_contract` 18/18 pass、
  **`list_comment_flags_contract` 0/1 FAIL（`call-inline/ParentNoNestedComments/retained`、
  `call-newline/ParentNoNestedComments/retained`）**。開始 SHA で再現する**継承失敗**（OPS-DEBT）。
  記録：`records/before-focused-baseline-nofailfast.log`。
- 新規 `printer_failure_contract`（無変更 main 上、25 row の初回は 23 row 版）：**2/23 exact、38 mismatch key**
  （`records/before-printer-failure-contract.log`）。差の分類：全 failure row で祖先の `after` が発火（P1）、
  partial prefix 無し（P2）、container carry 無し（P3）、`commentsDisabled` carry 無し（P4）、
  root comment 順序（P5、後続 row で顕在化）、改行位置（P6）、`SourceFile` hook 順序（P7）、生成名（境界）。

### 9.2 after（候補）

- `printer_failure_contract`：**23/25 exact、mismatch key 3 = 既知差分 3、exit 0**
  （`records/after4-printer-failure-contract.log`）。既知差分：`recover-other-source-same-positions#op2/#op3`
  （typed cursor）、`unique-name/after/statement-1/recover-new-unique#op2`（eager 生成名）。
  中間 run：after1 17/23（P1-P4 後）、after2 22/25（P5-P7 後）、after3 22/25（改行位置の順序修正前）。
- 各 case は 1 run 内で 2 回 replay して同一（`repetitions` 検証）。
- Rust-only battery：`rust_only_typed_errors_keep_the_printer_usable`（NodeList Unsupported → 次の print は fresh、
  identity arm の代替 statement → `TransformedNodeWorkerUnavailable` と after 2 件、次の print は完全）。
- probe fixture の構造 pin：`printer_failure_probes_are_recorded_upstream_only_evidence`。
- 隣接 suite（最終 bytes、`records/after-final-emitter-suites.log`・`records/after-final-clippy.log`）：**§9.3 に結果**。

### 9.3 隣接 regression（最終 bytes）

最終 bytes（候補 patch SHA-256 `65a315d9…`）で一度にまとめて実行（`records/final-emitter-suites.log`、`records/final-clippy.log`）：

| 集合 | 結果 |
| --- | --- |
| `printer_failure_contract`（3 test） | 3/3 pass（hooks 23/25 exact + 既知差分 3 key、probes 構造 pin、Rust-only battery） |
| handoff 指定の 5 suite | `comma_list_printer` 1/1、`emit_pipeline_phases` 1/1、`list_format_flags` 1/1、`source_comment_topology` 18/18 pass。`list_comment_flags` 0/1 FAIL = **開始 SHA と同じ 2 row の継承失敗**（before と同一） |
| 追加の隣接 target | `utf16_writer_contract` 2/2、`comma_argument_factory_contract` 2/2、`decorator_super_direct_contract` 1/1（direct 28 exact / 4 known は変化なし） |
| `contracts`（printer_foundation / comment_scope_witness / writer_position / token_cursor / printer_oracle / artifact_sink / active_transform に filter） | 401 pass / 0 fail（51 filtered out） |
| emitter lib unit | 505 pass / 0 fail |
| `cargo clippy -p tsc-rs-emitter --all-targets` | exit 0。警告 165 は全て候補 hunk 外・新規ファイル外（`records/classify-clippy.py` で分類。emitter 内 20 + program crate 145）。候補由来 0 |

未実行：全 530 / 672 complete command replay、全 oracle chain、`cargo xtask acceptance`（hosted で実施）。
新規 target の hosted job は未登録（§10）。

## 10. 提出物・再現手順・hosted 入口

| 種別 | パス |
| --- | --- |
| DESIGN | 本書 |
| observer | `scripts/observe-printer-failures.mjs`（`node scripts/observe-printer-failures.mjs --check`） |
| 入力/期待値 | `crates/emitter/tests/fixtures/printer-failure-hooks.json`（25）、`…/printer-failure-probes.json`（14） |
| Rust contract | `crates/emitter/tests/printer_failure_contract.rs`（3 test） |
| 候補 patch | `docs/design/greenfield/slices/h2-8a-printer-failure/records/h2-8a-printer-failure.candidate.patch` |
| 記録 | `docs/design/greenfield/slices/h2-8a-printer-failure/records/*.log`、`upstream-spans.json`、`printer-failure-hooks.rust-actual.json`（Rust 側の全 op capture。`TSC_RS_PRINTER_FAILURE_ACTUAL=<path>` を付けて contract を実行すると再生成） |

再現：

```sh
git worktree add -b draft/h2-8a-printer-failure ../tsc-rs-printer-failure f9ef828a56c9f9305947a6e6f3c1ae39072b3110
# 新規ファイルを配置し、候補 patch を適用
git apply docs/design/greenfield/slices/h2-8a-printer-failure/records/h2-8a-printer-failure.candidate.patch
node scripts/observe-printer-failures.mjs --check
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test --offline --manifest-path crates/emitter/Cargo.toml \
  --test printer_failure_contract -- --test-threads=1 --nocapture
```

hosted 入口：**未登録**。`.github/ci/replay.py` / `scripts/witness.py` は decorator witness と acceptance 31 slice
だけを持ち、新規 target `printer_failure_contract` はどの job にも含まれない。追加案：emitter の standalone test
target を選ぶ小さな job（対象 3 test、build 込み想定 10 分未満、選択規則は `crates/emitter/src/printer.rs`・
`printer/bundle.rs`・`comment_cursor.rs`・`writer.rs`・fixture・observer の変更）。統合担当が OPS-COVER で登録するまで、
本 target は hosted で実行済みとは扱わない。

## 11. 未完了事項

1. 既知差分 2 case（typed cross-source cursor、eager 生成名）の disposition（維持か上流互換化か）。
2. 継承失敗 `list_comment_flags_contract` 2 row の原因確認（`ParentNoNestedComments`、本候補の P4 と隣接）。
3. `print_json_source_file` と identity arm の Rust-only 経路は own writer だけ適用し、hook 順序は未変更。
4. `currentSourceFile` stale 参照、孤立 surrogate × 失敗経路、`printFile` 途中失敗後の `printNode`
   （entry 跨ぎの carry）、bundle helpers（`bundledHelpers`）の再利用 row は未追加。
5. compiler 経路：Rust の `noEmitOnError`/sink fault の既存 contract と probe の突合は未実施。
6. hosted job 登録（OPS-COVER）。
7. `Printer` に field が増えたため `Clone/Eq` の意味が変わる（own writer を含む）。executor は print 失敗で
   `Err` を返し printer を捨てるので影響しないが、複数 unit を同一 printer で刷る `print_script_units_with_recording_for_harness`
   は recording 付き（呼出ごと新規 writer）であることを統合時に再確認する。
