# H2.8a-A-RES-POST-T1 — 結果報告（before / after、新規対照、retire、未解決、未実行）

作成日：2026-09-17。設計は [DESIGN.md](DESIGN.md)。依頼書は [../h2-8a-post-t1-residuals-claude-handoff.md](../h2-8a-post-t1-residuals-claude-handoff.md)。
記録は [records/](records/)。全件 replay / hosted / PR / admission は統合担当（§6）。

## 1. 開始点と候補

| 項目 | 値 |
| --- | --- |
| 開始 SHA（origin/main、PR #549 / #550 を含む） | `eb6dc2c7872b18442657f8eefde5efc9e8fb4cf7`（[records/before/start-state.txt](records/before/start-state.txt)：`merge-base --is-ancestor eb6dc2c78` を確認） |
| worktree / branch | `~/dev/tsc-rs-post-t1-residuals` / `draft/h2-8a-post-t1-residuals` |
| toolchain | rustc 1.93.0、cargo 1.93.0、Node v25.2.1（`.node-version` と一致） |
| vendor | `_tsc.js` sha256 `1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3`、`typescript.js` `569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39` |
| 候補 | branch の原因別 commit 列（§5 の表；final head と patch sha256 は §5） |
| 上流 span | [records/upstream-spans.tsv](records/upstream-spans.tsv)（46 span、`records/span-hash.py`） |

依頼書 §2 の指示どおり、古い checkout（`~/dev/tsc-rs-emitter-residual-batch-handoff`）は実装ベースにせず、依頼書だけを新 worktree に複製した。

## 2. 対象 7 complete commands と 3 packet probes（before → after）

runner：依頼書 §5 の `post_t1_pipeline`（pipeline 4 件）と `witness.py bundle-metadata-t1 --all`（T1 18 件 / packet 15 件）。各対象を 2 回比較する既存 harness。
比較面は既存 complete-command 契約（JS / declaration / map の bytes・path・書込順序、診断、emit result、status、exit、callback metadata、partial writes）。

| 集合 | before（開始 SHA、正規 runner） | after（最終候補、正規 runner） | 記録 |
| --- | --- | --- | --- |
| pipeline focused 4（R9 ×2、R12 ×2） | **0 exact / 4 known / 0 failed**（observer `--check` 一致、592 s） | **4 exact / 0 known / 0 failed**（§4 final） | [records/before/pipeline-witness.log.gz](records/before/pipeline-witness.log.gz)、[records/final/](records/final/) |
| T1 complete 18（R12 ×2、RECEIVER-MAP ×1 を含む） | **15 exact / 3 known / 0 failed** | **18 exact / 0 known / 0 failed** | [records/before/t1-witness.log.gz](records/before/t1-witness.log.gz)、[records/final/](records/final/) |
| T1 packet 15（PRIVATE-SET ×1、DECORATOR ×2 を含む） | **12 exact / 3 known / 0 failed** | **15 exact / 0 known / 0 failed**（内部観測、互換成功に加算しない） | 同上 |

子別（編集ループは oracle 無しの `cargo test` 選択、[records/children/README.md](records/children/README.md)）：

| 子 | 対象 | 原因（DESIGN の節） | 修復 commit | 子の focused 結果 |
| --- | --- | --- | --- | --- |
| R9 | pipeline 2 件 | transformTypeScript の生成名 `default_N` が plain Identifier で、System / CommonJS が text を複製（§4） | `5f455be6c` | 新対照 `/r9/` 21：20 exact（残 1 は R12 の map 差）→ R12 後 21 exact；pipeline 2 件 exact ×2 |
| R12 | pipeline 2 件 + T1 2 件 | System の hoisted class expression / statement が `set_original_node` で宣言の `NO_TRAILING_SOURCE_MAP` を継承（§5） | `e3a3e609c` | 新対照 `/r12/` 18 exact；pipeline 2 件 + T1 2 件 exact ×2 |
| RECEIVER-MAP | T1 1 件 | compound / update の temp 初期化右辺が visited parsed receiver（上流は synthesized clone）；update の comma 列括弧の range（§6） | `9f203cf7a` | 新対照 `/receiver-map/` 18 exact（JS bytes 不変、map のみ）；T1 1 件 exact ×2 |
| PRIVATE-SET-COMMENTS | T1 1 probe | `create_private_set` の Rust 固有 `NO_TRAILING_COMMENTS`（H2.5g 由来）。現 printer の container 規則が所有者（§7） | `03097fc02` | flag 除去後も comment 17 + receiver 18 = 35 exact ×2、packet 33 / 33；T1 packet 行 exact |
| DECORATOR-COMMENTS | T1 2 probes | visited decorator 式に `NO_COMMENTS` 無し；cached receiver の bound target が original 付き；printer の native decorator に comments phase 無し；member 名の `NoLeadingComments` 無し（§8） | `150a91255` | 新対照 `/decorator-comments/` 27：22 exact + 5 known（printer 所有、§3.5）、packet 26 / 26；T1 2 probes exact |

retire した既存 ID（修復で exact になった行のみ、guard は無効化しない）：

| fixture | retire した行 | commit |
| --- | --- | --- |
| `decorator-binding-known-native.json` | R9 ×2、R12 ×2（4 行 → 0 行） | `13da7f33f`（"retire the R9 / R12 known-native rows"） |
| `bundle-metadata-t1-known-native.json` | `residual/*` 2 行、`receiver/static-compound` 1 行（3 → 0） | 同上、`af5c427c0` |
| `bundle-metadata-t1-known-packet.json` | `receiver/static-set` 1 行、`decorated/*` 2 行（3 → 0） | `03097fc02`、`150a91255` |

`.github/ci/test_replay.py` の runner test は pipeline の known 件数 4 → 0 に合わせた（planner 63 tests OK）。

## 3. 新規対照（新 ID `post-t1-residuals/<family>/<target>/<variant>`、101 件）

入力 [`crates/compiler/tests/fixtures/post-t1-residuals-inputs.json`](../../../../../crates/compiler/tests/fixtures/post-t1-residuals-inputs.json)（`scripts/generate-post-t1-residuals-inputs.mjs`）。
上流期待値 [`post-t1-residuals.json`](../../../../../crates/compiler/tests/fixtures/post-t1-residuals.json) は `scripts/observe-post-t1-residuals.mjs --write`（各 case を同一 process で 2 回採取して一致、[records/before-controls/observe-write.log](records/before-controls/observe-write.log)、251 s）と
runner の `--check`（別 process の再採取、同じ sha256 `fa4a5b38…`）で 2 回一致。T1 と同じ observer 契約（complete command ×2 + `after` / `afterDeclarations` emitNode probe + 2 回 emit）。
Rust 側 [`crates/compiler/tests/post_t1_residuals_contract.rs`](../../../../../crates/compiler/tests/post_t1_residuals_contract.rs)（T1 契約の複製、2 tests：complete ×2、bundle 行の packet ↔ probe）。runner：`python3 scripts/witness.py post-t1-residuals --all`（2 tests / 8 filtered）。

before（開始 SHA、正規 runner、[records/before-controls/](records/before-controls/)）：**40 exact / 0 known / 61 failed**、packet 40 exact / 39 failed（probed 79）。
after（最終候補、正規 runner、§4）：**96 exact / 5 known / 0 failed**、packet 79 / 79 exact。

| family | 件数 | before | after | 内容 |
| --- | ---: | --- | --- | --- |
| r9 | 21 | 11 exact / 10 failed（`default_1` vs `default_2` / `_3`：ESNext define の module / System / CommonJS / AMD / UMD、static initializer のみの class、bundle の順序反転、global script 同梱） | 21 exact | 衝突あり / なし、同一 file census、名前付き default、名前不要 class、function default（module info 経路、before から exact）、lowered set 経路（before から exact） |
| r12 | 18 | 5 exact / 13 failed（System の全行：main または second の static initializer class） | 18 exact | export 有無、宣言 emit 無し、末尾コメント、非 BMP、default 匿名、class expression 変数、関数内 class、両 source static private、ES2022、decorated ESNext define；AMD / UMD / CJS / ESNext module は before から exact |
| receiver-map | 18 | 5 exact / 13 failed（compound / update の全行：map のみ） | 18 exact | read / set / compound / update ×（used / discarded）、static / instance、副作用 receiver、括弧、関数内 class、outDir、ES2022 |
| private-set-comments | 17 | 16 exact / 1 failed（`compound-trailing` は receiver map 差） | 17 exact | 前置 / 後置 / 行末 / 括弧 / comma / 引数境界 / 複数行 / 行コメント / 値利用 / statement 後 / removeComments / outDir / instance / compound / ES2022 |
| decorator-comments | 27 | 3 exact / 24 failed（lowered：抑制されるべき trailing comment を印字；native：decorator の trailing comment を落とす；`set-line-between`：`obj.m` にコメント混入） | 22 exact + 5 known | class / member / property access / call 引数 / 括弧 / 複数 / computed name / 行間コメント × ES2022 / ESNext set、ES2015、removeComments、outDir、native ESNext define |

### 3.5 known-native として凍結した 5 行（printer 所有）

`post-t1-residuals-known-native.json`：`decorator-comments/{es2015,es2022,esnext}/set-property-access`、`decorator-comments/{es2022,esnext}/set-parenthesized`。
差は `[(_a = ns).dec /* c */.bind(_a)]` の `/* c */`（bound target の名前の後の同一行コメント）を Rust が出さないこと（JS text と map の該当 segment のみ；declaration 系は一致）。
owner は printer の PropertyAccess / ElementAccess の node 末尾 comment phase（DESIGN §8.4 決定 6：deferred phase の左端転送と synthesized 親の境界未 claim）。decorator transform 側は上流と同じ node 構成（fresh target、NoComments）になっている。
両 pass で native projection を assert し、exact になれば retire を要求して fail する（known-native 規約）。互換成功に数えない。

## 4. 最終候補の検証（final head、正規 runner、[records/final/](records/final/)）

（§4 の表は final chain の結果で確定する：`records/final/*.meta` の argv / env / exit / seconds と `*.log.gz`。）

| 集合 | 結果 |
| --- | --- |
| `fmt --check`、planner 63 tests、`git diff --check` | fmt exit 0（11 s）、planner OK 63 tests（5 s）、diff-check exit 0 |
| `witness.py post-t1-residuals --all`（observer `--check` + 2 tests） | **96 exact / 5 known / 0 failed / 101**（`SUMMARY exact=96 known=5 failed=0 selected=101`）、**packet 79 exact / 0 known / 0 failed**、`test result: ok. 2 passed … 8 filtered out`、observer 229.2 s、Cargo build + replay 182.8 s、runner exit 0（412 s） |
| `witness.py bundle-metadata-t1 --all` | **18 exact / 0 known / 0 failed / 18**、**packet 15 exact / 0 known / 0 failed**、`2 passed … 8 filtered out`、observer 31.1 s、Cargo 32.8 s、exit 0（64 s） |
| `post_t1_pipeline`（4 件、observer `--check` 767 件） | **4 exact / 0 known / 0 failed / 4**（`EXACT x2` ×4、`known_frozen: 0`）、`1 passed … 8 filtered out`、observer 360.6 s、Cargo 7.2 s、exit 0（368 s） |
| emitter `--lib` unit tests | 508 passed / 0 failed（108 s、fresh build 込み） |
| 隣接：`decorator-binding --all`（direct 156 + carry 10）、`printer --all`（142）、`compact-body-comments --all`（240）、`prologue-comments --all`（8）、`declaration-comments --all`（41）、`bundle-declarations --all`、`bundle-program --all`、`module-identities --all`、`bundle-original-javascript --all` | すべて exit 0：direct `exact=202 compared=204 mismatching=2`（既存の typed dispose KNOWN 2、1 passed）、printer 10 passed、compact-body 1 passed、prologue 1 passed、declaration-comments 3 passed / 12 filtered、bundle-declarations 3 passed / 1 filtered、bundle-program 4 passed、module-identities 3 passed、bundle-original-javascript 1 passed / 3 filtered（各 observer `--check` 一致） |
| `retained --case decorator-receiver-context/ --case decorator-static-accessor-handoff/ --case retained-accessor-followup/ --case retained-comma-factory/`（90 件） | `1 passed / 446 filtered`、exit 0（106 s；`selected 90 input cases`、各 2 回） |
| `decorator-binding-pipeline --case /lifecycle/ --case /global/esnext/ --case /nested/esnext/define/`（135 件） | **135 exact / 0 known / 0 failed**、observer `--check`（767 件）414.8 s、Cargo 217.9 s、exit 0（633 s） |
| `cargo clippy --manifest-path crates/emitter/Cargo.toml --all-targets -- -D warnings` | exit 101：dev-dependency の `tsc-rs-program`（`result_large_err` 等 144 件、main と同じ既存指摘）で止まる（T1 / C02 の記録と同じ）。`--lib`（`-D` 無し、`clippy-emitter-lib.log.gz`）：emitter は **16 warnings = main と同数**、位置は `execute.rs` / `declaration_map.rs` / `class_fields.rs` / `downlevel.rs:926,2779-2816,6846-6849` / `system.rs:210` / `builtins.rs:5235` で、本候補の変更 hunk（`system.rs:815-845,2160-2185`、`downlevel.rs:1560-1567,7700-7715,7760-7795`、`builtins.rs:5620-5640,9919-9995,10829-10840,11276-11292,12388-12420,13616-13640`、`standard_decorators.rs`、`printer.rs:13148-13185`）に新規指摘は無い |

## 5. 原因別 commit と合成候補

| commit | 子 | 変更 file |
| --- | --- | --- |
| `58f2dc77f` | scaffold | 新 suite（generator / observer / fixture / contract / runner 登録 / planner 件数）、before 記録、DESIGN |
| `5f455be6c` | R9 | `crates/emitter/src/builtins.rs`（TS visitor の `generated_declaration_bindings`、CJS visitor の登録 / `create_identifier`）、`builtins/system.rs`（`collect_hoisted_names`） |
| `e3a3e609c` | R12 | `builtins/system.rs`（`transform_hoisted_class`：`set_text_range` のみ） |
| `13da7f33f` | retire | pipeline 4 行、T1 residual 2 行 |
| `9f203cf7a` | RECEIVER-MAP | `builtins/class_fields/downlevel.rs`（`stabilize_inline_receiver` の clone、update 経路の括弧 range） |
| `af5c427c0` | retire | T1 receiver 1 行 |
| `03097fc02` | PRIVATE-SET-COMMENTS | `downlevel.rs`（`create_private_set` の flag 除去）、T1 known-packet 1 行 retire |
| `150a91255` | DECORATOR-COMMENTS | `builtins/standard_decorators.rs`（NoComments、fresh target、member 名 NoLeadingComments + classInfo gate）、`printer.rs`（`emit_modifiers` の Decorator comments phase）、T1 known-packet 2 行 retire、新 suite known-native 5 行、planner test |
| （docs） | REPORT / DESIGN / records | 本書、records |

合成候補 SHA と patch（`git format-patch --binary eb6dc2c78..<head>`、scratchpad へ生成）の sha256 は提出メッセージに記載。source / input / binary hash は [records/final/meta.txt](records/final/meta.txt)（final chain 開始時の head は docs commit 前の `150a91255`；docs commit は production / fixture / script に触れない）。

## 6. 未解決・未実行・統合担当への引き継ぎ

- **未解決（owner 付き）**：printer の bound decorator target の trailing comment（§3.5、5 行、owner = printer `E-COMMENT-SCOPE-H`）。修復は共有 printer の deferred comment phase（`printer.rs:7317-7455`、`DeferredExpressionSourceComments`）に及ぶため本候補では行わない。
- **未実行（新 source）**：全 767 件 `decorator-binding-pipeline --all`（目標 767 exact / 0 known、upstream exception 1 件別）、SUPER / retained 530 / acceptance chain、hosted witness / acceptance の全 job、5g qualification。
  PRIVATE-SET-COMMENTS の flag 除去は H2.5g 由来の境界を外すので、hosted の retained / 5g / acceptance を admission の条件にする（DESIGN §7.4）。
- **hosted へ渡す対象**：`scripts/witness.py` の `COMPILER_DIRECT["post-t1-residuals"]`（controls group に自動編入、pinned Node が必要；`.github/workflows/witness.yml` の pinned-Node 条件 list への追加は統合担当）。
  入口：`python3 scripts/witness.py post-t1-residuals --all`。入力 101（bundle 79 が packet probe）、2 tests / 8 filtered。ローカル実測：observer `--check` 約 250 s、Cargo build + replay 約 200 s（§4 の meta）。controls job に載せると前回 38m49s + 約 8 分：45 分の分割閾値に近づくため、独立 job または `decorator-binding-pipeline` job への同乗を検討する。
- **統合担当の持ち分**：profile / STAGE / accepted-state ratchet / 共有 CI policy の変更、PR / merge / admission、`E-NAMES-BASE`（R9）と `E-METADATA-BASE`（T1 部分）の `modified-requalify` の再 qualify（最終 validation ref）。
- runtime control：本候補で変換後の実行式が変わる修復は無い（R9 は綴りのみ、R12 / RECEIVER-MAP は map のみ、PRIVATE-SET / DECORATOR はコメントのみ）。JS bytes の一致は complete-command 契約で確認済み。
