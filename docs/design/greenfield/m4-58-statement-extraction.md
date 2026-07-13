# M4 stage 5.8: statements + declarations — semantic extraction

Extracted 2026-07-14 from the vendored bundle
`tsrs2/vendor/typescript-6.0.3/lib/_tsc.js` (all `L`-anchors are lines
in THAT file; re-grep on re-vendor). Parents: skeleton-steps §5.8
(scope + the RE-ENTRANCY TRAP), m4-55 §0 (stub-policy tags), m4-57
(links/EffectiveArg discipline — carried forward). Implementers start
HERE. This doc's slicing (§15, commits `m4 5.8a-…`) supersedes the
steps doc's single-commit line.

Status: §§0-5 extracted (driver, late-binding wall, variables,
control statements, iteration protocol, member/function
declarations). REMAINING TO EXTRACT (§§6-11 placeholders at the
tail carry the anchors): class band L84921-85524, interface/
typealias/enum L84871+85525-85839, module L85840-86028, alias/
import/export L86029-86504 + resolveAlias L49116 family, decorators
L82580-82783 + getDecoratorCallSignature L78699, type-node arms
L81838-82023, checkPotentialUncheckedRenamedBindingElementsInTypes
L83180, grammar workers for the declaration bands (L88907-90450),
then §§12-15 (Rust seams, state, FP register, slicing).

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
  are §8's module band; comment directives are a small program-layer
  port (scanner already collects commentDirectives? verify — tsc
  filters diagnostics whose line matches a directive at
  getDiagnosticsWithPrecedingDirectives; if the parser doesn't
  collect them yet, that port is part of the lift slice).
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
     early return is semantic — the drain
     checkPotentialUncheckedRenamedBindingElementsInTypes (L83180)
     goes live per §0; read its body when porting: it reports
     renamed binding elements in signature-only positions).
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
  LIVE for low-target fixtures (TS6 minimum target: verify the
  options enum floor at landing; if ES2015 is the floor the
  `arguments` check below is dead).
- checkCollisionWithArgumentsInGeneratedCode (L83229): ES2015+ →
  skip entirely (dead at any modeled target if the floor is ES2015 —
  note, keep the hook).
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
strictBuiltinIteratorReturn ? undefinedType : anyType — the option is
strict-family in TS6 (verify options.rs mapping; annotate.rs's
BuiltinIteratorReturn intrinsic escape lifts with this).

### Entry points
- checkIteratedTypeOrElementType (L83894): any input → input;
  getIteratedTypeOrElementType(use, input, sent, errorNode,
  checkAssignability=true) || anyType.
- getIteratedTypeOrElementType (L83907) — transcribe whole:
  - never + errorNode → reportTypeNotIterableError → undefined.
  - uplevelIteration = target ≥ ES2015 && globalIterableType
    resolves (≠ emptyGenericType); downlevelIteration = !uplevel &&
    options.downlevelIteration (mapped option); possibleOutOfBounds =
    noUncheckedIndexedAccess && (use & 128) (option: verify
    options.rs; unmodeled → constant false w/ note).
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
kit). Target < ES2015 (gated on the modeled @target; TS6 target
floor to verify): the ES5 promise-constructor entity band
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

## §§6-11 EXTRACTION PENDING (anchors + owners recorded in Status)

Class band (checkClassDeclaration completion L84982 /
checkClassLikeDeclaration L84994 / overrides L85112-85315 /
checkKindsOfPropertyMemberOverrides L85315 / heritage +
checkBaseTypeAccessibility L85269 / checkInheritedPropertiesAre
Identical L85439 / checkPropertyInitialization L85477 /
checkIndexConstraints L84705 / checkClassExpression(Deferred)
L84972-84981 / checkTypeParameterListsIdentical L84871), interface
completion (L85525), type-alias completion (L85561), enum
(L85767-85839, evaluator ALREADY PORTED — evaluate.rs 5.3b),
module (L85840 checkModuleDeclaration + augmentation elements),
alias band (checkAliasSymbol L86029 + resolveAlias L49116 +
import/export declaration checks L86163-86501 +
checkExternalModuleExports L86505 — §0 lists the drain), decorators
(checkDecorators L82744 + checkDecorator L82628 +
getDecoratorCallSignature L78699 + grammar L82580), type-node arms
(L81838-82023: checkTypeQuery/TypeLiteral/Array/Tuple/UnionOr
Intersection/IndexedAccess(+IndexType worker L81893)/Mapped/This/
TypeOperator/Conditional/Infer/TemplateLiteral/Import/
NamedTupleMember), checkPotentialUncheckedRenamedBindingElements
InTypes (L83180), declaration-band grammar workers (L88907-90450:
checkGrammarModifiers full port decision stays M7 vs 5.8 — the
statement band only needs the workers named in §§2-3).
