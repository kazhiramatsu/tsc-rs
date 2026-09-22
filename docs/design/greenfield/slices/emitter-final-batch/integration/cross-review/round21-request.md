# Round 21: adjacent for-of failures (priority over parser research)

r19修正後の16 ordinary full-command対照は12 exact×2 / 4 failure。
`cross-review/forof-adjacent-r19.json.gz`に完全native/expectedとdecoded JS/mapsを保存。
Rustビルド・テストは現在止まっています。編集/Cargo/他agent起動はしないでください。

独立source照合をお願いします。
1. pattern-headのES5×downlevelIteration on/offで、非patternと同じv2/v3反転が残ります。
`convert_for_of_statement_head`のpattern armはflatten_destructuring_bindingの返却declsを
そのままlist化するので、leafのnameにcolliding_declaration_name_substituteが適用されません。
通常visit_variable_declarationのpattern arm、parameter lowering等も同じshared flattenerを呼びます。
Codex案: ES2015 ownerに薄いbinding flatten wrapperを置き、shared flattenerの返却declsのnameだけ
既存colliding_declaration_name_substitute / update_variable_declaration_nameを通す。
既存5 callerを接続すれば新しいFlattenHost callbackを追加せず同じ不足を閉じられます。
代案はFlattenHostへdefault-noop materialize-binding-name callbackを追加しES2015でoverrideですが
一般shared APIを増やすよりowner wrapperを優先したい。どの範囲がupstream印字順と一致するか、
既存preallocated bindings/finalizerの副作用も照合し最小案を提案してください。

2. captured-loopの2行はJS完全一致、mapsだけ`_loop_1(v_2)`の引数にsource境界がありません。
`generate_call_to_converted_loop_snapshot`12040付近でparsed parameter.nameをcloneして明示map rangeを
付けますが、print時substitution_identifier_cloneはraw pos/endだけを見るためcloneの明示rangeが消えます。
上流107340は`map(state.loopParameters, p=>p.name)`で元nodeを直接使う。
Codex最小案: 不要なcloneとrange転写をやめて`call_arguments.push(name)`とする。
共有substitution関数全体を変更せず実producerを上流と同じに戻せます。
元nodeのparent/original/flagsが共有printで影響しないか確認してください。

この16対照の入力・TS観測は既に2回固定済みです。既存480も完全不変。
追加すべきnegative controls（通常blockのarray/object pattern、複数binding、captured noncolliding等）を
少数で提案してください。元4失敗はKNOWN化せず全一致まで進めます。
