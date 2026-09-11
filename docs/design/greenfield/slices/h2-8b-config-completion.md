# CFG1 完了までの追跡表

2026-09-12 JST。ユーザー目標「CFGのスライスを完成させてください」。
開始head `25a7034837ba7f1dc4abd5db18f7a919567af019`、branch `work/h2-8b-config-completion`。
前turnは着手方針の通知だけで中断し、実装・実行中processは無かった。今回状態を再確認した。

CFG1の元registryにあるconfig変換、診断優先順位、extends/path出所、fileNames/include/exclude/
wildcard、duplicate/invalid/inherited options、rootDir/outDirの交差を受入対象として維持する。
CFG1a/bで明記した未検証項目もこの追跡表で解消する。小単位の通過だけで全体完了とはしない。
H2.8b全体のprofile activation、独立HOST1/SYS1、build/watch製品全体は元計画の別ownerである。
ただしそこへの境界は実呼出元と対照で確認し、単に対象外という記述だけで完了扱いしない。

| 要件 | 必要な証拠 | 現状 |
| --- | --- | --- |
| config変換・extends・duplicate/invalid・option出所 | pinned source分岐と現typed option catalogueの対応、独立upstream/native比較 | CFG1a 28件済み、catalogue全体の残存監査が必要 |
| fileNames/include/exclude/wildcard | CFG1bに加えrootDir/outDir交差とsource依存診断 | CFG1b 24件と22純粋path計算済み、交差のProgram比較が必要 |
| option relationship diagnosticsの順序・span・fallback | verifyCompilerOptions、createDiagnosticForOptionの各分岐、inheritance/duplicate/negative controls | CFG1cで着手 |
| watchOptions/typeAcquisition/compileOnSave変換と継承 | parseOwnConfig/getExtendedConfigのsource照合、rawと変換値・診断を区別 | 未完 |
| config再利用 | graph内の重複/diamondとfresh呼出間の変更、cache有無の実caller対応 | 未完 |
| config host callback順序・fault | sourceの読取/exists/readDirectory境界と現adapter、失敗後の状態 | 未完 |
| 通常commandへの接続 | complete tuple、Program facts、option診断/出力境界 | CFG1a/b計16件済み、残存修正のcontrolsが必要 |
| 回帰・設計gate・完了監査 | 全CFG証拠再照合、既存suite、source/hash/exit記録、registry未解決の解消 | 未完 |

重い実行は専用target、taskpolicy -b nice -n 19、CARGO_BUILD_JOBS=1、1 test thread、1つずつ。
既存凍結観測と共有comparatorは保持する。新しい観測で既存の手書きassertが上流と矛盾した場合は、
その不一致とsource根拠を記録して修正する。期待値を実装に合わせるための変更はしない。

## CFG1c 実測（継続中のCFG全体とは別に記録）

`57e202e50`で76/76診断projection×2、通常commandは新規16件＋既存34件すべて
complete tuple/ordered Program factsが各2回一致（計200 PreparedPrograms）。Program契約は
472通過・失敗0・既存ignore5。`ratchets/h2-8b-config-diagnostics-final.v1.json`に実行を記録。
最初の診断baselineにはmessage chainを先頭だけ読む比較側の問題があり、修正してproduction不変で
50一致/26不一致×2を取り直した。通常commandの修正前は12件中6件失敗、追加拡張子4件中3件失敗。

修正：既存option_validationへ純粋な制約を統合、key/valueとcompilerOptions fallbackを修正、
root compilerOptionsの最初のsyntaxを診断に使用。通常emitのloaderはoption診断で止めず、
noEmitOnErrorの出力判断へ渡す。H0の明示gateは維持。allowImportingTsExtensionsは既にcheckerと
宣言specifierが読むoptionであり、emitterの一律拒否を削除し、実際の.ts exportで対照した。

rootDir外file、入力とoutDirの衝突、TS6の暗黙rootDirレイアウト診断も通常commandで一致。
root option40件の上流観測を準備した。変換・cache/host境界・最終source監査は依然未完。
