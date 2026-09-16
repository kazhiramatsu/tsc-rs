# C05 / L2.3 提出物の追加修正依頼 R1

2026-09-16、統合担当 Codex。**現候補は未統合。次の Claude 作業は本レビューの修正を優先する。**
C01 のリテラル監査は、この再提出の後に戻す。

対象は `../tsc-rs-resolution-cache` / `draft/l2-3-resolution-cache` の未 commit 提出物。
base は `d9cfb664ad89898b0b5601d85e7e2abf70e32b18`、提出 patch SHA-256 は
`ff4f60211981fca2211987e124396f7d673250db7d9660669db04f9fa5e3a80e`。
[提出時の patch 原本](records/submitted-candidate.patch.gz)も保持している。
レビューは別 worktree `../tsc-rs-resolution-cache-review` で行い、提出元は変更していない。
patch に含まれる15 source/document file が提出元と byte 一致することを確認した。

## 確認できた既存の成果

提出 source を変更せず独立にビルドし、元の contract 3 tests が成功した。
cached/fresh **139/139**、native **151/151**、lookup 未被覆 **0**を再現した。
1000 世代 soak の全 metrics も提出済み JSON と一致した。
upstream observer の2回採取結果は元の expected fixture と byte 一致した。

一方、[追加の3テスト](repro.rs)は **0 passed / 3 failed、exit 101**。
[実行記録](records/review.v1.json)は candidate の source hash、実行 argv、結果、ログを保持する。
元の比較件数が通ることと、以下の未検証境界の安全性は分けて扱う。

## R1: options identity が衝突して誤った解決結果を返す（P1）

対象：`crates/program/src/resolution_cache.rs:606–617, 651–706, 766`。
`OptionsIdentity` が配列をカンマ結合し、paths 等も未 escape の区切り文字で直列化する。
たとえば `customConditions: ["a,b"]` と `["a", "b"]` は同じ文字列になる。

package exports を `{"a,b":"./joined.d.ts","a":"./split.d.ts"}` とした再現では：

| 条件 | native fresh / Rust fresh | 既存 cache を再利用した Rust |
| --- | --- | --- |
| `["a,b"]` | `joined.d.ts` | `joined.d.ts` |
| `["a","b"]` へ変更 | `split.d.ts` | **`joined.d.ts`** |

[native の最小観測](native-conditions.mjs)も2回一致しており、[結果](records/native-conditions.json)を保存した。
解決先が実際に誤る。配列長・要素境界・None/空・順序・文字列の code units を保つ
構造化 identity に直す。`to_string_lossy()` を identity に使用しない。
customConditions だけでなく moduleSuffixes、types、rootDirs/typeRoots、paths と
その substitutions、config/current-directory 等の全 identity 成分を監査する。
trace 用の短い digest と等価判定用 identity は分離する。

追加対照：区切り文字を含む1要素と複数要素、空/未指定、要素順、UTF-16 の孤立 surrogate
と置換文字。値が変わる対照では fresh/native/cached の三者比較を行う。

## R2: 破棄した candidate が公開済み entry を変更する（P1）

対象：`resolution_cache.rs:1037–1064, 1734–1737`。
再利用 entry は published/held reader と同じ `Rc<CacheEntry>` なのに、lookup が
その `Cell<last_used_generation>` を更新する。

再現：g1 を公開して reader を保持 → g2 で既存 key を解決 → g2 を drop。
published id は g1 のままだが、held entry の `last_used_generation()` は **1 → 2**。
この値は LRU の退避対象選択にも使われる。既存の pointer/value 比較は mutation を検出しない。

candidate の usage 更新を公開前は分離し、drop、request failure、publish refusal が
旧 reader と公開側の退避順序を変更しないようにする。正常 publish 後も旧 snapshot に
見える管理情報の不変条件を明示する。複数 entry の LRU 結果まで含めて対照を追加する。

## R3: 別 cache の candidate を受理する（P2）

対象：`resolution_cache.rs:1509–1539, 1556–1567, 1653–1657`。
publish が比較するのはローカルな `parent_id` の数値だけ。
新規 cache A で作った candidate を、新規 cache B に publish すると **成功**し、
B の世代が0から1へ変わる。A/B は同じ親 snapshot を持たない。

cache/parent の所有 identity を保持して検証する。異なる owner で同じ世代番号の
candidate、同じ owner の古い candidate、evict 後の candidate を拒否し、destination を
不変に保つ。複数 project が同じ cache を明示的に共有する既存対照は維持する。

## R4: 退避履歴が上限の対象外で増え続ける（P1）

対象：`resolution_cache.rs:1455–1461, 1550, 1613–1632, 1636–1649`。
`evicted_keys: BTreeSet<RequestKey>` は退避ごとに insert され、削除・上限適用がない。
各 key は specifier と `Rc<OptionsIdentity>` を保持し、begin は集合全体を clone する。
新しい key/options が増える workload では、published entries が上限内でも常駐 state が
増え続ける。既存 soak の entries/bytes はこの履歴を数えていない。
`evict_all` の連続呼出しでは dead weak generation 記録の掃除も行われない。

この項目は上の3テストとは別に、[private state を測る2テスト](retention-tests.rs)で再現した。
production logic は変更せず、コピーの末尾に `#[cfg(test)]` module を追加して実行し、
その後 source を元の hash に戻した（[記録](records/review.v1.json)、exit 101 / 2 failed）。

- 1000回異なる option identity で公開：published は1 entry / 報告174 bytesだが、
  退避 keyは999個、履歴が保持する option文字列だけで **747,142 bytes**。
  設定は max_entries=1 / max_bytes=4096 / max_live_generations=2。
- readerを保持せず evict_all を1000回：実際の live世代は1、weak記録は **1001個**。

修正時には毎世代新しい key と
option identity を使う churn、および保持 reader 有無での連続 evict_all を実測する。
履歴を bounded にするか常駐 cache から分離し、履歴を忘れた場合の trace の意味も定める。
常駐する履歴・identity・世代管理を含む件数/bytes を報告し、既存の値の parity を維持する。

## 再現と再提出

提出候補を保持した worktree で、レビュー source を一時的な専用 test target として実行する：

```sh
cp docs/design/greenfield/slices/l2-3-resolution-cache-review-01/repro.rs \
  crates/program/tests/resolution_cache_review_contract.rs
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 cargo test --offline \
  --manifest-path crates/program/Cargo.toml --test resolution_cache_review_contract \
  -- --nocapture --test-threads=1
```

本資料の source は反証用であり、main の成功する CI target には登録しない。
修正時に適切な regression target へ採用する。元 fixture の expected を変更して
今回の誤った再利用を許容しない。新しい境界は追加入力で固定する。

同じ C05 branch で R1–R4 を修復し、元の139/151比較・1000世代 soak、追加対照、
必要な隣接 owner を最終 bytes で確認する。新規 source/入力/trace と patch の hash、
before/after、残項目を更新して再提出する。大きな既存全件ローカル replay は追加しない。
hosted entry の登録と commit/PR は統合担当が行う。Program/watch/LSP activation は引き続き含めない。
