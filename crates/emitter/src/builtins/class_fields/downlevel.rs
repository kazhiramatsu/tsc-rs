//! ES2021 class-element lowering.
//!
//! This module plans a class as retained members plus ordered instance and
//! static operations.  The representation keeps target policy out of the AST
//! walk and gives private storage and static-super aliases one ownership point.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use tsc_syntax::{
    for_each_child, try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId,
    SyntaxKind,
};
use tsc_types::{NodeCheckFlags, NodeFlags, ScriptTarget};

use crate::{
    factory::EmitHelperName,
    metadata::{ClassExpressionDeclarationOrigin, RelocatedTrailingCommentOwner},
    CommentRange, EmitFlags, EmitHelper, EmitResolver, EmitResolverNode, InternalEmitFlags,
    LexicalEnvironmentFlags, SourceMapRange, SourceRange, TransformArena, TransformError,
    TransformFlags, TransformNode, TransformNodeArray, TransformSourceId, TransformationContext,
};

use super::super::{
    flags_after_update,
    generated_bindings::{
        AncestorBindingPolicy, GeneratedBindingOwner, GeneratedBindingScopes, GeneratedBindings,
    },
    system::collect_identifier_texts,
    target_bindings::{finalize_generated_binding_names, TargetBinding},
};

const CLASS_PRIVATE_FIELD_GET_HELPER_TEXT: &str = r#"var __classPrivateFieldGet = (this && this.__classPrivateFieldGet) || function (receiver, state, kind, f) {
    if (kind === "a" && !f) throw new TypeError("Private accessor was defined without a getter");
    if (typeof state === "function" ? receiver !== state || !f : !state.has(receiver)) throw new TypeError("Cannot read private member from an object whose class did not declare it");
    return kind === "m" ? f : kind === "a" ? f.call(receiver) : f ? f.value : state.get(receiver);
};"#;

const CLASS_PRIVATE_FIELD_SET_HELPER_TEXT: &str = r#"var __classPrivateFieldSet = (this && this.__classPrivateFieldSet) || function (receiver, state, value, kind, f) {
    if (kind === "m") throw new TypeError("Private method is not writable");
    if (kind === "a" && !f) throw new TypeError("Private accessor was defined without a setter");
    if (typeof state === "function" ? receiver !== state || !f : !state.has(receiver)) throw new TypeError("Cannot write private member to an object whose class did not declare it");
    return (kind === "a" ? f.call(receiver, value) : f ? f.value = value : state.set(receiver, value)), value;
};"#;

const CLASS_PRIVATE_FIELD_IN_HELPER_TEXT: &str = r#"var __classPrivateFieldIn = (this && this.__classPrivateFieldIn) || function(state, receiver) {
    if (receiver === null || (typeof receiver !== "object" && typeof receiver !== "function")) throw new TypeError("Cannot use 'in' operator on non-object");
    return typeof state === "function" ? receiver === state : state.has(receiver);
};"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PublicFieldMode {
    Assignment,
    DefineProperty,
}

/// tsc-port: pushNameGenerationScope/generateNames @6.0.3
/// tsc-hash: f239cdd756ed9b0bea9db0fbbfe0101907185b8100950833d1e9b18ae02e7329
/// tsc-span: _tsc.js:120490-120665
#[derive(Clone, Debug)]
pub(super) enum ClassBinding {
    Existing(String),
    Generated(TargetBinding),
}

impl ClassBinding {
    fn existing(text: impl Into<String>) -> Self {
        Self::Existing(text.into())
    }

    fn planned_text(&self) -> &str {
        match self {
            Self::Existing(text) => text,
            Self::Generated(binding) => binding.provisional_name(),
        }
    }

    fn same_identity(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Existing(left), Self::Existing(right)) => left == right,
            (Self::Generated(left), Self::Generated(right)) => left.id() == right.id(),
            _ => false,
        }
    }

    pub(super) fn printable_text<'context>(
        &'context self,
        context: &'context TransformationContext,
    ) -> &'context str {
        match self {
            Self::Existing(text) => text,
            Self::Generated(binding) => context
                .generated_binding_name(binding.id())
                .unwrap_or_else(|| binding.provisional_name()),
        }
    }

    pub(super) fn write_generated_metadata(
        &self,
        arena: &mut TransformArena,
        identifier: TransformNode,
    ) {
        let Self::Generated(binding) = self else {
            return;
        };
        binding.write_generated_metadata(arena, identifier);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LexicalBindingOwner {
    Hoisted,
    CurrentLoop,
}

#[derive(Debug)]
struct PlannedTargetBinding {
    binding: TargetBinding,
    owner: LexicalBindingOwner,
}

#[derive(Debug, Default)]
struct ClassGeneratedBindings(Vec<PlannedTargetBinding>);

impl ClassGeneratedBindings {
    fn is_empty(&self) -> bool {
        !self.has_hoisted_declarations()
    }

    fn has_hoisted_declarations(&self) -> bool {
        self.0
            .iter()
            .any(|binding| binding.owner == LexicalBindingOwner::Hoisted)
    }

    fn bindings(&self) -> &[PlannedTargetBinding] {
        &self.0
    }

    fn hoisted_bindings(&self) -> impl Iterator<Item = &TargetBinding> {
        self.0
            .iter()
            .filter(|binding| binding.owner == LexicalBindingOwner::Hoisted)
            .map(|binding| &binding.binding)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FieldReceiver {
    Instance,
    Static,
}

#[derive(Clone)]
struct FieldOperation {
    original: TransformNode,
    receiver: FieldReceiver,
    name: NodeId,
    value: FieldValuePlan,
    range_static_expression_to_name: bool,
}

#[derive(Clone)]
enum FieldValuePlan {
    Declared {
        initializer: Option<NodeId>,
    },
    ParameterProperty {
        prefix: Option<NodeId>,
        local: TransformNode,
    },
}

impl FieldValuePlan {
    const fn has_runtime_value(&self) -> bool {
        match self {
            Self::Declared { initializer } => initializer.is_some(),
            Self::ParameterProperty { .. } => true,
        }
    }

    const fn is_parameter_property(&self) -> bool {
        matches!(self, Self::ParameterProperty { .. })
    }
}

struct PlannedPropertyName {
    name: NodeId,
    evaluation: Option<TransformNode>,
    assigned_class_name: Option<AssignedClassName>,
}

/// Runtime name supplied to `__setFunctionName` for an anonymous class whose
/// static elements are relocated by this pass. Literal property names own
/// their cooked text, while computed names own a distinct read of the
/// generated key binding shared with the property access. Keeping the
/// variants separate prevents a computed identifier from collapsing into its
/// source text.
#[derive(Clone)]
enum AssignedClassName {
    Literal(String),
    Evaluated(TransformNode),
}

#[derive(Clone)]
struct PrivateSlot {
    placement: PrivatePlacement,
    element: PrivateElement,
    is_valid: bool,
}

#[derive(Clone)]
enum PrivatePlacement {
    Instance { brand_name: ClassBinding },
    Static { class_alias: ClassBinding },
}

#[derive(Clone)]
enum PrivateElement {
    Field {
        value_name: ClassBinding,
    },
    Method {
        method_name: ClassBinding,
    },
    Accessor {
        getter_name: Option<ClassBinding>,
        setter_name: Option<ClassBinding>,
    },
}

impl PrivateSlot {
    fn is_static(&self) -> bool {
        matches!(self.placement, PrivatePlacement::Static { .. })
    }

    fn brand_name(&self) -> &ClassBinding {
        match &self.placement {
            PrivatePlacement::Instance { brand_name } => brand_name,
            PrivatePlacement::Static { class_alias } => class_alias,
        }
    }

    fn access_kind(&self) -> &'static str {
        match self.element {
            PrivateElement::Field { .. } => "f",
            PrivateElement::Method { .. } => "m",
            PrivateElement::Accessor { .. } => "a",
        }
    }

    fn getter_descriptor_name(&self) -> Option<&ClassBinding> {
        match &self.element {
            PrivateElement::Field { value_name } => self.is_static().then_some(value_name),
            PrivateElement::Method { method_name } => Some(method_name),
            PrivateElement::Accessor { getter_name, .. } => getter_name.as_ref(),
        }
    }

    fn setter_descriptor_name(&self) -> Option<&ClassBinding> {
        match &self.element {
            PrivateElement::Field { value_name } => self.is_static().then_some(value_name),
            PrivateElement::Method { .. } => None,
            PrivateElement::Accessor { setter_name, .. } => setter_name.as_ref(),
        }
    }

    fn field_value_name(&self) -> Option<&ClassBinding> {
        match &self.element {
            PrivateElement::Field { value_name } => Some(value_name),
            _ => None,
        }
    }
}

#[derive(Clone, Default)]
struct PrivateEnvironment {
    /// The slot visible to private-name references after the class declaration
    /// scan. tsc replaces this entry for an invalid duplicate, so declaration
    /// lowering must observe the last entry rather than its own allocation.
    effective_slots: BTreeMap<String, usize>,
    /// Slots own their data exactly once. The name table and declaration log
    /// refer to stable indices instead of cloning mutable accessor state.
    private_slots: Vec<PrivateSlot>,
    /// Every declaration in tsc's environment-scan order. Duplicate
    /// declarations still retain their slot allocation even though only the
    /// last index remains visible through `effective_slots`; a legal accessor
    /// pair shares one slot index.
    declarations: Vec<PrivateDeclarationSlot>,
    /// Native private names retained at ES2022 shadow transformed names in
    /// enclosing classes, but have no helper-backed slot of their own.
    untransformed_names: BTreeSet<String>,
    class_alias: Option<ClassBinding>,
    instance_brand: Option<ClassBinding>,
    static_receiver: Option<StaticReceiver>,
    static_super_policy: StaticSuperPolicy,
    super_alias: Option<ClassBinding>,
    is_legacy_decorated: bool,
}

#[derive(Clone)]
struct PrivateDeclarationSlot {
    declaration: TransformNode,
    slot_index: usize,
}

impl PrivateEnvironment {
    fn push_slot(&mut self, slot: PrivateSlot) -> usize {
        let index = self.private_slots.len();
        self.private_slots.push(slot);
        index
    }

    fn effective_slot(&self, name: &str) -> Option<&PrivateSlot> {
        self.effective_slots
            .get(name)
            .and_then(|index| self.private_slots.get(*index))
    }
}

#[derive(Clone)]
struct StaticBindings {
    receiver: StaticReceiver,
    super_alias: Option<ClassBinding>,
    super_policy: StaticSuperPolicy,
}

/// One resolved `super.name`/`super[key]` evaluation in a relocated static
/// initializer. Keeping the three runtime operands together makes reads,
/// writes, updates, calls, tags, and assignment-target wrappers share the
/// same evaluation-order rules instead of rediscovering aliases ad hoc.
struct StaticSuperAccess {
    super_alias: ClassBinding,
    class_receiver: ClassBinding,
    key: TransformNode,
}

enum StaticSuperAccessResolution {
    Bound(StaticSuperAccess),
    InvalidLegacyDecorated {
        class_receiver: Option<ClassBinding>,
    },
}

/// Runtime meaning of lexical `this` while a static initializer is emitted.
/// A legacy-decorated class can acquire a stable pre-decoration identity from
/// named evaluation, private-element facts, or an explicit class-this binding;
/// without that definition binding, recovery still uses `void 0` rather than
/// the replaceable publication variable.
#[derive(Clone)]
enum StaticReceiver {
    Bound(ClassBinding),
    InvalidLegacyDecorated,
}

/// Whether a relocated static `super` access may use the class-definition
/// receiver. Legacy decorators deliberately invalidate `super` even when a
/// separate class-definition binding makes lexical `this` available.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum StaticSuperPolicy {
    #[default]
    Available,
    InvalidLegacyDecorated,
}

/// Lexical ownership of `this`/`super` while class static initializers are
/// relocated. Arrow functions inherit the top frame. A nested class or an
/// ordinary function introduces a hard boundary, while a nested class's own
/// static initializer can install a new evaluation frame above that boundary.
#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
enum StaticBindingFrame {
    StaticEvaluation(Option<StaticBindings>),
    ClassBoundary,
    FunctionBoundary,
}

#[derive(Clone, Default)]
struct StaticBindingFrames {
    frames: Rc<RefCell<Vec<StaticBindingFrame>>>,
}

impl StaticBindingFrames {
    fn active(&self) -> Option<StaticBindings> {
        match self.frames.borrow().last() {
            Some(StaticBindingFrame::StaticEvaluation(bindings)) => bindings.clone(),
            Some(StaticBindingFrame::ClassBoundary | StaticBindingFrame::FunctionBoundary)
            | None => None,
        }
    }

    fn enter(&self, frame: StaticBindingFrame) -> StaticBindingFrameGuard {
        let depth = {
            let mut frames = self.frames.borrow_mut();
            let depth = frames.len();
            frames.push(frame);
            depth
        };
        StaticBindingFrameGuard {
            frames: Rc::clone(&self.frames),
            depth,
        }
    }

    /// Computed property names are evaluated in the class's enclosing lexical
    /// environment. This is the typed frame equivalent of tsc's
    /// `lexicalEnvironment.previous` switch in `onEmitNode`.
    fn enclosing_class_evaluation(&self) -> Option<StaticBindings> {
        let frames = self.frames.borrow();
        let mut crossed_class_boundary = false;
        for frame in frames.iter().rev() {
            match frame {
                StaticBindingFrame::ClassBoundary if !crossed_class_boundary => {
                    crossed_class_boundary = true;
                }
                StaticBindingFrame::StaticEvaluation(bindings) if crossed_class_boundary => {
                    return bindings.clone();
                }
                StaticBindingFrame::FunctionBoundary if crossed_class_boundary => return None,
                _ => {}
            }
        }
        None
    }
}

/// Owns one pushed frame. `Drop` restores the exact prior depth on success,
/// error, and panic, so recursive nested-class traversal cannot leak a static
/// receiver into a sibling subtree.
struct StaticBindingFrameGuard {
    frames: Rc<RefCell<Vec<StaticBindingFrame>>>,
    depth: usize,
}

impl Drop for StaticBindingFrameGuard {
    fn drop(&mut self) {
        let mut frames = self.frames.borrow_mut();
        debug_assert_eq!(frames.len(), self.depth + 1);
        frames.truncate(self.depth);
    }
}

#[derive(Clone, Default)]
struct LoopBindingScopes {
    scopes: Rc<RefCell<Vec<LoopBindingFrame>>>,
}

enum LoopBindingFrame {
    Iteration(Vec<TargetBinding>),
    FunctionBoundary,
}

impl LoopBindingScopes {
    fn enter_iteration(&self) -> LoopBindingScopeGuard {
        self.enter(LoopBindingFrame::Iteration(Vec::new()))
    }

    fn enter_function_boundary(&self) -> LoopBindingScopeGuard {
        self.enter(LoopBindingFrame::FunctionBoundary)
    }

    fn enter(&self, frame: LoopBindingFrame) -> LoopBindingScopeGuard {
        let depth = {
            let mut scopes = self.scopes.borrow_mut();
            let depth = scopes.len();
            scopes.push(frame);
            depth
        };
        LoopBindingScopeGuard {
            scopes: Rc::clone(&self.scopes),
            depth,
        }
    }

    fn add(&self, binding: TargetBinding) -> bool {
        let mut scopes = self.scopes.borrow_mut();
        let Some(LoopBindingFrame::Iteration(bindings)) = scopes.last_mut() else {
            return false;
        };
        bindings.push(binding);
        true
    }
}

struct LoopBindingScopeGuard {
    scopes: Rc<RefCell<Vec<LoopBindingFrame>>>,
    depth: usize,
}

impl LoopBindingScopeGuard {
    fn take(&self) -> Vec<TargetBinding> {
        let mut scopes = self.scopes.borrow_mut();
        let Some(LoopBindingFrame::Iteration(bindings)) = scopes.last_mut() else {
            panic!("iteration binding scope remains active");
        };
        std::mem::take(bindings)
    }
}

impl Drop for LoopBindingScopeGuard {
    fn drop(&mut self) {
        let mut scopes = self.scopes.borrow_mut();
        debug_assert_eq!(scopes.len(), self.depth + 1);
        scopes.truncate(self.depth);
    }
}

#[derive(Clone, Copy, Default)]
struct StaticLexicalFacts {
    contains_this: bool,
    contains_super: bool,
}

#[derive(Clone)]
struct PrivateFieldOperation {
    original: TransformNode,
    slot: PrivateSlot,
    initializer: Option<NodeId>,
}

#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
enum InstanceOperation {
    PrivateBrand(ClassBinding),
    Public(FieldOperation),
    PrivateField(PrivateFieldOperation),
}

#[derive(Clone)]
enum StaticOperation {
    Field(FieldOperation),
    PrivateField(Box<PrivateFieldOperation>),
    NamedEvaluation {
        original: Option<TransformNode>,
        expression: TransformNode,
    },
    Block {
        original: TransformNode,
        body: TransformNode,
    },
}

#[derive(Clone)]
struct PrivateDefinition {
    name: ClassBinding,
    function: TransformNode,
}

#[derive(Clone, Copy)]
enum PrivateDeclarationKind {
    Field,
    Method,
    Getter,
    Setter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PrivateGeneratedNameRole {
    Storage,
    Method,
    Getter,
    Setter,
}

impl PrivateGeneratedNameRole {
    const fn suffix(self) -> &'static str {
        match self {
            Self::Storage | Self::Method => "",
            Self::Getter => "_get",
            Self::Setter => "_set",
        }
    }
}

struct PrivateDeclaration {
    original: TransformNode,
    name: String,
    binding_owner: LexicalBindingOwner,
    is_static: bool,
    kind: PrivateDeclarationKind,
}

/// Side-effect-free declaration scan for one expanded private environment.
///
/// This plan deliberately owns generated private declarations, not class facts.
/// In particular, auto-accessor redirectors retain original-node provenance for
/// resolver queries, but must not make one source auto accessor appear to be
/// several ordinary private declarations while `getClassFacts` is reproduced.
struct PrivateEnvironmentPlan {
    declarations: Vec<PrivateDeclaration>,
    untransformed_names: BTreeSet<String>,
    instance_brand_owner: Option<LexicalBindingOwner>,
}

/// Source-member facts consumed by tsc's `getClassFacts` decision.
///
/// This plan is built before auto accessors expand. Its member categories are
/// therefore the source categories observed by tsc, while
/// `PrivateEnvironmentPlan` can independently own the expanded private slots.
#[derive(Clone, Copy, Default)]
struct ClassFactsPlan {
    static_facts: StaticLexicalFacts,
    has_static_private_or_auto_accessor: bool,
    has_instance_constructor_reference: bool,
}

/// Declaration owner selected once from the original class-expression
/// identity. Both the early semantic constructor identity and tsc's late
/// sequencing fallback must consume this same decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClassTempPlan {
    owner: LexicalBindingOwner,
}

impl ClassTempPlan {
    const HOISTED: Self = Self {
        owner: LexicalBindingOwner::Hoisted,
    };
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ClassConstructorReferencePlan {
    class_this_or_named_evaluation: bool,
    static_private_or_auto_accessor: bool,
    instance_constructor_reference: bool,
    static_lexical_this_or_super: bool,
}

impl ClassConstructorReferencePlan {
    fn from_class_facts(
        facts: &ClassFactsPlan,
        class_this_or_named_evaluation: bool,
        is_legacy_decorated: bool,
    ) -> Self {
        Self {
            class_this_or_named_evaluation,
            static_private_or_auto_accessor: facts.has_static_private_or_auto_accessor,
            instance_constructor_reference: facts.has_instance_constructor_reference,
            // ClassWasDecorated suppresses this fact in getClassFacts. Other
            // causes can still allocate a constructor identity, which static
            // `this` then consumes during substitution.
            static_lexical_this_or_super: !is_legacy_decorated
                && (facts.static_facts.contains_this || facts.static_facts.contains_super),
        }
    }

    const fn needs_identity(self) -> bool {
        self.class_this_or_named_evaluation
            || self.static_private_or_auto_accessor
            || self.instance_constructor_reference
            || self.static_lexical_this_or_super
    }
}

enum ClassPendingEntry {
    OrdinaryPrivateFieldStorage(PrivateSlot),
    InstanceBrand(ClassBinding),
    GeneratedAutoAccessorStorage(PrivateSlot),
    PrivateDefinition(PrivateDefinition),
    PublicFieldKeyOperand(TransformNode),
}

/// Ordered Rust ownership of tsc's class `pendingExpressions` channel.
///
/// The prefix invariant is ordinary private-field storage, then the instance
/// WeakSet brand, then generated auto-accessor storage. Source traversal may
/// only append private definitions and public-field key evaluations in arrival
/// order. A retained computed member drains the whole prefix accumulated so
/// far; no later materializer may regroup entries by their semantic kind.
/// Constructor-alias assignment is deliberately absent: tsc prepends it at
/// the class-declaration consumer after the member walk, rather than producing
/// it as part of the pending-expression stream.
#[derive(Default)]
struct ClassPendingPlan {
    entries: Vec<ClassPendingEntry>,
}

impl ClassPendingPlan {
    /// tsc-port: transformClassMembers @6.0.3
    /// tsc-hash: 8f02dc71f423a197caae79451edbed69e643ef5b909248bf13a649c2c2491071
    /// tsc-span: _tsc.js:97143-97237
    ///
    /// tsc-port: createBrandCheckWeakSetForPrivateMethods @6.0.3
    /// tsc-hash: 0f8e90657191cb048755f0d11736264b42f33ac8a3d774c92751fc37652d8677
    /// tsc-span: _tsc.js:97238-97252
    ///
    /// tsc-port: addPrivateIdentifierPropertyDeclarationToEnvironment @6.0.3
    /// tsc-hash: 76da1026f05a65b21788b59c156ca67e10008c6894a9989de475132bb70529ca
    /// tsc-span: _tsc.js:97678-97707
    fn from_setup_prefix(
        ordinary_private_storages: Vec<PrivateSlot>,
        instance_brand: Option<ClassBinding>,
        generated_auto_accessor_storages: Vec<PrivateSlot>,
    ) -> Self {
        let mut entries = Vec::with_capacity(
            ordinary_private_storages.len()
                + usize::from(instance_brand.is_some())
                + generated_auto_accessor_storages.len(),
        );
        entries.extend(
            ordinary_private_storages
                .into_iter()
                .map(ClassPendingEntry::OrdinaryPrivateFieldStorage),
        );
        entries.extend(instance_brand.map(ClassPendingEntry::InstanceBrand));
        entries.extend(
            generated_auto_accessor_storages
                .into_iter()
                .map(ClassPendingEntry::GeneratedAutoAccessorStorage),
        );
        Self { entries }
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// tsc-port: visitMethodOrAccessorDeclaration @6.0.3
    /// tsc-hash: da646c56cd8aaf5be0986400ea25fc55ac15f129de7aa3ea6cff07ec3749dcc9
    /// tsc-span: _tsc.js:96195-96225
    fn append_private_definition(&mut self, definition: PrivateDefinition) {
        self.entries
            .push(ClassPendingEntry::PrivateDefinition(definition));
    }

    /// tsc-port: transformPublicFieldInitializer @6.0.3
    /// tsc-hash: e72c55aad0e213de5657c51c1e4c95dfd15c8b6ae3ff8d229fddcfef20e43d72
    /// tsc-span: _tsc.js:96340-96376
    fn append_public_field_key_operands(
        &mut self,
        evaluations: impl IntoIterator<Item = TransformNode>,
    ) {
        self.entries.extend(
            evaluations
                .into_iter()
                .map(ClassPendingEntry::PublicFieldKeyOperand),
        );
    }

    fn take_entries(&mut self) -> Vec<ClassPendingEntry> {
        std::mem::take(&mut self.entries)
    }
}

#[derive(Default)]
struct ClassOperations {
    pending: ClassPendingPlan,
    retained_members: Vec<TransformNode>,
    instance: Vec<InstanceOperation>,
    static_: Vec<StaticOperation>,
}

#[derive(Debug)]
struct SuperStatementPath(Vec<usize>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClassHeritageSemantics {
    NoExtends,
    ExtendsNull,
    ExtendsValue,
}

impl ClassHeritageSemantics {
    const fn is_derived(self) -> bool {
        matches!(self, Self::ExtendsValue)
    }
}

struct StabilizedReceiver {
    read: TransformNode,
    initialized: Option<TransformNode>,
}

struct PrivateCallBinding {
    target: TransformNode,
    this_arg: TransformNode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExpressionValueUse {
    Required,
    Discarded,
}

/// Context owned by a destructuring assignment walk.
///
/// An assignment pattern is not an ordinary expression tree: private member
/// accesses in target position must become setter-backed assignment targets,
/// while receiver evaluations still happen before the assignment begins.
/// Keeping those prefix evaluations in a plan prevents the ordinary expression
/// visitor from accidentally turning a write target into a private-field read.
#[derive(Default)]
struct DestructuringAssignmentPlan {
    prefix_expressions: Vec<TransformNode>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParentOwnership {
    Unique(NodeId),
    Shared,
}

/// Parent links on synthesized nodes are intentionally absent. This snapshot
/// reconstructs ownership from the transform tree at the class-pass boundary,
/// keeping contextual decisions independent from mutable parser parent links.
#[derive(Debug, Default)]
struct OriginalTreeOwnership {
    parents: BTreeMap<NodeId, ParentOwnership>,
}

impl OriginalTreeOwnership {
    fn collect(
        arena: &TransformArena,
        source: TransformSourceId,
        root: NodeId,
    ) -> Result<Self, TransformError> {
        let mut ownership = Self::default();
        let mut pending = vec![root];
        let mut visited = BTreeSet::new();
        while let Some(parent) = pending.pop() {
            if !visited.insert(parent) {
                continue;
            }
            let parent_node = arena
                .node_ref(source, parent)
                .ok_or_else(|| TransformError::UnknownNode(TransformNode::new(source, parent)))?;
            let record = arena.node(parent_node)?.clone();
            let syntax = arena.source(source)?.syntax();
            let mut children = Vec::new();
            for_each_child(&syntax.arena, &record, |child| {
                children.push(child);
                false
            });
            for child in children {
                ownership
                    .parents
                    .entry(child)
                    .and_modify(|owner| {
                        if *owner != ParentOwnership::Unique(parent) {
                            *owner = ParentOwnership::Shared;
                        }
                    })
                    .or_insert(ParentOwnership::Unique(parent));
                pending.push(child);
            }
        }
        Ok(ownership)
    }

    fn unique_parent(&self, node: NodeId) -> Option<NodeId> {
        match self.parents.get(&node) {
            Some(ParentOwnership::Unique(parent)) => Some(*parent),
            Some(ParentOwnership::Shared) | None => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InlineSequencePlacement {
    ExistingListContext,
    RequiresParentheses,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StatementExpansionOwner(NodeId);

#[derive(Clone, Debug)]
struct DecoratedClassDeclarationExpansion {
    owner: StatementExpansionOwner,
    declaration: TransformNode,
    public_receiver: ClassBinding,
    initializer_receiver: Option<ClassBinding>,
}

pub(super) fn transform_source(
    context: &mut TransformationContext,
    source: TransformSourceId,
    resolver: &dyn EmitResolver,
    target: ScriptTarget,
    use_define_for_class_fields: bool,
    finalize_names: bool,
    class_aliases: &mut BTreeMap<(u32, u32), ClassBinding>,
) -> Result<(), TransformError> {
    let root = context.arena().root(source)?;
    let mode = if use_define_for_class_fields {
        PublicFieldMode::DefineProperty
    } else {
        PublicFieldMode::Assignment
    };
    let tree_ownership = OriginalTreeOwnership::collect(context.arena(), source, root.node())?;
    let mut visitor = DownlevelClassVisitor::new(
        context,
        source,
        resolver,
        target,
        mode,
        tree_ownership,
        class_aliases,
    );
    let transformed = visitor
        .visit(root.node())?
        .ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::SourceFile,
            field: "root",
        })?;
    let planned_source_bindings = visitor.generated_bindings.source_bindings();
    let source_bindings = ClassGeneratedBindings(
        visitor
            .generated_binding_frames
            .pop()
            .expect("class lowering owns one source binding frame"),
    );
    visitor.assert_generated_binding_plan(&planned_source_bindings, &source_bindings);
    let transformed = visitor
        .prepend_generated_declarations_to_source(visitor.node(transformed), source_bindings)?;
    if finalize_names {
        finalize_generated_binding_names(visitor.context, source, transformed)?;
    }
    visitor
        .context
        .arena_mut()?
        .replace_root(source, transformed)?;
    Ok(())
}

struct DownlevelClassVisitor<'context, 'resolver, 'aliases> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    resolver: &'resolver dyn EmitResolver,
    target: ScriptTarget,
    mode: PublicFieldMode,
    nodes: BTreeMap<NodeId, Option<NodeId>>,
    arrays: BTreeMap<NodeArrayId, Option<NodeArrayId>>,
    expanded_statements: BTreeMap<NodeId, Vec<NodeId>>,
    generated_bindings: GeneratedBindingScopes,
    generated_binding_frames: Vec<Vec<PlannedTargetBinding>>,
    private_environments: Vec<PrivateEnvironment>,
    static_binding_frames: StaticBindingFrames,
    loop_binding_scopes: LoopBindingScopes,
    generated_static_auto_accessors: BTreeSet<NodeId>,
    generated_auto_accessor_backings: BTreeSet<NodeId>,
    generated_auto_accessor_pairs: BTreeMap<NodeId, NodeId>,
    assigned_class_names: BTreeMap<NodeId, AssignedClassName>,
    tree_ownership: OriginalTreeOwnership,
    class_aliases: &'aliases mut BTreeMap<(u32, u32), ClassBinding>,
}

impl<'context, 'resolver, 'aliases> DownlevelClassVisitor<'context, 'resolver, 'aliases> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        resolver: &'resolver dyn EmitResolver,
        target: ScriptTarget,
        mode: PublicFieldMode,
        tree_ownership: OriginalTreeOwnership,
        class_aliases: &'aliases mut BTreeMap<(u32, u32), ClassBinding>,
    ) -> Self {
        Self {
            generated_bindings: GeneratedBindingScopes::new(
                collect_identifier_texts(context.arena(), source),
                AncestorBindingPolicy::Reserve,
            ),
            generated_binding_frames: vec![Vec::new()],
            context,
            source,
            resolver,
            target,
            mode,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
            expanded_statements: BTreeMap::new(),
            private_environments: Vec::new(),
            static_binding_frames: StaticBindingFrames::default(),
            loop_binding_scopes: LoopBindingScopes::default(),
            generated_static_auto_accessors: BTreeSet::new(),
            generated_auto_accessor_backings: BTreeSet::new(),
            generated_auto_accessor_pairs: BTreeMap::new(),
            assigned_class_names: BTreeMap::new(),
            tree_ownership,
            class_aliases,
        }
    }

    fn visit(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        self.visit_with_value_use(id, ExpressionValueUse::Required)
    }

    fn visit_discarded_value(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        self.visit_with_value_use(id, ExpressionValueUse::Discarded)
    }

    fn visit_with_value_use(
        &mut self,
        id: NodeId,
        value_use: ExpressionValueUse,
    ) -> Result<Option<NodeId>, TransformError> {
        if let Some(mapped) = self.nodes.get(&id) {
            return Ok(*mapped);
        }
        let original = self.node(id);
        let record = self.context.arena().node(original)?.clone();
        let kind = record.kind;
        let transformed = match record.data {
            NodeData::ClassDeclaration(data) => Some(self.visit_class_declaration(original, data)?),
            NodeData::ClassExpression(data) => Some(self.visit_class_expression(original, data)?),
            NodeData::PropertyAssignment(data) => {
                Some(self.visit_property_assignment(original, data)?)
            }
            NodeData::ComputedPropertyName(data) => {
                Some(self.visit_computed_property_name(original, data)?)
            }
            NodeData::PropertyAccessExpression(data) => {
                Some(self.visit_property_access(original, data)?)
            }
            NodeData::ElementAccessExpression(data) => {
                Some(self.visit_element_access(original, data)?)
            }
            NodeData::BinaryExpression(data) => {
                Some(self.visit_binary_expression(original, data, value_use)?)
            }
            NodeData::CallExpression(data) => Some(self.visit_call_expression(original, data)?),
            NodeData::TaggedTemplateExpression(data) => {
                Some(self.visit_tagged_template_expression(original, data)?)
            }
            NodeData::ExpressionStatement(data) => {
                Some(self.visit_expression_statement(original, data)?)
            }
            NodeData::ForStatement(data) => Some(self.visit_for_statement(original, data)?),
            NodeData::ForInStatement(data) => Some(self.visit_for_in_statement(original, data)?),
            NodeData::ForOfStatement(data) => Some(self.visit_for_of_statement(original, data)?),
            NodeData::WhileStatement(data) => Some(self.visit_while_statement(original, data)?),
            NodeData::DoStatement(data) => Some(self.visit_do_statement(original, data)?),
            NodeData::ParenthesizedExpression(data)
                if value_use == ExpressionValueUse::Discarded =>
            {
                Some(self.visit_parenthesized_expression(original, data, value_use)?)
            }
            NodeData::CommaListExpression(data) => {
                Some(self.visit_comma_list_expression(original, data, value_use)?)
            }
            NodeData::PrefixUnaryExpression(data) => {
                Some(self.visit_pre_or_postfix_unary_expression(
                    original,
                    data.operator,
                    data.operand,
                    true,
                    value_use,
                )?)
            }
            NodeData::PostfixUnaryExpression(data) => {
                Some(self.visit_pre_or_postfix_unary_expression(
                    original,
                    data.operator,
                    data.operand,
                    false,
                    value_use,
                )?)
            }
            NodeData::Parameter(data) => Some(self.visit_parameter(original, data)?),
            NodeData::FunctionDeclaration(data) => Some(self.visit_function_scope(
                original,
                NodeData::FunctionDeclaration(data),
                false,
            )?),
            NodeData::FunctionExpression(data) => Some(self.visit_function_scope(
                original,
                NodeData::FunctionExpression(data),
                false,
            )?),
            NodeData::ArrowFunction(data) => {
                Some(self.visit_function_scope(original, NodeData::ArrowFunction(data), true)?)
            }
            NodeData::MethodDeclaration(data) => Some(self.visit_function_scope(
                original,
                NodeData::MethodDeclaration(data),
                false,
            )?),
            NodeData::GetAccessor(data) => {
                Some(self.visit_function_scope(original, NodeData::GetAccessor(data), false)?)
            }
            NodeData::SetAccessor(data) => {
                Some(self.visit_function_scope(original, NodeData::SetAccessor(data), false)?)
            }
            NodeData::Constructor(data) => {
                Some(self.visit_function_scope(original, NodeData::Constructor(data), false)?)
            }
            NodeData::PrivateIdentifier(_) => Some(self.visit_private_identifier(original)?),
            NodeData::Token if kind == SyntaxKind::ThisKeyword => {
                if let Some(bindings) = self.static_binding_frames.active() {
                    Some(match bindings.receiver {
                        StaticReceiver::Bound(binding) => {
                            self.create_binding_identifier(&binding)?.node()
                        }
                        StaticReceiver::InvalidLegacyDecorated => self.create_void_zero()?.node(),
                    })
                } else {
                    Some(id)
                }
            }
            NodeData::Token => Some(id),
            data => Some(self.update_generic(original, data)?),
        };
        self.nodes.insert(id, transformed);
        Ok(transformed)
    }

    fn visit_function_scope(
        &mut self,
        original: TransformNode,
        data: NodeData,
        captures_static_bindings: bool,
    ) -> Result<NodeId, TransformError> {
        let _loop_binding_boundary = self.loop_binding_scopes.enter_function_boundary();
        let _static_binding_scope = (!captures_static_bindings).then(|| {
            self.static_binding_frames
                .enter(StaticBindingFrame::FunctionBoundary)
        });
        self.context.start_lexical_environment()?;
        let transformed =
            self.with_new_generated_scope(GeneratedBindingOwner::FunctionBody, |visitor| {
                let transformed = visitor.update_generic(original, data)?;
                let hoisted_in_parameters = visitor
                    .context
                    .lexical_environment_flags()
                    .contains(LexicalEnvironmentFlags::VARIABLES_HOISTED_IN_PARAMETERS);
                if hoisted_in_parameters && visitor.target >= ScriptTarget::ES2015 {
                    visitor.lower_function_parameter_defaults(visitor.node(transformed))
                } else {
                    Ok(transformed)
                }
            });
        let lexical_environment = self.context.end_lexical_environment();
        let (transformed, bindings) = transformed?;
        let lexical_environment = lexical_environment?;
        debug_assert!(lexical_environment.variable_declarations().is_empty());
        debug_assert!(lexical_environment.function_declarations().is_empty());
        self.install_function_bindings(
            self.node(transformed),
            bindings,
            lexical_environment.initialization_statements().to_vec(),
        )
        .map(TransformNode::node)
    }

    fn visit_expression_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ExpressionStatementData,
    ) -> Result<NodeId, TransformError> {
        data.expression = data
            .expression
            .map(|expression| self.visit_discarded_value(expression))
            .transpose()?
            .flatten();
        self.update_contextual_node(original, NodeData::ExpressionStatement(data))
            .map(TransformNode::node)
    }

    fn visit_for_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ForStatementData,
    ) -> Result<NodeId, TransformError> {
        data.initializer = data
            .initializer
            .map(|initializer| self.visit_discarded_value(initializer))
            .transpose()?
            .flatten();
        data.condition = self.visit_optional_node(data.condition)?;
        data.incrementor = data
            .incrementor
            .map(|incrementor| self.visit_discarded_value(incrementor))
            .transpose()?
            .flatten();
        data.statement = self.visit_iteration_body(data.statement, SyntaxKind::ForStatement)?;
        self.update_contextual_node(original, NodeData::ForStatement(data))
            .map(TransformNode::node)
    }

    fn visit_for_in_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ForInStatementData,
    ) -> Result<NodeId, TransformError> {
        data.initializer = self.visit_optional_node(data.initializer)?;
        data.expression = self.visit_optional_node(data.expression)?;
        data.statement = self.visit_iteration_body(data.statement, SyntaxKind::ForInStatement)?;
        self.update_contextual_node(original, NodeData::ForInStatement(data))
            .map(TransformNode::node)
    }

    fn visit_for_of_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ForOfStatementData,
    ) -> Result<NodeId, TransformError> {
        data.await_modifier = self.visit_optional_node(data.await_modifier)?;
        data.initializer = self.visit_optional_node(data.initializer)?;
        data.expression = self.visit_optional_node(data.expression)?;
        data.statement = self.visit_iteration_body(data.statement, SyntaxKind::ForOfStatement)?;
        self.update_contextual_node(original, NodeData::ForOfStatement(data))
            .map(TransformNode::node)
    }

    fn visit_while_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::WhileStatementData,
    ) -> Result<NodeId, TransformError> {
        data.expression = self.visit_optional_node(data.expression)?;
        data.statement = self.visit_iteration_body(data.statement, SyntaxKind::WhileStatement)?;
        self.update_contextual_node(original, NodeData::WhileStatement(data))
            .map(TransformNode::node)
    }

    fn visit_do_statement(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::DoStatementData,
    ) -> Result<NodeId, TransformError> {
        data.statement = self.visit_iteration_body(data.statement, SyntaxKind::DoStatement)?;
        data.expression = self.visit_optional_node(data.expression)?;
        self.update_contextual_node(original, NodeData::DoStatement(data))
            .map(TransformNode::node)
    }

    fn visit_iteration_body(
        &mut self,
        statement: Option<NodeId>,
        parent: SyntaxKind,
    ) -> Result<Option<NodeId>, TransformError> {
        let Some(statement) = statement else {
            return Ok(None);
        };
        let loop_scope = self.loop_binding_scopes.enter_iteration();
        let visited = self
            .visit(statement)?
            .map(|statement| self.node(statement))
            .ok_or(TransformError::RequiredChildRemoved {
                parent,
                field: "statement",
            })?;
        let bindings = loop_scope.take();
        drop(loop_scope);
        if bindings.is_empty() {
            return Ok(Some(visited.node()));
        }
        self.prepend_loop_binding_declarations(visited, bindings)
            .map(|statement| Some(statement.node()))
    }

    fn visit_parenthesized_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ParenthesizedExpressionData,
        value_use: ExpressionValueUse,
    ) -> Result<NodeId, TransformError> {
        data.expression = data
            .expression
            .map(|expression| match value_use {
                ExpressionValueUse::Required => self.visit(expression),
                ExpressionValueUse::Discarded => self.visit_discarded_value(expression),
            })
            .transpose()?
            .flatten();
        self.update_contextual_node(original, NodeData::ParenthesizedExpression(data))
            .map(TransformNode::node)
    }

    fn visit_comma_list_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::CommaListExpressionData,
        value_use: ExpressionValueUse,
    ) -> Result<NodeId, TransformError> {
        if let Some(elements) = data.elements {
            let original_elements = self.array(elements);
            let nodes = self
                .context
                .arena()
                .node_array(original_elements)?
                .nodes
                .clone();
            let length = nodes.len();
            let mut visited = Vec::with_capacity(length);
            for (index, element) in nodes.into_iter().enumerate() {
                let element = if value_use == ExpressionValueUse::Discarded || index + 1 < length {
                    self.visit_discarded_value(element)?
                } else {
                    self.visit(element)?
                };
                if let Some(element) = element {
                    visited.push(self.node(element));
                }
            }
            data.elements = Some(
                self.context
                    .factory()?
                    .update_node_array(original_elements, visited)?
                    .array(),
            );
        }
        self.update_contextual_node(original, NodeData::CommaListExpression(data))
            .map(TransformNode::node)
    }

    fn visit_pre_or_postfix_unary_expression(
        &mut self,
        original: TransformNode,
        operator: SyntaxKind,
        operand: Option<NodeId>,
        is_prefix: bool,
        value_use: ExpressionValueUse,
    ) -> Result<NodeId, TransformError> {
        let original_data = if is_prefix {
            NodeData::PrefixUnaryExpression(tsc_syntax::nodes::PrefixUnaryExpressionData {
                operator,
                operand,
            })
        } else {
            NodeData::PostfixUnaryExpression(tsc_syntax::nodes::PostfixUnaryExpressionData {
                operand,
                operator,
            })
        };
        if !matches!(
            operator,
            SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
        ) {
            return self.update_generic(original, original_data);
        }
        let operand = operand
            .map(|operand| self.skip_runtime_transparent_outer_expressions(self.node(operand)))
            .transpose()?
            .ok_or(TransformError::RequiredChildRemoved {
                parent: if is_prefix {
                    SyntaxKind::PrefixUnaryExpression
                } else {
                    SyntaxKind::PostfixUnaryExpression
                },
                field: "operand",
            })?;
        if let Some(access) = self.static_super_access(operand)? {
            return match access {
                StaticSuperAccessResolution::Bound(access) => {
                    self.lower_static_super_update(original, operator, is_prefix, value_use, access)
                }
                StaticSuperAccessResolution::InvalidLegacyDecorated { .. } => {
                    self.update_generic(original, original_data)
                }
            };
        }
        let Some((receiver, slot)) = self.private_access_target(Some(operand.node()))? else {
            return self.update_generic(original, original_data);
        };

        let receiver = self
            .visit(receiver.node())?
            .map(|receiver| self.node(receiver))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyAccessExpression,
                field: "expression",
            })?;
        let stabilized = self.stabilize_inline_receiver(receiver)?;
        let current = self.create_private_get(stabilized.read, &slot)?;
        let result_binding = (!is_prefix && value_use == ExpressionValueUse::Required)
            .then(|| self.allocate_shadowable_temp_name())
            .transpose()?;
        let update_binding = self.allocate_shadowable_temp_name()?;
        let update_target = self.create_binding_identifier(&update_binding)?;
        let mut expression = self.create_assignment(update_target, current)?;
        self.context
            .factory()?
            .set_text_range(expression, operand)?;

        let update_operand = self.create_binding_identifier(&update_binding)?;
        let mut operation = if is_prefix {
            self.context.factory()?.create_node(
                self.source,
                NodeData::PrefixUnaryExpression(tsc_syntax::nodes::PrefixUnaryExpressionData {
                    operator,
                    operand: Some(update_operand.node()),
                }),
                TransformFlags::NONE,
            )?
        } else {
            self.context.factory()?.create_node(
                self.source,
                NodeData::PostfixUnaryExpression(tsc_syntax::nodes::PostfixUnaryExpressionData {
                    operand: Some(update_operand.node()),
                    operator,
                }),
                TransformFlags::NONE,
            )?
        };
        self.context
            .factory()?
            .set_text_range(operation, original)?;

        if let Some(result_binding) = &result_binding {
            let result_target = self.create_binding_identifier(result_binding)?;
            operation = self.create_assignment(result_target, operation)?;
            self.context
                .factory()?
                .set_text_range(operation, original)?;
        }
        expression = self.inline_expressions(vec![expression, operation])?;
        self.context
            .factory()?
            .set_text_range(expression, original)?;
        if !is_prefix {
            let updated_value = self.create_binding_identifier(&update_binding)?;
            expression = self.inline_expressions(vec![expression, updated_value])?;
            self.context
                .factory()?
                .set_text_range(expression, original)?;
        }

        let assignment_receiver = stabilized.initialized.unwrap_or(stabilized.read);
        self.context
            .arena_mut()?
            .metadata_mut(expression)
            .add_flags(EmitFlags::NO_TRAILING_COMMENTS);
        let value = self.create_parenthesized(expression)?;
        expression = self.create_private_set(assignment_receiver, &slot, value)?;
        expression = self.set_original_and_range(expression, original)?;
        if let Some(result_binding) = &result_binding {
            let result = self.create_binding_identifier(result_binding)?;
            expression = self.inline_expressions(vec![expression, result])?;
            self.context
                .factory()?
                .set_text_range(expression, original)?;
        }
        Ok(expression.node())
    }

    fn visit_parameter(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ParameterData,
    ) -> Result<NodeId, TransformError> {
        let was_in_parameters = self
            .context
            .lexical_environment_flags()
            .contains(LexicalEnvironmentFlags::IN_PARAMETERS);
        self.context
            .set_lexical_environment_flags(LexicalEnvironmentFlags::IN_PARAMETERS, true)?;
        let transformed = self.update_generic(original, NodeData::Parameter(data));
        let restored = self.context.set_lexical_environment_flags(
            LexicalEnvironmentFlags::IN_PARAMETERS,
            was_in_parameters,
        );
        restored?;
        transformed
    }

    fn lower_function_parameter_defaults(
        &mut self,
        function: TransformNode,
    ) -> Result<NodeId, TransformError> {
        let record = self.context.arena().node(function)?.clone();
        let parameters = match &record.data {
            NodeData::FunctionDeclaration(data) => data.parameters,
            NodeData::FunctionExpression(data) => data.parameters,
            NodeData::ArrowFunction(data) => data.parameters,
            NodeData::MethodDeclaration(data) => data.parameters,
            NodeData::GetAccessor(data) => data.parameters,
            NodeData::SetAccessor(data) => data.parameters,
            NodeData::Constructor(data) => data.parameters,
            _ => None,
        };
        let Some(parameters) = parameters else {
            return Ok(function.node());
        };
        let original_parameters = self.array(parameters);
        let nodes = self
            .context
            .arena()
            .node_array(original_parameters)?
            .nodes
            .clone();
        let mut lowered = Vec::with_capacity(nodes.len());
        for parameter in nodes {
            lowered.push(self.lower_parameter_default(self.node(parameter))?);
        }
        let parameters = self
            .context
            .factory()?
            .update_node_array(original_parameters, lowered)?
            .array();
        let data = match record.data {
            NodeData::FunctionDeclaration(mut data) => {
                data.parameters = Some(parameters);
                NodeData::FunctionDeclaration(data)
            }
            NodeData::FunctionExpression(mut data) => {
                data.parameters = Some(parameters);
                NodeData::FunctionExpression(data)
            }
            NodeData::ArrowFunction(mut data) => {
                data.parameters = Some(parameters);
                NodeData::ArrowFunction(data)
            }
            NodeData::MethodDeclaration(mut data) => {
                data.parameters = Some(parameters);
                NodeData::MethodDeclaration(data)
            }
            NodeData::GetAccessor(mut data) => {
                data.parameters = Some(parameters);
                NodeData::GetAccessor(data)
            }
            NodeData::SetAccessor(mut data) => {
                data.parameters = Some(parameters);
                NodeData::SetAccessor(data)
            }
            NodeData::Constructor(mut data) => {
                data.parameters = Some(parameters);
                NodeData::Constructor(data)
            }
            _ => return Ok(function.node()),
        };
        let flags = flags_after_update(self.context.arena(), function, &data)?;
        Ok(self
            .context
            .factory()?
            .update_node(function, data, flags)?
            .node())
    }

    fn lower_parameter_default(
        &mut self,
        parameter: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::Parameter(mut data) = self.context.arena().node(parameter)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Parameter,
                field: "parameter data",
            });
        };
        if data.dot_dot_dot_token.is_some() {
            return Ok(parameter);
        }
        let name =
            data.name
                .map(|name| self.node(name))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Parameter,
                    field: "name",
                })?;
        let name_kind = self.context.arena().node(name)?.kind;
        if matches!(
            name_kind,
            SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
        ) {
            let alias = self.allocate_parameter_alias()?;
            let value = if let Some(initializer) = data.initializer.map(|id| self.node(id)) {
                let condition_name = self.create_binding_identifier(&alias)?;
                let condition = self.create_strict_undefined_check(condition_name)?;
                let fallback_name = self.create_binding_identifier(&alias)?;
                self.create_conditional(condition, initializer, fallback_name)?
            } else {
                self.create_binding_identifier(&alias)?
            };
            let declaration = self.create_variable_declaration(name, Some(value))?;
            let statement = self.create_variable_statement(vec![declaration], NodeFlags::NONE)?;
            self.context.add_initialization_statement(statement)?;
            let alias_name = self.create_binding_identifier(&alias)?;
            data.name = Some(alias_name.node());
            data.initializer = None;
        } else if let Some(initializer) = data.initializer.map(|id| self.node(id)) {
            let name_text = self
                .identifier_text(name)
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Parameter,
                    field: "identifier name",
                })?
                .to_owned();
            let condition_name = self.create_identifier(&name_text)?;
            let condition = self.create_strict_undefined_check(condition_name)?;
            let assignment_name = self.create_identifier(&name_text)?;
            let assignment = self.create_assignment(assignment_name, initializer)?;
            self.context
                .arena_mut()?
                .metadata_mut(assignment)
                .add_flags(EmitFlags::NO_COMMENTS | EmitFlags::NO_SOURCE_MAP);
            let statement = self.create_expression_statement(assignment)?;
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .add_flags(EmitFlags::NO_COMMENTS);
            let block = self.create_block(vec![statement], false)?;
            self.context.arena_mut()?.metadata_mut(block).add_flags(
                EmitFlags::SINGLE_LINE
                    | EmitFlags::NO_TRAILING_SOURCE_MAP
                    | EmitFlags::NO_TOKEN_SOURCE_MAPS
                    | EmitFlags::NO_COMMENTS,
            );
            let flags = self.context.arena().transform_flags(condition)
                | self.context.arena().transform_flags(block);
            let if_statement = self.context.factory()?.create_node(
                self.source,
                NodeData::IfStatement(tsc_syntax::nodes::IfStatementData {
                    expression: Some(condition.node()),
                    then_statement: Some(block.node()),
                    else_statement: None,
                }),
                flags,
            )?;
            self.context.add_initialization_statement(if_statement)?;
            data.initializer = None;
        }

        let updated_data = NodeData::Parameter(data);
        let flags = flags_after_update(self.context.arena(), parameter, &updated_data)?;
        self.context
            .factory()?
            .update_node(parameter, updated_data, flags)
    }

    fn allocate_parameter_alias(&mut self) -> Result<ClassBinding, TransformError> {
        let provisional = self.generated_bindings.allocate_local_temp();
        let binding = TargetBinding::allocate(self.context, provisional)?;
        Ok(ClassBinding::Generated(binding))
    }

    fn create_strict_undefined_check(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let undefined = self.create_void_zero()?;
        self.create_binary(expression, SyntaxKind::EqualsEqualsEqualsToken, undefined)
    }

    fn create_conditional(
        &mut self,
        condition: TransformNode,
        when_true: TransformNode,
        when_false: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let question = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::QuestionToken,
            TransformFlags::NONE,
        )?;
        let colon = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::ColonToken,
            TransformFlags::NONE,
        )?;
        let flags = self.context.arena().transform_flags(condition)
            | self.context.arena().transform_flags(when_true)
            | self.context.arena().transform_flags(when_false);
        self.context.factory()?.create_node(
            self.source,
            NodeData::ConditionalExpression(tsc_syntax::nodes::ConditionalExpressionData {
                condition: Some(condition.node()),
                question_token: Some(question.node()),
                when_true: Some(when_true.node()),
                colon_token: Some(colon.node()),
                when_false: Some(when_false.node()),
            }),
            flags,
        )
    }

    fn with_new_generated_scope<T>(
        &mut self,
        owner: GeneratedBindingOwner,
        operation: impl FnOnce(&mut Self) -> Result<T, TransformError>,
    ) -> Result<(T, ClassGeneratedBindings), TransformError> {
        let (previous, scope) = self.generated_bindings.enter(owner);
        self.generated_binding_frames.push(Vec::new());
        let result = operation(self);
        let planned_bindings = self.generated_bindings.exit(previous, scope);
        let bindings = ClassGeneratedBindings(
            self.generated_binding_frames
                .pop()
                .expect("nested class binding frame remains balanced"),
        );
        self.assert_generated_binding_plan(&planned_bindings, &bindings);
        result.map(|value| (value, bindings))
    }

    fn install_function_bindings(
        &mut self,
        function: TransformNode,
        bindings: ClassGeneratedBindings,
        initialization_statements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        if bindings.is_empty() && initialization_statements.is_empty() {
            return Ok(function);
        }
        let record = self.context.arena().node(function)?.clone();
        let body = match &record.data {
            NodeData::FunctionDeclaration(data) => data.body,
            NodeData::FunctionExpression(data) => data.body,
            NodeData::ArrowFunction(data) => data.body,
            NodeData::MethodDeclaration(data) => data.body,
            NodeData::GetAccessor(data) => data.body,
            NodeData::SetAccessor(data) => data.body,
            NodeData::Constructor(data) => data.body,
            _ => None,
        }
        .and_then(|body| self.context.arena().node_ref(self.source, body))
        .ok_or(TransformError::RequiredChildRemoved {
            parent: record.kind,
            field: "function body for generated bindings",
        })?;
        let body = if self.context.arena().node(body)?.kind == SyntaxKind::Block {
            self.prepend_function_prelude_to_block(body, bindings, initialization_statements)?
        } else if record.kind == SyntaxKind::ArrowFunction {
            let return_statement = self.context.factory()?.create_node(
                self.source,
                NodeData::ReturnStatement(tsc_syntax::nodes::ReturnStatementData {
                    expression: Some(body.node()),
                }),
                TransformFlags::NONE,
            )?;
            let body = self.create_block(vec![return_statement], false)?;
            self.prepend_function_prelude_to_block(body, bindings, initialization_statements)?
        } else {
            return Err(TransformError::RequiredChildRemoved {
                parent: record.kind,
                field: "block function body for generated bindings",
            });
        };
        let data = match record.data {
            NodeData::FunctionDeclaration(mut data) => {
                data.body = Some(body.node());
                NodeData::FunctionDeclaration(data)
            }
            NodeData::FunctionExpression(mut data) => {
                data.body = Some(body.node());
                NodeData::FunctionExpression(data)
            }
            NodeData::ArrowFunction(mut data) => {
                data.body = Some(body.node());
                NodeData::ArrowFunction(data)
            }
            NodeData::MethodDeclaration(mut data) => {
                data.body = Some(body.node());
                NodeData::MethodDeclaration(data)
            }
            NodeData::GetAccessor(mut data) => {
                data.body = Some(body.node());
                NodeData::GetAccessor(data)
            }
            NodeData::SetAccessor(mut data) => {
                data.body = Some(body.node());
                NodeData::SetAccessor(data)
            }
            NodeData::Constructor(mut data) => {
                data.body = Some(body.node());
                NodeData::Constructor(data)
            }
            _ => unreachable!("function scope is installed only on function-like nodes"),
        };
        let flags = flags_after_update(self.context.arena(), function, &data)?;
        self.context.factory()?.update_node(function, data, flags)
    }

    /// Rebuild a class-member list while preserving the two states consumed
    /// by the printer.
    ///
    /// A changed membership retains the parsed list range so comments beside
    /// an erased member remain owned by that source gap. An unchanged list is
    /// deliberately synthetic: this transformer still owns canonical
    /// re-emission of the class, and returning the parsed array would let the
    /// source-file fast path copy the entire class verbatim.
    fn rebuild_class_member_array(
        &mut self,
        original: Option<NodeArrayId>,
        members: Vec<TransformNode>,
    ) -> Result<TransformNodeArray, TransformError> {
        let Some(original) = original else {
            return self
                .context
                .factory()?
                .create_node_array(self.source, members);
        };
        let original = self.array(original);
        let membership_is_unchanged = {
            let original_nodes = &self.context.arena().node_array(original)?.nodes;
            original_nodes.len() == members.len()
                && original_nodes
                    .iter()
                    .zip(&members)
                    .all(|(original, member)| *original == member.node())
        };
        if membership_is_unchanged {
            self.context
                .factory()?
                .create_node_array(self.source, members)
        } else {
            self.context.factory()?.update_node_array(original, members)
        }
    }

    /// tsc-port: visitClassDeclarationInNewClassLexicalEnvironment @6.0.3
    /// tsc-hash: 07a4943badefc9b5d6d774a2d04dac4f3803e24852f8410d2bb735feef6fd6d7
    /// tsc-span: _tsc.js:96971-97045
    fn visit_class_declaration(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ClassDeclarationData,
    ) -> Result<NodeId, TransformError> {
        let _static_binding_scope = self
            .static_binding_frames
            .enter(StaticBindingFrame::ClassBoundary);
        let class_facts = self.scan_class_facts(data.members)?;
        data.members = self.expand_auto_accessors(data.members)?;
        let is_export_default = self.has_modifier(data.modifiers, SyntaxKind::ExportKeyword)?
            && self.has_modifier(data.modifiers, SyntaxKind::DefaultKeyword)?;
        if data.name.is_none()
            && is_export_default
            && self.class_has_transformable_static_member(data.members)?
        {
            let generated = self.allocate_declaration_name("default");
            data.name = Some(self.create_identifier(&generated)?.node());
        }
        let class_name = data
            .name
            .and_then(|name| self.identifier_text(self.node(name)).map(str::to_owned));
        let preferred_class_this = self
            .class_this_binding(original)
            .map(ClassBinding::existing);
        let heritage_semantics = self.class_heritage_semantics(data.heritage_clauses)?;
        let private_plan = self.scan_private_environment(data.members)?;
        let instance_brand = self.allocate_instance_brand(&private_plan, class_name.as_deref())?;
        let reference_plan = ClassConstructorReferencePlan::from_class_facts(
            &class_facts,
            preferred_class_this.is_some()
                || self.class_has_named_evaluation_member(data.members)?,
            false,
        );
        let class_alias = self.allocate_class_constructor_identity(
            reference_plan,
            preferred_class_this,
            ClassTempPlan::HOISTED,
        )?;
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.modifiers = self.filter_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?;
        let super_alias = self.allocate_super_base_binding(
            data.heritage_clauses,
            class_facts.static_facts.contains_super,
        )?;
        data.heritage_clauses = self.visit_optional_nodes(data.heritage_clauses)?;
        data.heritage_clauses =
            self.capture_super_base(data.heritage_clauses, super_alias.as_ref())?;
        let private_environment = self.materialize_private_environment(
            private_plan,
            class_name.as_deref(),
            class_alias,
            instance_brand,
            super_alias,
            false,
            class_facts.static_facts,
        )?;
        data.members = self.stabilize_auto_accessor_names(data.members)?;
        if !self.selectively_transforms_private_static_elements() {
            if let Some(alias) = private_environment.class_alias.as_ref() {
                self.register_class_alias(original, alias)?;
            }
        }
        self.private_environments.push(private_environment);

        let mut operations = self.plan_members(data.members)?;
        let has_trailing_operations = (!self.selectively_transforms_private_static_elements()
            && (!operations.pending.is_empty()
                || self
                    .private_environments
                    .last()
                    .is_some_and(|environment| environment.class_alias.is_some())))
            || !operations.static_.is_empty();
        let split_default_export = is_export_default && has_trailing_operations;
        if split_default_export {
            data.modifiers = self.filter_modifier(data.modifiers, SyntaxKind::ExportKeyword)?;
            data.modifiers = self.filter_modifier(data.modifiers, SyntaxKind::DefaultKeyword)?;
        }
        let mut retained = operations.retained_members;
        if !operations.instance.is_empty() {
            self.install_instance_operations(
                &mut retained,
                &operations.instance,
                heritage_semantics.is_derived(),
                class_name.as_deref(),
                original,
                data.members,
            )?;
        }
        self.install_private_static_pending_block(&mut retained, &mut operations.pending)?;
        let members = self.rebuild_class_member_array(data.members, retained)?;
        data.members = Some(members.array());
        let flags = flags_after_update(
            self.context.arena(),
            original,
            &NodeData::ClassDeclaration(data.clone()),
        )?;
        let class = self.context.factory()?.update_node(
            original,
            NodeData::ClassDeclaration(data),
            flags,
        )?;

        let private_environment = self
            .private_environments
            .pop()
            .expect("class private environment remains balanced");

        if has_trailing_operations {
            let binding = match &class_name {
                Some(class_name) => ClassBinding::existing(class_name.clone()),
                None => self.allocate_temp_name()?,
            };
            let mut trailing = Vec::new();
            if let Some(pending) = self.materialize_class_declaration_pending_statement(
                &mut operations.pending,
                private_environment.class_alias.as_ref(),
                &binding,
            )? {
                trailing.push(pending);
            }
            trailing.extend(self.materialize_static_operations(&binding, operations.static_)?);
            if split_default_export {
                let local_name =
                    class_name
                        .as_deref()
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::ClassDeclaration,
                            field: "default-export class local name",
                        })?;
                trailing.push(self.create_export_default(local_name)?);
            }
            self.expanded_statements.insert(
                class.node(),
                trailing.into_iter().map(TransformNode::node).collect(),
            );
        }
        Ok(class.node())
    }

    /// tsc-port: visitClassExpressionInNewClassLexicalEnvironment @6.0.3
    /// tsc-hash: 5885e805a286e1451a1c60771127ff84a6c108f88522eb2f90901c2703763319
    /// tsc-span: _tsc.js:97049-97129
    fn visit_class_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ClassExpressionData,
    ) -> Result<NodeId, TransformError> {
        let _static_binding_scope = self
            .static_binding_frames
            .enter(StaticBindingFrame::ClassBoundary);
        let class_temp_plan = self.class_temp_plan(original)?;
        let class_facts = self.scan_class_facts(data.members)?;
        let declared_class_name = data
            .name
            .and_then(|name| self.identifier_text(self.node(name)).map(str::to_owned));
        let decorated_declaration = self.decorated_class_declaration_expansion(original)?;
        let assigned_class_name = if declared_class_name.is_none() {
            // Explicit named-evaluation metadata (including evaluated
            // computed keys) is already the runtime name and stays
            // authoritative. The one exception is an anonymous default
            // legacy declaration: typed origin provenance owns its language
            // runtime name, while the enclosing variable is publication
            // plumbing only (`default_1`).
            if self.is_legacy_anonymous_default_class_expression(original)? {
                Some(AssignedClassName::Literal("default".to_owned()))
            } else {
                self.metadata_assigned_class_name(original)
                    .or(self.assigned_class_expression_name(original)?)
            }
        } else {
            None
        };
        let preferred_class_this = self
            .class_this_binding(original)
            .map(ClassBinding::existing);
        let has_transformable_static_member =
            self.class_has_transformable_static_member(data.members)?;
        let already_has_named_evaluation = self.class_has_named_evaluation_member(data.members)?;
        let needs_named_evaluation = !self.selectively_transforms_private_static_elements()
            && assigned_class_name.is_some()
            && has_transformable_static_member
            && !already_has_named_evaluation;
        // tsc injects a named-evaluation block before entering the private
        // environment. This pass emits that helper directly below, so its
        // pending assigned name represents the same cloned-class metadata.
        let class_name = if needs_named_evaluation {
            match assigned_class_name.as_ref() {
                Some(AssignedClassName::Literal(name))
                    if tsc_syntax::is_identifier_text_for_target(name, self.target) =>
                {
                    Some(name.clone())
                }
                _ => None,
            }
        } else {
            self.private_environment_class_name(original)?
        };
        data.members = self.expand_auto_accessors(data.members)?;
        let heritage_semantics = self.class_heritage_semantics(data.heritage_clauses)?;
        let private_plan = self.scan_private_environment(data.members)?;
        // tsc's private lexical environment owns this allocation before
        // getClassFacts creates a constructor identity.
        let instance_brand = self.allocate_instance_brand(&private_plan, class_name.as_deref())?;
        let reference_plan = ClassConstructorReferencePlan::from_class_facts(
            &class_facts,
            preferred_class_this.is_some()
                || needs_named_evaluation
                || already_has_named_evaluation,
            decorated_declaration.is_some(),
        );
        let class_definition_binding = self.allocate_class_constructor_identity(
            reference_plan,
            preferred_class_this,
            class_temp_plan,
        )?;
        // ClassWasDecorated suppresses NeedsClassSuperReference even when a
        // different fact allocated a usable constructor identity.
        let needs_super_reference =
            class_facts.static_facts.contains_super && decorated_declaration.is_none();
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        data.modifiers = self.filter_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?;
        let super_alias =
            self.allocate_super_base_binding(data.heritage_clauses, needs_super_reference)?;
        data.heritage_clauses = self.visit_optional_nodes(data.heritage_clauses)?;
        data.heritage_clauses =
            self.capture_super_base(data.heritage_clauses, super_alias.as_ref())?;
        let private_environment = self.materialize_private_environment(
            private_plan,
            class_name.as_deref(),
            class_definition_binding.clone(),
            instance_brand,
            super_alias,
            decorated_declaration.is_some(),
            class_facts.static_facts,
        )?;
        data.members = self.stabilize_auto_accessor_names(data.members)?;
        let private_expression_binding = private_environment.class_alias.clone();
        self.private_environments.push(private_environment);
        let mut operations = self.plan_members(data.members)?;
        let needs_expression_binding = (!self.selectively_transforms_private_static_elements()
            && (private_expression_binding.is_some() || !operations.pending.is_empty()))
            || !operations.static_.is_empty();
        // A semantic constructor identity was reserved before heritage/member
        // transforms. If getClassFacts did not require one, the ordinary
        // class-expression sequence temp is allocated only now, after member
        // keys and private declarations, matching createClassTempVar's second
        // call site in tsc.
        let expression_binding = match private_expression_binding
            .clone()
            .filter(|_| !self.selectively_transforms_private_static_elements())
        {
            Some(binding) => Some(binding),
            // A decorated declaration normally materializes static operations
            // against its public variable and needs no extra class-expression
            // temp. Named evaluation is different: tsc first injects a
            // `__setFunctionName(this, assignedName)` static block, promotes
            // that to NeedsClassConstructorReference, and then allocates a
            // temp even on the decorated-declaration path. Preserve that
            // class-definition identity separately from the public receiver.
            None if decorated_declaration.is_some() => None,
            None if needs_expression_binding => {
                Some(self.allocate_class_temp_name(class_temp_plan)?)
            }
            None => None,
        };
        let mut retained = operations.retained_members;
        if !operations.instance.is_empty() {
            self.install_instance_operations(
                &mut retained,
                &operations.instance,
                heritage_semantics.is_derived(),
                class_name.as_deref(),
                original,
                data.members,
            )?;
        }
        self.install_private_static_pending_block(&mut retained, &mut operations.pending)?;
        let members = self.rebuild_class_member_array(data.members, retained)?;
        data.members = Some(members.array());
        let flags = flags_after_update(
            self.context.arena(),
            original,
            &NodeData::ClassExpression(data.clone()),
        )?;
        let class = self.context.factory()?.update_node(
            original,
            NodeData::ClassExpression(data),
            flags,
        )?;
        let private_environment = self
            .private_environments
            .pop()
            .expect("class private environment remains balanced");

        // Named evaluation belongs to the same class-definition plan as
        // static fields. Legacy-decorator provenance can carry a runtime name
        // (`default`) that differs from its generated declaration binding
        // (`default_1`), so materialize the helper before choosing the
        // declaration or ordinary-expression placement below.
        if needs_named_evaluation {
            if let Some(assigned_name) = assigned_class_name.as_ref() {
                let binding =
                    expression_binding
                        .as_ref()
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::ClassExpression,
                            field: "named-evaluation class binding",
                        })?;
                let expression =
                    self.create_set_function_name_expression(binding, assigned_name)?;
                operations.static_.insert(
                    0,
                    StaticOperation::NamedEvaluation {
                        original: None,
                        expression,
                    },
                );
            }
        }
        if operations.pending.is_empty()
            && operations.static_.is_empty()
            && (private_environment.class_alias.is_none()
                || self.selectively_transforms_private_static_elements())
        {
            return Ok(class.node());
        }

        // transformClassFields treats the ClassExpression synthesized for a
        // decorated declaration as a declaration boundary. Its enclosing
        // variable statement keeps the class initializer, while field/setup
        // operations become following statements. This is the typed Rust
        // equivalent of tsc's ClassWasDecorated + pendingStatements channel.
        if let Some(decorated) = decorated_declaration {
            if let Some(binding) = decorated.initializer_receiver.as_ref() {
                self.register_class_alias(decorated.declaration, binding)?;
            }
            let initializer_binding = expression_binding.clone().filter(|binding| {
                !binding.same_identity(&decorated.public_receiver)
                    && !decorated
                        .initializer_receiver
                        .as_ref()
                        .is_some_and(|receiver| binding.same_identity(receiver))
            });
            let initializer = if let Some(binding) = initializer_binding.as_ref() {
                self.register_class_alias(decorated.declaration, binding)?;
                let target = self.create_binding_identifier(binding)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(class)
                    .mark_class_expression_alias_assigned();
                self.create_assignment(target, class)?
            } else {
                class
            };
            let mut trailing =
                self.materialize_class_pending_statements(&mut operations.pending)?;
            trailing.extend(
                self.materialize_static_operations(&decorated.public_receiver, operations.static_)?,
            );
            self.expanded_statements
                .entry(decorated.owner.0)
                .or_default()
                .extend(trailing.into_iter().map(TransformNode::node));
            return Ok(initializer.node());
        }
        let binding = expression_binding.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::ClassExpression,
            field: "lowered class expression binding",
        })?;
        self.register_class_alias(original, &binding)?;

        // Class expressions cannot expand their containing statement.  A
        // comma expression owns the temporary class value and every ordered
        // static operation, then yields the class binding.
        let target = self.create_binding_identifier(&binding)?;
        self.context
            .arena_mut()?
            .metadata_mut(class)
            .mark_class_expression_alias_assigned();
        let assign_class = self.create_assignment(target, class)?;
        if self.class_this_binding(original).is_some() {
            if let Some(owner) = self.variable_statement_expansion_owner(original)? {
                // tsc-port: visitClassExpressionInNewClassLexicalEnvironment @6.0.3
                // tsc-hash: 5885e805a286e1451a1c60771127ff84a6c108f88522eb2f90901c2703763319
                // tsc-span: _tsc.js:97049-97129
                //
                // A standard-decorator wrapper gives this class expression a
                // statement expansion owner. The class must be evaluated
                // before pending computed field names: retained computed
                // methods execute while evaluating the class expression,
                // whereas erased field keys become following statements.
                let initializer = self.set_original_and_range(assign_class, original)?;
                let mut trailing =
                    self.materialize_class_pending_statements(&mut operations.pending)?;
                trailing.extend(self.materialize_static_operations(&binding, operations.static_)?);
                self.expanded_statements
                    .entry(owner.0)
                    .or_default()
                    .extend(trailing.into_iter().map(TransformNode::node));
                return Ok(initializer.node());
            }
        }
        let mut expressions = vec![assign_class];
        expressions.extend(self.materialize_class_pending_expressions(&mut operations.pending)?);
        for statement in self.materialize_static_operations(&binding, operations.static_)? {
            let NodeData::ExpressionStatement(data) =
                self.context.arena().node(statement)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ClassExpression,
                    field: "static expression",
                });
            };
            if let Some(expression) = data.expression {
                let expression = self.node(expression);
                let original = self.context.arena().get_original_node(statement);
                let expression = if original != statement
                    && self.context.arena().node(original)?.kind == SyntaxKind::PropertyDeclaration
                {
                    // Class declarations retain the synthetic statement that
                    // owns a relocated field's comment range. A class
                    // expression unwraps that statement into its comma list,
                    // so move the same ownership onto the expression exactly
                    // as tsc's generateInitializedPropertyExpressions does.
                    let expression = self.set_original_and_range(expression, original)?;
                    // A comma-expression child does not pass through the
                    // statement/list leading-comment phase. This typed source
                    // anchor is the transform/printer equivalent of tsc's
                    // setCommentRange(expression, property): the node that is
                    // actually printed now owns the property's source trivia.
                    self.context
                        .arena_mut()?
                        .metadata_mut(expression)
                        .class_field_initializer_comment_source = Some(original);
                    expression
                } else {
                    expression
                };
                expressions.push(expression);
            }
        }
        expressions.push(self.create_binding_identifier(&binding)?);
        let expression = self.inline_class_expression(expressions, class, original)?;
        Ok(expression.node())
    }

    fn class_has_transformable_static_member(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for member in self.array_nodes(members)? {
            let record = self.context.arena().node(member)?;
            if self.selectively_transforms_private_static_elements() {
                let modifiers = match &record.data {
                    NodeData::PropertyDeclaration(data) => data.modifiers,
                    NodeData::MethodDeclaration(data) => data.modifiers,
                    NodeData::GetAccessor(data) => data.modifiers,
                    NodeData::SetAccessor(data) => data.modifiers,
                    _ => None,
                };
                if self.should_transform_private_class_element(member, modifiers)? {
                    return Ok(true);
                }
                continue;
            }
            match &record.data {
                NodeData::ClassStaticBlockDeclaration(_) => return Ok(true),
                NodeData::PropertyDeclaration(data)
                    if self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)?
                        && (data.initializer.is_some()
                            || self.name_is_private(data.name)?
                            || self
                                .has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?) =>
                {
                    return Ok(true);
                }
                _ => {}
            }
        }
        Ok(false)
    }

    fn selectively_transforms_private_static_elements(&self) -> bool {
        self.target >= ScriptTarget::ES2022
    }

    /// tsc-port: shouldTransformClassElementToWeakMap @6.0.3
    /// tsc-hash: fbed5640e00a4833b08f75371b47646671f0125dcb7d222e4efa6b858c2f0e6a
    /// tsc-span: _tsc.js:96185-96225
    fn should_transform_private_class_element(
        &self,
        member: TransformNode,
        modifiers: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        if !self.selectively_transforms_private_static_elements() {
            return Ok(true);
        }
        Ok(self.has_modifier(modifiers, SyntaxKind::StaticKeyword)?
            && self
                .context
                .arena()
                .metadata(member)
                .is_some_and(|metadata| {
                    metadata
                        .internal_flags()
                        .contains(InternalEmitFlags::TRANSFORM_PRIVATE_STATIC_ELEMENTS)
                }))
    }

    fn class_has_named_evaluation_member(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for member in self.array_nodes(members)? {
            if self.context.arena().node(member)?.kind == SyntaxKind::ClassStaticBlockDeclaration
                && self
                    .context
                    .arena()
                    .metadata(member)
                    .and_then(|metadata| metadata.assigned_name)
                    .is_some()
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn register_class_alias(
        &mut self,
        declaration: TransformNode,
        alias: &ClassBinding,
    ) -> Result<(), TransformError> {
        let declaration = self
            .context
            .arena()
            .require_parse_tree_resolver_node(declaration)?;
        self.class_aliases.insert(
            (declaration.source().raw(), declaration.node().0),
            alias.clone(),
        );
        Ok(())
    }

    /// Private storage uses getNameOfDeclaration on the current node. A clone
    /// has no parent, even when the runtime named-evaluation query can recover
    /// an enclosing assignment through the transform ownership index.
    fn private_environment_class_name(
        &self,
        class: TransformNode,
    ) -> Result<Option<String>, TransformError> {
        let arena = self.context.arena();
        let record = arena.node(class)?;
        let NodeData::ClassExpression(data) = &record.data else {
            return Ok(None);
        };
        let mut name = data.name;
        if name.is_none() {
            if let Some(parent) = record.parent {
                name = match &arena.node(self.node(parent))?.data {
                    NodeData::PropertyAssignment(data) => data.name,
                    NodeData::BindingElement(data) => data.name,
                    NodeData::VariableDeclaration(data) => data.name,
                    NodeData::BinaryExpression(data) if data.right == Some(class.node()) => {
                        match data.left {
                            Some(left) => match &arena.node(self.node(left))?.data {
                                NodeData::Identifier(_) => Some(left),
                                NodeData::PropertyAccessExpression(data) => data.name,
                                NodeData::ElementAccessExpression(data) => data.argument_expression,
                                _ => None,
                            },
                            None => None,
                        }
                    }
                    _ => None,
                };
            }
        }
        if let Some(name) = name.and_then(|name| self.identifier_text(self.node(name))) {
            return Ok(Some(name.to_owned()));
        }
        let Some(assigned_name) = arena.metadata(class).and_then(|data| data.assigned_name) else {
            return Ok(None);
        };
        let NodeData::StringLiteral(data) = &arena.node(assigned_name)?.data else {
            return Ok(None);
        };
        if let Some(name) = arena
            .metadata(assigned_name)
            .and_then(|data| data.string_literal_text_source)
            .and_then(|source| self.identifier_text(source))
        {
            return Ok(Some(name.to_owned()));
        }
        Ok(
            tsc_syntax::is_identifier_text_for_target(&data.text, self.target)
                .then(|| data.text.clone()),
        )
    }

    /// Resolve the named-evaluation identity of an anonymous class expression
    /// from the current transform tree. This is deliberately derived from the
    /// pass-owned ownership index rather than parser parent pointers, which
    /// may be stale after earlier transforms.
    fn assigned_class_expression_name(
        &self,
        class: TransformNode,
    ) -> Result<Option<AssignedClassName>, TransformError> {
        // Legacy decorator lowering turns an anonymous default declaration
        // into a class expression inside `let default_1 = ...`. The generated
        // local is publication plumbing, while native named evaluation would
        // have produced the runtime name `"default"`. Keep that name as a
        // derivable source fact; it does not by itself request a helper.
        if self.is_legacy_anonymous_default_class_expression(class)? {
            return Ok(Some(AssignedClassName::Literal("default".to_owned())));
        }
        let mut current = class.node();
        while let Some(parent) = self.tree_ownership.unique_parent(current) {
            let parent = self
                .context
                .arena()
                .node_ref(self.source, parent)
                .ok_or_else(|| TransformError::UnknownNode(self.node(parent)))?;
            let record = self.context.arena().node(parent)?;
            let outer_child = match &record.data {
                NodeData::ParenthesizedExpression(data) => data.expression,
                NodeData::PartiallyEmittedExpression(data) => data.expression,
                NodeData::TypeAssertionExpression(data) => data.expression,
                NodeData::AsExpression(data) => data.expression,
                NodeData::SatisfiesExpression(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                NodeData::ExpressionWithTypeArguments(data) => data.expression,
                _ => None,
            };
            if outer_child == Some(current) {
                current = parent.node();
                continue;
            }

            let assigned = match &record.data {
                NodeData::VariableDeclaration(data) if data.initializer == Some(current) => {
                    data.name.and_then(|name| self.assigned_name(name))
                }
                NodeData::Parameter(data) if data.initializer == Some(current) => {
                    data.name.and_then(|name| self.assigned_name(name))
                }
                NodeData::BindingElement(data) if data.initializer == Some(current) => {
                    data.name.and_then(|name| self.assigned_name(name))
                }
                NodeData::PropertyDeclaration(data) if data.initializer == Some(current) => {
                    data.name.and_then(|name| self.assigned_name(name))
                }
                NodeData::PropertyAssignment(data)
                    if data.initializer == Some(current)
                        && !self.is_proto_setter_name(data.name) =>
                {
                    data.name.and_then(|name| self.assigned_name(name))
                }
                NodeData::ShorthandPropertyAssignment(data)
                    if data.object_assignment_initializer == Some(current) =>
                {
                    data.name.and_then(|name| self.assigned_name(name))
                }
                NodeData::BinaryExpression(data) if data.right == Some(current) => {
                    let assignment = data
                        .operator_token
                        .and_then(|operator| self.context.arena().node_ref(self.source, operator))
                        .is_some_and(|operator| {
                            self.context.arena().node(operator).is_ok_and(|operator| {
                                matches!(
                                    operator.kind,
                                    SyntaxKind::EqualsToken
                                        | SyntaxKind::AmpersandAmpersandEqualsToken
                                        | SyntaxKind::BarBarEqualsToken
                                        | SyntaxKind::QuestionQuestionEqualsToken
                                )
                            })
                        });
                    assignment
                        .then(|| data.left.and_then(|left| self.assignment_target_name(left)))
                        .flatten()
                }
                NodeData::ExportAssignment(data) if data.expression == Some(current) => {
                    (!data.is_export_equals.unwrap_or(false))
                        .then(|| AssignedClassName::Literal("default".to_owned()))
                }
                _ => None,
            };
            return Ok(assigned);
        }
        Ok(None)
    }

    fn is_legacy_anonymous_default_class_expression(
        &self,
        class: TransformNode,
    ) -> Result<bool, TransformError> {
        let Some(ClassExpressionDeclarationOrigin::LegacyDecorated { declaration }) = self
            .context
            .arena()
            .metadata(class)
            .and_then(|metadata| metadata.class_expression_declaration_origin)
        else {
            return Ok(false);
        };
        // `transformTypeScript` can give the declaration a generated
        // publication name before legacy decorators synthesize this class
        // expression. The origin record deliberately points at that live
        // declaration node so resolver correlation remains valid; named
        // evaluation, however, is a source-language fact and must inspect the
        // end of its original chain.
        let declaration = self.context.arena().get_original_node(declaration);
        let NodeData::ClassDeclaration(data) = &self.context.arena().node(declaration)?.data else {
            return Ok(false);
        };
        Ok(data.name.is_none() && self.has_modifier(data.modifiers, SyntaxKind::DefaultKeyword)?)
    }

    fn assignment_target_name(&self, target: NodeId) -> Option<AssignedClassName> {
        match &self.context.arena().node(self.node(target)).ok()?.data {
            NodeData::Identifier(data) => Some(AssignedClassName::Literal(data.text.clone())),
            _ => None,
        }
    }

    fn assigned_name(&self, name: NodeId) -> Option<AssignedClassName> {
        self.assigned_class_names
            .get(&name)
            .cloned()
            .or_else(|| self.literal_assigned_name(name))
    }

    fn literal_assigned_name(&self, name: NodeId) -> Option<AssignedClassName> {
        match &self.context.arena().node(self.node(name)).ok()?.data {
            NodeData::Identifier(data) => Some(AssignedClassName::Literal(data.text.clone())),
            NodeData::PrivateIdentifier(data) => {
                Some(AssignedClassName::Literal(data.text.clone()))
            }
            NodeData::StringLiteral(data) => Some(AssignedClassName::Literal(data.text.clone())),
            NodeData::NoSubstitutionTemplateLiteral(data) => {
                Some(AssignedClassName::Literal(data.text.clone()))
            }
            NodeData::NumericLiteral(data) => Some(AssignedClassName::Literal(data.text.clone())),
            NodeData::ComputedPropertyName(data) => data
                .expression
                .and_then(|name| self.literal_assigned_name(name)),
            _ => None,
        }
    }

    fn is_proto_setter_name(&self, name: Option<NodeId>) -> bool {
        let Some(name) = name else {
            return false;
        };
        let Ok(name) = self.context.arena().node(self.node(name)) else {
            return false;
        };
        match &name.data {
            NodeData::Identifier(data) => data.text == "__proto__",
            NodeData::StringLiteral(data) => data.text == "__proto__",
            _ => false,
        }
    }

    fn metadata_assigned_class_name(&self, class: TransformNode) -> Option<AssignedClassName> {
        let assigned_name = self
            .context
            .arena()
            .metadata(class)
            .and_then(|metadata| metadata.assigned_name)?;
        match &self.context.arena().node(assigned_name).ok()?.data {
            NodeData::StringLiteral(data) => Some(AssignedClassName::Literal(data.text.clone())),
            NodeData::Identifier(_) => Some(AssignedClassName::Evaluated(assigned_name)),
            _ => None,
        }
    }

    fn class_this_binding(&self, class: TransformNode) -> Option<String> {
        let class_this = self
            .context
            .arena()
            .metadata(class)
            .and_then(|metadata| metadata.class_this)?;
        self.identifier_text(class_this).map(str::to_owned)
    }

    fn variable_statement_expansion_owner(
        &self,
        class: TransformNode,
    ) -> Result<Option<StatementExpansionOwner>, TransformError> {
        let mut current = class.node();
        loop {
            let Some(parent) = self.tree_ownership.unique_parent(current) else {
                return Ok(None);
            };
            let parent_node = self
                .context
                .arena()
                .node_ref(self.source, parent)
                .ok_or_else(|| TransformError::UnknownNode(self.node(parent)))?;
            let record = self.context.arena().node(parent_node)?;
            let outer_child = match &record.data {
                NodeData::ParenthesizedExpression(data) => data.expression,
                NodeData::PartiallyEmittedExpression(data) => data.expression,
                NodeData::TypeAssertionExpression(data) => data.expression,
                NodeData::AsExpression(data) => data.expression,
                NodeData::SatisfiesExpression(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                NodeData::ExpressionWithTypeArguments(data) => data.expression,
                _ => None,
            };
            if outer_child == Some(current) {
                current = parent;
                continue;
            }
            if let NodeData::BinaryExpression(data) = &record.data {
                let operator = data
                    .operator_token
                    .and_then(|operator| self.context.arena().node_ref(self.source, operator))
                    .map(|operator| self.context.arena().node(operator).map(|node| node.kind))
                    .transpose()?;
                if data.right == Some(current) && operator == Some(SyntaxKind::EqualsToken) {
                    current = parent;
                    continue;
                }
            }
            let NodeData::VariableDeclaration(data) = &record.data else {
                return Ok(None);
            };
            if data.initializer != Some(current) {
                return Ok(None);
            }
            let Some(list) = self.tree_ownership.unique_parent(parent) else {
                return Ok(None);
            };
            let list_node = self
                .context
                .arena()
                .node_ref(self.source, list)
                .ok_or_else(|| TransformError::UnknownNode(self.node(list)))?;
            if self.context.arena().node(list_node)?.kind != SyntaxKind::VariableDeclarationList {
                return Ok(None);
            }
            let Some(statement) = self.tree_ownership.unique_parent(list) else {
                return Ok(None);
            };
            let statement_node = self
                .context
                .arena()
                .node_ref(self.source, statement)
                .ok_or_else(|| TransformError::UnknownNode(self.node(statement)))?;
            return Ok((self.context.arena().node(statement_node)?.kind
                == SyntaxKind::VariableStatement)
                .then_some(StatementExpansionOwner(statement)));
        }
    }

    fn decorated_class_declaration_expansion(
        &self,
        class: TransformNode,
    ) -> Result<Option<DecoratedClassDeclarationExpansion>, TransformError> {
        let Some(ClassExpressionDeclarationOrigin::LegacyDecorated { declaration }) = self
            .context
            .arena()
            .metadata(class)
            .and_then(|metadata| metadata.class_expression_declaration_origin)
        else {
            return Ok(None);
        };
        let Some(owner) = self.variable_statement_expansion_owner(class)? else {
            return Ok(None);
        };
        let mut current = class.node();
        let mut initializer_receiver = None;
        while let Some(parent) = self.tree_ownership.unique_parent(current) {
            let parent_node = self
                .context
                .arena()
                .node_ref(self.source, parent)
                .ok_or_else(|| TransformError::UnknownNode(self.node(parent)))?;
            let record = self.context.arena().node(parent_node)?;
            let outer_child = match &record.data {
                NodeData::ParenthesizedExpression(data) => data.expression,
                NodeData::PartiallyEmittedExpression(data) => data.expression,
                NodeData::TypeAssertionExpression(data) => data.expression,
                NodeData::AsExpression(data) => data.expression,
                NodeData::SatisfiesExpression(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                _ => None,
            };
            if outer_child == Some(current) {
                current = parent;
                continue;
            }
            match &record.data {
                NodeData::BinaryExpression(data) if data.right == Some(current) => {
                    let operator = data
                        .operator_token
                        .and_then(|operator| self.context.arena().node_ref(self.source, operator))
                        .map(|operator| self.context.arena().node(operator).map(|node| node.kind))
                        .transpose()?;
                    if operator == Some(SyntaxKind::EqualsToken) {
                        if initializer_receiver.is_none() {
                            initializer_receiver = data
                                .left
                                .and_then(|left| self.identifier_text(self.node(left)))
                                .map(|name| ClassBinding::existing(name.to_owned()));
                        }
                        current = parent;
                        continue;
                    }
                    return Ok(None);
                }
                NodeData::VariableDeclaration(data) if data.initializer == Some(current) => {
                    let receiver = data
                        .name
                        .and_then(|name| self.identifier_text(self.node(name)))
                        .map(|name| ClassBinding::existing(name.to_owned()));
                    return Ok(receiver.map(|public_receiver| {
                        DecoratedClassDeclarationExpansion {
                            owner,
                            declaration,
                            public_receiver,
                            initializer_receiver,
                        }
                    }));
                }
                _ => return Ok(None),
            }
        }
        Ok(None)
    }

    fn inline_class_expression(
        &mut self,
        expressions: Vec<TransformNode>,
        class: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context
            .arena_mut()?
            .metadata_mut(class)
            .add_flags(EmitFlags::INDENTED);
        for expression in &expressions {
            self.context
                .arena_mut()?
                .metadata_mut(*expression)
                .set_starts_on_new_line(true);
        }
        let expression = self.inline_expressions(expressions)?;
        if self.inline_sequence_placement(original)? == InlineSequencePlacement::ExistingListContext
        {
            self.set_original_and_range(expression, original)
        } else {
            let parenthesized = self.create_parenthesized(expression)?;
            self.set_original_and_range(parenthesized, original)
        }
    }

    fn inline_sequence_placement(
        &self,
        original: TransformNode,
    ) -> Result<InlineSequencePlacement, TransformError> {
        let mut current = original.node();
        while let Some(parent) = self.tree_ownership.unique_parent(current) {
            let parent_node = self
                .context
                .arena()
                .node_ref(self.source, parent)
                .ok_or_else(|| TransformError::UnknownNode(self.node(parent)))?;
            let record = self.context.arena().node(parent_node)?;
            match &record.data {
                NodeData::ParenthesizedExpression(_)
                | NodeData::ReturnStatement(_)
                | NodeData::ArrowFunction(_) => {
                    return Ok(InlineSequencePlacement::ExistingListContext);
                }
                NodeData::PartiallyEmittedExpression(_) => current = parent,
                NodeData::BinaryExpression(data) => {
                    let operator = data
                        .operator_token
                        .and_then(|operator| self.context.arena().node_ref(self.source, operator))
                        .map(|operator| self.context.arena().node(operator).map(|node| node.kind))
                        .transpose()?;
                    if matches!(
                        operator,
                        Some(SyntaxKind::EqualsToken | SyntaxKind::CommaToken)
                    ) {
                        current = parent;
                    } else {
                        return Ok(InlineSequencePlacement::RequiresParentheses);
                    }
                }
                _ => return Ok(InlineSequencePlacement::RequiresParentheses),
            }
        }
        Ok(InlineSequencePlacement::RequiresParentheses)
    }

    fn expand_auto_accessors(
        &mut self,
        members: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(members) = members else {
            return Ok(None);
        };
        let original_array = self.array(members);
        let original_members = self.array_nodes(Some(members))?;
        let mut used_private_names = BTreeSet::new();
        for member in &original_members {
            let name = match &self.context.arena().node(*member)?.data {
                NodeData::PropertyDeclaration(data) => data.name,
                NodeData::MethodDeclaration(data) => data.name,
                NodeData::GetAccessor(data) => data.name,
                NodeData::SetAccessor(data) => data.name,
                _ => None,
            };
            let Some(name) = name.map(|name| self.node(name)) else {
                continue;
            };
            if let NodeData::PrivateIdentifier(data) = &self.context.arena().node(name)?.data {
                used_private_names.insert(data.text.trim_start_matches('#').to_owned());
            }
        }

        let mut expanded = Vec::with_capacity(original_members.len() + 4);
        for member in original_members {
            let NodeData::PropertyDeclaration(data) =
                self.context.arena().node(member)?.data.clone()
            else {
                expanded.push(member);
                continue;
            };
            if !self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)? {
                expanded.push(member);
                continue;
            }
            let name = data.name.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyDeclaration,
                field: "auto-accessor name",
            })?;
            let storage_name = match &self.context.arena().node(self.node(name))?.data {
                NodeData::Identifier(data) => self
                    .generated_bindings
                    .allocate_private_preferred_with_role_suffix(
                        data.text.trim_start_matches('#'),
                        "_accessor_storage",
                        &used_private_names,
                    ),
                NodeData::PrivateIdentifier(data) => self
                    .generated_bindings
                    .allocate_private_preferred_with_role_suffix(
                        data.text.trim_start_matches('#'),
                        "_accessor_storage",
                        &used_private_names,
                    ),
                _ => self
                    .generated_bindings
                    .allocate_private_temp_with_role_suffix(
                        "_accessor_storage",
                        &used_private_names,
                    ),
            };
            used_private_names.insert(storage_name.clone());
            let storage_text = format!("#{storage_name}");
            let storage =
                self.create_generated_private_identifier(&storage_text, self.node(name))?;
            let modifiers = self.filter_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?;
            let backing = self.context.factory()?.create_node(
                self.source,
                NodeData::PropertyDeclaration(tsc_syntax::nodes::PropertyDeclarationData {
                    name: Some(storage.node()),
                    modifiers,
                    question_token: None,
                    exclamation_token: None,
                    r#type: None,
                    initializer: data.initializer,
                }),
                TransformFlags::CONTAINS_CLASS_FIELDS,
            )?;
            self.generated_auto_accessor_backings.insert(backing.node());
            let getter = self.create_auto_accessor_getter(name, storage.node(), modifiers)?;
            let setter = self.create_auto_accessor_setter(name, storage.node(), modifiers)?;
            self.generated_auto_accessor_pairs
                .insert(getter.node(), setter.node());
            // transformAutoAccessor keeps all three nodes synthetic. Original
            // provenance and source maps are shared, but only the getter
            // receives the property's comment range. A text range on the
            // setter would become a second comment owner in ES2015 lowering.
            let arena = self.context.arena();
            let record = arena.node(member)?;
            let range = SourceRange::from_raw(
                record.pos,
                record.end,
                arena.source(member.source())?.syntax().positions(),
            )
            .map_err(|error| TransformError::InvalidSourceRange {
                node: member,
                error,
            })?;
            let metadata = arena.metadata(member);
            let source_map_range = metadata
                .and_then(crate::EmitMetadata::source_map_range)
                .unwrap_or_else(|| SourceMapRange::new(member.source(), range));
            let comment_range = metadata
                .and_then(crate::EmitMetadata::comment_range)
                .unwrap_or_else(|| CommentRange::new(member.source(), range));
            for generated in [backing, getter, setter] {
                let arena = self.context.arena_mut()?;
                arena.set_original_node(generated, Some(member))?;
                arena
                    .metadata_mut(generated)
                    .set_source_map_range(source_map_range);
            }
            self.context
                .arena_mut()?
                .metadata_mut(getter)
                .set_comment_range(comment_range);
            self.context
                .arena_mut()?
                .metadata_mut(backing)
                .add_flags(EmitFlags::NO_COMMENTS);
            self.context
                .arena_mut()?
                .metadata_mut(setter)
                .add_flags(EmitFlags::NO_COMMENTS);
            if self.has_modifier(modifiers, SyntaxKind::StaticKeyword)? {
                self.generated_static_auto_accessors.insert(getter.node());
                self.generated_static_auto_accessors.insert(setter.node());
            }
            expanded.extend([backing, getter, setter]);
        }
        Ok(Some(
            self.context
                .factory()?
                .update_node_array(original_array, expanded)?
                .array(),
        ))
    }

    /// Stabilize the shared name of each generated auto-accessor pair after
    /// the class lexical environment has allocated its receiver/super aliases.
    /// Upstream performs this in `transformAutoAccessor`: the getter owns the
    /// single key evaluation and the setter reads the same generated binding.
    /// Delaying allocation until this boundary also preserves generated-name
    /// order relative to the class alias selected by the private environment.
    fn stabilize_auto_accessor_names(
        &mut self,
        members: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(members) = members else {
            return Ok(None);
        };
        let original_array = self.array(members);
        let mut nodes = self.array_nodes(Some(members))?;
        let positions = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.node(), index))
            .collect::<BTreeMap<_, _>>();

        for getter_index in 0..nodes.len() {
            let getter = nodes[getter_index];
            let Some(setter_id) = self.generated_auto_accessor_pairs.remove(&getter.node()) else {
                continue;
            };
            let setter_index =
                positions
                    .get(&setter_id)
                    .copied()
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PropertyDeclaration,
                        field: "expanded auto-accessor setter",
                    })?;
            let setter = nodes[setter_index];
            let NodeData::GetAccessor(mut getter_data) =
                self.context.arena().node(getter)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PropertyDeclaration,
                    field: "expanded auto-accessor getter",
                });
            };
            let NodeData::SetAccessor(mut setter_data) =
                self.context.arena().node(setter)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PropertyDeclaration,
                    field: "expanded auto-accessor setter",
                });
            };
            let name = getter_data
                .name
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::GetAccessor,
                    field: "name",
                })?;
            let (getter_name, setter_name) = self.stabilize_auto_accessor_name(name)?;
            getter_data.name = Some(getter_name);
            setter_data.name = Some(setter_name);

            let getter_node_data = NodeData::GetAccessor(getter_data);
            let getter_flags = flags_after_update(self.context.arena(), getter, &getter_node_data)?;
            let updated_getter =
                self.context
                    .factory()?
                    .update_node(getter, getter_node_data, getter_flags)?;
            let setter_node_data = NodeData::SetAccessor(setter_data);
            let setter_flags = flags_after_update(self.context.arena(), setter, &setter_node_data)?;
            let updated_setter =
                self.context
                    .factory()?
                    .update_node(setter, setter_node_data, setter_flags)?;

            if self.generated_static_auto_accessors.remove(&getter.node()) {
                self.generated_static_auto_accessors
                    .insert(updated_getter.node());
            }
            if self.generated_static_auto_accessors.remove(&setter.node()) {
                self.generated_static_auto_accessors
                    .insert(updated_setter.node());
            }
            nodes[getter_index] = updated_getter;
            nodes[setter_index] = updated_setter;
        }

        Ok(Some(
            self.context
                .factory()?
                .update_node_array(original_array, nodes)?
                .array(),
        ))
    }

    /// tsc-port: transformAutoAccessor.computedNameBranch @6.0.3
    /// tsc-hash: bdc0c27f54ec58ea6649aab4bb4286cc80b06bc9c6adfdc8d47e5afbd0448b89
    /// tsc-span: _tsc.js:96256-96277
    fn stabilize_auto_accessor_name(
        &mut self,
        name: NodeId,
    ) -> Result<(NodeId, NodeId), TransformError> {
        let original = self.node(name);
        let NodeData::ComputedPropertyName(mut getter_data) =
            self.context.arena().node(original)?.data.clone()
        else {
            return Ok((name, name));
        };
        let expression = getter_data
            .expression
            .map(|expression| self.node(expression))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ComputedPropertyName,
                field: "auto-accessor name expression",
            })?;
        if self.is_simple_inlineable_expression(expression)? {
            return Ok((name, name));
        }

        let mut setter_data = getter_data.clone();
        if let Some(cached) = self.find_computed_property_name_cache(expression)? {
            setter_data.expression = Some(cached.node());
            let setter_name = self.update_computed_property_name(original, setter_data)?;
            return Ok((name, setter_name));
        }

        let temporary = self.allocate_temp_name()?;
        let target = self.create_binding_identifier(&temporary)?;
        let assignment = self.create_assignment(target, expression)?;
        self.context
            .factory()?
            .set_text_range(assignment, expression)?;
        getter_data.expression = Some(assignment.node());
        let getter_name = self.update_computed_property_name(original, getter_data)?;

        let read = self.create_binding_identifier(&temporary)?;
        setter_data.expression = Some(read.node());
        let setter_name = self.update_computed_property_name(original, setter_data)?;
        Ok((getter_name, setter_name))
    }

    /// tsc-port: findComputedPropertyNameCacheAssignment @6.0.3
    /// tsc-hash: 91427a44f29c8976dab2a9c759d08a91112da1672c014b01822fab049d558ecc
    /// tsc-span: _tsc.js:28193-28211
    fn find_computed_property_name_cache(
        &self,
        mut expression: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        loop {
            match self.context.arena().node(expression)?.data.clone() {
                NodeData::ParenthesizedExpression(data) => {
                    let Some(inner) = data.expression else {
                        return Ok(None);
                    };
                    expression = self.node(inner);
                }
                NodeData::PartiallyEmittedExpression(data) => {
                    let Some(inner) = data.expression else {
                        return Ok(None);
                    };
                    expression = self.node(inner);
                }
                NodeData::AsExpression(data) => {
                    let Some(inner) = data.expression else {
                        return Ok(None);
                    };
                    expression = self.node(inner);
                }
                NodeData::TypeAssertionExpression(data) => {
                    let Some(inner) = data.expression else {
                        return Ok(None);
                    };
                    expression = self.node(inner);
                }
                NodeData::NonNullExpression(data) => {
                    let Some(inner) = data.expression else {
                        return Ok(None);
                    };
                    expression = self.node(inner);
                }
                NodeData::SatisfiesExpression(data) => {
                    let Some(inner) = data.expression else {
                        return Ok(None);
                    };
                    expression = self.node(inner);
                }
                NodeData::CommaListExpression(data) => {
                    let Some(elements) = data.elements.and_then(|elements| {
                        self.context.arena().node_array_ref(self.source, elements)
                    }) else {
                        return Ok(None);
                    };
                    let Some(last) = self.context.arena().node_array(elements)?.nodes.last() else {
                        return Ok(None);
                    };
                    expression = self.node(*last);
                }
                NodeData::BinaryExpression(data) => {
                    let operator = data
                        .operator_token
                        .and_then(|operator| self.context.arena().node_ref(self.source, operator))
                        .map(|operator| self.context.arena().node(operator).map(|node| node.kind))
                        .transpose()?;
                    if operator == Some(SyntaxKind::CommaToken) {
                        let Some(right) = data.right else {
                            return Ok(None);
                        };
                        expression = self.node(right);
                        continue;
                    }
                    if operator != Some(SyntaxKind::EqualsToken) {
                        return Ok(None);
                    }
                    let Some(left) = data.left.map(|left| self.node(left)) else {
                        return Ok(None);
                    };
                    let generated = matches!(
                        &self.context.arena().node(left)?.data,
                        NodeData::Identifier(_)
                    ) && self
                        .context
                        .arena()
                        .metadata(left)
                        .and_then(|metadata| metadata.generated_binding_id())
                        .is_some();
                    return Ok(generated.then_some(left));
                }
                _ => return Ok(None),
            }
        }
    }

    fn create_auto_accessor_getter(
        &mut self,
        name: NodeId,
        storage: NodeId,
        modifiers: Option<NodeArrayId>,
    ) -> Result<TransformNode, TransformError> {
        let access = self.create_auto_accessor_storage_access(storage)?;
        let return_statement = self.context.factory()?.create_node(
            self.source,
            NodeData::ReturnStatement(tsc_syntax::nodes::ReturnStatementData {
                expression: Some(access.node()),
            }),
            TransformFlags::NONE,
        )?;
        let body = self.create_block(vec![return_statement], false)?;
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, Vec::new())?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::GetAccessor(tsc_syntax::nodes::GetAccessorData {
                name: Some(name),
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                body: Some(body.node()),
                modifiers,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_auto_accessor_setter(
        &mut self,
        name: NodeId,
        storage: NodeId,
        modifiers: Option<NodeArrayId>,
    ) -> Result<TransformNode, TransformError> {
        let value = self.create_identifier("value")?;
        let parameter = self.context.factory()?.create_node(
            self.source,
            NodeData::Parameter(tsc_syntax::nodes::ParameterData {
                name: Some(value.node()),
                modifiers: None,
                dot_dot_dot_token: None,
                question_token: None,
                r#type: None,
                initializer: None,
            }),
            TransformFlags::NONE,
        )?;
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, vec![parameter])?;
        let access = self.create_auto_accessor_storage_access(storage)?;
        let assignment = self.create_assignment(access, value)?;
        let statement = self.create_expression_statement(assignment)?;
        let body = self.create_block(vec![statement], false)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::SetAccessor(tsc_syntax::nodes::SetAccessorData {
                name: Some(name),
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                body: Some(body.node()),
                modifiers,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_auto_accessor_storage_access(
        &mut self,
        storage: NodeId,
    ) -> Result<TransformNode, TransformError> {
        let receiver = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::ThisKeyword,
            TransformFlags::CONTAINS_LEXICAL_THIS,
        )?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAccessExpression(tsc_syntax::nodes::PropertyAccessExpressionData {
                expression: Some(receiver.node()),
                question_dot_token: None,
                name: Some(storage),
            }),
            TransformFlags::CONTAINS_LEXICAL_THIS
                | TransformFlags::CONTAINS_PRIVATE_IDENTIFIER_IN_EXPRESSION,
        )
    }

    fn create_private_identifier(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::PrivateIdentifier(tsc_syntax::nodes::PrivateIdentifierData {
                escaped_text: text.to_owned(),
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
    }

    /// Create a generated private member name whose semantic identity remains
    /// the parsed member name that selected it.
    ///
    /// TypeScript's `getGeneratedPrivateNameForNode` assigns `name.original`
    /// even though the generated name has a synthetic text range. The checker
    /// resolver follows that original chain for facts such as
    /// `BlockScopedBindingInLoop`; copying only the enclosing property's range
    /// would leave the generated `NodeId` outside the immutable parse lease.
    fn create_generated_private_identifier(
        &mut self,
        text: &str,
        original_name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_private_identifier(text)?;
        self.context
            .arena_mut()?
            .set_original_node(name, Some(original_name))?;
        Ok(name)
    }

    fn filter_modifier(
        &mut self,
        modifiers: Option<NodeArrayId>,
        excluded: SyntaxKind,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(modifiers) = modifiers else {
            return Ok(None);
        };
        let original = self.array(modifiers);
        let retained = self
            .array_nodes(Some(modifiers))?
            .into_iter()
            .filter(|modifier| {
                self.context
                    .arena()
                    .node(*modifier)
                    .is_ok_and(|modifier| modifier.kind != excluded)
            })
            .collect();
        Ok(Some(
            self.context
                .factory()?
                .update_node_array(original, retained)?
                .array(),
        ))
    }

    /// tsc-port: getClassFacts @6.0.3
    /// tsc-hash: 18ea59522a3e87f378c8b5682c5eb2172be55cba02380fc3b240acbf0f4dd388
    /// tsc-span: _tsc.js:96844-96898
    ///
    /// Reproduce the member-category portion of `getClassFacts` from the source
    /// class shape. This must run before `expand_auto_accessors`: generated
    /// redirectors intentionally point back to their source property for other
    /// resolver facts, so querying them here would multiply one auto accessor
    /// into several ordinary private declarations.
    fn scan_class_facts(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<ClassFactsPlan, TransformError> {
        let static_facts = self.static_lexical_facts(members)?;
        let mut has_static_private_or_auto_accessor =
            self.class_has_named_evaluation_member(members)?;
        let mut has_instance_constructor_reference = false;

        for member in self.array_nodes(members)? {
            let record = self.context.arena().node(member)?;
            let (name, modifiers, is_auto_accessor) = match &record.data {
                NodeData::PropertyDeclaration(data) => (
                    data.name,
                    data.modifiers,
                    self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?,
                ),
                NodeData::MethodDeclaration(data) => (data.name, data.modifiers, false),
                NodeData::GetAccessor(data) => (data.name, data.modifiers, false),
                NodeData::SetAccessor(data) => (data.name, data.modifiers, false),
                _ => continue,
            };
            let name_is_private = name
                .map(|name| self.private_name_text(self.node(name)).is_some())
                .unwrap_or(false);
            let is_static = self.has_modifier(modifiers, SyntaxKind::StaticKeyword)?;

            if is_static {
                has_static_private_or_auto_accessor |= (name_is_private || is_auto_accessor)
                    && self.should_transform_private_class_element(member, modifiers)?;
                continue;
            }
            if is_auto_accessor
                || !name_is_private
                || self.has_modifier(modifiers, SyntaxKind::AbstractKeyword)?
            {
                continue;
            }
            has_instance_constructor_reference |= self.resolver.has_node_check_flag(
                self.resolver_node(member)?,
                NodeCheckFlags::CONTAINS_CONSTRUCTOR_REFERENCE.bits() as u32,
            )?;
        }

        Ok(ClassFactsPlan {
            static_facts,
            has_static_private_or_auto_accessor,
            has_instance_constructor_reference,
        })
    }

    fn scan_private_environment(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<PrivateEnvironmentPlan, TransformError> {
        let mut declarations = Vec::<PrivateDeclaration>::new();
        let mut auto_accessor_backings = Vec::<PrivateDeclaration>::new();
        let mut untransformed_names = BTreeSet::new();
        for member in self.array_nodes(members)? {
            let record = self.context.arena().node(member)?;
            let (name, modifiers, kind) = match &record.data {
                NodeData::PropertyDeclaration(data) => {
                    (data.name, data.modifiers, PrivateDeclarationKind::Field)
                }
                NodeData::MethodDeclaration(data) => {
                    (data.name, data.modifiers, PrivateDeclarationKind::Method)
                }
                NodeData::GetAccessor(data) => {
                    (data.name, data.modifiers, PrivateDeclarationKind::Getter)
                }
                NodeData::SetAccessor(data) => {
                    (data.name, data.modifiers, PrivateDeclarationKind::Setter)
                }
                _ => continue,
            };
            let Some(name) = name else {
                continue;
            };
            let name = self.node(name);
            let Some(private_name) = self.private_name_text(name) else {
                continue;
            };
            let private_name = private_name.to_owned();
            let is_static = self.has_modifier(modifiers, SyntaxKind::StaticKeyword)?;
            if !self.should_transform_private_class_element(member, modifiers)? {
                untransformed_names.insert(private_name);
                continue;
            }
            let binding_owner = if self.resolver.has_node_check_flag(
                self.resolver_node(name)?,
                NodeCheckFlags::BLOCK_SCOPED_BINDING_IN_LOOP.bits() as u32,
            )? {
                LexicalBindingOwner::CurrentLoop
            } else {
                LexicalBindingOwner::Hoisted
            };
            if self
                .generated_auto_accessor_backings
                .contains(&member.node())
            {
                auto_accessor_backings.push(PrivateDeclaration {
                    original: member,
                    name: private_name,
                    binding_owner,
                    is_static,
                    kind,
                });
                continue;
            }
            declarations.push(PrivateDeclaration {
                original: member,
                name: private_name,
                binding_owner,
                is_static,
                kind,
            });
        }
        declarations.extend(auto_accessor_backings);

        let instance_brand_owner = declarations
            .iter()
            .any(|declaration| {
                !declaration.is_static
                    && !matches!(&declaration.kind, PrivateDeclarationKind::Field)
            })
            .then(|| {
                if declarations.iter().any(|declaration| {
                    !declaration.is_static
                        && !matches!(&declaration.kind, PrivateDeclarationKind::Field)
                        && declaration.binding_owner == LexicalBindingOwner::CurrentLoop
                }) {
                    LexicalBindingOwner::CurrentLoop
                } else {
                    LexicalBindingOwner::Hoisted
                }
            });

        Ok(PrivateEnvironmentPlan {
            declarations,
            untransformed_names,
            instance_brand_owner,
        })
    }

    fn class_temp_plan(&self, original: TransformNode) -> Result<ClassTempPlan, TransformError> {
        let owner = if self.resolver.has_node_check_flag(
            self.resolver_node(original)?,
            NodeCheckFlags::BLOCK_SCOPED_BINDING_IN_LOOP.bits() as u32,
        )? {
            LexicalBindingOwner::CurrentLoop
        } else {
            LexicalBindingOwner::Hoisted
        };
        Ok(ClassTempPlan { owner })
    }

    /// Phase 1: tsc allocates the WeakSet brand while entering the private
    /// environment, before it computes/consumes class facts.
    fn allocate_instance_brand(
        &mut self,
        plan: &PrivateEnvironmentPlan,
        class_name: Option<&str>,
    ) -> Result<Option<ClassBinding>, TransformError> {
        plan.instance_brand_owner
            .map(|owner| {
                self.allocate_private_name(
                    self.private_generated_name(class_name, "instances"),
                    PrivateGeneratedNameRole::Storage,
                    owner,
                )
            })
            .transpose()
    }

    /// Phase 2: allocate only the semantic class-constructor identity selected
    /// by getClassFacts. A fallback class-expression result temp is a later
    /// sequencing concern and must not steal names from member transforms.
    fn allocate_class_constructor_identity(
        &mut self,
        plan: ClassConstructorReferencePlan,
        preferred_class_this: Option<ClassBinding>,
        temp_plan: ClassTempPlan,
    ) -> Result<Option<ClassBinding>, TransformError> {
        if !plan.needs_identity() {
            return Ok(None);
        }
        match preferred_class_this {
            Some(binding) => Ok(Some(binding)),
            None => self.allocate_class_temp_name(temp_plan).map(Some),
        }
    }

    /// Phase 4: private slot bindings are allocated only after the class
    /// constructor and real heritage-super identities have been reserved.
    #[allow(clippy::too_many_arguments)]
    fn materialize_private_environment(
        &mut self,
        plan: PrivateEnvironmentPlan,
        class_name: Option<&str>,
        class_alias: Option<ClassBinding>,
        instance_brand: Option<ClassBinding>,
        super_alias: Option<ClassBinding>,
        is_legacy_decorated: bool,
        static_facts: StaticLexicalFacts,
    ) -> Result<PrivateEnvironment, TransformError> {
        let needs_static_receiver = static_facts.contains_this || static_facts.contains_super;
        let static_receiver = needs_static_receiver.then(|| match class_alias.clone() {
            Some(receiver) => StaticReceiver::Bound(receiver),
            None if is_legacy_decorated => StaticReceiver::InvalidLegacyDecorated,
            None => unreachable!("ordinary static lexical evaluation owns a class identity"),
        });
        let static_super_policy = if is_legacy_decorated {
            StaticSuperPolicy::InvalidLegacyDecorated
        } else {
            StaticSuperPolicy::Available
        };
        let PrivateEnvironmentPlan {
            declarations,
            untransformed_names,
            instance_brand_owner: _,
        } = plan;
        let mut environment = PrivateEnvironment {
            effective_slots: BTreeMap::new(),
            private_slots: Vec::with_capacity(declarations.len()),
            declarations: Vec::with_capacity(declarations.len()),
            untransformed_names,
            class_alias: class_alias.clone(),
            instance_brand: instance_brand.clone(),
            static_receiver,
            static_super_policy,
            super_alias,
            is_legacy_decorated,
        };
        for declaration in declarations {
            let PrivateDeclaration {
                original,
                name,
                binding_owner,
                is_static,
                kind,
            } = declaration;
            let is_valid =
                name != "constructor" && !environment.effective_slots.contains_key(&name);
            let base_name = self.private_generated_name(class_name, &name);

            // tsc allocates one binding for each declaration event, but a
            // complementary getter/setter mutates the immediately visible
            // accessor slot instead of replacing it. This preserves both
            // source-order name allocation and the legal accessor-pair case.
            let (slot_index, replaces_effective_slot) = match kind {
                PrivateDeclarationKind::Field => {
                    let element = PrivateElement::Field {
                        value_name: self.allocate_private_name(
                            base_name,
                            PrivateGeneratedNameRole::Storage,
                            binding_owner,
                        )?,
                    };
                    let slot_index = environment.push_slot(PrivateSlot {
                        placement: Self::private_slot_placement(
                            is_static,
                            &element,
                            &class_alias,
                            &instance_brand,
                        ),
                        element,
                        is_valid,
                    });
                    (slot_index, true)
                }
                PrivateDeclarationKind::Method => {
                    let element = PrivateElement::Method {
                        method_name: self.allocate_private_name(
                            base_name,
                            PrivateGeneratedNameRole::Method,
                            binding_owner,
                        )?,
                    };
                    let slot_index = environment.push_slot(PrivateSlot {
                        placement: Self::private_slot_placement(
                            is_static,
                            &element,
                            &class_alias,
                            &instance_brand,
                        ),
                        element,
                        is_valid,
                    });
                    (slot_index, true)
                }
                PrivateDeclarationKind::Getter => {
                    let getter_name = self.allocate_private_name(
                        base_name,
                        PrivateGeneratedNameRole::Getter,
                        binding_owner,
                    )?;
                    let previous_index = environment.effective_slots.get(&name).copied();
                    let can_extend = previous_index
                        .and_then(|index| environment.private_slots.get(index))
                        .is_some_and(|previous| {
                            previous.is_static() == is_static
                                && matches!(
                                    &previous.element,
                                    PrivateElement::Accessor {
                                        getter_name: None,
                                        ..
                                    }
                                )
                        });
                    if can_extend {
                        let previous_index =
                            previous_index.expect("checked private accessor index remains present");
                        let previous = environment
                            .private_slots
                            .get_mut(previous_index)
                            .expect("checked private accessor slot remains present");
                        let PrivateElement::Accessor {
                            getter_name: previous_getter,
                            ..
                        } = &mut previous.element
                        else {
                            unreachable!("checked private accessor slot remains an accessor");
                        };
                        *previous_getter = Some(getter_name);
                        (previous_index, false)
                    } else {
                        let element = PrivateElement::Accessor {
                            getter_name: Some(getter_name),
                            setter_name: None,
                        };
                        let slot_index = environment.push_slot(PrivateSlot {
                            placement: Self::private_slot_placement(
                                is_static,
                                &element,
                                &class_alias,
                                &instance_brand,
                            ),
                            element,
                            is_valid,
                        });
                        (slot_index, true)
                    }
                }
                PrivateDeclarationKind::Setter => {
                    let setter_name = self.allocate_private_name(
                        base_name,
                        PrivateGeneratedNameRole::Setter,
                        binding_owner,
                    )?;
                    let previous_index = environment.effective_slots.get(&name).copied();
                    let can_extend = previous_index
                        .and_then(|index| environment.private_slots.get(index))
                        .is_some_and(|previous| {
                            previous.is_static() == is_static
                                && matches!(
                                    &previous.element,
                                    PrivateElement::Accessor {
                                        setter_name: None,
                                        ..
                                    }
                                )
                        });
                    if can_extend {
                        let previous_index =
                            previous_index.expect("checked private accessor index remains present");
                        let previous = environment
                            .private_slots
                            .get_mut(previous_index)
                            .expect("checked private accessor slot remains present");
                        let PrivateElement::Accessor {
                            setter_name: previous_setter,
                            ..
                        } = &mut previous.element
                        else {
                            unreachable!("checked private accessor slot remains an accessor");
                        };
                        *previous_setter = Some(setter_name);
                        (previous_index, false)
                    } else {
                        let element = PrivateElement::Accessor {
                            getter_name: None,
                            setter_name: Some(setter_name),
                        };
                        let slot_index = environment.push_slot(PrivateSlot {
                            placement: Self::private_slot_placement(
                                is_static,
                                &element,
                                &class_alias,
                                &instance_brand,
                            ),
                            element,
                            is_valid,
                        });
                        (slot_index, true)
                    }
                }
            };
            if replaces_effective_slot {
                environment.effective_slots.insert(name, slot_index);
            }
            environment.declarations.push(PrivateDeclarationSlot {
                declaration: original,
                slot_index,
            });
        }
        Ok(environment)
    }

    fn static_lexical_facts(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<StaticLexicalFacts, TransformError> {
        let mut facts = StaticLexicalFacts::default();
        for member in self.array_nodes(members)? {
            let record = self.context.arena().node(member)?;
            let is_static_property_or_block = match &record.data {
                NodeData::PropertyDeclaration(data)
                    if self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)? =>
                {
                    true
                }
                NodeData::ClassStaticBlockDeclaration(_) => true,
                _ => false,
            };
            if !is_static_property_or_block {
                continue;
            }
            // getClassFacts reads the complete member flags, not just the
            // initializer/body. A computed name executes in the enclosing
            // class-evaluation frame, but its propagated lexical flag still
            // reserves this class expression's constructor identity before
            // member traversal allocates computed-name caches.
            let member_flags = self.context.arena().transform_flags(member);
            facts.contains_this |= member_flags.contains(TransformFlags::CONTAINS_LEXICAL_THIS);
            facts.contains_super |= member_flags.contains(TransformFlags::CONTAINS_LEXICAL_SUPER);
        }
        Ok(facts)
    }

    fn static_bindings(&self) -> Option<StaticBindings> {
        let environment = self.private_environments.last()?;
        Some(StaticBindings {
            receiver: environment.static_receiver.clone()?,
            super_alias: environment.super_alias.clone(),
            super_policy: environment.static_super_policy,
        })
    }

    /// Resolve the receiver selected by `transformAutoAccessor` for a static
    /// redirector. This is deliberately separate from `static_bindings`: an
    /// auto-accessor reads the class constructor established by the class
    /// facts scan even when no relocated initializer contains lexical `this`.
    /// Its generated private backing guarantees that constructor identity in
    /// the downlevel target band.
    ///
    /// tsc-port: transformAutoAccessor @6.0.3
    /// tsc-hash: 8a50c14b7896add6d7bd02c80229f4bf2367d427d6111e04c0abc39d9b0ea3d1
    /// tsc-span: _tsc.js:96256-96291
    fn static_auto_accessor_bindings(&self) -> StaticBindings {
        let environment = self
            .private_environments
            .last()
            .expect("static auto-accessor owns a private environment");
        let class_alias = environment
            .class_alias
            .clone()
            .expect("downlevel static auto-accessor owns a class constructor binding");
        StaticBindings {
            receiver: StaticReceiver::Bound(class_alias),
            super_alias: environment.super_alias.clone(),
            super_policy: environment.static_super_policy,
        }
    }

    fn private_generated_name(&self, class_name: Option<&str>, suffix: &str) -> String {
        match class_name {
            Some(class_name) if !class_name.is_empty() => format!("_{class_name}_{suffix}"),
            _ => format!("_{suffix}"),
        }
    }

    fn private_slot_placement(
        is_static: bool,
        element: &PrivateElement,
        class_alias: &Option<ClassBinding>,
        instance_brand: &Option<ClassBinding>,
    ) -> PrivatePlacement {
        if is_static {
            PrivatePlacement::Static {
                class_alias: class_alias
                    .clone()
                    .expect("static private slots own a class alias"),
            }
        } else {
            let brand_name = match element {
                PrivateElement::Field { value_name } => value_name.clone(),
                PrivateElement::Method { .. } | PrivateElement::Accessor { .. } => instance_brand
                    .clone()
                    .expect("instance private behavior owns a WeakSet brand"),
            };
            PrivatePlacement::Instance { brand_name }
        }
    }

    fn private_slot(&self, name: TransformNode) -> Result<&PrivateSlot, TransformError> {
        self.private_slot_if_declared(name)?
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PrivateIdentifier,
                field: "declared private slot",
            })
    }

    /// tsc's `accessPrivateIdentifier2` is an optional lookup for use sites:
    /// a syntactic private name can be invalid because no enclosing private
    /// environment declares it. Declaration planning continues to use
    /// `private_slot` so a missing slot there remains an internal invariant
    /// violation rather than being silently recovered.
    fn private_slot_if_declared(
        &self,
        name: TransformNode,
    ) -> Result<Option<&PrivateSlot>, TransformError> {
        let private_name =
            self.private_name_text(name)
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PrivateIdentifier,
                    field: "private identifier text",
                })?;
        for environment in self.private_environments.iter().rev() {
            if environment.untransformed_names.contains(private_name) {
                return Ok(None);
            }
            if let Some(slot) = environment.effective_slot(private_name) {
                return Ok(Some(slot));
            }
        }
        Ok(None)
    }

    /// Recover an invalid, unresolved private identifier exactly where tsc's
    /// class-fields visitor does. A private identifier directly parented by a
    /// statement is retained; every other one that reaches the generic walk
    /// becomes an empty Identifier, producing the diagnostic-recovery output
    /// without inventing a private slot.
    ///
    /// tsc-port: visitPrivateIdentifier @6.0.3
    /// tsc-hash: e37c3cf4cf723c1da5397ce6429fc91d73981c739d48970520852bc0771822d9
    /// tsc-span: _tsc.js:96103-96110
    fn visit_private_identifier(
        &mut self,
        original: TransformNode,
    ) -> Result<NodeId, TransformError> {
        if self
            .private_name_text(original)
            .is_some_and(|private_name| {
                self.private_environments
                    .iter()
                    .rev()
                    .any(|environment| environment.untransformed_names.contains(private_name))
            })
        {
            return Ok(original.node());
        }
        let parent_is_statement = self
            .tree_ownership
            .unique_parent(original.node())
            .and_then(|parent| self.context.arena().node_ref(self.source, parent))
            .map(|parent| self.context.arena().node(parent).map(|node| node.kind))
            .transpose()?
            .is_some_and(|kind| {
                kind >= SyntaxKind::FirstStatement && kind <= SyntaxKind::LastStatement
            });
        if parent_is_statement {
            return Ok(original.node());
        }

        let recovered = self.create_identifier("")?;
        self.context
            .arena_mut()?
            .set_original_node(recovered, Some(original))?;
        Ok(recovered.node())
    }

    fn private_name_text(&self, name: TransformNode) -> Option<&str> {
        match &self.context.arena().node(name).ok()?.data {
            NodeData::PrivateIdentifier(data) => Some(data.text.trim_start_matches('#')),
            _ => None,
        }
    }

    fn visit_property_access(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::PropertyAccessExpressionData,
    ) -> Result<NodeId, TransformError> {
        if let Some(access) = self.static_super_access(original)? {
            match access {
                StaticSuperAccessResolution::Bound(access) => {
                    let expression = self.create_static_super_get(&access)?;
                    self.set_original_and_range(expression, original)?;
                    return Ok(expression.node());
                }
                StaticSuperAccessResolution::InvalidLegacyDecorated { .. } => {
                    data.expression = Some(self.create_void_zero()?.node());
                    return self.update_generic(original, NodeData::PropertyAccessExpression(data));
                }
            }
        }
        let Some(name) = data.name else {
            return self.update_generic(original, NodeData::PropertyAccessExpression(data));
        };
        let name = self.node(name);
        if self.private_name_text(name).is_none() {
            return self.update_generic(original, NodeData::PropertyAccessExpression(data));
        }
        let Some(slot) = self.private_slot_if_declared(name)?.cloned() else {
            return self.update_generic(original, NodeData::PropertyAccessExpression(data));
        };
        let receiver = self.visit_required(
            data.expression,
            SyntaxKind::PropertyAccessExpression,
            "expression",
        )?;
        let access = self.create_private_get(receiver, &slot)?;
        self.set_original_and_range(access, original)?;
        Ok(access.node())
    }

    fn visit_element_access(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ElementAccessExpressionData,
    ) -> Result<NodeId, TransformError> {
        if let Some(access) = self.static_super_access(original)? {
            match access {
                StaticSuperAccessResolution::Bound(access) => {
                    let expression = self.create_static_super_get(&access)?;
                    self.set_original_and_range(expression, original)?;
                    return Ok(expression.node());
                }
                StaticSuperAccessResolution::InvalidLegacyDecorated { .. } => {
                    data.expression = Some(self.create_void_zero()?.node());
                    return self.update_generic(original, NodeData::ElementAccessExpression(data));
                }
            }
        }
        self.update_generic(original, NodeData::ElementAccessExpression(data))
    }

    fn visit_binary_expression(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::BinaryExpressionData,
        value_use: ExpressionValueUse,
    ) -> Result<NodeId, TransformError> {
        let operator = data
            .operator_token
            .map(|operator| {
                self.context
                    .arena()
                    .node(self.node(operator))
                    .map(|node| node.kind)
            })
            .transpose()?;

        if operator == Some(SyntaxKind::EqualsToken)
            && self.is_destructuring_assignment_target(data.left)?
        {
            return self.visit_destructuring_assignment(original, data);
        }

        if operator == Some(SyntaxKind::InKeyword) {
            if let Some(left) = data.left {
                let left = self.node(left);
                if self.private_name_text(left).is_some() {
                    if let Some(slot) = self.private_slot_if_declared(left)?.cloned() {
                        let receiver =
                            self.visit_required(data.right, SyntaxKind::BinaryExpression, "right")?;
                        let expression = self.create_private_in(&slot, receiver)?;
                        self.set_original_and_range(expression, original)?;
                        return Ok(expression.node());
                    }
                }
            }
        }

        let Some(operator) = operator.filter(|operator| {
            *operator == SyntaxKind::EqualsToken
                || (*operator >= SyntaxKind::FirstCompoundAssignment
                    && *operator <= SyntaxKind::LastCompoundAssignment)
        }) else {
            return self.update_generic(original, NodeData::BinaryExpression(data));
        };
        let assignment_target = data
            .left
            .map(|left| self.skip_runtime_transparent_outer_expressions(self.node(left)))
            .transpose()?;
        if let Some(access) = assignment_target
            .map(|target| self.static_super_access(target))
            .transpose()?
            .flatten()
        {
            return match access {
                StaticSuperAccessResolution::Bound(access) => {
                    self.lower_static_super_assignment(original, data, operator, value_use, access)
                }
                StaticSuperAccessResolution::InvalidLegacyDecorated { .. } => {
                    self.update_generic(original, NodeData::BinaryExpression(data))
                }
            };
        }
        let Some((receiver, slot)) =
            self.private_access_target(assignment_target.map(TransformNode::node))?
        else {
            return self.update_generic(original, NodeData::BinaryExpression(data));
        };
        let receiver = self
            .visit(receiver.node())?
            .map(|receiver| self.node(receiver))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyAccessExpression,
                field: "expression",
            })?;
        let right = self.visit_required(data.right, SyntaxKind::BinaryExpression, "right")?;
        let value = if operator == SyntaxKind::EqualsToken {
            right
        } else {
            let stabilized = self.stabilize_inline_receiver(receiver)?;
            let current = self.create_private_get(stabilized.read, &slot)?;
            let binary_operator = Self::non_assignment_operator(operator);
            let right = self.parenthesize_right_binary_operand(binary_operator, right)?;
            let value = self.create_binary(current, binary_operator, right)?;
            let assignment_receiver = stabilized.initialized.unwrap_or(stabilized.read);
            let expression = self.create_private_set(assignment_receiver, &slot, value)?;
            self.set_original_and_range(expression, original)?;
            return Ok(expression.node());
        };
        let expression = self.create_private_set(receiver, &slot, value)?;
        self.set_original_and_range(expression, original)?;
        Ok(expression.node())
    }

    fn is_destructuring_assignment_target(
        &self,
        target: Option<NodeId>,
    ) -> Result<bool, TransformError> {
        let Some(target) = target else {
            return Ok(false);
        };
        Ok(matches!(
            self.context.arena().node(self.node(target))?.kind,
            SyntaxKind::ObjectLiteralExpression | SyntaxKind::ArrayLiteralExpression
        ))
    }

    /// tsc-port: assignmentTargetVisitor @6.0.3
    /// tsc-hash: 5a421385d0d56ce05cdf0e2911415f23e4a8ebdc243d13a3a75966b57e4db501
    /// tsc-span: _tsc.js:96038-96046
    ///
    /// tsc-port: visitBinaryExpression.destructuringAssignmentBranch @6.0.3
    /// tsc-hash: 91aeff54cb285f39f4720af42194346a66331062474f94790454d5beb2a41939
    /// tsc-span: _tsc.js:96694-96706
    ///
    /// tsc-port: wrapPrivateIdentifierForDestructuringTarget/visitAssignmentPattern @6.0.3
    /// tsc-hash: 78dd10e9980d346bc60ef3b6fba5d8b70e520f6d07ba37e45038f3e8030bdab4
    /// tsc-span: _tsc.js:97796-97926
    fn visit_destructuring_assignment(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::BinaryExpressionData,
    ) -> Result<NodeId, TransformError> {
        let mut plan = DestructuringAssignmentPlan::default();
        let left = data.left.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::BinaryExpression,
            field: "destructuring assignment target",
        })?;
        data.left = Some(
            self.visit_destructuring_assignment_target(left, &mut plan)?
                .node(),
        );
        data.right = Some(
            self.visit_required(data.right, SyntaxKind::BinaryExpression, "right")?
                .node(),
        );
        let updated_data = NodeData::BinaryExpression(data);
        let flags = flags_after_update(self.context.arena(), original, &updated_data)?;
        let assignment = self
            .context
            .factory()?
            .update_node(original, updated_data, flags)?;
        if plan.prefix_expressions.is_empty() {
            return Ok(assignment.node());
        }
        plan.prefix_expressions.push(assignment);
        Ok(self.inline_expressions(plan.prefix_expressions)?.node())
    }

    fn visit_destructuring_assignment_target(
        &mut self,
        id: NodeId,
        plan: &mut DestructuringAssignmentPlan,
    ) -> Result<TransformNode, TransformError> {
        let original = self.node(id);
        match self.context.arena().node(original)?.data.clone() {
            NodeData::ObjectLiteralExpression(mut data) => {
                data.properties = self.visit_object_assignment_elements(data.properties, plan)?;
                self.update_contextual_node(original, NodeData::ObjectLiteralExpression(data))
            }
            NodeData::ArrayLiteralExpression(mut data) => {
                data.elements = self.visit_array_assignment_elements(data.elements, plan)?;
                self.update_contextual_node(original, NodeData::ArrayLiteralExpression(data))
            }
            NodeData::PropertyAccessExpression(data)
                if data
                    .name
                    .map(|name| self.node(name))
                    .is_some_and(|name| self.private_name_text(name).is_some()) =>
            {
                let name = self.node(data.name.expect("private access owns a name"));
                if self.private_slot_if_declared(name)?.is_some() {
                    self.wrap_private_destructuring_target(original, data, plan)
                } else {
                    let updated =
                        self.update_generic(original, NodeData::PropertyAccessExpression(data))?;
                    Ok(self.node(updated))
                }
            }
            NodeData::PropertyAccessExpression(data) => {
                match self.static_super_access(original)? {
                    Some(StaticSuperAccessResolution::Bound(access)) => {
                        self.wrap_static_super_destructuring_target(original, access)
                    }
                    Some(StaticSuperAccessResolution::InvalidLegacyDecorated { .. }) => {
                        let mut data = data;
                        data.expression = Some(self.create_void_zero()?.node());
                        let updated = self
                            .update_generic(original, NodeData::PropertyAccessExpression(data))?;
                        Ok(self.node(updated))
                    }
                    None => {
                        let updated = self
                            .update_generic(original, NodeData::PropertyAccessExpression(data))?;
                        Ok(self.node(updated))
                    }
                }
            }
            NodeData::ElementAccessExpression(data) => match self.static_super_access(original)? {
                Some(StaticSuperAccessResolution::Bound(access)) => {
                    self.wrap_static_super_destructuring_target(original, access)
                }
                Some(StaticSuperAccessResolution::InvalidLegacyDecorated { .. }) => {
                    let mut data = data;
                    data.expression = Some(self.create_void_zero()?.node());
                    let updated =
                        self.update_generic(original, NodeData::ElementAccessExpression(data))?;
                    Ok(self.node(updated))
                }
                None => {
                    let updated =
                        self.update_generic(original, NodeData::ElementAccessExpression(data))?;
                    Ok(self.node(updated))
                }
            },
            _ => self.visit(id)?.map(|id| self.node(id)).ok_or(
                TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::BinaryExpression,
                    field: "destructuring assignment target",
                },
            ),
        }
    }

    fn visit_object_assignment_elements(
        &mut self,
        elements: Option<NodeArrayId>,
        plan: &mut DestructuringAssignmentPlan,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(elements) = elements else {
            return Ok(None);
        };
        let original = self.array(elements);
        let ids = self.context.arena().node_array(original)?.nodes.clone();
        let mut visited = Vec::with_capacity(ids.len());
        for id in ids {
            let node = self.node(id);
            let element = match self.context.arena().node(node)?.data.clone() {
                NodeData::PropertyAssignment(mut data) => {
                    data.name = self.visit_optional_node(data.name)?;
                    data.initializer = Some(
                        self.visit_assignment_element(
                            data.initializer
                                .ok_or(TransformError::RequiredChildRemoved {
                                    parent: SyntaxKind::PropertyAssignment,
                                    field: "initializer",
                                })?,
                            plan,
                        )?
                        .node(),
                    );
                    self.update_contextual_node(node, NodeData::PropertyAssignment(data))?
                }
                NodeData::SpreadAssignment(mut data) => {
                    data.expression = Some(
                        self.visit_destructuring_assignment_target(
                            data.expression
                                .ok_or(TransformError::RequiredChildRemoved {
                                    parent: SyntaxKind::SpreadAssignment,
                                    field: "expression",
                                })?,
                            plan,
                        )?
                        .node(),
                    );
                    self.update_contextual_node(node, NodeData::SpreadAssignment(data))?
                }
                _ => self.visit(id)?.map(|id| self.node(id)).ok_or(
                    TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ObjectLiteralExpression,
                        field: "assignment element",
                    },
                )?,
            };
            visited.push(element);
        }
        Ok(Some(
            self.context
                .factory()?
                .update_node_array(original, visited)?
                .array(),
        ))
    }

    fn visit_array_assignment_elements(
        &mut self,
        elements: Option<NodeArrayId>,
        plan: &mut DestructuringAssignmentPlan,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(elements) = elements else {
            return Ok(None);
        };
        let original = self.array(elements);
        let ids = self.context.arena().node_array(original)?.nodes.clone();
        let mut visited = Vec::with_capacity(ids.len());
        for id in ids {
            let node = self.node(id);
            let element = match self.context.arena().node(node)?.data.clone() {
                NodeData::SpreadElement(mut data) => {
                    data.expression = Some(
                        self.visit_destructuring_assignment_target(
                            data.expression
                                .ok_or(TransformError::RequiredChildRemoved {
                                    parent: SyntaxKind::SpreadElement,
                                    field: "expression",
                                })?,
                            plan,
                        )?
                        .node(),
                    );
                    self.update_contextual_node(node, NodeData::SpreadElement(data))?
                }
                NodeData::OmittedExpression(_) => node,
                _ => self.visit_assignment_element(id, plan)?,
            };
            visited.push(element);
        }
        Ok(Some(
            self.context
                .factory()?
                .update_node_array(original, visited)?
                .array(),
        ))
    }

    fn visit_assignment_element(
        &mut self,
        id: NodeId,
        plan: &mut DestructuringAssignmentPlan,
    ) -> Result<TransformNode, TransformError> {
        let original = self.node(id);
        let NodeData::BinaryExpression(mut data) =
            self.context.arena().node(original)?.data.clone()
        else {
            return self.visit_destructuring_assignment_target(id, plan);
        };
        let operator = data
            .operator_token
            .and_then(|operator| self.context.arena().node_ref(self.source, operator))
            .map(|operator| self.context.arena().node(operator).map(|node| node.kind))
            .transpose()?;
        if operator != Some(SyntaxKind::EqualsToken) {
            return self.visit_destructuring_assignment_target(id, plan);
        }
        data.left = Some(
            self.visit_destructuring_assignment_target(
                data.left.ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::BinaryExpression,
                    field: "assignment element target",
                })?,
                plan,
            )?
            .node(),
        );
        data.right = Some(
            self.visit_required(data.right, SyntaxKind::BinaryExpression, "right")?
                .node(),
        );
        self.update_contextual_node(original, NodeData::BinaryExpression(data))
    }

    fn wrap_private_destructuring_target(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::PropertyAccessExpressionData,
        plan: &mut DestructuringAssignmentPlan,
    ) -> Result<TransformNode, TransformError> {
        let name =
            data.name
                .map(|name| self.node(name))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PropertyAccessExpression,
                    field: "private name",
                })?;
        let Some(slot) = self.private_slot_if_declared(name)?.cloned() else {
            let updated =
                self.update_generic(original, NodeData::PropertyAccessExpression(data))?;
            return Ok(self.node(updated));
        };
        let receiver = data.expression.map(|receiver| self.node(receiver)).ok_or(
            TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyAccessExpression,
                field: "expression",
            },
        )?;
        let receiver_kind = self.context.arena().node(receiver)?.kind;
        let capture_receiver = matches!(
            receiver_kind,
            SyntaxKind::ThisKeyword | SyntaxKind::SuperKeyword
        ) || !self.is_simple_copiable_expression(receiver)?;
        let receiver = self.visit_required(
            data.expression,
            SyntaxKind::PropertyAccessExpression,
            "expression",
        )?;
        let receiver = if capture_receiver {
            let temporary = self.allocate_temp_name()?;
            let target = self.create_binding_identifier(&temporary)?;
            plan.prefix_expressions
                .push(self.create_assignment(target, receiver)?);
            self.create_binding_identifier(&temporary)?
        } else {
            receiver
        };

        let value_binding = self.allocate_parameter_alias()?;
        let value = self.create_binding_identifier(&value_binding)?;
        let assignment = self.create_private_set(receiver, &slot, value)?;
        self.create_assignment_target_wrapper(original, &value_binding, assignment)
    }

    fn wrap_static_super_destructuring_target(
        &mut self,
        original: TransformNode,
        access: StaticSuperAccess,
    ) -> Result<TransformNode, TransformError> {
        let value_binding = self.allocate_parameter_alias()?;
        let value = self.create_binding_identifier(&value_binding)?;
        let assignment = self.create_static_super_set(&access, value)?;
        self.create_assignment_target_wrapper(original, &value_binding, assignment)
    }

    fn create_assignment_target_wrapper(
        &mut self,
        original: TransformNode,
        value_binding: &ClassBinding,
        assignment: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        // This generated name is a setter parameter, not a hoisted binding.
        // Reserve it in the wrapper's function scope without materializing a
        // `var`, just as tsc's getGeneratedNameForNode does.
        let parameter_name = self.create_binding_identifier(value_binding)?;
        let parameter = self.context.factory()?.create_node(
            self.source,
            NodeData::Parameter(tsc_syntax::nodes::ParameterData {
                name: Some(parameter_name.node()),
                modifiers: None,
                dot_dot_dot_token: None,
                question_token: None,
                r#type: None,
                initializer: None,
            }),
            TransformFlags::NONE,
        )?;
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, vec![parameter])?;
        let statement = self.create_expression_statement(assignment)?;
        let body = self.create_block(vec![statement], false)?;
        let accessor_name = self.create_identifier("value")?;
        let setter = self.context.factory()?.create_node(
            self.source,
            NodeData::SetAccessor(tsc_syntax::nodes::SetAccessorData {
                name: Some(accessor_name.node()),
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                body: Some(body.node()),
                modifiers: None,
            }),
            TransformFlags::NONE,
        )?;
        let object = self.create_object_literal(vec![setter], false)?;
        let object = self.create_parenthesized(object)?;
        let wrapper = self.create_property_access(object, "value")?;
        self.context
            .arena_mut()?
            .set_original_node(wrapper, Some(original))?;
        Ok(wrapper)
    }

    fn update_contextual_node(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<TransformNode, TransformError> {
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        self.context.factory()?.update_node(original, data, flags)
    }

    fn visit_call_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::CallExpressionData,
    ) -> Result<NodeId, TransformError> {
        let Some(expression) = data.expression else {
            return self.update_generic(original, NodeData::CallExpression(data));
        };
        let expression_node = self.node(expression);
        if let Some(access) = self.static_super_access(expression_node)? {
            let (target, class_receiver) = match access {
                StaticSuperAccessResolution::Bound(access) => {
                    let class_receiver = access.class_receiver.clone();
                    let target = self.create_static_super_get(&access)?;
                    self.set_original_and_range(target, expression_node)?;
                    (target, class_receiver)
                }
                StaticSuperAccessResolution::InvalidLegacyDecorated {
                    class_receiver: Some(class_receiver),
                } => {
                    // visitInvalidSuperProperty owns the `void 0` base, while
                    // visitCallExpression independently consumes the available
                    // class constructor as the call receiver.
                    let target = self.visit_required(
                        Some(expression),
                        SyntaxKind::CallExpression,
                        "expression",
                    )?;
                    (target, class_receiver)
                }
                StaticSuperAccessResolution::InvalidLegacyDecorated {
                    class_receiver: None,
                } => return self.update_generic(original, NodeData::CallExpression(data)),
            };
            let call = self.create_property_access(target, "call")?;
            let mut arguments = vec![self.create_binding_identifier(&class_receiver)?];
            arguments.extend(self.visit_node_array(data.arguments)?);
            data.expression = Some(call.node());
            data.type_arguments = self.visit_optional_nodes(data.type_arguments)?;
            let arguments = self
                .context
                .factory()?
                .create_node_array(self.source, arguments)?;
            data.arguments = Some(arguments.array());
            let flags = flags_after_update(
                self.context.arena(),
                original,
                &NodeData::CallExpression(data.clone()),
            )?;
            return Ok(self
                .context
                .factory()?
                .update_node(original, NodeData::CallExpression(data), flags)?
                .node());
        }
        let Some(binding) = self.private_call_binding(expression_node)? else {
            return self.update_generic(original, NodeData::CallExpression(data));
        };
        let is_call_chain = NodeFlags::from_bits(self.context.arena().node(original)?.flags)
            .contains(NodeFlags::OPTIONAL_CHAIN);
        let call = if is_call_chain {
            let question_dot_token = data.question_dot_token.take();
            self.create_property_access_chain(binding.target, question_dot_token, "call")?
        } else {
            self.create_property_access(binding.target, "call")?
        };
        let mut arguments = vec![binding.this_arg];
        arguments.extend(self.visit_node_array(data.arguments)?);
        data.expression = Some(call.node());
        data.type_arguments = None;
        let arguments = self
            .context
            .factory()?
            .create_node_array(self.source, arguments)?;
        data.arguments = Some(arguments.array());
        let flags = flags_after_update(
            self.context.arena(),
            original,
            &NodeData::CallExpression(data.clone()),
        )?;
        let call =
            self.context
                .factory()?
                .update_node(original, NodeData::CallExpression(data), flags)?;
        Ok(call.node())
    }

    fn visit_tagged_template_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::TaggedTemplateExpressionData,
    ) -> Result<NodeId, TransformError> {
        let Some(tag) = data.tag.map(|tag| self.node(tag)) else {
            return self.update_generic(original, NodeData::TaggedTemplateExpression(data));
        };
        if let Some(access) = self.static_super_access(tag)? {
            let (target, class_receiver) = match access {
                StaticSuperAccessResolution::Bound(access) => {
                    let class_receiver = access.class_receiver.clone();
                    let target = self.create_static_super_get(&access)?;
                    self.set_original_and_range(target, tag)?;
                    (target, class_receiver)
                }
                StaticSuperAccessResolution::InvalidLegacyDecorated {
                    class_receiver: Some(class_receiver),
                } => {
                    let target = self.visit_required(
                        Some(tag.node()),
                        SyntaxKind::TaggedTemplateExpression,
                        "tag",
                    )?;
                    (target, class_receiver)
                }
                StaticSuperAccessResolution::InvalidLegacyDecorated {
                    class_receiver: None,
                } => {
                    return self.update_generic(original, NodeData::TaggedTemplateExpression(data));
                }
            };
            let bind = self.create_property_access(target, "bind")?;
            let this_arg = self.create_binding_identifier(&class_receiver)?;
            data.tag = Some(self.create_call(bind, vec![this_arg])?.node());
            data.type_arguments = None;
            data.template = Some(
                self.visit_required(
                    data.template,
                    SyntaxKind::TaggedTemplateExpression,
                    "template",
                )?
                .node(),
            );
            let flags = flags_after_update(
                self.context.arena(),
                original,
                &NodeData::TaggedTemplateExpression(data.clone()),
            )?;
            return self
                .context
                .factory()?
                .update_node(original, NodeData::TaggedTemplateExpression(data), flags)
                .map(TransformNode::node);
        }
        let Some(binding) = self.private_call_binding(tag)? else {
            return self.update_generic(original, NodeData::TaggedTemplateExpression(data));
        };
        let bind = self.create_property_access(binding.target, "bind")?;
        data.tag = Some(self.create_call(bind, vec![binding.this_arg])?.node());
        data.type_arguments = None;
        data.template = Some(
            self.visit_required(
                data.template,
                SyntaxKind::TaggedTemplateExpression,
                "template",
            )?
            .node(),
        );
        let flags = flags_after_update(
            self.context.arena(),
            original,
            &NodeData::TaggedTemplateExpression(data.clone()),
        )?;
        self.context
            .factory()?
            .update_node(original, NodeData::TaggedTemplateExpression(data), flags)
            .map(TransformNode::node)
    }

    /// tsc-port: createCallBinding @6.0.3
    /// tsc-hash: 445f6a3542132e1adf49e01683e039e6fa034bd127cd15ab5447db84951b41bc
    /// tsc-span: _tsc.js:24691-24753
    ///
    /// tsc-port: visitCallExpression.privateAccessBranch @6.0.3
    /// tsc-hash: 699964b066f969eeb71c2624e126d21f1e30e3bc647c5f2b599a0c4aada19181
    /// tsc-span: _tsc.js:96579-96613
    ///
    /// tsc-port: visitTaggedTemplateExpression.privateAccessBranch @6.0.3
    /// tsc-hash: 153ff7179d533b43af20f4535e1f3ab4ae2e1fd68978598e1eba0faf722886ed
    /// tsc-span: _tsc.js:96614-96633
    ///
    /// The callable and its `this` argument are two views of one receiver
    /// evaluation. Complex receivers are captured in the surrounding function
    /// scope and the assignment remains parenthesized as the private-get
    /// receiver, matching the property-access target built by tsc's factory.
    fn private_call_binding(
        &mut self,
        expression: TransformNode,
    ) -> Result<Option<PrivateCallBinding>, TransformError> {
        let access = self.skip_runtime_transparent_outer_expressions(expression)?;
        let Some((receiver, slot)) = self.private_access_target(Some(access.node()))? else {
            return Ok(None);
        };
        let receiver = self
            .visit(receiver.node())?
            .map(|receiver| self.node(receiver))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyAccessExpression,
                field: "expression",
            })?;
        let stabilized = self.stabilize_receiver(receiver)?;
        let target_receiver = match stabilized.initialized {
            Some(initialized) => self.create_parenthesized(initialized)?,
            None => stabilized.read,
        };
        let target = self.create_private_get(target_receiver, &slot)?;
        let target = self.set_original_and_range(target, access)?;
        Ok(Some(PrivateCallBinding {
            target,
            this_arg: stabilized.read,
        }))
    }

    fn property_receiver_is_super(&self, receiver: Option<NodeId>) -> Result<bool, TransformError> {
        receiver
            .map(|receiver| {
                self.context
                    .arena()
                    .node(self.node(receiver))
                    .map(|receiver| receiver.kind == SyntaxKind::SuperKeyword)
            })
            .transpose()
            .map(Option::unwrap_or_default)
    }

    /// Resolve a public static-`super` target once, before a consumer decides
    /// whether it needs a read, write, update, call binding, or setter wrapper.
    /// Element keys are visited here so every consumer observes their side
    /// effects exactly once.
    fn static_super_access(
        &mut self,
        expression: TransformNode,
    ) -> Result<Option<StaticSuperAccessResolution>, TransformError> {
        let access = match self.context.arena().node(expression)?.data.clone() {
            NodeData::PropertyAccessExpression(data)
                if self.property_receiver_is_super(data.expression)? =>
            {
                let name = data.name.map(|name| self.node(name)).ok_or(
                    TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PropertyAccessExpression,
                        field: "name",
                    },
                )?;
                let NodeData::Identifier(name) = self.context.arena().node(name)?.data.clone()
                else {
                    return Ok(None);
                };
                Some((Some(name.text), None))
            }
            NodeData::ElementAccessExpression(data)
                if self.property_receiver_is_super(data.expression)? =>
            {
                Some((None, data.argument_expression))
            }
            _ => None,
        };
        let Some((property_name, argument_expression)) = access else {
            return Ok(None);
        };
        let Some(bindings) = self.static_binding_frames.active() else {
            return Ok(None);
        };
        if bindings.super_policy == StaticSuperPolicy::InvalidLegacyDecorated {
            let class_receiver = match bindings.receiver {
                StaticReceiver::Bound(receiver) => Some(receiver),
                StaticReceiver::InvalidLegacyDecorated => None,
            };
            return Ok(Some(StaticSuperAccessResolution::InvalidLegacyDecorated {
                class_receiver,
            }));
        }
        let StaticReceiver::Bound(class_receiver) = bindings.receiver else {
            return Ok(None);
        };
        // A lexical-super flag can reach a base class from invalid source or
        // from a nested class inside a relocated static block. tsc creates the
        // super-base temp only while visiting an actual `extends` expression;
        // without that binding this transform does not own the Reflect
        // rewrite and must leave the access to the remaining pipeline.
        let Some(super_alias) = bindings.super_alias else {
            return Ok(None);
        };
        let key = match property_name {
            Some(property_name) => self.create_string_literal(&property_name)?,
            None => self.visit_required(
                argument_expression,
                SyntaxKind::ElementAccessExpression,
                "argument_expression",
            )?,
        };
        Ok(Some(StaticSuperAccessResolution::Bound(
            StaticSuperAccess {
                super_alias,
                class_receiver,
                key,
            },
        )))
    }

    fn create_static_super_get(
        &mut self,
        access: &StaticSuperAccess,
    ) -> Result<TransformNode, TransformError> {
        let key = self.context.factory()?.clone_node(access.key)?;
        self.create_reflect_get(&access.super_alias, key, &access.class_receiver)
    }

    fn create_static_super_set(
        &mut self,
        access: &StaticSuperAccess,
        value: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let key = self.context.factory()?.clone_node(access.key)?;
        self.create_reflect_set(&access.super_alias, key, value, &access.class_receiver)
    }

    /// A key used for both `Reflect.get` and `Reflect.set` must be stabilized
    /// before the value expression. The returned pair is `(getter, setter)`;
    /// for a complex key the setter side owns the initializing assignment,
    /// matching JavaScript argument evaluation order.
    fn split_static_super_key_for_read_write(
        &mut self,
        key: TransformNode,
    ) -> Result<(TransformNode, TransformNode), TransformError> {
        if self.is_simple_inlineable_expression(key)? {
            return Ok((self.context.factory()?.clone_node(key)?, key));
        }
        let binding = self.allocate_shadowable_temp_name()?;
        let getter_key = self.create_binding_identifier(&binding)?;
        let setter_target = self.create_binding_identifier(&binding)?;
        let setter_key = self.create_assignment(setter_target, key)?;
        Ok((getter_key, setter_key))
    }

    fn lower_static_super_assignment(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::BinaryExpressionData,
        operator: SyntaxKind,
        value_use: ExpressionValueUse,
        access: StaticSuperAccess,
    ) -> Result<NodeId, TransformError> {
        let right = self.visit_required(data.right, SyntaxKind::BinaryExpression, "right")?;
        let (getter_key, setter_key) = if operator == SyntaxKind::EqualsToken {
            (None, access.key)
        } else {
            let (getter_key, setter_key) =
                self.split_static_super_key_for_read_write(access.key)?;
            (Some(getter_key), setter_key)
        };
        let mut value = if let Some(getter_key) = getter_key {
            let current =
                self.create_reflect_get(&access.super_alias, getter_key, &access.class_receiver)?;
            self.set_original_and_range(
                current,
                data.left.map(|left| self.node(left)).unwrap_or(original),
            )?;
            let binary_operator = Self::non_assignment_operator(operator);
            let right = self.parenthesize_right_binary_operand(binary_operator, right)?;
            self.create_binary(current, binary_operator, right)?
        } else {
            right
        };
        let result_binding = (value_use == ExpressionValueUse::Required)
            .then(|| self.allocate_shadowable_temp_name())
            .transpose()?;
        if let Some(binding) = &result_binding {
            let result_target = self.create_binding_identifier(binding)?;
            value = self.create_assignment(result_target, value)?;
        }
        let mut expression = self.create_reflect_set(
            &access.super_alias,
            setter_key,
            value,
            &access.class_receiver,
        )?;
        expression = self.set_original_and_range(expression, original)?;
        if let Some(binding) = &result_binding {
            let result = self.create_binding_identifier(binding)?;
            expression = self.inline_expressions(vec![expression, result])?;
            self.context
                .factory()?
                .set_text_range(expression, original)?;
        }
        Ok(expression.node())
    }

    fn lower_static_super_update(
        &mut self,
        original: TransformNode,
        operator: SyntaxKind,
        is_prefix: bool,
        value_use: ExpressionValueUse,
        access: StaticSuperAccess,
    ) -> Result<NodeId, TransformError> {
        let (getter_key, setter_key) = self.split_static_super_key_for_read_write(access.key)?;
        let current =
            self.create_reflect_get(&access.super_alias, getter_key, &access.class_receiver)?;
        let result_binding = (value_use == ExpressionValueUse::Required)
            .then(|| self.allocate_shadowable_temp_name())
            .transpose()?;
        let update_binding = self.allocate_shadowable_temp_name()?;
        let update_target = self.create_binding_identifier(&update_binding)?;
        let mut value = self.create_assignment(update_target, current)?;

        let update_operand = self.create_binding_identifier(&update_binding)?;
        let mut operation = if is_prefix {
            self.context.factory()?.create_node(
                self.source,
                NodeData::PrefixUnaryExpression(tsc_syntax::nodes::PrefixUnaryExpressionData {
                    operator,
                    operand: Some(update_operand.node()),
                }),
                TransformFlags::NONE,
            )?
        } else {
            self.context.factory()?.create_node(
                self.source,
                NodeData::PostfixUnaryExpression(tsc_syntax::nodes::PostfixUnaryExpressionData {
                    operand: Some(update_operand.node()),
                    operator,
                }),
                TransformFlags::NONE,
            )?
        };
        self.context
            .factory()?
            .set_text_range(operation, original)?;
        if let Some(binding) = &result_binding {
            let result_target = self.create_binding_identifier(binding)?;
            operation = self.create_assignment(result_target, operation)?;
        }
        value = self.inline_expressions(vec![value, operation])?;
        if !is_prefix {
            let updated_value = self.create_binding_identifier(&update_binding)?;
            value = self.inline_expressions(vec![value, updated_value])?;
        }
        let value = self.create_parenthesized(value)?;
        let mut expression = self.create_reflect_set(
            &access.super_alias,
            setter_key,
            value,
            &access.class_receiver,
        )?;
        expression = self.set_original_and_range(expression, original)?;
        if let Some(binding) = &result_binding {
            let result = self.create_binding_identifier(binding)?;
            expression = self.inline_expressions(vec![expression, result])?;
            self.context
                .factory()?
                .set_text_range(expression, original)?;
        }
        Ok(expression.node())
    }

    fn private_access_target(
        &self,
        target: Option<NodeId>,
    ) -> Result<Option<(TransformNode, PrivateSlot)>, TransformError> {
        let Some(target) = target else {
            return Ok(None);
        };
        let NodeData::PropertyAccessExpression(access) =
            self.context.arena().node(self.node(target))?.data.clone()
        else {
            return Ok(None);
        };
        let Some(name) = access.name.map(|name| self.node(name)) else {
            return Ok(None);
        };
        if self.private_name_text(name).is_none() {
            return Ok(None);
        }
        let receiver = access
            .expression
            .map(|receiver| self.node(receiver))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyAccessExpression,
                field: "expression",
            })?;
        let Some(slot) = self.private_slot_if_declared(name)?.cloned() else {
            return Ok(None);
        };
        Ok(Some((receiver, slot)))
    }

    const fn non_assignment_operator(operator: SyntaxKind) -> SyntaxKind {
        match operator {
            SyntaxKind::PlusEqualsToken => SyntaxKind::PlusToken,
            SyntaxKind::MinusEqualsToken => SyntaxKind::MinusToken,
            SyntaxKind::AsteriskEqualsToken => SyntaxKind::AsteriskToken,
            SyntaxKind::AsteriskAsteriskEqualsToken => SyntaxKind::AsteriskAsteriskToken,
            SyntaxKind::SlashEqualsToken => SyntaxKind::SlashToken,
            SyntaxKind::PercentEqualsToken => SyntaxKind::PercentToken,
            SyntaxKind::LessThanLessThanEqualsToken => SyntaxKind::LessThanLessThanToken,
            SyntaxKind::GreaterThanGreaterThanEqualsToken => {
                SyntaxKind::GreaterThanGreaterThanToken
            }
            SyntaxKind::GreaterThanGreaterThanGreaterThanEqualsToken => {
                SyntaxKind::GreaterThanGreaterThanGreaterThanToken
            }
            SyntaxKind::AmpersandEqualsToken => SyntaxKind::AmpersandToken,
            SyntaxKind::BarEqualsToken => SyntaxKind::BarToken,
            SyntaxKind::BarBarEqualsToken => SyntaxKind::BarBarToken,
            SyntaxKind::AmpersandAmpersandEqualsToken => SyntaxKind::AmpersandAmpersandToken,
            SyntaxKind::QuestionQuestionEqualsToken => SyntaxKind::QuestionQuestionToken,
            SyntaxKind::CaretEqualsToken => SyntaxKind::CaretToken,
            _ => operator,
        }
    }

    fn parenthesize_right_binary_operand(
        &mut self,
        operator: SyntaxKind,
        operand: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if self.context.arena().node(operand)?.kind == SyntaxKind::ParenthesizedExpression {
            return Ok(operand);
        }
        let operator_precedence = Self::binary_precedence(operator);
        let operand_kind = self.context.arena().node(operand)?.kind;
        let operand_operator = match &self.context.arena().node(operand)?.data {
            NodeData::BinaryExpression(data) => data
                .operator_token
                .map(|token| {
                    self.context
                        .arena()
                        .node(self.node(token))
                        .map(|node| node.kind)
                })
                .transpose()?,
            _ => None,
        };
        let mixes_coalesce = operand_operator.is_some_and(|operand_operator| {
            (operator == SyntaxKind::QuestionQuestionToken
                && matches!(
                    operand_operator,
                    SyntaxKind::AmpersandAmpersandToken | SyntaxKind::BarBarToken
                ))
                || (operand_operator == SyntaxKind::QuestionQuestionToken
                    && matches!(
                        operator,
                        SyntaxKind::AmpersandAmpersandToken | SyntaxKind::BarBarToken
                    ))
        });
        let operand_precedence = self.expression_precedence(operand)?;
        let needs_parentheses = mixes_coalesce
            || (operand_kind == SyntaxKind::ArrowFunction && operator_precedence > 3)
            || operand_precedence < operator_precedence
            || (operand_precedence == operator_precedence
                && !operand_operator.is_some_and(|operand_operator| {
                    operand_operator == operator
                        && matches!(
                            operator,
                            SyntaxKind::AsteriskToken
                                | SyntaxKind::BarToken
                                | SyntaxKind::AmpersandToken
                                | SyntaxKind::CaretToken
                                | SyntaxKind::CommaToken
                        )
                })
                && !operand_operator.is_some_and(|operand_operator| {
                    operand_operator == SyntaxKind::AsteriskAsteriskToken
                }));
        if needs_parentheses {
            self.create_parenthesized(operand)
        } else {
            Ok(operand)
        }
    }

    fn expression_precedence(&self, expression: TransformNode) -> Result<u8, TransformError> {
        let node = self.context.arena().node(expression)?;
        Ok(match &node.data {
            NodeData::CommaListExpression(_) => 0,
            NodeData::SpreadElement(_) => 1,
            NodeData::YieldExpression(_) => 2,
            NodeData::BinaryExpression(data) => data
                .operator_token
                .map(|token| {
                    self.context
                        .arena()
                        .node(self.node(token))
                        .map(|token| Self::binary_precedence(token.kind))
                })
                .transpose()?
                .unwrap_or(0),
            NodeData::ConditionalExpression(_) => 4,
            NodeData::AsExpression(_) | NodeData::SatisfiesExpression(_) => 11,
            NodeData::PrefixUnaryExpression(_)
            | NodeData::TypeOfExpression(_)
            | NodeData::VoidExpression(_)
            | NodeData::DeleteExpression(_)
            | NodeData::AwaitExpression(_) => 16,
            NodeData::PostfixUnaryExpression(_) => 17,
            NodeData::CallExpression(_) => 18,
            NodeData::NewExpression(data) => {
                if data.arguments.is_some() {
                    19
                } else {
                    18
                }
            }
            NodeData::TaggedTemplateExpression(_)
            | NodeData::PropertyAccessExpression(_)
            | NodeData::ElementAccessExpression(_)
            | NodeData::MetaProperty(_) => 19,
            _ => 20,
        })
    }

    const fn binary_precedence(operator: SyntaxKind) -> u8 {
        match operator {
            SyntaxKind::CommaToken => 0,
            SyntaxKind::EqualsToken
            | SyntaxKind::PlusEqualsToken
            | SyntaxKind::MinusEqualsToken
            | SyntaxKind::AsteriskEqualsToken
            | SyntaxKind::AsteriskAsteriskEqualsToken
            | SyntaxKind::SlashEqualsToken
            | SyntaxKind::PercentEqualsToken
            | SyntaxKind::LessThanLessThanEqualsToken
            | SyntaxKind::GreaterThanGreaterThanEqualsToken
            | SyntaxKind::GreaterThanGreaterThanGreaterThanEqualsToken
            | SyntaxKind::AmpersandEqualsToken
            | SyntaxKind::BarEqualsToken
            | SyntaxKind::BarBarEqualsToken
            | SyntaxKind::AmpersandAmpersandEqualsToken
            | SyntaxKind::QuestionQuestionEqualsToken
            | SyntaxKind::CaretEqualsToken => 3,
            SyntaxKind::QuestionQuestionToken | SyntaxKind::BarBarToken => 5,
            SyntaxKind::AmpersandAmpersandToken => 6,
            SyntaxKind::BarToken => 7,
            SyntaxKind::CaretToken => 8,
            SyntaxKind::AmpersandToken => 9,
            SyntaxKind::EqualsEqualsToken
            | SyntaxKind::ExclamationEqualsToken
            | SyntaxKind::EqualsEqualsEqualsToken
            | SyntaxKind::ExclamationEqualsEqualsToken => 10,
            SyntaxKind::LessThanToken
            | SyntaxKind::GreaterThanToken
            | SyntaxKind::LessThanEqualsToken
            | SyntaxKind::GreaterThanEqualsToken
            | SyntaxKind::InstanceOfKeyword
            | SyntaxKind::InKeyword
            | SyntaxKind::AsKeyword
            | SyntaxKind::SatisfiesKeyword => 11,
            SyntaxKind::LessThanLessThanToken
            | SyntaxKind::GreaterThanGreaterThanToken
            | SyntaxKind::GreaterThanGreaterThanGreaterThanToken => 12,
            SyntaxKind::PlusToken | SyntaxKind::MinusToken => 13,
            SyntaxKind::AsteriskToken | SyntaxKind::SlashToken | SyntaxKind::PercentToken => 14,
            SyntaxKind::AsteriskAsteriskToken => 15,
            _ => 0,
        }
    }

    fn plan_members(
        &mut self,
        members: Option<NodeArrayId>,
    ) -> Result<ClassOperations, TransformError> {
        let mut operations = ClassOperations::default();
        let mut ordinary_private_storages = Vec::new();
        let mut generated_auto_accessor_storages = Vec::new();
        if let Some(environment) = self.private_environments.last() {
            for declaration in &environment.declarations {
                let slot = environment
                    .private_slots
                    .get(declaration.slot_index)
                    .expect("private declaration slot index remains valid");
                if slot.is_static() || !matches!(&slot.element, PrivateElement::Field { .. }) {
                    continue;
                }
                if self
                    .generated_auto_accessor_backings
                    .contains(&declaration.declaration.node())
                {
                    generated_auto_accessor_storages.push(slot.clone());
                } else {
                    ordinary_private_storages.push(slot.clone());
                }
            }
        }
        let instance_brand = self
            .private_environments
            .last()
            .and_then(|environment| environment.instance_brand.clone());
        if let Some(instance_brand) = instance_brand.as_ref() {
            operations
                .instance
                .push(InstanceOperation::PrivateBrand(instance_brand.clone()));
        }
        operations.pending = ClassPendingPlan::from_setup_prefix(
            ordinary_private_storages,
            instance_brand,
            generated_auto_accessor_storages,
        );
        for member in self.array_nodes(members)? {
            let record = self.context.arena().node(member)?.clone();
            match record.data {
                NodeData::PropertyDeclaration(mut data)
                    if self.name_is_private(data.name)?
                        && !self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?
                        && self
                            .should_transform_private_class_element(member, data.modifiers)? =>
                {
                    let private_name = data.name.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PropertyDeclaration,
                        field: "private name",
                    })?;
                    let slot = self.private_slot(self.node(private_name))?.clone();
                    let is_static_member =
                        self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)?;
                    if !slot.is_valid {
                        // tsc retains every declaration that resolves to an
                        // invalid duplicate entry, while still generating its
                        // constructor/static recovery initializer below.
                        operations.retained_members.push(member);
                    }
                    if !matches!(&slot.element, PrivateElement::Field { .. }) {
                        continue;
                    }
                    // Instance initializers execute in the constructor. Their
                    // nested generated names must therefore be allocated in
                    // the constructor scope when the operation is
                    // materialized, not while the class-level plan is built.
                    if is_static_member {
                        data.initializer = self.visit_optional_static_node(data.initializer)?;
                    }
                    let operation = PrivateFieldOperation {
                        original: member,
                        slot: slot.clone(),
                        initializer: data.initializer,
                    };
                    if is_static_member {
                        if self.selectively_transforms_private_static_elements() {
                            operations
                                .retained_members
                                .push(self.materialize_private_static_field_block(&operation)?);
                        } else {
                            operations
                                .static_
                                .push(StaticOperation::PrivateField(Box::new(operation)));
                        }
                    } else {
                        operations
                            .instance
                            .push(InstanceOperation::PrivateField(operation));
                    }
                }
                NodeData::MethodDeclaration(data)
                    if self.name_is_private(data.name)?
                        && self
                            .should_transform_private_class_element(member, data.modifiers)? =>
                {
                    let name = data.name.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::MethodDeclaration,
                        field: "private name",
                    })?;
                    let slot = self.private_slot(self.node(name))?.clone();
                    if !slot.is_valid {
                        operations.retained_members.push(member);
                        continue;
                    }
                    let PrivateElement::Method { method_name } = &slot.element else {
                        return Err(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::MethodDeclaration,
                            field: "private method slot",
                        });
                    };
                    let function =
                        self.create_private_method_function(member, data, method_name)?;
                    operations
                        .pending
                        .append_private_definition(PrivateDefinition {
                            name: method_name.clone(),
                            function,
                        });
                }
                NodeData::GetAccessor(data)
                    if self.name_is_private(data.name)?
                        && self
                            .should_transform_private_class_element(member, data.modifiers)? =>
                {
                    let name = data.name.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::GetAccessor,
                        field: "private name",
                    })?;
                    let slot = self.private_slot(self.node(name))?.clone();
                    if !slot.is_valid {
                        operations.retained_members.push(member);
                        continue;
                    }
                    let PrivateElement::Accessor { getter_name, .. } = &slot.element else {
                        return Err(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::GetAccessor,
                            field: "private accessor slot",
                        });
                    };
                    let function_name =
                        getter_name
                            .as_ref()
                            .ok_or(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::GetAccessor,
                                field: "private getter binding",
                            })?;
                    let function = if self
                        .generated_static_auto_accessors
                        .contains(&member.node())
                    {
                        self.with_static_auto_accessor_bindings(|visitor| {
                            visitor.create_private_getter_function(member, data, function_name)
                        })?
                    } else {
                        self.create_private_getter_function(member, data, function_name)?
                    };
                    operations
                        .pending
                        .append_private_definition(PrivateDefinition {
                            name: function_name.clone(),
                            function,
                        });
                }
                NodeData::SetAccessor(data)
                    if self.name_is_private(data.name)?
                        && self
                            .should_transform_private_class_element(member, data.modifiers)? =>
                {
                    let name = data.name.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::SetAccessor,
                        field: "private name",
                    })?;
                    let slot = self.private_slot(self.node(name))?.clone();
                    if !slot.is_valid {
                        operations.retained_members.push(member);
                        continue;
                    }
                    let PrivateElement::Accessor { setter_name, .. } = &slot.element else {
                        return Err(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::SetAccessor,
                            field: "private accessor slot",
                        });
                    };
                    let function_name =
                        setter_name
                            .as_ref()
                            .ok_or(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::SetAccessor,
                                field: "private setter binding",
                            })?;
                    let function = if self
                        .generated_static_auto_accessors
                        .contains(&member.node())
                    {
                        self.with_static_auto_accessor_bindings(|visitor| {
                            visitor.create_private_setter_function(member, data, function_name)
                        })?
                    } else {
                        self.create_private_setter_function(member, data, function_name)?
                    };
                    operations
                        .pending
                        .append_private_definition(PrivateDefinition {
                            name: function_name.clone(),
                            function,
                        });
                }
                NodeData::PropertyDeclaration(mut data)
                    if !self.name_is_private(data.name)?
                        && !self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?
                        && (!self.selectively_transforms_private_static_elements()
                            || self.mode == PublicFieldMode::Assignment) =>
                {
                    let receiver =
                        if self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)? {
                            FieldReceiver::Static
                        } else {
                            FieldReceiver::Instance
                        };
                    let initializer_needs_assigned_name = data
                        .initializer
                        .map(|initializer| {
                            self.anonymous_class_initializer_needs_assigned_name(initializer)
                        })
                        .transpose()?
                        .unwrap_or(false);
                    let should_capture_key =
                        data.initializer.is_some() || self.mode == PublicFieldMode::DefineProperty;
                    let original_name = data.name.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PropertyDeclaration,
                        field: "name",
                    })?;
                    let planned_name =
                        self.plan_public_field_name(original_name, should_capture_key)?;
                    if let Some(assigned_class_name) = planned_name.assigned_class_name.clone() {
                        if initializer_needs_assigned_name
                            && matches!(&assigned_class_name, AssignedClassName::Evaluated(_))
                        {
                            // Named evaluation computes ToPropertyKey before
                            // class-fields lowering consumes the generated
                            // cache. tsc's later class-key planner retains the
                            // helper request even though the final key setup
                            // can use the already-owned raw expression.
                            self.context
                                .request_emit_helper(super::super::helpers::prop_key())?;
                        }
                        self.assigned_class_names
                            .insert(original_name, assigned_class_name);
                    }
                    data.name = Some(planned_name.name);
                    if let Some(evaluation) = planned_name.evaluation {
                        let evaluations = self.flatten_class_pending_comma_list(evaluation)?;
                        operations
                            .pending
                            .append_public_field_key_operands(evaluations);
                    }
                    if receiver == FieldReceiver::Static {
                        data.initializer = self.visit_optional_static_node(data.initializer)?;
                    }
                    let parameter_property_local = if receiver == FieldReceiver::Instance {
                        self.parameter_property_local(member)?
                    } else {
                        None
                    };
                    let name = data.name.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PropertyDeclaration,
                        field: "name",
                    })?;
                    let value = if let Some(local) = parameter_property_local {
                        FieldValuePlan::ParameterProperty {
                            prefix: data.initializer,
                            local,
                        }
                    } else {
                        FieldValuePlan::Declared {
                            initializer: data.initializer,
                        }
                    };
                    let operation = FieldOperation {
                        original: member,
                        receiver,
                        name,
                        value,
                        range_static_expression_to_name: receiver == FieldReceiver::Static
                            && self
                                .private_environments
                                .last()
                                .is_some_and(|environment| environment.is_legacy_decorated),
                    };
                    match receiver {
                        FieldReceiver::Instance => {
                            if self.mode == PublicFieldMode::DefineProperty
                                || operation.value.has_runtime_value()
                            {
                                operations
                                    .instance
                                    .push(InstanceOperation::Public(operation));
                            }
                        }
                        FieldReceiver::Static => {
                            // Downlevel class-fields emit omits uninitialized
                            // static declarations in both assignment and
                            // define modes. Instance define-mode fields remain
                            // observable own properties and are handled above.
                            if operation.value.has_runtime_value() {
                                operations.static_.push(StaticOperation::Field(operation));
                            }
                        }
                    }
                }
                NodeData::ClassStaticBlockDeclaration(data)
                    if !self.selectively_transforms_private_static_elements() =>
                {
                    let body = data
                        .body
                        .and_then(|body| self.context.arena().node_ref(self.source, body))
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::ClassStaticBlockDeclaration,
                            field: "body",
                        })?;
                    if self
                        .context
                        .arena()
                        .metadata(member)
                        .and_then(|metadata| metadata.class_this)
                        .is_some()
                    {
                        // The surrounding class assignment initializes this
                        // explicit constructor binding. The synthetic block
                        // only transports that ownership across passes.
                        continue;
                    }
                    if self
                        .context
                        .arena()
                        .metadata(member)
                        .and_then(|metadata| metadata.assigned_name)
                        .is_some()
                    {
                        let NodeData::Block(body_data) =
                            self.context.arena().node(body)?.data.clone()
                        else {
                            return Err(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::ClassStaticBlockDeclaration,
                                field: "named-evaluation block body",
                            });
                        };
                        let statements = self.array_nodes(body_data.statements)?;
                        let [statement] = statements.as_slice() else {
                            return Err(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::ClassStaticBlockDeclaration,
                                field: "named-evaluation statement",
                            });
                        };
                        let NodeData::ExpressionStatement(statement_data) =
                            self.context.arena().node(*statement)?.data.clone()
                        else {
                            return Err(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::ClassStaticBlockDeclaration,
                                field: "named-evaluation expression statement",
                            });
                        };
                        let expression = statement_data.expression.ok_or(
                            TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::ExpressionStatement,
                                field: "named-evaluation expression",
                            },
                        )?;
                        let expression = self.visit_static_node(expression)?.ok_or(
                            TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::ExpressionStatement,
                                field: "visited named-evaluation expression",
                            },
                        )?;
                        operations.static_.push(StaticOperation::NamedEvaluation {
                            original: Some(member),
                            expression,
                        });
                        continue;
                    }
                    let (visited, bindings) = self.with_new_generated_scope(
                        GeneratedBindingOwner::StaticEvaluation,
                        |visitor| visitor.visit_static_node(body.node()),
                    )?;
                    let visited = visited.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ClassStaticBlockDeclaration,
                        field: "body",
                    })?;
                    let visited =
                        self.prepend_generated_declarations_to_block(visited, bindings)?;
                    operations.static_.push(StaticOperation::Block {
                        original: member,
                        body: visited,
                    });
                }
                data => {
                    let data = self.strip_accessor_modifier_from_class_member(data)?;
                    let updated = if self
                        .generated_static_auto_accessors
                        .contains(&member.node())
                    {
                        self.with_static_auto_accessor_bindings(|visitor| {
                            visitor.update_generic(member, data)
                        })?
                    } else {
                        self.visit_retained_class_member(member, data)?
                    };
                    let updated = self.inject_pending_expressions_into_member(
                        self.node(updated),
                        &mut operations.pending,
                    )?;
                    operations.retained_members.push(updated);
                }
            }
        }
        Ok(operations)
    }

    /// tsc's visitComputedPropertyName drains the current pending-expression
    /// prefix into the next retained computed member. Keeping this ownership
    /// in the class plan preserves source evaluation order without a
    /// transformer-wide mutable channel.
    ///
    /// tsc-port: injectPendingExpressions @6.0.3
    /// tsc-hash: 5ba282b28c8f6b724f359b12b848c573fa0c2218cd12f4619475d3d22596d54e
    /// tsc-span: _tsc.js:96167-96179
    ///
    /// tsc-port: visitComputedPropertyName @6.0.3
    /// tsc-hash: c82affe1bb42c8ede4f24eac8cbb1eec3bcc5bfaff55984ec555bc3ca2fafe3b
    /// tsc-span: _tsc.js:96180-96183
    fn inject_pending_expressions_into_member(
        &mut self,
        member: TransformNode,
        pending: &mut ClassPendingPlan,
    ) -> Result<TransformNode, TransformError> {
        let mut member_data = self.context.arena().node(member)?.data.clone();
        let name = match &member_data {
            NodeData::PropertyDeclaration(data) => data.name,
            NodeData::MethodDeclaration(data) => data.name,
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            _ => None,
        };
        let Some(name) = name.map(|name| self.node(name)) else {
            return Ok(member);
        };
        let NodeData::ComputedPropertyName(mut computed) =
            self.context.arena().node(name)?.data.clone()
        else {
            return Ok(member);
        };
        let Some(expression) = computed.expression.map(|expression| self.node(expression)) else {
            return Ok(member);
        };

        // A computed member name executes during class definition, so it owns
        // every pending expression observed before it. Draining the one typed
        // plan here prevents setup and erased-field keys from being regrouped
        // by later statement/expression owners.
        let mut expressions = self.materialize_class_pending_expressions(pending)?;
        if expressions.is_empty() {
            return Ok(member);
        }
        let injected = if let NodeData::ParenthesizedExpression(mut parenthesized) =
            self.context.arena().node(expression)?.data.clone()
        {
            let inner = parenthesized
                .expression
                .map(|inner| self.node(inner))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ParenthesizedExpression,
                    field: "computed member expression",
                })?;
            expressions.push(inner);
            parenthesized.expression = Some(self.inline_expressions(expressions)?.node());
            let data = NodeData::ParenthesizedExpression(parenthesized);
            let flags = flags_after_update(self.context.arena(), expression, &data)?;
            self.context
                .factory()?
                .update_node(expression, data, flags)?
        } else {
            expressions.push(expression);
            self.inline_expressions(expressions)?
        };
        computed.expression = Some(injected.node());
        let computed_data = NodeData::ComputedPropertyName(computed);
        let computed_flags = flags_after_update(self.context.arena(), name, &computed_data)?;
        let name = self
            .context
            .factory()?
            .update_node(name, computed_data, computed_flags)?
            .node();

        match &mut member_data {
            NodeData::PropertyDeclaration(data) => data.name = Some(name),
            NodeData::MethodDeclaration(data) => data.name = Some(name),
            NodeData::GetAccessor(data) => data.name = Some(name),
            NodeData::SetAccessor(data) => data.name = Some(name),
            _ => unreachable!("computed class member kind was matched above"),
        }
        let flags = flags_after_update(self.context.arena(), member, &member_data)?;
        self.context
            .factory()?
            .update_node(member, member_data, flags)
    }

    /// Preserve tsc's pending-expression granularity. In particular, a
    /// statement-owning decorated class must emit `a(); b();`, while a cached
    /// key assignment such as `_a = (a(), b())` remains one event.
    ///
    /// tsc-port: flattenCommaListWorker @6.0.3
    /// tsc-hash: a879551d103899488f8f2dbe2ca28ab980ecb860b8871916a3ad6958c3d274d2
    /// tsc-span: _tsc.js:28218-28231
    ///
    /// tsc-port: flattenCommaList @6.0.3
    /// tsc-hash: 54ff61c10aeb5c3e6cbf988835f018eff0271a90a921dec763606dbc907f86cb
    /// tsc-span: _tsc.js:28232-28236
    fn flatten_class_pending_comma_list(
        &self,
        expression: TransformNode,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let mut expressions = Vec::new();
        self.flatten_class_pending_comma_list_worker(expression, &mut expressions)?;
        Ok(expressions)
    }

    fn flatten_class_pending_comma_list_worker(
        &self,
        expression: TransformNode,
        expressions: &mut Vec<TransformNode>,
    ) -> Result<(), TransformError> {
        let record = self.context.arena().node(expression)?.clone();
        match record.data {
            NodeData::ParenthesizedExpression(data)
                if self.context.arena().metadata(expression).is_none()
                    && matches!(
                        SourceRange::from_raw(
                            record.pos,
                            record.end,
                            self.context
                                .arena()
                                .source(expression.source())?
                                .syntax()
                                .positions(),
                        )
                        .map_err(|error| {
                            TransformError::InvalidSourceRange {
                                node: expression,
                                error,
                            }
                        })?,
                        SourceRange::Synthesized
                    ) =>
            {
                let inner = data.expression.map(|inner| self.node(inner)).ok_or(
                    TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ParenthesizedExpression,
                        field: "expression",
                    },
                )?;
                self.flatten_class_pending_comma_list_worker(inner, expressions)?;
            }
            NodeData::BinaryExpression(data)
                if data
                    .operator_token
                    .and_then(|operator| self.context.arena().node_ref(self.source, operator))
                    .map(|operator| self.context.arena().node(operator).map(|node| node.kind))
                    .transpose()?
                    == Some(SyntaxKind::CommaToken) =>
            {
                let left = data.left.map(|left| self.node(left)).ok_or(
                    TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::BinaryExpression,
                        field: "left",
                    },
                )?;
                let right = data.right.map(|right| self.node(right)).ok_or(
                    TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::BinaryExpression,
                        field: "right",
                    },
                )?;
                self.flatten_class_pending_comma_list_worker(left, expressions)?;
                self.flatten_class_pending_comma_list_worker(right, expressions)?;
            }
            NodeData::CommaListExpression(data) => {
                if let Some(elements) = data
                    .elements
                    .and_then(|elements| self.context.arena().node_array_ref(self.source, elements))
                {
                    let nodes = self.context.arena().node_array(elements)?.nodes.clone();
                    for element in nodes {
                        self.flatten_class_pending_comma_list_worker(
                            self.node(element),
                            expressions,
                        )?;
                    }
                }
            }
            _ => expressions.push(expression),
        }
        Ok(())
    }

    fn strip_accessor_modifier_from_class_member(
        &mut self,
        mut data: NodeData,
    ) -> Result<NodeData, TransformError> {
        match &mut data {
            NodeData::PropertyDeclaration(data) => {
                data.modifiers =
                    self.filter_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?;
            }
            NodeData::MethodDeclaration(data) => {
                data.modifiers =
                    self.filter_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?;
            }
            NodeData::GetAccessor(data) => {
                data.modifiers =
                    self.filter_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?;
            }
            NodeData::SetAccessor(data) => {
                data.modifiers =
                    self.filter_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?;
            }
            NodeData::Constructor(data) => {
                data.modifiers =
                    self.filter_modifier(data.modifiers, SyntaxKind::AccessorKeyword)?;
            }
            _ => {}
        }
        Ok(data)
    }

    fn visit_retained_class_member(
        &mut self,
        original: TransformNode,
        data: NodeData,
    ) -> Result<NodeId, TransformError> {
        match data {
            NodeData::MethodDeclaration(data) => {
                self.visit_function_scope(original, NodeData::MethodDeclaration(data), false)
            }
            NodeData::GetAccessor(data) => {
                self.visit_function_scope(original, NodeData::GetAccessor(data), false)
            }
            NodeData::SetAccessor(data) => {
                self.visit_function_scope(original, NodeData::SetAccessor(data), false)
            }
            NodeData::Constructor(data) => {
                self.visit_function_scope(original, NodeData::Constructor(data), false)
            }
            data => self.update_generic(original, data),
        }
    }

    fn create_private_method_function(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::MethodDeclarationData,
        function_name: &ClassBinding,
    ) -> Result<TransformNode, TransformError> {
        self.create_private_function(
            original,
            function_name,
            data.type_parameters,
            data.parameters,
            data.r#type,
            data.asterisk_token,
            data.body,
            data.modifiers,
        )
    }

    fn create_private_getter_function(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::GetAccessorData,
        function_name: &ClassBinding,
    ) -> Result<TransformNode, TransformError> {
        self.create_private_function(
            original,
            function_name,
            data.type_parameters,
            data.parameters,
            data.r#type,
            None,
            data.body,
            data.modifiers,
        )
    }

    fn create_private_setter_function(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::SetAccessorData,
        function_name: &ClassBinding,
    ) -> Result<TransformNode, TransformError> {
        self.create_private_function(
            original,
            function_name,
            data.type_parameters,
            data.parameters,
            data.r#type,
            None,
            data.body,
            data.modifiers,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn create_private_function(
        &mut self,
        original: TransformNode,
        function_name: &ClassBinding,
        type_parameters: Option<NodeArrayId>,
        parameters: Option<NodeArrayId>,
        r#type: Option<NodeId>,
        asterisk_token: Option<NodeId>,
        body: Option<NodeId>,
        modifiers: Option<NodeArrayId>,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_binding_identifier(function_name)?;
        let type_parameters = self.visit_optional_nodes(type_parameters)?;
        let parameters = self.visit_optional_nodes(parameters)?;
        let r#type = self.visit_optional_node(r#type)?;
        let asterisk_token = self.visit_optional_node(asterisk_token)?;
        let body = self.visit_optional_node(body)?;
        let modifiers = self.visit_function_modifiers(modifiers)?;
        let function = self.context.factory()?.create_node(
            self.source,
            NodeData::FunctionExpression(tsc_syntax::nodes::FunctionExpressionData {
                name: Some(name.node()),
                type_parameters,
                parameters,
                r#type,
                asterisk_token,
                body,
                modifiers,
            }),
            TransformFlags::NONE,
        )?;
        // Later Rust transforms project resolver queries back into the
        // checker-owned parse tree. Keep that semantic provenance without
        // copying the erased member's text/comment range onto this synthetic
        // function shell.
        self.context
            .arena_mut()?
            .set_semantic_original_node(function, original)?;
        Ok(function)
    }

    fn visit_function_modifiers(
        &mut self,
        modifiers: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let mut retained = Vec::new();
        for modifier in self.array_nodes(modifiers)? {
            if matches!(
                self.context.arena().node(modifier)?.kind,
                SyntaxKind::StaticKeyword | SyntaxKind::AccessorKeyword
            ) {
                continue;
            }
            if let Some(modifier) = self.visit(modifier.node())? {
                retained.push(self.node(modifier));
            }
        }
        if retained.is_empty() {
            Ok(None)
        } else {
            Ok(Some(
                self.context
                    .factory()?
                    .create_node_array(self.source, retained)?
                    .array(),
            ))
        }
    }

    /// tsc-port: transformConstructorBody @6.0.3
    /// tsc-hash: ed62e2b9ac66528ca42730f1e550c81010c83d3b99a605bf1a1d66a4ed64667d
    /// tsc-span: _tsc.js:97329-97365
    fn install_instance_operations(
        &mut self,
        members: &mut Vec<TransformNode>,
        operations: &[InstanceOperation],
        derived: bool,
        class_name: Option<&str>,
        class: TransformNode,
        member_range: Option<NodeArrayId>,
    ) -> Result<(), TransformError> {
        // Private-brand setup precedes parameter properties, and parameter
        // properties precede ordinary field initializers regardless of the
        // synthetic member order produced by transformTypeScript.
        let is_parameter_property = |operation: &&InstanceOperation| {
            matches!(
                operation,
                InstanceOperation::Public(operation)
                    if operation.value.is_parameter_property()
            )
        };
        let mut ordered_operations = Vec::with_capacity(operations.len());
        ordered_operations.extend(
            operations
                .iter()
                .filter(|operation| matches!(operation, InstanceOperation::PrivateBrand(_))),
        );
        ordered_operations.extend(operations.iter().filter(is_parameter_property));
        ordered_operations.extend(operations.iter().filter(|operation| {
            !matches!(operation, InstanceOperation::PrivateBrand(_))
                && !is_parameter_property(operation)
        }));
        let class_binding = class_name.map(ClassBinding::existing);
        let (statements, bindings) =
            self.with_new_generated_scope(GeneratedBindingOwner::FunctionBody, |visitor| {
                let mut statements = Vec::with_capacity(ordered_operations.len());
                for operation in ordered_operations {
                    statements.push(match operation {
                        InstanceOperation::PrivateBrand(brand) => {
                            visitor.materialize_private_brand(brand)?
                        }
                        InstanceOperation::Public(operation) => {
                            let mut operation = operation.clone();
                            operation.value = visitor.visit_field_value_plan(operation.value)?;
                            visitor
                                .materialize_field_operation(&operation, class_binding.as_ref())?
                        }
                        InstanceOperation::PrivateField(operation) => {
                            let mut operation = operation.clone();
                            operation.initializer =
                                visitor.visit_optional_node(operation.initializer)?;
                            visitor.materialize_private_instance_field(&operation)?
                        }
                    });
                }
                Ok(statements)
            })?;
        let constructor = members.iter().position(|member| {
            self.context
                .arena()
                .node(*member)
                .is_ok_and(|member| member.kind == SyntaxKind::Constructor)
        });
        let constructor = if let Some(index) = constructor {
            let constructor = self.inject_into_constructor(members[index], &statements)?;
            members[index] = constructor;
            constructor
        } else {
            let constructor =
                self.create_synthetic_constructor(derived, statements, class, member_range)?;
            members.insert(0, constructor);
            constructor
        };
        let constructor = self.install_function_bindings(constructor, bindings, Vec::new())?;
        let index = members
            .iter()
            .position(|member| *member == constructor)
            .or_else(|| {
                members.iter().position(|member| {
                    self.context
                        .arena()
                        .node(*member)
                        .is_ok_and(|member| member.kind == SyntaxKind::Constructor)
                })
            })
            .expect("instance operations always own a constructor");
        members[index] = constructor;
        Ok(())
    }

    /// Materializes only the post-pending static phase. The absence of a
    /// pending-plan parameter makes reordering setup/key effects here
    /// impossible by construction.
    fn materialize_static_operations(
        &mut self,
        class_name: &ClassBinding,
        operations: Vec<StaticOperation>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let mut statements = Vec::with_capacity(operations.len());
        for operation in operations {
            let statement = match operation {
                StaticOperation::Field(operation) => {
                    self.materialize_field_operation(&operation, Some(class_name))?
                }
                StaticOperation::PrivateField(operation) => {
                    if operation.slot.is_static() {
                        self.materialize_private_static_field(&operation)?
                    } else {
                        // A duplicate private declaration replaces the
                        // name-table entry even when its staticness differs
                        // from the member currently being initialized. tsc
                        // still schedules this initializer in the syntactic
                        // static-member phase, but applies the effective
                        // instance-field slot to the class receiver.
                        let receiver = self.create_binding_identifier(class_name)?;
                        self.materialize_private_weak_map_field(&operation, receiver)?
                    }
                }
                StaticOperation::NamedEvaluation {
                    original,
                    expression,
                } => {
                    let statement = self.create_expression_statement(expression)?;
                    if let Some(original) = original {
                        self.set_original_and_range(statement, original)?;
                    }
                    statement
                }
                StaticOperation::Block { original, body } => {
                    let body = self.context.factory()?.set_multi_line(body, true)?;
                    let arrow = self.create_arrow_function(Vec::new(), body)?;
                    let arrow = self.create_parenthesized(arrow)?;
                    let call = self.create_call(arrow, Vec::new())?;
                    let statement = self.create_expression_statement(call)?;
                    self.set_original_and_range(statement, original)?;
                    statement
                }
            };
            statements.push(statement);
        }
        Ok(statements)
    }

    fn materialize_class_pending_expressions(
        &mut self,
        pending: &mut ClassPendingPlan,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let mut expressions = Vec::new();
        for entry in pending.take_entries() {
            let expression = match entry {
                ClassPendingEntry::OrdinaryPrivateFieldStorage(slot)
                | ClassPendingEntry::GeneratedAutoAccessorStorage(slot) => {
                    self.materialize_private_storage(&slot)?
                }
                ClassPendingEntry::InstanceBrand(brand) => {
                    let brand = self.create_binding_identifier(&brand)?;
                    let weak_set = self.create_identifier("WeakSet")?;
                    let weak_set = self.create_new(weak_set, Vec::new())?;
                    self.create_assignment(brand, weak_set)?
                }
                ClassPendingEntry::PrivateDefinition(definition) => {
                    let name = self.create_binding_identifier(&definition.name)?;
                    // tsc leaves both this assignment and its generated
                    // FunctionExpression synthetic. The visited children keep
                    // their provenance, but the erased member's boundary
                    // comments are not relocated to the private definition.
                    self.create_assignment(name, definition.function)?
                }
                ClassPendingEntry::PublicFieldKeyOperand(evaluation) => evaluation,
            };
            expressions.push(expression);
        }
        Ok(expressions)
    }

    fn materialize_class_declaration_pending_statement(
        &mut self,
        pending: &mut ClassPendingPlan,
        alias: Option<&ClassBinding>,
        class: &ClassBinding,
    ) -> Result<Option<TransformNode>, TransformError> {
        let mut expressions = Vec::new();
        if let Some(alias) = alias {
            let alias = self.create_binding_identifier(alias)?;
            let class = self.create_binding_identifier(class)?;
            expressions.push(self.create_assignment(alias, class)?);
        }
        expressions.extend(self.materialize_class_pending_expressions(pending)?);
        if expressions.is_empty() {
            return Ok(None);
        }
        let expression = self.inline_expressions(expressions)?;
        self.create_expression_statement(expression).map(Some)
    }

    fn materialize_class_pending_statements(
        &mut self,
        pending: &mut ClassPendingPlan,
    ) -> Result<Vec<TransformNode>, TransformError> {
        self.materialize_class_pending_expressions(pending)?
            .into_iter()
            .map(|expression| self.create_expression_statement(expression))
            .collect()
    }

    /// At ES2022, only flagged private static elements leave the native class.
    /// Their hoisted method/accessor definitions stay in a synthetic leading
    /// static block, after the class-this and named-evaluation transport
    /// blocks and before all ordinary members.
    ///
    /// tsc-port: transformClassMembers @6.0.3
    /// tsc-hash: 8f02dc71f423a197caae79451edbed69e643ef5b909248bf13a649c2c2491071
    /// tsc-span: _tsc.js:97143-97237
    fn install_private_static_pending_block(
        &mut self,
        members: &mut Vec<TransformNode>,
        pending: &mut ClassPendingPlan,
    ) -> Result<(), TransformError> {
        if !self.selectively_transforms_private_static_elements() || pending.is_empty() {
            return Ok(());
        }
        let expressions = self.materialize_class_pending_expressions(pending)?;
        if expressions.is_empty() {
            return Ok(());
        }
        let expression = self.inline_expressions(expressions)?;
        let statement = self.create_expression_statement(expression)?;
        let body = self.create_block(vec![statement], false)?;
        let static_block = self.context.factory()?.create_node(
            self.source,
            NodeData::ClassStaticBlockDeclaration(
                tsc_syntax::nodes::ClassStaticBlockDeclarationData {
                    body: Some(body.node()),
                    modifiers: None,
                },
            ),
            TransformFlags::NONE,
        )?;

        let class_this = members.iter().position(|member| {
            self.context
                .arena()
                .metadata(*member)
                .and_then(|metadata| metadata.class_this)
                .is_some()
        });
        let named_evaluation = members.iter().position(|member| {
            self.context
                .arena()
                .metadata(*member)
                .and_then(|metadata| metadata.assigned_name)
                .is_some()
        });
        let mut leading = Vec::with_capacity(3);
        if let Some(index) = class_this {
            leading.push(members[index]);
        }
        if let Some(index) = named_evaluation {
            if Some(index) != class_this {
                leading.push(members[index]);
            }
        }
        leading.push(static_block);
        leading.extend(
            members
                .iter()
                .copied()
                .enumerate()
                .filter_map(|(index, member)| {
                    (Some(index) != class_this && Some(index) != named_evaluation).then_some(member)
                }),
        );
        *members = leading;
        Ok(())
    }

    fn visit_property_assignment(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::PropertyAssignmentData,
    ) -> Result<NodeId, TransformError> {
        let initializer = data
            .initializer
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyAssignment,
                field: "initializer",
            })?;
        let needs_assigned_name = !self.is_proto_setter_name(data.name)
            && self.anonymous_class_initializer_needs_assigned_name(initializer)?;

        if needs_assigned_name {
            let original_name = data.name.ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyAssignment,
                field: "name",
            })?;
            let (name, assigned_name) =
                self.plan_property_assignment_name(self.node(original_name))?;
            data.name = Some(name.node());
            self.assigned_class_names
                .insert(original_name, assigned_name);
        } else {
            data.name = self.visit_optional_node(data.name)?;
        }
        data.initializer = Some(
            self.visit_required(
                Some(initializer),
                SyntaxKind::PropertyAssignment,
                "initializer",
            )?
            .node(),
        );

        let node_data = NodeData::PropertyAssignment(data);
        let flags = flags_after_update(self.context.arena(), original, &node_data)?;
        self.context
            .factory()?
            .update_node(original, node_data, flags)
            .map(TransformNode::node)
    }

    fn anonymous_class_initializer_needs_assigned_name(
        &self,
        initializer: NodeId,
    ) -> Result<bool, TransformError> {
        let initializer =
            self.skip_runtime_transparent_outer_expressions(self.node(initializer))?;
        let NodeData::ClassExpression(class) = &self.context.arena().node(initializer)?.data else {
            return Ok(false);
        };
        Ok(class.name.is_none() && self.class_has_transformable_static_member(class.members)?)
    }

    fn plan_property_assignment_name(
        &mut self,
        original: TransformNode,
    ) -> Result<(TransformNode, AssignedClassName), TransformError> {
        let NodeData::ComputedPropertyName(mut computed) =
            self.context.arena().node(original)?.data.clone()
        else {
            let assigned_name = self.literal_assigned_name(original.node()).ok_or(
                TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PropertyAssignment,
                    field: "literal assigned name",
                },
            )?;
            let name = self.visit_required(
                Some(original.node()),
                SyntaxKind::PropertyAssignment,
                "name",
            )?;
            return Ok((name, assigned_name));
        };

        let expression = computed
            .expression
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ComputedPropertyName,
                field: "expression",
            })?;
        if let Some(assigned_name) = self.computed_literal_assigned_name(expression) {
            let name = self.visit_required(
                Some(original.node()),
                SyntaxKind::PropertyAssignment,
                "name",
            )?;
            return Ok((name, assigned_name));
        }

        let expression = self.visit_computed_property_expression(Some(expression), false)?;
        let key = self.allocate_temp_name()?;
        let target = self.create_binding_identifier(&key)?;
        self.context
            .request_emit_helper(super::super::helpers::prop_key())?;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::PropKey)?;
        let property_key = self.create_call(helper, vec![expression])?;
        let assignment = self.create_assignment(target, property_key)?;
        computed.expression = Some(assignment.node());
        let name = self.update_computed_property_name(original, computed)?;
        let read = self.create_binding_identifier(&key)?;
        Ok((self.node(name), AssignedClassName::Evaluated(read)))
    }

    fn computed_literal_assigned_name(&self, expression: NodeId) -> Option<AssignedClassName> {
        match &self.context.arena().node(self.node(expression)).ok()?.data {
            NodeData::StringLiteral(data) => Some(AssignedClassName::Literal(data.text.clone())),
            NodeData::NoSubstitutionTemplateLiteral(data) => {
                Some(AssignedClassName::Literal(data.text.clone()))
            }
            NodeData::NumericLiteral(data) => Some(AssignedClassName::Literal(data.text.clone())),
            _ => None,
        }
    }

    /// Plan the class-definition-time evaluation of an ordinary field key.
    /// A non-inlineable key used by an emitted initializer is captured once
    /// in the containing lexical scope; erased uninitialized fields still
    /// retain side effects from complex computed keys.
    fn plan_public_field_name(
        &mut self,
        name: NodeId,
        should_capture: bool,
    ) -> Result<PlannedPropertyName, TransformError> {
        let original = self.node(name);
        let NodeData::ComputedPropertyName(mut data) =
            self.context.arena().node(original)?.data.clone()
        else {
            let assigned_class_name = self.literal_assigned_name(name);
            let name = self.visit_required(Some(name), SyntaxKind::PropertyDeclaration, "name")?;
            return Ok(PlannedPropertyName {
                name: name.node(),
                evaluation: None,
                assigned_class_name,
            });
        };
        let assigned_literal = data
            .expression
            .and_then(|expression| self.computed_literal_assigned_name(expression));
        let expression = self.visit_computed_property_expression(data.expression, true)?;

        if self
            .context
            .arena()
            .metadata(original)
            .is_some_and(|metadata| {
                metadata
                    .internal_flags()
                    .contains(InternalEmitFlags::GENERATED_COMPUTED_PROPERTY_NAME)
            })
        {
            // Legacy decorators encode their shared cache as
            // `[temporary = expression]`; standard decorators may hand us a
            // plain generated read. The former still belongs to the class's
            // ordered key-evaluation plan, while the field operation must use
            // only the cached read so constructors never repeat the key.
            let assignment_left = match &self.context.arena().node(expression)?.data {
                NodeData::BinaryExpression(binary)
                    if binary
                        .operator_token
                        .and_then(|operator| self.context.arena().node_ref(self.source, operator))
                        .is_some_and(|operator| {
                            self.context
                                .arena()
                                .node(operator)
                                .is_ok_and(|operator| operator.kind == SyntaxKind::EqualsToken)
                        }) =>
                {
                    binary
                        .left
                        .and_then(|left| self.context.arena().node_ref(self.source, left))
                }
                _ => None,
            };
            let (key_expression, evaluation) = if let Some(left) = assignment_left {
                (self.context.factory()?.clone_node(left)?, Some(expression))
            } else {
                (expression, None)
            };
            let assigned_class_name = match assigned_literal {
                Some(assigned_name) => Some(assigned_name),
                None => Some(AssignedClassName::Evaluated(
                    self.context.factory()?.clone_node(key_expression)?,
                )),
            };
            data.expression = Some(key_expression.node());
            let name = self.update_computed_property_name(original, data)?;
            return Ok(PlannedPropertyName {
                name,
                evaluation,
                assigned_class_name,
            });
        }

        let inner = self.skip_partially_emitted_expressions(expression)?;
        let inlineable = self.is_simple_inlineable_expression(inner)?;
        let identifier = self.context.arena().node(inner)?.kind == SyntaxKind::Identifier;
        let (key_expression, evaluation) = if should_capture && !inlineable {
            let temporary_name = self.allocate_temp_name()?;
            let target = self.create_binding_identifier(&temporary_name)?;
            let evaluation = self.create_assignment(target, expression)?;
            let read = self.create_binding_identifier(&temporary_name)?;
            (read, Some(evaluation))
        } else {
            let evaluation = (!inlineable && !identifier)
                .then(|| self.context.factory()?.clone_node(expression))
                .transpose()?;
            (expression, evaluation)
        };
        let assigned_class_name = match assigned_literal {
            Some(assigned_name) => Some(assigned_name),
            None if should_capture => Some(AssignedClassName::Evaluated(
                self.context.factory()?.clone_node(key_expression)?,
            )),
            None => None,
        };
        data.expression = Some(key_expression.node());
        let name = self.update_computed_property_name(original, data)?;
        Ok(PlannedPropertyName {
            name,
            evaluation,
            assigned_class_name,
        })
    }

    fn visit_computed_property_name(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ComputedPropertyNameData,
    ) -> Result<NodeId, TransformError> {
        let is_class_element_name = self
            .tree_ownership
            .unique_parent(original.node())
            .and_then(|parent| self.context.arena().node_ref(self.source, parent))
            .and_then(|parent| self.context.arena().node(parent).ok())
            .is_some_and(|parent| {
                matches!(
                    &parent.data,
                    NodeData::PropertyDeclaration(member)
                        if member.name == Some(original.node())
                ) || matches!(
                    &parent.data,
                    NodeData::MethodDeclaration(member)
                        if member.name == Some(original.node())
                ) || matches!(
                    &parent.data,
                    NodeData::GetAccessor(member)
                        if member.name == Some(original.node())
                ) || matches!(
                    &parent.data,
                    NodeData::SetAccessor(member)
                        if member.name == Some(original.node())
                )
            });
        data.expression = Some(
            self.visit_computed_property_expression(data.expression, is_class_element_name)?
                .node(),
        );
        self.update_computed_property_name(original, data)
    }

    fn visit_computed_property_expression(
        &mut self,
        expression: Option<NodeId>,
        crosses_class_boundary: bool,
    ) -> Result<TransformNode, TransformError> {
        let enclosing = crosses_class_boundary
            .then(|| self.static_binding_frames.enclosing_class_evaluation())
            .flatten();
        let _enclosing_scope = enclosing.map(|bindings| {
            self.static_binding_frames
                .enter(StaticBindingFrame::StaticEvaluation(Some(bindings)))
        });
        self.visit_required(expression, SyntaxKind::ComputedPropertyName, "expression")
    }

    fn update_computed_property_name(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::ComputedPropertyNameData,
    ) -> Result<NodeId, TransformError> {
        let node_data = NodeData::ComputedPropertyName(data);
        let flags = flags_after_update(self.context.arena(), original, &node_data)?;
        self.context
            .factory()?
            .update_node(original, node_data, flags)
            .map(TransformNode::node)
    }

    fn skip_partially_emitted_expressions(
        &self,
        mut expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        loop {
            let NodeData::PartiallyEmittedExpression(data) =
                &self.context.arena().node(expression)?.data
            else {
                return Ok(expression);
            };
            expression = data
                .expression
                .map(|expression| self.node(expression))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PartiallyEmittedExpression,
                    field: "expression",
                })?;
        }
    }

    fn is_simple_inlineable_expression(
        &self,
        expression: TransformNode,
    ) -> Result<bool, TransformError> {
        let kind = self.context.arena().node(expression)?.kind;
        Ok(matches!(
            kind,
            SyntaxKind::StringLiteral
                | SyntaxKind::NoSubstitutionTemplateLiteral
                | SyntaxKind::NumericLiteral
        ) || kind.value() >= SyntaxKind::FirstKeyword.value()
            && kind.value() <= SyntaxKind::LastKeyword.value())
    }

    fn is_simple_copiable_expression(
        &self,
        expression: TransformNode,
    ) -> Result<bool, TransformError> {
        Ok(
            self.context.arena().node(expression)?.kind == SyntaxKind::Identifier
                || self.is_simple_inlineable_expression(expression)?,
        )
    }

    fn visit_field_value_plan(
        &mut self,
        value: FieldValuePlan,
    ) -> Result<FieldValuePlan, TransformError> {
        Ok(match value {
            FieldValuePlan::Declared { initializer } => FieldValuePlan::Declared {
                initializer: self.visit_optional_node(initializer)?,
            },
            FieldValuePlan::ParameterProperty { prefix, local } => {
                FieldValuePlan::ParameterProperty {
                    prefix: self.visit_optional_node(prefix)?,
                    local,
                }
            }
        })
    }

    /// tsc-port: transformProperty @6.0.3
    /// tsc-hash: c4e9fbf0eb6953a64ba8257f83a5a79f3f8d904f06c12336d30b94ad5cdfd847
    /// tsc-span: _tsc.js:97488-97500
    /// tsc-port: transformPropertyWorker @6.0.3
    /// tsc-hash: fb5e7b8fdfc4fab54f8fdd4ea6f48902c80207af52647e23cb47491f0ce46edd
    /// tsc-span: _tsc.js:97501-97575
    fn materialize_field_operation(
        &mut self,
        operation: &FieldOperation,
        class_name: Option<&ClassBinding>,
    ) -> Result<TransformNode, TransformError> {
        let receiver = match operation.receiver {
            FieldReceiver::Instance => self.context.factory()?.create_token(
                self.source,
                SyntaxKind::ThisKeyword,
                TransformFlags::CONTAINS_LEXICAL_THIS,
            )?,
            FieldReceiver::Static => self.create_binding_identifier(class_name.ok_or(
                TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ClassDeclaration,
                    field: "static class binding",
                },
            )?)?,
        };
        let initializer = self.materialize_field_value(&operation.value)?;
        if operation.value.has_runtime_value() {
            self.context
                .arena_mut()?
                .metadata_mut(initializer)
                .relocated_trailing_comment_owner =
                Some(RelocatedTrailingCommentOwner::ClassFieldOperation);
        }
        let expression = match self.mode {
            PublicFieldMode::Assignment => {
                let target = self.create_member_access(receiver, operation.name)?;
                // transformPropertyOrClassStaticBlock moves the declaration's
                // leading trivia to the synthesized statement. The parsed
                // property name is retained below this access for spelling
                // and source maps, but must not emit that trivia again after
                // `this.`/the static receiver.
                self.context
                    .arena_mut()?
                    .metadata_mut(target)
                    .add_flags(EmitFlags::NO_LEADING_COMMENTS);
                self.create_assignment(target, initializer)?
            }
            PublicFieldMode::DefineProperty => {
                self.create_define_property(receiver, operation.name, initializer)?
            }
        };
        if operation.range_static_expression_to_name {
            let name = self.node(operation.name);
            let source_map_range = self
                .context
                .arena()
                .metadata(name)
                .and_then(crate::EmitMetadata::source_map_range)
                .or_else(|| {
                    let arena = self.context.arena();
                    let record = arena.node(name).ok()?;
                    let source = arena.source(name.source()).ok()?.syntax();
                    SourceRange::from_raw(record.pos, record.end, source.positions())
                        .ok()
                        .map(|range| SourceMapRange::new(name.source(), range))
                });
            let metadata = self.context.arena_mut()?.metadata_mut(expression);
            metadata.add_flags(EmitFlags::ADVISE_ON_EMIT_NODE);
            if let Some(source_map_range) = source_map_range {
                metadata.set_source_map_range(source_map_range);
            }
        }
        let statement = self.create_expression_statement(expression)?;
        self.set_original_and_range(statement, operation.original)?;
        let property_original = self.context.arena().get_original_node(operation.original);
        if self.context.arena().node(property_original)?.kind == SyntaxKind::Parameter {
            let source_map_range = {
                let arena = self.context.arena();
                let record = arena.node(property_original)?;
                let source = arena.source(property_original.source())?.syntax();
                SourceRange::from_raw(record.pos, record.end, source.positions())
                    .map(|range| SourceMapRange::new(property_original.source(), range))
                    .map_err(|error| TransformError::InvalidSourceRange {
                        node: property_original,
                        error,
                    })?
            };
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .set_source_map_range(source_map_range);
        }
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .set_starts_on_new_line(true);
        Ok(statement)
    }

    fn materialize_field_value(
        &mut self,
        value: &FieldValuePlan,
    ) -> Result<TransformNode, TransformError> {
        match value {
            FieldValuePlan::Declared {
                initializer: Some(initializer),
            } => Ok(self.node(*initializer)),
            FieldValuePlan::Declared { initializer: None } => self.create_void_zero(),
            FieldValuePlan::ParameterProperty { prefix, local } => {
                let source_local = *local;
                let local = self.context.factory()?.clone_node(source_local)?;
                self.context
                    .factory()?
                    .set_text_range(local, source_local)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(local)
                    .add_flags(EmitFlags::NO_COMMENTS);
                let Some(prefix) = prefix else {
                    return Ok(local);
                };
                let prefix = self.normalize_parameter_property_prefix(self.node(*prefix))?;
                self.inline_expressions(vec![prefix, local])
            }
        }
    }

    fn normalize_parameter_property_prefix(
        &self,
        prefix: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::ParenthesizedExpression(parenthesized) =
            &self.context.arena().node(prefix)?.data
        else {
            return Ok(prefix);
        };
        let Some(expression) = parenthesized
            .expression
            .and_then(|expression| self.context.arena().node_ref(self.source, expression))
        else {
            return Ok(prefix);
        };
        let NodeData::BinaryExpression(binary) = &self.context.arena().node(expression)?.data
        else {
            return Ok(prefix);
        };
        let is_comma = binary.operator_token.is_some_and(|operator| {
            self.context
                .arena()
                .node(self.node(operator))
                .is_ok_and(|operator| operator.kind == SyntaxKind::CommaToken)
        });
        if !is_comma {
            return Ok(prefix);
        }
        let Some(left) = binary
            .left
            .and_then(|left| self.context.arena().node_ref(self.source, left))
        else {
            return Ok(prefix);
        };
        let Some(right) = binary
            .right
            .and_then(|right| self.context.arena().node_ref(self.source, right))
        else {
            return Ok(prefix);
        };
        if !self.is_run_initializers_call(left)? || !self.is_void_numeric_literal(right)? {
            return Ok(prefix);
        }
        Ok(left)
    }

    /// tsc-port: isCallToHelper @6.0.3
    /// tsc-hash: 65c471809533a93e4ad2d44931471cb8a169cf9c93c9b291bc7a7dbdeede8fef
    /// tsc-span: _tsc.js:26566-26568
    fn is_run_initializers_call(&self, expression: TransformNode) -> Result<bool, TransformError> {
        self.context
            .arena()
            .is_call_to_emit_helper(expression, EmitHelperName::RunInitializers)
    }

    fn is_void_numeric_literal(&self, expression: TransformNode) -> Result<bool, TransformError> {
        let NodeData::VoidExpression(void_expression) =
            &self.context.arena().node(expression)?.data
        else {
            return Ok(false);
        };
        Ok(void_expression.expression.is_some_and(|operand| {
            self.context
                .arena()
                .node(self.node(operand))
                .is_ok_and(|operand| operand.kind == SyntaxKind::NumericLiteral)
        }))
    }

    fn materialize_private_brand(
        &mut self,
        brand_name: &ClassBinding,
    ) -> Result<TransformNode, TransformError> {
        let brand = self.create_binding_identifier(brand_name)?;
        let add = self.create_property_access(brand, "add")?;
        let receiver = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::ThisKeyword,
            TransformFlags::CONTAINS_LEXICAL_THIS,
        )?;
        let call = self.create_call(add, vec![receiver])?;
        self.create_expression_statement(call)
    }

    fn materialize_private_instance_field(
        &mut self,
        operation: &PrivateFieldOperation,
    ) -> Result<TransformNode, TransformError> {
        if operation.slot.is_static() {
            let statement = self.materialize_private_static_field(operation)?;
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .set_starts_on_new_line(true);
            return Ok(statement);
        }
        let receiver = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::ThisKeyword,
            TransformFlags::CONTAINS_LEXICAL_THIS,
        )?;
        self.materialize_private_weak_map_field(operation, receiver)
    }

    fn materialize_private_weak_map_field(
        &mut self,
        operation: &PrivateFieldOperation,
        receiver: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let storage_name =
            operation
                .slot
                .field_value_name()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PropertyDeclaration,
                    field: "private field storage",
                })?;
        let storage = self.create_binding_identifier(storage_name)?;
        let set = self.create_property_access(storage, "set")?;
        let initializer = operation
            .initializer
            .map(|initializer| self.node(initializer))
            .unwrap_or(self.create_void_zero()?);
        let call = self.create_call(set, vec![receiver, initializer])?;
        let statement = self.create_expression_statement(call)?;
        self.set_original_and_range(statement, operation.original)?;
        if let Some(source_map_range) =
            self.private_property_source_map_range(operation.original)?
        {
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .set_source_map_range(source_map_range);
        }
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .set_starts_on_new_line(true);
        Ok(statement)
    }

    fn materialize_private_storage(
        &mut self,
        slot: &PrivateSlot,
    ) -> Result<TransformNode, TransformError> {
        let storage_name = slot
            .field_value_name()
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyDeclaration,
                field: "private field storage",
            })?;
        let storage = self.create_binding_identifier(storage_name)?;
        let weak_map = self.create_identifier("WeakMap")?;
        let weak_map = self.create_new(weak_map, Vec::new())?;
        self.create_assignment(storage, weak_map)
    }

    fn materialize_private_static_field(
        &mut self,
        operation: &PrivateFieldOperation,
    ) -> Result<TransformNode, TransformError> {
        let storage_name =
            operation
                .slot
                .field_value_name()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PropertyDeclaration,
                    field: "private static field storage",
                })?;
        let storage = self.create_binding_identifier(storage_name)?;
        let initializer = operation
            .initializer
            .map(|initializer| self.node(initializer))
            .unwrap_or(self.create_void_zero()?);
        let value = self.create_property_assignment("value", initializer)?;
        let descriptor = self.create_object_literal(vec![value], false)?;
        let mut assignment = self.create_assignment(storage, descriptor)?;
        if let Some(comment_source) = self
            .context
            .arena()
            .metadata(operation.original)
            .and_then(|metadata| metadata.class_field_initializer_comment_source)
        {
            assignment = self.set_original_and_range(assignment, comment_source)?;
            self.context
                .arena_mut()?
                .metadata_mut(assignment)
                .class_field_initializer_comment_source = Some(comment_source);
        }
        if let Some(source_map_range) =
            self.private_property_name_source_map_range(operation.original)?
        {
            let metadata = self.context.arena_mut()?.metadata_mut(assignment);
            metadata.add_flags(EmitFlags::ADVISE_ON_EMIT_NODE);
            metadata.set_source_map_range(source_map_range);
        }
        let statement = self.create_expression_statement(assignment)?;
        self.set_original_and_range(statement, operation.original)?;
        if let Some(source_map_range) =
            self.private_property_source_map_range(operation.original)?
        {
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .set_source_map_range(source_map_range);
        }
        Ok(statement)
    }

    fn materialize_private_static_field_block(
        &mut self,
        operation: &PrivateFieldOperation,
    ) -> Result<TransformNode, TransformError> {
        let statement = self.materialize_private_static_field(operation)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .set_comment_range(crate::CommentRange::new(
                self.source,
                SourceRange::Synthesized,
            ));
        let body = self.create_block(vec![statement], true)?;
        let block = self.context.factory()?.create_node(
            self.source,
            NodeData::ClassStaticBlockDeclaration(
                tsc_syntax::nodes::ClassStaticBlockDeclarationData {
                    body: Some(body.node()),
                    modifiers: None,
                },
            ),
            TransformFlags::NONE,
        )?;
        self.context
            .arena_mut()?
            .set_semantic_original_node(block, operation.original)?;
        let record = self.context.arena().node(operation.original)?;
        let positions = self
            .context
            .arena()
            .source(operation.original.source())?
            .syntax()
            .positions();
        let range = SourceRange::from_raw(record.pos, record.end, positions).map_err(|error| {
            TransformError::InvalidSourceRange {
                node: operation.original,
                error,
            }
        })?;
        self.context
            .arena_mut()?
            .metadata_mut(block)
            .set_comment_range(crate::CommentRange::new(operation.original.source(), range));
        Ok(block)
    }

    /// The class-fields pass applies `moveRangePastModifiers` to each
    /// property expression before it becomes a constructor/static statement.
    /// Property declarations start that range at their name, including the
    /// generated backing field of an auto-accessor.
    ///
    /// tsc-port: generateInitializedPropertyExpressionsOrClassStaticBlock @6.0.3
    /// tsc-hash: 8e776d62fb988da8525039a9b7246226f4a003e34a285cec156b79e7f02a09a3
    /// tsc-span: _tsc.js:97460-97487
    fn private_property_source_map_range(
        &self,
        property: TransformNode,
    ) -> Result<Option<SourceMapRange>, TransformError> {
        let original = self.context.arena().get_original_node(property);
        let record = self.context.arena().node(original)?;
        let NodeData::PropertyDeclaration(data) = &record.data else {
            return Ok(None);
        };
        let Some(name) = data
            .name
            .and_then(|name| self.context.arena().node_ref(original.source(), name))
        else {
            return Ok(None);
        };
        let start = self.context.arena().node(name)?.pos;
        let source = self.context.arena().source(original.source())?.syntax();
        let range =
            SourceRange::from_raw(start, record.end, source.positions()).map_err(|error| {
                TransformError::InvalidSourceRange {
                    node: original,
                    error,
                }
            })?;
        Ok(Some(SourceMapRange::new(original.source(), range)))
    }

    fn private_property_name_source_map_range(
        &self,
        property: TransformNode,
    ) -> Result<Option<SourceMapRange>, TransformError> {
        let original = self.context.arena().get_original_node(property);
        let NodeData::PropertyDeclaration(data) = &self.context.arena().node(original)?.data else {
            return Ok(None);
        };
        let Some(name) = data
            .name
            .and_then(|name| self.context.arena().node_ref(original.source(), name))
        else {
            return Ok(None);
        };
        let record = self.context.arena().node(name)?;
        let source = self.context.arena().source(original.source())?.syntax();
        let range = SourceRange::from_raw(record.pos, record.end, source.positions())
            .map_err(|error| TransformError::InvalidSourceRange { node: name, error })?;
        Ok(Some(SourceMapRange::new(original.source(), range)))
    }

    fn create_private_get(
        &mut self,
        receiver: TransformNode,
        slot: &PrivateSlot,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(EmitHelper::with_text(
            "typescript:classPrivateFieldGet",
            false,
            CLASS_PRIVATE_FIELD_GET_HELPER_TEXT,
            None,
            Vec::new(),
        ))?;
        // tsc moves the receiver's comment range start to the synthetic
        // sentinel before placing it in the helper argument list. Rust's
        // range type intentionally rejects mixed synthetic/original ranges,
        // so encode the same ownership directly: the containing access owns
        // leading trivia, while the receiver retains its source range.
        self.context
            .arena_mut()?
            .metadata_mut(receiver)
            .add_flags(EmitFlags::NO_LEADING_COMMENTS);
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::ClassPrivateFieldGet)?;
        let brand = self.create_binding_identifier(slot.brand_name())?;
        let kind = self.create_string_literal(slot.access_kind())?;
        let mut arguments = vec![receiver, brand, kind];
        if let Some(descriptor) = slot.getter_descriptor_name() {
            arguments.push(self.create_binding_identifier(descriptor)?);
        }
        self.create_call(helper, arguments)
    }

    fn create_private_set(
        &mut self,
        receiver: TransformNode,
        slot: &PrivateSlot,
        value: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(EmitHelper::with_text(
            "typescript:classPrivateFieldSet",
            false,
            CLASS_PRIVATE_FIELD_SET_HELPER_TEXT,
            None,
            Vec::new(),
        ))?;
        self.context
            .arena_mut()?
            .metadata_mut(receiver)
            .add_flags(EmitFlags::NO_LEADING_COMMENTS);
        // The source assignment/update owns trivia at the end of the right
        // operand. Without this boundary, a retained trailing comment is
        // emitted inside the synthesized helper's argument list and then a
        // second time from the original-linked outer expression.
        self.context
            .arena_mut()?
            .metadata_mut(value)
            .add_flags(EmitFlags::NO_TRAILING_COMMENTS);
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::ClassPrivateFieldSet)?;
        let brand = self.create_binding_identifier(slot.brand_name())?;
        let kind = self.create_string_literal(slot.access_kind())?;
        let mut arguments = vec![receiver, brand, value, kind];
        if let Some(descriptor) = slot.setter_descriptor_name() {
            arguments.push(self.create_binding_identifier(descriptor)?);
        }
        self.create_call(helper, arguments)
    }

    fn create_private_in(
        &mut self,
        slot: &PrivateSlot,
        receiver: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(EmitHelper::with_text(
            "typescript:classPrivateFieldIn",
            false,
            CLASS_PRIVATE_FIELD_IN_HELPER_TEXT,
            None,
            Vec::new(),
        ))?;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::ClassPrivateFieldIn)?;
        let brand = self.create_binding_identifier(slot.brand_name())?;
        self.create_call(helper, vec![brand, receiver])
    }

    fn stabilize_receiver(
        &mut self,
        receiver: TransformNode,
    ) -> Result<StabilizedReceiver, TransformError> {
        if matches!(
            self.context.arena().node(receiver)?.kind,
            SyntaxKind::Identifier | SyntaxKind::ThisKeyword | SyntaxKind::SuperKeyword
        ) {
            return Ok(StabilizedReceiver {
                read: self.context.factory()?.clone_node(receiver)?,
                initialized: None,
            });
        }
        let temporary = self.allocate_shadowable_temp_name()?;
        let read = self.create_binding_identifier(&temporary)?;
        let target = self.create_binding_identifier(&temporary)?;
        let initialized = self.create_assignment(target, receiver)?;
        Ok(StabilizedReceiver {
            read,
            initialized: Some(initialized),
        })
    }

    fn stabilize_inline_receiver(
        &mut self,
        receiver: TransformNode,
    ) -> Result<StabilizedReceiver, TransformError> {
        if self.is_simple_inlineable_expression(receiver)? {
            return Ok(StabilizedReceiver {
                read: self.context.factory()?.clone_node(receiver)?,
                initialized: None,
            });
        }
        let temporary = self.allocate_shadowable_temp_name()?;
        let read = self.create_binding_identifier(&temporary)?;
        let target = self.create_binding_identifier(&temporary)?;
        let initialized = self.create_assignment(target, receiver)?;
        Ok(StabilizedReceiver {
            read,
            initialized: Some(initialized),
        })
    }

    fn create_define_property(
        &mut self,
        receiver: TransformNode,
        name: NodeId,
        value: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let object = self.create_identifier("Object")?;
        let define_property = self.create_property_access(object, "defineProperty")?;
        let key = self.property_key_expression(name)?;
        let true_value = self.create_boolean(true)?;
        let enumerable = self.create_property_assignment("enumerable", true_value)?;
        let true_value = self.create_boolean(true)?;
        let configurable = self.create_property_assignment("configurable", true_value)?;
        let true_value = self.create_boolean(true)?;
        let writable = self.create_property_assignment("writable", true_value)?;
        let value = self.create_property_assignment("value", value)?;
        let descriptor =
            self.create_object_literal(vec![enumerable, configurable, writable, value], true)?;
        self.create_call(define_property, vec![receiver, key, descriptor])
    }

    fn property_key_expression(&mut self, name: NodeId) -> Result<TransformNode, TransformError> {
        let name = self.node(name);
        match self.context.arena().node(name)?.data.clone() {
            NodeData::Identifier(data) => self.create_string_literal(&data.text),
            NodeData::PrivateIdentifier(data) => {
                self.create_string_literal(data.text.trim_start_matches('#'))
            }
            NodeData::StringLiteral(_) | NodeData::NumericLiteral(_) => {
                self.context.factory()?.clone_node(name)
            }
            NodeData::ComputedPropertyName(data) => data
                .expression
                .map(|expression| self.node(expression))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ComputedPropertyName,
                    field: "expression",
                }),
            _ => self.context.factory()?.clone_node(name),
        }
    }

    fn create_member_access(
        &mut self,
        receiver: TransformNode,
        name: NodeId,
    ) -> Result<TransformNode, TransformError> {
        let name_node = self.node(name);
        let (access, no_nested_source_maps) =
            match self.context.arena().node(name_node)?.data.clone() {
                NodeData::Identifier(_) | NodeData::PrivateIdentifier(_) => (
                    self.context.factory()?.create_node(
                        self.source,
                        NodeData::PropertyAccessExpression(
                            tsc_syntax::nodes::PropertyAccessExpressionData {
                                expression: Some(receiver.node()),
                                question_dot_token: None,
                                name: Some(name),
                            },
                        ),
                        TransformFlags::NONE,
                    )?,
                    true,
                ),
                NodeData::ComputedPropertyName(data) => (
                    self.context.factory()?.create_node(
                        self.source,
                        NodeData::ElementAccessExpression(
                            tsc_syntax::nodes::ElementAccessExpressionData {
                                expression: Some(receiver.node()),
                                question_dot_token: None,
                                argument_expression: data.expression,
                            },
                        ),
                        TransformFlags::NONE,
                    )?,
                    false,
                ),
                _ => (
                    self.context.factory()?.create_node(
                        self.source,
                        NodeData::ElementAccessExpression(
                            tsc_syntax::nodes::ElementAccessExpressionData {
                                expression: Some(receiver.node()),
                                question_dot_token: None,
                                argument_expression: Some(name),
                            },
                        ),
                        TransformFlags::NONE,
                    )?,
                    true,
                ),
            };
        // tsc's createMemberAccessForPropertyName gives the generated access
        // the member-name range. NoLeadingComments then establishes that
        // range start as the nested comment-container boundary, while the
        // access itself remains the sole outer source-map owner.
        self.context.factory()?.set_text_range(access, name_node)?;
        if no_nested_source_maps {
            self.context
                .arena_mut()?
                .metadata_mut(access)
                .add_flags(EmitFlags::NO_NESTED_SOURCE_MAPS);
        }
        Ok(access)
    }

    /// tsc-port: transformConstructorBody @6.0.3
    /// tsc-hash: a5aaf4143fef6da9e320076663f60354ba772956453da86bb764d6f1bf381333
    /// tsc-span: _tsc.js:97366-97431
    fn inject_into_constructor(
        &mut self,
        constructor: TransformNode,
        initializers: &[TransformNode],
    ) -> Result<TransformNode, TransformError> {
        let NodeData::Constructor(mut data) = self.context.arena().node(constructor)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ClassDeclaration,
                field: "constructor",
            });
        };
        let body = data
            .body
            .and_then(|body| self.context.arena().node_ref(self.source, body))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Constructor,
                field: "body",
            })?;
        let body_record = self.context.arena().node(body)?.clone();
        let original_multi_line = body_record.multi_line;
        let NodeData::Block(mut block) = body_record.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Constructor,
                field: "body block",
            });
        };
        let original_statements = block
            .statements
            .and_then(|array| self.context.arena().node_array_ref(self.source, array));
        let mut statements = self.array_nodes(block.statements)?;
        let original_statement_count = statements.len();
        let insertion =
            super::super::constructor_prologue(self.context.arena(), &statements)?.body_start();
        let replaces_parameter_properties = initializers
            .iter()
            .any(|initializer| self.original_kind(*initializer) == Some(SyntaxKind::Parameter));
        if let Some(path) = self.find_super_statement_path(&statements, insertion)? {
            self.inject_initializers_at_super_path(
                &mut statements,
                &path.0,
                initializers,
                replaces_parameter_properties,
            )?;
        } else {
            Self::insert_constructor_initializers(
                self.context.arena(),
                &mut statements,
                insertion,
                initializers,
                replaces_parameter_properties,
            );
        }
        let transformed_statement_count = statements.len();
        let array = if let Some(original_statements) = original_statements {
            self.context
                .factory()?
                .update_node_array(original_statements, statements)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, statements)?
        };
        block.statements = Some(array.array());
        let flags =
            flags_after_update(self.context.arena(), body, &NodeData::Block(block.clone()))?;
        let body = self
            .context
            .factory()?
            .update_node(body, NodeData::Block(block), flags)?;
        let multi_line = if original_statement_count >= transformed_statement_count {
            original_multi_line.unwrap_or(transformed_statement_count != 0)
        } else {
            transformed_statement_count != 0
        };
        self.context.factory()?.set_multi_line(body, multi_line)?;
        data.body = Some(body.node());
        let flags = flags_after_update(
            self.context.arena(),
            constructor,
            &NodeData::Constructor(data.clone()),
        )?;
        self.context
            .factory()?
            .update_node(constructor, NodeData::Constructor(data), flags)
    }

    fn find_super_statement_path(
        &self,
        statements: &[TransformNode],
        start: usize,
    ) -> Result<Option<SuperStatementPath>, TransformError> {
        for (index, statement) in statements.iter().enumerate().skip(start) {
            if self.statement_is_super_call(*statement)? {
                return Ok(Some(SuperStatementPath(vec![index])));
            }
            let NodeData::TryStatement(data) = &self.context.arena().node(*statement)?.data else {
                continue;
            };
            let Some(try_block) = data
                .try_block
                .and_then(|block| self.context.arena().node_ref(self.source, block))
            else {
                continue;
            };
            let NodeData::Block(block) = &self.context.arena().node(try_block)?.data else {
                continue;
            };
            let nested = self.array_nodes(block.statements)?;
            if let Some(SuperStatementPath(mut path)) =
                self.find_super_statement_path(&nested, 0)?
            {
                path.insert(0, index);
                return Ok(Some(SuperStatementPath(path)));
            }
        }
        Ok(None)
    }

    /// tsc-port: transformConstructorBodyWorker @6.0.3
    /// tsc-hash: 37e090fcc937a5c99a0fce3410f7d5a67fd9612316d31ef64b3dba2d7212ad4a
    /// tsc-span: _tsc.js:97290-97328
    fn inject_initializers_at_super_path(
        &mut self,
        statements: &mut Vec<TransformNode>,
        path: &[usize],
        initializers: &[TransformNode],
        replaces_parameter_properties: bool,
    ) -> Result<(), TransformError> {
        let (&index, remaining) =
            path.split_first()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Constructor,
                    field: "super statement path",
                })?;
        if remaining.is_empty() {
            Self::insert_constructor_initializers(
                self.context.arena(),
                statements,
                index + 1,
                initializers,
                replaces_parameter_properties,
            );
            return Ok(());
        }

        let statement = *statements
            .get(index)
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Constructor,
                field: "super statement path index",
            })?;
        let NodeData::TryStatement(mut try_statement) =
            self.context.arena().node(statement)?.data.clone()
        else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Constructor,
                field: "try statement on super path",
            });
        };
        let try_block = try_statement
            .try_block
            .and_then(|block| self.context.arena().node_ref(self.source, block))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TryStatement,
                field: "try_block on super path",
            })?;
        let NodeData::Block(mut block) = self.context.arena().node(try_block)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TryStatement,
                field: "try block on super path",
            });
        };
        let mut nested = self.array_nodes(block.statements)?;
        self.inject_initializers_at_super_path(
            &mut nested,
            remaining,
            initializers,
            replaces_parameter_properties,
        )?;
        let nested = if let Some(original) = block.statements.map(|array| self.array(array)) {
            self.context
                .factory()?
                .update_node_array(original, nested)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, nested)?
        };
        block.statements = Some(nested.array());
        let flags = flags_after_update(
            self.context.arena(),
            try_block,
            &NodeData::Block(block.clone()),
        )?;
        let try_block =
            self.context
                .factory()?
                .update_node(try_block, NodeData::Block(block), flags)?;
        try_statement.try_block = Some(try_block.node());
        let flags = flags_after_update(
            self.context.arena(),
            statement,
            &NodeData::TryStatement(try_statement.clone()),
        )?;
        statements[index] = self.context.factory()?.update_node(
            statement,
            NodeData::TryStatement(try_statement),
            flags,
        )?;
        Ok(())
    }

    fn insert_constructor_initializers(
        arena: &TransformArena,
        statements: &mut Vec<TransformNode>,
        insertion: usize,
        initializers: &[TransformNode],
        replaces_parameter_properties: bool,
    ) {
        let parameter_end = statements[insertion..]
            .iter()
            .take_while(|statement| {
                let original = arena.get_original_node(**statement);
                arena
                    .node(original)
                    .is_ok_and(|node| node.kind == SyntaxKind::Parameter)
            })
            .count()
            + insertion;
        if replaces_parameter_properties {
            statements.drain(insertion..parameter_end);
            statements.splice(insertion..insertion, initializers.iter().copied());
        } else {
            statements.splice(parameter_end..parameter_end, initializers.iter().copied());
        }
    }

    fn create_synthetic_constructor(
        &mut self,
        derived: bool,
        mut initializers: Vec<TransformNode>,
        class: TransformNode,
        member_range: Option<NodeArrayId>,
    ) -> Result<TransformNode, TransformError> {
        if derived {
            let arguments = self.create_identifier("arguments")?;
            let spread = self.context.factory()?.create_node(
                self.source,
                NodeData::SpreadElement(tsc_syntax::nodes::SpreadElementData {
                    expression: Some(arguments.node()),
                }),
                TransformFlags::CONTAINS_REST_OR_SPREAD,
            )?;
            let super_token = self.context.factory()?.create_token(
                self.source,
                SyntaxKind::SuperKeyword,
                TransformFlags::CONTAINS_LEXICAL_SUPER,
            )?;
            let call = self.create_call(super_token, vec![spread])?;
            initializers.insert(0, self.create_expression_statement(call)?);
        }
        let statements = match member_range {
            Some(member_range) => {
                let member_range = self.array(member_range);
                self.context
                    .factory()?
                    .update_node_array(member_range, initializers)?
            }
            None => self
                .context
                .factory()?
                .create_node_array(self.source, initializers)?,
        };
        let body = self.context.factory()?.create_node(
            self.source,
            NodeData::Block(tsc_syntax::nodes::BlockData {
                statements: Some(statements.array()),
            }),
            TransformFlags::NONE,
        )?;
        let body = self.context.factory()?.set_multi_line(body, true)?;
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, Vec::new())?;
        let constructor = self.context.factory()?.create_node(
            self.source,
            NodeData::Constructor(tsc_syntax::nodes::ConstructorData {
                name: None,
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                body: Some(body.node()),
                modifiers: None,
            }),
            TransformFlags::NONE,
        )?;
        let constructor = self.context.factory()?.set_text_range(constructor, class)?;
        let metadata = self.context.arena_mut()?.metadata_mut(constructor);
        metadata.set_starts_on_new_line(true);
        // The class range positions the generated member and its source map,
        // but the generated constructor owns no comments at the class
        // boundary. Its initializer statements retain their own property
        // comment ranges, including comments at the end of the member list.
        metadata.set_comment_range(crate::CommentRange::new(
            self.source,
            SourceRange::Synthesized,
        ));
        Ok(constructor)
    }

    fn prepend_generated_declarations_to_source(
        &mut self,
        root: TransformNode,
        bindings: ClassGeneratedBindings,
    ) -> Result<TransformNode, TransformError> {
        if bindings.is_empty() {
            return Ok(root);
        }
        let NodeData::SourceFile(mut data) = self.context.arena().node(root)?.data.clone() else {
            return Err(TransformError::RootKindExpected {
                actual: self.context.arena().node(root)?.kind,
            });
        };
        let statement = self.create_generated_variable_statement(&bindings)?;
        let original_statements = data
            .statements
            .and_then(|array| self.context.arena().node_array_ref(self.source, array));
        let mut statements = self.array_nodes(data.statements)?;
        let insertion = statements
            .iter()
            .take_while(|statement| self.is_prologue_statement(**statement).unwrap_or(false))
            .count();
        statements.insert(insertion, statement);
        let array = if let Some(original) = original_statements {
            self.context
                .factory()?
                .update_node_array(original, statements)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, statements)?
        };
        data.statements = Some(array.array());
        let flags = flags_after_update(
            self.context.arena(),
            root,
            &NodeData::SourceFile(data.clone()),
        )?;
        self.context
            .factory()?
            .update_node(root, NodeData::SourceFile(data), flags)
    }

    fn prepend_generated_declarations_to_block(
        &mut self,
        block: TransformNode,
        bindings: ClassGeneratedBindings,
    ) -> Result<TransformNode, TransformError> {
        self.prepend_function_prelude_to_block(block, bindings, Vec::new())
    }

    fn prepend_function_prelude_to_block(
        &mut self,
        block: TransformNode,
        bindings: ClassGeneratedBindings,
        initialization_statements: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        if bindings.is_empty() && initialization_statements.is_empty() {
            return Ok(block);
        }
        let NodeData::Block(mut data) = self.context.arena().node(block)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: self.context.arena().node(block)?.kind,
                field: "block for generated bindings",
            });
        };
        let original_statements = data
            .statements
            .and_then(|array| self.context.arena().node_array_ref(self.source, array));
        let mut statements = self.array_nodes(data.statements)?;
        let mut insertion = statements
            .iter()
            .take_while(|statement| self.is_prologue_statement(**statement).unwrap_or(false))
            .count();
        if !bindings.is_empty() {
            let statement = self.create_generated_variable_statement(&bindings)?;
            statements.insert(insertion, statement);
            insertion += 1;
        }
        statements.splice(insertion..insertion, initialization_statements);
        let array = if let Some(original) = original_statements {
            self.context
                .factory()?
                .update_node_array(original, statements)?
        } else {
            self.context
                .factory()?
                .create_node_array(self.source, statements)?
        };
        data.statements = Some(array.array());
        let flags =
            flags_after_update(self.context.arena(), block, &NodeData::Block(data.clone()))?;
        self.context
            .factory()?
            .update_node(block, NodeData::Block(data), flags)
    }

    fn create_generated_variable_statement(
        &mut self,
        bindings: &ClassGeneratedBindings,
    ) -> Result<TransformNode, TransformError> {
        let bindings = bindings.hoisted_bindings().cloned().collect::<Vec<_>>();
        self.create_binding_variable_statement(&bindings, NodeFlags::NONE)
    }

    fn create_binding_variable_statement(
        &mut self,
        bindings: &[TargetBinding],
        flags: NodeFlags,
    ) -> Result<TransformNode, TransformError> {
        let mut declarations = Vec::with_capacity(bindings.len());
        for binding in bindings {
            let name = self.create_binding_identifier(&ClassBinding::Generated(binding.clone()))?;
            declarations.push(self.create_variable_declaration(name, None)?);
        }
        let statement = self.create_variable_statement(declarations, flags)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::CUSTOM_PROLOGUE);
        Ok(statement)
    }

    fn prepend_loop_binding_declarations(
        &mut self,
        statement: TransformNode,
        bindings: Vec<TargetBinding>,
    ) -> Result<TransformNode, TransformError> {
        let declaration = self.create_binding_variable_statement(&bindings, NodeFlags::LET)?;
        if let NodeData::Block(mut data) = self.context.arena().node(statement)?.data.clone() {
            let original = data
                .statements
                .and_then(|array| self.context.arena().node_array_ref(self.source, array));
            let mut statements = self.array_nodes(data.statements)?;
            statements.insert(0, declaration);
            let statements = if let Some(original) = original {
                self.context
                    .factory()?
                    .update_node_array(original, statements)?
            } else {
                self.context
                    .factory()?
                    .create_node_array(self.source, statements)?
            };
            data.statements = Some(statements.array());
            let node_data = NodeData::Block(data);
            let flags = flags_after_update(self.context.arena(), statement, &node_data)?;
            return self
                .context
                .factory()?
                .update_node(statement, node_data, flags);
        }
        self.create_block(vec![declaration, statement], true)
    }

    fn allocate_temp_name(&mut self) -> Result<ClassBinding, TransformError> {
        self.allocate_temp_name_with_nested_scope_reservation(true)
    }

    fn allocate_class_temp_name(
        &mut self,
        plan: ClassTempPlan,
    ) -> Result<ClassBinding, TransformError> {
        self.allocate_temp_name_with_nested_scope_reservation_and_owner(true, plan.owner)
    }

    fn allocate_declaration_name(&mut self, base: &str) -> String {
        self.generated_bindings.allocate_local_numbered(base)
    }

    fn allocate_shadowable_temp_name(&mut self) -> Result<ClassBinding, TransformError> {
        self.allocate_temp_name_with_nested_scope_reservation(false)
    }

    fn allocate_temp_name_with_nested_scope_reservation(
        &mut self,
        reserve_in_nested_scopes: bool,
    ) -> Result<ClassBinding, TransformError> {
        self.allocate_temp_name_with_nested_scope_reservation_and_owner(
            reserve_in_nested_scopes,
            LexicalBindingOwner::Hoisted,
        )
    }

    fn allocate_temp_name_with_nested_scope_reservation_and_owner(
        &mut self,
        reserve_in_nested_scopes: bool,
        owner: LexicalBindingOwner,
    ) -> Result<ClassBinding, TransformError> {
        self.record_generated_binding_origin()?;
        let provisional = self.generated_bindings.allocate_temp();
        let binding = if reserve_in_nested_scopes {
            TargetBinding::allocate_reserved_in_nested_scopes(self.context, provisional)?
        } else {
            TargetBinding::allocate(self.context, provisional)?
        };
        let owner = self.claim_lexical_binding_owner(&binding, owner);
        self.generated_binding_frames
            .last_mut()
            .expect("class lowering owns an active binding frame")
            .push(PlannedTargetBinding {
                binding: binding.clone(),
                owner,
            });
        Ok(ClassBinding::Generated(binding))
    }

    fn allocate_private_name(
        &mut self,
        base: String,
        role: PrivateGeneratedNameRole,
        owner: LexicalBindingOwner,
    ) -> Result<ClassBinding, TransformError> {
        self.record_generated_binding_origin()?;
        // `getGeneratedNameForNode(privateName, ..., suffix)` assigns the
        // collision ordinal to the source node before applying the role
        // suffix: `_foo_1_get`, never `_foo_get_1`.
        let provisional = self
            .generated_bindings
            .allocate_preferred_with_role_suffix(&base, role.suffix());
        let binding = TargetBinding::allocate_preferred_with_role_suffix_reserved_in_nested_scopes(
            self.context,
            base,
            role.suffix().to_owned(),
            provisional,
        )?;
        let owner = self.claim_lexical_binding_owner(&binding, owner);
        self.generated_binding_frames
            .last_mut()
            .expect("class lowering owns an active binding frame")
            .push(PlannedTargetBinding {
                binding: binding.clone(),
                owner,
            });
        Ok(ClassBinding::Generated(binding))
    }

    fn claim_lexical_binding_owner(
        &self,
        binding: &TargetBinding,
        requested: LexicalBindingOwner,
    ) -> LexicalBindingOwner {
        if requested == LexicalBindingOwner::CurrentLoop
            && self.loop_binding_scopes.add(binding.clone())
        {
            LexicalBindingOwner::CurrentLoop
        } else {
            LexicalBindingOwner::Hoisted
        }
    }

    fn record_generated_binding_origin(&mut self) -> Result<(), TransformError> {
        if self
            .context
            .lexical_environment_flags()
            .contains(LexicalEnvironmentFlags::IN_PARAMETERS)
        {
            self.context.set_lexical_environment_flags(
                LexicalEnvironmentFlags::VARIABLES_HOISTED_IN_PARAMETERS,
                true,
            )?;
        }
        Ok(())
    }

    fn assert_generated_binding_plan(
        &self,
        planned: &GeneratedBindings,
        identities: &ClassGeneratedBindings,
    ) {
        debug_assert_eq!(planned.names().is_empty(), identities.bindings().is_empty());
        debug_assert_eq!(planned.names().len(), identities.bindings().len());
        debug_assert!(planned
            .names()
            .iter()
            .zip(identities.bindings())
            .all(|(planned, binding)| planned == binding.binding.provisional_name()));
    }

    /// Allocate the superclass binding at the semantic point that owns it.
    ///
    /// `getClassFacts` may report lexical `super` because a nested class
    /// propagates its transform flag through a relocated static block. tsc
    /// does not allocate from that fact alone: `visitExpressionWithTypeArgumentsInHeritageClause`
    /// creates the temp only when it reaches an actual `extends` expression.
    /// Keeping the fact and binding separate prevents an outer base class from
    /// hoisting an orphan temp while preserving allocation order for a real
    /// derived class.
    ///
    /// tsc-port: getClassFacts @6.0.3
    /// tsc-hash: 18ea59522a3e87f378c8b5682c5eb2172be55cba02380fc3b240acbf0f4dd388
    /// tsc-span: _tsc.js:96844-96898
    ///
    /// tsc-port: visitExpressionWithTypeArgumentsInHeritageClause @6.0.3
    /// tsc-hash: c49a28d05845a0aa57927b363dd8b8ca4fd65a6cbb483a630cee5fedd0a8dc2c
    /// tsc-span: _tsc.js:96899-96919
    fn allocate_super_base_binding(
        &mut self,
        heritage: Option<NodeArrayId>,
        needs_super_reference: bool,
    ) -> Result<Option<ClassBinding>, TransformError> {
        if !needs_super_reference {
            return Ok(None);
        }
        for clause in self.array_nodes(heritage)? {
            let NodeData::HeritageClause(clause) = &self.context.arena().node(clause)?.data else {
                continue;
            };
            if clause.token != SyntaxKind::ExtendsKeyword {
                continue;
            }
            let Some(first_type) = self.array_nodes(clause.types)?.first().copied() else {
                continue;
            };
            let NodeData::ExpressionWithTypeArguments(base) =
                &self.context.arena().node(first_type)?.data
            else {
                continue;
            };
            if base.expression.is_some() {
                return self.allocate_temp_name().map(Some);
            }
        }
        Ok(None)
    }

    fn capture_super_base(
        &mut self,
        heritage: Option<NodeArrayId>,
        super_alias: Option<&ClassBinding>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let (Some(heritage), Some(super_alias)) = (heritage, super_alias) else {
            return Ok(heritage);
        };
        let original_array = self.array(heritage);
        let mut clauses = self.array_nodes(Some(heritage))?;
        for clause in &mut clauses {
            let NodeData::HeritageClause(mut clause_data) =
                self.context.arena().node(*clause)?.data.clone()
            else {
                continue;
            };
            if clause_data.token != SyntaxKind::ExtendsKeyword {
                continue;
            }
            let Some(types) = clause_data.types else {
                continue;
            };
            let original_types = self.array(types);
            let mut type_nodes = self.array_nodes(Some(types))?;
            let Some(first_type) = type_nodes.first_mut() else {
                continue;
            };
            let NodeData::ExpressionWithTypeArguments(mut type_data) =
                self.context.arena().node(*first_type)?.data.clone()
            else {
                continue;
            };
            let expression = type_data
                .expression
                .map(|expression| self.node(expression))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ExpressionWithTypeArguments,
                    field: "expression",
                })?;
            let alias = self.create_binding_identifier(super_alias)?;
            let assignment = self.create_assignment(alias, expression)?;
            let assignment = self.create_parenthesized(assignment)?;
            type_data.expression = Some(assignment.node());
            let type_flags = flags_after_update(
                self.context.arena(),
                *first_type,
                &NodeData::ExpressionWithTypeArguments(type_data.clone()),
            )?;
            *first_type = self.context.factory()?.update_node(
                *first_type,
                NodeData::ExpressionWithTypeArguments(type_data),
                type_flags,
            )?;
            let types = self
                .context
                .factory()?
                .update_node_array(original_types, type_nodes)?;
            clause_data.types = Some(types.array());
            let clause_flags = flags_after_update(
                self.context.arena(),
                *clause,
                &NodeData::HeritageClause(clause_data.clone()),
            )?;
            *clause = self.context.factory()?.update_node(
                *clause,
                NodeData::HeritageClause(clause_data),
                clause_flags,
            )?;
            break;
        }
        Ok(Some(
            self.context
                .factory()?
                .update_node_array(original_array, clauses)?
                .array(),
        ))
    }

    /// tsc-port: transformConstructor/getEffectiveBaseTypeNode @6.0.3
    /// tsc-hash: 2a7a52a87db9d1910946b8c2fa416a4e223b21af04e716e7b392303703dec2f4
    /// tsc-span: _tsc.js:97253-97260
    ///
    /// `extends null` is syntactically an extends clause but has base-class
    /// constructor semantics. Runtime-transparent wrappers do not change that
    /// fact, so constructor synthesis must classify the unwrapped expression
    /// instead of using clause presence as a proxy for derivation.
    fn class_heritage_semantics(
        &self,
        heritage: Option<NodeArrayId>,
    ) -> Result<ClassHeritageSemantics, TransformError> {
        for clause in self.array_nodes(heritage)? {
            let NodeData::HeritageClause(data) = &self.context.arena().node(clause)?.data else {
                continue;
            };
            if data.token != SyntaxKind::ExtendsKeyword {
                continue;
            }
            let Some(first_type) = self.array_nodes(data.types)?.first().copied() else {
                return Ok(ClassHeritageSemantics::NoExtends);
            };
            let NodeData::ExpressionWithTypeArguments(data) =
                &self.context.arena().node(first_type)?.data
            else {
                return Ok(ClassHeritageSemantics::ExtendsValue);
            };
            let expression = data
                .expression
                .map(|expression| self.node(expression))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ExpressionWithTypeArguments,
                    field: "expression",
                })?;
            let expression = self.skip_runtime_transparent_outer_expressions(expression)?;
            return Ok(
                if self.context.arena().node(expression)?.kind == SyntaxKind::NullKeyword {
                    ClassHeritageSemantics::ExtendsNull
                } else {
                    ClassHeritageSemantics::ExtendsValue
                },
            );
        }
        Ok(ClassHeritageSemantics::NoExtends)
    }

    /// Rust ownership form of `skipOuterExpressions`: only wrappers whose
    /// runtime evaluation is exactly their child are transparent. In
    /// particular this deliberately does not descend through comma,
    /// conditional, call, or assignment expressions, where moving a field
    /// initializer past the containing statement could change evaluation
    /// order.
    fn skip_runtime_transparent_outer_expressions(
        &self,
        mut expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        loop {
            let record = self.context.arena().node(expression)?;
            let inner = match &record.data {
                NodeData::ParenthesizedExpression(data) => data.expression,
                NodeData::PartiallyEmittedExpression(data) => data.expression,
                NodeData::TypeAssertionExpression(data) => data.expression,
                NodeData::AsExpression(data) => data.expression,
                NodeData::SatisfiesExpression(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                NodeData::ExpressionWithTypeArguments(data) => data.expression,
                _ => return Ok(expression),
            };
            expression = inner.map(|inner| self.node(inner)).ok_or(
                TransformError::RequiredChildRemoved {
                    parent: record.kind,
                    field: "expression",
                },
            )?;
        }
    }

    fn statement_is_super_call(&self, statement: TransformNode) -> Result<bool, TransformError> {
        let NodeData::ExpressionStatement(data) = &self.context.arena().node(statement)?.data
        else {
            return Ok(false);
        };
        let Some(expression) = data.expression else {
            return Ok(false);
        };
        let expression = self.skip_runtime_transparent_outer_expressions(self.node(expression))?;
        let NodeData::CallExpression(data) = &self.context.arena().node(expression)?.data else {
            return Ok(false);
        };
        Ok(data.expression.is_some_and(|expression| {
            self.context
                .arena()
                .node(self.node(expression))
                .is_ok_and(|node| node.kind == SyntaxKind::SuperKeyword)
        }))
    }

    fn is_prologue_statement(&self, statement: TransformNode) -> Result<bool, TransformError> {
        let NodeData::ExpressionStatement(data) = &self.context.arena().node(statement)?.data
        else {
            return Ok(false);
        };
        Ok(data.expression.is_some_and(|expression| {
            self.context
                .arena()
                .node(self.node(expression))
                .is_ok_and(|node| matches!(node.data, NodeData::StringLiteral(_)))
        }))
    }

    fn original_kind(&self, node: TransformNode) -> Option<SyntaxKind> {
        let original = self.context.arena().get_original_node(node);
        self.context
            .arena()
            .node(original)
            .ok()
            .map(|node| node.kind)
    }

    fn parameter_property_local(
        &self,
        property: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let original = self.context.arena().get_original_node(property);
        let NodeData::Parameter(data) = &self.context.arena().node(original)?.data else {
            return Ok(None);
        };
        let Some(name) = data
            .name
            .and_then(|name| self.context.arena().node_ref(self.source, name))
        else {
            return Ok(None);
        };
        Ok((self.context.arena().node(name)?.kind == SyntaxKind::Identifier).then_some(name))
    }

    fn name_is_private(&self, name: Option<NodeId>) -> Result<bool, TransformError> {
        Ok(name.is_some_and(|name| {
            self.context
                .arena()
                .node(self.node(name))
                .is_ok_and(|name| name.kind == SyntaxKind::PrivateIdentifier)
        }))
    }

    fn has_modifier(
        &self,
        modifiers: Option<NodeArrayId>,
        expected: SyntaxKind,
    ) -> Result<bool, TransformError> {
        Ok(self.array_nodes(modifiers)?.iter().any(|modifier| {
            self.context
                .arena()
                .node(*modifier)
                .is_ok_and(|modifier| modifier.kind == expected)
        }))
    }

    fn create_identifier(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                escaped_text: tsc_syntax::escape_leading_underscores(text),
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_binding_identifier(
        &mut self,
        binding: &ClassBinding,
    ) -> Result<TransformNode, TransformError> {
        let identifier = self.create_identifier(binding.planned_text())?;
        binding.write_generated_metadata(self.context.arena_mut()?, identifier);
        Ok(identifier)
    }

    fn create_string_literal(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::StringLiteral(tsc_syntax::nodes::StringLiteralData {
                text: text.to_owned(),
                has_extended_unicode_escape: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_set_function_name_expression(
        &mut self,
        binding: &ClassBinding,
        assigned_name: &AssignedClassName,
    ) -> Result<TransformNode, TransformError> {
        self.context
            .request_emit_helper(super::super::helpers::set_function_name())?;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::SetFunctionName)?;
        let value = self.create_binding_identifier(binding)?;
        let assigned_name = match assigned_name {
            AssignedClassName::Literal(text) => self.create_string_literal(text)?,
            AssignedClassName::Evaluated(expression) => {
                self.context.factory()?.clone_node(*expression)?
            }
        };
        self.create_call(helper, vec![value, assigned_name])
    }

    fn create_boolean(&mut self, value: bool) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_token(
            self.source,
            if value {
                SyntaxKind::TrueKeyword
            } else {
                SyntaxKind::FalseKeyword
            },
            TransformFlags::NONE,
        )
    }

    fn create_void_zero(&mut self) -> Result<TransformNode, TransformError> {
        let zero = self.context.factory()?.create_node(
            self.source,
            NodeData::NumericLiteral(tsc_syntax::nodes::NumericLiteralData {
                text: "0".to_owned(),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VoidExpression(tsc_syntax::nodes::VoidExpressionData {
                expression: Some(zero.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_property_access(
        &mut self,
        expression: TransformNode,
        name: &str,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAccessExpression(tsc_syntax::nodes::PropertyAccessExpressionData {
                expression: Some(expression.node()),
                question_dot_token: None,
                name: Some(name.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_property_access_chain(
        &mut self,
        expression: TransformNode,
        question_dot_token: Option<NodeId>,
        name: &str,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        let access = self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAccessExpression(tsc_syntax::nodes::PropertyAccessExpressionData {
                expression: Some(expression.node()),
                question_dot_token,
                name: Some(name.node()),
            }),
            TransformFlags::NONE,
        )?;
        self.context
            .factory()?
            .set_node_flags(access, NodeFlags::OPTIONAL_CHAIN)
    }

    fn create_reflect_get(
        &mut self,
        super_alias: &ClassBinding,
        key: TransformNode,
        class_alias: &ClassBinding,
    ) -> Result<TransformNode, TransformError> {
        let reflect = self.create_identifier("Reflect")?;
        let get = self.create_property_access(reflect, "get")?;
        let super_alias = self.create_binding_identifier(super_alias)?;
        let class_alias = self.create_binding_identifier(class_alias)?;
        self.create_call(get, vec![super_alias, key, class_alias])
    }

    fn create_reflect_set(
        &mut self,
        super_alias: &ClassBinding,
        key: TransformNode,
        value: TransformNode,
        class_alias: &ClassBinding,
    ) -> Result<TransformNode, TransformError> {
        let reflect = self.create_identifier("Reflect")?;
        let set = self.create_property_access(reflect, "set")?;
        let super_alias = self.create_binding_identifier(super_alias)?;
        let class_alias = self.create_binding_identifier(class_alias)?;
        self.create_call(set, vec![super_alias, key, value, class_alias])
    }

    fn create_property_assignment(
        &mut self,
        name: &str,
        initializer: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::PropertyAssignment(tsc_syntax::nodes::PropertyAssignmentData {
                name: Some(name.node()),
                initializer: Some(initializer.node()),
                modifiers: None,
                question_token: None,
                exclamation_token: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_object_literal(
        &mut self,
        properties: Vec<TransformNode>,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let properties = self
            .context
            .factory()?
            .create_node_array(self.source, properties)?;
        let object = self.context.factory()?.create_node(
            self.source,
            NodeData::ObjectLiteralExpression(tsc_syntax::nodes::ObjectLiteralExpressionData {
                properties: Some(properties.array()),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.set_multi_line(object, multi_line)
    }

    fn create_call(
        &mut self,
        expression: TransformNode,
        arguments: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let arguments = self
            .context
            .factory()?
            .create_node_array(self.source, arguments)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::CallExpression(tsc_syntax::nodes::CallExpressionData {
                expression: Some(expression.node()),
                question_dot_token: None,
                type_arguments: None,
                arguments: Some(arguments.array()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_new(
        &mut self,
        expression: TransformNode,
        arguments: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let arguments = self
            .context
            .factory()?
            .create_node_array(self.source, arguments)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::NewExpression(tsc_syntax::nodes::NewExpressionData {
                expression: Some(expression.node()),
                type_arguments: None,
                arguments: Some(arguments.array()),
                question_dot_token: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_assignment(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.create_binary(left, SyntaxKind::EqualsToken, right)
    }

    fn create_binary(
        &mut self,
        left: TransformNode,
        operator: SyntaxKind,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let operator =
            self.context
                .factory()?
                .create_token(self.source, operator, TransformFlags::NONE)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
                left: Some(left.node()),
                operator_token: Some(operator.node()),
                right: Some(right.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn inline_expressions(
        &mut self,
        mut expressions: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let first = expressions.remove(0);
        expressions.into_iter().try_fold(first, |left, right| {
            self.create_binary(left, SyntaxKind::CommaToken, right)
        })
    }

    fn create_parenthesized(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::ParenthesizedExpression(tsc_syntax::nodes::ParenthesizedExpressionData {
                expression: Some(expression.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_expression_statement(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::ExpressionStatement(tsc_syntax::nodes::ExpressionStatementData {
                expression: Some(expression.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_export_default(&mut self, local_name: &str) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(local_name)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ExportAssignment(tsc_syntax::nodes::ExportAssignmentData {
                modifiers: None,
                is_export_equals: Some(false),
                expression: Some(name.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_variable_declaration(
        &mut self,
        name: TransformNode,
        initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(name.node()),
                exclamation_token: None,
                r#type: None,
                initializer: initializer.map(TransformNode::node),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_variable_statement(
        &mut self,
        declarations: Vec<TransformNode>,
        flags: NodeFlags,
    ) -> Result<TransformNode, TransformError> {
        let declarations = self
            .context
            .factory()?
            .create_node_array(self.source, declarations)?;
        let list = self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclarationList(tsc_syntax::nodes::VariableDeclarationListData {
                declarations: Some(declarations.array()),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.set_node_flags(list, flags)?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VariableStatement(tsc_syntax::nodes::VariableStatementData {
                modifiers: None,
                declaration_list: Some(list.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_arrow_function(
        &mut self,
        parameters: Vec<TransformNode>,
        body: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, parameters)?;
        let arrow = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::EqualsGreaterThanToken,
            TransformFlags::NONE,
        )?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ArrowFunction(tsc_syntax::nodes::ArrowFunctionData {
                type_parameters: None,
                parameters: Some(parameters.array()),
                r#type: None,
                body: Some(body.node()),
                modifiers: None,
                equals_greater_than_token: Some(arrow.node()),
            }),
            TransformFlags::CONTAINS_LEXICAL_THIS,
        )
    }

    fn create_block(
        &mut self,
        statements: Vec<TransformNode>,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let statements = self
            .context
            .factory()?
            .create_node_array(self.source, statements)?;
        let block = self.context.factory()?.create_node(
            self.source,
            NodeData::Block(tsc_syntax::nodes::BlockData {
                statements: Some(statements.array()),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.set_multi_line(block, multi_line)
    }

    fn update_generic(
        &mut self,
        original: TransformNode,
        mut data: NodeData,
    ) -> Result<NodeId, TransformError> {
        try_visit_each_child(&mut data, self)?;
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        Ok(self
            .context
            .factory()?
            .update_node(original, data, flags)?
            .node())
    }

    fn visit_optional_node(
        &mut self,
        node: Option<NodeId>,
    ) -> Result<Option<NodeId>, TransformError> {
        node.map(|node| self.visit(node))
            .transpose()
            .map(Option::flatten)
    }

    fn visit_optional_static_node(
        &mut self,
        node: Option<NodeId>,
    ) -> Result<Option<NodeId>, TransformError> {
        node.map(|node| self.visit_static_node(node))
            .transpose()
            .map(Option::flatten)
            .map(|node| node.map(TransformNode::node))
    }

    fn visit_static_node(&mut self, node: NodeId) -> Result<Option<TransformNode>, TransformError> {
        let bindings = self.static_bindings();
        let _static_binding_scope = self
            .static_binding_frames
            .enter(StaticBindingFrame::StaticEvaluation(bindings));
        self.visit(node)
            .map(|node| node.map(|node| self.node(node)))
    }

    fn with_static_auto_accessor_bindings<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, TransformError>,
    ) -> Result<T, TransformError> {
        let bindings = self.static_auto_accessor_bindings();
        let _static_binding_scope = self
            .static_binding_frames
            .enter(StaticBindingFrame::StaticEvaluation(Some(bindings)));
        operation(self)
    }

    fn visit_required(
        &mut self,
        node: Option<NodeId>,
        parent: SyntaxKind,
        field: &'static str,
    ) -> Result<TransformNode, TransformError> {
        let node = node.ok_or(TransformError::RequiredChildRemoved { parent, field })?;
        self.visit(node)?
            .map(|node| self.node(node))
            .ok_or(TransformError::RequiredChildRemoved { parent, field })
    }

    fn visit_optional_nodes(
        &mut self,
        nodes: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        nodes
            .map(|nodes| self.visit_nodes(nodes))
            .transpose()
            .map(Option::flatten)
    }

    fn visit_node_array(
        &mut self,
        nodes: Option<NodeArrayId>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let mut visited = Vec::new();
        for node in self.array_nodes(nodes)? {
            if let Some(node) = self.visit(node.node())? {
                visited.push(self.node(node));
            }
        }
        Ok(visited)
    }

    fn array_nodes(
        &self,
        array: Option<NodeArrayId>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let Some(array) = array.and_then(|id| self.context.arena().node_array_ref(self.source, id))
        else {
            return Ok(Vec::new());
        };
        self.context
            .arena()
            .node_array(array)?
            .nodes
            .iter()
            .map(|id| {
                self.context
                    .arena()
                    .node_ref(self.source, *id)
                    .ok_or_else(|| TransformError::UnknownNode(self.node(*id)))
            })
            .collect()
    }

    fn identifier_text(&self, node: TransformNode) -> Option<&str> {
        match &self.context.arena().node(node).ok()?.data {
            NodeData::Identifier(data) => Some(&data.text),
            _ => None,
        }
    }

    fn set_original_and_range(
        &mut self,
        node: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.set_text_range(node, original)?;
        self.context
            .arena_mut()?
            .set_original_node(node, Some(original))?;
        Ok(node)
    }

    fn resolver_node(&self, node: TransformNode) -> Result<EmitResolverNode, TransformError> {
        self.context.arena().require_parse_tree_resolver_node(node)
    }

    const fn node(&self, id: NodeId) -> TransformNode {
        TransformNode::new(self.source, id)
    }

    const fn array(&self, id: NodeArrayId) -> TransformNodeArray {
        TransformNodeArray::new(self.source, id)
    }
}

impl NodeDataChildVisitor for DownlevelClassVisitor<'_, '_, '_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(self.node(id))
            .expect("downlevel class child belongs to the current transform source")
            .kind
    }

    fn visit_node(&mut self, id: NodeId) -> Result<Option<NodeId>, Self::Error> {
        self.visit(id)
    }

    fn visit_nodes(&mut self, id: NodeArrayId) -> Result<Option<NodeArrayId>, Self::Error> {
        if let Some(mapped) = self.arrays.get(&id) {
            return Ok(*mapped);
        }
        let original = self.array(id);
        let nodes = self.context.arena().node_array(original)?.nodes.clone();
        let mut visited = Vec::with_capacity(nodes.len());
        for original_node in nodes {
            if let Some(node) = self.visit(original_node)? {
                visited.push(self.node(node));
                let expanded = self
                    .expanded_statements
                    .get(&node)
                    .or_else(|| self.expanded_statements.get(&original_node))
                    .cloned();
                if let Some(expanded) = expanded {
                    visited.extend(expanded.into_iter().map(|node| self.node(node)));
                }
            }
        }
        let updated = self
            .context
            .factory()?
            .update_node_array(original, visited)?;
        let mapped = Some(updated.array());
        self.arrays.insert(id, mapped);
        Ok(mapped)
    }

    fn required_child_removed(&mut self, parent: SyntaxKind, field: &'static str) -> Self::Error {
        TransformError::RequiredChildRemoved { parent, field }
    }
}
