//! Unchanged original output-directory intersections, freshly reobserved on TS6.
use std::path::Path;

#[allow(dead_code)] // The shared source also contains the separate D283 entry.
#[path = "integration/h2_7d_original_corpus_shared.rs"]
mod h2_7d_original_corpus_shared;

#[test]
fn original_output_directory_corpus_matches_production_commands() {
    let ids = h2_7d_original_corpus_shared::assert_output_directory_references(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
    );
    assert_eq!(ids.len(), 23);
}

#[test]
fn original_output_matrix_candidates_match_complete_production_commands() {
    let ids = h2_7d_original_corpus_shared::assert_output_matrix_candidates(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
    );
    assert_eq!(ids.len(), 769);
}

#[test]
fn original_export_assignment_annotations_match_complete_commands() {
    let ids = (1..=8)
        .map(|index| {
            format!(
                "typescript-6.0.3/compiler/checkJsdocTypeTagOnExportAssignment{index}.ts#default"
            )
        })
        .collect::<Vec<_>>();
    let names = ids.iter().map(String::as_str).collect::<Vec<_>>();
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &names,
    );
    assert_eq!(exact.len(), 8);
}

#[test]
fn original_require_alias_declarations_match_complete_commands() {
    let names = [
        "typescript-6.0.3/compiler/requireOfJsonFileWithDeclaration.ts#default",
        "typescript-6.0.3/conformance/declarationEmit/leaveOptionalParameterAsWritten.ts#default",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsClassExtendsVisibility.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsClassExtendsVisibility.ts#target%3Des5",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsCrossfileMerge.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsCrossfileMerge.ts#target%3Des5",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsExportAssignedVisibility.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsExportAssignedVisibility.ts#target%3Des5",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsExportedClassAliases.ts#default",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsExportForms.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsExportForms.ts#target%3Des5",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsJson.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsJson.ts#target%3Des5",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsPackageJson.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsPackageJson.ts#target%3Des5",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsReexportedCjsAlias.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsReexportedCjsAlias.ts#target%3Des5",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsTypeReferences.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsTypeReferences.ts#target%3Des5",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsTypeReferences3.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsTypeReferences3.ts#target%3Des5",
        "typescript-6.0.3/conformance/salsa/requireOfESWithPropertyAccess.ts#default",
    ];
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &names,
    );
    assert_eq!(exact.len(), 22);
}

#[test]
fn original_root_diagnostic_streams_match_complete_commands() {
    let names = [
        "typescript-6.0.3/compiler/pathMappingBasedModuleResolution_rootImport_aliasWithRoot_differentRootTypes.ts#default",
        "typescript-6.0.3/compiler/pathMappingBasedModuleResolution_rootImport_aliasWithRoot_multipleAliases.ts#default",
        "typescript-6.0.3/compiler/pathMappingBasedModuleResolution_rootImport_aliasWithRoot_realRootFile.ts#default",
        "typescript-6.0.3/compiler/pathMappingBasedModuleResolution_rootImport_aliasWithRoot.ts#default",
        "typescript-6.0.3/compiler/pathMappingBasedModuleResolution_rootImport_noAliasWithRoot_realRootFile.ts#default",
        "typescript-6.0.3/compiler/pathMappingBasedModuleResolution_rootImport_noAliasWithRoot.ts#default",
    ];
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &names,
    );
    assert_eq!(exact.len(), 6);
}

#[test]
fn original_javascript_imports_match_complete_commands() {
    let names = [
        "typescript-6.0.3/compiler/elidedJSImport1.ts#default",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsFunctionLikeClasses.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsFunctionLikeClasses.ts#target%3Des5",
        "typescript-6.0.3/conformance/node/allowJs/nodeModulesAllowJsSynchronousCallErrors.ts#module%3Dnode16",
        "typescript-6.0.3/conformance/node/allowJs/nodeModulesAllowJsSynchronousCallErrors.ts#module%3Dnode18",
        "typescript-6.0.3/conformance/node/allowJs/nodeModulesAllowJsSynchronousCallErrors.ts#module%3Dnode20",
        "typescript-6.0.3/conformance/node/allowJs/nodeModulesAllowJsSynchronousCallErrors.ts#module%3Dnodenext",
    ];
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &names,
    );
    assert_eq!(exact.len(), 7);
}

#[test]
fn original_local_alias_declarations_match_complete_commands() {
    let names = [
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsExportForms.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsExportForms.ts#target%3Des5",
    ];
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &names,
    );
    assert_eq!(exact.len(), 2);
}

#[test]
fn original_local_namespace_alias_matches_complete_command() {
    let names = [
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsConstsAsNamespacesWithReferences.ts#default",
    ];
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &names,
    );
    assert_eq!(exact.len(), 1);
}

#[test]
fn original_reexported_commonjs_aliases_match_complete_commands() {
    let names = [
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsReexportedCjsAlias.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsReexportedCjsAlias.ts#target%3Des5",
    ];
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &names,
    );
    assert_eq!(exact.len(), 2);
}

#[test]
fn original_static_class_transforms_match_complete_commands() {
    let names = [
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsClasses.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsClasses.ts#target%3Des5",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsComputedNames.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsComputedNames.ts#target%3Des5",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsDefaultsErr.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsDefaultsErr.ts#target%3Des5",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsExportAssignedClassInstance2.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsExportAssignedClassInstance2.ts#target%3Des5",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsExportAssignedClassInstance3.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsExportAssignedClassInstance3.ts#target%3Des5",
    ];
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &names,
    );
    assert_eq!(exact.len(), 10);
}

#[test]
fn original_export_specifier_names_match_complete_commands() {
    let names = [
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsDefaultsErr.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsDefaultsErr.ts#target%3Des5",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsExportSpecifierNonlocal.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsExportSpecifierNonlocal.ts#target%3Des5",
        "typescript-6.0.3/conformance/node/allowJs/nodeModulesAllowJsImportHelpersCollisions3.ts#module%3Dnode16%2Ctarget%3Des2015",
        "typescript-6.0.3/conformance/node/allowJs/nodeModulesAllowJsImportHelpersCollisions3.ts#module%3Dnode16%2Ctarget%3Des5",
        "typescript-6.0.3/conformance/node/allowJs/nodeModulesAllowJsImportHelpersCollisions3.ts#module%3Dnode18%2Ctarget%3Des2015",
        "typescript-6.0.3/conformance/node/allowJs/nodeModulesAllowJsImportHelpersCollisions3.ts#module%3Dnode18%2Ctarget%3Des5",
        "typescript-6.0.3/conformance/node/allowJs/nodeModulesAllowJsImportHelpersCollisions3.ts#module%3Dnode20%2Ctarget%3Des2015",
        "typescript-6.0.3/conformance/node/allowJs/nodeModulesAllowJsImportHelpersCollisions3.ts#module%3Dnode20%2Ctarget%3Des5",
        "typescript-6.0.3/conformance/node/allowJs/nodeModulesAllowJsImportHelpersCollisions3.ts#module%3Dnodenext%2Ctarget%3Des2015",
        "typescript-6.0.3/conformance/node/allowJs/nodeModulesAllowJsImportHelpersCollisions3.ts#module%3Dnodenext%2Ctarget%3Des5",
    ];
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &names,
    );
    assert_eq!(exact.len(), 12);
}

#[test]
fn original_synthetic_default_aliases_match_complete_commands() {
    let names = [
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsReexportAliasesEsModuleInterop.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsReexportAliasesEsModuleInterop.ts#target%3Des5",
    ];
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &names,
    );
    assert_eq!(exact.len(), 2);
}

#[test]
fn original_jsdoc_parentheses_guards_match_complete_commands() {
    let names = [
        "typescript-6.0.3/compiler/jsdocTypeCast.ts#default",
        "typescript-6.0.3/compiler/jsdocTypecastNoTypeNoCrash.ts#default",
        "typescript-6.0.3/conformance/jsdoc/checkJsdocSatisfiesTag15.ts#default",
    ];
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &names,
    );
    assert_eq!(exact.len(), 3);
}

#[test]
fn original_synthetic_namespace_exports_match_complete_commands() {
    let names = [
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsFunctionKeywordPropExhaustive.ts#target%3Des2015",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsFunctionKeywordPropExhaustive.ts#target%3Des5",
        "typescript-6.0.3/compiler/jsFileAlternativeUseOfOverloadTag.ts#default",
        "typescript-6.0.3/conformance/jsdoc/jsdocImplements_class.ts#default",
    ];
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
        &names,
    );
    assert_eq!(exact.len(), 4);
}
