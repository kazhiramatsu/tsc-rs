# H2.8c C04 prototype の統合

2026-09-16。Claude の無 commit 候補（base `526c2b37a`）を受領し、
source 42 files・patch・実行記録を退避した。供給候補は `ddf4caf6d` に保存し、
現行 CI との合成および以下の修正を別差分にした。
元 worktree と元の `inputs/expected/known-open.v1.json` は変更していない。

## 範囲

`transpile_module`、`transpile_declaration` と `ProgramNoCheck` の明示的な研究用入口、
そのための lazy checker flags / linked references / single-file API facts を統合する。
CLI/config の noCheck activation、builder の checkPending、custom transformers、
H2.8c 全体の qualification と性能測定は残る。[供給報告](REPORT.md) §1–8 は受領時の記録。
通常 Program の admission を、この prototype の観測結果で緩和しない。

## 再現と修正

供給候補の production bytes で元 287 cases を再実行し、265 exact / 22 known-open を再現した。
265 は正常結果262と TypeScript の出力生成失敗3を含む。
追加14ケースの TypeScript 観測を固定してから candidate を実行すると、12ケースで差が出た。

| 追加比較 | 修正 |
| --- | --- |
| `./`、`..`、backslash を含む jsxDEV の fileName | TypeScript の normalizePath を適用し、準備済み root の ID から API facts を設定 |
| 正規化される名前と moduleName / renamedDependencies / JSDoc の組合せ | raw path の文字列照合で facts を脱落させない |
| moduleName と rename のキー・値に単独 high/low surrogate | SourceFile の API 値を JsString のまま printer まで保持。replacement character への変換を除去 |
| surrogate pair の正常対照 | 同じ出力を維持 |
| rename の変更先が空文字 | TypeScript の truthy 判定に合わせて変更を無視 |

checker の lazy flag 計算で捨てられていた CheckResult は resolver の型付き abort に伝播させる。
新設された source anchor の仮 hash / 不正確な span は採用 TypeScript の実 bytes から再計算した。

## 比較と CI

- `transpile-routes` は controls job の compiler direct batch に登録する。301入力・9 Rust tests を要求。
- observer の `--check` は元287と追加14をそれぞれ2回採取し、固定期待値と byte 比較する。
  Node と TypeScript の pin、replica と public result、repeat state の一致も要求する。
- Native は全301入力の公開観測を2回比較する。transpile / noCheck が full source checking を
  実行していないこと、checked controls が source checking を実行することを別に確認する。
- 元22件の既知差分は candidate 実行から `known-native.v1.json` に固定した。
  別の誤出力・拒否・panic に変わっても known-open の名目で通過しない。
- TypeScript の例外3件は、同じ出力生成失敗の typed error と公開構造が完全一致する場合だけ一致とする。
  Rust-only の拒否や panic を例外の存在だけで加点しない。
- fixture の hash・件数・ID の一意性・完全な集合対応・route 対応を検査する。
  上記の誤通過と0件・欠落・ignored testsを負対照で検証する。
- 既存 compiler UTF-16/literal registry を `COMPILER_DIRECT` に改名して同じ runner を共有する。
  observer の引数、literal専用testのfilter、共有 helper の selection は保持する。

## 残件の読み方

known-open22は inherited emitter14、H2.9 recovery7、範囲外targetのRust拒否1。
この分母にない任意の compilerOptions / UTF-16 source・fileName / custom transformer を
互換実装済みとはしない。研究用 Rust API の公開形は安定 API の約束ではない。
追加14と既存287を、既存 corpus の exact 総数に加算しない。

修正後の最初の replay は9/9 tests成功。元287は265 exact / 22 known-open、
追加14は14/14 exact。TypeScript期待値も両集合で2回一致した。
planner35 testsとpolicy/schemaの選択3 testsも成功。
source anchor とnormalizePathの末尾境界を最終訂正した head の通常経路回帰・
最終route replay、およびhosted head/runは実行中。結果はPRで確定し、受領記録を追記する。
