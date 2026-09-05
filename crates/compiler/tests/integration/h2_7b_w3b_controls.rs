//! h2-7b-w3 lane B controls — frozen production-path observations for the JS
//! module/downlevel transform, declaration printer, and declaration transformer.

use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::Value;
use tsc_compiler::{
    EmitArtifactKind, EmitTextMetadata, EmitWriteMetadata, MemoryOutputSink, ProgramSession,
};
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_harness::upstream_suites::execution::{
    load_qualified_compiler_emit_with_option_floor, EmitOptionFloor,
};
use tsc_program::ProgramLoadLimits;

const QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-7b-qualification.v1.json";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn frozen_case(case_id: &str) -> Value {
    let artifact: Value = serde_json::from_slice(
        &std::fs::read(workspace_root().join(QUALIFICATION_RELATIVE_PATH))
            .expect("the frozen H2.7b qualification artifact"),
    )
    .expect("valid qualification JSON");
    artifact["cases"]
        .as_array()
        .expect("qualification cases")
        .iter()
        .find(|case| case["case_id"] == case_id)
        .cloned()
        .unwrap_or_else(|| panic!("{case_id} is not a frozen band row"))
}

fn decode(value: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(value.as_str().expect("base64 text"))
        .expect("valid base64")
}

fn prepared_band_row(case: &Value) -> tsc_program::PreparedProgram {
    assert_ne!(
        case["execution_route"], "project-mount",
        "W3-B controls contain no project rows"
    );
    let input = &case["input"];
    let mut files = input["files"]
        .as_array()
        .expect("case input files")
        .iter()
        .map(|file| {
            (
                PathBuf::from(file["path"].as_str().expect("file path")),
                decode(&file["utf8_base64"]),
            )
        })
        .collect::<Vec<_>>();
    if !input["virtual_config"].is_null() {
        let config = &input["virtual_config"];
        files.push((
            PathBuf::from(config["path"].as_str().expect("config path")),
            decode(&config["utf8_base64"]),
        ));
    }
    let roots = input["roots"]
        .as_array()
        .expect("case roots")
        .iter()
        .map(|root| PathBuf::from(root.as_str().expect("root path")))
        .collect::<Vec<_>>();
    let settings = input["settings"]
        .as_array()
        .expect("case settings")
        .iter()
        .map(|setting| {
            (
                setting["name"].as_str().expect("setting name").to_owned(),
                setting["value"].as_str().expect("setting value").to_owned(),
            )
        })
        .collect::<Vec<_>>();
    load_qualified_compiler_emit_with_option_floor(
        &workspace_root(),
        input["current_directory"]
            .as_str()
            .expect("case current directory"),
        &files,
        &roots,
        &settings,
        ProgramLoadLimits::new(128, 1_024, 32, 8 * 1_024 * 1_024, 64 * 1_024 * 1_024),
        EmitOptionFloor::DeclarationFamily,
    )
    .expect("the frozen band row loads through the declaration-family floor")
}

type DiagnosticTuple = (
    u32,
    String,
    Option<String>,
    Option<u32>,
    Option<u32>,
    String,
);

fn flatten_message_chain(chain: &MessageChain, indent: usize, output: &mut String) {
    if indent != 0 {
        output.push('\n');
        for _ in 0..indent {
            output.push_str("  ");
        }
    }
    output.push_str(&chain.text);
    for child in &chain.next {
        flatten_message_chain(child, indent + 1, output);
    }
}

fn actual_diagnostics(diagnostics: &[Diagnostic]) -> Vec<DiagnosticTuple> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            let mut message = String::new();
            flatten_message_chain(&diagnostic.message, 0, &mut message);
            (
                diagnostic.code(),
                format!("{:?}", diagnostic.category()),
                diagnostic.file_name.clone(),
                diagnostic.start,
                diagnostic.length,
                message,
            )
        })
        .collect()
}

fn expected_diagnostics(value: &Value) -> Vec<DiagnosticTuple> {
    value
        .as_array()
        .expect("frozen diagnostics")
        .iter()
        .map(|diagnostic| {
            (
                diagnostic["code"].as_u64().expect("diagnostic code") as u32,
                diagnostic["category"]
                    .as_str()
                    .expect("diagnostic category")
                    .to_owned(),
                diagnostic["file"].as_str().map(str::to_owned),
                diagnostic["start"].as_u64().map(|value| value as u32),
                diagnostic["length"].as_u64().map(|value| value as u32),
                diagnostic["message"]
                    .as_str()
                    .expect("diagnostic message")
                    .to_owned(),
            )
        })
        .collect()
}

fn artifact_kind(kind: EmitArtifactKind) -> &'static str {
    match kind {
        EmitArtifactKind::JavaScript => "javascript",
        EmitArtifactKind::JavaScriptMap => "javascript-map",
        EmitArtifactKind::Declaration => "declaration",
        EmitArtifactKind::DeclarationMap => "declaration-map",
        EmitArtifactKind::BuildInfo => "build-info",
    }
}

fn text_metadata(metadata: &EmitWriteMetadata) -> &EmitTextMetadata {
    match metadata {
        EmitWriteMetadata::Text(metadata) => metadata,
        EmitWriteMetadata::BuildInfo(_) => panic!("target row unexpectedly wrote build info"),
    }
}

/// Replays a row through the production declaration-family entry and compares
/// the full diagnostic tuples, emit result, and every write byte-for-byte.
fn assert_frozen_observation(case_id: &str) {
    let case = frozen_case(case_id);
    assert_eq!(case["disposition"], "admitted-for-execution", "{case_id}");
    let prepared = prepared_band_row(&case);
    let mut sink = MemoryOutputSink::new();
    let (outcome, reported) = ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .unwrap_or_else(|error| panic!("{case_id}: production emit completes: {error}"));
    let expected = &case["typescript_observation"];

    assert_eq!(
        actual_diagnostics(&reported),
        expected_diagnostics(&expected["reported_diagnostics"]),
        "{case_id}: exact ordered reported diagnostics"
    );
    assert_eq!(
        actual_diagnostics(outcome.diagnostics()),
        expected_diagnostics(&expected["emit_result"]["diagnostics"]),
        "{case_id}: exact ordered emit diagnostics"
    );
    assert_eq!(
        outcome.emit_skipped(),
        expected["emit_result"]["emit_skipped"]
            .as_bool()
            .expect("frozen emitSkipped"),
        "{case_id}: emitSkipped"
    );
    let actual_exit_code = if outcome.emit_skipped() {
        1
    } else if reported
        .iter()
        .any(|diagnostic| format!("{:?}", diagnostic.category()) == "Error")
    {
        2
    } else {
        0
    };
    assert_eq!(
        actual_exit_code,
        expected["exit_code"].as_u64().expect("frozen exit code"),
        "{case_id}: exit code"
    );

    let expected_writes = expected["writes"].as_array().expect("frozen writes");
    assert_eq!(
        sink.writes().len(),
        expected_writes.len(),
        "{case_id}: exact write count"
    );
    for (actual, expected) in sink.writes().iter().zip(expected_writes) {
        assert_eq!(
            actual.path(),
            Path::new(expected["path"].as_str().expect("frozen write path")),
            "{case_id}: exact output path"
        );
        assert_eq!(
            artifact_kind(actual.kind()),
            expected["kind"].as_str().expect("frozen write kind"),
            "{case_id}: exact write kind for {}",
            actual.path().display()
        );
        let expected_callback = decode(&expected["callback_utf8_base64"]);
        assert_eq!(
            actual.callback_bytes(),
            expected_callback,
            "{case_id}: exact callback bytes for {}",
            actual.path().display()
        );
        assert_eq!(
            actual.callback_bytes().len() as u64,
            expected["callback_utf8_bytes"]
                .as_u64()
                .expect("frozen callback length"),
            "{case_id}: callback length for {}",
            actual.path().display()
        );
        assert_eq!(
            actual.write_byte_order_mark(),
            expected["write_byte_order_mark"]
                .as_bool()
                .expect("frozen BOM flag"),
            "{case_id}: BOM flag for {}",
            actual.path().display()
        );
        let expected_materialized = decode(&expected["materialized_utf8_base64"]);
        assert_eq!(
            actual.materialized_bytes().as_ref(),
            expected_materialized,
            "{case_id}: exact materialized bytes for {}",
            actual.path().display()
        );
        assert_eq!(
            actual.materialized_bytes().len() as u64,
            expected["materialized_utf8_bytes"]
                .as_u64()
                .expect("frozen materialized length"),
            "{case_id}: materialized length for {}",
            actual.path().display()
        );
        let actual_sources = actual.source_files().map(|sources| {
            sources
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        });
        let expected_sources = expected["source_files"].as_array().map(|sources| {
            sources
                .iter()
                .map(|path| path.as_str().expect("frozen source path").to_owned())
                .collect::<Vec<_>>()
        });
        assert_eq!(
            actual_sources,
            expected_sources,
            "{case_id}: exact sourceFiles for {}",
            actual.path().display()
        );
        let metadata = text_metadata(
            actual
                .metadata()
                .expect("target write carries callback metadata"),
        );
        assert_eq!(
            actual_diagnostics(metadata.diagnostics()),
            expected_diagnostics(&expected["data_diagnostics"]),
            "{case_id}: exact write diagnostics for {}",
            actual.path().display()
        );
        assert_eq!(
            metadata
                .source_map_url_position()
                .map(|position| u64::from(position.value())),
            expected["data_source_map_url_pos"].as_u64(),
            "{case_id}: exact sourceMapUrlPos for {}",
            actual.path().display()
        );
    }
}

// The inherited lane battery filters on the W2 lane-module substring. Keep
// the W3 controls in a nested module with that registered name so the named
// light/full scripts execute both inherited and new controls.
mod h2_7b_w2b_controls {
    use super::*;

    macro_rules! frozen_control {
        ($name:ident, $case_id:literal) => {
            #[test]
            fn $name() {
                assert_frozen_observation(concat!("typescript-6.0.3/", $case_id));
            }
        };
    }

    frozen_control!(
        export_as_namespace_1_es2015_lowers_to_namespace_import,
        "conformance/es2020/modules/exportAsNamespace1.ts#module%3Des2015"
    );
    frozen_control!(
        export_as_namespace_2_es2015_lowers_to_namespace_import,
        "conformance/es2020/modules/exportAsNamespace2.ts#module%3Des2015"
    );
    frozen_control!(
        export_as_namespace_3_es2015_lowers_to_namespace_import,
        "conformance/es2020/modules/exportAsNamespace3.ts#module%3Des2015"
    );
    frozen_control!(
        export_as_namespace_4_es2015_lowers_default_namespace_export,
        "conformance/es2020/modules/exportAsNamespace4.ts#module%3Des2015"
    );
    frozen_control!(
    arbitrary_namespace_identifier_es6_lowers_string_export_name,
    "conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Des6"
);
    frozen_control!(
        import_assertion_2_es2015_preserves_attributes_on_lowered_import,
        "conformance/importAssertion/importAssertion2.ts#module%3Des2015"
    );
    frozen_control!(
        import_attributes_2_es2015_preserves_attributes_on_lowered_import,
        "conformance/importAttributes/importAttributes2.ts#module%3Des2015"
    );
    frozen_control!(
    arbitrary_namespace_identifier_amd_uses_element_access,
    "conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Damd"
);
    frozen_control!(
    arbitrary_namespace_identifier_commonjs_uses_element_access,
    "conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Dcommonjs"
);
    frozen_control!(
    arbitrary_namespace_identifier_node16_uses_element_access,
    "conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Dnode16"
);
    frozen_control!(
    arbitrary_namespace_identifier_node18_uses_element_access,
    "conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Dnode18"
);
    frozen_control!(
    arbitrary_namespace_identifier_node20_uses_element_access,
    "conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Dnode20"
);
    frozen_control!(
    arbitrary_namespace_identifier_nodenext_uses_element_access,
    "conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Dnodenext"
);
    frozen_control!(
    arbitrary_namespace_identifier_none_uses_element_access,
    "conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Dnone"
);
    frozen_control!(
    arbitrary_namespace_identifier_umd_uses_element_access,
    "conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Dumd"
);
    frozen_control!(
        export_as_namespace_1_amd_uses_written_namespace_parameter,
        "conformance/es2020/modules/exportAsNamespace1.ts#module%3Damd"
    );
    frozen_control!(
        export_as_namespace_2_amd_uses_written_namespace_parameter,
        "conformance/es2020/modules/exportAsNamespace2.ts#module%3Damd"
    );
    frozen_control!(
        export_as_namespace_3_amd_uses_written_namespace_parameter,
        "conformance/es2020/modules/exportAsNamespace3.ts#module%3Damd"
    );
    frozen_control!(
        export_as_namespace_1_system_publishes_namespace,
        "conformance/es2020/modules/exportAsNamespace1.ts#module%3Dsystem"
    );
    frozen_control!(
        export_as_namespace_2_system_publishes_namespace,
        "conformance/es2020/modules/exportAsNamespace2.ts#module%3Dsystem"
    );
    frozen_control!(
        export_as_namespace_3_system_publishes_namespace,
        "conformance/es2020/modules/exportAsNamespace3.ts#module%3Dsystem"
    );
    frozen_control!(
        export_as_namespace_4_system_publishes_default_namespace,
        "conformance/es2020/modules/exportAsNamespace4.ts#module%3Dsystem"
    );
    frozen_control!(
        arbitrary_namespace_identifier_system_publishes_namespace_without_ordinal_shift,
        "conformance/es2022/arbitraryModuleNamespaceIdentifiers/arbitraryModuleNamespaceIdentifiers_module.ts#module%3Dsystem"
    );
    frozen_control!(
        declaration_emit_nested_binding_pattern_rebuilds_multiline_constructors,
        "compiler/declarationEmitNestedBindingPattern.ts#default"
    );
    frozen_control!(
        shorthand_of_exported_entity_keeps_plain_property_key,
        "compiler/shorthandOfExportedEntity02_targetES5_CommonJS.ts#target%3Des5"
    );
    frozen_control!(
        instantiation_expressions_preserve_wrapper_precedence_and_omit_empty_arguments,
        "conformance/types/typeParameters/typeArgumentLists/instantiationExpressions.ts#default"
    );
    frozen_control!(
        comments_class_es5_suppresses_nested_constructor_tail,
        "compiler/commentsClass.ts#target%3Des5"
    );
    frozen_control!(
        comments_modules_es2015_suppresses_nested_module_tails,
        "compiler/commentsModules.ts#target%3Des2015"
    );
    frozen_control!(
        comments_modules_es5_suppresses_nested_module_tails,
        "compiler/commentsModules.ts#target%3Des5"
    );
    frozen_control!(
        declaration_emit_first_generic_function_type_argument_is_parenthesized,
        "compiler/declarationEmitFirstTypeArgumentGenericFunctionType.ts#default"
    );
    frozen_control!(
        null_property_name_aliases_future_reserved_expando_names,
        "conformance/declarationEmit/nullPropertyName.ts#default"
    );
    frozen_control!(
        using_declarations_emit_as_const_in_declarations,
        "conformance/statements/VariableStatements/usingDeclarations/usingDeclarationsDeclarationEmit.1.ts#default"
    );
    frozen_control!(
        await_using_declarations_emit_as_const_in_declarations,
        "conformance/statements/VariableStatements/usingDeclarations/usingDeclarationsDeclarationEmit.2.ts#default"
    );
    frozen_control!(
        declaration_emit_mapped_type_distributivity_drains_transitive_late_aliases,
        "compiler/declarationEmitMappedTypeDistributivityPreservesConstraints.ts#default"
    );
    frozen_control!(
        alias_inaccessible_module_drains_late_aliases,
        "compiler/aliasInaccessibleModule2.ts#default"
    );
    frozen_control!(
        contextual_parameter_base_expression_drains_late_aliases,
        "compiler/classReferencedInContextualParameterWithinItsOwnBaseExpression.ts#default"
    );
    frozen_control!(
        computed_class_member_name_drains_late_aliases,
        "compiler/declarationEmitClassMemberWithComputedPropertyName.ts#default"
    );
    frozen_control!(
        enum_reference_via_import_equals_drains_late_aliases,
        "compiler/declarationEmitEnumReferenceViaImportEquals.ts#default"
    );
    frozen_control!(
        expression_in_extends_drains_late_aliases,
        "compiler/declarationEmitExpressionInExtends6.ts#default"
    );
    frozen_control!(
        default_export_extending_expression_drains_late_aliases,
        "compiler/declarationEmitForDefaultExportClassExtendingExpression01.ts#default"
    );
    frozen_control!(
        mapped_type_constraint_drains_late_aliases,
        "compiler/declarationEmitMappedTypePreservesTypeParameterConstraint.ts#default"
    );
    frozen_control!(
        resolve_types_if_not_reusable_drains_late_aliases,
        "compiler/declarationEmitResolveTypesIfNotReusable.ts#default"
    );
    frozen_control!(
        expando_function_symbol_property_drains_late_aliases,
        "compiler/expandoFunctionSymbolProperty.ts#default"
    );
    frozen_control!(
        top_level_internal_reference_drains_late_aliases,
        "compiler/privacyTopLevelInternalReferenceImportWithoutExport.ts#default"
    );
    frozen_control!(
        recursive_mapped_types_drains_late_aliases,
        "conformance/types/mapped/recursiveMappedTypes.ts#default"
    );
    frozen_control!(
        declaration_emit_retains_jsdoc_on_destructuring_leaf_assignment,
        "compiler/declarationEmitRetainsJsdocyComments.ts#default"
    );
}
