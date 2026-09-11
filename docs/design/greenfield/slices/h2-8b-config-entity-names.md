# CFG1g JSX factory entity-name grammar

CFG1最終source監査でpure option validationの`is_isolated_entity_name`が
`split('.')`と文字列identifier判定を用いていることを確認した。上流
`parseIsolatedEntityName2`（_tsc.js:29042–29061）はJS parserのentity-name grammarを使い、
コメント・Unicode escape・予約語・JavaScript whitespaceを受理し、scanner/parse errorを拒否する。
`reactNamespace`は別の`isIdentifierText`を使うため、この差も対照する。

36 config projectionsを新規観測し、jsxFactory/fragment各16件とreactNamespace4件を固定。
4 complete commandsは有効なコメント/escape/fragmentと無効値+noEmitOnError。
既存76診断projection・全command・共有comparatorの凍結期待値は変更しない。
実装はsyntax crateの既存Parserに小さなvalidity projectionを追加し、Programから呼ぶ。
ASTを返すAPIの新設や別の字句解析器は不要。これはconfig validationの契約であり、
これらのfactory名を使ったJSX expressionのtransform全組合せの完了は主張しない。

修正前854662417は36件中22一致/14不一致 ×2、実exit101。全14件はコメント/escape/JS whitespaceを
持つ有効なfactory名への誤ったoption診断。reactNamespaceの4対照と無効値の拒否は一致した。
観測stdoutのJSONにはU+2028が含まれるため、capture抽出はLFで区切る（Python splitlinesは
U+2028まで区切る）。native比較の入力・期待値・assert自体は変更せず、72件のcaptureを保持した。

最終ec509858eでは36/36 projection×2、追加4 complete commandsとProgram factsも各2回一致。
既存syntax単体163件も通過。全体の計数・受領証は[完了報告](h2-8b-config-completion-report.md)を参照。
