//! Typed validation of relationships between effective compiler options.
//!
//! The validator owns semantic option relationships, while callers own the
//! diagnostic location projection appropriate to their input surface. A
//! config parser can therefore attach a violation to retained JSON syntax and
//! a programmatic `createProgram` caller can report the same violation without
//! fabricating a source location.

use tsc_diagnostics::{gen, sort_and_dedupe_diagnostics, Diagnostic, MessageChain};
use tsc_syntax::{is_entity_name_text, is_identifier_text_for_target};
use tsc_types::CompilerOptions;

use crate::prepared::{
    PathMapping, PathsOptionValidationPlan, PathsOptionViolation, PathsOptionViolationKind,
    ProgramOptions,
};

/// How TypeScript locates an option diagnostic on an option key or its
/// converted value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerOptionValidationLocation {
    Name,
    Value,
}

/// A failed relationship in the effective [`CompilerOptions`] snapshot.
///
/// Variants deliberately describe the failed invariant rather than a
/// diagnostic code or fixture. This keeps validation reusable by config,
/// command-line, and programmatic compiler entry points.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerOptionViolation {
    StrictPropertyInitializationRequiresStrictNullChecks,
    ExactOptionalPropertyTypesRequiresStrictNullChecks,
    OutFileConflictsWithIsolation {
        verbatim: bool,
    },
    CompositeRequiresDeclaration,
    CompositeRequiresIncremental,
    IsolatedModulesRequiresModuleOrEs2015,
    PreserveConstEnumsRequiredByIsolation {
        verbatim: bool,
    },
    CheckJsRequiresAllowJs,
    DecoratorMetadataRequiresExperimentalDecorators,
    IsolatedDeclarationsConflictsWithAllowJs,
    IsolatedDeclarationsRequiresDeclaration,
    EmitDeclarationOnlyRequiresDeclaration,
    DeclarationDirectoryRequiresDeclaration,
    DeclarationDirectoryConflictsWithOutFile,
    DeclarationMapRequiresDeclaration,
    ResolveJsonModuleConflictsWithClassicResolution,
    ResolveJsonModuleConflictsWithModule,
    OutFileRequiresAmdOrSystemModule,
    ReactNamespaceConflictsWithJsxFactory,
    JsxFactoryConflictsWithAutomaticRuntime {
        jsx: &'static str,
    },
    InvalidJsxFactory {
        value: String,
    },
    InvalidReactNamespace {
        value: String,
    },
    JsxFragmentFactoryRequiresJsxFactory,
    JsxFragmentFactoryConflictsWithAutomaticRuntime {
        jsx: &'static str,
    },
    InvalidJsxFragmentFactory {
        value: String,
    },
    ReactNamespaceConflictsWithAutomaticRuntime {
        jsx: &'static str,
    },
    JsxImportSourceConflictsWithClassicRuntime,
    InlineSourcesRequiresSourceMap,
    SourceRootRequiresSourceMap,
    MapRootConflictsWithInlineSourceMap,
    SourceMapConflictsWithInlineSourceMap,
    MapRootRequiresSourceMapOrDeclarationMap,
    VerbatimModuleRequiresSupportedModule,
    AllowImportingTsExtensionsRequiresEmitMode,
    PackageJsonExportsRequiresModernResolution,
    PackageJsonImportsRequiresModernResolution,
    CustomConditionsRequiresModernResolution,
    BundlerRequiresEsModule,
    NodeModuleRequiresNodeResolution {
        module: &'static str,
        resolution: &'static str,
    },
    NodeResolutionRequiresNodeModule {
        resolution: &'static str,
    },
}

impl CompilerOptionViolation {
    /// Option spellings inspected by `createDiagnosticForOption`. When more
    /// than one is present in retained config syntax, TypeScript reports the
    /// same relationship at every matching property in source order.
    pub const fn option_names(&self) -> &'static [&'static str] {
        match self {
            Self::VerbatimModuleRequiresSupportedModule => &["verbatimModuleSyntax"],
            Self::AllowImportingTsExtensionsRequiresEmitMode => &["allowImportingTsExtensions"],
            Self::PackageJsonExportsRequiresModernResolution => &["resolvePackageJsonExports"],
            Self::PackageJsonImportsRequiresModernResolution => &["resolvePackageJsonImports"],
            Self::CustomConditionsRequiresModernResolution => &["customConditions"],
            Self::BundlerRequiresEsModule | Self::NodeModuleRequiresNodeResolution { .. } => {
                &["moduleResolution"]
            }
            Self::NodeResolutionRequiresNodeModule { .. } => &["module"],
            Self::OutFileConflictsWithIsolation { verbatim: true } => {
                &["outFile", "verbatimModuleSyntax"]
            }
            Self::OutFileConflictsWithIsolation { verbatim: false } => {
                &["outFile", "isolatedModules"]
            }
            // verifyCompilerOptions deliberately locates the incremental
            // violation on declaration as well (_tsc.js:124783).
            Self::CompositeRequiresDeclaration | Self::CompositeRequiresIncremental => {
                &["declaration"]
            }
            Self::IsolatedModulesRequiresModuleOrEs2015 => &["isolatedModules", "target"],
            Self::PreserveConstEnumsRequiredByIsolation { verbatim: true } => {
                &["verbatimModuleSyntax", "preserveConstEnums"]
            }
            Self::PreserveConstEnumsRequiredByIsolation { verbatim: false } => {
                &["isolatedModules", "preserveConstEnums"]
            }
            Self::CheckJsRequiresAllowJs => &["checkJs", "allowJs"],
            Self::DecoratorMetadataRequiresExperimentalDecorators => {
                &["emitDecoratorMetadata", "experimentalDecorators"]
            }
            Self::StrictPropertyInitializationRequiresStrictNullChecks => {
                &["strictPropertyInitialization", "strictNullChecks"]
            }
            Self::ExactOptionalPropertyTypesRequiresStrictNullChecks => {
                &["exactOptionalPropertyTypes", "strictNullChecks"]
            }
            Self::IsolatedDeclarationsConflictsWithAllowJs => &["allowJs", "isolatedDeclarations"],
            Self::IsolatedDeclarationsRequiresDeclaration => {
                &["isolatedDeclarations", "declaration"]
            }
            Self::EmitDeclarationOnlyRequiresDeclaration => &["emitDeclarationOnly", "declaration"],
            Self::DeclarationDirectoryRequiresDeclaration => &["declarationDir", "declaration"],
            Self::DeclarationDirectoryConflictsWithOutFile => &["declarationDir", "outFile"],
            Self::DeclarationMapRequiresDeclaration => &["declarationMap", "declaration"],
            Self::ResolveJsonModuleConflictsWithClassicResolution => &["resolveJsonModule"],
            Self::ResolveJsonModuleConflictsWithModule => &["resolveJsonModule", "module"],
            Self::OutFileRequiresAmdOrSystemModule => &["outFile", "module"],
            Self::ReactNamespaceConflictsWithJsxFactory => &["reactNamespace", "jsxFactory"],
            Self::JsxFactoryConflictsWithAutomaticRuntime { .. }
            | Self::InvalidJsxFactory { .. } => &["jsxFactory"],
            Self::InvalidReactNamespace { .. }
            | Self::ReactNamespaceConflictsWithAutomaticRuntime { .. } => &["reactNamespace"],
            Self::JsxFragmentFactoryRequiresJsxFactory => &["jsxFragmentFactory", "jsxFactory"],
            Self::JsxFragmentFactoryConflictsWithAutomaticRuntime { .. }
            | Self::InvalidJsxFragmentFactory { .. } => &["jsxFragmentFactory"],
            Self::JsxImportSourceConflictsWithClassicRuntime => &["jsxImportSource"],
            Self::InlineSourcesRequiresSourceMap => &["inlineSources"],
            Self::SourceRootRequiresSourceMap => &["sourceRoot"],
            Self::MapRootConflictsWithInlineSourceMap => &["mapRoot", "inlineSourceMap"],
            Self::SourceMapConflictsWithInlineSourceMap => &["sourceMap", "inlineSourceMap"],
            Self::MapRootRequiresSourceMapOrDeclarationMap => &["mapRoot", "sourceMap"],
        }
    }

    pub const fn location(&self) -> CompilerOptionValidationLocation {
        match self {
            Self::AllowImportingTsExtensionsRequiresEmitMode
            | Self::BundlerRequiresEsModule
            | Self::NodeModuleRequiresNodeResolution { .. }
            | Self::NodeResolutionRequiresNodeModule { .. }
            | Self::InvalidJsxFactory { .. }
            | Self::InvalidReactNamespace { .. }
            | Self::InvalidJsxFragmentFactory { .. } => CompilerOptionValidationLocation::Value,
            _ => CompilerOptionValidationLocation::Name,
        }
    }

    pub fn message(&self) -> MessageChain {
        match self {
            Self::VerbatimModuleRequiresSupportedModule => MessageChain::new(
                &gen::Option_verbatimModuleSyntax_cannot_be_used_when_module_is_set_to_UMD_AMD_or_System, &[],
            ),
            Self::AllowImportingTsExtensionsRequiresEmitMode => MessageChain::new(
                &gen::Option_allowImportingTsExtensions_can_only_be_used_when_one_of_noEmit_emitDeclarationOnly_or_rewriteRelativeImportExtensions_is_set, &[],
            ),
            Self::PackageJsonExportsRequiresModernResolution | Self::PackageJsonImportsRequiresModernResolution | Self::CustomConditionsRequiresModernResolution => MessageChain::new(
                &gen::Option_0_can_only_be_used_when_moduleResolution_is_set_to_node16_nodenext_or_bundler,
                &[self.option_names()[0].to_owned()],
            ),
            Self::BundlerRequiresEsModule => MessageChain::new(
                &gen::Option_0_can_only_be_used_when_module_is_set_to_preserve_commonjs_or_es2015_or_later,
                &["bundler".to_owned()],
            ),
            Self::NodeModuleRequiresNodeResolution { module, resolution } => MessageChain::new(
                &gen::Option_moduleResolution_must_be_set_to_0_or_left_unspecified_when_option_module_is_set_to_1,
                &[(*resolution).to_owned(), (*module).to_owned()],
            ),
            Self::NodeResolutionRequiresNodeModule { resolution } => MessageChain::new(
                &gen::Option_module_must_be_set_to_0_when_option_moduleResolution_is_set_to_1,
                &[(*resolution).to_owned(), (*resolution).to_owned()],
            ),
            Self::OutFileConflictsWithIsolation { verbatim } => MessageChain::new(
                &gen::Option_0_cannot_be_specified_with_option_1,
                &["outFile".to_owned(), if *verbatim { "verbatimModuleSyntax" } else { "isolatedModules" }.to_owned()],
            ),
            Self::CompositeRequiresDeclaration => MessageChain::new(&gen::Composite_projects_may_not_disable_declaration_emit, &[]),
            Self::CompositeRequiresIncremental => MessageChain::new(&gen::Composite_projects_may_not_disable_incremental_compilation, &[]),
            Self::IsolatedModulesRequiresModuleOrEs2015 => MessageChain::new(
                &gen::Option_isolatedModules_can_only_be_used_when_either_option_module_is_provided_or_option_target_is_ES2015_or_higher, &[],
            ),
            Self::PreserveConstEnumsRequiredByIsolation { verbatim } => MessageChain::new(
                &gen::Option_preserveConstEnums_cannot_be_disabled_when_0_is_enabled,
                &[if *verbatim { "verbatimModuleSyntax" } else { "isolatedModules" }.to_owned()],
            ),
            Self::CheckJsRequiresAllowJs => MessageChain::new(
                &gen::Option_0_cannot_be_specified_without_specifying_option_1,
                &["checkJs".to_owned(), "allowJs".to_owned()],
            ),
            Self::DecoratorMetadataRequiresExperimentalDecorators => MessageChain::new(
                &gen::Option_0_cannot_be_specified_without_specifying_option_1,
                &["emitDecoratorMetadata".to_owned(), "experimentalDecorators".to_owned()],
            ),
            Self::StrictPropertyInitializationRequiresStrictNullChecks => MessageChain::new(
                &gen::Option_0_cannot_be_specified_without_specifying_option_1,
                &[
                    "strictPropertyInitialization".to_owned(),
                    "strictNullChecks".to_owned(),
                ],
            ),
            Self::ExactOptionalPropertyTypesRequiresStrictNullChecks => MessageChain::new(
                &gen::Option_0_cannot_be_specified_without_specifying_option_1,
                &[
                    "exactOptionalPropertyTypes".to_owned(),
                    "strictNullChecks".to_owned(),
                ],
            ),
            Self::IsolatedDeclarationsConflictsWithAllowJs => MessageChain::new(
                &gen::Option_0_cannot_be_specified_with_option_1,
                &["allowJs".to_owned(), "isolatedDeclarations".to_owned()],
            ),
            Self::IsolatedDeclarationsRequiresDeclaration => MessageChain::new(
                &gen::Option_0_cannot_be_specified_without_specifying_option_1_or_option_2,
                &[
                    "isolatedDeclarations".to_owned(),
                    "declaration".to_owned(),
                    "composite".to_owned(),
                ],
            ),
            Self::EmitDeclarationOnlyRequiresDeclaration => MessageChain::new(
                &gen::Option_0_cannot_be_specified_without_specifying_option_1_or_option_2,
                &[
                    "emitDeclarationOnly".to_owned(),
                    "declaration".to_owned(),
                    "composite".to_owned(),
                ],
            ),
            Self::DeclarationDirectoryRequiresDeclaration => MessageChain::new(
                &gen::Option_0_cannot_be_specified_without_specifying_option_1_or_option_2,
                &["declarationDir".to_owned(), "declaration".to_owned(), "composite".to_owned()],
            ),
            Self::DeclarationDirectoryConflictsWithOutFile => MessageChain::new(
                &gen::Option_0_cannot_be_specified_with_option_1,
                &["declarationDir".to_owned(), "outFile".to_owned()],
            ),
            Self::DeclarationMapRequiresDeclaration => MessageChain::new(
                &gen::Option_0_cannot_be_specified_without_specifying_option_1_or_option_2,
                &["declarationMap".to_owned(), "declaration".to_owned(), "composite".to_owned()],
            ),
            Self::OutFileRequiresAmdOrSystemModule => MessageChain::new(
                &gen::Only_amd_and_system_modules_are_supported_alongside_0,
                &["outFile".to_owned()],
            ),
            Self::ResolveJsonModuleConflictsWithClassicResolution => MessageChain::new(
                &gen::Option_resolveJsonModule_cannot_be_specified_when_moduleResolution_is_set_to_classic,
                &[],
            ),
            Self::ResolveJsonModuleConflictsWithModule => MessageChain::new(
                &gen::Option_resolveJsonModule_cannot_be_specified_when_module_is_set_to_none_system_or_umd,
                &[],
            ),
            Self::ReactNamespaceConflictsWithJsxFactory => MessageChain::new(
                &gen::Option_0_cannot_be_specified_with_option_1,
                &["reactNamespace".to_owned(), "jsxFactory".to_owned()],
            ),
            Self::JsxFactoryConflictsWithAutomaticRuntime { jsx } => MessageChain::new(
                &gen::Option_0_cannot_be_specified_when_option_jsx_is_1,
                &["jsxFactory".to_owned(), (*jsx).to_owned()],
            ),
            Self::InvalidJsxFactory { value } => MessageChain::new(
                &gen::Invalid_value_for_jsxFactory_0_is_not_a_valid_identifier_or_qualified_name,
                std::slice::from_ref(value),
            ),
            Self::InvalidReactNamespace { value } => MessageChain::new(
                &gen::Invalid_value_for_reactNamespace_0_is_not_a_valid_identifier,
                std::slice::from_ref(value),
            ),
            Self::JsxFragmentFactoryRequiresJsxFactory => MessageChain::new(
                &gen::Option_0_cannot_be_specified_without_specifying_option_1,
                &[
                    "jsxFragmentFactory".to_owned(),
                    "jsxFactory".to_owned(),
                ],
            ),
            Self::JsxFragmentFactoryConflictsWithAutomaticRuntime { jsx } => MessageChain::new(
                &gen::Option_0_cannot_be_specified_when_option_jsx_is_1,
                &["jsxFragmentFactory".to_owned(), (*jsx).to_owned()],
            ),
            Self::InvalidJsxFragmentFactory { value } => MessageChain::new(
                &gen::Invalid_value_for_jsxFragmentFactory_0_is_not_a_valid_identifier_or_qualified_name,
                std::slice::from_ref(value),
            ),
            Self::ReactNamespaceConflictsWithAutomaticRuntime { jsx } => MessageChain::new(
                &gen::Option_0_cannot_be_specified_when_option_jsx_is_1,
                &["reactNamespace".to_owned(), (*jsx).to_owned()],
            ),
            Self::JsxImportSourceConflictsWithClassicRuntime => MessageChain::new(
                &gen::Option_0_cannot_be_specified_when_option_jsx_is_1,
                &["jsxImportSource".to_owned(), "react".to_owned()],
            ),
            Self::InlineSourcesRequiresSourceMap => MessageChain::new(
                &gen::Option_0_can_only_be_used_when_either_option_inlineSourceMap_or_option_sourceMap_is_provided,
                &["inlineSources".to_owned()],
            ),
            Self::SourceRootRequiresSourceMap => MessageChain::new(
                &gen::Option_0_can_only_be_used_when_either_option_inlineSourceMap_or_option_sourceMap_is_provided,
                &["sourceRoot".to_owned()],
            ),
            Self::MapRootConflictsWithInlineSourceMap => MessageChain::new(
                &gen::Option_0_cannot_be_specified_with_option_1,
                &["mapRoot".to_owned(), "inlineSourceMap".to_owned()],
            ),
            Self::SourceMapConflictsWithInlineSourceMap => MessageChain::new(
                &gen::Option_0_cannot_be_specified_with_option_1,
                &["sourceMap".to_owned(), "inlineSourceMap".to_owned()],
            ),
            Self::MapRootRequiresSourceMapOrDeclarationMap => MessageChain::new(
                &gen::Option_0_cannot_be_specified_without_specifying_option_1_or_option_2,
                &[
                    "mapRoot".to_owned(),
                    "sourceMap".to_owned(),
                    "declarationMap".to_owned(),
                ],
            ),
        }
    }
}

/// Validate the option relationships in their TypeScript 6.0.3
/// `verifyCompilerOptions` order.
///
/// tsc-port: verifyCompilerOptions @6.0.3 (strict/isolated block)
/// tsc-hash: 2553c0a4e50ebd81142e6a0ef445ce7731ef7a91741f2c604a460c839d033ab9
/// tsc-span: _tsc.js:124751-124768
/// tsc-port: verifyCompilerOptions @6.0.3 (JSX block)
/// tsc-hash: 1972328cb915ef83d963c4d5f7f8abf148aa8651d6188a0dbdeee377490f69ad
/// tsc-span: _tsc.js:124954-124986
/// tsc-port: verifyCompilerOptions @6.0.3 (inline source-map conflicts)
/// tsc-hash: 73b19528c7f4dd48641c7b22cc9849d6ac109ee09517fde4e029e897166e8c62
/// tsc-span: _tsc.js:124770-124777
/// tsc-port: verifyCompilerOptions @6.0.3 (source-root/map-root prerequisites)
/// tsc-hash: 669a5b208c175af80f80a6a49fbaa43b7afcbe4d3cde9777804a682934d39513
/// tsc-span: _tsc.js:124855-124865
/// tsc-port: verifyCompilerOptions @6.0.3 (declaration-only prerequisite)
/// tsc-hash: 502558caa3484d85116380b71c5ffce3864d14b552e9368298016a238d9aee9f
/// tsc-span: _tsc.js:124946-124953
/// tsc-port: verifyCompilerOptions @6.0.3 (declaration map prerequisite)
/// tsc-hash: e1006c5a6a1d61f895b10092c7e7cff24f64d570cca0d9a4d13f30f4ba0c8c46
/// tsc-span: _tsc.js:124874-124876
pub fn validate_compiler_options(options: &CompilerOptions) -> Vec<CompilerOptionViolation> {
    let mut violations = Vec::new();
    if options.strict_property_initialization == Some(true)
        && !options.strict_option_value(options.strict_null_checks)
    {
        violations
            .push(CompilerOptionViolation::StrictPropertyInitializationRequiresStrictNullChecks);
    }
    if options.exact_optional_property_types == Some(true)
        && !options.strict_option_value(options.strict_null_checks)
    {
        violations
            .push(CompilerOptionViolation::ExactOptionalPropertyTypesRequiresStrictNullChecks);
    }
    let verbatim = options.verbatim_module_syntax == Some(true);
    let isolated = options.isolated_modules == Some(true);
    if (isolated || verbatim)
        && options
            .out_file
            .as_deref()
            .is_some_and(|path| !path.is_empty())
    {
        violations.push(CompilerOptionViolation::OutFileConflictsWithIsolation { verbatim });
    }
    if options.isolated_declarations == Some(true) {
        if options.allow_js {
            violations.push(CompilerOptionViolation::IsolatedDeclarationsConflictsWithAllowJs);
        }
        if options.declaration != Some(true) && options.composite != Some(true) {
            violations.push(CompilerOptionViolation::IsolatedDeclarationsRequiresDeclaration);
        }
    }

    let source_map = options.source_map == Some(true);
    let inline_source_map = options.inline_source_map == Some(true);
    let inline_sources = options.inline_sources == Some(true);
    let source_root = options
        .source_root
        .as_deref()
        .is_some_and(|value| !value.is_empty());
    let map_root = options
        .map_root
        .as_deref()
        .is_some_and(|value| !value.is_empty());

    // Keep this in the same order as verifyCompilerOptions. The final
    // diagnostic collection applies TypeScript's sorting, but callers of the
    // typed validator also rely on this upstream emission order.
    if inline_source_map {
        if source_map {
            violations.push(CompilerOptionViolation::SourceMapConflictsWithInlineSourceMap);
        }
        if map_root {
            violations.push(CompilerOptionViolation::MapRootConflictsWithInlineSourceMap);
        }
    }
    if options.composite == Some(true) {
        if options.declaration == Some(false) {
            violations.push(CompilerOptionViolation::CompositeRequiresDeclaration);
        }
        if options.incremental == Some(false) {
            violations.push(CompilerOptionViolation::CompositeRequiresIncremental);
        }
    }
    if !source_map && !inline_source_map {
        if inline_sources {
            violations.push(CompilerOptionViolation::InlineSourcesRequiresSourceMap);
        }
        if source_root {
            violations.push(CompilerOptionViolation::SourceRootRequiresSourceMap);
        }
    }
    if map_root && !(source_map || options.declaration_map == Some(true)) {
        violations.push(CompilerOptionViolation::MapRootRequiresSourceMapOrDeclarationMap);
    }
    // verifyCompilerOptions (:124866-124872) reports these independently.
    if options
        .declaration_dir
        .as_deref()
        .is_some_and(|directory| !directory.is_empty())
    {
        if options.declaration != Some(true) && options.composite != Some(true) {
            violations.push(CompilerOptionViolation::DeclarationDirectoryRequiresDeclaration);
        }
        if options
            .out_file
            .as_deref()
            .is_some_and(|path| !path.is_empty())
        {
            violations.push(CompilerOptionViolation::DeclarationDirectoryConflictsWithOutFile);
        }
    }
    if options.declaration_map == Some(true)
        && options.declaration != Some(true)
        && options.composite != Some(true)
    {
        violations.push(CompilerOptionViolation::DeclarationMapRequiresDeclaration);
    }
    if isolated || verbatim {
        if isolated
            && options.module == Some(0)
            && options.emit_script_target() < tsc_types::ScriptTarget::ES2015
        {
            violations.push(CompilerOptionViolation::IsolatedModulesRequiresModuleOrEs2015);
        }
        if options.preserve_const_enums == Some(false) {
            violations
                .push(CompilerOptionViolation::PreserveConstEnumsRequiredByIsolation { verbatim });
        }
    }
    // verifyCompilerOptions (:124891-124894) tests the raw module option,
    // including JavaScript falsiness of None=0. An absent module instead
    // belongs to the source-dependent TS6131 branch, not this relation.
    if options
        .out_file
        .as_deref()
        .is_some_and(|path| !path.is_empty())
        && options.emit_declaration_only != Some(true)
        && options
            .module
            .is_some_and(|module| !matches!(module, 0 | 2 | 4))
    {
        violations.push(CompilerOptionViolation::OutFileRequiresAmdOrSystemModule);
    }
    // verifyCompilerOptions (:124901-124907) uses the computed options.
    // Classic resolution takes precedence over unsupported JSON emit modules.
    // This relationship is independent of outFile and emitDeclarationOnly.
    if options.resolve_json_module_effective() {
        if options.emit_module_resolution_kind() == 1 {
            violations
                .push(CompilerOptionViolation::ResolveJsonModuleConflictsWithClassicResolution);
        } else if matches!(options.emit_module_kind(), 0 | 3 | 4) {
            violations.push(CompilerOptionViolation::ResolveJsonModuleConflictsWithModule);
        }
    }
    if options.check_js == Some(true) && !options.allow_js {
        violations.push(CompilerOptionViolation::CheckJsRequiresAllowJs);
    }
    if options.emit_declaration_only == Some(true)
        && options.declaration != Some(true)
        && options.composite != Some(true)
    {
        violations.push(CompilerOptionViolation::EmitDeclarationOnlyRequiresDeclaration);
    }

    if options.emit_decorator_metadata == Some(true) && !options.experimental_decorators {
        violations.push(CompilerOptionViolation::DecoratorMetadataRequiresExperimentalDecorators);
    }
    let target = options.emit_script_target();
    let jsx_factory = options
        .jsx_factory
        .as_deref()
        .filter(|value| !value.is_empty());
    let jsx_fragment_factory = options
        .jsx_fragment_factory
        .as_deref()
        .filter(|value| !value.is_empty());
    let react_namespace = options
        .react_namespace
        .as_deref()
        .filter(|value| !value.is_empty());
    let jsx_import_source = options
        .jsx_import_source
        .as_deref()
        .filter(|value| !value.is_empty());

    if let Some(factory) = jsx_factory {
        if react_namespace.is_some() {
            violations.push(CompilerOptionViolation::ReactNamespaceConflictsWithJsxFactory);
        }
        if let Some(jsx) = automatic_jsx_runtime_name(options.jsx) {
            violations
                .push(CompilerOptionViolation::JsxFactoryConflictsWithAutomaticRuntime { jsx });
        }
        if !is_entity_name_text(factory, target) {
            violations.push(CompilerOptionViolation::InvalidJsxFactory {
                value: factory.to_owned(),
            });
        }
    } else if let Some(namespace) = react_namespace {
        if !is_identifier_text_for_target(namespace, target) {
            violations.push(CompilerOptionViolation::InvalidReactNamespace {
                value: namespace.to_owned(),
            });
        }
    }

    if let Some(fragment_factory) = jsx_fragment_factory {
        if jsx_factory.is_none() {
            violations.push(CompilerOptionViolation::JsxFragmentFactoryRequiresJsxFactory);
        }
        if let Some(jsx) = automatic_jsx_runtime_name(options.jsx) {
            violations.push(
                CompilerOptionViolation::JsxFragmentFactoryConflictsWithAutomaticRuntime { jsx },
            );
        }
        if !is_entity_name_text(fragment_factory, target) {
            violations.push(CompilerOptionViolation::InvalidJsxFragmentFactory {
                value: fragment_factory.to_owned(),
            });
        }
    }
    if react_namespace.is_some() {
        if let Some(jsx) = automatic_jsx_runtime_name(options.jsx) {
            violations
                .push(CompilerOptionViolation::ReactNamespaceConflictsWithAutomaticRuntime { jsx });
        }
    }
    if jsx_import_source.is_some() && options.jsx == Some(2) {
        violations.push(CompilerOptionViolation::JsxImportSourceConflictsWithClassicRuntime);
    }
    // The option-only tail of verifyCompilerOptions is shared by config and
    // createProgram, including effective defaults and key/value locations.
    let module = options.emit_module_kind();
    let resolution = options.emit_module_resolution_kind();
    if verbatim && matches!(module, 2..=4) {
        violations.push(CompilerOptionViolation::VerbatimModuleRequiresSupportedModule);
    }
    if options.allow_importing_ts_extensions == Some(true)
        && options.no_emit != Some(true)
        && options.emit_declaration_only != Some(true)
        && options.rewrite_relative_import_extensions != Some(true)
    {
        violations.push(CompilerOptionViolation::AllowImportingTsExtensionsRequiresEmitMode);
    }
    if !matches!(resolution, 3..=100) {
        if options.resolve_package_json_exports == Some(true) {
            violations.push(CompilerOptionViolation::PackageJsonExportsRequiresModernResolution);
        }
        if options.resolve_package_json_imports == Some(true) {
            violations.push(CompilerOptionViolation::PackageJsonImportsRequiresModernResolution);
        }
        if options.custom_conditions.is_some() {
            violations.push(CompilerOptionViolation::CustomConditionsRequiresModernResolution);
        }
    }
    if resolution == 100 && !matches!(module, 1 | 5..=99 | 200) {
        violations.push(CompilerOptionViolation::BundlerRequiresEsModule);
    }
    if matches!(module, 100..=102 | 199) && !matches!(resolution, 3..=99) {
        violations.push(CompilerOptionViolation::NodeModuleRequiresNodeResolution {
            module: match module {
                101 => "Node18",
                102 => "Node20",
                199 => "NodeNext",
                _ => "Node16",
            },
            resolution: if module == 199 { "NodeNext" } else { "Node16" },
        });
    } else if matches!(resolution, 3 | 99) && !matches!(module, 100..=199) {
        violations.push(CompilerOptionViolation::NodeResolutionRequiresNodeModule {
            resolution: if resolution == 99 {
                "NodeNext"
            } else {
                "Node16"
            },
        });
    }
    violations
}

/// Render the raw-sensitive paths validation plan against the final effective
/// compiler options. In particular, TS5090 is intentionally delayed until an
/// embedding has applied its `baseUrl` override.
///
/// tsc-port: verifyCompilerOptions @6.0.3 (paths block)
/// tsc-hash: e18b8511def0edd57da25ed1bbcbd52b5d675efdeba80d8f8e924b5cb2a9b391
/// tsc-span: _tsc.js:124805-124854
pub fn validate_paths_option_diagnostics(
    options: &CompilerOptions,
    program_options: &ProgramOptions,
) -> Vec<Diagnostic> {
    let Some(plan) = program_options.paths_option_validation() else {
        return Vec::new();
    };
    let mut diagnostics = Vec::new();
    for violation in plan.violations() {
        let message = match violation.kind() {
            PathsOptionViolationKind::PatternHasMultipleAsterisks { pattern } => MessageChain::new(
                &gen::Pattern_0_can_have_at_most_one_character,
                std::slice::from_ref(pattern),
            ),
            PathsOptionViolationKind::SubstitutionsNotArray { pattern } => MessageChain::new(
                &gen::Substitutions_for_pattern_0_should_be_an_array,
                std::slice::from_ref(pattern),
            ),
            PathsOptionViolationKind::EmptySubstitutions { pattern } => MessageChain::new(
                &gen::Substitutions_for_pattern_0_shouldn_t_be_an_empty_array,
                std::slice::from_ref(pattern),
            ),
            PathsOptionViolationKind::SubstitutionHasMultipleAsterisks {
                pattern,
                substitution,
            } => MessageChain::new(
                &gen::Substitution_0_in_pattern_1_can_have_at_most_one_character,
                &[substitution.clone(), pattern.clone()],
            ),
            PathsOptionViolationKind::SubstitutionHasIncorrectType {
                pattern,
                substitution,
                actual_type,
            } => MessageChain::new(
                &gen::Substitution_0_for_pattern_1_has_incorrect_type_expected_string_got_2,
                &[substitution.clone(), pattern.clone(), actual_type.clone()],
            ),
            PathsOptionViolationKind::NonRelativeSubstitutionWithoutBaseUrl => {
                if options
                    .base_url
                    .as_deref()
                    .is_some_and(|base_url| !base_url.is_empty())
                {
                    continue;
                }
                MessageChain::new(
                    &gen::Non_relative_paths_are_not_allowed_when_baseUrl_is_not_set_Did_you_forget_a_leading,
                    &[],
                )
            }
        };
        diagnostics.push(match violation.location() {
            Some(location) => Diagnostic::new(
                Some(location.file_name().to_owned()),
                Some(location.span().start()),
                Some(location.span().length()),
                message,
            )
            .with_file_path(
                program_options
                    .config_file()
                    .filter(|config| config.diagnostic_file_name() == location.file_name())
                    .map_or("", |config| config.diagnostic_file_path()),
            ),
            None => Diagnostic::new(None, None, None, message),
        });
    }
    sort_and_dedupe_diagnostics(&mut diagnostics);
    diagnostics
}

pub(crate) fn paths_validation_plan_for_typed_mappings(
    mappings: &[PathMapping],
) -> PathsOptionValidationPlan {
    let mut violations = Vec::new();
    for mapping in mappings {
        let pattern = mapping.pattern();
        if !has_zero_or_one_asterisk(pattern) {
            violations.push(PathsOptionViolation::new(
                PathsOptionViolationKind::PatternHasMultipleAsterisks {
                    pattern: pattern.to_owned(),
                },
                None,
            ));
        }
        if mapping.substitutions().is_empty() {
            violations.push(PathsOptionViolation::new(
                PathsOptionViolationKind::EmptySubstitutions {
                    pattern: pattern.to_owned(),
                },
                None,
            ));
        }
        for substitution in mapping.substitutions() {
            if !has_zero_or_one_asterisk(substitution) {
                violations.push(PathsOptionViolation::new(
                    PathsOptionViolationKind::SubstitutionHasMultipleAsterisks {
                        pattern: pattern.to_owned(),
                        substitution: substitution.clone(),
                    },
                    None,
                ));
            }
            if !path_is_relative(substitution) && !path_is_absolute(substitution) {
                violations.push(PathsOptionViolation::new(
                    PathsOptionViolationKind::NonRelativeSubstitutionWithoutBaseUrl,
                    None,
                ));
            }
        }
    }
    PathsOptionValidationPlan::new(violations)
}

/// tsc-port: hasZeroOrOneAsteriskCharacter @6.0.3
/// tsc-hash: 28a64969081ad59009ed6f3fcb192a4ccef94471b01213a5de12c284cdd6eb45
/// tsc-span: _tsc.js:18318-18330
pub(crate) fn has_zero_or_one_asterisk(value: &str) -> bool {
    value.bytes().filter(|byte| *byte == b'*').take(2).count() <= 1
}

/// tsc-port: pathIsRelative @6.0.3
/// tsc-hash: f202555c891d7a914e21c5fe1199667a8d221940ce66c814b4898adfb228aac9
/// tsc-span: _tsc.js:5314-5316
pub(crate) fn path_is_relative(path: &str) -> bool {
    matches!(path, "." | "..")
        || path.starts_with("./")
        || path.starts_with(".\\")
        || path.starts_with("../")
        || path.starts_with("..\\")
}

/// tsc-port: pathIsAbsolute @6.0.3
/// tsc-hash: 0e64b150a899a6eb39ac2a3b370896f59ec02bdefdf07106a8740624318eb3f3
/// tsc-span: _tsc.js:5311-5313
/// tsc-port: getEncodedRootLength @6.0.3 (absolute/nonzero projection)
/// tsc-hash: ad42b701dd98c53ad89476947bccf551e3ab3db9ce0c9fc5009e16a41b49b1f9
/// tsc-span: _tsc.js:5349-5386
pub(crate) fn path_is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    if matches!(bytes.first(), Some(b'/' | b'\\')) {
        return true;
    }
    if bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(1) == Some(&b':')
        && (bytes.len() == 2 || matches!(bytes.get(2), Some(b'/' | b'\\')))
    {
        return true;
    }
    // TypeScript recognizes URL roots only with the literal forward-slash
    // separator. Normalizing backslashes first would incorrectly treat
    // `scheme:\\host` as absolute and suppress TS5090.
    path.contains("://")
}

fn automatic_jsx_runtime_name(jsx: Option<i32>) -> Option<&'static str> {
    match jsx {
        Some(4) => Some("react-jsx"),
        Some(5) => Some("react-jsxdev"),
        _ => None,
    }
}

#[cfg(test)]
#[path = "../tests/unit/option_validation_tests.rs"]
mod tests;
