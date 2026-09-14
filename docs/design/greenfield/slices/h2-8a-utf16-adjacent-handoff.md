# UTF-16 adjacent repairs: Claude への実装引き継ぎ

状態: **継続作業完了（2026-09-14）。この文書は 2026-09-13 時点の引き継ぎ記録として保存。**
§6 の全手順は同じ dirty worktree 上で完了し、最終結果は
[設計書 §32](h2-8a-utf16-adjacent-repair.md) と
[実装レビュー依頼](h2-8a-utf16-adjacent-implementation-review.md)（状態: レビュー可能）にあります。
要点: 40 件の adapter エラー解消と全ターゲット型チェック exit 0、関連 native test、
1,253 行の consumer 分類表と 6 件の suspect 処置、B admission の corpus 差分（新規 50 行、退行 0）と
その完全コマンド対照 50/50、codegen / schema check、最終ソースでの全再検証（11 runner 全緑、G5c は既知）。
main 由来の既知失敗 5 件は merge-base で同一に再現し、期待値は変更していません。
追記（2026-09-14、実装レビュー後の修正ラウンド）: レビュー回答の must-fix M-1〜M-4 と
follow-up（A-2〜A-7、B-4、B-5、C-1）を同じ未コミット差分上で修正し、指摘ごとの
修正内容・判断理由・検証結果を [設計書 §33](h2-8a-utf16-adjacent-repair.md) に、
判定の更新を [実装レビュー回答 §9](h2-8a-utf16-adjacent-implementation-review-response.md) に記録。
既存の freeze と receipt は保持し、修正後の証拠は新しいディレクトリに保存。
compiler contracts 全 module の 15 件の失敗はすべて migration 前（`f0aaa2de2`）で同一に再現する継承失敗。
以下は引き継ぎ時点の記述です。

---

## 1. 最初に読むものと作業場所

- 作業ワークツリー: `/Users/hiramatsu/dev/tsc-rs-declaration-comment-design`
- ブランチ: `work/h2-8a-declaration-comment-ranges`
- HEAD: `b652451f0ec4e6aba47aba3f4fd345c1b9168cdc`
- ユーザーの通常の cwd `/Users/hiramatsu/dev/tsc-rs` とは別のワークツリーです。
- [設計・合意・実装経過](h2-8a-utf16-adjacent-repair.md): §10、§10.5 が設計合意、§11–§31 が経過。末尾 §31.8 が引き継ぎ状態です。
- [最終実装レビュー依頼の草稿](h2-8a-utf16-adjacent-implementation-review.md): A/B/C の判断基準、対照、証拠チェックリスト。実装完了後に更新してレビューに使います。

`b652451f0` は canonical WTF-8 / EscapedName の基礎と契約テストのコミットです。
その後の実装・既存テストの移行は大きな未コミット差分に存在します。
HEAD だけを checkout しても現在の実装は再現しません。
引き継ぎ前の tracked diff は 369 files、20,185 insertions / 16,337 deletions でした。
この数には文書も含まれ、未追跡ファイルは別です。最終的な一覧は保存した git status を参照してください。

## 2. 保存した差分と最新の失敗

引き継ぎ保存先を以降 `H` と呼びます。

```text
/Users/hiramatsu/dev/tsc-rs-declaration-comment-design/target/declaration-comment-ranges-runs/utf16-claude-handoff-20260913
```

| ファイル | 内容 |
| --- | --- |
| [compiler-errors.txt](../../../../target/declaration-comment-ranges-runs/utf16-claude-handoff-20260913/compiler-errors.txt) | 最後の 40 エラーの全文。target 名を付与 |
| [compiler-error-summary.json](../../../../target/declaration-comment-ranges-runs/utf16-claude-handoff-20260913/compiler-error-summary.json) | Rust code、target、macro expansion を含む workspace 側の位置 |
| `compiler-errors.json` | cargo の compiler-message オブジェクトをそのまま保存 |
| `latest-check/` | 最後の receipt、stdout、stderr、runner のコピー |
| `tracked-head.patch` | `git diff --binary HEAD`。削除も含む tracked 差分 |
| `untracked-files.tar.gz` / `untracked-files.txt` | 未追跡ファイルの内容と一覧 |
| `source-snapshot.tar.gz` / `source-manifest.json` | 現在の crates の src/tests/examples、Cargo 入力、scripts、変更ファイルの内容・SHA。削除は manifest と patch に記録 |
| `git-status.txt` / `git-status-porcelain-v1.z` / `git-head.txt` | 未コミット状態と基点 |
| `evidence-index.json` / `receipts/` | 下記の中間検証 receipt の所在と SHA、receipt のコピー |
| `immutable-input-pins.json` | 元の 23 の input/expected と C v1/v2、C16、A/B65、noEmit14、JSX の SHA 照合 |
| `runners/` | ローカル実行用 runner の保存コピー。実行時は元の配置を使用する |
| `reference/` | 647 行の設計レビューと、見つかった移行前 consumer inventory のコピー |
| `diff-check.txt` / `freeze-receipt.json` | 差分の空白検査、保存時の整合性と範囲 |
| `artifact-sha256.json` | 保存物のハッシュ一覧。この一覧自身と外側の転送用 archive は対象外 |

最新の型チェックは `utf16-workspace-all-targets-check-20260913-233912/receipt.json`。
`cargo check --offline --workspace --all-targets --keep-going --message-format=json` が
**exit 101、inputs_unchanged=true** で終了しました。引き継ぎ時にも記録済み入力の SHA 一致を確認しました。
この runner は src/tests を記録しますが examples を記録しません。
引き継ぎの source manifest には examples も加えています。ただし後から保存した manifest を
実行時の examples 不変性の証明とは扱わないでください。

残エラーは既存テストの入力・観測側にあります。代表例は次のとおりです。

- `compiler/tests/h2_7d_declaration_bundles.rs`: JsString と String / Path の境界、JSON 観測、map helper の新しい引数。
- `compiler/tests/h2_7d_module_identities.rs`: `Vec<JsString>` の入力、JS path と native Path の既存比較。
- `compiler/tests/integration/source_map_recording_witness_contract.rs` ほか: selector の引数型と source-map の JS path。
- `harness/tests/integration/module_suffixes_oracle_contract.rs`: `scalar_json` の未導入 4 件。
- `program/tests/h2_7d_bundle_source_facts.rs`: `InvalidInput` pattern の新しい `js_path` field。
- `program/tests/integration/config_diagnostics_oracle_contract.rs`: serde JSON の index からの move。

同じ共通テストが複数 target に組み込まれるため、40 は一意の修正箇所数ではありません。
macro 内に見える Serialize エラーも summary の expansion から呼び出し元を辿れます。

## 3. ユーザーが確認した三つの不具合

| 対象 | 現在の実装と証拠 | 完了と断言できない点 |
| --- | --- | --- |
| 異なる孤立 surrogate が同じ symbol 名へ潰れる（2300 / 1117） | JS 値の所有型と EscapedName を導入し、元の 23 の該当コマンドは中間ソースで完全一致 | 全 consumer の意味分類、直近の JSX 修正の native 実行、最終再検証が未完了 |
| parse diagnostic のある source の一律 emit 拒否（H2.9） | parser provenance により literal-only recovery を許可。元の 23、追加 A/B65、noEmit14 の中間証拠あり | admission 前後の corpus 差分監査と最終再検証が未完了。構造的回復は拒否を維持 |
| ES2015 target の tagged invalid cooked | ES2018 flag/lowering、共有 host、tail var、revisit と採番を実装。C16 と元の 23 が中間ソースで完全一致 | 最終ソースでの回帰・レビューが未完了 |

最初の設計レビュー時点では「Cargo / CI 未実行」でしたが、その後の実装中には下記の Cargo 検証を実行しています。
full CI / hosted acceptance は実行していません。

## 4. 実装の入口と固定した設計判断

**A: 名前と値の所有権。** `diagnostics/src/js_string.rs` が `JsString` / `JsStr`、
`types/src/escaped_name.rs` が branded `EscapedName`、`binder/src/symbols/table.rs` が
private IndexMap と canonical な公開 query を所有します。既定の Ord はバイト順、JS 順は
明示的 `cmp_utf16()`。`Borrow<[u8]>` の契約と混同せず、非 canonical bytes を公開 lookup に渡させません。
tuple HashMap key は borrowed lookup の一般化対象ではありません。

scanner token value、parser literal text、binder/checker の producer・lookup・cache・診断を移行しています。
literal type の値には TemplateText を維持します。名前の保存に source spelling や replacement text を代用しません。
module/config/path/output の同一性にも移行が波及し、host/program/compiler と観測 adapter の差分が増えました。
この波及を含む既存テスト対応が現在の主なコンパイル残作業です。

**B: parser の回復事実。** `syntax/src/recovery.rs`、`parser.rs`、`scanner.rs`、
`incremental.rs` と `emitter/src/builtins.rs::preflight_source` を読みます。
診断保持の漏斗に origin を明示し、scanner drain だけが lexical origin を作ります。
structural events がゼロ、かつ lexical diagnostics が string/template token 内の場合だけ admission します。
抑止された診断、silent missing、speculation rollback、reparse/incremental、JSDoc の別リスト境界も要確認です。
invalid escape decoder 分岐の scanner 単体テストを含めて検証対象にしてください。

noEmit の完全コマンド対照のため `compiler/src/lib.rs::ProgramSession::emit_command_for_harness` と
`emitter/src/outcome.rs::EmitOutcome::no_emit_without_build_info` に分岐があります。
実際の typed NoEmit 実行から、map/list オプションに応じた空の EmitOutcome を作ります。
incremental/composite は BuildInfo の型付き拒否を維持し、resolver/output plan/writer/sink は動かしません。
TS の noEmit は、この経路では emitSkipped=false、診断時の command exit=2 です。直感で 1 に変えないでください。
追加 14 コマンドが比較根拠です。

**C: tagged template。** `emitter/src/builtins/{tagged_template,es2018,es2015,target_bindings}.rs`、
syntax の parser-owned template flags、generated schema、transform-flag row が入口です。
template flag は scanner の mask を運び、cooked-invalid と lowering 判定は同じ field の異なる mask を見ます。
既存の非 hoist numbered allocator を使用し、tail record と host を追加しました。
valid tag の bounded revisit は tail record と hoisted temp の副作用を含めます。
SourceFile / ModuleBlock の finalizer 先読みは共通 collector 側です。
旧 receipt v1 を変更せず、二重 visit の正の witness は v2 と C16 にあります。

**直近の意味修正。** `checker/src/jsx.rs::get_intrinsic_attributes_type_from_string_literal_type` の
`to_utf8()` による非 scalar 名の lookup 脱落を、JsString → escaped name → canonical lookup に修正しました。
`observe-utf16-jsx-intrinsic-identity.mjs` の TS 対照は 2 回と `--check` で一致しています。
追加 native test は `checker/tests/unit/jsx/tests.rs::utf16_intrinsic_literal_tag_names_retain_distinct_attribute_types`。
**まだ native 実行成功を確認していません。**

**既存 scalar 観測。** `host/tests/support/{scalar_path,scalar_query_bridge}.rs` と
`program/tests/support/scalar_json.rs` はテスト専用です。
元の scalar fixture の Path 比較、JSON field/null/array order/number/diagnostic 内容を保つための明示 adapter です。
非 scalar は拒否・panic し、lossy replacement にしません。任意 UTF-16 の完全比較の証拠には使いません。
mock host bridge は既存の native mock method を呼び、fault injection と probe order を保持します。
機械的な移行後のテストは未実行部分が多く、コンパイルだけで挙動保持を主張しないでください。

## 5. 中間ソースで成功した検証

以下の run は `target/declaration-comment-ranges-runs/` 配下です。
各 receipt の入力 SHA に対する結果であり、最新ソースの成功を意味しません。

| 範囲 | 結果 | run |
| --- | --- | --- |
| 元の adjacent 23 | complete exact ×2、46 supplemental captures も一致 | `utf16-adjacent-complete-20260913-215727` |
| 既存 UTF-16 64 | complete exact ×2 | `utf16-prior64-complete-20260913-215852` |
| C16 | complete exact ×2 | `utf16-tagged-controls-complete-20260913-220326` |
| declaration-comment 41 | complete exact ×2 | `utf16-prior41-complete-20260913-221129` |
| 元の H2.5h 4、G4a/G4b | required 比較成功。G5c は下記の既知失敗 | `utf16-original-routes-regression-20260913-221422` |
| A/B/C1 65 | 56 complete exact ×2、9 typed recovery refusals ×2、部分書き込みなし | `utf16-identity-recovery-complete-20260913-225202` |
| noEmit14 | complete exact ×2 | `utf16-noemit-command-complete-20260913-225604` |
| checker library | 1736 tests ×2 | `utf16-checker-library-20260913-224923` |
| emitter library / literal integration | 505 + 2 test functions ×2 | `utf16-emitter-literal-suite-20260913-224049` |
| program library | 46 tests ×2 | `utf16-program-lib-20260913-220517` |
| raw UTF-16 source 境界 | 6 cases ×2、下記の既存制限を確認 | `utf16-raw-source-boundary-20260913-220711` |
| 通常 workspace production 型チェック | exit 0 | `utf16-workspace-check-20260913-230918` |

G5c は strict exit 101 のままです。declaration return inference による write mismatch が修復前と同一で、
本修復の対象外として記録しています。全 original が緑とは記載しません。
raw UTF-16 LE/BE の孤立 surrogate は既存 decoder/loader が拒否し、pair は一致します。
合意済みの source decoding 制限であり、escaped literal の値保存とは別です。

## 6. 再開手順と未完了ゲート

1. 作業場所・HEAD・保存 manifest を照合し、40 エラーを現在のソースで修正する。
   `/tmp/utf16-legacy-*.py` などの一括移行スクリプトは適用済み・非冪等なので再実行しない。
   Serialize を production の lossy conversion で通す、既存の期待値を変更する等の回避はしない。
2. 全ターゲット型チェックを通した後、関連する native テストを実行する。
   checker JSX の新テスト、fuzz の `scalar_wire_boundary_distinguishes_replacement_text_from_unpaired_units`、
   conformance の `golden_message_boundary_preserves_scalars_and_refuses_unpaired_units`、
   harness の `virtual_normalization_keeps_scalar_corpus_semantics_and_utf16_identity` は追加後未実行。
   既存 host/program/config/module/source-map/output tests も変更範囲に応じて検証する。
3. `escaped_name` / `data.text` / `as_str` / `to_utf8` / lossy conversion の消費箇所を意味で分類する。
   移行前の約 245 箇所の inventory は検索結果で、分類完了表ではない。
   identity、文法上 scalar が保証される token、display、native I/O、scalar-only observer の根拠を関数単位で記録する。
   JSX の追加修正はこの監査から見つかったため、残りを未監査のまま閉じない。
4. B admission 前後の corpus 差分を取得し、新たに通る行を complete-command 比較へ結びつける。
   既存 `xtask::recovery_census` は overload/F2 の tree census で、この要件を自動的に満たすものではない。
5. nodes codegen / schema check、変更範囲の format と diff check を行う。
   `xtask/src/{node_codegen,codegen_common}.rs` は generator の抽出先で未追跡。
   `node_codegen::extract_balanced_after` の公開範囲と既存 unit test 呼び出しも変更されている。
6. ソースを固定し、23/64/C16/A-B65/noEmit14/41/original routes と必要な library 検証を再実行する。
   最終 source / binary / receipt の対応を記録し、G5c と raw source 制限は別記する。
7. レビュー依頼草稿を最終結果に更新する。consumer 表、B corpus 差分、追加対照、差分と証拠 manifest を添付して
   実装レビュー可能な状態にする。現在は原因別の最終コミット整理も未完了。

native job は直列、Cargo jobs=2、test threads=1、offline、専用 target dir を維持します。
CI や hosted acceptance の起動はこの引き継ぎの作業に含めません。

最初の再チェック（修正後に実行するコマンド。引き継ぎ作成中には再実行していません）:

```sh
cd /Users/hiramatsu/dev/tsc-rs-declaration-comment-design
python3 target/declaration-comment-ranges-runs/run-utf16-workspace-all-targets-check.py
```

通常の個別コマンドの環境例:

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/declaration-comment-ranges taskpolicy -b nice -n 15 cargo check --offline --workspace --all-targets --keep-going
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/declaration-comment-ranges taskpolicy -b nice -n 15 cargo test --offline -p tsc-rs-checker --lib utf16_intrinsic_literal_tag_names_retain_distinct_attribute_types -- --test-threads=1
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/declaration-comment-ranges taskpolicy -b nice -n 15 cargo run --offline -p tsc-rs-xtask -- codegen nodes-check
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=target/declaration-comment-ranges taskpolicy -b nice -n 15 cargo run --offline -p tsc-rs-xtask -- schema-audit
```

完全コマンドの再検証には元の配置の `run-utf16-{adjacent-complete,prior64-complete,tagged-controls-complete,identity-recovery-complete,noemit-command-complete,prior41-complete,original-routes-regression}.py` を使えます。
`checker-library`、`emitter-literal-suite`、`program-lib`、`raw-source-boundary` も保存されています。
各 runner の入力記録範囲と SHA 前提を読んでから実行してください。
古い `original-complete` / before 測定 runner は旧 SHA 固定を含み、そのまま最終検証には使えません。
`H/runners/` は保存用コピーなので、そこから起動すると相対 root がずれます。

## 7. 引き継ぎ時の境界

引き継ぎ作成では production/test/fixture の追加修正、コミット、reset、CI、外部への送信を行っていません。
最後の型チェック終了後は文書と保存物のみを更新しました。
原本の 23 input/expected は SHA 一致を確認済みです。
現在の dirty worktree が継続先なので、そこに patch/archive を重ねて適用しないでください。
保存物からの復元が必要な場合だけ、同じ HEAD の別 checkout に tracked patch と untracked archive を適用します。
`source-snapshot.tar.gz` は照合用の内容保存で、git 履歴や全 repository の複製ではありません。
以前の receipt が参照する入力・binary・capture は元の `target/declaration-comment-ranges-runs/` に残っています。
転送用 archive はそれらの全量を含まないため、別マシンでは該当証拠を別途渡すか再実行してください。
