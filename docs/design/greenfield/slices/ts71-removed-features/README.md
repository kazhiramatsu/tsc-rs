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

## R2 checker と binder の ES5 の検査（2026-10-12）

- tsgo の checker に残る ES2015 未満の検査は空の配列の binding pattern の型（checker.go:18378）の 1 つだけで、binder には無い。
  tsc 6.0 から持っていた残りを除いた：ES5 の helper の要求（`__extends`・`__generator`・`__values`・`__read`・`__spreadArray`・
  `__assign`・`__makeTemplateObject`）、ES5 の診断（arrow／async の `arguments`、ES2015 未満の private 名と `accessor`、object
  literal と method でない `super`、async の戻り値の型の ES5 の道と promise の constructor の検査）。iteration は global の
  `Iterable` だけを見る（`downlevelIteration` は何も選ばない）。名前の解決は arrow に `arguments` を与えず、parameter の scope に
  target の条件を持たない。static の `super` の走査と `Reflect` の衝突の検査は ES2021 以下のすべての target で行う。
- 除いた ES2015 の変換だけが読んでいた記録も除いた：入れ子の block-scoped binding の走査と loop の捕捉の check flag（tsgo に
  無い）、捕捉した binding の link、lexical な `this` の捕捉、空だった `this`／`new.target` の衝突の列とその検査、R1 から呼ばれて
  いなかった emit resolver の 4 つの method。
- binder：strict mode の flag は常に真だった（tsgo は strict mode を追わない）ので prologue の走査とともに除いた。関数宣言は常に
  block scoped、TS1250 は出さない（tsgo の message の helper は呼ばれない）、label の検査に target の条件は無い。binder は言語
  version を持たない。lib の bundle の cache の key から `alwaysStrict` を外した。
- **test**：TS1250 を出さない binder の test、construct signature の戻り値の行を省く relation の鎖（既定の target で tsgo の
  2322/2322/2326/2322）、`alwaysStrict` が bundle を分けないことを tsgo に合わせた。tsgo との横並び（`arguments`、private 名、
  `super`、async の戻り値の型、label、script の strict mode、global の `Iterable` の無い iteration を es5〜esnext で 20 構成）は
  全て同じ。
- **結果**（release build、head `f4c3dcc27`）：full の conformance（`--workers 2 --check`、472 s）は 0 regressions で R1b の
  report と全ての field で同じ。並列対照は記録済みの順序の 4 構成だけ。suites（`--check`）は 0 regressions、既知の違い 10。binder・
  checker・emitter の test 2,214 と clippy（`-D warnings`）は通る。
- **hosted と merge**：PR #755 の hosted run 38168904641（rust 10m48s、conformance 13m15s、gates pass、head `f4c3dcc27`）、merge
  commit `994d449fc`。次は R3（parse と bind が option を読まない）。

## R3 parse と bind が option を読まない（2026-10-12）

- tsgo の scanner は ESNext の identifier の表だけを持ち（`stringutil/identifier.go`）、parse は target を読まず、binder は option
  を読まない。parse cache の key は file 名・path・module indicator の option・script kind・text。tsc-rs は parse で target を、
  bind で `noFallthroughCasesInSwitch` を読み、document と lib の bundle の cache がそれで分かれていた。
- scanner と parser：ES5 の identifier の表と、scanner・parser・`SourceFile`・`ParseOptions` の言語 version を除いた。
  `is_identifier_text` と entity name の helper は target を取らない。`createIdentifier` の不正な identifier の再 scan（tsgo に
  無い。表が 1 つなら成功しない）と、それだけが使った scanner の投機の helper を除いた。正規表現は checker の target で検査する
  （tsgo grammarchecks.go:76）。
- binder は option を取らない。case 節の fallthrough の flow は常に記録し（tsgo `bindCaseBlock`）、診断は checker が
  `noFallthroughCasesInSwitch` のときだけ出す。
- cache：`DocumentAddress` は parse の option と identity domain だけ、lib の bundle の key は lib の名前と text の列だけ（どの
  option の組でも 1 つの bundle）。option の projection・その key の型・harness の key を除いた。
- **test**：U+08A1 と U+10400 はどの target でも identifier（`/a/ࢡ` は tsgo の checker が TS1499 にする flag、`foo.ࢡ` は
  parse できる）。binding の名前の不正な escape の emit の回復は、どの target も受け付けない `\u{30}` で確かめる。ES5 で観測した
  fixture の case のうち ES2015 と同じもの（literal 11、entity name 43）を除いた。tsgo との横並び（R3 の 15 構成と R2 の 20 構成）
  は全て同じ。
- **結果**（release build、head `891a3db64`）：full の conformance（`--workers 2 --check`、473 s）は 0 regressions で R2 の
  report と全ての field で同じ。並列対照は記録済みの順序の 4 構成だけ。suites（`--check`）は 0 regressions、既知の違い 10。
  syntax・binder・checker・emitter・program・compiler・api・cli・conformance の test 3,865 と clippy（`-D warnings`）は通る。
  TypeScript の API の client の test（local）は sync 336／356、async 345／365 で変わらない。
- **hosted と merge**：PR #756 の hosted run 38172710048（rust 10m55s、conformance 21m12s、gates pass、head `891a3db64`）、merge
  commit `4d7f1e58e`。R1〜R3 でこの計画は終わる。
- **残る差（bounded、利用者の判断が要る）**：client の test「createSourceFile shares identity with a matching program file」は
  option ではなく identity domain で分かれる。lib を持つ program は lib の bundle の domain で node の id を割り当て、作った file は
  registry の domain にあるので共有できない。同じ理由で、lib の組が違う project の間でも parse を共有できない。domain を 1 つに
  まとめれば共有できる（checker の node の表は page 単位で確保するので id の範囲が広がっても大きくは増えない見込み）が、program
  をまたぐ id の割り当ての変更なので、性能の計測と合わせて判断する。
