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
