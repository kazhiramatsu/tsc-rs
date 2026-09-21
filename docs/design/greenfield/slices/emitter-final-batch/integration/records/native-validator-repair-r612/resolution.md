# Local CI の残存テスト・H0 宣言照合の修復（r611–r612）

候補 `fa391cbff6468032ebd4969734cde9d2b9474237` の unsplit local CI r603 は
終了 1、21,586.876 秒、未合格だった。5 phases 完了後に workspace-tests で失敗した。
96 対象を実行し、94 対象が成功、compiler と conformance のライブラリで計 3 テストが失敗した。
失敗した実行のログ・終了状態・履歴 journal をそのまま保存し、合格記録に転用しない。

CLI のテストは、共有 option catalog により ES5 を受理する実装に対して古い拒否を期待していた。
既存テストを ES5=1 の確認へ更新し、未知の target を拒否する対照を保持した。
compiler の動作を変更していない。

H0 の失敗 2 件は、現在の宣言 `pub fn resolve<'j0, 'j1>(` と、固定台帳に残る
`pub fn resolve(` の照合不一致だった。全 241 行・19 種類のアンカーを調べ、
不一致を resolve 229 行、resolve_with_facts 11 行、resolve_type_reference 1 行に限定した。
3 宣言は trusted baseline でもすでにジェネリック付きであり、今回の train による変更ではない。
Opus は履歴上の変更を UTF-16 対応コミット d364a056a まで確認した。
各 closing commit の元の宣言は存在し、ルート側でも 10 組の commit/symbol を直接検証した。

修正は現在のファイルを照合する検証器に限定する。producer・crate・path を限定した
明示的な 3 件の対応表を使い、確認済みの lifetime 綴りだけを認める。
台帳に対応しない項目、重複、別の関数への対応付けは拒否する。
元の綴りが再び現れた場合は対応表の退役を要求し、コメント中の出現でも拒否する。
現在の綴りは、コメント・文字列の外にある所定の宣言行がちょうど 1 件の場合だけ通す。
これは字句上の検証であり、impl の型や cfg・macro の意味を解決するものではない。
それ以外のアンカー照合、closing commit の文字列照合、台帳の固定条件は変更していない。
台帳 JSON、compiler・program の本体、固定 recovery snapshot も元の bytes を保持している。

実際の Claude Opus 235・236 と独立に照合した。236 では差分の接続箇所、対象集合、
UTF-8 境界、文字列・コメント処理、履歴検証を確認し、修正範囲に問題なしとされた。
追加提案に従って Unicode 文字列・文字・escape と宣言行末の対照を補った。

検証結果は次のとおり。各コマンドの実行記録は analysis/ に置く。

- conformance 全ライブラリ 248 tests が成功。その後のコメント明確化・Unicode 対照追加を含む最終 bytes で、新規対照 5 tests を再実行して成功。
- 最終 bytes の compiler 全ライブラリ 11 tests が成功。
- `host-resolution check --baseline 3b1f5fe87fd31e3b303bb44bd257342735452ed9` が成功。全 241 行 closed、open 0、lapsed 0。後段の semantic history と共通の検証経路を確認した。
- 既存の CLI option 58 完全コマンド比較と診断順序 4 ケースが各 2 回成功。ES5 の直接 CLI 指定と診断も既存 fixture に含む。
- 全 pin surfaces の preflight と workspace/all-targets clippy `-D warnings` が成功。
- 元の 90 未追跡ファイルは path・bytes・SHA-256・mtime を保持した。

最終 Rust 集約 hash は `2250a4976d0c6b7ecbfec7bf6d9caa022bca2673863d0595fd7f8e8ec9bb4b84`。
CLI unit source は H2.5a～g の 7 profile の runtime_inputs でもある。
旧 r597 の全 Rust 収束証明を新しいソースへ流用せず、正式 walk を再実行する。
新しい witness inventory はその後の source から最後に作成する。
この記録は修復と重点検証の証拠であり、最終候補の full CI・hosted・merge の合格証明ではない。
