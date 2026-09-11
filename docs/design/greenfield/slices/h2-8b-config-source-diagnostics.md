# CFG1f source-dependent option diagnostics

CFG1完了監査で、pure option検証から除かれていたverifyCompilerOptionsのsource分岐を確認した。
対象はpinned `_tsc.js:124874–124898`のTS1148/TS6131。module none + ES5の
最初の非ambient external module、module未指定outFileの最初のexternal moduleが診断対象。

先に8 complete commandsと6 span controlsを上流観測した。新しいgroupなので以前の70件や
凍結oracleを変更しない。前者にはES2015/isolated/verbatim/declaration-only/noEmitOnErrorの
negative/positive分岐を含む。後者はnamed class/function、匿名class、forced moduleのSourceFile、
empty source、type alias、UTF-16とUTF-8位置が異なるcommentを使う。

設計：既存SourceRequestPlanの構文parseからexternalModuleIndicatorのerror spanを保持する。
Programのpostorderから最初の非ambient external moduleを選び、option diagnosticsに入れる。
追加parseを行わず、getErrorSpanForNodeのindicator到達可能分岐（名前、node全体、SourceFile/
匿名宣言のtoken）を使う。noEmitOnErrorは既存option gateで判断する。

TS5074（configFilePath無しincremental）とTS6307（compositeのfile list）はbuilder側の境界。
通常CFGからconfigFilePathは常に設定し、incremental/compositeの実emitは既存typed guardで拒否する。
この区別は最終owner表に残し、builder製品をCFG完了に含めた扱いにはしない。

修正前e62da34ab：14件中2一致/12失敗。うち10件はTS1148/TS6131の欠落、
2件は既存isolatedModules/verbatimModuleSyntaxのemitter guardで拒否された。後者は
H2.8c-MOD1の別ownerであることを実呼出で確認したため、凍結上流観測を保ったまま
新規native adapterで既存のtyped-guard controlへ分類する（共有comparatorは不変）。
2件は完全command一致数へ含めず、無書込の型付き拒否とProgram facts・TS1148不在を比較する。
今回の完全command受入は12件。ガード自体は変更しない。

最終ec509858eでは12 complete commandsと2 MOD1 boundary controlsが各2回通過。
14件すべてordered Program factsも各2回一致。全体の計数は[完了報告](h2-8b-config-completion-report.md)を参照。
