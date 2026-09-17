# H2.8a-A-RES-BUNDLE-METADATA-T1 — 結果報告（before / after、追加対照、残差、未実行）

作成日：2026-09-17。設計は [DESIGN.md](DESIGN.md)。依頼書は [../h2-8a-bundle-metadata-t1-claude-handoff.md](../h2-8a-bundle-metadata-t1-claude-handoff.md)。
記録は [records/](records/)。全件 replay / hosted / PR は統合担当（§6）。

## 1. 開始点と候補

| 項目 | 値 |
| --- | --- |
| 開始 SHA（origin/main、PR #549 を含む） | `84da0c0278c296fd295f15e48177ada87810f841`（[records/start-state.txt](records/start-state.txt)） |
| worktree / branch | `~/dev/tsc-rs-bundle-metadata-t1` / `draft/h2-8a-bundle-metadata-t1` |
| toolchain | rustc 1.93.0、cargo 1.93.0、Node v25.2.1（`.node-version` と一致） |
| vendor | `_tsc.js` sha256 `1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3`、`typescript.js` `569177652966bd528c319171c7dd22860dbf72bde116cbc4f644f1d02bb12e39` |
| 候補 | branch `draft/h2-8a-bundle-metadata-t1` の単一 commit（base `84da0c027` の直上；hash は `git log -1` と提出メッセージ）。patch は `git format-patch --binary -1 <commit>`（提出時に scratchpad へ生成、sha256 は提出メッセージ） |

第 2 原因の確認（DESIGN §3 / §5）：`class_fields/downlevel.rs:7636-7657` `set_private_receiver_comment_range`
（上流 `setCommentRange(receiver, moveRangePos(receiver, -1))`、`_tsc.js:96407` / `96808`）が
`B.#p` の parsed Identifier `B`（`TransformSourceId(1)` = second.ts）に `comment_range: EndOnly` を書き、
`snapshot_parsed_emit_metadata` の whitelist（`parsed_metadata.rs`）がそれを `ParsedEmitMetadataNotPortable` で拒む。
before の 5 件はすべてこの typed error（§2.1）。上流は Bundle root の parse node を dispose しないため
（`disposeEmitNodes` @25302-25310 と `getParseTreeNode(bundle) === undefined`）、この emitNode は
declaration transform までそのまま残る（DESIGN §3 の pinned span）。

修復：packet の許可項目に `comment_range` を加え、range 自身の source を `SourceFileId` で記録して
restore 時に再 mount する（DESIGN §6 決定 1〜6、変更 file は DESIGN §7）。

## 2. 対象 5 件（依頼書 §3）

入力 `decorator-binding-inputs.json` sha256 `0284b24a…`、期待値 `decorator-binding.json.zst` sha256 `be52ec87…`
（不変、[records/input-hashes.txt](records/input-hashes.txt)）。runner：`scripts/witness.py decorator-binding-pipeline --case …`（依頼書 §5 の `t1_witness`）。
既存 harness は各対象を 2 回比較する。

| | source | binary sha256 | 結果 | 記録 |
| --- | --- | --- | --- | --- |
| before | 開始 SHA（clean；dirty は複製した依頼書 .md 1 件のみ） | `7b66082a…`（`records/before/pipeline-binary.sha256`） | observer `--check` 一致（542 s）、**`exact=0 known=5 failed=0 selected=5`**（5 件とも凍結どおり `ParsedEmitMetadataNotPortable`、exit 0） | [records/before/t1-witness.log.gz](records/before/t1-witness.log.gz)、[meta](records/before/meta.txt) |
| after | 候補（production は最終 bytes；known-native から T1 5 行を retire、[records/retired-known-native-ids.txt](records/retired-known-native-ids.txt)） | `95b2fff1…`（`records/after/binaries.sha256`、witness 内で再 build） | observer `--check` 一致（496 s）、**`exact=5 known=0 failed=0 selected=5`**（5 件とも `EXACT x2`；runner の `known_frozen` = 4 = R9 ×2 + R12 ×2）、exit 0 | [records/after/t1-witness.log.gz](records/after/t1-witness.log.gz)、[meta](records/after/t1-witness-meta.txt) |

比較面は既存 complete-command 契約（JS / declaration / map の bytes・path・書込順序、診断、emit result、status、exit、callback metadata）。

## 3. 追加対照（新 ID、`bundle-metadata-t1/<family>/<target>/<variant>`）

入力 [`crates/compiler/tests/fixtures/bundle-metadata-t1-inputs.json`](../../../../../crates/compiler/tests/fixtures/bundle-metadata-t1-inputs.json)
（`scripts/generate-bundle-metadata-t1-inputs.mjs` が生成、18 件：`residual` 2 行は System、他の bundle 行は AMD、`source-file-roots` は ES2015 module）。上流期待値
[`bundle-metadata-t1.json`](../../../../../crates/compiler/tests/fixtures/bundle-metadata-t1.json)
は `scripts/observe-bundle-metadata-t1.mjs --write` が各 case を 2 回採取して一致を確認し、`--check` で全件をもう一度採取して一致
（[records/upstream-observe-write.log.gz](records/upstream-observe-write.log.gz)、[records/upstream-observe-check.log.gz](records/upstream-observe-check.log.gz)；
hash は [records/input-hashes.txt](records/input-hashes.txt)）。各 case は complete command（`emitFilesAndReportErrorsAndGetExitStatus`）
に加え、別の fresh Program で `after` / `afterDeclarations` identity hook が parse node の emitNode を投影する probe と、
同一 Program で通常 emit を 2 回行う probe を持つ（18 件すべて `second_ordinary_emit.identical = true`；
`other_keys`（portable packet 外の emitNode field）は全行で空）。

Rust 側 [`crates/compiler/tests/bundle_metadata_t1_contract.rs`](../../../../../crates/compiler/tests/bundle_metadata_t1_contract.rs)（2 tests）：

1. complete command ×2（共有 comparator `h2_7b_w4a_controls::assert_completed_observation`）。
2. packet ↔ probe：JS arena（root source を mount）→ script transformers → print → `snapshot_parsed_emit_metadata` → dispose →
   declaration arena を production 順（非 JSON の bundle source → その他の host source）で mount → `restore_parsed_emit_metadata` →
   parse node ごとの `{file, kind, pos, end(UTF-16), flags, internal_flags, comment_range{pos,end}(UTF-16 / -1), type_node, constant_value}`
   を上流 `after_javascript` probe と集合比較（bundle の複数 hook call が同一投影であることも assert）。comment range の行は常に完全一致を要求。

runner：`python3 scripts/witness.py bundle-metadata-t1 --all`（observer `--check` → 2 exact test、imported comparator の 8 test は filtered）。

### 3.1 結果

| 集合 | 件数 | 結果（各 2 回） | 記録 |
| --- | ---: | --- | --- |
| complete command（`bundle_metadata_t1_controls_match_complete_typescript_observations`） | 18 | **exact 15 / known 3 / failed 0**（`SUMMARY exact=15 known=3 failed=0 selected=18`）。known = `residual` 2 行 + `receiver/static-compound`（§3.2、いずれも JS map のみの差で JS / d.ts / d.ts.map bytes は一致） | [records/after/witness-bundle-metadata-t1.log.gz](records/after/witness-bundle-metadata-t1.log.gz) |
| packet ↔ probe（`bundle_metadata_t1_parsed_packet_matches_typescript_after_javascript_probe`） | 15（outFile + declaration + JS の bundle 行；`source-file-roots` / `no-declaration` / `emit-declaration-only` は packet を作らないため対象外） | **exact 12 / known 3 / failed 0**（`PACKET SUMMARY exact=12 known=3 failed=0 probed=15`）。comment range の行は 15 件すべて完全一致（known 3 行は flag のみの差、§3.2） | 同上 |
| runner | — | observer `--check` 一致、`test result: ok. 2 passed; 0 failed; 8 filtered out`、`{"suites": ["bundle-metadata-t1"] …}`（[records/after/witness-summaries.jsonl](records/after/witness-summaries.jsonl)） | 同上 |

family 別（complete command / packet）：receiver 8 行 = 7 exact + 1 known（compound、map のみ）/ 7 exact + 1 known（static-set、flag のみ）；
decorated 2 = 2 exact / 2 known（flag のみ）；mount-order 2 = 2 / 2 exact（JSON 除外で declaration arena の index が 1 ずれる；
`MOUNT` 行が両 arena の順序を記録）；utf16 1 = 1 / 1 exact（非 BMP の前置で byte ≠ UTF-16、restore 後の range 端点が上流の UTF-16 値と一致）；
lifetime 3 = 3 exact（packet 無し）；residual 2 = 2 known（map のみ）/ 2 exact（packet は一致）。

前段の観測（凍結前の同じ bytes、[records/after/contract-pre-freeze.log.gz](records/after/contract-pre-freeze.log.gz)）：complete 15 / 0 / 3、packet 12 / 0 / 3 で、
凍結した 3 + 3 行と一致する。

### 3.2 既知差分として凍結した行（本スライス外の owner）

| fixture | case | 内容 | owner | 証拠 |
| --- | --- | --- | --- | --- |
| `bundle-metadata-t1-known-native.json` | `residual/es2015/system-export-class-map`、`residual/es2015/system-hoisted-class-map` | JS bytes・declaration・d.ts.map は一致、`bundle.js.map` のみ差：System は top-level class を `A = class A … };` に hoist し、上流はその `};` 行（生成 line 14 / 29）の col 13 / 14 に class 終端の 2 segment を出す。Rust は出さない（exported でも unexported でも同じ） | System transform の hoisted class 文 range（C02 REPORT §3 の R12 と同じ症状；R12 は native decorator 経路の記録だったが、undecorated class 全般で再現） | [records/residual/system-export-class-map-diff.txt](records/residual/system-export-class-map-diff.txt)、[records/residual/system-hoisted-class-map-diff.txt](records/residual/system-hoisted-class-map-diff.txt)（`mapdiff.py` で segment を復号） |
| `bundle-metadata-t1-known-native.json` | `receiver/es2015/static-compound-second-file` | JS・declaration・d.ts.map は一致、`bundle.js.map` のみ差：`B.#p += 1` の copiable receiver temp（`_b = _a` / `_b`）を Rust は source の `B` に map する（生成 line 27 col 60 / 62 → source (2,20) / (2,21) の 2 segment 余分）。上流は synthesized temp を map しない。parse node への comment range は両者とも無し（clone 経路、DESIGN §3） | `class_fields/downlevel.rs` の copiable receiver temp の range（C02 決定 11 `stabilize_receiver` と同系） | [records/residual/static-compound-map-diff.txt](records/residual/static-compound-map-diff.txt) |
| `bundle-metadata-t1-known-packet.json` | `receiver/es2015/static-set-second-file` | Rust のみ：`B.#p = v` の右辺 parsed Identifier `v` に `flags: 2048`（NO_TRAILING_COMMENTS）。上流は emitNode を作らない | `class_fields/downlevel.rs:7700-7708` `create_private_set` の tsrs-native な comment 境界（helper 引数内と外側 original-linked 式の二重 emit 防止）。JS bytes は一致 | probe 差分（[records/after/contract-pre-freeze.log.gz](records/after/contract-pre-freeze.log.gz)） |
| 同上 | `decorated/es2022/static-get-second-file`、`decorated/esnext/static-get-second-file` | 上流のみ：decorator 式の parsed Identifier `dec`（pos 63-66）に `flags: 3072`（NoComments）。`transformDecorator`（`_tsc.js:100554-100556` `setEmitFlags(expression, 3072)`）が visit 済み parse node に書く。Rust `standard_decorators.rs` は同じ flag を parse node に残さない。JS bytes は一致 | standard decorator transform（parse node への flag 書込み）。declaration には decorator 式が出ないため出力差は無い | 同上 |

いずれも comment range ではなく、JS / declaration / d.ts.map の bytes は上流と一致する（差は JS map の segment か parse node の flag のみ）。凍結行は両 pass で厳密一致を要求し、
exact になった場合は retire を要求して fail する（known-native と同じ規約）。互換成功の件数には数えない。

最初の採取（System、`export class`、16 件版）では bundle 12 件が上記 map 差で不一致になり、class を unexported にした 17 件版でも
同じ差が残った（System は unexported class も hoist する）。producer と packet の検証を分離するため、`residual` 2 行だけを System に残し、
他の bundle 行は AMD で採り直した（DESIGN §8）。差し替え前の 2 版の入力は採用していない（記録：[records/residual/](records/residual/)）。

## 4. 隣接検証

| 入口 | 結果 | 記録 |
| --- | --- | --- |
| `cargo test --manifest-path crates/emitter/Cargo.toml --lib parsed_metadata_` | 6 passed（既存 4 + 新規 2：別 source を指す EndOnly / StartOnly / Original / Synthesized の往復と mount 反転、source 検証と atomic restore；拒否 test に synthetic comment / source-map range を追加） | [records/after/unit-parsed-metadata.log.gz](records/after/unit-parsed-metadata.log.gz) |
| `cargo test --manifest-path crates/emitter/Cargo.toml --lib parsed_`（constants test と他 module の `parsed_*` を含む） | 12 passed | [records/after/unit-parsed-all.log.gz](records/after/unit-parsed-all.log.gz) |
| `python3 scripts/witness.py bundle-declarations --all`（3 tests + 1 filtered） | 3 passed / 1 filtered（既存 packet 契約：visitor / map / metadata lifetime の ordinary + fresh-forced、fixture 不変） | [records/after/bundle-declarations.log.gz](records/after/bundle-declarations.log.gz) |
| `python3 scripts/witness.py bundle-program --all`（4 tests） | 4 passed | [records/after/bundle-program.log.gz](records/after/bundle-program.log.gz) |
| `python3 -m unittest discover -s .github/ci -p test_replay.py`（planner 59 tests） | OK（新 suite の件数 17 と retire 後の known 4 を反映） | — |
| `cargo fmt --all -- --check`、`git diff --check` | exit 0 | — |

printer / generated binding 自体は変更していない（DESIGN §7）。

## 5. 未実行（新 source で）

- 全 767 件の `decorator-binding-pipeline --all`（目標 763 exact / 4 known / 767 complete）、SUPER / retained / acceptance chain、
  hosted witness / acceptance の全 job：統合担当。
- clippy `-D warnings` 全体：既存 warning（program）で止まるため未実行。変更 file への指摘は無い前提を統合側で確認。
- 同一 Program の API 再 emit（`Program.emit` 2 回目）の Rust 側比較：上流 probe のみ（17 件 identical）。H2.9 / API1 の owner。

## 6. 提出物と統合担当への引き継ぎ

- patch / commit：§1（変更 file は DESIGN §7；`.github/ci/test_replay.py` は planner test の件数のみ）。retire した known-native ID：[records/retired-known-native-ids.txt](records/retired-known-native-ids.txt)。
- hosted 入口：`scripts/witness.py` の `COMPILER_DIRECT["bundle-metadata-t1"]`（controls group に自動編入、`.github/ci/replay.py` は
  `COMPILER_DIRECT` を列挙するため追加登録不要；planner test の件数 17 と known 4 は更新済み）。
  hosted job の実行・時間予算の確認は統合担当。
- 残る owner：R9 ×2、R12 ×2（既存 known-native 4 行）、§3.2 の 3 種（System export class map、private-set 右辺 flag、decorator 式 NoComments）、
  dispose 後 print の typed 差分 2 件（direct suite）。
- `E-METADATA-BASE` の bundle packet 行は `modified-requalify`：最終 validation ref での再 qualify は統合担当。
