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
`__global`, `__missing`, `__type`, `__object`, `__computed`,
`default` — M0 codegen's `InternalSymbolName`), and
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
  exports → `default`; missing → `__missing`).

Verify: micro-fixtures for each merge class — overloads (merge),
namespace+function (merge), namespace+class (merge), enum+enum
(merge), interface+class (merge), var+var (merge), let+let
(conflict), var+function (conflict), default+default (special
message) — expected diagnostics oracle-probed; symbol-diff on each.

Commit: `m2 3.2: declareSymbol merge engine`.

## Stage 3.3: containers and scopes [M]

- `getContainerFlags` (near 42734): the IsContainer /
  IsBlockScopedContainer / IsControlFlowContainer / HasLocals /
  IsFunctionLike / IsFunctionExpression classification per node kind
  — this table DEFINES the scope tree; port every arm.
- `bindContainer`: save/restore of `container`,
  `blockScopeContainer`, `currentFlow`, labels and break/continue
  targets; fresh `locals` where HasLocals; fresh flow `Start` where
  IsControlFlowContainer.
- `declareSymbolAndAddToSymbolTable` routing: which table a
  declaration lands in given the container kind, and
  `declareModuleMember` (42675) — the locals/exports split with
  `exportSymbol` linking (syntax-and-binder §3.2).

Commit: `m2 3.3: container classification + scope tree`.

## Stage 3.4: bindWorker — per-kind symbol creation [M]

`bind` (syntax-and-binder §3.3 skeleton) + `bindWorker` (44287) arms
in tsc's switch order: identifiers/this (flow assignment of
`flowNode`), binary assignment forms, catch variables, variable
declarations + binding elements, functions/methods/constructors/
accessors, class declarations+expressions (class expression names
bind in their own wrapper scope), interfaces, type aliases, enums
(const-enum flag), modules (instance-state classification —
`getModuleInstanceState` — is bound here and consumed by M7's
suggestion rules), import/export forms, JSX names, computed property
names, private identifiers, parameters (this-params excluded from
locals), type parameters, index signatures.

Strict mode threads through everything (syntax-and-binder §3.4):
prologue detection, class-body always-strict, module always-strict;
the binder-emitted strict diagnostics (with-statement, delete of
identifier, octal, duplicate parameter names, eval/arguments binding)
port here with oracle-probed pins.

Commit(s): `m2 3.4a..c: bindWorker arms (+audit results)`.

## Stage 3.5: flow-graph construction [M]

Port the family per syntax-and-binder §3.3 and core-interfaces §5
(FlowFlags are generated, bit-compatible):

- Node constructors `createFlowNode` variants + `addAntecedent` +
  `finishFlowLabel`; the `Shared` flag stamping when a node gains a
  second antecedent.
- `bindCondition` (43193) with `doWithConditionalBranches` — logical
  `&&`/`||`/`??` and optional chains create their edges during
  sub-expression binding, NOT as extra condition nodes on top.
- The statement family: `bindWhileStatement` (43218) as the loop-label
  pattern reference, do/for/for-in/for-of, if, switch (SwitchClause
  nodes with clauseStart/clauseEnd; the implicit default edge),
  try/catch/finally (ReduceLabel wiring), return/throw
  (Unreachable), break/continue via the active label list,
  labeled statements.
- Assignment/call/array-mutation flow nodes from the expression walk;
  `node.flowNode` stamping for identifiers/this so the checker can
  start the backward walk (stored in the binder's NodeLinks-side
  table, core-interfaces §1).

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
