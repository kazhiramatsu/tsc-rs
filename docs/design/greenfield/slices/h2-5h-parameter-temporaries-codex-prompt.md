# Codex への依頼: ES5 引数 temporary の pass 間引き渡し

`/Users/hiramatsu/dev/tsc-rs-parameter-temporaries` の
`work/h2-5h-parameter-temporaries` で作業する。
開始 base は `96037be2c876621d59ebf82cda1967cf8c59ac83`。
main `3462ef0e0` と共通準備 `46b9743b5` を取り込み済みで、Claude の今後の実装には依存しない。
着手中の worktree は reset しない。

[依頼資料](h2-5h-parameter-temporaries.md)、[固定 inventory](h2-5h-parameter-temporaries-selection.v1.json)、
[担当境界表](h2-8a-g5c-h2-5h-parameter-coordination.md) を読む。
`node scripts/check-h2-5h-parameter-preparation.mjs` で開始入力を確認する。

対象は H2.5h の optional/nullish の ParameterInitializer / ParameterBindingPattern、ES5 4行。
最初に元4行と ES2015 対照4行の fresh native before ×2 を取り、完全差分と実 exit を保存する。
準備資料は source inventory であり、native baseline や runtime-ready 設計はまだない。
第一仮説は `es2021.rs` の共通 parameter lowering が上流の target >= ES2015 条件を持たない点。
alias 予約、generated identity、後続 ES2015 prologue まで trace して設計 gate を完成させる。

production の初期許可は `crates/emitter/src/builtins/es2021.rs` と `es2015.rs` だけ。
Claude の binder/checker と G5c 用 test/fixture/observer/docs は触らない。
独立した observer/fixture/top-level test を作り、原 tuple と新 controls の全観測 ×2 を検証する。
元 ES5 の TS5107/exit 2 も一致させ、期待値・比較・refusal の削減で通さない。
範囲外 owner は証拠付きで分離し、可能な範囲を進める。

重い native 実行はこの Mac で1本、専用 target、低優先度、jobs 2、test thread 1。
Claude と実行枠を明示的に引き渡す。full CI / walk を追加しない。
最終 scope check、原因別 commit、source/evidence、command/exit、未解決を report に残して push する。
共有 manifest/architecture、PR と hosted acceptance は両担当の成果が揃った後の統合段階で扱う。
