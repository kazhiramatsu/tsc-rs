# UTF-16 adjacent repairs: 実装レビュー回答

回答日: 2026-09-14。レビュー依頼: [h2-8a-utf16-adjacent-implementation-review.md](h2-8a-utf16-adjacent-implementation-review.md)。
設計合意: [h2-8a-utf16-adjacent-repair.md](h2-8a-utf16-adjacent-repair.md) §10.1–§10.3、経過 §11–§32。

## 0. 対象と方法

| 項目 | 値 |
| --- | --- |
| 対象 head | `b652451f0ec4e6aba47aba3f4fd345c1b9168cdc` + 未コミット差分（tracked 369 files, +26,278 / −17,280、untracked 100） |
| 対象の固定 | `target/declaration-comment-ranges-runs/utf16-closing-source-freeze-20260914-034941/`（code manifest は `utf16-final-source-freeze-20260914-033140` と同一、tracked patch SHA-256 は各 receipt 参照） |
| production 修復前基準 | `ed6d8073a`（`git diff ed6d8073a` で確認） |
| `origin/main` merge-base | `f406f1200`（対照用 pristine worktree `~/dev/tsc-rs-main-baseline-f406f120`、target dir `target/baseline`） |
| レビュー中の変更 | production / fixture / ratchet / 既存 receipt は無変更。新規に作ったのは `target/declaration-comment-ranges-runs/review-probes-20260914/`（probe の入力と出力）だけ |
| 実行した検証 | 下記 0.1 |

### 0.1. レビュー中に実行したコマンド

読み取りと probe のみ。cargo は実行していません（最終 receipt は継続作業 §32.12 のものを対象 tree で参照）。

1. 全 observer fixture の再照合: `node scripts/observe-utf16-*.mjs --check`（`observe-utf16-literal-witnesses.mjs` は `<group> --check` で adjacent-probes / bundle-prologues / string-literals / template-literals の 4 group、`observe-utf16-literal-recovery-corpus.mjs` は `--census` 付き）と `node scripts/observe-prologue-only-detached-comments.mjs --check`。すべて一致。
2. CLI probe（`target/declaration-comment-ranges/debug/tsc-rs` と `node vendor/typescript-6.0.3/lib/_tsc.js`、いずれも `mktemp -d` 下の同一 tree、`-p tsconfig.json --pretty false|true`）:
   - A の表示境界: `const o = {"\uD800": 1, "\uD801": 2}; export const x = o["\uD802"]; export const y: "\uD800" = "\uDC00";`（strict, declaration, es2015/esnext）。`--pretty false` の stdout バイト列と `a.d.ts` が tsc と同一（診断文中の孤立 surrogate は双方 U+FFFD、型表示は `"\uD800"` 綴り、`.d.ts` は `y: "\uD800"`）。merge-base binary は同じ入力で TS1117（同名 property）を出す＝修復前の症状。
   - B の境界: JSDoc 診断（`.js` + checkJs、`/** @param {string x */`）は tsc/tsc-rs とも emit + TS1005 + exit 2 で同一。未終端 regex（TS1161）は tsc が emit、tsc-rs は合意どおり typed refusal（`emit recovery for 1 parse diagnostics is deferred to H2.9`）。
   - C の hoisted temp witness（ES2017, commonjs）: valid `ok\`x\`` / invalid `ok\`\unicode\`` / untagged で emitted JS が tsc と byte 同一（`var _a;` と `templateObject_1` tail）。
3. `--pretty true` の stdout は tsc と 1 改行の位置だけ異なる（最後の code frame 直後の空行が `Found ...` 行の前へ移動）。merge-base binary も同じ出力であり、本差分の前から存在する CLI pretty reporter の差。UTF-16 とは無関係。

### 0.2. 追加対照とレビュー回答 §5 の対応表（依頼書 §6 の「対応表」）

提出物には明示の対応表が無かったため、reviewer 側で fixture の case id から復元した。実装側は
この表を依頼書または設計書に取り込むこと（follow-up、文書のみ）。

| 要求（設計レビュー回答 §5） | 対照 fixture / case id | 最終結果 |
| --- | --- | --- |
| A-1 区別（D800/D801/DC00/FFFD/`\\uD800`/pair/`\u{1F600}`） | `utf16-identity-recovery-controls.json` `a1-distinct-and-pair/target-{1,2}` | exact ×2 |
| A-2 綴り同値（object 1117 / class 2300,2393 / interface 2300,2717） | `a2-object-equivalent`, `a2-class-equivalent`, `a2-interface-equivalent` ×2 targets | exact ×2 |
| A-3 lookup 両端・keyof / Pick / Omit / Record / mapped / narrowing / contextual / reverse-mapped | `a3-lookup-mapped`, `a3-narrowing-context`, `a3-reverse-mapped` ×2 targets | exact ×2 |
| A-4 late-bound | `a4-late-bound` ×2 | exact ×2 |
| A-5 enum 逆写像・const enum・pair 結合 | `a5-enum-pair` ×2 | exact ×2 |
| A-6 template literal type / Uppercase / `a${string}` | `a6-template-types` ×2 | exact ×2 |
| A-7 cross-file / string export-import names / ambient module `"\uD800"` | `a7-cross-file/module-{1,99}`, `a7-ambient-module/module-{1,99}` | exact ×2 |
| A-8 名前種別（`__proto__`, `__call`, `___x`, numeric, private static/instance） | `a8-name-kinds`, `a8-leading-expando` ×2, `a8-private-static-instance`（ES5 は upstream 内部エラーとして `upstream_limitations` に記録） | exact ×2 |
| A-9 診断値（7053 / 2322 / 2353 / 2551-2552） | `a9-diagnostic-values`, `a9-spelling-suggestion` ×2；`ratchets/h2-8a-utf16-diagnostic-values-design.v1.json` | exact ×2 |
| A-10 `.d.ts` literal 型ノードと NoAsciiEscaping の出力バイト | `a10-literal-type` ×2；`crates/emitter/tests/fixtures/utf16-declaration-literal-printer.json`, `utf16-literal-escaping.json`（288 cases）, `utf16-writer.json`（48） | exact ×2 / suites 緑 |
| B-1 noEmitOnError 14 | `b1-noEmitOnError/<row>` 14 | exact ×2 |
| B-2 noEmit + declaration | `b2-noEmit-declaration`；`utf16-noemit-command-controls.json` 14 | exact ×2 |
| B-3 構造的 recovery 負対照 | `b3-b4-rejected/{missing-initializer,missing-class,mixed-literal-structure}` | typed refusal ×2、部分書き込みなし |
| B-4 admitted 外の lexical 負対照 | `b3-b4-rejected/{numeric-separator,hex-digits,regexp-eof,comment-eof,conflict-marker,keyword-escape}` | typed refusal ×2 |
| B-5 tagged invalid は診断なし（C の領域） | `b5-tagged-no-diagnostic`（target 4） | exact ×2 |
| B-6 EOF backslash | `b6-eof-backslash`, `b6-value-eof-backslash` | exact ×2 |
| B corpus 差分（拒否→一致 / 拒否→新規不一致） | `utf16-literal-recovery-census.json`（newly-admitted 50 / newly-refused 0 / still-refused 571）；`utf16-literal-recovery-corpus.json` 50 行 | 50/50 exact ×2（printer 修正後；修正前は 49/50） |
| C-1 ES2015/16/17 invalid cooked、nested、複数 substitution、script 版 | `utf16-tagged-template-controls.json` `es2015-mixed`, `nested-invalid-tag`, `raw-cooked-units`, `script-invalid`；`c1-multiple-spans/target-{2,3,4}` | exact ×2 |
| C-2 ES5 混在の採番と tail 順 | `es5-valid-invalid-valid`；`namespace-mixed`（ModuleBlock） | exact ×2 |
| C-3 二重 visit（object spread tag、nested invalid tag、hoisted temp） | `object-spread-tag`, `nested-four-visits`, `rest-assignment-{valid,invalid,no}-tag`, `function-rest-valid`, `function-nested-invalid`, `escaped-valid-rest-tag`, `function-expression-tag` | exact ×2 |
| C-4 ES2018 retained | `es2018-retained-invalid` | exact ×2 |
| C-5 既存 ES5 控除と 64 件 exact 維持 | `utf16-literals-template-literals.json`（26）, `-string-literals.json`（36）, `-bundle-prologues.json`（2） | exact ×2 |
| v1 / v2 receipt の保存 | `utf16-tagged-template-review-v1.json` SHA `92fbb23e…`（§10.3 の値と一致、test が pin）, `-v2.json` | 維持 |

## 1. 判定の要約

| 対象 | 採否 | 条件 |
| --- | --- | --- |
| A（UTF-16 値と symbol 名の同一性） | **受理（follow-up 付き）** | 型定義 → producer → lookup/cache → 診断 → 出力の全経路で所有値が保持され、23/64/41/65 の完全コマンドと本レビューの probe が tsc 6.0.3 と byte 一致。相違はすべて修復前から存在する表示関数・scanner 診断の移植誤りで、値は失われない |
| B（literal-only recovery の emit admission） | **条件付き受理** | 設計 §10.2 のとおり実装されているが、merge 前に (B-1) legacy-decorator adapter test 2 件の修復（本レビューの実行で失敗を確認）、(B-2) harness NoEmit 分岐の `noEmit && declaration` declaration diagnostics 欠落の修復と control 追加が必要 |
| C（tagged template の ES2018 lowering） | **受理（follow-up 付き）** | 要求事項はすべて成立し、C の全入力で byte 一致。唯一の C 経路上の相違（optional-chain tag の callee 括弧）は `ed6d8073a` から存在する継承 gap |
| 全体 | **条件付き受理** | 下記 §5 の must-fix 4 件（うち 3 件は test 側、1 件は harness 経路の production）を閉じ、§6 の receipt を揃えれば merge 可能。ratchet / oracle / CI policy に gap を消す変更は無い |

上の表はレビュー時点（修正前）の判定である。**修正ラウンド後の判定（§9、設計書 §33）**:
A 受理（follow-up A-2〜A-7 を実装済み）、B 受理（M-1・M-2 を閉鎖、B-4・B-5 実装済み、B-3 は記録）、
C 受理（C-1 を閉鎖）、全体 **受理**。残る条件は hosted acceptance（ユーザー実行）と原因別のコミット分割のみで、
§6 の compiler contracts 15 件と emitter `list_comment_flags_contract` はいずれも migration 前（`f0aaa2de2`）から
同一に再現する継承失敗であり、本差分の採否には影響しない。

レーン別の全文報告（確認項目ごとの `file::function:line`、`_tsc.js` 行、probe 一覧）は
`target/declaration-comment-ranges-runs/review-lanes-20260914/review-lane-{a1,a2,a3,a4,a5,b,c}.md` に保存した。
本回答はその統合と、reviewer 自身の probe / 実行結果を加えたものである。

## 2. A: literal 値と symbol identity

### 2.1. 確認できたこと

1. **表現と契約**（lane A1）— `JsString(Vec<u8>)` / `JsStr<'a>{bytes}` は非公開フィールドで canonical WTF-8 を強制し（`js_string.rs:16-27, 74-76, 159-169`）、`push_js` は lead+trail の接合だけを結合する（`188-197`）。`split_at_byte` は継続バイト位置で `None`（`211-225`）。`as_str()` は unpaired surrogate のときだけ `None`（`227-230`）。`Ord` はバイト順（derive、文書化済み）、`cmp_utf16()` は `JsString`/`JsStr`/`EscapedName` の三型にあり、U+E000 < U+10000（バイト）/ U+E000 > U+10000（UTF-16）の witness test が diagnostics と types の両 crate に存在する。`BTreeMap<EscapedName,_>` は無く、lossy `Display`/`Deref` は無く、`forbid(unsafe_code)` は diagnostics/types/binder で維持。`SymbolTable(IndexMap<EscapedName,SymbolId>)` の公開 lookup は `impl Into<JsStr>`（canonical `JsStr`/`&str`/`&JsString`）に限られ、`Hash for EscapedName` と `Borrow<[u8]>` は同じバイト列に委譲（`escaped_name.rs:82-92`）。tuple key（`links.rs:671` `union_property_cache`）は所有 tuple lookup を維持。`escape`/`unescape`/`internal`/`from_identifier_escaped_text`/`quoted_module`/`from_escaped_value`（13 呼び出し元）は `_tsc.js:11438-11444, 42534-42601` 等と対応。`TemplateText` は literal type 値として存続し、`to_js_string`/`eq_utf8` が無損失。
2. **producer**（lane A2）— scanner の token value は `JsString` で、無効 escape は tsc `scanEscapeSequence`（`_tsc.js:9066-9246`）と同じく raw 綴りを cooked 値にする（`"\u000G"` → `.d.ts` `"\\u000G"`、`"\u{D800}"` → `hasExtendedUnicodeEscape` 保存）。五つの literal/template `text` と `template_flags` は parser が所有し、factory/relocate/observable_fields/incremental/codegen に cooked の side table も source 綴りからの再デコードも残っていない。binder の名前 producer は `EscapedName`。`paths` の capture は UTF-16 unit 境界で、pair を割る pattern が scalar file に解決する対照（`utf16-lexical-paths.json` `capture-splits-pair`）がある。entity-name 判定はコメント内の孤立 surrogate で値全体を拒否しない（`parser.rs:9971-10018`）。`new."\uD800"` の panic（consumer audit の suspect）は `parse_identifier_name(None)` で閉じ、tsc fixture 10 cases と一致。
3. **checker / declaration**（lane A3）— property access / element access / `in` / narrowing / keyof / mapped / Pick / Omit / Record / computed / late-bound / enum / quoted ambient module / `__proto__` 系 / unique symbol の producer・lookup・cache が `EscapedName`/`JsStr` 境界で閉じる。診断値は `MessageChain::new_js`、整列は `cmp_utf16`。declaration の property name は nameType 経路と raw symbol fallback の両方が所有値を保持し、`escapeString`/`stripQuotes`/`/\\./g` は UTF-16 unit 単位、`NoAsciiEscaping` の literal type node は生 unit を出力する。JSX の intrinsic / `ElementAttributesProperty` / `ElementChildrenAttribute` は `EscapedName` 比較。A3 の 9 件の完全コマンド probe（NoAsciiEscaping `.d.ts`、enum 4125/2322/2367、keyof/mapped `.d.ts`、診断値 2741/2353/2664/2307/7053/2339、JSX 名、quoted enum chain、accessor pair / unique symbol / template expando）は 2 件（§2.2 の 1・2）を除き byte 一致。
4. **program / host / config / resolver / 出力先**（lane A4）— extended-cache key・継承 base・`pathsBasePath`・循環診断が JS 値（`config.rs:2275-2788`）、JSON の `\ud800` 値は serde_json が lone surrogate を拒否するため JSONC 経路の parser-owned `JsString` で保持、`ProgramPath`/`CanonicalPath` の native 変換は実 I/O（`filesystem_path`、`NativeEmitFileSystem`）と stdout 直前のみ、全 error 型に `js_path()` と native context の両方、resolver の request/cache/continuation/package root/任意拡張子/realpath は JS 値、`withPackageId` と出力→入力の逆引きは `from_utf16_lossy`（基準）から `substring` へ、`moduleSuffixes` の無展開 hit は借用、canonical key は `to_file_name_lower_case_js`、`TSFILE:` は JS 値。9 種の完全コマンド probe（TSFILE、循環 extends、paths hit/miss/star、二重 identity 診断、related）は §6 の scalar 既存差分を除き byte 一致。
5. **emitter**（lane A5）— CommonJS の export/local key と export-location tuple、import property の元ノード、AMD/System の dependency path と grouping/exclusion、生成変数名（`isIdentifierText` gate、`member` stem）、quoted name の element access、D800/D801/FFFD の別公開名（12 controls + a7 完全コマンド）、`.ts`→`.js` の ASCII 境界、JSX raw `reactNamespace`、decorator stem、declaration literal type と `NoAsciiEscaping`、source map の `\udXXX` 引用と `encodeURI` の型付き失敗、writer/sink 境界と BOM を確認。12 件の CLI probe（CommonJS+declaration、AMD、System、ESM 書き換え、JSX raw namespace、jsxFactory fallback、standard decorators、`.d.ts` literal 型、inline source map with lone-surrogate file name、BOM）はすべて byte 一致。
6. **reviewer の probe**（§0.1）— 表示境界（`--pretty false` の stdout と `.d.ts` が tsc と同一、merge-base binary は TS1117 を出す＝修復前の症状）。
7. **consumer 分類表** — production 1,253 行の関数単位分類（`h2-8a-utf16-adjacent-consumer-audit.md`）は、設計レビュー回答 §5 の証拠項目 3（identity 位置の lossy 変換の列挙）を満たす。lane A1/A4 が抽出した「lossy コピー上の ASCII 判定」（§2.2 の 6）は同表で `identity`（等値）に分類されており、判定は同じ。

### 2.2. 修正要求

| # | 重要度 | 箇所 | 内容 | TypeScript の期待 / Rust の相違 | 必要な修正・証拠 |
| --- | --- | --- | --- | --- | --- |
| A-1 | must-fix（test 側） | `crates/program/tests/integration/path_identity_contract.rs::output_directories_preserve_distinct_js_components:80` | 本差分で新設された test が最終ソースで一度も実行されておらず、実行すると失敗する（本レビューの `utf16-review-evidence-native-tests-20260914-045703` `program-all-targets`: 480 passed / 1 failed） | 期待 `"/work/"`、実際 `"/work"`。`inferred_common_source_directory` は `computeCommonSourceDirectoryOfFilenames`（`_tsc.js:121909-121939`）の移植で末尾 separator を付けない。separator を付けるのは `getCommonSourceDirectory`（`116460-116475`）/`getComputedCommonSourceDirectory`（`116476-116482`）であり、production は upstream と一致 | test の期待値を `"/work"` にする（または `common_source_directory` 経由に書き換える）。production は変更しない。再実行 receipt を追加 |
| A-2 | follow-up（同一列車を推奨） | `crates/checker/src/spell.rs::get_suggestion_for_nonexistent_property:327-336`（呼び出し `indexed.rs:1661-1680`、`engine.rs:2668`） | 要素アクセス / excess property の "Did you mean" 候補名が `getNameOfSymbolAsWritten` 綴り（引用符と escape 綴り付き）になる | tsc は `symbolName(suggestion)`（`_tsc.js:75518-75521`）: `Did you mean 'abcdef<D800>'?`。Rust: `'"abcdef\uD800"'`。`ed6d8073a` から同一（main 由来）。既存 control `a9-spelling-suggestion` は候補が発火しない | `symbol_display_name`（unescape）へ置換し、発火する control（名前 7 unit 以上、`__` 接頭辞、2561 flavor）を追加 |
| A-3 | follow-up | `crates/checker/src/merge.rs::symbol_name_as_written_slice:422-478` | 宣言を持たない合成 property の表示が nameType 経路を通らない（`Record<"\uD800", number> = {}` → `Property '<D800>'`、tsc は `'"<D800>"'`；ASCII `Record<"a-b",number>` でも同じ） | `getNameOfSymbolAsWritten` は `getNameOfSymbolFromNameType` を先に試す（`_tsc.js:55586-55588`）。main 由来 | 宣言なし時に `check.rs::symbol_name_from_name_type_slice` 相当を先に適用 |
| A-4 | follow-up | `crates/emitter/src/execute.rs::emit_files_with_activity:1058,1089`、`artifact.rs::EmitArtifact:85` | callback 文字列が UTF-8 投影（U+FFFD）で固定され、tsc の `writeFile` callback（`_tsc.js:16644-16650`）が運ぶ生 unit（NoAsciiEscaping の literal type）を API 境界で区別できない。ファイル bytes は一致（probe）、hosted comparator も UTF-8 bytes 比較なので merge を止めない | tsc は callback に raw unit、`sys.writeFile`（`5164-5182`）で初めて U+FFFD | `PrintedText::text_utf16()` 由来の lossless accessor（例 `callback_units()`）を追加し、`export const k = "\uD800" as const; export const k2 = k;` を完全コマンド control に昇格、または投影を受容偏差として文書化し `captured_write` を fail-closed に |
| A-5 | follow-up | `crates/types/src/escaped_name.rs:112-116` `impl DiagnosticArgument for EscapedName` | escape 済み綴りをそのまま診断値にする入口。現行呼び出し元は無し（probe `'__x'` 等は tsc と一致） | 名前は常に `unescapeLeadingUnderscores` を経て表示（`_tsc.js:11452-11456`） | impl を削除するか、`unescape()` を返す表示用 API に置き換え文書化 |
| A-6 | follow-up | `crates/program/src/loader.rs::resolve_runtime_dependency_symlinks:899-905` | `/node_modules/` 判定を `to_string_lossy` コピー上で行う（結果は同値：ASCII 部分列は置換で不変） | `includes("/node_modules/")` を JS 値のまま | `JsStr::contains` / `split_ascii` へ置換（動作変更なし） |
| A-7 | follow-up | `crates/syntax/src/scanner.rs::scan_escape_sequence:1146-1151` / `scan_extended_unicode_escape:1269-1275` / `scan_unicode_escape:1064,1087-1091` | 修復前からの scanner 診断の移植差: (a) template 末尾 `\`+EOF で TS1160（tsc は TS1126、`_tsc.js:9069-9072`）、(b) `\u{FFFFFFFFFF}` で TS1125（tsc は TS1198 を escapedStart から、`9221-9226`）、(c) overflow 時に `pos` が戻らず token 列が変わり、拒否された escape の `UNICODE_ESCAPE` flag が前 token に残って `if\u0020(x)` に余分な TS1260 | 値（cooked）は失われない。admission は origin で決まるためコード差は admission に影響しない | 各分岐を upstream に合わせる（詳細は lane A2 報告 F1–F3） |
| A-8 | follow-up（文書） | 依頼書 §6 / 設計書 | 「追加 controls の対応表」が提出物に無い（本回答 §0.2 で復元） | — | §0.2 を設計書または依頼書へ取り込む |
| A-9 | note | `crates/emitter/src/execute.rs::encode_uri:394-400`、`cli.rs:48-51` | `encodeURI` 失敗経路の CLI exit が 2（tsc は未捕捉 `URIError` で exit 1、書き込み無しは同じ） | `_tsc.js:116826-116857`, `123693` | 方針（1 か記録済み偏差 2 か）を決めて control を追加。入力自体が upstream crash なので低優先 |
| A-10 | note | `escape/unescape` 三重実装（`syntax/src/lib.rs:430-437`、`escaped_name.rs`、`binder/src/symbols.rs:332-344`）、`escapeString` worker の重複（`merge.rs:22-56` と `check.rs:13936-13974`）、`chains.rs:420` / `prepared.rs:739` のバイト順 sort（lookup 専用で不活性） | 同値だが将来の乖離源 | — | 単一実装への集約、sort の `cmp_utf16` 化または注記 |

### 2.3. 未証明事項

- U-A1 `push_str` を孤立 lead の直後に呼ぶ直接 unit test、`split_ascii` の孤立 surrogate path の unit test が無い（構成上の証明のみ）。
- U-A2 `BTreeMap<JsString,_>`/`BTreeSet<JsString>` を反復して出力へ流す箇所の網羅監査（lane A1 が読んだ範囲は lookup 専用; 未読の候補は lane A1 報告 未証明 3 に列挙）。
- U-A3 `paths` の `*` 付き substitution で capture の lone unit が pair を再形成する経路の TS 観測付き対照が無い（`js_replace_stars` の実装読解のみ）。
- U-A4 `IDENTIFIER_HAS_EXTENDED_UNICODE_ESCAPE` を Rust parser が付けないが ES5 出力は一致（等価性が別経路で成立する理由は未検証）。
- U-A5 AMD / System / JSX / decorators / source map で孤立 surrogate 名を含む**完全コマンド** control が無い（CLI probe は一致だが fixture 化されていない）。enum 関係診断 4125 の unit 列、発火する spelling suggestion、宣言なし property の nameType 表示、`NoAsciiEscaping` `.d.ts` の書き込み bytes（probe p1 は一致）も同様。
- U-A6 `withPackageId` の `submodule_name` が pair 内で切られる場合、`exports`/`imports` の `*` pattern subpath、`ConfigExtendedCache` の case-insensitive key は読解のみ。
- U-A7 Windows / 非 UTF-8 OS path、case-insensitive host の完全コマンドは未観測（platform limitation として記録済み）。
- U-A8 compiler-enforced な raw/escaped `&str` 区別（設計 §10.1 "still outstanding"）は型では強制されていない（`from_escaped_value` の 13 呼び出し元は手動確認済み）。

## 3. B: parse diagnostic の emit 拒否境界

### 3.1. 確認できたこと

1. `syntax/src/recovery.rs`: retained diagnostic の origin 列（`diagnostic_origins`）と committed recovery event（`events`）は独立で、`is_literal_only` は `diagnostic_origins.len() == parse_diagnostics.len()`・全 origin literal・全 event literal（`SilentMissingNode` は常に拒否）で決まる（`recovery.rs:63-75`）。code や fixture 名の allowlist は無い（`"\xG"` admitted と `0x` refused が同じ TS1125 で分かれる）。
2. origin は `push_parse_diagnostic_with_index` の明示引数で通り、lexical origin を作るのは `drain_scanner_errors` だけ（11 の drain 呼び出し元を lane B が列挙）。trivia の error は producer で `trivia_kind` を持ち、直後の string/template token に帰属しない（probe: `/* unterminated`、conflict marker、JSX `<div a="oops`）。
3. 同一 start 抑止、無診断の missing node（Rust 3 箇所 ↔ tsc 3 箇所）、`try_parse`/`look_ahead` の rollback（origin と event を diagnostics・scanner error と同時に truncate）、JSDoc の別リスト、top-level await reparse、incremental reuse で事実が失われず、rollback した事実が残らない。
4. `preflight_source`（`builtins.rs:15695-15713`）は `has_only_literal_recovery()` を無条件に読み、保持診断を空にしても structural event が残れば拒否する（emitter unit test）。
5. `emit_command_for_harness` の NoEmit 分岐は typed H0 `run` を実行し `EmitOutcome::no_emit_without_build_info` の値だけを付ける（resolver/plan/writer/artifact/sink の活動ゼロ、`emitSkipped:false`、map/list option ごとの空リスト、exit 2 = `_tsc.js:125636-125640 / 123526-123548 / 116530-116553 / 129480-129485`）。noEmit14 の全行・b2 control と一致。
6. gate は A の declaration 値の上で検証されている: B-1 noEmitOnError 14 行、B-2 noEmit+declaration、B-3 structural、B-4 admitted 外 lexical（6 種の typed refusal）、B-5、B-6、lexical-only の完全コマンド（元の 23 + 新規 50 corpus 行）。§0.2 参照。
7. census（`utf16_literal_recovery_census.rs`）の母集合（recorded plans + embedded input を持つ全 artifact）は acceptance の corpus と一致し、verdict 論理は健全（`unchanged-admitted` は構成上 0、`newly-refused` 0 は退行無しの証明）。`.json` unit の parity 除外 3 件は妥当（JSON は preflight されない）。
8. reviewer probe: JSDoc 診断（`.js` + checkJs）は tsc と同一に emit + TS1005 + exit 2（JSDoc リストは admission 外）。未終端 regex（TS1161）は typed refusal（設計どおり；tsc は emit）。
9. 依頼書 §4 の但し書き（H0 filesystem config の option 制限）は維持: CLI は noEmit + `declaration` を typed に拒否する（reviewer probe: `unsupported-config-option: compiler option "declaration" is outside the H0 no-emit single-project driver`、exit 2）。

### 3.2. 修正要求

| # | 重要度 | 箇所 | 内容 | TypeScript の期待 / Rust の相違 | 必要な修正・証拠 |
| --- | --- | --- | --- | --- | --- |
| B-1 | **must-fix（test 側）** | `crates/emitter/tests/integration/active_transform_contract.rs::transform_and_print_legacy_decorator_recovery_at_target:6390-6431`（`parse_diagnostics.clear()` は 6401） | adapter が診断を消して full script pipeline を呼ぶが、新 predicate は retained origin 数と診断数の一致を要求するため `ParseDiagnosticsDeferred { count: 0 }` で panic する。本レビューの `utf16-review-evidence-native-tests-20260914-045703` `emitter-contracts-full`: 450 passed / 2 failed（`legacy_explicit_this_rules_keep_admission_serialization_and_runtime_distinct`、`legacy_private_expression_flag_is_scanned_before_runtime_admission`）。設計書 §19 自身が警告していた stage-isolation contract の破綻で、これまでの run は `artifact_sink_contract` filter 付きだったため未検出 | tsc は両入力とも emit する（TS1433 ×3 の入力、`this[#x]` 入力）。production predicate は正しく、弱めてはならない | adapter を production predicate を弱めずに修復: 入力を tsc / tsc-rs ともに parse 診断ゼロの形にするか、`SourceFile` に `#[doc(hidden)]` の test 専用「recovery record を空にする」API を追加して bypass を明文化。`this[#x]` の parser 乖離（tsc: checker TS1451、Rust: parser TS1005）は syntactic band の別項目として記録 |
| B-2 | **must-fix（production、harness 経路）** | `crates/compiler/src/lib.rs::emit_command_for_harness:914-937`（NoEmit 分岐）、`run_inner` | `noEmit && getEmitDeclarations(options)` のとき tsc は `program.getDeclarationDiagnostics()` を追加する（`_tsc.js:129433-129440`）が、NoEmit 分岐は `run()` の 5 bucket だけを詰める | 再現 `export const C = class { private x = 1; };` + `{noEmit:true, declaration:true}`: tsc CLI は TS4094 exit 2（reviewer 観測）。tsc-rs の CLI は H0 config 制限で `declaration` を typed 拒否するため直接観測不能だが、harness 経路はコード上 診断なし exit 0 になる。既存 control（noEmit14 の `declaration-map/*`、`b2-noEmit-declaration`）は declaration 診断を生まない入力なので未検出 | NoEmit 分岐に `get_declaration_diagnostics` の連結（条件: 他の診断が config 診断のみのとき）を追加するか、当該分岐で `declaration`/`composite` を typed error で拒否して範囲を明記。いずれも TS4094 型の control 行（noEmit + declaration）を `utf16-noemit-command-controls.json` に追加し 2 回照合 |
| B-3 | follow-up | `crates/syntax/src/scanner.rs::scan:266-305` | conflict marker trivia（`isConflictMarkerTrivia`）が未実装。`b3-b4-rejected/conflict-marker` は typed refusal で通るが、拒否理由が設計（trivia 由来）と異なる（parser 診断 10 件） | tsc: TS1185 ×3 + emit | 実装時は producer で `ScanError.trivia_kind` を設定すること（さもないと marker 直後の literal に帰属して admitted になる）。producer 列挙の unit test を先に固定 |
| B-4 | follow-up（文書） | 設計書 §32.9 | 「`declarationEmitUnknownImport2` の重複 TS1005 は parser parity の観測」は不正確: h2-7b oracle は codes を `Set` で記録しており tsc の parser も 2 件出す | — | §32.9 を訂正し、census の `typescript_parity` を h2-7b artifact に対しては集合比較にする（または oracle 記録を multiset に） |
| B-5 | note | `builtins.rs::preflight_source:15709-15711` | 拒否メッセージの `count` が silent/抑止 event のみの拒否で 0 になる | — | event 数の併記 |
| B-6 | note | `parser.rs:1373-1387` | `current_node` の再利用判定が候補ごとに全 event を走査し `byte_to_utf16` を毎回計算（性能のみ、結果 tree は同一） | — | hoist |

### 3.3. 未証明事項

- U-B1 compiler `contracts` の全 module 実行（`source_map_emit_witness_contract`、`h2_7d_original_corpus_shared` の拒否行 `const = 5;`（TS1134 ×2）と `[17008,1005]` は構造的で読みでは維持される）— 本レビューの `compiler-contracts-full` の結果は §6 に記す。
- U-B2 B-2 の harness 経路の実測（TS4094 control）。
- U-B3 census の dedupe: recorded plan を先に claim するため、同じ case id が artifact 側で異なる settings 投影を持つ場合は評価されない（case id が option variant ごとに一意であることは未確認）。
- U-B4 conflict marker + literal の admission は producer 実装後まで証明不能。
- U-B5 noEmit の範囲外 option（`incremental`/`composite` の build-info、`listFilesOnly`）は typed refusal / 未モデル化で control 無し。
- U-B6 hosted acceptance（ユーザー実行）。`Deferred(H2_9)` 行は集計のみで実行されないため admission 変更が hosted 結果を変えないという読みは `h2_2c_acceptance.rs:2236-2242` で確認。

## 4. C: ES2018 LiftRestriction の接続

### 4.1. 確認できたこと

1. scanner の `TemplateLiteralLikeFlags` mask は parser-owned `template_flags` として node header に載り、`has_invalid_escape` は `ContainsInvalidEscape`（`_tsc.js:16267-16271`）、`create_template_cooked` は `IsInvalid`（`94018`）の別 mask を同じフィールドに対して読む。raw text からの再構成関数は消えた。clone / update / incremental で保持。
2. transform flags: template flags 非ゼロ → `ContainsES2018`（valid な `\u0061` / `\u{61}` escape でも visitor が起動し `var _a, _b;` を出す witness）。
3. 共有 host は既存の非 hoist numbered allocator（`Es2018Visitor::allocate_local_numbered_binding`、`allocate_numbered_binding`）と source ごとの tail record を使い、hoisted temp と混同しない。
4. valid tag の bounded revisit は memo の読み書きを bounded に無効化して tail record と `hoist_variable_declaration` の両副作用を再現（`var _a, _b;` + `_b`、nested `templateObject_2`、`nested-four-visits`）。
5. 宣言先読みは SourceFile root と ModuleBlock で共通 collector に入り、ES2015 finalizer と print finalizer が同じ番号を出す（ES5 の 2,1,3 と tail 2 文、`namespace-mixed`、function-local、ES5 nested）。
6. C16 controls exact ×2（`utf16-tagged-controls-complete-20260914-033426`）、v1 receipt の SHA `92fbb23e…` を test が pin、lane C の約 100 対の tsc-vs-tsc-rs byte 比較で C の入力に差分なし。reviewer probe（ES2017 hoisted temp 3 変種）も byte 一致。

### 4.2. 修正要求

| # | 重要度 | 箇所 | 内容 | TypeScript の期待 / Rust の相違 | 必要な修正・証拠 |
| --- | --- | --- | --- | --- | --- |
| C-1 | follow-up（同一列車を推奨） | `crates/emitter/src/builtins/es2018.rs::create_call:4767-4786`、`es2015.rs::create_call:1420-1442`（呼び出し `tagged_template.rs:230`） | 生成 call の callee に `parenthesizeLeftSideOfAccess` が適用されない | `export {}; a?.b\`\unicode\`;`（ES2017）で tsc は `(a === null || a === void 0 ? void 0 : a.b)(templateObject_1 || …)`（`createCallExpression` `_tsc.js:22579-22585` → `20466-20471`）、Rust は `… ? void 0 : a.b(templateObject_1 || …)` で意味が変わる。ES5 の valid template でも再現。`ed6d8073a` の ES2015 lane に既にあった欠落を新 lane が継承。到達には TS1358 入力が必要で corpus に無い | 両 host の `create_call`（または factory の CallExpression 規則）で callee に `parenthesize_left_side_of_access` を適用し、4 入力を control 化 |
| C-2 | note（C 外・printer） | `printer.rs:6534-6572` | parse 済み optional-chain tag を印字時に `(a?.b)` で包む（tsc は substitution 後の node にだけ規則を適用、`117193-117196`） | `a?.b\`ok\`;` at ES2020 | 規則適用条件の限定 |
| C-3 | note（C 外・ES2018 parameter） | `es2018.rs` parameter 経路（2296, 2502） | initializer に object rest を含む parameter を `appendObjectRestAssignmentsIfNeeded`（`102709`）で本体へ移さない | `((x = ({a, ...r} = f(), r)) => x)` at ES2017 | ES2018 owner へ |
| C-4 | note（C 外・source map） | — | `for await` / async generator `super.x` で `mappings` のみ相違（template 非依存） | — | source-map owner へ |

### 4.3. 未証明事項

- U-C1 `outFile` bundle 内の tail 採番（CLI は `useCaseSensitiveFileNames` を typed 拒否するため native 未観測；memory host の AMD/System outFile control が閉じる証拠）。
- U-C2 retained ModuleBlock の printer arm は合成 printer test のみ（設計 §16.1 明記）。
- U-C3 memo bypass の同値性は約 20 形状の probe と control で確認したが、`es2018.rs` の再 visit 箇所の網羅監査ではない。
- U-C4 probe に用いた binary（03:26:52）の SHA-256 `accf5b8f…` は receipt に記録されていない（最終 code 変更後・freeze 前の生成物）。
- U-C5 `templateFlags` は `nodes.schema.json` / observable fields に含まれない（oracle syntax 観測との比較には載らない；C の正しさには影響なし）。

## 5. merge 前に閉じるもの（must-fix）と証拠

| # | 種別 | 内容 | 状態 |
| --- | --- | --- | --- |
| M-1 | test | B-1: `active_transform_contract` の adapter 2 test（本レビューで失敗を実測） | 修正済み（§9.2、設計書 §33.2） |
| M-2 | production（harness 経路） | B-2: NoEmit 分岐の declaration diagnostics + TS4094 control | 修正済み（§9.3、設計書 §33.3、control 6 行 exact ×2） |
| M-3 | test | A-1: `path_identity_contract::output_directories_preserve_distinct_js_components` の期待値 `"/work/"` | 修正済み（§9.1、設計書 §33.1） |
| M-4 | 証拠 | 最終ソースでの diagnostics / types / binder / host / program（全 target）/ emitter contracts（全 module）/ compiler contracts（全 module）の receipt。lane A1 R1・A4 R1・B 未証明 1 が指摘 | 本レビューの `utf16-review-evidence-native-tests-20260914-045703` で実行: diagnostics 49+…、types 35+3、binder 71+2、host 3+14…、program lib 46 + contracts 480/481（M-3）、emitter contracts 450/452（M-1）。compiler contracts 全 module: §6 参照 |

修正後に必要なもの: M-1〜M-3 の修正を含む再 freeze と、M-4 の全 target を含む再実行（既存の 11 runner + 上記 7 target）、および §0.2 の対応表の取り込み。→ いずれも修正ラウンドで実施（§9、設計書 §33.14〜§33.15）。

## 6. 本レビューで実行した test target の結果（最終ソース、`utf16-review-evidence-native-tests-20260914-045703`）

| target | 結果 |
| --- | --- |
| `cargo test -p tsc-rs-diagnostics`（全 target） | ok |
| `cargo test -p tsc-rs-types`（全 target） | ok |
| `cargo test -p tsc-rs-binder`（全 target） | ok |
| `cargo test -p tsc-rs-host`（全 target） | ok |
| `cargo test -p tsc-rs-program`（全 target） | lib 46 ok；contracts 480 passed / 1 failed（M-3）；utf16_* / h2_7d / smoke ok |
| `cargo test -p tsc-rs-emitter --test contracts`（全 module） | 450 passed / 2 failed（M-1） |
| `cargo test -p tsc-rs-compiler --test contracts`（全 module） | 416 passed / 15 failed / 16 ignored（11,733 s、inputs unchanged）。15 件の内訳: `emit_session_contract` 2 件（main 由来、§7）、`h2_7a_m4_controls` 3 件、`h2_8a_*` 7 件（class_field_alias_map_positions 28 行、hoisted_declaration_export_ranges 4、static_initializer_map_ranges 4、ellipsis_comment_owners 4、export_name_syntax_maps 8、import_helpers 3、token_comment_phases 1）、`program_session_contract::programmatic_node_module_resolution_relationships_keep_exact_module_names`、`source_map_emit_witness_contract` 2 件。後者 13 件は migration 前のコミット `f0aaa2de2`（= `b652451f0^`、別 worktree・別 target dir、receipt `pre-wtf8-baseline-20260914-081736`）で同一 test・同一行集合が失敗する継承失敗（§9.5） |

いずれも single run（receipt に stdout/stderr SHA と入力 manifest）。既存の 11 runner の ×2 結果は設計書 §32.12。

## 7. 継承・範囲外として記録するもの（本差分の採否に影響しない）

- `origin/main` 由来の既知失敗 5 件（設計書 §32.6–§32.7、§32.12: h2_7e の bundle typed boundary / CLI declaration-dir、emit_session_contract の outDir / AMD outFile boundary、xtask 6c manifest test）。merge-base の pristine worktree で同一に再現。
- CLI pretty reporter の scalar 差分（最後の code frame 後の空行位置、集計表の `:line` 灰色着色と末尾空行）: merge-base binary も同じ出力（§0.1 3、lane A4 R4）。
- options 診断（TS5101）がある場合に semantic 診断（TS2307）を抑止しない差（lane A4 R3、scalar、基準から存在）。
- `containsPath` の比較器（root component の case-insensitive 等値、`toFileNameLowerCase` の保護文字）の scalar 差（lane A4 R5）。
- JSON / 拡張子判定の ASCII case-insensitivity（lane A5 2-3）、JSX `&#x110000;`（tsc は RangeError で crash、Rust は黙って落とす）、harness `decode_utf16` の UTF-16 BOM 置換（pinned corpus では到達不能）、Windows の W-API 経路、`is_alphanumeric` による `.prop` 結合判定、System の `{ 0: a }` binding、builtins の lowercase 拡張子 canary（consumer audit §4）。
- 未終端 regex / comment / numeric などの literal 外 lexical recovery は合意どおり typed refusal のまま（tsc は emit）。H2.9 の所有。

## 8. 手続き上の残件

- 原因別のコミット整理（設計レビュー回答 §5 証拠 1）は未了。差分は未コミット（`utf16-closing-source-freeze-20260914-034941` に patch と archive）。
- hosted acceptance（証拠 7）はユーザー実行。profile artifact の 50 行は再 mint まで `deferred-to-slices`。
- ratchet / oracle / CI policy は無変更（追加された 3 artifact は本ブランチの以前のコミットによる evidence 追加のみ）。xtask comparator の変更は fail-closed 化と型置換で、gap を消す方向の変更ではない。

## 9. 修正ラウンド（2026-09-14、レビュー後）

依頼「指摘事項をあなたが修正してください」に基づき、同じ `~/dev/tsc-rs-declaration-comment-design`
の未コミット差分を起点に、must-fix M-1〜M-4 と follow-up を修正した。指摘ごとの修正内容・判断理由・
検証結果の詳細は設計書 §33（各項）、証拠ディレクトリは設計書 §33.14。既存の freeze
（`utf16-final-source-freeze-20260914-033140`、`utf16-closing-source-freeze-20260914-034941`）と
§32 の receipt は保持し、修正後の証拠は新規ディレクトリに保存した。hosted acceptance はユーザー実行。

| 指摘 | 修正内容 | 判断理由 | 検証結果 |
| --- | --- | --- | --- |
| M-3 / A-1 | test の期待値経路を `common_source_directory`（getCommonSourceDirectory の port）に変更。production 無変更 | 末尾区切りを付けるのは `getCommonSourceDirectory`（`_tsc.js:116460-116475`）であり、`inferred_common_source_directory`（computeCommonSourceDirectoryOfFilenames）は tsc と同じく付けない。test が tsc 関数を取り違えていた | `path_identity_contract` 8 passed（設計書 §33.1） |
| M-1 / B-1 | `SourceFile::discard_parse_recovery_for_harness()`（`#[doc(hidden)]`）を追加し adapter が診断と recovery record を同時に捨てる | production predicate を弱めない。tsc が emit する erroneous 入力を transform に通すのは test 側の明示的な責任 | `active_transform_contract::legacy_*` 46 passed（失敗していた 2 test を含む）、`preflight` unit 2 passed（§33.2） |
| M-2 / B-2 | NoEmit 分岐で `noEmit && getEmitDeclarations` のとき、他 stream が空なら clone した prepared program の第二 session で `get_declaration_diagnostics(WholeProgram)` を取得し semantic の後に連結。`get_emit_declarations` を port | `run()` の H0 no-emitter 契約と typed proof を維持しつつ tsc の gate（`129433-129440`）を再現。`composite+noEmit` は build-info 拒否が先行 | 新 control 6 行（TS4094 exit 2 / clean / semantic-first / syntactic-first / declaration なし / 2 file）exact ×2、既存 noEmit 14 行と H2.7c getter 24 test 不変（§33.3） |
| A-2 | `symbol_name`（symbolName port、private member は `#name`）を候補名・element/object-literal の suggestion・property access の suggestion と related row に適用。JSX 分岐は symbolToString（written face）のまま | tsc は `symbolName(suggestion)`（`75518-75521`、`75452`）。候補名の `__#N@#name` は長さ filter で候補から外れ `#hidden` が発火しなかった | 新 control 6 行 exact ×2（初回は `a2-private-name-candidate` が `__#1@#hidden` で相違 → access.rs 修正で解消）。unit test `excess_property_suggestion_uses_the_written_string_literal_name` の期待値 `'"ns:attribute"'` は tsc probe（`'ns:attribute'`）と矛盾したため tsc 値へ訂正・改名。JSX の unit test は不変（§33.4） |
| A-3 | `symbol_name_as_written_slice` の宣言なし fallback に `symbol_name_from_name_type_slice`（`pub(crate)` 化）を挿入 | `getNameOfSymbolAsWritten` は `getNameOfSymbolFromNameType` を先に試す（`55586-55588`） | 新 control 5 行（`"a-b"`、`"\uD800"`、`ab`、`[-1]`、`1`）exact ×2（§33.5） |
| A-4 | `EmitArtifact` の callback を `EmitCallbackText`（raw unit + UTF-8 投影）で保持、`callback_units()` 追加、3 producer を lossless 化。identity-recovery test の `captured_write` を fail-closed に | tsc の `writeFile` callback は raw string、`sys.writeFile` で初めて U+FFFD | 新 control 2 行（`.d.ts` callback に unit `D800`）exact ×2、artifact sink 8、utf16 emitter tests 4、identity-recovery 65+14 行 ×2（§33.6） |
| A-5 | `impl DiagnosticArgument for EscapedName` を削除 | escape 綴りを診断値にする入口を閉じる | all-targets check exit 0（§33.7） |
| A-6 | `/node_modules/` 判定を JsStr のバイト窓判定に置換、unit test 追加 | lossy 投影上の production 判定を排除（結果は同値） | program lib 2 passed（§33.8） |
| A-7 | F1: `Unexpected_end_of_text` を無条件報告。F2: overflow を 0x10FFFF 超として TS1198。F3: peek が flag を返し消費側だけが挿入、overflow で `pos` 復帰 | `_tsc.js:9066-9072`、`9214-9226`、`9247-9273`、`9285-9305`、`9751-9770` | 新 fixture 23 入力 ×2 exact、syntax lib 175、syntax fixture tests 全 pass、b6 行 exact（§33.9） |
| C-1 | 両 host の `create_call` で callee に `parenthesize_left_side_of_access`（`pub(crate)` 化）を適用 | `createCallExpression`（`22579-22585` → `20466-20471`） | 新 control 6 行 exact ×2、tagged controls・50 行 corpus 不変（§33.10） |
| B-4 | §32.9 を訂正、census parity を集合比較（sort+dedup）に | oracle は codes を Set で記録、tsc parser も TS1005 を 2 件出す | census 再実行（設計書 §33.14） |
| B-5 | `ParseDiagnosticsDeferred` に `recovery_events` を追加し表示に併記 | silent/抑止 event のみの拒否で `count: 0` が誤解を招く | unit test で `count: 0, recovery_events > 0` を固定（§33.12） |
| A-8 | 対応表を設計書 §33.15 に転記、依頼書 §6 に参照追加 | — | — |
| B-3 / B-6 / A-9 / A-10 / C-2 / C-3 / C-4 | 実装せず記録（設計書 §33.13） | 本修復の境界外・性能のみ・方針判断・refactor | — |
| M-4 | 全 target 実行と継承失敗の分類 | compiler contracts 15 失敗のうち 2 件は main 由来、13 件は `f0aaa2de2` で同一に再現 | §6 の表、設計書 §33.14 |

修正後の結果（設計書 §33.14 の表）: 最終ソースで `cargo fmt --check` clean、diagnostics / types / syntax / binder / host / program の全 target、checker lib（1,737）、emitter 全 target（`--no-fail-fast`、継承の `list_comment_flags_contract` 2 行のみ赤）、compiler の UTF-16 / H2.7 top-level test（継承の h2_7e 2 test のみ赤、新 controls 25 ×2 を含む）、compiler `contracts` 全 module（416 / 15 / 16、15 件は §6 の継承集合と同一）、codegen nodes-check と schema-audit exit 0、11 runner 再検証 全 exit 0（×2）、census 再実行 parity 0 mismatch、freeze `utf16-fix-round-source-freeze-20260914-130226`（+ closing freeze）。
