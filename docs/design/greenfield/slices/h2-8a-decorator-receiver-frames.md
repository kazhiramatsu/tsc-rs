# A6-41 receiver-frame design experiment (unqualified draft)

Purpose: implement source-owned this receiver selection in standard decorators,
then independently investigate the remaining static-accessor/naming/super/map
rows. This is an isolated research candidate, not production admission or whole
A6-41 completion. Full H2.8 scope remains active. Root production is forbidden
until the mandatory whole design/readiness gate closes all unresolved rows.
The allowed draft production file is standard_decorators.rs only; compose its
patch after v16 in a future isolated attempt once full60 is terminal. No source
fixture expectation, parser code or root production file may be changed.

Pinned source: TypeScript6.0.3 _tsc.js whole SHA
1c59e77a54b186ec43fa7f3e0d3c4bb15ca5eb5ba43e96b1d3a267139eddd3e3.

## Pinned source owners

- updateState 98973–98995: 43884579281d628ed37ed5860425dc7194a9a2e6bd63fceb6d4d6599dffe4f12
- enterClass 98996–99000: b4c3404bf758e51e69d069a580f1d9de3c34c753cfbf153ba9e7d7633dbea6cd
- exitClass 99001–99006: fe3a7b094d480e73fef4329ed7eabe60eee97fed1f846e819d7b0ef3349d1309
- enterClassElement 99007–99016: 79d5397bee4df100c7d80d6174cba45c1292c2bc1232187d5812eb0af4d44d78
- exitClassElement 99017–99026: 2a48803ab07ea1afdb2967bb4f97916a136b07628e679fca50faef148aaf3aa7
- enterName 99027–99031: e5304e441ab060194f053c160202192b13e8a95bbb05105062e3a67ef738c7ec
- exitName 99032–99036: d93030cd3a6c46f96d4a69627c776743683772c9fe6224be62b8a143231c1b72
- enterOther 99037–99046: f25ffa851798f80bd8020303b1920171083e429e706944c56ba8c450e0654b96
- exitOther 99047–99057: 078eef0c834a174c3ef001a9c287230077770b221d2c4ebfe560305d27253c08
- shouldVisitNode 99058–99060: 021945554ec8c13263e27a0e766fb3e192e2ef56abe5bafe785b2eec7787db83
- visitor 99061–99148: 797319c351cce85844e48557b070648fc10b28a3d671fbf1060be4b25cda6fc7
- partialTransformClassElement 99831–99944: 46d06f1175b4bfd01fe5b4df1893f5f1164b80e7a9c94f3340604e4a6b06b912
- visitThisExpression 100151–100153: 814bc6d3fc95059d2410d3f1fcdb835ca279bf98cbc4c90daa62ca3e3507423d
- transformAllDecoratorsOfDeclaration 100546–100553: 1b5a7f8ef32e03228943f699e46d9615e70cb9b615307d42e75b6c83fa19d6a6
- transformDecorator 100554–100569: d59f7ca48434d47359734b185ca41b870154ec401bea6c5336d7e0074b8ec622

## Rust ownership and ordered edits

1. Add a DecoratorReceiverFrame enum: Class, ClassElement{owner,receiver}, Name,
   Other; receiver is Option<TransformNode>, never a printable String. Own the
   frame stack in StandardDecoratorVisitor. ClassThis lookup is the top element
   receiver, or, for Name, the exact third predecessor if it is a ClassElement.
   Class and Other have no active replacement; arrows push no frame. The
   existing class-this assignment block's metadata supplies identity.
2. Frame entry allocates a visit-scope key; frame exit restores parent scope on
   both Ok and Err. Cache node/array/expanded declaration visits by that scope,
   not globally by node ID: reused nodes may have different receiver contexts.
   Other-frame depth need not be compressed for this receiver-only projection;
   an intervening Other remains a barrier to the same name lookup.
3. Return the actual classThis identity for a ThisKeyword; attach no replaced
   token range/original/comments. Keep source transform-flag recomputation on
   every changed enclosing node. Follow source VisitEachChild ordering for
   receiver transitions; method names get a Name frame before body traversal.
4. Undecorated classes visit heritage under the enclosing frame, then members
   under Class(None). Each member has a ClassElement frame: only static
   properties/blocks receive the classThis reference; ordinary method,
   constructor and instance property bodies don't. Ordinary object methods and
   functions enter Other; nested decorated classes allocate their own identity.
5. Allocate the decorated classThis assignment identity before member traversal.
   Class decorators and heritage run in the enclosing frame, before entering
   the new Class frame. Decorated property initializers run in their element
   frame. Member decorator expressions run in that frame; computed names run
   under Name. Existing descriptor creation, pending initializer, comment/map
   and naming work remains visible; do not collapse these into text rewriting.
6. Preserve synthetic class-this assignment block's suppression and the existing
   named-evaluation helper metadata. Class-frame state is restored before the
   caller resumes. Never rewrite generated helper construction by printable name.

## Witnesses and validation

Existing frozen decorator-receiver-context36 complete commands (two repetitions)
cover static arrow/function/object method, nested class name/heritage/body, and
nested function computed-name boundaries at ES2015/ES2022 and set/define. Run a
fresh TypeScript observer --check after current full60 is terminal, then native
selection context using the existing compiler contract's selector (runner must
expose it; do not modify expected output). Next use existing selection additional
(46 completed commands, two upstream exceptions excluded explicitly), then full530
against strongest completed predecessor. Preserve every prior positive and
report each complete tuple/map/diagnostic/callback result, not JS-only agreement.
Root and copied inputs remain frozen during each actual run; one heavy job at a
time, taskpolicy background, nice15, CARGO_BUILD_JOBS=2.

## Open rows, never implicit deferrals

The complete super read/call/tag/assignment/update/destructure callee graph,
FileLevel versus ReservedInNestedScopes binding creation, static accessor
backing/descriptor maps, full source member/constructor phase order and decorator
pending-expression interleaving require disposition and verification. A typed
this-state implementation is progress toward A6-41, not proof of the whole row.
These rows remain in scope and block root production readiness; the experiment
must retain all observations even if it improves only part of the existing53.

The first draft now also distinguishes source names from already prepared
computed names with DecoratorMemberNameVisit. Prepared names are consumed once;
public methods visit only their remaining children in the element frame.
Class decorator arrays consume the expressions already visited outside Class;
they no longer depend on a global cache to suppress a second visitor call.
Syntax try_visit_each_child maps absent optional fields without callbacks, so a
local NodeData can temporarily omit its visited name while visiting other
children; the name is restored before flag calculation/factory comparison.
The arena record remains untouched throughout that local projection.

Frozen first candidate: `docs/design/greenfield/slices/h2-8a-list-intervening-printer.candidate-v17.patch`, SHA-256
11b357f0bfcef1eeab1e7e2df3f369d7e313d0f46ff9ec2be810c2985c796d80. Root standard-decorator file SHA-256
042d017e951554a104adf938c138e28af63672ba6ff91b69d671b0c88b40b659, draft SHA-256
392613a1151ee535e12a0100bf7776542458d218fa9e2c77451c251c40f97c0f. The candidate had not executed at this first freeze; attempt61 is recorded below.

The strongest complete predecessor is full60:477/530 exact twice,53 fail,
all tuples/errors/partial writes unchanged from53. Receipt
`ratchets/h2-8a-literal-property-design-experiment.v1.json`, SHA-256
03419eb53896a349a2e6e1b6db11f52a00621076140d2e5d11c926cb391d9279.
The36 receiver source commands were freshly checked twice (actual exit0),
25.4468670419883s; fixture SHA-256
04ded10209011b3689d76ae74dc8363d03c0f38ee96ea6ced4fab86af31178d4.
The source-check log/input pins are retained alongside this design evidence.

## Execution record (2026-09-10, isolated draft `draft/h2-8a-decorator-receiver-frames`)

Not production admission, not a qualified after profile. Root production files
are untouched; the draft lives in the worktree `~/dev/tsc-rs-dec53`, composed
after v16 on the exact full60 candidate sources (the archive
`source-and-inputs.tar.gz` named by
`ratchets/h2-8a-literal-property-design-experiment.v1.json`, SHA-256
`10ab6e458533ed4f5f9dc3d37b7b48d78cf9e2bfee8755efaf3e38eb40bea114`).

Patches (apply in this order on that base; `git apply --check` clean):

- `h2-8a-decorator-receiver-frames.candidate.patch` (standard_decorators.rs only),
  SHA-256 `dbff2cc384d93651c5fbde694cbf50a862d9606126cf3944d660f27c672d346a`;
  draft file SHA-256 `7872d2f6eb27e03aa8bf74fc0affe0f30cca00ecd7dbd412240c475f80fbfdfe`.
- `h2-8a-decorator-static-accessor-lowering.candidate.patch` (class_fields.rs
  candidate v2 + class_fields/downlevel.rs; outside the allowed draft file, kept
  separate for that reason), SHA-256
  `d91e8ef40a45d0701b55ab10bd4c9c14eb0180529bcc350fd9f19e75343ce635`;
  draft file SHA-256 class_fields.rs
  `c423c24425c14a1201920514db3d9dd7ea235ffafc24ea8b803eab9552148081`, downlevel.rs
  `506ea9864f9e71cf58b991d05beff14ced5c3fdcd1ed93a53af74579dd1bc0b9`.

Results (complete command tuples, two repetitions each, compared against the
fixture `typescript_observation` and against full60): selection `context`
36/36 exact twice; selection `additional` 46/46; selection `all` **530/530 exact
twice, 0 typed failures, 0 regressions, all 53 full60 failures exact**
(actual exit 0, 1094 s, demoted, CARGO_BUILD_JOBS=2, own target dir). Evidence:
`target/h2-8a-decorator-receiver-frames-draft/` (1060 captures with the same
schema as the runner's, `run-all.log` SHA-256
`0273f1296d69eb37b7c56486bd62067cda28edd60ee43fa71bd1889f2a0a1b9b`,
`analysis.json` with per-capture receipts, `analyze-captures.py`, `mapdiff.py`).
The only edit after the 530 run is a clippy `nonminimal_bool` simplification of
the class_fields.rs routing condition with an identical truth table.

Root causes closed, by row count (53 = 28 receiver-context + 21 handoff + 4
redirectors):

1. Receiver frames (39 rows need it, 27 alone): esDecorators' `this` →
   `classThis` projection was absent; the class-fields pass substituted only
   below ES2022, stopped at every nested class, and copied the `this` token's
   range onto `_classThis`. Ported as `DecoratorReceiverFrame` (class /
   class-element / name / other), `visitThisExpression` returns a range-less
   clone of the class-this identity, nested undecorated classes visit heritage
   in the enclosing frame and computed names through the `top.next.next.next`
   rule, methods/functions open `other`, arrows do not.
2. `finishClassElement` (12 rows): an updated undecorated element takes the
   past-decorators source-map range; the set-semantics static block inherits it
   through `set_original_node` → `merge_from`.
3. Static private/accessor flag: `isAutoAccessorPropertyDeclaration` counts;
   flagged classes lower at ES2022 and ESNext (routing), and the downlevel's
   auto-accessor expansion follows `shouldTransformAutoAccessorsInCurrentClass`
   (`True` below ESNext, else set semantics && will-hoist).
4. FileLevel helper names collide only with source identifiers
   (`_classDecorators`, `_classDescriptor`, `_classExtraInitializers`,
   `_metadata`, `_classSuper`, `_static/_instanceExtraInitializers`, and
   `_classThis` unless a static private/accessor member exists).
5. esDecorators lowers only private auto-accessors itself (the ESNext public
   shortcut removed); the candidate class-fields Maybe rule then keeps
   `static accessor` native.
6. Computed-name cache temp: one generated binding
   (`TargetBinding::allocate_planned`) shared by every identifier spelled like
   the temp in the class scope (declaration, cache assignment, decorator
   context name, accessor names); the finalizer reserves every untagged
   identifier text, so a single untagged use renames the binding. Emitted
   computed names keep the parsed name's text range.
7. Rebuilt member list keeps the parsed member-list range (synthesized
   constructor `}` map); accessor-storage static-block statement follows
   `moveRangePastModifiers(backingField)` = generated name position (no leading
   map), emulated with `NO_LEADING_SOURCE_MAP`.

Deviations from the ordered edits above: visits stay memoized globally by node
id (a node is visited once per tree; the member name is pre-visited under the
name frame and the memo reuses it), so no scope-keyed cache or
`DecoratorMemberNameVisit` was needed; `ClassElement` frames carry only the
receiver, not the owner. Class decorators, heritage and the class-this identity
are handled before the class frame as specified.

Still open (unchanged rows): the super read/call/tag/assignment/update
callee graph, `_outerThis` numbering, decorator pending-expression interleaving,
and the three pre-existing clippy findings in the candidate v2 class_fields.rs
(unnecessary casts, type complexity) — none of them exercised by the 530.

## Independent handoff review and final composition (2026-09-10)

The external draft supersedes the first local v17 prototype. Attempt61 is
terminal (actual101, 284.57622245897073s): 26/36 context commands exact twice,
10 still different. It is retained as evidence, not developed further.
`scripts/analyze-decorator-receiver-handoff.py` independently checked all1060
external captures against the fixture bytes in the full60 source archive,
required both repetitions and null errors/partial writes, verified capture and
log receipts, and compared complete observations with full60. It confirmed
530/530 exact, every prior477 observation unchanged, and precisely the53
previous failures changed to exact. Receipt:
`ratchets/h2-8a-decorator-receiver-handoff-review.v1.json`, SHA-256
`4f239244c33884c1fcb7007b4af564c1a54c79bd909d868eb556762a2e8eb041`.

The saved handoff does not include a launch input manifest or a retained
executed binary, and its final routing predicate was edited after the run.
The capture comparison proves the saved observations; a new isolated run will
bind the final candidate bytes and executable to those observations.

For this research composition, the permitted isolated production files are
`standard_decorators.rs`, `class_fields.rs`, and `class_fields/downlevel.rs`.
This explicitly extends the earlier one-file experiment boundary to the two
class-fields owners identified by the review; it does not admit root production
edits or close any unresolved readiness row. The two external patches remain
separate immutable artifacts. Their final bytes were composed after v16 into
`h2-8a-list-intervening-printer.candidate-v18.patch`, SHA-256
`839f72ea3535695548dacfd13d51d92dc47c1a86138cb15145cb8f62c8e51d93`.
Applying v18 to the existing pre-list base reconstructs exactly the external
worktree's sixteen affected files. Across all1013 full60 copied inputs, only
the above three production files differ; the literal property/writer changes,
checker fixes, native adapters, and all expected fixtures are byte-identical.

Validation sequence: fresh TypeScript `--check` for receiver-context36,
class-helper-accessor-producers144 and retained-accessor-owners146, then
`python3 scripts/run-comma-printer-design-experiment.py 62 all --factory --list-owner`.
After the actual terminal result, run the full analyzer for62 and compare its
complete tuples with both full60 and the external handoff. The expected result
is530 exact twice, all477 prior positives unchanged,53 repaired, no typed
failure, and unchanged root/copied inputs. No expected-output changes are
permitted. Adjacent emitter checks follow on the same final source bytes.

Review items still requiring evidence before qualification:

- Global node-ID memoization assumes a node is visited in one receiver context.
  The computed-name previsit deliberately relies on that memo. The530 parsed
  command witnesses do not establish this invariant for shared or synthesized
  nodes; verify reachable producers before accepting or changing the policy.
- `clone_receiver_class_this` preserves flags and omits the replaced token's
  range, but returns a clone where upstream returns the actual identity. The
  530 prove their observed outputs, not identity/provenance equivalence for
  every downstream consumer.
- The draft calls `prepare_class_super` before transforming class decorator
  expressions in `prepare_decorators_and_computed_names`. Upstream
  `transformClassLike` transforms class decorators before visiting heritage.
  Both are outside the new class frame, but their relative visit/allocation
  order still needs a composition witness. The earlier execution description
  saying they are handled "as specified" is not evidence for that order.
- Frame teardown on an early `Result::Err`, all generated-binding collision
  inputs (including a supplied global-name oracle), and the previously listed
  super/pending-expression/constructor-phase rows remain undispositioned.

## Final-source verification: attempt62

The integrated v18 candidate passed all530 complete commands twice (actual
exit0, 983.3120198750403s). There are zero typed failures or regressions;
all477 prior positives are unchanged and exactly the53 full60 failures are
repaired. All1060 captured tuples equal the external handoff observations.
The frozen input manifest, source archive, executed binary and terminal result
now bind this result to the final patch bytes, including the post-handoff
routing simplification. Root production and copied inputs remained unchanged.

Receipt: `ratchets/h2-8a-decorator-receiver-design-experiment.v1.json`, SHA-256
`77f5440820c9c2ab3e9933533021684b67252b79c30e0545f3d1ac981578cf7d`.
Source archive SHA-256
`3dfa18b3b050a8a6182b8df8217c764a213e3352a285533bc298382de6acac56`;
executed binary SHA-256
`7e4542b241945b8c097520cb8bb89010531526304c2c3414eb519a35f61cc6fb`.
Fresh TypeScript checks passed for326 complete commands twice; their receipt is
`ratchets/h2-8a-decorator-handoff-source-checks.v1.json`, SHA-256
`df26ecf2497caa3fd2edafe140e94ce554fe2c7258209c9f6c35e2e722a25c03`.
The two retained upstream exceptions remain recorded separately and receive no
complete-command credit. Adjacent emitter validation uses attempt63 on the
same production and native test bytes.

Comparison lesson: all10 remaining local v17 context failures had identical
JavaScript and differed only in the map write and corresponding emit-result
map observation. The missing finishClassElement range is consistent with those
differences. The26/36 result does not show that scope-keyed memoization caused a
receiver error, and the external530 result does not prove global memoization
safe for every reachable shared-node producer.

A concrete name-owner review item remains: the draft initializes FileLevel
names with `system::collect_identifier_texts`, which scans the detached arena
including synthetic nodes. `ParsedSourceIdentifierNames::collect` already
provides the parse-only census. Upstream additionally consults `hasGlobalName`.
Freeze synthesized-name and cross-file global-name witnesses and trace the
existing typed binding finalizer before closing that ownership row. This is a
source-level gap requiring measurement, not a proved new530 regression.

## Adjacent regression correction following attempt63

Attempt63 was terminal101 after191.22053708403837s:494 library tests passed;
451 contracts passed and one failed. The failure was
`esnext_assignment_mode_static_auto_accessor_keeps_dynamic_this_receiver`,
which required a synthesized getter at ESNext. Its complete terminal log and
input/binary pins are preserved in
`ratchets/h2-8a-decorator-receiver-emitter-attempt.v1.json` (SHA-256
`f42a25ac1f93c3f1e2bc52a758d187992d0dd882141c611891b91bac54962d68`).
No passing-contract-suite claim is made for63.

The fresh source observer `scripts/observe-decorator-static-accessor-routing.mjs`
uses that exact input at ES2022/ESNext and set/define, twice each. Upstream
retains the ESNext static accessor in both modes and lowers it at ES2022 in
both modes. The fixture keeps source diagnostics, including the alwaysStrict
option deprecation; these are printed-output controls, not complete command
observations. No existing TypeScript expected fixture was changed.

The isolated test correction is explicitly allowed in
`crates/emitter/tests/integration/active_transform_contract.rs`, as the separate
`h2-8a-decorator-static-accessor-test.candidate.patch` (SHA-256
`792efc1fa4b35dbaea18628d690232f1249a2481e61063f73242d4ef220fc11e`).
It replaces the incorrect substring assumptions with full source-owned printed
output equality for all four controls. Root's existing native test and all
production files remain unchanged. The new fixture is
`crates/emitter/tests/fixtures/decorator-static-accessor-routing.json` (SHA-256
`062dfb6eccb239bc0a0edf9f309a7cb8bc1682690c68a4088eb44dbeaade387f`).
Attempt64 repeats the emitter suite. It must prove identical full62 production
and compiler-test bytes; only the emitter contract correction and new fixture
may differ. Full530 is not replayed for this test-only change.


Attempt64 exposed a test-adapter mode mismatch on the fourth control,
ESNext/define (actual101/20.979746332974173s;494 library tests and451 contracts
passed). The default standalone printer preserves unchanged source bytes;
compiler JavaScript emit in `execute.rs` selects `SourceFileTextMode::Canonical`.
The TypeScript transpileModule observation also prints canonically. The test
adapter must use that compiler mode on every control, including the unchanged
ESNext/define source. The first three full-output comparisons already passed.
The terminal log is preserved in
`ratchets/h2-8a-decorator-receiver-emitter-routing-attempt.v1.json` (SHA-256
`09dee0926cb4befdcf7d8cc8385968241056cd714c27c058fddcd62c295f3df9`).

The corrected isolated adapter is
`h2-8a-decorator-static-accessor-test.candidate-v2.patch` (SHA-256
`04ac9fe5b1ea2ee62c506e5fc711f21d4ecf73cdc5ee84c536a01c0d59ab5e67`).
It changes only the print mode relative to the first test patch; upstream
observations and production bytes are unchanged. Attempt65 repeats the emitter
suite with the same expected494/452 counts and all four printed controls.


Attempt65 passed: actual0 after20.569391999975778s,494 library tests and452
contracts, including all four source-owned accessor print controls. The same
suite also checked1350 declaration reprints and8 template-provenance controls
(two repetitions). These populations overlap and are not added to530.
Receipt: `ratchets/h2-8a-decorator-receiver-emitter-checks.v1.json`, SHA-256
`186ed40ee40b7b41df4e60a23a33d8f340ef39c0da02a575df74eeab8b994580`.
The frozen source-input comparison proves that all full62 production and
compiler-test bytes are unchanged; only the corrected emitter contract and
its new source-generated fixture differ. Both failed adjacent attempts remain
preserved, with their actual errors and binaries, alongside the passing result.

The current runnable research composition is v18 production + UTF16 testpatchv4
+ decorator testpatchv2, applied by the isolated runner. The53 supplied failures
and the adjacent emitter regression checks are closed for this composition.
The unresolved owner rows listed above, the whole readiness gate, hosted
acceptance and root-production integration remain open; this is not a whole
H2.8 completion or qualified-after claim.
