# TypeScript 7.1のテスト構成に合わせたconformance

状態：**P1 runner 実装済み**（2026-09-29、[P1の結果](#p1の結果2026-09-29)）、**P2-1 既定値の移行とhosted CI組込み済み**（2026-10-01、[P2-1の結果](#p2-1-既定値の移行2026-10-01)）。残るP2の修正とprofileの移行（P3）はこれから。
親計画：[TS7の方向](../../typescript-7-direction.md)、[7.1追従設計](../../typescript-7-upstream-sync.md)、
[移行基盤batch](../post-emitter-foundation-batch/README.md)のA2（test inventory）とA4（MAP/PIN）。
調査記録：[SURVEY.md](SURVEY.md)（上流harnessと現行runnerの比較、数値の根拠）。

## ユーザー決定（2026-09-29）

| # | 決定 | ユーザーの言葉 |
| --- | --- | --- |
| 1 | conformanceをTypeScript 7.1で実行されているテストの構成に合わせる | 「TypeScript7.1で実行されているテストに合わせたい。TypeScript6までとテストの構成が微妙に変わっており、今後はそちらに合わせていきたい」 |
| 2 | 期待結果は7.1のbaseline。ただし6.0で非推奨になったtarget/optionはエラーにせず、6.0.3の結果を維持する | 「7.1 のベースラインで非推奨のターゲットはエラーにせず6.0.3の結果を維持」 |
| 3 | 実装済みのES5・outFile・AMD/UMD/System対応は削除しない | 「この処理はがんばって実装したので削除するのはもったいないです」（その前の「維持が難しい場合はエラーにしても良い」への補足） |
| 4 | 非推奨optionを使う場合のTS5101/TS5107は6.0.3どおり出す。`ignoreDeprecations: "6.0"`で抑止する | 選択肢「6.0.3 どおり出す」を選択 |
| 5 | 非推奨機能以外のtsc-rs自身の挙動を7.1へ移行する（unionの安定順序、ES2026、既定target、7.xのJS/JSDoc変更など） | 選択肢「7.1 に移行する」を選択 |

これにより、[TS7の方向](../../typescript-7-direction.md#reference-selection)が「明示的なprofile移行まで6.0.3」
としていたaccepted profileは、7.1（非推奨機能のみ6.0.3）へ移る方針になった。
移行そのものは下記P1〜P3の結果と移行表を揃えて行い、本書の作成だけではprofileを変えない。
READMEの「tsc 6.0.3と診断が一致」という記述も、移行時に「7.1と一致、非推奨機能は6.0.3と一致」へ改める。

## 固定参照

- `microsoft/TypeScript` main `19dadef8888ba5b27d8b9f622480745cf623e020`（2026-09-29、`7.1.0-dev`）。
  7.1のtagはまだなく（最新tagは`v7.0.2`、`release-7.0` branchあり）、7.1が出た時点で移る。
- 既存の調査pin `1f70213d4922b434345f639b441681e470c7cfc1`（`scripts/typescript7.py`）は別記録として残す。
  runnerのvendor取込みでは、取込みに使った完全SHAとtree idを固定する。
- 6.0.3側の比較元は現行の`ts-tests/`（TS commit `050880ce59e30b356b686bd3144efe24f875ebc8`）と
  vendorの`typescript.js`。

## 調査結果の要点

詳細と出典の行番号は[SURVEY.md](SURVEY.md)にある。数値は一回限りのemulationによる
調査値で、P1のRust runnerで測り直したものを正とする。

- **配置**：`tsc/testdata/tests/cases/{compiler 6,839, conformance 5,911, transpile 25}`と`tests/lib`の
  React宣言4件。期待値は`tsc/testdata/baselines/reference/{compiler,conformance}/`に平坦に置かれ、
  `.errors.txt`は7,320件。submoduleの区分と`.diff`はなくなり、`submoduleAccepted.txt`等は履歴台帳。
- **6.0.3からの差**（blob id比較）：同一12,076、内容変更252、削除139、追加447。
  変更・削除のほぼすべてが6.0の非推奨optionに関係する（AMD→commonjs、outFile→outDirへの書換え等）。
- **harness**：directiveはfile全体から読み、unit本文から除き、CRLFをLFにし、先頭の空行を落とす。
  cwdは`/.src`。72のoptionが`*`/除外指定を含む組合せ展開の対象。既定値は`newLine: crlf`、
  `skipDefaultLibCheck`、`noErrorTruncation`。既定targetはES2026。
  ES5・UMD/System・classic/node10・baseUrl・`esModuleInterop=false`・`allowSyntheticDefaultImports=false`・
  `alwaysStrict=false`の設定はskip、AMDとoutFileは致命的エラー。
- **7.1の非推奨option**：TS5102/TS5108（removed）を常に出し、その後は無視する。
  `ignoreDeprecations`は受け付けるが読まない。6.0.3はTS5101/TS5107（deprecated）を出す。
- **`.errors.txt`**：CRLF、末尾改行なし。要約行の列はUTF-16、squiggleの幅はcode point数。
  診断の順序はGoの`CompareDiagnostics`（message keyと引数）で、6.0.3の表示文字列順と異なる。
- **現行runnerとの差**：
  - conformance 5,908 fixtureだけが対象で、directiveは先頭部分だけを読む。
  - 行末と途中のdirective行を残すため、行番号がずれる。
  - goldenは`noLib`とlib rootを使っているため、`/// <reference lib>`が効かない。
  - suggestionは含まれ、options/global/config診断は含まれない。
  - 5,857 fixtureがcheckerの旧in-memory module解決を通る（残る51件だけがProgram経路）。
- **7.1へ寄せるtsc-rs側の不足**：
  - unionの安定順序。6.0の`stableTypeOrdering: true`に当たり、tsc-rsはoffの経路だけを持つ。
  - ES2026のtargetとlib。
  - 8件のlib差分。
  - `tsc/CHANGES.md`のJS/JSDoc変更と文言・位置の変更。

## 設計

### lane A / lane B

設定展開後の各configurationを、有効option（tsconfigの`compilerOptions`にdirectiveを重ねたもの）で振り分ける。

| lane | 条件 | 期待結果 | 入力の識別 |
| --- | --- | --- | --- |
| A | 下記に当たらない | 7.1の`.errors.txt`。baselineがなく、実行されたconfigurationなら診断0件 | 7.1のblob |
| B | target es3/es5、module none/amd/umd/system、moduleResolution node/node10/classic、`outFile`/`out`/`baseUrl`、`esModuleInterop`/`allowSyntheticDefaultImports`/`alwaysStrict`がfalse、`downlevelIteration`、5.5で削除されたoptionのいずれか | 6.0.3 oracle。TS5101/TS5107を含む（決定4） | 6.0.3のblob |

- 7.1で書き換えられたcase（例：`@module: amd`→`commonjs`）は別のprogramとして扱う。
  6.0.3のblobはlane Bに残り、7.1のblobはlane Aの新しいcaseになる。
- 7.1で削除されたcaseはlane Bに残り、追加されたcaseはlane Aのみに入る。
- 検算に使う条件：
  - lane Bのconfigurationに7.1のbaselineがないこと（`downlevelIteration`はTS5102入りのbaselineがあるので除く）。
  - 7.1のbaselineにTS5102/TS5108/TS6046/TS5023が出たら、そのcaseを確認する。
- lane Bの期待値は、初めは現行の6.0.3 golden（schema 3）を使う。
  その後、vendorの`typescript.js`でharnessIOの`compileFiles`＋`getErrorBaseline`を再現した
  6.0.3用の成果物を新しいfamilyとして作り、lane Aと同じ粒度で比較できるようにする。
  既存goldenとその系譜は書き換えない。

### runner

- **入力**：`vendor/typescript-native/<profile>/`（[配置案](../../typescript-7-upstream-sync.md#配置の案)）に次を固定する。
  - `tests/cases/{compiler,conformance}`と`tests/lib`
  - `baselines/reference/{compiler,conformance}/*.errors.txt`
  - baselineの全file名の一覧（「実行され診断0件」と「skip」を区別するため）
  - 7.1のlib一式
  - commit・tree id・hashのmanifest
- **展開**：
  - unitは既存の`crates/harness/src/upstream_suites/compiler.rs`（Strada互換の分割）を再利用し、
    BOM/UTF-16の解読と`;`の除去を加える。
  - configurationはGoの`GetFileBasedTestConfigurations`を、7.1のvaryBy集合・enum表とともに移植する。
    baseline名を同じ規則で作る。
  - `skippedTests`と`SkipUnsupported`も移植する。skipされたconfigurationはlane Bの判定に使う。
- **実行**：両suiteをProgram経路（compilerの`ProgramSession`と`MemoryCompilerHost`、`load_compiler_no_emit`の一般化）で動かす。
  - cwdは`/.src`。root規則、tsconfig、symlink/link、`/.lib`、harnessの既定値を再現する。
  - 集める診断は、config＋options＋syntactic＋semantic＋global＋declaration（declaration emitが有効な場合）。
    suggestionは`@captureSuggestions`のときだけ集める。
  - checkerの旧in-memory module解決は使わない。
- **比較**：`.errors.txt`を構造化recordへ解析して比較する。
  - 要約行からfile・行・UTF-16列・category・code・展開済み文言を取る。
  - file blockからchain・related・squiggle幅を取る（squiggle幅はcode point数を、表示行を使ってUTF-16へ換算する）。
  - 採点は既存のT0〜T3（key集合、件数、category、start/length/先頭文言、chain/related）。
  - 最上位tierとして、Go形式の完全な描画とbyte比較を後から加える。
- **ratchet**：`ratchets/ts71/`に新しいfamilyを置き、append-onlyにする。
  6.0.3の`conformance-matches`・`oracle-inputs`とその系譜は変更しない。
- **再現用の上流実行**：`scripts/typescript7.py`で同じcommitのGo runnerを動かし、取込みの検算と原因調査に使う。
  別系統のrunnerは作らない。

## 実装順と終了条件

| 段階 | 内容 | 終了条件 |
| --- | --- | --- |
| P1 runner | 固定参照のvendor取込み、設定展開・skip・lane判定、Program経路での実行、`.errors.txt`比較、ratchet familyの作成。挙動の変更は含めない | 7.1の全compiler/conformance configurationについて、lane・期待値・実測をT0〜T3で記録した初回値。emulationとの件数照合。Goのbaseline名と展開結果の一致 |
| P2 挙動移行 | 失敗の多い順に不足を埋める：unionの安定順序、ES2026 target/lib、診断範囲、`CHANGES.md`の変更点、lib更新 | 各変更で該当laneの合格数が増え、lane Bと既存の6.0.3 witnessに退行がない。移行表に採用範囲を記録 |
| P3 置換 | lane Bを6.0.3の新成果物へ移し、旧conformance runnerを引退させる。checker unit test（`check_program*`の約280箇所）をtsc_programの解決へ移し、旧in-memory module解決を削除する | 旧runnerなしでlane A/Bがratchetで守られる。`authoritative_module_provider`がNoneの経路が消える |

P1だけで先にhosted CIへ入れるかは、実行時間を測ってから決める（共有CIの変更は統合担当の手順に従う）。

## P1の結果（2026-09-29）

挙動の変更は含めていない。tsc-rsのchecker・program・libは6.0.3のままで、差は下記の「P2の入力」として残した。

### 実装

- **取込み**：`vendor/typescript-native/7.1.0-dev-19dadef8/`（20,187 file）。`manifest.json`がcommit、setごとのtree id
  またはblob一覧、baseline全45,533名を固定する。`scripts/vendor_typescript_native.py --profile <name> --check`と
  harnessの`native_vendored_inputs_match_the_manifest`で検算する。
- **展開**：`crates/harness/src/upstream_suites/native.rs`にGoの`GetFileBasedTestConfigurations`（72 option、`*`と除外指定、
  25を超える組合せは致命的）、`skippedTests`（42件）、`SkipUnsupported`を移植した。
  `native_expansion_reproduces_the_reference_baseline_configurations`が7.1のbaseline名との一致を確かめる。
  12,748 caseのうち実行されるconfigurationは13,467で、baselineのある13,422件と出力のない45件（`noTypesAndSymbols`）に分かれる。
- **実行**：`load_native_compiler_program`と`ProgramSession::run_for_native_harness`。Goの`CompileFilesEx`と`compileFilesWithHost`に合わせて次を行う。
  - optionはすべて設定どおりに使い、`noEmit`を足さない。harnessの既定（CRLF、`skipDefaultLibCheck`、`noErrorTruncation`）を入れ、
    `rootDir`・`declarationDir`・`tsBuildInfoFile`を絶対pathにする。
  - 同じpathのunitは後のもので置き換える。root fileが`/.lib/`を含むときは`tests/lib`を`/.lib`にmountする。
  - config・options・出力pathの検査（TS5055など。`@suppressOutputPathCheck`では行わない）・syntactic・semantic・global・
    declaration（declarationを出す設定のとき）・suggestion（`@captureSuggestions`のとき）の診断を、CLIのgateなしで集める。
  - lib bundleはconfigurationごとに作る。process寿命のbundle cacheは数千configurationでworkerのmemoryを使い切るので使わない。
    lib catalogは6.0.3のまま（ES2026のlibはP2）。
- **比較**（`crates/conformance/src/ts71.rs`）：`.errors.txt`の要約行を、T0（file・行・UTF-16列・code）、T1（category）、
  T2（1行目の文言）の順に比べる。T3は、Goの`GetErrorBaseline`を移植した描画（`ts71/errors_baseline.rs`）との
  `.errors.txt`全体のbyte一致とする。T3はGoのtestそのものの合格条件で、span（squiggle）・chain・related情報・順序を含む。
  `@pretty`の14 caseはT3の対象外。
- **実行器**：`scripts/conformance_ts71.py`が`conformance-ts71 --worker`を4 processで動かす。stack overflow・120秒の無進捗・
  3 GiB超のconfigurationはharness errorとして記録し、次のconfigurationから再開する。binaryの`--dump <dir>`は、
  一致しないconfigurationについてtsc-rs側の`.errors.txt`を書き出す。
- **ratchet**：`ratchets/ts71/7.1.0-dev-19dadef8.tsv`。T0以上に達したlane Aのconfigurationと、その最深tierを並べる。
  `--check`は後退を検出し、`--update`は下げずに記録する。tierを下げるのはreviewした編集だけにする。

### 初回値

`python3 scripts/conformance_ts71.py --workers 4 --update`で測った（release build、`taskpolicy -b nice -n 20`で862秒。
開発機を低優先で使った参考時間で、性能計測ではない）。

| 区分 | configuration |
| --- | ---: |
| lane A | 13,444 |
| 　T3：`.errors.txt`が完全一致 | 12,467（92.7%） |
| 　T2まで一致（1行目の文言まで） | 124 |
| 　T1まで一致（categoryまで） | 100 |
| 　T0まで一致（位置とcodeまで） | 0 |
| 　不一致 | 696 |
| 　harness error | 57 |
| lane B（6.0で非推奨のoption） | 1,742 |
| skip list（Goも実行しない） | 42 |

- **検算**：lane Aの13,444件と、lane Bの`downlevelIteration`の23件（7.1は実行してTS5102を出す）の和13,467は、
  展開が予測した実行configuration数と一致する。lane Bの内訳は`target: es5` 1,315、`module` 215、
  `moduleResolution` 82、`alwaysStrict: false` 52、`baseUrl` 31、`esModuleInterop: false` 24、`downlevelIteration` 23。
- **T3の描画**：chain・related情報・libのmaskを含む`arrayAssignmentTest1`、`abstractPropertyInConstructor`などで、
  Goの出力とbyte一致した。T2までの224件にT3で残る差は、実際の挙動の差だった（下記）。

### P2の入力

- **harness error（57件）**
  - stack overflow 16、120秒の無進捗 6、memory上限 1（`templateLiteralTypeExcessiveLength`）。6.0.3も同じように失敗し、
    7.xで直ったcase（`circularDestructuring`、`unreachableFlowAfterThrowingFor*`、`excessivelyDeepConditionalTypes`など）。
  - ES2026のtarget 2とlib 10。
  - 7.xで加わったoption：`runExternalCode`（content mapper）15、`deduplicatePackages` 2。
  - declaration診断の経路が`tsBuildInfoFile`を受け付けない 2、`stripInternal`で未対応のcontract 1。
  - panic 2（`decoratorRestNoCrash1`、`dependentDestructuringCrossFilePosition`）。
- **T1・T2に留まる224件**：union・propertyの表示順だけの差 54、chainの差 51（7.1は多重定義の失敗を
  「The last overload gave the following error」で報告する）、文言の差 70（6.0.3のTS1344にある先頭の`'`などの修正）、
  要約行は同じでrelated情報の位置やspanだけが違うもの 49。
- **不一致（696件）**：最初に食い違う診断のcodeは、7.1にだけあるものがTS2741 51、TS2683 37（JSの`this`）、
  TS2339 29、TS2300 29、TS6196 26（未使用の型引数）、TS2307 17（module解決）、TS1005 16（parser）、TS6504 14。
  tsc-rsにだけあるものがTS2345 44（7.1は引数の不一致をTS2741などで直接報告する）、TS6133 39、TS2344 14、
  TS2857 10（import attributes）など。

### P1で残したもの

- lane Bの比較は、旧runner（`cargo xtask conformance`と6.0.3のgolden）が引き続き担う。新runnerはlane Bを分類するだけ。
- T3は`@pretty`の14 caseを対象外にしている。`stableTypeOrdering`（2 case）はtsc-rsに安定順序の経路がないので無視している。
- hosted CIへの組込みは、P3で旧runnerを引退させるときに合わせる。

## 未決事項とリスク

- **unionの安定順序**：診断文中の型表示が広く変わる。移行はP2の最初の大きな変更になる見込み。
  CLIの出力比較（READMEの26構成）も更新が必要になる。
- **位置**：7.1の内部位置はUTF-8だが、baselineの列はUTF-16。squiggleだけcode point数で数える。
- **lib**：7.1のlibを同梱すると既定の型環境が変わる。lane Bには6.0.3のlibを使う。
- **promotion rename**：4件は6.0.3のどちらの版とも一致しないので、blobで識別する。
- **transpile / FourSlash / project**：本packetの対象外。transpileは25件で別runner、
  FourSlashはGoのtest、projectは7.1にcase fileがない。

## P2-1 既定値の移行（2026-10-01）

ユーザー指示：「tsgoの結果とマッチングさせる方向にしていきたい。まずはCIをTypeScript 7.1のconformanceで完全一致させる修正をかけていきたい」。
conformanceは1 checker固定で走らせ、並列実行が順序以外で一致することは別に確認する。

### 参照profile

tsc 6.0.3とTypeScript 7.1がoptionなしに異なる点を`ReferenceProfile`（`crates/types/src/options.rs`）にまとめ、
`CompilerOptions.reference_profile`として持たせた。`LibraryCatalog`がprofileを決め（`typescript_7_1`／`typescript_6_0_3`）、
loaderがProgramのoptionsに写す。config parserは`ConfigParseHost::reference_profile()`で同じ値を受け取る。

| 差 | 6.0.3 profile | 7.1 profile（tsc-rsの既定） |
| --- | --- | --- |
| lib catalog | `vendor/typescript-6.0.3/lib`の107 entry | `tsc/internal/bundled/libs`の115 entry（es2026.*、`esnext.array`等はes2026へ） |
| 既定target | ES2025 | ES2026（`target: "es2026"`を受け付ける） |
| unionの順序 | 生成順（`stableTypeOrdering: true`で内容順） | 内容順（`stableTypeOrdering: false`で生成順） |
| 診断文言 | 6.0.3の`diagnosticMessages.json` | 7.1の`diagnosticMessages.json`（9件の文言変更、1463/1464の削除、87件の追加） |
| `resolution-mode` import attribute | TS1463/TS1464の鍵と個数の検査 | 7.1どおり鍵を探すだけ（値の検査のみ） |
| lib提案表（TS2550等） | 6.0.3の`getScriptTargetFeatures` | 7.1の`getFeatureMap` |

6.0.3の文言を要する5箇所（TS1344 binder、TS8030 checker、TS9019 emitter、TS5090 program、TS1463/1464 checker）は
生成した`gen::typescript_6_0_3`の静的データをprofileで選ぶ。`crates/diagnostics/src/gen.rs`は7.1のcatalogから生成し、
6.0.3で文言が異なる、または7.1にない11 entryを`typescript_6_0_3` moduleに持つ。

凍結した6.0.3の記録（acceptance／witness）は`LibraryCatalog::typescript_6_0_3`と6.0.3 oracleのconfig hostを通るので
挙動が変わらない。CLI（`tsc-rs`）、Rust APIの例、native harnessは7.1 profileで動く。transpile経路（`transpile_module`／`transpile_declaration`）は
凍結した6.0.3の観測（`h2_8c_transpile`）と比べているので、再観測するまで6.0.3 profileに留める（hosted witnessで判明：TS6046の候補一覧と既定targetが変わるため）。
`--version`は6.0.3のまま（emitterの参照）で、READMEにその旨を記した。

### 7.1側の取込み

- `vendor/typescript-native/7.1.0-dev-19dadef8/`に`tsc/internal/diagnostics/diagnosticMessages.json`を1 fileのsetとして追加
  （`git_blob_sha1`をmanifestに記録。`scripts/vendor_typescript_native.py`と`native_vendored_inputs_match_the_manifest`が検算）。
- executableに同梱するlibを7.1の113 fileに替えた（`crates/compiler/build.rs`）。

### 計測

`python3 scripts/conformance_ts71.py --workers 4`（release build、`taskpolicy -b nice -n 20`、開発機の参考時間）。

| 区分 | 変更前（main `ed173ea36`） | 変更後 |
| --- | ---: | ---: |
| lane A | 13,444 | 13,444 |
| 　T3：`.errors.txt`が完全一致 | 12,467 | 12,569（93.5%） |
| 　T2 | 124 | 116 |
| 　T1 | 100 | 21 |
| 　不一致 | 696 | 693 |
| 　harness error | 57 | 45 |
| ratchet | 0 regressions | 104件を上げ、2件を見直しで下げた（下記）。`--update`後の`--check`は0 regressions |

同じbytesで1 checkerの全件を2回走らせ、`scripts/conformance_ts71_compare.py`で`rendered_sha256`まで比べて
15,228 configurationすべてが同一だった（713秒と733秒）。

tierの推移（変更前→変更後、configuration数）：T1→T3 78（7.1の文言、主にTS1344）、harness error→T3 11（ES2026のtargetとlib）、
T2→T3 9、不一致→T3 5（es2026 libの member）、T3→不一致 2、不一致→harness error 1
（`compiler/importAttributeTypeOnlyImports`：7.xの`declare module "x" with { ... }`。import attributes classの一部）。
`resolution-mode` attributeはloader側（`crates/program/src/module_requests.rs`）もcheckerと同じ7.1の規則に直した。
直さないとloaderが用意しない解決modeをcheckerが求め、"authoritative Module resolution is missing"になる。

T3→不一致の2件は、7.1がtsc 6.0.3の`--stableTypeOrdering`とも異なる推論の修正で、ratchetの行を見直しのうえ下げた
（`compiler/implicitEmptyObjectType`：Go issue 1563、`unknown || {}`のunion縮約で残るmember；
`compiler/nestedGenericTypeInference`：Go issue 1789、`T[] | T[][]`へのunion引数の推論）。
tsc 6.0.3は生成順でそれぞれ一致・不一致、`--stableTypeOrdering`で不一致・一致、tsgo nightly（2026-07-07）は
7.1 baselineと同じ。tsc-rsは`--stableTypeOrdering`付きtsc 6.0.3と同じ結果になる。両方とも7.xの修正の移植が要る。

### 並列実行の対照

conformanceの比較は1 checkerで行う（`ProgramSession`の既定、`CheckerBudget::serial()`）。並列実行が順序以外で一致するかは、
同じbytesで`scripts/conformance_ts71.py --workers 4 --checkers 4`（各configurationを4 checkerで検査、2 file以上のcaseはfileごとに
shardされる）を走らせ、1 checkerのreportと`conformance_ts71_compare.py`で比べた。

- 15,228 configurationのうち15,223が同一（outcome・tier・描画した`.errors.txt`の digestまで）。
- 5件が順序以外で異なる。いずれも型の順序ではなく、partitionに依存した挙動の欠陥で、別PRで直す：

| configuration | 1 checker | 4 checkers | 見立て |
| --- | --- | --- | --- |
| `compiler/exportAssignmentMembersVisibleInAugmentation` | 不一致（TS4060を出さない） | T3 | serialがaugmentationの可視性を見落とす。並列が正しい |
| `compiler/tslibMissingHelper` | 不一致（TS2343を2件中1件） | T3 | serialはhelperの欠落をprogramで1回しか報告しない。tscはfileごと |
| `conformance/jsDeclarationsCrossfileMerge(target=es2015)` | 不一致（2件中1件） | 不一致（0件） | JS宣言のfile横断mergeの診断がpartitionに依存 |
| `compiler/declarationEmitAugmentationUsesCorrectSourceFile` | T3 | harness error | declaration transformerのcontract "late visibility alias belongs to another source" |
| `compiler/declarationEmitComputedPropertyNameSymbol2` | T3 | harness error | 同上 |

READMEのcorpus（hono、zod、Playwright、TypeScript `src/compiler`、Next.js、Effect、VS Code）でも、同じbinaryを
`TSRS_CHECKERS=1`／8／12で走らせた：`--noEmit`の診断は7 corpusすべてで同一、declaration出力はzod・Playwright・Next.jsで
同一、Effectは496 fileのうち`ai/internal/mcpProtocol/v2026_07_28.d.ts`の1 file（既知の型ID tiebreak、順序のみ）。
1 checkerの結果は7 corpusすべてで`tsc --stableTypeOrdering`（6.0.3）と診断・`.d.ts`とも同一だった。

### hosted CI

`.github/workflows/conformance-ts71.yml`（plan → `conformance (TypeScript 7.1)` → `conformance-ts71-gates`）を追加した。
`replay.py plan`が`has_conformance_ts71`を出し、早期group（`early`）が選ばれる変更で走る。jobはrelease buildの
`conformance-ts71`を`scripts/conformance_ts71.py --workers 4 --check`で動かし、ratchetの後退で失敗する。
`ci.yml`は変えていない（`qualification.mjs`がその構成を固定しているため、witnessと同じく別workflowにした）。
branch protectionの必須checkに`conformance-ts71-gates`を加えるのはrepository設定の作業。

### 残り

不一致693件の最初に食い違うcode（missing=7.1だけが出す、unexpected=tsc-rsだけが出す）の上位：

| 件数 | missing | unexpected | 見立て |
| ---: | --- | --- | --- |
| 27+14+6 | TS2741 | TS2345／TS2344／TS2322 | 7.1は引数の不一致を直接の原因（TS2741等）で報告する（elaboration） |
| 26+8 | TS6196 | TS6133 | 未使用の型引数は7.1でTS6196 |
| 24+5 | TS2683 | – | JSの`this`の暗黙any |
| 23+12 | TS2300 | – | 重複識別子の報告位置・件数 |
| 20 | TS2339 | – | 存在しないpropertyの報告（lib・JS関連） |
| 20 | TS2769 | TS2769 | overload解決の失敗の報告（"The last overload gave the following error"の鎖） |
| 18+7 | TS2309 | – | export assignmentと他のexportの衝突 |
| 17 | TS2304 | – | 名前解決 |
| 17 | TS2749 | – | 値を型として使った報告 |
| 14 | TS6504 | – | JS fileの`--allowJs`なし |
| 13 | TS2552 | TS2304 | 綴り候補 |
| 12 | TS2303 | – | circular definition of import alias |
| 10+6 | TS2307 | TS1479 | module解決（node16/nodenext） |
| 8+6 | – | TS2857／TS2856 | import attributes（7.1は型のみimportの検査を飛ばす） |
| 5 | TS6046 | TS5108 | 7.1は削除optionの値をTS5108で報告 |
| 残り | 多数の小class | | |

harness error 45件：load 16（`runExternalCode`／`deduplicatePackages`等の7.x option、`tsBuildInfoFile`経路）、
crash 15（stack overflow）、timeout 6、check 5、panic 2、memory 1。
T2以下137件：chainの差（7.1の"The last overload gave the following error"等）、related情報の位置、union・property順の残り。

次の一手は件数順に、elaboration（TS2741）、TS6196、JSの`this`、TS2300、overload chainの順で、各classを独立したPRにする。
lane Bは引き続き旧runnerが6.0.3 goldenで守る（P3で6.0.3 oracleを`--stableTypeOrdering`付きで取り直した成果物に移す）。
