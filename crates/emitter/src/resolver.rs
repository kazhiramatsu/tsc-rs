use std::error::Error;
use std::fmt;

use tsc_program::SourceFileId;
use tsc_syntax::NodeId;

use crate::{EmitConstantValue, EmitEnumMemberValue};

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
    IsReferencedAliasDeclaration,
    IsTopLevelValueImportEqualsWithEntityName,
    IsValueAliasDeclaration,
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
    pub const fn name(self) -> &'static str {
        match self {
            Self::GetConstantValue => "getConstantValue",
            Self::GetEnumMemberValue => "getEnumMemberValue",
            Self::GetReferencedExportContainer => "getReferencedExportContainer",
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
            Self::IsReferencedAliasDeclaration => "isReferencedAliasDeclaration",
            Self::IsTopLevelValueImportEqualsWithEntityName => {
                "isTopLevelValueImportEqualsWithEntityName"
            }
            Self::IsValueAliasDeclaration => "isValueAliasDeclaration",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmitResolverError {
    Unavailable {
        method: EmitResolverMethod,
        node: EmitResolverNode,
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
        }
    }
}

impl Error for EmitResolverError {}

/// Consumer-owned subset of TypeScript's checker-private `EmitResolver` used
/// by the three H1 script transformers. Defaults fail closed so an expanded
/// syntax profile cannot silently fabricate a semantic answer.
pub trait EmitResolver {
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
}

fn unavailable(method: EmitResolverMethod, node: EmitResolverNode) -> EmitResolverError {
    EmitResolverError::Unavailable { method, node }
}

/// Explicit resolver for transform-only tests whose admitted syntax reaches
/// no semantic query. Any accidental expansion fails with the method/node.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UnavailableEmitResolver;

impl EmitResolver for UnavailableEmitResolver {}
