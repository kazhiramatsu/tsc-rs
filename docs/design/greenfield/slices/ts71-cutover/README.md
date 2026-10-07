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

## P3-5ah 診断のファイル名：並び順、tsconfigの名前、ドライブ付きの名前、referenceの書かれた名前（2026-10-04）

P3-5agの後、errorsの不一致のうち、診断のファイル名の扱いが原因のもの：
- **並び順**：tsgoの`CompareDiagnostics`は`File().FileName()`で比べる（ast/diagnostic.go:390-395、482-520）。tsc 6.0は
  `SourceFile.path`で比べ、大文字小文字を区別しないfile systemでは小文字に畳み、parseしたconfigでは空だった。
  tsc-rsは6.0どおり`Diagnostic.file_path`を持っていた。これを削除し、ファイル名で比べる。`file_path`のためだけの
  コード（configの`diagnostic_file_path`、`with_resolved_config_source_path`、compilerのpathの付け直し、
  `EmitOutcome::diagnostics_mut`）も削除した。CLIでは`B.ts`が`a.ts`より先になる（tsgoと同じ。macOSの6.0は逆）。
- **tsconfigの名前**：tsgoのtest case parserはconfigを正規化した絶対path（`/.src/tsconfig.json`）で名付ける
  （testrunner/test_case_parser.go:84）。harnessは書かれた名前（`tsconfig.json`）を渡していたので、runnerが位置を
  引けず（`(1,1)`）、並びも先頭になっていた。
- **ドライブ付きの名前**：runnerはunitの名前に`/.src/`を常に付けていた（`/.src/c:/app/main.ts`）。harnessの
  rooted pathの正規化（`GetNormalizedAbsolutePath`）を使う。
- **referenceの名前**：path reference（TS6053、TS6504、TS6054、TS6231）は書かれたreferenceをslashだけ正規化して
  名付ける（tsgoの`diagnosticFileName`、compiler/fileloader.go:697）。6.0は解決したpathだった。checkerにも6.0由来の
  TS6053の経路があり、同じ文にした（違う文だと2行になる）。
- unit test：CLI（tsgoの出力にpin）でファイル名の順とreferenceの名前の2つ、runnerでrooted pathの正規化。
  6.0の挙動に固定していた4つを書き直した：diagnosticsの並び（configが先頭→名前順）、checkerの
  `'../typescript.ts'`、program sessionのTS6053、空のreferenceのTS6231（`''`、tsgoで確認）。
- conformance（release build、`d808eafd4`、`--workers 2`、553 s）：
  - 15,228構成、lane A 13,467（変化なし）。
  - errors full 13,346→13,353（+8、うち1つは下記の構成の外れ）：`pathsValidation5`、`tsconfigRootdirInclude`、
    `keepImportsInDts1`、`missingMemberErrorHasShortPath`、`pathMappingBasedModuleResolution1_node`、
    `typingsLookup3`、`selfReferencingFile2`、`parserharness`。text 7→6、category 15→13、mismatch 78→73。
  - emitの不一致は61のまま。下がった構成は無い。不一致のまま描いた文が変わった構成も無い。
  - `intersectionConstructorReductionCrash`はmemory制限（3,072 MiB）に2回かかりharness errorになった
    （emit full 13,377→13,376、harness errors 21→22はこの1構成）。負荷に依存する構成で、単独ではfull（35.7 s、
    最大RSS 3.26 GB）。ratchetには入れない。
- ratchet：0 regressions、8行raise。
- local：formatとworkspace全体のclippy。diagnostics・syntax・binder・program・harness・emitter・checker・compiler・
  conformanceのtest（62 targets、3,624件。空referenceのtestを直した後にprogramの`contracts` 503件を再実行）。試行：config／ドライブ付きのunitを持つ165 caseと、
  `@useCaseSensitiveFileNames: false`／`@currentDirectory`の55 case、path referenceかTS6053等を持つ345 caseを
  1件ずつ実行し、下がった構成は無かった。
- hosted：PR #659（head `729d0e4ae`、merge `03e009c0c`）、run 37204611385 — `plan` 27s、`rust` 9m42s、
  `conformance (TypeScript 7.1)` 18m3s、`gates` 13s。
- perf（README corpora、nice 20、main（P3-5agのbuild `965d874de`）と本branch（`d808eafd4`）のrelease build対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 160→148、zod 551→563、Playwright 407→398、
    TypeScript `src/compiler` 367→346、Next.js 879→900、Effect 579→583、VS Code 3,807→3,683。tsc-rs÷tsgo 0.58–0.92。
    読み込んだ文書数は7 corpora、診断は6 corporaでmainと同一。Effectは同じ診断をtsgoの順（`src/SchemaAST.ts`が
    `src/schema/…`より先。大文字小文字を区別するファイル名の順）で出す。
  - zodとNext.jsは10回のA/B：zod 527→526、Next.js 767→778（min 735→718）、zodの`bench-full` 632→629。
    1 checkerの命令数（5回のmedian）branch÷main 0.99945（zod）、0.99889（Next.js）。3回のmedianの差はnoise。
  - `tsconfig.bench-full.json` 3回：hono 167→152、zod 643→658、Playwright 522→505、TypeScript `src/compiler`
    549→527、Next.js 1,087→1,046、Effect 862→801。tsc-rs÷tsgo 0.55–0.80。出力fileは6 corporaとも同一、診断は5 corporaで
    同一、Effectは上と同じ順の違いだけ。劣化無し。
  - 比較の途中で、Effect（`--noEmit`）でtsgoが出す`src/http/HttpClient.ts(412,5)`のTS2322（`With<E1 | Exclude<E, …>>`が
    `With<E1 | ExcludeTag<E, …>>`に代入できない）をtsc-rsが出していないことに気づいた（mainでも同じ。別のslice）。

## P3-5ai 診断文の細部とimported helper（2026-10-04）

P3-5ahの後、errorsのCategory／Text／不一致のうち、診断文の組み立てとimported helperの検査が6.0どおりだったもの：
- **TS2741の名前**：tsgoは不足したpropertyを素のsymbolToStringで書く（relater.go:4394）。6.0はWriteComputedPropsで
  計算名を書き直していた（`[16]`、`[E.A]`）。tsgoどおり、早期束縛の計算名はsourceのまま（`[0x10]`、`[ 'ab' ]`）、
  enum keyのmapped propertyは値（`0`）。`missing_property_display_name`からWriteComputedPropsを除いた。
- **enum keyの型表示**：文字列の型表示に、tsgoのenum literalの枝（nodebuilderimpl.go:2535-2552）が無かった。
  描画のenclosingからenumが値として見えるとき`{ [E.B]: number; }`と書く（P3-5afでd.ts側に入れたものと同じ規則）。
- **TS18013のclass名**：tsgoはclassのsymbolをsymbolToStringで書く（checker.go:11724）。代入先の名前（`c1`、`k`）や
  `(Anonymous class)`になる。6.0は名前の無いclassを`(anonymous)`と書いていた。
- **TS18042のimport文**：tsgoは最も近いimport／import-equals／変数宣言のspecifierを使い、import specifierのときだけ
  名前を付ける（checker.go:6935-6950）：`import("@truffle/contract")`、`import("./t")`。
- **TS2713**：右側の名前が欠けているとき、tsgoは空のtext（checker.go:16199）、6.0は`(Missing)`。
- **TS18043の注記**：「automatically exported here」はtypedefの名前を指す（tsgoはtagをJSTypeAliasDeclarationに
  reparseし、その範囲は名前）。
- **imported helper**：tsgoは要求済みhelperをsource fileごとに持つ（checker.go:29053）。6.0はhelperのmoduleに持ち、
  2つ目以降のfileが報告されなかった。CommonJSのfileではdefault importに`__importDefault`、namespace importと
  `export * as`に`__importStar`を、esModuleInteropの条件無しで求める（checker.go:5419-5447、5509-5513、5688-5695、
  5734-5737）。`__spreadArray`の引数の数は検査しない（tsgoが検査するのはprivate fieldのhelperだけ）。
- **nodeの無いcheckerの診断**：tsgoの`NewDiagnosticForNode(nil, …)`は範囲(0, 0)、compilerの診断は(-1, -1)なので、
  ファイルの無い診断はcompilerのものが先に並ぶ（TS5053がTS2318より先）。tsc-rsはどちらも位置無しで、codeの順だった。
  checkerの位置無し診断に0を持たせた。
- unit test：CLI（tsgoの出力にpin）で2つ（TS2741・型表示・TS18013、helperのfileごとの報告とdefault import）。
  6.0に固定していた4つを書き直した：TS18013の名前、早期束縛の計算名（`[ 'ab' ]`、tsgoで確認）、`__spreadArray`の
  引数の数、TS18044の位置。
- conformance（release build、`b6fde98f7`、`--workers 2`、508 s）：
  - 15,228構成、lane A 13,467（変化なし）。
  - errors full 13,353→13,365（+12）：`assignmentCompatWithEnumIndexer`、`privateNameMethodClassExpression`、
    `elidedJSImport1`、`requireTypesOnly`、`errorForUsingPropertyOfTypeAsType01`、`importingExportingTypes`、
    `tslibMissingHelper`、`tslibMultipleMissingHelper`、`tslibImportDefaultHelperCommonJS`、
    `esModuleInteropTslibHelpers`、`arrayIterationLibES5TargetDifferent`（2構成）。text 6→3、category 13→8、
    mismatch 73→69。
  - emitは変化なし。下がった構成も、不一致のまま描いた文が変わった構成も無い。
  - `intersectionConstructorReductionCrash`は今回もmemory制限でharness error（負荷に依存、ratchetの外）。
- ratchet：0 regressions、12行raise。
- local：formatとworkspace全体のclippy。diagnostics・syntax・binder・program・harness・emitter・checker・compiler・
  conformanceのtest（62 targets、3,625件成功・1件失敗。TS18044の位置のtestを直した後にcheckerのlib 1,796件を再実行）。
  試行（filter `tslib`・`elper`・`sModuleInterop`・`rivateName`・`omputed`・`num`・`equire`・`xport`・`mport`・
  `oLib`・`lobal`・`ib`・`arget`・`ption`・`salsa`・`ypedef`・`jsdoc`）で下がった構成は無かった。
- hosted：PR #660（head `a26714be1`、merge `d61338295`）、run 37206742268 — `plan` 28s、`rust` 8m3s、
  `conformance (TypeScript 7.1)` 16m32s、`gates` 12s。
- perf（README corpora、nice 20、main（P3-5ahのbuild `d808eafd4`）と本branch（`b6fde98f7`）のrelease build対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 136→141、zod 525→532、Playwright 372→376、
    TypeScript `src/compiler` 349→332、Next.js 771→766、Effect 534→512、VS Code 3,547→3,445。tsc-rs÷tsgo 0.60–0.91。
    7 corporaとも診断と読み込んだ文書数はmainと同一。
  - honoとPlaywrightは10回のA/B：honoの`--noEmit` 132→129、honoの`bench-full` 144→144、Playwrightの`bench-full`
    491→485。1 checkerの命令数（5回のmedian）branch÷main 0.99961（hono）、0.99901（Playwright）。3回のmedianの差はnoise。
  - `tsconfig.bench-full.json` 3回：hono 147→152、zod 635→616、Playwright 488→503、TypeScript `src/compiler`
    576→498、Next.js 1,059→1,023、Effect 775→762。tsc-rs÷tsgo 0.53–0.79。6 corporaとも出力fileと診断はmainと同一。
    劣化無し。

## P3-5aj distributed type parameter（2026-10-04）

README corporaの比較で、Effect（`--noEmit`）でtsgoが出す`src/http/HttpClient.ts(412,5)`のTS2322をtsc-rsが出して
いなかった。縮めると`declare function g<K>(k: K): K extends string[] ? K[number] : K;`の結果を、同じ形の別の
conditional typeを返す関数から返すだけでtsgoはTS2719を出す。原因はTS 7の新しい規則（TypeScript issue 63708）：
- **distributed type parameter**：型parameterに分配するconditional typeの中（check typeがその型parameterへの単純な
  参照で、最も近いstatementより内側）にある参照は、その型parameterの「distributed」な形になる。同じsymbolを持ち、
  constraintが元の型parameterである別の型parameter（tsgoのgetDistributedTypeParameter、checker.go:23440-23470）。
  元の型parameterはdistributedな形に代入できない。
- `TypeData::TypeParameter`に`is_distributed`を加え、checkerは型parameterごとにdistributedな形を1つ作る（tsgoでは型の
  fieldなので、speculationで捨てない）。
- mapperは元の型parameterで写す（getMappedType、prepend/appendTypeMapping、mapper.go:40-88）。
  getActualTypeVariableは元に戻し、変わったindexed accessを作り直す（checker.go:32054-32067）。inferenceは元で照合し
  元から推論する（inference.go:554-560、1519-1522）。mapped typeのmodifierは元のconstraintを読む
  （checker.go:28606-28609）。
- relaterは元にだけ代入できるsourceをTS5113で説明する（relater.go:4802-4804）。型の表示は元を書く
  （nodebuilderimpl.go:3313）。
- unit test：CLI（tsgoの出力にpin）で縮めた例と`Show<A, B>`のTS2344。conditional typeの単純化のtestを書き直した
  （tsgoは真の枝のdistributedな`T`をそのまま返す、checker.go:28477-28479）。
- mapperの再帰：tsgoは`getMappedType`の入口で1回だけ元に戻し、merged／composite mapperの内側（`TypeMapper.Map`）は
  渡されたものを写す（mapper.go:264-296）。最初の実装は内側でも戻していたので、tsgoの形にした（`ec3bc970c`）。
  check typeのsymbolは、conditional typeが先に解決したnode linksから読む。
- conformance（release build、`b0f71e345`、`--workers 2`、534 s）：
  - 15,228構成、lane A 13,467（変化なし）。
  - errors full 13,365→13,366：`distributedTypeParameters`。mismatch 69→68。
  - emitは変化なし。下がった構成も、不一致のまま描いた文が変わった構成も無い。
  - 最終bytes（`ec3bc970c`、541 s）で再実行し、1回目と全構成で同じだった。
- ratchet：0 regressions、1行raise。
- Effectの`--noEmit`の出力はtsgoと同一になった（3件、18行）。VS Codeも、mainではtsgoより87行少なかった
  （同じ規則のTS2345／TS2322など）のが一致し、残りは3行（長い型の文字列を320 bytesで切る`...`、次のslice）。
- perf：Next.jsの命令数（1 checker、5回のmedian）がmain比+2.8%、zodが+0.8%。perf-countersでは、増えたのは
  関係判定（relation lookups 1,443,330→1,629,158、+12.9%；sets +9.7%）とintersectionで、instantiationは+0.3%。
  distributedな型parameterそのものが関わる比較は2,519件だけで、増えた分はdistributedな型parameterを含む別の型の
  比較だった。tsgoの規則が求める仕事で、実装の無駄ではない（mapperの正規化を入口1回にし、check typeのsymbolを
  cacheしても命令数は変わらなかった）。
- hosted：PR #661（head `e73127fd3`、merge `9c1186709`）、run 37211026146 — `plan` 29s、`rust` 9m44s、
  `conformance (TypeScript 7.1)` 15m19s、`gates` 12s。
- perf（README corpora、nice 20、main（P3-5aiのbuild `b6fde98f7`）対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch、最初のbuild `b0f71e345`）：hono 134→141、zod 564→599、Playwright 399→400、
    TypeScript `src/compiler` 360→341、Next.js 795→853、Effect 551→552、VS Code 3,760→3,636。tsc-rs÷tsgo 0.66–0.95。
    読み込んだ文書数は7 corporaとも同一、診断は5 corporaで同一、EffectとVS Codeは上記のとおりtsgoの診断が増えた。
  - 最終build（`ec3bc970c`）の10回のA/B：zod `--noEmit` 618→597、Next.js `--noEmit` 874→926、zod `bench-full`
    780→765、Effect `bench-full` 943→926。1 checkerの命令数（5回のmedian）branch÷main 1.02836（Next.js）、
    1.00762（zod）。
  - `tsconfig.bench-full.json` 3回（最初のbuild）：hono 154→152、zod 676→733、Playwright 605→605、TypeScript
    `src/compiler` 583→555、Next.js 1,096→1,101、Effect 827→865。tsc-rs÷tsgo 0.59–0.80。出力fileは6 corporaとも同一、
    診断は5 corporaで同一、Effectは上記。
  - Next.jsの+2.8%は上記の規則のコストとして記録し、mergeした（ユーザーの判断事項として報告する）。
- local：formatとworkspace全体のclippy。types・diagnostics・syntax・binder・program・harness・emitter・checker・
  compiler・conformanceのtest（65 targets、3,669件成功・1件失敗。conditional typeの単純化のtestを直した後にcheckerの
  lib 1,796件を再実行）。試行（filter `onditional`・`nfer`・`apped`・`eneric`）で下がった構成は無かった。

## P3-5ak 長い型の文字列の切り詰めと`@noErrorTruncation`（2026-10-05）

P3-5ajの後、VS Codeの`--noEmit`でtsgoと違う3行は、長い型の文字列の末尾だった：
- **型の文字列の切り詰め**：tsgoの`typeToStringEx`は、truncationの長さ（160、`noErrorTruncation`では1,000,000）の2倍
  以上のbytesになった型の文字列を、それより3 bytes短く切って`...`を付ける（checker/printer.go:103-115）。診断の
  文字列の表示にこれが無かった。切る位置は収まる最後の文字の後で、tsgoのbyteの切り方と違うのは多byte文字の途中に
  なる場合だけ。
- **`@noErrorTruncation`**：tsgoのharnessは`noErrorTruncation`をconfigの値の上に、testの設定より前に入れる
  （harnessutil.go:104-108）。tsc-rsのharnessは設定の後に入れていたので、`@noErrorTruncation: false`が効かず、
  `unionElementErrorTruncation`と`largeStringLiteralUnionSuggestion`で切り詰めない型を書いていた。
- unit test：CLI（tsgoの出力にpin）で320 bytesを越える型の文字列。
- conformance（release build、`57a56b914`、`--workers 2`、533 s）：
  - 15,228構成、lane A 13,467（変化なし）。
  - errors full 13,366→13,368：`unionElementErrorTruncation`、`largeStringLiteralUnionSuggestion`。category 8→6。
  - emitは変化なし。下がった構成も、不一致のまま描いた文が変わった構成も無い。
  - `excessivelyDeepConditionalTypes`（`@noErrorTruncation: false`）は今回もmemory制限でharness error（ratchetの外）。
- ratchet：0 regressions、2行raise。
- VS Codeの`--noEmit`の出力はtsgoと同一になった。
- local：formatとworkspace全体のclippy。diagnostics・harness・checker・compiler・conformanceのtest（16 targets、2,087件。
  CLIのtestのclippy指摘を直した後にcompilerの`contracts` 186件を再実行）。`@noErrorTruncation`を持つ6 caseを
  1件ずつ（`excessivelyDeepConditionalTypes`は監督無しで20分を越えたので止めた）と、filter `runcation`・`nion`・
  `iteral`で、下がった構成は無かった。
- hosted：PR #662（head `d2e55b7c8`）、run 37213652900 — `plan` 21s、`rust` 9m27s、`conformance (TypeScript 7.1)` 20m31s、
  `gates` 13s。
- perf（README corpora、nice 20、main（P3-5ajのbuild `ec3bc970c`）対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 128→144、zod 563→549、Playwright 402→384、TypeScript `src/compiler`
    362→351、Next.js 879→809、Effect 571→542、VS Code 3,853→3,694。tsc-rs÷tsgo 0.60–0.96。読み込んだ文書数は7 corpora、
    診断は6 corporaで同一。VS Codeはtsgoと同一になった。
  - honoは10回のA/B：`--noEmit` 126→126、`bench-full` 146→147。1 checkerの命令数branch÷main 1.00052。3回のmedianの差はnoise。
  - `tsconfig.bench-full.json` 3回：hono 151→155、zod 689→684、Playwright 542→530、TypeScript `src/compiler` 574→528、
    Next.js 1,108→1,102、Effect 834→838。tsc-rs÷tsgo 0.61–0.79。6 corporaとも出力fileと診断は同一。劣化無し。

## P3-5al JSONの値の検証とJSONを型検査しないこと（2026-10-05）

P3-5akの後、`require`で読むJSONの3件：
- **JSONの値の検証**：tsgoのJSON parserは、JSON source fileの値を解析の終わりに検証する（parser/parser.go:218-279）：
  文字列とproperty名は二重引用符（TS1327）、要素はproperty assignment（TS1136）、値はliteral、負の数、object、
  array（TS1328）。tsc 6.0にはこの検証が無く、tsconfigの変換だけが同じ診断を出していた。Programの
  JSON source file（`resolveJsonModule`）に入れた。tsconfigは今までどおり変換の診断を使う：tsgoの`tsoptions`の
  変換は6.0と違い（TS1327を変換では出さない、TS5024はtriviaを含む位置で型名が`enum`、構文診断のある拡張設定は
  処理しない）、別のsliceにする。
- **JSONを型検査しない**：tsgoの`canIncludeBindAndCheckDiagnostics`が検査するのはTypeScript、plain JS、
  checked JSだけ（compiler/program.go:856-873）。tsc-rsはJSON fileも検査し、計算名や省略形のpropertyで
  TS2304／TS18004を出していた。
- unit test：CLI（tsgoの出力にpin）で単一引用符のkeyと値、計算名、`undefined`の値。
- conformance（release build、`8f6353a9b`、`--workers 2`、547 s）：
  - 15,228構成、lane A 13,467（変化なし）。
  - errors full 13,368→13,371：`requireOfJsonFileWithComputedPropertyName`、`requireOfJsonFileWithErrors`、
    `requireOfJsonFileWithoutResolveJsonModule`。mismatch 68→65。
  - emitは変化なし。下がった構成も、不一致のまま描いた文が変わった構成も無い。
- ratchet：0 regressions、3行raise。
- local：formatとworkspace全体のclippy。syntax・binder・program・harness・emitter・checker・compiler・conformanceの
  test（60 targets、3,580件）。filter `son`・`equire`（tsconfigにも検証を入れていた途中のbuild）で下がった構成は無かった。
- hosted：PR #663（head `4cf514532`）、run 37215664101 — `plan` 29s、`rust` 10m23s、`conformance (TypeScript 7.1)` 14m19s、
  `gates` 13s。
- perf（README corpora、nice 20、main（P3-5akのbuild `d2e55b7c8`）対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 145→151、zod 561→571、Playwright 393→389、TypeScript `src/compiler`
    363→353、Next.js 875→832、Effect 533→555、VS Code 3,857→3,689。tsc-rs÷tsgo 0.58–0.97。読み込んだ文書数と診断は
    7 corporaで同一。
  - 10回のA/B：hono `--noEmit` 129→126、Effect `--noEmit` 530→510、zod `bench-full` 637→641。1 checkerの命令数
    branch÷main：hono 1.00038、Effect 0.99971。3回のmedianの差はnoise。
  - `tsconfig.bench-full.json` 3回：hono 158→158、zod 673→695、Playwright 534→521、TypeScript `src/compiler` 573→525、
    Next.js 1,134→1,085、Effect 823→821。tsc-rs÷tsgo 0.57–0.79。6 corporaとも出力fileと診断は同一。劣化無し。

## P3-5am configのエラーでProgramを止めないこととconfigの名前（2026-10-05）

P3-5alの後、CLIのconfigの扱いの3件：
- **configのエラーでProgramを止めない**：tsgoはconfigが何を報告してもProgramを作り、検査し、出力する。configの
  解析の診断（`GetConfigFileParsingDiagnostics`）とoptionの診断はProgramの診断と一緒に出す（compiler/program.go:
  2010-2065）。optionの行がある時だけ意味の診断を出さない（`GetDiagnosticsOfAnyProgram`）。tsc-rsのconfigの
  loaderはconfigの診断（変換のTS5024、未知のoptionのTS5023など）とoptionの行で止まり、それだけを出していた。
  loaderの関門は対応範囲の検査だけにし、公開の`validate_config_plan`は今までどおり両方の一覧を返す。
- **configの名前**：tsgoはconfigを正規化した絶対pathで名付け（`GetParsedCommandLineOfConfigFile`、
  tsoptions/tsconfigparsing.go:2071）、診断ではcurrent directoryからの相対で書く。tsc-rsは`-p`の綴りのままで
  名付けたので、`-p .`では`./tsconfig.json(..)`と書いていた。includeのpatternの説明はtsgoと同じく絶対pathになった。
- **no-emitの経路が受け付けるもの**：未知のoptionの名前（TS5023は報告済み）、出力にだけ効くsource mapのoption
  （`inlineSourceMap`、`inlineSources`、`sourceRoot`、`mapRoot`。その行はplanが出す）、使わないroot scope
  （`watchOptions`、`typeAcquisition`）を、emitの経路と同じく受け付ける。`incremental`などbuildinfoの意味を持つ
  optionとproject referencesは今までどおり止める。
- unit test：CLI（tsgoの出力にpin）でconfigの3種のエラーと型の誤り、`-p`の4つの綴りでのconfigの名前とincludeの
  patternの説明。programのtestは、configの診断で止まるとしていた3件と、継承したroot scopeの1件をtsgoの挙動に
  合わせた。
- conformance（release build、`4628a7e7f`、`--workers 2`、547 s）：
  - 15,228構成、lane A 13,467、errors full 13,371、emit full 13,376（`intersectionConstructorReductionCrash`を
    除く）で変化なし。変えたのはCLIとconfigのloaderで、conformanceのharnessの経路は変わらない。
  - `intersectionConstructorReductionCrash`は今回、memory制限で止まった後の再実行で最後まで通り、errorsとemitが
    fullになった。負荷で結果の変わる構成なので、今までどおりratchetに入れない。
- ratchet：0 regressions、変更無し。
- local：formatとworkspace全体のclippy。program・compilerのtest（13 targets、784件）。CLIの出力はtsgoと、configの
  変換の誤り、未知のoption、optionの衝突（TS5053）、`inlineSources`・`sourceRoot`（TS5051）、`mapRoot`（TS5069）、
  `--noEmit`の指定、`noEmitOnError`、`-p`の綴り、includeのpatternの説明、`watchOptions`と`typeAcquisition`で同一。
- 残り：tsgoはtsconfigの`watchOptions`を変換しない（tsoptions/tsconfigparsing.goに無い）ので、その値の誤りを報告
  しない。tsc-rsはTS6046を出す（emitの経路で以前からの違い）。tsconfigの変換をtsgoの`tsoptions`に合わせるslice
  （P3-5alの残り）で扱う。
- hosted：PR #664（head `11aba33eb`）、run 37217585427 — `plan` 29s、`rust` 9m53s、`conformance (TypeScript 7.1)` 20m31s、
  `gates` 11s。
- perf（README corpora、nice 20、main（P3-5alのbuild `90f930b6a`）対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 136→137、zod 523→509、Playwright 363→364、TypeScript `src/compiler`
    368→323、Next.js 798→774、Effect 525→481、VS Code 3,412→3,398。tsc-rs÷tsgo 0.59–0.94。読み込んだ文書数と診断は
    7 corporaで同一。
  - `tsconfig.bench-full.json` 3回：hono 145→146、zod 642→633、Playwright 491→479、TypeScript `src/compiler` 549→493、
    Next.js 1,070→1,025、Effect 758→762。tsc-rs÷tsgo 0.61–0.79。6 corporaとも出力fileと診断は同一。変えたのはconfigの
    読み込みだけで、差はnoiseの範囲。劣化無し。

## P3-5an 小さな検査と構文の10件（2026-10-05）

P3-5amの後、残りの不一致のうち、それぞれ小さく閉じる10件：
- **TS6807の提案**：tsgoの`errorOrSuggestion`は、enum memberの外で32以上のshiftを提案として出す（checker.go:
  12604-12612、複合代入でも）。tsc-rsはenum memberの中のerrorだけだった。`overshifts`。
- **assertionの後の二項式**：tsgoの`parseBinaryExpressionRest`は、`a ## b as T $$ c`で`as T`を消すと`$$`が`##`より
  先に結び付く時、assertionの後で式を終える（parser.go:4686-4703、TypeScript issue 63527）。
  `disallowUnerasableAssertion`（errorsとemit）。
- **JSXの属性値の前の空白**：tsgoの`ScanJsxAttributeValue`は`=`の後の空白と改行を飛ばし、tokenを引用符から始める
  （scanner.go:1330-1347）。`jsxMultilineAttributeStringValues2`（errorsとemit）。
- **constructorのaccess**：tsgoの`getConstructorAccessibilityError`は全てのconstruct signatureを見る（checker.go:
  8834-8862）。class型の交差でも、privateやprotectedのconstructorを持つclassを報告する。TS2675の名前は
  そのconstructorを宣言したclass。`extendPrivateConstructorClass2`。
- **import typeの属性**：tsgoの`checkImportType`は、属性の値が文字列literalであること（TS2858）と、属性の型が
  大域の`ImportAttributes`型に代入できることを検査する（checker.go:3372-3381）。TS2858のmessageは`assert`でも同じ
  （tsc 6.0のTS2837は使わない）。`importAttributes12`。
- **分割代入のprivate identifier**：TS18064（checker.go:5979、12805）。`privateNamesNotAllowedAsDestructuringPatterns`。
- **namespaceの中のexportされたclass**：tsgoの`parseClassDeclarationOrExpression`は、source elementsの中でblockや
  switch節の外にあるexportされたclassにだけ、moduleのtop levelのawait文脈を与える（parser.go:1753-1759）。
  namespaceの本体はblockなので、その中の`await`はTS1308。`awaitInNamespaceExportedClassComputedProperty`。
- **`??`のnullishの意味**：tsgoの`getSyntacticNullishnessSemantics`は、`??`と`??=`を左のoperandのnullishの道筋で
  決める（checker.go:13185-13194）。`b ?? null`などが`??`の左にある時のTS2871の誤検出が消えた。
  `nullishCoalescingAlwaysNullFalsePositive`、`predicateSemantics`。
- **binary file**：tsgoのscannerは、tokenの外のU+FFFDでTS1490をfileの先頭に出す（scanner.go:931-936）。
  `TransportStream`。
- **CommonJSの分割代入の`require`**：`--module preserve`のTS1293の対象から`BindingElement`も外す（checker.go:7011）。
  `modulePreserveRequireDestructuring`。
- unit test：CLI（tsgoの出力にpin）で9件（assertion、JSXの属性値、constructorのaccess、import typeの属性、TS18064、
  namespaceのawait、`??`、binary file（pretty出力も）、CommonJSの`require`）。checkerのTS6807のtestは提案として
  pinし直した。
- conformance（release build、`908ccd489`、`--workers 2`、514 s）：
  - 15,228構成、lane A 13,467（変化なし）。
  - errors full 13,371→13,383：上の10件の12構成（`corrupted`もTS1490で一致）。mismatch 65→53。
  - emit full 13,376→13,378：`disallowUnerasableAssertion`、`jsxMultilineAttributeStringValues2`。emit mismatch 61→59。
  - 下がった構成も、不一致のまま描いた文が変わった構成も無い。`intersectionConstructorReductionCrash`は今回は
    memory制限でharness error（負荷で結果の変わる構成で、ratchetの外）。
- ratchet：0 regressions、12行raise。
- local：formatとworkspace全体のclippy。syntax・checker・compilerのtest（19 targets、2,252件）、CLIのtest 2件を加えた後の
  compilerの`contracts` 198件。最初の5件を入れたbuildのfilter `mportAttribute`・`mportAssertion`・`hift`・`atisfies`・
  `sOperator`・`ssertion`・`onstructor`・`rivate`で下がった構成は無かった。CLIの出力は9件の例でtsgoと同一。
- 残り（次のslice）：
  - `objectBindingPatternDefaultMissingElements`：tsgoの`padObjectLiteralType`は既定値の無い要素も補い、TS7031を出す
    （checker.go:17142-17168）。tsc-rsはtsc 6.0の、既定値の有る要素だけを補う形。
  - `processingDiagnosticSkipLibCheck`・`processingDiagnosticTsIgnore`：tsgoはinclude processorの位置付きの診断を
    そのfileの意味診断とし、`SkipTypeChecking`と直前の`@ts-ignore`で除く（compiler/program.go:840-846）。
  - `exportAssignmentMerging8`：`export =`のmoduleからの名前付きimportは、元のmodule symbolのexportを探す
    （checker.go:14947-14951、16518-16542）。
  - `iterationErrorOverNotIterableUnions1`：tsgoの`getIterationTypesOfIterable`はcacheを型とuseで持ち、errorを報告する
    時はcacheされた失敗を計算し直す（checker.go:6435-6473）。
  - `awaitedTypeNoLib`：`Awaited`のTS2318を要求する経路（`maybeAddMissingAwaitInfo`からと見られる）は未調査。
- hosted：PR #665（head `d186d3410`）、run 37219473404 — `plan` 28s、`rust` 10m21s、`conformance (TypeScript 7.1)` 20m1s、
  `gates` 12s。
- perf（README corpora、nice 20、main（P3-5amのbuild `e52457b1b`）対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 153→148、zod 542→535、Playwright 370→373、TypeScript `src/compiler`
    342→341、Next.js 837→815、Effect 536→533、VS Code 3,576→3,542。tsc-rs÷tsgo 0.61–0.94。読み込んだ文書数と診断は
    7 corporaで同一。
  - `tsconfig.bench-full.json` 3回：hono 155→155、zod 676→671、Playwright 521→511、TypeScript `src/compiler` 569→533、
    Next.js 1,131→1,083、Effect 823→794。tsc-rs÷tsgo 0.57–0.82。6 corporaとも出力fileと診断は同一。差はnoiseの範囲で、
    劣化無し。

## P3-5ao 束縛の補い、include processorの診断、`export =`の型、unionのiteration（2026-10-05）

P3-5anで残した4件：
- **束縛の補い**：tsgoの`padObjectLiteralType`は、初期化子に無いpropertyを、rest要素以外の全ての要素について補う
  （checker.go:17142-17168）。既定値の無い要素は暗黙の`any`（TS7031）になる。tsc-rsはtsc 6.0の、既定値の有る要素
  だけを補う形で、TS2339やTS2353を出していた。`objectBindingPatternDefaultMissingElements`。
- **include processorの診断**：tsgoは、Programの行のうちsource fileに位置を持つもの（解決できない参照、fileを含む
  理由の説明、解決の診断）を、そのfileの意味診断として出す。fileが型検査の対象外（`skipLibCheck`の宣言fileなど）
  なら出さず、直前のcomment directiveでも除く（`GetIncludeProcessorDiagnostics`、compiler/program.go:840-846）。
  tsc-rsはcompilerがこれらをそのまま意味診断に加えていた。module providerがsourceのtokenを付けて行をcheckerに渡し、
  checkerは既存のfile単位の組み立て（型検査の対象外のfileを飛ばし、新しいdirective表で除く）に加える。compilerは
  どのsourceにも属さない行だけをoptionsの診断にする。`processingDiagnosticSkipLibCheck`、`processingDiagnosticTsIgnore`。
- **`export =`のmoduleの型**：tsgoでは、`export =`で定義したmoduleも元のmoduleの型とnamespaceの宣言をexportし
  （`getExportsOfModuleWorker`、checker.go:16518-16542）、名前付きimportは元のmodule symbolで探す
  （`getExternalModuleMember`、checker.go:14947-14951）。`exportAssignmentMerging8`。
- **unionのiteration**：tsgoの`getIterationTypesOfIterable`は、errorを報告する時はcacheされた失敗を計算し直し、その
  結果はcacheしない（checker.go:6440-6454）。unionの各constituentはerrorの位置無しで解決し、iterableでないものが
  あればunionを報告する（checker.go:6458-6473）。iterableでないunionを使う度にTS2488が出る。
  `iterationErrorOverNotIterableUnions1`。
- unit test：CLI（tsgoの出力にpin）で4件（include processorの3つの場合、束縛の補い、`export =`の型、unionの
  iteration）。
- conformance（release build、`9b029aa74`、`--workers 2`、522 s）：
  - 15,228構成、lane A 13,467（変化なし）。
  - errors full 13,383→13,389：上の4件の6構成と、束縛の補いによる`inferredRestTypeFixedOnce`。mismatch 53→47。
  - emitは変化なし。下がった構成も、不一致のまま描いた文が変わった構成も無い。`intersectionConstructorReductionCrash`は
    今回もmemory制限でharness error（ratchetの外）。
- ratchet：0 regressions、6行raise。
- local：formatとworkspace全体のclippy。checker・compiler・harness・conformanceのtest（14 targets、2,054件）。filter
  `terat`・`ForOf`・`orOf`・`pread`・`estructur`・`ield`（unionのiterationを入れたbuild）で下がった構成は無かった。
  CLIの出力は6つの例でtsgoと同一で、修正前のbuildでは4つが違っていた。
- 残り：`finallyLogicalOrAssignmentSwitchReturn`（tsgoの`isReachableFlowNodeWorker`はReduceLabelを辿る間は共有nodeの
  cacheを使わない、checker/flow.go:2528-2537）、`awaitedTypeNoLib`（harnessでだけ出る`Awaited`のTS2318）。
- hosted：PR #666（head `158990b35`）、run 37221135508 — `plan` 31s、`rust` 9m52s、`conformance (TypeScript 7.1)` 21m38s、
  `gates` 14s。
- perf（README corpora、nice 20、main（P3-5anのbuild `908ccd489`）対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 134→144、zod 558→545、Playwright 366→370、TypeScript `src/compiler`
    371→335、Next.js 816→778、Effect 550→550、VS Code 3,600→3,507。tsc-rs÷tsgo 0.57–0.96。
  - 出力：honoとNext.jsで診断が増えた。honoは`src/helper/css/index.ts`のTS7031が2件で、出力がtsgoと同一になった
    （終了状態も2）。Next.jsは`src/shared/lib/router/adapters.tsx`のTS7031が2件で、tsgoと同じ行。Next.jsのtsgoとの
    残りの差は以前からのTS2321（typeboxの3行）。他の5 corporaは同一。READMEの計測の節はtsc 6.0.3との比較で、
    honoが誤り無しという記述はその時点のもの。
  - 10回のA/B：hono `--noEmit` 131→129、Effect `--noEmit` 524→528、zod `bench-full` 639→634。1 checkerの命令数
    branch÷main：hono 0.99917、Effect 0.99950。3回のmedianのhonoの差はnoise。
  - `tsconfig.bench-full.json` 3回：hono 151→149、zod 661→675、Playwright 500→501、TypeScript `src/compiler` 555→515、
    Next.js 1,097→1,066、Effect 797→800。tsc-rs÷tsgo 0.58–0.79。出力fileは6 corporaで同一、診断はhonoとNext.jsで
    上と同じだけ増えた。劣化無し。

## P3-5ap 到達可能性のcache、束縛の親の型、JSXの型と名前（2026-10-05）

P3-5aoの後の4件：
- **到達可能性のcache**：tsgoの`isReachableFlowNodeWorker`は、ReduceLabelがlabelの前件を狭めている間は、共有nodeの
  到達可能性をcacheから読まず、書かない（checker/flow.go:2528-2537）。tsc 6.0はcacheしたので、`finally`の中の
  論理代入を辿った結果が残り、網羅したswitchの後を到達可能と見てTS2366を出していた。
  `finallyLogicalOrAssignmentSwitchReturn`。
- **束縛の親の型**：tsgoの`getTypeForBindingElementParent`は、strictNullChecksで省略可能な宣言の時、cacheされた
  parameterの型（省略可能性で`undefined`を含みうる）を使わない（checker.go:18031-18041）。
  `bindingPatternOptionalParameterCached`。
- **JSXの型**：tsgoの`instantiateAliasOrInterfaceWithDefaults`は、型の別名でもclassやinterfaceでもない宣言型
  （enumの`JSX.ElementType`など）には型を返さない（jsx.go:1030-1048）。tsc-rsはtsc 6.0のcrashを避ける分岐で宣言型を
  返し、TS2786を出していた。`jsxElementTypeUnexpectedType`。
- **JSXの名前**：tsgoの`getLiteralTypeFromPropertyName`は、JSXの属性名`ns:name`をその文字列のliteral型にする
  （`GetPropertyNameForPropertyNameNode`）。tsc-rsは式として検査し、合わないtemplate literalのindex signatureと
  比べていた。`jsxNamespacedNameNotComparedToNonMatchingIndexSignature`。
- unit test：CLI（tsgoの出力にpin）で4件。修正前のbuildでは4件ともtsgoと違っていた。
- conformance（release build、`092d910b3`、`--workers 2`、525 s）：
  - 15,228構成、lane A 13,467（変化なし）。
  - errors full 13,389→13,395：上の4件と、到達可能性のcacheによる`dependentDestructuredVariablesNoCrash1`（category→
    full）。mismatch 47→43、category 6→5。
  - emitは変化なし。下がった構成も、不一致のまま描いた文が変わった構成も無い。`intersectionConstructorReductionCrash`は
    今回は最後まで通ったが、負荷で結果の変わる構成なので、ratchetの更新ではreportから除いた。
- ratchet：0 regressions、5行raise。
- local：formatとworkspace全体のclippy。checker・compiler・harness・conformanceのtest（14 targets、2,058件）。
- perf（README corpora、nice 20、main（P3-5aoのbuild `9b029aa74`）対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 158→136、zod 535→531、Playwright 370→367、TypeScript
    `src/compiler` 343→329、Next.js 784→790、Effect 517→525、VS Code 3,515→3,536。tsc-rs÷tsgo 0.60–0.91。読み込んだ
    文書数と診断は7 corporaで同一。
  - `tsconfig.bench-full.json` 3回：hono 149→147、zod 636→627、Playwright 480→505、TypeScript `src/compiler` 544→508、
    Next.js 1,031→999、Effect 771→759。6 corporaとも出力fileと診断は同一。
  - 10回のA/B：Playwright `bench-full` 476→478、Effect `--noEmit` 516→527、Next.js `--noEmit` 768→766。1 checkerの
    命令数branch÷main：Playwright 0.99977、Effect 1.00005。3回のPlaywrightの差とEffectの差はnoiseで、劣化無し。
- hosted：PR #667（head `a5572a012`）、run 37222682145 — `plan` 22s、`rust` 8m40s、`conformance (TypeScript 7.1)` 20m15s、
  `gates` 14s。

## P3-5aq bigintの順序、常にstrictなbinder、`Object`の予約、TS5074（2026-10-05）

P3-5apの後の4件：
- **bigint literalの順序**：tsgoの`CompareTypes`はbigint literal型を値で並べる（checker/utilities.go:563-566、
  `PseudoBigInt.Compare`）。tsc-rsの安定した型の順序にはこの枝が無く、作られた順になっていた。`parseBigInt`。
- **常にstrictなbinder**：tsgoのbinderはstrict modeを持たず、strict modeの検査を常に行い、関数宣言を常にblock scopeで
  束縛する（binder/binder.go:1219-1225、1374-1440）。tsc-rsはtsc 6.0の`bindInStrictMode`で、宣言fileやmoduleでない
  scriptをstrictでないとしていた。`parserWithStatement1.d.ts`（`.d.ts`の`with`のTS1101）。
- **`Object`の予約**：tsgoの`checkCollisionWithGlobalObjectInGeneratedCode`は、CommonJSのmoduleのtop levelにある
  class以外の`Object`の宣言をTS2441にする（checker.go:10680-10694、noEmitでは出さない）。
  `objectNameCollisionCommonJS(module=commonjs)`。
- **TS5074**：tsgoの`verifyCompilerOptions`は、`tsBuildInfoFile`もconfig fileも無い`incremental`を報告する
  （compiler/program.go:1068-1070）。`incrementalInvalid`（errors。emitは`incremental`の出力が未対応のまま）。
- unit test：CLI（tsgoの出力にpin）で3件（bigintの順序、`.d.ts`の`with`、`Object`の予約）。修正前のbuildでは3件とも
  tsgoと違っていた。
- conformance（release build、`f881a216e`、`--workers 2`、516 s）：
  - 15,228構成、lane A 13,467（変化なし）。
  - errors full 13,395→13,398：上の4件の構成が上がり、前回最後まで通った`intersectionConstructorReductionCrash`が
    今回はmemory制限のharness error（負荷で結果の変わる構成で、ratchetの外）。mismatch 43→40、category 5→4。
  - emit full 13,379→13,379：`comparisonBigIntLiterals`の宣言のunionの順がtsgoと同じになりemitがfullに上がり、
    `intersectionConstructorReductionCrash`の分が下がった。下がった構成も、不一致のまま描いた文が変わった構成も無い。
- ratchet：0 regressions、6行raise（`comparisonBigIntLiterals`のemitを含む）。
- local：formatとworkspace全体のclippy。types・binder・checker・compiler・harness・conformanceのtest（20 targets、2,182件）。
  binderを変えた後のfilter `trict`・`igint`・`ith`（1,948 case）・`rguments`・`val`・`elete`・`abel`で下がった構成は無かった。
- perf（README corpora、nice 20、main（P3-5apのbuild `092d910b3`）対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 135→125、zod 521→528、Playwright 359→369、TypeScript
    `src/compiler` 363→327、Next.js 795→764、Effect 527→540、VS Code 3,477→3,487。tsc-rs÷tsgo 0.62–0.96。読み込んだ
    文書数と診断は7 corporaで同一。
  - `tsconfig.bench-full.json` 3回：hono 147→141、zod 617→634、Playwright 494→488、TypeScript `src/compiler` 577→494、
    Next.js 1,067→1,046、Effect 792→755。6 corporaとも出力fileと診断は同一。
  - 10回のA/B：zod `bench-full` 620→621、Effect `--noEmit` 522→511、Playwright `--noEmit` 362→363。1 checkerの命令数
    branch÷main：zod 0.99969、Effect 1.00055。3回の差はnoiseで、劣化無し。
- hosted：PR #668（head `baf19f2e4`）、run 37224328513 — `plan` 33s、`rust` 10m23s、`conformance (TypeScript 7.1)` 21m10s、
  `gates` 14s。

## P3-5ar `require`のmodule名とaccessorの重複（2026-10-05）

P3-5aqの後の2件：
- **`require`のmodule名**：tsgoの`getExternalModuleMember`は`require`の宣言の引数をmodule指定子として読み
  （`getExternalModuleRequireArgument`）、TS2305は書かれたとおりの名前（`"./mod"`）を出す。tsc-rsは`require`の宣言で
  指定子を見つけられず、module symbolの名前（CLIでは絶対path）を出していた。`commonJSAliasedExport`。
- **accessorの重複**：tsgoの`declareSymbolEx`は、accessorが別種の宣言と衝突した時、symbolを両方のaccessorとして印を
  付けるので、後に続く別種のaccessorも重複になる（binder/binder.go:279-285）。`get x`・`x()`・`set x`の3つ目にも
  TS2300が出る。`duplicateIdentifierChecks`。
- unit test：CLI（tsgoの出力にpin）で2件。修正前のbuildでは2件ともtsgoと違っていた。
- conformance（release build、`8ee2087a3`、`--workers 2`、511 s）：
  - 15,228構成、lane A 13,467（変化なし）。
  - errors full 13,398→13,400：上の2件（`commonJSAliasedExport`はcategory→full）。mismatch 40→39、category 4→3。
  - emitは変化なし。下がった構成も、不一致のまま描いた文が変わった構成も無い。`intersectionConstructorReductionCrash`は
    今回もmemory制限でharness error（ratchetの外）。
- ratchet：0 regressions、2行raise。
- local：formatとworkspace全体のclippy。binder・checker・compiler・harness・conformanceのtest（17 targets、2,141件）。
  filter `uplicate`・`ccessor`で下がった構成は無かった（`uplicate`のharness error 2件は以前からの`deduplicatePackages`）。
- perf（README corpora、nice 20、main（P3-5aqのbuild `f881a216e`）対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 140→138、zod 520→524、Playwright 367→358、TypeScript
    `src/compiler` 353→335、Next.js 799→795、Effect 511→535、VS Code 3,473→3,437。tsc-rs÷tsgo 0.59–0.95。読み込んだ
    文書数と診断は7 corporaで同一。
  - `tsconfig.bench-full.json` 3回：hono 147→144、zod 633→628、Playwright 506→488、TypeScript `src/compiler` 582→527、
    Next.js 1,041→1,026、Effect 738→766。6 corporaとも出力fileと診断は同一。
  - 10回のA/B：Effect `--noEmit` 524→516、Effect `bench-full` 740→753、zod `--noEmit` 522→518。1 checkerの命令数
    branch÷main：zod 1.00042、Effect 1.00008。3回のEffectの差はnoiseで、劣化無し。
- hosted：PR #669（head `88c4db3a8`）、run 37225784962 — `plan` 27s、`rust` 10m3s、`conformance (TypeScript 7.1)` 20m27s、
  `gates` 12s。

## P3-5as カンマの優先順位とBigIntのmetadata（2026-10-05）

P3-5arの後、emitの2件：
- **カンマの優先順位**：tsgoのprinterは計算プロパティ名とJSXの式を`OperatorPrecedenceDisallowComma`で出す
  （`emitComputedPropertyName`、`emitJsxExpression`、printer/printer.go:1221、4369）。書かれたカンマ式も括弧で
  囲まれる（`[(0, 1)]`、`{(class1, class2)}`）。tsc-rsはtsc 6.0の、factoryが置き換えた式だけを囲む形だった。
  `parserComputedPropertyName35`、`jsxParsingError1`。
- **BigIntのmetadata**：tsgoの`serializeBigIntConstructor`は、ES2020未満で`typeof BigInt === "function" ? BigInt :
  Object`を出す（transformers/tstransforms/typeserializer.go:388-399）。`emitDecoratorMetadataBigIntFallback`。
- unit test：CLI（tsgoの出力にpin）で2件（計算プロパティ名とJSXのカンマ、BigIntのmetadata）。
- conformance（release build、`ca9a186d5`、`--workers 2`、535 s）：
  - 15,228構成、lane A 13,467（変化なし）。errors full 13,400→13,401は`intersectionConstructorReductionCrash`
    （今回は最後まで通った。負荷で結果の変わる構成で、ratchetの外）。
  - emit full 13,379→13,383：上の3構成と`intersectionConstructorReductionCrash`。emit mismatch 58→55。
    `plainJSGrammarErrors`は計算プロパティ名の行がtsgoと同じになり、残る差は以前からの278行目（`async export`）。
  - 下がった構成は無い。
- ratchet：0 regressions、3行raise（emit）。
- local：formatとworkspace全体のclippy。emitter・compiler・harness・conformanceのtest（35 targets、901件）、CLIのtest 2件を
  加えた後のcompilerの`contracts` 213件。filter `omputed`・`jsx`・`tsx`・`etadata`・`ecorator`・`igint`で下がった構成は
  無かった。
- perf（README corpora、nice 20、main（P3-5arのbuild `8ee2087a3`）対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 135→128、zod 523→506、Playwright 368→361、TypeScript
    `src/compiler` 328→324、Next.js 775→765、Effect 488→516、VS Code 3,425→3,343。tsc-rs÷tsgo 0.58–0.94。読み込んだ
    文書数と診断は7 corporaで同一。
  - `tsconfig.bench-full.json` 3回：hono 148→146、zod 627→648、Playwright 494→490、TypeScript `src/compiler` 548→513、
    Next.js 1,058→1,026、Effect 763→762。6 corporaとも出力fileと診断は同一。
  - 10回のA/B：Effect `--noEmit` 507→526（min 477→466）、zod `bench-full` 642→646、Next.js `bench-full` 1,032→1,029。
    1 checkerの命令数branch÷main：zod 1.00020、Effect 0.99986。変更はemitterだけで`--noEmit`には効かず、差はnoise。
    劣化無し。
- hosted：PR #670（head `81661e22d`）、run 37227406231 — `plan` 22s、`rust` 8m0s、`conformance (TypeScript 7.1)` 20m8s、
  `gates` 11s。

## P3-5at JSファイルと置き場所の誤ったimport/export（2026-10-05）

P3-5asの後、import elisionの範囲とmodule変換の5件：
- **JSファイル**：tsgoのimport elisionは、verbatimModuleSyntaxでないTypeScript fileだけで走る（`importElisionEnabled`、
  compiler/emitter.go:115）。JSファイルとverbatimModuleSyntaxではtype eraserだけが通り、書かれた`import {}`と
  `export {}`を残し（typeeraser.go:325-329、362-367）、TypeScriptを含まない文には触れない（typeeraser.go:44-46）。
  type-onlyのimportとexport、`export =`はTypeScriptとして数える（ast.go:1868-1922）。tsc-rsはtsc 6.0の、JSファイルでも
  空のimportとexportや値でないexportを消す形で、消した後に`export {};`を末尾に足していた。
  `bundlerSyntaxRestrictions`（2構成）、`thisInObjectJs`、`plainJSGrammarErrors`（`async export`の修飾子）、
  `jsDeclarationsInterfaces(target=es2015)`（interfaceのexportがCommonJSの`exports.G = void 0`に残る）。
- **置き場所の誤ったimportとexport**：import elisionはsource fileとnamespace本体の文しか見ない
  （importelision.go:117-125）。文法エラーでblockに置かれた`export =`やaliasは束縛を残し、aliasは`var I = M`になる。
  namespaceの中のblockにあるaliasはruntime syntaxの変換が消す（runtimesyntax.go:123-125）。
  `moduleElementsInWrongContext`、`moduleElementsInWrongContext2`。
- **module変換**：tsgoのCommonJS変換はtop-levelのimportとexportだけを変換し（commonjsmodule.go:60-128）、ES module変換は
  全てのnodeを見て、入れ子の`import x = require()`と`export =`もtop-levelと同じに変換する（esmodule.go:35-53）。
  tsc-rsのCommonJS変換はblockの中のimportをtop-levelのものとして変換しようとして内部エラーになり、ES module変換は
  入れ子のものを残していた。ES module変換は、parse treeに入れ子のものがある時だけ全体を辿る。
- **消した埋め込みの文**：tsgoの`EmitContext.VisitEmbeddedStatement`は、変換が消した埋め込みの文（`if`の本体など）の
  位置とcommentを持つ空の文を置く（printer/emitcontext.go:998-1010）。tsc-rsは本体の無い`if`を作ってprintで失敗して
  いた。`typeOnlyExportAsIfBody`（`if (true) export type {};`）。
- **集められないmodule要求**：tsgoはsource fileとambient moduleの文にあるimportだけを集める
  （parser/references.go:11-90）。blockの中のimportの解決はnilで、TS2307になる。tsc-rsは解決表に行が無いとして内部
  エラーで止まっていた。checkerが表に無い要求を引いた時に限り、置き場所の誤ったimportとexportの要求を未処理の要求
  として集める（loaderの計画は全体を辿らない）。
- unit test：CLI（tsgoの出力にpin）で3件（JSファイルの空のimportとexport、置き場所の誤った要素の束縛と解決、module
  変換と消した埋め込みの文）。mainの10月3日のbuildでは、3件ともtsgoと違うか内部エラーだった。
- conformance（release build、`7cbe47bcd`、`--workers 2`、516 s）：
  - 15,228構成、lane A 13,467（変化なし）。errors full 13,401→13,400は`intersectionConstructorReductionCrash`（今回は
    memoryの上限で終わらなかった。負荷で結果の変わる構成で、ratchetの外）。
  - emit full 13,383→13,390：上の8構成から`intersectionConstructorReductionCrash`を引いたもの。emit mismatch 55→47。
  - 下がった構成は無い。
- ratchet：0 regressions、8行raise（emit）。
- local：formatとworkspace全体のclippy。compiler・program・emitterのtest（`e0a28c728`で37 targets、1,446件）、埋め込みの
  文の修正の後にcompilerとemitterのtest（29 targets、864件）。
- perf（README corpora、nice 20、main（P3-5asのbuild `ca9a186d5`）対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 134→140、zod 539→551、Playwright 370→387、TypeScript
    `src/compiler` 358→338、Next.js 799→831、Effect 533→547、VS Code 3,569→3,511。tsc-rs÷tsgo 0.63–0.97。読み込んだ
    文書数と診断は7 corporaで同一。
  - `tsconfig.bench-full.json` 3回：hono 140→147、zod 624→627、Playwright 488→485、TypeScript `src/compiler` 567→514、
    Next.js 1,016→1,042、Effect 791→762。6 corporaとも出力fileと診断は同一。
  - 10回のA/B：Effect `--noEmit` 518→495、Playwright `--noEmit` 363→361、hono `bench-full` 147→142、Next.js
    `bench-full` 997→985。1 checkerの命令数branch÷main：zod `--noEmit` 1.00019、Playwright `--noEmit` 0.99946、hono
    `bench-full` 0.99970、Next.js `bench-full` 0.99982。3回の差はnoiseで、劣化無し。
- hosted：PR #671（head `fa1781cc6`）、run 37231610173 — `plan` 26s、`rust` 6m39s、`conformance (TypeScript 7.1)` 21m0s、
  `gates` 15s。

## P3-5au async変換、`extends null`、アクセスの括弧（2026-10-05）

P3-5atの後、ES変換の5件：
- **static初期化子のasync arrow**：tsgoのasync arrowはawaitのfactだけを持ち、lexicalな`this`を含まない
  （ast.go:2064-2072）。class fieldsの変換は、移したstatic初期化子とstatic blockのIIFEに`EFNoLexicalThis`を付け
  （classfields.go:1417、2714-2718）、async変換はその中で`__awaiter`の`this`に`void 0`を渡す（async.go:127-130）。
  tsc-rsはtsc 6.0の、async arrowを`this`の参照として数える形で、使われないclass alias（`_a = Test`）を作り、namespaceの
  中では`__awaiter(_a, …)`を出していた。`asyncArrowInClassES5(target=es2015)`、`asyncArrowStaticFieldThis`。
- **引数の`super`**：tsgoの`transformAsyncFunctionBody`は、generatorへ移す引数を訪れる前に`_super`の捕捉を開く
  （async.go:713-731）。tsc-rsは本体の`super`だけを数え、`async k(b = super.m())`で内部エラーになっていた。
  `asyncSuperDefaultParameters`。
- **async generatorの`_super`**：tsgoは捕捉したpropertyがある時だけ`_super`のobjectを作る（forawait.go:827-831）。
  tsc-rsはtsc 6.0の、要素アクセスだけでも空の`_super`を作る形だった。`asyncMethodWithSuper_es6`。
- **`extends null`**：tsgoのES decoratorの変換は、`null`を継承するclassに合成するconstructorで`super(...arguments)`を
  呼ばない（esdecorator.go:737）。`esDecoratorExtendsNull`。
- **アクセスの括弧**：tsgoの`parenthesizeLeftSideOfAccess`は、引数の無い`new`とoptional chain以外の左辺式を括弧で
  囲まない（ast/utilities.go:396-408）。tsc-rsのES2020の変換は左辺式の一部（instantiation expressionなど）を数えず、
  `((a === null || …)).d`と二重に囲んでいた。`optionalChainWithInstantiationExpression1`（2構成）。
- unit test：CLI（tsgoの出力にpin）で4件（static初期化子のasync arrow、引数とasync generatorの`super`、
  `extends null`、アクセスの括弧）。P3-5atのbuildでは4件ともtsgoと違うか内部エラーだった。
- conformance（release build、`dc9ff072c`、`--workers 2`、509 s）：
  - 15,228構成、lane A 13,467（変化なし）、errors full 13,400（変化なし）。
  - emit full 13,390→13,396：上の6構成。emit mismatch 47→41。下がった構成は無い。
- ratchet：0 regressions、6行raise（emit）。
- local：formatとworkspace全体のclippy。compilerとemitterのtest（29 targets、868件）。filter `Chain`（83構成）で下がった
  構成は無かった。
- perf（README corpora、nice 20、main（P3-5atのbuild `7cbe47bcd`）対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 131→124、zod 514→511、Playwright 368→365、TypeScript
    `src/compiler` 344→327、Next.js 796→760、Effect 519→512、VS Code 3,423→3,390。tsc-rs÷tsgo 0.61–0.93。読み込んだ
    文書数と診断は7 corporaで同一。
  - `tsconfig.bench-full.json` 3回：hono 143→149、zod 624→633、Playwright 483→474、TypeScript `src/compiler` 528→491、
    Next.js 1,032→1,012、Effect 764→754。6 corporaとも出力fileと診断は同一。
  - 10回のA/B：zod `bench-full` 628→609、Playwright `--noEmit` 362→360、hono `bench-full` 144→141、Next.js
    `bench-full` 1,002→993。1 checkerの命令数branch÷main：zod `--noEmit` 0.99945、Playwright `--noEmit` 1.00021、hono
    `bench-full` 0.99932、Next.js `bench-full` 0.99913。3回の差はnoiseで、劣化無し。
- hosted：PR #672（head `979ff750b`）、run 37233885526 — `plan` 36s、`rust` 9m44s、`conformance (TypeScript 7.1)` 20m9s、
  `gates` 16s。

## P3-5av 一時変数の`var`文、namespaceの名前、`import =`の指定子（2026-10-05）

P3-5auの後、emitの3件：
- **`??`とoptional chainの一時変数**：tsgoは`??`とoptional chainを別の変換で下げ（estransforms/definitions.go:16）、
  `EmitContext.MergeEnvironment`は後の変換の`var`文を先に置く。各scopeはoptional chainの一時変数、`??`の一時変数の
  順に別の`var`文で宣言し、printerはその順に名前を付ける。tsc-rsはtsc 6.0の1つの変換の形で、1つの`var`文に
  まとめていた。`instantiationExpressionErrors`。
- **namespaceとenumの名前**：tsgoのCommonJS変換は、enumとnamespaceの宣言名を`exports.`で置き換えない
  （commonjsmodule.go:2064-2068、moduletransforms/utilities.go:12-20）。exportされたinterfaceと合わさった（exportされて
  いない）namespaceは`Foo || (Foo = {})`のままになる。tsc-rsはcheckerのexportの持ち主に従って
  `exports.Foo || (exports.Foo = {})`にしていた。`defaultExportsCannotMerge04(target=es2015)`。
- **`import =`の指定子**：tsgoのES module変換は、`import x = require()`から作るrequireの呼び出しの指定子も
  rewriteRelativeImportExtensionsで書き換える（`createRequireCall`、esmodule.go:290-296）。
  `rewriteRelativeImportExtensions/emit`（2構成）。
- 既存のtest 2件（namespaceの初期化の形）をtsgoの出力に合わせて直した。`export default function Foo`と合わさった
  namespaceの`exports.Foo_1 = Foo = {}`（tsgo）は、以前からの差として残る。
- unit test：CLI（tsgoの出力にpin）で3件。P3-5auのbuildでは3件ともtsgoと違っていた。
- conformance（release build、`fd39404ac`（testだけを直した`306ddbde3`と同じcode）、`--workers 2`、510 s）：
  - 15,228構成、lane A 13,467（変化なし）、errors full 13,400（変化なし）。
  - emit full 13,396→13,400：上の4構成。emit mismatch 41→37。下がった構成は無い。
- ratchet：0 regressions、4行raise（emit）。
- local：formatとworkspace全体のclippy。compilerとemitterのtest（29 targets、871件）。filter `nullish`・`Coalescing`・
  `ptional`・`Chain`・`amespace`・`num`・`xport`・`odule`・`rewriteRelative`で下がった構成は無かった。
- perf（README corpora、nice 20、main（P3-5auのbuild `dc9ff072c`）対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 137→134、zod 534→515、Playwright 370→371、TypeScript
    `src/compiler` 345→332、Next.js 793→766、Effect 544→525、VS Code 3,463→3,443。tsc-rs÷tsgo 0.59–0.94。読み込んだ
    文書数と診断は7 corporaで同一。
  - `tsconfig.bench-full.json` 3回：hono 146→144、zod 669→625、Playwright 480→495、TypeScript `src/compiler` 538→499、
    Next.js 1,037→1,031、Effect 765→760。診断は6 corporaで同一。出力fileはNext.js（ES2019）の178 files（`.js`と
    `.js.map`が89ずつ）で変わった（`??`の一時変数の`var`文）。tsgoの出力と違う`.js`は127→54 filesに減った。残りは
    代入の`=`の後の改行など（tsgoの`??`の変換は条件式に位置を付けない）。
  - 10回のA/B：Playwright `bench-full` 481→478、zod `bench-full` 623→628、hono `bench-full` 147→145、Next.js
    `bench-full` 1,007→1,034（min 971→958）。1 checkerの命令数branch÷main：zod `--noEmit` 1.00017、Playwright
    `bench-full` 1.00000、hono `bench-full` 1.00077、Next.js `bench-full` 0.99938。差はnoiseの範囲で、劣化無し。
- hosted：PR #673（head `b479dbb51`）、run 37236018822 — `plan` 31s、`rust` 9m54s、`conformance (TypeScript 7.1)` 20m21s、
  `gates` 11s。

## P3-5aw `??`の条件式の位置、JSXのruntime import、tsconfigのroot配列、`paths`の拡張子（2026-10-05）

P3-5avの後の4件（前の2件はREADMEのNext.jsの出力の差から）：
- **`??`の条件式の位置**：tsgoの`??`の変換は、作る条件式に位置もoriginalも付けない（nullishcoalescing.go:34-40）。
  printerは位置の無い右辺の前で改行しないので、`x =\n  a ?? b`は1行になる。tsc-rsはtsc 6.0の、条件式に元の範囲を
  付ける形で改行を残していた。
- **JSXのruntime import**：tsgoの`getSortedSpecifiers`は、runtime importの指定子をimportする名前の順に並べる
  （jsx.go:183-195）。tsc-rsは使われた順（`jsx`、`Fragment`、`jsxs`）だった。
- **tsconfigのroot配列**：tsgoの`convertConfigFileToObject`は、root配列の最初のobjectを変換し、その時はTS5092を出さない
  （tsoptions/tsconfigparsing.go:319-335）。optionの構文の位置はroot objectからしか探さない
  （`getTsConfigObjectLiteralExpression`）。tsc-rsはTS5092を出し、配列の中のobjectにも位置を付けていた（TS2688の
  related information）。`tsconfigMalformedNonObject`。
- **`paths`の拡張子**：tsgoは、拡張子を持つ`paths`の置き換え先をconfigの拡張子として扱い
  （`candidateEndingIsFromConfig`、module/resolver.go:1265-1283）、そこから見つけた宣言fileはTS拡張子を使った解決に
  ならない。tsc-rsは`"./some-path/index.ts"`から`index.d.ts`を見つけた時にTS5097を出していた。
  `pathsEntryReferencesDtsViaTsExtensionNoCrash`。
- 既存のtest 2件（root配列のTS5092）をtsgoの形に直した。tsgoはJSONの単引用符（TS1327）をparserで全体について出し、
  `extends`の循環で`{0}`の残ったTS18000を出す（どちらも以前からの差として残る）。
- READMEのcorpora（`tsconfig.bench-full.json`）でtsgoと違う`.js`：Next.js 54→2 files、Playwright 33→0 files。
  Next.jsの残りは`new (require(…) as T).X()`の括弧（tsgoは`new require(…).X()`と書き、意味が変わる）と、矢印関数の
  本体の後のcommentの重複（tsgo）。
- unit test：CLI（tsgoの出力にpin）で3件（`??`の条件式、JSXのruntime import、tsconfigと`paths`）。P3-5avのbuildでは
  3件ともtsgoと違っていた。
- conformance（release build、`205c4654b`（testを加えた`01eea9fcb`と同じcode）、`--workers 2`、528 s）：
  - 15,228構成、lane A 13,467（変化なし）。errors full 13,400→13,403：上の2構成と
    `intersectionConstructorReductionCrash`（今回は最後まで通った。負荷で結果の変わる構成で、ratchetの外）。
  - emit full 13,400→13,401（`intersectionConstructorReductionCrash`）。emit mismatch 37（変化なし）。下がった構成は無い。
- ratchet：0 regressions、2行追加（errors）。
- local：formatとworkspace全体のclippy。program・compiler・emitterのtest（37 targets、1,456件）。filter `nullish`・
  `Coalescing`・`jsx`・`tsx`・`tsconfig`・`onfig`で下がった構成は無かった。
- perf（README corpora、nice 20、main（P3-5avのbuild `fd39404ac`）対tsgo 7.1.0-dev）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 122→129、zod 521→515、Playwright 353→364、TypeScript
    `src/compiler` 332→326、Next.js 793→752、Effect 509→568（min 503→500）、VS Code 3,455→3,371。tsc-rs÷tsgo 0.60–0.92。
    読み込んだ文書数と診断は7 corporaで同一。
  - `tsconfig.bench-full.json` 3回：hono 143→147、zod 640→670、Playwright 493→587（min 483→499）、TypeScript
    `src/compiler` 536→514、Next.js 1,051→1,033、Effect 784→772。診断は6 corporaで同一。出力fileはPlaywright 33 files、
    Next.js 265 filesで変わり、どちらもtsgoの出力に近づいた（上の数）。
  - 10回のA/B：Playwright `bench-full` 497→495、zod `bench-full` 640→638、hono `bench-full` 147→148、Next.js
    `bench-full` 1,056→1,039。1 checkerの命令数branch÷main：zod `--noEmit` 0.99971、Playwright `bench-full` 1.00012、
    hono `bench-full` 1.00024、Next.js `bench-full` 0.99961。3回の差はnoiseで、劣化無し。
- hosted：PR #674（head `e3c293e19`）、run 37238181312 — `plan` 29s、`rust` 9m33s、`conformance (TypeScript 7.1)` 20m47s、
  `gates` 11s。

## P3-5ax tupleの`-?`、template literalの推論、`as const`、`{}`のunion、synthetic propertyのaccessibility（2026-10-05）

P3-5awの後、checkerの5件：
- **tupleの`-?`**：tsgoの`instantiateMappedTypeTemplate`は、`-?`で必須にするoptionalなtuple要素から、
  exactOptionalPropertyTypesではmissing型だけを除く（checker.go:23073-23074、`removeMissingOrUndefinedType`）。
  tsc-rsはtsc 6.0の、書かれた`undefined`も除く形だった。`stripMembersOptionality2(exactoptionalpropertytypes=true)`。
- **template literalの推論**：tsgoは、隣り合うplaceholderの間でcode pointを1つずつ取る（checker/relater.go:
  2462-2486）。surrogate pairは分けない。`templateLiteralInferenceSupplementarySplit`。
- **`as const`**：tsgoの`isMutableArrayLikeType`は`never`を除く（checker.go:23982-23986）。文脈の型が`never`でも
  `as const`のtupleはreadonlyのまま。`asConstReadonlyTupleInferenceThroughNestedGenericCall`。
- **`{}`のunion**：tsgoの`removeSubtypes`は、`emptyObjectType`と`unknownEmptyObjectType`を、symbolを持つ空の匿名型
  （書かれた`{}`）のために消さない（checker.go:26472-26474）。`unknown`の`v || {}`は非literalの`{}`になる。
  `implicitEmptyObjectType`。
- **synthetic propertyのaccessibility**：tsgoはunionとintersectionの各constituentの読みとset accessorのaccessibilityを
  別に数え（`CheckFlagsContainsWrite*`、checker.go:21851-21866）、unionはprivateやprotectedの読みのpropertyを作らず、
  書きは最も制限されたconstituentに合わせる（21901-21918）。synthetic propertyはvalue declarationより先にcheck flagsを
  読み、最も緩いaccessを取る（checker/utilities.go:761-776）。宣言したclassの無いprivateは含む型を名前に出す
  （checker.go:12035-12046）。`syntheticProtectedProperties`。
- unit test：CLI（tsgoの出力にpin）で3件（tupleとtemplate literal、`as const`と`{}`、accessibility）。P3-5awのbuildでは
  5つのprobeともtsgoと違っていた。
- conformance（release build、`b3cc300ec`、`--workers 2`、516 s）：
  - 15,228構成、lane A 13,467（変化なし）。errors full 13,403→13,407：上の5構成から
    `intersectionConstructorReductionCrash`（今回はmemoryの上限で終わらなかった。負荷で結果の変わる構成で、ratchetの外）を
    引いたもの。emit full 13,401→13,400（同じ構成）。下がった構成は無い。
  - その後のperfの2 commit（`c1836f54a`、`762c9fe36`）は結果を変えない読みの省略で、filter `rotected`・`rivate`・
    `ccessib`・`ixin`・`ntersection`・`nion`（868構成）で下がった構成は無かった。
  - `--checkers 4`の並列対照は、localの負荷の方針（full runはsliceごとに1回）により実行していない（途中で始めたものは
    止めた。途中のfull runも1回余分に流した）。
- ratchet：0 regressions、5行追加（errors）。
- local：formatと、checker・types・compilerのclippy。checker・types・compilerのtest（11 targets、2,082件）。workspace全体の
  testとclippyはhostedの`rust` job。
- perf（README corpora、nice 20、main（P3-5awのbuild `205c4654b`）対tsgo 7.1.0-dev、branchは`b3cc300ec`）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 131→133、zod 526→530、Playwright 362→361、TypeScript
    `src/compiler` 359→342、Next.js 799→758、Effect 536→562（min 520→499）、VS Code 3,511→3,470。tsc-rs÷tsgo 0.59–0.99。
    読み込んだ文書数と診断は7 corporaで同一。
  - `tsconfig.bench-full.json` 3回：hono 147→150、zod 628→630、Playwright 477→481、TypeScript `src/compiler` 570→501、
    Next.js 1,055→1,039、Effect 755→753。6 corporaとも出力fileと診断は同一。
  - 10回のA/B（`--noEmit`）：Effect 526→516、zod 519→518、VS Code 3,384→3,379、Next.js 766→766。
  - 1 checkerの命令数branch÷main（`--noEmit`）：zod 1.00625、Effect 1.00274、Next.js 1.00831、Playwright 1.00670。
    原因はsynthetic propertyのaccessibility（`9c5fe9c96`）で、それを除いたbuildはzod 1.0000。tsgoと同じくsynthetic
    propertyのcheck flagsをvalue declarationより先に読むためのlinksの読みで、transient symbolに限る2つのcommitの後は
    zod 1.0024、Next.js 1.0025。残りはtsgoの順序のための読みで、wall-clockの差はnoiseの範囲。
- hosted：PR #675（head `ee78fae5d`）、run 37241993974 — `plan` 30s、`rust` 10m10s、`conformance (TypeScript 7.1)` 20m14s、
  `gates` 11s。

## P3-5ay build info、harnessの出力順とunit、EOFと空リストのコメント、CommonJSのscript（2026-10-05）

P3-5axの後、emitとrunnerの5件：
- **build info**：tsgoの`compiler.Program.Emit`は、`incremental`と`composite`のProgramでもJavaScriptと宣言だけを書く。
  build infoを書くのはcommandのincremental programだけ（execute/incremental/program.go:243-273）。emitterは`incremental`、
  `composite`、`tsBuildInfoFile`を受け付け、build infoをまだ書けないcommandは、`incremental`か`composite`のProgramの
  emitを拒む（execute/tsc.go:245、`IsIncremental`）。`tsBuildInfoFile`だけではbuild infoは無い（`GetBuildInfoFileName`）
  ので、commandもtsgoと同じくemitする。`incrementalConfig`、`incrementalInvalid`、`incrementalTsBuildInfoFile`、
  `compositeWithNodeModulesSourceFile`、`declarationEmitWithComposite`、`declarationEmitToDeclarationDirWithCompositeOption`、
  `jsFileCompilationWithEnabledCompositeOption`、`optionsCompositeWithIncrementalFalse`、
  `optionsTsBuildInfoFileWithoutIncrementalAndComposite`、`nodeNextPackageSelfNameWithOutDirDeclDirComposite`。
  `tsBuildInfoFile`でcheckごと止まっていた`incrementalConcurrentSafeAliasFollowing`と`jsEmitIntersectionProperty`も
  比べられるようになった（errorsとemit）。
- **harnessの出力順**：harnessの`newCompilationResult`（harnessutil.go:746-834）は、各sourceの出力を`getOutputPath`の
  path（宣言でも`outDir`の下）でProgram順に取り、取られなかったfile（`outDir`と違う`declarationDir`の宣言、宣言map）を
  名前順で後ろに置く。runnerはemit順のままだった。`nodeNextPackageSelfNameWithOutDirDeclDirNestedDirs`、
  `nodeNextPackageSelfNameWithOutDirDeclDirCompositeNestedDirs`、`declarationMapCrossFileNodeReuse`のmap。
- **unitの分割**：Goのparserは各unitの内容を文字列に持つので、内容行の無いunitも空のfileになる
  （test_case_parser.go:199-216）。compiler runnerは最後のunitをcompileし、それ以外は同名のunitも含めて後から書く
  （compiler_runner.go:320-323）。tsc-rsはtscの、最後と同名のunitを除き、空のunitを書かない形だった。
  `augmentExportEquals2`（errorsとemit）。
- **EOFと空リストのコメント**：tsgoは、文の無いfileの残りのコメントを文リストの終わり（skipされたtokenの後）から読む
  （printer.go:5404-5417。位置0以外では、同じ行のコメントは前のtokenのもの）。空リストの中と末尾カンマの後のコメントは、
  そこで終わるcontainer（閉じていないリストの文）に任せる（printer.go:4765-4798、5371-5380、5604-5611）。
  `parserSkippedTokens6`、`parserSkippedTokens7`、`parser509630`。tokenの後のコメントの`containerEnd`の判定は、
  式の文脈を持つ経路（`emit_source_leading_token_with_context`）だけに入れ、他の経路は従来どおり。
- **CommonJSのscript**：tsgoのCommonJS変換はscriptをそのまま返す（commonjsmodule.go:228-233）ので、JSX runtimeの
  名前は書いたまま残る。tsc-rsはtscの、出力時の置換で`(0, _a.jsx)`にしていた。tsgoに無いAMDとUMDはtscの置換を残す。
  `commentsOnJSXExpressionsArePreserved`（`react-jsx`と`react-jsxdev`のCommonJS、`moduleDetection: legacy`）。
- unit test：CLI（tsgoの出力にpin）で4件、Program emit、runnerの出力順、harnessのunit分割で各1件。tscの置換に
  pinしていたemitterのunit testを1件、tsgoに合わせた。
- conformance（release build、`fa817ace1`、`--workers 2`、514 s）：
  - 15,228構成、lane A 13,467（変化なし）。errors full 13,407→13,410（`augmentExportEquals2`、
    `incrementalConcurrentSafeAliasFollowing`、`jsEmitIntersectionProperty`）。emit full 13,400→13,420、emitの不一致
    37→19、harness error 22→20。`.js.map`の不一致は47→34（ratchetの外）。下がった構成は無い。
    `intersectionConstructorReductionCrash`は今回もmemoryの上限で終わらなかった（負荷で結果の変わる構成で、ratchetの外）。
  - 途中はfilter（`ncremental`、`omposite`、`sBuildInfo`、`OutDirDecl`、`eclarationDir`、`eclarationMap`、`ootDir`、
    `utDir`、`ourceMap`、`omment`、`rrayLiteral`、`railingComma`、`kippedTokens`、`rrorRecovery`、`jsx`、`Jsx`、`tsx`、
    `ommonJS`、`ommonjs`と、空のunitや同名のunitを持つ各case）で確かめ、下がった構成は無かった。
  - `--checkers 4`の並列対照は、checkerを変えていないので実行していない。
- ratchet：0 regressions、20行（3行追加、17行のemitを`js`に）。
- local：formatと、emitter・compiler・harness・conformanceのclippy。emitter・compiler・harness・conformanceのtest
  （31 targets、926件。tscの置換にpinしていたunit test 1件を直した後、その1件を再実行）。workspace全体のtestとclippyは
  hostedの`rust` job。
- perf（README corpora、nice 20、main（P3-5axのbuild、`7c3a6c302`）対tsgo 7.1.0-dev、branchは`fa817ace1`）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 142→135、zod 523→522、Playwright 346→369（min 341→354）、
    TypeScript `src/compiler` 351→328、Next.js 807→782、Effect 500→483、VS Code 3,449→3,395。tsc-rs÷tsgo 0.62–0.93。
    読み込んだ文書数と診断は7 corporaで同一。
  - `tsconfig.bench-full.json` 3回：hono 147→145、zod 617→617、Playwright 501→474、TypeScript `src/compiler` 552→491、
    Next.js 1,029→989、Effect 754→747。6 corporaとも出力fileと診断は同一。
  - 10回のA/B（`--noEmit`）：Effect 514→501、zod 518→521、VS Code 3,365→3,353、Next.js 779→766。
  - 1 checkerの命令数branch÷main（`--noEmit`）：zod 0.99983、Effect 0.99972、Next.js 0.99968、Playwright 0.99967。
    Playwrightの3回の差はnoiseで、劣化無し。
- hosted：PR #676（head `1669f9b03`）、run 37246122418 — `plan` 30s、`rust` 9m49s、`conformance (TypeScript 7.1)` 17m52s、
  `gates` 13s。

## P3-5az source map（2026-10-05）

P3-5ayの後、`.js.map`の基準の不一致34構成（ratchetの外）の原因を、tsgoの`.sourcemap.txt`とmappingの復号で調べた。
tsc 6.0の`cloneNode`は位置を写さないが、tsgoの`Clone`は写す（ast.go:103-120）ので、tsgoで複製された名前はmapされる。
- **token**：tsgoの`emitToken`がmapするのは`{`と`}`だけ（`shouldEmitTokenSourceMaps`、printer.go:822-830）。tsc 6.0が
  `writeToken`でmapしていた`debugger`、case節の同じ行の`:`、`new.target`等のkeywordは、nodeのmapだけになる。
- **parameter property**：tsgoの代入文、`this.x`、代入は位置を持たず、名前の2つの複製だけが位置を持つ
  （runtimesyntax.go:784-807）。class fields変換がconstructorを書き直すのは、初期値をconstructorへ移すclassだけで
  （`transformConstructor`、classfields.go:2365-2377）、それ以外はTypeScript変換の代入文が残る。tsc-rsはparameter
  propertyだけのclassでも、propertyの範囲を付けた代入を作り直していた。
- **型を消したparameterと、static propertyのあるclass**：tsgoは名前の後ろとclassの後ろのmapを残す（typeeraser.go:
  228-249）。tsc 6.0はどちらにも`NoTrailingSourceMap`を付けていた（ES5のIIFE化はtsgoに無いので従来どおり）。
- **namespace**：`GetExternalModuleOrNamespaceExportName`は内側の名前に宣言名の複製を使い（printer/factory.go:
  561-586）、`N.M`や`N.C = C`の`M`、`C`がmapされる。参照の置換（`v`→`N.v`）の内側の名前はmapしない
  （runtimesyntax.go:931-945）。
- **CommonJS**：exportされたclassとfunctionの名前は`GetDeclarationName`（mapとcomment無し）で、`exports.x = x;`は
  comment rangeだけの文（commonjsmodule.go:502-588、940-968）。参照の`exports.x`と`import x = require()`の`x`は名前の
  複製でmapされる（commonjsmodule.go:777-821、2064-2079）。
- **宣言map**：tsgoはconstructorとmethodの宣言を`Update`で作るので、d.tsでも元の範囲にmapされる
  （declarations/transform.go:1074-1085、1140-1159）。tsc-rsはtscの、新しいnodeを作る形だった（範囲はsource map
  だけに渡し、commentは従来どおり）。
- **runner**：source mapのpreviewリンクは、mapのsourceをProgramの順（依存先が先）で探す（sourcemap_baseline.go:
  90-100）。`commonSourceDirectory`。
- unit test：CLI（tsgoの出力にpin）で1件（上の全てを含む5つのmap）。tsc 6.0.3の`new.target`のtoken mapに固定していた
  内部のinvariant testとそのfixtureは、挙動が変わったので外した。
- conformance（release build、`588754c50`、`--workers 2`、541 s）：15,228構成、lane A 13,467。`.js.map`の不一致は34→1
  （残りはAST深さで保留中の`binderBinaryExpressionStress`）。errorsとemitはP3-5ayと同じで、変わったのは負荷で結果の変わる
  `intersectionConstructorReductionCrash`（今回は終わりFull、ratchetの外）だけ。下がった構成は無い。途中はfilter
  （`ourceMap`、`ourcemap`、`eclarationMap`、`eclaration`、`xport`、`mport`、`ommonjs`、`lass`、`ecorator`、
  `amespace`、`arameter`、`odule`）で確かめ、下がった構成は無かった。`--checkers 4`の並列対照はcheckerを変えていない
  ので実行していない。
- ratchet：0 regressions。上がった行は無い（`.js.map`はratchetの外）。
- local：formatと、emitter・compiler・harness・conformanceのclippy。emitter・compiler・harness・conformanceのtest
  （926件。tsc 6.0.3のinvariant testを外した後、emitterのlib testを再実行して480件）。workspace全体のtestとclippyは
  hostedの`rust` job。
- perf（README corpora、nice 20、main（P3-5ayのbuild、`581a9ad12`と同じコード）対tsgo 7.1.0-dev、branchは`588754c50`）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 134→136、zod 525→526、Playwright 373→371、TypeScript
    `src/compiler` 356→326、Next.js 773→823（min 769→786）、Effect 525→477、VS Code 3,476→3,441。読み込んだ文書数と
    診断は7 corporaで同一。
  - `tsconfig.bench-full.json` 3回：hono 146→145、zod 628→633、Playwright 480→503、TypeScript `src/compiler` 536→514、
    Next.js 1,011→1,006、Effect 774→773。6 corporaとも診断は同一で、出力の違いはmap fileだけ。
  - 10回のA/B：`--noEmit` Effect 519→535（min 500→518）、zod 526→518、VS Code 3,383→3,383、Next.js 775→773。
    `bench-full` Playwright 466→466、Next.js 990→991。1 checkerの命令数branch÷main（`--noEmit`）：zod 1.00041、
    Effect 0.99972、Next.js 0.99988、Playwright 0.99997。3回の差はnoiseで、劣化無し。
  - bench-fullの出力をtsgoと比べた、tsgoと同じbyteのfileの数（main→branch）：js.mapはhono 161→187/187、zod
    363→470/475、Playwright 555→703/704、TypeScript `src/compiler` 4→51/78（JavaScript自体が59/78）、Next.js
    1,301→1,451/1,665、Effect 390→496/496。d.ts.mapはhono 162→187/187、zod 453→465/467、Playwright 402→700/703、
    TypeScript `src/compiler` 75→78/78、Next.js 1,495→1,657/1,665、Effect 427→470/496。JavaScriptと宣言は変わらない。
- hosted：PR #677（head `902111f18`）、run 37251690295 — `plan` 27s、`rust` 9m54s、`conformance (TypeScript 7.1)` 20m56s、
  `gates` 11s。

## P3-5ba const enumのinline化と、source mapの残り（2026-10-05）

P3-5azの後、README corporaの出力をtsgoと比べて見つけた差：
- **const enumのinline化**：tsgoは、module変換の後の最後の変換でconst enumの値を埋め込む（compiler/emitter.go:174-177、
  inliners/constenum.go:32-89）。値のliteralは位置もoriginalも持たず、印字の前から木にあるので、printerは合成された
  nodeとして並べる。参照の前の改行は保たれず、参照の前のコメントも書かれない。tsc-rsはtscの、印字時の置換
  （`substituteConstantValue`）で、printerが置換前のnodeで改行を決めていた。TypeScriptの`src/compiler`のJavaScriptは
  59/78→78/78でtsgoと同じになった。
- **区切りのコメント**：位置の無い子（埋め込んだ値）は後置コメントを書かないので、条件式の`:`などの区切りは、自分の
  前のコメントを自分で読む（`emitPunctuationNode`、printer.go:2901-2921）。tsc-rsは子が書いたものとして飛ばしていた。
- **source map**：CommonJSの`import * as x`は名前の複製で宣言し（commonjsmodule.go:722-760）、optional callの`this`
  引数は複製でmapする（optionalchain.go:198-204）。optional catchの下位変換は新しい`catch`節（位置なし）を作る
  （optionalcatch.go:24-30）。優先順位のための括弧はprinterが印字時に書くだけでmapしない（printer.go:3222-3226）が、
  object literalのconcise bodyの括弧は本体の範囲でmapする（printer.go:2669-2685）。
- **inline化の費用**：木の上の置換は、埋め込んだ参照の祖先を作り直す（tsgoの`VisitEachChild`と同じ）。全nodeを訪ねる
  素直な形では、TypeScriptの`src/compiler`のfull emit（1 checker）の命令数が2.1%増えた。そこで、ファイルごとに構文木の
  property/element accessを一度resolverに尋ね、値を持ちうるものとその祖先に印を付け、前の変換が残した構文木の部分木は
  印が無ければ訪ねない（変換はparse nodeの子を書き換えないので、残ったparse nodeは自分の部分木を持つ）。値を持つ
  accessの無いファイルは訪ねない。子が変わらないnodeはそのまま返し、作り直すnodeは元のtransform flagsを保つ（最後の
  変換の後で読むものは無い）。出力は変わらず（6 corporaの12,404 fileがbyte単位で同じ）、増分は1.0%になった。残りは
  作り直したnodeの生成とmetadata、printerがそれらのコメント範囲を引く費用で、全変換に共通のmetadataの持ち方に属する
  （下の候補）。
- unit test：CLI（tsgoの出力にpin）で2件（残した部分木、型を消して作り直した関数、constructorとclassの後ろへ移した
  初期値、element access、importしたconst enum、括弧の要るreceiver）。tscの形に固定していたemitterのtest 3件
  （変換の一覧、module変換の選択、optional catchのコメント）を、tsgoの形に直した。
- conformance（release build、`72bb65cc8`、`--workers 2`、530 s）：15,228構成、lane A 13,467、full 13,411、emit full
  13,421、`.js.map`の不一致1。P3-5azのreportと結果が全く同じ（conformanceにはこの形のcaseが無く、効くのは実際の
  project）。`dfb73ab61`での1回目（531 s）とは、負荷で結果の変わる`intersectionConstructorReductionCrash`（1回目は
  memory limit、ratchetの外）だけが違う。途中はfilter（`num`、`omment`、`onditional`、`ourceMap`、`atch`、`mport`、
  `ptionalChain`、`arenthes`）で確かめ、下がった構成は無かった。`--checkers 4`の並列対照はcheckerを変えていないので
  実行していない。
- ratchet：0 regressions。上がった行は無い。
- local：formatと、emitter・compilerのclippy（`dfb73ab61`ではconformanceも）。emitter・compiler・conformanceのtest
  （`dfb73ab61`で901件、tscの形に固定していた3件を直して再実行）、emitter・compilerのtest（`72bb65cc8`で884件）。
  workspace全体のtestとclippyはhostedの`rust` job。
- perf（README corpora、nice 20、main（P3-5azのbuild、`902111f18`と同じコード）対tsgo 7.1.0-dev、branchは
  `72bb65cc8`。どちらもconformanceと一緒のbuild）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 134→136、zod 548→526、Playwright 383→360、TypeScript
    `src/compiler` 369→337、Next.js 791→783、Effect 525→511、VS Code 3,503→3,459。読み込んだ文書数と診断は7 corporaで同一。
  - `tsconfig.bench-full.json` 3回：hono 152→147、zod 632→642、Playwright 479→492、TypeScript `src/compiler` 583→521
    （min 510→515）、Next.js 1,043→1,007、Effect 845→772。6 corporaとも診断は同一で、出力の違いはzod 2、TypeScript
    `src/compiler` 45、Next.js 200 file（下のtsgoとの比較の通り）。
  - 10回のA/B：`--noEmit` Effect 525→524、zod 526→523、VS Code 3,461→3,437、Next.js 787→785。`bench-full`
    TypeScript `src/compiler` 506→528（min 468→489、CPU 1,621→1,646 ms）、Next.js 1,092→1,085、Playwright 522→508、
    Effect 811→820。1 checkerの命令数branch÷main：`--noEmit` zod 0.99970、Effect 1.00033、Next.js 0.99978、Playwright
    0.99976、`bench-full` TypeScript `src/compiler` 1.00971（25.22G→25.47G）、Next.js 0.99855。TypeScript
    `src/compiler`のemitは、inline化の多い`checker.ts`（7,631箇所、作り直し27,523 node）が最も長い仕事なので、wallの
    増分が命令数より大きい。tsgoの構造に合わせた費用として記録し、上の候補で下げる（他のcorporaは劣化無し）。
  - bench-fullの出力をtsgoと比べた、tsgoと同じbyteのfileの数（main→branch）：JavaScriptはTypeScript `src/compiler`
    59→78/78（他は変わらず、Next.js 1,663/1,665）。js.mapはhono 187/187、zod 470→471/475、Playwright 703/704、
    TypeScript `src/compiler` 51→77/78、Next.js 1,451→1,631/1,665、Effect 496/496。宣言とd.ts.mapは変わらない。
- 残り：ES2018未満で`() => ({ ...x } as T)`のように、型を消した後のobject literalが別の式になるconcise bodyは、
  tsc-rsでは括弧が残る（tsgoは印字時に判断する）。
- 性能の候補（実装しない）：emit metadataを密な表にする（`set_original_node`の挿入とprinterの`comment_range_for_node`
  の検索が、作り直したnodeの数だけ増える）、`update_node`が元のpayloadを複製してから置き換える二重の複製。
- 次（P3-5bb）：concise bodyをblockにする変換（ES2021、class fields、decorator）がtsgoの`ConvertToFunctionBlock`の
  `statements.Loc = body.Loc`と、`VisitFunctionBody`の式の`EFNoComments`を欠く（`}`のmapと、本体の後ろのコメントが
  `}`の前に入る。Next.jsの`router.js`ではJavaScriptも違う）。class fieldsの`export default X;`はmapしない
  （`GetLocalName`）。parameterの既定値の`x === void 0`、private fieldの受け手、CommonJSの関数を持つexport変数の
  右辺は、名前の複製（位置あり）でmapする。型assertionを消した括弧の終わりのmap。
- hosted：PR #678（head `fa1d127f9`）、run 37258721214 — `plan` 28s、`rust` 8m35s、`conformance (TypeScript 7.1)` 20m57s、
  `gates` 12s。

## P3-5bb concise bodyのblock化、名前の複製の位置、印字時の括弧（2026-10-05）

P3-5baの後、README corporaの出力をtsgoと比べて残っていた差：
- **concise bodyのblock化**：下位変換（ES2021の一時変数、class fieldsの束縛、decoratorの一時変数）がconcise bodyを
  blockにするとき、tsgoの`VisitFunctionBody`は式にコメントを書かせず（`EFNoComments`）、`ConvertToFunctionBlock`は
  return文、その文のリスト、blockに本体の範囲を与える（printer/emitcontext.go:930-962）。blockの`}`は本体の後ろに
  mapされ、本体の後ろのコメントは`}`の前に書かれる（次の文も同じコメントを前置コメントとして書くので、tsgoでは2回
  出る）。1行のblockの文の後置コメントも、複数行のときと同じく囲みの終わりと比べる（return文の終わりはarrow関数の
  終わりと同じで、そのコメントはarrow関数が書く）。`using`の下位変換のblockは文の範囲を保つ（using.go:204-206）。
  Next.jsの`router.js`ではJavaScriptも変わる。
- **名前の複製の位置**：tsgoの`Clone`は位置を保つ（tscの`cloneNode`は保たない）。parameterの既定値の`x === void 0`と
  代入の名前（emitcontext.go:892-917）、private fieldの受け手（両方のhelper呼び出し、classfields.go:1269-1281）、
  関数を持つexport変数の`exports.x = x`の右辺（commonjsmodule.go:1067-1077）は名前にmapし、名前のコメントも書く。
- **mapしないもの**：`export { x }`のための`exports.x = x;`はcomment rangeだけの文（commonjsmodule.go:578-588）。
  class fieldsの`export default X;`は`GetLocalName`でmapしない（classfields.go:1978-1982）。private fieldのhelper
  引数の括弧と、printerが優先順位のために足す括弧は、印字時に書くだけでmapしない（printer.go:3222-3226）。object
  literalで始まるconcise bodyの括弧だけが、範囲を持つ括弧式になる（printer.go:2669-2685）。
- **`new`の呼び出し先**：tsgoの`emitNewExpression`は、呼び出し先そのものが呼び出しのとき（PartiallyEmittedExpression
  は飛ばす）だけ括弧を付け、それ以外はmemberの優先順位で書く。呼び出しの優先順位もmemberと同じ（printer.go:
  2592-2605、ast/precedence.go:274-284）。tscは呼び出し先の左端の呼び出しを見ていたので、`new (f() as T).C()`は
  `new (f().C)()`だったが、tsgoは`new f().C()`と書く。実行時の意味が変わる形だが、参照実装の出力に合わせた（Next.js
  の`webpack-config.ts`に2箇所）。
- unit test：CLI（tsgoの出力にpin）で2件。
- 結果：README corporaの`bench-full`のJavaScriptと`.js.map`は、6 corporaとも全fileがtsgoとbyte単位で同じになった
  （hono 187、zod 475、Playwright 704、TypeScript `src/compiler` 78、Next.js 1,665、Effect 496）。残る差は宣言とその
  mapだけ（unionとpropertyの順序など）。
- conformance（release build、`c8aef18bb`、`--workers 2`、513 s）：15,228構成、lane A 13,467、full 13,410、emit full
  13,420、`.js.map`の不一致1。P3-5baの最後のreportと比べて変わったのは、負荷で結果の変わる
  `intersectionConstructorReductionCrash`（今回はmemory limit、ratchetの外）だけ（conformanceにはこれらの形のcaseが
  無く、効くのは実際のproject）。途中はfilter（`ourceMap`、`ptionalChain`、`rrowFunction`、`omment`、`arenthes`、
  `rivateName`、`lassField`、`xport`、`efault`、`arameter`、`ecorator`、`sing`、`ewExpression`、`odule`）で確かめ、
  上がった構成も下がった構成も無かった。`--checkers 4`の並列対照はcheckerを変えていないので実行していない。
- ratchet：0 regressions。上がった行は無い。
- local：formatと、emitter・compiler・conformanceのclippy、test（`c8aef18bb`で904件）。workspace全体のtestとclippyは
  hostedの`rust` job。
- perf（README corpora、nice 20、main（P3-5baのbuild、`fa1d127f9`と同じコード）対tsgo 7.1.0-dev、branchは
  `c8aef18bb`。どちらもconformanceと一緒のbuild）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 138→143、zod 531→531、Playwright 385→362、TypeScript
    `src/compiler` 371→347、Next.js 807→785、Effect 548→530、VS Code 3,572→3,509。読み込んだ文書数と診断は7 corporaで
    同一。
  - `tsconfig.bench-full.json` 3回：hono 157→154、zod 623→627、Playwright 484→477、TypeScript `src/compiler` 562→534、
    Next.js 1,046→1,019、Effect 770→761。6 corporaとも診断は同一で、出力の違いはzod 4、Playwright 1、TypeScript
    `src/compiler` 1、Next.js 36 file（全てtsgoと同じになった側）。
  - 10回のA/B：`--noEmit` Effect 522→507、zod 517→520、VS Code 3,385→3,392、Next.js 781→773。`bench-full`
    TypeScript `src/compiler` 511→513、Next.js 1,003→1,008、Playwright 473→476、Effect 754→755。1 checkerの命令数
    branch÷main：`--noEmit` zod 1.00050、Effect 1.00042、Next.js 0.99964、Playwright 0.99986、`bench-full`
    TypeScript `src/compiler` 0.99994、Next.js 0.99979。劣化無し。
  - bench-fullの出力をtsgoと比べた、tsgoと同じbyteのfileの数（main→branch）：JavaScriptはNext.js 1,663→1,665/1,665
    （他は既に全て同じ）。js.mapはzod 471→475/475、Playwright 703→704/704、TypeScript `src/compiler` 77→78/78、
    Next.js 1,631→1,665/1,665（hono 187/187、Effect 496/496は変わらず）。宣言とd.ts.mapは変わらない。
- 次：宣言のmap。別のfileから再利用した型のnode（`?`の記号など）が、そのfileの位置でmapされる（tsgoの
  `setTextRange`は同じfileの位置だけを写す、nodebuilderimpl.go:1425-1460）。型parameterの制約の`Rpc.Any`などは、
  tsgoでは部分ごとにmapされる。
- hosted：PR #679（head `4300dde89`）、run 37262470561 — `plan` 36s、`rust` 7m37s、`conformance (TypeScript 7.1)` 20m2s、
  `gates` 12s。

## P3-5bc 宣言の型nodeの再利用とmap、symbolの鎖の選び方（2026-10-05）

P3-5bbの後、README corporaの宣言（`.d.ts`と`.d.ts.map`）をtsgoと比べて残っていた差。tsc 6.0の動作を再現していた
箇所を、tsgoのnode builderに合わせた：
- **再利用する型のtoken**：tsgoの既存nodeのvisitorにはtoken用のhookが無く（ast/visitor.go:225-230、
  checker/nodecopy.go:827-900）、`?`、`...`、`readonly`、`*`のtokenも他の子と同じく複製され、位置は囲みのfileの
  nodeのときだけ写る。tsc 6.0は元のtokenをそのまま使っていたので、別のfileから再利用したmemberの`?`が、その
  fileのoffsetをいまのfileのline mapでmapしていた。
- **直列化した型のcache**：tsgoはemit resolverへの要求ごとにnode builderを作る（checker/emitresolver.go:958-1277）
  ので、`NodeBuilderLinks.serializedTypes`は1つの要求の中だけで生きる。linksはいまの囲みの宣言のもので、
  `enterNewScope`の作るfake scopeのBlockごとに別になる（checker/nodebuilderscopes.go:92-160、nodebuilderimpl.go:
  3229-3273）。hitは`DeepCloneNode(cachedResult.node)`、つまり位置の無い複製を返す（ast/deepclone.go:6-73。listの
  末尾のcommaは残す）。tsc 6.0はcheckerのnode linksにemitの間ずっとcacheを持ち、範囲ごと複製していたので、2回目
  以降の型もmapされ、layoutもsourceから取っていた。cacheを要求のcontextに移し、keyにfake scopeのBlockの識別を
  入れた。
- **fake scopeの検索は意味が合うときだけ**：`getSymbol`は、tableのsymbolのflagsが求める意味を持つ（またはaliasの
  先が持つ）ときだけそのsymbolを返す。fake scopeの重ねは名前だけで答えていたので、parameterの`Model`や型
  parameterの`Rpc`が`Model.Any`や`Rpc.Any`のnamespaceを隠し、参照は再利用されずにsymbolから作り直されていた。
- **builderが作ったnodeは、範囲を付ける前に複製する**：`setTextRange`は、nodeの最も元のnodeが囲みのfileに属さない
  限りnodeを複製し、`GetSourceFileOfNode`はparse treeの親をたどってだけfileを見つける
  （checker/nodebuilderimpl.go:1434-1460）。型parameterの共有される名前のようにbuilderが作ったnodeにはfileが無く、
  複製されるので、共有されるnodeは範囲を持たない。tsc-rsはtargetのsourceのnodeを全てそのfileのものと扱い、
  使用箇所の範囲を共有の名前に書いていたので、宣言の側でもその位置にmapされていた。
- **mapperの下での制約と型参照の再利用**：`typeToTypeNodeHelperWithPossibleReusableTypeNode`は、nodeの型をcontextの
  mapperの下で比べ、stradaの`canReuseTypeNode`の関門無しに`tryReuseExistingNodeHelper`で再利用する
  （checker/nodebuilderimpl.go:1669-1681）。`tryVisitTypeReference`が断るのは、constの参照、mapperが置き換える型
  parameter、textから型の分からないJavaScriptの参照（checker/nodecopy.go:416-435）。mapperで型の変わる参照も
  再利用され、visitorが中の型parameterだけを置き換える。
- **symbolの鎖と、鎖の中の名前**：`trySymbolTable`は候補の鎖を全て集め、最も短いもの、同じ長さなら
  `compareSymbols`で最初のものを取る（checker/symbolaccessibility.go:535-609）。tsc 6.0はtableの順で最初の候補を
  取っていたので、`import * as ns`が`import { T }`より前にあると型は`ns.T`になっていた。
  `createAccessFromSymbolChain`は、親がその名前でexportしていれば鎖のsymbol自身の名前を、そうでなければ一致する
  exportのうち`compareSymbols`で最初のものを使う（checker/nodebuilderimpl.go:770-793）。tsc 6.0はtableの順で最初の
  ものを取り、`export default f`の後で`f`をre-exportするmoduleの`f`を`default`と呼んでいた。診断に出る名前も
  同じ規則で変わる（`export { N as M }; export { N };`のnamespaceは`M`ではなく`N`。tsgoで4通りを確かめ、6.0の
  順序をpinしていたcheckerのunit testを直した）。
- **別のfileのlistの位置と末尾のcomma**：既存nodeのvisitorの`VisitNodes` hookは、別のfileのnodeのlistを位置無しで
  写し（checker/nodecopy.go:878-888）、`NodeList.HasTrailingComma`はlistの終わりと最後のnodeの終わりを比べる
  （ast/ast.go:139-145）ので、そのlistに末尾のcommaは付かない。fileは、要素の最も元のnodeのもの。
- **async arrowのconcise body**（Vue.jsのe2e testで見つかった）：`transformAsyncFunctionBodyWorker`は、return文、
  その文のlist、blockにconcise bodyの範囲を与える（transformers/estransforms/async.go:876-893）ので、generatorの
  `}`は本体の次のtokenにmapされる。
- unit test：CLI（tsgoの出力にpin）で2件（宣言の名前・再利用・map、async arrowのmap）、位置の無い複製の共有で
  emitterに1件。checkerのunit test 1件をtsgoの出力に直した。
- **Vue.jsをperfの確認に加えた**（`vuejs/core` 3.5.43、`4ab865a`、`pnpm install --frozen-lockfile --ignore-scripts`。
  rootの`isolatedDeclarations`のため、`--noEmit`の構成も`declaration: true`のまま）。README corporaと同じく
  `tsconfig.bench-noemit.json`と`tsconfig.bench-full.json`で測る。
- 結果：`bench-full`の出力は、hono、zod、Playwright、TypeScript `src/compiler`、Next.js、Vue.jsの6 corporaで全fileが
  tsgoとbyte単位で同じになった。tsgoと同じbyteのfileの数（main→branch）：zodの宣言465→467/467とd.ts.map
  465→467/467、Playwrightの宣言701→703/703とd.ts.map 700→703/703、Next.jsの宣言1,664→1,665/1,665とd.ts.map
  1,657→1,665/1,665、Vue.jsのjs.map 436→440/440（宣言、d.ts.map、JavaScriptは440/440）、Effectの宣言479→484/496と
  d.ts.map 470→486/496。hono 187とTypeScript `src/compiler` 78は元から全て同じ。
- **tsgoの出力は実行ごとに変わる**：Effectの`bench-full`をtsgoで6回emitすると、`ai/McpSchema.d.ts`とそのmap、
  `ai/internal/mcpProtocol/`の`v2024_11_05`、`v2025_03_26`、`v2025_06_18`、`v2025_11_25`の宣言が回によって違う
  （unionの並び）。`getInferTypeParameters`がconditional typeの`locals`（Goのmap）をそのまま走査する
  （checker/checker.go:24259-24267）ので、`infer`の型parameterの順序が実行ごとに変わり、type aliasの中の遅延型参照
  （`Rpc.AddMiddleware`のtrue側の`Rpc<…>`）のinstantiationを比べるmapperの並び（checker/utilities.go:716-755）が
  変わる。tsc-rsは宣言の順で、この6 fileはどれも6回のうち3〜4回のtsgoの出力と同じbyteになる。上のEffectの数は
  1回のtsgoの出力との比較で、回によって宣言479〜484、d.ts.map 485〜486になる。
- Effectで、6回とも同じtsgoの出力と違うのは宣言12、d.ts.map 10：型parameterの名前（`ExecutionPlan`、`Request`、
  `Stream`、`HttpApiEndpoint`、`internal/doNotation`、`internal/effect`、`internal/schedule`、`AsyncResult`、
  `VariantSchema`、`Activity`、`schema/Model`）と、匿名のobject型のunionの順序（`mcpProtocol/v2026_07_28`。型の
  作成順で決まり、tsc-rsでもcheckerの数で変わる）。
- conformance（release build、`e068ff1c0`、`--workers 2`、535 s）：15,228構成、lane A 13,467、full 13,411、emit full
  13,423、emitの不一致17、`.js.map`の不一致1。P3-5bbの最後のreportと比べて変わったのは、emitがFullに上がった
  `declarationsWithRecursiveInternalTypesProduceUniqueTypeParams`と`defaultDeclarationEmitShadowedNamedCorrectly`、
  負荷で結果の変わる`intersectionConstructorReductionCrash`（今回は完走、ratchetの外）、tierは同じで出力がtsgoに
  1行近づいた`declarationEmitNameConflicts`（`typeof M.c.g`。残りは`typeof import("./declarationEmit_nameConflicts_1")`
  の1行）。途中はfilter（`eclaration`、`sDeclaration`、`ypeParameter`、`onditional`、`apped`、`eneric`、`sdoc`、
  `nfer`、`mport`、`xport`、`odule`、`amespace`、`lias`）で確かめ、下がった構成は無かった。
- full runの後の変更は、checkerのunit testとCLIのfixtureのlint（test fileだけ）と、下のperfの修正（`fdebe320e`、
  emitterの`deep_clone_node_without_positions`）。修正の後は12のfilter（`eclaration`、`ypeParameter`、`eneric`、
  `nfer`、`apped`、`onditional`、`lias`、`mport`、`xport`、`odule`、`amespace`、`sdoc`。5,073 case、6,346構成）を
  full runのreportと行ごとに比べ、tierも、診断とemitのdigestも全て同じだった。修正後のfull runはhostedの
  `conformance (TypeScript 7.1)` job。
- `--checkers 4`の並列対照：`--filter eclaration`（1,768 case、2,426構成）を1 checkerの同じfilterと
  `scripts/conformance_ts71_compare.py`で比べ、2,424構成が同じ。違う2構成は記録済みのpartition依存
  （`declarationEmitAugmentationUsesCorrectSourceFile`、`declarationEmitComputedPropertyNameSymbol2`の
  "late visibility alias belongs to another source"、[conformance-ts71](../conformance-ts71/README.md)）。全caseの
  対照はlocalの負荷の方針（full runはsliceごとに1回）により実行していない。
- ratchet：0 regressions。上の2行のemitを`none`から`js`に上げた。
- local：formatと、checker・emitter・compiler・conformanceのclippy、test（`fdebe320e`で2,704件）。workspace全体の
  testとclippyはhostedの`rust` job。
- perf（README corporaとVue.js、nice 20、main（P3-5bbのbuild、`c8aef18bb`と同じコード）対tsgo 7.1.0-dev、branchは
  `fdebe320e`。どちらもconformanceと一緒のbuild）：
  - **最初の計測で劣化が出て、直した**：`e068ff1c0`ではEffectの`bench-full`が757→865 ms、peak 1,152→1,525 MB
    （1 checkerで51.9→53.1 G命令、835→1,179 MB。`ai/internal/mcpSchema/v2026_07_28.ts`のemitだけで73→218 ms）。
    cacheのhitが返す位置無しの複製が、複製したnodeごとにnodeとemit metadataを割り当てていた（380,512 node）。
    位置を持つnodeもlistも無い部分木は、その複製と同じbyteを印字し何もmapしないので、複製せずに共有し、位置を
    持つnodeとそこへ至るnodeだけを複製する（根は必ず新しいnode。Effectで2,457 node）。出力は7 corporaとも
    `e068ff1c0`と同じで、1 checkerのEffectは51.8 G命令、837 MBに戻った。
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 141→126、zod 537→525、Playwright 377→384、TypeScript
    `src/compiler` 350→331、Next.js 812→775、Effect 509→513、Vue.js 350→344、VS Code 3,561→3,522。読み込んだ文書数と
    診断は8 corporaで同一。peak（MB）はhono 318→308、zod 1,309→1,297、Playwright 810→807、TypeScript
    `src/compiler` 288→289、Next.js 1,351→1,361、Effect 1,041→1,034、Vue.js 614→610、VS Code 5,456→5,443。
  - `tsconfig.bench-full.json` 3回：hono 145→148、zod 631→628、Playwright 485→471、TypeScript `src/compiler` 549→527、
    Next.js 1,045→1,025、Effect 749→764、Vue.js 408→407。peak（MB）はhono 356→355、zod 1,520→1,538、Playwright
    887→895、TypeScript `src/compiler` 474→473、Next.js 1,476→1,474、Effect 1,167→1,161、Vue.js 638→638。7 corpora
    とも診断は同一で、出力の違いはzod 4、Playwright 5、Next.js 9、Effect 25、Vue.js 4 file（全てtsgoと同じに
    なった側）。
  - 10回のA/B：`--noEmit` Effect 524→508、zod 525→528、VS Code 3,411→3,419、Next.js 776→774、Vue.js 353→344。
    `bench-full` TypeScript `src/compiler` 521→504、Next.js 1,011→1,010、Playwright 486→481、Effect 766→761（peak
    1,166→1,161 MB）、Vue.js 415→423（最小値は392→395）。1 checkerの命令数branch÷main：`--noEmit` zod 1.00017、
    Effect 0.99989、Next.js 0.99982、Playwright 1.00003、Vue.js 0.99997、`bench-full` TypeScript `src/compiler`
    0.99994、Next.js 0.99576、Vue.js 1.00020、Effect 0.99827。劣化無し。
  - tsgo（同じ計測の3回のmedian、ms／peak MB）：`--noEmit` hono 154／331、zod 861／1,770、Playwright 517／986、
    TypeScript `src/compiler` 359／393、Next.js 1,228／1,612、Effect 752／1,213、Vue.js 498／720、VS Code
    4,777／6,756。`bench-full` hono 198／370、zod 987／1,909、Playwright 692／1,315、TypeScript `src/compiler`
    628／658、Next.js 2,033（最小1,697）／2,057、Effect 1,085／1,785、Vue.js 585／859。
- 次：
  - 型parameterの名前のscope（Effectの残りの11 file）。既存nodeのvisitorのscopeの後始末
    （`SyntacticScopeCleanup`）が、tsc 6.0の短絡する`forEach`（最初の1件だけ消し、最初の1件だけ戻す）を再現して
    いる。tsgoは足した名前を全て消し、上書きした名前を全て戻す（checker/nodebuilderscopes.go:142-152）ので、
    兄弟のconditional typeの`infer _E`や、前のsignatureの`<O, E, R>`が後ろで`_E_1`、`E_1`にならない。mapped typeの
    型parameterの宣言は、tsgoではscopeに入ってから名前を付ける（checker/nodebuilderimpl.go:1583-1584）ので、
    兄弟のmapped typeの`K`が`K_1`にならない。
  - `declarationEmitNameConflicts`の残りの1行（`export import d = im`を通らず、moduleを`import("…")`で書く）。
  - Vue.jsの診断：tsgoは`packages/compiler-sfc/src/style/pluginScoped.ts(107,3)`のTS5115（`'Container', 'Diff'`の
    instantiationが深さ100に達する）、それに続くTS7006を2件、`packages/runtime-dom/src/directives/vModel.ts(446,37)`
    のTS2345を出すが、tsc-rsは出さない。
  - optionのerror（TS5069など）があるとき、tsgoは0.10 sで終わるが、tsc-rsはprogram全体をcheckしてから同じ1件を
    出す（Vue.jsで0.35 s、597 MB）。
- hosted：PR #680（head `c0213e6c2`）、run 37274156113 — `plan` 29s、`rust` 10m9s、`conformance (TypeScript 7.1)` 15m51s、
  `gates` 14s。

## P3-5bd 型parameterの名前のscope、鎖の親の順序、複製の合成コメント、JSXの子の行（2026-10-05）

P3-5bcの「次」のうち宣言の名前に関わるものと、conformanceのemitに残っていた不一致のうち原因が近いもの：
- **scopeが終わると、取った名前を全て返す**：`enterNewScope`の後始末は、再利用したfake scopeに足した名前を全て
  消し、上書きした名前を全て戻す（checker/nodebuilderscopes.go:142-152）。自分で作ったfake scopeは、囲みの宣言を
  戻すと一緒に無くなる。tsc 6.0は`Map.delete`と`Map.set`を短絡する`forEach`に渡していて最初の1件だけを戻し、
  既存nodeのvisitorのscopeの後始末（`SyntacticScopeCleanup`）がそれを再現していた。再利用した注釈の中で、兄弟の
  conditional typeの`infer _E`が`_E_1`に、`<O, E, R>(…)`を持つparameterの後のgenericな戻り値の型が
  `<E_1, R_1>`に、返されるtype literalの2つ目のmemberが`<R, O_1, E_1, …>`になっていた。後始末はlocalsをscopeに
  入る前の状態に戻すだけにした。再利用するmapped typeのscopeは、そのmapped type自身の型parameterを取る
  （checker/nodecopy.go:842-846。該当の分岐が、scopeを開かないinfer型を見ていた）。
- **mapped typeの型parameterはscopeの中で名付ける**：`createMappedTypeNodeFromType`は、mapped typeの型parameterで
  scopeに入ってから、その宣言、name type、templateを作る（checker/nodebuilderimpl.go:1582-1593）。scopeが終われば
  名前は返るので、兄弟のmapped typeはまた`K`と書く。tsc 6.0はscopeに入る前に名付けていた（`K_1`）。
- **鎖の親の順序**：`getSymbolChain`は鎖の根の親を`sortByBestName`で並べる（checker/nodebuilderimpl.go:1105-1107、
  1153-1170）。module同士はspecifierで、それ以外の組は`compareSymbols`で比べる。tsc 6.0は両方にspecifierが無い
  限り0を返し、実のcontainerが先に残っていた。targetをexportしていて、aliasのcontainerより前に宣言されたmoduleが
  先に試されるので、scopeに隠されたaliasはmodule経由で書かれる（`typeof import("./f1")`、
  `typeof import("./f2").g`。6.0は`typeof M.d`）。importしたmoduleがexportしていないtargetはcontainer経由のまま
  （`typeof M.k`）。比較は全順序でないので、`slices.SortStableFunc`が20件までに使う挿入sortで並べる。
- **cacheの複製は合成コメントを持たない**：複製は元のnodeのemit nodeを`emitNode.copyFrom`で受け取り、これはflagsや
  rangeは写すが合成コメントは写さない（printer/emitcontext.go:562-574）。cacheのhitが返す複製
  （`DeepCloneNode`）は、省略のplaceholderをただの`any`と書く：
  `getTags(c: { tags(c: /*elided*/ any): /*elided*/ any; }): { tags(c: any): any; };`。tsc 6.0はコメントも複製に
  mergeしていた。位置無しの複製は、複製するnodeから合成コメントを落とし、合成コメントを持つ部分木は共有しない。
- **合成のscopeでも、exportされた関数は名前で見つかる**：`lookupSymbolChainWorker`はそのまま`getSymbolChain`に進む
  （checker/nodebuilderimpl.go:1065-1078）。expandoの関数のmemberを書くnamespaceからは、fileのexportされた関数も
  localな関数やclassと同じく名前で届く。emitの経路は、合成の囲みの宣言の下でmoduleを親に持つ関数を全てmodule付きで
  返していた（tsc 6.0の出力、`typeof import("./a").Vec2`。tsgoは`typeof Vec2`）。
- **JSXの子の改行は、子ごとの印**：JSXのtransformは、子が2つ以上あるとき子の1つ1つに「新しい行で始める」印を
  付け（transformers/jsxtransforms/jsx.go:709-713、750-754）、listは印を持つ引数だけを新しい行に書く。CommonJSの
  transformは、importした名前を、名前のrangeだけを受け取る新しいaccessに置き換え、exportしたlocalを`exports.`と
  名前の複製（emit flagsを保つ）に置き換える（transformers/moduletransforms/commonjsmodule.go:2064-2124）：
  `(0, r.dom)(Box, { x: 1 }, component_1.tree, component_1.tree);`、
  `(0, r.dom)(Box, { x: 1 }, exports.⏎local, exports.⏎local);`。tscは呼び出しに印を付け、名前は印字のときに
  置き換えていたので、子は全て新しい行で始まっていた。印を子のものにし、呼び出しの引数のlistは引数ごとに印を読み
  （先に印字時の置き換えを尋ねる）、moduleの置き換えはaccessに印を残さず`exports`のmemberの名前に移し、property
  accessは印を持つ名前の前（dotの後）で改行する（`getLinesBetweenNodes`）。
- unit test：CLI（tsgoの出力にpin）で5件（scopeをまたぐ型parameterの名前、module経由のaliasの鎖、複製の省略
  placeholder、expandoのmemberの型、JSXの子の行）。emitterの位置無しの複製のtestに合成コメントの規則を足した。
- 結果：Effectの`bench-full`は、宣言495/496、d.ts.map 496/496がtsgoの6回の出力のどれかと同じbyteになった
  （P3-5bcは484と486）。残るのは`ai/internal/mcpProtocol/v2026_07_28.d.ts`（匿名のobject型のunionの順序）だけ。
  他の6 corporaは全fileがtsgoと同じまま。
- conformance（release build、`71feb95f6`、`--workers 2`、539 s）：15,228構成、lane A 13,467、full 13,411、emit full
  13,431、emitの不一致9、`.js.map`の不一致1。P3-5bcの最後のreportと比べて変わったのは、emitがFullに上がった8構成
  だけ：`declarationEmitHigherOrderRetainedGenerics`、`declarationEmitMappedTypeDistributivityPreservesConstraints`
  （名前のscope）、`declarationEmitNameConflicts`、`es5ExportEqualsDts`（target=es2015、鎖の親の順序）、
  `emitClassExpressionInDeclarationFile`、`noImplicitThisBigThis`（合成コメント）、
  `jsDeclarationsFunctionLikeClasses2`（target=es2015、expandoのmember）、`inlineJsxFactoryDeclarationsLocalTypes`
  （JSXの子の行）。他の構成はtierも、診断とemitのdigestも同じ。途中は18のfilter（`sx`、`eclaration`、`xpando`、
  `unction`、`mport`、`xport`、`odule`、`lias`、`amespace`、`ypeParameter`、`eneric`、`nfer`、`apped`、
  `onditional`、`sdoc`、`ommonjs`、`ropert`、`all`）をP3-5bcのreportと行ごとに比べた。
- `--checkers 4`の並列対照：`--filter eclaration`（1,768 case、2,426構成）を1 checkerの同じfilterと比べ、2,424構成が
  同じ。違う2構成は記録済みのpartition依存。全caseの対照はlocalの負荷の方針により実行していない。
- ratchet：0 regressions。上の8行のemitを`none`から`js`に上げた。
- local：formatと、checker・emitter・compiler・conformanceのclippy、test（`624a9c055`で2,709件）。workspace全体の
  testとclippyはhostedの`rust` job。
- perf（README corporaとVue.js、nice 20、main（P3-5bcのbuild、`fdebe320e`）対tsgo 7.1.0-dev、branchは
  `71feb95f6`。どちらもconformanceと一緒のbuild）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 142→158、zod 571→570、Playwright 409→402、TypeScript
    `src/compiler` 355→358、Next.js 863→807、Effect 571→530、Vue.js 400→377、VS Code 3,785→3,723。読み込んだ文書数と
    診断は8 corporaで同一。honoは1 checkerの命令数で4.321→4.319 G（3回のmedian）で、差は計測の揺れ。
  - `tsconfig.bench-full.json` 3回：hono 154→153、zod 736→713、Playwright 536→575（最小値は534→535）、TypeScript
    `src/compiler` 564→548、Next.js 1,162→1,128、Effect 831→845、Vue.js 461→486（最小値は453→460）。7 corporaとも
    診断は同一で、出力の違いはEffectの21 fileだけ（全てtsgoと同じになった側）。
  - 10回のA/B：`--noEmit` Effect 561→549、zod 569→573、VS Code 3,714→3,697、Next.js 842→830、Vue.js 388→382。
    `bench-full` TypeScript `src/compiler` 534→540、Next.js 1,099→1,098、Playwright 538→536、Effect 836→837、
    Vue.js 460→447。peakは全て±1%以内（Effectの`bench-full` 1,164→1,170 MB）。1 checkerの命令数branch÷main：
    `--noEmit` zod 1.00049、Effect 0.99947、Next.js 0.99983、Playwright 1.00018、Vue.js 0.99994、hono 0.99954、
    `bench-full` TypeScript `src/compiler` 0.99987、Next.js 1.00034、Vue.js 1.00011、Effect 1.00066、hono
    1.00032。劣化無し。
  - tsgo（同じ計測の3回のmedian、ms／peak MB）：`--noEmit` hono 191／324、zod 902／1,809、Playwright 585／1,016、
    TypeScript `src/compiler` 374／386、Next.js 1,334／1,684、Effect 833／1,214、Vue.js 519／721、VS Code
    5,034／6,758。`bench-full` hono 226／381、zod 1,088／2,027、Playwright 768／1,308、TypeScript `src/compiler`
    661／672、Next.js 1,772／2,041、Effect 1,182／1,806、Vue.js 659／860。（この回は計測機の負荷がP3-5bcの回より
    高く、3者とも1割ほど遅い。）
- 次：
  - **tsgoは基底型を処理する前にmemberを公開しない**：`resolveObjectTypeMembers`は、基底型のmemberを足し終えてから
    `setStructuredTypeMembers`を呼ぶ（checker/checker.go:19446-19493）。tsc 6.0（とtsc-rs）は基底型の前に自分の
    memberだけを一度公開するので、基底型の型引数のinstantiationが解決中の型のmemberを要るとき、6.0は途中の表で
    終わり、tsgoは同じ解決に再び入って深さ100でTS5115を出す。Vue.jsの
    `packages/compiler-sfc/src/style/pluginScoped.ts(107,3)`のTS5115（と続くTS7006の2件）は14行に縮められ
    （`type Diff<T, U> = T extends U ? never : T`、`interface _Selector<S> extends Container<string, Diff<Node, S>>`、
    `type Selector = _Selector<Selector>`）、conformanceの`keyofGenericExtendingClassDoubleLayer`のTS5115
    （`'Model', 'ModelAttributes', 'Exclude'`）も同じ形。
  - Vue.jsの`packages/runtime-dom/src/directives/vModel.ts(446,37)`のTS2345（unionの呼び出しで
    `DirectiveBinding<any, string, any>`が`DirectiveBinding<any, "number", any>`に代入できない）。
  - JavaScriptの`@type`がgenericな関数型のとき、関数宣言の型parameter（`ensureTypeParams`の`FullSignature`、
    `CreateTypeParametersOfSignatureDeclaration`、transformers/declarations/transform.go:2358-2390。
    `typeTagOnFunctionReferencesGeneric`）。
  - 匿名のmapped typeのunionの順序（`comparisonAnonymousMappedTypes`、`comparisonReverseMappedTypes`）、構文errorの
    回復でのコメントの二重出力（`objectTypesWithOptionalProperties2`）。
- hosted：PR #681（head `e6301e6b1`）、run 37282361616 — `plan` 28s、`rust` 9m55s、`conformance (TypeScript 7.1)` 13m45s、
  `gates` 14s。

## P3-5be 7.1のcheckerの規則：基底型の後のmember、交差型の縮約、比較の深さ、varianceの計測（2026-10-05）

P3-5bdの「次」のVue.jsの診断2件から始めて、tsgoのcheckerがtsc 6.0と違う規則を、corporaの診断の差から順に
見つけた。差の場所は、instantiationのstackをtsgo（同じcommitのsourceの写しに計測を足してbuildしたもの）と
tsc-rsの両方で出力し、最初に分かれる所で特定した：
- **memberは基底型の後で公開する**：`resolveObjectTypeMembers`は、基底型のmemberを足し終えてから一度だけ
  `setStructuredTypeMembers`を呼ぶ（checker/checker.go:19446-19493）。tsc 6.0は基底型を見る前に自分のmemberだけを
  公開していたので、基底型の型引数が解決中の型のmemberを要るとき、6.0は途中の表を読んで終わり、tsgoは同じ解決に
  再び入ってinstantiationの深さ100でTS5114／TS5115を出す（Vue.jsの`interface _Selector<S> extends
  Container<string, Diff<Node, S>>`と`type Selector = _Selector<Selector>`）。早い公開と、Err時にそれを取り消す
  処理を外した。
- **型引数の制約はその場で検査する**：`checkTypeReferenceOrImport`は、型を得た直後に型引数を制約と照合する
  （checker/checker.go:3046-3062）。tsc 6.0はこれをlazy diagnosticとして積み、fileの他の検査の後で、参照自身を
  current nodeにして実行していた。上のTS5115はinterfaceではなく参照の位置に出ており、制約のerrorは後続の文の
  errorより後に並んでいた。積むための補助（aliasの解決に再入し得る引数の判定、自己参照のaliasの判定）と、
  deferred nodeの型参照の分岐を消した。lazyの順序をpinしていたcheckerのunit testを新しい順序に直した。
- **交差型の縮約**（`getReducedType`、checker/checker.go:22177-22233）：
  - 交差型は、propertyを見る前に「計算済み」にする。propertyの解決の途中で同じ交差型に戻った問い合わせは、
    縮約されていない型を見る。tsc 6.0は計算の後で2つのflagを書いていた。
  - 構成要素が全て同じobject型の上のmapped typeなら、neverに縮約するpropertyを探さない
    （`isMappingOfSameObjectType`）。`M1<O> & M2<O>`は`kind`が`"a" & "b"`でも縮約されず、`M1<O> & M2<P>`は
    neverになる。zodの再帰するobject schemaでは、homomorphicなmapped typeを`{…} & {…}`の上でinstantiateする
    途中でそのmapped typeのmemberを解決しなくなり、tsc-rsだけが出していたTS5115（と続くTS2344の2件）が消えた。
  - それ以外では、2つ以上の構成要素が持つ名前だけ、合成のpropertyを作って調べる
    （`somePropertyReducesToNever`）。tsgoは名前をGoのmapの順でたどる。tsc-rsは構成要素が並べる順でたどる。
- **交差型のproperty**（`createUnionOrIntersectionProperty`、checker/checker.go:21789-21990）：
  - 交差型のpropertyはoptionalから始まり、構成要素のpropertyのうちproperty・method・accessorであるものだけが
    それを狭める。2つのnamespaceやmoduleがexportする同じ名前は、`typeof A & typeof B`の中でoptionalのままになる
    （`Property 'x' is optional in type 'typeof A & typeof B' but required in type …`）。tsc 6.0は最初のclass
    memberまでflagを立てなかった。
  - 構成要素のpropertyの宣言は、1つずつだけ集める（`AppendIfUnique`）。tsc 6.0は全てのlistを連結していて、
    構成要素自身が合成のpropertyのとき段ごとに倍になる。`intersectionConstructorReductionCrash`では3 GBになって
    いた。
- **比較の入れ子は100段でMaybe**：`recursiveTypeRelatedTo`は、比較が100段入れ子になると、進行中の比較に出会った
  ときと同じくMaybeを返す（checker/relater.go:3135-3140）。tsc 6.0はそこでoverflowを立て、比較全体をTS2321
  （Excessive stack depth comparing types）で失敗させていた。tsgoはTS2321を出さない。残るoverflowは複雑さの
  予算（TS2859）だけなので、`RelationComparisonResult`から`StackDepthOverflow`を消した（relater.go:66-75）。
  Next.jsでtsc-rsだけが出していたtypeboxの`UnionToTuple`の制約のTS2321 3件が消えた。同じ型を関数の戻り値に
  すると、100段でMaybeになった後、代入できないunionのmemberで失敗し、199行の鎖を出す（tsgoと同じbyte）。
- **varianceは、計測中の型のstackで測る**：`getVariancesWorker`は、計測中のgenericな型をstackに積む
  （checker/relater.go:1334-1434）。計測の途中で、既にstackにある型のvarianceが要ると、その循環の中でsymbolが
  最小の型（`compareSymbols`）から計測をやり直し、その型と途中で終わった型のvarianceを保存する。外側の計測は
  自分の途中の結果を捨てる。循環のどこから入っても同じvarianceになる。tsc 6.0は計測中の印をlinksに書き、
  最初に比較された型の中から循環の残りを測っていた。Vue.jsの`vModel.ts`では、戻り値の型のsubtype reductionが
  `ObjectDirective`から入り、`DirectiveBinding`が`dir` property抜きで測られ、tsgoが断る呼び出し（TS2345）が
  通っていた。保存された空のlistはtsgoの空のsliceで、計測中の型への循環の要求が残す印（計測が終わると
  置き換わる）であり、`getVariances`の呼び出し側はそれにTernary.Unknownを返す。
- **信頼性のflag**：`Reports*`のbitをtsgoの`reliabilityFlags`に移した（checker/checker.go:742、1146-1158、
  relater.go:3106、3155-3170）。report用のmapperがmarker型でbitを立て、`recursiveTypeRelatedTo`が比較ごとに
  集めて結果と一緒にcacheに書き、cacheのhitはそのbitを進行中の比較に足し、`getVariancesWorker`が型parameter
  ごとに読む。tsc 6.0の`outofbandVarianceMarkerHandler`のclosureの鎖は計測の間だけ在り、cacheのhitはsourceを
  report用のmapperでinstantiateして再生していた。関係のcacheの近道での再生（tsgoに無い）は消した。
- **型変数の無い型も、aliasの型引数はinstantiateする**：`instantiateTypeWithAlias`は、aliasの型引数が型変数を
  含み得る型もinstantiateする（checker/checker.go:22494-22500）。parameterを使わないunionや交差型のaliasも、
  参照ごとの型引数を持つ：`type Un<T> = string | number`は`Un<number>`、`type Brand<T> = number & {}`の
  `Brand<U>`は`number`（交差型は`& {}`無しで作り直される）。tsc 6.0は宣言された型をそのまま返し、診断と宣言に
  `Un<T>`、`Brand<T>`と書いていた。
- **`hasBaseType`は、同じ型を1回だけたどる**（tsrs-native）：tscとtsgoは、型から基底への全ての経路をたどる。
  interfaceがそれまでの全てのclassをextendsするmixinの鎖は、経路が指数的にある。
  `intersectionConstructorReductionCrash`はここで28 s（tsgoはfile全体で9.5 s）かかり、負荷のあるときに
  harnessの制限時間を超えるので、ratchetの外に置いていた。1回のたどりの中で基底に届かなかった型は、後で
  訪ねても届かないので、訪ねた型を覚える。上の宣言の重複と合わせて、このfileは0.1 s、63 MBになった
  （前は28 s、3.2 GB）。
- unit test：CLI（tsgoの出力にpin）で7件（基底型の後のmemberとTS5115の位置、同じobjectの上のmappingの交差型、
  100段の比較（通る場合と、199行の鎖で失敗する場合）、循環するvariance（Vue.jsの`vModel.ts`の縮約）、aliasの
  型引数、namespaceの交差型のoptionalなproperty）。checkerのunit test 1件をtsgoの順序に直し、speculationの
  unit testをvarianceのstackとflagに合わせた。
- 結果（corporaの`--noEmit`の診断、tsgo 7.1.0-dev-19dadef8と比べて）：hono、Playwright、TypeScript `src/compiler`、
  Next.js、Effect、Vue.js、VS Codeは、既定のchecker数で全て同じbyte（P3-5bdではVue.jsに4件足りず、Next.jsに3件
  多かった）。zodは1 checker同士（`TSRS_CHECKERS=1`と`--checkers 1`）で43行が同じ。既定のchecker数では、
  instantiationの深さのerror（TS5115）の出る場所がfileのcheckerへの割り当てで変わる：tsgo自身が`--checkers`
  1／2／4／8で35／35／36／35件を出し、tsc-rsは8 shardで37件（8回中7回。1回は36件）。256 file以上のprogramは
  shardが仕事を譲り合う（stealing）ので割り当てが実行ごとに変わり得る。`TSRS_SHARD_STEAL=0`では8回とも同じ出力。
- conformance（release build、`469ccc709`、`--workers 2`、459 s）：15,228構成、lane A 13,467、full 13,416、text 3、
  category 3、mismatch 27、emit full 13,432、emitの不一致9、未評価8、harness error 18、`.js.map`の不一致1。
  P3-5bdの最後のreportと行ごとに比べて変わったのは6構成：errorsがFullに上がった`circularVariance1`、
  `classVarianceResolveCircularity2`（varianceのstack）、`keyofGenericExtendingClassDoubleLayer`（基底型の後の
  member）、`templateLiteralTypeExcessiveLength`（制約をその場で検査する順序）、harness error（制限時間。前の
  buildは10分で終わらず5 GBを超えた）から全tierがFullになった`excessivelyDeepConditionalTypes`（0.44 s）、
  tierは同じで診断が変わった`mutuallyRecursiveInference`（TS5114がclassの位置になった。tsgoのCLIと同じ位置
  だが、baselineは`this.a`の位置。下の「次」）。他の構成はtierも、診断とemitのdigestも同じ。途中は21のfilterと、
  前のreportでerrorsがFullでなかった40 caseの個別の実行で確かめた。
- `--checkers 4`の並列対照：`--filter eclaration`（2,426構成）を1 checkerの同じfilterと比べ、2,424構成が同じ。違う
  2構成は記録済みのpartition依存。`--filter ariance`（28構成）と`--filter ircular`（73構成）は全て同じ。全caseの
  対照はlocalの負荷の方針により実行していない。
- ratchet：0 regressions。上の5行と`intersectionConstructorReductionCrash`を足した（13,416→13,422行）。後者は
  ratchetの外に置いていたが、0.1 sで終わるようになった。
- local：formatと、types・checker・emitter・compiler・conformanceのclippy、test（`ed1f0ff40`で2,759件）。最初の
  実行では、lazyの順序をpinしていたcheckerのunit test 1件が失敗し、tsgoの順序に直した。workspace全体のtestと
  clippyはhostedの`rust` job。
- perf（README corporaとVue.js、nice 20、main（P3-5bdのbuild、`71feb95f6`）対tsgo 7.1.0-dev、branchは
  `469ccc709`）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 144→166、zod 573→557、Playwright 398→393、TypeScript
    `src/compiler` 365→369、Next.js 891→810、Effect 580→586、Vue.js 403→386、VS Code 3,898→3,884。読み込んだ文書数は
    8 corporaで同一。診断はzod、Next.js、Vue.jsで変わった（上の通りtsgoの側へ）。honoは6回ずつの計り直しで
    0.11–0.13 sと0.12–0.13 s、1 checkerの命令数は4.316→4.260 G。
  - `tsconfig.bench-full.json` 3回：hono 158→162、zod 706→723、Playwright 545→526、TypeScript `src/compiler`
    581→552、Next.js 1,157→1,111、Effect 811→854（最小値は780→785）、Vue.js 465→466。出力は7 corporaとも
    mainと同じbyte。
  - 10回のA/B：`--noEmit` Effect 565→543、zod 558→552、VS Code 3,674→3,620、Next.js 830→811、Vue.js 383→367。
    `bench-full` TypeScript `src/compiler` 546→544、Next.js 1,111→1,106、Playwright 545→519、Effect 851→820、
    Vue.js 454→425。peakは±2%以内（Playwrightの`--noEmit`は5回ずつの計り直しでmedian 791→763 MB）。1 checkerの
    命令数branch÷main：`--noEmit` zod 0.968、Effect 0.978、Next.js 0.956、Playwright 0.976、Vue.js 0.964、hono
    0.987、`bench-full` TypeScript `src/compiler` 0.991、Next.js 0.969、Vue.js 0.972。劣化無し（関係のcacheの
    近道での再生が無くなったことと、交差型のpropertyの扱いが軽くなったことで、2–4%減った）。
  - tsgo（同じ計測の3回のmedian、ms／peak MB）：`--noEmit` hono 181／324、zod 931／1,710、Playwright 578／994、
    TypeScript `src/compiler` 405／395、Next.js 1,334／1,684、Effect 815／1,205、Vue.js 551／730、VS Code
    5,257／6,758。`bench-full` hono 210／380、zod 1,062／1,925、Playwright 752／1,340、TypeScript `src/compiler`
    691／737、Next.js 1,881／2,013、Effect 1,168／1,820、Vue.js 639／861。
  - 出力（`bench-full`、tsgoと同じbyteのfile数）：hono、zod、Playwright、TypeScript `src/compiler`、Next.js、Vue.jsは
    全file。Effectは宣言493/496（この回のtsgoの1回の出力に対して。tsgo自身の実行ごとの違いを含む）、他は全て。
- 次（conformanceでerrorsがFullでない33構成を見直した分類）：
  - **harnessはemitの後の診断をbaselineにする**：tsgoのtest harnessは、programを2つ作り、2つ目は先に`Emit`して
    から診断を集め、そちらを`.errors.txt`に書く（testutil/harnessutil/harnessutil.go:660-690）。emitのresolverが
    先に型を解決するので、順序に依存するerrorの位置が変わる：`mutuallyRecursiveInference`のTS5114は、CLIでは
    class `X`（8,7）、baselineでは`this.a`（12,9。`checkExpression`がcurrent nodeにした式）。
    `recursiveMappedTypes`のTS2615の位置、`recursivelyExpandingUnionNoStackoverflow`の余分なTS5114も同じ形の
    可能性がある。runnerに「emitしてから検査する」経路が要る。
  - **再帰の同一性**：`getRecursionIdentity`は、型nodeの解決から来た型参照（`ObjectFlagsFromTypeNode`。
    `createTypeReferenceEx`が最初に作ったときだけ付く）をsymbolやtupleのtargetでまとめない
    （checker/relater.go:820-865、checker.go:23664、24600-24602）。`Inner[]`→`Mid[]`→`Leaf[]`の3段が
    「深い入れ子」と見なされず、`deeplyNestedArrayTypes`、`deeplyNestedTupleTypes`のTS2322が出る。indexed accessの
    同一性も、左端のobject型そのものではなく、その型の同一性になった。
  - 循環の関連情報TS2751（`Circularity originates in type at this location`。
    `incorrectRecursiveMappedTypeConstraint`、`typeParameterWithInvalidConstraintType`）。
  - 推論と循環：zod形のgetterの循環のTS7022／TS7023（`recursiveTypeInference`、`recursiveTypeInference2`）、
    `infiniteConstraints2`のTS2589、`nestedGenericTypeInference`のTS2345、`mixinWithBaseDependingOnSelfNoCrash1`の
    TS2345、`contextualTypingGenericFunction2`のTS2322、`recursiveIndexedAccessSimplification`のTS2322。
  - JSX（`checkJsxChildrenProperty15`、`16`、`tsxStatelessFunctionComponentOverload4`）、JavaScriptの代入宣言
    （`nestedPrototypeAssignment`、`prototypePropertyAssignmentMergeWithInterfaceMethod`、
    `typeFromPropertyAssignment9`、`9_1`）、type-onlyのalias（`importEquals3`、`typeOnlyMerge3`）、その他
    （`awaitedTypeNoLib`、`bigintPropertyName`、`blockScopedBindingUsedBeforeDef`、`mappedTypeConstraints2`の鎖）。
  - 既定のchecker数での診断の再現性：zodのように順序に依存するerrorがあると、stealingで出力が実行ごとに変わり
    得る。静的な割り当て（`TSRS_SHARD_STEAL=0`）を既定にするかは、速度との兼ね合いで未決（stealingを入れた
    ときの記録は、zodで723 ms対983 ms）。
  - P3-5bdから持ち越し：`typeTagOnFunctionReferencesGeneric`、匿名のmapped typeのunionの順序、
    `objectTypesWithOptionalProperties2`。
- hosted：PR #682（head `b6acb7286`）、run 37296279975 — `plan` 31s、`rust` 11m14s、`conformance (TypeScript 7.1)` 20m23s、
  `gates` 12s。

## P3-5bf 7.1のcheckerの規則：再帰の同一性、推論、循環の扱い、JSX、JavaScriptの代入、初期化の順序（2026-10-05）

P3-5beの「次」に挙げた、conformanceでerrorsがFullでない構成を順に調べた。原因は1つずつ別の規則だった。
調べ方はP3-5beと同じで、instantiationのstackと呼び出しの経路をtsgo（同じcommitのsourceの写しに計測を足して
buildしたもの）とtsc-rsの両方で出力し、最初に分かれる所を見た：
- **再帰の同一性**：型nodeから作った型参照は`ObjectFlagsFromTypeNode`を持ち、`getRecursionIdentity`はそれを
  symbolやtupleのtargetでまとめず、node自身を同一性にする（checker/relater.go:766-870、checker.go:23664、
  24600-24602、25572-25584）。`Inner[]`→`Mid[]`→`Leaf[]`の3段が「同じ型が深く入れ子になった」と見なされ
  なくなり、比較が末端の違いまで届く（`deeplyNestedArrayTypes`、`deeplyNestedTupleTypes`のTS2322）。indexed
  accessの同一性も、左端のobject型そのものではなく、その型の同一性になった。
- **循環するmapped typeのproperty**：`getTypeOfMappedSymbol`は、循環を見つけたpropertyにerror型を書いてから
  TS2615を出す（checker/checker.go:21345-21349）。messageに書くmapped typeの表示が同じpropertyの型をもう一度
  求めるので、tsc 6.0の順序（messageの後で書く）では、表示がinstantiationの深さの制限まで解決をやり直して
  いた（`recursivelyExpandingUnionNoStackoverflow`の余分なTS5114）。
- **computed property nameの検査中の印**：`checkComputedPropertyName`は結果をcomputed nameのnodeにcacheし、
  式を検査している間はそのnodeに循環の印（`circularConstraintType`）を置く（checker/checker.go:27272-27290）。
  `let {[b]: b} = {}`のように、名前の式が自分の宣言する変数を指すbinding elementで、宣言前の使用を1回だけ報告し、
  implicit anyを出さない（`blockScopedBindingUsedBeforeDef`）。
- **要求時に見つからないglobalは全て報告する**：検査の途中でglobalを探して見つからなかったときの診断は、
  tscでもtsgoでもglobalの診断になる。tsc-rsは一部のgetterの分しかprogramのglobalの診断に公開していなかった。
  `getGlobalSymbol`が見つからなかった分を全て公開する（`noLib`での`Awaited`：`awaitedTypeNoLib`）。libの無い
  programでは、`import()`やarrow functionの戻り値のelaborationが探す`Promise`、import attributesが探す
  `ImportAttributes`の診断も出るようになった（tsgoと同じ。unit testの期待を直した）。
- **index型の基底制約とremappingするmapped typeのkey**：`as`節を持つgenericなmapped type `M`に対する`keyof M`の
  基底制約は、そのkeyの制約になる（checker/checker.go:27901-27910、27988-27995）。`getSimplifiedType`にindex型の
  場合は無い（28367-28375）。`checkIndexedAccessIndexType`は、remappingするmapped typeのkeyを遅延させずに取る
  （8399-8405）。`mappedTypeConstraints2`の鎖がtsgoと同じになった。
- **indexed accessの単純化は自分自身を除く**：`getSimplifiedIndexedAccessType`は、単純化の結果がunionで、
  その中に元の型自身があれば除く（checker/checker.go:28380-28393）。再入した読み出しは元の型をそのまま返すので、
  自分を含むunionができる。tsc 6.0は残していた。
- **近い型同士の推論は、深い方から**：`inferFromMatchingTypes`は、「近い型」（同じobject型のinstantiationや同じ
  aliasのinstantiation）の組を先に集め、targetをgenericなinstantiationの深さの大きい順に並べてから推論する
  （checker/inference.go:370-409、`compareTypesAndDepth`、`getTypeDepth`）。`T[] | T[][]`への
  `Value[] | Value[][]`の推論で、`Value[][]`→`T[][]`が先になり、Tは`Value`になる。tsc 6.0はunionの並び順で
  推論し、Tを`Value[]`にして呼び出しを断っていた（`nestedGenericTypeInference`のTS2345）。
- **再帰した呼び出しの解決は、推論を制約と照合しない**：tsgoは、overloadを選んでいる最中の呼び出しをstackに
  積む（`Checker.callResolutionStack`、checker/checker.go:798、9108-9116）。stackにある呼び出しをもう一度解決する
  とき、候補が1つなら、型引数の推論を制約と照合しない（`InferenceFlagsNoConstraintChecks`、checker.go:9243-9246、
  inference.go:1381）。制約との照合は、外側の解決がまだ作っている途中の型を読むからである。
  `class Item extends ClientDocumentMixin(BaseItem)`と`BaseItem extends Document<typeof Item>`では、内側の解決が
  construct signatureのまだ無い`typeof BaseItem`を制約と比べ、TS2345を出していた
  （`mixinWithBaseDependingOnSelfNoCrash1`）。zodの形の再帰するgetterのimplicit anyも同じ原因だった
  （`recursiveTypeInference`、`recursiveTypeInference2`）。
- **失敗した呼び出しの解決は、入れ子の解決がsignatureをcacheしていても報告する**：`resolveCall`は、選べた
  overloadを返すか、失敗なら失敗の候補を書いてから診断を出す（checker/checker.go:9117-9133）。同じnodeの
  入れ子の解決がcacheしたsignatureへの差し替えは、`getResolvedSignature`が`resolveCall`の戻った後で行う
  （8606-8614）。tsc 6.0は`resolveCall`の中で、診断を出す前に、cacheされたsignatureを返していた。上の規則と
  組み合わさると、arrow functionの引数の中の呼び出し（外側の呼び出しの推論のためにもう一度解決される）で、
  入れ子の解決が制約の照合無しで成功し、外側がその結果を採って、制約に合わない引数のerrorが消える。zodの
  `// @ts-expect-error`付きの`z.templateLiteral([z.object({})])`など23箇所が「使われていないdirective」
  （TS2578）になった。conformanceにこの形は無く、corporaの診断をtsgoと比べて見つけた（full runの後のcommit）。
- **conditional typeのpermissive／restrictiveなinstantiationは、要るときだけ作る**：`getConditionalType`は、check
  型とextends型のpermissive／restrictiveなinstantiationを`&&`と`||`の先で作る（checker/checker.go:24838-24893）。
  extends型が`unknown`や`any`なら1つも作らない。tsc-rsは4つを先に全部計算していたので、
  `T extends unknown ? … : never`でcheck型を毎回permissive mapperでinstantiateしていた。制約をたどるたびに
  check型が1段深くなる`infiniteConstraints2`では、これが深さの制限に届いていた（TS2589）。
- **iterableでない型の診断は、fileの検査の終わりに出す**：`reportTypeNotIterableError`は
  `addDeferredDiagnostic`で積まれ、fileの検査の終わりに実行される（checker/checker.go:6463-6467、6511-6522。
  「TypeToStringは解決中のsymbolを解決しようとして循環を起こし得る」）。tsc 6.0はその場で出していたので、
  `function* foo() { yield*foo }`では、戻り値型の推論の中で関数を`() => any`と表示し、その解決を失敗させて
  いた。tsgoは`() => Generator<any, void, unknown>`と書く（`YieldExpression6_es6`）。後回しにする診断
  （存在しないpropertyと、iterableでない型）は1つのlistにまとめ、積んだ順に実行する
  （`Checker.deferredDiagnosticCallbacks`）。
- **contextualな関数の戻り値型は、省略無しで計算する**：`contextuallyCheckFunctionExpressionOrObjectLiteralMethod`
  は、戻り値型を本体から求める前に`CheckModeSkipContextSensitive`を外す（checker/checker.go:10377-10384。
  「resolvedReturnTypeはずっとcacheされるので、anyFunctionTypeが混ざってはならない」）。tsc 6.0はmodeをそのまま
  渡していたので、context-sensitiveな関数を返すgenericなcallbackの戻り値型がwildcardの関数のまま残り、返す
  関数が合わなくても通っていた（TypeScript issue 61979、`contextualTypingGenericFunction2`のTS2322 6件）。
- **型がerrorのpropertyは、pseudo typeで表示する**：enclosing declarationのある型の表示では、
  `serializeTypeForDeclaration`がpropertyの値の宣言のpseudo typeを実際の型と比べ、実際の型がerror型なら等しいと
  見なす（checker/pseudotypenodebuilder.go:363-366）。object literalに同じ名前を2回書くと、値の宣言は最初の
  もの、型は最後のものになるので、最後の値が解決できないとき最初の値のpseudo typeが表示される：
  `{ a: 1, a: missing }`は`{ a: number; }`。pseudocheckerが諦める式（`[1]`など）はcheckerの型（`any`）、型
  assertionは書かれた型nodeになる。tsc 6.0は全て`any`だった（error recoveryでobject literalになった
  `reachabilityChecksNoCrash1`）。診断用の型の表示に、この場合だけを足した（下の「次」）。
- **何も書いていないliteralも、書いていないpropertyで判別する**：`discriminateContextualTypeByJSXAttributes`と
  `discriminateContextualTypeByObjectMembers`は、nodeにsymbolがあれば、書かれていないoptionalな判別propertyを
  `undefined`として使う（checker/jsx.go:266-292）。tsc-rsはmemberの表が空でないことを条件にしていたので、属性の
  無い`<Foo>{(value) => {}}</Foo>`が判別されず、childの関数のparameterがimplicit anyになっていた
  （`checkJsxChildrenProperty16`のTS7006）。
- **JSXのbodyのchildrenは、excess propertyの検査を受ける**：elementのbodyから合成する`children` propertyには、
  親が属性のnodeである偽のproperty signatureが宣言として付く（checker/jsx.go:845-848）。
  `shouldCheckAsExcessProperty`はその親を見る。tsc-rsはnodeを合成しないので宣言が無く、bodyのchildrenは
  excess propertyにならなかった：`children`を取らないcomponentへの`<Tag key="1"><div></div></Tag>`が通り、
  この検査だけが落とすoverloadが選ばれていた（`checkJsxChildrenProperty15`、
  `tsxStatelessFunctionComponentOverload4`）。偽の宣言の親をsymbolのlinksに記録し、検査がそれを読む。
- **予約名のmemberはpropertyにならない**：`setStructuredTypeMembers`は、型のpropertyを
  `getNamedMembers(members)`から取り、予約名（`__`で始まる内部名）を落とす。解決済みの表から作る匿名型で、
  tsc-rsは表の全てをpropertyにしていた。bigint literalを名前にしたobject literalのmemberはbinderが
  `__missing`として宣言するので、`{ 3n: "x" }`が`{ __missing: string; }`になり、excess property（TS2353）に
  なっていた。tscとtsgoでは`{}`で、足りないpropertyのTS2741になる（`bigintPropertyName`）。
- **`X = X || {}`は普通の式**：tsc 6.0は、JavaScriptの`X = X || {}`（既定値付きのexpandoの初期化）の型を右の
  operandだけから取り、その代入の検査を省いていた。tsgoにはこの形が無く、`checkBinaryLikeExpression`は両方の
  operandを検査し（checker/checker.go:12538-12544）、代入は常に比べる。tsc-rsは前半だけが残っていたので、
  `self['Common'] = self['Common'] || {}`で`{}`を`Common`の型と比べ、expandoのmemberが足りないと報告していた
  （`jsElementAccessNoContextualTypeCrash`、`typeFromPropertyAssignment9`、`9_1`）。
- **JavaScriptのproperty代入の根は、普通の名前**：tsc 6.0のbinderは、宣言の無い名前へのproperty代入に入れ物を
  宣言していた。tsc-rsのcheckerには、その代わりの例外が2つ残っていた：prototypeへの代入の根
  （`C.prototype = {}`、`C.prototype.m = …`）は未解決でも報告せず、値を持たないnamespaceを根にする代入は
  TS2708ではなくTS2304にしていた。tsgoのbinderはどちらの入れ物も宣言せず、checkerは普通の名前として解決する：
  使用のたびにTS2304、namespaceならTS2708（`nestedPrototypeAssignment`、
  `prototypePropertyAssignmentMergeWithInterfaceMethod`）。
- **exportの対象が可視になるのは、そのfileの宣言をtransformするとき**：export assignment、export specifier、
  CommonJSの`module.exports = x`が名指す宣言を可視にする印は、`PrecalculateDeclarationEmitVisibility`が付ける
  （checker/emitresolver.go:236-306）。これは宣言のtransformerが、transformするfileごとに1回呼ぶ
  （transformers/declarations/transform.go:304）。tsc 6.0は、checkerがexportを検査したときに、全てのfileで
  付けていた。declaration fileはtransformされないので、その`export = foo`は`namespace foo`を可視にしない：
  別のfileのaugmentationが`foo`のmemberに解決する名前はprivate nameになる（TS4060。
  `exportAssignmentMembersVisibleInAugmentation`）。tsc-rsの印は、そのfileを検査したchecker（shard）にだけ
  付いていたので、結果がfileの割り当てに依存していた（同じprogramが、libの数で結果を変えた）。fileのexport
  assignmentとexport specifierは、node recordの走査で見つけ、sourceの順に処理する（tsgoはfileの全nodeを
  たどる）。他のfileの文が「後から可視にするalias」として返る場合（augmentationの中の名前が、どのexportも
  可視にしていない宣言に解決したとき）、tsgoはその文をtransformして誰も読まない置き換えを作る。tsc-rsの
  transformerはこれを契約違反として止めていたので、その文を飛ばすようにした（harness errorだった
  `declarationEmitComputedPropertyNameSymbolStripInternal`がFullになった）。
- **初期化の順序：ambient moduleは、global augmentationの後でmergeする**：tsgoの`initializeChecker`は、script
  fileのlocalsをglobalsにmergeし、次にglobal scopeのaugmentation、global型の取得、その後でscript fileの
  ambient moduleの宣言（「他のglobalのsymbolや型の解決が要ることがあるので後に回す」）、pattern module、
  module augmentationの順に進む。
  - program driverは、moduleの解決の準備をglobalsのmergeの前に済ませる。同じambient moduleの2つの宣言を
    mergeすると、exportの中のaliasを解決する（`mergeSymbol`の`resolveSymbol`）ので、moduleの読み込みが要る。
    前は、importが見つからない（TS2307）と報告し、mergeの衝突（TS2451）を報告しなかった。
  - ambient moduleのsymbolは、localsのmergeから外し、global scopeのaugmentationの後でmergeする。
  - global型のsymbolは、global scopeのaugmentationの後で引き直す。宣言が1つしかないglobalは、最初の
    augmentationがmergeしたときに表の中で複製に置き換わるので、その前に引いたsymbolは古い。`lib`がes5だけの
    とき、augmentした`Array<T>`の宣言型が2つでき、`string[]`にaugmentationが見えなかった（TS2339）。mainでも
    再現する既存の不具合だった（`globalArrayAugmentationWithAmbientModuleReexportMerge1`）。
- **deferredな型引数は、外側の解決が上書きする**：`getTypeArguments`は、解決のframeをpopしてから、nodeの
  型引数を参照のmapperでinstantiateし、結果を`??=`で書く（checker/checker.go:22319-22323）。slotが空かどうかを
  見るのはinstantiateの前で、代入はその後なので、instantiateの途中で同じ型引数がもう一度解決されると（再帰する
  aliasは深さの制限までこれを繰り返す）、外側のframeの結果が内側のframeの書いたものを置き換える。tsc-rsは
  slotが空のときだけ書いていたので、一番内側のframeのerror型が配列の要素型として残り、`Recur<T>[number]`を
  返す呼び出しの型が`any`になっていた（tsgoでは`(T extends unknown[] ? {} : {…})[number]`）。
- **型parameterへの参照は、symbolで見つける**：`isTypeParameterPossiblyReferenced`は、型参照のsymbolを型
  parameterのsymbolと比べる（`getSymbolFromTypeReference(node) == tp.symbol`）。tsc 6.0は参照の型を型parameterと
  比べていた。distributiveなconditional typeのcheck型は、型parameterの「distributed」な形（同じsymbolの別の型。
  P3-5aj）なので、型の比較では参照が1つも見つからず、`isDistributionDependent`が全てのdistributiveな
  conditional typeでfalseになっていた。そのため、そういうconditional typeへの関係が「結果が分配に依存しない」
  場合の分岐に入り、check型をもう一度instantiateしていた（`recursiveIndexedAccessSimplification`で、TS2322の
  代わりにTS2589）。
- **harness：`extends`をたどってskipを決める**：tsgoのtest harnessは、testのtsconfigをcompilerのparserで読み、
  その結果のoptionでskipを決める（`SkipUnsupportedCompilerOptions`）。`extends`で継承した`baseUrl`もskipの
  理由になる：`pathMappingInheritedBaseUrl`にbaselineは無い。runnerはtsconfigのunit自身の`compilerOptions`
  しか見ておらず、referenceの無いcaseを実行していた。相対pathと絶対pathの`extends`（文字列とlist）を、testの
  unitの中でたどる。
- unit test：CLI（tsgoの出力にpin）で20件。19件は上の規則ごとで、どれもmainのbuildでは違う出力になる。1件は
  入れ子の呼び出しの解決（zodの縮約）。harnessの`extends`で1件。既存のunit testは、宣言のtransformのときの印、
  TS2615だけになった循環、libの無いprogramで公開されるようになったglobalの診断、prototype代入の根に合わせて
  直した。
- 結果（corporaの`--noEmit`の診断、tsgo 7.1.0-dev-19dadef8と比べて、`d193abfeb`のbuild）：hono、Playwright、
  TypeScript `src/compiler`、Next.js、Effect、Vue.js、VS Codeは、既定のchecker数で全て同じbyte。zodは37行対36行で、
  違いはP3-5beに記録したpartition依存のTS5115の1行。`af839cb7c`のbuildでは、zodに使われていない
  `@ts-expect-error`（TS2578）が23件多かった（上の「失敗した呼び出しの解決」）。conformanceはその時点で
  0 regressionsだったので、この後退を見つけたのはcorporaの診断の比較だけだった。
- conformance（release build、`af839cb7c`、`--workers 2`、468 s）：15,228構成、lane A 13,466、full 13,443（+27）、
  text 2、category 0、mismatch 4、emit full 13,434（+2）、emitの不一致7、未評価8、harness error 17（−1）、skip
  1,720（+1）、`.js.map`の不一致1。P3-5beの最後のreportと行ごとに比べて、errorsがFullに上がったのは27構成
  （上の各項目に挙げたcase。`declarationEmitComputedPropertyNameSymbolStripInternal`はharness errorから全tierが
  Full、`exportAssignmentMembersVisibleInAugmentation`はemitもFull）。`pathMappingInheritedBaseUrl`は比較から
  skipに移った。他の構成はtierも、診断とemitのdigestも同じ。
- full runは`d193abfeb`（入れ子の呼び出しの解決）より前のbyteで取った。`d193abfeb`のbuildでは、10のfilter
  （`all`、`eneric`、`nfer`、`ontextual`、`sx`、`verload`、`eclaration`、`ecursive`、`xport`、`tslib`：重複を除いて
  5,553構成）が、full runのreportとoutcome、tier、診断とemitのdigestまで同じことを確かめた。headでの全体の実行は
  hostedの`conformance (TypeScript 7.1)` job。
- `--checkers 4`の並列対照（`d193abfeb`、1 checkerの同じfilterと比べる）：`eclaration` 2,426構成、`xport` 1,067
  構成、`ecursive` 135構成、`nfer` 357構成、`tslib` 7構成が全て同じ。P3-5beまで違っていた2構成
  （`declarationEmitAugmentationUsesCorrectSourceFile`、`declarationEmitComputedPropertyNameSymbol2`。4 checkerで
  harness error）は、exportの対象の印をtransformのときに付けるようになって消えた。
  [conformance-ts71](../conformance-ts71/README.md#並列実行の対照)の表の5件は、1 checkerでも4 checkerでもFullに
  なった。全caseの対照はlocalの負荷の方針により実行していない。
- ratchet：0 regressions。23行を足し、4行（`blockScopedBindingUsedBeforeDef`、`reachabilityChecksNoCrash1`、
  `YieldExpression6_es6`がcategoryから、`mappedTypeConstraints2`がtextから）をfullに上げた（13,422→13,445行）。
- local：formatと、types・checker・emitter・compiler・conformance・harnessのclippy、test（`d193abfeb`で2,806件）。
  最初の実行では、置き換えた挙動をpinしていたunit test 7件が失敗し、tsgoの挙動に直した（上の「unit test」）。
  workspace全体のtestとclippyはhostedの`rust` job。
- perf（README corporaとVue.js、nice 20、main（P3-5beのbuild、`469ccc709`）対tsgo 7.1.0-dev、branchは
  `d193abfeb`）：
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 136→135、zod 528→527、Playwright 375→376、TypeScript
    `src/compiler` 335→343、Next.js 811→780、Effect 540→551、Vue.js 348→344、VS Code 3,547→3,558。読み込んだ
    文書数と出力は8 corporaでmainと同じ。peak（MB）：hono 317→299、zod 1,300→1,295、Playwright 799→803、
    TypeScript `src/compiler` 284→284、Next.js 1,326→1,290、Effect 1,016→1,041、Vue.js 598→595、VS Code
    5,424→5,430。
  - `tsconfig.bench-full.json` 3回：hono 159→157、zod 648→634、Playwright 504→497、TypeScript `src/compiler`
    552→529、Next.js 1,069→1,027、Effect 764→780、Vue.js 419→412。peak：hono 337→329、zod 1,525→1,532、
    Playwright 872→874、TypeScript `src/compiler` 476→474、Next.js 1,445→1,434、Effect 1,160→1,174、Vue.js
    625→623。出力は7 corporaとも、全fileがmainと同じbyte。
  - 10回のA/B：`--noEmit` Effect 529→526、zod 530→526、VS Code 3,454→3,467、Next.js 772→757、Vue.js 351→341。
    `bench-full` TypeScript `src/compiler` 498→503、Next.js 1,003→998、Playwright 475→476、Effect 752→765、Vue.js
    404→403。peakは±2%以内。
  - 1 checkerの命令数branch÷main：`--noEmit` zod 0.996、Effect 0.998、Next.js 0.980、Playwright 0.962、Vue.js
    0.999、`bench-full` TypeScript `src/compiler` 1.000、Next.js 0.988、Vue.js 1.000。
  - Effectの`bench-full`は3回と10回のどちらでも2%ほど遅く読めたので、計り直した：1 checkerの命令数は3回ずつで
    51.20／51.37／51.40 G対51.21／51.32／51.43 G、20回のA/Bはmedian 717 ms対720 ms（最小値はどちらも702 ms）、
    peakは1,166 MB対1,163 MB。差は計測の誤差だった。Effectの`--noEmit`のpeak（1,016→1,041）も、10回のA/Bでは
    1,035対1,036。劣化無し。fileごとのexportの走査（node record）は命令数に表れない。
  - tsgo（同じ計測の3回のmedian、ms／peak MB）：`--noEmit` hono 167／325、zod 907／1,716、Playwright 575／1,012、
    TypeScript `src/compiler` 358／390、Next.js 1,313／1,612、Effect 803／1,202、Vue.js 503／724、VS Code
    4,802／6,936。`bench-full` hono 204／382、zod 1,007／1,915、Playwright 719／1,334、TypeScript `src/compiler`
    645／654、Next.js 1,842／2,044、Effect 1,114／1,782、Vue.js 605／861。
  - 出力（`bench-full`、tsgoと同じbyteのfile数）：hono、zod、Playwright、TypeScript `src/compiler`、Next.js、Vue.jsは
    `.js`、`.d.ts`、両方のmapの全file。Effectは宣言494/496（mainも同じ）、他は全て。
- 次（conformanceでerrorsがFullでない6構成と、その他の残り）：
  - **type-onlyのalias**（`importEquals3`、`typeOnlyMerge3`）：tsgoは、type-onlyの宣言を、そう書かれたalias
    自身（とexport star経由）にだけ記録し、`resolveAlias`が純粋なaliasをたどるときに次のaliasの記録を写す
    （`resolveIndirectionAlias`）。`getTypeOnlyAliasDeclarationEx`は、求める意味を持つsymbolに着くまで1段ずつ
    解決して記録を見る。tsc-rsはtsc 6.0の形（解決のたびに、直接の対象と最終の対象から印を付ける）のままで、
    印を付ける場所が17、問い合わせが12ある。次のslice。
  - **harnessはemitの後の診断をbaselineにする**（`mutuallyRecursiveInference`、`recursiveMappedTypes`、
    `incorrectRecursiveMappedTypeConstraint`、`typeParameterWithInvalidConstraintType`）：P3-5beに書いた通り、
    runnerに「emitしてから検査する」経路が要る。後ろの2つはTS2751の関連情報の違いで、同じ原因
    （emit resolverが先に型を解決する）。
  - 型がerrorのpropertyのpseudo typeは、property assignmentとshorthandの宣言だけ、単純なpseudo type（literal、
    書かれた型node）だけを表示する。他の宣言の種類と、合成のpseudo type（関数、object literal、tuple）は
    宣言自身の型の表示のまま。
  - 他のfileの文である「後から可視にするalias」は飛ばすだけで、transformしない。tsgoはtransformするので、
    その中でaccessibilityのerrorが見つかれば他のfileのnodeに報告する。その形のcaseはconformanceに無い。
  - emitの不一致7（`binderBinaryExpressionStress`、`comparisonAnonymousMappedTypes`、
    `comparisonReverseMappedTypes`、`privateNameStaticMethod`、`computedPropertyNames52`のtarget=es2015、
    `typeTagOnFunctionReferencesGeneric`、`objectTypesWithOptionalProperties2`）、harness error 17
    （`runExternalCode`の15件、`deduplicatePackages`の2件）。
  - 既定のchecker数での診断の再現性（zod。P3-5beの記録の通り未決）。
- hosted：PR #683（head `954075524`）、run 37323970919 — `plan` 28s、`rust` 10m3s、`conformance (TypeScript 7.1)` 19m43s、
  `gates` 13s。merge commitは`635305534`。

## P3-5bg type-onlyのaliasをtsgoのmodelにする（2026-10-06）

P3-5bfの「次」の1つ目。tsgoは、aliasの解決とtype-onlyの記録を、tsc 6.0と違う形に作り直している。tsc-rsは6.0の
形のままだったので、aliasを解決する関数群をtsgoの形に書き換えた（checker/checker.go:14667-15341、
15885-15947、16055-16086、16585-16712、2173-2194）：
- **aliasの宣言のtargetは、直接のtarget**：`getTargetOfAliasDeclaration`に`dontRecursivelyResolve`の引数は無く、
  どの種類の宣言でも、その宣言が直接名指すsymbolを返す（import clause、import specifier、export specifier、
  `import x = a.b`、`export =`などの式は、全て`dontResolveAlias`がtrue）。targetが他の意味を持たない純粋なaliasの
  ときに先へたどるのは`resolveAlias`で、`resolveIndirectionAlias(source, target)`がtargetを解決し、targetの
  type-onlyの記録を、記録の無いsourceに写す。tsc 6.0は、各関数が自分のtargetを最後まで解決し、「直接のtarget」
  と「最終のtarget」の両方からaliasに印を付けていた。
- **type-onlyの記録は、そう書かれたalias自身に付く**：`markSymbolOfAliasDeclarationIfTypeOnly(aliasDeclaration,
  exportStarDeclaration)`は、宣言自身が`import type`／`export type`のときにその宣言を、名前が`export type *`
  経由でしか届かないとき（`typeOnlyExportStarMap`）にそのexport宣言を記録する。先に書かれた記録は変えない。
  targetが何であるかは見ない。linksの`typeOnlyDeclaration`は「未計算／無し／宣言」の3状態から「無し／宣言」に
  なり、`typeOnlyExportStarName`は無くなった。
- **問い合わせ**：`getTypeOnlyAliasDeclaration(symbol)`は、aliasを解決した後のそのalias自身の記録。意味を指定する
  `getTypeOnlyAliasDeclarationEx(symbol, meaning)`は、その意味を持つsymbolに着くまでaliasを1段ずつたどり、途中の
  記録を返す。aliasが同じ意味の宣言とmergeされていれば、そこで終わる。`getSymbolFlagsEx`は、
  `excludeTypeOnlyMeanings`のとき、type-onlyのaliasに着いた所で止まる（6.0は、type-onlyの宣言の解決先まで進んで
  いた）。
- **import aliasの報告**：`checkAndReportErrorForResolvingImportAliasToTypeOnlySymbol`は、`import x = a.b.c`の
  名前を、全体から左端の識別子へ順に、どの意味ででも解決し、type-onlyのaliasを名指す最初の部分でTS1380を
  報告する。6.0は、import alias自身にtargetから印が付いたかを見ていた。
- 一緒に合わせた所：`resolveESModuleSymbol`はaliasの宣言を受け取り、純粋なaliasである`export =`を
  `resolveIndirectionAlias`でたどる。`combineValueAndTypeSymbols`は、型の側が値の意味を持てばそれを返し、
  `getExternalModuleMember`は両方あれば（同じsymbolでも）合成する。`getTargetOfExportAssignment`は、namespaceの
  中の`export =`／`export default`（文法error）にtargetを持たせない。decorator metadataの型参照の直列化は、
  値のsymbolがあっても型のsymbolのtype-onlyの記録を見る。
- 観測できる違い（どれもtsgoと同じ出力になった。probeは25本）：
  - `import type * as a`を通した`import A = a.A`は、import aliasの位置のTS1380だけになる。`A`自身はtype-onlyに
    ならないので、`A`の使用（TS1361）と、`A`をre-exportした先のimport alias（TS1380）は報告されない
    （`importEquals3`）。
  - 関数を`export type { A }`し、それをimportしてnamespaceとmergeし直した`A`は、値として使える。たどる途中で
    値の意味を持つsymbol（mergeされたalias）に着くので、その先の`export type`に届かない（`typeOnlyMerge3`の
    TS1362 3件が消える）。
  - `export type *`でしか届かない名前を、`export { A as A1 } from`で名前を変えてre-exportすると、`A1`の値としての
    使用がTS1362になる。6.0は、別名`A1`を`export type *`のmoduleのexportから探して見つけられず、通していた。
  - `import type A = require(…)`を名指す`import AA = A`は、namespaceとしての解決に失敗しても（TS2702）、import
    aliasのTS1380が出る。
  - **tsgoの挙動で、6.0より緩い所**：`import type T = N.C`（文法errorのTS1392）で、名前の中にaliasが1つも
    無いと、type-onlyは記録されず、`new T()`は報告されない。tsgoはimport aliasのtype-onlyを、名前の中のaliasを
    解決するとき（`resolveEntityName`、checker.go:16135-16138）にだけ記録するためである。名前がimportから始まる
    `import type U = M.C`は記録され、使用がTS1361になる。上流の退行かもしれないが、tsgoの出力に合わせ、unit
    testに固定した。
- unit test：CLI（tsgoの出力にpin）で5件（type-onlyのnamespaceを通したimport alias、namespaceとmergeされた
  alias、`export type *`の名前の別名でのre-export、type-onlyのimportを名指すimport alias、aliasを通らない
  `import type`）。どれもmainのbuildでは違う出力になる。checkerのunit test 1件は、関数の引数の変更に合わせた。
- 結果（corporaの`--noEmit`の診断、tsgo 7.1.0-dev-19dadef8と比べて、`8ff6dca95`のbuild）：hono、Playwright、
  TypeScript `src/compiler`、Next.js、Effect、Vue.js、VS Codeは、既定のchecker数で全て同じbyte。zodは7回の実行の
  うち6回が37行対36行（P3-5beに記録したpartition依存のTS5115の1行）、1回が同じbyte。
- conformance（release build、`8ff6dca95`、`--workers 2`、466 s）：15,228構成、lane A 13,466、full 13,445（+2）、
  text 2、category 0、mismatch 2、emit full 13,434、emitの不一致7、未評価8、harness error 17、skip 1,720、
  `.js.map`の不一致1。P3-5bfの最後のreportと行ごとに比べて変わったのは2構成だけ：`importEquals3`と
  `typeOnlyMerge3`のerrorsがFullになった。他の構成はtierも、診断とemitのdigestも同じ。途中は11のfilter
  （`mport`、`xport`、`ypeOnly`、`lias`、`amespace`、`equire`、`odule`、`ecorator`、`solated`、`erbatim`、
  `eprecat`）で確かめた。
- `--checkers 4`の並列対照（1 checkerの同じfilterと比べる）：`mport` 1,059構成、`ypeOnly` 84構成、`lias` 276構成、
  `eclaration` 2,426構成が全て同じ。全caseの対照はlocalの負荷の方針により実行していない。
- ratchet：0 regressions。上の2行を足した（13,445→13,447行）。
- local：formatと、types・checker・emitter・compiler・conformance・harnessのclippy、test（2,811件）。workspace全体の
  testとclippyはhostedの`rust` job。
- perf（README corporaとVue.js、nice 20、main（P3-5bfのbuild、`d193abfeb`）対tsgo 7.1.0-dev、branchは
  `8ff6dca95`）：
  - 1 checkerの命令数branch÷main：`--noEmit` zod 1.000、Effect 1.000、Next.js 0.999、Playwright 0.999、Vue.js
    1.000、`bench-full` TypeScript `src/compiler` 1.000、Next.js 0.999、Vue.js 1.001。仕事の量は変わっていない。
  - `--noEmit` 3回のmedian（ms、main→branch）：hono 173→153、zod 567→589、Playwright 405→421、TypeScript
    `src/compiler` 357→355、Next.js 825→845、Effect 585→589、Vue.js 390→383、VS Code 3,885→3,796。読み込んだ
    文書数は8 corporaでmainと同じ。診断は、zodのpartition依存の1行（この回はmainが36件、branchが37件）を
    除いて同じ。peak（MB）：hono 320→303、zod 1,291→1,286、Playwright 804→806、TypeScript `src/compiler`
    285→284、Next.js 1,292→1,286、Effect 1,030→1,008、Vue.js 599→602、VS Code 5,412→5,429。
  - `tsconfig.bench-full.json` 3回：hono 162→159、zod 711→702、Playwright 551→544、TypeScript `src/compiler`
    575→559、Next.js 1,170→1,136、Effect 855→875、Vue.js 442→460。peak：hono 341→334、zod 1,517→1,506、
    Playwright 872→871、TypeScript `src/compiler` 467→469、Next.js 1,430→1,423、Effect 1,164→1,173、Vue.js
    624→623。出力は7 corporaとも、全fileがmainと同じbyte。
  - 10回のA/B：`--noEmit` Effect 567→588（最小値533→503）、zod 591→592、VS Code 3,871→3,896、Next.js 832→844
    （最小値744→804）、Vue.js 380→388。`bench-full` TypeScript `src/compiler` 548→537、Next.js 1,088→1,095、
    Playwright 529→530、Effect 836→839、Vue.js 438→443。peakは±1%以内。時間の差は±3%の中で向きが揃わず、
    命令数が同じなので、計測の誤差と判断した。劣化無し。この回は全体に前回（P3-5bf）より数%から1割ほど遅く
    読めており、mainとbranchの両方が同じだけ動いている。
  - tsgo（同じ計測の3回のmedian、ms／peak MB）：`--noEmit` hono 196／332、zod 985／1,760、Playwright 602／991、
    TypeScript `src/compiler` 377／396、Next.js 1,476／1,691、Effect 864／1,215、Vue.js 557／732、VS Code
    5,271／6,760。`bench-full` hono 216／375、zod 1,080／1,919、Playwright 779／1,335、TypeScript `src/compiler`
    669／653、Next.js 1,817／2,052、Effect 1,285／1,788、Vue.js 664／945。
  - 出力（`bench-full`、tsgoと同じbyteのfile数）：hono、zod、Playwright、TypeScript `src/compiler`、Next.js、Vue.jsは
    `.js`、`.d.ts`、両方のmapの全file。Effectは宣言493/496（mainも同じ。tsgo自身の実行ごとの違いを含む）、他は
    全て。
- 次：
  - **harnessはemitの後の診断をbaselineにする**（errorsがFullでない残りの4構成：`mutuallyRecursiveInference`、
    `recursiveMappedTypes`、`incorrectRecursiveMappedTypeConstraint`、`typeParameterWithInvalidConstraintType`）。
    このsliceの途中で、runnerの2つ目のProgramを「emitしてから診断を集める」順序にする試作をした（checkerの
    driverに、初期化の直後に1回operationを呼ぶscheduleを足し、そこでemitする）。分かったこと：
    - tsgoのharnessのProgramは1 thread・1 checker（`TestProgramIsSingleThreaded`）で、`Emit`はsource fileを
      Programの順に、fileごとにJS、宣言の順で出す。emitの中でcheckerを動かすのは主に2つ：import elisionが最初に
      呼ぶ`MarkLinkedReferencesRecursively`（fileの全nodeで`markLinkedReferences`）と、const enumのinlinerが全ての
      property／element accessに呼ぶ`GetConstantValue`（解決済みのsymbolが無ければ`checkExpressionCached`）。
      `mutuallyRecursiveInference`のTS5114が`this.a`の位置に出るのは後者による。
    - 試作では4構成とも、errorsもemitもFullになった。
    - 一方、tsc-rsのemit resolverは検査済みのProgramを前提にしている所があり、4つのfilter（`ecursive`、
      `ircular`、`num`、`eclaration`。重複を除いて2,913構成）で、emitのtierが190構成、errorsのtierが16構成
      下がった。原因は2種類：(1) import elisionが、検査のときに付く
      aliasの参照の印に頼っている（tsc 6.0の形：未検査のfileだけ`markLinkedReferences`を歩く。tsgoは常に
      歩く）。未検査のままemitすると、使われているimportが消える。(2) resolverの問い合わせの一部が、tscとtsgoの
      `getReferencedValueSymbol`（診断を出さず、cacheもしない名前解決）ではなく、診断を出す`getResolvedSymbol`を
      使っている（`getReferencedExportContainer`、`getReferencedValueDeclaration`など）。検査の後のemitでは、
      その診断は誰にも読まれないので見えなかった。emitが先だと、宣言の名前に対する「宣言の前に使われた」
      （TS2450／TS2448）などが診断に混ざり、harnessの「emitの前後で診断の数が違う」行になる。
    - 次のsliceで、(1)を7.1の`markLinkedReferences`（未検査の場所を歩かないための条件が6.0より増えている）に
      合わせ、(2)を直してから、runnerをemit-firstにする。`noEmitOnError`のProgramは、tsgoでも`Emit`が先に全ての
      診断を求めるので、順序は今のまま。`--checkers 4`の対照は今の順序（検査が先）で走るので、この4構成は
      記録する違いになる。
  - emitの不一致7、harness error 17（`runExternalCode`の15件、`deduplicatePackages`の2件）。
  - 既定のchecker数での診断の再現性（zod。P3-5beの記録の通り未決）。
- hosted：PR #684（head `59b2afb24`）、run 37333860159 — `plan` 30s、`rust` 10m54s、`conformance (TypeScript 7.1)` 19m1s、
  `gates` 11s。merge commitは`e70475159`。

## P3-5bh harnessの「emitが先」の順序と、検査の前に答えるemit resolver（2026-10-06）

errorsがFullでない残りの4構成（`mutuallyRecursiveInference`、`recursiveMappedTypes`、
`incorrectRecursiveMappedTypeConstraint`、`typeParameterWithInvalidConstraintType`）は、tsc-rsの診断がtsgoの
command lineと同じで、baselineとだけ違っていた。tsgoのtest harnessは、1つの構成を2回compileする
（`compileFilesWithHost`、testutil/harnessutil/harnessutil.go:647-712）：1つ目のProgramは診断だけを集め、2つ目は
**先に`Emit`してから**診断を集める。`.errors.txt`に書くのは2つ目の診断で、2つの数が違えば短い方に「Pre-emit (n)
and post-emit (m) diagnostic counts do not match!」の行を足す（その行を持つreferenceのbaselineは無い）。Programは
1 thread・1 checkerで（`TestProgramIsSingleThreaded`）、`Emit`はsource fileをProgramの順に、fileごとにJS、宣言の
順で出す。emitがcheckerに尋ねたことは、その順序で解決される。解決の順序に依存する診断（循環の報告の位置など）は、
emitが最初に届いた所に出る。runnerをこの順序にした：
- **runnerの2つ目のProgram**（`crates/conformance/src/ts71.rs`）：これまでは「検査してからemit」するsessionでemitの
  出力だけを取っていた。これを「emitしてから、同じcheckerで診断を集める」sessionにし、errorsのbaselineには
  その診断を使う。1つ目のProgramの診断と数が違えば、harnessの行を持つbaselineは無いので不一致にし、dumpの
  先頭に、多い方にだけある診断を書く。`noEmit`のProgramと、`noEmitOnError`のProgram（`Emit`が先に全ての診断を
  求める。`HandleNoEmitOptions`、compiler/program.go:1976-2008）は、emitの前に検査するので、1つ目の順序のまま。
  `--checkers 4`の対照も検査が先の順序で走る（下の「並列対照」）。
- **checkerのdriver**（`crates/checker/src/lib.rs`）：`DiagnosticSchedule::EagerAfterEmit`を足した。初期化した直後、
  どのsourceも検査していないsessionでoperationを1回呼び（emit）、そのstateのままeagerな検査を進め、検査の後で
  もう1回呼ぶ（宣言の診断のgetter）。入口は`check_program_with_authoritative_modules_at_emit_first_with_workers`。
- **compilerのsession**（`crates/compiler/src/lib.rs`）：`ProgramSession::emit_then_run_for_native_harness`。emitが
  先にならないProgram（上の2種類と、複数checkerのbudget）では、sessionを消費せずに返す。

emitを先にすると、tsc-rsのemit resolverが「検査済みのProgram」を前提にしている所が表に出た。tscのemitは必ず
検査の後なので、6.0からの移植ではそれで足りていた。tsgoのemitは検査の前でも動く。次を直した：
- **resolverの名前解決は診断を出さない**：`getReferencedExportContainer`、`getReferencedValueDeclaration`（と
  複数形）、`isArgumentsLocalBinding`、`getReferencedDeclarationWithCollidingName`は、tscでもtsgoでも
  `getReferencedValueSymbol`を使う（binder/referenceresolver.go:83-105）：検査が解決したsymbolがnodeにあれば
  それを返し、無ければ、診断を出さず、使用の印も付けず、cacheもしない名前解決をする。tsc-rsはこれらに
  `getResolvedSymbol`（診断を出し、結果をnodeに書く）を使っていた。検査の後では既に解決済みのnodeにしか
  当たらないので見えなかったが、検査の前では、宣言の名前（`namespace x`と後ろの`enum x`のmergeなど）を値として
  解決し、「宣言の前に使われた」（TS2450／TS2448）を出していた。
- **aliasの参照の印は、未検査のsourceでは歩いて付ける**：import elisionは、aliasの「参照された」印を読む。tsgoの
  import elisionは、最初に`MarkLinkedReferencesRecursively`でfileの全nodeを歩いて印を付ける
  （transformers/tstransforms/importelision.go:29、checker/emitresolver.go:808-830）。tsc 6.0は、検査しない
  source（`noCheck`など）でだけ歩いていたので、tsc-rsもそうだった。emitterは常にresolverに頼み、resolverは
  自分のcheckerがまだ検査していないsourceだけを歩く（検査済みなら、同じ印が検査で付いている）。歩く関数は
  7.1の`markLinkedReferences`の`Unspecified`の腕に合わせた（checker/checker.go:28654-28817）：検査が解決しない
  場所の識別子を解決すると、検査が出さない診断が出るので、`with`の中、JSXのintrinsicなtag名、meta property、
  decorateできないnodeのdecorator、宣言の無い`for`-`in`／`of`の式、enum memberや不正なcomputed name、interfaceの
  `extends`、classの2つ目以降の`extends`、初期値付きのshorthand propertyの名前は歩かない。type queryの`this`は
  解決しない（`markIdentifierAliasReferenced`）。`import x = a.b.c`の名前はproperty accessとして印を付けない
  （`isPartOfImportEqualsModuleReference`）。
- **const enumのinlinerは、未検査のsourceでは変換後の木だけを尋ねる**：tsgoのinlinerは最後のtransformerで、
  変換後の木に残ったproperty／element accessにだけ`GetConstantValue`を呼ぶ（解決済みのsymbolが無ければ
  `checkExpressionCached`。checker/services.go:869-888）。tsc-rsのinlinerは、速くするために、parseされた全ての
  accessを先に尋ねていた。検査済みのsourceでは結果を読むだけだが、未検査では、型の消去で消えるaccess
  （`implements a.B`など）まで検査してしまう。resolverに`is_source_checked`を足し、未検査のsourceでは
  tsgoと同じく、訪ねたaccessごとに尋ねる。`mutuallyRecursiveInference`のTS5114が`this.a`に出るのは、この
  問い合わせが`X`のinstantiationを最初に必要とするからである。
- **node check flagは、未検査のsourceなら求める**：`calculateNodeCheckFlagWorker`は、6.0では「Programが検査する
  source」なら何もしない（検査が付けた筈）。検査がまだ走っていないsourceでもそうなるので、decorateされたclassの
  自己参照のalias（`Foo_1`）や、async関数の`arguments`の捕捉が出力から消えていた。条件を「このcheckerが検査した
  source」にした。flagのために識別子を解決するときは、上と同じ診断を出さない名前解決を使う（検査が解決しない
  JSXのtag名などで診断を出さない）。
- **JSXのruntimeのmoduleが無いときの位置**：`getJsxNamespaceContainerForImplicitImport`は、fileで最初のJSXのtag
  （fragmentなら開きtag）に報告する（checker/jsx.go:1449-1484）。tsc 6.0は、最初に尋ねたnodeに報告していた。
  検査の順（arrow functionの本体は後回し）でも、emitが先の順（参照を歩くとき、開きtagから尋ねる）でも位置が
  変わるので、tsgoは位置を固定している。command lineの出力も変わる（下のunit test）。
- **捨てられたnodeからmodule名を集めない**：top-levelの`await`を見つけてparseし直した文の古いnodeは、arenaに
  残るがどの木にも入らない。宣言emitのmodule specifierの計算は、arenaの全nodeからdynamic importの名前を
  集めていて、古いnodeの文字列（親が無いので`import()`の中だと分からない）をfileの既定のmodeで解決しようとし、
  Programが記録していない組を求めて、sessionの「解決の欠落」を立てていた。検査の後のemitではその記録は
  読まれなかった。emitが先だと、sessionが失敗する（`dynamicImportsDeclaration`）。親の無いnodeを飛ばす。
- **context-sensitiveなsignatureのparameterは、型の問い合わせからは暗黙の`any`を報告しない**：全体の実行で1構成
  （`reverseMappedPartiallyInferableTypes`）が「emitの前後で診断の数が違う」になった。emitの参照の印付けは、
  `k.length`の左の`k`の型を求める（`markPropertyAliasReferenced`、checker/checker.go:28903-28907）。`k`は、まだ
  解決されていない呼び出し`id({ foo: { contains(k) { … } } })`の中のmethodのparameterで、その問い合わせの中から
  呼び出しが解決され、`k`には推論の途中の文脈の型（`unknown`）が付く。問い合わせ自身は、解決の後で文脈の型を
  取り直す。引数の文脈の型は`Mapped<unknown>`になっていて、`foo`のpropertyが無いので、文脈のsignatureは
  見つからない。7.1の`getTypeOfVariableOrParameterOrPropertyWorker`は、context-sensitiveなsignatureのparameterに
  ついては診断を報告しない（checker.go:16927-16929「context-sensitive ones may have their type fixed to
  something else」。報告は`assignParameterType`が、文脈の型が無いときにする）。tsc 6.0は常に報告していたので、
  ここでTS7006（暗黙の`any`）が1件増えていた。検査が先の順序では、parameterの型は呼び出しの解決の中で先に
  付くので、この経路は通らない。tsgoのcommand lineとも、emitが先のtsgo（計器を付けたbuild）とも同じ1件
  （TS18046）になった。
- conformance（このsliceの1回の全体実行。macOS、`taskpolicy -c maintenance nice -n 20`、1 worker、4,710秒）：
  `32865e697`で15,228 configuration、lane A 13,466、full 13,448、text 0、不一致1、emit full 13,434、emitの不一致7、
  harness error 17、skipped 1,720。P3-5bgの最終reportと行ごとに比べると、上がったのは狙った4構成
  （`mutuallyRecursiveInference`と`recursiveMappedTypes`が不一致→full、`incorrectRecursiveMappedTypeConstraint`と
  `typeParameterWithInvalidConstraintType`がtext→full）、下がったのは上の1構成で、他の構成はtierもdigest
  （errors、emit）も変わらない。上の修正の後、18本のfilter（`everse`、`ontextual`、`nfer`、`mplicit`、`rrow`、
  `allback`、`eneric`、`unction`、`sx`、`bject`、`estructur`、`arameter`、`verload`、`apped`、`ypeGuard`、
  `ontrolFlow`、`sync`、`enerator`。重複を除いて4,883構成）を走らせ、全体のreportと全ての欄を比べた：違うのは
  直した1構成（不一致→full）だけ。**errorsを比べる13,449構成は、全てfullになった**（lane Aの残りの17は
  harness error）。ratchetは2行を足し、2行を上げた（`--filter <case> --update`。13,449行）。全体の実行は
  修正の前のbytesのもので、修正後のheadの全件はhostedの`conformance (TypeScript 7.1)` jobが走らせる。
- test：CLI 2件（JSXのruntimeの位置。どちらもmainのbuildと違う）、sessionのtest 5件（2つ目のProgramがemitの
  最初に届いた所に報告する／emitが先でも検査が出さない診断を出さない／使われているaliasを残す／parameterに
  暗黙の`any`を報告しない／`noEmitOnError`のProgramは渡し返される）、runnerのtest 1件（4構成）。
- local（macOS。夜間のため全て`taskpolicy -c maintenance nice -n 20`、1 job、test thread 1）：`cargo fmt --all --
  --check`、6 crate（types、checker、emitter、compiler、conformance、harness）の`cargo clippy --all-targets -- -D
  warnings`、同じ6 crateの`cargo test --no-fail-fast`（41 target、2,819 passed）を`adccdbb0b`のtreeで実行した。
  workspace全体のtestとclippyはhostedの`rust` job。
- **並列対照**：1 checkerのrunnerは2つ目のProgram（emitが先）の診断をbaselineにし、`--checkers 4`の対照は
  検査が先の順序のままなので、2つのProgramで診断が違う4構成は、1 checkerと4 checkerで作りとして違う
  （[conformance-ts71](../conformance-ts71/README.md)の「並列実行の対照」に追記）。
- **この記録の時点で未実行のもの**：corpusの速度・peak memory・出力の比較、`--checkers 4`の対照の実行、corpusの
  診断のtsgoとの比較。夜間の低CPU設定（QoSのclamp）では時間の比較ができないので、mergeの前に通常の設定で
  実行し、結果をhostedの記録に書く。
- 次：
  - emitの不一致7（`typeTagOnFunctionReferencesGeneric`、`privateNameStaticMethod`、`computedPropertyNames52`の
    es2015、`objectTypesWithOptionalProperties2`、`comparisonAnonymousMappedTypes`、`comparisonReverseMappedTypes`、
    `binderBinaryExpressionStress`）とharness error 17（`runExternalCode`の15件、`deduplicatePackages`の2件）。
  - 既定のchecker数での診断の再現性（zod。P3-5beの記録の通り未決）。

## P3-5bi emitの残りの不一致：型の順序、JavaScriptの`@type`の型parameter、コメント、class fieldsの変数環境（2026-10-06）

P3-5bhの後に残ったemitの不一致7構成のうち6構成と、harness errorの2構成（`deduplicatePackages`）を直した。
class fieldsの2構成（`privateNameStaticMethod`、`computedPropertyNames52`）の原因は、tsgoのclass fields
transformerの構造にあった：置換をvisitのときに行い、変数環境（`var`と`let`の宣言先）を関数ごとに自分で開閉する。
同じ構造から出る違いは、baselineが当たっていない所にもある。class式を文・式・memberの各位置に置いたsource
10本を、target 4種（es2015、es2021、es2022、esnext）と`useDefineForClassFields`の両方でtsgoと突き合わせ、
見つかった違いを一緒に直した。
- **型の順序**（`CompareTypes`、checker/utilities.go:414-755。`crates/types/src/type_order.rs`、
  `crates/checker/src/type_order.rs`）：7.1の比較器は、tsc 6.0の`--stableTypeOrdering`の比較器から次の所が
  変わっている（`comparisonAnonymousMappedTypes`、`comparisonReverseMappedTypes`の宣言の出力）。
  - 同じ名前で別のsymbol（`N1.A`と`N2.A`）は、aliasの型引数や構造より先に、`compareSymbols`で分ける
    （`compareTypeNames`、632-649）。
  - instantiation expressionの型どうしは、symbolの最初の宣言で比べ、次に式のnodeで比べる（442-462）。その
    symbolは、元の型のsymbolの宣言を持つ（`getInstantiationExpressionType`、checker/checker.go:10893-10895。
    `crates/checker/src/operators.rs`）。`[g<string>, f<string>]`の要素の型は、`f`の宣言が先なら`f`が先になる。
  - tupleのlabelは全ての要素で比べる（`compareTupleTypes`、668-700）。これまでは1つ目のtupleのlabelだけを
    数えていたので、labelの無いtupleとlabelのあるtupleが等しくなり、型idの順（作った順）になっていた。
  - reverse mapped typeは、source、mapped type、constraintの順に比べる（497-509）。これまでは型idの順だった。
  - mapped typeのinstantiationは、composite mapperの2つ目（型引数を運ぶ方）で比べる（509-520）。1つ目は、
    instantiationごとに新しく作る型parameterへの対応で、比べても作った順にしかならない。
  - mapperの種類の番号は、tsgoの4種（unknown 0、simple 1、array 2、merged 3。checker/mapper.go:13-18）にした。
- **JavaScriptの`@type`がsignatureを与える関数の型parameter**（`typeTagOnFunctionReferencesGeneric`）：
  `/** @type {<T>(m: T) => T} */ function f(m) {}`の宣言は`declare function f<T>(m: T): T;`である。関数自身には
  型parameterのlistが無いので、`ensureTypeParams`はresolverに尋ねる（transformers/declarations/transform.go:
  2376-2384、`CreateTypeParametersOfSignatureDeclaration`、checker/emitresolver.go:962）。答えるのは
  `typeParametersToTypeParameterDeclarations`（checker/nodebuilderimpl.go:1696-1713）で、関数のsymbolなら、その
  宣言の型parameter（JavaScriptでは`@type`のsignatureのもの）を返す。これまでは`<T>`が出ていなかった。resolverの
  methodを足した（`crates/emitter/src/resolver.rs`、`crates/checker/src/emit.rs`、
  `crates/checker/src/node_builder/serialize.rs`、`crates/emitter/src/declarations/ensure.rs`）。methodのsymbolには
  答えが無いので、tsgoは`method(m: T): T;`と、宣言されていない`T`を書く。同じ出力にして、testで固定した。
- **property assignmentの値の前のコメント**（`objectTypesWithOptionalProperties2`）：`x()?: 1 // error`は、
  本体の無いmethodと、値の無いproperty `1`にparseされる。`emitPropertyAssignment`は値の始まりの位置の
  trailing commentを求めるが（printer/printer.go:4554-4558）、`emitTrailingComments`は、その位置が今の
  containerの終わりなら何も書かない（5604-5611。containerが自分で書く）。これまでは値の前にも書いていて、
  コメントが2回出ていた（`crates/emitter/src/printer.rs`）。
- **class fields：class aliasの置換**（`privateNameStaticMethod`）：tsgoは、class名をaliasに置き換える処理を
  identifierのvisitで行い、使うたびに複製を作る（`visitIdentifier`、estransforms/classfields.go:462-473）。
  private staticなmethodの呼び出し`A1.#method()`では、receiverがhelperの引数と`.call`の`this`の2か所に出る。
  helperの側だけがコメントの始まりを手放す（`createPrivateIdentifierAccessHelper`、1027-1028）ので、`this`の側の
  複製は`A1`の範囲を持ったままになり、引数の無い呼び出しの後ろのコメントが`.call(`の直後にも出る。tsc 6.0は
  印字のときに置換していて、2か所が同じnodeだった。classのaliasへの参照のときだけ、`this`の側を別の複製にした。
- **class fields：loopの中のclass式**（`computedPropertyNames52`のes2015）：`requiresBlockScopedVar`は
  「`inIterationStatement`で、今のclassがclass式」（195-201）。tsc 6.0はcheckerの`BlockScopedBindingInLoop`の印
  （loopの変数を捕まえる束縛にだけ付く）を読んでいた。7.1では、loopの中のclass式のcomputed nameとprivate nameの
  一時変数は、捕まえているかに関係なく`let`になる。classそのものの一時変数は、computed nameを持つinstance
  propertyがあるときだけ`let`（`classExpressionNeedsBlockScopedTemp`、203-218）。`inIterationStatement`は、
  `for`では本体だけ、`for`-`in`・`for`-`of`・`while`・`do`では全ての子で立ち（329-330、`visitForStatement`
  1244-1253）、関数宣言・関数式・object literalのmethodとaccessorで下り、arrow functionとclassのmemberでは
  そのまま（333-336、`visitClassElement` 416-437）。`let`の宣言先は一番内側のlexical environmentで、loopの本体か、
  loopの外なら関数の本体かsource fileである（`AddLexicalDeclaration`、printer/emitcontext.go:237-242）。関数や
  source fileでは、`var`の文の後ろ（初期化の文があればその後ろ）に`let`の文を置く（`EndVariableEnvironment`、
  117-133）。fieldをnativeに残す変換（es2022以降で`useDefineForClassFields: false`）にも同じ規則を入れた
  （`crates/emitter/src/builtins/class_fields.rs`）。
- **class fields：一時変数を宣言する関数**（`crates/emitter/src/builtins/class_fields/downlevel.rs`、
  `class_fields.rs`）：tsgoは、parameterのvisitで変数環境を開き、関数の本体のvisitで閉じる
  （`VisitParameters`／`VisitFunctionBody`、printer/emitcontext.go:799-818、943-970）。class fieldsのtransformerは、
  一部の関数でこれを自分で組み立てる。その通りにした：
  - methodとaccessorの名前は、関数の環境が開く前にvisitされる。computed nameの中の一時変数は、classを囲む
    scopeに宣言される。**mainには不具合があった**：`class C { [class A { static x = 1 }]() {} }`のes2015の出力で、
    名前の一時変数がmethodの本体の中に`var`で宣言されていた（classの定義の時点では未宣言で、strict modeでは
    ReferenceErrorになる）。
  - 関数に下ろしたprivateなmethod・accessor（`visitMethodOrAccessorDeclaration`、695-702）：本体は自分の
    変数環境を持ち、本体の一時変数はその関数の中に宣言される。parameterは本体の後で、環境の外でvisitされるので、
    parameterの初期値の一時変数はclassを囲むscopeに出て、初期値はparameterのlistに残る。これまでは本体の
    一時変数もclassを囲むscopeに出ていた。
  - initializerを受け取るconstructor（`transformConstructor`、2365-2422）：parameterは本体の環境が開く前に
    普通のvisitorでvisitされる（2382-2387）。初期値の一時変数はclassを囲むscopeに出て、初期値はparameterのlistに
    残る（tsc 6.0は本体に移していた）。initializerと本体の文は1つの環境でvisitされる（`transformConstructorBody`、
    2520-2600）ので、一時変数は1つの`var`の文になる（initializerの分が先。これまでは2文）。
  - 出力に残るstatic block：本体は普通の子としてvisitされ、変数環境を開かない（`visitClassStaticBlockDeclaration`、
    2181-2184）。中の一時変数は、囲む関数・loopの本体・source fileに宣言される（これまではblockの中）。
  - auto-accessorのcomputed nameをcacheする一時変数は、memberの順で、そのaccessorに届いたときに作る
    （`transformAutoAccessor`、839-857）。入れ子のscopeには予約しない。これまではclassの最初に作っていて、
    他のmemberの一時変数と番号がずれていた。
- **lowerしたclass式の括弧**：static memberを持つclass式は、comma式になる。tsgoの印字は、`if`・`while`・`do`・
  `switch`・`case`・`throw`・`with`の式を一番低い優先度で書く（printer/printer.go:3465、3509、3628-3638、3656、
  4471）ので、括弧は付かない。class fieldsの変換が位置を見て付けていた括弧をやめ、factoryが`switch`と`case`の
  式に付けていた括弧（tsc 6.0の`parenthesizeExpressionForDisallowedComma`）も外した。必要な括弧は、印字が
  優先度から付ける。
- **async generatorのmethod**（`crates/emitter/src/builtins/es2018.rs`）：内側のgeneratorの名前は、名前の種類に
  よらず`getGeneratedNameForNode(node.name)`（estransforms/forawait.go:803-806）。identifierなら`name_1`、
  computed name・文字列・数値の名前なら一時変数の名前（`function* _a()`）になる。これまではidentifierのときだけ
  名前を付けていた。`isSimpleParameterList`はrestの印を見ない（estransforms/async.go:952-960）ので、
  `...rest`は外側の関数に残る（これまではgeneratorに移していた）。
- **`deduplicatePackages`**（TypeScript 7のoption。tsoptions/declscompiler.go:188-195）：既定はtrueで、同じ名前と
  versionのpackageの2つ目以降のfileを、1つ目のfileにredirectする。`false`ならredirectしない
  （compiler/filesparser.go:361-368）。tsconfigのoptionとharnessの設定に足し、loaderのpackage idの表を、`false`の
  ときに使わないようにした（`crates/program/src/loader.rs`）。command lineのflagは未対応
  （tsc-rsのcommand lineは、READMEの一覧のflagだけを受ける）。
- **意図して合わせていない所**：`for (const x of <comma式>)`。tsgoは`for`-`of`の式も一番低い優先度で書くので
  （`emitForOfStatement`、printer/printer.go:3565-3590）、変換がcomma式を置くと括弧の無い
  `for (const x of _a = class {…}, _a)`になる。これはJavaScriptとして不正である（`of`の右はAssignmentExpression）。
  tsc-rsは括弧を残す。当たるbaselineは無い。
- conformance（このsliceの1回の全体実行。macOS、`taskpolicy -c maintenance nice -n 20`、1 worker、4,763秒）：
  `09c2be445`で15,228 configuration、lane A 13,466、full 13,451、text 0、不一致0、emit full 13,442、emitの不一致1、
  emit未評価8、harness error 15、skipped 1,720。P3-5bhの最終report（直した1構成を差し替えたもの）と行ごとに
  比べると、変わったのは狙った8構成だけ：emitがnone→fullの6構成と、harness error→compared（errors full・emit
  full）の`deduplicatePackages`2構成。他の構成はtierもdigest（errors、emit、map）も変わらない。ratchetは
  6行を上げ、2行を足した（`--filter <case> --update`。13,451行）。**emitの不一致は`binderBinaryExpressionStress`
  の1構成だけになった**（下）。
- probe：class式を文・式・memberの各位置に置いたsource 10本（loop、文、式、消される包み、computed name、
  async generator、private method、static block・constructor）と宣言の順序のsource 2本を、tsgoとtsc-rsで
  tsconfig経由にemitし、出力をdiffした。全てbyte一致（`for`-`of`の括弧の1点を除く）。
- test：CLI 22件（tsgoのbytes。型の順序5、anonymousの順序1、JSの`@type`1、コメント2、class fields 9、
  async generator 2、`deduplicatePackages` 2。20件はbranchの基点のbuildと違う）。`deduplicatePackages`を
  足したので、optionの一覧の順序のtest（program）を7.1の宣言順に合わせた。
- local（macOS。夜間のため全て`taskpolicy -c maintenance nice -n 20`、1 job、test thread 1）：`cargo fmt --all --
  --check`、7 crate（types、checker、emitter、compiler、conformance、harness、program）の`cargo clippy
  --all-targets -- -D warnings`を`09c2be445`のtreeで、同じ7 crateの`cargo test --no-fail-fast`（49 target、
  3,422 passed）を直前のtree（違いはoption一覧のtestの期待値とtestのコメント）で、その2 targetを`09c2be445`の
  treeで（program contracts 503、compiler contracts 307 passed）実行した。workspace全体のtestとclippyは
  hostedの`rust` job。
- **この記録の時点で未実行のもの**：corpusの速度・peak memory・出力の比較、`--checkers 4`の対照の実行、corpusの
  診断のtsgoとの比較。P3-5bhと同じく、夜間の低CPU設定では時間の比較ができないので、mergeの前に通常の設定で
  実行し、結果をhostedの記録に書く。
- 次：
  - `binderBinaryExpressionStress`（emit）：emitterは、構文木の深さが256を超えるsourceを拒む
    （`preflight_source`、`crates/emitter/src/builtins.rs`の`MAX_TRANSFORM_DEPTH`。H2期の上限）。compileの全体が
    「compiler failure」で失敗する。400項の文字列連結、400段のmethod chain、300重の配列literalでも起きる。tsgoは
    Goのstack（1 GBまで伸びる）でそのままemitする。worker threadのstackは16 MiB（`WORKER_STACK_BYTES`）、
    conformanceのcase threadは256 MiB、CLIのmain threadは8 MiB。上限を外すにはemitのstackの方針（深さに応じた
    stackのthread、または大きな仮想stackと上限の見直し）が要り、設計の判断になる。別のsliceで扱う。
  - harness error 15（`runExternalCode`。content mapperの機能）。
  - 既定のchecker数での診断の再現性（zod。P3-5beの記録の通り未決）。

## P3-5bj 構文木の深さの上限を撤廃し、compileを1 GiBの仮想stackで走らせる（2026-10-06）

ユーザー決定（2026-10-06）：「H2期の上限『深さ256を超えるsourceは拒む』はなくしてください」。

emitterは、H2期の安全策として、構文木の深さが256を超えるsourceを拒んでいた（`preflight_source`の
`MAX_TRANSFORM_DEPTH`、`TransformError::AstDepthDeferred`「deferred to H2.9」）。compile全体が「compiler failure」で
失敗し、conformanceの`binderBinaryExpressionStress`（約4,950段の`+`の連鎖）がemitの最後の不一致として残っていた。
400項の文字列連結、400段のmethod chain、300重の配列literalでも起きるので、実プロジェクトでも当たり得る。
tsgoはGoのstack（1 GBまで伸びる）でそのままcompileする。

実測（上限を環境変数で外した実験build、P3-5biのhead）：emitは16 MiBのthread stackで深さ1,000〜2,000の間で
stack overflowする（文字列連結1,800は通る＝1段約8 KiB、method chainと入れ子のobject literalは1,000で落ちる＝1段
16 KiB以上）。checkerには上限が無く、`f(f(f(…)))`2,000段はmain thread（8 MiB）でabortする。

- **stackの方針**（`crates/program/src/workers.rs`）：compilerが起こす全てのthread（worker、checker shard、emitの
  prelude、conformanceのcase thread）の予約を16 MiBから**1 GiB**にした（`WORKER_STACK_BYTES`）。予約は仮想で、
  再帰が触ったpageだけが確保されるので、普通のsourceのthreadの費用は変わらない。Goのstackの上限と同じ桁なので、
  tsgoがcompileするsourceはこの予約にも収まる。
- **commandの作業もそのthreadで**（`crates/compiler/src/bin/tsc-rs.rs`）：main threadのstack（macOS・Linuxで
  8 MiB）はprocessが選べない唯一の大きさなので、`run_cli`を`WORKER_STACK_BYTES`のthread（`tsc-rs-main`）で走らせ、
  joinして出力する。threadが拒まれたらmain threadで走る。panicはmainで再送する。
- **上限の削除**（`crates/emitter/src/builtins.rs`、`factory.rs`、`transform.rs`）：preflightは深さを見ない。
  classifierの深さの計測（`parsed_max_depth`）と`AstDepthDeferred`も、使う所が無くなったので削除した。
- **conformanceのcase thread**（`crates/conformance/src/ts71.rs`）：256 MiBの独自定数を`WORKER_STACK_BYTES`にした。
- **Rust API**：既定のserialな予算はcallerのthreadで走り、そのstackはembeddingが選ぶ。READMEのAPIの節に、深い
  sourceは`std::thread::Builder::new().stack_size(tsc_program::WORKER_STACK_BYTES)`のthreadで走らせるよう書いた。
- 結果：深さ5,000の文字列連結、2,000段の入れ子呼び出し・配列・arrow・条件式・括弧・template・block、1,000段の
  method chainと入れ子object literal、50,000項の文字列連結、20,000段のmethod chainが、どれもtsgoと同じ出力
  （JS・d.ts）と同じ診断になる。深い入力はstackのpageを深さに比例して確保する：50,000項の連結はpeak RSS
  555 MB（tsgoは199 MB。tsgoのbinderとcheckerは二項式を反復で処理する）。普通のsourceでは変わらない。
  `binderBinaryExpressionStress`はerrors・emit・mapが全てFull。**emitの不一致は0になった。**
- test：CLI 5件（5,000項の文字列連結、2,000段の入れ子呼び出し（checkerのoverflowの再現）、1,000段のmethod
  chain、2,000重の配列literal、1,000重のobject literalとその宣言。期待値はtsgoのbytes）。
- conformance（このsliceの1回の全体実行。macOS、`nice -n 20`、2 worker、523秒）：`9c4a7c8ae`で15,228 configuration、
  lane A 13,466、full 13,451、text 0、不一致0、emit full 13,443、emitの不一致0、emit未評価8、harness error 15、
  skipped 1,720。P3-5biの最終reportと行ごとに比べると、変わったのは`binderBinaryExpressionStress`だけ（emitと
  mapがnone→Full）。ratchetは1行を上げた（`--filter <case> --update`。13,451行）。**errorsを比べる13,451構成は
  全てFull、emitを比べる13,443構成も全てFullになった。** 残りはharness error 15（content mapperの
  `runExternalCode`）、emit未評価8（native runnerがJS baselineを持たない構成）、tsgo自身のskip list 42。
- local（macOS、`nice -n 20`、2 job）：`cargo fmt --all -- --check`、6 crate（emitter、compiler、conformance、program、
  checker、harness）の`cargo clippy --all-targets -- -D warnings`、4 crate（emitter、compiler、conformance、program）
  の`cargo test --no-fail-fast`（40 target、1,561 passed）を`9c4a7c8ae`のtreeで実行した。workspace全体はhostedの
  `rust` job。
- **この記録の時点で未実行のもの**：corpusの速度・peak memory・出力の比較、`--checkers 4`の対照、corpusの診断の
  tsgoとの比較。#685・#686と合わせて、積んだheadで1回実行し、hostedの記録に書く。
- 次：
  - harness error 15（`runExternalCode`。content mapperの機能）。
  - 既定のchecker数での診断の再現性（zod。P3-5beの記録の通り未決）。
  - 実プロジェクトでの完全一致と計測（DefinitelyTyped、azure-sdk-for-js、material-ui。roadmapのstep 2）。

### P3-5bh・P3-5bi・P3-5bjのhostedの記録とmerge（2026-10-06）

3つのsliceは積み重ねたbranchで進め、通常負荷の計測を1回にまとめてからmergeした。
- hosted：P3-5bh PR #685 run 37372359768（`plan`、`rust` 6m58s、`conformance (TypeScript 7.1)` 15m34s、`gates`。
  初回はhosted runnerが`conformance`と`gates`を取れず「The job was not acquired by Runner of type hosted」で
  cancelledになり、`gh run rerun --failed`で成功）。P3-5bi PR #686 run 37394229477（`rust` 10m10s、`conformance`
  12m26s、`gates`成功）。#685のbranchを`git push --delete`で消したとき、それをbaseにしていた#686をGitHubが閉じた
  （閉じたPRはbaseを変えられず、開き直せない）ので、同じhead `fab5bc989`をmain向けのPR #688として開き直した
  （run 37402238012、`rust`・`conformance`・`gates`成功）。P3-5bj PR #687 run 37400729383（`rust` 10m31s、
  `conformance` 19m15s、`gates`）と、mainへのbase変更の後の`gh pr update-branch`による`7ab2f7d6e`の
  run 37404416817（成功）。**積んだPRは、baseのbranchを消す前に`gh pr edit --base main`で切り替える。**
- merge：#685 → `f85fdd159`、#688 → `41f38ccaa`、#687 → `136d1d512`（全てmerge commit）。
- 並列対照（`--checkers 4`、`9c4a7c8ae`＝P3-5bjのhead。3 sliceの変更を含む）：`lass` 1,946・`eclaration` 2,426・
  `omputed` 361構成が一致。`apped` 140（＋2）、`nfer` 356（＋1）、`ecursive` 132（＋3）の違いは全て、P3-5bhが
  記録した「emitが先」の4構成のうちの`incorrectRecursiveMappedTypeConstraint`、`mutuallyRecursiveInference`、
  `recursiveMappedTypes`（作りとして1 checkerと4 checkerで違う）。
- corpusの診断（`--noEmit`、既定のchecker数。main `e70475159`、P3-5bi `09c2be445`、P3-5bj `4b4ff6df3`の
  3 buildをtsgoと比較）：hono、Playwright、TypeScript `src/compiler`、Next.js、Effect、Vue.js、VS Codeは3 build
  ともtsgoとbyte一致。zodはP3-5beの記録の、partitionに依存するTS5115の1行だけ違う（37 vs 36。変わらず）。
- 性能（README corpora＋Vue.js、3 round、interleaved、中央値。main / P3-5bi / P3-5bj / tsgo）：`--noEmit`は
  P3-5bi/main 0.87〜0.99、P3-5bj/main 0.90〜1.03（VS Code 3,720・3,718 vs 3,784 ms）、peak RSSは±1%以内
  （honoだけ285 MBの基数で＋7〜11%）。bench-full（emitあり）はP3-5bi/main 0.96〜1.05、P3-5bj/main 0.99〜1.07
  （中央値の揺れ。遅く見える行のminはmain以下：TypeScript compiler 519 vs 537 ms、Effect 778 vs 771 ms）。tsgoに
  対しては0.59〜0.98（`--noEmit`）、0.59〜0.87（bench-full）。10 roundのA/Bと命令数は、ユーザーが機械を使って
  いたので省略した（負荷を下げる指示。以後の計測は小さく分けて回す）。
- emit出力（7 corpusのbench-full）：main→P3-5biで変わったのはEffectの宣言1 file（tsgoのbytesに一致するように
  なった。Effectのd.tsのtsgo一致は493→494/496。残る2 fileと1つのd.ts.mapはP3-5beの記録のpartition依存の順序）。
  他の6 corpus（hono 748、zod 1,884、Playwright 2,816、TypeScript compiler 312、Next.js 6,660、Vue.js 1,760 file）
  は変わらず。P3-5bi→P3-5bjは全fileが同一（stackの予約は出力を変えない）。
- **この時点で、errorsを比べる13,451構成とemitを比べる13,443構成は全てFull。** 残りはcontent mapperの
  harness error 15、JS baselineの無い8構成、tsgoのskip list 42。

## P3-5bk 実プロジェクトの第1回：DefinitelyTypedの全projectでtsgoと同じ出力にする（2026-10-06）

roadmapのstep 2（2026-10-05）：conformanceの完全一致の後、実プロジェクトの診断をtsgoと比べ、違いを直す。
最初のcorpusはDefinitelyTyped（`fbd2f560`、`types/<pkg>/tsconfig.json`と`types/<pkg>/<version>/tsconfig.json`の
9,067 project）。各projectのtestは自分自身を`node_modules/@types/<pkg>`経由でimportするので、rootで
`pnpm install --ignore-scripts`を1回行い（ユーザー承認2026-10-06）、各projectで`-p tsconfig.json --pretty false`を
両compilerで走らせてstdoutとexit codeを比べた（`~/dev/real-projects.noindex/dt-compare.py`）。

**参照はtsgoの`--singleThreaded`**：tsgoの既定の4 checkerは、1つの宣言を2つのcheckerが検査すると
interfaceの両立性の診断（TS2320・TS2430）を2回出す（checkerごとの`interfaceChecked`の印）。これはpartitionの
artifactで、追う対象ではない。tsc-rsは1 checker（`TSRS_CHECKERS=1`）で比べた。

第1回の全体比較で53 projectの出力が違った。原因は全て、tsc-rsがtsc 6.0.3か自分の以前の作りに従っていて
tsgo 7.1が違う所だった：
- **module augmentationの名前はfileを読み込まない**（`crates/program/src/loader.rs`、`crates/compiler/src/lib.rs`）：
  tsgoはimportの名前にだけfileを足す（compiler/fileloader.go:915-923）。augmentationの対象でどのimportも
  読み込まないfileは、解決されるがprogramに入らず、checkerは「見つからない」として扱う（moduleのfileならTS2664、
  ambient contextでは何も出さない。checker.go resolveExternalModule、mergeModuleAugmentationはambientのaugmentationに
  errorを渡さない）。tsc-rsはprogramの構築で失敗していた（「resolved non-JavaScript target … has no independent
  program membership」。DefinitelyTypedの`ember`の`declare module "htmlbars-inline-precompile"`）。
- **global型の構築は、遅延したambient moduleのmergeより前**（`crates/checker/src/merge.rs`、`lib.rs`。tsgo
  initializeChecker、checker.go:1352-1382）：`Function`の宣言型を作るとき`isThislessInterface`が全ての`Function`宣言の
  基底名を解決し、importしたinterfaceを継承するglobal augmentationはそのimportを解決する。ambient moduleの宣言だけが
  与える名前のimportはこの時点では見つからず、TS2307になる（`ember/v2`）。`merge_module_augmentations_around`で
  programの経路だけがこの位置で構築し、unit testは遅延のまま。
- **内蔵libraryは`bundled:///libs/<lib>`**（`crates/compiler/src/cli.rs`、`crates/diagnostics/src/render.rs`。tsgo
  internal/bundled）：libraryのfileに出る診断はその名前で出て、URLはschemeがrootなので相対化されず、`/`で始まる全ての
  fileの後ろに並ぶ。
- **missing propertyの文は全て`getTypeNamesForErrorDisplay`で型を名付ける**（relater.go:1270-1292。
  `crates/checker/src/check.rs`、`structural.rs`、`operators.rs`）：同じ表示になる2つの型は完全限定名で書く。
  TS2739と鎖の中の文も含む（tsc-rsは1 propertyのTS2741だけだった）。
- **`this.q`を自分の条件の本体で使う**のは意図した判定（`isSymbolUsedInConditionBody`はsymbolを比べる。
  `crates/checker/src/operators.rs`）：`ThisType`で型の付くobject literalの`if (this.q) { this.q(); }`にTS2774を
  出さない（`wechat-miniprogram`）。
- **基底classのstaticは、派生classのnamespaceの値でないmemberを置き換える**（`addInheritedMembers`、
  checker.go:19935-19947。`crates/checker/src/annotate.rs`）：passportのstrategyの`export import Strategy = …`は
  基底の`Strategy`に譲る。tsc 6.0.3はJavaScriptのexpando代入だけを置き換えていた。
- **fileの`export =`の対象はfileで名付ける**（`getSpecifierForModuleSymbol`、nodebuilderimpl.go:1249-1336。
  `crates/checker/src/check.rs`）：`typeof import("…/index.d.ts")`。そのsymbolにmergeした`declare module "react"`の
  引用名では名付けない。
- **node builderの長さの見積もり**（`crates/checker/src/check.rs`）：signatureは＋3、parameterは名前の長さ＋3、
  型parameterはparameterの前に書く、再利用した型nodeは中の加算を捨てて元のspanを足す（`tryReuseExistingNodeHelper`、
  nodecopy.go:197-231）。予算に届く位置がtsgoと同じになり、省くmember（`... N more ...`）が一致する
  （`tuya-panel-kit`）。
- **診断の重複はmessage chain全体で判定する**（`EqualDiagnosticsNoRelatedInfo`、ast/diagnostic.go:405-417。
  `crates/diagnostics/src/lib.rs`）：`checkInheritedPropertiesAreIdentical`はpropertyごとにTS2320を出し、鎖だけが
  違うそれらは全て残る。tsgoにcanonical diagnosticは無いので、「Did you mean」の診断は自分のcodeと文で並び、
  重複判定される（tsc 6.0の`getCanonicalDiagnostic`相当を削除）。
- **再入した基底制約の解決はcircularの印を返し、cacheしない**（`getResolvedBaseConstraint`、
  checker.go:27912-27953。`crates/checker/src/constraints.rs`、`links.rs`）：tsgoは1つの関数と1つのcacheで
  top-levelと入れ子の解決を行い、解決中の型への要求は印を返すだけなので、その解決の間の要求は毎回循環を見つけて
  間の解決をcircularにする。tsc 6.0（`getResolvedBaseConstraint`と`getImmediateBaseConstraint`の2段）は最初の再入の
  印を`resolvedBaseConstraint`にcacheして後の要求に答えていて、tsc-rsも同じだった。`infer P`の制約を
  `DefineComponent<infer P>`から推論する間、分配的なconditional `ExtractDefaultPropTypes<P>`はsubstitution `P & object`
  の`P`を`object`と何度も比べ、tsgoはそのたびに循環を見つけてconditional型の制約を印にするので、mapped型の
  parameter `K`の制約を求めることが無い。tsc-rsは2回目以降の比較でcacheを返し、循環の中で`K`の制約を解決して
  TS2313を報告していた（`@vue/runtime-core`。`vue-writer`、`vue-draggable-resizable`、`vue3-carousel-3d`）。
  `isRelatedTo`の「型parameter＝制約」のfast path（relater.go:2668-2675）も、inlineの制約を読むのではなく
  `getConstraintOfType`で制約を解決するようにした（M4以来のKNOWN-GAP）。
- **classとinterfaceの宣言型は、型parameterを計算する前に公開する**（`getDeclaredTypeOfClassOrInterface`。
  `crates/checker/src/annotate.rs`）：継承を通って再入した読みはshell（型parameterもthisTypeも無い）を見る。tsc-rsは
  成功時にだけslotを書き、再入をassertで止めていた（「re-entrant declared-type computation must route through the
  in-progress set」）。`@types/node`のweb globalsの`interface Console extends console.Console {}`と`var console: Console`
  が同じglobal blockにあると、`console.Console`の値の探索が`Console`の型を求めて再入する。in-progressの集合は
  不要になり削除した。
- **interfaceの`extends`とclassの`implements`の要素の名前は、tsgoのparserではqualified name**
  （parser.go parseTypeHeritageClauseElement、convertEntityNameExpressionToEntityName。tsc 6.0はproperty access
  expressionのまま）：`resolveQualifiedName`のQualifiedNameを読むerror pathのうち、値を型に使ったときの`typeof`の
  提案（TS2749）と「型であってnamespaceでない」（TS2713）が、そのproperty accessの鎖に当たるようにし、
  `tryGetQualifiedNameAsValue`も鎖を辿るようにした（`crates/checker/src/resolve.rs`
  `type_heritage_qualified_name_root`）。parserはproperty accessのままにしている（emitterと宣言emitの形に影響する
  ので、ここでは必要な観測だけを合わせた）。`dockerode-compose`（`@types/node`の2 version）。
- test：CLI 13件（原因ごとに1件。DefinitelyTypedの形を縮約したもの。期待値はtsgoのbytes）、diagnosticsのunit
  test 2件、program loaderのcontract test 1件の更新（augmentationの対象は解決されるが読み込まれない）。

- conformance（このsliceの1回の全体実行。macOS、`taskpolicy -c maintenance nice -n 20`、2 worker、2,400秒）：
  `38b7240aa`のbuild（testは`1b947c265`）で15,228 configuration、lane A 13,466、full 13,451、text 0、不一致0、
  emit full 13,443、emitの不一致0、emit未評価8、harness error 15、skipped 1,720。ratchetは0 regression、
  上がった行も0。P3-5bjの最終report（`9c4a7c8ae`）と行ごとに比べて、tier・digest（errors、emit、map）の変化は
  0行：conformanceの範囲ではこのsliceは何も変えず、実プロジェクトだけが当たる違いだった。
- local（macOS、`taskpolicy -c maintenance nice -n 20`、2 job）：`cargo fmt --all -- --check`、6 crate（checker、
  compiler、diagnostics、program、conformance、harness）の`cargo clippy --all-targets -- -D warnings`、同じ6 crateの
  `cargo test --no-fail-fast`（24 target、2,813 passed）と`contracts -- cli_contract`（188 passed。最後のCLI test
  1件はその後に追加）。workspace全体はhostedの`rust` job。
- DefinitelyTyped：第1回の全体比較（修正前のbuild、tsgoは既定のchecker数）で53 projectが違い、`--singleThreaded`で
  53を再実行して49が一致、残る4（`vue-writer`、`vue-draggable-resizable`、`vue3-carousel-3d`、`dockerode-compose`）が
  上のbase constraintとqualified nameの修正で一致した。原因ごとの縮約repro 15件と、以前に違った`ember/v2`、
  `wechat-miniprogram`、`passport-github`、`tuya-panel-kit`も最終buildでtsgoとbyte一致。**9,067 project全体の
  再実行は最終buildで行い、hostedの記録に書く。**
- **この記録の時点で未実行のもの**：`--checkers 4`の対照（6 filter。実行中）、8 corpusの`--noEmit`診断のtsgoとの比較、
  corpusの速度・peak memoryの比較（ユーザーの指示で小さく分けて回す）、DefinitelyTyped全体の再実行。
- 次：
  - material-ui（`packages/*/tsconfig.json`）とazure-sdk-for-js（packageをまたぐ型が`dist/*.d.ts`なので、比較の前に
    distを作る方法をユーザーに提案する）。
  - harness error 15（`runExternalCode`。content mapperの機能）。
  - 既定のchecker数での診断の再現性（zod。P3-5beの記録の通り未決）。

### P3-5bkのhostedの記録、対照、計測、merge（2026-10-06）

- hosted：PR #689 run 37438322299（head `9459d824a`：`plan` 28s、`rust` 7m07s、`conformance (TypeScript 7.1)`
  19m17s、`gates` 14s。全て成功）。merge → `d286575ea`（merge commit）。
- conformanceの全体実行のbinaryは、`38b7240aa`になるtreeの、最後のcommentの編集（`crates/checker/src/engine.rs`の
  fast pathの説明）の前のbuild。codeは同じ。
- 並列対照（`--checkers 4`、6 filter、同じbinary）：`lass` 1,946・`eclaration` 2,426・`omputed` 361構成が一致。
  `apped` 140（＋2）、`nfer` 356（＋1）、`ecursive` 132（＋3）の違いは全て、P3-5bhが記録した「emitが先」の
  partition依存の構成（`incorrectRecursiveMappedTypeConstraint`、`recursiveMappedTypes`、`mutuallyRecursiveInference`）
  で、P3-5bjの対照と同じ結果。
- corpusの診断（`--noEmit`、既定のchecker数。`9459d824a`のtreeの最終build vs tsgo 7.1.0-dev）：hono、Playwright、
  TypeScript `src/compiler`、Next.js、Effect、Vue.js、VS Codeはbyte一致。zodはP3-5beの記録の、partitionに依存する
  TS5115の1行だけ違う（37 vs 36。変わらず）。
- 性能（ユーザーの指示で小さく分けて計測：corpusごとに1回ずつ、3 round、interleaved、`nice -n 20`。main＝
  `136d1d512`のbuild）：`--noEmit`のwallはこのslice/main 0.925〜1.002（hono 123 vs 133 ms、zod 484 vs 490、
  Playwright 341 vs 346、TypeScript compiler 312 vs 313、Next.js 702 vs 720、Effect 491 vs 497、Vue.js 318 vs 317、
  VS Code 3,305 vs 3,310）、peak RSS 0.963〜0.998。bench-full（emitあり）は0.969〜1.037（honoの149 vs 143 msは
  10 roundのA/Bで144 vs 145＝0.994。zod 588 vs 598、Playwright 455 vs 464、TypeScript compiler 484 vs 484、
  Next.js 962 vs 953、Effect 713 vs 735、Vue.js 386 vs 388）、peak RSS 0.955〜0.998。tsgoに対してはwall
  0.57〜0.93（`--noEmit`）、0.61〜0.78（bench-full）、peak memory 0.66〜0.91。劣化なし。
- **DefinitelyTyped 9,067 projectの最終buildでの再実行（`dt-compare.py`、tsgo `--singleThreaded`／tsc-rs
  `TSRS_CHECKERS=1`、`taskpolicy -c maintenance nice -n 20`、2 job、3,483秒）：9,067 project全てでstdoutとexit codeが
  一致した**（エラーのあるprojectは279、診断行は4,370。両compilerが同じ行を出す）。同じ条件で測った壁時計の合計は
  tsgo 4,023秒、tsc-rs 2,939秒（projectの中央値 534 ms vs 362 ms。低優先度クラスで2 job同時なので、READMEの
  計測とは比べない）。

## P3-5bl material-uiの第1回で見えた2つの性能欠陥：構文エラーで検査を省くこと、planのUTF-16変換（2026-10-06）

material-uiの38 tsconfig project（`packages/*`、`packages-internal/*`、`docs`、`test`、`examples/*`、root）をDefinitelyTypedと
同じ手順で比べた（`compare-projects.py`、tsgo `--singleThreaded`／tsc-rs `TSRS_CHECKERS=1`）。26 projectが一致し、違った
12 projectは全て未対応のoption（`incremental` 5、project references 5、`composite` 2。tsc-rsはprogramの構築で拒み、
tsgoは検査する）。診断の違いは無い。ただしroot `tsconfig.json`（32,302 file、`packages/mui-icons-material/templateSvgIcon.js`に
構文エラー5件）は同じ出力に54.8秒・peak 4.5 GBかかった（tsgo 2.0秒・1.3 GB。`nice -n 20`、1 checker vs `--singleThreaded`）。
原因は2つ。
- **構文エラーがあれば検査しない**（`crates/checker/src/lib.rs`、`crates/compiler/src/lib.rs`）：tscの`emitFilesAndReportErrors`
  （tsgo execute.go `compileAndEmit`）は、configと構文の診断が空のときだけoptions・global・semanticの診断を求める。構文エラーの
  あるProgramはbindもcheckもされない。tsc-rsは全てを検査してから報告の段で選んでいた（出力は同じ、36秒の検査は無駄）。
  commandのnoEmitの検査に`SyntacticDiagnosticsGate::CloseTheCheck`（schedule `EagerUnlessSyntacticDiagnostics`）を入れ、
  parse済みのsourceの構文の行をbindの前に読んで、あれば構文の行だけの結果を返す。native harness（conformance）は全部を
  集めるので`CheckEverySource`のまま。emitのある実行はtsgoもemitのためにfileごとに検査するので変えていない。
- **module requestのplanのUTF-16変換**（`crates/program/src/module_requests.rs`）：occurrenceごとにtextの先頭からUTF-16単位を
  数えていて、fileの長さ×occurrence数の二乗になっていた。`@mui/icons-material/lib/index.js`（2.4 MB、`require`約10,000）の
  planに13秒。snapshotの位置indexで直接求めるようにした。
- 結果：root projectは54.8秒→1.88秒（tsgo 1.97秒）、peak RSS 4.48 GB→1.53 GB（tsgo 1.28 GB）、出力は同一。単独の
  `index.js`は17.0秒→4.1秒（tsgo 5.4秒）。
- test：`program_session_contract`の作業量counterのtest 1件を新しい挙動に更新し（構文エラーのあるProgramはlibrary prefixだけ
  bindする）、構文エラーと型エラーが別fileにあるとき構文エラーだけが報告されbindが起きないこと、native harnessの経路は
  両方を集めることを固定するtest 1件を追加した。

- conformance（このsliceの1回の全体実行。macOS、`nice -n 20`、2 worker、444秒）：`ee0e6a073`のbuild（testは`aaae738da`）で
  15,228 configuration、full 13,451、不一致0、emit full 13,443、harness error 15、ratchet 0 regression。P3-5bkのreportと
  行ごとに同一（harnessの経路は変えていない）。
- local（`nice -n 20`、2 job）：`cargo fmt --all -- --check`、5 crate（checker、compiler、program、conformance、harness）の
  `cargo clippy --all-targets -- -D warnings`と`cargo test --no-fail-fast`（2,763 passed）。
- 次：material-uiの残り（`incremental`・`composite`・project references）はroadmapの「未対応のoption」の段階。
  azure-sdk-for-js（467 project）は、packageをまたぐ型が`dist/*/index.d.ts`（package.jsonの`exports`）なので、比較の前に
  distを作る必要がある。ユーザーに方法を提案する：(a) tsgoで依存順にdistの宣言をemitする、(b) `pnpm build`（turbo、時間と
  失敗の可能性が大きい）、(c) `@azure/*`に依存しないpackageだけ先に比べる。

### P3-5blのhostedの記録、material-uiの再実行、merge（2026-10-06）

- hosted：PR #690 run 37456149036（head `ab991836e`：`plan` 28s、`rust` 8m42s、`conformance (TypeScript 7.1)`
  19m28s、`gates` 13s。全て成功）。merge → `0f7955f3a`（merge commit）。
- material-uiの38 projectの再実行（`compare-projects.py`、tsgo `--singleThreaded`／tsc-rs `TSRS_CHECKERS=1`、
  `nice -n 20`、1 job）：第1回はconfigのまま（emitあり）で573秒、26 projectが一致。第2回は両compilerに`--noEmit`を
  足して234秒、25 projectが一致。違いの12 projectは未対応のoption（`incremental` 5、project references 5、
  `composite` 2）で変わらず、残る1件が新しい診断の違い：`scripts/buildLlmsDocs/tsconfig.json`（両方36 error）の
  TS6059の「The file is in the program because:」に、tsgoは`Imported via './utils/createTypeScriptProject' from file
  '…/packages-internal/api-docs-builder/src/index.ts'`を2行（値のre-exportと型のre-export）、tsc-rsは1行。
  第1回でこのprojectが一致していたのは、emitの有無で出力が違うため（第1回はtsgo 2 1／rs 2 1）。原因と修正はP3-5bm。
- 時間（第2回、2 compilerを順に、他の処理と同時）：root `tsconfig.json`はtsc-rs 9.3秒 vs tsgo 14.8秒（第1回の336秒は
  P3-5blの前のbuild）、`test/tsconfig.json` 7.4 vs 9.3秒。小さいprojectで遅く見えた2件（`scripts/buildLlmsDocs` 3.3 vs
  3.4秒、`examples/material-ui-remix-ts` 0.79 vs 0.14秒）はP3-5bmで機械が空いているときに再計測した：前者は3回で0.28
  vs 0.35秒（tsc-rsが速い）、後者は本当に遅く（0.08 vs 0.02秒）、原因（options診断があるときtsgoはfileを検査しない）と
  修正はP3-5bmに記す。
- **corpusを汚さない**：第1回はconfigのまま実行したので、emitするprojectの`.js`出力と、tsgoの`incremental`が書く
  `tsconfig.tsbuildinfo`がcloneに残った。`git status --short --ignored | grep -v node_modules`で列挙して消した。
  `compare-projects.py`は既定で`--noEmit`を足すようにしたが、`incremental`のprojectではtsgoが`tsconfig.tsbuildinfo`
  （`tsBuildInfoFile`の指定があればその場所、`packages-internal/scripts/build/`）を書くので、実行のたびに同じ列挙で
  確認して消す。

## P3-5bm material-uiの残り：importの出現ごとの「Imported via」と、options診断があるときの検査（2026-10-06）

P3-5blのbuildでのmaterial-uiの再実行（両compilerに`--noEmit`）に残った、診断の違い1件と遅いproject 1件。どちらも
tsgoの挙動をcommandが持っていなかったもので、修正した。
- **fileはimportの出現ごとに説明される**（`crates/program/src/module_requests.rs`、`loader.rs`）：tsgoの
  `processImportedModules`はimportの出現ごとに解決したfileをprogramに加える（fileloader.go:928-940。同じfileへの
  2つ目のsubtaskは読み込まれずreasonだけ足す）ので、「The file is in the program because:」にはspecifierの出現ごとに
  `Imported via`が1行並ぶ。planはoccurrenceを解決keyで重複排除して最初のspanだけ持っていた。
  `scripts/buildLlmsDocs/tsconfig.json`（両方36 error）では`packages-internal/api-docs-builder/src/index.ts`が
  `./utils/createTypeScriptProject`から値と型を別の文でre-exportしていて、tsgoは2行、tsc-rsは1行だった。planは
  sourceを読み込む出現（augmentationの名前と、TypeScript fileのJSDocのimport typeは読み込まない）のspanを全て持ち、
  loaderはkeyごとに1回解決して、出現ごとに`visit_source`してinclusion reasonを1つずつ積む（辺は1回）。1つの
  specifierを3回importしたfileのTS6059はtsgoのbyteになり、projectは一致した。
- **options診断があるときはsourceを検査しない**（`crates/checker/src/lib.rs`、`crates/compiler/src/lib.rs`、
  `cli.rs`）：tscの`emitFilesAndReportErrors`（tsgo compiler/program.go `GetDiagnosticsOfAnyProgram`）は、optionsと
  globalの診断が空のときだけfileのsemantic診断を求める。`examples/material-ui-remix-ts`は削除されたoption
  （`moduleResolution: node`、TS5108）を持ち、tsgoはfileをbindしてglobal診断のためにcheckerを初期化するだけで
  sourceを検査せず20 ms。tsc-rsは全部を検査して報告の段で捨てていて80 ms（大きなprojectにoptionのエラーがあれば
  検査全体が無駄になる）。options行は検査の前に分かる：Program自身のもの（`available_options_diagnostics`、1回だけ
  計算するようにした）と、CLIが自分で報告するconfig planの非致命の行（`ProgramSession::with_command_options_diagnostics`）。
  どちらかがあればsessionは`SyntacticDiagnosticsGate::GlobalDiagnosticsOnly`を渡す。これは構文診断がbindの前に閉じる
  点は同じで、そうでなければcheckerを1つ初期化してsourceを検査しないschedule
  （`DiagnosticSchedule::GlobalDiagnosticsUnlessSyntacticDiagnostics`。serial driverの`check_sources` false。tsgoの
  checkerが全て出すglobal行は、初期化したchecker 1つが出す）。native harnessは全sourceを検査したまま。lib-cacheの
  harness経路にも同じgateを通し、cacheありのsessionがcache無しと同じ報告をするようにした。exampleは30 msで出力は同一。
- material-ui（このbuild、`compare-projects.py`、tsgo `--singleThreaded`／`TSRS_CHECKERS=1`、`--noEmit`、1 job、
  46秒）：38 projectのうち26が一致、違う12は未対応のoptionだけ（`incremental` 5、project references 5、`composite` 2）。
  `test/tsconfig.json`はこの条件で1.43 vs 2.31秒、両compilerの既定の並列では0.96 vs 0.96〜1.11秒、
  `--singleThreaded`／1 checkerの単発では1.35 vs 1.59秒。実行後にtsgoの`incremental`が書いた`.tsbuildinfo` 6件
  （`tsBuildInfoFile`の`packages-internal/scripts/build/`を含む）を消し、cloneは0件。
- test：specifierを2回importしたplanのspan（とspanの無いaugmentation名）の契約、1つのspecifierを3回importしたfileの
  TS6059をtsgoのbyteで固定するcommandの契約、削除されたoptionと型エラーのcommandの契約（tsgoのbyte。phase traceが
  bindと閉じた検査を示す）とsessionの契約。TS5108を持つprogramの検査を`run`で観測していたsessionの契約7件は、ungated
  なnative harnessで観測するようにした（commandと`run`はそのprogramのsourceを検査しないので、authoritativeな表は参照
  されず、suggestionも作られない）。
- 残る制限：合成import（`importHelpers`の`tslib`、JSX runtime）はinclusion reasonを持たない（tsgoは「to import
  'importHelpers' as specified in compilerOptions」「to import 'jsx' and 'jsxs' factory functions」で説明する）。
  合成かつ書かれたspecifierは書かれた出現だけを並べる（以前と同じ）。
- conformance（このsliceの1回の全体実行。macOS、`nice -n 20`、2 worker）：`985edadd1`のbuildで15,228 configuration、full 13,451、不一致0、emit full 13,443、harness error 15、ratchet 0 regression（474秒）。P3-5blのreportと行ごとに同一（import理由だけのbuildの全体実行も同じ結果）。
  file inclusionの説明を含む16 caseの`--filter`実行は全てFull。
- local（`nice -n 20`、2 job）：`cargo fmt --all -- --check`、5 crate（checker、program、compiler、conformance、
  harness）の`cargo clippy --all-targets -- -D warnings`と`cargo test --no-fail-fast`（2,767 passed）。

### P3-5bmのhostedの記録、計測、merge（2026-10-06）

- hosted：PR #691 run 37466148849（head `e82c06b8b`：`plan` 30s、`rust` 6m54s、`conformance (TypeScript 7.1)` 12m03s、
  `gates` 15s。全て成功）。merge → `6c515bad9`（merge commit）。
- corpusの診断（`--noEmit`、既定のchecker数。`985edadd1`のbuild vs tsgo 7.1.0-dev）：hono、zod、Playwright、
  TypeScript `src/compiler`、Next.js、Effect、Vue.js、VS Codeの8つ全てがbyte一致（zodのpartition依存のTS5115の
  1行はこの実行では一致した）。
- 性能（P3-5blとP3-5bmの両方を含む。分割して計測：corpusごとに1回ずつ、3 round、interleaved、`nice -n 20`。
  main＝P3-5blの前の`d286575ea`のbuild）：`--noEmit`のwallはこのbuild/main 0.944〜1.032（hono 132 vs 129 ms、
  zod 509 vs 539、Playwright 372 vs 360、TypeScript compiler 324 vs 333、Next.js 724 vs 741、Effect 521 vs 526、
  Vue.js 342 vs 356、VS Code 3,512 vs 3,663）、peak RSS 0.885〜1.005。bench-full（emitあり）は0.957〜1.043
  （hono 151 vs 146 ms、zod 670 vs 669、Playwright 507 vs 486、TypeScript compiler 581 vs 569、Next.js 1,033 vs
  1,052、Effect 766 vs 800、Vue.js 412 vs 412）、peak RSS 0.983〜1.011。Playwrightの＋3〜4%は10 roundのA/B
  （`--noEmit`）で432 vs 439 ms＝0.984、最小値は390 vs 390で同じ：noise。tsgoに対してはwall 0.57〜0.93
  （`--noEmit`）、0.61〜0.82（bench-full）、peak memory 0.64〜0.87。劣化なし。
- bench corpusとmaterial-uiのcloneに、この実行が残したfileは無い（bench configと以前からの`out/`だけ）。

## P3-5bn azure-sdk-for-jsの第1回：redirectされたpackageの命名、packageIdの説明、peerDependencies（2026-10-06）

azure-sdk-for-js（467 workspace package）は、packageをまたぐ型をpackage.jsonの`exports`が指す`dist/<target>/index.d.ts`
で参照するので、比較の前にtsgoで依存順にdistを作った（ユーザーの決定(a)。`~/dev/real-projects.noindex/az-build-dist.py`：
warpのtargetごとに`-p`を1回、repo rootの`tsconfig.src.*.json`はwarpと同じくpackage rootに置いた仮想configで`extends`
して`${configDir}`をpackageに向ける。1,465 target、199秒。distはgitignoreされた本来のビルド出力先）。次に各packageの
`tsconfig.json`の`references`が指す2,756のleaf configを両compilerで実行した（`compare-projects.py`、`--noEmit`、tsgo
`--singleThreaded`／`TSRS_CHECKERS=1`、1 job、250 configずつ12回、`az-results-*.jsonl`）：**2,671が一致**。違う85の
うち78は古いlayoutの`tsconfig.test.json`がproject referencesを使うもの（未対応、roadmapの次の段階）、残る7件が
tsgoの挙動の欠落3つで、修正した。
- **redirectされたpackageは全てのコピーを通して命名される**（`crates/emitter/src/host.rs`、
  `declarations/tracker.rs`、`crates/compiler/src/lib.rs`）：宣言emitのmodule specifierは、programがそのfileに
  redirectしたpackageのコピーを、そのfileの別名として候補に加える（tsgo `redirectTargetsMap`、filesparser.go:462、
  `forEachFileNameOfModule`が消費）。pnpmはworkspace packageをstoreの同じversionのコピーと並べてinstallする：store
  のコピーが別の依存（`@azure/identity`）経由で先に読まれ、workspaceのlinkはそれにredirectし、portableなspecifierは
  linkだけが与える。portの`forEachFileNameOfModule`はhostにredirect targetsを尋ねていたが、全てのhostが空を返して
  いた：6 configで`@azure/logger`にTS2883。`EmitHost::redirect_targets`がprepared programの`package_redirect_paths`
  を返す。
- **inclusion reasonは解決のpackage identityを持ち、説明は「… with packageId '…'」になる**
  （`crates/program/src/loader.rs`。fileInclude.go computeReferenceFileDiagnostic：import、type reference、
  automatic type directive。`types`が`*`を含むときは「implicit type library」の文言）。packageの`imports`で解決した
  fileがそれ無しで説明されていた（`@azure/ai-projects`のTS6307）。
- **package identityはpeer dependencyのsuffixを含む**（`crates/program/src/module_resolution.rs`、`resolution.rs`。
  tsgo readPackageJsonPeerDependencies）：`peerDependencies` object（値は全てstring）の名前をsort順に、packageの
  directoryのreal pathの最も近い`node_modules`にpackage.jsonがあるものだけ`+name@version`を足す。同じversionの
  2つのコピーでもpeerが違えば別のpackage。identityの文字列（`PackageId::display_text`）はtsgoのPackageId.String
  （azureの説明では`openai/core/resource.d.mts@6.49.0+ws@8.22.0+zod@4.6.5`）。peerは`load_package`の中でhostから
  直接読む（packageとしては読まないので、peerの循環で再帰しない）。
- test：pnpmのlayout（workspaceのlinkとstoreのコピー）をsymlinkで作りtsgoの空出力で固定するcommandの契約、
  packageの`imports`で解決したfileのTS6307の「with packageId」をtsgoのbyteで固定するcommandの契約、peer suffixの
  resolverの契約（sort順、無いpeer、versionの無いpeer、無効なfield）。
- 残る制限：`--explainFiles`は未実装（inclusion reasonはfileを説明する診断でだけ観測できる）。distの生成は
  warpの`commonjs` targetの仮想`{"type":"commonjs"}` package.jsonを再現しない（emitされるJSの形だけが違い、比較が
  読む宣言は同じ）。合成importのinclusion reasonはP3-5bmのまま。
- conformance（このsliceの1回の全体実行。macOS、`nice -n 20`、2 worker）：`a540e7bda`のbuildで15,228 configuration、full 13,451、不一致0、emit full 13,443、harness error 15、ratchet 0 regression（462秒）。P3-5bmのreportと行ごとに同一。
  `--filter`のduplicatePackage（11構成）、symlinkedWorkspace（4）、packageId（1）、peerDep（1）、
  moduleResolutionWithSymlinks（5）、typeReferenceDirectives（11）は全てFull。
- local（`nice -n 20`、2 job）：`cargo fmt --all -- --check`、6 crate（emitter、program、compiler、checker、
  conformance、harness）の`cargo clippy --all-targets -- -D warnings`、4 crate（emitter、program、compiler、checker）
  の`cargo test --no-fail-fast`（3,359 passed）。
- azure-sdk-for-js（最終build）：2,756 leaf configのうち2,678が一致（12回に分けて実行、両compilerの壁時計の合計は
  tsgo 278秒／tsc-rs 219秒）。違う78は全てproject references（`tsconfig.test.json`の古いlayout）。tsgoは
  `composite`のconfigでは`--noEmit`でも`*.tsbuildinfo`を書く（`tsconfig.lib.json`の`composite: true`。338件）ので、
  実行後に列挙して消した。cloneに残るのはdistだけ。
- DefinitelyTyped（package identityがpeerを含むようになったので再実行）：9,067 project全てで一致（`dt-compare.py`、
  1 job、`nice -n 20`、1,184秒。壁時計の合計はtsgo 700秒／tsc-rs 483秒。エラーのあるprojectは279で変わらず）。

### P3-5bnのhostedの記録、計測、merge（2026-10-06）

- hosted：PR #692 run 37480249272（head `74020aba9`：`plan` 30s、`rust` 7m05s、`conformance (TypeScript 7.1)` 19m31s、
  `gates` 13s。全て成功。code+testのhead `a540e7bda`のrun 37475426412も全て成功）。merge → `3365d0e7b`（merge commit）。
- corpusの診断（`--noEmit`、既定のchecker数。`a540e7bda`のbuild vs tsgo 7.1.0-dev）：hono、Playwright、TypeScript
  `src/compiler`、Next.js、Effect、Vue.js、VS Codeはbyte一致。zodはP3-5beの記録の、partitionに依存するTS5115の1行だけ
  違う（37 vs 36）。
- 性能（分割して計測：corpusごとに1回ずつ、3 round、interleaved、`nice -n 20`。main＝P3-5bmの`985edadd1`のbuild）：
  `--noEmit`のwallはこのbuild/main 0.950〜1.020（hono 137 vs 138 ms、zod 513 vs 517、Playwright 370 vs 363、
  TypeScript compiler 326 vs 324、Next.js 741 vs 781、Effect 509 vs 505、Vue.js 342 vs 336、VS Code 3,444 vs
  3,588）、peak RSS 0.995〜1.019。bench-full（emitあり）は0.944〜1.059（hono 149 vs 152 ms、zod 616 vs 610、
  Playwright 522 vs 493、TypeScript compiler 492 vs 501、Next.js 1,003 vs 1,062、Effect 745 vs 736、Vue.js 393 vs
  388）、peak RSS 0.994〜1.038。Playwright/fullとTypeScript compiler/fullの10 roundのA/Bは0.997／1.011（wall）、
  0.998／0.984（RSS）：noise。tsgoに対してはwall 0.58〜0.92（`--noEmit`）、0.61〜0.78（bench-full）、peak memory
  0.64〜0.93。劣化なし。
- cloneの状態：azure-sdk-for-jsにはdist（467 package）だけが残る（ユーザーが(a)で許可）。DefinitelyTypedとmaterial-ui、
  bench corpusにこの実行が残したfileは無い。

## P3-6a project referencesの第1段：`-p`で参照先の出力を読む（2026-10-07）

実projectの段階（P3-5bk〜5bn）が終わった時点で残っていた違いは全て未対応のoptionで、ユーザーは次の順序を承認した：
project references（`composite`／`incremental`込み）→ `--explainFiles` → content mapper／LSP。このsliceはその第1段、
`tsc -p` での project references（tsgo compiler/projectreferenceparser.go、projectreferencefilemapper.go、
tsoptions ParseInputOutputNames、program.go verifyProjectReferences、checker.go resolveExternalModule のTS6305）。
`tsc -b` と tsbuildinfo の読み書き（execute/build、execute/incremental）は次のslice。
- **参照先のconfigを全て読む**（新しい`crates/program/src/project_references.rs`）：rootの`references`から到達できる
  configを（`ResolveConfigFileNameOfProjectReference`：`.json`でなければ`<path>/tsconfig.json`）1回ずつparseし
  （`parse_config_root_plan_with_cache`、循環は既読で止まる）、root以外の各projectの入力fileを出力の宣言fileに
  対応づける（`.d.ts`とJSONは出力無し。`output_declaration_file_name`：`declarationDir`／`outDir`の下に、projectの
  common source directory（`rootDir`、無ければconfigのdirectory）からの相対path。tsgo 7の
  `getOutputPathWithoutChangingExtension`は相対pathで`..`を許す）。親→子の順で後のprojectが上書きする（tsgo
  initMapperWorker）。`ResolvedProjectReferences`は`ProgramOptions::with_project_references`でloaderに渡す
  （command 1回につき1つ、同一性で等しい）。`build_info_file_name`（tsgo GetBuildInfoFileName）も計算する。
- **参照先のsourceは、その出力として読まれる**（`crates/program/src/loader.rs`）：commandはproject referenceの
  sourceを使わない（`UseSourceOfProjectReference`はlanguage serviceだけ）ので、`visit_source`が参照先projectの
  source（出力のあるもの）に来たら、同じinclusion reasonで出力の`.d.ts`を読む（tsgo parseTask.redirect。
  `VisitState::ProjectReferenceRedirect`）。解決の`resolved_file`はsourceのまま、targetは出力のsource
  （`PreparedSourceFile::project_reference_source_paths`にsourceを登録してbuilderの検証が通る）。出力が無ければ
  何も読まず、解決は`UnloadedModuleReason::ProjectReferenceOutputNotBuilt`になり、checkerが`Cannot find module`
  ではなく**TS6305**「Output file '…' has not been built from source file '…'」を出す（compilerが
  `AuthoritativeNotFoundModule::project_reference_output`で出力名とsource名を渡す）。参照先の出力のmodule名は
  そのprojectのoptionで解決する（tsgo GetCompilerOptionsWithRedirect。projectごとのresolver、directory cacheは
  共有しない。read-aheadは参照先のsourceと出力の解決を先読みしない）。参照先のsourceは`allowJs`の判定でも
  JavaScriptとして扱わない（fileloader.go:911）。
- **referencesの検証**（tsgo verifyProjectReferences）：configの無い参照はTS6053（書かれたpathを絶対化したもの）、
  参照元にfileがあるとき`composite`でなければTS6306、`noEmit`ならTS6310、参照先と同じtsbuildinfoを書くならTS5056。
  それぞれ参照元configの`references`要素の位置（`ProgramConfigFile::project_reference_location`）。参照先projectの
  referencesも同じく検証し、参照先configはauxiliary fileとしてprogramに入れて診断の位置を描ける。`files: []`で
  referencesがあるconfigはTS18002にならない（既存）。
- **config**：`references`の拒否を外した。`incremental`、`tsBuildInfoFile`、`assumeChangesOnlyAffectDirectDependencies`
  はno-emitのcommandが受け入れる（tsgoはincremental projectも同じように検査する。**tsbuildinfoはまだ書かない**：
  次のslice）。
- test：`project_references_contract`（参照の解決、source→出力の対応、nested project、`declarationDir`、
  出力の有無による解決の形、参照先configのauxiliary file）、`config_program_loader_contract`の2件を新しい挙動に
  （referencesはloadされTS6053、incrementalは受け入れ）、commandの契約3件をtsgoのbyteで固定（参照先の出力で検査、
  未buildのTS6305、TS6053/6306/6310と`files: []`）。
- 実project（このbuild、`compare-projects.py`、`--noEmit`、1 job）：azure-sdk-for-jsは2,756 leaf config全てが一致（P3-5bnの残り78のproject references configを含む。壁時計の合計はtsgo 306秒／tsc-rs 234秒。tsgoが書いた`*.tsbuildinfo`は実行後に消した）。material-ui：38 project全てが一致（残っていた12のreferences／`incremental`／`composite`のprojectを含む。27秒／20秒）。実projectの段階の残差はこれで無くなった（DefinitelyTyped 9,067、material-ui 38、azure-sdk-for-js 2,756）。
- 参照先の出力のimplied module formatとmodule requestの計画もそのprojectのoptionで行う（tsgo
  getCompilerOptionsForFile）。
- 残る制限：tsbuildinfoを書かない（`incremental`／`composite`のprojectで、tsgoは`--noEmit`でも書く）。`tsc -b`
  （build mode）は未対応。`--listFiles`は未対応。
- conformance（このsliceの1回の全体実行。macOS、`nice -n 20`、2 worker）：`f75e2d799`のtreeのbuild（codeは`4a0810f5a`）で15,228 configuration、full 13,451、不一致0、emit full 13,443、harness error 15、ratchet 0 regression（484秒）。P3-5bnのreportと行ごとに同一。`--filter`のcomposite（3構成）、incremental（4）、tsconfig（5）、reference（21）、outDir（1）は全てFull。
- local（`nice -n 20`、2 job）：`cargo fmt --all -- --check`、5 crate（program、compiler、checker、conformance、
  harness）の`cargo clippy --all-targets -- -D warnings`と`cargo test --no-fail-fast`（2,776 passed）。

### P3-6aのhostedの記録、計測、merge（2026-10-07）

- hosted：PR #693 run 37496993797（head `78fb61f93`：`plan` 28s、`rust` 10m40s、`conformance (TypeScript 7.1)`
  19m51s、`gates` 13s。全て成功）。最初のrun 37495842161（head `54711e4db`）は`rust`の`cargo fmt --check`で
  落ちた（最後のfmtの後に加えた編集）。`78fb61f93`はその整形だけ。merge → `4c8efd09e`（merge commit）。
- corpusの診断（`--noEmit`、既定のchecker数。`4a0810f5a`のbuild vs tsgo 7.1.0-dev）：hono、Playwright、TypeScript `src/compiler`、Next.js、Effect、Vue.js、VS Codeはbyte一致。zodはP3-5beの記録の、partitionに依存するTS5115の1行だけ違う（37 vs 36）。
- 性能（分割して計測：corpusごとに1回ずつ、3 round、interleaved、`nice -n 20`。main＝P3-5bnの`a540e7bda`のbuild）：
  `--noEmit`のwallはこのbuild/main 0.929〜1.000（hono 133 vs 143 ms、zod 484 vs 486、Playwright 341 vs 351、TypeScript compiler 313 vs 313、Next.js 692 vs 726、Effect 489 vs 504、Vue.js 316 vs 333、VS Code 3,242 vs 3,455）、peak RSS 0.993〜1.006。bench-full（emitあり）は0.977〜1.013（hono 140 vs 142 ms、zod 584 vs 598、Playwright 452 vs 460、TypeScript compiler 474 vs 474、Next.js 935 vs 951、Effect 710 vs 711、Vue.js 387 vs 382）、peak RSS 0.982〜1.022。tsgoに対してはwall 0.58〜0.91（`--noEmit`）、0.61〜0.79（bench-full）、peak memory 0.63〜0.94。劣化なし（referencesの無いprogramでは参照の表が無く、追加の検索は無い）。
- cloneの状態：azure-sdk-for-jsにはdistだけ、DefinitelyTypedにはpnpm-lock.yamlだけ（どちらも以前から）。material-uiと
  bench corpusにこの実行が残したfileは無い。

## P3-6b incrementalの第1段：tsbuildinfoを書く（2026-10-07）

P3-6aで`-p`のproject referencesが動いた後の、承認された順序の続き。tsgoは`incremental`か`composite`のprogramを
incremental program（execute/incremental）として compile し、`--noEmit`でも`.tsbuildinfo`を書く。このsliceは
**初回build（古いstateが無い）**のその書き出しを byte で一致させる。古いtsbuildinfoの読み込みと再利用（変更file、
signature、診断の再利用）は次のslice（P3-6c）。

- **文書の形**（新しい`crates/incremental`、tsgo buildInfo.go／snapshottobuildinfo.go）：`version` "7.1.0-dev"、
  `root`（連続するfile idの範囲）、`fileNames`（buildinfoのdirectoryからの相対path、default libは素の名前）、
  `fileInfos`（text の XXH3-128、`signature`、`affectsGlobalScope`、`impliedNodeFormat`。signature==versionで
  CommonJSかつglobalでないfileは文字列だけ）、`fileIdsList`／`referencedMap`、`options`（`AffectsBuildInfo`の
  67 optionをtsgoのstruct順で。file pathのoptionは相対、tristateのfalseも書く）、`semanticDiagnosticsPerFile`
  （checkしなかったfileはidだけ、cacheした行は`messageKey`／`messageArgs`／byte offsetの`pos`／`end`／
  `category`／chain／relatedInformation／`skippedOnNoEmit`）、`emitDiagnosticsPerFile`、
  `affectedFilesPendingEmit`（emit kind bit、fullは id だけ、dtsは`[id]`）、`latestChangedDtsFile`、
  `emitSignatures`、`resolvedRoot`、`packageJsons`／`missingPackageJsons`、`errors`／`checkPending`。
  JSONは`encoding/json/v2`の compact 形（空白無し、omitzero、`<>&`はescapeしない）を自前のwriterで書く。
- **診断のkeyと引数**（`crates/diagnostics`）：tsgoは cache した診断を`messageKey`（`Name_code`、名前は100 byteまで）
  と引数で記録する。生成catalogに`key`（tsgo diagnostics/generate.go convertPropertyName）を加え、`MessageChain`に
  `key`と`args`を保持する（等価性には入れない）。tsgoが文に出さない引数も記録する：global typeが無いTS2318は
  `getSuggestedLibForNonExistentName`のlib（`["Promise","es2015"]`）を第2引数に持つので、checkerの
  `get_global_symbol`も同じ引数で作る。
- **checkerのfact**（`crates/checker/src/incremental.rs`、tsgo programtosnapshot.go）：fileごとに、cacheする行
  （bind＋check、noEmit filterの前、include processorの行は含めない、sort／dedupe）、`referencedFiles`
  （import／dynamic import／`require`（JS）／import typeのmodule symbolの宣言file、`declare module "x"`の
  augmentationの merged symbol、全てのambient module（patternも）の宣言file：tsgoは Strada と違い
  `declarations.length > 1`で絞らない）、path referenceの名前、`affectsGlobalScope`（global augmentation、
  module／JSONでないscriptの非ambient-module statement）、`skipped`。serial／sharded 両方の driver で集め、
  構文errorでcheckが閉じるときも（tsgoはincremental programの生成時にcheckerを作り referencedMap を計算する）
  bind＋initだけ行って集める。
- **programのfact**（`crates/program`）：resolverが探した全ての`package.json`（存在するものはincrementalのとき
  realpath、無いものは`node_modules`下だけが`missingPackageJsons`）を root／library／project／read-ahead worker
  の resolver から集めて`PreparedProgram::package_json_probes`に持つ。`allowJs`／`experimentalDecorators`の
  生の値（`*_specified`）を`CompilerOptions`に持つ（tsgoは設定されたときだけ書く）。
- **compilerの組み立て**（`crates/compiler/src/incremental.rs`）：`--noEmit`のcommandはsessionが文書を作り
  （`NoEmitOutcome::build_info`）CLIが書く（`--listEmittedFiles`なら`TSFILE:`、失敗はTS5033）。emitのcommand
  （serial／sharded 両route）はsinkをwrapして書いた`.d.ts`のsignature（source map commentの手前までのhash）を
  記録し、emitの後に文書を書いてemitted filesに加える。tsgoのgate通り、構文／option／global の診断があれば
  semantic 行は cache しない（全fileがidだけ）、`noCheck`は`checkPending`。宣言診断は`--noEmit`で
  requestされたときだけ`emitDiagnosticsPerFile`と pending kind（DtsErrors bitを落とす）に反映。`noEmitOnError`
  でemitが飛んだときは全fileがpendingのまま。
- **入口**：emitのcommandの`incremental`／`composite`拒否を外した。`--noEmit`のconfigが`noCheck`を受け入れる。
- **実projectの比較で直した3件**（material-ui 38 configの最初の比較は33件一致、azureの途中経過は`packageJsons`と
  未書き出しの2種だけだった）：
  1. **JSX runtime importの合成規則**：tsgo fileloader.go:849は JavaScript か `.tsx` のfileにだけ
     `react/jsx-runtime`を合成する（module性や`isolatedModules`には依らない）。portは6.0.3の規則
     （非宣言fileの`isolatedModules`／external module）で`.ts`にも付けていたので、Next.jsのexampleで
     `theme.ts`より前に`@types/react`が読まれ、file順（`root`の範囲、id）がtsgoとずれた。
     `crates/program/src/module_requests.rs`を tsgo の規則にし、契約testで固定。
  2. **依存symlinkの探索のprobe**：tsgo program.go GetSymlinkCache の依存名の解決（ResolvePackageDirectory）は
     package directoryの存在だけを見て`package.json`を読まないので、`packageJsons`／`missingPackageJsons`に
     何も残さない。portはloaderの先行解決（6.0.3のgetAllModulePathsWorkerの前段）のprobeも集めていたため、
     importされない`dependencies`（`@emotion/cache`、`react-dom`…）が現れていた。先行解決の前にprobeを取り出し、
     先行解決のものは捨てる。programの契約testで固定。
  3. **fileの無いincremental program**（`files: []`＋`references`のsolution config。azureに21件）：tsgoは
     `{"version":"7.1.0-dev","fileInfos":[],"options":{…}}`を書く（`fileInfos`はnon-nilのsliceなので空でも
     書かれ、`fileNames`等は無い。`incremental`だけなら`options`も無い）。checkerの空programの早期returnが
     factを返さず文書が作られなかった。serial／sharded両driverの空program経路でfactを返し、emit routeの
     空program分岐でも文書を書く。CLIの契約test 1件（noEmit composite／composite／incrementalの3構成）。
  4. **peerDependencyのprobe**（azureの`tsconfig.snippets.json`等）：tsgoのpackage id（resolver.go
     readPackageJsonPeerDependencies）はpackageの実pathの`node_modules`で各peerの`package.json`を
     getPackageJsonInfoで探すので、見つかっても見つからなくてもlistに残る（viteやvitestのoptional peer、
     `@mui/material`等）。portはhostから直接読んでいたので記録されなかった。`load_package`と共通の
     `record_package_json_probe`で記録する。
  5. **type referenceのprimary lookupのprobe**（material-ui `docs`）：tsgoは存在するtypeRootごとに
     `<typeRoot>/<name>/package.json`をcacheに問い合わせる（loadNodeModuleFromDirectory →
     getPackageJsonInfo。candidateのdirectoryが無くても）。portはdirectoryが無いと先に返していた。
     programの契約test（fixture fx5/typeroot）で固定。
  6. **相対pathの`.`の縮約**：tsgoのGetPathComponentsRelativeToは両側をreducePathComponentsで縮約するので、
     相対のtype reference（`/// <reference types="./css" />`）が`node_modules/@types/./css/package.json`を
     探しても`@types/css`の綴りで列挙される。portの`relative_path_from_directory`（P3-6aの出力pathにも使う）に
     縮約を加えた。pathの契約testで固定。
  7. **commandだけが書く**（hostedの最初の実行で`compiler/incrementalConcurrentSafeAliasFollowing`が
     regression）：tsgoのtest harnessは`compiler.Program`を作るだけでbuild infoを書かない（書くのは
     `execute`のincremental program）。portのsessionはharness／APIのemitでも文書を作っていた（harnessの
     `@outDir: ./res`は相対のままなので相対path計算がpanic）。`ProgramSession::command_build_info`を
     `run_no_emit_command`／`emit_for_cli`だけが立て、他の経路（`run`、`emit`、harness）は書かない。
     APIのemitの契約testを`tsBuildInfoFile`ありに広げて固定。
  8. **rootの無いprogramのread-ahead**（azureの`tsconfig.samples.json`等14件）：tsgoはroot fileがあるときだけ
     libと automatic type directive のtaskを加える（processAllProgramFiles）。portのread-aheadはrootが無くても
     それらの対象を先読みし、worker resolverのprobeが`packageJsons`に残っていた。rootがあるときだけ先読みする。
  9. **peerDependencyのprobeは identity が付くとき**（azure 10件）：tsgoのreadPackageJsonPeerDependenciesは
     getPackageId、つまりresolutionがpackageの中で終わったときに走る。portは`package.json`を読むたび（`ws`のように
     `@types/ws`へ落ちる探索の途中でも）peerを探していた。探索結果はcached packageに持ち、identityを付ける3箇所で
     記録する（probe mapは`RefCell`）。
  10. **chainの各段のrelatedInformation**（azure `ai-agents` snippets 1件）：tsgoのcreateDiagnosticChainFromErrorChain
     はrelaterの関連情報をleafに付け、NewDiagnosticChainはその上に積む各段にも同じ関連情報を持たせる（headを含む
     全段）。portはheadだけだった。`MessageChain::related`（nested用、等価性には入れない＝tsgoのequalMessageChain）
     を加え、relaterの出力で全段に伝え、build infoで段ごとに書く。text出力（pretty／errors.txt）はheadの関連情報
     だけ印字するので変わらない。CLI契約test（fixture fx5/related、4段）で固定。
- **残る実projectの差（material-ui `docs`、38件中1件）**：pnpmの2つの`next@16.3.8`のcopyのうち、docs側のcopy Aの
  `router.d.ts`／`link.d.ts`／`app.d.ts`／`document.d.ts`は`next/index.d.ts`の`/// <reference path>`で
  先にprogramに入る（package id無し）。tsgoはその後の`next/router`等のimportのidをfileに伝え
  （filesparser.go:293）、collect時にprogram順で同じidの後のfile（core-docs側のcopy B）をredirectにする
  （:448）。portは admission 時にidを判定するので copy B の4 fileがprogramに残り、以降のidと`root`の範囲が
  ずれる（診断とemitは同じ）。collect時のpackage dedupe（到達性の再計算込み）はloaderの構造変更なので
  次の follow-up（P3-6bの続き、P3-6cの前）に回す。
- test：fixture比較（tsgoの実行記録とのbyte比較：noLib＋最小lib 16件＋実lib 15件＋宣言map／emitDeclarationOnly／
  isolatedDeclarations／宣言診断／非ASCII 8件）、CLIの契約13件（tsgoのbyteで固定）、`tsc-incremental`の
  unit 11件（hash、JSON、snapshot）、programの契約3件（JSX runtime importの規則、probe、typeRootのprobe）。
- 実project（このbuild、`compare-buildinfo.py`：tsgo `--singleThreaded` と `TSRS_CHECKERS=1` の`--noEmit`、tsbuildinfoのbyte＋stdout＋exit、1 job）：material-ui 38 configのうち37件一致（buildinfoを書く5件のうち4件。残る1件は上の`docs`）。azure-sdk-for-js 2,756 configは2,756件一致（buildinfoを書く338件を含む。rc6では25件が違い、その原因3種（8〜10）を直したrc7で25/25、最終binaryで全件再実行）。。
- 残る制限：古いtsbuildinfoは読まない（毎回初回buildとして書く；差分の`changeFileSet`等は次のslice）。
  `tsc -b`は未対応。`--declarationMap`等のCLI flagは未対応のまま（configでは動く）。JSDoc `@import`は
  referencedMapに入れない。`contentMapperIdentities`は無い。
- conformance（最終bytes `3561ee8ad`での全体実行。macOS、maintenance clamp＋`nice -n 20`、1 worker）：12,748 case／4,742 s、full 13,451、emit_full 13,443、mismatch 0、ratchet 0 regressions／0 above tiers（P3-6aと同じ行、ratchetの更新なし）。focused：jsx 344 full、tsx／incremental／composite／uildInfo 0 regressions。parallel control（`--checkers 4`）は未実行。
- local（maintenance clamp、1 job）：6 crate（checker、compiler、program、incremental、emitter、diagnostics）の`cargo test --no-fail-fast` 3,442 passed／0 failed；触ったcrateの`cargo clippy --all-targets -- -D warnings`と`cargo fmt --all -- --check`；`cargo xtask codegen diagnostics-check`。corpusの診断（8 corpora、`--noEmit`、既定のchecker数）はtsgoと8/8一致（zodのpartitionに依存するTS5115の1行はこの実行では出ず、tsgoと同じ）。
- hosted：PR #694 run 37542163883（head `3561ee8ad`：`plan` 30s、`rust` 6m46s、`conformance (TypeScript 7.1)` 19m31s、
  `gates` 12s。全て成功）。それ以前のhead `af32c409a`のrun 37521886916も全て成功（rust 8m57s、conformance 18m34s）。
  最初のhead `58c840c17`のrun 37512340686はconformanceが1 regression（上の7.）で失敗。

### P3-6bのhostedの記録、計測、merge（2026-10-07）

- hosted：PR #694 run 37553918032（head `6b238a97c`＝packetの記録を含む最終候補：`plan` 24s、`rust` 10m40s、
  `conformance (TypeScript 7.1)` 19m16s、`gates` 13s。全て成功）。その前のcodeの最終head `3561ee8ad`のrun 37542163883も
  全て成功（plan 30s、rust 6m46s、conformance 19m31s、gates 12s）。merge → `f774090ce`（merge commit）。
- corpusの診断（`--noEmit`、既定のchecker数。`3561ee8ad`のbuild vs tsgo 7.1.0-dev）：8 corpora全てbyte一致（zodのpartitionに
  依存するTS5115の1行はこの実行では出なかった）。
- 性能（分割して計測：corpusごとに1回ずつ、3 round、interleaved、`nice -n 20`。main＝P3-6aの`4c8efd09e`のbuild）：
  `--noEmit`のwallはこのbuild/main 0.961〜1.021（hono 138 vs 141 ms、zod 528 vs 549、Playwright 364 vs 359、TypeScript
  compiler 337 vs 330、Next.js 772 vs 759、Effect 488 vs 493、Vue.js 320 vs 332、VS Code 3,339 vs 3,310）、peak RSS
  0.988〜1.019。bench-full（emitあり）は0.978〜1.011（hono 145 vs 148 ms、zod 590 vs 585、Playwright 462 vs 457、
  TypeScript compiler 480 vs 481、Next.js 941 vs 948、Effect 720 vs 719、Vue.js 382 vs 383）、peak RSS 0.995〜1.018。
  1.015を超えた`--noEmit`の3件を10 roundで再計測：TypeScript compiler 328 vs 327 ms（1.003）、Next.js 720 vs 734（0.981）、
  Playwright 368 vs 366（1.005）＝noise。tsgoに対してはwall 0.59〜0.89（`--noEmit`）、0.60〜0.78（bench-full）、
  peak memory 0.65〜0.91。劣化なし（incrementalでないprogramではfactを集めず文書も書かない）。
- cloneの状態：material-ui、azure-sdk-for-js、bench corpusにこの実行が残したfileは無い。

## P3-6b2 packageの重複排除をcollect時に決める（2026-10-07）

P3-6bの実projectに残った1件（material-ui `docs`）の原因。tsgoのfile loader（compiler/filesparser.go）はparse taskを
全て読み込んでから`collectFiles`で program の file を集める：pathごとのtask dataは「そのpathに到達した task のうち
最初に非空の packageId」を持ち（:293-295）、collectはroot taskのsubtaskを順に辿りながら、入った file の packageId を
登録し（preorder）、同じ id が既に別の file に登録されていれば redirect にして subtask を辿らず（:448-481）、file は
subtask の後に並ぶ（postorder）。portは admission 時（最初に読み込む瞬間）に redirect を決めていたので、
`/// <reference path>`で先に（id無しで）入った copy に後の import の id が伝わる場合、後から入った同 id の copy が
program に残った（`docs`：core-docs側の`next`のcopyの`router.d.ts`等4 file）。
- **loader**（`crates/program/src/loader.rs`）：admission 時の redirect（`package_id_to_source`、
  `VisitState::PackageRedirect`）を外し、全ての copy とその subtree を tsgo と同じく読み込む。`StagedSource::package_id`
  に最初の id を持つ（admission の reason、無ければ後の `observe_existing_source` の reason）。`top_level_sources`
  （depth 0 の訪問：root、自動 type directive の対象、library）を起点に、`finish()` が `collect_files()` で tsgo の
  collect と同じ DFS（`source_edges` の順＝path reference → type reference → lib reference → import）を行い、
  id の登録／redirect（owner に `remember_package_redirect`、reason を owner へ）／到達しなかった source の除外／
  program 順（`program_order`、located diagnostic もこれを使う）を決める。root と project reference の redirect の
  index は owner に付け替える。lib reference は collect の edge として記録する（non-external の到達性には使わない）。
- **root の redirect**（conformance `compiler/declarationEmitForGlobalishSpecifierSymlink`：harness では全 file が root）：
  root の file が redirect になると tsgo は root の path が owner を指す（`filesByPath[rootPath] = packageIdFile`）。
  `PreparedProgram` の root の検証（`try_add_root`）に、owner に登録された redirect path を認めた。
- **順序の変化**：reprocess（JS depth の昇格）で後から読み込んだ file は、Strada では親の後に追加されたが、tsgo の
  collect では親の subtask として親の前に並ぶ。no_lib の契約 2 件の期待順序を tsgo（`tsgo --listFiles`、fixture
  p36b2/fx-reprocess）に合わせた。
- **ついでに見つかった referencedMap の差**（`docs`の`.cjs` 2 file）：tsgoのgetReferencedFilesはimport literalを
  GetSymbolAtLocationで解決し、それがmoduleを返すのはimport／export宣言の名前、`import x = require()`、`import()`、
  import type、そして**variable declarationのinitializerである`require()`**だけ（checker.go getSymbolAtLocationの
  string literalの分岐）。`module.exports = require("x")`のようなrequireは解決されず参照に入らない。portは
  JavaScriptの全requireを解決していた。`collect_module_references`に`symbol_imports`（解決される literal だけ）を
  加え、referencedMapはそれを使う。CLIの契約（fixture p36b2/fx-require：`.cjs`の`module.exports = require`、`.js`の
  `const d = require`、`.ts`のimport）をtsgoのbyteで固定。
- test：programの契約（collect fixture：path reference で先に入った copy が id の owner になる、`b.ts` の解決が
  owner を指す、redirect path の登録）、CLIの契約（同 fixture の tsbuildinfo を tsgo の byte で固定）。
- 実project（`compare-buildinfo.py`、`--noEmit`、tsgo `--singleThreaded` vs `TSRS_CHECKERS=1`、byte＋stdout＋exit）：material-ui 38 configのうち37件一致（`docs`は下のschedulingの残差＝`affectedFilesPendingEmit`の2 fileだけ。file集合・順序・package.json・referencedMap・診断は一致）。azure-sdk-for-js 2,756/2,756一致。DefinitelyTyped：9,067/9,067一致（`dt-compare.py --jobs 4`、349 s）。
- 残る差（記録のみ、dedupe とは独立）：**parse task の scheduling**。tsgo の filesParser は task の subtask を
  「最初に処理された depth」で一度だけ開始し（`startedSubTasks`）、同じ path を後で浅い depth で訪れても自身の
  `lowestDepth` だけ更新して subtree には伝えない。処理順は work queue（single-threaded なら FIFO＝BFS 的）で決まる。
  port は Strada の findSourceFile（DFS、depth 0 の再訪で reference を全て再処理、浅い再訪で import を再処理）。
  現れ方：(1) material-ui `docs` の `TextareaAutosize.tsx`／`.types.ts`：`index.ts` が x-data-grid の
  augmentation（depth 2）から先に処理され、その子は external のまま＝tsgo は emit 対象外、port は docs 側の
  depth 0 の import で再処理して emit 対象（`affectedFilesPendingEmit` に 2 file 多い。file 集合、順序、
  referencedMap、診断は一致）。(2) fixture p36b2/fx-reprocess/two：tsgo は `shared` を depth 1 で先に処理するので
  path reference 先の import（`reference-leaf.js`）まで読み込むが、port は読み込まない。multi-threaded の tsgo では
  scheduling に依存する。次の follow-up の候補（queue 順と「subtask は一度だけ」の再現）。
- conformance（最終bytes `42653406f`での1回の全体実行。macOS、`nice -n 20`、2 worker）：12,748 case／451 s、full 13,451、emit_full 13,443、mismatch 0、ratchet 0 regressions／0 above tiers（P3-6bと同じ行、ratchetの更新なし）。focused：duplicatePackage 11、library-reference 15、packageJson 16、moduleResolution 130、typeRoots 3、declarationEmit 307、Symlink 11 full、0 regressions。parallel control（`--checkers 4`）は未実行。
- local（`nice -n 20`、2 job）：`cargo test` program 513＋compiler contracts（build info 17）；program／compiler／checker／conformance／emitter／harness の`cargo clippy --all-targets -- -D warnings`と`cargo fmt --all -- --check`。corpusの診断（8 corpora、`--noEmit`、既定のchecker数）は8/8 tsgoと一致（zodのpartition行はこの実行では出なかった）。

### P3-6b2のhostedの記録、計測、merge（2026-10-07）

- hosted：PR #695 run 37562780818（head `1edb3e911`＝packetの記録を含む最終候補：`plan` 27s、`rust` 10m16s、
  `conformance (TypeScript 7.1)` 20m07s、`gates` 12s。全て成功）。codeの最終head `42653406f`のrun 37560121323も
  全て成功（plan 32s、rust 10m51s、conformance 19m09s）。最初のhead `d8e297872`はconformanceが1 regression
  （rootのredirect、`42653406f`で修正）で失敗。merge → `3817ea436`（merge commit）。
- corpusの診断（`--noEmit`、既定のchecker数。`42653406f`のbuild vs tsgo 7.1.0-dev）：8 corpora全てbyte一致。
- 性能（分割して計測：corpusごとに1回ずつ、3 round、interleaved、`nice -n 20`。main＝P3-6bの`f774090ce`のbuild）：`--noEmit`のwallはこのbuild/main 0.967〜1.030（hono 135 vs 136 ms、zod 488 vs 490、Playwright 347 vs 355、TypeScript compiler 315 vs 326、Next.js 689 vs 704、Effect 498 vs 484、Vue.js 326 vs 317、VS Code 3,318 vs 3,345）、peak RSS 0.988〜1.095（honoの300 MB台の±30 MB）。bench-full（emitあり）は0.946〜1.045（hono 147 vs 150 ms、zod 609 vs 583、Playwright 461 vs 488、TypeScript compiler 482 vs 483、Next.js 935 vs 963、Effect 742 vs 732、Vue.js 393 vs 397）、peak RSS 0.986〜1.010。1.015を超えた4件を10 roundで再計測：zod/full 595 vs 597 ms（0.997）、Vue.js/noEmit 330 vs 325（1.015）、Effect/noEmit 488 vs 510（0.957）、hono/noEmit 134 vs 132（RSS 312 vs 306 MB）＝noise。tsgoに対してはwall 0.57〜0.87、peak memory 0.66〜0.94。劣化なし（重複copyのsubtreeも読み込むが、bench corpusに重複packageは無い）。
- cloneの状態：material-ui、azure-sdk-for-js、DefinitelyTyped、bench corpusにこの実行が残したfileは無い。

## P3-6c incrementalの第2段：古いtsbuildinfoの読み込みと再利用（2026-10-07）

P3-6bが書いたbuild infoを、次の`tsc -p`が読み込んで再利用する（tsgo execute/incremental：
`ReadBuildInfoProgram`、`programToSnapshot`、`collectAllAffectedFiles`、`emitFilesIncremental`、
`emitBuildInfo`）。commandは変更の無いfileの検査を省き、pendingのfileだけをemitし、stateが変わらなければ
build infoを書き直さない。tsgoの各stepの出力（stdout、exit、書かれたfile、build infoのbyte）と一致する。
- **読み込み**（`crates/incremental/src/reader.rs`、`old_state.rs`）：`BuildInfo::from_json`はbuildInfo.goの
  `UnmarshalJSON`の形（root／fileInfos／semanticDiagnosticsPerFile／affectedFilesPendingEmit／emitSignaturesの
  compact形）を読み、tsgoが拒むもの（壊れたJSON、未知のversion、incrementalでない文書）は読み込み無し＝fresh build。
  `OldState::from_build_info`は`buildInfoToSnapshot`：fileInfos、options（`parse_build_info_options`：
  `GetCompilerOptions`、pathのoptionはbuild info directoryから絶対化）、referencedMap、changeFileSet、cached rows、
  pending emit（0は旧optionのfull kind）、emitSignatures（plain／other-options形）、errors／checkPending、packageJsons。
- **option変更の判定**（`options.rs`）：declscompiler.goの`AffectsSemanticDiagnostics`／`AffectsEmit`／
  `AffectsDeclarationPath` flagの表（記録される67 optionと同じ集合）と`optionsHaveChanges`（strict系は
  `GetStrictOptionValue`、allowJsは`GetAllowJS`で比較）。
- **snapshotの状態機械**（`snapshot.rs`）：`Snapshot::new(program, old)`＝`programToSnapshot`（version／
  affectsGlobalScope／impliedNodeFormat／referenceの差分でchanged、削除されたreference、新file、unchanged fileの
  semantic rows／emit rows／emit signatureの引き継ぎ、global fileの削除・global性の消失、option変更によるpending
  emit、checkPending）。`collect_all_affected_files`＝affectedfileshandler.go（`updateShapeSignature`は
  d.tsのsignature、宣言file／JSONはversion；globalなfileは全file；isolatedModules；`referencedBy`のBFS；
  `handleDtsMayChangeOf*`：signatureをversionにしてdts emitをpending、const enumのexportはJSも；lib fileの
  rowsの除去；`assumeChangesOnlyAffectDirectDependencies`）。`store_fresh_rows`／`finish_check`、
  `record_emit`＝emitfileshandler.goの`updateSnapshot`（emit時にsignature==versionならd.tsのhash、composite
  のemitSignatures、`latestChangedDtsFile`、pending kindの更新、emit rows）、`to_build_info`＝
  `ensureHasErrorsForState`／`ensurePackageJsonsForState`／`buildInfoEmitPending`（falseなら書かない）＋
  `snapshotToBuildInfo`（cached rowsは新しいfile idで再直列化、changeFileSet）。fresh buildは`old=None`の同じ機械。
- **checker**（`crates/checker`）：`IncrementalRequest { facts, planner }`。plannerはcheckerの初期化後・検査前に
  `CheckerSession`と全fileのmodule facts（referenced files、affectsGlobalScope、`program_rows`、const enumの
  export）を受けて、検査するfileの集合を返す（serial、sharded、emit-callbackの3経路。shardedは
  coordinatorの計画用checker 1つ）。選ばれないfileは検査されず、whole-program getterと
  `IncrementalFileFacts.semantic_rows`に現れない。gate：`check_runs`＝syntactic rows無し かつ global rows無し
  （options rowsはcompiler側）。
- **compiler**（`crates/compiler/src/incremental.rs` `IncrementalDriver`、`lib.rs`、`cli.rs`）：CLIがbuild info
  を読む（`read_old_build_info`：default libraryは`lib.*.d.ts`の名前）。plannerはProgramState＋Snapshotを作り、
  tsgoの`collectAllAffectedFiles`が走る条件（semantic getterが走る＝syntactic／options／global gateが開いていて
  noCheckでない、またはemitする（noEmitOnErrorでgateが閉じていれば走らない））でaffected filesを扱う。signatureは
  `ForcedDeclarationEmitter`（tsgoの`EmitOnlyBuilderSignature`：1 fileの強制d.ts emit、mapの手前までのtext＋
  `diagnosticToStringBuilder`のhash）。検査後：検査しなかったfileのcached rowsをDiagnosticに戻して（byte→UTF-16、
  keyと引数からmessageを整形、chain／related、noEmit filter）semantic rowsに合流、include-processor rowsを添える；
  宣言診断getterはpending（DTS_ERRORS）のfileだけ、他はcacheのrowsを印字；emitはpendingのunitだけ
  （`emit_planned_files`：JS／d.tsの別、`UnitEmitRequest`）、compositeの変わらないd.tsは書かない
  （`SkippedUnchanged`、listingにも出ない）；build infoは変化があるときだけ書く（`TSFILE:`もそのときだけ）。
- **emitter**：`emit_planned_files`／`emit_planned_units_with_kinds`（unitの一部のmemberだけをemit）、
  `ForcedDeclarationEmitter`、listingはtsgoの順（source mapを本文より先：`printSourceFile`）。
- **CLI**：`--listEmittedFiles`は`--noEmit`の経路でも受け付ける（tsgoはbuild infoを列挙する）。
- scenarioと実projectで直した3点：(1) `declarationMap`のときsignatureのd.ts emitにmapのpathを渡す（渡さないと
  `sourceMappingURL`の前のtextが本番のemitと食い違い、変わっていないfileのsignatureが変わる）。(2) const enumの
  exportの判定はtsgoのSkipAliasの通りaliasだけを解決する（debug buildでは非aliasの`resolve_alias`がpanicする）。
  (3) `--noEmit`の宣言診断getterのgate（tsgoのemitFilesAndReportErrors：semantic diagnosticsが空のときだけ
  `GetDeclarationDiagnostics`）は、build infoから引き継いだcached rowsも数える。azure-sdk-for-jsのsnippets
  configで、2回目に新しい検査が無い（全fileがcache）のにgetterが走り、pending kindの`DtsErrors` bitを落として
  build infoを書き直していた。
- `TSRS_INCREMENTAL_TRACE=1`で、plannerの判断（old stateの有無、gate、affected handling、各fileのsignatureと
  check／pending emit）をstderrに出す。
- test：`crates/incremental`の28 unit test（reader／old state／options／snapshotの遷移をscenarioのbyteで固定）、
  `cli_contract.rs`に生成した23 scenario（`scratchpad/p36c/scenarios.py`、`steps.py`でtsgoの各stepを記録、
  `gen-steps-tests.py`で生成）：chain／composite／option変更／noEmit＋declaration／noCheck／isolatedModules／
  assumeChanges…／const enum／file削除／壊れたbuild info／JS+JSON／宣言errorのcache／declarationMap／global
  augmentation／reference変更／error chain／skipLibCheck／types reference／noEmitOnError。
- 実project（`compare-rerun.py`：各compilerが同じdirectoryで2回走り、2回目のstdout／exit／書き直したbuild infoを比較）：material-ui 38 configのうち37件一致（`docs`はP3-6b2に記録したparse-task schedulingの残差＝`affectedFilesPendingEmit`の2 entryが1回目から持ち越されるだけ。2回目のstdout・exit・書き直す判断は一致）。azure-sdk-for-js 2,756 configのうち2,756件一致（2回目にbuild infoを書き直したのはtsgo 92件、port 92件。1,021 s）。DefinitelyTypedはincrementalでないので対象外。
- 残る制限：`outFile`はTS 7.1で削除されたoption（TS5102）で、tsgoはbundleせずfileごとにemitするがportはbundle
  する（incrementalと無関係の既存の差）。noEmitOnErrorの宣言診断gate（emit経路）はcached emit rowsを印字しない。
  `repopulateInfo`（module解決の診断chainの再計算）は書かない／読まない。tsc -bは次。
- conformance（最終bytes `409868c87`での1回の全体実行。macOS、`nice -n 20`、2 worker）：12,748 case／452 s、full 13,451、emit_full 13,443、mismatch 0、ratchet 0 regressions／0 above tiers（P3-6b2と同じ行、ratchetの更新なし）。
- local（`nice -n 20`、2 job）：`cargo fmt --all -- --check`、crate（incremental、checker、emitter、program、
  compiler）の`cargo clippy --all-targets -- -D warnings`と`cargo test --no-fail-fast`（最終bytes `409868c87`で3,436 passed／0 failed、`--test-threads 2`）。corpusの診断（8 corpora、`--noEmit`、既定のchecker数）：7件がtsgoとbyte一致、zodは既知のpartition依存の1行（P3-6aの出力と同一）。clone（material-ui、azure-sdk-for-js、bench corpus）にこの実行が残したfileは無い。
- 性能（分割して計測：corpusごとに1回ずつ、3 round、interleaved、`nice -n 20`。main＝P3-6b2の`3817ea436`のbuild、このbuild＝`409868c87`）：`--noEmit`のwallはこのbuild/main 0.972〜0.998（hono 134 vs 137 ms、zod 497 vs 501、Playwright 344 vs 344、TypeScript compiler 317 vs 318、Next.js 694 vs 696、Effect 476 vs 480、Vue.js 322 vs 327、VS Code 3,283 vs 3,350）、peak RSS 0.977〜1.069（Playwrightの795 vs 744 MBはmain側の外れ値）。bench-full（emitあり）は0.962〜1.017（hono 145 vs 147 ms、zod 595 vs 593、Playwright 460 vs 452、TypeScript compiler 481 vs 483、Next.js 935 vs 972、Effect 729 vs 743、Vue.js 384 vs 388）、peak RSS 0.980〜1.017。1.015を超えた2件を10 roundで再計測：Playwright/full 455 vs 462 ms（0.986、RSS 1.001）、Playwright/noEmit 344 vs 346（0.993、RSS 789 vs 798 MB）＝noise。tsgoに対してはwall 0.48〜0.93、peak memory 0.63〜0.91。劣化なし（`incremental`でないprojectではplannerは作られず、fresh buildのplannerはsnapshotを作るだけ）。

### P3-6cのhostedの記録とmerge（2026-10-07）

- hosted：PR #696 run 37573603085（codeの最終head `409868c87`：`plan` 33s、`rust` 11m4s、`conformance (TypeScript 7.1)` 19m21s、`gates` 13s。全て成功）。packetの記録を含む最終候補 `51a93ad21`（docsだけの変更）のrun 37576597729（plannerはPR全体の差分で選ぶので両jobが走った：`plan` 28s、`rust` 9m58s、`conformance (TypeScript 7.1)` 19m16s、`gates` 16s。全て成功）。merge → `615216273`（merge commit）。
- 計測（性能、corpusの診断、実project、conformance）は上の記録のとおり、merge前に最終bytes `409868c87`で行った。
- cloneの状態：material-ui、azure-sdk-for-js、bench corpusにこの実行が残したfileは無い。

## P3-6d `tsc -b`（build mode）（2026-10-07）

tsgo execute/build（`orchestrator.go`、`buildtask.go`、`uptodatestatus.go`、`host.go`）の第1段：`-b`／`--build`を
最初の引数に取る command。命名された project（無ければ `.`）とその参照先を graph にし、参照先から順に、各 project
を「up to date か」判定してから build／pseudo-build（timestamp の更新）／skip／clean する。watch と並列 builder
（`--builders` は受け付けて無視、tsgo が報告する順に1つずつ build）は対象外。
- **command line**（`crates/compiler/src/cli.rs`）：tsgo `ParseBuildCommandLine`。build option は `--verbose/-v`、
  `--dry/-d`、`--force/-f`、`--clean`、`--stopBuildOnErrors`、`--builders N`；`-p` が受け付ける compiler option は
  全 project に適用；位置引数が project；`clean`+`force`／`clean`+`verbose` は TS5053（exit 1）。`-p` と共通の
  option 解析は `parse_common_option` に括り出した。
- **orchestrator**（`crates/compiler/src/build.rs`）：`createBuildTasks`（config を1回ずつ解析、無ければ task だけ）、
  `setupBuildTask`（DFS、`completed`／`analyzing`、循環は `circular: true` の文脈でなければ TS6202、post-order が
  `order`）、`buildProject`（`getUpToDateStatus` → verbose の報告 → `handleStatusThatDoesntRequireBuild`：UpToDate は
  dry のみ報告、UpstreamErrors は verbose で skip を報告、Solution、ConfigFileNotFound は TS6053、pseudo-build は
  dry なら報告のみ／それ以外は `updateTimeStamps`、dry なら「would build」→ build しないときは config の解析診断を
  報告し error があれば exit 1）、`compileAndEmit`（verbose「Building project」、`--force` でなければ task の build
  info を old state に渡して `-p` と同じ pipeline で build、emit された file 以外の output を touch
  （`Updating unchanged output timestamps`；incremental／noEmit の project は build info だけ）、status を
  BuildErrors／UpToDate(最初の emit file) に）、`onBuildInfoEmit`（書かれた build info を読み直して entry に、
  d.ts が変わっていれば dtsTime=now）、`getLatestChangedDtsMTime`、`hasConflictingBuildInfo`、clean（出力と build
  info を削除、input と同名は残す、dry は一覧「A non-dry build would delete the following files:」）、報告（task
  ごとの buffer を order で結合、exit は最大、pretty なら全 project の error summary）。status 行は tsgo の
  `CreateBuilderStatusReporter`：plain は `HH:MM:SS AM - message` + 空行、pretty は `[`灰色の時刻`] message`。時刻は
  `TZ`／`/etc/localtime` の TZif から local time を計算（crate は unsafe 禁止なので libc を使わない）。
- **up-to-date 判定**：`getUpToDateStatus` を 1:1 に移植（build info の有無／version／errors・semanticErrors・
  checkPending／incremental の emitDiagnosticsPerFile・changeFileSet・semanticDiagnosticsPerFile・
  affectedFilesPendingEmit・`IsEmitPending`／input の mtime と version（root info reader 経由）／root の増減／
  build info の非 root file／非 incremental の output の mtime／upstream の build info 衝突と d.ts の変更時刻／config と
  extends 先／package.json）。mtime は path ごとに1回だけ観測（tsgo `host.mTimes`）。
- **build mode の pipeline**（tsgo `CompilerOptions.Build`）：`ProgramSession::with_build_mode`、driver の
  `AffectedPolicy.build`。非 incremental の project も build info を書く（`build_info_file_name_in_build_mode`、
  `ProgramState.build`／`root_file_names`、`Snapshot::can_use_incremental_state`、`serialize` の非 incremental 形
  ＝`root` に canonical な名前、errors／semanticErrors／checkPending／packageJsons、`fileInfos` 無し）。`BuildInfo` に
  `incremental`（tsgo の non-nil `FileInfos`：incremental の形なら空でも `"fileInfos":[]`）。`ensure_has_errors` は
  old state が無いときも pending にする（tsgo は unknown との比較）。`has_changed_dts_file`。composite の d.ts が
  signature は同じで map だけ違うとき（old signature が「別 option」形）は書いた後に mtime を戻す
  （`differsOnlyInMap`：orchestrator が build 前に d.ts の mtime を控えて戻す）。emit の file 一覧は build では
  `listEmittedFiles` に関わらず集める（`EmitHost::collects_emitted_files`；`TSFILE:` の印字は option で gate）。
- **program crate**：`output_file_names`（tsgo `GetOutputFileNames`：file ごとに js、map、d.ts、d.ts.map）、
  `output_js_file_name`／`output_extension`／`source_map_file_path`、`build_info_file_name_in_build_mode`。
- **参照の検証の修正**（`loader.rs` `project_reference_diagnostics`）：tsgo `rangeResolvedProjectReference` は root の
  config を `seen` に入れてから辿るので、循環（`circular: true`）で root に戻る参照は検証されない。port は root を
  参照として再訪し TS6377 を出していた。
- **incremental crate**：`BuildInfo::root_info_reader`（tsgo `GetBuildInfoRootInfoReader`）、`is_emit_pending`、
  `file_name_of`／`file_info_of`、`is_default_library_name` を公開。
- test：`cli_contract.rs` に生成した 12 の build scenario（`scratchpad/p36c/build_scenarios.py`：sample（core／logic／
  tests：no change、leaf の編集、upstream の本体のみ／形の変更、同じ text の書き直し、force、dry、verbose 無し、
  clean dry、clean、clean 後の rebuild、tsconfig の変更）、errors（upstream の error、stopBuildOnErrors、leaf の
  error／構文 error）、noEmit の root、非 incremental の root（output の削除、error）、solution（`files: []`＋references、
  複数 project、既定の `.`）、missing config（`-b bogus.json`、無い参照、dry、clean）、cycle（TS6202、clean、
  `circular: true`）、declarationMap の切り替え、version 不一致と壊れた build info、root の追加・削除と参照先 file の
  削除、outDir/rootDir＋`--listEmittedFiles`、extends 先の変更）。runner は status 行の時刻を正規化し、削除された
  file も比較する。unit test：時刻の比較、TZif の読み取り、status の書式。
- 残る制限：`--watch`、並列 builder、`--listFiles`／`--explainFiles`／`--traceResolution`／`--diagnostics`／`--help`／
  `--locale`（`-p` と同様に未対応）、config に解析診断のある project は報告だけで build しない（tsgo は build する）、
  content mapper。
- scenario oracle（`steps.py`、tsgo `-b --singleThreaded` vs この binary）：build の 12 scenario／82 step と P3-6c の
  23 scenario が全 step 一致（stdout（時刻は正規化）、exit、書かれた file、削除された file、build info の byte）。
- conformance（最終bytes `69eedeeee`での1回の全体実行。macOS、`nice -n 20`、2 worker）：12,748 case／463 s、full 13,451、emit_full 13,443、mismatch 0、ratchet 0 regressions／0 above tiers（P3-6c と同じ行、ratchet の更新なし）。
- local（`nice -n 20`、2 job）：`cargo fmt --all -- --check`、program／incremental／emitter／compiler の
  `cargo clippy --all-targets -- -D warnings`、`cargo test --no-fail-fast`（4 crate 1,658 passed／0 failed、Clippy のみの
  修正の後に最終 bytes `69eedeeee` で program／compiler を再実行：995 passed／0 failed）。
