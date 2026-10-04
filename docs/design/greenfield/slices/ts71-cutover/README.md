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

## P3-5j `esModuleInterop`／`allowSyntheticDefaultImports`は常にon（2026-10-02）

P3-5i後の集計でunexpected TS2497「This module can only be referenced with ECMAScript imports/exports by turning on
the '{0}' flag and referencing its default export」が9構成（`conflictingDeclarationsImportFromNamespace1`／`2`、
`es6ExportEqualsInterop`、`es6ImportEqualsExportModuleCommonJsError`／`Es2015Error`、`importNonExportedMember5`／`7`／`9`／`11`）。
tsgoは2つのoptionをdeprecatedなtristateとしてだけ持ち、`false`はremoved値（TS5108、P3-5a）で、それ以外ではどこからも
読まない：checkerはinteropを常にonとして扱い（checker.go:14780「With `esModuleInterop` (always enabled)」）、CommonJS
transformはinterop helperを常に出し、`resolveESModuleSymbol`にはTS2497の報告が無い。tsc-rsの`es_module_interop_effective`／
`allow_synthetic_default_imports_effective`は明示の`false`を尊重していたので常に`true`にし、`resolve_es_module_symbol`の
TS2497 2箇所（`export =`がmodule／variableでないときと、node16〜nodenextの`module.exports` arm）と、それだけに使われていた
`suppress_interop_error`引数を削除した。
同じ理由でTS1259「Module '{0}' can only be default-imported using the '{1}' flag」の2箇所（node20〜nodenextの
`module.exports` default importと、`export =` moduleのdefault import）も到達しなくなったので削除した（tsgoにTS1259は無い）。
unit testは1件を再pin：`esModuleInterop: false`でのnode20の`module.exports` default importはtsgo（`tsc-19dadef8`）と同じく
semantic rowなし（configのTS5108はprogram側）。
- conformance：15,228 configuration、lane A 13,467（変化なし）、full 12,864→12,873（+9）、text 93、category 21、mismatch 445→436、harness error 44、emit full 12,412。fullに上がったのはTS2497 classの9構成すべて。ratchet：0 regressions、9行追加（`intersectionConstructorReductionCrash`は従来どおり載せない）。localは型crateの変更なのでworkspace全体のclippyとtest（70 targets、3,619 passed）、2 workerのfull run（800 s）。
- hosted：PR #628（head `c98387daf`、merge `df43b0048`）、run 36978413299 — `plan` 28s、`rust` 9m36s、`conformance (TypeScript 7.1)` 21m45s、`gates` 15s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `7e466a148`と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 125→123、zod 535→551、Playwright 359→363、TypeScript `src/compiler` 360→353、Next.js 829→798、Effect 553→547、VS Code 3,732→3,725。tsc-rs÷tsgoは0.59〜0.99で従来どおり、peak memoryは同等（MB main→本branch：285→305、1,296→1,296、760→770、293→291、1,331→1,327、1,038→1,044、5,471→5,429）。zodとPlaywrightは同条件の5 rounds A/Bで、medianは本branch 548／main 543、364／355だがminは本branchが低く（529／531、344／349）、CPU時間も同等以下（3,586／3,634、2,503／2,500 ms）なのでノイズ。退行なし。

## P3-5k JavaScriptのconstructor functionとprototype代入は宣言でない（2026-10-02）

P3-5j後の集計で最大の残りはJavaScriptの一群（missing TS2683 23＋5、TS2309 18＋7、TS2339 16＋4、TS2749 9＋7…）。根は
tsgo 7.1がJavaScriptのconstructor functionとprototype代入の宣言を持たないこと：checkerに`isJSConstructor`が無く
（nodebuilderimpl.go:2949にコメントで残るだけ）、`GetAssignmentDeclarationKind`（ast/utilities.go:1547-1579）の種類は
None／ModuleExports／ExportsProperty／ThisProperty／Property／ObjectDefinePropertyValue／ObjectDefinePropertyExportsの
7つだけ（`F.prototype.m = x`はentity `F.prototype`へのProperty expandoで、`prototype` symbolが無いので何も束縛しない）、
binderの`bindThisPropertyAssignment`はclass memberの中だけで束縛し（function containerは`// !!! constructor functions`、
source fileはthis containerでない）、bindWorkerにspecial property declaration（JSDoc型付きの`this.p;`文）も無い。
JavaScriptの宣言の移植は3段に分け、このPRは第1段：
- 分類：tsc 6.0のPrototype／PrototypeProperty／ObjectDefinePrototypeProperty（`assignment.rs`）を返さない（tsgoと同じく
  Property／ObjectDefinePropertyValue）。
- 束縛（`bind.rs`）：`.prototype`を通る代入はtsgoのlookupと同じく何も束縛しない、別名`this`（`var self = this; self.x`）
  の振り替えを削除、`this.x = …`はclass memberの中だけ（普通の関数とsource fileでは宣言なし）、special property
  declarationを削除。expandoは同名の非expando宣言が無いときだけ宣言する（tsgoの`bindDeferredExpandoAssignment`）ので、
  `X.prototype = {…}`がclassの合成`prototype`に付かず、`C.x = …`がstatic memberやnamespace exportを再宣言しない。
- checker：`is_js_constructor`は常にfalse（呼び出し側の整理は後段）。class・interfaceの宣言型に`X.prototype = {…}`の
  object literalをmergeしない（tsc 6.0の`getAssignedClassSymbol`。tsgoの`getDeclaredTypeOfClassOrInterface`には無い）。
  最初のfull実行で出た`compiler/targetTypeTest1`の後退（`declare class`と`function`のmergeで、prototypeのobject
  literalのmemberが`Duplicate identifier 'add'`）はこの2点で解消した。
expandoの宣言先（tsgoは初期化子のsymbol、tsc-rsは変数のsymbol）と暗黙namespaceの廃止は第2段、CommonJSのexportsの束縛は
第3段。unit test 25件をtsgoの行に再pin（`tsc-19dadef8`、noLibのglobalsで照合）：constructor functionの`this`はTS2683、
`new`はTS7009、prototype代入は宣言にならない（classの`prototype`経由はTS2339）など。tsgoの表示とまだ違う2件（expando
functionの型表示、CommonJSのTS2323行）は現在の行を残し、tsgoの行を注記した。新しいunit test 3件（function merge
したclassへのprototype object代入、JavaScriptのstatic memberと同名のexpando、namespace exportと同名のexpando）も
tsgoの行にpinした。
- conformance：15,228 configuration、lane A 13,467（変化なし）、full 12,873→12,906（+33）、text 93→98、category 21→26、mismatch 436→393、harness error 44、emit full 12,412→12,413。上がったのは43構成：constructor function・prototype代入・`this`代入のJavaScript群がfull 33（`constructorFunctions3`、`constructorFunctionsStrict`、`typeFromJSConstructor`、`inferringClassMembersFromAssignments2`／`6`／`7`、`prototypePropertyAssignmentMergeAcrossFiles`、`topLevelThisAssignment`、`lateBoundAssignmentDeclarationSupport4`〜`6`…）、text 5、category 5。ratchet：0 regressions、43行追加・1行raise（emit、`jsDeclarationsClassLikeHeuristic`）（`intersectionConstructorReductionCrash`は従来どおり載せない）。最初のfull runで`compiler/targetTypeTest1`が退行し、上記のexpandoの規則とclass宣言型のmerge削除で解消した。localはbinderの変更なのでworkspace全体のclippyとtest（70 targets、3,622 passed）、2 workerのfull run（801 s）。Clippyの指摘2件（不要な借用、挙動は変わらない）はfull runの後に直し、workspace clippyとbinderのtest（74 passed）を再実行した。
- hosted：PR #629（head `7467f5876`、merge `97d186da0`）、run 36988318740 — `plan` 26s、`rust` 9m58s、`conformance (TypeScript 7.1)` 21m36s、`gates` 11s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `e3b12d215`と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 132→134、zod 528→537、Playwright 364→380、TypeScript `src/compiler` 356→353、Next.js 808→788、Effect 536→525、VS Code 3,574→3,552。tsc-rs÷tsgoは0.59〜0.96で従来どおり、peak memoryは同等（MB main→本branch：310→291、1,303→1,301、755→774、293→292、1,326→1,322、1,041→1,036、5,456→5,439）。zod・Playwright・honoは同条件の5 rounds A/B（本branch／main）で、medianは529／521、341／338、120／124、minは503／505、320／332、116／118、CPU時間は3,515／3,522、2,391／2,389、639／617 ms（3 roundsでは637／657）なのでノイズ。退行なし。

## P3-5l JavaScriptのexpando宣言をtsgoの方式で束縛・型付けする（2026-10-02）

P3-5kの第2段。tsgo 7.1のexpando（`F.p = …`、`Object.defineProperty(F, "p", …)`）は、tsc 6.0と束縛先・時機・宣言ノードが
すべて違う：`GetAssignmentDeclarationKind`（ast/utilities.go:1547-1579）はentity nameへの代入をすべてPropertyにし（`void 0`の
除外なし、任意のelement access）、binderは`bindExpandoPropertyAssignment`で全部をファイルの束縛後まで延期し
（binder.go:1027-1076）、`lookupEntity`（1274-1304）でsymbolを作らずに対象を引き、`getInitializerSymbol`（1096-1121）が返す
symbol——function宣言、JavaScriptのclass宣言、`const`（JavaScriptでは任意の）変数のexpando initializer（function／arrow、
JavaScriptのclass式・型なしの空object literal）の**initializer自身のsymbol**——のexportsに、同名の非expando宣言が無いときだけ
Property|Assignmentを宣言する。宣言ノードは代入（またはcall）そのもので、暗黙のnamespaceもMethod／accessorのflagも作らない。
- binder（`assignment.rs`、`bind.rs`）：分類をtsgoの関数そのものに置き換え、PrototypeProperty／Prototype／
  ObjectDefinePrototypeProperty の種類を列挙から削除。Property／ObjectDefinePropertyValueはファイル末尾で束縛し
  （`bind_deferred_expando_assignment`、`lookup_entity`、`lookup_name`、`get_initializer_symbol`、`is_expando_initializer`）、
  tsc 6.0のspecial property assignment・暗黙namespace・prototype束縛の一式を削除。延期した束縛時には`this` containerが
  無いので`this.a.b = …`は何も束縛しない（tsgoと同じ）。JSDocの`@class`／`@constructor`はClassを付けない（tsgoにJSDoc class
  tagの束縛は無い）。
- checker：`getWidenedTypeForAssignmentDeclaration`をtsgo版に移植（this代入のTyped／Constructor／Method、最初の`@type`、
  代入型のunion、CommonJS exportの先頭`undefined`の除外、JavaScriptの全nullableはimplicit any、
  `getAssignmentDeclarationInitializerType`／`containsSameNamedThisProperty`／`hasParentWithTypeAnnotation`／
  `getTypeFromPropertyDescriptor`）。expandoはinitializerのsymbolにあるので`getSymbolOfExpando`／`mergeJSSymbols`を削除
  （関数・class型、call式）、空object literalの型はexportsから作る（`checkObjectLiteral`のexpando arm）。代入の文脈型は
  `getContextualTypeForAssignmentExpression`／`getContextualTypeForBinaryOperand`（reparseされた`@type`、`module`／
  `exports`起点の代入は文脈型なし、`ns.p = ns.p || {}`の例外なし）。`=`はtsgoと同じく常に代入可能性を検査して右辺の型を
  返す（CommonJS exportへの`undefined`だけ除外）。JavaScriptのprototype・constructor function向けの残りの腕
  （`getThisType`、`tryGetThisTypeAt`、`getOuterTypeParameters`、JSDocのprototype host、`isPrototypeProperty`、late-bound
  代入のinstance側、node builderのprototype range）を削除し、`isClassInstanceProperty`は代入宣言を左辺で判定する
  （tsgo、`C.p = …`はinstance fieldでない；最初のfull runで`classFieldSuperAccessibleJs1`に余分なTS2855が出ていた）。`isDeclarationWithExplicitTypeAnnotation`と`x.p = …`宣言の
  assume-uninitialized（TS2565）もtsgoに合わせた。
- tsgoは「存在しないproperty」のエラー（TS2339）を`addDeferredDiagnostic`でファイル検査の最後まで遅らせる
  （checker.go:11547「must be deferred because reporting this error can cause us to materialize the containing type
  completely (to print it), leading to erroneous circularity errors」）。tsc-rsでも同じ遅延を入れ、expando関数の型を印字する
  途中で関数の戻り値型を再解決して出ていた偽のTS7023（`jsdocTypeFromChainedAssignment`など）を解消した。
- emitter：宣言のaccessibility診断で代入宣言を対象propertyの名前で呼ぶ（tsgo `GetNameOfDeclaration`、TS4032の引数）。
- CommonJSの束縛と型付けは第3段なので、そこまでの橋渡しを3つ残した：`exports`／`module.exports`の別名への
  `util.p = …`はexportとして束縛し、`module.exports = <object>`は従来どおり検査を省いてmoduleの型を返し、`x.p = …`形の
  CommonJS宣言（tsc-rsでは左辺が宣言ノード）は代入宣言として扱う。tsc 6.0の宣言emitterが使うalias判定では、
  aliasableな右辺を持つexpando代入をaliasとみなす（`foo.foo = foo`は`export { foo }`、tsgoのtransformerと同じ出力）。
unit testはbinder 5件、checker 17件を再pin（`tsc-19dadef8`、noLibのglobalsで照合。16件はtsgoの行で、うち6件は遅延した
TS2339がsinkの後ろに来る順序だけの変更；CommonJSの再宣言TS2323の1件はtsgoに無い行で、宣言ノードが代入になった分だけ
spanを更新し第3段まで残す）、tsgoの行にpinした新しいtest 5件（expandoの型とinitializer、JavaScriptの延期束縛と
namespace無し、遅延TS2339で循環なし、`super`経由のTS2855の判定、binderの構造）を追加。
- conformance：15,228 configuration、lane A 13,467（変化なし）、full 12,906→12,967（+61）、text 98→99、category 26→21、mismatch 393→336、harness error 44、emit full 12,413→12,414。上がったのは63構成（full 61、text 2）：expando・prototype代入・this代入のJavaScript群（`typeFromPropertyAssignment`系14、`typeFromPrototypeAssignment`1〜3、`jsContainerMergeTsDeclaration`1〜3、`expandoFunctionNestedAssigments`、`jsExpandoObjectDefineProperty`、`lateBoundClassMemberAssignmentJS`／`2`、`jsdocTypeFromChainedAssignment`、`declarationEmitExpandoPropertyPrivateName`…）。ratchet：0 regressions、57行追加・6行raise（`intersectionConstructorReductionCrash`は従来どおり載せない）。最初のfull runでは`compiler/classFieldSuperAccessibleJs1`が退行し（余分なTS2855）、`isClassInstanceProperty`の移植で解消して最終bytesで2回目を実行した。localはbinderの変更なのでworkspace全体のclippyとtest（70 targets、3,627 passed）、2 workerのfull run（796 s）。
- hosted：PR #630（head `fc4d0801e`、merge `7749efb2e`）、run 37005000503 — `plan` 28s、`rust` 10m33s、`conformance (TypeScript 7.1)` 21m52s、`gates` 13s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `621084854`と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 122→129、zod 532→546、Playwright 358→353、TypeScript `src/compiler` 354→346、Next.js 801→759、Effect 538→509、VS Code 3,644→3,611。tsc-rs÷tsgoは0.57〜0.95で従来どおり、peak memoryは同等以下（MB main→本branch：284→296、1,288→1,294、756→758、292→292、1,326→1,320、1,037→1,027、5,478→5,283）。honoとzodは同条件の5 rounds A/B（本branch／main）で、medianは123／121、513／521、minは114／115、510／507、CPU時間は613／615、3,444／3,505 msなのでノイズ。退行なし。

## P3-5m JavaScriptのCommonJS exportをtsgoの方式で束縛・型付けする（2026-10-02）

P3-5kの第3段（最終段）。tsgo 7.1のCommonJSは束縛も型付けもtsc 6.0より単純で、moduleは`export=`の解決先そのものになる：
- binder（`bind.rs`、`containers.rs`）：`module.exports = X`はファイルのexportsに`export=`を宣言し（Xがentity name・class式なら
  Alias、それ以外はProperty、宣言ノードは代入；`bindModuleExportsAssignment`、binder.go:1018-1025）、`exports.p = X`／
  `module.exports.p = X`／`Object.defineProperty(exports, "p", …)`はファイルのexportsにFunctionScopedVariable（代入でXが
  aliasableならAlias）を宣言する（`bindExportsOrObjectDefineProperty`、1088-1094）。CommonJS indicatorのあるJavaScript
  ファイルは末尾で`module`／`exports`をlocalに宣言し（`declareCommonJSVariable`：FunctionScopedVariable|ModuleExports、宣言は
  source file、`module`は同じ種類の`exports` memberを持つ）、moduleのsource fileとambient moduleは型・namespaceのexportを
  `export=`のexportsにも載せて`export=`をNamespaceModuleにする（`bindCommonJSTypeExports`、1043-1056；ambient moduleは
  `bindContainer`の末尾、1607-1622）。tsc 6.0の`exports`別名の振り替え、entity nameからのnamespace作成、
  `module.exports = {}`／`module.exports = exports`の除外、shorthand object literalのexport化を削除した。
- checker：`resolveExternalModuleSymbol`は`export=`の解決先だけを返す（tsc 6.0の`getCommonJsExportEquals`のmergeは無い、
  checker.go:15875-15883）。`exports`の型は解決したmoduleの型、`module`の型はmemberの匿名型（16919-16924）、CommonJSの
  file symbolの型は解決先の型（`getTypeOfFuncClassEnumModuleWorker`）、CommonJS exportの代入宣言の型は右端の値のregular型
  （18452-18478）。重複CommonJS exportのflow型と`any`の境界（`getFlowTypeFromCommonJSExport`、`isDuplicatedCommonJSExport`）、
  `module.exports = {…}`へのexportのmember合成（`getInitializerTypeFromAssignmentDeclaration`のCommonJS枝）、
  `checkAssignmentDeclaration`を削除。
- TS2309は`checkExternalModuleExports`（5858-5870）どおり、value exportがあるか`export=`がnamespaceを影にする
  （`hasShadowedNamespace`）ときで、JavaScriptの除外は無い。TS2323は`exports.p = …`だけで宣言された名前を除く。
- `resolveEntityName`はmeaningを持つsymbolが出るまでalias chainを辿る（`export=`がAlias|NamespaceModuleならもう1段）。
  import typeのmeaning、TS18042、`checkAliasSymbol`は`getSymbolFlags`で判定する。
- CommonJSファイルのtop-levelの`this`は`typeof globalThis`（`tryGetThisTypeAt`）、未使用の検査は`module`／`exports`を除き
  （ModuleExports）、`exports`起点の代入と関数の`this`の文脈型はModuleExportsの変数で判定する。
- P3-5lが残したCommonJSの橋渡し（`exports`別名の`util.p = …`、`module.exports = <object>`の検査省略、左辺のCommonJS宣言）は
  無くなった。tsc 6.0の宣言emitterが使うexpandoのalias判定は残る（emitterの側で扱う）。JSDoc typedefの束縛
  （`delayedBindJSDocTypedefTag`、ドット名のnamespace、`jsGlobalAugmentations`）はtsc 6.0のままで、tsgoのreparseした
  type aliasの束縛への置き換えは別のclass。
unit testはbinder 2件、checker 9件をtsgoの行に再pin（`tsc-19dadef8`、noLibのglobalsで照合：CommonJSのexportは
FunctionScopedVariable、`module.exports = F`はAlias、`exports`別名への代入は何も宣言しない、TS2309のJavaScript行、`module.exports.p`は
`export=`の型を読む、先頭の`undefined`だけ除く宣言型、top-levelの`this`はTS7017）、削除した`getFlowTypeFromCommonJSExport`の
test 2件を削除、tsgoの行にpinした新しいtest 3件（`export=`への型exportの昇格、ambient moduleのTS2309と`import { I }`、
requireの別名からのalias chain）を追加。
- conformance：15,228 configuration、lane A 13,467（変化なし）、full 12,967→13,023（+56）、text 99→94、category 21→20、mismatch 336→286、harness error 44、emit full 12,414→12,415。上がったのは58構成（errorのfull 56、category 1、emit 1）：CommonJSの`module.exports`／`exports`群（`moduleExportAlias`系6、`moduleExportAssignment`系4、`moduleExportWithExportPropertyAssignment`1〜4、`moduleExportDuplicateAlias`／`3`、`moduleExportPropertyAssignmentDefault`、`commonjsAccessExports`…）、JavaScriptの宣言emitの`jsDeclarations`系12、`typedefCrossModule`1〜4、`jsExportMemberMergedWithModuleAugmentation`1〜3、TypeScriptの`export=`と型exportの`exportAssignmentMerging`1／2／3／7／10、`incompatibleExports1`／`2`、`importDeclWithExportModifierAndExportAssignmentInAmbientContext`（emitは`importDeclWithExportModifierAndExportAssignment`）。ratchet：0 regressions、50行追加・8行raise（`intersectionConstructorReductionCrash`は従来どおり載せない）。localはbinderとcheckerとその逆依存（compiler、conformance）のclippyとtest（14 targets、2,031 passed）、2 workerのfull run（800 s）で、workspace全体のtestとclippyはhostedの`rust` job。full runの後に変えたのはtsgoの行番号を引くコメント6か所だけで、fmtとbinder／checkerのclippyを再実行した。`--checkers 4`の並列対照はlocalの負荷の方針（full runはsliceごとに1回）により実行していない。
- hosted：PR #631（head `ed33b8911`、merge `ff7c4b0c5`）、run 37015472485 — `plan` 27s、`rust` 9m59s、`conformance (TypeScript 7.1)` 16m51s、`gates` 13s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `1d39cabd6`と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 123→125、zod 529→545、Playwright 355→344、TypeScript `src/compiler` 353→340、Next.js 792→769、Effect 536→538、VS Code 3,588→3,625。tsc-rs÷tsgoは0.60〜0.94で従来どおり、peak memoryは同等（MB main→本branch：290→312、1,300→1,294、755→752、292→292、1,323→1,327、1,039→1,044、5,468→5,419）。hono・zod・VS Codeは同条件の5 rounds A/B（本branch／main）で、medianは122／126、526／533、3,674／3,642、minは121／120、517／510、3,594／3,502、CPU時間は657／661、3,585／3,558、25,030／24,989 ms（3 roundsではVS Codeのminが3,537／3,545）なのでノイズ。退行なし。

## P3-5n JSDocのtagとtypedefをtsgoの方式で扱う（2026-10-02）

P3-5m後の不一致の約半分（286中146）がJavaScript／JSDocで、その根はtsgo 7.1のJSDocの扱い全体にある：tsgoはJSDocを
parseしたあとreparser（parser/reparser.go）でtagを普通のTS構文に作り替え、`@typedef`／`@callback`（type expressionが
あるときだけ）はhostの文の直前に置くtype alias文、`@type`／`@param`／`@return`／`@template`／`@this`などはhostの型注釈・
型parameter・`this` parameterになり、checkerはtag自体を検査しない（checker.go:2295-2303はJSDocの`@link`名を解決するだけ）。
JSDocのparse diagnosticは従来どおりcheckJsのファイルだけで報告される（jsdoc.go:171-175、program.go:1525-1528）が、reparserの
誤りは普通のparse diagnosticになる。これを3段に分け（設計メモはこの節の末尾）、このPRは第1段のtagとtypedef：
- parser（`parser/jsdoc.rs`、`parser.rs`、`recovery.rs`）：`@author`、`@class`／`@constructor`、`@enum`はunknown tag
  （jsdoc.go:471-528）。名前の無い`@typedef`／`@callback`は欠落identifier（TS1003、JSDoc diagnostic）を名前に持ち、type
  expressionのあるtypedefではreparserの`checkNonIdentifierName`（reparser.go:41-56）と同じく名前の1文字前にTS1003を
  parse diagnosticとして出す（treeは完全なので、TS2880と同じくemitを妨げないreport-onlyの`Reparse` origin）。typedef／
  callback／overloadの子tagの`@template`はTS8039でparseを続け（jsdoc.go:873、1044、1114）、2つ目の`@type`もparseを止めない。
  typedefのtype literalは最初のproperty tagから始まり、終端・callbackのsignatureの開始もtsgoの位置。`}`は`{`があったときだけ
  期待する（jsdoc.go:106-123、`@satisfies T`は`'{' expected`だけ）。dotted nameの`.`はJSDoc scannerで読み、bodyの無い
  namespaceはそれ自身がaliasの名前。子tagの`@type`とsignatureの`@returns`は前のtagを見ない（TS1223）。
- binder（`bind.rs`、`containers.rs`、`declare.rs`、`node_util.rs`）：typedef／callbackはtype expressionがあるときだけ
  block-scopedに束縛し、名前の無いtypedefや`@enum`を次の宣言に結び付けない（tsc 6.0の
  `delayedBindJSDocTypedefTag`の分岐、`bindPotentiallyMissingNamespaces`、`jsGlobalAugmentations`、
  `nameForNamelessJSDocTypedef`を削除）。JSDocのtype aliasとdotted nameのnamespaceはmodule memberとしてexportする
  （`IsImplicitlyExportedJSDocDeclaration`、ast/utilities.go:4226-4236、内側のnamespaceにはreparserがexportを付ける）。
- checker：typedefはtype aliasとして検査し（`checkTypeAliasDeclaration`の順、TS8021なし、未使用の検査に登録）、TS8022・
  TS8025と`@class`のTS2348を削除。TS8023は`@augments`の型とbase型の同一性で判定する
  （`checkJSDocAugmentsTagMatchesExtends`、checker.go:4424-4447、class検査から）。base type nodeは`extends`要素で、classの
  最後のJSDoc commentの同名の`@augments`だけが型引数の無い`extends`に型引数を与える（reparser.go:582-600）。JavaScriptの
  object literalは文脈型が無くJSONファイルでなければJS literal（`@enum`の条件なし、checker.go:13413-13415）。`@enum`の
  型付けと死んだ`JSDocEnumTag`／`JSDocClassTag`の腕（binder・checker・emitter）を削除。
unit testはbinder 1件、checker 9件をtsgoの行に再pin（`tsc-19dadef8`とtsgoのbaseline：`@enum`はtypeでなく循環もない、
`@class`は`new`不要、`@template`の子はTS8039でpropertyはその後も続く、templateInsideCallbackのTS8039は6件、名前の無い
typedefのreparse TS1003、`@satisfies`は`'{' expected`だけ、`@augments`はbase型と同じ型なら報告なし、宙に浮いた
`@extends`／`@implements`は何も報告しない）、tsgoの行にpinした新しいtest 4件（`@enum`の変数は値、型の無いtypedefは
aliasを作らない、名前の無いtypedefの2つのTS1003、`@augments`の型引数と同一性）を追加。
- conformance：15,228 configuration、lane A 13,467（変化なし）、full 13,023→13,042（+19）、text 94→95、category 20、mismatch 286→266、harness error 44、emit full 12,415（変化なし）。上がったのは20構成（full 19、text 1）：`@enum`の群（`enumTag`、`enumTagCircularReference`、`enumTagUseBeforeDefCrash`、`exportedEnumTypeAndValue`、`jsEnumTagOnObjectFrozen`、`jsEnumCrossFileExport`、`jsDeclarationsEnumTag`）、`@class`（`callOfPropertylessConstructorFunction`、`constructorFunctions`）、typedef（`jsdocTypedefMissingType`、`jsdocTypedefNoCrash`／`2`、`misspelledJsDocTypedefTags`、`checkJsdocTypedefOnlySourceFile`）、`@augments`／`@extends`（`extendsTag2`／`4`、`jsdocAugments_nameMismatch`、`jsdocAugments_notAClass`、`jsdocAugmentsMissingType`はtext）、`checkJsdocSatisfiesTag14`。下がった構成はemitを含めて無い。ratchet：0 regressions、20行追加（`intersectionConstructorReductionCrash`は従来どおり載せない）。localはsyntaxとその逆依存（binder、checker、compiler、conformance、emitter、harness、program）のclippyとtest（60 targets、3,511 passed）、2 workerのfull run（792 s）で、workspace全体のtestとclippyはhostedの`rust` job。filterの段階で各tierをP3-5mのreportと比べ、reparserのTS1003を普通のparse diagnosticにすると`jsdocTypedefNoCrash`／`2`のemitがFullからNoneに落ちる（ratchetに無い行なので`--check`では見えない）ことを見つけ、report-onlyのoriginで直した。`--checkers 4`の並列対照は実行していない。
- 残りのJSDocは2段：P3-5o（hostされるtag）＝reparseHostedの規則（`@type`はVariableStatementの最初の型無し宣言、
  `@param`は名前か位置で一致、`@this`／`@return`／`@template`、修飾子tagはmember・constructor・binary expressionの修飾子）で
  型nodeを検査し、TS8024／TS8028／TS8029／TS8032を削除、TS8030／TS8020をtsgoに合わせる。P3-5p（JSDocの型構文）＝Closureの
  `function(...)`型・単独の`?`・`!`の優先順位・`module:` namepathの削除、名前の欠落の報告（`parseJSDocIdentifierName`、
  messageが無ければ報告しない）、`@`がtagを始める条件とfenced code block、`@see`の名前。
- hosted：PR #632（head `a94bb3235`、merge `066c24253`）、run 37025092097 — `plan` 27s、`rust` 9m45s、`conformance (TypeScript 7.1)` 22m0s、`gates` 14s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `3bd8fee84`と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 127→120、zod 557→540、Playwright 360→372、TypeScript `src/compiler` 374→361、Next.js 850→860、Effect 538→560、VS Code 3,792→3,810。tsc-rs÷tsgoは0.58〜0.98で従来の幅、peak memoryは同等以下（MB main→本branch：287→296、1,299→1,298、758→757、291→292、1,328→1,329、1,035→1,037、5,455→5,127）。Effect・Playwright・Next.jsは同条件の5 rounds A/B（本branch／main）で、medianは599／592、372／386、784／827、minは517／528、356／358、777／783、CPU時間は3,447／3,500、2,487／2,537、4,786／4,765 msなのでノイズ。退行なし。測定scriptは前のsessionのscratchpadから消えていたので同じ方式（交互実行、wall／CPU／peak RSS、bench-corporaの`tsconfig.bench-noemit.json`、VS Codeは`src/tsconfig.bench-stable.json`）で作り直し、honoでP3-5mの測定と同程度の値（tsgo 162、tsc-rs 119 ms；P3-5mでは165、125 ms）になることを確かめた。

## P3-5o JSDocのhostされるtagをtsgoのreparseの方式で扱う（2026-10-03）

P3-5nの続き（JSDocの第2段）。tsgo 7.1はJavaScriptファイルのnode（host）のparseが終わるところで、その最後のJSDoc
commentのtagをhostの構文に作り替える（parser/reparser.go reparseHosted、346-613；子が先に終わるので、parameter自身の
`/** @type */`は関数の`@param`より先に効き、同じcommentの後のtagは前のtagの結果を見る）。checkerはtag自体を検査せず、
作り替えた型注釈・型parameter・`this` parameter・question token・修飾子・cast・heritage clauseを普通の構文として検査する。
tsc-rsはtreeを変えず、reparserの判断を側表に記録してcheckerがそれを読む：
- syntax（`jsdoc_hosted.rs`、新設）：`JsDocHosted`はreparserが変えるnodeごとに、型（`@type`、`@param`、`@return`、get
  accessorの`@type`）、FullSignature（関数全体を型付ける`@type`）、型parameter（`@template`のtagと、最初のtagの開始から
  最後のtagの終わりまでの範囲）、question token（`[x]`か`=`型の`@param`）、`@param`が一致したparameter、`this`
  parameter（`@this`）、修飾子tag、cast（`as`／`satisfies`）、`@implements`、`@augments`を持つ。SourceFileのcellで遅延
  計算し（TypeScriptファイルでは空）、表はBoxに入れる：SourceFileに直接持つと`tsc-rs-program`の
  `config_parser_and_extends_graph_have_typed_depth_limits`（256段のextends）がdebug buildでstack overflowした。
- binder（`hosted.rs`、新設）：reparseHostedの規則の移植（post-orderの走査、最後のcommentだけ、object literalの中の
  修飾子tagの除外、`getFunctionLikeHost`、`findMatchingParameter`（名前か位置、binding patternは位置、reparseした
  `this`を数える）、`gatherTypeParameters`（typedef／callbackのあるcommentでは無し）、`makeQuestionIfOptional`）。
  `@template`の型parameterはhostされたときだけ束縛し（関数はlocals、classはmember、`@overload`のsignature）、hostの
  無い`@template`は何も宣言しない。修飾子flagはhostされた修飾子tagから（`@deprecated`は従来どおり）、JSDocのcastの判定は
  表から。`GetRightMostAssignedExpression`は複合代入も辿る。`@overload`はfunction／method／constructor（object literal
  の外）のoverload宣言で、tag名の範囲を持つ（reparser.go:138-142、236-240）。
- checker：
  - `check_attached_jsdoc`は`@typedef`／`@callback`／`@import`と、reparseされる`@overload`のsignatureだけを検査する。
    `@template`／`@type`／`@satisfies`／`@this`／accessibility tagの検査、`@type`のfunction型の検査、TS8028（`...`の
    位置）を削除。hostされた型はhostで型注釈として検査される。
  - FullSignature：`getSignatureOfFullSignatureType`（JavaScriptのfunction宣言・method・function式・arrow）、検査と
    TS8030（`getContextualCallSignature`が無いとき）、`checkAllCodePathsInNonVoidFunctionReturnOrThrow`の報告位置
    （型注釈、FullSignature、関数の順）。文脈signatureの型tagの近道とgetterの`@type`の腕を削除。
  - `@param`：一致したparameterの型とoptional（TS1047／TS1051はtagの位置）。`checkUnmatchedJSDocParameters`はtsgoの
    `getAllJSDocTags`（`GetNextJSDocCommentLocation`）で名前の無いtagを除き、名前の無い`@param`にTS1003は出さない。
    `JSDocVariadicType`は常にarray型で、callback／overloadのsignatureの`...T`だけがTのrest parameter（TS2370）。
    JavaScriptの合成`args` rest parameter（tsc 6.0の`maybeAddJsSyntheticRestParameter`）を削除。
  - `@this`は`this` parameterとしてsignature・`this`の型・表示に入り、constructor（TS2681）、arrow（TS2730）、
    accessor（TS2784）の報告はtag名の位置。`@template`は型parameterとして検査し、TS1092とTS6205の範囲はtagの範囲。
  - 修飾子tag：`checkGrammarModifiers`は構文の修飾子の後にhostされた修飾子を並べ、reparseされた修飾子には「must
    precede」の順序エラーを出さない。
  - cast：括弧の`@type`と`return`文の`@type`は`as`、`@satisfies`は`satisfies`（文脈型、TS2352はreparseされた型の位置、
    checker.go:12524-12527）。空配列literalの判定はJSDocのcastを外さない。
  - `@implements`はclassのimplements型、`@augments`は`extends`要素の型引数（`heritage_type_arguments`、TS2344）。
  - 名前解決：typedef／callback／importは最も近いSourceFile／Block／ModuleBlockの文の位置、`@param`／`@return`はhost
    された関数の中、`@template`は関数の型parameterの位置として扱う。tsc 6.0の代入宣言からのJSDoc名前解決
    （`resolveEntityNameFromAssignmentDeclaration`）を削除。
  - その他：候補の無いcallはTS2346を出さずunknownSignature、`getConstructorsForTypeArguments`のJavaScript緩和の削除、
    JavaScriptのobject literal memberの`@type`の代入検査、`@overload`の暗黙any（TS7010にhost名）とimplementationとして
    のhost。隣接の差として、JavaScriptの非strictでの`undefined`／`unknown`／`any`引数の省略の緩和を削除し（tsgo
    checker.go:9362-9372）、tsc 6.0の移植で欠けていた`new Promise()`のTS2810を足した。
- 既知の制限：castは式ごとに1つ（tsgoは`@type`と`@satisfies`を入れ子にする）、reparseした`this`を型付ける`@param`は
  未対応、宣言emit（`syntactic_type_node_builder`）はtsc 6.0のJSDoc探索のまま、`@overload`のsignatureからhostの型
  parameterが見える、`Outer<any>.Inner`の表示（tsgoは`Outer.Inner`、従来からの差）。
unit testはchecker 13件をtsgoの行に再pin（`tsc-19dadef8`、noLibのglobalsで照合：TS2728は`@property`の名前、`@satisfies`
の重複・variadicの位置・未一致の`@param`・名前の無い`@param`は報告なし、`@type`の関数型の関係、TS2355の位置、
`@type`の変数はasyncの戻り値注釈でない、外側の`@template`、parameterの型とsuggestion、非strictのJavaScriptの引数省略は
TS2554）、tsgoの行にpinした新しいtest 4件（hostされたtagの検査6行、TS2810、binderの表の構造、TypeScriptでは空）を追加。
- conformance：15,228 configuration、lane A 13,467（変化なし）、full 13,042→13,095（+53）、text 95→91、category 20→18、mismatch 266→219、harness error 44、emit full 12,415→12,416。上がったのは53構成（すべてfull、emitは`argumentsPropertyNameInJsMode2`も）：`@template`と型parameterの範囲（`jsdocOuterTypeParameters1`〜`3`、`jsdocTemplateClass`、`jsdocTemplateTag3`、`jsdocTypeParameterTagConflict`、`unusedTypeParameters_templateTag2`、`templateInsideCallback`、`callbackTag2`、`jsdocIllegalTags`、`classCanExtendConstructorFunction`）、`@param`（`jsdocParamTag2`、`jsdocParamTagNoName`、`jsdocPrefixPostfixParsing`、`typedefInnerNamepaths`、`paramTagWrapping`、`syntaxErrors`、`checkJsdocParamOnVariableDeclaredFunctionExpression`、`noParameterReassignmentIIFEAnnotated`、`jsFileFunctionParametersAsOptional2`、`jsdocRestParameter`、`callbackTagVariadicType`、`jsDeclarationsFunctions`、`jsDeclarationsReusesExistingTypeAnnotations`）、合成`args`の削除（`argumentsObjectCreatesRestForJs`、`argumentsPropertyNameInJsMode2`、`argumentsReferenceInFunction1_Js`、`paramTagOnFunctionUsingArguments`）、`@type`・FullSignature・cast（`checkJsdocTypeTag5`／`6`、`jsdocTypeTagFunctionTypePredicate`、`errorOnFunctionReturnType`、`asyncFunctionDeclaration16_es5`、`checkJsdocSatisfiesTag1`／`4`／`11`／`12`、`propertyAssignmentUseParentType2`、`jsDeclarationsMissingTypeParameters`）、修飾子tag（`jsdocReadonly`、`jsdocReadonlyDeclarations`）、`@overload`（`overloadTag1`、`jsFileMethodOverloads3`）、`@augments`の型引数（`superCallInJSWithWrongBaseTypeArgumentCount1`／`2`の4構成、`jsExtendsImplicitAny`）、`@this`（`thisTypeOfConstructorFunctions`）、tsc 6.0の名前解決の削除（`typeFromPropertyAssignment6`）、TS2346の削除（`interfaceMergeWithNonGenericTypeArguments`）、隣接の2件（`callWithMissingVoidUndefinedUnknownAnyInJs`の非strict、`jsPromiseNeedsJSDocHint`）。下がった構成はemitを含めて無い。ratchet：0 regressions、47行追加・6行raise（`intersectionConstructorReductionCrash`は従来どおり載せない）。localはsyntaxとその逆依存（binder、checker、compiler、conformance、emitter、harness、program）のclippyとtest（60 targets、3,515 passed）と`cargo xtask codegen diagnostics-check`、2 workerのfull run（791 s）で、workspace全体のtestとclippyはhostedの`rust` job。filterの段階では、JavaScriptを含む1,061 case（1,244構成）をP3-5nのreportとtierごとに比べ（emitを含めて下降0）、TypeScriptの`super`／`overload`のfilterも確かめた。最初のunit testで`tsc-rs-program`のdepth limitのtestがstack overflowし（`error: 1 target failed`、FAILEDの行は出ない）、表のBox化で直した。`--checkers 4`の並列対照は実行していない。
- JSDocの残り（P3-5p）：Closureの`function(...)`型（TS1005）、単独の`?`、`!`の優先順位、`module:` namepath、`object`は
  JavaScriptでもnonPrimitive、名前の欠落の報告、`@`がtagを始める条件とfenced code block、`@see`、`@import`のparse
  （TS1141）、JSDocのimport型（TS1340／TS2694）。
- hosted：PR #633（head `7a1f5926a`、merge `47bec9f1b`）、run 37095454879 — `plan` 27s、`rust` 10m9s、`conformance (TypeScript 7.1)` 13m55s、`gates` 14s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `4e4f61f3e`（P3-5nのhead `a94bb3235`と同じコードのrelease build）と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 136→139、zod 574→567、Playwright 362→366、TypeScript `src/compiler` 369→350、Next.js 877→761、Effect 540→561、VS Code 3,589→3,722。tsc-rs÷tsgoは0.55〜0.93で従来の幅、peak memoryは同等以下（MB main→本branch：292→288、1,300→1,295、781→753、292→290、1,333→1,337、1,037→1,031、5,484→5,439）。VS Code・Effect・hono・Playwrightは同条件の5 rounds A/B（本branch／main）で、medianは3,499／3,593、556／563、131／127、380／375、minは3,485／3,435、534／548、126／118、341／345、CPU時間は23,990／24,546、3,388／3,476、653／657、2,463／2,602 msなのでノイズ。退行なし。測定scriptはP3-5nと同じ方式（交互実行、wall／CPU／peak RSS）で作り直した。

## P3-5p JSDocの型構文とcommentの構文をtsgoに合わせる（2026-10-03）

P3-5oの続き（JSDocの第3段、最終段）。tsgo 7.1のJSDocの型とcommentのparseはtsc 6.0と次の点が違う：
- 型（`parser.rs`）：Closureの`function(…)`型は無く、`function`は型名（JavaScriptのJSDocではglobalの`Function`、
  TypeScriptでは名前解決の対象）なので、`(`でJSDocのTS1005、TypeScriptでは回復parseになる（parser.go:2804-2890）。単独の
  `?`（tsc 6.0のJSDoc unknown型）も無く、前置の`?`／`!`は型演算子以上を取る（parseJSDocNullableType／
  parseJSDocNonNullableType、2898-2909）ので`{?}`はTS1110。`module:` namepathも無い（parseJSDocType、2911-2926）。
- JSDoc comment（`parser/jsdoc.rs`、`scanner.rs`）：3つ以上のbacktickがfenced code blockを切り替え、その中では`@`がtagを、
  `{`がlinkを始めない。`@`は直後が識別子の開始・空白・改行のときだけtagを始める（CanFollowJSDocAt、scanner.go:1392-1398）。
  本文のmarginは最初に保存した文字の位置で、0でも置き換えない（jsdoc.go:192-349、546-712）。`@see`は`://`の続かない
  識別子か、`{`と識別子のときだけ名前を読む（907-916）。名前の欠落（tag名、`@augments`／`@implements`のclass、
  qualified nameの右辺）は現在のtokenに報告し、messageの無い呼び出し（`@param`の名前）は報告しない
  （parseJSDocIdentifierName、1341-1355；tsc 6.0は長さ0でtokenのfull startに報告していた）。
- checker：JavaScriptの`object`はnon-primitive（checker.go:23268、tsc 6.0の非noImplicitAnyでの`any`は無い）、
  `Object<K, V>`はKが有効なindex keyなら`Record<K, V>`のalias instantiation（23514-23522）。import clauseのある`@import`は
  reparseされたimport宣言として`checkImportDeclaration`で検査する（reparser.go:123-137、checker.go:2414）：module
  specifierが文字列でなければTS1141、default importのTS2613、namespaceの判定は再配置先の文リスト、JSDocのimport clause
  はdefaultとnamedを併用できる（grammarchecks.go:2103、TS1363なし）。import clauseの無い`@import`は検査しない。
- 隣接してtsgoに合わせた点：`return`文は文法エラーより先に式を検査する（checker.go:4114-4122、誤った位置の`return`でも
  名前を解決する）、無名のfunction式の報告位置は代入先の名前（GetErrorRangeForNodeのGetNameOfDeclaration、
  scanner.go:2603-2607；TS7011、TS2738のrelated）、TS2613はimport clause全体に報告する（checker.go:14830）。
- 残す点：parserが作らなくなった`JSDocFunctionType`／`JSDocUnknownType`／`JSDocNamepathType`は、node builderが
  `JSDocFunctionType`を合成するのでkindとbinder・checker・emitterの腕を残した（撤去は宣言emitの段）。instantiation
  expressionの型引数の消去（tsgoは`foo`、tsc-rsは`(foo)`と、JSDoc型を含むものはそのまま）はemitのclass。
unit test：checker 11件とemitter 2件をtsgoの行に再pin（`tsc-19dadef8`、noLibのglobalsで照合）：Closureの`function(…)`型を
使っていたtestはTypeScriptの関数型に書き換えるか（可変長と固定長のarity、`this`の型、asyncの戻り値）、7.1の行に合わせ
（tagは`Function`を与えるのでTS7014もcallbackの関係のTS2345も無い、名前の欠落のTS1003は長さ1、TS8020は`.<`と`*`、
TS7011は代入先の名前）、emitterの単独の`?`は前置の`?string`にした。7.1に無い構文を扱っていたtest 2件（JSDoc function型の
call／construct memberの束縛、TypeScriptでの`function(this:…)`／`function(new:…)`）を削除し、tsgoの行にpinした新しい
test 7件（JSDocの型構文、fenced code blockと`@`と`@see`、名前の欠落の位置、`object`と`Object<K, V>`、誤った位置の
`return`、function式の報告位置、`@import`の検査）を追加。
- conformance：15,228 configuration、lane A 13,467（変化なし）、full 13,095→13,132（+37）、text 91→87、category 18→19、mismatch 219→185、harness error 44、emit full 12,416→12,418。上がったのは39構成（full 37、category 1、text 1）とemitの2構成：Closureの`function(…)`型と`?`／`!`／`module:`（`jsdocFunctionType`、`jsdocParseHigherOrderFunction`、`jsdocParseParenthesizedJSDocParameter`、`jsdocParseDotDotDotInJSDocFunction`、`jsdocParseStarEquals`、`jsdocTypeTagParameterType`、`jsdocTypeTagRequiredParameters`、`checkJsdocTypeTag1`／`2`、`checkJsdocTypeTagOnObjectProperty1`／`2`、`jsdocThisType`、`jsdocTemplateTag`、`jsdocVariadicType`、`jsdocFunction_missingReturn`、`jsdocParameterParsingInfiniteLoop`、`jsdocTypesWithPrefixesAndUnionTypes1`、`asyncArrowFunction_allowJs`、`jsDeclarationsRestArgsWithThisTypeInJSDocFunction`、`jsDeclarationsModuleReferenceHasEmit`、`noAssertForUnparseableTypedefs`、`typedefTagWrapping`はtext、TypeScriptの`jsdocDisallowedInTypescript`と`expressionWithJSDocTypeArguments`）、`object`と`Record`（`paramTagNestedWithoutTopLevelObject3`、`jsDeclarationsReusesExistingNodesMappingJSDocTypes`）、`@import`（`importTag13`／`14`、`importDeferJsdoc`）、名前の欠落の位置（`jsdocAugmentsMissingType`、`jsdocImplements_missingType`、`jsdocPrivateName2`）、`return`の検査順（`parseErrorIncorrectReturnToken`、`parserErrorRecovery_ModuleElement1`、`parserStatementIsNotAMemberVariableDeclaration1`、`multiLinePropertyAccessAndArrowFunctionIndent1`、`reachabilityChecksNoCrash1`はcategory）、function式の報告位置（`typeFromParamTagForFunction`、`awaitInNonAsyncFunction`）、emitは`jsDeclarationsJSDocRedirectedLookups`と`jsDeclarationsMissingTypeParameters`。下がった構成はemitを含めて無い。ratchet：0 regressions、34行追加・7行raise（`intersectionConstructorReductionCrash`は従来どおり載せない）。localはsyntaxとその逆依存（binder、checker、compiler、conformance、emitter、harness、program）のclippyとtest（60 targets、3,520 passed）、2 workerのfull run（791 s）で、workspace全体のtestとclippyはhostedの`rust` job。filterの段階では、JavaScriptを含む1,061 caseと`jsdoc`／`parser`／`import`／`return`／`implicit`／`this`／`function`／`default`／`export`のfilterをP3-5oのreportとtierごとに比べた（下降0）。`--checkers 4`の並列対照は実行していない。
- JavaScript／JSDocの残り（次のclass候補）：JSDocのimport型とvalue参照（TS1340 5、TS2694 4、TS2749 2）、node16系の
  CommonJSの自己名・`#`import（TS2307／TS1479、8構成）、JavaScriptの文法エラーの位置（TS8017／TS8009／TS8010、4）、
  宣言emitの診断（TS4023／TS9006 2、isolatedDeclarationsのTS9010／TS9013 2）、重複宣言のrelated（`typedefTagWrapping`、
  `typedefCrossModule5`のtext）、JSONの文法（TS1327／TS1136 2）、tslib helper（TS2343 3）。
- hosted：PR #634（head `1da276b1e`、merge `1d007136e`）、run 37100180435 — `plan` 25s、`rust` 6m45s、`conformance (TypeScript 7.1)` 21m43s、`gates` 14s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `ec16f69cf`（P3-5oのhead `7a1f5926a`と同じコードのrelease build）と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 123→138、zod 520→501、Playwright 330→353、TypeScript `src/compiler` 326→326、Next.js 730→747、Effect 517→479、VS Code 3,317→3,357。tsc-rs÷tsgoは0.56〜0.95、peak memory（MB main→本branch）：287→309、1,301→1,288、747→813、288→288、1,326→1,327、1,015→1,037、5,431→5,457。5 roundsのA/B（本branch／main）でhonoは129／120 ms・CPU 629／643 ms、Next.jsは742／739 msでノイズ、Playwrightは361／334 ms・CPU 2,517／2,360 ms・RSS 807／750 MBで一貫して重い。原因はcheckの仕事量でなく既定のshard分割：7.1のJSDocのparseで`@see https://…`が名前参照を作らなくなり、宣言ファイルのnode数が減る（`csstype`で1,920など）ので、node数で決まるディレクトリ優先の決定的な分割が1,416ファイル中957を別のshardに移し、shardごとのcheck時間の合計が約1,642から約1,793 msになった。`TSRS_CHECKERS=1`ではPlaywright 940／935 ms・CPU 1,457／1,451 ms・RSS 528／531 MB、hono 179／178 ms、zod 1,371／1,368 msで同等、`TSRS_SHARD_PARTITION=contiguous`でも同等（340／339 ms）。重みからTypeScriptファイルのJSDoc nodeを除く案は一様には効かなかった（Playwright 355 ms、zod 534／522 ms、Effect 446／490 ms）ので入れていない。仕事量の退行は無く、node数の小さな変化に対する分割の安定性を性能作業の候補として残す。

## P3-5q JavaScriptの型参照と専用構文の診断をtsgoに合わせる（2026-10-03）

P3-5p後のJavaScriptの不一致のうち2つのclass：
- JSDocのimport型と値の参照（11構成）：
  - `import("…")`の型は型の意味だけで解決する（checker.go:25040-25113）。tsc 6.0のJavaScriptの緩和（Value|Typeでの解決、
    `export=`のmoduleの値のproperty）を削除したので、moduleが型でなければTS1340、qualifierが値ならTS2694。CommonJSの
    `module.exports`の横にexportされたtypedefはqualifierから引ける（25077-25086）。
  - `require`の別名：tsgoのbinderは型の無い・exportされない宣言の素の`require("…")`だけを別名にする
    （IsVariableDeclarationInitializedToRequire、ast/utilities.go:2874-2903）。`require("…").C`は通常の変数で、型として
    使えばTS2749。checkerの`checkVariableLikeDeclaration`と`isAliasSymbolDeclaration`も同じ判定。
  - CommonJSの`exports`は`declareCommonJSVariable`の局所変数だけで解決し、tsc 6.0のファイルsymbolへのfallbackを削除
    （`@type {exports}`はTS2749）。
- JavaScriptの専用構文の診断（TS80xx、8構成）：tsgoはtsc 6.0の`getJSSyntacticDiagnosticsForFile`をparserの
  `checkJSSyntax`に移した（parser.go:6711-6856）。`js_grammar.rs`をその移植に置き換えた：検査するのはparserが呼ぶnode
  （型シグネチャのparameterとaccessor、class外のindex signatureは除き、class `extends`の式は含む）、範囲はnode・名前・
  型・listから先頭のtriviaを除いたもの（本体の無い関数様のTS8017はnode全体で、classのindex signatureもTS8017）、報告の後も
  子を検査する（`as`の内側の`!`も報告）、TS8009の修飾子はexport／static／accessor／async／default以外、parameterの
  decoratorはここでは見ない。experimentalDecoratorsが無いときの未チェックのJavaScriptファイルのparameter decoratorは、
  programの追加の構文診断としてdecoratorの範囲にTS1206（compiler/program.go:743-784）。
unit test：binder 1件、checker 3件をtsgoの行に再pin（素の`require`だけが別名、`exports.Foo`の参照はTS2503で`exports.Foo`と
表示、値のexportを指すimport型の表示、`require(…).foo`が非推奨の再exportに出すTS6385）、7.1では作られない
`require(…).y`の別名の宣言emitのtest 2件を削除、tsgoの行にpinした新しいtest 3件（JSDocの型参照の4行、JavaScript専用構文の
9行、未チェックのparameter decorator）を追加。
- conformance：15,228 configuration、lane A 13,467（変化なし）、full 13,132→13,150（+18）、text 87→85、category 19、mismatch 185→169、harness error 44、emit full 12,418（変化なし）。上がったのは18構成（すべてfull）：import型と値の参照（`jsdocImportTypeReferenceToESModule`、`jsdocImportTypeReferenceToCommonjsModule`、`jsdocImportTypeReferenceToStringLiteral`、`jsdocImportTypeNodeNamespace`、`jsdocTypeReferenceToImportOfFunctionExpression`、`jsDeclarationsFunctionClassesCjsExportAssignment`、`jsDeclarationsParameterTagReusesInputNodeInEmit1`、`enumTagImported`、`moduleExportAssignment7`、`jsdocTypeReferenceToImport`、`jsdocTypeReferenceExports`）、JavaScript専用構文（`jsFileCompilationFunctionOverloadSyntax`、`jsFileCompilationConstructorOverloadSyntax`と`jsFileCompilationMethodOverloadSyntax`はtextから、`jsDeclarationsClassesErr`、`jsDeclarationsTypeReferences4`、`plainJSGrammarErrors`、`parameterDecoratorInJsFile`のcheckjs=true）。下がった構成はemitを含めて無い（途中でcheckJs無しの`parameterDecoratorInJsFile`が落ち、programの追加の構文診断で戻した）。ratchet：0 regressions、16行追加・2行raise（`intersectionConstructorReductionCrash`は従来どおり載せない）。localはbinderとcheckerとその逆依存（compiler、conformance）のclippyとtest（14 targets、2,042 passed）、2 workerのfull run（792 s）で、workspace全体のtestとclippyはhostedの`rust` job。filterの段階ではJavaScriptを含む1,061 caseと`import`／`export`／`module`／`require`／`decorator`のfilterをP3-5pのreportとtierごとに比べた（下降0）。`--checkers 4`の並列対照は実行していない。
- JavaScriptの残り：node16系のCommonJSの自己名・`#`import（TS2307／TS1479、8構成）、宣言emitの診断（TS4023／TS9006、
  isolatedDeclarations）、JSONの文法（2）、tslib helper（3）、重複宣言のrelated。
- hosted：PR #635（head `857a31272`、merge `3f710610e`）、run 37104409078 — `plan` 30s、`rust` 9m36s、`conformance (TypeScript 7.1)` 15m11s、`gates` 12s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `dff0d2260`（P3-5pのhead `1da276b1e`と同じコードのrelease build）と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 125→134、zod 515→498、Playwright 364→359、TypeScript `src/compiler` 331→328、Next.js 749→730、Effect 518→488、VS Code 3,341→3,343。tsc-rs÷tsgoは0.56〜0.93、peak memory（MB main→本branch）：311→328、1,286→1,298、790→816、288→288、1,318→1,322、1,034→1,031、5,442→5,443。honoとPlaywrightは同条件の5 roundsのA/B（本branch／main）でhono 125／128 ms・CPU 597／624 ms、Playwright 360／362 ms、`TSRS_CHECKERS=1`のhonoは178／180 msなのでノイズ（変更はJavaScriptだけに効く）。退行なし。

## P3-5r 文法エラー下の検査と報告位置をtsgoに合わせる（2026-10-03）

P3-5q後の不一致から、tsgoが検査の順序・報告の位置・まとめ方を変えた小さなclassをまとめて直した：
- 文法エラーがあっても式を検査する：`yield`は包む関数とgeneratorの判定より先にoperandを検査する
  （checker.go:11152-11161）。`export =`／`export default`は文脈の文法エラーより先に式を検査する（5748-5753）。そのため
  型や名前空間の名前を値として報告しない（isExportAssignmentExpressionName、checker/utilities.go:139-152、
  checker.go:1686-1690と1726-1730。tsc 6.0の`export = NS.J`のTS2708は無い）。
  型注釈の検査はtsgoどおり`checkTypeAssignableToAndOptionallyElaborate`。
- ブロック内の`import`／`export`／`import =`：source fileの直下のブロックにあれば、文法エラーの後もmodule名を解決する
  （checkExternalModuleNameInGlobalScope、5702-5714）。programは入れ子の`import`を解決しないので、存在するファイルでも
  TS2307になる。side-effect importと関数内は解決しない。tsc-rsではこの検査の間だけ、authoritativeな表に無いmodule名を
  未解決として扱う（`module_name_outside_program`。tsgoの`GetResolvedModule`がnilを返すのに当たる）。JavaScriptの
  `import =`の文言（TS1473）もtsgoどおり。
- `new`式の型：`getQuickTypeOfExpression`の`new`の分岐（7549-7550）を移植した。構築signatureが1つで非genericなら、その
  戻り値型を変数の型にする。constructorがprivate／protected／abstractで呼び出しがエラーでも同じ。callの分岐も
  memberのある型のsignatureを読む（getReturnTypeOfSingleNonGenericSignature、7559-7565、allowMembers=true）。
- 報告位置：
  - TS2447は演算子のtoken（12577）。
  - TS1453は`resolution-mode`の値。空の値も報告する（parseResolutionMode、parser.go:6696-6708）。
  - 先頭以外の`#!`は2文字のUnknown token（scanner.go:912-915）。
  - 単独の`\`のTS1127は長さ1（scanInvalidCharacter、2206-2211）。
  - JSXの子の式はdid-you-mean-to-call／constructを試す（relater.go:438-448）。報告は子の式で、related TS6212／TS6213が付く。
- `new A?.b()`はTS1209（parser.go:5818-5820）。木は完全なので、報告だけのparse診断（`ParseDiagnosticOrigin::Grammar`）
  にしてemitを止めない。
- 暗黙のJSX runtime（react-jsx／react-jsxdev）では、classicのfactoryの引数個数の検査（TS6229）をしない（jsx.go:604-606）。
  `React`は参照されないので、未使用ならTS6133。
- 未使用の宣言（checkUnusedLocalsAndParameters、checker.go:7255-7418）：tsgoはtsc 6.0のgroup分けをやめ、宣言の持ち主
  （宣言list、関数のparameter）ごとに一度ずつ見る。
  - listとbinding patternは、宣言が2つ以上ですべて未参照のときだけ全体を報告する（TS6199／TS6198）。省略した配列要素も数え、
    範囲は宣言list。それ以外は名前に報告する。
  - `_`の免除はparameter、for-in/of、`using`、名前を変えたbinding要素。
  - class static blockのlocalsも検査する（2861-2867）。
  - JSDocの`@import`と関数内の`@typedef`／`@callback`は、reparseされた宣言としてTS6192／TS6196を出す。
  - `import defer type`の既定名と、import attributesのparse errorでの抑制（tsc 6.0の範囲判定）は無い。
- unit test：
  - tsgoの行に再pin：
    - 既存のtest 8件（TS2447の2件、TS1453の3件、TS6199の範囲、JSDocの`@import`の未使用2件）。
    - tsc 6.0.3の観測だったsyntaxのfixture 2つの、TS1127の長さの行（`utf16-recovery-boundary.json`の1行、
      `utf16-scanner-escape-diagnostics.json`の7行。tsc-19dadef8で確認し、fixtureに注記）と、recoveryの分類の条件。
  - tsgoの行にpinした新しいtest 7件：
    - checker：文法エラー下の検査と`new`の型とTS2447、`export =`の型・名前空間の名前、JSXの子のdid-you-mean、暗黙のruntimeの
      `React`未使用、未使用の宣言の14行
    - syntax：`#!`・`\`・TS1209
    - compiler：authoritativeなprogramでのブロック内の`import`のTS2307
- conformance：
  - 15,228 configuration、lane A 13,467（変化なし）、806 s。
  - full 13,150→13,209（+59）、text 85→57、category 19、mismatch 169→138、harness error 44（同じ集合）。
  - emit full 12,418→12,422。
- 上がった60構成（fullへ59、textへ1）とemitの4構成：
  - operandの検査（8）：`YieldExpression2`／`12`／`14`／`15`／`17_es6`、`parserExportAssignment5`／`9`、
    `checkChildrenAlwaysChecked`。
  - ブロック内の`import`（2）：`moduleDeclarationsInNonScopeBlock`、`moduleElementsInWrongContext`。
  - `new`の型（2）：`typesWithPrivateConstructor`、`typesWithProtectedConstructor`。emitは、この2つと
    `classConstructorAccessibility`／`2`の宣言emitで型が`any`から`C`になった。
  - TS2447（3）：`bitwiseCompoundAssignmentOperators`、`arithmeticOperatorWithInvalidOperands`、
    `constructorWithIncompleteTypeAnnotation`（TS1127の長さも要る）。
  - TS1453（4）：`nodeModulesTripleSlashReferenceModeOverrideModeError`のnode16／node18／node20／nodenext。
  - `#!`（2）：`shebangError`。`manyCompilerErrorsInTheTwoFiles`はtextまで（`@pretty`の残り）。
  - TS1127の長さ（27）：`parserSkippedTokens`の19件、`invalidUnicodeEscapeSequance`1〜4、
    `slashBeforeVariableDeclaration1`、`unicodeEscapesInNames02`、`TypeArgumentList1`、`parserX_TypeArgumentList1`。
  - JSXの子（3）：`jsxFragmentWrongType`、`checkJsxChildrenProperty4`／`5`。
  - 暗黙のruntime（2）：`reactImportUnusedInNewJSXEmit`のreact-jsx／react-jsxdev。
  - TS1209（1）：`invalidOptionalChainFromNewExpression`。
  - 未使用の宣言（6）：`unusedDestructuring`、`unusedLocalsAndParameters`、`unusedLocalsInMethod2`／`3`、
    `unusedVariablesWithUnderscoreInBindingElement`、`unusedVariablesWithUnderscoreInForOfLoop`。
- 下がった構成はemitを含めて無い。
- ratchet：0 regressions、31行追加・31行raise（errorのtext→full 29行、emitのnone→js 2行）。
  `intersectionConstructorReductionCrash`は従来どおり載せない。
- local：
  - syntax・binder・checker・compiler・conformance・emitter・harness・programのclippyとtest（60 targets、3,528 passed）。
  - 2 workerのfull run（806 s）。workspace全体のtestとclippyはhostedの`rust` job。
  - filterの段階では、`export =`／`export default`／`yield`を含むcase、constructorの可視性と循環のcase、JSX・`noUnused`・
    `#!`・`resolution-mode`・ビット演算のcase、`\`を含む全case、入れ子のimport/exportのcaseをP3-5qのreportとtierごとに比べた
    （下降0）。
  - `--checkers 4`の並列対照は実行していない。
- 残り（lane Aでfullでない258構成：mismatch 138、text 57、category 19、harness error 44）の主なclass：
  - 関係エラーのchain（tsgoのErrorChainと報告時の縮約。「Call signature return types…」はmarkerとして畳まれる。text 16前後）
  - ambient moduleのimport attributes（約13）
  - node16系のCommonJSの自己名・`#`import（12）
  - 正規表現の検証（9）
  - 重複宣言のrelated（約7）
  - conflict marker（6）
  - インスタンス化の循環（TS5114／TS5115、5）
  - tsconfigの位置などharnessの行（約6）
- hosted：PR #636（head `e7c808588`、merge `97abe471c`）、run 37109454811 — `plan` 37s、`rust` 7m30s、`conformance (TypeScript 7.1)` 14m5s、`gates` 11s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `47d8bc192`（P3-5qのhead `857a31272`と同じコードのrelease build）と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 152→148、zod 546→574、Playwright 405→384、TypeScript `src/compiler` 387→361、Next.js 814→826、Effect 594→572、VS Code 3,788→3,716。計測時のload averageが高く（6前後）絶対値はP3-5qの記録より大きいが、同じroundの交互実行で比べている。tsc-rs÷tsgoは0.60〜0.98、peak memory（MB main→本branch）：316→306、1,292→1,296、819→802、289→289、1,316→1,323、1,032→1,046、5,430→5,448。zodとNext.jsは同条件の5 roundsのA/B（main／本branch）でzod 583／579 ms・CPU 3,725／3,718 ms、Next.js 826／831 ms・CPU 4,880／4,848 msなのでノイズ。退行なし。

## P3-5s 関係エラーのchainと診断の集約をtsgoに合わせる（2026-10-03）

P3-5r後のtext tierの主なclass（関係エラーのchainの文言）と、診断の集約の違いを直した：
- 関係エラーのchain（relater.go:2574-2620、4737-4990）：
  - tsgoのrelaterは、tsc 6.0の`incompatibleStack`／`overrideNextErrorInfo`／`skipParentCounter`／`lastSkippedInfo`を持たない。
    rowはすべてchainに積む。`reportError`は「Types of property 'x' are incompatible」を積むとき、chainの2つ下を見て
    次のように畳む：
    - signatureの戻り値型のmarkerなら「The types returned by 'x(...)'」（構築なら`new x()`、引数なしなら`x()`）にする。
    - property incompatibilityなら「The types of 'x.y'」にする（addToDottedName）。
    - 直下がexcess propertyのrowなら積まない。
  - markerの4つのmessage（Call／Construct signature return types…）は`elidedInCompatibilityPyramid`で、診断を作るときに
    飛ばす（createDiagnosticChainFromErrorChain、400-412）。tsc-rsのchainは(message, args)の列にし、診断の時に組み立てる。
  - `reportRelationError`はchainの先頭を見てheadを落とす：excess propertyのrowは常に、missing propertyや
    readonly・excessive complexityのrowは同じsource／targetのとき（P2-2で入れた判定にexcess propertyを加えた）。
  - `reportErrorResults`にtsc 6.0のheadの延期は無い。1対1の構築signatureの比較に「Types of construct signatures are
    incompatible」の行は無い（4515-4522）。
  - indexed accessのconstraintの再試行はchainの深さで短い方を選ぶ（chainDepth）。cacheされたoverflowは
    `Excessive complexity`を報告するだけになった。
- tupleの関係（propertiesRelatedTo、4142-4180）：位置の対応はrest要素（Rest）で決め、長さの検査は可変要素（Variable）で
  行う。restの無いtargetの範囲を越えたsourceの要素は「Target allows only N element(s) but source may have more」。
- overloadの失敗（reportCallResolutionErrors、checker.go:9841-9860）：最後のcandidateの診断を1つずつ「No overload matches
  this call」／「The last overload gave the following error」で包む。tsc 6.0は1つの可変なcontaining chainを全ての
  引数の関係に通したので、2つ目以降の行がchainの下に重なっていた。
- arrow functionの戻り値のelaboration（elaborateArrowFunction、661-668）：asyncでない関数の戻り値型がPromiseなら合う
  ときに、related「Did you mean to mark this function as 'async'?」を加える。
- 診断の集約：
  - tsgoの`SortAndDeduplicateDiagnostics`（compiler/program.go:1651-1688）は、related infoだけが違う診断を1つにまとめ、
    related infoを並べ替えて重複を除く。tsc 6.0は最初の1つだけを残していた。
  - `lookupOrIssueError`はrelated infoまで等しい既存の診断だけを再利用する（ast/diagnostic.go:269-297）。
  - tsgoには`amalgamatedDuplicates`が無い（checker.go:14437-14460）。ファイルをまたぐ重複宣言はmergeごとにすぐ報告し、
    TS6200「Definitions of the following identifiers conflict…」の要約は無い。module augmentationもaugmentationごとに
    報告し、2つの「'x' was also declared here」になる。
  - checkがskipされるfile（skipLibCheckの宣言ファイル、skipDefaultLibCheck、noCheck、checkしないJavaScript）には
    重複宣言の行を作らない。tsgoはcheckerごとに作り、`getSemanticDiagnosticsForFile`がfileごと捨てるので、出力は
    変わらない。zodでは2つの`@types/node`（22.10.5と22.13.13）が衝突し、checkerごとに2,142行（約4 MB）を作って
    いた（amalgamationをやめただけではpeak footprintがchecker 10個で約40 MB増えた）。
- unit test：
  - tsgoの行に再pin：
    - 既存のtest 6件（`(new f()).g`と`map(...).size`の「The types returned by」、ES5のasync constructorの関係で
      markerを飛ばすchain、型assertionのexcess propertyはTS2353だけ、構築signatureのwrapperの無いchain、
      重複宣言の報告順）。
    - relationのerror stateのtest 1件（chainの深さ）。
  - tsgoの行にpinした新しいtest 6件：
    - 関係のchain（markerの省略、「The types returned by」、tupleの範囲、overloadの包み方）
    - arrowの`async`のrelated
    - ファイルをまたぐ8つの重複（TS6200でなく各宣言にTS2451）
    - skipLibCheckの宣言ファイルの重複は行を作らない（`.ts`の行と宣言ファイルへのrelatedは残る）
    - augmentationの重複のrelatedの統合
    - diagnosticsの集約でのrelated infoの統合
- conformance：
  - 15,228 configuration、lane A 13,467（変化なし）、803 s。
  - full 13,209→13,241（+32）、text 57→29、category 19、mismatch 138→133、emit full 12,421（12,422から1減ったのは
    下記のharness error）。
  - harness error 44→45：stressの`intersectionConstructorReductionCrash`がrunnerのmemory上限（3,072 MiB）を越えた。
    P3-5rとP3-5sのrelease CLIで同じ設定（strict）にすると、どちらもpeak 3.28 GB・約28 s・出力も同一で、上限の際に
    あるcaseのsamplingの差（本変更による増加ではない）。従来どおりratchetには載せない。
- 上がった36構成（fullへ33、mismatchからtextへ3）：
  - 関係エラーのchain（20）：`arrayCast`、`assignmentCompatWithOverloads`、`assignmentCompatability44`／`45`、
    `classSideInheritance3`、`complexRecursiveCollections`、`excessPropertyCheckWithUnions`、
    `genericCallAtYieldExpressionInGenericCall1`、`invariantGenericErrorElaboration`、`iterableTReturnTNext`の
    strictbuiltiniteratorreturn=true、`nestedCallbackErrorNotFlattened`、`promisePermutations`／`2`／`3`、
    `typeParameterArgumentEquivalence5`、`generatorTypeCheck25`／`62`／`63`、`types.asyncGenerators.es2018.2`、
    `intraExpressionInferences`。
  - tupleの範囲（1）：`variadicTupleMismatch`。
  - overloadの包み方（4）：`bigintWithLib`、`heterogeneousArrayAndOverloads`、`jsxChildrenWrongType`、
    `jsxChildrenArrayWrongType`のtarget=es2015。
  - arrowの`async`のrelated（2）：`errorOnUnionVsObjectShouldDeeplyDisambiguate`／`2`。
  - related infoの統合（4）：`multipleDefaultExports05`と`objectSpreadNegative`のtarget=es2015、
    `jsxSpreadOverwritesAttributeStrict`、`typedefTagWrapping`。
  - 重複宣言（5）：`duplicateIdentifierRelatedSpans_moduleAugmentation`、`exportAsNamespace_augment`、
    `duplicateIdentifierRelatedSpans2`／`4`／`7`（`@pretty`はtextまで）。
- 下がった構成はemitを含めて無い。
- ratchet：0 regressions、5行追加・31行raise（text→full）。`intersectionConstructorReductionCrash`は従来どおり載せない。
- local：
  - formatとworkspace全体のclippy。diagnosticsとその逆依存（types、host、syntax、binder、checker、compiler、
    conformance、emitter、harness、program）のtest（69 targets、3,654 passed）。
  - 2 workerのfull run（803 s）。workspace全体のtestはhostedの`rust` job。
  - filterの段階では、baselineに連鎖した診断を持つ1,063 case、関係エラーを持つ残りの707 case、related infoか
    重複宣言を持つ残りの615 case、tupleのcaseをP3-5rのreportとtierごとに比べた（下降0）。
  - skipされるfileの重複宣言の変更はfull run（`f6f269042`）の後の後続commit。checker、compiler、conformanceのtest
    （11 targets、1,976 passed）、workspace全体のclippy、skipLibCheck／skipDefaultLibCheck／noCheck／`@ts-nocheck`／
    `checkJs: false`を含む164 case（179構成）をfull runのreportとtierごとに比べた（変化0）。最終headの全体の
    conformanceはhostedの`conformance (TypeScript 7.1)`。
  - `--checkers 4`の並列対照は実行していない。
- 残り（lane Aでfullでない226構成：mismatch 133、text 29、category 19、harness error 45）の主なclass：
  - ambient moduleのimport attributes（`declare module "*.ext" with {…}`、約14）
  - node16系のCommonJSの自己名・`#imports`・exports（TS2307／TS1479、14）
  - 正規表現の検証（9）
  - conflict marker（6）
  - インスタンス化の循環（TS5114／TS5115、5）
  - 型の表示（`mixB<typeof A>.(Anonymous class)`、TS2208のconstraint、union順）
  - tsconfigの位置などharnessの行（約6）
- hosted：PR #637（head `0f387f437`、merge `86b5e003a`）、run 37115190226 — `plan` 28s、`rust` 8m11s、`conformance (TypeScript 7.1)` 17m22s、`gates` 13s。最初のhead `f6f269042`のrun 37114103318は`rust`が9m21sで通り、後続commitのpushで新しいrunに替わった。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `b5e50139a`（P3-5rのhead `e7c808588`と同じコードのrelease build）と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 130→137、zod 545→546、Playwright 368→378、TypeScript `src/compiler` 337→348、Next.js 795→770、Effect 545→511、VS Code 3,649→3,511。tsc-rs÷tsgoは0.60〜0.96、peak memory（MB main→本branch）：313→300、1,293→1,289、751→748、288→290、1,314→1,314、1,037→1,048、5,454→5,452。診断の出力は7 corpusともmainと同一。hono・Playwright・`src/compiler`は5 roundsのA/B（main／本branch）でhono 129／125 ms、Playwright 365／373 ms、`src/compiler` 336／330 ms、`TSRS_CHECKERS=1`の命令数はPlaywright 22.30／22.28 G、hono 4.309／4.301 Gなのでノイズ。最初のcommit（`f6f269042`）ではzodのpeak memoryが1,295→1,329 MBに増え、5 roundsの全sampleが分かれた。`TSRS_CHECKERS=1`では同等（867／868 MB）、checker 4個で+16 MB、10個で+40 MBとcheckerの数に比例し、`TSRS_PHASE_TRACE`ではshardのinitで増えていた。2つの`@types/node`の衝突の行（skipLibCheckなので報告されない）をcheckerごとに作っていたためで、後続のcommit（`0f387f437`）のあとはchecker 10個のfootprintがmain 1,246／1,252 MB、本branch 1,248／1,252 MB。退行なし。

## P3-5t package mapの出力先から入力ファイルへの対応をtsgoに合わせる（2026-10-03）

P3-5s後のnode16系のclass（package自己名・`#imports`・exportsが出力先を指すときの解決。tsc-rsはTS1479を報告するか解決して
しまい、tsgoはTS2307）を直した：
- `tryLoadInputFileForPath`（module/resolver.go:879-966）：
  - tsc 6.0はproject rootを推測した（要求元のdirectoryとpackage.jsonの共通directoryと、その親を順に試す）。tsgoは
    推測しない。rootは`rootDir`、無ければconfig fileのdirectory。どちらも無ければTS2209／TS2210（「The project root
    is ambiguous…」）を報告し、unresolvedで探索を終える（importはTS2307になる）。
  - 出力directoryの基点は、config fileがあればcurrent directory、無ければroot（getOutputDirectoriesForBaseDirectory）。
  - 入力拡張子の検査と読み込みは要求全体の拡張子（`r.extensions`）で行い、読み込みが見つからなければ次の拡張子へ
    進む。tsc 6.0は現在のpassの拡張子で調べ、最初に存在したファイルで終えていた。
  - package mapの文字列targetは、入力ファイルの結果が「探索を続ける」でなければそれを返す（unresolvedも含む）。
- `loadFileNameFromPackageJSONField`（1702-1732）：TypeScriptの拡張子を指定子から取った（`resolvedUsingTsExtension`）と
  みなすのは、package.jsonのtargetが`*`で終わるときだけ。tsc 6.0はtargetが拡張子で終わらなければtrueにしたので、
  `"./src/*ts"`でTS5097が出ていた。
- `#imports`のbare targetの入れ子の解決は、外側の解決のstateで行う（`r`のresolveNodeLike、741-765）。その診断
  （TS2210など）は外側の要求に属する。tsc 6.0の移植は入れ子の診断を捨てていた。
- 使われなくなった`has_config_source`と、要求ごとのdirectory（推測用）を削除。
- unit test：
  - host errorの後の要求の復元のtestを、`rootDir`を与えて入力ファイルへの対応を通る形に再pin。
  - tsgoの行にpinした新しいtest：rootもconfigも無いときは`local`がTS2209、`#alias`が入れ子の`#value`からTS2210で、
    どちらもunresolved。config fileがあればそのdirectoryがrootになり、どちらも入力ファイルに解決する。
- conformance：
  - 15,228 configuration、lane A 13,467（変化なし）、792 s。
  - full 13,241→13,256（+15）、text 29、category 19、mismatch 133→118、emit full 12,421→12,429、harness error 45
    （同じ集合）。
- 上がった15構成（すべてfullへ）：
  - 自己名・`#imports`・exports（14）：`nodeNextPackageSelfNameWithOutDir`、`nodeNextPackageSelfNameWithOutDirDeclDir`、
    `nodeAllowJsPackageSelfName`・`nodeModulesAllowJsPackageImports`・`nodeModulesDeclarationEmitWithPackageExports`の
    module=node16／node18／node20／nodenext。emitは`nodeModulesAllowJsPackageImports`と
    `nodeModulesDeclarationEmitWithPackageExports`の8構成がfullへ。
  - `resolvedUsingTsExtension`（1）：`packageJsonImportsWildcardNoCrash`。
- 下がった構成はemitを含めて無い。
- ratchet：0 regressions、15行追加。`intersectionConstructorReductionCrash`は従来どおり載せない。
- local：
  - formatとworkspace全体のclippy。programとその逆依存（checker、compiler、conformance、emitter、harness）のtest
    （46 targets、3,219 passed）。
  - 2 workerのfull run（792 s）。workspace全体のtestはhostedの`rust` job。
  - filterの段階では、package.jsonに`exports`／`imports`を持つ126 case（307構成。すべてerrorがfull）をP3-5sのreportと
    tierごとに比べた（下降0）。挙動はtsgoのCLI（traceResolution）でも確かめた。
  - `--checkers 4`の並列対照は実行していない。
- 残り（lane Aでfullでない211構成：mismatch 118、text 29、category 19、harness error 45）の主なclass：
  - ambient moduleのimport attributes（`declare module "*.ext" with {…}`、7.1の新しい構文、15）
  - 正規表現の検証（9）：tsgoのregexp.goは、名前付きgroupの重複をdisjunctionごとに集めて外側へ伝える、非unicode modeで
    BMP外の文字をsurrogateに分けても位置は文字の先頭、量指定子の上限を10進文字列で比べる、pattern modifierと重複した
    名前付きgroupのES2025の行、など。
  - conflict marker（6）
  - インスタンス化の循環（TS5114／TS5115、5）
  - 型の表示（`mixB<typeof A>.(Anonymous class)`、TS2208のconstraint、union順）
  - tsconfigの位置などharnessの行（約6）
  - `composite`のemit（tsc-rsは`composite`のemitを扱わず、`nodeNextPackageSelfNameWithOutDirDeclDirComposite`などのemitが
    空になる）
- hosted：PR #638（head `c632036d7`、merge `4b1427b9e`）、run 37117376522 — `plan` 22s、`rust` 9m17s、`conformance (TypeScript 7.1)` 15m52s、`gates` 12s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `efb8af16e`（P3-5sのhead `0f387f437`と同じコードのrelease build）と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 130→142、zod 538→551、Playwright 397→393、TypeScript `src/compiler` 356→354、Next.js 774→809、Effect 532→545、VS Code 3,542→3,553。tsc-rs÷tsgoは0.58〜0.94、peak memory（MB main→本branch）：320→311、1,294→1,295、801→805、289→288、1,320→1,328、1,036→1,046、5,449→5,448。診断の出力と読み込んだdocument数は7 corpusともmainと同一。honoとNext.jsは5 roundsのA/B（main／本branch）でhono 129／129 ms・CPU 619／610 ms、Next.js 739／745 ms・CPU 4,494／4,451 ms、`TSRS_CHECKERS=1`の命令数はhono 4.307／4.305 G、Next.js 48.45／48.19 Gなのでノイズ。退行なし。

## P3-5u 正規表現の検証をtsgoに合わせる（2026-10-03）

P3-5t後の正規表現のclass（9構成）を、tsgoの`scanner/regexp.go`と`scanner.go`の該当部分に合わせた。tsc-rsの検証は
tsc 6.0の移植で、UTF-16の単位で走査する：
- 名前付きgroup（regexp.go:156-184、500-522）：
  - alternativeごとに名前のscopeを持ち、disjunctionの全alternativeの名前を合わせて、group内なら外側のalternativeの
    scopeに加える。入れ子のgroupで定義した名前が、同じalternativeの後のgroupと衝突する（TS1515）。
  - 排他的なalternativeでの重複は、ES2018以上ES2025未満でTS18063（「Duplicate named capturing groups are only
    available when targeting 'es2025' or later」）。
  - group名はtsgoの`scanIdentifier`（RegExpGroupName）で読む。先頭も含めて`\u`のescape（波括弧付きも、flagに
    関係なく）と、波括弧の無い`\uHigh\uLow`の組を受け付ける。tsc 6.0の移植は先頭のescapeを受け付けず、TS1514・
    TS1538などを報告していた。
- pattern modifier（268-284）：修飾文字を読んだ`(?flags:`／`(?flags-flags:`は、ES2025未満でTS18062（その文字の範囲）。
  subpattern内では個々のflagの対象versionを調べない。正規表現のflagの対象versionはd・s・vだけ（45-49。tsc 6.0の
  u・yのes6は無い）。
- 量指定子の上限（291-333）：10進の文字列として比べる（compareDecimalStrings）。tsc 6.0の移植は浮動小数点で比べ、
  2^53を越える数の順序を誤っていた。
- 文字集合（v flag、593-769）：
  - 最初のoperandの後の単独の`-`／`&`は何もしない。loopの`&`は`&&`のときだけ演算子で、単独の`&`は普通の文字
    （tsc 6.0の移植はTS1508を報告していた）。
  - 範囲の分岐の後もloopを続ける（tsc 6.0のJavaScriptの`break`はswitchを抜けるだけだが、移植はloopを抜けていた）。
  - unionのoperandごとに、否定の集合での文字列の可能性（TS1518）を調べ、文字列の可能性を合わせる。
  - 差の集合は最初のoperandの文字列の可能性を保ち、積の集合だけが論理積をとる。
- BMP外の文字（1016-1057）：Unicode modeでなければ2つのatomで、上位surrogateは位置を進めずに返し、下位surrogateで
  文字全体を進める。どちらのatomも文字の先頭から始まるので、範囲の順序の誤り（TS1517）は文字の先頭の列になる。
- escape（scanner.go:1689-1851）：`\u{`の後に16進数字が無ければすぐ戻る（`}`を探さない）。BMP外の文字のidentity
  escapeは1文字。Unicode modeで波括弧の無い`\uHigh\uLow`は、正規表現の末尾でも1文字（tsc 6.0の`pos + 6 < end`は
  1つずれていた）。
- 句読点との比較で、16bitの単位を`u8`に切り詰めていた（U+012Fが`/`、U+0126が`&`に一致した）のをASCIIだけに限った。
- unit test：
  - 再pin：`/\u{-DDDD}/gu`（TS1125とTS1508。TS1199は無い）、u・yのflagの対象version（無い）、checkerのES5の`u`の行
    （無い）。
  - tsgoの行にpinした新しいtest 4件（名前付きgroupとmodifier、量指定子の上限、文字集合、BMP外の文字とgroup名の
    escape）。tsgoのCLIで同じ正規表現を3つのtargetで比べ、80行がすべて一致した。
- conformance：
  - 15,228 configuration、lane A 13,467（変化なし）、808 s。
  - full 13,256→13,266（+10）、text 29、category 19、mismatch 118→108、emit full 12,429（変化なし）、harness error 45
    （同じ集合）。
- 上がった10構成（すべてfullへ）：`regexNamedGroupDuplicateNestedInGroup`、`regularExpressionCharacterClassRangeOrder`、
  `regularExpressionES2025Syntax`のtarget=es2017／es2022、`regularExpressionGroupNameUnicodeEscapes`、
  `regularExpressionQuantifierBounds1`、`regularExpressionScanning`のtarget=es2015／esnext、
  `regularExpressionUnicodeSetsLoneAmpersand`、`negatedUnicodeSetUnionMayContainStrings`。
- 下がった構成はemitを含めて無い。
- ratchet：0 regressions、10行追加。`intersectionConstructorReductionCrash`は従来どおり載せない。
- local：
  - formatとworkspace全体のclippy。syntaxとその逆依存（binder、checker、compiler、conformance、emitter、harness、
    program）のtest（60 targets、3,538 passed）。
  - 2 workerのfull run（808 s）。workspace全体のtestはhostedの`rust` job。
  - filterの段階では、正規表現を含む218 case（303構成）をP3-5tのreportとtierごとに比べた（上昇10、下降0）。
  - `--checkers 4`の並列対照は実行していない。
- 残り（lane Aでfullでない201構成：mismatch 108、text 29、category 19、harness error 45）の主なclass：
  - ambient moduleのimport attributes（`declare module "*.ext" with {…}`、7.1の新しい構文、15）
  - conflict marker（6）：tsc-rsのscannerはmerge conflict markerを認識しない（tsgoのisConflictMarkerTrivia／
    scanConflictMarkerTrivia、scanner.go:2402-2470）。
  - インスタンス化の循環（TS5114／TS5115、5）
  - 型の表示（`mixB<typeof A>.(Anonymous class)`、TS2208のconstraint、union順）
  - tsconfigの位置などharnessの行（約6）
  - `composite`のemit
- hosted：PR #639（head `102910e4c`、merge `48013c69d`）、run 37119793606 — `plan` 33s、`rust` 9m50s、`conformance (TypeScript 7.1)` 22m14s、`gates` 13s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `c48e4c4ee`（P3-5tのhead `c632036d7`と同じコードのrelease build）と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 146→145、zod 556→560、Playwright 391→398、TypeScript `src/compiler` 350→360、Next.js 811→792、Effect 559→519、VS Code 3,919→3,806。計測時のload averageが高く（4〜7）絶対値は前の記録より大きいが、同じroundの交互実行で比べている。tsc-rs÷tsgoは0.56〜0.93、peak memory（MB main→本branch）：322→306、1,289→1,289、811→801、289→288、1,326→1,315、1,037→1,026、5,446→5,461。診断の出力と読み込んだdocument数は7 corpusともmainと同一。TypeScript `src/compiler`とPlaywrightは5 roundsのA/B（main／本branch）で331／332 ms、364／358 ms、`TSRS_CHECKERS=1`の命令数は15.94／15.94 G、22.29／22.28 Gなのでノイズ。退行なし。

## P3-5v merge conflict markerをtsgoと同じく走査する（2026-10-03）

tsc-rsのscannerはmerge conflict markerのtriviaを持たず、`<<<<<<<`などを演算子として読んでいた（conflictMarker*の
6構成）。tsgo（scanner/scanner.go:765-871、1277-1283、2402-2470）に合わせた：
- 行頭で同じ文字が7つ並び、`<<<<<<<`・`|||||||`・`>>>>>>>`なら空白が続くもの、`=======`はそのままのものがmarker。
  TS1185「Merge conflict marker encountered.」を7文字に報告する。
- `<`・`>`のmarkerは行末まで、`|`・`=`のmarkerは次の`=======`か`>>>>>>>`のmarkerまでを飛ばす。最初の
  alternativeだけが残る。
- JSXのtextではmarkerを1つのtokenとして返す（要素は閉じtagを欠く）。triviaを飛ばす関数もmarkerを飛ばす。
- 報告は木を欠かさない（markerはtrivia）ので、文字列literalだけの回復と同じくemitを妨げない
  （`ParseDiagnosticOrigin::ScannerTrivia(ConflictMarkerTrivia)`）。

## P3-5w インスタンス化の循環を型の名前で報告する（2026-10-03）

tsgoはインスタンス化の深さの上限で、tsc 6.0のTS2589の代わりに循環している型を名前で報告する（5構成）：
- tsc-rsの`instantiation_depth`をtsgoの`instantiationStack`（インスタンス化中の型の列、checker.go:598、22494-22544）に
  置き換えた。長さが深さで、推測の巻き戻しは列を切り詰める。
- 上限（深さ100か、1つの文・式で5,000,000回）では、列で3回目に現れた型を、alias symbolか型のsymbolの名前で、列の
  順に集める（getCircularTypeNames、22546-22564）。内部の名前（tsgoの`\xFE`。tsc-rsでは`___`でない`__`で
  始まる名前）は除く。1つならTS5114「Instantiations of type 'X' appear infinitely circular.」、複数なら
  TS5115「Instantiations of the following types appear infinitely circular: 'A', 'B'.」、無ければTS2589。
- 名前はsymbolToStringで作る。その間は列を退けておく（上限の下で動かす）。
- unit test：
  - tsgoの行にpinした新しいtest：conflict marker（diff3を含むclass、markerでないもの、triviaを飛ばす関数）とJSXの
    textのmarker、TS5114。
  - 推測の巻き戻しのtestは列で確かめる。再帰的に広がるunionのtest（recursivelyExpandingUnionNoStackoverflow）は、
    深さの上限の行がTS5114になったのに合わせた。tsgoはこのcaseで上限に達しない（TS2615だけ）ので、この行は既知の
    違いとして残る。
- conformance（P3-5vとP3-5wを1回で）：
  - 15,228 configuration、lane A 13,467（変化なし）、805 s。
  - full 13,266→13,275（+9）、text 29、category 19、mismatch 108→100、emit full 12,429→12,435、harness error
    45→44。
  - harness errorの1減はstressの`intersectionConstructorReductionCrash`が今回はmemoryの上限内で終わったため（上限の
    際にあるcase）。fullになったが、従来どおりratchetには載せない（`--update`の後に手で除いた）。
- 上がった構成（すべてfullへ）：
  - conflict marker（6）：`conflictMarkerTrivia1`～`4`、`conflictMarkerDiff3Trivia1`／`2`。emitは`conflictMarkerTrivia3.tsx`
    以外の5構成がfullへ。`conflictMarkerTrivia3.tsx`のemitは、parserのTS1005（`'</' expected`）がemitを止める一般の
    class。
  - インスタンス化の循環（2）：`limitDeepInstantiations`、`recursiveConditionalCrash4`。
- 対象の残り3構成：`recursiveMappedTypes`はTS5114の行が合い、最初の違いは別のTS2615の位置（73行と79行）に移った。
  `mutuallyRecursiveInference`と`keyofGenericExtendingClassDoubleLayer`は、tsc-rsが上限に達しない（tsgoは達する）。
  評価の順序の違いで、本sliceの範囲外。
- 下がった構成はemitを含めて無い。
- ratchet：0 regressions、8行追加。
- local：
  - formatとworkspace全体のclippy。syntaxとその逆依存（binder、checker、compiler、conformance、emitter、harness、
    program）のtest（60 targets、3,541 passed）。
  - 2 workerのfull run（805 s）。workspace全体のtestはhostedの`rust` job。
  - filterの段階では、markerの行を持つ6 caseをP3-5tのreportとtierごとに比べた（errorの上昇6、emitの上昇5、下降0）。
    markerの各形、markerでないもの、JSXのtextはtsgoのCLIでも比べ、行と出力したJavaScriptが一致した。
  - `--checkers 4`の並列対照は実行していない。
- 残り（lane Aでfullでない192構成：mismatch 100、text 29、category 19、harness error 44）の主なclass：
  - ambient moduleのimport attributes（`declare module "*.ext" with {…}`、7.1の新しい構文、15）
  - 型の表示（`mixB<typeof A>.(Anonymous class)`、TS2208のconstraint、union順）
  - tsconfigの位置などharnessの行（約6）
  - `composite`のemit、parse errorの後のemit
- hosted：PR #640（head `eca775c97`、merge `d5d487b37`）、run 37123173839 — `plan` 27s、`rust` 9m22s、`conformance (TypeScript 7.1)` 23m20s、`gates` 12s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `694358426`（P3-5uのhead `102910e4c`と同じコードのrelease build）と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 137→135、zod 532→532、Playwright 371→372、TypeScript `src/compiler` 333→349、Next.js 796→776、Effect 526→539、VS Code 3,549→3,505。tsc-rs÷tsgoは0.59〜0.98、peak memory（MB main→本branch）：320→297、1,287→1,292、808→808、290→289、1,316→1,325、1,029→1,015、5,430→5,443。診断の出力と読み込んだdocument数は7 corpusともmainと同一。TypeScript `src/compiler`とEffectは5 roundsのA/B（main／本branch）で326／330 ms・CPU 992／993 ms、529／521 ms、`TSRS_CHECKERS=1`の命令数は15.96／15.96 G、27.96／27.96 Gなのでノイズ（インスタンス化の列のpush／popは計測に表れない）。退行なし。

## P3-5x ambient moduleのimport attributes型をtsgoと同じく扱う（2026-10-03）

TypeScript 7.1は`declare module "*.css" with { type: "css" } { … }`のようにpattern ambient moduleへimport attributesの型を
持たせ、importのattributesで解決先を選ぶ（7.1の新しい構文、15構成）。tsgoに合わせた：
- 構文：`global`でないambient moduleの名前の後では、`with`に続けて型literalを読む（parser.go:2218-2244）。
  `ModuleDeclarationData.attributes`として名前と本体の間の子にし、factory、d.tsの変換、printer（printer.go:3841-3846）も
  通す。
- binder：
  - attributesを持つpattern moduleは、宣言ごとに別の内部名（`__"<pattern>"pattern@<file>#<node>`、tsgoでは
    `\xFE"<pattern>"pattern@<nodeId>`）で束縛する（binder.go:301-317）。
  - patternでない名前にattributesがあればTS1550。
- checker：
  - 同じpatternでattributes型が同一のmoduleを1つにまとめる（mergePatternAmbientModules、checker.go:1408-1432）。
    augmentationは対象のmoduleを記録し、その対象に解決したときだけ使う。
  - module名の解決はimport・export・import型・`import()`のattributes型を受け取る
    （getImportAttributesTypeForModuleSpecifier）。それを受け入れるpattern moduleから、最も厳しい型、次に最長の
    prefixのものを選ぶ（tryResolvePatternAmbientModule、15689-15745）。attributesの無いimportは、解決済みのmoduleが
    あればそれを使う。
  - `import()`の引数に書いた`with`のobjectはconst contextで、literal型のまま選ぶ（isInlineImportAttributes）。
  - attributes型の中の名前は、moduleの外側のscopeで解決する（nameresolver.go:48-50、110-112）。
  - TS1551（augmentationのattributes）、TS1552〜TS1558（attributes型の文法）、global `ImportAttributes`への代入
    可能性（TS2322）。
- 表示とd.ts：
  - source fileを持たないmoduleは、文字列の名前を持つ最初の宣言の名前で書く（`import("*.style")`）。tsc 6.0の
    `declare module ""`のfile名へのfallbackは無くなり、`import("")`になる。
  - import型には、そのmoduleのattributes型の文字列literalのpropertyを名前順で添える
    （`import("*.style", { with: { type: "css" } })`）。説明している宣言自身のimportのattributesでも同じmoduleに
    解決するなら、そちらを使う（getSpecifierForModuleSymbol・createImportAttributesForModuleSpecifier、
    nodebuilderimpl.go:1249-1398）。診断の表示とd.tsのnode builderの両方。
  - d.tsのimport・export宣言はattributesを書いたまま残す（transform.go:1172-1179、2478-2560）。tsc 6.0は有効な
    `resolution-mode`だけを残した。
  - 宣言の初期化子の`import()`もmodule specifierを持つ宣言として扱う（tryGetModuleSpecifierFromDeclaration）。
- program：type-onlyのexport宣言も、type-onlyのimportと同じく`resolution-mode`のattributeでmodeを決める
  （getModeForUsageLocation、fileloader.go:1049-1066）。`importAttributeTypeOnlyImports`のharness errorが消えた。
- unit test（すべてtsgoの出力にpin）：
  - checker：attributesによるpattern moduleの選択と`{ with: { … } }`を添えたTS2339の表示、外側のaliasを使う
    attributes型、`import()`のinlineの`with`と変数のoptions。attributes型の文法の行（TS1550、TS1551、TS1552、
    TS1555〜TS1558、`ImportAttributes`へのTS2322）。
  - d.ts：attributes付きのimport型（dynamic importのmodule型を含む）、attributesを残すimport・export宣言、
    attributes型を持つambient module宣言の出力。
  - program：type-onlyのexportのrequestのmode。parser：`attributes`の子。
  - `declare module ""`のtestはtsgoの`typeof import("")`に合わせた。
- conformance：
  - 15,228 configuration、lane A 13,467（変化なし）、800 s。
  - full 13,275→13,291（+16）、text 29、category 19→18、mismatch 100→86、emit full 12,435→12,462、harness error
    44→43。
  - stressの`intersectionConstructorReductionCrash`は今回もfullで終わったが、従来どおりratchetには載せない
    （`--update`の後に手で除いた）。
- 上がった構成：
  - errorがfullへ（16）：
    - 対象の15構成すべて（`importAttributeTypeOnlyImports`はharness errorから）。
    - `importAttributes9`（categoryから）。
  - emitがfullへ（27）：
    - 対象の3構成：`declarationEmitPatternAmbientModuleImportAttributes`、`importAttributesDeclarationEmit`、
      `importAttributeTypeOnlyImports`。
    - d.tsでattributesを残す24構成：`importAssertion1`～`3`、`importAttributes1`～`3`、
      `nodeModulesImportAttributesModeDeclarationEmitErrors`、`nodeModulesImportModeDeclarationEmitErrors1`。
- 下がった構成はemitを含めて無い。
- ratchet：0 regressions、15行追加、25行引き上げ。
- local：
  - formatとworkspace全体のclippy。syntaxとその逆依存（binder、checker、compiler、conformance、emitter、harness、
    program）のtest（60 targets、3,547 passed）。
  - 2 workerのfull run（800 s）。workspace全体のtestはhostedの`rust` job。
  - filterの段階では、import attributes・import assertion・`resolution-mode`を書いたcaseと、declaration emitの
    `import()`を持つcase（423 case、625構成）をP3-5vのreportとtierごとに比べた（errorの上昇15、emitの上昇26、
    下降0）。attributesによる解決と表示、文法の行、d.tsの出力はtsgoのCLIでも比べ、行と出力が一致した。
  - `--checkers 4`の並列対照は実行していない。
- 残り（lane Aでfullでない176構成：mismatch 86、text 29、category 18、harness error 43）の主なclass：
  - compilerのcrashとhang（harness error約25：tsgoの修正がある`unreachableFlowAfterThrowing*`、分割代入の
    narrowing、`for…of`の自己参照、計算されたenum memberの名前、表示の再帰など）
  - 型の表示（`mixB<typeof A>.(Anonymous class)`、TS2208のconstraint、union順）
  - tsconfigの位置などharnessの行（約6）
  - `composite`のemit、parse errorの後のemit、import型のattributesの値の文法（TS2858、`importAttributes12`）
- hosted：PR #641（head `c73350466`、merge `45d06373d`）、run 37128054037 — `plan` 28s、`rust` 7m59s、`conformance (TypeScript 7.1)` 23m1s、`gates` 16s。`c73350466`は振る舞いを変えないhot pathの修正で、localでは同じimport attributesのcase群（625構成）の行と出力のhashが一致した。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `84692ca7e`（P3-5v／P3-5wのhead `eca775c97`と同じコードのrelease build）と本branchのhead `c73350466`のrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 145→141、zod 524→524、Playwright 370→363、TypeScript `src/compiler` 346→350、Next.js 767→791、Effect 500→541、VS Code 3,523→3,524。tsc-rs÷tsgoは0.59〜1.00、peak memory（MB main→本branch）：319→313、1,289→1,288、808→752、289→289、1,308→1,312、1,032→1,041、5,444→5,442。診断の出力と読み込んだdocument数は7 corpusともmainと同一。最初のbuild（`2517f726b`）では`TSRS_CHECKERS=1`のEffectの命令数が+0.47%（5回のmedianで27.894→28.025 G）だった。名前解決の各scope、`isConstContext`の各node、各module解決で新しい検査が走っていたため、`c73350466`でそれぞれ必要な所だけに絞り、27.969／27.972 G（main／本branch）に戻した。Next.jsは5 roundsのA/Bで738／740 ms、Effectは10 roundsのA/Bで516／509 ms・CPU 3,278／3,265 msなので、3 roundsの差はwork stealingの割り振りによるノイズ。退行なし。

## P3-5y compilerのcrashとhangをtsgoの修正どおりに直す（2026-10-03）

tsc-rsはharness errorの約20構成で、stackのoverflow、hang、panic、memoryの上限超過を起こしていた。多くは
`*NoCrash*`のようにtsgo（TypeScript 7.1）がcrashを直したときに足したcaseで、tsgoの修正を移植した：
- binder：
  - `for`の初期化子、`for…in`／`for…of`の式でflowが到達不能になったら、loopのflowを作らずに残りを結び
    （binder.go:1886-1928）、出口の無い循環を作らない（`unreachableFlowAfterThrowing*`の8構成）。
- checker：
  - 分割代入の要素は、それ自身のroot宣言の初期化子の中（同じflow container）では絞り込まない
    （getNarrowedTypeOfSymbol）。`circularDestructuring`、`dependentDestructuredVariables*`。
  - `tryGetNameFromEntityNameExpression`は分割代入の要素を除く（flow.go:1767-1770、TypeScript issue 63192、
    `infiniteRecursionDestructuringLoop`）。
  - getExplicitTypeOfSymbolは解決中のsymbolを覚え、再び来たら型無しとする（`resolvingExplicitTypeOfSymbol`、
    `for (const a of a)`）。
  - enumの型は動的な名前のmemberを構文だけで除く（`ast.HasDynamicName`。tsc 6.0のlate-bindな名前は自身の
    memberに戻って循環した、`computedEnumMemberKeyNoCrash1`）。
  - 別のfileで宣言された分割代入の要素を絞り込むとき、flowの探索は使う側のfileのflowを使う（それまでは宣言側の
    fileのflow arenaを引いてpanic、`dependentDestructuringCrossFilePosition`）。
  - 旧decoratorのmethodの引数の数はgetParameterCountで数える（restのtupleを展開する、`decoratorRestNoCrash1`の
    panic）。
  - template literalの照合（isTypeMatchedByTemplateLiteralType）は、関係の検査の中ではその検査自身の比較
    （isRelatedToWorker）を使う（relater.go:3616）。それまでは毎回新しい代入可能性の検査で、深さの上限が働かず
    stackを使い切った（`varianceComputationNoCrash`）。比較を渡すtrait（`TemplateTypeComparer`）を足した。
  - computeBaseConstraintの条件型の制約の入れ子を100で止める（`conditionalConstraintDepth`、checker.go:28027-28034、
    issue 63269、`infiniteConstraints2`のhang）。
  - template literal型は、文字列がUTF-8で50,000,000 byteか、placeholderが100,000個を超えたらTS2589と誤りの型にする
    （checker.go:29610-29672、issue 63271、`templateLiteralTypeExcessiveLength`のmemory超過）。
- 表示：
  - instantiation式の型の`typeof`のnodeを再利用できないとき、同じ型に戻る再帰を訪問済みの印で止める
    （nodebuilderimpl.go:2921-2934、`symbolToNodeBoundaryNoStackOverflow`）。
- unit test（tsgoの出力にpin）：
  - checker：throwするloopの頭、自身を読む分割代入、`for (const a of a)`、別fileの分割代入の絞り込み、計算された
    enum memberの名前、restのtupleを持つdecoratorの入力で、tsgoと同じlibraryでの行。
  - types：template literal型の上限（placeholderの数、UTF-8 byteの文字列の長さ）。
  - 計算されたenum memberの名前のtestは、両方を計算されたenum型とするtsgoに合わせた（tsgoではどちらも`0`に
    代入できない）。
- conformance：
  - 15,228 configuration、lane A 13,467（変化なし）、450 s（120 sのtimeoutで待つcaseが無くなり、前の800 sから短縮）。
  - full 13,291→13,309（+18）、text 29、category 18→19、mismatch 86→88、emit full 12,462→12,481、harness error
    43→22。
  - harness errorから比べられるようになった22構成：fullが19、categoryが1（`dependentDestructuredVariablesNoCrash1`：
    TS2488の型の表示で、引数の省略可能性を含めるかが解決の順序で違う）、mismatchが2：
    - `infiniteConstraints2`：hangは無くなったが、tsc-rsはtsgoに無いTS2589を報告する。
    - `templateLiteralTypeExcessiveLength`：3行のうち2行が合う。3行目はtsc-rsが型parameterの制約を制約のnodeより
      先に解決し、位置が型parameterになる。
  - stressの`intersectionConstructorReductionCrash`は今回はmemoryの上限を越えた（ratchetには載せていない）。
  - ほかの構成はemitを含めて上がりも下がりも無い。
- ratchet：0 regressions、20行追加。
- local：
  - formatとworkspace全体のclippy。typesとsyntaxとその逆依存（binder、checker、compiler、conformance、emitter、
    harness、program）のtest（63 targets、3,591 passed）。
  - 2 workerのfull run（450 s）。workspace全体のtestはhostedの`rust` job。
  - crashした各caseはdevのrunnerで1つずつ確かめ、小さな入力はtsgoのCLIとも比べた（行が一致）。
  - `--checkers 4`の並列対照は実行していない。
- 残り：
  - `excessivelyDeepConditionalTypes`のmemory超過。tsgo側の修正（typescript-go issue 2917）を特定できていない。
  - 上の2つのmismatchとcategory 1つ。
  - lane Aでfullでない158構成（mismatch 88、text 29、category 19、harness error 22）の主なclass：
    - `--pretty`の出力（関連spanの`duplicateIdentifierRelatedSpans*`、`pretty*`など約10）
    - 型の表示
    - tsconfigの位置などharnessの行
    - `composite`のemit、parse errorの後のemit
- hosted：PR #642（head `9c4cc4494`、merge `47a718ad7`）、run 37131470020 — `plan` 27s、`rust` 9m59s、`conformance (TypeScript 7.1)` 12m38s（120 sのtimeoutで待つcaseが無くなり、前の約23分から短縮）、`gates` 11s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `b7ca12d80`（P3-5xのhead `c73350466`と同じコードのrelease build）と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 136→138、zod 516→513、Playwright 360→368、TypeScript `src/compiler` 331→344、Next.js 771→754、Effect 522→518、VS Code 3,516→3,452。tsc-rs÷tsgoは0.58〜0.99、peak memory（MB main→本branch）：301→319、1,282→1,297、798→798、287→288、1,317→1,311、1,039→1,038、5,446→5,440。診断の出力と読み込んだdocument数は7 corpusともmainと同一。TypeScript `src/compiler`とPlaywrightは5 roundsのA/B（main／本branch）で318／316 ms、349／348 ms、TypeScript `src/compiler`の`TSRS_CHECKERS=1`の命令数は15.948／15.941 G（5回のmedian）なのでノイズ。退行なし。

## P3-5z `--pretty`の出力をtsgoのdiagnostic writerに合わせる（2026-10-04）

P3-5iで`@pretty`のbaselineはheader行を読んでtext tierまで比べるようにしたが、Full tierは評価していなかった。
tsc-rsのpretty出力はtsc 6.0.3の`formatDiagnosticsWithColorAndContext`の移植で、色の無いlayoutをCLIが後から
行ごとに色付けしていた。tsgo（TypeScript 7.1）のdiagnostic writer（`internal/diagnosticwriter`）を
`tsc-rs-diagnostics`に移植し、CLIとconformanceのrunnerが同じ実装を使う：
- `FormatDiagnosticWithColorAndContext`（diagnosticwriter.go:229-262）：
  - related locationは`  file:line:col - message`の後にsnippetを書く（6.0はlocation、snippet、改行してmessage）。
  - 色はwriterが書く：`WriteLocation`の名前はcyan、行と列はyellow、categoryの色、灰色の`TS<code>: `、gutterは反転。
- `writeCodeSnippet`（264-345）：
  - 行末はGoの`unicode.IsSpace`で削る（U+0085を削り、U+FEFFは残す。6.0はJavaScriptの空白）。
  - 長さ0のspanは次の1文字に`~`を引く（6.0は引かない）。数はUTF-16単位で、行の長さで切り詰めない。
  - 開始位置の前は空白で埋める（6.0は元の空白文字を残した）。
- `WriteErrorSummaryText`（`Found N errors…`）と`writeTabularErrorsDisplay`：fileごとの最初のerrorの行を灰色の
  `:line`で添える（`prettyPathForFileError`）。fileはGoの文字列の順（UTF-8 byte順）に並べる。
- runner：`@pretty`のbaselineはpretty診断、file section、error summaryの順に描き、Full tierも評価する。
- CLI：
  - `--pretty`の既定はtsgoの`defaultIsPretty`（`FORCE_COLOR`、空でない`NO_COLOR`、`TERM=dumb`、stdoutが端末か）。
    6.0の移植はstdoutが端末かだけを見ていた。
  - `-p`の指すものが無いときのTS5058は正規化した絶対pathで書き、`tsconfig.json`の無いdirectoryはTS5081
    （`Cannot find a tsconfig.json file at the current directory: <dir>/tsconfig.json`、tsc.go:165-180）。tsc 6.0は
    書いたままのpathとTS5057だった。
- checker：namespace-styleのimportを呼んだり代入したりしたerrorに、そのimportを指すTS7038の関係情報を足す：
  - checkTypeRelatedTo（relater.go:383-392）：head messageのある関係のerrorで、sourceの型のsymbolが
    namespace-styleのimport（`import()`でない）から来ていて、importが表すmoduleの型なら関係が成り立つとき。
  - invocationErrorRecovery（checker.go:10197-10211）：呼び出しや`new`のerrorで、moduleの型がその種類の
    signatureを持つとき。
- unit test（tsgoの出力にpin）：
  - diagnostics：related情報とchain、5行を超えるsnippet、長さ0のspan、Goの空白、file無しの診断、error summaryの
    各形。
  - conformance：vendorの`multiLineContextDiagnosticWithPretty.errors.txt`と同じbytes。
  - compiler（CLI）：重複宣言の3 fileのprojectとTS7038のprojectで、tsgoのCLIの出力とbyte単位で同じ。TS5058と
    TS5081の絶対path。
- conformance：
  - 15,228 configuration、lane A 13,467（変化なし）、447 s。
  - full 13,309→13,324（+15）、text 29→14、category 19、mismatch 88、emit full 12,481（変化なし）、harness error 22。
  - 上がった15構成はすべてtext→full：`@pretty`の14構成（`duplicateIdentifierRelatedSpans1`〜`7`、
    `deeplyNestedAssignabilityIssue`、`esModuleInteropPrettyErrorRelatedInformation`、`manyCompilerErrorsInTheTwoFiles`、
    `multiLineContextDiagnosticWithPretty`、`prettyContextNotDebugAssertion`、`prettyFileWithErrorsAndTabs`、
    `typedefCrossModule5`）と`invocationErrorRecovery`（TS7038）。
  - 下がった構成は無い。`@pretty`でない構成で描いたbaselineのdigestが変わったのは`invocationErrorRecovery`だけで、
    emitのdigestはすべて同じ。
- ratchet：0 regressions、15行raise（追加なし）。
- local：
  - formatとworkspace全体のclippy。diagnosticsとその逆依存（types、syntax、binder、host、program、emitter、checker、
    compiler、harness、conformance）のtest（69 targets、3,670 passed）。workspace全体のtestはhostedの`rust` job。
  - 2 workerのfull run（447 s）。
  - release buildのCLIとtsgoの比較（stdout・stderr・終了code）：TS2322、TS2688（関係情報付きのfile無し診断）、
    3 fileの重複宣言、TS7038の各projectを`--pretty true`／`false`で、TS5058／TS5081の4つの`-p`を両方で（16組）、
    `--pretty`無しで`FORCE_COLOR`・`NO_COLOR`・`TERM`の10通り。すべてbyte単位で同じ。
  - `--checkers 4`の並列対照は実行していない。
- 残り：
  - lane Aでfullでない143構成（mismatch 88、category 19、text 14、harness error 22。harness errorのうち15は
    content mapper（`runExternalCode`）で対象外）。
  - emitの不一致956構成で最大のclassはparse errorの後のemit（500構成。preflightが`emit recovery … deferred to
    H2.9`で断る）。
- hosted：PR #643（head `05f4e147d`、merge `ce2a4250b`）、run 37134212482 — `plan` 26s、`rust` 9m12s、`conformance (TypeScript 7.1)` 19m21s、`gates` 11s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `ae05d6b30`（P3-5yのcode `8e3d62acc`と同じcodeのrelease build）と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 146→138、zod 548→515、Playwright 365→371、TypeScript `src/compiler` 337→341、Next.js 772→750、Effect 536→522、VS Code 3,524→3,465。tsc-rs÷tsgoは0.58〜0.94、peak memory（MB main→本branch）：324→323、1,294→1,292、761→800、289→288、1,313→1,313、1,055→1,024、5,443→5,468。診断の出力と読み込んだdocument数は7 corpusともmainと同一。PlaywrightとTypeScript `src/compiler`は5 roundsのA/B（main／本branch）で359／358 ms（peak memory 811／777 MB）、322／322 ms、`TSRS_CHECKERS=1`の命令数（5回のmedian）はPlaywright 22.294／22.297 G、TypeScript `src/compiler` 15.927／15.925 Gなのでノイズ。退行なし。

## P3-5aa parse errorのあるfileもtsgoと同じくemitする（2026-10-04）

lane Aのemitの不一致956構成のうち500構成は、tsc-rsがparse errorのあるfileを出力しなかったもの（emitの
preflightが`emit recovery … deferred to H2.9`で断る）。CLIもそのようなfileがあるとcompile全体を
「compiler failure」で止め、診断も出さなかった。tsgo（TypeScript 7.1）はparserが回復したtreeをそのまま
変換して出力する。preflightの拒否（6.0.3の出力と照合した回復だけを通すH2期のadmission）を外し、回復した
treeの出力でtsgoと違った点を直した：
- parser：区切りlistの末尾のcommaはtsgoの`NodeList.HasTrailingComma`（listが最後の要素の後で終わる、
  ast.go:139-145）。object literalのmemberの区切りに使った`;`が最後の要素の後にあれば末尾のcommaになる
  （6.0は実際にcommaを読んだときだけ）。
- printer：
  - 欠けたtemplate literalのtoken（閉じていない`${`の後のTemplateTail）は何も書かない（getLiteralTextは
    sourceの文字列を読む）。
  - 空のblockは、範囲の終わりが始まり（triviaを飛ばした位置）と同じ行にあるときだけ1行で書く（isEmptyBlock）。
  - 欠けたblock（`{`の無い`try`など）は、tscのemitTokenWithCommentどおり`{`と`}`を次のtokenの位置に置き、
    その1文字後のtrailing commentを`{`と`}`の後にそれぞれ書く。
- 変換：
  - constructorとsetterのreturn型とtype parameter、getterのtype parameterを消す（typeeraser.go:148-186）。
    tsc 6.0のupdaterはそれらを元のnodeから戻し、JavaScriptにも書いていた（parse errorの無いfileでも、
    TS1093／TS1095の`constructor(): T`がそのまま出た）。d.tsのaccessorもtsgoどおりtype parameterを持たない
    （transform.go:1009-1034）。
  - instantiation式（`f<T>`）の優先順位はmember（precedence.go:293-295）で、括弧を付けない（6.0は`(f)`）。
  - TypeScript fileのJSDoc型（`foo<string?>`）はTypeScriptの構文で、type引数ごと消す（JSDocTypeBaseの
    SubtreeContainsTypeScript、ast.go:1619-1621）。
  - 名前がidentifierでないか、最も内側のmoduleにbodyの無いmodule宣言は出さない（typeeraser.go:122-128、
    classの中の`global x`）。
  - 文字列literalでないmodule指定子（回復したimport）は`require()`とし、生成名は`module`から作る
    （createRequireCall、generateNameForImportOrExportDeclaration）。
  - 配列の分割代入で代入先になれない要素（回復したprivate名）は子だけを訪れる（visitArrayAssignmentElement、
    classfields.go:3256-3266）。
- unit test（tsgoの出力にpin）：
  - CLI：回復したtree（欠けた式、閉じていないtemplate、`;`区切りのobject literal、欠けた`try`とその
    comment）、constructorとsetterの型、instantiation式とJSDoc型、bodyの無いmodule、`require()`の出力と診断。
  - parser：末尾のcommaの4つの形。
  - emitter：preflightはどの回復も通す（parserの事実は変えない）。
- conformance：
  - 15,228 configuration、lane A 13,467（変化なし）、440 s。
  - errorsはfull 13,324、text 14、category 19、mismatch 88、harness error 22で変化なし（描いたbaselineの
    digestもすべて同じ）。
  - emit full 12,481→12,979（+498）、emit mismatch 956→458。上がった498構成：
    - parse errorのある491構成（500構成のうち）。
    - そのほかの7構成：instantiation式の括弧（`instantiationExpressions`、
      `assignmentToInstantiationExpression`、`instanceofOnInstantiationExpression`、
      `optionalChainWithInstantiationExpression2`（es2019）、`importWithTypeArguments`）、setterの型
      （`gettersAndSettersErrors`）、`declarationEmitUsingTypeAlias2`。
  - 下がった構成は無く、それまでfullだったemitのdigestもすべて同じ。
- ratchet：0 regressions、494行raise（emit none→js。errorsがmismatchの4構成はratchetに行が無い）。
- local：
  - formatとworkspace全体のclippy。syntaxとその逆依存（binder、program、emitter、checker、compiler、harness、
    conformance）のtest（60 targets、3,551 passed）。
  - 2 workerのfull run（440 s）。
  - CLIのtestの入力（3 file）はtsgoと診断・3つの`.js`がbyte単位で同じ。
  - `--checkers 4`の並列対照は実行していない。
- 残り：
  - parse errorのある500構成のうち9構成：
    - `privateNameInTypeQuery`、`declarationEmitPrivateNameInTypeQuery`：tsgoは`typeof this.#a`のprivate名を
      読み（parseTypeQueryのallowPrivateName）、d.tsではTS7080で出力を止める（transform.go:668-672）。
    - `dependentDestructuredVariablesNoCrash3`：tsgoのd.tsは初期値を持つ要素が無ければ分割代入をそのまま
      書く（transform.go:841-857、6.0は常に要素に分けた）。
    - `instantiationExpressionErrors`：tsgoはoptional chainと`??`の一時変数を別々の`var`文にする。
    - `optionalChainWithInstantiationExpression1`（es2019）：括弧が二重になる。
    - `parser509630`、`parserSkippedTokens6`／`7`、`objectTypesWithOptionalProperties2`：回復した位置の
      commentの置き方。
  - parserの回復の記録（`ParseRecovery`とadmissionの判定）は使われなくなった。削除は別のsliceで行う。
  - emitの不一致は残り458構成。
- hosted：PR #644（head `626bb842e`、merge `b562ab28c`）、run 37138499811 — `plan` 32s、`rust` 9m43s、`conformance (TypeScript 7.1)` 16m2s、`gates` 13s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `5397457e3`（P3-5zのcode `0b7e38a26`と同じcodeのrelease build）と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 139→137、zod 513→505、Playwright 369→359、TypeScript `src/compiler` 332→341、Next.js 763→739、Effect 534→489、VS Code 3,443→3,372。tsc-rs÷tsgoは0.58〜0.95、peak memory（MB main→本branch）：317→325、1,295→1,290、806→811、287→288、1,319→1,315、1,036→1,036、5,431→5,440。診断の出力と読み込んだdocument数は7 corpusともmainと同一。TypeScript `src/compiler`は5 roundsのA/B（main／本branch）で322／319 ms、`TSRS_CHECKERS=1`の命令数（5回のmedian）は15.939／15.950 Gでノイズ。emitを含む`tsconfig.bench-full.json`（JS・d.ts・source map、3 rounds）：hono 145→144、zod 609→623、Playwright 484→486、TypeScript `src/compiler` 504→500、Next.js 1,044→1,007、Effect 801→801（tsgo比0.50〜0.77）、zodは5 roundsのA/Bで584／577 msなのでノイズ。出力はhono・zod・TypeScript `src/compiler`・Next.jsでmainと同一。Playwrightの8 fileとEffectの5 file（と各source map）はinstantiation式の括弧が無くなった差で、Effectの5 fileとPlaywrightの4 fileはtsgoの出力と同一（Playwrightの残る4 fileの差はmainにもある`react/jsx-runtime`のimportの並び順で、tsgoは名前順）。退行なし。

## P3-5ab d.tsの分割代入とnamespaceの`var`をtsgoに合わせる（2026-10-04）

P3-5aa後のemitの不一致458構成のうち、TypeScript fileで原因がはっきりした3つのclass：
- d.tsの分割代入：初期値を持つ要素（入れ子を含む）があるときだけ要素ごとの宣言に分け、それ以外は
  binding patternのまま宣言全体の型で書く（transform.go:841-873、`declare var [x]: [number, string];`）。6.0は
  常に分けた。型はsymbolの無い宣言としてgetTypeForVariableLikeDeclarationの型（nodebuilderimpl.go:2263-2272）で、
  tsc-rsのgetSymbolOfDeclarationが返すunknown symbolをtsgoのnilとして扱う。空のpattern（`var [] = …`）は
  従来どおり出さない。
- namespaceとenumの`var`：同じscopeで先に宣言された変数（分割代入の名前を含む）・関数・classがあれば
  `var x;`を書かない（tsgo RuntimeSyntaxTransformerのpushScope／recordDeclarationInScope、runtimesyntax.go:52-67、
  140-171）。6.0は関数とclassだけを数えた。ambientな文は、tsgoではtype eraserが先に消すので数えない。
- `declare`の付いたimport-equals宣言（`declare import a = b;`、TS1079）は出さない（typeeraser.go:48-50：`declare`の
  付いた文はすべて消す）。6.0は`var a = b;`を書いた。
- unit test（CLI、tsgoの出力にpin）：分割代入のd.ts（初期値の有無、入れ子、rest、型注釈）、namespaceとenumの
  merge（変数・関数・分割代入・`declare var`・後に来る変数・`let`とenum）、`declare import`、それぞれの診断。
- conformance：
  - 15,228 configuration、lane A 13,467（変化なし）、443 s。
  - errors：full 13,324→13,325（+1）、mismatch 88→87。`isolatedDeclarationErrorsExpressions`は、分割代入を要素に
    分けなくなってtsgoに無いTS9019が消えた。
  - emit full 12,979→13,003（+24）、emit mismatch 458→434。上がった24構成：
    - 分割代入のd.ts 19構成（`declarationEmitDestructuringArrayPattern1`〜`5`、
      `declarationEmitDestructuringObjectLiteralPattern`／`1`／`2`、`declarationEmitDestructuringPrivacyError`、
      `declarationEmitOptionalMappedTypePropertyNoStrictNullChecks1`〜`3`、`declarationEmitNonExportedBindingPattern`、
      `declarationEmitExpressionInExtends6`、`declarationEmitSubpathImportsReexport`、`subpathImportDeclarationEmit`、
      `dependentDestructuredVariables`、`dependentDestructuredVariablesNoCrash3`、`stringLiteralTypesAndTuples01`）。
    - namespaceの`var` 3構成（`augmentedTypesVar`、`module_augmentExistingVariable`、`nameCollisions`）。
    - `declare import` 2構成（`declareModifierOnImport1`、`importDeclWithDeclareModifier`）。
  - 下がった構成は無い。描いたbaselineのdigestが変わったのは`isolatedDeclarationErrorsExpressions`だけで、それまで
    fullだったemitのdigestはすべて同じ。
- ratchet：0 regressions、24行raise・1行追加。
- local：
  - formatとworkspace全体のclippy。emitter・checker・compiler・conformanceのtest（35 targets、2,622 passed）。
  - 2 workerのfull run（443 s）。
  - CLIのtestの入力（3 file）はtsgoと診断・d.ts・`.js`がbyte単位で同じ。
  - `--checkers 4`の並列対照は実行していない。
- 残り（emitの不一致434構成）：
  - JavaScriptからのd.ts（約270構成）：tsgoは`declare`を付けたclass、CommonJSの`export =`と`_exports`、暗黙の
    constructor、export宣言の位置などが6.0と違う。tsgoのd.tsは再解析したJavaScriptのtreeから作り、tsc-rsは6.0の
    symbolからの直列化なので、別の設計で進める。
  - CommonJSのexportへの分割代入（7構成）：tsgoは可能なら分割代入のまま書く（visitDestructuringAssignment、
    commonjsmodule.go:1110-1135、1415-1480）。
  - TypeScriptのd.tsの単発の差：再利用した文字列literal型の引用符、unionの並び、expandoのnamespace、
    `declare const _default = 0;`、引数の無いsetterの`value: any`など。
- hosted：PR #645（head `94e330b79`、merge `601cbd50e`）、run 37141407870 — `plan` 37s、`rust` 7m42s、`conformance (TypeScript 7.1)` 12m31s、`gates` 14s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main `eec61370e`（P3-5aaのcode `979e0e741`と同じcodeのrelease build）と本branchのrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 136→128、zod 523→508、Playwright 376→358、TypeScript `src/compiler` 343→343、Next.js 758→739、Effect 524→515、VS Code 3,423→3,391。tsc-rs÷tsgoは0.58〜0.98、peak memory（MB main→本branch）：317→320、1,289→1,289、811→810、289→290、1,314→1,320、1,035→1,034、5,447→5,444。診断の出力と読み込んだdocument数は7 corpusともmainと同一。`tsconfig.bench-full.json`（JS・d.ts・source map、3 rounds）：hono 143→148、zod 610→631、Playwright 484→479、TypeScript `src/compiler` 504→500、Next.js 987→1,001、Effect 811→805（tsgo比0.53〜0.77）、5 roundsのA/B（main／本branch）はzod 595／590 ms、Next.js 999／973 ms、hono 147／143 msでノイズ。出力はhonoとTypeScript `src/compiler`でmainと同一。zod・Playwright・Next.jsの変わったd.ts（各1〜2 file）は分割代入をそのまま書く形でtsgoの出力と同一、Effectの2 fileは同じ形になり、残る差はmainにもある型の表示（tsgoは`ReadonlyArray<…>`やmapped typeを書いたまま残す）。退行なし。

## P3-5ac CommonJSのexportした分割代入と、エラー回復した構文の消去をtsgoに合わせる（2026-10-04）

[JavaScriptのd.ts](../ts71-js-declarations/README.md)の完了後、emitの不一致144構成のうち、TypeScript fileのJavaScript出力で
原因がはっきりした2つのclass：
- **CommonJSのexportした分割代入**：exportした変数のbinding patternは、葉を`exports.name`にした分割代入として書き、
  配列patternのiteratorの意味を保つ（transformInitializedVariable、commonjsmodule.go:1110-1128）。`export { … }`が葉を
  別の名前でも、複数の名前でもexportするときだけ平らにする（destructuringNeedsFlattening、commonjsmodule.go:
  1428-1480）。6.0は常に平らにした。変換したpatternのnodeは、tsgoの`ConvertVariableDeclarationToAssignmentExpression`と
  同じく元のcommentとsource mapの範囲だけを持ち、textの範囲を持たない（printerは合成したnodeとして並べる）。
- **エラー回復した構文の消去**（tsgoのtype eraser、typeeraser.go）：
  - accessorは型引数と戻り値の型を、constructorは加えて修飾子をすべて落とす（`get foo<T>()`、`set foo(v): number`、
    `constructor<T>(): number`、`export constructor()`）。
  - memberの型引数と型、`export`のkeywordは、transform flagsでTypeScriptとして数える（ast.goのsubtree facts）。
  - `in`／`out`は、`in`演算子以外の位置ではすべて消す（`in x = 1;`）。
  - constructorはTypeScriptの通常の関門を通して訪れる（6.0のclass要素の訪問はconstructorを常に訪れた）。
    TypeScriptを含まないconstructor（`accessor constructor() {}`、`static constructor() {}`）は書いたまま残る。
  - `export`がTypeScriptとして数えられると、namespaceの本体のblockの中にあるexport宣言もTypeScriptの訪問に入る。
    namespaceが消すのはmemberの`export`と`default`だけで、入れ子の宣言は残す（`innerModExport1`／`2`、
    `moduleElementsInWrongContext3`。1回目のfull runで見つけた）。
- unit test（CLI、tsgoの出力にpin）：exportした分割代入（配列、object、rest、default、穴、入れ子、別名でもexportする
  ので平らにする葉）、エラー回復した構文の消去（5 file、診断も）。
- conformance：
  - 15,228構成、lane A 13,467（変化なし）、455 s。
  - errorsは変化なし（描いたbaselineのdigestもすべて同じ）。
  - emit full 13,293→13,311（+18）、emit mismatch 144→126。上がった18構成：
    - exportした分割代入 10構成（`exportDestructuring`、`exportDestructuringIterator`、`exportEmptyArrayBindingPattern`、
      `exportEmptyObjectBindingPattern`、`exportObjectRest`、`downlevelLetConst13`、`destructuringInVariableDeclarations1`、
      `commonjsExportDestructuringImportedValue`、`bindingPatternOmittedExpressionNesting`、
      `declarationEmitRetainsJsdocyComments`）。
    - エラー回復した構文 8構成（`parserGetAccessorWithTypeParameters1`、`parserSetAccessorWithTypeAnnotation1`、
      `parserSetAccessorWithTypeParameters1`、`parserConstructorDeclaration3`／`9`／`10`、`varianceModifiersOnClassMembers`、
      `autoAccessorDisallowedModifiers`）。
  - 下がった構成は無い。不一致のまま出力が変わったのは`plainJSGrammarErrors`で、`static constructor()`と
    `async constructor()`がtsgoと同じく残り、最初の差は233行目から278行目に移った（下の「残り」）。
- ratchet：0 regressions、18行raise（emit none→js）。`intersectionConstructorReductionCrash`は今回もharness errorで、ratchetに入れない。
- local：
  - formatとworkspace全体のclippy。binder・emitter・checker・compiler・conformanceのtest（38 targets、2,706 passed、`91fea3f56`）。emitterのunit testは、exportしたobjectのbinding patternをtsgoの`({ toString: exports.toString } = 1);`に再pinした。
  - 2 workerのfull run（455 s、`91fea3f56`のrelease build）。1回目（`6dc951eb8`、444 s）はemit full 13,308で、
    namespaceの入れ子のexport宣言の3構成が下がっていた。これを直した。
  - CLIのtestの入力はtsgoと診断・`.js`がbyte単位で同じ。
- 残り：
  - JavaScriptのfileのimport elision：tsgoはJavaScriptのfileでimport elisionを行わない（emitter.go:118）ので、
    `async export { C }`の`async`が残る。tsc-rsは名前付きのexport宣言の修飾子をいつも落とす（`plainJSGrammarErrors`。
    この経路は本sliceで変えていない）。
  - TypeScriptのJavaScript出力の残り：static memberのarrow関数の`this`（`asyncArrowStaticFieldThis`など）、
    `react-jsx`のcommonjsでの`_jsx`の呼び方、commentと改行の配置、decorator metadataの`BigInt`、`super(...arguments)`、
    namespaceのimport alias（`moduleElementsInWrongContext`）など。
- hosted：PR #652（head `f70f1f07f`、merge `5de8ec7f5`）、run 37183720437 — `plan` 27s、`rust` 7m19s、`conformance (TypeScript 7.1)` 19m36s、`gates` 16s。
- perf（README corpora、`--noEmit`、3 rounds、nice 20、main（J4のcode `de729e110`）と本branch（`91fea3f56`）のrelease build対tsgo 7.1.0-dev、median wall ms main→本branch）：hono 136→135、zod 549→516、Playwright 364→377、TypeScript `src/compiler` 345→337、Next.js 779→753、Effect 541→520、VS Code 3,494→3,483。tsc-rs÷tsgoは0.57〜0.95、診断の出力と読み込んだdocument数は7 corpusともmainと同一。`tsconfig.bench-full.json`（3 rounds）：hono 141→144、zod 626→623、Playwright 484→499、TypeScript `src/compiler` 536→506、Next.js 1,067→1,039、Effect 823→810（tsgo比0.60〜0.79）、出力と診断は6 corpusともmainと同一。Playwrightの差をA/Bで確かめた：5 roundsは`--noEmit` 357／355 ms、full 471／472 ms、honoのfull 140／140 ms。単一checker（`TSRS_CHECKERS=1`、5回のmedian）の命令数はPlaywrightの`--noEmit` 22.313／22.318 G（peak memory footprint 517.4／517.9 MB）、full 33.970／33.980 G（556.3／555.9 MB）。退行なし。

## P3-5ad runnerのemit baselineをtsgoのrunnerと同じく描く（2026-10-04）

[pseudochecker](../ts71-pseudochecker/README.md)のPC4の後、emitの不一致111構成のうち、runnerが描かない部分と
optionの扱いが原因の3 class：
- **`suppressOutputPathCheck`**：tsgoではcompiler option（core/compileroptions.go:114）。harnessはdirectiveをoptionとして
  読み（harnessutil.go:317-333）、Programは出力pathを検査しない（compiler/program.go:1340）。tsc-rsではtranspileの経路
  だけが検査を止めていたので、出力pathが入力と同じJavaScript fileがemitされず、`output missing`になっていた。
  内部用のtyped option（tsconfigには無い）にし、emitの計画が検査を止める。runnerの、出力pathの診断を集めるか
  どうかの専用のflagは消した。
- **`[DtsFileErrors]`**（`DoJSEmitBaseline`、js_emit_baseline.go:76-97）：宣言を求め、診断が無く、d.tsを出した構成では、
  fixtureの各sourceのd.ts（outDirとcommon source directoryから探す。tsgoと同じくdeclarationDirは見ない）と、d.tsと
  JSONのfixtureを、同じoptionでもう一度compileし、その診断を書く。harnessに`load_native_declaration_program`を加え、
  file systemとProgramの読み込みを`load_native_compiler_program`と共有した。outDirが相対のとき、emitterが書くpathも
  相対（`./out/a.d.ts`）なので、current directoryで解決してから探す。
- **noCheckの比較**（js_emit_baseline.go:99-131）：tsgoはnoCheckでもう一度emitし、出力の違いを書く。referenceに差が
  出るのは、`noEmitOnError`が診断でemitを止めた構成だけ（2 baseline）なので、その構成だけemitし直す。両方が書いた
  fileの内容の違いは、tsgoの見出しだけを書く（行のdiffを書くreferenceは無い）。
- d.tsの再compileで見つかったcheckerの差：tsgoはTS18057（es2015／es2020での文字列のexport名）をd.tsでは報告しない
  （checker.go:5523）。tsc-rsは報告し、`moduleExportAliasElementAccessExpression`にtsgoに無い`[DtsFileErrors]`が出た。
- supervisor（`scripts/conformance_ts71.py`）：workerのresident sizeはそれまでのcaseの分も含むので、`--max-rss-mib`を
  超えた構成は、まず新しいworkerでもう一度実行し、そこでも超えたときだけharness errorにする。1回目のfull run
  （`282caf330`、480 s）では、宣言の再compileの分だけworkerが早く上限に達し、単独では通る
  `nestedSpreadsAndWidening`（1.1 GB、3.5 s）がharness errorになった。
- unit test：
  - runner：`[DtsFileErrors]`の見出し、noCheckの比較（tsgoの`noEmitOnError.js`と同じ形）、d.tsの名前
    （`ChangeToDeclarationExtension`、`IsDeclarationFileName`）。
  - harness：`suppressOutputPathCheck`はcompiler option。
  - CLI（tsgoの出力にpin）：d.tsの文字列のexport名にTS18057を出さない。
- conformance（release build、`d5ed710d5`、`--workers 2`、523 s）：
  - 15,228構成、lane A 13,467（変化なし）。errorsは変化なし（描いたbaselineのdigestもすべて同じ）。
  - emit full 13,326→13,363（+37）、emit mismatch 111→74：
    - `suppressOutputPathCheck` 12構成（`checkJsdocOptionalParamOrder`、`checkJsdocParamOnVariableDeclaredFunctionExpression`、
      `checkJsdocParamTag1`、`checkJsdocTypeTag1`／`2`、`checkJsdocTypeTagOnObjectProperty1`／`2`、
      `checkJsdocTypedefInParamTag1`、`checkJsdocTypedefOnlySourceFile`、`malformedTags`、`jsdocTypeTag`、`jsdocTypeTagCast`）。
    - `[DtsFileErrors]` 23構成（`fakeInfinity2`／`3`、`typeReferenceDirectives3`／`4`、`moduleAugmentationInAmbientModule5`、
      `privacyCannotName*`の4つ、`commonSourceDirectory`、`declarationEmitToDeclarationDirWithDeclarationOption`、
      `jsDeclarationsExpandoInternal`、`jsDeclarationsJson`、`jsDeclarationsNonIdentifierInferredNames`、
      `jsDeclarationsPackageJson`、`importTag16`、`tsNoCheckForTypescript`・`…Comments1`／`2`、
      `nodeModulesTripleSlashReferenceModeDeclarationEmit7`の4構成）。
    - noCheckの比較 2構成（`noEmitOnError`、`isolatedModulesNoEmitOnError`）。
  - 下がった構成は無い。不一致のまま出力が変わったのは2構成で、どちらも既存のd.tsの差が再compileで診断になった：
    `enumComputedPropertyDeclarationEmit`（enumのcomputed key、TS2344）、`typeTagOnFunctionReferencesGeneric`（JSDocの
    `@type`の関数型の型parameter、TS2304）。
  - 実行時間は442 sから523 sになった。宣言の再compileと、memory上限で止まる重い2構成
    （`excessivelyDeepConditionalTypes`、`intersectionConstructorReductionCrash`）を新しいworkerでもう一度実行する分。
    この2構成は新しいworkerでも上限を超え、今までどおりharness errorで、ratchetに入れない。
- ratchet：0 regressions、37行raise（emit none→js）。
- local：formatとworkspace全体のclippy。harness・conformance・compiler・emitter・programのtest（43 targets、1,447件、
  `282caf330`）とCLIのtest。
- hosted：PR #655（head `ff6b3154a`、merge `c6b070f84`）、run 37195721743 — `plan` 27s、`rust` 10m10s、
  `conformance (TypeScript 7.1)` 20m36s、`gates` 12s。
- perf（README corpora、nice 20、main（PC4のcode `31b313976`）と本branch（`d5ed710d5`）のrelease build対tsgo 7.1.0-dev、
  median wall ms main→本branch。compilerの変更はemitの計画とcheckerの条件が1つずつ）：`--noEmit`（3 rounds）hono
  129→136、zod 526→523、Playwright 364→358、TypeScript `src/compiler` 339→330、Next.js 797→752、Effect 572→498、
  VS Code 3,448→3,524（min 3,410→3,388）。tsc-rs÷tsgoは0.60〜0.96、診断の出力と読み込んだdocument数は7 corpusとも
  mainと同一。`tsconfig.bench-full.json`（3 rounds）：hono 141→149、zod 614→641、Playwright 491→484、TypeScript
  `src/compiler` 536→516、Next.js 1,027→1,023、Effect 761→768（tsgo比0.59〜0.81）、出力と診断は6 corpusともmainと
  同一。A/B（5 rounds）：zod full 597／596、hono full 139／142、honoの`--noEmit` 126／120、Effect full 739／746 ms。
  単一checker（`TSRS_CHECKERS=1`、5回のmedian）の命令数：hono full 6.319／6.315 G（peak memory footprint
  152.3／151.8 MB）、zod full 39.451／39.431 G（895.4／893.8 MB）。退行なし。
- 残り：emitの不一致74構成。`composite`と`incremental`（buildinfo、8構成）、TypeScriptのd.tsの差（computed key、
  型の括弧、型parameterの名前の付け直し、unionの順）、JavaScriptの出力の差など。

## P3-5ae 型の括弧をtsgoのprinterと同じく付ける（2026-10-04）

P3-5adの後、TypeScriptのd.tsの不一致のうち、型の括弧が原因の7構成。tsgoのfactoryは型を括弧で包まず、printerが
位置ごとのprecedenceより低い型を括弧で包む（`emitTypeNode`、printer.go:2271-2302、`ast.GetTypeNodePrecedence`）。
tsc-rsは6.0どおりfactoryが型を作るたびに括弧を付けていた。
- **factory**：6.0の型のparenthesizer（`createParenthesizerRules`の型の半分）を、型の作成と更新のすべてから外した。
- **printer**：tsgoの位置ごとのprecedenceで括弧を付ける。
  - unionとintersectionの構成要素はTypeOperator：parseした`A & B | C`も`(A & B) | C`になる。
  - type operatorの被演算子はTypeOperator（`readonly`はPostfix）。
  - 配列・indexed access・optionalの被演算子はPostfix。ただしparseした後置型の`typeof`は書いたまま（`typeof C[K]`、
    printer.go:1999-2012）。tsgoの更新・複製したnodeは元のflagを引き継ぐ（`updateNode`、ast.go:103-112）ので、
    parse treeに元を持つnodeをparseしたものとして扱う。
  - conditional型のcheckはUnion、`extends`節はtsgoの`inExtends`の状態で書く：そこではconditional型を括弧で包み、
    関数型が返す制約付きの`infer`も包む。`infer`の制約も`extends`節として書く。
  - 型引数は最低のprecedence：generic関数型の型引数は括弧なし（`X<<T>() => T>`。6.0は`X<(<T>() => T)>`）。
- **nodeの再利用**：書いた括弧付きの型を残す。tsgoの`tryVisitSimpleTypeNode`は式の括弧だけを飛ばす
  （`ast.SkipParentheses`、nodecopy.go:452-466）。6.0は型の括弧も飛ばした（`keyof (A['a'])`→`keyof A['a']`）。
- unit test：CLI（tsgoの出力にpin）で上の各形。factoryのtestは、構成要素を括弧なしのまま保つことに書き直した。
- conformance（release build、`f1ecdeba1`、`--workers 2`、523 s）：
  - 15,228構成、lane A 13,467（変化なし）。errorsは変化なし（`intersectionConstructorReductionCrash`は今回は
    memory上限に達せずfullだったが、負荷に依るので今までどおりratchetに入れない）。
  - emit full 13,363→13,371、emit mismatch 74→67。上がった7構成：`declarationEmitFirstTypeArgumentGenericFunctionType`、
    `declarationEmitPromise`、`importTypeGenericArrowTypeParenthesized`、`declarationEmitResolveTypesIfNotReusable`、
    `inferTypesWithExtends1`、`spreadObjectOrFalsy`、`readonlyArraysAndTuples`。
  - 下がった構成も、tierが同じまま出力が変わった構成も無い。
- ratchet：0 regressions、7行raise（emit none→js）。
- local：formatとworkspace全体のclippy。binder・emitter・checker・compiler・conformanceのtest（38 targets、2,716件）。
  試行（filter `declarationEmit`・`onditional`・`types/`・`uple`）で下がった構成は無かった。
- hosted：PR #656（head `e5ab166ea`、merge `bafc31db5`）、run 37197710925 — `plan` 23s、`rust` 7m57s、
  `conformance (TypeScript 7.1)` 13m52s、`gates` 12s。
- perf（README corpora、nice 20、main（P3-5adのcode `d5ed710d5`）と本branch（`f1ecdeba1`）のrelease build対tsgo 7.1.0-dev、
  median wall ms main→本branch）：`--noEmit`（3 rounds。tsgo自身も前回より5〜15 %遅い騒がしい回）hono 147→152、
  zod 562→605（min 557→527）、Playwright 428→400、TypeScript `src/compiler` 383→350、Next.js 859→826、Effect 541→537、
  VS Code 4,077→3,794。診断の出力と読み込んだdocument数は7 corpusともmainと同一。`tsconfig.bench-full.json`
  （3 rounds）：hono 152→150、zod 661→655、Playwright 531→529、TypeScript `src/compiler` 521→510、Next.js
  1,073→1,063、Effect 812→807（tsgo比0.61〜0.77）、診断はmainと同一。A/B（5 rounds）：zodの`--noEmit` 515／525
  （min 514／517）、honoの`--noEmit` 126／133（min 125／122）、Effect full 757／758 ms。単一checker
  （`TSRS_CHECKERS=1`、5回のmedian）の命令数：zodの`--noEmit` 31.851／31.841 G（peak memory footprint
  866.9／865.3 MB）、Effect full 51.713／51.707 G（808.0／808.3 MB）。退行なし。fullの出力をtsgoと比べた
  （6構成）：mainから変わった40 fileのうち34（d.tsとdeclaration map）がtsgoとbyte単位で同じになり、tsgoと
  同じだったfileで違うようになったものは無い。変わったmapのmapping segmentは1,262がtsgoに近づき、36が離れた
  （Effectの1つのmapで行がずれ、同じ数だけ得て失った）。

## P3-5af d.tsの細部：enumのkey、setterの値、mapped typeの型、型のqueryのprivate名（2026-10-04）

P3-5aeの後、TypeScriptのd.tsの不一致から、原因が独立した4つ：
- **enum memberの名前のproperty**：node builderは、enumが要素一覧のenclosing declaration（無ければenclosing file）から
  値として参照できるとき、memberへのcomputed参照（`[S.A]`）を書く（nodebuilderimpl.go:2535-2552）。6.0には無い腕で、
  tsc-rsは値（`a`、`"not-an-identifier"`）を書いていた。参照できないenum（関数の中の`enum`）は値のまま。
- **setterの値parameter**：declaration transformは位置で選び（`this`の次、無ければ先頭）、無いときは`value: any`を
  作る（privateでは型なし、transform.go:1037-1071）。6.0は型の無い`value`を作った。`this` parameterは先頭のものだけ
  （`ast.GetThisParameter`）。
- **型の無いmapped type**：declaration transformはtemplateの型を`any`で補う（transform.go:723-740）。mapped typeを
  subtreeの変換の対象に加えた。
- **型のqueryのprivate名**：tsgoのparserは`typeof this.#a`のprivate名を受け付ける（parser.go:3164-3174）。tsc-rsは
  TS1003にしていた。declaration transformは、qualified nameの右がprivate名ならTS7080を報告する
  （transform.go:668-672）。この診断でそのfileのd.tsは出ない。
- unit test（CLI、tsgoの出力にpin）：enum memberの名前（参照できるenumとできないenum）、setter・mapped type・
  型のqueryのprivate名（診断とd.ts）。
- conformance（release build、`7ee088e7f`、`--workers 2`、550 s）：
  - 15,228構成、lane A 13,467（変化なし）。
  - errors full 13,338→13,340（`privateNameInTypeQuery`、`declarationEmitPrivateNameInTypeQuery`）、mismatch 80→78。
  - emit full 13,371→13,377、emit mismatch 67→61：`enumComputedPropertyDeclarationEmit`、
    `declarationEmitComputedNameConstEnumAlias`、`declarationEmitSetAccessorNoParameter`、`mappedTypeNoTypeNoCrash`、
    上の2構成。
  - 下がった構成も、tierが同じまま出力が変わった構成も無い。
- ratchet：0 regressions、6行raise。`intersectionConstructorReductionCrash`は今回も通ったが、負荷に依るので
  ratchetに入れない。
- local：formatとworkspace全体のclippy。syntax・binder・emitter・checker・compiler・conformanceのtest（49 targets、
  2,964件）。試行（filter `num`・`declarationEmit`・`omputed`・`ccessor`・`apped`・`rivate`・`ypeQuery`・`ypeof`）で
  下がった構成は無かった。
- hosted：PR #657（head `1a6704876`、merge `6dd401ccf`）、run 37199882545 — `plan` 29s、`rust` 8m12s、
  `conformance (TypeScript 7.1)` 14m12s、`gates` 11s。
- perf（README corpora、nice 20、main（P3-5aeのcode `f1ecdeba1`）と本branch（`7ee088e7f`）のrelease build対tsgo 7.1.0-dev、
  median wall ms main→本branch）：`--noEmit`（3 rounds）hono 142→143、zod 550→552、Playwright 396→382、TypeScript
  `src/compiler` 357→351、Next.js 818→810、Effect 555→519、VS Code 3,813→3,768（tsgo比0.63〜0.94）。診断の出力と
  読み込んだdocument数は7 corpusともmainと同一。`tsconfig.bench-full.json`（3 rounds）：hono 158→153、zod 670→665、
  Playwright 527→531、TypeScript `src/compiler` 572→528、Next.js 1,148→1,100、Effect 837→834（tsgo比0.61〜0.78）、
  出力と診断は6 corpusともmainと同一。退行なし。

## P3-5ag 診断の型表示：外側の型parameterの組、対になっていないsurrogate、再利用したliteralの引用符（2026-10-04）

P3-5afの後、errorsのCategory／Textの不一致のうち、診断文の型表示（`check.rs`の文字列の表示）が原因の3つ：
- **外側の型parameterの組**：generic関数の中のclass式の型は、外側の型parameterの組ごとに親の名前で修飾される。
  tsgoの`appendReferenceToType`は組の型引数を落とし、修飾子と最後の参照の型引数だけを残す
  （nodebuilderimpl.go:272-323、「nested type args are silently elided」）。tsc-rsは6.0どおり
  `mixin<typeof BaseClass>.(Anonymous class)`と書いていた。tsgoと同じく`mixin.(Anonymous class)`にした。組の型引数は
  今までどおり一度描く（tsgoの`mapToTypeNodes`の長さの見積もりと切り詰めのため）。
- **対になっていないsurrogate**：文字列literal型の表示で、tsgoの`escapeStringWorker`と同じく`\uDC00`にする
  （printer/utilities.go:84-86）。tsc-rsは単位をそのまま持ち、出力で`�`になっていた。
- **再利用したliteralの引用符**：enclosingがあるときに再利用した型の文字列literalは、書いた引用符のまま、ASCIIの
  escape無しで書く（tsgoの再利用は引用符のflagごと複製する、nodecopy.go:810-821）。6.0の複製は常に二重引用符だった。
- unit test：CLI（tsgoの出力にpin）で3つ。6.0の表示に固定していたunit testを書き直した：UTF-16のliteral表示の
  fixture（`tsgo_overrides`の3 caseの`display`と診断文）、mixinの静的側、外側の型引数、JavaScriptの入れ子のclass。
- conformance（release build、`965d874de`、`--workers 2`、542 s）：
  - 15,228構成、lane A 13,467（変化なし）。
  - errors full 13,340→13,346（+6）：`mixinAccessors3`、`mixinPrivateAndProtected`、
    `typeArgumentInferenceWithClassExpression2`、`loneSurrogateStringLiteralTypes`、
    `overloadOnConstNoAnyImplementation2`／`NoStringImplementation2`。text 9→7、category 19→15。
  - emitは変化なし。下がった構成は無い。不一致のまま描いた文が変わったのは
    `templateLiteralInferenceSupplementarySplit`で、tsgoに無い余分な診断の型が`"\uD83D"`と書かれるようになった
    （余分な診断は、tsgoが型literalの推論をcode point単位で行うのにtsc-rsがcode unit単位で行う差。残り）。
- ratchet：0 regressions、6行raise。`intersectionConstructorReductionCrash`は今回も通ったがratchetに入れない。
- local：formatとworkspace全体のclippy。syntax・binder・emitter・checker・compiler・conformanceのtest（49 targets、
  2,965件）。試行（filter `verload`・`ixin`・`iteral`・`urrogate`・`lassExpression`）で下がった構成は無かった。
- hosted：PR #658（head `fd5924fc6`、merge `371167833`）、run 37201677255 — `plan` 24s、`rust` 7m25s、
  `conformance (TypeScript 7.1)` 20m19s、`gates` 47s。
- perf（README corpora、nice 20、main（P3-5afのcode `7ee088e7f`）と本branch（`965d874de`）のrelease build対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 143→130、zod 538→554、Playwright 384→353、
    TypeScript `src/compiler` 363→339、Next.js 807→798、Effect 524→587、VS Code 3,702→3,528。tsc-rs÷tsgo 0.60–0.91。
    7 corporaとも診断と読み込んだ文書数はmainと同一。
  - zodとEffectは10回のA/B：zod 512→511、Effect 521→516。1 checkerの命令数（5回のmedian）branch÷main
    0.99986（zod）、0.99976（Effect）。3回のmedianの差はnoise。
  - `tsconfig.bench-full.json` 3回：hono 149→148、zod 669→649、Playwright 504→506、TypeScript `src/compiler`
    567→534、Next.js 1,083→1,111、Effect 812→807。tsc-rs÷tsgo 0.63–0.81。6 corporaとも出力fileと診断はmainと同一。
    Next.jsは6回のA/Bで997→984、命令数branch÷main 1.00016。劣化無し。
