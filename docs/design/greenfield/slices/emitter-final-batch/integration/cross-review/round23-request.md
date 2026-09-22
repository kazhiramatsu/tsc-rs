# Round 23: retained object/array pattern maps

r21の修正で元16は全exact×2、追加28のうち26 exact×2。初期化子内同名bindingの仮説対照も全一致。
残る2行はES2015×iteration on/offのblock-object-restで、JS完全一致、mapに1 segmentだけ余分です。
`object-rest-map-r21.json.gz` にnative/TS完全captureがあります。いまRust build/testは走っていません。

Codex解析:
入力 `declare const o: {a?: number, b: number}; let v; { let { a: v = 1, ...r } = o; }`
出力 `let { a: v = 1 } = o, r = __rest(o, ["a"]);`
nativeだけgenerated(line=14,col=20) -> original(line=0,col=73)がある（0-based）。`=` tokenの位置で、name.endが元patternの閉じbraceに向いています。
es2018.rs::flush_object_pattern_chunk 3528はBindingだけset_original_and_range(pattern, original_pattern)します。
TS flattenObjectBindingOrAssignmentPattern 93500以降はmakeObjectBindingPattern=93681のfresh factory patternを
emitBindingOrAssignmentへ渡します。元patternは最後のoriginal引数として返却VariableDeclarationへ付くので、
retained chunk自体のpos/endやoriginalは付けていません。

最小案はchunk側set_original_and_rangeを除去し、既存materialize_binding_planのdeclaration範囲/originalと
retained leafのsource provenanceは保持すること。代替としてNO_TRAILING_SOURCE_MAPだけを足すのは
実際のfresh node状態を直さず`=` map producerを抑えないので採りません。
同じ疑いがflatten_array_pattern 3587のset_original_and_range(retained_pattern, pattern)にもあります。
TSのmakeArrayBindingPattern 93675およびemitBindingOrAssignment 93598は同じfresh patternです。
配列側も直すべきか、現在のRust emission-plan/下流ES2015でoriginal/rangeを必要としている箇所はないか、
独立に検討してください。JS/map/comment両面の負の対照も少数提案を。

この2行もKNOWNにせず閉じます。r22のparser-facts設計はAに同意し、missingのFULL-startを診断位置と
別に記録する補足をrecovery-design-r22.mdへ保存しました。こちらは現在のfor-of/ES2018差分の修復を先に固定してから実装します。
編集/Cargo/他agentなし、sourceと必要なら小さなTS probeだけで回答してください。
