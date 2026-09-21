The r108 repair holds up. The 35 new checker identities map to 22 honest ports and 13 Rust-native adapters (the table's tallies say 21/14 and are off by one; the table itself is correct). Nothing was missed. The larger finding is that "zero stale" does not mean the existing spans point at the right functions: 391 older entries fail a containment check. Sources: the ledger-prep diff, the 35 function bodies, the vendored `_tsc.js` and `/tmp/emitter-ts-functions-r108.json`. My only scan was a read-only Python pass (`/tmp/r110-containment.txt`); no edits or builds.

## 1. The r108 repair: no bad mappings

- **Every r108-changed block passes containment.** I checked all 4,087 `_tsc.js` entries for whether the span equals, or sits inside, the upstream function with the same name. None of the failures is in a block r108 changed.
- **Your three corrections match the index exactly:**
  - `createPrinter.getSeparatingLineTerminatorCount` `120301-120329`. The old `120299-120327` was shifted.
  - `createNodeFactory.createExpressionWithTypeArguments` `22944-22954`. The old span ran 3 lines into the next function.
  - `parseIsolatedEntityName2` `29042-29060`, hash `38241864…`. This changes `parser.rs` bytes, so it needs the corpus successor pair you planned.
- The prose line "Reference detail: … @6.0.3" is harmless: it doesn't start with `tsc-port:`.

## 2. Inherited issue: 391 older entries fail containment (not caused by r108)

All of these have self-consistent hashes, so `ledger check` passes them, but their spans don't match the named function. By crate: checker 205, emitter 144, program 18, binder 9, types 9, compiler 3, syntax 2, conformance 1.

Three kinds:
- **Over-extended.** For example `bind` `44226-44255` against the function's `44226-44251`.
- **Shifted.** For example the siblings of the new `_js` twins:
  - `engine.rs:1606` `reportError` is `65037-65046`; the real function is `65042-65051`.
  - `engine.rs:1850` `reportIncompatibleError` is `64942-64947`; the real function is `64947-64951`.
- **Composite names**, 116 rows with no matching name, such as `captureErrorCalculationState/resetErrorInfo`.

Don't fix these on this train. Record the list as a follow-up, and don't copy those spans into the new twins.

## 3. The 35 new identities

Summary: 21 honest `tsc-port` blocks, some split into exact sub-spans (22 per the table's `kind` column), and 14 `tsrs-native` with a reason (13 per the table). Hashes use the ledger's inclusive-line rule.

```json
[
{"id":"access.rs::is_method_access_for_call","kind":"tsc-port","port":"isMethodAccessForCall","span":"_tsc.js:75081-75086","hash":"9548723ba085419779c0d0e8fea4e739dfb92b3de3ad2a6d8003d5bc78a19988"},
{"id":"access.rs::related_info_for_node_js","kind":"tsrs-native","reason":"JS-valued twin of related_info_for_node: RelatedInfo adapter over createDiagnosticForNode span/message; same disposition as its sibling"},
{"id":"check.rs::strip_symbol_name_quotes_slice","kind":"tsc-port","blocks":[
  {"port":"stripQuotes","span":"_tsc.js:16340-16346","hash":"4673cbaa5c93e9e8bab885c7105082659bff78533e932a123e9d4c5a51fe5e63"},
  {"port":"createExpressionFromSymbolChain","span":"_tsc.js:53368-53368","hash":"a1d9b416b5bf0ea54c37c3efe9002c65fa36dba915643ac6cb3477d84438ffd5","prose":"stripQuotes(...).replace(/\\\\./g) literal; prose 53368-53371 overstated"}]},
{"id":"check.rs::symbol_name_from_name_type_slice","kind":"tsc-port","port":"getNameOfSymbolFromNameType","span":"_tsc.js:55523-55540","hash":"f499724c3f5c9e776aaed901cfc7531d0c6d73d9d048a13ea76880cacec9fc3c","note":"prose says 55523-55539; index end is 55540"},
{"id":"class.rs::first_syntactic_decorator","kind":"tsc-port","port":"getFirstTransformableStaticClassElement","span":"_tsc.js:84937-84937","hash":"0347591824e6fc73b7ea30fef6d58a29777ea5a89f8b2b20ab02633bb50c95dd","prose":"firstOrUndefined(getDecorators(node)) projection"},
{"id":"class.rs::first_transformable_static_class_element","kind":"tsc-port","port":"getFirstTransformableStaticClassElement","span":"_tsc.js:84921-84949","hash":"2963f39a45b2e07f6581c2a3e95d26d288f775c5edeee85290d9dbcb71bce83a","prose":"classOrConstructorParameterIsDecorated(false,node) projected as class-has-decorator: parameters are not decoratable under standard decorators"},
{"id":"declaration_emit.rs::with_declaration_emit_replay_observer_for_harness","kind":"tsrs-native","reason":"doc(hidden) harness-only scalar-serde compatibility wrapper over the JS replay observer seam; no upstream counterpart"},
{"id":"elaboration.rs::capture_literal_assignment_elaboration_from_types","kind":"tsrs-native","reason":"captured-diagnostic sink variant of capture_literal_assignment_elaboration (same disposition as its sibling); drives elaborate_assignment_relation"},
{"id":"emit.rs::get_global_diagnostics","kind":"tsrs-native","reason":"observes the current file-less bucket with sort/dedupe WITHOUT tsc checker.getGlobalDiagnostics' ensurePendingDiagnosticWorkComplete (87133-87136); cite that difference"},
{"id":"emit.rs::with_program_diagnostics","kind":"tsrs-native","reason":"builder setter storing program preparation/semantic diagnostic context"},
{"id":"engine.rs::report_error_js","kind":"tsc-port","port":"reportError","span":"_tsc.js:65042-65051","hash":"a871642f2e5fcb05497d98e85f5b73f64a73926c96c9b7b595b44ce86be6b5d9","note":"do not copy sibling's shifted 65037-65046"},
{"id":"engine.rs::report_incompatible_error_js","kind":"tsc-port","port":"reportIncompatibleError","span":"_tsc.js:64947-64951","hash":"ef22e98a04b5ee9d00cec528c64967a94f296599254a0b8a5539960de323b210","note":"sibling span 64942-64947 is shifted"},
{"id":"expr.rs::grammar_error_on_node_js","kind":"tsc-port","port":"grammarErrorOnNode","span":"_tsc.js:90240-90247","hash":"7b4cfaed5d73c81a86f98ab2a735bed402c39439fcddf19cb213a07e01f7ef50"},
{"id":"functions.rs::get_entity_name_from_type_node","kind":"tsc-port","port":"getEntityNameFromTypeNode","span":"_tsc.js:14623-14635","hash":"18fbb4d47813f69cb1fc135ef8805253c782b926f276b3cd78f327f8c1c1efa1"},
{"id":"functions.rs::is_private_identifier_class_element","kind":"tsc-port","port":"isPrivateIdentifierClassElementDeclaration","span":"_tsc.js:11944-11946","hash":"4d2410e4b12837c830e30a5cdf2c7dd2d7a5fd7223a3a19fe0a698e91a925795"},
{"id":"jsdoc.rs::has_jsdoc_property","kind":"tsrs-native","reason":"projects node.jsDoc presence from the immutable arena plus the getJSDocTagsWorker cache side effect"},
{"id":"lib.rs::check_directive","kind":"tsrs-native","reason":"lexical projection of leading-comment @ts-check/@ts-nocheck (processCommentPragmas 36215 + processPragmasIntoFields checkJsDirective ~36288, last wins); not a structural port"},
{"id":"lib.rs::concat_js","kind":"tsrs-native","reason":"JsString concatenation utility (JS + on strings)"},
{"id":"lib.rs::join_js_strings","kind":"tsrs-native","reason":"JsString Array.join utility without scalar round-trip"},
{"id":"lib.rs::join_js_texts","kind":"tsrs-native","reason":"JsTextPart Array.join utility"},
{"id":"lib.rs::with_authoritative_modules_at_for_declarations","kind":"tsrs-native","reason":"Rust API entry configuring one authoritative checker for per-source declaration APIs"},
{"id":"lib.rs::with_js_doc_parsing_mode","kind":"tsrs-native","reason":"builder setter for transpile jsDocParsingMode (typescript.js:146097 as cited)"},
{"id":"lib.rs::with_module_name","kind":"tsrs-native","reason":"builder setter for transpile moduleName (typescript.js:146099-146101)"},
{"id":"lib.rs::with_renamed_dependencies","kind":"tsrs-native","reason":"builder setter for renamedDependencies (typescript.js:146102-146104)"},
{"id":"links.rs::or_calculated_flags","kind":"tsrs-native","reason":"links storage primitive for calculateNodeCheckFlagWorker's `calculatedFlags |=` sites (88170/88179/88187/88199/88201/88213); current prose cites 88132, which is the noCheck guard: fix it"},
{"id":"mapped.rs::is_array_or_tuple_or_intersection","kind":"tsc-port","blocks":[
  {"port":"isArrayOrTupleOrIntersection","span":"_tsc.js:59086-59088","hash":"7d99cf49f398cf7f86a8c5c1556230d69c1065a67a2d323af9722dd08a5f97eb"},
  {"port":"getResolvedApparentTypeOfMappedType","span":"_tsc.js:59080-59080","hash":"993cbe1a08b150cb5cef94c1040dc8a91d3bb90590ac1da233cfc5fa241f2ab6","prose":"combined `isArrayOrTupleType(t) || isArrayOrTupleOrIntersection(t)` lambda"}]},
{"id":"merge.rs::related_for_node_js","kind":"tsrs-native","reason":"JS twin of related_for_node RelatedInfo adapter (sibling is tsrs-native)"},
{"id":"modules.rs::mark_linked_references_async_function","kind":"tsc-port","blocks":[
  {"port":"markLinkedReferences","span":"_tsc.js:71662-71668","hash":"5bfb1b06d6bbd776c9ef32b5d1aa3e0720db62113276dc9abe99b2a9a0558522","prose":"front-door guards"},
  {"port":"markLinkedReferences","span":"_tsc.js:71678-71679","hash":"808d14cedeff78d9fa5481de6203b976b60c89d4ecbb6b9b0a74cd59748e0780","prose":"AsyncFunction arm; reject whole 71662-71732 (other hint arms not mirrored)"}]},
{"id":"modules.rs::normalize_js_program_path","kind":"tsrs-native","reason":"JS-valued lexical program-path normalization adapter over tsc_program (empty→'.'/'/' defaults)"},
{"id":"node_builder/specifier.rs::normalized_slashes","kind":"tsc-port","port":"normalizeSlashes","span":"_tsc.js:5452-5454","hash":"d53c3e92f0b97072b15fe2ed30c413ab7f33522619f88528f818eef207535163"},
{"id":"state.rs::create_error_js","kind":"tsc-port","port":"createError","span":"_tsc.js:47580-47582","hash":"dedcf6cc6c301274f018ef98543f4abebe1b7826c45f601b914137812caa8cfa"},
{"id":"state.rs::diagnostic_for_node_js","kind":"tsc-port","port":"createDiagnosticForNode","span":"_tsc.js:13909-13912","hash":"358ec88d52a45803957de382a162466361b967ba9a0665a2d6d7431d2eff55fe"},
{"id":"state.rs::error_at_js","kind":"tsc-port","port":"error","span":"_tsc.js:47583-47587","hash":"be9cd419909a0ad4fd544342a9a6c97f837da3819b2844e45c7be96b438439c9"},
{"id":"state.rs::error_at_with_related_js","kind":"tsrs-native","reason":"JS twin of error_at_with_related (sibling tsrs-native): attaches related info to createError"},
{"id":"state.rs::lookup_or_issue_error_js","kind":"tsc-port","port":"lookupOrIssueError","span":"_tsc.js:47565-47574","hash":"9571aad04fba17397e7740b9b0f7b02e8646fb85b89ae01858ad7879ead111d6"}
]
```

## 4. Dependency and call-chain concerns

1. **`_js` twins.** They duplicate the logic of their ported scalar siblings: `create_error`, `error_at`, `lookup_or_issue_error`, `report_error`, `report_incompatible_error`, `grammar_error_on_node`, `diagnostic_for_node`. Give each twin its own `tsc-port` block, and add one prose line naming its scalar twin, for example "JS-valued twin of `report_error`". Without that link, a later fix to one copy can silently miss the other.
2. **`is_array_or_tuple_or_intersection`.** Its body is the combined 59080 lambda, not upstream's intersection-only predicate. The `instantiate.rs:1300` caller is correct only because it tests `INTERSECTION` first, matching upstream 63592. Put that dependency in the prose so the external guard isn't removed later.
3. **`get_global_diagnostics`.** It deliberately skips `ensurePendingDiagnosticWorkComplete`. Its only caller is `compiler/src/declaration_diagnostics.rs:189`. Keep it `tsrs-native` and state the difference, rather than claiming a port that behaves differently.
4. **`first_transformable_static_class_element`.** It relies on `first_syntactic_decorator(node).is_some()` standing in for `classOrConstructorParameterIsDecorated(false, node)`. That equivalence holds only because parameters can't be decorated under standard decorators. Record it in the prose.
5. **`or_calculated_flags`.** Its current prose line reference is wrong (88132 is the noCheck guard). Correct it along with the disposition.

Adding these checker `tsc-port` spans may create new frozen D2 joins for rows currently marked *deferred*. That is allowed, since joins only move in one direction. Rerun the D2 audit and the backlog check after the root adds the comments, without editing `fn-dispositions.toml` or any gate.
