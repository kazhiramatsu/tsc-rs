# Destructuring parameter implicit-any

This note is the design guardrail for the `TS7031` destructuring
parameter cluster. The goal is to stop reporting binding-pattern
implicit-any diagnostics by "all leaves or no leaves" and move toward
the same leaf-level implied-type model that tsc uses.

Related background:

- `checker-foundations.md` section 2-3 for check ordering and contextual
  typing.
- `knowledge-base.md` section 5 for strict batch defaults and probe line
  numbering.
- `tsc-source-guide.md` for reading `oracle/node_modules/typescript/lib/_tsc.js`.

## Problem

The current implementation in `src/checker/functions.rs` reports
destructuring parameter `TS7031` with this coarse rule:

1. If the parameter has a type annotation, a parameter initializer, or a
   cached contextual parameter type, report nothing.
2. Otherwise, collect every identifier leaf with
   `collect_binding_idents` and report `TS7031` on every leaf.

That loses the distinction tsc makes between individual binding
elements:

- a binding element default suppresses `TS7031` for that element;
- a parameter initializer can type some slots/properties but not others;
- a contextual parameter type suppresses the whole pattern;
- function signatures and method signatures still report binding leaves;
- member implementations use the same leaf rules as free functions.

This is why a simple "add more leaves" patch is dangerous. It fixes
standing FNs in one fixture and creates FPs in another.

## Current Damage

Snapshot: `/tmp/fcc_after_yield_star_iterable.json`, after commit
`82d0b3b`.

Filtered `TS7031` residue includes:

| Bucket | Count | Representative fixture |
|---|---:|---|
| FN | 35 | `destructuringParameterDeclaration2.ts` |
| FN | 9 | `typeGuardFunctionErrors.ts` |
| FN | 5 | `destructuringWithLiteralInitializers2.ts` |
| FN | 4 | `destructuringParameterDeclaration6.ts` |
| FN | 2 | `wideningTuples5.ts` |
| FP | 9 | `dependentDestructuredVariables.ts` |
| FP | 7 | `destructuringWithLiteralInitializers2.ts` |
| FP | 2 | `argumentExpressionContextualTyping.ts` |
| FP | 6 | `destructuringParameterDeclaration1ES{5,6}.ts` variants |

The `wideningTuples5.ts` FNs are variable-binding `TS7031`, not
parameter `TS7031`. They share the tsc helper path, but the first
implementation should stay parameter-scoped unless the helper can be
shared without widening the behavioral surface.

## tsc Anchors

Vendored tsc is `oracle/node_modules/typescript/lib/_tsc.js`.

- `getTypeForBindingElement` around line 55942 obtains a binding
  element type from the parent binding pattern.
- `getBindingElementTypeFromParentType` around line 55952 handles
  property/element access, defaults, rest, and tuple bounds.
- `getTypeForVariableLikeDeclaration` around line 56080 prefers
  declared type, contextual parameter type, and expression initializer
  before falling back to binding-pattern implied type.
- `getTypeFromBindingElement` around line 56468 reports implicit any
  for a binding element only when that element has no initializer and no
  nested pattern type source.
- `getTypeFromArrayBindingPattern` around line 56531 and
  `getTypeFromBindingPattern` around line 56546 build the implied
  pattern type and pass `reportErrors` down leaf-by-leaf.
- `reportImplicitAny` around line 68122 maps `BindingElement` to
  diagnostic `TS7031` and returns early when `noImplicitAny` is off.
- `getContextualTypeForBindingElement` around line 72736 derives a
  binding-element contextual type from a parent declaration type or
  initializer.

The important shape is: tsc does not have a separate
"collect all leaf identifiers" rule. It reports while constructing or
using the implied binding-pattern type.

## Probe Facts

Use `python3 scripts/probe.py <fixture>` and ignore the standing lib
noise documented in `knowledge-base.md`.

### Binding element defaults suppress only their subtree

`destructuringWithLiteralInitializers2.ts`:

```ts
function f10([x = 0, y]) {}
```

tsc reports `TS7031` for `y`, not for `x`.

`argumentExpressionContextualTyping.ts`:

```ts
function bar({x: [a, b = 10], y: {c, d, e = { f:1 }}}) { }
```

tsc reports `a`, `c`, and `d`. It does not report `b` or `e`.

`dependentDestructuredVariables.ts`:

```ts
function foo({
    value1,
    test1 = value1.test1,
    ...
}) {}
```

tsc reports `TS7031` for `value1` only. It does not report `TS7031`
for `test1` through `test9`, even though their defaults refer to
`value1`. Those defaults still need normal expression checking, but the
TS7031 traversal must not report those leaves.

### Parameter initializer is not a whole-pattern suppressor

`destructuringWithLiteralInitializers2.ts`:

```ts
function f01([x, y] = []) {}
function f02([x, y] = [1]) {}
function f03([x, y] = [1, 'foo']) {}
```

tsc reports:

- `f01`: `x` and `y`;
- `f02`: `y` only;
- `f03`: no `TS7031`.

So `p.initializer.is_some()` cannot return early for a binding pattern.
The initializer provides a source for present slots/properties, while
missing slots/properties still fall back to implied `any`.

### Top-level annotation or contextual parameter type suppresses all leaves

This current behavior is correct and should be preserved:

- `function c3({b}: { b: number|string } = { b: "hello" }) { }`
  produces no binding-element `TS7031`.
- Function expressions/arrow functions checked with a contextual
  parameter type should not report binding leaves through the untyped
  parameter path.

### Signature-only declarations still report

`destructuringParameterDeclaration2.ts` shows tsc reporting `TS7031` in
interface method signatures:

```ts
interface F2 {
    d3([a, b, c]?);
    d4({x, y, z}?);
    e0([a, b, c]);
}
```

The helper must not assume a function body exists. It should keep being
called from the existing type-member paths.

### Parse/grammar reachability is a separate concern

`destructuringParameterDeclaration2.ts` currently has many `TS7031` FNs,
but the sibling `destructuringParameterDeclaration1ES6.ts` already
matches many of the same patterns. Do not curve-fit the leaf helper to
this one file. Some residue is tied to parse/grammar recovery and
semantic reachability, which belongs to the parse-error gate workstream.

## Design

Introduce a diagnostic-only binding traversal that classifies each
binding leaf by whether it has a concrete source. This should replace
the binding-pattern branch of `report_implicit_any_param`; it should not
replace `destructure_binding`, which assigns binding symbol types and
reports property/iterator errors.

### Source model

Use a small local model first:

```rust
enum BindingAnySource {
    Unknown,
    Known,
    ArraySlots(Vec<BindingAnySource>),
    ObjectProps(Vec<(String, BindingAnySource)>),
}
```

Meanings:

- `Unknown`: no annotation/context/default/initializer source is known;
  an identifier leaf under this source needs `TS7031`.
- `Known`: some source expression/type exists; suppress all identifier
  leaves below it for TS7031.
- `ArraySlots`: an array literal source with per-index availability.
  A binding element at an index beyond the available slots receives
  `Unknown`.
- `ObjectProps`: an object literal source with per-property
  availability. A missing property receives `Unknown`.

This is intentionally not a type replacement. It is the minimum
availability model needed to avoid the current FPs/FNs while leaving a
future path to real binding-pattern implied type construction.

### Source construction

Build a source only for binding-pattern parameters:

1. If `p.ty.is_some()`, return without reporting.
2. If `param_ctx_types` contains the parameter, return without
   reporting.
3. If `p.initializer` is absent, use `Unknown`.
4. If `p.initializer` is present:
   - array literal -> `ArraySlots`, recursively mapping each element
     expression;
   - object literal -> `ObjectProps`, recursively mapping property and
     shorthand values with literal property names;
   - anything else -> `Known`.

When mapping an initializer expression:

- nested array/object literals keep their nested shape;
- non-literal expressions become `Known`;
- object spreads should conservatively make unknown properties remain
  `Unknown`; do not let a spread suppress every missing property until
  a focused probe proves that behavior;
- computed property names should not be treated as known literal keys
  unless `PropName::text()` can produce a stable key.

Rust's current `Expr::Array` stores `Vec<Expr>` and does not preserve
elisions as source slots. Do not broaden behavior for elisions in the
first patch; add a probe before supporting them.

### Traversal rules

Add a helper shaped like:

```rust
fn report_binding_pattern_implicit_any(
    &mut self,
    binding: &'a Binding,
    source: &BindingAnySource,
    defaulted: bool,
)
```

Rules:

1. If `defaulted` is true, suppress the entire subtree. This mirrors
   `getTypeFromBindingElement`: a binding element initializer gives that
   element a type source and does not call `reportImplicitAny` for its
   leaf.
2. `Binding::Ident(id)`:
   - if source is `Unknown`, emit `TS7031` at `id.span`;
   - if source is `Known`, `ArraySlots`, or `ObjectProps`, suppress.
     A structured source reaching an identifier means the binding
     element itself had a source.
3. `Binding::Array(p)`:
   - with `ArraySlots(slots)`, recurse element-by-element using the
     matching slot or `Unknown` when missing;
   - with `Known`, suppress the subtree;
   - with `Unknown`, recurse with `Unknown`;
   - pass `defaulted=true` when the `ArrayPatternElem.default` exists.
4. `Binding::Object(p)`:
   - with `ObjectProps(props)`, recurse by property key using the
     matching property source or `Unknown` when missing;
   - with `Known`, suppress the subtree;
   - with `Unknown`, recurse with `Unknown`;
   - pass `defaulted=true` when the `ObjectPatternProp.default` exists.
5. Rest bindings are probe-first. For the first patch:
   - under `Unknown`, report their identifier leaves;
   - under `Known`, suppress;
   - under structured sources, prefer `Known` only when the rest source
     can be proven from the initializer shape. Otherwise use `Unknown`
     and triage any NEW_FP with focused probes.

Keep the existing `noImplicitAny` gate for `BindingElement`: tsc's
`reportImplicitAny` returns early for `BindingElement` when
`noImplicitAny` is off.

### Integration points

Change only the binding-pattern branch in
`src/checker/functions.rs::report_implicit_any_param` at first.

Identifier parameters should keep the existing behavior:

- a parameter initializer suppresses `TS7006`/`TS7044`;
- contextual parameter type suppresses;
- rest identifier parameters use `TS7019`/`TS7047`.

Binding-pattern parameters should change behavior:

- do not return early just because `p.initializer.is_some()`;
- still return early for top-level `p.ty` and `param_ctx_types`;
- emit only `TS7031` under `noImplicitAny`;
- do not call `check_expr` from the TS7031 traversal. Default and
  initializer expression checking belongs to the existing parameter and
  destructuring code paths.

If this helper proves stable for parameters, the same traversal can be
shared with variable declarations that have no initializer/type source.
Do that as a separate step because variable bindings involve `TS1182`,
auto/autoArray types, and initializer assignability.

## Risk Areas

- `destructuringParameterDeclaration2.ts`: do not treat every FN in this
  file as a leaf-helper bug. Some are grammar/recovery reachability.
- `dependentDestructuredVariables.ts`: suppressing defaulted leaves
  should remove `test1`-`test9` `TS7031` FPs, but existing name
  resolution FPs for `value1` in default expressions may remain.
- Rest elements and elisions need focused probes before broad support.
- Object spreads in parameter initializers can easily over-suppress
  missing-property leaves.
- Any change that calls `check_expr` earlier can perturb caches and
  create unrelated FPs. The first implementation should be diagnostic
  only.

## Expected Local Wins

The first parameter-scoped implementation should:

- fix `destructuringWithLiteralInitializers2.ts` FNs for `f01`, `f02`,
  `f11`, and `f12`;
- remove FPs for defaulted leaves in
  `destructuringWithLiteralInitializers2.ts`;
- remove FPs for `b` and `e` in
  `argumentExpressionContextualTyping.ts`;
- remove `TS7031` FPs for `test1` through `test9` in
  `dependentDestructuredVariables.ts`;
- remove the `c1`, `c2`, and `c6` default/initializer FPs in the
  `destructuringParameterDeclaration1ES*` variants.

FNs in `typeGuardFunctionErrors.ts`, `destructuringParameterDeclaration6.ts`,
and parts of `destructuringParameterDeclaration2.ts` should be treated
as follow-up coverage after the default/initializer FP risk is under
control.
