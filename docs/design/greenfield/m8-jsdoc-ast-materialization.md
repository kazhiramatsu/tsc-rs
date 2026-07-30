# M8: JSDoc AST materialization

Status: approved M8 design correction; implementation active.

This page owns the JSDoc representation used by the parser, binder, and
checker during M8. It refines the
[M8 execution contract](m8-execution-and-close.md) after repeated
`checkjs-jsdoc` probes reached the same model ceiling.

The target remains TypeScript 6.0.3. This is not a new JSDoc semantics
design: the implementation ports tsc's comment-to-AST, attachment, binding,
and checking paths.

## Why the representation must change

The Rust syntax schema already contains most JSDoc `SyntaxKind` and payload
names, and the ordinary type parser already recognizes several JSDoc type
forms. However, source-file parsing does not currently:

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
belongs to the nested `JSDocPropertyTag`. Neither identity exists in the
current arena.

This is a structural omission, not another relation-reporting exception.
The source projections remain useful as oracle probes while this migration
lands, but they are not the final implementation.

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
the normal port ledger. A bounded first landing may cover only the tag
families consumed by its owner cluster, but it must use this common AST
path. It may not introduce another independent comment scanner in the
checker.

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

### Missing schema fields

The generated schema must materialize the fields required by real JSDoc
declarations:

- `JSDocTypeLiteral.jsDocPropertyTags` as a child array and
  `isArrayType`;
- `JSDocPropertyTag` / `JSDocParameterTag` `isBracketed` and
  `isNameFirst`;
- the effective `name` of JSDoc typedef/callback declarations where tsc
  stores it separately from `fullName`;
- JSDoc signature fields when their owner cluster is activated.

The generator and `schema-audit` manifest remain the source of truth. In
particular, `jsDocPropertyTags` must be taught to the child-table extractor;
it must not be hidden as an unchecked runtime side table.

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

Source-file parsing owns JSDoc materialization. The implementation follows
these rules:

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
5. Build nested `JSDocTypeLiteral` property arrays during comment parsing;
   root tag arrays must not also own the consumed property tags.
6. Keep parser-created JSDoc diagnostics in the parser-owned diagnostic
   channel. Checker projections must not recreate grammar diagnostics once
   their AST producer is active.

The migration may port tag parsers in dependency order, starting with
typedef/property/satisfies. Unknown or not-yet-activated tags may use the
ordinary `JSDocTag` shape only when tsc does so; a tag whose distinct kind
changes binding or checking stays explicitly unsupported until its parser
lands.

## Binder path

The binder mirrors tsc's separate traversal:

- `bindChildren(node)` binds ordinary children, then `bindJSDoc(node)`;
- JS files bind attached JSDoc nodes normally; TS files only establish
  their parent tree;
- `JSDocTypeLiteral` is an anonymous type container;
- nested `JSDocPropertyTag` nodes declare property symbols in that
  container, including optional flags from brackets or optional types;
- typedef/callback/enum tags enter the delayed type-alias queue and are
  declared in the enclosing host scope using their real declaration node;
- JSDoc imports use the delayed import path and the effective host scope.

The first typedef/property/satisfies slice must produce a normal
`TYPE_ALIAS` symbol whose declaration is the `JSDocTypedefTag`, and normal
property symbols whose declarations are the `JSDocPropertyTag` nodes. No
checker-created stand-in symbol is accepted.

## Checker migration

Checker consumers migrate vertically:

1. locate tags through host attachments and the tsc JSDoc host walk;
2. obtain types through normal `getTypeFromTypeNode` and name resolution;
3. dispatch through the existing tsc checker worker;
4. delete the equivalent source-text projection and its diagnostic
   fabrication;
5. retain focused oracle tests that pin the removed projection's behavior.

For `@satisfies`, `checkParenthesizedExpression` calls
`checkSatisfiesExpressionWorker(expression, tag.typeExpression.type)`.
That worker's ordinary reporting relation owns 1360, its nested chain, and
related declarations. The checker does not transplant a relation chain onto
an explicit source range.

Source projections for unrelated JSDoc owners may coexist temporarily, but
each must be skipped once its AST producer is active. The migration ends
only when all JSDoc comment scanners outside the syntax layer are removed or
have a reviewed non-AST purpose.

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

## Landing and acceptance order

The architecture lands in dependency order:

1. schema/header attachment, parent finalization, and syntax inspection
   tests;
2. comment-range attachment plus the typedef/property/satisfies parser
   slice;
3. binder anonymous-type/property/delayed-alias slice;
4. checker host lookup and normal `@satisfies` dispatch;
5. deletion of the matching source projection;
6. expansion to the next frozen JSDoc owner cluster.

The first vertical slice is accepted only when:

- AST tests match tsc kinds, parents, arrays, flags, and byte spans;
- symbol tests show the typedef and properties bound to their real JSDoc
  declarations;
- checked-JS satisfies fixtures preserve T0-T3 and the exact
  1360 → 2741 + related 2728 shape;
- LF, CRLF, inline, non-BMP, optional-property, and multiple-comment
  canaries pass;
- accepted identities lost remain zero and all-corpus false positives
  remain zero;
- focused performance evidence stays within the bounds above; and
- the full local CI and all required hosted lanes pass.

This is an M8 prerequisite/consumer chain, not permission to combine
unrelated diagnostic families into one PR. Infrastructure-only landings name
the immediately consuming `checkjs-jsdoc` cluster and carry direct AST and
symbol pins.
