# M4 stage 5.7: call resolution with stubbed inference — semantic extraction

Extracted 2026-07-13 from the vendored bundle
`tsrs2/vendor/typescript-6.0.3/lib/_tsc.js` (all `L`-anchors are lines
in THAT file; re-grep on re-vendor). Parents: skeleton-steps §5.7
(scope), checker-key-functions §3 (resolveCall/chooseOverload
skeletons — this doc supersedes its M4 details where they differ),
m4-55-expression-extraction (stub-policy tags §0, forcing map,
FP discipline — all carried forward). Implementers start HERE.

This doc's slicing (§12: commits `m4 5.7a-c`) supersedes the steps
doc's single-commit line, same as m4-55 did for 5.5.

## §0 The M6 stub — contract and observability rule

THE stub M6 swaps is `inferTypeArguments` (L75938) plus the
inference-context construction at BOTH call sites:
chooseOverload (L76809-76817) and
inferSignatureInstantiationForOverloadFailure (L76946-76954, reached
from pickLongestCandidateSignature L76932).

**M4 stub value**: per type parameter, default → constraint →
`unknownType`. Ledger `/// M6-stub` on BOTH sites, with the note: tsc's
real no-inference fallback is default → unknown; the constraint step is
an M4-only enrichment M6 MUST REMOVE, not preserve. The stub does NOT
run tsc's argument walk (no checkExpressionWithContextualType with an
inference context — 5.5b's uninhabited InferenceContextPlaceholder
SURVIVES 5.7), no JSX-attributes inference, no this-type/return-type
contextual inference, no impliedArity.

**The observability rule (THE FP=0 cut of this stage)**: resolve the
full structure, but escape (`Unsupported`, class M6-stub) any VALUE the
stub invents at the moment it would become observable:

- chooseOverload, generic candidate, NO explicit type arguments →
  instantiate with stub types, run arity as usual, then:
  - applicability FAILS against the stub instantiation → do NOT record
    a candidatesForArgumentError entry — ESCAPE the whole resolution
    (contain). Reporting tsc's 2345/2769 chains against param types tsc
    never saw is a wrong-payload FP; containment is the honest FN.
  - applicability SUCCEEDS → the RESULT (return type) is stub-poisoned
    (`identity(1)` would return `unknown` where tsc infers `number` —
    downstream 2322 FP) → ESCAPE the resolution result.
- Generic candidate WITH explicit type arguments: checkTypeArguments is
  REAL (getTypeFromTypeNode + constraint checks) → fully live, report
  everything (2344/2345/2554/2769...).
- Non-generic candidates: fully live.
- Paths that never depend on stub values stay live even for generic
  candidates: arity errors (2554/2555/2575 — declared parameter counts;
  EXCEPTION: the getNonArrayRestType re-arity check L76819 reads the
  INSTANTIATED rest tuple — if the winning arity verdict hinges on a
  stub-typed rest tuple, escape), type-argument arity (2558/2743),
  explicit-typearg constraint failures (2344), target-not-callable
  bands (2349/2351/2348/2346...), untyped-call band.

Practical consequence: calls to generic signatures without explicit
type arguments (most lib methods: `map`, `then`, ...) contain until M6;
non-generic calls, explicit-typearg calls, and every
target-shape/arity/typearg error goes LIVE now. That is what the
skeleton means by "port the REAL structure so M6 only swaps one
function".

**M6-dead machinery to port as guarded shape (not escape)**:
skippedGenericFunction (L80816: requires CheckMode::INFERENTIAL — M6),
the `checkMode & SkipGenericFunctions && isGenericFunctionReturningFunction`
→ return resolvingSignature arm in resolveCallExpression (L77039-42),
checkCallExpression's `signature === resolvingSignature →
silentNeverType` guard (L77616). SkipGenericFunctions is only ever
injected inside the REAL inferTypeArguments arg walk, which the stub
omits — so expr.rs's CheckMode audit (`unreachable!` for
INFERENTIAL|SKIP_GENERIC_FUNCTIONS reaching checkExpression) STAYS
VALID at 5.7. argCheckMode |= SkipGenericFunctions via
`inferenceContext.flags & SkippedGenericFunction` (L76816) reduces to a
no-op (stub context has no flags).

**What IS live in the checkMode dance**: argCheckMode =
SkipContextSensitive when any arg isContextSensitive (L76611-13,
isContextSensitive = 5.5f kit); the RE-RUN block (L76840-64): after a
candidate passes under SkipContextSensitive, argCheckMode resets to
Normal and applicability re-runs — for NON-generic candidates this is
tsc's full context-sensitive re-check and works completely at M4; for
generic candidates tsc re-infers first (L76843) — we skip the re-infer
(stub) and the result escapes per the observability rule anyway.

## §1 Forcing map (what 5.7 turns on)

- expr.rs worker: CallExpression (checkMode via fallthrough) /
  NewExpression → checkCallExpression (L77607); TaggedTemplate →
  checkTaggedTemplateExpression (L77854); import-call arm →
  checkImportCallExpression (L77718).
- jsx.rs: the L74804 escape lifts —
  checkJsxOpeningLikeElementOrOpeningFragment completes; both deferred
  workers finish (closing tag, children); fragments resolve;
  checkJsxAttributes' 5.7-escaped worker arm un-escapes.
- operators.rs: instanceof callable-hasInstance shape → resolveCall
  with the 2860 head (the 5.5e escape lifts).
- access.rs: checkNonNullTypeWithReporter Invoke flavor 2721/2722/2723
  goes live via resolveCallExpression ([FLOW M5] narrowable-receiver
  containment gate applies unchanged).
- contextual.rs: getContextualTypeForArgument/AtIndex full port;
  tagged-template substitution arm; JSX opening ForArgumentAtIndex
  fallback; IIFE parameter arm.
- check.rs: deferred Call/New/TaggedTemplate/JsxOpeningElement +
  instanceof-Binary arms → resolveUntypedCall (L86923-28, 86960-64).
- NOT forced at 5.7 (re-mark seams, don't delete): Decorator resolution
  (forcing = checkDecorators, 5.8; resolveDecorator + 1238-1241/1329/
  1278/1279 + getDecoratorCallSignature L78699 recorded for 5.8);
  super-call base-constructor path (constructor bodies unchecked until
  5.8 — but see §6 super arm: the error/any shapes ARE reachable);
  candidatesOutArray/IsForSignatureHelp/apparentArgumentCount (LSP —
  always None/false, port as constant fields with notes); JS-file arms
  (JSDoc-class-tag, expando, require, AnyDefault inference flag).

## §2 getResolvedSignature (L77491) + the links discipline

resolveSignature dispatch (L77472): Call → resolveCallExpression, New
→ resolveNewExpression, TaggedTemplate → resolveTaggedTemplateExpression,
Decorator → [5.8 escape], JsxOpeningFragment/JsxOpeningElement/
JsxSelfClosingElement → resolveJsxOpeningLikeElement, BinaryExpression
→ resolveInstanceofExpression; exhaustive match, assertNever else.

Cache protocol (L77491-77508), transcribe exactly:
```
cached = links.resolvedSignature
if cached && cached != resolvingSignature && !candidatesOutArray: return cached
save = resolutionStart; if !cached: resolutionStart = resolutionTargets.len()
links.resolvedSignature = resolvingSignature          // SENTINEL WRITE
result = resolveSignature(node, candidatesOutArray, checkMode)
resolutionStart = save
if result != resolvingSignature:
    links.resolvedSignature = (flowLoopStart == flowLoopCount) ? result : cached
return result
```
- links.resolvedSignature is the SAME NodeLinks field
  getSignatureFromDeclaration uses (disjoint node kinds — mirrors tsc).
  The sentinel→final transition is a REWRITE: route it through a
  LinkSlot resolving-state (the resolved_return_type pattern), NOT the
  write-once panic path.
- flowLoopStart == flowLoopCount: both 0 until M5 → always cache. [FLOW
  M5] note on the field reads.
- The resolutionStart save/restore scopes the 5.1 cycle guard so outer
  in-flight resolutions don't poison call caching.
- resolveCall's mid-flight early return (L76621-25): after the two
  chooseOverload passes, if links.resolvedSignature was concretely set
  by a re-entrant resolution (context-sensitive arg → getContextualType
  → ForArgumentAtIndex → getResolvedSignature(same node)) and
  !candidatesOutArray → return links.resolvedSignature. Transcribe.
- The sentinel READ valve: getContextualTypeForArgumentAtIndex L72918
  `links.resolvedSignature === resolvingSignature ? resolvingSignature
  : getResolvedSignature(callTarget)` — during resolution, contextual
  reads see the sentinel (params empty → getTypeAtPosition → None-ish),
  never re-enter. This is the "anySignature save/restore shape" m4-55
  mentioned for IIFEs.
- Failure ordering (L76629-30): links.resolvedSignature =
  getCandidateForOverloadFailure result BEFORE error reporting — later
  arg re-checks (deferred resolveUntypedCall, contextual reads) see the
  failure candidate's param types. Load-bearing for parity.

Singletons (create at state init, L47220-47275): anySignature and
resolvingSignature (return anyType), unknownSignature (return
errorType), silentNeverSignature (return silentNeverType); all
declaration-less, no params, minArg 0. Verify exact return types
against the bundle when landing.

## §3 resolveCall (L76579)

reportErrors = !isInferencePartiallyBlocked && !candidatesOutArray →
constant true at M4 (port both as fields, note M6/LSP).

- typeArguments: skipped entirely for decorator/instanceof/super-call/
  jsx-fragment; checkSourceElement of each EXCEPT super-expression
  calls (L76593-98).
- reorderCandidates (L75768): transcribe whole (lastSymbol/lastParent
  runs, cutoffIndex, specialized-first splice via
  signatureHasLiteralTypes flag, callChainFlags →
  getOptionalCallSignature wrap). getSymbolOfDeclaration =
  getMergedSymbol — the L2 bug class. getOptionalCallSignature
  (L57895): per-signature 2-slot cache (inner/outer), cloneSignature +
  flag; consumed by getReturnTypeOfSignature (L59816-20: IsInnerCallChain
  → addOptionalTypeMarker, IsOuterCallChain → getOptionalType) —
  verify/add those two arms in the existing return-type resolver.
- Empty candidates (non-fragment): 2346 via getDiagnosticForCallNode →
  resolveErrorCall. Fragment skips the check (L76601).
- getEffectiveCallArguments → §4. isSingleNonGenericCandidate;
  argCheckMode init; signatureHelpTrailingComma = false always.
- Pass 1 subtype (only when candidates>1), pass 2 assignable — both
  relations exist since M3.
- Failure ladder (L76631-42), in order:
  1. candidatesForArgumentError, len 1 or >3: re-run applicability on
     the LAST failed candidate with reportErrors=true and a chain
     closure — >3 wraps 2770 then 2769; headMessage (instanceof 2860 /
     decorator) chains outermost; related 2793 via
     addImplementationSuccessElaboration; related "last overload is
     declared here" when >3.
  2. len 2-3: re-run EACH with a 2772 chain (`Overload i of N`),
     collect diag arrays; max errors > 1 → the min-error candidate's
     diags, else flatten all; wrap in 2769 (+headMessage); if every
     diag shares one span → synthesize the diagnostic AT that span
     with chain.code (2769), else at getErrorNodeForCallNode; related
     info = all flattened relatedInformation + 2793.
  3. candidateForArgumentArityError → getArgumentArityError.
  4. candidateForTypeArgumentError → checkTypeArguments(reportErrors=
     true, headMessage).
  5. else (non-fragment): filter signatures by typearg arity; none fit
     → getTypeArgumentArityError; else getArgumentArityError over the
     fits.
- addImplementationSuccessElaboration (L76744): save/restore the three
  error slots; overload with a body-bearing impl decl → chooseOverload
  against the impl signature → success adds related 2793. Runs real arg
  checks; duplicate expression diags collapse via exact-dedupe.
- STUB INTERACTION: candidates that failed only under stub
  instantiation never enter candidatesForArgumentError (§0) — the
  ladder then reports arity/typearg errors normally or, when arity fit
  but stub args failed, the whole resolution escaped before the ladder.
- Spans: getDiagnosticForCallNode (L76381) — CallExpression → callee
  NAME span (getDiagnosticSpanForCallNode L76376: property-access
  callee → `.name`, else the callee; getErrorSpanForNode); non-call
  nodes → node span. getErrorNodeForCallNode (L76395): call/new →
  callee (name of property access), tagged → tag (name), jsx → tagName.

## §4 chooseOverload + applicability + arity + effective args

chooseOverload (L76763) — transcribe, including: three error-slot
resets at entry; single-non-generic fast path (explicit typeargs
present → None immediately; arity fail → None; applicability fail →
candidatesForArgumentError=[c]); per-candidate loop (typearg arity →
arity → typeargs-or-infer → instantiate → non-array-rest re-arity →
applicability → re-run block → memo write `candidates[i] =
checkCandidate` → return).

- checkTypeArguments (L76043): fillMissingTypeArguments over
  map(getTypeFromTypeNode); per node-arg constraint →
  checkTypeAssignableTo(typeArg, instantiated constraint w/
  getTypeWithThisArgument, node, 2344-or-headMessage, chain) — silent
  (reportErrors=false) during selection, real on the failure ladder.
- getSignatureApplicabilityError (L76194): errorOutputContainer with
  skipLogging=true — errors are RETURNED, never added during
  selection; the failure ladder's re-runs pass reportErrors=true and
  still collect via the container, then diagnostics.add each. Model as
  `-> Option<Vec<Diagnostic>>`.
  - JSX call-likes → checkApplicableSignatureForJsxCallLikeElement
    (§7) — return its container errors.
  - this-check (skip for new / super-property calls):
    getThisArgumentOfCall (L76277: instanceof → right; call/tagged →
    skipOuterExpressions callee, access-expression → its expression),
    getThisArgumentType (L75931: void when absent; checkExpression;
    instanceof-RHS raw; optional-chain-root → getNonNullableType;
    inner chain → removeOptionalTypeMarker), checkTypeRelatedTo w/
    2684 head.
  - per-arg loop: skip OmittedExpression; paramType =
    getTypeAtPosition; argType = checkExpressionWithContextualType(arg,
    paramType, None, checkMode); SkipContextSensitive →
    getRegularTypeOfObjectLiteral; errorNode = getEffectiveCheckNode
    (L76190: skipOuterExpressions Parentheses|Satisfies);
    checkTypeRelatedToAndOptionallyElaborate w/ 2345 head; on fail
    maybeAddMissingAwaitInfo (2773 related when the SOURCE awaits to
    the target — getAwaitedTypeOfPromise 5.5f kit) and return.
  - rest tail: getSpreadArgumentType(args, argCount, len, restType,
    None, checkMode) vs restType via checkTypeRelatedTo w/ 2345;
    errorNode: 0 rest-args → node, 1 → effective-check-node of it,
    n → SYNTHETIC spanning args[argCount].pos..last.end.
- hasCorrectArity (L75813) — transcribe whole: fragment → true; tagged
  → template-span count + unterminated-literal incompleteness;
  decorator → getDecoratorArgumentCount (ES default:
  min(max(paramCount,1),2); legacy table L76359 behind
  experimentalDecorators); instanceof-Binary → 1; JSX opening →
  attributes.end==node.end incomplete → true, else the 0/1-arg clamp
  quirk (L75838-40); argument-less `new` → minArgs==0; else spread
  fast path (spreadIndex >= minArgs && (rest || spreadIndex <
  paramCount)), callIsIncomplete = arguments.end==node.end, over-max
  reject, under-min → per-missing-position filterType acceptsVoid
  (JS+nonstrict variant) Never test. Kit
  (getParameterCount/getMinArgumentCount/hasEffectiveRestParameter/
  getTypeAtPosition/tryGetTypeAtPosition/getNonArrayRestType) exists in
  structural.rs (5.3c).
- getEffectiveCallArguments (L76295) — **EffectiveArg enum mandate**:
  tsc fabricates SyntheticExpression parse nodes; the Rust port carries
  `EffectiveArg::Node(NodeId) | Synthetic { pos, end, ty, is_spread,
  tuple_name_source }` instead of appending arena nodes. Consumers:
  checkExpression(Synthetic) ≡ checkSyntheticExpression (L73946:
  isSpread → getIndexedAccessType(ty, numberType) else ty);
  isSpreadArgument (SpreadElement node | synthetic is_spread); arity;
  applicability; spans (pos/end); contextual indexOf (Node equality);
  the deferred resolveUntypedCall walks the RAW node arguments, not
  effective args.
  - fragment → [Synthetic(emptyFreshJsxObjectType)]; tagged →
    [Synthetic(getGlobalTemplateStringsArrayType — global type, 2318
    fallback under noLib)] + span expressions; decorator → 5.8;
    instanceof → [left]; JSX opening → [attributes] when properties
    or children nonempty, else [];
  - spread expansion: args before first spread kept; each spread arg:
    checkExpressionCached(expression) (flowLoopCount==0 at M4); tuple
    type → per-element Synthetic (elementFlags Rest → createArrayType
    wrap; Variable bits → is_spread; labeledElementDeclarations →
    tuple_name_source), else the raw arg.
- getSpreadArgumentType (L76002): last-spread fast path (synthetic →
  its ty; else checkExpressionWithContextualType(expression, restType,
  None, mode)); isArrayLikeType → getMutableArrayOrTupleType (L75993:
  union → mapType; any/mutable-or-constraint → self; tuple →
  readonly-strip clone; else single-Variadic tuple); non-array-like →
  checkIteratedTypeOrElementType(Spread) — [ITER] 5.5c kit, escapes
  where it escapes; multi-element tuple synthesis loop (Variadic/Rest
  per spread-ness; contextual per index: tuple restType →
  getContextualTypeForElementExpression, else
  getIndexedAccessType(restType, i, Contextual); const-context or
  primitive-ish contextual → getRegularTypeOfLiteralType else
  getWidenedLiteralType; names from tuple_name_source) →
  createTupleType(+readonly under const rest).

## §5 Failure candidates (L76871)

- getCandidateForOverloadFailure: checkNodeDeferred(node) ALWAYS — the
  deferred pass runs resolveUntypedCall(node): plain checkExpression of
  each RAW argument, which now picks up contextual types from the
  stashed failure candidate (§2 ordering). This is how args inside
  failed calls get their errors (and context-sensitive params their
  types) — do not shortcut it.
- hasCandidatesOutArray || len==1 || any-generic →
  pickLongestCandidateSignature (L76924): getLongestCandidateIndex
  (first with rest or params>=argc, else max params);
  apparentArgumentCount None at M4; generic + explicit typeargs →
  createSignatureInstantiation over getTypeArgumentsFromNodes (L76936:
  map getTypeOfNode, pop excess, fill default → constraint →
  getDefaultTypeArgumentType = unknown/any-js); generic w/o typeargs →
  inferSignatureInstantiationForOverloadFailure = **M6-stub site #2**
  (stub types; the instantiated candidate feeds ERROR SELECTION and
  the resolvedSignature stash only — observability rule applies to
  anything downstream).
- else → createUnionOfSignaturesForOverloadFailure (L76876): combined
  this/params via createSymbolWithType clones + union(Subtype) of
  per-position types (short signatures contribute their rest tail);
  min/max non-rest scan; union'd rest array when any; return type =
  INTERSECTION of returns; flags |=
  IsSignatureCandidateForOverloadFailure (+HasRestParameter/
  HasLiteralTypes).
- getArgumentArityError (L76434): spread present → 2556 AT the spread
  arg (plain node diagnostic); scan min/max/maxBelow/minAbove +
  closestSignature; parameterRange `min-max` string when unequal &&
  no-rest; isPromiseResolveArityError (L76407: callee ident resolves to
  a parameter of a fn-expression directly under `new
  <globalPromiseSymbol>`) → 2794 (TS flavor; 2810 JS);
  - between min and max → 2575 (chained under headMessage when given)
    at call-node span;
  - under min → 2554 (2555 when rest; decorator flavors 1278/1279) at
    getDiagnosticForCallNode span, + related "argument for X not
    provided" (related info — attach, not a separate row);
  - over max → span = the excess args slice pos..end (end==pos → +1),
    createDiagnosticForNodeArray 2554/2555.
- getTypeArgumentArityError (L76521): single signature → 2558 on the
  typeArguments RANGE; multiple → bracketed counts → 2743, else 2558
  with the boundary count.

## §6 Per-kind resolvers + workers

- resolveCallExpression (L76972):
  - super arm: checkSuperExpression (5.5a); any → check args,
    anySignature; NOT errorType → containing-class base-type-node →
    getInstantiatedConstructorsForTypeArguments → resolveCall — that
    path needs constructor-body forcing (5.8): mark Unsupported
    "super base constructors (5.8)"; errorType/no-base →
    resolveUntypedCall (REACHABLE now via top-level `super()` recovery
    — checkSuperExpression already errored).
  - call-chain flags: isCallChain → getOptionalExpressionType (access
    kit) vs funcType; changed → isOutermostOptionalChain ? Outer :
    Inner (L76990-98).
  - checkNonNullTypeWithReporter + Invoke reporter (2721/2722/2723,
    [FLOW M5] containment gate); silentNever passthrough
    (silentNeverSignature); apparent errorType → resolveErrorCall.
  - isUntypedFunctionCall (L77052: any funcType; apparent-any from
    TypeParameter; zero sigs && non-union && non-never &&
    assignable-to-globalFunctionType) → 2347 when typeArguments on a
    non-error target → resolveUntypedCall.
  - no call signatures: construct sigs present → 2348 `error(node)`;
    else optional related 2734 (single arg starting on a new line —
    scanner probe) + invocationError(callee, Call) → resolveErrorCall.
  - JS @class tag arm → skip (JS). SkipGenericFunctions arm → M6-dead
    shape. → resolveCall(callSignatures, chainFlags).
- invocationError family (L77167-77258): union targets → constituent
  chains (2755 no-constituent / 2756+per-type 2349-chain rows /
  2758 mutually-incompatible; construct flavor 2759/2760/2762); plain →
  "Type X has no call signatures" chained under head 2349 (call) /
  2351 (construct); zero-arg call of a get-accessor-resolved symbol →
  head 6234 (reads links.resolvedSymbol of the callee); span override:
  callee within a CallExpression parent → getDiagnosticSpanForCallNode
  span (callee NAME); related: 2773 await hint when awaited type has
  sigs; invocationErrorRecovery → related 7038 for namespace-import
  targets (originatingImport symbol link — module band; if the link is
  unmodeled the related row is absent: attach-only, safe).
- resolveNewExpression (L77055): checkNonNullExpression; silentNever;
  apparent; any → (2347 if typeargs) untyped; construct sigs →
  isConstructorAccessible (L77138: private → 2673, protected → 2674
  unless typeHasProtectedAccessibleBase L77108 through first base
  intersection/mixin walk) → abstract signature (someSignature
  composite walk L77102) or abstract class modifier → 2511 →
  resolveCall; else call sigs → resolveCall then `!noImplicitAny` →
  2350 (non-void return, non-JS-ctor) / 2679 (void this) — DEAD under
  the strict default, live under noImplicitAny:false directives; else
  invocationError(Construct) → error call.
- checkCallExpression worker (L77607): checkGrammarTypeArguments (1009
  trailing comma / 1099 empty list); getResolvedSignature;
  resolving-sentinel → silentNeverType (M6-dead); checkDeprecatedSignature
  (6387 suggestion — only if declaration Deprecated flag is modeled;
  else no-op note); super → voidType; NewExpression + declaration not
  ctor/ctor-sig/ctor-type → noImplicitAny 7009 → anyType (LIVE under
  strict default); JS require arm skip; returnType; ESSymbolLike &&
  isSymbolOrSymbolForCall (L77692: `Symbol(...)`/`Symbol.for(...)`
  against the global ESSymbol ctor) → getESSymbolLikeTypeForNode
  (L63117: isValidESSymbolDeclaration positions → per-symbol
  uniqueESSymbolType links cache) — MUST port with this band: without
  it `const s: unique symbol = Symbol()` renders a 2322 FP; void-return
  + type-predicate assertion-position checks (2776/2775) — signature
  type predicates are UNMODELED at M4 → arm provably dead, note +
  residual (predicates port with M5 getEffectsSignature work); JS
  expando arm skip; → returnType.
- checkTaggedTemplateExpression (L77854): checkGrammarTaggedTemplateChain
  (1358 optional-chain) else checkGrammarTypeArguments; helper check
  skipped (ES2025 ≥ TaggedTemplates); getResolvedSignature →
  deprecation → return type. resolveTaggedTemplateExpression (L77259):
  tag check; apparent error → errorCall; untyped → untypedCall; no call
  sigs → parent-array-literal comma hint 2796 AT tag → errorCall, else
  invocationError(Call) → errorCall; → resolveCall.
- checkImportCallExpression (L77718): checkGrammarImportCallExpression
  (L90428: verbatimModuleSyntax row dead at ESNext-module; import.defer
  → 18060 unless ESNext/Preserve; ES2015-module → 1323; typeargs →
  1326; non-Node16+/ESNext/Preserve → trailing-comma + second-arg 1324;
  count 0 or >2 → 1450; spread → 1325 — gate on the modeled moduleKind
  default, verify options.rs); empty args → Promise<any>; specifier
  checkExpressionCached → 7036 on Undefined/Null flag or
  !assignable-to-string; options arg vs getGlobalImportCallOptionsType
  | undefined (plain checkTypeAssignableTo → 2322 rows live); `assert`
  key → 2880; resolveExternalModuleName → SILENT None stub (module
  resolution is unmodeled — fabricating 2307 here would FP on
  multi-file fixtures; resolvable-module return types stay Promise<any>
  = FN-safe) → createPromiseReturnType(anyType) (5.5f kit).
- resolveInstanceofExpression (L77445): right/probe sides exist (5.5e);
  hasInstance-method type → apparent error → errorCall; untyped →
  untypedCall; call sigs → resolveCall (headMessage injected at the
  resolveCall failure ladder: 2860); operators.rs then finishes
  checkInstanceOfExpression's tail — re-read L79558-79577 when landing
  (the 2861-adjacent boolean check; errorType/any pass silently per the
  5.5e comment).
- resolveUntypedCall (L75747): typeargs checkSourceElement
  (callLikeExpressionMayHaveTypeArguments); operand checks by kind
  (template / attributes / left / RAW arguments); anySignature.
  resolveErrorCall = resolveUntypedCall + unknownSignature.

## §7 JSX band

- resolveJsxOpeningLikeElement (L77397):
  - intrinsic tag: getIntrinsicAttributesTypeFromJsxOpeningLikeElement
    (L74728 — ports WITH getIntrinsicTagSymbol per m4-55 §9:
    links.resolvedSymbol + jsxFlags Named/Indexed, 2339 w/
    "JSX.IntrinsicElements" arg, 7026 noImplicitAny;
    links.resolvedJsxElementAttributesType cache) →
    createSignatureForJSXIntrinsic (L77332) — port DECLARATION-LESS:
    transient `props` parameter symbol typed by the intrinsic
    attributes type, return = JSX.Element declared type | errorType,
    minArg 1 (tsc fabricates a FunctionTypeNode only for display —
    T2); then checkTypeAssignableToAndOptionallyElaborate(attributes
    checked against effective-first-arg, errorNode=tagName,
    expr=attributes) + explicit typeargs → checkSourceElement each +
    2558(0,n) on the range → return fake signature (resolveCall NOT
    entered).
  - fragment: getJSXFragmentType (L77372: per-sourcefile links cache;
    only jsx===React or jsxFragmentFactory-set resolves — else
    anyType; resolveName miss → 2879 + errorType; React.Fragment
    member lookup).
  - value tag: checkExpression(tagName); apparent error → errorCall;
    getUninstantiatedJsxSignaturesOfType (L74659: String → [any-sig];
    StringLiteral → intrinsic-by-name (miss → 2339 vs
    JSX.IntrinsicElements) → fake sig; construct-then-call sigs; union
    → getUnionSignatures over per-constituent recursion); untyped →
    untypedCall; none → 2604 AT tagName (fragment flavor AT node) →
    errorCall; → resolveCall.
- checkApplicableSignatureForJsxCallLikeElement (L76088): paramType =
  getEffectiveFirstArgumentForJsxSignature (L73648: fragment or
  refKind != Component → call-props (first param type || unknown;
  LibraryManagedAttributes instantiation via
  instantiateAliasOrInterfaceWithDefaults L74768; ∩ IntrinsicAttributes)
  else class-props (getJsxElementPropertiesName over
  ElementAttributesProperty: absent → first param, "" → return type,
  name → member of return type w/ composite intersection; +
  IntrinsicClassAttributes; read L73708 tail when landing));
  attributesType = fragment → createJsxAttributesTypeFromAttributesProperty
  (5.5f worker — verify its fragment flavor) else
  checkExpressionWithContextualType(attributes, paramType, None,
  checkMode); SkipContextSensitive → regular-of-object-literal;
  checkTagNameDoesNotExpectTooManyArguments (L76109: implicit-import
  container → pass; tag call sigs + getJsxFactoryEntity resolved to a
  VALUE (ignoreErrors) + factory first-param sigs → min tag args vs
  max factory-provided params → 6229 + related decl) &&
  checkTypeRelatedToAndOptionallyElaborate(checkAttrType, paramType,
  relation, tagName (fragment: node), attributes, no head, chain,
  container).
- checkJsxOpeningLikeElementOrOpeningFragment tail (L74804-24):
  getResolvedSignature; deprecation; opening-like only:
  getJsxElementTypeTypeAt (JSX.ElementType — absent in most fixture
  namespaces → undefined) → present: tagType (intrinsic → string
  literal of the name; else checkExpression(tagName)) relatedTo the
  constraint, error "Its type X is not a valid JSX element type"
  chained under 2786; absent: checkJsxReturnAssignableToAppropriateBound
  (L74698: Function → return type vs (JSX.Element | null) w/ 2787
  chained under 2786; Component → instance vs JSX.ElementClass w/
  2788; Mixed → union of both w/ 2789; each constraint absent → skip).
  getJsxReferenceKind L76075.
- Deferred completion: element deferred = opening + closing tag
  (intrinsic → getIntrinsicTagSymbol, else checkExpression(tagName)) +
  checkJsxChildren; self-closing = opening only; fragments complete via
  the eager worker. 2602 stays DEAD (getJsxType returns errorType,
  never undefined — pinned 5.5f). 17004/grammar rows already fire
  pre-escape; keep order.

## §8 Contextual un-escapes (5.5b/5.5f leftovers)

- getContextualTypeForArgument (L72906): full — effective args +
  position of the arg (EffectiveArg::Node equality) → AtIndex.
- getContextualTypeForArgumentAtIndex (L72911): import-call arm already
  live; SENTINEL valve (§2); JSX opening + argIndex 0 →
  getEffectiveFirstArgumentForJsxSignature; rest-param positions →
  getIndexedAccessType(restParamType, literal(argIndex-restIndex),
  Contextual); else getTypeAtPosition.
- Tagged-template substitution arm (L72929) → ForArgument on the
  tagged parent.
- IIFE parameter arm in getContextuallyTypedParameterType (L72687
  region) — re-read the exact IIFE shape when landing (m4-55 recorded
  a resolvedSignature=anySignature save/restore); the sentinel valve
  makes the naive port safe.
- Decorator contextual arm: stays None (5.8) — re-mark.
- JSX opening ForArgumentAtIndex fallback (contextFlags !== 4 strict
  compare quirk) — wire to AtIndex.

## §9 Rust seam inventory

- NEW calls.rs (est. 3.5-4.5k lines): §§2-6 + EffectiveArg + arity/
  span helpers. jsx.rs grows §7 (~1k).
- expr.rs: three worker stubs → live arms (checkMode fallthrough only
  to Call/New); import-call arm → worker.
- check.rs: deferred call arms → resolveUntypedCall; instanceof
  deferred arm.
- state.rs: 4 signature singletons; is_inference_partially_blocked
  (false, M6), apparent_argument_count (None, LSP-dead) fields.
- links.rs: resolved_signature call-node protocol (Resolving
  transition — reuse the LinkSlot resolving pattern, NOT a new field);
  resolved_jsx_element_attributes_type; per-sourcefile
  jsx_fragment_type.
- Signature: optional_call_signature_cache (inner/outer 2-slot).
- structural.rs: getReturnTypeOfSignature call-chain flag arms (verify/
  add); getSignaturesOfType/getPropertyOfType function-AUGMENT re-audit
  (5.5d survivor (a)) — call results now flow into member access.
- contextual.rs: §8 arms. operators.rs: instanceof lift + 79558 tail.
  access.rs: Invoke reporter wiring.
- annotate.rs (or expr.rs): getESSymbolLikeTypeForNode +
  isValidESSymbolDeclaration + isSymbolOrSymbolForCall; SymbolLinks
  unique_es_symbol_type (verify existing).
- Grammar: checkGrammarTypeArguments / TaggedTemplateChain /
  ImportCallExpression (js_grammar.rs or calls.rs).
- Syntax predicates to verify/add: isSuperCall,
  callLikeExpressionMayHaveTypeArguments, isCallChain/
  isOutermostOptionalChain (node_util has the chain kit).

## §10 New state summary

CheckerState: singletons + two constant fields. NodeLinks: the
resolving-transition on resolved_signature; JSX links above.
SymbolLinks: unique_es_symbol_type (if absent). Signature: the
optional-call cache. NO type-predicate modeling this stage.

## §11 FP=0 risk register (pin at each slice)

1. **Stub observability leaks** — the §0 rule is the FP wall: any
   stub-filled instantiation result or arg-verdict that reaches
   diagnostics or a cached type is a latent wrong-payload FP. Audit
   every path out of chooseOverload/pickLongestCandidateSignature; pin
   `identity(1)` (contain — statement FN), `f<number>("x")` (2345
   live), generic-arity `f<T>(a:T,b:T); f(1)` (2554 live), callback
   contravariance `declare function g<T>(f:(x:T)=>void):void;
   g((x:number)=>{})` (contain, NOT 2345).
2. **Deferred re-check divergence**: failed calls re-check args plainly
   with candidate-fed contextual types; context-sensitive params
   assigned once (ContextChecked) — the symbolLinks once-write panic is
   the tripwire. Pin `f((x)=>x.foo)` under arity failure vs oracle
   (expects candidate-typed x, NOT 7006).
3. **Code identity pairs**: 2348-vs-2349 (construct sigs present);
   2554-vs-2555 (rest); 2575-vs-2554 (between-range); 2558-vs-2743
   (multi-sig brackets); 2350/2679 dead under strict default (pin a
   noImplicitAny:false fixture); 7009 live under default. Wrong member
   = set-compare FP.
4. **Span discipline**: 2554-family at callee-NAME span; over-arity at
   the excess-args range (end==pos bump); 2558/2743 on the typeArguments
   range; 2556 AT the spread arg; 2796 AT the tag; invocationError
   span-override inside call parents; 2604 at tagName. Pin each shape.
5. **checkTypeArguments double-run**: silent during selection, emitting
   only on the failure ladder; a passing sibling candidate keeps 2344
   silent. Don't emit from the silent pass.
6. **JSX fake signatures**: declaration-less is display-only (T2);
   assert no T0 path dereferences the declaration. Intrinsic path
   returns WITHOUT resolveCall — don't route it through the ladder.
7. **Import calls**: no 2307 fabrication (silent module-resolution
   stub); grammar rows gated on the ACTUAL modeled moduleKind default —
   verify options.rs before wiring 1323/1324.
8. **Union invocation chains**: 2755/2756/2758(2759/2760/2762) are
   chain HEADS on one diagnostic — one row, not per-constituent rows.
9. **Optional-chain calls**: chain flags adjust RETURN optionality via
   getReturnTypeOfSignature arms — missing arms silently drop
   `undefined` from result unions (downstream FP/FN both possible).
   Pin `a?.b()` result-type consumers.
10. **Dedupe reliance**: failure-ladder re-runs and impl-success
    elaboration re-check args → duplicate expression-internal diags
    must be EXACT duplicates (existing insertSorted-equality dedupe).
    A contextual-type-dependent difference across runs would double-
    report — the §0 escape covers the stub case; non-generic overload
    re-runs mirror tsc exactly.
11. **TemplateStringsArray under noLib**: unit pins for tagged
    templates are lib-gated or tolerate 2318 (conformance is
    lib-loaded).

## §12 Slicing + sequencing (commits `m4 5.7a-c`)

- **5.7a — core band**: singletons + links protocol + EffectiveArg +
  reorderCandidates + arity kit + effective args/spread +
  checkTypeArguments + applicability + resolveCall/chooseOverload +
  failure band + resolveCallExpression/resolveNewExpression/
  untyped/error calls + invocationError family + checkCallExpression
  worker (minus import/JS arms) + deferred arms + M6-stub +
  expr.rs Call/New un-stub + contextual ForArgument/AtIndex + Invoke
  reporter + call-chain flags + return-type chain arms + super
  error-shape arm. Codes: 2346 2347 2348 2349(6234/2755/2756/2758)
  2351(2759/2760/2762) 2350 2679 2511 2673 2674 7009 2554 2555 2556
  2575 2344 2558 2743 2345 2769 2770 2772 2684 2721 2722 2723 2794
  (+related 2734/2773/2793/7038).
- **5.7b — tagged/import/instanceof/symbol tail**: tagged-template trio
  (1358 grammar + 2796 + TemplateStringsArray effective arg) + import
  call (grammar band + 7036 + options 2322 + 2880 + silent resolution
  stub) + instanceof completion (2860 head + operators tail) +
  unique-symbol tail (getESSymbolLikeTypeForNode) + IIFE contextual arm
  + tagged substitution arm + deprecation decision (flag audit).
- **5.7c — JSX band**: getIntrinsicTagSymbol +
  intrinsic-attributes/fake signatures + getUninstantiatedJsxSignatures
  + resolveJsxOpeningLikeElement + fragment type +
  applicability-for-JSX + effective-first-arg (managed/class props) +
  post-resolution tail (ElementType / appropriate-bound 2786-2789) +
  closing-tag/children completion + checkJsxAttributes worker
  un-escape + factory arity 6229 + 2879. Codes: 7026 2339-intrinsic
  2604 2558-jsx 2786 2787 2788 2789 6229 2879 (+"valid JSX element
  type" row — number from gen.rs at landing).

Per-commit gates unchanged: cargo test workspace; relpin regen /0;
ledger append-only; invariants idempotence; conformance FULL
(lib-loaded) — T0 rate in commit body, **FP=0 absolute**; ratchet
bump; new-code oracle pins land WITH the commit (scratchpad pin.sh +
probe.sh recreated per memory). Expect modest rate movement from 5.7a
(non-generic/arity/typearg bands: 2345 488 + 2554/2555 + 2349/2351
FN rows), 7026's 612 rows at 5.7c; generic-inferred calls stay
contained until M6 by design.

Cross-references: [[tsrs2-m4-checker]] memory (session state);
checker-key §3 (skeletons); m4-55 §0/§11 (stub policy, risk
discipline); m6-inference-calls-steps.md (the swap contract consumer —
flag the §0 constraint-enrichment note there when M6 starts).
