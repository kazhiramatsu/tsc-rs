# L2.3 隔離 prototype：段階 B の結果報告

**提出時点の記録**：以下の数値はClaude再提出時。統合レビューでR1のpath表記保持とR4の追加2件を修正し、identityを含むbytes集計とCI入口を追加した。現在の結果は[統合記録](integration/README.md)を参照。

作成日：2026-09-16（同日、[レビュー R1](REVIEW-01-RESPONSE.md) の R1–R4 修正後に再提出。§10）。base `d9cfb664ad89898b0b5601d85e7e2abf70e32b18`（origin/main）。
設計は [DESIGN.md](DESIGN.md)、後続統合は [INTEGRATION.md](INTEGRATION.md)。
本報告は Program 再利用・watch・LSP の完成を報告するものではない（§9）。

## 1. 提出物

| 種別 | path |
| --- | --- |
| prototype | `crates/program/src/resolution_cache.rs`（`ObservationHost`、`Dependency`/`DependencySet`、`RequestKey`/`OptionsIdentity`/`IdentityValue`、`CachedValue`、`ChangeBatch::violated_dependency`、`ResolutionCache`/`Candidate`/`GenerationHandle`/`GenerationView`、`RetentionLimits`/`ResidentStats`、`CancellationToken`、trace JSON）＋ 8 unit tests |
| 既存 crate の変更 | `crates/program/src/loader.rs`（wildcard 自動 @types 名の列挙を自由関数 `discover_wildcard_type_directive_names` に切り出し、`implied_node_format` を `pub(crate)`）、`crates/program/src/lib.rs`（module 公開） |
| change manifest / native 期待値 | `crates/program/tests/fixtures/resolution_cache/manifest.v1.json`、`expected.v1.json` |
| observer | `scripts/observe-resolution-cache.mjs` |
| contract | `crates/program/tests/resolution_cache_contract.rs`（7 tests：change trace、seeded soak、observation host、review R1–R3 回帰、R4 churn） |
| trace / 計測 | `target/l2-3/traces/*.json`（24 family）、`target/l2-3/summary.json`、`target/l2-3/soak.json`、`target/l2-3/churn.json`。gzip を [validation/](validation/) に退避 |
| レビュー対応 | [REVIEW-01-RESPONSE.md](REVIEW-01-RESPONSE.md)（R1–R4 の原因・修正・回帰・実測） |
| 実行記録 | [validation/](validation/) の `*.log.gz`（本 §8） |
| 開始記録 | [validation/start.txt](validation/start.txt) |
| 依頼資料の注記 | [l2-3-resolution-cache-claude-handoff.md](../l2-3-resolution-cache-claude-handoff.md) 冒頭に提出状態の注記（2026-09-16）を追加 |
| base→candidate patch | [validation/candidate.patch.gz](validation/candidate.patch.gz)（`git diff d9cfb664a` ＋ untracked） |

focused 入口（0 tests 不可）：

```sh
node scripts/observe-resolution-cache.mjs \
  --manifest crates/program/tests/fixtures/resolution_cache/manifest.v1.json \
  --out crates/program/tests/fixtures/resolution_cache/expected.v1.json
taskpolicy -b nice -n 15 env CARGO_BUILD_JOBS=2 \
  cargo test --offline -p tsc-rs-program --test resolution_cache_contract -- --test-threads=1
taskpolicy -b nice -n 15 env CARGO_BUILD_JOBS=2 \
  cargo test --offline -p tsc-rs-program --lib resolution_cache
```

## 2. 三者比較の結果（24 family、105 世代、180 request 観測）

| 集計 | 値 |
| --- | --- |
| cached/reused == fresh resolver（同一 host 状態） | 161 / 161 |
| Rust == native 6.0.3（公開観測 projection） | 173 / 173（request 161 ＋ 自動 @types 名 5 ＋ config 7） |
| native の failedLookup / affecting / resolved 位置のうち Rust 依存に覆われないもの | 0 |
| trace 行 | reused 53、recomputed（値変化あり）39、recomputed（値変化なし）3、fresh 66 |
| 決定性 | 24 family × 2 replay で trace 一致；observer 2 回採取 byte 一致（sha256 `1733c25a…3fe8234`）；soak・churn 2 回一致 |
| 期待 disposition | manifest の `expect`（reused/recomputed/fresh/aborted/config）全行一致 |

再提出前（22 family）は 139/139・151/151 で、既存 22 family の値は再提出後も同一（追加 2 family 分だけ増加）。

family 別（fresh 一致 / native 一致 / reused / recomputed 変化 / recomputed 不変 / fresh）：

| family | fresh | native | reused | rec+ | rec= | fresh |
| --- | --- | --- | --- | --- | --- | --- |
| `module/negative-lookup/create-file/minimal` | 8 | 8 | 5 | 1 | 0 | 2 |
| `module/positive-file/content-change/reuse` | 4 | 4 | 1 | 2 | 0 | 1 |
| `module/positive-file/priority-extension/add` | 4 | 4 | 1 | 2 | 0 | 1 |
| `module/negative-lookup/parent-directory/create-then-file` | 6 | 6 | 1 | 3 | 1 | 1 |
| `module/package/exports/update-delete-invalid-swap` | 6 | 6 | 1 | 4 | 0 | 1 |
| `module/package/types-field-and-types-versions/node10` | 8 | 8 | 2 | 3 | 1 | 2 |
| `module/config-options/identity/switch-and-return` | 7 | 7 | 2 | 0 | 0 | 5 |
| `module/config-options/identity/structured-conditions`（R1） | 14 | 14 | 4 | 0 | 0 | 10 |
| `module/config-options/identity/structured-paths`（R1） | 8 | 8 | 2 | 0 | 0 | 6 |
| `module/mode/esnext-vs-commonjs-distinct-keys` | 6 | 6 | 2 | 2 | 0 | 2 |
| `types-libs/automatic-types/default-typeroots` | 10 | 15 | 6 | 2 | 0 | 2 |
| `types-libs/explicit-typeroots-and-types/custom-root` | 8 | 8 | 4 | 2 | 0 | 2 |
| `types-libs/lib-replacement/add-remove-package` | 4 | 4 | 1 | 2 | 0 | 1 |
| `identity/case-insensitive/folded-keys` | 9 | 9 | 3 | 3 | 0 | 3 |
| `identity/symlink/retarget-and-preserve` | 5 | 5 | 0 | 3 | 0 | 2 |
| `identity/containing-file/move` | 6 | 6 | 2 | 1 | 1 | 2 |
| `generation/hold-old-reader/immutable-view` | 4 | 4 | 1 | 2 | 0 | 1 |
| `generation/shared-across-projects/same-identity` | 6 | 6 | 2 | 0 | 0 | 4 |
| `generation/source-membership/rebind` | 6 | 6 | 3 | 1 | 0 | 2 |
| `failure/host-error/normalize` | 2 | 2 | 0 | 1 | 0 | 1 |
| `failure/cancel/during-build-and-before-publish` | 6 | 6 | 2 | 1 | 0 | 3 |
| `failure/evict/refetch` | 16 | 16 | 6 | 0 | 0 | 10 |
| `module/package-scope/type-change` | 8 | 8 | 2 | 4 | 0 | 2 |
| `config/extends/chain` | – | 7 | – | – | – | – |

最小出発例（`module/negative-lookup/create-file/minimal`）の trace 抜粋：

| 世代 | batch | invalidation | `./missing` | `./unrelated` | host 呼び出し |
| --- | --- | --- | --- | --- | --- |
| g0 | – | – | fresh → NotFound（依存 7：`/p/missing.ts`〜`/p/missing/index.*` の欠落と `/p/missing` directory 欠落） | fresh → `/p/unrelated.ts`（依存 2） | 8（memo hit 3） |
| g1 | changed `/p/unrelated.ts` | 0 / retained 2 | reused | reused | 0 |
| g2 | created `/p/missing.ts` | 1（`file_exists /p/missing.ts=false` 違反）/ retained 1 | recomputed, changed → `/p/missing.ts` | reused | 2（memo hit 1） |
| g3 | – | 0 / retained 2 | reused | reused | 0 |

各世代の native 観測（`resolveModuleName`）は Rust の値と exact。

## 3. 過剰無効化の台帳（結果に影響しない依存の違反）

| family / 世代 | 違反した依存 | 原因 | 6.0.3 での挙動 |
| --- | --- | --- | --- |
| `module/negative-lookup/parent-directory/create-then-file` g1 | `directory_exists /p/lib = false`（空 directory の作成） | 候補 directory の存在を負の依存として記録 | `isInDirectoryChecks` で同じく無効化される |
| `module/package/types-field-and-types-versions/node10` g1 | `file_content …/lib/package.json`（`lib/z` は `types` 変更の影響を受けない） | package.json 本文 digest の単一依存 | `affectingLocations` に package.json が入り同じく無効化される |
| `identity/containing-file/move` g2 | `directory_exists /p/src = true`（最後の file 削除で推論 directory が消える memory host の model） | host model 由来 | 実 FS では directory が残るため発生しない |
| `config/extends/chain` g3（`recomputed_unchanged`） | `directory_entries /p`（`README.md` 作成） | Rust の `CompilerConfigHost` が include base の親 `/p` の一覧も読む（H0.5 の config host 投影） | tsc の `matchFiles` は `/p/src` だけを列挙 |
| `types-libs/automatic-types/default-typeroots` g3/g4 | `directory_entries /p/node_modules/@types`（hidden / typings:null package の追加） | 一覧 digest の変化 | `typeRootsWatches` で同じく再列挙される |

いずれも再計算後の値は不変で、parity は保たれている。config g3 は Rust config host の観測範囲の差で、cache の欠陥ではない（INTEGRATION §2 に記録）。

## 4. 依頼の必須 change trace 行の disposition

| Dependency | 変更・対照 | family | 結果 |
| --- | --- | --- | --- |
| positive file | 変更なし再利用 / 内容変更 / 削除 / 優先拡張子追加 | F02、F03、F01 | reused / reused（内容非依存）/ recomputed→NotFound / recomputed（`.tsx`、`.ts`）、未探索 `.js` は reused |
| negative lookup | NotFound→作成 / 親 directory 作成 / 削除→再作成 / 無関係追加 | F01、F04 | recomputed→resolved / recomputed-unchanged（計測）/ recomputed×2 / reused |
| package | exports / types / typesVersions / type / package.json 作成・削除・不正 JSON / node_modules 入替 | F05、F06、F21、F08 | すべて recomputed、値は native と exact（不正 JSON は `{}` 扱いで index.d.ts、入替後 `pkg@2.0.0`）。`imports` は request として未採取（§9） |
| config/options | extends 先 / paths・baseUrl・rootDirs・moduleSuffixes・customConditions / moduleResolution と mode | F22、F07、F08 | extends 本文・欠落・package extends は `options_changed`；include 一覧は `roots_changed`；option 変更は新 identity で fresh、復帰で reused；mode は別 key |
| types/libs | 自動 @types / 明示 typeRoots・types / lib replacement / default lib / type package 追加・削除 | F09、F10、F11 | 名前列挙の依存で再列挙、追加・削除は recomputed；custom root は node_modules を探索せず reused；`@typescript/lib-dom` 追加・削除は recomputed；default lib は resolution でなく identity（`lib`/`target`）に吸収 |
| identity | case-sensitive/insensitive / symlink 付替 / realpath alias / preserveSymlinks / containing file 移動 | F12、F13、F14 | fold した directory key と原綴り specifier；link 付替は `realpath` 依存で recomputed（`originalPath` と `packageId` が native と exact）；preserveSymlinks は新 identity；移動は directory 単位 key |
| generation | 旧 snapshot 保持 / 複数 project 共有 / source membership 再割当 | F15、F16、F17 | held reader の view は fresh（旧 host 状態）と一致；同一 identity は 1 entry、別 identity は分離；`/p/a.ts` の id が g0=0、g1=1 と世代ごとに再束縛され、行は fresh loader と一致 |
| failure | host error 後の正常化 / 構築中 cancel / publish 直前 cancel / evict 後の再取得 | F18、F19、F20 | 3 種の abort で published 世代 id・entry の `Rc` 同一性が不変；次世代は published 状態からの batch で recomputed；退避 2 件は `Fresh{evicted_before:true}` で parity |

## 5. 失敗・取消・退避の trace 抜粋

`failure/cancel/during-build-and-before-publish`：g0 published(1) → g1 aborted `cancelled`（published 1 のまま）→ g2 aborted `dropped`（同）→ g3 published(2)：reused 2、recomputed 1（g1 で削除した `/p/a.ts`）。
`failure/evict/refetch`：g0 fresh 4（既定上限、退避 0）→ g1 で `max_entries` 2 を設定：reused 4、publish で LRU 2 件退避（entries 2）→ g2 fresh 2（`evicted_before: true`）＋ reused 2、publish で再び 2 件退避 → g3 `evict_all` 後 fresh 4（全件 `evicted_before: true`）。各世代の値は fresh と一致。
`failure/host-error/normalize`：g1 で `file_exists /p/util.ts` に注入した host error は `CacheError::Resolution` になり entry は作られず published 不変；g2（失敗解除）で `/p/util.ts` の作成が published 状態との差分として現れ recomputed → resolved。

## 6. 1000 世代 seeded soak（`target/l2-3/soak.json`、seed `5eed1234abcd0001`、2 回一致）

| 指標 | 値 |
| --- | --- |
| 世代 | published 717、cancelled 216、live 上限で publish 拒否 67（合計 1000） |
| ops | create 282、update 503、delete 267、hold 201、release 192、cancel 233、noop 334 |
| trace 行 | reused 5548、recomputed 変化 511、recomputed 不変 205、fresh 3144（うち退避後 3132） |
| 退避 | 2868 entry（`max_entries` 8、`max_bytes` 64 KiB） |
| 観測最大 | entries 8、bytes 5479、live 世代 6（上限 6） |
| parity 検査 | published entry 9408 件すべて fresh と一致；held reader の entry 17168 件すべて旧 host 状態の fresh と一致 |
| host 通信 | 実 host 呼び出し 26073、memo hit 19120 |
| 最終状態 | entries 8、bytes 4792 |
| 常駐 state（`resident_stats`、`max_eviction_history` 16） | 退避履歴 4 entry / 148 bytes、忘れた key 0、live 世代記録 5、intern identity 746 bytes |

2000 世代 churn（`target/l2-3/churn.json`、毎世代新 specifier ×3 と新 identity、reader 保持と連続 `evict_all`）：退避履歴は上限 32 entry / 1472 bytes で頭打ち（忘れた key 5960）、published 最大 8 entry / 2632 bytes、live 世代記録最大 6、`evict_all` 42 回中 11 回を live 上限で拒否。詳細は [REVIEW-01-RESPONSE.md](REVIEW-01-RESPONSE.md) §4。

## 7. Go 内部の無効化範囲との違い

- Go（pin `1f70213d`）は解決 cache を Program 構築（`fileloader` の `module.NewResolver`）単位で作り、file 変更は project を dirty にして再構築する（`markFilesChanged`：changed/deleted は seen file、created は `SeenFileOrMissingParentDirectory`）。単一 file 変更で import 構造が同じなら `ReuseProgram` が files だけ差し替える。entry 単位の無効化は持たない。
- 6.0.3 は失敗候補 directory / package.json の watcher で entry 単位に無効化する。本 prototype は 6.0.3 の粒度（entry 単位、失敗候補と affecting location）を Go の所有構造（不変 snapshot、owner/refcount、失敗非 cache、negative directory の記録）で実装した。
- version 差：Go の `fromInferredTypesContainingFile` key を採用（6.0.3 は per-directory 共有）。6.0.3 の非相対名 cache の祖先埋めは未採用（hit 率のみ）。
- ownership 差：`Rc`/`RefCell` single-owner；watcher/timer なし（明示 batch）；Program 側の `SourceFileId` 束縛は世代ごとに loader/Program が行う。

## 8. 隣接 regression の実行記録（最終 bytes、`taskpolicy -b nice -n 15`、`CARGO_BUILD_JOBS=2`）

| 実行 | 結果 | 記録 |
| --- | --- | --- |
| `cargo test --offline -p tsc-rs-program --test resolution_cache_contract -- --test-threads=1` | 7 passed / 0 failed（6.6 s；再提出前は 3 tests） | `validation/contract-final.log.gz` |
| `cargo test --offline -p tsc-rs-program --lib resolution_cache` | 8 passed（48 filtered；再提出前は 3） | `validation/lib-unit.log.gz` |
| レビュー `repro.rs` を一時 target として実行（R1/R3 原文、R2 は view 経由） | 3 passed / 0 failed；実行後に target を削除 | `validation/review-repro.log.gz` |
| `cargo test --offline -p tsc-rs-program --test contracts -- --test-threads=2`（module_resolution_contract、automatic_type_directive_loader_contract、library_program_loader_contract、no_lib_program_loader_contract、config 系を含む既存 contracts） | 481 passed / 0 failed / 5 ignored（再提出後の最終 bytes で再実行） | `validation/program-contracts.log.gz` |
| `cargo test --offline -p tsc-rs-program --lib --test h2_7d_bundle_source_facts --test host_platform_smoke_contract` | 51 + 1 + 1 passed（初回提出時。lib.rs の export 追加と resolution_cache.rs 以外は不変） | `validation/program-others.log.gz` |
| `cargo fmt --all -- --check` | 差分なし（`cargo fmt --all` 適用後） | – |
| `cargo clippy --offline -p tsc-rs-program --all-targets -- -D warnings` | 候補 head：lib 144 / lib-test 145 指摘。すべて既存 file（loader.rs 57、prepared.rs 35、module_resolution.rs 31、config.rs 16、path.rs 6、js_path.rs 1、json tests 1）の `result_large_err` 91、`useless_conversion` 48、`redundant_closure` 5 ほか。新規 file（`resolution_cache.rs`、`resolution_cache_contract.rs`）と loader 追加行の指摘は 0。base `d9cfb664a` の同一 command は lib 145 / lib-test 146 指摘（loader.rs 58、他は候補と同数）で、候補は loader.rs の切り出しで 1 件減った以外同一（`validation/clippy-base-compare.txt`） | `validation/clippy-program.log.gz`、`validation/clippy-program-base.log.gz` |
| observer 2 回採取 | families 24 / generations 105 / requests 180、byte 一致 | `validation/observer.log.gz` |
| Go reference tests（pin `1f70213d`、`go test ./internal/project/ -run '^(TestExtendedConfigCacheOwnership|TestRefCountingCaches|TestParseCacheBindsBeforePublishing|TestSnapshotFSBuilder|TestSnapshotFS|TestSourceFS|TestRealpathAliasLifecycle|TestExpandAndFilterWatchEvents|TestSnapshot|TestConfigFileChanges)$' -count=1 -p 1 -json`、helper と同じ env） | 10 top-level / 80 terminal pass events、fail 0、package pass 1.577 s、checkout clean。参照した Go file 自身の test であり Rust prototype の parity ではない | `validation/go-tests.txt`、`validation/go-tests.json.gz` |

hosted 未実行：新規 target `resolution_cache_contract` は `.github/ci/replay.py` に未登録（INTEGRATION §4）。`cargo xtask acceptance` は実行していない（ユーザー指示）。

## 10. レビュー R1（2026-09-16）への対応

統合担当のレビュー [l2-3-resolution-cache-review-01](../l2-3-resolution-cache-review-01/README.md) の
R1（identity 衝突で誤った解決結果）、R2（破棄 candidate が公開 entry の stamp を変更）、R3（別 cache の
candidate を受理）、R4（退避履歴が無限に増える）を同 branch で修正した。原因・修正・回帰テスト・実測は
[REVIEW-01-RESPONSE.md](REVIEW-01-RESPONSE.md)。元 fixture の expected は変更せず、identity 境界の
2 family（native 三者比較）と review 回帰 4 tests、unit 5 tests を追加した。

## 9. 未完了・報告しないこと

- Program 再利用（`isProgramUptoDate` 相当）、watch/LSP の公開 route、build-info、root の persistent cache への切替は含まない。
- `imports`（`#` specifier）と `exports` の pattern trailer、project reference redirect、`ambient module` の再試行（`filesWithInvalidatedNonRelativeUnresolvedImports`）、非相対名 cache の祖先埋めは request family として未採取。
- 型参照の resolutionDiagnostics は Rust の host result に無い（loader 所有）。観測した native 値はすべて空。
- Rust `CompilerConfigHost` の親 directory 一覧読取による config の過剰無効化（§3）。
- 性能（wall / bytes）の実測は soak の推定 bytes のみ。実 FS host での I/O 削減量は未計測。
- hosted CI の entry は未登録。
