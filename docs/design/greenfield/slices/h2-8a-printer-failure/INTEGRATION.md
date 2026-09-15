# C03 / A-INT3：printer failure 候補の統合レビュー

2026-09-15。統合担当：Codex。基準 main：`938dec454904398fd94d9939c173750ac9ba8d05`。
Claude の提出は [DESIGN](DESIGN.md) と `records/` に原形で保存する。
提出 worktree `../tsc-rs-printer-failure` / `draft/h2-8a-printer-failure` は変更していない。
提出 base `f9ef828a5` から統合 base までの変更は PLAN-BASE の文書・棚卸しのみ。

**状態：統合候補のローカル検証完了。hosted の結果と統合 SHA は PR に記録する。**
printer 全 API の failure 互換性が完了したという記録ではない。
生成名と source をまたぐ container の2 case は後続 owner を明示して残す。

## 1. 採用する変更とレビューで直した点

提出 patch SHA-256：`65a315d99f61b7f776383c49940cdd7a86b4f1c9610b8a5f8a0815c2206dfb19`。
提出 source の `git diff` と完全一致し、基準 main に適用できることを確認した。
P1〜P7（祖先 after の未発火、owned writer の保持、comment scope の持越し、comment 抑止、
root comment 相、statement 前の改行、SourceFile 通知順序）を基礎に、次を修正した。

### コメント抑止が実際の worker 全体に届くようにする

提出 P4 は failure 後に `comments_disabled_after_failure` を設定するが、worker 中は
不変 `EmitContext` だけで NoNestedComments を伝えていた。context を受け取らない
token/list の comment writer がコメントを書いてしまう。

入力 `/*a*/ f(/*arg*/ x); //t` で、通常出力にも `/*arg*/` が残り、x の before hook が
失敗した後も、次回出力の prefix が `/*a*/ f(/*arg*/ f(x);` となった。
固定 upstream は `/*a*/ f(f(x);`。提出時の list comment 継承失敗2件と同じ owner である。

`emitCommentsBeforeNode` に対応する worker 入口で抑止を設定し、正常終了時だけ解除する。
これで既存 `list_comment_flags_contract` **48 row exact ×2**、追加の failure 対照も一致。
既存 fixture の期待値は変更していない。

### bundle helper の記録も printer の寿命で保持する

上流の `bundledHelpers` は createPrinter の Map（`_tsc.js:116929`）であり、
`reset()` でも消えない。`emitHelpers`（117719〜117755）は既出の unscoped helper を抑止する。
提出候補は bundle 呼出しごとに集合を作るため、成功後・失敗後とも次回に同じ helper を
重複出力した。`Printer.bundled_helpers` に移し、失敗前の helper 出力も保持する。
bundle → file → bundle の対照では、file は自身の helper を出し、後続 bundle は再出力しない。

### 既知差分の拡大と別種のエラーを検知する

提出 contract は mismatch の **key 集合だけ**を比較していた。同じ op の出力が壊れても
key が増えなければ通るため、提出の Rust capture から3 op の native tuple を
`printer-failure-known-native.json` に固定した。別の出力・位置・status に変われば失敗する。
その検証自体に、同じ key の text を変えて拒否される対照を追加した。

hook replay の Err も、注入した `TransformError::Unsupported(CustomTransformers)` と
armed fault を要求する。一般の型付きエラーを「上流も threw」として受け入れない。

## 2. 追加対照と再現

提出 observer / 25 hooks / 14 probes は bytes を維持した。
`scripts/observe-printer-failure-review.mjs` は提出 observer の SHA を検証して同じ hook adapter を
使い、op ごとの entry 選択、helper 入力、UTF-16 配列の観測を追加する。cleanup は追加しない。
新規 `printer-failure-review.json` は native 実行前に upstream を2回採取して固定した。

| 追加集合 | row | 最終結果 |
| --- | ---: | --- |
| Node / File / Bundle の異なる entry の組合せ × before / after | 12 | exact ×2 |
| 同じ printer の成功 → 失敗 → 失敗 → 成功 | 1 | exact ×2 |
| NoNestedComments の失敗後に File / Bundle へ | 2 | exact ×2 |
| 引数コメントを持つ NoNestedComments の失敗・再利用 | 1 | 修復後 exact ×2 |
| helper の成功後 / 失敗後、bundle → file → bundle | 2 | 修復後 exact ×2 |
| 孤立 high / low surrogate、失敗前後で surrogate pair が接合 | 3 | exact ×2。code units と UTF-8 sink bytes を両方比較 |
| clone 後の独立した再利用 | Rust-only 1 test | 値を複製し、片方の成功・clear が他方を変えない。上流に clone API はないため加点しない |

追加 **21/21**、提出 **23/25**：合計 **44 exact / 2 deferred case**。
14 upstream-only probes と Rust-only battery、検証用の negative control はこの分母に入れない。

最初の探索では bundle-only roots に printFile を要求し、Rust の root validation に拒否された。
これはテスト側の不正な呼出しである。最終 adapter は Bundle と SourceFile の両 root を
明示的に登録し、無関係な typed error を拒否する。探索時の件数を production failure に加算しない。
旧16 row 版から最終21 row 版へ増やす際、最初の16 row の upstream 観測は維持した
（追加の UTF-16 配列と対応する明示的 UTF-8 text 表現を除く）。

```sh
node scripts/observe-printer-failures.mjs --check
node scripts/observe-printer-failure-review.mjs --check
python3 scripts/witness.py printer --list
# 46小規模 direct row と同じ target の安全性・検証対照だけ。Program 全件 replay ではない
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 python3 scripts/witness.py printer --all
```

`records/integration-review.v1.json` に source/input hashes、実行結果と capture の参照を保存する。
ローカルは7 target / 30 test と noEmitOnError の owner control 1 test。
全 emitter lib、全 contracts、530 / 672 complete command、全 acceptance は再実行していない。
fmt と CI planner の選択・失敗伝播テストは成功。
Clippy `-D warnings` は既存 program 145件で停止する。通常の指定 target の Clippy は exit 0、
program 145 + emitter 16 の既存警告。printer/bundle と新規 target への診断は0。
これは workspace lint green の主張ではない。

## 3. Compiler・writer/map の到達境界

| 観測 | 現行 Rust との対応・検証 |
| --- | --- |
| printer の外部 writer / map generator が途中で throw | Rust の TextWriter / SourceMapRecording は不可謬。caller writer/generator を渡す同じ public entry はない。14 probes の該当行は source evidence のまま |
| compiler custom transformer が2 file目で throw | custom transformer は未採用。`execute.rs` は通常 script の全 artifact を構築してから sink を呼ぶ。source probe の partial write と同一の経路とは扱わない |
| noEmitOnError | `emit_files_with_activity` の diagnostic gate が transform/print 前に return。既存 `output_plan_contract::duplicate_output_preflight_reaches_no_sink_and_obeys_no_emit_on_error` を再検証・hosted に登録。probe の型エラーと同じ入力を replay したとは主張しない |
| JavaScript/declaration map の sink fault | 既存 `h2_7d_bundle_sinks::ordinary_bundle_sink_commands_match_complete_typescript_twice` の10 command に map throw / onError と正常対照がある。hosted controls に新規登録。source map recording 内部の fault とは区別 |
| 同じ printer で複数 unit を出す harness | `print_script_units_with_recording_for_harness` は callback が Some を返せば毎回 fresh recording writer、None なら owned writer。全呼出しが必ず recording 付きという仮定はしない。成功時 clear、Err は `?` で unit ループを終了する |
| currentSourceFile の未指定・stale source | Rust の request は source ID を持つ node/source/bundle を要求し、省略 API はない。source ID/transform lifecycle の失敗は Rust-only boundary |
| identity / JSON entry | PreserveUnchanged と JSON worker は独立した既存経路。own writer 保持は適用するが、本観測の Canonical hook 順序から全経路の互換性を推定しない。API1.1a の到達・error 契約に残す |

## 4. 残差の disposition と担当

| 残差 | disposition / owner | 完了に必要なこと |
| --- | --- | --- |
| `recover-other-source-same-positions` の op2/op3 | **互換差を保留、exact 非加点**。H2.8a-A-INT3-CS（統合担当） | 上流は UTF-16 の整数比較。Rust の CommentCursor は source ID + byte offset。単に source 比較を外すだけでは非 BMP / 別 arena の意味が合わない。永続 state の表現と claim/leading/trailing の読者を一緒に設計し、同位置/異位置/別 transformation を観測する |
| `unique-name/after/statement-1/recover-new-unique` の op2 | **互換差を保留、exact 非加点**。C02 / A-INT2 | printer の lazy 生成・failure 後の名前表寿命を transformation 所有の eager 確定と照合。`x_2` / `x_1` の現差と対照を C02 に渡す |
| print 中の substitute factory、array/token hooks、NodeList、外部 fallible writer/generator | API1.1a の未到達面 | API 所有権・lifecycle の設計と公開時の新規 contract。C03 のために全 writer を Result 化しない |

既知差分の維持は upstream と同じになったという判断ではない。3 op の native tuple を
固定して退行を検知し、C03 全面完了・readiness/admission の加点は行わない。
Claude の隔離候補提出は受領済みなので、同じ③全体を再依頼せず次は④へ進める。

## 5. Hosted の入口と時間予算

- `witnesses (printer)`：上流 observer 2本、7つの emitter target、noEmitOnError の exact test。
  **20分上限 / 2 workers**。Program/oracle chain は実行しない。
- `witnesses (controls)`：従来の SUPER controls に bundle sink の **10 command ×2** を追加。
  compiler build を共有し、新しい compiler 専用 job は作らない。
- printer fixture/target/observer だけなら printer job のみ、bundle-sinks fixture/target だけなら
  controls の bundle-sinks のみ。共通 printer source の変更は全関連 acceptance/witness を維持。
- selected target の0 test、失敗、取消、選択された job の skip は gate を通さない。
- 今回の実測時間・run URL・head と merge 状態は PR に保存する。過去 #524 の時間を新 job の実測には使わない。
