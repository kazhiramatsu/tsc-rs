# H2.8a A6-39: retained field and accessor producers

Runtime sub-slice of the full H2.8a-e output-matrix goal. Base
`0c8691e76603d82859799937ceee3f3803a80b3e`, 2026-09-10. This packet closes
producer metadata, existing receiver identity and factory-time child flags.
It does not close anonymous constructor allocation, computed cache ownership,
constructor placement, decorator receiver frames or the full output matrix.
Those are ordered successors A6-40 (retained lexical/computed/constructor
ownership) and A6-41 (standard-decorator handoff/state/naming and super).
The source-derived patch is frozen in the readiness manifest before any
production edit. All production writes belong to the root integrator.

Authority: pinned TypeScript 6.0.3, fresh architecture boundaries at their
recorded hashes, then Rust representation. The canonical before record is
`ratchets/h2-8a-retained-accessor-owners-before.v1.json`: 454 complete cases,
228 exact/226 failed twice, 908 primary and 908 supplemental executions.
All prior 418 complete captures are unchanged at this base. New 36 context
observations were produced by 72 actual TS commands. The other 148 fresh
source cases include two upstream exceptions, preserved without native
execution or complete-command equality credit. There is no new before replay.
The last full original corpus remains 755/769; no arithmetic projection of
focused repairs is a new global result.

## Executable sequence

A6-39-1 applies the manifest's exact baseline-to-candidate replacements to
`crates/emitter/src/builtins/class_fields.rs` only. The staged source was
constructed as Rust design, outside production. Before applying it run
`python3 scripts/check-retained-field-producers-readiness.py --before`.
Any new source owner/file/representation requires amendment and a fresh gate.

1. In both retained class visitors, preserve `metadata.class_this` when
   present, otherwise the actual `data.name` TransformNode. Pass that node
   through transform_members and transform_native_auto_accessor. The existing
   absence fallback remains `this`; absence is an A6-40 negative control, not
   qualified anonymous-constructor behavior. Existing TypeScript class-name
   generation already supplies the anonymous default declaration name:
   `finish_typescript_class_declaration`/`update_class_declaration` remain
   unchanged. The fresh default declaration controls have matching JS before.
2. In transform_auto_accessor and transform_native_auto_accessor, create the
   private storage identifier with original-name provenance. Produce backing
   via NodeFactory::update_node, preserving raw property range. Complete its
   local/child flags immediately. Getter and setter remain fresh synthetic
   nodes. Pass the original modifier tokens to the getter, and flags-derived
   fresh modifiers to the setter using modifier_flags and
   create_modifiers_from_modifier_flags.
3. create_accessor_storage_access constructs an ordinary synthetic
   PropertyAccessExpression from the actual receiver and storage node. It
   carries no initializer-name text range or NoNestedSourceMaps. Keep
   create_receiver_access/create_this_access unchanged for relocated ordinary
   field assignments; they own those ranges and suppression flags.
4. set_accessor_metadata reads incoming comment/map ranges separately, using
   raw comment endpoints or raw SourceRange only when that metadata is absent.
   In TS order: backing original, replace flags with NoComments, map; getter
   original, comment, map; setter original, replace flags with NoComments, map.
   Do not copy raw getter/setter positions. All conversions return existing
   InvalidSourceRange errors; unknown required children retain typed errors.
5. create_class_field_node and complete_created_node_flags complete exactly
   the newly created node, from already completed children and existing local
   factory flag functions. Preserve NodeFactory's private-expression flags.
   Use this seam for storage access, return, getter/setter, assignment,
   expression statement, block and static block. Static block explicitly adds
   ContainsClassFields. Getter/setter body propagation excludes possible
   top-level await through existing factory_child_transform_flags. New node
   arrays aggregate their child flags. There is no recursive repair walk and
   no change to initialize_transform_flags at the existing pass boundary.
6. create_static_initializer_block leaves the block synthetic, sets its
   original to the field and its comment range to the field's raw endpoints.
   Its child initializer keeps its own map range. Set the child's comment
   range to Synthesized and clear synthetic leading/trailing comments; do not
   install a new NoComments flag. This preserves the source operation order
   in transformPublicFieldInitializer. No source-map range is invented for
   the generated static block.

A6-39-2 runs the unchanged complete comparator over all 454 cases:
`TSC_RS_RETAINED_ACCESSOR_CASE_SET=all cargo test -p tsc-rs-compiler --test
contracts -- retained_accessor_owners_match_complete_typescript_observations
--nocapture --test-threads=1`. The manifest freezes required repair IDs,
prior exact IDs, and successor-owned negative controls before execution.
Require all prior positives and all required repairs exact twice. Capture
and inspect every remaining mismatch; a test failure is reported as a
failure, never converted into a passing complete inventory. Preserve actual
exit, repetitions, complete tuples, sources and binary before another build.
An unexplained new failure blocks A6-39 qualification. Additional source-owned
repairs may be credited only when the complete tuple is exact twice.

A6-39-3 runs `cargo test -p tsc-rs-emitter --lib --test contracts --
--test-threads=1`, requiring the same 494 unit and 451 contract test identities
and the 1350 frozen declaration reprints. Freeze the after profile and amend
this row's lifecycle only after the exact required IDs and adjacent evidence
pass. Full H2.8 completion still requires its original inventories and hosted
acceptance; the user-authorized lightweight workflow omits historical full
local CI/certificate walks, without claiming those commands passed.

## Semantic and local-gap map

All new helpers are private methods on ClassFieldsVisitor. Their arena is
owned by TransformationContext for one transform session; TransformNode
contains source and node identity. None of these operations caches a mutable
checker borrow or changes the parsed tree.

| Row | Producer -> Rust owner -> consumer | Current gap / action / evidence |
| --- | --- | --- |
| existing static receiver | class metadata or parsed/generated class name -> Option<TransformNode> -> create_accessor_storage_access | partial-or-stale String projection; A6-39-1; named, Unicode, escaped and default controls |
| backing raw positions | updatePropertyDeclaration -> NodeFactory::update_node -> retained property printer | partial-or-stale construction; A6-39-1; static/instance map controls |
| accessor original/map/comment | original field -> set_accessor_metadata -> EmitMetadata/printer | partial-or-stale getter raw copy/setter absence; A6-39-1; comment/newline/removeComments controls |
| storage access | receiver/storage nodes -> create_accessor_storage_access -> return/assignment | partial-or-stale initializer metadata reuse; A6-39-1; named/static/instance controls |
| modifier provenance | getter source array -> fresh_accessor_modifiers -> setter | partial-or-stale parsed modifier reuse; A6-39-1; static modifier comment/map controls |
| local flags | completed child nodes -> complete_created_node_flags -> parent factory and following passes | partial-or-stale NONE-only producers; A6-39-1; accessor bodies and adjacent emitter contracts |
| static block metadata | source field -> create_static_initializer_block -> printer map/comment pipeline | partial-or-stale raw positions; A6-39-1; alias-map static fields and function/arrow/comment controls |
| ordinary initializer access | source name -> create_receiver_access -> moved initializer statement | already-exact adjacent producer boundary; A6-39-1 preserves code; prior exact set and after complete inventory |
| raw comment endpoints | source positions -> CommentRange::from_raw -> getter/static block | already-exact qualified independent-endpoint representation; A6-39-1 calls existing conversion; Unicode and comment controls |
| actual map coordinates | source positions -> SourceRange/SourceMapRange -> recorder | unchanged typed boundary; A6-39-1; complete JS maps, CRLF and Unicode controls |
| generated private identity | original member name -> storage node original link -> downstream provenance | partial-or-stale missing original; A6-39-1; collision and lower-target negative controls |
| synthetic constructor/list positions | class container and source member array -> create_synthetic_constructor -> constructor/body-list printer maps | partial-or-stale missing raw spans; A6-39-1; both instance-this-adjacent commands and all prior positives |
| error/host products | command -> existing ProgramSession/comparator -> complete tuple | premise-unchanged; A6-39-2; diagnostics/status/declarations/writes remain compared |

The exact patch removes former accessor callers of create_receiver_access
and former raw getter range copies. The ordinary initializer caller remains.
Existing private-name allocation is not certified from a string prefix;
this packet preserves it and its collision controls, and A6-40 owns general
scope-aware generated identities. Flags are invalidated only by subsequent
AST changes at existing update/pass boundaries. Metadata is session scoped
and follows existing original-node merge rules; set_flags then replaces the
inherited bitset in the two source-specified places.

## Guarded successors and composition

These are observed negative controls, not inferred exact prerequisites.
No production case/path predicate or new admission gate is introduced.
The admitted *qualification* scope consists of current producer cases without
any of the following predecessor/successor obligations; the all-case runner
continues to execute and report their failures normally.

- A6-40 anonymous constructor and lexical environments: static accessor on an
  unnamed class expression without earlier class_this, including source,
  function, arrow, parameter and loop scopes. Guard: no class receiver node
  exists. Frozen anonymous/function/return/loop/parameter/arrow and combined
  loop/binding-default controls expose the missing allocation explicitly.
- A6-40 computed caches and private generated naming: non-simple computed
  accessor key or prior generated assignment. Guard: computed key is not a
  simple inlineable expression. Frozen computed-call/symbol/decorator-cache
  controls remain complete adjacent negatives. They cannot qualify by spelling.
- A6-40 constructor/accessor result visitation: public instance fields force
  moving the accessor backing initializer. Guard: assignment mode with a
  non-static initialized ordinary public field. Mixed-instance-fields controls
  expose the missing initializer and constructor map owner. No output exception
  or comparator exemption is added for this branch.
- A6-41 standard decorators: decorated class/member enters the existing
  standard decorator pass. Frozen handoff/context controls expose the separate
  receiver, private-static, descriptor and helper-name causes. ESNext/define
  bypass and ES2015 runtime-static-block positives remain protected. The
  classSuper/Reflect read/call/tag/assignment/update/destructuring source owners
  remain required by A6-41; this-only projections do not qualify that visitor.
- Existing lower-target route: target < ES2022, or ES2022 private-static file
  handoff, enters DownlevelClassVisitor. A6-39 does not alter it. All 144 helper
  cases protect that boundary; its backing-update/provenance discrepancy is
  explicitly future-owned by A6-40 and its decorated handoff by A6-41.

Transform ordering stays TypeScript -> standard/legacy decorator -> class
fields -> existing target/module transforms -> printer. Receiver selection
uses current nodes; no reordering of heritage, computed names, initializer
visitation or helper requests is authorized. Printer ExpressionSyntaxContext,
comment scope/resume and substitution hooks are untouched. A6-40 must correct
allocation before attempting anonymous parity. A6-41 must correct the earlier
producer before claiming the private-static route or lexical-this behavior.
Custom transforms/API1, cancellation/emit API expansion/H2.8d and CLI/H2.8e
remain with their existing owners and typed admission boundaries.

## Source, architecture and resource ledger

The machine manifest pins 47 whole upstream owners, every recorded predicate
and call within them, current Rust inputs, all complete source fixtures,
baseline and staged patch. Read its owners/branches/calls/local_gap/rust_map/
architecture/witness rows as executable parts of this packet. Calls are in
source order; each maps to an owned producer step, an unchanged frozen seam,
or one of the guarded successors above. The source-only 136-owner inventory
and 81-function decorator closure remain research evidence; they are not
claimed as fully implemented graphs by A6-39.

Applicable rows: E-PROTOCOL, E-ARENA, E-METADATA-BASE, E-METADATA-G-CLASS,
E-CAPTURE-CLASS-G, E-NAMES-CLASS-G, E-PRINTER-BASE, E-COMMENTS-G,
E-COMMENT-SCOPE-H, E-POSITIONS, E-RETAINED-PRODUCERS-A39. Exact current rows,
visibility, validation refs and dispositions are frozen in the manifest.
EA-GAP-FLAGS and EA-GAP-CAPTURE are split here: owned local producers are
modified-requalify; broader allocation/context rows remain future-owned with
the explicit guards above. Historical qualifications are preserved. The producer sub-row is now active-qualified only for the measured A6-39
profile below; successor behavior retains its separate negative controls.
Neither dormant maps nor an unqualified capture row is an inherited premise.

Use one heavy command at a time, taskpolicy background, nice 15,
CARGO_BUILD_JOBS=2. Archives/prelaunch hashes/log/exit/binary/captures make each
job resumable. Poll its live handle; never restart because observation times
out. No fixture/path branches, text substitution, handwritten outputs,
unknown-branch success, relaxed comparisons, expected-output regeneration,
profile membership change or source-wide flag repair is permitted.

Readiness unresolved owned semantic/ownership/oracle rows: 0; undispositioned
rows: 0. Successor rows are named, guarded and observed, not silently closed.
Completion of A6-39 requires the measured profile above; H2.8 remains active.

## A6-39 implementation amendment after the first full replay

The first complete candidate leaves three required targets. Two instance-this-adjacent
commands have the synthetic constructor/list map discrepancy described below.
It fixes the accessor metadata on the required
private-storage-collision case, but still moves its existing private field
initializer into a constructor. Its archived before capture already contains
that extra constructor; this is a remaining old cause, not a newly admitted
case. The required127 repairs and all228 prior positives remain unchanged.
The first full result and exact counts are preserved in the amendment record.

The added whole getClassFacts owner (_tsc.js96844-96898) supplies the missing
condition proof. On this retained route, shouldTransformPrivateElementsOrClassStaticBlocks
is false and shouldTransformInitializersUsingDefine is false. An instance
auto-accessor alone does not set WillHoistInitializersToConstructor. The existing
first ordinary initialized public instance-field scan is the correct retained
condition. A6-39-1 now removes the second accessor-only try_fold in
transform_members and removes the unnecessary mut. Private storage collision
and the ordinary/mixed field controls remain in the complete comparison.
Other getClassFacts outputs (constructor reference, static this/super, decorated
class) are independent bits, not inputs to this boolean projection; no
implementation claim for those bits is added. Their A6-40/A6-41 obligations
and guarded negatives remain unchanged.

A6-39-1 also removes the now-unused private identifier_text method. Both former
consumers were the class receiver String projections already removed by the
first candidate; a zero-call source check and the compile dead_code diagnostic
prove the old representation has no remaining consumers. No replacement
String receiver API is added. The next measurement must use this amended
source and retain all454 comparisons.

Reproducible source and input-shape checks are now durable scripts:
`node scripts/observe-retained-field-producer-sources.mjs --check` and
`node scripts/classify-retained-field-producers.mjs --check`. The manifest
embeds all source/disposition rows and no longer depends on untracked scratch
files to run its ready check. Original scratch evidence remains in the first
after archive. The first amendment added the constructor-hoist condition to the local-flags
row. The final constructor-range amendment below completes the current ledger:
47 whole owners,230 predicates,285 calls and13 Rust representation rows.

The two required instance-this-adjacent commands expose the synthetic constructor
position producer shared with the already named mixed-field successor. A6-39-1
now threads the actual class container and original member array from both class
visitors through transform_members into create_synthetic_constructor. After
creating the body, copy source member pos/end into its statement array with
set_node_array_text_range; leave the Block itself synthetic. Create the
constructor through the completed child-flag producer; copy only the container's
raw range to it, leave its original absent, and mark starts_on_new_line.
transformConstructor97253-97289 and transformConstructorBody97329-97431 define
these separate positions. createConstructorDeclaration21982-22001 and
setTextRange28256-28258 pin the unchanged factory contracts. Both required
commands keep matching JS bytes and must now also match every map field. The
initializer content/order and parameter/loop state remain with A6-40; this
change does not substitute a raw class original link for constructor ownership.



## A6-39 measured qualification

The [frozen result](../../../../ratchets/h2-8a-retained-field-producers-after.v1.json) has SHA-256
`189df9f4f36320a2145298bd7f2b32544b0f6edd9b61745c59b7c647309b5def`. Its final native source is
`26de511b53ea232b073235a4507f703ae78cd4881c12bfea720f71f932dc03e7`. Source/input archives, complete captures,
actual exits/logs and binary identities were frozen before subsequent builds.

The unchanged454-case inventory produces360 complete commands exact twice:
132 repairs and all228 prior positives, whose entire captured payloads remain
unchanged. All127 predeclared repair IDs pass. Five additional complete repairs
are recorded individually, including four decorator-handoff map controls and
one ESNext/set mixed-instance-field control. They do not qualify whole
decorator or mixed-initializer algorithms. Both repetitions agree for every
case; no diagnostic/status/declaration or typed-boundary regression appears.
There are908 primary and908 supplemental executions in the final native job.

The inventory itself still exits101 with94 failures, all in the frozen
successor-negative set. Preserve that result:55 decorator-only controls,
32 anonymous-constructor controls,4 computed-cache controls,2 combined
computed/decorator controls and1 moved-accessor-initializer control. No fixture,
expected tuple, comparator or accepted profile was relaxed. The initial
candidate's356/454 result and its three required misses are retained alongside
the amended candidate's result; the final profile does not erase that attempt.

The unchanged emitter regression command passes the same494 unit and451
contract identities, including1350 frozen declaration reprints. This qualifies
the A6-39 producer ownership row, including the source-backed WillHoist
condition and constructor/list raw positions. It does not establish a passing
full454 inventory or a new original769/class1228 total. The last full original
checkpoint remains A37 at755/769; all remaining H2.8a-e work and hosted
acceptance before landing remain required.
