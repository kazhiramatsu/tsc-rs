# H2.8b-CFG1a config extends の変換・継承

2026-09-11。ユーザーの「次のスライスに進んでください」により、LR2最終
`2b2423041796938091568cea1a756ec1d186b46e`から`work/h2-8b-config`で着手する。
CFG1 outlineを、まず既存config parserの変換・継承に沿うこの単位へ具体化する。
原計画・LR1/LR2の観測と受領証は開始時点の記録として保存する。

## 対象とsource

`parse_config_root_plan`、`ParseContext::parse_node/parse_node_uncached`と既存
`CompilerConfigHost`を通して28個のfresh config graphを比較する。
sourceはTypeScript 6.0.3の`parseJsonSourceFileConfigFileContent`、
`parseJsonConfigFileContentWorker`、`parseConfig`、`parseOwnConfigOfJsonSourceFile`、
`getExtendedConfig`。全bundleと行範囲のSHAを観測artifactに固定する。

- 単一・多重・逆順・diamond・重複のextendsと、path-valued optionの宣言元ディレクトリ。
- `${configDir}`の最終置換、null/invalidなown optionによる継承値のmask。
- `files/include/exclude`と出力ディレクトリの継承・上書き・空配列。
- root/extended configの構文エラー、option変換エラー、欠落・循環・後続siblingの順序。
- `compileOnSave`の継承境界（false/true、複数base、own false/null）。

これは28入力のconfig plan比較であり、全`ParsedCommandLine`や通常emitの完全比較ではない。
比較面はraw JSON、fileNames、wildcardDirectories、extendedSourceFilesとsource text、
20種のtyped optionのabsent/undefined/value/list状態、root parse / parsed errors /
compiler-visible config diagnosticsの3列である。診断はcode/category/file/span/message/relatedを照合する。
pathの出所は上流の解決後option値とextended sourceの順序から検査する。

Program構築・option relationship diagnostics・CLI表示・watch/typeAcquisitionの全変換・
config cache/oldProgram再利用・任意のhost hookは後続単位。
H2.8a-closeを前提とするBのprofile activationには進めない。

## 入力・上流観測・native比較

- 入力：`crates/program/tests/fixtures/h2-8b-config-extends-inputs.json`。
- 上流：`scripts/observe-h2-8b-config-extends.mjs --write|--check`。
- 凍結観測：`crates/program/tests/fixtures/h2-8b-config-extends.json`。
- native：`crates/program/tests/integration/h2_8b_config_extends.rs`。

上流は専用の仮想file/directory表から`ts.matchFiles`を呼び、実機filesystemへfallbackしない。
Rustは同じfile表を`MemoryCompilerHost`へ載せ、既存`CompilerConfigHost`の実際のenumerationを使う。
それぞれ各caseで2回freshに構築し、比較は順序を含む。callback回数・host logの同一性は対象外。
nativeは不一致があっても28件×2回すべて記録し、最後に通常assertで実exit101にする。
失敗をignore/expected-failureへ変更せず、期待値の修正で通過させない。

baselineで再現した差をsource分岐へ帰属させてから、必要な既存productionだけを修正する。
主な編集候補は`crates/program/src/config.rs`。host/matcherに差があれば別に原因を記録する。
emitter、printer、metadata、共有comparator、xtask/profileはこの単位の編集候補ではない。

## 受入

28/28を各2回一致させ、通常testをexit0にする。各caseの全actual recordを保存する。
productionを修正した場合は、既存config契約群、関係するoption unit、LR2のlibrary契約も実行する。
既存artifact・既存期待値を維持し、source不変なら不要な追加productionを作らない。
実HEAD、入力/上流/source SHA、実exit、log/capture/binary SHAを新規受領証に記録する。

前回と同じ専用targetをこの作業だけで使い、1 build job、1 test thread、background I/O、nice19。
別worktreeのacceptanceと重なる場合は使用状況を確認して記録し、他のprocessは変更しない。
新規hosted acceptance・walk・chain-walk・full `cargo xtask ci`はこのfocused作業には追加しない。
