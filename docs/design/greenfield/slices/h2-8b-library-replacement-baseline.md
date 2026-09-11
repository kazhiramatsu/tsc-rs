# H2.8b-LR1 — fresh Program library replacement baseline

2026-09-11。状態：**ready-for-baseline / evidence-only**。設計の未解決事項は、この比較を
追加・実行する範囲では0。native未実行なので、production修正の設計完了・互換性達成は主張しない。
分割全体は[計画](h2-8b-e-design-plan.md)、固定値は[registry](h2-8b-e-design-registry.v1.json)。

## 目的と変更範囲

TypeScript 6.0.3のfresh Programで、library replacementの選択が通常のJS・declaration・map・
診断・終了状態まで一致するかを12件で確定する。既存loader testsの成功だけでは確認できない
経路を埋める。全件一致ならruntimeは変更せず、この範囲の証拠を残す。不一致なら原因を分類して
H2.8b-LR2または実際のownerへ渡す。H2.8bのactivationはB-CLOSEが持つ。

開始production：`5134bb0187f9f3f6ac2a0218f8d8e98859cf4fea`、tree
`1e632db73fbcd8e37a6e3c63b03daf57dfbdafdc`。TS source commitは
`050880ce59e30b356b686bd3144efe24f875ebc8`。
`_tsc.js` SHA256は`1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3`。
ASTで取得した上流関数範囲、各範囲のSHA、Rust/comparator/全vendor library/観測物のSHAをregistryに固定する。
`actualResolveLibrary`のみ、宣言だけでなくcallback選択を含む122748–122755行を固定した。

着手時に専用worktree/branchをこの設計branchから作り、実際のHEADとtreeを記録する。
baseの祖先関係と全pinを検査する。変更済みの前提がある場合は差分を読み、依存する設計と
source観測を再確認して新しい版にする。SHAだけを付け替えて着手可能に戻さない。

このpacketで許可する編集：

- 新規`crates/compiler/tests/integration/h2_8b_library_replacement.rs`。
- `crates/compiler/tests/contracts.rs`の新規module登録のみ。統合担当が専有する。
- 専用名のbaseline/final receipt、設計記録。実行ログ・captureは専用`target`配下。

既存共有comparator、Rust production、CI/manifest/profileの変更はLR1の範囲外。
既存fixtureは不一致に合わせて書き換えない。新しい入力が必要なら別ID・別観測版で追加する。

## Sourceから既存Rustへの対応

| 上流owner（`_tsc.js`） | 観測する意味 | 現行Rustで確認した対応 |
| --- | --- | --- |
| `getOptionsForLibraryResolution` 40643–40645、`resolveLibrary` 40646–40648 | library解決はNode10を使う。通常moduleのsuffixes/exports設定を流用しない | `load_program_worker`内でlibrary専用`ModuleResolver`を作る |
| `getInferredLibraryNameResolveFrom` 122398–122401 | configFilePathがあればconfig directory、なければcwdのsynthetic containing file | `StagedGraph::resolved_library_path`のbase・synthetic name生成 |
| `getLibraryNameFromLibFileName` 122402–122411 | `lib.dom.d.ts`→`@typescript/lib-dom`、`lib.dom.iterable.d.ts`→`@typescript/lib-dom/iterable` | `library::replacement_package_name` |
| `actualResolveLibrary` 122748–122755 | host callbackがあれば優先、なければlibrary resolver/cache | LR1はcallback不在の通常hostのみ。任意callbackはHOST1/API1境界 |
| `pathForLibFile` 124518–124525、`pathForLibFileWorker` 124526–124573 | `libReplacement`不在/falseのfallback、解決成功/NotFound、同一libのcache | `resolved_library_path`、`resolved_library_paths`、`LibraryCatalog` |
| `processLibReferenceDirectives` 124574–124591 | triple-slash lib referenceがlibraryとしてloadされる | `process_lib_references`とsource分類 |
| `isSourceFileDefaultLibrary` 123562–123564、`getDefaultLibFilePriority` 123124–123138 | rootにも存在するreplacementのlibrary membershipとsource/library順序 | `SourceClass::Library`、loader `finish`、`PreparedProgram::library_files` |
| `createProgram` 122625–125485 | roots、default libs、noLib、重複とfresh Programの構築 | `load_emitting_config_program`とloaderの既存構築順 |
| `emitFilesAndReportErrorsAndGetExitStatus` 129468–129485 | 通常commandの診断・write・status・終了状態 | `ProgramSession`と既存complete-command comparator |

既存`library_program_loader_contract.rs`にはroot promotion、config directory/subpath、missing
replacement fallbackの3testsがある。これは今回の通常emit比較の代替ではない。

[emitter architecture](../emitter-architecture.md)のE-ENTRY / E-PROTOCOL / E-OUTPUT-SCRIPTと
[post-H1 schedule](../post-h1-completion-slices.md)を引き続きauthorityとする。
このpacketでは現在のloader・entry・test helperの実在と呼出条件を読み直した。
古いarchitecture行のvalidation日付を更新したことにも、新しいruntime qualificationにも数えない。

## 固定入力と上流観測

入力：`crates/compiler/tests/fixtures/h2-8b-library-replacement-inputs.json`。
期待値：`crates/compiler/tests/fixtures/h2-8b-library-replacement.json`。
observer：`scripts/observe-h2-8b-library-replacement.mjs`。
期待値SHA256：`32ee0d8204de1def87d953c685c448467bb9d403080881b7e93c5ca72e48de2d`。

全caseはcwd `/project`、case-sensitive、fresh Program、ES2015/ESNext、通常emit。
入力はconfig文字列を持ち、既存Rust helperのconfig経路を使用する。JS/declarationと両mapを
出し、CRLF、listEmittedFilesを有効にする。ambient replacementのmarkerをdeclaration出力まで
参照するpositiveと、標準libに戻るcontrolを用意した。

| case ID末尾 | 固定する分岐・対照 |
| --- | --- |
| enabled-package | `libReplacement:true`でpackageのmarkerを使用 |
| disabled-package | falseではpackageがあっても標準domを使用 |
| omitted-option | option不在でも標準domを使用 |
| missing-package-fallback | trueだがpackage不在なら標準domへfallback |
| duplicate-lib-reference | `lib`重複とtriple-slash参照がsource/libraryを重複させない |
| config-directory-anchor | config配下packageとcwd配下の異なるmarkerを置き、config側を選ぶ |
| package-subpath | `dom.iterable`をpackage subpathへ解決 |
| module-suffixes-isolated | 通常moduleSuffixesの不一致がlibrary解決を妨げない |
| package-exports-isolated | 通常bundler解決のexportsがlibraryのNode10解決を上書きしない |
| root-promoted-to-library | replacementを明示rootにしてもlibraryとして分類 |
| missing-subpath-fallback | package subpath不在で標準lib.dom.iterableへfallback |
| no-lib-gate | noLibではreplacementを読まず、明示globals rootだけを使う |

観測結果は**12/12 complete ×2、24 fresh Programs、例外0**。
11件は診断0・exit0、config-directory-anchorは診断6059→5011・exit2である。
このcaseはconfigとsource directoryの交差を含むため、rootDir関連診断も含めて保存した。
全12件とも4writeである。exit2を失敗扱いから除いたり、期待値をexit0に直したりしない。
Rustの比較test自体は、期待されたexit2を含む全タプルが一致すればexit0になる。

観測はwrite callbackの順序・path・bytes/BOM・sources・data、emit resultの有無/内容、map JSON、
reported diagnostics（related informationを含む）、status文字列、command exitを保存する。
加えて`program_facts`にsource順、library membership順、root名順を記録した。
通常hostのreadFile/fileExistsとdirectory overlayを使い、vendor JSを改変していない。

## Test追加手順

新規moduleに2つの独立したtestsを設ける。両方とも凍結artifactを読み、TS version、
12件、2 repetitions、upstream_failures空をassertする。

1. `library_replacement_matches_complete_typescript_observations`：既存
   `h2_7c_declaration_blocking::assert_cases_with_inspection(&artifact, true, record_attempt)`を呼ぶ。
   `record_attempt`はcase IDとprepared Program到達をログに出すだけの関数とする。
   `h2_7b_w4a_controls::assert_command_observation`までの通常経路を変えない。
2. `library_replacement_matches_program_membership`：既存
   `assert_cases_with_inspection(&artifact, true, inspect_program_facts)`を呼ぶ。
   inspectionの先頭で同じ到達ログを出し、`PreparedProgram::source_files()`、`library_files()`からsource IDで引いたpath、
   `roots()`のpathをJSON配列にし、`typescript_observation.program_facts`と順序込みで比較する。
   pathは既存`ProgramPath::display()`を使う。ソートやフィルターを追加しない。

2は同じhelperを使うため、membership一致時にはcommand比較も実行する。
成功時のnative構築回数は2 tests ×12 cases ×2 repetitions =48。
unique complete-command対象は12件であり、24件に水増ししない。
失敗時はhelperがcase単位でpanicを捕捉するため、そのcaseの2回目に到達しない場合がある。
inspection到達ログで実回数を数え、load前の失敗は別に記録する。全件×2完了と記載しない。2つを独立させることでmembershipの不一致が
complete-command比較の実行を妨げない。

registrationは`contracts.rs`に既存形式の`#[path = "integration/h2_8b_library_replacement.rs"]`
と`mod h2_8b_library_replacement;`を追加するだけ。共有helperに新しいoption matchは不要である。
caseの`options`は空で、lib/libReplacement/noLibはconfig parserから入る。

## 実行・受入・引継ぎ

軽い前提検査：

```sh
python3 scripts/check-h2-8-design-plan.py
node --check scripts/observe-h2-8b-library-replacement.mjs
taskpolicy -b nice -n 15 node scripts/observe-h2-8b-library-replacement.mjs library-replacement --check
```

test登録後はcontracts.rsのSHAが開始pinから変わる。変更前の検査をreceiptに保存し、
変更後はdiffがmodule登録だけであることを確認する。registryの開始pinは保存する。
同じMacの重い実行枠が空いてから、一つずつ実行する：

```sh
lr1_run_dir="target/library-replacement-runs/$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$lr1_run_dir"
taskpolicy -b nice -n 15 env CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="$PWD/target/h2-8b-lr1" cargo test -p tsc-rs-compiler --test contracts h2_8b_library_replacement -- --nocapture --test-threads=1 > "$lr1_run_dir/native.log" 2>&1
lr1_status=$?
printf '%s\n' "$lr1_status" > "$lr1_run_dir/native.exit"
```

receiptには実HEAD/tree、diff、入力/観測/上流/production/comparatorのSHA、環境、実command、
開始/終了UTC、実exit、実test数、caseごとの到達/失敗、バイナリSHA、ログSHA、実行後のpin不変を記す。
失敗をpipeや`|| true`で成功に変えない。baselineを保持してから原因に基づく別packetを設計する。

LR1の完了条件は、12件を欠落なく両比較で実行し、実結果とowner分類を残すこと。
互換性が確認できたという主張の条件は、12件の全タプルとmembershipが×2一致し、通常test exit0。
不一致があればLR1の調査結果として記録できるが、そのcaseをqualifiedにはしない。

LR1単独ではproduction変更がないため全acceptanceの追加実行は不要。
将来LR2で変更する場合は、失敗原因に必要なfocused test、既存library loader contracts、
影響する通常command群を選び、統合時に既存hosted acceptanceで確認する。
現在の192 decorator regression、530 complete-command subset、全769 checkpointは別の集合として扱う。

未観測として残す境界：oldProgram reuse/invalidation（L2/BLD1）、host.resolveLibraryなど公開hook
（HOST1/API1）、host例外/IO fault（HOST1/SYS1）、trace出力（E-OBS1）、case-insensitive/realpath、
追加package条件、全catalog名の網羅。12件の成功でlibrary replacement全体やH2.8を閉じない。
