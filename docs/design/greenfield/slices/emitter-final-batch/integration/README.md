# Emitter final r11 integration

## 最終スキーマ検査の限定修復（r543–r558）

[正式 walk の失敗と修復・検証の全記録](records/schema-contract-repair-r559/manifest.json)。
4 回目の正式 walk（r543）は、2 巡目の全 75 段、16 owner-control scripts、61 tests を
通過した後、最後のスキーマ検査で終了 1 となった。収束成功の証明は発行しない。
H2.5g の新しいキーで 9,027 ケースを各 2 回比較した結果は成功し、2 巡目は再採取 0。
その実記録は保存するが、walk 全体の成功とは区別する。

登録済み 27 スキーマを個別検査し、H2.7c の循環参照と、H2.5h-a の旧 45 行を要求する
定数の 2 件を特定した。Opus 221–222 と照合し、前者は現在の全診断を覆う 1 層の
関連情報を厳密に検査する形とし、後者は既に承認された 58 行・集計値に同期した。
より深い関連情報は今後の明示的な検証まで拒否する。汎用検査器の循環参照拒否、
TypeScript の採取処理、Rust 本体、設計項目の内容と実行許可は変更しない。

正式 writer で両 artifact を更新し、診断・出力・件数の不変を確認した。
新しい異常系を含む登録済みテストは 66/66 成功。前回失敗した検査コマンド全体も
終了 0 となり、全 27 contracts と FCI の 45 envelopes・42 ready を通過した。
正式 walk の新規実行、最終候補の unsplit CI、hosted checks、merge は引き続き必要。
以下は各時点の経過記録である。

## H2.5g の採取場所に依存する再利用判定の修復（r526–r540）

[失敗・停止の記録と限定修復の全証拠](records/g5-library-reuse-r541/manifest.json)。
3 回目の正式 walk（r526）は 9,027 ケースを各 2 回採取した後、4 ケースで古い
作業場所の標準ライブラリ診断パスとの不一致を検出した。正式 writer が全件を
再利用して同じ古い記録を再生成するため収束できず、原因を確認して実行を停止した。
終了値は -15（起動側 241）であり、成功証明を発行しない。ログと旧キーの全 journal を保存した。

検証器の再利用判定を限定修正し、現在の入力・symlink・config パスを先に検証した上で、
別の採取場所を指す標準ライブラリ診断を再利用から外す。TypeScript の host、診断の
生の内容、Rust 本体、比較基準は変更しない。Opus 218–220 と照合し、影響が広い host
仮想化案を撤回してこの方法を採用した。登録済み検証器テストは 61/61 成功した。

正式 writer（r539）は 4 ケースを新規採取し、9,023 ケースを再利用して終了 0。
全件比較により、4 ケース内の診断ファイルパス 17 箇所と対応する hash、および生成器・
artifact の hash だけが変わり、生成コード、他の診断内容、件数、実行条件は不変と確認した。
旧 journal との一致は差分検査の証拠であり、新しい生成器キーでの検証の代用にはしない。
今後、正式 walk で新キーの 9,027 ケース各 2 回比較と全段の収束検証を実施する。

これは [gate-tax 3](../../../gate-tax-3.md) の採取場所に依存する証明範囲を維持する修復で、
artifact の移植性を保証しない。対象は標準ライブラリの診断パスであり、その他の出力パスは
従来どおり全件採取による検査に従う。最終候補での unsplit CI、hosted checks、merge は未完了。

[最終 CI と納品記録の検査補助](records/delivery-guards-r531/manifest.json) も同じ候補に同梱した。
実際の Opus 215–217 の指摘を反映し、18 phases の完全性、通常優先度での再試行の前提、
24 profiles・18 rows・74 owner instances、commit 内の walk 証明との結合を検査する。
限定テスト 9 件は成功しているが、実際の最終 CI や納品の成功を表すものではない。

## W5 の現在値検査と過去の固定記録の分離（r519–r523）

[正式実行の失敗と修復・検証の記録](records/w5-stratum-validation-r524/manifest.json)。
2 回目の正式 walk（r519）は H2.7a witnesses で停止した。新規実行した Rust の
172 行 census では選択済み 67 行の出力欠落が解消していたが、旧判定が「宣言出力の
欠落が残ること」を要求していた。途中の成功した生成物と exit 1 の記録を保存し、
この実行には収束成功を認めない。

検証器は過去の stratum 全体を元の M1 固定 hash で検証して保持する。選択する
67 行、元の観測、M1/S2 の hash は不変。現在値には出力欠落・差分・余剰を認めず、
期待件数・診断件数・emitSkipped・map 件数を検査する。毎回 native census を
再実行し、古い出力や環境変数で指定した census の採用を拒否する。
比較は出力パスごとの byte/BOM と件数であり、完全な command parity の証明ではない。

修正後の実 census は 172 行を生成し、対象 67 行が通過、過去の M1/S2 記録も一致した。
異常系を含む登録済み Node 55 tests と、宣言 resolver/transformer 関連 3 tests が成功した。
後者の transformer は 116 cases・199 declaration writes で差分 0。Opus 212–214 の
実際のレビューと判断理由も保存した。Rust 本体は変更していない。
正式 walk の再実行、最終候補の unsplit CI、hosted checks、merge は引き続き必要。

## 正式 walk の停止と監査参照の修復（r509–r515）

[初回の正式実行と参照修復の全記録](records/source-anchor-validation-r516/manifest.json)。
r509 の dry run は成功した。r510 の正式実行も整形、workspace 全 targets の Clippy、
構成監査とコメント処理 429 ケースの各 2 回完全比較を通過したが、最初の L0 生成器が
古い Rust の参照文字列で停止した。成功した生成・収束証明はなく、失敗として保存する。

L0 の 27 参照中 4 件を、現在の呼び出し元・関数移動先へ対応させた。
H2.7a close の参照行番号 10 箇所と、owner inventory の 245 箇所も更新した。
後者は header 付き 238 件と名前付き関数 7 件で、既に有効な header 付き 20 件と
従来の範囲検査のみの 96 件は維持する。複数の同名 header は参照導入時の Git 履歴から
元の Rust 所有者を追跡した。factory の 7 件は元の `classify_created_node_flags` を保ち、
既に分離されたプリンタ本体は対応する `emit_transformed_node_worker_unscoped` を指す。
識別子・状態・header・判定ロジックは不変で、行番号だけの差分であることを検証した。

Opus 208–211 の実際のレビューと訂正も保存した。データ表の追加検査では、gap matrix
13 行の 60 参照と close の 4 surfaces・9 arms を元の検証処理で確認した。
従来の前段チェックには H2 のエラー文言の分類漏れがあったため、r512 の終了 0 は
その参照が正しかった証明にならない。新しい実行モードを作らず、既存 `--check` が出す
欠落・重複・header 不一致を分類する。元の生成器を用いた子プロセス内の改変対照を追加し、
r515 で 4 tests・6 対照、前段チェック全体、実際の workspace audit が成功した。
テスト補助の文字列・Buffer の扱いで失敗した途中の記録も残す。

この修復では Rust 本体と生成済み artifact を変更していない。L0 の次回棚卸しでは、
既に存在する設定項目 `allow_non_ts_extensions` と `no_emit_for_js_files` も反映される。
正常終了する正式 walk、最終候補での unsplit CI、hosted checks、merge は引き続き必要。


## テスト配置と census 固定の整合性修復（r488–r506）

[テスト配置と固定 census の整合性修復](records/test-layout-validation-r507/manifest.json)：
14 ソース内の 15 テストモジュールを `tests/unit` へ移した。Rust AST に基づいて
処理部分のトークン列が不変であること、テスト本文が通常の整形と 9 箇所の相対参照調整以外
不変であること、fixture の参照先と内容が同じであることを確認した。移動後の 6 クレートの
lib テストは計 2,431 件成功した。元 16,994 入力の census がファイル全体を固定する
`recovery_parse_snapshot.rs` は byte 不変で保持し、1 モジュールだけを完全 hash・path・
宣言位置・census との結合で限定する。変更・削除・別ファイル・検査対象外化を拒否する
ガードを Python と Rust で検証し、検査スクリプトと例外定義も事前チェックの再利用キーに含めた。

H2.5g 入力は、削除した diagnostics の src 内テスト 1 ファイルだけを除いて 920 とし、
移した 15 モジュールをテストとして分類した。処理ソースの登録漏れはない。テスト移動に伴う
loader と hosted source の既存 hash 各 1 件を更新し、入力復元 26 検査、統合ポリシー
50 検査、workspace 検査を実行した。更新前に拒否された記録も保持する。
Opus との実際のレビュー 203–207 と判断理由を同じ archive に保存した。
これらは範囲を限定した検証であり、最終の正式 walk・unsplit CI・hosted gates・merge の代替ではない。

実際の workspace audit は、基準 main にも存在した「統合テストは最大 2 本」という
規則と、個別 band を実行する既存構成との不整合で停止した。compiler 33・emitter 22・
program 7・syntax 8 の計 70 ファイルは全て byte 不変で維持し、正確なファイル名一覧・
各件数・登録合計を監査する。追加・欠落・改名・重複・不要になった登録・workspace 外の
登録を拒否し、他 crate の上限 2 は維持する。Cargo の自動検出、テスト内容、hosted の
実行指定は変更しない。70 は登録した 4 crate の合計であり、内容の一致や実行範囲の証明
ではない。修正後の workspace 単体 26 tests と実際の監査が成功した。正式 walk の前にも
この監査を毎回実行し、Rust 用の事前検証結果を再利用できる場合も省略しない。

## walk事前検証の修復（r456–r485）

[実行記録とactual Opus196–202のレビュー](records/walk-preflight-repair-r485/manifest.json)を保存した。
D/Eの元325入力と323件×2回のTypeScript出力は、元generator・vendor・corpusをコピーし、
固定された4親データを用いて全体一致を再現した。現在のgeneratorでも全325入力とcensusを比較し、
4親のprovenance hash以外の変更は拒否する。bundle55件と出力先23件も各2回一致した。
元3artifactの固定hashとRust側の検証は維持した。

walkには独立したmap-optionのcanonical checkと、D/Eの固定参照検証を明示登録した。
75段階の生成処理は実依存順に並べ、plannerと27件のschema登録表を同期した。
複数出力とimportされた出力パスも検査し、未登録・欠落・重複や順序逆転は拒否する。
登録表は3項目の順序だけを変更し、各label/schema/artifactの組は全て不変。
更新後の構造検証50 testsは成功した。途中の失敗と棄却した提案も記録に残した。

r478のdry runは登録・依存順序・現在の入力比較・pin・static checks・fmtを通過し、
既存cfg(test) moduleの配置違反16箇所で停止した。Clippyと実walkは未実行。
この修復ではcrates本体を変更していない。次にテストをtests/unitへ移し、内容の同一性、
関連suiteとsource参照を検証する。実walk・最終unsplit CI・hosted・mergeは引き続き未完了。

追加監査で通常emitの残差・回帰を確認した。提出REPORTの「emitter owner未解決0」を
そのまま完了判定には採用しない。[追加監査と修復状況](residual-audit.md)を参照。

2026-09-18。producer canonical `~/dev/tsc-rs-emitter-final` の未commit r11を、
main `3b1f5fe87fd31e3b303bb44bd257342735452ed9` に基づく
`work/emitter-final-integration` へ受領した。提出worktreeのsourceと元workspaceの未commit作業は変更していない。
提出時の検証と統合候補の検証は以下で区別する。hosted完了前に全体完了を主張しない。

## canonical 再検証と残る境界修復（r396–r432）

[固定候補 r396 の実行](records/canonical-focused-r398/manifest.json)では、emitter 全1,020 tests、
helper551・MetaProperty228・export名80・context788・空ブロック72件の各2回比較、
構文回復2 tests、関連xtask3 bands、workspace全targetsのClippyが成功した。
全体は20段階中5段階が失敗したため未qualified。M4の古い関数名assertion、
同一のimportコメント重複を検出したtoken/ellipsisの2段階、UTF-16の古い拒否1件、
H2.1aの古い拒否1件を区別して記録した。

[境界の追加観測](records/module-boundary-review-r419/manifest.json)に基づき、
importのmodifier末尾とkeyword先頭のコメント所有、side-effect import/star/namespace exportの
現在のmodifier保持、exportのkeyword位置、export assignmentのmodifier省略を限定修復した。
TypeScript観測は既存165件を保持して80件追加し、計245件。共有parser/checker/map処理は変更していない。
actual Opus191/192の議論で棄却した仮説も記録に残す。

UTF-16 keyword-escapeは元の完全観測を各2回比較して一致したため、拒否期待値だけを更新した。
残る8拒否境界は維持する。H2.1a decoratorOnUsingも元の出力・診断・結果tupleが各2回一致し、
追加で構文木25 nodesの種類・位置・親をTypeScriptと照合した。両方のdecoratorが変数文に属し、
class-fields経路のH2.4b活動1回が正しい。actual Opus193/194と確認し、現在の昇格記録に
その正確な回数を持たせた。旧qualificationは変更せず、他の活動は引き続き0を要求する。
[最初の再検証](records/module-boundary-validation-r433/manifest.json)はtoken407/409件が一致し、
namespace exportの2件がコメント再開位置の所有者不一致で失敗した。後続9段階は未実行。
actual Opus195と照合し、export clauseだけに既存の独立したコメント所有者の投影を用いた。
共通CommentResume検証は維持し、追加20件を含むTypeScript計265件の観測を保持した。

[修正後の固定差分による再検証](records/export-clause-validation-r442/manifest.json)は全13段階成功。
先行する36 complete commandsは各2回一致し、全72補助実行の出力・診断・結果tupleを保存した。
共通resume所有者guardも成功した。
emitter全1,020 tests、token429・ellipsis534・helper551・export名80・context788件の各2回比較、
source-map witnesses全8 tests、M4全4 tests、UTF-16全2 tests、H2.1a全4 tests、
workspace全targetsのClippyが成功した。最終qualificationは元corpus再実行、walk、unsplit CI、
hosted確認とmergeの完了後に記録する。

## 統合後の関数本体コメント検証（r349–r395）

[統合後の全emitter実行](records/canonical-focused-r382/manifest.json)はunit515件を通過したが、
contractsで449成功・3失敗となった。ドット区切りnamespaceの2件は、片側だけ実位置を持つ
statement arrayをSourceRangeとして検証した回帰だった。Systemの1件は、明示alwaysStrict:false
で残る末尾コメントを一律に消す古い期待値だった。後続のcompiler/xtask段階は実行していない。

[修復と追加対照](records/comment-range-repair-r395/manifest.json)では既存CommentSourceRangeを
関数本体のコメント端点に用い、先頭・末尾を独立に扱う。AST/mapのSourceRange検証は変更しない。
合成された空関数から元のコメントを再取得する別の不具合もAPI probeで再現した。
関数本体には空リストの括弧コメント経路を使わず、独立した先頭・末尾の経路だけを用いる。
actual Opus189/190と上流経路を照合した。通常の非関数ブロック経路は維持する。

公式TypeScript観測はprinter340件、通常command788件＋拒否境界72件。
元172/244件と元748＋72件の観測は不変。片側端点・合成配列・隣接行/空行・抑制flags、
namespace/SystemのJS/map/declaration/declarationMapを含む。StartOnlyはfactory APIの対照であり、
通常producerの存在を主張しない。修正後のnative再検証・最終gateは未完了。

## 最終統合前の追加監査（r330–r369）

提出時の68行のKNOWNは退役済みだが、追加監査で見つかった不具合の修復と最終統合は継続中。
旧候補の成功を、変更後の候補の成功へ読み替えない。

[構文回復の追加証拠](records/empty-variable-proof-r344/manifest.json)では、
元16,994入力のAST・診断・raw recovery factsと先行5 profilesの不変を確認した。
限定した空の変数宣言とUnicode bindingの回復を別worktreeで検証し、
80件のbinding controlsと、従来432件＋空宣言156件のcomplete commandsは各2回一致した。
110件のload failureは引き続き未検証として保持する。

[System境界とコメントの記録](records/body-comment-review-r366/manifest.json)では、
execute本体のstatement-array範囲とEOF token mapを修復した候補について、
684件中680件が各2回一致、追加したコメント入力4件が不一致だった。
旧8件のSystem map不一致は解消し、source-map witnesses、宣言map、関連APIなど
残り9 bandsは成功した。この候補全体のqualificationは未成立。

4件の不一致と、関数本体自身のNoNestedComments／NoTrailingCommentsを別途観測し、
Opusとhead・body・tailの所有順序を照合した。関数本体に限ってコメント抑制の範囲を直し、
成功時だけ復元し、失敗時はTypeScriptと同じプリンタ状態を保持する候補を検証している。
共有ASTの位置制約や診断位置は緩めない。追加controlsは既存のTypeScript観測を保持し、
748 complete commands、172 comment-flag rows、22 serialized failure rowsへ拡張した。
[単体・プリンタ状態の検証](records/body-comment-validation-r374/manifest.json)では、
emitter lib全515 tests、172 flag rowsと22 failure-review rowsの各2回比較が成功した。
古いSystem unitの期待値1件はTypeScriptの直接観測で訂正し、失敗後の末尾座標1件は
文字列を測るoracleと同じ定義へtest adapterを合わせた。共有writerは変更していない。
[全コマンドとmapの再検証](records/body-comment-complete-r379/manifest.json)では、
748 complete commands、空ブロックの既存72件、既存出力21行＋System結合出力2行が
各2回一致し、記録・出力のsource-map witness全8 testsも成功した。
一時的なテスト選択コードの所有権エラーで止まったr365も、実行0件の失敗として保持する。
[canonicalへの反映](records/canonical-integration-r380/manifest.json)は完了した。
既存のimportコメント修復を保持し、構文回復・System範囲・EOF map・関数本体の修復を統合した。
canonicalでの対象テスト、chain walk、unsplit CI、hosted確認、mergeは未完了。

## r211–r224 の固定候補に対する検証記録（2026-09-20）

この節はruntime source `0336c56663ff243503a18987a2cf8021109e8295` と、その検証後に
既知差分を退役させた `4dfc0fb4d3b3ca2364957c85d9ad84bd0a677dc2` の実測記録。
最終のqualificationと統合状態は [architecture validation](architecture-validation.md) と
[PR #561](https://github.com/kazhiramatsu/tsc-rs/pull/561) に記録する。

| 検証 | 実測結果と範囲 |
| --- | --- |
| controls r211 | 176＋1,080＋344＝1,600件すべて各2回完全一致、失敗0、全4 steps exit0 |
| 元commands / projects r212 | 46 emit＋2 noEmitの元48 commands、および108 projectsが各2回完全一致。全9 steps exit0。noEmitはemit成功へ数えない |
| 既存command回帰 r213 | 508＋145＋120＋JSDoc1＝774件すべて各2回完全一致。773件の完全観測archiveと、JSDoc1件を各2回比較する別helperの成功記録を保持 |
| emitter / checker r213 | emitter全23 targets・1,015 tests、checker lib全1,739 testsが成功。ignored0 |
| transpile r213 | 291件中289 exact・2 independent known。6件がexactへ変わったため、2 testsは退役要求assertionだけでexit101。新規不一致0 |
| 退役 r222–r223 | live parse KNOWN36→0、transpile KNOWN8→2。過去のnative観測を比較器の改変検知testに残し、witness入力へ明示登録。planner全84 tests成功 |
| 退役後 r224 | 元36行すべて各2回完全一致、比較器guard3 tests・transpile9 tests成功。workspace Clippyは構文回復testのunnecessary_filter_map指摘1件でexit101 |
| 元census / parser proof r185 | 元16,994 parse inputsを保持。AST・診断・raw recovery factsはr177と一致、先行5 profiles不変、context admissionは単調。製品sourceと依存hashの同一性で証拠を再利用。r224/r231後の変更は構文回復testの同値なfilter置換・新actionの記録分岐追加・template testの不要な借用撤去のみで、元crate全体のtree hash不変とは主張しない |
| corpus selection r185 | 元14,329 ID＝14,219 loaded＋110明示的load failureから48 commandsを選択。従来44件を保持。110 load failuresは成功に数えない。class-body gapの新動作は元入力0件・専用controlsで検証 |

[全127 artifactsと観測archive](records/layout-controls-r211-r213-complete/manifest.json)、
[退役差分とplanner記録](records/known-retirement-r222-r223/README.md)を保存した。
[退役後の全記録](records/retirement-validation-r215-r224/manifest.json)も保存した。
r231ではfilter修正のunit2 testsが成功し、全targetsのClippyが記録用testの新action分岐漏れを検出した。
[失敗記録と製品sourceの同一性](records/prewalk-validation-r231/manifest.json)を保持し、
記録用testには既存censusと同じ完全なaction項目を追加した。
r234では記録用testの全36・432・72・285件が成功した。全targetsのClippyはハーネスのslice複製4件とtestの借用2件を検出し、同値な標準処理へ修復した。
[全記録](records/prewalk-validation-r234/manifest.json)と[修復範囲](records/lint-repairs-r237/manifest.json)を保存した。
compiler/emitter/parserの製品sourceは変更していないが、projectハーネスの4式は変更するため、製品source全体のbyte不変とは主張しない。
r238では関連23 testsが成功し、全targetsのClippyがtest補助moduleの重複読み込みと型移行後の冗長処理を検出した。
[失敗を含む全記録](records/prewalk-validation-r238/manifest.json)を保持し、[Opus166](cross-review/round166-opus.md)と補助moduleの全rootを照合して、各test crateの入口で一度だけ読み込む形へ修復した。補助処理の実装とtest登録は不変。
[修復差分と範囲](records/lint-repairs-r241/manifest.json)を保存した。
r242–r243では修復後Rustの固定hashに対して全targetsのClippyとstatic walk preconditionsが成功した。
[実行記録とsource同一性](records/prewalk-validation-r242-r243/manifest.json)を保持する。生成artifactのfreshnessはchain walkで確認する。
この記録時点で、修復後のruntime再検証、chain walk、unsplit local CI、hosted確認、mergeは未完了。
r215はbuild成功後に通常binをtest artifactと誤認して停止した準備スクリプトの失敗であり、
同じ固定sourceで対象testの選択を修正したr224で全runtime再比較が成功した。失敗記録も保存した。

[Opus162](cross-review/round162-opus.md) と [Opus163](cross-review/round163-opus.md) は、
関数本体ではtriviaを除いた開始位置、JSXではraw開始位置を使う差を確認した。
両者とcase/default・binary・property access・conditionalの5 helperは既存の行indexで
比較する。コメントcollectorのCR/LF限定の所有境界は別契約であり、この修復では変更しない。
この最終レイアウト修復はparser/checkerの共有処理を変更せず、既存の全観測を保持してcontrolsを
拡張した。JavaScript・宣言・両map・診断・write callbackを含むcomplete commandで比較した。

先行r203の1,192件中16不一致と、それを前提にbuild前に停止したr204–r205も
[保存](records/unicode-layout-controls-r203-r205-complete/manifest.json)している。
過去の成功を後の候補の成功へ読み替えず、各記録のsource hashと実行範囲を使う。

## r120–r124 の固定候補に対する検証履歴（2026-09-19）

r120–r124の記録時点では統合は未完了だった。検証対象のRust・fixture・HEADを固定し、修復候補は別worktreeで準備した。
次の成功はそれぞれ記載した候補の証拠であり、現在の最終候補の成功へ読み替えない。

| 検証 | 確認済みの結果 |
| --- | --- |
| r120 recovery 境界 | 1,930 complete commands が各2回一致。3,860回分の完全観測archiveもr106とbyte同一 |
| r106 program / checker | program 558成功・5 ignored、checker lib 1,739成功 |
| r120 emitter | 23 binaries、1,015 tests成功・ignored0 |
| r112 post-child metadata | 既存96＋追加24の120 controlsが各2回一致 |
| r120 追加境界 | 110 commandsが各2回一致。r116で残った3差分も解消 |
| r120 元のSystem/recovery | 4,088 commandsが各2回一致、11 Rust tests成功。全8,176観測を保存 |
| r122 parse KNOWN | 残る36行すべて各2回一致。2 testsは退役要求assertionのみでexit101。全corpusの影響確認・退役は未完了 |
| r123 config/library | 現在の固定compiler binaryで24 tests成功・ignored0・filtered459。r121 planner全84 testsも成功 |
| r124 EF7 / PLAN-BASE全件 | 217 / 1,798行すべて各2回一致・新規差分0。両testsは既存KNOWNの退役要求のみでexit101。比較器／shard guard3 testsは成功 |
| r110b ledger | 固定済み旧xtask実行ファイルで4,113 entries、stale0・undispositioned0。最終toolによる再確認は未実施 |

r116の3差分は、System変数名の型コメントと、クラス／引数プロパティの宣言コメントだった。
[全tupleと結果](cross-review/r116-native-results.md)を保存した。
Opus116–119と照合した限定修復、および追加12＋24 controlsを統合し、
`b451489e4a18abbff42651d8eb814537f4c5f800` に対するr120検証は完了した。
110 complete commands、emitter全1,015 tests、境界1,930、元の4,088 commandsがすべて成功した。
[r120の全結果と観測archive](cross-review/r120-native-results.md)を保存した。
失敗時は後続を進めず、current bytesでの成功を要求する。

[r122の36行の一致と退役要求](cross-review/r122-known36-results.md)は元fixture・比較器で確認した。
live KNOWNはまだ変更していない。過去の拒否観測をbyte同一で保存し、parser/corpusの証明が
完了した後に比較ガードをarchiveへ接続して退役させる。単独36行の一致で共有変更をqualifyしない。
[r124の全件比較](cross-review/r124-full-universe-results.md)でも元の217＋1,798行の集合を
変えずに全件一致した。raw exit101と全実行IDを保存し、KNOWN退役前の成功扱いはしない。

censusは7,908 recorded plansと、過去のqualification/candidate入力にのみある6,421 IDの
計14,329 IDを対象にする。これは入力IDの棚卸しであり、実行成功件数ではない。
固定したCensus HEADの入力hashと全IDは `records/census-r78-claimed-id-inventory.json.gz` に保存した。
census完了時には各IDがloaded rowsまたは明示的なload failureへ残っていることも確認する。
この時点で進行中だったcensusとparser replayは、その後r131/r151で完了した。
選択された元commandのnative比較と最終統合は、上段の現状を参照する。
censusは固定sourceと実行ファイルで採取し、停止を含む経過時間や機能比較を併走させた時間は
性能qualificationに使用しない。
Fableはround54でlimitに達したため、以降は実際のClaude Opusを1 CLIずつ使用している。

共通処理の変更が複数出力へ及ぶ設計は、[統合後の設計調査](cross-review/post-integration-dependency-boundaries.md)
へ記録した。checkerの診断用symbol表示もprinterを呼ぶ実経路を確認している。
このtrainでは互換差分の修復と検証を完了させ、境界の再設計は別の設計判断とする。

## 受領とレビュー

[受領台帳](records/received.v1.json)はproducerの37 tracked filesと557 untracked files、
各byte hash、基点とpatch hashを保持する。`candidate-tracked.diff` のSHA-256は
`241f13aa19a15ca76ad85db8392a19d741d2f5bda3389e790bf3baaf7e4c2d53`。
mainとのproduction競合はなかった。[原因とcommitの対応](records/producer-commits.v1.json)により
16 commitsへ分割した。共通hunkを持つbinding/class/async等は、提出の最終bytesを保つ単位にまとめた。
元のpatch scriptsとCOMMIT-PLANも保存した。

305本のignored raw logsはlossless gzipとして隣接保存し、
[archive台帳](records/producer-log-archive.v1.json)に展開後のhashを記録した。
提出metaの `.log` は同名 `.log.gz` を展開して照合できる。履歴の失敗logも保持する。

レビュー対象は、checkerのsource identity・enum/async/JSDoc facts、factoryのoriginalとgenerated binding、
名前確定のscope/print順、async superとPromise constructor、class/decoratorのreceiverとassigned name、
System/moduleのhelper/import/export、printerのcommentとsource spelling、option admissionとharness floor。
実装は既存typed producer/consumerへ接続し、test IDや期待outputをproductionの分岐に用いない。
全共有経路の回帰確認は既存acceptance/witnessと追加global/classで行う。

## 統合時の追加修正

### KNOWNを新しい不具合の免除にしない

提出のKNOWNはIDと原因を保持していたが、比較では「そのIDが何らかの差分を持つ」ことしか要求しなかった。
`emitter-final-known-native.json` は受領時のr11の68行のnative outcomeを固定した。追加監査で解決した32行を退役archiveへ移し、現在のKNOWN36行にのみ残す。
受領時の27 checker行はwrite/diagnostics/source order/emit result/exit等の全観測を、41拒否行は正確な拒否理由を比較した。checker全27行は完全一致を確認して退役した。live KNOWNに残る36行は受領時のparse-recovery拒否観測を保持しているが、現在の候補では全36行がexactとなり退役ガードが発火した。全corpusへの影響確認が終わるまで台帳の退役を保留している。
command構築時の拒否はpartial callbackのpath/hashも既存messageに含む。
2回の独立観測が一致し、ID集合・owner/cause・native outcomeがすべて一致したときだけKNOWNとして認める。
exactになったKNOWNと、新しく差分が出た行は引き続き失敗する。
[由来](records/known-native-provenance.v1.json)と `local/known-native-68.log.gz` を参照。

受領時68行はH2.9 parse recovery 36、module resolution request plan 4、H2.5h helper collision 1、checker 27。追加監査後はparse recovery36行のみ。helper1、resolution4、checker27は各2回の完全一致を確認して退役した。
受領時のparse refusal自体は互換成功に数えない。r122の36行の一致は現在の限定観測として記録し、共有parser変更の全体qualificationとは区別する。受領時の「emitter owner 0」は残差全体の解決済みを意味しない。

### Case-insensitive oracleと修復済み台帳

H2.6cのcompiler-runner hostに `@useCaseSensitiveFileNames` を反映し、lookup keyのみcanonical化した。
出力pathやsource spellingを正規化して比較する変更ではない。project descriptor経路にはこのdirectiveがなく、
当該2件はcompiler経路だけなのでproject/config hostは変更しない。
2件の再採取とcase-sensitive対照2件を各2回検証した。643 case recordsのうち641件は不変。
[旧新の完全観測](records/oracle-host-correction.v1.json)では変更はmap write/emit-result mapとそのfingerprintに限られる。
診断、JS callback、exitは不変。採取cacheのidentityもこのhost変更を含む。

RustでEF2/EF3の21 profile memberships（20 unique IDs）がすべてexact×2となったため、
H2.5h 12、H2.6a 1、H2.6c 8の既知差分台帳を空にした。
[退役前の全レコード](records/retired-known.v2.json)を保存し、固定した21行のreplayは残す。
最初のrunがstale-KNOWNを検出したexit 101も保存する。
後続のD/E・globalのcurrent oracle artifactsは公式writerで再生成し、全case/input/observationの不変を
[機械検証](refresh-provenance.py)する。歴史的before/after測定packetのhashを書き換えない。

## Hosted入口と予算

既存65 witness suitesを保持し、syntax-emitter-recoveryと次の12 suitesを追加する（合計78）。全jobは既存の2 workers・60分制限、
45分で分割再検討の方針を維持する。

| suite | 固定した観測範囲 |
| --- | --- |
| emitter-final | oracle対照4、EF2/EF3 21所属、EF2–EF6 batch11 tests、EF8 22＋4、filesystem24、helpers/controls551（539 exact＋12 typed boundary）、CLI58、class24、empty-block72、session4、source-map70、global2、EF7 217＋guard3 tests、request-plan40 tests |
| emitter-universe-oracle | TypeScript 6.0.3で217＋1798を各2回再観測 |
| emitter-plan-base-0..3 | sorted IDのmodulo 4、450/450/449/449行を各2回 |
| emitter-global | 既存global output-only 769行を各2回 |
| emitter-comment-controls | export/destructuring1130＋元JSDoc1＋async arrow372の完全commandを各2回。emitter-globalと同じjobで実行 |
| emitter-class-0 / -1 | 既存8 class bandsを700/528行に分割、各2回 |
| emitter-system-controls | System・関数・コメント・構文回復の周辺5,957 fixture memberships、23 Rust tests。新1,600 controlsを含む |
| emitter-recovery-controls | bounded parse-recovery 1,224 fixture memberships、8 Rust tests |

重複する観測所属なので件数を足して互換ケース総数にしない。PLAN-BASEの約70分のローカル直列測定は
hosted一jobへ持ち込まない。専用fixtureは担当suiteを選び、shared sourceは全既存群を含める。
editing selectorsを除去し、zero/ignored testsと欠けたshard summaryを拒否する。
入口台帳はcomposite runnerを実Cargo commandsへ展開する。
[固定台帳の案内](../../witness-coverage/README.md)からsource hash付きの記録を参照できる。

## Validationとarchitecture

[run-local.py](run-local.py)はargv、開始HEAD/diff、環境、exit、時間、log hashを保存する。
macOS background priority・Cargo 2 workersで逐次実行し、canonicalのtargetをcacheとして利用する。
68 KNOWNの再比較、planner 83 tests、policy checkとfocused policy 2 testsは成功。
ローカル途中のhash drift失敗は、oracle修正後に旧pinを検出したもので記録から除外しない。
最終候補のqualificationは[専用記録](architecture-validation.md)とPR #561に記録する。

この統合作業の開始時、変更する18 concernsを `active-unqualified` に戻した。
[以前のqualification](records/architecture-before.v1.json)を保存し、最終immutable headの実測後に
変更した範囲だけ再qualifyする。E-PLAN-SCRIPTのoption admission、resolver/checker facts、
metadata/class provenance、async capture、name allocation、helper import、printer/commentが対象。
H2.9/一般checker全体、disposed-transform/API再emit、build/watch、TS7移行の完了には拡張しない。
STAGE・TypeScript 6.0.3 pin・製品全体のadmission policyは変更しない。

## 追加監査候補

最初の統合CIは23 checks中13成功・10失敗。失敗は隠さず全jobを
[保存](hosted/progress-ed71c45343b2.json)し、原因ごとの修復と追加比較を
[残差監査](residual-audit.md)に記録した。helper alias、resolution fallback、匿名class名、
async super identity、arrow factory更新順、namespace map、EOF comment、過剰なoption/comment拒否、
CLI named valuesを修復対象とした。既存範囲を縮めず、current artifact consumersのhashも同期する。

影響が大きい・未知の変更については、実際にClaude Codeへ独立調査を依頼し、Codexの
原因分析・観測と[突き合わせ](cross-review/decisions.md)た。意見が食い違った場合も
実測を優先し、review回答だけで成功判定しない。source-kind JSON / parse recovery /
stableTypeOrderingは共有基盤の未解決境界として残す。数値targetの1234と内部JSON値100は
区別し、後者を別名で互換成功に数えない。


## 次段のconsumer更新（r26）

候補3c1dfa51cでconformance49,024はT0/T1/T2/T3すべて一致し、H2.5hも
888 exact / 44 deferred / known0となった。H2.1aの旧comment拒否2行は元の
fingerprintと2回のTS観測を使う完全比較へ接続し、元qualificationは変更しない。
H2.6cは空KNOWNの正規状態を「manifest不在」とする既存readerにwriterを合わせる。
元21所属の再比較は保持する。これらのnative再検証は継続中。

bundle declaration/mapのcurrent参照は公式writerで再採取し、全inputと全観測が
不変であることを[bundle-provenance-refresh-r26](records/bundle-provenance-refresh-r26.json)
へ固定した。差は依存artifactのhashとobserver identityのみ。
旧退役台帳v1の空before payloadは、元SHA-256を照合した[v2](records/retired-known.v2.json)
で訂正した。旧版も訂正理由とともに保存する。

## r34 時点の実測と CI 再実行

D772候補の全23 checksとlogsを[保存](hosted/progress-d772eb554098.json)した。
19成功、2つの実行job失敗とそれに伴う2 gate失敗。conformance49,024全層、
wide、全PLAN-BASE shards、global/class、emitter-finalは成功した。
新しいempty-block72とsession4は完全commandを各2回一致。過去source-map70も
既存全比較を通過した（全70行を各2回測定したという意味ではない）。
config-libraryの24テストもすべて成功したが、shared contractsに7テストが増えたため
filtered countの旧423 pinが実測430を拒否していた。選択・ignored・filteredの厳密検証を
維持して固定値を修正した。H2.6cは追加した1行の元tuple hashがJSONのkey順序を
誤っていたため停止した。135行のpinを元artifactの順序で全照合し、当該1行だけ修正した。
元inputとTypeScript観測は変更しない。次のCIで再検証し、全成功まではqualificationを行わない。

構文回復は別候補で段階的に検証中。通常候補の36 KNOWNは維持しており、
新しいparser predicateや追加426 controlsのTypeScript観測だけを根拠に退役させない。
Claude Fableとの実際の比較を継続している。現時点でrate limitによるOpus切替はない。

## 最終入力台帳の登録修復（r228–r229、2026-09-20）

H2.5gのchanged-crates入力規則を再計算し、599未登録pathsと既に削除されたtest1 pathを確認した。
486 pathsはtrusted mainからbyte不変で、113 pathsはこの統合で変更したものだった。
親86と既存の生存236を保持し、599を明示追加、削除済み1だけを除いて計921入力とした。
既存のshadow除外はbyte不変であり、入力hashをtest実行の証明には数えない。
[Opus165の独立再計算](cross-review/round165-opus.md)と
[提案・census](records/runtime-input-registration-r229/proposal.json)を保存した。
最終の生成物は公式chain walkで更新し、入口台帳v36はwalk後の固定sourceから新規生成する。

[元corpus再検証の途中記録](records/canonical-corpus-pin-review-r451/manifest.json)：
16,994入力のparser再実行は一致した。実出力比較は、過去のClippy修正に伴う検証用ソースhash
2件の更新漏れで採取前に停止した。配列コピー4箇所と不要な参照1箇所の差分だけであることを
元ファイルからの完全一致で照合し、2件のhashを更新した。比較器・入力集合・期待値は維持し、
[確定HEADでの再実行](records/canonical-corpus-r453/manifest.json)は全9段階が成功した。
元16,994入力のparser比較、選定済み48 commands各2回（出力46件・noEmit 2件）、
別枠108 projects各2回が一致した。元の読み込み失敗110件は未検証のまま保持し、
別枠projectsをその代替とは扱わない。正式walk・最終unsplit CI・hosted確認は継続中。

[最終ソース参照の更新](records/final-pin-preparation-r455/manifest.json)：既存ソースhash 25件と、
それに依存するdraft artifact hash 2件を更新した。draft状態・対象集合・判定条件・hosted登録は
変更せず、正式walkと最終CIで検証する。
