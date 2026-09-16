# C01 / A-INT1 — literal update integration

2026-09-16。統合baseは `a2413610a2cbffd38156cfb337b00f20ce9eee06`。
[提出時の設計](../DESIGN.md)、[before/after](../REPORT.md)、[引継ぎ](../INTEGRATION.md)は履歴として保持する。
C01と[OPS-COVER-3E](../../witness-coverage/compiler-require-rewrite/README.md)を一つの候補へまとめる。
複数スライスを実装してからhosted CIへ進める2026-09-16のユーザー指示に従う。

## 取り込みと確認した境界

提出patchのSHA-256は `080311638452dc2f575b765b460fdebef54bc0625f279cbc02ca50d1b7bcd980`。
base `6c41a0388` から現在のmainまで、変更するproduction 4 fileに差分はない。
提出patchは進捗表の競合だけを解消して適用した。production・observer・凍結fixtureは提出bytesと同じ。
元の未commit worktreeは変更していない。

sourceから到達するpipeline 22 commandはbefore/afterとも一致。
修復対象はdirect factory/updateとtagged-template raw fallbackであり、sourceの不一致22件を修復したとは数えない。
factoryはgeneric 782 exact / 205 n/a、typed 987 exact、transformはgeneric 315 exact / 84 n/a、typed 399 exact。
各routeのexact/n/a件数を統合時にassertへ追加し、typed経路の比較が減った場合も失敗させる。
lifetimeは統合時に生成・print・disposeの一連を2回実行し、同じ観測になることを検査する。
synthetic 5行のdispose後metadata差は修復せず、parsed 5行との区別を保持する。

`set_literal_value` はproduction callerがなく、既存test 2件が使うRustのin-place seamとして維持する。
今回のtyped update追加を理由に削除しない。custom transformのruntime admissionとsession寿命の差は後続ownerへ残す。

## CI入口

| suite | job | 比較対象 | native tests |
| --- | --- | --- | ---: |
| literal-update | printer | factory 987 / transform 399 / lifetime 10 | 3、filterなし |
| literal-update-pipeline | controls | 22 complete command | 1、filterなし |
| require-rewrite（OPS-COVER-3E） | controls | focused 60 / composition 4 / substitution 4 / dynamic 6 | 専用4、10 filtered |

emitter observerも `(script, group)` を受け取り、同じscriptの異なるgroupを省略しない。
共有observerの変更はliteral-updateとliteral-update-pipelineの両方を選択する。
両jobで必要な場合に `.node-version` のNode 25.2.1を使用する。
report/capture環境変数とrequire-rewrite内部filterを消去し、選択件数を減らす継承設定を許さない。
共有production変更は従来どおり全acceptance / witnessesを選択する。worker数・時間上限は維持する。

## 検証と残件

ローカル統合検証は成功。[受領記録](local.v1.json)に最終source・command・環境・時間・binary/log hashを保存した。
[emitterログ](emitter-witnesses.log.gz)はC01 3＋隣接6＝9 tests、5 target、observer 27.735秒、build/replay 173.518秒。
最終format後の[専用3 tests](emitter-final.log.gz)も成功し、[route別JSON](routes/)を保存した。
[compilerログ](compiler-witnesses.log.gz)はC01 pipeline・require-rewrite・UTF-16 tagged/literalの4 target / 7 tests、
observer 251.335秒、build/replay 1133.662秒（新規worktreeのbuildを含む）。
[emitter lib](emitter-lib.log.gz)は506 pass、fmtと入口台帳v12の再生成も一致。
全体の時間を追加suite単独の費用と解釈しない。提出recordのpatch/logは空白も含めてbytesを保存し、
それ以外のsource/documentは `git diff --check` を通した。
CI選択・failure propagation 45 testsは成功。policy suiteは40 pass / 1 inherited failure：
`registered h2 artifact labels follow the chain-walk ORDER`。
変更前のmainと同じpolicy関連sourceでも同じassertが失敗し、期待配列末尾のH2.8a二項目が現行ORDERに無い。
提出者が確認したemitter contractsのcompact private-body comment failureも本修復の完了へ含めない。
hosted結果とmainへの統合を以下に記録する。ローカル受領記録のpendingは採取時点の履歴として保持する。

## Hostedとmain統合

[PR #542](https://github.com/kazhiramatsu/tsc-rs/pull/542)をmainへ統合した。
head `8610f3a735ba5dc76f7921a53bd4949d9594f7de`、merge `7df1a8ed138ec8060fdd8bfc33cb539d92b8302e`。
全7 replay jobと両gateが成功し、candidate / tested merge / landed mergeのtreeは同一。
[受領記録](hosted.v1.json)、[printerログ](hosted-printer.log.gz)、[controlsログ](hosted-controls.log.gz)を保存した。

[acceptance](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35076082072)と
[witnesses](https://github.com/kazhiramatsu/tsc-rs/actions/runs/35076082024)が同じheadを検証した。
CIでもfactory generic 782 / typed 987、transform generic 315 / typed 399が一致し、各routeに差分・errorは無い。
C01 pipeline 22 commandとrequire-rewrite74専用commandの二重比較も成功。
printerのemitter direct batch全体は12 target / 17 tests、observer 24.85秒、build/replay 17.169秒。
controlsのcompiler direct batch全体は12 target / 28 tests、observer 355.586秒、build/replay 1090.67秒。
これらのbatch時間には既存suiteを含め、追加suiteだけの費用とはしない。

| PR replay job | 実時間 |
| --- | ---: |
| acceptance (early) | 12分03秒 |
| acceptance (late) | 17分58秒 |
| acceptance (wide) | 20分33秒 |
| witnesses (controls) | 33分29秒 |
| witnesses (retained) | 8分57秒 |
| witnesses (primary) | 8分25秒 |
| witnesses (printer) | 2分51秒 |

7 jobの合計は104分16秒。plan/gate/main pushは含めない。
controlsは33分29秒。45分の分割検討目安・60分の上限内で完了した。
PR結果からmain pushの成功を推論しない。C01 / A-INT1とOPS-COVER-3Eの実装・統合は完了。
synthetic dispose metadata、既存のpolicy ORDER testとcompact-body comment failure、runtime admissionは既述の境界に残る。
