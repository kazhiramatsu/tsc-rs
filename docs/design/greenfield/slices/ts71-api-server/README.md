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
