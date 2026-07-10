# M2: binder — steps

Parent design: syntax-and-binder.md §3 (declareSymbol, module members,
containers, flow construction, strict mode); core-interfaces.md §2
(Symbol contract), §5 (FlowNode contract). tsc source region:
`bindSourceFile` (42408) through the bind*Statement family.
Prerequisite: M1 gates green.

Gate: crash-free bind of the full corpus + a 50-fixture symbol
spot-audit vs the oracle.

## Stage 3.0: the symbol audit harness FIRST [P]

`oracle/symbol-dump.mjs`: for a program.json, print a deterministic
symbol summary the Rust side can reproduce —

```js
// per source file: for each statement-level declaration name,
// resolve via checker.getSymbolAtLocation and print:
//   escapedName \t ts.SymbolFlags-bits \t declarations.length \t
//   sorted(members keys) \t sorted(exports keys)
```

`cargo xtask symbol-diff <fixture>` compares. The audit is a SPOT
check (top-level + one nesting level), not full parity — full parity
is what the conformance metric measures indirectly from M4 on.

Commit: `m2 3.0: symbol audit harness`.

## Stage 3.1: Symbol model + name escaping [M]

Per core-interfaces §2: `Symbol` struct, ORDERED `SymbolTable`
(IndexMap — iteration order is observable), the internal symbol names
(`__call`, `__constructor`, `__new`, `__index`, `__export`,
`__global`, `__missing`, `__type`, `__object`, `__jsxAttributes`,
`__computed`, `__function`, `export=`, `default`, `this` — M0
codegen's `InternalSymbolName`), and
`escapeLeadingUnderscores` (11438): names beginning `__` gain a third
leading underscore so user `__proto__` cannot collide with internal
names. Port the escape AND the unescape used at display time.

Commit: `m2 3.1: symbol model + escaped names`.

## Stage 3.2: declareSymbol — the merge engine [M]

Port `declareSymbol` (42602) from the annotated skeleton in
syntax-and-binder §3.1, with its supporting pieces:

- `addDeclarationToSymbol`: pushes into `declarations`, ORs `includes`
  into flags, sets `valueDeclaration` FIRST-value-decl-wins.
- The includes/excludes masks come from the generated `SymbolFlags`
  (the `*Excludes` members are IN the extracted enum table). NEVER
  hand-enumerate which kinds merge — the masks are the rule.
- On conflict: the duplicate-identifier report family
  (Duplicate_identifier, block-scope redeclaration, enum-merge and
  multiple-default special messages) with relatedInformation pointing
  at every prior declaration, then a FRESH symbol replaces the table
  entry so the error reports once.
- `getDeclarationName` (computed names → `__computed`; default
  exports → `default`; missing → `__missing`; plus the tail cases:
  `export * from` declarations → `__export`, SourceFile and
  `module.exports =` binary assignments → `export=`, JSDoc
  function-type parameters → synthetic `argN`).

Verify: micro-fixtures for each merge class — overloads (merge),
namespace+function (merge), namespace+class (merge), enum+enum
(merge), interface+class (merge), var+var (merge), let+let
(conflict), var+function (conflict), default+default (special
message) — expected diagnostics oracle-probed; symbol-diff on each.

Commit: `m2 3.2: declareSymbol merge engine`.

## Stage 3.3: containers and scopes [M]

- `getContainerFlags` (45143): the per-kind classification — this
  table DEFINES the scope tree; port every arm. It has NINE flags,
  not six: IsContainer / IsBlockScopedContainer /
  IsControlFlowContainer / IsFunctionLike / IsFunctionExpression /
  HasLocals / IsInterface /
  IsObjectLiteralOrClassExpressionMethodOrAccessor /
  PropagatesThisKeyword — plus the per-kind conditionals
  (PropertyDeclaration is a control-flow container only WITH an
  initializer; Block only block-scopes when its parent is not
  function-like/static-block; method/accessor gains flag 128 only via
  isObjectLiteralOrClassExpressionMethodOrAccessor).
- `bindContainer` (42734): save/restore of `container`,
  `blockScopeContainer`, `thisParentContainer`, `currentFlow`,
  `currentReturnTarget`, `activeLabelList`, break/continue targets
  and `hasExplicitReturn`; fresh `locals` where HasLocals; fresh flow
  `Start` where IsControlFlowContainer. Port the tails exactly:
  `currentReturnTarget` is created only for IIFEs, constructors, and
  JS-file functions; on exit `hasExplicitReturn` stamps
  NodeFlags.HasExplicitReturn and the accumulated `emitFlags` stamp
  NodeFlags.HasAsyncFunctions onto the container node — the checker's
  implicit-return analysis (M5) consumes both.
- `declareSymbolAndAddToSymbolTable` routing: which table a
  declaration lands in given the container kind, and
  `declareModuleMember` (42675) — the locals/exports split with
  `exportSymbol` linking (syntax-and-binder §3.2).
- `bindModuleDeclaration`/`declareModuleSymbol` +
  `setExportContextFlag`/`hasExportDeclarations`: ambient-module
  shape checks land here (5061 Pattern_0_can_have_at_most_one_Asterisk,
  2668 export-modifier-on-ambient-module — the latter belongs in the
  first-2xxx pin set), and NodeFlags.ExportContext stamping.

Commit: `m2 3.3: container classification + scope tree`.

## Stage 3.4: bindWorker — per-kind symbol creation [M]

`bind` (syntax-and-binder §3.3 skeleton) + `bindWorker` (44287) arms
in tsc's switch order: identifiers/this/super/import.meta (flow
assignment of `flowNode` — MetaProperty and SuperKeyword get one
too), binary assignment forms, catch variables, variable
declarations + binding elements, functions/methods/constructors/
accessors, class declarations+expressions (class expression names
bind in their own wrapper scope), interfaces, type aliases, enums
(const-enum flag), modules (instance-state classification —
`getModuleInstanceState` (42278, with its memoized
`getModuleInstanceStateCached` companion) — is bound here and
consumed by M7's suggestion rules), import/export forms
(`bindExportAssignment` `export=`/default aliases,
`bindNamespaceExportDeclaration` with its Global_module_exports
1314/1315/1316 + Modifiers_cannot_appear_here checks), JSX names
(`__jsxAttributes` for JsxAttributes), computed property names,
private identifiers, parameters (this-params excluded from locals),
type parameters (including the `infer T` container routing via
`getInferTypeContainer` — infer parameters bind into the enclosing
conditional type's locals), index signatures.

ANONYMOUS symbols are their own family — port them together:
FunctionType/ConstructorType/JSDocFunctionType/JSDocSignature
(`bindFunctionOrConstructorType`: a `__call`/`__new` member inside a
fresh `__type` symbol), TypeLiteral/MappedType (`__type`),
ObjectLiteralExpression (`__object`), FunctionExpression/
ArrowFunction (`__function`), CallSignature/ConstructSignature
(`__call`/`__new`), and the SourceFile itself when it is an external
module (`bindSourceFileAsExternalModule`: a ValueModule symbol named
`"<fileName-without-extension>"` in quotes).

STATEMENT-LIST ORDER: SourceFile/Block/ModuleBlock bind via
`bindEachFunctionsFirst` (42830) — FunctionDeclarations bind BEFORE
the other statements. This is observable (valueDeclaration selection
and declaration order), not an optimization.

Strict mode threads through everything (syntax-and-binder §3.4):
prologue detection (`updateStrictModeStatementList`), class-body
always-strict, module always-strict, and the `alwaysStrict`/`strict`
COMPILER OPTIONS (`bindInStrictMode`) — CompilerOptions must grow
`always_strict`/`strict` and `target` here. The binder-emitted
diagnostics port with oracle-probed pins:

- strict family: with-statement 1101, delete-of-identifier 1102,
  eval/arguments binding/assignment/++/-- (1100/1103 family via
  `checkStrictModeEvalOrArguments`), octal handled at scan time,
  function-declaration-in-block ES5 family 1250/1251/1252 (TARGET
  DEPENDENT — needs `target`), reserved-word identifiers
  1212/1213/1214 with the class/module message variants,
  A_label_is_not_allowed_here 1344 (`checkStrictModeLabeledStatement`
  — strict AND target ≥ ES2015 AND labeling a declaration/variable
  statement). NOTE: duplicate parameter names are a CHECKER grammar
  error, not a binder one.
- non-strict contextual family (`checkContextualIdentifier`): `await`
  as identifier at the top level of a module (1262), await/yield used
  in their own context (reserved-word-here 1359-family), and
  `#constructor` (`checkPrivateIdentifier`). These and the contextual
  checks fire ONLY when `file.parseDiagnostics` is EMPTY — port the
  suppression gate, it is observable.

JS SPECIAL-ASSIGNMENT BINDING (the `getAssignmentDeclarationKind`
dispatch inside the BinaryExpression/CallExpression arms — ~25
functions, 44589–45142): CommonJS `exports.x =`/`module.exports =`
(`setCommonJsModuleIndicator`), `this.x =` property assignments,
prototype and `Object.defineProperty` forms, expando namespaces
(`bindPotentiallyMissingNamespaces`). The corpus includes allowJs
fixtures since M1, so the bind-corpus gate WILL execute these paths;
port at minimum the dispatch + crash-free no-op arms in 3.4, and the
symbol-producing bodies before the symbol audit is extended to .js
files (audit stays TS-only until then — record the carve-out in the
gate output).

JSDOC BINDING (`bindJSDoc`, `delayedBindJSDocTypedefTag`,
`bindJSDocImports`, the JSDoc tag arms): DEFERRED — the parser does
not produce JSDoc nodes yet. Leave explicit `todo_port!` arms so the
ledger tracks them; they activate when JSDoc parsing lands.

Bookkeeping the arms rely on: `seenThisKeyword` (ThisType),
`inAssignmentPattern` (destructuring targets), `file.symbolCount` and
`file.classifiableNames` (services-facing — populate, cheap).

Commit(s): `m2 3.4a..c: bindWorker arms (+audit results)`.

## Stage 3.5: flow-graph construction [M]

Port the family per syntax-and-binder §3.3 and core-interfaces §5
(FlowFlags are generated, bit-compatible):

- THE NARROWING PREDICATES FIRST (42977–43076):
  `isNarrowingExpression` / `isNarrowableReference` /
  `containsNarrowableReference` / `hasNarrowableArgument` /
  `isNarrowingTypeofOperands` / `isNarrowingBinaryExpression` /
  `isNarrowableOperand`. `createFlowCondition`/`createFlowMutation`/
  `createFlowCall` consult these to decide whether a flow node is
  created AT ALL (a non-narrowing condition just returns its
  antecedent) — without them every flow-dump pin has spurious nodes.
- Node constructors `createFlowNode` variants + `addAntecedent` +
  `finishFlowLabel`; the `Shared` flag stamping when a node gains a
  second antecedent.
- `bindChildren` (42843) is the dispatch spine: the per-kind routing
  between plain `bindEachChild` and the flow-aware binders, plus the
  `inAssignmentPattern` save/restore for destructuring targets.
- `bindCondition` (43193) with `doWithConditionalBranches` — logical
  `&&`/`||`/`??` and optional chains create their edges during
  sub-expression binding, NOT as extra condition nodes on top.
- The statement family: `bindWhileStatement` (43218) as the loop-label
  pattern reference, do/for/for-in/for-of, if, switch (SwitchClause
  nodes with clauseStart/clauseEnd; the implicit default edge),
  try/catch/finally (ReduceLabel wiring), return/throw
  (Unreachable), break/continue via the active label list,
  labeled statements, and `maybeBindExpressionFlowIfCall` for
  expression statements.
- The expression family: `createBindBinaryExpressionFlow` (43540) is
  a NON-RECURSIVE work-stack state machine (onEnter/onLeft/
  onOperator/onRight/onExit) — port it as such or deep binary chains
  in the corpus overflow the stack and fail the zero-panic gate;
  conditional (`?:`), delete, prefix/postfix, optional-chain
  (`bindOptionalChain*` — chain edges), non-null, access, and call
  flow binders (`bindCallExpressionFlow` creates the FlowCall used by
  assertion narrowing).
- Assignment/call/array-mutation flow nodes from the expression walk;
  `node.flowNode` stamping for identifiers/this/super/import.meta so
  the checker can start the backward walk (stored in the binder's
  NodeLinks-side table, core-interfaces §1).

Verify: a flow-dump unit format (node id, flags, antecedent ids) over
~15 hand-written control-flow micros; expected shapes derived by
reading the cited tsc — record each as a unit pin with the tsc
function named in the test comment.

Commit: `m2 3.5: flow graph construction`.

## Final gate

```sh
cargo xtask bind-corpus             # binds every fixture; expect: zero panics
cargo xtask symbol-diff --sample 50 # expect: zero diffs on the audit format
cargo xtask ledger check
```

## Expected failure modes

| Symptom | Diagnosis | Fix |
|---|---|---|
| Overload sets have one declaration each | merge masks hand-written or mis-extracted | Masks come from the generated SymbolFlags; re-run codegen verifier |
| Duplicate-identifier errors repeat per reference | conflict did not install a fresh symbol | Port the fresh-symbol-on-conflict branch exactly |
| Names with `__` prefix resolve wrongly | escaping applied on one side of lookup only | Escape at table-insert AND lookup; unescape only for display |
| Flow joins have 1 antecedent everywhere | finishFlowLabel pruning ported wrong | A label with a single antecedent is REPLACED by it; with none it is unreachable |
| for-of/destructuring binds miss flow assignments | expression-walk flow nodes skipped for pattern targets | bindWorker's assignment arms cover every pattern leaf |
