# Content mapper：計画（2026-10-12）

利用者の指示（2026-10-11）：「比較が終われば次は.types/.symbolsに進んでください。それが終われば一度パフォーマンス計測を
おこなってチューニングしたいと思います。content mapperに進みます。」。性能の計測とチューニング（[ts71-perf-2026-10](../ts71-perf-2026-10/README.md)、
#758・#759）の後に着手する。LSP の部分（project・ls・lsp・fourslash の content mapper）は LSP と一緒に後で行う
（[post-emitter roadmap](../../post-emitter-roadmap.md) の API1.2-EXT）。

## tsgo の content mapper

- **何か**：TypeScript が読めない file（`.vue` など）を、program を作る途中で仮想の TypeScript に変える plugin。tsconfig の
  `contentMappers`（`{ "package": "mapper", "extensions": [".vue"], "options": ... }`）で宣言し、npm package の package.json の
  `typescript.contentMapper`（`exec`・`compilerOptions`・`dynamicConfig`）が実行の仕方を決める。`--runExternalCode` が無いと
  TS18068 を報告して mapper を使わない。
- **process**：tsgo は package の `exec` を子 process として起動し、JSON-RPC（`initialize`・`openProject`・`closeProject`・
  `transform`）で話す。同じ identity（`name@version`）の mapper は 1 つの process を共有する。`initialize` は 5 秒で打ち切り、
  位置の encoding（UTF-8／UTF-16）と診断の source を受け取る。
- **変換の結果**：仮想の text とその拡張子（`.ts`・`.tsx`・`.js`・`.json` など）、元の text との span map（verbatim・atom・
  alias の segment、言語 service の feature）、元の text の構文の診断、診断の directive（仮想の範囲の診断を ignore／expect
  する）、追加の出力（supplemental file：program に別の file として入る）。
- **失敗**：初期化の失敗は program の診断 1 つで、その mapper は以後使わない。変換の失敗は file ごとの診断で、5 回で mapper を
  止める（診断 1 つ）。失敗した file は空の TypeScript の file（failure stub）として program に入り、import した側に
  「module が無い」を出さない。
- **診断の表示**：位置は仮想の text のもの。表示する時に span map で元の text に写す（alias の segment は message の引数の名前も
  元の名前に替える）。写せない合成のコードの診断は仮想の text で表示する。
- **emit**：content-mapped file の JS は出さない。宣言は出す（`declaration` の時）。宣言 map は元の text を指す。
- **incremental**：build info の `contentMapperIdentities` が変わると作り直す。file の version は「元の text + NUL + transform
  identity」。

## 参照（tsgo `aa8149273`）

| 在りか | 行 | 役割 |
| --- | ---: | --- |
| `contentmapper/contentmapper.go` | 181 | `Definition`・`Manifest`・`Mapper`、identity、`TransformIdentity`（xxh3-128） |
| `contentmapper/host.go`・`hostimpl.go` | 316・1,374 | `Host`・`Project`、protocol、process の lifecycle と共有、handshake、project（dynamic config・watched files・option diagnostics）、結果の decode、位置の正規化、失敗の種類、timings、locale |
| `contentmapper/transform.go` | 135 | 結果を canonical と supplemental の source file に parse、名前の衝突 |
| `spanmap/spanmap.go` | 818 | segment・feature・fidelity、仮想と元の位置の写像、検査、JSON |
| `ipc/` | 1,596 | API server と共有の JSON-RPC の接続。content mapper は `AsyncConn`（同時の call） |
| `tsoptions/` | 約 300 | `contentMappers` の parse（`extends` を含む）、TS18068、manifest の解決（bundler の package directory の解決）、診断の位置、mapper の拡張子を含む file の列挙 |
| `compiler/` | 約 500 | file loader（`parseContentMappedFile`、失敗の stub と予算）、host の parse cache、program（`GetContentMapper`、option diagnostics、更新時の再利用、診断の directive）、emitter、`outputpaths` |
| `ast/`・`diagnosticwriter/` | 約 150 | `SourceFile` の content mapper の情報、alias の引数、写した表示 |
| `execute/` | 約 300 | tsc（spawner と `--runExternalCode`）、incremental（build info）、build、watch |
| `api/` | 約 100 | encoder の field、server の spawner |
| `testutil/contentmappertest/` | 1,595 | in-process の test mapper 18 種（`net.Pipe` の上の JSON-RPC） |

## 合わせる test

- **conformance**：`compiler/contentMapper*.ts` の 15。今は `@runExternalCode` を知らず harness error（`unsupported compiler
  fixture option "runExternalCode"`）。baseline 47：`.contentmapper` 15（元の text・変換後の text・写した診断）、`.types` 10、
  `.symbols` 10、`.js` 6、`.js.map` 2、`.sourcemap.txt` 2、`.errors.txt` 2。tsgo の runner は content-mapped file の baseline に
  変換後の text を使い、error baseline からは除く。
- **suites**：tsc 6（`contentMapperExplainFiles`、`contentMapperFailures` 3、`contentMapperOptionDiagnostics`、
  `contentMapperSynthesized`）と tsbuild 2（`contentMapperIdentity`、`contentMapperOptionDiagnostics`）が `none`。
  `tsc/contentMapperEmit` は既に一致。
- **unit test**：tsgo の Go の test を Rust に移す。`spanmap_test.go`（744）、`transform_test.go`（110）、`host_test.go`
  （1,315）、`tsoptions/contentmappers_test.go` と `tsconfigparsing_test.go` の content mapper の case、
  `compiler/contentmapper_test.go`、`incremental/buildinfo_contentmapper_test.go`、`tsctests/contentmapper_watch_test.go`。
- 対象外：LSP の部分と `packages/vscode-typescript`。API の client test は content mapper の field が空であることだけを見ていて、
  既に通る。

## port での置き場所

- **spanmap**：`tsc-diagnostics`（最下層。診断の表示が使う）。
- **ipc**：`crates/api/src/ipc` を `tsc-host` に移し、API server と content mapper で共有する（tsgo の `internal/ipc`）。
  `AsyncConn`（応答を id で突き合わせる同時の call）を足す。
- **content mapper の型・host・結果の decode**：`tsc-program` の `content_mapper`（tsoptions と file loader と同じ crate）。
- **子 process の起動**：`Spawner` trait。native の System は `std::process` で起動する。WebAssembly の System は spawner を
  持たず、tsgo の「起動できない」の初期化の失敗になる（[P4-6a の System 層](../ts71-suites/README.md)）。test の
  in-process の spawner と test mapper 18 種は `tsc-harness`（conformance と suites の runner が使う）。
- **位置**：tsgo の `TextPos` は UTF-8 の byte、port の位置は UTF-16 の code unit。mapper の位置（`initialize` で UTF-8 か
  UTF-16）は port の UTF-16 に正規化する（tsgo は UTF-8 に正規化する）。API の encoder は tsgo の UTF-8 に直して書く。

## 手順

- **C1 土台**（振る舞いは変わらない）：spanmap、ipc の移動と `AsyncConn`、mapper の型（`Definition`・`Manifest`・`Mapper`、
  identity、`TransformIdentity`）。test：`spanmap_test.go` の移植、`AsyncConn` の test、identity の test。
- **C2 host**：protocol、process の lifecycle と共有、handshake の検査、project、結果の decode、位置の正規化、失敗の種類、
  timings、locale。`Spawner`（native と in-process）と test mapper 18 種の移植。test：`host_test.go`・`transform_test.go`
  の移植と、本物の子 process で protocol を話す小さな test mapper の binary での native spawner の test。
- **C3 設定**：`contentMappers`（`extends` を含む）、TS18068、manifest の解決と診断、mapper の拡張子を含む file の列挙。
  test：tsoptions の test の移植。config の suites は変わらない見込み。
- **C4 program と conformance**：loader が content-mapped file を host で変えて parse する（失敗の stub と予算、初期化の
  失敗）、supplemental file、`SourceFile` の content mapper の情報、parse cache の key（transform identity）、診断の
  directive、alias の引数、写した診断の表示、emit（JS を出さない、宣言、宣言 map）、conformance runner（`@runExternalCode`、
  `.contentmapper` baseline、変換後の text の baseline）。conformance の 15 が一致するはず。
- **C5 CLI・incremental・build・watch**：tsc の spawner、`--explainFiles` の supplemental の理由、build info の
  `contentMapperIdentities` と file の version、`tsc -b` の option diagnostics と identity の変化での作り直し、watch の
  dynamic config の watched files。suites の 8 が一致するはず。
- **C6 API**：encoder の content mapper の field（`originalText`・`spanMap`・`supplementalSourceFileNames`・
  `canonicalSourceFileName`・`contentMapper`・`virtualFileName`・`diagnosticDirectives`）と server の spawner。
- 各 slice は今までと同じく full の conformance と suites で 0 regressions、checker に触れたら並列対照。

## C1 土台（PR #762）

- **`tsc_diagnostics::spanmap`**：tsgo `spanmap` の segment・kind・feature・fidelity、仮想と元の text（UTF-16）の上の検査、
  仮想から元への写像と alias の引き当て。位置は compiler の UTF-16 の code unit（tsgo は UTF-8 の byte。host が mapper の位置を
  正規化してから map を作る）。segment は mapper が送った値をそのまま持つので、壊れた map は検査に届いて tsgo の error の種類に
  なる。元から仮想への射影は言語 service だけが使うので、それと一緒に移す。
- **`tsc_host::ipc`**：JSON-RPC の message と framing を API の crate から移して content mapper と共有する（API は自分の server
  の接続を持ち、これらを re-export するので道は変わらない）。framing は読む側と書く側に分け、`AsyncConn` は tsgo の
  `ipc.AsyncConn`：任意の thread からの call を ID で応答に結び、call ごとの時間の限り（mapper の handshake の 5 秒）、閉じた
  接続の message（`ipc: connection closed` と原因）と相手の error の message（`ipc: remote error [code]: message`）。
- **`tsc_program::content_mapper`**：`Definition`・`Manifest`・`Mapper` と tsgo の identity・診断の名前・仮想の拡張子・
  transform identity（identity・mapper の options・宣言された compiler options の xxh3-128、正準の byte 順）。
- **test**：spanmap_test.go の仮想から元と検査の case（11）と mapping error の message、conn_async_test.go の case と並行の call・
  時間の限り（7）、transform identity は tsgo の `contentmapper` 自身が計算した値（`aa8149273` の tree に一時の probe を置いて
  実行し、後で消した）（3）。
- **結果**（release build、head `53a3393a7`）：full の conformance（482 s）0 regressions で #761 の report と全ての field で同じ、
  suites 0 regressions、test 4,155、clippy、API の client test（sync 337／356・async 346／365、変わらず）。
- **hosted と merge**：PR #762 の hosted run 38222612506（rust 8m13s、conformance 19m52s、gates pass、head `53a3393a7`）、merge
  commit `f053cac8c`。

## C2 host（PR #763）

- **`content_mapper::errors`**（tsgo host.go）：初期化の失敗の種類と tsgo の message、project の応答の誤り、directive の誤り、
  file loader が区別する transform の失敗。
- **`content_mapper::protocol`**：JSON-RPC の method の引数と結果、span map の tuple、transform の結果の decode。mapper は
  UTF-8 か UTF-16 で数え（`initialize` で選ぶ）、正規化は compiler の UTF-16 の code unit にする（tsgo は UTF-8 の byte）。
  code point の途中の位置は tsgo と同じく拒む。診断の directive の検査と正規化、supplemental の出力は自分の directive を持つ。
- **`content_mapper::host`**：mapper の identity ごとに 1 つの process（最初の利用で起動、5 秒以内の初期化、tsgo の
  NoResponse・ProcessExit・Request と結果の検査）、mapper の project は遅れて開き同じ spec で共有、static と dynamic の
  identity（xxh3-128、tsgo の `combinedIdentity`）、watched files、option diagnostics、最後の持ち主で process を閉じる lease と
  acquisition、時間の計測、protocol と stderr の log。
- **`tsc_compiler::content_mapper_process::NativeSpawner`**：tsgo の `spawnProcess` を `std::process` で（`PATH` にない
  command は Go の message）。
- **`tsc_host::ipc::pipe`**：process の中で動かす mapper のための memory の pipe。
- **test**：host_test.go の case を process の中の mapper で（12）、応答の case を decoder で（8）、本物の `/bin/sh` の mapper と
  `initialize` 前に終わる process と無い command（3）。
- **結果**（release build、head `ee84678f9`）：test 4,179、clippy、suites 0 regressions、full の conformance（487 s）0 regressions
  で #762 の report と 15,339 の構成が全て同じ。checker に触れないので並列対照は無し。
- **hosted と merge**：PR #763 の hosted run 38226414167（rust 11m47s、conformance 13m28s、gates pass、head `ee84678f9`）、merge
  commit `097f178bb`。`setLocale`（LSP だけの request）は言語 server と一緒に移す。

## C3 設定と C4a program・CLI（PR #764）

C3 だけでは載せられない（mapper の拡張子の file を列挙しても読めるのは C4a から。tsc suites の `contentMapperEmit` は、
`--runExternalCode` が落とされて `.vue` が列挙されなかったため偶然一致していた）ので、2 つを 1 つの PR にした。

- **C3**（`config_content_mappers.rs`）：`contentMappers` の entry（`package`・`extensions`・`options`、`extends` で最後に
  持つ base から継承）、拡張子の検査（TS18065–18067、Go の `EqualFold`）、`--runExternalCode` が無い時の TS18068、manifest
  の解決（`ResolvePackageDirectory`：`node_modules`、次に `@types`、symlink は実体）と TS18075–18079、mapper の拡張子ごとの
  supported extension の group。`runExternalCode` を command line の option に残す。`references` と `contentMappers` の
  object でない要素の TS5024（`references` の分は以前から抜けていた）。`ConfigParseHost::directory_exists`。
- **C4a**：`content_mapper::Project` を trait に、`content_mapper::source`（mapped file の情報、`ParseResult`、失敗の stub、
  file loader の診断）。loader の変換、supplemental の subtask、失敗の予算 5 と初期化の失敗、仮想名での parse と implied
  format、resolver の `extraExtensions`、mapped file の JS を出さない emit、CLI の spawner と project、tsgo の診断 writer
  （元の text か仮想の text、合成の code の注記、supplemental は canonical の file として、`TS` の代わりに mapper の source）。
  `tsc_harness::content_mappers`（tsgo contentmappertest の process の中の mapper 6 種）を suites の test system に。
- **test**：C3 は tsconfigparsing_test.go と contentmappers_test.go の case、tsgo の binary が出す診断（無効な entry の全種、
  `--runExternalCode` の有無、ずれた要素、継承）。C4a は fake project での loader（5）、`ParseResult` と診断（5）。
- **結果**（release build、code `58e0ce4ae` と ratchet `53f19e73d`）：test 4,204、clippy、suites 0 regressions で 6 が
  accepted tier を上回り `--update`（tsc の content mapper の baseline が none から full、7/7 一致）、full の conformance
  （502 s）0 regressions で C2 の report と 15,339 の構成が全て同じ、並列対照（`--checkers 4`、482 s）は記録済みの
  partition 依存の 4 構成だけ違う。
- **hosted と merge**：PR #764 の hosted run 38238772145（rust 7m13s、conformance 21m2s、gates pass、head `53f19e73d`）、
  merge commit `7ca1df174`。
- **残り**：C4b（conformance runner の `@runExternalCode` と `.contentmapper` baseline、他の test mapper、診断の directive、
  宣言 emit の細部。conformance の content mapper の 15 は harness error のまま）、C5（build info の identity、tsbuild の 2、
  watch）、C6（API）、mapper の JavaScript 出力の checker 上の JS 判定（file 名で決まる）、CLI の mapper の locale。

## 実プロジェクト（PR #765）

ユーザーの依頼で、content mapper を使う実際の Vue と Astro のプロジェクトを tsgo（`tsc-aa814927`）と比べた
（`~/dev/real-projects.noindex/content-mapper`、各プロジェクトの設定を `extends` して mapper を足した `tsconfig.cm.json`）。

- **対象**：vinhphm/site（Astro、`@astrojs/ts-content-mapper@0.2.0`、`.astro` 35）、withastro/starlight（Astro の
  monorepo、2,408 file、`.astro` 99）、antfu-collective/vitesse（Vue、`@chatium/vue-ts-mapper-rs@0.2.5`、`.vue` 10）、
  soybeanjs/soybean-admin（Vue、`.vue` 90）。診断（plain と pretty）、`--explainFiles`、`--listFilesOnly` が全て tsgo と同じ
  bytes。違うのは pretty の要約の mapped file の行の順だけ（tsgo は Go の map の順で、実行ごとに変わる）。mapper との通信
  （包む package で記録）も `openProject` の要求と返事、90 の transform の結果、最後の `closeProject` が tsgo と同じ。
- **直したもの**：一般の 4 つ（引数の TS2379 の判定を relater の報告に、合成 import の `--explainFiles` の理由と import の
  出現順の subtask、`pkg/` の package directory、ファイル末尾の JSDoc の import type）と content mapper の 6 つ（mapper の
  拡張子の解決先の読み込み、仮想名での JSX runtime、診断の directive（C4b から前倒し）、要約での mapped file の数え方、
  `project` option、command の終わりの project と host の close）。
- **結果**（head `b12d6e969`）：test 4,212、clippy、suites 0 regressions、full の conformance（491 s）0 regressions で
  #764 の report と 15,339 の構成が全て同じ、並列対照は記録済みの 4 構成だけ違う。
- **hosted と merge**：PR #765 の hosted run 38249930905（rust 12m49s、conformance 12m33s、gates pass、head `b12d6e969`）、
  merge commit `fd74846b0`。
- **次**：mapped file の変換を並列の read-ahead で（tsgo は parse task の中で変換する。soybean-admin の walk の 106 ms の
  大半が 90 の順番の変換）、その後 C4b。

## 変換の並列化（PR #766）

mapped file の変換を並列の read-ahead で行う（tsgo は parse task の中で変換するので、project の変換が mapper に同時に送られる）。
walk は変換を自分の順で受け取るので、失敗の予算、初期化の失敗、program は直列の loader と同じ（walk が使わない変換を worker が
送ることはある）。mapped root を 1 つ先頭に、残りを最後に置き、mapper の起動待ちで worker が全部止まらないようにした。

- **計測**（tsgo `tsc-aa814927` と、`-p <config> --runExternalCode --noEmit --pretty false`、warm-up 1 回の後 7 回の中央値。
  script はこの directory の `timing.py`）：
  site 199 → 175 ms（tsgo 比 0.88）、starlight 450 → 427 ms（0.95）、vitesse 98 → 128 ms（1.31、mapper 無しでも 71 対 92 ms の
  小さい project の固定費）、soybean-admin 234 → 249 ms（1.06）。出力は 4 project とも #765 と同じ。
- **結果**（head `77a00d651`）：test 4,213、clippy、suites 0 regressions、full の conformance（487 s）0 regressions で #765 の
  report と 15,339 の構成が全て同じ。
- **hosted と merge**：PR #766 の hosted run 38254782725（rust 7m49s、conformance 21m27s、gates pass）、merge commit `40c217168`。
- **次（C4b）**：span map の original→virtual（済、branch 上）、test mapper 6 種（済、branch 上）、vendor に `.contentmapper`
  baseline を加える、conformance runner の `@runExternalCode`・mapper project・`.contentmapper` baseline（ratchet の列）・
  mapped file を除いた errors baseline・types/symbols/js の変換後の text、宣言 emit の細部。

## C4b conformance runner と残りの C4（PR #767）

conformance runner が compiler の case の content mapper を process の中で走らせ（tsgo の compiler runner と同じ）、tsgo の
`.contentmapper` baseline を書く。case が要る program・emit・診断の振る舞いと合わせて、content mapper の 15 構成が全ての
baseline（errors、宣言を含む `.js`、`.js.map`、`.sourcemap.txt`、`.types`、`.symbols`、`.contentmapper`）で tsgo と一致する
（以前は harness error）。

- **runner**：tsgo `contentmappertest` の mapper のうち compiler の case が使うもの（`compiler-test-mapper` と project の
  lifecycle、Lisp、診断 code の衝突、supplemental の診断・global・module）を process の中の spawner で。LS だけが使うものは
  language service の作業で。`@runExternalCode` は config の解析に command line の `--runExternalCode` として渡り、config の
  解析は case の `@link` を見る（tsgo の test case parser は symlink の有る FS を渡す）。compile と宣言の compile が config の
  content mapper を持ち（tsgo `compileDeclarationFiles` が渡す）、load の後に mapper の host を閉じる。
- **`.contentmapper` baseline**（tsgo `DoContentMapperBaseline`）：mapped file ごとの元と変換後の text、その file の診断を
  program の診断 writer（`ProgramDiagnosticWriter`：`WrapASTDiagnostics` の上の `FormatDiagnosticsWithColorAndContext`）で、
  ANSI の装飾を除いて。errors baseline は mapped file とその診断を除き、残りを tsgo の診断の wrapper と同じく解決する。他の
  baseline では mapped file の内容を変換後の text に（tsgo の runner の `file.Content = sf.Text()`）、map の baseline は元の
  text を（`OriginalText()`）。vendor に 15 の `.contentmapper` を加え、ratchet に 9 列目、並列対照の比較にもその列。
- **compiler**：mapper の拡張子の file の宣言は `.d<拡張子>.ts`（tsgo `ChangeToDeclarationExtension`、最長の拡張子）。mapped
  file は JavaScript を出さず、宣言を出すときだけ emit の対象（tsgo `sourceFileMayBeEmitted`）。canonical の宣言は
  supplemental の宣言を参照する（tsgo `SupplementalReferencesTransformer`）。宣言 map は mapped file の位置を元の正確な位置に、
  元の名前で記録し、正確な位置の無いところは生成側だけの mapping（tsgo `MapSourcePosition` と
  `VirtualToOriginalPositionExact`）。mapper の拡張子の import は最長の mapper の拡張子を外す（tsgo `SplitExtension`。
  `./component.y.z` が `component.d.y.z.ts` に）。alias の segment の仮想名の引数は元の名前で表示（tsgo `displayMessageArgs`）、
  合成の code の「不要」の診断は落とす（tsgo `filterAndSortDiagnostics`。checker の変更）、transform の結果の `null` の list は
  空。`tsc_diagnostics::spanmap` に元から仮想への射影と `VirtualToOriginalPositionExact`。
- **README**：content mapper の節（`contentMappers` と `--runExternalCode`、宣言、実プロジェクト、未対応）、`--runExternalCode`
  の行、追従する commit（`aa814927`）、conformance の数、API server の未実装の更新、`.types`/`.symbols` の制限を削除。
- **test**：spanmap（tsgo の射影と正確な写像、不連続な境界）、protocol（`null` の list）、loader（宣言 file の多段拡張子の
  import が宣言の対を解決し tsgo の trace、mapped file は宣言のときだけ emit の対象で強制の宣言は残る。規則を外すと失敗）、
  conformance runner の宣言名（多段拡張子）、vendor の manifest の contract に `.contentmapper`。
- **結果**（release build、code `323850c7a`、ratchet `8bad12c24`、head `297bcb62e`）：clippy（9 crate）は clean、suites は 0 regressions、
  full の conformance（`--workers 2 --check`、500 s）は 0 regressions で 13,577 が新しい列で accepted tier を上回り `--update` で記録、
  #765 の report と比べて 15,324 が同じで違う 15 は content mapper の case（harness error → Full）。並列対照（`--checkers 4`、477 s）
  は記録済みの 4 構成だけ。test（xtask 以外の全 crate、`--no-fail-fast`）は 4,234 が通り、1 件（vendor の manifest の総数の pin）を
  `297bcb62e` で直した。実プロジェクトの 4 つは診断・`--explainFiles`・`--listFilesOnly` が tsgo と同じ。`--declaration
  --declarationMap --emitDeclarationOnly` の宣言と宣言 map は site（144 file、`.astro` から 29）・vitesse（46、`.vue` から 10）・
  soybean-admin（428、`.vue` から 90）で同じ、starlight は `.astro` からの 90 が同じで、content mapper 以外の 3 つが違う（JavaScript
  の `@typedef` の宣言の `export {};`、JavaScript の宣言 map 2 つの mapping、同じ位置の TS2883 の順。次の slice）。最後の test・
  clippy・実プロジェクトは夜間の設定（maintenance の QoS、`nice -n 20`、job 1、test thread 1）。
- **hosted と merge**：PR #767 の hosted run 38273271121（rust 11m21s、conformance 15m58s、gates pass、head `297bcb62e`）、merge
  commit `1a5cc6cab`。
- **残り**：C5（tsbuild の 2、build info の identity、watch の mapper の設定 file、`-b` での mapper）、C6（API）、LS だけの test
  mapper と元から仮想への射影の利用、pretty の要約の mapped file の行の順（#765 の通り）、CLI の `with_content_mapper_project` の
  doc comment の位置（`ContentMapperSession` の前に分かれている。C5 で直す）。
- **次**：starlight の宣言の 3 つ（content mapper の外）を別の PR で、その後 C5a（build info の identity と mapped file の version、
  `-b` の mapper、identity の変化での作り直し、supplemental の build info の path）、C5b（watch の mapper の manifest と設定 file）。

## starlight の宣言（PR #768）

C4b の実プロジェクトの宣言の比較（#767）で starlight に見つかった、content mapper の外の 3 つを tsgo と同じにした。

- **JavaScript の型定義は module の印**：`docs/lunaria/freshness.mjs` は拡張子で module で、`@typedef` だけを宣言する。tsgo の
  reparse された型 alias は宣言の変換の top-level の文なので、export されたものが external module の印になり
  （`transformAndReplaceLatePaintedStatements`、`IsExternalModuleIndicator`）、宣言 file に `export {};` を書かない。
- **reparse された JSDoc の宣言は tag の範囲**：tsgo の reparser は JSDoc の tag から作る node に tag の範囲を与え
  （`finishReparsedNode`）、宣言 map がそれを記録する。型 alias は `@typedef`・`@callback` の tag、dotted な名前の namespace は
  名前、型 literal は JSDoc の型 literal、property と parameter は tag、callback の関数型は signature、overload は tag 名、
  制約のある `@template` の parameter は書かれたもの。これまでは範囲が無く、`freshness.d.mts.map` と
  `packages/markdoc/html.d.mts.map` の mapping が欠けていた。
- **同じ位置の診断は message の key と引数の順**：tsgo `CompareDiagnostics` は file 名と span、code、category、source の後に、
  key（無ければ text）、引数、chain（長い方が先、各 entry の引数）、related information（多い方が先）で比べ、文字列は UTF-8 の
  byte 順。これまでは message の text の順（tsc 6.0.3 の規則）で、TS2883 は 3 番目の引数を 2 番目より前に表示するので、型の名前の
  順になっていた（tsgo は module の順）。等しさも tsgo `EqualDiagnosticsNoRelatedInfo`（key、引数、chain の code と引数）。
- **test**：tsgo の bytes の CLI contract 3 つ（`@typedef` だけの `.mjs`、reparse の全ての形の宣言と宣言 map、text と引数で順の
  変わる同じ位置の TS2883 2 つ）。診断の unit test を tsgo の規則に（6.0.3 の期待を置き換え、key → 引数の順、長い chain と多い
  related information が先）。
- **結果**（release build、head `87fd5287d`、夜間の設定）：test 4,240 が全て通る、clippy（6 crate）は clean、suites は
  0 regressions、full の conformance（`--workers 1 --check`、5,226 s）は 0 regressions で C4b の report と 15,339 の構成が全て
  同じ、並列対照（`--checkers 4`、4,952 s）は記録済みの 4 構成だけ。実プロジェクトの 4 つは診断・`--explainFiles`・
  `--listFilesOnly`・宣言と宣言 map の 16 の比較が全て tsgo と同じ（starlight は 736 file、`.astro` から 90）。
- **hosted と merge**：PR #768 の hosted run 38277733560（rust 11m17s、conformance 22m05s、gates pass、head `87fd5287d`）、merge
  commit `8f97cd0fa`。
- **次**：C5a（build info の identity と mapped file の version、`-b` の mapper、identity の変化での作り直し、supplemental の
  build info の path）、C5b（watch の mapper の manifest と設定 file）。

## C5 incremental・`tsc -b`・watch（PR #769）

C5a と C5b を一つの PR にした。content mapper が、tsgo が走らせる全ての command の形で走る。

- **incremental**：build info に `contentMapperIdentities`（program の content mapper の project の identity を、build info を
  作るときに読む。tsgo `snapshotToBuildInfo`）。読めないときは build info を書かず、emit をその失敗で飛ばす（tsgo
  `emitBuildInfo`。`noEmit` では emit の診断に入る）。mapped file の version は元の text と mapper の transform identity の
  hash（tsgo `computeProgramFileChanges`）、保存する implied format は仮想名の拡張子と、その名前で探した package の type
  （tsgo `filesParser`）。identity が今の project と違う古い build info は使わない（tsgo `ReadBuildInfoProgram`）。
- **`tsc -b`**：`--runExternalCode` のとき build の session に mapper の host を一つ（tsgo `Orchestrator.start`）、task ごとに
  project を一度作る（`getContentMapperProject`）。build の終わりに project を、次に host を閉じる。project が失敗した task は
  build せず、tsgo の `BuildErrors` で exit 1。up-to-date の検査では、identity が変わったか読めないとき「compilerOptions の
  変化」（`OutOfDateOptions`）。build info に載る supplemental の出力は、同じ名前の file が無い間は欠けた入力ではない
  （`isContentMapperSupplementalBuildInfoPath`）。
- **watch**：`--watch` は自分の host と今の設定の project を持ち、新しい project を作ってから古いものを閉じる（tsgo
  `replaceContentMapperProject`。両方が使う mapper の process は残る）。mapper の package の manifest が変わると設定を解析し
  直し、mapper が watch する file が変わると project を作り直す。`-b --watch` は設定の変わった task と graph から消えた task の
  project を閉じ、manifest の変化で task の設定を戻し、watch する file の一覧の失敗を project の失敗にする（tsgo
  `orchestrator.go`）。どちらも終わるときに mapper を閉じる。普通の command は `--watch` と `--showConfig` の分岐の後で mapper
  の project を作る。
- **test**：in-memory の System と harness の mapper で、tsgo `contentmapper_watch_test.go` の全て（build の lifecycle、disk に
  書かれた supplemental（TS18069）、読めない identity、watch と build watch の lifecycle（process (1,0) → (2,1) → (2,2) →
  (3,2) → (3,3)）、supplemental の診断の元の名前、名前の衝突、dynamic mapper の watch する設定、混在した batch、link の
  manifest の変化と削除、大文字小文字を区別しない manifest、二つの project の process の共有）。mapper の version が変わる
  incremental の `-p`（行の作り直し、保持、作り直しと build info の identity）、`TestIsContentMapperSupplementalBuildInfoPath`。
  harness に tsgo の `ProjectLifecycle`・`NewSpawnerWithProjectLifecycle`・`PackageJSON`。
- **README**：`-b` と `--watch` の content mapper。未対応は API server の content mapper。
- **結果**（code `c84685c9b`、`91ddee3cb` は rustfmt だけ、ratchet `077a9bcfd`、夜間の設定）：test 4,256 が全て通る、clippy
  （6 crate）は clean、suites は 0 regressions で tsbuild 193/193（`contentMapperIdentity` と `contentMapperOptionDiagnostics`
  が Full になり `--update` で記録）、tsc 227/229（generateTrace の 2 は以前から）。full の conformance（`--workers 1 --check`、
  5,133 s）は 0 regressions で、#768 の report と 15,339 の構成が全て同じ。checker の変更は無いので並列対照は無し。実プロジェクト
  の 4 つは診断・`--explainFiles`・`--listFilesOnly`・宣言の 16 の比較が全て tsgo と同じ。incremental の `--noEmit` の build
  info（1 回目と、それを読む 2 回目）は site と starlight が同じ。vitesse と soybean は出力、mapper の identity、file の version
  が同じで、`missingPackageJsons` の順だけが違う（soybean は重複が 8）。Vue の mapper の
  `/// <reference types="../../node_modules/…" />` の package.json の探索で、tsgo は type root からの候補を正規化したパスにし
  （`resolutionCandidateFromDirectoryPath`）、こちらは TS 6 の `combinePaths` のまま `..` を残していた。resolver の差なので
  次の PR。Vue の mapper は dynamic config で identity が compiler option を含むので、比べるには両方に同じ
  `--tsBuildInfoFile` を渡す。
- **hosted と merge**：PR #769 の hosted run 38319585066（rust 11m33s、conformance 20m13s、gates pass、head `077a9bcfd`）、merge
  commit `8dff0f4bc`。
- **残り**：type root の候補の正規化（次の PR）、C6（API。下の計画）、LS だけの test mapper と元から仮想への射影の利用、pretty
  の要約の mapped file の行の順（#765 の通り）。
- **次**：type root の候補の正規化、その後 C6。

## C6 の計画（API）

tsgo の API server は `--runExternalCode` のときだけ content mapper を走らせる（`cmd/tsc/api.go` の flag、
`StdioServerOptions.RunExternalCode` と `ContentMapperSpawner`、project の `SessionOptions.RunExternalCode` と
`SessionInit.Spawner`）。API の build orchestrator は process を起こさない（`apiBuildSystem.Spawn` が断る）。

- **C6a（API の層）**：
  - encoder（`recordExtendedData_SourceFile`）が mapped file の欄を書く：`originalText`（text と違うときだけ別の文字列）、
    `spanMap`（segment ごとに仮想の start と長さ、元の start と長さ、kind、features が全部でなければ features の 5 か 6 の組）、
    `supplementalSourceFileNames`、`canonicalSourceFileName`、`contentMapper`、`virtualFileName`、`diagnosticDirectives`（元と
    仮想の start と長さ、policy、unused code の 6 の組）。文字列表への追加の順（text、originalText、fileName、path、…、
    canonical、contentMapper、virtualFileName）と structured data の順も tsgo と同じにする。
  - mapped file の symbol は snapshot の持ち物（tsgo `symbolOwnerFile` が mapped file に nil を返す）。
  - test：tsgo `TestEncodeContentMapperSourceFileMetadata`、`TestContentMappedSymbolsAreSnapshotOwned`。`toAPITextEdits` の元の
    座標（`TestToAPITextEditsUsesOriginalCoordinates`）は、それを使う import の request が LS の作業なので、その時に。
- **C6b（project system）**：
  - `tsc-rs --api --runExternalCode`。session の mapper host は `--runExternalCode` で spawner があるときだけ作る（tsgo
    `newContentMapperHost`）。
  - program を作るときに、config に content mapper があれば host から project を作って plan に付ける（tsgo の compiler host の
    `ContentMapperProject`）。program がどの snapshot からも参照されなくなったら閉じ（tsgo `Snapshot.dispose`）、session を
    閉じるときに host も閉じる。
  - API の module resolution は mapper の拡張子を `ExtraExtensions` に入れる（tsgo `module_resolution.go`）。
  - API の build は mapper の process を起こさない（tsgo `apiBuildSystem.Spawn` の文言）。
  - test：mapped file を持つ project を API で開き、encode された source file の欄、診断、symbol を確かめる（harness の
    mapper で）。
- **LS の作業に回すもの**：client の contribution と inferred project の mapper、watch の mapper の file、`toAPITextEdits`、LS
  だけの test mapper。
