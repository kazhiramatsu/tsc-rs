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

Landed shape (2026-07-10): the walk contract lives in
`crates/oracle/symbol-dump.mjs` + `crates/xtask/src/symbol_audit.rs`
(mirrored, documented in both headers); line format
`pos \t end \t escapedName \t flags \t decls \t members \t exports`
with pos/end in UTF-16. `--sample N` stride-samples the corpus,
`--positions-only` compares just the pos/end columns so the WALK
mirror is gate-able before the binder exists — green at 300
fixtures/743 files. Files with parse errors on either side are
excluded (ast-diff convention); .js/.json program files are skipped
(the stage-3.4 TS-only carve-out). driver.mjs's program host moved to
the shared `program-host.mjs` for reuse.

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
  at every prior declaration, then a FRESH symbol takes the new
  declaration. SOURCE CORRECTION (2026-07-10, 42602 body): the fresh
  symbol is DETACHED — the table KEEPS the original symbol (only the
  isReplaceableByMethod branch replaces the entry), so every later
  duplicate re-conflicts against the ORIGINAL and reports again
  (oracle-pinned: triple `let y` yields 2451 ×4, two per conflict).
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

Landed shape (2026-07-10): declare.rs (Binder + TableRef +
declareSymbol/addDeclarationToSymbol/setValueDeclaration/
getDeclarationName/report family), node_util.rs (modifier flags,
getNameOfDeclaration, dynamic-name predicates, getErrorSpanForNode —
each with tsc-port ledger entries; JS-only arms carved out with
comments). Oracle-pinned unit tests: 2451/2300/2567/2528(+2752/2753
relateds) with exact UTF-16 spans. Notable oracle finds: default
function + default class MERGE silently (ClassExcludes excludes
Function via the JS constructor-function pattern) — the 2528 pin
needs class+class or export-assignment pairs; `var f`+`function f`
end-to-end diagnostic ORDER proves bindEachFunctionsFirst. Prereq
fixes landed here: Identifier/PrivateIdentifier escapedText now
factory-escaped in the parser (createIdentifier 21609 — table keys
were unescaped otherwise), and PrefixUnary/PostfixUnaryExpression
gained the `operator: SyntaxKind` payload via codegen seed (needed by
signed-numeric declaration names, strict-mode ++/--, flow mutation).

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

Landed shape (2026-07-10): containers.rs (ContainerFlags +
getContainerFlags table pins, bindContainer with flow-Start/IIFE/
returnTarget tails, declareSymbolAndAddToSymbolTable routing family,
bindModuleDeclaration + getModuleInstanceState family + 5061/2668
oracle pins, pattern_ambient_modules), flow.rs (FlowArena +
createFlowNode/labels/addAntecedent/finishFlowLabel — SOURCE FACT:
setFlowNodeReferenced is called by the CALLERS of createFlowNode, not
inside it; the impl-binder sketch was wrong), bind.rs (bind spine +
bindEachFunctionsFirst; bindWorker stub for 3.4, flow-aware
bindChildren arms for 3.5). Binder grew container/flow state, a
node_flags_mut view (tsc mutates node.flags; parse-time readers keep
the arena), and flowNode/endFlowNode/returnFlowNode side tables.

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

FP-gate findings (2026-07-10, from the first full-band conformance
with binder diagnostics): (1) TS 6.0.3 is STRICT BY DEFAULT —
`_computedOptions.alwaysStrict.computeValue` is `alwaysStrict !==
false` and does NOT consult `strict`; every plain script binds in
strict mode (oracle: `var private` alone yields 1212). (2) tsc
program machinery filters bind diagnostics: plain-JS files keep only
the plainJSErrors allowlist (none binder-emitted yet — bind diags
suppressed for JS files until 3.4c), and `// @ts-ignore` /
`@ts-expect-error` comment directives suppress following-line
diagnostics (ported as an interim comment-only-line filter in
check_program; unused-expect-error 2578 waits for M4 = tsc's
partialCheck path).

Landed shape 3.4a (2026-07-10): full bindWorker switch (TS arms) +
bind* family + strict-mode/contextual check family + bindSourceFile
entry; CompilerOptions moved to tsrs2-types (checker re-exports) and
grew target/alwaysStrict/strict (target default is ES2025
LatestStandard in TS 6.0.3, NOT ES5 — ES3 counts as unset);
conformance parses the target string map. getAssignmentDeclarationKind
dispatch + bindable-static predicates ported; JS symbol bodies and the
TS expando (function-parent Property assignment) body are 3.4c.
Parser fidelity fix surfaced: .d.ts files now parse with the Ambient
context flag on every node (parseSourceFileWorker) — ExportContext
binding inside ambient namespaces was wrong otherwise. Audit
normalizations (comparator-side, documented in xtask): oracle lines
with the Transient bit are checker-MERGED symbols (dropped in pairs);
`__#N@` private-name ids wildcard the program-global counter digits.
symbol-diff --sample 200: differing=0 (343 files compared).

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

Landed shape (2026-07-10): narrowing predicates + optional-chain/
assignment-target/logical predicate families in node_util.rs; flow
constructors (createFlowCondition/SwitchClause/Mutation/Call) as
Binder methods in flow.rs; the full bindChildren dispatch
(unreachable stamping via isPotentiallyExecutableNode/canHaveFlowNode,
statement flowNode stamps) + every statement/expression flow binder in
bind.rs; createBindBinaryExpressionFlow as an explicit
Enter/Left/Operator/Right/Exit work-stack machine. ActiveLabel is a
stack (tsc linked list); ConditionalExpression
flowNodeWhenTrue/WhenFalse, SwitchStatement.possiblyExhaustive and
clause fallthroughFlowNode live in side tables;
noFallthroughCasesInSwitch joined CompilerOptions. Flow-shape unit
pins (11): if-join antecedents, non-narrowing-condition collapse,
loop back edges, post-return unreachability, try/finally ReduceLabel,
narrowing-switch SwitchClause set incl. implicit default,
assignment mutation + reference stamps, logical/optional-chain
condition shapes, label reference/unreachable stamping, 50k-term
chain. RECURSION DEBT FIXED en route (all pre-existing M1 walkers,
exposed by the 50k chain): arena finalize_node, parser
walk_tree_for_import_meta and subtree_contains_possible_top_level_await
are now explicit-stack iterative. bind-corpus full: 11130 files,
148066 symbols, 118547 flow nodes, zero panics.

Commit: `m2 3.5: flow graph construction`.

## Final gate

```sh
cargo xtask bind-corpus             # binds every fixture; expect: zero panics
cargo xtask symbol-diff --sample 50 # expect: zero diffs on the audit format
cargo xtask ledger check
```

GATE MET (2026-07-10): bind-corpus 5908 fixtures / 7691 programs /
11130 files / 148066 symbols / 118547 flow nodes / ZERO panics;
symbol-diff --sample 50 differing=0 (and sample-200 differing=0);
ledger 132 entries 0 stale; invariants suite=all ok; conformance
syntactic band unchanged (T0 99.8219% FP 0) and all band T0 6.0260%
FP 0 (binder contributes ~685 matched semantic diagnostics — the
first non-syntactic matches; [t0] ratchet set to 0.0602).
CARVE-OUTS delivered to M3+: stage 3.4c (JS special-assignment symbol
bodies + TS expando + plainJSErrors allowlist + .js audit extension),
JSDoc binding (needs JSDoc parsing), unused @ts-expect-error 2578
(checker/M4), cross-file symbol merging + program-wide getSymbolId
counter parity (checker/M4; audit normalizes both).

## Expected failure modes

| Symptom | Diagnosis | Fix |
|---|---|---|
| Overload sets have one declaration each | merge masks hand-written or mis-extracted | Masks come from the generated SymbolFlags; re-run codegen verifier |
| Duplicate-identifier errors repeat per reference | conflict did not install a fresh symbol | Port the fresh-symbol-on-conflict branch exactly |
| Names with `__` prefix resolve wrongly | escaping applied on one side of lookup only | Escape at table-insert AND lookup; unescape only for display |
| Flow joins have 1 antecedent everywhere | finishFlowLabel pruning ported wrong | A label with a single antecedent is REPLACED by it; with none it is unreachable |
| for-of/destructuring binds miss flow assignments | expression-walk flow nodes skipped for pattern targets | bindWorker's assignment arms cover every pattern leaf |
