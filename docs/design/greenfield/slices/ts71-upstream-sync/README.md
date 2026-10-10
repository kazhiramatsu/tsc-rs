# TypeScript 7.1 の本家への追従：計画（2026-10-11）

vendoring の commit（`19dadef8`、2026-09-29）から本家の TypeScript（microsoft/TypeScript）の main に追従する計画。利用者の
決定（2026-10-11）：API server（P5）の後、LSP の前に行い、再 vendor の前に計画を立てて確認する。P5 は LS の handler（6）・
`formatNodeForInsertion`・`preserveSourceNewlines`（P5-5b）を除いて終わった（[API server の packet](../ts71-api-server/README.md)）。

## 本家の差分（2026-10-11 に fetch）

- main の先頭は `aa8149273`（2026-10-09）で、`19dadef8` から 82 commit。7.x の tag は `v7.0.2` が最新で、7.1 の beta の tag は
  まだ無い（`[api] Prepare for beta release`（#64681）は入っている）。
- 分野ごとの commit の数（重なりあり）：checker 29、program と build 21、API と client 21、emit 15、LS 15、parser 10、標準
  library 6、diagnostics と翻訳 6。baseline を変える commit は 42。
- baseline の file（約 780）：追加 394、変更 363、削除 21、名前の変更 2。compiler 433、conformance 160、tsbuild と
  tsbuildWatch と tsc と tsoptions と config と transpile で約 90、fourslash（LS。対象外）約 80。
- 主な変更：source phase import（#63915。新しい構文 `import source x from "…"` と `lib.esnext.modulesource.d.ts`）、標準
  library（DOM と webworker の更新、esnext の iterator method と `Promise.allKeyed`）、compiler option の定義の生成（#64457）、
  declaration emit（re-export する module の index、reverse mapped type の保存、循環と省略の報告）、checker の pool の排他の
  取得、file path の型付け（#64159）。
- API（11 commit）：`getSymbol(decl)` と merged symbol の checker の method、callback FS の改善、client での binder の symbol の
  所有（source file が持つ）、同期の接続での入れ子の request への応答の順（stack の順）、config の無い program の project
  reference の diagnostics、customized な module resolution での disk の配置の import diagnostics の省略、async の dispose。

## 手順

- **S0 再 vendor**：`scripts/vendor_typescript_native.py --commit <sha> --profile 7.1.0-dev-<sha の先頭 8 桁>` で新しい profile を
  作り、`scripts/typescript7.py` で同じ commit の tsgo を作る。profile の名前を使う所（約 30：conformance と suites の runner、
  `crates/compiler/build.rs` の埋め込みの library、翻訳、生成物（diagnostics、option、encoder の表）、test、CI の
  `test_replay.py`、README）を新しい profile に替え、生成物を作り直す。新しい profile の ratchet は、そのときの状態（新しい
  baseline との違いを含む）で始める。
- **S1〜**：違いを分野ごとに直す（標準 library と message → parser（source phase import）→ checker → emit → program と
  build → API の method と client の test）。各 slice は今までと同じく full の conformance と suites で 0 regressions、ratchet は
  上げるだけ。
- **終わり**：古い profile（`vendor/typescript-native/7.1.0-dev-19dadef8` と ratchet）を除く。client の test は新しい commit の
  `packages/typescript` で走らせる。

## 決めること（利用者に尋ねる）

- 追従する commit：今の main（`aa8149273`）か、7.1 の beta の tag を待つか。
