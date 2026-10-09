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
