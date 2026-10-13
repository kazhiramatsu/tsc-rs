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
