# 作業報告書：A6-41 標準 decorator 残存 source 経路（2026-09-11）

依頼：[貼り付け用プロンプト](h2-8a-decorator-followup-claude-prompt.md)・
[詳細 handoff](h2-8a-decorator-followup-handoff.md)・
[開始 manifest](h2-8a-decorator-followup-start.v1.json)。
設計記録（source→Rust owner→witness 対応表、原因別の所有権判断、全計測）は
[h2-8a-decorator-followup.md](h2-8a-decorator-followup.md)。本書はその要約と結論です。

## 1. 結論

- 準備 48 件（4 仮説 × 隣接対照 × ES2015/ES2022/ESNext × set/define）は、
  修正後 production で **48/48 完全タプル一致 ×2、通常実行 exit 0**。
- 不一致が再現した仮説は 3 つ（1・3・4）。仮説 2（object literal を含む computed name の
  pending 吸収）は**修正前から一致**しており、理由を記録して修正していません。
- 仮説 4 の到達性不足（source file 直下の class）を補うため、top-level 版 18 件を新グループとして
  TypeScript 観測し、修正前の失敗を記録してから直しました。**18/18 一致 ×2**。
- 既存 126 件・530 件は期待値を変えずに完全一致 ×2、full62 比較は変更 0・欠落 0・追加 0、
  emitter suites（lib 495 / contracts 452）通過。
- 「準備 48 件（＋18 件）が通る」ことの主張であり、「標準 decorator 全 source 経路／H2.8 全体の完了」
  ではありません（§6）。

## 2. 開始点と前作 PR の状態

- worktree `/Users/hiramatsu/dev/tsc-rs-dec-followup`、branch `prep/h2-8a-decorator-followup`、
  開始 HEAD `0fda49509`（= manifest の `2953ecb8a` ＋準備 docs）。production 9 ファイル・
  入力・観測・`_tsc.js` の SHA-256 は開始時に再計算し manifest と一致。
- PR #512：着手時 draft、head `2953ecb8a`、base `main`、hosted gates 実行中。作業中に
  同 head の hosted gates（run 34565157958）が完走し **H2.5h のみで失敗**（§5）。
  PR は本作業中に動いていません（head 不変）。前作 worktree・root・dec53 は未変更。
- 依頼どおり、前作 PR のマージ先の決定や root への反映は行っていません。

## 3. Baseline（修正前）と分類

test 登録のみで 48 件を実行（`ratchets/h2-8a-decorator-followup-baseline.v1.json`、
バイナリ `fc6f3768…`、exit 101）：33 exact / 15 failed。

| 仮説 | 結果 | 原因（source → Rust owner） |
| --- | --- | --- |
| 1 `@dec ["x"]` | 5 失敗（ESNext/define は native） | `partialTransformClassElement` はリテラル computed name を `{computed:true, name:"x"}` として temp も `__propKey` も作らないが、Rust `decorator_property_name` は全 computed name を temp 経路へ |
| 2 object literal 内の pending 吸収 | 6 一致 | tsc `visitor` は decorator を含まない部分木に入らない（`shouldVisitNode`）；Rust の generic visit も内側で注入しないため両者とも外側の class computed name で消費 |
| 3 decorated computed field 内の匿名 class | 5 失敗 | `prepare_property_named_evaluation` が decorated member で早期 return（open row）。tsc は named evaluation で hoist → `visitReferencedPropertyName` が同じ generated name を再 hoist（`var _a, _a`、`_a = __propKey(_a = __propKey(k))`）、class 名は `_a` |
| 4 undecorated outer の temp scope | 5 失敗 | undecorated class の member で named evaluation が走らず、加えて source file 直下には lexical environment が無い（top-level では error になる経路） |

## 4. 原因別コミット（production 最終 `a4c089c7b`）

| commit | 内容 | focused 検証 |
| --- | --- | --- |
| `b320bb389` | 48 件を harness に登録、baseline 受領証、設計記録 | — |
| `8b2660f19` | 原因 1：`computed_literal` を plan に持たせ、context/access をリテラル（`textSourceNode` 相当の `string_literal_text_source`）で構築。要素名は name frame で通常 visit | literal ×6 ＋ identifier 対照 ×6 = 12/12 |
| `707704835` | 原因 2：decorated member でも named evaluation を実行し、`visit_referenced_property_name` が同一 binding を再 hoist（`hoist_existing_temp_variable`、temp 序数は進めない） | anonymous ×6 ＋ named 対照 ×6 = 12/12 |
| `59bc7252e` | top-level 18 件の観測・登録・baseline（1/6、8/18） | — |
| `a4c089c7b` | 原因 3：undecorated class の member で named evaluation（関数 body の environment へ hoist）＋ `transform_root` に source-file environment と `mergeLexicalEnvironment` 相当の splice | outer ×6 ＋ 対照 ×6 = 12/12、top-level 18/18 |
| `333f25f58` | 設計記録・最終受領証・候補 patch | — |

候補 patch：`scripts/export-decorator-followup-candidates.sh`（開始 `0fda49509`・終点 `a4c089c7b` 固定）
→ `docs/design/greenfield/slices/h2-8a-decorator-followup-01..05-*.candidate.patch`
＋ `…-production.cumulative.patch`。既存 exporter と `306930ab7` は未変更。

## 5. 最終計測（production `a4c089c7b`、1 つのバイナリ `081b095f…`）

| suite | 件数 | exact ×2 | exit | 受領証 |
| --- | --- | --- | --- | --- |
| 準備 48 件 | 48 | 48 | 0 | `…-final48.v1.json` |
| top-level 18 件 | 18 | 18 | 0 | `…-final-top-level.v1.json` |
| 既存 transform-order / super-paths / name-owners | 60 / 42 / 24 | 全件 | 0 | `…-final-{group}.v1.json` |
| 既存 530 件 | 530 | 530 | 0 | `…-final530.v1.json`（full62 比 変更 0 / 欠落 0 / 追加 0、`…-final530-full62.v1.json`） |
| emitter lib / contracts | 495 / 452 pass | — | 0 | `…-final-emitter-suites.v1.json` |

各受領証は head・production/fixture/test の SHA-256・環境・実 exit・ログ SHA・実行バイナリ SHA・
全 capture SHA・実行後の source/fixture 不変を記録。run dir は
`target/decorator-followup-runs/`（prelaunch、入力 archive、log、captures、binaries、receipt）。

### acceptance と H2.5h

- ローカル `cargo xtask acceptance`（handoff の環境）：conformance 49024/49024、H1〜H2.5g は
  開始 head の hosted run と**同一計数**（H2.5g candidates 9027 / exact 8511 / h2_8a_deferred 6 /
  h2_9_deferred 510）→ 私の修正は H2.5g の disposition を変えていません。H2.5h で開始 head と同じ
  stale 行により exit 1（`…-acceptance-start-manifest.v1.json`）。
- 依頼を受けて H2.5h manifest を縮小（`eedbe221d`）：write モードで現在の差分集合を列挙
  （932 件、exact 860、known 28、deferred 44）→ 22 行が exact 化、新規差分 0、facet 変化 0。
  owner を保持した純削除（50 → 28）。確認モード exit 0。
- 帰属：開始 head `2953ecb8a` の専用 worktree で同じ列挙を行い、**バイト同一**の差分集合
  （SHA `268b5888…`）。22 行の stale はすべて開始 head 時点で既に exact（前提 63 コミット＋
  4 修正による）で、本 branch の decorator 修正に起因しません
  （`…-h2-5h-start-head-attribution.v1.json`）。
- 発見：`h2_slice_ratchet_join`（H2.5h/6a/6b）は最初の stale/NEW/facet 行で止まる実装。
  `h2_vector_ratchet_join`（H2.6c/7b）と H2.5g（前作 `f8e47ecf4`）は全件報告型。指示により
  前者を後者と同じ蓄積型に変更（`b7d388886`）：unit test 2 件、二重 stale 行の実機で 1 回の
  失敗に 2 行、既存 manifest で exit 0。`class_fields.rs` の clippy 3 件は本 branch 未変更ファイル
  の既存 lint。
- hosted acceptance（手動 dispatch、run 34578894288、head `eedbe221d`）：run 34578894288（head `eedbe221d`）は **success**。
  conformance 49024/49024、H1〜H2.5g は開始 head と同一計数、H2.5h 932/860/28/44、
  H2.6a 177/171/4/2、H2.6b 6/6/0/0、H2.6c 643/631/8/4、H2.7b 1593/1557/0/36。
  PR #512 系統で初めて hosted が緑になった run です。最終 head `b7d388886`（join 修正を含む）の
  run 34582573688 は **success**（09:07–09:50 UTC、全 band の計数が 1 回目と同一）。最終 head の hosted acceptance も緑です。

## 6. 未判定・未主張の行

- 同じ callee の未 witness 分岐（実装は共有するが数えていない）：literal computed name を持つ
  decorated method/accessor、numeric / template literal キー（tsc は template の backtick 表記を
  そのまま出す；Rust printer の text-source は string source のみ）、hoisted `var` の
  `CUSTOM_PROLOGUE` フラグ（tsc は付与、Rust は関数 body でも従来から未付与）。
- H2.5h の残り 28 行、H2.5g の deferred（h2_8a 6 / h2_9 510）は本作業の対象外。
- PR #512 のマージ先・前提 63 コミットの扱いは未決のまま（本 branch は同 PR の head 上）。

## 7. 環境と運用

- 重い実行は 1 つずつ、専用 target `target/decorator-followup-acceptance`、
  `taskpolicy -b nice -n 15`、`CARGO_BUILD_JOBS=2`。walk・chain-walk・`cargo xtask ci` は未実行。
- 前作 worktree（Codex 検証中）・root・dec53 の production と実行中入力は未変更。
- 一時 worktree（開始 head 帰属用）は計測後に削除。
