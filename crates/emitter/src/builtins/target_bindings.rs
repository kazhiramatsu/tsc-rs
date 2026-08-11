//! Shared generated-binding identity and final name assignment for the target ladder.
//!
//! Target transforms allocate stable binding identities while they are building
//! syntax. The last transform that runs for a target then assigns printable
//! names from the completed ownership tree. Keeping those two operations apart
//! lets independent lowering passes compose without guessing which earlier
//! pass has already occupied `_a`, `_b`, and the remaining generated slots.

use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{for_each_child, NodeData, SyntaxKind};

use crate::{
    transform::GeneratedBindingId, TransformArena, TransformError, TransformNode,
    TransformSourceId, TransformationContext,
};

use super::generated_bindings::{
    AncestorBindingPolicy, GeneratedBindingOwner, GeneratedBindingScopes,
};

#[derive(Clone, Debug)]
pub(super) struct TargetBinding {
    id: GeneratedBindingId,
    provisional_name: String,
    numbered_base: Option<String>,
    preferred_base: Option<String>,
    reserve_in_nested_scopes: bool,
}

impl TargetBinding {
    pub(super) fn allocate(
        context: &mut TransformationContext,
        provisional_name: String,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            id: context.allocate_generated_binding_id()?,
            provisional_name,
            numbered_base: None,
            preferred_base: None,
            reserve_in_nested_scopes: false,
        })
    }

    pub(super) fn allocate_reserved_in_nested_scopes(
        context: &mut TransformationContext,
        provisional_name: String,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            id: context.allocate_generated_binding_id()?,
            provisional_name,
            numbered_base: None,
            preferred_base: None,
            reserve_in_nested_scopes: true,
        })
    }

    pub(super) fn allocate_numbered(
        context: &mut TransformationContext,
        numbered_base: String,
        provisional_name: String,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            id: context.allocate_generated_binding_id()?,
            provisional_name,
            numbered_base: Some(numbered_base),
            preferred_base: None,
            reserve_in_nested_scopes: false,
        })
    }

    pub(super) fn allocate_numbered_reserved_in_nested_scopes(
        context: &mut TransformationContext,
        numbered_base: String,
        provisional_name: String,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            id: context.allocate_generated_binding_id()?,
            provisional_name,
            numbered_base: Some(numbered_base),
            preferred_base: None,
            reserve_in_nested_scopes: true,
        })
    }

    pub(super) fn allocate_preferred_reserved_in_nested_scopes(
        context: &mut TransformationContext,
        preferred_base: String,
        provisional_name: String,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            id: context.allocate_generated_binding_id()?,
            provisional_name,
            numbered_base: None,
            preferred_base: Some(preferred_base),
            reserve_in_nested_scopes: true,
        })
    }

    pub(super) const fn id(&self) -> GeneratedBindingId {
        self.id
    }

    pub(super) fn provisional_name(&self) -> &str {
        &self.provisional_name
    }

    pub(super) fn numbered_base(&self) -> Option<&str> {
        self.numbered_base.as_deref()
    }

    pub(super) fn preferred_base(&self) -> Option<&str> {
        self.preferred_base.as_deref()
    }

    pub(super) const fn reserve_in_nested_scopes(&self) -> bool {
        self.reserve_in_nested_scopes
    }
}

#[derive(Clone, Debug)]
enum BindingNameEvent {
    EnterFunction,
    Identifier {
        node: TransformNode,
        binding: GeneratedBindingId,
        numbered_base: Option<String>,
        preferred_base: Option<String>,
        planned_name: String,
        reserve_in_nested_scopes: bool,
    },
    ExitFunction,
}

pub(super) fn collect_untagged_identifier_texts(
    arena: &TransformArena,
    source: TransformSourceId,
    root: TransformNode,
) -> Result<BTreeSet<String>, TransformError> {
    let syntax = arena.source(source)?.syntax();
    let mut names = BTreeSet::new();
    let mut stack = vec![root.node()];
    let mut seen = BTreeSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let node = TransformNode::new(source, id);
        let record = arena.node(node)?;
        if arena
            .metadata(node)
            .and_then(|metadata| metadata.generated_binding_id())
            .is_none()
        {
            if let NodeData::Identifier(data) = &record.data {
                names.insert(data.text.clone());
            }
        }
        for_each_child(&syntax.arena, record, |child| {
            stack.push(child);
            false
        });
    }
    Ok(names)
}

pub(super) fn finalize_generated_binding_names(
    context: &mut TransformationContext,
    source: TransformSourceId,
    root: TransformNode,
) -> Result<(), TransformError> {
    let mut events = Vec::new();
    collect_binding_name_events(context.arena(), source, root, true, &mut events)?;
    if events.is_empty() {
        return Ok(());
    }

    let reserved = collect_untagged_identifier_texts(context.arena(), source, root)?;
    let mut scopes = GeneratedBindingScopes::new(reserved, AncestorBindingPolicy::AllowShadow);
    let mut scope_stack = Vec::new();
    let mut assigned = BTreeMap::<GeneratedBindingId, String>::new();
    let mut node_names = BTreeMap::<TransformNode, String>::new();
    for event in events {
        match event {
            BindingNameEvent::EnterFunction => {
                scope_stack.push(scopes.enter(GeneratedBindingOwner::FunctionBody));
            }
            BindingNameEvent::Identifier {
                node,
                binding,
                numbered_base,
                preferred_base,
                planned_name,
                reserve_in_nested_scopes,
            } => {
                let name = assigned
                    .entry(binding)
                    .or_insert_with(|| match (numbered_base, preferred_base) {
                        (None, Some(base)) => scopes.allocate_planned_preferred_with_policy(
                            &base,
                            planned_name,
                            reserve_in_nested_scopes,
                        ),
                        (Some(base), None) if reserve_in_nested_scopes => scopes
                            .allocate_source_planned_numbered_with_policy(
                                &base,
                                planned_name,
                                true,
                            ),
                        (Some(base), None) => {
                            scopes.allocate_source_planned_numbered(&base, planned_name)
                        }
                        (None, None) if reserve_in_nested_scopes => {
                            scopes.allocate_temp_with_policy(true)
                        }
                        (None, None) => scopes.allocate_temp(),
                        (Some(_), Some(_)) => {
                            unreachable!("a target binding cannot be both numbered and preferred")
                        }
                    })
                    .clone();
                node_names.insert(node, name);
            }
            BindingNameEvent::ExitFunction => {
                let (previous, completed) =
                    scope_stack
                        .pop()
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::FunctionDeclaration,
                            field: "generated-binding function scope",
                        })?;
                let _ = scopes.exit(previous, completed);
            }
        }
    }
    if !scope_stack.is_empty() {
        return Err(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::SourceFile,
            field: "balanced generated-binding scopes",
        });
    }
    let _ = scopes.source_bindings();
    for (binding, name) in &assigned {
        context.record_generated_binding_name(*binding, name);
    }
    let arena = context.arena_mut()?;
    for (node, name) in node_names {
        arena.set_generated_identifier_text(node, &name)?;
    }
    Ok(())
}

pub(super) const fn is_function_scope_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ArrowFunction
            | SyntaxKind::Constructor
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::GetAccessor
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::SetAccessor
    )
}

fn collect_binding_name_events(
    arena: &TransformArena,
    source: TransformSourceId,
    node: TransformNode,
    scope_root: bool,
    events: &mut Vec<BindingNameEvent>,
) -> Result<(), TransformError> {
    let record = arena.node(node)?.clone();
    let enters_function = !scope_root && is_function_scope_kind(record.kind);
    if enters_function {
        events.push(BindingNameEvent::EnterFunction);
    }
    if let Some(binding) = arena
        .metadata(node)
        .and_then(|metadata| metadata.generated_binding_id())
    {
        let numbered_base = arena
            .metadata(node)
            .and_then(|metadata| metadata.generated_binding_base())
            .map(str::to_owned);
        let preferred_base = arena
            .metadata(node)
            .and_then(|metadata| metadata.generated_binding_preferred_base())
            .map(str::to_owned);
        let reserve_in_nested_scopes = arena
            .metadata(node)
            .is_some_and(|metadata| metadata.generated_binding_reserved_in_nested_scopes());
        let planned_name = match &record.data {
            NodeData::Identifier(data) => data.text.clone(),
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: record.kind,
                    field: "generated binding identifier",
                });
            }
        };
        events.push(BindingNameEvent::Identifier {
            node,
            binding,
            numbered_base,
            preferred_base,
            planned_name,
            reserve_in_nested_scopes,
        });
    }
    let syntax = arena.source(source)?.syntax();
    let mut children = Vec::new();
    for_each_child(&syntax.arena, &record, |child| {
        children.push(child);
        false
    });
    for child in children {
        collect_binding_name_events(
            arena,
            source,
            TransformNode::new(source, child),
            false,
            events,
        )?;
    }
    if enters_function {
        events.push(BindingNameEvent::ExitFunction);
    }
    Ok(())
}
