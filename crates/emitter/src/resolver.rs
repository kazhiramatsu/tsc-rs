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
    GetReferencedValueDeclaration,
    HasNodeCheckFlag,
    IsInstantiatedModule,
    IsReferencedAliasDeclaration,
    IsTopLevelValueImportEqualsWithEntityName,
    IsValueAliasDeclaration,
}

impl EmitResolverMethod {
    pub const fn name(self) -> &'static str {
        match self {
            Self::GetConstantValue => "getConstantValue",
            Self::GetEnumMemberValue => "getEnumMemberValue",
            Self::GetReferencedExportContainer => "getReferencedExportContainer",
            Self::GetReferencedImportDeclaration => "getReferencedImportDeclaration",
            Self::GetReferencedValueDeclaration => "getReferencedValueDeclaration",
            Self::HasNodeCheckFlag => "hasNodeCheckFlag",
            Self::IsInstantiatedModule => "isInstantiatedModule",
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
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
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

    fn get_referenced_value_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        Err(unavailable(
            EmitResolverMethod::GetReferencedValueDeclaration,
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

    fn is_instantiated_module(&self, node: EmitResolverNode) -> Result<bool, EmitResolverError> {
        Err(unavailable(EmitResolverMethod::IsInstantiatedModule, node))
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
