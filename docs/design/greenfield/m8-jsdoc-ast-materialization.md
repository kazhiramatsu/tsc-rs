# M8: complete JSDoc subsystem port

Status: approved M8 design correction; dependency-complete implementation
landed. Formal corpus-tier and T4 acceptance remain governed by the M8
execution contract.

This page owns the complete JSDoc subsystem used by the parser, binder, and
checker during M8. It refines the
[M8 execution contract](m8-execution-and-close.md) after repeated
`checkjs-jsdoc` probes reached the same model ceiling.

The target remains TypeScript 6.0.3. This is not a new JSDoc semantics
design: the implementation ports tsc's comment-to-AST, attachment, binding,
and checking paths. A bounded JSDoc owner slice is no longer an accepted
landing unit. Work may be committed in dependency order on one branch, but
the subsystem is accepted only after its parser, AST, binder, checker, and
diagnostic dependencies are coherent across the full supported corpus.

## Landed result

The complete scanner/parser, arena-node, attachment, binder, checker, and
ordinary diagnostic-control-flow chain has landed. The follow-up scope review
returned every resolved historical `jsdoc-semantics` exclusion through A2
tombstones instead of redefining the supported denominator. Superseded
checker-side comment projections and partial-activation guards are not the
production semantics.

This result closes the representation correction described below. It does
not by itself claim that the separate A1 tier transition or A3 rendered-output
transition has occurred; those remain executable corpus-wide gates in the
[M8 execution contract](m8-execution-and-close.md).

## Why the representation had to change

At the point of this design correction, the Rust syntax schema contained
most JSDoc `SyntaxKind` and payload names, and the ordinary type parser
already recognized several JSDoc type forms. Source-file parsing did not:

- materialize `/** ... */` comments as arena nodes;
- attach `JSDoc[]` to their ordinary host node;
- carry `JSDocTypeLiteral.jsDocPropertyTags` and its declaration identity;
- run `bindJSDoc`, delayed typedef binding, or JSDoc import binding; or
- let ordinary checker relations report through real JSDoc declaration
  nodes.

Checker-side source projections have therefore had to rescan comment text,
fabricate transient types, reproduce tag spans, and manually reconstruct
relation descendants or related information. The checked-JS `@satisfies`
1360 → 2741 + related 2728 probe demonstrates the ceiling: the root
diagnostic belongs to the `JSDocSatisfiesTag`, while the related declaration
belongs to the nested `JSDocPropertyTag`. Neither identity existed in the
arena before this correction.

The first materialization experiment also established a stronger constraint:
activating `@type`, `@param`, typedef, property, or `@satisfies` nodes before
their `@template`, `@import`, signature, class, and host-resolution
dependencies are present changes real symbol and relation behavior. Local
activation guards merely hide that incomplete model. This is a structural
subsystem omission, not another relation-reporting exception.

Source projections may remain temporarily as differential oracle probes
while this branch is developed, but they are not an implementation fallback.
No semantic JSDoc source scanner, transient declaration, local activation
guard, or hand-built diagnostic chain remains at acceptance.

## TypeScript anchors

The authoritative TypeScript 6.0.3 paths are:

- scanner `PrecedingJSDocComment`: `_tsc.js:9543-9576`;
- `getJSDocCommentRanges`: `_tsc.js:14179`;
- parser `withJSDoc`: `_tsc.js:29235-29248`;
- `JSDocParser.parseJSDocComment` and
  `parseJSDocCommentWorker`: `_tsc.js:34820-35820`;
- parent fixup for attached JSDoc: `_tsc.js:19045-19090`;
- `bindJSDoc`, `bindJSDocTypeAlias`, delayed typedef binding, and JSDoc
  imports: `_tsc.js:42456-42505`, `42843-42976`, `43999-44097`, and
  `44226-44527`;
- `checkSatisfiesExpressionWorker`: `_tsc.js:78051-78060`;
- JSDoc satisfies dispatch: `_tsc.js:81000-81010`.

Every landed parser/binder/checker body records its exact span and hash in
the normal port ledger. The implementation may be translated and tested in
dependency groups, but no group is treated as an independently complete
JSDoc semantic slice and no new independent comment scanner is introduced
outside the syntax layer.

The port boundary includes:

- scanner comment classification and JSDoc comment ranges;
- every JSDoc node kind and observable field emitted by tsc 6.0.3;
- comment text/link nodes, tag parsing, JSDoc type grammar, diagnostics,
  attachment, parents, flags, and spans;
- every binder JSDoc dispatch, delayed declaration, scope, namespace,
  import, template, signature, overload, class, and property path;
- checker JSDoc host/tag utilities, grammar checks, type construction,
  signature selection, visibility/modifier behavior, name resolution,
  unused/reference handling, and ordinary diagnostic control flow; and
- removal of all superseded semantic source-text projections.

## Arena model

### Host attachment

Every arena `Node` gains an optional `js_doc: NodeArrayId`, equivalent to
tsc's internal `node.jsDoc`. It is a header-owned attachment, not a field
repeated across every host payload.

The attachment is deliberately not part of ordinary `for_each_child`.
tsc also traverses it through dedicated JSDoc paths:

- parser parent finalization sets each `JSDoc.parent` to its host and then
  parents the JSDoc subtree;
- binder `bindChildren` calls `bindJSDoc` after ordinary children;
- generic syntax walks remain unchanged unless they explicitly request
  attached documentation.

This prevents existing node visitors from silently doubling their work and
preserves tsc's observable bind order.

### Complete schema surface

The generated schema materializes every observable JSDoc kind and field
emitted by tsc 6.0.3, including fields not visited by ordinary
`forEachChild`. At minimum this includes:

- fieldless `JSDocAllType` and `JSDocUnknownType`;
- `JSDocNamepathType.type` and `JSDocText.text`;
- `JSDocTypeLiteral.jsDocPropertyTags` and `isArrayType`;
- `JSDocPropertyTag` / `JSDocParameterTag` `isBracketed` and
  `isNameFirst`;
- `JSDocFunctionType.name` and `typeParameters`;
- `JSDocLink`, `JSDocLinkCode`, and `JSDocLinkPlain` text;
- `JSDocNonNullableType` and `JSDocNullableType` postfix state;
- the effective names of typedef, callback, enum, and related declarations;
  and
- all signature, template, import, comment, and tag payloads.

The generator and `schema-audit` manifest remain the source of truth. In
particular, `jsDocPropertyTags` must be taught to the child-table extractor;
it must not be hidden as an unchecked runtime side table.

Stored payload and traversal are separate contracts. A tsc field omitted
from `forEachChild`, such as a compatibility or shared-name field, is still
represented when it is observable; the generated traversal follows tsc's
dedicated ordering instead of dropping the field from the arena model.

### Identity and spans

Each parsed comment, tag, tag name, type expression, and property
declaration receives a normal program-unique `NodeId`. Byte positions are
the parser's source positions and are converted to UTF-16 only at diagnostic
creation.

The following are acceptance invariants:

- `JSDocSatisfiesTag` errors select its `tagName` span through
  `getErrorSpanForNode`;
- a `JSDocPropertyTag` declaration retains the entire tsc node span,
  including its terminating line break or the trivia up to the next tag;
- CRLF, non-BMP text before a tag, adjacent comments, inline casts, and
  trailing-comment host cases preserve exact positions;
- parent chains are
  `host -> JSDoc -> tag -> type/declaration children`, except for tsc's
  explicit shared-name fields.

## Parser path

Source-file parsing owns the complete JSDoc materialization. The
implementation follows these rules:

1. Detect JSDoc in the scanner/leading-comment range used by the ordinary
   parser; do not scan the whole file once per checker operation.
   Files without `/**` take a fast path that records no attachment
   candidates and allocates no JSDoc nodes.
2. Parse each attachment while the source parser and its arena are alive, so
   JSDoc type nodes are allocated in the same arena and id range.
3. Parse under `NodeFlags::JSDoc`, restoring scanner, context, and diagnostic
   state exactly as `JSDocParser.parseJSDocComment` does.
4. Attach only to tsc `canHaveJSDoc` hosts and preserve the special
   leading/trailing comment-range rules.
5. Port the full JSDoc type grammar, tag-name dispatch, tag-specific
   parsers, comment-text/link parsing, duplicate/conflict rules, and
   recovery control flow. A distinct tsc tag kind is never downgraded to a
   generic `JSDocTag`.
6. Build nested `JSDocTypeLiteral` property arrays during comment parsing;
   root tag arrays must not also own the consumed property tags.
7. Keep parser-created JSDoc diagnostics in the parser-owned diagnostic
   channel. Checker projections must not recreate grammar diagnostics once
   their AST producer is active.
8. Preserve tsc scanner restoration, lookahead, mode, newline, whitespace,
   asterisk-margin, escaping, and malformed-comment recovery exactly enough
   for the parser diagnostic and AST oracles to agree.

Translation can proceed in dependency order, but an unported distinct tag is
a branch-local implementation failure rather than an accepted generic-tag
fallback.

## Binder path

The binder mirrors tsc's complete separate traversal:

- `bindChildren(node)` binds ordinary children, then `bindJSDoc(node)`;
- JS files bind attached JSDoc nodes normally; TS files only establish
  their parent tree;
- `JSDocTypeLiteral` is an anonymous type container;
- nested `JSDocPropertyTag` nodes declare property symbols in that
  container, including optional flags from brackets or optional types;
- typedef/callback/enum tags enter the delayed type-alias queue and are
  declared in the enclosing host scope using their real declaration node;
- JSDoc imports use the delayed import path and the effective host scope.
- parameter tags declare only in the exact signature/type-literal parents
  used by tsc; a root host `@param` does not redeclare the ordinary
  parameter;
- template parameters, callback/signature/overload nodes, class and
  constructor tags, namespace/module qualification, export-assignment
  hosts, and delayed imports use the same scope and bind order as tsc; and
- duplicate declaration, meaning, and symbol-flag behavior is preserved
  rather than repaired later in the checker.

Typedef/property/satisfies behavior must therefore be validated together
with template/import/signature and host-scope behavior. Every declaration
uses its real JSDoc node; no checker-created stand-in symbol is accepted.

## Checker migration

Checker consumers migrate as one dependency-complete subsystem:

1. locate tags through host attachments and the tsc JSDoc host walk;
2. obtain types through normal `getTypeFromTypeNode` and name resolution;
3. dispatch through the existing tsc checker worker;
4. delete the equivalent source-text projection, local activation guard,
   and diagnostic fabrication;
5. retain focused oracle tests that pin the removed projection's behavior;
   and
6. continue until every semantic JSDoc consumer uses arena nodes or has a
   reviewed, demonstrably non-semantic reason to read raw comment text.

For `@satisfies`, `checkParenthesizedExpression` calls
`checkSatisfiesExpressionWorker(expression, tag.typeExpression.type)`.
That worker's ordinary reporting relation owns 1360, its nested chain, and
related declarations. The checker does not transplant a relation chain onto
an explicit source range.

Source projections may coexist only as branch-local comparison probes while
the full port is incomplete. They must not choose production semantics,
mask missing AST dependencies, or survive the final acceptance. The
temporary contextual `@template` deferral introduced by the initial
materialization experiment is explicitly removed when template binding
lands; it is not part of the design.

### Relation and member-display invariants

JSDoc relation failures use the same reporting path as ordinary tsc
relations. At each failure level that renders a type pair, the source is
read-normalized and the target is write-normalized immediately before
display. This applies independently to the head and every nested message;
checker-side range replacement or post-hoc chain reconstruction cannot
substitute for it.

The deleted checked-JS memberless/symbol-carrying empty-resolution admission
heuristic is not a JSDoc fallback. JSDoc may affect a symbol only through its
real parser, binder, and checker declarations. In particular, the
plain-JS nested-object to TypeScript-consumer canary keeps the vendored
compiler's 2339 when the member is absent; no open-ended-object display
exception suppresses it.

## Performance contract

JSDoc must not reintroduce the high-CPU node-visit behavior removed earlier
in M8.

- Source comments are parsed once during source parsing, not once per
  checker query or diagnostic family.
- Ordinary `for_each_child` does not include host attachments.
- Parser node completion records only eligible hosts when the source contains
  JSDoc. Attachment discovery consumes that bounded candidate list rather
  than walking the finished AST; identical trivia starts are cached within
  the parse. Only the rare top-level-await reparse performs a reachability
  filter to discard superseded parser nodes.
- Binder work is proportional to the number of attached JSDoc nodes and
  runs once per source file.
- Checker tag lookup uses attachment arrays and bounded host-parent walks;
  no full-source `find("/**")` loop is allowed.
- No new whole-corpus Node AST visit is added to CI. Existing
  content-addressed M8 evidence rules remain unchanged.

Focused benchmarks record parse time, bind time, node count, and peak memory
for a no-JSDoc control, a JSDoc-heavy JS file, and the current checked-JS
owner fixtures. A semantic slice must not repeatedly run the full
conformance or B2 Node sweep during editing.

Rust verification uses `CARGO_BUILD_JOBS=2`, no more than two test threads,
and batched focused invocations. The complete corpus and local CI run once
after focused closure; increasing parallelism is not a substitute for
removing repeated parser, binder, or checker traversal.

## Implementation and acceptance order

The branch is implemented in dependency order:

1. complete schema/header attachment and a dedicated tsc JSDoc AST oracle;
2. scanner ranges, full comment/type/tag parser, diagnostics, parent
   finalization, and syntax parity tests;
3. complete binder dispatch, declarations, delayed work, imports, template
   scopes, signatures, overloads, and host qualification;
4. checker host/tag utilities and type/signature/name-resolution consumers;
5. replacement and deletion of all semantic source projections and local
   guards;
6. focused JSDoc corpus closure, then one all-corpus measurement;
7. exact scope/ratchet/evidence retirement, performance evidence, and final
   CI.

These are implementation checkpoints, not separately accepted semantic
slices. The complete JSDoc port is accepted only when:

- a dedicated AST oracle matches tsc kinds, all observable fields, parents,
  arrays, flags, byte spans, comment text/link structure, and parser
  diagnostics across valid and malformed JSDoc;
- symbol oracles match typedef, enum, callback, property, parameter,
  template, import, signature, overload, class, namespace, and host
  declarations using their real JSDoc nodes;
- checker paths use normal tsc control flow for JSDoc types, signatures,
  relations, modifiers, visibility, references, and diagnostics;
- the checked-JS relation fixtures preserve the exact
  1360 → 2741 + related 2728 shape and all other supported T0-T3/T4 shapes;
- LF, CRLF, inline, non-BMP, optional/rest/defaulted names, multiple
  comments, links, casts, malformed tags, duplicate tags, and attachment
  edge cases pass;
- no production semantic JSDoc source scanner, transient declaration,
  partial-activation guard, or hand-built replacement diagnostic remains;
- every one of the existing `jsdoc-semantics` scope exclusions is reviewed
  against the completed subsystem and retired when its true dependency is
  now present; no exclusion is silently retained as a JSDoc implementation
  escape;
- accepted identities lost remain zero, supported JSDoc tier residuals are
  exact, and all-corpus false positives remain zero;
- focused performance evidence stays within the bounds above; and
- the full local CI and all required hosted lanes pass.

The complete subsystem is one approved prerequisite/consumer chain for the
`checkjs-jsdoc` owner family. It does not authorize unrelated non-JSDoc
diagnostic work in the same PR.

## Scope boundary

This port closes only the TypeScript 6.0.3 batch-diagnostics JSDoc surface.
JavaScript or declaration emission, LSP/watch/incremental operation, and a
public `TypeChecker` API remain separate design tracks with their own
compatibility and completion contracts.
