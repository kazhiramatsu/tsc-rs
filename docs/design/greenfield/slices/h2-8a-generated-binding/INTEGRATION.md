# C02 / A41-BINDING — 統合仕様（提出物、hosted 入口、再適用）

**本書は提出仕様。現行 registry / hosted への登録と追加 10 controls は [統合記録](integration/revised/README.md) を参照。**

作成日：2026-09-16。状態：隔離候補の提出。統合・admission は統合担当が持つ。
設計は [DESIGN.md](DESIGN.md)、結果は [REPORT.md](REPORT.md)。

## 1. 変更ファイル

| 種別 | path | 内容 |
| --- | --- | --- |
| production | `crates/emitter/src/builtins/generated_bindings.rs` | `GeneratedBindingScopes::generated_names`（tsc `generatedNames`）：file-level / file-wide / numbered 名が書き込み、scoped / temp / private-temp / numbered / file-wide の割当が参照。bundle への seed / export |
| production | `crates/emitter/src/builtins/target_bindings.rs` | transformer-time finalize の reserved set を parse census（`ParsedSourceIdentifierNames`）に統一。bundle には `generated_names` 全体を持ち越す |
| production | `crates/emitter/src/builtins/standard_decorators.rs` | planner `used_names` を parse census から開始（synthetic identifier を数えない）；`reserved_private_generated_names`（enclosing class の private 記憶域名を inner class の割当から除く）；decorated private auto-accessor の backing field / descriptor forwarder の source map range（決定 15） |
| production（分離差分） | `crates/emitter/src/factory/parsed_metadata.rs` | bundle の parse-node metadata packet に `internal_flags` を含める（決定 7） |
| production（分離差分） | `crates/emitter/src/builtins/system.rs` | hoisted 宣言が generated binding identity を運ぶ（決定 8） |
| production（分離差分） | `crates/emitter/src/builtins.rs` | CJS `export default` 文は `setTextRange` のみ（決定 9） |
| production（分離差分） | `crates/emitter/src/builtins/class_fields/downlevel.rs` | earlier transform 由来の generated 名 property の leading map 抑止（決定 10）、receiver clone の位置保持（決定 11） |
| production（分離差分） | `crates/emitter/src/builtins/class_fields.rs` | private-name scope に synthesized private member 名を seed（決定 13） |
| production（分離差分） | `crates/emitter/src/factory.rs`、`crates/emitter/src/transform.rs` | `generated_binding_identity` accessor（決定 12）、`CarriedGeneratedNames`（決定 4） |
| production（分離差分） | `crates/emitter/src/printer.rs`、`crates/emitter/src/printer/bundle.rs` | failure 後の名前表の持ち越し（決定 4） |
| test | `crates/emitter/tests/printer_failure_contract.rs`、`crates/emitter/tests/fixtures/printer-failure-known-native.json` | C03 の KNOWN `recover-new-unique#op2` を撤去（exact になった）；negative control は空集合を許す |
| tool | `scripts/decorator-binding-capture-diff.py` | `TSC_RS_H2_8A_CAPTURE_WRITES_DIR` の capture を case ごとに要約（JS / declaration の差分行、map 差、diagnostics、exit） |
| observer | `scripts/observe-decorator-bindings.mjs` | `pipeline` / `direct` の 2 mode |
| generator | `scripts/generate-decorator-binding-inputs.mjs` | 128 variant × 6 = 768 の入力 manifest |
| fixture | `crates/compiler/tests/fixtures/decorator-binding-inputs.json`、`crates/compiler/tests/fixtures/decorator-binding.json.zst` | manifest と複合 command の凍結観測（767 complete + 1 upstream exception） |
| fixture | `crates/emitter/tests/fixtures/decorator-binding-direct.json` | direct 146 row（96 + 統合レビュー後の failure-carry 25 + scope 25） |
| fixture | `crates/compiler/tests/fixtures/decorator-binding-known-native.json` | 複合 command の残差 9 row の native 観測（typed error / write の SHA-256・exit・diagnostics）。comparator が `known` として両 pass で assert し、exact になった row は retire を要求して fail |
| test | `crates/compiler/tests/decorator_binding_pipeline_contract.rs` | 複合 command の replay（SUPER comparator と同じ `assert_completed_observation`（比較のみ、Program は再構築しない）、pass ごとに Program 1 個 × 2 pass、`TSC_RS_DECORATOR_BINDING_CASE_SET` 選択、`TSC_RS_H2_8A_CAPTURE_WRITES_DIR` capture） |
| test | `crates/emitter/tests/decorator_binding_contract.rs` | direct 3 group（synthetic は `print` と `print_javascript_with_global_names` の 2 route）、`KNOWN_DIVERGENCES`、`TSC_RS_DECORATOR_BINDING_REPORT_DIR` |
| runner | `scripts/witness.py` | `decorator-binding`（EMITTER_DIRECT、146 row）、`decorator-binding-pipeline`（BINDING：SUPER 型の env 選択、pipeline observer の `--check` を replay 前に実行、`exact + known == selected` と `--all` の `known == 凍結 row 数` を要求）。capture / report / known-native dump の環境変数は継承しない。既存 `--dry-run` の tuple observer 表示を修正 |
| docs | `docs/design/greenfield/slices/h2-8a-generated-binding/` | DESIGN / REPORT / INTEGRATION、records |

## 2. Hosted 入口（未登録：統合担当が `.github/workflows/witness.yml` に追加）

| suite | 対象 | 件数 | 想定時間 | 入口 |
| --- | --- | ---: | --- | --- |
| `decorator-binding` | direct 3 group（printed text） | 146 row × 2 route/1 route、各 2 回（194 比較、192 exact + 2 typed KNOWN） | observer 数十秒 + emitter build、replay は秒 | printer job の EMITTER_DIRECT 集合に同居（literal-update と同じ tuple observer） |
| `decorator-binding-pipeline` | 複合 command | 767 complete（+1 recorded exception）× 2 pass（pass ごとに Program 1 個） | ローカル実測 13 分（他 job 無し）〜 21 分（build と並走）、nice 15 単一 thread；observer `--check` は約 11 分 | 独立 job を推奨（SUPER primary と同じ形。`python3 scripts/witness.py decorator-binding-pipeline --all`）。60 分制限内に収まらない場合は `--case /parse-census/`、`/global/`、`/nested/`、`/reserved/`、`/computed/`、`/ordering/`、`/lifecycle/` の family 分割 |

`cargo xtask acceptance` はこれらを実行しない（ts-tests 由来の suite のみ）。
両 target とも observer の `--check` を先に走らせ、0 test / 選択外 / 失敗を成功扱いしない
（`witness.py` の `run_binding` がこれを実装する：pipeline observer `--check` → replay →
`SUMMARY exact=… known=… failed=0 selected=…` の検査、`--all` では `known` が
`decorator-binding-known-native.json` の row 数と一致すること）。`--all` の期待値は
exact 758 / known 9 / failed 0、exit 0。planner（変更 file → suite の選択：共有 observer
`scripts/observe-decorator-bindings.mjs` は両 suite を選ぶ）と台帳の更新は現在の registry 側で
統合担当が行う（`binding_inputs(suite)` が入力集合を返す）。

## 3. 再適用

```sh
git fetch origin main
git worktree add -b integrate/h2-8a-generated-binding ../tsc-rs-generated-binding-int <base>
cd ../tsc-rs-generated-binding-int
git apply --check ../tsc-rs-generated-binding/docs/design/greenfield/slices/h2-8a-generated-binding/records/candidate.patch
git apply ../tsc-rs-generated-binding/docs/design/greenfield/slices/h2-8a-generated-binding/records/candidate.patch
# 凍結 expected の再検証（別 process、上流 2 回一致；artifact は observer の自己 hash を含むので
# 共有 observer を変えたら両 artifact を再採取する）
node scripts/observe-decorator-bindings.mjs direct --check
node scripts/observe-decorator-bindings.mjs pipeline --check
# focused
python3 scripts/witness.py decorator-binding --all
python3 scripts/witness.py decorator-binding-pipeline --case /reserved/esnext/set/
```

`candidate.patch` は開始 SHA `ccb6661c1` からの `crates/` と `scripts/` の全差分（§1 の production 12 file
= 候補 3 file + 分離差分 9 file、test 4 file、fixture 5 file（known-native を含む）、observer / generator / diff script、
`witness.py`）、`candidate.patch.sha256` がその hash、`candidate.stat` が `--stat`。凍結 fixture の byte は
patch に含まれる（`.json.zst` は binary patch）。docs（本 directory）は worktree からそのまま取り込む。
旧 v18 / SUPER candidate の patch を重ねて適用しない。

分離差分を個別に採否したい場合は、`records/candidate.patch` を `git apply --include=<path>` で
file 単位に分けて当てる（決定 4 の carry は `printer.rs` / `printer/bundle.rs` / `transform.rs` /
`target_bindings.rs` / `generated_bindings.rs` / `factory.rs` を一組で扱う。決定 7〜13 は各 1 file で
独立。`printer_failure_contract.rs` と `printer-failure-known-native.json` の KNOWN 撤去は決定 4 と
一組）。

## 4. 既知の差分と未完了（REPORT.md §3 / §5 の詳細）

- direct `lifecycle/dispose/*#after_dispose`（2 row）：Rust は dispose 後の print を `InvalidLifecycle` で
  拒否（session model）。typed KNOWN（`decorator_binding_contract.rs` の `KNOWN_DIVERGENCES`）、exact 非加点。
- direct `lifecycle/failure/*#op2`（5 row）と C03 `recover-new-unique#op2` は決定 4 で exact になり、
  KNOWN を撤去した（`printer-failure-known-native.json` は `cases: []`）。
- 複合 command の残差（最終候補、REPORT.md §2.2.3 / §3）：R9 esnext/define の default class 名 2 row
  （`es_next.rs`）、R12 esnext/define の System bundle 文末 map 2 row（`system.rs`）、T1 lowered 5 row
  （bundle の parse-node metadata 可搬性、第 2 原因）。いずれも開始 SHA から同じ症状で、decorator の
  生成名 owner ではない。9 row の native 観測は `crates/compiler/tests/fixtures/decorator-binding-known-native.json`
  に凍結し（typed error の Debug 文字列（NodeId は `_`）、または write の path + SHA-256 + byte 数、
  exit code、status、diagnostics）、comparator は両 pass でそれと一致することを assert して `known` に
  数える。exact になった row は「retire せよ」で fail、凍結 id が観測 fixture に無ければ load で fail。
  除外・任意 panic の受容・exact への加算はしない。
- 統合レビュー（別 worktree の `integration/README.md`）の F1 / F2 と項目 2〜6 への対応は
  [REVIEW-RESPONSE.md](REVIEW-RESPONSE.md)。
