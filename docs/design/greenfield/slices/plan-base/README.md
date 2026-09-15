# PLAN-BASE：現行残差と検証記録の台帳

2026-09-15。**台帳作成は完了。runtime の修復・profile admission は行っていない。**
担当：Codex／統合担当。親：[残タスクと完了までのスライス設計](../../remaining-completion-slices.md)。
基準 main：`f9ef828a56c9f9305947a6e6f3c1ae39072b3110`。

## 結果

凍結された compiler / conformance / project / transpile の **15,642 ID** を、
H2 の29段階の qualification、現在の hosted 観測、global/class の履歴、既知差分と照合した。
台帳は **6,045 unique IDs / 9,004所属**。内訳は元の corpus 5,904 ID と、class / parameter /
direct / exception の別集合141 ID。元の corpus には「台帳に存在」「少なくとも一つの現在の
hosted profile に exact 記録」「凍結 H1 完了」のいずれかがあり、未分類は0。

**6,045 は不具合数ではない。** 同じ case が複数段階に属し、古い保留と新しい exact が
併存する。現行 hosted profile に exact 記録を持つ元 corpus は12,290 IDだが、
これも全 option / 出力観測の完了数や製品全体の pass rate ではない。
別 profile の成功だけで残差や既知差分を消していない。

| 集合 | 照合結果 | 現在の扱い |
| --- | --- | --- |
| H2.5h / 6a / 6c の known | 12 / 1 / 8 所属。6a/6c が1 ID重複し **20 unique IDs** | 現行 hosted でも既知差分として実行。除外や fixture 更新なし |
| 旧 class A6-34 の128失敗 | **88 ID は同じ fixture bytes で現在の retained hosted が exact ×2**。残り40は現在の全 command が未再測定 | 88件を修復依頼に戻さない。40件を現在の失敗と断定しない |
| 旧 global A6-37 の14失敗 | **7 ID に後続の完全 command 修復記録**。7 ID は今回確認した後続記録では未確認 | 最新 main で14件を replay した記録ではない。global 755/769 の履歴も再計算しない |
| H2.7c の旧11保留 | rootDir の1件が現在 exact。**10保留** | 旧qualificationは保存し、移行した観測を別記 |
| H2.7d/e の旧34保留 | directory 23件が現在 exact。D283＋E-only8＋directory23＝**314 exact / 11 later references** | 中間の D-only reference 32 を残件数として使わない |
| parameter/comment | **5 ID**。統合記録に strict failure が残る | H2.8a-A-PC1。今回は新たに実行していない |
| SUPER direct | **4 ID** が memoized lowering の明示的な native 差分対照 | exact に加点しない。通常 command の witness と別扱い |
| upstream exceptions | SUPER primary **2**、retained 入力 **2** | 合計4 ID。native の command 成功・Rust修復件数に加点しない |
| profile の保留台帳外 | **217 ID** に、今回の照合で現在の hosted emit 記録が見つからない | 元の全分母との突合せで追加。H2.9-INV が source / option / runner を調べる |

corpus の所属や古い `required_slices` は設計時点の入力として保存している。
たとえば H2.3d の695 future-deferred、H2.5g の2,883 future-deferred は、その数だけの新規
不具合を表さない。台帳中の `hosted_exact` と対にして読む。

## 根拠と再現性

- [inventory.v1.json](inventory.v1.json)：全6,045 ID、所属、disposition、triage owner、
  profile ごとの exact 記録、historical failure、後続修復、64 source/input pins。
- [hosted-evidence.v1.json](hosted-evidence.v1.json)：6 job の採取済みログから、件数・
  exact ID・exception・known の行を抽出。元ログの SHA-256 と job URL を保持。
- [inventory.py](inventory.py)：固定 main の git blob を読み、件数・集合・入力 identity を照合。
  これ自体は acceptance/admission gate ではない。

利用した hosted run は [acceptance 34967807268](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34967807268)
と [witness 34967807283](https://github.com/kazhiramatsu/tsc-rs/actions/runs/34967807283)。
いずれも head `dac0d55cec8d9e41cf958b8d5256a0c3cfe8c4b9` で成功。
この head から台帳の基準 main までの全 tracked diff は docs のみであり、source / fixture /
workflow / vendor に差がないことを検証した。今回は native / oracle を新規実行していない。

class の8 fixture はすべて A6-34 receipt の SHA-256 と一致する。
88 ID の集合は、retained job が実際に出した `EXACT x2` と旧失敗集合との交差である。
他 profile の「admitted」は、その profile の現在の件数と既知差分 manifest を hosted の
集計と照合して採用した。H2.7c と D/E はログの exact ID を直接突き合わせた。

この台帳は **指定日の snapshot**。再生成も `BASE` の git blob を使うため、後日実行しても
最新 main の診断結果にはならない。次の更新では新しい base と実行記録を固定して、
既存 snapshot を履歴として保持する。

```sh
# この日付の台帳を再計算して保存済み bytes と比較。Rust build/replay なし
python3 docs/design/greenfield/slices/plan-base/inventory.py --check

# 再生成せず、旧 class 失敗のうち現在未再測定の40 IDを列挙
python3 docs/design/greenfield/slices/plan-base/inventory.py --list \
  --disposition historical-class-failure-not-remeasured

# ある case の所属・状態を調べる。0 ID はエラー
python3 docs/design/greenfield/slices/plan-base/inventory.py --list \
  --case jsDeclarationsFunctionsCjs

# 台帳の判定境界だけを検証
python3 -m unittest discover -s docs/design/greenfield/slices/plan-base -p 'test_*.py'
```

## 修復・調査へ渡す具体的な集合

### class：未再測定40 ID

| 旧 cause / fixture | 件数 | 次の調査境界 |
| --- | ---: | --- |
| C2 / class-field-alias-map-positions | 24 | ES5 の this phase / generated binding。先に VER1.0-MAP で対象 version を確認 |
| C3 / class-field-alias-map-positions | 4 | nested computed name。C02 の監査と重なるため統合担当が依頼範囲を調整 |
| C4 / class-header-token | 8 | export/default の後の comment / token ownership。printer failure とは原因を分離 |
| C5 / hoisted-declaration-export-ranges | 4 | escaped class/export name の生成元・range |

cause 名は旧 source 分析の手掛かりであり、現在の実装に原因を再現した判定ではない。
この3 fixture の現 target は個別 case selector がないため、40件だけを実行する入口の
整備を OPS-COVER に渡す。既存 test 名を指定すると fixture 全体を走らせる可能性がある。

### global：後続修復記録7 ID

| 修復 | 対象 | 根拠 |
| --- | --- | --- |
| require rewrite | emitModuleCommonJS の commonjs / nodenext 2 ID | `h2-8a-require-rewrite-final.v1.json` の original 2、対応 test の固定 ID |
| G4a | jsDeclarationEmitExportAssignedFunctionWithExtraTypedefsMembers | `h2-8a-declaration-comment-ranges-after.v1.json` の final/original-g4a exit 0 |
| G4b | jsDeclarationsClassMethod | 同 receipt の final/original-g4b exit 0 |
| G5c | jsDeclarationsFunctionsCjs | PR #522 統合記録の combined-g5c。G4 の古い shared-G5c failure より後の修復 |
| declaration specifier | declarationEmitPathMappingMonorepo2 | `h2-8a-declaration-specifiers-after.v1.json` の original exact ×2 |
| G7 / object property | plainJSGrammarErrors | `h2-8a-object-property-owners-after.v1.json` の original 4 exact ×2、そのうち当該1件を修復 |

最新 main の回帰確認は、これらの **対象 target のみ**を OPS-COVER に載せて実行する。
この表のために global 769 をローカルで再走する必要はない。

### global：後続の exact を未確認の7 ID

| 旧 cause | 対象 | 件数 |
| --- | --- | ---: |
| G8a | reactImportDropped | 1 |
| G5b | jsDeclarationsExportAssignedClassExpressionAnonymousWithSub、ES2015 / ES5 | 2 |
| G5a | jsDeclarationsParameterTagReusesInputNodeInEmit1 / 2 | 2 |
| G6 | linkTagEmit1 | 1 |
| G8b | bundlerImportTsExtensions、allowImportingTsExtensions=true / noEmit=false | 1 |

まず VER1.0-MAP で retained / intentional change / retired を分け、採用対象だけを現在の
完全 command で再現する。該当 producer を改修する前に最新の失敗と owner を確定する。
global 14 の全件再測定は別の実行として記録し、上の7件の履歴から現在値を推測しない。

## 次の担当と完了境界

| 担当 slice | この台帳から渡すもの | 終了条件 |
| --- | --- | --- |
| VER1.0-MAP（統合担当） | known 20、global 14、class 40、元の15,642 IDと必要な TS7 対応 | version ごとの採用/意図的変更/廃止/新機能と固定 tests。旧 ES5 行を自動的に修復依頼へ戻さない |
| OPS-COVER（統合担当） | class 40 selector、global の focused 入口、declaration/CFG/literal 等の standalone targets | 必要 target のみを実行し、0件や skip を検知する。無関係な全 suite を追加しない |
| H2.9-INV（統合担当） | 217 ID、過去の future-deferred と現在 exact の重複、profile 間の入力・観測差 | 各採用観測の owner と runner を確定。ID一致だけで全観測の完了にしない |
| H2.8a-A-PC1（統合担当） | parameter/comment の5 ID | 現在の失敗を再現し、synthetic prologue 後の comment owner を閉じる |
| C03 printer failure（Claude） | 本台帳は参照情報。③の現在の依頼範囲を維持 | 失敗時の output/state と次の呼出しの観測。PC1 や C4 を無関係に同乗させない |

PLAN-BASE の完了は「残差の修復が終わった」ではなく、**固定した分母の全 ID に、証拠の
種類と次の担当を付け、未計測を成功と混同しない台帳を作った**ことを指す。
凍結 ratchet / fixture / readiness / accepted profile は無変更。
