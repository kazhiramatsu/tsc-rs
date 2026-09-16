# OPS-COVER-3E — require rewrite dedicated commands

2026-09-16。C01 / A-INT1のliteral更新統合と同じ候補で実装する。
[統合記録](../../h2-8a-literal-update/integration/README.md)にsource・ローカル検証・hosted結果をまとめる。

`h2_8a_require_rewrite` の専用4 testsをcontrols jobの `require-rewrite` suiteへ登録した。
focused 60、composition 4、substitution 4、dynamic 6、計74完全commandを各2回比較する。
凍結fixtureとobserverは既存のまま。4 observerをgroup引数つき `--check` で再観測してからnativeを実行する。
入力JSON、expected JSON、observer、target sourceはこのsuiteの所有入力として扱う。

共有helper経由で実行済みという推論をせず、専用74入力の入口を追加する。
原本wrapperの2 IDとimportされた9 testsは除外し、Cargoには4 exact test名を渡す。
終了時に4 pass / 0 ignored / 10 filteredを要求し、0件・部分実行・余分な実行・異常終了は失敗にする。
内部の `TSC_RS_REQUIRE_REWRITE_FILTER` と追加capture環境変数を消去する。
共有comparatorとlibrary snapshotを変更した場合は、既存ownerに加えてこのsuiteを選ぶ。

既存acceptanceの原本replayやruntime admission、凍結expectedは変更しない。
C01の22 commandと本suiteの74 commandは入力経路と比較面を分けて記録し、製品のexact総数へ加算しない。
入口台帳v12は69 standalone中27 unfiltered / 11 filtered / 31直接入口なし。
残る直接入口なしはcompiler11 / その他20、lib/binは15。
