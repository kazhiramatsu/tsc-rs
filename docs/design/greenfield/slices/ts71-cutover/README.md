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

RUN_RECORD
