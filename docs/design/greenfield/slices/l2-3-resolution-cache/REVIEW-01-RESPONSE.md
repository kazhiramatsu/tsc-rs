# レビュー R1（`l2-3-resolution-cache-review-01`）への対応

**提出時点の記録**：以下の数値はClaude再提出時。統合レビューでR1のpath表記保持とR4の追加2件を修正し、identityを含むbytes集計とCI入口を追加した。現在の結果は[統合記録](integration/README.md)を参照。

作成日：2026-09-16。対象：[追加修正依頼 R1](../l2-3-resolution-cache-review-01/README.md)
（統合担当 Codex、提出 patch `ff4f6021…e3a80e` に対するもの）。
同じ branch `draft/l2-3-resolution-cache`（base `d9cfb664a`）で R1–R4 を修復し、再提出する。
本書は修正内容・根拠・再検証の記録であり、Program/watch/LSP activation は引き続き含めない。

## 0. 概要

| 指摘 | 種別 | 原因 | 修正 | 回帰テスト |
| --- | --- | --- | --- | --- |
| R1 options identity の衝突 | P1・誤った解決結果 | `OptionsIdentity` が配列を `,` 結合・`to_string_lossy` で文字列化 | 型付き構造 `IdentityValue` による構造的等価（§1） | unit `options_identity_is_structural_not_serialized`、`distinct_condition_arrays_never_reuse_a_resolution`；contract `review_r1_*`；manifest family `identity/structured-conditions`、`identity/structured-paths`（native 三者比較） |
| R2 破棄 candidate が公開 entry を変更 | P1 | `CacheEntry.last_used_generation: Cell` を lookup が更新 | usage stamp を view（および candidate の working view）へ移動、`CacheEntry` は完全不変（§2） | unit `dropped_candidate_leaves_published_stamps_and_lru_untouched`（LRU 結果まで）；contract `review_r2_*` |
| R3 別 cache の candidate を受理 | P2 | `publish` が `parent_id` の数値だけ比較 | owner token の `Rc::ptr_eq` と parent view の `Rc::ptr_eq`（§3） | unit `foreign_and_stale_candidates_are_rejected`；contract `review_r3_*` |
| R4 退避履歴が無限に増える | P1 | `evicted_keys: BTreeSet` に insert のみ、begin で全 clone、`evict_all` が dead weak を掃除しない | 有界の不変 `EvictionHistory`（`Rc` 共有、FIFO、`max_eviction_history`）、`ResidentStats`、`evict_all` の live 上限と掃除（§4） | unit `eviction_history_is_bounded_and_reported`；contract `review_r4_key_and_identity_churn_keeps_resident_state_bounded`（2000 世代 churn） |

レビューの [repro.rs](../l2-3-resolution-cache-review-01/repro.rs) を修正後の候補で一時 target として実行：
R1・R3 は原文のまま **pass**、R2 は削除した `CacheEntry::last_used_generation()` の代わりに
`GenerationHandle::view().last_used(key)` で同じ不変条件を assert して **pass**（3 passed / 0 failed、
`validation/review-repro.log.gz`）。一時 target は実行後に削除し、CI target には登録していない。

## 1. R1：構造化 identity

`crates/program/src/resolution_cache.rs`：

```text
pub enum IdentityValue { Undefined, Bool, Integer, NumberBits, Text(JsString),
                         List(Vec<IdentityValue>), Entries(Vec<(JsString, IdentityValue)>), HostError(String) }
pub struct OptionsIdentity { components: Vec<(&'static str, IdentityValue)> }   // Eq/Hash/Ord は derive（構造的）
```

- 19 個の `affectsModuleResolution` 宣言＋`pathsBasePath`、Rust 側 resolver 入力（`module`、`target`、
  `noDtsResolution`、`allowArbitraryExtensions`、`allowImportingTsExtensions`、`libReplacement`、
  `preserveSymlinks`、`types`、`configFilePath`）、host profile（case sensitivity、current directory）を
  それぞれ型付き component として保持する。配列は `List`（長さ・要素境界を保持）、`paths` は
  `Entries(pattern → List(substitutions))`（宣言順）、文字列は `JsString`（UTF-16 code unit を保持、
  孤立 surrogate と U+FFFD は別値）、数値 option は canonical bits、未指定は `Undefined`（空 list と別）。
- 等価判定に文字列化は使わない。`describe()`（trace 用の可読文字列）と `digest()`（tag byte と長さ
  prefix 付きの canonical encoding の FNV-1a）は trace 専用で、等価判定に関与しない。
- `to_string_lossy()` は identity から除去。host の current directory 取得失敗は `HostError(message)`。

対照（unit test）：`customConditions` の `["a,b"]` / `["a","b"]` / `["b","a"]` / `[]` / 未指定の 5 通りが
互いに不一致で digest も不一致、同一入力は一致；孤立 surrogate `\uD800` と `�` は不一致；
`moduleSuffixes` `[".a,b"]` と `[".a",".b"]`；`paths` の `["src,alt/*"]` と `["src/*","alt/*"]` および
pattern 分割；`types` の `["a,b"]` と `["a","b"]`。

値が変わる対照は manifest family として native 三者比較に載せた：

| family | 世代 | native / Rust fresh / Rust cached |
| --- | --- | --- |
| `module/config-options/identity/structured-conditions` | g0 `["a,b"]`＋`moduleSuffixes [".a,b",""]` | `joined.d.ts`、`util.a,b.ts` |
| | g1 `["a","b"]`＋`[".a",".b",""]` | `split.d.ts`、`util.a.ts`（fresh、再利用なし） |
| | g2 `["b","a"]` | `split.d.ts`（fresh：identity は順序も区別） |
| | g3 `[]` / g4 未指定 | NotFound（fresh：空と未指定は別 identity） |
| | g5 `["a","b"]` / g6 `["a,b"]` | reused（それぞれ g1 / g0 の entry） |
| `module/config-options/identity/structured-paths` | g0 `["src,alt/*"]` → g1 `["src/*","alt/*"]` → g2 戻す → g3 pattern 追加 | `src,alt/x.ts`+NotFound → `src/x.ts`+`alt/y.ts`（fresh）→ reused → fresh |

両 family とも fresh/native 一致 14/14・8/8、期待 disposition 一致。

## 2. R2：usage stamp を view へ

- `CacheEntry` から `last_used_generation: Cell<u64>` を削除。entry は構築後に一切変化しない。
- `GenerationView.entries: BTreeMap<RequestKey, ViewEntry { entry: Rc<CacheEntry>, last_used: u64 }>`。
  公開 API：`GenerationView::last_used(&key)`、`usage_stamps()`。candidate は working view に自分の
  stamp を持ち（`Candidate::working_last_used`）、lookup は candidate の working view だけを更新する。
- publish は working の stamp をそのまま新 view に写す。drop / request failure / publish 拒否は
  公開 view・held reader の stamp と退避順序を変えない。正常 publish 後も旧 view の stamp は不変
  （view は不変値）。
- LRU（`select_victims`）は純関数で、view の stamp と key 順で決定する。

対照：unit test で g1 公開（`./other`・`./util` とも stamp 1）→ reader 保持 → g2 が `./other` を
参照して drop → held view の `usage_stamps()` 不変、publish 拒否（live 上限）でも不変 →
`max_entries 1` で無 request の g3 を publish → 退避されるのは stamp 同点の key 順で `./other`
（旧実装の漏れなら `./util`）。

## 3. R3：owner と parent object の検証

- `ResolutionCache` は `Rc<OwnerToken>` を持ち、candidate はその clone を持つ。`publish` は
  `Rc::ptr_eq(owner)` を検査し、不一致は `CacheError::ForeignCandidate`（destination 不変）。
- parent は `Rc<GenerationView>` を保持し、`Rc::ptr_eq(&candidate.parent, &self.published)` を検査する。
  別 publish 後の古い candidate、`evict_all` 後の candidate は `ParentMismatch`（destination 不変）。
  同じ owner で同じ世代番号でも object が違えば拒否される。
- 同一 cache を複数 project が共有する既存対照（`generation/shared-across-projects/same-identity`）は不変。

## 4. R4：有界の退避履歴と常駐 state の報告

- `EvictionHistory { order: VecDeque, keys: BTreeSet, dropped: u64 }` は不変値で `Rc` 共有。退避時に
  「旧履歴＋victims」を新しく作り、`RetentionLimits::max_eviction_history`（既定 4096）を超える最古の
  key を忘れ `dropped` に数える。`begin` は `Rc` を clone するだけで集合を複製しない。
- `Disposition::Fresh { evicted_before }` の意味：保持中の履歴に key があれば true。履歴を忘れた key は
  false になるため、candidate trace に `eviction_history: { len, dropped, complete }` を出す
  （`complete == (dropped == 0)` の間は false が「未退避」を意味する）。
- `ResolutionCache::resident_stats()` → `ResidentStats { published_entries/bytes, eviction_history_len/bytes/dropped, live_generation_records, interned_identity_bytes }`。
- `evict_all(&mut self) -> Result<(), CacheError>`：publish と同じ live 上限を検査し、dead weak 記録を掃除する。
- identity は `interned_identity` で直前の候補と等しければ `Rc` を共有する（世代ごとの重複割当を抑える）。

実測（`target/l2-3/churn.json`、`validation/churn.json.gz`）：毎世代新しい specifier ×3 と新しい
`customConditions` identity、`max_entries 8` / `max_eviction_history 32` / `max_live_generations 6`、
reader 保持あり（最大 4）と連続 `evict_all`（97 世代ごとに 2 回）で 2000 世代：

| 指標 | 値 |
| --- | --- |
| 退避履歴 | 最大 32 entry / 1472 bytes（上限で頭打ち）、忘れた key 5960 |
| published | 最大 8 entry / 2632 bytes |
| live 世代記録 | 最大 6（上限 6）、`evict_all` 42 回中 11 回を live 上限で拒否 |
| interned identity | 763 bytes（1 個） |

既存 soak（1000 世代、`max_eviction_history 16`）は entries/bytes/parity の全値が再提出前と同一で、
resident は履歴 4 entry / 148 bytes、dropped 0、live 5、identity 746 bytes。

## 5. 数値の before / after

| 項目 | 提出前（patch `ff4f6021`） | 再提出 |
| --- | --- | --- |
| manifest | 22 family / 94 世代 / 158 request | 24 / 105 / 180（expected sha256 `1733c25a…3fe8234`、2 回採取一致） |
| cached == fresh | 139 / 139 | 161 / 161 |
| Rust == native | 151 / 151 | 173 / 173（request 161 ＋ 名前 5 ＋ config 7） |
| native lookup 位置の未被覆 | 0 | 0 |
| trace 行 | reused 47 / rec+ 39 / rec= 3 / fresh 50 | reused 53 / rec+ 39 / rec= 3 / fresh 66 |
| contract tests | 3 | 7（＋review R1–R3 回帰、R4 churn） |
| module unit tests | 3 | 8 |
| レビュー repro.rs | 0 passed / 3 failed | 3 passed（R2 は view 経由） |
| 1000 世代 soak | 717 / 216 / 67、parity 9408 | 同一値＋resident 報告 |

## 6. 公開 API の変更（研究用 prototype、安定 API の約束ではない）

削除：`CacheEntry::last_used_generation()`、`OptionsIdentity::as_str()`。
追加：`IdentityValue`、`OptionsIdentity::{components, component, describe}`、`GenerationView::{last_used, usage_stamps}`、
`Candidate::working_last_used`、`RetentionLimits::max_eviction_history`、`ResidentStats`、
`ResolutionCache::resident_stats`、`CacheError::ForeignCandidate`。変更：`ResolutionCache::evict_all` は `Result`。

## 7. 残項目

- 履歴の bound を超えた key は `evicted_before: false` になる（trace 意味は §4 のとおり明示）。
- 元 fixture の expected は変更していない（新 family の追加のみ）。
- hosted entry の登録、commit/PR は統合担当。Program/watch/LSP activation は含めない。
