# Destructuring parameter implicit-any steps

Companion to `destructuring-parameter-implicit-any.md`. Follow these
steps in order. Do not skip the focused probes: this cluster already has
standing FPs, and the safe implementation path is leaf-by-leaf.

## Stage 0: Baseline and probes [P]

Build the release binary before probing:

```sh
cargo build --release
```

Run these focused probes and save the outputs under `/tmp` if you need
to diff during implementation:

```sh
python3 scripts/probe.py ts-tests/tests/cases/conformance/es6/destructuring/destructuringWithLiteralInitializers2.ts
python3 scripts/probe.py ts-tests/tests/cases/conformance/expressions/contextualTyping/argumentExpressionContextualTyping.ts
python3 scripts/probe.py ts-tests/tests/cases/conformance/controlFlow/dependentDestructuredVariables.ts
python3 scripts/probe.py ts-tests/tests/cases/conformance/es6/destructuring/destructuringParameterDeclaration1ES6.ts
```

Use this filter when focusing only one-sided main-file diagnostics:

```sh
python3 scripts/probe.py <fixture.ts> \
  | awk '/--- tsc:/{s=1} /^  \* main/{print (s ? "FN " : "FP ") $0}'
```

Expected baseline highlights:

- `destructuringWithLiteralInitializers2.ts`: FNs on `[x,y]=[]` and
  missing initializer slots; FPs on defaulted leaves.
- `argumentExpressionContextualTyping.ts`: FPs on defaulted leaves `b`
  and `e`.
- `dependentDestructuredVariables.ts`: FPs on `test1` through `test9`.
- `destructuringParameterDeclaration1ES6.ts`: FPs on `c1`/`c2`/`c6`
  default and initializer cases.

## Stage 1: Add a diagnostic source model [M]

In `src/checker/functions.rs`, add a private helper model near
`report_implicit_any_param`:

```rust
enum BindingAnySource {
    Unknown,
    Known,
    ArraySlots(Vec<BindingAnySource>),
    ObjectProps(Vec<(String, BindingAnySource)>),
}
```

Add helpers:

- `binding_any_source_from_initializer(init: &Expr) -> BindingAnySource`
- `binding_any_source_from_expr(expr: &Expr) -> BindingAnySource`
- `object_source_lookup(props: &[(String, BindingAnySource)], key: &str)`

Implementation notes:

- `Expr::Array { elements, .. }` maps each element recursively.
- `Expr::Object { props, .. }` maps `Property` and `Shorthand` with a
  literal/static key; skip `Spread` and unsupported computed keys.
- Any non-literal expression returns `Known`.

No behavior should change in this stage. Keep it compile-only if
possible.

## Stage 2: Replace collect-all leaf reporting for parameters [M]

Change only the `pattern => { ... }` branch in
`report_implicit_any_param`.

Preserve these top-level gates:

- `suppress_next_function_implicit_any_params`;
- `p.ty.is_some()`;
- `param_ctx_types.contains_key(&node_key(p))`;
- `noImplicitAny` for binding elements.

Do not preserve `p.initializer.is_some()` as a binding-pattern early
return. For identifier parameters it should still suppress the
identifier implicit-any diagnostic.

Add traversal:

```rust
fn report_binding_pattern_implicit_any(
    &mut self,
    binding: &'a Binding,
    source: &BindingAnySource,
    defaulted: bool,
)
```

Apply the rules from the design doc:

- `defaulted` suppresses the subtree;
- `Unknown` at an identifier reports `TS7031`;
- `Known` suppresses;
- `ArraySlots` and `ObjectProps` recurse by index/key;
- missing index/key becomes `Unknown`.

After this stage, run:

```sh
cargo build --release
python3 scripts/probe.py ts-tests/tests/cases/conformance/es6/destructuring/destructuringWithLiteralInitializers2.ts
python3 scripts/probe.py ts-tests/tests/cases/conformance/expressions/contextualTyping/argumentExpressionContextualTyping.ts
python3 scripts/probe.py ts-tests/tests/cases/conformance/controlFlow/dependentDestructuredVariables.ts
```

Expected result: the known default/initializer FPs should shrink before
you chase additional FNs.

## Stage 3: Triage NEW_FP before broadening [T]

Run:

```sh
./verify.sh golden-check
```

If there are NEW_FPs:

1. Classify whether they are default suppression, parameter initializer
   source, contextual parameter type, rest/elision, or object spread.
2. Probe the smallest existing fixture first. If needed, make a scratch
   micro-fixture outside the repo and probe it.
3. Fix the source/traversal rule, not the fixture.

Do not stop just because NEW_FNs appear. Root-cause them and continue
when they are explainable by this workstream.

## Stage 4: Signature and member coverage [P]

Probe:

```sh
python3 scripts/probe.py ts-tests/tests/cases/conformance/es6/destructuring/destructuringParameterDeclaration2.ts \
  | rg '########|---|TS7031|TS2463|TS2371|TS7010'
python3 scripts/probe.py ts-tests/tests/cases/conformance/expressions/typeGuards/typeGuardFunctionErrors.ts \
  | rg '########|---|TS7031|TS1230|TS1229'
python3 scripts/probe.py ts-tests/tests/cases/conformance/es6/destructuring/destructuringParameterDeclaration6.ts \
  | rg '########|---|TS7031'
```

Check that:

- type-member method signatures still call `report_implicit_any_param`;
- function declarations without bodies are not silently excluded;
- invalid optional binding-pattern implementation signatures can still
  report both their grammar diagnostic and `TS7031`;
- parse/grammar reachability issues are documented instead of patched
  through the TS7031 helper.

This is the point to decide whether any remaining FNs belong to
parse-error semantic gating rather than this workstream.

## Stage 5: Optional variable-binding sharing [T]

Only after Stage 3 is at 0 NEW_FP, consider sharing the traversal with
variable declarations.

Primary probe:

```sh
python3 scripts/probe.py ts-tests/tests/cases/conformance/types/tuple/wideningTuples5.ts
```

Keep this separate from the parameter commit if it touches
`src/checker/stmts.rs`, because variable declarations have additional
interactions:

- `TS1182` for destructuring declarations without initializers;
- auto and autoArray variable types;
- initializer assignability;
- for-in/for-of declaration behavior.

## Final Gate

Before committing any implementation:

```sh
cargo fmt
cargo build --release
cargo test --release
./verify.sh golden-check
```

Commit only at 0 NEW_FP. If NEW_FNs remain, document their root cause in
the commit message and prefer keeping the implementation narrow over
masking them with broad suppression.
