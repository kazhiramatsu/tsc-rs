# M4 stage 5.5: expression checking, non-call arms — semantic extraction

Companion to `m4-checker-skeleton-steps.md` §5.5. Extracted 2026-07-12
from `tsrs2/vendor/typescript-6.0.3/lib/_tsc.js` (the oracle bundle) by
a 6-way parallel source audit; every line number and diagnostic code
below was re-verified by the orchestrator against the bundle. The tsc
source at the cited lines stays the transcription source of truth —
port with it open side-by-side. Conventions: `LNNNNN` = bundle line;
`→ NNNN` = diagnostic code (verified `Name: diag(NNNN` in the bundle).

Scope refresher (steps doc): checkExpressionWorker (L81011) dispatch in
tsc order, contextual typing arriving with the stage, binary trampoline
mandatory, call resolution 5.7, flow narrowing M5, inference M6.

## §0 Stage-boundary stub policy (the tags)

Each tag marks machinery another stage owns. The 5.5 port wires call
sites to the stub named here; the owning stage un-stubs. All stubs get
ledger rows.

- **[FLOW] → M5.** `getFlowTypeOfReference` (L70394): ONE stub function
  `get_flow_type_of_reference_stub(reference, declared, initial,
  container) -> declared` — M5 replaces exactly this function. Call
  sites at 5.5: checkIdentifier L72199, tryGetThisTypeAt L72433/L72450
  (2-arg form), getFlowTypeOfAccessExpression L75365, element-read
  L62238, getFlowTypeOfProperty L56222 (via L75348),
  getFlowTypeOfDestructuring L55892, getNarrowedTypeOfSymbol L72001.
  Also M5: `isPostSuperFlowNode` (checkThisBeforeSuper → stub `true`,
  17009/17011 become FN), `functionHasImplicitReturn`/
  `isReachableFlowNode` (→ checkAllCodePathsInNonVoidFunctionReturn-
  OrThrow L79075 stubs out ENTIRELY: FN on 2355/2366/2534/7030),
  `isSymbolAssigned`/`isPastLastAssignment` family (flow-container
  widening loop L72193 — stub the loop out, keep flowContainer as
  computed).
- **[FACTS] → port AT 5.5 (scope refinement).** The steps doc said
  "identity-stub, M5"; the extraction shows that is not FP-safe: the
  2531/2532/2533/18047-18049 SELECTION (L75040), the `!`/`&&`/`||`/`??`
  RESULT types (L79461, L80221, L80229, L80237), destructuring-default
  undefined-stripping (L79688, L79718), and getNonNullableType (L67869)
  all read fact bits. `getTypeFacts`/`getTypeFactsWorker` (L69697/…),
  `hasTypeFacts` (L69700), `getTypeWithFacts` (L69781),
  `getAdjustedTypeWithFacts` (L69784) are pure type→bitmask classifiers
  and filters with NO flow dependency — port them at 5.5d. What stays
  M5 is narrowing itself. Bits used this stage: Truthy 4194304, Falsy
  8388608, EQUndefinedOrNull 262144, NEUndefined 524288,
  NEUndefinedOrNull 2097152, IsUndefined 16777216, IsNull 33554432,
  IsUndefinedOrNull 50331648.
- **[WIDEN] → 5.6, with a literal-level carve-out ported AT 5.5.**
  Literal-level (pure classifiers, no widening context):
  `getBaseTypeOfLiteralType` L67755, `getBaseTypeOfLiteralTypeForComparison`
  L67762 (NO enum-like arm; Enum 65536 → number), `getWidenedLiteralType`
  L67765, `getWidenedUniqueESSymbolType` L67768,
  `getWidenedLiteralLikeTypeForContextualType` L67771 (+ the
  contextual-iteration/return variants L67777/L67784 — generator arms
  ride [ITER]). Object-level stays 5.6: `getWidenedType` L68013,
  `getRegularTypeOfObjectLiteral` L67923, `reportErrorsFromWidening`
  L68187, `reportImplicitAny`. 5.5 call sites of the 5.6 functions get
  identity stubs + ledger: receiver widening L75204/L75720,
  checkAssertionDeferred L77939 (getRegularTypeOfObjectLiteral +
  getWidenedType inside the 2352 lazy closure — pin literal-assertion
  fixtures; pull the two functions forward if pins diverge),
  getReturnTypeFromBody widening tail L78807-78830.
- **[ITER] → 5.8** (steps doc: getIterationTypesOfIterable ports there).
  5.5 sites escape Unsupported per-element: checkSpreadExpression
  L73944, checkArrayLiteral non-array-like spread L73993, destructuring
  L73981(silent)/L79672, yield (checkYieldExpression + aggregation),
  generator arms of contextual return/yield. LOAD-BEARING FAST PATH
  that keeps common code working WITHOUT [ITER]: checkArrayLiteral
  L73978 `isArrayLikeType(spreadType)` (L67680: isArrayType ‖
  non-nullable && assignable-to `readonly any[]`) → Variadic element,
  no protocol needed. Array/tuple spreads work at 5.5; Set/generator
  spreads escape.
- **[CALLS] → 5.7.** Worker arms CallExpression/NewExpression/
  TaggedTemplateExpression → Unsupported escape (per-element
  containment). `getContextualTypeForArgumentAtIndex` L72911 → None,
  EXCEPT its isImportCall arm (argIndex 0 → stringType, 1 →
  ImportCallOptions global, else any) which is self-contained — port.
  `getResolvedSignature`-reaching JSX/instanceof paths: run the ported
  prefix, escape at the sig call (see §8/§10). checkImportCallExpression
  L77718 / checkTaggedTemplateExpression L77854: grammar + escape.
- **[INFER] → M6.** `getInferenceContext` returns None until M6 (no
  inference stack exists). `instantiateTypeWithSingleGenericCallSignature`
  L80751: its step-1 gate `checkMode & (Inferential|SkipGenericFunctions)`
  is UNREACHABLE at M4 (no producer sets those bits — verified CheckMode
  audit below) → port the gate returning `type`, `unreachable!()` the
  tail. assignContextualParameterTypes' infer branches (L79166-79173)
  take the `inferenceContext == None` path naturally.
- **[JSDOC]/JS** — follow the standing policy (M2 3.4c, plain-JS band,
  check_js gate): JS-only arms skip/escape with ledger. Includes
  expando/assignment-declaration arms (onEnter L79839, arm J's declKind
  routing — kind 0 None is the TS path), JS literal types, JSDoc type
  tags.
- **Grammar checks**: port the ones that EMIT band-relevant error codes
  or GATE control flow; defer pure-modifier walks to 5.8/M7 with FN
  ledger. Port at 5.5: checkGrammarBigIntLiteral (2737, bool ignored),
  checkGrammarPrivateIdentifierExpression (18016/1451/2304),
  checkAwaitGrammar (lazy=eager; 1308/1375/1378/2524/18037 family),
  checkGrammarTaggedTemplateChain (1358, GATES), checkNullishCoalesce-
  Operands (5076 — not a checkGrammar* fn), checkGrammarJsxElement
  (2633/2639/17000/17001) + checkGrammarJsxExpression (18007),
  checkGrammarStatementInAmbientContext (expression-statement head),
  checkGrammarFunctionLikeDeclaration (GATES checkGrammarForGenerator),
  checkGrammarMetaProperty (17012/18061/1005), 7059/1294 assertion-file
  checks. Side-effect-only walks that emit nothing the corpus exercises
  (checkGrammarObjectLiteralExpression duplicate-`__proto__` etc.) may
  defer — inventory each with a ledger row at landing.
- **addLazyDiagnostic = eager identity** (5.4 decision, L87104 band) —
  everything tsc defers (checkAssignmentOperatorWorker, awaitGrammar,
  2352 closure, regex grammar, 7057 closure) runs inline for us; order
  inside one element is tsc's lazy-drain order only if it matters to
  dedupe — it does not: sort_and_dedupe handles final order.

## §1 What forces expressions at 5.5 (the forcing map)

- `checkExpressionStatement` L83622 = checkGrammarStatementInAmbientContext
  + checkExpression(node.expression). Un-stub check.rs:228. This is the
  ONLY new eager driver arm at 5.5.
- Deferred drain (5.4 driver, check.rs check_deferred_node): 5.5
  registrations replace the `unreachable!()`s — function/arrow/method
  bodies (checkFunctionExpressionOrObjectLiteralMethodDeferred, incl.
  MethodSignature 174 in tsc's kind list L86930), ClassExpression,
  JSX element/self-closing (fragment is EAGER, L74324), TypeAssertion/
  As/Paren → checkAssertionDeferred, VoidExpression → checkExpression
  of operand (L86957), BinaryExpression-instanceof → resolveUntypedCall
  [CALLS] stays unreachable until 5.7. Object-literal ACCESSORS also
  register (checkObjectLiteral L74245 → checkNodeDeferred) — the
  GetAccessor/SetAccessor deferred arm routes to checkAccessorDeclaration
  [5.8-DECL]: register + escape.
- Variable initializers do NOT force at 5.5: checkVariableStatement is
  a 5.8 stub AND annotate.rs's initializer arm (6827) stays escaped —
  see §12 sequencing. Statements (if/return/for…) stay 5.8. The rate
  wave arrives at 5.7/5.8; expect a modest bump now.
- instantiation_count: checkExpression resets it (L80965) — the third
  and last reset point (state.rs:219 note closes).

## §2 Dispatch + driver band (L80557-81127)

### CheckMode (L46386) — flags.rs audit
Normal 0, Contextual 1, Inferential 2, SkipContextSensitive 4,
SkipGenericFunctions 8, IsForSignatureHelp 16, RestBindingElement 32,
TypeOnly 64. At M4 the producible set is {0,1,4,32,64}: Contextual from
checkExpressionWithContextualType (inferenceContext always None → no
Inferential), SkipContextSensitive from getContextFreeTypeOfExpression,
TypeOnly from getTypeOfExpression, RestBindingElement from
getContextualTypeForBindingElement. Verify flags.rs has all 8.

### checkExpression L80960-80974 — THE wrapper
currentNode save/restore; `instantiationCount = 0`; worker; higher-order
tail (`instantiateTypeWithSingleGenericCallSignature` — dead gate at M4,
§0 [INFER]); `isConstEnumObjectType(type)` → checkConstEnumAccess
L80975 (→ 2475 illegal position; 2748 ambient-const-enum under
isolatedModules/verbatimModuleSyntax — note the precedence
`isolatedModules || (verbatim && ok && !resolveName(...))`, and 2475
does NOT early-return). NO alias/links handling in 6.0.3 — do not
invent one.

### checkExpressionWorker L81011-81127 — master switch (complete)
Cancellation pre-check for 232/219/220 (skip — no cancellation).
Default → errorType (L81126).

| kind | arm | target |
|---|---|---|
| Identifier 80 | checkIdentifier(node, checkMode) | L72126 |
| PrivateIdentifier 81 | checkPrivateIdentifierExpression | L75112 |
| ThisKeyword 110 | checkThisExpression | L72348 |
| SuperKeyword 108 | checkSuperExpression | L72509 |
| NullKeyword 106 | inline `nullWideningType` (L47050) | — |
| StringLiteral 11, NoSubstitutionTemplate 15 | skipDirectInference? blockedStringType : fresh(getStringLiteralType(text)) | L68509/L47020 |
| NumericLiteral 9 | checkGrammarNumericLiteral (suggestion 80008 only — skip) ; fresh(number literal +text) | L90342 |
| BigIntLiteral 10 | checkGrammarBigIntLiteral (→ 2737 error, bool ignored); fresh(bigint {negative:false, base10Value: parsePseudoBigInt}) | L90358 |
| TrueKeyword 112 / FalseKeyword 97 | fresh trueType/falseType singletons | L47062/L47054 |
| TemplateExpression 229 | checkTemplateExpression | L80524 |
| RegularExpressionLiteral 14 | checkRegularExpressionLiteral: once-flag on links (TypeChecked bit), lazy regex grammar, return globalRegExpType | L73931 |
| ArrayLiteral 210 | checkArrayLiteral(node, checkMode, forceTuple) | L73956 |
| ObjectLiteral 211 | checkObjectLiteral(node, checkMode) | L74135 |
| PropertyAccess 212 | checkPropertyAccessExpression | L75069 |
| QualifiedName 167 | checkQualifiedName | L75077 |
| ElementAccess 213 | checkIndexedAccess (chain split wrapper) | L75711 |
| CallExpression 214 | isImportCall → checkImportCallExpression [CALLS]; else FALLS THROUGH | L77718 |
| NewExpression 215 | checkCallExpression(node, checkMode) [CALLS→5.7 escape] | L77607 |
| TaggedTemplate 216 | checkTaggedTemplateExpression [CALLS→5.7 escape] | L77854 |
| Parenthesized 218 | checkParenthesizedExpression (JSDoc arms; else recurse) | L81000 |
| ClassExpression 232 | checkClassExpression | L84972 |
| FunctionExpression 219 / ArrowFunction 220 | checkFunctionExpressionOrObjectLiteralMethod | L79109 |
| TypeOfExpression 222 | checkTypeOfExpression → typeofType | L79330 |
| TypeAssertion 217 / AsExpression 235 | checkAssertion | L77863 |
| NonNullExpression 236 | checkNonNullAssertion | L77960 |
| ExpressionWithTypeArguments 234 | checkExpressionWithTypeArguments | L77963 |
| SatisfiesExpression 239 | checkSatisfiesExpression | L78047 |
| MetaProperty 237 | checkMetaProperty | L78061 |
| DeleteExpression 221 | checkDeleteExpression | L79303 |
| VoidExpression 223 | checkVoidExpression: checkNodeDeferred + return undefinedWideningType | L79334 |
| AwaitExpression 224 | checkAwaitExpression | L79408 |
| PrefixUnary 225 / PostfixUnary 226 | checkPrefix/PostfixUnaryExpression | L79427/L79482 |
| BinaryExpression 227 | checkBinaryExpression(node, checkMode) — trampoline var L46480 | L79810 |
| Conditional 228 | checkConditionalExpression | L80513 |
| SpreadElement 231 | checkSpreadExpression | L73939 |
| OmittedExpression 233 | inline undefinedWideningType | — |
| YieldExpression 230 | checkYieldExpression | L80447 |
| SyntheticExpression 238 | checkSyntheticExpression: isSpread? indexedAccess(type, number) : type | L73946 |
| JsxExpression 295 | checkJsxExpression | L74847 |
| JsxElement 285 / JsxSelfClosing 286 / JsxFragment 289 / JsxAttributes 293 | §10 | L74320/74307/74324/74522 |
| JsxOpeningElement 287 | Debug.fail | — |

Gotchas: checkMode reaches only Call/New via fallthrough among [CALLS]
arms; forceTuple reaches only ArrayLiteral; NonNullExpression arm is
`checkNonNullAssertion` L77960 (verified): `node.flags & 64
OptionalChain ? checkNonNullChain(node) :
getNonNullableType(checkExpression(node.expression))` — note it does
NOT report (no checkNonNullType): `x!` strips silently via [FACTS]
getNonNullableType; checkNonNullNonVoidType L75051's only consumers
are checkVariableLikeDeclaration L83479/83488 (5.8), not this arm
(§6 corrected 2026-07-12; earlier "AWAIT-band consumer" was wrong too).

### Driver-band helpers (L80557-80959) — all port at 5.5
- checkExpressionCached L80580: checkMode truthy → NO cache; else
  nodeLinks.resolvedType with flowLoopStart/flowTypeCache save-reset-
  restore (flow vars exist as plain fields; M5 gives them meaning —
  wire the save/restore shape NOW, the M5 fixpoint edits expect it).
- checkDeclarationInitializer L80604 (JSDoc satisfies arm [JSDOC];
  getQuickTypeOfExpression ‖ contextual/cached; parameter binding-
  pattern padding: padObjectLiteralType L80629 / padTupleType L80665 —
  padTuple reportImplicitAny call is [WIDEN]→5.6 stub).
- checkExpressionForMutableLocation L80724: isConstContext ‖
  isCommonJsExportedExpression [JSDOC] → getRegularTypeOfLiteralType;
  isTypeAssertion → unchanged; else getWidenedLiteralLikeTypeFor-
  ContextualType(type, instantiateContextualType(getContextualType(…))).
- checkPropertyAssignment L80737 (computed-name check first),
  checkObjectLiteralMethod L80743 (checkGrammarMethod side-effect →
  defer w/ ledger; higher-order tail).
- isConstContext L80720, isLiteralOfContextualType L80706 (the
  fresh-keep test), isTypeAssertion L80596.
- getTypeOfExpression L80895: getQuickTypeOfExpression fast path;
  TypeCached/flowTypeCache read (dormant until M5 — port shape);
  checkExpression(node, TypeOnly 64); flowInvocationCount comparison
  gates cache write (dormant).
- getQuickTypeOfExpression L80915: JSDoc-assertion arm [JSDOC]; await
  recursion → getAwaitedType; non-super/require/Symbol/import CALL arm →
  getReturnTypeOfSingleNonGenericCallSignature(checkNonNullExpression…)
  (+ call-chain variant L80889 w/ optional propagation); assertion arm →
  type-node; literal/boolean → checkExpression(node). Port whole.
- getContextFreeTypeOfExpression L80945: links.contextFreeType cache +
  pushContextualType(node, anyType, isCache=false) + CheckMode 4.
- instantiateTypeWithSingleGenericCallSignature L80751: dead gate (§0).

## §3 Identifier / this / super

### checkIdentifier L72126 (+ calc-flags L72063, TDZ L48448)
Port order of operations EXACTLY:
1. isThisInTypeQuery → checkThisExpression.
2. getResolvedSymbol L69389 (links.resolvedSymbol; resolveName with
   getCannotFindNameDiagnosticForName — resolve.rs already has the
   table) ; unknownSymbol → errorType.
3. checkIdentifierCalculateNodeCheckFlags L72063: arguments-symbol arms
   (2815 property-initializer/static-block; ES5 2496 arrow / 2522
   async; CaptureArguments 512 chain), deprecation hooks (suggestions —
   skip), ConstructorReference class-self-reference NodeCheckFlags
   (emit-era bookkeeping: WRITE the flags, they're inert),
   checkNestedBlockScopedBinding (ES5 loop-capture NodeCheckFlags —
   languageVersion ≥ ES2015 skips; port the gate + skip).
4. arguments arm: property-initializer/static-block → errorType; else
   getTypeOfSymbol(argumentsSymbol) — globals.rs arguments_symbol_type
   is ready (IArguments via libs).
5. shouldMarkIdentifierAliasReferenced L72219 + markLinkedReferences
   (alias-reference marking — 5.8/M7 alias band: no-op stub + ledger).
6. contextualBindingPatterns circularity → nonInferrableAnyType
   (L72151; the stack lives in getTypeFromBindingPattern — B band).
7. getNarrowedTypeOfSymbol L72001 → getTypeOfSymbol + dependent-
   destructuring arm (guarded by NodeCheckFlags InCheckIdentifier
   4194304; needs [FLOW] — stub the arm to plain getTypeOfSymbol).
8. assignment-target mutability (getAssignmentTargetKind L15580 =
   AssignmentKind 0/1/2): non-Variable → 2628 enum / 2629 class / 2631
   namespace / 2630 function / 2632 import / 2539 not-a-variable
   (JS ValueModule exemption [JSDOC]); isReadonlySymbol L79253 →
   Variable? 2588 : 2540; all → errorType. Message arg uses
   symbolToString(symbol) but flag test uses localOrExportSymbol.
9. Variable + Definite(1) → early return (isInCompoundLikeAssignment ?
   getBaseTypeOfLiteralType : type) — BEFORE flow. Alias → declaration
   swap. Non-variable non-alias → return type (functions/classes/
   enums/modules exit; no flow).
10. getNarrowableTypeForReference L71640 (constraint substitution +
    NoInfer unwrap; checkMode Inferential disables — port, mostly
    inert at M4).
11. Flow-container computation (getControlFlowContainer L71477) +
    widening loop L72193 ([FLOW] helpers → stub loop out),
    assumeInitialized ladder L72197 (SYNTACTIC — port verbatim),
    initialType L72198 (removeOptionalityFromDeclaredType uses [FACTS]
    NEUndefined — available per §0).
12. THE SEAM L72199: flowType = get_flow_type_of_reference_stub(node,
    type, initialType, flowContainer) = type.
13. auto-type arm L72200 (7034+7005): dormant until autoType exists
    (5.6/M5 evolving types) — port the branch, it never fires with the
    stub (flowType==type, autoType unconstructed).
14. 2454 arm L72208: `!containsUndefinedType(type) &&
    containsUndefinedType(flowType)` — always false under the stub
    (flowType==type). Port verbatim; M5 activates.
15. return assignmentKind ? getBaseTypeOfLiteralType(flowType) : flowType.

TDZ checkResolvedBlockScopedVariable L48448 (called from resolveName's
result path — resolve.rs owns the callsite): 2448 var / 2449 class /
2450 enum (const enum only under isolatedModules) + relatedInfo 2728;
ambient exempt; isBlockScopedNameDeclaredBeforeUse exists (evaluate.rs
slice — un-escape the shared walker rows as consumers arrive).

### checkThisExpression L72348
Container walk with arrow/computed-name toggling loop (L72362 — the two
flags interlock; transcribe exactly). Emissions: 17009 (super-before-
this, [FLOW] isPostSuperFlowNode → stub true = FN), 2816 (static prop
initializer in decorated class), 2465 (computed property name), 2331
(module body), 2332 (enum body), 7041/2683+2738 under noImplicitThis
(tryGetThisTypeAt undefined path). captureLexicalThis ES5 NodeCheckFlags.
tryGetThisTypeAt L72422: function-like arm (getThisTypeOfDeclaration ‖
[JSDOC] arms ‖ getContextualThisParameterType) + [FLOW] wrap; class arm
(static → getTypeOfSymbol, instance → declaredType.thisType) + [FLOW]
wrap; SourceFile arm (commonjs [JSDOC]; external module → undefinedType;
includeGlobalThis → globalThisSymbol type). Errors do NOT return
errorType — result is `type || anyType`.

### checkSuperExpression L72509
Legality matrix (isLegalUsageOfSuperExpression L72597): super-call →
Constructor only; static member access → method/accessor/prop-decl/
static-block; instance → + PropertySignature/Constructor. Emissions:
2466 computed-name, 2337 super-call-position, 2660 outside-derived,
2338 property-access-position, 17011 (super-before-super-prop, [FLOW]
stub → FN), 2659 object-literal-ES5, 2335 no-extends, 2336 constructor-
arg-initializer. Object-literal ES2015+ → anyType. extends null →
nullWideningType (non-call). NodeCheckFlags (SuperStatic 32 /
SuperInstance 16, ContainsSuperPropertyInStaticInitializer 2097152
walk, MethodWithSuperPropertyAccess/AssignmentInAsync 128/256) are
written BEFORE later error returns — order matters. Result: static →
getBaseConstructorTypeOfClass(classType) (5.3e exists); instance →
getTypeWithThisArgument(baseType, classType.thisType) (5.3 exists).

### Literals
Fresh types via getFreshTypeOfLiteralType L63066 (tables have
fresh/regular from M3). blockedStringType arm (skipDirectInference
links flag) is [INFER]-fed — port the read; never set at M4.
checkQualifiedName L75077: typeof-this left → checkThisExpression +
checkNonNullType; else checkNonNullExpression; → shared worker (§6).

## §4 Contextual typing (L72612-73955 + stacks)

ContextFlags (inlined const enum): None 0, Signature 1, NoConstraints 2,
IgnoreNodeInferences 4 (TS6 rename of Completions), SkipBindingPatterns
8 — flags.rs already carries these; verify member names.

State: FOUR parallel-array stacks on checker state —
contextualTypeNodes/contextualTypes/contextualIsCache (+count) L47404-;
inferenceContextNodes/… [INFER, exists-but-empty until M6];
contextualBindingPatterns L47408 (pushed by getTypeFromBindingPattern
L56546 under includePatternInType); flowTypeCache/flowLoopStart [M5].
push L73569 / pop L73575 / findContextualNode L73581 (innermost-first
identity scan; `includeCaches = !contextFlags`) /
pushCachedContextualType L73557 (isCache=true — literal checkers).

checkExpressionWithContextualType L80557: getContextNode (JSX
attributes → grandparent element), push real entry, pushInferenceContext
(None), checkExpression(node, checkMode | Contextual 1 | (ctx?
Inferential : 0)), fresh→regular via isLiteralOfContextualType +
getRegularTypeOfLiteralType, pop both. Consumer at 5.5:
checkDeclarationInitializer only (arg-checking consumers are 5.7).

getContextualType L73471: InWithStatement guard → None; pushed-entry
priority read; parent-kind switch — arm inventory + dispositions:

| parent | helper (line) | 5.5 disposition |
|---|---|---|
| VariableDecl 261 / Parameter 170 / PropertyDecl 173 / PropertySig 172 / BindingElement 209 | getContextualTypeForInitializerExpression L72757 | port; Parameter→getContextuallyTypedParameterType L72687 (IIFE arm [CALLS] → skip w/ resolvedSignature=anySignature save/restore shape; contextual-signature arm live); BindingElement recursion L72736 (RestBindingElement 32; array→ForElementExpression 2-arg, object→getTypeOfPropertyOfType); SkipBindingPatterns gate → getTypeFromBindingPattern L56546 |
| Arrow 220 (concise) / Return 254 | ForReturnExpression L72776 | port; generator filter [ITER] escape; async arm → getAwaitedTypeNoAlias + createPromiseLikeType = `T ‖ PromiseLike<T>` (awaited family §9 lands same stage) |
| Yield 230 | ForYieldOperand L72810 (parent passed) | [ITER] → None + ledger |
| Await 224 | ForAwaitOperand L72802 (parent passed) | port (awaited family) |
| Call 214 / New 215 | getContextualTypeForArgument L72906 [CALLS] | None until 5.7 EXCEPT isImportCall arm (port) |
| Decorator 171 | ForDecorator L72925 [CALLS] | None until 5.7 |
| Assertion 217/235 | const? recurse : getTypeFromTypeNode | port |
| Binary 227 | ForBinaryOperand L72935 | port: `=`-family → assignment-declaration analysis L72980 (JS kinds [JSDOC]-slice: kind 0 None → getTypeOfExpression(left); kind 4/5 this-property arms port their TS-visible shape); `‖`/`??` → RHS gets LHS type when `type.pattern` ‖ no-context-and-not-expando; `&&`/comma → RHS gets outer context |
| PropertyAssignment 304 / Shorthand 305 | ForObjectLiteralElement L73211 | port (getTypeOfPropertyOfContextualType L73112 — intersection walk w/ any→unknown laundering L73163, mapped-substitution arm via substituteIndexedMappedType M8-escape? NO: getIndexedMappedTypeSubstitutedTypeOfContextualType L73166 needs mapped machinery → escape that sub-arm M8, concrete+index-info arms port) |
| SpreadAssignment 306 | recurse to literal's parent | port |
| ArrayLiteral 210 | ForElementExpression L73258 via apparent + nodeLinks.spreadIndices cache (getSpreadIndices L73248) | port; tuple slicing arms (getElementTypeOfSliceOfTupleType L67820 exists since 5.3c); QUIRK L73264: `elementFlags[index] && 2` — literal `&&` in the bundle (not `&`) → isOptional effectively always true; TRANSCRIBE VERBATIM; non-tuple fallback: numeric-name property THEN getIteratedTypeOrElementType(Element 1, silent) [ITER]→None; `!firstSpreadIndex` falsy-test quirk (index 0 spread ≡ no spread) |
| Conditional 228 | branch operands recurse; condition → None | port |
| TemplateSpan 240 | tagged → ForArgument [CALLS] | None until 5.7 |
| Parenthesized 218 | JSDoc arms [JSDOC]; else recurse | port |
| NonNull 236 | recurse | port |
| Satisfies 239 | getTypeFromTypeNode(parent.type) | port |
| ExportAssignment 278 | tryGetTypeFromEffectiveTypeNode [JSDOC] | None |
| JsxExpression 295 / JsxAttribute 292/294 / Jsx opening 287/286 | L73321/L73325/L73635 | §10; opening → ForArgumentAtIndex fallback [CALLS] (quirk: `contextFlags !== 4` STRICT compare, not bitmask) |
| ImportAttribute 302 | getGlobalImportAttributesType | port (lib-backed) |

getApparentTypeOfContextualType L73424: objectLiteralMethod split;
instantiateContextualType L73441 ([INFER] mappers — None path =
identity; port structurally); NoConstraints + TypeVariable → None;
mapType(t => Mapped 32 ? t : getApparentType(t)) — mapped types NOT
apparent-ified (deliberate, L73430 comment); union + object-literal →
discriminateContextualTypeByObjectMembers L73357 (cachedTypes map keyed
`D{nodeId},{typeId}`; getMatchingUnionConstituentForObjectLiteral
L69635 + discriminateTypeByDiscriminableItems L67259 — M3 relation kit;
isPossiblyDiscriminantValue L73336 inventory); union + jsx-attributes →
JSX twin L73391.

getContextualSignature L73851: JSDoc type-tag [JSDOC]; apparent w/
Signature flag; non-union → getContextualCallSignature L73830
(arity-filter isAritySmaller L73835 + getIntersectedSignatures L73741
noImplicitAny-only); union → per-constituent + compareSignaturesIdentical
(partialMatch=false, ignoreThisTypes=true, ignoreReturnTypes=true) —
ANY mismatch → None; createUnionSignature (5.3d exists).
getContextualSignatureForFunctionLikeDeclaration L73848.
getContextualReturnType L72874 (annotation → contextual signature →
IIFE [CALLS-lite: getContextualType of the call — port]; generator/
async filters [ITER]/awaited).

## §5 Literals band (array/object/spread)

### checkArrayLiteral L73956-74033 + createArrayLiteralType L74034
pushCachedContextualType/pop discipline; inDestructuringPattern =
isAssignmentTarget; isConstContext; contextual apparent; inTupleContext
detection L73969 (isSpreadIntoCallOrNew; someType isTupleLikeType ‖
non-nameType homomorphic mapped [M8-escape that disjunct w/ ledger —
generic mapped]). Element loop arms: spread (emit-helper gates skip;
forceTuple THREADS into operand; array-like fast path → Variadic 8;
destructuring → getIndexTypeOfType(number) ‖ silent [ITER] ‖ unknown,
Rest 4; else [ITER] w/ errorNode → escape); omitted-under-
exactOptionalPropertyTypes → undefinedOrMissingType Optional 2 (plain
omitted → else-arm as undefinedWideningType); plain →
checkExpressionForMutableLocation + addOptionality(hasOmitted) — flags
Optional after ANY omission + [INFER] intra-expression site (dead).
Exits: destructuring → createTupleType RAW (no ArrayLiteral stamp!);
forceTuple/const/tuple-context → createArrayLiteralType(createTupleType(
elementTypes, elementFlags, readonly = inConstContext && !contextual-
mutable-array)); else createArrayLiteralType(createArrayType(union of
elements w/ Subtype reduction 2; Variadic entries unwrap via
getIndexedAccessTypeOrUndefined(t, number) ‖ any; empty →
strictNullChecks? implicitNeverType : undefinedWideningType)).
createArrayLiteralType: Reference-flagged only; type.literalType clone
cache + ArrayLiteral 16384 | ContainsObjectOrArrayLiteral 131072.

### checkObjectLiteral L74135-74299 (+ helpers)
Full extraction facts that MUST survive transcription:
- checkGrammarObjectLiteralExpression side-effect (defer w/ ledger).
- allPropertiesTable only under strictNullChecks (spread-override 2783
  + related 2785 via checkSpreadPropOverrides L74511).
- pushCachedContextualType; contextualTypeHasPattern (pattern back-
  channel: result.pattern set at L74295 when destructuring target).
- Pre-pass forces checkComputedPropertyName on every computed name
  L74159 (cache on the EXPRESSION node's links, L74062; `[K in T]`
  recovery arm → errorType silently; 2464 legality: Nullable-first,
  then StringLike|NumberLike|ESSymbolLike kind ‖ assignable to
  stringNumberSymbolType).
- Member loop: PropertyAssignment → checkPropertyAssignment; Shorthand
  → checkExpressionForMutableLocation(name) (objectAssignmentInitializer
  only outside destructuring — recovery semantics); method →
  checkObjectLiteralMethod; [JSDOC] enum-tag/jsDocType arms skip.
  PropagatingFlags harvest 458752. Late-bound names: computed-name
  type usable-as-property-name → synthesized prop with CheckFlags.Late
  4096 + links.nameType (this IS the 5.5 late-binding slice for object
  literals — annotate.rs's container late-binding stays M7).
  Destructuring-default Optional flag; pattern-implied optionality +
  the REVERSE excess check 2353 (L74197) when contextual pattern lacks
  the member and has no string index.
  [INFER] intra-expression sites — dead.
- SpreadAssignment arm: segment flush via local createObjectLiteralType
  → getSpreadType fold; checkMode masked to Inferential bit for the
  operand check (== 0 at M4); getReducedType; isValidSpreadType L74300
  (constraint-mapped, definitely-falsy-stripped; Any|NonPrimitive|
  Object|InstantiableNonPrimitive|recursive-union/intersection) else →
  2698 + spread=errorType; tryMergeUnionOfObjectTypeAndEmptyObject
  L62924 (all-optional partial synthesis — port); offset tracks
  post-spread segment.
- Accessors: checkNodeDeferred(memberDecl) → deferred accessor arm
  [5.8-DECL escape]; original symbol enters tables (no Readonly flag).
- Computed names NOT usable as property names: hasComputedString/
  Number/Symbol flags → getObjectLiteralIndexInfo L74098 synthesis
  (string covers numeric-named, excludes symbol-named; empty segment →
  undefinedType index). ASYMMETRY GOTCHA: the mid-loop flush resets
  all three hasComputed* flags (L74223-74225) but the FINAL flush
  (L74270-74276) resets only string+number — hasComputedSymbolProperty
  survives into the post-spread replacement literal. Transcribe as-is.
- Freshness: objectFlags accumulator starts FreshLiteral 8192; result
  |= ObjectLiteral 128 | ContainsObjectOrArrayLiteral 131072 (+JSLiteral
  4096 [JSDOC], +ObjectLiteralPatternWithComputedProperties 512);
  NO nodeLinks caching of the result — fresh identity per call.
- getSpreadType L62964: any/unknown/never lattice; left-merge; union
  distribution BOTH sides behind checkCrossProductUnion (≥1e5 → 2590 on
  currentNode + errorType); primitive-like right (incl. Index 2097152)
  → left; generic arms → intersection folding (isNonGenericObjectType
  tail-fold); concrete merge: getUnionIndexInfos (5.3d kit),
  private/protected skip via getDeclarationModifierFlagsFromSymbol,
  isSpreadableProperty L63040 (no #private; class methods/accessors
  don't spread), right-optional merge (leftType ‖ union w/ de-undefined
  right, Subtype 2; links leftSpread/rightSpread), getSpreadSymbol
  L63044 (readonly NORMALIZATION both directions; set-only accessor →
  undefined-typed), result flags |= ObjectLiteral|Contains…|
  ContainsSpread 2097152 | caller objectFlags. NO 2698 inside — caller
  emits (two sites: L74239 object literal, L74423 JSX).

## §6 Property/element access + non-null (the 2339 band)

### Non-null core (port first — everything routes through it)
checkNonNullExpression L74990 = checkNonNullType(checkExpression(node),
node). checkNonNullTypeWithReporter L75028: strictNullChecks+Unknown →
entity-name<100 ? 18046 : 2571 → errorType; facts = getTypeFacts(type,
IsUndefinedOrNull 50331648) [FACTS-ported]; facts hit → report + strip
via getNonNullableType; still nullable/never → errorType.
reportObjectPossiblyNullOrUndefinedError L74999 selection: NullKeyword
→ 18050("null"); identifier "undefined" → 18050("undefined");
entity-name<100 chars → 18049/18048/18047 (undef+null/undef/null);
else → 2533/2532/2531. checkNonNullNonVoidType L75051 (+Void → 18048/
18050/2532 flavor): CORRECTED 2026-07-12 (oracle+source verified) — its
ONLY consumers are checkVariableLikeDeclaration L83479/83488 (binding-
pattern band → ports at 5.8, NOT here). The assertion arm 236 is
checkNonNullAssertion L77960 = OptionalChain ? checkNonNullChain :
getNonNullableType(checkExpression(…)) — NO reporting: `x!` strips
silently (`x!` on void → never, no diagnostic; pinned). Facts quirks
pinned: getTypeFacts∌void ⇒ void receiver gets plain 2339 "on 'void'"
(never likewise); `(null).foo` → 2531 (parens defeat both the
NullKeyword kind test and the entity-name test — no skipParentheses).
Invoke flavor 2721/2722/2723 = [CALLS] reporter (5.7).

### checkPropertyAccessExpressionOrQualifiedName L75201-75322
- apparentType = getApparentType(assignment-or-method-call-receiver ?
  getWidenedType(leftType) [WIDEN-stub identity] : leftType).
- Private-name arm: emit-helper gates skip; lexical lookup L75087
  (members=instance, exports=static walk); 2803 private-method assign;
  anyLike passthrough; 18016 no-class; prop lookup on LEFTTYPE (not
  apparent); miss → checkPrivateIdentifierPropertyAccess L75139 (18014+
  17-18 shadow chain ‖ 18013) ‖ plain-JS 1111; set-only accessor →
  2806.
- anyLike (any ‖ silentNever) → apparent/errorType passthrough.
- getPropertyOfType(apparentType, name, skipObjectFunctionPropertyAugment
  = isConstEnumObjectType, includeTypeOnlyMembers = QualifiedName).
- Unresolved: index-info attempt (getApplicableIndexInfoForName; gated
  writing-into-generic exception); index hit → 2542 readonly-write,
  noUncheckedIndexedAccess non-Definite → ∪ missingType,
  noPropertyAccessFromIndexSignature → 4111; NO index → isUncheckedJS
  [JSDOC] / JSLiteral anyType / globalThis arm (block-scoped 2339 ‖
  noImplicitAny 7017 ‖ anyType) / checkAndReportErrorForExtendingInterface
  gate → reportNonexistentProperty; → errorType.
- Resolved: deprecation skips; checkPropertyNotUsedBeforeDeclaration
  L75372 (2729 + 2449 + related 2728 — property-initializer/static-
  block positions; declared-before-use walker = evaluate.rs slice);
  markPropertyAsReferenced L75598 (private-member isReferenced
  bookkeeping — unused-checks M7: port the symbol write, inert);
  nodeLinks(node).resolvedSymbol = prop; checkPropertyAccessibility
  (below); isAssignmentToReadonlyEntity → 2540 + errorType;
  propType: this-property-in-constructor → autoType (dormant);
  writeOnly/isWriteOnlyAccess → getWriteTypeOfSymbol (5.3e exists;
  resolve.rs isWriteOnlyAccess row un-escapes); else getTypeOfSymbol.
- Tail: getFlowTypeOfAccessExpression L75339 — Definite →
  removeMissingType; non-narrowable symbol kinds passthrough; autoType
  → [FLOW-stub]; getNarrowableTypeForReference; assumeUninitialized
  (strictPropertyInitialization this-prop arm + JS arm) → initial =
  ∪undefined → 2565 when flow(≡stub) lacks undefined — under the stub
  flowType==propType: 2565 fires ONLY when propType lacks undefined
  AND initial added it… stub returns propType ⇒ containsUndefined
  false ⇒ 2565 DORMANT until M5 unless assumeUninitialized path
  compares differently — transcribe the branch verbatim, it self-
  deactivates. Return assignmentKind? getBaseTypeOfLiteralType : flow.

### reportNonexistentProperty L75416 + suggestions — PORT AT 5.5d
nodeLinks.nonExistentPropCheckCache dedupe (`{typeId}|{isUncheckedJS}`
keys). Chain build: non-primitive UNION → first constituent lacking
prop+index chains its own 2339 under the head; typeHasStaticProperty →
2576; getPromisedTypeOfPromise has prop → 2339 + related 2773 (await
hint — promise family §9); lib-feature table →
getSuggestedLibForNonExistentProperty → 2550 (resolve.rs
SCRIPT_TARGET_FEATURE table: per-lib member lists un-escape);
spelling → 2551 (2568 unchecked-JS, suggestion-category) + related
2728; else containerSeemsToBeEmptyDomElement → 2812 ‖ 2339 with
elaborateNeverIntersection wrapper. DECISION: port getSpellingSuggestion
core (L951-975: maximumLengthDifference max(2,⌊0.34len⌋), bestDistance
⌊0.4len⌋+1, levenshteinWithMax w/ 0.1 case-substitution cost, "-prefix
reject, alias-resolving meaning filter L75579) — ~60 lines self-
contained; it is the FP boundary (plain-2339 where tsc says 2551 = FP).
Same core retires the name-side FP-strategy debt (2552/2662 at
resolve.rs onFailed) — recommend porting name-side in the same commit.

PINNED 2026-07-12 (oracle, session pins55d-findings.md):
- Spelling-core arithmetic: substitution cost 2 / case-only 0.1 /
  ins-del 1, max passed = bestDistance−0.1 ⇒ a pure-substitution typo
  suggests only at name-len ≥ 5 (`abd`→`abc` NO, `worls`→`world` YES,
  `helo`→`hello` insertion YES); candidate len<3 requires full
  case-insensitive equality (`ax`↛`ab`, `AB`→`ab`).
- Element-access 2551 (ladder row) has NO related 2728; property-side
  2551 HAS it. Element-side 2576 renders `C["s"]`, property-side `C.s`.
- Property-side suggestions are NOT budget-gated (see below); they fire
  freely in noLib.

### Name-side suggestionCount budget — DISCOVERED 2026-07-12, PORT AT 5.5d
onFailedToResolveSymbol L48111: the 2552-family suggestion block is
gated by checker-wide `suggestionCount < maximumSuggestionCount (10)`;
`suggestionCount++` sits INSIDE the guard-chain if, after emission —
so guard-arm-handled failures (2662/2663/2693/…) do NOT consume, while
no-suggestion failures (plain-2304 tail) and lib-suggestion (2583-
family) failures DO. Budget is program-wide and ordered (increments at
lazy-drain time = queue order = our eager program order; the 5.4
addLazyDiagnostic-eager-identity argument extends to the counter).
**noLib bootstrap burns exactly 10**: initializeTypeChecker L88732's
reportErrors=true getGlobalType list — IArguments, Array, Object,
Function, CallableFunction†, NewableFunction†, String, Number, Boolean,
RegExp († strictBindCallApply-gated; the rest of init uses
getGlobalTypeOrUndefined = no burn); errorLocation=undefined
short-circuits the guard, lib-name probes (Array/String/RegExp →
"es2015") skip the suggest attempt but still ++. ⇒ noLib pins: 2552
NEVER fires (near-miss → plain 2304 — pin it); lib-loaded: fires, and
failure #11+ degrades 2552→2304 (conformance-gated). Verified noLib pin
set via strictBindCallApply:false (burn=8, budget=2): three near-misses
→ 2552/2552/2304; 2662-first → non-consuming; xyzzy-first → consuming.
PORT: suggestion_count on CheckerState; init-slice burn = the 10
ordered NAME-RESOLUTION probes incl. the suggest attempt (type
materialization stays lazy — documented extension of the 5.0
lazy-globals deviation); getSuggestedSymbolForNonexistentSymbol =
resolve_name lookup-mode twin (Normal | Suggestion; createNameResolver
L19516 structure) w/ capitalized-primitive synthetic candidates at the
globals level (L75522); 2552 carries canonicalHead=(2304, plain text) —
diags comparer needs the canonicalHead arms (sort: canonical-bearing
first among equal, L17881; equality: compare canonical code+message,
L17949/17953). getResolvedSymbol meaning = Value|ExportValue, isUse =
!isWriteOnlyAccess; nameNotFoundMessage per-name table
getCannotFindNameDiagnosticForName L69324.

### checkPropertyAccessibility L74871/74875
Modifier source = getDeclarationModifierFlagsFromSymbol(prop, writing)
(5.3e fixed the value-decl bug). Arms in order: super+ES5 non-method →
2340; super+abstract → 2513; super + instance-field (class instance
property) → 2855; abstract-in-constructor-position → 2715; non-public
fast-path; private wrong-class → 2341; super → protected OK; protected:
enclosing-class derivation walk (incl. this-parameter fallback
getEnclosingClassFromThisParameter) → 2445, static → OK, containingType
TypeParameter → constraint, hasBaseType miss → 2446.

### Element access checkIndexedAccess L75711 wrapper
chain split (checkElementAccessChain: getOptionalExpressionType →
checkNonNullType → worker → propagateOptionalTypeMarker).
checkElementAccessExpression L75719: receiver widening [WIDEN-stub];
index checked BEFORE errorType bail; const-enum non-string-literal
index → 2476 + errorType; isForInVariableForNumericPropertyNames →
numberType substitution; AccessFlags: None-assignment →
ExpressionPosition 32; else Writing 4 | (generic non-this →
NoIndexSignatures 2), Compound adds ExpressionPosition;
getIndexedAccessTypeOrUndefined(objectType, effectiveIndexType,
accessFlags, node) ‖ errorType; tail = checkIndexedAccessIndexType(
getFlowTypeOfAccessExpression(node, links.resolvedSymbol, …,
indexExpression), node) — L81893: non-IndexedAccess passthrough;
every-index-assignable check → mapped-readonly write 2542; generic +
private-named prop → 4105; else 2536.

### getPropertyTypeForIndexType L62211 expression arms (un-escape
indexed.rs 784 ladder + 655 write rows + 498 IncludeUndefined)
- noUncheckedIndexedAccess + ExpressionPosition → IncludeUndefined
  injection at L62575 (indexed.rs 498 note).
- Deferral asymmetry L62576: expressions defer only generic TUPLES;
  type nodes defer any generic object (already-live tables path).
- propName fast path: Contextual flag arm (5.7); prop hit →
  ReportDeprecated skip; accessExpression block:
  markPropertyAsReferenced, readonly-write → 2540 + undefined(→error),
  CacheSymbol → nodeLinks(accessNode).resolvedSymbol = prop,
  this-prop-in-ctor → autoType; Writing → getWriteTypeOfSymbol; flow
  wrap non-Definite [FLOW-stub] ‖ type-node missing-type arm.
- Tuple numeric names: no-Variable tuples w/o AllowMissing → negative
  → 2514 + undefinedType; out-of-range → 2493 (typeToString, arity,
  name); union-of-tuples → 2339; in-range →
  getTupleElementTypeOutOfStartCount (5.3c exists) + readonly-index
  write 2542.
- Index-signature arm: applicable ‖ string fallback; NoIndexSignatures
  non-number → Writing? 2862 : 2536 → undefined; string-key +
  non-string/number index → 2538 (still returns the type!);
  readonly-write 2542; IncludeUndefined → ∪ missingType (enum-keyed
  exemption); Never → neverType; JSLiteral → any [JSDOC].
- Expression error tail (non-const-enum): object-literal noImplicitAny
  literal-index → 2339(value) + undefinedType, number/string index →
  union-of-all-props ∪ undefined; globalThis block-scoped → 2339;
  noImplicitAny non-suppressed ladder: static-prop → 2576(suggestion
  `T["x"]` text), number-index-exists → 7015, spelling → 2551,
  get/set-method probe → 7052, else per-index-kind head (EnumLiteral/
  UniqueESSymbol/String/NumberLiteral → 2339 variants; Number|String →
  7054) wrapped in 7053 chain. SuppressNoImplicitAnyError 128 =
  union-distribution retry protocol (first miss suppresses later
  constituents' reports) — L62589-62606: union index distributes;
  Writing → intersection of per-constituent results, else union w/
  Literal reduction 1; non-union path adds CacheSymbol|ReportDeprecated.
- Post-arm: AllowMissing object-literal → undefinedType; JSLiteral →
  any; accessNode fallback errors 2339(literal-value)/2537/2538
  (bigint-literal special text "bigint"); any-index passthrough;
  undefined.

### Optional chaining plumbing
getOptionalExpressionType L67880 (chain-root → getNonNullableType;
chain-link → removeOptionalTypeMarker); propagateOptionalTypeMarker
L67877 (outermost → getOptionalType ∪ undefined; inner →
addOptionalTypeMarker w/ the MARKER undefined singleton — distinct
from undefinedType; non-strict = identity). Tables need the marker
singleton (optionalType twin of undefined) — check ty.rs; add if
missing (intern-level, no relation impact: marker only flows through
removeType).

## §7 Operators

### The trampoline (MANDATORY shape — no recursive checkBinary)
GENERIC machine at L27945-28071 (`BinaryExpressionState` +
`BinaryExpressionStateMachine` + `createBinaryExpressionTrampoline`):
states are enter/left/operator/right/exit/done over parallel
stateStack/nodeStack/userStateStack + resultHolder + shared outerState;
`left`/`right` advance-then-maybe-push (returned BinaryExpression child
pushes a frame; else eager checkExpression via user callback); `exit`
folds into parent via foldState with side inference (parent state ptr).
checker instance = createCheckBinaryExpression L79810-79935, wired
`var checkBinaryExpression = …` L46480. USER STATE = ONE shared record
{checkMode, skip, stackIndex, typeStack[]} reused across frames
(leftType at [stackIndex], lastResult at [stackIndex+1]) — onEnter
re-entry increments stackIndex; onExit decrements. onEnter arms:
JS expando skip [JSDOC]; checkNullishCoalesceOperands (5076 mixing +
2871 always-nullish/2869 never-nullish via getSyntacticNullishnessSemantics
L79969 — syntactic classifier, port whole); destructuring `=` route →
skip + checkDestructuringAssignment(left, checkExpression(right), …,
rightIsThis = right is `this`). onOperator: logical ops → walk
non-paren logical parents; `&&` or if-parent →
checkTestingKnownTruthyCallableOrAwaitableOrEnumMemberType (L83636 —
strictNullChecks-only; 2845 enum-literal condition, 2774 function-
always-defined, 2801 promise-always-defined [promise probe §9];
body-use walkers L83690/L83722); `&&`/`||` → checkTruthinessOfType.
maybeCheckExpression L79917 = the ONLY recursion into checkExpression
(non-binary children). checkBinaryLikeExpression L80009 = non-trampoline
entry for synthetic `name = initializer` (destructuring defaults) —
port as thin wrapper.

### checkBinaryLikeExpressionWorker L80023-80435 — operator arms
(errorNode param = whole binary node from trampoline; grammar fallback
operatorToken.)
- **Arithmetic band** (`* ** / % - << >> >>> & | ^` + compounds):
  silentNever propagation; checkNonNullType both; both-BooleanLike +
  getSuggestedBooleanOperator (|→||, ^→!==, &→&&) → 2447 + numberType;
  checkArithmeticOperandType L79214 (LHS 2362 / RHS 2363; await probe
  → errorAndMaybeSuggestAwait + related 2773); result: any/non-bigint
  → number; both-BigIntLike → `>>>` reportOperatorError, `**` ES<2016
  → 2791, else bigint; mixed → reportOperatorError → errorType;
  checkAssignmentOperator on ok; shift-simplify 6807 via evaluate
  (evaluate.rs EXISTS — un-escape its checkExpression-era rows;
  error-inside-enum-member else suggestion → suggestion path skips).
- **`+`/`+=`**: silentNever; non-StringLike both → checkNonNullType;
  matrix number/bigint/string/any (strict kind tests via
  isTypeAssignableToKind L79528); symbol operand → 2469 (result still
  returned); no result → reportOperatorError(close-enough probe) →
  anyType; `+=` → checkAssignmentOperator.
- **Relational `< > <= >=`**: 2469 symbol gate;
  getBaseTypeOfLiteralTypeForComparison ∘ checkNonNullType both;
  compatible iff any-side ‖ both-number/bigint-assignable ‖
  neither+areTypesComparable; failure → **2365** (NOT 2367); →
  booleanType.
- **Equality `== != === !==`**: TypeOnly checkMode skips ENTIRE arm;
  object-literal/fn/class/regex literal operand → 2839
  (always-false/true by op); checkNaNEquality → 2845 + related 1369
  `Number.isNaN(…)` hint; comparable-either-direction else
  reportOperatorError → equality upgrade **2367** (tryGiveBetterPrimary-
  Error; args (leftStr, rightStr) after getBaseTypesIfUnrelated
  literal-base widening + await probe); → booleanType.
- **instanceof**: §8 slice below. **in**: silentNever; private `#x in
  obj` → emit-helpers skip + unresolved-in-class → reportNonexistent-
  Property(2339-family on rightType); else LHS
  checkTypeAssignableTo(checkNonNullType(left), stringNumberSymbolType)
  = plain 2322 (2360/2361 DO NOT EXIST in 6.0.3 — verified absent);
  RHS assignable-to-nonPrimitive (2322) + empty-object-intersection →
  2638; → booleanType.
- **`&&`/`&&=`**: hasTypeFacts(left, Truthy) ? getUnionType([
  extractDefinitelyFalsyTypes(strictNullChecks ? leftType :
  getBaseTypeOfLiteralType(rightType)), rightType]) : leftType —
  VERBATIM QUIRK: non-strict takes the falsy-part of the RIGHT type's
  literal base. extractDefinitelyFalsyTypes L67842/getDefinitelyFalsy-
  PartOfType L67845 (string→"" type, number→0, bigint→0n, literal-
  falsy→self, else never). `&&=` → checkAssignmentOperator(rightType).
- **`||`/`||=`**: hasTypeFacts(left, Falsy) ? ∪([getNonNullableType(
  removeDefinitelyFalsyTypes(left)), right], Subtype 2) : leftType.
- **`??`/`??=`**: hasTypeFacts(left, EQUndefinedOrNull) ?
  ∪([getNonNullableType(left), right], Subtype 2) : leftType.
- **`=`**: assignment-declaration analysis [JSDOC] (kind 0 None = TS
  path); checkAssignmentOperator(rightType); returns leftType on the
  expando-ish shapes else rightType — TS path: isAssignmentDeclaration2
  false → checkAssignmentOperator + return rightType.
- **comma**: 2695 unused-left (isSideEffectFree table L79754;
  indirect-call `(0, f)()` exemption; JSX-2657-parse-diag suppression
  check) → rightType.
- **checkAssignmentOperator** (nested, L80323): addLazyDiagnostic
  (=eager); compound + property-access LHS → re-check writeOnly
  (checkPropertyAccessExpression(left, undefined, true) — write type);
  checkReferenceExpression(left, 2364, 2779); exactOptionalPropertyTypes
  mismatch headMessage 2412; checkTypeAssignableToAndOptionallyElaborate
  (valueType, assigneeType, errorNode=LEFT, expr=RIGHT) → 2322 head.
- **reportOperatorError** L80374: await-probe (getAwaitedTypeNoAlias
  both) → wouldWorkWithAwait; getBaseTypesIfUnrelated display
  widening; getTypeNamesForErrorDisplay (dual typeToString +
  fully-qualified retry on collision L50748) → 2365 w/ optional 2773
  related.

### Destructuring assignment family L79609-79753 (port whole; [ITER]
escapes on array iteration)
checkDestructuringAssignment (shorthand-default [FACTS] NEUndefined
strip + synthetic checkBinaryLikeExpression on `name = init`; inner
`=` target → trampoline + NEUndefined strip); object →
checkObjectLiteralAssignment (strictNullChecks empty-pattern
checkNonNullType; per-property: literal-name → getIndexedAccessType(
…, ExpressionPosition|AllowMissing-on-default, name) +
markPropertyAsReferenced + accessibility(writing=true) +
getFlowTypeOfDestructuring [FLOW-stub]; spread → 2462 non-last /
getRestType L55841 + trailing-comma 1013; else 1136); array →
checkArrayLiteralAssignment ([ITER] Destructuring|PossiblyOutOfBounds
→ escape; per-element getIndexedAccessTypeOrUndefined w/ synthetic
expression L76289 + [FACTS] NEUndefined on defaults + [FLOW-stub];
spread → 2462 / initializer 1186 / sliceTupleType ‖ createArrayType);
reference → checkReferenceExpression (rest flavor 2701/2778, else
2364/2779) + checkTypeAssignableToAndOptionallyElaborate → 2322;
private-field target emit-helper skip. All return sourceType.

### Unary band
checkPrefixUnaryExpression L79427: fresh-literal negation ARMS FIRST
(-numeric → fresh(-n); +numeric → fresh(+n); -bigint → fresh negative
bigint; NO +bigint arm); `+ - ~` → checkNonNullType + ESSymbolLike →
2469; `+` on maybe-bigint → 2736(getBaseTypeOfLiteralType display) +
numberType; result getUnaryResultType L79501 (maybe-BigIntLike →
any/number-mixed? numberOrBigInt : bigint; else number); `!` →
checkTruthinessOfType + getTypeFacts(Truthy|Falsy) → false/true/boolean;
`++ --` → checkArithmeticOperandType(2356) ∘ checkNonNullType +
checkReferenceExpression(2357/2777). checkPostfixUnaryExpression
L79482 = the ++/-- arm. checkDeleteExpression L79303: 2703 non-access /
18011 private / readonly → 2704 / strict non-optional → 2790 ([FACTS]
IsUndefined); → booleanType. checkTypeOfExpression → typeofType
(createTypeofType L50136 union singleton — construct at state init).
checkVoidExpression → checkNodeDeferred + undefinedWideningType.

### Conditional / template / assertions / satisfies / meta
checkConditionalExpression L80513: checkTruthinessExpression(condition)
+ known-truthy probe; branches; ∪([t1,t2], **Subtype 2**).
checkTruthinessOfType L83748: Void → 1345; getSyntacticTruthySemantics
L83762 (syntactic classifier) → 2872 always / 2873 falsy.
checkTemplateExpression L80524: per-span ESSymbolLike → 2731;
span type kept iff assignable to templateConstraintType (∪ string,
number, boolean, bigint, null, undefined — singleton L47101) else
string; evaluate (non-tagged) → fresh string literal on success;
isConstContext ‖ isTemplateLiteralContext ‖ contextual template-y →
getTemplateLiteralType (M3 exists); else stringType.
checkAssertion L77863 (.mts/.cts 7059; erasableSyntaxOnly 1294) →
checkAssertionWorker L77908: const-assertion →
isValidConstAssertionArgument L77877 (kind table + enum-member
access arm via resolveEntityName) else 1355 →
getRegularTypeOfLiteralType; else links.assertionExpressionType stamp
+ checkSourceElement(type) + checkNodeDeferred → getTypeFromTypeNode.
checkAssertionDeferred L77939: getRegularTypeOfObjectLiteral ∘
getBaseTypeOfLiteralType [WIDEN-stub identity + pins]; lazy(=eager)
2352 comparable check (target→widened silent, then
checkTypeComparableTo(exprType, targetType, errNode, 2352)).
checkSatisfiesExpression L78047: checkSourceElement(type);
checkTypeAssignableToAndOptionallyElaborate(exprType, target,
findAncestor-Satisfies errorNode, expression, **1360**) → return
exprType.
checkExpressionWithTypeArguments L77963: checkGrammarExpressionWith-
TypeArguments (1326 import-with-typeargs); typeArguments →
checkSourceElement each; instanceof-RHS instantiation-expr → 2848;
exprType (typeof-with-args path via exprName/this); →
getInstantiationExpressionType L77975: links.instantiationExpressionTypes
map cache (STORE BEFORE ERROR); per-part instantiation (Object →
resolved members + getInstantiatedSignatures: arity filter
hasCorrectTypeArgumentArity [CALLS-kit] + checkTypeArguments
(reportErrors=true → 2344 via 5.4 constraint kit) +
getSignatureInstantiation (5.2 exists); anonymous
`__instantiationExpression` type w/ InstantiationExpressionType
8388608 flag; InstantiableNonPrimitive → constraint retry; Union →
mapType full accounting; Intersection → per-part) → no-applicable →
**2635** on the typeArguments RANGE (createDiagnosticForNodeArray).
checkMetaProperty L78061 (+grammar 17012/18061/1005): new.target →
checkNewTargetMetaProperty L78086 (17013 outside fn/ctor; ctor →
class symbol type; else function symbol type); import.meta →
checkImportMetaProperty L78099 (node-format 1470 / moduleKind 1343;
global ImportMeta ‖ emptyObjectType; `import.defer` → errorType).

### instanceof slice (checkInstanceOfExpression L79558)
Port: silentNever; LHS all-Primitive non-any → 2358; then tsc resolves
a SIGNATURE for the binary (resolveInstanceofExpression L77445 inside
resolveSignature): hasInstance-method present → resolveCall w/ 2860
head [CALLS]; no call/construct sigs AND not subtype-of-Function →
**2359** + errorCall; else anySignature. 5.5 slice: port the 2358/2359
sides + `getSymbolHasInstanceMethodOfObjectType` L79546 probe; when a
callable hasInstance EXISTS → Unsupported escape (5.7 resolves; 2860/
2861 land there); when absent → booleanType return (matches tsc's
anySignature path: returnType any → 2861 check passes silently).
Deferred instanceof re-registration (checkDeferredNode binary arm) =
resolveUntypedCall [CALLS] — stays unreachable.

## §8 Functions / class / await / yield

### Function-expression trio (un-stubs check.rs deferred arms)
checkFunctionExpressionOrObjectLiteralMethod L79109: UNCONDITIONAL
checkNodeDeferred; checkCollisionsForDeclarationName (5.8 no-op stub);
SkipContextSensitive+contextSensitive arm: return-only signature
synthesis (contextFreeType cache; NonInferrableType flags) ‖
anyFunctionType — reachable at M4 only via getContextFreeTypeOfExpression
(CheckMode 4) — port; grammar gate; contextuallyCheck… L79152:
ContextChecked once-flag (64) DOUBLE-CHECK pattern (getContextualSignature
may re-enter — transcribe the two reads); single call signature of
getTypeOfSymbol; isContextSensitive L63832 (syntactic classifier —
port whole incl. hasContextSensitiveParameters/ReturnExpression/Yield);
contextualSignature → [INFER]-None path → assignContextualParameterTypes
L78374 (both-generic bail; this-param; per-param annotation-wins +
initializer widening probe [WIDEN-lite]; rest via getRestTypeAtPosition
5.3c kit; assignParameterType → symbolLinks.type once-write +
binding-pattern element propagation assignBindingElementTypes) ‖
assignNonContextualParameterTypes; contextual return seeding
signature.resolvedReturnType (re-check before write); 
checkSignatureDeclaration [5.8-DECL stub]. getTypeOfSymbol path =
annotate.rs function-symbol arm (exists since 5.3e).
checkFunctionExpressionOrObjectLiteralMethodDeferred L79194:
checkAllCodePathsInNonVoidFunctionReturnOrThrow → M5 stub (§0);
body Block → checkSourceElement (5.8 statement stubs swallow — the
deferred body drives NOTHING extra until 5.8; expression body →
checkExpression + unwrapReturnType (generator [ITER] escape; async →
getAwaitedTypeNoAlias) + checkReturnExpression L84550 (conditional
recursion; async → checkAwaitedType w/ 1058; effective-check-node;
2322 head).

### getReturnTypeFromBody L78752 (+aggregators)
Expression body: checkExpressionCached(body, checkMode & ~SkipGeneric-
Functions); const-context → regular-of-literal; async →
unwrapAwaitedType(checkAwaitedType(…, 1058)). Block: checkAndAggregate-
ReturnExpressionTypes L78959 (functionHasImplicitReturn [FLOW] → stub
false; per-return skipParentheses (+async await unwrap); SELF-RECURSIVE
`return f()` skip (checkExpressionCached(callee).symbol ===
getMergedSymbol(func.symbol)); Never tracking; const-context regular;
strict + no-expression returns → ∪ undefinedType) — undefined result →
never/Promise<never>; empty → contextual-undefined probe → void/
undefined ‖ Promise-wrapped; else ∪(types, Subtype). Generator arms
[ITER] escape (yield aggregation checkAndAggregateYieldOperandTypes
L78874, createGeneratorType L78842). Widening tail L78807 [WIDEN-stub
identity + ledger]: reportErrorsFromWidening ×3, unit-type contextual
widening (getWidenedLiteralLikeTypeForContextual… variants), final
getWidenedType each. Async wrap: createPromiseType L78702 (global
Promise ‖ unknownType) / createPromiseReturnType L78724 (2697 async /
2711 import-call when Promise missing; ES5 constructor probe 2705/2712)
/ createPromiseLikeType L78713.

### Await family (port whole at 5.5f — lib-loaded Promise resolves)
checkAwaitGrammar L79338 (lazy=eager): static-block → 18037/18054;
non-AwaitContext: top-level → non-module 1375/2853, node-CJS 1309,
target/module ladder → 1378/2854; nested → 1308/2852 + related 1356
async-hint; param-initializer → 2524. checkAwaitExpression L79408:
grammar; operand; checkAwaitedType(…, withAlias=TRUE, node, **1320**);
no-op await → suggestion 80007 (skip — suggestion category).
getAwaitedType family L82312-82497: getPromisedTypeOfPromise L82316
(per-TYPE promisedTypeOfPromise cache field; global-Promise reference
fast path via getTypeArguments[0]; primitive bail; `then` property →
call sigs (STRUCTURAL getSignaturesOfType — not [CALLS]) → 1059 none;
this-type filter → 2684 w/ thisTypeForError; onfulfilled param ∪ w/
[FACTS] NEUndefinedOrNull → 1060 non-callable; ∪ first-params Subtype);
getAwaitedTypeNoAlias L82435 (**awaitedTypeStack global type-id stack**
= the recursion guard; union arm w/ 1062 self-referential; per-type
awaitedTypeOfType cache; isAwaitedTypeNeeded generic passthrough →
Awaited<T> alias instantiation via tryCreateAwaitedType/getGlobalAwaited-
Symbol; promised recursion w/ 1062 stack check; thenable-but-unresolved
→ chained 2684?+caller-message diag; non-thenable → self);
unwrapAwaitedType L82399 (Awaited-alias-instantiation detector);
createAwaitedTypeIfNeeded L82424; checkAwaitedType L82377.
checkAsyncFunctionReturnType L82498 = 5.8-DECL consumer (1055/1064/
1065/2705/2520 + tail 1058) — extraction recorded; port at 5.8.

### checkYieldExpression L80447
Grammar closure (1163 non-generator-context, 2523 param-initializer);
non-generator func → anyType; [ITER] escapes: signature yield/next via
getIterationTypesOfGeneratorFunctionReturnType; yield* iterable checks;
assignability yieldedType→signatureYieldType (2322 head); `yield`
result = Next iteration type ‖ contextual ‖ anyType + 7057
noImplicitAny closure. 5.5 slice: port the grammar + non-generator
arm; generator bodies escape Unsupported (ledger 5.8-ITER) — per-
element containment keeps the enclosing file checked.

### checkClassExpression L84972
checkClassLikeDeclaration EAGERLY [5.8-DECL → Unsupported escape — the
whole arm escapes at 5.5: class expressions stay FN until 5.8; ledger];
checkNodeDeferred; helpers probe; getTypeOfSymbol. Deferred →
forEach members checkSourceElement + unused-registration [M7]. Note:
because the EAGER call escapes, checkClassExpression = escape row at
5.5 (do not partially port — heritage/member checks are one unit).

## §9 JSX attribute slice (5.5f; call resolution 5.7)

Namespace/config: JsxNames L90915 (JSX, IntrinsicElements, ElementClass,
ElementAttributesProperty, ElementChildrenAttribute, Element,
ElementType, IntrinsicAttributes, IntrinsicClassAttributes,
LibraryManagedAttributes); getJsxNamespaceAt L74586
(links.jsxNamespace cache; implicit-import container per-file cache
2792/2875 [module resolution — escape]; pragma/jsxFactory → "React"
resolveName Namespace; global JSX fallback). emptyJsxObjectType
singleton (JsxAttributes 2048 objectFlags).

Eager arms: checkJsxSelfClosingElement/checkJsxElement L74307/74320 =
checkNodeDeferred + getJsxElementTypeAt ‖ anyType (JSX.Element lookup
getJsxType L74525). checkJsxFragment L74324 EAGER: opening-fragment
check + factory pragma errors 17016/17017 + children + Element type.
checkJsxAttributes L74522 → createJsxAttributesTypeFromAttributesProperty
L74346: per-attribute synthesized props (checkJsxAttribute L74343:
initializer checkExpressionForMutableLocation ‖ trueType);
children-name via JSX.ElementChildrenAttribute container (>1 prop →
2608); explicit-children overwrite → 2710; spreads → getSpreadType
folds + isValidSpreadType else 2698 + checkSpreadPropOverrides
2783/2785; any-spread → anyType; children synthesis (single → type;
tuple-like contextual → createTupleType; else createArrayType(∪));
freshness FreshLiteral|ObjectLiteral|Contains… (EPC applies to JSX
attributes). checkJsxExpression L74847 (grammar 18007; dotDotDot
non-array → 2609).

Deferred arms + THE BOUNDARY: checkJsxElementDeferred/SelfClosing →
checkJsxOpeningLikeElementOrOpeningFragment L74797: prefix =
checkGrammarJsxElement (2633/2639 namespace-names, 17001 dup attr,
17000 empty initializer) + checkJsxPreconditions (17004 no-jsx-option,
2602 noImplicitAny-no-JSX.Element) + markJsxAliasReferenced [no-op
stub] — then **L74804 `getResolvedSignature` = the 5.7 line**: escape
Unsupported there. Unreachable until 5.7: 18053/2786/2787/2788/2789,
2607, 2604/2605-family (resolveJsxOpeningLikeElement L77397).
Closing-tag check (element deferred): intrinsic → getIntrinsicTagSymbol
(links.resolvedSymbol + jsxFlags; 2339 unknown-intrinsic w/
"JSX.IntrinsicElements" arg, 7026 noImplicitAny-no-interface) ‖
checkExpression(tagName). Intrinsic lookups port (getPropertyOfType /
index-signature on JSX.IntrinsicElements).

## §10 Rust-side seam inventory (what 5.5 un-stubs)

check.rs: L228 ExpressionStatement stub → live arm; deferred arms
(fn-trio incl. MethodSignature per tsc L86930, accessors → register +
5.8-escape, ClassExpression → register (eager arm itself escapes),
JSX pair, Assertion trio, Void → checkExpression(operand); Call-band +
instanceof-resolveUntypedCall arms STAY unreachable!("5.7")).
annotate.rs: 1675/6827 initializer arms STAY ESCAPED (→5.6, §12);
2456/2500/2529/4396/4516 late-bound container names STAY (M7; object-
literal late-binding is NEW code in checkObjectLiteral, not these
rows); 2904-2908 mixin extends expression → live via checkExpression
(getBaseConstructorTypeOfClass entity-name slice widens to full
checkExpression); 3417-3446 getter-body inference → getReturnTypeFromBody
lands — un-escape the getter arm (accessor TYPE side; accessor
declaration CHECKS stay 5.8); 4700 call-chain optionality → optional-
chain plumbing lands (§6) — un-escape.
indexed.rs: 784 expression diagnostic ladder + 655 write-position +
637 flow/deprecation/referenced-marking + 498 IncludeUndefined +
323-348 keyof computed/expression names (checkComputedPropertyName +
checkExpression now exist — un-escape) + 501 tuple-gate note.
resolve.rs: 1171 isWriteOnlyAccess consumers; 1574 per-lib member
lists (2550); 871/936 suggestion-divergence notes → resolved by
spelling-core port (§6 decision).
evaluate.rs: 205/255 (`enum E { A = A }` 2565-then-escape →
checkTypeAssignableTo now live — un-escape the tail); 696-845
declared-before-use walkers (property/class arms consumed by 2729/2449
property path); 1068-1072 computed-name text.
structural.rs: 3062 write-position relations (getWriteTypeOfSymbol
consumers). instantiate.rs: 1536/1577 getOuterTypeParameters
function-expression containers + isContextSensitive (real fn lands
§8). globals.rs: 445 arguments/IArguments consumer. links.rs: 98
resolved_members pre-5.5 slice note stays (M7).
state.rs: 219 instantiation-count third reset point (checkExpression).

New state needed: contextual stacks ×3 (+binding-patterns stack),
currentNode already exists (5.4), flowLoopStart/flowTypeCache/
flowInvocationCount placeholder fields (M5 shape), awaitedTypeStack
(Vec<TypeId>), cachedTypes map (discriminate memo), deferred-diag =
eager. NodeLinks new fields: resolvedType (expr cache), contextFreeType,
spreadIndices, skipDirectInference (read-only), assertionExpressionType,
instantiationExpressionTypes, nonExistentPropCheckCache, jsxNamespace/
jsxImplicitImportContainer/jsxFlags, NodeCheckFlags writes (capture/
super/loop bits — inert emit bookkeeping). Type-level caches:
literalType (array-literal clone), promisedTypeOfPromise,
awaitedTypeOfType, freshType/regularType (exist). SymbolLinks:
leftSpread/rightSpread/syntheticOrigin, isReferenced (via flags),
nameType (exists), target (exists).

## §11 FP=0 risk register (pin these at each slice)

1. **2551-vs-2339 code identity** (property suggestions): without the
   spelling core we emit 2339 where tsc emits 2551 → set-compare FP.
   Mitigation: port spelling core at 5.5d (decision §6) + pin fixtures
   with near-miss property names. Same for element-access 2551/7052/
   7053 ladder — the ladder's ORDER is the observable.
   PINNED 2026-07-12: matrices verified (§6 "PINNED" block). The risk
   turned out to have a second half: the NAME-side suggestionCount
   budget (§6 new section) — 2552-where-we'd-say-2304 AND
   2304-where-we'd-say-2552 are both live FP shapes; the budget +
   bootstrap burn must ship in the same commit as the spelling core.
2. **2352 as-assertion** under [WIDEN]-stubs: getWidenedType/
   getRegularTypeOfObjectLiteral identity may flip comparable verdicts
   on literal assertions. Pin `{a:1} as {a:number}` / `"a" as "b"` /
   array-literal-as-tuple fixtures BEFORE landing 5.5e; pull the two
   5.6 fns forward if any pin diverges.
3. **Fresh-literal EPC surface**: object literals become constructible
   → excess-property errors (2353 head via relation) fire wherever
   fresh types meet relations. EPC machinery is M3-tested, but the
   FRESHNESS PLUMBING (which positions strip fresh) is new: mutable-
   location regular-conversion, checkExpressionWithContextualType's
   fresh→regular tail, assertion regular-conversion. A missed strip =
   spurious 2353 = FP. Pin the four positions.
4. **Facts-classifier fidelity**: getTypeFactsWorker drives nonnull
   SELECTION + operator RESULTS. A wrong bit = wrong code (2532 vs
   2533) or wrong result type cascading into 2322 args. Port from
   source verbatim + oracle-pin nullable-receiver matrix (null,
   undefined, null|undefined, unknown, optional-chain flavors ×
   entity-name/long-expression).
   PINNED 2026-07-12: matrix verified (rows in §6 corrected block +
   session pins55d-findings.md). Surprises locked: facts ∌ void
   (void/never receivers → plain 2339); 18050 only for LITERAL
   null/`undefined` expressions (nullable-TYPED idents → 18047/18048);
   `(null).foo` → 2531; `x?.a` error span includes `?.`, message text
   doesn't; `x!` never reports (§6 correction). getNonNullableType =
   getAdjustedTypeWithFacts(NEUndefinedOrNull) L69784 — needs
   recombineUnknownType + unknownUnionType + removeNullableByIntersection
   + getGlobalNonNullableTypeInstantiation (noLib fallback
   ∩ emptyObjectType) — all port with the nonnull core.
5. **checkMode plumbing**: checkExpressionCached bypasses cache for
   ANY nonzero mode — caching under Contextual would freeze contextual
   results. Transcribe the falsy-gate exactly.
6. **Per-element containment inflation**: expression statements now
   route EVERYTHING through checkExpression — any Unsupported in a
   subexpression kills the whole statement's diags (FN, safe) but a
   PANIC kills the run: the [CALLS]/[ITER] arms must be Unsupported
   escapes, never unreachable!().
7. **Deferred-pass ordering**: object-literal accessors + assertions +
   void operands now enter deferred_nodes alongside 5.4's type
   parameters — IndexSet insertion order = tsc's checkDeferredNodes
   order; diag dedupe/sort makes order mostly unobservable, but
   instantiation-count resets are per-deferred-node (already wired).
8. **JS fixtures**: expando/assignment-declaration arms [JSDOC] skip →
   plain-JS band already gates; verify check_js interplay on the new
   arms (onEnter expando skip must NOT run for TS files — it is
   isInJSFile-gated in source).

## §12 Slicing + sequencing (commits `m4 5.5a-f`, rate per commit)

- **5.5a — driver + leaves**: CheckMode audit; checkExpression/worker
  skeleton w/ ALL arms as named Unsupported stubs; literals (fresh
  types, 2737 grammar, regex once-flag, globalRegExpType at state);
  identifier (flow stub INTRODUCED; mutability codes; TDZ wiring);
  this/super; qualified-name; paren/omitted/synthetic; typeof/void/
  delete; checkExpressionStatement un-stub; typeofType singleton;
  instantiation-count reset. Gates: conformance bump from identifier/
  literal/statement fixtures; pins: mutability sextet 2628-2632/2539,
  2588/2540, this/super matrices.
- **5.5b — contextual typing**: stacks + getContextualType full switch
  (5.7 arms → None) + apparent/instantiate + contextual signatures +
  checkExpressionWithContextualType + driver helpers (mutable-location,
  declaration-initializer, quick-type) + literal-level wideners (§0
  carve-out). Mostly latent; gates must stay flat (FP=0 trivially).
- **5.5c — literals**: array/object/spread + computed names +
  getSpreadType + index synthesis + freshness + 2464/2353/2698/2590/
  2783+2785 + [ITER] fast-path/escapes. EPC goes LIVE — pin risk #3.
- **5.5d — access band**: nonnull core + facts classifier port (§0
  decision) + property/element access + accessibility + 2339 chain +
  spelling core (property + name side) + indexed.rs ladder un-escape +
  optional chaining + write-position rows. Biggest pin set: risk #1/#4
  matrices, 2493/2514/2476/2542/4111/7015/7052/7053/7054, 18013/18014/
  18016, 2341/2445/2446/2513/2340/2855/2715, 2729+2728.
- **5.5e — operators**: generic trampoline + checkBinary handlers +
  worker arms + destructuring assignment + unary + conditional +
  template + assertions/satisfies (pin risk #2 FIRST) +
  instantiation-expressions + meta-properties + instanceof/in slices.
  Codes: 2362/2363/2447/2791/6807-band, 2365/2367/2839/2845+1369,
  2469/2736/2356/2357/2777, 5076/2871/2869, 2695, 2412, 1345/2872/
  2873/2731, 1355/2352/1360/2848/2635, 17012/17013/18061/1343/1470,
  2358/2359/2638, 2462/1186/1136/2701/2778.
- **5.5f — functions/await/JSX**: fn-expression trio + deferred
  registrations + isContextSensitive + assignContextualParameterTypes
  + getReturnTypeFromBody + promise/awaited family (1320/1058/1059/
  1060/1062/2684/2697/2705/2711/2712 + await grammar 1308-band) +
  yield slice (1163/2523) + JSX attribute slice (§9 codes) +
  getter-body un-escape + class-expression escape row.

Per-commit gates (unchanged discipline): cargo test workspace; relpin
regen 403+/0; ledger append-only (span-hash helper); conformance FULL
(lib-loaded, ~260s) — T0 rate recorded in commit body, **FP=0
absolute**; invariants idempotence; new-code oracle pins land WITH the
commit that makes them constructible.

Cross-references: steps doc §5.5 (scope), §0 stub policy supersedes
its TypeFacts identity-stub sentence (facts classifier ports at 5.5d —
rationale §0/[FACTS]); checker-foundations §3 (contextual) §5
(widening split); impl-checker-2xxx §5/§5b (emission tables — note
2360/2361 rows there are 5.x-era: 6.0.3 uses plain 2322 for `in`
operands, verified absent from the bundle).
