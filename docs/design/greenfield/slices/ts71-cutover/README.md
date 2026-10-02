# TypeScript 7.1への切替（6.0.3の維持終了）

状態：**設計**（2026-10-01）。6.0.3互換のreleaseは実施済み（下記）。実装はP3-1から。
前段：[conformance-ts71](../conformance-ts71/README.md)（P1 runner、P2-1 既定値、P2-2 relation head）、
[TS7の方向](../../typescript-7-direction.md)、[post-emitter roadmap](../../post-emitter-roadmap.md)。

## ユーザー決定（2026-10-01）

| # | 決定 | ユーザーの言葉・選択 |
| --- | --- | --- |
| 1 | 6.0.3互換の時点で一度releaseし、以後は6.0.3の内容を維持せず、7.1とのconformance一致を改めて目指す | 「6.0.3互換までで一度releaseしてしまい、7.1からは完全に6.0.3までの内容を維持せず改めてconformance一致を目指す」 |
| 2 | releaseの対象commitは`ed173ea36`（PR #610のmerge。既定挙動とREADMEが6.0.3互換の最後のmain） | 選択 |
| 3 | 形式はtag `v0.1.0`＋GitHub Release＋branch `release/6.0.3`（source release、binaryなし） | 選択。実施済み：[v0.1.0](https://github.com/kazhiramatsu/tsc-rs/releases/tag/v0.1.0) |
| 4 | 6.0で非推奨・7.xで削除されたoption（ES5、outFile、AMD/UMD/System、node10/classic、baseUrl等）は7.1に完全に従う | 選択。2026-09-29の決定2〜4（非推奨optionは6.0.3どおり）を置き換える |
| 5 | emit（JS・d.ts・source map）は7.1のjs/sourcemap baselineをvendorしてconformanceで比較する | 選択 |
| 6 | 6.0.3の観測に依存する検証面は、7.1のemit比較が入った後にmainから退役・削除する | 選択 |
| 7 | CLAUDE.mdの検証方針は本packetに文案を載せ、切替PRで適用する | 選択 |

これまでの二重profile（`ReferenceProfile`、6.0.3の凍結記録を守るための経路）は、binderや宣言構造に触る
7.xの変更（JSの`this`・expando、JSDoc規則、重複識別子）で両立が難しくなる見込みだったため、
6.0.3の成果をreleaseとして保存した上で7.1に一本化する。

## 範囲と原則

- `main`は7.1系。参照は`vendor/typescript-native/7.1.0-dev-19dadef8`（7.1のtagが出た時点で移る）。
  同じcommitからbuildしたtsgo（`scripts/typescript7.py`のcheckout、`go -C tsc build ./cmd/tsc`）が参照実装で、
  挙動の疑問はtsgoの実行で決める。
- 6.0.3系は`release/6.0.3`とtag `v0.1.0`。mainには6.0.3の挙動・記録・oracleを残さず、6.0.3の出力を
  byte一致に保つための作業はしない。
- 対象外：LSP、watch/build、project reference（roadmapのP4以降）。

## 棚卸し：6.0.3に依存する面と処置

| 面 | 内容 | 規模 | 処置 | PR |
| --- | --- | --- | --- | --- |
| 参照profileの二重化 | `ReferenceProfile`、`LibraryCatalog::typescript_6_0_3`、`gen::typescript_6_0_3`（11 message）、6.0.3のlib表・提案表、TS1463/1464、transpile経路の6.0.3 profile | 約20 file（types 1、program 6、checker 5、binder 1、emitter 1、harness 1、compiler/transpile 1、test 4） | 6.0.3のarmを削除し、profile型そのものを撤去 | P3-3 |
| `vendor/typescript-6.0.3` | lib 107 entry、`typescript.js`（oracle）、`_tsc.js`（tsc-port headerのspan） | 66 MB | 削除。`tsc-port … @6.0.3`のheaderは出所の記録として残し、参照先をtag `v0.1.0`のvendorと注記。ledger検査は退役 | P3-3 |
| `crates/oracle` | 6.0.3 oracle script（driver、h1/h2 profile・qualification・owner controls等） | 4 MB、90 file超 | 削除 | P3-3 |
| `ratchets/` | h0/h1/h2 artifact、`conformance-matches`、`escapes.toml`、fuzz manifest、`STAGE`等 | 292 MB | `ratchets/ts71/`以外を削除 | P3-3 |
| hosted CI | `ci.yml` acceptance（early/wide/late＝`xtask acceptance`の31 slice、H2.5gの9,027観測を含む）、`witness.yml`（93 suite）、`qualification-policy.v2.json`と`qualification.mjs`のpin、`replay.py`、perf workflow（h1-noemit/l0/l1） | 約4,400行 | `conformance-ts71.yml`と新設`rust.yml`に置換。perf workflowは退役（READMEの手動計測手順は残す） | P3-2 |
| `crates/xtask` | acceptance／h2-*-acceptance／conformance（6.0.3）／ratchet／escapes／ledger／slice-evidence／completion／l0／l1／m8／fuzz等 | 2.3 MB | 6.0.3 oracleに依存するcommandを削除。codegen・readme-status・workspace-audit等の汎用は残す | P3-3 |
| `crates/conformance`旧runnerと`ts-tests/` | families／goldens_diff／h0_memory／host_resolution／identity／ratchet／rendered／scope／shadow_diff、6.0.3 corpus | 1.1 MB＋53 MB | 削除。`ts71.rs`をcrateの本体にする | P3-3 |
| `crates/harness`の6.0.3側、`crates/compiler/tests/fixtures` | 6.0.3 corpusの展開・h1/h2 integration test、witness fixture | 114 MB | 6.0.3観測との比較を削除。自己完結のRust test（parser/binder/program contract、emitter printer contract等）は残す | P3-2／P3-3 |
| checker unit testの6.0.3 pin | creation orderや6.0.3 profileを明示した14件（PR #612）、`type_order` test、「oracle (vendored 6.0.3)」注記のpin | | tsgo probeで7.1に再pin（class移行と同時に） | P3-5 |
| checkerの旧in-memory module resolver | `check_program*`約280箇所 | | program経路へ移してから削除 | P3-6 |
| README／`--version`／examples | 互換target、Run CI、Performanceの比較記述、Stable type orderingの節 | | 7.1へ書換え。`--version`はvendored profile（`7.1.0-dev-19dadef8`）を表示 | P3-2／P3-3 |
| CLAUDE.md | 検証方針、branch workflow、historical tooling | | 下記の文案で差し替え | P3-2 |

## 新しい検証

### conformance-ts71（errors＋emit）

- **lane Aのみ**。Goのskip／致命的ruleをそのまま適用する（ES5、UMD/System、classic/node10、baseUrl、
  `esModuleInterop=false`、`alwaysStrict=false`はskip、AMDとoutFileは致命的）。lane Bはなくなり、
  `downlevelIteration`の23件は7.1どおり実行してTS5102を期待する。
- **emit**：上流の`.js` baseline（Goの`DoJSEmitBaseline`形式。入力unitと`//// [name.js]`／`//// [name.d.ts]`区画の
  連結。compiler 6,158＋conformance 6,016 file、約10 MB）と`.sourcemap.txt`／`.js.map`（157件）をvendorし
  （manifestにsetを追加、`vendor_typescript_native.py --check`とharness testで検算）、runnerにemit tier
  （js一致・d.ts一致・map一致）を足す。ratchet行はerrors tierとemit tierを並べる。
  `.types`／`.symbols`（各約12,700 file、約40 MB）と`trace.json`（module resolution trace、148件）は見送る。
- 1 checker固定。並列対照は従来どおり`--checkers 4`＋`conformance_ts71_compare.py`。
- hostedは`conformance-ts71-gates`を必須check（branch protectionはrepository設定）。

### Rust checks（新設`rust.yml`）

`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、
`cargo test --workspace`（6.0.3観測比較の退役後は自己完結testだけが残る）。worker 2、60分。
今はhostedで走っていないcheckerのunit suite（1,785件）がここで守られる。

### tsgoとのcorpus比較（補助、gateではない）

READMEのcorpora（hono、zod、Playwright、TypeScript `src/compiler`、Next.js、Effect、VS Code）で
diagnosticsとemitをtsgoと比較するscript（`scripts/compare_tsgo.py`。tsgoは`scripts/typescript7.py`の
checkoutからbuild）。release前と各classの完了時に手動で実行し、READMEに記録する。

### 退役

acceptance 31 slice、witness 93 suite、perf workflow、旧conformance、ratchet／escapes／ledger／
slice-evidence／completion gate、qualification policyのpin。

## 作業順

| PR | 内容 | 終了条件 |
| --- | --- | --- |
| P3-1 emit baseline | vendor set追加と検算、Goのjs／sourcemap baseline writerの移植、runnerのemit比較とratchet拡張、初回計測。既存gateはそのまま | lane Aの全configurationにemit tierが記録され、hosted conformance-ts71がerrors＋emitで緑。emitの不一致classを計測して本packetに記録 |
| P3-2 gate切替 | `rust.yml`新設、`ci.yml`／`witness.yml`／perf workflowと`replay.py`・qualificationの6.0.3部分を削除、CLAUDE.md・README「Run CI」の書換え | required check＝`conformance-ts71-gates`＋`rust-gates`。PRのhosted時間が15〜25分 |
| P3-3 削除 | 6.0.3 profileのarm、`vendor/typescript-6.0.3`、`crates/oracle`、`ratchets/`（ts71以外）、xtaskの旧command、旧conformance runnerと`ts-tests/`、6.0.3観測のfixture／test、`--version`・READMEの互換記述 | `rg "6\.0\.3"`の残りが歴史記述（tsc-port header、packet、release note）だけ。workspaceのtestが緑 |
| P3-4 lane Bの7.1化 | runnerのlane判定をGoのskip ruleに置換。非推奨optionの実装は残置（保証なし）とし、削除は別判断 | 15,228 configurationの内訳がGoの実行数と一致 |
| P3-5 class移行の継続 | P2の残りclass（TS6196、overload chain、JSの`this`、TS2300…）をprofileなしで実装。unit testはtsgo probeで再pin | classごとにratchet上昇・0 regressions |
| P3-6 旧resolverの退役 | checker unit testをprogram経路へ移し、旧in-memory module resolverを削除 | `authoritative_module_provider`がNoneの経路が消える |

P3-1をP3-2より先にするのは、emitの退行検知を途切れさせないため。P3-2とP3-3は同じintegration branchでよい
（hostedの短縮がP3-3の大きな削除の検証を速くする）。P3-5はP3-1〜P3-3と並行できるが、profileのarmを
新たに足さない。

## CLAUDE.md差替案

「Current verification policy」を次に置き換える（P3-2で適用）。

> The compiler follows TypeScript 7.1 at the vendored native profile
> (`vendor/typescript-native/<profile>`); tsgo built from the same commit is
> the reference implementation. tsc 6.0.3 compatibility ended with release
> v0.1.0 (tag at `ed173ea36`, branch `release/6.0.3`): `main` keeps no 6.0.3
> behavior, record or oracle, and no change is required to keep 6.0.3 output.
>
> - Merge criteria: the hosted `conformance-ts71-gates` (the TypeScript 7.1
>   error and emit baselines, one checker, `ratchets/ts71/`) and `rust-gates`
>   (formatting, Clippy, workspace tests) succeed for the final candidate.
>   The PR body records the commands, source identity, results and remaining
>   bounded limitations.
> - During implementation, run the affected conformance cases locally
>   (`conformance-ts71 --filter/--case`, `scripts/conformance_ts71.py --filter`),
>   probe tsgo for the exact behavior, add adjacent unit tests pinned to
>   tsgo's output, and keep formatting and Clippy clean.
> - The ratchet must report 0 regressions (`--check`); raise it with
>   `--update` at the final bytes. Lowering a row is a reviewed edit recorded
>   in the owning packet with the 7.1 evidence.
> - After a checker change, run the parallel control (`--checkers 4` and
>   `scripts/conformance_ts71_compare.py`); differences beyond the recorded
>   partition-dependent cases are defects to fix or to record.
> - Compare the README corpora with tsgo before a release.
> - Report only checks actually performed, with their source identity and scope.

「Branch workflow」は3（merge criteria）を上の二つのhosted checkに、8（hosted execution）を
`conformance-ts71.yml`と`rust.yml`に、9（pinned sources）を削除に改める。
「Verification quick reference」からwitness guideと6.0.3のoracle経路を外し、
「Historical tooling」は「6.0.3時代のtool（xtask acceptance／ratchet／escapes／ledger／slice-evidence／completion、
`crates/oracle`、`ratchets/`のts71以外）はP3-3で削除し、`release/6.0.3`とtag `v0.1.0`に残る」に置き換える。

## リスクと未決

- **emitの7.1差**：7.xのhelper・class field・declaration順序などの差はP3-1の初回計測で量を見る。6.0.3の
  期待値を7.1へ無条件に写さない（roadmap C-TRANSPILEの原則）。
- **project級scenarioの喪失**：9,027観測が持っていたconfig・module解決・bundleのscenarioは、tsgoとのcorpus比較と、
  必要なら7.1のtest形式で足すproject testで補う。`trace.json`の採用はその時に判断する。
- **`stableTypeOrdering: false`**：tsgoはoptionを受理して無視する（`compileroptions.go`に項目はあるがcheckerは読まない）。
  tsc-rsも受理して無視し、6.0.3の生成順経路を削除する案をP3-3で採る。READMEのStable type orderingの節を書き換える。
- **hostedの所要時間**：conformance-ts71は今15分。emit比較の増分はP3-1で測る。
- **tsgoのbuild**：GoのtoolchainがhostedでもあればREADME corpora比較を自動化できるが、まずは手動。

## P3-1 emit baseline（2026-10-02）

### 取込みと描画

- `scripts/vendor_typescript_native.py`を、runnerが比較するbaseline種別（`*.errors.txt`、`*.js`、`*.js.map`、
  `*.sourcemap.txt`）ごとにmanifestのsetを作る形に一般化した。`19dadef8`のvendor treeは20,188 fileから32,680 fileになる
  （`.js` 6,167＋6,018、`.js.map` 130＋20、`.sourcemap.txt` 137＋20）。`--check`とharnessの
  `native_vendored_inputs_match_the_manifest`が検算し、`NativeProfile`に各種別のpath helperを足した。
- `crates/conformance/src/ts71/emit_baseline.rs`がGoの`DoJSEmitBaseline`（header、`otherFiles`→`toBeCompiled`の
  入力、JavaScript file、declaration file。`@fullEmitPaths`とBOMを含む）と`DoSourcemapBaseline`（raw mapと
  visualization link）を描画する。再現しないのは`[DtsFileErrors]`区画（出力したd.tsを再compileした診断、参照23件）と
  `noCheck` emitの比較（2件）で、これらの参照は不一致として数える。
- runnerはlane Aの各configurationでdiagnosticsの後に第2 sessionを走らせてemitする（`ProgramSession::emit`、
  memory sink）。Goの`compileFilesWithHost`も診断用とemit用の2つのProgramを作るので、同じ形。reportに`emit`／
  `emit_detail`／`emit_sha256`（`.js`）と`map`／`map_detail`（`.js.map`）が加わり、`--dump`は差分の`.js`／`.js.map`も
  書く。`skippedEmitTests`の8 caseは`NotAssessed`。
- ratchet（`ratchets/ts71/<profile>.tsv`）は第3列にemit tier（`js`＝`.js` baselineがbyte一致、それ以外`none`）を持つ。
  `--check`はerrors tierとemit tierのどちらの後退でも失敗し、`--update`はどちらも上げる。
  `conformance_ts71_compare.py`はemitのdigestも比べる。

### 計測

`python3 scripts/conformance_ts71.py --workers 4 --check`（release build、`taskpolicy -b nice -n 20`、1,230秒。
emitの第2 sessionで従来の784秒から約1.6倍）。errors tierは変更なし（T3 12,641、不一致 623、0 regressions）。

| 区分 | configuration |
| --- | ---: |
| lane Aで比較 | 13,399 |
| 　`.js` baseline一致（emit tier `js`） | 12,386（92.4%） |
| 　`.js` 不一致 | 1,005 |
| 　評価外（`skippedEmitTests`） | 8 |
| 　`.js.map` 一致（参照なし同士を含む） | 12,847 |
| 　`.js.map` 不一致 | 552（うち出力の差 37、残りはemit error） |
| ratchet | emit tierを全行に記録（`js` 11,908行、`none` 868行）。errors tierの後退なし |

### 不一致のclass（初回、`.js` 1,010件）

| 件数 | class | 内容 |
| ---: | --- | --- |
| 515 | emit error | tsc-rsのemitterがparse errorのある入力のemitを拒む（"emit recovery … is deferred to H2.9"、500件超）。`composite`／`incremental`／`tsBuildInfoFile`のunsupported 11件、transformの失敗3件 |
| 453 | 出力の差 | JS fileのdeclaration emitと`arguments`の扱い 245（7.xはexpandoをobject型で出す、`...args`を補わない等）、declaration emit 84（型nodeの引用符の保存、arrow aliasの`function`化、`export =`の位置、literal型の`declare const x = "..."`、unionの順序）、module／export assignmentのmerge 47、class 10（`in`／`out` modifierの残留、accessibility）、async／decorator／template／enumなど |
| 23 | `[DtsFileErrors]` | 再現しない区画 |
| 12 | 出力なし | `noEmitOnError`の2件（7.1はemitする）、JSDocのcase 10件 |
| 7 | 参照なし | うち5件は`skippedEmitTests`（NotAssessedへ）、`pathMappingInheritedBaseUrl`、`isolatedDeclarationsJsThisPropertyAssignmentInference` |

`.js.map`は、emit errorを除くと37件が差（declaration mapのmappingと上と同じclass）。
`.sourcemap.txt`（source-map record、`GetSourceMapRecord`＝`harnessutil.go`の約500行）はP3-1bで移植する。

これらはP3-5でclassごとに直す。emit errorの大半（parse error後のemit）はroadmapのH2.9（emit recovery）そのもので、
7.1 baselineがそのoracleになる。

## P3-2 gateの切替（2026-10-02）

- hosted CIを`.github/workflows/ci.yml`の一本にした：`plan`（変更pathから選択）→`rust`（`cargo fmt --check`、
  `cargo clippy --workspace --all-targets -- -D warnings`、自己完結のRust test target：types／diagnostics／syntax／
  binder／host／checker／program(lib)／emitter／compiler(lib)／harness(lib＋native test)／conformance(ts71)）と
  `conformance (TypeScript 7.1)`（release buildの`scripts/conformance_ts71.py --workers 4 --check`、errors＋emit）→
  `gates`（選択されたjobの成功を要求）。aggregateの名前を`gates`のまま保ったので、branch protectionの
  required check（`gates`）は変更不要。`witness-gates`と`conformance-ts71-gates`はなくなる。
- `.github/ci/replay.py`は小さなplannerに書き直した（docs/・root README.md・CONTRIBUTING.md・LICENSEだけの変更は
  何も選ばない、それ以外は両job）。`test_replay.py`はその契約だけを検査する。
- 削除：`witness.yml`、`conformance-ts71.yml`（ci.ymlへ統合）、`h1-noemit-performance.yml`／`l0-performance.yml`／
  `l1-performance.yml`、`.github/ci/qualification.mjs`とpolicy・pin-index・plans・slice-readiness・
  fci／gate-tax／artifact-schemaのtest。`.github/ci/contracts/`のschemaはharnessのh1／h2 integration testが
  `include_str!`で読むので、そのtestと一緒にP3-3で消す。xtaskの旧commandが参照するfileは実行時にのみ読むので、
  xtaskもP3-3まで残る（hostedのrust jobはxtaskのtestを走らせない）。
- CLAUDE.md：検証方針を本packetの文案で置き換え、branch workflowの3／4／6／7／8を書き換え、9（pin）を削除、
  quick referenceとhistorical toolingを「Retired tooling」に置き換えた。README「Run CI」を新しいjobで書き直し、
  `docs/witness-testing.md`に退役の注記を置いた。
- `rust` jobが走らせないもの：`crates/compiler/tests/*`（witness suite）、harnessのh1／h2 profile、programと
  conformanceの6.0.3 contract、xtask／fuzz／oracle。P3-3で削除した後に`cargo test --workspace`へ切り替える。
- hosted記録：PR #616（head `20ac82e4a`、merge `3c82d6e09`）、run 36900968172 — `plan` 32s、`rust` 14m35s、
  `conformance (TypeScript 7.1)` 22m29s、`gates` 19s、すべて成功。

## P3-3 削除（2026-10-02）

21,475 file、約1,235万行の削除（挿入1,054行）。残った`6.0.3`の出現は`tsc-port … @6.0.3`のheader、
本packetを含む歴史記述、release noteの類だけになった（`rg "6\.0\.3" --glob '!docs/**'`で確認）。

### 削除したもの

| 面 | 内容 |
| --- | --- |
| 参照profile | `ReferenceProfile`型と`CompilerOptions.reference_profile`、`gen::typescript_6_0_3`（11 message）、6.0.3のlib表（`TYPESCRIPT_6_0_3_LIBRARIES`、`LIB_LIST_DESCRIPTOR`）・`TARGET_VALUES`、`SCRIPT_TARGET_FEATURE_*`の6.0.3表、`LibraryCatalog::typescript_6_0_3`／`for_profile`／`reference_profile`、`CompilerConfigHost::with_reference_profile`、`ConfigParseHost::reference_profile`。各armは7.1の挙動に固定：TS1344／TS5090／TS8030／TS9019は7.1の文面、`resolution-mode`はkeyの探索だけ（TS1463／TS1464／TS1454／TS1455の個数・key検査は消滅）、relation headの抑止は常時、`stableTypeOrdering`の既定はtrue、既定targetはES2026。transpile経路も7.1 catalog |
| `vendor/typescript-6.0.3`、`vendor/PIN.md` | 66 MB。`tsc-port … @6.0.3`のheaderはtag `v0.1.0`の`vendor/typescript-6.0.3/lib/_tsc.js`を指す（CLAUDE.md「Retired tooling」に注記） |
| `crates/oracle`、`crates/fuzz` | workspace memberから除去（fuzzは6.0.3 oracleとの差分fuzzer） |
| `crates/xtask` | `codegen diagnostics`／`diagnostics-check`だけを残した（`diagnostics_codegen.rs`、legacy entryの生成なし）。nodes／enums／scanner codegenは入力（6.0.3の`typescript.d.ts`／`_tsc.js`）が消えたので削除し、生成済みsourceは手保守。`workspace sync`／`readme-status`もCI退役で削除（Cargo.tomlのprofile blockは手保守） |
| `crates/conformance` | `ts71.rs`とそのbinだけ。旧runner（families／goldens_diff／h0_memory／host_resolution／identity／ratchet／rendered／scope／shadow_diff）、`identity-vectors-v1.json`、依存（oracle／checker／host／toml_edit／zstd） |
| `crates/harness` | native（7.x）経路だけ：`upstream_suites.rs`は`OrderedSetting`／`CompilerLink`／`SourceEncoding`／`decode_source`に、`compiler.rs`は`makeUnitsFromTest`と directive scanに縮小。`execution.rs`から6.0.3 corpus（manifest／SourceCache／`load_compiler_no_emit`／emit floor／qualified emit／project suite）を削除し、`EmitOptionFloor`を廃して全directiveを無条件に投影（`load_native_compiler_program`が唯一の入口）。`lib.rs`のProgramJson（oracle driver入力）削除。h1／h2 integration test、`ratchets/pins`依存のtest削除 |
| `ratchets/` | `ts71/`以外（292 MB）。`pins/`、`goldens/`、`.github/ci/contracts`も |
| `ts-tests/` | 53 MB |
| `scripts/` | `conformance_ts71*.py`、`typescript7.py`、`vendor_typescript_native.py`、`benchmark-cli.py`（＋test）以外の全script（observe／check／witness／walk／pin等） |
| 6.0.3観測との比較test | `crates/compiler/tests/fixtures`（114 MB）と、それを読むcompiler test（top-level 32 file、integration 108 file）；programの`config_diagnostics_oracle_contract`／`h2_7d_bundle_source_facts`、node経由で6.0.3 bundleを呼ぶ`#[ignore]` test 5件；checkerの`emit`（H1 active-transform oracle、H2.5h foundation replay）・`node_builder_statements`／`syntactic_type_node_builder`のcompiler fixture test、`ts603_profile_*`；emitterの`active_transform_contract`／`printer_oracle_contract`／`declaration_printer_reprint_contract`／`comment_scope_witness_contract`／`comma_argument_factory_contract`、`helpers`（`_tsc.js`との文面照合）、source_mapのwitness replay、builtinsのcompiler fixture test；syntaxの`emitter-context-recovery`検査；CLI contractのtsc 6.0.3 parity test（`run_typescript`経由）とemit sessionのowner-controls test；参照されなくなったfixture 14 file |
| docs | `docs/witness-testing.md`削除、`docs/setup.md`を現行手順に書き直し、`docs/verification-status.md`の注記、`ratchets/README.md`、README（「Reference」節、`--version`、API、Run CI、limitations）、CLAUDE.md（intro、quick reference、Retired tooling） |

### 残したもの（判断基準）

削除したassetに依存しないtestは、6.0.3の観測を記録したfixture（`crates/emitter/tests/fixtures`、
`crates/syntax/tests/fixtures`、programの`h2-8b-*`、checkerの`tests/fixtures`）を読むものも含めて残した。
それらは今の出力に対するregression testとして通っており、7.1への追随で期待値が変わる時点で個別に更新・削除する。
適応したtest：`LibraryCatalog::typescript_6_0_3` → `typescript_7_1`（program contract 36箇所、compiler 3箇所）、
`ts-tests`／`vendor/typescript-6.0.3/lib`のpathをnative profileの同一内容のfileへ（checker unit 10箇所、
`preserve_symlinks_session_contract`）、`typescript_library_catalog_contract`はnative corpusの`mergeTwoInterfaces.ts`で
7.1 closure（ES2026 90、es2025 82、es2015 19、es5+dom 15）を検査、`config_option_catalog_contract`は7.1の`LibMap`
（115 entry）を検査。tsgoで再pinしたもの：`import_type_with_form_reports_only_the_resolution_row`（`with: {}`にTS1464なし）、
`jsdoc_import_tag_bare_with_reports_only_the_parser_diagnostic`（TS1463／1464なし、parserのTS1005のみ）。
P3-5へ送った差：import typeの`assert`形のTS2880は7.1ではparserが`assert` keyword（tsgo `a.ts(1,33)`、長さ6）に
報告する（`parser.go` `parseImportType`、import／export declarationも同様に`tryParseImportAttributes`／
`parseExportDeclaration`）が、tsc-rsはcheckerが6.0の位置（value側、`ignoreDeprecations`で抑止）に報告する。
TS2880 classの移行時にparserへ移し、checkerの3 site（`check.rs` import type、`modules.rs` declaration、
`calls.rs` import call＝tsgoは`checkImportCallExpression`で`assert` property名）を揃える。

### CI

`rust` jobは`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、
`cargo test --workspace`、`cargo xtask codegen diagnostics-check`になった（`.github/ci/replay.py`）。
harness crateに`[lints] workspace = true`を足した（`iter_over_hash_type`に合わせてsymlink alias列挙をsort順に）。

### 検証（local、最終bytes）

- `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`：clean。
- `cargo test --workspace --no-fail-fast`：70 target、3,613 passed、0 failed、0 ignored（node経由の`#[ignore]`
  oracle auditは削除済み）。checker unit 1,774、syntax 216、program 60（lib）＋500（contracts）、
  compiler 23（lib）＋143（contracts）。
- `cargo xtask codegen diagnostics-check`：`gen.rs`は生成器の出力と一致（legacy module削除後）。
- `python3 -m unittest discover -s .github/ci -p test_replay.py`：6 tests。`git diff --check`：clean。
  READMEのanchorと変更docsの相対linkを検査。
- 速度・peak memory（qbench、3 round、`nice -n 20`、`--noEmit`、median wall ms／peak RSS MiB、tsgo比）：
  hono 121.8（283）／154.9（325）0.79、zod 532.1（1,297）／896.1（1,793）0.59、Playwright 362.8（786）／560.9（1,037）0.65、
  TypeScript `src/compiler` 341.6（291）／345.2（408）0.99、Next.js 770.6（1,325）／1,344.5（1,636）0.57、
  Effect 533.5（1,037）／798.6（1,203）0.67、VS Code 3,477（5,477）／4,563（7,002）0.76。#615の比（0.76／0.57／0.61／
  0.95／0.59／0.70／0.75）と差はrun間のばらつきの範囲で、退行なし（P3-3が消したのはcold branchだけ）。
- hosted：PR #617（head `b27eb13bb`）、run 36909154991 — `plan` 35s、`rust`（`cargo test --workspace`）10m46s、
  `conformance (TypeScript 7.1)` 17m26s、`gates` 12s、すべて成功。

## P3-4 lane Bの7.1化（2026-10-02）

runnerのlane判定から`deprecated_option`（6.0で非推奨になったoptionを使う構成をlane Bへ送る自前の表）を外し、
Goのrule（`SkipUnsupportedCompilerOptions`＝skip、`failOnUnsupportedCompilerOptions`／`skippedTests`／未知の
directive＝not run）だけで決めるようにした。`Outcome::Deprecated`は`Outcome::Skipped { rule }`に、report／
supervisorの`deprecated`は`skipped`に改名。README「Run CI」の「比較しない構成」の説明をGoのruleの列挙に差し替え。
`DocumentRegistry::default()`のnamespace labelを`tsc-rs`に。

- 内訳（15,228 configuration）：lane A 13,467（＝Goの実行数 15,228 − skipped 1,719 − not run 42）、
  full 12,641、text 114、category 21、mismatch 646、harness error 45（変化なし）。lane Aに移ったのは
  `downlevelIteration`の23構成だけ（他の旧lane B 1,719はGoもskipする）。23件はすべてemit一致、errorsは
  tsc-rsがTS5101（6.0の非推奨）を、7.1がTS5102（削除済み）を報告する1行差で不一致。
- ratchet：0 regressions。`compiler/comparisonSameNamedAliases`のemit tierを`none`→`js`に上げた（#615の
  tsgo同形stable sortで一致するようになっていたが、#615は`--check`だけだった）。新規行なし（23件はmismatch）。
- 次（P3-5の最初のclass）：削除済みoptionの診断。tsgo `program.go` `verifyCompilerOptions`「Removed in TS7」は
  `baseUrl`（tsconfigがあれば`"paths": {"*": ["./…/*"]}`の`Use_0_instead`付き）、`outFile`、`target: ES5`、
  `module: AMD／System／UMD`、`moduleResolution: Classic／node10`、`alwaysStrict: false`、`esModuleInterop: false`、
  `allowSyntheticDefaultImports: false`、`downlevelIteration`（値を問わず）をTS5102／TS5108で報告し、
  `ignoreDeprecations`は検証も抑止もしない（TS5103／TS5101／TS5107は7.1に存在しない経路）。tsc-rsの
  `crates/program/src/config.rs`（tsconfig）と`crates/compiler/src/lib.rs`（programmatic／CLI）の6.0経路を
  これに置き換え、programのh2-8b config-diagnostics fixtureとREADME「Reference」の非推奨optionの説明を更新する。

## P3-5a 削除済みoptionの診断（2026-10-02）

tsgo `program.go` `verifyCompilerOptions` の「Removed in TS7」blockを移植し、6.0の非推奨経路
（TS5101／TS5107、`ignoreDeprecations`の検証TS5103と抑止、`aka.ms/ts6`のchain、`module: none`の行）を消した。

- tsconfig経路 `crates/program/src/config.rs` `removed_option_diagnostics`、programmatic／CLI経路
  `crates/compiler/src/lib.rs` `programmatic_option_diagnostics`：`baseUrl`（tsconfigがあれば
  `tspath.GetRelativePathFromFile`＋Goの`encoding/json`quoteで`Use '"paths": {"*": ["./…/*"]}' instead.`を
  chainに付ける。`removed_base_url_paths_suggestion`、`js_path::relative_path_from_directory`）、`outFile`、
  `downlevelIteration`（名前の位置、TS5102）、`target=ES5`、`module=AMD／System／UMD`、
  `moduleResolution=Classic／node10`、`alwaysStrict=false`、`esModuleInterop=false`、
  `allowSyntheticDefaultImports=false`（値の位置、TS5108）。`ignoreDeprecations`はcatalogに残り、値は読まれない。
- 位置の規則をtsgoに合わせた：`tsoptions.ForEachPropertyAssignment`は名前が一致する最初のpropertyで止まる。
  6.0.3（と旧tsc-rs）は重複keyや2名のrow（TS5052／5053／5069／5091…の`option1`／`option2`）を一致するproperty
  ごとに出していたが、7.1は文書順で最初の1件だけ。configの`emit_option_validation_diagnostic_for_properties`と
  programmaticの`push_programmatic_option_diagnostic`を1件に。
- 非致命の分類 `is_non_fatal_option_diagnostic`：5101／5107→5102／5108。tsgoは削除済みoptionの行を出しながら
  checkもemitもする（`tsgo -p`で`a.js`が書かれる）。CLIはtsgoと同じく、option診断があればsemantic診断を出さない。
- checkerの`assert`（TS2880）：`ignoreDeprecations: "6.0"`による抑止を外し無条件に（`calls.rs`のdynamic import
  option、`modules.rs`のimport／export attributes、`check.rs`のimport type）。報告位置はparserへ移していない
  （P3-5のTS2880 classのまま）。
- fixture：programの`h2-8b-config-diagnostics`（76 case）と`h2-8b-config-entity-names`（36 case）をtsc-rsの
  出力で記録し直し、tsgo（vendored commitのbuild）で検算した。方法：各caseのfile／configを一時dirに展開し
  `tsgo -p <config> --pretty false`、`.json`に位置する行とfileなしの行（5xxx／6xxx／18xxx、TS5011はprogram load
  の行なので除外）のfile／line／column／code／message chainを比較（lengthはtsgoが出力しない）。結果：
  entity-names 36/36（2 caseはmessage内の改行を検算scriptが切った見かけの差）、config-diagnostics 56/76。
  差の20 caseはすべて7.1が削除したoption／値に関わる行で、次のclass（P3-5b）に送る：
  - 7.1に無い関係row：`outFile`×`isolatedModules`／`verbatimModuleSyntax`／`declarationDir`（TS5053）と
    `outFile`＋commonjs（TS6082）、`verbatimModuleSyntax`＋AMD／UMD／System（TS5105）、`resolveJsonModule`＋classic
    （TS5070）／system（TS5071）、`resolvePackageJsonExports`／`Imports`／`customConditions`＋classic（TS5098）、
    node16／node18／node20／nodenext＋classic（TS5109）、`isolatedModules`＋`module: none`＋低target（TS5047）、
    複数`*`のpattern後のsubstitution型row（TS5064）。
  - 7.1のdefault `moduleResolution`はnode16／nodenext以外のすべてのmoduleでbundler：amd／umd／system／noneに
    TS5095（`Option 'bundler' can only be used when 'module' is set to 'preserve', 'commonjs', or 'es2015' or later.`）。
  - `module: none`と`target: es3`は7.1の値集合に無い（TS6046、listは`'commonjs', 'es6', 'es2015', 'es2020',
    'es2022', 'esnext', 'node16', 'node18', 'node20', 'nodenext', 'preserve'`／`'es6'…'es2025', 'esnext'`）。
  - 5.5で削除されたoption名（`charset`、`out`、`keyofStringsOnly`、`noImplicitUseStrict`、`noStrictGenericChecks`、
    `suppressExcessPropertyErrors`、`suppressImplicitAnyIndexErrors`、`importsNotUsedAsValues`、
    `preserveValueImports`）はTS5023 Unknown compiler option。tsc-rsのTS5102／5108 rowとCompilerOptionsの
    fieldは残置。
- test：programのloader／paths／option-validation、compilerのsession／emit／cli／filesystem、checkerのcallsを
  7.1の行に再pin。node10のauthoritative resolution testはnode10のまま、semantic行は`run_for_native_harness`の
  ungated union（`consume_ungated`）から読む（CLIのbucketはoption診断で閉じる）。
- conformance（release、`--workers 4 --check`）：15,228 configuration、lane A 13,467（変化なし）、full 12,641→12,666（+25）、text 114、category 21、mismatch 646→622、harness error 45→44、emit full 12,411。lane Aに上がったのはP3-4で予告した`downlevelIteration`の23構成（errorsがTS5102で一致）と`importAssertionsDeprecatedIgnored`（`@ignoreDeprecations: 6.0`が無効になりTS2880 3件が一致）の計24件。ratchet：0 regressions、`--update`相当（report記録）で24行追加。`compiler/intersectionConstructorReductionCrash`は今回fullだったが、P3-1以降の計測ではharness error（stress case、負荷で結果が変わる）だったので行を追加しない（安定したら追加）。
- hosted：PR #619（head `6ca8d017f`、merge `47c4bd309`）、run 36918572262 — `plan` 32s、`rust` 9m34s、`conformance (TypeScript 7.1)` 21m49s、`gates` 11s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、本branchのrelease build対tsgo 7.1.0-dev、median wall／peak RSSのtsc-rs÷tsgo）：hono 0.71／0.86、zod 0.57／0.72、Playwright 0.60／0.72、TypeScript `src/compiler` 0.98／0.71、Next.js 0.60／0.77、Effect 0.67／0.86、VS Code 0.75／0.79。#615後の計測と同じ帯（READMEの比率＋記録済みのstable ordering 2〜6%）で、peak memoryは同等以下。checkerのhot pathに触れない変更なので退行なし。

## P3-5b option catalogと関係rowの7.1化（2026-10-02）

tsgo（vendored commit `19dadef8`で`scripts/typescript7.py build`したもの。PINを`1f70213d`から更新、Go toolchainは
go.workの要求どおりgo1.27.1）でprobeし、option catalog・計算値・関係rowを7.1に合わせた。

- catalog（`crates/program/src/config_options.rs`）：5.5で削除された9 option（`charset`、`out`、`keyofStringsOnly`、
  `noImplicitUseStrict`、`noStrictGenericChecks`、`suppressExcessPropertyErrors`、`suppressImplicitAnyIndexErrors`、
  `importsNotUsedAsValues`、`preserveValueImports`）を外した（tsgo同様TS5023 Unknown compiler option、
  suggestionなし）。`CompilerOptions`のfieldも削除（checker／emitterは読んでいなかった。harness directiveは
  unknown→not run、Goの`SetOptionsFromTestConfig`も`Fatalf`）。`module: none`と`target: es3`を値集合から外した
  （TS6046。list文面はP2-1の`config_named_option_choices`のまま一致）。`moduleResolution`の値順をtsgoの
  `moduleResolutionOptionMap`順に、`stableTypeOrdering`の宣言位置を`alwaysStrict`の次に（tsgoの宣言順）。
- 計算値（`crates/types/src/options.rs`）：`emit_module_kind`は`Some(0)`を未指定扱い（tsgo `GetEmitModuleKind`は
  `ModuleKindNone`＝未指定）。`emit_module_resolution_kind`の既定はnode16／nodenext以外すべてbundler
  （6.0はamd／umd／system／noneをclassicにしていた）。明示のclassic／node10は従来どおりそのresolverを選ぶ
  （tsgoは`GetModuleResolutionKind`で既定へ写像する——下記P3-5c）。
- 関係row（`option_validation.rs`、emitterの`plan.rs`の重複も）：7.1に無い`outFile`×`isolatedModules`／
  `verbatimModuleSyntax`／`declarationDir`（TS5053）、`outFile`＋非amd/system（TS6082）、`verbatimModuleSyntax`＋
  AMD/UMD/System（TS5105）、`resolveJsonModule`＋classic（TS5070）／none・system・umd（TS5071）、`isolatedModules`＋
  `module: none`（TS5047）を削除。`paths`の非文字列substitutionはtsgoが変換時に落とすので行を出さず、全要素が
  非文字列ならTS5066（空配列）。`lib`×`noLib`も文書順で最初のproperty 1件に。
- fixture：`h2-8b-config-diagnostics`を再記録しvendored commitのtsgoで検算（方法はP3-5a）：56→63/76。
  entity-names 36/36（P3-5aと同じ見かけの差2件）。残り13 caseは全てP3-5c（明示classic／node10の写像）：
  classic＋`resolvePackageJsonExports`／`Imports`／`customConditions`のTS5098 3件、node16〜nodenext＋classicの
  TS5109 4件、amd／system＋`moduleResolution: node`でtsgoだけが出すTS5095 6件。
- test：program（catalog順、loader、paths 5064→5066、scaling、module_request）、compiler（session／emit）、
  emitter（builtins module 0、bundle printer fixtureの`module: none` caseはskip、output plan）を7.1に再pin。
- 明示`classic`／`node10`をtsgoどおり既定へ写像する件は、resolverのclassic／node10経路が死に
  `module_resolution_contract`等のnode10前提のtest（91箇所）の再pin／削除が要り、conformanceの利得がない
  （harnessがその構成をskipする）ので、P3-6（旧resolver退役）に送る。P3-5はconformanceのclassを続ける。
- conformance（release、`--workers 4 --check`）：15,228 configuration、lane A 13,467（変化なし）、full 12,666→12,671、text 114、category 21、mismatch 622→616、harness error 44→45（`compiler/intersectionConstructorReductionCrash`がP3-5a時のfullからharness errorへ戻った。負荷依存のstress caseでratchet行は入れていない）、emit full 12,411→12,410（同じcase）。fullに上がったのは`compiler/deprecatedCompilerOptions1`〜`6`の6構成（tsconfigの削除済み／5.5削除optionの行がTS5023等で一致、各8行）。ratchet：0 regressions、6行追加。
- hosted：PR #620（head `06ce2a074`、merge `df5b75156`）、run 36926416348 — `plan` 31s、`rust` 9m54s、`conformance (TypeScript 7.1)` 17m12s、`gates` 12s。
- perf（README corpora、`--noEmit`、nice 20、release build対tsgo 7.1.0-dev）：3 roundsの初回計測ではhono 0.82／TypeScript `src/compiler` 1.08がP3-5a（0.71／0.98）より悪く見えたが、main（P3-5a、`4b2c383cb`）と本branchのbinaryを同条件で5 rounds A/Bすると同値（median wall ms、main→本branch：hono 116→116、`src/compiler` 323→326、zod 514→511；peak RSS MB 310→289、291→292、1301→1300）で、tsgo比はhono 0.73、`src/compiler` 0.95、zod 0.56。初回の差は計測ノイズ（release build直後）で、退行なし。他のcorporaは初回計測でP3-5aと同じ帯（Playwright 0.59、Next.js 0.59、Effect 0.65、VS Code 0.76；peak memory同等以下）。

## P3-5c 未使用type parameterの診断（TS6196、2026-10-02）

P3-5bまでのreport（mismatch 616）を最初の差のcodeで集計すると、(missing TS6196, unexpected TS6133)が26構成で最大の
単独classだった（次点：TS2683 24、TS2300 23、TS2339 20、TS2309 18、TS2304 17、TS2749 17、TS6504 14、TS2303 12、
(TS2769,TS2769) 20、(TS2552,TS2304) 13）。tsgo `checker.go` `checkUnusedTypeParameters`／`checkUnusedInferTypeParameter`
（vendored commit）に合わせた：

- 未使用のtype parameterはTS6196「'{0}' is declared but never used.」をそのtype parameter node（名前＋constraint／
  default）に出す。6.0.3は値と同じTS6133で、listに1つだけのときは`<T>`の範囲に出していた。
- listに2つ以上あり全部未使用ならTS6205「All type parameters are unused.」を`<…>`の範囲に（6.0.3と同じ）。
- `infer U`はTS6196を名前に（6.0.3は`infer U`全体にTS6133）。
- 検査対象は「symbolの全declarationが同じfileにある」宣言すべて（tsgo `allDeclarationsInSameSourceFile`）。6.0.3は
  最後のdeclarationだけだった。mergeされたinterfaceのtype parameterは1つのsymbolを共有するのでどちらかの使用で
  使用済み、overloadのtype parameterは各宣言ごと（tsgo probe：`c.ts`で確認）。
- gateは従来どおり`noUnusedParameters`（`UnusedKindParameter`）。
- JSの`@template`で全部未使用のときのTS6205の範囲は、tsgoが`@template`群を1つのlistに再parseするので
  「最初のtagの位置−1」から「最後のparameterの後のtriviaを飛ばした位置＋1」まで。
- unit test（`crates/checker/tests/unit/unused/tests.rs`の4本と、type parameter行を付随して固定していた
  calls／functions／libの9本）をtsgoの行に再pin。

同じPRで、harnessのroot選択も直した（最初の差がTS6504 14構成／TS6054 7構成のclass）：Goのrunner
（`harnessutil.CompileFilesEx`）は`.json`と`.tsbuildinfo`以外の全unitをprogramのrootに渡し、allowJsなしのJS rootや
未対応拡張子のrootはprogramがTS6504／TS6054（「The file is in the program because: Root file specified for
compilation」のchain付き）で報告する。tsc-rsのharnessは旧tscの`isSupportedSourceFileName`でroot候補を落としていた
（`supported_compiler_roots`）ので、Goと同じ除外だけにした。対象の21構成（`checkJsFiles6`、
`jsFileCompilationWithoutJsExtensions`、`privateIdentifierPropertyAccessDestructuringAssignmentES6`、
`bundlerConditionsExcludesNode`×2、`bundlerNodeModules1`×2、`nodeModulesAtTypesPriority`、
`resolvesWithoutExportsDiagnostic1`×2、`nodeModulesExportsBlocksTypesVersions`×4、`jsFileCompilationWithMapFileAsJs*`×3、
`bundlerImportTsExtensions`×4）は全てfull（loaderの`load_root`が既にchain付きのTS6504／TS6054を出していた）。
- conformance：15,228 configuration、lane A 13,467（変化なし）、full 12,671→12,719（+48）、text 114、category 21、mismatch 616→568、harness error 45、emit full 12,410。fullに上がったのは未使用type parameterの27構成（上記26＋`unusedTypeParameters8`）とharness root選択の21構成。ratchet：0 regressions、48行追加。`unusedTypeParameters_templateTag2`は残る（C1／C3の`/** @type {T} */ this.p;`を7.1は宣言と見ず、TS2339とTS6205になる——JSのexpando／`this` class）。
- hosted：PR #621（head `095b95311`、merge `675452004`）、run 36933021735 — `plan` 25s、`rust` 9m28s、`conformance (TypeScript 7.1)` 21m57s、`gates` 11s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `68043e837`と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 121→120、zod 529→518、Playwright 345→341、TypeScript `src/compiler` 355→345、Next.js 781→789、Effect 545→514、VS Code 3,472→3,503。tsc-rs÷tsgoは0.57〜0.98で従来どおり、peak memoryは同等（MB main→本branch：307→306、1,298→1,304、759→745、291→292、1,329→1,332、1,049→1,039、5,500→5,473）。退行なし。

## P3-5d did-you-mean suggestionの上限撤廃（2026-10-02）

P3-5c後の集計で(missing TS2552, unexpected TS2304)が13構成（`commonMissingSemicolons`、
`maximum10SpellingSuggestions`、`parserharness`、`parserindenter`、`parserRealSource11`…）。tsc 6.0は
`maximumSuggestionCount`＝10でchecker全体のDid-you-mean候補計算を打ち切っていた（tsc-rsは`suggestion_count`と
`MAXIMUM_SUGGESTION_COUNT`で移植し、noLibのinit probeの消費や推測rollbackでの復元まで模していた）が、tsgoには上限が
無い（`onFailedToResolveSymbol`、`maximum10SpellingSuggestions`の7.1 baselineは12個全部にsuggestion）。
`suggestion_count`／`MAXIMUM_SUGGESTION_COUNT`と関連state（speculation checkpoint、init probeの消費）を削除し、
annotate／speculateのunit testから上限の観測を外した。noLibで上限の挙動をpinしていたaccessのunit test 4件はtsgoの行に再pin（`tsc-19dadef8`を`--noLib`＋global型の宣言で実行：`helo`→TS2552 "Did you mean 'hello'?"、近似名3件はすべてTS2552、候補の無い名前はTS2304のままで後続のsuggestionに影響しない）。facts／spell／calls／speculateのbudget言及コメントも書き換えた。
- conformance：15,228 configuration、lane A 13,467（変化なし）、full 12,719→12,731（+12）、text 114、category 21→22、mismatch 568→555、harness error 45、emit full 12,410。(missing TS2552, unexpected TS2304)の13構成がすべて上がった：fullが12（`commonMissingSemicolons`、`maximum10SpellingSuggestions`、`parserindenter`、`parserRealSource6`〜`9`／`11`〜`13`、`parserS7.6_A4.2_T1`、`scannerS7.6_A4.2_T1`）、`parserharness`はcategory（位置・code・categoryは一致し、残る差はTS6053 "File '{0}' not found."の引数——tsgoは`/// <reference path="../compiler/io.ts" />`の記述どおり`'../compiler/io.ts'`、tsc-rsは解決後の`'/compiler/io.ts'`。別classとして次の集計に残す）。ratchet：0 regressions、13行追加。
- hosted：PR #622（head `1a4df1aa5`、merge `32a9f718f`）、run 36938033235 — `plan` 23s、`rust` 9m43s、`conformance (TypeScript 7.1)` 21m50s、`gates` 12s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `7d10f2141`と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 121→129、zod 523→515、Playwright 342→356、TypeScript `src/compiler` 359→341、Next.js 810→770、Effect 546→524、VS Code 3,482→3,505。tsc-rs÷tsgoは0.57〜0.99で従来どおり、peak memoryは同等（MB main→本branch：290→301、1,302→1,302、747→769、292→292、1,326→1,336、1,044→1,041、5,484→5,482）。honoとPlaywrightのmedian差は同条件のA/Bで再計測：5 roundsでPlaywright 324→324、hono 116→122（CPU時間は同じ）、binaryの順序を入れ替えた10 roundsでhono main 116／本branch 117（min 112／113）となり、最初の差は計測順序のノイズ。退行なし。

## P3-5e 循環import aliasのTS2303（2026-10-02）

P3-5d後の集計でmissing TS2303が12構成（`circular1`／`circular3`、`recursiveExportAssignmentAndFindAliasedType1`〜`6`、
`declarationEmitUnknownImport`／`2`、`circularModuleImports`、`exportAsNamespaceConflict`）。tsc 6.0の`resolveAlias`は
`resolvingSymbol` sentinelで循環を検出し、内側のaliasを黙ってunknownに潰して最外のalias 1件だけにTS2303を報告した。tsgoの
`resolveAlias`（checker.go:16585-16611）はalias targetを型解決stackのproperty `AliasTarget`として`pushTypeResolution`／
`popTypeResolution`で解決するので、循環の再入はslotを書かずにunknownSymbolを返し、循環に属するすべてのaliasが自分のframeの
popでTS2303を報告する（`import self = require(...)`と`export = self`は別々のaliasなので、ambient module 2つの循環で4行、
3つで6行）。`try_resolve_alias`も`findResolutionCycleStartIndex(symbol, AliasTarget)`で判定する（checker.go:16622-16628）。
tsc-rsでは`TypeSystemPropertyName::ALIAS_TARGET`を追加し、`resolve_alias`／`try_resolve_alias`をこの形に置き換え、
sentinel専用だった`revert_symbol_alias_target`を削除した。`get_target_of_*`の構造（6.0の`dontRecursivelyResolve`引数）は
変えない：tsgoは`getTargetOfExportSpecifier`等を即時targetで止めて`resolveIndirectionAlias`で再帰するが、再帰はどちらも
`resolve_alias`のframeを通るので循環の報告は同じになる。modulesのunit test（`circular_import_alias_reports_2303`）を
tsgoの2行に再pinし、ambient moduleの循環（4行）と`export type { A } from`の2 file循環（`circular1`、2行）を追加した
（`tsc-19dadef8`で確認）。TS2303の引数はtsgoの`symbolToString`＝`getNameOfSymbolAsWritten`（nodebuilderimpl.go:973-1025）で、
`export = self`／`export default Foo`のaliasは式のidentifier（`'self'`／`'Foo'`）を出す。6.0移植はescaped name
（`'export='`／`'default'`）を出していたので、alias用の`alias_name_as_written`（最初の名前付き宣言の名前、無ければsymbol name）を
加えてそれに合わせた（`recursiveExportAssignmentAndFindAliasedType*`6構成、`exportAsNamespaceConflict`、
`declarationEmitUnknownImport2`がcategoryで止まっていた原因）。
- conformance：15,228 configuration、lane A 13,467（変化なし）、full 12,731→12,743（+12）、text 114、category 22、mismatch 555→543、harness error 45、emit full 12,410。fullに上がったのはTS2303 classの12構成すべて（`circular1`／`circular3`、`recursiveExportAssignmentAndFindAliasedType1`〜`6`、`declarationEmitUnknownImport`／`2`（target=es2015）、`circularModuleImports`、`exportAsNamespaceConflict`）。ratchet：0 regressions、12行追加。
- hosted：PR #623（head `ac2a31657`、merge `f946e0cf8`）、run 36942229574 — `plan` 31s、`rust` 9m47s、`conformance (TypeScript 7.1)` 21m45s、`gates` 12s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `02d078f30`と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 121→119、zod 528→524、Playwright 340→350（min 337→330）、TypeScript `src/compiler` 355→346、Next.js 826→792、Effect 546→506、VS Code 3,480→3,513。tsc-rs÷tsgoは0.59〜1.01で従来どおり、peak memoryは同等（MB main→本branch：284→312（honoは日中の計測でも284〜312の幅）、1,302→1,294、760→760、292→292、1,325→1,328、1,051→1,032、5,478→5,484）。退行なし。

## P3-5f JavaScriptのvalue-as-type fallback撤廃（TS2749／TS2503）（2026-10-02）

P3-5e後の集計でmissing TS2749が17構成（`jsdocTypeReferenceToValue`、`typeFromPropertyAssignment`／`2`／`3`／`40`、
`jsDeclarationsEnumTag`、`jsEnumTagOnObjectFrozen`、`enumTagUseBeforeDefCrash`、`exportedEnumTypeAndValue`、
`jsdocTypeReferenceExports`／`ToImport`、`commonJSImportNestedClassTypeReference`、
`prototypePropertyAssignmentMergedTypeReference`、`typeTagCircularReferenceOnConstructorFunction`、
`jsDeclarationsJSDocRedirectedLookups`、`jsDeclarationsReferenceToClassInstanceCrossFile`、
`jsdocTypeNongenericInstantiationAttempt`）。tsc 6.0の`getTypeFromTypeReference`はJSDocの型参照を
`Type`で引けないとき`Type|Value`で引き直し、`getTypeReferenceType`が`getExpandoSymbol`（初期化子のclass／function faceの
merge）と`getTypeFromJSDocValueReference`（値の型をそのまま型として使う）で値を型に変換していた。tsgoの
`getTypeFromTypeReference`（checker.go:23425-23438）は`getIntendedTypeFromJSDocTypeReference`の後、
`getSymbolFromTypeReference`（meaning `Type`のみ）→`getTypeReferenceType`で、後者にはexpando mergeも値の変換も無く
（`// !!! Resolving values as types for JS`でerrorType）、JSDocで値を参照するとTypeScriptと同じTS2749
「'X' refers to a value, but is being used as a type here.」（qualified nameの左側はTS2503）になる。tsc-rsは
`get_type_from_type_reference`の`Type|Value`再解決と`get_type_reference_type`のexpando merge／JS value armを削り、
`get_expando_symbol`／`get_type_from_jsdoc_value_reference`を削除した。qualified nameの左側は6.0がJavaScriptでは
`Namespace|Value`で引いていたが、tsgoの`resolveQualifiedName`は`Namespace`だけで引く（`@type {NS.Inner}`の`NS`が
変数ならTS2503「Cannot find namespace」）ので`resolve_entity_name`の`namespace_meaning`もそれに合わせた。6.0の
`resolveEntityNameFromAssignmentDeclaration`（JSDoc型参照の二次location解決）はtsgoに無いが、tsc-rsではJSDoc
`@template`の既定値や`@overload`のtype parameter解決がこれに依っている（外すと`jsdocTemplateTagDefault`／
`jsdocVariadicInOverload`がTS2304に退行）ため残す——tsgoは`resolveName`のcontainer walkで解くので、JSDoc scopeの
移植時に外す。もう1つ、tsc-rs固有の「JavaScriptの全meaning shield」（`on_failed_to_resolve_symbol`で、JSでは名前が
別のmeaningで存在すれば未解決エラーを出さない——binderのvalue／namespace mergeの未実装を隠すための逸脱）が
`@type {NS.Inner}`のTS2503を飲み込んでいたので外し、`check_and_report_error_for_using_type_as_namespace`の
JS `Namespace|Value`比較も`Namespace`にした（tsgoにはどちらも無い）。jsdoc 458／salsa 199／checkJs 77／commonjs 24
構成のfilterで退行0。P3-5dで残っていた`resolve.rs`のsuggestion budgetコメントも書き換えた。unit testはtsgoの行に
再pin（`tsc-19dadef8`で確認）：check
（`jsdoc_value_references_report_2749_and_2503_like_tsgo`：TS2749＋TS2503；declaration emitのreuse testのJS側は
`(x: V | string)`と値の名前で表示）、functions（`@type {Self}`にTS8030がもう1件）、modules（accessed requireの
`NestedK`はTS2749で面を持たない）、unused（`import("./MC")`の値は型にならず（tsgo TS1340）cross-file登録が無い）、
compilerの`jsdoc_import_resolution_mode_overrides_select_distinct_rows`（`@import`した値を`@returns`の型に使っていた
のでfixtureを`export type`に）。
残る差はbinderのclass（tsgoのbinderは`// !!! constructor functions`で未実装）：JavaScriptのconstructor function
（`function MyClass() {}`＋`MyClass.prototype = …`、`f.prototype.x = …`）や`require`で束縛したclassにtsc-rsのbinderは
class faceを与えるので`@type {MyClass}`が型として解決してTS2749が出ない（`typeTagCircularReferenceOnConstructorFunction`、
`prototypePropertyAssignmentMergedTypeReference`、`jsDeclarationsReferenceToClassInstanceCrossFile`）、`var Outer = {}`＋
`Outer.Inner = …`のexpando（`Outer`にnamespace faceが付きTS2709／`Outer.Inner`のTS2749になるが、tsgoは素のvarとして
TS2749／TS2503：`typeFromPropertyAssignment*`）、`@enum` tag（tsc-rsはtype aliasとしてbindするがtsgoは値のまま：
`jsDeclarationsEnumTag`、`jsEnumTagOnObjectFrozen`、`enumTagUseBeforeDefCrash`、`exportedEnumTypeAndValue`）。
- conformance：15,228 configuration、lane A 13,467（変化なし）、full 12,743→12,750（+7）、text 114、category 22、mismatch 543→536、harness error 45、emit full 12,410。fullに上がったのはTS2749 classの4構成（`jsdocTypeReferenceToValue`、`jsdocTypeNongenericInstantiationAttempt`、`jsDeclarationsJSDocRedirectedLookups`、`commonJSImportNestedClassTypeReference`）とshield撤廃で揃った3構成（`uniqueSymbolJs`、`importTag17`、`typeLookupInIIFE`）。TS2749 classの残り13構成は上記binder class。ratchet：0 regressions、7行追加。
- hosted：PR #624（head `669dc1e83`、merge `80ec90601`）、run 36948318404 — `plan` 25s、`rust` 6m4s、`conformance (TypeScript 7.1)` 17m32s、`gates` 11s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `9070f5886`と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 124→125、zod 567→569、Playwright 366→366、TypeScript `src/compiler` 371→347、Next.js 831→784、Effect 548→570、VS Code 3,848→3,833（この回はtsgoを含め全体が朝の計測より5〜8%遅く、machineの負荷差）。tsc-rs÷tsgoは0.59〜0.94で従来どおり、peak memoryは同等（MB main→本branch：304→284、1,296→1,304、756→757、291→292、1,329→1,330、1,037→1,034、5,480→5,473）。Effectのmedian差は同条件の5 rounds A/B（本branch 508／main 520、min 476／513）で本branchの方が速く、ノイズ。退行なし。

## P3-5g overload失敗の報告位置とunused宣言の報告位置（2026-10-02）

P3-5f後の集計で(missing TS2769, unexpected TS2769)が20構成、(missing TS6133, unexpected TS6133)が8構成——どちらも位置だけの差。
tsc 6.0の`reportCallResolutionErrors`は失敗候補が2〜3個なら各候補を「Overload N of M」のchainで再評価して
1つのTS2769にまとめ（全部が同じspanならそこ、違えばcalleeのerror node）、1個または4個以上なら最後の候補のエラーをそのまま
（4個以上は「No overload matches this call」→「The last overload gave the following error」のprefix付き）報告した。tsgoの
`reportCallResolutionErrors`（checker.go:9850-9870）は常に最後の失敗候補の適用エラーをそれぞれ別のdiagnosticとして報告し
（配列literalの要素ごとのエラーなら要素ごとにTS2769）、候補が2個以上ならprefix 2段を付けて「The last overload is declared
here」をrelatedに、さらに`addImplementationSuccessElaboration`。tsc-rsの`report_call_resolution_failure`をこの形にした
（`append_to_linear_tail`は不要）。unusedは、tsgoの`reportUnusedImports`が「All imports in import declaration are unused」を
宣言名が2つ以上で全部未使用のときだけ出し、それ以外は各importを名前の位置で報告し、`reportUnusedBindingElements`が
「All destructured elements are unused」を要素2つ以上で全部未参照のときだけ出し、それ以外は各要素を名前の位置で報告する
（6.0は単独の未使用importを宣言全体、単独要素のpatternをpattern位置やvariable宣言のrowにまとめていた）。unused.rsの
importとdestructuringのarmをそれに合わせた。
新しい「The last overload is declared here」のrelated行で、conformance runnerの`.errors.txt`描画が`/.lib/`のtest library
（`react18.d.ts`）内の位置を`1:1`に落としていた（position indexがfixture fileだけだった）ので、fixtureが`/.lib/`に
言及するときはharnessの`read_test_library`（`load_native_compiler_program`の内側から公開関数へ）でtest libraryの
textもindexに入れ、native runnerと同じ`react18/react18.d.ts:478:9`を出すようにした。
- conformance：15,228 configuration、lane A 13,467（変化なし）、full 12,750→12,807（+57）、text 114→83、category 22、mismatch 536→510、harness error 45、emit full 12,410。上がったのは61構成（overload失敗のcompiler／conformance case、`unusedImports*`／`unusedDestructuringParameters`、JSX children、tagged template、union signatureなど）：57がfull、4がtext。ratchet：0 regressions、26行追加・35行をtext→fullに上げた。local full runは今回から`--workers 2`（798 s）。
- hosted：PR #625（head `f07421dcf`、merge `1afe0317b`）、run 36953157844 — `plan` 27s、`rust` 7m20s、`conformance (TypeScript 7.1)` 17m2s、`gates` 12s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `0985315a7`と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 126→129、zod 548→565、Playwright 371→371、TypeScript `src/compiler` 356→360、Next.js 828→818、Effect 545→557、VS Code 3,685→3,712（この回もtsgoを含め全体が朝の計測より遅く、machineの負荷差）。tsc-rs÷tsgoは0.58〜1.01で従来どおり、peak memoryは同等（MB main→本branch：287→286、1,298→1,299、742→759、292→292、1,331→1,330、1,039→1,035、5,484→5,479）。honoとzodのmedian差は同条件の5 rounds A/B（hono 本branch 118／main 120、zod 508／515）で本branchの方が速く、ノイズ。退行なし。

## P3-5h import attributesの文法row（2026-10-02）

P3-5g後の集計でunexpected TS2857が8、TS2823が6、TS2856が6構成（計20、すべてtype-only import／exportまたはimport typeに
attributesが付くもの）、(missing TS2880, unexpected TS2880)が6構成。tsc 6.0の`checkImportAttributes`はtype-onlyでも
resolution-mode override以外の文法row（module optionのTS2823、CommonJS requireのTS2856、type-onlyのTS2857）を報告した
が、tsgoの`checkImportAttributes`（checker.go:5537-5565）は`isExclusivelyTypeOnlyImportOrExport || isImportTypeNode`なら
overrideの検査だけで戻る。tsc-rsも同じ早期returnにした。TS2880「Import assertions have been replaced by import
attributes」は、tsgoではparserが`assert` keywordの位置に出す（parser.go:2552／2619／3093）。import／export宣言はtsc-rsも
attributesの先頭token＝`assert`で一致するが、import typeは`{ assert: {...} }`の内側の`{`に出していた（6構成）ので、
TS2880をparserへ移した：`parse_import_attributes`（宣言、keyword位置）と`parse_import_type`（`{`の次の`assert`）で
report-only originの`parse_deprecation_at_current_token`で報告し、checkerの`check_import_type`／
`check_import_attributes`の2880 armは削除。dynamic `import()` callの`{ assert: … }`はtsgoもcheckerがproperty名に
出すが、tsgoは通常の`error`なので（6.0移植はgrammar error＝fileにparse diagnosticがあると抑制）`error_at`に変えた。
parse diagnosticが付くとtsc-rsのemit preflightはrecovery未対応として出力を止めていた（`importAssertionsDeprecated`の
emit baselineが退行）ので、`ParseDiagnosticOrigin::Deprecation`（木は完全で何も補わない）をliteral-only扱いにして
tsgoと同じくemitする。tsgoと同じくparse errorになるのでCLIでは`assert`を含むfileのsemantic rowが抑制される。unit test：parser（`import_assertions_report_2880_at_the_keyword`、
offset 20／70）を追加、checkのreuse testは`with`形に、libのCommonJS優先testはtype-only側を「rowなし」に再pin、
statementsの注記を更新。
- conformance：15,228 configuration、lane A 13,467（変化なし）、full 12,807→12,832（+25）、text 83、category 22、mismatch 510→486、harness error 45→44、emit full 12,410→12,411（増分はload依存の`intersectionConstructorReductionCrash`がこの回fullになったもので、P3-5aの判断どおりratchetには載せない）。上がったのはimport attributes classの20構成（`importAssertion3`／`importAttributes3`／`importTag15`の各module、`nodeModulesJson`、`nodeModulesImportAttributesModeDeclarationEmitErrors`、`nodeModulesImportModeDeclarationEmitErrors1`、`nodeModulesImportTypeModeDeclarationEmitErrors1`の各module）とTS2880 classの`importTypeAssertionDeprecation`／`Ignored`（`nodeModulesImportTypeModeDeclarationEmitErrors1`の4構成は両classに属する）。ratchet：0 regressions、24行追加。localの検証は2026-10-02のlocal-load方針どおり：fmt、syntax／checker／compiler／conformance crateのclippyとtest、release build、2 workerのfull run 1回（794 s）。
- hosted：PR #626（head `e48700253`、merge `6374743d8`）。最初のhead `c3c8ace4e`のrun 36956869370は`rust`が失敗：emitterの`import_type_attributes_contract`のfixtureが`assert` caseのnode flagをparse errorなしで記録していた（import typeとattributesは今parse-error flagを持つ；flagだけの差でemit hookとprintは同じ）。localの軽量chainはsyntax／checker／compiler／conformanceだけでemitterのtestを回していなかった。fixtureを再pinした`e48700253`のrun 36958330916 — `plan` 34s、`rust` 9m32s、`conformance (TypeScript 7.1)` 22m11s、`gates` 13s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `5f7b51da6`と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 123→120、zod 519→521、Playwright 357→352、TypeScript `src/compiler` 351→349、Next.js 787→765、Effect 528→536（CPU時間は3,369→3,325 msで減、run間の幅の内）、VS Code 3,508→3,504。tsc-rs÷tsgoは0.59〜0.99で従来どおり、peak memoryは同等（MB main→本branch：306→311、1,290→1,302、753→753、292→292、1,329→1,339、1,040→1,021、5,466→5,474）。退行なし。

## P3-5i `@pretty` baselineの比較とmember重複宣言の報告（2026-10-02）

P3-5h後の集計で、`@pretty: true`の14 case（`duplicateIdentifierRelatedSpans1`〜`7`、
`esModuleInteropPrettyErrorRelatedInformation`、`manyCompilerErrorsInTheTwoFiles`、`multiLineContextDiagnosticWithPretty`、
`prettyContextNotDebugAssertion`、`prettyFileWithErrorsAndTabs`、`deeplyNestedAssignabilityIssue`、`typedefCrossModule5`）が
すべてmismatchだった。native runnerの`@pretty` baselineはANSI色付きの`file:line:col - category TScode: text`形式
（`FormatDiagnosticsWithColorAndContext`：code frame、chain行、related行、末尾の`Found N errors`）で、runnerの
`parse_errors_baseline`は要約形式しか読めず期待行が0になり、tsc-rsの行がすべて「unexpected」になっていた。
ANSI escapeを外してheader行だけを読む`parse_pretty_errors_baseline`を加えた（Full tierは従来どおり`@pretty`では評価しない）。
`@pretty`の10構成は期待行が読めるようになり、text tierに上がった（Full tierは`@pretty`では評価しない）。
もう1つ、missing TS2300の23構成（`duplicateClassElements`、`numericClassMembers1`、`numericNamedPropertyDuplicates`、
`stringNamedPropertyDuplicates`、`objectTypeWithDuplicateNumericProperty`、`constructorParameterProperties2`、
`parameterPropertyInConstructor2`、`symbolProperty37`／`44`、`duplicatePropertyNames`…）。scannerは数値名を正規化済みで、差は
報告の形：tsgoの`checkObjectTypeForDuplicateDeclarations`（checker.go:3190-3259、class／interface／type literal共通）は
member名ごとの状態（1＝property、2＝accessor、3＝報告済み）で、2つ目のpropertyかpropertyとaccessorの併存を見つけると
`reportDuplicateMemberErrors`でその名前の全memberに報告し（引数はsymbolの書かれた名前：`0`と`0.0`なら`'0'`）、parameter
propertyはinstance propertyとして数え、private名のinstance／static共用はclass bodyだけで報告する。tsc-rsの6.0移植は
class用とinterface／type literal用の2関数で、後の宣言だけにその宣言自身のtext（`''0''`）で報告していた。tsgoの1関数に
置き換え（引数はmerge.rsの`symbol_name_as_written`＝getNameOfSymbolAsWrittenの移植）、`checkClassForDuplicateDeclarations`は削除。
late-boundのmember（computed nameが定数で解決するもの）もtsgoの`lateBindMember`に合わせた：衝突するflagを持つ
late-bound memberは名前の全宣言にTS2300（6.0はTS2733「was also declared here」とTS2718「Duplicate property」）、
classも対象（6.0はclassを`checkClassForDuplicateDeclarations`に任せて除外していた）、accessor同士でないmemberとの衝突で
late symbolが両accessor flagを得る。tsgoで確かめた例：`interface I { [k]: number; [k](): void; [k]: boolean; }`は
TS2300 'x'×2、TS2300 '[k]'×3、TS2717。unit testの再pin：late binding 2件、classの空文字列member（tsgoは`""`同士も
TS2300、6.0は報告しなかった）。
binderの除外maskもtsgoに合わせた（4つだけが6.0と違う）：PropertyExcludes＝Value & ~(Property | Accessor)（6.0はNone）、
GetAccessorExcludes／SetAccessorExcludesからPropertyを外し、auto-accessorのAccessorExcludes＝Value & ~Property
（6.0はValue & ~Accessor）。propertyとaccessorの併存はbinderではなく宣言ごとの`checkObjectTypeForDuplicateDeclarations`
が見るので、別々のinterface宣言やclass＋interfaceにまたがるmerge（`propertyAndAccessorMerging`）は報告しない。
新しいPropertyExcludesはvariableとmethodも除外するので、tsgoが同時に入れた2つの例外も移植した：binderの
`declareSymbol`はassignment宣言とvariableのmergeを両方向で許し（6.0はvariableが後のときだけ；TSのexpando
`f.p = …`と`namespace f { export const p }`の組でTS2300／TS2451が出ていた）、`getExcludedSymbolFlags`は
methodに置き換えられうるJavaScriptの`this[sym] = …`からMethodを外す（`[sym]()`との組でlate-boundのTS2300が
出ていた）。最初のfull runで退行した`expandoFunctionNestedAssigmentsDeclared`、
`jsDeclarationEmitThisAssignmentDuplicatingMethod`、`lateBoundMethodNameAssigmentJS`、`typeFromPropertyAssignment31`が
その2つ。
- conformance：15,228 configuration、lane A 13,467（変化なし）、full 12,832→12,864（+32）、text 83→93、category 22→21、mismatch 486→445、harness error 44、emit full 12,411→12,412。上がったのは43構成：重複宣言classの32構成がfull（`duplicateClassElements`、`numericClassMembers1`、`numericNamedPropertyDuplicates`、`propertyAndAccessorMerging`、`constructorParameterProperties2`、`privateNameDuplicateField`、`autoAccessor11`、`objectLiteralErrors`…）、`@pretty`の10構成がtext、1構成がcategory→full。ratchet：0 regressions、42行追加・1行raise（`intersectionConstructorReductionCrash`は従来どおり載せない）。最初のfull runでは4構成が退行し、上記2つの例外の移植で解消した。localは型crateの変更なのでworkspace全体のclippyとtest（70 targets、3,619 passed）、2 workerのfull run（792 s）。
- hosted：PR #627（head `0a4e0c9df`、merge `c74559509`）、run 36974432705 — `plan` 31s、`rust` 8m29s、`conformance (TypeScript 7.1)` 21m34s、`gates` 14s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `45f6024cd`と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 132→131、zod 576→559、Playwright 386→382、TypeScript `src/compiler` 360→355、Next.js 837→801、Effect 551→519、VS Code 3,762→3,750（この回もtsgoを含め全体が朝より遅い）。tsc-rs÷tsgoは0.58〜0.99で従来どおり、peak memoryは同等以下（MB main→本branch：313→294、1,303→1,298、768→755、291→293、1,327→1,328、1,046→1,028、5,470→5,302）。退行なし。
