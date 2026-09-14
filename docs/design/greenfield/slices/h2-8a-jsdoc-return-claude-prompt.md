# Claude への依頼：H2.8a G5c JSDoc return annotation

`/Users/hiramatsu/dev/tsc-rs-jsdoc-return` の `work/h2-8a-jsdoc-return` で作業してください。
runtime base は `7d6bc9848e97c26b0438da5ec73bd08c9a175c9a`（PR #521 の統合候補）です。
資料のコミットが上に載っていても、着手中の worktree を reset しないでください。

[依頼資料](h2-8a-jsdoc-return.md) と [固定入力・source inventory](h2-8a-jsdoc-return-selection.v1.json) を読み、
`node scripts/check-h2-8a-jsdoc-return-selection.mjs` で開始 source を確認してください。
この資料は調査・実装依頼であり、runtime-ready 設計や原因の実測証明ではありません。

対象は原ケース `jsDeclarationsFunctionsCjs.ts#default` の d/e が、外側の
`@return {string}` / `@return {T & U}` を失って `.d.ts` で `any` になる G5c です。
元の完全 input/options/TS tuple を固定してください。最新 base の strict exit 101 と同一の完全 captures ×2 は
[before 受領証](h2-8a-jsdoc-return-before.v1.json) に保存済みです。source が一致すれば重複した baseline 実行は不要です。
semantic checker と syntactic builder の JSDoc ownership の差を第一仮説として調べ、
annotation → single-return expression → semantic fallback のどこで誤選択するかを実証してください。

次に、上流 owner、Rust の型/関数/寿命、編集手順、正負対照と未解決0を設計 gate に固定し、
必要な原因閉包を実装してください。設計の具体化後は、依頼済みの範囲で修正と focused 検証を完了まで進めてください。
最新の JsString/JsStr 表現、generic identity、G4a/G4b コメント、parameter-tag lookup、
noEmit/declaration の既修復を保持してください。原 G5c と新 controls は全 command ×2、
回帰は資料 §6 の範囲で確認し、期待値変更・比較削減・skip で成功にしないでください。

同じ Mac の重い native 実行は1本、専用 target、低優先度、Cargo build jobs 2、test thread 1。
準備時点で前統合のローカル検証は終了し、重い実行枠は解放済みです。full CI / chain-walk を追加しないでください。
原因別 commit、最終 source と証拠、実 command/exit、未解決/別 owner を report に残し、branch を push してください。
PR と hosted acceptance は Codex が担当します。Claude 側で PR・merge・global profile 更新を重複実行しないでください。
