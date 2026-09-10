# Remaining 53 decorator/accessor cases: review handoff

This is a review aid for the user's Claude Code collaboration request, not a
production design gate or a claim that H2.8 is complete.

The strongest completed full command comparison is attempt53: **477/530 exact
twice,53 fail**, all complete tuples unchanged from42. Authority:
`ratchets/h2-8a-utf16-writer-design-experiment.v1.json`, SHA-256
`72bead2204a6091714650a6a2b0065a8b3439aea8840cd07cb432a4f9dd20f31`.
Its prelaunch section pins the actual copied source, layered candidate patches,
source archive and retained binaries. Root code alone is not that candidate.

The current v15 changes literal-property ownership and raw UTF16 values; its
2155 direct controls and72 neighbors pass. It has no standard-decorator change,
and its full530 effect has not yet been measured. This handoff therefore uses
full53 as the observed baseline. See the owning retained-lexical-owners document
for open production gates and the current runner for candidate layering.

Please investigate receiver state and static-accessor/map composition, propose
source-derived changes, and compare against all530 complete command tuples.
The preserved477 and the two repeated observations remain required. Exact
JavaScript alone does not establish source-map, diagnostic or callback equality.
Use an isolated draft for the proposed standard_decorators.rs change so that
both implementations remain reviewable during the current frozen test run.

## Source and current Rust

Pinned source is `vendor/typescript-6.0.3/lib/_tsc.js` (whole-file SHA-256
`1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3`).

- `transformESDecorators` begins98946. State update, class/class-element/name/
  other transitions and visit predicates are98973–99060. A static property or
  static block carries the decorated class receiver; an ordinary function or
  object method enters an other frame; an arrow retains lexical state.
- Name-state lookup uses the specific `top.next.next.next` class-element frame.
  A blanket stop at every nested class or every method loses computed-name
  behavior. Heritage expressions are visited before enterClass (99367–99387).
- `visitThisExpression`100151–100153 returns the classThis node identity directly;
  it does not copy the replaced this token's range. Map ownership must follow it.
- `visitClassStaticBlockDeclaration`100005–100040 suppresses receiver substitution
  inside the class-this assignment block, but permits it in named-evaluation
  helpers and ordinary static blocks.
- `createClassInfo`99241–99318 selects FileLevel versus ReservedInNestedScopes
  for classThis based on private static and auto-accessor members. Super property,
  call, tag, assignment and update paths have separate receiver behavior.
- Current Rust `crates/emitter/src/builtins/standard_decorators.rs` allocates the
  class_this identity near1208 and retargets the named-evaluation member near1241.
  Ordinary static blocks near1310 and properties near1400 use the generic visitor.
  `DecoratorClassThisRewriter` near5094 stops whole classes/functions/methods;
  broadening its callers alone does not establish source frame semantics.

## Failure families

- decorator-receiver-context: 28
- decorator-static-accessor-handoff: 21
- retained-accessor-redirectors: 4

## Three concrete witnesses

### `decorator-static-accessor-handoff/es2022/esnext/define/class-static-public-field`

Fixture: `crates/compiler/tests/fixtures/retained-accessor-owners.json`.

```ts
interface I { method(): number; }
function dec(value: any, context: any): any { return value; }
@dec export class Box { static value: any = this; }
export const tail = 1;
```

```diff
--- TypeScript
+++ native
@@ -47,5 +47,5 @@
             if (_metadata) Object.defineProperty(_classThis, Symbol.metadata, { enumerable: true, configurable: true, writable: true, value: _metadata });
         }
-        static value = _classThis;
+        static value = this;
         static {
             __runInitializers(_classThis, _classExtraInitializers);
```

### `decorator-receiver-context/es2022/esnext/define/nested-function-computed-name`

Fixture: `crates/compiler/tests/fixtures/decorator-receiver-context.json`.

```ts
function dec(value: any, context: any): any { return value; }
@dec export class Box { static key: string = "key"; static value: any = (function(this: any) { return class Inner { [this.key](): any { return this; } static self: any = this; }; }).call(this); }
export const tail = 1;
```

```diff
--- TypeScript
+++ native
@@ -51,5 +51,5 @@
             [this.key]() { return this; }
             static self = this;
-        }; }).call(_classThis);
+        }; }).call(this);
         static {
             __runInitializers(_classThis, _classExtraInitializers);
```

### `decorator-static-accessor-handoff/es2015/esnext/set/class-static-private-field`

Fixture: `crates/compiler/tests/fixtures/retained-accessor-owners.json`.

```ts
interface I { method(): number; }
function dec(value: any, context: any): any { return value; }
@dec export class Box { static #value: any = this; static read(): any { return this.#value; } }
export const tail = 1;
```

JavaScript is identical; the source-map `mappings` field differs.

## All failing case IDs

```text
decorator-receiver-context/es2015/esnext/define/nested-arrow-computed-name
decorator-receiver-context/es2015/esnext/define/nested-function-computed-name
decorator-receiver-context/es2015/esnext/define/nested-undecorated-computed-name
decorator-receiver-context/es2015/esnext/define/nested-undecorated-heritage
decorator-receiver-context/es2015/esnext/define/static-arrow-and-function
decorator-receiver-context/es2015/esnext/set/nested-arrow-computed-name
decorator-receiver-context/es2015/esnext/set/nested-function-computed-name
decorator-receiver-context/es2015/esnext/set/nested-undecorated-computed-name
decorator-receiver-context/es2015/esnext/set/nested-undecorated-heritage
decorator-receiver-context/es2015/esnext/set/static-arrow-and-function
decorator-receiver-context/es2022/esnext/define/nested-arrow-computed-name
decorator-receiver-context/es2022/esnext/define/nested-function-computed-name
decorator-receiver-context/es2022/esnext/define/nested-undecorated-computed-name
decorator-receiver-context/es2022/esnext/define/nested-undecorated-heritage
decorator-receiver-context/es2022/esnext/define/runtime-static-block
decorator-receiver-context/es2022/esnext/define/static-arrow-and-function
decorator-receiver-context/es2022/esnext/set/nested-arrow-computed-name
decorator-receiver-context/es2022/esnext/set/nested-function-computed-name
decorator-receiver-context/es2022/esnext/set/nested-undecorated-computed-name
decorator-receiver-context/es2022/esnext/set/nested-undecorated-heritage
decorator-receiver-context/es2022/esnext/set/runtime-static-block
decorator-receiver-context/es2022/esnext/set/static-arrow-and-function
decorator-receiver-context/esnext/esnext/set/nested-arrow-computed-name
decorator-receiver-context/esnext/esnext/set/nested-function-computed-name
decorator-receiver-context/esnext/esnext/set/nested-undecorated-computed-name
decorator-receiver-context/esnext/esnext/set/nested-undecorated-heritage
decorator-receiver-context/esnext/esnext/set/runtime-static-block
decorator-receiver-context/esnext/esnext/set/static-arrow-and-function
decorator-static-accessor-handoff/es2015/esnext/define/class-instance-accessor
decorator-static-accessor-handoff/es2015/esnext/define/class-static-accessor
decorator-static-accessor-handoff/es2015/esnext/define/class-static-private-field
decorator-static-accessor-handoff/es2015/esnext/define/class-static-public-field
decorator-static-accessor-handoff/es2015/esnext/define/nested-class-static-accessor
decorator-static-accessor-handoff/es2015/esnext/set/class-instance-accessor
decorator-static-accessor-handoff/es2015/esnext/set/class-static-accessor
decorator-static-accessor-handoff/es2015/esnext/set/class-static-private-field
decorator-static-accessor-handoff/es2015/esnext/set/class-static-public-field
decorator-static-accessor-handoff/es2015/esnext/set/nested-class-static-accessor
decorator-static-accessor-handoff/es2022/esnext/define/class-static-accessor
decorator-static-accessor-handoff/es2022/esnext/define/class-static-private-field
decorator-static-accessor-handoff/es2022/esnext/define/class-static-public-field
decorator-static-accessor-handoff/es2022/esnext/define/nested-class-static-accessor
decorator-static-accessor-handoff/es2022/esnext/set/class-static-accessor
decorator-static-accessor-handoff/es2022/esnext/set/class-static-private-field
decorator-static-accessor-handoff/es2022/esnext/set/class-static-public-field
decorator-static-accessor-handoff/es2022/esnext/set/nested-class-static-accessor
decorator-static-accessor-handoff/esnext/esnext/set/class-static-private-field
decorator-static-accessor-handoff/esnext/esnext/set/class-static-public-field
decorator-static-accessor-handoff/esnext/esnext/set/member-static-accessor
retained-accessor-redirectors/es2022/esnext/define/decorated-class-this-control/retain
retained-accessor-redirectors/es2022/esnext/define/decorated-computed-cache-control/retain
retained-accessor-redirectors/es2022/esnext/set/decorated-class-this-control/retain
retained-accessor-redirectors/es2022/esnext/set/decorated-computed-cache-control/retain
```
