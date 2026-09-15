# A-INT3-CS：失敗した printer の comment container を別 source へ持ち越す

2026-09-16。統合担当：Codex。C03 / [PR #527](https://github.com/kazhiramatsu/tsc-rs/pull/527)
の follow-up。開始 source：`3aa9ba638c2210d1fb68bebf0935d3981e834dd7`。
ユーザーの「あなたの作業を選んで進めてください」と保留事項の処理指示に基づく、
元の `recover-other-source-same-positions` 1 case / 2 op の修復。
Claude の元 worktree と提出成果物は変更しない。

状態：修復と focused 検証は完了。最終 receipt は [review.v1.json](records/review.v1.json)、
hosted の head・run・時間と merge 結果は PR に記録する。
本書はこの source 修復・対照・CI 所有関係の範囲を扱う。profile/admission の変更は含まない。
③を再依頼する必要はなく、Claude は④ noCheck / transpile を並行して開始できる。

## 上流・現在の型・変更境界

固定 TypeScript 6.0.3 の `_tsc.js`：

- 116957–116959：`containerPos` / `containerEnd` / `declarationListContainerEnd` の初期値 `-1`。
- 121007–121032：leading 相の独立した位置 claim と declaration-list end の設定。
- 121034–121052：正常な trailing 相で親の値へ復元する。
- 121219–121238：現在の source の UTF-16 位置と、保持した整数値を比較する。

上流の guard は source identity を比較しない。C03 は失敗した最内 scope を保持するが、
元の Rust `CommentEmissionScope` は `CommentCursor`（source ID と byte offset）を保持し、
同じ UTF-16 位置を持つ別 source のコメントを抑止できなかった。
source ID の比較だけを外すと、非 BMP の UTF-8 byte と UTF-16 code unit が混同される。
また前回の source ID を次の transformation の arena で解決する方法も、ID の再利用で誤る。

変更は `comment_cursor.rs` と `printer.rs` の container の producer / reader に限定する。

1. 3つの container を `Option<SourceUtf16Position>` にする。`None` は `-1`。
2. claim の時に、**claim した range 自身の source** の PositionIndex で byte → UTF-16 を変換する。
3. leading/trailing/list の reader も自身の source で変換してから比較する。
4. nested scope は従来どおり不変値で渡す。失敗後 root に保持される値は arena に依存しない。
5. 実際の source 走査・slice・再開は `CommentCursor` / `CommentResume` の source ID と
   検証済み byte offset を使い続ける。異なる source の resume 合成を許可しない。

[E-COMMENT-SCOPE-H と E-COMMENTS-G](../../emitter-architecture.md)に対応する。
旧 unit の「container guard は別 source と絶対に一致しない」は上流と異なる期待だったため、
新規の固定 upstream 対照に基づき「guard は UTF-16 比較、resume は source を区別」に改めた。

## 観測と比較

`scripts/observe-printer-comment-carry.mjs` は C03 observer の SHA を確認し、同じ hook adapter を
使う。上流を各2回採取して固定した `printer-comment-carry.json`（24 case）の SHA-256：
`828484275265dbc389df0e388b514766cf7e30a26a580539fa6e50b33f2f19e5`。
最初の native 実行後、この fixture の期待値・入力・比較対象を変更していない。

| 配置 | LF/CRLF × before/after | 修復前 | 修復後 |
| --- | ---: | --- | --- |
| 同じ ASCII 位置 | 4 | 次回/再々回の leading comment が余分 | exact ×2 |
| 同じ UTF-16、異なる byte offset（😀 / ab） | 4 | leading comment と改行が余分 | exact ×2 |
| 同じ byte、異なる UTF-16（😀 / abcd） | 4 | exact ×2 | exact ×2 |
| 異なる位置 | 4 | exact ×2 | exact ×2 |
| 保持位置より短い source、その後に元 source | 4 | exact ×2 | exact ×2 |
| 宣言リストの end を保持 | 4 | 出力 exact、hook hint に既存差分 | 出力 exact、同じ hint 差分を固定 |

各 case は別 source の fresh control → 元 source で fault → 別 source → 元 source → 別 source。
出力 bytes、UTF-16 末尾位置、status、完全な hook event 列を比較する。
**12/24 → 20/24 complete exact**、operation result は **24/24 exact**。
4 case のイベント差分は、修復前に採取した **全 native event 列**を fixture に固定する。
同じ `#events` key のまま順序・位置・hint が広がる場合も、出力が変わる場合も test は失敗する。
検証器自身に両方の mutation を拒否する対照を置く。

追加の Rust allocation 対照は、各 print ごとに transformation を破棄・再生成し、
source ID が0に再利用される状態で同じ24 caseを2回比較する。上流との追加24一致には数えない。
unit は containerEnd と declarationListContainerEnd の UTF-16 比較、source をまたぐ resume 拒否、
既存の片側・synthesized・zero-width・JsxText predicate を維持する。

提出の25 hooks は **24/25 exact**（元の23からコメント1 caseを修復）、
前回追加21は **21/21 exact**。printer catalog は合計 **65/70 complete exact / 5 deferred**。
元の2つの comment op を既知差分リストから外し、生成名1 op の完全 native tuple は維持する。
過去の C03 DESIGN / capture / receipt の23/25や44/46は、その時点の記録として変更しない。

## 残る差分

| 対象 | 担当・次のスライス | 終了条件 |
| --- | --- | --- |
| failure 後の生成名：TypeScript `x_2`、Rust `x_1` | Claude C02 / 統合 A-INT2。既存の②依頼へ追記済み | printer の lazy 名前表と transformation の eager 確定を照合し、同一/別 printer・成功/失敗・新規 unique name の固定対照を閉じる |
| declaration binding name の substitute / before / after hint：Unspecified / IdentifierName | **統合担当 API1.2-HINT**。Claude C01〜C05の追加依頼にはしない | 通常/失敗/再利用で名前の hint と callback 順序・substitute の戻り値を比較し、4 case の差分固定を廃止する |
| declaration initializer CallExpression の substitute hint：Expression / Unspecified | 同じ API1.2-HINT。探索として別 fixture / before log を保存 | 上記と同じ producer 範囲で before/after notification を含めて観測。現在の24 case分母には含めない |
| 外部 fallible writer/generator、print 中の factory、NodeList/array/token hooks、identity/JSON の公開 contract | API1.1a / API1.2 | C03 integration の到達境界を維持し、それぞれ実際の公開 API の contract を作る |

CallExpression への fault point 変更を一度探索したが、別の既存 hint 差分も観測された。
探索の2回目 native 実行は exit101。最終 suite は元の Identifier 版24 caseへ戻し、
差分を隠すためのイベント削除・hint 正規化・分母縮小は行わない。

## 実行範囲と検証記録

ローカルは変更した owner の7 emitter target、対象 scope/cursor units、noEmitOnError の exact test。
全 lib / contracts、530 / 672、acceptance chain はローカルでは実行しない。
`cargo test --lib --no-run` は unit binary のビルドだけで、実行は対象名を列挙して限定する。

```sh
node scripts/observe-printer-comment-carry.mjs --check
python3 scripts/witness.py printer --list
CARGO_BUILD_JOBS=2 taskpolicy -b nice -n 15 python3 scripts/witness.py printer --all
```

hosted は既存 printer job に observer と24 rowを追加する。job/worker/時間上限は増やさない。
新 fixture / observer 単独変更は printer job のみ、共通 printer の変更は従来の関連全グループ。
CI planner の catalog70、owner選択、0 test拒否と policy source hash を更新して検証する。

raw before / after capture と実 exit は records に保存する。最初の before binary は後のビルドで
置き換わったため SHA 未採取であり、遡って捏造しない。before の production source は開始 commit
と一致し、固定入力・実ログ・完全な native capture がある。最終 binary/source/input は receipt に固定する。

ローカル最終結果：7 target 33 test、scope unit19、走査量 unit1、noEmitOnError 1 testが全て成功。
observer / fmt / CI planner17 / policy checkも成功。通常の対象 Clippyは exit0、
program145 + emitter16の既存警告、変更した3ファイルへの診断0。strict `-D warnings` は
既存警告が残るため green と扱わず、同じ全体lintを繰り返していない。
