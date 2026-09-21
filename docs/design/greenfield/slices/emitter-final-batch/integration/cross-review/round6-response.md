最終確認です。追加読み取りは不要でした。

## 判定: 現在の所有分割は upstream と一致し、2 件の regression 修正は正しい

- **before-side** = `expression.pos` の trailing comment のみ (同一行、改行で停止)。upstream 119772-119774 `emitTrailingCommentsOfPosition(node.expression.pos)` と同じ。
- **子の通常 phase** = `child.pos` の leading (改行後のみ、`collect_source_comment_ranges` 18526) と `child.end` の同一行 trailing。upstream の pipeline と同じ。
- **after-side** = `expression.end` の leading のみ (16459-16470、無変更)。upstream 119776-119778 と同じ。

regression 2 件の入力 `(\n// comment\n undefined as number)` は、`expression.pos` からの trailing 走査が最初の改行で止まって何も書かず、`// comment` は子 phase が 1 回だけ書く。削除した leading 呼び出しは子 phase 稼働前の補償だったので、除去が唯一正しい形です。`/*open-block*/\n//lead` (4271) は `/*open-block*/` が before-side、`//lead` が子 leading で、子の収集は改行前を採らないため二重化しません。

## 既存の小さな不一致 (今回の変更が原因ではない、任意)

before-side は `emit_source_trailing_comments_of_position` (= `emitTrailingComment` の port、**prefix space あり**) + 16449-16454 の suffix space 補正を使っていますが、upstream の `emitTrailingCommentsOfPosition(pos)` は prefixSpace 未指定なので `emitTrailingCommentOfPosition` (121208-121218): **prefix なし、各 comment 後に space か改行**。Rust には同等の port `emit_source_intervening_comments_of_position` (18468-18480) が既にあります。

観測可能な差は直前に空白がない場合だけです:
- `f(<T>/*c*/x)` → TS `f(/*c*/ x)`、Rust は `f( /*c*/ x)` になる見込み
- `f((/*c*/ x as T))` も同形

修正は 16448 の呼び出しを `emit_source_intervening_comments_of_position` に差し替え、16449-16454 の補正を削除する 1 箇所のみ。4271 の `return ( /*open-block*/` は parsed paren の **token** 経路 (upstream 118755-118761、prefixSpace=true) なので影響しません。上記 2 入力を TS 観測で固定した上で入れるなら本 train に載せられる規模です。観測が一致しなければ据え置きで構いません。
