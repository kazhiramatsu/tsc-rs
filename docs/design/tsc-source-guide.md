# Reading the vendored tsc source (oracle/node_modules/typescript/lib/_tsc.js)

The vendored build is **tsc 6.0.3**, a single bundled file (~8MB). It
is the source of truth for every semantics question the oracle probe
can't settle structurally. This guide makes it navigable for an agent
that has never opened it.

## Techniques

1. **Find a function**: `grep -n "function <name>" _tsc.js`. Checker
   internals are nested inside `createTypeChecker`, so many appear as
   `function name2(` with a numeric suffix (minifier dedup) — grep
   without the suffix first, then with `name2`.
2. **Decode numeric flags**: every flag use carries an inline comment:
   `t.flags & 1024 /* StringLiteral */`. TRUST THE COMMENT, never a
   memorized table. To decode a bare mask with no comment, find the
   enum: `grep -n '"StringLiteral"' _tsc.js | head` lands in the
   TypeFlags table region.
3. **Minified parameter names**: `source2`, `target2`, `checkMode`
   etc. — suffixed duplicates of outer names; read them positionally.
4. **When the code is ambiguous, probe.** Reading + probing together
   is the house method; reading alone produced two wrong derivations
   in the operator sweep (fx2/fx4 analysis) that only probes resolved.
5. Comments like `/* Ambient */`, `/* Both */` on ENUM-typed arguments
   follow the same trust rule.

## Verified function index (line numbers valid for THIS vendored build)

Re-grep if the file is ever re-vendored; treat drift > ±50 lines as
"re-verify everything you cite".

| Function | Line | Used for |
|---|---|---|
| `containsParseError` | 12854 | parse-error gate |
| `isLeftHandSideExpressionKind` | 12210 | `=` recovery |
| `parseAssignmentExpressionOrHigher` (LHS gate inside) | 31788 | `=` recovery |
| `getIntersectionType` | 61789 | intersection normalization (incl. `{}`-nullish folding, undefined/null union pull-out, cross-product) |
| `intersectUnionsOfPrimitiveTypes` | 61737 | primitive-union distribution |
| `compareSignaturesRelated` | 64487 | signature relation core (strictVariance, checkMode bits) |
| `isSimpleTypeRelatedTo` | 64733 | simple relation rules (incl. comparable reversed use) |
| `isTypeRelatedTo` | ~64762 | comparable reversed-simple entry |
| `unionOrIntersectionRelatedTo` | ~65414 | union/intersection relation arms (comparable SOME, primitive-target constraint collapse) |
| `someTypeRelatedToType` | 65564 | union-source SOME |
| `signaturesRelatedTo` | 66934 | overload-list matching, eraseGenerics rule |
| `signatureRelatedTo` | 67067 | erase entry per pair |
| `getBaseTypeOfLiteralTypeForComparison` | 67762 | relational-operator basing |
| `getAssignmentReducedType` | 69675 | assignment narrowing (union declared) |
| `getTypeAtFlowAssignment` | ~70510 | assignment narrowing (all arms incl. auto) |
| `getNarrowableTypeForReference` | 71640 | (narrowing gate — NOT the assignment gate; that's above) |
| `instantiateSignatureInContextOf` | 75910 | generic-source signature inference |
| `checkAssertionWorker` / `checkAssertionDeferred` | 77908 / 77939 | cast (2352) comparable usage |
| `maybeTypeOfKindConsideringBaseConstraint` | 79507 | symbol-operand (2469) checks |
| `checkForDisallowedESSymbolOperand` | 80300 | 2469 |
| relational-operator arm (`case 30 LessThanToken`...) | ~80184 | 2365 formula |

## checkMode bits seen in compareSignaturesRelated (from inline comments)

`1 BivariantCallback`, `2 StrictCallback`, `3 Callback` (mask),
`4 IgnoreReturnTypes`, `8 StrictArity`, `16 StrictTopSignature`.
Relation→checkMode: subtype=16, strictSubtype=24, else 0.

## Relation objects

`assignableRelation`, `comparableRelation`, `subtypeRelation`,
`strictSubtypeRelation`, `identityRelation` — passed as the `relation`
closure variable inside `checkTypeRelatedTo`. Grep
`relation === comparableRelation` to enumerate ALL comparable special
cases (that grep was how the operator sweep found the collapse rule).

## Key structural facts confirmed this sweep

- Comparable relation: reversed `isSimpleTypeRelatedTo(target, source)`
  at EVERY isRelatedTo level; union SOURCE = someTypeRelatedToType;
  intersection SOURCE with Primitive target substitutes instantiable
  members with base constraints and, if the re-intersection collapses
  to a non-intersection, DECIDES bidirectionally right there.
- signaturesRelatedTo: 1-vs-1 → `eraseGenerics = relation ===
  comparableRelation` and generic SOURCE instantiates in context;
  N-vs-M → `signatureRelatedTo(..., /*erase*/ true, ...)`
  unconditionally.
- strictVariance = `!(checkMode & Callback) && strictFunctionTypes &&
  target.declaration.kind ∉ {MethodDeclaration, MethodSignature,
  Constructor}` — the TARGET's declaration kind.
- getTypeAtFlowAssignment: Compound → base-of-antecedent; auto →
  widened assigned; union declared → getAssignmentReducedType;
  otherwise DECLARED (no RHS adoption).
- getIntersectionType members map (`typeMembershipMap`) dedupes by type
  IDENTITY — structurally equal but distinct anonymous types do NOT
  dedupe (why tsrs can't reproduce fx2/fx4; stall-playbook §2.3).
- Diagnostic message constants: tsrs's generated
  `Diagnostics_*`-equivalents live in `target/*/build/tsrs-*/out/diagnostics_gen.rs`
  (find with `find target -name diagnostics_gen.rs`); grep there to
  check whether a tsc message exists in tsrs before wiring a new path.
