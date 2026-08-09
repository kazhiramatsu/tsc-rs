//! Checker-owned resolver projection used only while an emitting checker
//! session remains alive.

use std::cell::RefCell;

use tsc_emitter::{EmitResolver, EmitResolverError, EmitResolverMethod, EmitResolverNode};

use crate::state::{CheckResult, CheckerState};
use crate::{AuthoritativeSourceToken, ProgramSnapshot};
use tsc_types::CompilerOptions;

/// One fresh checker whose semantic links and transient arenas remain alive
/// while transform and print borrow its narrow [`EmitResolver`] projection.
/// The session never owns or mutates the immutable [`ProgramSnapshot`].
pub struct CheckerSession<'program> {
    state: RefCell<CheckerState<'program>>,
}

impl<'program> CheckerSession<'program> {
    /// Construct a fresh session over already parsed and bound documents.
    /// Host facts and source checking are installed by the checker driver
    /// before it exposes this session to the emitter.
    /// tsrs-native: checker-session owner over the immutable Program snapshot.
    pub fn from_snapshot(
        snapshot: &'program ProgramSnapshot,
        options: &'program CompilerOptions,
    ) -> Self {
        Self::from_checked_state(CheckerState::from_snapshot(snapshot, options))
    }

    /// Transfer a fully initialized checker into the scoped resolver owner.
    /// tsrs-native: ownership adapter for the H1 checker callback boundary.
    pub fn from_checked_state(state: CheckerState<'program>) -> Self {
        Self {
            state: RefCell::new(state),
        }
    }

    /// Borrow only the consumer-owned resolver protocol for transform/print.
    /// tsrs-native: scoped Rust borrowing form of tsc's checker-owned resolver.
    pub fn with_emit_resolver<T>(&self, operation: impl FnOnce(&dyn EmitResolver) -> T) -> T {
        operation(self)
    }

    /// Reclaim checker state after the emitter has released its resolver
    /// borrow so the driver can assemble diagnostics and observations.
    /// tsrs-native: ownership adapter after the H1 checker callback boundary.
    pub fn into_state(self) -> CheckerState<'program> {
        self.state.into_inner()
    }

    fn with_resolver_node<T>(
        &self,
        method: EmitResolverMethod,
        node: EmitResolverNode,
        operation: impl FnOnce(&mut CheckerState<'program>, tsc_syntax::NodeId) -> CheckResult<T>,
    ) -> Result<T, EmitResolverError> {
        let mut state = self.state.borrow_mut();
        validate_resolver_node(&state, method, node)?;
        operation(&mut state, node.node()).map_err(|abort| EmitResolverError::CheckerAborted {
            method,
            node,
            reason: abort.description(),
        })
    }
}

/// tsc-port: createResolver @6.0.3
/// tsc-hash: 56a0d47f897fcf258d6e316a00f9dc5e7d18a3ed1936033ab7c9350f623b3df2
/// tsc-span: _tsc.js:88545-88718
///
/// H1.3 exposes only the two resolver producers reachable from its first
/// erasable-TypeScript slice. Every other consumer-owned method retains the
/// trait's typed unavailable default until its transformer branch is admitted.
impl EmitResolver for CheckerSession<'_> {
    fn get_referenced_export_container(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetReferencedExportContainer,
            node,
            CheckerState::emit_get_referenced_export_container,
        )
        .map(|container| container.map(|container| EmitResolverNode::new(node.source(), container)))
    }

    fn get_referenced_import_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetReferencedImportDeclaration,
            node,
            CheckerState::emit_get_referenced_import_declaration,
        )
        .map(|declaration| {
            declaration.map(|declaration| EmitResolverNode::new(node.source(), declaration))
        })
    }

    fn get_referenced_value_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<Option<EmitResolverNode>, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::GetReferencedValueDeclaration,
            node,
            CheckerState::emit_get_referenced_value_declaration,
        )
        .map(|declaration| {
            declaration.map(|declaration| EmitResolverNode::new(node.source(), declaration))
        })
    }

    fn is_referenced_alias_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsReferencedAliasDeclaration,
            node,
            CheckerState::emit_is_referenced_alias_declaration,
        )
    }

    fn is_value_alias_declaration(
        &self,
        node: EmitResolverNode,
    ) -> Result<bool, EmitResolverError> {
        self.with_resolver_node(
            EmitResolverMethod::IsValueAliasDeclaration,
            node,
            CheckerState::emit_is_value_alias_declaration,
        )
    }
}

fn validate_resolver_node(
    state: &CheckerState<'_>,
    method: EmitResolverMethod,
    node: EmitResolverNode,
) -> Result<(), EmitResolverError> {
    let source_token = AuthoritativeSourceToken(node.source().raw());
    let expected_program_index = if state.authoritative_source_index_by_token.is_empty() {
        let index = node.source().index();
        if index >= state.binder.file_count() {
            return Err(EmitResolverError::UnknownSource { method, node });
        }
        index
    } else {
        state
            .authoritative_source_index_by_token
            .get(&source_token)
            .copied()
            .ok_or(EmitResolverError::UnknownSource { method, node })?
    };
    let actual_program_index = state
        .binder
        .try_file_index_of_node(node.node())
        .ok_or(EmitResolverError::UnknownNode { method, node })?;
    if actual_program_index != expected_program_index {
        return Err(EmitResolverError::SourceNodeMismatch {
            method,
            node,
            actual_program_index,
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/emit/tests.rs"]
mod tests;
