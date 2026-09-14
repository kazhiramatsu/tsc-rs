# H2.5h parameter temporary 実装記録

2026-09-14。状態: **upstream before 完了 / native before 待ち / production 未変更**。
[依頼](h2-5h-parameter-temporaries.md) と [設計](h2-5h-parameter-temporaries-design.md) に従う。
この記録は作業途中であり、修復成功・runtime-ready・統合完了を表さない。

## 1. 現在の source と成果物

- runtime base: `96037be2c876621d59ebf82cda1967cf8c59ac83`。
- 準備資料: `9ec083e9e1471e27b0558f1db65420a14f9dcc16`。
- upstream fixture / 専用 native runner / 設計 draft: `2e1ad4f28`。
- worktree: `/Users/hiramatsu/dev/tsc-rs-parameter-temporaries`。
- branch: `work/h2-5h-parameter-temporaries`。

`crates/emitter/src/builtins/es2021.rs` / `es2015.rs` はまだ開始 source と同一。
別 worktree の Claude の変更には触れていない。
native test は top-level の独立 target として Cargo metadata で登録を確認した。共有 registry の編集はない。

## 2. Upstream before

observer: `scripts/observe-h2-5h-parameter-temporaries.mjs`。
fixture: `crates/compiler/tests/fixtures/h2-5h-parameter-temporaries.json`。

| 検査 | 実結果 |
| --- | --- |
| pinned TS complete command | 68ケース ×2 =136実行、各反復は完全一致 |
| 元の H2.5h/H2.5g 行 | ES5 4行 + ES2015 4行の input/settings と保存済み全 observed fields が不変 |
| ESNext 原対照 | 元 directive の target を使用した4ケース ×2 |
| 追加 controls | P2–P6 の56ケース ×2 |
| 上流 trace | 原12ケース、340 events。instrumented な全 command が無改造 TS の全 tuple と一致 |
| target 条件だけを外す調査 | 原12ケース ×2。ES5 4件だけで JS bytes が変わり、ES2015/ESNext 8件は完全不変 |
| preparation verifier | 27 pins、15 upstream owners、原8行、保持12 manifest 行、一致 |
| scope / syntax / formatting | 許可範囲内。Node syntax、専用 Rust test の rustfmt、diff-check 成功 |

fresh upstream fixture SHA-256:
`8cdf5349118b856c7488e1d35a2901acbc1193526c95102309c8cba7873f7145`。
observer SHA-256:
`36caddcf6869158593cfe06c0cbae63db72eba04bde57ee6df9022e9a2da7581`。
上流 trace SHA-256:
`977afc9879615d7736adc2317a84008bdc78b0110cc12cb22a3bc2b56207acf6`。

実 command は `node scripts/observe-h2-5h-parameter-temporaries.mjs --write`（exit 0）、
`node target/parameter-temporaries-runs/upstream/trace.mjs`（exit 0）。
log、trace script、receipt はこの worktree の `target/parameter-temporaries-runs/upstream/` に保存。
期待値としての正式 fixture は callback の raw path を記録する。
準備中の初回出力は同 directory の `initial-supplement.json` に保存し、最終 fixture と区別する。

上流 trace は、ES5 の ES2020 pass が initializer/pattern を残し、後段 ES2015 が展開することを示した。
ES2015 では ES2020 pass が展開し、ESNext ではどちらの pass も起動しない。
Rust 側の first-divergence は native before/trace で確認するため未確定。

`node target/parameter-temporaries-runs/upstream/counterfactual.mjs` は exit 0。
上流の target 条件のみを外した調査用 compiler の結果を `counterfactual.json` に保存した。
ES5 initializer では multiline が失われ、binding pattern では余分な binding も発生する。
これは Rust before と照合する予測であり、native 修復の証拠には算入しない。

## 3. 未完了の gate と次の実行

1. 先行する Claude の重い native 実行枠の引き継ぎ。
2. 専用 runner の fresh native before（原12 + controls56、全136 captures）。
3. Rust trace、原因分類、設計 gate の未解決0。
4. production 修正、最終 source の after と関連回帰。
5. 原因別 commits と証拠の提出、両担当の合流、必要 manifest 純削除、PR/hosted acceptance。

native command は `target/parameter-temporaries-runs/run-step.py` で real returncode、
UTC 時刻、前後 source hash、stdout/stderr、capture/binary hash を保存する。
各 directory は新規作成とし、既存 captures を上書きしない。
Cargo target は `target/parameter-temporaries`、jobs 2、低優先度、test threads 1。

未実施: native before/after、production 修正、今回の hosted acceptance。
full developer CI / certificate walk / global profile 再 mint は、現行 schedule に従い実行しない。
