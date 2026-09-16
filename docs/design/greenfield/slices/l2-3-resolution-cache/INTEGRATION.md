# L2.3 段階 C：registry / Program / watch への統合仕様（未完了項目つき）

作成日：2026-09-16。段階 A/B は [DESIGN.md](DESIGN.md)・[REPORT.md](REPORT.md)。
本書は L2.3a/L2.3b（[remaining-completion-slices.md](../../remaining-completion-slices.md)）が
本 prototype を本番 snapshot に組み込むときに供給すべき情報と、未成立の前提を列挙する。
Program 再利用・LSP の完成は報告しない。

## 1. 供給すべき情報（required capability）

| capability | 供給者 | 現状 | 本 prototype の受け口 |
| --- | --- | --- | --- |
| `ChangeBatch`（created/changed/deleted file、created/deleted directory、`realpath_changed`、`invalidate_all`） | watch/overlay 層（L2.4、L4） | 未成立。prototype は仮想 FS の状態差分から決定的に算出 | `ResolutionCache::begin(batch, …)`。directory event と realpath 変更（link 付替は配下すべて）を supplier が出せることが条件 |
| request set の再計画（source version 単位） | Program loader / registry（L2.2） | `plan_source_requests` は存在。containing file の version 変更時に request set と mode を再計画する driver が未成立 | key は containing directory 単位。source version は key に入れない（DESIGN §4.9 (a)） |
| implied node format の供給 | Program loader | prototype は `Candidate::package_scope`（`PackageScope` kind）で供給 | `impliedFormatPackageJsons` 相当。loader の `implied_node_format` を `pub(crate)` にした |
| `OptionsIdentity` の供給 | config registry（L2.1） | prototype は `CompilerOptions`＋`ProgramOptions`＋host から生成 | config の `options_changed` は新 identity（旧 entry は退避で消える） |
| config の `RebuildRequest` | config registry | prototype の test driver が `options_changed` / `roots_changed` / `reused` を返す | registry が `parse_config_root_plan` を `ObservationHost::record` 越しに走らせ依存を保持する必要 |
| Program への再束縛 | loader / `PreparedProgramBuilder` | `HostResolvedModule::into_resolved_module(ResolvedModuleTarget)` / `into_resolved_type_reference_directive` は既存 | 世代ごとに新 `SourceFileId` で束縛（F17）。cache value は id を持たない |
| 自動 @types と library の driver | loader | `discover_automatic_type_directive_names`（依存つき）と `resolve_library` は prototype にある | `types` に `"*"` を含むときだけ列挙（6.0.3 と一致） |
| cancellation | service scheduler（L2.4） | `CancellationToken` は明示 flag、request 境界で検査 | host 操作単位の取消は未実装 |
| retention policy | project/service | `RetentionLimits`（entries / bytes / live generations / eviction history）を publish 時に適用。`resident_stats()` が常駐 state（published、履歴、live 記録、intern identity）を報告 | bytes は推定値。実測 heap は未計測 |
| publish / release の順序 | project/service | cache の publish は候補 Program の成功後、old Program の release 後に old `GenerationHandle` を drop | Program publish との原子性は L2.4 の未完了項目 |

## 2. 共有 / 排他境界

- `ResolutionCache`、`Candidate`、`GenerationView` は single-owner（`Rc`、`RefCell`、`!Send`）。project/service の single thread で所有し、別 thread へ移さない。
- 同一 `OptionsIdentity` の project は entry を共有できる（F16）。別 identity は key が分かれる。
- `ObservationHost` は候補世代ごとに 1 つ。memo は世代内でのみ有効で、publish 後は使わない。
- entry の owner = `Rc` strong count（published view ＋ held handle ＋ 構築中 candidate）。`max_live_generations` は publish と `evict_all` で検査し、超過は typed error で published 不変。
- candidate は cache の owner token と parent view object を保持し、別 cache・別 publish 後・`evict_all` 後の candidate は publish で拒否される。`CacheEntry` は不変で、usage stamp は view / candidate working view にある（レビュー R2/R3）。
- 退避履歴は `max_eviction_history` と `max_bytes` で有界。忘れた key は `Fresh{evicted_before:false}` になるので、trace を読む側は candidate JSON の `eviction_history.complete` を見る（レビュー R4）。
- request ごとに `ModuleResolver` を生成する（DESIGN §4.9 (b)）。実 FS の I/O は `ObservationHost` の memo が吸収するが、package.json の JSON 再 parse は世代内 request ごとに起きる。CPU が問題なら `CachedPackage` 相当を `ObservationHost` に memo する拡張が必要（resolver への hook は不要）。
- Rust `CompilerConfigHost` は include base の親 directory も読むため、config の依存が tsc より広い（REPORT §3）。config host の観測範囲を tsc の `matchFiles` に合わせる修正は H0.5 config host の owner 側。

## 3. 正式 readiness の未完了項目

1. L2.1 registry：config 単位の `OptionsIdentity` と `RebuildRequest` の供給、extends 先の owner 管理（Go `OwnerCache` 相当）。
2. L2.2 Program reuse：`isProgramUptoDate` 相当（root/options/missing/import/lib/package 比較）と、`InvalidationReport` からの containing-file 単位 `hasInvalidatedResolutions`。
3. L2.4 publication/cancellation：watcher からの `ChangeBatch` 生成（directory event、symlink alias 展開）、Program と cache の publish 順序、host 操作単位の取消。
4. `imports`（`#`）、pattern trailer、project reference redirect、ambient module 再試行の family 追加と observer 拡張。
5. 型参照 resolutionDiagnostics の所有（loader）と cache value の関係整理。
6. 実 FS host（`FsCompilerHost`）での I/O 削減と heap 実測（同一優先度・同一環境の before/after）。
7. Go の非相対名 cache の祖先埋めを採用するかの判断（正しさには影響しない）。
8. hosted CI への登録は本統合で実施（§4）。実行証跡は[統合記録](integration/README.md)へ追記する。

## 4. hosted entry（登録済み、実行結果は統合記録）

`python3 scripts/witness.py resolution-cache --all` をcontrols jobに登録。
固定Node25.2.1のnative observerは2回採取の一致と凍結expectedへのbyte一致を確認する。
続いてProgram lib56件とcache contract9件を実行し、ignored/filtered/欠落を拒否。
専用入力は本suite、共有Program sourceは全関連coverageを選択する。
詳細・実測は[統合レビュー](integration/README.md)。`cargo xtask acceptance`自体の対象には含めない。

## 5. 統合担当への注意

- [レビュー R1](REVIEW-01-RESPONSE.md) の修正を受領後、統合側でR4のサイズ集計と上限変更適用を追加修正した。レビューの repro.rs は R1/R3 原文のまま pass、R2 は view 経由で同じ不変条件を assert して pass。
- 本 prototype は root CLI の loader/session を変更していない。`load_program` 系は fresh route のまま。
- loader の変更は wildcard 名列挙の関数抽出と `implied_node_format` の可視性のみで、`ProgramLoadError` の変換は元の変数ごとに保持した。既存 program contracts 481 件は緑（REPORT §8）。
- 新規 module は `tsc_program` の公開 API に型を追加する。安定 API の約束ではない。
- clippy は本 toolchain（1.93）で既存 file に 144 指摘があり、新規 file には無い。hosted の gate は clippy を走らせない。
