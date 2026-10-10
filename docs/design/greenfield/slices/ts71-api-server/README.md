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

### P5-1a の hosted の記録と merge（2026-10-09）

- hosted：最終候補 `771463d14`（修正 `9eb9861ec`・ratchet `19bd8151c`・packet の記録）の run 37924497777（`plan` 39s、`rust` 11m34s、
  `conformance (TypeScript 7.1)` 16m23s、`gates` 18s。全て成功）。merge → `ec4dd6c34`（merge commit、PR #719）。
- 次：P5-1b（project の核）。

## P5-1b project の核の設計（2026-10-09）

### tsgo の API の経路（`19dadef8` で読み、Go の probe で確かめた）

- standalone の API の session は `SnapshotHost` の `NewRootSnapshot`、`CloneSnapshot(base, FileChangeSummary, APISnapshotRequest)`、
  `Snapshot.Deref`、`AcquireSourceFile` だけを使う。LSP の overlay は無く（全ての snapshot の overlay が空）、LS の resource request
  も空。
- `Snapshot.Clone`（`snapshot.go:431-727`）：base の FS を決め（request の FS があればそれ。全置換なら全ての cache を無効にする）、
  file の変更を処理し（§FS）、builder で `DidChangeFiles`（変更が空でなければ）→ `HandleAPIRequest` → `Finalize`。何も変わらな
  ければ base の project collection と config registry をそのまま使う。新しい snapshot の id は host の counter（root は 0）。
  `HandleAPIRequest` が失敗した snapshot は登録されず、clone もされない。
- `HandleAPIRequest`（`projectcollectionbuilder.go:186-377`）の順：closeProjects を数える → openProjects（configured project を
  作るか探し、program を更新し、open の数を足す。同じ request の close は取り消す）→ closeFiles／openFiles を数える → close
  する project を削除 → 新しく開いた file の置き場（default project の探索。見つからず inferred に置けなければ
  `no project found for opened file: %s`。後片付け）→ reconfigure／remove の検証 → remove → create（synthetic の id は空いて
  いる最小の `/dev/null/synthetic/<n>`）→ 作った／変えた synthetic program の更新 → ensureFiles → ensurePrograms →
  ensureAll → program の module 解決のエラー。
- program を作り直すのは request の open／ensure／create／reconfigure だけで、file の変更の通知は project を dirty にする
  だけ（dirty は次の更新まで残る）。configured project の program は config の `ParsedCommandLine` から作り、config の
  診断があっても作る（`GetConfigFileParsingDiagnostics` で返す）。
- tsgo の癖（どれも probe で確かめた）：API で開いた inferred の file は root が 2 回入る（`[a, a]`、同じ build で program を
  2 回作る）。API で開いた file には祖先の（solution の）tsconfig を探さない。存在しない config の openProjects はエラーに
  ならず project も残らない。closeProjects は API で開いた file の default project でも消す。ensurePrograms は project を
  作らない。file の default project の記録はその build だけで、後の snapshot は program に含まれるかで探す。同じ request の
  remove と create、tsconfig に含まれない file の開き直しは、tsgo では nil の参照で server が落ちる。

### port の方針（利用者の指示：既存の動作は変えてよい、余計な処理を足さない）

- 新しい crate `crates/project`（`tsc-rs-project`／`tsc_project`）は、上の経路の `SnapshotHost`、`Snapshot`、`ProjectCollection`
  とその builder、`Project`、`ConfigFileRegistry`、`SnapshotFs` だけを持つ。LSP の overlay、ATA、auto-import、content mapper、
  log、watch、preference、client への progress は移さない。
- FS は `tsc_host::vfs::FileSystem`（tsgo の `vfs.FS`）。`SnapshotFs` はその上に、program が読んだ file の cache（内容と
  hash）を持ち、`VfsCompilerHost` を通して既存の loader（`load_config_program`／`load_emitting_config_program`、
  `load_program`）と config の解析に渡す。project system 用の読み込みの経路は作らない。
- tsgo の `dirty` package（並行の copy-on-write の map）は移さない。snapshot の map は `Arc<BTreeMap>` で、builder は変える
  ときだけ複製する。変わらない project は同じ `Arc` のまま（API の応答の差分は pointer の同一性で、tsgo と同じ）。普通の
  map なので同じ build で消した key を作り直せ、tsgo の 2 つの crash は起きない。
- thread は使わない（WASM）。tsgo の default project の BFS は level ごとに最小の index の結果を選ぶので、順に辿っても同じ
  結果になる。
- 寿命は `Arc`。tsgo の program の数え（`programCounter`）と parse cache の参照の数えは P5-1c（parse cache）で扱う。
- project の program は `LiveProgram`（P5-1a）。更新の種類は P5-1b では NewFiles と SameFileNames（file 名が同じ）だけで、
  1 file の変更で program を再利用する Cloned は P5-1c。
- config の entry は自分の extends の path を持ち（tsgo の `retainingConfigs`）、extends の file の変更で拡張する config を
  reload する。tsgo の extended config の cache（snapshot の id で所有を数える共有の cache）は移さず、reload ごとに既存の
  `ConfigExtendedCache` で解析する。
- tsgo の癖は手順どおりに移して同じにする（root の重複も含む）。

### slice

- **P5-1b-1**：crate、ID（`/dev/null/inferred`、`/dev/null/synthetic/<n>` の解析と正規化、他は configured）、`SnapshotHost`／
  `Snapshot`、`SnapshotFs`（cache、changed／created／deleted と全ての無効化）、`ConfigFileRegistry`（acquire／release、reload の
  印、extends、cleanup）、synthetic program（create／reconfigure／remove と検証）、openProjects／closeProjects、
  ensurePrograms／ensureAll、file の変更（`DidChangeFiles`：dirty と config の reload の印）、`Finalize`。test：tsgo の
  `TestSnapshot`（synthetic program の作成と削除、失敗した update、存在しない file）、`TestProjectIDNarrowing`、
  `session_createprogram_test.go`（snapshot の層で）、`configfilechanges_test.go` と `project_test.go` の NewFiles／SameFileNames
  （open／changed／ensure に書き換え、期待値は tsgo の `SnapshotHost` の Go probe で確かめる）。
- **P5-1b-2**：openFiles／closeFiles／ensureFiles：default project の探索（祖先の tsconfig／jsconfig、参照の BFS、file の無い
  config、composite の早い否定、`disableReferencedProjectLoad`／`disableSolutionSearching`）、後片付け（registry の cleanup を
  含む）、inferred project（既定の option）、`GetDefaultProject`。test：default project の case（API の open に書き換え）、
  `TestSnapshot` の synthetic program の test の開いた file の部分。
- **P5-1b-3**：project system の project 参照：参照先の config を registry から取ること（tsgo `GetResolvedProjectReference`。
  参照先の変更で参照する project を dirty にする）と、参照先の source を使う program（tsgo `UseSourceOfProjectReference`：出力の
  d.ts の読み込みを source に redirect し、まだ build されていない d.ts を在るものとして解決し、参照先の file にはその option を
  使う。今の loader は CLI の出力への redirect だけを持つ）。test：`projectreferencesprogram_test.go`（API の open に書き換え）。

## P5-1b-1 snapshot、configured project、synthetic program（2026-10-09）

- **crate**：`crates/project`（`tsc-rs-project`／`tsc_project`）。`SnapshotHost`（host の FS、session の option、snapshot の id）、
  `Snapshot`（id、親の id、project、config、読んだ file、request の FS か）、`ProjectId`（tsgo の ID の解析と正規化）、`Project`
  （configured／synthetic、command line、program、dirty、更新の種類）、`ProjectProgram`（prepared program と `LiveProgram`。
  checker は一度に 1 つの呼び出しが使う）。
- **`clone_snapshot`**（tsgo `Snapshot.Clone` の API の経路）：base の FS を決め（request の FS。全置換と、request の FS から
  host の FS に戻るときは全てを無効にする）、file の変更を処理し、`DidChangeFiles` → `HandleAPIRequest`（openProjects／
  closeProjects、synthetic program の create／reconfigure／remove とその検証、ensurePrograms／ensureAll）→ 確定（何も変わらな
  ければ base の collection を使う）。失敗した request はエラーだけを返す（tsgo はエラー付きの snapshot を返し、API が捨てる）。
- **FS**：`SnapshotFs` は base の FS の上に、program と config が読んだ file を持つ。変更の通知で、cache した file の内容が同じ
  なら変更としない、違えば読み直す、消えていれば落とす。削除されたディレクトリはその下の cache した file に広げ、他の削除と
  変更は関係する拡張子だけを数える（tsgo `expandAndFilterWatchEvents`）。1,000 を超える変更は、cache と重なれば cache を全て
  無効にし、重ならなければ changed／deleted を捨てる。program の build は見た file と無かったディレクトリを記録し（tsgo
  `seenFiles`／`missingDirectories`）、作られた file や変わった file で program が dirty になるかに使う。
- **config**：tsgo の registry（entry、reload の印、保持する project、拡張元の config）。config か、それが拡張する config の
  変更で Full の reload、wildcard で入った root の削除と、include に合いそうな file の作成で FileNames の reload の印を付ける。
  `PossiblyMatchesFileName`／`PossiblyMatchesDirectoryName` は watch の private な実装を `ConfigRootPlan` に移し、watch も
  それを使う。FileNames の reload は parse（エラーも）を保って root だけを入れ替える（tsgo `ReloadFileNamesOfParsedCommandLine`、
  watch と同じ `with_reloaded_file_names`）。
- **program**：configured project は CLI と同じ `load_config_program`／`load_emitting_config_program`（`noEmit` で選ぶ）、
  synthetic program は `load_program`／`load_emitting_program`。更新の種類は NewFiles と SameFileNames（tsgo `HasSameFileNames`）。
- **tsgo と違う所**：同じ request の remove と create は、消した番号を新しい program に使う（tsgo は nil の参照で落ちる）。
- **LiveProgram の修正**：file の無い program（root も lib も無い synthetic program）で `LiveChecker` が panic した（snapshot の
  作成が `EmptyProgram`）。バッチの driver と同じく、file の無い program は checker を持たない（診断は空と、lib の無い global の
  型の行。`with_checker` は `None` を返す）。
- **片付け（利用者の指示：余計な処理を残さない）**：config の診断で読み込みを止める古いゲート（`validate_config_plan` と
  `ConfigProgramLoadError::Diagnostics`。loader はもう使っていない）と、それを受けていた CLI と example の分岐を削除し、
  `load_config_program` の古い説明を直した。
- **test**（`crates/project/tests/snapshot.rs`、19 件）：tsgo の `TestSnapshot`（synthetic program の作成と削除、失敗した更新、
  存在しない file、"no-op watch change does not rebuild program"）、`TestProjectIDNarrowing`、`session_createprogram_test.go` の
  snapshot の層の分（独立した root の番号、作った program の root と option、再構成、検証の文、
  `TestUpdateSnapshotEnsuresSyntheticProgram`）、`TestSnapshotUpdateCarriesHostFileSystemWithoutOverride`、configfilechanges／
  project_test を open／changed／ensure に書き換えたもの（option の変更と SameFileNames、2 段の extends、root の追加と
  NewFiles、close と同じ request の open、存在しない config）、同じ request の remove と create、`Send + Sync`。LiveProgram の契約
  test に file の無い program を加えた（8 件）。期待値は tsgo の test と、P5-1b の設計の調べ（Go の probe で確かめた挙動）による。
- **検証**（最終 bytes `f435a39ad`。macOS、`nice -n 20`、Cargo の job 2）：`cargo fmt --all -- --check`、Clippy（program・checker・
  compiler・conformance・emitter・harness・incremental・project、`--all-targets -- -D warnings`）は clean。`cargo test -p`：program
  599、checker 1,796、compiler 460、conformance 52、emitter 635、harness 31、incremental 29、project 19、全て成功（workspace 全体の
  test と Clippy は hosted の `rust` job）。`TSRS_LIVE_CHECK=1 scripts/conformance_ts71.py --workers 2 --check`：12,748 case を
  715 s、数は main と同じ（errors full 13,451／mismatch 0、emit full 13,443、types 12,678／89、symbols 12,718／49、sourcemap
  13,451、trace 13,451、harness error 15）、regression 0、live check の違い 0。`scripts/suites_ts71.py --check`：全ての suite が
  変わらず（api 2、config 87、transpile 41、tsbuild 182／192、tsbuildWatch 63／65、tsc 211／223、tscWatch 42、tsoptions 80）、
  regression 0（watch の一致判定の移動を含む）。並列対照は走らせていない（checker の変更は `LiveChecker` だけで、batch の検査は
  変わらない）。実 project と性能は計測していない（利用者の指示）。
- **残り**：P5-1b-2、P5-1b-3（上）。P5-1c〜P5-1e（parse cache と Cloned、checker pool、FS の細部：大量の変更の node_modules の扱い、
  realpath の alias、cache の掃除、cache した file のディレクトリの一覧への合成）。

### P5-1b-1 の hosted の記録と merge（2026-10-09）

- hosted：最終候補 `32035ebe4`（コード `f435a39ad`・packet の記録）の run 37934606585（`plan` 35s、`rust` 11m12s、
  `conformance (TypeScript 7.1)` 12m54s、`gates` 16s。全て成功）。merge → `06fdab449`（merge commit、PR #720）。
- 次：P5-1b-2（API で開く file、default project、inferred project、registry の cleanup）。

## P5-1b-2 API で開く file、default project、inferred project（2026-10-09）

- **request**：`openFiles`（開く file。空の集合でも後片付けを求める）、`closeFiles`、`ensureFiles`（API は開く file を全て ensure
  する）。API の open 状態に file の open の数を持つ（tsgo `apiOpenedFile`）。
- **default project の探索**（tsgo `findOrCreateDefaultConfiguredProjectForFile` とその worker）：この build で見つけたもの、
  なければ file のディレクトリから上の一番近い `tsconfig.json`／`jsconfig.json` から project 参照を辿る breadth-first の探索。
  config を acquire し（file から acquire した config は、project が保持しないかぎり後片付けで消える）、root の無い config
  （solution）は参照に進むだけ、composite の config は列挙しない file に否定を返し、project を作って program を作ってから file を
  持つかを見る。tsgo は level の config を並行に訪れて file を持つ最小の index を取り、順に訪れても同じものを得る。API で開いた
  file には祖先の（solution の）探索が無い（tsgo と同じ）。参照先の source は P5-1b-3 なので、今は全ての inclusion が direct。
- **後片付け**（tsgo `cleanupConfiguredProjects`）：API で開いた file の default project、探索の道の config、API で開いた project
  の、どれでもない configured project を削除し、configured project の無い開いた file を inferred project の root にし、registry
  の保持されない entry を消す。`openFiles` があるとき（空でも）と、`closeFiles` だけのときに走る。
- **inferred project**（tsgo `NewInferredProject` の既定の option）：root（並べ替え）が変われば command line を変え、無くなれば
  消す。program は open／ensure／ensurePrograms で作る。tsgo の癖も同じ：open と ensure の両方を受けた file は root に 2 回入る
  （`[a, a]`、同じ build で program を 2 回作る）。
- **`Snapshot::default_project`**（tsgo `GetDefaultProject`）：build が見つけた default project、なければ file を持つ最初の
  configured project（ID 順）、なければ file を持つ inferred project。`Projects()` と `GetProject` に inferred project を加えた。
- **tsgo と違う所**：一番近い tsconfig に含まれない file を開き直すと、tsgo は同じ build で消した config の entry を nil として
  扱って落ちる。port は entry を作り直して続ける（file は inferred のまま）。
- **test**（`crates/project/tests/snapshot.rs`、26 件。新しい 7 件）：`TestSnapshot` の synthetic program の test の開いた file
  の部分、open と ensure での root の重複と SameFileNames（tsgo の probe）、configured project への配置と ensure だけでの作成、
  祖先を探さないこと（probe）と開き直し、close での後片付け（probe）、closeProjects が開いた file を守らないこと（probe）、
  project の無い file の拒否。
- **検証**（最終 bytes `60ac51f8a`。macOS、`nice -n 20`、Cargo の job 2）：`cargo fmt --all -- --check`、Clippy（program・
  checker・compiler・conformance・emitter・harness・incremental・project、`--all-targets -- -D warnings`）は clean。`cargo test -p`：
  program 599、project 26、全て成功。program の変更は `ConfigOptionBag::option_bool` の公開だけで、batch の compile の経路は
  変えていないので、conformance と suites は local では走らせず hosted の job に任せた。
- **残り**：P5-1b-3（project 参照）、P5-1c〜P5-1e。

### P5-1b-2 の hosted の記録と merge（2026-10-09）

- hosted：最終候補 `cb74744b0`（コード `60ac51f8a`・packet の記録）の run 37937350630（`plan` 30s、`rust` 11m15s、
  `conformance (TypeScript 7.1)` 21m3s、`gates` 15s。全て成功）。この slice は local で conformance と suites を走らせていない
  （batch の compile の経路を変えていない）ので、その確認はこの hosted の job による。merge → `6ad4b317b`（merge commit、PR #721）。
- 次：P5-1b-3（project system の project 参照）。

## P5-1b-3 project system の project 参照（2026-10-09）

- **参照先の config**（tsgo の compiler host `GetResolvedProjectReference`）：`resolve_project_references_with` が参照先の config
  の解析を呼び出し側から受け取る（command は今までどおり host で読んで解析する）。project system は各参照先の config を registry
  から project のものとして acquire し、program が読んだ file として記録する。作り直した program が参照しなくなった config と、
  削除する project の program の参照先は release する（tsgo `releaseDroppedProjectReferences`、`deleteProject`）。これで参照先
  の config の変更と、参照先の project に作られた file が、参照する project に届く。
- **参照先の source を読む program**（tsgo `UseSourceOfProjectReference`、`disableSourceOfProjectReferenceRedirect` で無効）：
  `load_project_config_program` と `ProgramOptions::with_project_reference_sources`。loader は参照先の出力の d.ts（とその
  symlink の綴り）の代わりに source を読み、module 解決は tsgo `projectReferenceDtsFakingHost` の移植（`dts_faking_host.rs`：
  未 build の出力の d.ts は source があれば在る、宣言の directory を持つ・その中の directory は在る、`node_modules` の package の
  link を辿る）を通す。checker は参照先の source を検査しない（tsgo `SkipTypeChecking` の `IsSourceFromProjectReference`。
  `ProgramFileFacts` の bit）。
- **default project**：参照先の source として file を持つ project は direct な inclusion ではないので、探索はその先へ進む。
  snapshot と builder の default project は tsgo `findDefaultConfiguredProjectFromProgramInclusion` の規則で direct な project を
  選ぶ。後片付けは default project の program の参照先の project も残す（tsgo `retainConfiguredProjectAndReferences`）。
- **synthetic program**：project 参照を受け取る（tsgo `APICreateProgramRequest.ProjectReferences`。`CreateProgramRequest` と
  `ProgramRoots` の `project_references`）。
- **option**：`disableSourceOfProjectReferenceRedirect`、`disableSolutionSearching`、`disableReferencedProjectLoad` は command も
  受け付ける language service の option にした（tsgo では project system だけが読む。今までの port は emit する program で
  unsupported として拒んでいた）。
- **command の経路**：command が解決した参照先の file は、source でも出力でも JavaScript として読まない判定にした（tsgo
  fileloader.go:911。JavaScript の file では今までの判定と同じなので結果は変わらない）。他は source を読む program だけの経路。
- **tsgo と違う所**：API で開いた file の探索が project を見つけず、探索した config を保持する project も無いとき（solution の
  `disableReferencedProjectLoad`、参照の循環）、tsgo は open の ensure が、同じ build の後片付けが消した config を acquire し直して
  落ちる（Go probe で確認。ensure の無い open なら file は inferred project）。port は file を inferred project に置く。
- **test**（`crates/project/tests/snapshot.rs`、37 件。新しい 11 件）：`projectreferencesprogram_test.go` の 14 case（参照先の
  source を読むこと、redirect の無効化、symlink の package 8 通り、directory index の subpath 2 通りと file の semantic diagnostics
  が 0、参照先への file の追加で program が変わること、参照の削除と registry）、`projectcollectionbuilder_test.go` の参照の case
  を API の request で（solution の direct／indirect／混在、`disableReferencedProjectLoad` の direct／indirect と参照の循環、own
  files を持つ solution、d.ts と ts の隣接）、synthetic program の参照。期待値は tsgo の test と、tsgo の `SnapshotHost` の Go
  probe（API の open／close と後の open。registry に残る config を含む）。
- **検証**（最終 bytes `7fcd3f94d`。macOS、`nice -n 20`、Cargo の job 2）：`cargo fmt --all -- --check`、Clippy（program・checker・
  compiler・conformance・emitter・harness・incremental・project、`--all-targets -- -D warnings`）は clean。`cargo test -p`：
  program 599、project 37、compiler の `live_program` 8、全て成功。loader と checker の batch の経路に触れたので、release build
  （2m31s）の full conformance を local で 1 回（`--workers 2 --check`、518s）：0 regressions、accepted tier を超える構成 0。
  errors full 13,451、emit 13,443、types 12,678（mismatch 89）、symbols 12,718（49）、sourcemap 13,451、trace 13,451、harness
  error 15（main と同じ）。suites の ratchet は hosted の `conformance (TypeScript 7.1)` に任せた。
- **残り**：synthetic program は常に参照先の source を読む（`CompilerOptions` に `disableSourceOfProjectReferenceRedirect` が
  無いので、API の createProgram の option の変換の slice で決める）。P5-1c〜P5-1e。

### P5-1b-3 の hosted の記録と merge（2026-10-10）

- hosted：最終候補 `5f1ad6a4a`（コード `7fcd3f94d`・packet の記録）の run 37945773018（`plan` 34s、`rust` 11m32s、
  `conformance (TypeScript 7.1)` 20m16s、`gates` 17s。全て成功。suites の ratchet もこの job で確認）。merge → `559e04b31`
  （merge commit、PR #722）。
- 次：P5-1c（parse cache と program の再利用）。tsgo の API の snapshot の変更（`computeSnapshotChanges`）は、program の
  `FilesByPath` の `*ast.SourceFile` の pointer が違う file を変更として返す。parse cache（parse の option・内容の hash・script
  kind が鍵の、parse と bind を済ませた file の参照の数を数える cache）があるので、作り直した program でも変わっていない file は
  同じ pointer になる。port の program は今は作るたびに全ての source を parse と bind し直すので、P5-1c はこの同一性（と
  `UpdateProgram` の Cloned）を持つ。lib は既に process 全体の cache（`lib_bundle`、lib の内容と binder の option の射影が鍵、
  identity domain を持つ）で共有されていて、source の cache はその identity domain の中に置く必要がある。

## P5-1c の設計（parse cache と program の再利用、2026-10-10）

### tsgo

- **parse cache**（`project/parsecache.go`、`SnapshotHost.parseCache`）：鍵は `ParseCacheKey`（`SourceFileParseOptions`：file 名・
  path・external module の判定の option・JSDoc の parse の mode、script kind、内容の hash）。値は parse と bind を済ませた
  `*ast.SourceFile`（tsgo の binder は compiler option を読まない）。compiler host の `GetSourceFile` が `Acquire` し、snapshot
  が program を手放すと `Release` する（参照の数を数える `RefCountCache`）。作り直した program でも、parse の option と内容が
  同じ file は同じ pointer になる。
- **API の snapshot の変更**（`api/session.go` `computeSnapshotChanges`）：変わった project の program について、
  `FilesByPath` の `*ast.SourceFile` の pointer が違う file を `changedFiles` に、無くなった file を `deletedFiles` に返す。
  parse cache があるので、作り直した program でも変わっていない file は変更に入らない。
- **`UpdateProgram`／`ReuseProgram`**（`compiler/program.go`）：project が 1 つの file だけの変更で dirty
  （`dirtyFilePath`）で command line が同じとき、その file を parse し直し、`canReplaceFileInProgram`（parse の option、
  script kind、external module か、`node:` の書き方、import の specifier と usage の mode、module augmentation、ambient
  module、参照、type reference、lib reference、checkJs の directive が同じ）で、package の redirect に関わらず、import
  helper と JSX runtime の import が要らないとき、他の file・解決・診断の元を共有した program を作る（`Cloned`）。include
  processor の診断は新しい file に対して作り直す（`updateFileIncludeProcessor`）。program の読んだ file の記録は前の program
  のものを使う。tsgo の test（`project_test.go` "Cloned on single-file change"、`snapshot_test.go`、API の
  `session_requestfilesystem_test.go`）が `Cloned` を確かめる。

### port

- **P5-1c-1 document cache**（parse cache と file の同一性）：
  - checker の `DocumentRegistry`（L0 の部品で、今は unit test だけが使う。明示の lease と host の version を持ち、version が
    同じで text が違うと失敗する）を tsgo の parse cache に作り替える。鍵は file 名、parse の option（tsc_syntax の
    `ParseOptions` の identity の base 以外）、binder の読む option の射影（`lib_bundle_options`：target、alwaysStrict、
    noFallthroughCasesInSwitch。port の binder はこれを読むので鍵に要る）、identity domain、内容（hash で引き、text で
    確かめる）。値は `Weak<BoundDocument>`：program（`ProgramSnapshot`）が持つ `Arc` が tsgo の参照の数の代わりで、明示の
    release は要らない。使われない incremental reparse の入口（`update_incrementally`）と namespace、version の扱いは除く。
  - `LiveChecker`（P5-1a）は cache を受け取り、program の source ごとに cache を引く。当たれば bound document をそのまま
    使い（loader の parse は捨てる）、外れれば今までどおり parse（loader の parse の採用を含む）と program 順の bind を
    して cache に入れる。lib の無い program は cache の持つ reclaiming な identity domain を使う（program ごとの ephemeral な
    domain では共有できない）。
  - project system は `SnapshotHost` に cache を持ち、program を作るたびに渡す。`ProjectProgram` は file の文書（同一性）を
    返す（API の `changedFiles` の比較に使う）。
  - batch の compile（CLI、conformance）は cache を使わない（経路は変えない）。
- **P5-1c-2 `Cloned`**：`update_program` が dirty な file 1 つで command line が同じとき、`PreparedProgram` の source を 1 つ
  差し替えた program を作る（新しい file の request の計画が古いものと同じ、`canReplaceFileInProgram` の比較、package の
  redirect と import helper／JSX runtime の除外）。他の file は document cache から同じものが来る。loader が file に位置を
  決めた program の診断（参照の file が無い、など）は、差し替えた file の分を作り直す必要がある（tsgo は include processor の
  診断を新しい program で作る）。program の読んだ file の記録は前のものを使う。`ProgramUpdateKind::Cloned`。詳細は P5-1c-1 の
  後に調べて決める。
- **test**：P5-1c-1 は、変わらない file の文書が作り直した program で同じであること、変えた file だけが違うこと、
  option の変更で bind が変わる場合に別の文書になること、cache から作った program の診断と型が cache の無い batch と同じこと
  （identity の順が違っても結果が変わらないこと）、program を手放すと cache から消えること。P5-1c-2 は tsgo の Cloned の
  test（API の open／change／ensure に書き換え）。

## P5-1c document cache と program の再利用（2026-10-10）

- **P5-1c-1 document cache**（tsgo `parseCache`）：
  - checker の `DocumentRegistry`（使われていなかった L0 の部品）を tsgo の parse cache にした。file 名と text で引き、parse の
    option、port の binder が読む option の射影、identity domain を address とする。entry は `Weak` で、program が document を
    持つ間だけ生きる（tsgo の参照の数の代わり）。lib の無い program 用に reclaiming な identity domain を持つ。明示の lease、
    host の version、incremental reparse の入口は除いた。
  - `LiveChecker` は cache を受け取り、text と address が同じ document を program が持っていればそれを使い（loader の parse は
    捨てる）、ほかは今までどおり parse と program 順の bind をして cache に入れる（`EphemeralDocumentStore::adopt`）。
    `LiveProgram::with_documents` と `LiveProgram::document`。
  - lib bundle の cache は lib を snapshot の同一性だけでなく text でも照合する（その文書の記述どおり）。program を読み直すと
    lib の file も読み直されるので、今までは program を作り直すたびに bundle を作って leak し、identity domain も変わっていた。
  - `SnapshotHost` が cache を持ち（tsgo `SnapshotHost.parseCache`）、snapshot の更新ごとに program が持たない entry を消す。
    batch の compile は cache を使わない。
- **P5-1c-2 `Cloned`**（tsgo `UpdateProgram`／`ReuseProgram`）：
  - 1 つの file の変更で dirty で command line が同じ project は、その file を読み直して前の program に差し替える
    （`LiveProgram::reuse`、`PreparedProgram::with_replaced_source`、`PreparedSourceFile::with_snapshot`）。解決と program の
    診断を共有し、他の file は document cache から同じ document が来る。前の build が読んだ file の記録と参照は引き継ぐ。
  - 差し替えない（tsgo `canReplaceFileInProgram`）のは、request（import と mode、module augmentation、path／type／lib
    reference、module か。位置は除く：`SourceRequestPlan::has_same_requests`）が変わる、check directive が変わる（checker の
    `check_directive` を公開）、synthesized な `tslib`／JSX runtime の import がある（`has_synthetic_imports`）、package の
    redirect に関わる、file に program の行がある（tsgo は新しい file で位置を決め直す）とき。
- **tsgo と違う所**：
  - port の binder は target、alwaysStrict、noFallthroughCasesInSwitch を読むので、これらと lib の組が変わると document は
    別になる（tsgo の binder は option を読まず、parse の option だけが鍵）。
  - global な宣言 file の ambient module 名だけが変わる変更は、tsgo は作り直し、port は差し替える（port の request の計画は
    ambient module 名を持たない。checker は document から作り直すので結果は同じで、更新の種類だけが違う）。
- **test**：checker の registry 2 件（同じ file・text・address で同じ document、text・file 名・parse・bind・domain が違えば別、
  program が手放すと消える）。compiler の live program 2 件（作り直した program が lib を含む変わらない file の document を
  共有し、診断は単独で作った program と同じ、parse を変えない option（strict）では共有、変える option（moduleDetection
  force）では別、program を手放すと空になる、lib の無い program も共有する）。project 3 件（snapshot をまたいで変わらない
  file の document が同じ、1 file の変更で Cloned・helper の document は同じ・inferred project の program と種類は変わらない、
  import の追加と `@ts-nocheck` の追加で SameFileNames・値の変更で Cloned。期待値は tsgo の `SnapshotHost` の Go probe）。
- **検証**（最終 bytes `b638a482b`。macOS、`nice -n 20`、Cargo の job 2）：`cargo fmt --all -- --check`、Clippy（program・checker・
  compiler・conformance・emitter・harness・incremental・project、`--all-targets -- -D warnings`）は clean。`cargo test`：program 599、
  project 40、compiler の `live_program` 10、checker の `program::tests` 9、全て成功。checker の parse の段と lib bundle の照合が
  batch の経路にもあるので、release build（2m03s）の full conformance を local で 1 回（`--workers 2 --check`、487s）：
  0 regressions、accepted tier を超える構成 0。errors full 13,451、emit 13,443、types 12,678（mismatch 89）、symbols 12,718（49）、
  sourcemap 13,451、trace 13,451、harness error 15（main と同じ）。suites の ratchet と workspace の test は hosted に任せた。
- **残り**：P5-1d（checker pool）、P5-1e（`SnapshotFS` の細部）。loader は cache を引かない（作り直す program は loader で全ての
  file を parse し、checker が cache の document を使う。tsgo は loader も cache を引くので parse しない。結果は同じで cost が
  違う）。

### P5-1c の hosted の記録と merge（2026-10-10）

- hosted：最終候補 `482d993f1`（コード `b638a482b`・packet の記録）の run 37957868864（`plan` 34s、`rust` 10m29s、
  `conformance (TypeScript 7.1)` 17m55s、`gates` 15s。全て成功。suites の ratchet もこの job で確認）。merge → `a8d2e3875`
  （merge commit、PR #723）。hosted の conformance は前の 2 回（20m16s、21m3s）より短い。lib bundle を text で照合して
  再利用するようになったためと見られる（時間の比較は条件をそろえた測定ではない）。
- 次：P5-1d（checker pool）。tsgo の API は diagnostics の request（syntactic／bind／semantic／suggestion／declaration／global）を
  pool の diagnostics checker（`CheckerLifetimeDiagnostics`）で、型と symbol の query を API checker（`CheckerLifetimeAPI`、
  消されない persistent な checker）で行う。port の `LiveProgram` は 1 つの checker で両方を行うので、query が後の診断に
  影響しないよう、API checker を分ける。LSP の query checker と idle の後片付け（時間で決まる）は移さない。

## P5-1d API checker（2026-10-10）

- **tsgo**：project の checker pool（`project/checkerpool.go`）は、diagnostics checker（index 0。LSP の診断。idle で消える）、
  query checker（LSP の操作）、API checker（`CheckerLifetimeAPI`。消されない persistent な checker で、API の型と symbol の
  handle の同一性を保つ）を持つ。API の session は diagnostics の request（syntactic／bind／semantic／suggestion／declaration／
  global。`api/session.go` の `handleGet*Diagnostics`）を `CheckerLifetimeDiagnostics` で、型の query（`GetTypeChecker`）を
  `CheckerLifetimeAPI` で行う。
- **port**：`LiveChecker` は診断の checker のほかに API checker を持ち（最初の query で作る）、`with_checker`
  （`LiveProgram::with_checker`）はそれを使う。今までは 1 つの checker が両方を行ったので、file を検査する query がその検査の
  行を後の診断に足すことがあった。
- **移さないもの**：LSP の query checker、pool の idle の後片付け（30 秒で消して作り直す。時間で結果が決まる部分）、diagnostics
  checker の作り直しをまたぐ global diagnostics の蓄積（port の diagnostics checker は program と同じだけ生きる）。
- conformance の runner の `TSRS_LIVE_CHECK`（任意の比較）は、batch の walk の checker と同じく、API checker で先に全ての file を
  検査してから型と symbol を walk する。
- **test**：compiler の live program 1 件（library を検査する query の後でも、script の最初の診断は空で、library の検査の後に
  2374）。共有の checker（前の code）ではこの test が `[2374]` で失敗することを確かめた。
- **検証**（最終 bytes `63a105c8a`。macOS、`nice -n 20`、Cargo の job 2）：`cargo fmt --all -- --check`、Clippy（checker・
  compiler・conformance・project、`--all-targets -- -D warnings`）は clean。`cargo test`：compiler の `live_program` 11、project 40、
  全て成功。変えたのは `LiveChecker` と任意の live 比較だけで、batch の compile の経路と checker の検査は変えていないので、
  conformance と parallel control は local では走らせず hosted の job に任せた。
- **残り**：P5-1e（`SnapshotFS` の細部）。

### P5-1d の hosted の記録と merge（2026-10-10）

- hosted：最終候補 `8e59137c6`（コード `63a105c8a`・packet の記録）の run 37960764872（`plan` 30s、`rust` 11m9s、
  `conformance (TypeScript 7.1)` 20m7s、`gates` 34s。全て成功）。この slice は local で conformance を走らせていないので、その
  確認はこの hosted の job による。merge → `cf74a88b6`（merge commit、PR #724）。
- 訂正：P5-1c の hosted の記録で、conformance の job が短かった（17m55s）のは lib bundle の再利用のためと見た。この run は
  20m7s で、その差は hosted の runner のばらつきの範囲と見るのが妥当（条件をそろえた測定は無い）。
- 次：P5-1e（`SnapshotFS` の細部）。API の経路で効くもの：`node_modules` の symlink を通して読んだ file の realpath の alias
  （realpath への変更の通知を symlink の path に広げる）、削除の通知があり program が作り直されたときの cache の掃除（どの
  project も読んでいない file を消す）、作った snapshot の file system の読み（cache に無い file の読みを snapshot ごとに 1 回に
  する memo、directory の一覧に cache の file を合わせる）。API の通知は常に `node_modules` の外の変更として扱われる
  （`toFileChangeSummary`）ので、`node_modules` だけの無効化は API の経路では使われない。

## P5-1e `SnapshotFS` の細部（2026-10-10）

- **realpath の alias**（tsgo `nodeModulesRealpathAliases`、`recordRealpathAlias`、`expandRealpathAliases`）：`node_modules` の中の
  file を link を通して初めて読んだとき、その real path に link の path を記録する。real path の file の変更と削除の通知は、
  link の path にも広げてから（`markDirtyFiles` の前）cache に当てる。削除した entry は alias から外す（file がまだ在る
  `deleteCacheEntry` は除く。tsgo の `sourceBackedReplacements`）。
- **cache の掃除**（tsgo `Clone` の `shouldCleanFileCache`）：削除の通知があり、program を作り直した（再利用ではない）project が
  あるとき、どの project の最後の build も読んでいない cache の file を消す（registry が解析した config も読んだ file には
  入らないので消える）。
- **読み**：build の directory の一覧は、snapshot が cache した file と directory を先に並べ、disk の一覧で足りないものを足す
  （tsgo `GetAccessibleEntries` と `cacheDirectories`。cache の path の並びから求める）。作った snapshot は cache に無い file を
  snapshot ごとに 1 回だけ読む（tsgo `readFiles`）。
- API の通知は常に `node_modules` の外の変更として扱われる（tsgo `toFileChangeSummary`）ので、`node_modules` だけを無効にする
  経路（`invalidateNodeModulesCache`）は API の経路に無い。LSP の overlay、open／close の変換、content mapper は移さない。
- **test**（`crates/project/tests/unit/fs.rs`、10 件）：tsgo `TestRealpathAliasLifecycle` の case（link で読んだ file の記録と
  link でない file、`node_modules` の外、snapshot をまたいだ持ち越し、削除で外れること、1 つの real file への複数の link と
  個別に外れること・前の snapshot が変わらないこと、変更と削除の通知の広がり・alias が無いとき、real path の変更で link の
  file が読み直されること）、cache した file を残す一覧、cache に無い file の 1 回の読み、削除の後の cache の掃除（tsgo の
  `SnapshotHost` の Go probe：open の後は `a.ts`・`b.ts`・`tsconfig.json`、`b.ts` の削除と ensure の後は `a.ts` だけ）。
- **検証**（最終 bytes `be1246806`。macOS、`nice -n 20`、Cargo の job 2）：`cargo fmt --all -- --check`、Clippy（project、
  `--all-targets -- -D warnings`）は clean。`cargo test -p tsc-rs-project`：unit 10、snapshot 40、全て成功。変えたのは project の
  crate だけなので、conformance は local では走らせず hosted の job に任せた。
- **P5-1 の完了**：project system の核（P5-1a〜P5-1e）はこれで揃う。次は API の transport と session（P5-2 以降）。

### P5-1e の hosted の記録と merge（2026-10-10）

- hosted：最終候補 `c4232a5ca`（コード `be1246806`・packet の記録）の run 37964140513（`plan` 28s、`rust` 13m15s、
  `conformance (TypeScript 7.1)` 16m6s、`gates` 16s。全て成功）。この slice は local で conformance を走らせていないので、その
  確認はこの hosted の job による。merge → `b6e5b991a`（merge commit、PR #725）。
- **P5-1（project system の核）はこれで終わる**：live program（P5-1a）、snapshot と project（P5-1b-1〜3）、document cache と
  program の再利用（P5-1c）、API checker（P5-1d）、`SnapshotFS` の細部（P5-1e）。

## P5-2 API の transport と session の計画（2026-10-10）

### tsgo の構成（`19dadef8`）

- **transport**（`api/server.go`、`ipc/`）：stdio（既定）か named pipe／Unix socket。既定は同期の MessagePack
  （`MessagePackProtocol` と `SyncConn`）、`--async` で JSON-RPC（`AsyncConn`）。
- **MessagePack の形**（`api/protocol_msgpack.go`）：要素 3 の配列 `[type, method, payload]`。type は request 1、call の response
  2、call の error 3、response 4、error 5、call 6（正の fixint か uint8）。method と payload は bin8／16／32。payload は JSON
  （binary の response は raw）。id は無く、method 名で request と response を対応させる。
- **SyncConn**（`ipc/conn_sync.go`）：message を 1 つずつ読み、request をその場で処理して response を書く。処理中の client への
  call（callback の FS など）は response をその場で読み、その間に client から来た request（入れ子）も処理する。
  `getServerTiming`／`resetServerTiming` は conn が答える。panic は stack 付きの error の response になる。
- **session**（`api/session.go`）：`HandleRequest` が method で振り分ける。snapshot の handle ごとに snapshot、open の状態、request の
  FS を持ち、project ごとに symbol／type／signature の handle を登録する。`release` で手放す。`batchRequests` は request を順に
  処理し、response を page に分けて返す。
- **CLI**：`tsc --api [--async] --cwd <dir> [--callbacks <names>] [--pipe <path>]`。

### port の方針

- session は transport から分ける（WASM の build で、process を立てずに同じ session を呼べるように）。protocol の型は workspace に
  既にある `serde`／`serde_json` で表す。
- request は 1 つずつ処理する（WASM。tsgo の `AsyncConn` は request を並行に処理するが、response は request の id で対応するので、
  順に処理しても結果は同じ）。
- project system は P5-1 のもの（`tsc-rs-project`）、source file の encoder は P4-7d のもの（`tsc-rs-api`）を使う。

### slice

- **P5-2a transport と session の骨格**：MessagePack の protocol と SyncConn（入れ子の request を含む）、JSON-RPC の protocol と
  AsyncConn、session の振り分けと error の形、`initialize`、`createSnapshot`／`updateSnapshot`／`release`、
  `getDefaultProjectForFile`、`batchRequests`、`tsc-rs --api`。test：`proto_test`、`server_test`、`jsonvalue_test`、`ipc` の conn
  の test、`session_batch_test`、`session_apistate_test`、`session_createprogram_test`（Rust の test に移す）。
- **P5-2b request の file system と callback の FS**：`requestfilesystem`（path tree、file changes、層）と `callbackfs`。test：
  `requestfilesystem` の 3 file、`session_requestfilesystem_test`。
- 以降は「依存と順序」の 3〜6（project の要らない request、checker の query、印字、build orchestrator）。LS の 6 handler は
  未実装の error を返す（LSP の作業に回す）。

## P5-2a transport と session の骨格（2026-10-10）

- **構成**：
  - `tsc-rs` の実行 file を `tsc-rs-compiler` から新しい `crates/cli`（`tsc-rs-cli`、tsgo `cmd/tsc`）に移した。API の session は
    `tsc-rs-project`（compiler に依存する）を使うので、`--api` を compiler の package に置くと依存が循環する。実行 file を起動する
    CLI の contract test（`cli_contract.rs`、262 件）も一緒に移した。README の build と install の command は `crates/cli` を指す。
  - API は tsgo の `internal/api` と同じく `tsc-rs-api` に置く：`ipc`（message、protocol、`Conn`、timing、JSON-RPC の framing）、
    `msgpack`、`proto`、`session`、`server`。
- **transport**（tsgo `ipc/conn_sync.go`、`conn_async.go`、`protocol_jsonrpc.go`、`api/protocol_msgpack.go`、`ipc/timing.go`）：
  - `Conn` は 1 つで、`Mode::Sync`（MessagePack。call の ID は method 名、待たれていない response は error）と `Mode::Async`
    （JSON-RPC。request を 1 つずつ処理する。call の ID は `api<n>`、待たれていない response は捨て、止まった後の call と notify は
    原因つきの `ipc: connection closed`）を持つ。
  - call は client の response まで message を読み、その間に来た request（入れ子）と notification を処理する。handler の panic は
    `panic: <message>` の error response になる（tsgo は stack も付ける）。`getServerTiming` と `resetServerTiming` は conn が
    答える（`--timing`。時間の数は Go の `float64` の書き方）。
- **session**（tsgo `api/session.go`）：`echo`、`ping`、`initialize`、`batchRequests`（page 分け、continuation token、入れ子の
  拒否、request ごとの panic の回収）、`createSnapshot`、`updateSnapshot`、`release`、`getDefaultProjectForFile`。
  `getCurrentLanguageServerSnapshot` は tsgo の standalone の session と同じ error。tsgo の他の method（`proto.go` の 170 の
  うち残り）は `<method> is not implemented yet`、それ以外は tsgo と同じ `unknown API method`。
  - snapshot ごとの open の状態（`reconcileSnapshotOpens`）、file の通知、`DocumentIdentifier`（file 名か URI。tsgo の
    `DocumentUri.FileName` を Go probe で固定）、`createPrograms`／`reconfigurePrograms`／`removePrograms`／`ensurePrograms` の検証と
    その順は tsgo のまま。update の response の project は、追加分（新しい snapshot の順）の後に置き換え分（基の snapshot の順）。
    changes は基の順で、program の file は document の同一性で比べる（tsgo は `SourceFile` の pointer）。
- **option**：client の `compilerOptions`（tsgo `core.CompilerOptions` の JSON。enum は数、`lib` は file 名）を `ConfigOptionBag` に
  する（`tsc_program::go_json::compiler_options_bag`）。synthetic program と inferred project は bag を持ち
  （`CreateProgramRequest.options`、`ProgramRoots::new`）、program の option は command line と同じ変換
  （`command_line_program_inputs`）で作る。前は typed の option を直接持っていた。tsgo の内部 option（`allowNonTsExtensions`、
  `noEmitForJsFiles`、`suppressOutputPathCheck`）は bag から読む（tsconfig と command line は宣言しないので現れない）。
  - response の option には、conformance の runner にあった tsgo の JSON の writer（`GoJson`、`core.CompilerOptions` の field の
    順、enum の数）を `tsc_program::go_json` に移して使う。tsconfigParsing、tsoptions、tsc の suite も同じものを使う。
  - `encoding/json/v2` の規則に合わせた：`omitempty` は `null`・`""`・`{}`・`[]` を省いて `false` は残す（`changes`、
    `typeAcquisition`、`raw` の `{}` を省き、project reference の `circular` は常に書く）。config の `compileOnSave` は常に書く
    （raw が boolean でなければ false）。`null` の parameter は zero 値。synthetic project の ID は decode で検証して正規化する。
    JSON の `echo` は空白を詰める。
- **`tsc-rs --api`**（tsgo `cmd/tsc/api.go`、`api/server.go`）：`--cwd`、`--pipe`（Unix domain socket。終わりに socket の file を
  消す）、`--callbacks`、`--async`、`--timing`、`--runExternalCode`。Go の `flag` の形、error と usage、exit code（server の失敗
  1、flag の誤り 2）。library は実行 file に埋め込んだもの（`BundledFs`、tsgo `bundled.WrapFS`。native の compiler host と
  library の検索を共有する）。
- **移さないもの・違い**：`--callbacks` の callback FS と request の file system（P5-2b。今は error）、Windows の named pipe、
  content mapper（`--runExternalCode` は受けて使わない）、signal で止める扱い、error の stack。不正な parameter の decode の
  error の文言は serde のもの（Go は `json: cannot unmarshal ... within "/path"`）。Go が map から書く file の list
  （`deletedFiles`、`changedFiles`）は Go では順不同で、port は path の順。不正な URI は tsgo では panic、port は client error。
- **test**：
  - api の unit：timing 6（tsgo `timing_test.go`。負の duration は Rust に無い）、proto 8（`proto_test.go` の 4、synthetic の ID、
    `ToDiagnostic`、URI の変換 9 例）、session 26（`session_batch_test.go` 8、`session_createprogram_test.go` 9、
    `session_apistate_test.go` の standalone の 2、tsgo の response に合わせた snapshot の更新、config の response、release、
    default project、initialize、echo、未実装の method、request の FS）。
  - api の `ipc` 14（`conn_sync_test.go` 2、`conn_async_test.go` のうち 1 つずつ処理しても成り立つ case、入れ子の request、
    call の error、timing の request、MessagePack と JSON-RPC の framing と error）、`server` 2（flag）。
  - cli の contract 2（`tsc-rs --api` を起動して MessagePack で話す、flag の error）、compiler の system 1（`BundledFs`）。project の
    test は option を tsgo の JSON で書くように変えた。
  - 移さない Go の test：`server_test.go`（context の cancel）、`jsonvalue_test.go`（package.json の値の変換。使う method と
    一緒に移す）、`conn_async_test.go` の handler を並行に動かす case（RunWaitsForHandlers、CancelsHandlersOnEOF、
    CallReturnsWhenPeerCloses。1 つずつ処理するので成り立たない）。
- **tsgo との比較**：同じ request の列（configured project 3 つ（参照、config の error、jsconfig）、synthetic program（URI の root、
  option、参照、client の診断）、inferred project、file の変更・ensure・close・削除・再構成・除去、batch の page、error の case）を
  tsgo（19dadef8 の実行 file）と tsc-rs に MessagePack と JSON-RPC で送った：response は sync 40 行・async 50 行が一致（decode
  の error の文言と Go の map の順を除く）。flag の error と usage の出力は byte 単位で一致。
- **TypeScript の client の test**（local。`packages/typescript/test/sync/api.test.ts` を Node 25 で source から実行。実行 file は
  copy した package の `built/local/tsc` で切り替えた）：tsgo 327/327、tsc-rs 6/327。失敗のうち 316 は client の FS を
  `--callbacks` で渡す test（P5-2b）、残りは未実装の method。
- **検証**（最終 bytes `558550d53`。macOS、`nice -n 20`、Cargo の job 2）：`cargo fmt --all -- --check`、Clippy（api・cli・project・
  program・compiler・conformance、`--all-targets -- -D warnings`）は clean。`cargo test`：api（unit 40、`ipc` 14、`server` 2、
  encoder 5、fixture 1）、cli の contract 264（移した 262 と `--api` の 2）、project 50、program の lib 70、conformance の lib 50、
  compiler の system の `BundledFs` 1、全て成功。option の bag と JSON の writer の移動は batch の経路（config の option と suite の
  JSON）に触れるので、release build（2m13s）の full conformance を local で 1 回（`--workers 2 --check`、467s）：0 regressions、
  accepted tier を超える構成 0。errors full 13,451、emit 13,443、types 12,678（mismatch 89）、symbols 12,718（49）、
  sourcemap 13,451、trace 13,451、harness error 15（main と同じ）。suites（`scripts/suites_ts71.py --check`）：0 regressions。
  api 2、config 87、transpile 41、tsoptions 80、tscWatch 42 は全て full、tsc 211/223、tsbuild 182/192、tsbuildWatch 63/65 は
  main と同じ。workspace 全体の test と Clippy は hosted の `rust` job に任せた。
- **残り**：P5-2b（request の file system と callback の FS。client の test の多くはこれで走る）。その後は「依存と順序」の 3〜6
  （project の要らない request、checker の query、印字、build orchestrator）。

### P5-2a の hosted の記録と merge（2026-10-10）

- hosted：最終候補 `2be38d592`（コード `558550d53`・packet の記録）の run 37973788728（`plan` 30s、`rust` 9m0s、
  `conformance (TypeScript 7.1)` 13m36s、`gates` 15s。全て成功。workspace 全体の test と Clippy はこの `rust` job による）。
  merge → `216c9f83a`（merge commit、PR #726）。
- 次：P5-2b（request の file system と callback の FS）。tsgo の request の file system（`api/requestfilesystem`）は snapshot の
  file system の層（`project.LayeredFileSystem`。基の file system を差し替えられ、request の file を overlay として見せる）として
  入り、file の変更の通知を request の file に広げる（`ExpandFileChanges`、`addFileChanges`）。callback の FS（`api/callbackfs.go`）
  は conn ができた後に結ばれ、client への call は protocol の lock で 1 つずつ行う（worker の thread からの読みも同じ lock で
  待つ）。

## P5-2b request の file system と callback の FS（2026-10-10）

- **request の file system**（tsgo `api/requestfilesystem`）：`tsc_api::request_fs`。full（正準で全体）と layer（host の前に
  引く）。file、完全な一覧、link（request の中か host への）、取り除く path を持つ。path tree（`pathtree.go`）は node を path
  ごとに持ち、層を合成すると触らない node を共有する（copy-on-write）。request の file system の上の layer はすぐ合成する
  （tsgo の eager compaction）ので、request の file system は常に host の上に直接ある。vfs の `FileSystem` を実装する（読み、
  metadata、link を含む一覧、real path、layer は host へ書く）。
- **file の変更**：layer の file は changed か created、取り除く path は deleted、一覧と link は deleted と created になる
  （tsgo `addFileChanges`）。snapshot の file の変更は request の link で別名にも広げる（tsgo `ExpandFileChanges`。tsgo の
  snapshot が行う所を session が clone の直前に、同じ file system で行う）。
- **session**：snapshot ごとに request の file system を持ち、update はその上に layer を重ねる（full は host から始め直す）。
  full の file system の update は前に読んだものを捨てる（`replace_file_system`）。
- **callback の FS**（tsgo `api/callbackfs.go`）：`tsc_api::callback_fs`。`--callbacks` の readFile、fileExists、
  directoryExists、getAccessibleEntries、realpath、writeFile、removeFile を client が答え、`null` は base（埋め込みの library
  の上の disk）に任せる。conn ができた後に結ぶ。失敗した call は tsgo と同じく panic し、その request が error になる。
- **一覧の順**：`VfsCompilerHost` は file system の一覧の順を保つ（前は UTF-16 の順に並べ替えていた）。tsgo は config の glob を
  その順で照合する。disk の一覧は名前の順（`os.ReadDir`）、client の一覧は client の順。TypeScript の client の test
  （`project exposes parsedCommandLine`）で見つかった。
- **修正**：root の config（`/tsconfig.json`）の project の directory が空になっていた（tsgo `GetDirectoryPath` では `/`）。
  config の探索も tsgo `ForEachAncestorDirectory` のとおり祖先をたどる（drive の root を含む）。P5-1 の test の config は全て
  下位の directory にあったので現れなかった。
- **tsgo と違う所・移さないもの**：
  - language server の overlay（`Overlays`、overlay の file handle、overlay を隠す層の変更）は standalone の session に無い。
  - tsgo の snapshot は cache した一覧の項を Go の map の順で先に並べる（`mergeCachedDirectoryEntries`）ので、tsgo では実行
    ごとに順が変わる（同じ要求を 6 回送って 2 通り）。port は path の順で、tsgo の取りうる順の 1 つ。重複した symlink の error
    がどちらの名を先に書くかも Go の map の順。
  - CLI の disk の host（`FsCompilerHost`）は今も UTF-16 の順に並べる（tsgo は byte の順）。違うのは U+E000〜U+FFFF と補助面の
    文字を混ぜた名だけで、この slice では変えない。
- **test**：
  - api の unit：request_fs 64。tsgo `requestfilesystem_test.go` の 103 case（overlay の 1 つを除く。parameter 付きの helper は
    1 つの Rust の test で全ての組を回す）、`pathtree_test.go` の 15（Go の `FileInfo` の mode と同一性は種類・大きさ・時刻で）、
    `filechanges_test.go` の 4（URI でなく file 名で）、変更の展開、種類の誤り、JSON。
  - session 14：`session_requestfilesystem_test.go` の standalone の 12（`GetFile` の同一性は内容で）、誤りの 1、一覧の順の 1
    （client の test と同じ形）。emit の 1 は emit と、LSP（editor の overlay と変更、auto import）を使う 10 は LSP と一緒に移す。
  - `callback_fs` 7（各 callback の答えの形、base への fallback、未知の名、conn の前の call）、project 1（root の config）、
    host 1（一覧は file、次に directory、それぞれ file system の順）。
- **tsgo との比較**：request の file system の場面（full の file system の config、explicit な一覧、link、layer による変更・
  削除・追加・link 先の変更、disk の上の layer、full への置き換え、invalidateAll、誤り）を MessagePack と JSON-RPC で比べ、
  上の tsgo の不定な順を除いて一致した。
- **TypeScript の client の test**（local。P5-2a と同じ実行）：tsc-rs 21/327（P5-2a は 6）。残りの 306 は全て未実装の method
  （getSourceFile 89、getSymbolAtPosition 87 など）で、それ以外の失敗は無い。
- **検証**（macOS、`nice -n 20`、Cargo の job 2）：`cargo fmt --all -- --check`、Clippy（api・project・host・program・cli、
  `--all-targets -- -D warnings`。最後の test の追加の後に api・host を再び）は clean。`cargo test`：api（unit 117、
  `callback_fs` 7、`ipc` 14、`server` 2、encoder 5、fixture 1）、host（unit 22、contract 15＋10）、project（unit 10、snapshot 41）、
  compiler の `system` 9、cli の contract の `--api` 2、全て成功。`VfsCompilerHost` の一覧の順と project の builder は batch の
  経路（suite の tsc／tsbuild の memory の file system）に触れるので、release build（2m15s）の full conformance を local で
  1 回（コード `b07627cd9`、`--workers 2 --check`、464s）：0 regressions、accepted tier を超える構成 0。errors full 13,451、
  emit 13,443、types 12,678（mismatch 89）、symbols 12,718（49）、sourcemap 13,451、trace 13,451、harness error 15（main と
  同じ）。suites（`scripts/suites_ts71.py --check`）：0 regressions。api 2、config 87、transpile 41、tsoptions 80、tscWatch 42
  は全て full、tsc 211/223、tsbuild 182/192、tsbuildWatch 63/65 は main と同じ。その後の変更は test 2 つと `server.rs` の
  comment だけ（conformance の runner は `tsc-rs-api` を link しない）。workspace 全体の test と Clippy は hosted の `rust` job に
  任せた。
- **残り**：P5-2 はこれで終わる。次は「依存と順序」の 3（project の要らない request：command line と config、`createSourceFile`、
  `transpile*`、module resolver）から計画する。

### P5-2b の hosted の記録と merge（2026-10-10）

- hosted：最終候補 `800a5743e`（コード `b07627cd9`、test `7d3536cf5`、packet の記録）の run 37980248024（`plan` 24s、`rust`
  11m40s、`conformance (TypeScript 7.1)` 20m6s、`gates` 15s。全て成功。workspace 全体の test と Clippy はこの `rust` job に
  よる）。merge → `f2f010b07`（merge commit、PR #727）。
- P5-2（transport と session、request の file system、callback の FS）はこれで終わる。次は P5-3 の計画（「依存と順序」の 3：
  project の要らない request）。

## P5-3 project の要らない request と program の情報の計画（2026-10-10）

「依存と順序」の 3（project の要らない request）に、project の program を読むが checker を使わない request（program の情報）を
加える。TypeScript の client の test で、最初に足りない method が `getSourceFile` のものが 89 ある。

### tsgo の構成（`19dadef8`）

- **config と command line**（`api/session.go` 1749–1828）：
  - `parseCommandLine{commandLine}`：`tsoptions.ParseCommandLine` の結果を `ConfigFileResponse`（P5-2a の project の形）で返す。
  - `readConfigFile{file}`：読めなければ `{config: {}, error: Cannot_read_file_0}`。読めれば `ParseConfigFileTextToJson` の JSON
    値と最初の error。
  - `parseJsonConfigFileContent{json, configDirectory | configFileName}`：JSON 値から parse する（`ParseJsonConfigFileContent`）。
    directory と file 名はちょうど 1 つ（両方か無しは client error）。
  - `parseConfigFile{file}`：tsconfig の source file から parse する（`ParseJsonSourceFileConfigFileContent`）。読めなければ
    client error（`could not read file %q`）。
- **source file**（1835–1930）：`createSourceFile{fileName, sourceText, options{scriptKind}}` と `createSourceFileFromFile`。
  script kind は Unknown なら file 名から決め、JS・JSX・TS・TSX・JSON 以外は client error。parse cache から lease を取り
  （`SnapshotHost.AcquireSourceFile`）、encode して header の lease ID（52–59 byte）を書く。`releaseSourceFile{lease}` で手放す
  （0 は `empty source file lease`、無い lease は `not found`）。session を閉じるとき残りを手放す。
- **transpile**（1830、1932–1969）：`transpileModule`／`transpileDeclaration`（`{input, options{compilerOptions, fileName,
  reportDiagnostics}}`）と、file を読んで file 名を置き換える `*FromFile`。`{outputText, diagnostics, sourceMapText}` を返す。
- **program の情報**（1971–2283。project の program を読み、checker は使わない）：
  - `getSourceFile`：encode する。無ければ MessagePack は空の binary、JSON は null。
  - `getSourceFileNames`。
  - `getSourceFileMetadata`：default library か、external library か、package.json の type と directory、implied node format。
  - `getConfigFileNames`（config と extends の file）と `getConfigSourceFile`（そのどれかの tsconfig source file）。
  - `getModeForUsageLocation`／`getModeForResolutionAtIndex`。
  - `getResolvedModule`（＋`FromModuleSpecifier`）と `getResolvedTypeReferenceDirective`（＋`FromReference`）。
  - node は handle（`index.kind.path`。index は encoder の node index table のもの）で指す（`resolveNodeHandle`）。
- **module resolver**（`api/module_resolution.go` 401 行）：
  - `createModuleResolver{compilerOptions, moduleResolutions, resolveModuleNameCallback}` と `releaseModuleResolver`。
  - `resolveModuleName{resolver, moduleName, containingDirectory, resolutionMode, snapshot | inProgressSnapshot}`：
    `{resolvedModule, trace}` を返す。
  - `createSnapshot` の `createPrograms` に `moduleResolver` を渡すと、program の module の解決が static な解決表
    （`module.StaticResolutions`。specificity で選ぶ）と client の callback に替わる。callback の失敗は program の
    `ModuleResolutionError` として request の error になる。

### port の状態

- **既にあるもの**：
  - command line の parse（`tsc_program::parse_command_line`、P3-6g）と config の text から JSON へ
    （`parse_config_file_text_to_json`）。
  - config の source file からの parse（project と CLI の経路）。
  - transpile（`tsc_compiler::transpile_module`／`transpile_declaration`、P4-4）。
  - encoder（`encode_source_file`、`build_node_index_table`、P4-7d）と parse cache（`DocumentRegistry`、P5-1c）。
- **無いもの**：
  - JSON 値からの config の parse。tsconfig の suite は text から parse して error の位置を外している。
  - lease と node handle。
  - program の情報の照会の一部：metadata、mode、解決済みの module の照会。
  - module resolver の差し替え（loader は自前の resolver だけを使う）と static resolutions。

### slice

- **P5-3a config・command line・source file・transpile**：
  - 対象は `parseCommandLine`、`readConfigFile`、`parseJsonConfigFileContent`、`parseConfigFile`、`createSourceFile`
    （＋`FromFile`）、`releaseSourceFile`、transpile の 4 種。
  - test：`session_createsourcefile_test`（1）、tsgo の probe で固定した Rust の test、client の test。
- **P5-3b program の情報**：
  - 対象は `getSourceFile`、`getSourceFileNames`、`getSourceFileMetadata`、`getConfigFileNames`、`getConfigSourceFile`、mode と
    解決済みの module／type reference の照会、node handle。
  - test：tsgo の probe と client の test。
- **P5-3c module resolver**：
  - 対象は `createModuleResolver`／`releaseModuleResolver`／`resolveModuleName`、static resolutions、program の resolver の
    差し替え（callback を含む）。
  - test：`session_module_resolution_test` の 5。LSP の snapshot を使う 1 は LSP と一緒に移す。
- 次は P5-4（checker の query）。

## P5-3a config・command line・source file・transpile（2026-10-10）

- **request**（tsgo `api/session.go` 1749–1969）：
  - `parseCommandLine`：`tsoptions.ParseCommandLine` を session の file system（response file を読む）の上で行い、
    `ConfigFileResponse` で返す。option は command line が設定する全ての compiler option（command 自身の `project`、`watch`
    なども。`project` は絶対 path）、raw は parser の値（名前の値は番号、`lib` は file 名、他は書かれたまま）。
  - `readConfigFile`：config の JSON と最初の error。読めなければ `{config: {}, error: Cannot_read_file_0}`。
  - `parseConfigFile`：config file の parse。読めなければ client error。
  - `parseJsonConfigFileContent`：値を JSON text にして parse し、error の位置を外す（tsconfig の suite の json api と同じ）。
    directory だけなら config file の無い parse（`configFilePath` なし、message の file 名は `''`）。directory と file 名は
    ちょうど 1 つ。object でない値は空の object。
  - `createSourceFile`（＋`FromFile`）：script kind（0 は file 名から、知らない拡張子は TS、JS・JSX・TS・TSX・JSON 以外は
    client error）、parse と bind、header の lease（52–59 byte）。MessagePack は binary、JSON は base64。
  - `releaseSourceFile`：tsgo の error（0 は `empty source file lease`、無い lease は `not found`）。lease は番号の集合で持つ。
    tsgo の lease は parse cache の参照だが、port の parse cache は program の identity domain の中にあり、client の file を
    program と共有しない。
  - transpile 4 種：tsgo の `CompilerOptions` を option の bag にして program の option に変え、P4-4 の transpile に渡す。
    diagnostic の source line は input から。
- **既存の経路の変更**（いずれも tsgo の動作に合わせる）：
  - encoder は bind 済みの file に binder の flag（到達性、`this`、implicit／explicit return、export context、async
    function）を書く。tsgo の binder は node の flag をその場で設定するので、API が encode する file にはそれがある。P4-7d の
    fixture は tsgo の bind しない parse で、従来どおり。
  - JSON file は JSX の language variant で parse する（`getLanguageVariant`。tsc も同じ）。
  - command line の空か未知の enum の値は null（未知のものは error 付き）で、config の option への merge は明示の null として
    扱う（tsgo の parser と `mergeCompilerOptions`）。
  - config plan は file 名なしで作れる（tsgo の `ParseJsonConfigFileContent` の `configFileName ""`）：base path に置き、
    config file path も program の config file も持たない。
  - tsgo の API の option の enum は名前で持つ。`newLine` と `moduleDetection` は tsgo と converter で番号が違い、P5-2a は
    tsgo の CRLF（1）を LF と読んでいた。
  - 位置の無い diagnostic の pos と end は -1（tsgo の undefined range。P5-2a は 0 にしていた）。
- **tsgo と違う所**：
  - ES5 の target と AMD／System の module：port は下げて出す（conformance と suite と同じ意図的な違い）。tsgo 7.1 は ES5 を
    下げず、AMD／System は CommonJS で出す。
  - locale の検査：port は BCP 47 の緩い検査で `xx_YY_bad` を通す。tsgo は Go の `language.Parse`（登録の無い subtag も
    拒む）。P4-6b からの違い。
  - config の list の null の要素：
    - tsgo の値の経路（`parseOwnConfigOfJson`）は null を raw に残し、files・include・exclude・references ごとに
      `Compiler option '{0}' requires a value of type {1}` を出す（`getPropFromRaw`）。port は値の JSON text を parse するので、
      config file と同じく null を落とす。client の test `parseJsonConfigFileContent reports null array elements` はこれで
      失敗する。
    - config file でも、null だけの list は Go の nil slice になる。tsgo は raw と option に `[]` と書き、`paths` の key も
      残す。port は null と書き、key を落とす。
    - references の null の要素の後の error を、tsgo は位置なしで出す（raw の index で AST の要素を引く）。
  - JavaScript の file の JSDoc の reparse（P4-7d の encoder の制限）：`createSourceFile` の JS の file は tsgo と違う。
  - tsgo の lease は parse cache を program と共有する（Go の subtest `shares parse cache with programs`。LSP の session と
    pointer の同一性を使う）。これは移さない。
- **test**：
  - api の unit：session の request 11。`TestCreateSourceFile` の 6 subtest の移植（parse cache の共有の 1 を除く）と、
    command line、config file、config の値、transpile。tsgo の response に固定した。
  - program の `go_json` 1（API の enum を名前で持つ）。
- **tsgo との比較**（local、MessagePack と JSON-RPC）：
  - P5-3a の request 56 行は全て一致。
  - command line と transpile の 38 行は 35 が一致。残り 3 は上の意図的な違い（AMD、System）と locale。
  - `createSourceFile` の 13 の text は 12 が binder の flag を含めて byte まで一致。残り 1 は JSDoc の reparse。
  - JSON の値の config の端（directory だけ、無い extends、include の検査、paths）は一致。
  - P5-2a の snapshot の transcript は一致のまま。
- **TypeScript の client の test**（local）：tsc-rs 42/327（P5-2b は 21）。残り 285 のうち 284 は未実装の method、1 は上の
  null の要素。
- **検証**（最終 bytes。コード `bcf05f435`。macOS、`nice -n 20`、Cargo の job 2）：
  - `cargo fmt --all -- --check` と Clippy（api・program・syntax・cli・conformance、`--all-targets -- -D warnings`）は clean。
  - `cargo test` は全て成功：api（unit 126、`callback_fs` 7、`ipc` 14、`server` 2、encoder 5、fixture 1）、program（unit 71、
    lib の test 513 ほか）、syntax（229 ほか）、cli の contract 264、compiler の contract 153 と `system` 9。
  - command line の parser、config の plan、JSON の parse は batch の経路に触れるので、release build（2m17s）の full
    conformance を local で 1 回（`--workers 2 --check`、466s）：0 regressions、accepted tier を超える構成 0。errors full
    13,451、emit 13,443、types 12,678（mismatch 89）、symbols 12,718（49）、sourcemap 13,451、trace 13,451、harness
    error 15（main と同じ）。
  - suites（`scripts/suites_ts71.py --check`）：0 regressions。api 2、config 87、transpile 41、tsoptions 80、tscWatch 42 は
    全て full、tsc 211/223、tsbuild 182/192、tsbuildWatch 63/65 は main と同じ。
  - workspace 全体の test と Clippy は hosted の `rust` job に任せた。
- **残り**：P5-3b（program の情報：`getSourceFile` ほか。bind 済みの program の file を、program が集めた import と binder の
  flag で encode する）、P5-3c（module resolver）。

### P5-3a の hosted の記録と merge（2026-10-10）

- hosted：最終候補 `93b470a86`（コード `bcf05f435`、packet の記録）の run 37987382457（`plan` 33s、`rust` 8m12s、
  `conformance (TypeScript 7.1)` 19m21s、`gates` 13s。全て成功。workspace 全体の test と Clippy はこの `rust` job による）。
  merge → `7ba946f74`（merge commit、PR #728）。
- 次：P5-3b（program の情報）。

## P5-3b program の情報（2026-10-10）

- **request**（tsgo `api/session.go` 1971–2283）：
  - `getSourceFile`：project の program の file を、tsgo の parse cache が持つ形で encode する（program が集めた import、binder
    の flag、module detection）。program に無い file は MessagePack で空の binary、JSON で null。
  - `getSourceFileNames`（program の順、library が先）。
  - `getSourceFileMetadata`：default library か、node_modules を探して見つけた file か、package scope の type と directory、
    implied node format。type は tsgo の `loadSourceFileMetaData` のとおり、拡張子で format が決まらない file が node16〜
    nodenext で解決されるとき、または node_modules の file のときだけ書く。
  - `getConfigFileNames`：config と extends の file。config の無い program は `[]`（Go の nil slice）。
  - `getConfigSourceFile`：config か extends の config を tsconfig source として parse して encode する（content hash なし）。
  - `getModeForUsageLocation`、`getModeForResolutionAtIndex`、`getResolvedModule`（＋`FromModuleSpecifier`）、
    `getResolvedTypeReferenceDirective`（＋`FromTypeReferenceDirective`）。
  - node は handle（`index.kind.path`。index は encoder の node index table のもの、path は file の path）で指す。
  - file 名は project の directory からの相対（tsgo の program が解決するとおり）。
- **既存の経路の変更**（いずれも tsgo の動作に合わせる）：
  - source file は parse したときの module detection（tsgo の `ExternalModuleIndicatorOptions`：JSX と Force）を持ち、
    encoder はそれを header に書く。program の JSON file も同じ option で parse する（tsgo は nodenext の JSON file の header
    に Force を書く）。
  - prepared file は tsgo の `sourceFilesFoundSearchingNodeModules` を持つ。loader は `may_be_emitted` に畳んでいた。
  - checker は API の `GetModeForUsageLocation` と `GetDefaultResolutionModeForFile` を答える。tsgo では program の関数で、
    port では同じ規則が checker にある。
  - 修正（P5-1）：program が missing の directory として探した path が作られると、project を dirty にする。tsgo の
    `SeenFileOrMissingParentDirectory` は path 自身から確かめるが、port は親から確かめていた。client の test
    `Snapshot.update host symlinks bypass an inherited full filesystem`（missing の node_modules に host の link を作る）で
    見つかった。
- **test**：
  - api の unit：program の情報 7（file の順と config file、metadata、encode の header、mode と解決、node handle と error、
    project と file の error、host の link）。
  - project の unit 1（missing の directory）。
- **tsgo との比較**（local）：
  - P5-3b の request 50 行（MessagePack と JSON-RPC）：全て一致。`getSourceFile` と `getConfigSourceFile` の encode は
    byte まで一致。
  - node handle の走査（2 つの file の index 0〜59、configured・synthetic・inferred の project）790 行：全て一致。
  - host の link の場面：一致。
  - P5-3a、P5-2a、P5-2b の比較は変わらない（P5-2b の 2 行は記録済みの Go の map の順）。
- **TypeScript の client の test**（local）：tsc-rs 96/327（P5-3a は 42）。残り 231 のうち 230 は未実装の method、1 は
  P5-3a の null の要素。
- **検証**（最終 bytes。コード `f96644823`。macOS、`nice -n 20`、Cargo の job 2）：
  - `cargo fmt --all -- --check` と Clippy（api・program・syntax・checker・project・cli、`--all-targets -- -D warnings`）は
    clean。
  - `cargo test` は全て成功：api（unit 133、`callback_fs` 7、`ipc` 14、`server` 2、encoder 5、fixture 1）、project（unit 11、
    snapshot 41）、program、syntax、cli の contract 264、compiler の contract 153 と `system` 9。
  - syntax の source file、checker の program の parse、loader は batch の経路なので、release build（2m18s）の full
    conformance を local で 1 回（`--workers 2 --check`、473s）：0 regressions、accepted tier を超える構成 0。errors full
    13,451、emit 13,443、types 12,678（mismatch 89）、symbols 12,718（49）、sourcemap 13,451、trace 13,451、harness
    error 15（main と同じ）。
  - suites（`scripts/suites_ts71.py --check`）：0 regressions、数は main と同じ。
  - workspace 全体の test と Clippy は hosted の `rust` job に任せた。
- **残り**：P5-3c（module resolver）。

### P5-3b の hosted の記録と merge（2026-10-10）

- hosted：最終候補 `71d343d14`（コード `f96644823`、packet の記録）の run 37993238150（`plan` 29s、`rust` 7m50s、
  `conformance (TypeScript 7.1)` 20m26s、`gates` 14s。全て成功。workspace 全体の test と Clippy はこの `rust` job に
  よる）。merge → `b3c64500d`（merge commit、PR #729）。
- 次：P5-3c（module resolver）。

## P5-3c module resolver（2026-10-10）

- **request**（tsgo `api/module_resolution.go`、`module/staticresolver.go`）：
  - `createModuleResolver`：compiler option、static resolutions（`fallback` は resolve か unresolved、entry は moduleName と、
    あれば containingDirectory・resolutionMode、result）、callback の名。tsgo の client error（fallback、null の entry、空の
    moduleName、result なし、resolutionMode、重複）。
  - `releaseModuleResolver`。
  - `resolveModuleName`：static の entry（directory と mode、directory、mode、名前だけの順に最も具体的なもの。fallback が
    unresolved なら未解決）、次に callback、callback が無ければ既定の resolver。既定の resolver は resolver の option で、
    snapshot（読んだものを持つ）、作っている途中の snapshot、または host の file system の上で解決し、option が求めれば trace
    を返す。
  - program：`createPrograms` の `moduleResolver` で作った program は、static と callback を先に引き、その後に program の
    resolver を resolver の option で使う（tsgo の factory）。callback は作っている途中の snapshot の番号
    （`inProgressSnapshot`）を受け取る。callback の失敗は snapshot の作成を失敗させる（`failed to create snapshot: ...`）。
    同じ resolver で reconfigure した program は同じ program のまま。
- **既存の経路の変更**：
  - `ModuleResolver` は program の option にある module resolution の override（`ModuleResolutionOverride`）を先に引く。
    override が option を持つときはその option で解決する（tsgo の factory は resolver の option を使う）。directory からの
    解決（tsgo `ResolveModuleNameFromDirectory`）も持つ。
  - 作り終えた snapshot は自身の file を file system として見せる（tsgo は snapshot を resolver の host にする）。
- **tsgo と違う所**：
  - tsgo は program を作るたびに resolution の文脈（番号）を作り、作り終えると解放する。port は request ごとに 1 つ持ち、
    callback を最初に呼ぶときに番号を振る。作っている途中の snapshot の file system は、request の file system（または
    host）。
- **test**：api の unit：module resolver 9。
  - tsgo の 4 test の移植（LSP の snapshot を使う 1 は除く）。
  - callback の 1：script した client で、tsgo の error の test に当たる。
  - client の test にある振る舞い 3：snapshot の読み、同じ resolver の reconfigure、resolver の option。
  - client error の 1。
- **tsgo との比較**（local、MessagePack、callback を答える client）：40 行全て一致（static、callback、trace、error、
  program の file 名、callback の失敗）。P5-3b までの比較も変わらない。
- **TypeScript の client の test**（local）：tsc-rs 103/327（P5-3b は 96）。残り 224 のうち 223 は未実装の method、1 は
  P5-3a の null の要素。
- **検証**（最終 bytes。コード `a59b4cde2`。macOS、`nice -n 20`、Cargo の job 2）：
  - `cargo fmt --all -- --check` と Clippy（api・program・project・cli、`--all-targets -- -D warnings`）は clean。
  - `cargo test` は全て成功：api（unit 142、`callback_fs` 7、`ipc` 14、`server` 2、encoder 5、fixture 1）、program、project、
    cli の contract 264、compiler の contract 153 と `system` 9。
  - module の解決（`ModuleResolver`）は batch の経路なので、release build（2m15s）の full conformance を local で 1 回
    （`--workers 2 --check`、469s）：0 regressions、accepted tier を超える構成 0。errors full 13,451、emit 13,443、types
    12,678（mismatch 89）、symbols 12,718（49）、sourcemap 13,451、trace 13,451、harness error 15（main と同じ）。
  - suites（`scripts/suites_ts71.py --check`）：0 regressions、数は main と同じ。
  - workspace 全体の test と Clippy は hosted の `rust` job に任せた。
- **残り**：P5-3 はこれで終わる。次は P5-4（checker の query）の計画。

### P5-3c の hosted の記録と merge（2026-10-10）

- hosted：最終候補 `69114cb22`（コード `a59b4cde2`、packet の記録）の run 37998538106（`plan` 30s、`rust` 9m20s、
  `conformance (TypeScript 7.1)` 20m34s、`gates` 20s。全て成功。workspace 全体の test と Clippy はこの `rust` job に
  よる）。merge → `cfbd71ecf`（merge commit、PR #730）。
- P5-3（project の要らない request、program の情報、module resolver）はこれで終わる。次は P5-4（checker の query）の計画。

## P5-4 checker の query の計画（2026-10-10）

### tsgo の構成（`19dadef8`）

- `setupChecker(snapshot, project)` で project の program の API checker（P5-1d）を取る。query は handle を使う：symbol は
  snapshot の registry（`ast.GetSymbolId`）、type と signature は project の registry（checker の id）。handle は数で、空か
  未登録なら client error。
- response：`SymbolResponse`（id、project、name、flags、checkFlags、declarations と valueDeclaration の node handle、parent、
  exportSymbol）、`TypeResponse`（flags、objectFlags、literal の value、target、type parameter、tuple、indexed access、
  conditional、substitution、mapped、template literal、freshable、this、intrinsic、alias、symbol）、`SignatureResponse`、
  `IndexInfo`。
- method（約 110）：
  - **symbol**：`getSymbolAtPosition`(s)、`getSymbolAtLocation`(s)、`getSymbolOfSourceFile`(s)、parent・members・exports・export
    symbol、alias（aliased・immediate・target・export specifier の local target）、shorthand assignment、fully qualified name、
    module の exports、`resolveName`、`getSymbolsInScope`、`isReadonlySymbol`、`getConstantValue`、well-known symbol。
  - **type**：`getTypeOfSymbol`(s)、declared・non-missing・at location の type、`getTypeAtLocation`(s)、`getTypeAtPosition`(s)、
    `getTypeFromTypeNode`、type の部分（target、types、type parameter、this、alias、indexed access・conditional・mapped の部分、
    fresh／regular、symbol）、base types、properties、index infos、apparent・reduced・widened・non-nullable・awaited・literal の
    base、constraint・default、type arguments、contextual type、assignability、array（like）、context sensitive、intrinsic の
    12、`typeToString`。
  - **signature**：signatures of type、resolved signature、declaration からの signature、type parameter・parameter・this・
    target・return・rest・type predicate、parameter type、well-known signature。
  - **node builder**：`typeToTypeNode`、`signatureToSignatureDeclaration`（作った node を encode する）。
  - **diagnostics**：syntactic・bind・semantic・suggestion・declaration・program・global・config file parsing。
  - **emit**：`emit`、`emitToString`、`getJavaScriptEmit`、`getDeclarationEmit`。
  - LS（completion、references、signature usage、import adder、documentation comment・JSDoc tag）と profile（Go の
    profiler）。

### port の状態

- checker：P5-1d の API checker（`LiveProgram::with_checker`）。types と symbols の baseline のために、型の文字列と symbol
  の照会は既にある。
- flag：`TypeFlags` は tsgo と同じ並び。`SymbolFlags` は 28/32 が同じで、違うのは内部の 3 と `All`。`ObjectFlags`（違う
  ものが 19）、`CheckFlags`（16）、`SignatureFlags`（7）は並びが違うので、名前で tsgo の番号に写す。type の cache を表す内部の
  bit は port に無いものがある。
- position からの node（tsgo `astnav.GetTouchingPropertyName`）は移す。

### slice

- **P5-4a handle と symbol**：registry、position と node handle からの symbol、symbol の response と flag の写し、symbol の
  照会、`getTypeOfSymbol` 系と `getTypeAtLocation` 系、type の response（基本の field）、`typeToString`、intrinsic の type。
  client の test で最初に足りない method が `getSymbolAtPosition` のものは 90。
- **P5-4b type の構造**：type の部分の照会、base・properties・index info・apparent・reduced・widened・awaited ほか、
  assignability、contextual type。
- **P5-4c signature と node builder**：signature の照会、`resolveName`・`getSymbolsInScope`、well-known、
  `typeToTypeNode`・`signatureToSignatureDeclaration`。
- **P5-4d diagnostics と emit**。
- LS の handler と profile は LSP の作業に回す（それまでは未実装の error のまま）。P5-4 の後は P5-5（`printNode` と decoder）
  と P5-6（build orchestrator）。

## P5-4a handle と symbol（2026-10-10）

- **handle**（tsgo `snapshotData` の registry と `checkerSetup`）：
  - symbol：tsgo は symbol を最初に聞かれたときに global の counter で番号を振る（`ast.GetSymbolId`）。二つの program が
    共有する source file の binder の symbol は一つの番号になる。port の binder の symbol の番号は file の identity の中の
    位置で、file を共有する program は同じ番号を持つ。checker の symbol（transient）は checker ごと。そこで snapshot は、
    symbol を最初に聞かれたときに、binder の symbol は宣言した file で、checker の symbol は program で見分けて番号を振る
    （session の counter）。registry は handle から symbol と最初に渡した project を引く（tsgo
    `symbolCanonicalProjects`）。他の project で聞かれた handle は、binder の symbol ならその project も同じ file を持つとき
    だけ、checker の symbol なら同じ program のときだけ答える（tsgo は pointer をそのまま別の checker に渡す。ここは
    registry の error になる）。
  - type と signature：checker の番号（`TypeId` と `SignatureId` の index + 1）、project ごとの registry（tsgo と同じ）。
  - node handle：`index.kind.path`。index と kind は encoder の node の表（tsgo `GetNodeIndexTable`、kind は tsgo の番号）。
    表は snapshot ごとに source file の単位で持つ。
  - error は tsgo の文（empty handle、registry にない、no registry for project、invalid node handle など）。
- **response**：`SymbolResponse`（name は tsgo `EscapeSymbolName` と同じ escape 済みの名）、`TypeResponse`（literal の
  value、fresh・regular、class・interface の type parameter と this、tuple の target、indexed access・conditional・
  substitution・mapped の部分、template literal の text、intrinsic の名、alias、symbol）、`ConstantValueResponse`。Go の
  `encoding/json/v2` の `omitempty` は `false` と `0` を残し（`objectFlags`、`isTupleType`、`isThisType` は常にある）、
  `value` は無ければ `null`、nil の slice は `[]` になる。数値は Go の float の書き方（JavaScript の形。`-0` は符号を
  残し、無限大と NaN は文字列）。
- **flag の写し**：`SymbolFlags` は tsgo の 28 bit と同じで、tsgo の `ConstEnumOnlyModule`・`ReplaceableByMethod` は
  port の symbol の別の field から。`CheckFlags` は名前で写す（tsgo の `IsDiscriminant*`・`IndexSymbol` は port に無い）。
  `ObjectFlags`（object の型）は 21 bit が同じ、`MembersResolved` は checker が member を解決したか、その後の flag は名前で
  写す。
- **position**：tsgo `astnav`（`GetTouchingPropertyName`、`getTokenAtPosition`、`FindPrecedingTokenEx`、
  `findRightmostValidToken`）を移した（`crates/api/src/astnav.rs`）。木は encoder の view（tsgo の形の木）で、JSDoc を
  子の前に、一つの部分だけの JSDoc の comment は訪れない。木に無い token は scanner で読む（syntax に `TokenScanner` と
  tsgo `SkipTriviaEx` の option を足した。`skip_trivia` は既定の option の `skip_trivia_ex`）。position は UTF-16 から
  tsgo `PositionMap.UTF16ToUTF8` で byte にする。
- **method**（44）：`getSymbolAtPosition`(s)、`getSymbolAtLocation`(s)、`getSymbolOfSourceFile`(s)、
  `getTypeOfSymbol`(s)、`getDeclaredTypeOfSymbol`、`getNonMissingTypeOfSymbol`、`getTypeOfSymbolAtLocation`、
  `getTypeAtLocation`(s)、`getTypeAtPosition`(s)、`getParentOfSymbol`、`getMembersOfSymbol`、`getExportsOfSymbol`、
  `getExportSymbolOfSymbol`、`getSymbolOfType`、`typeToString`、intrinsic の 12、`getAliasedSymbol`、
  `getImmediateAliasedSymbol`、`getTargetSymbol`、`getExportSymbolOfSymbolForChecker`、
  `getExportSpecifierLocalTargetSymbol`、`getShorthandAssignmentValueSymbol`、`getFullyQualifiedName`、
  `getExportsOfModule`、`getMemberInModuleExports`、`isReadonlySymbol`、`getConstantValue`。
- **checker**：tsgo `checker/exports.go` に当たる `exports.rs`（intrinsic の型、`CompareSymbols` の並べ替え、
  `TypeToStringEx`（node builder で作って 1 行で印字、unresolved の型だけ comment を残し、tsgo の長さで切る）、mapped の
  部分、export specifier の local の target、shorthand、module の exports、`GetTypeOfSymbolAtLocation`、
  `GetConstantValue`、木に無い token の型）。
- **既存の経路の変更**（tsgo に合わせた。types・symbols の baseline の walker が聞かない node で違いが出た）：
  - `getTypeFromTypeNode` の identifier・qualified name・property access の arm（tsc 6.0 のもの）を削った。tsgo 7.1 には
    無く、型の位置の名前は error 型になる（unresolved 型でもなくなる）。
  - `IsDeclaration` を tsgo の `IsDeclarationNode`（declaration の data を持つ node）にした。source file（module の
    `typeof`）、`export =`、call signature などが加わる。
  - `getSymbolAtLocation` の token の arm を `get_symbol_at_token` に分けた（木に無い token も同じ arm で答える）。型の中の
    `this`（木では ThisType の node）の arm を足した。JSDoc の `@param` の名の arm（tsgo `GetNodeAtPosition`）を移した。
  - `getNameOfSymbolAsWritten` の名の無い宣言の arm（変数の名、`(Anonymous class)`、`(Anonymous function)`）を足した
    （`getFullyQualifiedName` に出る）。
  - `import.meta` の `meta` の symbol に tsgo の `Readonly` の check flag を付けた。
  - pattern の ambient module の import attributes 付きの名を tsgo の形（`__"*.css"pattern@<attributes の node の番号>`）に
    した（以前は file 名も入れていた）。
- **vendor**：tsgo の astnav の baseline（`testdata/baselines/reference/astnav`、7）とその file
  （`testdata/fixtures/services/mapCode.ts`）を 7.1 の profile に加えた（`scripts/vendor_typescript_native.py`、manifest、
  harness の数の contract）。
- **test**：
  - api の unit：astnav 3（tsgo の Go baseline `GetTouchingPropertyName`・`GetTokenAtPosition`・`FindPrecedingToken` の
    `mapCode.ts` の全ての位置）、checker の session 14（tsgo の応答に合わせた。handle の番号は実装ごとなので、同一性で
    比べる）。
- **tsgo との比較**（local、MessagePack、番号を初出順に付け直して比べる）：
  - fixture の 3 種（位置からの symbol と型を全ての位置で、出てきた symbol と型を各 query に通す）：mapCode.ts 2,713 行は
    全て一致。TS・TSX・JS・非 ASCII・多様な構文の fixture 4,614 行は、下の bounded の初めの二つを除いて一致。
  - 個別の probe 99 行と 55 行は全て一致（tsgo が panic の error に Go の stack を足すのを除く）。
- **TypeScript の client の test**（local）：tsc-rs 168/327（P5-3c は 103）。残り 159 のうち 158 は未実装の method
  （error の文を確かめる 2 を含む）、1 は P5-3a の null の要素。
- **tsgo と違う所**（bounded）：
  - private name の symbol の名の番号：tsgo は class の symbol の global の番号（`__#1@#secret`）、port は binder の
    serial（`__#147833@#secret`）。
  - JavaScript の JSDoc（`@typedef` など）：tsgo は reparse した node を宣言にする（P4-7d の encoder と同じ bounded）。
  - 別の project の checker の symbol を聞くと registry の error（tsgo は pointer を渡す）。
  - file の無い program（checker が無い）への query は client error（`project has no checker`）。
- **検証**（コード `80841b9e8`。macOS、`nice -n 20`、Cargo の job 2）：
  - `cargo fmt --all -- --check` と workspace の Clippy（`--all-targets -- -D warnings`。cache を足した後は api・cli・
    conformance）は clean。
  - `cargo test`：api（188）、syntax（248）、binder（78）、project（52）、harness の vendored manifest の contract。
  - checker の経路を変えたので、release build（2m18s）の full conformance を local で 1 回（`--workers 2 --check`、
    472s）：0 regressions、accepted tier を超える構成 0。errors full 13,451、emit 13,443、types 12,678（mismatch 89）、
    symbols 12,718（49）、sourcemap 13,451、trace 13,451、harness error 15（main と同じ）。
  - suites（`scripts/suites_ts71.py --check`）：0 regressions、数は main と同じ（api 2/2 ほか）。
  - conformance の後に、API の session の query の cache（program の file の path と source file の position の表を
    snapshot に持つ。`crates/api/src/checker.rs` だけ。runner は通らない）を足した。その後に api の test、比較、client の
    test をやり直し、結果は同じ。parallel control（`--checkers 4`）は local では走らせていない。
  - workspace 全体の test は hosted の `rust` job に任せる。
- **残り**：P5-4b（type の構造）。

### P5-4a の hosted の記録と merge（2026-10-10）

- hosted：最終候補 `d9f71418e`（コード `80841b9e8`、packet の記録）の run 38015341860（`plan` 29s、`rust` 11m59s、
  `conformance (TypeScript 7.1)` 13m4s、`gates` 14s。全て成功。workspace 全体の test と Clippy はこの `rust` job に
  よる）。merge → `9800f6b01`（merge commit、PR #731）。
- 次は P5-4b（type の構造）。

## P5-4b type の構造（2026-10-10）

- **method**（46）：
  - type の部分（tsgo `resolveTypePropertyOfType` ほか、`objectId` を取る 20）：`getTargetOfType`、
    `getFreshTypeOfType`、`getRegularTypeOfType`、`getTypesOfType`、`getTypeParametersOfType`、
    `getOuterTypeParametersOfType`、`getLocalTypeParametersOfType`、`getThisTypeOfType`、`getAliasTypeArgumentsOfType`、
    `getAliasSymbolOfType`、`getObjectTypeOfType`、`getIndexTypeOfType`、`getCheckTypeOfType`、`getExtendsTypeOfType`、
    `getBaseTypeOfType`、`getConstraintOfType`、mapped type の 4（`getTypeParameterOfMappedType` ほか）。
  - checker の照会（26）：`getBaseTypes`、`getPropertiesOfType`、`getApparentPropertiesOfType`、`getApparentType`、
    `getReducedType`、`getIndexInfosOfType`、`getIndexInfoOfType`、`getConstraintOfTypeParameter`、
    `getDefaultFromTypeParameter`、`getBaseConstraintOfType`、`getPropertyOfType`、`getTypeOfPropertyOfType`、
    `getTypeArguments`、`getTrueTypeOfConditionalType`、`getFalseTypeOfConditionalType`、`getAwaitedType`、
    `getBaseTypeOfLiteralType`、`getNonNullableType`、`getTypeFromTypeNode`、`getWidenedType`、`isArrayType`、
    `isArrayLikeType`、`isTypeAssignableTo`、`getContextualType`、`getContextualTypeForArgument`、`isContextSensitive`。
- **tsgo の型の accessor**（checker の `exports.rs`）：tsgo `types.go` の `Type.Target`（reference の target。class・
  interface・tuple の target は自身、instantiate した object 型と clone した型 parameter は元、index・string mapping は
  operand）、`Type.Types`、`LiteralType` の fresh・regular、`AsInterfaceType` の型 parameter と this（thisless の interface
  は両方無い）、indexed access・conditional・substitution の部分、mapped type の部分（checker が解決した分だけ。tsgo の
  field と同じ）。型の種類が合わないと tsgo は panic し、API はそれを request の error にする（`Unhandled case in
  Type.Target`、Go の型 assertion の `interface conversion: checker.TypeData is *checker.UnionType, not
  *checker.LiteralType`、nil の参照の `runtime error: invalid memory address or nil pointer dereference`）。port も同じ文
  で panic する。Go の data の型は flag から決める（object 型は tsgo `newObjectType` の object flag の順）。
  `GetTypeArguments` は reference（class・interface・tuple の target を含む）以外で、`GetApparentProperties` は
  apparent type の property に `CallableFunction`・`NewableFunction` の property を足した named member。
- **response**：`IndexInfoResponse`（`isReadonly` は false も書き、`declaration` は無ければ省く）。型の配列と symbol の
  配列は空なら `[]`（Go の nil の slice）。
- **名**：tsgo の symbol の名は escape しない。名で引く request（`getPropertyOfType`、`getTypeOfPropertyOfType`、
  P5-4a の `getMemberInModuleExports`）は tsc の escape（`escapeLeadingUnderscores`）を通して引く。応答の名は tsgo
  `EscapeSymbolName` と同じ：port は late-bound の名を tsgo の内部の接頭辞（U+FFFD）で持つので、`__@` に書く（P5-4a で
  漏れていた）。
- **`typeToString`**：unresolved 型の `/*unresolved*/` の comment は enclosing declaration があるときだけ（tsgo の
  printer は source file が無いと comment を書かない。P5-4a は常に残していた）。
- **既存の経路の変更**（tsgo に合わせた。walk で tsgo と違った所）：
  - union・intersection の property を並べ替えない。tsc 6.0.3 の stableTypeOrdering は `getNamedMembers` で並べ替えたが、
    tsgo は最初の構成型の property の順のまま（`checker.go:19201-19225`）。
  - `getBaseTypes` の入口を tsgo の `ClassOrInterface | Tuple` にした（以前は `ClassOrInterface | Reference`。tsc 6.0 は
    参照も受けた）。参照を渡していた `typeHasProtectedAccessibleBase` は tsgo と同じく参照の target の base を見る（full
    conformance で `protectedAccessibilityCheck` が落ちて分かった。他の呼び出し元は宣言の型か target を渡す）。
  - `getDefaultFromTypeParameter` は型 parameter 以外に null（tsgo）。`isContextSensitive` の assert を外した（tsgo に
    無い）。`resolveSignature` の panic の文を tsgo の `Unhandled case in resolveSignature` にした。
  - node builder：切り詰めの member（`...`、`... N more ...`）を identifier で作る（以前は identifier にできない名として
    string literal にし、`"..."` と印字した）。条件型は切り詰めの長さを超えていたら `...`（tsgo
    `conditionalTypeToTypeNode` の入口）。
  - decorator の getter・setter・field の初期化子の mutator の関数型を `getOrCreateTypeFromSignature` で作る（tsgo
    `newFunctionType`。`Anonymous | SingleSignatureType` で signature に memo する。以前は `SingleSignatureType` が無かった）。
- **test**：api の unit 8（`crates/api/tests/unit/session_types.rs`。tsgo の応答に合わせた。型の部分と panic、literal、
  class・interface・base type・type argument、structured type の部分、property・index info・union の property の順、
  apparent・awaited・non-nullable、array と assignability、contextual type と type node）。
- **tsgo との比較**（local、MessagePack、番号を初出順に付け直して比べる）：fixture の 3 種（P5-4b の `types`（型の構造が
  多い TS）、P5-4a の mapCode.ts、P5-4a の basic から JS を除いた 6 file）で、全ての位置の symbol と型、全ての node への
  `getTypeFromTypeNode`・`getContextualType`・`isContextSensitive`（呼び出しの node には `getContextualTypeForArgument`）、
  symbol の型、見つけた型（幅優先、上限 3,000）の全てに 46 method と `typeToString` を送る。336,913 行のうち違うのは
  46 行で、全て下の bounded（source file の node 14、EOF の token 7、JSDocText の node 9、`?` を含む型の
  `typeToString` 16）。
- **TypeScript の client の test**（local）：tsc-rs 213/327（P5-4a は 168）。残り 114 のうち 111 は未実装の method、
  2 は未実装の method の error の文を確かめる test、1 は P5-3a の null の要素。
- **tsgo と違う所**（bounded）：
  - source file の node への `getTypeFromTypeNode`・`getContextualType`、EOF の token への `getTypeFromTypeNode`：tsgo は
    親（nil）を参照して panic する。tsc-rs は error 型と null。
  - encoder が JSDoc の comment の text から作る JSDocText の node（tsc-rs の木に無い）の handle は解決できない（client
    error）。
  - API からしか作れない型（tuple の target の this 型など、symbol の無い型 parameter `?` を含む型を `getAwaitedType` など
    で包んだもの）の `typeToString`：tsgo は長さの見積もりから union の後の member の型引数を `...` にする。
  - `getContextualTypeForArgument` の負の index：tsgo は import の呼び出しなら `any`、他は Go の index out of range の
    panic。tsc-rs は常にその panic。
  - late-bound の名の番号：private name と同じく各実装の symbol の番号（tsgo `__@iterator@24`、tsc-rs
    `__@iterator@135732`）。
  - tsc-rs の server は request の panic（error として答える）を Rust の panic hook で stderr にも書く（P5-2a から）。tsgo
    は書かない。
- **検証**（コード `2eb669e67`。macOS、`nice -n 20`、Cargo の job 2）：
  - `cargo fmt --all -- --check`、Clippy（`-p tsc-rs-checker -p tsc-rs-api --all-targets -- -D warnings`）は clean。
  - `cargo test`：checker（1,794）、api（196。P5-4b の 8 を含む）。
  - checker の経路を変えたので、release build の full conformance を local で（`--workers 2 --check`）。最初の実行（474 s）で
    regression 1（`compiler/protectedAccessibilityCheck`。上の `typeHasProtectedAccessibleBase`）。直した最終のコードで
    471 s：0 regressions、accepted tier を超える構成 0。errors full 13,451、emit 13,443、types 12,678（mismatch 89）、
    symbols 12,718（49）、sourcemap 13,451、trace 13,451、harness error 15（main と同じ）。
  - suites（release の `suites-ts71` を作り直して `scripts/suites_ts71.py --check`）：0 regressions、数は main と同じ（api
    2/2、config 87、transpile 41、tsbuild 182／192、tsbuildWatch 63／65、tsc 211／223、tscWatch 42、tsoptions 80）。
  - 並列対照（`--checkers 4`、471 s、`scripts/conformance_ts71_compare.py`）：15,224 構成が一致、違う 4 構成は記録済みの
    partition 依存の構成（`mutuallyRecursiveInference`、`incorrectRecursiveMappedTypeConstraint`、
    `typeParameterWithInvalidConstraintType`、`recursiveMappedTypes`）。
  - 上の tsgo との比較と client の test は最終のコードでやり直した（結果は同じ）。
  - workspace 全体の test と Clippy は hosted の `rust` job に任せる。
- **残り**：P5-4c（signature と node builder）。

### P5-4b の hosted の記録と merge（2026-10-10）

- 最初の hosted run 38024526273（head `8d1541e12`）は `rust` で cli の contract test
  `incremental_steps_explain_references` が落ちた。hosted の Linux では続けて書いた fixture の 2 file が同じ更新時刻になり、
  `tsc -b` が同じ時刻の入力のうち先に見た file を最新と報告した（tsgo の記録は file ごとに時刻が進む所で作った）。
  `write_fixture_files` が同じ時刻の file を書いた順に 1 µs ずつずらすように直した（`b0e21f7e4`。local：cli の contracts
  264 passed、Clippy clean）。この run は `conformance` の途中で止めた。
- 同じ branch で、利用者の指示による README の整理（`8d1541e12`。短い利用者向けの案内にし、詳しい内容を
  `docs/performance.md`・`docs/rust-api.md`・`docs/setup.md` に移した。性能は tsc 6.0.3 互換の build での計測と注記）と
  CLAUDE.md の整理（`82a5d67e3`。repository から読み取れない約束だけを残した）も入れた。
- hosted：最終候補 `82a5d67e3` の run 38025264190（`plan` 32s、`rust` 7m34s、`conformance (TypeScript 7.1)` 14m1s、
  `gates` 12s。全て成功。workspace 全体の test と Clippy はこの `rust` job による）。merge → `fa3ac45cd`（merge commit、
  PR #732）。
- 次は P5-4c（signature、`resolveName`・`getSymbolsInScope`、well-known、`typeToTypeNode`・
  `signatureToSignatureDeclaration`）。
