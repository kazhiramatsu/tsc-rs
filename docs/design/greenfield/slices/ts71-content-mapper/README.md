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
