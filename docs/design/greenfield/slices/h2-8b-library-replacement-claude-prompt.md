次の独立作業としてH2.8b-LR1のbaselineを実行してください。現在のdecorator作業と共有ファイルを同時編集しないでください。

開始資料：
- docs/design/greenfield/slices/h2-8b-e-design-plan.md
- docs/design/greenfield/slices/h2-8b-library-replacement-baseline.md
- docs/design/greenfield/slices/h2-8b-e-design-registry.v1.json

branch `design/h2-8b-e-slices`から専用worktree/branchを作り、まず
`python3 scripts/check-h2-8-design-plan.py`を実行してください。
production基準は5134bb018、mainへのmergeは9806a4cb9（同一tree）です。
この資料の状態はready-for-baselineであり、production修正はまだ設計していません。

固定済みのlibrary replacement 12件を使い、新規test moduleと登録だけを追加してください。
通常command全タプルとsource/library/root membershipを別testsで比較し、実到達回数・実exit・SHAを
receiptに残してください。上流は12件×2完了、nativeは未実行です。
config-directory-anchorの上流exit2と診断6059/5011は期待された結果なので、そのまま比較してください。

不一致を再現したらbaselineを保存し、source ownerとRust producerを特定してLR2の修正設計へ渡してください。
LR1の途中でproduction、共有comparator、CI/manifest/profileを変更しないでください。
全件一致ならproduction変更なしの結果を記録してください。12件の成功をH2.8b全体の完了としないでください。

重い実行は同じMacで一つずつ、専用target、taskpolicy -b nice -n 15、CARGO_BUILD_JOBS=2。
PR #513はマージ済みです。元のfollow-up branchとdecorator担当のbranchを保ち、この作業は別branchで報告してください。
