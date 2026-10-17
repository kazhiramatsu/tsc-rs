# LSP の性能（2026-10-16〜）

利用者の指示（2026-10-16）：「主要プロジェクトでの差分を修正してください。その後、パフォーマンス改善に進んでください。LSP、
noEmit、d.ts、js emit でtsgoより速くなる、ピークメモリを少なくするのが目標です」。主要 project の LSP の差分（#822〜#824）の後、
言語 server の速さと memory を tsgo `aa8149273` に揃える。CLI の各 mode（noEmit・d.ts・JS emit）の README の計測は、
利用者がいる時に `nice` なしで行う（それまでの値は [ts71-perf-2026-10](../ts71-perf-2026-10/README.md)）。

## 計測の方法

- **session**：#822 の生成した session（`~/dev/real-projects.noindex/lsp-compare`。各 file を開き、診断・symbol・折り畳み・
  semantic token・整形、識別子 40 か所の hover・定義・型定義・highlight、その 1/10 の references、1/4 の completion と
  signature help）。tsgo と tsc-rs に同じ要求を送り、答えを突き合わせる。
- **道具**：`time-run.sh <label> <root> <glob>`（両 server を `/usr/bin/time -l` の下で、要求ごとの時間を `<name>.times` に）、
  `times.py <out>`（method ごとの合計と差の大きい要求）、`repeat.py`（一つの要求を繰り返す。`--touch` で先に他の file を
  開閉、`--crlf-to-lf`、`--log`、`--pid-file`/`--wait-file` で profiler を付ける）、`each.py`（各 file に一つの要求）。
  profile は macOS の `sample` と `target/claude-tools/sample_tree.py`（関数の下の重い枝の集計）、program の作成は
  `TSRS_PHASE_TRACE=1`。
- **負荷**：triage は `nice -n 20`（両 server とも同じ条件）。README に載せる計測は利用者と一緒に行う。
- **driver の本文**：driver は file を Python の `read_text` で読み、CRLF を LF に変えて送っていた。TypeScript-6.0 の checkout
  には CRLF の file があるので、開くたびに file が変わり、両 server が project を作り直していた。editor と同じく本文の byte
  のまま送るように直した（`read_bytes().decode("utf-8-sig")`）。それまでの比較は両 server に同じ入力を送っており有効。

## slice 1：references と program の読み込み（#825）

- **merge**：#825 → `0ea502d21`（hosted run 38881695652）。commit：7b3edf580（名前の表）、4cf22627a（projects をまたぐ検索）、
  048d99c39（program の読み込み）、24890b8b2（planner の clippy）。
- **名前の表は file ごとに一度**：tsgo は `SourceFile.GetNameTable` を file に持つ（`nameTableOnce`）。tsc-rs は大域の
  references のたびに、検索する program の全 file（library を含む）の表を所有の文字列の `BTreeMap` で作っていた。表を
  `SourceFile` の cell（`NameTableCell`）に置き、候補の位置は memmem（Go `strings.Index`）で探す。
- **projects をまたぐ検索を並列に**：tsgo `handleCrossProject` は project ごとの検索を work group に積む。queue の項目を
  波ごとに `WorkerBudget::automatic().map_ordered` で検索し、答え・元の位置・既定の定義は queue の順に取る（一つずつの
  loop と同じ答えと次の project）。
- **program の作成は parse cache から**：tsgo の compiler host は session の parse cache から file を取る。loader は parse の
  前に parse cache（`ProgramOptions` の `ParseCacheHandle`、project system の document registry。library の bundle も
  `lib_documents` に登録する）を引き、cache の木から import を計画して自分の parse を残さない（checker は前から cache の
  document を取っていた）。UTF-16 の位置は木の snapshot から（cache の木は index を作り済み）。
- **並列の読み込みと bind**：`FileSystem::permits_parallel_reads`（project system の `SourceFs` は true：読むのは snapshot の
  file、記録は集合）で VFS の compiler host が loader の先読みを許す。言語 server は自動の worker の予算で読み込み、
  live program は worker で bind する（tsgo の work group）。
- **snapshot の file system の cache**：tsgo の snapshot の file system は `cachedvfs`。各 snapshot の file system を
  `CachedFs`（metadata・有無・realpath・一覧を一度だけ問い合わせる）で包む。
- **計測**（`nice -n 20`。前は main と同じ code の build、ただし * の行は slice 1 の途中の build（名前の表と parse cache の後、
  並列化の前）。後はこの branch）：

  | 場面 | 前 | 後 | tsgo |
  |---|---:|---:|---:|
  | 大域の references の繰り返し（TypeScript compiler `tsbuild.ts:51:13`） | 636 ms | 0.9 ms | 0.9 ms |
  | 12 project にまたがる references（`declarations/diagnostics.ts:270:17`）* | 2.66 s | 1.65 s | 1.61 s |
  | 開き直しで 11 project を作り直す（CRLF→LF で強制） | 1.75 s | 0.35 s | 0.026 s |
  | hono：289 file を開いて診断 * | 17.05 s | 13.8 s | 4.31 s |

  session 全体（editor と同じ本文）：

  | project | 要求 | tsc-rs | tsgo | tsc-rs peak RSS | tsgo peak RSS | 違い |
  |---|---:|---:|---:|---:|---:|---:|
  | TypeScript compiler（`src/compiler`、12 project） | 36,739 | 44.8 s | 50.3 s | 1.69 GB | 3.28 GB | 0 |
  | hono（`src`） | 47,637 | 42.9 s | 23.4 s | 1.12 GB | 0.82 GB | 42 |

  hono の 42 件は tsgo 自身の揺れ：40 件は #822 で記録した timing middleware の import 先（tsc-rs `../..`、tsgo
  `../../middleware/timing`）、2 件は rest の型の property の順（以前の 5 回と tsgo だけの 2 回は tsc-rs と同じ順）。
- **test**：program `the_planner_reads_a_cached_parse_instead_of_parsing`、host
  `a_cached_file_system_answers_entries_as_it_first_did`。
- **検査（048d99c39）**：conformance 0 regressions、`--checkers 4` は既知の 4 件、suites 0 regressions（ratchet は変わらず、
  lsp 4,583/4,584、tsc 227/229。suites は 33 s、#824 の時は 249 s）、全 crate の test、API client sync 356/356・async
  365/365。24890b8b2 で clippy（`-D warnings`）、fmt、program の test。
- **残り**：project の作り直しはまだ tsgo の約 2 倍（hono の test の project 50〜80 ms 対 27〜55 ms、強制した 11 project の
  作り直し 0.35 s 対 0.026 s：cache の木の import の巡回、registry の本文の hash、直列の識別子と bind の予約）。

## slice 2：auto-import の completion（#826）

- **merge**：#826 → `f298be6af`（hosted run 38890844211）。commit：0489ce606（cache の計画）、0f5a3a7c8（alias resolver）、
  1b686b3ea（group の map）、8f6a08e91（clippy）。
- **alias resolver は解決した file だけを読む**：tsgo の `aliasResolver` は checker に entrypoint だけを渡し
  （`SourceFiles()`）、module を解決した時にその file を読んで bind する（`GetSourceFileForResolvedModule`）。tsc-rs は到達
  できる全 file の全要求を先に読んでいた（vitest の型から jsdom の JS が 1,054 file、vue の root の bucket を作るたびに
  494 ms。tsgo も open file の依存が変わるたびに作り直すが約 10 ms）。tsc-rs の checker は file を先に持つので、読んだ
  file で checker を作り、まだ読んでいない file に解決した要求は記録して（失敗した lookup にはしない）、その file を足した
  checker で抽出をやり直す。足りない file が無くなるまで繰り返す（`extract_from_resolved_files`）。読む file は tsgo と同じ。
- **alias resolver の parse と document**：session の parse cache から計画し（`plan_source_requests_cached`）、checker は
  session の document を共有する（`RegistryCloneHost::documents`）。前は二度 parse し、何も共有しなかった。
- **package の抽出を並列に**：tsgo は実 path ごとの package を work group で抽出し、その後 `@types` の代わりを抽出する。
  二つの波とも worker で行う（入力は `Sync`）。
- **completion の group は map で引く**：tsgo の group は map。tsc-rs は順序のため `Vec` に持ち、export ごとに線形に探して
  いた（候補の数の二乗。playwright-core の completion は毎回 172 ms、tsgo 39 ms）。鍵から位置を引く map を併せて持つ
  （順序は変わらない）。
- **編集の古い file**：`LiveProgram::reuse` は古い file の計画も parse cache から作る（前は編集のたびに parse し直していた）。
- **計測**（`nice -n 20`、editor と同じ本文）：

  | 場面 | 前（#825） | 後 | tsgo |
  |---|---:|---:|---:|
  | vue：package で最初の completion（`runtime-core/src/apiWatch.ts`） | 0.83 s | 0.16 s | 0.06 s |
  | vue：同じ completion の繰り返し（2.6 MB） | 51 ms | 22 ms | 27 ms |
  | playwright-core：completion の繰り返し（`client/frame.ts`、3.5 MB） | 172 ms | 41 ms | 39 ms |

  | project | 要求 | 前（#825） | 後 | tsgo | 違い |
  |---|---:|---:|---:|---:|---:|
  | vue（`packages/*/src`） | 41,256 | 23.3 s | 16.0 s | 12.2 s | 9 |
  | playwright-core（`src`） | 60,108 | 42.0 s | 33.8 s | 26.8 s | 50 |
  | effect（`packages/effect/src`） | 89,249 | 76.1 s | 60.1 s | 63.0 s | 69 |
  | hono（`src`） | 47,637 | 42.9 s | 40.0 s | 22.5 s | 45 |
  | zod（`packages/zod/src`） | 59,207 | 20.9 s | 20.3 s | 22.3 s | 0 |
  | TypeScript compiler（`src/compiler`） | 36,739 | 44.8 s | 43.7 s | 49.0 s | 0 |

  （vue・playwright-core・effect・zod の「前」は #825 の途中の build。）違いは #822 と #824 で記録した種類：vue の
  runtime-core の `version`/`TrackOpTypes` の import 先（ほか 4 件は tsgo だけの再実行 2 回で違う答えか tsc-rs と同じ答え）、
  playwright-core の codemirror の `lint`/`overlayMode` と `registry`、effect の `HttpApiSchema`/`Schema` の再 export、hono の
  middleware の import 先と hover。hono の tsc-rs の 45 件の答えは、#825 より前の今朝の記録と byte まで同じ（tsgo の答えは
  16 件で変わった）。比較の道具に `classify.py`（違う項目の名前でまとめる）を足した。
- **test**：lsp `auto_imports_follow_a_chain_of_re_exports`（`export *` 二段と `export { deepValue } from "dep"`：tsgo の
  候補。import だけの file は読まない）。これと #824 の `auto_imports_resolve_a_package_s_javascript_re_export` は、抽出を
  一つの checker で止めると落ちる。
- **検査（53d72e28a、origin/main 18a35e729 と merge 済み）**：conformance 0 regressions、`--checkers 4` は既知の 4 件、suites
  0 regressions（ratchet は変わらず、lsp 4,583/4,584、tsc 227/229）、全 crate の test（lsp 63）、API client sync 356/356・
  async 365/365。8f6a08e91 で clippy（`-D warnings`）、fmt、ls・lsp の test。
- **残り**：package の bucket の最初の作成はまだ tsgo の約 2.5 倍（vue の package で最初の completion 0.15 s 対 0.06 s）：
  読んだ file の要求を全部先に解決し（tsgo は checker が求めた時）、段ごとに checker を作り直す。

## slice 3：要求の thread・symbol の連鎖・build の file の lock（local のみ、未 push）

2026-10-16 の利用者の指示で差分の修正を先にすることになり、slice 3 は local の branch のまま Fable 5.1 に引き継いだ。
branch・commit・済んだ検査・残りは [ts71-real-projects](../ts71-real-projects/README.md) の「3. 性能の slice 3」。

## 起点（slice 1 の途中、名前の表と parse cache の後・並列化の前）

editor と同じ本文、`nice -n 20` の session の時間。差の大きい要求の種類が次の slice の対象。

| project | tsc-rs | tsgo | tsc-rs peak RSS | tsgo peak RSS | 主な差 |
|---|---:|---:|---:|---:|---|
| TypeScript compiler | 60.5 s | 50.3 s | 1.66 GB | 3.28 GB | references（projects をまたぐ） |
| hono | 52.6 s | 23.4 s | 1.10 GB | 0.82 GB | 診断（src と test の project の切り替えのたびの作り直しと検査） |
| zod | 20.9 s | 22.4 s | 1.45 GB | 2.70 GB | （tsc-rs が速い） |
| vue | 23.3 s | 12.3 s | 0.99 GB | 1.00 GB | auto-import の completion |
| playwright-core | 42.0 s | 27.3 s | 1.40 GB | 1.97 GB | auto-import の completion |
| effect | 76.1 s | 64.5 s | 6.20 GB | 6.62 GB | references・completion・診断 |

## 次

- **auto-import の completion**：vue では package ごとの最初の completion が tsc-rs 0.6〜0.8 s、tsgo 0.06 s。tsgo も
  open file が変わるたびに root の node_modules の bucket を作り直すが、package の抽出は実 path ごとに並列で、alias
  resolver は checker が解決した module の file だけを parse cache から読む（`SourceFiles()` は root だけ）。tsc-rs の
  alias resolver は到達できる全 file を先に読み（vitest の 20 root から jsdom の JS を含む 1,054 file、494 ms）、抽出も直列。
- program の作り直しの残り、hono の test file の検査、要求ごとの固定の費用（hover などの中央値は約 1.5 倍）。
