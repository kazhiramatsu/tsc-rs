# L2.3 — resolution cache の依存追跡と無効化：snapshot/ownership 設計と隔離 prototype

作成日：2026-09-16（同日、[レビュー R1](REVIEW-01-RESPONSE.md) の修正を反映）。状態：段階 A（source graph・gap・型と不変条件・change manifest）＋
段階 B（隔離 prototype、fresh/reused/native の三者比較、失敗・取消・退避、1000 世代 soak、2000 世代 churn）。
段階 C（後続統合仕様）は [INTEGRATION.md](INTEGRATION.md)、結果は [REPORT.md](REPORT.md)。
依頼：[l2-3-resolution-cache-claude-handoff.md](../l2-3-resolution-cache-claude-handoff.md)、
共通手順：[claude-high-difficulty-handoffs.md](../claude-high-difficulty-handoffs.md)。
本書は L2 全体、watch/LSP、Program 再利用 runtime の activation を許可する packet ではない。
root の loader/session は fresh route のまま変更していない。

## 1. 開始点と保存物

| 項目 | 値 |
| --- | --- |
| worktree / branch | `~/dev/tsc-rs-resolution-cache` / `draft/l2-3-resolution-cache` |
| 開始 SHA | `d9cfb664ad89898b0b5601d85e7e2abf70e32b18`（origin/main。SUPER PR #523 `f6444330` と CI PR #524 `bb2d51c8` を含むことを `merge-base --is-ancestor` で確認） |
| toolchain | rustc 1.93.0、cargo 1.93.0、node v25.2.1（`.node-version` と一致） |
| 6.0.3 pin | `_tsc.js` `1c59e77a…ddd3e3`、`typescript.js` `56917765…12be39`（共通手順の pin と一致、observer が起動時に検証） |
| Go pin | `microsoft/TypeScript@1f70213d4922b434345f639b441681e470c7cfc1`、`target/typescript7/upstream` の HEAD 一致・`status --short` 空を確認。参照 file の SHA-256 は [validation/start.txt](validation/start.txt) |
| 開始 manifest | [validation/start.txt](validation/start.txt)（HEAD、status、toolchain、vendor/Go/Rust 入力 hash） |
| change manifest | `crates/program/tests/fixtures/resolution_cache/manifest.v1.json`（24 family、105 世代（g0 含む）、180 request 観測） |
| native 期待値 | `crates/program/tests/fixtures/resolution_cache/expected.v1.json`（observer が 2 回採取し byte 一致を要求。最終 sha256 `1733c25a…3fe8234`） |
| observer | `scripts/observe-resolution-cache.mjs` |
| prototype | `crates/program/src/resolution_cache.rs`（新規 module、`tsc_program` から公開） |
| contract | `crates/program/tests/resolution_cache_contract.rs`（7 tests：change trace、seeded soak、observation host、review R1–R3 回帰、R4 churn） |
| trace | `target/l2-3/traces/*.json`、`target/l2-3/summary.json`、`target/l2-3/soak.json`、`target/l2-3/churn.json`（test が書く。gzip 退避は validation/） |

段階 B の driver は test-only の仮想 host（`MemoryCompilerHost`）と明示的 change batch を使い、
解決アルゴリズムは既存の `ModuleResolver` をそのまま呼ぶ。fake resolver は使っていない。

## 2. Source graph

### 2.1 6.0.3 `createResolutionCache`（`_tsc.js:128134-129051`）

| 構成要素 | 行 | 役割 | 本設計での対応 |
| --- | --- | --- | --- |
| `resolvedModuleNames` / `resolvedTypeReferenceDirectives` / `resolvedLibraries` | 128152、128158、128166 | containing path ごとの mode-aware 結果 map | `RequestKey{kind, containing_directory, specifier, mode, identity}` の単一 map。kind は畳まない |
| `resolvedFileToResolution` | 128141 | resolved file → 依存する解決の逆引き（`invalidateResolutionOfFile` 用） | `Dependency::FileExists/Realpath` に resolved path が含まれるので逆引き map は不要。batch 照合で同じ集合が求まる |
| `resolutionsWithFailedLookups` / `resolutionsWithOnlyAffectingLocations` | 128139-128140 | 失敗候補 / package.json を持つ解決の watch 対象 | `DependencySet` の負の `FileExists`/`DirectoryExists` と `FileContent` |
| `watchFailedLookupLocation*` → `failedLookupChecks` / `startsWithPathChecks` / `isInDirectoryChecks` / `affectingPathChecks` | 128570-128650、128908-128940、128947-128998 | watcher event を無効化 predicate に変換 | `ChangeBatch` と `violated_dependency`（§4.5）。`isInvalidatedFailedLookup`（128989-128991）と同じ exact / startsWith / in-directory の 3 判定 |
| `invalidateResolutionOfFile` | 128897-128904 | containing file 削除時に file の解決を除去＋その file に解決していた行を無効化 | key が containing directory 単位のため除去は上位 driver の request set 再計画で表現。resolved file の削除は `FileExists{exists:true}` 違反 |
| `onChangesAffectModuleResolution` | 128248-128254 | option 変更で全無効化 | option/host identity を key に含める（`OptionsIdentity`）。旧 identity の entry は残り、戻すと再利用できる（F07 g2/g6） |
| `impliedFormatPackageJsons` / `affectingPathChecksForFile` | 128142、128312-128333、128950-128960 | source の impliedNodeFormat が依存した package.json の watch | `RequestKind::PackageScope`（§4.2） |
| `typeRootsWatches` → `hasChangedAutomaticTypeDirectiveNames` | 128182、128999-129047 | typeRoots 配下の変化で自動 @types 名を再列挙 | `discover_automatic_type_directive_names` が `DirectoryEntries`/`FileExists`/`FileContent` 依存を返し、driver が再列挙を決める |
| `resolveLibrary` / `resolvedLibraries.isInvalidated` | 128511-128540、40643-40648 | `@typescript/lib-*` の Node10 隔離解決 | `RequestKind::Library`（`getOptionsForLibraryResolution` と同じ `{moduleResolution: Node10}`） |
| `startCachingPerDirectoryResolution` / `finishCachingPerDirectoryResolution` | 128282-128292、128306-128345 | Program 構築の前後で per-directory cache を clear/readonly 化 | candidate 世代の `ObservationHost` memo が per-build cache に相当し、publish 後は immutable |

### 2.2 6.0.3 のキーと redirect（`_tsc.js:40327-40640`）

- `createPerDirectoryResolutionCache`（40444）：`toPath(containingDirectory)` → `ModeAwareCache(name, mode)`。
- `createNonRelativeNameResolutionCache`（40513）：`(name, mode)` → directory → result。共通 prefix までの祖先 directory に同じ result を埋める（40551-40565）。本 prototype は祖先埋めを行わない（§4.9 (c)）。
- `createPackageJsonInfoCache`（40419）：`toPath(package.json)` → info。本 prototype では per-request resolver の package cache と `ObservationHost` の per-generation memo が同じ役割を持つ。
- `getKeyForCompilerOptions`（40343-40345）：`affectsModuleResolution: true` の 19 宣言（allowJs, forceConsistentCasingInFileNames, checkJs, jsx, locale, moduleDetection, moduleResolution, baseUrl, paths, rootDirs, typeRoots, moduleSuffixes, resolvePackageJsonExports, resolvePackageJsonImports, customConditions, resolveJsonModule, maxNodeModuleJsDepth, noResolve, jsxImportSource）を `compilerOptionValueToString` で連結し `|pathsBasePath` を付ける。`OptionsIdentity` はこの 19 個に、Rust が別引数で渡す resolver 入力（`preserveSymlinks`、`types`、`configFilePath`、`module`、`target`、`noDtsResolution`、`allowArbitraryExtensions`、`allowImportingTsExtensions`、`libReplacement`）と host profile（case sensitivity、current directory）を加える。

### 2.3 6.0.3 `isProgramUptoDate`（`_tsc.js:122454-122496`）

再利用の 8 条件：`hasChangedAutomaticTypeDirectiveNames` が偽、root 名一致、project reference 一致、各 source の version 一致かつ `hasInvalidatedResolutions(path)` 偽、missing path が依然欠落、`compareDataObjects(options)` 一致、`resolvedLibReferences` の未無効化、config text 一致。
本 prototype は `hasInvalidatedResolutions(path)` の入力（containing file 単位の無効化集合）を `InvalidationReport.invalidated` の key から導ける形で保持する。Program 再利用判定そのものは L2.2 の未完了項目（[INTEGRATION.md](INTEGRATION.md) §1）。

### 2.4 Go reference（`1f70213d`、path は `tsc/internal/` 相対）

| file | 参照した predicate / 関数 | 採用した点 | 採用しなかった点 |
| --- | --- | --- | --- |
| `project/snapshot.go` | `Snapshot.Clone`（477-754）、`cloneForProgram`（105-236）、`processFileChanges`（271-316）、`ref/tryRef/Deref/dispose`（759-833） | 世代は不変・clone で作る、候補構築中は builder（`dirty.*`）だけを変更し `Finalize` で公開、refcount 0 で `dispose` が owner を release | goroutine 並列（`sync`、`atomic`）、ProjectCollection/AutoImports/ATA の再構築 |
| `project/snapshotfs.go` | `SnapshotFS`（56-118）、`snapshotFSBuilder.Finalize`（199-288）、`recordRealpathAlias`（361-375）、`markDirtyFiles`（480-515）、`expandRealpathAliases`（555-587）、`sourceFS.SeenFileOrMissingParentDirectory`（762-780）、`sourceFS.DirectoryExists`（793-799） | per-generation の読み取り memo（`ObservationHost`）、negative directory を依存として記録、realpath alias を変更集合へ展開（driver 側の `realpath_changed`） | overlay（open document）、`cachedvfs`、node_modules 限定の alias 記録（本 prototype は全 realpath 呼び出しを記録） |
| `project/extendedconfigcache.go` / `ownercache.go` | `NewExtendedConfigCache`（25-38）、`hash`（40-51）、`OwnerCache.LoadAndAcquire`（38）/`Release`（78） | config entry は「本文＋全 extended file 本文」の内容 hash で期限切れを判定、owner = snapshot id | 本 prototype の config 依存は hash ではなく host 事実（`FileContent`/`FileExists`/`DirectoryEntries`）で表す（tsc の `extendedConfigCache` と同じ read 単位） |
| `project/refcountcache.go` / `programcounter.go` / `parsecache.go` | `RefCountCache.Acquire`（44）/`Deref`（105）、`AcquireOrError`（66：失敗は cache しない）、`programCounter.Ref/Deref` | 失敗した produce を cache しない（`CacheError::Resolution` は entry を作らない）、owner 数 = 参照数 | parse cache そのもの（L0/L1 の registry が既に存在） |
| `project/projectcollectionbuilder.go` | `DidChangeFiles`（270-345）、`markFilesChanged`（1344-1396：created event は `SeenFileOrMissingParentDirectory` のときだけ dirty） | created path は「見た file か、欠落していた親 directory の配下」のときだけ無効化 → `has_created_ancestor` / `any_created_under` | project 単位の dirty flag（本 prototype は entry 単位） |
| `project/configfileregistrybuilder.go` | `DidChangeFiles`（452-636）、`updateExtendingConfigs`（172-214）、`GetExtendedConfig`（793-808） | config 変更は「extend している config」へ伝播し project を dirty にする → 本設計の `RebuildRequest`（§4.7） | registry 本体 |
| `module/cache.go` | `moduleResolutionCacheKey{containingDirectory, moduleName, resolutionMode, redirectConfigName}`、`typeRefDirectiveResolutionCacheKey{…, fromInferredTypesContainingFile}` | containing *directory* 単位のキー、inferred-types 起点を別キーにする | resolver 単位（= Program 構築単位）の寿命 |
| `compiler/program.go` | `UpdateProgram`（304-315）、`ReuseProgram`（323-416）、`canReplaceFileInProgram`（437）：単一 file 変更で `canReplaceFileInProgram` なら files だけ差し替え、redirect/importHelpers/jsx-runtime に関わる場合は全再構築 | 「containing file の text 変更は解決キーを変えない」根拠の一つ | Program 差し替え本体（L2.2） |
| `project/project.go` | `CreateProgram`（394-471）：`ProgramUpdateKind{Cloned, SameFileNames, NewFiles}` | 世代 trace の disposition 分類の参考 | typings/ATA |

### 2.5 version 差と ownership 差の分離

- **version 差（6.0.3 vs Go）**：Go は resolver（と package.json cache）を Program 構築ごとに作り直し、細粒度の failed-lookup watch を持たない。無効化は file 変更 → project dirty → 再構築（単一 file の `ReuseProgram` fast path を除く）。6.0.3 は失敗候補 directory と package.json の watcher で entry 単位に無効化する。本 prototype は 6.0.3 の entry 単位無効化を採用し、Go の snapshot/owner/refcount 構造で寿命を管理する。
- **ownership 差（Rust）**：`ModuleResolver<'a>` は host/options を借用する one-shot resolver で、cache は上位（project/service）所有。`HostResolvedModule` は `Rc<PackageMetadata>` を含むため single-owner（`!Send`）。Go の `sync.Map`/goroutine 前提は持ち込まない（依頼どおり）。
- **6.0.3 と異なる点（意図的）**：type-reference key に origin（source / automatic）を含める（Go と同じ。6.0.3 は per-directory で共有するため、config directory 直下の source と `__inferred type names__.ts` が同一 entry を共有し得る）。fresh 解決の semantics には影響しない。

## 3. Rust 現行実装との照合（gap 表）

| 現行 | 事実 | gap / 扱い |
| --- | --- | --- |
| `ModuleResolver::package_cache` | one-shot、`resolve_json_config` 中は無効化。寿命延長では依存変化を検出できない（L2 設計 §6 の禁止事項） | 使わない。cache は上位に置き、resolver は request ごとに生成（§4.9 (b)） |
| host 操作 | resolver が使うのは `directory_exists_js`（24 箇所）、`file_exists_js`（4）、`read_file_js`（1：package.json）、`realpath_js`（1）、`current_directory_js`、`use_case_sensitive_file_names`。`read_directory`/`get_directories` は loader の wildcard 自動 @types だけ | 6 操作すべてを `ObservationHost` が memo＋記録する |
| 依存の記録 | resolver に failedLookupLocations / affectingLocations に相当する出力はない（doc comment のみ） | tracing host で外側から採取（依頼手順 1）。native の failedLookup/affecting 集合との包含は contract で検査（REPORT §2） |
| `HostResolvedModule` / `HostModuleResolution` / `HostResolvedTypeReferenceDirective` | host 事実のみ。`into_resolved_module(target)` / `into_resolved_type_reference_directive(target, source)` で Program の `SourceFileId` に束縛 | この境界をそのまま cache value に使う（§4.3）。再束縛は世代ごとに loader/Program 側で行う（F17） |
| `ResolutionKey`（prepared.rs） | `(source canonical, specifier, mode)`：Program の権威 table のキー | cache key（containing directory 単位）とは別物。両者の対応は driver が持つ |
| `ConfigExtendedCache` | canonical path → parsed node。依存追跡なし、`clear` のみ | 使わない。config は `parse_config_root_plan` を `ObservationHost` 越しに走らせ、依存を採取（§4.7） |
| wildcard 自動 @types 名の列挙 | `StagedGraph::discover_wildcard_type_directives`（loader 内） | 自由関数 `discover_wildcard_type_directive_names(host, roots)` に切り出し、loader と cache で共有（loader の error 変換は不変） |
| `implied_node_format` | loader 内 private | `pub(crate)` にして cache の `PackageScope` から呼ぶ。Program 契約は不変 |
| 6.0.3 の `getAutomaticTypeDirectiveNames` | `types` に `"*"` を含むときだけ typeRoots を列挙し、それ以外は `types ?? []`（`usesWildcardTypes`） | Rust loader と一致。manifest の自動 @types family は `types: ["*"]` を指定 |
| checker `program.rs` の `DocumentRegistry` | acquire/update/release と generation。Program snapshot は別 owner | 本 prototype は参照のみ（L2.1/L2.2 前提を未完了として記録） |

## 4. 設計：型と不変条件（`crates/program/src/resolution_cache.rs`）

### 4.1 層構成

```text
real host (Fs/Memory)
  └─ ObservationHost<'h>     : 候補世代ごとの memo（SnapshotFS 相当）＋ request ごとの DependencySet 記録（sourceFS 相当）
       └─ ModuleResolver      : request ごとに new_with_program_options（既存アルゴリズム、無改変）
            └─ CachedValue    : HostModuleResolution / TypeReference outcome / Library outcome / PackageScopeFacts
ResolutionCache { published: Rc<GenerationView>, live: Vec<Weak<GenerationView>>, limits }
  begin(batch) → Candidate（invalidation は開始時に一度だけ計算）→ requests → publish（原子的差し替え）| drop（= cancel）
```

`ObservationHost` は host error を memo しない（後続の成功を観測できる）。error が起きた request は entry を作らず `CacheError::Resolution` を返す（Go `AcquireOrError` と同じ失敗非 cache）。

### 4.2 Key

```text
RequestKey { kind: Module | TypeReference | AutomaticTypeReference | Library | PackageScope,
             containing_directory: PathKey（host case fold 済み canonical directory）,
             specifier: JsString（原綴り、fold しない）,
             mode: ResolutionMode（Unspecified/CommonJs/EsNext）,
             identity: Rc<OptionsIdentity> }
```

- module/type/lib/config/package-scope は kind で分離され、同じ specifier・directory でも別 entry。
- `PackageScope` の specifier は file の basename（拡張子が implied format に効く）。
- `Library` の containing は tsc の synthetic file `__lib_node_modules_lookup_<lib>__.ts`、specifier は `@typescript/lib-*`。
- containing *file* の version は key に含めない（§4.9 (a)）。request set の再計画は上位 driver が source version で行う。
- `OptionsIdentity` は型付き component の列（`IdentityValue::{Undefined, Bool, Integer, NumberBits, Text(JsString), List, Entries, HostError}`）で、等価は構造的（derive）。配列は長さと要素境界、文字列は UTF-16 code unit、未指定と空 list を区別し、文字列連結による別名化が起きない（レビュー R1）。`describe()` と `digest()` は trace 専用。

### 4.3 Value

`CachedValue::{Module(HostModuleResolution), TypeReference(ResolutionOutcome<HostResolvedTypeReferenceDirective>), Library(ResolutionOutcome<HostResolvedModule>), PackageScope(PackageScopeFacts)}`。
成功 / NotFound / 診断 / alternate result / package facts（`PackageId`、`Rc<PackageMetadata>`）を保持し、`SourceFileId` は持たない。I/O failure と cancellation は value にならない。

### 4.4 Dependency

`FileExists{path, exists}`、`DirectoryExists{path, exists}`、`FileContent{path, digest: Option<u64>}`（FNV-1a、None = 読めなかった）、`Realpath{path, target: Option<PathKey>}`、`DirectoryEntries{path, digest}`（`read_directory`/`get_directories` の一覧 digest）。
負の事実（存在しなかった候補・directory）が第一級の依存。package boundary は `DirectoryExists(pkg dir)`＋`FileExists(package.json)`＋`FileContent(package.json)` の組で表れ、config extends は `FileContent(extended)`／欠落時は `FileExists=false`、typeRoots は `DirectoryEntries(root)`、symlink/realpath alias は `Realpath{path, target}` で表す。

### 4.5 ChangeBatch と無効化 predicate

`ChangeBatch { created_files, changed_files, deleted_files, created_directories, deleted_directories, realpath_changed, invalidate_all }`（すべて `BTreeSet<PathKey>`、決定的）。
`violated_dependency(&DependencySet) -> Option<&Dependency>` は最初に違反した依存を返す：

| 依存 | 違反条件 |
| --- | --- |
| `FileExists{p, e}` | p が created/deleted；e=true かつ削除 directory 配下；e=false かつ作成 directory 配下；realpath 変更が p か祖先 |
| `DirectoryExists{p, e}` | p が created/deleted directory；e=true かつ削除 directory 配下；e=false かつ（作成 directory 配下 or p 配下に何か作成された = 暗黙の親作成）；realpath 変更 |
| `FileContent{p, d}` | p が changed/deleted/created；削除 directory 配下；d=None かつ作成 directory 配下；realpath 変更 |
| `Realpath{p, t}` | p の realpath 変更 / 削除 / 作成；t の realpath 変更 / 削除 / 削除 directory 配下 |
| `DirectoryEntries{p}` | p が created/deleted directory；削除 or 作成 directory 配下；p の直下 entry の作成/削除；realpath 変更 |

6.0.3 の `isInvalidatedFailedLookup`（128989-128991：exact / `startsWith` / in-directory）と同じ方向に保守的で、過剰無効化は `Disposition::Recomputed{changed:false}` として計測する（REPORT §3）。

### 4.6 世代・公開・解放の契約

1. `begin(batch, host, options, program_options)`：published view の各 entry を batch で照合し、retained / invalidated を `InvalidationReport` に確定。candidate の working view = retained entry とその view stamp のコピー。identity は直前の生存値と等しければ `Rc` を共有（weak intern slot）。
2. request：working にあれば `Reused`（host 呼び出し 0）で、candidate の working stamp だけを更新する。無ければ `ObservationHost` で記録しつつ fresh resolver を走らせ、`Recomputed{violated, changed}`（親に entry があった）または `Fresh{evicted_before}`。
3. `publish(candidate)`：candidate の owner token が同じ cache であること（`Rc::ptr_eq`、違えば `ForeignCandidate`）、parent view が現在の published view と同一 object であること（違えば `ParentMismatch`：別 publish 後や `evict_all` 後の古い candidate）、live 世代数＋1 が `max_live_generations` 以下であることを検査し、`max_entries`/`max_bytes` を超える分を LRU（view stamp、同点は key 順、純関数）で退避してから `published` を差し替える。失敗時は published view・その stamp・退避履歴すべて不変（レビュー R2/R3）。
4. `drop(candidate)`＝cancel：published に何も反映しない（F18/F19）。`CancellationToken` は request 境界で検査する。
5. reader は `GenerationHandle`（`Rc<GenerationView>`）を保持。`CacheEntry` は完全不変（usage stamp は view 側）、entry の owner 数は `Rc` strong count；old reader の view は値も stamp も不変（F15、soak の held-reader 検査、R2 unit test）。handle drop が release。
6. `evict_all` は publish と同じ live 上限を検査して次の published view を空にし（old reader 不変）、dead な世代記録を掃除する。退避済み key は有界の履歴（§4.8）にある間 `Fresh{evicted_before:true}` で観測できる。

### 4.7 config：resolver 層で処理しない

config は cache entry ではなく driver 状態：`parse_config_root_plan` を `ObservationHost::record` で走らせて依存を得る。次世代で `violated_dependency` が真なら再 parse し、option projection（19 宣言＋pathsBasePath）と root membership の差から `options_changed` / `roots_changed` / `recomputed_unchanged` を返す。`options_changed` は新 `OptionsIdentity` を意味し、module/type/lib の旧 entry は key 不一致で再利用されない（明示 invalidation 不要、退避で消える）。`roots_changed` は request set の再計画要求。

### 4.8 上限と常駐 state

統合側のR4追加修正・集計訂正は[統合レビュー](integration/README.md)を参照。

`RetentionLimits { max_entries, max_bytes（推定：key＋option identity＋value＋依存 path 長）, max_live_generations, max_eviction_history }`。既定 4096 / 8 MiB / 8 / 4096。
公開entriesと退避履歴へ `max_bytes` をそれぞれ適用する。共有identityもkeyごとに計上する保守的なpayload推定で、allocator/RSSではない。
退避履歴 `EvictionHistory` は不変値（`Rc` 共有、candidate は clone しない）で、`max_eviction_history` または `max_bytes` を超えた最古の key を忘れて `dropped` に数える。忘れた key の `Fresh{evicted_before}` は false になるため、candidate trace に `eviction_history: {len, dropped, complete}` を出す（レビュー R4）。
`ResolutionCache::resident_stats()` は published entries/bytes、履歴 len/bytes/dropped、live 世代記録数、最新の生存intern identityのbytes（所有keyへ既に計上済み）を報告する。上限変更は新規victimのないpublish / evict_allにも適用し、weak intern slotは単独でidentityを保持しない。soak（REPORT §6）は 8 / 64 KiB / 6 / 16、churn（REPORT §6）は 8 / 64 KiB / 6 / 32 で退避・拒否・履歴上限を実測。

### 4.9 決定と却下案

| 決定 | 理由 | 却下した案 |
| --- | --- | --- |
| (a) containing file の version を key に入れない | 解決結果は containing *directory*・specifier・mode・option・FS 状態の関数で、本文には依存しない（6.0.3 `resolveNamesWithLocalCache` は本文変更後も `resolutionsInFile` を再利用、Go の key にも version はない）。本文が変えるのは request set と mode で、それは driver の再計画で表す | version を key に含める：編集ごとに全 import を再解決し cache の目的を失う |
| (b) request ごとに `ModuleResolver` を生成 | resolver 内 package cache が世代内 2 件目以降の package.json 読取を隠すと依存が欠落する。`ObservationHost` の memo が同一世代の実 I/O を吸収する | 世代単位 resolver＋「触れた package key」hook：resolver 改変が必要。JSON 再 parse の CPU が問題なら INTEGRATION §2 の memo 拡張 |
| (c) containing directory 単位の key、祖先埋めなし | 6.0.3 per-directory cache / Go key と同じ。祖先埋め（non-relative name cache）は hit 率最適化で正しさに関与しない | per-file key：同一 directory の複数 file で重複計算 |
| (d) `Rc`/`RefCell` の single-owner | 依頼の要件。`HostResolvedModule` の `Rc<PackageMetadata>` を `Arc` 化しない | 無条件 `Send+Sync` 化 |
| (e) file 作成は祖先 directory の暗黙作成として扱う | memory host が親 directory を推論するのと同じ。Go `SeenFileOrMissingParentDirectory` と同方向 | file 作成を file のみの event として扱う：負の directory 依存を取りこぼす |
| (f) 型参照 key に origin（automatic/source）を含める | §2.5 | 6.0.3 の per-directory 共有 |
| (g) identity は型付き component の構造的等価（R1） | 文字列連結は `["a,b"]` と `["a","b"]` 等を別名化し、誤った entry を返す（レビューで実証） | `getKeyForCompilerOptions` 風の文字列 key：tsc は `,` を含む値を想定していないが Rust では値の code unit を保つ必要がある |
| (h) usage stamp は view / working view に置き、entry は不変（R2） | 共有 `Rc<CacheEntry>` の `Cell` を candidate が更新すると、drop した candidate が公開側の LRU 順序を変える | entry 内 `Cell` |
| (i) publish は owner token と parent object の `Rc::ptr_eq` で検証（R3） | 世代番号の数値比較は別 cache や `evict_all` 後の candidate を通す | `parent_id` 比較 |
| (j) 退避履歴は有界・不変・共有（R4） | 無限に増える `BTreeSet` と begin ごとの clone は常駐 state を増やす | 無限履歴 / 履歴なし（trace の `evicted_before` を失う） |

## 5. Change manifest（24 family）

ID は `resolution-cache/<request-kind>/<dependency>/<transition>/<variant>`。各 family は initial FS、request set、世代（ops と Rust-only controls、期待 disposition）を持つ。observer は controls を無視して常に fresh に観測する。

| 依頼表の行 | family | 世代の内容 |
| --- | --- | --- |
| positive file：変更なし再利用、内容変更、削除、優先拡張子追加 | `module/positive-file/content-change/reuse`、`module/positive-file/priority-extension/add`、`module/negative-lookup/create-file/minimal`（無関係変更） | 本文変更で reused、削除/再作成で recomputed、`.tsx`→`.ts` 追加で recomputed、探索されない `.js` 追加で reused |
| negative lookup：NotFound→作成、存在しない親 directory、削除→再作成、無関係 file | `module/negative-lookup/create-file/minimal`、`module/negative-lookup/parent-directory/create-then-file` | 最小出発例（依頼の `/p/missing.ts`）、空 directory 作成は recomputed-unchanged（計測） |
| package：exports/imports/types/typesVersions/type、package.json 作成/削除/不正 JSON、node_modules 入替 | `module/package/exports/update-delete-invalid-swap`、`module/package/types-field-and-types-versions/node10`、`module/mode/esnext-vs-commonjs-distinct-keys`、`module/package-scope/type-change` | exports 更新・削除・`{`・node_modules 丸ごと入替、types/typesVersions、import/require 条件別 key、`type` 変更の implied format |
| config/options：extends 先、paths/baseUrl/rootDirs/moduleSuffixes/customConditions、moduleResolution と mode | `config/extends/chain`、`module/config-options/identity/switch-and-return`、`module/config-options/identity/structured-conditions`、`module/config-options/identity/structured-paths` | extends 本文/欠落/package extends、include 一覧、option identity の切替と復帰、identity の要素境界（`,` を含む 1 要素と複数要素、順序、空/未指定、paths substitution/pattern）の三者比較（レビュー R1） |
| types/libs：自動 @types、明示 typeRoots/types、lib replacement、default lib、type package 追加/削除 | `types-libs/automatic-types/default-typeroots`、`types-libs/explicit-typeroots-and-types/custom-root`、`types-libs/lib-replacement/add-remove-package` | 名前列挙の依存（一覧・hidden・typings null）、custom root の primary 権威、`@typescript/lib-dom` の追加/削除。default lib は resolution ではなく catalog 選択（`lib`/`target` は identity に含まれる）なので family なし |
| identity：case、symlink 付替、realpath alias、preserveSymlinks、containing file 移動 | `identity/case-insensitive/folded-keys`、`identity/symlink/retarget-and-preserve`、`identity/containing-file/move` | fold された directory key と原綴り specifier、link 付替/preserveSymlinks/link 除去、directory 単位 key |
| generation：旧 snapshot 保持、複数 project 共有、source membership 再割当 | `generation/hold-old-reader/immutable-view`、`generation/shared-across-projects/same-identity`、`generation/source-membership/rebind` | held reader の不変性、同一 identity の共有と別 identity の分離、世代ごとの `SourceFileId` 再束縛 |
| failure：host error 後の正常化、構築中 cancel、publish 直前 cancel、evict 後の再取得 | `failure/host-error/normalize`、`failure/cancel/during-build-and-before-publish`、`failure/evict/refetch` | 3 種の abort で published 不変、退避後の `Fresh{evicted_before:true}` と parity |

## 6. 観測 schema と parity

- native（observer）：request ごとに `resolveModuleName` / `resolveTypeReferenceDirective` / `resolveModuleName(Node10)`（library）/ `getAutomaticTypeDirectiveNames` / `getImpliedNodeFormatForFile` の公開結果（resolvedFileName、extension、isExternalLibraryImport、resolvedUsingTsExtension、originalPath、packageId、alternateResult、resolutionDiagnostics）と内部証拠（failedLookupLocations、affectingLocations）を分けて記録。roots family は `createProgram`（noLib/noEmit/types:[]）の source order と options 診断、config family は `parseJsonSourceFileConfigFileContent` の fileNames/errors/option projection/extendedSourceFiles。
- primary parity（Rust cached vs fresh）：`CachedValue` の完全一致（診断・package metadata 含む）。PackageScope は package.json path と module type。
- primary parity（Rust vs native）：公開観測の projection 一致。型参照の resolutionDiagnostics は Rust の host result に無い（loader 所有）ため `[]` を明示し、観測された native 値もすべて `[]`。
- 依存の完全性：native の failedLookupLocations ∪ affectingLocations ∪ {resolvedFileName, originalPath} の各 path が、Rust 依存の path、realpath target、または「欠落と記録した祖先 directory」のいずれかで覆われること。
- 決定性：family ごとに 2 回 replay して trace JSON が一致、observer は 2 回採取して byte 一致、soak は 2 回実行して metrics 一致。

## 7. 結果

[REPORT.md](REPORT.md)。

## 8. 段階 C

[INTEGRATION.md](INTEGRATION.md)。Program 再利用・watch・LSP の完成は報告しない。
