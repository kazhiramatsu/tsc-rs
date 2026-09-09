# H2.8a A6-28-7: independent class comment endpoints

Independent comment endpoints are qualified on the current train candidate.
The [frozen comparison](../../../../ratchets/h2-8a-one-sided-class-comments-after.v1.json) has 896/996 complete commands exact
twice: all 880 prior positives remain exact, and all 16 selected comment
failures (12 new and four original A28) are repaired. There are
1892 primary executions and no supplemental captures.
All 100 remaining failures preserve the prerequisite's first comparison vectors
or typed boundaries and retain explicit source owners.

The [emitter qualification](../../../../ratchets/h2-8a-one-sided-class-comments-regressions.v1.json) passes all 494 units and 451
contracts. It preserves every earlier test ID and adds the four declared
endpoint-validation and scope tests. The single existing contract type pattern
was adapted before its first build, with the original assertion preserved.
Original A28 is now 396/492 exact twice: all 192 original owned failures are
repaired, all 198 original positives are preserved, and six originally outside
ES2022 cases are exact. The 96 other original outside cases remain open.
This closes A28's selected source owners, including its unchanged minimum 390
target. H2.8a, the global output matrix and B–E remain open.

The following readiness and before record describes the frozen pre-edit state;
its source scope and targets are retained.

Readiness uses the frozen complete repeated before: both native jobs preserve
all 116 exact cases and the same 12 first failure vectors. Production edits
have not started. HEAD is
`dc868373612af16ad299ed08bdec4ece437325dc`; the trusted train base is
`10748f6ee19ec083ce5748224930c5dcfbbd86df`. The semantic prerequisite is the
qualified promoted/export candidate: 764 of 868 complete commands exact twice,
all 100 selected map repairs, and all 490 emitter units / 451 contracts passing.
Original A28 still has four owned wrapper-comment failures. H2.8a and B–E remain
open; this packet makes no new global matrix claim.

The fresh matrix has 128 commands: eight class shapes across ES5/ES2015,
CommonJS/ESNext, set/define fields, and both removeComments values. It retains
strict checking, ordinary libraries, JavaScript and declaration maps, CRLF,
outDir, all write callback details, diagnostics, results and status. TypeScript
mint and independent check each ran every command twice with identical logs.
There are 64 ES5 diagnostic 5107 occurrences, 64 exit-2 cases and 64 exit-0
cases; each produces four writes and two maps.

The first native before has 116 complete commands exact twice and 12 failures
at the first source-map assertion. Every supplemental failure capture has one
extra ` /* class tail */` after the generated constructor declaration, plus a
JavaScript map difference confined to mappings. Declarations are unchanged.
Supplemental captures establish source cause; they do not turn a failed primary
tuple into an exact tuple. The independent repeat preserves every first
vector. The completed two-job count is 488 primary executions and 244 separately counted captures, total 732.

The 12 owned failures are ES5, comments retained, anonymous-expression,
named-expression and nested-expression shapes, across both modules and field
modes. Explicit constructors, derived explicit constructors, declarations,
anonymous default exports and comments-removed variants are adjacent controls.
The unchanged 868-command prerequisite supplies the four original repairs and
protects surrounding class, export, decorator, source-map and initializer work.

## Source and ownership

The source audit has 117 whole TypeScript 6.0.3 functions. The original 110-owner
and 112-owner artifacts remain immutable; the constructor supplement supplies
the actual producer of the disputed trailing position. Whole functions and
hashes, not the count alone, are the authority.

`transformClassLikeDeclarationToExpression` 105178–105220 creates an inner
partial expression with synthesized pos and class.end, and an outer partial
with synthesized pos and skipTrivia(class.pos). Both carry NoComments.
`addConstructor` 105263–105289 ranges the generated FunctionDeclaration to the
explicit constructor, or to the whole class when no constructor exists.
`createDefaultConstructorBody` also ranges its block to the class, but suppresses
that block's comments. The erroneous comment therefore comes from the generated
constructor declaration's trailing phase. The surrounding inner partial must
claim class.end while suppressing its own comments. The existing paired-empty
encoding cannot make that claim.

`transformClassBody` 105221–105249 gives its return-name partial only the closing
brace end, and its ReturnStatement only the closing brace start. TypeScript's
class promotion at 94434–94548 uses the same independent return positions.
These phases belong in the same representation change. Their already-exact
command controls must remain exact; no map encoding or name allocation changes.

`emitLeadingCommentsOfNode` 121007–121032 and
`emitTrailingCommentsOfNode` 121033–121046 use the precise extent predicate
`(pos > 0 || end > 0) && pos !== end`. A missing endpoint never claims that side.
A suppression flag disables comment output while allowing a present endpoint
to claim its container. JsxText claims a side only when that side's suppression
flag is present. The enclosing values are restored before source trailing
comments. Declaration-list end remains an independent scoped value.

Native `add_constructor` has a pre-existing NoLeadingComments flag for absent
constructors, introduced by `6e4516d38`. Upstream does not set that flag. This
packet retains its behavior on the selected controls and does not claim broad
leading-class-comment equivalence or remove the flag. Its multiline leading
comment behavior has a separate source owner. Constructor ranges, parameters,
super handling, captured receivers, lexical environments and body creation are
unchanged premises for this endpoint repair.

| Value | Producer / updater | Rust owner and consumer | Lifetime / invalidation |
| --- | --- | --- | --- |
| source identity | current transform source | CommentRange.source; comment cursor guards | transformed tree; discarded with emit |
| independent endpoints | validated raw projection at the actual class producer | CommentSourceRange; optional start/end views | immutable metadata value |
| active comment extent | exact TS range predicate | CommentSourceRange::has_nonempty_extent; node phases and scope claims | one node phase |
| three container values | existing per-side claim methods | CommentEmissionScope in EmitContext | child value; parent restored by value |
| trivia lookup bound | actual source and validated start | private leading_trivia_end lookup; individual printer consumers | bounded read only |
| inner/outer partial end | ES2015 class expression producer | explicit CommentRange on each fresh partial | wrapper subtree |
| return-name end | ES2015 and TypeScript promotion | explicit CommentRange on fresh partial | return subtree |
| return-statement start | the same two class producers | explicit CommentRange on fresh return | return subtree |
| original/map/comment transport | existing clone/update/merge | EmitMetadata.comment_range, separate original and map identities | transformed tree; no parsed mutation |

## Implementation sequence

Allowed production files are exactly `crates/emitter/src/metadata.rs`, `lib.rs`,
`comment_cursor.rs`, `printer.rs`, `builtins/es2015.rs`, and `builtins.rs`.
Tests may extend `crates/emitter/tests/unit/lib/tests.rs` and
`crates/emitter/tests/unit/comment_scope_predicate/tests.rs`. The test-only
type amendment below also permits active_transform_contract.rs. No parser, scanner,
checker, source-map recorder, generic factory, name allocator, resolver query,
module substitution, transform-flag recomputation or raw node-range change.
Do not infer endpoints from a node kind or map-suppression flag.

**A6-28-7a — represent independent comment positions.** Add public
`CommentSourceRange::{Synthesized, Original(SourceByteRange),
StartOnly(SourceBytePosition), EndOnly(SourceBytePosition)}` in metadata.rs and
re-export it from lib.rs. CommentRange continues to carry TransformSourceId.
Keep `CommentRange::new(source, SourceRange)` as the paired/synthetic constructor;
its range accessor now honestly returns CommentSourceRange. Add a fallible raw
projection that validates each real endpoint independently. Both real endpoints
still require an ordered SourceByteRange; both sentinels mean Synthesized.
Neither Unicode/bounds errors nor inverted paired ranges are swallowed.
SourceRange and SourceMapRange keep their existing mixed-sentinel rejection.

Provide optional start/end views and the exact nonempty-extent predicate.
Paired empty, synthesized, StartOnly(0) and EndOnly(0) are inactive. For a leading
trivia lookup, paired values retain their existing bounded scan; StartOnly may
use source EOF only as a lookup limit. That limit is never stored as an ownership
end. EndOnly and Synthesized have no leading lookup. The public constructor
retains its existing caller-owned source/PositionIndex association; it does not
claim a new arena validation guarantee.

**A6-28-7b — consume each endpoint in its actual phase.** Update the two
CommentEmissionScope range views to combine the extent predicate with the
appropriate optional endpoint. Keep the three-value immutable scope, per-side
JsxText/flag conditions, restoration order and declaration-list writer intact.
Update the obsolete printer explanation that a single synthesized side cannot
occur.

Review each printer consumer individually. End consumers are child trailing
escape checks, deferred trailing comments, ordinary trailing comments, enclosing
trailing guards, cursor-producing trailing comments, positional trailing comments,
comment-range-end cursors and delimited-element trailing comments. Start
consumers are separated/multiline declaration-list comments, intervening trivia,
token/parent-owned resumes, ordinary leading comments, detached prefixes, pending
detached resumes and delimited resumes. Node phases use the nonempty-extent gate;
positional workers retain their separate position-only semantics. Synthetic
comments keep their existing independent phases.

The raw list-element end worker and source-map recording consume SourceRange or
SourceMapRange, and remain unchanged. The partially-emitted boundary worker
compares raw wrapper/child positions and remains unchanged. Generic comment
fallback and virtual-parenthesis owners still project the unchanged paired raw
node encoding. Do not mechanically replace every range match or broaden scope
into token cursor semantics.

**A6-28-7c — attach ES2015 partial wrapper ends.** In
`create_partially_emitted_expression_positioned`, attach explicit EndOnly
comment metadata from node.end or skipTrivia(node.pos), according to its existing
position enum. Keep the paired-empty raw range, NoLeadingSourceMap, NoComments,
child flags, expression shape and generated identity. A synthesized end stays
Synthesized; an actual zero is represented but inactive.

**A6-28-7d — attach the two return endpoints.** In ES2015 transform_class_body
and TypeScript promote_class_declaration_to_iife, attach EndOnly at
closingBrace.end to the fresh return-name partial and StartOnly at
closingBrace.pos to the fresh ReturnStatement. Keep paired close-brace raw map
encodings and every existing map flag. The closing-brace sentinel arithmetic can
yield zero; preserve that typed inactive value. Attach the metadata at distinct
call sites; do not change the meaning of set_close_brace_token_range. No changes
to constructor flags/ranges, helper requests, hoisting, class aliases or names.

**A6-28-7e — extend low-level contracts.** Preserve all old test IDs. Add
`tests::comment_ranges_validate_independent_endpoints_and_preserve_source_identity`
and three IDs under `printer::comment_scope_predicate_tests`:
`one_sided_comment_ranges_claim_present_endpoint_and_inherit_other`,
`one_sided_zero_position_has_no_comment_extent`, and
`jsx_one_sided_ranges_require_present_suppressed_side`.
Cover Unicode scalar boundaries, bounds, paired inversion, source identities,
both endpoint directions, flags naming an absent side, inherited opposite
containers and zero. These four additions make 494 library tests; all 451
contracts retain their identities. No assertion weakening or fixture rewriting.

**A6-28-7f — qualify the shared change.** Compare all 996 complete commands:
868 prior and 128 fresh, without supplemental captures. Preserve all 880 exact
cases (764 prior plus 116 fresh before controls), repair the 12 fresh failures
and four original wrapper-comment failures, and require at least 896 exact
twice. Freeze every remaining first vector and explain any changed vector or
additional exact case through its source owner. No loss of a prior exact case
is permitted. Then run all 494 emitter library tests and 451 contracts on the
same source, verify old test IDs plus only the four declared additions, and
freeze actual exits, source bytes, logs and binary identities.

If these checks pass, original A28 reaches 396 of 492 exact and all its original
owned cases are repaired; its 96 outside cases retain separate owners. This is
still not a global H2.8a replay or an H2.8 completion claim. Hosted acceptance
remains required at the final train candidate. The authorized lightweight
workflow does not claim a historical certificate walk or full developer CI.

## Gate and architecture

The readiness manifest must pin the complete repeated before, every source
owner, exact witness hashes, the qualified prerequisite, this packet and the
current architecture rows. Before production, refresh the new 128-case test
registration and packet/index references in the parent readiness manifests,
retaining all old authority bytes. After the candidate type inventory changes,
refresh the affected architecture references explicitly before running the
after matrix. No silent pin refresh or baseline replacement.

E-METADATA-BASE, E-POSITIONS, E-COMMENTS-G and E-COMMENT-SCOPE-H are
modified-requalify for this comment-domain extension. Their existing typed
source/map domains and immutable context remain current premises. E-MAPS keeps
its dated dormant compatibility statement; fresh complete maps are the evidence.
E-ORDER-H and E-NAMES-H supply active source premises without a blanket parity
claim. Keep current transform order, full transform-flag recomputation points,
parsed/current/synthetic provenance, lexical receivers, expression contexts and
generated binding scopes unchanged.

Required architecture rows: E-PRINTER-BASE, E-POSITIONS, E-COMMENTS-G,
E-COMMENT-SCOPE-H, E-RESOLVER-BASE, E-ARENA, E-METADATA-BASE, E-CONTEXT,
E-ENTRY, E-PROTOCOL, E-ORDER-G, E-ORDER-H, E-MAPS, E-NAMES-BASE,
E-NAMES-CLASS-G, E-NAMES-H, E-HELPERS-BASE, E-HELPERS-PROVENANCE-G,
E-METADATA-G. The six production paths have one writer. Heavy commands run
with CARGO_BUILD_JOBS=2 and background priority; no canonical source mutation
while observations or regression jobs are live. A new owner, required file or
data-model change must be documented and rechecked before production continues.


Before the first candidate build, the repository-wide consumer audit found one
existing contract matching CommentRange::range against SourceRange::Original.
Update only that pattern to CommentSourceRange::Original in
`crates/emitter/tests/integration/active_transform_contract.rs`, preserving
`standard_decorator_named_declaration_has_one_outer_leading_comment_owner` and
every assertion. This is the same paired-range property in the now explicit
comment domain. The file is byte-identical to HEAD and is archived before its
edit; no earlier prelaunch pin is claimed for it. Six production files, four
new unit IDs, 494/451 test counts and the 996/minimum896 output target remain
unchanged. No failing build or executed contract result is claimed at discovery.
