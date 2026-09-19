The shared helper should be one exact checker port of `getFirstTransformableStaticClassElement`, plus a small syntactic accessor-group helper. The one existing accessor-group port is symbol-based and doesn't match upstream's syntactic grouping. The class-expression route has four concrete divergences from upstream. `r92-nested-candidate.diff` is correct and needs no global sortedness check; I'd narrow one new check so it can't touch profiles that were already admitted. Read-only: current checker/syntax source, `_tsc.js`, and the r92 diff.

## 1. A shared `first_transformable_static_class_element`

**Upstream** (`_tsc.js:84921-84948`):
```
wTSDecorated = !legacy && target < ESNext && classOrConstructorParameterIsDecorated(false, class)
wTPrivate    = target < ES2022 || target < ESNext        // i.e. target < ESNext
wTInit       = !emitStandardClassFields
if (wTSDecorated || wTPrivate) for member in class.members (source order):
  if wTSDecorated && classElementOrClassElementParameterIsDecorated(false, member, class)
      return firstDecorator(class) ?? class        // ANY member, static or not; returns the CLASS's decorator
  else if wTPrivate:
      if static block -> return member
      else if isStatic(member) && (isPrivateIdentifierClassElementDeclaration(member)
                                   || wTInit && PropertyDeclaration with initializer) -> return member
```

**Reusable native pieces:**
- `is_static_element` (annotate.rs:5289) is exact `isStatic`, including static blocks.
- `CompilerOptions::emit_standard_class_fields` (types/src/options.rs:509) is exact `getEmitStandardClassFields`.
- `node_can_be_decorated` (calls.rs:571).
- `property_name_for_property_name_node` (class.rs:1461).

**Accessor grouping needs a new syntactic helper.** The only existing port, `node_builder/serialize.rs:2246`, is `getAllAccessorDeclarationsForDeclaration`: it's symbol-based, so it groups duplicates and malformed pairs differently. Port the utility `getAllAccessorDeclarations(members, accessor)` (:16719) exactly:
- An accessor with a dynamic name is its own group.
- Otherwise, group accessors whose `isStatic` equals the target's and whose property name equals it, in member order, recording the first and second accessor plus the get and set accessors.

Then `class_element_or_class_element_parameter_is_decorated(false, member, class)` (:14697):
- **Accessor:** the group's *first accessor that has decorators* must be this member, otherwise false. Parameters come from the set accessor.
- **Method:** its own parameters.
- **Result:** `hasDecorators(member) && node_can_be_decorated(false, member, class, …)`. Parameter decorators never qualify with `useLegacy = false`, because `nodeCanBeDecorated(false, Parameter)` is false. Keep the parameter loop anyway for exactness; it costs nothing.

**The malformed-pair case** is why "any member has decorators" isn't enough: `@dec abstract get x(): number; @dec set x(v) {}`.
- Upstream's first-decorated accessor is the abstract getter, which can't be decorated, so the result is false.
- A naive "any member decorated" check sees the setter and returns true.

**Callers:**
- **calls.rs ~198, standard branch, class declaration:** `needs_set_function_name = name.is_none() || first_transformable_static_class_element(node).is_some()`. Here `wTSDecorated` is true, since this class is decorated.
- **class.rs:514:** see section 2.

## 2. The class-expression route (class.rs:514-605) against :84950-84971

Concrete divergences, all on this route:
1. **Early returns for legacy, ESNext, or no decorator** (:519-531). Upstream only returns early on `node.name`. Undecorated anonymous classes still go through the static-element search when `target < ESNext`, including under `experimentalDecorators`.
   - At ESNext both flags are false, so the result is none anyway.
2. **The location:** upstream uses the class's `firstDecorator ?? class` when standard decorators apply and the class is decorated; otherwise the returned *member*. Native always uses the first decorator.
3. **`__proto__`:** `isNamedEvaluationSource(PropertyAssignment)` is `!isProtoSetter(name)` (:15940), i.e. false for an identifier or string-literal `__proto__`. Native returns true for every PropertyAssignment (:551).
4. **Outer-expression walk:** it matches `walkUpOuterExpressions(All)`: parentheses, all three assertion kinds, non-null, ExpressionWithTypeArguments, and JSDoc casts included. Nothing to change.

**Exact replacement:**
- Return if the class has a name.
- Walk up the outer expressions (unchanged).
- Require a named-evaluation source, adding the `__proto__` exclusion.
- Compute `location = (!legacy && target < ESNext && class decorated) ? first_decorator ?? node : first_transformable_static_class_element(node)`.
- If there's a location: check `SetFunctionName` at it, then `PropKey` at it when the parent's name is computed.

The call-site order (class-like check, then deferred check, then helpers) already matches upstream, so which node receives a missing-helper error still follows upstream's first-request order.

## 3. Compact fixtures

Settings for the helper cases: `importHelpers: true`, with a tslib stub missing `__setFunctionName` and `__propKey`, and a present-export control, run as whole commands over the CommonJS and ESNext modules. Mint expected output from TS. The "report"/"none" labels below are what the upstream code implies.

**Class declarations** (calls.rs route, standard decorators unless noted):
- `@dec export class C { static x = 1 }`: ES2021 report; ES2022 with `useDefineForClassFields: false` report; ES2022 default none; ESNext none.
- `@dec export class C { @dec m() {} }`: ES2022 report (non-static member); ESNext none.
- `@dec export abstract class C { @dec abstract get x(): number; @dec set x(v) {} }`: none (the malformed-pair control).
- `@dec export class C { @dec get x() { return 1 } set x(v) {} }`: report.
- Legacy, `@dec export class C { static #x = 1 }`: no SetFunctionName (legacy branch).
- `export default @dec class {}`: report (no name).

**Class expressions** (class.rs route):
- `export const C = class { static {} }`: ES5 and ES2022 report at the static block; ESNext none; with `experimentalDecorators` at ES2022, still report.
- `export const C = class { static #p = 1 }`: ES2022 report at the member.
- `export const C = class { static x = 1 }`: ES2021 report; ES2022 default none; ES2022 with `useDefine: false` report.
- `export const o = { ["k"]: class { static {} } }`: SetFunctionName and PropKey.
- `export const o = { __proto__: class { static {} } }` and `{ "__proto__": … }`: none.
- Outer expressions: `(class { static {} } as any)`, `<any>(…)`, `(…)!`, `satisfies`: report. `f(class { static {} })`: none.
- `let { C = class { static {} } } = o;` and `C ||= class { static {} }`: report.
- `export const C = @dec class { static x = 1 }`: ES2022 standard, the location is the decorator; ESNext none.
- **Diagnostic ownership:** `export const C = class { static {} }; export const D = class { static #p = 1 };`: exactly one diagnostic, at C's static block.

## 4. `r92-nested-candidate.diff`

**Correct:**
- **Units:** `close_paren_runs` is keyed by the UTF-16 `end` and looked up by the UTF-16 `full_start`; the stored boundary is a byte offset, consistent with `missing_byte`.
- **Chained runs:** each validated skip stores its (possibly bridged) boundary, so runs of any length chain, and owner equality is enforced.
- **Closer chain:**
  - one root among the closers;
  - a direct `.expression` walk whose set equals the closers;
  - an event count over the same start, length and `full_start` (so the speculative event with `full_start` 89 is excluded);
  - exactly one reported event;
  - the single-closer path is unchanged.
- **Tests:** they cover the positives, including a stray `)))` after a statement and the block-with-emoji-comments case with its UTF-16/byte mix. They cover type-assertion, re-nested and `)]` negatives, a leading `));`, and the mutation set: dropped, duplicated or shifted events; reversed, duplicated or shifted actions; a forged token kind; a forged mixed run.

**Sortedness:** no global check is needed. The bridge adds acceptance only through an entry stored by an *earlier validated* skip.
- A reversed order leaves the later skip without an entry, so it falls back to the old `preceding.end == missing_byte` test and fails.
- Nothing previously admitted depended on the map.

**Consume-once:** implied, because two non-overlapping skips can't share a `full_start` equal to one stored end. Removing the key is optional, for readability.

**Least-impact change:** the new rejection (`event.length != length || text != ")"`) runs for *every* `CloseParen` ListAbort skip in every profile, including parameter-gap arrays. It's almost certainly always true today, but it changes an admitted path. Apply it only where a skip is stored into or bridged through `close_paren_runs` (`allow_statement_gaps && missing_node.is_none() && token == CloseParenToken`). Old profiles then keep byte-identical behaviour.

This is supported only for these listed shapes plus the TS whole-command oracle. Native qualification remains pending.
