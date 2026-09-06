use std::error::Error;
use std::fmt;

use tsc_program::SourceFileId;
use tsc_syntax::NodeId;
use tsc_types::SymbolFlags;

use crate::factory::{TransformArena, TransformNode, TransformSourceId};
use crate::transform::TransformError;
use crate::{EmitConstantValue, EmitEnumMemberValue};

/// The checker result of an emit-resolver accessibility query.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EmitSymbolAccessibility {
    Accessible = 0,
    NotAccessible = 1,
    CannotBeNamed = 2,
    NotResolved = 3,
}

/// The result shape returned by the declaration-emit accessibility queries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmitSymbolAccessibilityResult {
    pub accessibility: EmitSymbolAccessibility,
    /// `Some` is reserved for a non-empty ordered alias list.
    pub aliases_to_make_visible: Option<Vec<EmitResolverNode>>,
    /// Error symbol name reported by the checker accessibility worker.
    pub error_symbol_name: Option<String>,
    /// Error module name reported by the checker accessibility worker.
    pub error_module_name: Option<String>,
    pub error_node: Option<EmitResolverNode>,
}

/// The symbol-meaning mask passed to an emit-resolver accessibility query.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EmitSymbolMeaning(pub u32);

impl EmitSymbolMeaning {
    pub const VALUE_EXPORT_VALUE: Self = Self(111_551 | 1_048_576);
    pub const NAMESPACE: Self = Self(1_920);
    pub const TYPE: Self = Self(788_968);
    pub const ALIAS_RESOLVE: Self = Self(111_551 | 788_968 | 1_920 | 2_097_152);
    pub const IMPORT_EQUALS_RESOLVE: Self = Self(111_551 | 788_968 | 1_920);
}

/// A session-scoped handle to a checker symbol.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EmitResolverSymbol {
    pub session_token: u64,
    pub symbol_index: u32,
}

/// A property returned by `getPropertiesOfContainerFunction`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmitFunctionProperty {
    pub name: String,
    pub symbol: EmitResolverSymbol,
    pub parent: EmitResolverSymbol,
    pub value_declaration: Option<EmitResolverNode>,
}

/// NodeBuilderFlags word consumed by the declaration-serialization resolver
/// members. Upstream numerics verbatim; the vendored inline constants are the
/// authority for every named bit.
/// tsc-port: NodeBuilderFlags @6.0.3
/// tsc-hash: 0223b722e847648b076bd0aeac3874a1695e09c55c917b40f51043b49c7c02b0
/// tsc-span: _tsc.js:114263-114263
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct EmitNodeBuilderFlags(pub u32);

impl EmitNodeBuilderFlags {
    pub const NONE: Self = Self(0);
    pub const NO_TRUNCATION: Self = Self(1);
    pub const GENERATE_NAMES_FOR_SHADOWED_TYPE_PARAMS: Self = Self(4);
    pub const USE_STRUCTURAL_FALLBACK: Self = Self(8);
    pub const SUPPRESS_ANY_RETURN_TYPE: Self = Self(256);
    pub const MULTILINE_OBJECT_LITERALS: Self = Self(1024);
    pub const WRITE_CLASS_EXPRESSION_AS_TYPE_LITERAL: Self = Self(2048);
    pub const USE_TYPE_OF_FUNCTION: Self = Self(4096);
    pub const ALLOW_EMPTY_TUPLE: Self = Self(524_288);
    /// The `declarationEmitNodeBuilderFlags` composition at _tsc.js:114263.
    pub const DECLARATION_EMIT: Self = Self(1024 | 2048 | 4096 | 8 | 524_288 | 4 | 1);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// InternalNodeBuilderFlags word.
/// tsc-port: InternalNodeBuilderFlags @6.0.3
/// tsc-hash: 2754707d6e07f23719ba3f001d9cc2bf9e740ee34ecfcedde771334818dd0b38
/// tsc-span: _tsc.js:114264-114264
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct EmitInternalNodeBuilderFlags(pub u32);

impl EmitInternalNodeBuilderFlags {
    pub const NONE: Self = Self(0);
    pub const WRITE_COMPUTED_PROPS: Self = Self(1);
    pub const NO_SYNTACTIC_PRINTER: Self = Self(2);
    pub const ALLOW_UNRESOLVED_NAMES: Self = Self(8);
    /// The `declarationEmitInternalNodeBuilderFlags` value at _tsc.js:114264.
    pub const DECLARATION_EMIT: Self = Self(8);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// The `symbolToDeclarations` expansion out-parameter.
/// tsc-span: _tsc.js:51246-51253
/// (context.out construction and copy)
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EmitSymbolExpansionOut {
    pub can_increase_expansion_depth: bool,
    pub truncated: bool,
}

/// Opaque checker-minted symbol token handed to tracker callbacks and
/// accepted back verbatim by [`EmitTrackerAccess`]. The token is valid only
/// for the duration of the callback that received it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EmitTrackerSymbol(pub u64);

/// Opaque checker-minted node token (parse-tree or synthesized identity)
/// with the same callback-scoped validity as [`EmitTrackerSymbol`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EmitTrackerNode(pub u64);

/// Recording projection of a tracker node token: parse-tree coordinates,
/// original-node coordinates for a synthesized node, or neither.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EmitTrackerNodeDescription {
    pub parse: Option<EmitResolverNode>,
    pub original: Option<EmitResolverNode>,
}

/// Recording projection of a tracker symbol token (the probe symbolRef
/// shape: verbatim escaped name, declaration count, first eight
/// declarations).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EmitTrackerSymbolDescription {
    pub escaped_name: String,
    pub declaration_count: u32,
    pub declarations: Vec<EmitTrackerNodeDescription>,
}

/// The four resolver queries the upstream declaration transformer's tracker
/// re-enters, exposed re-entrantly by the checker for the duration of a
/// tracker callback so no second checker borrow is ever taken.
/// tsc-span: _tsc.js:114362-114369
/// (trackSymbol), :114317-114331
/// (reportExpandoFunctionErrors), :114193-114201 (parameter error arm)
pub trait EmitTrackerAccess {
    fn is_symbol_accessible(
        &mut self,
        symbol: EmitTrackerSymbol,
        enclosing_declaration: Option<EmitTrackerNode>,
        meaning: EmitSymbolMeaning,
        should_compute_aliases: bool,
    ) -> Result<EmitSymbolAccessibilityResult, EmitResolverError>;

    fn is_expando_function_declaration(
        &mut self,
        node: EmitTrackerNode,
    ) -> Result<bool, EmitResolverError>;

    fn get_properties_of_container_function(
        &mut self,
        node: EmitTrackerNode,
    ) -> Result<Vec<EmitFunctionProperty>, EmitResolverError>;

    fn requires_adding_implicit_undefined(
        &mut self,
        parameter: EmitTrackerNode,
        enclosing_declaration: Option<EmitTrackerNode>,
    ) -> Result<bool, EmitResolverError>;

    /// Recording projections for harness trackers; production trackers may
    /// ignore these.
    fn describe_symbol(&mut self, symbol: EmitTrackerSymbol) -> EmitTrackerSymbolDescription;

    fn describe_node(&mut self, node: EmitTrackerNode) -> EmitTrackerNodeDescription;
}

/// Narrow module-specifier host protocol consumed by the NodeBuilder's
/// specifier synthesis. The seven concrete members mirror the
/// always-consulted upstream host reads; the capability members model the
/// optional host surfaces with typed absent defaults (the h2-7a-m-3 §3a
/// host-fact discipline: an absent capability answers its typed absent
/// form, and no arm may fabricate a specifier from a missing capability).
/// tsc-span: _tsc.js:90948-90968
/// (basic host), :45368-46289 (consumption)
pub trait EmitModuleSpecifierHost {
    fn get_current_directory(&self) -> String;
    fn use_case_sensitive_file_names(&self) -> bool;
    fn file_exists(&self, file_name: &str) -> bool;
    fn read_file(&self, file_name: &str) -> Option<String>;
    fn get_common_source_directory(&self) -> String;
    /// Default resolution mode for a file (upstream `getDefaultResolutionModeForFile`).
    fn get_default_resolution_mode_for_file(&self, file: EmitResolverNode) -> EmitResolutionMode;
    /// Resolution mode at a module-specifier index (upstream
    /// `getModeForResolutionAtIndex`).
    fn get_mode_for_resolution_at_index(
        &self,
        file: EmitResolverNode,
        index: u32,
    ) -> EmitResolutionMode;

    // Capability-optional surfaces (typed absent defaults).
    fn symlinked_directories(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    fn symlinked_files(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    fn get_nearest_ancestor_directory_with_package_json(&self, _file_name: &str) -> Option<String> {
        None
    }
    fn get_global_typings_cache_location(&self) -> Option<String> {
        None
    }
    fn redirect_targets(&self, _file_path: &str) -> Vec<String> {
        Vec::new()
    }
    fn get_redirect_from_source_file(&self, _file_name: &str) -> Option<String> {
        None
    }
    fn is_source_of_project_reference_redirect(&self, _file_name: &str) -> bool {
        false
    }
    /// The Import-kind file-include reasons for an imported module path —
    /// the existing-specifier-reuse arm of `computeModuleSpecifiers`
    /// (_tsc.js:45496-45508): each row names an importing Program file and
    /// the module-specifier index inside it; the checker reads the literal
    /// specifier text at that index from its own parse tree. Hosts without
    /// include-reason tracking answer empty.
    fn import_include_reasons(&self, _imported_path: &str) -> Vec<EmitImportIncludeReason> {
        Vec::new()
    }
    /// Whether the host carries a module-resolution cache (the
    /// `getAllModulePathsWorker` package-json dependency arm,
    /// _tsc.js:45717-45735, additionally requires the package-json
    /// capability before it can act on this).
    fn module_resolution_cache_available(&self) -> bool {
        false
    }
}

/// One Import-kind file-include reason row (upstream `FileIncludeReason`
/// with `kind === Import`): the importing Program file and the index of the
/// module-specifier literal inside it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmitImportIncludeReason {
    pub importing_file: SourceFileId,
    pub index: u32,
}

/// Module resolution mode selector (upstream `ResolutionMode`:
/// undefined | CommonJS = 1 | ESNext = 99).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum EmitResolutionMode {
    #[default]
    None,
    CommonJs,
    EsNext,
}

/// Caller-supplied symbol tracker: the declaration transformer's tracker
/// object protocol behind the checker's SymbolTrackerImpl forwarding.
/// Callbacks that receive an [`EmitTrackerAccess`] handle are fallible and
/// propagate checker aborts fail-closed; pure report callbacks are
/// infallible. Default implementations model an absent upstream member
/// (SymbolTrackerImpl forwards only to members that exist).
/// tsc-port: symbolTracker @6.0.3 (transformer object) + SymbolTrackerImpl
/// tsc-hash: b4bd865ce0b957f5fa54a3d254cd9265290499820e924c6d10156fac0a09228a
/// tsc-span: _tsc.js:114280-114304, :90969-91068
pub trait EmitSymbolTracker {
    /// Upstream member presence: SymbolTrackerImpl computes
    /// `canTrackSymbol` from the inner tracker's `trackSymbol` existence.
    fn can_track_symbol(&self) -> bool {
        false
    }

    /// tsc-port: trackSymbol @6.0.3
    /// tsc-hash: d64605e9e90a69fc35680689c16e2c076a85f20902bf78ac699284e7189c7f85
    /// tsc-span: _tsc.js:114360-114370
    fn track_symbol(
        &mut self,
        access: &mut dyn EmitTrackerAccess,
        symbol: EmitTrackerSymbol,
        symbol_flags: SymbolFlags,
        enclosing_declaration: Option<EmitTrackerNode>,
        meaning: EmitSymbolMeaning,
    ) -> Result<bool, EmitResolverError> {
        let _ = (access, symbol, symbol_flags, enclosing_declaration, meaning);
        Ok(false)
    }

    fn report_inference_fallback(
        &mut self,
        access: &mut dyn EmitTrackerAccess,
        node: EmitTrackerNode,
    ) -> Result<(), EmitResolverError> {
        let _ = (access, node);
        Ok(())
    }

    fn report_private_in_base_of_class_expression(&mut self, property_name: &str) {
        let _ = property_name;
    }

    fn report_inaccessible_unique_symbol_error(&mut self) {}

    fn report_cyclic_structure_error(&mut self) {}

    fn report_inaccessible_this_error(&mut self) {}

    fn report_likely_unsafe_import_required_error(
        &mut self,
        specifier: &str,
        symbol_name: Option<&str>,
    ) {
        let _ = (specifier, symbol_name);
    }

    fn report_truncation_error(&mut self) {}

    /// tsc-port: reportNonlocalAugmentation @6.0.3
    /// tsc-hash: 087a8e3b3f966c3356348f688be63ce7db15cc3179310de23a29fe56669d95fe
    /// tsc-span: _tsc.js:114413-114425
    fn report_nonlocal_augmentation(
        &mut self,
        primary_declaration: Option<EmitTrackerNodeDescription>,
        augmenting_declarations: Vec<EmitTrackerNodeDescription>,
    ) {
        let _ = (primary_declaration, augmenting_declarations);
    }

    fn report_non_serializable_property(&mut self, property_name: &str) {
        let _ = property_name;
    }

    /// tsc-port: pushErrorFallbackNode @6.0.3
    /// tsc-hash: 3c76d7c1df2f8bb11f48c5d1fc17a8aa1f4045e220624ba13bef2eca2702f409
    /// tsc-span: _tsc.js:114292-114300
    fn push_error_fallback_node(&mut self, node: Option<EmitTrackerNodeDescription>) {
        let _ = node;
    }

    fn pop_error_fallback_node(&mut self) {}

    fn module_specifier_host(&self) -> Option<&dyn EmitModuleSpecifierHost> {
        None
    }
}

/// Stable source/node identity passed from the emitter back into the live
/// checker. Synthetic nodes are never valid resolver inputs; transforms first
/// follow their original-node chain and then project the owning Program file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmitResolverNode {
    source: SourceFileId,
    node: NodeId,
}

/// Runtime constructor category selected by TypeScript's checker for a type
/// reference used by `emitDecoratorMetadata`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmitTypeReferenceSerializationKind {
    Unknown,
    TypeWithConstructSignatureAndValue,
    VoidNullableOrNeverType,
    NumberLikeType,
    BigIntLikeType,
    StringLikeType,
    BooleanType,
    ArrayLikeType,
    ESSymbolType,
    Promise,
    TypeWithCallSignature,
    ObjectType,
}

impl EmitResolverNode {
    pub const fn new(source: SourceFileId, node: NodeId) -> Self {
        Self { source, node }
    }

    /// Construct the resolver identity at a checker boundary that retains the
    /// authoritative source token but does not otherwise depend on the
    /// prepared-program crate.
    pub const fn from_raw_source(source: u32, node: NodeId) -> Self {
        Self::new(SourceFileId::from_raw(source), node)
    }

    pub const fn source(self) -> SourceFileId {
        self.source
    }

    pub const fn node(self) -> NodeId {
        self.node
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EmitResolverMethod {
    GetConstantValue,
    GetEnumMemberValue,
    GetReferencedExportContainer,
    GetExternalModuleFileFromDeclaration,
    GetReferencedImportDeclaration,
    GetReferencedImportDeclarationAtLocation,
    GetJsxFactoryImportDeclaration,
    GetJsxFactoryExportContainer,
    GetReferencedDeclarationWithCollidingName,
    GetReferencedValueDeclaration,
    GetReferencedValueDeclarations,
    GetTypeReferenceSerializationKind,
    HasNodeCheckFlag,
    IsArgumentsLocalBinding,
    IsBindingCapturedByNode,
    IsDeclarationWithCollidingName,
    IsExternalOrCommonJsModule,
    IsInstantiatedModule,
    IsUniqueLocalName,
    HasGlobalName,
    CollectLinkedAliases,
    CanIncludeBindAndCheckDiagnostics,
    IsReferencedAliasDeclaration,
    IsTopLevelValueImportEqualsWithEntityName,
    IsValueAliasDeclaration,
    IsDefinitelyReferenceToGlobalSymbolObject,
    IsSymbolAccessible,
    IsEntityNameVisible,
    IsDeclarationVisible,
    IsOptionalParameter,
    IsImplementationOfOverload,
    RequiresAddingImplicitUndefined,
    IsExpandoFunctionDeclaration,
    GetPropertiesOfContainerFunction,
    IsLiteralConstDeclaration,
    IsLateBound,
    IsImportRequiredByAugmentation,
    IsLastBodilessOverloadOfSymbol,
    IsFirstDeclarationOfSymbol,
    CreateTypeOfDeclaration,
    CreateTypeOfDeclarationInExpandoScope,
    CreateReturnTypeOfSignatureDeclaration,
    CreateTypeOfExpression,
    CreateLiteralConstValue,
    GetDeclarationStatementsForSourceFile,
    CreateLateBoundIndexSignatures,
    SymbolToDeclarations,
}

/// Selects the checker view used by `getReferencedExportContainer`.
///
/// Ordinary references and names created by TypeScript's
/// `getDeclarationName` retain a merged declaration's local binding. Names
/// created by `getExportName` set `prefixLocals`, allowing the module
/// transformer to address the owning export even when that symbol also has a
/// local declaration.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EmitExportContainerMode {
    Reference,
    ExportName,
}

impl EmitExportContainerMode {
    pub const fn prefixes_locals(self) -> bool {
        matches!(self, Self::ExportName)
    }
}

impl EmitResolverMethod {
    /// tsrs-native: stable diagnostic names for the typed resolver operation set.
    pub const fn name(self) -> &'static str {
        match self {
            Self::GetConstantValue => "getConstantValue",
            Self::GetEnumMemberValue => "getEnumMemberValue",
            Self::GetReferencedExportContainer => "getReferencedExportContainer",
            Self::GetExternalModuleFileFromDeclaration => "getExternalModuleFileFromDeclaration",
            Self::GetReferencedImportDeclaration => "getReferencedImportDeclaration",
            Self::GetReferencedImportDeclarationAtLocation => {
                "getReferencedImportDeclarationAtLocation"
            }
            Self::GetJsxFactoryImportDeclaration => "getJsxFactoryImportDeclaration",
            Self::GetJsxFactoryExportContainer => "getJsxFactoryExportContainer",
            Self::GetReferencedDeclarationWithCollidingName => {
                "getReferencedDeclarationWithCollidingName"
            }
            Self::GetReferencedValueDeclaration => "getReferencedValueDeclaration",
            Self::GetReferencedValueDeclarations => "getReferencedValueDeclarations",
            Self::GetTypeReferenceSerializationKind => "getTypeReferenceSerializationKind",
            Self::HasNodeCheckFlag => "hasNodeCheckFlag",
            Self::IsArgumentsLocalBinding => "isArgumentsLocalBinding",
            Self::IsBindingCapturedByNode => "isBindingCapturedByNode",
            Self::IsDeclarationWithCollidingName => "isDeclarationWithCollidingName",
            Self::IsExternalOrCommonJsModule => "isExternalOrCommonJsModule",
            Self::IsInstantiatedModule => "isInstantiatedModule",
            Self::IsUniqueLocalName => "isUniqueLocalName",
            Self::HasGlobalName => "hasGlobalName",
            Self::CollectLinkedAliases => "collectLinkedAliases",
            Self::CanIncludeBindAndCheckDiagnostics => "canIncludeBindAndCheckDiagnostics",
            Self::IsReferencedAliasDeclaration => "isReferencedAliasDeclaration",
            Self::IsTopLevelValueImportEqualsWithEntityName => {
                "isTopLevelValueImportEqualsWithEntityName"
            }
            Self::IsValueAliasDeclaration => "isValueAliasDeclaration",
            Self::IsDefinitelyReferenceToGlobalSymbolObject => {
                "isDefinitelyReferenceToGlobalSymbolObject"
            }
            Self::IsSymbolAccessible => "isSymbolAccessible",
            Self::IsEntityNameVisible => "isEntityNameVisible",
            Self::IsDeclarationVisible => "isDeclarationVisible",
            Self::IsOptionalParameter => "isOptionalParameter",
            Self::IsImplementationOfOverload => "isImplementationOfOverload",
            Self::RequiresAddingImplicitUndefined => "requiresAddingImplicitUndefined",
            Self::IsExpandoFunctionDeclaration => "isExpandoFunctionDeclaration",
            Self::GetPropertiesOfContainerFunction => "getPropertiesOfContainerFunction",
            Self::IsLiteralConstDeclaration => "isLiteralConstDeclaration",
            Self::IsLateBound => "isLateBound",
            Self::IsImportRequiredByAugmentation => "isImportRequiredByAugmentation",
            Self::IsLastBodilessOverloadOfSymbol => "isLastBodilessOverloadOfSymbol",
            Self::IsFirstDeclarationOfSymbol => "isFirstDeclarationOfSymbol",
            Self::CreateTypeOfDeclaration => "createTypeOfDeclaration",
            Self::CreateTypeOfDeclarationInExpandoScope => "createTypeOfDeclarationInExpandoScope",
            Self::CreateReturnTypeOfSignatureDeclaration => {
                "createReturnTypeOfSignatureDeclaration"
            }
            Self::CreateTypeOfExpression => "createTypeOfExpression",
            Self::CreateLiteralConstValue => "createLiteralConstValue",
            Self::GetDeclarationStatementsForSourceFile => "getDeclarationStatementsForSourceFile",
            Self::CreateLateBoundIndexSignatures => "createLateBoundIndexSignatures",
            Self::SymbolToDeclarations => "symbolToDeclarations",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmitResolverError {
    Unavailable {
        method: EmitResolverMethod,
        node: EmitResolverNode,
    },
    UnknownSymbol {
        method: EmitResolverMethod,
        symbol: EmitResolverSymbol,
    },
    ForeignSymbol {
        method: EmitResolverMethod,
        symbol: EmitResolverSymbol,
    },
    UnknownSource {
        method: EmitResolverMethod,
        node: EmitResolverNode,
    },
    UnknownNode {
        method: EmitResolverMethod,
        node: EmitResolverNode,
    },
    SourceNodeMismatch {
        method: EmitResolverMethod,
        node: EmitResolverNode,
        actual_program_index: usize,
    },
    CheckerAborted {
        method: EmitResolverMethod,
        node: EmitResolverNode,
        reason: &'static str,
    },
    /// Fail-closed default for symbol-scoped members that carry no node
    /// argument (`symbolToDeclarations`).
    UnavailableForSymbol {
        method: EmitResolverMethod,
        symbol: EmitResolverSymbol,
    },
    /// Fail-closed default for a name-scoped member (`hasGlobalName`).
    UnavailableForName {
        method: EmitResolverMethod,
        name: Box<str>,
    },
    /// Fail-closed default for a source-scoped member with no node argument.
    UnavailableForSource {
        method: EmitResolverMethod,
        source: SourceFileId,
    },
    /// A factory or arena operation inside a serialization member failed.
    /// Boxed: `TransformError::Resolver` already wraps this type, so the
    /// unboxed form would be an infinite-size cycle.
    Factory {
        method: EmitResolverMethod,
        error: Box<TransformError>,
    },
}

impl fmt::Display for EmitResolverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { method, node } => write!(
                formatter,
                "emit resolver method {} is unavailable for source {} node {}",
                method.name(),
                node.source().raw(),
                node.node().0
            ),
            Self::UnknownSymbol { method, symbol } => write!(
                formatter,
                "emit resolver method {} received unknown symbol {} in session {}",
                method.name(),
                symbol.symbol_index,
                symbol.session_token
            ),
            Self::ForeignSymbol { method, symbol } => write!(
                formatter,
                "emit resolver method {} received symbol {} from foreign session {}",
                method.name(),
                symbol.symbol_index,
                symbol.session_token
            ),
            Self::UnknownSource { method, node } => write!(
                formatter,
                "emit resolver method {} received unknown source {} for node {}",
                method.name(),
                node.source().raw(),
                node.node().0
            ),
            Self::UnknownNode { method, node } => write!(
                formatter,
                "emit resolver method {} received unknown node {} for source {}",
                method.name(),
                node.node().0,
                node.source().raw()
            ),
            Self::SourceNodeMismatch {
                method,
                node,
                actual_program_index,
            } => write!(
                formatter,
                "emit resolver method {} received source {} for node {}, but the node belongs to Program index {}",
                method.name(),
                node.source().raw(),
                node.node().0,
                actual_program_index
            ),
            Self::CheckerAborted {
                method,
                node,
                reason,
            } => write!(
                formatter,
                "emit resolver method {} aborted for source {} node {}: {}",
                method.name(),
                node.source().raw(),
                node.node().0,
                reason
            ),
            Self::UnavailableForSymbol { method, symbol } => write!(
                formatter,
                "emit resolver method {} is unavailable for symbol {} in session {}",
                method.name(),
                symbol.symbol_index,
                symbol.session_token
            ),
            Self::UnavailableForName { method, name } => write!(
                formatter,
                "emit resolver method {} is unavailable for name {}",
                method.name(),
                name
            ),
            Self::UnavailableForSource { method, source } => write!(
                formatter,
                "emit resolver method {} is unavailable for source {}",
                method.name(),
                source.raw()
            ),
            Self::Factory { method, error } => write!(
                formatter,
                "emit resolver method {} factory operation failed: {}",
                method.name(),
                error
            ),
        }
    }
}

impl Error for EmitResolverError {}

/// Consumer-owned subset of TypeScript's checker-private `EmitResolver` used
/// by the three H1 script transformers. Defaults fail closed so an expanded
/// syntax profile cannot silently fabricate a semantic answer.
pub trait EmitResolver {
    /// tsc-port: hasGlobalName @6.0.3
    /// tsc-hash: 53c9de85b0c10c5de2b868bc13acdbf715186648a451e034c1d6395b5096c7d9
    /// tsc-span: _tsc.js:88396-88398
    fn has_global_name(&self, name: &str) -> Result<bool, EmitResolverError> {
        Err(EmitResolverError::UnavailableForName {
            method: EmitResolverMethod::HasGlobalName,
            name: name.into(),
        })
    }

    /// tsc-port: collectLinkedAliases @6.0.3
    /// tsc-hash: 8fe011e257a2763196e5bd485d330cf0df070bbdf96d1d78fd9edf54c0f391c5
    /// tsc-span: _tsc.js:55675-55727
    fn collect_linked_aliases(
        &self,
        node: EmitResolverNode,
        set_visibility: bool,
    ) -> Result<Option<Vec<EmitResolverNode>>, EmitResolverError> {
        let _ = set_visibility;
        Err(unavailable(EmitResolverMethod::CollectLinkedAliases, node))
    }

    /// tsc-port: canIncludeBindAndCheckDiagnostics @6.0.3
    /// tsc-hash: e833101f7d0b7e59d1247180868406c7e65ac869387a07face9965d430e98204
    /// tsc-span: _tsc.js:18898-18905
    fn can_include_bind_and_check_diagnostics(
        &self,
        source: SourceFileId,
    ) -> Result<bool, EmitResolverError> {
        Err(EmitResolverError::UnavailableForSource {
            method: EmitResolverMethod::CanIncludeBindAndCheckDiagnostics,
            source,
        })
    }

    fn get_constant_value(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitConstantValue>, EmitResolverError> {
        Err(unavailable(EmitResolverMethod::GetConstantValue, node))
    }

    fn get_enum_member_value(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitEnumMemberValue>, EmitResolverError> {
        Err(unavailable(EmitResolverMethod::GetEnumMemberValue, node))
    }

    fn get_referenced_export_container(
        &self,
        node: EmitResolverNode,
        mode: EmitExportContainerMode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        let _ = mode;
        Err(unavailable(
            EmitResolverMethod::GetReferencedExportContainer,
            node,
        ))
    }

    /// tsc-port: getExternalModuleFileFromDeclaration @6.0.3
    /// tsc-hash: b1d92eecc4c854409d14d155534bda3b0cc44c709b9f3f2eeb970f1a233861a0
    /// tsc-span: _tsc.js:88719-88731
    fn get_external_module_file_from_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::GetExternalModuleFileFromDeclaration,
            node,
        ))
    }

    fn get_referenced_import_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::GetReferencedImportDeclaration,
            node,
        ))
    }

    /// Resolve an entity-name root as if it were parented by `location`.
    /// `emitDecoratorMetadata` clones a type name and reparents that clone to
    /// the class/name scope before the module transform asks for its import
    /// declaration. Rust's immutable syntax ownership carries that operation
    /// explicitly instead of mutating a parse-tree parent.
    fn get_referenced_import_declaration_at_location(
        &self,
        node: EmitResolverNode,
        _location: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::GetReferencedImportDeclarationAtLocation,
            node,
        ))
    }

    /// Resolve the first identifier of a classic JSX factory/fragment entity
    /// at the JSX location. Upstream gives the synthesized factory expression
    /// a parse-tree parent so module substitution can resolve imports; this
    /// typed projection carries the equivalent declaration across Rust's
    /// immutable checker/emitter boundary.
    fn get_jsx_factory_import_declaration(
        &self,
        node: EmitResolverNode,
        _name: &str,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::GetJsxFactoryImportDeclaration,
            node,
        ))
    }

    /// Resolve the first identifier of a classic JSX factory/fragment entity
    /// at the JSX location to its enclosing namespace or enum. This is the
    /// immutable typed equivalent of the parse-tree parent installed by
    /// upstream's `createReactNamespace` before TypeScript substitution.
    fn get_jsx_factory_export_container(
        &self,
        node: EmitResolverNode,
        _name: &str,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::GetJsxFactoryExportContainer,
            node,
        ))
    }

    fn get_referenced_value_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::GetReferencedValueDeclaration,
            node,
        ))
    }

    /// The declaration an identifier references, when that declaration is a
    /// block-scoped binding whose name collides during ES2015 down-level
    /// emission. The ES2015 transformer substitutes such references with the
    /// declaration's generated name.
    fn get_referenced_declaration_with_colliding_name(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::GetReferencedDeclarationWithCollidingName,
            node,
        ))
    }

    /// Whether a declaration is a block-scoped binding whose name collides
    /// during ES2015 down-level emission (an outer value binding with the
    /// same spelling, or a captured loop binding that must be renamed when
    /// its loop body is converted).
    fn is_declaration_with_colliding_name(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::IsDeclarationWithCollidingName,
            node,
        ))
    }

    /// Whether `node` (a for-statement part) captured the block-scoped
    /// binding introduced by `declaration` into a function. Loop conversion
    /// consults this to decide which loop parts ride the synthesized loop
    /// body function.
    fn is_binding_captured_by_node(
        &self,
        node: EmitResolverNode,
        declaration: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        let _ = declaration;
        Err(unavailable(
            EmitResolverMethod::IsBindingCapturedByNode,
            node,
        ))
    }

    /// All runtime declarations owned by the referenced merged symbol.
    /// Module assignment substitution needs the full set because the export
    /// binding can belong to a namespace/enum merge declaration other than
    /// the symbol's primary `valueDeclaration`.
    fn get_referenced_value_declarations(
        &self,
        node: EmitResolverNode,
    ) -> Result<Vec<EmitResolverNode>, EmitResolverError> {
        Ok(self
            .get_referenced_value_declaration(node)?
            .into_iter()
            .collect())
    }

    /// Classify a metadata type name at the lexical/name scope that will own
    /// the emitted decorator. `location` prevents a constructor parameter
    /// with the same spelling from shadowing an imported runtime class.
    fn get_type_reference_serialization_kind(
        &self,
        node: EmitResolverNode,
        _location: EmitResolverNode,
    ) -> Result<EmitTypeReferenceSerializationKind, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::GetTypeReferenceSerializationKind,
            node,
        ))
    }

    fn has_node_check_flag(
        &self,
        node: EmitResolverNode,
        _flag: u32,
    ) -> Result<bool, EmitResolverError> {
        Err(unavailable(EmitResolverMethod::HasNodeCheckFlag, node))
    }

    /// Whether an identifier resolves to the checker-owned lexical
    /// `arguments` binding rather than a user declaration or property name.
    /// Async lowering uses this to capture only references whose binding
    /// would otherwise change inside the synthesized generator function.
    fn is_arguments_local_binding(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::IsArgumentsLocalBinding,
            node,
        ))
    }

    /// Whether the source owning `node` is an external ES module or has a
    /// CommonJS indicator. The automatic JSX transformer needs this only to
    /// choose between a synthetic `import` and a direct destructuring
    /// `require` for implicit runtime helpers.
    /// tsc-port: isExternalOrCommonJsModule @6.0.3
    /// tsc-hash: e395fd4c4d5df1373eb3cc17bc653dfcd8f2e41b9e32d949b3063633dc02c07d
    /// tsc-span: _tsc.js:14119-14121
    fn is_external_or_common_js_module(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::IsExternalOrCommonJsModule,
            node,
        ))
    }

    fn is_instantiated_module(&self, node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        Err(unavailable(EmitResolverMethod::IsInstantiatedModule, node))
    }

    /// Whether `name` is absent from the value/alias locals owned by a
    /// namespace declaration and every descendant binder container in its
    /// `nextContainer` segment. The TypeScript printer uses this semantic
    /// scope query when materializing a generated module/enum name.
    fn is_unique_local_name(
        &self,
        node: EmitResolverNode,
        _name: &str,
    ) -> Result<bool, EmitResolverError> {
        Err(unavailable(EmitResolverMethod::IsUniqueLocalName, node))
    }

    fn is_referenced_alias_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::IsReferencedAliasDeclaration,
            node,
        ))
    }

    fn is_top_level_value_import_equals_with_entity_name(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::IsTopLevelValueImportEqualsWithEntityName,
            node,
        ))
    }

    fn is_value_alias_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::IsValueAliasDeclaration,
            node,
        ))
    }

    /// tsc-port: isDefinitelyReferenceToGlobalSymbolObject @6.0.3
    /// tsc-hash: 0a9f99b8eb62eb0a85b6019c76e13f9934dd0be091a0ebebf82f54a888ea237e
    /// tsc-span: _tsc.js:47469-47483
    fn is_definitely_reference_to_global_symbol_object(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::IsDefinitelyReferenceToGlobalSymbolObject,
            node,
        ))
    }

    /// tsc-port: isSymbolAccessible @6.0.3
    /// tsc-hash: 9235fa70af1dbdc19b0a98214131caf0ac9eb80fa208e9af814c4740abb1fd6f
    /// tsc-span: _tsc.js:50499-50508
    fn is_symbol_accessible(
        &self,
        symbol: EmitResolverSymbol,
        enclosing_declaration: EmitResolverNode,
        meaning: EmitSymbolMeaning,
        should_compute_aliases: bool,
    ) -> Result<EmitSymbolAccessibilityResult, EmitResolverError> {
        let _ = (symbol, meaning, should_compute_aliases);
        Err(unavailable(
            EmitResolverMethod::IsSymbolAccessible,
            enclosing_declaration,
        ))
    }

    /// tsc-port: isEntityNameVisible @6.0.3
    /// tsc-hash: 060c123c45cc5190b222fb3e1170d371492ca672405b04b4283dca7f7b5d8369
    /// tsc-span: _tsc.js:50606-50648
    fn is_entity_name_visible(
        &self,
        entity_name: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
    ) -> Result<EmitSymbolAccessibilityResult, EmitResolverError> {
        let _ = enclosing_declaration;
        Err(unavailable(
            EmitResolverMethod::IsEntityNameVisible,
            entity_name,
        ))
    }

    /// tsc-port: isDeclarationVisible @6.0.3
    /// tsc-hash: b569e8243cf2db9de0dbec7462f29fa1e70f4b94405adb5a134b6571d4c8fbeb
    /// tsc-span: _tsc.js:55589-55674
    fn is_declaration_visible(&self, node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        Err(unavailable(EmitResolverMethod::IsDeclarationVisible, node))
    }

    /// tsc-port: isOptionalParameter @6.0.3
    /// tsc-hash: 230cc8ce09e27fc4b9b6e370079e26817941e278127f592eca3c51ecb55ac67b
    /// tsc-span: _tsc.js:59509-59527
    fn is_optional_parameter(&self, node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        Err(unavailable(EmitResolverMethod::IsOptionalParameter, node))
    }

    /// tsc-port: isImplementationOfOverload @6.0.3
    /// tsc-hash: 8e84478797279cd09461d21f45d61335f27c12c7711fac1678ef91e806cfd378
    /// tsc-span: _tsc.js:88055-88068
    fn is_implementation_of_overload(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::IsImplementationOfOverload,
            node,
        ))
    }

    /// tsc-port: requiresAddingImplicitUndefined @6.0.3
    /// tsc-hash: 520f7a6ffc45898f262773b00042189eaf39127a40ed129daa103f6ce663e2d7
    /// tsc-span: _tsc.js:88075-88077
    fn requires_adding_implicit_undefined(
        &self,
        parameter: EmitResolverNode,
        enclosing_declaration: Option<EmitResolverNode>,
    ) -> Result<bool, EmitResolverError> {
        let _ = enclosing_declaration;
        Err(unavailable(
            EmitResolverMethod::RequiresAddingImplicitUndefined,
            parameter,
        ))
    }

    /// tsc-port: isExpandoFunctionDeclaration @6.0.3
    /// tsc-hash: ed2afab33ef4b7bc2c878b2943e881dedafba9030fa884dbf9155c3578fe9554
    /// tsc-span: _tsc.js:88090-88112
    fn is_expando_function_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::IsExpandoFunctionDeclaration,
            node,
        ))
    }

    /// tsc-port: getPropertiesOfContainerFunction @6.0.3
    /// tsc-hash: a194d2629e8413c8dd13b4a67567fdb2dba8a6a09f563e8b705d9488c0e588a3
    /// tsc-span: _tsc.js:88113-88120
    fn get_properties_of_container_function(
        &self,
        node: EmitResolverNode,
    ) -> Result<Vec<EmitFunctionProperty>, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::GetPropertiesOfContainerFunction,
            node,
        ))
    }

    /// tsc-port: isLiteralConstDeclaration @6.0.3
    /// tsc-hash: 1c1cef46271c6fce5e62c4307e8471561e2f10782682ef7dfff2a10013b1f1d6
    /// tsc-span: _tsc.js:88485-88490
    fn is_literal_const_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::IsLiteralConstDeclaration,
            node,
        ))
    }

    /// tsc-port: isLateBound @6.0.3
    /// tsc-hash: d11842db30b0440c571390f2deed480c5a03d4e45ef954902841c3178c938112
    /// tsc-span: _tsc.js:88600-88604
    fn is_late_bound(&self, node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        Err(unavailable(EmitResolverMethod::IsLateBound, node))
    }

    /// tsc-port: isImportRequiredByAugmentation @6.0.3
    /// tsc-hash: 7498ec7545df67711e0cdeb1967852804859c42964ae9f0f61444d3ca2c3124c
    /// tsc-span: _tsc.js:88696-88717
    fn is_import_required_by_augmentation(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::IsImportRequiredByAugmentation,
            node,
        ))
    }

    /// Serialize the type of a declaration with an inferred type into a
    /// synthesized `TypeNode` allocated in the caller's transform arena.
    /// The parse-tree filter (`hasInferredType`) and the AnyKeyword-token
    /// fallback are implementation-side and upstream-exact; the wrapper
    /// composes `flags | MULTILINE_OBJECT_LITERALS`.
    /// tsc-port: createTypeOfDeclaration @6.0.3
    /// tsc-hash: e7208311f25e05e9154f95d68bf3d6c1e0a2fa42f4e8197d11a2dc83a462c624
    /// tsc-span: _tsc.js:88359-88366
    #[allow(clippy::too_many_arguments)]
    fn create_type_of_declaration(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        declaration: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
        flags: EmitNodeBuilderFlags,
        internal_flags: EmitInternalNodeBuilderFlags,
        tracker: &mut dyn EmitSymbolTracker,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let _ = (
            arena,
            target,
            enclosing_declaration,
            flags,
            internal_flags,
            tracker,
        );
        Err(unavailable(
            EmitResolverMethod::CreateTypeOfDeclaration,
            declaration,
        ))
    }

    /// Serialize an expando property's declaration under the synthetic
    /// module scope formed from its container function's properties.
    /// tsc-port: createTypeOfDeclarationInExpandoScope @6.0.3
    /// tsc-hash: 37a21cd710c255c1fe8fc4e0e704b11c8062854069c854051651e47a8e392a90
    /// tsc-span: _tsc.js:115400-115425
    #[allow(clippy::too_many_arguments)]
    fn create_type_of_declaration_in_expando_scope(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        declaration: EmitResolverNode,
        function: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
        flags: EmitNodeBuilderFlags,
        internal_flags: EmitInternalNodeBuilderFlags,
        tracker: &mut dyn EmitSymbolTracker,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let _ = (
            arena,
            target,
            function,
            enclosing_declaration,
            flags,
            internal_flags,
            tracker,
        );
        Err(unavailable(
            EmitResolverMethod::CreateTypeOfDeclarationInExpandoScope,
            declaration,
        ))
    }

    /// tsc-port: shouldEmitFunctionProperties @6.0.3
    /// tsc-hash: 1019be7df9648f1710946cbbe99f1b872a3b8c516f16028a4da3dfcd0880e2e9
    /// tsc-span: _tsc.js:114736-114743
    fn is_last_bodiless_overload_of_symbol(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::IsLastBodilessOverloadOfSymbol,
            node,
        ))
    }

    /// tsc-port: visitDeclarationSubtree @6.0.3 (first-declaration filter)
    /// tsc-hash: 6bef4aa822019d44c58d0738e8c20b2f979f1b30a94bbb089b596794d54d19a3
    /// tsc-span: _tsc.js:114986-114988
    fn is_first_declaration_of_symbol(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::IsFirstDeclarationOfSymbol,
            node,
        ))
    }

    /// tsc-port: createReturnTypeOfSignatureDeclaration @6.0.3
    /// tsc-hash: afee5b310b2c60519f7fdfe73b676da237a0a34b6f3ae97a60a3674b892406b6
    /// tsc-span: _tsc.js:88382-88388
    #[allow(clippy::too_many_arguments)]
    fn create_return_type_of_signature_declaration(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        signature_declaration: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
        flags: EmitNodeBuilderFlags,
        internal_flags: EmitInternalNodeBuilderFlags,
        tracker: &mut dyn EmitSymbolTracker,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let _ = (
            arena,
            target,
            enclosing_declaration,
            flags,
            internal_flags,
            tracker,
        );
        Err(unavailable(
            EmitResolverMethod::CreateReturnTypeOfSignatureDeclaration,
            signature_declaration,
        ))
    }

    /// tsc-port: createTypeOfExpression @6.0.3
    /// tsc-hash: dd314f61d3160f871fe3d2568358c718dbca65cc107f1668ef3d0f6f79611fb4
    /// tsc-span: _tsc.js:88389-88395
    #[allow(clippy::too_many_arguments)]
    fn create_type_of_expression(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        expression: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
        flags: EmitNodeBuilderFlags,
        internal_flags: EmitInternalNodeBuilderFlags,
        tracker: &mut dyn EmitSymbolTracker,
    ) -> Result<Option<TransformNode>, EmitResolverError> {
        let _ = (
            arena,
            target,
            enclosing_declaration,
            flags,
            internal_flags,
            tracker,
        );
        Err(unavailable(
            EmitResolverMethod::CreateTypeOfExpression,
            expression,
        ))
    }

    /// Serialize a literal-const initializer value. Takes NO flag words —
    /// the upstream signature is `(node, tracker)`.
    /// tsc-port: createLiteralConstValue @6.0.3
    /// tsc-hash: aed30591a56b896560cdc11531e90bd746b037ffa64fa9d884cd9e384048ee53
    /// tsc-span: _tsc.js:88506-88509
    /// (helper literalTypeToNode :88491-88505)
    fn create_literal_const_value(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        node: EmitResolverNode,
        tracker: &mut dyn EmitSymbolTracker,
    ) -> Result<TransformNode, EmitResolverError> {
        let _ = (arena, target, tracker);
        Err(unavailable(
            EmitResolverMethod::CreateLiteralConstValue,
            node,
        ))
    }

    /// tsc-port: getDeclarationStatementsForSourceFile @6.0.3
    /// tsc-hash: 517de08538d0b91488cd2e54201e7dc44b404b08fe126ba36a1b63ce84ec70dc
    /// tsc-span: _tsc.js:88612-88621
    #[allow(clippy::too_many_arguments)]
    fn get_declaration_statements_for_source_file(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        node: EmitResolverNode,
        flags: EmitNodeBuilderFlags,
        internal_flags: EmitInternalNodeBuilderFlags,
        tracker: &mut dyn EmitSymbolTracker,
    ) -> Result<Option<Vec<TransformNode>>, EmitResolverError> {
        let _ = (arena, target, flags, internal_flags, tracker);
        Err(unavailable(
            EmitResolverMethod::GetDeclarationStatementsForSourceFile,
            node,
        ))
    }

    /// tsc-port: createLateBoundIndexSignatures @6.0.3
    /// tsc-hash: 57a5aa62b412607a3d4c1fc9811e8e9ec66f85ef4aa82dab2cc6afe36885e6c9
    /// tsc-span: _tsc.js:88624-88691
    #[allow(clippy::too_many_arguments)]
    fn create_late_bound_index_signatures(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        container: EmitResolverNode,
        enclosing_declaration: EmitResolverNode,
        flags: EmitNodeBuilderFlags,
        internal_flags: EmitInternalNodeBuilderFlags,
        tracker: &mut dyn EmitSymbolTracker,
    ) -> Result<Option<Vec<TransformNode>>, EmitResolverError> {
        let _ = (
            arena,
            target,
            enclosing_declaration,
            flags,
            internal_flags,
            tracker,
        );
        Err(unavailable(
            EmitResolverMethod::CreateLateBoundIndexSignatures,
            container,
        ))
    }

    /// Serialize a symbol's declarations (the NodeBuilder API surface
    /// member; zero transformer call sites — consumers arrive with the
    /// API1/H2.8 eras).
    /// tsc-port: symbolToDeclarations @6.0.3
    /// tsc-hash: 19c143701a83249c07ce89bb797f58b8ab835466cdc2b47918310f6487ac3caf
    /// tsc-span: _tsc.js:88692-88694
    /// (member :51136-51164)
    #[allow(clippy::too_many_arguments)]
    fn symbol_to_declarations(
        &self,
        arena: &mut TransformArena,
        target: TransformSourceId,
        symbol: EmitResolverSymbol,
        meaning: EmitSymbolMeaning,
        flags: EmitNodeBuilderFlags,
        maximum_length: Option<u32>,
        verbosity_level: Option<i32>,
        out: Option<&mut EmitSymbolExpansionOut>,
    ) -> Result<Vec<TransformNode>, EmitResolverError> {
        // Upstream passes NO tracker: withContext installs the basic
        // SymbolTrackerImpl over an absent inner tracker (:51136-51149).
        let _ = (
            arena,
            target,
            meaning,
            flags,
            maximum_length,
            verbosity_level,
            out,
        );
        Err(EmitResolverError::UnavailableForSymbol {
            method: EmitResolverMethod::SymbolToDeclarations,
            symbol,
        })
    }
}

fn unavailable(method: EmitResolverMethod, node: EmitResolverNode) -> EmitResolverError {
    EmitResolverError::Unavailable { method, node }
}

#[cfg(test)]
#[path = "../tests/unit/node_builder_seams/tests.rs"]
mod node_builder_seam_tests;

/// Explicit resolver for transform-only tests whose admitted syntax reaches
/// no semantic query. Any accidental expansion fails with the method/node.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UnavailableEmitResolver;

impl EmitResolver for UnavailableEmitResolver {}
