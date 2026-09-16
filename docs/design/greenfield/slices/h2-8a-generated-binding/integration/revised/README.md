# C02 修正版の再レビューと統合

2026-09-17。提出元 `~/dev/tsc-rs-generated-binding`（base `ccb6661c1`）の source は変更していない。
統合は `~/dev/tsc-rs-binding-integration` / `work/generated-binding-integration`。
main `ecf2401b7` を取り込み、提出 patch の runner 以外を適用した。runner は現行 registry に
三者マージしてから新 suite の依存・実行条件を確認した。実装 commit は `d8d37bd0b`。PR 全件 hosted 検証は未完了。

## 受領・再レビュー

提出 patch の SHA-256 は `3583cb82d0ad934ee035e07e76e361316cb488213ffb934c33f03afca2b5aeb1`。
未コミットの `git diff --binary -- crates scripts` と byte 一致、49 upstream span の hash 一致を確認。
[receipt](received.v2.json)。元の review probe は提出修正版に対し 7 / 7 exact ×2
（[probe receipt](original-probe-receipt.json)）。F1 / F2 の修復を確認した。

scope の追加 controls、dispose の `InvalidLifecycle` assert、native known の両 pass 検査と
exact 時の retire 要求、Program ごとの実行回数表記を確認した。提出 docs に残った
141 row の表記は 146 に直した。提出 after7 の結果は REPORT.md にそのまま残す。

## F3：衝突で消費した temp ordinal

再レビューで追加発見した failure-carry の誤り。parsed `_a` がある source で最初の temp は `_b`。
その standalone declaration の after hook を失敗させ、別 binding を印字すると、提出候補は
`const _b = 1;const _b = 1;`、上流は `const _b = 1;const _c = 1;`。
独立 probe の native / upstream を各2回実行し再現した
（[native](temp-collision-before-native.json)、[upstream](temp-collision-before-typescript.json)、
[probe source](collision.rs)、[observer source](collision.mjs)）。

原因は `temp_count += 1` が binding 数しか数えないこと。`makeTempVariableName`
（`_tsc.js:120703-120740`）は衝突候補と `_i` / `_n` の slot も消費する。
DESIGN 決定18として、通常 temp の final allocation の counter を binding 単位で記録し、
`EmitMetadata::generated_binding_temp_ordinal` 経由で printer の open scope に渡す。
同一 binding の再印字では再加算しない。planned spelling / loop-variable の既存 policy は維持する。

専用 observer `scripts/observe-decorator-binding-carry.mjs` からtemp ordinalの8 rowを凍結した。
parsed collision、`_i` / `_n` skip、数字列移行 × before/after fault。
元の shared observer / pipeline fixture は byte 不変で、再採取していない。

## F4：関数本体入口でまだ印字していない宣言

関数本体に2宣言を置き、最初の宣言のafter hookで失敗する2 rowを追加した。
上流は `emitBlockFunctionBody` の `generateNames(body)`（`_tsc.js:119021-119032`）で両方の
名前を生成済みだが、候補は最初の宣言しか持ち越さなかった。次のtempは上流 `_c` に対して `_b`、
scoped は上流 `_s_5` に対して `_s_4`（[before](scope-entry-before.log.gz)）。
[追加source span](additional-source-spans.json) のhashを固定した。DESIGN 決定19として、source file入口の宣言収集をfunction-body Block入口にも使い、
未印字の宣言のcounter・名前・cacheも現在scopeへ記録する。補足controlsは計10 row。

## 現行 registry・hosted 登録

- direct は printer job に登録。提出146入力 + 補足10入力、計204 route比較。
- pipeline は独立60分 job。pinned Node を使い、pipeline observer `--check` → native replay。
- shared observer 変更は direct / pipeline の両方、専用 fixture は所有 suite を選択。
  shared witness libraries は pipeline も選ぶ。共有 production の変更は全体 coverage を保持。
- capture / report / dump / selection 環境を除去。runner は `exact + known == selected` に加え、
  requested complete ID 数と selected known 数を照合。`--all` は758 exact +9 known /767 complete。
  upstream exception のみの選択、0 test、欠落 summary、部分実行、observer / Cargo failure を拒否。
- planner/runner 59 tests、qualification policy の execution hashes、入口台帳v18を更新。

## 統合 source のローカル検証

最終実装 commit `d8d37bd0b` の結果。`*-final.log.gz` と `local-checks-final.json` に記録する。

- direct runner: observer146 + 補足10一致、202 /204 exact、残り2は typed dispose KNOWN。
- SUPER direct: 28 exact +4 recorded divergences。literal-update: 3 tests。printer failure: 10 tests。
- generated_bindings 19、target_bindings 5、printer 11 unit tests。
- planner/runner: 59 tests。qualification check-policy、fmt、production/tool の diff whitespace check 成功。
- pipeline focused: 28 exact +9 known /37、failed0、両pass一致。全件は PR head の hosted job が担う。

元の `records/after7` の全件758 exact /9 knownを、統合後の全件実測としては加算しない。
R9 / R12 / T1 の9 native divergence、dispose の2 typed divergenceは既存 owner の未解決項目として保持。
main への merge と runtime admission は、この記録では宣言しない。
