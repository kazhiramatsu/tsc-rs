No blockers in `r91-checker-candidate.diff`. All nine probes match TS 6.0.3's placement, gates and helper bits, and none can produce a duplicate diagnostic. One concrete divergence lies just outside the diff, in the pre-existing class `SetFunctionName` gate, with two counterexamples below. The nested-paren proposal is sound, but its skip bridge is broader than "nested parens" and needs specific controls. Your H1 schema correction is right: two edits are needed. Read-only: the diff, the current checker source, the `.github` schema, and `_tsc.js`. Nothing was compiled or run.

## Blockers

None in the diff.

## Findings on the diff

- **Helper bits:**
  - `ASYNC_DELEGATOR_INCLUDES = (1<<11)|(1<<13)|(1<<14)` equals upstream 26624.
  - `FOR_AWAIT_OF_INCLUDES = 1<<14` equals 16384 (`AsyncValues`).
  - Metadata = 16, Param = 32, Values = 256, MakeTemplateObject = 262144, ClassPrivateFieldIn = 2097152.
  - The dispatcher now names these instead of using numeric literals, with no protocol change.
- **for-of** (statements.rs ~2058): mirrors :83822-83833.
  - A missing container maps to `INVALID`, as upstream `getFunctionFlags(undefined)` does, so top-level `for await` requests nothing.
  - The static-block branch stays grammar-only.
  - The `else if` downlevel `Values` probe sits in upstream's position.
- **yield** (functions.rs:1726): placed after the `!func` and non-generator returns and after `isAsync`, as at :80456-80461. The eager grammar pass versus upstream's lazy one doesn't matter for output, since reported diagnostics are sorted.
- **Tagged template** (calls.rs ~6448): `< ES2015`, before signature resolution, as at :77855.
- **`in` expression** (operators.rs:1157): `< ESNext || !useDefine` is the exact reduction of :79586, since `< ES2022` implies `< ESNext`. It runs before the resolved-symbol logic.
- **Reference-assignment tail** (operators.rs:2505): ungated, on `parent(target)`, and only for a direct private-identifier property access (a parenthesized target doesn't match, as upstream).
  - At every target below ESNext, the Set probe at the target (access.rs:2025) has already requested the helper, so the tail's request is dropped by the requested-helper check. No duplicate.
  - At ESNext with `useDefineForClassFields`, only the tail reports.
- **Param** (calls.rs:187): the merged branch runs Decorate, then Param when `experimental_decorators && Parameter`. That is exactly legacy `Decorate → Param`. With standard decorators, parameters return earlier at `node_can_be_decorated`.
- **Metadata** (calls.rs:267): the gates match `markLinkedReferences(…, Decorator)` → `markDecoratorAliasReferenced`:
  - `!verbatimModuleSyntax`;
  - the ambient return with the PropertySignature/PropertyDeclaration exemption;
  - `emitDecoratorMetadata`;
  - a first decorator exists.
  - The helper is checked before the per-kind switch, and the arms are Class/Accessor/Method/Property/Parameter, as at :71873-71903.
  - The emit-time unspecified dispatcher (modules.rs:1344) reaches the same function, just as upstream's Unspecified arm reaches `markDecoratorAliasReferenced`. Any repeat request is dropped by the requested-helper check.

**Behaviour change in alias marking:** with `verbatimModuleSyntax` on, or for ambient non-property decorated nodes, aliases are no longer marked. Upstream does the same. It isn't observable, because `verbatimModuleSyntax` never elides imports, and ambient decorators are errors and emit nothing. A witness costs little: `verbatimModuleSyntax` + `emitDecoratorMetadata` + a decorated method taking an imported type; the import must be kept either way.

**Call-site accounting:** confirmed. 25 existing plus 9 new gives 34 native sites. The other 2 of upstream's 36 are merges:
- upstream has separate legacy `Decorate` and standard `ESDecorateAndRunInitializers` sites (both bit 8); native has one;
- upstream has three `SetFunctionName` sites in `checkDecorators` (:82761/:82765/:82770); native has two (calls.rs:214/:233).

## Medium: the `SetFunctionName` condition (pre-existing, outside the diff)

The call-site count matches, but the *gate* doesn't. Native `needs_set_function_name` (calls.rs ~198) approximates upstream `getFirstTransformableStaticClassElement` (:84921-84943) as "static and (static block, private name, or decorated)". It misses two branches:

1. **Non-static decorated members.** Upstream returns for *any* member decorated with standard decorators when the class is decorated below ESNext (`willTransformStaticElementsOfDecoratedClass`).
   - Counterexample: `@dec class C { @dec m() {} }` at ES2022, standard decorators, `importHelpers`, and a tslib without `__setFunctionName`. Upstream reports; native requires `static` and doesn't.
2. **Static initialized properties** when `!emitStandardClassFields` (`willTransformInitializers && isInitializedProperty`).
   - Counterexample: `@dec class C { static x = 1 }` at ES2021, or at ES2022 with `useDefineForClassFields: false`. Upstream reports; native doesn't.

The private-static and static-block arms are correct: upstream's `< ES2022 || < ESNext` is true below ESNext. Add both counterexamples to the fixture expansion. They are a condition divergence, not a missing call site.

## Nested-paren proposal

**Sound as specified.** Validated `CloseParen` skip ends map to (unique owner, first boundary), and bridging happens only when the next skip is a `CloseParen` whose `full_start` equals a stored end. Also require:
- `TokenSkipped.token == CloseParenToken` on both the stored and the bridging skip;
- actions sorted by `start`, refusing otherwise, rather than being order-independent;
- each stored end consumed by at most one bridge, so a run of any length works as a chain.

**It's broader than nested parens.** Bridging isn't tied to a missing-close chain, so `foo()\n))` (two stray `)` after an ordinary statement) becomes admitted too. That is the same statement-gap family and probably fine, but it's new admission. Control it: compare `foo()\n))` and `(x //c\n as number)) /*m*/ );` (a comment *between* skipped parens) against TS complete commands.

**Closer chain:** "exactly one direct `ParenthesizedExpression.expression` chain with one outer root, a same start/length/`full_start` parser event count equal to the chain length, `missing_node = None`, exactly one reported" is tight.
- The speculative event (start 87, `full_start` 89) is correctly excluded from the count by its `full_start`.
- The single-paren rule is unchanged.

**Negative controls:**
- mixed closers `(x //c\n as number)]`;
- a type-assertion break `(<any>(x //c\n as number))`: two closers end at 81 but aren't a direct chain, so it's refused;
- a comma break `(a, (x //c\n as number))`;
- unit mutations of the event list: drop the suppressed event, change its `full_start`, add a second reported event, reverse the order of the two skip actions, overlap two spans;
- positives: triple nesting, and nesting with a block comment.

## H1 schema

Confirmed: zero absence proofs needs both edits:
- `summary.absence_proofs`: `positive_integer` → `nonnegative_integer` (schema line 89);
- `evidence.absence_proofs`: `minItems: 1` → `0` (line 127).

The per-row `evidence.absence_proofs` ID lists (line 174) have no `minItems`.

Once these probes pass, retire the 20 implemented rows and keep the constructor-capture row with its accurate "no observable consumer in 6.0.3" wording. One row still satisfies `checker_emit_elisions` `minItems: 1` and the positive `explicit_checker_emit_elisions`.

Don't record the `SetFunctionName` gate divergence as an elision. Fix it, or keep it as an open, observable item.

These controls are finite witnesses. They support the listed shapes, not parity across every program.
