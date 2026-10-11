# TypeScript 7 で除かれたもの：計画（2026-10-12）

利用者の決定（2026-10-11）：TS7 が支えないもの（ES5 など）は tsc-rs からも除く。2026-09-29 の「ES5・`outFile`・AMD・
UMD・System を残す」を置き換える。あわせて binder が option を読まないようにし、parse を project の間で共有する（大きな
project で差を出すため）。順は `.types`／`.symbols` の残り（#751）と実プロジェクトの残り（#752）の後、性能の計測の前。

## tsgo `aa8149273` の振る舞い（同じ入力で tsgo と tsc-rs を比べた）

- 除いたと報告して（TS5108・TS5102）、emit は続ける：
  - `target: ES5`：ES2015 として変換する。tsgo に ES2015 の変換（class、arrow、分割代入、for-of、generator など）は無く、
    `GetESTransformer` は ES2016 より前を最大の変換の列にする。`useDefineForClassFields` の既定などは ES5 のまま読む。
  - `module: AMD`／`UMD`／`System`：CommonJS の変換（`getModuleTransformer` の default）。
  - `outFile`：出力には使わず file ごとに出す（読むのは診断と rootDir の layout の検査だけ）。
- tsc-rs が既に tsgo と同じもの：`downlevelIteration`、`moduleResolution: classic`／`node10`、`esModuleInterop: false`、
  `allowSyntheticDefaultImports: false`、`alwaysStrict: false`（診断と振る舞い）、値として受け付けない `target: ES3` と
  `module: None`（TS6046）。
- tsgo の変換・printer・compiler には ES2015 の target の分岐が無い。checker は 2 箇所、binder・parser・scanner は 0。

## tsc-rs の今

- emitter：`es2015.rs`（12,963 行）、`generators.rs`（6,291 行）、`system.rs`（4,577 行）、AMD／UMD の wrapper、`outFile` の
  bundle（declaration の bundle を含む）。ほかに ES2015／ES5 の target の分岐が 37。
- checker 28、binder 2、parser 3、program 1 の ES5／ES2015 の分岐（tsc 6.0 から）。
- suites の既知の違い 24 のうち 14 は ES5 の変換の出力（tsgo の `const` に対し `var` など）。

## 手順

- **R1 emit**：ES5 を ES2015 として変換する（`es2015` と `generators` の変換、emitter と printer の target の分岐を除く）。
  AMD／UMD／System は CommonJS の変換にする（AMD／UMD の wrapper と `system.rs` を除く）。`outFile` は出力に使わない（bundle
  の emit と path を除く）。suites の 14 が一致するはず。conformance は tsgo の runner がこれらの構成を飛ばすので変わらない
  見込み。
- **R2 checker と parser**：tsgo に無い ES5 の言語 version の検査を除く（tsgo と同じ入力で照らす）。
- **R3 binder**：binder が option（`target`、`alwaysStrict`、`noFallthroughCasesInSwitch`）を読まないようにして、parse の
  cache の key を tsgo のもの（`DocumentAddress`）にする。program の file と parse を共有する `createSourceFile`（API の client
  の bounded な 1）もこれで閉じる。
- 各 slice は今までと同じく full の conformance と suites で 0 regressions、checker に触れたら並列対照。

## R1 emit（2026-10-12）

- ES5 は ES2015 として変換する：ES2015 と generators の変換（`es2015.rs`・`generators.rs`）と、ほかの変換の ES2015／ES5 の
  分岐（`const` の代わりの `var`、`"".concat`、arrow の代わりの関数式、`__assign`、class の IIFE、`__awaiter` の promise の
  constructor、decorate した class の内部名、ES5 の tagged template の段階）、それらだけが使った helper（`__extends`・
  `__generator`・`__values`・`__spreadArray`・`__read`・`__assign`）と分割代入の flattener を除いた。`downlevelIteration` は効果
  なし（`__read` の腕も）。
- AMD・UMD・System は CommonJS の変換（tsgo `getModuleTransformer` の default）。AMD／UMD の wrapper、AMD の dynamic import と
  import の初期化、UMD の同期 require、`export =` の `return`、`system.rs` を除いた。module kind は tsgo が読む所
  （`IsEffectiveExternalModule`、helper の module 名と enum の comment の System）では書いたまま読む。module 名は tsgo の
  `tryGetModuleNameFromFile` と同じく付けない（`amd-module` も outFile も）、`renamedDependencies` は残る。
- `outFile` は出力に使わない（TS5102 と rootDir の layout の検査だけ）：file ごとに出し、JSON は outDir にだけ、node builder の
  bundled mode・sharded emit の outFile の gate・loader の TS6131 と TS1148（tsgo はどちらも出さない）を除いた。
- **test**：`--target es5` の CLI contract（tsgo の TS5108 と ES2015 の出力）、AMD／UMD／System（`export =`、import-equals）と
  AMD + outFile（source ごとの CommonJS）の emit-session contract を tsgo の byte に。除いた変換・wrapper・flattener・outFile の
  bundle の計画の test と fixture を除き、literal-update の transform の fixture から ES5 の 133 行を除いた。tsgo との横並び
  （`--target es3/es5`、`--module amd/system/umd/none`、AMD + `--outFile`、`--moduleResolution classic`、`--esModuleInterop false`、
  `--alwaysStrict false`、`--downlevelIteration`、ES5 の例を es5〜es2018 で）は全て同じ。
- **結果**（release build、head `41feca255`）：full の conformance（`--workers 2 --check`、472 s）は 0 regressions で、#752 の
  report と全ての field で同じ（tsgo の harness はこれらの構成を飛ばす）。並列対照は記録済みの順序の 4 構成だけ。suites
  （`--check`）は 0 regressions、ES5 を残していた 14 が全て一致して `--update` で記録（既知の違いは 24 → 10：content mapper 8、
  generateTrace の型 id 2）。約 10 万行を除いた。
- **hosted と merge**：PR #753 の hosted run 38162182191（rust 11m9s、conformance 13m30s、gates pass、head `41feca255`）、merge
  commit `e092970b3`。次は R1b（到達しなくなった bundle の仕組みを除く。振る舞いは変わらない）。

## R1b 到達しなくなった bundle の仕組み（2026-10-12）

- R1 で `outFile` が出力に使われなくなり到達しなくなった bundle の仕組みを除いた（振る舞いは変わらない）：transform と emit の
  bundle の root（`TransformRoot::Bundle`・`EmitRoot::Bundle` とその型）、printer の `bundle.rs` と `PrintRequest::Bundle`・
  `SourceFileEmitMode`・bundle の helper、declaration の `bundle.rs` と `is_bundled_emit`、parse 済みの emit metadata の
  snapshot（`factory/parsed_metadata.rs` と `TransformError::ParsedEmitMetadata*`）、bundle の declaration map と生成名の
  引き継ぎ、外部 module 名の解決（`get_resolved_external_module_name`・`external_module_name_from_path`）。約 2 万行を除いた。
- **test**：bundle の test と fixture を除いた（printer-failure の hook 25 → 23、review 22 → 11、decorator-binding-direct
  146 → 141）。
- **結果**（release build、head `2de700bc3`）：full の conformance（`--workers 2 --check`、480 s）は 0 regressions で R1 の
  report と全ての field で同じ。suites（`--check`）は 0 regressions、既知の違い 10。emitter・compiler・cli・api・conformance の
  test と clippy（`-D warnings`）は通る。
- **hosted と merge**：PR #754 の hosted run 38164098280（rust 11m9s、conformance 20m29s、gates pass、head `2de700bc3`）、merge
  commit `8a0f1950e`。次は R2（checker と binder の ES5 の検査）。
