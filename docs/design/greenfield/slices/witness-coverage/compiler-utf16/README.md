# OPS-COVER-3B：compiler UTF-16 controls の実行入口

2026-09-16。統合担当：Codex。base は main `0323f2358a1a95d6bfc4fe436462032e0f20088b`。
OPS-COVER-3A に続き、未登録だった3つの standalone target を既存 controls job に追加する。
変更範囲は runner、選択処理、その検証・policy pin・入口台帳。Rust と固定 fixture は変更しない。

## 対象と比較面

| suite | target（prefix `h2_8a_`） | fixture 行 | 完全一致の比較 | typed refusal | Rust tests |
| --- | --- | ---: | ---: | ---: | ---: |
| `utf16-identity-recovery` | `utf16_identity_recovery_controls` | 65 + 14 | 70 ×2 | 9 ×2 | 2 |
| `utf16-review-fix` | `utf16_review_fix_controls` | 25 | 25 ×2 | 0 | 1 |
| `utf16-tagged-template` | `utf16_tagged_template_controls` | 16 | 16 ×2 | 0 | 1 |
| 合計 | 3 target | **120** | **111 ×2** | **9 ×2** | **4** |

完全コマンドの比較は callback の UTF-16 値、UTF-8 bytes、BOM、materialized bytes、
write metadata、JS/declaration/maps、診断と related information、emit result、status、exit を含む。
各行は fresh Program で2回実行する。identity target の9行は H2.9 の
`ParseDiagnosticsDeferred` と書込みなしを確認する既存負対照であり、TypeScriptとの一致には数えない。
noEmit の14行は専用 run の activity が全て0であることも確認する。
incremental/composite の2つの BuildInfo 拒否チェックは120行へ加算しない。

identity observer が持つ upstream 内部エラー1行は、oracle のみの再現であり Rust の120行とは別。
tagged-template fixture の direct printer 観測と親v1/v2は、Rust が比較する16 complete commandsへ
重複加算しない。上表は要求する比較の分母であり、未実行時点の成功を意味しない。

## 既存 acceptance との境界

[v3台帳](../inventory.v3.json)で3 targetとも直接入口なし、acceptanceの共有source参照なし。
専用fixtureを消費するRust sourceと、`#[path]` を使うacceptance driverの参照を照合した。
これらは synthetic ID の専用観測であり、既存H2 corpusのexact総数に111を足さない。
emitter direct の literal値／printer API、4 original UTF-16 rows、literal witnesses64行、
recovery corpus50行、retained530行は別の比較集合。今回追加するのは上表の3 targetだけ。

## 選択と実行

```sh
python3 scripts/witness.py utf16-identity-recovery --list
python3 scripts/witness.py utf16-review-fix --all --dry-run
python3 scripts/witness.py utf16-tagged-template --all
```

- 各 suite は target 全体を filter なしで呼ぶ。最大79行のtargetをfocused単位とし、`--case` は拒否する。
  `--list` / `--dry-run` はNodeもCargoも起動しない。
- 専用target、fixture、observer、tagged-templateの親fixtureはそのsuiteだけを選ぶ。
  複数変更は所有者の和集合。identity observerが読む共有の
  `utf16-literals-adjacent-probes-inputs.json` は従来通り全groupを選ぶ。
- 選択された4本以下のobserverを `--check` で照合し、選択されたRust targetを1回のCargoで実行する。
  observer失敗、非0 exit、必要test数の変化、ignored/filtered/0 test、fixtureの空・重複・件数変更は失敗。
- hosted は既存controls jobのcompiler buildを共有する。job追加なし、2 workers、60分上限を維持。
  45分を分割検討の目安にする。実測はbuild、oracle、replayを分けて保存する。

## 検証状況

ローカルbaselineと新しいrunner経由の検証を進行中。hosted実行・main統合は未実施。
この入口追加によって、Rustの製品admissionやprofile qualificationを変更しない。
