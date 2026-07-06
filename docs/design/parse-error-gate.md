# Design: per-node parse-error gating + non-LHS `=` recovery (PAIRED)

**Yield**: the single largest FN lever left. tsrs currently drops ALL
semantic diagnostics for any file containing ≥1 syntax error; tsc drops
only checks whose enclosing node actually contains the parse error.
Corpus damage: parserRealSource11 (87 FN), parserharness (36),
typeGuardFunctionErrors (29), most of the 6133 FN family (387), plus the
1005 (125 FN) / 1109 (80 FN) parser-recovery families.

**Why PAIRED**: a previous attempt to fix either half alone was reverted
(see conformance-sweep memory, item (b)). Fixing the recovery without
un-gating semantics flips files to syntactic-only output in new places;
un-gating semantics without fixing recovery floods 2364-family FPs from
mis-built `Assign` nodes. Land as ONE gated series where intermediate
commits may be classifier-red locally but the SERIES gate is run on the
final state. Prefer a feature branch.

## Part A — non-LHS `=` recovery (parser)

### tsc behavior (source of truth)

`parseAssignmentExpressionOrHigher` (grep in
`oracle/node_modules/typescript/lib/_tsc.js`): after parsing the LHS as
a binary/unary expression, tsc only makes an assignment when

```js
if (isLeftHandSideExpression(expr) && isAssignmentOperator(reScanGreaterToken())) {
    return makeBinaryExpression(expr, parseTokenNode(), parseAssignmentExpressionOrHigher(...), pos);
}
```

`isLeftHandSideExpression` = kind ∈ {Identifier, PropertyAccess,
ElementAccess, Call, New, TaggedTemplate, ArrayLiteral, ObjectLiteral,
Parenthesized, literals, this/super, MetaProperty, NonNullExpression,
class/function expressions, JSX...} — see `isLeftHandSideExpressionKind`.
Critically NOT: unary expressions (`!x`, `-x`, `typeof x`, `await x`),
binary expressions, conditionals, arrows.

So for `await x = 1` / `a + b = c`, tsc parses the LHS, does NOT consume
`=`, and statement-level recovery emits **TS1005 ';' expected** at the
`=` (or 1128/1109 depending on context), leaving `= 1` to be re-parsed
as a new (error-recovered) statement. tsrs instead consumes `=` and
builds `Expr::Binary{op: Assign}`, later producing semantic 2364
("invalid assignment target") FPs and shifting all downstream spans.

### tsrs change

- Anchor: `src/parser/expr.rs` — the binary/assignment parse loop (the
  `Tok::Eq => Some(BinOp::Assign)` mapping near line 63 feeds it).
- Add `fn is_left_hand_side_expr(e: &Expr) -> bool` mirroring
  `isLeftHandSideExpressionKind` (match on tsrs `Expr` variants;
  include `Paren`, `NonNull`, exclude `Unary`, `Await`, `Binary`,
  `Cond`, `Arrow`).
- In the assignment-parse branch: if the parsed LHS fails
  `is_left_hand_side_expr` and the pending token is `=` (or any
  compound-assign token — tsc treats all `isAssignmentOperator` tokens
  the same here), do NOT consume the operator; return the LHS
  expression as-is and let statement-level recovery run. The expected
  recovery: expression-statement終了処理 fails to find `;` → emit 1005
  at the `=` token position → `parse_statement` resumes at `=`?? — tsc
  resumes by treating the remainder `= rhs;` via error recovery: the
  `=` token is SKIPPED by `parseErrorAtCurrentToken` + statement loop
  (verify exact resume behavior against oracle probes BEFORE coding:
  probe `await using = 6;`, `a + b = c;`, `!x = 1;`, `x++ = 3;` and pin
  the exact 1005/1128/1109 code+position each produces).
- KNOWN COUPLING (from unused-sweep U4): `using [x]` / AWAIT-using kept
  `[`-recovery to preserve the old profile — re-check
  `parser/stmt.rs` `is_using` handling when the recovery changes; the
  U4 commit message and memory note the exact fixtures
  (asiPreventsParsingAs* family).

### Acceptance for Part A alone (intermediate)

`tsrs --check-batch` over the corpus: the SYNTACTIC diagnostic set for
the ~200 files in the 1005/1109 families moves toward oracle; expect
temporary whole-file flips (worse) until Part B lands. Track with the
classifier but do not hard-gate until B.

## Part B — per-node semantic gating

### Current tsrs behavior

`src/lib.rs:742` (`check_program_core`):

```rust
let diags = if !syntactic.is_empty() { syntactic } else { /* bind + check */ };
```

All-or-nothing. tsc instead ALWAYS runs the checker and CONCATENATES
syntactic + semantic diagnostics per file; the checker self-censors in
only two ways:

1. `checkSourceElement` / grammar checks consult
   `containsParseError(node)` (`_tsc.js:12854`; a node-flag
   `ThisNodeHasError`/`ThisNodeOrAnySubNodesHasError` propagated by the
   parser) to skip SOME checks on nodes that own a parse error.
2. Error-recovery placeholder nodes ("missing" identifiers/expressions,
   `nodeIsMissing`) type as errorType and stay silent downstream
   (errorType suppresses cascades — tsrs `TypeKind::Error` already has
   this contract).

### tsrs change (staged)

1. **Parser: record error-carrying nodes.** Add to the parse result a
   set `parse_error_nodes: HashSet<usize /*node_key*/>` — every node in
   whose SPAN a syntactic diagnostic landed, plus explicit "missing"
   placeholder nodes the recovery synthesizes. Cheapest faithful
   approximation of tsc's flag propagation: after parsing, for each
   syntactic diagnostic, walk the statement list of the file and mark
   the innermost enclosing STATEMENT (statement granularity is enough
   for the corpus; tsc's flag is per-node but checks that consult it
   are statement/declaration-level). Store on `ParsedFile`.
2. **Gate flip in `check_program_core`:** always bind + check; output =
   `syntactic ++ semantic`, sorted by (file, span) the way the current
   single-source path sorts. KEEP the options-diagnostics gate
   (`check_options`) exactly as-is — tsc's options gate is real.
3. **Checker: skip statements that carry parse errors.** In
   `check_statements` (src/checker/stmts.rs), skip
   `check_statement(s)` when `node_key(s) ∈ parse_error_nodes`
   (still bind them — the binder must see the declarations so that
   references from HEALTHY statements resolve; tsc binds error nodes
   too). Unused-locals reporting (`checkUnusedLocalsAndParameters`
   mirror) must also skip symbols whose ONLY declarations are inside
   error statements — probe tsc on a fixture with an unused var inside
   a broken function to pin the expected behavior first.
4. **Binder hardening.** The binder has never seen error-recovered ASTs
   at scale. Run the full corpus with the gate flipped and fix panics
   before looking at diagnostics at all
   (`./target/release/tsrs --check-batch /tmp/chunk1.txt … 2>&1 | grep -i panic`).

### Verification plan for the pair

1. Branch `parse-gate`. Land A, then B, committing locally per stage.
2. Full classifier run vs the pre-branch golden. Expect a LARGE delta:
   hundreds of files gain semantic diagnostics. Triage NEW_FP clusters
   (there will be some — semantics over recovered ASTs was never
   exercised) by family; each cluster is its own fix-commit on the
   branch. Iterate until 0 NEW_FP. NEW_FNs here should be ~0 by
   construction (we only ADD diagnostics).
3. Expect the biggest single jump in gate-filtered % of any remaining
   workstream. Re-measure absolutes; refresh golden; update the
   conformance-sweep memory.

### Risks / notes

- Cascading FPs inside error-recovered functions are the main risk;
  statement-granularity skipping (step 3) is what keeps them out. If a
  cluster of FPs comes from EXPRESSION-level recovery inside otherwise
  healthy statements, tighten granularity selectively (mark the
  enclosing declaration instead) rather than reverting the gate.
- `.d.ts` files: oracle collects suggestions there too; tsrs skips —
  pre-existing standing FNs, out of scope here.
- BOM fixtures (602) parse with a stripped BOM on both sides — already
  handled; do not touch.
