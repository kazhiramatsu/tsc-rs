# TypeScript 7.1 の API server：計画（2026-10-09）

tsgo の `--api`（`tsc/internal/api`）を tsc-rs に移す計画。[post-emitter roadmap](../../post-emitter-roadmap.md) の P5「公開
API」を、vendoring の commit（`19dadef8`）の source と test で具体化する。P4-7d で source file の encoder（`tsc_api`）を移した。
API server は tsgo の project system（LSP の土台）の上にあり、利用者の「LSP と content mapper は今は除く」という指示と
重なるので、下の「決めること」を利用者に尋ねた（2026-10-09、下の「決定」）。

## tsgo の API server の構成

| 部分 | Go（行） | 内容 |
| --- | --- | --- |
| transport | `api/server.go`（139）、`protocol_msgpack.go`（281）、`ipc`（989） | stdio の同期 MessagePack（既定）、`--async` の JSON-RPC、named pipe／Unix socket |
| session | `api/session.go`（5,118） | 154 の request の handler、snapshot の保持と解放、symbol・type・signature の handle の登録 |
| protocol | `api/proto.go`（2,044）、`enum_values_generated.go`（1,020） | request／response の型、client に渡す enum の値 |
| file system | `callbackfs.go`（243）、`requestfilesystem/`（3 file） | client への callback で読む FS、request ごとの FS の差分（`updateSnapshot` の file changes） |
| module resolver | `module_resolution.go`（401） | `createModuleResolver`／`resolveModuleName` |
| encoder／decoder | `api/encoder`（P4-7d で encoder を移した） | source file と node の binary 形式。decoder は `printNode` と `formatNodeForInsertion` が使う |
| project system | `project`（39 file、12,966） | snapshot（clone と file changes）、configured／synthetic program、program の所有、LSP の session との共有 |
| language service | `ls`（41,570）、`format`（4,257）、`autoimport` | 6 handler だけ（completion、documentation comment、JSDoc tag、import adder の edit、node の format、getTypeOfSymbol の一部） |
| その他 | `astnav`（794）、`checker`、`nodebuilder`、`printer`、`transpile`、`tsoptions`、`execute/build` | position → node、checker の query、typeToTypeNode、印字、transpile、config、build orchestrator |

request は 170 種（`proto.go` の `Method*`）：snapshot と project（`createSnapshot`／`updateSnapshot`／`getDefaultProjectForFile`／
`getCurrentLanguageServerSnapshot`）、config と command line、`createSourceFile`／`transpile*`、build orchestrator（`build`・
`clean`）、module resolver、source file と program の情報、checker の query（symbol・type・signature の 100 前後）、
`printNode`・`typeToTypeNode`、LS の 6 種。

## 一致の基準

- **Go の test**：`api` の session test 8 file（67 test：snapshot と program 9、LS の snapshot との共有 15、batch 8、completion 4、
  module resolution 6、request file system 23、createSourceFile 1、text edit 1）、`proto_test`・`server_test`・
  `jsonvalue_test`、`requestfilesystem` の 3 file、encoder／decoder の 3 file。Rust の test として移す。
- **TypeScript の client の test**：`packages/typescript/test/{sync,async}/api.test.ts`（327＋335 test）と
  `sync/api-generators.test.ts`（42）。client は `<tsc> --api [--async] --cwd <dir>` を起動し、実行 file の path は
  option で変えられる。tsc-rs の `--api` に向けて走らせれば、同じ client が同じ結果を得ることを直接確かめられる（`node --test`。
  CI では Node と `npm ci` が要る）。

## 依存と順序

1. **project system**（tsgo `project`）：API の snapshot は project の session（`Session.APIUpdate`、`CloneSnapshot`）が作る。
   configured project、synthetic program、file の overlay、parse の cache、checker の pool、program の所有を移す。roadmap の L2
   （LSP の土台）にあたる。これが無いと snapshot の request と Go の session test の大半を移せない。
2. **transport と session の骨格**：同期 MessagePack と JSON-RPC、batch、handle の解放、error の形。
3. **project を要らない request**：`parseCommandLine`・config の読み込み、`createSourceFile`、`transpile*`、module resolver。
4. **checker の query**：symbol・type・signature の handle（tsgo の id と同じく snapshot ごとの登録）、position からの node
   （astnav）、`typeToTypeNode`（node builder）。port の checker が `.types`／`.symbols` のために持つ query を公開の形にする。
5. **decoder と印字**：`printNode`（P4-7d で後回しにした decoder）。
6. **build orchestrator**：`createBuildOrchestrator`／`build`／`clean`（P3-6d の orchestrator を使う）。
7. **LS の 6 handler**：LSP の作業に回す（それまでは tsgo と同じ request に未実装の error を返す）。

## 決めること

- **project system を今移すか**：API server の前提で、LSP の土台でもある。「LSP は今は除く」の範囲に入るかどうか。移すなら
  API server（1〜6）を順に進め、LS の 6 handler と LSP 本体は後にする。移さないなら API server は project system の後に回す。
- **client の test を CI に入れるか**：hosted job に Node と `npm ci`（network）を足す。入れない場合は local の確認と Go の test の
  移植を基準にする。
- **WebAssembly**：session を transport から分けておけば、WASM の build で JS から同じ API を process を立てずに呼べる
  （tsgo に WASM の target は無い）。WASM の build そのものは利用者が別に計画する。

## 決定（2026-10-09）

- **project system から移す**：project system（依存と順序の 1）を移し、API server（2〜6）を順に進める。LS の 6 handler と LSP
  本体は後に残す（「LSP は今は除く」の範囲はそのまま）。
- **client の test は今は local の確認だけ**：tsgo の TypeScript client の test は local で tsc-rs の `--api` に向けて走らせて
  確かめ、merge の基準は Go の session test の移植と Rust の test にする。hosted job は変えない。
- この決定の前に、resolver の残り（削除済みの `node10`／`classic`／`baseUrl`、trace の順序と implied format）を先に片付けた
  （[ts71-suites](../ts71-suites/README.md) の 2 つの slice、#717・#718）。

## P5-1 project system の計画（2026-10-09）

### tsgo の構成（API server が使う部分）

- standalone の API（`api/session.go` `NewStandaloneSession`）は `SnapshotHost` だけを使う：`NewRootSnapshot`、
  `CloneSnapshot(base, FileChangeSummary, APISnapshotRequest)`、`AcquireSourceFile`、`Close`。LSP の `Session`（overlay、debounce、
  watch、ATA、auto-import、content mapper、push diagnostics、telemetry）は使わない。
- snapshot：`SnapshotFS`（host の FS か API の FS を層にし、snapshot ごとの cache）、`ProjectCollection`（configured：config の path、
  synthetic：`/dev/null/synthetic/N`、inferred：1 つ）、`ConfigFileRegistry`（config の entry と保持者、extended config の owner 付き
  cache）、ref count、program counter。
- `Snapshot.Clone`：file の変更で project を dirty にし（config の変更は reload、program の 1 file の変更は `UpdateProgram` で
  program を cloned に、他は作り直し）、API の request を順に適用する（close projects → open projects → close／open files →
  synthetic の create／reconfigure／remove → ensure）。default project は tsconfig／jsconfig を上に探し、参照を BFS で辿る。不要な
  configured project は後片付けで消す。program は求められたときだけ作る。
- parse cache：content の hash と parse options を key に、同じ `*ast.SourceFile` を program と snapshot の間で共有する（ref
  count）。checker pool：program ごとに diagnostics 用 1、query 用 N−1、API 用 1。module resolution の cache は program ごと。

### port の対応

- 新しい crate `crates/project`（`tsc-rs-project`／`tsc_project`）：`SnapshotHost`、`Snapshot`、`SnapshotFS`、
  `ConfigFileRegistry`、`Project`、`ProjectCollection` とその builder。
- program は既存の loader（`tsc_program::load_program`）と config の parse（`parse_config_root_plan`）を snapshot の FS の host で
  使う。
- **live program**：今の checker は scoped な callback（`CheckerSession<'program>` が snapshot を借りる）でしか使えない。prepared
  program、bind 済みの document、option、module provider を持つ owner と、それを借りる `CheckerSession` を 1 つの値にする
  （self-referential。`self_cell` か小さな内部の helper）。全 file を先に検査せず、query と file ごとの診断を遅延で行う schedule を
  加える。WASM の build（thread が無い）でも動くよう、program ごとの thread は使わない。
- parse cache：`ParsedDocument`（`Arc`）は program の間で共有できる。bind は symbol の id を program の順で予約するので、program
  ごとにやり直す（tsgo は bind 済みの file を共有する。結果は同じで cost が違う）。

### slice

- **P5-1a live program**：owned な program と API の checker、遅延の query と file ごとの診断（batch の結果と同じであること）。
- **P5-1b project の核**：crate、`SnapshotFS`、`ConfigFileRegistry`（extended config cache）、`Project`（configured／synthetic／
  inferred）、`ProjectCollection` の builder（API の request、file の変更、default project、後片付け）、`Snapshot` の `Clone` と
  ref count。
- **P5-1c** parse cache と program の更新の種類（`UpdateProgram` の再利用、`SameFileNames`／`NewFiles`）。
- **P5-1d** checker pool（diagnostics 用・query 用・API 用、global diagnostics の蓄積）。
- **P5-1e** `SnapshotFS` の細部（大量の変更、realpath の alias、cache の掃除）。

### test

- tsgo の `internal/project` の test のうち API の経路で通るもの（`snapshot_test`、`refcountcache_test`、
  `extendedconfigcache_test`、`configfilechanges_test`、`projectreferencesprogram_test`、`projectcollectiondefaultproject_test`、
  `project_test` の update kind、`checkerpool_test`、`snapshotfs_test`、`bulkcache_test`、`dirty` の test）を移す。
- LSP の open file で進む test（`session_test`、`projectlifetime_test`、`projectcollectionbuilder_test`）は API の open と file の
  変更に置き換える。期待値は tsgo の `SnapshotHost` に同じ操作をする Go の probe で確かめる（API で開いた file は祖先の solution
  を探さないなど、LSP の open と違うところがある）。
- LSP・LS・content mapper・ATA・watch・push diagnostics・preference の test は移さない。

## P5-1a live program（2026-10-09）

P5-1 の最初の slice。tsgo の API は project の `compiler.Program` と、その checker pool の API 用の checker で request に答え、
Program を request の間保つ。port の batch の driver は checker を scoped な callback にしか渡さない（checker は Program の
snapshot、option、module provider を借りる）。

- **`LiveChecker`**（`crates/checker/src/live.rs`）：checker が借りる値（snapshot、option、module provider、metadata、host facts、
  構文の診断）を持つ owner と、それを借りる `CheckerState` を 1 つの値にした（`self_cell`。workspace の依存に加えた）。作り方は
  batch の on-demand の driver と同じ（lib の bundle、parse、Program 順の bind、source を 1 つも検査しない checker の初期化）。
  file の semantic／suggestion の診断を初めて求められたときにその file を検査し、その検査が出した global の行をその file に
  帰属させる（getDiagnosticsWorker）。`with_checker` が query を走らせる。thread は使わない（WASM の build のため）。
- **`LiveProgram`**（`crates/compiler/src/live.rs`、`tsc_compiler::LiveProgram`）：`Arc<PreparedProgram>` と `LiveChecker`。module
  provider は batch の session と同じもの（借りた Program と持つ Program の両方で使えるよう generic にした）。Program の getter：
  config file parsing、program（tsgo `GetProgramDiagnostics`：preparation の options の行、programmatic な option の行、source の
  外の program の行、emit する Program の出力 path の行。content mapper の行は port していない）、syntactic、semantic、
  suggestion、global。README の Rust API の節に加えた。
- **live check で見つかった差と修正**：どちらも checker の module specifier の host。tsgo の node builder の host は常に Program で
  （`nodebuilder.go:285`）、表示する型（`.types`、エラーの文、API の typeToString）は emit の有無に依らず同じ specifier を使う。
  port には checker の host（`BasicModuleSpecifierHost`）と宣言の emitter の host があり、2 つは共有の `specifierCache` を埋めるので、
  先に計算した方の答えが残る。batch の walk が一致していたのは、emitter の host を渡す宣言の診断が先に cache を埋めていたから。
  - **symlink**：tsgo の Program は `GetSymlinkCache` で symlink を知り、link の名前を書く（`import("package-a").Foo`）。checker の
    host は symlink を返さず、live の Program は `import("../packageA").Foo` と書いた。`AuthoritativeModuleProvider` に
    `symlink_facts`（tsgo `GetSymlinkCache`）を加え、prepared の provider は emit の host と同じ `discover_symlink_facts` を返し、
    checker の host がそれを報告する。7 構成（`declarationEmitReexportedSymlinkReference2`・`3`、
    `declarationEmitSubpathImportsReexport`、`symlinkedWorkspaceDependenciesNoDirectLink` の 4 つ）が一致し、
    `symlinkedWorkspaceDependenciesNoDirectLinkGeneratesDeepNonrelativeName` の `.types` が tsgo と一致した（none → full）。
  - **既存の import の再利用**：tsgo 7.1 の `computeModuleSpecifiers`（`specifiers.go:376-404`）は、module path ごとに、その
    module に解決される file の**最初の** import だけを候補にし、その usage の mode（`GetModeForUsageLocation`：type-only の import
    と import type の `resolution-mode`、`require`／import equals の CommonJS、import call、他は file の emit の構文）が生成する
    mode と違えば使わず次の module path に進む。port は 6.0 のまま、mode の合う import を全て探し、mode は host の答え（checker の
    host は index に関係なく file の既定の mode、emitter の host は `None`）か、`None` のときは import call かどうかだけで決めた。
    checker の host では CommonJS の file が自分の package の `import("package/cjs")`（ESM の mode）を使い（tsgo は
    `"./index.cjs"`）、live の Program が batch と違った。checker の host を emitter の host と同じ `None` にしただけでは、
    `resolution-mode` 付きの type-only の import や import type の 19 構成の `.types` が full から落ちた（`"./module.mts"` を使う。
    tsgo は最初の import が CommonJS の mode なので使わず `"./module.mjs"`。前は checker の host の既定の mode で偶然一致していた）。
    tsgo の規則にした：最初に解決される import だけを見て、usage の mode は checker の `resolution_mode_for_usage`（
    `GetModeForUsageLocation` の port。checker の module 解決が使うもの）で取り、provider への解決の request もその mode で引く。
    2 つの host は index の mode を返さない（`None`）。`nodeModulesDeclarationEmitDynamicImportWithPackageExports` の 3 構成の
    `.symbols` が tsgo と一致し（none → full。`.types` は emit の後の walk で既に一致していた）、19 構成は full のまま。
- **file 単位の診断の順序**：file の semantic の行は、その file を聞いたときに checker が持つ行で（tsgo の file 単位の
  `GetSemanticDiagnostics` → `Checker.GetDiagnostics`）、後の file の検査が前の file に行を足すことがある（lib と merge する global
  の interface は、最初の宣言のある lib の検査で検査される）。tsgo の全体の取得は全ての file を検査してから集める。`LiveProgram` は
  tsgo の file 単位の挙動のとおりで、`duplicateNumericIndexers` と `objectTypeHidingMembersOfExtendedObject` は source → lib の順に
  1 回ずつ聞くと、lib の検査が source に足した行が入らない。runner の live check は全ての file を検査してから聞くようにした。
- **tests**（`crates/compiler/tests/integration/live_program_contract.rs`、7 件）：6 つの Program で、live の診断（file ごと、全ての
  種類）の和が batch の native harness の和と等しい（batch の outcome の getter は getPreEmitDiagnostics のように前の種類の行が
  あると後の種類を返さないので和で比べ、構文の行の無い Program では program と global の行も比べる）。type と symbol の walk が
  batch の walk と等しい（検査の前と後）。`LiveProgram` は `Send`。emit 無しの query が tsgo の `.types` の行になる 3 件：symlink の
  workspace（修正を外すと `"../packageA"`）、CommonJS の file から自分の package の dynamic import（checker の host が既定の mode を
  返すと `"package/cjs"`）、`typeOnlyESMImportFromCJS`（mode の合う import を全て探す前の規則と `None` の host では
  `"./module.mts"`）。いずれも失敗することを確かめた。lib と merge する interface の行は、lib を検査した後に聞き直すと返る（tsgo の
  file 単位の挙動）。
- **runner の live check**：`TSRS_LIVE_CHECK=1|verbose` で、conformance の 1 checker の各構成について `LiveProgram` を batch の
  結果と比べる（全ての file を batch の順で検査してから聞いた診断の和から宣言の診断の行を除いたもの、type と symbol の walk。
  emit する構成では最初の Program の walk を追加で取る。batch の結果は変えない）。違いは shard の stderr に `live mismatch` と
  書く（walk は最初の違う行も）。1 つの case で Program を 2 つ持つので、worker の常駐 memory が 3,072 MiB の上限を超えて新しい
  worker で case をやり直すことが増える（結果は同じ）。
- **検証**（最終 bytes：修正 `9eb9861ec`、ratchet `19bd8151c`。macOS、`nice -n 20`、Cargo の job 2）：
  `cargo fmt --all -- --check`、Clippy（checker・compiler・conformance、`--all-targets -- -D warnings`）は clean。`cargo test -p`：
  checker 1,796、compiler 459（live の契約 test 7 件を含む）、conformance 52、全て成功（workspace 全体の test と Clippy は hosted の
  `rust` job）。`TSRS_LIVE_CHECK=verbose scripts/conformance_ts71.py --workers 2 --check`：12,748 case を 703 s、errors full
  13,451／mismatch 0、emit full 13,443、types 12,678／89（12,677／90 から）、symbols 12,718／49（12,715／52 から）、sourcemap 13,451、
  trace 13,451、harness error 15、regression 0、4 構成が tier を上回った。live check は比べた全ての構成（13,451、walk は 12,767）で
  違い 0（2 つの Program の memory で worker を 93 回やり直し、34 構成は 2 度比べた）。`--update`（2 つの case の filter）で ratchet
  の 4 行を上げた（上の `.types` 1 行と `.symbols` 3 行）。途中の実行：WIP の全体の実行（7,940 構成で止まった）で symlink の
  7 構成、`b6f2f6732` の全体の実行で 5 構成（既存の import の再利用 3、file 単位の順序 2）、`f3d7a1ffc` の全体の実行で
  regression 19（既存の import の再利用）。並列対照（`--checkers 4`、464 s、`scripts/conformance_ts71_compare.py`）：15,224 構成が
  一致、違う 4 構成は記録済みの partition 依存の構成（`mutuallyRecursiveInference`、`incorrectRecursiveMappedTypeConstraint`、
  `typeParameterWithInvalidConstraintType`、`recursiveMappedTypes`）。`scripts/suites_ts71.py --check`：全ての suite が変わらず（api 2、
  config 87、transpile 41、tsbuild 182／192、tsbuildWatch 63／65、tsc 211／223、tscWatch 42、tsoptions 80）、regression 0。
  実 project と性能は計測していない（利用者の指示）。
- **残り**：checker pool（P5-1d）、bind の診断だけを返す getter と宣言の診断の getter（API の request の slice）、project の核
  （P5-1b）。specifier の生成で tsgo と違う所が 2 つ残る（2 つの host の答えは同じなので live check には現れない。`.types` の
  残りの不一致との関係は調べていない）：囲む宣言の module specifier の mode（tsgo はその usage の mode、port は file の既定の
  mode）、ending の推定（tsgo 7.1 は node の解決で CommonJS の mode を生成するとき相対の import を全て飛ばす。port は ESM の mode の
  import だけを飛ばす 6.0 の形で、host が mode を返さないので飛ばさない）。6.0 の include reason の経路（host が reason を返すとき）
  は残っているが、reason を返す host は無い。
