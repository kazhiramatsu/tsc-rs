# Emitter final r11 integration

2026-09-18。producer canonical `~/dev/tsc-rs-emitter-final` の未commit r11を、
main `3b1f5fe87fd31e3b303bb44bd257342735452ed9` に基づく
`work/emitter-final-integration` へ受領した。提出worktreeのsourceと元workspaceの未commit作業は変更していない。
提出時の検証と統合候補の検証は以下で区別する。hosted完了前に全体完了を主張しない。

## 受領とレビュー

[受領台帳](records/received.v1.json)はproducerの37 tracked filesと557 untracked files、
各byte hash、基点とpatch hashを保持する。`candidate-tracked.diff` のSHA-256は
`241f13aa19a15ca76ad85db8392a19d741d2f5bda3389e790bf3baaf7e4c2d53`。
mainとのproduction競合はなかった。[原因とcommitの対応](records/producer-commits.v1.json)により
16 commitsへ分割した。共通hunkを持つbinding/class/async等は、提出の最終bytesを保つ単位にまとめた。
元のpatch scriptsとCOMMIT-PLANも保存した。

305本のignored raw logsはlossless gzipとして隣接保存し、
[archive台帳](records/producer-log-archive.v1.json)に展開後のhashを記録した。
提出metaの `.log` は同名 `.log.gz` を展開して照合できる。履歴の失敗logも保持する。

レビュー対象は、checkerのsource identity・enum/async/JSDoc facts、factoryのoriginalとgenerated binding、
名前確定のscope/print順、async superとPromise constructor、class/decoratorのreceiverとassigned name、
System/moduleのhelper/import/export、printerのcommentとsource spelling、option admissionとharness floor。
実装は既存typed producer/consumerへ接続し、test IDや期待outputをproductionの分岐に用いない。
全共有経路の回帰確認は既存acceptance/witnessと追加global/classで行う。

## 統合時の追加修正

### KNOWNを新しい不具合の免除にしない

提出のKNOWNはIDと原因を保持していたが、比較では「そのIDが何らかの差分を持つ」ことしか要求しなかった。
新しい `emitter-final-known-native.json` はr11の68行のnative outcomeも固定する。
27 checker行はwrite/diagnostics/source order/emit result/exit等の全観測を、41拒否行は正確な拒否理由を比較する。
command構築時の拒否はpartial callbackのpath/hashも既存messageに含む。
2回の独立観測が一致し、ID集合・owner/cause・native outcomeがすべて一致したときだけKNOWNとして認める。
exactになったKNOWNと、新しく差分が出た行は引き続き失敗する。
[由来](records/known-native-provenance.v1.json)と `local/known-native-68.log.gz` を参照。

68行はH2.9 parse recovery 36、module resolution request plan 4、H2.5h helper collision 1、checker 27。
互換成功に数えない。「emitter owner 0」はこれらの解決済みを意味しない。

### Case-insensitive oracleと修復済み台帳

H2.6cのcompiler-runner hostに `@useCaseSensitiveFileNames` を反映し、lookup keyのみcanonical化した。
出力pathやsource spellingを正規化して比較する変更ではない。project descriptor経路にはこのdirectiveがなく、
当該2件はcompiler経路だけなのでproject/config hostは変更しない。
2件の再採取とcase-sensitive対照2件を各2回検証した。643 case recordsのうち641件は不変。
[旧新の完全観測](records/oracle-host-correction.v1.json)では変更はmap write/emit-result mapとそのfingerprintに限られる。
診断、JS callback、exitは不変。採取cacheのidentityもこのhost変更を含む。

RustでEF2/EF3の21 profile memberships（20 unique IDs）がすべてexact×2となったため、
H2.5h 12、H2.6a 1、H2.6c 8の既知差分台帳を空にした。
[退役前の全レコード](records/retired-known.v1.json)を保存し、固定した21行のreplayは残す。
最初のrunがstale-KNOWNを検出したexit 101も保存する。
後続のD/E・globalのcurrent oracle artifactsは公式writerで再生成し、全case/input/observationの不変を
[機械検証](refresh-provenance.py)する。歴史的before/after測定packetのhashを書き換えない。

## Hosted入口と予算

既存65 witness suitesを保持し、次の9 suitesを追加する。全jobは既存の2 workers・60分制限、
45分で分割再検討の方針を維持する。

| suite | 固定した観測範囲 |
| --- | --- |
| emitter-final | oracle対照4、EF2/EF3 21所属、EF2–EF6 batch11 tests、EF8 22＋4 commands、EF7 217＋guard3 tests |
| emitter-universe-oracle | TypeScript 6.0.3で217＋1798を各2回再観測 |
| emitter-plan-base-0..3 | sorted IDのmodulo 4、450/450/449/449行を各2回 |
| emitter-global | 既存global output-only 769行を各2回 |
| emitter-class-0 / -1 | 既存8 class bandsを700/528行に分割、各2回 |

重複する観測所属なので件数を足して互換ケース総数にしない。PLAN-BASEの約70分のローカル直列測定は
hosted一jobへ持ち込まない。専用fixtureは担当suiteを選び、shared sourceは全既存群を含める。
editing selectorsを除去し、zero/ignored testsと欠けたshard summaryを拒否する。
入口台帳v26はcomposite runnerを実Cargo commandsへ展開する。

## Validationとarchitecture

[run-local.py](run-local.py)はargv、開始HEAD/diff、環境、exit、時間、log hashを保存する。
macOS background priority・Cargo 2 workersで逐次実行し、canonicalのtargetをcacheとして利用する。
68 KNOWNの再比較、planner 82 tests、policy checkとfocused policy 2 testsは成功。
ローカル途中のhash drift失敗は、oracle修正後に旧pinを検出したもので記録から除外しない。
最終候補の成功とhosted receiptsは完了後に追記する。

現在変更中の11 concernsは `active-unqualified` に戻した。
[以前のqualification](records/architecture-before.v1.json)を保存し、最終immutable headの実測後に
変更した範囲だけ再qualifyする。E-PLAN-SCRIPTのoption admission、resolver/checker facts、
metadata/class provenance、async capture、name allocation、helper import、printer/commentが対象。
H2.9/一般checker全体、disposed-transform/API再emit、build/watch、TS7移行の完了には拡張しない。
STAGE・TypeScript 6.0.3 pin・製品全体のadmission policyは変更しない。
