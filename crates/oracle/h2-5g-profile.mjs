import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const GENERATOR_PATH = fileURLToPath(import.meta.url);
const WORKSPACE = path.resolve(path.dirname(GENERATOR_PATH), "../..");
const GENERATOR_RELATIVE_PATH = "crates/oracle/h2-5g-profile.mjs";
const TARGET_RELATIVE_PATH = "ratchets/h2-5g-profile.v1.json";
const CONTRACT_RELATIVE_PATH = ".github/ci/contracts/h2-5g-profile.schema.json";
const QUALIFICATION_RELATIVE_PATH = "ratchets/h2-5g-qualification.v1.json";
const OWNER_CONTROLS_RELATIVE_PATH = "ratchets/h2-5g-owner-controls.v1.json";
const PARENT_PROFILE_RELATIVE_PATH = "ratchets/h2-5f-profile.v1.json";
const H2_1A_QUALIFICATION_RELATIVE_PATH = "ratchets/h2-1a-qualification.v1.json";
const H2_1A_QUALIFICATION_SHA256 =
  "3fddcb23babb181e9d4248b4898587477e68ec67043f2720d013fcea036cb95d";
const H2_1A_CURRENT_EXACT_PROMOTIONS = Object.freeze([
  Object.freeze({
    source_phase: "H2.1a",
    case_id: "typescript-6.0.3/compiler/arrayFromAsync.ts#default",
    historical_case_fingerprint_sha256:
      "9f63ee4777950bf7023052d1eef2c48a0fea492820217a9e4ff49cdc86da19aa",
    historical_disposition: "diagnostic-deferred-output-control",
    historical_diagnostic_state: "deferred-to-H2.9",
    current_disposition: "exact-required",
    exact_reported_diagnostics: 0,
    exact_writes: 1,
  }),
  Object.freeze({
    source_phase: "H2.1a",
    case_id:
      "typescript-6.0.3/compiler/arrayIterationLibES5TargetDifferent.ts#nolib%3Dtrue%2Ctarget%3Desnext",
    historical_case_fingerprint_sha256:
      "7d155578b5fa4353d81d2798cdc36d4d55bd5c892e2eec1b5580ad8d89f82292",
    historical_disposition: "diagnostic-deferred-output-control",
    historical_diagnostic_state: "deferred-to-H2.9",
    current_disposition: "exact-required",
    exact_reported_diagnostics: 11,
    exact_writes: 1,
  }),
  Object.freeze({
    source_phase: "H2.1a",
    case_id: "typescript-6.0.3/compiler/mapGroupBy.ts#default",
    historical_case_fingerprint_sha256:
      "f0bcdec1d79c70a608fcbbd5ae0629dc5637866d306c5fb1e5ba4a8e8fd371a5",
    historical_disposition: "diagnostic-deferred-output-control",
    historical_diagnostic_state: "deferred-to-H2.9",
    current_disposition: "exact-required",
    exact_reported_diagnostics: 0,
    exact_writes: 1,
  }),
  Object.freeze({
    source_phase: "H2.1a",
    case_id: "typescript-6.0.3/compiler/objectGroupBy.ts#default",
    historical_case_fingerprint_sha256:
      "5d4213b87de2c690084a684f590df4e2f4dd16f54789073916e347cd01f13d13",
    historical_disposition: "diagnostic-deferred-output-control",
    historical_diagnostic_state: "deferred-to-H2.9",
    current_disposition: "exact-required",
    exact_reported_diagnostics: 1,
    exact_writes: 1,
  }),
  Object.freeze({
    source_phase: "H2.1a",
    case_id:
      "typescript-6.0.3/compiler/regularExpressionScanning.ts#target%3Desnext",
    historical_case_fingerprint_sha256:
      "991c35eaee4cb7fd5a92b60fd696b3bc1046a36bab6bf25af873dae39ae6a428",
    historical_disposition: "diagnostic-deferred-output-control",
    historical_diagnostic_state: "deferred-to-H2.9",
    current_disposition: "exact-required",
    exact_reported_diagnostics: 193,
    exact_writes: 1,
  }),
]);
const TRUSTED_BASE = "11f5d0abb93fed4b109bdb1dc552721ceb05e707";

const HISTORICAL_AUTHORITIES = Object.freeze([
  ["profile", "ratchets/h2-5f-profile.v1.json", "cd751cd7e5e28186dd60d0654eeec3b87607358bf0715a910afae8fef9efb684"],
  ["qualification", "ratchets/h2-5f-qualification.v1.json", "7dadfe321be49079a18fdcebf6ca5bceaa7aeedd93f41cf30f9586caec162bb3"],
  ["owner_controls", "ratchets/h2-5f-owner-controls.v1.json", "a4d9f500be900a0e3f759ba3231a3db20f789f5dcf4b888137ca886686ce9469"],
  ["profile_generator", "crates/oracle/h2-5f-profile.mjs", "cd2c141a103d132a0b667f0916dbcae9aedb0e1c48a11df8184b81dd4bcec142"],
  ["qualification_generator", "crates/oracle/h2-5f-qualification.mjs", "5b26879d8b2bbb3c8d07a50f176bba0e08de438e70b9909c58a4fa63fa34b828"],
  ["owner_controls_generator", "crates/oracle/h2-5f-owner-controls.mjs", "8b922d23867a697345be2ef173815feb85bc4543a47f636d3db08eaaf6dfb80e"],
  ["profile_contract", ".github/ci/contracts/h2-5f-profile.schema.json", "5e57df22fab8c62dee892564090681afd48bfa2ec72d582356cf9ec1b99488ee"],
  ["qualification_contract", ".github/ci/contracts/h2-5f-qualification.schema.json", "562a98c418e649440fe3aaf7ed6ef52af185099fb09f27b41254cc9606b1f362"],
  ["owner_controls_contract", ".github/ci/contracts/h2-5f-owner-controls.schema.json", "b11e39f35381e8b24218dc6d4dac68b73de346330c68726cac5519e31da0a947"],
]);

// These files establish new runtime or direct acceptance ownership in H2.5g.
// Existing runtime inputs retain their parent order and receive fresh hashes;
// this append-only set is every non-oracle crate path changed from the trusted
// H2.5f merge that was not already part of the parent profile.
const NEW_RUNTIME_INPUTS = Object.freeze([
  "crates/xtask/tests/unit/h2_2c_acceptance/de_registry_contracts.rs",
  "crates/xtask/src/h2_6c_de_promotions.rs",
  "crates/xtask/src/h2_6c_refusal_migrations.rs",
  // Shared original-corpus acceptance and explicit legacy measurement.
  "crates/xtask/src/h2_7de_acceptance.rs",
  "crates/compiler/tests/integration/h2_7d_original_corpus_shared.rs",
  "crates/compiler/tests/integration/h2_7e_original_corpus_shared.rs",
  "crates/xtask/tests/unit/h2_2c_acceptance/de_legacy_collector.rs",
  // Parallel H2.7d/e foundations and ordinary nonbundle map candidate.
  // Formal runtime adoption and the remaining API/bundle boundaries stay separate.
  "crates/compiler/tests/fixtures/declaration-maps.json",
  "crates/compiler/tests/h2_7e_declaration_maps.rs",
  "crates/compiler/tests/h2_7e_original_corpus.rs",
  "crates/compiler/tests/h2_7e_declaration_map_apis.rs",
  "crates/compiler/tests/h2_7d_module_identities.rs",
  "crates/compiler/tests/h2_7d_declaration_bundles.rs",
  "crates/compiler/tests/h2_7d_bundle_program.rs",
  "crates/compiler/tests/h2_7d_original_corpus.rs",
  "crates/compiler/tests/h2_7d_bundle_sinks.rs",
  "crates/emitter/src/factory/parsed_metadata.rs",
  "crates/emitter/tests/fixtures/bundle-declaration-map-paths.json",
  "crates/emitter/tests/fixtures/bundle-maps.json",
  "crates/emitter/tests/fixtures/module-alias-underscores.json",
  "crates/compiler/tests/fixtures/bundle-sinks.json",
  "crates/compiler/tests/fixtures/declaration-reference-paths.json",
  "crates/compiler/Cargo.toml",
  "crates/emitter/src/declarations/bundle.rs",
  "crates/compiler/tests/fixtures/declaration-maps-disabled-declaration.json",
  "crates/emitter/src/declaration_map.rs",
  "crates/emitter/tests/fixtures/bundle-plan.json",
  "crates/emitter/tests/unit/bundle_plan/tests.rs",
  "crates/program/tests/h2_7d_bundle_source_facts.rs",
  "crates/compiler/tests/fixtures/declaration-map-apis.json",
  "crates/compiler/tests/fixtures/declaration-maps-runtime.json",
  "crates/emitter/tests/fixtures/bundle-module-identities.json",
  "crates/emitter/tests/fixtures/system-generated-names.json",
  "crates/emitter/tests/fixtures/bundle-declarations.json",
  "crates/emitter/src/printer/bundle.rs",
  "crates/emitter/tests/fixtures/bundle-printer.json",
  "crates/emitter/tests/unit/bundle_printer/tests.rs",
  "crates/emitter/src/external_module_names.rs",
  "crates/emitter/tests/unit/external_module_names/tests.rs",
  "crates/emitter/tests/fixtures/bundle-transform.json",
  "crates/emitter/tests/unit/bundle_transform/tests.rs",
  "crates/compiler/tests/fixtures/h2-7c-corpus-inputs.json",
  "crates/compiler/tests/integration/h2_7c_corpus.rs",
  "crates/xtask/src/h2_7c_acceptance.rs",
  // h2-7c: focused option observations and production-entry comparisons.
  "crates/compiler/tests/fixtures/strip-internal.json",
  "crates/compiler/tests/integration/h2_7c_strip_internal.rs",
  "crates/compiler/tests/fixtures/declaration-blocking.json",
  "crates/compiler/tests/integration/h2_7c_declaration_blocking.rs",
  "crates/compiler/tests/integration/cli_contract.rs",
  "crates/emitter/src/declarations/isolated.rs",
  "crates/compiler/tests/fixtures/isolated-declaration-inference.json",
  "crates/compiler/tests/integration/h2_7c_isolated_inference.rs",
  "crates/compiler/tests/fixtures/isolated-declaration-parameters.json",
  "crates/compiler/tests/integration/h2_7c_isolated_parameters.rs",
  "crates/compiler/tests/fixtures/isolated-declaration-accessors.json",
  "crates/compiler/tests/integration/h2_7c_isolated_accessors.rs",
  "crates/compiler/tests/fixtures/isolated-declaration-enums.json",
  "crates/compiler/tests/integration/h2_7c_isolated_enums.rs",
  "crates/compiler/tests/fixtures/isolated-declaration-expando-augmentation.json",
  "crates/compiler/tests/integration/h2_7c_isolated_expando_augmentation.rs",
  "crates/compiler/tests/fixtures/isolated-declaration-private-types.json",
  "crates/compiler/tests/integration/h2_7c_isolated_private_types.rs",
  "crates/compiler/tests/fixtures/declaration-dir.json",
  "crates/compiler/tests/integration/h2_7c_declaration_dir.rs",
  "crates/compiler/src/declaration_diagnostics.rs",
  "crates/compiler/tests/fixtures/declaration-getters.json",
  "crates/compiler/tests/integration/h2_7c_declaration_getters.rs",
  "crates/compiler/tests/fixtures/forced-declarations.json",
  "crates/compiler/tests/integration/h2_7c_forced_declarations.rs",

  // h2-7b-ca: W5 inputs omitted during the lightweight closure pilot.
  "crates/checker/src/inference.rs",
  "crates/checker/src/instantiate.rs",
  "crates/checker/src/jsdoc.rs",
  "crates/checker/src/mapped.rs",
  "crates/checker/src/widen.rs",
  "crates/checker/tests/unit/annotate/tests.rs",
  "crates/types/src/tables.rs",
  "crates/types/src/ty.rs",
  "crates/types/tests/unit/tables/tests.rs",

  // h2-6a-m-3: the runtime flip touched the emit error/outcome surfaces
  // (SourceMapRecordingUnavailable, the SourceMapObservation producer).
  "crates/emitter/src/error.rs",
  "crates/emitter/src/outcome.rs",
  "crates/checker/Cargo.toml",
  "crates/checker/src/access.rs",
  "crates/checker/src/check.rs",
  // h2-7b-w3: W3-A's T2 port (the inferred predicate parameter is unescaped at construction).
  "crates/checker/src/narrow.rs",
  // h2-7b-w4: W4-A's S4 port (property-assignment binding scope) and the
  // integrator's symlink cache (module specifiers name symlinked packages).
  "crates/binder/src/bind.rs",
  "crates/program/src/symlinks.rs",
  "crates/checker/src/class.rs",
  "crates/checker/src/constraints.rs",
  "crates/checker/src/contextual.rs",
  "crates/checker/src/declaration_emit.rs",
  // h2-7a-m-3: the dormant NodeBuilder foundation joins the checker
  // runtime-input closure (modules + relocated unit-test bodies).
  "crates/checker/src/node_builder/chains.rs",
  "crates/checker/src/node_builder/context.rs",
  "crates/checker/src/node_builder/mod.rs",
  "crates/checker/src/node_builder/serialize.rs",
  "crates/checker/src/node_builder/signatures.rs",
  "crates/checker/src/node_builder/specifier.rs",
  "crates/checker/src/node_builder/statements.rs",
  "crates/checker/src/node_builder/tracker.rs",
  "crates/checker/src/node_builder/type_nodes.rs",
  "crates/checker/src/syntactic_type_node_builder.rs",
  "crates/checker/tests/unit/declaration_emit_members/tests.rs",
  "crates/checker/tests/unit/node_builder_chains/tests.rs",
  "crates/checker/tests/unit/node_builder_core/tests.rs",
  "crates/checker/tests/unit/node_builder_serialize/tests.rs",
  "crates/checker/tests/unit/node_builder_signatures/tests.rs",
  "crates/checker/tests/unit/node_builder_specifier/tests.rs",
  "crates/checker/tests/unit/node_builder_statements/tests.rs",
  "crates/checker/tests/unit/node_builder_type_nodes/tests.rs",
  "crates/checker/tests/unit/syntactic_type_node_builder/tests.rs",
  "crates/emitter/tests/unit/factory_seams/tests.rs",
  "crates/emitter/tests/unit/node_builder_seams/tests.rs",
  "crates/checker/src/elaboration.rs",
  "crates/checker/src/expr.rs",
  "crates/checker/src/facts.rs",
  "crates/checker/src/functions.rs",
  "crates/checker/src/globals.rs",
  "crates/checker/src/indexed.rs",
  "crates/checker/src/jsx.rs",
  "crates/checker/src/links.rs",
  "crates/checker/src/literals.rs",
  "crates/checker/src/operators.rs",
  "crates/checker/src/program.rs",
  "crates/checker/src/relate.rs",
  "crates/checker/src/speculate.rs",
  "crates/checker/src/spell.rs",
  "crates/checker/src/state.rs",
  "crates/checker/src/statements.rs",
  "crates/checker/src/structural.rs",
  "crates/checker/src/unions.rs",
  "crates/checker/tests/unit/access/tests.rs",
  "crates/checker/tests/unit/annotate/alias_and_typeof_tests.rs",
  "crates/checker/tests/unit/annotate/late_binding_tests.rs",
  "crates/checker/tests/unit/annotate/mapped_type_tests.rs",
  "crates/checker/tests/unit/annotate/unique_symbol_tests.rs",
  "crates/checker/tests/unit/calls/tests.rs",
  "crates/checker/tests/unit/check/tests.rs",
  "crates/checker/tests/unit/constraints/tests.rs",
  "crates/checker/tests/unit/elaboration/tests.rs",
  "crates/checker/tests/unit/emit/tests.rs",
  "crates/checker/tests/unit/functions/tests.rs",
  "crates/checker/tests/unit/indexed/tests.rs",
  "crates/checker/tests/unit/inference/tests.rs",
  "crates/checker/tests/unit/jsx/tests.rs",
  "crates/checker/tests/unit/lib/tests.rs",
  "crates/checker/tests/unit/literals/tests.rs",
  "crates/checker/tests/unit/mapped/tests.rs",
  "crates/checker/tests/unit/modules/tests.rs",
  "crates/checker/tests/unit/operators/tests.rs",
  "crates/checker/tests/unit/program/tests.rs",
  "crates/checker/tests/unit/relate/tests.rs",
  "crates/checker/tests/unit/resolve/tests.rs",
  "crates/checker/tests/unit/speculate/tests.rs",
  "crates/checker/tests/unit/statements/tests.rs",
  "crates/checker/tests/unit/structural/tests.rs",
  "crates/checker/tests/unit/unions/tests.rs",
  "crates/checker/tests/unit/variance/tests.rs",
  "crates/compiler/tests/contracts.rs",
  "crates/compiler/tests/integration/es2015_generators_witness_contract.rs",
  "crates/compiler/tests/integration/h2_5h_ca2a_promote_contract.rs",
  "crates/compiler/tests/integration/h2_5h_ca2b_seam_contract.rs",
  "crates/compiler/tests/integration/filesystem_loader_contract.rs",
  "crates/compiler/tests/integration/declaration_resolver_replay_decision_equal.rs",
  "crates/compiler/tests/integration/h2_7a_partition_projection.rs",
  "crates/compiler/tests/unit/lib/tests.rs",
  "crates/emitter/src/comment_cursor.rs",
  "crates/emitter/src/position.rs",
  "crates/emitter/src/source_map.rs",
  "crates/emitter/src/token_cursor.rs",
  "crates/emitter/src/writer.rs",
  "crates/emitter/tests/contracts.rs",
  "crates/emitter/tests/integration/token_cursor_contract.rs",
  "crates/emitter/tests/integration/writer_position_contract.rs",
  "crates/emitter/tests/source_comment_topology_contract.rs",
  "crates/emitter/tests/unit/builtins/tests.rs",
  "crates/emitter/tests/unit/comment_scope_predicate/tests.rs",
  "crates/emitter/tests/unit/lib/tests.rs",
  "crates/emitter/tests/unit/source_map/tests.rs",
  "crates/emitter/tests/unit/token_cursor/tests.rs",
  "crates/harness/src/lib.rs",
  "crates/harness/tests/integration/upstream_execution_plan.rs",
  "crates/harness/tests/unit/lib/tests.rs",
  "crates/program/src/config.rs",
  "crates/program/src/lib.rs",
  "crates/program/src/library.rs",
  "crates/program/src/module_resolution.rs",
  "crates/program/src/option_validation.rs",
  "crates/program/src/resolution.rs",
  "crates/program/tests/integration/config_paths_program_options_contract.rs",
  "crates/program/tests/integration/config_program_loader_contract.rs",
  "crates/program/tests/integration/config_root_plan_contract.rs",
  "crates/program/tests/integration/library_program_loader_contract.rs",
  "crates/program/tests/integration/module_resolution_contract.rs",
  "crates/program/tests/unit/library/tests.rs",
  "crates/syntax/nodes.schema.json",
  "crates/syntax/src/for_each_child.rs",
  "crates/syntax/src/nodes.rs",
  "crates/syntax/src/observable_fields.rs",
  "crates/syntax/src/regex.rs",
  "crates/syntax/src/relocate.rs",
  "crates/syntax/src/scanner.rs",
  "crates/syntax/tests/unit/regex/tests.rs",
  "crates/syntax/tests/unit/scanner/tests.rs",
  "crates/xtask/tests/unit/h2_1a_acceptance/tests.rs",
  "crates/types/src/flags.rs",
  "crates/emitter/tests/integration/tsx_type_argument_transform_contract.rs",
  "crates/emitter/tests/unit/builtins_jsx_tests.rs",
  "crates/emitter/tests/unit/target_bindings_tests.rs",
  "crates/harness/tests/unit/upstream_suites/execution_tests.rs",
  "crates/harness/tests/integration/h2_5h_project_emit.rs",
  "crates/program/tests/unit/option_validation_tests.rs",
  "crates/emitter/tests/integration/comment_scope_witness_contract.rs",
  "crates/emitter/tests/integration/declaration_printer_reprint_contract.rs",
  // h2-7a-m-4: the dormant declaration transformer module tree, its unit
  // tests, and the L1/L2 transformer replay (the m-3 replay precedent).
  "crates/compiler/tests/integration/declaration_transformer_replay_decision_equal.rs",
  "crates/emitter/src/declarations/diagnostics.rs",
  "crates/emitter/src/declarations/ensure.rs",
  "crates/emitter/src/declarations/mod.rs",
  "crates/emitter/src/declarations/orchestration.rs",
  "crates/emitter/src/declarations/root.rs",
  "crates/emitter/src/declarations/selection.rs",
  "crates/emitter/src/declarations/state.rs",
  "crates/emitter/src/declarations/statements.rs",
  "crates/emitter/src/declarations/subtree.rs",
  "crates/emitter/src/declarations/tracker.rs",
  "crates/emitter/tests/unit/declarations/tests.rs",
  "crates/emitter/tests/unit/declarations/tests_p2.rs",
  "crates/emitter/tests/unit/factory_classifier/tests.rs",
  "crates/emitter/tests/unit/helpers/tests.rs",
  "crates/emitter/tests/unit/hook_chaining/tests.rs",
  "crates/emitter/src/builtins/flatten_destructuring.rs",
  "crates/emitter/src/builtins/generators.rs",
  "crates/emitter/src/builtins/tagged_template.rs",
  "crates/emitter/src/builtins/es2015.rs",
  "crates/emitter/tests/unit/flatten_destructuring/tests.rs",
  "crates/emitter/tests/unit/generators/tests.rs",
  "crates/emitter/tests/unit/es2015/tests.rs",
  // h2-7b-m-2: the activation flip's declaration output-path planner
  // (PlanDeclarationPaths, the collision requalification).
  "crates/emitter/src/declarations/paths.rs",
  // Emitter-final integration: preserve all prior surviving identities and
  // bind the complete changed crate closure, including integration controls.
  // A path hash is input identity, not proof that a test or fixture executes.
  "crates/binder/src/assignment.rs",
  "crates/binder/src/containers.rs",
  "crates/binder/src/declare.rs",
  "crates/binder/src/node_util.rs",
  "crates/binder/src/symbols.rs",
  "crates/binder/src/symbols/table.rs",
  "crates/binder/tests/fixtures/utf16-binder-names.rs",
  "crates/binder/tests/owned_symbol_names.rs",
  "crates/binder/tests/unit/bind/tests.rs",
  "crates/binder/tests/unit/declare/tests.rs",
  "crates/binder/tests/unit/symbols/tests.rs",
  "crates/checker/src/conditional.rs",
  "crates/checker/src/declaration_emit/replay_json.rs",
  "crates/checker/src/display_clone.rs",
  "crates/checker/src/display_clone_body.rs",
  "crates/checker/src/display_clone_module.rs",
  "crates/checker/src/flow.rs",
  "crates/checker/src/iterate.rs",
  "crates/checker/src/js_grammar.rs",
  "crates/checker/src/unused.rs",
  "crates/checker/tests/authoritative_external_fact.rs",
  "crates/checker/tests/fixtures/import-publication-resolver.json",
  "crates/checker/tests/fixtures/utf16-jsx-intrinsic-identity.json",
  "crates/checker/tests/fixtures/utf16-literal-type-display.json",
  "crates/checker/tests/fixtures/utf16-specifier-regex-values.json",
  "crates/checker/tests/unit/annotate/accessor_ladder_tests.rs",
  "crates/checker/tests/unit/annotate/alias_instantiation_tests.rs",
  "crates/checker/tests/unit/annotate/c0_annotation_recovery_tests.rs",
  "crates/checker/tests/unit/annotate/enum_tests.rs",
  "crates/checker/tests/unit/annotate/generic_declared_type_tests.rs",
  "crates/checker/tests/unit/annotate/generic_reference_tests.rs",
  "crates/checker/tests/unit/class/tests.rs",
  "crates/checker/tests/unit/contextual/tests.rs",
  "crates/checker/tests/unit/engine/relation_error_state_tests.rs",
  "crates/checker/tests/unit/evaluate/tests.rs",
  "crates/checker/tests/unit/flow/tests.rs",
  "crates/checker/tests/unit/globals/tests.rs",
  "crates/checker/tests/unit/instantiate/tests.rs",
  "crates/checker/tests/unit/modules/c0_module_recovery_tests.rs",
  "crates/checker/tests/unit/state/tests.rs",
  "crates/checker/tests/unit/unused/c0_unused_owner_recovery_tests.rs",
  "crates/checker/tests/unit/unused/tests.rs",
  "crates/checker/tests/unit/widen/tests.rs",
  "crates/compiler/examples/h2_baseline_qualification.rs",
  "crates/compiler/src/transpile.rs",
  "crates/compiler/tests/bundle_metadata_t1_contract.rs",
  "crates/compiler/tests/decorator_binding_pipeline_contract.rs",
  "crates/compiler/tests/decorator_super_contract.rs",
  "crates/compiler/tests/emitter_final_batch.rs",
  "crates/compiler/tests/emitter_final_rows.rs",
  "crates/compiler/tests/emitter_final_universe.rs",
  "crates/compiler/tests/fixtures/access-token-ranges.json",
  "crates/compiler/tests/fixtures/alias-conflict-display.json",
  "crates/compiler/tests/fixtures/ambient-alias-target-names.json",
  "crates/compiler/tests/fixtures/array-comment-publication.json",
  "crates/compiler/tests/fixtures/async-arrow-body-ranges.json",
  "crates/compiler/tests/fixtures/async-arrow-comment-boundaries.json",
  "crates/compiler/tests/fixtures/async-capture-source-ranges.json",
  "crates/compiler/tests/fixtures/await-flag-commands.json",
  "crates/compiler/tests/fixtures/binding-name-map-ranges.json",
  "crates/compiler/tests/fixtures/bundle-metadata-t1-inputs.json",
  "crates/compiler/tests/fixtures/bundle-metadata-t1-known-native.json",
  "crates/compiler/tests/fixtures/bundle-metadata-t1-known-packet.json",
  "crates/compiler/tests/fixtures/bundle-metadata-t1.json",
  "crates/compiler/tests/fixtures/cjs-default-reexport-names.json",
  "crates/compiler/tests/fixtures/class-declaration-dependency-order.json",
  "crates/compiler/tests/fixtures/class-declaration-member-order.json",
  "crates/compiler/tests/fixtures/class-field-alias-map-positions.json",
  "crates/compiler/tests/fixtures/class-field-initializer-comments.json",
  "crates/compiler/tests/fixtures/class-header-token.json",
  "crates/compiler/tests/fixtures/class-helper-accessor-producers.json",
  "crates/compiler/tests/fixtures/class-optional-name.json",
  "crates/compiler/tests/fixtures/class-statement-layout.json",
  "crates/compiler/tests/fixtures/class-transform-flags.json",
  "crates/compiler/tests/fixtures/commonjs-class-instance.json",
  "crates/compiler/tests/fixtures/commonjs-esmodule-marker.json",
  "crates/compiler/tests/fixtures/const-modifier-erasure.json",
  "crates/compiler/tests/fixtures/declaration-comment-detached-prefixes.json",
  "crates/compiler/tests/fixtures/declaration-comment-parameter-tags.json",
  "crates/compiler/tests/fixtures/declaration-comment-range-traces.json",
  "crates/compiler/tests/fixtures/declaration-comment-ranges.json",
  "crates/compiler/tests/fixtures/declaration-map-bundle-boundary.json",
  "crates/compiler/tests/fixtures/declaration-token-comments.json",
  "crates/compiler/tests/fixtures/decorator-binding-inputs.json",
  "crates/compiler/tests/fixtures/decorator-binding-known-native.json",
  "crates/compiler/tests/fixtures/decorator-binding.json.zst",
  "crates/compiler/tests/fixtures/decorator-lexical-prologue-inputs.json",
  "crates/compiler/tests/fixtures/decorator-lexical-prologue-readers-inputs.json",
  "crates/compiler/tests/fixtures/decorator-lexical-prologue-readers-v2-inputs.json",
  "crates/compiler/tests/fixtures/decorator-lexical-prologue-readers-v2.json",
  "crates/compiler/tests/fixtures/decorator-lexical-prologue-readers.json",
  "crates/compiler/tests/fixtures/decorator-lexical-prologue.json",
  "crates/compiler/tests/fixtures/decorator-literal-key-spelling-inputs.json",
  "crates/compiler/tests/fixtures/decorator-literal-key-spelling.json",
  "crates/compiler/tests/fixtures/decorator-literal-member-kinds-inputs.json",
  "crates/compiler/tests/fixtures/decorator-literal-member-kinds.json",
  "crates/compiler/tests/fixtures/decorator-name-owners-inputs.json",
  "crates/compiler/tests/fixtures/decorator-name-owners.json",
  "crates/compiler/tests/fixtures/decorator-parameter-binding-inputs.json",
  "crates/compiler/tests/fixtures/decorator-parameter-binding.json",
  "crates/compiler/tests/fixtures/decorator-parameter-class-fields-inputs.json",
  "crates/compiler/tests/fixtures/decorator-parameter-class-fields.json",
  "crates/compiler/tests/fixtures/decorator-receiver-context-inputs.json",
  "crates/compiler/tests/fixtures/decorator-receiver-context.json",
  "crates/compiler/tests/fixtures/decorator-source-followup-inputs.json",
  "crates/compiler/tests/fixtures/decorator-source-followup-top-level-inputs.json",
  "crates/compiler/tests/fixtures/decorator-source-followup-top-level.json",
  "crates/compiler/tests/fixtures/decorator-source-followup.json",
  "crates/compiler/tests/fixtures/decorator-super-extra-inputs.json",
  "crates/compiler/tests/fixtures/decorator-super-extra.json.zst",
  "crates/compiler/tests/fixtures/decorator-super-followup-inputs.json",
  "crates/compiler/tests/fixtures/decorator-super-followup.json.zst",
  "crates/compiler/tests/fixtures/decorator-super-followup2-inputs.json",
  "crates/compiler/tests/fixtures/decorator-super-followup2.json.zst",
  "crates/compiler/tests/fixtures/decorator-super-followup3-inputs.json",
  "crates/compiler/tests/fixtures/decorator-super-followup3.json.zst",
  "crates/compiler/tests/fixtures/decorator-super-inputs.json",
  "crates/compiler/tests/fixtures/decorator-super-paths-inputs.json",
  "crates/compiler/tests/fixtures/decorator-super-paths.json",
  "crates/compiler/tests/fixtures/decorator-super.json.zst",
  "crates/compiler/tests/fixtures/decorator-system-helper-prologues-inputs.json",
  "crates/compiler/tests/fixtures/decorator-system-helper-prologues.json",
  "crates/compiler/tests/fixtures/decorator-system-map-followup-inputs.json",
  "crates/compiler/tests/fixtures/decorator-system-map-followup.json",
  "crates/compiler/tests/fixtures/decorator-transform-order-inputs.json",
  "crates/compiler/tests/fixtures/decorator-transform-order.json",
  "crates/compiler/tests/fixtures/defineproperty-readonly-exports.json",
  "crates/compiler/tests/fixtures/defineproperty-readonly.json",
  "crates/compiler/tests/fixtures/defineproperty-setter-annotations.json",
  "crates/compiler/tests/fixtures/defineproperty-setter-names.json",
  "crates/compiler/tests/fixtures/ellipsis-comment-owners.json",
  "crates/compiler/tests/fixtures/emitter-audit-class-regressions.json",
  "crates/compiler/tests/fixtures/emitter-class-helper-gates.json",
  "crates/compiler/tests/fixtures/emitter-cli-options.json",
  "crates/compiler/tests/fixtures/emitter-context-recovery.json",
  "crates/compiler/tests/fixtures/emitter-final-known-native.json",
  "crates/compiler/tests/fixtures/emitter-final-universe-plan-base.json.zst",
  "crates/compiler/tests/fixtures/emitter-final-universe.json",
  "crates/compiler/tests/fixtures/emitter-helper-probes.json",
  "crates/compiler/tests/fixtures/emitter-heritage-boundaries.json",
  "crates/compiler/tests/fixtures/emitter-jsdoc-original-command.json",
  "crates/compiler/tests/fixtures/emitter-missing-await.json",
  "crates/compiler/tests/fixtures/emitter-missing-declaration-binding.json",
  "crates/compiler/tests/fixtures/emitter-missing-declaration-effects.json",
  "crates/compiler/tests/fixtures/emitter-missing-declaration-scripts.json",
  "crates/compiler/tests/fixtures/emitter-missing-declaration.json",
  "crates/compiler/tests/fixtures/emitter-nested-paren-recovery.json",
  "crates/compiler/tests/fixtures/emitter-parameter-gap-module.json",
  "crates/compiler/tests/fixtures/emitter-parameter-gap-recovery.json",
  "crates/compiler/tests/fixtures/emitter-r104-rest-controls.json",
  "crates/compiler/tests/fixtures/emitter-r107-declaration-comments.json",
  "crates/compiler/tests/fixtures/emitter-r109-token-neighbours.json",
  "crates/compiler/tests/fixtures/emitter-r111-do-body-controls.json",
  "crates/compiler/tests/fixtures/emitter-r113-type-comment-controls.json",
  "crates/compiler/tests/fixtures/emitter-r117-type-comment-controls.json",
  "crates/compiler/tests/fixtures/emitter-r119-type-comment-controls.json",
  "crates/compiler/tests/fixtures/emitter-r129-variable-type-controls.json",
  "crates/compiler/tests/fixtures/emitter-r145-variable-comma-controls.json",
  "crates/compiler/tests/fixtures/emitter-r167-corpus-controls.json",
  "crates/compiler/tests/fixtures/emitter-r168-corpus-controls.json",
  "crates/compiler/tests/fixtures/emitter-r171-corpus-controls.json",
  "crates/compiler/tests/fixtures/emitter-r77-regressions.json",
  "crates/compiler/tests/fixtures/emitter-r95-call-boundaries.json",
  "crates/compiler/tests/fixtures/emitter-r95-neighbours.json",
  "crates/compiler/tests/fixtures/emitter-r95-wrapper-rest.json",
  "crates/compiler/tests/fixtures/emitter-recovery-boundaries.json",
  "crates/compiler/tests/fixtures/emitter-session-retirements.json",
  "crates/compiler/tests/fixtures/emitter-statement-gap-recovery.json",
  "crates/compiler/tests/fixtures/empty-block-comments.json",
  "crates/compiler/tests/fixtures/export-destructuring-boundaries.json",
  "crates/compiler/tests/fixtures/export-destructuring-comments.json",
  "crates/compiler/tests/fixtures/export-destructuring-trailing.json",
  "crates/compiler/tests/fixtures/export-name-syntax-maps.json",
  "crates/compiler/tests/fixtures/export-name-syntax.json",
  "crates/compiler/tests/fixtures/export-specifier-names.json",
  "crates/compiler/tests/fixtures/export-star-declaration-producer.json",
  "crates/compiler/tests/fixtures/h2-5h-parameter-temporaries.json",
  "crates/compiler/tests/fixtures/h2-6a-map-option-projection.json",
  "crates/compiler/tests/fixtures/h2-8a-declaration-specifiers-composition-inputs.json",
  "crates/compiler/tests/fixtures/h2-8a-declaration-specifiers-composition.json",
  "crates/compiler/tests/fixtures/h2-8a-declaration-specifiers-inputs.json",
  "crates/compiler/tests/fixtures/h2-8a-declaration-specifiers.json",
  "crates/compiler/tests/fixtures/h2-8a-jsdoc-return.json",
  "crates/compiler/tests/fixtures/h2-8a-require-rewrite-composition-inputs.json",
  "crates/compiler/tests/fixtures/h2-8a-require-rewrite-composition.json",
  "crates/compiler/tests/fixtures/h2-8a-require-rewrite-dynamic-inputs.json",
  "crates/compiler/tests/fixtures/h2-8a-require-rewrite-dynamic.json",
  "crates/compiler/tests/fixtures/h2-8a-require-rewrite-inputs.json",
  "crates/compiler/tests/fixtures/h2-8a-require-rewrite-substitution-inputs.json",
  "crates/compiler/tests/fixtures/h2-8a-require-rewrite-substitution.json",
  "crates/compiler/tests/fixtures/h2-8a-require-rewrite.json",
  "crates/compiler/tests/fixtures/h2-8b-config-commands-inputs.json",
  "crates/compiler/tests/fixtures/h2-8b-config-commands.json",
  "crates/compiler/tests/fixtures/h2-8b-config-conversion-commands-inputs.json",
  "crates/compiler/tests/fixtures/h2-8b-config-conversion-commands.json",
  "crates/compiler/tests/fixtures/h2-8b-config-diagnostic-commands-inputs.json",
  "crates/compiler/tests/fixtures/h2-8b-config-diagnostic-commands.json",
  "crates/compiler/tests/fixtures/h2-8b-config-diagnostic-routing-inputs.json",
  "crates/compiler/tests/fixtures/h2-8b-config-diagnostic-routing.json",
  "crates/compiler/tests/fixtures/h2-8b-config-discovery-commands-inputs.json",
  "crates/compiler/tests/fixtures/h2-8b-config-discovery-commands.json",
  "crates/compiler/tests/fixtures/h2-8b-config-entity-commands-inputs.json",
  "crates/compiler/tests/fixtures/h2-8b-config-entity-commands.json",
  "crates/compiler/tests/fixtures/h2-8b-config-extension-commands-inputs.json",
  "crates/compiler/tests/fixtures/h2-8b-config-extension-commands.json",
  "crates/compiler/tests/fixtures/h2-8b-config-root-commands-inputs.json",
  "crates/compiler/tests/fixtures/h2-8b-config-root-commands.json",
  "crates/compiler/tests/fixtures/h2-8b-config-source-commands-inputs.json",
  "crates/compiler/tests/fixtures/h2-8b-config-source-commands.json",
  "crates/compiler/tests/fixtures/h2-8b-config-source-span-commands-inputs.json",
  "crates/compiler/tests/fixtures/h2-8b-config-source-span-commands.json",
  "crates/compiler/tests/fixtures/h2-8b-library-order-inputs.json",
  "crates/compiler/tests/fixtures/h2-8b-library-order.json",
  "crates/compiler/tests/fixtures/h2-8b-library-replacement-inputs.json",
  "crates/compiler/tests/fixtures/h2-8b-library-replacement.json",
  "crates/compiler/tests/fixtures/h2_8c_transpile/expected.v1.json",
  "crates/compiler/tests/fixtures/h2_8c_transpile/inputs.v1.json",
  "crates/compiler/tests/fixtures/h2_8c_transpile/known-native.v1.json",
  "crates/compiler/tests/fixtures/h2_8c_transpile/known-open.v1.json",
  "crates/compiler/tests/fixtures/h2_8c_transpile/review-expected.v1.json",
  "crates/compiler/tests/fixtures/h2_8c_transpile/review-inputs.v1.json",
  "crates/compiler/tests/fixtures/hoisted-declaration-export-ranges.json",
  "crates/compiler/tests/fixtures/import-helpers.json",
  "crates/compiler/tests/fixtures/import-publication-reference.json",
  "crates/compiler/tests/fixtures/javascript-import-retention.json",
  "crates/compiler/tests/fixtures/jsdoc-block-scope-container.json",
  "crates/compiler/tests/fixtures/jsdoc-implements-serialization.json",
  "crates/compiler/tests/fixtures/jsdoc-parentheses-guard.json",
  "crates/compiler/tests/fixtures/literal-update-pipeline.json",
  "crates/compiler/tests/fixtures/local-alias-declarations.json",
  "crates/compiler/tests/fixtures/meta-property-token-maps.json",
  "crates/compiler/tests/fixtures/module-export-identifiers.json",
  "crates/compiler/tests/fixtures/module-transformer-selection.json",
  "crates/compiler/tests/fixtures/object-property-owners.json",
  "crates/compiler/tests/fixtures/one-sided-class-comments.json",
  "crates/compiler/tests/fixtures/output-directories.json",
  "crates/compiler/tests/fixtures/output-filesystem.json",
  "crates/compiler/tests/fixtures/output-matrix-filesystem.json",
  "crates/compiler/tests/fixtures/output-matrix.json",
  "crates/compiler/tests/fixtures/output-root-format.json",
  "crates/compiler/tests/fixtures/output-roots.json",
  "crates/compiler/tests/fixtures/package-output-inputs.json",
  "crates/compiler/tests/fixtures/post-t1-residuals-inputs.json",
  "crates/compiler/tests/fixtures/post-t1-residuals-known-native.json",
  "crates/compiler/tests/fixtures/post-t1-residuals-known-packet.json",
  "crates/compiler/tests/fixtures/post-t1-residuals.json",
  "crates/compiler/tests/fixtures/prologue-only-detached-comments.json",
  "crates/compiler/tests/fixtures/promoted-class-export-maps.json",
  "crates/compiler/tests/fixtures/property-initializer-comment-ownership.json",
  "crates/compiler/tests/fixtures/property-literal-key-ranges.json",
  "crates/compiler/tests/fixtures/repeated-target-declaration-aliases.json",
  "crates/compiler/tests/fixtures/retained-accessor-inputs.json",
  "crates/compiler/tests/fixtures/retained-accessor-owners.json",
  "crates/compiler/tests/fixtures/retained-comma-factory-inputs.json",
  "crates/compiler/tests/fixtures/retained-comma-factory.json",
  "crates/compiler/tests/fixtures/retained-constructor-reference-inputs.json",
  "crates/compiler/tests/fixtures/retained-constructor-references.json",
  "crates/compiler/tests/fixtures/retained-lexical-edge-inputs.json",
  "crates/compiler/tests/fixtures/retained-lexical-edges.json",
  "crates/compiler/tests/fixtures/retained-lexical-environments.json",
  "crates/compiler/tests/fixtures/retained-lexical-inputs.json",
  "crates/compiler/tests/fixtures/setter-symbol-invariant.json",
  "crates/compiler/tests/fixtures/static-initializer-map-ranges.json",
  "crates/compiler/tests/fixtures/static-this-super-auto-accessor-storage-names-inputs.json",
  "crates/compiler/tests/fixtures/static-this-super-auto-accessor-storage-names.json",
  "crates/compiler/tests/fixtures/static-this-super-conditional-super-calls-inputs.json",
  "crates/compiler/tests/fixtures/static-this-super-conditional-super-calls.json",
  "crates/compiler/tests/fixtures/static-this-super-legacy-decorated-static-super-inputs.json",
  "crates/compiler/tests/fixtures/static-this-super-legacy-decorated-static-super.json",
  "crates/compiler/tests/fixtures/static-this-super-static-super-calls-inputs.json",
  "crates/compiler/tests/fixtures/static-this-super-static-super-calls.json",
  "crates/compiler/tests/fixtures/synthetic-default-alias.json",
  "crates/compiler/tests/fixtures/synthetic-namespace-export-modifiers.json",
  "crates/compiler/tests/fixtures/system-binding-boundaries.json",
  "crates/compiler/tests/fixtures/system-binding-publication.json",
  "crates/compiler/tests/fixtures/system-destructuring-order.json",
  "crates/compiler/tests/fixtures/system-dynamic-imports.json",
  "crates/compiler/tests/fixtures/system-using-publication.json",
  "crates/compiler/tests/fixtures/system-variable-publication.json",
  "crates/compiler/tests/fixtures/token-comment-phases.json",
  "crates/compiler/tests/fixtures/transformed-class-assigned-names.json",
  "crates/compiler/tests/fixtures/utf16-identity-recovery-controls.json",
  "crates/compiler/tests/fixtures/utf16-literal-recovery-census.json",
  "crates/compiler/tests/fixtures/utf16-literal-recovery-corpus.json",
  "crates/compiler/tests/fixtures/utf16-literals-adjacent-probes-inputs.json",
  "crates/compiler/tests/fixtures/utf16-literals-adjacent-probes.json",
  "crates/compiler/tests/fixtures/utf16-literals-bundle-prologues-inputs.json",
  "crates/compiler/tests/fixtures/utf16-literals-bundle-prologues.json",
  "crates/compiler/tests/fixtures/utf16-literals-string-literals-inputs.json",
  "crates/compiler/tests/fixtures/utf16-literals-string-literals.json",
  "crates/compiler/tests/fixtures/utf16-literals-template-literals-inputs.json",
  "crates/compiler/tests/fixtures/utf16-literals-template-literals.json",
  "crates/compiler/tests/fixtures/utf16-noemit-command-controls.json",
  "crates/compiler/tests/fixtures/utf16-original-rows-complete.json",
  "crates/compiler/tests/fixtures/utf16-review-fix-controls.json",
  "crates/compiler/tests/fixtures/utf16-tagged-template-controls.json",
  "crates/compiler/tests/fixtures/utf16-tagged-template-review-v1.json",
  "crates/compiler/tests/fixtures/utf16-tagged-template-review-v2.json",
  "crates/compiler/tests/h2_5h_parameter_temporaries.rs",
  "crates/compiler/tests/h2_5h_utf16_literal_rows.rs",
  "crates/compiler/tests/h2_5h_utf16_literal_witnesses.rs",
  "crates/compiler/tests/h2_5h_utf16_original_rows_complete.rs",
  "crates/compiler/tests/h2_6a_map_option_projection.rs",
  "crates/compiler/tests/h2_8a_declaration_comment_ranges.rs",
  "crates/compiler/tests/h2_8a_declaration_specifiers.rs",
  "crates/compiler/tests/h2_8a_jsdoc_return.rs",
  "crates/compiler/tests/h2_8a_original_corpus.rs",
  "crates/compiler/tests/h2_8a_prologue_only_detached_comments.rs",
  "crates/compiler/tests/h2_8a_require_rewrite.rs",
  "crates/compiler/tests/h2_8a_utf16_identity_recovery_controls.rs",
  "crates/compiler/tests/h2_8a_utf16_literal_recovery_corpus.rs",
  "crates/compiler/tests/h2_8a_utf16_review_fix_controls.rs",
  "crates/compiler/tests/h2_8a_utf16_tagged_template_controls.rs",
  "crates/compiler/tests/integration/async_arrow_body_ranges.rs",
  "crates/compiler/tests/integration/automatic_type_directive_session_contract.rs",
  "crates/compiler/tests/integration/emitter_residual_audit.rs",
  "crates/compiler/tests/integration/h0_qualification_contract.rs",
  "crates/compiler/tests/integration/h1_memory_emit_oracle_contract.rs",
  "crates/compiler/tests/integration/h2_5h_static_this_super_rows.rs",
  "crates/compiler/tests/integration/h2_5h_static_this_super_witnesses.rs",
  "crates/compiler/tests/integration/h2_8a_alias_conflict_display.rs",
  "crates/compiler/tests/integration/h2_8a_array_comment_publication.rs",
  "crates/compiler/tests/integration/h2_8a_binding_name_map_ranges.rs",
  "crates/compiler/tests/integration/h2_8a_cjs_default_reexport_names.rs",
  "crates/compiler/tests/integration/h2_8a_class_dependency_order.rs",
  "crates/compiler/tests/integration/h2_8a_class_field_alias_map_positions.rs",
  "crates/compiler/tests/integration/h2_8a_class_field_initializer_comments.rs",
  "crates/compiler/tests/integration/h2_8a_class_header_token.rs",
  "crates/compiler/tests/integration/h2_8a_class_helper_accessor_producers.rs",
  "crates/compiler/tests/integration/h2_8a_class_optional_name.rs",
  "crates/compiler/tests/integration/h2_8a_class_statement_layout.rs",
  "crates/compiler/tests/integration/h2_8a_class_transform_flags.rs",
  "crates/compiler/tests/integration/h2_8a_commonjs_class_instance.rs",
  "crates/compiler/tests/integration/h2_8a_commonjs_esmodule_marker.rs",
  "crates/compiler/tests/integration/h2_8a_const_modifier_erasure.rs",
  "crates/compiler/tests/integration/h2_8a_declaration_token_comments.rs",
  "crates/compiler/tests/integration/h2_8a_decorator_next_witnesses.rs",
  "crates/compiler/tests/integration/h2_8a_decorator_super.rs",
  "crates/compiler/tests/integration/h2_8a_defineproperty_readonly.rs",
  "crates/compiler/tests/integration/h2_8a_defineproperty_readonly_exports.rs",
  "crates/compiler/tests/integration/h2_8a_defineproperty_setter_annotations.rs",
  "crates/compiler/tests/integration/h2_8a_defineproperty_setter_names.rs",
  "crates/compiler/tests/integration/h2_8a_ellipsis_comment_owners.rs",
  "crates/compiler/tests/integration/h2_8a_export_name_syntax.rs",
  "crates/compiler/tests/integration/h2_8a_export_name_syntax_maps.rs",
  "crates/compiler/tests/integration/h2_8a_export_specifier_names.rs",
  "crates/compiler/tests/integration/h2_8a_export_star_declaration_producer.rs",
  "crates/compiler/tests/integration/h2_8a_hoisted_declaration_export_ranges.rs",
  "crates/compiler/tests/integration/h2_8a_import_helpers.rs",
  "crates/compiler/tests/integration/h2_8a_import_publication_reference.rs",
  "crates/compiler/tests/integration/h2_8a_javascript_imports.rs",
  "crates/compiler/tests/integration/h2_8a_jsdoc_block_scope_container.rs",
  "crates/compiler/tests/integration/h2_8a_jsdoc_implements_serialization.rs",
  "crates/compiler/tests/integration/h2_8a_jsdoc_parentheses_guard.rs",
  "crates/compiler/tests/integration/h2_8a_local_aliases.rs",
  "crates/compiler/tests/integration/h2_8a_meta_property_token_maps.rs",
  "crates/compiler/tests/integration/h2_8a_module_transformer_selection.rs",
  "crates/compiler/tests/integration/h2_8a_object_property_owners.rs",
  "crates/compiler/tests/integration/h2_8a_one_sided_class_comments.rs",
  "crates/compiler/tests/integration/h2_8a_output_directories.rs",
  "crates/compiler/tests/integration/h2_8a_output_filesystem.rs",
  "crates/compiler/tests/integration/h2_8a_output_matrix.rs",
  "crates/compiler/tests/integration/h2_8a_output_roots.rs",
  "crates/compiler/tests/integration/h2_8a_package_output_inputs.rs",
  "crates/compiler/tests/integration/h2_8a_promoted_class_export_maps.rs",
  "crates/compiler/tests/integration/h2_8a_repeated_target_aliases.rs",
  "crates/compiler/tests/integration/h2_8a_retained_accessor_owners.rs",
  "crates/compiler/tests/integration/h2_8a_static_initializer_map_ranges.rs",
  "crates/compiler/tests/integration/h2_8a_synthetic_default_alias.rs",
  "crates/compiler/tests/integration/h2_8a_synthetic_namespace_export_modifiers.rs",
  "crates/compiler/tests/integration/h2_8a_system_variable_publication.rs",
  "crates/compiler/tests/integration/h2_8a_token_comment_phases.rs",
  "crates/compiler/tests/integration/h2_8a_transformed_class_assigned_names.rs",
  "crates/compiler/tests/integration/h2_8b_config_commands.rs",
  "crates/compiler/tests/integration/h2_8b_config_conversion_commands.rs",
  "crates/compiler/tests/integration/h2_8b_config_diagnostic_commands.rs",
  "crates/compiler/tests/integration/h2_8b_config_diagnostic_routing.rs",
  "crates/compiler/tests/integration/h2_8b_config_discovery_commands.rs",
  "crates/compiler/tests/integration/h2_8b_config_entity_commands.rs",
  "crates/compiler/tests/integration/h2_8b_config_extension_commands.rs",
  "crates/compiler/tests/integration/h2_8b_config_root_commands.rs",
  "crates/compiler/tests/integration/h2_8b_config_source_commands.rs",
  "crates/compiler/tests/integration/h2_8b_config_source_span_commands.rs",
  "crates/compiler/tests/integration/h2_8b_library_replacement.rs",
  "crates/compiler/tests/integration/library_loader_session_contract.rs",
  "crates/compiler/tests/integration/original_path_session_contract.rs",
  "crates/compiler/tests/integration/preserve_symlinks_session_contract.rs",
  "crates/compiler/tests/literal_update_pipeline_contract.rs",
  "crates/compiler/tests/post_t1_residuals_contract.rs",
  "crates/compiler/tests/support/complete_command_corpus.rs",
  "crates/compiler/tests/support/witness_libraries.rs",
  "crates/compiler/tests/transpile_routes_contract.rs",
  "crates/conformance/src/rendered.rs",
  "crates/conformance/tests/unit/h0_memory/tests.rs",
  "crates/conformance/tests/unit/rendered/tests.rs",
  "crates/diagnostics/src/js_string.rs",
  "crates/diagnostics/src/lib.rs",
  "crates/diagnostics/src/render.rs",
  "crates/diagnostics/tests/unit/lib/tests.rs",
  "crates/diagnostics/tests/unit/render/tests.rs",
  "crates/emitter/src/artifact.rs",
  "crates/emitter/src/builtins/relative_imports.rs",
  "crates/emitter/src/route.rs",
  "crates/emitter/src/sink.rs",
  "crates/emitter/tests/class_header_token_metadata_contract.rs",
  "crates/emitter/tests/comma_argument_factory_contract.rs",
  "crates/emitter/tests/comma_list_printer_contract.rs",
  "crates/emitter/tests/compact_body_comments_contract.rs",
  "crates/emitter/tests/decorator_binding_contract.rs",
  "crates/emitter/tests/decorator_super_direct_contract.rs",
  "crates/emitter/tests/ellipsis_comment_metadata_contract.rs",
  "crates/emitter/tests/emit_pipeline_phases_contract.rs",
  "crates/emitter/tests/fixtures/class-expression-updates.json",
  "crates/emitter/tests/fixtures/class-header-token-printer-metadata.json",
  "crates/emitter/tests/fixtures/comma-argument-factory.json",
  "crates/emitter/tests/fixtures/comma-list-printer.json",
  "crates/emitter/tests/fixtures/compact-body-comments.json",
  "crates/emitter/tests/fixtures/decorator-binding-carry-edge.json",
  "crates/emitter/tests/fixtures/decorator-binding-direct.json",
  "crates/emitter/tests/fixtures/decorator-static-accessor-routing.json",
  "crates/emitter/tests/fixtures/decorator-super-direct.json",
  "crates/emitter/tests/fixtures/ellipsis-comment-printer-metadata.json",
  "crates/emitter/tests/fixtures/emit-pipeline-bundle.json",
  "crates/emitter/tests/fixtures/emit-pipeline-phases.json",
  "crates/emitter/tests/fixtures/emitter-r112-post-child-metadata.json",
  "crates/emitter/tests/fixtures/import-type-attributes.json",
  "crates/emitter/tests/fixtures/list-boundary-lines.json",
  "crates/emitter/tests/fixtures/list-comment-flags.json",
  "crates/emitter/tests/fixtures/list-format-flags.json",
  "crates/emitter/tests/fixtures/list-intervening-owners.json",
  "crates/emitter/tests/fixtures/list-trailing-token-owners.json",
  "crates/emitter/tests/fixtures/literal-parent-provenance-utf16.json",
  "crates/emitter/tests/fixtures/literal-parent-provenance.json",
  "crates/emitter/tests/fixtures/literal-update-factory.json",
  "crates/emitter/tests/fixtures/literal-update-lifetime.json",
  "crates/emitter/tests/fixtures/literal-update-transform.json",
  "crates/emitter/tests/fixtures/mapped-type-members.json",
  "crates/emitter/tests/fixtures/meta-property-token-map-invariants.json",
  "crates/emitter/tests/fixtures/printer-comment-carry.json",
  "crates/emitter/tests/fixtures/printer-failure-hooks.json",
  "crates/emitter/tests/fixtures/printer-failure-known-native.json",
  "crates/emitter/tests/fixtures/printer-failure-probes.json",
  "crates/emitter/tests/fixtures/printer-failure-review.json",
  "crates/emitter/tests/fixtures/printer-hook-hints.json",
  "crates/emitter/tests/fixtures/string-literal-identifier-source.json",
  "crates/emitter/tests/fixtures/string-property-provenance.json",
  "crates/emitter/tests/fixtures/template-fragment-provenance.json",
  "crates/emitter/tests/fixtures/template-raw-provenance.json",
  "crates/emitter/tests/fixtures/token-comment-phase-printer-metadata.json",
  "crates/emitter/tests/fixtures/utf16-accessor-pair-names.json",
  "crates/emitter/tests/fixtures/utf16-declaration-literal-printer.json",
  "crates/emitter/tests/fixtures/utf16-decorator-name-values.json",
  "crates/emitter/tests/fixtures/utf16-generated-declaration-order.json",
  "crates/emitter/tests/fixtures/utf16-jsx-name-values.json",
  "crates/emitter/tests/fixtures/utf16-literal-escaping.json",
  "crates/emitter/tests/fixtures/utf16-module-name-values.json",
  "crates/emitter/tests/fixtures/utf16-source-map-values.json",
  "crates/emitter/tests/fixtures/utf16-synthetic-export-names.json",
  "crates/emitter/tests/fixtures/utf16-writer.json",
  "crates/emitter/tests/import_type_attributes_contract.rs",
  "crates/emitter/tests/integration/artifact_sink_contract.rs",
  "crates/emitter/tests/list_comment_flags_contract.rs",
  "crates/emitter/tests/list_format_flags_contract.rs",
  "crates/emitter/tests/literal_parent_provenance_contract.rs",
  "crates/emitter/tests/literal_update_contract.rs",
  "crates/emitter/tests/literal_value_provenance_contract.rs",
  "crates/emitter/tests/mapped_type_members_contract.rs",
  "crates/emitter/tests/printer_failure_contract.rs",
  "crates/emitter/tests/string_literal_identifier_source_contract.rs",
  "crates/emitter/tests/token_comment_phase_metadata_contract.rs",
  "crates/emitter/tests/unit/builtins/template_flags.rs",
  "crates/emitter/tests/utf16_literal_escaping_contract.rs",
  "crates/emitter/tests/utf16_writer_contract.rs",
  "crates/fuzz/src/adapters/tsrs.rs",
  "crates/fuzz/tests/unit/adapters/tsrs/tests.rs",
  "crates/harness/Cargo.toml",
  "crates/harness/src/upstream_suites/execution/js_paths.rs",
  "crates/harness/src/upstream_suites/execution/observable_input.rs",
  "crates/harness/tests/integration/module_suffixes_oracle_contract.rs",
  "crates/host/Cargo.toml",
  "crates/host/src/error.rs",
  "crates/host/src/filesystem.rs",
  "crates/host/src/js_path.rs",
  "crates/host/src/lib.rs",
  "crates/host/src/memory.rs",
  "crates/host/tests/compiler_host_contract.rs",
  "crates/host/tests/filesystem_host_contract.rs",
  "crates/host/tests/support/scalar_path.rs",
  "crates/host/tests/support/scalar_query_bridge.rs",
  "crates/host/tests/unit/filesystem/tests.rs",
  "crates/program/src/config_host.rs",
  "crates/program/src/config_matcher.rs",
  "crates/program/src/config_options.rs",
  "crates/program/src/error.rs",
  "crates/program/src/js_path.rs",
  "crates/program/src/js_string_ops.rs",
  "crates/program/src/json.rs",
  "crates/program/src/json_value.rs",
  "crates/program/src/output_directories.rs",
  "crates/program/src/path.rs",
  "crates/program/src/resolution_cache.rs",
  "crates/program/src/resolution_error.rs",
  "crates/program/tests/contracts.rs",
  "crates/program/tests/fixtures/h2-8b-config-catalogue.json",
  "crates/program/tests/fixtures/h2-8b-config-diagnostics-inputs.json",
  "crates/program/tests/fixtures/h2-8b-config-diagnostics.json",
  "crates/program/tests/fixtures/h2-8b-config-discovery-inputs.json",
  "crates/program/tests/fixtures/h2-8b-config-discovery-paths-inputs.json",
  "crates/program/tests/fixtures/h2-8b-config-discovery-paths.json",
  "crates/program/tests/fixtures/h2-8b-config-discovery-spelling-inputs.json",
  "crates/program/tests/fixtures/h2-8b-config-discovery-spelling.json",
  "crates/program/tests/fixtures/h2-8b-config-discovery.json",
  "crates/program/tests/fixtures/h2-8b-config-entity-names-inputs.json",
  "crates/program/tests/fixtures/h2-8b-config-entity-names.json",
  "crates/program/tests/fixtures/h2-8b-config-extends-inputs.json",
  "crates/program/tests/fixtures/h2-8b-config-extends.json",
  "crates/program/tests/fixtures/h2-8b-config-reuse-inputs.json",
  "crates/program/tests/fixtures/h2-8b-config-reuse.json",
  "crates/program/tests/fixtures/h2-8b-config-root-boundaries-inputs.json",
  "crates/program/tests/fixtures/h2-8b-config-root-boundaries.json",
  "crates/program/tests/fixtures/h2-8b-config-root-options-inputs.json",
  "crates/program/tests/fixtures/h2-8b-config-root-options.json",
  "crates/program/tests/fixtures/h2-8b-library-loader-order.json",
  "crates/program/tests/fixtures/h2-8b-library-priority-inputs.json",
  "crates/program/tests/fixtures/h2-8b-library-priority.json",
  "crates/program/tests/fixtures/resolution_cache/expected.v1.json",
  "crates/program/tests/fixtures/resolution_cache/manifest.v1.json",
  "crates/program/tests/fixtures/utf16-config-matching.json",
  "crates/program/tests/fixtures/utf16-config-names.json",
  "crates/program/tests/fixtures/utf16-config-path-values.json",
  "crates/program/tests/fixtures/utf16-generated-module-names.json",
  "crates/program/tests/fixtures/utf16-json-values.json",
  "crates/program/tests/fixtures/utf16-lexical-paths.json",
  "crates/program/tests/fixtures/utf16-package-accessors.json",
  "crates/program/tests/fixtures/utf16-package-names.json",
  "crates/program/tests/fixtures/utf16-raw-source-boundary.json",
  "crates/program/tests/fixtures/utf16-string-replacement.json",
  "crates/program/tests/host_platform_smoke_contract.rs",
  "crates/program/tests/integration/automatic_type_directive_loader_contract.rs",
  "crates/program/tests/integration/config_diagnostics_oracle_contract.rs",
  "crates/program/tests/integration/config_keyword_recovery_contract.rs",
  "crates/program/tests/integration/config_option_bag_scaling_contract.rs",
  "crates/program/tests/integration/h2_8b_config_diagnostics.rs",
  "crates/program/tests/integration/h2_8b_config_discovery.rs",
  "crates/program/tests/integration/h2_8b_config_entity_names.rs",
  "crates/program/tests/integration/h2_8b_config_extends.rs",
  "crates/program/tests/integration/h2_8b_config_reuse.rs",
  "crates/program/tests/integration/h2_8b_config_root_boundaries.rs",
  "crates/program/tests/integration/h2_8b_config_root_options.rs",
  "crates/program/tests/integration/host_text_decode_contract.rs",
  "crates/program/tests/integration/module_suffixes_arbitrary_extension_contract.rs",
  "crates/program/tests/integration/module_suffixes_contract.rs",
  "crates/program/tests/integration/path_identity_contract.rs",
  "crates/program/tests/integration/prepared_program_contract.rs",
  "crates/program/tests/resolution_cache_contract.rs",
  "crates/program/tests/support/scalar_json.rs",
  "crates/program/tests/unit/config_matcher/tests.rs",
  "crates/program/tests/unit/json/tests.rs",
  "crates/program/tests/utf16_config_paths.rs",
  "crates/program/tests/utf16_module_paths.rs",
  "crates/program/tests/utf16_raw_source_boundary.rs",
  "crates/syntax/src/arena.rs",
  "crates/syntax/src/parser/jsdoc.rs",
  "crates/syntax/src/recovery.rs",
  "crates/syntax/src/recovery/context.rs",
  "crates/syntax/tests/emitter_recovery.rs",
  "crates/syntax/tests/entity_names.rs",
  "crates/syntax/tests/fixtures/await-flag-boundary.json",
  "crates/syntax/tests/fixtures/emitter-recovery.json",
  "crates/syntax/tests/fixtures/utf16-entity-names.json",
  "crates/syntax/tests/fixtures/utf16-new-meta-property-name.json",
  "crates/syntax/tests/fixtures/utf16-owned-literal-values.json",
  "crates/syntax/tests/fixtures/utf16-recovery-boundary.json",
  "crates/syntax/tests/fixtures/utf16-scanner-escape-diagnostics.json",
  "crates/syntax/tests/fixtures/utf16-template-flags.json",
  "crates/syntax/tests/new_meta_property_name.rs",
  "crates/syntax/tests/owned_literal_values.rs",
  "crates/syntax/tests/recovery_provenance.rs",
  "crates/syntax/tests/scanner_escape_diagnostics.rs",
  "crates/syntax/tests/template_escape_flags.rs",
  "crates/syntax/tests/template_flags.rs",
  "crates/syntax/tests/unit/arena/tests.rs",
  "crates/syntax/tests/unit/lib/tests.rs",
  "crates/syntax/tests/unit/parser/recovery.rs",
  "crates/types/src/escaped_name.rs",
  "crates/types/src/lib.rs",
  "crates/xtask/src/codegen_common.rs",
  "crates/xtask/src/h1_emit_acceptance.rs",
  "crates/xtask/src/h2_6c_output_promotions.rs",
  "crates/xtask/src/node_codegen.rs",
  "crates/xtask/src/recovery_corpus_native.rs",
  "crates/xtask/src/recovery_corpus_native/options.rs",
  "crates/xtask/src/recovery_corpus_native/project.rs",
  "crates/xtask/src/recovery_parse_snapshot.rs",
  "crates/xtask/src/symbol_audit.rs",
  "crates/xtask/src/utf16_literal_recovery_census.rs",
  "crates/xtask/tests/unit/h2_1b_acceptance/tests.rs",
  "crates/xtask/tests/unit/h2_1c_acceptance/tests.rs",
]);

// These files implement the non-authoritative acceptance impact/restart
// shadow. They are not read by the fixed H2.5g acceptance command and must
// not silently become H2 runtime evidence inputs.
const NON_RUNTIME_SHADOW_INPUTS = new Set([
  // Outlined unit-test modules retain their production modules and fixtures.
  "crates/checker/tests/unit/declaration_emit/replay_json/tests.rs",
  "crates/diagnostics/tests/unit/js_string/tests.rs",
  "crates/emitter/tests/unit/factory/parsed_metadata/tests.rs",
  "crates/harness/tests/unit/upstream_suites/execution/js_paths/tests.rs",
  "crates/harness/tests/unit/upstream_suites/execution/project/descriptor_option_tests.rs",
  "crates/program/tests/unit/config_host/tests.rs",
  "crates/program/tests/unit/config_options/schema_oracle.rs",
  "crates/program/tests/unit/js_path/tests.rs",
  "crates/program/tests/unit/js_string_ops/tests.rs",
  "crates/program/tests/unit/loader/node_modules_membership_tests.rs",
  "crates/program/tests/unit/loader/typed_error_source_tests.rs",
  "crates/program/tests/unit/resolution_cache/tests.rs",
  "crates/types/tests/unit/escaped_name/tests.rs",
  "crates/xtask/tests/unit/h2_7de_acceptance/tests.rs",
  "crates/xtask/tests/unit/recovery_corpus_native/tests.rs",
  // h2-7b-w4: the lanes' and the integrator's frozen controls (tests only).
  "crates/compiler/tests/integration/h2_7b_w4a_controls.rs",
  "crates/compiler/tests/integration/h2_7b_w4b_controls.rs",
  "crates/compiler/tests/integration/h2_7b_w4i_controls.rs",
  "crates/program/tests/unit/symlinks/tests.rs",
  // h2-7b-w5: the lanes' frozen controls (tests only).
  "crates/compiler/tests/integration/h2_7b_w5a_controls.rs",
  "crates/compiler/tests/integration/h2_7b_w5b_controls.rs",
  "crates/xtask/src/acceptance_plan.rs",
  "crates/xtask/src/acceptance_slices.rs",
  "crates/xtask/tests/unit/acceptance_plan/tests.rs",
  "crates/xtask/tests/unit/acceptance_slices/tests.rs",
  "crates/xtask/src/local_ci_resume.rs",
  "crates/xtask/tests/unit/local_ci_resume/tests.rs",
  // Evidence/gate producers and their tests: B2-B4 artifact production,
  // the performance observation, and the CI lane/worker policy live
  // outside the H2 emit runtime, like the resume journal above.
  "crates/xtask/src/m8_evidence.rs",
  "crates/xtask/tests/unit/m8_evidence/tests.rs",
  "crates/xtask/tests/unit/main/ci_lane_tests.rs",
  // gate-tax 6: per-target workspace-test receipts + the darwin child-scoped
  // RSS observation — gate phases and their dependency seam, not H2 emit
  // runtime (the local_ci_resume/m8_evidence class above).
  "crates/xtask/Cargo.toml",
  "crates/xtask/src/ci_test_receipts.rs",
  "crates/xtask/tests/unit/ci_test_receipts/tests.rs",
  "crates/xtask/src/l1_incremental_stress.rs",
  "crates/xtask/tests/unit/l1_incremental_stress/tests.rs",
  "crates/harness/tests/integration/h1_compiler_profile_classification.rs",
  "crates/harness/tests/integration/h1_conformance_profile_classification.rs",
  "crates/harness/tests/integration/h1_fourslash_whole_program_equivalence.rs",
  "crates/harness/tests/integration/h1_project_profile_classification.rs",
  "crates/harness/tests/contracts.rs",
  "crates/harness/tests/integration/h2_transition.rs",
  "crates/harness/tests/integration/h2_1a_profile.rs",
  "crates/harness/tests/integration/h2_1b_profile.rs",
  "crates/harness/tests/integration/h2_1c_profile.rs",
  "crates/harness/tests/integration/h2_1d_profile.rs",
  "crates/harness/tests/integration/h2_1e_profile.rs",
  "crates/harness/tests/integration/h2_2a_profile.rs",
  "crates/harness/tests/integration/h2_2b_profile.rs",
  "crates/harness/tests/integration/h2_2c_profile.rs",
  "crates/harness/tests/integration/h2_2d_profile.rs",
  "crates/harness/tests/integration/h2_3a_profile.rs",
  "crates/harness/tests/integration/h2_3b_profile.rs",
  "crates/harness/tests/integration/h2_3c_profile.rs",
  // gate-tax 8 S3: shared pin-manifest helper for the converted harness
  // tests above — gate evidence plumbing, not read by the fixed H2.5g
  // acceptance command.
  "crates/harness/tests/integration/support/pins.rs",
  "crates/harness/tests/integration/transpile_suite_inventory.rs",
  // h2-6a-m-2 replay suite: gate evidence over the frozen W-H2.6A
  // witnesses through the harness print bridge; not read by the fixed
  // H2.5g acceptance command (the CA-4 discovery (a) pattern).
  "crates/compiler/tests/integration/source_map_recording_witness_contract.rs",
  // h2-6a m-3/ca-2 gate evidence and burn-down tooling: same pattern.
  "crates/compiler/tests/integration/source_map_emit_witness_contract.rs",
  "crates/compiler/tests/integration/source_map_band_probe.rs",
  "crates/compiler/tests/integration/h2_7a_m35_controls.rs",
  "crates/compiler/tests/integration/h2_7a_m4_controls.rs",
  // h2-7b-m-2: the frozen controls of the flip train and the runner's unit tests.
  "crates/compiler/tests/integration/h2_7b_m2_controls.rs",
  "crates/xtask/tests/unit/h2_7b_acceptance/tests.rs",
  // h2-7b-w1: the closure wave's frozen controls and the evidence lane's
  // ignored diff-census instrument (test-only; no runtime input).
  "crates/compiler/tests/integration/h2_7b_w1_controls.rs",
  "crates/compiler/tests/integration/h2_7b_w1_diff_census.rs",
  // h2-7b-w2: the two implementer lanes' frozen controls and the evidence
  // lane's ignored write-census instrument (test-only; no runtime input).
  "crates/compiler/tests/integration/h2_7b_w2a_controls.rs",
  "crates/compiler/tests/integration/h2_7b_w2b_controls.rs",
  "crates/compiler/tests/integration/h2_7b_w2_write_census.rs",
  // h2-7b-w3: the two implementer lanes' frozen controls (test-only; no runtime input).
  "crates/compiler/tests/integration/h2_7b_w3a_controls.rs",
  "crates/compiler/tests/integration/h2_7b_w3b_controls.rs",
  "crates/compiler/tests/integration/h2_7a_ca_controls.rs",
  // Diagnostic conformance-runner orchestration: drives the T0 harness
  // over ProgramSession's no-emit surface and is outside the H2 emit
  // runtime (emit acceptance routes through the harness emit drivers,
  // not this runner).
  "crates/conformance/src/h0_memory.rs",
  "crates/conformance/src/bounded_pipeline.rs",
  "crates/conformance/src/families.rs",
  "crates/conformance/src/lib.rs",
  "crates/conformance/src/ratchet.rs",
  "crates/conformance/tests/unit/bounded_pipeline/tests.rs",
  "crates/conformance/tests/unit/lib/tests.rs",
  // H0 registry validation belongs to the semantic CI phase; the fixed H2.5g
  // acceptance command does not call it or compile its outlined unit tests.
  "crates/conformance/src/host_resolution.rs",
  "crates/conformance/tests/unit/host_resolution/tests.rs",
  // Recovery-census gate infrastructure over the diagnostic corpus.
  "crates/xtask/src/recovery_census.rs",
  // Workspace-audit maintenance rules (the CS-6 permanent
  // zero-contextless audit): gate infrastructure, not read by the fixed
  // H2.5g acceptance command.
  "crates/xtask/src/workspace_maintenance.rs",
  "crates/xtask/tests/unit/workspace_maintenance/tests.rs",
]);

function fail(message) {
  throw new Error(message);
}

function requireCondition(condition, message) {
  if (!condition) fail(message);
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value !== null && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function readBytes(relativePath) {
  return fs.readFileSync(path.join(WORKSPACE, relativePath));
}

function readJson(relativePath) {
  return JSON.parse(readBytes(relativePath).toString("utf8"));
}

function pathHash(relativePath) {
  return { path: relativePath, sha256: sha256(readBytes(relativePath)) };
}

function changedRuntimeInputPaths() {
  const runGit = (argv) =>
    execFileSync("git", argv, {
      cwd: WORKSPACE,
      maxBuffer: 16 * 1024 * 1024,
    })
      .toString("utf8")
      .split("\0")
      .filter(Boolean);
  return [
    ...runGit([
      "diff",
      "--name-only",
      "--diff-filter=ACMRTUXB",
      "-z",
      TRUSTED_BASE,
      "--",
      "crates",
    ]),
    ...runGit([
      "ls-files",
      "--others",
      "--exclude-standard",
      "-z",
      "--",
      "crates",
    ]),
  ]
    .filter((relativePath) => !relativePath.startsWith("crates/oracle/"))
    .filter((relativePath) => !NON_RUNTIME_SHADOW_INPUTS.has(relativePath))
    .filter((relativePath, index, paths) => paths.indexOf(relativePath) === index)
    .sort();
}

function withFingerprint(value, field) {
  return { ...value, [field]: sha256(Buffer.from(canonical(value), "utf8")) };
}

function declarationOutputQualificationEvidence() {
  const artifactPath = "ratchets/h2-7de-qualification.v1.json";
  const generatorPath = "crates/oracle/h2-7de-qualification.mjs";
  // Reuse the frozen join's exact source/input/observation/disposition checks.
  // --check executes no TypeScript compiler and reads no Rust/hosted results.
  execFileSync(process.execPath, [path.join(WORKSPACE, generatorPath), "--check"], {
    cwd: WORKSPACE,
    stdio: "pipe",
    maxBuffer: 1024 * 1024,
  });
  const band = readJson(artifactPath);
  const eligible = band.cases.filter((row) => row.disposition === "eligible-for-rust-comparison");
  const deferred = band.cases.filter((row) => row.disposition === "deferred");
  const inBand = (rows, owner) => rows.filter((row) => row.bands.includes(owner)).length;
  requireCondition(
    band.kind === "h2-7de-qualification" &&
      band.status === "qualified-typescript-oracle" && band.repetitions === 2 &&
      band.cases.length === 325 && new Set(band.cases.map((row) => row.case_id)).size === 325 &&
      eligible.length === 291 && deferred.length === 34 &&
      inBand(band.cases, "H2.7d") === 315 && inBand(eligible, "H2.7d") === 283 &&
      inBand(deferred, "H2.7d") === 32 &&
      inBand(band.cases, "H2.7e") === 13 && inBand(eligible, "H2.7e") === 11 &&
      inBand(deferred, "H2.7e") === 2 &&
      eligible.filter((row) => row.bands.length === 2).length === 3 &&
      deferred.every((row) => row.bands.length === 1) &&
      canonical(band.summary.union) === canonical({ candidates: 325, eligible: 291, deferred: 34 }) &&
      canonical(band.summary.bands) === canonical({
        "H2.7d": { candidates: 315, eligible: 283, deferred: 32 },
        "H2.7e": { candidates: 13, eligible: 11, deferred: 2 },
      }) &&
      canonical(band.summary.intersection) === canonical({ candidates: 3, eligible: 3, deferred: 0 }),
    "H2.7d/e close requires the frozen union325/eligible291/deferred34 and D283/E11/intersection3",
  );
  requireCondition(
    !fs.existsSync(path.join(WORKSPACE, "ratchets/h2-7d-known-divergences.v1.json")) &&
      !fs.existsSync(path.join(WORKSPACE, "ratchets/h2-7e-known-divergences.v1.json")) &&
      !fs.existsSync(path.join(WORKSPACE, "ratchets/h2-7de-known-divergences.v1.json")),
    "H2.7d/e close requires absent divergence manifests",
  );
  const identities = [band.generator, band.contract, ...band.inputs];
  requireCondition(
    identities.every((record) => canonical(record) === canonical(pathHash(record.path))),
    "H2.7d/e qualification producer or direct input identity changed",
  );
  return {
    artifact: pathHash(artifactPath),
    generator: band.generator,
    contract: band.contract,
    direct_inputs: band.inputs,
  };
}

function buildArtifact() {
  const qualification = readJson(QUALIFICATION_RELATIVE_PATH);
  const ownerControls = readJson(OWNER_CONTROLS_RELATIVE_PATH);
  const parentProfile = readJson(PARENT_PROFILE_RELATIVE_PATH);
  const h2_1aQualification = readJson(H2_1A_QUALIFICATION_RELATIVE_PATH);
  requireCondition(
    qualification.schema === 1 &&
      qualification.phase === "H2.5g-es2016-target" &&
      qualification.status === "qualified-typescript-oracle" &&
      qualification.selection_contract.global_h2_5g_rows === 11_910 &&
      qualification.selection_contract.global_candidate_denominator === 9_027 &&
      qualification.selection_contract.candidate_denominator === 9_027 &&
      qualification.selection_contract.future_deferred_rows === 2_883 &&
      qualification.summary.candidates === 9_027 &&
      qualification.summary.compiler_candidates === 4_712 &&
      qualification.summary.conformance_candidates === 4_315 &&
      qualification.summary.recorded_compiler_plan_cases === 4_712 &&
      qualification.summary.qualified_vfs_cases === 4_315 &&
      qualification.summary.virtual_config_cases === 56 &&
      qualification.summary.vfs_symlink_cases === 3 &&
      qualification.summary.vfs_symlink_paths === 4 &&
      qualification.summary.admitted_cases === 8_511 &&
      qualification.summary.deferred_cases === 516 &&
      qualification.summary.diagnostic_deferred_output_control_cases === 0 &&
      qualification.summary.source_deferred_cases === 516 &&
      qualification.summary.no_emit_control_cases === 59 &&
      qualification.summary.typescript_runs === 18_054 &&
      qualification.summary.deterministic_typescript_cases === 9_027 &&
      qualification.summary.admitted_typescript_writes === 9_466 &&
      qualification.summary.diagnostic_control_typescript_writes === 0 &&
      qualification.summary.admitted_typescript_diagnostics === 26_815 &&
      qualification.summary.unexecuted_candidates === 0 &&
      qualification.summary.undispositioned_candidates === 0 &&
      Array.isArray(qualification.cases) &&
      qualification.cases.length === 9_027 &&
      Array.isArray(qualification.owner_closure) &&
      qualification.owner_closure.length === 1 &&
      qualification.owner_closure[0].key === "transform-es2016",
    "H2.5g qualification is not closed",
  );
  requireCondition(
    ownerControls.schema === 1 &&
      ownerControls.phase === "H2.5g-es2016-target-owner-controls" &&
      ownerControls.status === "qualified" &&
      ownerControls.summary.controls === 22 &&
      ownerControls.summary.exact_outputs === 21 &&
      ownerControls.summary.typescript_runs === 44 &&
      ownerControls.summary.reported_diagnostics === 2 &&
      ownerControls.summary.emit_diagnostics === 1 &&
      ownerControls.summary.no_emit_on_error_controls === 1 &&
      ownerControls.summary.es2015_controls === 21 &&
      ownerControls.summary.es2016_controls === 1 &&
      ownerControls.summary.exponentiation_controls === 22 &&
      ownerControls.summary.exponentiation_assignment_controls === 15 &&
      ownerControls.summary.property_assignment_controls === 6 &&
      ownerControls.summary.element_assignment_controls === 5 &&
      ownerControls.summary.parameter_controls === 1 &&
      ownerControls.summary.collision_controls === 1 &&
      ownerControls.summary.super_controls === 1 &&
      ownerControls.summary.precedence_controls === 1 &&
      ownerControls.summary.comment_controls === 1 &&
      ownerControls.summary.class_composition_controls === 5 &&
      ownerControls.summary.commonjs_controls === 1 &&
      ownerControls.summary.async_composition_controls === 2 &&
      ownerControls.summary.using_controls === 1 &&
      ownerControls.summary.h2_5a_active_controls === 21 &&
      ownerControls.summary.h2_5b_active_controls === 21 &&
      ownerControls.summary.h2_5c_active_controls === 21 &&
      ownerControls.summary.h2_5d_active_controls === 21 &&
      ownerControls.summary.h2_5e_active_controls === 21 &&
      ownerControls.summary.h2_5f_active_controls === 21 &&
      ownerControls.summary.h2_5g_active_controls === 20 &&
      Array.isArray(ownerControls.controls) &&
      ownerControls.controls.length === 22,
    "H2.5g owner controls are not closed",
  );
  requireCondition(
    parentProfile.schema === 1 &&
      parentProfile.phase === "H2.5f" &&
      parentProfile.admitted_profile.exact_cases === 680 &&
      parentProfile.summary.completed_runtime_slices === 21 &&
      qualification.typescript.version === "6.0.3" &&
      qualification.typescript.source_commit ===
        "050880ce59e30b356b686bd3144efe24f875ebc8" &&
      canonical(ownerControls.typescript) === canonical(qualification.typescript) &&
      canonical(parentProfile.typescript) === canonical(qualification.typescript),
    "H2.5f parent profile is not closed",
  );

  const historical = Object.fromEntries(
    HISTORICAL_AUTHORITIES.map(([key, relativePath, expected]) => {
      const record = pathHash(relativePath);
      requireCondition(record.sha256 === expected, `${relativePath} historical bytes changed`);
      return [key, record];
    }),
  );
  const h2_1aQualificationRecord = pathHash(H2_1A_QUALIFICATION_RELATIVE_PATH);
  requireCondition(
    h2_1aQualificationRecord.sha256 === H2_1A_QUALIFICATION_SHA256,
    `${H2_1A_QUALIFICATION_RELATIVE_PATH} historical bytes changed`,
  );
  const h2_1aHistoricalControls = h2_1aQualification.cases?.filter(
    (candidate) => candidate.disposition === "diagnostic-deferred-output-control",
  );
  requireCondition(
    h2_1aHistoricalControls?.length === H2_1A_CURRENT_EXACT_PROMOTIONS.length &&
      h2_1aHistoricalControls.every((candidate) =>
        H2_1A_CURRENT_EXACT_PROMOTIONS.some(
          (promotion) => promotion.case_id === candidate.case_id,
        ),
      ),
    "H2.1a current exact promotion denominator changed",
  );
  const h2_1aCurrentExactPromotions = H2_1A_CURRENT_EXACT_PROMOTIONS.map(
    (promotion) => {
      const candidate = h2_1aHistoricalControls.find(
        (candidate) => candidate.case_id === promotion.case_id,
      );
      requireCondition(
        candidate?.case_fingerprint_sha256 ===
            promotion.historical_case_fingerprint_sha256 &&
          candidate.disposition === promotion.historical_disposition &&
          candidate.diagnostic_disposition?.state ===
            promotion.historical_diagnostic_state &&
          candidate.typescript_runs?.length === 2 &&
          candidate.typescript_runs.every(
            (run) =>
              run.reported_diagnostics?.length ===
                promotion.exact_reported_diagnostics &&
              run.writes?.length === promotion.exact_writes,
          ),
        `${promotion.case_id} current exact promotion evidence changed`,
      );
      return {
        ...promotion,
        historical_qualification: h2_1aQualificationRecord,
      };
    },
  );
  const declarationBand = readJson("ratchets/h2-7b-qualification.v1.json");
  requireCondition(
    declarationBand.summary.candidates === 1_593 &&
      declarationBand.summary.admitted_cases === 1_557 &&
      declarationBand.summary.deferred_cases === 36 &&
      declarationBand.cases.filter((row) => row.disposition === "admitted-for-execution").length === 1_557 &&
      !fs.existsSync(path.join(WORKSPACE, "ratchets/h2-7b-known-divergences.v1.json")),
    "H2.7b close requires the frozen 1,557/36 band and an absent divergence manifest",
  );
  const declarationOptionsBand = readJson("ratchets/h2-7c-qualification.v1.json");
  requireCondition(
    declarationOptionsBand.status === "qualified-typescript-oracle" &&
      declarationOptionsBand.summary.corpus === 42 &&
      declarationOptionsBand.summary.exact === 31 &&
      declarationOptionsBand.summary.deferred === 11 &&
      declarationOptionsBand.summary.boundary_probes === 1 &&
      declarationOptionsBand.cases.filter((row) => row.disposition === "exact").length === 31 &&
      !fs.existsSync(path.join(WORKSPACE, "ratchets/h2-7c-known-divergences.v1.json")),
    "H2.7c close requires the original 31/11 band and an absent divergence manifest",
  );
  const declarationOutputQualification = declarationOutputQualificationEvidence();
  const runtimeInputPaths = [
    ...parentProfile.runtime_inputs.map((record) => record.path),
    ...NEW_RUNTIME_INPUTS,
  ];
  const runtimeInputSet = new Set(runtimeInputPaths);
  const parentRuntimeInputSet = new Set(
    parentProfile.runtime_inputs.map((record) => record.path),
  );
  const changedRuntimeInputs = changedRuntimeInputPaths();
  const missingRuntimeInputs = changedRuntimeInputs.filter(
    (relativePath) => !runtimeInputSet.has(relativePath),
  );
  const staleNewRuntimeInputs = NEW_RUNTIME_INPUTS.filter(
    (relativePath) =>
      parentRuntimeInputSet.has(relativePath) ||
      !changedRuntimeInputs.includes(relativePath),
  );
  requireCondition(
    missingRuntimeInputs.length === 0,
    `H2.5g runtime input closure is missing ${missingRuntimeInputs.join(", ")}`,
  );
  requireCondition(
    staleNewRuntimeInputs.length === 0,
    `H2.5g new runtime inputs are stale ${staleNewRuntimeInputs.join(", ")}`,
  );
  requireCondition(
    runtimeInputSet.size === 920,
    `H2.5g runtime input identity changed (measured ${runtimeInputSet.size}, pinned 920)`,
  );

  return withFingerprint(
    {
      schema: 1,
      kind: "h2-runtime-profile",
      status: "qualified",
      phase: "H2.5g",
      typescript: qualification.typescript,
      generator: pathHash(GENERATOR_RELATIVE_PATH),
      contract: pathHash(CONTRACT_RELATIVE_PATH),
      origin: {
        trusted_h2_5f_merge: TRUSTED_BASE,
        historical,
        interpretation:
          "H2.5f artifacts remain immutable lineage; current runtime ownership transfers to this H2.5g profile",
      },
      qualification: pathHash(QUALIFICATION_RELATIVE_PATH),
      current_exact_promotions: h2_1aCurrentExactPromotions,
      runtime_inputs: runtimeInputPaths.map(pathHash),
      admitted_profile: {
        execution: "single-project-one-shot-whole-program",
        target_states: [
          "ES2015(2)", "ES2016(3)", "ES2017(4)", "ES2018(5)", "ES2019(6)",
          "ES2020(7)", "ES2021(8)", "ES2022(9)", "ES2023(10)",
          "ES2024(11)", "ES2025(12)", "ESNext(99)",
        ],
        module_states: [
          "absent-effective-ESNext", "None(0)", "ES2015(5)", "ES2020(6)",
          "ES2022(7)", "ESNext(99)", "CommonJS(1)", "AMD(2)", "UMD(3)",
          "System(4)", "Node16(100)", "Node18(101)", "Node20(102)",
          "NodeNext(199)", "Preserve(200)",
        ],
        jsx_modes: [
          "Preserve(1)", "React(2)", "ReactNative(3)", "ReactJSX(4)", "ReactJSXDev(5)",
        ],
        source_kinds: [
          ".ts", ".mts", ".cts", ".tsx", ".js", ".mjs", ".cjs", ".jsx", ".json",
        ],
        products: ["javascript", "mjs", "cjs", "jsx", "json"],
        exact_cases: 9_196,
        h2_5g_exact_cases: 8_511,
        exact_reported_diagnostics: 28_415,
        exact_writes: 10_445,
        diagnostic_deferred_output_controls: 0,
        diagnostic_control_writes: 0,
        source_deferred_cases: 531,
        candidate_denominator: 9_715,
        h2_5g_candidate_denominator: 9_027,
        h2_5g_global_future_rows: 2_883,
        h2_5g_owner_controls: 22,
        h2_5g_owner_writes: 21,
      },
      transition: {
        completed_slice: "H2.5g",
        // H2.7d/e adds 291 band admissions, unique within their shared union.
        // The original admitted_profile remains the frozen H2.5g band.
        next_slice: "H2.8a",
        next_slice_scope: "full-output-matrix",
        next_runtime_activation_slice: "H2.8a",
        active_runtime_slices: [
          "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a",
          "H2.2b", "H2.2c", "H2.2d", "H2.3a", "H2.3b", "H2.3c",
          "H2.3d", "H2.4a", "H2.4b", "H2.5a", "H2.5b", "H2.5c",
          "H2.5d", "H2.5e", "H2.5f", "H2.5g", "H2.5h", "H2.6a",
          "H2.6b", "H2.6c", "H2.7b", "H2.7c", "H2.7d", "H2.7e",
        ],
        inactive_runtime_slice_count: 7,
        classic_jsx_tsx_owner: "complete",
        automatic_jsx_runtime_owner: "complete",
        json_output_owner: "complete",
        legacy_decorators_owner: "complete",
        standard_decorators_and_class_fields_owner: "complete",
        target_esnext_transform_owner: "complete",
        target_es2021_transform_owner: "complete",
        target_es2020_transform_owner: "complete",
        target_es2019_transform_owner: "complete",
        target_es2018_transform_owner: "complete",
        target_es2017_transform_owner: "complete",
        target_es2016_transform_owner: "complete",
        target_es2015_transform_owner: "complete-with-h2-5h-divergence-ratchet",
        target_generators_transform_owner: "complete-with-h2-5h-divergence-ratchet",
        general_output_matrix_owner: "H2.8a",
        h2_5g_candidate_cases: 9_027,
        h2_5g_admitted_cases: 8_511,
        h2_5h_candidate_cases: 932,
        h2_5h_admitted_cases: 888,
        h2_5h_exact_cases: 795,
        h2_5h_known_divergences: 93,
        h2_5h_source_deferred_cases: 44,
        h2_6a_candidate_cases: 177,
        h2_6a_admitted_cases: 175,
        h2_6a_exact_cases: 130,
        h2_6a_known_divergences: 45,
        h2_6a_source_deferred_cases: 2,
        h2_6b_candidate_cases: 6,
        h2_6b_admitted_cases: 6,
        h2_6b_exact_cases: 4,
        h2_6b_known_divergences: 2,
        h2_6b_source_deferred_cases: 0,
        h2_6c_candidate_cases: 643,
        h2_6c_admitted_cases: 639,
        h2_6c_exact_cases: 481,
        h2_6c_known_divergences: 158,
        h2_6c_source_deferred_cases: 4,
        h2_7b_candidate_cases: 1_593,
        h2_7b_admitted_cases: 1_557,
        h2_7b_exact_cases: 1_557,
        h2_7b_known_divergences: 0,
        h2_7b_source_deferred_cases: 36,
        h2_7c_candidate_cases: 42,
        h2_7c_admitted_cases: 31,
        h2_7c_exact_cases: 31,
        h2_7c_known_divergences: 0,
        h2_7c_source_deferred_cases: 11,
        // Final per-band coverage overlaps by three. The joint band is counted
        // once; the dependency-order deltas assign the shared rows to E.
        // These deltas do not assert a separately tested D-only runtime state.
        h2_7d_candidate_cases: 315,
        h2_7d_admitted_cases: 283,
        h2_7d_exact_cases: 283,
        h2_7d_known_divergences: 0,
        h2_7d_source_deferred_cases: 32,
        h2_7e_candidate_cases: 13,
        h2_7e_admitted_cases: 11,
        h2_7e_exact_cases: 11,
        h2_7e_known_divergences: 0,
        h2_7e_source_deferred_cases: 2,
        h2_7de_candidate_cases: 325,
        h2_7de_admitted_cases: 291,
        h2_7de_exact_cases: 291,
        h2_7de_known_divergences: 0,
        h2_7de_source_deferred_cases: 34,
        h2_7de_intersection_cases: 3,
        h2_7d_runtime_admissions_delta: 280,
        h2_7e_runtime_admissions_delta: 11,
        h2_5g_global_future_rows: 2_883,
        h2_5g_source_deferred_cases: 516,
        deferred_failure_boundary: "typed failure before first sink write",
      },
      evidence: {
        typescript_repetitions: 2,
        rust_repetitions: 2,
        legal_worker_control: "h2_5g_cases_and_owner_controls_run_twice_in_isolated_programs",
        denominator_control: "h2_5g_exact_denominator_is_9027_with_516_source_deferred_cases_and_2883_global_future_rows",
        target_band_control: "es2015_lowers_es2016_syntax_while_es2016_preserves_it",
        exponentiation_control: "binary_and_assignment_exponentiation_preserve_associativity_evaluation_and_temp_ownership",
        generated_binding_control: "typed_scope_hoists_and_binding_identity_preserve_property_element_parameter_and_collision_names",
        composition_control: "async_object_rest_decorators_class_fields_using_and_commonjs_compose_in_transform_order",
        diagnostic_control: "reported_emit_and_no_emit_on_error_diagnostics_match_tsc",
        printer_control: "comments_precedence_and_final_tree_layout_are_exact",
        failure_control: "516_later_owned_sources_fail_before_first_sink_write_and_no_emit_on_error_writes_nothing",
        owner_controls: {
          artifact: pathHash(OWNER_CONTROLS_RELATIVE_PATH),
          generator: pathHash("crates/oracle/h2-5g-owner-controls.mjs"),
          contract: pathHash(".github/ci/contracts/h2-5g-owner-controls.schema.json"),
        },
        declaration_output_qualification: declarationOutputQualification,
        qualification_vfs_overlay_test: pathHash(
          "crates/oracle/vfs-directory-overlay.test.mjs",
        ),
        h0_authority: pathHash("ratchets/h0-qualification.v1.json"),
        h1_authority: pathHash("ratchets/h1-emit-qualification.v1.json"),
        l1_authority: pathHash("ratchets/l1-incremental-parser-performance.v1.json"),
        historical_h2_5f_profile: historical.profile,
        local_full_gate: `cargo xtask ci --baseline ${TRUSTED_BASE}`,
        hosted_gate: "cargo xtask acceptance",
        hosted_gate_scope: "fixed-unsplit-ts-tests-only",
      },
      summary: {
        completed_runtime_slices: 29,
        next_slice_runtime_slice_delta: 0,
        runtime_admissions: 11_075,
        executed_candidates: 11_594,
        h2_5g_executed_candidates: 9_027,
        h2_5g_global_future_rows: 2_883,
        unexecuted_candidates: 0,
        undispositioned_candidates: 0,
        historical_artifacts_reinterpreted: 0,
      },
    },
    "profile_fingerprint_sha256",
  );
}

function render(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

const artifact = buildArtifact();
const rendered = render(artifact);
const mode = process.argv[2];
if (mode === "--write") {
  fs.writeFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), rendered);
  process.stdout.write(
    `wrote ${TARGET_RELATIVE_PATH}: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === "--check") {
  requireCondition(
    fs.existsSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH)) &&
      fs.readFileSync(path.join(WORKSPACE, TARGET_RELATIVE_PATH), "utf8") === rendered,
    `stale ${TARGET_RELATIVE_PATH}; run h2-5g-profile.mjs --write and review`,
  );
  process.stdout.write(
    `H2.5g profile is fresh: exact=${artifact.admitted_profile.exact_cases} next=${artifact.transition.next_slice}\n`,
  );
} else if (mode === undefined) {
  process.stdout.write(rendered);
} else {
  fail("usage: h2-5g-profile.mjs [--write|--check]");
}
