//! Links tables — the memo policy in one place (greenfield §4.3).
//!
//! Every slot is written ONCE; in-progress states are the explicit
//! `Resolving` value (mirroring tsc's resolving sentinels), never an
//! implicit absence. Speculative checking must not write links: every
//! write asserts the checker-wide speculation depth is zero (the
//! single rule that replaces the quiet/expr_type_cache pollution
//! family). M3 has no speculation yet — the assertion is the contract
//! future stages inherit.

use std::collections::HashMap;

use tsrs2_binder::SymbolId;
use tsrs2_syntax::NodeId;
use tsrs2_types::TypeId;

use crate::state::SignatureId;

/// One memo slot: Vacant → Resolving → Resolved, one transition each.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum LinkSlot<T> {
    #[default]
    Vacant,
    Resolving,
    Resolved(T),
}

impl<T: Clone> LinkSlot<T> {
    pub fn resolved(&self) -> Option<T> {
        match self {
            LinkSlot::Resolved(value) => Some(value.clone()),
            _ => None,
        }
    }

    pub fn is_resolving(&self) -> bool {
        matches!(self, LinkSlot::Resolving)
    }
}

/// tsc NodeLinks (getNodeLinks) — the per-node subset M3 consumes.
#[derive(Clone, Debug, Default)]
pub struct NodeLinks {
    /// tsc links.resolvedType (per-arm caching in the
    /// getTypeFromTypeNode workers).
    pub resolved_type: LinkSlot<TypeId>,
    /// tsc links.resolvedSignature (getSignatureFromDeclaration 59570).
    pub resolved_signature: LinkSlot<SignatureId>,
}

/// tsc SymbolLinks — the per-symbol subset M3 consumes.
#[derive(Clone, Debug, Default)]
pub struct SymbolLinks {
    /// tsc links.declaredType (getDeclaredTypeOfClassOrInterface 57381).
    pub declared_type: LinkSlot<TypeId>,
    /// tsc links.type (getTypeOfVariableOrParameterOrProperty 56633).
    pub type_of_symbol: LinkSlot<TypeId>,
}

/// Resolved-members store — tsc keeps these directly on the type
/// object (setStructuredTypeMembers); a side table keeps Type immutable
/// after interning.
#[derive(Clone, Debug, Default)]
pub struct TypeLinks {
    pub resolved_members: LinkSlot<crate::state::MembersId>,
}

#[derive(Debug, Default)]
pub struct LinksTables {
    node: HashMap<NodeId, NodeLinks>,
    symbol: HashMap<SymbolId, SymbolLinks>,
    ty: HashMap<TypeId, TypeLinks>,
}

impl LinksTables {
    pub fn node(&self, id: NodeId) -> NodeLinks {
        self.node.get(&id).cloned().unwrap_or_default()
    }

    pub fn symbol(&self, id: SymbolId) -> SymbolLinks {
        self.symbol.get(&id).cloned().unwrap_or_default()
    }

    pub fn ty(&self, id: TypeId) -> TypeLinks {
        self.ty.get(&id).cloned().unwrap_or_default()
    }

    fn assert_writable(speculation_depth: u32) {
        assert_eq!(
            speculation_depth, 0,
            "links writes are forbidden during speculation (greenfield §4.3)"
        );
    }

    fn write_slot<T: Clone + std::fmt::Debug>(slot: &mut LinkSlot<T>, next: LinkSlot<T>) {
        match (&*slot, &next) {
            (LinkSlot::Vacant, _) | (LinkSlot::Resolving, LinkSlot::Resolved(_)) => *slot = next,
            _ => panic!("links slot rewritten: {slot:?} -> {next:?}"),
        }
    }

    pub fn set_node_resolved_type(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: LinkSlot<TypeId>,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(&mut self.node.entry(id).or_default().resolved_type, value);
    }

    pub fn set_node_resolved_signature(
        &mut self,
        speculation_depth: u32,
        id: NodeId,
        value: LinkSlot<SignatureId>,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.node.entry(id).or_default().resolved_signature,
            value,
        );
    }

    pub fn set_symbol_declared_type(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: LinkSlot<TypeId>,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(&mut self.symbol.entry(id).or_default().declared_type, value);
    }

    pub fn set_symbol_type(
        &mut self,
        speculation_depth: u32,
        id: SymbolId,
        value: LinkSlot<TypeId>,
    ) {
        Self::assert_writable(speculation_depth);
        Self::write_slot(
            &mut self.symbol.entry(id).or_default().type_of_symbol,
            value,
        );
    }

    pub fn set_type_members(
        &mut self,
        speculation_depth: u32,
        id: TypeId,
        value: LinkSlot<crate::state::MembersId>,
    ) {
        Self::assert_writable(speculation_depth);
        let slot = &mut self.ty.entry(id).or_default().resolved_members;
        // setStructuredTypeMembers writes an empty table first as a
        // re-entrancy guard, then the real one (58333/58339) — allow
        // Resolved -> Resolved for that one tsc-shaped double write.
        match (&*slot, &value) {
            (LinkSlot::Resolved(_), LinkSlot::Resolved(_)) => *slot = value,
            _ => Self::write_slot(slot, value),
        }
    }
}
