# H2.8b-CFG1 完了報告

CFG1の元registryにあるconfig変換・出所・探索・診断優先順位・通常command接続を実装し、
CFG1a/bで残したroot option変換、cache/host callback、source依存診断も照合した。
開始headは`25a7034837ba7f1dc4abd5db18f7a919567af019`、最終計測headは`ec509858e75a6c9efc1efc759f6872049e1d74e2`。
branchは`work/h2-8b-config-completion`。完了受領証は
[`h2-8b-config-completion-final.v1.json`](../../../../ratchets/h2-8b-config-completion-final.v1.json)。

| 比較面 | 最終実測 |
| --- | --- |
| config projection | CFG1a 28、CFG1b 24、CFG1c 76、CFG1d 40+32、CFG1g 36＝236/236 ×2 |
| config cache/host | CFG1e 26/26 ×2、変更/clear手順を含む60 parse attempts、callback順序・引数・fault一致 |
| discovery base path | 22/22 ×2（Programを作らない純粋path計算） |
| option catalogue | compiler 129 + watch 6 + acquisition 4＝139定義のmetadata一致 |
| 通常command | CFG 68 + LR回帰18＝86/86 complete tuple ×2 |
| MOD1境界 | isolatedModules/verbatimModuleSyntax 2件 ×2、型付き拒否・書込無し。完全一致数へ含めない |
| Program facts | 上記88件すべてsource/library/root順序一致 ×2、計352 PreparedPrograms |
| 実filesystem CLI | 6ケース ×2、stdout/stderr/exit・全output path/bytes、両実行後のinput不変 |
| Program / syntax / emitter 回帰 | Program lib 29 / contracts 477（既存ignore 5）、syntax 163、compiler対象22 / CLI 18（既存ignore 10）、emitter lib 495 / contracts 452。すべて失敗0 |
| fmt / clippy / diff / observers / completion gate | すべてexit0（19 observer、歴史的design gate、completion gateを含む） |

complete tupleは診断・emit result・status writes・全writeの内容と順序・exitを共有comparatorで
比較する。Program factsは別のfresh Programを作って順序を比較する。これらを一つの実行として
二重計上しない。CLIの12組の比較は別の実filesystem subprocessであり、上記352へ加算しない。
ログと受領証は`target/config-completion-runs/20260911T151359Z/`。
受領証はhead/tree、実exit、入力の前後SHA、ログSHA、実行直後のテストバイナリSHA、CLI production binaryの実行前後SHA、capture SHAを保持する。
full tupleの独立したnative JSON captureを新設したという主張はせず、共有comparatorのassertとログを証拠とする。

## 修正と根拠

- CFG1c：option relationshipを共通validatorへ統合し、config由来のkey/value/fallbackと重複位置を修正。
  config/option診断を通常Programへ渡し、noEmitOnErrorと書込判断に接続。
  76件の正規baselineは50一致/26不一致 ×2。最初のmessage chain flattenの比較側誤りは
  production不変で訂正・再測定し、旧失敗ログも保持した。
- CFG1d：watchOptionsの型変換とproperty単位継承、typeAcquisitionの既定値と非継承、
  compileOnSaveのraw truthinessとbool診断を分離。config conversion診断はProgramへ引き継ぎ、
  診断があるだけでは通常emitを止めない。上流と矛盾していた旧手書き3契約を凍結観測に基づき訂正。
- CFG1e：呼出元所有のextended-config cacheを追加し、通常CLIにも接続。hit時のsource表記、
  変換診断の抑制、parse/read診断の再報告、clearを上流に合わせた。baselineは18/26一致 ×2。
  最初の実装の64段extends stack overflowはcached nodeをBox化して解消。制限値は不変。
  実CLIが見つけた相対config診断aliasの失敗は、canonical auxiliary sourceとsnapshot一致を検証して対応。
  source欠落/別textは引き続き拒否する。
- CFG1f：既存構文parseのexternalModuleIndicatorからUTF-16 error spanを保持し、最初の対象sourceへ
  TS1148/TS6131を付与。baseline14件は2完全一致、10診断欠落、2 MOD1ガード拒否。
  新規12完全commandと2境界対照を区別して受け入れた。
- CFG1g：JSX factoryの文字列分割による誤拒否を、既存JS parserのentity-name検証へ置換。
  コメント、Unicode escape、予約語、JS whitespace、scanner errorを比較。
  reactNamespaceは上流どおり別のidentifier-text規則を保つ。修正前は36件中22一致/14不一致 ×2。

新規観測はproduction修正前に固定した。既存の凍結oracle・共有complete-command comparatorは変更していない。
CFG1d/e/gの設計記録には事前観測・途中失敗・訂正理由を記載し、全test完走に数えない実行を明示した。

## 元設計要件と実owner

| 元の要件・未解決項目 | 解消したowner / 証拠 |
| --- | --- |
| extends and origin | config.rsのroot/base変換、独立source snapshot、option provenance。CFG1a 28件、CFG1cの継承/重複診断 |
| fileNames/include/exclude/wildcard | config_host.rsが探索開始点を計算、config.rsがmatchと優先順位を保持。CFG1b 24+22件 |
| duplicate/invalid and inherited options | config_options.rsの139定義、config.rsのtyped bag/undefined/list/rawと診断owner。CFG1c/d/g |
| rootDir/outDir intersections | configのpath出所をloaderへ渡し、source membership・root外source・暗黙rootDir・出力衝突をcommandで照合 |
| typed optionsと残存inventoryの照合 | compiler/watch/acquisition全schemaを上流exportと照合。通常emitへの投影と既存guardは別契約 |
| config_host/option_validationを実callerから分割 | CLI→CompilerConfigHost→ConfigRootPlan、loader→PreparedProgram、compiler→共通option validator、emitter→出力判断 |
| config diagnostics/path provenanceの凍結 | CFG1a/b/c/d/g projection、CFG1e callback+cache、CFG1f source診断、68 CFG complete commands |

config.rsがJSONC変換、syntax/option出所、extends graph/cacheと診断位置を所有する。
config_host.rsはCompilerHostのconfig adapterと探索開始点、option_validation.rsは純粋なoption間制約を所有。
loader.rsはsource membershipを必要とする診断と、config診断sourceのPreparedProgramへの所有権移送を担う。
compiler/src/cli.rsはroot config bytesの取得/復号と通常CLIごとのcacheを所有する。

元の`h2-8b-e-design-registry.v1.json`は準備時点の23 outline/1 ready-for-baseline・native 0という
履歴であり、その内容を変更して実装済みに見せない。作成commit `f3e7d612dd4824d5b347dca74ded90893fe1c115`
の134 pins/38 source anchorsを復元し、既存design checkerがexit0になることを確認した。
現在のproductionへの変更は別のcompletion受領証と`check-h2-8b-config-completion.py`で照合する。
これにより元CFG1の全witness axesと未解決3項目の対応を検証する。

## 別スライスとの境界

- HOST1/SYS1：root configのSystem read/decode前段、optional host capability、encoding/IO fault製品全体。
  CFG1eはConfigParseHostの実callback順序・引数・absence/throwを比較済み。CLIがroot textを先に
  渡す契約を、TypeScriptのgetParsedCommandLineOfConfigFile（38311–38334）と対応付けた。
- MOD1/NC1：isolatedModules/verbatimModuleSyntax/noCheckのemit activation。2 MOD1 controlsは実際の
  typed guardに到達して無書込を確認。CFGのoption変換・診断受付と製品activationを混同しない。
- BLD1/W1：incremental/composite/project references/watch process。TS5074はconfigFilePathを持たない
  incremental caller、TS6307はcompositeの閉じたfile listに属する。通常CFGはconfigFilePathを設定し、
  実emitには既存incremental/composite guardがある。既存composite拒否・watch CLI対照も回帰で確認。
- CFG1は既存のJSONC/extends資源制限とpublic API境界を維持する。全TypeScript API・全emitterの
  完了、H2.8b-CLOSEのprofile activation、hosted acceptance成功はこの報告に含めない。

重い実行は専用target、taskpolicy -b nice -n 19、CARGO_BUILD_JOBS=1、1 test threadで1本ずつ。
walk、chain-walk、cargo xtask ci、hosted acceptanceは今回実行していない。
