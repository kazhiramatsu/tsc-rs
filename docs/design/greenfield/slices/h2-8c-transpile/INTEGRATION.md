# H2.8c C04 prototype の統合

2026-09-16。Claude の無 commit 候補（base `526c2b37a`）を受領し、
source 42 files・patch・実行記録を退避した。供給候補は `ddf4caf6d` に保存し、
現行 CI との合成および以下の修正を別差分にした。
元 worktree と元の `inputs/expected/known-open.v1.json` は変更していない。

## 範囲

`transpile_module`、`transpile_declaration` と `ProgramNoCheck` の明示的な研究用入口、
そのための lazy checker flags / linked references / single-file API facts を統合した。
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

## 最終検証と統合

[PR #535](https://github.com/kazhiramatsu/tsc-rs/pull/535) を merge commit で統合した。
最終 hosted head は `e86ed768f5b3adbba886130205df30d1d72cb44f`。
ローカルで検証した native bytes は `e010bc875` と同一で、後続 commit はCI runtime設定・検証・記録のみ。
[ローカル受領記録](validation/local.v1.json)にsource/binary hash、command、exit、全観測と圧縮ログを保存した。

| 最終集合 | 結果 |
| --- | --- |
| transpile-js 原287の内訳 | 138/150一致（出力生成失敗2含む）、既知差分12 |
| transpile-dts 原287の内訳 | 86/86一致（出力生成失敗1含む） |
| Program noCheck 原287の内訳 | 41/51一致、既知差分10 |
| 追加review | 14/14一致、候補で異なった12を修正 |
| syntax / program / emitter / checker library | 175 / 48 / 506 / 1738 tests成功 |
| declaration bundles | 4 tests成功（visitor/printer、map、forced metadata lifetime、原本JS） |
| route contract | 9 tests成功。原287と追加14をそれぞれ2回比較 |
| planner / selected policy-schema | 36 / 3 tests成功 |
| clippy / fmt / diff | clippy exit 0、変更行警告0。既存警告は残る。fmt/diff clean |

全301ケースは正常結果276、TypeScriptと同じ出力生成失敗3、固定した既知差分22。
新しいRust-only拒否・panicを互換性に加点していない。
[候補の追加ケース](validation/candidate-review.json.gz)と
[修正後の追加ケース](validation/native-review.json.gz)を比較できる。
元のinputs/expected/known-openの3ファイルは受領bytesのまま。

ローカルwallはlibrary 692.377秒、bundle 138.412秒、route 338.16秒、clippy 94.399秒（build込み、background priority・2 workers）。

[Hosted受領記録](validation/hosted.v1.json)と
[compiler direct実行ログ](validation/hosted-compiler-direct.log.gz)も保存した。
[acceptance](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35049768614)の3 job、
[witness](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35049768606)の4 jobと両集約gateが成功。
controlsは945秒（15m45s）。compiler6 target / 15 tests、observer106.626秒、Cargo256.307秒。
追加したtranspile targetはhostedでも9/9成功し、元287の265一致/22既知差分と追加14一致を保持した。

| hosted job | 秒 |
| --- | ---: |
| acceptance (wide) | 1404 |
| acceptance (early) | 738 |
| acceptance (late) | 1062 |
| witnesses (controls) | 945 |
| witnesses (primary) | 516 |
| witnesses (printer) | 136 |
| witnesses (retained) | 526 |

7 replay jobの合計は5327秒。入口追加の実行記録であり、H2.8c全体の性能qualificationではない。

初回hosted controlsはNode22.23.2で起動し、oracleが要求する25.2.1と異なるため採取前に停止した
（[失敗ログ](validation/first-hosted-controls.log.gz)）。`transpile-routes`を含むwitness jobだけ、
固定SHAのsetup-nodeで`.node-version`を設定した。期待値・runtime pin・比較条件を保持して全jobを再実行した。

CLI/configのnoCheck activation、builder checkPending、H2.9 recovery、inherited emitter、
custom transformersおよびH2.8c全体のqualificationは§範囲のとおり残る。
