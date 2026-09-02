//! Shared generated-binding identity and final name assignment for the target ladder.
//!
//! Target transforms allocate stable binding identities while they are building
//! syntax. The last transform that runs for a target then assigns printable
//! names from the completed ownership tree. Keeping those two operations apart
//! lets independent lowering passes compose without guessing which earlier
//! pass has already occupied `_a`, `_b`, and the remaining generated slots.

use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{for_each_child, NodeData, NodeId, SyntaxKind};

use crate::{
    transform::GeneratedBindingId, EmitFlags, TransformArena, TransformError, TransformNode,
    TransformSourceId, TransformationContext,
};

use super::generated_bindings::{
    AncestorBindingPolicy, GeneratedBindingOwner, GeneratedBindingScopes,
};

/// The collision set consulted by an optimistic preferred generated name.
///
/// Ordinary optimistic names use the active name-generation scope and must be
/// distinct from generated peers. TypeScript's `FileLevel` names instead ask
/// only whether the spelling occurred in the parsed source (or in the global
/// name table supplied to its printer). Distinct generated identities may
/// therefore deliberately share one printable file-level name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PreferredNameDomain {
    ScopedOptimistic,
    FileLevelOptimistic,
}

/// Determines when an ordinary (`_a`, `_b`, ...) binding receives its final
/// spelling.
///
/// Most target transforms mirror tsc's printer-time name generation: the
/// completed output tree and its lexical scopes own the ordinal cursor. A
/// small set of legacy-decorator transaction temps instead carry semantic
/// allocation order across separately materialized declaration epochs, so
/// their planned spelling is authoritative when it remains collision-free.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OrdinaryTempNamePolicy {
    FinalizerTraversal,
    PlannedSpellingAuthoritative,
    /// tsc `createLoopVariable`: the finalize walk assigns the name per
    /// printer scope — the dedicated `_i` slot when free, the ordinary
    /// temp sequence otherwise.
    LoopVariable,
}

/// Immutable `SourceFile.identifiers` projection used by file-level names.
///
/// A transform arena also contains nodes appended by earlier passes. Filtering
/// by parse ownership is essential: those synthetic identifiers participate
/// in ordinary scoped name generation, but TypeScript's file-level predicate
/// deliberately ignores them. Candidate lookup never mutates this snapshot,
/// so two independently allocated file-level bindings can select the same
/// spelling while retaining distinct [`GeneratedBindingId`] values.
#[derive(Clone, Debug)]
pub(super) struct ParsedSourceIdentifierNames(BTreeSet<String>);

impl ParsedSourceIdentifierNames {
    pub(super) fn collect(
        arena: &TransformArena,
        source: TransformSourceId,
    ) -> Result<Self, TransformError> {
        let syntax = arena.source(source)?.syntax();
        let node_base = syntax.arena.node_base();
        let mut names = BTreeSet::new();
        for (offset, record) in syntax.arena.nodes().iter().enumerate() {
            let offset = u32::try_from(offset).expect("transform node count exceeds u32");
            let id = NodeId(
                node_base
                    .checked_add(offset)
                    .expect("transform node identity overflow"),
            );
            let node = TransformNode::new(source, id);
            if !arena.is_parsed_node(node)? {
                continue;
            }
            if let NodeData::Identifier(identifier) = &record.data {
                names.insert(identifier.text.clone());
            }
        }
        Ok(Self(names))
    }

    pub(super) fn contains(&self, name: &str) -> bool {
        self.0.contains(name)
    }

    pub(super) fn optimistic_candidate(&self, preferred: &str) -> String {
        if !self.0.contains(preferred) {
            return preferred.to_owned();
        }
        let base = if preferred.ends_with('_') {
            preferred.to_owned()
        } else {
            format!("{preferred}_")
        };
        let mut ordinal = 1usize;
        loop {
            let candidate = format!("{base}{ordinal}");
            if !self.0.contains(&candidate) {
                return candidate;
            }
            ordinal += 1;
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct TargetBinding {
    id: GeneratedBindingId,
    provisional_name: String,
    numbered_base: Option<String>,
    preferred_base: Option<String>,
    preferred_role_suffix: Option<String>,
    preferred_name_domain: Option<PreferredNameDomain>,
    ordinary_temp_name_policy: OrdinaryTempNamePolicy,
    reserve_in_nested_scopes: bool,
    derived_from: Option<GeneratedBindingId>,
}

impl TargetBinding {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn from_existing(
        id: GeneratedBindingId,
        provisional_name: String,
        numbered_base: Option<String>,
        preferred_base: Option<String>,
        preferred_role_suffix: Option<String>,
        file_level_optimistic: bool,
        planned_name_authoritative: bool,
        reserve_in_nested_scopes: bool,
    ) -> Self {
        debug_assert!(!file_level_optimistic || preferred_base.is_some());
        debug_assert!(
            !planned_name_authoritative
                || numbered_base.is_none()
                    && preferred_base.is_none()
                    && preferred_role_suffix.is_none()
        );
        let preferred_name_domain = preferred_base.as_ref().map(|_| {
            if file_level_optimistic {
                PreferredNameDomain::FileLevelOptimistic
            } else {
                PreferredNameDomain::ScopedOptimistic
            }
        });
        Self {
            id,
            provisional_name,
            numbered_base,
            preferred_base,
            preferred_role_suffix,
            preferred_name_domain,
            ordinary_temp_name_policy: if planned_name_authoritative {
                OrdinaryTempNamePolicy::PlannedSpellingAuthoritative
            } else {
                OrdinaryTempNamePolicy::FinalizerTraversal
            },
            reserve_in_nested_scopes,
            derived_from: None,
        }
    }

    pub(super) fn allocate(
        context: &mut TransformationContext,
        provisional_name: String,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            id: context.allocate_generated_binding_id()?,
            provisional_name,
            numbered_base: None,
            preferred_base: None,
            preferred_role_suffix: None,
            preferred_name_domain: None,
            ordinary_temp_name_policy: OrdinaryTempNamePolicy::FinalizerTraversal,
            reserve_in_nested_scopes: false,
            derived_from: None,
        })
    }

    pub(super) fn allocate_planned(
        context: &mut TransformationContext,
        provisional_name: String,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            id: context.allocate_generated_binding_id()?,
            provisional_name,
            numbered_base: None,
            preferred_base: None,
            preferred_role_suffix: None,
            preferred_name_domain: None,
            ordinary_temp_name_policy: OrdinaryTempNamePolicy::PlannedSpellingAuthoritative,
            reserve_in_nested_scopes: false,
            derived_from: None,
        })
    }

    /// tsc `createLoopVariable()`: the visit-time spelling is provisional;
    /// the finalize walk re-assigns per printer scope (H2.5h CA-2a C(i)).
    pub(super) fn allocate_planned_loop(
        context: &mut TransformationContext,
        provisional_name: String,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            id: context.allocate_generated_binding_id()?,
            provisional_name,
            numbered_base: None,
            preferred_base: None,
            preferred_role_suffix: None,
            preferred_name_domain: None,
            ordinary_temp_name_policy: OrdinaryTempNamePolicy::LoopVariable,
            reserve_in_nested_scopes: false,
            derived_from: None,
        })
    }

    /// `getGeneratedNameForNode` over another generated identifier: the
    /// ordinal appends to the BASE BINDING's finalized spelling (H2.5h
    /// CA-2a C(iii) residual); the recorded text base is the fallback when
    /// the finalize never assigned the base (planned-authoritative bases).
    pub(super) fn allocate_numbered_derived(
        context: &mut TransformationContext,
        base: &Self,
        provisional_name: String,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            id: context.allocate_generated_binding_id()?,
            provisional_name,
            numbered_base: Some(base.provisional_name.clone()),
            preferred_base: None,
            preferred_role_suffix: None,
            preferred_name_domain: None,
            ordinary_temp_name_policy: OrdinaryTempNamePolicy::FinalizerTraversal,
            reserve_in_nested_scopes: false,
            derived_from: Some(base.id),
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
            preferred_role_suffix: None,
            preferred_name_domain: None,
            ordinary_temp_name_policy: OrdinaryTempNamePolicy::FinalizerTraversal,
            reserve_in_nested_scopes: true,
            derived_from: None,
        })
    }

    pub(super) fn allocate_planned_reserved_in_nested_scopes(
        context: &mut TransformationContext,
        provisional_name: String,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            id: context.allocate_generated_binding_id()?,
            provisional_name,
            numbered_base: None,
            preferred_base: None,
            preferred_role_suffix: None,
            preferred_name_domain: None,
            ordinary_temp_name_policy: OrdinaryTempNamePolicy::PlannedSpellingAuthoritative,
            reserve_in_nested_scopes: true,
            derived_from: None,
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
            preferred_role_suffix: None,
            preferred_name_domain: None,
            ordinary_temp_name_policy: OrdinaryTempNamePolicy::FinalizerTraversal,
            reserve_in_nested_scopes: false,
            derived_from: None,
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
            preferred_role_suffix: None,
            preferred_name_domain: None,
            ordinary_temp_name_policy: OrdinaryTempNamePolicy::FinalizerTraversal,
            reserve_in_nested_scopes: true,
            derived_from: None,
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
            preferred_role_suffix: None,
            preferred_name_domain: Some(PreferredNameDomain::ScopedOptimistic),
            ordinary_temp_name_policy: OrdinaryTempNamePolicy::FinalizerTraversal,
            reserve_in_nested_scopes: true,
            derived_from: None,
        })
    }

    pub(super) fn allocate_file_level_optimistic_reserved_in_nested_scopes(
        context: &mut TransformationContext,
        preferred_base: String,
        provisional_name: String,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            id: context.allocate_generated_binding_id()?,
            provisional_name,
            numbered_base: None,
            preferred_base: Some(preferred_base),
            preferred_role_suffix: None,
            preferred_name_domain: Some(PreferredNameDomain::FileLevelOptimistic),
            ordinary_temp_name_policy: OrdinaryTempNamePolicy::FinalizerTraversal,
            reserve_in_nested_scopes: true,
            derived_from: None,
        })
    }

    pub(super) fn allocate_preferred_with_role_suffix_reserved_in_nested_scopes(
        context: &mut TransformationContext,
        preferred_base: String,
        preferred_role_suffix: String,
        provisional_name: String,
    ) -> Result<Self, TransformError> {
        Ok(Self {
            id: context.allocate_generated_binding_id()?,
            provisional_name,
            numbered_base: None,
            preferred_base: Some(preferred_base),
            preferred_role_suffix: Some(preferred_role_suffix),
            preferred_name_domain: Some(PreferredNameDomain::ScopedOptimistic),
            ordinary_temp_name_policy: OrdinaryTempNamePolicy::FinalizerTraversal,
            reserve_in_nested_scopes: true,
            derived_from: None,
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

    pub(super) const fn is_file_level_optimistic(&self) -> bool {
        matches!(
            self.preferred_name_domain,
            Some(PreferredNameDomain::FileLevelOptimistic)
        )
    }

    pub(super) fn printable_text<'context>(
        &'context self,
        context: &'context TransformationContext,
    ) -> &'context str {
        context
            .generated_binding_name(self.id)
            .unwrap_or(&self.provisional_name)
    }

    pub(super) fn write_generated_metadata(
        &self,
        arena: &mut TransformArena,
        identifier: TransformNode,
    ) {
        let metadata = arena.metadata_mut(identifier);
        metadata.set_generated_binding_id(self.id);
        if let Some(base) = &self.numbered_base {
            metadata.set_generated_binding_base(base);
        }
        if let Some(base) = &self.preferred_base {
            metadata.set_generated_binding_preferred_base(base);
        }
        if let Some(suffix) = &self.preferred_role_suffix {
            metadata.set_generated_binding_role_suffix(suffix);
        }
        if self.is_file_level_optimistic() {
            metadata.mark_generated_binding_file_level_optimistic();
        }
        if self.ordinary_temp_name_policy == OrdinaryTempNamePolicy::PlannedSpellingAuthoritative {
            metadata.mark_generated_binding_planned_name_authoritative();
        }
        if self.ordinary_temp_name_policy == OrdinaryTempNamePolicy::LoopVariable {
            metadata.mark_generated_binding_loop_variable();
        }
        if self.reserve_in_nested_scopes {
            metadata.reserve_generated_binding_in_nested_scopes();
        }
        if let Some(base) = self.derived_from {
            metadata.set_generated_binding_derived_from(base);
        }
    }
}

#[derive(Clone, Debug)]
enum BindingNameEvent {
    EnterScope(GeneratedBindingOwner),
    /// A naming-moment boundary WITHOUT a uniqueness scope: a
    /// `ReuseTempVariableScope` function expression continues the enclosing
    /// temp alphabet, but its body's generated names still resolve when its
    /// own emission pass runs (`generateNames` never descends into function
    /// expressions, `_tsc.js:120556-120574`), so source-numbered ordinals
    /// order after every enclosing-pass name (H2.5h CA-2a C(iii)).
    EnterNamingMoment,
    ExitNamingMoment,
    Identifier {
        node: TransformNode,
        binding: GeneratedBindingId,
        numbered_base: Option<String>,
        preferred_base: Option<String>,
        preferred_role_suffix: Option<String>,
        preferred_name_domain: Option<PreferredNameDomain>,
        ordinary_temp_name_policy: OrdinaryTempNamePolicy,
        planned_name: String,
        reserve_in_nested_scopes: bool,
        derived_from: Option<GeneratedBindingId>,
    },
    ExitScope,
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

fn allocate_ordinary_temp_name(
    scopes: &mut GeneratedBindingScopes,
    planned_name: String,
    reserve_in_nested_scopes: bool,
    policy: OrdinaryTempNamePolicy,
) -> String {
    match policy {
        OrdinaryTempNamePolicy::FinalizerTraversal => {
            scopes.allocate_temp_with_policy(reserve_in_nested_scopes)
        }
        OrdinaryTempNamePolicy::PlannedSpellingAuthoritative => {
            scopes.allocate_planned_temp_with_policy(planned_name, reserve_in_nested_scopes)
        }
        OrdinaryTempNamePolicy::LoopVariable => {
            let _ = planned_name;
            scopes.allocate_loop_variable(reserve_in_nested_scopes)
        }
    }
}

pub(crate) fn finalize_generated_binding_names(
    context: &mut TransformationContext,
    source: TransformSourceId,
    root: TransformNode,
) -> Result<(), TransformError> {
    finalize_generated_binding_names_with_policy(
        context,
        source,
        root,
        GeneratedNameReservedSetPolicy::TransformerRoot,
    )
}

#[derive(Clone, Copy, Debug)]
enum GeneratedNameReservedSetPolicy<'reserved> {
    TransformerRoot,
    PrintSource(&'reserved BTreeSet<String>),
}

fn finalize_generated_binding_names_with_policy(
    context: &mut TransformationContext,
    source: TransformSourceId,
    root: TransformNode,
    reserved_set_policy: GeneratedNameReservedSetPolicy<'_>,
) -> Result<(), TransformError> {
    let mut events = Vec::new();
    collect_binding_name_events(context.arena(), source, root, true, &mut events)?;
    if events.is_empty() {
        return Ok(());
    }

    let reserved = match reserved_set_policy {
        GeneratedNameReservedSetPolicy::TransformerRoot => {
            collect_untagged_identifier_texts(context.arena(), source, root)?
        }
        GeneratedNameReservedSetPolicy::PrintSource(reserved) => reserved.clone(),
    };
    let mut scopes = GeneratedBindingScopes::new(reserved, AncestorBindingPolicy::AllowShadow);
    let mut scope_stack = Vec::new();
    let mut assigned = BTreeMap::<GeneratedBindingId, String>::new();
    let mut node_names = BTreeMap::<TransformNode, String>::new();
    // Source-numbered assignment order: upstream names a scope's own
    // declarations in that scope's pass, parents before children
    // (`generateNames` does not descend into nested function bodies; a
    // nested scope's names are assigned when its emission pass runs), so
    // ordinals sort by (scope path, in-scope sequence) — the
    // renumber_state_bindings discipline generalized (H2.5h CA-2a
    // C(iii)). Phase 1 records the numbered events; phase 2 assigns in
    // that order before the per-node name map is written.
    struct NumberedAssignment {
        moment_path: Vec<u32>,
        sequence: usize,
        binding: GeneratedBindingId,
        base: String,
        reserve_in_nested_scopes: bool,
        derived_from: Option<GeneratedBindingId>,
    }
    let mut numbered_order: Vec<NumberedAssignment> = Vec::new();
    let mut scope_path: Vec<u32> = Vec::new();
    let mut scope_ordinal: u32 = 0;
    let mut sequence: usize = 0;
    for event in &events {
        match event {
            BindingNameEvent::EnterScope(_) | BindingNameEvent::EnterNamingMoment => {
                scope_ordinal += 1;
                scope_path.push(scope_ordinal);
            }
            BindingNameEvent::ExitScope | BindingNameEvent::ExitNamingMoment => {
                scope_path.pop();
            }
            BindingNameEvent::Identifier {
                binding,
                numbered_base: Some(base),
                preferred_base: None,
                preferred_role_suffix: None,
                preferred_name_domain: None,
                reserve_in_nested_scopes,
                derived_from,
                ..
            } => {
                if let Some(existing) = numbered_order
                    .iter_mut()
                    .find(|entry| entry.binding == *binding)
                {
                    // A re-materialized identifier (the Generators pass
                    // rebuilds hoisted declaration lists) may carry fuller
                    // metadata than the binding's first event; keep the
                    // derived-from link whichever event carries it.
                    if existing.derived_from.is_none() {
                        existing.derived_from = *derived_from;
                    }
                } else {
                    sequence += 1;
                    numbered_order.push(NumberedAssignment {
                        moment_path: scope_path.clone(),
                        sequence,
                        binding: *binding,
                        base: base.clone(),
                        reserve_in_nested_scopes: *reserve_in_nested_scopes,
                        derived_from: *derived_from,
                    });
                }
            }
            BindingNameEvent::Identifier { .. } => {}
        }
    }
    numbered_order.sort_by(|a, b| {
        a.moment_path
            .cmp(&b.moment_path)
            .then(a.sequence.cmp(&b.sequence))
    });
    for entry in &numbered_order {
        // A derived binding appends its ordinal to the base binding's
        // FINALIZED spelling (`getGeneratedNameForNode`); the recorded
        // text base covers bases outside the numbered family.
        let base = entry
            .derived_from
            .and_then(|parent| assigned.get(&parent).cloned())
            .unwrap_or_else(|| entry.base.clone());
        let name =
            scopes.allocate_source_numbered_with_policy(&base, entry.reserve_in_nested_scopes);
        assigned.insert(entry.binding, name);
    }
    for event in events {
        match event {
            BindingNameEvent::EnterNamingMoment | BindingNameEvent::ExitNamingMoment => {}
            BindingNameEvent::EnterScope(owner) => {
                scope_stack.push(scopes.enter(owner));
            }
            BindingNameEvent::Identifier {
                node,
                binding,
                numbered_base,
                preferred_base,
                preferred_role_suffix,
                preferred_name_domain,
                ordinary_temp_name_policy,
                planned_name,
                reserve_in_nested_scopes,
                derived_from: _,
            } => {
                let name = assigned
                    .entry(binding)
                    .or_insert_with(|| {
                        match (
                            numbered_base,
                            preferred_base,
                            preferred_role_suffix,
                            preferred_name_domain,
                        ) {
                            (
                                None,
                                Some(_),
                                None,
                                Some(PreferredNameDomain::FileLevelOptimistic),
                            ) => scopes.reserve_planned_file_level_optimistic_with_policy(
                                planned_name,
                                reserve_in_nested_scopes,
                            ),
                            (
                                None,
                                Some(base),
                                Some(role_suffix),
                                Some(PreferredNameDomain::ScopedOptimistic),
                            ) => scopes.allocate_planned_preferred_with_role_suffix_with_policy(
                                &base,
                                &role_suffix,
                                planned_name,
                                reserve_in_nested_scopes,
                            ),
                            (
                                None,
                                Some(base),
                                None,
                                Some(PreferredNameDomain::ScopedOptimistic),
                            ) => scopes.allocate_planned_preferred_with_policy(
                                &base,
                                planned_name,
                                reserve_in_nested_scopes,
                            ),
                            (Some(base), None, None, None) => {
                                // Pre-assigned in phase 2 (scope-pass
                                // order); reaching this arm means the
                                // binding escaped phase 1.
                                let _ = (&base, &planned_name);
                                unreachable!(
                                    "source-numbered binding missed the scope-pass assignment"
                                )
                            }
                            (None, None, None, None) => allocate_ordinary_temp_name(
                                &mut scopes,
                                planned_name,
                                reserve_in_nested_scopes,
                                ordinary_temp_name_policy,
                            ),
                            _ => unreachable!("invalid target generated-name policy"),
                        }
                    })
                    .clone();
                node_names.insert(node, name);
            }
            BindingNameEvent::ExitScope => {
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

impl TransformationContext {
    pub(crate) fn finalize_generated_binding_names_for_print(
        &mut self,
        root: TransformNode,
    ) -> Result<(), TransformError> {
        let source = root.source();
        let mut events = Vec::new();
        collect_binding_name_events(self.arena(), source, root, true, &mut events)?;
        let requires_print_finalization = events.iter().any(|event| {
            matches!(
                event,
                BindingNameEvent::Identifier { binding, .. }
                    if self.generated_binding_name(*binding).is_none()
                        || self.generated_binding_was_finalized_for_print(*binding)
            )
        });
        if !requires_print_finalization {
            return Ok(());
        }

        let reserved = ParsedSourceIdentifierNames::collect(self.arena(), source)?.0;
        finalize_generated_binding_names_with_policy(
            self,
            source,
            root,
            GeneratedNameReservedSetPolicy::PrintSource(&reserved),
        )?;
        for event in events {
            if let BindingNameEvent::Identifier { binding, .. } = event {
                self.record_print_finalized_generated_binding(binding);
            }
        }
        Ok(())
    }
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
    // `EmitFlags.ReuseTempVariableScope` (metadata.rs:40): tsc's
    // `createTempVariable` scope stack skips push/pop for flagged
    // function-likes, so their temps continue the enclosing alphabet. The
    // Generators machine stamps it on the `__generator` callback (the
    // upstream `build` does, `_tsc.js:109697-109725`); es2017's awaiter
    // body is the other producer. Skipping the scope special-case here
    // reproduces the upstream skip symmetrically
    // (`docs/design/greenfield/slices/h2-5h-b-b-3.md` §12.3).
    let reuses_enclosing_temp_scope = arena.metadata(node).is_some_and(|metadata| {
        metadata
            .flags()
            .contains(EmitFlags::REUSE_TEMP_VARIABLE_SCOPE)
    });
    if !scope_root
        && is_function_scope_kind(record.kind)
        && reuses_enclosing_temp_scope
        && record.kind != SyntaxKind::FunctionDeclaration
    {
        // A reuse-flagged FUNCTION DECLARATION inlines into the parent's
        // naming moment at its tree position (`_tsc.js:120568-120574`);
        // every other reuse-flagged function-like keeps the parent
        // uniqueness scope but delays its body's naming moment.
        let body = match &record.data {
            NodeData::ArrowFunction(data) => data.body,
            NodeData::Constructor(data) => data.body,
            NodeData::FunctionExpression(data) => data.body,
            NodeData::GetAccessor(data) => data.body,
            NodeData::MethodDeclaration(data) => data.body,
            NodeData::SetAccessor(data) => data.body,
            _ => None,
        };
        let syntax = arena.source(source)?.syntax();
        let mut surface = Vec::new();
        let mut scoped = Vec::new();
        for_each_child(&syntax.arena, &record, |child| {
            let child_node = TransformNode::new(source, child);
            if Some(child) == body
                || arena
                    .node(child_node)
                    .is_ok_and(|record| record.kind == SyntaxKind::Parameter)
            {
                scoped.push(child);
            } else {
                surface.push(child);
            }
            false
        });
        for child in surface {
            collect_binding_name_events(
                arena,
                source,
                TransformNode::new(source, child),
                false,
                events,
            )?;
        }
        events.push(BindingNameEvent::EnterNamingMoment);
        for child in scoped {
            collect_binding_name_events(
                arena,
                source,
                TransformNode::new(source, child),
                false,
                events,
            )?;
        }
        events.push(BindingNameEvent::ExitNamingMoment);
        return Ok(());
    }
    if !scope_root && is_function_scope_kind(record.kind) && !reuses_enclosing_temp_scope {
        // tsc changes its generated-name scope after visiting the
        // function-like declaration surface. In particular, a computed method
        // name belongs to the enclosing class-evaluation scope, while its
        // parameters and body share the function scope. A whole-node enter
        // would incorrectly let every method reuse the computed-name `_a`.
        let body = match &record.data {
            NodeData::ArrowFunction(data) => data.body,
            NodeData::Constructor(data) => data.body,
            NodeData::FunctionDeclaration(data) => data.body,
            NodeData::FunctionExpression(data) => data.body,
            NodeData::GetAccessor(data) => data.body,
            NodeData::MethodDeclaration(data) => data.body,
            NodeData::SetAccessor(data) => data.body,
            _ => None,
        };
        let syntax = arena.source(source)?.syntax();
        let mut surface = Vec::new();
        let mut scoped = Vec::new();
        for_each_child(&syntax.arena, &record, |child| {
            let child_node = TransformNode::new(source, child);
            if Some(child) == body
                || arena
                    .node(child_node)
                    .is_ok_and(|record| record.kind == SyntaxKind::Parameter)
            {
                scoped.push(child);
            } else {
                surface.push(child);
            }
            false
        });
        for child in surface {
            collect_binding_name_events(
                arena,
                source,
                TransformNode::new(source, child),
                false,
                events,
            )?;
        }
        events.push(BindingNameEvent::EnterScope(
            GeneratedBindingOwner::FunctionBody,
        ));
        for child in scoped {
            collect_binding_name_events(
                arena,
                source,
                TransformNode::new(source, child),
                false,
                events,
            )?;
        }
        events.push(BindingNameEvent::ExitScope);
        return Ok(());
    }

    if !scope_root && record.kind == SyntaxKind::ClassStaticBlockDeclaration {
        let body = match &record.data {
            NodeData::ClassStaticBlockDeclaration(data) => data.body,
            _ => None,
        };
        let syntax = arena.source(source)?.syntax();
        let mut surface = Vec::new();
        let mut scoped = Vec::new();
        for_each_child(&syntax.arena, &record, |child| {
            if Some(child) == body {
                scoped.push(child);
            } else {
                surface.push(child);
            }
            false
        });
        for child in surface {
            collect_binding_name_events(
                arena,
                source,
                TransformNode::new(source, child),
                false,
                events,
            )?;
        }
        events.push(BindingNameEvent::EnterScope(
            GeneratedBindingOwner::StaticEvaluation,
        ));
        for child in scoped {
            collect_binding_name_events(
                arena,
                source,
                TransformNode::new(source, child),
                false,
                events,
            )?;
        }
        events.push(BindingNameEvent::ExitScope);
        return Ok(());
    }
    // tsc-port: emitModuleBlock @6.0.3
    if !scope_root && record.kind == SyntaxKind::ModuleBlock {
        events.push(BindingNameEvent::EnterScope(
            GeneratedBindingOwner::FunctionBody,
        ));
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
        events.push(BindingNameEvent::ExitScope);
        return Ok(());
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
        let preferred_role_suffix = arena
            .metadata(node)
            .and_then(|metadata| metadata.generated_binding_role_suffix())
            .map(str::to_owned);
        let preferred_name_domain = preferred_base.as_ref().map(|_| {
            if arena
                .metadata(node)
                .is_some_and(|metadata| metadata.generated_binding_is_file_level_optimistic())
            {
                PreferredNameDomain::FileLevelOptimistic
            } else {
                PreferredNameDomain::ScopedOptimistic
            }
        });
        let ordinary_temp_name_policy = if arena
            .metadata(node)
            .is_some_and(|metadata| metadata.generated_binding_is_loop_variable())
        {
            OrdinaryTempNamePolicy::LoopVariable
        } else if arena
            .metadata(node)
            .is_some_and(|metadata| metadata.generated_binding_planned_name_is_authoritative())
        {
            OrdinaryTempNamePolicy::PlannedSpellingAuthoritative
        } else {
            OrdinaryTempNamePolicy::FinalizerTraversal
        };
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
        let derived_from = arena
            .metadata(node)
            .and_then(|metadata| metadata.generated_binding_derived_from());
        events.push(BindingNameEvent::Identifier {
            node,
            binding,
            numbered_base,
            preferred_base,
            preferred_role_suffix,
            preferred_name_domain,
            ordinary_temp_name_policy,
            planned_name,
            reserve_in_nested_scopes,
            derived_from,
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
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/target_bindings_tests.rs"]
mod tests;
