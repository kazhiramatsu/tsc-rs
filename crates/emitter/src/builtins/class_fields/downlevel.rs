//! ES2021 class-element lowering.
//!
//! This module plans a class as retained members plus ordered instance and
//! static operations.  The representation keeps target policy out of the AST
//! walk and gives private storage and static-super aliases one ownership point.

use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{
    for_each_child, try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId,
    SyntaxKind,
};
use tsc_types::NodeFlags;

use crate::{
    EmitFlags, EmitHelper, InternalEmitFlags, TransformArena, TransformError, TransformFlags,
    TransformNode, TransformNodeArray, TransformSourceId, TransformationContext,
};

use super::super::{
    flags_after_update,
    generated_bindings::{
        AncestorBindingPolicy, GeneratedBindingOwner, GeneratedBindingScopes, GeneratedBindings,
    },
    system::collect_identifier_texts,
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
    initializer: Option<NodeId>,
}

struct PlannedPropertyName {
    name: NodeId,
    evaluation: Option<TransformNode>,
}

#[derive(Clone)]
struct PrivateSlot {
    placement: PrivatePlacement,
    element: PrivateElement,
}

#[derive(Clone)]
enum PrivatePlacement {
    Instance { brand_name: String },
    Static { class_alias: String },
}

#[derive(Clone)]
enum PrivateElement {
    Field {
        value_name: String,
    },
    Method {
        method_name: String,
    },
    Accessor {
        getter_name: Option<String>,
        setter_name: Option<String>,
    },
}

impl PrivateSlot {
    fn is_static(&self) -> bool {
        matches!(self.placement, PrivatePlacement::Static { .. })
    }

    fn brand_name(&self) -> &str {
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

    fn getter_descriptor_name(&self) -> Option<&str> {
        match &self.element {
            PrivateElement::Field { value_name } => self.is_static().then_some(value_name),
            PrivateElement::Method { method_name } => Some(method_name),
            PrivateElement::Accessor { getter_name, .. } => getter_name.as_deref(),
        }
    }

    fn setter_descriptor_name(&self) -> Option<&str> {
        match &self.element {
            PrivateElement::Field { value_name } => self.is_static().then_some(value_name),
            PrivateElement::Method { .. } => None,
            PrivateElement::Accessor { setter_name, .. } => setter_name.as_deref(),
        }
    }

    fn field_value_name(&self) -> Option<&str> {
        match &self.element {
            PrivateElement::Field { value_name } => Some(value_name),
            _ => None,
        }
    }
}

#[derive(Clone, Default)]
struct PrivateEnvironment {
    slots: BTreeMap<String, PrivateSlot>,
    class_alias: Option<String>,
    instance_brand: Option<String>,
    super_alias: Option<String>,
}

#[derive(Clone)]
struct StaticBindings {
    class_alias: String,
    super_alias: Option<String>,
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
enum InstanceOperation {
    PrivateBrand(String),
    Public(FieldOperation),
    PrivateField(PrivateFieldOperation),
}

#[derive(Clone)]
enum StaticOperation {
    Field(FieldOperation),
    PrivateField(PrivateFieldOperation),
    Block {
        original: TransformNode,
        body: TransformNode,
    },
}

#[derive(Clone)]
struct PrivateDefinition {
    original: TransformNode,
    name: String,
    function: TransformNode,
}

enum PrivateDeclarationKind {
    Field,
    Method,
    Accessor { has_getter: bool, has_setter: bool },
}

struct PrivateDeclaration {
    name: String,
    is_static: bool,
    kind: PrivateDeclarationKind,
}

#[derive(Default)]
struct ClassSetup {
    field_storages: Vec<PrivateSlot>,
    instance_brand: Option<String>,
    auto_accessor_storages: Vec<PrivateSlot>,
    definitions: Vec<PrivateDefinition>,
}

impl ClassSetup {
    fn is_empty(&self) -> bool {
        self.field_storages.is_empty()
            && self.instance_brand.is_none()
            && self.auto_accessor_storages.is_empty()
            && self.definitions.is_empty()
    }
}

#[derive(Default)]
struct ClassOperations {
    key_evaluations: Vec<TransformNode>,
    retained_members: Vec<TransformNode>,
    instance: Vec<InstanceOperation>,
    setup: ClassSetup,
    static_: Vec<StaticOperation>,
}

struct StabilizedReceiver {
    read: TransformNode,
    initialized: Option<TransformNode>,
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

pub(super) fn transform_source(
    context: &mut TransformationContext,
    source: TransformSourceId,
    use_define_for_class_fields: bool,
    class_aliases: &mut BTreeMap<(u32, u32), Box<str>>,
) -> Result<(), TransformError> {
    let root = context.arena().root(source)?;
    let mode = if use_define_for_class_fields {
        PublicFieldMode::DefineProperty
    } else {
        PublicFieldMode::Assignment
    };
    let tree_ownership = OriginalTreeOwnership::collect(context.arena(), source, root.node())?;
    let mut visitor =
        DownlevelClassVisitor::new(context, source, mode, tree_ownership, class_aliases);
    let transformed = visitor
        .visit(root.node())?
        .ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::SourceFile,
            field: "root",
        })?;
    let source_bindings = visitor.generated_bindings.source_bindings();
    let transformed = visitor
        .prepend_generated_declarations_to_source(visitor.node(transformed), source_bindings)?;
    visitor
        .context
        .arena_mut()?
        .replace_root(source, transformed)?;
    Ok(())
}

struct DownlevelClassVisitor<'context, 'aliases> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    mode: PublicFieldMode,
    nodes: BTreeMap<NodeId, Option<NodeId>>,
    arrays: BTreeMap<NodeArrayId, Option<NodeArrayId>>,
    expanded_statements: BTreeMap<NodeId, Vec<NodeId>>,
    generated_bindings: GeneratedBindingScopes,
    private_environments: Vec<PrivateEnvironment>,
    active_static_bindings: Option<StaticBindings>,
    generated_static_auto_accessors: BTreeSet<NodeId>,
    generated_auto_accessor_backings: BTreeSet<NodeId>,
    tree_ownership: OriginalTreeOwnership,
    class_aliases: &'aliases mut BTreeMap<(u32, u32), Box<str>>,
}

impl<'context, 'aliases> DownlevelClassVisitor<'context, 'aliases> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        mode: PublicFieldMode,
        tree_ownership: OriginalTreeOwnership,
        class_aliases: &'aliases mut BTreeMap<(u32, u32), Box<str>>,
    ) -> Self {
        Self {
            generated_bindings: GeneratedBindingScopes::new(
                collect_identifier_texts(context.arena(), source),
                AncestorBindingPolicy::Reserve,
            ),
            context,
            source,
            mode,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
            expanded_statements: BTreeMap::new(),
            private_environments: Vec::new(),
            active_static_bindings: None,
            generated_static_auto_accessors: BTreeSet::new(),
            generated_auto_accessor_backings: BTreeSet::new(),
            tree_ownership,
            class_aliases,
        }
    }

    fn visit(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if let Some(mapped) = self.nodes.get(&id) {
            return Ok(*mapped);
        }
        let original = self.node(id);
        let record = self.context.arena().node(original)?.clone();
        let kind = record.kind;
        let transformed = match record.data {
            NodeData::ClassDeclaration(data) => Some(self.visit_class_declaration(original, data)?),
            NodeData::ClassExpression(data) => Some(self.visit_class_expression(original, data)?),
            NodeData::PropertyAccessExpression(data) => {
                Some(self.visit_property_access(original, data)?)
            }
            NodeData::ElementAccessExpression(data) => {
                Some(self.visit_element_access(original, data)?)
            }
            NodeData::BinaryExpression(data) => Some(self.visit_binary_expression(original, data)?),
            NodeData::CallExpression(data) => Some(self.visit_call_expression(original, data)?),
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
            NodeData::Token
                if kind == SyntaxKind::ThisKeyword && self.active_static_bindings.is_some() =>
            {
                let alias = self
                    .active_static_bindings
                    .as_ref()
                    .expect("guarded static bindings")
                    .class_alias
                    .clone();
                Some(self.create_identifier(&alias)?.node())
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
        let previous_static_bindings =
            (!captures_static_bindings).then(|| self.active_static_bindings.take());
        let transformed = self
            .with_new_generated_scope(GeneratedBindingOwner::FunctionBody, |visitor| {
                visitor.update_generic(original, data)
            });
        if let Some(previous) = previous_static_bindings {
            self.active_static_bindings = previous;
        }
        let (transformed, bindings) = transformed?;
        self.install_function_bindings(self.node(transformed), bindings)
            .map(TransformNode::node)
    }

    fn with_new_generated_scope<T>(
        &mut self,
        owner: GeneratedBindingOwner,
        operation: impl FnOnce(&mut Self) -> Result<T, TransformError>,
    ) -> Result<(T, GeneratedBindings), TransformError> {
        let (previous, scope) = self.generated_bindings.enter(owner);
        let result = operation(self);
        let bindings = self.generated_bindings.exit(previous, scope);
        result.map(|value| (value, bindings))
    }

    fn install_function_bindings(
        &mut self,
        function: TransformNode,
        bindings: GeneratedBindings,
    ) -> Result<TransformNode, TransformError> {
        if bindings.is_empty() {
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
            self.prepend_generated_declarations_to_block(body, bindings)?
        } else if record.kind == SyntaxKind::ArrowFunction {
            let declaration = self.create_generated_variable_statement(&bindings)?;
            let return_statement = self.context.factory()?.create_node(
                self.source,
                NodeData::ReturnStatement(tsc_syntax::nodes::ReturnStatementData {
                    expression: Some(body.node()),
                }),
                TransformFlags::NONE,
            )?;
            self.create_block(vec![declaration, return_statement], true)?
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

    fn visit_class_declaration(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ClassDeclarationData,
    ) -> Result<NodeId, TransformError> {
        data.members = self.expand_auto_accessors(data.members)?;
        let class_name = data
            .name
            .and_then(|name| self.identifier_text(self.node(name)).map(str::to_owned));
        let private_environment =
            self.prepare_private_environment(data.members, class_name.as_deref())?;
        let super_alias = private_environment.super_alias.clone();
        self.private_environments.push(private_environment);
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.heritage_clauses = self.visit_optional_nodes(data.heritage_clauses)?;
        data.heritage_clauses =
            self.capture_super_base(data.heritage_clauses, super_alias.as_deref())?;
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;

        let operations = self.plan_members(data.members)?;
        let mut retained = operations.retained_members;
        if !operations.instance.is_empty() {
            self.install_instance_operations(
                &mut retained,
                &operations.instance,
                self.has_extends_clause(data.heritage_clauses)?,
                class_name.as_deref(),
            )?;
        }
        let members = self
            .context
            .factory()?
            .create_node_array(self.source, retained)?;
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

        if !operations.setup.is_empty()
            || !operations.static_.is_empty()
            || !operations.key_evaluations.is_empty()
            || private_environment.class_alias.is_some()
        {
            let binding = class_name.unwrap_or_else(|| self.allocate_temp_name());
            let mut trailing = Vec::new();
            if let Some(evaluations) =
                self.materialize_class_key_evaluations(operations.key_evaluations)?
            {
                trailing.push(evaluations);
            }
            trailing.extend(self.materialize_static_operations(
                &binding,
                operations.setup,
                operations.static_,
                &private_environment,
                true,
            )?);
            self.expanded_statements.insert(
                class.node(),
                trailing.into_iter().map(TransformNode::node).collect(),
            );
        }
        Ok(class.node())
    }

    fn visit_class_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ClassExpressionData,
    ) -> Result<NodeId, TransformError> {
        data.members = self.expand_auto_accessors(data.members)?;
        let class_name = data
            .name
            .and_then(|name| self.identifier_text(self.node(name)).map(str::to_owned))
            .or(self.assigned_class_expression_name(original)?);
        let private_environment =
            self.prepare_private_environment(data.members, class_name.as_deref())?;
        let private_expression_binding = private_environment.class_alias.clone();
        let super_alias = private_environment.super_alias.clone();
        data.name = self.visit_optional_node(data.name)?;
        data.type_parameters = self.visit_optional_nodes(data.type_parameters)?;
        data.heritage_clauses = self.visit_optional_nodes(data.heritage_clauses)?;
        data.heritage_clauses =
            self.capture_super_base(data.heritage_clauses, super_alias.as_deref())?;
        data.modifiers = self.visit_optional_nodes(data.modifiers)?;
        self.private_environments.push(private_environment);
        let operations = self.plan_members(data.members)?;
        let needs_expression_binding = private_expression_binding.is_some()
            || !operations.key_evaluations.is_empty()
            || !operations.setup.is_empty()
            || !operations.static_.is_empty();
        // tsc allocates computed-key captures while visiting class members and
        // allocates the class-expression receiver afterwards. Keeping that
        // order here makes the binding plan stable without encoding names in
        // the operation representation.
        let expression_binding = private_expression_binding
            .or_else(|| needs_expression_binding.then(|| self.allocate_temp_name()));
        let mut retained = operations.retained_members;
        if !operations.instance.is_empty() {
            self.install_instance_operations(
                &mut retained,
                &operations.instance,
                self.has_extends_clause(data.heritage_clauses)?,
                class_name.as_deref(),
            )?;
        }
        let members = self
            .context
            .factory()?
            .create_node_array(self.source, retained)?;
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
        if operations.setup.is_empty()
            && operations.static_.is_empty()
            && operations.key_evaluations.is_empty()
            && private_environment.class_alias.is_none()
        {
            return Ok(class.node());
        }
        let binding = expression_binding.ok_or(TransformError::RequiredChildRemoved {
            parent: SyntaxKind::ClassExpression,
            field: "lowered class expression binding",
        })?;
        self.register_class_alias(original, &binding)?;

        // Class expressions cannot expand their containing statement.  A
        // comma expression owns the temporary class value and every ordered
        // static operation, then yields the class binding.
        let target = self.create_identifier(&binding)?;
        let assign_class = self.create_assignment(target, class)?;
        let mut expressions = vec![assign_class];
        expressions.extend(operations.key_evaluations);
        for statement in self.materialize_static_operations(
            &binding,
            operations.setup,
            operations.static_,
            &private_environment,
            false,
        )? {
            let NodeData::ExpressionStatement(data) =
                self.context.arena().node(statement)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ClassExpression,
                    field: "static expression",
                });
            };
            if let Some(expression) = data.expression {
                expressions.push(self.node(expression));
            }
        }
        expressions.push(self.create_identifier(&binding)?);
        let expression = self.inline_class_expression(expressions, class, original)?;
        Ok(expression.node())
    }

    fn register_class_alias(
        &mut self,
        declaration: TransformNode,
        alias: &str,
    ) -> Result<(), TransformError> {
        let declaration = self.context.arena().get_original_node(declaration);
        let program_source = self
            .context
            .arena()
            .source(declaration.source())?
            .program_source()
            .ok_or(TransformError::MissingProgramSource(declaration))?;
        self.class_aliases.insert(
            (program_source.raw(), declaration.node().0),
            alias.to_owned().into_boxed_str(),
        );
        Ok(())
    }

    /// Resolve the named-evaluation identity of an anonymous class expression
    /// from the current transform tree. This is deliberately derived from the
    /// pass-owned ownership index rather than parser parent pointers, which
    /// may be stale after earlier transforms.
    fn assigned_class_expression_name(
        &self,
        class: TransformNode,
    ) -> Result<Option<String>, TransformError> {
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
                    data.name.and_then(|name| self.assigned_name_text(name))
                }
                NodeData::Parameter(data) if data.initializer == Some(current) => {
                    data.name.and_then(|name| self.assigned_name_text(name))
                }
                NodeData::BindingElement(data) if data.initializer == Some(current) => {
                    data.name.and_then(|name| self.assigned_name_text(name))
                }
                NodeData::PropertyDeclaration(data) if data.initializer == Some(current) => {
                    data.name.and_then(|name| self.assigned_name_text(name))
                }
                NodeData::PropertyAssignment(data) if data.initializer == Some(current) => {
                    data.name.and_then(|name| self.assigned_name_text(name))
                }
                NodeData::ShorthandPropertyAssignment(data)
                    if data.object_assignment_initializer == Some(current) =>
                {
                    data.name.and_then(|name| self.assigned_name_text(name))
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
                    Some("default".to_owned())
                }
                _ => None,
            };
            return Ok(assigned);
        }
        Ok(None)
    }

    fn assignment_target_name(&self, target: NodeId) -> Option<String> {
        match &self.context.arena().node(self.node(target)).ok()?.data {
            NodeData::Identifier(data) => Some(data.text.clone()),
            NodeData::PropertyAccessExpression(data) => {
                data.name.and_then(|name| self.assigned_name_text(name))
            }
            NodeData::ElementAccessExpression(data) => data
                .argument_expression
                .and_then(|name| self.assigned_name_text(name)),
            _ => None,
        }
    }

    fn assigned_name_text(&self, name: NodeId) -> Option<String> {
        match &self.context.arena().node(self.node(name)).ok()?.data {
            NodeData::Identifier(data) => Some(data.text.clone()),
            NodeData::StringLiteral(data) => Some(data.text.clone()),
            NodeData::NumericLiteral(data) => Some(data.text.clone()),
            NodeData::ComputedPropertyName(data) => data
                .expression
                .and_then(|name| self.assigned_name_text(name)),
            _ => None,
        }
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
            let NodeData::PropertyDeclaration(data) = &self.context.arena().node(*member)?.data
            else {
                continue;
            };
            let Some(name) = data.name.map(|name| self.node(name)) else {
                continue;
            };
            if let NodeData::PrivateIdentifier(data) = &self.context.arena().node(name)?.data {
                used_private_names.insert(data.text.clone());
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
            let base = match &self.context.arena().node(self.node(name))?.data {
                NodeData::Identifier(data) => data.text.trim_start_matches('#').to_owned(),
                NodeData::PrivateIdentifier(data) => data.text.trim_start_matches('#').to_owned(),
                _ => "accessor".to_owned(),
            };
            let mut storage_text = format!("#{base}_accessor_storage");
            let mut ordinal = 1usize;
            while !used_private_names.insert(storage_text.clone()) {
                storage_text = format!("#{base}_{ordinal}_accessor_storage");
                ordinal += 1;
            }
            let storage = self.create_private_identifier(&storage_text)?;
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
            self.set_original_and_range(backing, member)?;
            let getter = self.create_auto_accessor_getter(name, storage.node(), modifiers)?;
            let setter = self.create_auto_accessor_setter(name, storage.node(), modifiers)?;
            self.set_original_and_range(getter, member)?;
            self.set_original_and_range(setter, member)?;
            self.context
                .arena_mut()?
                .metadata_mut(getter)
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

    fn prepare_private_environment(
        &mut self,
        members: Option<NodeArrayId>,
        class_name: Option<&str>,
    ) -> Result<PrivateEnvironment, TransformError> {
        let static_facts = self.static_lexical_facts(members)?;
        let mut declarations = Vec::<PrivateDeclaration>::new();
        let mut auto_accessor_backings = Vec::<PrivateDeclaration>::new();
        for member in self.array_nodes(members)? {
            let record = self.context.arena().node(member)?;
            let (name, modifiers, kind) = match &record.data {
                NodeData::PropertyDeclaration(data) => {
                    (data.name, data.modifiers, PrivateDeclarationKind::Field)
                }
                NodeData::MethodDeclaration(data) => {
                    (data.name, data.modifiers, PrivateDeclarationKind::Method)
                }
                NodeData::GetAccessor(data) => (
                    data.name,
                    data.modifiers,
                    PrivateDeclarationKind::Accessor {
                        has_getter: true,
                        has_setter: false,
                    },
                ),
                NodeData::SetAccessor(data) => (
                    data.name,
                    data.modifiers,
                    PrivateDeclarationKind::Accessor {
                        has_getter: false,
                        has_setter: true,
                    },
                ),
                _ => continue,
            };
            let Some(name) = name else {
                continue;
            };
            let name = self.node(name);
            let Some(private_name) = self.private_name_text(name) else {
                continue;
            };
            let is_static = self.has_modifier(modifiers, SyntaxKind::StaticKeyword)?;
            if self
                .generated_auto_accessor_backings
                .contains(&member.node())
            {
                auto_accessor_backings.push(PrivateDeclaration {
                    name: private_name.to_owned(),
                    is_static,
                    kind,
                });
                continue;
            }
            if let Some(existing) = declarations
                .iter_mut()
                .find(|declaration| declaration.name == private_name)
            {
                if let (
                    PrivateDeclarationKind::Accessor {
                        has_getter,
                        has_setter,
                    },
                    PrivateDeclarationKind::Accessor {
                        has_getter: next_getter,
                        has_setter: next_setter,
                    },
                ) = (&mut existing.kind, kind)
                {
                    *has_getter |= next_getter;
                    *has_setter |= next_setter;
                }
                continue;
            }
            declarations.push(PrivateDeclaration {
                name: private_name.to_owned(),
                is_static,
                kind,
            });
        }
        declarations.extend(auto_accessor_backings);

        let instance_brand = declarations
            .iter()
            .any(|declaration| {
                !declaration.is_static && !matches!(declaration.kind, PrivateDeclarationKind::Field)
            })
            .then(|| {
                self.allocate_hoisted_name(self.private_generated_name(class_name, "instances"))
            });
        let needs_class_alias = declarations.iter().any(|declaration| declaration.is_static)
            || static_facts.contains_this
            || static_facts.contains_super;
        let class_alias = needs_class_alias.then(|| self.allocate_temp_name());
        let super_alias = static_facts
            .contains_super
            .then(|| self.allocate_temp_name());
        let mut environment = PrivateEnvironment {
            slots: BTreeMap::new(),
            class_alias: class_alias.clone(),
            instance_brand: instance_brand.clone(),
            super_alias,
        };
        for declaration in declarations {
            let base_name = self.private_generated_name(class_name, &declaration.name);
            let element = match declaration.kind {
                PrivateDeclarationKind::Field => PrivateElement::Field {
                    value_name: self.allocate_hoisted_name(base_name),
                },
                PrivateDeclarationKind::Method => PrivateElement::Method {
                    method_name: self.allocate_hoisted_name(base_name),
                },
                PrivateDeclarationKind::Accessor {
                    has_getter,
                    has_setter,
                } => PrivateElement::Accessor {
                    getter_name: has_getter
                        .then(|| self.allocate_hoisted_name(format!("{base_name}_get"))),
                    setter_name: has_setter
                        .then(|| self.allocate_hoisted_name(format!("{base_name}_set"))),
                },
            };
            let placement = if declaration.is_static {
                PrivatePlacement::Static {
                    class_alias: class_alias
                        .clone()
                        .expect("static private slots own a class alias"),
                }
            } else {
                let brand_name = match &element {
                    PrivateElement::Field { value_name } => value_name.clone(),
                    PrivateElement::Method { .. } | PrivateElement::Accessor { .. } => {
                        instance_brand
                            .clone()
                            .expect("instance private behavior owns a WeakSet brand")
                    }
                };
                PrivatePlacement::Instance { brand_name }
            };
            environment
                .slots
                .insert(declaration.name, PrivateSlot { placement, element });
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
            let candidate = match &record.data {
                NodeData::PropertyDeclaration(data)
                    if self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)? =>
                {
                    data.initializer.map(|initializer| self.node(initializer))
                }
                NodeData::ClassStaticBlockDeclaration(data) => {
                    data.body.map(|body| self.node(body))
                }
                _ => None,
            };
            let Some(candidate) = candidate else {
                continue;
            };
            let candidate = self.static_lexical_facts_in(candidate)?;
            facts.contains_this |= candidate.contains_this;
            facts.contains_super |= candidate.contains_super;
        }
        Ok(facts)
    }

    fn static_lexical_facts_in(
        &self,
        root: TransformNode,
    ) -> Result<StaticLexicalFacts, TransformError> {
        let mut facts = StaticLexicalFacts::default();
        let mut stack = vec![root.node()];
        while let Some(id) = stack.pop() {
            let node = self
                .context
                .arena()
                .node_ref(self.source, id)
                .ok_or_else(|| TransformError::UnknownNode(self.node(id)))?;
            let record = self.context.arena().node(node)?;
            match record.kind {
                SyntaxKind::ThisKeyword => facts.contains_this = true,
                SyntaxKind::SuperKeyword => facts.contains_super = true,
                SyntaxKind::ClassDeclaration
                | SyntaxKind::ClassExpression
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
                | SyntaxKind::Constructor
                    if node != root =>
                {
                    continue;
                }
                _ => {}
            }
            if facts.contains_this && facts.contains_super {
                break;
            }
            let mut children = Vec::new();
            let syntax = self.context.arena().source(self.source)?.syntax();
            for_each_child(&syntax.arena, record, |child| {
                children.push(child);
                false
            });
            stack.extend(children.into_iter().rev());
        }
        Ok(facts)
    }

    fn static_bindings(&self) -> Option<StaticBindings> {
        let environment = self.private_environments.last()?;
        Some(StaticBindings {
            class_alias: environment.class_alias.clone()?,
            super_alias: environment.super_alias.clone(),
        })
    }

    fn private_generated_name(&self, class_name: Option<&str>, suffix: &str) -> String {
        match class_name {
            Some(class_name) if !class_name.is_empty() => format!("_{class_name}_{suffix}"),
            _ => format!("_{suffix}"),
        }
    }

    fn private_slot(&self, name: TransformNode) -> Result<&PrivateSlot, TransformError> {
        let private_name =
            self.private_name_text(name)
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PrivateIdentifier,
                    field: "private identifier text",
                })?;
        self.private_environments
            .iter()
            .rev()
            .find_map(|environment| environment.slots.get(private_name))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PrivateIdentifier,
                field: "declared private slot",
            })
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
        data: tsc_syntax::nodes::PropertyAccessExpressionData,
    ) -> Result<NodeId, TransformError> {
        if self.property_receiver_is_super(data.expression)? {
            if let Some(bindings) = self.active_static_bindings.clone() {
                let super_alias =
                    bindings
                        .super_alias
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::PropertyAccessExpression,
                            field: "captured super base",
                        })?;
                let name = data.name.map(|name| self.node(name)).ok_or(
                    TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PropertyAccessExpression,
                        field: "name",
                    },
                )?;
                let identifier = match &self.context.arena().node(name)?.data {
                    NodeData::Identifier(data) => Some(data.text.clone()),
                    _ => None,
                };
                let key = match identifier {
                    Some(identifier) => self.create_string_literal(&identifier)?,
                    _ => self.context.factory()?.clone_node(name)?,
                };
                let access = self.create_reflect_get(&super_alias, key, &bindings.class_alias)?;
                self.set_original_and_range(access, original)?;
                return Ok(access.node());
            }
        }
        let Some(name) = data.name else {
            return self.update_generic(original, NodeData::PropertyAccessExpression(data));
        };
        let name = self.node(name);
        if self.private_name_text(name).is_none() {
            return self.update_generic(original, NodeData::PropertyAccessExpression(data));
        }
        let slot = self.private_slot(name)?.clone();
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
        data: tsc_syntax::nodes::ElementAccessExpressionData,
    ) -> Result<NodeId, TransformError> {
        if self.property_receiver_is_super(data.expression)? {
            if let Some(bindings) = self.active_static_bindings.clone() {
                let super_alias =
                    bindings
                        .super_alias
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::ElementAccessExpression,
                            field: "captured super base",
                        })?;
                let key = self.visit_required(
                    data.argument_expression,
                    SyntaxKind::ElementAccessExpression,
                    "argument_expression",
                )?;
                let access = self.create_reflect_get(&super_alias, key, &bindings.class_alias)?;
                self.set_original_and_range(access, original)?;
                return Ok(access.node());
            }
        }
        self.update_generic(original, NodeData::ElementAccessExpression(data))
    }

    fn visit_binary_expression(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::BinaryExpressionData,
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

        if operator == Some(SyntaxKind::InKeyword) {
            if let Some(left) = data.left {
                let left = self.node(left);
                if self.private_name_text(left).is_some() {
                    let slot = self.private_slot(left)?.clone();
                    let receiver =
                        self.visit_required(data.right, SyntaxKind::BinaryExpression, "right")?;
                    let expression = self.create_private_in(&slot, receiver)?;
                    self.set_original_and_range(expression, original)?;
                    return Ok(expression.node());
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
        let Some((receiver, slot)) = self.private_assignment_target(data.left)? else {
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
            let stabilized = self.stabilize_receiver(receiver)?;
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

    fn visit_call_expression(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::CallExpressionData,
    ) -> Result<NodeId, TransformError> {
        let Some(expression) = data.expression else {
            return self.update_generic(original, NodeData::CallExpression(data));
        };
        let expression_node = self.node(expression);
        if self.is_super_access(expression_node)? {
            if let Some(bindings) = self.active_static_bindings.clone() {
                let target = self
                    .visit(expression)?
                    .map(|target| self.node(target))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::CallExpression,
                        field: "expression",
                    })?;
                let call = self.create_property_access(target, "call")?;
                let mut arguments = vec![self.create_identifier(&bindings.class_alias)?];
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
        }
        let NodeData::PropertyAccessExpression(access) =
            self.context.arena().node(expression_node)?.data.clone()
        else {
            return self.update_generic(original, NodeData::CallExpression(data));
        };
        let Some(name) = access.name else {
            return self.update_generic(original, NodeData::CallExpression(data));
        };
        let name = self.node(name);
        if self.private_name_text(name).is_none() {
            return self.update_generic(original, NodeData::CallExpression(data));
        }
        let slot = self.private_slot(name)?.clone();
        let receiver = self.visit_required(
            access.expression,
            SyntaxKind::PropertyAccessExpression,
            "expression",
        )?;
        let stabilized = self.stabilize_receiver(receiver)?;
        let target_receiver = stabilized.initialized.unwrap_or(stabilized.read);
        let target = self.create_private_get(target_receiver, &slot)?;
        let call = self.create_property_access(target, "call")?;
        let mut arguments = vec![stabilized.read];
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
        let call =
            self.context
                .factory()?
                .update_node(original, NodeData::CallExpression(data), flags)?;
        Ok(call.node())
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

    fn is_super_access(&self, expression: TransformNode) -> Result<bool, TransformError> {
        Ok(match &self.context.arena().node(expression)?.data {
            NodeData::PropertyAccessExpression(data) => {
                self.property_receiver_is_super(data.expression)?
            }
            NodeData::ElementAccessExpression(data) => {
                self.property_receiver_is_super(data.expression)?
            }
            _ => false,
        })
    }

    fn private_assignment_target(
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
        Ok(Some((receiver, self.private_slot(name)?.clone())))
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
        if let Some(instance_brand) = self
            .private_environments
            .last()
            .and_then(|environment| environment.instance_brand.clone())
        {
            operations
                .instance
                .push(InstanceOperation::PrivateBrand(instance_brand.clone()));
            operations.setup.instance_brand = Some(instance_brand);
        }
        for member in self.array_nodes(members)? {
            let record = self.context.arena().node(member)?.clone();
            match record.data {
                NodeData::PropertyDeclaration(mut data)
                    if self.name_is_private(data.name)?
                        && !self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)? =>
                {
                    let private_name = data.name.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PropertyDeclaration,
                        field: "private name",
                    })?;
                    let slot = self.private_slot(self.node(private_name))?.clone();
                    // Instance initializers execute in the constructor. Their
                    // nested generated names must therefore be allocated in
                    // the constructor scope when the operation is
                    // materialized, not while the class-level plan is built.
                    if slot.is_static() {
                        data.initializer = self.visit_optional_static_node(data.initializer)?;
                    }
                    let operation = PrivateFieldOperation {
                        original: member,
                        slot: slot.clone(),
                        initializer: data.initializer,
                    };
                    if slot.is_static() {
                        operations
                            .static_
                            .push(StaticOperation::PrivateField(operation));
                    } else {
                        operations
                            .instance
                            .push(InstanceOperation::PrivateField(operation));
                        if self
                            .generated_auto_accessor_backings
                            .contains(&member.node())
                        {
                            operations.setup.auto_accessor_storages.push(slot);
                        } else {
                            operations.setup.field_storages.push(slot);
                        }
                    }
                }
                NodeData::MethodDeclaration(data) if self.name_is_private(data.name)? => {
                    let name = data.name.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::MethodDeclaration,
                        field: "private name",
                    })?;
                    let slot = self.private_slot(self.node(name))?.clone();
                    let PrivateElement::Method { method_name } = &slot.element else {
                        return Err(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::MethodDeclaration,
                            field: "private method slot",
                        });
                    };
                    let function =
                        self.create_private_method_function(member, data, method_name)?;
                    operations.setup.definitions.push(PrivateDefinition {
                        original: member,
                        name: method_name.clone(),
                        function,
                    });
                }
                NodeData::GetAccessor(data) if self.name_is_private(data.name)? => {
                    let name = data.name.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::GetAccessor,
                        field: "private name",
                    })?;
                    let slot = self.private_slot(self.node(name))?.clone();
                    let PrivateElement::Accessor { getter_name, .. } = &slot.element else {
                        return Err(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::GetAccessor,
                            field: "private accessor slot",
                        });
                    };
                    let function_name =
                        getter_name
                            .as_deref()
                            .ok_or(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::GetAccessor,
                                field: "private getter binding",
                            })?;
                    let function = if self
                        .generated_static_auto_accessors
                        .contains(&member.node())
                    {
                        self.with_static_bindings(|visitor| {
                            visitor.create_private_getter_function(member, data, function_name)
                        })?
                    } else {
                        self.create_private_getter_function(member, data, function_name)?
                    };
                    operations.setup.definitions.push(PrivateDefinition {
                        original: member,
                        name: function_name.to_owned(),
                        function,
                    });
                }
                NodeData::SetAccessor(data) if self.name_is_private(data.name)? => {
                    let name = data.name.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::SetAccessor,
                        field: "private name",
                    })?;
                    let slot = self.private_slot(self.node(name))?.clone();
                    let PrivateElement::Accessor { setter_name, .. } = &slot.element else {
                        return Err(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::SetAccessor,
                            field: "private accessor slot",
                        });
                    };
                    let function_name =
                        setter_name
                            .as_deref()
                            .ok_or(TransformError::RequiredChildRemoved {
                                parent: SyntaxKind::SetAccessor,
                                field: "private setter binding",
                            })?;
                    let function = if self
                        .generated_static_auto_accessors
                        .contains(&member.node())
                    {
                        self.with_static_bindings(|visitor| {
                            visitor.create_private_setter_function(member, data, function_name)
                        })?
                    } else {
                        self.create_private_setter_function(member, data, function_name)?
                    };
                    operations.setup.definitions.push(PrivateDefinition {
                        original: member,
                        name: function_name.to_owned(),
                        function,
                    });
                }
                NodeData::PropertyDeclaration(mut data)
                    if !self.name_is_private(data.name)?
                        && !self.has_modifier(data.modifiers, SyntaxKind::AccessorKeyword)? =>
                {
                    let receiver =
                        if self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)? {
                            FieldReceiver::Static
                        } else {
                            FieldReceiver::Instance
                        };
                    let should_capture_key =
                        data.initializer.is_some() || self.mode == PublicFieldMode::DefineProperty;
                    let planned_name = self.plan_public_field_name(
                        data.name.ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::PropertyDeclaration,
                            field: "name",
                        })?,
                        should_capture_key,
                    )?;
                    data.name = Some(planned_name.name);
                    if let Some(evaluation) = planned_name.evaluation {
                        operations.key_evaluations.push(evaluation);
                    }
                    if receiver == FieldReceiver::Static {
                        data.initializer = self.visit_optional_static_node(data.initializer)?;
                    }
                    if data.initializer.is_none()
                        && self.mode == PublicFieldMode::DefineProperty
                        && receiver == FieldReceiver::Instance
                    {
                        if let Some(local_name) = self.parameter_property_local_name(member)? {
                            data.initializer = Some(self.create_identifier(&local_name)?.node());
                        }
                    }
                    let name = data.name.ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::PropertyDeclaration,
                        field: "name",
                    })?;
                    let operation = FieldOperation {
                        original: member,
                        receiver,
                        name,
                        initializer: data.initializer,
                    };
                    match receiver {
                        FieldReceiver::Instance => {
                            if self.mode == PublicFieldMode::DefineProperty
                                || operation.initializer.is_some()
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
                            if operation.initializer.is_some() {
                                operations.static_.push(StaticOperation::Field(operation));
                            }
                        }
                    }
                }
                NodeData::ClassStaticBlockDeclaration(data) => {
                    let body = data
                        .body
                        .and_then(|body| self.context.arena().node_ref(self.source, body))
                        .ok_or(TransformError::RequiredChildRemoved {
                            parent: SyntaxKind::ClassStaticBlockDeclaration,
                            field: "body",
                        })?;
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
                    let updated = if self
                        .generated_static_auto_accessors
                        .contains(&member.node())
                    {
                        self.with_static_bindings(|visitor| visitor.update_generic(member, data))?
                    } else {
                        self.visit(member.node())?
                            .ok_or(TransformError::RequiredChildRemoved {
                                parent: self.context.arena().node(member)?.kind,
                                field: "retained class member",
                            })?
                    };
                    operations.retained_members.push(self.node(updated));
                }
            }
        }
        Ok(operations)
    }

    fn create_private_method_function(
        &mut self,
        original: TransformNode,
        data: tsc_syntax::nodes::MethodDeclarationData,
        function_name: &str,
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
        function_name: &str,
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
        function_name: &str,
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
        function_name: &str,
        type_parameters: Option<NodeArrayId>,
        parameters: Option<NodeArrayId>,
        r#type: Option<NodeId>,
        asterisk_token: Option<NodeId>,
        body: Option<NodeId>,
        modifiers: Option<NodeArrayId>,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(function_name)?;
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
        self.set_original_and_range(function, original)
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

    fn install_instance_operations(
        &mut self,
        members: &mut Vec<TransformNode>,
        operations: &[InstanceOperation],
        derived: bool,
        class_name: Option<&str>,
    ) -> Result<(), TransformError> {
        let (statements, bindings) =
            self.with_new_generated_scope(GeneratedBindingOwner::FunctionBody, |visitor| {
                let mut statements = Vec::with_capacity(operations.len());
                for operation in operations {
                    statements.push(match operation {
                        InstanceOperation::PrivateBrand(brand) => {
                            visitor.materialize_private_brand(brand)?
                        }
                        InstanceOperation::Public(operation) => {
                            let mut operation = operation.clone();
                            operation.initializer =
                                visitor.visit_optional_node(operation.initializer)?;
                            visitor.materialize_field_operation(&operation, class_name)?
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
            let constructor = self.create_synthetic_constructor(derived, statements)?;
            members.insert(0, constructor);
            constructor
        };
        let constructor = self.install_function_bindings(constructor, bindings)?;
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

    fn materialize_static_operations(
        &mut self,
        class_name: &str,
        setup: ClassSetup,
        operations: Vec<StaticOperation>,
        private_environment: &PrivateEnvironment,
        assign_private_alias: bool,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let mut statements = Vec::with_capacity(operations.len() + 1);
        let mut setup_expressions = Vec::new();
        if assign_private_alias {
            if let Some(alias) = &private_environment.class_alias {
                let alias = self.create_identifier(alias)?;
                let class = self.create_identifier(class_name)?;
                setup_expressions.push(self.create_assignment(alias, class)?);
            }
        }
        for slot in setup.field_storages {
            setup_expressions.push(self.materialize_private_storage(&slot)?);
        }
        if let Some(brand) = setup.instance_brand {
            let brand = self.create_identifier(&brand)?;
            let weak_set = self.create_identifier("WeakSet")?;
            let weak_set = self.create_new(weak_set, Vec::new())?;
            setup_expressions.push(self.create_assignment(brand, weak_set)?);
        }
        for slot in setup.auto_accessor_storages {
            setup_expressions.push(self.materialize_private_storage(&slot)?);
        }
        for definition in setup.definitions {
            let name = self.create_identifier(&definition.name)?;
            let assignment = self.create_assignment(name, definition.function)?;
            self.set_original_and_range(assignment, definition.original)?;
            setup_expressions.push(assignment);
        }
        if !setup_expressions.is_empty() {
            if assign_private_alias {
                let setup = self.inline_expressions(setup_expressions)?;
                statements.push(self.create_expression_statement(setup)?);
            } else {
                for setup in setup_expressions {
                    statements.push(self.create_expression_statement(setup)?);
                }
            }
        }
        for operation in operations {
            let statement = match operation {
                StaticOperation::Field(operation) => {
                    self.materialize_field_operation(&operation, Some(class_name))?
                }
                StaticOperation::PrivateField(operation) => {
                    self.materialize_private_static_field(&operation)?
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

    fn materialize_class_key_evaluations(
        &mut self,
        evaluations: Vec<TransformNode>,
    ) -> Result<Option<TransformNode>, TransformError> {
        if evaluations.is_empty() {
            return Ok(None);
        }
        let evaluations = self.inline_expressions(evaluations)?;
        self.create_expression_statement(evaluations).map(Some)
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
            let name = self.visit_required(Some(name), SyntaxKind::PropertyDeclaration, "name")?;
            return Ok(PlannedPropertyName {
                name: name.node(),
                evaluation: None,
            });
        };
        let expression = self.visit_required(
            data.expression,
            SyntaxKind::ComputedPropertyName,
            "expression",
        )?;

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
            data.expression = Some(expression.node());
            let name = self.update_computed_property_name(original, data)?;
            return Ok(PlannedPropertyName {
                name,
                evaluation: None,
            });
        }

        let inner = self.skip_partially_emitted_expressions(expression)?;
        let inlineable = self.is_simple_inlineable_expression(inner)?;
        let identifier = self.context.arena().node(inner)?.kind == SyntaxKind::Identifier;
        let (key_expression, evaluation) = if should_capture && !inlineable {
            let temporary_name = self.allocate_temp_name();
            let target = self.create_identifier(&temporary_name)?;
            let evaluation = self.create_assignment(target, expression)?;
            let read = self.create_identifier(&temporary_name)?;
            (read, Some(evaluation))
        } else {
            let evaluation = (!inlineable && !identifier)
                .then(|| self.context.factory()?.clone_node(expression))
                .transpose()?;
            (expression, evaluation)
        };
        data.expression = Some(key_expression.node());
        let name = self.update_computed_property_name(original, data)?;
        Ok(PlannedPropertyName { name, evaluation })
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

    fn materialize_field_operation(
        &mut self,
        operation: &FieldOperation,
        class_name: Option<&str>,
    ) -> Result<TransformNode, TransformError> {
        let receiver = match operation.receiver {
            FieldReceiver::Instance => self.context.factory()?.create_token(
                self.source,
                SyntaxKind::ThisKeyword,
                TransformFlags::CONTAINS_LEXICAL_THIS,
            )?,
            FieldReceiver::Static => {
                self.create_identifier(class_name.ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::ClassDeclaration,
                    field: "static class binding",
                })?)?
            }
        };
        let initializer = operation
            .initializer
            .map(|initializer| self.node(initializer))
            .unwrap_or(self.create_void_zero()?);
        let expression = match self.mode {
            PublicFieldMode::Assignment => {
                let target = self.create_member_access(receiver, operation.name)?;
                self.create_assignment(target, initializer)?
            }
            PublicFieldMode::DefineProperty => {
                self.create_define_property(receiver, operation.name, initializer)?
            }
        };
        let statement = self.create_expression_statement(expression)?;
        self.set_original_and_range(statement, operation.original)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .set_starts_on_new_line(true);
        Ok(statement)
    }

    fn materialize_private_brand(
        &mut self,
        brand_name: &str,
    ) -> Result<TransformNode, TransformError> {
        let brand = self.create_identifier(brand_name)?;
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
        let storage_name =
            operation
                .slot
                .field_value_name()
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::PropertyDeclaration,
                    field: "private field storage",
                })?;
        let storage = self.create_identifier(storage_name)?;
        let set = self.create_property_access(storage, "set")?;
        let receiver = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::ThisKeyword,
            TransformFlags::CONTAINS_LEXICAL_THIS,
        )?;
        let initializer = operation
            .initializer
            .map(|initializer| self.node(initializer))
            .unwrap_or(self.create_void_zero()?);
        let call = self.create_call(set, vec![receiver, initializer])?;
        let statement = self.create_expression_statement(call)?;
        self.set_original_and_range(statement, operation.original)?;
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
        let storage = self.create_identifier(storage_name)?;
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
        let storage = self.create_identifier(storage_name)?;
        let initializer = operation
            .initializer
            .map(|initializer| self.node(initializer))
            .unwrap_or(self.create_void_zero()?);
        let value = self.create_property_assignment("value", initializer)?;
        let descriptor = self.create_object_literal(vec![value], false)?;
        let assignment = self.create_assignment(storage, descriptor)?;
        let statement = self.create_expression_statement(assignment)?;
        self.set_original_and_range(statement, operation.original)?;
        Ok(statement)
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
            0,
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
        let helper = self.create_identifier("__classPrivateFieldGet")?;
        let brand = self.create_identifier(slot.brand_name())?;
        let kind = self.create_string_literal(slot.access_kind())?;
        let mut arguments = vec![receiver, brand, kind];
        if let Some(descriptor) = slot.getter_descriptor_name() {
            arguments.push(self.create_identifier(descriptor)?);
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
            0,
            Vec::new(),
        ))?;
        self.context
            .arena_mut()?
            .metadata_mut(receiver)
            .add_flags(EmitFlags::NO_LEADING_COMMENTS);
        let helper = self.create_identifier("__classPrivateFieldSet")?;
        let brand = self.create_identifier(slot.brand_name())?;
        let kind = self.create_string_literal(slot.access_kind())?;
        let mut arguments = vec![receiver, brand, value, kind];
        if let Some(descriptor) = slot.setter_descriptor_name() {
            arguments.push(self.create_identifier(descriptor)?);
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
            0,
            Vec::new(),
        ))?;
        let helper = self.create_identifier("__classPrivateFieldIn")?;
        let brand = self.create_identifier(slot.brand_name())?;
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
        let temporary = self.allocate_temp_name();
        let read = self.create_identifier(&temporary)?;
        let target = self.create_identifier(&temporary)?;
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
        match self.context.arena().node(name_node)?.data.clone() {
            NodeData::Identifier(_) | NodeData::PrivateIdentifier(_) => {
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
                )
            }
            NodeData::ComputedPropertyName(data) => self.context.factory()?.create_node(
                self.source,
                NodeData::ElementAccessExpression(tsc_syntax::nodes::ElementAccessExpressionData {
                    expression: Some(receiver.node()),
                    question_dot_token: None,
                    argument_expression: data.expression,
                }),
                TransformFlags::NONE,
            ),
            _ => self.context.factory()?.create_node(
                self.source,
                NodeData::ElementAccessExpression(tsc_syntax::nodes::ElementAccessExpressionData {
                    expression: Some(receiver.node()),
                    question_dot_token: None,
                    argument_expression: Some(name),
                }),
                TransformFlags::NONE,
            ),
        }
    }

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
        let NodeData::Block(mut block) = self.context.arena().node(body)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::Constructor,
                field: "body block",
            });
        };
        let mut statements = self.array_nodes(block.statements)?;
        let mut insertion = 0usize;
        while insertion < statements.len() && self.is_prologue_statement(statements[insertion])? {
            insertion += 1;
        }
        if let Some(super_index) = statements[insertion..]
            .iter()
            .position(|statement| self.statement_is_super_call(*statement).unwrap_or(false))
        {
            insertion += super_index + 1;
        }
        let parameter_start = insertion;
        while insertion < statements.len()
            && self.original_kind(statements[insertion]) == Some(SyntaxKind::Parameter)
        {
            insertion += 1;
        }
        let replaces_parameter_properties = initializers
            .iter()
            .any(|initializer| self.original_kind(*initializer) == Some(SyntaxKind::Parameter));
        if replaces_parameter_properties {
            statements.drain(parameter_start..insertion);
            insertion = parameter_start;
        }
        statements.splice(insertion..insertion, initializers.iter().copied());
        let array = self
            .context
            .factory()?
            .create_node_array(self.source, statements)?;
        block.statements = Some(array.array());
        let flags =
            flags_after_update(self.context.arena(), body, &NodeData::Block(block.clone()))?;
        let body = self
            .context
            .factory()?
            .update_node(body, NodeData::Block(block), flags)?;
        self.context.factory()?.set_multi_line(body, true)?;
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

    fn create_synthetic_constructor(
        &mut self,
        derived: bool,
        mut initializers: Vec<TransformNode>,
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
        let body = self.create_block(initializers, true)?;
        let parameters = self
            .context
            .factory()?
            .create_node_array(self.source, Vec::new())?;
        self.context.factory()?.create_node(
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
        )
    }

    fn prepend_generated_declarations_to_source(
        &mut self,
        root: TransformNode,
        bindings: GeneratedBindings,
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
        bindings: GeneratedBindings,
    ) -> Result<TransformNode, TransformError> {
        if bindings.is_empty() {
            return Ok(block);
        }
        let NodeData::Block(mut data) = self.context.arena().node(block)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: self.context.arena().node(block)?.kind,
                field: "block for generated bindings",
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
        let flags =
            flags_after_update(self.context.arena(), block, &NodeData::Block(data.clone()))?;
        self.context
            .factory()?
            .update_node(block, NodeData::Block(data), flags)
    }

    fn create_generated_variable_statement(
        &mut self,
        bindings: &GeneratedBindings,
    ) -> Result<TransformNode, TransformError> {
        let mut declarations = Vec::with_capacity(bindings.names().len());
        for name in bindings.names() {
            let name = self.create_identifier(name)?;
            declarations.push(self.create_variable_declaration(name, None)?);
        }
        let statement = self.create_variable_statement(declarations, NodeFlags::NONE)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::CUSTOM_PROLOGUE);
        Ok(statement)
    }

    fn allocate_temp_name(&mut self) -> String {
        self.generated_bindings.allocate_temp()
    }

    fn allocate_hoisted_name(&mut self, preferred: String) -> String {
        self.generated_bindings.allocate_preferred(preferred)
    }

    fn capture_super_base(
        &mut self,
        heritage: Option<NodeArrayId>,
        super_alias: Option<&str>,
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
            let alias = self.create_identifier(super_alias)?;
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

    fn has_extends_clause(&self, heritage: Option<NodeArrayId>) -> Result<bool, TransformError> {
        Ok(self.array_nodes(heritage)?.iter().any(|clause| {
            matches!(
                self.context.arena().node(*clause).ok().map(|node| &node.data),
                Some(NodeData::HeritageClause(data)) if data.token == SyntaxKind::ExtendsKeyword
            )
        }))
    }

    fn statement_is_super_call(&self, statement: TransformNode) -> Result<bool, TransformError> {
        let NodeData::ExpressionStatement(data) = &self.context.arena().node(statement)?.data
        else {
            return Ok(false);
        };
        let Some(expression) = data.expression else {
            return Ok(false);
        };
        let NodeData::CallExpression(data) =
            &self.context.arena().node(self.node(expression))?.data
        else {
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

    fn parameter_property_local_name(
        &self,
        property: TransformNode,
    ) -> Result<Option<String>, TransformError> {
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
        Ok(self.identifier_text(name).map(str::to_owned))
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

    fn create_reflect_get(
        &mut self,
        super_alias: &str,
        key: TransformNode,
        class_alias: &str,
    ) -> Result<TransformNode, TransformError> {
        let reflect = self.create_identifier("Reflect")?;
        let get = self.create_property_access(reflect, "get")?;
        let super_alias = self.create_identifier(super_alias)?;
        let class_alias = self.create_identifier(class_alias)?;
        self.create_call(get, vec![super_alias, key, class_alias])
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
        let previous = std::mem::replace(&mut self.active_static_bindings, bindings);
        let result = self
            .visit(node)
            .map(|node| node.map(|node| self.node(node)));
        self.active_static_bindings = previous;
        result
    }

    fn with_static_bindings<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, TransformError>,
    ) -> Result<T, TransformError> {
        let bindings = self.static_bindings();
        let previous = std::mem::replace(&mut self.active_static_bindings, bindings);
        let result = operation(self);
        self.active_static_bindings = previous;
        result
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

    const fn node(&self, id: NodeId) -> TransformNode {
        TransformNode::new(self.source, id)
    }

    const fn array(&self, id: NodeArrayId) -> TransformNodeArray {
        TransformNodeArray::new(self.source, id)
    }
}

impl NodeDataChildVisitor for DownlevelClassVisitor<'_, '_> {
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
        for node in nodes {
            if let Some(node) = self.visit(node)? {
                visited.push(self.node(node));
                if let Some(expanded) = self.expanded_statements.get(&node).cloned() {
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
