# UTF-16 adjacent repairs: Claude 実装レビュー依頼

状態: **実装完了・最終ソース固定済み・レビュー可能（2026-09-14）**。
2026-09-13 の[実装継続の引き継ぎ資料](h2-8a-utf16-adjacent-handoff.md)にある §6 の全手順を
同じ dirty worktree 上で完了し、経過と証拠は設計書 §32 に記録しました。
この文書はその最終結果に対するレビュー依頼です。
hosted acceptance / full CI / oracle chain walk は指示どおり未実行で、ユーザーが merge 時に実行します。

## 1. 依頼と判断基準

[設計書](h2-8a-utf16-adjacent-repair.md) §10 と §10.5 の合意に対し、
A（UTF-16 の値と symbol 名の同一性）、B（literal recovery の emit admission）、
C（tagged template の ES2018 lowering）が最終差分で成立するか確認してください。
TypeScript 6.0.3 と同じ操作を、元の UTF-16 値を保って行うことが判断基準です。

ユーザーが再確認した三つの不具合は、元の 23 コマンドに含まれます。

| 不具合 | 実装上の所有者と確認点 |
| --- | --- |
| 異なる孤立 surrogate が同じ symbol 名へ潰れ、2300 / 1117 が出る | scanner/parser の所有値、EscapedName、binder/checker の producer・lookup・cache、診断と declaration 出力 |
| parse diagnostic のある source を一律 emit 拒否する H2.9 | parser-owned provenance により literal-only recovery を許可。構造的回復と literal 外の lexical recovery は拒否を維持 |
| ES2015 target の tagged invalid cooked | ES2018 transform flag、parser-owned template flags、共有 host、tail var、二重 visit の副作用と採番 |

問題ごとに重要度、`file::function` と行番号、再現入力、TypeScript の期待動作、
Rust の相違、必要な修正または不足する証拠を示してください。
「確認できたこと」「修正要求」「未証明事項」を分け、A/B/C ごとの採否をください。
実装を読む際は、型の定義から入力・消費・出力まで所有値と呼び出し経路を追ってください。

## 2. 対象と検証記録

| 項目 | 値 / 固定状況 |
| --- | --- |
| ワークツリー | `/Users/hiramatsu/dev/tsc-rs-declaration-comment-design` |
| ブランチ | `work/h2-8a-declaration-comment-ranges` |
| production 修復前基準 | `ed6d8073ae0f4f4d307f21a37c21a483e0a41bf0` |
| `origin/main` merge-base | `f406f12009cd476e28f9e0dcfb3ae4030563fa67` |
| 現在の head | `b652451f0ec4e6aba47aba3f4fd345c1b9168cdc`。実装は未コミット差分（tracked 369 files、+26,213 / −17,280、untracked 100）に存在する |
| 最終対象 | `target/declaration-comment-ranges-runs/utf16-final-source-freeze-20260914-033140/`（source-manifest.json、tracked-head.patch SHA-256 `424d4a565e57c63b9f96905a9c13e0db4fd467c083f149cc3bf2dde91c9ae953`、untracked archive）。文書更新後の閉じ freeze は設計書 §32.12 末尾に追記する |
| 実行制約 | ネイティブ job は直列、Cargo jobs=2、test threads=1、offline、専用 target dir。重い job は `taskpolicy -b nice -n 15` |
| hosted acceptance / full CI | 未実行（ユーザー側）。`ratchets/` と `crates/oracle/` は無変更 |

以下の所在はすべて `target/declaration-comment-ranges-runs/` の下です。
各 `receipt.json` にコマンド・環境・入力 SHA・ログ・実 exit と実行中の入力不変性があります。
下表は**最終ソース（上記 freeze）での再実行結果**です。中間 receipt は設計書 §31–§32 に残しています。

| 検証 | 最終結果 | receipt ディレクトリ |
| --- | --- | --- |
| 全ターゲット型チェック `--workspace --all-targets --keep-going` | exit 0、error 0、warning 0 | `utf16-workspace-all-targets-check-20260914-032439` |
| 元の adjacent 23 | 完全コマンド exact ×2、supplemental captures 一致 | `utf16-adjacent-complete-20260914-033109` |
| 既存 UTF-16 64 | 完全コマンド exact ×2 | `utf16-prior64-complete-20260914-033206` |
| C の追加 16 | 完全コマンド exact ×2 | `utf16-tagged-controls-complete-20260914-033426` |
| 追加 A/B/C1 65 | 56 完全一致 ×2、9 必須の型付き拒否 ×2、部分書き込みなし | `utf16-identity-recovery-complete-20260914-033500` |
| noEmit の追加 14 | 完全コマンド exact ×2 | `utf16-noemit-command-complete-20260914-033653` |
| declaration-comment 41 | 完全コマンド exact ×2 | `utf16-prior41-complete-20260914-033735` |
| 元の H2.5h 4（typed / complete）と G4a/G4b | 必須比較成功。G5c は修復前と同じ declaration mismatch を別記 | `utf16-original-routes-regression-20260914-033913` |
| checker library | 1737 tests ×2 成功（JSX intrinsic identity test を含む） | `utf16-checker-library-20260914-034034` |
| emitter library / literal integration | 505 + 2 ×2 成功 | `utf16-emitter-literal-suite-20260914-034303` |
| program library | 46 ×2 成功 | `utf16-program-lib-20260914-034311` |
| raw UTF-16 source 境界 | ×2 成功。raw lone は decoder / loader 拒否、pair は一致 | `utf16-raw-source-boundary-20260914-034320` |
| 新規: parser `new.<name>` 対照 | `crates/syntax/tests/new_meta_property_name.rs` 10 cases ×2 成功、syntax crate 全緑 | `utf16-round2-native-tests-20260914-012217` |
| 新規: B 新規 admission 50 行の完全コマンド | 50/50 exact ×2 | `utf16-round5-native-tests-20260914-032509` |
| 新規: prologue-only detached prefix | 8 cases ×2 成功 | `utf16-round5-native-tests-20260914-032509` |
| 変更済み既存 target（compiler/harness/program/emitter/fuzz/conformance） | 設計書 §32.6 の表。4 件は main 由来（下記） | `utf16-focused-native-tests-20260914-003327`、`utf16-round3-native-tests-20260914-012217` |
| `xtask codegen nodes-check` / `xtask schema-audit` | exit 0 / exit 0 | `utf16-focused-native-tests-20260914-003327` |
| 全再検証の sequencing summary | failed: [] | `utf16-final-reverification-20260914-033109.json` |

**完了した追加成果物（設計書 §32）:**

- consumer 分類表: [`h2-8a-utf16-adjacent-consumer-audit.md`](h2-8a-utf16-adjacent-consumer-audit.md)。
  production 1,253 行を関数単位で分類（identity 350、grammar-scalar 147、display 64、native-io 17、
  scalar-observer 105、not-js-value 564、suspect 6）。証拠 `utf16-consumer-audit-20260914-004515/`。
- suspect 6 件の処置: parser `parse_new_expression_stub` を tsc `parseIdentifierName` に合わせて修正
  （`new."\uD800"` の panic 解消、`new."x"` / `new.if` の既存乖離も同時解消）。xtask の fail-open な
  `to_string_lossy` 5 箇所（h1_emit_acceptance 2、h2_2c `source_maps_value`、symbol_audit 2）を fail-closed に変更。
- B admission 前後の corpus 差分: `cargo xtask utf16-literal-recovery-census`（14,219 行、newly-admitted 50、
  newly-refused 0、still-refused 571）。`target/declaration-comment-ranges-runs/utf16-literal-recovery-census.json`
  SHA-256 `16fafabaac5e46d99e0caebd37f215d387a2199dbd36f0473f9fbd678cb78efb`。新規 50 行は
  `scripts/observe-utf16-literal-recovery-corpus.mjs` で TS 完全コマンドを 2 回観測し
  `crates/compiler/tests/h2_8a_utf16_literal_recovery_corpus.rs` で 50/50 exact ×2。
- 隣接修復（B の linkage で発覚、valid escape でも再現）: prologue directive のみの source で tsc が
  detached comment prefix を prologue の後に再出力する経路（`emitSourceFile` → `emitBodyWithDetachedComments`）を
  `printer.rs` に補完。witness `crates/compiler/tests/fixtures/prologue-only-detached-comments.json`（8 cases）。
- JSX checker test は fixture が `flattenDiagnosticMessageText` 連鎖を持つのに test が head だけを比較していた
  観測側の誤りで、checker は変更なし。`host_text_decode_contract` の package name 期待値は tsc が escaped lone surrogate を
  保持する（node で確認）ことに合わせて修正。
- rustfmt 済み（`cargo fmt --all -- --check` 0）、`git diff --check` 0。

**main 由来の既知失敗（本差分の原因ではない）:** `h2_7e_bundle_maps_keep_typed_boundary`、
`h2_7e_cli_status_and_exit_match_every_typescript_observation`（options/declaration-dir; tsc 6.0.3 CLI 自身が
TS5011 と `types/src/` 配置を出す。ground truth `tsc-cli-ts5011-groundtruth-20260914-005619`）、
`emit_session_contract::h2_3a_narrow_out_dir_and_source_family_boundary_fails_closed`、
`emit_session_contract::unsupported_options_and_unadmitted_extensions_fail_before_the_first_sink_call`、
xtask `h2_6c_current_manifest_keeps_only_unclosed_historical_refusals`。いずれも merge-base `f406f1200` の
pristine worktree（`main-baseline-h2-7e-20260914-012637`、`main-baseline-xtask-6c-manifest-20260914-0343*`）で
同一に失敗し、期待値は変更していません。

既存 scalar JSON 観測は、孤立 surrogate を明示的に拒否するテスト専用 adapter を使います。
Path 比較、JSON のフィールド・配列順・null・数値比較は維持します。
これらを任意 UTF-16 の比較証拠に転用せず、任意 UTF-16 の対照は専用の unit 配列で確認します。
中間の失敗や修正履歴は設計書 §11–§32 に残し、この依頼書の最終結果と混ぜません。

## 3. A の重点確認

- `diagnostics::JsString` / `JsStr` の canonical WTF-8 表現で、等価な UTF-16 列が
  一つのバイト列になること。構築、連結境界の surrogate pair、部分列、Eq/Hash を確認する。
- `types::EscapedName` と binder の `SymbolTable` / `Symbol.escaped_name` が同じ契約を持つこと。
  `Borrow<[u8]>` に対応する既定順序はバイト順。JS の順序が必要な呼び出しは明示的な
  `cmp_utf16()` を使う。U+E000 / U+10000 の逆順 witness を確認する。
- 公開 lookup が canonical `JsStr` / `str` を受け、非 canonical な生バイトを受け付けないこと。
  tuple key は異種 lookup の対象外であること。symbol identity 用の `BTreeMap<EscapedName, _>` を新設していないこと。
- scanner の token value、五つの literal/template text payload、factory の合成・更新・clone（cooked 値の side table や元ソース再デコードに依存しないこと）、
  binder/checker の名前 producer と lookup、cache、診断値まで所有権が切れないこと。
  `data.text` と `escaped_name` の消費箇所一覧を最終差分に対応づけること。
  JSX の entity-name 文法判定ではコメント内の孤立 surrogate を理由に値全体を拒否しないこと。
- `paths` の capture 境界は UTF-16 単位で運び、Rust str のバイト添字に使わないこと。
  補助文字の前後の capture と、pair を分割する pattern が scalar ファイルへ解決する対照も確認する。
- declaration の property name は nameType と raw symbol fallback の両経路で所有値を確認する。
  nameType の written name、enum 値の型比較診断でも `escapeString` が孤立 surrogate を値のまま残し、
  `\\uXXXX` という別の文字列へ早期に変換していないことを確認する。
  `stripQuotes` と `/\\./g` の処理は UTF-16 単位・一致する引用符・改行・末尾 backslash も比較し、
  parser の値を source spelling から補い直す実装に戻さないこと。
  一時的な文法判定用入力が、名前や診断値として保存されていないことを確認すること。
- `TemplateText` は literal type 値として存続すること。`keyof`、mapped type、narrowing、
  computed property、module name、internal/private/unique-symbol/numeric name、先頭 `__` の
  escape/unescape を確認すること。表示用の置換文字や escape spelling を identity に使わないこと。
- SourceFile、Diagnostic/RelatedInfo のファイル名と path、外部ソースモジュールの symbol 名を
  canonical JS 値で保持し、診断の整列は `cmp_utf16()`、ソース参照は元の名前で行うこと。
  Formatter の相対パス計算、出力先計算、config/host の失敗情報でも孤立 surrogate が早期に
  replacement character へ潰れていないこと。native I/O・最終表示の変換と内部の名前を区別すること。
- config の extended-cache key・継承元 base・pathsBasePath・循環診断と、resolver の
  request/cache/continuation・package root・任意拡張子・realpath が同じ JS 値を運ぶこと。
  withPackageId と出力先からの入力逆引きは UTF-16 で切り出し、切り出した孤立 surrogate を
  replacement character に変えないこと。moduleSuffixes の無展開 hit は候補を借用すること。
  追加 5 件は native の部分検証であり、既存の host trace・config/module integration と
  完全コマンドの回帰確認を代替しないこと。
- CommonJS の export/local key と export-location tuple、import property の元ノード、
  AMD/System の dependency path と System の grouping/exclusion が JS 値を保持すること。
  出力順は既存の vector/source order、検索は canonical key、生成変数名は識別子用の
  処理に従うこと。quoted name が element access のままか、D800/D801/FFFD が
  別の公開名・依存先として残るかを、追加 12 対照と完全コマンドで確認すること。
  `.ts` 等の書き換えは ASCII 境界だけを切り、名前本体を置換しないこと。
- JSX の raw reactNamespace の回復出力を scalar 制約による既定値へ置換しないこと。
  標準 decorators の helper stem は getHelperVariableName の文法分類に従い、
  非識別子の literal 名では `member` を選ぶ一方、runtime name は元の JS 値を保持すること。
- `.d.ts` の literal type node と `NoAsciiEscaping`、診断の値と表示、JSON/config、module 解決、
  ファイル名・ホスト境界、UTF-16 source の raw lone surrogate について、対応範囲と実測を確認すること。
- `ProgramPath` / `CanonicalPath` と host の JS 入口で、コンパイラの同一性と実ファイルの UTF-8
  変換が混ざらないこと。memory host の key・失敗注入・listing は JS 値を保持し、filesystem host は
  I/O 時に変換しても返す listing の元の parent を保持すること。エラーの `js_path()` と native context
  も確認すること。native API に戻す際の scalar 制約を、module 解決の早期拒否へ持ち込まないこと。
  std::path の component 判定に使う一時的な scalar spelling が保存・検索・出力されないこと。

## 4. B の重点確認

- `syntax::recovery` の retained diagnostic origin と structural recovery event が独立していること。
  `parser::push_parse_diagnostic_with_index` が明示 origin を受け、lexical origin の producer が
  `drain_scanner_errors` に限定されること。scanner trivia を string/template token と誤認しないこと。
- 同一 start の診断抑止、無診断の missing node、lookahead / try_parse / scanner restore、
  JSDoc の別リストへの移動、top-level await reparse、incremental reuse において事実が失われず、
  rollback した事実が残らないこと。preflight が読む parse_diagnostics で被覆が閉じること。
- admission が「structural event がゼロ、保持された全 lexical diagnostic が string/template token」
  で決まり、診断 code や fixture 名の allowlist に依存しないこと。
- 保持診断を空にしても structural event が残れば拒否すること。既存の legacy-decorator
  recovery テスト補助は診断を消して full script pipeline を呼ぶため、stage の検証範囲を
  整理して回帰を確認すること。production の回復条件や期待値を弱めて回避しないこと。
- `ProgramSession::emit_command_for_harness` の NoEmit 接続と
  `EmitOutcome::no_emit_without_build_info` を確認すること。通常の typed NoEmit を実行し、
  incremental/composite を拒否した空の build-info 結果だけを値として返すこと。
  resolver / output plan / writer / artifact / sink の活動がゼロのまま、`emitSkipped:false` と
  map/list option ごとの空リスト、診断報告と exit を upstream に合わせること。
  既存 H0 filesystem config の option 制限まで拡張したと解釈しないこと。
- A の declaration 値が成立した状態で gate を検証すること。`noEmit`、`noEmitOnError`、
  structural-only / mixed / lexical-only の complete-command 対照で、診断・出力・exit を確認すること。

## 5. C の重点確認

- scanner の `TemplateLiteralLikeFlags` mask を parser-owned `template_flags` として運ぶこと。
  raw text から別々の再構成関数を作らないこと。`ContainsInvalidEscape` と `IsInvalid` は同じ
  フィールドに対する異なる mask。clone / update / incremental でも保持すること。
- fragment の transform flags が ES2018 visitor を実際に起動すること。TypeScript の
  `getTransformFlagsOfTemplateLiteralLike` は template flags が非ゼロなら ES2018 を立てるため、
  valid な Unicode / hex escape も対照に含めること。
- shared tagged-template host と ES2018 の source ごとの tail record を確認すること。
  既存の非 hoist numbered allocator を利用し、通常の hoisted temp と tail var を混同しないこと。
- valid tag の二重 visit が memo hit で消えず、tail record と `hoist_variable_declaration` の
  両方の副作用を再現すること。ES2017 の `({a, ...r} = f(), r).f` に対し、valid template は
  `_a, _b` を宣言して `_b` を使い、invalid template と untagged は `_a` 一つになる witness を使うこと。
- `target_bindings::collect_function_body_declaration_name_events` 相当の先読みが SourceFile root と
  ModuleBlock にも適用され、ES2015 / print の共通 finalizer が番号を再び誤らせないこと。
  ES5 の valid / invalid / valid 混在で呼び出し番号 2, 1, 3 と宣言の所属を確認すること。

## 6. 提出時に必要な証拠

| 証拠 | 最終提出時の条件 |
| --- | --- |
| 元の adjacent 23 コマンド | 全ての captured tuple が TypeScript と一致し、同一ソース・バイナリで二回一致 |
| 元の fixture 二本 | §7 の SHA-256 を維持。期待値・comparator・manifest・ratchet・CI policy の変更で差を消さない |
| 追加 controls | A/B/C の上記境界、設計書 §5・§9・§10、レビュー回答 §5 の要求に対応表を付ける（対応表: 実装レビュー回答 §0.2、および設計書 §33.15 に転記。修正ラウンドの追加 controls は設計書 §33 各項） |
| C の v1/v2 witnesses | 既存 receipt を保存。追加 16 controls の `h2_8a_utf16_tagged_template_controls.rs` で全コマンドを照合する |
| C の ModuleBlock / flags | retained ModuleBlock の generated-declaration-order と template flags / factory clone の emitter tests を実行する。namespace の command 対照とは被覆を区別する |
| 既存回帰 | UTF-16 64 controls、declaration-comment 41 controls、G4a/G4b、元の H2.5h 四行の完全コマンド対照 |
| 既知の G5c | 修復前からの declaration mismatch と新規回帰を区別し、結果を省略しない |
| ビルドと生成物 | 最終対象の必要な build / codegen check / 関連 test を記録する。部分 module harness を全体 build と扱わない |
| 再現性 | exact command、cwd、環境、入力/ソース/バイナリ hash、stdout/stderr、実 exit、二回分の capture を保存 |
| 未証明事項 | 成功数に隠さず、未実行・未対応・既存制約・新規問題を具体的に記載 |

ローカル full CI は現在の実行計画に含まれない。最終資料にも実行の有無を正確に記す。
検証コマンドは既存の実行制約（重い job は一つずつ、Cargo jobs=2、test threads=1）で確定する。

## 7. 固定した入力

元の入力: `crates/compiler/tests/fixtures/utf16-literals-adjacent-probes-inputs.json`

```text
44672cd7192c2897f25cf93f0c04892a2c3bcabd00589de83ac32bfff2ee6b69
```

元の TypeScript 観測: `crates/compiler/tests/fixtures/utf16-literals-adjacent-probes.json`

```text
6cc7c405903740308ed765aa0c3d1e72a51521d9207058c50b40391cd9e8a963
```

vendored `typescript.js` / `_tsc.js`:

```text
569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39
1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3
```

設計レビュー回答:
`/Users/hiramatsu/dev/tsc-rs/target/next-slices-20260913/fable-5.1-max-utf16-adjacent-review-response.md`
（647 行、SHA-256 `c62b04d60dad64bea70057e54fe70a1b724ce9896f7f728a7f9f2127b1277236`）。
設計書 §10 はその後のユーザーによる追加レビューを反映しているため、両方を読むこと。

追加 fixture（本継続で新規作成、いずれも TS 観測 2 回一致・observer SHA 付き）:

| fixture | SHA-256 |
| --- | --- |
| `crates/syntax/tests/fixtures/utf16-new-meta-property-name.json` | `e45c558e6e72bad015e861e0ce44865fdda77b1a86c4c9629dfddf4c9ef92d15` |
| `crates/compiler/tests/fixtures/utf16-literal-recovery-corpus.json` | `8974a5d03595a5d9cea728556467ee70e187a5295a57e12ecdb71eb7c0d2e87c` |
| `crates/compiler/tests/fixtures/prologue-only-detached-comments.json` | `65ceacc91bd3fbb4f3260d9f1d4d8fff2971437fa35f0814adc0f80b8f9a27f4` |
| `crates/checker/tests/fixtures/utf16-jsx-intrinsic-identity.json`（§31 で作成、`--check` 一致） | `fd1d5536defa8648aa8e91f9fd148a90574f47360c4b710cd93cff82c00c31c2` |

## 8. 回答先

実装完了後のレビュー回答は、別ファイル
`docs/design/greenfield/slices/h2-8a-utf16-adjacent-implementation-review-response.md`
へ記録してください。対象 head、確認した差分・証拠、実行したコマンドを冒頭に明記し、
レビュー中に production / fixture / 既存 receipt は変更しないでください。
ユーザーからの依頼時点で、この文書が「準備中」のままなら、最終対象の固定が済んでいない状態です。
