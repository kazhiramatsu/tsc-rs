# H2.8a A6-16: direct JSDoc guard on parenthesized checking

The40-command TS observation is minted and byte checked. Two independent
complete native before jobs are frozen. Sole production path: crates/checker/src/expr.rs,
check_parenthesized_expression. Runtime base is completed A6-15 `9c0cc016092eaade9be34aaf83faa78db2e1a735`.

The frozen A6-14 original769 checkpoint includes jsdocTypeCast.ts with identical
writes but missing both TS2322 diagnostics and wrong exit0 vs2. The two relevant
parentheses inherit declaration @type tags through the existing binder helper,
so isJSDocTypeAssertion itself returns true in both implementations. TS's caller
checkParenthesizedExpression first requires nonempty direct hasJSDocNodes, and
native lacks this guard. Source probes establish that neither untagged original
parenthesis has direct attachments; the inline assertion control does. The
probes are research, not complete oracle qualification.

A6-16-1 adds self.has_jsdoc_nodes(node) to the existing is_in_js_file guard
before querying either JSDoc satisfies or type assertion. Existing shared
checker::jsdoc::has_jsdoc_nodes implements the pinned hasJSDocNodes predicate;
use it, not has_jsdoc_property (which includes an empty tag cache) or an arena
scan. No AST mutation, tag lookup/cache changes or comment source scan.
Preserve missing-expression recovery and CheckMode propagation. The JS-file
guard is equivalent to the JS guard already in both TS JSDoc predicates; TS
negative controls verify the equivalence. The original checkParenthesized ledger
span/hash remains valid. The next ordinary checkExpression returns its existing
CheckResult; no new state, allocation, lifetime, callback, cache or error edge.

Dependency order: checkExpressionWorker's ParenthesizedExpression branch →
checkParenthesizedExpression → hasJSDocNodes → isJSDocSatisfiesExpression then
isJSDocTypeAssertion → existing satisfies/assertion or ordinary expression worker.
In particular, inherited getJSDocTypeTag is preserved when a non-type direct
JSDoc makes the caller guard true. Existing immutable parser attachments are the
source of truth; enclosing comments and caches are distinct facts.
E-PROTOCOL, E-RESOLVER-BASE, E-METADATA-BASE, E-PRINTER-BASE and
E-CHECKER-FACTS-BASE are premise-unchanged at their published boundaries. This
is a private semantic branch correction; no emitter architecture shape changes.
Requalify its diagnostic/type behavior through full commands and adjacent
checker tests. No new sink or host failure edge needs fault injection.

40 complete TS commands:10 shapes ×JS/TS ×ES5/ES2015; declaration-tagged one/
three/no parentheses, untagged parentheses, inline type one/three, inline
satisfies with/without declaration type, non-type direct JSDoc and literal input.
Each retains a trailing export, declaration and source maps. Compare every
ordered diagnostic/span/message, file path/byte/callback field, emit result,
status and exit. All expectations are fresh TS6 outputs repeated twice and
byte checked, with no normalization. ES5 TS5107 output controls remain separate
from active ES2015 semantic controls. Count the actual diagnostic modes after
minting. Failed fresh commands require two independent jobs because the shared
helper catches outside its internal loop; duplicate panic text is one execution.

After must pass all40 twice, and an unconditional original projection of
jsdocTypeCast.ts, jsdocTypecastNoTypeNoCrash.ts and checkJsdocSatisfiesTag15.ts.
The latter two are existing positives in the frozen original706. Also run the
existing checker tests for JSDoc assertion/quick type, empty-cache property vs
attachment presence, unrelated JSDoc on parameter assignments, missing-expression
recovery, JSDoc signature this-type and compound-assignment inherited this-tag.
No new unit merely mirrors the boolean expression. Source/options/expectations
stay unchanged between before jobs. Poll all jobs to real exit before mutation.
Follow the user-authorized lightweight loop in the schedule header. All other
A owners, remaining standalone comments, B–E and hosted acceptance remain open.

Pinned TS6.0.3 owners (vendor/typescript-6.0.3/lib/_tsc.js):

- checkParenthesizedExpression: 81000–81010, SHA256 `e09e928d312103d37dda4c13512d952ea898028cf66871324eb8b78f15db7144`.
- hasJSDocNodes: 12534–12538, SHA256 `45123251ec15122f9da8fa85555784320976613ea79839fdaec4671cfb8e256d`.
- isJSDocTypeAssertion: 27553–27555, SHA256 `62e3b328401615642ade63804d7b852af9ff6cd6f4ed3ff1c159e9409f492193`.
- isJSDocSatisfiesExpression: 19322–19324, SHA256 `79cfc3070f41c28cc9d54bf73f6a0877061e1e669ce3812813007a063da8b3be`.
- getJSDocTypeTag: 11714–11720, SHA256 `0c07d7f33cca41752e2a1714164de9d4912d78de25145a44694d688c4ceb48a9`.
- checkExpressionCached: 80580–80595, SHA256 `d53b8def69286cea6beb2fdadf985dcd3c6d0dec3ef171f10eda495f50485178`.
- checkDeclarationInitializer: 80604–80628, SHA256 `140897d2a5fd50d78e8cfdcf90087bec70318e099c79f4d2acef4c49302c902d`.

Before:36 exact per job (four actual executions each) and4 failed per independent
job (two actual executions each),32.1s and32.32s,exit101.
Two failures are ES2015 JS declaration-one and declaration-three, at ordered
diagnostics; they miss TS2322. Both targets also fail declaration-literal at
main.d.ts: the function returns the full declared union instead of the literal
"a". The same parenthesis guard currently returns the inherited assertion type
to the existing initializer/cache/flow/inferred-signature path; adding the guard
lets that path consume the literal expression type. All preceding tuple fields
are exact; fields after each first mismatch remain unqualified until after.
Require all four differences to disappear; do not rewrite inferred signature
nodes or special-case declaration output. If the literal failures remain, retain
them explicitly and investigate their separate proven owner before another edit.
No outside mismatch was found.20 ES5 commands retain TS5107 output controls;
20 ES2015 commands have active semantic checking, including the existing TS2322
no-parentheses and TS1360 satisfies positives. A directly attached non-type
JSDoc intentionally admits the inherited type assertion and remains positive.
The before receipt is ratchets/h2-8a-jsdoc-parentheses-guard-before.v1.json; its outside archive contains complete logs and
execution-time sources. The global original-before tuple is at A6-14 while this
fresh native before is at A6-15; these revision scopes are not conflated.

Final measured result: all40 fresh commands are exact twice (43.78s),
including both missing-diagnostic and both inferred-return-type failures.
All three original commands are exact twice (19.56s), including the
original jsdocTypeCast failure and two previous positives. Both compiler
harnesses exit0. Six adjacent checker tests pass (0.02s,exit0). No new
failure or expected-output edit was required; all36 fresh positives are retained.
The one added guard fixes both observable effects through the existing checker
paths. The result and outside source/log receipts are in
ratchets/h2-8a-jsdoc-parentheses-guard-after.v1.json. Other A owners, the four
standalone parent NoNestedComments controls, B–E and hosted acceptance remain
open. The A6-14 whole769 checkpoint stays immutable; no global count is inferred
from this focused repair.
