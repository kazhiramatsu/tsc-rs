# Current emitter architecture

Document role: **active architecture authority for the current Rust emitter**.
This page records only facts checked against the current `crates/emitter`,
`crates/checker`, and `crates/compiler` code. It does not own slice order,
candidate counts, acceptance history, or the semantics of TypeScript 6.0.3.
It is a dated validation record, not a promise that the implementation still
matches an emitter design written earlier. Before a slice relies on a row, its
symbols and invariants are checked against the current code again; older
emitter designs supply rationale or intended semantics only.

- TypeScript 6.0.3 owns semantics and phase behavior.
- This page owns the validated Rust representation and integration seams.
- [Post-H1 completion slices](post-h1-completion-slices.md) owns order, scope,
  design readiness, and slice completion.
- The current packet selected from the
  [slice-packet index](slices/README.md) owns the bounded change and exact
  commands.
- Ratchets and tests own evidence.
- [H1 emit](h1-emit.md) owns the frozen H1 claim and its design history. It is
  not a current implementation map.

## 1. Validation and lifecycle

The first code audit for this page was performed on 2026-08-14 against the
uncommitted H2.5g candidate worktree based on commit
`11f5d0abb93fed4b109bdb1dc552721ceb05e707`. That base hash does **not** identify
the current candidate: production files are modified and the cursor modules
are not yet tracked by a commit. Consequently every current-code row touched
by H2.5g is `active-unqualified`. H2.5g changes are not a frozen premise until
its runtime gates pass and its profile is frozen at an immutable final
validation ref as described below. The later merge ref is delivery lineage;
it is not the validation ref.

Every architecture row uses one lifecycle:

| Lifecycle | Meaning |
| --- | --- |
| `active-qualified` | Present in current code and protected by an exact profile frozen at an immutable final validation ref. |
| `active-unqualified` | Present in the current candidate and code-reviewed, but its current profile has not yet closed. |
| `dormant` | A typed seam exists, but production selection cannot activate it and it earns no compatibility credit. |
| `planned` | Required by a named future slice; the final Rust representation is not yet approved. |
| `superseded` | Historical design or former representation; it is not an implementation instruction. |

At every slice close, the slice owner updates the affected rows with the final
validation ref, date, Rust symbols, tsc owner identities, tests, and lifecycle.
A stale symbol or missing validation ref makes the row unavailable as a design
premise; the next slice starts with a read-only architecture-validation
sub-slice instead of guessing.

Validation and delivery use two distinct immutable refs:

1. Before merge, commit the final production code, tests, generated evidence,
   and profile, then run all runtime gates from that clean immutable commit.
   Once they pass, that implementation/evidence commit is the **final
   validation ref**. A following documentation-only commit may cite it, freeze
   the architecture/profile relationship, and promote the affected candidate
   rows to `active-qualified`.
2. After merge, the roadmap-review documentation records the actual merge ref
   as delivery lineage and verifies that it contains the final validation ref.
   It also verifies that every profile-bound runtime/evidence input is
   byte-identical there. Otherwise the affected rows return to
   `active-unqualified` and require a new validation ref. The merge ref does
   not replace the validation ref or retroactively change the profile.

A commit cannot contain its own hash. Therefore neither the documentation
commit that cites the validation ref nor a predicted merge commit may be used
as that ref. If the implementation, tests, or evidence change after the final
validation ref is created, create a new implementation/evidence commit, rerun
the required gates, and freeze against the new ref.

Lifecycle use and transitions are strict:

- only a fresh `active-qualified` concern is a frozen implementation premise;
- `active-unqualified` is code-reviewed research input, not compatibility;
- `dormant` is a typed non-compatible seam and `planned` is a design target;
- editing a qualified concern first changes it to `active-unqualified`;
- `planned -> dormant`, `planned -> active-unqualified`,
  `dormant -> active-unqualified`, and
  `active-qualified -> active-unqualified` are candidate transitions;
- only an exact profile freeze at an immutable final validation ref permits
  `active-unqualified -> active-qualified`; and
- no row moves directly from `planned` or `dormant` to `active-qualified`.

When one concern contains differently qualified behavior, it is split into
sub-rows. Slice packets record the sub-row, exact symbol and visibility,
lifecycle before/after, and an impact disposition. A stale/missing symbol,
illegal transition, or undispositioned architecture gap fails readiness.

## 2. Required reading path

An emitter implementation task follows this path and no shorter one:

```text
repository README
  -> docs/design/README.md                 document roles and precedence
  -> docs/design/greenfield/README.md      active execution entry
  -> this document                         current Rust ownership and seams
  -> post-h1-completion-slices.md          schedule and design gate
  -> slices/README.md -> current packet    exact bounded work
  -> pinned tsc owners + frozen predecessor contracts
  -> implementation and focused tests
  -> full inventory and runtime gates
  -> immutable implementation/evidence commit (final validation ref)
  -> profile/architecture freeze and PR evidence
  -> post-merge delivery-ref record
```

The slice packet contains a mandatory-reference table. Each row names an
architecture row ID below, its validation ref/date, the exact current Rust
symbols, the pinned tsc declaration/hash, the frozen predecessor contract, and
the lifecycle transition the slice intends to make. A historical document may
explain a decision but cannot substitute for this table.

## 3. Current pipeline and dependency direction

The current one-shot path is:

```text
ProgramSession::emit
  -> PreparedEmitHost (planning-only; syntax is unavailable)
  -> validate_bootstrap_emit_request
  -> preflight_emit -> EmitPreflight { EmitOutputPlan, diagnostics, blocked outputs }
  -> check_program_with_authoritative_modules_at_for_emit
       -> callback with ProgramSnapshot + CheckerSession + checked diagnostics
            -> CheckedEmitHost (the same Program facts + snapshot syntax)
            -> CheckerSession::with_emit_resolver
                 -> emit_files_with_activity
                      -> revalidate request/plan + apply the diagnostic gate
                      -> for each output unit:
                           TransformArena
                           -> get_script_transformers_with_activity
                           -> transform_nodes
                           -> Printer::print
                           -> EmitArtifact construction
                      -> after every artifact is constructed:
                           ordered OutputSink writes
                      -> EmitOutcome
  -> empty Program/plan: UnavailableEmitResolver fail-closed branch
```

`ProgramSession::run` remains a separate no-emit entry. The dependency
direction is intentionally acyclic:

```text
emitter  -> syntax + types + diagnostics + program
checker  -> emitter protocols and its normal dependencies
compiler -> checker + emitter + program + host
```

`crates/emitter` owns consumer-facing traits. `CheckerSession` implements
`EmitResolver`; the emitter never depends on `crates/checker`. `EmitHost` is
read-only and filesystem writes cross `OutputSink` only. The compiler's private
`PreparedEmitHost` deliberately exposes no syntax while request validation and
output planning run. Only after checking does private `CheckedEmitHost` add the
immutable `ProgramSnapshot` syntax, and it remains scoped to the live checker
and resolver borrow.

## 4. Validated architecture map

The validation ref for every `active-unqualified` candidate row below is the
candidate described in section 1. Exact tsc spans and hashes remain on the
cited Rust functions and in slice owner artifacts; a future packet must
revalidate them rather than copying line numbers from this page.

The Rust owners, visibility, and internal invariants in this table come from a
current-source audit plus the named focused Rust contracts. The H2.5g
qualification artifact pins `transformES2016` as its upstream owner closure;
its exhaustive inventory and the cumulative profile prove observable output,
diagnostic, and failure-boundary equivalence, not the choice of an internal
Rust representation. A row therefore names those two evidence layers
separately and never treats a profile count as proof that a particular helper
type exists.

| ID | Concern and invariant | Current Rust owners | Lifecycle / validation | Evidence or next owner |
| --- | --- | --- | --- | --- |
| `E-ENTRY` | No-emit and emit are distinct typed entries; no-emit constructs no emitter-only component. | `tsc_compiler::ProgramSession::{run,emit}`; `tsc_emitter::emit_files_with_activity` (public) | `active-unqualified`; candidate audit 2026-08-14. H2.5f protects the predecessor subset. | H2.5g profile/final validation ref; H0/H1 no-emit canaries |
| `E-PROTOCOL` | Read host, semantic resolver, artifact, sink, and outcome have separate ownership; planning cannot observe syntax, checked syntax exists only within the live checker/resolver scope, and sink errors become diagnostics at the write boundary. | `tsc_emitter::{EmitHost,EmitResolver,EmitArtifact,OutputSink,EmitOutcome}` (public); private `tsc_compiler::{PreparedEmitHost,CheckedEmitHost}`; `tsc_checker::emit::CheckerSession` implementation in `crates/checker/src/emit.rs` | `active-unqualified`; candidate audit 2026-08-14 | H2.5g profile; every output slice preserves this boundary |
| `E-PLAN-SCRIPT` | JavaScript selection/root/mode/path planning stays typed and fail-closed. | `tsc_emitter::{EmitSelection,EmitRoot,EmitMode,EmitOutputPaths,EmitOutputUnit,EmitOutputPlan,EmitPreflight}` (public re-exports; definitions in private `plan` module) | `active-unqualified`; candidate audit 2026-08-14 | H2.5g profile and output-plan controls |
| `E-PLAN-FUTURE` | Map/declaration/bundle/build-info/targeted axes exist as non-compatible typed seams. | The same public plan types; `crate::plan::EmitOutputPlan::validate_bootstrap_shape` and unsupported branches in `execute.rs`/`printer.rs` | `dormant`; candidate audit 2026-08-14 | H2.6, H2.7, and H2.8 activate them separately |
| `E-ARENA` | Parsed trees remain immutable; the detached arena appends synthetic nodes and tracks the mounted parsed interval. | `tsc_emitter::{TransformArena,TransformSource,TransformSourceId,TransformNode,TransformNodeArray,NodeFactory}` (public) | `active-unqualified`; candidate audit 2026-08-14. H2.5f protects the predecessor subset. | H2.5g profile; every later transform packet maps provenance explicitly |
| `E-RESOLVER-IDENTITY-G` | A resolver query accepts only an identity anchored in the mounted immutable parse interval. Projection follows typed original-node provenance, rejects synthesized/range-incompatible identities, and carries the owning Program source so an appended raw `NodeId` cannot alias a parsed node in another source. | `tsc_emitter::TransformSource::{contains_parsed_node,program_source}` and `TransformArena::{is_parsed_node,parse_tree_resolver_node,require_parse_tree_resolver_node}` (public); `tsc_emitter::EmitResolverNode` | `active-unqualified`; candidate-only audit 2026-08-14 | Current-code audit and resolver-identity contracts; H2.5g inventory/profile for observable parity; every later resolver consumer revalidates the projection |
| `E-METADATA-BASE` | Transform flags and `emitNode`-equivalent facts are sparse session side tables; there is no standalone Rust `EmitNode` syntax object. Original/map/comment/value identities are separate. | Public `tsc_emitter::TransformArena::{transform_flags,set_transform_flags,array_transform_flags,set_array_transform_flags,metadata,metadata_mut,clear_session_metadata,get_original_node,set_original_node}` over private storage in `crates/emitter/src/factory.rs`; public `tsc_emitter::{EmitMetadata,SourceMapRange,CommentRange,JavaScriptString}` defined in `crates/emitter/src/metadata.rs` (`EmitMetadata` storage fields remain `pub(crate)`, with its cross-crate operations exposed by public methods) | `active-unqualified`; candidate audit 2026-08-14 | H2.5g profile; H2.5h/H2.6 extend rather than infer from text |
| `E-METADATA-G` | The H2.5g candidate extends typed comment, erased-type-boundary, resolver/substitution provenance, string-spelling ownership, and generated-name facts without changing parsed nodes. Class-definition coordination is the separate sub-row below. | `crate::metadata::{RelocatedStatementListComments,RelocatedTrailingCommentOwner}` (`pub(crate)` types); public re-export `tsc_emitter::InternalEmitFlags::DECLARATION_NAME_REFERENCE`; public `tsc_emitter::EmitMetadata` with `pub(crate)` fields `type_node`, `string_literal_text_source`, `referenced_export_container`, `generated_import_reference`, and `generated_binding_role_suffix`; consumers in private `printer.rs` and active transforms | `active-unqualified`; candidate-only audit 2026-08-14 | H2.5g exact inventory and profile freeze |
| `E-METADATA-G-CLASS` | Cross-pass class facts retain typed source/declaration identity: a synthesized legacy-decorated class expression records its declaration owner, standard/ESNext transforms can transport `class_this` and `assigned_name`, generated constructor reads point at their class owner, and generated computed names retain cache provenance. The TypeScript pass projects a parameter property as a `PropertyDeclaration` whose original is its source `Parameter`; later standard-decorator updates preserve that chain. Class-field lowering follows the full original-node chain when it needs either source-language declaration, and never derives a runtime name, constructor local, or resolver identity from a publication/generated spelling. | `crate::metadata::ClassExpressionDeclarationOrigin` (`pub(crate)`); public re-export `tsc_emitter::InternalEmitFlags::GENERATED_COMPUTED_PROPERTY_NAME`; public `tsc_emitter::EmitMetadata` with `pub(crate)` fields `class_this`, `assigned_name`, `class_constructor_reference`, and `class_expression_declaration_origin`; producers in private `builtins.rs`, `legacy_decorators.rs`, `es_next.rs`, and `standard_decorators.rs`; consumers in private `class_fields.rs` and `class_fields/downlevel.rs` | `active-unqualified`; candidate-only audit 2026-08-14 | Current-code audit and focused named-evaluation/decorated-class/parameter-property contracts, including `legacy_decorated_anonymous_default_reserves_definition_before_computed_key`; H2.5g inventory/profile for observable parity |
| `E-JSX-FACTORY-G` | Classic-JSX factory resolution carries a typed parse-tree lookup location rather than installing a synthetic parent or resolving from printable text. A paired element uses its `JsxOpeningElement`, a self-closing element uses that element itself, and a fragment uses its `JsxOpeningFragment`. Import-declaration and namespace/enum-container identities resolved there are copied onto the synthesized factory-root metadata; TypeScript namespace/enum substitution consumes the container identity, while module substitution consumes the import identity. | Private `crate::builtins::jsx::{JsxFactoryResolverLocation,JsxVisitor::{visit_jsx_element,visit_jsx_self_closing_element,visit_jsx_fragment,create_entity_expression}}`; public `tsc_emitter::EmitResolver::{get_jsx_factory_import_declaration,get_jsx_factory_export_container}`; public `tsc_emitter::EmitMetadata` with `pub(crate)` fields `referenced_import_declaration` and `referenced_export_container`; private `crate::builtins::TypeScriptTransformer::try_substitute_exported_name` and module-transform consumers | `active-unqualified`; candidate-only audit 2026-08-14 | Current-code audit plus `classic_jsx_factory_root_carries_typed_namespace_identity`, its self-closing/fragment siblings, and checker CommonJS JSX contracts; H2.5g inventory/profile for observable parity |
| `E-CONTEXT` | One per-unit context owns lexical/block environments, hoists, helpers, diagnostics, hooks, initialization, and reverse disposal. | `tsc_emitter::{TransformationContext,Transformer,TransformationResult,transform_nodes}` (public) | `active-unqualified`; candidate audit 2026-08-14. H2.5f protects the predecessor subset. | H2.5g profile and transform lifecycle controls |
| `E-CAPTURE-BASE` | Active target transforms use scoped typed plans/frames for their admitted lexical `arguments`, `super`, receiver, and binding behavior; these are a current integration base, not the complete ES2015 capture model. Every pushed frame has an RAII guard that restores the exact prior depth on success, error, or panic. | `crate::builtins::es2017::{AsyncLexicalArgumentsPlan,AsyncSuperCapture,Es2017Visitor}`; class-specific owners are in the sub-row below | `active-unqualified`; candidate audit 2026-08-14. H2.4b/H2.5f protect the async predecessor subsets. | H2.5g exact inventory/profile; H2.5h-a maps extensions onto this base |
| `E-CAPTURE-CLASS-G` | Class lowering separates source-member facts, expanded private declarations, constructor-reference causes, static-evaluation frames, and declaration-placement ownership. `ClassFactsPlan` is scanned from source member categories before auto-accessor expansion; for static properties/blocks it reads the complete member transform flags, so a computed-name lexical `this`/`super` reserves the constructor identity before member-key temporaries. The side-effect-free `PrivateEnvironmentPlan` then owns expanded private declarations without multiplying source class facts. `StaticReceiver` answers lexical `this`, while the orthogonal `StaticSuperPolicy` keeps legacy-decorated `super` invalid even when a stable definition receiver exists. Computed names evaluate in the enclosing class environment; arrows inherit a static frame, and classes/ordinary functions install typed boundaries. | Private `crate::builtins::class_fields::downlevel::{ClassFactsPlan,PrivateEnvironmentPlan,ClassTempPlan,ClassConstructorReferencePlan,PrivateEnvironment,StaticReceiver,StaticSuperPolicy,StaticSuperAccessResolution,StaticBindingFrame,StaticBindingFrames,StaticBindingFrameGuard,StaticLexicalFacts,OriginalTreeOwnership,DecoratedClassDeclarationExpansion}` and `DownlevelClassVisitor::{scan_class_facts,static_lexical_facts}` | `active-unqualified`; candidate-only audit 2026-08-14 | Current-code audit plus `private_auto_accessor_provenance_does_not_invent_a_constructor_fact`, `nested_class_computed_field_names_use_the_enclosing_static_evaluation_frame`, `legacy_decorated_definition_binding_serves_static_this_but_not_super`, and nested-super controls; H2.5g inventory/profile for observable parity |
| `E-CLASS-PENDING-G` | Class-definition setup and erased-member effects form one ordered typed stream. Its setup prefix is ordinary instance-private storage, one optional instance brand, then generated instance auto-accessor storage; the member walk appends private definitions and flattened erased public-field-key operands in arrival order. A retained computed member drains the complete current prefix. Constructor-alias assignment belongs only to the declaration consumer and is not a pending entry. Each consumer preserves tsc's distinct comma-statement, individual-statement, or expression-operand placement before static operations. | Private `crate::builtins::class_fields::downlevel::{ClassPendingEntry,ClassPendingPlan,ClassPendingPlan::{from_setup_prefix,is_empty,append_private_definition,append_public_field_key_operands,take_entries},ClassOperations::pending,DownlevelClassVisitor::{plan_members,flatten_class_pending_comma_list,inject_pending_expressions_into_member,materialize_class_pending_expressions,materialize_class_declaration_pending_statement,materialize_class_pending_statements,visit_class_declaration,visit_class_expression}}` | `active-unqualified`; candidate-only audit 2026-08-14 | Current-code audit; focused `class_pending_*`, `decorated_class_pending_comma_keys_preserve_tsc_operand_boundaries`, `ordinary_class_expression_keeps_pending_and_static_work_as_ordered_operands`, and standard/legacy decorated pending contracts; `standard-decorator-composition` owner control; H2.5g inventory/profile for observable parity |
| `E-DECORATOR-INITIALIZERS-G` | Standard decorators own typed instance/static queues and receivers for **member** `addInitializer` extras only. Method extras seed the queues before the source-member walk. Every property drains the relevant queue before a decorated field enqueues its own extra initializer; a runtime static block drains the static queue before its body. Residual instance work belongs in the constructor after the reachable direct or transparently parenthesized `super()` path, including a nested `try` path; residual static member work is placed by tsc's pre-injection `hasStaticInitializers` fact. Constructor prologues are scanned as two typed, consecutive phases (standard directives, then custom prologues). The standard-decorator no-super path replays that prefix before the initializer and complete body, matching tsc 6.0.3 without rerunning the Rust visitor; the two class-field paths retain it once. These member queues are never merged with either class-definition `ClassPendingPlan` or the class-decorator finalizer below. | Private `crate::builtins::{ConstructorPrologue,constructor_prologue}`; private `crate::builtins::standard_decorators::{DecoratorInitializerPlacement,DecoratorInitializerReceiver,PendingDecoratorInitializer,PendingDecoratorInitializerBatch,ClassPendingDecoratorInitializers,ClassPendingDecoratorInitializers::{new,receiver,enqueue,drain,queue_mut},DecoratorConstructorSuperPath,DecoratorConstructorInitializerPlacement,StandardDecoratorVisitor::{class_has_static_initializers,transform_class_expression,inject_pending_initializer_expression,inject_pending_initializers_into_property,materialize_pending_initializer_expressions,materialize_pending_initializer_statements,inject_constructor_statement,find_constructor_super_path,inject_constructor_statement_at_super_path,statement_is_super_call,skip_parenthesized_expression}}` | `active-unqualified`; candidate-only audit 2026-08-14 | Current-code audit; focused `constructor_prologue_stops_before_strings_and_noncontiguous_custom_statements`, `standard_decorator_residual_initializer_replays_base_constructor_prologue`, `standard_decorator_residual_initializer_replays_custom_variable_prologue`, `standard_decorator_residual_initializer_keeps_derived_prologue_single`, `standard_decorator_pending_*`, parenthesized/nested-super, and paired residual-static contracts plus the `standard-decorator-composition` owner control; H2.5g inventory/profile for observable parity |
| `E-DECORATOR-CLASS-INITIALIZERS-G` | A class decorator owns a third, single-consumer finalizer lane. `__esDecorate` populates the class-extra array; after decoration and class replacement, `__runInitializers` consumes that array exactly once and only after any residual static **member** extras. The pre-injection `hasStaticInitializers` fact selects placement: `false` merges the class finalizer into the leading decoration block, while `true` places it after static member work in the trailing block. This lane is not an entry in `ClassPendingPlan` or `ClassPendingDecoratorInitializers`. | Private `crate::builtins::standard_decorators::{ClassDecorationPlan}` fields `extra_initializers_name` and `has_static_initializers`; the plan build and placement in `StandardDecoratorVisitor::transform_class_expression`; private `StandardDecoratorVisitor::{create_class_decorate_statement,create_decoration_block,create_run_initializers_statement_with_target}` | `active-unqualified`; candidate-only audit 2026-08-14 | Current-code audit; focused `standard_decorator_class_extra_follows_static_method_extra_in_leading_block` and `standard_decorator_class_extra_follows_static_field_extra_in_trailing_block`; paired member-extra controls; `standard-decorator-composition` owner control; H2.5g inventory/profile for observable parity |
| `E-DECORATOR-PARAMETER-PROPERTY-G` | A standard-decorator initializer prefix attached to TypeScript's projected parameter-property field remains an expression plan until target/field mode selects its materialization boundary. ES2015-ES2021 in either field mode and ES2022+ assignment mode combine the prefix with the constructor local and replace the earlier TypeScript parameter assignment. ES2022 through pre-ESNext define/native mode leaves only the helper prefix in the native field and preserves the original constructor assignment. The fallback `(call, void 0)` is normalized only when the callee carries factory-produced helper provenance and the exact typed helper spelling; a parsed or ordinary synthetic user call named `__runInitializers` is not consumed. In ESNext define mode the standard-decorator transform is omitted; the class-fields transformer is still constructed and registers substitution during `initialize`, but its root transform returns the input unchanged and the substitution remains inert because no class alias was created. | Original-chain producer `crate::builtins::TypeScriptVisitor::create_parameter_property_declaration`; provenance walkers `crate::factory::TransformArena::{get_original_node,is_call_to_emit_helper}`; typed producer `crate::factory::{EmitHelperName,NodeFactory::create_unscoped_helper_identifier}` and `crate::builtins::standard_decorators::StandardDecoratorVisitor::{create_run_initializers_with_target,create_run_initializers_statement_with_target,inject_pending_initializers_into_property}`; private `crate::builtins::class_fields::downlevel::{FieldOperation,FieldValuePlan,SuperStatementPath,DownlevelClassVisitor::{install_instance_operations,materialize_field_value,normalize_parameter_property_prefix,is_run_initializers_call,materialize_field_operation,inject_into_constructor,find_super_statement_path,inject_initializers_at_super_path,statement_is_super_call,skip_runtime_transparent_outer_expressions,parameter_property_local}}`; private `crate::builtins::class_fields::{MovedInstanceInitializerPlan,MovedFieldInitializerPlan,MovedFieldValuePlan,ParameterPropertyLocal,ParameterPropertyAssignmentPolicy,MovedInstanceInitializers,SuperStatementPath,ClassFieldsVisitor::{materialize_moved_instance_initializers,inject_initializers_into_constructor,find_super_statement_path,inject_initializers_at_super_path,statement_is_super_call,skip_parenthesized_expression,parameter_property_local}}` | `active-unqualified`; candidate-only audit 2026-08-14 | Current-code audit; `emit_helper_name_distinguishes_user_calls_and_survives_factory_updates` covers typed, ordinary synthetic, parsed, clone, and update identities; `standard_decorator_helper_does_not_claim_same_spelling_source_call`; the ES2015/ES2021 matrix in `standard_decorator_pending_method_initializer_flows_into_es2015_parameter_property`, its ES2022 counterpart, `standard_decorator_parameter_property_bridge_is_inactive_in_esnext_define_mode`, multi-parameter controls, and direct/nested/parenthesized-super paths; H2.5g inventory/profile for observable parity |
| `E-ORDER-G` | Current built-ins compose through ES2016 and then the selected module transform. | `crate::builtins::get_script_transformers_with_optional_host` (private, `builtins.rs`) | `active-unqualified`; candidate audit 2026-08-14 | H2.5g profile/final validation ref |
| `E-ORDER-H` | ES2015 and Generators are not registered; their exact clusters/order are design work. | No current production symbol | `planned`; H2.5h-a | H2.5h-a owner graph decides runtime suffixes |
| `E-RESOLVER-BASE` | Semantic facts cross a borrowing consumer-owned resolver; unavailable methods return typed errors. | `tsc_emitter::{EmitResolver,EmitResolverMethod,EmitResolverError}` (public); `tsc_checker::emit::CheckerSession` impl in `crates/checker/src/emit.rs` | `active-unqualified`; candidate audit 2026-08-14 | H2.5g profile and resolver controls |
| `E-RESOLVER-CAPTURE-BASE` | Already-active transforms obtain check flags, lexical `arguments` identity, and local-name uniqueness through semantic queries rather than printable-name inference. | Public `EmitResolver::{has_node_check_flag,is_arguments_local_binding,is_unique_local_name}`; `tsc_checker::emit::CheckerSession` implementations | `active-unqualified`; candidate audit 2026-08-14. H2.5f protects the lexical-`arguments` predecessor subset. | H2.5g profile and existing resolver controls |
| `E-RESOLVER-CAPTURE-H` | The complete ES2015 collision/capture query set and its producer/cache ownership map are not yet approved. | Research starts from the current producers in `crates/checker/src/{links.rs,modules.rs,functions.rs,literals.rs,expr.rs,program.rs,emit.rs}`; final additions are unresolved by design | `planned`; H2.5h-a | owner/local-gap/Rust-map closure before implementation |
| `E-SYNTAX-FACTS` | Scanner token facts required by exact ES2015 flags must have an approved persistent representation and clone/incremental propagation. | Current producer `crates/syntax/src/scanner.rs::TokenFlags` is private and ephemeral; no approved persistent target | `planned`; H2.5h-a | template flags and extended-Unicode identifiers; no blanket Node tax without evidence |
| `E-CHECKER-FACTS-BASE` | Existing facts remain with their actual semantic producer and cross only typed resolver queries: check flags in NodeLinks, local-name collision data in binder locals/container topology, and lexical `arguments` identity in checker resolution. | `crate::links::LinksTables::{node,or_node_check_flags}`; `crate::program::ProgramBinder::{locals_of,next_container_of}`; `crate::state::CheckerState::emit_is_arguments_local_binding` (implemented in `modules.rs`); resolver projections in `crates/checker/src/emit.rs`; flag-producer call sites in `expr.rs`, `functions.rs`, and `literals.rs` | `active-unqualified`; candidate audit 2026-08-14 | H2.5g profile and existing direct controls |
| `E-CHECKER-FACTS-H` | The producer/cache/invalidation/resolver map needed by the complete ES2015 capture and collision graph is not yet approved. | The current producer categories in `E-CHECKER-FACTS-BASE` plus owner-graph additions selected by H2.5h-a | `planned`; H2.5h-a | capture/collision owner graph and direct controls |
| `E-NAMES-BASE` | Generated identity, printable name, target provenance, and allocation scope are separate; final names use the composed tree. | `crate::transform::GeneratedBindingId` (`pub(crate)`); `crate::builtins::generated_bindings::GeneratedBindingScopes` and `target_bindings::{TargetBinding,finalize_generated_binding_names}` (`pub(super)`) | `active-unqualified`; candidate audit 2026-08-14 | H2.5g profile and generated-binding controls |
| `E-NAMES-CLASS-G` | Class lowering preserves tsc's semantic allocation phases without representing every role as one mutable alias. `ClassBinding` carries existing or generated identity; `ClassTempPlan` fixes declaration ownership once; `LexicalBindingOwner` and `LoopBindingScopes` place resolver-marked class/private bindings in the current iteration and otherwise hoist them. The allocation sequence is instance brand, semantic constructor identity, actual heritage-super capture, private slots, member/key temporaries, then an optional late class-expression result temp. Publication receivers and aliases created by an earlier decorator pass are observed, not silently reclassified as one of those allocations. | `crate::builtins::class_fields::downlevel::ClassBinding` (`pub(super)`); private `ClassTempPlan`, `ClassConstructorReferencePlan`, `LexicalBindingOwner`, `PlannedTargetBinding`, `ClassGeneratedBindings`, `LoopBindingFrame`, `LoopBindingScopes`, `LoopBindingScopeGuard`, and `DownlevelClassVisitor::{allocate_instance_brand,allocate_class_constructor_identity,allocate_super_base_binding,materialize_private_environment,allocate_class_temp_name,claim_lexical_binding_owner}` in the same module; `TargetBinding`/`GeneratedBindingScopes` are `pub(super)` in `target_bindings.rs`/`generated_bindings.rs` | `active-unqualified`; candidate-only audit 2026-08-14 | Current-code audit plus `class_binding_allocations_follow_tsc_lexical_environment_phases` and both iteration-scoped class-expression contracts; H2.5g inventory/profile for observable parity |
| `E-NAMES-H` | ES2015/Generator scope/name operations are not yet mapped to the current finalization model. | No approved new symbol | `planned`; H2.5h-a | exact generated-name owner and collision controls |
| `E-HELPERS-BASE` | Helpers are typed requests with dependency order and per-unit de-duplication. | `tsc_emitter::EmitHelper`; `TransformationContext::{request_emit_helper,read_emit_helpers}` (public); private `builtins/helpers.rs` | `active-unqualified`; candidate audit 2026-08-14 | H2.5g profile and current helper controls |
| `E-HELPERS-PROVENANCE-G` | Every current AST reference to an unscoped emit helper is created through one typed seam. `EmitHelperName` selects one of the 23 admitted spellings, and the factory atomically creates the identifier with `HELPER_NAME | ADVISE_ON_EMIT_NODE`; helper request/dependency order remains a separate concern. Clone/update propagation follows the existing emit-metadata merge. A consumer that distinguishes a specific helper requires both that provenance and the exact escaped spelling, so parsed and ordinary synthetic same-spelling identifiers remain user code. Category consumers use `HELPER_NAME` to keep helper calls/tags out of ordinary module receiver erasure. The UMD-generated `__syncRequire` binding is deliberately not a helper and never receives these flags. | Private `crate::factory::{EmitHelperName,NodeFactory::create_unscoped_helper_identifier,TransformArena::is_call_to_emit_helper}`; all current AST helper-reference producers in private `builtins.rs`, `class_fields/downlevel.rs`, `es2017.rs`, `es2018.rs`, `es_next.rs`, `system.rs`, `legacy_decorators.rs`, and `standard_decorators.rs`; specific-name consumer in private `class_fields/downlevel.rs`; category consumers in private module-transform call/tagged-template substitution in `builtins.rs` | `active-unqualified`; candidate-only audit 2026-08-14 | Current-code audit and zero plain-helper-producer audit; `emit_helper_name_distinguishes_user_calls_and_survives_factory_updates`; parameter-property source controls; full emitter and H2.5g inventory/profile for observable parity |
| `E-HELPERS-H` | ES2015/Generators helper IR and dependencies are not yet approved. | No approved new symbol | `planned`; H2.5h-a | pinned helper graph and direct mirror controls |
| `E-PRINTER-BASE` | Printer has no direct checker dependency. It consumes transformed syntax/metadata, drives retained substitution and before/after notification hooks, and applies immutable structural planning. | `tsc_emitter::Printer` (public); `tsc_emitter::TransformationResult::{substitute_node,before_emit_node,after_emit_node}`; `crate::printer::EmissionPlan` and `ExpressionEmissionContext` (private); `crate::factory::NodeFactory::apply_parenthesizer_rules` (private) | `active-unqualified`; candidate audit 2026-08-14. H2.5f protects the predecessor subset. | H2.5g profile; later packets preserve hook composition and enumerate new expression contexts |
| `E-PRINTER-G` | H2.5g keeps a simple arrow in tsc's ordinary `Parameters` list phase: private `ParameterListParentheses` removes only the `Parenthesis` format bit, so intervening/leading/element-end comment phases and the parameter's ordinary node substitution/notification pipeline still run. Only a retained `EqualsGreaterThanToken` uses the arrow-specific comments adapter. That adapter queries the typed effective `EmitPipelineHooks` capability and returns release-safe `PrinterError::RetainedArrowTokenPipelineHooks` when substitution or notification would change the required phase order. | Private `crate::printer::{ParameterListParentheses,Printer::{emit_arrow_parameter_list,emit_parameter_list_with_parentheses,emit_retained_arrow_token_with_comments}}`; `crate::transform::{EmitPipelineHooks,TransformationResult::emit_pipeline_hooks}` (`pub(crate)`); public `tsc_emitter::PrinterError::RetainedArrowTokenPipelineHooks`; private `ExpressionEmissionContext` additions | `active-unqualified`; candidate-only audit 2026-08-14 | Current-code audit plus the `simple_arrow_parameter_*` and `retained_arrow_*` focused contracts; H2.5g inventory/profile for observable parity |
| `E-COMMENTS-G` | Parsed ownership, synthetic comments, relocated owners, token progress, and comment progress are distinct typed facts. `TokenEmission` may hand its cursor plus an optional `CommentResume` only to the immediately following token/child; the resume carries both owner start and next position, requires one source and monotone progress, and can merge only with the same owner. A retained arrow anchors trailing comments at its own comment-range end, not semantic-original provenance, so a synthetic token cannot borrow source comments. The simple parameter's list-owned trailing comments and the retained arrow token's trailing comments remain separate phases, including tsc's observable list/leading replay at a shared range start. Token/comment ownership performs no source-wide token search; the position-cursor contract keeps local source work linear. | Public `tsc_emitter::SyntheticComment`; `crate::metadata::{RelocatedStatementListComments,RelocatedTrailingCommentOwner}` (`pub(crate)`); `crate::comment_cursor::{CommentCursor,CommentResume,CommentResumeError}` and `crate::token_cursor::{TokenCursor,TokenEmission,TokenAnchor,TokenCommentBoundary,FixedToken,TokenWriteKind,TokenLeadingSpace}` (`pub(crate)`); private printer comment/list workers | `active-unqualified`; candidate-only audit 2026-08-14 | Current-code audit, token/comment cursor unit contracts, `position_cursor_2727_statement_work_is_linear_and_scan_free`, and arrow replay/resume integration contracts; H2.5g inventory/profile for observable parity |
| `E-COMMENTS-H` | ES2015 wrapper/loop/generator relocations have no approved owner/resume matrix. | No approved new symbol | `planned`; H2.5h-a | packet maps every generated-node category and transition |
| `E-POSITIONS` | Source bytes, source/generated UTF-16, synthetic ranges, and source switches remain typed domains. | Public `tsc_emitter` position/writer/hook types; definitions in private `position`, `writer`, `metadata`, and `printer` modules | `active-unqualified`; candidate audit 2026-08-14. H1/H2.5f protect predecessor behavior. | H2.5g profile and H1 Unicode/newline controls |
| `E-MAPS` | Hook/range seams exist, but actual source-map generation is not compatible. | `tsc_emitter::{SourceMapRange,SourceMapRecorder,DisabledSourceMapRecorder,SourceMapHookEvent,SourceMapHookPhase,SourceMapObservation}` (public) | `dormant`; candidate audit 2026-08-14 | H2.6 activation and map evidence |
| `E-STRINGS` | JavaScript string values preserve UTF-16 code units; lexical spelling and synthesized cooked values have distinct provenance. | Public `tsc_emitter::JavaScriptString` and `EmitMetadata` accessors/setters; `pub(crate)` metadata fields for cooked value, quote choice, and text-source identity; private printer quote routines | `active-unqualified`; candidate audit 2026-08-14 | H2.5g profile and literal/JSX/module controls |
| `E-OUTPUT-SCRIPT` | JavaScript artifacts are constructed before the first sink callback; callback order and `emittedFiles` stay independent. | `tsc_emitter::{emit_files_with_activity,EmitArtifact,MemoryOutputSink,FsOutputSink,EmitOutcome}` (public) | `active-unqualified`; candidate audit 2026-08-14 | H2.5g profile and H1 sink-failure controls |
| `E-OUTPUT-FUTURE` | Multi-product write/report ordering remains non-compatible. | Dormant product arms in public artifact/plan/outcome types | `dormant`; candidate audit 2026-08-14 | H2.6/H2.7/H2.8 activation |

### 4.1 Current class-lowering identities and phases

This subsection records the H2.5g candidate that is present now. It is the
Rust ownership model corresponding to tsc's `getClassFacts`, new-class lexical
environment, heritage visitor, class-expression visitor, class-alias
substitution, and named-evaluation paths. It is not an instruction to revive
an older emitter design.

The implementation deliberately keeps these runtime roles distinct:

| Runtime role | Current Rust carrier and owner | Invariant |
| --- | --- | --- |
| Publication receiver | `DecoratedClassDeclarationExpansion::public_receiver`, discovered from the current transformed variable declaration | This is the replaceable local that receives relocated static operations and is later assigned the decorated value. Its spelling is not the anonymous class's language-level name and it is not automatically a stable pre-decoration identity. |
| Class-definition identity | `ClassConstructorReferencePlan` records the semantic causes; `DownlevelClassVisitor::allocate_class_constructor_identity` selects `class_definition_binding`. Materialization stores it as `PrivateEnvironment::class_alias`, and `ClassFieldsTransformer::class_aliases` makes constructor-reference substitution consume the same `ClassBinding`. | It is allocated before heritage and member-name temporaries when any semantic cause requires it: explicit `class_this`/named evaluation, static private or auto-accessor state, an instance-private constructor reference, or non-legacy static lexical `this`/`super`. A supplied standard-decorator `class_this` is reused instead of allocating a new identity. |
| Legacy initializer alias | `DecoratedClassDeclarationExpansion::initializer_receiver`, recovered from assignment wrappers already produced by `transformLegacyDecorators` | This alias belongs to the earlier transform and is registered for resolver-backed constructor-reference substitution. It neither supplies the anonymous runtime name nor makes legacy static `super` valid, and it is not silently conflated with the public receiver or a newly selected definition identity. |
| Actual superclass capture | `PrivateEnvironment::super_alias`, allocated by `allocate_super_base_binding` and installed by `capture_super_base` | A lexical-super fact is only a need fact. A binding is allocated at the tsc heritage-owner point, and only for a real `extends` expression in a non-legacy-decorated class. A propagated fact from a nested class cannot steal a temporary from an outer base class. |
| Late expression-result temporary | The fallback `expression_binding` allocated with the already selected `ClassTempPlan` after `plan_members` | This exists only when an ordinary class expression still needs an ordered comma sequence and no semantic constructor identity was required. It is intentionally late, so computed keys/private members retain tsc's allocation order. A decorated declaration normally uses its publication channel instead. |
| Assigned runtime name | `AssignedClassName::{Literal,Evaluated}` and `DownlevelClassVisitor::assigned_class_names` | This is the second argument of `__setFunctionName`, not a JavaScript binding. An evaluated computed key owns a stable expression read; a literal owns cooked text. Neither variant can be reconstructed from a generated binding's printable name. |

The source scans are allocation-free. `ClassFactsPlan` observes the original
member categories before auto-accessor expansion, while
`PrivateEnvironmentPlan` scans the expanded private declarations and owns
their slot events. This prevents one source auto accessor, whose redirectors
retain original-node provenance, from being counted several times by the
`getClassFacts` equivalent. Once both plans exist, observable generated-name
allocation follows these phases:

1. allocate the instance-private method/accessor brand;
2. consume `ClassConstructorReferencePlan` and allocate or reuse the semantic
   class-definition identity;
3. allocate the superclass binding only while handling an actual `extends`
   expression;
4. materialize private field/method/accessor slots in declaration-event order;
5. visit members and allocate computed-key and other member-local
   temporaries; and
6. if still necessary, allocate the ordinary class-expression result
   temporary.

#### Ordered class pending evaluation and placement

`ClassPendingPlan` is the current Rust representation of the class-fields
`pendingExpressions` channel; it is not the older design-time split between
setup and key vectors. `plan_members` creates its prefix in three tsc phases:
all ordinary instance-private field storages in declaration order, the one
optional instance-method/accessor brand, then all generated instance
auto-accessor storages in declaration order. The subsequent member walk
appends private method/getter/setter definitions and erased public-field key
operands in source arrival order. Before insertion, it applies tsc's
`flattenCommaList` rules to the final evaluation expression: an uncached
`a(), b()` becomes two events, while a cache assignment
`_a = (a(), b())` remains one. These categories may interleave and are never
regrouped during materialization.

`inject_pending_expressions_into_member` drains every item accumulated so far
into the next retained computed member name, followed by that member's own key.
Remaining entries have consumer-specific placement:

| Consumer | Required placement |
| --- | --- |
| Class declaration | Emit the class first; prefix the remaining stream with any constructor-alias assignment at this consumer only; emit the result as one comma-expression statement; then emit static operations and any split default export. |
| Ordinary class expression | Emit class assignment, each remaining pending operand, static operands, and the final class-result binding in one ordered expression. |
| Legacy-decorated or standard-decorator statement owner | Keep the class in its variable initializer, then emit every remaining pending entry as its own following `ExpressionStatement`, then named-evaluation/static/decorator work. |

`ClassPendingEntry` deliberately has no constructor-alias variant: aliasing is
a declaration-consumer prefix, not a producer event. Static materialization
also cannot accept a pending plan, which makes the required ordering explicit
in the type-level call graph.

The semantic owners below are separately pinned because no one broad span or
single function hash represents this stream. Hashes cover the exact inclusive
source-line slice from vendored TypeScript 6.0.3, including the trailing
newline; function-body inventory hashes are a different artifact.

| tsc owner | Exact span | Source-line-slice SHA-256 |
| --- | --- | --- |
| `flattenCommaListWorker` | `_tsc.js:28218-28231` | `a879551d103899488f8f2dbe2ca28ab980ecb860b8871916a3ad6958c3d274d2` |
| `flattenCommaList` | `_tsc.js:28232-28236` | `54ff61c10aeb5c3e6cbf988835f018eff0271a90a921dec763606dbc907f86cb` |
| `injectPendingExpressions` | `_tsc.js:96167-96179` | `5ba282b28c8f6b724f359b12b848c573fa0c2218cd12f4619475d3d22596d54e` |
| `visitComputedPropertyName` | `_tsc.js:96180-96183` | `c82affe1bb42c8ede4f24eac8cbb1eec3bcc5bfaff55984ec555bc3ca2fafe3b` |
| `visitMethodOrAccessorDeclaration` | `_tsc.js:96195-96225` | `da646c56cd8aaf5be0986400ea25fc55ac15f129de7aa3ea6cff07ec3749dcc9` |
| `transformPublicFieldInitializer` | `_tsc.js:96340-96376` | `e72c55aad0e213de5657c51c1e4c95dfd15c8b6ae3ff8d229fddcfef20e43d72` |
| `visitClassDeclarationInNewClassLexicalEnvironment` | `_tsc.js:96971-97045` | `07a4943badefc9b5d6d774a2d04dac4f3803e24852f8410d2bb735feef6fd6d7` |
| `visitClassExpressionInNewClassLexicalEnvironment` | `_tsc.js:97049-97129` | `5885e805a286e1451a1c60771127ff84a6c108f88522eb2f90901c2703763319` |
| `transformClassMembers` | `_tsc.js:97143-97237` | `8f02dc71f423a197caae79451edbed69e643ef5b909248bf13a649c2c2491071` |
| `createBrandCheckWeakSetForPrivateMethods` | `_tsc.js:97238-97252` | `0f8e90657191cb048755f0d11736264b42f33ac8a3d774c92751fc37652d8677` |
| `addPrivateIdentifierPropertyDeclarationToEnvironment` | `_tsc.js:97678-97707` | `76da1026f05a65b21788b59c156ca67e10008c6894a9989de475132bb70529ca` |

For a class expression, `ClassTempPlan` selects the owner once from
`NodeCheckFlags::BLOCK_SCOPED_BINDING_IN_LOOP`; both an early semantic identity
and the late fallback consume that same decision. Class declarations use the
explicit hoisted plan, and private declarations retain their own
resolver-derived owner. `LoopBindingScopes` accepts a
`LexicalBindingOwner::CurrentLoop` binding only while the immediate iteration
frame is live, inserts its `let` declaration in that loop body, and has a hard
function boundary. Otherwise `claim_lexical_binding_owner` records it as
`Hoisted`. The scope guards truncate to the saved depth on every exit.

Static `this` and static `super` are related but not one capability:

| Class state | Lexical `this` | Relocated static `super` |
| --- | --- | --- |
| Ordinary class with a definition identity and actual captured base | `StaticReceiver::Bound` reads the definition identity. | `StaticSuperPolicy::Available` permits `Reflect.get`/`Reflect.set` with the captured base and definition receiver. |
| Ordinary class with only a propagated super fact and no actual `extends` expression | The propagated class fact can still select a constructor identity, matching `getClassFacts`. | No additional superclass binding is allocated; the class-field pass does not invent a capture or rewrite from the propagated fact alone. |
| Legacy-decorated class with a definition identity from another cause | The bound definition identity remains valid for lexical `this`. | `StaticSuperPolicy::InvalidLegacyDecorated` still lowers the property base through tsc's invalid-super recovery. Calls/tags use the available identity only as the `.call(...)`/`.bind(...)` receiver. |
| Legacy-decorated class without a definition identity | `StaticReceiver::InvalidLegacyDecorated` recovers as `void 0`. | Invalid-super recovery has no call/tag receiver to preserve. |

`StaticBindingFrames` makes these decisions lexical: arrows inherit the active
static-evaluation frame, ordinary functions stop it, nested classes install a
class boundary, and a computed class name explicitly reads the enclosing
class evaluation. `StaticBindingFrameGuard` prevents any receiver or super
policy from leaking into a sibling subtree.

Named evaluation also has an explicit cross-pass route. `es_next.rs` and
`standard_decorators.rs` transport assigned-name/class-this facts in
`EmitMetadata`; `legacy_decorators.rs` writes
`ClassExpressionDeclarationOrigin::LegacyDecorated` on its synthesized class
expression. Class-field lowering builds `OriginalTreeOwnership` from the
current transform tree for assignment/statement ownership rather than using
stale parser parents. For an anonymous legacy default declaration it then
follows `TransformArena::get_original_node` through the complete declaration
chain, so the runtime name remains `"default"` even when an earlier pass
created a publication local such as `default_1`. `AssignedClassName` preserves
literal versus evaluated computed-key provenance, and
`DecoratedClassDeclarationExpansion` keeps the statement owner, publication
receiver, and optional legacy initializer alias separate. It leaves ordered
pending placement to `E-CLASS-PENDING-G`, then places named-evaluation and
static operations after that stream and before the later decoration
assignment.

#### Three ordered class/decorator lanes and the parameter-property bridge

The current implementation has three independent ordered lanes. They meet at
consumer-specific placement boundaries, but an entry never migrates between
their plans:

1. `ClassPendingPlan` orders effects evaluated while the class definition is
   built, such as private-definition setup and erased computed field keys.
2. `ClassPendingDecoratorInitializers` orders only member `addInitializer`
   extras at instance or static initialization boundaries after member
   decoration has populated their arrays.
3. `ClassDecorationPlan::extra_initializers_name` identifies the class
   decorator's finalizer array. Class decoration populates it, and a dedicated
   `__runInitializers` statement consumes it exactly once after residual static
   member extras.

The member lane owns two queues selected by
`DecoratorInitializerPlacement`. `DecoratorInitializerReceiver` records
whether materialization uses lexical `this` or a class binding. Instance
member extras always use lexical `this`; static member extras use
`ClassDecorationPlan::class_this_name` when a class decorator can replace the
class, and otherwise use lexical `this`. `PendingDecoratorInitializer`
distinguishes method extras from field extras, while
`PendingDecoratorInitializerBatch` couples a drained ordered batch to its
receiver. The member queues follow this lifecycle:

1. method/accessor extras seed the instance or static queue before walking
   source members;
2. every `PropertyDeclaration`, decorated or ordinary and initialized or
   uninitialized, drains its placement queue into that property's value;
3. a decorated field enqueues its own field-extra initializer only after that
   drain;
4. a runtime static block drains the static queue into a preceding synthetic
   block; and
5. remaining instance work is injected into the constructor, while remaining
   static member work uses tsc's `hasStaticInitializers` placement decision.
   That fact is computed from the original/pre-injection member initializer: a
   pending method extra that gives an otherwise uninitialized static property
   a value does not retroactively promote it.

The class-finalizer lane always targets the replacement-capable
`class_this_name`, never the lexical receiver of an unrelated member plan.
`create_class_decorate_statement` is its sole producer in the current Rust
path. `create_decoration_block` consumes it in the leading block when the
pre-injection `has_static_initializers` fact is false; otherwise
`transform_class_expression` appends it to the trailing block after the
drained static member batch. Neither route also queues it, so the finalizer is
run exactly once.

Constructor insertion has three current Rust branches, all of which search a
typed statement-index path through nested `try` blocks and insert immediately
after the first reachable direct `super()` statement:

| Branch | Current Rust path owner | Transparent outer expressions |
| --- | --- | --- |
| Standard-decorator residual instance member extras | `DecoratorConstructorSuperPath`; `StandardDecoratorVisitor::{find_constructor_super_path,inject_constructor_statement_at_super_path,statement_is_super_call,skip_parenthesized_expression}` | Consecutive `ParenthesizedExpression` nodes only. |
| ES2022+ assignment-mode moved fields and parameter properties | `class_fields::SuperStatementPath`; `ClassFieldsVisitor::{find_super_statement_path,inject_initializers_at_super_path,statement_is_super_call,skip_parenthesized_expression}` | Consecutive `ParenthesizedExpression` nodes only. |
| ES2015-ES2021 downlevel fields and parameter properties | `class_fields::downlevel::SuperStatementPath`; `DownlevelClassVisitor::{find_super_statement_path,inject_initializers_at_super_path,statement_is_super_call,skip_runtime_transparent_outer_expressions}` | Parentheses plus the TypeScript-erasure wrappers that can still reach this later branch (`PartiallyEmitted`, type assertion, `as`, `satisfies`, non-null, and expression-with-type-arguments). It still does not cross comma or another runtime evaluation boundary. |

The downlevel branch's broader helper is intentional composition handling,
not a broader definition of a direct super statement. The other two branches
match tsc's `getSuperCallFromStatement` `skipParentheses` boundary directly.

All three branches obtain their top-level start from the current shared
`ConstructorPrologue`. It scans consecutive string directives first and only
then consecutive statements carrying `EmitFlags::CUSTOM_PROLOGUE`. A string
or flagged statement after that contiguous pair is ordinary body syntax; the
implementation deliberately does not use a single `standard || custom` loop.

The standard-decorator no-super branch preserves a non-obvious TypeScript
6.0.3 observable. Upstream first copies the prologue, appends the residual
initializer, and then visits the complete original body from offset zero, so
both standard and custom prefix statements occur twice. The current Rust
constructor subtree has already been visited when its residual queue becomes
known. `DecoratorConstructorInitializerPlacement::ReplayPrologueThenBody`
therefore builds `[visited prefix, initializer, all visited statements]` with
the same `TransformNode` identities instead of rerunning a visitor or cloning
metadata. A direct or nested reachable `super()` takes the other typed branch,
retains the prefix once, and inserts immediately after that call.

Parameter properties cross three transforms. The TypeScript transform first
projects a `PropertyDeclaration` whose original-node chain reaches the source
`Parameter`. Standard decorators may then attach a pending initializer prefix
to that property and must preserve the chain. Class-fields lowering recognizes
the parameter identity through the full chain and keeps the constructor local
as typed data rather than reconstructing it from printed text.

| Target and field mode | Current materialization |
| --- | --- |
| ES2015-ES2021, assignment or define | `FieldValuePlan::ParameterProperty { prefix, local }` is materialized in constructor scope as `(prefix, local)` (or `local`), and the earlier TypeScript parameter assignment is replaced. |
| ES2022+, assignment (including ESNext) | `MovedFieldValuePlan::ParameterProperty` supplies the same constructor-scoped value and `ParameterPropertyAssignmentPolicy::Replace` removes the earlier assignment. |
| ES2022 through pre-ESNext, define/native | The native field retains only its decorator helper prefix; the TypeScript-produced `this.x = x` remains in the constructor. A field initializer never reads a constructor local. |
| ESNext, define | The standard-decorator transform is omitted. `ClassFieldsTransformer` remains registered but its root gate is a no-op, so the bridge is inactive. |

In the ES2015-ES2021 branch,
`DownlevelClassVisitor::normalize_parameter_property_prefix` removes only the
decorator-created `(__runInitializers(...), void 0)` fallback wrapper before
combining the prefix with the constructor local. It does not generically
flatten user comma expressions or recover a local from printed text. The
callee must have been created through
`NodeFactory::create_unscoped_helper_identifier(EmitHelperName::RunInitializers)`;
`TransformArena::is_call_to_emit_helper` requires `HELPER_NAME` and the exact
escaped spelling. A parsed or ordinary synthetic same-spelling call therefore
retains its `void 0` operand and cannot be mistaken for decorator provenance.

The semantic owner pins use SHA-256 over the exact inclusive source-line slice
from vendored TypeScript 6.0.3, including its trailing newline. Broad transform
root or function-body hashes do not substitute for these entries.

| Phase | tsc owner and exact span | Source-line-slice SHA-256 |
| --- | --- | --- |
| Parameter-property projection | `transformClassMembers`, `_tsc.js:94564-94598` | `306e5388a9a5c510a3594d97b7fbe7bf945415e4f4601770e266d55ce28765f8` |
| Queue creation and seed facts | `createClassInfo`, `_tsc.js:99241-99318` | `2457b6249522b8b9349b000eff2caf2d626295f98eedc6ce759c1291f8004185` |
| Class-finalizer allocation | `transformClassLike` class-plan allocation, `_tsc.js:99344-99364` | `cc63e7e5a08da6f16ac6b79dece72553e777487b91623ff131a7383595bb36d0` |
| Class-finalizer producer and final placement | `transformClassLike` class decorate/final placement, `_tsc.js:99488-99528` | `e7c279d7ef714e4c9b18693ddd733b1473d97bff6f71b3e894a60b9ba36ffe91` |
| Class-level queue placement | `transformClassLike`, `_tsc.js:99319-99616` | `7199607733dc27e3d53faa0e8e37a065b7ec4ae8f2fdf154d925291fa23f61df` |
| Direct/nested-super discovery shared by the three Rust placement branches | `getSuperCallFromStatement` and `findSuperStatementIndexPath`, `_tsc.js:93070-93093` | `14dfc2d8ccf6dcb0d10be798e055c204560e8561e32e5851d32e1a18703f2201` |
| Constructor standard/custom prologue boundary | `copyPrologue`, `copyStandardPrologue`, and `copyCustomPrologue`, `_tsc.js:24827-24869` | `555445a3fd02a4b53bbc05f05e48729ca0f7208892d66dbc7985f51f3e897a8e` |
| Residual instance preparation | `prepareConstructor`, `_tsc.js:99747-99758` | `2a79ab99613abecdfd7e854650bbaac5f5b831bde37c6c0a45fd71d923d79954` |
| Nested-super constructor worker | `transformConstructorBodyWorker`, `_tsc.js:99759-99787` | `aaf0c5324b33bbc52730bda4f4a77db2c952a35f0f18f78dafe9750923fd9c12` |
| Constructor consumer | `visitConstructorDeclaration`, `_tsc.js:99788-99823` | `c5fbc638b5cdc3d6b829a1354f0c0eaafd8821813e4634bc9ca8c45426b49c61` |
| Static-block consumer | `visitClassStaticBlockDeclaration`, `_tsc.js:100005-100040` | `5ba6f2d5e5b218a418e3ca67a6714022b5a77e460c16e042d950b765f0a6504a` |
| Universal property drain/enqueue | `visitPropertyDeclaration`, `_tsc.js:100041-100150` | `32896629db3e477cdb54934eeefadb12c80ac10d8909d811eb7a90e2de4b164d` |
| Pending-expression composition | `injectPendingExpressionsCommon`, `_tsc.js:100511-100526` | `0409cc30806f5998022df21eceb6369af27f21778e795597936eddc5350f379b` |
| Queue drain | `injectPendingInitializers`, `_tsc.js:100535-100545` | `dee2ea62ca9186b228bf257a5bf9a171193f1b637d16426df8d8b7713e7cd8d5` |
| Class-fields nested-super consumer | `transformConstructorBodyWorker`, `_tsc.js:97290-97328` | `37e090fcc937a5c99a0fce3410f7d5a67fd9612316d31ef64b3dba2d7212ad4a` |
| Class-fields constructor placement | `transformConstructorBody`, `_tsc.js:97329-97431` | `6ab03601cab55c7af832a1cec8e17a822e21aa330f32a65b2b79637c4765c9f3` |
| Class-fields property materialization | `transformPropertyWorker`, `_tsc.js:97501-97575` | `fb5e7b8fdfc4fab54f8fdd4ea6f48902c80207af52647e23cb47491f0ce46edd` |
| Typed unscoped helper producer | `getUnscopedHelperName`, `_tsc.js:25526-25528` | `4eccb820e726db854c379fb20072e2506d22a8caa82b367dcd88168334c0936e` |
| `__runInitializers` helper call producer | `createRunInitializersHelper`, `_tsc.js:25715-25723` | `ac7241f25e6f4d82e533ae048fbe9de24149093224ff8713b1483e39c8798e68` |
| Helper-call provenance predicate | `isCallToHelper`, `_tsc.js:26566-26568` | `65c471809533a93e4ad2d44931471cb8a169cf9c93c9b291bc7a7dbdeede8fef` |

### 4.2 Upstream correspondence

The current primary tsc owners are listed here for routing. Their exact spans
and hashes live on the corresponding Rust ledger comments and in the owning
slice artifacts; a packet must read and pin those identities afresh.

| Architecture rows | Primary TypeScript 6.0.3 owners |
| --- | --- |
| `E-ENTRY`, `E-OUTPUT-*` | Program `emit`/`emitWorker`, `handleNoEmitOptions`, `emitFiles`, `writeFile`, `writeFileEnsuringDirectories` |
| `E-PROTOCOL`, `E-RESOLVER-*`, `E-CHECKER-FACTS-*` | Program `getEmitHost`, `getEmitResolver`, `createResolver`, and the exact checker/binder producers reached by each resolver query |
| `E-PLAN-*` | `getSourceFilesToEmit`, `sourceFileMayBeEmitted`, `getOutputPathsFor`, `forEachEmittedFile` |
| `E-ARENA`, `E-METADATA-*` | `getParseTreeNode`, `getOriginalNode`, `setOriginalNode`, `mergeEmitNode`, `cloneNode`, `propagateChildFlags` |
| `E-METADATA-G-CLASS`, `E-CAPTURE-CLASS-G`, `E-NAMES-CLASS-G` | `getClassFacts`, `visitInNewClassLexicalEnvironment`, `visitClassDeclarationInNewClassLexicalEnvironment`, `visitClassExpressionInNewClassLexicalEnvironment`/`createClassTempVar`, `visitExpressionWithTypeArgumentsInHeritageClause`, `visitInvalidSuperProperty`, `substituteThisExpression`, `trySubstituteClassAlias`, `getAssignedName`/`transformNamedEvaluation`, and the legacy/standard decorator producers that write class coordination metadata |
| `E-CLASS-PENDING-G` | The separately pinned producer, drain, and declaration/expression consumer owners in section 4.1; no combined owner name or enclosing `transformClassFields` hash substitutes for those identities |
| `E-DECORATOR-INITIALIZERS-G`, `E-DECORATOR-PARAMETER-PROPERTY-G` | The separately pinned standard-decorator member-queue, two-phase prologue, direct/nested-super discovery, constructor, helper-provenance, and class-fields bridge owners in section 4.1, plus TypeScript's parameter-property projection; no broad `transformESDecorators` hash substitutes for the drain, enqueue, constructor, or helper owners |
| `E-DECORATOR-CLASS-INITIALIZERS-G` | `createClassInfo` plus the separately pinned `transformClassLike` class-plan allocation and class-decorate/final-placement spans in section 4.1; this finalizer is not inferred from the member-queue owner |
| `E-JSX-FACTORY-G` | `transformJsx` classic element/fragment visitors, `createReactNamespace`, `createJsxFactoryExpressionFromEntityName`, and the TypeScript/module substitution owners reached from their synthesized factory root |
| `E-CONTEXT`, `E-CAPTURE-BASE` | `transformNodes` and its transformation-context closures, plus the active `transformES2017` lexical/capture owners |
| `E-ORDER-*` | `getTransformers`, `getScriptTransformers`, and each registered transform root |
| `E-NAMES-*`, `E-HELPERS-*` | Generated-name, lexical-environment, helper-request, helper-reference provenance, and helper-emission dependencies reached from the active transform roots; each slice regenerates the exact closure |
| `E-PRINTER-*`, `E-COMMENTS-*` | `createPrinter`, `emitParametersForArrow`, `emitList`/`emitNodeListItems`, `pipelineEmitWithComments`, their token/comment closures, and `createParenthesizerRules` |
| `E-POSITIONS`, `E-MAPS` | `createTextWriter`, source-map range accessors, and `createSourceMapGenerator` when H2.6 activates it |
| `E-STRINGS` | Literal/factory/printer paths reached by each active transform; exact owners are per-branch and must not be collapsed to one generic string helper |
| `E-SYNTAX-FACTS` | scanner/parser token-flag capture, node creation, clone/update, incremental copy/equality, and every transform-flag consumer reached by the admitted owner graph |

## 5. Current pass order

The H2.5g candidate selects the following order in
`get_script_transformers_with_optional_host`; option/target gates may omit
optional entries, but remaining entries do not reorder:

1. TypeScript syntax transform;
2. legacy decorators when enabled;
3. JSX when enabled;
4. ESNext;
5. standard decorators when enabled;
6. class fields;
7. ES2021;
8. ES2020;
9. ES2019;
10. ES2018;
11. ES2017;
12. ES2016;
13. the selected Preserve, System, CommonJS/AMD/UMD, or ESM module transform.

This list is a current Rust fact, not the authority for adding the next pass.
H2.5h must first pin `getScriptTransformers` and the ES2015/Generators owner
graph, then define the exact ES2015 -> Generators -> module composition and
the point at which flags and generated names are recomputed/finalized.

## 6. Open architectural constraints

These are design blockers, not a queue of local fixes.

### `EA-GAP-FLAGS` — exact recomputable transform facts

The current classifier and update logic are embedded in `builtins.rs`.
`TransformArena::propagate_child_flags` and subtree exclusions are reusable,
but H2.5h cannot inherit old ES2015/Generator/Yield bits through a partial mask.
The foundation packet must:

- inventory every reachable tsc local/subtree flag producer and exclusion;
- persist the required lexical scanner/parser facts, including template token
  flags and extended-Unicode identifier facts, through clone and incremental
  reuse;
- define one classifier shared by full postorder initialization and full
  recomputation of a changed node; and
- prove composition after every earlier transformer that synthesizes a node
  consumed by a later pass.

### `EA-GAP-CAPTURE` — lexical receiver and binding ownership

The active target transforms already have the scoped local plans recorded by
`E-CAPTURE-BASE` and `E-CAPTURE-CLASS-G`, but those plans are not a complete
ES2015 model. ES2015 requires one typed account of hierarchy facts, `this`,
`arguments`, `new.target`, `super`, converted loops, catch scopes, and captured
bindings.
The packet must map each tsc state producer/updater/consumer onto the current
Rust frames/plans or an explicitly approved extension and add the missing
resolver collision/capture queries. Printable names and parent guessing are
not semantic facts.

### `EA-GAP-COMPOSITION` — ES2015 and Generators

ES2015 and Generators are not current transformer entries. Their owner graph
must determine whether one runtime slice is truly dependency-closed. If the
graph contains independent owner SCCs, H2.5h receives further suffixes before
production implementation. The packet must state hook composition, generated
binding finalization, provenance for every wrapper/state-machine node, comment
owner/resume transitions, expression contexts, and typed helper IR.

### `EA-GAP-MAPS-DECLS` — dormant products

The recorder hooks and output slots deliberately exist, but
`DisabledSourceMapRecorder`, declaration printing, bundle roots, declaration
maps, and build info are not compatibility. H2.6 and H2.7 must re-audit that
all source/original/token/comment facts survived intervening H2 transforms;
the existence of a dormant enum arm is not evidence.

## 7. Adjacent and historical source disposition

| Document | Current role | Use in a slice |
| --- | --- | --- |
| [H0 no-emit/CLI](noemit-cli.md) | Frozen H0 contract and design history for the read-only host, no-emit entry, and CLI canaries. | Use only as the predecessor contract for `E-ENTRY` and the no-emit non-regression boundary; resolve current host/API shapes from code and this architecture map. |
| [H1 emit](h1-emit.md) | Frozen H1 claim, lineage, and original design rationale. | Read only the predecessor invariants named by the packet. Validate them against this page and current symbols before relying on them. |
| [Compiler compatibility residual](compiler-compatibility-residual.md) | Audited surface/owner inventory and historical gap analysis. Its implementation-state prose can age. | Use its owner lists as research input, then regenerate the local gap matrix from current code. |
| [Post-H1 slices](post-h1-completion-slices.md) | Current execution order and design/acceptance gate. | Mandatory for every active slice. It does not define Rust architecture. |
| [Core interfaces](../core-interfaces.md) | M-track core data-contract lineage. | Use only validated shared Node/Symbol/Type facts; it does not own emitter-specific types. |
| [tsc source guide](../tsc-source-guide.md) | Navigation technique and historical checker index. | Use the technique, not old line numbers; packets pin current vendored spans/hashes directly. |
| [Architectural debt](../architectural-debt.md) | v1 historical debt. | Not a current backlog. Current emitter constraints live in section 6 with a slice owner. |
| [Legacy execution guide](../EXECUTION-GUIDE.md) | v1 historical operating procedure. | Do not use for the current workspace. Follow the greenfield entry and slice packet. |
| [2XXX emitter inventory](2xxx-emitter-inventory.md) and [descriptions](2xxx-emitter-descriptions.md) | Historical M-track inventory of functions that emit 2XXX diagnostics; despite their filenames, they are not JavaScript-emitter designs. | Do not use them for current emitter architecture or implementation. |

No historical statement is promoted by copying it into a new packet. Promotion
requires current-code validation, an architecture row update, and the exact
evidence named by the owning slice.
