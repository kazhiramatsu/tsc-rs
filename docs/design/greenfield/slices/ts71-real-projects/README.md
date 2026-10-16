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

- **merge**：#827 → `ae6f234f3`（hosted run 38911087732）。commit：9f3052b43（共有 file の specifier を先に埋める）、85292efd1（package の export は entrypoint の順）。
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
- **検査**：8934a33aa（origin/main 41316289c の上）で release binary を作り直し、`nice -n 20`・worker 2：conformance 0 regressions、`--checkers 4` は既知の 4 件（15,335 一致）、suites 0 regressions（ratchet は変わらず、lsp 4,583/4,584、tsc 227/229）、全 crate の test（types 49・syntax 251・binder 78・program 683・checker 1,806・emitter 350・ls 232・lsp 67・api 332・project 235・compiler 241・conformance 49・lsproto 31）、clippy（ls・lsp、`-D warnings`）、fmt、API client sync 356/356・async 365/365。
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

- **merge**：#828 → `39d8a0121`（hosted run 38956397408）。commit：54cec86a5（emit：書き換えた specifier の引用符、line comment の
  次の export specifier）、95153f130（check：`@overload` の型引数、`this` property の文脈型、`T & object` と array の
  comparable、TS2694 の module 名）、73a8c2a73（program：未 build の参照先出力に redirect される root、package scope の順）、
  1ce172122（check：overload の template の型引数は symbol 一つのまま、comment の table に置く）。
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
- **検査**：1ce172122（origin/main cf8fb7444 の上）で release binary を作り直し、`taskpolicy -c maintenance nice -n 20`・
  worker 1（利用者の就寝中）：conformance 0 regressions（13,577 full、emit 13,569 full・mismatch 0、5,083 s）、
  `--checkers 4` は既知の 4 件（15,335 一致）、suites 0 regressions（ratchet 変わらず）、全 crate の test（types 49・syntax 251・
  binder 78・program 684・checker 1,811・emitter 352・ls 232・lsp 67・api 332・project 235・compiler 241・conformance 49・
  lsproto 31）、fmt。clippy は hosted の `rust` job。73a8c2a73 での最初の full run は overload の修正の第 1 版で 4 件
  regress（`.types`・d.ts の `T_1` 改名）、1ce172122 で解消。関連する既存の conformance case 23 件（`jsFileFunctionOverloads*`・`jsFileMethodOverloads*`・`jsDocGenericOverloads`・`overloadTag1-3`・
  `templateInsideCallback`・`jsdocVariadicInOverload`・`jsdocTemplateTag*`・`jsdocImportTypeNodeNamespace`・
  `annotatedThisPropertyInitializerDoesntNarrow`・`rewriteRelativeImportExtensions*`）は個別に full。
- **道具**：`~/dev/real-projects.noindex/emit-compare.py`（outDir を分けて両方で emit し tree を比べる。違う tree は
  `frontend/work/<repo>-<label>/` に残る）、`frontend/compare-all.sh <label> [diag|emit|both]`、`frontend/<repo>-projects.txt`、
  `lsp-compare/repro/{svelte-js,overload,overload2,overload3,overload4,derived,comparable,comparable2,astro-refs,…}`、
  `handover/upstream-tests/run-case.py`（本家形式の case を tsgo で走らせる）。
- **残り**：frontend repo の LSP session（astro・svelte・solid・remix・nuxt、`lsp-compare/run.sh`）、hono の
  `Context<any, any, {}>.header` の hover（B、tsgo 3/4）。

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
