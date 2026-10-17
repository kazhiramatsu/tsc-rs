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
