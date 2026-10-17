# tsgo の bug と思われる現象のメモ

利用者の指示（2026-10-17）：「tsgo のバグと思われる現象はメモしておいてくださいね」。
tsgo（TypeScript 7.1、vendored commit aa814927）と比べていて、tsgo の側が怪しいと
判断した現象をここに積む。本家（microsoft/TypeScript）への報告は利用者が行う
（AI 利用の開示が必要。本家の test 追加 PR の材料は `target/claude-tools/handover/upstream-tests-pr.md`、
`target/` 以下で未 commit）。tsc-rs 側の扱いは原則「tsgo に合わせる」。例外は
利用者の決定があるものだけ（下表の「tsc-rs の扱い」）。

書式：見つけた日 / 見つけた場所 / 現象 / 根拠（再現と tsgo の該当 code）/ tsc-rs の扱い /
本家への報告の状態。新しく見つけたら末尾に足し、報告したら状態を更新する。

## 一覧

### 1. auto-import が、export を augment するだけの file を specifier に選ぶ（run ごとに違う）

- 見つけた日・場所：2026-10-16、hono・vue・effect・playwright の LSP session（PR #827）。
- 現象：auto-import の module specifier が session ごとに変わり、ときに symbol を export して
  いない file（augment するだけの file：hono の `../..`、effect の `./http-api/HttpApiSchema.ts`、
  `@types/codemirror` の addon）を指す。その import は書いても解決しない。
- 根拠：`tsc/internal/ls/autoimport` の group は Go の map 順で処理され、specifier の cache は
  importing file ごとに export の file で引かれる。先に処理された group の答えが後の group にも
  使われるため、augment 側が先に来ると無効な specifier になる。再現
  `lsp-compare/repro/{vue-mini,hono-mini,cm-mini}`、tsgo を 3〜11 回走らせた記録
  （`out-hono-*`、`out-vue-tsgo*`）。本家に該当 issue は無い（2026-10-16 時点。#64821 は別件）。
- tsc-rs の扱い：利用者の決定（2026-10-16）で、tsgo の答えに無効な import が混じるときは宣言
  file（有効な方）を返す。tsgo が有効な答えの間で揺れるときは最頻の答え（PR #827 の
  `fill_shared_specifiers`）。
- 報告：下書き `target/claude-tools/handover/tsgo-autoimport-issue.md`、未投稿（利用者の判断待ち）。

### 2. 未 build の参照先出力に redirect される root に、file も診断も出ない

- 見つけた日・場所：2026-10-17、astro の `markdoc` の tsconfig（PR #828、`compare-all.sh`）。
- 現象：参照 project が持つ file を `include` が含むと、その root は参照先の出力 `.d.ts` に
  redirect される。出力が未 build だと tsgo は file を program に入れず、診断も出さない。
  TypeScript 6.0.3 は TS6305（「出力 '…' は source '…' から build されていない」）を報告する。
- 根拠：再現 `~/dev/real-projects.noindex/lsp-compare/repro/astro-refs`（`pkg/tsconfig.json`、`${configDir}` を
  使う `extends`、`references: [./tsconfig.test.json]`、test 側の `include` が `components/**/*` も含む）を `-p` で
  build すると、tsc 6.0.3 は exit 2 で `error TS6305: Output file '.../out/components/a.d.ts' has not been built
  from source file '.../components/a.ts'.`（`The file is in the program because: Matched by include pattern
  './components/'`）、tsgo aa814927 は exit 0・診断無し・`a.ts` は `--listFiles` に出ず、出力は `tsbuildinfo` だけ。
  astro 本体では `packages/markdoc` などがこの形。
- tsc-rs の扱い：tsgo に合わせる（`loader.rs resolve_root_sources` が redirect 先の無い root を落とす）。
- 報告：本家の test 追加 PR の材料（`upstream-tests-pr.md`）に「test で固定するより issue 向き」と
  して判断材料を置いた。未投稿。

### 3. `@this` の parameter の definition が空の範囲

- 見つけた日・場所：2026-10-17、svelte の LSP session（PR #829）。
- 現象：JavaScript の関数の JSDoc `@this {T}` から作られる hosted parameter の名前が位置 0 の
  node で、`textDocument/definition` の `targetRange`/`targetSelectionRange` が `0:0` になる。
- 根拠：tsgo の reparser が `@this` の parameter 名を Loc 0 で作る。再現は svelte の
  `packages/svelte/src` の該当 file（session の `differing.jsonl`、`out-svelte`）。
- tsc-rs の扱い：正しい範囲を返す（1. と同じく、tsgo の答えが使えないときは有効な答え）。
- 報告：未投稿。

### 4. hover の表示が checker の状態で変わる（session の前の file 次第）

- 見つけた日・場所：2026-10-16〜17、hono `Context<any, any, {}>.header`（tsgo 11 回中 6 回は
  具体化した形、5 回は宣言の型引数の形）、svelte `Value<V = unknown>.equals`（`Value<V>`）。
- 現象：同じ file の同じ位置の hover が run ごとに違う。前に開いた file の check が、対象の
  型の member を型解決前に instantiate していたかどうかで表示が変わる。
- 根拠：単独で record すると両 server とも同じ答え。`lsp-compare/replay.py` で session の
  file 列を二分探索できる。
- tsc-rs の扱い：単独 session の答え（checker の状態に依らない形）を返す。bounded（class B/C）。
- 報告：未投稿（非決定性の報告は 1. と合わせるのがよい）。

### 5. JSDoc の `@returns {x is T}` の `x` に symbol が無い（`asserts x is T` には有る）

- 見つけた日・場所：2026-10-17、svelte の documentHighlight（PR #829）。
- 現象：JavaScript の `@returns {x is T}` の `x` は highlight・references に含まれず、
  `@returns {asserts x is T}` の `x` は含まれる。
- 根拠：tsgo は JSDoc の名前を reparse した clone 経由で解決するが、clone を位置の二分探索で探す
  `ast.GetReparsedNodeForNode` は clone の根と同じ位置から始まる node（型の先頭 token）を
  取りこぼす。checker の表示（述語の型）は clone 経由で正しく解決する。
- tsc-rs の扱い：tsgo に合わせた（`location.rs`、test
  `a_plain_jsdoc_predicate_does_not_name_its_parameter`）。本家で直れば追随する。
- 報告：未投稿。

### 6. LSP test suite の 1 case が run ごとに違う

- 見つけた日・場所：2026-10-15、tsgo の language server test 4,584 件の再生（packet ts71-lsp）。
- 現象：4,584 件中 1 件は tsgo 自身の答えが run ごとに変わる（README の「4,583 of 4,584」）。
- tsc-rs の扱い：記録のみ。
- 報告：未投稿。
