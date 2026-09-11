# H2.8a A6-25 shared prerequisite: binding-name cloning

This amends the initial [setter-name packet](h2-8a-defineproperty-setter-names.md)
after its frozen first repair. The first-after ratchet retains that one-file
packet and readiness unchanged. Production adds only
`crates/checker/src/node_builder/signatures.rs` to the existing statements.rs
scope; it does not widen the printer, general factory or comparison contract.

The initial repair passes 197/213 complete commands twice, with eight existing
readonly failures and eight new binding-name declaration-map failures. All 62
then-selected node-builder units pass. The eight map failures block closing the
setter owner. Their first boundary is the complete source-map result; later
tuple fields are not qualified by component agreement.

The new 48-command matrix is frozen on that first repair with the shared helper
still byte-equal to the base. Both jobs match 12 commands twice and fail the same
36 once, with identical first vectors. All first boundaries are source-map
results. The 60 additional cloned-Program captures show 28 map-only differences,
four computed-access map plus extra namespace differences, and four map plus
escaped-name spelling differences. Captures are supplemental executions, not
full actual tuples. Total fresh comparison/capture executions are 180. The
three TS typed-pattern shapes (12 commands) are positive controls. Preserve all
48 expected observations and the earlier 108 setter inputs without rewriting.

| Step | Pinned upstream behavior | Rust implementation | Proof |
| --- | --- | --- | --- |
| A6-25-4 | elideInitializerAndSetEmitFlags visits every child and token before removing an initializer | A private typed NodeDataChildVisitor borrows the same checker, arena and context; reuse generated try_visit_each_child, with no string reconstruction or shallow leaf cloning | Nested object/array, elisions, rest tokens, computed names and initializer controls |
| A6-25-5 | isLateBindableName uses entity-name AST and StringOrNumberLiteralOrUnique type flags; tracking precedes traversal | Before children, only when tracker.can_track_symbol and ComputedPropertyName, recover its original checker node, test is_entity_name_expression, call existing check_computed_property_name, test the exact flags, then track_computed_name; propagate existing checker abort | Literal identifier versus widened property-access controls, complete diagnostics and namespace writes |
| A6-25-6 | update binding element, clone non-synthesized nodes, replace emit flags | Use existing typed update_factory_node; explicit object/array binding factory faces recalculate child flags. Visit initializers first and then update the binding element to initializer=None. Clone only if both pos/end are non-synthetic, without restoring parsed ranges; set exact SingleLine and NoAsciiEscaping | Same-source AST origins/positions/flags and all JS/declaration maps, escaped identifier output |
| A6-25-7 | TypeScript has one node pool; native child handles have source ownership | Validate target; mount an absent owning Program source using the existing resolver projection policy. Traverse within that source, preserve array positions/trailing comma through update_node_array, then clone_node_to_source only when target differs | Native initially-unmounted origin/different target control and UnknownSource factory-error control |

The entry still takes the original checker NodeId. Determine its Program source
from binder.file_index_of_node, form the existing EmitResolverNode, project it,
and mount binder.source(file_index) with its SourceFileId if absent. An absent
projection after mounting propagates the existing UnknownNode resolver error;
it never becomes an empty identifier. No new public resolver API, global helper
policy, raw child-ID attachment across sources, or mutable parsed tree is needed.
Each recursive call reads NodeData from its source-scoped original, performs the
computed-name action, visits all children, and uses existing factory update/clone
operations. The visitor returns every child and token; invalid handles propagate
the existing factory error. NodeArray identity is retained when unchanged and
its source ranges/list flags are copied on update. Source remapping is last.

E-RESOLVER-BASE is modified-requalify for the late-bindability query guard and
schedule. E-POSITIONS is modified-requalify for detached binding-name ranges;
E-METADATA-BASE is modified-requalify for exact recursive emit flags. These local
policies are active-unqualified until the focused tests pass. E-ARENA,
E-CONTEXT and E-PROTOCOL are premise-unchanged: original identity projection,
mounting, deep cross-source remapping, single borrowed checker/context lifetime,
and typed factory/checker errors already exist and are re-read and pinned.
The architecture itself gains no new boundary or dormant feature.

The native same-source control failed before at ObjectBindingPattern pos/end.
The source-control test's first compile attempt used the wrong existing
with_context callback arity and omitted out; its zero-test failure is archived
separately, then only the test setup was amended. The final three native
controls execute before implementation; their exact results are frozen in the
source-before ratchet. Invalid target is a native ownership fault, not a new
TypeScript input admission. Source identities are native representation proofs.

After qualification runs 261 complete commands: all 156 fresh setter/annotation/
binding cases, 96 adjacent JSDoc-implements cases, one original getter/setter and
eight original JSDoc-implements commands. Require 253 exact twice and only the
unchanged eight readonly failures, with their identical first vectors. Run all
65 current node_builder:: substring tests (including syntactic builder units),
and the 22 declaration-blocking controls (44 emit-only executions). No capture
executions after repair. Preserve any new failure before revising runtime or
evidence. Historical global denominators stay immutable; H2.8 remains open.

Use two Cargo jobs, background priority, real exits and immutable launch/log
receipts. The current lightweight schedule applies; no full developer CI or
historical certificate walk is claimed. Readiness requires unresolved=0 and
undispositioned=0 before the shared helper changes.

Whole TypeScript owners added for this amendment:

- elideInitializerAndSetEmitFlags: 52880–52907, SHA256 `f8bdbba84cdea52719e327d0bb00c48532bd6d5d35b61b62620ccddfc12a6099`.

- trackComputedName: 52910–52938, SHA256 `747771ab8de848541c5687c87f5865240522be824fb4b335ebd8884e26e437ed`.

- isLateBindableName: 57616–57618, SHA256 `a3ca84f693cd555a61ef5703f86728ddd264868531c337681ab9c95f38d397ba`.

- isLateBindableAST: 57622–57628, SHA256 `59c4b435e4afe281eb82962635f3f7d2b9656bffe8d030005b431226a59f1a0b`.

- isTypeUsableAsPropertyName: 19351–19353, SHA256 `7f27114b05645570b64cd4fa6e83275753a40f1fb2ad7deef7a16335d60e0acd`.

- checkComputedPropertyName: 74061–74082, SHA256 `9f89c9f08a99020384ee9ddada6d5172dd05aee82d8d3bc8aa6be2499f0bc7e3`.

- cloneNode: 24436–24466, SHA256 `d223dcea6ccf14e9212d40d5b8df188197023622ea3e5d624ffb974a25db19d6`.

- nodeIsSynthesized: 16000–16002, SHA256 `d5eb53abaa73cfcae3a8c02425faff560733177bd619f5e0db719a6f9f603f4c`.

- positionIsSynthesized: 18811–18813, SHA256 `d7c8efa6a3407c62a96399f410fac2ae254372213b526c0c0c96811612cfb7b7`.

- setEmitFlags: 25318–25321, SHA256 `433f76eb52bda18764000c020a783490cd7f87d060b7daca70c2a0427085df34`.

- visitEachChild: 91318–91324, SHA256 `77052fb8845fc55cd604db6cb8fe5c3e22bfc7f435b2efc90dc0a57fc2adcc7c`.

- visitNodes2: 91087–91115, SHA256 `211c41370d3526081e4f4c696ed20f63ade07e0407cab76facb848ceb8a9eff9`.

- createObjectBindingPattern: 22407–22415, SHA256 `23d7a5579cd4dfaa4635b01de2c532bbaeabc4068a99d8ababa9f708fc61c827`.

- updateObjectBindingPattern: 22416–22418, SHA256 `a1877f7e9c670537afcd95fa37782cc6ceb2f526f24b395c6a82041793550d3f`.

- createArrayBindingPattern: 22419–22424, SHA256 `bfaf580f0cf09f3625153d01c95fece6e5bed1ef4870c7b59651a14268e6a21c`.

- updateArrayBindingPattern: 22425–22427, SHA256 `ae9ab75e1a9b7dcc480dc265373b88b3e065b2ab608c71268602266ff8d74d2e`.

- createBindingElement: 22428–22437, SHA256 `53d5a02d4d99e09ae5b8f52777e56e6bdb06131543f763f573cf767b5735b7df`.

- updateBindingElement: 22438–22440, SHA256 `ceb986743d45f7b61e48c74a230ec55ddc7d87cdd373504d1461c786efe5c0e7`.

- visitEachChildOfComputedPropertyName: 91333–91338, SHA256 `2195eb2d1676d5af17d17d82545d081010ca362e1c5b7cdc28043731308e712d`.

- visitEachChildOfObjectBindingPattern: 91644–91649, SHA256 `c414876fc423b9403d02e40d427ab23d103b1da8395946ad8d38a4387933ac18`.

- visitEachChildOfArrayBindingPattern: 91650–91655, SHA256 `c6675d07772807c640c77b9defc3d34a836f582348da7037ba3cbf83e090c331`.

- visitEachChildOfBindingElement: 91656–91664, SHA256 `03d788123083a4fd377397fd6efc4e5244efd568418195eaad3ece4cc8f80906`.

- visitEachChildOfPropertyAccessExpression: 91678–91689, SHA256 `0c69c8bc929840ed1088405511c335835fe86b057d98f790512c7cf904898eb2`.

- visitEachChildOfElementAccessExpression: 91690–91701, SHA256 `f819b2db2d942b7f07ee7460f27e1364b1f0d961b29c23e1f3427d53d37485a4`.

Final measured result with the binding-name prerequisite: 253 of 261 complete
commands match TS twice. All 28 fresh setter-name failures, 36 additional binding
clone failures and the original getter/setter command are repaired. The first
setter repair's eight new binding-map regressions are also closed. All previous
positive commands, 96 JSDoc-implements commands and eight original implements
commands remain exact. Eight separately owned readonly failures execute once
with exactly their before first vectors. Their unconditional comparisons retain
the contracts target's expected exit 101; this is a focused owner close.

The final run executes 514 complete native commands with no supplemental captures,
plus 44 separate declaration-blocking emit executions. All 65 tests selected by
node_builder:: pass: 54 node_builder and 11 syntactic_type_node_builder tests.
They include omitted private write-type queries, the expected missing-setter
assertion, recursive binding ranges/flags/origins, initially unmounted source
remapping and the typed unknown-target error. The 22 declaration-blocking cases
pass. The real combined exit is 101 for the retained readonly differences;
test-target durations in order are [0.05, 0.0, 275.35, 24.2] seconds. No new emitter-suite run
is claimed, because emitter production surfaces are unchanged.

The final record is ratchets/h2-8a-defineproperty-setter-names-after.v1.json. Production changes are bounded
to statements.rs and signatures.rs. The input adapter projects declarationMap;
complete comparators and frozen TS expectations are unchanged. Original
one-file readiness and its failed first-after observation, plus the expanded
prerequisite readiness, remain in immutable archives. Other A owners, B–E, the
final global replay and hosted acceptance remain open. Historical global totals
are unchanged; no full developer CI or certificate walk is claimed.
