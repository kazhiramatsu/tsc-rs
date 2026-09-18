## 回帰 2 件の独立検討

### 1. conformance set-ratchet 38 行(T3/T4)

**原因(確度高): `elementwise_elaboration_related` に追加した default-library フィルタが、oracle のホストモデルと食い違う。**

- upstream の判定は `isSourceFileDefaultLibrary(file) = libFiles.has(file.path)` (`_tsc.js`, program 内) で、ファイル内容ではなく Program が既定 lib として追加したかどうかだけを見ます。
- conformance oracle は `noLib: true` + lib をルートとして渡す構成です(`crates/oracle/files-dump.mjs:4`、`coverage-driver.mjs:72`、`h1-emit-oracle.mjs:56`)。この構成では `libFiles` が空なので、tsc は lib 宣言に対しても related 行 (`The expected type comes from this index signature` / `...declared here on type`) を必ず出します。
- Rust 側は `elaboration.rs:493` の `is_default_library_declaration` が `file_facts(file).is_default_library()` を読み、H0 アダプタは lib 文書を無条件に `DEFAULT_LIBRARY` にします (`crates/checker/src/lib.rs:2068`、conformance は `ProgramJson.libs` を別配列で渡す: `crates/harness/src/lib.rs:47`)。結果、lib 由来の related 行が抑止されます。
- 可視 3 行はすべて配列リテラル要素 vs `number[]`/`any[]` で、related の宣言先が `lib.es5.d.ts` の `Array` index signature です(0-based 列で `"string"`、`undefined`、`null` の位置と一致)。残り 30 行も「target の宣言が lib にある elementwise 行」と予想します。T3 identity に related 情報が含まれている前提です(xtask 側の identity 構成は未確認)。

**確認すべき観測**: 38 行それぞれで差分が「lib.*.d.ts を指す related 行の欠落のみ」であること。逆に、このフィルタを入れた動機の CLI 行(catalog lib 構成、`import-helpers.json` などの `comes from this index signature` 行)は抑止が正しいので、両モデルを同時に満たす必要があります。

**最小修復案**: `ProgramFileFacts` に「host の libFiles メンバか」を表す 1 bit を追加し(既存の `default_library` は `lib_count()`/`skipDefaultLibCheck` 用に据え置き)、H0 アダプタ (`lib.rs:2068`) で呼び出し元のホストモデルから設定する。oracle 互換 conformance 経路は false、Program/CLI の catalog lib は true。`is_default_library_declaration` はこの bit だけを読む。フィルタ撤去で戻すのは CLI 側の正しい行を壊すので不可。

### 2. H2.5h ES5For-of20 (target es5)

**原因は未特定です。** 以下は仮説と収集すべき観測です。

事実:
- 設定は target/strict のみで sourceMap なし。`writes_diverging=1` は唯一の `.js` 書き込みが異なる意味 (`h2_2c_acceptance.rs:2728`)。ratchet は `two-deterministic-exact-runs` なので以前は exact。
- source 129 バイト = ディレクティブ 2 行込みなので、出力は `// @target…` `// @strict…` を for 文の leading comment として持ちます。
- vendored tsc の期待本体(ディレクティブ行を除いた scratch 出力)は次のとおりです。

```
for (var _i = 0, _a = []; _i < _a.length; _i++) {
    var v = _a[_i];
    var v_1;
    for (var _b = 0, _c = [v_2]; _b < _c.length; _b++) {
        var v_2 = _c[_b];
        var v_3;
    }
}
```

除外できる変更: CommonJS transformer 群(module 無しの script)、es2017/class_fields/standard_decorators(該当構文なし)、execute の option 検証、parser の `(` hasJSDoc 変更(ExpressionStatement なし)、module_requests。printer PEE 分岐の `debug_assert!` は panic にしかならず、差分の原因にはなりません。

仮説(順位付き)と観測:
1. **リネーム/明示初期化子**: `v_1/v_2/v_3`、`[v_2]`、`var v_1;`(`= void 0` 無し)が一致するか。resolver 経路 (`resolve.rs`) の差分は isolatedModules ゲート付きなので低確度ですが、最初に見るべき箇所です。
2. **EOF 改行**: `printer.rs:2474` の追加 `write_line(false)` は行頭では no-op です。末尾バイト数の差だけなら、statement list 終端で行頭になっていない経路が原因です。
3. **先頭コメント**: 2 行のディレクティブが重複/欠落していないか。今回の comment 変更は PEE/アクセス対象限定なので低確度。
4. **`const v;` 経路**: `es2015.rs` の `pending_assigned_name` 撤去は関数/クラス名のみに影響するはずで、最も低確度。

最初の差分バイトを取れば上記のどれかに即座に絞れます。絞れない場合は、emitter 側 hunk が少ないので `printer.rs:2474` と `es2015.rs` の hunk を個別に戻して単一ケースを再生するのが最短です。

### 3・4
CLI 契約の macOS case-insensitive outFile 行の完全 TS 比較化、controls の manifest hash 同期は妥当です。追加の指摘はありません。
