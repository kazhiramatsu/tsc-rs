# H2.8b-LR2 library order repair

2026-09-11。ユーザーからLR1で見つかった7件の不一致修正を明示的に依頼された。
既存productionのlibrary順序修正として実施し、B全体のprofile activationには進めない。
LR1の失敗受領証・12入力・期待値は保存する。

## Sourceと修正

`getDefaultLibFilePriority` (`_tsc.js:123124–123138`) をそのまま用いる上流probeで、
15個の正規化済みpath対を各2回観測した。`containsPath(..., false)`はrootだけをcase-insensitive、
後続成分をcase-sensitiveに比較する。既存`normalized_root_parts`を使い、Program loaderが
正規化済みの`ProgramPath`を渡すことで、余分なpath正規化の複製を避ける。

`LibraryCatalog::source_file_priority(source, default_library_directory)`を追加する。
包含確認後に解決後basenameを既存catalog順位へ写し、範囲外では`libs.length + 2`を返す。
`load_selected_libraries`と`process_lib_references`の両producerから呼ぶ。
canonical lookup keyではなくdisplay pathを参照する。root case・directory case・prefix隣接・
catalog外の既知basename・未知basename・ES6 alias・UNC/file URLは15ケースで固定した。
basenameのprefix/suffix除去は上流のoptionalな除去に合わせ、`/lib/es5.d.ts`と
`/lib/lib.es5`の2 pathも上流関数を各2回replayしてからunitへ追加した。

通常commandの追加6ケースは`h2-8b-library-order-inputs.json`と`h2-8b-library-order.json`。
上流6/6 complete ×2、診断0・exit0・4writes、source/library/root factsを保存した。
2置換の正逆順、root promotionと重複lib参照、catalog外のcatalog風basename、catalog内の別basename、
`/lib-extra`の境界を含む。7件だけを名前で特別扱いする修正にはしない。

## 検証と境界

元の12件と追加6件は通常command全タプル・Program factsの別testsで比較する。
順位probeはProgram構築とは別の15ケースであり、18件に足してProgram件数を水増ししない。
既存programのlibrary unit/loader contractsも実行する。共有comparatorと元12件の期待値を維持する。
既存loaderの手書き期待順序1件は、同一入力・optionsの上流観測で誤りを確認して訂正した。
その最初の実exit101と再観測・再実行結果は完了報告と受領証に残す。

sourceの`findSourceFileWorker`は最初に渡された`isDefaultLib`によってprocessing bucketを選び、
後からlibFiles membershipが増えても既存sourceを再挿入しない。追加promotionケースでこの境界も
確認する。`StagedSource.initially_library`で最初のbucketを保持し、新規libraryのstable sort後に、
promotionされたrootの元のpostorderを残す。任意の非library rootとの交差を含む
既存PreparedProgramのlibrary-prefix表現を一般の非prefix layoutへ拡張する作業は別途必要であり、
今回の18件でその到達性を網羅したとは主張しない。

検査の実結果・実exit・SHAは[最終受領証](../../../../ratchets/h2-8b-library-order-final.v1.json)と
[完了報告](h2-8b-library-order-report.md)に記録した。元12件・追加6件の完全commandとProgram factsは
各2回一致し、library units 5件・loader contracts 20件・compiler tests 4件が実exit0で通過した。
