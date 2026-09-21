# H2.8a-A-RES-EMITTER-FINAL — 結果報告（開始 SHA と最終 HEAD、ID/観測ごとの before → after、既存 positive、残る差、未実行）

> 統合担当注記（2026-09-18）：以下はproducer r11の提出報告。追加監査で通常emitの残差と回帰を確認したため、§9.8の「emitter owner未解決0」は統合完了の根拠にしない。[追加監査記録](integration/residual-audit.md)と最終hosted receiptを参照。

（作成中：各節は実測後に確定する。argv / env / exit / 秒数 / log hash は [records/](records/) の `*.meta.json`。）

## 1. 開始点と候補

- 開始 SHA：main `c35e00ccb006e3b4e3e2643491e8fe3207595097`（[records/start.v1.json](records/start.v1.json)：clean、rustc 1.93.0、Node 25.2.1 = `.node-version`、vendor hash 一致、`inventory.py --check` exit 0）。
- worktree `../tsc-rs-emitter-final`、branch `draft/h2-8a-emitter-final`。build：`cargo test --workspace --no-run`（demoted、2 workers）。
- 最終 HEAD / 合成候補 SHA / patch hash：（最終節で確定）。

## 2. EF1 — bound decorator target の末尾コメント（5 commands）

| 集合 | before（開始 SHA） | after（EF1 patch、known 凍結のまま） | after（known 5 行 retire 後） |
| --- | --- | --- | --- |
| `post-t1-residuals` `decorator-comments/` 27 complete commands ×2 | **22 exact / 5 known / 0 failed**（[before](records/ef1/before-decorator-comments.meta.json)、110 s、exit 0） | 22 exact + **5 行「recorded native divergence is exact now; retire」**（[after1](records/ef1/after1-decorator-comments.meta.json)、exit 101 = retire 要求のみ） | **27 exact / 0 known / 0 failed**（[after2](records/ef1/after2-decorator-comments-retired.meta.json)、exit 0） |
| 同 packet probe（bundle 26 行） | 26 exact | 26 exact | 26 exact |

retire した 5 行の元 projection：[post-t1-residuals-known-native.before-retire.json](records/ef1/post-t1-residuals-known-native.before-retire.json)。
隣接回帰と全 101 行（`--all`）は §7 の最終 chain で記録する。

## 3. EF2 / EF3 — 20 unique IDs（21 profile 所属）の再測定

入口 `crates/compiler/tests/emitter_final_rows.rs`（各 2 回、typed diff、`KNOWN` retire 規約）。r1 = 開始 SHA（EF1 patch のみ）。

| round | 変更 | exact / known / failed | 新たに exact |
| --- | --- | --- | --- |
| r1 [ef2-ef3-rows-20](records/measure/ef2-ef3-rows-20.meta.json) | — | 0 / 21 / 0 | — |
| r2 [ef2-ef3-rows-20-r2](records/measure/ef2-ef3-rows-20-r2.meta.json) | EF3-HARNESS-FLOOR、EF3-ISOLATED、EF2-CLASS-NAME | 5 / 16 / 0（retire 要求） | `isolatedModulesSourceMap`、`jsFileCompilationWithMapFileAsJsWithOutDir`、`requireOfJsonFileWithSourceMap`、`sourceMapValidationVarInDownLevelGenerator`、`sourceMapWithCaseSensitiveFileNamesAndOutDir` |
| r3 [ef2-ef3-rows-20-r3](records/measure/ef2-ef3-rows-20-r3.meta.json) | EF6-UMD-FACTORY（retire 後） | 5 / 16 / 0（exit 0） | — |
| r4 [ef2-ef3-rows-20-r4](records/measure/ef2-ef3-rows-20-r4.meta.json) | EF2-PROMISE-CTOR、EF2-ARROW-PARENS、EF4-DEFAULT-NAME（第 1 案）、EF2-ASYNC-SUPER（es2017 gate） | 7 / 14 / 0（retire 要求） | `asyncAwait_es5`、`emitAccessExpressionOfCastedObjectLiteralExpressionInArrowFunctionES5` |

| r5 [ef2-ef3-rows-20-r5](records/measure/ef2-ef3-rows-20-r5.meta.json) | EF2-READ-COMMENT、EF5-ESCAPED、ES2018 async-super gate（第 1 案） | 途中 panic（ES5 async generator の capture 構造を壊した；r6 で修正） | — |
| r6 [ef2-ef3-rows-20-r6](records/measure/ef2-ef3-rows-20-r6.meta.json) | ES2018 capture 再構成、EF4-NESTED-THIS、EF4-ARROW-CRASH、EF4-DEFAULT-NAME v2、EF6-JSDOC-LINK | 7 / 13 / 1（retire 要求） | `destructuringVariableDeclaration1ES5iterable` |
| r7 [ef2-ef3-rows-20-r7](records/measure/ef2-ef3-rows-20-r7.meta.json) | EF2-ASYNC-ALIAS-MARK、EF2-ASYNC-GEN-BODY-FLAG、EF2-TOP-LEVEL-FOR-AWAIT（ゲートのみ）、EF5-BARE-CLONE-SPELLING（第 1 案）、EF2-ARROW-BLOCK-ORDER、EF2-BLOCK-SCOPED-DECORATED、EF4-FILE-THIS-CAPTURE | panic（module top-level `for await` が enclosing async function を要求；chain8） | — |
| r7b（r7 最終バイト） [ef2-ef3-rows-20-r7b](records/measure/ef2-ef3-rows-20-r7b.meta.json) | ＋ for-await plan の fallback、EF5 の merge 継承復元＋`createExportExpression` bare clone | 10 / 11 / 0（exit 0） | `asyncImportedPromise_es5`、`emitter.asyncGenerators.classMethods.es5`（KNOWN から retire 済み） |
| r8 [ef2-ef3-rows-20-r8](records/measure/ef2-ef3-rows-20-r8.meta.json)（中間 `-r8b`/`-r8c`/`-r8d`） | 第 2 依頼（DESIGN §7.1 r8 行）：EF2-ALIAS-NUMBERING、EF2-DETACHED-COMMENT、EF2-AWAIT-USING-MISSING-NAME、EF3-ITERABLE-2318、EF6-IMPORT-TYPE-SELF、EF3-CASE-CANONICAL の outFile guard 解除、EF2-LOOP-VARIABLE-POLICY | r8c **14 / 5 / 2**（retire 要求 2）→ r8d **16 / 2 / 3**（retire 要求 3）→ **r8（最終バイト）19 / 2 / 0（exit 0）** | `decoratedBlockScopedClass2`、`ES5For-of37`、`sourceMapValidationDestructuringForArrayBindingPattern`（6c／6a の 2 所属）（r8c）；`awaitUsingDeclarationsInForAwaitOf.3`、`awaitUsingDeclarationsInForAwaitOf`（r8c、retire 後の r8d で確認）；`decoratedBlockScopedClass3`、`awaitUsingDeclarationsInForOf.1`、`awaitUsingDeclarationsInForOf.5`（r8d）— 9 所属すべて KNOWN から retire 済み |

残る所属（r8 後）：EF3 の大小無視 host 2 行だけ（`sourceMapWithNonCaseSensitiveFileNames`、`…AndOutDir`）。tsc-rs の最終バイトは提案観測（方針 1、[records/oracle/case-canonical-proposal.md](records/oracle/case-canonical-proposal.md)）と両行とも完全一致しており、凍結観測（identity canonical）との差は map `sources` の相対 path だけ；integrator の再採取と比較の完了まで KNOWN（未解決）のまま。
DESIGN §5–7.1 に原因と決定を記載。

## 4. EF4 / EF5 — 旧 class 40 commands の再測定

入口 `crates/compiler/tests/emitter_final_batch.rs ef4_ef5_…`。r1 [ef4-ef5-class-40](records/measure/ef4-ef5-class-40.meta.json)：**8 exact / 32 failed**
（class-header-token 8 行は開始 SHA で exact）。capture（[ef4-ef5-class-40-capture](records/measure/ef4-ef5-class-40-capture.meta.json)）の JS diff で 5 原因：
EF4-DEFAULT-NAME 4、EF4-NESTED-THIS 8、EF4-FILE-THIS-CAPTURE 8、EF4-ARROW-CRASH 8（command 失敗）、EF5-ESCAPED 4（DESIGN §7）。r8 最終バイト（[ef4-ef5-class-40-r8](records/measure/ef4-ef5-class-40-r8.meta.json)）：**exact 40 / failed 0**（r7b と同じ、退行なし）。
r4（`legacy-bound-this` 4 行）：`default_2`（数値は 1 つずれ）→ r5 で assigned-name 追跡を実装。
r5 [ef4-ef5-class-40-r5](records/measure/ef4-ef5-class-40-r5.meta.json)：**10 exact / 30 failed**（class-header-token 8 + escaped 2）。
r6 [ef4-ef5-class-40-r6](records/measure/ef4-ef5-class-40-r6.meta.json)：**26 exact / 14 failed**（nested-computed-name 8、legacy-bound-this 4、static-block-arrow 4 が exact；残り concise-arrow / field-arrow / legacy-static-block ×4 = EF4-FILE-THIS-CAPTURE、direct-escaped ×2 = EF5-BARE-CLONE-SPELLING）。
r7 [ef4-ef5-class-40-r7](records/measure/ef4-ef5-class-40-r7.meta.json)（chain8、merge 継承を外した中間バイト）：**36 exact / 4 failed**（escaped ×2 が退行、direct-escaped ×2）→ EF5 の決定を修正（DESIGN §7.1）。
**r7b（最終バイト）** [ef4-ef5-class-40-r7b](records/measure/ef4-ef5-class-40-r7b.meta.json)：**40 exact / 0 failed（exit 0）**。

## 5. EF6 — 旧 global 14 IDs の再測定

入口 `ef6_…`（`assert_output_matrix_projection`）。r1 [ef6-global-14](records/measure/ef6-global-14.meta.json)：**10 exact / 4 failed**。
r3 [ef6-global-14-r3](records/measure/ef6-global-14-r3.meta.json)（EF6-UMD-FACTORY 後）：**11 exact / 3 failed**（`reactImportDropped` が exact；records/probes/probe4 も一致）。
r6 [ef6-global-14-r6](records/measure/ef6-global-14-r6.meta.json)（EF6-JSDOC-LINK 後）：**12 exact / 2 failed**（`linkTagEmit1` が exact）。
r7b（最終バイト） [ef6-global-14-r7b](records/measure/ef6-global-14-r7b.meta.json)：**12 exact / 2 failed**（不変）。
r8 最終バイト [ef6-global-14-r8](records/measure/ef6-global-14-r8.meta.json)（EF6-IMPORT-TYPE-SELF 後）：**14 exact / 0 failed**（`jsDeclarationsExportAssignedClassExpressionAnonymousWithSub` ×2 が exact；records/probes/probe13 の `index.d.ts` も一致）。

## 6. EF7 / EF8

- EF7 台帳 [ledger/](ledger/)：`build_ledger.py --check` exit 0（6,806 行 / 24,875 観測単位、状態別件数は README、非加算）。
- EF8 監査 [ef8/](ef8/)：`axis-matrix.py --check` exit 0（74 要件行：67 covered-hosted / 4 covered-unhosted / 3 uncovered、未被覆 U1–U10、最小入力 P1–P8、A-CLOSE 骨子、API 境界）。

## 7. 隣接回帰（現時点）

| 集合 | round | 結果 |
| --- | --- | --- |
| `printer --all`（142）、`compact-body-comments --all`（240）、`prologue-comments --all`、`declaration-comments --all`、`bundle-metadata-t1 --all`（18） | r1（EF1 後、[records/ef1/adjacent](records/ef1/adjacent/)） | 全て exit 0 |
| emitter `--lib`（508） | r1 / r3 / r4 | 508 passed |
| emitter `--test contracts`（452） | r1 | 451 passed / 1 failed = **inherited red**（開始 SHA でも同一失敗、[inherited-red.md](records/ef1/adjacent/inherited-red.md)） |
| harness `--test contracts`（83） | r3（EF3-HARNESS-FLOOR 後） | 83 passed |
| `printer --all`、`compact-body-comments --all`、`decorator-binding --all` | r5 [records/measure/*-all-r5](records/measure/) | exit 0 |
| `printer --all`、`compact-body-comments --all`、`class-header-token-metadata --all`、`declaration-comments --all`、`jsdoc-return --all`、`decorator-binding --all` | r6 [records/measure/*-all-r6](records/measure/) | exit 0 |
| emitter `--lib`（508） | r5 / r6 | 508 passed |
| emitter `--test contracts`（452） | r5 / r6 | 451 passed / 1 failed = 同じ inherited red |
| checker `--lib`（1738） | r6（EF6-JSDOC-LINK 後） | 1738 passed |
| r7b = chain9（r7 最終バイト、resumable、`CARGO_BUILD_JOBS=1`） | `records/measure/*-r7b*` | 全て緑（inherited red 1 件を除く） |
| **r8 = chain10（最終バイト）** | §8.1 | 全て緑（inherited red 1 件を除く）；途中の chain10 1・2 回目（`*-r8e`/`*-r8f`）が見つけた退行 2 件（emitter lib 1、contracts 3 本）は r8-patches-7/8 で修正し 3 回目で確認 |

## 8. 最終候補の検証・原因別 commit・未解決・未実行

最終バイト = chain10（`records/measure/*-r8`；r7 は chain9 `*-r7b*`、各 meta.json に head / dirty / argv / env / exit / seconds / log sha256）。

### 8.1 chain10（r8 最終バイトでの一括計測；r7 は chain9 `*-r7b*`）

chain10（scratchpad `chain10.sh`、resumable、`CARGO_BUILD_JOBS=1`）は 3 回走った。1 回目（記録 `*-r8e`）は emitter `--lib` の
`hoisted_exports_leave_detached_comments_for_the_function_declaration` が赤（EF2-DETACHED-COMMENT の取りこぼし → `r8-patches-7`）。
2 回目（`*-r8f`）は、1 回目を止めた際に途中で切った rustc が残した壊れた test-profile rlib で contracts / checker の link が失敗
（`cargo clean -p tsc-rs-emitter` で回復）した後、emitter contracts 3 本が `Resolver(Unavailable { IsDeclarationWithCollidingName })`
で赤（EF2-ALIAS-NUMBERING の visit 時問い合わせに `BlockScopedBindings` gate が無かった → `r8-patches-8`）。3 回目が最終バイト
（focused 先行：[emitter-contracts-focused-r8](records/measure/emitter-contracts-focused-r8.meta.json) 4/4、
[emitter-lib-hoisted-exports-r8](records/measure/emitter-lib-hoisted-exports-r8.meta.json) 1/1；probe 出力は
[chain10-r8-probes.log](records/measure/chain10-r8-probes.log.gz)）。debug hook 0、`cargo fmt --check` 緑、`git diff --check` 緑。

| step | 記録（`records/measure/`） | r7b | **r8（最終）** |
| --- | --- | --- | --- |
| CLI build | [build-cli-r8](records/measure/build-cli-r8.meta.json) | exit 0 | exit 0 |
| probes | [chain10-r8-probes.log](records/measure/chain10-r8-probes.log.gz)、records/probes | 15/17 identical | **26/27 identical**（DIFF = probe2/B、EF2-COMMENT-BOUNDARY 設計項目、行なし） |
| EF2/EF3 rows（21 所属） | [ef2-ef3-rows-20-r8](records/measure/ef2-ef3-rows-20-r8.meta.json) | exact 10 / known 11 / failed 0 | **exact 19 / known 2 / failed 0**（exit 0；known = 大小無視 host 2 行） |
| EF4/EF5 class 40 | [ef4-ef5-class-40-r8](records/measure/ef4-ef5-class-40-r8.meta.json) | exact 40 / failed 0 | **exact 40 / failed 0** |
| EF6 global 14 | [ef6-global-14-r8](records/measure/ef6-global-14-r8.meta.json) | exact 12 / failed 2 | **exact 14 / failed 0** |
| emitter `--lib` | [emitter-lib-r8](records/measure/emitter-lib-r8.meta.json) | 508 passed | 508 passed |
| emitter `--test contracts` | [emitter-contracts-r8](records/measure/emitter-contracts-r8.meta.json) | 451 passed / 1 failed = inherited red | 451 passed / 1 failed = inherited red（不変：`compact_private_function_body_emits_inter_statement_comment_once`） |
| checker `--lib` | [checker-lib-r8](records/measure/checker-lib-r8.meta.json) | 1738 passed | 1738 passed |
| harness `--test contracts` | [harness-contracts-r8](records/measure/harness-contracts-r8.meta.json) | 83 passed | 83 passed |
| witness suites（18、`--all`） | [witness-*-all-r8](records/measure/) | 全 exit 0 | **全 exit 0**：printer / compact-body-comments / class-header-token-metadata / decorator-binding / utf16-literal-witnesses / utf16-identity-recovery / utf16-review-fix / utf16-original-commands / string-literal-identifier-source / utf16-literal-escaping / literal-update / prologue-comments / parameter-temporaries / transpile-routes / declaration-comments / jsdoc-return / post-t1-residuals / bundle-metadata-t1 |

### 8.2 原因別 commit

未 commit（handoff §5：producer は原因別 patch を用意し、integrator が commit/PR/hosted 登録を行う）。合成後の clean candidate = 本ツリーの tracked diff（`records/patches/candidate-tracked.diff`、sha256 `5618e4b0878b58748588c5b09a53d1637a1d7ecdae1ea7d0536aff9e155b2a3c`、2960 行、head c35e00ccb、r8 最終バイト（r8-patches-8 後）で再生成）＋ 未追跡の新規 test 2 本と本ディレクトリ；patch script の sha256 は `records/patches/SHA256SUMS`。
順序と hunk 所属は scratchpad `commit-plan.md` と各 `*-patch.py`（`ef1-patch.py` … `r7-patches-6.py`、`r8-patches-1/2/3/5/6/7/8.py`；順序は `records/patches/README.md`）に固定してある。
共有ファイル（printer.rs / es2015.rs / factory.rs / builtins.rs / downlevel.rs / es2018.rs）は hunk 単位で分割する。

最終ツリーの変更（`git status`、base c35e00ccb）：
- checker：`emit.rs`（EF6-UMD-FACTORY）、`functions.rs` + `modules.rs`（EF2-ASYNC-ALIAS-MARK）、`node_builder/type_nodes.rs`（EF6-JSDOC-LINK）；r8：`check.rs`（EF6-IMPORT-TYPE-SELF `class_expression_assignment_container`）、`globals.rs`（EF3-ITERABLE-2318 `get_global_iterable_type` の global 診断公開）
- emitter：`builtins.rs`（EF6 same-source filter、`create_wrapper_local_name`、`static_this_substitute_flags`、`create_export_access_from_module_name` bare clone）、`builtins/class_fields/downlevel.rs`（EF4-NESTED-THIS、`this` substitute flags、`strip_update_introduced_concise_parentheses`）、`builtins/es2015.rs`（EF2-CLASS-NAME、EF4-DEFAULT-NAME、`get_name`、`note_lexical_this_use`）、`builtins/es2017.rs`（EF2-PROMISE-CTOR、EF2-ASYNC-SUPER）、`builtins/es2018.rs`（capture 再構成、body flag、top-level for-await）、`builtins/legacy_decorators.rs`（`create_declaration_head_name`）、`builtins/system.rs`（same-source filter）、`execute.rs`（EF3-ISOLATED）、`factory.rs`（arrow parens、`clone_node_with_source_spelling`、`set_original_node` guard）、`metadata.rs`（`cloned_identifier_spelling`、`clear_generated_binding`）、`printer.rs`（EF1、EF2-READ-COMMENT、identifier spelling）；r8：`builtins/es_next.rs`（EF2-AWAIT-USING-MISSING-NAME 合成 temp）、`builtins/target_bindings.rs`（print-order 除外、naming-moment 再構成、derived-of-temp 保留、`from_existing(loop_variable)`）、`builtins/es2015.rs`（`colliding_declaration_name_substitute`、substitution の記録済み綴り）、`metadata.rs`（`generated_binding_print_order`、loop flag の merge 伝播）、`printer.rs`（`carried_source_detached`）、`builtins/legacy_decorators.rs`（`create_declaration_head_name`）、`execute.rs`（outFile＋大小無視 refusal arm 撤去）、`builtins.rs` / `class_fields.rs` / `standard_decorators.rs` / `system.rs` / `class_fields/downlevel.rs` / `es2017.rs`（`from_existing` の loop flag 引数）
- harness：`upstream_suites/execution.rs`（EF3-HARNESS-FLOOR）
- tests/fixtures：`emitter_final_batch.rs`、`emitter_final_rows.rs`（新規）、`post-t1-residuals-known-native.json`（5 行 retire）、`h2_8c_transpile/known-open.v1.json`（4 行 retire）；r8：`emitter_final_rows.rs` の `KNOWN` は大小無視 host 2 行だけ（9 所属を retire）
- docs：本ディレクトリ一式（DESIGN / REPORT / records / ledger / ef8）

### 8.3 retire / admission 提案（`ratchets/` は無変更）

案（`ratchets/` は producer が触らない；integrator が retire 時に元 projection を保存する）。run fingerprint = 凍結観測の `run_fingerprint_sha256`（先頭 16 hex、`records/measure` の capture `expected.json`）。

| ratchet | case ID | owner | run fingerprint | both-pass 証拠（exact ×2） |
| --- | --- | --- | --- | --- |
| h2-5h-known-divergences | `conformance/async/es5/asyncAwait_es5.ts#target=es5` | h2-5h-ca-2a-r4 | `d96167b67a158ba1` | r4 → r7 |
| h2-5h-known-divergences | `compiler/emitAccessExpressionOfCastedObjectLiteralExpressionInArrowFunctionES5.ts#target=es5` | h2-5h-ca-2a-r4 | `f9992f3e55109b05` | r4 → r7 |
| h2-5h-known-divergences | `conformance/es6/destructuring/destructuringVariableDeclaration1ES5iterable.ts#target=es5` | h2-5h-ca-2a-r4 | `e1b9e498a496c5bc` | r6 → r7 |
| h2-5h-known-divergences | `conformance/async/es5/asyncImportedPromise_es5.ts#target=es5` | h2-5h-ca-2a-r4 | `1272ba91c616d8e0` | r7（chain8） |
| h2-5h-known-divergences | `conformance/emitter/es5/asyncGenerators/emitter.asyncGenerators.classMethods.es5.ts#target=es5` | h2-5h-ca-2a-r4 | `533261587d3ed8ad` | r7（chain8） |
| h2-6c-known-divergences | `compiler/isolatedModulesSourceMap.ts#default` | h2-6c-m-2-divergence-closure | `43f3b1422ecd6598` | r2 → r7（refusal 撤去 = EF3-ISOLATED） |
| h2-6c-known-divergences | `compiler/jsFileCompilationWithMapFileAsJsWithOutDir.ts#default` | h2-6c-m-2-divergence-closure | `36698afdd557d5cc` | r2 → r7 |
| h2-6c-known-divergences | `compiler/requireOfJsonFileWithSourceMap.ts#default` | h2-6c-m-2-divergence-closure | `14541b7ba8bbd6e0` | r2 → r7 |
| h2-6c-known-divergences | `compiler/sourceMapValidationVarInDownLevelGenerator.ts#target=es5` | h2-6c-m-2-divergence-closure | `a9edc27cf0230eac` | r2 → r7 |
| h2-6c-known-divergences | `compiler/sourceMapWithCaseSensitiveFileNamesAndOutDir.ts#default` | h2-6c-m-2-divergence-closure | `f80b1c63e609756b` | r2 → r7 |
| h2-5h-known-divergences | `conformance/decorators/class/decoratedBlockScopedClass2.ts#target=es5` | h2-5h-ca-2a-r4 | `343e2705d39f9cff` | r8c → r8 |
| h2-5h-known-divergences | `conformance/decorators/class/decoratedBlockScopedClass3.ts#target=es5` | h2-5h-ca-2a-r4 | `e057b3311a1e4d62` | r8d → r8 |
| h2-5h-known-divergences | `conformance/statements/for-ofStatements/ES5For-of37.ts#target=es5` | h2-5h-ca-2a-r4 | `e76ac4116184ffa8` | r8c → r8 |
| h2-5h-known-divergences | `conformance/statements/VariableStatements/usingDeclarations/awaitUsingDeclarationsInForAwaitOf.3.ts#target=es5` | h2-5h-ca-2a-r4 | `379b04a688b7fd94` | r8c → r8 |
| h2-5h-known-divergences | `conformance/statements/VariableStatements/usingDeclarations/awaitUsingDeclarationsInForAwaitOf.ts#target=es5` | h2-5h-ca-2a-r4 | `b76ecac01922ce85` | r8c → r8 |
| h2-5h-known-divergences | `conformance/statements/VariableStatements/usingDeclarations/awaitUsingDeclarationsInForOf.1.ts#target=es5` | h2-5h-ca-2a-r4 | `f27d5e34cf82c000` | r8d → r8 |
| h2-5h-known-divergences | `conformance/statements/VariableStatements/usingDeclarations/awaitUsingDeclarationsInForOf.5.ts#target=es5` | h2-5h-ca-2a-r4 | `369ec3c14c326bc7` | r8d → r8 |
| h2-6c-known-divergences | `compiler/sourceMapValidationDestructuringForArrayBindingPattern.ts#target=es2015` | h2-6c-m-2-divergence-closure | `de20670e9c3952db` | r8c → r8 |
| h2-6a-known-divergences | `compiler/sourceMapValidationDestructuringForArrayBindingPattern.ts#target=es2015` | h2-6a-r3-destructuring-binding-ranges | `e199af8ea077e2ea` | r8c → r8 |

known-native fixture（`post-t1-residuals-known-native.json`、5 行）は本 batch で retire 済み（元 projection：`records/ef1/post-t1-residuals-known-native.before-retire.json`）。
transpile-routes の known-open（`crates/compiler/tests/fixtures/h2_8c_transpile/known-open.v1.json`、fixture であり `ratchets/` ではない）：EF2-ASYNC-SUPER が直した「ES5 async method の `super.m()` が `_super_1` accessor になる」原因の 4 行（`transpile-js/lang/es5-flags`、`program-no-check/flags/es5-loop-capture`、`…/es5-loop-capture-commonjs`、`…/control-checked/es5-flags`）が最終バイトで exact になり suite が retire を要求 → retire 済み（known-open 22 → 18 行、対応する `known-native.v1.json` の 4 observation も削除、contract の count pin 22→18；元 projection：`records/ef2/h2_8c-known-open.before-retire.json` / `h2_8c-known-native.before-retire.json`）。
admission（新 profile）は提案なし：本 batch は既存 profile の行だけを扱った。

### 8.4 未解決（attributed）

DESIGN §7.1 / §11、[ledger/DELTA-r7.md](ledger/DELTA-r7.md)：EF2-ALIAS-NUMBERING（2 行）、EF2-DETACHED-COMMENT（1）、EF2-AWAIT-USING-MISSING-NAME（4）、EF3-ITERABLE-2318（1、2 profile）、大小無視 host（2、oracle host 契約 → 方針 1 採用：[records/oracle/case-canonical-proposal.md](records/oracle/case-canonical-proposal.md) に oracle 修正案・新旧観測差分・再採取と検証手順；`outFile` 行の refusal guard は候補で完全一致（writes・map・TS5101・exit 2、capture `ef2-ef3-r8c`）を確認してから解除；integrator の再採取と Rust 比較が完了するまで 2 行は未解決のまま）、EF6-IMPORT-TYPE-SELF（2）。inherited red：compact-private-body（開始 SHA で再現）。

### 8.5 未実行（hosted）

[records/hosted-entry-proposal.md](records/hosted-entry-proposal.md)：`decorator-binding-pipeline --all`、global 769 / class 1228 全件 replay、EF8 P1–P8 の観測。ローカルの重い replay は chain8 の 1 回のみ（heavy-tests-hosted-ci 方針）。

## 9. r9 — レビュー指摘 3 件への対応（2026-09-18）

指摘：(1) 既存 contracts 失敗（private method 内コメント二重出力）、(2) probe2/B が実際に未修復、(3) EF7/EF8 が調査・提案止まり。

### 9.1 (1) inherited red の正体と修正

[DESIGN §4.7](DESIGN.md#47-補足--inherited-redcompact_private_function_body_emits_inter_statement_comment_onceの正体r9)：二重出力は TypeScript 6.0.3 自身の出力
（[records/compact-private-body/](records/compact-private-body/)：`_tsc.js` CLI と `transpileModule`、ES2015 / ES2022、4 形；tsc-rs CLI は byte 一致 sha256 `27725962…`）。
test の期待値を修正（`compact_private_function_body_duplicates_inter_statement_comment_like_tsc`）→ emitter `--test contracts` **452 / 452**（`emitter-contracts-r9a`、`-r9b`）。

### 9.2 (2) probe2/B — EF2-COMMENT-BOUNDARY の実装

[DESIGN §5.1 実装](DESIGN.md#51-設計-ef2-comment-boundaryprinteref1-の一般化)。probe2/A・B とも tsc と byte 一致；records/probes 33 形すべて出力ファイル単位で一致（`probes-r9a`；r9b では新規 probe 4 形を加えて 40/41、残 1 = accessor keyword の own-line comment → r9c で修正、§9.3）。

### 9.3 (3) EF7 — 未観測範囲の実観測（217 ＋ 1,798）

| round | 対象 | exact / failed | 内訳 |
| --- | --- | --- | --- |
| r9b（開始 SHA の producer） | 217（[emitter-final-universe.json](../../../../../crates/compiler/tests/fixtures/emitter-final-universe.json)） | **88 / 129** | noEmit route 106（replay 側の loader 分岐）、System `using` export 12、member 名の own-line comment 3、decorator parse parens 1、verbatimModuleSyntax gate 4、emitDecoratorMetadata gate 1、TS18055 1、parse-diagnostics boundary 1（DESIGN §9.1.1） |
| r9d（原因修復後、最終バイト） | 217 | （§9.5 の表で確定） | KNOWN = parse-diagnostics boundary（owner H2.9） |
| r9d | 1,798（[emitter-final-universe-plan-base.json.zst](../../../../../crates/compiler/tests/fixtures/emitter-final-universe-plan-base.json.zst)） | （§9.5） | 初回 replay；attributed causes は [ledger/DELTA-r9.md](ledger/DELTA-r9.md) |

観測の要点（TS 側）：217 = writes 142、noEmit 106（`emit_skipped:false`、exit 0 ×67 / 2 ×39）、非 0 exit 87；1,798 = writes 1,090、noEmit 939、emit_skipped 1、非 0 exit 1,006。
新規 producer 修復（DESIGN §9.1.1）：EF7-SYSTEM-USING-EXPORT（system.rs）、EF7-MEMBER-NAME-LEADING-COMMENT（printer.rs、accessor keyword token phase を含む）、EF7-DECORATOR-PARSE-PARENS（printer.rs）、EF7-VERBATIM-GATE / EF7-METADATA-INERT（execute.rs）、EF7-ENUM-18055（checker evaluate.rs）；EF7-NOEMIT-ROUTE は replay が CLI と同じ `load_program` 分岐を採る（producer 変更なし）。

### 9.4 (3) EF8 — P1–P8 の入力・比較

[DESIGN §10.1](DESIGN.md#101-ef8-p1p8--未被覆軸の入力比較の実装r9)：`output-matrix.json` 22 行 ＋ `output-matrix-filesystem.json` 4 行（各 2 回採取）、`contracts h2_8a_output_matrix` **2 tests passed**（`contracts-output-matrix-r9a` 24 s、`-r9b`）。axis matrix：74 要件行 = covered-hosted 67 / covered-unhosted 7 / **uncovered 0**。

### 9.5 r9 chain（records/measure `*-r9a`（chain11、EF2-COMMENT-BOUNDARY のみ）、`*-r9b`（chain13、EF7 producer 修復込み）、`*-r9c/-r9d`（chain14、accessor keyword 修正込み））

| step | r9a | r9b | r9c / r9d |
| --- | --- | --- | --- |
| CLI build | exit 0 | exit 0 (919 s) | exit 0 |
| probes（records/probes 33 ＋ 新規） | 33/33 + probe1 | 40/41（accessor keyword） | §9.6 |
| emitter `--lib` | 508 | 508 | §9.6 |
| emitter `--test contracts` | 452 | 452 | §9.6 |
| `emitter_final_rows`（21 所属） | exact 19 / known 2 / failed 0 | 同 | — |
| `emitter_final_batch`（class 40 ＋ global 14） | 40/40, 14/14 | 同 | — |
| `contracts h2_8a_output_matrix` | 2/2 | 2/2 | — |
| `emitter_final_universe`（217） | pin 失敗（fixture 再採取前） | 88 exact / 129 failed（原因分割） | §9.6 |
| `emitter_final_universe`（1,798） | — | — | §9.6 |
| checker `--lib` | — | 1738 | — |
| witness 18 suites `--all` | 全 exit 0（`witness-*-all-r9a`；utf16-review-fix は r9b バイトで実行） | — | §9.6 |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` は **inherited red**：145 件すべて未変更の `crates/program/src`（`result_large_err` 91、`useless_conversion` 48、`redundant_closure` 3 …；jsdoc-return slice の記録と同じ）。本 batch の変更 crate は `--no-deps` で確認（§9.6） | | |

### 9.6 r9 最終バイト（chain15、records/measure `*-r9`）と全件 census

chain15（r9 最終バイト、canonical）：CLI build / probes 33+8 / emitter `--lib` 508 / `--test contracts` 452 / checker `--lib` 1738 / harness `--test contracts` 83 / `contracts h2_8a_output_matrix` 2 / `emitter_final_rows` / `emitter_final_batch` / witness 18 suites すべて exit 0。
`universe-r9`：217 = **exact 214 / failed 3**（parse-diagnostics boundary 1 ＋ `verbatimModuleSyntax*CJS` 2）。`universe-plan-base-r9`（3,283 s、foreground nice 15）：1,798 = **exact 1,626 / failed 172**。172 行の分類（[DESIGN §9.1.2](DESIGN.md#912-plan-base-1798-行の初回-replayr9-最終バイトchain15-universe-plan-base-r9と-r10-の原因分割) / [§9.1.3](DESIGN.md#913-plan-base-全行-replaychain15-universe-plan-base-r9exact-1626--failed-172から分割した-r11-の原因)）：emit bytes 76、parse-recovery typed refusal 35、診断 34、`promoted class name` refusal 13、`MixedSyntheticRange` refusal 4、module resolution refusal 4、harness projection 2、`noCheck` refusal 2、H2.5h helper alias refusal 1、static auto-accessor panic 1。

### 9.7 r10 / r11 — 原因別修復と最終バイト（chain16a / chain16b、records/measure `*-r11`）

r10（20 原因、`records/patches/r10-patches-{1,2,3}.py`、scratch tree で probe 28/28）で 1,798 行は 1,673 exact / 125 failed（scratch `r10-plan-base`：emit bytes 42、parse-recovery 35、診断 27、promoted class name 13、resolution 4、projection 2、H2.5h 1、static accessor 1）。
r11（13 原因、`r11-patches-{1,2,3}.py`、DESIGN §9.1.3）を加えた canonical の最終バイトで chain16a/16b を走らせた。

| step | 記録（`records/measure/`） | **r11（最終）** |
| --- | --- | --- |
| `cargo fmt --check` | [fmt-r11](records/measure/fmt-r11.meta.json) | exit 0 |
| CLI build | [build-cli-r11](records/measure/build-cli-r11.meta.json) | exit 0 |
| probes（records/probes 33 ＋ r9 scratch 8） | [probes-r11](records/measure/probes-r11.meta.json) → [probes-r11b](records/measure/probes-r11b.meta.json) | 40/41 → **41/41**（差 1 = `sysusing`：`await using`（target esnext、非 lowering）は tsc の `ContainsAwait` を立てない → walker を `createAwaitExpression` 由来だけに修正、chain16b で再計測） |
| probes（r10 28 形 / r11 37 形、tsc 直採取） | [probes-r10set-r11](records/measure/probes-r10set-r11.meta.json)、[probes-r11set-r11](records/measure/probes-r11set-r11.meta.json) | **28/28**、**36/37**（残 1 = `tslibReExportHelpers2`、checker TS2343、KNOWN） |
| emitter `--lib` / `--test contracts` | [emitter-lib-r11](records/measure/emitter-lib-r11.meta.json)、[emitter-contracts-r11](records/measure/emitter-contracts-r11.meta.json) | **508 / 452**（`es2015_await_lowering_restores_prefix_unary_operand_parentheses` は r10 で赤 → r11 `yield yield 0` で緑） |
| checker `--lib` / harness `--test contracts` | [checker-lib-r11](records/measure/checker-lib-r11.meta.json)、[harness-contracts-r11](records/measure/harness-contracts-r11.meta.json) | **1738 / 83** |
| `contracts h2_8a_output_matrix` / `emitter_final_rows` / `emitter_final_batch` | [contracts-output-matrix-r11](records/measure/contracts-output-matrix-r11.meta.json)、[emitter-final-rows-r11](records/measure/emitter-final-rows-r11.meta.json)、[emitter-final-batch-r11](records/measure/emitter-final-batch-r11.meta.json) | **2 / 19 exact・2 known・0 failed / 40+14** |
| witness 18 suites `--all` | [witness-*-all-r11](records/measure/)、[witness-transpile-routes-all-r11b](records/measure/witness-transpile-routes-all-r11b.meta.json) | 17 suites exit 0；`transpile-routes` は r10 の EF7-EXPORTED-REST-HOIST / EF7-CJS-EXPORT-INLINE で h2-8c known-open 3 行（`transpile-js/lang/import-helpers`、`program-no-check/{control-checked,no-check}/import-helpers`：`export const {a, ...r}` の CJS 形）が exact 化 → retire assertion が赤 → `known-open.v1.json` 18 → 15 行（元は [records/ef2/h2_8c-known-open.before-retire-r11.json](records/ef2/h2_8c-known-open.before-retire-r11.json)、`known-native.v1.json` の対応 3 observation も retire、元は同 `h2_8c-known-native.before-retire-r11.json`）、count pin 18 → 15、chain16c 再計測 [witness-transpile-routes-all-r11c](records/measure/witness-transpile-routes-all-r11c.meta.json) exit 0（transpile-js: exact=140 known_open=10 unexpected_open=[] known_open_now_exact=[]；program-no-check: exact=46 known_open=5 unexpected_open=[] known_open_now_exact=[]） |
| clippy | [clippy-nodeps-r11](records/measure/clippy-nodeps-r11.meta.json)（5 crate）、[clippy-emitter-compiler-r11b](records/measure/clippy-emitter-compiler-r11b.meta.json) | 本 batch が触った emitter の 18 件（r9 の `useless_conversion`/`unnecessary_cast`/`too_many_arguments` 等）は r11 で解消。5 crate 一括は **inherited red 145 件**：すべて checker crate の未変更ファイル（`structural.rs` 43、`modules.rs` 36（未変更 hunk）、`engine.rs` 22、`indexed.rs` 11 …、`useless_conversion` 中心）。r9 までは emitter の赤で checker の lint が走らなかっただけで、開始 SHA からの既存分。emitter ＋ compiler の `--no-deps`（lib/bin）は [clippy-emitter-compiler-lib-r11c](records/measure/clippy-emitter-compiler-lib-r11c.meta.json) exit 0（226 s）；`--all-targets` は emitter の未変更 test ファイル（`artifact_sink_contract.rs`、`output_plan_contract.rs`）の inherited 4 件（`duplicate_mod`、`redundant_closure_call`、`needless_borrow` ×2）で赤（[clippy-emitter-compiler-r11b](records/measure/clippy-emitter-compiler-r11b.meta.json)） |
| `emitter_final_universe`（217） | [universe-r11](records/measure/universe-r11.meta.json) | **exact 216 / known 1 / failed 0**（exit 0、666 s；known = parse-diagnostics boundary、owner H2.9） |
| `emitter_final_universe`（1,798） | [universe-plan-base-r11](records/measure/universe-plan-base-r11.meta.json) | **exact 1731 / known 67 / failed 0**（exit 0、4219 s、foreground nice 15；known = §9.8 の owner 付き 67 行） |

### 9.8 未解決（継続中、owner 付き）— `KNOWN` / `KNOWN_PLAN_BASE`

依頼書の契約どおり、通常 emit の未解決行は「継続中」として owner と原因を付けて `crates/compiler/tests/emitter_final_universe.rs` の `KNOWN`（217 集合：1 行）／`KNOWN_PLAN_BASE`（1,798 集合：67 行）に残す（retire assertion 付き：exact 化した行は落とせない）。emitter が owner の未解決行は無い（r11 最終バイトで emit bytes の divergence 0）。

| owner / 原因 | 行 | case（抜粋） |
| --- | --- | --- |
| H2.9 parse-recovery emit boundary（typed refusal `ParseDiagnosticsDeferred`；parse diagnostic を持つ入力の recovery 木の emit は H2.9 の境界） | 35 ＋ 217 集合の 1 | `asyncArrowFunction{6,7,8,9}_{es2017,es5×2,es6}`、`asyncFunctionDeclaration{6,7,9,10}_{es2017,es5×2,es6}`、`esDecorators-decoratorExpression.3` ×2、`topLevelAwaitErrors.1` ×2 |
| module resolution request plan（typed refusal `static-module-request-plan`；bundler resolution の `PlanSourceRequests`、owner = resolution（L2-3 / H2 loader）） | 4 | `bundlerDirectoryModule` ×3 module、`bundlerOptionsCompat` |
| H2.5h corpus adoption：tslib helper import が source 識別子と衝突（typed refusal `importHelpers`） | 1 | `importHelpersWithLocalCollisions#module=es2015` |
| checker（isolatedModules alias 診断：TS2865 / TS2866 / TS1269 / global namespace・enum） | 8 | `isolatedModulesSketchyAliasLocalMerge` ×2、`isolatedModulesShadowGlobalTypeNotValue` ×2、`isolatedModulesExportImportUninstantiatedNamespace`、`isolatedModulesReExportType`、`isolatedModulesExportDeclarationType`、`isolatedModulesGlobalNamespacesAndEnums` |
| checker（JS checking：TS2351 / TS2300 / JSDoc optional param / span length / late-bound typedef） | 8 | `genericDefaultsJs`、`jsExtendsImplicitAny`、`jsExportMemberMergedWithModuleAugmentation2`、`jsdocTypedefBeforeParenthesizedExpression`、`contextuallyTypedParametersOptionalInJSDoc`、`jsFileCompilationConstructorOverloadSyntax`、`jsDeclarationsTypedefAndLatebound`、`checkJsdocSatisfiesTag9` |
| checker（relatedInformation 6500/6501 の elaboration：lib property / index signature） | 3 | `jsdocArrayObjectPromiseNoImplicitAny`、`typeSatisfaction_errorLocations1`、`excessPropertyCheckIntersectionWithRecursiveType` |
| checker（型表示・深さ・関係計算：TS2322 message / TS2589 / TS2859 / mapped tuple） | 5 | `jsxIntrinsicDeclaredUsingTemplateLiteralTypeSignatures`、`deeplyNestedMappedTypes`、`recursiveConditionalCrash4`、`relationComplexityError`、`mappedArrayTupleIntersections` |
| checker（module 診断：TS2616 vs TS2597、tslib helper TS2343 ×2） | 3 | `importNonExportedMember9`、`tslibReExportHelpers2`（ESM tslib entry 経由の再 export）、`tslibMultipleMissingHelper`（複数欠落） |

r9 census 172 行のうち上記以外（emit bytes 76、promote refusal 13、`MixedSyntheticRange` 4、static accessor panic 1、`noCheck` refusal 2、harness projection 2、checker 診断 9）は r10/r11 で exact ×2（[ledger/DELTA-r9.md](ledger/DELTA-r9.md)）。


合成後の clean candidate（r11 最終バイト）= 本ツリーの tracked diff `records/patches/candidate-tracked.diff`（sha256 `241f13aa19a15ca76ad85db8392a19d741d2f5bda3389e790bf3baaf7e4c2d53`、6721 行、head c35e00ccb；tracked 37 ファイル）＋ 未追跡 16 path（本ディレクトリ `emitter-final-batch/` 全体、新規 test 4 本、fixture 4 本、`integration/h2_8a_output_matrix.rs`、observer script 2 本；`git status`）。

### 9.9 未実行（hosted）／integrator 項目

- hosted 登録：`emitter_final_universe`（217 ＋ 1,798、後者は ~55–75 分/demoted → `witness-heavy` 相当の分離 job）、`contracts h2_8a_output_matrix`（[records/hosted-entry-proposal.md](records/hosted-entry-proposal.md)）。
- oracle 再採取：大小無視 host 2 行（方針 1、`records/oracle/`）— 不変。
- clippy：checker crate の inherited 145 件（未変更ファイル、`useless_conversion` 等）は本 batch では触らない（hygiene commit の候補）。`h1-rust-omission-inventory --check` は開始 SHA から `artifact.rs` anchor で赤（inherited；`transform_ecmascript_module` の宣言行はそのまま残した）。
- `ratchets/` / accepted profile / STAGE / hosted policy は無変更（retire 提案は §8.3 ＋ h2-8c known-open 3 行の retire は fixture 側で実施済み）。
