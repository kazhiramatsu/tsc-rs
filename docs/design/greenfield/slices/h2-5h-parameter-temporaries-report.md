# H2.5h parameter temporary 実装記録

2026-09-14。状態: **原4件 exact ×2、全68件中63 exact / 5件の既存 printer 差分、関連回帰完了、PR #522 で main 統合済み**。
[依頼](h2-5h-parameter-temporaries.md) と [設計](h2-5h-parameter-temporaries-design.md) に従う。
原4件の修復を実測した。共通 producer 全体は C3 を残す部分閉包。
[統合記録](h2-8a-g5c-parameter-integration.md) に hosted success と main landing を記録する。

## 1. 現在の source と成果物

- runtime base: `96037be2c876621d59ebf82cda1967cf8c59ac83`。
- 準備資料: `9ec083e9e1471e27b0558f1db65420a14f9dcc16`。
- upstream fixture / 専用 native runner / 設計 draft: `2e1ad4f28`。
- worktree: `/Users/hiramatsu/dev/tsc-rs-parameter-temporaries`。
- branch: `work/h2-5h-parameter-temporaries`。

production 変更は `crates/emitter/src/builtins/es2021.rs` のみ。`es2015.rs` は開始 source と同一。
別 worktree の Claude の変更には触れていない。記録用 instrumentation は両ファイルから除去済み。
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

## 3. Native before

Claude の focused battery と原因別 commits の引き継ぎを final report で確認した。
head `886e60771d104b698dc560018450339b40ecec55` は clean、production 4ファイルは
すべて binder/checker 内で、Codex の許可2ファイルとの交差0。
`target/parameter-temporaries-runs/claude-handoff-review.json` に実 diff と final steps を記録した。
先行 native job が終了したことを確認してから Codex の実行を開始した。

| run directory（`target/parameter-temporaries-runs/` 配下） | exit / 秒 | 実結果 |
| --- | --- | --- |
| `before-20260914-1740` | 101 / 534.23 | compile 成功。64ケース128 captures。新規 `noEmit` control を emit 専用 loader に渡した test adapter の不備で focused group が停止 |
| `before-complete-20260914-1656` | 101 / 83.62 | adapter 修正後、全68ケース136 captures。37 exact、31差分。すべて反復不変、実行中の source 不変 |

directory の suffix は識別子であり、正確な実行時刻は各 `receipt.json` の UTC 値を使う。
両 command は `cargo test --offline -p tsc-rs-compiler --test h2_5h_parameter_temporaries -- --nocapture --test-threads=1`。
compile / command / test の exit を区別し、expectation は一切更新していない。

adapter 修正は、新規 `noEmit` 2件だけを `load_program` に渡し、fixture の全 effective options と
同じ input/library files を MemoryCompilerHost に設定するもの。原12件と他54 controls は従来の
qualified loader のまま。最初の128 captures は修正後と全 field が一致した。
noEmit 2件と noEmitOnError/ES5 は exact、noEmitOnError/ES2015 は既存 comment 差分を観測する。

原4件の actual は、上流の target 条件だけを外す counterfactual の全 tuple と完全一致。
原 ES2015/ESNext 8件は無改造 TS と完全一致。追加 controls の差分には ES5 の先行 lowering と、
ES2015 にも存在する parameter initializer の comment/source-map metadata の差がある。
後者は `lower_parameter_default` と pinned `addDefaultValueAssignmentForInitializer` の
clone/range/flags を Rust trace で分離する。before summary の SHA-256 は
`419bab1f6da5563653c8180daa5412a92244dbbb6d0881c3c649c751fb906aa5`。

## 4. 原因別の変更と after

設計 gate は `41437525c`、Rust trace は `before-trace-20260914-1700`
（exit 101 / 155.46秒）。全136 captures が uninstrumented before と全 field 一致した。
trace 除去後に元27 source pins と再一致し、その後に production を編集した。

- **C1 (`e9d0fa100`)**: effective target を shared visitor に渡し、parameter lowering と
  そのための binding alias 予約を ES2015 以上に限定する。原4件を含む26ケースが exact になり、
  元37 positives は exact を維持した。後段 `es2015.rs`、context、name finalizer は変更しない。
- **C2 (`9df833266`)**: initializer worker の name を typed original を保持する clone に変更し、
  assignment/block の parameter range と、name/initializer/assignment/block の emit flags を上流に合わせる。
  使用されなくなった `identifier_text` helper を削除。重複 `/* after */` と assignment map の差を修復した。
  body 先頭コメントの欠落は下記 C3 として分離し、5件を exact と数えない。

| run directory | exit / 秒 | 完全比較の結果 |
| --- | --- | --- |
| `after-c1-20260914-1707` | 101 / 167.84 | 63 exact、5件は before と全 actual 不変。新たな失敗0 |
| `after-c2-20260914-1711` | 101 / 107.49 | 63 exact、5件は重複コメント等が改善した部分差分。unused helper warning 1 |
| `final-parameter-20260914-1718` | 101 / 108.25 | unused helper 削除後、全136 captures は C2 と完全一致。compile warning 0 |

いずれも同じ68ケース×2、完全 tuple と反復一致を検査した。最終の
`original_parameter_commands` は12ケース×2で pass、`focused_parameter_commands` は
51 exact / 5 strict failures。TS 期待値も比較も変更せず、skip/ignore/xfail を追加していない。
全 runner を緑とは報告しない。初期37 positives の非退行と26 repaired cases は維持されている。

### C3 — OUT-OF-SCOPE: synthetic prologue 後の compact body comment

残る ID は `parameter-temporaries/{comments-lf,comments-crlf,source-map,bom,no-emit-on-error}/es2015`。
無改造 TS と最終 Rust の JS 差分は、同一行の `} /* body */ return x;` が `} return x;` となること。
source-map case はこの出力と map positions / callback URL position にも差分を持つ。
write path/BOM/順序、診断、result presence、status、exit は完全一致する。

元 source の ReturnStatement と Block の range は保たれ、合流後の statement 順も上流と同じ
`var → if → original return`。upstream `mergeLexicalEnvironment`（24889–24932）は
元 array range を保持し、`updateBlock`（23055–23057）も元 block range を保持する。
Rust の既存 merge と factory update はこの配置に一致する。

差は `printer.rs` の compact function-body loop（7267付近）が list-owned intervening
comment phase を `index == 0` でだけ呼ぶこと。先頭が synthetic var であるため元 return 前の
same-line comment を拾えない。TS `createPrinter/emitNodeListItems`（120068–120155）は
120119–120124 の `shouldEmitInterveningComments` 分岐で各 child の comment range に対し
`emitTrailingCommentsOfPosition` を呼ぶ。これは `lower_parameter_default` の flags を修復しても残る。
多行になる ES5 counterpart と removeComments control は exact。

上流 helper pins は `target/parameter-temporaries-runs/upstream/printer-followup-owners.json`。
`emitNodeListItems` の AST body SHA-256 は
`f7da864479c04d23c6e375b40a09c19a7776552e5d60d6582bb24d6aa1529fc3`。
次の ticket は printer の list/comment phase を owner とし、synthetic/source sibling、既存 trailing-comment
所有との重複防止、LF/CRLF/BOM/removeComments/maps をこの完全 fixture で追う。
現 ticket の production 許可2ファイル外のため printer は変更しない。

## 5. 最終 source の関連回帰

すべて `final-regress-20260914-1722-<label>` 配下。各 run の source snapshot が
`final-parameter-20260914-1718` と全 file hash 一致し、実行中も不変であることを確認した。

| label | command の対象 | exit / 秒 | 結果 |
| --- | --- | --- | --- |
| emitter-library | `cargo test --offline -p tsc-rs-emitter --lib` | 0 / 144.61 | 505 passed |
| emitter-contracts | 同 crate `--test contracts`、active_transform / token_cursor / comment_scope_witness / factory_transform filters | 0 / 129.58 | 381 passed、71 filtered |
| static-this-super | compiler `--test contracts -- h2_5h_static_this_super` | 0 / 185.52 | 3 passed、原行・decorated accessor・完全 witnesses |
| utf16-original | compiler `--test h2_5h_utf16_original_rows_complete` | 0 / 10.99 | 原4行の全 command ×2 |
| map-projection | compiler `--test h2_6a_map_option_projection` | 0 / 128.07 | 3 passed、既存の census/probe 2件は元から ignored |
| fmt | `cargo fmt --all -- --check` | 0 / 18.49 | final source |

test threads はすべて1。実 argv / environment / stdout / stderr / binary hashes は各 receipt に保存。
scope verifier は11 changed paths が許可範囲内、production の実変更は `es2021.rs` 1件。
docs の相対参照と diff-check も成功。共有 source、profiles、ratchets、registry、CI は変更していない。

## 6. Source / evidence pins と統合への引き継ぎ

runtime head `9df833266`（C1 `e9d0fa100` + C2）。この後の報告 commit は docs のみ。

| file / observation | SHA-256 |
| --- | --- |
| `crates/emitter/src/builtins/es2021.rs` | `1f64db0d38f95eba7330e6a46f3d77c656a144110013e9eb95a3904af11cb323` |
| unchanged `es2015.rs` | `a3dd5caf99be448c4aaa315bdd65f9e3df79130caea513a5250ce9fc344cb8be` |
| dedicated native runner | `2382d5c34dd6bf641a407e7799d791bd5a9455bd18f543b4adda5bcc250e2287` |
| final parameter binary | `6d3e1989a6f802da4f7eb0f36474a2372a3bd81f61d5b417f29014524c1f935a` |
| final source snapshot | `3e377cf268d3ac9ff83cf285cf778f3ef904a38d1740519c079a989e3baf5cf7` |
| final parameter receipt | `6e16bd29f4a1c3494cf0d925599716bd69ee573c0e0558af48352af4596d40c8` |
| `final-evidence-index.json` | `945646e0f5074d721314ff848abdacea16c189e4627012e778501c1e36a5c607` |

fixture / observer pins は §2 のまま。index は `target/parameter-temporaries-runs/` 内にあり、
before / trace / C1 / C2 / final / 各回帰の receipt と binary hashes をまとめる。
統合担当は Claude head `886e60771` と本 branch を履歴を保って合流し、combined source の
focused 比較、元4行だけの manifest 純削除、共有 docs、PR/hosted acceptance を行う。
5件の C3 strict failures は別 owner のまま明示し、緑や global admission として扱わない。

native command は `target/parameter-temporaries-runs/run-step.py` で real returncode、
UTC 時刻、前後 source hash、stdout/stderr、capture/binary hash を保存する。
各 directory は新規作成とし、既存 captures を上書きしない。
Cargo target は `target/parameter-temporaries`、jobs 2、低優先度、test threads 1。

統合と今回の hosted acceptance は PR #522 で完了。C3 は本 ticket 外の次 owner。
full developer CI / certificate walk / global profile 再 mint は、現行 schedule に従い実行しない。
