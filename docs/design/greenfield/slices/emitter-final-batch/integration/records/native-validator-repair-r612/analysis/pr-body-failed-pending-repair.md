通常のコンパイルで残っていた helper 名衝突、module resolution、checker 診断、構文回復と、追加監査で見つかった生成名・async/ES5・class/decorator・コメント・source map の差分を修復する。提出時の KNOWN 68 行は、元の入力と完全観測を保持したまま TypeScript との一致を確認し、すべて退役した。退役済み差分の再発と KNOWN の退役漏れはテストで拒否する。

コメントは出力段階ごとの所有者と位置を修復し、JavaScript・宣言・両 source map・診断・write callback を比較する。構文回復された `import` に余分な括弧を付ける factory の左辺判定も、既存の式種別判定へ合わせた。共有処理の変更は Claude Code と独立に調査し、直接観測と照合した。CST/AST の分離と関係表への移行は今後の設計課題として記録している。

既存の検証入口を保持し、EF7 universe、PLAN-BASE の分割実行、class/global、追加の完全コマンド比較を hosted witness に登録する。固定 census と歴史的観測は元の bytes を保持する。生成チェーンの全段を更新した後に、列挙した5 fixtureの古い親参照を元の observer で同期する。許可した参照情報以外の内容変化や生成失敗を拒否し、6ファイルを復元して停止する。検証器の拒否境界、履歴の失敗記録、未検証範囲も保持している。

最終候補は `fa391cbff6468032ebd4969734cde9d2b9474237`。検証状況は次のとおり。

- 最終 Rust bytes の H2.5g 全体検証：9,027件を分類、native 対象8,511件が各2回 exact。既存 deferred の H2.8a 6件・H2.9 510件は、この native exact 件数に含めない。
- factory 13ケース、source map・コメントを含む heritage 140件、H2.7d/e の314件と既存11参照、関連27グループ、System の23 native tests・6,035 memberships 各2回が成功。
- 正式生成チェーン r596：終了0、13,522.593秒。3巡目は全75段・追加更新0。全225個の巡回ログと、5 fixtureの全内容が参照情報以外不変である証拠を保存。最終の16 owner-control scripts、固定参照再検証、全 pin surfaces、66検証器 tests、27 contracts、FCI 45 envelopes / 42 ready も成功。
- テスト登録台帳 v39：元の検査コマンドが成功。77 targets、53 unfiltered / 19 filtered / 5 indirect の区分は不変。これは静的な登録確認であり、全テスト実行の証明ではない。
- hosted foundations の成功ログ混入を修正し、厳密な名前・件数判定を保ったまま全17対象・49 native tests、88 planner tests が成功。過去と現在の9,027ケースの順序・分類・分割一致を検証して shadow membership plan の参照1個を更新し、Node 94 tests が成功。実際の Opus 233・234との照合、元の失敗ログ、旧候補CIの停止記録を保存した。Rust と現行実行入力は不変。
- **同じ候補の hosted は26 checks すべて成功**。元の universe は217 exact /0 known /0 failed、PLAN-BASEは全4 shards合計1,798 exact /0 known /0 failed。foundationsは17対象・50 Linux tests。System・構文回復の追加対照も成功。
- **この候補の unsplit local CI r603 は失敗（終了1、21,586.876秒、qualified=false）**。5 phases完了後、workspace-testsでcompilerの古いES5拒否期待値1件と、conformanceのRust宣言アンカー不一致2件が失敗した。アンカー3種類の不一致はtrusted baselineにも存在する。現在は原因と履歴制約を調査しており、未マージ。trusted baseline は `3b1f5fe87fd31e3b303bb44bd257342735452ed9`。修正後の最終候補で全18 phasesとhosted checksを完了し、conformance・FP・escapes・testsの実績を追記してからマージする。

検証範囲は有限の入力集合に限る。別集合の transpile には Unicode 識別子の複合構文回復と ScriptTarget.JSON の2つの既知境界が残る。元 census の load failure 110件は未検証で、別途比較した108 projectsは代替ではない。build/watch、public API の re-emit、TypeScript 7 はこの統合の完了範囲に含めない。

[統合報告と検証記録](https://github.com/kazhiramatsu/tsc-rs/blob/fa391cbff6468032ebd4969734cde9d2b9474237/docs/design/greenfield/slices/emitter-final-batch/integration/README.md)・[今回の収束証明](https://github.com/kazhiramatsu/tsc-rs/blob/fa391cbff6468032ebd4969734cde9d2b9474237/docs/design/greenfield/slices/emitter-final-batch/integration/records/canonical-walk-r597/certificate.json)。元の producer worktree とユーザー workspace の未 commit 作業は保持している。
