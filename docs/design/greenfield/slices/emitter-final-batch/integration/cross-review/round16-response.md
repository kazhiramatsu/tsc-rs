## r15 独立確認: H0 noEmit 分岐の option/semantic ゲート

結論: **実 CLI にも同じ差分があります。** 修正は 1 行の共有ゲートで済み、既存 contract は all-report を期待していません。

**1. 実 CLI の差分は確定**
- `cli.rs:990-998` の H0 分岐は `outcome.options_diagnostics` と `additional_diagnostics`(plan 所有の 5107)を足した後、`semantic_diagnostics` を無条件に足します。
- `lib.rs:1643-1665` の `run()` は program 所有の options/global が空でないときだけ semantic を空にします。plan 所有の行は `run()` から見えません。
- upstream `emitFilesAndReportErrors` (129419-129431) は options+global を足した後 `allDiagnostics.length === configFileParsingDiagnosticsLength` を再判定してから semantic を足すので、config 位置の 5107 は options 診断 (`getOptionsDiagnosticsOfConfigFile`, 124030) として semantic を抑止します。
- noEmit では `handleNoEmitOptions` (125636-125640) が `emitSkippedWithNoDiagnostics` を返すため、emit 結果経由で semantic が復活することもありません。fixture 290/292/294/296 の期待値 `[5107]` はこの経路そのものです。
- 対照: import-helpers case 79 (`module=AMD` 5107 + 2304、emitting + noEmitOnError) は `handleNoEmitOptions` の noEmitOnError ブロック (125641-125663) が semantic を emit 結果に載せるので両方出ます。Rust の `into_reported` は `emit.diagnostics()` を足す (lib.rs:144) ので emitting 側は整合済みです。

**2. 既存 contract の期待**
全 fixture を走査した結果、config `noEmit:true` で tsconfig 位置の行とソース行を同時に期待する case は 0 件です。H0 の getPreEmitDiagnostics 相当 all-report を pin した case はなく、ゲート追加で壊れる既存期待はありません。

**3. 最小共有修正**
`cli.rs:997` を `if additional_diagnostics.is_empty() { semantic を足す }` にする。`run()` が program 所有側を既に処理しているので、この 1 条件で emitting 側 `into_reported` (lib.rs:127-135) と同じ gate になります。global は upstream 通り無条件のまま。

**4. harness wrapper 案の確認**
- `EmitCommandOutcome::new` (lib.rs:169-171) は `into_reported(&[])` 固定なので、新 hidden 関数は外部診断を `options_diagnostics` にマージするのではなく `into_reported(external)` に渡す形が正しい。マージすると `options_are_empty` 判定は同じでも、`run()` が計算済みの semantic を消す責務が `into_reported` の `additional` 条件に依存しているため、渡し方を揃えた方が単純です。
- 宣言診断 gate (lib.rs:1050-1055) に `external.is_empty()` を加える点は必要で、upstream 129433 の同じ length 判定に対応します。
- emitting 側は program 所有なので `[]` のままで正しい。
- test adapter は noEmit loader 時に plan の non-fatal options を保存して渡す。CLI 側と同じ `is_non_fatal_option_diagnostic` フィルタを通すこと。

**5. 注意点**
- 実 CLI 直接比較は 4 case のうち 1 つ(290)で十分です。修正前は `[5107, 2617, 2497]`、修正後は `[5107]` になるはずです。
- `run()` の `IncompleteCheck` (lib.rs:1653) は external options があっても発火します。upstream は semantic を報告しないだけで checker は走るので、挙動差は診断出力ではなくエラー化の有無だけです。今回の 4 case には影響しません。
