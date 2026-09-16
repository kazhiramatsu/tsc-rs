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

baselineは4 tests成功。111 complete commandsと9 typed refusalsを各2回確認した。
初回buildは5分12秒、test本体は98.28 + 33.33 + 16.83 = 148.44秒。
[ローカル記録](local.v1.json)に全ID、source/input/binary/logのhash、実際のcommand・exit・時間を保存した。
実装commitは `71370fc80`。新しいrunner経由も4 tests成功、
111 complete commandsと9 typed refusalsが各2回通過した。TypeScript observerは4本全て一致。
observer 77.301秒、Cargo build＋replay 145.823秒、
合計 223.324秒。3つのtest binaryはbaselineと同じSHA-256だった。
hosted実行・main統合は未実施で、controls job全体の所要時間は未計測。
これはローカルの追加分の実測であり、hostedやcold buildの所要時間へ換算しない。

plannerの31 tests、`qualification.mjs check-policy`、policy/schema境界test、
入口台帳v4の再生成一致が成功。Nodeのpolicy suite全体は40 pass / 1 fail。
失敗した `registered h2 artifact labels follow the chain-walk ORDER` は開始時のmainでも
同じ失敗を再現した。ORDERにあるH2.8a candidates/observationsとschema登録の不一致であり、
今回変更していない3 source（qualification本体/test/chain-walk）に属する既存のOPS-DEBT。
全suite greenとは記録しない。

この入口追加によって、Rustの製品admissionやprofile qualificationを変更しない。
