# G5c 開始点の統合結果と補助ファイルの整理

2026-09-14 15:21 JST、[PR #521](https://github.com/kazhiramatsu/tsc-rs/pull/521) は
merge commit `3462ef0e0ca10b90eaed2c93c12baacbec2e628d` で main に統合済み。
[hosted acceptance 34810033022](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34810033022)
は candidate `7d6bc9848e97c26b0438da5ec73bd08c9a175c9a` で全段階成功した（46分41秒）。
main と candidate の tree はともに `627cb7e207fea304f5537953fef5009a18b4c659`。
selection JSON の `in_progress` は15:12時点の保存済み snapshot であり、最終結果は本記録が補う。
開始 source を reset する必要はない。

統合時のアーカイブ展開で、レビュー freeze に存在しない AppleDouble `._*` 100ファイルを
Codex が誤ってコミットした。すべて AppleDouble magic を持つ補助ファイルで、
review freeze 外の追加ファイルはこの100件だけであることを確認した。
本 G5c branch に純削除をコミットする。Rust/TypeScript の source、原 input、oracle、
fixture、比較、profile、CI 設定の内容は変えない。

リポジトリの小修正を次の train にまとめる運用に従い、この削除は G5c の次回統合に同梱する。
今回の main にはまだ削除を反映していない。Claude はこの整理を再実装する必要はなく、
この branch の履歴を保ったまま G5c の調査・実装へ進む。
`node scripts/check-h2-8a-jsdoc-return-selection.mjs` は削除後も成功し、固定した
15 source/artifact pins、12 upstream owners、元ケースの before captures は不変である。
