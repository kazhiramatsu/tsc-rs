# Claude 先行実装依頼⑤：L2.3 — resolution cache の依存追跡と無効化

作成日：2026-09-14。状態：source inventory・設計・隔離 prototype。
L2 全体、watch/LSP、Program 再利用 runtime の activation を許可する資料ではありません。


**2026-09-15 更新**：開始点と検証分担は[共通手順](claude-high-difficulty-handoffs.md)の最新版に従います。
SUPER 統合後の main の SHA を固定し、ローカルは新規失敗・関連 owner の focused set、
重い全件 replay は hosted で実行します。以下の技術要件は現行実装と照合し、既実装部分を再実装しません。

**2026-09-16 再提出・統合候補**：[DESIGN](l2-3-resolution-cache/DESIGN.md)、
[REPORT](l2-3-resolution-cache/REPORT.md)、[INTEGRATION](l2-3-resolution-cache/INTEGRATION.md)を受領。
R1〜R3の独立再現3件は成功。R4の残るサイズ集計・上限変更適用を統合側で追加修正し、
専用CI入口を登録した。[統合レビューと実測](l2-3-resolution-cache/integration/README.md)を参照。
元の[レビューR1](l2-3-resolution-cache-review-01/README.md)と提出patchは履歴として保存する。
Program reuse/watch/LSPのactivationは含まない。

## 依頼

module/type/lib/config/package-json/directory/failed-lookup の依存を明示的に記録し、
変更に応じて cache を無効化する設計と隔離 prototype を作成してください。
TypeScript 7 の snapshot/cache 所有権を参考にし、実際の Rust resolver を使って、
毎回 fresh に解決した結果と cache 再利用した結果が各変更後に一致することを検証します。
単なる memoization や fake resolver のテストで終了せず、変更 trace と実測を提出してください。

[共通手順](claude-high-difficulty-handoffs.md)と
[TypeScript 7 direction](../typescript-7-direction.md)を読み、共通手順の開始 SHA から
`draft/l2-3-resolution-cache` / `../tsc-rs-resolution-cache` を作ってください。
現在の resolver/host/path identity を基準にし、旧 v18 emitter patch は適用しません。
root の loader/session を永続 cache へ切り替える変更は含めません。

## 前提と段階

[L2 設計](../lsp-and-incremental.md#6-l2-program-and-resolution-reuse)は、再利用 cache を
project/service が所有し、`ModuleResolver` の上に置く境界を定めています。
既存 `ModuleResolver::package_cache` は一回の load のためのもので、寿命の延長だけでは
依存の変化を検出できません。`PreparedProgram::resolutions()` は snapshot 固有です。

L2.1 registry、L2.2 Program reuse、L2.4 publication/cancellation の全機能が完成しているとは
仮定しません。本依頼では次の段階を完了し、組込み条件を引き継ぎます。

| 段階 | 成果物 |
| --- | --- |
| A：依存/ownership 設計 | source graph、既存 Rust gap、cache key/value/dependency 型、世代と公開/破棄契約、具体的 change manifest |
| B：isolated prototype | 明示 change batch を入力に取る世代 driver、実 ModuleResolver/host と cache、fresh/reused 比較、release/cancel controls |
| C：後続統合仕様 | registry/Program/watch から供給すべき情報、共有/排他境界、正式 readiness の未完了項目 |

B の driver は test-only host/snapshot を使えますが、解決アルゴリズムは既存 Rust を使います。
root の通常 CLI は fresh route のままです。Go の設計参照と 6.0.3 semantics の採用は別に扱います。

## 参照 source と pin

Rust の accepted semantics は固定 TypeScript 6.0.3。bundle SHA は共通手順にあります。
Go は [native workflow](../typescript-7-workflow.md) の以下の固定 reference を使います。

```text
repository: microsoft/TypeScript
Go reference commit: 1f70213d4922b434345f639b441681e470c7cfc1
local checkout: target/typescript7/upstream
```

2026-09-14 に root の Go checkout HEAD がこの commit と一致することを確認しました。
作業時は `git -C target/typescript7/upstream rev-parse HEAD` と `status --short` を再確認し、
参照 file の hash と差分を保存します。既存の他作業の probe は reset しません。
local checkout がない場合は `python3 scripts/typescript7.py setup` の固定 pin 手順で用意します。
moving main へ更新せず、Go の意図的な semantic 差を 6.0.3 の既存 golden に混ぜません。

| Source | 調査するもの | Rust 側の接続先 |
| --- | --- | --- |
| `_tsc.js:128134` createResolutionCache | successful/failed lookup、invalidate、type-root 等の依存 graph | [module_resolution.rs](../../../../crates/program/src/module_resolution.rs) の `ModuleResolver`、resolve/resolve_with_facts/resolve_type_reference |
| `_tsc.js:40621` createModuleResolutionCache、40633 type-reference cache | option/mode、directory/package cache の key と clear | [resolution.rs](../../../../crates/program/src/resolution.rs) の `ResolutionKey` / `TypeReferenceResolutionKey` |
| `_tsc.js:122454` isProgramUptoDate と Program reuse consumer | root/options/missing/import/lib/package の再利用判定 | [loader.rs](../../../../crates/program/src/loader.rs)、[prepared.rs](../../../../crates/program/src/prepared.rs) |
| Go `tsc/internal/project/snapshot.go`：Snapshot.Clone / processFileChanges / Deref | 世代の変更、保持、破棄 | [checker/program.rs](../../../../crates/checker/src/program.rs) の DocumentRegistry / ProgramSnapshot / ParsedDocument / BoundDocument |
| Go `snapshotfs.go`：SnapshotFS、markDirtyFiles、realpath alias | filesystem snapshot、directory/negative lookup、symlink invalidation | [host/lib.rs](../../../../crates/host/src/lib.rs)、program path/symlink owner |
| Go `extendedconfigcache.go`、`refcountcache.go`、各対応 test | config dependencies、owner/refcount、失敗した produce の扱い | 新規の project/cache owner。低層 program crate へ checker 依存を追加しない |

Go の path はすべて固定 checkout 内の相対 path です。呼出先と predicate の範囲/hash を
個別に固定してください。tsserver wire protocol の移植は不要です。
Rust の source identity と lifetime を保ち、native LSP に接続可能な所有権を設計します。

## 必要な型と不変条件

以下は設計上の要件です。型名・分割は現在の Rust に合わせて決めてください。

- cache request key は containing source identity/version、specifier の原綴り、resolution mode、
  意味に関わる option/config fingerprint、host case/realpath policy を識別する。
  module/type/lib/config の異なる request を同じ key に畳まない。
- value は成功/NotFound/診断/alternate result/package facts を保持する。
  一時的な I/O failure や cancellation を恒久的 NotFound に置換しない。
- dependency は読んだ file だけでなく、探索したが存在しなかった候補、directory membership、
  package boundary、config extends、typeRoots、symlink/realpath の alias を表現する。
- cached host-level result と Program 固有の SourceFileId を分ける。
  再利用した結果は新 Program の source membership に結び直し、旧 generation の ID を流用しない。
- 旧 snapshot の reader は不変な view を持つ。candidate 世代の失敗や cancellation で
  公開済み cache/snapshot を部分更新しない。publish 前後と release 順序を定義する。
- retained state は entry/bytes/owner の上限を持つ。全世代を global map へ残さない。
  現在の Rc/buffer/borrow を無条件に Send+Sync 化せず、single-owner のまま成立する prototype を先に作る。

## 実装手順と変更面

1. module/type/lib/config 各 request の host read/probe を tracing host で採取し、dependency の
   取りこぼしを調べる。fresh result と raw host trace を immutable before として保存する。
2. 明示的な `ChangeBatch` 相当と generation driver を作る。file 作成/更新/削除、directory、
   config/options、package、symlink を変更して、invalidate set を決定的に計算する。
3. 元の ModuleResolver を呼ぶ cache layer を実装する。変更と無関係な entry の再利用と、
   影響する entry の再計算を同じ trace で示す。結果に影響しない依存の過剰無効化も記録する。
4. module → type → lib/config の順に接続する。config の変更が option key/root membership に
   及ぼす影響を resolver 層だけで処理せず、上位 driver の再構築要求として表現する。
5. cancellation/host error/eviction を加え、旧 snapshot の fresh parity を維持する。
   実 watcher/timer は作らず、明示 change trace で順序を固定する。
6. Program/registry へ組み込むときの required capability と不足 owner を C の仕様へまとめる。

主な調査・編集候補は `program/src/module_resolution.rs`、`resolution.rs`、`loader.rs`、
`config.rs`、`library.rs`、`host/src/lib.rs` と、新しい上位 cache owner/test harness。
新 owner の配置は crates の依存方向を確認して A で決めます。
checker `program.rs` は既存 identity の参照先であり、B で registry 全体を改造する必須条件ではありません。
root の cache 永続化、watch/LSP の公開 route、build-info は含めません。

## 必須 change trace

ID は `resolution-cache/<request-kind>/<dependency>/<transition>/<variant>`。
各 transition の前後で fresh resolver と reused resolver を走らせ、結果を比較します。

| Dependency | 必須の変更・対照 |
| --- | --- |
| positive file | 変更なしで再利用、内容変更、削除、より優先する拡張子の file を追加 |
| negative lookup | NotFound → file 作成、存在しない親 directory の作成、削除→再作成、無関係 file 追加 |
| package | exports/imports/types/typesVersions/type、package.json の作成/削除/不正 JSON、node_modules 入替 |
| config/options | extends 先、paths/baseUrl/rootDirs/moduleSuffixes/customConditions、moduleResolution と mode |
| types/libs | 自動 @types、明示 typeRoots/types、lib replacement、default lib、type package の追加/削除 |
| identity | case-sensitive/insensitive、symlink の付替、realpath alias、preserveSymlinks、containing file 移動 |
| generation | 同じ旧 snapshot を保持したまま更新、複数 project の共有、source membership の再割当 |
| failure | host error の後の正常化、candidate 構築中 cancel、publish 直前 cancel、evict 後の再取得 |

最小出発例：`/p/main.ts` の `import "./missing"` を未解決で cache し、
source 本体を変えず `/p/missing.ts` を追加して次世代で解決できることを確認します。
同時に `/p/unrelated.ts` の変更では既存の無関係な解決を再利用する対照を置きます。
これらは入力案であり、計測済みの成功例ではありません。

primary parity は resolved path/extension/mode/package identity/original path/alternate result/
diagnostics と、適用した場合の Program source order・診断です。
各 family の初期状態と変更後の代表状態は、固定 6.0.3 の `resolveModuleName` /
type-reference / Program 観測でも結果を採取します。native fresh/reused が共通の誤りを
持つ場合を検出し、既存 resolver の未対応ケースと新 cache の不具合を分離します。
cache hit で host 操作が省略されるため、fresh と reused の I/O trace 全体が同一とは要求しません。
trace は依存の完全性と再利用範囲の証拠として比較します。
Go 内部の無効化範囲との違いは説明し、version 差と Rust ownership 差を分けます。

## 検証と提出

新規 target 案は `crates/program/tests/resolution_cache_contract.rs`。
上位 owner が別 crate にある場合はその integration target とし、実 resolver を使います。
既存回帰入口の例です（共通の低優先度/env を適用）。
変更した request/host owner に応じて filter を選び、実行件数を確認します。

```sh
cargo test --offline --manifest-path crates/program/Cargo.toml --test contracts module_resolution_contract:: -- --test-threads=1
cargo test --offline --manifest-path crates/host/Cargo.toml --lib -- --list
```

host の一覧から変更 owner の test を選んで実行します。重い全件 regression は hosted へ載せます。
compiler/registry を変更したときだけ、該当する compiler/checker Program tests を追加します。
この依頼は emitter v18 を使わないため、530 件の candidate regression は既定 battery にしません。
Go test は固定 pin の test 名を一覧から選び、正の実行数と実コマンドを保存します。
Go 自体の既存 test 成功を Rust prototype の parity と呼びません。

deterministic trace は 2 回同じ結果になることを確認します。さらに固定 seed で少なくとも
1000 世代の create/update/delete/reuse/release を実行し、古い reader の値、retained entry/bytes、
eviction 後の fresh equality を測定します。メモリ上限と再利用対象は実行前に設計で固定します。

完了条件は A+B+C、選定 dependency 全行の disposition、fresh/reused parity、
影響外の正の再利用証拠、失敗/cancel 後の不変性、bounded retention、再現可能な patch と trace です。
組込み先の L2 前提が未完成ならその required capability を具体的に記録し、Program 再利用や
LSP の完成とは報告しないでください。
