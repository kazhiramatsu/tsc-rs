# OPS-COVER-3C：compiler literal の専用比較を hosted に接続

2026-09-16。統合担当：Codex。base は main `eee53773f`（PR #533）。
OPS-COVER-3B に続き、UTF-16/literal の2 targetを既存 controls job に接続する。
変更は runner、選択規則、policy pin、その検証と入口台帳。Rustと固定fixtureは変更しない。

## 対象と既存 acceptance の境界

| suite | Rust target | 観測行 | 選択する Rust test |
| --- | --- | ---: | --- |
| `utf16-literal-witnesses` | `h2_5h_utf16_literal_witnesses` | string36 / template26 / bundle2＝64 | 専用1 test。共有moduleから取り込まれる9 testsはfilter |
| `utf16-original-commands` | `h2_5h_utf16_original_rows_complete` | 原本4 | target全体の1 test |

64行は専用synthetic ID。共有 `h2_7b_w4a_controls` / `h2_7c_declaration_blocking`
がacceptanceにも存在することは、この64入力の実行を意味しない。
専用testだけを `--exact` で呼び、既存共有9 testsの重複replayを追加しない。
string/templateのES5・ES2015・ESNextとbundle prologueを、固定TypeScriptからの
callback bytes、BOM/materialized bytes、write順序・metadata、診断・related information、
emit result、status、exitで各2回比較する。失敗時はtestを失敗として伝播する。

原本4行はH2.5hの既存corpus ID（unicodeExtendedEscapesInStrings10/11、
InTemplates10/11のES5）。旧 `h2_5h_utf16_literal_rows` とacceptanceが比較する
write/診断/emit/exitに対し、別の凍結fixtureでcallback metadata、related diagnostics、
command status等を追加比較する。入力と親qualificationのhashも検査する。
**68行をcorpusのexact総数へ加算しない。** emitter directの値/identity、3Bの120行、
retained530とは別の比較集合。

`adjacent-probes` 23行は以前の診断用選択として保持し、promoted64行へ混ぜない。
recovery corpus50行は別target。そのobserverはローカルcensusを要求するため、
入力の永続化・既存acceptanceとの比較面の照合を次のOPS-COVER-3へ残す。
旧literal rows4行も既存acceptance/今回のcomplete targetとの重複を区別して台帳に保持する。

## 実行・選択・失敗伝播

```sh
python3 scripts/witness.py utf16-literal-witnesses --list
python3 scripts/witness.py utf16-literal-witnesses --all --dry-run
python3 scripts/witness.py utf16-original-commands --all
```

- 両suiteは `--all` で固定集合を実行し、`--case` を拒否。list/dry-runはNode/Cargoを起動しない。
- literal observerは同一scriptを3つのgroup引数で呼ぶ。パスだけで重複除去せず、各groupを
  `--check` する。original observerも固定fixtureを照合する。fixtureを書き換えない。
- 専用target/fixture/observer/inputはそのsuiteだけを選択する。変更集合は和集合。
  shared declaration helperはlate/retained/literal、shared library helperはlate/SUPER/retained/literal。
  qualification、VFS overlay、他の共有・不明入力は従来通り全groupを選ぶ。
- originalは既存unfiltered compiler batchに加える。literalのみ別のexact-test Cargo呼出しにし、
  同じcompiler buildを共有する。全targetをひとつのtest filterで誤って省略しない。
- inherited `TSC_RS_UTF16_LITERAL_WITNESS_SET` / `FILTER` を消してから起動する。
  Rust testが1件成功しても内部入力が絞られる事故を防ぐ。
- observer失敗でCargoを開始しない。非0 exit、0件、missing/ignored tests、想定外のfilter数、
  fixtureの空・重複・件数変更を拒否。literalは1 pass / 9 filteredを要求する。
- controlsの2 workers・60分上限を維持し、45分を分割検討の目安にする。
  build、oracle、replayとjob全体の時間を別記する。

## 検証

入力membershipの検証、選択・observer引数・環境変数・filter/exitの負対照を含む
planner34 tests、policy checkとpolicy/schema境界tests、台帳v5の再生成一致が成功。
v5は64 standalone中21 unfiltered / 7 filtered / 36直接入口なし。
残りはcompiler16と既存filtered targetの未選択部分、その他20とlib/bin16。

[ローカル記録](local.v1.json)に入力・実行source・binaryのhash、実際のcommand/exit、全IDとログを保存した。

- baseline：専用64行と原本4行が各2回一致。1 pass / 9 filtered と1 pass / 0 filtered。
  初回buildを含む実行は331.022秒＋6.243秒、test本体は57.98秒＋4.29秒。
- 新しいrunner：同じ68行が各2回一致、2 tests成功。4つのobserver呼出しも一致。
  `adjacent-probes` と不一致filterを環境に置いた状態でもpromoted64行全てを実行した。
  合計107.165秒。observerとCargoの内訳は下記recordのsummaryに保存。
- 2つのtest binaryはbaselineと新runnerでSHA-256が同一。Rust/fixture/observer/親qualificationは
  baseとbyte単位で同一。実行後にrunnerのsourceを変更していない。
- hostedの結果とcontrols全体の時間はPRで確認する。
製品のruntime admission、H2全体のqualification、既存lint/strict test債務の解消はこの入口追加のclaimに含めない。

## Hosted と統合

[PR #534](https://github.com/kazhiramatsu/tsc-rs/pull/534) は head `99a1739c3d0f231903309f6053ac6a701dca670b` で
acceptance3 job・witness4 job・両gateが成功し、merge commit で統合した。
[受領記録](hosted.v1.json)と[compiler実行ログ](hosted-compiler-literals.log)を保存。
controls全体452秒、compiler5 target / 6 tests、observer49.375秒、Cargo74.546秒。
追加literal64行は各2回一致（1 pass / 9 filtered）、original4行も各2回一致した。
wide acceptanceは1659秒、7 replay jobsの合計は4906秒。
