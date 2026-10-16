# 実 project の差分（2026-10-16〜）と引き継ぎ

2026-10-16 に Opus 5.5 から Fable 5.1 へ引き継いだ。この文書から始め、細部は各 packet の記録を読む：LSP の実 project の
差分 test は [ts71-lsp](../ts71-lsp/README.md) の「実 project の LSP 差分 test」（#822〜#824）、LSP の性能は
[ts71-perf-lsp](../ts71-perf-lsp/README.md)（#825・#826）。

## Fable 5.1 への命令

利用者（2026-10-16）：「fable5.1への命令は実プロジェクトの差分の修正とパフォーマンスチューニングです」。

1. **実 project の差分の修正**：既存の project の残り（§1）と、新しい frontend project の検証（§2）。
2. **性能の調整**：slice 3 の仕上げ（§3）と、その先（§4）。目標は下の指示 1 のとおり（LSP・noEmit・d.ts・JS emit で tsgo
   より速く、peak memory を少なく）。

差分の修正が先で、性能はその後（下の指示の順）。

## 利用者の指示（この順に進める）

1. 2026-10-16：「主要プロジェクトでの差分を修正してください。その後、パフォーマンス改善に進んでください。LSP、noEmit、d.ts、
   js emit でtsgoより速くなる、ピークメモリを少なくするのが目標です」
2. 同日、性能の作業の途中で：「差分はまだ完全に修正していませんね」。続けて「ひとまず差分修正に集中してください。
   パフォーマンスチューニングはfable5.1へバトンタッチして継続しますが、その前に差分を潰したいです。実プロジェクトでの
   検証を進めてください。astro,remix,nuxt.js,signal.jsなどのフロントエンドプロジェクトを中心に検証を追加してください」
   「signal.jsはreactの対抗馬のものです」「svelteも追加してください」。
3. 「この辺でfable5.1へバトンタッチします」。

つまり、まず差分を潰す（既存の実 project の残りと、新しい frontend project の LSP・CLI）。性能はその後。
「signal.js」は SolidJS（signal を軸にした React の対抗）と解釈して solid を入れた。利用者には確かめていない（Preact
Signals の意味なら足す）。

## 1. 既存の project の LSP の残りの差分

最新の記録（`~/dev/real-projects.noindex/lsp-compare/out-<name>`、tsc-rs は slice 2〜3 の build、tsgo aa814927）：

| project | 要求 | 違い | A | B | C |
|---|---:|---:|---:|---:|---:|
| hono（`src`） | 47,637 | 43 | 15 | 6 | 22 |
| vue（`packages/*/src`） | 41,256 | 11 | 5 | 4 | 2 |
| effect（`packages/effect/src`） | 89,249 | 68 | 未 | 未 | 未 |
| playwright-core（`src`） | 60,108 | 50 | 未 | 未 | 未 |
| zod・TypeScript compiler・next.js・vscode の `src/vs/base` | — | 0 | | | |

- A：tsgo の答えは再実行のどれでも同じで、tsc-rs だけが違う。B：tsgo の答えは揺れ、tsc-rs はその一つと同じ。C：tsgo は
  揺れ、tsc-rs はどれとも違う。hono は tsgo を 4 回（`out-hono`・`out-hono-tsgo1`・`out-hono-tsgo2`・`out-hono-5`）、vue は
  3 回（`out-vue`・`out-vue-tsgo1`・`out-vue-tsgo2`）比べた。結果は `lsp-compare/analysis-2026-10-16/{hono,vue}-stability.txt`。
- **#824 の「残りは全て tsgo 自身の揺れ」は誤り**：tsgo の答えは乱数の順で決まるが、tsc-rs の決まった順は tsgo では稀な
  結果を毎回選んでいる。tsc-rs は tsgo の最も出やすい結果を返すべき（次の節）。

### 原因（hono・vue に共通）：auto-import の specifier の cache と group の処理順

tsgo（`target/typescript7/upstream/tsc/internal/ls/autoimport`）：

- `View.GetCompletions`（view.go:179）は export を `grouped`（Go の map）にまとめ、`for _, exps := range grouped` で処理する
  （乱数の順）。group の中は `core.MinAllFunc(fixesForGroup, CompareFixesForRanking)`（core/core.go:357：各候補をその時の
  最小とだけ比べる）。
- `GetModuleSpecifier`（specifiers.go:8）の cache は import する file ごと（`registry.specifierCache[importingFilePath]`、開いて
  いる間続く）。鍵は `export.Path`（export を見つけた file）、値は `export.ModuleFileName` から計算した specifier。hit は
  元の kind が paths でも `ResultKindRelative` で返す。
- `Fix.IsReExport` は `export.Target.ModuleID != export.ModuleID`（fix.go:617）。target を持たない export も真になる。
- `compareModuleSpecifiersForRanking`（fix.go:1029）は、両方の kind が `Relative` の時だけ
  `isFixPossiblyReExportingImportingFile`（import する file を含む directory の index からの再 export を後ろへ）を比べ、
  その後に directory の区切りの数で比べる。これで比較が循環し得る：`.`（自分の package の index）は paths の fix に区切りの
  数で勝ち、paths の fix は相対 path に勝ち、相対 path は `.` に罰で勝つ。
- project の bucket の index は file を Go の map の順に入れ（registry.go:1305）、file の中も symbol table の map の順
  （extract.go:151）。同じ `ExportID` の export は group の中で merge され、後の export の `Path` が残る（view.go:213）。

tsc-rs（`crates/ls/src/autoimport/view.rs`）の `get_completions`（268 行）は group を見つけた順で処理する。それ以外は
tsgo の写し（cache の hit は `Relative`、`is_re_export` の癖、順位付け、`MinAllFunc`）。

**(a) hono**（43 件のうち 41 件）：middleware の index（jwt・language・request-id・secure-headers・timing）は
`declare module '../..' { interface ContextVariableMap ... }` を持つ。この augmentation の export は `Path` がその index、
`ModuleFileName` が `src/index.ts`（extract.go:154–170）。全 middleware の `ContextVariableMap` は一つの group に merge
され、`Path` は結果の順で最後の file のもの。その group が同じ file の通常の export の group より先に処理されると、
cache[その index] が `../..` になり、`timing` などの通常の export も `../..` から import される。
- tsgo では、汚れる file は乱数で決まる。汚れる確率は、通常の export が r 個なら 1/(r+1)（timing は 6 個で約 14%）。
- tsc-rs では、group が最初の augmentation（jwt）の位置に作られ、merge で最後の file（timing）の `Path` が残る。そのため
  毎回 timing を汚す。
- 41 件の内訳：
  - 35 件は、tsgo の最新の答えがどの file も汚さない（このうち A が 15 件）。
  - 6 件は、tsgo が別の file を汚した（language 2 件、request-id 4 件）。
  - 残る 2 件は B で、一つは hono の `Context<any, any, {}>` の hover。

**(b) vue**（A の 5 件）：runtime-core の `version`（`export const version`、index の最初の export）は、runtime-dom・
runtime-test・vue の index と、`vue/src/runtime`・`vue-compat/src/esm-index`・`esm-runtime` に `export *` で再 export
される。
- 違う completion は、どれもその file で auto-import を含む唯一の completion（cache は空）。
- tsc-rs は `version` の group を最初に処理する。そのため三つの index の path を `version` の group が最初に計算し、kind が
  paths になる。`.` との比較が循環し、`.` が先頭の順では相対 path の三つが残る。
- tsgo は 146 の label 全てで `@vue/runtime-dom`・`@vue/runtime-test`・`@vue/vue` を返す。
- 7 つの fix の全ての並び順で計算すると：
  - kind が全て `Relative` なら 100% この答え。
  - `version` の group が runtime-dom の path だけを最初に計算した場合でも 72.6%。
  - tsc-rs の状況（三つの index の path を全て最初に計算）では 17.9% で、tsc-rs の答え（相対 path の三つ）は 2.9%。
  - tsgo がその状況になるのは、三つの path を共有する約 150 の group のうち `version` が最初になる時だけ。
- 小さな再現：`lsp-compare/repro/vue-mini`（`packages/runtime-core/src/componentSlots.ts` の 1:5 で completion）。tsgo は
  `version <- .`、tsc-rs は相対 path の三つ。`vue-compat` と `vue/src/runtime.ts` を外すと両方 `.`。

### 直し方の案（未実装）

`View::get_completions` と `get_module_specifier`（view.rs:790）で、tsgo の最も出やすい結果を返す。tsgo の順は乱数なので、
どの決まった順も tsgo の可能な順の一つで、その中で最も出やすい結果を選ぶ。

- 処理の前に、この要求の export のうち cache に届くものを path ごとに数える。数えるのは：
  - 届くもの：package 名が無く、`unresolved_module_specifier` が無く、module id が ambient でなく、`GetFixes` で
    `tryAddToExistingImport` が先に返さないもの。
  - 数える値：group の数 n、通常の export と augmentation の export の数。
- cache に無い path を計算する時：
  - n ≥ 2：その path の多数派（通常の export）の module file 名から specifier を作って cache に入れ、kind は `Relative` で
    返す。tsgo では、別の group が先に計算している確率が (n−1)/n。
  - n = 1：今のまま（tsgo でも必ず最初）。
- これで hono の汚染（cache の値）と vue の循環（kind）の両方が tsgo の多数派になる見込み。
- 回帰 test（tsgo の多数派の答えで固定し、変更なしで落ちることを確かめる。crates/lsp/tests の auto-import の test の
  helper を使う）：
  1. vue-mini 相当：`@vue/*` の paths、`export *` の連鎖、`.` の index。
  2. hono 相当：`declare module '../..'` の augmentation と通常の export が同じ file。
- suites の lsp（4,583/4,584）に、この順に依存する baseline が無いかを見る。

### まだ見ていない差分

- effect の 68：#822・#823 の記録では auto-import の specifier が 67 で、同じ種類の見込み。ほかに `HttpApiSchema`/`Schema` の
  再 export。
- playwright-core の 50：
  - codemirror の `lint`/`overlayMode`：node_modules の bucket の merge で、後の export の `Path` が残る。
  - `registry`（#824 の記録）。
- hono の hover：tsgo は `Context<any, any, {}>`、tsc-rs は `Context<E extends Env = any, ...>`（B。#822 の記録では tsgo は
  5 回中 2 回、宣言の型引数を答えた）。
- vue の B・C：`findDir`・`resolveDirective` の候補の有無、`getCurrentScope <- .`、`TrackOpTypes` の specifier。
- slice 3 の build の vue は 11 件、slice 2 の build は 9 件。増えた 2 件が slice 3 の変更によるものかは未確認。
- 調べ方：
  - `stability.py OUT [tsgo の再実行の jsonl ...]`：A/B/C に分ける。
  - `show.py OUT KEY`：項目の違いを見る。
  - `both.py`・`lsp-compare.py probe --raw`：一つの位置を両方の server に尋ねる。
  - tsgo だけの再実行：`lsp-compare.py record --server <tsgo> --name tsgo --root <root> --files <glob> --out out-<name>-tsgoN`
    （`run.sh` の中身と同じ引数）。

## 2. 新しい frontend project（取得と依存の install まで済み）

置き場所は `~/dev/real-projects.noindex/frontend`（`clone.sh`、`install-<name>.log`）。取得は `git clone --depth 1`。依存は
`pnpm install --frozen-lockfile --ignore-scripts` で入れた（lifecycle script は走らせていない。pnpm は `packageManager` の版に
切り替わる）。

| repo | commit | repo 自身の型検査 |
|---|---|---|
| astro | `484822f`（2026-10-16） | root の `tsc -b`：solution の `references` で各 package。package の build は esbuild（`astro-scripts`）と `tsc -b`（d.ts） |
| remix | `b65325d`（2026-10-15） | package ごとに `tsc --noEmit`（typecheck）と `tsc -p tsconfig.build.json`（build、JS と d.ts）。repo は TypeScript 7.0.2 を使う |
| nuxt | `5adfcb1`（2026-10-16） | root の `vue-tsc --noEmit`（paths で package の src を写す）。build は tsdown |
| solid（SolidJS） | `25138e1`（2026-10-07） | package ごとの `tsc`（`types:*`。`tsconfig.build.json` は emitDeclarationOnly） |
| svelte | `40bb084`（2026-10-15） | `tsc --project tsconfig.runtime.json && tsc && cd tests/types && tsc`（JS と JSDoc。`check:tsgo` の script がある） |

比べる単位の案：

- astro：`-b` を両方で走らせる（outDir を分けた copy で）。または tsgo で d.ts を作ってから、package ごとに `-p --noEmit`。
- remix：58 package の `tsconfig.json` を `--noEmit`。`tsconfig.build.json` の emit は outDir を分けて比べる。
- nuxt：root の `tsconfig.json` を `--noEmit`（`.vue` は tsc の対象外）。`test/fixtures` は `nuxi prepare` が要る。
- solid：package の `tsconfig.build.json` の d.ts と、`tsconfig.json` の `--noEmit`。
- svelte：上の三つ（checkJs の比較になる）。

次にやること：

1. 比べる tsconfig の一覧を作る（`compare-projects.py --list` の形式で、一行に `<directory> <tsconfig>`）。
2. CLI の診断：`compare-projects.py --root <repo> --list <list> --tsgo <tsgo> --rs <tsc-rs> --out <jsonl>`。既定は
   `--noEmit`、reference は `tsgo --singleThreaded`、tsc-rs は `TSRS_CHECKERS=1`。
3. emit（d.ts・JS）：比べる道具はまだ無い。両方を `--outDir` を分けて走らせ、tree を `diff -r` する script を足す。
4. LSP：`lsp-compare/run.sh <label> <root> <glob>`。glob の例：
   - astro：`packages/astro/src/**/*.ts`
   - svelte：`packages/svelte/src/**/*.js`（JS の session）
   - solid：`packages/solid/src/**/*.ts`
   - remix・nuxt：`packages/*/src/**/*.ts`
   記録は大きくなる（next.js で 8 GB）ので、比べる時は一行ずつ読む。
5. 違いは checker・LSP・tsgo の揺れに分ける。tsgo の揺れは再実行で確かめる（A/B/C）。直したものは Rust の回帰 test に
   固定する。

## 3. 性能の slice 3（local のみ、未 push）

- **worktree と branch**：`/Users/hiramatsu/dev/tsc-rs-p34` の `perf/ts71-lsp-request-threads`、HEAD は `b121051dd`
  （origin/main `630f0cb24` を merge 済み）。commit：
  - c7e141653 lsp: requests run on kept threads（`crates/lsp/src/request_threads.rs`、test は
    `crates/lsp/tests/request_threads.rs`）
  - 86b7efdd8 checker: accessible symbol chains are cached（tsgo `accessibleChainCache`）
  - 378c96778 project, host, checker: the loader's threads read the build's files at once（snapshot の file cache の
    `RwLock`、build が見た file の記録の 16 分割、`CachedFs` と document registry の `RwLock`）
- **検査**（`b121051dd`、`target/claude-tools/final.sh`、log は `target/claude-tools/logs/perf3-b121051dd/`）：
  - conformance：0 regressions。
  - `--checkers 4`：既知の 4 件だけ（15,335 一致）。
  - suites：0 regressions（ratchet は変わらず。lsp 4,583/4,584、tsc 227/229）。
  - test は全て通過：types 49・syntax 251・binder 78・program 683・checker 1,806・emitter 350・ls 232・lsp 65・api 332・
    project 235・compiler 241・conformance 49・lsproto 31。
- **残り**：
  - API client の test（sync・async）、clippy（`-D warnings`）、`cargo fmt --all -- --check`。
  - PR の本文：下書きは `target/claude-tools/handover/perf3-pr-body.md`。`@MEASUREMENTS@` と `@CHECKS@` を埋める。
  - PR、hosted の検査、merge、性能 packet への記録。
  - PR の前に、同じ build で effect と playwright-core の差分を比べる。
- **計測済み**（`nice -n 20`）：
  - hover の中央値：`Null` は 47 → 34 µs（tsgo と同じ）、`Context` は 105 → 64 µs（tsgo 50）。
  - session（request threads と chain cache の build、lock の変更の前）：hono 38.7 s（tsgo 23.3）、vue 15.1 s（12.5）、
    effect 59.9 s（66.3）。
  - hono の全 289 file の診断（`each.py`）：13.8 → 11.2 s（tsgo 4.3 s）。

## 4. 性能（差分の後）

- 計画と記録は [ts71-perf-lsp](../ts71-perf-lsp/README.md)（計測の方法、起点の表、slice 1・2、次の候補）。
- tsgo との差が大きい候補：
  - hono の src と test の project の切り替え：program の作り直しと test file の検査。診断の合計は 14.2 s 対 4.7 s。
    作り直しは 75 ms 対 27〜29 ms（先読み 46・adopt 5.4・bind の予約 7.9（直列）・bind 2.9 ms）。
  - package の auto-import の bucket の最初の作成：vue で 0.15 s 対 0.06 s。alias resolver の要求を、checker が求めた時に
    解決する。
  - next.js の session の測り直し（以前は 1,246 s 対 324 s）。
  - memory（hono 1.12 GB 対 0.82 GB）。
- CLI の各 mode（noEmit・d.ts・JS emit）と peak memory の README の計測は、利用者と一緒に `nice` なしで行う。

## 5. 決まりと道具

- CLAUDE.md の決まり：
  - tsgo に合わせる時は既存の経路を変える。
  - tsgo の答えで unit test を書く。
  - 最後の bytes で全体の検査をする。
  - 負荷を低く保つ。
  - exit status を本当に見る。
  - PR の流れに従い、merge は merge commit だけ。
- conformance が見られない修正は Rust の回帰 test にする（2026-10-13 の利用者の指示）。
- commit は `git -c user.name=kazhiramatsu -c user.email=kaz_h0827@yahoo.co.jp commit`（trailer なし）。merge commit も
  `--amend --no-edit --reset-author` で同じ author にする。gh は `-R kazhiramatsu/tsc-rs`。
- tsgo は `target/typescript7/bin/tsc-aa814927`（本家 microsoft/TypeScript `aa8149273`）。source は
  `target/typescript7/upstream/tsc/internal`。repository は名前で呼ぶ（「上流」は tsc-rs の main と読まれた。「本家」と書く）。
- 全体の検査：`target/claude-tools/final.sh <worktree> <log dir>`（`tests.sh` を含む）。hosted の検査の待ち：
  `target/claude-tools/wait_gates.sh <PR>`（background で）。
- API client の test：
  - 以前は scratchpad の `client71b/packages/typescript` にあった。scratchpad（`/private/tmp`）は macOS が 3 日で消すので、
    無ければ `target/typescript7/upstream` から rsync で作り直す。
  - 実行：`node --conditions=@typescript/source --test --test-concurrency=1 --test-timeout=30000 test/sync/api.test.ts`
    （async も同じ）。
- 実 project の道具（`~/dev/real-projects.noindex`）：
  - `compare-projects.py`（CLI の診断）と `dt-compare.py`。
  - `lsp-compare/`：`lsp-compare.py` の `record`・`compare`・`probe`、`run.sh`、`time-run.sh`、`times.py`、`repeat.py`、
    `each.py`、`both.py`、`cases.py`、`classify.py`、`stability.py`、`show.py`、`bisect_files.py`。
- 負荷：
  - 重い仕事は一度に一つだけ。`nice -n 20`、`cargo -j 2`、worker は 1〜2。
  - 記録の jsonl は大きいので一行ずつ読む。
  - Python の出力を file に書く時は `python3 -u`。
- zsh：
  - 変数に入れた command は word-split されない。
  - `bash -c` の loop は止められる。
  - 前景の `sleep` の連鎖も止められる（until の loop で待つ）。
