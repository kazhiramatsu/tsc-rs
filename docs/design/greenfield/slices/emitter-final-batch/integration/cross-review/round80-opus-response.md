All 32 System rows, the six `using` rows and the context-recovery rows reduce to seven code causes. At least four of them change the emitted JS, not just the maps. I reconstructed native line text from the actual maps' column arithmetic, so those JS claims are inferred, not proven; the captures section lists what would confirm them. Everything was read-only: code reading plus small Node probes against the vendored `_tsc.js`. The probe's TS map equals the recorded expected map on every row I ran.

## A. Metadata constructor-union / accessor-union (24 rows): maps only

**What differs.** The class is mapped at `class` (col 15) instead of `@dec` (col 10). The decorated accessor is mapped at `get` instead of `@dec`. This hits the ES5 IIFE `}())` segment, the `let C = class C` segments, the accessor member itself, ES5 `Object.defineProperty`, and the member `__decorate` statement.

**Upstream cause.** With `emitDecoratorMetadata`, the ts transform splices *synthesized* `__metadata` decorators into the modifier list:
- **Class:** `injectClassTypeMetadata` (`_tsc.js:94599`) builds `[leading export/default, decorators, METADATA, remaining non-decorator modifiers]`.
- **Class element:** `injectClassElementTypeMetadata` (`:94611`) builds `[decorators, METADATA, non-decorator modifiers]`.

`moveRangePastModifiers` (`:17311`) then sees a last modifier with a synthesized position. It falls back to `moveRangePastDecorators`, whose last decorator is also synthesized, so it returns the whole node.

It isn't union-specific. The existing property-decorator controls pass because properties use `name.pos`, and their class has no constructor, so no class metadata. Upstream probes confirm the rule: `@dec class C { constructor(p: Missing) {} }` also maps to col 10, while the property variant maps to col 15.

**Native cause.** `legacy_decorators.rs` `move_range_past_modifiers` (line ~4360) walks only the real modifiers. It is used at four sites that upstream feeds the same transformed node:
- line 819: class declaration location;
- line 1498: class decoration statement;
- line 1592: member decoration statement;
- line 4344: `finish_class_element` source-map range.

**Minimal patch.**
- Give `move_range_past_modifiers` a typed marker, `InjectedMetadata::{None, Class, Element}`, and compute the last element of the composed list:
  - **Element:** the last non-decorator modifier that survives if there is one, otherwise `declaration.pos`.
  - **Class:** the last non-decorator modifier after skipping only the *leading* run of export/default modifiers if there is one, otherwise `declaration.pos`.
- Decide "metadata present" with pure predicates, and never by calling `member_metadata`, because that allocates `_a`/`_b` and would change temp order:
  - **Class:** emitDecoratorMetadata, AND (the class is decorated OR a constructor parameter is decorated), AND a constructor with a body exists. An empty `constructor() {}` still counts.
  - **Accessor:** emitDecoratorMetadata, AND this accessor is the decorated owner under the existing `metadata_decorator_owner` / accessor-owner rules.
  - **Methods and properties:** no marker needed; they keep `name.pos`.
- Pass the same marker at all four sites.

**Neighbouring witnesses (mint expected output from TS).**
- **Class forms:**
  - `constructor() {}` with no parameters;
  - no constructor (stays past `@dec`);
  - `@dec export class`, where the range moves past `export`;
  - `export @dec class` and `export default @dec class`, which take the node range;
  - `@dec export default class`, which moves past `default`;
  - `@dec abstract class`, where `abstract` is elided, so the node range;
  - a class with only constructor-parameter decorators.
- **Accessor forms:**
  - `@dec static get`, which moves past `static`;
  - `@dec public get`, where `public` is elided, so the node range;
  - decorator on the second accessor only;
  - a decorated setter only.
- **Controls:**
  - `emitDecoratorMetadata: false`;
  - method and property members, which must stay on `name.pos`.
- **Matrix:** ES5, ES2015 and ES2022 × CommonJS and System × both comment settings.

The earlier qualified-name `clone_node_with_source_spelling` fix is unrelated. None of the differing segments fall inside the metadata expressions.

## B. `using` + exported destructuring (2 existing + 6 new rows): JS differs

**U1: missing `exports.` (7 rows).** Native line 59 of `using-object-pattern` is `(x = source.x, y = source.y);`; TS writes `(exports.x = source.x, exports.y = source.y);`. The columns fit exactly with 2×8 characters missing. The ES5 `object`/`nested-rest` rows and the ES2015 `nested-rest` `rest` target show the same pattern.

- **Upstream:** `convertToObjectAssignmentElement` and `convertToArrayAssignmentElement` (`:20716-20746`) reuse the binding's own `element.name` node as the assignment target. CommonJS `substituteExpressionIdentifier` rewrites it because substitution is driven by the emit hint, not by the parent node. The names are not in `exportedBindings` (upstream records only local names there), so upstream takes neither the flattening path nor `substituteBinaryExpression`.
- **Native:** `flatten_destructuring.rs` reuses the same node, which is faithful. But `builtins.rs:4781` `is_non_reference_identifier_node` classifies by the *parse* parent, sees a `BindingElement` name, and skips `substitute_identifier`.
  - The ES2015-target shorthand rows pass only because `visit_shorthand_property_assignment` substitutes separately.
  - The ES5 and ES2018 flatteners emit plain `x = …` / `rest = …` and lose the substitution.
- **Fix:** in the converters' identifier target positions (shorthand name, rest/spread name, property-assignment target, array element), return an expression-role reference instead of the raw binding name. That is a clone with `original = name`, the same text range, and `InternalEmitFlags::DECLARATION_NAME_REFERENCE`. This is exactly es_next's existing `VariableInitializerClone` projection (`project_parsed_assignment_target`), so move that helper into the flattener module.
  - **Scope:** `convert_to_assignment_pattern` has a single caller (es_next `hoist_variable_statement`), so the change stays inside the top-level `using` hoisting.
  - **System:** it never reads that flag, but must be re-witnessed because the node identity changes.

**U2: ES2018 object-rest location (the nested-rest rows).**
- **Symptom:** ES2015 `70:5` should map to `source` (1:59); native maps it to the pattern (1:41).
- **Upstream:** `flattenDestructuringAssignment` switches `location = value` when `nodeIsSynthesized(node)`. The hoisted `using` assignment is synthesized.
- **Native:** the shared flattener has this arm (`flatten_destructuring.rs:222`). The separate `es2018.rs:3258` `flatten_destructuring_assignment` always passes `Some(original)`.
- **Fix:** port that one arm, applied only when neither the fresh-value nor the required-value branch is taken.

**Witnesses:**
- the ES2015 `using` object rows that pass today (shorthand route unchanged);
- System × all `using-hoisted` shapes;
- ESNext module (no module transform);
- non-`using` `({a, ...r} = source)`, which is a parse node, so the location stays the node;
- `export let {x: y}` (property-assignment target) and `export let [x, ...r]` (array targets) at ES5 and ES2015 × CommonJS.

## C. Context-recovery rows

**C1: standard decorators on optional chains: JS differs, and valid programs are affected too.**
- **Symptom:** from the columns, native emits `[((_a = )?.x).bind(_a)]`; TS emits `[(_a = ).x.bind(_a)]`.
- **Upstream:** `createCallBinding` builds a plain `createPropertyAccessExpression` / `createElementAccessExpression`. Valid programs show the same thing: `@(x?.y) class C {}` gives `((_a = x).y.bind(_a))`, and `@(x?.[k])` gives `((_a = x)[k].bind(_a))`.
- **Native:** `standard_decorators.rs:2945` / `:2979`, the cached branch, rebuilds from the chain's data and keeps `question_dot_token`.
- **Fix:** in the cached branch only, clear the question-dot token and do not carry the optional-chain node flag. The uncached branch keeps the chain, which matches upstream `target = callee`.
- **Witnesses:**
  - `@(x?.y)` and `@(x?.[k])`;
  - `@(x?.y.z)`, where the receiver keeps its inner chain: `((_a = x?.y).z.bind(_a))`;
  - `@(this?.x)`, uncached, so `(this?.x).bind(this)`;
  - the same shapes as member decorators;
  - ES5, ES2015 and ES2022 × CommonJS and ESNext.

**C2: assertion-trivia prints the comment twice: JS differs.**
- **Symptom:** the native line is 7 UTF-16 units longer (the emoji counts as two units). The map implies `await /*😀*/  /*😀*/, string > (1);`; TS emits the comment once.
- **Upstream:**
  - The `PartiallyEmittedExpression` [16,24] sets `containerEnd = 24`. So `emitPartiallyEmittedExpression`'s `emitTrailingCommentsOfPosition(expression.pos = 24)` is skipped by `forEachTrailingCommentToEmit`, because `end === containerEnd`.
  - The comment is printed once, by the `AwaitExpression`'s trailing comments at 24.
  - The operator's leading-comment scan at 24 collects nothing, because same-line comments aren't collected at pos > 0.
- **Native:** `printer.rs:16446` `emit_partially_emitted_boundary_comments` emits the before-child comments with no container check. The after-child arm likewise lacks the `pos != containerPos` check.
- **Fix:** gate both arms with the active comment scope's end/pos, exactly as `forEachTrailingCommentToEmit` / `forEachLeadingCommentToEmit` do, and confirm the PEE's own scope is the active one there.
- **Non-recovery witnesses:**
  - `a = <any>/*c*/y;` (the before arm must still emit);
  - `a = y /*c*/ as any;`;
  - a line break before `as`;
  - `y /*c*/!`;
  - `satisfies`.

**C3: heritage-trivia (1 row): capture before patching.**
- **Expected:** `22→45` (end of `ExpressionWithTypeArguments`), `23→46` (its trailing `/*😀*/`, emitted because 45 ≠ the heritage clause's `containerEnd` of 53), then `29→53`.
- **Native:** only `22→53`, so the `ExpressionWithTypeArguments`' own end mapping and trailing comment are missing or moved.
- **Probable cause:** `printer.rs:5298` emits the expression with the `ExpressionWithTypeArguments` as its source extent. That folds the wrapper's own end mapping and trailing comment into the child, whose trailing comment is correctly suppressed because its end equals the wrapper's end (45).
- **Why this row only:** the shape only exists when the heritage clause ends after its types, which happens in recovery. The `removeComments: true` twin passes because both mappings land on the same generated column.

## Captures needed after the full r77 run

Native JS text is enough for these, not maps:
1. **assertion-trivia:** confirm the duplicated comment.
2. **heritage-trivia:** is the comment dropped, or printed after the heritage end mapping? That decides C3's patch.
3. **One optional-chain decorator row:** confirm `?.` is retained.
4. **Optional:** one ES5 `using-object-pattern` row, to confirm U1's missing `exports.` directly.

The metadata rows need no capture: the difference is maps only, and the rule is proven by the upstream probes.
