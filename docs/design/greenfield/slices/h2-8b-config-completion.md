# CFG1 完了追跡表

2026-09-12 JST。ユーザー目標「CFGのスライスを完成させてください」。
開始head `25a7034837ba7f1dc4abd5db18f7a919567af019`、branch `work/h2-8b-config-completion`。
最終production計測 `ec509858e75a6c9efc1efc759f6872049e1d74e2`。
元CFG1の要件とCFG1a/bで残した項目を解消した。結果と実行の詳細は
[完了報告](h2-8b-config-completion-report.md)と
[完了受領証](../../../../ratchets/h2-8b-config-completion-final.v1.json)を参照。

| 要件 | 実装・照合 | 状態 |
| --- | --- | --- |
| config変換・extends・duplicate/invalid・option出所 | CFG1a 28件、compiler/watch/acquisition全139定義、typed bag/raw/undefinedを区別 | 完了 |
| fileNames/include/exclude/wildcard | CFG1b 24件と22純粋path、通常Program順序・rootDir/outDir/出力衝突対照 | 完了 |
| option relationship diagnosticsの順序・span・fallback | CFG1c 76件、CFG1f TS1148/6131、CFG1g entity-name 36件 | 完了 |
| watchOptions/typeAcquisition/compileOnSave変換と継承 | CFG1d 72件、watchのproperty継承・acquisition非継承・raw truthiness/診断分離 | 完了 |
| config再利用 | CFG1e fresh/cache・duplicate/diamond・呼出間変更・clear、通常CLIのcache接続 | 完了 |
| config host callback順序・fault | CFG1e 26件/60 parse attempts、read/exists/readDirectoryの引数・順序・absence/throw | 完了 |
| 通常commandへの接続 | CFG68+LR回帰18件 complete tuple ×2、MOD1境界2件、全88件Program facts ×2 | 完了 |
| 回帰・設計gate・完了監査 | Program/syntax/emitter/CLI、全19 observer、新規source pin、歴史的design gateとcompletion supplement | 完了 |

元の準備registryは変更せず、作成commitの134 pins/38 anchorsを復元して検証した。
`check-h2-8b-config-completion.py`は元witness axes・未解決3項目と今回の実装/証拠との対応を照合する。
CFGの未解決作業は残さない。HOST1/SYS1、MOD1/NC1、BLD1/W1は元の別ownerであり、報告書に
実callerと対照を記録した。H2.8b-CLOSEのprofile activationやhosted acceptanceの成功は主張しない。

途中の修正前失敗、比較側のmessage chain訂正、cacheのstack overflow、相対config診断aliasの
実CLI失敗、旧手書きassertと上流観測の相違は各小単位の設計記録/受領証に保持した。
既存凍結oracleと共有complete-command comparatorは変更していない。
