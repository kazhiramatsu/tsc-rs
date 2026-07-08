# 2XXX emitter inventory — the function-level reproduction checklist

GENERATED ANALYSIS (2026-07-08, tsc 6.0.3 pin) — the answer to "is
every tsc function that emits a 2XXX diagnostic reproduced?" as a
complete, line-ordered checklist. Regenerate on re-vendor with
`xtask codegen band-inventory --by-function` (the tool this table
specifies; until it exists, the analysis script lives in the
conformance-sweep session notes and is ~40 lines: one pass over
`_tsc.js` collecting `Diagnostics.<name>` uses joined against the
2000-2999 message table, attributed to the nearest enclosing
`function`, EXCLUDING `.code` reads).

How to read:

- **What each function implements**: the hand-audited companion
  [2xxx-emitter-descriptions.md](2xxx-emitter-descriptions.md)
  carries, for every row below, a description of the function's role
  and the condition under which each code fires (plus corrections
  the audit surfaced, e.g. the @47312 resolver-object artifact).
- **NAMED** = the function appears explicitly in the design docs
  (a port-table row or algorithm skeleton owns it).
- **helper** = not individually named; it is reached when its
  region's named row is transcribed (2xxx-first-order.md rule:
  unknown helpers encountered mid-transcription become new port
  rows — never inlined approximations). Both statuses end the same
  way: a `tsc-port` ledger entry per function.
- **Closure criterion (phase 9)**: every row below has a ledger
  entry or an explicit out-of-scope note. 246 functions, 593
  emission sites.
- **module home** is derived from a line-band map of checker.ts
  region order — it is a NAVIGATION hint; the binding assignment is
  the port-table row. A function near a band boundary may belong to
  the neighboring module (e.g. `getCannotFindNameDiagnosticForName`
  @69328 sits in the infer band but belongs to symbols.rs with
  resolveName).
- **Membership tables, not emitters** (`Diagnostics.X.code` reads;
  excluded above): `lookupFromPackageJson` (the module-resolution
  diagnostics set, 13 codes), `reportIncompatibleStack` (7 —
  ALSO a real emitter below), `reportUnmatchedProperty` (2),
  `reportNonexistentProperty` (1), `checkBinaryLikeExpressionWorker`
  (1). The band-inventory tool must keep this split.

Notable facts this table fixes beyond earlier passes: the parser
emits SEVEN band codes, not three (`parseErrorForMissingSemicolonAfter`
also carries 2427 and 2457, and
`parseJsxElementOrSelfClosingElementOrFragment` emits 2657 — JSX
parent-element rule — at PARSE time); the binder's emitters and the
program layer's are exactly the ones already homed in impl-binder /
program-and-modules §2b.

| line | function | codes (n) | module home | status |
|---|---|---|---|---|
| 19643 | `resolveNameHelper` | 2302,2467,2562 (3) | (utilities) symbols.rs / stmts.rs enum-eval | NAMED |
| 29613 | `parseErrorForMissingSemicolonAfter` | 2427,2457,2819 (3) | PARSER (impl-parser) | NAMED |
| 32323 | `parseSuperExpression` | 2754 (1) | PARSER (impl-parser) | NAMED |
| 32394 | `parseJsxElementOrSelfClosingElementOrFragment` | 2657 (1) | PARSER (impl-parser) | NAMED |
| 33077 | `parseBlock` | 2809 (1) | PARSER (impl-parser) | NAMED |
| 36275 | `processPragmasIntoFields` | 2458 (1) | program: pragmas (§2b) | NAMED |
| 41831 | `tryLoadInputFileForPath` | 2209,2210 (2) | program: moduleResolution (§2) | helper |
| 42626 | `declareSymbol` | 2300,2451,2528,2567,2752,2753 (6) | BINDER (impl-binder) | NAMED |
| 43913 | `bindModuleDeclaration` | 2668 (1) | BINDER (impl-binder) | helper |
| 44996 | `bindClassLikeDeclaration` | 2300 (1) | BINDER (impl-binder) | helper |
| 47312 | `getResolvedSignatureWorker` | 2489,2490,2519,2547,2767,2768 (6) | checker: symbols.rs (init/name/alias) | helper |
| 47618 | `errorAndMaybeSuggestAwait` | 2773 (1) | checker: symbols.rs (init/name/alias) | helper |
| 47628 | `addDeprecatedSuggestionWorker` | 2798 (1) | checker: symbols.rs (init/name/alias) | helper |
| 47747 | `mergeSymbol` | 2649 (1) | checker: symbols.rs (init/name/alias) | NAMED |
| 47758 | `reportMergeSymbolError` | 2300,2451,2567 (3) | checker: symbols.rs (init/name/alias) | helper |
| 47840 | `mergeModuleAugmentation` | 2664,2671 (2) | checker: symbols.rs (init/name/alias) | helper |
| 47888 | `addUndefinedToGlobalsOrErrorOnRedeclaration` | 2397 (1) | checker: symbols.rs (init/name/alias) | helper |
| 48103 | `checkAndReportErrorForInvalidInitializer` | 2301,2844 (2) | checker: symbols.rs (init/name/alias) | helper |
| 48137 | `onFailedToResolveSymbol` | 2552,2570,2833 (3) | checker: symbols.rs (init/name/alias) | helper |
| 48177 | `onSuccessfullyResolvedSymbol` | 2372,2373,2866 (3) | checker: symbols.rs (init/name/alias) | helper |
| 48240 | `checkAndReportErrorForMissingPrefix` | 2662,2663 (2) | checker: symbols.rs (init/name/alias) | helper |
| 48263 | `checkAndReportErrorForExtendingInterface` | 2689 (1) | checker: symbols.rs (init/name/alias) | helper |
| 48303 | `checkAndReportErrorForUsingTypeAsNamespace` | 2713 (1) | checker: symbols.rs (init/name/alias) | helper |
| 48339 | `checkAndReportErrorForExportingPrimitiveType` | 2661 (1) | checker: symbols.rs (init/name/alias) | helper |
| 48352 | `checkAndReportErrorForUsingTypeAsValue` | 2840,2863,2864 (3) | checker: symbols.rs (init/name/alias) | helper |
| 48426 | `checkAndReportErrorForUsingNamespaceAsTypeOrValue` | 2708,2709 (2) | checker: symbols.rs (init/name/alias) | helper |
| 48462 | `checkResolvedBlockScopedVariable` | 2448,2449,2450 (3) | checker: symbols.rs (init/name/alias) | NAMED |
| 48699 | `getTargetofModuleDefault` | 2594 (1) | checker: symbols.rs (init/name/alias) | helper |
| 48751 | `reportNonDefaultExport` | 2613 (1) | checker: symbols.rs (init/name/alias) | helper |
| 48917 | `errorNoModuleMemberSymbol` | 2614 (1) | checker: symbols.rs (init/name/alias) | helper |
| 48933 | `reportNonExportedMember` | 2305,2459,2460 (3) | checker: symbols.rs (init/name/alias) | helper |
| 49127 | `resolveAlias` | 2303 (1) | checker: symbols.rs (init/name/alias) | NAMED |
| 49299 | `resolveEntityName` | 2503,2694,2713 (3) | checker: symbols.rs (init/name/alias) | helper |
| 49467 | `resolveExternalModuleName` | 2307,2792 (2) | checker: symbols.rs (init/name/alias) | helper |
| 49504 | `resolveExternalModule` | 2306,2665,2732,2834,2835,2846… (9) | checker: symbols.rs (init/name/alias) | NAMED |
| 49912 | `visit` | 2308,2744 (2) | checker: symbols.rs (init/name/alias) | helper |
| 55968 | `getBindingElementTypeFromParentType` | 2700 (1) | checker: symbols.rs (typeOfSymbol) | helper |
| 56396 | `getInitializerTypeFromAssignmentDeclaration` | 2300 (1) | checker: symbols.rs (typeOfSymbol) | helper |
| 56906 | `reportCircularityError` | 2303 (1) | checker: symbols.rs (typeOfSymbol) | NAMED |
| 57170 | `getBaseConstructorTypeOfClass` | 2507 (1) | checker: declared.rs | NAMED |
| 57211 | `reportCircularBaseType` | 2310 (1) | checker: declared.rs | helper |
| 57268 | `resolveBaseTypesOfClass` | 2310,2508,2509 (3) | checker: declared.rs | helper |
| 57338 | `resolveBaseTypesOfInterface` | 2312 (1) | checker: declared.rs | helper |
| 57427 | `getDeclaredTypeOfTypeAlias` | 2456 (1) | checker: declared.rs | helper |
| 57678 | `lateBindMember` | 2718,2733 (2) | checker: declared.rs | helper |
| 58594 | `getTypeOfMappedSymbol` | 2615 (1) | checker: declared.rs | helper |
| 58941 | `getImmediateBaseConstraint` | 2313,2751 (2) | checker: members.rs | helper |
| 59825 | `getReturnTypeOfSignature` | 2577 (1) | checker: type_nodes.rs / types-crate | helper |
| 60236 | `getTypeFromClassOrInterfaceReference` | 2314,2707 (2) | checker: type_nodes.rs / types-crate | helper |
| 60305 | `getTypeFromTypeAliasReference` | 2314,2707 (2) | checker: type_nodes.rs / types-crate | helper |
| 60488 | `checkNoTypeArguments` | 2315 (1) | checker: type_nodes.rs / types-crate | helper |
| 60623 | `getTypeDeclaration` | 2316,2317 (2) | checker: type_nodes.rs / types-crate | helper |
| 60633 | `getGlobalValueSymbol` | 2468 (1) | checker: type_nodes.rs / types-crate | helper |
| 60636 | `getGlobalTypeSymbol` | 2318 (1) | checker: type_nodes.rs / types-crate | helper |
| 60639 | `getGlobalTypeAliasSymbol` | 2317,2318 (2) | checker: type_nodes.rs / types-crate | helper |
| 61243 | `createNormalizedTupleType` | 2799,2800 (2) | checker: type_nodes.rs / types-crate | NAMED |
| 61400 | `removeSubtypes` | 2590 (1) | checker: type_nodes.rs / types-crate | NAMED |
| 61879 | `checkCrossProductUnion` | 2590 (1) | checker: type_nodes.rs / types-crate | helper |
| 62227 | `getPropertyTypeForIndexType` | 2339,2493,2514,2536,2537,2538… (10) | checker: type_nodes.rs / types-crate | helper |
| 62404 | `errorIfWritingToReadonlyIndex` | 2542 (1) | checker: type_nodes.rs / types-crate | helper |
| 62652 | `getConditionalType` | 2589 (1) | checker: type_nodes.rs / types-crate | NAMED |
| 62860 | `getTypeFromImportTypeNode` | 2694 (1) | checker: type_nodes.rs / types-crate | helper |
| 63157 | `getThisType` | 2526 (1) | checker: type_nodes.rs / types-crate | helper |
| 63692 | `instantiateTypeWithAlias` | 2589 (1) | checker: type_nodes.rs / instantiate.rs | helper |
| 64347 | `elaborateJsxComponents` | 2745,2746 (2) | checker: relations.rs (assignableTo heads) | helper |
| 64449 | `elaborateArrayLiteral` | 2418 (1) | checker: relations.rs (assignableTo heads) | helper |
| 64501 | `compareSignaturesRelated` | 2328,2685,2849 (3) | checker: relations.rs | helper |
| 64609 | `compareTypePredicateRelatedTo` | 2518 (1) | checker: relations.rs | helper |
| 64693 | `isEnumTypeRelatedTo` | 2324 (1) | checker: relations.rs | NAMED |
| 64884 | `checkTypeRelatedTo` | 2321,2859 (2) | checker: relations.rs | NAMED |
| 64996 | `reportIncompatibleStack` | 2200,2201,2202,2203,2626,2627 (6) | checker: relations.rs | NAMED |
| 65096 | `reportRelationError` | 2322,2345,2678,2719,2820 (5) | checker: relations.rs | NAMED |
| 65229 | `isRelatedTo` | 2559,2560 (2) | checker: relations.rs | NAMED |
| 65273 | `reportErrorResults` | 2208,2696 (2) | checker: relations.rs | helper |
| 65376 | `hasExcessProperties` | 2326,2339,2353,2551,2561 (5) | checker: relations.rs | NAMED |
| 65752 | `recursiveTypeRelatedTo` | 2321,2859 (2) | checker: relations.rs | NAMED |
| 66670 | `propertyRelatedTo` | 2325,2326,2327,2442,2443,2444 (6) | checker: relations.rs | NAMED |
| 66743 | `reportUnmatchedProperty` | 2741 (1) | checker: relations.rs | NAMED |
| 66784 | `propertiesRelatedTo` | 2339,2618,2619,2620,2621,2623… (10) | checker: relations.rs | NAMED |
| 66960 | `signaturesRelatedTo` | 2322,2419,2517,2658 (4) | checker: relations.rs | NAMED |
| 67057 | `reportIncompatibleCallSignatureReturn` | 2202,2204 (2) | checker: relations.rs | helper |
| 67063 | `reportIncompatibleConstructSignatureReturn` | 2203,2205 (2) | checker: relations.rs | helper |
| 67130 | `membersRelatedToIndexInfo` | 2530 (1) | checker: relations.rs | helper |
| 67192 | `typeRelatedToIndexInfo` | 2329 (1) | checker: relations.rs | helper |
| 67226 | `constructorVisibilitiesAreCompatible` | 2672 (1) | checker: relations.rs | helper |
| 69328 | `getCannotFindNameDiagnosticForName` | 2304,2311,2583,2584 (4) | checker: infer.rs | helper |
| 70238 | `reportFlowControlError` | 2563 (1) | checker: flow.rs | helper |
| 71789 | `markJsxAliasReferenced` | 2874 (1) | checker: flow.rs | helper |
| 72071 | `checkIdentifierCalculateNodeCheckFlags` | 2496,2522,2815 (3) | checker: flow.rs | NAMED |
| 72158 | `checkIdentifier` | 2454,2539,2540,2588,2628,2629… (9) | checker: exprs.rs / contextual.rs | NAMED |
| 72345 | `checkThisInStaticClassFieldInitializerInDecoratedClass` | 2816 (1) | checker: exprs.rs / contextual.rs | helper |
| 72386 | `checkThisExpression` | 2331,2332,2465,2683,2738 (5) | checker: exprs.rs / contextual.rs | NAMED |
| 72535 | `checkSuperExpression` | 2335,2336,2337,2338,2466,2659… (7) | checker: exprs.rs / contextual.rs | NAMED |
| 73714 | `getJsxPropsTypeFromClassType` | 2607 (1) | checker: exprs.rs / contextual.rs | helper |
| 74078 | `checkComputedPropertyName` | 2464 (1) | checker: exprs.rs / contextual.rs | helper |
| 74197 | `checkObjectLiteral` | 2353,2698 (2) | checker: exprs.rs / contextual.rs | NAMED |
| 74423 | `createJsxAttributesTypeFromAttributesProperty` | 2698 (1) | checker: jsx.rs | NAMED |
| 74517 | `checkSpreadPropOverrides` | 2785 (1) | checker: jsx.rs | helper |
| 74552 | `getIntrinsicTagSymbol` | 2339 (1) | checker: jsx.rs | NAMED |
| 74577 | `getJsxNamespaceContainerForImplicitImport` | 2792,2875 (2) | checker: jsx.rs | helper |
| 74639 | `getNameFromJsxElementAttributesContainer` | 2608 (1) | checker: jsx.rs | helper |
| 74665 | `getUninstantiatedJsxSignaturesOfType` | 2339 (1) | checker: jsx.rs | helper |
| 74702 | `checkJsxReturnAssignableToAppropriateBound` | 2787,2788,2789 (3) | checker: jsx.rs | helper |
| 74793 | `checkJsxPreconditions` | 2602 (1) | checker: jsx.rs | helper |
| 74852 | `checkJsxExpression` | 2609 (1) | checker: members.rs / exprs.rs (access) | helper |
| 74882 | `checkPropertyAccessibilityAtLocation` | 2340,2341,2445,2446,2513,2715… (7) | checker: members.rs / exprs.rs (access) | helper |
| 75018 | `reportObjectPossiblyNullOrUndefinedError` | 2531,2532,2533 (3) | checker: members.rs / exprs.rs (access) | helper |
| 75025 | `reportCannotInvokePossiblyNullOrUndefinedError` | 2721,2722,2723 (3) | checker: members.rs / exprs.rs (access) | helper |
| 75037 | `checkNonNullTypeWithReporter` | 2571 (1) | checker: members.rs / exprs.rs (access) | helper |
| 75065 | `checkNonNullNonVoidType` | 2532 (1) | checker: members.rs / exprs.rs (access) | helper |
| 75107 | `checkGrammarPrivateIdentifierExpression` | 2304 (1) | checker: members.rs / exprs.rs (access) | helper |
| 75218 | `checkPropertyAccessExpressionOrQualifiedName` | 2339,2540,2542,2803,2806 (5) | checker: members.rs / exprs.rs (access) | helper |
| 75367 | `getFlowTypeOfAccessExpression` | 2565 (1) | checker: members.rs / exprs.rs (access) | helper |
| 75380 | `checkPropertyNotUsedBeforeDeclaration` | 2449,2729 (2) | checker: members.rs / exprs.rs (access) | helper |
| 75429 | `reportNonexistentProperty` | 2339,2550,2551,2568,2576,2773… (7) | checker: members.rs / exprs.rs (access) | NAMED |
| 75727 | `checkElementAccessExpression` | 2476 (1) | checker: calls.rs | helper |
| 76055 | `checkTypeArguments` | 2344 (1) | checker: calls.rs | helper |
| 76208 | `getSignatureApplicabilityError` | 2345,2684 (2) | checker: calls.rs | NAMED |
| 76272 | `maybeAddMissingAwaitInfo` | 2773 (1) | checker: calls.rs | helper |
| 76438 | `getArgumentArityError` | 2554,2555,2556,2575,2794,2810 (6) | checker: calls.rs | helper |
| 76531 | `getTypeArgumentArityError` | 2558,2743 (2) | checker: calls.rs | helper |
| 76604 | `resolveCall` | 2346,2769,2770,2771,2772,2860 (6) | checker: calls.rs | NAMED |
| 76756 | `addImplementationSuccessElaboration` | 2793 (1) | checker: calls.rs | helper |
| 77015 | `resolveCallExpression` | 2347,2348,2734 (3) | checker: calls.rs | NAMED |
| 77066 | `resolveNewExpression` | 2347,2350,2511,2679 (4) | checker: calls.rs | NAMED |
| 77158 | `isConstructorAccessible` | 2673,2674 (2) | checker: calls.rs | helper |
| 77186 | `invocationErrorDetails` | 2349,2351,2755,2756,2757,2758… (11) | checker: calls.rs | helper |
| 77272 | `resolveTaggedTemplateExpression` | 2796 (1) | checker: calls.rs | helper |
| 77379 | `getJSXFragmentType` | 2879 (1) | checker: calls.rs | helper |
| 77413 | `resolveJsxOpeningLikeElement` | 2558,2604 (2) | checker: jsx.rs (call side) | NAMED |
| 77463 | `resolveInstanceofExpression` | 2359 (1) | checker: jsx.rs (call side) | helper |
| 77641 | `checkCallExpression` | 2775,2776 (2) | checker: jsx.rs (call side) | helper |
| 77743 | `checkImportCallExpression` | 2880 (1) | checker: exprs.rs (import()/EWTA/meta) | NAMED |
| 77950 | `checkAssertionDeferred` | 2352 (1) | checker: exprs.rs (import()/EWTA/meta) | helper |
| 77969 | `checkExpressionWithTypeArguments` | 2848 (1) | checker: exprs.rs (import()/EWTA/meta) | NAMED |
| 77993 | `getInstantiationExpressionType` | 2635 (1) | checker: exprs.rs (import()/EWTA/meta) | helper |
| 78729 | `createPromiseReturnType` | 2697,2705,2711,2712 (4) | checker: exprs.rs / operators.rs | NAMED |
| 79090 | `checkAllCodePathsInNonVoidFunctionReturnOrThrowDiagnostics` | 2355,2366,2534 (3) | checker: exprs.rs / operators.rs | helper |
| 79307 | `checkDeleteExpression` | 2703,2704 (2) | checker: exprs.rs / operators.rs | NAMED |
| 79327 | `checkDeleteExpressionMustBeOptional` | 2790 (1) | checker: exprs.rs / operators.rs | helper |
| 79352 | `checkAwaitGrammar` | 2524,2852,2853,2854 (4) | checker: exprs.rs / operators.rs | NAMED |
| 79455 | `checkPrefixUnaryExpression` | 2356,2357,2469,2736,2777 (5) | checker: exprs.rs / operators.rs | helper |
| 79490 | `checkPostfixUnaryExpression` | 2356,2357,2777 (3) | checker: exprs.rs / operators.rs | helper |
| 79563 | `checkInstanceOfExpression` | 2358,2861 (2) | checker: exprs.rs / operators.rs | NAMED |
| 79604 | `checkInExpression` | 2638 (1) | checker: exprs.rs / operators.rs | NAMED |
| 79646 | `checkObjectLiteralDestructuringPropertyAssignment` | 2462 (1) | checker: exprs.rs / operators.rs | helper |
| 79699 | `checkArrayLiteralDestructuringElementAssignment` | 2462 (1) | checker: exprs.rs / operators.rs | helper |
| 79744 | `checkReferenceAssignment` | 2364,2701,2778,2779 (4) | checker: exprs.rs / operators.rs | helper |
| 79963 | `checkNullishCoalesceOperandLeft` | 2869,2871 (2) | checker: exprs.rs / operators.rs | helper |
| 80055 | `checkBinaryLikeExpressionWorker` | 2362,2363,2447,2695,2791,2839 (6) | checker: exprs.rs / operators.rs | helper |
| 80289 | `checkAssignmentDeclaration` | 2300 (1) | checker: exprs.rs / operators.rs | helper |
| 80303 | `checkForDisallowedESSymbolOperand` | 2469 (1) | checker: exprs.rs / operators.rs | helper |
| 80338 | `checkAssignmentOperatorWorker` | 2364,2779 (2) | checker: exprs.rs / operators.rs | helper |
| 80392 | `reportOperatorError` | 2365 (1) | checker: exprs.rs / operators.rs | helper |
| 80408 | `tryGiveBetterPrimaryError` | 2367 (1) | checker: exprs.rs / operators.rs | helper |
| 80420 | `checkNaNEquality` | 2845 (1) | checker: exprs.rs / operators.rs | helper |
| 80509 | `checkYieldExpressionGrammar` | 2523 (1) | checker: exprs.rs / operators.rs | helper |
| 80530 | `checkTemplateExpression` | 2731 (1) | checker: exprs.rs / operators.rs | helper |
| 80979 | `checkConstEnumAccess` | 2475,2748 (2) | checker: exprs.rs / operators.rs | helper |
| 81138 | `checkTypeParameter` | 2344,2368,2716 (3) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 81156 | `checkTypeParameterDeferred` | 2636,2637 (2) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 81179 | `checkParameter` | 2369,2370,2398,2463,2680,2681… (8) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | NAMED |
| 81228 | `checkTypePredicate` | 2677 (1) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | NAMED |
| 81343 | `checkSignatureDeclarationDiagnostics` | 2505 (1) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 81406 | `addName` | 2300,2804 (2) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 81441 | `checkClassForStaticPropertyNameConflicts` | 2699 (1) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 81467 | `checkObjectTypeForDuplicateDeclarations` | 2300 (1) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 81502 | `checkTypeForDuplicateIndexSignatures` | 2374 (1) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 81589 | `checkConstructorDeclarationDiagnostics` | 2376,2377,2401 (3) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 81639 | `checkAccessorDeclarationDiagnostics` | 2378,2676,2808 (3) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 81697 | `checkTypeArgumentConstraints` | 2344 (1) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | NAMED |
| 81862 | `checkTupleType` | 2574 (1) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | NAMED |
| 81902 | `checkIndexedAccessIndexType` | 2536,2542 (2) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 81972 | `checkInferType` | 2838 (1) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 81991 | `checkImportType` | 2880 (1) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | NAMED |
| 82042 | `checkFlagAgreementBetweenOverloads` | 2383,2384,2385,2512 (4) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 82060 | `checkQuestionTokenAgreementBetweenOverloads` | 2386 (1) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 82098 | `reportImplementationExpectedError` | 2387,2388,2389,2390,2391,2392… (12) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | NAMED |
| 82253 | `checkExportsOnMergedDeclarationsWorker` | 2395,2652 (2) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 82360 | `getPromisedTypeOfPromise` | 2684 (1) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 82489 | `getAwaitedTypeNoAlias` | 2684 (1) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 82534 | `checkAsyncFunctionReturnType` | 2520,2705 (2) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | NAMED |
| 82797 | `checkJSDocTypeAliasTag` | 2457 (1) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 82851 | `checkJSDocThisTag` | 2730 (1) | checker: stmts.rs / classes.rs / grammar.rs (decl shapes) | helper |
| 83190 | `checkPotentialUncheckedRenamedBindingElementsInTypes` | 2843 (1) | checker: stmts.rs / iteration.rs | helper |
| 83235 | `checkCollisionWithArgumentsInGeneratedCode` | 2396 (1) | checker: stmts.rs / iteration.rs | helper |
| 83300 | `checkCollisionWithRequireExportsInGeneratedCode` | 2441 (1) | checker: stmts.rs / iteration.rs | helper |
| 83312 | `checkCollisionWithGlobalPromiseInGeneratedCode` | 2529 (1) | checker: stmts.rs / iteration.rs | helper |
| 83353 | `checkReflectCollision` | 2818 (1) | checker: stmts.rs / iteration.rs | helper |
| 83363 | `checkCollisionsForDeclarationName` | 2414,2431 (2) | checker: stmts.rs / iteration.rs | helper |
| 83394 | `checkVarDeclaredNamesNotShadowed` | 2481 (1) | checker: stmts.rs / iteration.rs | NAMED |
| 83465 | `checkVariableLikeDeclaration` | 2371,2687,2850,2851 (4) | checker: stmts.rs / iteration.rs | helper |
| 83577 | `errorNextVariableOrPropertyDeclarationMustHaveSameType` | 2403,2717 (2) | checker: stmts.rs / iteration.rs | helper |
| 83658 | `helper` | 2774,2801,2845 (3) | checker: stmts.rs / iteration.rs | helper |
| 83756 | `checkTruthinessOfType` | 2872,2873 (2) | checker: stmts.rs / iteration.rs | helper |
| 83845 | `checkForOfStatement` | 2487,2781 (2) | checker: stmts.rs / iteration.rs | helper |
| 83864 | `checkForInStatement` | 2405,2406,2407,2491,2780 (5) | checker: stmts.rs / iteration.rs | NAMED |
| 83926 | `getIteratedTypeOrElementType` | 2763,2764,2765,2766 (4) | checker: stmts.rs / iteration.rs | helper |
| 83979 | `getIterationDiagnosticDetails` | 2461,2495,2548,2549,2802 (5) | checker: stmts.rs / iteration.rs | helper |
| 84258 | `reportTypeNotIterableError` | 2488,2504 (2) | checker: stmts.rs / iteration.rs | helper |
| 84535 | `checkReturnStatement` | 2408,2409 (2) | checker: stmts.rs / iteration.rs | NAMED |
| 84600 | `checkWithStatement` | 2410 (1) | checker: stmts.rs / iteration.rs | NAMED |
| 84693 | `checkTryStatement` | 2492 (1) | checker: stmts.rs / iteration.rs | helper |
| 84749 | `checkIndexConstraintForProperty` | 2411 (1) | checker: stmts.rs / iteration.rs | helper |
| 84789 | `checkClassNameCollisionWithObject` | 2725 (1) | checker: stmts.rs / iteration.rs | helper |
| 84845 | `createCheckTypeParameterDiagnostic` | 2300,2706 (2) | checker: stmts.rs / iteration.rs | helper |
| 84886 | `checkTypeParameterListsIdentical` | 2428 (1) | checker: stmts.rs / iteration.rs | helper |
| 85044 | `checkClassLikeDeclaration` | 2415,2417,2510,2545,2797 (5) | checker: classes.rs | NAMED |
| 85095 | `createImplementsDiagnostics` | 2420,2422,2720 (3) | checker: classes.rs | helper |
| 85247 | `issueMemberSpecificError` | 2416 (1) | checker: classes.rs | helper |
| 85276 | `checkBaseTypeAccessibility` | 2675 (1) | checker: classes.rs | helper |
| 85371 | `checkKindsOfPropertyMemberOverrides` | 2423,2425,2426,2515,2612,2653 (6) | checker: classes.rs | helper |
| 85464 | `checkInheritedPropertiesAreIdentical` | 2319,2320 (2) | checker: classes.rs | helper |
| 85492 | `checkPropertyInitialization` | 2564 (1) | checker: classes.rs | NAMED |
| 85532 | `checkInterfaceDeclaration` | 2427,2430 (2) | checker: classes.rs | NAMED |
| 85563 | `checkTypeAliasDeclaration` | 2457,2795 (2) | checker: classes.rs | helper |
| 85598 | `computeEnumMemberValue` | 2452 (1) | checker: classes.rs | helper |
| 85640 | `computeConstantEnumMemberValue` | 2474,2477,2478 (3) | checker: classes.rs | helper |
| 85742 | `evaluateEnumMember` | 2565,2651 (2) | checker: classes.rs | helper |
| 85786 | `checkEnumDeclarationWorker` | 2432,2473 (2) | checker: classes.rs | helper |
| 85853 | `checkModuleDeclarationDiagnostics` | 2433,2434,2435,2436,2669,2670 (6) | checker: classes.rs | NAMED |
| 85934 | `checkModuleAugmentationElement` | 2666,2667 (2) | checker: classes.rs | helper |
| 86002 | `checkExternalImportOrExportDeclaration` | 2439,2837,2858 (3) | checker: classes.rs | helper |
| 86071 | `checkAliasSymbol` | 2440,2484,2748,2865 (4) | checker: classes.rs | helper |
| 86193 | `checkImportAttributes` | 2821,2822,2823,2836,2856,2857… (7) | checker: stmts.rs (import attrs/export=) | NAMED |
| 86256 | `checkImportDeclaration` | 2882 (1) | checker: stmts.rs (import attrs/export=) | NAMED |
| 86286 | `checkImportEqualsDeclaration` | 2437,2438 (2) | checker: stmts.rs (import attrs/export=) | helper |
| 86381 | `checkExportSpecifier` | 2661 (1) | checker: stmts.rs (import attrs/export=) | NAMED |
| 86492 | `checkExportAssignment` | 2714 (1) | checker: stmts.rs (import attrs/export=) | NAMED |
| 86513 | `checkExternalModuleExports` | 2309,2323 (2) | checker: stmts.rs (import attrs/export=) | helper |
| 88746 | `initializeTypeChecker` | 2300,2397,2451 (3) | checker: driver.rs | NAMED |
| 88922 | `checkExternalEmitHelpers` | 2343,2807 (2) | checker: driver.rs | NAMED |
| 89006 | `resolveHelpersModule` | 2354 (1) | checker: driver.rs | helper |
| 89644 | `checkGrammarObjectLiteralExpression` | 2300,2501 (2) | transform/emit region — audit individually | helper |
| 89750 | `checkGrammarJsxName` | 2633,2639 (2) | transform/emit region — audit individually | helper |
| 89836 | `checkGrammarForInOrForOfStatement` | 2404,2483 (2) | transform/emit region — audit individually | helper |
| 90027 | `checkGrammarBindingElement` | 2462,2566 (2) | transform/emit region — audit individually | helper |
| 90118 | `checkGrammarNameInLetOrConstDeclarations` | 2480 (1) | transform/emit region — audit individually | helper |
| 90362 | `checkGrammarBigIntLiteral` | 2737 (1) | transform/emit region — audit individually | helper |
| 90423 | `checkGrammarNamedImportsOrExports` | 2206,2207 (2) | transform/emit region — audit individually | helper |
| 114386 | `reportInaccessibleUniqueSymbolError` | 2527 (1) | transform/emit region — audit individually | helper |
| 114396 | `reportInaccessibleThisError` | 2527 (1) | transform/emit region — audit individually | helper |
| 114402 | `reportLikelyUnsafeImportRequiredError` | 2742,2883 (2) | transform/emit region — audit individually | helper |
| 123752 | `getMergedBindAndCheckDiagnostics` | 2578 (1) | program layer (§2b / filter) | NAMED |
| 124513 | `processTypeReferenceDirectiveWorker` | 2688 (1) | program layer (§2b / filter) | NAMED |
| 125846 | `filePreprocessingLibreferenceDiagnostic` | 2726,2727 (2) | program layer (§2b / filter) | NAMED |
