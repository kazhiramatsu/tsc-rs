# LSP：計画（2026-10-13）

利用者の順序（2026-10-12）：loader の修正、content mapper、その後に LSP。[post-emitter roadmap](../../post-emitter-roadmap.md)
の P5（Language Service と native LSP の batch：LS-BOOT、LS-FIRST、LS-COMPLETE、LS-NAV、LS-EDIT、LS-PROJECT、LS-CLOSE）を、
tsgo `aa8149273` の実物に合わせて具体化する。[content mapper](../ts71-content-mapper/README.md) の LS の部分と、
[API server](../ts71-api-server/README.md) で LS に回した handler もここに入れる。

## tsgo の LSP

- **入口**：`tsgo --lsp --stdio`（`cmd/tsc/lsp.go`）。stdio だけ（`--pipe`・`--socket` は受け取るが使わない）。
  `--clientProcessId` は親 process の監視（5 秒ごと）。server は `lsp.NewServer` に FS（bundled の lib を被せた OS の FS）、
  typings の場所、npm の実行（ATA）、content mapper の spawner、progress の遅延（250 ms）を渡す。
- **transport**：`Content-Length` の JSON-RPC（`internal/jsonrpc`）。protocol の型は LSP の meta model 3.18 から生成した
  `lsproto/lsp_generated.go`（17,538 行、599 型、request 81、notification 23。生成器は `tools/scripts/lsp/generate.mts`、
  meta model は vscode-languageclient の版の `metaModel.json`）。
- **request の流れ**（`server.go`）：読む loop、順に処理する dispatch loop、書く loop の三つ。dispatch loop は handler の同期の
  部分（snapshot の取得まで）を順に走らせ、LS の呼び出しは非同期に走らせる。`$/cancelRequest` は dispatch loop が拾った request
  の context を取り消し、`RequestCancelled` で答える。未知の request は `InvalidRequest`。handler の panic は `InternalError`。
- **method**：54。LS の document request 22（diagnostic（pull）、hover、definition、typeDefinition、signatureHelp、format 三種、
  documentSymbol、documentHighlight、selectionRange、inlayHint、codeLens、prepareCallHierarchy、foldingRange、prepareRename、
  linkedEditingRange、semanticTokens 二種、`custom/textDocument/sourceDefinition`、`custom/textDocument/multiDocumentHighlight`、
  `textDocument/_vs_onAutoInsert`）、auto-import の再試行つき 2（completion、codeAction）、複数 project 3（references、
  implementation、`_vs_references`）、同期の request 15（initialize、shutdown、rename、willRenameFiles、call hierarchy 二種、
  workspace/symbol、completionItem/resolve、codeLens/resolve、`custom/*` の profile 五種、`custom/initializeAPISession`、
  `custom/projectInfo`、`custom/setContentMapperContributions`）、通知 10（initialized、exit、didChangeConfiguration、didOpen、
  didChange、didSave、didClose、didChangeWatchedFiles、`$/setTrace`、`custom/setLogVerbosity`）。server から client へは
  registerCapability、workspace/configuration、各種 refresh、progress、publishDiagnostics（tsconfig の診断だけ。file の診断は
  pull）、logMessage、telemetry。
- **language service**（`internal/ls`）：31,145 行、`ls/autoimport` 5,507、`ls/change`（change tracker）1,604、`ls/lsconv`（位置
  と URI の変換、content mapper の span の写像）700、`ls/lsutil`（user preferences の生成コードなど）3,340、`internal/format`
  4,257。大きいのは completion（9,768）、find all references（3,532）、hover と signature help（2,618）、code fix（3,028。provider
  は import の追加、isolatedDeclarations の型注釈、interface の実装の 3 種だけ）、organize imports（1,675）。refactor は無い。
- **project の LS の部分**（`project/session.go` 2,151 行）：API の snapshot host の上に、open file の overlay（didOpen は同期、
  didChange は次の request まで溜める、didClose は 500 ms の debounce）、inferred project、background の task（watch の更新、
  tsconfig の診断の push、auto-import の cache の準備）、client への watch の登録、locale、content mapper の contribution、
  ATA（npm で `@types` を入れる）、auto-import の registry、checker pool の query checker。

## 参照（tsgo `aa8149273`）

- `cmd/tsc/lsp.go`、`internal/lsp`（server、progress、lspwatcher、lsproto）、`internal/jsonrpc`。
- `internal/ls` と `ls/{autoimport,change,lsconv,lsutil}`、`internal/format`。
- `internal/project/{session,overlayfs,watch,checkerpool,autoimport}.go`、`project/{ata,background}`。
- `internal/fourslash`（harness 9,612 行、test 4,372 file）、`internal/testutil/lsptestutil`。
- `tools/scripts/lsp`（protocol の生成）、`tools/userPreferences.schema.json`（preferences の生成）。

## 合わせる test

- **fourslash**：4,571 test（4,372 file。383 file は tsgo 自身が known failing で skip）。harness は in-process の server と pipe
  で本物の JSON-RPC を話し、判定の method は 108（completion 1,114 file、find all references 373、definition 241、quick info 322、
  code fix と import の修正 400 余、format 188 など）。baseline は 1,761（21 種）。content mapper の 18 file（56 test）を含む。
- **unit test**：`lsp` 32、`lsproto` 31、`lspwatcher` 16、`ls` 9、`autoimport` 12、`change` 1、`lsconv` 6、`lsutil` 13、`format`
  7、`project` 99 のうち API で移していないもの（session、overlay、watch、checker pool など）、`ata` 4。API の LS の handler の
  test（`api/session_completion_test.go` の 10 など）。

## test の方式（提案）

fourslash の Go の test を 1 本ずつ Rust に書き直すのでも、harness の判定の 108 method を移すのでもなく、**tsgo の server の
入出力を記録し、tsc-rs の server に同じ入力を送って同じ出力を求める**。

- **記録**：tsgo の fourslash の harness の pipe（`lsptestutil.NewLSPClient`）を一時的な patch で tee し、test ごとに client から
  server への message と、server からの message を記録する（Go probe と同じく upstream の作業ツリーに置いて、記録の後に消す）。
- **vendor に置くもの**：test ごとの scenario（test の file と option、client の message の列、server からの request への harness
  の答え）と、server の応答の digest（正規化した JSON の xxh3）。応答の全文は大きい（global の completion の一覧など）ので vendor
  には置かず、違いを見る時に手元で tsgo の build から作り直す。
- **比べるもの**：client の request への応答は id ごとに完全一致。push の診断は順に依らない集合で比べる。log、progress、
  telemetry は比べない。tsgo の判定は一部しか見ない（completion の `Includes` など）ので、完全一致はそれより厳しいが、tsgo と
  同じ出力が目標なのでこれに合わせる。tsgo が known failing とする 383 file も、tsgo の今の出力に合わせる。
- **決定性**：記録を二度取って digest が同じことを確かめる。時間で決まるもの（debounce、background の診断、idle の後片付け）は、
  記録でも tsc-rs でも test では止める。
- **baseline**：fourslash の baseline 1,761 は応答から導けるので、harness の baseline の出力は移さない（要るようになれば後で）。
- **unit test**：`lsp`・`ls`・`project`・`format` の Go の unit test はこれまで通り Rust に移す。

## port での置き場所

| tsgo | tsc-rs |
| --- | --- |
| `lsp`（server、handler、progress）、`lsproto`、`jsonrpc` | 新しい `crates/lsp`。protocol の型は meta model と tsgo の追加から生成（生成器は `scripts/` の Python）。framing は `tsc_host::ipc` の `Content-Length` を共有 |
| `ls`、`ls/{autoimport,change,lsconv,lsutil}`、`format` | 新しい `crates/ls`（format はその module） |
| `project/session.go` など LS の部分 | `crates/project`（API の `SnapshotHost` の上に `Session`） |
| `cmd/tsc/lsp.go` | `crates/cli`（`tsc-rs --lsp --stdio`） |
| `fourslash`、`lsptestutil` | harness の transcript の runner（suites の一つにする） |

時間に依る部分（debounce、idle、progress の遅延、監視）は `tsc_compiler::system` の clock と同じく抽象にして、test では
決定的に進める（WASM でも動く形にする）。

## 手順

1. **L1 protocol と server**：protocol の型の生成、JSON-RPC、三つの loop、initialize・initialized・shutdown・exit、取消、
   workspace/configuration と user preferences（schema からの生成）。test：`lsproto`、`lsp` の server の test、`lsutil` の
   preferences。
2. **L2 session**：overlay と open file、debounce、inferred project、client への watch の登録、tsconfig の診断の push、locale。
   test：`project` の session、overlay、watch、project の寿命の test。
3. **L3 transcript と最初の機能**：記録の道具と runner、vendor への追加（scenario と digest）、pull の診断、hover、definition、
   typeDefinition（LS-FIRST：initialize → open → 診断 → hover／definition → 編集 → 更新 → close → shutdown）。
4. **L4 completion**：completion（string completion と JSDoc を含む）と completionItem/resolve。auto-import は L5。
5. **L5 auto-import**：registry、import の修正の code fix、completion の auto-import と code action の再試行。
6. **L6 文書の構造**：signature help、document symbol、folding、selection range、semantic token、linked editing、auto insert、
   document highlight（複数文書を含む）、inlay hint。
7. **L7 移動と rename**：references、implementation、`_vs_references`、rename と file rename（willRenameFiles）、call hierarchy、
   code lens、workspace symbol、source definition、複数 project の orchestrator。
8. **L8 編集**：format（文書、範囲、入力時）、change tracker、organize imports、code fix の残り二種と fix-all。
9. **L9 API の LS**：API server で LS に回した handler（completion、参照された symbol、signature の使用、JSDoc tag、
   documentation comment、import adder の edit、`formatNodeForInsertion`、P5-5b）。
10. **L10 content mapper の LS**：client の contribution と inferred project の mapper（その作成途中の解決の拡張子を含む）、
    `RegisterContentMapperExtensions`、mapper の locale、`toAPITextEdits` の元の座標、LS だけの test mapper、content mapper の
    fourslash 56 test。
11. **L11 残りの server の機能**：progress、`custom/projectInfo`、`custom/initializeAPISession`（LSP の上の API）、決定に従って
    ATA、内蔵の file 監視、profile と telemetry。
12. **L12 仕上げ**：全ての transcript、editor での確認、README。

各 slice は他の slice と同じく PR と最終の検査（release build、conformance、suites、test、clippy、transcript）で閉じる。

## 決定（2026-10-13、利用者）

1. **test の方式**：transcript の方式。tsgo の fourslash の test で server の入出力を記録し、tsc-rs に同じ入力を送って、
   client の request への応答に id ごとの完全一致を求める（fourslash の判定の移植はしない）。
2. **transcript の置き場所**：vendor には scenario と応答の digest を置き、応答の全文は違いを見る時に手元で tsgo から作り直す。
3. **範囲**：全て入れる。内蔵の file 監視（client が監視の登録を持たない時、macOS の FSEvents など）、ATA（npm を実行して
   `@types` を入れる）、profile の `custom/*` と telemetry（L11）。
4. **editor での確認**：hover と definition が揃う L3 の後に VS Code で行う（利用者に伝えた予定）。
5. **perf の再計測**：後で（利用者が居る時に nice なしで）。

## L1 protocol と server（PR #772）

- **merge**：#772 → `d55466f02`（hosted run 38351038264：rust 9m18s、conformance 13m12s、gates）。commit：958c06f97（protocol の型）、
  905198a17（server）、1abf202bf（vendor の file 数の契約 test、59,634）。最初の hosted run（38347536520）はその契約で止まった。
- **protocol の型**：`scripts/generate_lsp_protocol.py` が vendor した tsgo の `lsp_generated.go` から生成（402 struct、97 union、
  47 列挙、81 request、23 notification、resolved capability の `omitzero` の JSON）。hosted の rust job が `--check` を実行する。
- **server**：tsgo `lsp.Server` の三つの loop、取消、client への request（`ts<n>`）、`initialize`・`initialized`・`shutdown`・
  `exit`・`$/setTrace`・`custom/setLogVerbosity`、`window/logMessage` の log（Go の `%v` と `time.Duration` の文字列）。params と
  result は型の field 順で書いた raw JSON。`tsc-rs --lsp --stdio`（tsgo の flag、usage、status：入力の終わり 0、`exit` は
  `context canceled` で 1、flag の誤り 2）と親 process の watchdog。Go の `flag` は `tsc_host::flag`（`--api` と共有）、URI の
  file 名は `tsc_program::uri` に移した。
- **tsgo との照合**：tsgo で記録した session（`scripts/lsp_transcript.py --normalize`、`crates/lsp/tests/fixtures/`）を test が
  in-process で再生し、message が全て一致する（VS Code 型の initialize からの一連と、誤りの経路）。
- **検査（905198a17）**：suites 0 regressions、conformance 0 regressions（15,339、483 s）、API client sync 337/356・async 346/365
  （変わらず）、lsp の test（lsproto 26、message 5、queue 3、server 6）、clippy、fmt。
- **有界の違い**：不正な params の誤りの詳細（tsgo の JSON v2 は最も内側の Go の型と JSON pointer（`within "/capabilities"`）か
  byte offset を書く。tsc-rs は params の型名まで。JSON の構文誤りの文言も違う。Go は "cannot"／"unable to" を process ごとに
  無作為に選ぶ）、内蔵の file 監視がまだない時の log、`--pprofDir` の profile、SIGINT・SIGTERM、128 段より深い入力の JSON、
  client が送った空配列の capability の log、`exit` の log の競合（tsgo でも）。

## L1c user preferences（PR #773）

- **merge**：#773 → `a9c98a8da`（hosted run 38354260439：rust 8m12s、conformance 13m42s、gates）。commit：0061fe498（preferences）、
  10ca5ce2b（記録した session が最後の handler の log を待ってから入力を閉じる。最初の hosted run 38352336923 はその log の
  競合で止まった。tsgo で記録し直した transcript は変わらない）。
- **preferences**：vendor した tsgo の `tools/userPreferences.schema.json` から `scripts/generate_lsp_preferences.py`（tsgo
  `tools/gen-preferences` の移植）が `crates/ls/src/lsutil/user_preferences_generated.rs` を生成する（hosted の rust job が
  `--check` を実行）。新しい crate `crates/ls`（`tsc_ls::lsutil`）に手書きの型、`NewDefaultUserPreferences`、
  `ParseUserPreferences`、format の設定。protocol の型は `crates/lsproto`（`tsc_lsproto`）に分けた。checker の module specifier の
  preference は tsgo の値の全てを持つ newtype で public（compiler の挙動は変わらない）。
- **server**：`initialized` が設定を読み（`workspace/configuration`、初期化 option、tsgo の既定）、
  `workspace/didChangeConfiguration` で読み直して session に保つ。
- **tsgo との照合**：`userpreferences_test.go` の 10 test、tsgo 自身（Go の probe）で parse と直列化した 10 の設定が tsc-rs と
  同じ形（`crates/ls/tests/fixtures/preferences.json`）、server の三つの出所の test。vendor の file 数の契約は 59,635。
- **検査（0061fe498）**：suites 0 regressions、conformance 0 regressions（15,339、507 s）、`--checkers 4` の対照は既知の 4 件だけ、
  API client sync 337/356・async 346/365（変わらず）、clippy、fmt。
- **有界の違い**：auto-import の除外（`ParsedAutoImportFileExcludePatterns`、`IsModuleSpecifierExcluded`）は L5。

## L2a session（PR #774）

- **merge**：#774 → `026a7f683`（hosted run 38358291473：rust 7m58s、conformance 15m56s、gates）。commit：8abffa88c（lsconv と URI）、
  0fb750399（overlay）、70f337dc7（open file を持つ snapshot）、4074878d7（Session）、dde477d27（server の document 通知）、
  3b5ca0b07（test）。
- **lsconv**：tsgo の `LSPLineMap` と位置の変換（UTF-8・UTF-16）、`LanguageKindToScriptKind`、`FileNameToDocumentURI` と動的な
  file 名（`^/~ts-uri~/…`）。`ScriptKind` は `tsc_syntax` に移した。
- **overlay と snapshot**：tsgo `overlayfs.go`（`processChanges` の畳み込みと panic を含む）。snapshot の更新（`SnapshotChange`）が
  open document、request の document、新しい preferences、API の request を運び、読みは overlay が先でキャッシュしない。
  config registry は open file ごとの config 名（最寄りと上位）、`customConfigFileName`、open file が保つ config を持つ。builder は
  tsgo の open file の経路（solution 探索、`DidRequestFile`、inferred project の掃除）を移植した。
- **Session**：tsgo `project.Session`。open は保留中の変更をすぐ適用し、document の request は default project が無いか dirty の時
  だけ更新し、close と追跡中の config の watch event は debounce の後に background で更新する（tsgo の background queue と
  世代）。preferences は次の更新で適用し、locale と inlay hint・code lens の refresh は client に渡す。
- **server**：`initialized` が session を作り（workspace の directory、合意した位置の encoding、client が watcher を登録できる時の
  監視、push 診断、`runExternalCode` の content mapper、500 ms の debounce）、`didOpen`・`didChange`・`didSave`・`didClose`・
  `didChangeWatchedFiles` と設定の変更が session に届く。server は session の client と logger（`Processed N file changes`）。
  Go の `time.Duration` の文字列は `tsc_host::go_time`。
- **tsgo との照合**：tsgo の test の移植（overlayfs 13、session、projectlifetime 8、customconfigfilename、untitled、
  projectcollectionbuilder 13 と defaultproject、configfilechanges 7、ProgramUpdateKind 6、snapshot の一部、background queue）。
  session の test は 200 回の連続実行で失敗なし。
- **検査（3b5ca0b07）**：conformance 0 regressions（15,339、486 s）、suites 0 regressions、API client sync 337/356・
  async 346/365（変わらず）、clippy、fmt。
- **有界の違い**：open document の言語種別がまだ parse に届かない（次の slice、`SourceFile.ScriptKind`）。session の log は
  `Processed N file changes` だけで、tsgo の builder の log tree は L2c。watch の登録（L2b）、config 診断の push、診断の refresh、
  idle cache clean（L2c）、ATA、auto-import の warming、telemetry と progress（L5、L11）。fourslash の harness が protocol の外で
  呼ぶ `SetCompilerOptionsForInferredProjects` は記録器と共に L3。tsgo の `didOpen` は失敗しないが tsc-rs の snapshot 更新は
  host の誤りで失敗しうる（handler の誤りとして返す）。

## source file の script kind（PR #775）

- **merge**：#775 → `9515f8394`（hosted run 38362451488：rust 12m18s、conformance 20m53s、gates）。commit：2de7f2f62（script kind）、
  67eabeeca（`SourceFile::from_tree` の clippy の許可だけ）。
- **内容**：parse は tsgo の script kind をとり（言語の variant と JavaScript の context はそこから決まる、tsgo
  `initializeState`）、source file が保つ（`SourceFile.script_kind`、`is_source_file_js`）。prepared source の kind は content
  mapper の出力の仮想名、file handle（open document の言語）、file 名の順。project は open document の言語を load の時に渡し
  （保存した command line には入れない）、言語の変わった document は program の in-place の再利用をしない。checker の
  `IsSourceFileJS`・`IsPlainJSFile`・`IsInJSFile` に当たる判定は kind を読み、拡張子による対応の判定は tsgo と同じく名前のまま。
- **検査（2de7f2f62）**：conformance 0 regressions（15,339、500 s）、`--checkers 4` の対照は既知の 4 件だけ、suites 0 regressions、
  API client sync 337/356・async 346/365（変わらず）、crate の test（syntax 247、binder 78、program 669、checker 1,798、
  emitter 341、compiler 215、project 140、api 298、lsp 12）、clippy、fmt。L2a の有界の違い（untitled の JavaScript が
  TypeScript として parse される）を解消した。

## L2b file の監視（PR #776）

- **merge**：#776 → `d73cdec53`（hosted run 38364892878：rust 12m39s、conformance 20m53s、gates）。commit：3bcf9a7b3（ef70ffd4a を
  main に rebase、差はドキュメントだけ）。
- **監視**（`crates/project/src/watch.rs`）：tsgo の `WatchedFiles`（求められた時に計算し、clone は元の watcher を保って変わった
  時だけ process 全体の counter から新しい ID をとる）、glob を watch 全体で数える registry（client への登録は一度、どの watch も
  持たなくなったら解除、失敗は次の更新で再試行）、`getPathComponentsForWatching`、`GetCommonParentDirectories`、program file の
  glob（`createResolutionLookupGlobMapper`）。config ごとの root file の監視（全体の parse ごとに更新）、project ごとの program
  file の監視（新しい file の build ごと）と content mapper の監視。build が見た file は名前も保つ。
- **session と server**：snapshot の採用ごとに background で監視を更新する（tsgo の log を含む）。server は
  `client/registerCapability`・`client/unregisterCapability` を tsgo の 1 秒の timeout で送る。workspace の外の directory は
  client が対応すれば relative pattern。
- **tsgo との照合**：`watch_test.go`、`GetCommonParentDirectories` の 11 入力（Go の probe に固定）、session の
  DidChangeWatchedFiles の 17 test、project の寿命の監視の数、server の test。
- **検査（ef70ffd4a）**：project 166、lsp 13、api 298、clippy、fmt、API client sync 337/356・async 346/365（変わらず）。compiler の
  経路に届かないので conformance と suites は回していない。
- **有界の違い**：watch ID の数字（typings と auto-import の監視、config registry の `LoadOrStore` の余分な entry の分の counter が
  まだ無い。登録は transcript の判定の対象外で、glob と kind は tsgo と同じ）。typings と auto-import の監視は L11 と L5、
  dynamic registration の無い client の内蔵の監視は L11。

## L2c config の診断の push と refresh（PR #777）

- **merge**：#777 → `be84cfa42`（hosted run 38369297130：rust 12m8s、conformance 21m32s、gates）。commit：6002bf8fe
  （6131868d4 を main に rebase、差はドキュメントだけ）。
- **push の診断**（tsgo `publishProgramDiagnostics`）：snapshot の採用ごとに、開いた file のある configured project の
  `tsconfig.json` へ、config の parse の診断と program の診断と、作られた checker の global の診断（`LiveProgram::
  global_diagnostics_so_far`：checker を使う前は無し）を送る。program を新しく作った時（一 file の clone でなく）と file を
  開き直した時だけ。project が無くなった時、file を全て閉じた時、利用者が検証を切った時は空で送る。
- **protocol の診断**（`tsc_ls::lsconv::diagnostic_to_lsp`、tsgo `diagnosticToLSP`）：重さ（style check を警告にする設定）、
  code（Visual Studio には `TS<n>`）、source、client の locale で平らにした message の連鎖、関連情報、不要・非推奨の tag、
  UTF-16 の span から protocol の range。
- **refresh**（tsgo `scheduleDiagnosticsRefresh`）：program を変えうる監視の event、tsgo が refresh する設定の変更、content
  mapper の file の編集（即時）で debounce して `workspace/diagnostic/refresh`。普通の編集は待ちの refresh を取り消す。
- **アイドルの掃除**（tsgo `scheduleIdleCacheClean`）：30 秒の静止の後、build が見なかった cache の file を捨てる。
  background の待ちは取り消し、close は取り消して走っている掃除を待つ。
- **採用の log**：`Adopted snapshot N (parent M) as current session snapshot (replacing K)`、追加・削除・新しい file で build
  された project（verbose なら file も）、cache の統計。
- **tsgo との照合**：`TestPushDiagnostics` の 9（global の診断と hover の 3 は L3）、refresh の test、アイドルの掃除の close
  の test、code lens・inlay hint・locale の設定の test。server の transcript（`crates/lsp/tests/fixtures/push_diagnostics.*`、
  不正な `target` の config を開く）は log と watch ID の数字を除いて tsgo と一致。
- **検査（6131868d4）**：release build、conformance 0 regressions（15,339、493 s）、`--checkers 4` と比較で既知の 4 件だけ、
  suites 0 regressions、API client sync 337/356・async 346/365（変わらず）、test（diagnostics 82、ls 18、project 184、
  lsp 14、api 298）、clippy、fmt。
- **有界の違い**：builder の log tree と verbose の cache 統計、checking で見つかる global の診断の request 後の push（L3）、
  content mapper の span map（L10）。

## L3a transcript の記録と再生（PR #778）

- **merge**：#778 → `8bdd5df36`（hosted run 38377954175：rust 11m43s、conformance 20m31s、gates。一度目の run 38375873279 は
  `documents_reach_the_session` が log の順に依って落ちた：L2c の採用 log が background で間に入るため。test を順に依らない形に
  した ecc532dea）。commit：c8fb8f871、ratchet 8a3107fa1、ecc532dea。
- **記録**（`scripts/lsp_transcripts.py`）：tsgo の fourslash の harness・4,372 の test file・その util と
  `lsptestutil` を pin した checkout の `internal/tsrsfourslash` に写し、import を写しに向け、known failing の skip（383）を外し、
  記録器（`scripts/lsp_transcripts_dump/tsrs_record.go`：pipe の frame の両方向、harness の file の書き込みと削除、file と
  inferred project の option）を足して全 test を走らせる（`go test -json -parallel 2`、約 2 分）。失敗してよいのは known failing
  だけ（383 中 383 が失敗、ほかは無し）。
- **vendor**：`vendor/typescript-native/7.1.0-dev-aa814927/fourslash-transcripts.jsonl.gz`。4,584 harness、JSON lines で
  43.6 MB、gzip で 1.9 MB（initialize と configuration は全 harness で同じ）。harness ごとに file、大文字小文字、inferred の
  option、client の message と file の操作の列、`workspace/configuration` への harness の答え、request ごとの tsgo の応答の
  digest（result か error の canonical JSON の SHA-256 の先頭 16 桁。当初案の xxh3 は Python の標準に無いので SHA-256）。全文は
  手元の `target/lsp-transcripts/<profile>/raw.jsonl`。
- **決定性**（5 回の記録を比較）：tsgo の配列の順が run ごとに変わるもの（Go の map の symbol table からの completion、
  `crossproject.go` が sync map の順で合わせる複数 project の結果：references、implementation、rename、incoming calls、code lens
  の resolve、willRenameFiles）は並べ替えて digest する（`UNORDERED`、runner も同じ表）。harness が map の順で request を送り file
  を rename する test があるので、`--check` は step を多重集合で比べる。その順で tsgo の答え自体が変わる 4 test
  （`NONDETERMINISTIC`：二つの export が共有する名前の completion、map の順で pull する JS の診断 ×2、`repeated.ts`／
  `repeated.d.ts` の path completion）は `--check` で比べない。
- **再生**（suites の `lsp`、`crates/conformance/src/ts71/suites/lsp.rs`）：場面ごとに MemFs と組込みの lib、tsc-rs の server を
  pipe で動かし、message を順に送り（request は応答を待つ）、server の request には harness の `handleServerRequest` と同じに
  答え、file の操作を間に適用し、応答の digest を比べる。method ごとの一致数を表示し、`--dump` で違う場面の request と応答を
  書く。場面の後に session を閉じる（tsgo は GC 任せだが、ここではアイドルの timer が program を保つ：200 場面で 6.8 GB →
  471 MB）。約 4 分。
- **server**：tsgo `SetCompilerOptionsForInferredProjects`（`initialized` で適用）、session の
  `DidChangeCompilerOptionsForInferredProjects`、snapshot と builder が inferred project の option を運ぶ。semantic token の
  legend に tsgo の `local`（transcript で見つかった）。
- **時点の結果**：initialize 4,584/4,584、完全一致の場面 18（ratchet に `lsp/<test>` 4,584 行）。
- **検査（c8fb8f871 + 8a3107fa1）**：release build、`suites_ts71.py --update` と `--check` 0 regressions、
  `lsp_transcripts.py --check` 一致、API client sync 337/356・async 346/365（変わらず）、test、clippy、fmt。compiler の経路に
  届かないので conformance は回していない。

## L3b pull の診断（PR #779）

- **merge**：#779 → `616f7224d`（hosted run 38392705342：rust 12m4s、conformance 29m8s、gates。一度目の run 38390971840 は
  checker の単体 test が tsc 6.0.3 の構築 signature の文（`'(): object'`）を固定していて落ちた：tsgo は `new (): object`。七つの
  提案を Go の probe で確かめて期待値を直した 43bbc6f71）。commit：a08c179c4、618eb8c8f、5009c3863、e47138dbf、ratchet
  3b34134d2、43bbc6f71。
- **言語 service**：`tsc_ls::LanguageService`（program、converters、preferences、client の capability、locale の catalog）と
  `provide_diagnostics`（tsgo `getAllDiagnostics`：構文、意味、提案、宣言を出す時は宣言の診断）。server は tsgo の
  `registerLanguageServiceDocumentRequestHandler`（同期の部分で既定の project、後で program に handler、panic は tsgo の
  `recover` と同じ error、その後に checking で見つかった global の診断の push）。
- **checker の修正**（transcript で見つかり、conformance の baseline では見えない）：object literal と JSX 属性の文脈的な非推奨、
  構築 signature の文（`new (): C`）、JS の 2570 の関連情報、late-bound の override の名前、切り詰めの長さ、宣言の診断の範囲
  （emitter が `get_error_span_for_node` を使う）と順、関連情報の並べ替えと統合、shard をまたぐ同じ行の重複除去。
- **回帰 test**（利用者の指示：conformance で捕まえられない誤りは Rust の test に）：
  `crates/compiler/tests/language_service_diagnostics.rs` の 9（8 が修正前に失敗）、shard の test 1、project の test 1。
- **見つけた罠**：CLI の `run` が process 全体の設定（未使用の識別子の提案、JSDoc の parse mode）を変え、同じ process の後の LSP の
  場面に漏れた（suites で 812 → 1,039）。suites の tsc の `render()` で保存と復元。設定を実行ごとにするのは後の候補。
- **検査（3b34134d2 + 43bbc6f71）**：conformance 0 regressions（全 13,577 full）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（lsp 150 full、診断 1,039/1,043、残りは content mapper の 4 → L10）、API client sync 337/356・async 346/365、
  test、clippy、fmt。

## L3c go to definition と type definition（PR #780）

- **merge**：#780 → `577bc9afa`（hosted run 38404039864：rust 8m34s、conformance 23m30s、gates）。commit：169f32702（tree view と
  astnav を言語 service へ）、a4a6962eb（sourcemap の decoder と document position mapper を emitter へ）、3b7d7c1cd（機能）、
  ratchet 764e5dce9。
- **言語 service**：tsgo `ls/definition.go`（`ProvideDefinition`、`ProvideTypeDefinition`、`createDefinitionLocations`、
  `getDeclarationsFromLocation`、文脈型からの object literal の property、呼ばれた signature の宣言、override、label、`switch` と
  関数の keyword、index signature）、`getReferenceAtPosition`（triple-slash の path・types・lib、相対の module 指定子）、
  `ls/source_map.go`（宣言 file の位置を declaration map で source へ）、tsgo `ls.Host`（snapshot が実装）。
- **checker**：tsgo の services（`GetRootSymbols`、`GetPropertySymbolsFromContextualType` と
  `IsTypeInvalidDueToUnionDiscriminant`、`GetIndexSignaturesAtLocation`、`GetFirstTypeArgumentFromKnownType`）と
  `GetContextualType` の `IgnoreNodeInferences`（`runWithInferenceBlockedFromSourceNode`、`runWithoutResolvedSignatureCaching`：
  三つの skip-direct-inference の stub を置き換え、`resolveCall` の `reportErrors` も）。transcript で見つかった修正：
  `resolveDecorator` の `CanHaveDecorators`（関数宣言の decorator で panic）、JSDoc `@import` の module 指定子（tsgo は reparse
  した宣言で解決）。
- **回帰 test**：`crates/ls/tests/definition.rs` の 6（decorator、`@import`、declaration map、`IgnoreNodeInferences` の二つ：
  union の方は遮断なしで失敗、type definition の戻り値型）、emitter の source mapper の test。
- **検査（764e5dce9）**：conformance 0 regressions（全 full、sourcemap は共有 decoder で 13,577 full）、`--checkers 4` は既知の
  4 だけ、suites 0 regressions（lsp 392 full、definition 472/482、typeDefinition 59/64、診断 1,039/1,043）、API client sync
  337/356・async 346/365、test、clippy、fmt。
- **有界の違い**：content mapper（L10）の 5 test、`preferGoToSourceDefinition`（L7）の 1 test。

## L3d-1 hover の verbosity 0（PR #781）

- **merge**：#781 → `25ae95f8f`（hosted run 38427807862：rust 9m13s、conformance 19m7s、gates）。commit：2aa391a01
  （JSDoc の `@` と comment の終わり）、7bd80f600（printer の一行の type literal、末尾の semicolon の省略、`EscapeString`）、
  d91f8e159（checker の表示の文字列と verbosity、hover の node builder の規則）、1ce3f6c18（hover）、ratchet dc58ad3f4。
- **言語 service**：tsgo `ls/hover.go`（`ProvideHover`、`getQuickInfoAndDeclarationAtLocation` と全ての symbol の種類の表示、
  `getNodeForQuickInfo`、`getSymbolAtLocationForQuickInfo`、`shouldGetType`、呼び出しと `new` の signature）、`ls/jsdoc.go`
  （symbol・signature・alias の documentation、tag、link とその名前）、`getMeaningFromLocation` と `getAdjustedLocation`。
  definition の request の状態は共有の `Request` に。server は hover を言語 service の document request として登録。
- **checker**：tsgo の `TypeToStringEx`・`SignatureToStringEx`・`SymbolToStringEx`・`TypeParameterToStringEx` と
  `VerbosityContext`。hover で tsc 6.0.3 の振る舞いが見えた node builder を tsgo に：`UseInstantiationExpressions`
  （`f<string>.C`）、`getClassOrInterfaceLikeDeclaration` による `lookupInstantiatedTypeArgumentNodes`（関数の外側の型引数も）、
  signature の具体化した型引数、`UseOnlyExternalAliasing`、展開の判定（`isExpandableType`・`shouldExpandType`・
  `checkTypeExpandability`・`walkNodeForExpandability` が tsc の `canPossiblyExpandType` を置き換え）、union の enum だけを展開。
  JSDoc の名前は tsgo が reparse した複製で解くのと同じに（名前空間付きの typedef の最も内側の名前は alias の名前、無効な識別子の
  property は symbol なし、名前空間付きの alias の型の名前はその名前空間から）。
- **JSDoc の parser**（tsgo）：`@` は識別子の始まりの前でだけ tag を始め、主の comment は最後の部分だけが末尾の空白を落とす。
- **printer**：一行の印の付いた type literal は一行に（tsgo `LFSingleLineTypeLiteralMembers`）、`OmitTrailingSemicolon`、
  `EscapeString`。
- **回帰 test**（利用者の指示：conformance で捕まえられない誤りは Rust の test に）：`crates/ls/tests/hover.rs` の 17（期待値は
  transcript の tsgo の答えのまま）、checker の node builder の単体 test は tsgo の `shouldExpandType` を固定。
- **検査（dc58ad3f4）**：conformance 0 regressions（全 13,577 full、488 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（lsp 893 full、hover 2,325/2,455、definition・typeDefinition・診断は変わらず）、API client sync 337/356・
  async 346/365、test、clippy、fmt。
- **有界の違い**：verbosity 1 以上（L3d-2）68、Visual Studio の `_vs_rawContent`（L3d-3）53、content mapper（L10）9。

## L3d-2 hover の verbosity 1 以上（PR #782）

- **merge**：#782 → `ebd61b3fb`（hosted run 38433092888：rust 9m43s、conformance 28m50s、gates）。commit：5b99c750b
  （checker の展開）、bc85c7b40（hover）、ratchet 64b8fc669。
- **checker**（`crates/checker/src/node_builder/hover.rs`）：tsgo `checker/nodebuilder_hover.go` と
  `NodeBuilder.ExpandSymbolForHover`。hover が見せる宣言を直接作る：enum の member（書いた初期化子か定数値）、class の
  member・constructor・index signature・heritage 句（class の修飾子つき）、interface の signature と property、
  namespace の export（alias（局所の対象を先に宣言）、関数、入れ子の宣言、型 alias）。切り詰めの長さで切り、元の宣言の
  修飾子（`export`・`declare` なし）を付ける。`CheckerState::expand_symbol_for_hover` が tsgo の `ExpandSymbolForHover`
  と同じに印字する。
- **言語 service**：`tryExpandSymbol` が一段下の verbosity で展開を見せ、`canIncreaseVerbosity` と切り詰めを持ち出す。
- **回帰 test**：`crates/ls/tests/hover.rs` に 17（計 34。期待値は transcript の tsgo の答えのまま）：enum と空の enum
  （panic していた）、class（abstract、mixin の基底、interface との統合）、interface の member の順、namespace（alias、
  default export、型 alias、default export の関数の binding、interface・class の heritage、crash の場面）、切り詰め、
  import attributes 付きの pattern ambient module。
- **検査（56a630eab、main 789857f83 への rebase は packet の README だけ、ratchet 64b8fc669）**：conformance 0
  regressions（全 13,577 full、481 s）、`--checkers 4` は既知の 4 だけ、suites 0 regressions（lsp 925 full、hover
  2,393/2,455）、API client sync 337/356・async 346/365、test（checker 1,797、ls 61、api 295）、clippy、fmt。
- **有界の違い**：Visual Studio の `_vs_rawContent`（L3d-3）53、content mapper（L10）9。

## L3d-3 Visual Studio の hover の raw content（PR #783）

- **merge**：#783 → `7dee0bc8c`（hosted run 38443134012：rust 8m8s、conformance 28m30s、gates）。commit：90417ac14
  （printer の display parts）、30fe05e6d（checker の分類された印字）、dff3e7ad5（hover）、ratchet bb6b909e4。
- **printer**：単独の印字が書くたびの分類を記録する（tsgo の `displayPartsWriter`：一行、改行は空白）。識別子は symbol に
  与えられた分類で（tsgo `IdToSymbol`）、token の node は tsgo の `emitTokenNode` と同じく keyword か punctuation、配列型の
  括弧は二つの punctuation。普通の出力は変わらない。
- **checker**：node builder が symbol に名前を付ける識別子を記録する（tsgo `idToSymbol`：`newIdentifier`、宣言の引数名、
  `setTextRange` の複製）。型・signature・module の import attributes を tsgo の Visual Studio 向けの hover と同じく
  display parts で印字（verbosity なし、comment を残し、signature は末尾に semicolon）。tsgo の `IsUndefinedSymbol`・
  `IsArgumentsSymbol`・`IsDeprecatedDeclaration`・`IsThisInTypeQuery` を公開。
- **言語 service**：quick info の writer が tsgo と同じく全ての書き込みを分類し（keyword、punctuation、operator、literal、
  `classificationForSymbol` による symbol）、hover は icon（tsgo `hovericon.go`、`lsutil` の `GetSymbolKind`・
  `GetSymbolModifiers` を `lsutil/symbol_display.rs` に port）と comment だけの documentation を加える。
- **回帰 test**：`crates/ls/tests/hover.rs` に Visual Studio の 5（計 39）：alias の icon と documentation の要約、省略可能と
  rest の引数名、accessibility ごとの method の icon、property、tag なしの documentation の要約。
- **検査（bb6b909e4、ratchet の commit は code を変えない）**：conformance 0 regressions（全 13,577 full、478 s）、
  `--checkers 4` は既知の 4 だけ、suites 0 regressions（lsp 930 full、hover 2,446/2,455）、API client sync 337/356・
  async 346/365、test（emitter 348、checker 1,797、ls 66、api 295、compiler 225、conformance 49）、clippy、fmt。
- **有界の違い**：content mapper（L10）9。transcript の hover は他が全て tsgo と同じ。

## L4 completion の計画

tsgo の `ls/completions.go`（6,922 行）、`ls/string_completions.go`（2,249 行）、`ls/jsdoc_snippet.go`（597 行）。
transcript では 1,210 の場面に completion 2,419（null 233、auto-import の項目を含むもの 262）と completionItem/resolve 655。
項目の順は比べない（digest が並べ替える）。auto-import（registry、import の code fix、completion の auto-import）は L5。
小分けにして、各段で transcript の一致を測る。

1. **L4a 土台**：`ProvideCompletion`、`getCompletionData` の一般の道（文脈の token と前の token、補完を止める位置、新しい
   識別子の位置と commit characters、scope の symbol（`getGlobalCompletions`）、property access の member
   （`getTypeScriptMemberSymbols`）、keyword、import 文の keyword だけの補完）、文脈上の型によるリテラルと推奨の項目、
   `completionInfoFromData`・`getCompletionEntriesFromSymbols`・`createCompletionItem`（名前、kind と kindModifiers、
   sortText、insertText と filterText、置き換えの範囲、commit characters、itemDefaults の editRange、resolve のための
   data）。
2. **L4b 形の決まった場所**：object literal・JSX 属性・object の binding pattern・import／export の句・class の member・
   型引数の object 型の補完（`tryGetObjectLikeCompletionSymbols` ほか）と、特別な項目（object literal の method と class の
   member の snippet：`createObjectLiteralMethod`、`getEntryForMemberCompletion`。printer と node builder を使う）、label の
   補完、JSX の閉じ tag、switch の case の snippet。
3. **L4c string completion**：`string_completions.go`（literal 型、element access の property 名、path と module 指定子、
   package.json）。
4. **L4d JSDoc**：tag 名・param 名の completion と `jsdoc_snippet.go`。
5. **L4e completionItem/resolve**：details と documentation（hover の symbol の表示と JSDoc を使う）。auto-import の code
   action は L5。

## L4a completion の土台（PR #784）

- **merge**：#784 → `56b2c8e52`（hosted run 38458124716：rust 12m33s、conformance 23m48s、gates）。commit：bb15bb98e
  （checker の service）、89ce863de（completion）、ratchet f394701e4。
- **checker**：tsgo の completion が使う service（`IsValidPropertyAccess`、`GetAllPossiblePropertiesOfTypes`、
  `GetNonOptionalType`、`GetStringIndexType`、`GetNumberIndexType`、`TryGetMemberInModuleExportsAndProperties`、
  `GetExportsAndPropertiesOfModule`、`GetJsxIntrinsicTagNamesAt`、`TryGetThisTypeAtEx`、`GetAccessibleSymbolChain`、
  `GetTypeArgumentConstraint`、`GetContextualTypeForArrayLiteralAtPosition`、`SkipAlias`、`IsUnknownSymbol`）。
  `getContextualType` の分岐は tsc-rs の木にない token（tsgo の token の node：親の operand のどれでもない）にも答え、
  推論の遮断も tsgo と同じ範囲に。`GetExportsOfModule` は予約名を除き（`symbolsToArray`）、`IsTypeNodeKind` は tsgo の
  JSDoc の型を含める。
- **言語 service**（`crates/ls/src/completions/`）：`ProvideCompletion`、文脈の token と前の token、補完を止める位置、新しい
  識別子の位置と commit characters、`getScopeNode` と `IsCompletedNode`、scope の symbol、dot の右の member（`?.`・
  element access・`await` の挿入）、`this.` の member、keyword、import 文の keyword だけの補完、文脈上の型のリテラルと推奨の
  項目（signature help の引数の位置）、`completionInfoFromData`・`getCompletionEntriesFromSymbols`・`createCompletionItem`
  （名前、kind と修飾、sortText、filterText、置き換えの範囲、itemDefaults、resolve の data、引用符の好み、Go の JSON の
  引用）。検査しない JavaScript は file の識別子（tsgo の name table）を足す。navigator に `GetTokenAtPosition`・
  `FindNextToken`・`FindChildOfKind`・`GetFirstToken`・`GetLastToken`・`nodeEndsWith`。tsgo の class の construct
  signature は型引数の並びが空でも nil でない（`new Bar<` は型引数の位置）。
- **回帰 test**：`crates/ls/tests/completion.rs` に 24（期待値は transcript の tsgo の答え。大きな一覧は件数・既定値・要の
  項目）。
- **検査（f394701e4）**：conformance 0 regressions（全 13,577 full、478 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（lsp 1,371 full、completion 1,164/2,419、441 行が none→full）、API client sync 337/356・async 346/365、
  test（syntax 228、checker 1,797、ls 90、lsp 14、api 295、compiler 225、conformance 49）、clippy、fmt。
- **有界の違い**：L4b〜L4e、auto-import（L5）、content mapper（L10）、JSDoc の `function(...)` 型（tsgo は `function` と
  いう名前の型参照として読み、name table に入る）。

## L4b 決まった形の場所の completion（PR #785）

- **merge**：#785 → `e4db6609f`（hosted run 38465407861：rust 11m48s、conformance 29m55s、gates）。commit：6702fcccd
  （completion）、ratchet 65003da6b。
- **言語 service**（`crates/ls/src/completions/searches.rs` ほか）：tsgo の `tryGetGlobalSymbols` が global の前に試す
  探索。型引数の object 型の member（制約の member から書いたものを除く）、object literal の文脈上の property と binding
  pattern の property（JavaScript の引数は `@param` の型。checker の `type_node_of` が tsgo の `Node.Type`）、書いた member と
  spread 代入が宣言する名前を除く sortText、import・export 句の module の export、import attribute、`export { | }` の
  local、class の基底の未実装 member と class・interface の member keyword、JSX element の attribute（optional と spread の
  sortText）。JSX の閉じ tag、`break`・`continue` の label、JSX attribute の snippet（`name={$1}`・`name="$1"`）、method
  snippet の設定時の object literal property の sortText も。
- **回帰 test**：`crates/ls/tests/completion.rs` に 11（計 35）。
- **検査（65003da6b）**：conformance 0 regressions（全 13,577 full、476 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（lsp 1,527 full、completion 1,550/2,419、156 行が none→full）、API client sync 337/356・async 346/365、test
  （checker 1,797、ls 101、lsp 14、api 295、compiler 225、conformance 49）、clippy、fmt。
- **有界の違い**：class member・object literal method・網羅的な switch の case の snippet は tsgo の formatter
  （`format.FormatNodeGivenIndentation`）を使うので L8 の後。L4c〜L4e、L5、L10。

## L4c-1 文字列リテラルの completion（PR #786）

- **merge**：#786 → `656b39336`（hosted run 38473329341：rust 12m37s、conformance 23m48s、gates）。commit：fddecc587
  （checker の候補一覧）、c0aa39f9c（completion）、ratchet a510edf5a。
- **checker**：tsgo の `candidatesOutArray`。`getResolvedSignature` は候補を求められると cache 済みの呼び出しも解決し直し、
  `resolveCall` は候補を返し（選んだ候補・失敗の候補の位置はその instantiation。tsgo の slice の共有と同じ）、その間は
  error を報告せず、失敗では常に最長の候補を選ぶ。`GetCandidateSignaturesForStringLiteralCompletions`（編集中の引数からの
  推論を止めた候補、次に止めない候補）。`printer.EscapeString` は引用符を取る。
- **言語 service**（`crates/ls/src/completions/strings.rs`）：文脈上の型のリテラル、object literal の引用符付き property 名、
  element access と indexed access type の property 名、union（書いた member を除く）、型引数とその property、呼び出し・
  `new`・JSX の引数（候補の引数の型。引数の数も）、case 句（書いた case を除く）、import・export specifier、`in` 演算子。
  項目は tsgo の置き換え範囲と、literal の引用符に合わせた escape（tsgo は引用符を literal の値から読む）。
- **回帰 test**：`crates/ls/tests/completion.rs` に 11（計 46）。
- **検査（a510edf5a）**：conformance 0 regressions（全 13,577 full、491 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（lsp 1,592 full、completion 1,688/2,419、65 行が none→full）、API client sync 337/356・async 346/365、test
  （checker 1,797、emitter 196、ls 112、lsp 14、api 295、compiler 225、conformance 49）、clippy、fmt。
- **有界の違い**：path・module 名・triple-slash 参照（L4c-2）、snippet（L8）、L4d、L4e、L5、L10。

## L4c-2 path の completion（PR #787）

- **merge**：#787 → `c444c2d7c`（hosted run 38480483705：rust 8m27s、conformance 29m30s、gates）。commit：a9229cad1
  （module 解決の部品）、c241be164（path completion）、ratchet f12c0b6d0。
- **言語 service の host**：tsgo の `ls.Host` の file system（`ReadDirectory`・`GetDirectories`・`DirectoryExists`・
  `FileExists`。module specifier の completion だけが使う）。snapshot は open document を除いた file system（tsgo の
  `s.fs.fs`、`vfsmatch.ReadDirectory`）で答える。request が host を持つ。
- **言語 service**（`crates/ls/src/completions/paths.rs`）：相対 path（`rootDirs` も）、`paths`、ambient module の名前、
  typings（type root と各祖先の `node_modules/@types`）、script より上の各 package.json の依存、`node_modules` の
  directory、package の `exports`・`imports`（条件・wildcard・末尾の `/`）、`typesVersions` の転送、`imports` の
  `outDir`・`declarationDir` の入力 directory、許される module specifier の語尾による file 名、triple-slash の
  `path`・`types` 参照。tsc-rs の program は load 後に host を持たないので、tsgo の `Program.GetPackageJsonInfo`・
  `GetNearestAncestorDirectoryWithPackageJson` は言語 service の host から manifest を読む。
- **checker・program**：`GetAmbientModules`（incremental build の referenced files も使う）、no old specifier の
  `GetAllowedEndingsInPreferredOrder`、`TryGetJSExtensionForFile`、`TryGetRealFileNameForNonJSDeclarationFileName`、
  `GetConditions`、`GetVersionPaths`、`ComparePatternKeys`、package.json の文書順の entry、`NormalizeSlashes`、既定の
  type root（resolver 自身も使う）、options の `GetResolvePackageJsonExports`・`Imports`。
- **回帰 test**：`crates/ls/tests/completion.rs` に 10（計 56）。test の host は test の file だけを並べる（tsgo は library
  を bundle する）。
- **検査（f12c0b6d0）**：conformance 0 regressions（全 13,577 full、488 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（lsp 1,650 full、completion 1,935/2,419、58 行が none→full）、API client sync 337/356・async 346/365、test
  （checker 1,797、program 114、types 44、ls 122、lsp 14、api 295、compiler 225、conformance 49、project 186）、clippy、
  fmt。
- **有界の違い**：`TestCompletionForStringLiteralNonrelativeImport2`（同じ名前の `repeated.ts` と `repeated.d.ts` の
  どちらの拡張子を残すかが tsgo の実行ごとに変わる。`scripts/lsp_transcripts.py` が非決定として扱う）。content mapper の
  file の path completion（L10）、L4d、L4e、L5、L8。

## L4d JSDoc の completion（PR #788）

- **merge**：#788 → `91d20e42b`（hosted run 38486266220：rust 12m21s、conformance 29m56s、gates）。commit：b5bc6f8b0
  （parser）、8efdd5fb7（JSDoc completion）、ratchet b27a449b6。
- **言語 service**（`crates/ls/src/completions/jsdoc.rs`）：tag 名の位置では JSDoc の tag 名（comment の行頭なら `@`
  付き）と、tag のない各 parameter の `@param` tag（JavaScript では既定値の型、分割代入の parameter は property 付きの
  `@param {object}`。tsgo はここで snippet を出さない）。`@param` の名前の位置では、他の tag が持たない parameter 名。
  `/**` の後では doc comment の template（`getDocCommentTemplateAtPosition`：宣言の parameter と、
  `generateReturnInDocTemplate` なら `@returns`）。tsgo と同じく、本文のある閉じていない comment は ` */` を足して
  file を parse し直す。snippet を受ける client には template を snippet にする。request は locale の message catalog
  を持つ（template の detail `JSDoc comment`）。
- **parser**：tsgo の `GetJSDocCommentRanges` は 4 文字未満の comment を落とすので、file 末尾の `/**` は JSDoc comment
  ではない（`TestDocCommentTemplateEmptyFile`）。
- **回帰 test**：`crates/ls/tests/completion.rs` に 9（計 65。tag 名と tag、関数のない `@param`、parameter 名、
  JavaScript の既定値と分割代入、変数文の template（parse し直し）、snippet の template、字下げされた method、
  短い閉じていない comment）。
- **検査（b27a449b6）**：conformance 0 regressions（全 13,577 full、489 s。parser の修正で動く configuration はない）、
  suites 0 regressions（lsp 1,665 full、completion 2,044/2,419、15 行が none→full）、API client sync 337/356・async
  346/365、test（syntax 228、checker 1,797、ls 131、lsp 14、api 295、compiler 225、conformance 49）、clippy、fmt。
  checker の変更はないので `--checkers 4` はなし。
- **有界の違い**：`completionItem/resolve`（L4e）、auto-import（L5）、formatter が要る snippet（L8）、content mapper
  の file（L10）。
