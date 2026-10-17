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

- **merge**：#772 → `b7158ecdc`（hosted run 38351038264：rust 9m18s、conformance 13m12s、gates）。commit：958c06f97（protocol の型）、
  698af9a37（server）、3d4a4fb44（vendor の file 数の契約 test、59,634）。最初の hosted run（38347536520）はその契約で止まった。
- **protocol の型**：`scripts/generate_lsp_protocol.py` が vendor した tsgo の `lsp_generated.go` から生成（402 struct、97 union、
  47 列挙、81 request、23 notification、resolved capability の `omitzero` の JSON）。hosted の rust job が `--check` を実行する。
- **server**：tsgo `lsp.Server` の三つの loop、取消、client への request（`ts<n>`）、`initialize`・`initialized`・`shutdown`・
  `exit`・`$/setTrace`・`custom/setLogVerbosity`、`window/logMessage` の log（Go の `%v` と `time.Duration` の文字列）。params と
  result は型の field 順で書いた raw JSON。`tsc-rs --lsp --stdio`（tsgo の flag、usage、status：入力の終わり 0、`exit` は
  `context canceled` で 1、flag の誤り 2）と親 process の watchdog。Go の `flag` は `tsc_host::flag`（`--api` と共有）、URI の
  file 名は `tsc_program::uri` に移した。
- **tsgo との照合**：tsgo で記録した session（`scripts/lsp_transcript.py --normalize`、`crates/lsp/tests/fixtures/`）を test が
  in-process で再生し、message が全て一致する（VS Code 型の initialize からの一連と、誤りの経路）。
- **検査（698af9a37）**：suites 0 regressions、conformance 0 regressions（15,339、483 s）、API client sync 337/356・async 346/365
  （変わらず）、lsp の test（lsproto 26、message 5、queue 3、server 6）、clippy、fmt。
- **有界の違い**：不正な params の誤りの詳細（tsgo の JSON v2 は最も内側の Go の型と JSON pointer（`within "/capabilities"`）か
  byte offset を書く。tsc-rs は params の型名まで。JSON の構文誤りの文言も違う。Go は "cannot"／"unable to" を process ごとに
  無作為に選ぶ）、内蔵の file 監視がまだない時の log、`--pprofDir` の profile、SIGINT・SIGTERM、128 段より深い入力の JSON、
  client が送った空配列の capability の log、`exit` の log の競合（tsgo でも）。

## L1c user preferences（PR #773）

- **merge**：#773 → `65b2e05bf`（hosted run 38354260439：rust 8m12s、conformance 13m42s、gates）。commit：44146b45f（preferences）、
  38fdcad2b（記録した session が最後の handler の log を待ってから入力を閉じる。最初の hosted run 38352336923 はその log の
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
- **検査（44146b45f）**：suites 0 regressions、conformance 0 regressions（15,339、507 s）、`--checkers 4` の対照は既知の 4 件だけ、
  API client sync 337/356・async 346/365（変わらず）、clippy、fmt。
- **有界の違い**：auto-import の除外（`ParsedAutoImportFileExcludePatterns`、`IsModuleSpecifierExcluded`）は L5。

## L2a session（PR #774）

- **merge**：#774 → `30279da3a`（hosted run 38358291473：rust 7m58s、conformance 15m56s、gates）。commit：b9c158da6（lsconv と URI）、
  eb0499111（overlay）、c4a690579（open file を持つ snapshot）、5d9874db0（Session）、f926027ba（server の document 通知）、
  29972e16b（test）。
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
- **検査（29972e16b）**：conformance 0 regressions（15,339、486 s）、suites 0 regressions、API client sync 337/356・
  async 346/365（変わらず）、clippy、fmt。
- **有界の違い**：open document の言語種別がまだ parse に届かない（次の slice、`SourceFile.ScriptKind`）。session の log は
  `Processed N file changes` だけで、tsgo の builder の log tree は L2c。watch の登録（L2b）、config 診断の push、診断の refresh、
  idle cache clean（L2c）、ATA、auto-import の warming、telemetry と progress（L5、L11）。fourslash の harness が protocol の外で
  呼ぶ `SetCompilerOptionsForInferredProjects` は記録器と共に L3。tsgo の `didOpen` は失敗しないが tsc-rs の snapshot 更新は
  host の誤りで失敗しうる（handler の誤りとして返す）。

## source file の script kind（PR #775）

- **merge**：#775 → `0883beab6`（hosted run 38362451488：rust 12m18s、conformance 20m53s、gates）。commit：d0cdd313c（script kind）、
  15dc8e1e8（`SourceFile::from_tree` の clippy の許可だけ）。
- **内容**：parse は tsgo の script kind をとり（言語の variant と JavaScript の context はそこから決まる、tsgo
  `initializeState`）、source file が保つ（`SourceFile.script_kind`、`is_source_file_js`）。prepared source の kind は content
  mapper の出力の仮想名、file handle（open document の言語）、file 名の順。project は open document の言語を load の時に渡し
  （保存した command line には入れない）、言語の変わった document は program の in-place の再利用をしない。checker の
  `IsSourceFileJS`・`IsPlainJSFile`・`IsInJSFile` に当たる判定は kind を読み、拡張子による対応の判定は tsgo と同じく名前のまま。
- **検査（d0cdd313c）**：conformance 0 regressions（15,339、500 s）、`--checkers 4` の対照は既知の 4 件だけ、suites 0 regressions、
  API client sync 337/356・async 346/365（変わらず）、crate の test（syntax 247、binder 78、program 669、checker 1,798、
  emitter 341、compiler 215、project 140、api 298、lsp 12）、clippy、fmt。L2a の有界の違い（untitled の JavaScript が
  TypeScript として parse される）を解消した。

## L2b file の監視（PR #776）

- **merge**：#776 → `117de4293`（hosted run 38364892878：rust 12m39s、conformance 20m53s、gates）。commit：e19f1959f（ef70ffd4a を
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

- **merge**：#777 → `1b2f9c8f2`（hosted run 38369297130：rust 12m8s、conformance 21m32s、gates）。commit：70bf8c263
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

- **merge**：#778 → `3390cf4d0`（hosted run 38377954175：rust 11m43s、conformance 20m31s、gates。一度目の run 38375873279 は
  `documents_reach_the_session` が log の順に依って落ちた：L2c の採用 log が background で間に入るため。test を順に依らない形に
  した e27ab0122）。commit：f7e648cb3、ratchet a169e2aa9、e27ab0122。
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
- **検査（f7e648cb3 + a169e2aa9）**：release build、`suites_ts71.py --update` と `--check` 0 regressions、
  `lsp_transcripts.py --check` 一致、API client sync 337/356・async 346/365（変わらず）、test、clippy、fmt。compiler の経路に
  届かないので conformance は回していない。

## L3b pull の診断（PR #779）

- **merge**：#779 → `e5a65d11d`（hosted run 38392705342：rust 12m4s、conformance 29m8s、gates。一度目の run 38390971840 は
  checker の単体 test が tsc 6.0.3 の構築 signature の文（`'(): object'`）を固定していて落ちた：tsgo は `new (): object`。七つの
  提案を Go の probe で確かめて期待値を直した dbd854e87）。commit：b69cecb54、01590459e、38c7c867c、f787e9e87、ratchet
  fd2df8fa7、dbd854e87。
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
- **検査（fd2df8fa7 + dbd854e87）**：conformance 0 regressions（全 13,577 full）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（lsp 150 full、診断 1,039/1,043、残りは content mapper の 4 → L10）、API client sync 337/356・async 346/365、
  test、clippy、fmt。

## L3c go to definition と type definition（PR #780）

- **merge**：#780 → `7adc501d8`（hosted run 38404039864：rust 8m34s、conformance 23m30s、gates）。commit：ca9012a9b（tree view と
  astnav を言語 service へ）、6db4e9605（sourcemap の decoder と document position mapper を emitter へ）、cda8ba959（機能）、
  ratchet 165ce1bbb。
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
- **検査（165ce1bbb）**：conformance 0 regressions（全 full、sourcemap は共有 decoder で 13,577 full）、`--checkers 4` は既知の
  4 だけ、suites 0 regressions（lsp 392 full、definition 472/482、typeDefinition 59/64、診断 1,039/1,043）、API client sync
  337/356・async 346/365、test、clippy、fmt。
- **有界の違い**：content mapper（L10）の 5 test、`preferGoToSourceDefinition`（L7）の 1 test。

## L3d-1 hover の verbosity 0（PR #781）

- **merge**：#781 → `400bc11d1`（hosted run 38427807862：rust 9m13s、conformance 19m7s、gates）。commit：a9e0ac987
  （JSDoc の `@` と comment の終わり）、56fc87aa5（printer の一行の type literal、末尾の semicolon の省略、`EscapeString`）、
  8b65b3035（checker の表示の文字列と verbosity、hover の node builder の規則）、1bab022a9（hover）、ratchet a34e40978。
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
- **検査（a34e40978）**：conformance 0 regressions（全 13,577 full、488 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（lsp 893 full、hover 2,325/2,455、definition・typeDefinition・診断は変わらず）、API client sync 337/356・
  async 346/365、test、clippy、fmt。
- **有界の違い**：verbosity 1 以上（L3d-2）68、Visual Studio の `_vs_rawContent`（L3d-3）53、content mapper（L10）9。

## L3d-2 hover の verbosity 1 以上（PR #782）

- **merge**：#782 → `1284daff3`（hosted run 38433092888：rust 9m43s、conformance 28m50s、gates）。commit：53513b346
  （checker の展開）、8818e85c9（hover）、ratchet 38e62b52d。
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
- **検査（56a630eab、main a0de78fb9 への rebase は packet の README だけ、ratchet 38e62b52d）**：conformance 0
  regressions（全 13,577 full、481 s）、`--checkers 4` は既知の 4 だけ、suites 0 regressions（lsp 925 full、hover
  2,393/2,455）、API client sync 337/356・async 346/365、test（checker 1,797、ls 61、api 295）、clippy、fmt。
- **有界の違い**：Visual Studio の `_vs_rawContent`（L3d-3）53、content mapper（L10）9。

## L3d-3 Visual Studio の hover の raw content（PR #783）

- **merge**：#783 → `1031f83d7`（hosted run 38443134012：rust 8m8s、conformance 28m30s、gates）。commit：1f93e200d
  （printer の display parts）、90fe8bb15（checker の分類された印字）、157a8bb87（hover）、ratchet 377007e6a。
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
- **検査（377007e6a、ratchet の commit は code を変えない）**：conformance 0 regressions（全 13,577 full、478 s）、
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

- **merge**：#784 → `3165d7889`（hosted run 38458124716：rust 12m33s、conformance 23m48s、gates）。commit：4597dcead
  （checker の service）、6761b372b（completion）、ratchet 90ce0a2f3。
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
- **検査（90ce0a2f3）**：conformance 0 regressions（全 13,577 full、478 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（lsp 1,371 full、completion 1,164/2,419、441 行が none→full）、API client sync 337/356・async 346/365、
  test（syntax 228、checker 1,797、ls 90、lsp 14、api 295、compiler 225、conformance 49）、clippy、fmt。
- **有界の違い**：L4b〜L4e、auto-import（L5）、content mapper（L10）、JSDoc の `function(...)` 型（tsgo は `function` と
  いう名前の型参照として読み、name table に入る）。

## L4b 決まった形の場所の completion（PR #785）

- **merge**：#785 → `cf2fb3792`（hosted run 38465407861：rust 11m48s、conformance 29m55s、gates）。commit：f5bb8fcc9
  （completion）、ratchet 46344558c。
- **言語 service**（`crates/ls/src/completions/searches.rs` ほか）：tsgo の `tryGetGlobalSymbols` が global の前に試す
  探索。型引数の object 型の member（制約の member から書いたものを除く）、object literal の文脈上の property と binding
  pattern の property（JavaScript の引数は `@param` の型。checker の `type_node_of` が tsgo の `Node.Type`）、書いた member と
  spread 代入が宣言する名前を除く sortText、import・export 句の module の export、import attribute、`export { | }` の
  local、class の基底の未実装 member と class・interface の member keyword、JSX element の attribute（optional と spread の
  sortText）。JSX の閉じ tag、`break`・`continue` の label、JSX attribute の snippet（`name={$1}`・`name="$1"`）、method
  snippet の設定時の object literal property の sortText も。
- **回帰 test**：`crates/ls/tests/completion.rs` に 11（計 35）。
- **検査（46344558c）**：conformance 0 regressions（全 13,577 full、476 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（lsp 1,527 full、completion 1,550/2,419、156 行が none→full）、API client sync 337/356・async 346/365、test
  （checker 1,797、ls 101、lsp 14、api 295、compiler 225、conformance 49）、clippy、fmt。
- **有界の違い**：class member・object literal method・網羅的な switch の case の snippet は tsgo の formatter
  （`format.FormatNodeGivenIndentation`）を使うので L8 の後。L4c〜L4e、L5、L10。

## L4c-1 文字列リテラルの completion（PR #786）

- **merge**：#786 → `33b843829`（hosted run 38473329341：rust 12m37s、conformance 23m48s、gates）。commit：fc48eea48
  （checker の候補一覧）、eadc7a612（completion）、ratchet 83e116ed1。
- **checker**：tsgo の `candidatesOutArray`。`getResolvedSignature` は候補を求められると cache 済みの呼び出しも解決し直し、
  `resolveCall` は候補を返し（選んだ候補・失敗の候補の位置はその instantiation。tsgo の slice の共有と同じ）、その間は
  error を報告せず、失敗では常に最長の候補を選ぶ。`GetCandidateSignaturesForStringLiteralCompletions`（編集中の引数からの
  推論を止めた候補、次に止めない候補）。`printer.EscapeString` は引用符を取る。
- **言語 service**（`crates/ls/src/completions/strings.rs`）：文脈上の型のリテラル、object literal の引用符付き property 名、
  element access と indexed access type の property 名、union（書いた member を除く）、型引数とその property、呼び出し・
  `new`・JSX の引数（候補の引数の型。引数の数も）、case 句（書いた case を除く）、import・export specifier、`in` 演算子。
  項目は tsgo の置き換え範囲と、literal の引用符に合わせた escape（tsgo は引用符を literal の値から読む）。
- **回帰 test**：`crates/ls/tests/completion.rs` に 11（計 46）。
- **検査（83e116ed1）**：conformance 0 regressions（全 13,577 full、491 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（lsp 1,592 full、completion 1,688/2,419、65 行が none→full）、API client sync 337/356・async 346/365、test
  （checker 1,797、emitter 196、ls 112、lsp 14、api 295、compiler 225、conformance 49）、clippy、fmt。
- **有界の違い**：path・module 名・triple-slash 参照（L4c-2）、snippet（L8）、L4d、L4e、L5、L10。

## L4c-2 path の completion（PR #787）

- **merge**：#787 → `b4496c6de`（hosted run 38480483705：rust 8m27s、conformance 29m30s、gates）。commit：427158120
  （module 解決の部品）、7012bb05f（path completion）、ratchet 071ed41b5。
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
- **検査（071ed41b5）**：conformance 0 regressions（全 13,577 full、488 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（lsp 1,650 full、completion 1,935/2,419、58 行が none→full）、API client sync 337/356・async 346/365、test
  （checker 1,797、program 114、types 44、ls 122、lsp 14、api 295、compiler 225、conformance 49、project 186）、clippy、
  fmt。
- **有界の違い**：`TestCompletionForStringLiteralNonrelativeImport2`（同じ名前の `repeated.ts` と `repeated.d.ts` の
  どちらの拡張子を残すかが tsgo の実行ごとに変わる。`scripts/lsp_transcripts.py` が非決定として扱う）。content mapper の
  file の path completion（L10）、L4d、L4e、L5、L8。

## L4d JSDoc の completion（PR #788）

- **merge**：#788 → `e58c88646`（hosted run 38486266220：rust 12m21s、conformance 29m56s、gates）。commit：7341aae8e
  （parser）、87d20b253（JSDoc completion）、ratchet 94d386223。
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
- **検査（94d386223）**：conformance 0 regressions（全 13,577 full、489 s。parser の修正で動く configuration はない）、
  suites 0 regressions（lsp 1,665 full、completion 2,044/2,419、15 行が none→full）、API client sync 337/356・async
  346/365、test（syntax 228、checker 1,797、ls 131、lsp 14、api 295、compiler 225、conformance 49）、clippy、fmt。
  checker の変更はないので `--checkers 4` はなし。
- **有界の違い**：`completionItem/resolve`（L4e）、auto-import（L5）、formatter が要る snippet（L8）、content mapper
  の file（L10）。

## L4e completionItem/resolve（PR #789）

- **merge**：#789 → `ea74a5424`（hosted run 38491566623：rust 10m12s、conformance 20m3s、gates）。commit：514d99bb5
  （resolve）、ratchet 522faf8da。L4（completion）はこれで auto-import（L5）、snippet（L8）、content mapper（L10）を
  除いて終わり。
- **言語 service**（`crates/ls/src/completions/resolve.rs`）：tsgo の `getCompletionItemDetails`。item data の位置で
  completion を計算し直し、表示名と source で symbol を見つける。detail は symbol の quick info、documentation は
  hover の `getQuickInfoAndDocumentationForSymbol`（client の最初の documentation format）。keyword・JSDoc tag・文脈の
  literal は名前が detail。文字列リテラルでは property の quick info か literal 型の名前（path completion はそのまま）。
  switch-case の snippet と auto-import はそのまま（auto-import の edit は L5）。request は LSP の位置だけでなく file の
  位置（item data の）も取れる。
- **LSP server**：`completionItem/resolve` は tsgo の `registerRequestHandler` と同じく同期部分で、item の file の
  default project が答える。panic は tsgo の `recover` の error。
- **回帰 test**：`crates/ls/tests/completion.rs` に `resolve` の harness と 5（計 70。keyword、JSDoc tag、文字列リテラル
  （literal 型、method、documentation 付きの property）、documentation 付きの変数、library の型の member）。
- **検査（522faf8da）**：suites 0 regressions（lsp 1,797 full、resolve 424/655、132 行がすべて none→full）、API client
  sync 337/356・async 346/365、test（ls 136、lsp 14）、clippy、fmt。compiler の path に触れないので conformance と
  `--checkers 4` はなし。
- **有界の違い**：auto-import の item（resolve 227。L5）、content mapper の file（4。先の completion の panic が session
  の checker state を poison する。L10）。

## L8a formatter（PR #790）

- **merge**：#790 → `4b4cd0861`（hosted run 38498011478：rust 12m36s、conformance 30m28s、gates）。commit：a2a8279f1（scanner の
  trivia token）、cbf3d50c5（formatter と formatting の request）、ratchet 1d7789c20。
- **順序**：L8 の format と change tracker を L5 の前に置く。tsgo の change tracker は挿入する node を formatter で整形し、
  L5 の import の edit と L4b に残した snippet がそれを使う。
- **scanner**（`crates/syntax/src/scanner.rs`）：tsgo の `SetSkipTrivia(false)`。空白、改行（`\r\n` は一つ）、comment
  （閉じていないものは unterminated）、conflict marker を trivia token として返す。tsgo と同じく shebang、行・段落の
  区切り、U+0085 は trivia を返すときも読み飛ばす。`TokenScanner` に formatter の呼び出し（`with_trivia`、
  `reset_token_state`、`re_scan_greater_token`、`re_scan_slash_token`、`scan_jsx_identifier`、
  `re_scan_jsx_attribute_value`）、`re_scan_jsx_token` は tsgo の `allowMultilineJsxText` を取る。parser の scanner は
  変わらない（既定で trivia を飛ばす）。
- **formatter**（`crates/ls/src/format/`）：tsgo の規則（同じ表・優先度・bucket の順）、文脈、formatting scanner、span
  worker（token の対、trivia、字下げ、行末の空白、範囲の後の edit）、node の字下げ（`GetIndentationForNode`、
  `ShouldIndentChildNode`、`NodeWillIndentChild`）、request の範囲（`FormatDocument`、`FormatSelection`、
  `FormatOnOpeningCurly`・`ClosingCurly`・`Semicolon`・`Enter`）。token navigation が見せる tsgo の木（`File`、node は
  `astnav::Found`）を tsgo の親（`Tree::parent`：heritage clause の TypeReference・QualifiedName、JSDoc comment の
  host）で読む。
- **言語 service**（`crates/ls/src/formatting.rs`）：`ProvideFormatDocument`・`Range`・`OnType`（comment の中、`{`・`}`・
  `;`・Enter 以外の文字では何もしない）。複製せず共有：`lsutil/asi.rs`（tsgo の `PositionIsASICandidate`。formatter の
  semicolon 規則と completion）、`utilities::range_of_enclosing_comment`・`is_in_comment`（tsgo の
  `getRangeOfEnclosingComment`・`isInComment`。completion が持ち、hover が縮めた形で持っていたものを三者が使う。答えは
  変わらない）。
- **LSP server**：`textDocument/formatting`・`rangeFormatting`・`onTypeFormatting`。
- **回帰 test**：`crates/ls/tests/format.rs` に 39（transcript の文書・設定・tsgo の edit。各 trigger、comment 中の入力、
  他の文字、文書と範囲、設定（行末の空白を残す、tab 幅 0、制御 block と関数の `{` を改行、空行から始まる object literal、
  `baseIndentSize`、`semicolons: remove`）、comment（tab 入りの複数行）、JSX、template、数値の後の `.`、型引数と `<<`、
  mapped・conditional 型、連鎖した呼び出し、semicolon のない文、JSDoc comment から始まる範囲、parse error、conflict
  marker）。`crates/syntax/tests/scanner_trivia.rs` に 3（Go の probe で tsgo の scanner から取った token 列）。
- **検査（1d7789c20）**：conformance 0 regressions（全 13,577 full、482 s）、suites 0 regressions（lsp 2,208 full、411 行が
  すべて none→full。onTypeFormatting 3,092/3,092、formatting 172/175、rangeFormatting 31/32）、API client sync
  337/356・async 346/365、test（syntax 250、ls 175、lsp 14、api 295、compiler 225、conformance 49）、clippy、fmt。checker
  の変更はないので `--checkers 4` はなし。
- **有界の違い**：content mapper の file の mapped range の整形（4。L10）。位置の字下げ（`GetIndentation`）と
  `FormatNodeGivenIndentation`、それを使う API の `formatNodeForInsertion` は L8b。

## L8b-1 挿入する node の印字・位置・字下げ（PR #791）

- **merge**：#791 → `80d8531b7`（hosted run 38508650596：rust 12m46s、conformance 19m53s、gates）。commit：3df17d6ef（型注釈・modifier の
  投影と完了 node の判定の共有）、b8aba24c6（`PrintAndPositionNode` と `preserveSourceNewlines`）、3cabf2f0b
  （`GetIndentation` と API の `formatNodeForInsertion`）、534e84205（test）。
- **共有**：binder の `node_util` に `type_annotation_of(_data)`・`type_node_of`・`modifiers_of_data`。checker（自分の node
  参照のまま）、印字の位置付け、言語 service が同じ照合を使う。tsgo の `PositionBelongsToNode`・`IsCompletedNode` は
  completion から `lsutil/completed_node.rs` に移り、smart indenter も使う（答えは変わらない）。
- **印字の位置**（`crates/emitter/src/{writer,printer}.rs`）：tsgo の `ChangeTrackerWriter`。node・token node・node list の
  印字の始まりと終わりを、trivia でない最後の text の終わりで記録する。node は node の pipeline（とその他二つの経路）、
  token は二項演算子と条件式の `?`・`:`、list は開き括弧の後と閉じ括弧の前（末尾の comma の後）、空の省略可能な list は
  その場所、mapped type の型引数、関数本体の文は prologue の後から。`as`・`satisfies`・非 null・型表明の式は自分の括弧を
  hook の外に書く（tsgo では親が書く）。
- **`preserveSourceNewlines`**（printer の option）：source の位置のない木での tsgo の挙動。`writeLineOrSpace` は
  `else`・`catch`・`finally`・do の `while` を同じ行に置き、埋め込みの文は新しい行で始まるもの以外を同じ行に置き、空の
  本体と block は行を分ける。source file の行を読む枝（`getEffectiveLines`、source の node の list の改行）は L8b-2。
- **`print_and_position_node`**（`crates/emitter/src/synthetic_file.rs`）：tsgo の `PrintAndPositionNode` と
  `CreateSyntheticSourceFile`。decode した node を印字し（comment も source file もない）、位置を写しに付けて合成の
  source file に包む。modifier list は位置を持たない（tsgo の新しい `ModifierList`）。`TransformArena::mark_single_quoted`
  が decode した literal の引用符を保つ（`printNode` も使う）。
- **formatter**（`crates/ls/src/format/`）：tsgo の `GetIndentation`（comment、string・template literal、comma の後と
  最初の前の list 要素、object literal、block と none の style、`getSmartIndent`）と `FormatNodeGivenIndentation`。
  `tsc_ls::format_node_for_insertion` が handler の後半（位置の字下げ、`ShouldIndentChildNode` の delta、
  `ApplyBulkEdits`）。
- **API**：`formatNodeForInsertion`（tsgo `handleFormatNodeForInsertion`：snapshot・program・file、node の decode、UTF-16
  の位置、snapshot の format 設定で印字と整形）。`createSnapshot`・`updateSnapshot` が `userPreferences`（tsgo の既定値に
  object の設定）を取り、snapshot の設定にする（tsgo の `CloneSnapshot` は API の request の設定を新しい設定にする）。
- **tsgo との照合（Go の probe）**：位置なしで decode した 157 文の印字と全 node・list の位置、`handleFormatNodeForInsertion`
  を写した 83 の挿入（client test の形、関数本体・入れ子の block・top level への 22 種の node、他の位置、設定）、4 file と
  二つの style の全位置への挿入 977。tsgo が答えるものはすべて一致。
- **回帰 test**：`crates/ls/tests/insertion.rs`（22 文の印字と位置、44 の挿入、各位置の字下げ。tsgo から）、
  `crates/api/tests/unit/session_print.rs` に 2（snapshot の設定での `formatNodeForInsertion`、error の順）。
- **検査（3cabf2f0b。534e84205 は test だけ）**：conformance 0 regressions（全 13,577 full、493 s）、`--checkers 4` は既知の
  4 だけ違う（checker の変更は `type_annotation_of` の共有だけ）、suites 0 regressions（ratchet の変化なし、lsp 2,208
  full）、API client sync 340/356・async 349/365（`formatNodeForInsertion` の 3 が通り、他の失敗は同じ）、test（binder 78、
  checker 1,798、emitter 348、ls 178、lsp 14、api 297、project 186、compiler 225、conformance 49）、clippy、fmt。
- **有界の違い**：tsgo の `AssignPositionsToNode` は `else` のない `if` に位置のない空の `else` block を足し（nil の文の
  `liftToBlock`）、formatter の assert で落ちる。tsc-rs は `else` のある `if` と同じに整形する。入れ子の namespace の暗黙の
  `export` の modifier list の位置は tsgo の -1 でなく 0（formatter は modifier を一つずつ読む）。`printNode` は
  `preserveSourceNewlines` をまだ使わない（L8b-2）。

## L8b-2 行の区切り・change tracker・organize imports・codeAction（PR #792）

- **merge**：#792 → `9d24c7ad0`（hosted run 38524336858：rust 12m47s、conformance 21m34s、gates）。commit：591dd6ab3（tsgo の
  行の区切りと source の node の `preserveSourceNewlines`）、3ca915e9d（change tracker、organize imports、
  `textDocument/codeAction`）、ec2744bbd（JSDoc 補完の改行）、a63a86fe7（ratchet）、be4a37958（`emitList` の MultiLine 規則）。
- **印字**（`crates/emitter/src/printer.rs`）：tsgo の `getLeadingLineTerminatorCount`・`getSeparatingLineTerminatorCount`・
  `getClosingLineTerminatorCount`（`getEffectiveLines`、`siblingNodePositionsAreComparable`、`originalNodesHaveSameParent`、
  `shouldEmitOnNewLine`、括弧式の前後の改行）を change tracker が印字する全 list に。tsgo は source の行を、親のない最初の
  子と一つの original の親を持つ parse 済みの兄弟だけで読み、`shouldEmitOnNewLine` は合成の node に限らず
  StartOnNewLine を読む。`emitList` の規則（MultiLine の node の list に PreferNewLine と Indented）を全 list で共有
  （`shouldEmitOnMultipleLines`。配列リテラルと binding pattern に無かった）。JSX の children の配列は改行の印を外す。
  `print_and_position` は source file のある node を印字し、位置を写し（`NodeArena::copy_subtree`）に付ける。
- **change tracker**（`crates/ls/src/change/`）：tsgo の `ls/change`。text・node・複数 node の置き換えと削除（list の要素、
  import、宣言：`delete.go`）、trivia の option による範囲（`getAdjustedStartPosition`・`EndPosition`）、`GetChanges`
  （印字・整形・重なりの検査）。node は tracker の emit arena に住み（file の node は同じ id、新しい node は factory）、
  `Tracker::syntax` が両方を読む。
- **organize imports**（`crates/ls/src/organize_imports.rs`、`lsutil/organize_imports.rs`）：未使用の import の除去（tsgo の
  checker の `IsDeclarationUsed`・`IsSymbolReferencedInFile`：参照の候補位置、shorthand、export specifier の local symbol、
  JSX の namespace と fragment factory）、空行での group 分け、属性ごとの結合（source phase、default と namespace、named、
  `a as a`）、検出した comparer（ordinal・natural・Unicode。表は `scripts/generate_unicode_tables.py` が Go の `unicode` と
  `norm` から生成し、`--check` で照合）での並べ替え、export の結合と並べ替え、ambient module。
- **codeAction**（`crates/ls/src/code_actions.rs`、LSP）：organize imports の 3 kind（`source.organizeImports(.ts)`、
  `sortImports`、`removeUnusedImports`）。tsgo の quick fix と fix-all は code fix の provider が出すもので、provider の
  ない今は何も加えない（tsgo の loop と同じ）。
- **修正**：JSDoc の `@param` 補完の改行。tsc-rs の `newLine` は crlf 0・lf 1、tsgo の enum は CRLF 1・LF 2 で、lf が CRLF に
  なっていた（`lsutil::new_line_kind` を共有）。
- **tsc 6.0.3 の契約 fixture**：list format flags 24、mapped type members 163（既存の override 312 の内）、import type
  attributes 2、printer failure hooks 1 の観測を tsgo の印字に置き換え（`tsgo_overrides` に理由）。Go の probe で tsgo の
  printer に同じ木を印字させ、両 fixture の 488 ケースすべてで一致。
- **回帰 test**：`crates/ls/tests/insertion.rs`（source の node 109 文の印字と位置）、`crates/ls/tests/completion.rs`
  （JSDoc の改行：crlf・lf・既定。tsgo の Go probe から）、emitter の契約 fixture。
- **検査（be4a37958）**：conformance 0 regressions（全 13,577 full、483 s）、`--checkers 4` は既知の 4 だけ違う、suites 0
  regressions（ratchet は 3ca915e9d の後に 242 行 none→full。lsp 2,208 → 2,450 full、codeAction 291/772 = organize
  imports 140/140 と空の quick fix 151）、API client sync 340/356・async 349/365、test（syntax 250、binder 78、checker
  1,798、emitter 348、ls 180、lsp 14、api 297、project 186、compiler 225、conformance 49）、clippy、fmt。
- **有界の違い**：quick fix 480 と fix-all 1（import の修正は L5、型注釈と interface の実装は L8c）、content mapper（L10）。
  tsc-rs の `clone_node` は位置を消し（tsc 6.0.3）、tsgo の `Clone` は保つ（印字の照合は同じ木で行った）。
- **次**：L5 auto-import（registry、import の修正、completion の auto-import、code action の再試行）。L8c の二つの provider は
  import adder を使うので L5 の後。

## L5 auto-import の計画

- **規模**：tsgo の `ls/autoimport` 5,507 行（registry 1,853、fix 1,329、import_adder 502、extract 476、util 338、view 264、
  aliasresolver 248、export 188、index 186、specifiers 90）、`project/autoimport.go` 170、`ls/codeactions_importfixes.go` 449、
  completion の auto-import の部分。
- **transcript**：completion の auto-import の項目 361（project 内の相対 207・110 場面、node_modules 123・54 場面、project の
  ambient module 31・17 場面）、その resolve 227、quick fix の `Add import from` 284（190 場面。node_modules 81・50）、
  `Update import from` 80（37 場面）、fix-all の `Add all missing imports` 38（24 場面）、`Remove … from import` など。
- **L5a project の registry と completion**：`Export`（module と export の ID、構文、flag、別名の先、script element kind）、
  program の file からの抽出（binder の symbol、別名の解決に checker）、`Index`（語の頭文字の索引、`Find`・
  `SearchWordPrefix`）、`View`（検索、`GetCompletions` の group 化と順位）、session の registry（snapshot ごとの clone、
  `IsPreparedForImportingFile`、`ErrNeedsAutoImports` で準備して request をやり直す handler）。completion の auto-import の
  項目（sort text 16、`data.autoImport`）と resolve の edit（`fix.go`：既存の import への追加、新しい import 宣言、
  type-only、module specifier。change tracker を使う）。
- **L5b import の code fix**：`codeactions_importfixes.go`（未解決の名前の診断からの fix、`Update import from`、fix-all）、
  import adder（L8c と API の `getImportAdderEdits` が使う）。
- **L5c node_modules**：package.json の依存、entrypoint（exports・typesVersions）、program 外の package の file を読む
  alias resolver の checker、ambient module、上位の node_modules の遮蔽、`@types/node` と `node:`。
- **後へ**：auto-import の監視（L11）、background の warm、telemetry。L8c の二つの provider は L5b の import adder の後。

## L5a project の auto-import と completion（PR #793）

- **merge**：#793 → `30a6310a9`（hosted run 38549670836：rust 12m16s、conformance 30m42s、gates）。commit：d7c28c402（tsgo の
  triple-slash と pinned comment の規則）、f0a39af24（tsgo の vfsmatch）、4e36b82ea（auto-import の completion と edit）、
  b691e2d1a（ratchet）。
- **registry**（`crates/ls/src/autoimport/`）：`Export`（module と export の ID、構文、flag、別名の先、script element kind）、
  program の file からの抽出（binder の symbol、別名の解決に checker）、語の頭文字の `Index`、snapshot ごとの
  `Registry`（project の bucket、dirty の印、要求された file のための再構築）。bucket の対象は tsgo の `buildProjectBucket`
  と同じ：default library・`node_modules` の下・project の外から `node_modules` の link で読んだ file（tsgo
  `hasSymlinkToNodeModules`。program の既知の link から）・`autoImportFileExcludePatterns` に合う file を除く。
- **view と fix**：検索（語の頭文字）、completion の group 化と順位、fix（namespace の使用、既存の import への追加、新しい
  import・require、JSDoc の import type）、module specifier（program の `paths`・`rootDirs` を含む。以前の auto-import
  用の入口は落としていた）、`shouldUseRequire`（tsgo `GetEmitModuleDetectionKind`：node16〜nodenext は force）。
- **completion と resolve**：auto-import の項目（sort text 16、`labelDetails.description`、`data.autoImport`）、import 文の
  completion の挿入文。resolve は change tracker の挿入（`InsertNodeAt`・`Before`・`After`、`InsertNodeInListAfter`、
  `InsertImportSpecifierAtIndex`、`InsertAtTopOfFile`、`InsertModifierBefore`）と検出付きの import comparer で edit を作る。
  quote の好みは file の最初の import から（completion と共有）。
- **session**：snapshot の registry、`ErrNeedsAutoImports` で準備した snapshot に request（completion、codeAction）をやり直し、
  背後で採用する（session が先に進んでいれば捨てる）。
- **共有した tsgo の部品**：`tsc_program::vfsmatch`（`SpecMatcher`。config の wildcard directory の除外も使う）、emitter の
  `is_recognized_triple_slash_comment`・`is_pinned_comment`（`<amd-module/>`、`/*!*/` は pinned でない）、
  `tsc_syntax::get_shebang`。
- **検査（4e36b82ea）**：conformance 0 regressions（全 13,577 full、497 s）、`--checkers 4` は既知の 4 だけ違う、suites 0
  regressions（ratchet は main のものから `--update`：111 行 none→full、下げは無し。lsp 2,450 → 2,561 full、completion
  2,213/2,419、resolve 651/655、import の completion の場面の resolve 130/130）、API client sync 340/356・async 349/365、
  test（syntax 251、binder 78、program 674、checker 1,798、emitter 350、ls 180、lsp 14、api 297、project 186、compiler
  225、conformance 49）、clippy、fmt、Unicode の表。
- **教訓**：`suites_ts71.py --update` は段を下げない。slice の途中で working tree の ratchet を `--update` すると、前の
  code が得た段が残り、後の `--check` の「regression」が揺れに見える。最後の `--update` の前に ratchet を main のものに
  戻す。
- **有界の違い**：package の export（L5c）、import の code fix と fix-all（L5b）、content mapper の file（L10）、JSDoc の
  `@import`（tsgo の `Imports()` は reparse した宣言を含む）、大文字小文字を区別しない glob の比較は Rust の単一文字の
  case 対応、config の file の探索は 6.0.3 の `files` の pattern のまま（vfsmatch への移行は後で）。
- **次**：L5b import の code fix（provider と fix-all、type-only の昇格、import adder）。

## L5b import の code fix（PR #794）

- **merge**：#794 → `aeeffc91a`（hosted run 38559044085：rust 12m、conformance 30m07s、gates）。commit：967950fc4（Go の
  unicode の関数と `IsIntrinsicJsxName` を `tsc_syntax` に）、29794e210（module specifier の `paths` の pattern と project
  reference の出力）、c0438e50c（import の code fix）、e1365be70（ratchet）。
- **code fix の枠組み**（`crates/ls/src/code_actions/mod.rs`）：tsgo の `ProvideCodeActions` の provider の loop（要求の診断
  ごとに quick fix、`CodeAction.Compare` での重複除去）、provider ごとの「file 全体の修正」（修正できる診断が 2 つ以上の
  とき）、`source.fixAll`。今の provider は import だけ（L8c で型注釈と interface の実装を足す）。
- **import の provider**（`code_actions/import_fixes.rs`）：tsgo の `codeactions_importfixes.go`。未解決の名前（JSX の tag と
  JSX の namespace を含む）、module の中の UMD の global、値として使った type-only の import。tsgo の順に並べる。fix-all
  （`Add all missing imports`）は各診断の最良の fix を import adder（`autoimport/import_adder.rs`：同じ module の import を
  一つの宣言にまとめる）に入れる。
- **fix の edit**：tsgo の `promoteFromTypeOnly`（import specifier、import clause と namespace import、`import type =`）。
  tsgo は昇格の説明に空の名前を入れる（`Remove 'type' from import of '' from …`）のでそのまま。view の完全一致・大文字
  小文字を区別しない検索、`SymbolToExport`。
- **module specifier**（checker）：tsgo の `resolvePathPatternIfInSameVolume`（`paths` の pattern を paths の基準に対して
  解決：root からの `..\x` は `x`）、star の前後の比較は host の大文字小文字の区別に従う。auto-import の host は参照先
  project の出力の `.d.ts` を先に候補にする（tsgo `GetOutputAndProjectReference`。program と
  `ResolvedProjectReferences::output_dts_of_source` を共有）。
- **共有**：`tsc_syntax::go_unicode`（LS から移し、Go の `SimpleFold`・`EqualFold` を戻した。vfsmatch の大文字小文字を
  区別しない比較も Go の `EqualFold` に）、`tsc_syntax::is_intrinsic_jsx_name`（checker と emitter の私的な写しを置換）。
- **検査（c0438e50c）**：conformance 0 regressions（全 13,577 full、493 s）、`--checkers 4` は既知の 4 だけ違う、suites 0
  regressions（ratchet は main のものから `--update`：180 行 none→full、下げは無し。lsp 2,561 → 2,741 full、codeAction
  291 → 527/772。完全に一致する場面で `Add import from` 166/226、`Update import from` 56/63、`Add all missing imports`
  33/38、`Remove 'type' …` 11/11、`Change …` 7/7、`Fix All` 1/1）、API client sync 340/356・async 349/365、test（syntax
  251、binder 78、program 674、checker 1,799、emitter 350、ls 180、lsp 14、api 297、project 186、compiler 225、
  conformance 49）、clippy、fmt、Unicode の表。
- **有界の違い**：package の import（L5c。残る import の fix の差はすべて package）、他の code fix の provider（L8c）、
  同じ順位の fix の順（tsgo の sort は不安定、tsc-rs は安定。観測なし）。

## L5c node_modules の計画

- **規模**：tsgo の `registry.go` の node_modules の部分（directory と package.json、`computeDependenciesForNodeModulesDirectory`、
  `discoverBucketPackages`（realpath、`@types`）、`extractPackage`、`installExtractions`、`buildNodeModulesBucket`・
  `updateNodeModulesBucket`、ambient module の名前の解決）約 1,000 行、`aliasresolver.go` 248、`util.go` の package 名・
  依存・realpath、module の `GetEntrypointsFromPackageJsonInfo`（exports map、main、directory search）、view の
  node_modules bucket の検索（package.json の依存と program の解決した package での絞り込み、下の node_modules の遮蔽）、
  `specifiers.go` の package の module specifier。
- **難所**：tsgo は package の entrypoint を root に、別名の解決に要る file だけを遅延で読む alias resolver の checker
  （`checker.NewChecker(aliasResolver)`）で抽出する。tsc-rs の checker は program の file を先に束ねる。→ entrypoint を
  root にした小さな program（module 解決で推移的に読む、lib なし）の checker で抽出し、export と別名の先が tsgo と同じか
  transcript で確かめる。
- **段階**：L5c-1 entrypoint の列挙（`tsc_program`）、package の発見と抽出、node_modules bucket、view の検索と絞り込み・
  遮蔽、package の module specifier（completion と quick fix の package の項目）。L5c-2 package の ambient module
  （`declare module`、失敗した ambient module の解決の補い）、`@types` への fallback、`node:` の core module。L5c-3
  granular な更新（dirty な package、workspace の package、recursive search。監視は L11）。
- **transcript**：completion の node_modules の項目 123（54 場面）、quick fix の package の `Add import from` 81（50 場面）、
  `Update import from`、fix-all の一部。

## L5c node_modules の auto-import（PR #795）

- **merge**：#795 → `9a721f557`（hosted run 38577604092：rust 13m、conformance 32m07s、gates）。commit：9d6f8fd03（package の
  entrypoint と alias resolver の checker の事実）、e923d0323（node_modules bucket）、a72b02a45（ratchet）。
- **entrypoint**（`tsc_program` の `module_resolution/entrypoints.rs`）：tsgo `GetEntrypointsFromPackageJsonInfo`。`exports` の
  target を条件付きで（tsgo の除外条件の積み方そのまま）、pattern は config の走査（= tsgo `vfs.ReadDirectory`）で、無ければ
  main と directory search。symlink の entrypoint は実体の path。package 名・拡張子・prefix/suffix の補助関数は checker の
  私的な写しをやめて `tsc_program` に集めた。
- **alias resolver**（`autoimport/alias_resolver.rs`）：tsgo `aliasresolver.go`。package の entrypoint を program とする
  `LiveChecker`。entrypoint の module 要求が解決する file は program の外として読み（checker は global に合流させない：
  `ProgramFileFacts::OUTSIDE_PROGRAM`）、checker の解決は ESM・既定の options で事前に解いた表から答え、解決できない
  非相対の名前を記録する（第二段の ambient module の補い）。tsgo の alias resolver は JSX runtime の import を持たない
  （provider の `synthesizes_jsx_runtime_import`）。tsgo は解決のたびに file を読むが、tsc-rs は checker を作る前に読む。
- **registry**（`registry.rs`、`node_modules.rs`）：開いた file の上の directory と package.json、node_modules ごとの bucket
  （package.json の依存と program の package、`knownRecursiveSearchPackages`、deep import）、実体の directory ごとに一度の
  抽出、`@types` への fallback、ambient module の第二段、workspace package の部分更新、registry の entrypoint。
- **view**：上の bucket の検索（package.json の依存＋program の package、下の bucket が上を隠す）、file の条件に合う最初の
  entrypoint の specifier（`ProcessEntrypointEnding`）、`node:` の core module の group と順位（tsgo の program 側の
  `UsesUriStyleNodeCoreModules` は設定されないので file の import だけで決まる）。project bucket は program の package を記録し、
  相対の module augmentation は既定の options で解決（tsgo `moduleResolver`）。
- **検査（e923d0323）**：conformance 0 regressions（全 13,577 full、490 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（ratchet は HEAD から `--update`：105 行 none→full。lsp 2,741 → 2,846 full、completion 2,213 → 2,285/2,419、
  codeAction 527 → 592/772）、API client sync 340/356・async 349/365、test（program 680、checker 1,800、ls 184 ほか全通過）、
  clippy、fmt、Unicode の表。
- **有界の違い**：node_modules の場面で残るのは class member snippet（L8）、content mapper（L10）、他の code fix（L8c）。
  `TestCompletionForStringLiteralNonrelativeImport2` は tsgo 側が非決定。動的（URI）package の entrypoint は未対応（resolver が
  未対応）。package の並列抽出は無し（性能は後で）。

## L8c 他の code fix の計画

- **対象**（transcript の欠けている action）：interface の実装（`Implement interface` 98、`Implement all unimplemented
  interfaces` 4）と isolated declarations の型注釈（`Add satisfies and an inline type assertion` 69、`Add annotation of type`
  46、`Add return type` 43、`Extract …` 47、`Add all missing type annotations` 15、expando 5、`Mark array literal as const` 2）。
- **基盤**：node builder の出力を change tracker の arena に作る（checker の `type_to_type_node_with_identifier_symbols` などを、
  tracker の arena に program の file を source として登録して呼ぶ）。import adder に tsgo の `AddImportFromExportedSymbol`、
  `TypeToAutoImportableTypeNode`、`TryGetAutoImportableReferenceFromTypeNode`、view の `SearchByExportID`。
- **段階**：L8c-1 interface の実装（`codeactions_fixclassincorrectlyimplementsinterface.go` 242、`missingmemberfixer.go` 514：
  property・accessor・method・index signature、stub の body、class member の挿入）。L8c-2 型注釈
  （`codeactions_fixmissingtypeannotation.go` 1,421）。

## L8c-1 interface の実装（PR #796）

- **merge**：#796 → `d1cfca8f6`（hosted run 38601240787：rust 9m38s、conformance 28m35s、gates）。commit：bc21fa275（node builder
  を呼び出し側の arena に、thisless の instantiation、匿名の名前、型の子の comment）、ecb9a94dd（interface の実装）、083716771
  （ratchet）。
- **provider**（`code_actions/implement_interface.rs`、`missing_member.rs`）：tsgo の
  `FixClassIncorrectlyImplementsInterfaceProvider` と missing member fixer。実装する型ごとの `Implement interface 'I'` と fix-all
  `Implement all unimplemented interfaces`（全 diagnostic の class）。足りない member（宣言も継承もなく private でないもの）と
  class に無い number・string の index signature を node builder の型で property・accessor・method にし、stub の body
  （`throw new Error("Method not implemented.")`）をつけて constructor の後か class の先頭（`InsertMemberAtStart`）に挿入する。
  型の中の import type は import adder（`AddImportFromExportedSymbol`）で import にする。
- **基盤**：node builder の出力（型・signature・index signature）を呼び出し側の transform arena に作る checker の export
  （idToSymbol 付き）。change tracker は program の file をその id で mount し、node builder が再利用した node の位置と comment
  が残る。
- **compiler の修正**（それぞれ Rust の test 付き）：
  - `instantiateSymbol`（tsgo checker.go:21166-21200）：`this` だけを写す mapper では `this` を含まない symbol をそのまま返し、
    `isThisless` は parameter も見る（tsc 6.0.3 の版のままだった）。`I<this>` の member の parameter が宣言のままになり、
    node builder が `i is I` を再利用できる（`boolean` に落ちていた）。
  - `getNameOfSymbolAsWritten`（nodebuilderimpl.go:974-1027）：匿名の class・関数の名前を書くのは、匿名の識別子を許さない
    限り node builder の error（匿名の class を返す method には action を出さない）。
  - 宣言出力の comment：signature の return type、型 parameter の名前・制約・既定、mapped type の parameter・name type・type、
    template literal type の span の型、関数型の return type の後ろの comment を tsgo のとおり出す（`m(): string /** c */;` の
    comment が落ちていた。conformance は検出しない）。
- **検査（ecb9a94dd のコード）**：conformance 0 regressions（全 13,577 full、470 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（ratchet は HEAD から `--update`：92 行 none→full。lsp 2,846 → 2,938 full）、2420・2720 の diagnostic を持つ
  96 場面すべて一致、宣言出力の probe 4 file が tsgo と byte 一致、test（checker 1,802、compiler 226、ls 184 ほか全通過）、
  API client sync 340/356・async 349/365（main と同じ失敗の一覧）、clippy、fmt。
- **有界の違い**：lsp の 21 の panic（`completions/jsdoc.rs:565`、続く `checker/src/emit.rs:242` の PoisonError）は main と
  同じで、すべて `TestContentMapper*` の 10 場面（content mapper の file の completion）＝ L10。

## L8c-2 型注釈の code fix（PR #797）

- **merge**：#797 → `200369129`（hosted run 38610122507：rust 13m36s、conformance 20m53s、gates）。commit：c429da02b（checker・
  emitter の export）、0bf9147f5（型注釈の code fix）、fb13c1586（ratchet）。
- **provider**（`code_actions/missing_type_annotation.rs`）：tsgo の `IsolatedDeclarationsFixProvider`
  （`codeactions_fixmissingtypeannotation.go`）。`Add annotation of type`、`Add return type`、`Add satisfies and an inline type
  assertion`、`Extract …`、expando、`Mark array literal as const` と fix-all `Add all missing type annotations`。
- **基盤**：change tracker の `try_insert_type_annotation`、arrow function の parameter の括弧、一行の印字（tsgo
  `typeToStringForDiag`：157 byte で切り `...`、切った byte ごとに U+FFFD）。emitter の `deep_clone_node`（tsgo
  `DeepCloneNode`：宣言出力の位置の無い部分木の共有はしない）と、node builder の emit flag・comment を忘れる
  `forget_emit_flags_and_comments`（tsgo の `TypeToTypeNodeEx` は新しい EmitContext で作るので、tracker は builder の flag を
  見ない：tuple が複数行にならない）。checker の export：`type_predicate_to_type_predicate_node_in_arena`、
  `requires_adding_implicit_undefined`、`get_widened_literal_type`、`EmitNodeBuilderFlags::ALLOW_UNIQUE_ES_SYMBOL_TYPE`。
  tsgo の `ast.IsDeclaration` は `node_util::is_declaration_node` に一本化。
- **検査（fb13c1586）**：conformance 0 regressions（全 13,577 full、475 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（ratchet は HEAD から `--update`：68 行 none→full。lsp 2,938 → 3,006 full）、対象の 69 場面のうち 68 が一致、
  test（checker 1,802、compiler 226、ls 184 ほか全通過）、API client sync 340/356・async 349/365（main と同じ失敗の一覧）、
  clippy、fmt。
- **有界の違い**：`TestContentMapperDiagnosticCodeDoesNotSelectTypeScriptFix`（content mapper の file＝L10）。


## L7 参照・rename・移動の計画

- **対象**（transcript の欠けている応答を持つ場面の数）：references 405、rename 203、documentHighlight 139（L6 の機能だが参照の
  検索を使う）、sourceDefinition 91、implementation 69、willRenameFiles 40、call hierarchy 39（prepare・incoming・outgoing）、
  prepareRename 26、workspace symbol 17、code lens 16（resolve を含む）、multiDocumentHighlight 7、`_vs_references` 6。
- **順**：参照の検索がこの多くの土台なので、L6 より先に行う（利用者の指示：順は任せる）。
- **段階**：
  - L7a 参照の検索（`findallreferences.go` 2,764、`importTracker.go` 768）：entry と definition、symbol の scope、名前の
    文字列検索と位置の検証、関連 symbol（root symbol、基底型の同名の property、contextual type の property、分割代入、
    parameter property、export specifier）、import・export の追跡（直接の import の表、間接の利用者、名前空間 import、
    re-export）、特別な参照（this・super・label・keyword・`import.meta`・文字列リテラル）、module の参照（import・
    triple-slash・暗黙の JSX runtime と import helper）、constructor と class の検索、実装の queue。`textDocument/references`、
    `_vs_references`、`textDocument/implementation`。
  - L7b document highlight（`documenthighlights.go` 797）と `custom/textDocument/multiDocumentHighlight`。
  - L7c rename（`rename.go` 463：`prepareRename`、`rename`、prefix・suffix の文字列）と `workspace/willRenameFiles`
    （`file_rename.go` 404）。
  - L7d call hierarchy（`callhierarchy.go` 1,138）と code lens（`codelens.go` 243）。
  - L7e source definition（`sourcedefinition.go` 749）、workspace symbol、複数 project の orchestrator（`crossproject.go` 436）。
- **checker**：既にある `GetRootSymbols`、`GetPropertySymbolsFromContextualType`、`GetShorthandAssignmentValueSymbol`、
  `GetExportSpecifierLocalTargetSymbol`、`GetImmediateAliasedSymbol` に、`GetPropertySymbolOfDestructuringAssignment`、
  `GetSymbolsOfParameterPropertyDeclaration`、emit resolver の `IsDeclarationVisible` の export を足す。

## L7a 参照の検索（#798）

- **merge**：#798 → `01aeda193`（hosted run 38632470929：rust 13m3s、conformance 30m46s、gates。最初の run の後に main
  が進み、branch protection の strict（base に追いついていること）で merge できなかったので `gh pr update-branch` で
  main を取り込み、その run で通った）。commit：765543519（compiler 側）、e538e9662（language service）、f8028bf42
  （ratchet）。
- **compiler 側**：JSDoc の `C#m` を qualified name に、`@see {…}` の link 名（tsgo `parseJSDocLinkName`・
  `parseJSDocNameReference`。tsc 6.0.3 の `JSDocMemberName` は消した）、tsgo の `resolveJSDocMemberName` と class member の
  fallback。reparse の模倣（`GetReparsedNodeForNode`）を typedef・callback の名前、`@callback`・`@overload` の
  parameter、type literal の property 名へ。checker の export：`GetPropertySymbolOfDestructuringAssignment`（分割代入の
  検査は tsgo と同じく型を返す）、`IsArrayLiteralOrObjectLiteralDestructuringPattern`、
  `GetSymbolsOfParameterPropertyDeclaration`、synthetic import の module symbol、write access、super container。
  `tsc_program::synthetic_imports`（`tslib`・JSX runtime）を module request の計画と language service で共有。
- **language service**（`find_all_references/`）：特別な検索（型 keyword、`import.meta`、label、this、super、文字列
  リテラル、module）、symbol の検索（scope、関連 symbol、基底型、contextual な property、分割代入、parameter property、
  export specifier、constructor・class）、import tracker（直接の import、間接の利用者、名前空間 import、re-export、JSDoc
  `@import`、import type）、module の参照、実装の queue、VS references（quick info の run と read/write の種別）。
  JavaScript file の `@import` を module の参照に数える（tsgo の reparse した `JSImportDeclaration` の位置）が、
  auto-import の既存 import には数えない（tsgo の `JSDocImportTag` の case は成り立たない）。回帰 test は
  `crates/ls/tests/references.rs`（11 件）。
- **検査（f8028bf42）**：conformance 0 regressions（全 13,577 full、485 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（ratchet は HEAD から `--update`：424 行 none→full。lsp 3,006 → 3,430 full）、参照の 480 場面のうち 440 が
  一致、test（checker 1,802、ls 195 ほか全通過）、API client sync 340/356・async 349/365（main と同じ失敗の一覧）、clippy、
  fmt。
- **有界の違い**：複数 project の 6 場面（L7e の orchestrator）、content mapper の 5 場面（L10）、残りは rename・
  highlight・source definition（L7b・L7c・L7e）。
- **教訓**：main の branch protection は strict。PR が base より遅れていると gates が通っても merge できない。merge の前に
  `gh pr update-branch`、head branch は `state` が MERGED になってから消す（先に消すと PR が閉じる）。

## L7b document highlight（#799）

- **merge**：#799 → `7eca40f9c`（hosted run 38637173027：rust 13m7s、conformance 21m35s、gates）。commit：39c70a6f2（checker
  の this 型）、b26c208b7（document highlight）、dc6363c98（ratchet）。検査の後の main の取り込み（764fb62c0）は packet の
  README だけ。
- **checker**：this を持たない interface の this 型は error 型（tsgo の `getThisType` は
  `Coalesce(declaredType.thisType, errorType)`）。`typeof this` だけを持つ interface では ContainsThis が立たず this 型が
  無い。tsc-rs は language service がその `this` の symbol を問うと panic していた（`TestDocumentHighlightTypeofThis`）。
  compiler はそこに来ない（その file の診断は tsgo と同じ）。
- **language service**（`document_highlights.rs`、tsgo `documenthighlights.go`）：JSX tag の開きと閉じ、file（または検索
  する file）の中の symbol の参照（L7a の検索を adjusted location なしで、read・write の種別つき）、それ以外は keyword
  の組（if/else、return/throw、try/catch/finally、switch/case/default、loop と break/continue、accessor、constructor、
  async/await、yield、modifier）。共有：`Navigator::children_from_non_js_doc`（tsgo `getChildrenFromNonJSDocNode`）、
  欠けた（幅 0 の）token の開始位置（tsgo `GetTokenPosOfNode`）、completion と formatter の
  `IsWhiteSpaceSingleLine` を一つに。回帰 test は `crates/ls/tests/references.rs`。
- **検査（dc6363c98）**：conformance 0 regressions（全 13,577 full、487 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（147 行 none→full。lsp 3,430 → 3,577 full）、highlight の 153 場面のうち 147、参照の 480 場面のうち 442 が
  一致、test（checker 1,802、ls 197 ほか全通過）、API client sync 340/356・async 349/365（main と同じ失敗の一覧）、
  clippy、fmt。
- **有界の違い**：`TestContentMapperDocumentHighlights`（L10）、残りは rename（L7c）。

## L7c rename・ファイルの rename（#800）

- **merge**：#800 → `dcc2ae3d4`（hosted run 38643214451：rust 10m55s、conformance 21m22s、gates）。commit：fd9af80ee
  （共有の tspath 補助）、718068194（rename・prepare rename）、8a5c14758（`change_full_extension` の修正）、817dbf891
  （ファイルの rename）、7837100b3（ratchet）。検査の後の main の取り込みは L7b の merge（同じ code）と packet の
  README だけ。
- **rename**（`rename.rs`、tsgo `rename.go`）：`GetRenameInfo`（宣言のある symbol、contextual な文字列リテラル型を持つ
  文字列、label。標準 library・`default` の import・別の `node_modules` package の symbol は不可。相対 module specifier
  は editor ができればそのファイルを rename）、`ProvideRename`（参照の編集。別名が要るところでは古い名前を残す：
  shorthand property、binding element、`module.exports` の shorthand、import・export specifier。数値 index は引用符付き。
  編集の重複は除き、食い違えば答えなし）。`prepareRename` は理由を `RequestFailed` で返す。
- **compiler 側の修正**：`change_full_extension` を tsgo の `ChangeFullExtension` に（宣言拡張子は `.d` から：
  `app.d.css.ts` → `app.css`。以前は `app.d.css.css`。拡張子の無い path はそのまま：exports の `./dist/**/*` が
  `./dist/**/*.*` になっていた）。unit test つき。
- **ファイルの rename**（`file_rename.rs`、tsgo `file_rename.go`）：config の path（`files`・`include`・`exclude`、path
  option、`paths`。合わなくなる include には新しい path を足す：tsgo `GetMatchedIncludeSpec` を
  `ConfigRootPlan::matched_include_spec` に）、相対 triple-slash 参照、対象かファイル自体が動く module specifier
  （checker に tsgo `UpdateModuleSpecifier` を `update_module_specifier` として追加。解決しない import は動いたファイルと
  相対 path で補う）、宣言ファイルの元（`foo.d.css.ts` と `foo.css`）。project system に tsgo の全 project の
  project tree 要求（`DidRequestProjectTrees`）と snapshot の language service project を足し、
  `workspace/willRenameFiles` はその全部で tsgo の重複除去をする。`textDocument/rename` は tsgo と同じく plain な request
  handler にした（willRenameFiles を持たない client の module specifier の rename が、既定 project の program の外で
  ファイルの rename の編集を計算できるように）。
- **検査（7837100b3）**：conformance 0 regressions（全 13,577 full、489 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（243 行 none→full。lsp 3,577 → 3,820 full）、rename・prepareRename・willRenameFiles の 257 場面のうち 253、
  test（program 681、ls 201 ほか全通過）、API client sync 340/356・async 349/365（main と同じ失敗の一覧）、clippy、fmt。
- **有界の違い**：content mapper の 4 場面（L10）。

## L7d call hierarchy・code lens（#801）

- **merge**：#801 → `7b4a25842`（hosted run 38647895390：conformance 31m36s、rust は 1 回目に 1 件落ちて再実行で 12m42s、
  gates）。commit：a85834dd2（共有の補助）、6235f0cbd（call hierarchy・code lens）、06a1d62cf（ratchet）。検査の後の main
  の取り込みは L7c の merge（同じ code）と packet の README だけ。
- **1 回目の rust の失敗**：`crates/lsp/tests/server.rs` の `pushed_config_diagnostics_match_tsgo` で、server の終了 status
  が tsgo の記録の 0 でなく 1。fixture は `exit` を送ってすぐ入力を閉じる。reader の入力の終わり（正常）と dispatcher の
  `exit`（`context canceled`。入力を開いたままなら status 1 になるのが tsgo で、`exit_stops_the_server` がそれを固定して
  いる）の競合で、先に止めた方が status を決める。tsgo も同じ競合を持つ（`handleExit` の `io.EOF` → dispatch の
  `context.Canceled` と read loop の `io.EOF` のどちらが errgroup の最初の error になるか）。server の意味は変えず、
  落ちた job の再実行で通した（以前の PR では起きていない、まれな flake）。
- **共有の補助**：`node_util` に tsgo の `Node.Initializer()`（property signature のものも）と `Node.Members()`、
  `getAssignedName` を public に。language service の複製（参照の検索、completion、document highlight、型注釈の fix）が
  それを使う。`Node.Text` は 3 つを 1 つに、`SymbolToString` は 2 つを 1 つに、tsgo の `isCalleeWorker`（call・new・
  tagged template・decorator・JSX の target。element access と外側の式も）を参照検索の狭い callee 判定の代わりに。
  `ast.IsExpression` を request へ。実装の変換は tsgo の `dropOriginNodes` の位置（code lens のもの）を取る。
- **call hierarchy**（`call_hierarchy.rs`、tsgo `callhierarchy.go`）：item になれる宣言（file、module、class の static
  block、名前のある呼べるもの）、参照からは symbol を 1 度だけたどる。tsgo の名前（`default`、`static {}`、
  `(anonymous)`、symbol の名前）と container 名。incoming は参照の検索から（呼ぶ参照を、それを囲む宣言ごとに）、outgoing
  は宣言の本体から（ambient、型、他の宣言の中は見ない）。どちらも file と最初の範囲の順、同じ相手の範囲はまとめる。
- **code lens**（`code_lens.rs`、tsgo `codelens.go`）：preference が有効にする references・implementations の lens（overload
  は 1 度）、resolve で数と client の show-locations command。session から消えた file の lens は `ContentModified`。
  tsgo `getSymbolKindFromNode` で `symbols.rs` を始めた。
- **検査（06a1d62cf）**：conformance 0 regressions（全 13,577 full、490 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（48 行 none→full。lsp 3,820 → 3,868 full）、call hierarchy・code lens の 55 場面のうち 48、test（ls 202 ほか
  全通過）、API client sync 340/356・async 349/365（main と同じ失敗の一覧）、clippy、fmt。
- **有界の違い**：複数 project の 3 場面（L7e の orchestrator）、content mapper の 4 場面（L10）。

## L7e source definition・workspace symbol（#802）

- **merge**：#802 → `f0dac87cc`（hosted run 38655436044：rust 10m49s、conformance 31m0s、gates）。commit：0b484dd2f
  （compiler 側）、90f4f5bef（language service）、fa85b72cb（ratchet）。検査の後の main の取り込みは L7d の merge（同じ code）
  と packet の README だけ。
- **compiler 側**：tsgo 7.1 の `GetNonAssignedNameOfDeclaration` に識別子の場合はない（6.0.3 は識別子を返した）。tsgo が
  reparse で作る宣言を表す識別子だけは名前を持つ：`@overload` tag の名前（host の名前の複製）と `@this` tag の上の識別子
  （`this` 引数の幅のない名前。`.symbols` の基準は `(Missing)`）。最初の版は `@this` の場合を欠き、`.symbols` に 5 件の退行
  （thisTag1、constructorTagWithThisTag、inferThis、thisInFunctionCallJs、thisPrototypeMethodCompoundAssignmentJs）が出た。
  `node_util` に `TryGetTextOfPropertyName`、`getContainerNode`（hover の複製もそれを使う）、構文だけの
  `IsArrayLiteralOrObjectLiteralDestructuringPattern`（checker の問い合わせはそれに委ねる）。checker に tsgo 7.1 の
  `GetNodeModulePathParts` の package の事実（`node_module_package_path`）と `TryGetModuleSpecifierFromDeclaration`。
  program に tsgo `ChangeExtension`（`change_full_extension` も使う）と `GetImpliedNodeFormatForFile`。
- **source definition**（`source_definition.rs`、tsgo `sourcedefinition.go`）：module specifier の実装の入口、実装の file の
  名前の宣言（import を通って転送、property 的なものと具体的なものを優先）、declaration map の source、package の
  declaration file に対する JavaScript。declaration file を使わない解決（`noDtsResolution`、優先の mode、ESM、CommonJS の順）
  を snapshot の file system の上で行い、program にない file を parse する。定義の位置は任意の parse の宣言を取れる（context
  node と範囲は source 基準）。`preferGoToSourceDefinition` で `textDocument/definition` も source definition を答える。
- **workspace symbol**（`symbols.rs`、tsgo `ProvideWorkspaceSymbols`）：file ごとの declaration map（overload の組は一度）、
  tsgo の一致の点数と順、先頭の 256 件。`currentProject` の範囲では文書の project、それ以外は全 project（project tree を
  読み込んで）。
- **検査（fa85b72cb）**：conformance 0 regressions（全 13,577 full、485 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（108 行 none→full。lsp 3,868 → 3,976）、source definition 91/91・workspace symbol 17/17、test（program 682、
  checker 1,803、ls 204 ほか全通過）、API client sync 340/356・async 349/365（main と同じ失敗の一覧）、clippy、fmt。
- **回帰 test**：`crates/ls/tests/references.rs`（parameter property と計算された名前の property の workspace symbol）、
  checker の package の事実、program の `ChangeExtension`。

## L7e-3 複数 project の検索（#803）

- **merge**：#803 → `370d0fbee`（hosted run 38661143906：conformance 31m27s、rust は 1 回目に 1 件落ちて再実行で 12m58s、
  gates）。commit：a10dc9f23（project）、439b46674（検索）、4985f09a8（ratchet）。検査の後の main の取り込みは L7e の
  merge（同じ code）と packet の README だけ。
- **1 回目の rust の失敗**：`crates/cli/tests/integration/cli_contract.rs` の
  `string_literal_export_names_are_allowed_in_declaration_files_like_tsgo` で、test が起動した `tsc-rs` が signal で終わった
  （終了 status が 2 でなく `None`）。2 つの file の project を CLI で compile する test で、この PR は CLI の経路に触れず、
  手元では通る。落ちた job の再実行で通した。
- **project**：tsgo `ProjectTreeRequest`（全ての tree、または指定の project を参照する tree）と `ResourceRequest.Projects`
  を `SnapshotChange` に。builder の `DidRequestProjectTrees` は、指定のある時は、それを参照しうる project
  （`hasPotentialProjectReference`）だけを更新し、tree を下る時はそれを直接または下で参照する project だけを読み込む
  （`RangeResolvedProjectReferenceInChildConfig`）。`ensureProjectTree` は program の直接の参照をたどって再帰し、
  `disableReferencedProjectLoad` の project の下には行かない。session の `getSnapshot` は tsgo の `ResourceRequest` を取り
  （開いていない configured project document は snapshot を更新する）、tsgo `GetProjectsForFile`、
  `GetLanguageServiceForProjectWithFile`、`WithSnapshotLoadingProjectTree` を持つ。
- **検索**（`crates/ls/src/cross_project.rs`、`crates/lsp/src/server/cross_project.rs`、tsgo `handleCrossProject` と
  `crossProjectOrchestrator`）：references（`_vs_references` も）、implementation、rename、incoming calls、code lens の数を、
  文書の default project と文書を持つ他の project、定義の元の位置（declaration file の source、参照先 project の source）の
  project、default project の定義が外から見える時（`getNonLocalDefinition`、emit resolver の `IsDeclarationVisible` による
  `isDefinitionVisible`）はその project を参照する tree の project（定義、その source、その generated position の位置で）
  で検索し、tsgo の順に並べ、二つ以上の project を尋ねた時は tsgo `combine*` で合わせる。単一 project の入口（tsgo の
  orchestrator のない経路）は一つの project の答えを返し、変換は結合を真似ない（implementation の位置は重複を残し、全ての
  edit が落ちた rename は空の `changes`）。
- **検査（4985f09a8）**：conformance 0 regressions（全 13,577 full、487 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（35 行 none→full。lsp 3,976 → 4,011）、対象の method を送る 731 場面のうち元の full 685 は維持、35 が新たに
  full、content mapper の 11 が残る、test（project 187 ほか全通過）、API client sync 340/356・async 349/365（main と同じ）、
  clippy、fmt。
- **道具の不具合**：手元の再生の script（`run_list.sh`）は場面名を log の file 名に使い、`/` を含む subtest の場面は実行
  されずに ok と出ていた。名前の `/` を置き換えて直した（以前の slice の判定は suites の検査が正で、影響はない）。

## L6a signature help（#804）

- **merge**：#804 → `a7592734b`（hosted run 38667955418：rust 9m16s、conformance 24m21s、gates）。commit：c9105cf4c
  （printer）、6b1d5358f（checker）、500450c32（language service と server）、f3a5a6dbd（ratchet）。検査の後の main の
  取り込みは L7e-3 の merge（同じ code）と packet の README だけ。
- **printer**：tsgo の printer は識別子の文字を `writeKind` で書く（`emitParameterName` の引数名は parameter、
  `emitPropertyName` の property 名は property：property と method の signature と宣言、accessor、property と shorthand の
  代入、binding element の property 名、enum member）。list の区切りは tsgo `writeDelimiter`（comma は punctuation、bar と
  ampersand は空白と punctuation）。変わるのは display parts の分類だけで、text writer の文字は同じ。
- **checker**：tsgo `GetResolvedSignatureForSignatureHelp`（囲む呼び出しの解決済み signature を忘れ、
  `CheckModeIsForSignatureHelp`、見かけの引数の数）と、`resolveCall` の `signatureHelpTrailingComma`（signature help の解決
  で引数が comma で終わる呼び出しは、全ての arity の検査で引数を一つ多く数える。これまで設定されず、comma の前の数で
  overload を選んでいた）。tsgo 7.1 `getArgumentArityError` の防御（数が引数の数と合う時は余分な引数の範囲を作らず呼び出し
  の誤り。末尾の comma で解決が失敗した時の panic を防ぐ）。tsgo `GetExpandedParameters` の union の展開（rest の tuple の
  union で tuple ごとの list）と tsgo 7.1 の要素の名前（全ての要素で `getTupleElementLabel`。6.0.3 の位置の引数名への
  fallback ではない。`Generator.next(value: TNext)`）。signature help 用の表示の export、`TypePredicateToString`。
- **signature help**（`crates/ls/src/signature_help.rs`、tsgo `signaturehelp.go`）：位置を囲む呼び出し（call、`new`、
  tagged template、JSX 要素、書いている途中の型引数、文脈の signature を持つ関数）、候補（checker の signature help の
  解決、generic signature、generic 型の型引数）、候補ごと・展開した引数の list ごとの item（tsgo の label、VS client には
  display parts）と文書、active な signature と引数（client の capability による）。JavaScript では checker が候補を見つけ
  ない時に呼ばれた名前の宣言を探す。tsgo `getImmediatelyContainingArgumentInfo` は一つの移植で、completions の
  `getArgumentInfoForCompletions` もそれを読む（completions の部分的な複製は消した）。JSDoc signature（`@callback`、
  `@overload`）の `@param`・`@this` tag は tsgo の reparse した引数（tag の comment を JSDoc に持つ）を表す。
- **検査（f3a5a6dbd）**：conformance 0 regressions（全 13,577 full、486 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（181 行 none→full、全て signature help。lsp 4,011 → 4,192）、signature help 181/184、test（ls 207 ほか全通過）、
  API client sync 340/356・async 349/365（main と同じ）、clippy、fmt。
- **回帰 test**：`crates/ls/tests/references.rs`（末尾の comma の引数が overload を選ぶ、入れ子の呼び出しの末尾の comma、
  JavaScript の名前の宣言への fallback と rest の tuple の union の binding 名）。
- **有界の違い**：content mapper の 3 場面（L10）。

## L6b document symbol（#805）

- **merge**：#805 → `5ece24096`（hosted run 38671480253：rust 12m28s、conformance 32m5s、gates）。commit：1488bab3f
  （binder）、4ec776e1c（language service と server）、3e23eb56c（ratchet）。検査の後の main の取り込みは L6a の merge
  （同じ code）と packet の README だけ。
- **binder**：tsgo 7.1 `GetAssignedName`。element access に代入された関数・class 式は access の literal の引数だけで名前が
  付く（6.0.3 は他の引数でも access 自体で名前を付けた。`f[Symbol.iterator]` の関数は tsgo では `<function>`）。
- **document symbol**（`crates/ls/src/symbols.rs`、tsgo `symbols.go`）：文書の宣言（本体や初期化子の symbol を子に
  持つ）、JSDoc の typedef と callback、最上位以外の import、constructor の引数 property、点で区切った namespace を一つの
  symbol、関数と class の expando（`A.b = …`、`A.prototype.b = …`、`Object.defineProperty(A, "b", …)`）は対象に合わせ、
  同名の namespace も合わせる。tsgo の名前（escape した引用文字列、callback の `f("literal") callback`、`<function>`、
  `<class>`、`export=`、`default`、`constructor`、`()`、`new()`、`[]`）を 150 文字で切り、範囲は名前を選ぶ。名前・種類・
  範囲ごとに一つ。階層を持たない client には symbol information に平らにする。
- **検査（3e23eb56c）**：conformance 0 regressions（全 13,577 full、500 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（85 行 none→full、全て document symbol。lsp 4,192 → 4,277）、document symbol 85/86、call hierarchy と
  workspace symbol（`GetAssignedName` を読む）56/56、test（ls 208 ほか全通過）、API client sync 340/356・async 349/365
  （main と同じ）、clippy、fmt。
- **回帰 test**：`crates/ls/tests/references.rs`（computed key の expando は名前のない関数を持つ）。
- **有界の違い**：TestContentMapperSupplementalDocumentSymbols（L10）。

## L6c folding range と selection range（#806）

- **merge**：#806 → `a20f978b9`（hosted run 38675002932：rust 8m20s、conformance 22m7s、gates）。commit：07bd0dfa6
  （language service と server）、8c45e10ce（ratchet）。検査の後の main の取り込みは L6b の merge（同じ code）と packet の
  README だけ。
- **folding range**（`crates/ls/src/folding.rs`、tsgo `folding.go`）：深さ 40 までの node の範囲（block。関数は引数が
  複数行なら開き括弧から。class・interface・enum・型 literal の本体、case 節、object と array の literal と binding
  pattern、tuple 型、JSX 要素（`<tag>...</tag>`）と属性、template、arrow 関数の本体、複数行の呼び出しと括弧の式、import
  と export の list）、二つ以上の import の連なり（最初の `import` から）、comment（複数行のもの、二つ以上続く一行の
  もの。region の区切りは混ぜない）と `#region`（comment の中でないもの、名前つき）。行単位で畳む client には閉じ括弧で
  終わる範囲を一行手前で終える。並べて重複を除く。
- **JavaScript の reparse**：tsgo の parser は文の JSDoc から reparse した宣言（型を持つ `@typedef`・`@callback`、import
  節を持つ `@import`）を文の前に置き、`IsAnyImportSyntax` はそれを取らない。それで import の前の宣言は import の連なりを
  切る。次の連なりは tsgo `FindChildOfKind` の古い `import` token の位置（JSDoc comment の終わり）から始まる
  （`child_of_kind` は既にこれを真似ている）。
- **selection range**（`crates/ls/src/selection_ranges.rs`、tsgo `selectionranges.go`）：各位置で file から最も内側の
  node まで。list、文字列の内側、`${ … }`、複数行の関数本体、後ろの一行 comment（斜線ありとなし）、JSDoc を先に。mapped
  型は tsgo の selection の子（`-readonly`・`-?` をまとめ、型引数を括弧とまとめ、colon で分ける）。範囲は 999 まで、
  内側を残す。
- **共有**：`getLineEndOfPosition` と位置の行は一つの helper（completions の二つの複製をまとめた）。
- **検査（8c45e10ce）**：language service と server だけの変更（compiler の経路なし）なので compiler の conformance と
  並列の対照は走らせていない。suites 0 regressions（73 行 none→full、全て folding と selection range。lsp 4,277 →
  4,350）、folding と selection range 73/76、test（language service を使う crate：ls 209、lsp 14、api 297、conformance
  49、全通過）、API client sync 340/356・async 349/365（main と同じ）、clippy、fmt。
- **回帰 test**：`crates/ls/tests/references.rs`（reparse した JSDoc の宣言は import の連なりを切る。期待値は
  `scripts/lsp_transcript.py` で取った tsgo の答え）。
- **有界の違い**：content mapper の 3 場面（L10）。
- **一時的な誤判定**：suites の更新中に見えた panic（`completions/jsdoc.rs:565` の 9 件と mutex の poison 12 件）は L6b の
  実行にもある既存のもの（content mapper の場面、L10）で、L6c の回帰ではない。

## L6d semantic token・linked editing・auto insert（#807）

- **merge**：#807 → `d727a8257`（hosted run 38683880598：rust 10m44s、conformance 18m33s、gates）。commit：371e1187d
  （syntax）、ba080f075（language service と server）、ec6215374（ratchet）、09b39d9c3（modifier の共有）。最初の head
  `09b39d9c3` の run 38680567352 も通ったが、その間に README を main に入れたため strict な main の更新（`gh pr
  update-branch`）で CI をもう一度回した（文書は PR の gates 待ちの間に main へ入れない）。
- **syntax**：tsgo `ast.TagNamesAreEquivalent` を `tsc_syntax::utilities` の共有の関数にし、parser もそれを呼ぶ（動作は
  同じ）。
- **semantic token**（`crates/ls/src/semantic_tokens.rs`、tsgo `semantictokens.go`）：file か範囲に重なる node の識別子
  （JSX 要素の tag と属性、import 節、`Infinity`・`NaN` を除く）のうち、（alias を解いた）symbol を flag か宣言の種類で
  分類できるもの。点の右の引数は property、型（か union の要素）が construct できる変数・property・引数は class、呼べる
  もの（property を持たないか呼び出しの callee）は function か method。declaration・static・async・readonly・local・
  defaultLibrary の modifier。開始で並べ、開始ごとに一つ、client の legend で符号化。legend の表は tsgo と同じく
  language service に移した。
- **modifier**：`node_util::get_combined_modifier_flags` は JavaScript の member の JSDoc（`@readonly` など）から tsgo の
  parser が reparse する modifier を hosted の reparse 表から既に数える（class の property・method・accessor、object
  literal の中では無し、代入、変数は無し）。最初に同じ規則を別に書いてしまい、`09b39d9c3` で共有の helper に替えた。
- **linked editing と auto insert**（`crates/ls/src/jsx_tags.rs`）：fragment の `<`・`</` の後の位置か、要素の二つの tag 名
  （どちらかの中、両方が整い同じ文字のとき）と tsgo の word pattern。`>` で閉じていない要素（fragment）の開き tag を
  終えた後に閉じ tag の snippet（`$` は escape、利用者が auto closing tag を切っていなければ）。
- **検査（09b39d9c3）**：conformance 0 regressions（全 13,577 full）、suites 0 regressions（61 行 none→full、semantic
  token 46・linked editing 12・auto insert 3。lsp 4,350 → 4,411）、61/61、test（ls 211 ほか全通過）、API client sync
  340/356・async 349/365（main と同じ）、clippy、fmt。
- **回帰 test**：`crates/ls/tests/references.rs`（reparse した JSDoc の modifier の readonly、範囲の token と既定の
  library。期待値は `scripts/lsp_transcript.py` で取った tsgo の答え）。
- **README**：language server の節を更新（実装済みに semantic token と linked editing、一致数 4,411）。

## L6e inlay hint（#808）

- **merge**：#808 → `1bb41ce0e`（hosted run 38685712417：rust 9m13s、conformance 30m49s、gates）。commit：4245e7ab3
  （checker）、8e26c0d29（language service と server）、47243797d（ratchet）。検査の後の main の取り込みは L6d の
  merge（同じ code）、packet の README と README だけ。
- **checker**：`INLAY_HINT_NODE_BUILDER_FLAGS`（tsgo `typeToInlayHintParts` の node builder の flag）と
  `has_context_sensitive_parameters` の公開（動作は同じ）。
- **inlay hint**（`crates/ls/src/inlay_hints.rs`、tsgo `inlay_hints.go`）：範囲の中、型の外で、利用者の設定により、型注釈の
  ない変数・property（binding pattern、module、literal・`new`・object literal・assertion の初期化子を持つ `const`、名前と
  同じ綴りの型を除く）、文脈に依る関数の引数、関数の返り値（か型述語）の推論された型、enum member の値、各引数の引数名
  （spread の tuple の必須の要素を数え、ラベル付きの rest の tuple の要素名。literal だけか全て）。ただし引数か comment が
  既に名前を書いていれば出さない。型の label の部分は node builder が識別子の symbol と共に作り
  （`type_to_type_node_in_arena`、tsgo の `idToSymbol`）、宣言された symbol の識別子は宣言の名前の位置を持つ（引数名の
  hint も）。tsgo の区切りのまま（型引数の間は `,`、`keyof` の後に空白なし）。
- **JavaScript**：tsgo の `Node.Type()` は `node_util::type_node_of`（JSDoc の `@type`・`@param`・`@returns` の型を持つ
  宣言には hint なし）。tsgo `IsVarConst`（`const` だけ、`using`・`await using` は違う）。
- **検査（47243797d）**：conformance 0 regressions（全 13,577 full、489 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（64 行 none→full、全て inlay hint。lsp 4,411 → 4,475）、64/65、test（ls 213 ほか全通過）、API client sync
  340/356・async 349/365（main と同じ）、clippy、fmt。
- **回帰 test**：`crates/ls/tests/references.rs`（JSDoc の型は型の hint を出さない、変数・ラベル付き rest の tuple と rest
  の引数（位置つき）・enum member の hint。期待値は tsgo の答え）。
- **有界の違い**：TestContentMapperInlayHintsReleaseEachRange（L10）。
- **README**：language server の節（実装済みに inlay hint、一致数 4,475）。

## L4f completion の snippet（#809）

- **merge**：#809 → `a66b39bfc`（hosted run 38692085292：rust 11m26s、conformance 31m19s、gates）。commit：739b56597
  （checker）、88ac5bc3e（emitter）、80dde74a8（language service）、23c5e1413（ratchet）。
- **checker**：override の検査が tsgo の `MemberOverrideStatus`（override が要る、不正な override、なし）を返し、型の表示は
  報告する時だけ作る（tsgo の順。tsc は基底 class の表示を先に作っていた）。`get_member_override_modifier_status`（tsgo の
  service 向けの入口）。`ast.CanHaveDecorators` は `node_util` へ。
- **emitter（tsgo の snippet printer）**：tab stop（`SnippetElement::TabStop`、空の文に `$N`）、text writer の snippet の
  escape（`$` を含む write を、整形の後に適用する edit として記録。`StandaloneWriter::SnippetChangeTracker`、
  `PositionedPrint.snippet`）。template literal の token は区切りを含めて一つの string literal の write（tsgo `emitLiteral`。
  出力の文字列は同じ、display part は一つ、snippet は `${` の `$` を escape）。change tracker の arena は deep clone で全ての
  node を複製する（位置を割り当てた node が木に二度現れると整形が壊れる。共有された `?` token が逆向きの edit を作った）。
  宣言の emit は位置のない部分木の共有を保つ（fdebe320e の memory の節約）。
- **language service**（`crates/ls/src/completions/snippets.rs`、tsgo `getEntryForMemberCompletion`・
  `getEntryForObjectLiteralMethodCompletion`・`getExhaustiveCaseSnippets`）：class member の宣言全体（missing member の
  fixer の member に空か tab stop の本体、書かれた modifier との合成、必要な `override` の追加、import と書かれた modifier の
  削除は追加の edit）、object literal の method（呼び出し signature が一つの property・method に、型のない引数の method、
  label の detail は引数の並び、property の下に並べる）、switch の残りの case（literal の union に、clause のない member の
  case を一つの snippet item に。enum member は auto-import できる型 node を式に）。共有：change tracker の snippet の
  print、`TypeToAutoImportableTypeNode`、completion の source の定数、`IsDynamicFileName`、`isRightOfDotOrQuestionDot`。
- **検査（23c5e1413）**：conformance 0 regressions（全 13,577 full、484 s。template literal の一回の write、override の
  遅延表示、tracker の共有しない複製は compiler の出力を変えない）、`--checkers 4` は既知の 4 だけ、suites 0 regressions
  （59 行 none→full：class member・object literal method・switch case の 54 場面と、completion が class member の宣言である
  auto-import の 5 場面。lsp 4,475 → 4,534）、test（全 crate 通過）、API client sync 340/356・async 349/365（main と同じ）、
  clippy、fmt。
- **有界の違い**：これらの場面にはなし。残る completion は auto-import（L5d）と content mapper（L10）。
- **README**：language server の節（completion に snippet、一致数 4,534）。

## L5d auto-import の module specifier（#810）

- **merge**：#810 → `cd2ce72ec`（hosted run 38695913803：rust 12m45s、conformance 31m27s、gates）。commit：c34ab9bc6
  （checker）、f1316c535（language service）、fee175eda（ratchet）。検査の後の main の取り込みは L4f の merge（同じ code）、
  packet の README と README だけ。
- **checker（tsgo の module specifier）**：`getPathsRelativeToRootDirs` は別の volume の root を空の相対 path として残す
  （tsc は捨てた。`rootDirs: ["D:/"]` の project は tsgo の `./` になる）、`isPathRelativeToParent` は
  `RelativePath.IsParentRelative`（`..` かその下）、最短の候補は区切りが最も少ないものの最初（tsc は最後）。
  `tryGetModuleNameFromPackageJsonImports` は nodenext と bundler で `#/` の key（`#/` だけを除く）を読む（tsc は全ての `#/` を
  飛ばした。解決は既に読んでいた）。base URL の選択は tsgo の `..` の前置の判定。
- **language service**：解決しない module augmentation の名前は `./`・`../` の時だけ取り込む file からの相対に直し、
  root からの名前（`/types/rooted.css`）はそのまま（tsgo `ModuleSpecifier.IsRelative`）。
- **検査（fee175eda）**：conformance 0 regressions（全 13,577 full、484 s）、`--checkers 4` は既知の 4 だけ、suites 0
  regressions（3 行 none→full：TestAutoImportCssModule、TestAutoImportPackageJsonImportsHashSlashNodenext、
  TestAutoImportsWithRootDirsAndRootedPath01。lsp 4,534 → 4,537）、test（全 crate 通過）、API client sync 340/356・async
  349/365（main と同じ）、clippy、fmt。
- **有界の違い**：TestCompletionForStringLiteralNonrelativeImport2（tsgo 側が非決定、L4c-2）。残る lsp の行は content
  mapper（L10）の 46。
- **README**：language server の一致数 4,537。

## L9 API の LS（#811）

- **merge**：#811 → `03edfd0b4`（hosted run 38700647220：rust 13m10s、conformance 30m59s、gates）。commit：d7ee32a1f
  （language service）、898ac86b3（API）、b22baa2b7（test）。検査の後の main の取り込みは L5d の merge（同じ code）、
  packet の README と README だけ。
- **language service**：API が呼ぶ tsgo の入口。`GetCompletionsAtPosition`（API が求める時は symbol の item に symbol。内部
  の list の `CompletionItem.Symbol` を `getCompletionEntriesFromSymbols` が付ける。string の property 名も）、
  `GetReferencedSymbolsForNode`（definition の node・symbol と node の参照）、`GetSignatureUsages`（宣言の名前以外の参照と、
  property access を越えて callee である呼び出し）、`Checker.GetReferencesToSymbolInFile`（tsgo は checker/services.go。
  tsc-rs の木の走査は language service にあるのでそこに置く）、`GetSymbolJSDocTags`（`declarationJSDocTags` と
  `GetNextJSDocCommentLocation`、`getJSDocTagText`）と `GetSymbolDocumentationComment`、`importSymbol` の import adder
  の edit。`with_api_checker`（tsgo `CheckerLifetimeAPI`。API の request は program の API の checker で走る。tsc-rs の
  handle は checker ごとの symbol を名指すため。tsgo は global な symbol id でどの checker の symbol も通す）。
- **API**：`getCompletionsAtPosition`（UTF-16 の位置。snapshot の auto-import が準備されていなければ準備した snapshot で、
  symbol を求める時は error）、`getReferencedSymbolsForNode`、`getSignatureUsages`、`getReferencesToSymbolInFile`、
  `getJsDocTags`、`getDocumentationComment`、`getImportAdderEdits`（`toAPITextEdits`：元の text の UTF-16 の offset）。
  `createSnapshot`・`updateSnapshot` の `prepareAutoImports`（`validatePreparedAutoImports`）。空の結果の list は `[]`
  （tsgo の JSON v2 は nil の slice をそう書く）。
- **oracle**：tsgo 自身の client の上の probe（62 request：client の test の場面と端の場面）で tsgo と tsc-rs の応答が一致
  （symbol と parse の id は各実装のもの、global の completion は集合で比べる。tsgo は Go の map の順に並べる）。
- **検査（898ac86b3）**：compiler の経路の変更なし（conformance は走らせない）、suites 0 regressions（行の変化なし。
  lsp 4,537）、test（api 305 ほか全通過）、API client **sync 356/356・async 365/365**（main は 340/349）、clippy、fmt。
- **回帰 test**：`crates/api/tests/unit/session_language_service.rs`（8 本。期待値は tsgo の応答）。
- **有界の違い**：symbol の handle の id と source file の node id は各実装のもの。`printNode` の `preserveSourceNewlines`
  （P5-5b）は別の slice（printer の list の改行の port と、それを使う合成 file）。
- **README**：API server の節（language service の request、client の test が全て通る）。

## L10 content mapper の LS の計画（2026-10-15）

残る lsp の none 47 のうち 46 が content mapper の場面（`TestContentMapper*`）。tsgo の作りは次の三つで、どの機能にも掛かる。

- **server**：`ServerOptions.Spawn`（mapper の process の起動。tsgo の fourslash は `contentmappertest` の in-process の
  mapper を渡す）。fourslash の client は動的登録を持たないので `RegisterContentMapperExtensions` は何もしない。
- **変換**（`lsconv`）：`Script` が `OriginalFileName`・`SpanMap`・`OriginalText` を持ち、`ToLSPPosition`・`ToLSPRange` は
  virtual の座標を元の file に自動で戻す（`ForFeature` の形は segment の feature で fidelity を落とす）。入力の
  `FromLSPPositionForSourceFile`（と range、交わる range）は canonical と supplemental の出力の全ての projection を返す。
- **各機能**：projection ごとに処理し（多くは single segment だけ）、結果を合わせる（重複を除く・内容を連ねる）。
  出力は機能の feature で fidelity を見て、参加しない segment の位置を落とす。

tsc-rs の span map は compiler の UTF-16 なので、LS には UTF-8（tsgo の単位）の写しを parse した source に持たせる。

### 小 slice

1. **L10a 基盤・hover・diagnostics**：spawner の配線、test mapper（component・duplicate・duplicate-projection・
   prefixed-supplemental・unmapped-folding・hoisting）、source の content mapper の情報、変換、hover（projection の
   結合）、diagnostics（supplemental の出力、mapper の診断の元の範囲、合成部分の 18080 への集約、提案の不要 code の除外）。
   この時点で 46 のうち 14 が一致する（出力の自動の写像だけで通る 8 を含む）。
2. **L10b 移動**：definition・type definition・implementation・references（`_vs_references`、無効な移動先）・
   document highlight・rename（outgoing を含む）・signature help・重複の写像。
3. **L10c completion**：completion と resolve、content mapped な file の auto-import（`filterContentMappedAutoImports`
   の先の解決と元の座標の edit）。
4. **L10d 残り**：format（文書と範囲）、folding、selection range、document symbol、code lens、inlay hint、file rename、
   code action。

## L10a content mapper の LS の基盤・hover・diagnostics（#812）

- **merge**：#812 → `b120cf5e6`（hosted run 38710133417：rust 13m01s、conformance 32m10s、gates）。commit：731355668
  （spanmap・syntax・checker）、179e43d10（harness の test mapper）、55758414a（lsp の spawner）、e54df0552（変換・hover・
  diagnostics）、dcad04d9e（folding・symbol・format・rename・auto-import の対象）、5800895f2（ratchet）。検査の後の main の
  取り込みは L9 の merge（同じ code）、packet の README と README だけ。
- **spanmap・syntax・checker**：tsgo `VirtualToOriginalSpanForFeature`・`VirtualToOriginalPositionForFeature`
  （`virtualSpanSupportsFeature`）と UTF-8 の map（compiler の UTF-16 の map の写し。LS は tsgo の単位の byte）。parse した
  source に `ContentMapperOutput`（mapper の識別・元の text・canonical と supplemental の出力の名前・UTF-8 の span map）。
  提案の行にも `filterAndSortDiagnostics`（合成部分だけの不要 code の提案を出さない）。
- **server**：tsgo `ServerOptions.Spawn`。`tsc-rs --lsp` は mapper を native に起動し、transcript の replay は harness の
  in-process の test mapper（component・duplicate・duplicate-projection・prefixed-supplemental・unmapped-folding・hoisting。
  tsgo の fourslash と同じ）。
- **変換**：tsgo `Script` の `OriginalFileName`・`SpanMap`・`OriginalText`。`ToLSPPosition`・`ToLSPRange` は出力の virtual
  の座標を元の file に戻す（`ForFeature` と fidelity）。`FromLSPPosition`・`FromLSPRange`・交わる range は file と
  supplemental の出力の、feature に参加する segment の projection を全て返す（`sourceFileProjections`）。
- **機能**：hover は single segment の projection を全て合わせる（内容を区切りで連ね、共通の range、VS の積み重ね）。
  diagnostics は supplemental の出力も含め、mapper 自身の診断は元の範囲、合成部分の診断は file の先頭、出力の合成部分の
  診断は 18080 一つに集約（関連情報に各診断）。folding・document symbol・workspace symbol は tsgo の feature の filter。
  mapped な format（`getFormattingEditsForMappedRange`・`nonOverlappingFormattingRanges`）と範囲・on-type の format は exact
  な projection 一つ。`GetRenameInfo` は最初の exact な projection、`getRenameInfoSuccess`・`renameEditRange` は exact を
  求める。supplemental の出力は auto-import の対象にしない（`IsContentMapperSupplemental`）。
- **教訓**：mapper を起動すると、mapper が無くて空の答えで偶然一致していた場面が崩れる（5 件。folding・symbol・format・
  rename・auto-import を port して戻した）。replay は一致済みの content mapper の場面も一緒に回す。
- **検査（e54df0552 の conformance、dcad04d9e の suites と test）**：conformance 0 regressions（13,577 全て full、498 s）、
  `--checkers 4` は既知の 4 件だけ、suites 0 regressions（lsp 24 行 none → full、**4,537 → 4,561**）、test（diagnostics 84・
  harness 31 を含め全通過）、API client sync 356/356・async 365/365、clippy、fmt。
- **README**：Status の language server の数（4,561）と、language server の節（content mapper の file は一部）。

## L10b content mapper の LS の移動（#813）

- **merge**：#813 → `2fe83df15`（hosted run 38714441602：rust 13m40s、conformance 24m15s、gates）。commit：96051b97f
  （language service）、89cb1336e（ratchet）。検査の後の main の取り込みは L10a の merge（同じ code）、packet の README と
  README だけ。
- **移動**：`ProvideDefinition`・`ProvideTypeDefinition`・`ProvideSourceDefinition` は single segment の projection を全て
  合わせる。`createDefinitionLocations` は名前が feature の一つの segment に写らない移動先を落とし、文脈が写らなければ
  名前の範囲（`sourceFileRangeToLSPLocationForFeature`）。switch の定義は写らなければ file のもの。
- **references・implementation・VS references**：feature の一つの segment に写らない entry と定義を落とす
  （`getLocationOfEntryForFeature`）。link の文脈は写る時だけ。supplemental の出力の entry は元の file に。
- **document highlight**：single segment の projection を全て合わせ、元の file ごとにまとめる。highlight は写る時だけ。
- **signature help**：help のある最初の single segment の projection。
- **`{@link}`**：tsgo `documentationLocationMapper`。宣言が request の feature（hover・signature help）の一つの segment に
  写る時だけ markdown の link、他は text。API の documentation はどこにも写さない（`noMappedLocation`）。
- **検査（89cb1336e）**：compiler の経路の変更なし（conformance は走らせない）、suites 0 regressions（lsp 8 行 none → full、
  **4,561 → 4,569**）、test 全通過、API client sync 356/356・async 365/365、clippy、fmt。
- **README**：Status の language server の数（4,569）と、content mapper の file で使える機能。

## L10c content mapper の LS の completion・edit・残りの機能（#814）

- **merge**：#814 → `74c67efdf`（hosted run 38718568469：rust 11m11s、conformance 24m57s、gates）。commit：e948ab86a
  （language service）、27a793974（ratchet）。検査の後の main の取り込みは L10b の merge（同じ code）、packet の README と
  README だけ。計画の L10d（code lens・selection range）もここで行った。
- **change tracker**：tsgo `getTextChangesFromChanges`・`computeNewText`・`toLSPEditRange`・`fromLSPEditRange`。
  `GetChanges` は元の file ごとの edit と、元の text に書き戻せない file（verbatim の span の外の edit、projection の間で
  食い違う edit）を返し、後者の edit は落とす。node の text は位置の exact な projection 全てで format し、再 indent は
  元の text を読む。複数の projection の edit は重複を除いて衝突を調べる。`InsertText`・`ReplaceRangeWithText` は元の
  位置。`InsertAtTopOfFile` は mapper が合成した header を飛ばす。
- **auto-import**：`Fix.Edits` は書き戻せたかを返し（`fileEdits` の `ok`）、書き戻せない import の fix は出さない
  （implement interface は edit なし）。content mapped な node_modules の file は project bucket に入る。supplemental の
  出力は canonical の出力として import する（`getPreparedAutoImportView`・`NewView`）。
- **completion**：位置の最初の projection（exact でなければ無し）、item の data は元の file と supplemental の index
  （`supplementalFileIndex`、resolve は `sourceFileForSupplementalFileIndex` で戻す）、`filterContentMappedAutoImports`。
- **残りの機能**：code lens は supplemental の出力も（data の index、解決もそこで）。selection range は single segment の
  唯一の projection で、mapper の出力には file の範囲を付けない。call hierarchy・linked editing・auto insert・semantic
  token・inlay hint・code action にも tsgo の projection と feature の filter。LSP の位置を取る機能は全て projection を
  通り、language service の素の LSP 位置の入口（`with_request`）は無くなった。
- **回帰 test**：tsgo `TestTextEditsConflictAtSameInsertionPointAcrossProjections`（`change::tests`）。
- **検査（27a793974）**：compiler の経路の変更なし（conformance は走らせない）、suites 0 regressions（lsp 14 行 none →
  full、**4,569 → 4,583 / 4,584**。残る 1 は tsgo の答えが非決定的な `TestCompletionForStringLiteralNonrelativeImport2`）、
  content mapper の 56 場面全て一致、test（ls 214 ほか全通過）、API client sync 356/356・async 365/365、clippy、fmt。
- **次（L10d）**：project system と server の content mapper。tsgo の `project/contentmapper_test.go` の LSP の session の
  場面と `lsp/server_contentmapper_test.go`：editor の contribution（`custom/setContentMapperContributions`、inferred
  project の mapper）、mapper の拡張子の動的登録（`RegisterContentMapperExtensions`）、mapper の locale、作られた
  mapped file の configured project への採用（`PossiblyMatchesFileName`）。typings の install の場面は ATA と共に L11。
- **README**：Status の language server の数（4,583）と、content mapper の file の機能。

## L10d project system と server の content mapper（#815）

- **merge**：#815 → `8a3b0a88a`（hosted run 38726499368：rust 13m07s、conformance 30m50s、gates）。commit：adf541bee
  （compiler・program）、9d7fc175d（project）、757bedebe（lsp）。検査の後の main の取り込みは L10c の merge（同じ code）、
  packet の README と README だけ。
- **locale**：tsgo `locale.Locale` は tag を持つ（`Locale.String()`、既定は空）。tsc-rs の `Locale` も翻訳と tag を持つ。
  mapper の host の `SetLocale`（新しい locale で初期化し直し、動いている process を閉じる）、project system の content
  mapped な cache の key に locale（`contentMappedParseCacheKey`）、CLI の host は `--locale` の tag。session の host は
  client の locale で始まり、`Configure` の locale の変更で切り替わる。
- **作られた mapped file**：tsgo aa8149273 の `PossiblyMatchesFileName`（literal な include の判定、wildcard の directory の
  下の content mapped な file、mapper の拡張子を含む wildcard の glob。glob は書かれた通りに照合）。
- **contribution**：`custom/setContentMapperContributions`（`parseContentMapperContributions` の検査）、session の
  `SetContentMapperContributions`（inferred project だけの mapper、snapshot が持つ、その拡張子は inferred project で
  対応、開いている文書の configured project を探す）。
- **登録**：`updateContentMapperRegistrations`（読み込んだ全ての config の mapper と contribution の拡張子）と server の
  `RegisterContentMapperExtensions`（client が動的に登録する機能だけ、同期・pull diagnostics・各機能の 26 の id）。
- **その他**：`contentMapperFallbackResponse`（project の無い未知の種類の文書に空の答え、`ErrNoProjectForUnknownScriptKind`）、
  `contentMapperLogger`（trace の時だけ mapper の行）。
- **test**：tsgo の LSP の session の場面の移植 `crates/project/tests/content_mappers.rs`（10）、server の
  `content_mapper_contributions_before_did_open`（tsgo `TestSetContentMapperContributionsBeforeDidOpen`）、parser の 4、
  program の locale の cache、compiler の tag。
- **検査（757bedebe）**：conformance 0 regressions（13,577 全て full、485 s）、`--checkers 4` は既知の 4 件だけ、suites 0
  regressions（行の変化なし、lsp 4,583）、test 全通過（lsp 19・project 197・compiler 227 ほか）、API client sync 356/356・
  async 365/365、clippy、fmt。
- **有界の違い**：`TestContentMapperInferredProjectSurvivesTypingsInstall` は ATA と共に L11。tsgo の
  `TestContentMapperLocaleChange` の locale の読み取り回数（tsgo の context の配管）は移さず、mapper の生死を確かめる。
- **README**：language server の節（editor の contribution を含め content mapper の file の機能が揃った）。

## L11 残りの server の機能の計画（2026-10-15）

tsgo の server と session のうち transcript が届かない機能（fourslash の client はこれらを使わない）。tsgo 自身の lsp と
project の test を移して固定する。

1. **L11a progress・projectInfo・telemetry**：`projectLoadingProgress`（`window/workDoneProgress/create` と `$/progress`
   の begin と end、Client の `ProgressStart`・`ProgressFinish`）、`custom/projectInfo`、telemetry（`telemetry/event`：
   要求の失敗、性能の統計、project の情報。Client の `SendTelemetry`・`IsActive`）。tsgo の `server_progress_test`・
   `server_projectinfo_test`。
2. **L11b API の session**：`custom/initializeAPISession`（LSP の session を共有する API の session を pipe で開く）。
3. **L11c ATA**：typings の installer（`discoverTypings`、npm の install、typesMap、package 名の検査、typings の cache）と
   それに乗る session の流れ。tsgo の `project/ata` の test と content mapper の typings の場面
   （`TestContentMapperInferredProjectSurvivesTypingsInstall`）。
4. **L11d 内蔵の file 監視**：client が監視の動的登録を持たない時の tsgo `lspwatcher` と vendored の `fswatch`
   （FSEvents・inotify・fanotify・kqueue・Windows）。
5. **profile の `custom/*`**（runGC・save*Profile・start/stopCPUProfile）：Go の runtime の機能なので、tsc-rs での扱いは
   利用者に確かめる。

## L11a 進捗表示・project 情報・telemetry（#816）

- **merge**：#816 → `afc758856`（hosted run 38732142217：rust 10m44s、conformance 31m35s、gates）。commit：320251fd7
  （lsp・project）。検査の後の main の取り込みは L10d の merge（同じ code）、packet の README と README だけ。
- **進捗**：tsgo `projectLoadingProgress`（`lsp/progress.go`）。project の読み込みが遅延（`tsc-rs --lsp` で 250 ms、
  `cmd/tsc/lsp.go` と同じ）を超えた時だけ `window/workDoneProgress/create` と `$/progress`（begin・report・end）、実行中の
  操作を report、最後の操作で end。work done progress を受ける client だけ。tsgo の goroutine と timer は state machine
  の上の thread。build が報告する（`updateProgram` の `Project '{0}'`、project の `DisplayName`）。
- **`custom/projectInfo`**：文書の既定の project の config（inferred は空）。
- **telemetry**：tsgo `sendProjectInfoTelemetryForNewProjects`／`collectProjectInfoTelemetry`。`enableTelemetry` の時、
  新しい project ごとに一度、種類・config・compiler options（boolean はそのまま、列挙は名前）・file の数と大きさ。
- **test**：tsgo `TestProgress` の各場面（`synctest` の偽の時刻は state の明示の段階に）、
  `TestProgressNotificationsEndToEnd`、`TestInitializeCodeActionKinds`、`TestProjectInfoConfiguredProject`・
  `TestProjectInfoInferredProject`、project 情報の 2 件（tsgo に test なし、`collectProjectInfoTelemetry` から固定）。
- **検査（320251fd7）**：compiler の経路の変更なし（conformance は回さず）、suites 0 regressions（行の変化なし、lsp
  4,583）、test 全通過（lsp 33・project 199・api 305 ほか）、API client sync 356/356・async 365/365、clippy、fmt。
- **有界の違い**：Go runtime 固有の部分（5 分ごとの性能 telemetry、要求失敗の telemetry の Go の stack、`IsActive`、
  profile の command `custom/runGC`・`save*Profile`・`start/stopCPUProfile`）は利用者の判断待ち（2026-10-15 に
  pprof の移植可能性を説明：CPU profile は pprof-rs で現実的、heap・alloc は allocator の作り替えが要る）。

## L11b 言語サーバーの上の API session（#817）

- **merge**：#817 → `24291b006`（hosted run 38739655322：rust 12m02s、conformance 32m07s、gates）。commit：1d4946e83
  （api・project）、3ef8485c0（lsp）。検査の後の main の取り込みは L11a の merge（同じ code）と packet の README だけ。
- **API session**：tsgo `NewLSPSession`（project session の snapshot host を共有、要求は editor の locale で、要求の
  file system は editor の開いた文書の上に）、`getCurrentLanguageServerSnapshot`（`APIUpdate`：保留の変更を流し、失敗した
  要求は採らないが流した変更は進める。session が開いた project・file は閉じるか終わるまで開いたまま、作った program だけ
  を消せる・設定し直せる）、auto-import の準備の snapshot を背景で採る（`TryAdoptSnapshotInBackground`）。
- **見つけた既存の不具合**（tsgo の test の移植で）：(1) 要求の file system が開いた文書の下にあった → tsgo
  `layerOverlayFileSystem` の通り文書の上に載せ直し、その file・削除・link が隠す文書は開いた file から外す
  （`RebasableFileSystem`）。(2) auto-import の clone が要求の file system を失っていた（`CloneSnapshotWithAutoImports`）。
  (3) resolver の callback の失敗が API 要求を失敗させていなかった（`HandleAPIRequest` の最後の検査）。
- **server**：`custom/initializeAPISession`（pipe の最初の client に JSON-RPC で、pipe は接続後に消える、shutdown と
  server の終わりで閉じる）。`tsc-rs --api` の pipe を `PipeTransport` に切り出し（別 thread からの close が待つ accept
  を終わらせる、Go の listener と同じ）。
- **test**：tsgo `session_apistate_test.go` の言語サーバーの 13 件、`session_requestfilesystem_test.go` の editor の
  文書を伴う 10 件（大きな layer の場面を editor 付きに）、`session_completion_test.go` の 2 件、
  `session_module_resolution_test.go` の 1 件、server の pipe 越しの端から端まで。
- **検査（3ef8485c0）**：conformance 0 regressions（13,577 全て full、504 s）、`--checkers 4` は既知の 4 件だけ、suites 0
  regressions（行の変化なし）、test 全通過（lsp 34・api 330・project 199 ほか）、API client sync 356/356・async 365/365、
  clippy、fmt。
- **有界の違い**：tsc-rs の非同期 API 接続は要求を一つずつ扱い、client への呼び出しの答えは呼び出し側で読む（別 thread の
  言語サーバーの更新からの resolver の callback は今の要求では起きない）。Windows の名前付き pipe は未移植。
- **L11 の残りの順**（利用者の判断 2026-10-15 を受けて）：L11c CPU profile（pprof-rs、`--pprofDir` の CPU 分も）・
  heap と allocation は未対応のエラー・`runGC` は mimalloc の `mi_collect`・要求失敗の telemetry（Rust の panic の
  stack を tsgo の規則で消毒）・flaky diagnostics の追跡 → L11d ATA（d-1 `project/ata` package、d-2 project・session・
  server への統合）→ L11e 組み込みの file 監視。
- **README**：API server の節（言語サーバーの上の API）。

## L11c CPU profile・要求失敗の telemetry・flaky diagnostics（#818）

- **merge**：#818 → `9c99ba484`（hosted run 38746347290：conformance 32m18s、rust は 1 回目に既知の終了の競合
  （`pushed_config_diagnostics_match_tsgo` の `EXIT 0` と `EXIT 1`、L7d と同じ）で落ち、失敗した job の再実行で
  11m06s で通過、gates）。commit：9ae79a0cd（compiler・lsp）。検査の後の main の取り込みは L11b の merge（同じ code）と
  packet の README と README だけ。
- **CPU profile**：tsgo `internal/pprof` を pprof-rs で（利用者の判断 2026-10-15）。Unix の `SIGPROF` で 1 秒 100 回の
  採取、Go の形式（gzip した `perftools.profiles.Profile`、`samples/count` と `cpu/nanoseconds`）の符号化はここで書き、
  gzip は miniz_oxide。一度に一つ（Go と同じ）。`--pprofDir` は compile・build（コマンド行の誤りの後に始め、出力の後に
  path）と言語サーバー（stderr へ、`cmd/tsc/lsp.go` と同じ）。`custom/startCPUProfile`・`custom/stopCPUProfile`。
- **heap・allocation**：`custom/saveHeapProfile`・`custom/saveAllocProfile`・`--pprofDir` の memory profile は「tsc-rs は
  対応しない」のエラー（Go の runtime は割り当てごとの stack を採るが tsc-rs の allocator は採らない）。
- **`custom/runGC`**：GC はないので binary の allocator が使わない memory を返す（mimalloc の `mi_collect`）。答えは tsgo
  と同じ。
- **要求失敗の telemetry**：tsgo `recover`。要求の panic を stack 付きで log（panic hook が stack を保つ。Rust の stack は
  panic を捕えた後には残らない）、telemetry が有効なら `RequestFailureTelemetryEvent`。stack は tsgo
  `sanitizeStackTrace` の規則を Rust の backtrace に（tsc-rs の frame と repository の path は残し、他の frame は伏せる、
  VS Code の generic secret の pattern を外す）。四つの panic の場所が共有。
- **flaky diagnostics**：tsgo `handleDocumentDiagnostic` の `trackFlakyDiagnostics`（program の emit の後にもう一度
  diagnostics を採り、違えば log・telemetry・`Panic` の時は panic）。lsproto に tsgo `CompareDiagnostics`・`AsString`・
  `CodeAsString`。
- **test**：profile の符号化（Go の sample の種類・period・値）、gzip、profiler の状態と message、本物の CPU profile、
  `--pprofDir`（`--version` の出力の後に path）、server の profile の command の端から端まで、tsgo
  `TestFlakyDiagnosticTrackingParallelEmit`（`noEmitOnError` の両方）、tsgo `stack_sanitizer_test.go` の Rust の stack
  への置き換え（利用者の checkout の path 付きの debug の stack、release の stack）と generic secret の場面はそのまま、
  本物の panic の stack での `recover` の答えと telemetry。
- **検査（9ae79a0cd）**：conformance 0 regressions（13,577 全て full、501 s。`--pprofDir` の wrapper は option なしで
  出力を変えない）、`--checkers 4` は既知の 4 件だけ、suites 0 regressions（行の変化なし、lsp 4,583・tsc 227）、test
  全通過（lsp 41・compiler 233・lsproto 31 ほか）、API client sync 356/356・async 365/365、clippy、fmt。
- **有界の違い**：tsgo の 5 分ごとの性能 telemetry（Go の runtime の統計）は移さない。CPU profile は Unix の signal が要る
  （他では `startCPUProfile` が platform の非対応を答え、`--pprofDir` は tsgo の `BeginProfiling` の誤りと同じく失敗）。
  pprof-rs の依存（Unix だけ）。
- **README**：Limitations に profile（CPU だけ、Unix）。

## L11d 型の自動取得（ATA）（#819）

- **merge**：#819 → `181c9595c`（hosted run 38755394431：rust 14m12s、conformance 32m07s、gates）。commit：
  4ff06acde（project の `ata` package）、cfd8f2e76（program・project・lsp）。検査の後の main の取り込みは #818 の merge
  （この branch の土台の code）と L11c の記録だけ。
- **`project/ata`**：tsgo `DiscoverTypings`（config の include、`package.json`・`bower.json` の依存と型を持たない導入済み
  package、tsgo の typesMap のよく知られた script 名、JSX の React、解決できない import（Node の core module は node）。
  exclude を除き、cache の新しい typings を除く。監視する manifest と module の folder）。tsgo `TypingsInstaller`
  （cache の `package.json` と lock file（v1 と v3）、npm の `types-registry` を一度入れて読む、`@types` の package を
  検査して 8,000 文字以下の command で install、file を cache から解決、失敗は missing に）、`ValidatePackageName` とその
  message。tsgo `semver.Version`（types の semver を使う）。checker の Node の core module の判定を共有。
- **統合**：tsgo の `TypingsLocation` と `NpmExecutor`（両方ある session が install する。`tsc-rs --lsp` は tsgo
  `GetGlobalTypingsCacheLocation`（`<user cache>/typescript/7.1`）と `npm`）。更新ごとに tsgo `triggerATAForUpdatedProjects`
  （inferred と synthetic は既定で、configured は config の通り。`ShouldTriggerATA`）を背景で、editor の進捗
  （`Installing types for {0}`）と共に。新しい typings は保留の変更になり、どの flush も取り、diagnostics の refresh が
  更新を届ける。利用者の `automaticTypeAcquisition` の設定で止まる。tsgo `DidUpdateATAState`（typings info が変わらない
  project に typings の file を root として、`getCommandLineWithTypingsFiles`）とその監視（`getTypingsLocationsGlobs`）。
  tsgo `Program.GetUnresolvedImports`。
- **resolver**：tsgo `tryResolveFromTypingsLocation`。型を取得する project の program に typings の cache
  （`ProgramOptions.TypingsLocation`）を渡し、TypeScript に解決しなかった非相対の名前を cache の `node_modules` の宣言で
  試す。それを持たない program（全ての compile、conformance の suite）は変わらない。
- **test**：tsgo `discovertypings_test.go`（9）、`validatepackagename_test.go`、`installnpmpackages_test.go`、installer
  （tsgo の npm の mock）。`ata_test.go` の `TestATA` の場面 17（tsgo が skip する 1 を除く。tsgo の
  `SetupWithTypingsInstaller` と npm の mock）と、やり直した解決が typings の cache に着くことの確認（tsgo の assertion は
  この経路なしでも通る）。`contentmapper_test.go` の `TestContentMapperInferredProjectSurvivesTypingsInstall`（L10d から
  持ち越し）。project の test の session は tsgo の `Setup` と同じく typings を install する（全て変わらず通る）。
- **検査（cfd8f2e76）**：conformance 0 regressions（13,577 全て full、484 s）、`--checkers 4` は既知の 4 件だけ、suites 0
  regressions（行の変化なし）、test 全通過（project 235・lsp 41・api 330・compiler 233 ほか）、API client sync 356/356・
  async 365/365、clippy、fmt。
- **有界の違い**：tsgo の `MustParse` は壊れた registry や lock file の version で panic、tsc-rs はその typing を cache に
  入れない。install の log は終わった時に書く（tsgo は次の更新の builder の log に埋める）。`tsc-rs --lsp` は `npm` を名前で
  起動（Windows では Go の検索は `npm.cmd` も見つける）。
- **README**：language server の節から「Not yet: automatic type acquisition」を外す。

## L11e 内蔵の file 監視（#820）

- **merge**：#820 → `0d8b60fe3`（hosted run 38761956035：rust 14m16s（Linux の inotify でも監視の test が通る）、
  conformance 32m46s、gates）。commit：cac8aede5（compiler・lsp）。検査の後の main の取り込みは #819 の merge（この
  branch の土台の code）と L11d の記録だけ。
- **lspwatcher**：tsgo `lsp/lspwatcher`。client が `workspace/didChangeWatchedFiles` の動的登録を持たず、system が木を
  一度に監視できる時（macOS の FSEvents、Windows の ReadDirectoryChangesW：tsgo `HasFastRecursiveBackend`）、server が
  session の watcher を自ら監視し、変更を client の通知と同じ形で 75 ms ごとにまとめて session に渡す。無い directory は
  一番近い既存の祖先で監視し、現れたらその中身（再帰なら木）を作成として報告（`emitSyntheticCreates`）、消えた
  directory も同じく祖先に戻る（`handleTerminated`）。id を失敗させるのは backend の失敗だけで、id は session の再試行の
  ために戻す。server の `WatchFiles`・`UnwatchFiles` が使い、shutdown で閉じる。他の platform では従来どおり監視なし
  （tsgo の log の通り）。
- **backend**：tsgo の callback（`fswatch.WatchCallback`：event、`ErrOverflow`、`ErrWatchTerminated`）を取る形に。
  `tsc --watch` の manager に tsgo `handleWatchTerminated`（消えた directory の監視を rebuild と共にやり直す）。process の
  backend は多数の監視を扱う（tsgo `fswatch` と同じく）：notifier は一つ、event は backend の thread が配る（callback が
  監視を始め・止められる）、各監視は自分の directory の下の変更を自分の綴りで受け、同じ directory の監視は共有（どれかが
  木を望む間は木を監視）。tsgo の backend と同じく、消えた path は削除（FSEvents は path の flag を溜め、notify はそれを
  別々の event にする）、directory 自身の変更は報告せず削除は監視を終える、相対（bundled の library の）や無い directory
  は Go の message で失敗（`fswatch: path must be absolute`、`stat <dir>: no such file or directory`）。
- **見つけた不具合**（端から端の test で）：notify の FSEvents の watcher は path の追加と削除のたびに stream を止めて
  「今から」で始め直し、途中の変更を落としていた。tsgo は新しい stream を始めてから古いものを止める。macOS では全ての
  directory を持つ新しい notifier を始めてから古いものを捨てる。
- **`gofmt::quote`**（Go の `%q`）を content mapper の message と共有。
- **test**：tsgo `lspwatcher_test.go` の 16 全て（tsgo の偽の backend で bookkeeping・kind・祖先・下降・合成の作成・
  終了・巻き戻し・close の競合、本物の backend で作成・変更・削除と後で作られる directory）。process の backend を本物の
  file system で 6（共有、再帰、消えた directory の終わり、callback からの監視の開始、error の message）。manager の
  `handleWatchTerminated`。server の端から端（macOS と Windows）：client に watcher を登録せず、disk の変更が session の
  diagnostics に届く（watcher の変更を捨てると落ちることを確かめた）。
- **検査（cac8aede5）**：conformance 0 regressions（13,577 全て full、525 s）、`--checkers 4` は既知の 4 件だけ、
  suites 0 regressions（行の変化なし。tscWatch・tsbuildWatch は harness の backend で変わらず）、test 全通過（lsp 58・
  compiler 240 ほか。本物の file system の監視の test は macOS で 5 回続けて通過）、API client sync 356/356・async
  365/365、clippy、fmt。
- **L11 の完了**：L11a〜L11e で tsgo の server と session の transcript が届かない機能は全て。残りは P5-5b
  （`printNode` の `preserveSourceNewlines`）と L12。
- **有界の違い**：tsgo の session は program files の watcher に library の directory（`bundled:///libs/**/*`、
  `createResolutionLookupGlobMapper`）を入れるが、tsgo の内蔵の監視はそれを監視できず（`fswatch: path must be
  absolute`）、その id 全体が失敗して保留のまま残る。library を埋め込んでいる間、内蔵の監視では `node_modules` と
  workspace の外の file が監視されない。tsc-rs は tsgo の振る舞いのまま（本家での修正はその directory を飛ばすこと）。
  tsgo の `fswatch`（FSEvents・inotify・fanotify・kqueue・Windows）は移さず、`tsc --watch` が使う `notify` crate の上に
  上の event の規則を置いた。
- **README**：language server の節に内蔵の file 監視。

## P5-5b `printNode` の `preserveSourceNewlines`（#821）

- **merge**：#821 → `df8e03129`（hosted run 38776929704：rust 12m14s、conformance 32m38s、gates）。commit：fcc973bcd
  （emitter・api）。検査は main を取り込んだ 4d91a00e2 で（merge の前の main の取り込みは無し）。
- **tsgo の振る舞い**（decode した木）：tsgo の decoder は factory で親の無い node を作る。そのため option があると、
  list の最初の項目の前に行を書かない（`getLeadingLineTerminatorCount` は次の token そのものから測る：
  `getPreviousNonWhitespacePosition` が token の位置から始まり、常に 0）、項目の間にも書かない（親の無い兄弟は比べられ
  ない）、閉じの行は source の行。行の無い項目は自分の位置の comment を先に書く（intervening comment）ので、tsgo は
  comment を二度書くことがある（`// c\na;` は `// c\n// c\na;`）。
- **実装**：`printNode` が option を printer に渡す（tsgo `newPrinter`）。printer の list のうち固定の改行を書いていた
  所を tsgo `emitListItems` の数に：source file の文、関数本体（prologue の前の行、その後の list）、空の list（option が
  一行の範囲を一行に保つ時は行なし）、enum の member と tuple の要素、JSX の開き要素（`writeLineSeparatorsAndIndent
  Before/After`）。文・member・case 節・decorator・enum の list の intervening comment。tsgo `emitModifierList`（decorator
  と modifier の連なりをそれぞれ一つの list に：`LFDecorators`・`LFModifiers`）。tsgo `emitComments`（`//` の comment は
  必ず行を終える）、file の末尾の comment の後の行は書いた時だけ。while 文の keyword の前に空白を書かない、空白の後の
  埋め込み文は自分の先頭の comment を書く。
- **既定の印字の修正**（同じ比較で見つけた）：二項式の括弧付きの左の被演算子は外側の文法を持ち込まない（初期化子の中で
  `/` の左の comma 式に tsgo は括弧を一組、tsc-rs は二組書いていた）。
- **tsgo との比較**（local、P5-5 の方法：conformance の test の file 12,829 を tsgo の `createSourceFile` で作り、同じ
  bytes を両方の `printNode` に送る。file 全体と各文 104,015）：option なし 24 → 12 file・1 → 0 文、
  `preserveSourceNewlines` 2,255 → 15 file・45 → 0 文（以前の tsc-rs は option を無視、「前」は option を渡しただけの
  数）、`neverAsciiEscape`・`terminateUnterminatedLiterals` 単独 12 file・0 文、三つとも 15 file・0 文。残りは全て有界：
  BOM 10・非 ASCII 2（client が UTF-16 の位置を送り tsgo が UTF-8 の offset として読む、P5-5d）、tsgo が option で panic
  する 3（`getPreviousNonWhitespacePosition` が file の末尾で `text[len]` を読む。tsc-rs は印字する）。
- **test**：`preserveSourceNewlines` の 14 の形（文、二度書く comment、file の末尾の comment、member、enum、tuple、
  block、埋め込み文の comment、decorator、case 節、prologue、回復した block）と括弧の被演算子を tsgo の答え（aa814927）
  で固定。前者は option を渡さないと落ちる。
- **検査（4d91a00e2）**：conformance 0 regressions（13,577 全て full、482 s。既定の emit が変えた list を通る）、`--checkers 4` は
  既知の 4 件だけ、suites 0 regressions（行の変化なし。lsp 4,583/4,584：LS の change tracker も
  `preserveSourceNewlines` で印字する）、test 全通過（emitter 350・api 332・ls 214 ほか）、API client sync 356/356・
  async 365/365、clippy、fmt。
- **有界の違い**：上の BOM・非 ASCII の file（tsgo の位置の扱い）、tsgo が panic する 3 file。
- **API server の完了**：L9 と P5-5b で API packet の残り（LS の handler 6、`formatNodeForInsertion`、
  `preserveSourceNewlines`）が全て揃った。README の API server の節から「Not yet」を外す。

## 実 project の LSP 差分 test の計画（2026-10-16、利用者の判断）

- **判断**：L11 と P5-5b の後に利用者に確かめた。対象は **hono・zod・effect**（TypeScript・next.js、vscode、
  DefinitelyTyped の sample も示したが選ばれなかった）。L12 の editor での確認（決定 4、VS Code）は**後回し**にして、この
  差分 test を先に行う。
- **方法**（2026-10-14 に利用者が承認）：tsgo を oracle にする。各 project で session を生成し（file を開き、識別子
  ごとに hover・definition・references・completion・signature help、file ごとに document symbol・folding・semantic
  token・inlay hint・formatting・diagnostics、編集による入力時の format と completion、rename）、同じ session を
  `tsgo --lsp --stdio` と `tsc-rs --lsp --stdio` に実 file の上で送り、応答を比べる（tsgo が順序を保たない配列は
  transcript と同じく正規化）。fourslash の記録器は使えないので新しい driver を作る。
- **目的**：fourslash の場面を超える実 code での一致と、checker・binder・printer の bug 探し（LSP の要求は識別子ごとに
  checker を通る）。checker の修正には Rust の回帰 test。
- **その後**：後回しにした VS Code での確認と L12 の README。

## 実 project の LSP 差分 test：hono・zod・effect の一回目（#822）

- **merge**：#822 → `a7e97dae3`（hosted run 38816901429）。commit：c68211b40（hono）、6307bcbec（zod）、28545cbe7（effect）、
  ed424dcde（exclude の照合が lone surrogate を保つ・6.0.3 の規則を固定していた test を tsgo へ）。
- **道具**（repository の外、`~/dev/real-projects.noindex/lsp-compare`）：`lsp-compare.py record` が project の
  `src/**/*.ts` を一つずつ開き、file ごとに diagnostic・document symbol・folding・semantic token・formatting、散らした
  識別子で hover・definition・type definition・document highlight、その一部で references、`.` の後で completion、`(` の
  後で signature help を送る。同じ session を tsgo（aa814927）と tsc-rs に送り、tsgo が順序を保たない配列は並べ替えて
  比べる（`compare`）。`probe`・`both.py`・`cases.py` は一つの位置を両方に尋ねる（`--touch`・`--before` で前の要求、
  `--raw` で並べ替えなし）。
- **結果**：

  | project | 要求 | 違い（前） | 違い（後） |
  |---|---:|---:|---:|
  | hono（`src`） | 47,637 | 169 | 44 |
  | zod（`packages/zod/src`） | 59,207 | 9 | 1 |
  | effect（`packages/effect/src`） | 89,249 | 277 | 70 |

- **直したもの**（それぞれ tsgo の答えで Rust の回帰 test に固定）：
  - hono：type definition（初期化子の無い変数と値の宣言の無い型の symbol を等しいとみる Go の nil 比較で、call
    signature の戻り値型へ）。signature help の戻り値型は checker 自身の emit context で作られるので、emit flag・
    synthetic comment・comment range を落とす（tuple は `[ string, number ]`、property の comment は出ない）。node
    builder：recovery boundary が approximate length を戻し（tsgo `finalizeBoundary`）、展開の切り詰めだけが hover を
    「切り詰めた」とする（`canIncreaseVerbosity`）。template literal 型の union の span は checker の `mapType` で写し、
    その union が template に合う string literal を除く（type table は span の組み立てだけ）。
    `checkUnmatchedJSDocParameters` は TypeScript の file では suggestion。JSDoc の `@param`/`@returns` の型の名前解決が
    host の関数へ進むのは JavaScript の file だけ（tsgo の reparse は JavaScript だけ）。
  - zod：`this` 型の `this` は token（tsgo が合成する keyword）で、hover の meaning では型の参照、`this` の参照では
    keyword としても参照としても扱う（`const` と merge した interface で `this` を表示）。既存の型 node の再利用は `this`
    型を常に写す（tsgo nodecopy.go）。
  - effect：config の file の探索は exclude を exclude の用途で compile し、項目の path 全体に合わせる（tsgo
    `matchFileNames` の `UsageExclude`）：`/**` で終わる exclude がその directory の下を全て除く（effect の
    `tsconfig.tests.json` が deno の test 21 file を含めていた。`tsc-rs -p` にも影響）。index signature で答える
    property access は index symbol を resolved symbol として記録し、`getTypeOfSymbolAtLocation` が narrow した型を答える。
    readonly の index signature は修飾子の長さ（+9）を node builder と checker の型表示の両方で数える（長い型の切り詰め
    位置が変わる）。`GetHostSignatureFromJSDoc` は comment が付いた宣言だけを見る（tsgo は `getEffectiveJSDocHost` を
    移植していない）。
- **調べ方の記録**：切り詰め位置の違いは、tsgo の copy（scratchpad の `tsgo-debug`）の `approximateLength` の変更全てに
  呼び出し位置と合計を `APPROX_LOG` に書く行を足して build し、tsc-rs にも同じ記録を一時的に足して、二つの列を並べて
  最初に合計が戻らなくなる所を探した（readonly の +9 はこれで見つけた）。
- **有界の違い**：
  - auto-import の module specifier（hono 42、effect 69 の completion）：tsgo の specifier cache は export の file
    （`Export.Path`）で引き、module augmentation の export は augment する file の path と augment される module の
    file 名を持つので、その file の export のうち最初に計算されたものが cache を埋める。tsgo はそれを Go の map の順で
    計算する（`View.GetCompletions` の `range grouped`）。同じ session で tsgo 自身の答えが変わった（hono で
    `../language` が一回、`../..` が一回）。
  - hono の `Context<any, any, {}>.header` の hover：同じ session 5 回で tsgo は宣言の型引数を 2 回、具体化した引数を
    3 回答えた（query checker で property の型が具体化より先に解決されたかどうか）。
  - node builder の cache の寿命（hono の signature help、zod の hover）：tsgo は `serializedTypes` を NodeBuilder に
    持ち（signature help の item ごと、`typeToString`・`TypeToTypeNode` は checker ごと）、後の parameter や要求が数えた
    長さと共に完全な node を再利用する。tsc-rs の cache は呼び出しごと。次の slice。
  - effect の `Ordering.match` の signature help：tsgo は戻り値型で `{ onEqual, onGreaterThan, onLessThan, }` と書く。
    位置で決める `NodeList.HasTrailingComma` が clone した要素の list（位置を持つ list に合成した要素）で真になり、別の
    emit context の printer は元の list を辿って直せない。tsc-rs の list は末尾の comma を flag で持つ。
- **検査（ed424dcde）**：conformance 0 regressions（13,577 全て full、492 s）、`--checkers 4` は既知の 4 件だけ（15,335 一致）、
  suites 0 regressions（ratchet は変わらず。lsp 4,583/4,584、tsc 227/229 は generateTrace の 2 件）、test 全通過（types 47・
  syntax 251・binder 78・program 683・checker 1,806・emitter 350・ls 226・lsp 58・api 332・project 235・compiler 240・
  conformance 49・lsproto 31）、API client sync 356/356・async 365/365、clippy（`-D warnings`）、fmt。一回目の検査で
  program の UTF-16 の test（lone surrogate を含む path の exclude）と、6.0.3 の規則を固定していた checker の test 2 件が
  落ち、直してから全てを最後の bytes でやり直した。
## 実 project の LSP 差分 test：node builder の cache の寿命（#823）

- **merge**：#823 → `c62c43acb`（hosted run 38825894719）。commit：76b8288bc（cache の寿命）、b4b74a056（types writer）。
- **tsgo の builder**：NodeBuilder は作った node の cache（`serializedTypes`、enclosing declaration ごと）を builder が
  生きている間持つ。後の build は完全な node を数えた長さと共に再利用し、新しく作れば切り詰める所でも切らない。
  - checker 自身の builder（`getNodeBuilder`、printer.go）：`typeToString`・`symbolToString`・`signatureToString`・
    `typePredicateToString`・`SignatureToSignatureDeclaration`・`ExpandSymbolForHover`・`TypeParameterToStringEx`。
    checker が生きている間、要求をまたいで持つ。
  - 呼び出しごとに新しい builder（`getNodeBuilderEx`）：`TypeToTypeNode`・`TypeToTypeNodeEx`・
    `TypePredicateToTypePredicateNode`。
  - signature help の item ごとの builder：`getTypeHelpItem`・`itemInfoForTypeParameters`・`itemInfoForParameters`。
  - types baseline の walker：node ごとに `NewNodeBuilder`。
  hover の verbosity context を持つ build と enclosing declaration の無い build は cache を読み書きしない（変わらず）。
- **実装**：checker の display arena が node builder の cache を持ち続ける（宣言の emit の request builder と同じ
  `keep_node_builder_cache`）。`CheckerState::with_node_builder` が `NodeBuilder` 自身の cache を呼び出しの間だけ
  display に貸す：signature help の item（label の build だけが通り、parameter の documentation は checker の
  builder のまま）、`TypeToTypeNode` に当たる `type_to_type_node_with_flags`・`signature_help_type` は呼び出しごとに
  新しいもの、conformance の types writer は node ごとに新しいもの。
- **途中で見つかったこと**：最初は signature help の戻り値型も checker の cache を共有し、hono の signature help 2 件で
  別の signature が切り詰めた node を再利用した（tsgo の `TypeToTypeNode` は新しい builder）。一回目の全体検査で
  `fixSignatureCaching.types` が別の node の循環の中で作った node を再利用して 1 件 mismatch（tsgo の walker は node
  ごとに新しい builder）。
- **結果**（#822 の比較を同じ session で）：hono 44 → 40、zod 1 → 0、effect 70 → 68。残りは全て #822 で記録した有界の
  違い（auto-import の specifier：hono 39・effect 67、hono の `Context<any, any, {}>` の hover、effect の
  `Ordering.match` の末尾の comma）。
- **test**（tsgo の答え aa814927 で固定）：`a_later_hover_reuses_the_checker_s_built_nodes`（前の hover の qualifier が
  作った `typeof a` を後の hover が丸ごと再利用、単独なら `... 8 more ...`。test helper が一つの program に要求を順に
  送る）、`signature_help_items_and_return_types_keep_their_own_built_nodes`（item の後の parameter が前の parameter
  の node を再利用し、戻り値型は signature ごとに新しく作る）。どちらも変更なしでは落ち、後者は戻り値型が cache を
  共有しても落ちる。
- **検査（b4b74a056）**：conformance 0 regressions（13,577 全て full、types・symbols 12,874 全て full、484 s）、
  `--checkers 4` は既知の 4 件だけ（15,335 一致）、suites 0 regressions（ratchet は変わらず：lsp 4,583/4,584、tsc
  227/229）、test 全通過（types 47・syntax 251・binder 78・program 683・checker 1,806・emitter 350・ls 228・lsp 58・
  api 332・project 235・compiler 240・conformance 49・lsproto 31）、API client sync 356/356・async 365/365、clippy
  （`-D warnings`）、fmt。
- **有界の違い**：tsgo の checker pool は 30 秒使われない checker を捨てる（`IdleTimeout`）ので、editor の中で時間を
  置くと tsgo の cache は空からやり直す。tsc-rs の query checker は捨てないので、間を置いた後の切り詰めの位置が違い
  得る（生成した session には間が無い）。

## 実 project の LSP 差分 test：playwright-core・vue・next.js（#824）

- **merge**：#824 → `65d0692e4`（hosted run 38863007896。rust の job は既知の終了の race（`pushed_config_diagnostics_match_tsgo` の
  `EXIT 0`）で一度落ち、再実行で通った）。
- **対象**：playwright-core（`packages/playwright-core/src`、328 file）、vue（`packages/*/src`、234 file）、next.js
  （`packages/next/src`、1,457 file）。#822 と同じ生成した session（tsgo aa814927 が答え）。
- **結果**：

  | project | 要求 | 違い（最初） | 違い |
  |---|---:|---:|---:|
  | playwright-core | 60,108 | 41,310 | 50 |
  | vue | 41,256 | 38 | 10 |
  | next.js | 229,939 | 185 | 0 |
  | hono（再実行） | 47,637 | — | 43 |
  | zod（再実行） | 59,207 | — | 0 |
  | effect（再実行） | 89,249 | — | 65 |
  | TypeScript compiler（`src/compiler`） | 36,739 | — | 0 |
  | vscode（`src/vs/base`、program は src 全体） | 89,329 | — | 0 |

  （playwright の最初の数は下の panic の連鎖。残りは全て tsgo 自身の揺れで、同じ hono の session の tsgo どうしでも
  completion が 10 件違った。）
- **修正**：
  - **auto-import の並べ替えは Go の `slices.SortFunc`**：tsgo の fix の比較は全順序でない（node core と re-export の
    規則が同じ種類の fix 同士でしか比べない）。Rust の sort はこれで panic した（playwright の completion）。Go 1.27 の
    pdqsort を `tsc_types::sort_func_like_go` に移し、tsgo が `slices.SortFunc` で並べる auto-import の 4 か所で使う。
  - **panic した要求の後も checker を使う**：tsgo は panic した要求に panic を答え、`defer done()` で checker を pool
    に返す。session の mutex が poison しても状態を取り戻す（前は以後の要求が全て失敗）。
  - **module specifier の host は checker ごとに一つ**：auto-import は fix ごとに
    `GetModuleSpecifiersForFileWithInfo` を聞き、そのたびに host（全 file の path・mode・text と program の symlink）
    を作り直していた（next.js の completion 1 回が 175 秒）。最初の検索で作り、以後は共有する。
  - **auto-import の registry の alias resolver は tsgo の形**：
    - format の hook は全ての file を ESNext と答える。なので `.d.cts` の `export { default as ar } from "./ar.cjs"`
      （`export =` の module）には合成 default が無い（playwright の zod 4 の locale）。alias resolver の file に ESNext
      の format を持たせ、checker は拡張子より program の format を先に読む（実 program では一致）。
    - 解決が届いた file はどれも parse する（`GetSourceFileForResolvedModule`。alias の解決は解決の診断を出さない）。
      なので JS だけの package からの re-export は CommonJS の export に解ける（next.js の `createServerReference`）。
      checker は program の外で読む file を拡張子で落とさない：`allowJs` 無しで落とすと target の token が分からず、
      記録した失敗がその checker の以後の module の解決を全て失敗にしていた。
  - **symlink cache の依存の pass は tsgo の形**：`GetSymlinkCache` は emit する file の package scope の実行時依存を
    package directory として探し（`ResolvePackageDirectory`：近い `node_modules/<name>` か
    `node_modules/@types/<name>`、`paths` は見ない）、scope の `node_modules/<name>`（`@types` も）が既に知っている
    symlink の directory なら飛ばす。tsc-rs は module として解いたので、`paths` が写す workspace の package（vue の
    `@vue/*`）の link が見えず、auto-import が相対 path で名付けていた。宣言 emit の specifier にも効く。
  - **`getContextualTypeForObjectLiteralElement` は tsgo の形**：要素自身の型 node を先に答える（object literal の
    method を除く）。class の method の parameter の中の signature help は戻り値型の signature を出す（playwright の
    `debugProtocolLogger`）。
  - **symbol の宣言は tsgo の `IsDeclaration`**（modifier を読む時）：`@deprecated` の付いた `export default` の
    auto-import の item は deprecated（next.js の cache handler）。`node_util::is_symbol_declaration` は
    `is_declaration_node` に、tsgo の reparse した宣言の代わりに tsc-rs が bind する JSDoc の tag を足したもの。
  - **合成 default の wrapper の symbol は module の宣言を持つ**（tsgo `getTypeWithSyntheticDefaultImportType`）：
    `await import('inspector')` の値の type definition は module へ行く。
  - **別の emit context の printer**：tsgo は `TypeToTypeNode` の node を checker 自身の emit context で作る。
    signature help の printer は list の元を辿れず位置を信じるので、`cloneBindingName` が clone した binding pattern
    （元の list の範囲に合成した要素）は末尾の comma 付きで書かれる（`({ a, b, }: Box) => void`。effect の
    `Ordering.match`・playwright の `bind`、前は有界として記録）。
  - **1 行の type literal の member は間の comment を書く**（tsgo `emitListItems`）：comma の後ろの property の行末の
    comment が signature help に出る（next.js の config schema）。
- **残る違い（tsgo 自身の揺れ）**：
  - node_modules の bucket は export を file の Go の map の順に index に入れるので、同じ ExportID の二つの export
    （codemirror の augmentation と namespace）では後の方の `Path` が specifier を決める（tsgo は三つの session で
    `json-lint`・`yaml-lint`・`html-lint` と答えた）。
  - specifier の cache は hit を `Relative` と答えるので、どの group が file を先に計算するか（`range grouped`）で
    順位が変わる：playwright の `registry`（tsgo は 6 回中 1 回 `.`）、vue の runtime-core の `version`（tsc-rs の順は
    runtime-core の最初の export を最初に計算する）、hono の timing middleware。
  - hono の `Context<any, any, {}>` の hover（#822 で記録）。
- **調べ方**：tsgo の copy（scratchpad の `tsgo-debug`）の `GetSymlinkCache`・`GetModuleSpecifier`・
  `tryGetModuleNameAsNodeModule`・`createExport` に `SYMLINK_LOG` へ書く行を足した。比較の script は記録（next.js は
  8 GB）を一行ずつ並べて読む（丸ごと読むと 52 GB 使った）。`record` は 1 秒を超えた要求を `<name>.slow` に書く。
- **test**（tsgo か Go の答えで固定、どれも変更なしでは落ちる）：types `the_go_sort_port_gives_go_s_order`・
  `the_go_sort_port_tolerates_a_comparison_with_no_total_order`、compiler
  `a_query_that_panicked_leaves_the_checker_to_the_next_query`、lsp `auto_imports_resolve_a_package_s_aliases_as_esm`・
  `auto_imports_name_a_file_through_a_dependency_s_symlink`・`auto_imports_resolve_a_package_s_javascript_re_export`・
  `a_deprecated_default_export_s_auto_import_is_deprecated`、ls
  `signature_help_in_a_class_method_s_parameters_shows_its_return_type`・
  `signature_help_return_types_print_cloned_binding_patterns_with_a_trailing_comma`・
  `signature_help_prints_a_property_s_same_line_comment`・
  `a_dynamically_imported_module_s_type_definition_goes_to_the_module`。
- **検査（57fea92cf、origin/main 3dfa47352 の上）**：conformance 0 regressions（13,577 全て full、types・symbols 12,874
  全て full、477 s）、`--checkers 4` は既知の 4 件だけ（15,335 一致）、suites 0 regressions（ratchet は変わらず）、test
  全通過（types 49・syntax 251・binder 78・program 682・checker 1,806・emitter 350・ls 232・lsp 62・api 332・project 235・
  compiler 241・conformance 49・lsproto 31）、API client sync 356/356・async 365/365。79bacc21d（重複した `#[allow]` を
  一つ消しただけ）で clippy（`-D warnings`）と fmt。c4ffb0412 での一回目の全体検査は conformance の 3 件
  （`symlinkedWorkspaceDependenciesNoDirectLink*GeneratesNonrelativeName`）を落とした：harness が link の directory の
  realpath を返さず（file の alias だけ）、package directory を realpath する依存の pass が link を見失った。tsgo の
  compiler runner の vfs と同じく link の directory も target に realpath するようにした（57fea92cf）。
- **性能の観察**（測定は利用者と一緒に）：session 全体の時間は tsgo の 2〜5 倍（next.js 1,246 秒対 324 秒、
  TypeScript compiler 279 秒対 52 秒）。auto-import の completion は next.js で 1 回約 5 秒。
