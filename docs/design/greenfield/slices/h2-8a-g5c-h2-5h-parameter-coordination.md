# Claude / Codex の担当境界と統合手順

2026-09-14。ユーザーの「できるだけ被らない部分」という依頼に基づく分担。
既存の [Claude G5c 依頼](h2-8a-jsdoc-return.md) に、
[Codex ES5 引数 temporary 依頼](h2-5h-parameter-temporaries.md) を並行配置する。
資料作成時点の production 許可ファイルの交差は **0**。
別ファイルでも同じコンパイラを変更するため、最終 combined source の回帰確認は必要。

## 1. 担当

| 項目 | Claude | Codex 実装 | Codex 統合（最後に直列） |
| --- | --- | --- | --- |
| 対象 | G5c 原1行、外側 JSDoc return と宣言 d/e | H2.5h ES5 原4行、引数 temporary の pass 境界 | 両成果の取り込みと回帰 |
| worktree | `~/dev/tsc-rs-jsdoc-return` | `~/dev/tsc-rs-parameter-temporaries` | 両 head を固定後、独立した統合 worktree |
| branch | `work/h2-8a-jsdoc-return` | `work/h2-5h-parameter-temporaries` | 実装終了時に作成 |
| 初期 production | `binder/src/node_util.rs`、`checker/src/syntactic_type_node_builder.rs` | `emitter/src/builtins/es2021.rs`、`es2015.rs` | 共有ファイルが必要な場合の順序調整 |
| 追加調査先 | checker の jsdoc/functions/contextual/annotate/node_builder。変更は元 ticket の因果証明条件に従う | context/generated bindings 等は read-only。初期2ファイル外の原因は別 ticket | ticket 外の閉包を別途分類 |
| 新規 test | `compiler/tests/h2_8a_jsdoc_return.rs` | `compiler/tests/h2_5h_parameter_temporaries.rs` | combined source で両方実行 |
| 新規 fixture / observer | `h2-8a-jsdoc-return` 名 | `h2-5h-parameter-temporaries` 名 | 共通比較契約を保持 |
| docs | `h2-8a-jsdoc-return-*` の実装設計/報告 | `h2-5h-parameter-temporaries-*` の実装設計/報告、本境界表 | architecture、schedule、index、レビュー依頼 |
| profiles / ratchets / oracle / xtask / CI | 読む | 読む | 必要と証明した更新だけ。CI を弱めない |
| PR / hosted / merge | 実行しない | 実装中は実行しない | 1つの統合 PR、固定 acceptance、merge commit |

表の crate パスは `crates/` 配下。
Codex は binder/checker 全体を Claude 用に予約し、Claude の ticket を広げる許可とは扱わない。
Claude は emitter 全体を Codex の調査領域として避けるが、Codex の編集許可は上記2ファイルに限定される。
両者とも `compiler/src`、syntax、program、harness、共有 `contracts.rs`/test registry は初期編集対象外。

## 2. 共通 base とファイル検査

runtime は PR #521 の main `3462ef0e0ca10b90eaed2c93c12baacbec2e628d`。
hosted 成功 candidate `7d6bc9848` と同じ tree `627cb7e207fea304f5537953fef5009a18b4c659`。
Claude は既に配布した共通準備 `46b9743b525efd6659ff940dfc24b911f9d91946` から続ける。
Codex は main から分岐し、その共通準備だけを merge した `96037be2c876621d59ebf82cda1967cf8c59ac83` から続ける。
準備履歴中の AppleDouble 100件の純削除は同一 commit を共有し、別々に再実装しない。
開始済みの相手 worktree に checkout/reset/pull をかけない。

Codex の selection JSON と verifier は、初期の concrete allowed paths、Claude の予約領域、
共有禁止領域、before source hashes を検査する。`--scope` は Codex 自身の base 以降の
tracked/untracked diff を検査する。Claude の実 diff を検査したと主張するツールではない。
統合時には Claude の元 ticket と両者の実 diff を照合し、交差0を再確認する。

範囲外の真の owner が判明した場合は、証拠と対象 case を当該 report の `OUT-OF-SCOPE` に残し、
残る独立作業を続ける。相手領域を直接編集しない。必要なら次の ticket で担当と順序を固定する。
shared helper の抽出、テスト共通化、広域 rename は実装中に並行して始めない。

## 3. 同じ Mac の実行枠

worktree、Cargo target、capture directory を分ける。source 読解、資料作成、軽い Node 検査は並行可能。
Cargo/rustc/重い native runner は **この Mac 全体で1本**。
PID の確認だけでは起動競合を防げないため、実行前に現在の担当から明示的に枠を受け取る。
実装用 session 間で連絡できない場合は、ユーザーから渡された実行順序に従い、
枠が不明な側は source/設計/軽い観測を進める。経過時間を枠の解放とみなさない。
先に依頼済みの Claude を最初の実行担当とし、Claude の focused 実行終了を受けて Codex へ渡す。
枠の連絡はローカルな作業連携で行い、Slack/email 等への送信を前提にしない。

| 担当 | Cargo target | captures / logs |
| --- | --- | --- |
| Claude | 自分の worktree の `target/jsdoc-return` | `target/jsdoc-return-runs/` |
| Codex | 自分の worktree の `target/parameter-temporaries` | `target/parameter-temporaries-runs/` |

heavy command は `taskpolicy -b nice -n 15`、`CARGO_BUILD_JOBS=2`、test threads 1、offline。
相手の target や capture を流用しない。今回の資料作成では Codex は native 枠を使用していない。
現行 schedule の軽量運用を継続し、古い full CI / chain-walk 指示を追加実行しない。

## 4. 合流

1. 各担当は最終 commit、dirty/untracked の有無、source pins、実 command/exit と証拠を提出する。
2. 統合担当は実 diff と ownership を確認し、両 branch を merge して履歴を保つ。
   相手の未コミット worktree からファイルをコピーしない。
3. combined source で両方の strict tests と共有 producer の回帰を確認する。
   単独 branch の成功を combined head の成功として流用しない。
4. H2.5h の4行が全一致なら16→12の純削除。G5c 側の記録も実測結果で更新する。
   deferred を根拠なく exact に変えず、全 profile 再 mint は行わない。
5. 共有 architecture/index と review/PR body を更新し、1つの PR で既存 hosted acceptance を実行する。
   失敗は同じ train で原因別に処置し、final head の成功を確認して merge commit で統合する。

Claude に追加で渡す文:

> Codex は H2.5h の ES5 引数 temporary 4ケースを担当し、production は emitter の
> `builtins/es2021.rs` / `es2015.rs` に限定します。G5c の binder/checker と専用 test/fixture/docs は
> Claude の担当のままです。共有登録・profiles・PR/CI は最後に Codex が統合します。
> 同じ Mac の重い native 枠は最初に Claude が使い、focused 実行が終了したら終了を引き継いでください。
