# M1b: parser + recovery — steps

Parent design: syntax-and-binder.md §2 (node creation/error flag,
Pratt loop, list recovery, statement dispatch); core-interfaces.md §1
(Node contract). tsc source region: `parseSourceFile` (29014) through
the JSX/type grammars. Prerequisite: M1a gate green.

This milestone is THE foundation (greenfield §12): per-node
parse-error flags and tsc-exact recovery are what every downstream
milestone prices in. The gate is syntactic-diagnostic parity ≥ 99.5%
corpus-wide, measured continuously from stage 2.3 onward via
`cargo xtask conformance --syntactic-only` (T0 restricted to the
oracle's `getSyntacticDiagnostics` set).

## Stage 2.0: AST shape [M]

`NodeData` per core-interfaces §1: one arena of
`Node { kind, flags, pos, end, parent, data }`, `NodeData` variants
per syntax kind carrying child `NodeId`s. Generate the
SyntaxKind↔variant mapping table (xtask codegen extension) so match
arms are mechanical. `NodeArray { nodes, pos, end, has_trailing_comma }`
is a first-class type — trailing-comma presence is observable in
several grammar diagnostics.

Commit: `m1 2.0: AST arena + NodeData`.

## Stage 2.1: parser infrastructure [M]

Port, in this order (all in `crates/syntax/src/parser/`):

1. `parseErrorAtPosition` (29467): the SAME-START DEDUP — no two parse
   errors at one start position — and
   `parseErrorBeforeNextFinishedNode = true`.
2. `finishNode` (29778): stamps pos/end; transfers
   `parseErrorBeforeNextFinishedNode` into `NodeFlags.ThisNodeHasError`
   and clears it; ORs child aggregate into
   `ThisNodeOrAnySubNodesHasError` — this pair of flags IS the
   parse-error gate the checker consumes (containsParseError), and
   porting it here is what makes per-node semantic gating free later.
3. `parseExpected` / `parseOptional` / `parseTokenNode` /
   `createMissingNode` — a missing required element becomes a REAL
   zero-width node so the checker can errorType it.
4. Context flags threading: `doInsideOfContext`/`doOutsideOfContext`
   save/restore for `DisallowInContext`, `YieldContext`,
   `AwaitContext`, `DecoratorContext` (syntax-and-binder §2.2 note) —
   these are NodeFlags bits stamped onto nodes created inside them.
5. Parser-level `lookAhead`/`tryParse` on top of the scanner's
   speculation, ALSO saving parser state (context flags, error count,
   `parseErrorBeforeNextFinishedNode`).

Commit: `m1 2.1: parser infrastructure (error flag, missing nodes, contexts)`.

## Stage 2.2: the list engine [M]

Port BEFORE any grammar production, because every production uses it
(syntax-and-binder §2.3 has the full annotated skeleton):

- `parseList` (30169) and `parseDelimitedList` (30428) verbatim,
  including the no-progress force-advance guard and trailing-comma
  bookkeeping.
- `isListTerminator` (30097): the full per-ParsingContext terminator
  table. Port every arm; each is a recovery boundary.
- `abortParsingListOrMoveToNextToken` (30356) +
  `parsingContextErrors`: the per-context error messages and the
  isInSomeParsingContext escalation that aborts inner lists when an
  OUTER context could consume the token.
- The `ParsingContext` enum (M0 codegen, const-inlined).

Verify: unit fixtures with deliberately broken lists
(`[a, {b: , c]`, unterminated parameter lists) — expected diagnostics
oracle-probed.

Commit: `m1 2.2: list parsing + recovery engine`.

## Stage 2.3: source file + statements [M]

`parseSourceFile` (29014) → statement list → `parseStatement` (33528)
dispatch, porting the arms in tsc's switch order. Productions not yet
ported return a missing-statement via the standard error path — the
parity metric then climbs stage by stage instead of jumping at the
end. Start the continuous gate now:

```sh
cargo xtask conformance --syntactic-only    # record the rate in the commit body
```

Port with the statements: variable statements/declaration lists
(let/const/using flags → NodeFlags), block, if/do/while/for/for-in/
for-of, switch (clause lists via the list engine), try/catch/finally,
labeled, break/continue, throw, return, debugger, empty.

Commit: `m1 2.3: statement dispatch (+rate in body)`.

## Stage 2.4: expressions [M]

In dependency order, each its own commit if large:

1. `parsePrimaryExpression`: literals, identifiers, this/super,
   parenthesized, array/object literals (list engine), function/class
   expressions (bodies parse but their productions may still be
   partial), template expressions (reScanTemplateToken),
   regex via `reScanSlashToken`, `new` (incl. `new.target` meta
   property — parse it as a REAL MetaProperty node).
2. Member/call chains: property/element access, optional chaining,
   call arguments, tagged templates, type arguments in expressions
   (`lookAhead` disambiguation vs relational `<`).
3. Unary/update expressions, `typeof/void/delete/await/yield`
   (context-flag gated).
4. The Pratt loop `parseBinaryExpressionOrHigher` (32107) — port from
   the skeleton in syntax-and-binder §2.2 with its three subtleties:
   `reScanGreaterToken` at every iteration head, `**`
   right-associativity via the >= comparison, and the
   `as`/`satisfies` newline (ASI) break.
5. `parseAssignmentExpressionOrHigher`: arrow-function speculation
   (`tryParse` the parenthesized-arrow head, the async-arrow
   lookahead), conditional `?:`, and THE recovery rule:
   an assignment operator only binds when
   `isLeftHandSideExpression(left)` — otherwise the `=` is left for
   the outer context to report. This single rule defines the
   downstream recovery profile of every malformed-assignment fixture.

Commit(s): `m1 2.4a..2.4e: expressions (+rate)`.

## Stage 2.5: types grammar [M]

`parseType` and its tree: union/intersection lists, operators
(keyof/typeof/readonly/infer), array/tuple (named/optional/rest
members), function/constructor types, object type literals (list
engine, TypeMembers context), conditional types (`extends` with the
DisallowConditionalTypes context flag), mapped types, template literal
types, import types, type predicates, parenthesized. Then type
annotations get wired into the stage-2.3/2.4 productions that
stubbed them.

Commit: `m1 2.5: type grammar (+rate)`.

## Stage 2.6: declarations + modifiers [M]

`parseDeclaration`: modifier parsing with lookahead (`async`,
`declare`, accessibility, `abstract`, `override`, `accessor`,
decorators via `DecoratorContext`), function declarations (overload
bodyless forms), classes (heritage clauses, member kinds incl.
accessors, index signatures, static blocks, private `#` names),
interfaces, type aliases, enums (`const enum`), namespaces/modules
(dotted names desugar to NESTED module declarations — port
`parseModuleOrNamespaceDeclaration`'s recursion), import/export forms
(equals, namespace, named, type-only, assertions), ambient contexts.

Commit: `m1 2.6: declarations (+rate)`.

## Stage 2.7: JSX [M]

`.tsx` only: elements/fragments/self-closing, attributes + spread,
expression children, the JSX-vs-type-assertion variant split. The
scanner side landed in M1a stage 1.6.

Commit: `m1 2.7: JSX (+rate)`.

## Final gate

```sh
cargo xtask conformance --syntactic-only   # expect: ≥ 99.5%
cargo xtask invariants --suite prefix-determinism
cargo xtask ledger check
```

Triage of the residual <0.5%: every remaining mismatch file gets a
one-line classification in `docs/NOTES-m1.md` (recovery-order
difference vs unported production vs oracle quirk). Nothing may be
silenced.

## Expected failure modes

| Symptom | Diagnosis | Fix |
|---|---|---|
| Error counts double at one position | same-start dedup missing/bypassed | ALL parse errors flow through parseErrorAtPosition |
| Downstream nodes all carry ThisNodeHasError | flag not cleared after transfer in finishNode | finishNode must consume parseErrorBeforeNextFinishedNode |
| `a < b > c` parses as type args | missing lookAhead disambiguation in call-expression type args | Port the tsc lookahead, do not approximate with local heuristics |
| Recovery consumes the whole rest of file on one bad token | abortParsingList not consulting OUTER contexts | isInSomeParsingContext must walk every active context bit |
| Rate stuck below gate with recovery-shaped diffs | recovery ported "approximately" | Re-read the cited tsc lines; recovery is a port target, not a behavior to tune (syntax-and-binder §2.3) |
