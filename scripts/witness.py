#!/usr/bin/env python3
"""Run a focused witness selection without building xtask first."""
import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import time
import foundation_witnesses
import emitter_final_witnesses

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "crates/compiler/tests/fixtures"
SUPER = {
    "primary": ("", "TSC_RS_DECORATOR_SUPER_CASE_SET"),
    **{name: (f"-{name}", f"TSC_RS_DECORATOR_SUPER_{name.upper()}_CASE_SET")
       for name in ("extra", "followup", "followup2", "followup3")},
}
# Small direct targets share the hosted printer build, but retain individual
# selection. Each fixture tuple is (path, row count, ID key). These counts are
# input memberships, not a claim about complete compiler command equivalence.
EMITTER_DIRECT = {
    "literal-update": {
        "target": "literal_update_contract",
        "fixtures": tuple((f"crates/emitter/tests/fixtures/literal-update-{group}.json", count, "case_id")
                          for group, count in (("factory", 987), ("transform", 399), ("lifetime", 10))),
        "observers": tuple(("scripts/observe-literal-update.mjs", group)
                           for group in ("factory", "transform", "lifetime")),
    },
    "compact-body-comments": {
        "target": "compact_body_comments_contract",
        "fixtures": (("crates/emitter/tests/fixtures/compact-body-comments.json", 240, "case_id"),),
        "observers": ("scripts/observe-compact-body-comments.mjs",),
    },
    "literal-parent-provenance": {
        "target": "literal_parent_provenance_contract",
        "fixtures": (("crates/emitter/tests/fixtures/literal-parent-provenance-utf16.json", 128, "case_id"),),
        "observers": ("scripts/observe-literal-parent-provenance-utf16.mjs",),
        "inputs": ("scripts/observe-literal-parent-provenance.mjs",
                   "crates/emitter/tests/fixtures/literal-parent-provenance.json"),
    },
    "literal-value-provenance": {
        "target": "literal_value_provenance_contract",
        "fixtures": (("crates/emitter/tests/fixtures/template-raw-provenance.json", 480, "case_id"),
                     ("crates/emitter/tests/fixtures/string-property-provenance.json", 60, "case_id")),
        "observers": ("scripts/observe-template-raw-provenance.mjs", "scripts/observe-string-property-provenance.mjs"),
    },
    "string-literal-identifier-source": {
        "target": "string_literal_identifier_source_contract",
        "fixtures": (("crates/emitter/tests/fixtures/string-literal-identifier-source.json", 72, "case_id"),),
        "observers": ("scripts/observe-string-literal-identifier-source.mjs",),
    },
    "utf16-literal-escaping": {
        "target": "utf16_literal_escaping_contract",
        "fixtures": (("crates/emitter/tests/fixtures/utf16-literal-escaping.json", 288, "case_id"),
                     ("crates/emitter/tests/fixtures/utf16-declaration-literal-printer.json", 8, "id")),
        "observers": ("scripts/observe-utf16-literal-escaping.mjs", "scripts/observe-utf16-declaration-literal-printer.mjs"),
    },
    "class-header-token-metadata": {
        "target": "class_header_token_metadata_contract",
        "fixtures": (("crates/emitter/tests/fixtures/class-header-token-printer-metadata.json", 32, "case_id"),),
        "observers": ("scripts/observe-class-header-token-printer-metadata.mjs",),
    },
    "comma-argument-factory": {
        "target": "comma_argument_factory_contract",
        "fixtures": (("crates/emitter/tests/fixtures/comma-argument-factory.json", 44, "case_id"),
                     ("crates/emitter/tests/fixtures/list-intervening-owners.json", 96, "case_id"),
                     ("crates/emitter/tests/fixtures/list-trailing-token-owners.json", 104, "case_id"),
                     ("crates/emitter/tests/fixtures/list-boundary-lines.json", 264, "case_id"),
                     ("ratchets/h2-8a-list-cursor-lifecycle.v1.json", 11, "case_id")),
        "observers": tuple(f"scripts/observe-{name}.mjs" for name in (
            "comma-argument-factory", "list-intervening-owners", "list-trailing-token-owners",
            "list-boundary-lines", "list-cursor-lifecycle")),
    },
    "ellipsis-comment-metadata": {
        "target": "ellipsis_comment_metadata_contract",
        "fixtures": (("crates/emitter/tests/fixtures/ellipsis-comment-printer-metadata.json", 144, "case_id"),),
        "observers": ("scripts/observe-ellipsis-comment-printer-metadata.mjs",),
    },
    "import-type-attributes": {
        "target": "import_type_attributes_contract",
        "fixtures": (("crates/emitter/tests/fixtures/import-type-attributes.json", 84, "case_id"),),
        "observers": ("scripts/observe-import-type-attributes.mjs",),
    },
    "mapped-type-members": {
        "target": "mapped_type_members_contract",
        "fixtures": (("crates/emitter/tests/fixtures/mapped-type-members.json", 328, "case_id"),),
        "observers": ("scripts/observe-mapped-type-members.mjs",),
    },
    "token-comment-phase-metadata": {
        "target": "token_comment_phase_metadata_contract",
        "fixtures": (("crates/emitter/tests/fixtures/token-comment-phase-printer-metadata.json", 96, "case_id"),),
        "observers": ("scripts/observe-token-comment-phase-printer-metadata.mjs",),
    },
    # A41-BINDING (C02): synthetic-census, global-oracle and lifecycle controls
    # of the generated-name domains (printed text only).
    "decorator-binding": {
        "target": "decorator_binding_contract",
        "fixtures": (("crates/emitter/tests/fixtures/decorator-binding-direct.json", 146, "case_id"),
                     ("crates/emitter/tests/fixtures/decorator-binding-carry-edge.json", 10, "case_id")),
        "observers": (("scripts/observe-decorator-bindings.mjs", "direct"),
                      "scripts/observe-decorator-binding-carry.mjs"),
    },
}
CONFIG_LIBRARY_GROUPS = (
    ("config-commands", 8), ("config-conversion-commands", 8),
    ("config-diagnostic-commands", 12), ("config-diagnostic-routing", 8),
    ("config-discovery-commands", 8), ("config-entity-commands", 4),
    ("config-extension-commands", 4), ("config-root-commands", 12),
    ("config-source-commands", 8), ("config-source-span-commands", 6),
    ("library-replacement", 12), ("library-order", 6),
)
# These compiler witnesses have dedicated inputs or additional command fields.
# Shared helper tests stay in acceptance; select the dedicated test where needed.
COMPILER_DIRECT = {
    "module-identities": {
        "target": "h2_7d_module_identities",
        "tests": 3,
        # 38 ordinary native inputs; four API and 14 path-helper references
        # remain oracle-only memberships, never additional native executions.
        "fixtures": (("crates/emitter/tests/fixtures/bundle-module-identities.json", 24, "case_id"),
                     ("crates/emitter/tests/fixtures/system-generated-names.json", 12, "case_id"),
                     ("crates/emitter/tests/fixtures/module-alias-underscores.json", 6, "case_id")),
        "fixture_sections": (("crates/emitter/tests/fixtures/bundle-module-identities.json",
                              "path_cases", 14, "case_id"),),
        "observers": ("scripts/observe-bundle-module-identities.mjs",
                      "scripts/observe-system-generated-names.mjs",
                      "scripts/observe-module-alias-underscores.mjs"),
    },
    "bundle-original-javascript": {
        "target": "h2_7d_declaration_bundles",
        "test": "original_javascript_declaration_bundles_match_typescript_twice",
        "tests": 1,
        "filtered_tests": 3,
        "fixtures": (("docs/design/greenfield/slices/witness-coverage/compiler-module-facets/original-javascript-inputs.v1.json",
                      4, "case_id"),),
        # Shared ratchet/oracle dependencies retain the planner's full fallback.
        "observers": ("scripts/observe-bundle-original-javascript.mjs",),
    },
    "declaration-map-apis": {
        "target": "h2_7e_declaration_map_apis",
        "tests": 3,
        "fixtures": (("crates/compiler/tests/fixtures/declaration-map-apis.json", 54, "case_id"),
                     ("crates/compiler/tests/fixtures/declaration-reference-paths.json", 8, "case_id")),
        "fixture_sections": (
            ("crates/compiler/tests/fixtures/declaration-map-apis.json",
             "adjacent_ordinary_api_observations", 2, "case_id"),
            ("crates/compiler/tests/fixtures/declaration-reference-paths.json",
             "supplemental_reference_targets", 3, "case_id"),
            ("crates/compiler/tests/fixtures/declaration-reference-paths.json",
             "adjacent_out_dir_observations", 8, "case_id"),
        ),
        "observers": ("scripts/observe-declaration-map-apis.mjs",
                      "scripts/observe-declaration-reference-paths.mjs"),
    },
    "declaration-maps": {
        "target": "h2_7e_declaration_maps",
        "tests": 8,
        "fixtures": (("crates/compiler/tests/fixtures/declaration-maps.json", 24, "case_id"),
                     ("crates/compiler/tests/fixtures/declaration-maps-runtime.json", 3, "case_id"),
                     ("crates/compiler/tests/fixtures/declaration-maps-disabled-declaration.json", 2, "case_id"),
                     ("crates/compiler/tests/fixtures/declaration-map-bundle-boundary.json", 1, "case_id"),
                     ("crates/compiler/tests/fixtures/declaration-map-apis.json", 54, "case_id")),
        "observers": ("scripts/observe-declaration-maps.mjs",
                      ("scripts/observe-declaration-maps.mjs", "--runtime"),
                      ("scripts/observe-declaration-maps.mjs", "--disabled-declaration"),
                      ("scripts/observe-declaration-maps.mjs", "--bundle-boundary"),
                      "scripts/observe-declaration-map-apis.mjs"),
    },
    "bundle-program": {
        "target": "h2_7d_bundle_program",
        "tests": 4,
        "fixtures": (("crates/emitter/tests/fixtures/bundle-declarations.json", 25, "case_id"),),
        "fixture_sections": (("crates/emitter/tests/fixtures/bundle-declarations.json",
                              "adjacent_owner_references", 2, "case_id"),),
        "observers": ("scripts/observe-bundle-declarations.mjs",),
    },
    "bundle-declarations": {
        "target": "h2_7d_declaration_bundles",
        "test": ("ordinary_declaration_bundles_match_typescript_visitor_and_printer_twice",
                 "ordinary_bundle_source_maps_match_complete_typescript_maps_twice",
                 "ordinary_and_fresh_forced_bundle_metadata_lifetimes_match_typescript_twice"),
        "tests": 3,
        # The original-JavaScript recorder has its own registered selection.
        "filtered_tests": 1,
        "fixtures": (("crates/emitter/tests/fixtures/bundle-declarations.json", 25, "case_id"),
                     ("crates/emitter/tests/fixtures/bundle-maps.json", 12, "case_id")),
        "fixture_sections": tuple(("crates/emitter/tests/fixtures/bundle-maps.json", section, count, "case_id")
                                  for section, count in (("json_bundle_references", 8),
                                                         ("metadata_lifetime_references", 6),
                                                         ("constant_value_references", 2),
                                                         ("runtime_comment_owner_references", 3))),
        "observers": ("scripts/observe-bundle-declarations.mjs", "scripts/observe-bundle-maps.mjs"),
    },
    "utf16-recovery-corpus": {
        "target": "h2_8a_utf16_literal_recovery_corpus",
        "tests": 1,
        "fixtures": (("crates/compiler/tests/fixtures/utf16-literal-recovery-corpus.json", 50, "case_id"),),
        "observers": (("scripts/observe-utf16-literal-recovery-corpus.mjs", "--census",
                       "target/declaration-comment-ranges-runs/utf16-literal-recovery-census.json"),),
        # The frozen observer records this exact relative path in its result.
        # Preserve both its source hash and the original observation bytes.
        "staged_inputs": (("crates/compiler/tests/fixtures/utf16-literal-recovery-census.json",
                           "target/declaration-comment-ranges-runs/utf16-literal-recovery-census.json",
                           "16fafabaac5e46d99e0caebd37f215d387a2199dbd36f0473f9fbd678cb78efb"),),
    },
    "map-option-projection": {
        "target": "h2_6a_map_option_projection",
        "test": ("original_floor_divergence_and_existing_map_family_parity",
                 "h2_6a_rows_and_adjacent_controls_match_complete_frozen_tuples",
                 "map_option_directives_and_virtual_configs_match_typescript"),
        "tests": 3,
        "filtered_tests": 2,
        "fixtures": (("crates/compiler/tests/fixtures/h2-6a-map-option-projection.json", 31, "case_id"),),
        "observers": ("crates/oracle/h2-6a-map-option-projection.mjs",),
    },
    # These exact tests share the large contracts binary. Only their dedicated
    # modules/fixtures own this suite; contracts.rs remains a shared input.
    "config-library": {
        "target": "contracts",
        "test": (
            "h2_8b_config_commands::config_commands_match_complete_typescript_observations",
            "h2_8b_config_commands::config_commands_match_ordered_program_facts",
            "h2_8b_config_conversion_commands::config_conversion_commands_match_complete_typescript_observations",
            "h2_8b_config_conversion_commands::config_conversion_commands_match_ordered_program_facts",
            "h2_8b_config_diagnostic_commands::config_diagnostic_commands_match_complete_typescript_observations",
            "h2_8b_config_diagnostic_commands::config_diagnostic_commands_match_ordered_program_facts",
            "h2_8b_config_diagnostic_routing::config_diagnostic_routing_match_complete_typescript_observations",
            "h2_8b_config_diagnostic_routing::config_diagnostic_routing_match_ordered_program_facts",
            "h2_8b_config_discovery_commands::config_discovery_commands_match_complete_typescript_observations",
            "h2_8b_config_discovery_commands::config_discovery_commands_match_ordered_program_facts",
            "h2_8b_config_entity_commands::config_entity_commands_match_complete_typescript_observations",
            "h2_8b_config_entity_commands::config_entity_commands_match_ordered_program_facts",
            "h2_8b_config_extension_commands::config_extension_commands_match_complete_typescript_observations",
            "h2_8b_config_extension_commands::config_extension_commands_match_ordered_program_facts",
            "h2_8b_config_root_commands::config_root_commands_match_complete_typescript_observations",
            "h2_8b_config_root_commands::config_root_commands_match_ordered_program_facts",
            "h2_8b_config_source_commands::config_source_commands_match_observations_and_module_boundaries",
            "h2_8b_config_source_commands::config_source_commands_match_ordered_program_facts",
            "h2_8b_config_source_span_commands::config_source_span_commands_match_complete_typescript_observations",
            "h2_8b_config_source_span_commands::config_source_span_commands_match_ordered_program_facts",
            "h2_8b_library_replacement::library_replacement_matches_complete_typescript_observations",
            "h2_8b_library_replacement::library_replacement_matches_program_membership",
            "h2_8b_library_replacement::library_order_controls_match_complete_typescript_observations",
            "h2_8b_library_replacement::library_order_controls_match_program_membership",
        ),
        "tests": 24,
        "filtered_tests": 423,
        "sources": tuple(f"crates/compiler/tests/integration/{module}.rs" for module in (
            "h2_8b_config_commands",
            "h2_8b_config_conversion_commands",
            "h2_8b_config_diagnostic_commands",
            "h2_8b_config_diagnostic_routing",
            "h2_8b_config_discovery_commands",
            "h2_8b_config_entity_commands",
            "h2_8b_config_extension_commands",
            "h2_8b_config_root_commands",
            "h2_8b_config_source_commands",
            "h2_8b_config_source_span_commands",
            "h2_8b_library_replacement",
        )),
        "fixtures": tuple((f"crates/compiler/tests/fixtures/h2-8b-{group}.json", count, "case_id")
                          for group, count in CONFIG_LIBRARY_GROUPS),
        "observers": tuple((f"scripts/observe-h2-8b-{group}.mjs", group)
                           for group, _ in CONFIG_LIBRARY_GROUPS),
        "inputs": tuple(f"crates/compiler/tests/fixtures/h2-8b-{group}-inputs.json"
                        for group, _ in CONFIG_LIBRARY_GROUPS),
    },
    "prologue-comments": {
        "target": "h2_8a_prologue_only_detached_comments",
        "tests": 1,
        "fixtures": (("crates/compiler/tests/fixtures/prologue-only-detached-comments.json", 8, "id"),),
        "observers": ("scripts/observe-prologue-only-detached-comments.mjs",),
    },
    "literal-update-pipeline": {
        "target": "literal_update_pipeline_contract",
        "tests": 1,
        "fixtures": (("crates/compiler/tests/fixtures/literal-update-pipeline.json", 22, "case_id"),),
        "observers": (("scripts/observe-literal-update.mjs", "pipeline"),),
    },
    "require-rewrite": {
        "target": "h2_8a_require_rewrite",
        "test": tuple(f"require_rewrite_{group}_complete_commands"
                      for group in ("focused", "composition", "substitution", "dynamic")),
        "tests": 4,
        "filtered_tests": 10,
        "fixtures": tuple((f"crates/compiler/tests/fixtures/h2-8a-require-rewrite{suffix}.json", count, "case_id")
                          for suffix, count in (("", 60), ("-composition", 4), ("-substitution", 4), ("-dynamic", 6))),
        "observers": tuple((f"scripts/observe-require-rewrite{suffix}.mjs", f"require-rewrite{suffix}")
                           for suffix in ("", "-composition", "-substitution", "-dynamic")),
        "inputs": tuple(f"crates/compiler/tests/fixtures/h2-8a-require-rewrite{suffix}-inputs.json"
                        for suffix in ("", "-composition", "-substitution", "-dynamic")),
    },
    "declaration-specifiers": {
        "target": "h2_8a_declaration_specifiers",
        "test": ("focused_declaration_specifiers_match_complete_commands",
                 "composition_declaration_specifiers_match_complete_commands"),
        "tests": 2,
        "filtered_tests": 9,
        "fixtures": tuple((f"crates/compiler/tests/fixtures/h2-8a-{group}.json", count, "case_id")
                          for group, count in (("declaration-specifiers", 24), ("declaration-specifiers-composition", 6))),
        "observers": tuple((f"scripts/observe-h2-8a-{group}.mjs", group)
                           for group in ("declaration-specifiers", "declaration-specifiers-composition")),
        "inputs": tuple(f"crates/compiler/tests/fixtures/h2-8a-{group}-inputs.json"
                        for group in ("declaration-specifiers", "declaration-specifiers-composition")),
    },
    "declaration-comments": {
        "target": "h2_8a_declaration_comment_ranges",
        "test": ("declaration_comment_range_focused_complete_commands",
                 "declaration_comment_detached_prefix_complete_commands",
                 "declaration_comment_parameter_tags_complete_commands"),
        "tests": 3,
        "filtered_tests": 12,
        "fixtures": tuple((f"crates/compiler/tests/fixtures/declaration-comment-{group}.json", count, "case_id")
                          for group, count in (("ranges", 17), ("detached-prefixes", 12), ("parameter-tags", 12))),
        "observers": ("scripts/observe-declaration-comment-commands.mjs",),
        # The reused observer serves other owners too; its path keeps the
        # planner's conservative shared-input rule, rather than owning it here.
    },
    "jsdoc-return": {
        "target": "h2_8a_jsdoc_return",
        "test": "jsdoc_return_controls_match_complete_commands_twice",
        "tests": 1,
        "filtered_tests": 1,
        "fixtures": (("crates/compiler/tests/fixtures/h2-8a-jsdoc-return.json", 58, "id"),),
        "observers": ("scripts/observe-h2-8a-jsdoc-return.mjs",),
    },
    # H2.8a-A-RES-BUNDLE-METADATA-T1: parse-node comment ranges carried from a
    # bundle's JavaScript transform into its declaration transform (18
    # complete commands + upstream emitNode probes, two tests).
    "bundle-metadata-t1": {
        "target": "bundle_metadata_t1_contract",
        "test": ("bundle_metadata_t1_controls_match_complete_typescript_observations",
                 "bundle_metadata_t1_parsed_packet_matches_typescript_after_javascript_probe"),
        "tests": 2,
        # The imported exact comparator module carries its own eight tests.
        "filtered_tests": 8,
        "fixtures": (("crates/compiler/tests/fixtures/bundle-metadata-t1.json", 18, "case_id"),),
        "observers": ("scripts/observe-bundle-metadata-t1.mjs",),
        "inputs": tuple(f"crates/compiler/tests/fixtures/bundle-metadata-t1-{name}.json"
                        for name in ("inputs", "known-native", "known-packet"))
                  + ("scripts/generate-bundle-metadata-t1-inputs.mjs",),
    },
    # H2.8a-A-RES-POST-T1: adjacent controls for the R9 / R12 / receiver-map /
    # private-set-comments / decorator-comments residuals (101 complete
    # commands + upstream emitNode probes for the bundle rows, two tests).
    "post-t1-residuals": {
        "target": "post_t1_residuals_contract",
        "test": ("post_t1_residuals_controls_match_complete_typescript_observations",
                 "post_t1_residuals_parsed_packet_matches_typescript_after_javascript_probe"),
        "tests": 2,
        # The imported exact comparator module carries its own eight tests.
        "filtered_tests": 8,
        "fixtures": (("crates/compiler/tests/fixtures/post-t1-residuals.json", 101, "case_id"),),
        "observers": ("scripts/observe-post-t1-residuals.mjs",),
        "inputs": tuple(f"crates/compiler/tests/fixtures/post-t1-residuals-{name}.json"
                        for name in ("inputs", "known-native", "known-packet"))
                  + ("scripts/generate-post-t1-residuals-inputs.mjs",),
    },
    "parameter-temporaries": {
        "target": "h2_5h_parameter_temporaries",
        "tests": 2,
        "fixtures": (("crates/compiler/tests/fixtures/h2-5h-parameter-temporaries.json", 68, "case_id"),),
        "observers": ("scripts/observe-h2-5h-parameter-temporaries.mjs",),
        "inputs": ("docs/design/greenfield/slices/h2-5h-parameter-temporaries-selection.v1.json",),
    },
    "transpile-routes": {
        "target": "transpile_routes_contract",
        "tests": 9,
        "fixtures": (("crates/compiler/tests/fixtures/h2_8c_transpile/inputs.v1.json", 287, "id"),
                     ("crates/compiler/tests/fixtures/h2_8c_transpile/review-inputs.v1.json", 14, "id")),
        "observers": ("scripts/observe-transpile-routes.mjs",),
        "inputs": tuple(f"crates/compiler/tests/fixtures/h2_8c_transpile/{name}.v1.json"
                        for name in ("expected", "known-open", "known-native", "review-expected")),
    },
    "utf16-identity-recovery": {
        "target": "h2_8a_utf16_identity_recovery_controls",
        "tests": 2,
        "fixtures": (("crates/compiler/tests/fixtures/utf16-identity-recovery-controls.json", 65, "id"),
                     ("crates/compiler/tests/fixtures/utf16-noemit-command-controls.json", 14, "id")),
        "observers": ("scripts/observe-utf16-identity-recovery-controls.mjs",
                      "scripts/observe-utf16-noemit-command-controls.mjs"),
    },
    "utf16-review-fix": {
        "target": "h2_8a_utf16_review_fix_controls",
        "tests": 1,
        "fixtures": (("crates/compiler/tests/fixtures/utf16-review-fix-controls.json", 25, "id"),),
        "observers": ("scripts/observe-utf16-review-fix-controls.mjs",),
    },
    "utf16-tagged-template": {
        "target": "h2_8a_utf16_tagged_template_controls",
        "tests": 1,
        "fixtures": (("crates/compiler/tests/fixtures/utf16-tagged-template-controls.json", 16, "id"),),
        "observers": ("scripts/observe-utf16-tagged-template-controls.mjs",),
        "inputs": ("crates/compiler/tests/fixtures/utf16-tagged-template-review-v1.json",
                   "crates/compiler/tests/fixtures/utf16-tagged-template-review-v2.json"),
    },
    "utf16-literal-witnesses": {
        "target": "h2_5h_utf16_literal_witnesses",
        "test": "utf16_literal_witnesses_match_complete_typescript_observations",
        "tests": 1,
        "filtered_tests": 9,
        "fixtures": tuple((f"crates/compiler/tests/fixtures/utf16-literals-{group}.json", count, "case_id")
                          for group, count in (("string-literals", 36), ("template-literals", 26), ("bundle-prologues", 2))),
        "observers": tuple(("scripts/observe-utf16-literal-witnesses.mjs", group)
                           for group in ("string-literals", "template-literals", "bundle-prologues")),
        "inputs": tuple(f"crates/compiler/tests/fixtures/utf16-literals-{group}-inputs.json"
                        for group in ("string-literals", "template-literals", "bundle-prologues")),
    },
    "utf16-original-commands": {
        "target": "h2_5h_utf16_original_rows_complete",
        "tests": 1,
        "fixtures": (("crates/compiler/tests/fixtures/utf16-original-rows-complete.json", 4, "case_id"),),
        "observers": ("scripts/observe-utf16-original-rows-complete.mjs",),
    },
}
# A41-BINDING (C02): 768 complete commands selected like the SUPER sets
# (`TSC_RS_DECORATOR_BINDING_CASE_SET`), observed by
# `scripts/observe-decorator-bindings.mjs pipeline` (its `--check` runs
# before every replay, like the direct observers). The known-native fixture
# freezes the remaining owner-classified native divergences; the comparator
# counts them as `known`, never exact, and a `--all` run must report exactly
# that many.
BINDING = {
    "decorator-binding-pipeline": {
        "target": "decorator_binding_pipeline_contract",
        "test": "decorator_binding_forms_match_complete_typescript_observations",
        "env": "TSC_RS_DECORATOR_BINDING_CASE_SET",
        "inputs": "crates/compiler/tests/fixtures/decorator-binding-inputs.json",
        "cases": 768,
        "upstream_exceptions": ("decorator-binding/computed/esnext/set/static-accessor-decorated",),
        "observation": "crates/compiler/tests/fixtures/decorator-binding.json.zst",
        "known": "crates/compiler/tests/fixtures/decorator-binding-known-native.json",
        "observers": (("scripts/observe-decorator-bindings.mjs", "pipeline"),),
    },
}
SUITES = (*SUPER, "retained", "direct", "printer", "bundle-sinks", "declaration-map-cli",
          *EMITTER_DIRECT, *COMPILER_DIRECT, *BINDING, "resolution-cache", *foundation_witnesses.SUITES,
          *emitter_final_witnesses.SUITES)
RESOLUTION_INPUTS = {
    "crates/program/tests/resolution_cache_contract.rs",
    "crates/program/tests/fixtures/resolution_cache/manifest.v1.json",
    "crates/program/tests/fixtures/resolution_cache/expected.v1.json",
    "scripts/observe-resolution-cache.mjs",
}
RESOLUTION_OBSERVER = [
    "node", "scripts/observe-resolution-cache.mjs", "--manifest",
    "crates/program/tests/fixtures/resolution_cache/manifest.v1.json", "--check",
    "crates/program/tests/fixtures/resolution_cache/expected.v1.json",
]
RETAINED_FIXTURES = (
    "retained-accessor-owners", "class-helper-accessor-producers",
    "class-field-alias-map-positions", "decorator-receiver-context",
    "retained-lexical-environments", "retained-constructor-references",
    "retained-lexical-edges", "retained-comma-factory",
)


def read_cases(file):
    return json.loads(file.read_text())["cases"]


def case_ids(suite):
    if suite in emitter_final_witnesses.SUITES:
        return emitter_final_witnesses.case_ids(suite)
    if suite in foundation_witnesses.SUITES:
        return foundation_witnesses.test_names(suite)
    if suite == "resolution-cache":
        manifest = json.loads((ROOT / "crates/program/tests/fixtures/resolution_cache/manifest.v1.json").read_text())
        families = manifest["families"]
        ids = [family["id"] for family in families]
        generations = [generation for family in families for generation in family["generations"]]
        if (len(ids) != 26 or len(set(ids)) != 26 or any(not item.strip() for item in ids)
                or len(generations) + len(families) != 112
                or sum(len(family["requests"]) * (1 + len(family["generations"])) for family in families) != 197):
            raise ValueError("resolution-cache: changed family/generation/request membership")
        return ids
    if suite in EMITTER_DIRECT or suite in COMPILER_DIRECT:
        spec = EMITTER_DIRECT[suite] if suite in EMITTER_DIRECT else COMPILER_DIRECT[suite]
        ids = []
        sections = [(file, "cases", expected, key) for file, expected, key in spec["fixtures"]]
        sections.extend(spec.get("fixture_sections", ()))
        for file, section, expected, key in sections:
            rows = (read_cases(ROOT / file) if section == "cases"
                    else json.loads((ROOT / file).read_text())[section])
            local_ids = [row[key] for row in rows]
            if (len(rows) != expected or not rows
                    or any(not isinstance(item, str) or not item.strip() for item in local_ids)
                    or len(set(local_ids)) != len(local_ids)):
                raise ValueError(f"{suite}: empty, duplicate or changed fixture membership: {file}:{section}")
            prefix = Path(file).stem if section == "cases" else f"{Path(file).stem}/{section}"
            ids.extend(f"{prefix}/{item}" for item in local_ids)
        if len(set(ids)) != len(ids):
            raise ValueError(f"{suite}: duplicate fixture membership")
        return ids
    if suite == "declaration-map-cli":
        cases = [row for row in read_cases(ROOT / "ratchets/h2-7de-observations.v1.json")
                 if row["required_slices"] == ["H2.7e"]]
        if len(cases) != 8:
            raise ValueError("declaration-map-cli: changed eight-case membership")
    elif suite in SUPER:
        suffix, _ = SUPER[suite]
        # Input IDs include the two primary upstream exceptions. The Rust
        # comparator reports those separately and requires a native match.
        cases = read_cases(FIXTURES / f"decorator-super{suffix}-inputs.json")
    elif suite in BINDING:
        cases = read_cases(ROOT / BINDING[suite]["inputs"])
        if len(cases) != BINDING[suite]["cases"]:
            raise ValueError(f"{suite}: changed input manifest membership")
    elif suite == "retained":
        cases = []
        for name in RETAINED_FIXTURES:
            rows = read_cases(FIXTURES / f"{name}.json")
            if name == "class-field-alias-map-positions":
                rows = [row for row in rows if row["options"]["target"] == 9]
            cases.extend(rows)
    elif suite == "bundle-sinks":
        cases = read_cases(FIXTURES / "bundle-sinks.json")
    elif suite == "printer":
        cases = []
        for name in ("printer-failure-hooks", "printer-failure-review", "printer-comment-carry",
                     "printer-hook-hints"):
            cases.extend(read_cases(ROOT / f"crates/emitter/tests/fixtures/{name}.json"))
    else:
        cases = read_cases(ROOT / "crates/emitter/tests/fixtures/decorator-super-direct.json")
    ids = [case["case_id"] for case in cases]
    if not ids or len(set(ids)) != len(ids):
        raise ValueError(f"{suite}: empty or duplicate case IDs")
    return ids


def select_cases(ids, needles):
    if any(not needle.strip() or "," in needle or needle.strip() == "all" for needle in needles):
        raise ValueError("--case requires a nonempty substring (no comma or reserved 'all')")
    needles = [needle.strip() for needle in needles]
    selected = [case for case in ids if not needles or any(needle in case for needle in needles)]
    if not selected:
        raise ValueError("selection matched no cases")
    return selected


def invocation(suite, needles, environ=None):
    env = dict(os.environ if environ is None else environ)
    # Explicit CLI selection owns the entire selection, including --all.
    for _, key in SUPER.values():
        env.pop(key, None)
    for spec in BINDING.values():
        env.pop(spec["env"], None)
    env.pop("TSC_RS_RETAINED_ACCESSOR_CASE_FILTER", None)
    env.pop("TSC_RS_RETAINED_ACCESSOR_CASE_SET", None)
    env.pop("TSC_RS_LITERAL_UPDATE_REPORT_DIR", None)
    # Capture, report and known-native dump directories are never inherited:
    # a registered replay compares, it does not write evidence.
    for key in ("TSC_RS_H2_8A_CAPTURE_WRITES_DIR", "TSC_RS_DECORATOR_BINDING_REPORT_DIR",
                "TSC_RS_H2_8A_KNOWN_NATIVE_DUMP_DIR"):
        env.pop(key, None)
    env.setdefault("CARGO_BUILD_JOBS", "2")
    if suite in emitter_final_witnesses.SUITES:
        if needles:
            raise ValueError(f"{suite}: registered shard runs together; use --all")
        return [sys.executable, "scripts/emitter_final_witnesses.py", suite], emitter_final_witnesses.environment(suite, env)
    if suite in foundation_witnesses.SUITES:
        if needles:
            raise ValueError(f"{suite}: foundation target runs together; use --all")
        return foundation_witnesses.command([suite]), env
    if suite == "resolution-cache":
        if needles:
            raise ValueError("resolution-cache: complete trace and controls run together; use --all")
        return ["cargo", "test", "--manifest-path", "crates/program/Cargo.toml",
                "--lib", "--test", "resolution_cache_contract", "--", "--nocapture", "--test-threads=1"], env
    if suite in COMPILER_DIRECT:
        if needles:
            raise ValueError(f"{suite}: compiler target runs together; use --all")
        # Internal selectors can narrow cases without changing Cargo's test
        # count. Registered suites always own all their frozen inputs.
        env.pop("TSC_RS_UTF16_LITERAL_WITNESS_SET", None)
        env.pop("TSC_RS_UTF16_LITERAL_WITNESS_FILTER", None)
        env.pop("TSC_RS_H2_5H_PARAMETER_FILTER", None)
        env.pop("TSC_RS_H2_5H_PARAMETER_CAPTURE_DIR", None)
        env.pop("TSRS_MAP_OPTION_CAPTURE", None)
        for key in ("TSC_RS_DECL_COMMENT_FILTER", "TSC_RS_DECL_COMMENT_CAPTURE_DIR",
                    "TSC_RS_JSDOC_RETURN_FILTER", "TSC_RS_JSDOC_RETURN_CAPTURE_DIR",
                    "TSC_RS_DECLARATION_SPECIFIER_CAPTURE_DIR",
                    "TSC_RS_REQUIRE_REWRITE_FILTER", "TSC_RS_H2_8A_CAPTURE_WRITES_DIR",
                    "TSC_RS_BUNDLE_METADATA_T1_CASE_SET", "TSC_RS_BUNDLE_METADATA_T1_DUMP_DIR",
                    "TSC_RS_POST_T1_RESIDUALS_CASE_SET", "TSC_RS_POST_T1_RESIDUALS_DUMP_DIR"):
            env.pop(key, None)
        return compiler_direct_command([suite]), env
    if suite in EMITTER_DIRECT:
        if needles:
            raise ValueError(f"{suite}: small direct target runs together; use --all")
        return emitter_command([suite]), env
    if suite == "printer":
        if needles:
            raise ValueError("printer failure controls run together; use --all (142 small direct rows)")
        return ["cargo", "test", "--manifest-path", "crates/emitter/Cargo.toml",
                "--test", "printer_failure_contract", "--", "--nocapture", "--test-threads=1"], env
    if suite in SUPER:
        suffix, key = SUPER[suite]
        name = f"decorator_super{suffix.replace('-', '_')}_forms_match_complete_typescript_observations"
        target, test = "decorator_super_contract", f"h2_8a_decorator_super::{name}"
        env[key] = ",".join(needle.strip() for needle in needles) if needles else "all"
    elif suite in BINDING:
        spec = BINDING[suite]
        target, test = spec["target"], spec["test"]
        env[spec["env"]] = ",".join(needle.strip() for needle in needles) if needles else "all"
    elif suite == "bundle-sinks":
        if needles:
            raise ValueError("bundle sink controls run together; use --all (10 complete commands)")
        target, test = "h2_7d_bundle_sinks", "ordinary_bundle_sink_commands_match_complete_typescript_twice"
    elif suite == "declaration-map-cli":
        if needles:
            raise ValueError("declaration map CLI controls run together; use --all (8 CLI cases)")
        target, test = "h2_7e_original_corpus", "h2_7e_original_cli_matches_outputs_diagnostics_and_exit_twice"
    elif suite == "retained":
        target = "contracts"
        test = "h2_8a_retained_accessor_owners::retained_accessor_owners_match_complete_typescript_observations"
        env["TSC_RS_RETAINED_ACCESSOR_CASE_SET"] = "all"
        if needles:
            env["TSC_RS_RETAINED_ACCESSOR_CASE_FILTER"] = ",".join(needle.strip() for needle in needles)
    else:
        if needles:
            raise ValueError("direct controls run together; use --all")
        target, test = "decorator_super_direct_contract", "decorator_super_direct_controls_match_typescript"
    owner = "emitter" if suite == "direct" else "compiler"
    command = ["cargo", "test", "--manifest-path", f"crates/{owner}/Cargo.toml",
               "--test", target, test, "--", "--exact", "--nocapture", "--test-threads=1"]
    return command, env


def emitter_inputs(suite):
    spec = EMITTER_DIRECT[suite]
    return {f"crates/emitter/tests/{spec['target']}.rs",
            *(observer[1] for observer in direct_observers(EMITTER_DIRECT, [suite])),
            *(file for file, _, _ in spec["fixtures"]), *spec.get("inputs", ())}


def compiler_direct_inputs(suite):
    spec = COMPILER_DIRECT[suite]
    # The identity observer also reads utf16-literals-adjacent-probes-inputs.
    # That shared input keeps full replay via the planner's unknown-input rule.
    return {*spec.get("sources", (f"crates/compiler/tests/{spec['target']}.rs",)),
            *(observer[1] for observer in compiler_direct_observers([suite])),
            *(source for source, _, _ in spec.get("staged_inputs", ())),
            *(file for file, _, _, _ in spec.get("fixture_sections", ())),
            *(file for file, _, _ in spec["fixtures"]), *spec.get("inputs", ())}


def compiler_direct_observers(suites):
    return direct_observers(COMPILER_DIRECT, suites)


def binding_inputs(suite):
    spec = BINDING[suite]
    return {f"crates/compiler/tests/{spec['target']}.rs", spec["inputs"], spec["observation"],
            spec["known"], *(observer[1] for observer in direct_observers(BINDING, [suite]))}


def run_binding(suite, command, env, needles):
    """Check the frozen pipeline observation, replay, and require exact + known == selected."""
    known_rows = read_cases(ROOT / BINDING[suite]["known"])
    selected_ids = set(select_cases(case_ids(suite), needles)) - set(BINDING[suite]["upstream_exceptions"])
    expected_known = sum(row["case_id"] in selected_ids for row in known_rows)
    if not selected_ids:
        raise ValueError(f"{suite}: selection contains no complete command")
    started = time.monotonic()
    for observer in direct_observers(BINDING, [suite]):
        subprocess.run(list(observer), cwd=ROOT, env=env, check=True)
    oracle_seconds = time.monotonic() - started
    started = time.monotonic()
    result = subprocess.run(command, cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, check=False)
    print(result.stdout, end="", flush=True)
    result.check_returncode()
    summary = re.findall(r"decorator binding SUMMARY exact=(\d+) known=(\d+) failed=(\d+) selected=(\d+)", result.stdout)
    if len(summary) != 1 or "test result: ok. 1 passed; 0 failed; 0 ignored;" not in result.stdout:
        raise ValueError(f"{suite}: missing or zero-test comparator summary")
    exact, known, failed, selected = map(int, summary[0])
    if failed or exact + known != selected or selected == 0:
        raise ValueError(f"{suite}: {failed} failed, {exact} exact + {known} known of {selected}")
    if selected != len(selected_ids) or known != expected_known:
        raise ValueError(f"{suite}: replay membership differs from the requested complete commands")
    if not needles and known != len(known_rows):
        raise ValueError(f"{suite}: {known} known divergences replayed, {len(known_rows)} frozen")
    print(json.dumps({"suite": suite, "exact": exact, "known": known, "selected": selected,
                      "known_frozen": len(known_rows), "observer_seconds": round(oracle_seconds, 3),
                      "cargo_build_and_replay_seconds": round(time.monotonic() - started, 3)}), flush=True)


def direct_observers(catalog, suites):
    # An observer may take a group before --check. Deduplicate commands, not
    # paths: the three literal groups use the same script with distinct inputs.
    return list(dict.fromkeys(
        ("node", *((observer,) if isinstance(observer, str) else observer), "--check")
        for suite in suites for observer in catalog[suite]["observers"]))


def compiler_direct_command(suites):
    if not suites or len(set(suites)) != len(suites) or any(suite not in COMPILER_DIRECT for suite in suites):
        raise ValueError("invalid compiler direct selection")
    command = ["cargo", "test", "--manifest-path", "crates/compiler/Cargo.toml"]
    for suite in suites:
        command.extend(("--test", COMPILER_DIRECT[suite]["target"]))
    filtered = [suite for suite in suites if "test" in COMPILER_DIRECT[suite]]
    if filtered:
        if len(suites) != 1:
            raise ValueError("filtered compiler target requires its own invocation")
        names = COMPILER_DIRECT[filtered[0]]["test"]
        names = (names,) if isinstance(names, str) else names
        if (not names or any(not isinstance(name, str) or not name.strip() for name in names)
                or len(set(names)) != len(names)
                or len(names) != COMPILER_DIRECT[filtered[0]]["tests"]):
            raise ValueError("invalid exact compiler test names")
        # Cargo accepts one TESTNAME; libtest accepts additional exact names
        # after `--`. Their union runs once without replaying imported tests.
        return [*command, names[0], "--", "--exact", *names[1:], "--nocapture", "--test-threads=1"]
    return [*command, "--", "--nocapture", "--test-threads=1"]


@contextmanager
def staged_compiler_inputs(suites):
    """Restore hash-pinned historical inputs only for the frozen observer call."""
    created = []
    try:
        for suite in suites:
            for source, destination, expected in COMPILER_DIRECT[suite].get("staged_inputs", ()):
                data = (ROOT / source).read_bytes()
                if hashlib.sha256(data).hexdigest() != expected:
                    raise ValueError(f"{suite}: archived input hash drift: {source}")
                target = ROOT / destination
                target.parent.mkdir(parents=True, exist_ok=True)
                try:
                    output = target.open("xb")
                except FileExistsError:
                    if target.read_bytes() != data:
                        raise ValueError(f"{suite}: existing staged input differs: {destination}") from None
                else:
                    created.append(target)
                    with output:
                        output.write(data)
        yield
    finally:
        for target in reversed(created):
            target.unlink()


@contextmanager
def compiler_step(phase, command, suites):
    """Expose wall time and failures without attributing a shared build to one suite."""
    fields = {"witness_step": "compiler-direct", "phase": phase,
              "suites": list(suites), "argv": list(command)}
    print(json.dumps({**fields, "event": "start"}), flush=True)
    started = time.monotonic()
    try:
        yield
    except Exception as error:
        print(json.dumps({**fields, "event": "finish", "status": "failed",
                          "seconds": round(time.monotonic() - started, 3),
                          "error": type(error).__name__,
                          "exit_code": getattr(error, "returncode", None)}), flush=True)
        raise
    else:
        print(json.dumps({**fields, "event": "finish", "status": "passed",
                          "seconds": round(time.monotonic() - started, 3)}), flush=True)


def run_compiler_direct(suites):
    """Check selected frozen oracles and replay their complete standalone targets."""
    if not suites or len(set(suites)) != len(suites) or any(suite not in COMPILER_DIRECT for suite in suites):
        raise ValueError("invalid compiler direct selection")
    unfiltered = [suite for suite in suites if "test" not in COMPILER_DIRECT[suite]]
    batches = ([unfiltered] if unfiltered else []) + [[suite] for suite in suites if "test" in COMPILER_DIRECT[suite]]
    for suite in suites:
        print(f"{suite}: {len(case_ids(suite))} fixture rows (including any typed refusal controls)", flush=True)
    started = time.monotonic()
    with staged_compiler_inputs(suites):
        for command in compiler_direct_observers(suites):
            owners = [suite for suite in suites if command in compiler_direct_observers([suite])]
            with compiler_step("observer", command, owners):
                subprocess.run(list(command), cwd=ROOT, check=True)
    oracle_seconds = time.monotonic() - started
    _, env = invocation(suites[0], [])
    started = time.monotonic()
    tests_passed = 0
    for batch in batches:
        command = compiler_direct_command(batch)
        with compiler_step("cargo-build-and-replay", command, batch):
            result = subprocess.run(command, cwd=ROOT, env=env, text=True,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=False)
            print(result.stdout, end="", flush=True)
            result.check_returncode()
            counts = re.findall(r"test result: ok\. (\d+) passed; 0 failed; (\d+) ignored;.*? (\d+) filtered out;", result.stdout)
            expected = sorted((COMPILER_DIRECT[suite]["tests"], 0, COMPILER_DIRECT[suite].get("filtered_tests", 0)) for suite in batch)
            if sorted(tuple(map(int, row)) for row in counts) != expected:
                raise ValueError("compiler direct target omitted, ignored, filtered or selected zero tests")
            tests_passed += sum(int(row[0]) for row in counts)
    print(json.dumps({"compiler_direct": suites, "targets": len(suites),
                      "tests_passed": tests_passed,
                      "observer_seconds": round(oracle_seconds, 3),
                      "cargo_build_and_replay_seconds": round(time.monotonic() - started, 3)}), flush=True)


def emitter_command(suites):
    if not suites or len(set(suites)) != len(suites) or any(suite not in EMITTER_DIRECT for suite in suites):
        raise ValueError("invalid emitter direct selection")
    command = ["cargo", "test", "--manifest-path", "crates/emitter/Cargo.toml"]
    for suite in suites:
        command.extend(("--test", EMITTER_DIRECT[suite]["target"]))
    return [*command, "--", "--nocapture", "--test-threads=1"]


def run_emitter_direct(suites):
    """Check selected oracles, then build/replay only their targets in one Cargo call."""
    command = emitter_command(suites)
    for suite in suites:
        print(f"{suite}: {len(case_ids(suite))} fixture rows (each compared twice)", flush=True)
    started = time.monotonic()
    for observer in direct_observers(EMITTER_DIRECT, suites):
        subprocess.run(list(observer), cwd=ROOT, check=True)
    oracle_seconds = time.monotonic() - started
    _, env = invocation(suites[0], [])
    started = time.monotonic()
    result = subprocess.run(command, cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, check=False)
    print(result.stdout, end="", flush=True)
    result.check_returncode()
    counts = re.findall(r"test result: ok\. (\d+) passed; 0 failed; (\d+) ignored;.*? (\d+) filtered out;", result.stdout)
    if (len(counts) != len(suites)
            or any(int(passed) == 0 or int(ignored) or int(filtered) for passed, ignored, filtered in counts)):
        raise ValueError("emitter direct target omitted, ignored, filtered or selected zero tests")
    print(json.dumps({"emitter_direct": suites, "targets": len(suites),
                      "tests_passed": sum(int(row[0]) for row in counts),
                      "observer_seconds": round(oracle_seconds, 3),
                      "cargo_build_and_replay_seconds": round(time.monotonic() - started, 3)}), flush=True)


def run_resolution_cache(command, env):
    case_ids("resolution-cache")
    started = time.monotonic()
    subprocess.run(RESOLUTION_OBSERVER, cwd=ROOT, check=True)
    oracle_seconds = time.monotonic() - started
    started = time.monotonic()
    result = subprocess.run(command, cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, check=False)
    print(result.stdout, end="", flush=True)
    result.check_returncode()
    counts = re.findall(r"test result: ok\. (\d+) passed; 0 failed; (\d+) ignored;.*? (\d+) filtered out;", result.stdout)
    if sorted(tuple(map(int, row)) for row in counts) != [(11, 0, 0), (56, 0, 0)]:
        raise ValueError("resolution-cache: missing, ignored, filtered or changed target results")
    print(json.dumps({"resolution_cache": {"families": 26, "generations": 112, "requests": 197},
                      "contract_tests": 11, "program_unit_tests": 56,
                      "observer_seconds": round(oracle_seconds, 3),
                      "cargo_build_and_replay_seconds": round(time.monotonic() - started, 3)}), flush=True)


def run_declaration_map_cli(command, env):
    started = time.monotonic()
    result = subprocess.run(command, cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, check=False)
    print(result.stdout, end="", flush=True)
    result.check_returncode()
    if "test result: ok. 1 passed; 0 failed; 0 ignored;" not in result.stdout:
        raise ValueError("declaration-map-cli: missing or zero-test CLI comparison")
    print(json.dumps({"declaration_map_cli_cases": 8, "repetitions": 2,
                      "cargo_build_and_replay_seconds": round(time.monotonic() - started, 3)}), flush=True)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("suite", choices=SUITES)
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--case", action="append", default=[], metavar="ID_SUBSTRING",
                           help="repeat to select the union of matching case IDs")
    selection.add_argument("--all", action="store_true", help="explicit full replay (normally hosted)")
    parser.add_argument("--list", action="store_true", help="list matching input IDs without Cargo")
    parser.add_argument("--dry-run", action="store_true", help="print selection and command without running")
    args = parser.parse_args(argv)
    if not args.list and not (args.case or args.all):
        parser.error("choose --case or --all; use --list to inspect IDs")
    try:
        selected = select_cases(case_ids(args.suite), args.case)
        command, env = invocation(args.suite, args.case)
    except (ValueError, KeyError, OSError) as error:
        parser.error(str(error))
    unit = "test functions" if args.suite in foundation_witnesses.SUITES else "input cases"
    print(f"{args.suite}: selected {len(selected)} {unit}", flush=True)
    if args.suite == "primary":
        print("Primary upstream exceptions are reported separately by the comparator.", flush=True)
    if args.list:
        print("\n".join(selected))
        return 0
    assignments = {key: value for key, value in env.items()
                   if key.startswith("TSC_RS_") and (key.endswith("CASE_SET") or key.endswith("CASE_FILTER"))}
    print(shlex.join(["env", *[f"{key}={value}" for key, value in sorted(assignments.items())], *command]), flush=True)
    if args.dry_run:
        if args.suite in emitter_final_witnesses.SUITES:
            for command, _ in emitter_final_witnesses.commands(args.suite):
                print(shlex.join(command))
        if args.suite in foundation_witnesses.SUITES:
            spec = foundation_witnesses.SUITES[args.suite]
            if "oracle" in spec:
                print(shlex.join(["node", f"scripts/observe-{spec['oracle']}.mjs", "--check"]))
        if args.suite == "resolution-cache":
            print(shlex.join(RESOLUTION_OBSERVER))
        if args.suite in EMITTER_DIRECT:
            for command in direct_observers(EMITTER_DIRECT, [args.suite]):
                print(shlex.join(command))
        if args.suite in COMPILER_DIRECT:
            for command in compiler_direct_observers([args.suite]):
                print(shlex.join(command))
        if args.suite in BINDING:
            for command in direct_observers(BINDING, [args.suite]):
                print(shlex.join(command))
        return 0
    if args.suite in foundation_witnesses.SUITES:
        foundation_witnesses.run([args.suite], env)
        return 0
    if args.suite == "resolution-cache":
        run_resolution_cache(command, env)
        return 0
    if args.suite in COMPILER_DIRECT:
        run_compiler_direct([args.suite])
        return 0
    if args.suite in EMITTER_DIRECT:
        run_emitter_direct([args.suite])
        return 0
    if args.suite == "declaration-map-cli":
        run_declaration_map_cli(command, env)
        return 0
    if args.suite in BINDING:
        run_binding(args.suite, command, env, args.case)
        return 0
    return subprocess.run(command, cwd=ROOT, env=env, check=False).returncode


if __name__ == "__main__":
    sys.exit(main())
