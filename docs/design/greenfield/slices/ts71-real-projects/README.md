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
「signal.js」は SolidJS（signal を軸にした React の対抗）。利用者に確かめた（「SolidJSで合っています」）。

## 1. 既存の project の LSP の残りの差分

最新の記録（`~/dev/real-projects.noindex/lsp-compare/out-<name>`、tsc-rs は slice 2〜3 の build、tsgo aa814927）：

| project | 要求 | 違い（#827 の前） | A | B | C | #827 の後 |
|---|---:|---:|---:|---:|---:|---:|
| hono（`src`） | 47,637 | 43 | 15 | 6 | 22 | 4（全て B） |
| vue（`packages/*/src`） | 41,256 | 11 | 5 | 4 | 2 | 3（全て B） |
| effect（`packages/effect/src`） | 89,249 | 68 | — | — | — | 0 |
| playwright-core（`src`） | 60,108 | 50 | — | — | — | 50（codemirror の 5 label、label ごとに B） |
| zod・TypeScript compiler・next.js・vscode の `src/vs/base` | — | 0 | | | | 0 |

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

### 修正 1：auto-import の group の順（PR #827、2026-10-16、Fable 5.1）

- **merge**：#827 → `0ca4b8aec`（hosted run 38911087732）。commit：c07baee0f（共有 file の specifier を先に埋める）、85292efd1（package の export は entrypoint の順）。
- **tsgo の順の性質**：Go の map の走査は、要素が少ない map（package の数 file）では挿入順にほぼ従う（2 要素なら 7/8 が
  挿入順）。要素が多い map（group の数千、project の file の数百）では hash 次第で、run ごとにほぼ一様な乱数。したがって
  tsc-rs の決まった順は、小さい map では tsgo の挿入順、大きい map では「tsgo の最も出やすい結果」を狙う。
- **共有 file の specifier**（`crates/ls/src/autoimport/view.rs` `fill_shared_specifiers`）：要求の group のうち、import する
  file の specifier cache を読む export（package 名が無く、未解決の specifier でなく、ambient でなく、既存の import に
  足せない）を file（`Export.path`）ごとに数え、2 つ以上の group が届く file は、どの group より先に、その group が与える
  module file 名の多数派から specifier を計算して cache に入れる（tsgo では、どの group が最初に計算するかは一様に
  乱数で、他の group が先に計算している確率は (n−1)/n）。1 つの group だけが届く file は今までどおり（tsgo でも必ず
  その group が計算し、kind は本来のもの）。
  - hono：middleware の index の `declare module '../..'` の export（path はその index、module file は `src/index.ts`）が
    index 自身の export より先に計算して `../..` で汚していた 41 件が消えた。
  - vue：`version` の group が三つの package の index の path を最初に計算して paths の kind になり、罰のある `.` と
    順位の比較が循環していた 5 件が消えた。
- **package の export の順**（`crates/ls/src/autoimport/node_modules.rs`）：`PackageExtraction.exports` と
  `BucketExtraction.exports` は path の `BTreeMap` から、entrypoint の順の `Vec` にした。tsgo は package ごとの map を
  entrypoint の順（main の file が先、次に directory の探索順）に埋めて Go の順で読む。main の file と addon の
  augmentation の両方が宣言する export は一つの group に merge され、後の export の path が specifier を決める（tsgo は
  addon の側が 8 回中 6 回）。tsc-rs は path の順で main の file（`index.d.ts`）が最後になり、常に main の側だった
  （playwright の `@types/codemirror` の `overlayMode`）。
- **test**（lsp。tsgo aa814927 の最頻の答えで固定、どれも変更なしでは落ちる）：
  `auto_imports_name_a_file_from_its_own_exports_despite_an_augmentation`（二つの middleware が `'../..'` を augment：
  汚れない。tsgo は file ごとに 8 回中 6 回）、`auto_imports_rank_a_shared_paths_specifier_as_cached`（`@v/*` の paths の
  六つの file が再 export する `version`：三つの package。8 回中 7 回）、
  `auto_imports_name_an_export_an_addon_augments_by_the_addon`（main と addon の augmentation が宣言する `overlayMode`：
  addon の specifier。8 回中 6 回）。test の helper `auto_import_items_in` は開く file を指定できる。
- **session**（tsgo aa814927、`nice -n 20`。違いは tsgo の再実行と突き合わせて A/B/C に分けた）：

  | project | 要求 | 前 | 後（2 回） | tsgo の run | 残りの種類 |
  |---|---:|---:|---:|---:|---|
  | hono（`src`） | 47,637 | 43（A 15・B 6・C 22） | 4・10（全て B） | 6 | その run で tsgo の順が汚した middleware |
  | vue（`packages/*/src`） | 41,256 | 11（A 5・B 4・C 2） | 5・3（全て B） | 5 | その run で tsgo が最初に計算した group |
  | effect（`packages/effect/src`） | 89,249 | 68 | 11・11（全て B） | 4 | `Annotations`（`Schema.ts` の宣言と `HttpApiSchema.ts` の augmentation の merge：後の file の path。2 file の順は 1/2） |
  | playwright-core（`src`） | 60,108 | 50 | 50・50（label ごとに B） | 4 | `@types/codemirror` の `on`/`off`/`signal`/`overlayMode`/`lint`：57 file の map の順（tsgo の 4 run で label ごとに答えが割れる） |

  playwright の 50 件は、5 つの label の組み合わせが run ごとに違うので答え全体では C になるが、label ごとには tsc-rs の
  答えは tsgo のどれかの run の答え（`lint` は 7 file のうち tsc-rs は `yaml-lint`。4 run では css・json）。
- **検査**：13482a6db（origin/main 84d5ec7fa の上）で release binary を作り直し、`nice -n 20`・worker 2：conformance 0 regressions、`--checkers 4` は既知の 4 件（15,335 一致）、suites 0 regressions（ratchet は変わらず、lsp 4,583/4,584、tsc 227/229）、全 crate の test（types 49・syntax 251・binder 78・program 683・checker 1,806・emitter 350・ls 232・lsp 67・api 332・project 235・compiler 241・conformance 49・lsproto 31）、clippy（ls・lsp、`-D warnings`）、fmt、API client sync 356/356・async 365/365。
- **道具**（`~/dev/real-projects.noindex/lsp-compare`）：`stability.py OUT [tsgo の jsonl…]`（A/B/C）、`show.py OUT KEY`
  （項目の違い）、`items.py SERVER ROOT FILE LINE CHAR [label の接頭辞…]`（一つの server の auto-import の項目）、
  `replay.py SERVER ROOT GLOB KEY`（session の file の接頭辞を再生して、答えが変わる最短の接頭辞を二分探索。
  `lsp-compare.py record --file-list` と `--until`）、`repro/{vue-mini2,hono-mini,cm-mini}`（tsgo を繰り返し走らせた
  小さな再現）。

### 残りと見方

- hono の hover `Context<any, any, {}>.header`（B）：tsgo は 4 回に 1 回ほど宣言の型引数の形を答える。新しい session と、その
  file 自身の前の要求を再生した後では両 server とも具体化した形。前の file の要求が query checker に残す状態が次の対象
  （`lsp-compare/replay.py` で session の file の接頭辞を二分探索できる）。
- tsgo の bug：augment するだけの file から import する specifier（hono の `../..`、effect の `./http-api/HttpApiSchema.ts`、
  codemirror の addon）。本家に該当 issue は無く（2026-10-16 時点。関連の #64821 は preference 変更時の cache 消去で別件、
  `tsc/internal/ls/autoimport` の source は本家 main と vendored commit で同一）、下書きは
  `target/claude-tools/handover/tsgo-autoimport-issue.md`（利用者の判断待ち）。
- **再実行 2026-10-17（main `02e340443` の bytes、#830 の後、recorder は ATA を待つ）**：hono 18（completion 17 + hover 1。11 回分の tsgo 記録との
  照合で全部 class B、17 件は tsc-rs が tsgo の最頻の答え（11 回中 7〜10 回）、残る hover は `Context<any, any, {}>.header` で tsgo が 6/5 に割れる）、
  zod 0、effect 0、vue 1（B）、playwright 50（codemirror の label、C）、TypeScript compiler 0、VS Code base 0、next.js 0（229,939 request）、
  solid 0、svelte 49（completion 47 B + definition 1 = tsgo の `@this` bug + hover 1 C）。astro 0・nuxt 13（C）・remix 0 は修正 4 の通り。
  log：`target/claude-tools/logs/rerun13/`（`handover/rerun13-chain.sh`、8 project は前回と同じ file 列 `lsp-compare/lists/*.txt` で `rerun.sh --file-list`）。
- 調べ方：`stability.py OUT [tsgo の再実行の jsonl…]`（A/B/C）、`show.py OUT KEY`（項目の違い）、`items.py`（一つの server の
  auto-import の項目）、`lsp-compare.py probe --raw`、tsgo だけの再実行は `lsp-compare.py record --server <tsgo> --name tsgo
  --root <root> --files <glob> --out out-<name>-tsgoN`。

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

### 修正 2：frontend project の compiler・emitter の差分（PR #828、2026-10-17、Fable 5.1）

- **merge**：#828 → `8c6468703`（hosted run 38956397408）。commit：fcb612110（emit：書き換えた specifier の引用符、line comment の
  次の export specifier）、1ef5d7d6e（check：`@overload` の型引数、`this` property の文脈型、`T & object` と array の
  comparable、TS2694 の module 名）、058a275d7（program：未 build の参照先出力に redirect される root、package scope の順）、
  a65f19659（check：overload の template の型引数は symbol 一つのまま、comment の table に置く）。
- **比べたもの**（tsgo aa814927、`nice -n 20`。`frontend/compare-all.sh <label> both`：各 tsconfig の `-p --noEmit` の診断と、
  outDir を分けた emit の tree）：astro 30・remix 100・nuxt 4・solid 17・svelte 6 の 157 tsconfig。前（base1）：診断の違い
  5（astro 1・svelte 4）、emit の違い 76（remix 99・astro 2・svelte 3 → remix は引用符だけで 863 file）。後（base3）：
  診断 157/157、emit 157/157 が同一。
- **六つの種類と直し方**（どれも tsgo の経路に合わせて既存の code を変えた）：
  1. **書き換えた specifier の引用符**（remix）：`rewriteRelativeImportExtensions` が `'./a.ts'` を `"./a.js"` にしていた。
     tsgo は literal の TokenFlags（single quote）を写す。`relative_imports.rs` `rewrite_literal` は flag が無ければ source の
     引用符を読む。CommonJS の `require('./a.js')` も同じ。
  2. **line comment の次の export specifier**（remix `fetch-router`）：`export { a, a as b, // shorthand\n c }` で次の行の `c`
     の前に space が付いていた。tsgo は sibling の space を comment の前に書く。named import/export の worker は行頭なら
     space を書かない（他の list の worker は既にそうしていた）。
  3. **field の `@type` と `this.x = …`**（svelte 3 file）：initializer の無い field に JSDoc の型があると代入の文脈型を失い、
     literal 型が広がり（TS2322）、arrow の仮引数が implicit any（TS7006）、`push` が TS2345 になっていた。tsgo の
     `d.Type()` は reparse した `@type` を含む（`contextual.rs`）。
  4. **`@overload` の型引数**（svelte `derived`）：overload の `@template` を signature の locals に宣言した後で signature
     の container の bind が table を作り直し、`T` が見つからない（TS2304）か実装側の型引数に解決して呼び出しが
     TS2345/TS7006 になっていた。template の型引数の symbol は一つのまま（checker の overload signature はその型の
     instantiation なので、symbol を増やすと `.types`・d.ts の名前が `T_1` に割れる：最初の版が
     `jsDocGenericOverloads`・`jsFileFunctionOverloads2`・`jsFileMethodOverloads2`・`jsdocVariadicInOverload` を落とした）、
     comment（JSDoc node）を key にした table にも置く（host の無い comment ではそこだけ：
     `hosted::overload_template_comment`）。overload 内の名前は signature の locals → comment の table → host を飛ばして
     host の container の順で解決する（tsgo の reparse した overload 宣言は host の隣。`resolve.rs` `advance_walk`、
     last location は host）。実装側の locals は見えないので、`@overload` の後ろに置いた `@template` は実装が同名を
     宣言していても TS2304（`overloadTag4`）。
  5. **`T & object` を `any[]` に cast**（svelte proxy）：intersection の source と array の target の comparable 判定で
     target の optional property の追加検査に入っていた。tsgo は tuple だけでなく array も除く（`isArrayOrTupleType`）。
  6. **TS2694 の module 名**（svelte）：`import('est').Decorator` の message が `'est'`。tsgo は symbol の escaped name
     `'"est"'`（`annotate.rs` `get_fully_qualified_name`）。
  7. **未 build の参照先出力に redirect される root**（astro `markdoc`）：参照 project が持つ file を `include` が含むと root は
     その project の出力 `.d.ts` に redirect され、未 build なら tsgo は file も診断も出さない（TypeScript 6.0.3 は TS6305。
     本家への issue 候補、`handover/upstream-tests-pr.md`）。`resolve_root_sources` で落とす。package.json の scope は file を
     読む前に引く（tsgo `parseTask.load` の順）ので、tsbuildinfo の `missingPackageJsons` も一致する。
- **test**（どれも変更なしでは落ちる。program の probe の assert は astro の tsbuildinfo の一致で確かめた）：checker
  `jsdoc_overload_signatures_keep_their_template_type_parameters`、
  `jsdoc_overload_signatures_do_not_see_the_implementation_s_type_parameters`、
  `js_this_property_assignments_are_contextually_typed_by_the_jsdoc_type`、
  `a_generic_intersection_is_comparable_to_an_array_type`、`import_type_namespace_errors_print_the_module_s_quoted_name`；
  emitter `rewritten_module_specifiers_keep_their_quotes`、`export_list_item_after_a_line_comment_starts_its_line`；
  program `a_root_redirected_to_an_unbuilt_output_is_dropped_silently`。
- **本家の test 形式**：同じ 5 件を tsgo の test 形式で書き、tsgo の出力を記録した（`target/claude-tools/handover/upstream-tests/`）。
  利用者が「test case の不足」の issue と PR を本家に出す（`upstream-tests-pr.md`）。conformance の corpus にはどれも無い
  形（`rewriteRelativeImportExtensions` の test は全部 double quote、`@overload` の test は実装側にも同じ `@template`、…）。
- **検査**：a65f19659（origin/main 7a5e280ff の上）で release binary を作り直し、`taskpolicy -c maintenance nice -n 20`・
  worker 1（利用者の就寝中）：conformance 0 regressions（13,577 full、emit 13,569 full・mismatch 0、5,083 s）、
  `--checkers 4` は既知の 4 件（15,335 一致）、suites 0 regressions（ratchet 変わらず）、全 crate の test（types 49・syntax 251・
  binder 78・program 684・checker 1,811・emitter 352・ls 232・lsp 67・api 332・project 235・compiler 241・conformance 49・
  lsproto 31）、fmt。clippy は hosted の `rust` job。058a275d7 での最初の full run は overload の修正の第 1 版で 4 件
  regress（`.types`・d.ts の `T_1` 改名）、a65f19659 で解消。関連する既存の conformance case 23 件（`jsFileFunctionOverloads*`・`jsFileMethodOverloads*`・`jsDocGenericOverloads`・`overloadTag1-3`・
  `templateInsideCallback`・`jsdocVariadicInOverload`・`jsdocTemplateTag*`・`jsdocImportTypeNodeNamespace`・
  `annotatedThisPropertyInitializerDoesntNarrow`・`rewriteRelativeImportExtensions*`）は個別に full。
- **道具**：`~/dev/real-projects.noindex/emit-compare.py`（outDir を分けて両方で emit し tree を比べる。違う tree は
  `frontend/work/<repo>-<label>/` に残る）、`frontend/compare-all.sh <label> [diag|emit|both]`、`frontend/<repo>-projects.txt`、
  `lsp-compare/repro/{svelte-js,overload,overload2,overload3,overload4,derived,comparable,comparable2,astro-refs,…}`、
  `handover/upstream-tests/run-case.py`（本家形式の case を tsgo で走らせる）。
- **残り**：frontend repo の LSP session（astro・svelte・solid・remix・nuxt、`lsp-compare/run.sh`）、hono の
  `Context<any, any, {}>.header` の hover（B、tsgo 3/4）。

### 修正 3：svelte・SolidJS の LSP session の差分（PR #829、2026-10-17、Fable 5.1）

- **merge**：#829 → `22b7f8413`（hosted run 38984723179）。commit：3b4827895・41aaf229c（checker・ls の 12 種類）、759a7e8fe（述語の
  開き名、cast の symbol、`@deprecated:`）、2dfb3d32b・ed5624d75（symbol walker の reparse 後の子、lsp test harness の `send_and_close`）。
- **比べたもの**：svelte 40bb084 `packages/svelte/src/**/*.js`（368 file、`lsp-compare/run.sh svelte … 40`、
  `TSRS_WORKERS=1 TSRS_CHECKERS=1`、`nice -n 20`）。recorder は `.js` を `languageId: javascript` で開くよう直した
  （前の run は両 server とも TS として開いていた）。前：61,659 request のうち 387 が違う。後：50（ed5624d75。completion 48 = 下の class B、hover 1、definition 1）。solid（23 file、3,824 request）：前 1 → 後 0。
- **種類と直し方**（どれも tsgo の経路に合わせて既存の code を変えた）：
  1. **documentSymbol 286**：`@import` の binding と inline `@typedef` の type literal の member は、tsgo の reparse が
     host の前に置く statement の symbol として、typedef 自身と host より前に出る（`symbols.rs`）。
  2. **module specifier の ending 54+17（completion・hover・signatureHelp）**：`@import` 宣言は static import の先頭
     （tsgo の `file.Imports()` は reparse した JSImportDeclaration を host の前に数える）。svelte compiler の
     `/** @import { Context } from '../types' */` が最初の相対 import なので ending は minimal。`import("…/constants")`
     の表示と、`fly`/`migrate` の auto-import の二重（`…/transition/index.js` と `…/transition/public.js` が
     separator 数で同点 → tsgo も MinAll で両方残す経路。minimal なら `../transition` が勝つ）も同じ原因（`specifier.rs`
     `module_name_literals`）。
  3. **semanticTokens 12**：`@property` で書いた typedef の member（JSDocPropertyTag）は property（`semantic_tokens.rs`）。
  4. **typeDefinition 4（JS method）**：`is_thisless*` が構文上の注釈しか見ず、`@returns {string}` の JS method が
     this-mapper で instantiate され、symbol が `type.symbol` と一致せず戻り値型への redirect が効かなかった。tsgo の
     reparse は `@returns`・`@param`・`@template` を method の注釈にするので effective な注釈を読む（`annotate.rs`）。
  5. **`@type` cast の narrowing 3+（hover・typeDefinition・references）**：`/** @type {T} */ (x)` は tsgo では `as` 式で
     narrowing に不透明。`flow.rs` の reference 判定で cast の括弧を reference にしない。
  6. **documentHighlight 4・hover 1（`@returns {asserts name is T}` / `{name is T}`）**：述語の `name` は host 関数の
     parameter に解決する（tsgo の reparse は `@returns` の型を関数の戻り値注釈にするので `lastLocation === location.type`。
     `resolve.rs`：JSDocReturnTag を last location に持つ FUNCTION_SCOPED_VARIABLE は見える）。hover の述語表示も同じ原因。
  7. **foldingRange 4**：`@this` のある関数は `(` から畳む（`folding.rs`）。
  8. **hover の docs 3**：`@overload` のある JS 関数の value declaration は tsgo では最初の reparse した overload なので
     docs が無い（`ls/jsdoc.rs` `tsgo_value_declaration`）。
  9. **diagnostic 2（TS6387）**：`signatureToString` は `noErrorTruncation` を見ない（160 文字で `...`、`display_signature_string`）。
  10. **references 2（`Object.prototype.toString`）**：JS class の `@implements` は tsgo の reparse で implements clause に
      なる。`is_date` の `Object.prototype.toString` から、`ICompileDiagnostic` を実装する `CompileDiagnostic.toString` へ、
      interface の見かけの `Object` member 経由で届く（`node_util.rs` `get_all_super_type_nodes`）。
  11. **signatureHelp 5（JS fallback の接頭辞）**：receiver に call signature が無い呼び出し（`generator._sources.toArray()`、
      `entry.unsubscribe()` の `Function`）は tsgo の `createJSSignatureHelpItems` が program 中の同名宣言（lib が先）を使い、
      `SymbolToStringEx(symbol, 宣言の file, None, UseAliasDefinedOutsideCurrentScope)` で接頭辞を作る。ALLOW_ANY_NODE_KIND の
      無い `symbolToString` は tsgo では `symbolToName`（enclosing から見た symbol chain）だが、こちらは `symbolToEntityNameNode`
      （`symbol.parent` を素朴に辿る、`any` の alias 表示用）を使っていたので `__global.IteratorObject.toArray`・`__type.unsubscribe`
      になっていた（`serialize.rs` `build_symbol_display_node`）。
  12. **signatureHelp 1（`render` の `transformError?:`）**：`src/internal/server/index.js` の `@template Props` と
      `@param {{ …; transformError?: (error: unknown) => unknown }}`。`isTypeParameterPossiblyReferenced` が `@template` の
      型引数の container を JSDocTemplateTag にしていたため、`@param` 型の中の function type から辿り着けず「参照されるかも」
      → 型を instantiate → 注釈の型と別物 → 注釈を再利用できず `((error: unknown) => unknown) | undefined`。tsgo の reparse は
      型引数を関数の下に置くので container は host（typedef/callback の tag、overload の comment も同じ規則、
      `instantiate.rs`）。
  13. **documentHighlight 7（`@returns {x is T}` の述語名）**：plain な述語の名前は tsgo では参照にならない（`asserts x is T` はなる）。
      tsgo は JSDoc の名前を reparse した clone 経由で解決するが、clone を位置の二分探索で探す `ast.GetReparsedNodeForNode` は
      clone の根と同じ位置から始まる node（型の先頭 token）を取りこぼし、comment 側から解決して parameter が見えない。
      checker の表示は clone 経由で解決するので 6. の規則はそのまま、services の symbol lookup（`getSymbolAtLocation`）だけが
      `@returns` の型を開く名前に symbol を返さない（`location.rs` `opens_hosted_return_type_predicate`）。
  14. **documentSymbol 12（`@type` cast・`@param` の型 literal）**：`/** @type {T & { toJSON?: any }} */ (value)` は tsgo の `as` 式で、
      navigation bar は式の後に型を辿る。変数初期化子の cast（`const e = /** @type {…} */ (x)`）と、関数式・arrow を初期化子に
      持つ変数の `@param {{ … }}` 型（tsgo は初期化子の子を `forEachChild` で辿るので parameter の reparse した型も）も同じ。
      symbol walker の `children()` が reparse 後の子（parameter の `@param` 型、cast の型）を含む（`symbols.rs`）。
  15. **solid の completion 1（`@deprecated:`）**：TS file の JSDoc は tsgo では遅延 parse で、deprecated の印は scanner の
      文字列走査（`@deprecated` の次が空白・`}`・`*`・末尾：`hasJSDocTag`）。`@deprecated:` は印にならない（JS file は eager
      parse でどの `@deprecated` tag も印）（`parser.rs` `comment_has_deprecated_tag`）。
- **残り（bounded）**：
  - solid の `src/{reactive,render}/*.d.ts`（`.ts` と同名の生成 `.d.ts`。`tsconfig.json` の include は `.ts` を優先するので
    どの project にも属さず inferred project）：run ごとに 2〜10 件が違い、両 server とも run 間で揺れていた。原因は ATA
    （自動型取得）の timing：inferred project は両 server とも background で global typings cache の `@types` を取り込み、
    取り込み前の答え（global completion 1,141 件・lib symbol の references 347 件）と後の答え（1,159 件・13,213 件）は
    両 server で一致する（`lsp-compare/ata-probe.py`：tsgo は open の約 1.1 s 後、tsc-rs は約 0.6 s 後に取り込む）。recorder の
    request がその間に届いていた。recorder は open ごとに「Installing types」の progress の終わりを待つようにした
    （`lsp-compare.py settle_types`、`window.workDoneProgress` を initialize に追加）→ solid の最終 session は 0 件。
    （前に書いた「lib が node_modules/typescript/lib 由来」は誤り：`HTMLParamElement` の deprecated が消えるのも typings
    （`@types/*` の再宣言）が加わった後の状態。）
  - completion 48（auto-import の specifier の種類と再 export の罰則）：file で最初の auto-import 補完は tsgo でも実行ごとに
    答えが変わる（class B）。tsgo の specifier cache は importing file ごと・export の file 単位で、cache hit は常に
    `ResultKindRelative`、fresh な計算は `paths`/`imports` なら `ResultKindPaths`。`isFixPossiblyReExportingImportingFile`
    の罰則は両方 Relative のときだけ効くので、最初の補完で group がどの順で cache を埋めるか（Go の map 順）で
    `#compiler` が 2 件か 1 件か、`svelte` か `../index-client` かが変わる。再現 `repro/ambient-self`（`src/compiler/use/a.js`）
    を tsgo で 3 回走らせると最初の補完は 3 通り、2 回目以降は安定。こちらは 2 回目以降と同じ答え（全部 cache hit）を返す。
    最頻の答えを返す設計（PR #827 の `fill_shared_specifiers` の拡張）は別 slice。
  - hover `Value<V = unknown>.equals`（1）：checker の状態依存。単独で record すると両 server とも `Value<V = unknown>`。
    tsgo の session では、`sources.js` より前に開いた file の check が `Value<V>` の member を `equals` の型解決前に
    instantiate していた（partition が違えば答えが変わる）。class C。
  - definition `this`（1）：tsgo の bug（hosted `@this` の parameter 名の Loc が 0 → targetRange 0:0）。こちらは正しい範囲を返す
    （2026-10-16 の auto-import の決定と同じ扱い）。
- **test**（どれも変更なしでは落ちる）：lsp server `document_symbols_of_a_javascript_file_s_jsdoc_tags`、`a_function_with_a_this_tag_folds_from_its_parameters`、
  `semantic_tokens_of_a_property_tag_typedef_s_members`、`a_deprecated_signature_s_message_is_cut_despite_no_error_truncation`、
  `a_jsdoc_import_decides_a_javascript_file_s_specifier_ending`、`javascript_signature_help_fallback_prefixes_name_the_declaring_container`、
  `a_jsdoc_template_is_not_referenced_by_a_function_type_in_a_param_type_literal`、`document_symbols_of_reparsed_jsdoc_type_literals`、
  `a_deprecated_tag_followed_by_a_colon_does_not_deprecate_a_typescript_declaration`；ls definition `a_javascript_method_s_type_definition_is_its_return_type_s`
  （+ TS 版）、hover `javascript_return_type_predicate`・`javascript_overloads_have_no_documentation`・`javascript_instantiated_qualifier_has_no_defaults`、
  references `a_jsdoc_asserts_predicate_names_its_parameter`・`a_jsdoc_implements_tag_relates_a_method_to_the_interface_s_apparent_member`・
  `a_plain_jsdoc_predicate_does_not_name_its_parameter`；checker `a_jsdoc_type_cast_is_opaque_to_narrowing`。
  lsp の fixture harness は `send` の直後の `close` を 1 つの lock で行う（`Session::send_and_close`）：hosted で `exit` 通知と入力終端の
  race（tsgo の `Run` も read loop の io.EOF と dispatch loop の cancel の早い方）に負けて `EXIT 1` になった。server は変えていない。
- **検査**：759a7e8fe で `scripts/conformance_ts71.py --workers 2 --check` exit 0（12,829 case、lane A 13,577/13,577 full、mismatch 0、
  ratchet 0 regressions）、`--checkers 4` + `conformance_ts71_compare.py`：15,335 一致・4 違い（既知の partition 依存 4 件）；
  ed5624d75（その後の commit は crates/ls・crates/lsp・test のみ）で `scripts/suites_ts71.py --update` → `--check` exit 0（5,315 baseline、
  0 regressions、ratchet 不変）、lsp server 38/38（exit の 2 test は 25 回連続 pass）、ls 240/240；他 crate は 759a7e8fe で全 pass；fmt clean。
  hosted gates（rust・conformance・plan）pass。
- **道具**：`lsp-compare/summarize.py OUT [per] [maxdiffs]`、`lsp-compare.py language_id()`、repro
  `lsp-compare/repro/{svelte-ls*,hl-order,sem-tokens,ls-tests,spec-ending,imports-alias,dts-pair,svelte-transition,value-default,equals-alias,iterator-global,js-fallback,render-sig,tostring-refs,pred-hl,deprecated-colon,cast-symbols*,ambient-self,inferred-ata}`、
  `lsp-compare/ata-probe.py`（ATA の前後の答えと progress・log を見る）、recorder の `settle_types`（open ごとに「Installing types」の progress の終わりを待つ）、
  `packages/svelte-scratch`（svelte package の tests 抜きの rsync copy、`src/server/index.d.ts` を v2 に単純化）。

### 修正 4：astro・nuxt・remix の LSP session の差分（PR #830、2026-10-17、Fable 5.1）

- **merge**：#830 → `02e340443`（hosted run 39007760258）。commit：601c504de（program）、121931ed8（checker）、e42f344ed（ls・lsp test）。
- **比べたもの**：astro 484822f `packages/astro/src/**/*.ts`、nuxt 5adfcb1・remix b65325d `packages/*/src/**/*.ts`
  （`lsp-compare/run.sh <name> … 40`、`TSRS_WORKERS=1 TSRS_CHECKERS=1`、`nice -n 20`、recorder は open ごとに ATA の終わりを待つ）。
  前：astro は全 request が「no project found」、nuxt 31、remix 1（hover の panic）。後（e42f344ed）：astro 0（680 file、113,109 request）、nuxt 13（507 file、85,414 request）、remix 0（796 file、118,095 request）。
- **種類と直し方**（どれも tsgo の経路に合わせて既存の code を変えた）：
  1. **astro：全 request が失敗（program loader）**：参照 project の source を読む program は出力 `.d.ts` を source に redirect する。
     redirect の subtask が親の package id を継いでいたため、collect walk の package 単位の重複除去が同じ package の後続の出力の
     redirect を飛ばし、出力名で bind した resolution の持ち主の source が無かった。tsgo の redirect 済み parse task は package id を
     持たず、bind 時に `getSourceFileWithRedirect` で出力を source に引き直す（`loader.rs`）。
  2. **astro：TS file の JSDoc 本文の `@import`（documentSymbol 1）**：tsgo が `@import` の binding を symbol に出すのは reparse で
     statement になる JS file だけ。「CSS @import statements」という comment が `statements` という symbol になっていた（`symbols.rs`）。
  3. **nuxt：二度 augment された module 越しの definition・references（6）**：`vue` の 2 つ目の augmentation は、`getExportsOfModule` の
     cache が既に埋まった transient module にその場で merge され、import は merge 前の export-star の先を引いていた。merge 後に
     cache を消す（`merge.rs`・`links.rs`）。
  4. **nuxt：宣言 file の `@typedef` の `@property` 型（references 1）**：TS file では tsgo は JSDoc を reparse しないので typedef の型は
     host の下のままで host の type parameter が見える。webpack-dev-middleware の `middleware.d.ts` の `Response` が global に解決され
     global の references に数えられていた。typedef への hop は JS file だけ（`resolve.rs`）。
  5. **nuxt：signatureHelp が tsgo の「... 4 more ...」を省かず全 member を出す（1）**：tsgo は late-bound property 名を
     `"\xfe@<name>@<symbol id>"` で測り、id は 2^48 始まりで常に 15 桁。こちらは 2 桁と仮定していた。概算長は `name + 18`
     （`type_nodes.rs`。計装した tsgo と property ごとの長さ 76/132/149/170 が一致）。
  6. **nuxt：auto-import の specifier `./internal/index`（completion 1）**：`processEnding` の minimal ending は、module file の
     directory（`sourceFileName.Directory()`）に directory と同名の file があるときだけ `/index` を残す。こちらは specifier の綴り自体を
     current directory 基準で引いていたので、nuxt の root の `internal.d.ts` が `packages/kit/src/internal/index.ts` の `/index` を残し、
     宣言 file の `./internal/renderer` と同点になっていた。`process_ending`・`validate_ending`・`try_get_module_name_from_paths`・
     `try_get_module_name_from_root_dirs` が module file 名を受け取り、tsgo が host を渡す所では host も渡す（`specifier.rs`）。
  7. **remix：hover の panic「should always get typenode」（1）**：再利用する型注釈の `import()` が `node_modules` 経由でしか名前の
     付かない module（pnpm の入れ子 `@redis/client` の RESP 型）を指すと unsafe import が報告される。tsgo の再利用の recovery
     boundary は error を記録し、`finalizeBoundary` で `encounteredError` を元に戻して、その property を import 無しで新しく
     serialize する（`NumberReply<number>`）。こちらの boundary は flag を戻さず、builder が node を返さず panic した（`mod.rs`
     `SyntacticRecoveryBoundary`）。
- **残り（bounded）**：nuxt の completion 22 は tsgo 自身の揺れ：webpack の `bundle` 9 件は class B で tsc-rs は tsgo の最頻
  （3 回中 2 回）と同じ；`#app` 13 件は tsgo が 3 回で 3 通り（class C）。最終 session の 13 件は `packages/nuxt/src/app/` の file ごとに 1 件（その file で最初の auto-import 補完）：`index.ts` が再 export する symbol を tsgo は run ごとに違う部分集合で `#app`（alias 経由の index）か `#app/*`（宣言 file。`createError` は `#app/composables` と `#app/error` の 2 件）で名付ける（3 run で 3 通り、一度は `../../../../nitro-server/src/runtime/server`）。こちらは常に宣言 file（tsgo の同じ file の 2 回目以降の補完と同じ）。最頻の答えの模倣は既出の別 slice（利用者に確認）。
- **test**（どれも変更なしでは落ちる）：lsp server `a_referenced_project_s_output_resolves_to_its_source_after_another_output_of_the_package`、
  `an_import_s_definitions_include_an_augmentation_merged_after_another`、`document_symbols_of_a_typescript_file_skip_jsdoc_import_bindings`、
  `an_index_specifier_is_kept_only_for_a_sibling_named_like_its_directory`；ls hover
  `a_jsdoc_typedef_in_a_declaration_file_sees_its_host_s_type_parameters`、`a_reused_import_type_through_node_modules_falls_back_to_a_fresh_type`；
  checker の specifier の unit test は module file 名を渡す。
- **検査**：e42f344ed で `scripts/conformance_ts71.py --workers 2 --check` exit 0（12,829 case、lane A 13,577/13,577 full、mismatch 0、emit 13,569 full / 0 mismatch、ratchet 0 regressions、499 s）、`--checkers 4` + `conformance_ts71_compare.py`：15,335 一致・4 違い（既知の partition 依存 4 件）、`scripts/suites_ts71.py --update` → `--check` exit 0（0 regressions、ratchet 不変）、全 crate の test pass（types 49、syntax 251、binder 78、program 684、checker 1812、emitter 352、ls 242、lsp 80、api 332、project 235、compiler 241、conformance 49、lsproto 31）、fmt・clippy clean。hosted gates（rust・conformance・plan）pass。
- **道具**：repro `lsp-compare/repro/{index-ending,unsafe-import,jsdoc-typedef}`、`handover/instrument-node-builder.py`（node builder の
  `return Ok(None)` と property 長の一時 trace）、計装した tsgo（`nodebuilderimpl.go` に同じ trace、build 後に元へ戻した）。

### 修正 5：webpack の JavaScript（probe 6 file）の差分（PR #831、2026-10-17、Fable 5.1）

- **merge**：#831 → `06dd75112`（hosted run 39029179330）。commit：c387f9b30（binder・checker）、14c0308f3（ls・lsp test）。
- **比べたもの**：webpack e12e1224 の `lib/{Compilation,Compiler,NormalModule,javascript/JavascriptParser,cache/PackFileCacheStrategy,util/createHash}.js`
  （6 file、960 request、`lsp-compare/rerun.sh webpack-probe … lists/webpack-probe.txt 40`）：12 件違う → 修正後 0（`lib/Compilation.js:documentSymbol`
  の 1 件は後述の順序の差）。全体（`lib/**/*.js` 790 file、134,174 request、tsgo 694 s・tsc-rs 699 s）：156 件 — 下の「残り」。
- **種類と直し方**（どれも tsgo の経路に合わせて既存の code を変えた）：
  1. **object typedef の `@property {T=}` の型が検査されない（checker）**：tsgo の reparser は tag から property signature を作り、宣言の検査で
     optional 型を解決する（`getTypeFromTypeNode` の JSDocOptionalType）ので内側の参照の誤りが出る。こちらは tag の型式の検査が `T=` で止まっていた。
     そこでしか使わない `@import` の binding が未使用（TS6196/6192）になり、値だけの import を型に使った TS2749 も出なかった（`check.rs`）。
  2. **class 本体の `@typedef` は file のもの（binder・ls）**：tsgo の parser は reparser が作った type alias / import 宣言を、source file と block の
     statement list 以外の list（class・object literal の member、parameter、switch clause）から外へ出し、host を含む statement の前に置く
     （`parseListIndex`）。typedef は module の export になり（未使用にならない）、inline type literal の member は file の先頭側の symbol、
     tag 自身の symbol は host の位置のまま。`jsdoc_statement_host` が binding の container を選び、symbol walker は statement で部分木の
     reparse 分を出す（`node_util.rs`・`bind.rs`・`symbols.rs`）。
  3. **`this.x = …` の `@type` と arrow の `@returns` の documentSymbol（ls）**：reparser は代入の binary expression に文の `@type` を、arrow に
     `@returns` を戻り値注釈として付け、navigation bar は子として辿る（`symbols.rs children()`）。
  4. **`this.x = …` の文の `@deprecated` は deprecate しない（binder）**：tsgo の deprecate は JSDoc host の node flag（`IsDeprecatedDeclarationWithCachedFlags`）で、
     代入の宣言はそれを合成しない。tag から modifier flag は作られない（`node_util.rs get_jsdoc_modifier_flags_no_cache`）。
  5. **入れ子の `@param options.x desc` が property の docs（ls hover）**：`reparseJSDocTypeLiteral` は parameter tag も property signature にし、tag を
     その JSDoc にする（`ls/jsdoc.rs`）。
  6. **`@callback` 型の parameter 経由の呼び出しの definition は parameter だけ（ls definition）**：tsgo は callback を function type に reparse し、
     `tryGetSignatureDeclaration` はそこへ行かない（`definition.rs`）。
- **残り（全体 session 156 件、次の slice）**：hover 66（`Array<T>.length` 等の qualifier 37 = tsgo の状態依存の疑い、再実行で分類中；
  `TypedHookMap<RecordFactoryFromTuple<…>>` の intersection の要素数 16）、signatureHelp 24（同じ intersection）、references 17（入れ子 cast の内側を tsgo が
  数えない、memo 10）、definition 15（JSDoc `@overload` の 2 つ目への位置、`0:0`）、documentHighlight 13（cast 括弧への代入の Read/Write、入れ子 cast）、
  diagnostic 10（TS1095/TS2483 = memo 8、CommonJS の `let` の TS2454、空 alias `{}` の表示、TS6205 の位置、TS2322/2345 の文言）、documentSymbol 4、
  semanticTokens 4（入れ子 `@param` の property token）、typeDefinition 2、completion 1。詳細は memory とこの packet の次の記録。
- **test**（どれも変更なしでは落ちる）：checker `an_object_typedef_s_optional_property_type_is_checked`；lsp server
  `a_jsdoc_import_used_only_in_an_optional_property_type_is_used`、`a_typedef_in_a_class_body_is_hoisted_to_the_file_s_statements`、
  `document_symbols_of_an_assignment_s_type_and_an_arrow_s_returns_type`、`a_this_assignment_under_a_deprecated_comment_is_not_deprecated_for_completions`；
  ls hover `a_nested_param_tag_documents_its_property`、definition `a_call_of_a_jsdoc_callback_typed_parameter_goes_to_the_parameter_only`；binder の
  `jsdoc_visibility_tags_contribute_effective_modifier_flags` は deprecated flag が無いことを確かめる。
- **検査**：14c0308f3 で focused の 37 case（typedef・import・deprecated）全部 full；`scripts/conformance_ts71.py --workers 2 --check` exit 0（12,829 case、lane A 13,577/13,577 full、mismatch 0、emit 13,569 full / 0 mismatch、ratchet 0 regressions、510 s）；`--checkers 4` + `conformance_ts71_compare.py`：15,335 一致・4 違い（既知の partition 依存 4 件）；`scripts/suites_ts71.py --update` → `--check` exit 0（0 regressions、ratchet 不変）；全 crate の test pass（types 49、syntax 251、binder 78、program 684、checker 1813、emitter 352、ls 244、lsp 84、api 332、project 235、compiler 241、conformance 49、lsproto 31）；clippy（binder・checker・ls・lsp）・fmt clean。hosted gates pass （14c0308f3 と、main を取り込んだ eda368a92 の両方）。
- **道具**：repro `lsp-compare/repro/{webpack-mini,webpack-tests,typedef-prop,fn-type-tag,alias-empty,nested-cast,let-assigned,w2-cli,w2-cli2,w2-ls}`、
  `lsp-compare/rerun.sh`（file list で記録）、`lists/webpack-probe.txt`、`handover/webpack-chain1.sh`。tsgo memo 7〜11 はこの session から。

### 修正 6：webpack の全体 session の差分、その 2（PR #832、2026-10-17、Fable 5.1）

- **merge**：#832 → `112754ba2`（hosted run 39039629967）。commit：78c493e8a（binder・checker）、4ec5077ce（ls・lsp test）、58d917681（binder の cast）、ba4ff875b（診断 printer）。
- **比べたもの**：webpack e12e1224 の `lib/**/*.js`（790 file、134,174 request）。#831 の後の 156 件（tsgo の 3 回の記録で全部 class A）→
  修正後 113 件（tsc-rs だけ記録し直して `out-webpack` の tsgo の記録と比べた）。
- **種類と直し方**（どれも tsgo の経路に合わせて既存の code を変えた。test は変更なしでは落ちる）：
  1. **reparse した `@overload` は host の symbol を持つ（binder・ls definition）**：tsgo の `declareSymbol` が overload 宣言に method の symbol を
     付け、`symbolMatchesSignature` がそれを読むので、呼び出しは解決先の overload へ行く（`hash.digest("hex")` が 1 つ目へ行っていた、definition 12 件）。
     test `a_call_goes_to_the_jsdoc_overload_it_resolves_to`。
  2. **`@this` が `@type` より前なら parameter は型なし（binder hosted）**：reparser.go の `noTypedParams` はそこまでに reparse した parameter を読み、
     `@this` が型付きの `this` を先に足している（memo 9、`Generator.js` の `generateError`：TS7006 ×3 と hover・definition・completion・
     typeDefinition・semantic token）。test `a_this_tag_before_a_function_s_type_tag_leaves_its_parameters_untyped`。
  3. **cast の括弧への代入は名前の Read（checker `access_kind`）**：tsgo の `accessKind` に `as` 式の arm が無い（`(load) = undefined`、`(len)++` の
     documentHighlight）。test `an_assignment_to_a_jsdoc_cast_is_a_read_of_the_name`。
  4. **reparse した注釈の文法検査 TS1095・TS2483（checker）**：setter の `@returns` は戻り値注釈、for-of 変数の `@type` は型注釈として文法検査に
     かかる（memo 8）。test `reparsed_setter_returns_and_for_of_variable_types_are_grammar_errors`。
  5. **TS6205 の範囲は typedef の `@template` tag 列（checker unused）**：tsgo の reparse した type parameter list は最初の tag の `@` から最後の
     tag の終わり（次の tag の `@`）までで、`rangeOfTypeParameters` が前後 1 文字広げる。こちらは type parameter の範囲だった
     （`Serializer.js`、`util/serialization.js`）。test `all_type_parameters_unused_spans_a_typedef_s_template_tags`。
  6. **CommonJS module の top-level `let` は mutable local（checker）**：`isParameterOrMutableLocalVariable` が「global file」を ES module だけで
     判定していたので、他の関数で代入される `let` が assigned と記録されず、外側の使用が TS2454（`ScopeAnalyzer.js` の遅延 require の
     `NodeType` 252 件）、型なしの `let` は implicit any（TS7034/7005）でなく possibly undefined（TS18048）になっていた。規則を
     `is_mutable_local_variable_declaration` に一本化。test `a_common_js_module_s_let_assigned_in_another_function_is_assumed_initialized`、
     `a_common_js_module_s_untyped_let_used_in_another_function_is_implicitly_any`。
  7. **無い property の message は空の type literal を alias 名で（checker）**：tsgo の `reportNonexistentProperty` は `typeToString(containingType)`
     そのまま。こちらの `{}` 特別扱い（3 つの helper）を外した（`AssetGenerator.js` の 'EmptyGeneratorOptions'）。
     test `a_missing_property_names_an_empty_type_literal_s_alias`。
  8. **入れ子 `@param options.x` は property token（ls semantic tokens）**：reparser の property signature が宣言（`state.ast`、`targetProperties: tp`）。
     test `semantic_tokens_of_nested_param_tag_properties`。
  9. **documentSymbol：host の型は代入先の object より前、`@template` の constraint、`@returns` は body の後（ls）**。
     test `document_symbols_of_jsdoc_template_constraints`、`document_symbols_of_a_this_assignment_s_type_before_its_object`。
  10. **cast の括弧は narrowable reference でない（binder）**：tsgo の binder の `isNarrowableReference` に reparse した `as` 式の arm が無く、cast 越しの
      property 読みに flow node が付かない。constructor の `/** @type {Scope} */ (upper).variableScope` は TS2565 にならない（cast 無しの
      `upper.depth` は tsgo もなる。TypeScript 6.0.3 はどちらも報告しない、memo 12）。test `a_property_read_through_a_jsdoc_cast_is_not_a_flow_reference`。
  11. **再利用した注釈の届かない名前は error 文字列で bare（checker の診断 printer）**：tsgo の typeToString は UseAliasDefinedOutsideCurrentScope を
      常に立て、symbolToTypeNode の chain は module root を出さない。こちらの再利用の復旧は出していた（spread した他 module の options の
      `amd?: import("…/WebpackOptions").Amd`、pure TypeScript でも同じ）。test `a_spread_property_s_unreachable_alias_prints_bare_in_an_error`。
- **残り（113 件、全部 class A）**：hover 63（`Array<Module>.length` 等の qualifier を宣言の形 `Array<T>.length` で出す 43 件 = session の順序依存、1 file なら両方とも instantiate 形 `repro/w2-hover`；`TypedHookMap<…>` の intersection 要素数 16 件 = signatureHelp 14 件・NormalModuleFactory.js の TS2322 文言 4 件と同じ推論の差 `ParserOptions | [EmptyParserOptions]` 対 `[EmptyParserOptions]`；切り詰め 2 件 htmlMinify.js；JS の generic `@returns {ParserByType[T]}` が呼び出しの hover で instantiate されない 2 件 `repro/w2-generic`、TS 版は一致）、signatureHelp 24（intersection 14、`@typedef` の property の function type を呼ぶ label に callee 名が無い 4、`middleware.serialize` の候補選択 3、`ContextDependencyHelpers.PartialContextDependencyOptions` の alias を tsgo が保つ 2、`values: Values` 1）、references 17・documentHighlight 4（入れ子 cast、memo 10；cast の中の arrow の `@param`）、definition 3（`@this` 関数の `this`、0:0、memo 3、利用者の決定で現状維持）、semanticTokens 1（syntax-printer.js の `toplevel.*`：JS の generic arrow（`@template T`）を context-sensitive な callback で呼ぶと tsc-rs は T = any、`repro/w2-infer/d.js`、次 slice の最初）、diagnostic 1（NormalModuleFactory.js、上記）。
- **検査**：branch 先頭 ba4ff875b（commit 78c493e8a・4ec5077ce・58d917681・ba4ff875b）、tsgo aa814927、log は `target/claude-tools/logs/webpack2c/`（chain）と
  `webpack2-final/`（`final.sh`）、nice 20・`cargo -j 2`・worker 2。focused 114 case（JSDoc の typedef/import/overload/`@this`/template/cast と、`{}` の
  無い property・JS の TS2454/TS2565・TS6205 を印字する baseline）は全部 full・mismatch 0。`conformance_ts71.py --workers 2 --check` exit 0：12,829 case、
  lane A 13,577/13,577 full、mismatch 0、emit 13,569 full / 0 mismatch、ratchet 0 regressions（507 s）。`--checkers 4`（exit 0）+ compare：15,335 一致、
  4 件は既知の partition 依存（incorrectRecursiveMappedTypeConstraint・mutuallyRecursiveInference・typeParameterWithInvalidConstraintType・recursiveMappedTypes）。
  `suites_ts71.py --update` → `--check` exit 0、0 regressions、ratchet 変更なし。全 crate の test 合格（types 49、syntax 251、binder 78、program 684、checker 1,821、
  emitter 352、ls 246、lsp 87 = batch 86 + 単独で通る watcher test、api 332、project 235、compiler 241、conformance 49、lsproto 31）。clippy（binder・checker・ls・lsp、
  all targets、`-D warnings`）と fmt は clean。hosted gates は 39039629967。
- **道具**：`handover/webpack2-chain.sh`（release build → focused 87 case → tsc-rs だけの再記録と比較）、repro
  `lsp-compare/repro/{w2-sym,w2-unused,w2-hover}`、`scratchpad` の t2454/talias の変種。

### 修正 7：webpack の全体 session の差分、その 3（PR #833、2026-10-17/18、Fable 5.1）

- **merge**：#833 → `a9d85213b`（hosted run 39049920116）。commit：6d38b7c63（ls）、2d1d9d1da（checker）、a8cfe702c（binder・checker・ls）。
- **比べたもの**：webpack e12e1224 の `lib/**/*.js`（790 file、134,174 request）：#832 の後の 113 件 → 修正後 92 件（tsc-rs だけ記録し直して
  `out-webpack` の tsgo の記録と比較）。preact 630a87e（`src`・`hooks`・`compat`・`debug`・`test-utils`・`jsx-runtime` の JS、33 file、
  5,307 request）：3 件（全部 `@this` 関数の `this` の definition で tsgo が 0:0 を返すもの、memo 3、利用者の決定で有効な範囲を返す）。
  SvelteKit dd2d975（`packages/kit/src/**/*.js`、382 file、50,333 request）：3 → 1 件（`error` の completion の sortText は 4. で直した。
  残り：object literal の union の hover 1 = property の順序が session に依存（1 file の session なら両者一致）。auto-import `Redirect` の差は再記録では出なかった）。ESLint 8ffeb8c（`lib/**/*.js`、`tsconfig.base.json` を継ぐ session 用
  tsconfig、389 file、68,905 request）：12 → 12 件（key は file の診断単位なので、TS18048 ×2 を 5. で直しても `eslint.js` の key は別の文言差で残る。残り：tsgo が
  namespace の member を container か alias 経由で修飾する診断文言 6 file（`Linter.Config<RulesConfig>`・`ESLint.Options`・`Scope.Reference`・`Rule.CodePathSegment`・
  `Rule.NodeParentExtension`・`AST.SourceLocation`。原因判明、次の PR）、長い object literal の文言の切り詰め位置の差 4 file（`source-code.js`・
  `no-restricted-globals/imports/modules.js`）、constructor の `this[SYMBOL] = …` への tsgo だけの TS2323 ×3 = memo 14、alias の hover に `@overload` の
  comment が出ない 1（原因を絞った、次の PR））。
  prettier 344d6dc（`src/**/*.js` を含む session 用 tsconfig、541 file、79,819 request）：5 件（型の無い `@property` に tsgo が出す TS7045 の
  suggestion ×6 = 1 件、object literal を自分の中で印字する時に tsgo が `...` にする TS2339/TS2551 の文言 3、`ArrayLike<T>.length` の hover 1 = session
  順序、`@this` の definition 0:0 = memo 3）。prettier の run 1（prettier 自身の tsconfig、168 件）は JSDoc の計測ではない：除外 file が inferred project
  になり、tsc-rs の ATA が `@types/core-js` 等を取り込む差（別件）。
- **種類と直し方**（どれも tsgo の経路に合わせて既存の code を変えた。test は変更なしでは落ちる）：
  1. **cast の中身の JSDoc は位置探索から届かない（ls `astnav`、利用者の決定 2026-10-17「入れ子castはtsgoに合わせてください」）**：tsgo の reparser は
     括弧の中身を `e as T` に置き換え、`as` node は自分の JSDoc を持たず位置は `e` の full start なので、`getTokenAtPosition` は `e` 自身の先頭 JSDoc を
     飛ばす。入れ子 cast の型名に hover/definition/references が無く、型位置の `null` は keyword references・highlight に出ず、cast の括弧の中の arrow の
     `@param` も highlight に出ない（`ModuleConcatenationPlugin.js`・`syntax-printer.js`・`lib/index.js`。TypeScript 6.0.3 は全部届く、memo 10）。
     `start_of_node` が hosted cast の operand の JSDoc を start に含めない。test `a_nested_cast_s_jsdoc_is_out_of_reach_for_highlights`、
     `a_nested_cast_s_type_name_has_no_hover`。
  2. **contextual な signatureHelp の label に typedef の property 名（ls）**：`@type {Utils}` の object literal の method の parameter list の中で、
     tsgo の `chooseBetterSymbol` は function type から reparse した property signature へ上がる。こちらではその tag が宣言（`ImportPhase.js` の
     `isEvaluation(phase)`）。test `javascript_contextual_signature_help_names_the_typedef_property`。
  3. **generic な JS arrow が context-sensitive な callback から推論する（checker）**：`isTypeParameterPossiblyReferenced` は hosted な JSDoc 型の中の
     node から型 parameter の container まで上る。tsgo の reparser はその型を host の関数の下に置くので関数に届くが、こちらは元の comment を
     VariableStatement・file まで上って arrow に会わず、comment の中の type literal を全部「参照の可能性あり」にしていた。`@param {(program: { kind: string }) => T}` の
     `{ kind: string }` が `T` を outer type parameter に持ち、推論 context の fixing mapper で instantiate した時点で `T` が any に固定され、callback の
     戻り値からの推論が効かなかった（webpack `syntax-printer.js` の `readSource(…, (program, …) => adoptEstree(…))` が any：property token も error も
     出ない。function 宣言が host なら正しかった）。JSDoc comment から host の function-like host へ続けて上る。
     test `a_generic_javascript_arrow_infers_from_a_context_sensitive_callback`。
  4. **`@overload` の 1 つが `@deprecated` でも関数は deprecated でない（binder・checker）**：tsgo の reparse した overload 宣言は parser の context flag しか
     持たないので、completion の「全宣言が deprecated」は成り立たない。こちらでは `@overload` tag の名前を deprecated にせず、binder が export された
     関数の local symbol にも overload 宣言を載せる（module 内の completion が読む方）。SvelteKit の `error`：sortText `11`・tag 無し。
     test `a_function_with_one_deprecated_overload_is_not_deprecated_for_completions`。
  5. **入れ子 tag の property 名は最後の識別子（checker）**：tsgo の reparser は `@param options.x` の property signature を右端の識別子で名付ける。こちらは
     rest 要素が property を除外する時に書かれた qualified name を式として検査し、ESLint の `lintText` が自分の `@param [options.filePath]` に
     `'options' is possibly 'undefined'` を出していた。test `nested_param_tags_of_an_optional_object_parameter_are_not_checked_as_expressions`。
  6. **contextual な signatureHelp に `@callback`・function 型 `@typedef` の alias 名（ls）**：`chooseBetterSymbol` が function type の親から type alias へ
     （`GetExportsFromDataFn(data, …)`）。2. の test に含む。
- **bounded（利用者の決定 2026-10-17「@importの件はboundedで残してOKです」）**：`@import X, { T } from` の default 名が X の references に出ない
  （webpack の 5 file、小さい再現では tsgo も出す、memo 13）。
- **残り**：92 件（全部 class A）：hover の `Array<T>.length`/`Set<T>.size`/`Map<K, V>.size` の qualifier 43（session の順序依存、1 file なら両方 instantiate 形）、`TypedHookMap<…>` の intersection の hover 16・signatureHelp 14 と `NormalModuleFactory.js` の TS2322 文言（原因判明：tsgo の reparse した `@typedef` は type alias なので、その型の中の参照は deferred な型参照になり、conditional type の instantiate ごとに新しい `SyncBailHook<…>` ができて同じ表示の 17 member が残る。tsc-rs は 12 に併合していた。次の PR）、signatureHelp の label 6（`Serializer.js` の候補選択 3：tsgo は `WebpackError.serialize({ write }: ObjectSerializerContext)`、tsc-rs は `HookWebpackError.serialize(context: …)`；`create(Dep, …)` 2：tsgo は alias `ContextDependencyHelpers.PartialContextDependencyOptions` を保ち、tsc-rs は `Partial<Omit<…>>` に展開；`parseObjectExpression` 1：tsgo は `values: Record<string, …>` に展開し、tsc-rs は alias `Values` を保つ）、切り詰め 2（`htmlMinify.js`）、JS generic の `@returns {ParserByType[T]}` が呼び出しの hover で instantiate されない 2、bounded の 8（`@import` の default 名 5 = memo 13・利用者の決定、`@this` の 0:0 definition 3 = memo 3）。
- **検査**：a8cfe702c で（release build、nice 20、`cargo -j 2`、worker 2）：`conformance_ts71.py --workers 2 --check` 12,829 case・15,339 configuration、lane A 13,577 full・
  text/category/location/mismatch 0、ratchet 0 regressions（変更なし）。`--checkers 4` + `conformance_ts71_compare.py`：15,335 一致・4 差（既知の直列/並列差
  `incorrectRecursiveMappedTypeConstraint`・`mutuallyRecursiveInference`・`typeParameterWithInvalidConstraintType`・`recursiveMappedTypes`、#832 と同じ 4 件）。
  `suites_ts71.py --update` → `--check` exit 0、5,315 baseline・0 regressions・ratchet 変更なし（既知の 3 mismatch：lsp `TestCompletionForStringLiteralNonrelativeImport2` =
  tsgo が run ごとに変わる、`tsc/generateTrace` ×2）。focused case 114（`handover/slice3-focused.sh` + slice 2 の list）全部 full。全 crate の test 合格（types 49、syntax 251、
  binder 78、program 684、checker 1,823、emitter 352、ls 248、lsp 89、api 332、project 235、compiler 241、conformance 49、lsproto 31）。fmt clean。clippy は hosted gates。
  log：`target/claude-tools/logs/slice3-final`・`slice3c`・`slice3d`。
- **道具**：repro `lsp-compare/repro/{nested-cast,w2-arrow,w2-sighelp,w2-infer,w2-import,w2-import2}`、`handover/slice3-focused.sh`（`@template` を含む
  61 case）、`handover/slice3-sessions.sh`、`handover/jsdoc-sessions.sh <label>`（preact/kit/eslint/prettier）、`handover/jsdoc-install*.sh`。

### 修正 8：JSDoc project の差分、その 4（PR #834、2026-10-18、Fable 5.1）

- **merge**：#834 → `98cb2fc67`（hosted run 39059282150）。commit：f3edd1166（binder・ls test）、da4a1a3de（checker）。
- **比べたもの**：#833 の後の session を slice 4 の binary で記録し直した。webpack e12e1224 の `lib/**/*.js`（790 file、134,174 request）：92 → 62 件
  （tsc-rs だけ記録し直して `out-webpack` の tsgo の記録と比較）。prettier 344d6dc（session 用 tsconfig、541 file、79,819 request）：5 → 5 件。
  ESLint 8ffeb8c（`lib/**/*.js`、389 file、68,905 request）：12 key → 5 key。
- **種類と直し方**（どれも tsgo の経路に合わせて既存の code を変えた。test は変更なしでは落ちる。3 つは TypeScript の source でも見える差）：
  1. **`@typedef` の型の中の参照は deferred な型参照（checker `annotate.rs is_resolved_by_type_alias`）**：tsgo の reparser は typedef の型を
     `JSTypeAliasDeclaration` の直下に clone する（reparser.go `KindJSDocTypedefTag`）ので、その中の参照に `isResolvedByTypeAlias` が成り立ち、既定値のある
     型引数と合わせて `isDeferredTypeReferenceNode` になる。conditional type の instantiate ごとに新しい参照ができ、webpack の `RecordFactoryFromTuple<…>` は
     `TypedHookMap.for<K>` の `M[K]` に同じ表示の `SyncBailHook<…>` 17 member を残す。こちらは `JSDocTypeExpression → JSDocTypedefTag` で止まり 12 に併合、
     `NormalModuleFactory.js` の推論も `[EmptyParserOptions]`（tsgo は `ParserOptions | [EmptyParserOptions]`）になっていた（hover 16・signatureHelp 14・
     TS2322 文言）。TypeScript の alias では両者とも同じだった。test `a_reference_nested_in_a_typedef_s_type_is_a_deferred_type_reference`。
  2. **型の無い `@property`・`@param options.x` は member の implicit any（checker `check.rs check_jsdoc_property_like_tag`・`widen.rs report_implicit_any`）**：
     tsgo の reparser は tag を property signature にし（`reparseJSDocTypeLiteral`）、`checkPropertySignature → checkVariableLikeDeclaration` が型を解決するので、
     noImplicitAny なら TS7008「Member 'logger' implicitly has an 'any' type.」、そうでなければ TS7045 の suggestion を、その property を誰も使わなくても報告する。
     こちらは「Variable」（TS7005/7043）で、使われた時だけだった（prettier の `src/cli/context.js` の `@property logger` 等 6 つ。6.0.3 は tsc-rs と同じ挙動、memo 15）。
     test `untyped_property_tags_of_a_type_literal_are_implicit_any_members`（両 category）。
  3. **enclosing declaration がある時の symbol face は accessible な chain を辿る（checker `check.rs symbol_to_type_face`。`with_reused_node_scope` は tsgo の
     `enterNewScope` と同じく scope がある時だけ押す）**：tsgo の `lookupSymbolChainWorker` は enclosing declaration があれば UseFullyQualifiedType でなくても
     chain を作り、relation error は object literal の型を literal を enclosing にして印字する（`getTypeNamesForErrorDisplay`）ので、property の namespace member は
     accessible な container か alias 経由で `{ b: Rule.CodePathSegment; }`・`T.Linter.Config<number>`・`Local.Seg` と出る（TypeScript でも同じ）。こちらは
     fully qualified の retry でしか辿らず `CodePathSegment` だった（ESLint の診断文言 6 file：`Linter.Config<RulesConfig>`・`ESLint.Options`・`Scope.Reference`・
     `Rule.CodePathSegment`・`Rule.NodeParentExtension`・`AST.SourceLocation`）。test `an_object_literal_s_error_display_qualifies_a_namespace_member_through_its_accessible_container`。
  4. **TypeScript file の `@overload` tag は何も宣言しない（binder `hosted.rs reparsed_overload_host`）**：tsgo の reparser は JavaScript だけで走る
     （parser/jsdoc.go `if p.isJavaScript() { p.reparseTags(…) }`）ので、TypeScript の関数の value declaration は最初に書かれた宣言のままで、tag だけの JSDoc が
     alias の hover の doc になる（`*@overload* — Reads ignore files and returns objects with the ignore patterns.`、ESLint `lib/config-api.js` の
     `includeIgnoreFile` = `@eslint/config-helpers` の .d.cts）。こちらの helper は TypeScript にも答え、hover の value declaration が tag の signature（doc 無し）に
     逸れていた（宣言が 1 つなら一致していた）。test `an_alias_to_two_overload_declarations_shows_the_first_one_s_overload_tag`。
  5. **union の同名 member の再印字は module root を付けない（checker `check.rs map_to_type_string_nodes`・`symbol_to_type_face`）**：tsgo の `mapToTypeNodes` は
     同じ名前になった識別子参照の member を、context の flag に UseFullyQualifiedType を足して印字し直す。typeToString の flag は
     UseAliasDefinedOutsideCurrentScope を保つので chain は module symbol を出さず、`SourceLocation | AST.SourceLocation | null`・`Scope.Reference | Reference` に
     なる。こちらの再印字は `getTypeNameForErrorDisplay`（flag は UseFullyQualifiedType だけ）の流儀で `import("…").SourceLocation | import("…").AST.SourceLocation`
     と出していた（ESLint の `complexity.js`・`no-console.js`。TypeScript でも同じ）。test `a_union_s_same_name_retry_qualifies_without_module_roots`。
- **残り**：webpack 62 件（全部 class A）：hover の `Array<T>.length`/`Set<T>.size`/`Map<K, V>.size` の qualifier 43（session の順序依存、1 file なら一致）、signatureHelp の label 6
  （`Serializer.js` の候補選択 3、`create(Dep, …)` の alias `ContextDependencyHelpers.PartialContextDependencyOptions` 2、`parseObjectExpression` の `values` 1）、
  切り詰め 2（`htmlMinify.js`）、JS generic の `@returns {ParserByType[T]}` が呼び出しの hover で instantiate されない 2、`NormalModuleFactory.js` の TS2322 文言 1
  （member が別物になった今は純粋な推論の差：17 member の contextual union の下で `new SyncBailHook([…])` の T を tsgo は `ParserOptions | [EmptyParserOptions]`、
  tsc-rs は `[EmptyParserOptions]` と推論する。縮小再現が要る）、bounded の 8（`@import` の default 名 5 = memo 13・利用者の決定、`@this` の 0:0 definition 3 = memo 3）。
  prettier 5 件：object literal を自分の中で印字する時に tsgo が `...` にする TS2339/TS2551 の文言 2（`ExtendNode<_Node>` まわりの型の同一性の差）、session 依存の 2
  （`ArrayLike<T>.length` の hover、auto-import `literallineWithoutBreakParent`：1 file の session では両者一致）、`@this` の definition 0:0 = memo 3。ESLint 5 key：長い
  object literal の文言の切り詰め位置の差 4 file（`source-code.js`・`no-restricted-globals/imports/modules.js`）、tsgo だけの TS2323 ×3 = memo 14（bounded 提案）。
  SvelteKit 1（union の property 順の hover、session 順序）、preact 3（`@this` 0:0）。
- **検査**：da4a1a3de で（release build、nice 20、`cargo -j 2`、worker 2）：`conformance_ts71.py --workers 2 --check` 12,829 case・15,339 configuration、lane A 13,577 full・
  text/category/location/mismatch 0、ratchet 0 regressions（変更なし）。`--checkers 4` + `conformance_ts71_compare.py`：15,335 一致・4 差（既知の直列/並列差 4 件、#833 と同じ）。
  `suites_ts71.py --update` → `--check` exit 0、5,315 baseline・0 regressions・ratchet 変更なし（既知の 3 mismatch）。focused case 116（slice 3 の list + `jsdocInTypeScript`・
  `jsdocTypedef_propertyWithNoType`・`typeTagNoErasure`）全部 full（各修正の後に実行）。全 crate の test 合格（types 49、syntax 251、binder 78、program 684、checker 1,827、
  emitter 352、ls 249、lsp 89、api 332、project 235、compiler 241、conformance 49、lsproto 31）。fmt clean、clippy（binder・checker・ls・lsp、all targets、`-D warnings`）clean。
  log：`target/claude-tools/logs/slice4-final`・`slice4`。
- **道具**：`handover/slice4-plan.md`、`handover/apply_{deferred_typedef,property_implicit_any,enclosing_chain,alias_overload_hover,overload_host_js_only,collision_retry}.py`、
  再現 `handover/repro/{tdeferred,tprop,tns5,tns6,tovl2,tinter3,tinter3-v12}`、`handover/slice4-chain{1,2}.sh`、`handover/jsdoc-sessions-p35.sh`、
  `handover/jsdoc-corpus-suite-design.md`（次の slice）。

### 修正 9：JSDoc corpus（tsgo の答えを持つ小さな project の suite）と、それが見つけた 2 件（PR #835、2026-10-18、Fable 5.1）

- **merge**：#835 → `7f05e1b30`（hosted run 39064206564）。commit：14709b568（ls）、546c2c3a7（checker）、204b05f30（corpus・script・test・docs）。
- **何を入れたか**：tsgo の test suite は実 project の JSDoc の形をほとんど含まない（conformance 12,829 case 中：JSDoc cast 31、入れ子 cast 6、const arrow の
  `@template` 2、`@import` 27、context-sensitive な callback を渡す generic JS arrow 0）ので、#829〜#834 の差分はすべて LSP session から出た。それらの
  再現用に縮めた project を、tsgo 自身の答えと一緒に suite として残す（利用者の決定 2026-10-17「そうしてください」）。
  - `tests/jsdoc-corpus/`（README あり）：60 case（source 210 file）。各 case に `requests.jsonl`（合計 12,919 request：file ごとに diagnostics・
    documentSymbol・semanticTokens、識別子の spread に hover・definition・typeDefinition・documentHighlight、その一部に references、`(` の後に
    signatureHelp）、`expected/cli.txt`（tsgo の `tsc -p . --pretty false` の出力と exit code）、`expected/lsp.jsonl`（request ごとの tsgo の答え。
    差分記録器と同じ正規化：`{"result": …}`、tsgo が順序付けない配列は sort、case の絶対 path は `${ROOT}`、canonical JSON）、`known.txt`（差があってよい
    id と理由 = ratchet）。
  - `crates/cli/tests/jsdoc_corpus.rs`：全 case に `tsc-rs -p .` と `tsc-rs --lsp --stdio`（case ごとに 1 session、file は 1 度だけ open、共通の
    `initialize.json`、UTF-16）を流して `expected/` と比べる。`known.txt` に無い差は fail、載っているのに一致した id も fail（ratchet は前にしか動かない）。
    `JSDOC_CORPUS_CASE=<name>` で 1 case、`JSDOC_CORPUS_DUMP=<dir>` で tsc-rs の答えを保存。debug build で 60 case 約 11 秒。
  - `scripts/jsdoc_corpus.py`：`init <case>`（request の sampling）、`update [<case>…]`（vendored tsgo で `expected/` を作り直す。tsgo が要るのはここだけ、
    local で実行して commit）、`check`（同じ比較を Python から、`--server`・`--dump`）。
  - `docs/setup.md` に説明。local の `final.sh` は crate test の後にこの test を走らせる（`tests.sh` は cli crate を回さない）。
- **known.txt（5 答え）**：`@this` 関数の `this` の definition を tsgo が空の範囲で返す 4（memo 3、利用者の決定で有効な範囲：`alias-empty`・
  `ls-tests` ×2・`svelte-ls`）、JS generic method の `@returns {ByType[T]}` が呼び出しの hover で instantiate されない 1（`w2-generic`、webpack
  `implicitTypeLoaderFallback.js`、直す）。
- **corpus が見つけた 2 件（直した。test は変更なしでは落ちる）**：
  1. **入れ子 `@param options.x` の type literal は documentSymbol を持たない（ls `symbols.rs`）**：tsgo の reparser はその literal を reparsed 印の
     property signature で組み（`reparseJSDocTypeLiteral`）、navigation bar は reparsed node に symbol を足さず property signature の中にも入らないので、
     そこに書いた inline の `{ kind: string }` は何も列挙しない（parameter の型として直接書いた `{ a: string }` の `a` は列挙する）。こちらは関数の下に
     `kind` を出していた（`w2-ls/nested.js`）。test `a_nested_param_tag_s_type_literal_has_no_document_symbols`。
  2. **signature 文字列は `noErrorTruncation` でも `... N more ...` の member を保つ（checker `check.rs`）**：tsgo の `signatureToStringEx` は option から
     NoTruncation を立てないので、TS6387 の deprecation 文言は既定の予算で長い parameter 型を切り詰め、最後の member の前に marker を出す。こちらは同じく
     切り詰めながら option を見て marker を落とし `c: …; h: Map<...>` と出していた（`ls-tests/dep3.ts`、TypeScript file）。union の marker と elision
     placeholder も同じ context flag に揃えた。test `a_signature_string_under_no_error_truncation_keeps_its_more_members_marker`。
- **検査**：最終バイトで（release build、nice 20、`cargo -j 2`、worker 2）：`conformance_ts71.py --workers 2 --check` 12,829 case・15,339 configuration、lane A 13,577 full・
  mismatch 0、ratchet 0 regressions（変更なし）。`--checkers 4` + compare：15,335 一致・4 差（既知の 4 件、#832〜#834 と同じ）。`suites_ts71.py --update` → `--check`
  exit 0、5,315 baseline・0 regressions・ratchet 変更なし（既知の 3 mismatch）。全 crate の test 合格（types 49、syntax 251、binder 78、program 684、checker 1,828、
  emitter 352、ls 250、lsp 89、api 332、project 235、compiler 241、conformance 49、lsproto 31）。JSDoc corpus（`cargo test -p tsc-rs-cli --test jsdoc_corpus`）：
  60 case・12,919 答え、連続 2 回合格（各 11 秒。hermetic 化の前は project 外の file が inferred project になり ATA で 1 答えが機械依存だった：全 case `types: []`、
  全 source file を project に入れ、`update` が `--listFiles` で検査する）。fmt clean、clippy（checker・ls・cli、all targets、`-D warnings`）clean。
  `scripts/jsdoc_corpus.py check --server target/release/tsc-rs`：60/60 ok。log：`target/claude-tools/logs/slice5-final`・`slice5`。
- **道具**：`handover/jsdoc-corpus-suite-design.md`、`handover/slice5-chain1.sh`、`handover/slice5-pr-body.md`。

### 修正 10：JS method の `@returns {X[T]}` が呼び出しの hover で instantiate されない（PR #836、2026-10-18、Fable 5.1）

- **merge**：#836 → `83e8a676a`（hosted run 39074664075）。
- **現象**：`@template {string} T`・`@param {T} type`・`@returns {Mapped[T]}` の JS class method を `f.mapped("a")` と呼ぶと、hover が
  `(method) F.mapped<"a">(type: "a"): Mapped[T]` だった（tsgo は `Mapped["a"]`）。corpus `w2-generic` の known 1 件、webpack
  `implicitTypeLoaderFallback.js` ×2。
- **原因**：`isTypeParameterPossiblyReferenced` の `containsReference` の MethodDeclaration 分岐（本体があり return 型が無ければ true）。tsgo の
  checker は reparse 済みの node を見る —— JS method の `@returns` の型は `Type`、`@template` は `TypeParameters`、`@this` は先頭の parameter —— ので、
  `@returns` のある JS method は class の `this` 型を「参照しうる」とされず、その型は `this` 引数で instantiate されず（`resolveTypeReferenceMembers` は
  `[thisType]` を class 型で埋め、`Mapped[T]` では `isThisless` が偽）、型パラメータも clone されない。こちらは構文上の `.type`（JS には無い）を見て
  `this` mapper で method の型を instantiate し、`T` を clone していた。呼び出しの mapper は clone が key、書かれた `@returns {Mapped[T]}` の `T` は
  宣言の方に解決されるので、node が `T` のまま再利用された。TS の twin も同じ機構：素の class は `Indexed["a"]`、namespace と merge した class
  （宣言 2 つ = possibly referenced）は両者とも `Indexed[T]`。
- **修正**（checker `instantiate.rs`・`jsdoc.rs`）：method 分岐は effective な return 型・type parameter 宣言・`@this` の型を見る。`children_of`
  （walker の forEachChild adapter。`containsSameNamedThisProperty` も使う）は JS node の hosted reparse（type parameter、`@this` の型、
  `@type`/`@param`/`@returns` の型、関数全体の `@type`、cast の型）を足す —— tsgo の生成 `ForEachChild` が訪れる node。helper
  `reparsed_this_type_node` を追加。
- **test**：ls hover `a_javascript_returns_index_access_is_instantiated_in_a_call_s_hover`（`Mapped["a"] | null`・`Indexed["a"]`・`Mapped["a"]`。
  修正なしでは `Mapped[T] | null` で落ちる）。corpus `w2-generic/known.txt` を削除（一致した id は ratchet が fail させる）。
- **session**：webpack 62 → 60（implicitTypeLoaderFallback ×2 が消えた。残り：qualifier 43、signatureHelp 6、truncation 2、`@import`
  references 5、`@this` 0:0 3、NMF の推論 1）。prettier 5 → 6（新規 1 件は `indent.js:60:12` の auto-import `align`：1 file の probe では両者とも tsc-rs の答え＝tsgo の session 依存、`group.js:40:31` と同じ class）、ESLint 5（変化なし：truncation 4・TS2323 memo 14）、SvelteKit 1（key は変わった：`query.js:84:10` の auto-import `read` を tsgo の session だけが足す。1 file の probe は両者同一＝tsc-rs の答え、class B。前回の `routing.spec.js:365` は再現せず）、preact 3（変化なし：`component.js` の `this` definition 3 件、tsgo の 0:0 = memo 3）。
- **検査**：最終バイトで（release build、nice 20、`cargo -j 2`、worker 2）：`conformance_ts71.py --workers 2 --check` 12,829 case・15,339 configuration、lane A 13,577 full・
  mismatch 0、ratchet 0 regressions（変更なし）。`--checkers 4` + compare：15,335 一致・4 差（既知の 4 件、#832〜#835 と同じ）。`suites_ts71.py --update` → `--check`
  exit 0、5,315 baseline・0 regressions・ratchet 変更なし（既知の 3 mismatch）。全 crate の test 合格（types 49、syntax 251、binder 78、program 684、checker 1,828、
  emitter 352、ls 251、lsp 89、api 332、project 235、compiler 241、conformance 49、lsproto 31）。JSDoc corpus 60 case 合格（`w2-generic` の hover を ratchet が強制）。
  fmt clean、clippy（checker・ls・cli、all targets、`-D warnings`）clean。focused：#834 の JSDoc 関連 116 case すべて full、`jsdoc_corpus.py check` 60/60。
  hover test は修正なしでは `Mapped[T] | null` で落ちる。log：`target/claude-tools/logs/slice6`・`logs/slice6/final`。
- **道具**：`handover/slice6-chain0.sh`（修正なしで test が落ちる確認 → chain 1）、`handover/slice6-chain1.sh`、`handover/slice6-pr-body.md`、
  `handover/jsdoc-sessions-notes.md`（2026-10-18 06:05）。

### 修正 11：JS の `@template {X} C` の制約は括弧の内側の型（PR #837、2026-10-18、Fable 5.1）

- **merge**：#837 → `247e23f9a`（hosted run 39080260366）。
- **現象**：webpack `lib/html/htmlMinify.js` の `util` の hover（予算 500）で、tsgo は `readonly registerNotSerializable: (Constructor: Constructor) => void;`
  と出すのにこちらは `readonly registerNotSerializable: ...;`（session の「truncation budget」hover 2 件）。さらに JS の `/** @template {T} T */` の TS2313 を
  tsgo は括弧内の `T`（`a.js(2,15)`）に出し、こちらは `{`（`a.js(2,14)`）に出していた。
- **原因**：tsgo の reparser は `@template {X} C` の型パラメータの constraint に括弧内の型を clone する（`gatherTypeParameters` の `constraint.Type()`）が、
  `get_constraint_declaration` は `JSDocTypeExpression`（`{X}`）全体を返していた。node builder は書かれた constraint を再利用し、再利用した node の
  span を予算に加算する（tsgo `nodecopy.go`）ので、JS の constraint 1 つにつき 2 文字多く加算し、webpack では `register*` getter 4 つの後で
  `registerNotSerializable` の関数型を予算超過で訪れて `...` にしていた。両 builder に一時 trace（tsgo は scratchpad に別 build、`TSGO_TRACE_PROP`；
  tsc-rs は `TSRS_TRACE_PROP`、いずれも除去済み）を入れて member ごとの approximateLength を突き合わせて特定。修正後は一致し、webpack の hover も同一。
- **修正**（checker `constraints.rs`）：`@template` の constraint 宣言は type expression の内側の型。
- **test**（修正なしでは落ちる）：ls hover `a_javascript_template_constraint_charges_the_type_inside_its_braces`（制約つき getter 16 個で最後の member の型が
  ちょうど予算 500 に乗る：tsgo は `readonly registerLast: (s: string) => void;`、こちらは `...` だった）、checker
  `a_circular_template_constraint_is_reported_on_the_type_inside_its_braces`。
- **検査**：最終バイトで（release build、nice 20、`cargo -j 2`、worker 2）：`conformance_ts71.py --workers 2 --check` 12,829 case・15,339 configuration、lane A 13,577 full・
  mismatch 0、ratchet 0 regressions（変更なし）。`--checkers 4` + compare：15,335 一致・4 差（既知の 4 件）。`suites_ts71.py --update` → `--check` exit 0、
  5,315 baseline・0 regressions・ratchet 変更なし（既知の 3 mismatch）。全 crate の test 合格（types 49、syntax 251、binder 78、program 684、checker 1,829、
  emitter 352、ls 252、lsp 89、api 332、project 235、compiler 241、conformance 49、lsproto 31）、JSDoc corpus 60 case 合格。fmt・clippy clean。
  log：`target/claude-tools/logs/slice7`・`logs/slice7/final`。
- **道具**：`handover/slice7-plan.md`（残りは diagnostic 側 = string path の pseudo 経路の移植）、`scratchpad/ttp2`〜`ttp6`（trace 用 repro）、
  `scratchpad/build-tsgo-trace.sh`（計測用 tsgo の build と vendored checkout の復元）。

### 修正 12：diagnostic の object literal の member は宣言の pseudo 型で印字する（PR #838、2026-10-18、Fable 5.1）

- **merge**：#838 → `d9ddaa621`（hosted run 39085195418）。
- **現象**：ESLint `source-code.js` の TS2322 が `... 27 more ...; valueOf: boolean; }`（tsgo は 16 member を出して 320 文字で切る）、
  `no-restricted-globals/imports/modules.js` の TS2322 が `message: ...; }; required: ...` を早く省略（tsgo は残して後で `... 4 more ...`）。
  session の「truncation budget」diagnostic 4 件。
- **原因**：tsgo の `typeToString` は enclosing declaration があると（relation error の object literal は `getTypeNamesForErrorDisplay` が
  渡す）member を `serializeTypeForDeclaration` の tryReuse で印字する：宣言の pseudo 型（pseudochecker の構文的な読み：literal の初期化子・入れ子
  literal・arrow・`as const` tuple・cast・accessor の戻り）が member の型と等価なら `pseudoTypeToNode` で組む。この経路自体は予算に加算せず、再利用
  した node の span（`nodecopy.go`：要素名・literal・型パラメータ宣言・書かれた型 node）だけを加算し、pseudochecker が諦める member
  （`Inferred`/`NoResult`：呼び出し・配列 literal・識別子）だけが checker の型に落ちて通常どおり加算する。こちらの string 表示経路（check.rs、
  diagnostic の printer）は error 型の member にしか pseudo 経路を使わず、全 member の checker 型を印字・加算していたので予算が早く尽き、
  `... N more ...`・`...`・320 文字の hard cut の位置がずれていた。
- **修正**：`node_builder/pseudo.rs`・`serialize.rs` の pseudo 等価判定（pseudoTypeEquivalentToType と object literal・parameter・predicate・
  pseudoTypeToType・getRegularTypeOfExpression）を context 非依存の core（`*_core`、`CheckResult`、報告する inference fallback は `reports` に集める）
  と node builder の薄い wrapper に分け、表示経路も同じ等価判定を読む（並行コピーなし。呼ばれなくなった wrapper は削除）。`check.rs`
  `property_signature`：enclosing があれば `pseudo_declaration_text`（serializeTypeForDeclaration の pseudo 分岐）と `pseudo_text`
  （pseudoTypeToNode の text 版：Direct は existing-node visitor で再利用、Inferred/NoResult は宣言か signature の checker 型、MaybeConstLocation の
  const context 判定、union は平坦化し `undefined` は 1 つ、keyword、literal は書かれたまま、`readonly [...]`、scope に入った `<T>(…) => R`、
  const context では readonly の object literal、method signature か const の関数型 property、accessor。再利用した名前・literal・型パラメータ宣言は
  span を加算）が annotation 再利用の block と `error_property_pseudo_type_text` を置き換える（error 型は等価判定の側で charitably 等価）。
- **test**（`crates/checker/tests/unit/check/tests.rs`。harness に lib が無いので最小 lib を同梱し、同じ lib の `noLib` で tsgo の文面を固定。3 本とも
  修正なしでは落ちる）：`an_object_literal_s_literal_members_charge_nothing_toward_truncation`（boolean 38 member → 320 文字の hard cut
  `isFinite: boole...`）、`a_nested_object_literal_prints_through_the_pseudo_path`（配列初期化子は fallback して加算：`... 4 more ...; minItems: nu...`）、
  `an_object_literal_s_accessor_method_and_literal_members_print_from_pseudo_types`（accessor の対・method・generic arrow・`as const` tuple・
  cast・literal・入れ子 literal：`twelfth: ......`）。既存の `reused_this_type_query_recovers_through_its_this_container` の `typeof this` parameter
  の case は tsgo の `(x: { f: (x: ...) => void; }) => void`（member は pseudo signature で印字され、再帰する parameter が literal に戻る）に更新
  （tsc 6.0.3 とこれまでの tsc-rs は `{ f: ...; }`）。
- **2 つ目の commit：mapped 型も diagnostic printer で `createAnonymousTypeNode` の経路を通る。** tsgo は anonymous と mapped の object 型を同じく
  `createAnonymousTypeNode` → `visitAndTransformType`（visited 判定・truncation の省略・symbol depth の上限）に通すが、こちらの string 表示は mapped 型を
  member から直接印字していたので、印字中に再び現れる再帰 mapped 型（prettier の `ExtendNode<_Node>`：`parent` の member が印字中の node 型）を予算が
  尽きるまで入れ子に印字していた（tsgo は `parent: ...`）。prettier の `handle-comments.js`・`strip-chain-element-wrappers.js` の TS2339（「`...` placeholder」の
  session 2 行）。test `a_recursive_mapped_type_s_revisit_is_the_cyclic_placeholder`（修正なしでは落ちる）。repro `scratchpad/tplace2`〜`tplace5`。
- **session**：ESLint 5 → 1（truncation の 4 file が一致。残りは `token-store/index.js` の TS2323 = memo 14）、prettier 6 → 3（2 つ目の commit で `...` placeholder の diagnostic 2 件が消えた。残り：`group.js:40:31` の auto-import（session 依存）・`template-literal.js` の ArrayLike hover（session 依存）・`@this` 0:0）、webpack 60 → 58（#837 の `htmlMinify.js` hover 2 件が消え、新規なし。残り 58 = qualifier 43・signatureHelp 6・`@import` references 5・`@this` 3・NMF 1）
- **検査**：最終バイトで（release build、nice 20、`cargo -j 2`、worker 2）：`conformance_ts71.py --workers 2 --check` 12,829 case・15,339 configuration、lane A 13,577 full・
  mismatch 0、ratchet 0 regressions（変更なし）。`--checkers 4` + compare：15,335 一致・4 差（既知の 4 件）。`suites_ts71.py --update` → `--check` exit 0、
  5,315 baseline・0 regressions・ratchet 変更なし（既知の 3 mismatch）。全 crate の test 合格（types 49、syntax 251、binder 78、program 684、checker 1,833、
  emitter 352、ls 252、lsp 89、api 332、project 235、compiler 241、conformance 49、lsproto 31）、JSDoc corpus 60 case 合格。fmt・clippy clean。
  新しい test 4 本は修正なしでは落ちる。log：`target/claude-tools/logs/slice7b`・`logs/slice7b/final`。
- **道具**：`handover/slice7-plan.md`、`handover/slice7b-tests.sh`・`slice7b-chain2.sh`・`slice7b-chain3.sh`、`scratchpad/tflat2`・`tnest2`・`tacc2`・
  `tthis1`（最小 lib の repro）、`handover/jsdoc-sessions-notes.md`（2026-10-18 10:30）。

### 修正 13：JSDoc typedef の alias — function 内では local、file の top では top-level（PR #839、2026-10-18、Fable 5.1）

- **merge**：#839 → `36cdecc7c`（hosted run 39090722612）。
- **現象**：webpack `WorkerAndWorkletPlugin.js` の signature help が `values: Values`（tsgo は `values: Record<string, …>`）。縮小 repro では function 内の
  `@typedef {Record<string, number>} LocalValues` を `@type` に使った `const values` の hover が `LocalValues`（tsgo は `Record<string, number>`）。
  top-level の typedef は両者とも名前を保つ。
- **原因**：`getTypeFromTypeAliasReference` は `isLocalTypeAlias(symbol) || !isLocalTypeAlias(aliasSymbol)` のときだけ参照側の alias を generic alias の
  instantiation に付ける（function 内の `type Values = Record<…>` は両者とも `Record<string, number>` と印字）。こちらの `is_local_type_alias` は
  TypeAliasDeclaration しか alias と見なさず、function 内の `@typedef` を local と判定しなかった。
- **修正**（checker `annotate.rs`）：reparse 済みの `@typedef`/`@callback` も alias 宣言（tsgo の `isTypeAlias` は JSTypeAliasDeclaration を含む）。tsgo の
  reparser は comment の属する文の隣に hoist するので、containing function はその文の親から探す（top-level 関数に付いた typedef は local でない）。
- **test**：ls hover `a_typedef_inside_a_function_is_a_local_type_alias`（修正なしでは `LocalValues` で落ちる）。
- **2 つ目：top-level の `@typedef` alias は non-generic top-level type。** tsgo の `isNonGenericTopLevelType` は type alias 宣言か reparse 済みの
  JSTypeAliasDeclaration を受けるが、こちらは `type` 宣言だけだったので、top-level の JS typedef が alias する型は「型変数を含みうる」と見なされ、generic な
  呼び出しの mapper で構造の同じ別 object に再 instantiate された。書かれた `@param {PartialContextDependencyOptions}` が parameter の型と一致しなくなり、
  signature help は複製の方を印字した：webpack `ContextDependencyHelpers.create` の `contextOptions: Partial<Omit<ContextDependencyOptions, "resource">>`
  （tsgo は `ContextDependencyHelpers.PartialContextDependencyOptions`；`CommonJsImportsParserPlugin.js:234`・`URLParserPlugin.js:322`）。修正後は実位置で一致
  （`instantiate.rs`）。test ls hover `a_top_level_typedef_alias_survives_a_generic_call_s_instantiation`（修正なしでは落ちる）。repro `scratchpad/talias3`。
- **corpus**：この回の repro 4 case を JSDoc corpus に追加（`alias-local`・`alias-generic`・`mapped-cycle`・`pseudo-members`、839 request、tsgo の答えと一致）。
- **webpack 再記録（この slice の binary、6402bd543）**：790 file・134,174 request で 58 → 55 件（signature help 6 → 3；残る 3 件は `Serializer.js` の union member 順、43 hover は qualifier の session 依存、diagnostic 1・references 5・definition 3 は従来どおり）。
- **session（sessions2）**：NestJS（1,000 file・144,763 request）1 件 = `Stream.Duplex` の qualifier が tsgo の session 順に依存（1 file の probe は両者 `Stream.Duplex`、class B）。RxJS（231 file・41,628 request）2 file＝長い TS2345 の elaboration 内で `<...>` の省略位置が違う（class A：instantiate 済み signature の `K extends T` の constraint node 再利用、次 slice）。Excalidraw（85,695 request）82 件＝completion 78（tsgo の session だけが package 自身の index 経由の auto-import source `..`/`@excalidraw/excalidraw` を出す、1 file probe は両者同一 → class B）、hover 3（190 member の object literal で tsgo だけ `... 22 more ...` に省略 → class A：tsgo の approximate length は Go の `len()` = UTF-8 byte、次 slice）、typeDefinition 1（`declare module "*.scss"` の `declare`：両 server とも 1 file では位置、`.scss` を import する file を touch した後は `[]`、tsgo も同じに振れる → session 内の状態順の差、bounded）。Puppeteer（puppeteer-core）18 件＝hover 16（`@example` 内の `{@link X | y}` の `|` 前の空白を tsc-rs が落とす → class A、次 slice）、diagnostic 2（`Iterable<T> | AsyncIterable<T>` への `yield*`/`for await` に tsc-rs だけ TS2504：iteration types cache の key、class A、次 slice）。
- **検査**：`cargo fmt --check`・`cargo clippy`（checker・ls）0；`conformance-ts71 --workers 2 --check`：12,829 case / 15,339 configuration、lane A 13,577 full、mismatch 0、ratchet 0 regressions（493 s）；`--checkers 4` の control：ratchet 0、`conformance_ts71_compare.py` 15,335 identical / 4 differ（既知の partition 依存 4 件：incorrectRecursiveMappedTypeConstraint・mutuallyRecursiveInference・typeParameterWithInvalidConstraintType・recursiveMappedTypes）；`suites_ts71.py --update` → `--check` 0 regressions / 0 above / 0 not listed（ratchet 変更なし）；crate test：types 49・syntax 251・binder 78・program 684・checker 1,833・emitter 352・ls 254・lsp 89・api 332・project 235・compiler 241・conformance 49・lsproto 31、失敗 0；JSDoc corpus gate 0（839 request）。source identity：branch `fix/ts71-local-typedef-alias` 6402bd543（main d0897abb9 上）、tsgo aa814927。
- **道具**：`scratchpad/talias2`・`talias5`、`handover/slice8b-tests.sh`。

### 修正 14：表示の長さ見積もりと iteration cache の 4 件 — RxJS・Excalidraw・Puppeteer の session から（PR #840、2026-10-18、Fable 5.1）

- **merge**：#840 → `bc2dc64a6`（hosted run 39093155210）。
- **(1) instantiate 済み signature の `K extends T` の constraint は宣言 node を再利用する。** 現象：RxJS `combine-latest-all.spec.ts`・`zip-all.spec.ts` の TS2345 elaboration（6 件・13 件）で `KeyedGroupObservable<...>`（tsgo は `KeyedGroupObservable<true, K>`）、1 file の probe でも再現。原因：tsgo の `typeParameterToDeclaration` は `typeToTypeNodeHelperWithPossibleReusableTypeNode` で、宣言された constraint node の型を **context mapper の下で** constraint と比べ、一致すれば `tryReuseExistingNodeHelper` だけで再利用する（canReuseTypeNode の gate なし）。existing-node visitor は mapper で remap された type parameter 参照を recovery boundary の中で型から再 serialize するので、印字は instantiation（`string[]`）だが課金は boundary が捨てて node の source span（` T` = 2）だけ。tsc-rs の string display path は未 instantiate の `T` と比べて不一致 → 構造印字で 6 を課金し、後ろの `<...>` 省略が早まった（両 printer の全課金を trace して特定）。修正（checker `check.rs` `type_parameter_to_declaration_display`）：`display_mappers.last()` で instantiate してから比べ、`reused_type_node_boundary_face` を直接呼ぶ。test：checker unit `a_reused_constraint_of_an_instantiated_signature_charges_its_source_span`（main では `Obs<...>; <K>...`、tsgo は `Obs<K>; <K>(k...`）。repro `scratchpad/tcons2`。
- **(2) approximate length は UTF-8 byte。** 現象：Excalidraw `deburr.ts` の `const deburredLetters`（190 member）の hover で tsgo だけ `... 22 more ...;` に省略（3 件）。原因：tsgo の見積もりは Go の `len()` = UTF-8 byte 数（`addPropertyToElementList` の `len(ast.SymbolName(prop)) + 1` は `À` で 3）、tsc-rs は UTF-16 unit（2）で、500 の hover budget に届かなかった。node builder の `len(...)` 課金はすべて byte。修正：node builder の `js_len`（type_nodes.rs・chains.rs）・`source_name_length`・late-bound 名の計算・index signature 名、string display path の `slice_js_length`・parameter 名を byte 長に。test：checker unit `a_non_ascii_literal_charges_its_utf8_bytes_toward_truncation`（60 文字の `"あ…"` を含む tuple が `... 5 more ...` に省略）、ls hover `a_non_ascii_property_name_charges_its_utf8_bytes_toward_the_hover_budget`（40 文字名 12 member が `... 6 more ...`）。repro `scratchpad/tutf1`・`tutf2`。
- **(3) `@example` 内の `{@link X | y}` は source text。** 現象：puppeteer-core の 16 hover で `{@link Page| pages}`（tsgo は `{@link Page | pages}`）。原因：tsgo の `scanner.GetTextOfJSDocComment` は link node を `GetTextOfNode`（source text）で書き、末尾の空白を trim する；tsc-rs の `text_of_comment` は `{@link ` + name + text + `}` を組み立てて `|` 前の空白を落としていた。修正（ls `jsdoc.rs`）：`CommentPart::Link` に node を持たせ、source text を書く。test：ls hover `an_example_tag_keeps_a_link_s_source_text`。repro `scratchpad/tlink1`。
- **(4) iteration types の cache は (type, use & CacheFlags) で key。** 現象：puppeteer-core `AriaQueryHandler.ts` 77:11（1 file で再現）と `AsyncIterableUtil.ts` 25:13（session 内）で tsc-rs だけ TS2504 `Type 'AwaitableIterable<…>' must have a '[Symbol.asyncIterator]()' method`。原因：tsgo の `getIterationTypesOfIterable` は `iterationTypesCache[{typeId, use & (AllowsSync|AllowsAsync|ForOf)}]` だけを持ち、worker（union の constituent 再帰・fast・slow）と iterator / iterator result は cache しない。async generator の **return type** 検査（use = AllowsAsync のみ）が `Iterable<T> | AsyncIterable<T>` に No を cache しても、`yield*`・`for await`（AllowsSync|AllowsAsync）は別 key で sync constituent を解決する。tsc-rs は strada の type ごとの 2 slot（iterable / asyncIterable）と worker 内の resolver 別 cache を持っていて、汚染された No を踏んで報告した。修正（checker `iterate.rs`・`links.rs`、types `IterationUse::CACHE_FLAGS`）：tsgo の cache model に置き換え（`IterationTypesKey`、worker は union → async fast/slow → sync fast/slow、slow の diagnostics は成功時に追加・失敗時に related）。test：compiler `language_service_diagnostics` `a_sync_or_async_iterable_union_is_iterated_after_its_generator_return_type_check`（main では TS2504 ×2）。repro `scratchpad/tyield1`。
- **session 再実行（slice 9 の binary）**：RxJS（231 file・41,628 request）2 → 0。Excalidraw（501 file・85,695 request）82 → 77：残りは全部 completion の auto-import source（tsgo の session だけが package 自身の index 経由の `..`/`@excalidraw/excalidraw` を出す；1 file の probe は両者同一 → class B）で、hover 3 件と typeDefinition 1 件は消えた。puppeteer-core（214 file・36,954 request）18 → 0（session 内だけだった `AsyncIterableUtil.ts` の TS2504 も消えた）。tsgo 側も毎回記録し直している。
- **検査**：`cargo fmt --all` 適用・`cargo clippy -p tsc-rs-types -p tsc-rs-checker -p tsc-rs-ls -p tsc-rs-compiler --all-targets` 0；5 つの新 test はソースを stash した状態で全部落ちる（negative run）；`conformance-ts71 --workers 2 --check`：12,829 case / 15,339 configuration、lane A 13,577 full、mismatch 0、ratchet 0 regressions（498 s）；`--checkers 4` の control：ratchet 0、`conformance_ts71_compare.py` 15,335 identical / 4 differ（既知の partition 依存 4 件）；`suites_ts71.py --update` → `--check` 0 regressions / 0 above / 0 not listed（ratchet 変更なし）；crate test：types 49・syntax 251・binder 78・program 684・checker 1,835・emitter 352・ls 254・lsp 89・api 332・project 235・compiler 242・conformance 49・lsproto 31、失敗 0；JSDoc corpus gate 0（839 request）。source identity：branch `fix/ts71-constraint-reuse-length` 63ec7a52b（main d0897abb9 上、#839 より前）、tsgo aa814927。
- **道具**：両 printer の課金 trace（tsgo は `scratchpad/tsgo-trace2`、tsc-rs は `TSRS_TRACE_TRUNC` の一時 patch）、`scratchpad/cmp-trace.py`、`probe-cmp.py`。

### 修正 15：推論の invokeOnce は (source, target, priority, contravariant, bivariant) で記憶する — webpack `NormalModuleFactory.js` の推論差（PR #841、2026-10-18、Fable 5.1）

- **merge**：#841 → `6adf32486`（hosted run 39096254127）。
- **現象**：webpack `lib/module/NormalModuleFactory.js` の TS2322 ×4（656・658・660・664 行、`createParser`/`parser`/`createGenerator`/`generator`）で、tsc-rs は `HookMap<SyncBailHook<[EmptyParserOptions], HtmlParser, …>>`、tsgo は `HookMap<SyncBailHook<ParserOptions | [EmptyParserOptions], HtmlParser, …>>`（1 file の probe でも再現、class A。webpack の session で最後まで残っていた diagnostic 1 件）。
- **原因**：両実装の推論を候補単位で trace して特定。`new HookMap(() => new SyncBailHook(["parserOptions"]))` の内側の `new SyncBailHook` は contextual type（17 個の hook の intersection）から構造的に推論され、`promise(...args: AsArray<T>)` の parameter で `[ParserOptions]` → `AsArray<T>` を反変に訪れた後、`tap(fn: (...args: AsArray<T>) => R)` の callback の parameter で同じ pair を共変に訪れる。tsgo の `invokeOnce` は visited を `InferenceKey{source, target, priority, contravariant, bivariant}` で引くので 2 回目も推論し、`[ParserOptions]`・`ParserOptions` を候補に加える（return type 優先度なので候補の union）。tsc-rs は strada の `source.id + "," + target.id` のまま 2 回目を飛ばし、`pop` 由来の `[EmptyParserOptions]` だけが残った。
- **修正**（checker `inference.rs`）：visited の key を tsgo の `InferenceKey` にした（`invoke_once`）。
- **test**：checker unit `a_pair_inferred_contravariantly_is_inferred_again_covariantly`（縮小 repro：`Shape<[A], B>` を contextual type にした `new Hook("x")` が tsgo では `Hook<A | [A] | [C], B>`、main では `Hook<[C], B>`）。repro `scratchpad/tvisit1`。
- **webpack**：NMF の 1 file probe が tsgo と一致（42 item）。webpack の記録済み差分 55 件を `lsp-compare.py recheck`（差分のあったリクエストだけを再実行；2026-10-18 追加）でこの修正の binary に対して再実行：1 file ずつサーバーを立て直す per-file mode で 40 件が tsgo の記録と一致（NMF の diagnostic 1 件＋qualifier の hover 39 件）、残り 15 件＝hover 4（1 file の tsgo は tsc-rs と同じ答え → tsgo の session 依存、class B）＋既知の bounded 11（`@this` の definition 3・`@import` の references 5・`Serializer.js` の union 順 signature help 3）。93 s。
- **検査**：`cargo fmt --all` 適用・`cargo clippy -p tsc-rs-checker --all-targets` 0；新 test は `inference.rs` を stash した状態で落ちる（negative run）；`conformance-ts71 --workers 2 --check`：12,829 case / 15,339 configuration、lane A 13,577 full、mismatch 0、ratchet 0 regressions（494 s）；`--checkers 4` の control：ratchet 0、`conformance_ts71_compare.py` 15,335 identical / 4 differ（既知の partition 依存 4 件）；`suites_ts71.py --update` → `--check` 0 regressions / 0 above / 0 not listed（ratchet 変更なし）；crate test：types 49・syntax 251・binder 78・program 684・checker 1,836・emitter 352・ls 256・lsp 89・api 332・project 235・compiler 242・conformance 49・lsproto 31、失敗 0；JSDoc corpus gate 0（839 request）。source identity：branch `fix/ts71-inference-visited-key` df4d0602a（main e980b6353 上）、tsgo aa814927。
- **道具**：両 printer の推論 trace（tsgo `scratchpad/tsgo-trace3` = getCovariantInference・候補追加・inferFromSignatures・applyToParameterTypes・inferToConditionalType の print、tsc-rs は `TSRS_TRACE_INFER` の一時 patch）、`nmf-trace.sh`・`nmf-cmp2.py`。

## 3. 性能の slice 3（local のみ、未 push）

- **worktree と branch**：`/Users/hiramatsu/dev/tsc-rs-p34` の `perf/ts71-lsp-request-threads`、HEAD は `b121051dd`
  （origin/main `d51b8a347` を merge 済み）。commit：
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

- tsgo の bug と思われる現象は [tsgo-bugs.md](tsgo-bugs.md) に積む（利用者の指示 2026-10-17。報告は利用者が行う）。

- CLAUDE.md の決まり：
  - tsgo に合わせる時は既存の経路を変える。
  - tsgo の答えで unit test を書く。
  - 最後の bytes で全体の検査をする。
  - 負荷を低く保つ。
  - exit status を本当に見る。
  - PR の流れに従い、merge は merge commit だけ。
- conformance が見られない修正は Rust の回帰 test にする（2026-10-13 の利用者の指示）。
- commit は repo の設定の identity で（`git commit`。`-c user.email=` で上書きしない。trailer なし）。merge commit も
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
