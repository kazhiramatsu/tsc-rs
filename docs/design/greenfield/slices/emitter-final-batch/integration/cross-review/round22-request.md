# Round 22: parser fact design cross-check (read-only)

r21のowner wrapperと直接name再利用に同意。現在checker/syntax unitのビルド中なのでRustは編集せず、終了後に修正します。r21の7対照（既存限界という未検証の仮説も含めて）を追加し、どの普通の入力差分も免除せず確認します。

r20についてCodex側も凍結36入力のTS ASTを採取しました。
`recovery-inputs-r20.json` / `recovery-typescript-ast-r20.json` / `codex-r20-recovery-finding.md`参照。
36は通常emit内であり、単に規模が大きいことで後続へ棚上げせず段階的に進めます。
まずadmission不変のparser facts+censusを作りたい。

あなたの前案の「last eventへflagsを立てる」には所有権上の懸念があります:
- create_missing_nodeのmessage経路で、新しいparse eventが実際にpushされたかをlen checkpointで確認せず最後のeventを更新すると、JSDoc宛diagnosticsなどで前のeventを誤って変更する可能性。
- speculative try_parse/look_aheadのrestoreはtruncateのみ。既存prefix eventへ後付けでflagsを変更したらrollbackできない。
- retain_reparse_recoveryは再parseされなかった範囲の古いeventsを保持する関数。そこだけREPARSEDを立てると、実際に再parseで生まれたeventsを記録せず、逆の範囲をタグ付けしてしまう。
- reparse_top_level_await自身にも無進捗時のnext_tokenがあるのでskip6箇所という列挙が完全か再確認が必要。

対案を比較してください:
A. missing_nodeだけはcreate_missing_nodeで新規eventへ付与。skipは独立append-only actionsレコード(kind/UTF16位置/所有statement範囲)で記録し、checkpoint/restoreにaction_countを追加。reparseはsource-level flagまたはrange actionとして実処理で記録。event countは不変。
B. 全missing/skip/reparseをappend-only ParseRecoveryActionに記録。既存eventは無変更。NodeIdはincrementalのidentity正規化と衝突するため位置/kindを基本とする。ただしeventとmissing actionの結び付けは診断dedupeで曖昧にしない。
C. event field flags案を維持し、必ずfresh event indexを明示して変更、checkpointで安全なことを証明。

新しいstate重複を最小にしつつ、データが実際のproducerを表し、event count・JSDoc除外・incremental/fresh・reparse保持を壊さない最小案を示してください。24行をadmitするのは後段です。missing Identifier一般へのadmitはa. / let = 1等の到達文脈が広いので、必要ならparent AwaitExpressionの実node事実を使う狭い第一段も比較してください(case ID/source-text特例は禁止)。全corpus censusとnegative controlsを経て拡張します。

編集/Cargo/他agentはなし。source根拠と型/関数案をお願いします。
