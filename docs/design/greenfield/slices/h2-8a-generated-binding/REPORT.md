# C02 / A41-BINDING — 結果報告（before / after、残差、未完了）

作成日：2026-09-16。設計と対応表は [DESIGN.md](DESIGN.md)、統合仕様は [INTEGRATION.md](INTEGRATION.md)。
全件 replay は hosted の担当（[共通手順](../claude-high-difficulty-handoffs.md)）。本書のローカル数値は
focused 集合と、最終 bytes での 1 回の全件 replay である。

## 1. 計測の枠

| 集合 | 件数 | 比較 | before（開始 SHA production） | after（候補） |
| --- | ---: | --- | --- | --- |
| direct `synthetic` | 48 row × 2 route（`print` / `print_javascript_with_global_names`） | JS text、各 2 回 | §2.1 | §2.1 |
| direct `global` | 22 row | printer text + status、各 2 回 | §2.1 | §2.1 |
| direct `lifecycle` | 26 row | op ごとの status + text、各 2 回 | §2.1 | §2.1 |
| pipeline 複合 command | 767 complete + 1 upstream exception | 複合 command tuple（writes / diagnostics / emit result / status / exit）、2 pass × 2 Program 構築 | 688 / 767（§2.2） | 723 / 767（§2.2.2；残 44 は owner 明記） |
| 隣接 focused | `--lib target_bindings` 5、`--lib generated_bindings` 19、`witness.py printer --all` 10 | 既存 | 全 green | §2.3 |

before の pipeline replay は開始 SHA production で build した binary
（[records/before/pipeline-binary.sha256](records/before/pipeline-binary.sha256)）を、
候補の編集後に直接実行した（cargo の再 build を経ない）。

## 2. 結果

### 2.1 direct

（[records/before/direct-contract.log](records/before/direct-contract.log)、
[records/after/direct-contract.log](records/after/direct-contract.log)、
native 観測は各 `direct/decorator-binding-direct-native.json`）

| group | route | before exact | after（第 1 候補、3 file） | after（最終、分離差分込み） | 差の内訳 |
| --- | --- | ---: | ---: | ---: | --- |
| synthetic | plain | 18 / 48 | 48 / 48 | 48 / 48 | before：`_x_decorators` 5、`_outerThis` 5、`_a` 5、`class_1` 5（esnext/define は native decorator で自明一致） |
| synthetic | oracle | 38 / 48 | 48 / 48 | 48 / 48 | before：`_x_decorators` 5、`_outerThis` 5 |
| global | direct | 22 / 22 | 22 / 22 | 22 / 22 | hit / hit-chain / miss / error × 5 domain、parsed+hit 2：before から一致 |
| lifecycle | direct | 19 / 26 | 19 / 26 | 24 / 26 | before：failure after-fault 5 + dispose 2。最終候補は failure carry（DESIGN §6.4、決定 4・14）で after-fault 5 row を閉じ、dispose 2 row だけが typed KNOWN |
| 合計 | | 107 / 144 | 137 / 144 | **142 / 144** | 37 → 7 → 2（dispose、typed KNOWN） |

synthetic group の各 row は `identity_trace`（binding ごとの宣言 / 参照 occurrence と綴り、決定 12）を
`records/after4/direct/decorator-binding-direct-native.json` に保存し、1 binding = 1 綴りを assert する。
最終候補では C03 の printer suite も `recover-new-unique#op2` が exact になり、25 / 25、KNOWN 0
（[records/after4/adjacent-printer.log](records/after4/adjacent-printer.log)）。

### 2.2 pipeline（複合 command）

（[records/before/pipeline-contract.log](records/before/pipeline-contract.log)、差分要約
[records/before/capture-diff.txt](records/before/capture-diff.txt)（`scripts/decorator-binding-capture-diff.py`
の出力。生の capture は 69 MB のため repo 外へ退避、`TSC_RS_H2_8A_CAPTURE_WRITES_DIR` で再生成可）、
after は [records/after/](records/after/)）

before（開始 SHA の binary、767 complete、2 pass × 2 Program）：**exact 688 / failed 79**。
失敗の内訳（class は §3）：

| class | variant（× combo） | 件数 |
| --- | --- | ---: |
| R1 sibling `_classThis` domain | `reserved/file-level-then-{static-private-field, static-private-method, static-private-accessor, static-auto-accessor, static-private-auto-accessor, decorated-static-private-auto-accessor}` × 5、`lifecycle/cjs-sibling-file-level-then-scoped` × 5 | 35 |
| R2 bundle の持ち越し | `bundle-outer-this-two-files` × 5 | 5 |
| T1 bundle 内 static private の typed error | `lifecycle/bundle-file-level-then-scoped-across-files` × 5（capture 無し、§2.2.1） | 5 |
| R3 nested private 記憶域 | `reserved/private-accessor-storage-nested` × 5 | 5 |
| R5 System hoisted default 名 | `lifecycle/bundle-default-two-files` × 5（lowered combo） | 5 |
| R6 source map のみ | `reserved/static-private-method-then-file-level` × 5、`private-accessor-storage-siblings` × 5、`private-and-public-same-stem` × 5、`lifecycle/cjs-default-export` × 5、esnext/define の `bundle-computed-temps-two-files` と `bundle-file-level-then-scoped-across-files`（native decorator、System bundle の map） | 22 |
| R9 native decorator の default class 名 | esnext/define の `global/script-let-default_1`（`class default_2` → `default_1`：global を見ない）、`lifecycle/bundle-default-two-files`（2 番目の source も `default_1`） | 2 |
| 合計 | | 79 |

esnext/define（decorator が native のまま）の 4 件は decorator の生成名 owner ではない（R6 / R9）。
それ以外の 15 variant × 5 lowered combo = 75 件のうち、R1 + R2 + R3 の 45 件が本候補の修復対象、
R5 / R6 / T1 の 30 件は owner を明記して残す（§3）。

after は 3 段階で計測した：第 1 候補（[records/after/](records/after/)、候補 3 file の修復）、
第 2 候補（[records/after4/](records/after4/)、分離差分 = 決定 4・7〜13 を含む）、最終候補
（[records/after5/](records/after5/)、決定 15 を含む最終 bytes、全件 1 回）。

#### 2.2.1 第 1 候補（records/after）

subset（`/reserved/`, `/lifecycle/`, `/nested/` の 336 case）：exact 293 / failed 43（before の同じ 336 は
263 / 73）。全件（767）：**exact 723 / failed 44**（before 688 / 79）。R1（35）、R2（5）、R3（5）が閉じ、
残り 44 は R6 map のみ 32、R5 5、T1 5、R9 2（[records/after/capture-diff-full.txt](records/after/capture-diff-full.txt)）。

#### 2.2.2 第 2 候補の family subset（records/after4）

（[records/after4/pipeline-subsets.log](records/after4/pipeline-subsets.log)、要約
[records/after4/capture-diff-subsets.txt](records/after4/capture-diff-subsets.txt)）

`/reserved/`, `/lifecycle/`, `/nested/`, `/computed/`, `/global/esnext/define/` の 452 case：
**exact 434 / failed 18**。第 1 候補の残差のうち R5（5）、R6 の `static-private-method-then-file-level`
（5）、`cjs-default-export`（5）、`private-accessor-storage-*` / `private-and-public-same-stem` の ES2015
（6）と ES2022+ の backing field、T1 の第 1 原因（esnext/define の 1 件は map のみに変化）が閉じた。
残り 18 の内訳：

| class | 件数 | 内容 |
| --- | ---: | --- |
| T1 typed error（第 2 原因） | 5 | `lifecycle/bundle-file-level-then-scoped-across-files` lowered combo：`ParsedEmitMetadataNotPortable`（parsed Identifier の `comment_range: EndOnly`、§3） |
| R11 descriptor forwarder の leading map | 9 | ES2022 set / define、ESNext set の `private-accessor-storage-{nested,siblings}`、`private-and-public-same-stem`：`get #a()` / `set #a(value)` の先頭 segment が `(10, 9)`（上流 `(10, 4)`、decorator を含む range）。決定 15 で修復（§2.2.3） |
| R9 native decorator の default 名 | 2 | esnext/define の `global/script-let-default_1`、`lifecycle/bundle-default-two-files` |
| R12 System bundle（native decorator）の文末 map | 2 | esnext/define の `lifecycle/bundle-computed-temps-two-files`、`bundle-file-level-then-scoped-across-files`：`bundle.js` の decorated export class を閉じる `};` 行で上流は `;`（col 13）と行末（col 14）に class 終端 `(11, 1)` の 2 segment を出し、Rust は出さない。decorator 生成名ではなく System の export 文の range（native 経路） |

#### 2.2.3 最終候補（records/after5）

決定 15 の対象 5 variant（`private-accessor-storage-{nested,siblings}`、`private-and-public-same-stem`、
`file-level-then-decorated-static-private-auto-accessor`、`decorated-static-private-auto-accessor-then-file-level`）
× 6 combo = 30 case の focused subset（[records/after5/pipeline-subsets.log](records/after5/pipeline-subsets.log)）：
**exact 30 / failed 0**（R11 の 9 row が閉じ、ES2015 と esnext/define に退行無し）。

全件（767、最終 bytes で 1 回、[records/after5/pipeline-contract-full.log](records/after5/pipeline-contract-full.log)、
binary [records/after5/pipeline-binary.sha256](records/after5/pipeline-binary.sha256)、要約
[records/after5/capture-diff-full.txt](records/after5/capture-diff-full.txt)）：
**exact 758 / failed 9**（before 688 / 79、第 1 候補 723 / 44）。残り 9 は §3 の owner 明記 class と一致し、
それ以外の family / combo に退行は無い（parse-census 174、global 125+1、nested 90、computed 95 + 1 upstream
exception、ordering 36、reserved 168、lifecycle 71 が exact）：

| class | 件数 | 件 |
| --- | ---: | --- |
| T1 typed error（第 2 原因、before と同一） | 5 | `lifecycle/bundle-file-level-then-scoped-across-files` lowered combo |
| R9 native decorator の default 名 | 2 | esnext/define の `global/script-let-default_1`、`lifecycle/bundle-default-two-files` |
| R12 System bundle（native）の文末 map | 2 | esnext/define の `lifecycle/bundle-computed-temps-two-files`、`bundle-file-level-then-scoped-across-files` |

修復した class：R1 35、R2 5、R3 5、R5 5、R6 32 のうち 30（残り 2 は R12 として再分類）、R10、R11 9 —— before の
79 件のうち 70 件が閉じ、typed error の第 1 原因も閉じた。

### 2.3 隣接 focused（最終候補 bytes、records/after5；第 1 候補の同じ表は records/after）

| 入口 | 結果 | 記録 |
| --- | --- | --- |
| `cargo test --lib generated_bindings` | 19 passed | [records/after5/lib-generated-bindings.log](records/after5/lib-generated-bindings.log) |
| `cargo test --lib target_bindings` | 5 passed | [records/after5/lib-target-bindings.log](records/after5/lib-target-bindings.log) |
| `cargo test --lib printer` | 11 passed | [records/after5/lib-printer.log](records/after5/lib-printer.log) |
| `witness.py decorator-binding --all`（observer `--check` + replay） | 1 test、142 exact / 144、2 KNOWN（dispose） | [records/after5/adjacent-decorator-binding.log](records/after5/adjacent-decorator-binding.log) |
| `witness.py direct --all`（SUPER direct 32） | 28 exact、4 recorded divergence（従来どおり）、0 failed | [records/after5/adjacent-direct.log](records/after5/adjacent-direct.log) |
| `witness.py printer --all`（C03 / A-INT3-CS / API1.2-HINT の 142 row） | 10 tests passed、hooks 25 / 25 exact、KNOWN 0（C03 の `x_2` 行が閉じた）、REVIEW 21 / 21 | [records/after5/adjacent-printer.log](records/after5/adjacent-printer.log) |
| `witness.py literal-update --all`（C01） | 3 tests passed：generic exact 315 / divergent 0、typed exact 399 / divergent 0 | [records/after5/adjacent-literal-update.log](records/after5/adjacent-literal-update.log) |
| `cargo test --test contracts`（emitter 452） | 451 passed、1 failed = 開始 SHA から継承の `compact_private_function_body_emits_inter_statement_comment_once`（C01 記録と同一） | [records/after5/adjacent-emitter-contracts.log](records/after5/adjacent-emitter-contracts.log) |
| SUPER 複合 subset（`followup3 --all`、`extra --case phase-order/`、`followup2 --case private-name/`、`primary --case handoff/`） | 48 / 48、24 / 24、36 / 36、58 / 58 exact ×2（0 failed） | [records/after5/adjacent-followup3.log](records/after5/adjacent-followup3.log) 等 |
| `cargo fmt --all -- --check` | exit 0 | [records/after5/fmt-check.log](records/after5/fmt-check.log) |
| clippy（emitter all-targets、compiler の新 target、deny 無し） | exit 0 / 0。emitter 168 warning は全て既存（program 144、downlevel 6 / class_fields 3 / system 1 / builtins 1 は本候補の hunk 外の行）、compiler 311 warning に新 target への指摘 0。`-D warnings` は開始 SHA と同じく program の既存 warning で停止する（C03 と同じ） | [records/after5/clippy-emitter-allow.log](records/after5/clippy-emitter-allow.log)、[records/after5/clippy-compiler-binding-allow.log](records/after5/clippy-compiler-binding-allow.log) |

530 retained / 672 primary / 156+162 follow-up の全件は hosted の担当（INTEGRATION.md §2）。

## 3. 残差の disposition

before で再現した差の全 class と、最終候補後の扱い。件数は §2.2 の表。

| class | family / 例 | 症状（上流 → Rust @ before） | owner | 最終候補後 |
| --- | --- | --- | --- | --- |
| R1 sibling `_classThis` domain | `reserved/file-level-then-<static private / static auto-accessor>` 6 variant、`lifecycle/cjs-sibling-file-level-then-scoped` | `_classThis_1` → `_classThis`（file-level 名が `generatedNames` に入らず、後続の scoped 名が衝突を見ない） | decorator 生成名 finalizer（`generated_bindings.rs`） | **修復**（DESIGN §5 決定 1） |
| R2 bundle の名前表持ち越し | `lifecycle/bundle-file-level-then-scoped-across-files`、`bundle-outer-this-two-files` | 2 番目の source で `_classThis_1` / `_outerThis_1` → `_classThis` / `_outerThis`（numbered 名だけを持ち越していた） | 同上 | **修復**（決定 1：集合全体を持ち越す） |
| R3 nested private 記憶域 | `reserved/private-accessor-storage-nested` | inner class の `#a_1_accessor_storage` → `#a_accessor_storage`（private 生成名は nested scope に予約される） | `standard_decorators.rs` `allocate_private_storage` | **修復**（決定 6） |
| R4 synthetic identifier の混入 | direct `synthetic` 30 row | `_x_decorators` / `_outerThis` / `_a` / `class_1` → `_1` / `_b` / `class_2` | planner `used_names`、transformer-time finalize の reserved set | **修復**（決定 2・3） |
| R5 System bundle の hoisted default 名 | `lifecycle/bundle-default-two-files`（lowered combo） | 2 番目の source の hoisted `var …, default_2, …` → `default_1`（`hoisted_names: Vec<String>` が綴りだけを保持） | `builtins/system.rs` | **修復**（決定 8、分離差分） |
| R6 source map のみ | `reserved/static-private-method-then-file-level`（`B.#m()` の receiver）、`private-accessor-storage-siblings` / `private-and-public-same-stem`（backing field の初期化文 / ES2022+ の backing field）、`lifecycle/cjs-default-export`（`exports.default = default_1;`） | JS byte は一致、`.js.map` の mappings が差 | class-fields downlevel、CommonJS visitor、decorator の backing field range | **修復**（決定 9・10・11、backing field の `setSourceMapRange`、分離差分） |
| R7 failure 後の名前表 | direct `lifecycle/failure/*/after-statement-1` 5 row、C03 `recover-new-unique#op2` | `x_2` / `_s_1` / `_o_1` / `_b` → fresh と同じ | printer の名前表の寿命（DESIGN §6） | **修復**（決定 4・14、分離差分；両 contract の KNOWN を撤去） |
| R8 dispose 後の print | direct `lifecycle/dispose/*` 2 row | 上流は再 print 可、Rust は `InvalidLifecycle` | session model | **typed KNOWN**（`decorator_binding_contract.rs`） |
| R9 native decorator の default class 名 | esnext/define の `global/script-let-default_1`、`lifecycle/bundle-default-two-files` | `class default_2` → `class default_1`：ESNext × define では esDecorators が走らず、匿名 default class の名前は module transform（`es_next.rs` `hoist_binding_identifier` / `allocate_numbered`）が決める。global `default_1` と前 source の `default_1` を見ない | `builtins/es_next.rs`（+ TypeScript transform の default 名） | **OUT-OF-SCOPE**（decorator 経路外）。修復案：module transform の default 名を `TargetBinding::allocate_numbered` に載せ、finalizer の global oracle / bundle `generated_names` を通す |
| R10 class-fields の private 記憶域名の重複 | `reserved/private-and-public-same-stem` ES2022+ | public `accessor a` の記憶域 `#a_1_accessor_storage` → `#a_accessor_storage`（decorator 側の synthesized private 名を見ない） | `class_fields.rs` `retained_private_storage` | **修復**（決定 13、分離差分）。逆順（public accessor が先）は記録のみ |
| R11 descriptor forwarder の leading map | ES2022 / ESNext × set の `private-accessor-storage-*`、`private-and-public-same-stem` 9 row | `get #a()` / `set #a(value)` の先頭 mapping が decorator の後ろ（上流は member 全体） | `standard_decorators.rs` `create_auto_accessor_members` | **修復**（決定 15） |
| R12 System bundle（native）の文末 map | esnext/define の `lifecycle/bundle-computed-temps-two-files`、`bundle-file-level-then-scoped-across-files` | decorated export class を閉じる `};` の `;` と行末の 2 segment（class 終端）が無い | `builtins/system.rs` の export 文 range（native decorator 経路、decorator 生成名ではない） | **OUT-OF-SCOPE**（記録のみ） |
| T1 bundle 内の static private の typed error | `lifecycle/bundle-file-level-then-scoped-across-files`（lowered combo） | before / after とも `ParsedEmitMetadataNotPortable` | bundle × parse-node metadata 可搬性（H2.7d / A-INT3-CS） | **部分修復**：第 1 原因（`InternalEmitFlags` 32）は決定 7 で閉じ、第 2 原因（parsed Identifier に printer の comment ownership が書く `comment_range: EndOnly` を snapshot が拒む）が残る。**OUT-OF-SCOPE**（生成名ではない、開始 SHA から同じ typed error） |
| U1 upstream exception | `computed/esnext/set/static-accessor-decorated` | tsc `Debug Failure: Undeclared private name for property declaration`（SUPER `handoff/static-*-accessor` と同じ上流不具合） | 上流 | 記録のみ、加点なし |

before で **一致していた**面（追加対照が確認した既存 owner）：parse census の全 29 variant
（JSDoc tag / label / type 位置 / escape / `_1` 既取得 / temp 列）、global の全 21 variant
（script の let / function / type alias / namespace / `declare let` / block-scoped / numbered chain /
temp chain、module local、`declare global`、script+source）、nested の全 15 variant
（inner の 7 位置、sibling の `_outerThis_1` / `class_2` / `default_1`+`class_1`、通常関数 scope、
三重入れ子）、computed 16 variant、ordering 6 variant、reserved の反対順序と負の trigger、
direct `global` 22 row。これらは既存 code が正しい証拠として維持する。

## 4. 修復した差（DESIGN.md §5 の決定に対応）

| 決定 | file | 変更 |
| --- | --- | --- |
| 1 | `generated_bindings.rs` | `GeneratedBindingScopes::generated_names`（tsc `generatedNames`）。書込：`reserve_planned_file_level_optimistic_with_policy`、`reserve_file_wide`、`allocate_source_numbered_with_policy`（非 reserved）。参照：`reserve_in_current`、`reserve_in_source`。bundle：`seed_generated_names` / `generated_names()` |
| 1 | `target_bindings.rs` | bundle の seed を reserved set から `generated_names` へ移し、finalize 末尾で集合全体を返す（`shared_numbered_bindings` を廃止） |
| 2 | `target_bindings.rs` | `GeneratedNameReservedSetPolicy::TransformerRoot` の reserved set = `ParsedSourceIdentifierNames` |
| 3 | `standard_decorators.rs` | planner `used_names` の初期値 = parse census |
| 6 | `standard_decorators.rs` | `reserved_private_generated_names`：enclosing class の `backing_name` を inner class の `used_private` に加える。class の終わりで復元 |
| 7 | `factory/parsed_metadata.rs` | bundle の parse-node metadata packet に `internal_flags` を含める（T1 の第 1 原因） |
| 8 | `system.rs` | hoisted 宣言名が generated binding identity を運ぶ（`collect_binding_name_nodes` + `generated_binding_of_identifier` → `generated_bindings`） |
| 9 | `builtins.rs`（CJS visitor） | `export default` 文は `setTextRange` のみ（original を結ばない） |
| 10 | `class_fields/downlevel.rs` | earlier transform 由来の generated 名 property（decorated private accessor の backing field）の初期化文 / inline 式に `NO_LEADING_SOURCE_MAP`（`property_name_is_synthesized`、class-fields 自身の backing は除外） |
| 11 | `class_fields/downlevel.rs` | `stabilize_receiver` の clone に receiver の位置を戻す（`__classPrivateFieldGet(B, …).call(B)` の 2 つの `B`） |
| 12 | `factory.rs` / `transform.rs` | `TransformArena::generated_binding_identity`（identity trace 用の opaque 数値） |
| 13 | `class_fields.rs` | private-name scope に synthesized private member 名を seed（`#a_1_accessor_storage`） |
| 4 / 14 | `printer.rs` / `printer/bundle.rs` / `transform.rs` / `target_bindings.rs` / `generated_bindings.rs` | failure 後の名前表の持ち越し（`CarriedGeneratedNames`、`note_generated_identifier`、`finish_print`、finalizer の seed と node cache）；print 時の optimistic 名は base から決め直す |
| — | `standard_decorators.rs` | decorated private accessor の backing field：`setSourceMapRange(backingField, getSourceMapRange(node))` と `setSourceMapRange(backingField.name, node.name)`（ES2022+ の member の mapping） |
| 15 | `standard_decorators.rs` | descriptor forwarder（`get #a()` / `set #a(value)`）は member 全体の range（decorator を含む）。`moveRangePastModifiers` は非 descriptor 経路のみ |

## 5. 未完了・引き継ぎ

- **R9**（native decorator の default class 名、esnext/define 2 row）、**R12**（System bundle の native
  export class の文末 map、esnext/define 2 row）、**T1 第 2 原因**（bundle snapshot が parsed Identifier の
  `comment_range: EndOnly` を拒む、lowered 5 row）は owner を明記して残す（§3）。いずれも decorator の
  生成名ではなく、`es_next.rs` / `system.rs` / bundle の parse-node metadata 可搬性の面。
- **R8** dispose 2 row は typed KNOWN（session model）。
- hosted 入口（INTEGRATION.md §2）は未登録。SUPER 全件（672 / 530 / 156 / 162 / 48）と H2.5h 等の
  全件 regression は hosted で確認する（決定 2 は全 transform の transformer-time finalize に、決定 4 は
  printer の全 print に及ぶ）。
- 記録した edge（修復対象外）：同じ class 内で parsed private 名が `#a_accessor_storage` と一致する場合、
  Rust は `_1` へ進み上流は重複名を出す（parsed private 名は `identifiers` に無い）；undecorated
  `accessor #a`（class-fields 所有）と decorated `accessor #a` が入れ子になる cross-transform の
  private 予約は本候補の stack に入らない；`private-and-public-same-stem` の逆順（public accessor が先）
  では上流が decorator 側の backing field に `_1` を付けるが、decorator の名前は transform 時に確定する
  ため未対応（決定 13）。
- `computed/undecorated-computed-method-after` は TS2300（`[key()]()` が `x` に late-bind）を持つ
  複合 command として凍結（Rust も同じ diagnostics を出して一致）。
- **computed key の評価回数（event log）**：[records/after/runtime-events-computed.json](records/after/runtime-events-computed.json)。
  computed family の esnext/set 15 row の凍結 `main.js`（候補の JS と byte 一致）を node で実行し、
  `key` / `key2` の event 数を記録した：各 computed key は 1 回、`getter-setter-decorated` は get / set の
  2 式で 2 回、`multiple-keys` は `key` 1 + `key2` 1。native `accessor` 構文の 1 row は node が
  解釈できず not-executable（SUPER の runtime control と同じ扱い）。
- **native ID の宣言→参照 trace**：`TransformArena::generated_binding_identity`（決定 12、opaque）で
  direct contract の synthetic group が変換後 tree を歩き、binding ごとの occurrence と綴りを
  `identity_trace` として保存し、1 binding = 1 綴りを assert する（§2.1）。declaration / assignment /
  decorator context / accessor name が同じ binding を共有することは、この trace と全 row の byte 一致で示す。
- handoff の出発例（外側 script の `_metadata` + 別 file の `@dec static accessor [key()] = 1`）は
  `global/script-let-_metadata`（FULL_CLASS）と `computed/static-accessor-decorated` の 2 row に
  分けて観測した（1 row への合成は未実施）。
- 隣接 suite の inherited red：`contracts::active_transform_contract::compact_private_function_body_emits_inter_statement_comment_once`
  （開始 SHA でも同じ、§2.3）。
