The six emitter changes are faithful ports with no high-severity bug. There are two medium verification gaps (C2's comment scope and U2's value-use variants) and one low risk in A. G2 and C3 each trace to one printer call. Neither has a native JS capture yet: the directory `/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-r77-supplemental-captures` doesn't exist. So every native-JS statement below is inferred from map column arithmetic; the upstream side comes from probes against the pinned vendor build, whose maps equal your expected maps. This was read-only: `git diff` over 5e6a4cfa1 plus the final differences file.

## 1. Review of the six frozen files

**A (`legacy_decorators.rs`): correct.**
- **Class sites:** the marker is `constructor_metadata.is_some()`. It reaches both the class declaration location and the class decoration statement.
- **Member decoration statement:** uses `!metadata.is_empty()`, taken before `extend` consumes the vector. The owner set is filled at the metadata computation site, and `finish_class_element` looks it up by original ID.
- **Composition loop:**
  - class: skips only the leading export/default run, then counts non-decorator modifiers;
  - element: counts non-decorator modifiers;
  - if none remain, the start falls back to `declaration.pos`;
  - properties and methods still short-circuit to `name.pos`.
- **Low risk:** the member decoration statement now ranges from the *current* member. If a ts-transform rebuilt a member with a synthesized range, the statement's source map loses its location where it used to take the original's. The metadata-false neighbours cover this: `@dec public get`, `@dec override get`, and parameter-property classes.

**U1 (`flatten_destructuring.rs`): correct.**
- **Scoping:** the typed role is threaded through every converter. The generic flattener passes `Existing`; only es_next passes `ExpressionReference`.
- **Clone rule:** only parse-tree, non-generated identifiers are cloned. `clone_node` keeps their emit flags; the clone gets the original, the range, and the internal reference flag. Generated or synthesized names return unchanged.
- **Positions covered:** shorthand, object and array rest, and element/property targets.
- **Witness still needed:** because node identity changes, run the ES2015 shorthand CommonJS rows that already pass, plus every System `using` shape.

**U2 (`es2018.rs`): correct, with one question.** It implements `else if (nodeIsSynthesized(node)) location = value` exactly, only under `!force_fresh && Unused`. If `ExpressionValueUse` has a third variant besides `Required` and `Unused`, check that it counts as "discarded" the same way upstream's `!needsValue` does.

**C1 (`standard_decorators.rs`): correct.** It clears the question-dot token only on the fresh node in the cached branch; the uncached branch keeps the chain, as upstream does.

**C2 (`printer.rs`): the gates are right; one assumption needs a witness.**
- **Upstream checks:** the before-arm checks `end !== containerEnd` via `retains_end`, and the after-arm checks `pos !== containerPos`. Both are also gated on disabled comments.
- **Medium, unverified:** `expression_context.comments()` must be the PartiallyEmittedExpression's *own* container. Upstream's `emitCommentsBeforeNode(PEE)` sets `containerEnd = PEE.end` before `emitPartiallyEmittedExpression` runs.
  - The failing row can't tell the two apart, because the Await and the PEE both end at 24.
  - If this is the parent's scope, a zero-width missing operand inside a call or array still prints the comment twice. Examples: `f(<number /*c*/, 2)` or `[<number /*c*/, 1]` in module code.
  - Make sure the 72 assertion-comment neighbours include a missing operand as a call argument, an array element, and the right side of a binary.

## 2. G2: statement-gap optional-line / utf16-comments (12 rows, remove-false)

**Inferred native text** (utf16-comments, ES5/ESNext):
```
export var value = (a //😀
); //😀
as;
```
TS writes `export var value = (a); //😀`. The comment is printed twice and a line break is inserted, so the JS differs. Evidence: native maps put a comment start at col 22 (0:56) on line 0, then paren-end and statement-end maps at line 1 cols 1–2, then the comment again at line 1 col 3.

**Upstream.**
- The `)` is missing (no line break is allowed before `as`), so `ParenthesizedExpression.end == a.end == 55`.
  - The child's trailing comment is suppressed because its end equals the container end.
  - `emitTokenWithComment(CloseParen, expression.end=55, …, node)` emits only **leading** comments at 55. Leading comment ranges at pos > 0 skip same-line comments, so nothing prints.
  - The statement's trailing comment prints `//😀` once.
- optional-line at ES2015/ES5: the child is the synthesized conditional that optional-chain lowering produces. `expression.end` is -1 there, so upstream emits no token comments at all.

**Native cause** (printer :7346). The source-paren branch writes the close token with `emit_token_with_comments`, i.e. `PositionCommentPhase::BoundaryUnion`, the trailing+leading union, at the child's original end. The union takes the same-line `//😀` and forces a line break.

**Fix.** Use `emit_token_with_source_leading_comments` (`PositionCommentPhase::SourceLeading`) for this close token. The printer's own doc-comment describes that helper as "used after an ordinary node's complete comments phase". Keep the cursor as it is.

**Why ordinary parens don't regress:**
- `(a /*c*/)` and `(a //c\n)`: the child's own trailing phase has already printed the comment, because `a.end` differs from the paren's end. Leading ranges at `a.end` exclude same-line comments, so nothing more prints, exactly as upstream.
- `(a\n/*c*/)`: after the newline the comment is a leading comment; both phases print it before `)`.

The only behavioural change is losing the union's same-line trailing part. That part only matters when the child's trailing phase was suppressed, which requires `child.end == paren.end`, which means a missing `)`. That shape exists only in recovery.

**Open cursor edge:** upstream uses the current child's end, -1 for a synthesized child; native uses `original_node_end_cursor`. They diverge only for `(object?.x\n// c\n)` at ES2019 or below, where upstream prints no comment and native `SourceLeading` at 67 would print `// c`. Add that as a witness. If it differs, gate the token phase on whether the child has a source range, but don't change the cursor preemptively.

## 3. C3: heritage-trivia (4 rows: es2015 and esnext targets × CommonJS and ESNext modules, remove-false)

**Expected:** `22→45` (the end of the expression-with-type-arguments), `23→46` (its trailing ` /*😀*/`), `29→53` (the end of the heritage clause).

**Native:** only `22→53`. Nothing is written between `string` and the heritage end mapping. **Inference:** the comment is dropped (`class C extends string {`); the capture should confirm this.

**Upstream:**
- The `ExpressionWithTypeArguments` [39,45] and its identifier [39,45] share one range.
- The identifier's trailing comment at 45 is suppressed, since its end equals the container end.
- The `ExpressionWithTypeArguments` then runs its own after-node phase with the heritage container restored (end 53): it prints the trailing comments at 45 and `emitPos(45)`.
- The heritage clause then maps its end to 53.

**Native cause** (printer :5298). The arm emits the child through `emit_required_node_with_context_and_source_extent(..., node, LeadingAndTrailing)`. That hands the wrapper's comment extent to the child, where the end-45 trailing phase is correctly suppressed. As a result the wrapper's own after-node trailing phase and its end map never run with the heritage container.

**Fix:**
- When `data.expression` has a source range (it is a parse node), emit it with plain `emit_required_node_with_context` (keeping the `left_side_of_access` context), so the wrapper's own pipeline runs both before- and after-phases.
- Keep the extent path only for synthesized children. The arm's comment names the reason for that path: optional-chain lowering turns the heritage expression into a conditional with no range.

**Why ordinary heritage clauses don't regress:**
- `extends B {}`: wrapper end = child end = heritage end. The wrapper's trailing comment is suppressed (equals the heritage `containerEnd`), and the extra `emitPos` lands on the same generated column, where upstream's source-map generator keeps the last mapping. Same bytes.
- `extends B<T>`: the wrapper ends after `>`, which equals the heritage end, so the wrapper's trailing comment is still suppressed. The child's trailing comments at `B.end` then *do* print when a comment sits between `B` and `<`. That matches upstream; today native probably drops it (a latent bug that the fix repairs).

**Neighbours:**
- `class C extends B /*c*/<T> {}`
- `extends B<T /*c*/> {}` (the comment is dropped, as upstream does)
- `extends B<T> /*c*/ {}`
- `extends (B) {}` and `extends B.C<T> {}`
- `extends a?.b {}` at ES2019 or below, the synthesized-child path, for both comment settings
- ES5 `__extends` rows

## 4. Missing neighbours to add

- **C2:** the missing-operand assertion as a call argument, an array element, and the right side of a binary (the scope question in section 1).
- **G2:**
  - `(a //c\n)` and `(a /*c*/)` with a real `)`;
  - `(a\n/*c*/)`;
  - a block comment with a missing close paren: `(a /*c*/\n as number)`;
  - `((a //c\n as number))`;
  - `(object?.x\n// c\n)` at ES2019 or below.
- **C3:** the heritage list in section 3.
- **U1:** the ES2015 System and CommonJS `using` object rows that pass today, as identity-change controls.