# H2.8a source require/import extension rewriting

Runtime repair, based on main `0aaf808be`. The original A37 matrix names two
`emitModuleCommonJS.ts` failures; both unchanged cases remain acceptance targets.
This closes one module-transform owner group. H2.8a and the later B–E profile
activations remain open. Existing PR #516 owns declaration specifiers separately.
The production writer owns only `crates/emitter/src/builtins.rs` and the new
private `builtins/relative_imports.rs`, plus the R5 CallExpression worker in
`crates/emitter/src/printer.rs`; no checker, parser, other printer worker, profile,
known-divergence, oracle-corpus, or shared-comparator edits are authorized.

## Source and design gate

Run `python3 scripts/check-require-rewrite-readiness.py`. The [schema](h2-8a-require-rewrite-readiness.schema.json) and versioned manifest
pins the TypeScript bundle, complete observer/inputs, base Rust and architecture,
source owner spans, steps and tests. It requires unresolved=0 and
undispositioned=0. Source observations are frozen before production edits; the
native baseline's actual exit and inputs are retained separately. The existing
lightweight schedule applies: focused complete observations and affected product
regressions, then hosted acceptance. Historical walks/full developer CI are not
run or claimed.

The pinned source owns `forEachDynamicImportOrRequireCall` (20016–20039),
`getNodeAtPosition` (20040–20055), `isRequireCall` (14901–14914), the module
collector/visitor (110130–110159, 110621–110658), the CJS shim worker
(110938–110951), ESM collector/visitor (113391–113468, 113484–113494),
`rewriteModuleSpecifier` (93242–93248), `shouldRewriteModuleSpecifier`
(15250–15252), and the rewrite helper factory (26013–26021). The manifest
records exact newline-inclusive hashes. These owners, their source caller
sequence and the already-admitted output-extension/helper workers are the
semantic authority, rather than the old diagnostic corpus's expectations.

| Upstream object/transition | Rust representation, producer and consumer | Gap and step |
| --- | --- | --- |
| source-text candidate positions plus `getNodeAtPosition` AST verification | Private `ImportCallRewrites` collects exact current-tree `TransformNode` identities; substring hits are only candidate positions and never semantic recognition | missing; R1 |
| `importsAndRequiresToRewriteOrShim` array and `shift()` | FIFO `VecDeque<TransformNode>` owned by the module transformer; visitor borrows it, consumes only the matching head; no membership-set substitution | missing; R1/R2 |
| `isInJSFile`, callee kind/text, argument count | Existing SourceFile flags, `NodeData::CallExpression`, Identifier and argument array; no binder lookup for the shadowed-require predicate | missing for require; R1 |
| string-literal-like selection versus string-only rewrite | `StringLiteral` can change; `NoSubstitutionTemplateLiteral` remains unchanged; other expressions receive the typed helper call | partial; R2/R3 |
| jsx=Preserve and helper arguments | Immutable `preserve_jsx` option; `.tsx` literal becomes `.jsx`; dynamic helper gets the second `true` argument | missing; R3 |
| module substitution and dynamic import lowering | Existing CJS visitor retains receiver erasure and import lowering; queue selection determines rewriting independently of lowering | partial; R2 |
| ranges, quotes, helper metadata and write products | Existing factory clone/update, helper identifier factory, printer/map and Program comparator | shared-prerequisite, unchanged; R3/R4 |

The queue's identity is observable. A selected outer require rewrites/shims its
first argument without visiting that argument through the module transform.
A nested queued call can therefore remain at the head and prevent later calls
from being rewritten. The source queue belongs to the transformer closure and
is not explicitly cleared between roots of that transformer. Retain that lifetime;
never silently clear or skip an unmatched entry. Failure drops the whole emit
transform; no partial queue is published or reused by a later Program.

## Required architecture references

All rows below are read from the frozen architecture at the trusted base.
The manifest pins each full row including its validation date/ref and visibility.
No unqualified architecture statement is treated as proof of compatibility.

| Concern | Existing Rust seam and disposition | Validation |
| --- | --- | --- |
| E-CONTEXT | Public `TransformationContext` / `Transformer`; private module transformers gain owned rewrite queues. `modified-requalify` for module traversal state; context lifecycle remains unchanged | Source queue lifetime and repeated complete commands; transform errors drop the run |
| E-METADATA-BASE | Existing arena metadata and public factory update/original/map seams. `premise-unchanged` | Exact JavaScript plus source maps and declaration maps |
| E-HELPERS-BASE | Existing `TransformationContext::request_emit_helper` and helper ordering. `premise-unchanged` | Dynamic require and existing dynamic import composition |
| E-HELPERS-PROVENANCE-G | Private `EmitHelperName` and `NodeFactory::create_unscoped_helper_identifier`. `premise-unchanged` | Typed helper references and shared emitter contracts |
| E-PROTOCOL | Existing ProgramSession, artifacts, sink and outcome. `premise-unchanged` | Complete tuple with diagnostics, callback bytes/metadata, order, result, status and exit |

The new module is internal (`mod relative_imports`, `pub(super)` entrypoints).
It creates no public compiler/custom-transform API. System module rewriting,
API1 custom source trees and later option/profile activation remain
`future-owned-fail-closed` or existing behavior; this repair does not activate
those paths. Existing generated-name/ESM helper-import alias gaps retain their
own owners and are not resolved by this packet.

## Executable implementation

R1. Add the private candidate queue. Scan the same two ASCII spellings in source
order, obtain the containing current-tree node with the pinned per-child range
and MetaProperty boundary, then apply the exact JS/require/import/arity and
argument predicates. Do not infer calls from substring matches. Preserve
candidate duplicates and FIFO order. Only source candidate collection may read
source text; there is no output-text replacement. Reuse the detached arena's
child traversal and identity, including synthetic one-sided source ranges.

R2. At each admitted module source, append candidates when rewriting is enabled.
The ESM rewrite visitor consumes a matching call before generic child traversal,
updates only its first argument and leaves the rest and type arguments as-is.
The CJS visitor consumes before its dynamic-import decision. Dynamic import
lowering keeps the existing module/target decision, using rewriting only for a
selected call. Selected non-import calls preserve the first argument's visitor
boundary, visit later arguments, erase type arguments, and retain the existing
callee substitution/receiver-erasure behavior. Visitors borrow the transformer-owned queue; an error propagates and terminates
the transform. A selected first argument still receives tsc's later expression
substitutions. The existing eager Rust substitution model must therefore use an
explicit substitution-only visit phase, with a separate/restored memo extent;
that phase cannot consume rewrite candidates or lower nested dynamic imports.

R3. Share literal/argument production in the private module. Reuse the existing
relative-extension eligibility routine; retain string quote/range/original
metadata through factory updates. Handle JSX preservation from immutable
compiler options. No-substitution templates are string-literal-like for
selection but are unchanged by `rewriteModuleSpecifier`; never add a helper for
them. Dynamic arguments request the existing helper and construct a typed
helper-reference CallExpression; JSX Preserve adds the literal `true` argument.
No new string-based helper references or error-to-success fallbacks.

R4. Run the standalone compiler target `h2_8a_require_rewrite` with filter
`require_rewrite_`. Sixty frozen focused commands (ten shapes in six module
modes) and two original commands must each match twice. Shapes cover literals,
dynamic expressions, excluded paths/arity/callee, local shadowing, escaped and
optional calls, templates, nested queue blocking, disabled rewriting, TS inputs,
and JSX preservation. Do not rewrite inputs or expectations to remove failures.
Additional source-file/import/helper composition controls are separately frozen
if the baseline or source lifetime audit requires them. Existing emitter unit
and contract suites, formatting and scoped strict clippy validate shared seams.
Any pre-existing check failure is retained and compared at the base.

## Evidence and resources

`node scripts/observe-require-rewrite.mjs require-rewrite --write` creates the
absent fixture and compares two independent upstream Programs per case.
`--check` repeats and byte-checks that same observation. Native tests use the
unchanged complete-command comparator. Supplemental captures preserve all fields
after a first mismatch and actual partial writes. They are not added to primary
case counts. All before/after runs use a new directory through the existing
`run-decorator-next-review.py` runner, recording command, real exit, prelaunch
source/input hashes, archive, binary hashes and logs. Copy the resulting receipt
index into this slice's ratchet before landing; never relabel a previous head.

Heavy execution is demoted with `taskpolicy -b nice -n 15`, at most two Cargo
workers and one test thread; no competing owned heavy run. No ceilings increase.
Final production edits invalidate after evidence. Original tuples, known
failures and accepted membership remain unchanged. Hosted `gates` must succeed
at the PR head before a merge commit lands.

## Exact source inventory

| Owner | Lines | SHA-256 | Step |
| --- | --- | --- | --- |
| `forEachDynamicImportOrRequireCall` | 20016–20039 | `18fad0df970874a5281d71546ad89276245efc168a40dc588e26ae3fee67a42d` | R1 |
| `getNodeAtPosition` | 20040–20055 | `fc5235720fccffb0238b56e00eae48c726391a7ce760c4560b6ccdf11c4f393c` | R1 |
| `isRequireCall` | 14901–14914 | `97ce63365149bdbd32198cf0b6527081ed21cc939dcfbe25d0c24caf3aad5077` | R1 |
| `transformModule collector` | 110130–110159 | `66bd8e6f84739519fd78b12adc9e298efd62d1899b189fad43913b31002198c2` | R1 |
| `visitorWorker` | 110621–110658 | `e08f1c5ab48313eb69a0f44b9865454e47ae63583899b06df3d34b07d990a8fd` | R2 |
| `shimOrRewriteImportOrRequireCall` | 110938–110951 | `306616cb5c155e176f85dc5f1ff353bba50813c425194d9adcbc611a13a55208` | R2 |
| `ESM collector and visitor` | 113391–113468 | `4bc7520bd8667e09178eb242dcad74448e9d8aa2f5f16bce793a372cb515afad` | R2 |
| `visitImportOrRequireCall` | 113484–113494 | `79b3c28488900eea2aa6d87dc50f2c65acc735a742e247db1e1b29f86f4752a7` | R2 |
| `rewriteModuleSpecifier` | 93242–93248 | `f922e640861acb3c4f3e223a052ecf480ebdc989e9c1d4b545efca742e40aace` | R3 |
| `shouldRewriteModuleSpecifier` | 15250–15252 | `a93fcbefd630ae756d3a55367bf8884b7835c8836f7a8c12fec4e489872dfff3` | R1 |
| `createRewriteRelativeImportExtensionsHelper` | 26013–26021 | `9f8e5ae8ebf093baa14531dc1f3004059db1d12d91bc0e509c9ef8a880761e0a` | R3 |

## Current output-unit boundary

`execute.rs::emit_files_with_activity` (base lines866–915) constructs module
transformers independently for non-bundle output units. This packet preserves
that qualified boundary; its queue lives for one transformer instance and can
span multiple roots supplied to that instance. Cross-output-unit residual queue
reuse is a separate H2.8a module-pipeline owner, not an inherited premise or a
claim of this focused repair. A source-level negative control must retain the
actual result if it reaches that broader owner; no runtime admission is expanded.

The first-argument substitution amendment reads `onSubstituteNode`
(111871–111882) and the existing `CommonJsVisitor::visit`/call/tag/shorthand
substitution workers. It does not migrate printing callbacks or alter their
identity protocols. An imported-path first argument and imported `require`
callee are frozen as independent composition witnesses before using this seam.

## Lowered dynamic-import map amendment

The baseline TS-input controls in CommonJS/AMD/UMD have identical JavaScript
but a source-map tail difference. `visitImportCallExpression` returns the
new synthetic call/conditional directly. Rust additionally attaches the
original import call's text range, producing a redundant mapping between the
generated closing parenthesis and the original statement's semicolon. Remove
those three outer `set_original_and_range` calls; preserve the visited argument's
ranges and the enclosing ExpressionStatement's range. NodeNext/ESM retain native
import calls and their original ranges. This is an R2 producer correction, with
no printer/map-writer change. The existing TS controls and nested controls in
all six modes are its exact before/after witnesses.

| Additional owner | Lines | SHA-256 | Step |
| --- | --- | --- | --- |
| `visitImportCallExpression` | 110952–110969 | `4dd523c5a460cb4c637ef4d4b347f27348bd96434e6f795c25a60c6fd736a147` | R2 |
| `createImportCallExpressionAMD` | 111009–111088 | `2e0e5aafc7ed719f37491d8cc0e38f92aefb1db526e78a13013372493867e792` | R2 |
| `createImportCallExpressionCommonJS` | 111089–111167 | `930020af82c4b4af2a8f77820aa26cb50267ff6a5115c17e519ae690cdb14d23` | R2 |
| `onSubstituteNode` | 111871–111882 | `4275c47c81e1ec247934ac9d79029304a6b07f259ccde7a9036f1bdc3d160a3c` | R2 |

## R5 call comments and optional-token prerequisite

The initial candidate closes both original commands and all four import
substitution controls. Its escaped/optional/callee-comment control still differs
in JavaScript comments and maps, exactly at the existing call-printer omissions
already present in the before captures. R5 adds this required shared prerequisite
to the allowed paths before further production work.

`emitCallExpression` emits the callee through ordinary node comments, then emits
the actual question-dot token through the ordinary token-node pipeline. Rust's
CallExpression worker currently forwards only an optional inherited comment phase
and writes `?.` directly. Complete the callee's ordinary leading/trailing phase
using the existing deferred comment scope/resume, preserving any inherited leading
continuation and nested suppression; emit questionDotToken with the existing
`emit_optional_ordinary_child` / Unspecified hint. The existing call-argument
list and punctuation writer remain unchanged. No source-wide comment search,
new printer state, token flags, normalization, or output splice is introduced.

E-COMMENTS-G and E-COMMENT-PHASES-A36 are `premise-unchanged` for their typed
comment scope/resume and ordinary child protocols; this call consumer is
`modified-requalify` for the six escaped/optional commands and the complete
emitter regression suites. Their full current row/ref/symbol/visibility is
pinned in the manifest. The module correction and this prerequisite ship
together so successful rewriting does not silently discard the callee comment.

| Additional owner | Lines | SHA-256 | Step |
| --- | --- | --- | --- |
| `emitCallExpression` | 118275–118290 | `0c5b07dd72b5883ea433bfe104faff0fa6b60b6f9f24b2d8ecc3eefea81be587` | R5 |
| `writeTokenNode` | 120213–120221 | `04ee5d9812e94e045643f0f4fadc7327cf8c883a3261f0bded2aa14fb847ec80` | R5 |
| `pipelineEmitWithComments` | 120978–120986 | `263af5299b06aaeca9c4e6397b6013e6b2c465afcab2709d8cbdd5ace688bd34` | R5 |
