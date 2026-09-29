# TypeScript 7.1のテスト構成に合わせたconformance

状態：**design**、2026-09-29のユーザー決定を記録。runnerの実装とprofileの移行はまだ行っていない。
親計画：[TS7の方向](../../typescript-7-direction.md)、[7.1追従設計](../../typescript-7-upstream-sync.md)、
[移行基盤batch](../post-emitter-foundation-batch/README.md)のA2（test inventory）とA4（MAP/PIN）。
調査記録：[SURVEY.md](SURVEY.md)（上流harnessと現行runnerの比較、数値の根拠）。

## ユーザー決定（2026-09-29）

| # | 決定 | ユーザーの言葉 |
| --- | --- | --- |
| 1 | conformanceをTypeScript 7.1で実行されているテストの構成に合わせる | 「TypeScript7.1で実行されているテストに合わせたい。TypeScript6までとテストの構成が微妙に変わっており、今後はそちらに合わせていきたい」 |
| 2 | 期待結果は7.1のbaseline。ただし6.0で非推奨になったtarget/optionはエラーにせず、6.0.3の結果を維持する | 「7.1 のベースラインで非推奨のターゲットはエラーにせず6.0.3の結果を維持」 |
| 3 | 実装済みのES5・outFile・AMD/UMD/System対応は削除しない | 「この処理はがんばって実装したので削除するのはもったいないです」（その前の「維持が難しい場合はエラーにしても良い」への補足） |
| 4 | 非推奨optionを使う場合のTS5101/TS5107は6.0.3どおり出す。`ignoreDeprecations: "6.0"`で抑止する | 選択肢「6.0.3 どおり出す」を選択 |
| 5 | 非推奨機能以外のtsc-rs自身の挙動を7.1へ移行する（unionの安定順序、ES2026、既定target、7.xのJS/JSDoc変更など） | 選択肢「7.1 に移行する」を選択 |

これにより、[TS7の方向](../../typescript-7-direction.md#reference-selection)が「明示的なprofile移行まで6.0.3」
としていたaccepted profileは、7.1（非推奨機能のみ6.0.3）へ移る方針になった。
移行そのものは下記P1〜P3の結果と移行表を揃えて行い、本書の作成だけではprofileを変えない。
READMEの「tsc 6.0.3と診断が一致」という記述も、移行時に「7.1と一致、非推奨機能は6.0.3と一致」へ改める。

## 固定参照

- `microsoft/TypeScript` main `19dadef8888ba5b27d8b9f622480745cf623e020`（2026-09-29、`7.1.0-dev`）。
  7.1のtagはまだなく（最新tagは`v7.0.2`、`release-7.0` branchあり）、7.1が出た時点で移る。
- 既存の調査pin `1f70213d4922b434345f639b441681e470c7cfc1`（`scripts/typescript7.py`）は別記録として残す。
  runnerのvendor取込みでは、取込みに使った完全SHAとtree idを固定する。
- 6.0.3側の比較元は現行の`ts-tests/`（TS commit `050880ce59e30b356b686bd3144efe24f875ebc8`）と
  vendorの`typescript.js`。

## 調査結果の要点

詳細と出典の行番号は[SURVEY.md](SURVEY.md)にある。数値は一回限りのemulationによる
調査値で、P1のRust runnerで測り直したものを正とする。

- **配置**：`tsc/testdata/tests/cases/{compiler 6,839, conformance 5,911, transpile 25}`と`tests/lib`の
  React宣言4件。期待値は`tsc/testdata/baselines/reference/{compiler,conformance}/`に平坦に置かれ、
  `.errors.txt`は7,320件。submoduleの区分と`.diff`はなくなり、`submoduleAccepted.txt`等は履歴台帳。
- **6.0.3からの差**（blob id比較）：同一12,076、内容変更252、削除139、追加447。
  変更・削除のほぼすべてが6.0の非推奨optionに関係する（AMD→commonjs、outFile→outDirへの書換え等）。
- **harness**：directiveはfile全体から読み、unit本文から除き、CRLFをLFにし、先頭の空行を落とす。
  cwdは`/.src`。72のoptionが`*`/除外指定を含む組合せ展開の対象。既定値は`newLine: crlf`、
  `skipDefaultLibCheck`、`noErrorTruncation`。既定targetはES2026。
  ES5・UMD/System・classic/node10・baseUrl・`esModuleInterop=false`・`allowSyntheticDefaultImports=false`・
  `alwaysStrict=false`の設定はskip、AMDとoutFileは致命的エラー。
- **7.1の非推奨option**：TS5102/TS5108（removed）を常に出し、その後は無視する。
  `ignoreDeprecations`は受け付けるが読まない。6.0.3はTS5101/TS5107（deprecated）を出す。
- **`.errors.txt`**：CRLF、末尾改行なし。要約行の列はUTF-16、squiggleの幅はcode point数。
  診断の順序はGoの`CompareDiagnostics`（message keyと引数）で、6.0.3の表示文字列順と異なる。
- **現行runnerとの差**：
  - conformance 5,908 fixtureだけが対象で、directiveは先頭部分だけを読む。
  - 行末と途中のdirective行を残すため、行番号がずれる。
  - goldenは`noLib`とlib rootを使っているため、`/// <reference lib>`が効かない。
  - suggestionは含まれ、options/global/config診断は含まれない。
  - 5,857 fixtureがcheckerの旧in-memory module解決を通る（残る51件だけがProgram経路）。
- **7.1へ寄せるtsc-rs側の不足**：
  - unionの安定順序。6.0の`stableTypeOrdering: true`に当たり、tsc-rsはoffの経路だけを持つ。
  - ES2026のtargetとlib。
  - 8件のlib差分。
  - `tsc/CHANGES.md`のJS/JSDoc変更と文言・位置の変更。

## 設計

### lane A / lane B

設定展開後の各configurationを、有効option（tsconfigの`compilerOptions`にdirectiveを重ねたもの）で振り分ける。

| lane | 条件 | 期待結果 | 入力の識別 |
| --- | --- | --- | --- |
| A | 下記に当たらない | 7.1の`.errors.txt`。baselineがなく、実行されたconfigurationなら診断0件 | 7.1のblob |
| B | target es3/es5、module none/amd/umd/system、moduleResolution node/node10/classic、`outFile`/`out`/`baseUrl`、`esModuleInterop`/`allowSyntheticDefaultImports`/`alwaysStrict`がfalse、`downlevelIteration`、5.5で削除されたoptionのいずれか | 6.0.3 oracle。TS5101/TS5107を含む（決定4） | 6.0.3のblob |

- 7.1で書き換えられたcase（例：`@module: amd`→`commonjs`）は別のprogramとして扱う。
  6.0.3のblobはlane Bに残り、7.1のblobはlane Aの新しいcaseになる。
- 7.1で削除されたcaseはlane Bに残り、追加されたcaseはlane Aのみに入る。
- 検算に使う条件：
  - lane Bのconfigurationに7.1のbaselineがないこと（`downlevelIteration`はTS5102入りのbaselineがあるので除く）。
  - 7.1のbaselineにTS5102/TS5108/TS6046/TS5023が出たら、そのcaseを確認する。
- lane Bの期待値は、初めは現行の6.0.3 golden（schema 3）を使う。
  その後、vendorの`typescript.js`でharnessIOの`compileFiles`＋`getErrorBaseline`を再現した
  6.0.3用の成果物を新しいfamilyとして作り、lane Aと同じ粒度で比較できるようにする。
  既存goldenとその系譜は書き換えない。

### runner

- **入力**：`vendor/typescript-native/<profile>/`（[配置案](../../typescript-7-upstream-sync.md#配置の案)）に次を固定する。
  - `tests/cases/{compiler,conformance}`と`tests/lib`
  - `baselines/reference/{compiler,conformance}/*.errors.txt`
  - baselineの全file名の一覧（「実行され診断0件」と「skip」を区別するため）
  - 7.1のlib一式
  - commit・tree id・hashのmanifest
- **展開**：
  - unitは既存の`crates/harness/src/upstream_suites/compiler.rs`（Strada互換の分割）を再利用し、
    BOM/UTF-16の解読と`;`の除去を加える。
  - configurationはGoの`GetFileBasedTestConfigurations`を、7.1のvaryBy集合・enum表とともに移植する。
    baseline名を同じ規則で作る。
  - `skippedTests`と`SkipUnsupported`も移植する。skipされたconfigurationはlane Bの判定に使う。
- **実行**：両suiteをProgram経路（compilerの`ProgramSession`と`MemoryCompilerHost`、`load_compiler_no_emit`の一般化）で動かす。
  - cwdは`/.src`。root規則、tsconfig、symlink/link、`/.lib`、harnessの既定値を再現する。
  - 集める診断は、config＋options＋syntactic＋semantic＋global＋declaration（declaration emitが有効な場合）。
    suggestionは`@captureSuggestions`のときだけ集める。
  - checkerの旧in-memory module解決は使わない。
- **比較**：`.errors.txt`を構造化recordへ解析して比較する。
  - 要約行からfile・行・UTF-16列・category・code・展開済み文言を取る。
  - file blockからchain・related・squiggle幅を取る（squiggle幅はcode point数を、表示行を使ってUTF-16へ換算する）。
  - 採点は既存のT0〜T3（key集合、件数、category、start/length/先頭文言、chain/related）。
  - 最上位tierとして、Go形式の完全な描画とbyte比較を後から加える。
- **ratchet**：`ratchets/ts71/`に新しいfamilyを置き、append-onlyにする。
  6.0.3の`conformance-matches`・`oracle-inputs`とその系譜は変更しない。
- **再現用の上流実行**：`scripts/typescript7.py`で同じcommitのGo runnerを動かし、取込みの検算と原因調査に使う。
  別系統のrunnerは作らない。

## 実装順と終了条件

| 段階 | 内容 | 終了条件 |
| --- | --- | --- |
| P1 runner | 固定参照のvendor取込み、設定展開・skip・lane判定、Program経路での実行、`.errors.txt`比較、ratchet familyの作成。挙動の変更は含めない | 7.1の全compiler/conformance configurationについて、lane・期待値・実測をT0〜T3で記録した初回値。emulationとの件数照合。Goのbaseline名と展開結果の一致 |
| P2 挙動移行 | 失敗の多い順に不足を埋める：unionの安定順序、ES2026 target/lib、診断範囲、`CHANGES.md`の変更点、lib更新 | 各変更で該当laneの合格数が増え、lane Bと既存の6.0.3 witnessに退行がない。移行表に採用範囲を記録 |
| P3 置換 | lane Bを6.0.3の新成果物へ移し、旧conformance runnerを引退させる。checker unit test（`check_program*`の約280箇所）をtsc_programの解決へ移し、旧in-memory module解決を削除する | 旧runnerなしでlane A/Bがratchetで守られる。`authoritative_module_provider`がNoneの経路が消える |

P1だけで先にhosted CIへ入れるかは、実行時間を測ってから決める（共有CIの変更は統合担当の手順に従う）。

## 未決事項とリスク

- **unionの安定順序**：診断文中の型表示が広く変わる。移行はP2の最初の大きな変更になる見込み。
  CLIの出力比較（READMEの26構成）も更新が必要になる。
- **位置**：7.1の内部位置はUTF-8だが、baselineの列はUTF-16。squiggleだけcode point数で数える。
- **lib**：7.1のlibを同梱すると既定の型環境が変わる。lane Bには6.0.3のlibを使う。
- **promotion rename**：4件は6.0.3のどちらの版とも一致しないので、blobで識別する。
- **transpile / FourSlash / project**：本packetの対象外。transpileは25件で別runner、
  FourSlashはGoのtest、projectは7.1にcase fileがない。
