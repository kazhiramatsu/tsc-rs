# 性能の計測とチューニング（2026-10-12）

利用者の指示（2026-10-12）：「niceなしでよいです。1プロジェクトごとでOKです。hono/zod/effect/typescript/next.js/vscodeを順番に
お願い」。ES5 などの削除（R1〜R3、`ts71-removed-features`）と identity domain の統一（R3b）の後、tsgo `aa8149273` と実時間で
比べ、その後チューニングする。

## 計測（head `68625b636` = R3b）

Apple M5（10 core、32 GB）、macOS 26.5.1。tsgo `7.1.0-dev` `aa8149273` と tsc-rs の release build を、1 project ずつ `nice` なしで
交互に走らせた中央値（hono・zod 7 回、effect・typescript・next 5 回、vscode 3 回）。variant は各 corpus の
`tsconfig.bench-{noemit,emit,decl,sourcemap,full}.json`、VS Code は `src/tsconfig.json` + `noEmit`。harness は session の
scratchpad の `perf/bench.py`（最初に出力の一致（診断・終了 code・出力した file の全 byte）を確かめ、違えば tsgo
`--singleThreaded` と tsc-rs の checker 1 つで比べ直す）。

| project | variant | tsc-rs | tsgo | 比 | tsc-rs CPU | tsgo CPU | tsc-rs RSS | tsgo RSS |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| hono | noemit | 0.124 s | 0.150 s | 0.83 | 0.57 s | 0.62 s | 304 MB | 276 MB |
| hono | emit | 0.135 s | 0.161 s | 0.83 | 0.64 s | 0.69 s | 318 MB | 297 MB |
| hono | decl | 0.140 s | 0.183 s | 0.77 | 0.73 s | 0.85 s | 333 MB | 325 MB |
| hono | sourcemap | 0.136 s | 0.166 s | 0.82 | 0.66 s | 0.73 s | 322 MB | 303 MB |
| hono | full | 0.148 s | 0.191 s | 0.77 | 0.76 s | 0.94 s | 338 MB | 334 MB |
| zod | noemit | 0.512 s | 0.814 s | 0.63 | 3.39 s | 4.26 s | 1323 MB | 1606 MB |
| zod | emit | 0.569 s | 0.896 s | 0.64 | 3.90 s | 5.02 s | 1357 MB | 1580 MB |
| zod | decl | 0.607 s | 0.952 s | 0.64 | 4.32 s | 5.44 s | 1513 MB | 1702 MB |
| zod | sourcemap | 0.577 s | 0.928 s | 0.62 | 3.96 s | 5.33 s | 1361 MB | 1682 MB |
| zod | full | 0.620 s | 0.995 s | 0.62 | 4.46 s | 5.63 s | 1532 MB | 1834 MB |
| effect | noemit | 0.469 s | 0.721 s | 0.65 | 3.09 s | 3.39 s | 1016 MB | 1091 MB |
| effect | emit | 0.583 s | 0.789 s | 0.74 | 3.75 s | 4.04 s | 1116 MB | 1182 MB |
| effect | decl | 0.712 s | 1.016 s | 0.70 | 4.78 s | 5.56 s | 1161 MB | 1602 MB |
| effect | sourcemap | 0.545 s | 0.863 s | 0.63 | 3.90 s | 4.42 s | 1116 MB | 1232 MB |
| effect | full | 0.758 s | 1.097 s | 0.69 | 4.93 s | 6.04 s | 1168 MB | 1640 MB |
| typescript | noemit | 0.317 s | 0.343 s | 0.93 | 0.94 s | 1.14 s | 290 MB | 379 MB |
| typescript | emit | 0.459 s | 0.503 s | 0.91 | 1.37 s | 1.80 s | 451 MB | 553 MB |
| typescript | decl | 0.465 s | 0.547 s | 0.85 | 1.43 s | 2.09 s | 473 MB | 639 MB |
| typescript | sourcemap | 0.473 s | 0.561 s | 0.84 | 1.41 s | 2.02 s | 444 MB | 597 MB |
| typescript | full | 0.506 s | 0.610 s | 0.83 | 1.54 s | 2.30 s | 475 MB | 640 MB |
| next | noemit | 0.715 s | 1.240 s | 0.58 | 4.22 s | 4.97 s | 1290 MB | 1535 MB |
| next | emit | 0.821 s | 1.370 s | 0.60 | 5.25 s | 5.98 s | 1316 MB | 1590 MB |
| next | decl | 0.950 s | 1.523 s | 0.62 | 6.46 s | 7.29 s | 1436 MB | 1911 MB |
| next | sourcemap | 0.879 s | 1.458 s | 0.60 | 5.71 s | 6.50 s | 1325 MB | 1652 MB |
| next | full | 1.010 s | 1.650 s | 0.61 | 7.04 s | 8.22 s | 1443 MB | 1994 MB |
| vscode | stable | 3.529 s | 4.518 s | 0.78 | 24.52 s | 22.20 s | 5499 MB | 6369 MB |

- 全 project・全 variant で tsc-rs が速い（0.58〜0.93）。RSS は hono を除いて少ない。VS Code は実時間が短いが CPU の合計は
  tsgo より 1 割多い。VS Code の段階：tsc-rs は parse 0.85 s・check 2.56 s、tsgo は parse 0.50 s + bind 0.22 s・check 3.68 s。
  program の読み込みだけ（`--listFilesOnly`）は tsc-rs 1.04 s（CPU 3.5 s）、tsgo 0.71 s（CPU 4.8 s）で、tsc-rs の loader は
  main thread の逐次の部分（graph の走査・module 解決・program の publish）が長い。
- **一致**：hono・typescript・next・vscode は全 variant で出力が全て一致。zod の診断は tsgo 自身の multi／single thread で違う
  （checker の分割に依る TS5115・TS2344）、single thread どうしは一致。effect は decl／full で 2 つの違い（下）。

## Effect の宣言 emit の 2 つの違い（PR #758）

- **`infer` の型 parameter の順**：tsgo は conditional type の `infer` の型 parameter を `CompareTypes`（名前の順）で並べる
  （checker.go:24332-24341）。true の枝の deferred な型参照はその parameter の mapper を持ち、`CompareTypes` はそれらを mapper で
  並べるので、その union（Effect の `RpcGroup` の `.middleware(...)` の後）は tag でなく payload の順になる。
- **node builder の `serializedTypes`**：tsgo の emit resolver は全ての request に 1 つの node builder（`requestNodeBuilder`、
  emitresolver.go:65-70）を使うので、ある enclosing declaration で一度直列化した型は後の request で位置のない copy として返る
  （nodebuilderimpl.go:3276-3290）。tsc-rs の cache は request ごとだった（521e3cc42 は builder を request ごとに作っていた tsgo
  19dadef8 に従った）。宣言 emit の `TransformArena` が cache を持ち、1 つの file の宣言 emit の request が共有する。表示
  （types writer・診断）は request ごとのまま（表示まで共有すると `conformance/fixSignatureCaching` の `.types` が後退した）。
  Effect の `McpServer` では先に作る static member が共有の literal を map し、後の `McpServer_base` は何も map しない。
- **test**：2 つの CLI contract（tsgo の byte、修正前のコードで失敗）。
- **結果**（release build、head `24122cc81`）：full の conformance（`--workers 2 --check`、477 s）は 0 regressions で R3b の report
  と全ての field で同じ。並列対照は記録済みの順序の 4 構成だけ。suites は 0 regressions、既知の違い 10。test 3,597 と clippy は
  通る。6 project の一致は 26 が一致、zod の 5 は分割に依るだけ（実の違い 0）。DefinitelyTyped は 9,067／9,067 一致（1,216 s）。
- **hosted と merge**：PR #758 の hosted run 38197536645（rust 8m52s、conformance 13m1s、gates pass、head `24122cc81`）、merge
  commit `d5200ae0a`。

## チューニング（PR #759）

- VS Code の profile（macOS の `sample`、全 thread）は平ら（1 つで数 % を超える所は無い）。小さな改善（interned name の
  `str` の検証を intern の時に一度だけ、node の core module の判定を interning なしで、path の相対 segment と拡張子の判定を byte
  で）は合わせて 0〜1.6 % で、測れる差はほぼ無い。
- profile の途中で見つけた一致の違い：`types` に `"*"` があると、tsgo は「install type definitions」の診断から「`types` に
  足す」の助言を外す（TS2580・TS2581・TS2582・TS2867、`UsesWildcardTypes`、checker.go:14187-14205・15413-15422）。port は
  常に TS2591・TS2592・TS2593・TS2868 だった。checker の options が `types` を持つ。
- **test**：`--types *` の CLI contract（tsgo の byte、修正前のコードで失敗）と `has_relative_segment` の unit test。
- **結果**（release build、head `46c54cf08`）：full の conformance（`--workers 2 --check`、487 s）は 0 regressions で #758 の report
  と全ての field で同じ。並列対照は記録済みの順序の 4 構成だけ。suites は 0 regressions、既知の違い 10。test は xtask 以外の全
  crate で 4,130 が通り、clippy も通る。6 project の一致は 26 が一致、zod の 5 は分割に依るだけ。DefinitelyTyped は 9,067／9,067
  一致（1,171 s）。R2・R3 で消した option の tsgo との並べた比較は 20／20、15／15。
- **hosted と merge**：PR #759 の hosted run 38202308283（rust 11m9s、conformance 21m40s、gates pass、head `46c54cf08`）、merge
  commit `e92da5703`。

## 残る候補（利用者の判断待ち）

- **loader の並列化**：VS Code の読み込みだけ（`--listFilesOnly`）で tsc-rs 1.04 s（CPU 3.5 s）、tsgo 0.71 s（CPU 4.8 s）。
  段階の計測（`TSRS_PHASE_TRACE`）では、root の read-ahead の parse（10 thread、270〜360 ms）の後、依存の read-ahead（190〜220 ms）
  は調整役の main thread で律速され、root walk（125 ms）・graph finish（42 ms）・program の publish（約 130 ms）は main thread で
  逐次に走る。tsgo の file loader のように worker の task が parse と module 解決をまとめて行う形にすれば VS Code で 0.2〜0.3 s
  縮む見込み（比 0.78 → 0.72 前後）。loader の構造の変更で、順序は一致（trace・package.json の lookup の順）に関わるので、
  利用者の判断を待つ。
- typescript（src/compiler）は check が `checker.ts` を 1 thread で検査する時間で決まり（tsc-rs 0.27 s、tsgo 0.28 s）、安い改善は無い。
