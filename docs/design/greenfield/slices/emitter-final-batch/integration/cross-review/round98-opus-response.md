I traced all four failure groups to concrete code causes, and I withdraw my r97 claim about `h2_6c_de_promotions.rs`. This analysis uses the immutable `c1e699ab5` code, the r95 partial-differences file and the pinned `_tsc.js`; nothing was built or run. Only the four decorated-class rows are confirmed JS differences. For the other 39 the first failing assertion is a map, and complete captures are still needed before calling any of them maps-only.

## 1. Anonymous decorated class expression at ES5 (4 rows, JS differs): `class_2` instead of `class_1`

**Upstream:** both names come from the same cache entry.
- The ES-decorators pass names the reference with `getGeneratedNameForNode(node)`.
- The ES2015 pass later calls `getInternalName(classExpr)`, which is also `getGeneratedNameForNode(classExpr)`.
- The printer resolves each generated name with `getNodeForGeneratedName` (:28084), which follows `original` up to the parse-tree class. `generateNameCached` (:120633) keys on that node, so both yield `class_1`.

**Native:** two separate caches.
- `standard_decorators.rs:1653-1666` allocates the outer reference with `TargetBinding::allocate_numbered("class", …)` and records it nowhere shared.
- `es2015.rs` `get_generated_name_for_node` (~1250) looks in its own `print_state.generated_names_for_nodes`, keyed by the root original. It misses, and allocates a second `class` name, `class_2`.

**Smallest fix:** give both passes one context-level cache (in `TransformationContext`), keyed by `(source, root original)` exactly as `getNodeForGeneratedName` resolves it, and mapping to a `TargetBinding`.
- Standard decorators register the anonymous class-expression reference binding there, under the declaration owner.
- ES2015's lookup consults the shared cache before allocating.

Don't give the class expression a name instead: at ES2015 and above upstream prints `class {}` unnamed, so naming it would change that output.

**Controls:**
- two anonymous decorated class expressions in one file (`class_1`/`class_1` and `class_2`/`class_2`);
- a user identifier `class_1` in scope (numbering);
- ES2015 and ES2022 (no inner function; bytes unchanged);
- an undecorated anonymous class expression at ES5;
- a nested decorated anonymous class;
- `export default @dec class {}` (the existing `default_1` path);
- a named `@dec class D {}`.

## 2. `export const C = (class { static {} } as any)` at ES5 (4 rows)

**Upstream pipeline:**
1. The ts transform (`visitParenthesizedExpression`, :95108) turns the parenthesized assertion into `PartiallyEmitted[paren range 17..45](PartiallyEmitted[as range](class))`. The source paren disappears.
2. Class-field lowering updates the variable declaration. `parenthesizeExpressionForDisallowedComma` sees a comma inside the partially-emitted wrappers and creates a paren ranged to the **outer** wrapper.
3. So `(` maps to 1:17, `_a` to 1:18, and the closing `)` to 1:45.

**Native:** the paren maps to 1:18 (the class), and the 1:18 and 1:45 segments are missing. The paren was therefore created against the class or comma node, after the outer partially-emitted wrapper chain was lost.

**Where to look:** ES2015 passes, so the loss happens only on the ES5 path. The candidates are:
- **ES5 class-to-IIFE lowering:** `es2015.rs:8140-8200` (`restore_outer_expressions` for the callee, alias and initializer);
- **`is_ignorable_paren` (:8315):** it treats a factory paren as ignorable unless it has a source-map or comment range. Upstream's `isIgnorableParen` also checks the node's own position, and the parenthesizer's paren has a text range (`setTextRange(paren, expression)`). If native's factory paren carries only a text range, it gets dropped and later re-wrapped at the class.

**Required invariant:** the disallowed-comma paren keeps the outer wrapper's range. Fix whichever of the two drops it. Don't special-case `as`.

**Controls:**
- `(class { static {} } as any)`, `(<any>class { static {} })`, `(class { static {} })!`, `(… satisfies unknown)`;
- `(class { static {} })`, where the source paren is kept;
- an unparenthesized `class { static {} }`;
- each at ES5 and ES2015.

## 3. `yield*` and `for await`

**3a. Kept `yield*` (ES2015/ES2022/ESNext, including a kept async `yield*`): the maps at `*` are missing** (0:39/0:40).
- **Upstream:** `emitYieldExpression` does `emit(node.asteriskToken)`, i.e. the full node pipeline, which maps the token's start and end.
- **Native:** printer.rs:5082-5104 writes `*` with `emit_token_with_comments(FixedToken)`, which has no token-node source-map phase.
- **Fix:** when `asterisk_token` is a parsed token node, emit it through the retained-token pipeline, generalizing `emit_retained_arrow_token_with_comments` to the asterisk. A synthesized asterisk keeps the current path.
- **Controls:** `yield*x`, `yield /*a*/ * /*b*/ x`, and a nested `yield*`, at ES2015, ES2022 and ESNext.

**3b. Lowered async `yield*` (ES2015 row 4:110 at 0:40; ES5 generator rows 6:16, 6:38 etc.).**
- **Upstream** (:101912-101940): the inner yield is `factory.updateYieldExpression(node, node.asteriskToken, …)`, so it keeps the node's range and original.
- **Native:** `es2018.rs:1897-1898` builds it with `create_yield_expression(asterisk, delegated)`, a fresh node with no range. The ES5 generator lowering then has no location to use.
- **Fix:** build the inner yield as an update of `original`. It is the single root cause for the ES5 async rows too, plus 3a's asterisk mapping where the asterisk is kept.

**3c. `for await` lowering (7 rows): missing and extra ranges.** Port `transformForAwaitOfStatement` and `convertForOfStatementHead` (:102241) line by line in es2018.rs:
- `callValues`, the iterator declaration and the declaration list are ranged to `node.expression`, the list with `NoHoisting`;
- the for statement is ranged to `node` with `NoTokenTrailingSourceMaps`;
- the two head statements get `setSourceMapRange(node.expression)` (native lacks these: the rows `_d = …` and `_a = false` at 0:61..0:67);
- the body block gets `setTextRange(bodyLocation)`, and its statement array `statementsLocation` (native lacks the `}` at 0:70);
- the surrounding try/catch/finally blocks get **no** ranges. Native maps `try {` and `{` to 0:39 (extra segments).

**Controls:**
- a block body;
- a non-block body `for await (const x of v) f(x);`;
- a labeled `for await`;
- a destructured binding;
- comments inside the body;
- at ES5, ES2015 and ES2017.

All of these keep the existing child comment phases and the bind/heritage fixes untouched.

## `h2_6c_de_promotions.rs`: correction

You're right, and I withdraw my r97 statement. Lines 2006-2020 hold three `frozen(workspace, "ratchets/h2-7de-…", "hex")` checks. The existing `PAIR` form matches them, so it belongs in `FROZEN_VERIFY` with a count of 3. The case and input fingerprints elsewhere in the file aren't paths and stay uncounted.

**Risks in the Codex draft:**
- A named constant defined in another file (`H2_6C_QUALIFICATION_RELATIVE_PATH`) must stay unresolved rather than being guessed.
- The per-file counts should be exact, so that any extra matching `"path", "hex"` text, for example in mutation tests, fails loudly.
- `--fix` must skip `FROZEN_VERIFY` files before any write.
- The discovery guard must keep refusing unclassified files.
