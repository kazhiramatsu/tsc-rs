# M4 stage 5.8: statements + declarations — semantic extraction

Extracted 2026-07-14 from the vendored bundle
`tsrs2/vendor/typescript-6.0.3/lib/_tsc.js` (all `L`-anchors are lines
in THAT file; re-grep on re-vendor). Parents: skeleton-steps §5.8
(scope + the RE-ENTRANCY TRAP), m4-55 §0 (stub-policy tags), m4-57
(links/EffectiveArg discipline — carried forward). Implementers start
HERE. This doc's slicing (§15, commits `m4 5.8a-…`) supersedes the
steps doc's single-commit line.

Status: EXTRACTION COMPLETE (§§0-15). Implementers start at §15's
slicing; §12 is the landing-time transcription CHECKLIST (functions
whose bodies transcribe when their first caller lands — everything
else in this doc is already implementation-grade). Review round
2026-07-14 folded in: decorator DUAL-MODE (experimentalDecorators is
modeled + mapped — §10), the OPTIONS AUDIT (§13: present / add-in-5.8
/ absent-dead, with fixture counts), ES5 as MANDATORY scope (807
fixtures — every languageVersion<ES2015 arm is live 5.8 scope, not
floor-verify), and the comment-directive gap scoped to the EXISTING
interim filter (§15 5.8e).

Diagnostic codes: message NAMES are authoritative here (they map 1:1
onto `tsrs2_diags::gen` statics); numbers are cited where load-bearing
for code-identity risks. Resolve any uncited number from gen.rs at
landing — never guess.

## §0 Driver contract — what 5.8 turns on

5.8 replaces `source_element_stub` arms in check.rs with their tsc
workers. The driver shell (5.4) is already tsc-shaped: two-phase
(eager statements in source order, then the deferred drain),
per-element Unsupported containment, eager addLazyDiagnostic identity.
What changes:

- **checkSourceFileWorker tail (L87003-87061)** completes. Exact
  order after the statement loop + EOF token + checkDeferredNodes:
  1. external/CJS module → registerForUnusedIdentifiersCheck (M7-inert);
  2. addLazyDiagnostic block: the unused-identifiers drain
     (noUnusedLocals/Parameters absent → inert, M7) AND
     `checkPotentialUncheckedRenamedBindingElementsInTypes()` for
     non-declaration files — NOT option-gated, goes LIVE at 5.8
     (L83180; pushers come from checkVariableLikeDeclaration §2);
  3. external/CJS module → checkExternalModuleExports (§8);
  4. the four collision drains IN ORDER: potentialThisCollisions →
     potentialNewTargetCollisions → potentialWeakMapSetCollisions →
     potentialReflectCollisions (§2 collisions band).
  The 5 potential* vectors are CheckerState fields cleared per file at
  worker entry (the PartiallyTypeChecked restore block stays elided —
  nodesToCheck unported).
- **Statement arms**: Block/ModuleBlock (live), VariableStatement,
  If/Do/While/For/ForIn/ForOf, Break/Continue, Return, With, Switch,
  Labeled, Throw, Try, VariableDeclaration, BindingElement (§§2-3).
- **Declaration arms**: FunctionDeclaration, Parameter, Property
  Declaration/Signature, Method, Constructor, Accessors, static
  blocks, TypePredicate, class/interface/typealias completions, Enum,
  Module, Import/Export family, MissingDeclaration (§§5-10).
- **Type-node arms**: TypeQuery, TypeLiteral, Array, Tuple,
  Union/Intersection, IndexedAccess, Mapped (grammar-only + M8-stub),
  This, TypeOperator, Conditional/Infer (M8-stub tail), Template
  Literal, Import, NamedTupleMember (§11). RE-ENTRANCY TRAP
  (skeleton-steps §5.8): any arm that FORCES ITS OWN node inherits
  the TypeReference overwrite requirement the moment its node kind
  can sit in a type-parameter default subtree — decide per arm:
  overwrite semantics or prove unreachable; the write-once panic is
  the tripwire.
- **checkDeferredNode arms that un-stub**: Decorator →
  checkDecorators-registered deferrals (§10); GetAccessor/SetAccessor
  deferred object-literal accessors → checkAccessorDeclaration (§5);
  ClassExpression → checkClassExpressionDeferred (§6). The
  FunctionExpression/Method deferred body path stops swallowing
  statements: deferred bodies now DRIVE (checkSourceElement of body
  statements reaches the new arms).
- **Grammar policy** unchanged: checkGrammarStatementInAmbientContext
  is live; checkGrammarSourceFile + checkGrammarModifiers stay
  M7-stub hooks — EXCEPT the specific checkGrammar* workers this doc
  names per band (they are 5.8-owned; each is cited where its caller
  ports). grammarErrorOnNode/AtPos/OnFirstToken remain suppressed
  when the file has parse diagnostics (hasParseDiagnostics gate) —
  per existing discipline.
- **skippedOn machinery** (L47575 errorSkippedOn): collision-band
  diagnostics carry `skippedOn: "noEmit"` — the program layer drops
  them when options.noEmit is set. Conformance fixtures DO set
  @noEmit. Port: a `skipped_on_no_emit` flag on Diagnostic + the
  program-layer filter, or FP on every noEmit fixture with a
  collision row. Verify the harness's noEmit mapping BEFORE wiring
  the collision band (risk §14.9).
- **errorOrSuggestion (L47604)**: isError=false → suggestion band
  (not in error baselines). Statement consumers: Unused_label
  (allowUnusedLabels unmodeled → constant-suggestion → SKIP with
  note), noFallthroughCasesInSwitch (option absent + needs M5
  isReachableFlowNode → dead, note both).

## §1 THE LATE-BINDING WALL (read this before slicing)

Empirical (probe 2026-07-14, throwaway tests/probe58.rs): an
interface carrying ONE late-bindable member (`[Symbol.iterator](): …`)
contains its ENTIRE member table today — `a.length as number` on such
an interface emits NOTHING (control without the computed member emits
the 2352). The escape is get_resolved_members_or_exports_of_symbol's
INTERFACE arm (annotate.rs ~2883, "late-bound INTERFACE members (lib
well-known-symbol surface, M7-stub)").

Consequence under lib-loaded conformance: at target ≥ ES2015 the lib
declares `[Symbol.iterator]`-family members on Array/String/Map/Set/
IterableIterator/… — EVERY member resolution on those types contains
(arr.push/str.charAt FN wholesale on modern-target fixtures). This
throttles all bands, not just iteration; and getIterationTypesOf*'s
slow path (§4) dies at the same wall (getPropertyOfType(T[],
"__@iterator@…") unwinds before the protocol runs).

History (commit aea5fd3, 5.7b review): lateBindMember itself IS
ported and live for type literals/classes/object literals. Interfaces
were deliberately kept contained — un-containing them unmasked 9
corpus FPs from three OTHER bands' recorded gaps:
1. [FLOW M5] assignment/instanceof narrowing
   (controlFlowAssignmentExpression,
   controlFlowInstanceofExtendsFunction);
2. @ts-ignore/@ts-expect-error comment directives
   (directives/multiline fixtures);
3. declare-global augmentation merges (symbolProperty61,
   nullPropertyName).

The 5.8 plan — **the lift is a 5.8 sub-slice, sequenced LAST**:
- (2) and (3) are addressable at 5.8: declare-global/augment merges
  are §8's module band; comment directives already have an INTERIM
  single-line filter (checker/src/lib.rs
  filter_by_comment_directives) — the lift slice EXTENDS/REPLACES it
  with scanner-backed collection (multi-line-comment directives are
  the recorded gap; the 5.7b FP fixtures were directives/multiline).
- (1) stays M5. The lift slice must re-run the 5.7b experiment
  (delete the INTERFACE escape, full conformance) and triage every
  new FP: FLOW-narrowing shapes get targeted [FLOW M5] containment
  gates (the 5.5d/5.5e "second face" pattern: gate the REPORT when a
  narrowable-reference divergence is in play), not a re-containment
  of the member table.
- If the FP triage exceeds the slice, fall back: keep the interface
  escape BUT add a well-known-symbol-only carve-out — late-bind
  members whose computed name resolves through the global
  SymbolConstructor (the getPropertyNameForKnownSymbolName shape,
  operators.rs port exists) and keep containing interfaces whose
  late-bindable names resolve elsewhere. That unlocks lib iteration
  + array/string member tables while keeping user-side dynamic-name
  interfaces contained. Decide by measurement, record in the ledger.
- Iteration (§4) lands BEFORE the lift and is exercised via
  class-typed/generator-typed iterables (fast path + class
  late-binding are live); the for-of-over-array recovery arrives
  WITH the lift.

## §2 Variable band

### checkVariableStatement (L83618)
`!checkGrammarModifiers(node) && !checkGrammarVariableDeclarationList(
node.declarationList) → checkGrammarForDisallowedBlockScopedVariableStatement(node)`;
then checkVariableDeclarationList. (checkGrammarModifiers stays the
M7-stub hook — its false return feeds the && chain; port the chain
shape so the grammar workers land in slot.)

- checkGrammarVariableDeclarationList (L90130): trailing-comma check;
  empty list → grammarErrorAtPos(declarations.pos,
  end-pos, Variable_declaration_list_cannot_be_empty); using/await
  using: for-in LHS → The_left_hand_side_of_a_for_in_statement_cannot_
  be_a[n]_[await_]using_declaration; direct child of case/default
  clause → [await_]using_declarations_are_not_allowed_in_case_or_
  default_clauses_unless_contained_within_a_block; ambient →
  …not_allowed_in_ambient_contexts; await-using → checkAwaitGrammar
  (L79338 — the 5.5f await-position grammar kit; verify the shared
  fn is exposed, else port with this band).
- checkGrammarForDisallowedBlockScopedVariableStatement (L90178):
  allowBlockDeclarations(parent) (L90163: false under if/do/while/
  with/for/for-in/for-of, recursing through LabeledStatement chains)
  → blockScopeKind ≠ 0 → **plain error() channel**
  _0_declarations_can_only_be_declared_inside_a_block with keyword
  "let"/"const"/"using"/"await using" AT the statement.
- checkVariableDeclarationList (L83611): using/await-using +
  languageVersion < UsingAndAwaitUsing → checkExternalEmitHelpers
  (importHelpers-gated → no-op unless the option lands; note, skip);
  forEach declarations → checkSourceElement.

### checkVariableDeclaration (L83600) / checkBindingElement (L83607)
checkGrammarVariableDeclaration + checkVariableLikeDeclaration;
checkGrammarBindingElement + checkVariableLikeDeclaration.

- checkGrammarVariableDeclaration (L90063): binding-pattern name +
  using/await-using → _0_declarations_may_not_have_binding_patterns;
  non-for-in/of parents: Ambient → checkAmbientInitializer (L90049:
  invalid unless string/number literal, `-`-prefixed numeric, bigint,
  true/false, or isSimpleLiteralEnumReference (L90044: property/
  element access over entity-name whose checkExpressionCached has
  EnumLike flags); const-or-readonly WITHOUT annotation → invalid →
  A_const_initializer_in_an_ambient_context_must_be_…; otherwise →
  Initializers_are_not_allowed_in_ambient_contexts); no initializer:
  pattern name (parent not pattern) → A_destructuring_declaration_
  must_have_an_initializer; await-using/using/const →
  _0_declarations_must_be_initialized("await using"|"using"|"const");
  exclamationToken outside (VariableStatement parent && annotated &&
  un-initialized && non-ambient) → Declarations_with_initializers_
  cannot_also…/Declarations_with_definite_assignment_assertions_must_
  also…/A_definite_assignment_assertion_is_not_permitted…(selection
  by initializer? → !type? → context); module format < System &&
  exported non-ambient statement → checkESModuleMarker (L90100:
  "__esModule" identifier (recursing into patterns) →
  grammarErrorOnNodeSkippedOn("noEmit", …esModule_is_reserved…));
  block-scoped → checkGrammarNameInLetOrConstDeclarations (L90115:
  identifier "let" → let_is_not_allowed_to_be_used_as_a_name…,
  recursive into pattern elements).
- checkGrammarBindingElement (L90023): rest not-last →
  A_rest_element_must_be_last_in_a_destructuring_pattern; trailing
  comma after rest → A_rest_parameter_or_binding_pattern_may_not_
  have_a_trailing_comma; rest + propertyName → A_rest_element_cannot_
  have_a_property_name (AT node.name); rest + initializer →
  grammarErrorAtPos(initializer.pos-1, 1, A_rest_element_cannot_have_
  an_initializer).

### checkVariableLikeDeclaration (L83403) — the core; transcribe
Order matters; every step:
1. checkDecorators(node) (§10 — parameters can carry decorators).
2. NOT BindingElement → checkSourceElement(node.type) — forces the
   annotation subtree (type-node arms §11).
3. No name → return (recovery).
4. ComputedPropertyName name → checkComputedPropertyName +
   (hasOnlyExpressionInitializer && initializer) →
   checkExpressionCached(initializer).
5. BindingElement arm:
   - propertyName && Identifier name && isPartOfParameterDeclaration
     && containing-fn body MISSING → push
     potentialUnusedRenamedBindingElementsInTypes; **return** (the
     early return is semantic). Drain (L83180, extracted): per
     recorded node, symbol NOT isReferenced (risk §14.16) →
     _0_is_an_unused_renaming_of_1_Did_you_intend_to_use_it_as_a_
     type_annotation AT node.name (+ when the wrapping parameter
     declaration is UN-annotated: related FILE diagnostic at
     wrappingDeclaration.end width 0, We_can_only_write_a_type_for_
     0_by_adding_a_type_for_the_entire_parameter_here).
   - computed propertyName → checkComputedPropertyName.
   - parentType = getTypeForBindingElementParent(parent.parent,
     dotDotDot ? RestBindingElement : Normal) (5.5b kit, L55824);
     name = propertyName || name; parentType && !isBindingPattern(
     name): exprType = getLiteralTypeFromPropertyName;
     isTypeUsableAsPropertyName → property = getPropertyOfType(
     parentType, nameText); if present: markPropertyAsReferenced
     (M7 no-op hook) + checkPropertyAccessibility(node,
     isSuper(parent.initializer), writing=false, parentType,
     property) — private/protected destructuring rows via the
     access.rs kit.
6. BindingPattern name → forEach(elements, checkSourceElement)
   (recurses into BindingElements; the ES-emit helper probes are
   importHelpers-gated no-ops).
7. initializer && isPartOfParameterDeclaration && body missing →
   error A_parameter_initializer_is_only_allowed_in_a_function_or_
   constructor_implementation AT node; return.
8. BindingPattern name (second arm):
   - isInAmbientOrTypeNode → return.
   - needCheckInitializer = hasOnlyExpressionInitializer &&
     initializer && parent.parent NOT ForInStatement;
     needCheckWidenedType = elements are ALL OmittedExpression
     (`!some(elements, not(isOmittedExpression))` — empty patterns).
   - either → widenedType = getWidenedTypeForVariableLikeDeclaration
     (5.6 kit); needCheckInitializer: initializerType =
     checkExpressionCached(initializer); strictNullChecks &&
     needCheckWidenedType → checkNonNullNonVoidType(initializerType,
     node) (access.rs port — its "5.8 declaration band consumers"
     note un-escapes HERE) else checkTypeAssignableToAndOptionally
     Elaborate(initializerType, getWidenedTypeForVariableLike…(re-
     computed), node, node.initializer); needCheckWidenedType:
     ArrayBindingPattern → checkIteratedTypeOrElementType(
     Destructuring=65, widenedType, undefinedType, node) [§4] else
     strictNullChecks → checkNonNullNonVoidType(widenedType, node).
   - return.
9. symbol = getSymbolOfDeclaration(node); Alias-flagged
   require-initialized shapes → checkAliasSymbol + return (JS —
   elide w/ note).
10. BigIntLiteral name → error A_bigint_literal_cannot_be_used_as_a_
    property_name AT name.
11. type = convertAutoToAny(getTypeOfSymbol(symbol)) (L83400:
    autoType→anyType, autoArrayType→anyArrayType — [FLOW M5]: the
    5.6 AUTO arm already answers anyType; port the twin as identity
    with the note).
12. node === symbol.valueDeclaration:
    - initializer (hasOnlyExpressionInitializer &&
      getEffectiveInitializer) && parent.parent NOT ForInStatement →
      initializerType = checkExpressionCached(initializer);
      **checkTypeAssignableToAndOptionallyElaborate(initializerType,
      type, node, initializer)** — THE annotated-declaration 2322
      row. errorNode = node (VariableDeclaration): verify
      getErrorSpanForNode's VariableDeclaration arm → the NAME span
      (pin `const x: string = 1` exact span).
    - blockScopeKind == AwaitUsing → globalAsyncDisposableType +
      globalDisposableType probes (reportErrors=true; noLib → 2318
      band) both ≠ emptyObjectType → checkTypeAssignableTo(
      widenTypeForVariableLikeDeclaration(initializerType, node),
      union([AsyncDisposable, Disposable, null, undefined]),
      initializer, The_initializer_of_an_await_using_declaration…);
      == Using → union([Disposable, null, undefined]) +
      The_initializer_of_a_using_declaration….
    - symbol.declarations.length > 1 && some other var-like decl
      with !areDeclarationFlagsIdentical(d, node) → error
      All_declarations_of_0_must_have_identical_modifiers AT name.
13. node !== valueDeclaration (the merged-declaration face):
    - declarationType = convertAutoToAny(getWidenedTypeForVariable…);
      both non-error && !isTypeIdenticalTo(type, declarationType) &&
      !(symbol.flags & Assignment) →
      errorNextVariableOrPropertyDeclarationMustHaveSameType (L83575):
      PropertyDeclaration/Signature → Subsequent_property_declarations
      _must_have_the_same_type… (2717) else Subsequent_variable_
      declarations… (2403); args (declName, typeToString(firstType),
      typeToString(nextType)) — display band: unrenderable types →
      Unsupported (containment, §14.3); error AT nextDeclarationName
      + related _0_was_also_declared_here at firstDeclaration.
    - initializer → checkTypeAssignableToAndOptionallyElaborate(
      checkExpressionCached(init), declarationType, node, init).
    - valueDeclaration flag mismatch → identical-modifiers error
      again (AT name).
14. Tail, NON-property kinds only: checkExportsOnMergedDeclarations
    (§5); VariableDeclaration|BindingElement →
    checkVarDeclaredNamesNotShadowed (L83371: non-block-scoped,
    non-parameter, FunctionScopedVariable symbol; resolveName(name,
    Variable, no-error, isUse=false) finds a DIFFERENT BlockScoped
    symbol whose declaration is block-scoped; container-scope test
    (varDeclList ancestor; shares scope when function-Block/
    ModuleBlock/ModuleDeclaration/SourceFile container) → error
    Cannot_initialize_outer_scoped_variable_0_in_the_same_scope_as_
    block_scoped_declaration_1 AT node, symbolToString twice);
    checkCollisionsForDeclarationName(node, name).
- areDeclarationFlagsIdentical (L83590): Parameter↔VariableDeclaration
  → true; hasQuestionToken mismatch → false; selected effective
  modifiers (Private|Protected|Async|Abstract|Readonly|Static) equal.

### Collisions band (L83229-83402) — mostly option/target-gated
checkCollisionsForDeclarationName (L83356) = require/exports check +
Promise check + WeakMapSet record + Reflect record + classLike →
checkTypeNameIsReserved(Class_name_cannot_be_0) (+ non-ambient →
checkClassNameCollisionWithObject L84787) + enum →
checkTypeNameIsReserved(Enum_name_cannot_be_0). Callers beyond
variables: function/class/enum/module/import declarations (each cited
in its §).

- needCollisionCheckForIdentifier (L83239): name match; NOT property/
  method/accessor/property-assignment kinds; NOT Ambient; NOT
  type-only import; NOT parameter of body-less function.
- require/exports (L83288): emit module format ≥ ES2015 → skip; else
  top-level in external/CJS module → errorSkippedOn("noEmit",
  Duplicate_identifier_0_Compiler_reserves_name_1_in_top_level_scope_
  of_a_module) — LIVE for @module:commonjs fixtures (module option
  modeled since 5.7b). Non-instantiated module declarations skip.
- Promise (L83303): languageVersion ≥ ES2017 → skip; else module file
  with HasAsyncFunctions flag → …_in_top_level_scope_of_a_module_
  containing_async_functions. languageVersion is the mapped @target —
  LIVE for low-target fixtures (ES5 is mapped; 807 conformance
  fixtures).
- checkCollisionWithArgumentsInGeneratedCode (L83229): ES2015+ →
  skip; **LIVE for the 807 ES5-target conformance fixtures**
  (rest-param +
  `arguments`-named parameter under target<ES2015, non-ambient,
  bodied).
- WeakMapSet (L83315): target ≤ ES2021 && name WeakMap/WeakSet →
  record; drain (L83320): enclosing block scope has
  ContainsClassWithPrivateIdentifiers check flag →
  errorSkippedOn(Compiler_reserves_name_0_when_emitting_private_
  identifier_downlevel). The NodeCheckFlags pusher lands with the
  class band (private identifiers, §6).
- Reflect (L83327): ES2015 ≤ target ≤ ES2021 && "Reflect" → record;
  drain (L83332): ContainsSuperPropertyInStaticInitializer on
  class-expression members / function-expression / enclosing block
  scope → errorSkippedOn(Duplicate_identifier_0_Compiler_reserves_
  name_1_when_emitting_super_references…, name, "Reflect"). Flag
  pusher: super-property checks in static initializers (§6).
- this/newTarget capture checks (L83260/83274): drains exist per §0
  but the CaptureThis/CaptureNewTarget pushers are downlevel-emit
  paths (checkThisExpression at target<ES2015 etc.) — if the target
  floor kills the pushers, the drains stay empty; port drains + note.

## §3 Control statements

### Truthiness kit
- checkTruthinessExpression (L83796) = checkTruthinessOfType(
  checkExpression(node, checkMode), node).
- checkTruthinessOfType (L83748): Void flag → An_expression_of_type_
  void_cannot_be_tested_for_truthiness (1345) AT node; else
  getSyntacticTruthySemantics(node) ≠ Sometimes → This_kind_of_
  expression_is_always_truthy / …always_falsy.
- getSyntacticTruthySemantics (L83762): skipOuterExpressions;
  NumericLiteral text "0"/"1" → Sometimes, other numerics → Always;
  Array/Arrow/BigInt/ClassExpr/FnExpr/JsxElement/JsxSelfClosing/
  ObjectLiteral/Regex → Always; Void/Null → Never; String/NoSubst
  template → text nonempty ? Always : Never; ConditionalExpression →
  bitwise OR of branches (Always|Never = Sometimes); Identifier →
  getResolvedSymbol === undefinedSymbol → Never; default Sometimes.

### checkIfStatement (L83626)
ambient grammar; type = checkTruthinessExpression(expression);
checkTestingKnownTruthyCallableOrAwaitableOrEnumMemberType(expression,
type, thenStatement); checkSourceElement(then); then ==
EmptyStatement → error The_body_of_an_if_statement_cannot_be_the_
empty_statement AT thenStatement; checkSourceElement(else).

### checkTestingKnownTruthyCallableOrAwaitableOrEnumMemberType (L83636)
strictNullChecks-gated. bothHelper: skipParentheses(cond); helper;
descend LEFT operands of ||/?? chains re-running helper. helper:
location = logical-or-coalescing binary ? skipParentheses(right) :
self; isModuleExportsAccessExpression → return (JS); nested logical →
recurse bothHelper; type = (location === condExpr) ? condType :
checkExpression(location).
- **Enum-member face**: type has EnumLiteral flag && location is
  PropertyAccess && raw links.resolvedSymbol of location.expression
  (?? unknownSymbol) has Enum flag → error This_condition_will_
  always_return_0 with "true"/"false" by !!type.value; return.
- isPropertyExpressionCast (property access over type assertion) or
  !hasTypeFacts(type, Truthy) → return (facts.rs kit).
- callSignatures = getSignaturesOfType(type, Call); isPromise =
  !!getAwaitedTypeOfPromise(type) (5.5f kit); neither → return.
- testedNode = Identifier | PropertyAccess.name; testedSymbol =
  symbol-at-location (port as a NARROW helper: resolved symbol of an
  identifier / the access node's links.resolvedSymbol for names —
  do NOT build a general getSymbolAtLocation);
  !testedSymbol && !isPromise → return.
- isUsed = (condExpr.parent is Binary &&
  isSymbolUsedInBinaryExpressionChain(parent, sym) — L83720: climb
  parent chain while `&&`-binaries, forEachChild walk of each RIGHT
  side for an identifier resolving to the symbol) || (body &&
  isSymbolUsedInConditionBody(condExpr, body, testedNode, sym) —
  L83689: forEachChild walk (explicit stack — deep-tree rule)
  comparing resolved symbols; on a hit, when both tested and child
  are property chains, compare name symbols pairwise ASCENDING
  (expression↔expression hops; this↔this via symbols; call↔call hop
  through .expression) — transcribe exactly).
- !isUsed: isPromise → errorAndMaybeSuggestAwait(location, true,
  This_condition_will_always_return_true_since_this_0_is_always_
  defined, getTypeNameForErrorDisplay(type)) — display band:
  unrenderable → Unsupported-containment of THIS report only; else →
  error This_condition_will_always_return_true_since_this_function_
  is_always_defined_Did_you_mean_to_call_it_instead.
- errorAndMaybeSuggestAwait (L47615): error + related
  Did_you_forget_to_use_await AT the same location.
- SECOND CALLER: tsc runs this from checkConditionalExpression too —
  verify expr.rs's conditional arm and wire the call (whenTrue as
  body) with this band; grep expr.rs for the elision note.

### Loops
- checkDoStatement (L83738): ambient grammar; checkSourceElement(
  statement) THEN checkTruthinessExpression(expression) — body
  before condition.
- checkWhileStatement (L83743): condition then body.
- checkForStatement (L83799): !ambient-grammar → initializer-is-list
  → checkGrammarVariableDeclarationList; initializer: list →
  checkVariableDeclarationList else checkExpression; condition →
  checkTruthinessExpression; incrementor → checkExpression; body;
  locals → registerForUnusedIdentifiersCheck (M7-inert).
- checkForOfStatement (L83819): checkGrammarForInOrForOfStatement
  (below); awaitModifier: containing CLASS STATIC BLOCK →
  grammarErrorOnNode(awaitModifier, for_await_loops_cannot_be_used_
  inside_a_class_static_block); else emit-helper probes (no-op);
  initializer list → checkVariableDeclarationList; else varExpr:
  iteratedType = checkRightHandSideOfForOf(node) FIRST, then
  Array/ObjectLiteral varExpr → checkDestructuringAssignment(varExpr,
  iteratedType || errorType) (operators.rs 5.5e kit; its [ITER] arms
  un-escape with §4) else leftType = checkExpression(varExpr);
  checkReferenceExpression(varExpr, The_left_hand_side_of_a_for_of_
  statement_must_be_a_variable_or_a_property_access, …may_not_be_an_
  optional_property_access) (5.5e kit); iteratedType →
  checkTypeAssignableToAndOptionallyElaborate(iteratedType, leftType,
  varExpr, node.expression) — note errorNode=varExpr, expr=RHS.
  Body; locals register.
- checkForInStatement (L83858): grammar; rightType =
  getNonNullableTypeIfNeeded(checkExpression(expression)) (L74996:
  isNullableType → getNonNullableType); initializer list: first
  declaration with pattern name → error The_left_hand_side_of_a_for_
  in_statement_cannot_be_a_destructuring_pattern AT variable.name;
  checkVariableDeclarationList; else varExpr: leftType =
  checkExpression FIRST; Array/ObjectLiteral → same destructuring-
  pattern error AT varExpr; else !isTypeAssignableTo(
  getIndexTypeOrString(rightType), leftType) → error The_left_hand_
  side_of_a_for_in_statement_must_be_of_type_string_or_any (L62024
  getIndexTypeOrString: getExtractStringType(getIndexType(t)),
  never → stringType — indexed.rs kit + extract-string worker);
  else checkReferenceExpression(…for_in… variants). THEN rightType
  === neverType || !isTypeAssignableToKind(rightType, NonPrimitive|
  InstantiableNonPrimitive) → error The_right_hand_side_of_a_for_in_
  statement_must_be_of_type_any_an_object_type_or_a_type_parameter_
  but_here_has_type_0 (typeToString display) AT expression. Body;
  locals.
- checkGrammarForInOrForOfStatement (L89761): ambient-context head;
  for-of awaitModifier WITHOUT AwaitContext flag: top-level context →
  (non-module file → for_await_loops_are_only_allowed_at_the_top_
  level_of_a_file_when_that_file_is_a_module…) + moduleKind switch:
  Node16/18/20/Next + impliedNodeFormat CJS → The_current_file_is_a_
  CommonJS_module_and_cannot_use_await_at_the_top_level; ES2022/
  ESNext/Preserve/System + target ≥ ES2017 → ok; default →
  Top_level_for_await_loops_are_only_allowed_when_the_module_option_
  is_set_to…; non-top-level → for_await_loops_are_only_allowed_
  within_async_functions_and_at_the_top_levels_of_modules + related
  Did_you_mean_to_mark_this_function_as_async at the containing
  non-constructor function (all diagnostics.add — parse-diag gated).
  Then: for-of, no AwaitContext, initializer is identifier "async" →
  grammarErrorOnNode(The_left_hand_side_of_a_for_of_statement_may_
  not_be_async). Then initializer-list checks: list grammar; empty →
  false; >1 declarations → Only_a_single_variable_declaration_is_
  allowed_in_a_for_[in|of]_statement AT declarations[1] first token;
  initializer on first → The_variable_declaration_of_a_for_[in|of]_
  statement_cannot_have_an_initializer AT name; type annotation →
  The_left_hand_side_of_a_for_[in|of]_statement_cannot_use_a_type_
  annotation AT declaration.
- checkRightHandSideOfForOf (L83890): use = awaitModifier ?
  ForAwaitOf(15) : ForOf(13); checkIteratedTypeOrElementType(use,
  checkNonNullExpression(expression), undefinedType, expression).

### checkBreakOrContinueStatement (L84497)
ambient grammar else checkGrammarBreakOrContinueStatement (L89978):
ancestor walk; function-like/static-block boundary → Jump_target_
cannot_cross_function_boundary; matching LabeledStatement: continue
+ label statement not iteration → A_continue_statement_can_only_
jump_to_a_label_of_an_enclosing_iteration_statement; else ok;
Switch + unlabeled break → ok; iteration statement + unlabeled → ok;
exhausted: labeled → A_break_statement_can_only_jump_to_a_label_of_
an_enclosing_statement / continue-flavor; unlabeled → A_break_
statement_can_only_be_used_within_an_enclosing_iteration_or_switch_
statement / A_continue_statement_can_only_be_used_within_an_
enclosing_iteration_statement. All AT node.

### checkReturnStatement (L84516) + checkReturnExpression (L84550)
ambient grammar → return. container = getContainingFunctionOrClass
StaticBlock; static block → grammarErrorOnFirstToken(A_return_
statement_cannot_be_used_inside_a_class_static_block); none →
grammarErrorOnFirstToken(A_return_statement_can_only_be_used_within_
a_function_body); signature = getSignatureFromDeclaration(container);
returnType = getReturnTypeOfSignature(signature).
- (strictNullChecks || node.expression || Never-flagged returnType):
  exprType = expression ? checkExpressionCached : undefinedType;
  - SetAccessor container: expression → error Setters_cannot_return_
    a_value (2408) AT NODE.
  - Constructor container: expression && !checkTypeAssignableToAnd
    OptionallyElaborate(exprType', returnType, node, expression) →
    error Return_type_of_constructor_signature_must_be_assignable_to_
    the_instance_type_of_the_class (2409) AT NODE — the relation call
    reports ITS failure too (both rows in one baseline: transcribe,
    pin `class C { constructor() { return 1 } }` against the oracle).
  - else getReturnTypeFromAnnotation(container) PRESENT →
    unwrappedReturnType = unwrapReturnType(returnType, functionFlags)
    ?? returnType; checkReturnExpression(container, unwrapped, node,
    node.expression, exprType). (Annotation gate: un-annotated
    containers rely on return-type-from-body inference — no
    return-site check.)
  - else noImplicitReturns && non-constructor &&
    !isUnwrappedReturnTypeUndefinedVoidOrAny → Not_all_code_paths_
    return_a_value (option absent from CompilerOptions → verify
    options.rs; skip w/ note if unmodeled).
- unwrapReturnType (L84500): generator →
  getIterationTypeOfGeneratorFunctionReturnType(Return, returnType,
  isAsync) (§4; missing → errorType); async-generator → getAwaited
  TypeNoAlias(unwrapAwaitedType(t)); async → getAwaitedTypeNoAlias(t)
  || errorType; sync → t. (functions.rs unwrapReturnType [ITER]
  escape lifts.)
- checkReturnExpression (L84550): expr → skipParentheses (JSDoc flag
  false in TS); ConditionalExpression → recurse per branch with
  FRESH checkExpression(branch) and inConditionalExpression=true,
  return. unwrappedExprType = async container ? checkAwaitedType(
  exprType, withAlias=false, node, The_return_type_of_an_async_
  function_must_either_be_a_valid_promise_or_must_not_contain_a_
  callable_then_member) : exprType (5.5f kit); effectiveExpr =
  getEffectiveCheckNode(expr); errorNode = (node is ReturnStatement
  && !inConditionalExpression) ? node : effectiveExpr;
  checkTypeAssignableToAndOptionallyElaborate(unwrappedExprType,
  unwrappedReturnType, errorNode, effectiveExpr). SPAN: direct
  returns report AT THE RETURN STATEMENT; conditional branches at
  the branch expression. Other caller: the 5.5f expression-body
  path in functions.rs (verify its existing shape routes here when
  this lands — the (None,None)-return-arm regression class).

### checkWithStatement (L84589)
ambient grammar else AwaitContext flag → grammarErrorOnFirstToken(
with_statements_are_not_allowed_in_an_async_function_block);
checkExpression(expression); then UNCONDITIONALLY (parse-diag gate):
grammarErrorAtPos(sourceFile, start-of-token-at(node.pos), end =
statement.pos - start, The_with_statement_is_not_supported_All_
symbols_in_a_with_block_will_have_type_any) — span from `with` token
start to the statement start. node.statement is NOT checked (with
bodies are unchecked — do not recurse).

### checkSwitchStatement (L84603)
ambient grammar; expressionType = checkExpression(expression); per
clause IN ORDER: duplicate DefaultClause (2nd+) → grammarErrorOnNode(
A_default_clause_cannot_appear_more_than_once_in_a_switch_statement)
once (flag latch); CaseClause → (eager addLazyDiagnostic identity)
caseType = checkExpression(clause.expression);
!isTypeEqualityComparableTo(expressionType, caseType) (L79807:
target Nullable-flagged → true, else isTypeComparableTo(source,
target)) → checkTypeComparableTo(caseType, expressionType,
clause.expression, no head) → Type_0_is_not_comparable_to_type_1
(2678) AT the case expression — note ARG ORDER (case type is the
SOURCE). Then forEach(clause.statements, checkSourceElement).
noFallthroughCasesInSwitch arm: M5+option dead (note). caseBlock
locals → register (M7).

### checkLabeledStatement (L84643)
ambient grammar else ancestor duplicate-label walk (stop at
function-like) → grammarErrorOnNode(node.label, Duplicate_label_0,
getTextOfNode — SOURCE TEXT not escaped text); Unreachable label
flag + allowUnusedLabels → suggestion-band (skip, note);
checkSourceElement(statement).

### checkThrowStatement (L84661)
ambient grammar else Identifier expression with EMPTY escapedText →
grammarErrorAfterFirstToken(Line_break_not_permitted_here);
expression → checkExpression.

### checkTryStatement (L84671)
ambient grammar; checkBlock(tryBlock); catch clause:
variableDeclaration →
- checkVariableLikeDeclaration(declaration) (§2 — the catch-variable
  type comes from the annotate.rs side: unannotated catch var under
  strict useUnknownInCatchVariables → unknown; verify the 5.6
  declared-type arm),
- annotation present: getTypeFromTypeNode; type NOT Any|Unknown →
  grammarErrorOnFirstToken(typeNode, Catch_clause_variable_type_
  annotation_must_be_any_or_unknown_if_specified);
- else initializer → grammarErrorOnFirstToken(initializer,
  Catch_clause_variable_cannot_have_an_initializer);
- else NO annotation/initializer → catch-clause locals vs block
  locals: forEachKey(catchClause.locals): blockLocals.get(name) with
  valueDeclaration + BlockScopedVariable flag → grammarErrorOnNode(
  blockLocal.valueDeclaration, Cannot_redeclare_identifier_0_in_
  catch_clause) — iteration order = symbol-table insertion order.
checkBlock(catch block); finally → checkBlock.

## §4 Iteration protocol ([ITER] — the m4-55 stub tag retires)

### Shape
IterationTypes = {yieldType, returnType, nextType}. Singletons:
noIterationTypes (poison — field reads Debug.fail; port as a
dedicated enum variant, NOT a triple), anyIterationTypes =
(any,any,any). createIterationTypes (L84020): intrinsic-ish triples
(yield Intrinsic && return/next in Any|Never|Unknown|Void|Undefined)
intern in iterationTypesCache keyed by getTypeListId — port as a
state HashMap<[TypeId;3], IterationTypesId> (identity only matters
for the cache; the noIterationTypes sentinel must remain
distinguishable). combineIterationTypes (L84032): skip undefined/no;
any → anyIterationTypes; unions of yields/returns, INTERSECTION of
nexts.

IterationUse flags: AllowsSyncIterables=1, AllowsAsyncIterables=2,
AllowsStringInput=4, ForOfFlag=8, YieldStarFlag=16, SpreadFlag=32,
DestructuringFlag=64, PossiblyOutOfBounds=128. Values: Element=1,
Spread=33, Destructuring=65, ForOf=13, ForAwaitOf=15, YieldStar=17,
AsyncYieldStar=19, GeneratorReturnType=1, AsyncGeneratorReturnType=2.
IterationTypeKind: Yield=0/Return=1/Next=2 →
getIterationTypesKeyFromIterationTypeKind (L90932) field selector.

Resolvers (L47301/47316): sync = {cache keys iterationTypesOf
Iterable/Iterator, symbol "iterator", global Iterator/Iterable/
IterableIterator/IteratorObject/Generator types,
getGlobalBuiltinIteratorTypes = [ArrayIterator, MapIterator,
SetIterator, StringIterator] (L60847, memoized), resolveIterationType
= identity, diagnostics An_iterator_must_have_a_next_method /
The_0_property_of_an_iterator_must_be_a_method /
The_type_returned_by_the_0_method_of_an_iterator_must_have_a_value_
property}; async = {…Async flavors, symbol "asyncIterator",
resolveIterationType = getAwaitedType(type, errorNode,
Type_of_await_operand_must_either_be…), async diagnostic flavors}.
New global-type probes to add (globals.rs): AsyncIterator,
AsyncIterable, AsyncIterableIterator, AsyncIteratorObject,
AsyncGenerator, IteratorObject, Generator, IteratorYieldResult,
IteratorReturnResult, the 4+4 builtin iterator types, Awaited symbol
probe (exists 5.5f), AsyncDisposable/Disposable (§2). All
deferred-memoized like existing globals; missing under noLib → the
2318 band only when reportErrors=true.

Caches: per-TYPE fields (TypeLinks additions):
iteration_types_of_{iterable,async_iterable,iterator,async_iterator,
iterator_result} — 5 slots storing IterationTypes-or-noIterationTypes
verdicts. getBuiltinIteratorReturnType (L60844):
strictBuiltinIteratorReturn ? undefinedType : anyType — **VERIFIED
strict-family in TS6 (bundle L46472: getStrictOptionValue) ⇒
default-ON under strict-by-default. NOT in CompilerOptions yet:
5.8b ADDS strict_builtin_iterator_return (read through
strict_option_value)**; annotate.rs's BuiltinIteratorReturn
intrinsic escape lifts with this.

### Entry points
- checkIteratedTypeOrElementType (L83894): any input → input;
  getIteratedTypeOrElementType(use, input, sent, errorNode,
  checkAssignability=true) || anyType.
- getIteratedTypeOrElementType (L83907) — transcribe whole:
  - never + errorNode → reportTypeNotIterableError → undefined.
  - uplevelIteration = target ≥ ES2015 && globalIterableType
    resolves (≠ emptyGenericType); downlevelIteration = !uplevel &&
    options.downlevelIteration — **NOT in CompilerOptions yet: 5.8b
    ADDS the field + conformance mapping (21 conformance fixtures set
    @downlevelIteration; without it those select the WRONG
    diagnostic flavor under low targets = FP)**; possibleOutOfBounds
    = noUncheckedIndexedAccess && (use & 128) (option PRESENT in
    options.rs — read directly).
  - (uplevel || downlevel || allowsAsync) → iterationTypes =
    getIterationTypesOfIterable(input, use, uplevel ? errorNode :
    undefined); checkAssignability && iterationTypes: per-use
    sent-type diagnostic (ForOfFlag → Cannot_iterate_value_because_
    the_next_method_of_its_iterator_expects_type_1_but_for_of_will_
    always_send_0; Spread/Destructuring/YieldStar flavors) →
    checkTypeAssignableTo(sentType, iterationTypes.nextType,
    errorNode, diag); (iterationTypes || uplevel) → return
    yieldType? (+ includeUndefinedInIndexSignature under
    possibleOutOfBounds; L69829: union w/ missingType).
  - String-input band (downlevel reach): StringLike constituents
    filtered from unions (Subtype-reduced union) / whole → never;
    all-string → return stringType.
  - !isArrayLikeType → errorNode: getIterationDiagnosticDetails
    (inner fn): downlevel → Type_0_is_not_an_array_type[_or_a_
    string_type]_or_does_not_have_a_Symbol_iterator_method… (+await
    hint true); else iterable-when-downlevel probe (getIterationType
    OfIterable Yield, no error) → Type_0_can_only_be_iterated_
    through_when_using_the_downlevelIteration_flag_or_with_a_target_
    of_es2015_or_higher (hint false); isES2015OrLaterIterable(
    symbol name ∈ {Float32Array…Uint8ClampedArray, NodeList} L83997)
    → same message, hint true; else Type_0_is_not_an_array_type[_or_
    a_string_type] (hint true). errorAndMaybeSuggestAwait(errorNode,
    hint && getAwaitedTypeOfPromise(arrayType), diag, typeToString).
    Return string-constituent fallback.
  - Array path: getIndexTypeOfType(arrayType, numberType); string-
    constituent merge (StringLike element && !noUnchecked →
    stringType; else Subtype union [+undefined]); return element
    (+undefined under 128-flag).
- checkRightHandSideOfForOf → §3. Spread/destructuring/yield*
  call-sites already exist behind [ITER] escapes: calls.rs
  getSpreadArgumentType, operators.rs destructuring arms, literals.rs
  array-literal spread arms, functions.rs yield/return arms,
  contextual.rs generator arms, widen.rs generator rows, expr.rs
  checkSpreadExpression — grep `[ITER]` and lift each against its
  cited anchor as part of this band.

### getIterationTypesOfIterable (L84062) — the cache/error dance
getReducedType FIRST. any → anyIterationTypes. NON-union:
errorOutputContainer = errorNode ? {errors: undefined, skipLogging:
TRUE} : undefined; worker; noIterationTypes → errorNode:
reportTypeNotIterableError as ROOT diagnostic + container errors as
RELATED INFO (addRelatedInfo) → undefined; else container errors →
diagnostics.add each (collected during a SUCCESSFUL resolution —
mustHaveAValue face); return. UNION: per-type cache key by async
flag; cached verdict short-circuits (no → undefined); per
constituent: container WITHOUT skipLogging ({errors: undefined});
worker; any noIterationTypes → report root + related, CACHE no →
undefined; success-errors → diagnostics.add; combine; cache; return.
**Container-semantics audit (risk §14.6)**: with skipLogging UNSET
the relation machinery both PUSHES and ADDS — re-read our 5.7a
container port and keep exactly one add per diagnostic; the union
path's success-with-errors add-loop plus an eager relation add would
double-report. Decide collect-only + explicit add at the call sites
and pin a union-iterable failure fixture.

### getIterationTypesOfIterableWorker (L84129)
async flag: cached||fast(async); found: no+errorNode → noCache=true
(re-run for errors, DON'T cache) else return (ForOfFlag →
getAsyncFromSyncIterationTypes! for-await over sync-only remains);
slow(async, noCache); ≠no → return. sync flag: cached||fast(sync);
found: no+errorNode → noCache else asyncAllowed ? (≠no →
asyncFromSync + cache under ASYNC key unless noCache) : return;
slow(sync, noCache); ≠no → asyncAllowed ? asyncFromSync (+cache
async key) : return. → noIterationTypes.
- getAsyncFromSyncIterationTypes (L84113): awaited yield/return
  (getAwaitedType || anyType; errorNode → getGlobalAwaitedSymbol(
  true) probe), next passes through.
- getIterationTypesOfIterableFast (L84179): isReferenceToType
  against resolver's global Iterable/IteratorObject/
  IterableIterator/Generator → typeArguments [y,r,n] triple with
  resolveIterationType(y)||y etc; cache. isReferenceToSomeType
  against builtin iterator types → [y] + getBuiltinIteratorReturnType
  + unknownType; cache. (isReferenceToType L56990: target-of-
  reference equality — structural.rs kit.)
- getIterationTypesOfIterableSlow (L84227): method =
  getPropertyOfType(type, getPropertyNameForKnownSymbolName(
  resolver.iteratorSymbolName)) — **the §1 wall**: promote the
  operators.rs port of getPropertyNameForKnownSymbolName (L84219:
  global Symbol ctor property → unique-symbol name via
  getPropertyNameFromType, else `__@name` fallback) to a shared
  site; methodType = non-Optional method → getTypeOfSymbol; any →
  anyIterationTypes (cache unless noCache); validSignatures =
  call sigs with minArgCount 0; none valid: errorNode &&
  some(allSignatures) → checkTypeAssignableTo(type,
  globalIterableType(reportErrors=true), errorNode, no-head,
  no-chain, container) — elaboration rows into the container; cache
  no; else iteratorType = INTERSECTION of return types of valid
  sigs → getIterationTypesOfIteratorWorker ?? no; cache.
- reportTypeNotIterableError (L84257): async-allowed →
  Type_0_must_have_a_Symbol_asyncIterator_method_that_returns_an_
  async_iterator else Type_0_must_have_a_Symbol_iterator_method_
  that_returns_an_iterator (2488); suggestAwait =
  getAwaitedTypeOfPromise(type) || (!async && for-of parent whose
  expression is errorNode && globalAsyncIterableType exists && type
  assignable to AsyncIterable<any,any,any> via
  createTypeFromGenericGlobalType); errorAndMaybeSuggestAwait.

### Iterator side (L84271-84496)
- getIterationTypesOfIteratorWorker: any → anyIterationTypes;
  cached||fast; no+errorNode → clear + noCache; ??= slow; no →
  undefined.
- fast (L84296): refs to IterableIterator/Iterator/IteratorObject/
  Generator → [y,r,n] RAW (no resolveIterationType on the iterator
  side); builtin iterator refs → BuiltinIteratorReturn arm; cache.
- getIterationTypesOfIteratorSlow (L84462): combine([method "next",
  "return", "throw"]); cache unless noCache.
- getIterationTypesOfMethod (L84378) — transcribe: prop lookup;
  absent && name ≠ "next" → undefined; methodType: next-Optional →
  undefined; non-next → getTypeWithFacts(typeof, NEUndefinedOrNull)
  (facts.rs); any → anyIterationTypes; no call sigs → errorNode:
  (next ? resolver.mustHaveANextMethodDiagnostic :
  mustBeAMethodDiagnostic with methodName arg) container-push-or-
  error; return next ? noIterationTypes : undefined. Single-sig
  whose methodType.symbol === global Generator/Iterator member
  symbol of that name → SHORTCUT: getMappedType(globalType.
  typeParameters[i], methodType.mapper) triple (y, r, next-only-for-
  "next") — needs instantiated-type mapper access (instantiate.rs
  identity; the member symbol compare is table identity on the
  GLOBAL's members — lib-loaded only). Else: param types = union of
  first-position types of sigs (skip for "throw"); "next" → nextType
  = that; "return" → returnTypes += resolveIterationType(paramUnion,
  errorNode)||any; methodReturnType = INTERSECTION of sig returns;
  resolved = resolveIterationType(…)||any;
  getIterationTypesOfIteratorResult(resolved); ===no → errorNode →
  container-push-or-error(resolver.mustHaveAValueDiagnostic,
  methodName); yield=any, returns+=any; else yield=its yield,
  returns+=its return. → createIterationTypes(yield,
  union(returns), next).
- getIterationTypesOfIteratorResult (L84330): any → any; cache
  "iterationTypesOfIteratorResult"; ref to IteratorYieldResult →
  (arg0, -, -); ref to IteratorReturnResult → (-, arg0, -); else
  filterType by isYield/isReturnIteratorResult (L84320: doneType =
  prop "done" || falseType; assignable(Yield ? falseType :
  trueType, doneType)); yieldType = value-prop of yield-filtered
  (≠never); returnType likewise; neither → no; →
  (yield, return || voidType, -).
- Generator return types (L84470/84477): any → undefined/any;
  use = Async ? 2 : 1; getIterationTypesOfIterable(type, use,
  no-error) || getIterationTypesOfIterator(type, resolver,
  no-error, no-container). Consumers un-escape: contextual.rs
  generator arms, widen.rs shouldReportErrorsFromWidening generator
  rows, functions.rs unwrapReturnType/yield aggregation/generator
  body inference (checkYieldExpression's full port rides the
  function band §5, but its iteration calls come from HERE).

Cross-checks for this band: checkSpreadExpression (expr.rs stub) =
checkIteratedTypeOrElementType(Spread=33, …); array literal spreads
(literals.rs); binding-pattern element types (functions.rs
array-binding [ITER] arm = Destructuring use with
possibleOutOfBounds bit per tsc call sites — re-read each call site
when lifting, several pass `65 | 128`); getSpreadArgumentType's
checkIteratedTypeOrElementType(Spread) in calls.rs;
getTypeOfDestructuredSpreadExpression (L69836: createArrayType of
checkIteratedTypeOrElementType(Destructuring…)).

FN expectation: array/string iteration recovers only WITH the §1
lift; class/generator/explicit-interface iterables recover at this
band. 2488/2504 must never fire on a type whose member table
CONTAINED (§1) — the slow path's Unsupported unwind guarantees that
(containment, not misreport); assert via TSRS_TRACE_CONTAIN probe
before pinning.

## §5 Member + function declarations

### checkParameter (L81170)
grammar-modifiers hook; checkVariableLikeDeclaration (§2 — parameters
share the whole band incl. collisions tail); parameter-property
modifiers: erasableSyntaxOnly option row (This_syntax_is_not_allowed_
when_erasableSyntaxOnly_is_enabled — option unmodeled → dead, note);
containing fn NOT (Constructor with present body) → A_parameter_
property_is_only_allowed_in_a_constructor_implementation; ctor param
property named "constructor" → constructor_cannot_be_used_as_a_
parameter_property_name. No-initializer + optional + binding-pattern
name + fn body present → A_binding_pattern_parameter_cannot_be_
optional_in_an_implementation_signature. this/new-named params:
index ≠ 0 → A_0_parameter_must_be_the_first_parameter; ctor/
ctor-sig/ctor-type container → A_constructor_cannot_have_a_this_
parameter; arrow → An_arrow_function_cannot_have_a_this_parameter;
accessors → get_and_set_accessors_cannot_declare_this_parameters.
Rest param, non-pattern name, !isTypeAssignableTo(getReducedType(
getTypeOfSymbol(node.symbol)), anyReadonlyArrayType) → A_rest_
parameter_must_be_of_an_array_type (anyReadonlyArrayType singleton —
verify globals.rs; noLib fallback = anyArrayType shape).

### checkTypePredicate (L81206)
getTypePredicateParent (L81254: fn-like parents where node ===
parent.type); miss → A_type_predicate_is_only_allowed_in_return_
type_position_for_functions_and_methods. THEN
getTypePredicateOfSignature — signature predicates are UNMODELED at
M4 (m4-57 note; they port with M5 getEffectsSignature) → the entire
tail (checkSourceElement(node.type), rest-param row, predicate-type
assignability with the A_type_predicate_s_type_must_be_assignable…
chain, Cannot_find_parameter_0, binding-pattern element walk
L81269) is PROVABLY DEAD until the predicate model lands. Port the
1228-row + an M5-stub named escape for the tail; record the FN class
(2677/1230/1225-family rows).

### checkSignatureDeclaration (L81289)
IndexSignature → checkGrammarIndexSignature; FunctionType/
FunctionDeclaration/ConstructorType/CallSignature/Constructor/
ConstructSignature → checkGrammarFunctionLikeDeclaration. Emit-helper
probes (no-op). checkTypeParameters(getEffectiveTypeParameter
Declarations) (5.4 kit — replaces the functions.rs 5.8-DECL no-op
hooks at their two sites). forEach(parameters, checkParameter) —
DIRECT call, not checkSourceElement (no containment reset; keep
per-parameter Err containment like checkTypeParameters does).
node.type → checkSourceElement. Lazy tail (eager-inline):
checkCollisionWithArgumentsInGeneratedCode (§2 note);
noImplicitAny && no return annotation → ConstructSignature →
Construct_signature_which_lacks_return_type_annotation_implicitly_
has_an_any_return_type (7013) / CallSignature → …Call_signature…
(7020) AT node; return annotation present: pure generator →
returnType === voidType → A_generator_cannot_have_a_void_type_
annotation else checkGeneratorInstantiationAssignabilityToReturnType
(L81356: yield/return/next = getIterationTypeOfGeneratorFunction
ReturnType(k, returnType, isAsync) || (any / yieldType / unknown);
createGeneratorType (L78842: Generator/AsyncGenerator global ref —
read when porting) → checkTypeAssignableTo(generatorInstantiation,
returnType, errorLocation) plain 2322 head — contextual.rs's
[ITER 5.8] escape lifts here); async non-generator →
checkAsyncFunctionReturnType (below). JSDoc return-type indirection
is JS-only (returnTypeErrorLocation === returnTypeNode in TS).

### checkAsyncFunctionReturnType (L82498)
target ≥ ES2015 (the modeled default): returnType from the node;
errorType → return; getGlobalPromiseType(reportErrors=TRUE) (2318
under noLib); ≠ emptyGenericType && !isReferenceToType(returnType,
globalPromiseType) → error The_return_type_of_an_async_function_or_
method_must_be_the_global_Promise_T_type_Did_you_mean_to_write_
Promise_0 (1064) AT the return-type node, arg = typeToString(
getAwaitedTypeNoAlias(returnType) || voidType); return. Then tail
for the passing shape: checkAwaitedType(returnType, withAlias=false,
node, The_return_type_of_an_async_function_must_either_be…) (5.5f
kit). Target < ES2015 (gated on the modeled @target — MANDATORY
scope, 807 ES5 conformance fixtures): the ES5 promise-constructor
entity band
(Type_0_is_not_a_valid_async_function_return_type_in_ES5…,
An_async_function_or_method_in_ES5_requires_the_Promise_constructor…,
PromiseConstructorLike relation with headMessage+chain,
Duplicate_identifier_0_Compiler_uses_declaration_1_to_support_async_
functions on node.locals collision) — transcribe from L82512-82562
when the low-target band matters; markLinkedReferences is emit-only
(no-op hook).

### checkFunctionDeclaration (L82784) / checkFunctionOrMethodDeclaration (L82899)
checkFunctionDeclaration = lazy{checkFunctionOrMethodDeclaration;
checkGrammarForGenerator; checkCollisionsForDeclarationName}.
checkFunctionOrMethodDeclaration: checkDecorators;
checkSignatureDeclaration; computed name → checkComputedPropertyName;
hasBindableName: localSymbol = node.localSymbol || symbol (binder:
exported declarations carry a localSymbol — verify the binder field
exists; if unmodeled, the export-symbol face diverges); node ===
first same-kind non-JS declaration of localSymbol →
checkFunctionOrConstructorSymbol(localSymbol); symbol.parent
(member functions) → checkFunctionOrConstructorSymbol(symbol) —
BOTH calls can run. body = MethodSignature ? undefined : node.body;
checkSourceElement(body) — FUNCTION DECLARATION BODIES DRIVE
EAGERLY (no deferral; the 5.5f deferred path covers fn EXPRESSIONS
only). checkAllCodePathsInNonVoidFunctionReturnOrThrow(node,
getReturnTypeFromAnnotation(node)). Lazy: no return annotation →
(body MISSING && !isPrivateWithinAmbient (L82010) →
reportImplicitAny(node, anyType) — the 5.6 kit's 7010/7011 rows now
fire for overloads/ambient declarations); generator with body →
getReturnTypeOfSignature(getSignatureFromDeclaration(node)) —
FORCING demand (yield aggregation, [ITER] functions.rs arm lifts).

### checkAllCodePathsInNonVoidFunctionReturnOrThrow (L79075)
Lazy. type = returnType && unwrapReturnType(returnType, flags);
void-ish (maybeTypeOfKind Void) / Any|Undefined → return;
MethodSignature / missing body / non-Block body /
!functionHasImplicitReturn → return — **needs NodeFlags
HasImplicitReturn + HasExplicitReturn from the BINDER** (verify M2
ports these flow-exit flags; if absent this band blocks on a binder
slice). errorNode = return-type node || func. Never-flagged type →
A_function_returning_never_cannot_have_a_reachable_end_point (2534);
type && !HasExplicitReturn → A_function_whose_declared_type_is_
neither_undefined_void_nor_any_must_return_a_value (2355); type &&
strictNullChecks && !isTypeAssignableTo(undefined, type) →
Function_lacks_ending_return_statement_and_return_type_does_not_
include_undefined (2847); else noImplicitReturns tail (option —
unmodeled → dead note; L79095-79104 when it lands). Callers:
checkFunctionOrMethodDeclaration, checkAccessorDeclaration (getter),
and the 5.5f function-expression path (verify functions.rs already
routes or stubs this — un-stub with this band).

### checkConstructorDeclaration (L81556)
checkSignatureDeclaration; checkGrammarConstructorTypeParameters →
checkGrammarConstructorTypeAnnotation; checkSourceElement(body) —
EAGER; symbol; node === first Constructor declaration →
checkFunctionOrConstructorSymbol(symbol); missing body → return;
lazy: containing class has extends heritage → (captureLexicalThis =
emit no-op hook); classExtendsNull = classDeclarationExtendsNull
(L72324: base type node expression is a NullKeyword after skip);
superCall = findFirstSuperCall(body) (L72321: forEachChild walk,
stop at fn boundaries): present: extendsNull → A_constructor_cannot_
contain_a_super_call_when_its_class_extends_null; superCallShouldBe
RootLevel = !emitStandardClassFields && (some instance prop with
initializer || private-identifier class element || some parameter
property) — emitStandardClassFields = target ≥ ES2022 &&
useDefineForClassFields default TRUE ⇒ band dead at default, LIVE
for low-@target fixtures: not root-level (walkUpParenthesized parent
is expression-statement of body) → A_super_call_must_be_a_root_
level_statement_within_a_constructor…; root-level → statement scan:
first expression-statement whose skipOuterExpressions is a super
call, breaking early when nodeImmediatelyReferencesSuperOrThis
(L81616: super/this token; stop at this-container/function-block
boundaries) → none found → A_super_call_must_be_the_first_statement_
in_the_constructor…; absent superCall && !extendsNull →
Constructors_for_derived_classes_must_contain_a_super_call (2377)
AT node.

### checkAccessorDeclaration (L81625)
"constructor"-named accessor in class → Class_constructor_may_not_
be_an_accessor. Lazy block runs BEFORE the body (eager identity —
order): grammar chain (checkGrammarFunctionLikeDeclaration →
checkGrammarAccessor L89843 → checkGrammarComputedPropertyName);
checkDecorators; checkSignatureDeclaration; GetAccessor + non-ambient
+ body present + HasImplicitReturn && !HasExplicitReturn → A_get_
accessor_must_return_a_value (2378) AT name (binder flags again);
computed name → checkComputedPropertyName; hasBindableName →
getter/setter pair ONCE-latch (getter links TypeChecked flag —
route through NodeCheckFlags): abstract-flag mismatch → Accessors_
must_both_be_abstract_or_non_abstract at BOTH names (two rows);
getter visibility < setter (protected getter w/ public setter |
private getter w/ non-private setter) → A_get_accessor_must_be_at_
least_as_accessible_as_the_setter at BOTH names; returnType =
getTypeOfAccessors(symbol) (annotate.rs 5.3 kit); GetAccessor →
checkAllCodePathsInNonVoidFunctionReturnOrThrow. THEN
checkSourceElement(body); setNodeLinksForPrivateIdentifierScope.
The deferred object-literal-accessor arm (check.rs) routes to THIS
worker (the named escape lifts).

### checkClassStaticBlockDeclaration (L81552)
grammar-modifiers hook; forEachChild(node, checkSourceElement) —
drives the block body (return/await grammar rows come from the
statement band's static-block gates).

### checkMethodDeclaration (L81522)
checkGrammarMethod → checkGrammarComputedPropertyName; generator
named "constructor" → Class_constructor_may_not_be_a_generator;
checkFunctionOrMethodDeclaration (bodies of object-literal/class
methods still ALSO defer through 5.5f's checkNodeDeferred path —
tsc double-drives via links TypeChecked/instantiation caching; our
existing deferred worker + this eager path must agree: the deferred
arm re-checks the SAME body via checkSourceElement, idempotent
through node links; verify no duplicate diagnostics via the dedupe
+ once-flags, pin an object-literal method fixture); abstract +
body → Method_0_cannot_have_an_implementation_because_it_is_marked_
abstract; private-identifier name outside class → Private_
identifiers_are_not_allowed_outside_class_bodies;
setNodeLinksForPrivateIdentifierScope.

### checkPropertyDeclaration (L81508) / checkPropertySignature (L81516)
Property: grammar chain (modifiers-hook → checkGrammarProperty →
checkGrammarComputedPropertyName); checkVariableLikeDeclaration
(§2); setNodeLinksForPrivateIdentifierScope; abstract + initializer
→ Property_0_cannot_have_an_initializer_because_it_is_marked_
abstract. Signature: private-identifier name → Private_identifiers_
are_not_allowed_outside_class_bodies; → checkPropertyDeclaration.

### setNodeLinksForPrivateIdentifierScope (L81536)
private name && (target < PrivateNames/ClassStaticBlocks || target <
ClassAndClassElementDecorators || !useDefineForClassFields) → every
enclosing block-scope container gets NodeCheckFlags
ContainsClassWithPrivateIdentifiers (the §2 WeakMapSet drain
feeder); class-expression-in-iteration emit flags (BlockScopedBinding
InLoop / LoopWithCapturedBlockScopedBinding — emit-only, port as
notes unless the check flags are read elsewhere).

### checkFunctionOrConstructorSymbol (L82024) — overload band
Lazy worker; transcribe whole (L82027-82214):
- flagsToCheck = Export|Ambient|Private|Protected|Abstract; fold
  some/all flags + some/all questionToken over same-kind
  declarations (FunctionDeclaration/MethodDeclaration/MethodSignature/
  Constructor only); ambient-or-interface parents RESET
  previousDeclaration (consecutiveness is per-container);
  class declarations among a Function symbol's declarations set
  hasNonAmbientClass.
- Body accounting: second body → isConstructor ?
  multipleConstructorImplementation : duplicateFunctionDeclaration;
  gap between consecutive same-container declarations
  (previousDeclaration.end !== node.pos) →
  reportImplementationExpectedError(previousDeclaration).
- reportImplementationExpectedError (L82076): missing name → skip;
  subsequentNode = next sibling via forEachChild scan; adjacent
  (pos === end) same-kind: same-name (private-identifier pair /
  computed-name pair via isTypeIdenticalTo of checkComputedPropertyName
  types / literal-name pair) → static-mismatch methods →
  Function_overload_must_be_static / …must_not_be_static (at
  subsequent's name), else return; subsequent has body →
  Function_implementation_name_must_be_0 (args: node's name). Fall
  through: constructor → Constructor_implementation_is_missing;
  abstract → All_declarations_of_an_abstract_method_must_be_
  consecutive; else Function_implementation_is_missing_or_not_
  immediately_following_the_declaration. All AT name||node.
- multipleConstructorImplementation → Multiple_constructor_
  implementations_are_not_allowed at EVERY function declaration;
  duplicateFunctionDeclaration → Duplicate_function_implementation
  at every declaration's name.
- hasNonAmbientClass && !ctor && Function-flagged symbol →
  per-declaration Class_declaration_cannot_implement_overload_list_
  for_0 (class decls) / Function_with_bodies_can_only_merge_with_
  classes_that_are_ambient (fn decls), each with related Consider_
  adding_a_declare_modifier_to_this_class rows for every class
  declaration.
- lastSeenNonAmbient with no body, non-abstract, no questionToken →
  reportImplementationExpectedError.
- hasOverloads: checkFlagAgreementBetweenOverloads — deviation vs
  CANONICAL overload (implementation if it shares the FIRST
  overload's container, else first overload; per-FILE grouping for
  Export/Ambient deviations): Overload_signatures_must_all_be_
  exported_or_non_exported / …ambient_or_non_ambient /
  …public_private_or_protected / …abstract_or_non_abstract;
  question-token deviation → Overload_signatures_must_all_be_
  optional_or_required. THEN implementation compatibility:
  bodySignature vs every symbol signature via
  isImplementationCompatibleWithOverload (grep anchor at landing —
  assignability both ways on the erased shapes; FIRST failure only)
  → This_overload_signature_is_not_compatible_with_its_
  implementation_signature AT the overload's declaration + related
  The_implementation_signature_is_declared_here; break.

### checkExportsOnMergedDeclarations (L82215)
Lazy. symbol = node.localSymbol || (getSymbolOfDeclaration if it has
an exportSymbol, else return); only the FIRST declaration of
node.kind runs. Declaration spaces (ExportValue=1|ExportType=2|
ExportNamespace=4) per kind (L82259 table: interface/typealias →
Type; module → Namespace|Value when ambient-or-instantiated else
Namespace; class/enum/enum-member → Type|Value; SourceFile → all;
ExportAssignment/Binary → Value unless entity-name → fall into
ALIAS arm; ImportEquals/NamespaceImport/ImportClause → union of
resolveAlias target's declaration spaces (§9 dependency — until
resolveAlias lands, symbols with alias declarations must ESCAPE
here, not guess); var/binding-element/function/import-specifier →
Value; method/property signatures → Type). Fold exported (default
vs non-default) vs non-exported; intersections → per-declaration
rows: default∩nonDefault → Merged_declaration_0_cannot_include_a_
default_export_declaration…; exported∩local → Individual_
declarations_in_merged_declaration_0_must_be_all_exported_or_all_
local. AT each declaration's name.

### Duplicate-member checks (class/object-type) — called from §6/§7 owners
- checkClassForDuplicateDeclarations (L81363): meaning bits
  Get=1/Set=2/Property=3/Method=8/PrivateStatic=16; ctor parameter
  properties → instance map as Property; private-static mismatch →
  Duplicate_identifier_0_Static_and_instance_elements_cannot_share_
  the_same_private_name; method-vs-nonmethod or overlapping
  non-method meanings → Duplicate_identifier_0 (getTextOfNode arg);
  get+set merge bits.
- checkClassForStaticPropertyNameConflicts (L81425): static member
  named prototype always — name/length/caller/arguments only when
  !useDefineForClassFields → Static_property_0_conflicts_with_built_
  in_property_Function_0_of_constructor_function_1 AT name.
- checkObjectTypeForDuplicateDeclarations (L81449):
  PropertySignature members with literal/identifier names; duplicate
  → TWO rows: at the FIRST declaration's name (symbol
  valueDeclaration) and at the current name.
- checkTypeForDuplicateIndexSignatures (L81475): interfaces run on
  the FIRST declaration only; index symbol declarations → per
  parameter-type CONSTITUENT (forEachType) map by TypeId; >1 →
  Duplicate_index_signature_for_type_0 at EVERY declaration.

### checkMissingDeclaration (L81670)
checkDecorators only.


## §6 Class band

### checkClassDeclaration (L84982) / checkClassExpression (L84972)
Declaration: legacy static-private grammar row — **LIVE under
experimental_decorators=true** (§10 dual mode): first decorator +
any static private-identifier class element → grammarErrorOnNode(
firstDecorator, Class_decorators_can_t_be_used_with_static_private_
identifier_Consider_removing_the_experimental_decorator); unnamed
non-default → A_class_declaration_without_the_default_modifier_must_
have_a_name (grammarErrorOnFirstToken); checkClassLikeDeclaration;
forEach members checkSourceElement; M7 register. Expression (the
expr.rs 5.8 stub lifts): checkClassLikeDeclaration;
checkNodeDeferred(node) — the check.rs ClassExpression deferred arm
un-unreachables → checkClassExpressionDeferred (L84978): forEach
members checkSourceElement; emit-helper worker = no-op; returns
getTypeOfSymbol(symbol).

### checkClassLikeDeclaration (L84994) — transcribe; order is the spec
checkGrammarClassLikeDeclaration (grammar band, L~89400 — extract
with the grammar sweep); checkDecorators (§10);
checkCollisionsForDeclarationName (§2 — Class_name_cannot_be_0 +
checkClassNameCollisionWithObject L84787 inside);
checkTypeParameters (5.4 live); checkExportsOnMergedDeclarations
(§5); type = getDeclaredTypeOfSymbol; typeWithThis =
getTypeWithThisArgument(type); staticType = getTypeOfSymbol(symbol);
checkTypeParameterListsIdentical (L84871: >1 class/interface
declarations; symbolLinks.typeParametersChecked once-latch;
areTypeParametersIdentical L84891 — count range vs min/max, name
text ==, constraint/default isTypeIdenticalTo when both present —
mismatch → All_declarations_of_0_must_have_identical_type_parameters
at EVERY declaration name); checkFunctionOrConstructorSymbol (§5);
checkClassForDuplicateDeclarations (§5); non-ambient →
checkClassForStaticPropertyNameConflicts (§5).

Base-type block (getEffectiveBaseTypeNode present):
- eager: forEach typeArguments checkSourceElement; emit-helpers
  no-op; (JS @augments divergence elided).
- baseTypes = getBaseTypes(type) (5.2 kit); nonempty → LAZY:
  - staticBaseType = getApparentType(getBaseConstructorTypeOfClass);
  - checkBaseTypeAccessibility (L85269: first construct signature's
    declaration has Private modifier && node not within that class →
    Cannot_extend_a_class_0_Class_constructor_is_marked_as_private,
    getFullyQualifiedName arg);
  - checkSourceElement(baseTypeNode.expression) — dispatch no-op for
    entity expressions; port literally;
  - typeArguments → checkSourceElement each (idempotent re-force) +
    for constructor in getConstructorsForTypeArguments(staticBase,
    typeArguments, baseTypeNode): checkTypeArgumentConstraints(
    baseTypeNode, constructor.typeParameters) until first failure —
    NOTE: check.rs's checkTypeArgumentConstraints currently asserts
    TypeReference node data; generalize to
    ExpressionWithTypeArguments (heritage) + keep the effective-
    type-arguments kit;
  - baseWithThis = getTypeWithThisArgument(baseType, type.thisType);
    !isTypeAssignableTo(typeWithThis, baseWithThis) [SILENT] →
    issueMemberSpecificError(node, typeWithThis, baseWithThis,
    Class_0_incorrectly_extends_base_class_1); else →
    checkTypeAssignableTo(staticType, getTypeWithoutSignatures(
    staticBaseType), node.name||node, Class_static_side_0_
    incorrectly_extends_base_class_static_side_1)
    (getTypeWithoutSignatures — new helper, grep anchor at landing);
  - mixin band: baseConstructorType TypeVariable-flagged →
    !isMixinConstructorType(staticType) → A_mixin_class_must_have_a_
    constructor_with_a_single_rest_parameter_of_type_any; else
    abstract construct sig && class not abstract → A_mixin_class_
    that_extends_from_a_type_variable_containing_an_abstract_
    construct_signature_must_also_be_declared_abstract;
  - non-class-symbol static base && non-TypeVariable →
    getInstantiatedConstructorsForTypeArguments (5.7 kit); any sig
    return not identical to baseType → Base_constructors_must_all_
    have_the_same_return_type AT baseTypeNode.expression;
  - checkKindsOfPropertyMemberOverrides(type, baseType).

checkMembersForOverrideModifier (L85112): per non-ambient member
(+ constructor parameter properties flagged memberIsParameterProperty)
→ checkMemberForOverrideModifier (L85171):
- override modifier + non-bindable dynamic name → This_member_cannot_
  have_an_override_modifier_because_its_name_is_dynamic;
- base exists && (override || noImplicitOverride option): prop/
  baseProp lookups by escapedName on (static ? staticType/
  baseStaticType : typeWithThis/baseWithThis); prop && !baseProp &&
  override → …because_it_is_not_declared_in_the_base_class_0
  [+ _Did_you_mean_1 flavor via
  getSuggestedSymbolForNonexistentClassMember — spell.rs]; prop &&
  baseProp && noImplicitOverride && !ambient: override → Ok;
  base not abstract → This_member_must_have_an_override_modifier_…
  (parameter-property flavor swaps the message); member abstract &&
  base abstract → …overrides_an_abstract_method…;
- no base && override → …containing_class_0_does_not_extend_another_
  class. noImplicitOverride: ABSENT from CompilerOptions (§13
  audit) → only override-present faces live; the needs-override
  rows stay dead w/ note. baseClassName/className =
  typeToString (display band containment applies).

implements list: per node: non-entity-name/optional-chain →
A_class_can_only_implement_an_identifier_Slashqualified_name_with_
optional_type_arguments; checkTypeReferenceNode(typeRefNode) (same
heritage generalization); LAZY: t = getReducedType(getTypeFromType
Node); non-error: isValidBaseType(t) → baseWithThis = withThis(t);
silent relation fail → issueMemberSpecificError(node, typeWithThis,
baseWithThis, class-symbol-t ? Class_0_incorrectly_implements_class_
1_Did_you_mean_to_extend_1_and_inherit_its_members_as_a_subclass :
Class_0_incorrectly_implements_interface_1); !isValidBaseType →
A_class_can_only_implement_an_object_type_or_intersection_of_object_
types_with_statically_known_members.

Final LAZY: checkIndexConstraints(type, symbol) +
checkIndexConstraints(staticType, symbol, /*isStaticIndex*/ true) +
checkTypeForDuplicateIndexSignatures (§5) +
checkPropertyInitialization.

### issueMemberSpecificError (L85233)
Per NON-static member with a declared prop (member.name symbol):
prop = getPropertyOfType(typeWithThis, name), baseProp = …(
baseWithThis, name); both → checkTypeAssignableTo(getTypeOfSymbol(
prop), getTypeOfSymbol(baseProp), member.name||member, headMessage=
undefined, rootChain: Property_0_in_type_1_is_not_assignable_to_the_
same_property_in_base_type_2 (symbolToString, two typeToStrings)) —
the CHAIN ROOT is the reported code/message (reuse the calls.rs
chain support; check_type_assignable_to needs the optional chain
param). Any member failure suppresses the broad row; else broad
checkTypeAssignableTo(typeWithThis, baseWithThis, node.name||node,
broadDiag) (2415/2420/2720 heads).

### checkKindsOfPropertyMemberOverrides (L85315) — transcribe
Per base property (skip Prototype-flagged): base = getTargetSymbol
(instantiated → links.target); baseSymbol = getPropertyOfObjectType(
type, name); absent → continue; derived = target(baseSymbol).
- derived === base (inherited, not overridden): base ABSTRACT-
  flagged && derived class not abstract → check OTHER base types
  first (a different base implementing it elides — continue outer);
  collect per-derived-class-decl notImplementedInfo {baseTypeName,
  typeName, missedProperties[]} (typeToString displays).
- derived ≠ base: either side Private-modifier → skip. Both
  property-or-accessor flagged: skip when base is
  abstract-or-interface property everywhere (Synthetic base → SOME
  declarations; else EVERY declaration — isPropertyAbstractOrInterface
  L85417: abstract && (!property-decl || no initializer) ||
  interface-parent) or base Mapped check-flag or derived
  valueDeclaration is a binary expression (JS); accessor→property
  override → _0_is_defined_as_an_accessor_in_class_1_but_is_
  overridden_here_in_2_as_an_instance_property; property→accessor →
  …defined_as_a_property…_as_an_accessor; else useDefineForClassFields
  (default TRUE at ES2022+): derived has an uninitialized
  PropertyDeclaration, non-transient, neither side abstract, no
  ambient declaration → UNLESS (no exclamationToken && constructor
  exists && identifier name && strictNullChecks &&
  isPropertyInitializedInConstructor) → Property_0_will_overwrite_
  the_base_property_in_1_… — the exception clause is FLOW ([M5]):
  at M4, report only the flow-free faces (exclamationToken present /
  no constructor / non-identifier name / !strictNullChecks) and
  ESCAPE when the constructor-initialization probe is required.
  Method faces: base prototype-property: derived prototype-or-
  property → skip; derived accessor → Class_0_defines_instance_
  member_function_1_but_extended_class_2_defines_it_as_instance_
  member_accessor; base accessor → …accessor_1_…_as_instance_member_
  function… wait — transcribe the three message selections exactly
  from L85377-85388 (function→accessor / accessor→function /
  property→function). Error AT derived valueDeclaration name.
- notImplementedInfo drain: 1 missing → Non_abstract_class_0_does_
  not_implement_inherited_abstract_member_1_from_class_2 (class
  expressions: Non_abstract_class_expression_does_not_implement_
  inherited_abstract_member_0_from_class_1); >5 → first-4 quoted +
  _and_N_more flavors; else joined list flavors. Quote format
  `'name'` joined ", ".

### checkPropertyInitialization (L85477) — the 2564 band
Gates: strictNullChecks && strictPropertyInitialization (BOTH strict-
family defaults ON) && !ambient. Per member: skip ambient-modifier /
static / !isPropertyWithoutInitializer (L85499: PropertyDeclaration,
no abstract, no exclamation, no initializer); identifier/private/
computed names; type = getTypeOfSymbol(member symbol); skip
Any|Unknown or contains-undefined; **!constructor → REPORT
Property_0_has_no_initializer_and_is_not_definitely_assigned_in_the_
constructor AT member.name — FLOW-FREE face, live at M4**;
constructor present → isPropertyInitializedInConstructor (L85517:
fabricated this.prop reference + getFlowTypeOfReference on
constructor.returnFlowNode) = [FLOW M5] → ESCAPE (recorded FN;
the M5 swap lifts it, plus isPropertyInitializedInStaticBlocks
L85502 for the static-block face). 2564 = 1,139 FN rows — the
no-constructor subset recovers NOW.

### checkInheritedPropertiesAreIdentical (L85439)
Interfaces with ≥2 base types; seen map from resolveDeclaredMembers
declaredProperties (containingType=type), then per base
getPropertiesOfType(withThis(base)): new → record (containingType=
base); existing with containingType ≠ type && !isPropertyIdenticalTo
(relate.rs compareProperties identity — verify exposure) → chain
Named_property_0_of_types_1_and_2_are_not_identical UNDER Interface_
0_cannot_simultaneously_extend_types_1_and_2, diagnostic AT the
INTERFACE NAME node (typeNode param = node.name), code = outer
chain. Returns ok (gates the base-assignability loop).

### checkIndexConstraints (L84705)
indexInfos of type; none → return. Per property of object type
(skip Prototype-flagged under isStaticIndex):
checkIndexConstraintForProperty(type, prop, getLiteralTypeFromProperty
(prop, StringOrNumberLiteralOrUnique, includeNonPublic=true),
getNonMissingTypeOfSymbol(prop)). Class declarations additionally
run NON-bindable-name members (computed names) per static-ness with
getTypeOfExpression(member.name.expression) as the name type.
>1 index infos → pairwise checkIndexConstraintForIndexSignature.
- ForProperty (L84735): private-identifier names skip;
  applicable infos via getApplicableIndexInfos(type, propNameType)
  (indexed.rs kit); errorNode selection: LOCAL prop declaration
  (parent symbol === type.symbol) || LOCAL index declaration ||
  (interface type && no base carries both the property and the
  index → the interface declaration) — else SILENT (inherited
  conflicts report only on the declaring interface); fail →
  Property_0_of_type_1_is_not_assignable_to_2_index_type_3
  (+related _0_is_declared_here at binary/computed propDeclaration
  when errorNode differs).
- ForIndexSignature (L84757): per other info; errorNode = local
  check declaration || local other-index declaration || interface-
  without-base-carrying-both; fail → _0_index_type_1_is_not_
  assignable_to_2_index_type_3.

## §7 Interface / type alias / enum completions

### checkInterfaceDeclaration (L85525) — completes the 5.4 slice
Order: grammar-modifiers hook → checkGrammarInterfaceDeclaration
(grammar band); !allowBlockDeclarations(parent) → _0_declarations_
can_only_be_declared_inside_a_block("interface");
checkTypeParameters (5.4 live); LAZY: checkTypeNameIsReserved (5.4
live — MOVE under the lazy identity, order-sensitive only vs new
rows); checkExportsOnMergedDeclarations (§5);
checkTypeParameterListsIdentical (§6); node === FIRST interface
declaration → type/typeWithThis; checkInheritedPropertiesAre
Identical(type, node.name) (§6) OK → per baseType:
checkTypeAssignableTo(typeWithThis, withThis(baseType, thisType),
node.name, Interface_0_incorrectly_extends_interface_1) +
checkIndexConstraints(type, symbol) (§6);
checkObjectTypeForDuplicateDeclarations (§5). EAGER: per
getInterfaceBaseTypeNodes: non-entity/optional-chain → An_interface_
can_only_extend_an_identifier_Slashqualified_name_with_optional_
type_arguments; checkTypeReferenceNode(heritage) — heritage
generalization (§6); forEach members checkSourceElement (5.4 live);
LAZY: checkTypeForDuplicateIndexSignatures (§5) + M7 register.
NOTE the interface-extends relation reports at node.NAME with the
2430 head — no member-specific elaboration (unlike classes).

### checkTypeAliasDeclaration (L85561) — delta over the 5.4 slice
Adds: !allowBlockDeclarations(parent) → block-decl row ("type");
checkExportsOnMergedDeclarations. Everything else (name-reserved,
type params, intrinsic-keyword arm, checkSourceElement(type))
already landed at 5.4 — verify order matches L85562-85578 exactly
(name-reserved BEFORE block-decl row).

### Enum band (L85580-85839) — the evaluator is ALREADY PORTED (5.3b)
computeEnumMemberValues/computeEnumMemberValue/
computeConstantEnumMemberValue/evaluate* live in evaluate.rs with
their diagnostic rows (numeric-name 2452-family, 1061,
isolatedModules rows behind getIsolatedModules — OPTION unmodeled →
those rows dead w/ note, const-enum NaN/non-finite, 2474, 1066,
computed-value assignability). 5.8 adds ONLY the drivers:
checkEnumDeclaration (L85767) / checkEnumDeclarationWorker (L85770) /
checkEnumMember (L85810) / getFirstNonAmbientClassOrFunction
Declaration (L85818) / inSameLexicalScope (L85829) — EXTRACTED IN
§8 (Enum drivers); this section only scopes the split: the evaluator
is 5.3b, the drivers are 5.8c.

## §8 Enum drivers + module band

### checkEnumDeclaration (L85767) — entirely LAZY (worker L85770)
grammar-modifiers hook; checkCollisionsForDeclarationName (§2 —
Enum_name_cannot_be_0 rides inside); checkExportsOnMergedDeclarations
(§5); members forEach checkSourceElement → checkEnumMember;
erasableSyntaxOnly row (option unmodeled → dead, note);
computeEnumMemberValues — ALREADY PORTED (evaluate.rs 5.3b; its
rows go live on this driver path — verify the isolatedModules-gated
rows inside stay dead w/ the option note); node === FIRST enum
declaration:
- >1 declarations: const/non-const mismatch per declaration →
  Enum_declarations_must_all_be_const_or_non_const AT each offending
  declaration name;
- cross-declaration rule: among enum declarations WITH members,
  every first member lacking an initializer after the first such
  declaration → In_an_enum_with_multiple_declarations_only_one_
  declaration_can_omit_an_initializer_for_its_first_enum_element AT
  that first member's name (declaration order; single-latch bool).

### checkEnumMember (L85810)
private-identifier name → An_enum_member_cannot_be_named_with_a_
private_identifier; initializer → checkExpression (idempotent over
the evaluator's checkExpressionCached demand).

### checkModuleDeclaration (L85840)
EAGER: body → checkSourceElement(body) (ModuleBlock arm = existing
check_block; namespace bodies DRIVE from here — the expr.rs 2683
probe note un-blocks); non-global-augment → M7 register. LAZY:
- global augment && !ambient → Augmentations_for_the_global_scope_
  should_have_declare_modifier_unless_they_appear_in_already_
  ambient_context AT name.
- checkGrammarModuleElementContext(node, ambient-external ?
  An_ambient_module_declaration_is_only_allowed_at_the_top_level_
  in_a_file : A_namespace_declaration_is_only_allowed_at_the_top_
  level_of_a_namespace_or_module) (L86347 — grammar sweep) → true →
  RETURN (context errors suppress the rest).
- grammar-modifiers hook chain: !ambient && string-literal name →
  Only_ambient_modules_can_use_quoted_names.
- identifier name → checkCollisionsForDeclarationName; node flags
  lack Namespace|GlobalAugmentation → A_namespace_declaration_
  should_not_be_declared_using_the_module_keyword_Please_use_the_
  namespace_keyword_instead (the TS6 `module X {}` row — requires
  the parser-set NodeFlags.Namespace bit; VERIFY tsrs2 parser stamps
  it, else this row FPs on every `namespace`).
- checkExportsOnMergedDeclarations (§5).
- ValueModule symbol && !ambient && isInstantiatedModule(node,
  shouldPreserveConstEnums(options)) (binder getModuleInstanceState
  — verify exposure):
  - erasableSyntaxOnly / isolatedModules / verbatimModuleSyntax
    rows: options unmodeled → dead w/ notes;
  - >1 declarations: getFirstNonAmbientClassOrFunctionDeclaration
    (L85818: first non-ambient class or bodied function) → different
    source file → A_namespace_declaration_cannot_be_in_a_different_
    file_from_a_class_or_function_with_which_it_is_merged; node.pos
    < its pos → A_namespace_declaration_cannot_be_located_prior_to_
    a_class_or_function_with_which_it_is_merged (both AT name);
    merged class declaration in same lexical scope (L85829) →
    NodeCheckFlags LexicalModuleMergesWithClass (emit-read flag —
    port the write).
- ambient external module: external-module AUGMENTATION →
  (global-augment || Transient module symbol) && body → per body
  statement checkModuleAugmentationElement(statement, isGlobal);
  else parent is global source file: global-augment → Augmentations_
  for_the_global_scope_can_only_be_directly_nested_in_external_
  modules_or_ambient_module_declarations; relative module name →
  Ambient_module_declaration_cannot_specify_relative_module_name;
  else nested: global-augment → same directly-nested row; else →
  Ambient_modules_cannot_be_nested_in_other_modules_or_namespaces.

### checkModuleAugmentationElement (L85925)
VariableStatement → recurse per declaration; ExportAssignment/
ExportDeclaration → Exports_and_export_assignments_are_not_
permitted_in_module_augmentations (first token); ImportEquals
(external only — internal `import A = B.C` allowed) /
ImportDeclaration → Imports_are_not_permitted_in_module_
augmentations_Consider_moving_them_to_the_enclosing_external_module;
binding patterns recurse elements. (The 5.7b-era declare-global
FP class lands here + §1's lift dependency.)

### checkExternalImportOrExportDeclaration (L85983)
moduleName missing → false; non-StringLiteral → String_literal_
expected; parent not SourceFile nor ambient-module block →
ExportDeclaration ? Export_declarations_are_not_permitted_in_a_
namespace : Import_declarations_in_a_namespace_cannot_reference_a_
module; ambient-module + relative specifier (not top-level in an
external-module augmentation) → Import_or_export_declaration_in_an_
ambient_module_declaration_cannot_reference_module_through_relative_
module_name AT node; import attributes (non-ImportEquals):
non-string attribute values → Import_attribute_values_must_be_
string_literal_expressions (with-token) / Import_assertion_values_
must_be_string_literal_expressions (assert-token), return !hasError.

### checkModuleExportName (L86019)
string-literal export/import name: !allowStringLiteral →
Identifier_expected; moduleKind ES2015|ES2020 → String_literal_
import_and_export_names_are_not_supported_when_the_module_flag_is_
set_to_es2015_or_es2020 (modeled module option read).

## §9 Import/export/alias band (checker half; resolveAlias family pending)

### checkAliasSymbol (L86029) — the shared alias check
target = resolveAlias(symbol) — **THE §9 dependency**; target ===
unknownSymbol → whole check skips (resolveAlias reports its own
2303/2305/2307-family during resolution). symbol = getMergedSymbol(
symbol.exportSymbol || symbol). JS arm elided. LIVE core:
- targetFlags = getSymbolFlags(target) (alias-aware flag union —
  grep anchor at landing); excludedMeanings from the LOCAL symbol's
  meanings (Value|ExportValue → Value; Type; Namespace); overlap →
  ExportSpecifier ? Export_declaration_conflicts_with_exported_
  declaration_of_0 : Import_declaration_conflicts_with_local_
  declaration_of_0 AT node (symbolToString display).
- isolatedModules/verbatimModuleSyntax/Preserve-CJS/ambient-const-
  enum faces (L86074-86128): ALL behind unmodeled options → dead w/
  notes (getIsolatedModules = isolatedModules || verbatimModuleSyntax).
- ImportSpecifier deprecation walk (L86138) → suggestion band, skip.

### checkImportBinding (L86163)
checkCollisionsForDeclarationName (§2); checkAliasSymbol;
ImportSpecifier → checkModuleExportName(propertyName) (§8) +
esModuleInterop default-import emit-helper probe (no-op).

### checkImportDeclaration (L86220)
checkGrammarModuleElementContext(node, An_import_declaration_can_
only_be_used_at_the_top_level_of_a_namespace_or_module) → return;
modifiers present (post grammar-modifiers hook) → An_import_
declaration_cannot_have_modifiers (first token);
checkExternalImportOrExportDeclaration (§8) →
- importClause (+ !checkGrammarImportClause — grammar sweep):
  name → checkImportBinding(importClause); namedBindings:
  NamespaceImport → checkImportBinding (+interop helper no-op);
  NamedImports → **resolvedModule = resolveExternalModuleName(node,
  moduleSpecifier); resolved → forEach elements checkImportBinding**
  — named bindings check ONLY when the module resolves.
- Node18..NodeNext JSON default-import row (modeled module-option
  read; isOnlyImportableAsDefault + hasTypeJsonImportAttribute
  L86262); noUncheckedSideEffectImports row (option → dead).
- checkImportAttributes(node) (L86173: ImportAttributes global-type
  relation + moduleSupportsImportAttributes(moduleKind) grammar rows
  + assert-deprecation rows (ignoreDeprecations option → the
  non-Node20 assert row is LIVE by default — verify) + CJS-emit rows
  + type-only row + resolution-mode row; getTypeFromImportAttributes
  builds the object type from checkImportAttribute L86217 =
  getRegularTypeOfLiteralType(checkExpressionCached(value))).

**resolveExternalModuleName is 5.8-owned now** (the 5.7b import-call
silent stub UN-silences for declarations): minimal worker = relative/
absolute specifiers against program files + ambient module symbols +
pattern ambient modules; misses emit Cannot_find_module_0… (2307
family — 480 FN rows) — EXTRACT the worker family (L~48800-48950)
with resolveAlias next part. Import-call sites KEEP the silent stub
(m4-57 §6 risk 7) until this lands; then both route through one
worker.

### checkImportEqualsDeclaration (L86268)
context gate (same import message); grammar-modifiers hook;
erasableSyntaxOnly (dead); (internal-module form ||
checkExternalImportOrExportDeclaration) →
checkImportBinding; NON-external module reference: target =
resolveAlias(symbol) ≠ unknown → targetFlags Value: first identifier
of moduleReference resolveEntityName(Value|Namespace) NOT
Namespace-flagged → Module_0_is_hidden_by_a_local_declaration_with_
the_same_name AT that identifier; targetFlags Type →
checkTypeNameIsReserved(node.name, Import_name_cannot_be_0);
isTypeOnly → An_import_alias_cannot_use_import_type (grammar).
EXTERNAL reference form: moduleKind ES2015..ESNext && !typeOnly &&
!ambient → Import_assignment_cannot_be_used_when_targeting_
ECMAScript_modules… (grammar; modeled module option).

### checkExportDeclaration (L86303) + checkExportSpecifier (L86354)
context gate (export message); modifiers → An_export_declaration_
cannot_have_modifiers; checkGrammarExportDeclaration (L86340:
type-only + NamedExports → checkGrammarNamedImportsOrExports —
grammar sweep). (no specifier || external-decl ok):
- Named exports (non-namespace clause): forEach checkExportSpecifier;
  non-(SourceFile | ambient-module block | ambient namespace block)
  parent → Export_declarations_are_not_permitted_in_a_namespace.
- star/namespace-export: moduleSymbol = resolveExternalModuleName;
  hasExportAssignmentSymbol(moduleSymbol) → Module_0_uses_export_
  and_cannot_be_used_with_export_Asterisk AT specifier;
  namespace-export clause → checkAliasSymbol(clause) +
  checkModuleExportName(clause.name). Emit helpers no-op.
- checkImportAttributes tail.
checkExportSpecifier: checkAliasSymbol; checkModuleExportName(
propertyName, allowStringLiteral = has specifier) + (name);
declaration-emit collect = no-op; NO specifier: string-literal
exported name → return; resolveName(exportedName, Value|Type|
Namespace|Alias, no-error, isUse=TRUE); symbol is undefinedSymbol/
globalThisSymbol/declared in a GLOBAL source file → Cannot_export_0_
Only_local_declarations_can_be_exported_from_a_module AT name.

### checkExportAssignment (L86391)
context gate (An_export_assignment_must_be_at_the_top_level… /
A_default_export_must_be_at_the_top_level…); erasable (dead);
container = SourceFile | parent.parent; non-ambient ModuleDeclaration
container → export= ? An_export_assignment_cannot_be_used_in_a_
namespace : A_default_export_can_only_be_used_in_an_ECMAScript_style_
module; RETURN. modifiers → An_export_assignment_cannot_have_
modifiers. (JSDoc type-annotation arm — JS, elide.) Identifier
expression: sym = getExportSymbolOfValueSymbolIfExported(
resolveEntityName(id, ALL meanings, ignoreErrors, dontResolveAlias=
TRUE, location=node)); sym: getSymbolFlags(sym) & Value →
checkExpressionCached(id) — **a pure-type export= does NOT check
the expression** (no 2693 here); verbatim/isolatedModules rows dead;
!sym → checkExpressionCached(id). Non-identifier →
checkExpressionCached(expression). checkExternalModuleExports(
container) (§0 drain ALSO runs from here — symbolLinks.exportsChecked
once-guard dedupes). Ambient + non-entity expression → The_
expression_of_an_export_assignment_must_be_an_identifier_or_
qualified_name_in_an_ambient_context (grammar). export= tails:
moduleKind ≥ ES2015 && ≠ Preserve && (per impliedNodeFormat-for-emit
arms) → Export_assignment_cannot_be_used_when_targeting_ECMAScript_
modules… ; System non-ambient → Export_assignment_is_not_supported_
when_module_flag_is_system. (impliedNodeFormat: single-file
conformance → None; transcribe the arms with the host read.)

### resolveAlias protocol (L49109-49229) — the resolve.rs elision lifts
- resolveAlias (L49116): SymbolLinks.aliasTarget slot protocol —
  unset → write resolvingSymbol SENTINEL → node =
  getDeclarationOfAliasSymbol → target = getTargetOfAliasDeclaration
  (node) → slot still sentinel ? (target || unknownSymbol) :
  **Circular_definition_of_import_alias_0** error AT node (a
  re-entrant write happened); sentinel found ON ENTRY → unknownSymbol
  (cycle collapse). Rust: SymbolLinks.alias_target via the LinkSlot
  resolving-state pattern (the resolvedSignature twin), NOT
  write-once. tryResolveAlias (L49134): sentinel → None (spell.rs's
  tryResolveAlias-chase escape lifts).
- resolveSymbol (L49113) = isNonLocalAlias(symbol) && !dontResolve →
  resolveAlias : symbol; isNonLocalAlias (L49109): (flags & (Alias |
  excludes)) === Alias || (Alias && Assignment) with excludes
  defaulting Value|Type|Namespace — resolve.rs's getSymbol/
  resolveEntityName sites that currently return aliases unchanged
  route through THIS predicate pair.
- getTargetOfAliasDeclaration (L49071) kind dispatch (TS core):
  ImportEqualsDeclaration → getTargetOfImportEqualsDeclaration;
  ImportClause; NamespaceImport; NamespaceExport; ImportSpecifier;
  ExportSpecifier (meaning Value|Type|Namespace); ExportAssignment;
  NamespaceExportDeclaration; PropertyAssignment/Shorthand/access
  expressions/VariableDeclaration/BindingElement = JS arms (elide
  w/ note). Per-kind workers (L48652-49070) are TRANSCRIBED below in
  this section ("Per-kind alias targets").
- getSymbolFlags (L49141): alias-chain flag union — walk
  getExportSymbolOfValueSymbolIfExported(resolveAlias(symbol)) while
  Alias-flagged, accumulating flags with a seen-set; unknownSymbol →
  ALL. excludeTypeOnlyMeanings arm needs getTypeOnlyAliasDeclaration
  + export-star resolution (checkAliasSymbol's §9 caller passes no
  excludes — port the plain walk first, the exclude arms with the
  isolatedModules faces they serve).
- Type-only marking protocol (L49176-49229): SymbolLinks additions
  typeOnlyDeclaration: TRI-STATE (unset | false | NodeId) +
  typeOnlyExportStarName; markSymbolOfAliasDeclarationIfTypeOnly
  writes from type-only declarations / export-star declarations /
  target-links propagation (worker L49195: target's export= symbol
  or self; first-write-wins unless overwriteEmpty over false);
  getTypeOnlyAliasDeclaration (L49204): lazy compute on first read
  via resolveSymbol + mark(declarations[0], immediate, final,
  overwriteEmpty=true); include-filtered flavor resolves through
  getExportsOfModule for export-star marks. Consumers: checkAliasSymbol
  isolatedModules faces (dead now), checkExportAssignment rows
  (dead), export-specifier faces. Port the LINKS + lazy protocol
  with the per-kind workers; dead consumers keep their option notes.

### Per-kind alias targets (L48481-49070) — transcribed
Shared kit: getAnyImportSyntax (L48481), getDeclarationOfAliasSymbol
(L48495: findLast over isAliasSymbolDeclaration L48498 — TS core
kinds + JS arms to elide), getModuleSpecifierForImportOrExport
(L48730).
- getTargetOfImportEqualsDeclaration (L48504): (JS require/property
  arms elide); ExternalModuleReference form → immediate =
  resolveExternalModuleName(node, expression); resolved =
  resolveExternalModuleSymbol(immediate); (Node20+ module.exports
  arm — mode machinery, dead at default); mark(node, immediate,
  resolved, overwriteEmpty=false); return resolved. Entity-name form
  → getSymbolOfPartOfRightHandSideOfImportEquals (L49230 —
  transcribe at landing: resolveEntityName-based namespace walk) +
  checkAndReportErrorForResolvingImportAliasToTypeOnlySymbol
  (L48535: marked type-only && !node.isTypeOnly →
  An_import_alias_cannot_reference_a_declaration_that_was_
  [exported_using_export_type | imported_using_import_type] AT
  moduleReference + related _0_was_[exported|imported]_here at the
  type-only declaration; name arg "*" for export-star declarations).
- resolveExportByName (L48552): export= present →
  getPropertyOfType(typeof export=, name, skipObjectFunctionProperty
  Augment=TRUE) else exports.get(name); resolveSymbol + mark.
- Synthetic-default kit (L48570-48651): isSyntacticDefault;
  isOnlyImportableAsDefault (JSON-under-node16+ — mode machinery);
  canHaveSyntheticDefault — **allowSyntheticDefaultImports** =
  option || esModuleInterop || module===System: **NEITHER option is
  in CompilerOptions yet — 5.8d ADDS es_module_interop +
  allow_synthetic_default_imports + the computed derivation (19
  conformance fixtures set @esModuleInterop; standalone
  @allowSyntheticDefaultImports does not appear in conformance —
  the field carries the computed value; the §9 default-import gates
  are load-bearing)**; declaration files: probe syntactic default +
  __esModule marker; TS files → hasExportAssignmentSymbol (L49778).
- getTargetOfImportClause (L48652) → getTargetofModuleDefault
  (L48658): shorthand-ambient module → the module symbol; (Node20
  CJS→ESM module.exports arm dead); exportDefaultSymbol =
  resolveExportByName(moduleSymbol, "default", node); no specifier →
  return; NO default/synthetic/default-only:
  hasExportAssignmentSymbol && !allowSyntheticDefaultImports →
  Module_0_can_only_be_default_imported_using_the_1_flag (flag name
  "allowSyntheticDefaultImports" when moduleKind ≥ ES2015 else
  "esModuleInterop") AT node.name + related This_module_is_declared_
  with_export_and_can_only_be_used_with_a_default_import_when_using_
  the_0_flag at the export= declaration; ImportClause →
  reportNonDefaultExport (L48746: module exports the LOCAL name →
  Module_0_has_no_default_export_Did_you_mean_to_use_import_1_from_
  0_instead AT name; else Module_0_has_no_default_export + related
  export_Asterisk_does_not_re_export_a_default at an export-star
  redistributing a default); else errorNoModuleMemberSymbol.
  synthetic/default-only face → resolveExternalModuleSymbol(module)
  || resolveSymbol(module) + mark → return. Final mark + return.
- getTargetOfNamespaceImport (L48771) / NamespaceExport (L48790):
  resolveESModuleSymbol(immediate, specifier, dontResolveAlias,
  suppressInteropError=false) + mark.
- getExternalModuleMember (L48851): targetSymbol =
  resolveESModuleSymbol(module, specifier, false,
  suppressInteropError = name==="default" && allowSynthetic);
  shorthand-ambient → module symbol; export= module →
  symbolFromVariable = getPropertyOfType(typeof target, name,
  skipAugment=TRUE) else getPropertyOfVariable (L48843: annotated
  Variable → property of annotation type); symbolFromModule =
  getExportOfModule (L48825: Module flag → getExportsOfSymbol(
  symbol).get(name) — routes through the LATE-BINDING container
  path §1 + typeOnlyExportStarMap mark); default-name synthetic
  fallback; both differ → combineValueAndTypeSymbols (L48809:
  transient merge of flags/declarations/members/exports); JSON
  named-import row (mode, dead); **!symbol →
  errorNoModuleMemberSymbol (L48902)**: spelling suggestion
  (getSuggestedSymbolForNonexistentModule, spell.rs) →
  _0_has_no_exported_member_named_1_Did_you_mean_2 + related
  _0_is_declared_here; exports has "default" → Module_0_has_no_
  exported_member_1_Did_you_mean_to_use_import_1_from_0_instead;
  else reportNonExportedMember (L48926): module-local symbol of that
  name: export= same-reference → reportInvalidImportEqualsExport
  Member (L48945: three flavors by moduleKind ≥ ES2015 / JS /
  esModuleInterop — args differ per flavor); exported under another
  name → Module_0_declares_1_locally_but_it_is_exported_as_2 +
  related declaration chain (_0_is_declared_here / and_here); else
  Module_0_declares_1_locally_but_it_is_not_exported + chain; no
  local → **Module_0_has_no_exported_member_1 (2305)**.
- getTargetOfImportSpecifier (L48959): default-named → module-default
  path; else getExternalModuleMember(importDecl, node) + mark.
- getTargetOfExportSpecifier (L49003): default-named → module-default;
  with specifier → getExternalModuleMember(exportDecl, node);
  local (no specifier): string-literal name → undefined (invalid
  syntax skip); else resolveEntityName(name, Value|Type|Namespace,
  **ignoreErrors=FALSE** — the resolver emits its 2304-family) +
  mark.
- getTargetOfExportAssignment (L49032) → getTargetOfAliasLike
  Expression (L49045): ClassExpression → checkExpressionCached.symbol;
  non-entity → undefined; resolveEntityName(all meanings,
  ignoreErrors=TRUE); fallback checkExpressionCached +
  links.resolvedSymbol read. NamespaceExportDeclaration (L48989):
  resolveExternalModuleSymbol(sourceFile.symbol) + mark (UMD).

### Module resolution (L49465-49682) — the 2307 band
- resolveExternalModuleName (L49465): default errorMessage =
  getCannotResolveModuleNameErrorForSpecificModule (L69377 —
  known-package suggestion table, transcribe at landing) ?? (Classic
  resolution ? Cannot_find_module_0_Did_you_mean_to_set_the_
  moduleResolution_option… : **Cannot_find_module_0_or_its_
  corresponding_type_declarations (2307)**). The Classic-vs-not
  selection reads getEmitModuleResolutionKind — **moduleResolution
  is NOT in CompilerOptions yet: 5.8d ADDS the field + mapping +
  the computed default (transcribe _computedOptions.moduleResolution
  .computeValue at landing); 85 conformance fixtures set
  @moduleResolution (43 classic)**. Worker (L49470): string-literal
  specifiers only.
- resolveExternalModule (L49473) — port order EXACTLY:
  1. errorNode && "@types/" prefix → Cannot_import_type_declaration_
     files_Consider_importing_0_instead_of_1;
  2. tryFindAmbientModule(ref, withAugmentations=true) (L59499) →
     hit → return (ambient `declare module "x"` — including quoted
     lookups in globals; PLUS patternAmbientModules fallback below);
  3. host.getResolvedModule(currentSourceFile, ref, mode) — **THE
     PROGRAM SEAM**: tsrs2's conformance program must implement a
     minimal resolver for multi-file fixtures (relative specifiers
     against in-program file names with .ts/.tsx/.d.ts probing; the
     harness's fixture layout is the spec — verify how @filename
     fixtures map to InputFile names before wiring); mode/
     impliedNodeFormat machinery reduces at the modeled defaults;
  4. resolved source file: resolution-diagnostic band + ts-extension
     rows + rewriteRelativeImportExtensions rows (options → dead
     notes); file.symbol → (external-library implicit-any =
     suggestion band; Node16/18 ESM-require chain rows = mode
     machinery, dead default) → **getMergedSymbol(sourceFile.symbol)**;
     no symbol → File_0_is_not_a_module (non-side-effect imports);
  5. patternAmbientModules best-match (wildcard ambient modules;
     patternAmbientModuleAugmentations map);
  6. error tail: project-reference redirect (dead);
     resolutionDiagnostic; JSON-module row (option); ESM
     extensionless-relative rows (mode, dead); alternateResult chain
     (dead); **else error(errorNode, moduleNotFoundError, ref)** —
     the live 2307/Classic row.
  isForAugmentation flavor → Invalid_module_name_in_augmentation_…
  rows (untyped resolution face — dead-ish; the NOT-FOUND face for
  augmentations rides checkModuleDeclaration→resolveExternalModule
  Name with isForAugmentation=true — verify the call site when
  porting §8).
- errorOnImplicitAnyModule (L49664): suggestion/error under
  noImplicitAny for untyped resolutions — external-library-gated;
  dead for fixture programs, note.
- **UN-SILENCING ORDER**: land the worker + program resolver FIRST,
  then flip the 5.7b import-call silent stub (calls.rs
  checkImportCallExpression) to route through it — one shared
  worker, two call-site flavors (declarations report, import-calls
  report the SAME rows in tsc — re-verify the m4-57 §6 "silent"
  decision at that point; it was FP-avoidance for fabricated 2307s,
  which the real worker eliminates).

### Module symbol resolution + exports (L49683-49931)
- resolveExternalModuleSymbol (L49683): export= chase →
  getCommonJsExportEquals (L49691: clone + ValueModule flag + merge
  the module's OTHER exports into the export= target's exports via
  mergeSymbol; cjsExportMerged links memo; skip when exports.size ===
  1 or alias-flagged) — **depends on mergeSymbol (L47707)**: merge.rs
  owns the port; its module-resolution rows un-defer here.
- resolveESModuleSymbol (L49715): non-module non-variable export=
  target referenced from ESM namespace-import/import-call (+ !
  suppressInteropError) → This_module_can_only_be_referenced_with_
  ECMAScript_imports_Slashexports_by_turning_on_the_0_flag_and_
  referencing_its_default_export; esModuleInterop/mode synthetic-
  default MODULE TYPE cloning (getTypeWithSyntheticDefaultOnly /
  getTypeWithSyntheticDefaultImportType + cloneTypeAsModuleType
  L49764 — clones the symbol with links.target + links.
  originatingImport = the m4-57 invocationErrorRecovery 7038 hook +
  a fresh anonymous type over the module members): gate on the
  verified esModuleInterop mapping; unmodeled → plain symbol
  passthrough w/ note (FN on interop-shaped fixtures, no FP).
- getExportsOfModule (L49837) + Worker (L49868): links.
  resolvedExports + typeOnlyExportStarMap memo; worker: visited-set
  walk from resolveExternalModuleSymbol(module); __export star
  declarations → per-declaration resolveExternalModuleName +
  recursive visit (type-only propagates) + extendExportSymbols
  (L49846: "default" NEVER propagates through export-star; conflict
  lookupTable) → duplicate distinct-resolution names →
  **Module_0_has_already_exported_a_member_named_1_Consider_
  explicitly_re_exporting_to_resolve_the_ambiguity** per export-star
  node (skip export= and own-export shadowed names); type-only-star
  name stamping; nonTypeOnlyNames prune the map. **This lifts
  annotate.rs's getExportsOfModuleWorker escape (3173) and
  access.rs's typeOnlyExportStarMap read (1603); getExportsOfSymbol
  (L49834) then routes Module symbols here (the 5.7c
  get_exports_of_jsx_factory_symbol carve-out folds back in).**
- mergeModuleAugmentation (L47830) + mergeSymbolTable (L47818):
  checker-init module/global augmentation merging — merge.rs's
  deferred rows; extraction of the exact merge order rides the
  implementation slice (anchors recorded; the §1 lift depends on
  the declare-global face).

## §4-addendum: generator body inference (captured with §10 reads)

getReturnTypeFromBody's generator arm (L78752-78841) — the
functions.rs "[ITER] yield aggregation" escape lifts:
- Block-bodied generator: returnTypes = checkAndAggregateReturn
  ExpressionTypes (L78959 — the EXISTING 5.5f walk; verify its
  generator gate) → none → fallbackReturnType = neverType; some →
  Subtype union. {yieldTypes, nextTypes} =
  checkAndAggregateYieldOperandTypes (L78874 — transcribe at
  landing: walks yield expressions; yield* routes through
  getIterationTypesOf* §4); yieldType = Subtype union; nextType =
  INTERSECTION.
- Widening tail runs per component (reportErrorsFromWidening w/
  GeneratorYield/FunctionReturn/GeneratorNext flavors — widen.rs
  generator rows un-escape L68187); unit-type contextual
  de-literalization via getWidenedLiteralLikeTypeForContextual
  IterationTypeIfNeeded (L67784 — the 5.7b-close async arm's
  generator sibling); then getWidenedType each.
- Final: createGeneratorType(yield || never, return || fallback,
  next || getContextualIterationType(Next, func) (L72862) ||
  unknown, isAsync) — L78842: resolver-based; global Generator
  missing → IterableIterator fallback (read tail at landing);
  resolveIterationType per component (async → awaited).
- Async non-generator tail: createPromiseType/createPromiseReturn
  Type (L78702-78742: Promise<awaited T> reference; missing global →
  A_dynamic_import_call_returns_a_Promise… / An_async_function_or_
  method_must_return_a_Promise… (2712/2697) + ES5 ctor flavors) —
  the 5.5f kit's createPromiseReturnType already exists (verify
  which rows landed).

## §10 Decorators band

Ownership — **DUAL MODE (review round 2026-07-14)**:
legacyDecorators = compilerOptions.experimentalDecorators, which IS
modeled (options.rs experimental_decorators) AND mapped by the
conformance harness (134 conformance fixtures set
@experimentalDecorators).
Both modes are 5.8c scope:
- experimental_decorators=false (default): ES2022+ standard
  decorators — the ES signature table below.
- experimental_decorators=true: the LEGACY signature table
  (L78613-78698, fully transcribed below) — legacy-only faces go
  live under the flag: parameter decorators, the PropertyDeclaration
  void-head selection, nodeCanBeDecorated's legacy positions.
Never mix modes; every mode read routes through ONE
`legacy_decorators()` accessor. FALLBACK if legacy shapes produce
FP surprises at landing: contain the ENTIRE decorator band behind
Unsupported when experimental_decorators=true (honest FN on the 134
fixtures) — applying ES semantics to legacy fixtures is a
wrong-payload FP, never acceptable. emitDecoratorMetadata stays
unmapped → its entity-name machinery (L82698-82743) remains
emit-only/elided in both modes.

### checkDecorators (L82744) — the forcing entry
Gates: canHaveDecorators && hasDecorators && modifiers &&
nodeCanBeDecorated(**legacy_decorators()**, node, parent,
grandparent) (util — verify tsrs2_binder::node_util port carries the
legacy flavor: legacy admits PARAMETER positions that the ES flavor
rejects; passing constant false would gate legacy parameter
decorators out at the entry). Emit-helper probes: no-op.
markLinkedReferences → no-op hook. Per decorator modifier →
checkDecorator (L82628):
- checkGrammarDecorator (L82580): parse-diag gated; parenthesized →
  ok; walk expression through ExpressionWithTypeArguments/NonNull/
  Call(once)/PropertyAccess chains — second call, ?.-tokens, or
  non-identifier head → Expression_must_be_enclosed_in_parentheses_
  to_be_used_as_a_decorator AT decorator.expression + related
  Invalid_syntax_in_decorator AT the offending node.
- getResolvedSignature(node) — the calls.rs Decorator arm UN-STUBS:
  resolveSignature dispatch Decorator → resolveDecorator (L77298):
  checkExpression(expression); apparent error → resolveErrorCall;
  untyped-call → resolveUntypedCall; isPotentiallyUncalledDecorator
  && !parenthesized → _0_accepts_too_few_arguments_to_be_used_as_a_
  decorator_here_Did_you_mean_to_call_it_first_and_write_0 AT node
  (getTextOfNode source text) → errorCall; no call signatures →
  invocationErrorDetails chained under getDiagnosticHeadMessageFor
  DecoratorResolution (1329 Decorator_function_return_type…? NO —
  the head is 1241-family "Unable to resolve signature of X
  decorator when called as an expression" — resolve exact statics
  from gen.rs: getDiagnosticHeadMessageForDecoratorResolution maps
  parent kind → 1238/1239/1240/1241 family) + related + recovery →
  errorCall; else resolveCall(..., headMessage) — the failure
  ladder chains the decorator head OUTERMOST (m4-57 §3 shape,
  calls.rs already models headMessage chains).
- getEffectiveCallArguments' Decorator arm (L76310, calls.rs escape
  lifts) → getEffectiveDecoratorArguments (L76340, EXTRACTED):
  synthetic args from the decorator SIGNATURE's FULL parameter list
  — `for param in getDecoratorCallSignature(node).parameters:
  Synthetic(getTypeOfSymbol(param))` at the decorator EXPRESSION's
  span; no signature → Debug.fail (unreachable: resolveDecorator
  precedes). The effective-arg COUNT comes from the DECORATOR
  SIGNATURE alone: ES = 2 (target, context); legacy = 1 (class) /
  2 (plain property) / 3 (accessor-modifier property, method,
  getter, setter — the descriptor parameter is ALWAYS present in
  the signature — and parameter). Do NOT conflate with
  getLegacyDecoratorArgumentCount below: that is the ARITY
  ALLOWANCE for the CANDIDATE decorator function, not the effective
  list — method/getter/setter effective args are always 3, so the
  descriptor argument's assignability IS checked whenever the
  candidate declares a 3rd parameter.
- **Dual-mode call-machinery helpers (calls.rs — audit the existing
  5.7a ports)**:
  - getDecoratorArgumentCount (L76353): the ARITY ALLOWANCE (how
    many args a candidate may accept — NOT the effective-arg list;
    method/get/set are the only positions where it varies, by the
    CANDIDATE's own parameter count): experimentalDecorators →
    getLegacyDecoratorArgumentCount (L76359: class=1; property =
    accessor-modifier ? 3 : 2; method/get/set = CANDIDATE
    signature.parameters ≤ 2 ? 2 : 3; parameter=3) else ES
    `min(max(paramCount,1),2)` —
    calls.rs's hasCorrectArity decorator arm (m4-57 §4) cites this
    pair: verify the existing arm reads the REAL option, not a
    constant.
  - getThisArgumentOfCall (L76277): the Decorator arm is gated
    `node.kind === Decorator && !legacyDecorators` → ES decorators
    take a this-argument from an access-expression callee; LEGACY
    decorators take NONE. The 5.7a port predates decorator forcing —
    add the arm with the flag read.
  - isPotentiallyUncalledDecorator (L77469): every signature has
    minArg 0 && no rest && parameters.length <
    getDecoratorArgumentCount(decorator, signature) — consumes the
    mode-dependent count (the "Did_you_mean_to_call_it_first" probe
    in resolveDecorator).
- checkDeprecatedSignature → no-op/suggestion.
- returnType any → done. decoratorSignature =
  getDecoratorCallSignature (L78699 → ES flavor L78571):
  NodeLinks.decoratorSignature memo (anySignature sentinel = "no
  signature"); parent kind:
  - Class declaration/expression: targetType = static type;
    contextType = createClassDecoratorContextType(targetType)
    (L78468 — instantiates global ClassDecoratorContext<T>);
    signature = createESDecoratorCallSignature(target, context,
    RETURN targetType) (L78558: (target, context) =>
    return | void — read exact optionality at landing).
  - Method/accessors (class-parented only): valueType = method
    signature type / accessor type; thisType = static-modifier ?
    static type : declared instance type; target/return = getter →
    createGetterFunctionType(valueType) (L82677: () => V), setter →
    createSetterFunctionType (L82687: (value: V) => void), method →
    valueType; contextType = createClassMemberDecoratorContextType
    ForNode (L78524 — ClassMethodDecoratorContext/ClassGetter…/
    ClassSetter… global instantiations).
  - PropertyDeclaration (class-parented): valueType; thisType;
    accessor-modifier → ClassAccessorDecoratorTarget/Result global
    instantiations (L78532/78538) else target = undefinedType +
    return = createClassFieldDecoratorInitializerMutatorType
    (L78544: (this: This, value: V) => V shape).
  All the create*ContextType helpers instantiate GLOBAL lib types
  (ClassDecoratorContext etc., lib.decorators.d.ts — lib-loaded
  only; noLib → emptyGenericType-ish fallbacks: check each helper's
  miss arm at landing; unit pins hand-declare or tolerate).
- headMessage selection (L82640): class → Decorator_function_return_
  type_0_is_not_assignable_to_type_1; PropertyDeclaration: non-legacy
  → same, legacy → falls through to the void-or-any head; Parameter
  (reachable ONLY under experimental_decorators=true) →
  Decorator_function_return_type_is_0_but_is_expected_to_be_void_or_
  any; method/accessors → the assignable flavor. checkTypeAssignableTo(returnType,
  decoratorSignature.resolvedReturnType, node.expression, head).
- check.rs deferred Decorator arm un-unreachables:
  checkDeferredNode's Decorator → checkDecorator re-entry shape —
  verify tsc's deferred kind list handling (86923-86928 walks RAW
  arguments for overload-failure deferrals of decorators too).

### Legacy table (L78613-78698, LIVE under
experimental_decorators=true): class → (target) => target|void; parameter →
(target, propertyKey, parameterIndex: numberLiteral) => void (this-
param and index math transcribed); method/accessor/property →
(target = getParentTypeOfClassElement L87798, propertyKey =
getClassElementPropertyKeyType L87802, descriptor?:
TypedPropertyDescriptor<T> L61029) => T-descriptor|void (property
sans accessor-modifier omits descriptor, returns void|...).
emitDecoratorMetadata entity-name machinery (L82698-82743) =
emit-only, elide.

## §11 Type-node arms (L81838-82023)

- checkTypeQuery (L81838): getTypeFromTypeQueryNode force (annotate
  kit).
- checkTypeLiteral (L81841): members forEach checkSourceElement;
  LAZY: type = getTypeFromTypeLiteralOrFunctionOrConstructorTypeNode;
  checkIndexConstraints(type, type.symbol) (§6);
  checkTypeForDuplicateIndexSignatures;
  checkObjectTypeForDuplicateDeclarations (§5).
- checkArrayType (L81851): element recursion only — SELF-FORCING
  ABSENT (no re-entrancy trap exposure).
- checkTupleType (L81854): per element getTupleElementFlags walk:
  Variadic → getTypeFromTypeNode(e.type) NOT array-like →
  A_rest_element_type_must_be_an_array_type AT element, break;
  array/rest-carrying-tuple variadic → Rest reclass; Rest after
  Rest → A_rest_element_cannot_follow_another_rest_element
  (grammar); Optional after Rest → An_optional_element_cannot_
  follow_a_rest_element; Required after Optional → A_required_
  element_cannot_follow_an_optional_element; ALL break-on-first.
  THEN element recursion + **getTypeFromTypeNode(node) SELF-FORCE**
  (re-entrancy trap: route through the overwrite-tolerant cache or
  prove unreachable from default subtrees — §0).
- checkUnionOrIntersectionType (L81889): recursion + SELF-FORCE
  (same trap note).
- checkIndexedAccessType (L81919): recurse both; checkIndexedAccess
  IndexType (L81893) on the RESOLVED type: non-IndexedAccess pass;
  index assignable to keyof objectType (or applicable number index)
  → (element-access assignment to mapped readonly → Index_signature_
  in_type_0_only_permits_reading — expression-side twin exists in
  access.rs:3260 escape) → pass; generic objectType + property-name
  index + non-public member → Private_or_protected_member_0_cannot_
  be_accessed_on_a_type_parameter → errorType; else Type_0_cannot_
  be_used_to_index_type_1 → errorType. NOTE annotate.rs's indexed-
  access TYPE resolution already reports its own band — this arm is
  the CHECK-side; verify no double-report with the 5.2g resolver
  rows (tsc's resolver reports via the same helper on access
  EXPRESSIONS; type-node path reports HERE — pin `T['x']` bad-index
  shape).
- checkMappedType (L81924): checkGrammarMappedType (L81941: members
  present → A_mapped_type_may_not_declare_properties_or_methods);
  recurse typeParameter/nameType/type; !type → reportImplicitAny(
  node, anyType) (5.6 kit — 7040-family? number from gen.rs);
  type = getTypeFromMappedTypeNode (M8-stub in annotate.rs —
  **containment: when the mapped-type resolver escapes, the
  remaining rows escape with it**; the grammar + recursion + 7061
  rows above still fire); nameType present → checkTypeAssignableTo(
  nameType, stringNumberSymbolType, node.nameType) else
  constraint → …(constraintType, stringNumberSymbolType, effective
  constraint node) — keyof-side kit exists.
- checkThisType (L81947): getTypeFromThisTypeNode force.
- checkTypeOperator (L81950): checkGrammarTypeOperatorNode (grammar
  band — unique/readonly position rules 1331/1354/1355) + operand
  recursion.
- checkConditionalType (L81954): forEachChild recursion ONLY (the
  M8-stub stays on the annotate side — no self-force here).
- checkInferType (L81957): ancestor probe for conditional-extends
  position → infer_declarations_are_only_permitted_in_the_extends_
  clause_of_a_conditional_type (grammar); recurse typeParameter;
  multi-declaration constraint identity via areTypeParametersIdentical
  (§6 kit) once-latched on symbolLinks.typeParametersChecked →
  All_declarations_of_0_must_have_identical_constraints per
  declaration name; M7 register.
- checkTemplateLiteralType (L81979): per span: recurse + span type
  assignable to templateConstraintType (string|number|bigint|
  boolean|null|undefined intrinsic union — state singleton, verify
  tables) AT span.type; self-force tail.
- checkImportType (L81987): recurse argument; attributes: assert
  deprecation row + getResolutionModeOverride grammar;
  checkTypeReferenceOrImport tail (check.rs 5.4 port GENERALIZES:
  its current TypeReference-only data assert must accept ImportType
  — same §6 heritage generalization family).
- checkNamedTupleMember (L81997): rest+optional → A_tuple_member_
  cannot_be_both_optional_and_rest; OptionalType inner → A_labeled_
  tuple_element_is_declared_as_optional_with_a_question_mark_after_
  the_name…; RestType inner → …declared_as_rest_with_a_before_the_
  name…; recurse + self-force.
- isPrivateWithinAmbient (L82010) + getEffectiveDeclarationFlags
  (L82013: combined modifiers + ambient-context Export/Ambient
  synthesis — non-class/interface members in ambient ExportContext
  containers gain Export|Ambient; global-augment exception) —
  shared §5 kit, transcribe with the overload band.

## §12 Landing-time transcription CHECKLIST

Everything else in this doc is implementation-grade; the items below
transcribe their FULL bodies from the cited anchors when their first
caller lands (a worker shared by multiple bands lands with the FIRST
band and is re-audited by later ones). Check each off in the landing
commit's body.

Grammar workers (suppression discipline per existing grammarError*
plumbing; checkGrammarModifiers itself STAYS the M7-stub hook — 5.8
lands only the workers named here):
- [x] 5.8a: checkGrammarComputedPropertyName; checkAwaitGrammar
      (L79338 — await-using lists §2); checkGrammarTypeOperatorNode
      (§11); checkGrammarMappedType is DONE (L81941, §11).
      — landed @37b8b545 (§11 arms @b81ba333).
- [x] 5.8b: checkGrammarFunctionLikeDeclaration; checkGrammarIndex
      Signature; checkGrammarForGenerator; checkGrammarMethod;
      checkGrammarProperty; checkGrammarConstructorTypeParameters +
      checkGrammarConstructorTypeAnnotation. (checkGrammarAccessor
      L89843 already read §5-adjacent.)
      — landed @212c90cf (iteration band @3cb92a97).
- [x] 5.8c: checkGrammarClassLikeDeclaration; checkGrammarInterface
      Declaration.
      — landed @7c2a15e6 (checked off in that commit's body).
- [x] 5.8d: checkGrammarImportClause; checkGrammarNamedImportsOr
      Exports. (checkGrammarModuleElementContext L86347 +
      checkGrammarExportDeclaration L86340 already extracted §8-9.)
      — landed @f3b1c621.

Checker workers / helpers:
- [x] 5.8a: getErrorSpanForNode's VariableDeclaration arm (the 2322
      span pin, §2.12); checkExternalEmitHelpers body (verify the
      importHelpers-gated no-op claim); errorSkippedOn → the
      skippedOn filter seam beside filter_by_comment_directives;
      isReferenced bit decision (risk §14.16); getModuleInstance
      State binder exposure (§2 collisions); templateConstraint
      Type / anyReadonlyArrayType / stringNumberSymbolType
      singletons verify.
      — landed @28fa28d4 + @37b8b545; the emit-helpers no-op
      verdict is recorded in-source (statements.rs top comment +
      modules.rs "emit-marking no-op").
- [x] 5.8b: createGeneratorType tail (L78842+: IterableIterator
      fallback when global Generator is missing);
      checkAndAggregateYieldOperandTypes (L78874) +
      checkAndAggregateReturnExpressionTypes (L78959) generator
      gates; isImplementationCompatibleWithOverload (grep anchor);
      getIndexTypeOrString/getExtractStringType verify (for-in).
      — landed @3cb92a97 (§4 iteration) + @212c90cf (§5 overload);
      the Extract kit @dd7bfffc.
- [x] 5.8c: calls.rs dual-mode audit — hasCorrectArity's decorator
      arm reads the real experimental_decorators (not a constant);
      getThisArgumentOfCall gains the `!legacy_decorators()`
      Decorator arm (L76281); ES decorator context builders
      (L78468-78570);
      legacy helpers getParentTypeOfClassElement (L87798) /
      getClassElementPropertyKeyType (L87802) /
      createTypedPropertyDescriptorType (L61029);
      getTypeWithoutSignatures; isMixinConstructorType +
      getConstructorsForTypeArguments verify; isPropertyIdenticalTo
      (compareProperties) exposure.
      — landed @7c2a15e6 (checked off item-by-item in that
      commit's body).
- [x] 5.8d: getSymbolOfPartOfRightHandSideOfImportEquals (L49230);
      tryFindAmbientModule (L59499) + the patternAmbientModules
      initialization walk (the distinct ambientModulesCache memo was
      NOT ported as a named cache — the walk is direct; cache shape
      is performance-only, revisit only if B4 flags it);
      getCannotResolveModuleNameErrorForSpecificModule (L69377);
      mergeModuleAugmentation (L47830) + mergeSymbolTable (L47818)
      exact merge order (merge.rs); getResolutionDiagnostic (mostly
      option-dead); getSuggestedSymbolForNonexistentModule (spell.rs
      twin verify); _computedOptions.moduleResolution.computeValue +
      getIsolatedModules/shouldPreserveConstEnums derivations.
      — landed @79936faf (§9 core) + @f3b1c621 (§8/§9 drivers).
- [ ] 5.8d residue: markExportAsReferenced (L71945 — decide
      emit-only no-op). The decision was never recorded: no port, no
      escape, no commit-body verdict (2026-07-18 audit). It sits in
      m8-emitter-inventory.json (direct_emitter=false, sites=[]), so
      the D2 emitter-dependency closure forces an explicit
      disposition before M7 close; decide it there (likely alongside
      the M7 8.3 unused band, which owns alias-referenced marking).

## §13 Rust seam inventory + new state

Checker crate growth (file plan mirrors band structure):
- statements.rs NEW (§§2-3, est. 1.5-2k): variable band + control
  statements + truthiness kit + collisions band + grammar workers.
- iterate.rs NEW (§4 + §4-addendum, est. 1-1.3k): IterationTypes
  arena/singletons + resolvers + worker family + generator return
  types + createGeneratorType + the [ITER] escape lifts touch
  operators/literals/functions/contextual/widen/expr/calls.
- functions.rs GROWS (§5): checkParameter/SignatureDeclaration/
  FunctionOrMethodDeclaration/overload band/checkAsyncFunctionReturn
  Type/checkAllCodePaths/accessor pair checks; evaluate.rs's
  declared-before-use walk untouched.
- class.rs NEW (§6, est. 1-1.4k): checkClassLikeDeclaration +
  overrides + member-kind overrides + property initialization +
  index constraints + issueMemberSpecificError.
- modules.rs NEW (§§8-9, est. 1.5-2k): module/augmentation band +
  import/export checks + alias protocol + per-kind targets + module
  resolution + exports worker; merge.rs completes mergeModule
  Augmentation rows; annotate.rs 3173/access.rs 1603 escapes lift.
- calls.rs GROWS (§10): resolveDecorator + decorator effective args
  + getDecoratorCallSignature (+ its global-type builders).
- check.rs: stub arms → dispatch to the above; heritage/import-type
  generalization of checkTypeReferenceNode/checkTypeArgument
  Constraints/checkTypeReferenceOrImport; chain-closure param on
  check_type_assignable_to (reuse calls.rs's chain support);
  checkSourceFileWorker tail (drains + checkExternalModuleExports +
  checkPotentialUncheckedRenamed).

New state:
- CheckerState: potentialThisCollisions/NewTarget/WeakMapSet/
  Reflect/UnusedRenamedBindingElements vectors;
  ambientModulesCache + patternAmbientModules +
  patternAmbientModuleAugmentations (init from bound files);
  templateConstraintType/anyReadonlyArrayType/stringNumberSymbolType
  singletons (verify existing); iterationTypesCache intern map.
- TypeLinks: iteration_types_of_{iterable,async_iterable,iterator,
  async_iterator,iterator_result} (5 verdict slots storing the
  noIterationTypes sentinel distinguishably).
- NodeLinks: decorator_signature (anySignature-sentinel memo);
  enum_member_value EXISTS; has_reported_statement_in_ambient_context
  EXISTS.
- SymbolLinks: alias_target (LinkSlot w/ Resolving), type_only_
  declaration (tri-state) + type_only_export_star_name,
  type_only_export_star_map, resolved_exports (module flavor beside
  the late-bind flavor — SAME slot in tsc; keep one slot routed by
  getExportsOfSymbol), exports_checked, type_parameters_checked,
  cjs_export_merged.
- NodeCheckFlags: LexicalModuleMergesWithClass +
  ContainsClassWithPrivateIdentifiers (verify bits exist).
- PROGRAM layer: minimal module resolver (relative specifiers vs
  in-program file names + extension probes; harness @filename
  layout is the spec); skippedOn("noEmit") diagnostic filter;
  commentDirectives collection (parser) + directive filter — the
  §1 LIFT slice's dependency.
- BINDER verifications (BLOCKING their bands): NodeFlags.Namespace
  stamp on `namespace` keyword (§8 row); HasImplicitReturn/
  HasExplicitReturn flow-exit flags (§5 checkAllCodePaths + getter
  2378); localSymbol on exported declarations (§5 overload band);
  getModuleInstanceState exposure (§2 collisions + §8).
- **OPTIONS AUDIT (verified against options.rs + the conformance
  mapping, 2026-07-14). Count unit: fixture FILES under
  `ts-tests/tests/cases/conformance` — the only tree the conformance
  runner walks (select_fixtures) — via `/usr/bin/grep -rli
  '@<option>' ts-tests/tests/cases/conformance | wc -l` (plain BSD
  grep: the workspace's interactive grep aliases to ugrep, and
  all-suite scopes inflate the numbers; option-matrix expansion
  multiplies files into more programs/cases). Counts are DIRECTIVE
  PRESENCE; some fixtures set the option false — true-valued subsets
  run lower (experimentalDecorators=true: 119 of 134; noEmit=true:
  469 of 727). Re-measure per slice with the same command**:
  - PRESENT + mapped (read directly): target — **ES5 IS mapped and
    807 conformance fixtures use it ⇒ every languageVersion<ES2015 arm in this
    doc is MANDATORY 5.8 scope** (collision bands §2 at their
    ranges, the ES5 async-return band §5, the downlevel iteration
    reach §4, emitStandardClassFields=false super-call band §5,
    setNodeLinksForPrivateIdentifierScope gates §5, class
    emit-helper gates as no-ops); module; jsx (+factory strings);
    lib; allowJs/checkJs; **experimentalDecorators (§10 dual
    mode; 134 fixtures)**; the strict family (strict/alwaysStrict/
    strictNullChecks/strictFunctionTypes/strictBindCallApply/
    noImplicitAny/noImplicitThis/strictPropertyInitialization/
    useUnknownInCatchVariables via strict_option_value);
    exactOptionalPropertyTypes; noFallthroughCasesInSwitch (present
    — the ARM still waits on M5 flow); allowUnreachableCode;
    noUncheckedIndexedAccess; noPropertyAccessFromIndexSignature;
    useDefineForClassFields (+ emit_standard_class_fields).
  - **ADD in 5.8** (CompilerOptions field + conformance mapping,
    owner slice in §15): no_emit (5.8a — the skippedOn filter's
    input; 727 conformance fixtures set @noEmit);
    downlevel_iteration (5.8b; 21 fixtures); strict_builtin_iterator_return (5.8b;
    strict-family per bundle L46472 — default ON); module_resolution
    (5.8d; 85 fixtures / 43 classic; 2792-vs-2307 + Classic
    behaviors + computed default); es_module_interop +
    allow_synthetic_default_imports (5.8d; 19 / 0-standalone
    conformance fixtures; §9 gates + resolveESModuleSymbol interop
    faces);
    preserve_const_enums (5.8d; 2 fixtures;
    shouldPreserveConstEnums = preserveConstEnums || isolatedModules
    feeds isInstantiatedModule §8).
  - ABSENT → arms stay DEAD w/ ledger notes: noImplicitReturns,
    noImplicitOverride, allowUnusedLabels, isolatedModules,
    verbatimModuleSyntax, erasableSyntaxOnly, ignoreDeprecations
    (absent ⇒ the assert-deprecation row IS live — it fires unless
    the option equals "6.0"), resolveJsonModule, importHelpers,
    emitDecoratorMetadata, allowImportingTsExtensions,
    rewriteRelativeImportExtensions, noUncheckedSideEffectImports,
    moduleDetection.

## §14 FP=0 risk register (pin at each slice)

1. **The late-binding lift (§1)** is its own slice with a re-run +
   full-conformance triage protocol; FLOW-narrowing FPs get targeted
   [FLOW M5] report gates, never member-table re-containment; the
   well-known-symbol-only carve-out is the fallback. NO other slice
   may un-contain interfaces as a side effect.
2. **skippedOn(noEmit)**: the collision band lands ONLY with (a)
   no_emit added to CompilerOptions + the mapping and (b) the
   skippedOn filter at the diagnostics-finalize seam (beside the
   existing filter_by_comment_directives in checker/src/lib.rs);
   727 conformance fixtures set @noEmit — an unfiltered collision row FPs
   corpus-wide.
3. **errorOutputContainer double-add** (§4 union path): keep
   collect-only containers + one explicit add per diagnostic; pin a
   union-iterable failure.
4. **Display-band containment**: 2403/2717/2415/2416/2320-family
   render whole types in args — unrenderable displays ESCAPE (the
   established T2 curtain), never partial-render.
5. **Flow-gated faces split**: 2564 + 2612 report ONLY the
   no-constructor/flow-free faces; constructor-present faces escape
   [FLOW M5]. Pin both shapes each.
6. **NodeFlags.Namespace**: verify the parser stamps it before
   wiring the module-keyword row — else FP on every `namespace`.
7. **2307 un-silencing order**: resolver must consult in-program
   files + ambient + pattern-ambient modules BEFORE the error tail;
   flip the import-call silent stub only after declaration-side
   conformance shows no fabricated 2307/2306.
8. **Body double-drive**: eager function/constructor body checks +
   the 5.5f deferred method path must stay idempotent through links
   + exact-dedupe; pin an object-literal method with an internal
   error.
9. **Demand-caveat inversion**: checkVariableLikeDeclaration forces
   getTypeOfSymbol on EVERY declaration — 5.6's demand-gated
   implicit-any rows now fire at declaration sites; re-audit pins
   that assumed absence (the "const-position rows remain 5.8" FN
   class flips to live).
10. **Truthiness/always-defined bands** (§3): symbol-identity walks
    (isSymbolUsedIn*) compare RESOLVED symbols; text comparison FPs
    under shadowing. 2774/2801 gate on strictNullChecks.
11. **Grammar suppression**: every new grammarError* row stays
    behind hasParseDiagnostics; plain error() rows (e.g.
    _0_declarations_can_only_be_declared_inside_a_block) do NOT.
12. **Heritage/import-type self-forcing** rides the TypeReference
    overwrite-tolerant cache arm (§0 re-entrancy trap) — audit
    checkTupleType/checkUnionOrIntersectionType/checkTemplateLiteral
    self-forces for default-subtree reachability.
13. **checkExternalModuleExports** (2309/2323) lands AFTER the §9
    exports worker inside the module slice — its getExportsOfModule
    dependency is real; the once-guard must dedupe the
    checkExportAssignment-driven second run.
14. **Decorators DUAL MODE**: experimental_decorators IS modeled —
    every mode read through one accessor; applying ES semantics to
    a legacy fixture (or vice versa) is a wrong-payload FP. If the
    legacy table misbehaves at landing, contain the WHOLE band
    under experimental_decorators=true (FN), never mix. ES context
    types are lib-loaded (noLib pins hand-declare or tolerate 2318).
15. **checkTypePredicate tail** is provably dead (predicates
    unmodeled until M5) — port the 1228 row + M5-stub escape; do
    not fabricate 2677/1230 shapes.
16. **isReferenced gate** (§2 renamed-binding drain): the 2842-family
    row fires only when the binding symbol is NOT referenced —
    tsc's symbol.isReferenced bit. Port the bit (SymbolLinks write
    on identifier resolution) or prove signature-only parameter
    bindings unreferenceable; reporting without the gate FPs on
    referenced renames.

## §15 Slicing + sequencing (commits `m4 5.8a-e`)

- **5.8a — statements + variables + type-nodes**: §0 driver arms;
  no_emit option + the skippedOn(noEmit) filter (risk #2); §2 whole
  (collisions band incl. the ES5-live faces); §3
  minus for-of/for-in ITERATION semantics (the statements land with
  checkRightHandSideOfForOf escaping to 5.8b; grammar + LHS rows
  live); §11 type-node arms (checkTypeLiteral's lazy block pulls
  checkIndexConstraints + duplicate-check helpers forward — they
  are self-contained); checkPotentialUncheckedRenamed drain.
  Expected recovery: 17004/7050 const-position rows, 1155/1156-
  family, 2403/2717, truthiness bands, switch 2678, statement-body
  reach for ALL existing expression machinery (the biggest rate
  multiplier of the stage).
- **5.8b — iteration + function/member declarations**:
  downlevel_iteration + strict_builtin_iterator_return options; §4
  protocol + §4-addendum generator inference + for-of/for-in
  completion +
  [ITER] escape-lift sweep (operators/literals/functions/contextual/
  widen/expr/calls) + §5 whole (binder HasImplicitReturn verify is
  this slice's precondition; checkTypePredicate M5-stub tail).
  Recovery: 2461/2488/2549-family, 2345-in-bodies, 7010/7011 at
  overloads, 2391-2394 overload band, 2378/2676/2808 accessors,
  1064 async returns, 2355/2534/2847.
- **5.8c — class band + interface/enum completions + decorators**:
  §6 whole + §7 + §8 enum drivers + §10 decorators DUAL MODE (both
  signature tables; calls.rs Decorator arm + deferred arm; the
  experimental_decorators=true containment fallback per risk #14). Recovery: 2415/2417/2420/2720,
  4112-4116, 2610/2611/2423-2425, abstract 2515/2653-family,
  2564-no-ctor face, 2300/2374/2699, 1206x decorator band.
- **5.8d — module/alias/import-export band**: module_resolution +
  es_module_interop + allow_synthetic_default_imports +
  preserve_const_enums options; §8 module band + §9 whole (alias protocol → per-kind targets → module resolution →
  exports worker → checkExternalModuleExports + checkExportsOnMerged
  Declarations' alias arm un-escapes) + the VALUE_MODULE
  getTypeOfSymbol arm (annotate.rs 4907 — namespace value types,
  unblocks 2683/2631/2632 + globalThis) + merge.rs augmentation
  completion. Recovery: 2305/2307/2306/2613/2614/2724/2459/2460,
  2440/2484, 2308, 1192-family, namespace bands, import-equals rows.
- **5.8e — the LIFT slice**: comment-directive completion —
  **EXTEND/REPLACE the existing interim filter**
  (checker/src/lib.rs filter_by_comment_directives: single-line
  comment-only lines today) with scanner-backed commentDirectives
  collection (multi-line-comment directives + exact regex parity;
  do NOT build a parallel second filter) → interface late-binding
  lift → full-conformance FP triage → targeted [FLOW M5] gates (or
  the well-known-symbol carve-out fallback per §1). Recovery: array/string member access
  corpus-wide + for-of over arrays + the §4 slow path.

Per-slice gates unchanged: cargo test workspace; relpin /0; ledger;
invariants idempotence; conformance FULL — T0 in commit body,
**FP=0 absolute**; integer ratchet non-regression + bump; escapes
--stale $(cat STAGE) clean incl. untagged/recovery ceilings; STAGE
bumps per letter; oracle pins land WITH each commit. Letterless
"5.7" tags EXPIRE at 5.8a — grep and re-own in the first slice.
"M4-end sweep 5.8"-tagged residuals (tables.rs tuple collapse,
constraints.rs generic tuples, structural keyof/indexed relation
arms, jsx corners, createTypeReference-over-non-generic-interface)
are NOT 5.8a-e scope: they form the M4 final-gate sweep after 5.8e
(re-tag then per skeleton-steps final gate + NOTES-m4.md).

Cross-references: [[tsrs2-m4-checker]] memory (session state);
skeleton-steps §5.8 (scope + re-entrancy trap + final gate);
m4-55 §0 (stub tags) / §11-12 (discipline); m4-57 (links/chain/
EffectiveArg patterns, M6-stub rule — decorators + §9 reuse);
m5-flow-steps.md (the [FLOW M5] faces recorded here: 2564/2612
constructor faces, checkTestingKnownTruthy narrowing adjacency,
isPropertyInitializedInConstructor/StaticBlocks swap surface).
