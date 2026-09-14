# G5c JSDoc return / ES5 parameter temporary integration

2026-09-14。状態: **hosted acceptance 成功、両担当を main へ統合済み（C3 5件は別 owner の既知失敗）**。
G5c と ES5 parameter の独立した補修を1つの train へ合流する。
H2.8a 全体の close、global profile の再 mint、新規 admission は主張しない。

## 1. 入力と所有

- trusted main: `3462ef0e0ca10b90eaed2c93c12baacbec2e628d`（PR #521）。
- Claude: `886e60771d104b698dc560018450339b40ecec55`。
  [設計](h2-8a-jsdoc-return-design.md) / [報告](h2-8a-jsdoc-return-report.md)。
- Codex: `754a2afe3411965d374b92b5f3a629d3f24c59df`、runtime final `9df833266`。
  [設計](h2-5h-parameter-temporaries-design.md) / [報告](h2-5h-parameter-temporaries-report.md)。
- 両 branch を clean な状態で受領し、merge commits `33ae8f1ab`、`ea72e7c19` で履歴ごと取り込んだ。
  production は binder 1ファイル、checker 3ファイル、emitter 1ファイル。交差0、merge conflict 0。
- 共通準備 `46b9743b5` の AppleDouble `._*` 100件の純削除は同一履歴を1回取り込んだ。
  compiler source / fixtures / profiles の内容を置き換える削除ではない。

独立した統合 worktree は、前 train が終了して clean だった
`/Users/hiramatsu/dev/tsc-rs-utf16-integration` を再利用する。
branch は `work/h2-8a-g5c-parameter-integration`、専用 Cargo target は既存の
`target/utf16-integration`。他担当の worktree/target を書き換えず、Cargo に通常の依存再検証を行わせる。
同じ Mac の重い native 実行は1本、jobs 2、低優先度、test threads 1 を維持する。

Claude の `slices/README.md` への G5c 行追加は、統合担当がレビューして受け取った。
共有 runtime/policy ファイルへの lane 側変更はない。共有 architecture/index/manifest はここで直列更新する。

## 2. 実装と境界

G5c は owned JSDoc return lookup（binder → syntactic builder）、JSDoc assertion を保持する
return aggregation、診断の signature display の annotation/shortcut 順序を修復した。
単独 source では元 G5c と58 controls が exact ×2、comment-range 全6 tests、binder73、
syntactic-builder13、関連checker238、全checker library1738、隣接compiler3 suitesが成功。
Clippy `-D warnings` は untouched program crate の145件の既存 lint で停止する。
詳細と同一失敗の切り分けは Claude report §4.2 を参照する。

Parameter は effective target を shared visitor に渡し、ES5 の parameter lowering と alias 予約を
後続 ES2015 pass に委ねる。native parameter initializer の clone/range/flags も上流へ合わせる。
単独 source では原12ケース exact ×2、追加 controls は51 exact / 5 strict failures。
元の37 positives を維持し、原ES5 4行を含む26ケースを修復した。
emitter505、関連contracts381、static-this/super、UTF-16 原4行、map controls、fmt が成功。

**既存 printer 残差 C3**: ES2015 の comments-lf / comments-crlf / source-map / bom /
no-emit-on-error で、synthetic prologue 後の compact body 先頭 `/* body */` を落とす。
`printer.rs` の list-owned intervening comment phase が次 owner。両 lane の許可範囲外なので変更しない。
この5件の TS 期待値・完全比較を保持し、ignore/xfail を追加しない。
`focused_parameter_commands` の exit 101 を成功とは扱わない。
原4件の exact と、共通 producer の部分閉包を区別する。

## 3. Manifest の純削除

`ratchets/h2-5h-known-divergences.v1.json` から、selection が固定した ES5 4ケースだけを削除する。
16 → 12、diff は0追加/32削除。保持12行は preparation selection の全 object と一致し、
TS qualification とその全 tuple、残る owner/facet、profiles、oracle、xtask の実 source と CI は維持する。
AppleDouble 純削除には `crates/xtask/src/._*` 3件も含むため、xtask 配下の全 path が
不変とは扱わない。削除4ケースは単独・combined source の両方で exact ×2 を実測した。

## 4. Combined verification

実 command / environment / UTC / source snapshots / stdout / stderr / binary hashes は
`target/g5c-parameter-integration-runs/` の各 directory に保存する。
単独 source の成功を combined source の成功として流用しない。

両比較は head `ea72e7c19` + §3 の manifest 純削除で実行した。
全 source/fixture/observer/manifest snapshot の SHA-256 は
`64048bb83981f19e019b8397e42a0c032a6439eb6c43b9b66a525f51b3729ecf`。
各 command の前後で snapshot が同一、両 lane の production/fixture/runner は受領 head と同一。

| directory | 実結果 | 秒 | receipt SHA-256 |
| --- | --- | --- | --- |
| `combined-parameters` | exit 101。原12 exact ×2、追加51 exact / 5 C3 strict failures。全136 capture が Codex final と同一 | 271.35 | `76508e11440b61c60cf8af880ac2cafc11d27bea424800f18a6ee4c2032f7951` |
| `combined-g5c` | exit 0。58 controls / 116 captures 全 exact、元 G5c、comment-range runner 全15 tests（本体6 + 同居回帰9）が成功 | 200.05 | `ac4986f13536a517ab5711e8425993821db8033ec0d00bf808986b3e70a17faa` |
| `combined-checker` | exit 0。checker library 1738 passed | 220.21 | `38b739e30c461d8548a572787a26efbc2190232b6df904792c7fb0dd0edfa57f` |
| `combined-fmt` | exit 0。workspace fmt check | 14.31 | `e6ea2a22740aa4111d9cfac64546927185244beabbc25f17c78e8ba13daca100` |

重い command は直列に実行した。manifest の純削除 commit は `6c90b97cd`。
`combined-comparison.json` は全136 parameter captures の component 同一性と G5c 116 captures の exact を記録する
（SHA-256 `0832c70f374d85976adc991f213e0ded0ecfce31d77ca519af42eb57e3258a99`）。
emitter の library/contracts と隣接3 runners は component report の final-source 結果を使い、
combined で再実行した結果とは区別する。統合で変更された checker と両者を通る完全 command を再検証する。

再現 command（全件・filter 未指定）:

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/target/utf16-integration" taskpolicy -b nice -n 15 cargo test --offline -p tsc-rs-compiler --test h2_5h_parameter_temporaries -- --nocapture --test-threads=1
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/target/utf16-integration" taskpolicy -b nice -n 15 cargo test --offline -p tsc-rs-compiler --test h2_8a_jsdoc_return --test h2_8a_declaration_comment_ranges -- --nocapture --test-threads=1
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/target/utf16-integration" taskpolicy -b nice -n 15 cargo test --offline -p tsc-rs-checker --lib -- --test-threads=1
cargo fmt --all -- --check
```

実行時は `run-step.py` で command を包み、新規 run directory へ実 exit と証拠を保存した。
parameter は `TSC_RS_H2_5H_PARAMETER_CAPTURE_DIR`、G5c は `TSC_RS_JSDOC_RETURN_CAPTURE_DIR`
をそれぞれ新規 absolute directory に設定した。正確な environment と binary hashes は各 receipt にある。

## 5. PR / hosted / landing

開始時点の open PR は0件。既に合流済みの #516/#520/#521 を重複して開かない。
combined candidate を1つの PR とし、既存の unsplit `cargo xtask acceptance` を hosted で実行する。
full developer CI / certificate walk / global profile 再 mint は現行 schedule に従い実行せず、成功とも記録しない。
hosted の final candidate 成功と mergeable を確認して、merge commit で統合する。

2026-09-14 landing: [PR #522](https://github.com/kazhiramatsu/tsc-rs/pull/522) を merge commit
`1738a661a829c56100cd6f2c962268e6ecc5b6bc` で main に統合した（`2026-09-14T09:40:14Z`）。
[hosted acceptance run 34824935291](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34824935291) は
candidate `02988d64b9c236fe75aff73efad9ab616c8da211` に対して success。
job は46分47秒。実ログの diagnostic conformance は7691 cases、49024/49024 matched、
FP=0 / FN=0 / mismatches=0。H2.5h は932 candidates =876 exact /12 known /44 deferred、
repetitions=2。今回削除した4行を含め、既存の hosted boundary を通過した。
Claude `886e60771` と Codex `754a2afe3` の最終 commits は両方 merge の ancestry に含まれる。
C3 の5件は引き続き別 owner の strict failures で、全68件の閉包や H2.8a close は主張しない。
この landing 記録は Markdown のみで、検証済み runtime/fixture/manifest の bytes は変更しない。
