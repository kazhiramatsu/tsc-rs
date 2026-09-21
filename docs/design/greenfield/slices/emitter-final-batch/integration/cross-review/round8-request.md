追加の共同レビューです。A1–A4 は提示した局所修正を実装し、27 対照を追加して観測中。root は B isolatedModules の全分岐を独立に精査します。あなたは C 15行のうち関連情報4行と JS 6行を読み取りだけで具体化してください。残件というラベルだけで広範囲と判断せず、既存関数の条件や省略を直す小修正で exact にできる行がないか調べるのが目的です。
参照: docs/design/greenfield/slices/emitter-final-batch/integration/cross-review/checker-residuals.json (入力全文と双方の診断)、vendor/typescript-6.0.3/lib/_tsc.js、crates/checker/src、binder。C の型深さ・表示5行は今回は不要。
出力: 原因関数と upstream/Rust の行、最小変更案、負の対照、既存基盤不足なら必要な基盤と波及範囲。実装やテスト実行、他 agent への委譲は禁止。ここまでの分類を再掲せず新しい具体的根拠を返してください。
