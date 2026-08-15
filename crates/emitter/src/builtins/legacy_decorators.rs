//! H2.4a legacy-decorator lowering.

use std::collections::{BTreeMap, BTreeSet};

use tsc_syntax::{
    try_visit_each_child, NodeArrayId, NodeData, NodeDataChildVisitor, NodeId, SyntaxKind,
};
use tsc_types::{CompilerOptions, NodeCheckFlags, NodeFlags, ScriptTarget};

use crate::{
    factory::EmitHelperName, metadata::ClassExpressionDeclarationOrigin, CommentRange, EmitFlags,
    EmitHelper, EmitResolver, EmitResolverError, EmitResolverNode,
    EmitTypeReferenceSerializationKind, InternalEmitFlags, SourceRange, TransformError,
    TransformFlags, TransformNode, TransformNodeArray, TransformRoot, TransformSourceId,
    TransformationContext, Transformer, UnsupportedEmitFeature,
};

use super::{
    flags_after_update,
    generated_bindings::{
        AncestorBindingPolicy, GeneratedBindingOwner, GeneratedBindingScopeId,
        GeneratedBindingScopes,
    },
    is_prologue_statement,
    system::collect_identifier_texts,
    target_bindings::TargetBinding,
};

const DECORATE_HELPER_TEXT: &str = r#"var __decorate = (this && this.__decorate) || function (decorators, target, key, desc) {
    var c = arguments.length, r = c < 3 ? target : desc === null ? desc = Object.getOwnPropertyDescriptor(target, key) : desc, d;
    if (typeof Reflect === "object" && typeof Reflect.decorate === "function") r = Reflect.decorate(decorators, target, key, desc);
    else for (var i = decorators.length - 1; i >= 0; i--) if (d = decorators[i]) r = (c < 3 ? d(r) : c > 3 ? d(target, key, r) : d(target, key)) || r;
    return c > 3 && r && Object.defineProperty(target, key, r), r;
};"#;

const METADATA_HELPER_TEXT: &str = r#"var __metadata = (this && this.__metadata) || function (k, v) {
    if (typeof Reflect === "object" && typeof Reflect.metadata === "function") return Reflect.metadata(k, v);
};"#;

const PARAM_HELPER_TEXT: &str = r#"var __param = (this && this.__param) || function (paramIndex, decorator) {
    return function (target, key) { decorator(target, key, paramIndex); }
};"#;

/// tsc-port: transformLegacyDecorators @6.0.3
/// tsc-hash: a189529b3222643cfd792a2698fc5adcc51c91ef8de4ce775a4b866bad3c839c
/// tsc-span: _tsc.js:98430-98943
pub(super) fn transform_legacy_decorators<'resolver>(
    options: &CompilerOptions,
    resolver: &'resolver dyn EmitResolver,
) -> Box<dyn Transformer + 'resolver> {
    Box::new(LegacyDecoratorTransformer {
        resolver,
        target: options.emit_script_target(),
        emit_decorator_metadata: options.emit_decorator_metadata == Some(true),
        strict_null_checks: options.strict_option_value(options.strict_null_checks),
    })
}

struct LegacyDecoratorTransformer<'resolver> {
    resolver: &'resolver dyn EmitResolver,
    target: ScriptTarget,
    emit_decorator_metadata: bool,
    strict_null_checks: bool,
}

impl Transformer for LegacyDecoratorTransformer<'_> {
    fn name(&self) -> &'static str {
        "transformLegacyDecorators"
    }

    fn transform_root(
        &mut self,
        context: &mut TransformationContext,
        root: TransformRoot,
    ) -> Result<TransformRoot, TransformError> {
        let TransformRoot::SourceFile(source) = root else {
            return Err(TransformError::Unsupported(
                UnsupportedEmitFeature::BundleRoot,
            ));
        };
        if context.arena().source(source)?.syntax().is_declaration_file {
            return Ok(root);
        }
        let current_root = context.arena().root(source)?;
        let mut visitor = LegacyDecoratorVisitor::new(
            context,
            source,
            self.resolver,
            self.target,
            self.emit_decorator_metadata,
            self.strict_null_checks,
        );
        let transformed =
            visitor
                .visit(current_root.node())?
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::SourceFile,
                    field: "root",
                })?;
        let transformed = visitor.prepend_hoisted_declarations(visitor.node(transformed))?;
        visitor
            .context
            .arena_mut()?
            .replace_root(source, transformed)?;
        Ok(TransformRoot::SourceFile(source))
    }
}

struct LegacyDecoratorVisitor<'context, 'resolver> {
    context: &'context mut TransformationContext,
    source: TransformSourceId,
    resolver: &'resolver dyn EmitResolver,
    target: ScriptTarget,
    emit_decorator_metadata: bool,
    strict_null_checks: bool,
    nodes: BTreeMap<NodeId, Option<NodeId>>,
    arrays: BTreeMap<NodeArrayId, Option<NodeArrayId>>,
    expanded_classes: BTreeMap<NodeId, Vec<NodeId>>,
    used_names: BTreeSet<String>,
    generated_bindings: GeneratedBindingScopes,
    lexical_binding_frames: Vec<LegacyLexicalBindingFrame>,
    generated_binding_scope_stack: Vec<(GeneratedBindingScopeId, GeneratedBindingScopeId)>,
    preentered_function_scopes: BTreeSet<NodeId>,
    class_aliases: BTreeMap<NodeId, TargetBinding>,
    computed_names: BTreeMap<NodeId, TargetBinding>,
}

/// The source interval that begins after a declaration's modifiers while
/// retaining the declaration end. It is a text-range location rather than
/// semantic or comment provenance.
#[derive(Clone, Copy, Debug)]
struct RangePastModifiers {
    source: TransformSourceId,
    range: SourceRange,
}

#[derive(Debug, Default)]
struct LegacyLexicalBindingFrame {
    class_aliases: Vec<TargetBinding>,
    producer_temps: Vec<TargetBinding>,
}

impl LegacyLexicalBindingFrame {
    fn is_empty(&self) -> bool {
        self.class_aliases.is_empty() && self.producer_temps.is_empty()
    }
}

/// The generated-name provenance determines both collision behavior and
/// whether the spelling remains reserved in nested function scopes.
///
/// - Metadata temporaries are ordinary lexical temps and may be shadowed.
/// - Decorated computed names are observed during class evaluation and keep
///   their spelling reserved in descendants.
/// - Class aliases use tsc's source-wide numbered identity while their `var`
///   declaration is owned by the active lexical frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LegacyGeneratedBindingKind {
    MetadataTemp,
    DecoratedComputedName,
    ClassAlias,
}

/// Typed equivalent of tsc's class `shouldAddParamTypesMetadata` result.
/// The plan cannot exist without the explicit constructor body that owns the
/// serialized parameter list, so an implicit or signature-only constructor
/// cannot accidentally request the metadata helper with an empty array.
#[derive(Clone, Copy, Debug)]
struct ConstructorMetadataPlan {
    original_constructor_with_body: TransformNode,
    serialization_context: MetadataSerializationContext,
}

impl ConstructorMetadataPlan {
    fn for_class(
        emit_decorator_metadata: bool,
        has_constructor_decoration: bool,
        constructor_with_body: Option<TransformNode>,
        serialization_context: MetadataSerializationContext,
    ) -> Option<Self> {
        if !emit_decorator_metadata || !has_constructor_decoration {
            return None;
        }
        Some(Self {
            original_constructor_with_body: constructor_with_body?,
            serialization_context,
        })
    }
}

#[derive(Clone, Copy, Debug)]
struct ConstructorHandoff {
    original_with_body: TransformNode,
    current_with_body: TransformNode,
}

/// The parse-tree name scope used by tsc's runtime type serializer. Keeping
/// this separate from the annotated declaration is significant for parameter
/// properties such as `constructor(Service: Service)`: value lookup for the
/// type name starts at the class, not at the shadowing parameter.
#[derive(Clone, Copy, Debug)]
struct MetadataSerializationContext {
    resolver_location: EmitResolverNode,
}

/// The source declaration that is allowed to materialize one legacy member
/// decoration. Signature-only methods are deliberately absent, while an
/// accessor pair has exactly one owner selected from its original nodes.
#[derive(Clone, Copy, Debug)]
enum DecoratorOwner {
    Property(TransformNode),
    MethodWithBody(TransformNode),
    AccessorWithBody(TransformNode),
}

impl DecoratorOwner {
    const fn member(self) -> TransformNode {
        match self {
            Self::Property(member)
            | Self::MethodWithBody(member)
            | Self::AccessorWithBody(member) => member,
        }
    }
}

/// The identity used by tsc when it groups a getter and setter into one
/// accessor declaration. Computed names are deliberately not compared by
/// syntax: only literal-like computed names have a stable property identity.
/// Every other computed expression is dynamic, even an identifier, and owns
/// an independent accessor decoration.
///
/// tsc-port: isDynamicName/getPropertyNameForPropertyNameNode/
/// getAllAccessorDeclarations @6.0.3
#[derive(Clone, Debug, Eq, PartialEq)]
enum AccessorPropertyNameIdentity {
    Static(String),
    Dynamic,
}

/// The accessor set observed while transformTypeScript injects decorator
/// metadata. Its names are parse-tree names: a signed numeric computed name is
/// still static at this point, before `visitPropertyNameOfClassElement` can
/// replace it with an assignment to a generated binding.
#[derive(Clone, Copy, Debug)]
struct MetadataAccessorGroup {
    first_accessor: TransformNode,
    second_accessor: Option<TransformNode>,
    get_accessor: Option<TransformNode>,
    set_accessor: Option<TransformNode>,
}

/// The accessor set observed later by transformLegacyDecorators. Its identity
/// projects transformTypeScript's computed-name rewrite, so a cached signed
/// numeric name is dynamic even though the corresponding metadata group was
/// static. Keeping this type distinct prevents the two phases from silently
/// sharing the wrong owner or setter parameter list.
#[derive(Clone, Copy, Debug)]
struct RuntimeAccessorGroup {
    first_accessor: TransformNode,
    second_accessor: Option<TransformNode>,
    get_accessor: Option<TransformNode>,
    set_accessor: Option<TransformNode>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PreparedDecoratorRuntimeRole {
    Direct {
        admits_owner: bool,
    },
    Parameter {
        runtime_index: usize,
        admits_owner: bool,
    },
    VisitationOnly,
}

#[derive(Clone, Copy, Debug)]
struct PreparedDecoratorExpression {
    visited: TransformNode,
    runtime_role: PreparedDecoratorRuntimeRole,
    contains_private_identifier_in_expression: bool,
}

impl PreparedDecoratorExpression {
    const fn admits_owner(self) -> bool {
        match self.runtime_role {
            PreparedDecoratorRuntimeRole::Direct { admits_owner }
            | PreparedDecoratorRuntimeRole::Parameter { admits_owner, .. } => admits_owner,
            PreparedDecoratorRuntimeRole::VisitationOnly => false,
        }
    }
}

/// Decorator expressions visited exactly once at their transformTypeScript
/// source position. The legacy helper consumes this handoff instead of
/// reopening modifiers on either the parse tree or the decorator-stripped
/// emitted member.
#[derive(Clone, Debug, Default)]
struct PreparedClassElementDecorators {
    member: Vec<PreparedDecoratorExpression>,
    parameters: Vec<PreparedDecoratorExpression>,
}

impl PreparedClassElementDecorators {
    fn admits_owner(&self) -> bool {
        self.member
            .iter()
            .chain(&self.parameters)
            .copied()
            .any(PreparedDecoratorExpression::admits_owner)
    }

    fn contains_private_identifier_in_expression(&self) -> bool {
        self.member
            .iter()
            .chain(&self.parameters)
            .any(|decorator| decorator.contains_private_identifier_in_expression)
    }
}

struct PreparedClassDecoration {
    direct: Vec<PreparedDecoratorExpression>,
    constructor_parameters: Vec<PreparedDecoratorExpression>,
    metadata: Option<TransformNode>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClassElementPreparationMode {
    Runtime,
    VisitationOnly,
}

struct PreparedParameterList {
    emitted: Option<NodeArrayId>,
    decorators: Vec<PreparedDecoratorExpression>,
}

/// One legacy decoration materialized after the class body has been visited.
/// Metadata is prepared while walking source members so serializer-generated
/// temporaries retain transformTypeScript's ordering relative to computed-name
/// temporaries. The later statement pass consumes these nodes without running
/// metadata serialization a second time.
struct PreparedMemberDecoration {
    owner: DecoratorOwner,
    current_member: TransformNode,
    decorators: PreparedClassElementDecorators,
    metadata: Vec<TransformNode>,
}

/// One class element after its transformTypeScript-compatible work has run.
///
/// `runtime_member` deliberately remains the decorated, pre-legacy node used
/// by `getAllAccessorDeclarations` and decorator aggregation. `emitted_member`
/// is the separately visited node whose decorators and type-only syntax have
/// been removed for the class body. Keeping both roles in one typed handoff
/// prevents the runtime grouping pass from accidentally observing the later
/// legacy-decorator rewrite.
#[derive(Clone, Debug)]
struct PreparedClassElement {
    runtime_member: TransformNode,
    emitted_member: Option<TransformNode>,
    decorators: PreparedClassElementDecorators,
}

/// The runtime accessor regrouping result. `parameter_member` is the rewritten
/// setter (or the method itself) whose already-visited parameter decorators
/// are appended after the direct member decorators.
#[derive(Clone, Copy, Debug)]
struct RuntimeDecoratorOwner {
    owner: DecoratorOwner,
    parameter_member: Option<TransformNode>,
    admits_owner: bool,
}

/// Source-ordered handoff between transformTypeScript's class-element work and
/// transformLegacyDecorators' trailing helper statements.
struct LegacyClassMemberPlan {
    emitted_members: Option<NodeArrayId>,
    decorations: Vec<PreparedMemberDecoration>,
    constructor_parameters: Vec<PreparedDecoratorExpression>,
    contains_private_decorator_expression: bool,
}

struct TransformedClassMembers {
    emitted_members: Option<NodeArrayId>,
    decoration_statements: Vec<TransformNode>,
    constructor_parameters: Vec<PreparedDecoratorExpression>,
    contains_private_decorator_expression: bool,
}

/// Runtime ownership of the expression used as a legacy decoration key.
///
/// A direct member decorator makes transformTypeScript cache a dynamic
/// computed name during class evaluation. A parameter-only decorator does not:
/// transformLegacyDecorators still asks the factory for a generated reference,
/// but tsc deliberately emits no declaration or class-key assignment for it.
/// Ambient/abstract properties keep using their source expression directly.
///
/// tsc-port: getExpressionForPropertyName(...,
/// !hasSyntacticModifier(member, ModifierFlags.Ambient)) @6.0.3
/// tsc-span: _tsc.js:98805-98809
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DecoratorComputedNamePlan {
    AmbientExpression,
    ClassEvaluationCacheCandidate,
    HelperReferenceOnly,
}

impl DecoratorComputedNamePlan {
    const fn caches_at_class_evaluation(self) -> bool {
        matches!(self, Self::ClassEvaluationCacheCandidate)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LegacyTempBindingStorage {
    Hoisted,
    ReferenceOnly,
}

impl<'context, 'resolver> LegacyDecoratorVisitor<'context, 'resolver> {
    fn new(
        context: &'context mut TransformationContext,
        source: TransformSourceId,
        resolver: &'resolver dyn EmitResolver,
        target: ScriptTarget,
        emit_decorator_metadata: bool,
        strict_null_checks: bool,
    ) -> Self {
        let used_names = collect_identifier_texts(context.arena(), source);
        let generated_bindings =
            GeneratedBindingScopes::new(used_names.clone(), AncestorBindingPolicy::AllowShadow);
        Self {
            context,
            source,
            resolver,
            target,
            emit_decorator_metadata,
            strict_null_checks,
            nodes: BTreeMap::new(),
            arrays: BTreeMap::new(),
            expanded_classes: BTreeMap::new(),
            used_names,
            generated_bindings,
            lexical_binding_frames: vec![LegacyLexicalBindingFrame::default()],
            generated_binding_scope_stack: Vec::new(),
            preentered_function_scopes: BTreeSet::new(),
            class_aliases: BTreeMap::new(),
            computed_names: BTreeMap::new(),
        }
    }

    fn visit(&mut self, id: NodeId) -> Result<Option<NodeId>, TransformError> {
        if let Some(mapped) = self.nodes.get(&id) {
            return Ok(*mapped);
        }
        let original = self.node(id);
        let record = self.context.arena().node(original)?.clone();
        let transformed = match record.data {
            NodeData::Decorator(_) => None,
            NodeData::Token => Some(original.node()),
            NodeData::Identifier(data) => {
                if let Some(alias) = self.class_alias_for_reference(original)? {
                    let replacement = self.create_generated_identifier(&alias)?;
                    self.set_original_and_range(replacement, original)?;
                    Some(replacement.node())
                } else {
                    Some(self.update_generic(original, NodeData::Identifier(data))?)
                }
            }
            NodeData::ClassExpression(mut data) => {
                // A recovery-tree decorator on a class expression is still a
                // transformTypeScript producer: its expression is visited even
                // though transformLegacyDecorators has no statement position
                // at which it can materialize a helper. Heritage and members
                // follow that modifier visit in source evaluation order.
                let _ = self.visit_decorator_expressions(
                    data.modifiers,
                    PreparedDecoratorRuntimeRole::VisitationOnly,
                )?;
                data.modifiers = self.strip_decorators(data.modifiers)?;
                if let Some(heritage) = data.heritage_clauses {
                    data.heritage_clauses = self.visit_nodes(heritage)?;
                }
                data.members = self.prepare_class_expression_members(data.members)?;
                let flags = flags_after_update(
                    self.context.arena(),
                    original,
                    &NodeData::ClassExpression(data.clone()),
                )?;
                Some(
                    self.context
                        .factory()?
                        .update_node(original, NodeData::ClassExpression(data), flags)?
                        .node(),
                )
            }
            NodeData::ClassStaticBlockDeclaration(data) => Some(self.update_class_static_block(
                original,
                tsc_syntax::nodes::ClassStaticBlockDeclarationData {
                    body: data.body,
                    modifiers: data.modifiers,
                },
            )?),
            NodeData::Constructor(mut data) => {
                data.modifiers = self.strip_decorators(data.modifiers)?;
                Some(self.update_function_like(original, NodeData::Constructor(data))?)
            }
            NodeData::MethodDeclaration(mut data) => {
                data.name = self.rewrite_computed_member_name(original, data.name)?;
                data.modifiers = self.strip_decorators(data.modifiers)?;
                Some(self.update_function_like(original, NodeData::MethodDeclaration(data))?)
            }
            NodeData::GetAccessor(mut data) => {
                data.name = self.rewrite_computed_member_name(original, data.name)?;
                data.modifiers = self.strip_decorators(data.modifiers)?;
                Some(self.update_function_like(original, NodeData::GetAccessor(data))?)
            }
            NodeData::SetAccessor(mut data) => {
                data.name = self.rewrite_computed_member_name(original, data.name)?;
                data.modifiers = self.strip_decorators(data.modifiers)?;
                Some(self.update_function_like(original, NodeData::SetAccessor(data))?)
            }
            NodeData::ArrowFunction(data) => {
                Some(self.update_function_like(original, NodeData::ArrowFunction(data))?)
            }
            NodeData::FunctionDeclaration(data) => {
                Some(self.update_function_like(original, NodeData::FunctionDeclaration(data))?)
            }
            NodeData::FunctionExpression(data) => {
                Some(self.update_function_like(original, NodeData::FunctionExpression(data))?)
            }
            NodeData::PropertyDeclaration(mut data) => {
                if NodeFlags::from_bits(self.context.arena().node(original)?.flags)
                    .contains(NodeFlags::AMBIENT)
                    || self.has_modifier(data.modifiers, SyntaxKind::DeclareKeyword)?
                {
                    None
                } else {
                    data.name = self.rewrite_computed_member_name(original, data.name)?;
                    data.modifiers = self.strip_decorators(data.modifiers)?;
                    Some(self.update_generic(original, NodeData::PropertyDeclaration(data))?)
                }
            }
            NodeData::Parameter(mut data) => {
                data.modifiers = self.strip_decorators(data.modifiers)?;
                Some(self.update_generic(original, NodeData::Parameter(data))?)
            }
            data => Some(self.update_generic(original, data)?),
        };
        self.nodes.insert(id, transformed);
        Ok(transformed)
    }

    fn visit_class_declaration(
        &mut self,
        id: NodeId,
    ) -> Result<Vec<TransformNode>, TransformError> {
        if let Some(expanded) = self.expanded_classes.get(&id) {
            return Ok(expanded.iter().copied().map(|id| self.node(id)).collect());
        }
        let current = self.node(id);
        let record = self.context.arena().node(current)?.clone();
        let NodeData::ClassDeclaration(mut data) = record.data else {
            return Err(TransformError::RequiredChildRemoved {
                parent: record.kind,
                field: "class declaration",
            });
        };
        let is_export = self.has_modifier(data.modifiers, SyntaxKind::ExportKeyword)?;
        let is_default = self.has_modifier(data.modifiers, SyntaxKind::DefaultKeyword)?;
        let original_class = self.context.arena().get_original_node(current);
        let serialization_context = MetadataSerializationContext {
            resolver_location: self.resolver_node(original_class)?,
        };
        let original_members = match &self.context.arena().node(original_class)?.data {
            NodeData::ClassDeclaration(data) => data.members,
            _ => data.members,
        };
        let constructor_handoff = self.constructor_handoff(data.members, original_members)?;
        let class_has_direct_decorator = !self.decorator_expressions(data.modifiers)?.is_empty();
        let constructor_has_parameter_decorator = constructor_handoff
            .map(|handoff| {
                self.constructor_parameter_list_is_legacy_decorated(handoff.original_with_body)
            })
            .transpose()?
            .unwrap_or(false);
        let has_constructor_decoration =
            class_has_direct_decorator || constructor_has_parameter_decorator;
        let has_member_decoration = self.class_has_member_decoration(original_members)?;
        let has_member_decorator_syntax = self.class_has_legacy_decorator_syntax(data.members)?;
        if !has_constructor_decoration && !has_member_decoration && !has_member_decorator_syntax {
            let updated = self.update_generic(current, NodeData::ClassDeclaration(data))?;
            self.nodes.insert(id, Some(updated));
            self.expanded_classes.insert(id, vec![updated]);
            return Ok(vec![self.node(updated)]);
        }
        let explicit_name = data.name;
        // transformTypeScript may have assigned `default_N` to an originally
        // anonymous declaration so later passes can publish its statements.
        // That generated local is not a source class-expression name: legacy
        // decoration must retain anonymous named-evaluation semantics and
        // ultimately call `__setFunctionName(..., "default")`.
        let expression_name = match &self.context.arena().node(original_class)?.data {
            NodeData::ClassDeclaration(data) if data.name.is_none() => None,
            _ => explicit_name,
        };
        let name = if let Some(name) = explicit_name {
            name
        } else if is_default {
            // Error-recovery syntax can contain `default class {}` without an
            // `export` modifier. `getLocalName` still gives that decorated
            // declaration a stable `default_N` binding so emit can continue;
            // publication remains independently guarded by `is_export`.
            let generated = self.allocate_generated_class_name("default");
            self.create_identifier(&generated)?.node()
        } else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ClassDeclaration,
                field: "name",
            });
        };
        let name_text = self.identifier_text(name)?.to_owned();

        // transformTypeScript's class transaction is intentionally not the
        // runtime helper order: modifiers are producer-visited first, class
        // metadata allocates its temporaries next, then heritage and members.
        let direct = self.visit_decorator_expressions(
            data.modifiers,
            PreparedDecoratorRuntimeRole::Direct {
                admits_owner: class_has_direct_decorator,
            },
        )?;
        let constructor_metadata = ConstructorMetadataPlan::for_class(
            self.emit_decorator_metadata,
            has_constructor_decoration,
            constructor_handoff.map(|handoff| handoff.original_with_body),
            serialization_context,
        );
        let metadata = constructor_metadata
            .map(|plan| self.create_constructor_parameter_metadata(plan))
            .transpose()?;
        if let Some(heritage) = data.heritage_clauses {
            data.heritage_clauses = self.visit_nodes(heritage)?;
        }
        let class_alias = if has_constructor_decoration
            && self.resolver.has_node_check_flag(
                self.resolver_node(original_class)?,
                NodeCheckFlags::CONTAINS_CONSTRUCTOR_REFERENCE.bits() as u32,
            )? {
            let alias = self.allocate_class_alias(&name_text)?;
            self.class_aliases
                .insert(original_class.node(), alias.clone());
            Some(alias)
        } else {
            None
        };

        let TransformedClassMembers {
            mut emitted_members,
            mut decoration_statements,
            constructor_parameters,
            contains_private_decorator_expression,
        } = self.transform_class_members(
            current,
            data.members,
            &name_text,
            serialization_context,
            constructor_handoff,
        )?;
        let class_decoration = PreparedClassDecoration {
            direct,
            constructor_parameters,
            metadata,
        };
        let materializes_member_decoration = !decoration_statements.is_empty();
        if contains_private_decorator_expression && !decoration_statements.is_empty() {
            let block = self.create_block(decoration_statements, true)?;
            let static_block = self.context.factory()?.create_node(
                self.source,
                NodeData::ClassStaticBlockDeclaration(
                    tsc_syntax::nodes::ClassStaticBlockDeclarationData {
                        body: Some(block.node()),
                        modifiers: None,
                    },
                ),
                TransformFlags::NONE,
            )?;
            let mut member_nodes = self.array_nodes(emitted_members)?;
            member_nodes.push(static_block);
            emitted_members = Some(
                self.context
                    .factory()?
                    .create_node_array(self.source, member_nodes)?
                    .array(),
            );
            decoration_statements = Vec::new();
        }
        data.members = emitted_members;

        let mut statements = if has_constructor_decoration {
            self.transform_decorated_class_declaration(
                current,
                data,
                name,
                expression_name,
                class_alias.as_ref(),
            )?
        } else {
            data.name = if materializes_member_decoration {
                Some(name)
            } else {
                expression_name
            };
            data.modifiers = self.strip_decorators(data.modifiers)?;
            let transform_flags = self.context.arena().transform_flags(current);
            let updated = self.context.factory()?.update_node(
                current,
                NodeData::ClassDeclaration(data),
                transform_flags,
            )?;
            vec![updated]
        };
        statements.append(&mut decoration_statements);

        if has_constructor_decoration {
            let decorators = self.materialize_class_decorators(class_decoration)?;
            let class_name = self.create_identifier(&name_text)?;
            let mut decorate = self.create_decorate_call(decorators, class_name, None, None)?;
            if let Some(alias) = class_alias.as_ref() {
                let alias = self.create_generated_identifier(alias)?;
                decorate = self.create_assignment(alias, decorate)?;
            }
            let class_name = self.create_identifier(&name_text)?;
            if let Some(explicit_name) = explicit_name {
                self.set_original_and_range(class_name, self.node(explicit_name))?;
            }
            // This is tsc's `getDeclarationName`, not `getLocalName`: the
            // module transformer must still relate the assignment target to
            // the original exported class and wrap it in `exports.name =`.
            // The variable declaration above owns the local binding.
            let assignment = self.create_assignment(class_name, decorate)?;
            let statement = self.create_expression_statement(assignment)?;
            self.context
                .arena_mut()?
                .metadata_mut(statement)
                .add_flags(EmitFlags::NO_COMMENTS);
            statements.push(statement);
        }
        if has_constructor_decoration && is_export {
            statements.push(if is_default {
                self.create_export_default(&name_text)?
            } else {
                let declaration_name = explicit_name.map(|name| self.node(name)).ok_or(
                    TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ClassDeclaration,
                        field: "name",
                    },
                )?;
                self.create_named_export(&name_text, declaration_name)?
            });
        }

        self.nodes.insert(id, None);
        self.expanded_classes.insert(
            id,
            statements
                .iter()
                .map(|statement| statement.node())
                .collect(),
        );
        Ok(statements)
    }

    fn transform_decorated_class_declaration(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ClassDeclarationData,
        name: NodeId,
        expression_name: Option<NodeId>,
        class_alias: Option<&TargetBinding>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let location = self.move_range_past_modifiers(original, data.modifiers)?;
        let declaration_comment_range = self.raw_comment_range(original)?;
        // tsc keeps the alias assignment in the variable initializer below
        // ES2022.  Only native static fields/blocks need the class-this
        // transport block; class-field lowering otherwise owns the ordered
        // statements following the declaration.
        let assign_class_alias_in_static_block = self.target >= ScriptTarget::ES2022
            && class_alias.is_some()
            && self.class_has_static_property_or_block(data.members)?;
        if assign_class_alias_in_static_block {
            let alias =
                self.create_generated_identifier(class_alias.expect("class alias is present"))?;
            let this = self.context.factory()?.create_token(
                self.source,
                SyntaxKind::ThisKeyword,
                TransformFlags::NONE,
            )?;
            let assignment = self.create_assignment(alias, this)?;
            let statement = self.create_expression_statement(assignment)?;
            let block = self.create_block(vec![statement], false)?;
            let static_block = self.context.factory()?.create_node(
                self.source,
                NodeData::ClassStaticBlockDeclaration(
                    tsc_syntax::nodes::ClassStaticBlockDeclarationData {
                        body: Some(block.node()),
                        modifiers: None,
                    },
                ),
                TransformFlags::NONE,
            )?;
            let mut members = self.array_nodes(data.members)?;
            members.insert(0, static_block);
            data.members = Some(
                self.context
                    .factory()?
                    .create_node_array(self.source, members)?
                    .array(),
            );
        }
        let modifiers = self.strip_class_declaration_modifiers(data.modifiers)?;
        let transform_flags = self.context.arena().transform_flags(original);
        let class_expression = self.context.factory()?.create_node(
            self.source,
            NodeData::ClassExpression(tsc_syntax::nodes::ClassExpressionData {
                name: expression_name,
                type_parameters: None,
                heritage_clauses: data.heritage_clauses.take(),
                members: data.members.take(),
                modifiers,
            }),
            transform_flags,
        )?;
        self.set_original_only(class_expression, original)?;
        self.context.factory()?.set_text_range_from_source_range(
            class_expression,
            location.source,
            location.range,
        )?;
        self.context
            .arena_mut()?
            .metadata_mut(class_expression)
            .class_expression_declaration_origin =
            Some(ClassExpressionDeclarationOrigin::LegacyDecorated {
                declaration: original,
            });
        let initializer =
            if let Some(alias) = class_alias.filter(|_| !assign_class_alias_in_static_block) {
                let alias = self.create_generated_identifier(alias)?;
                self.create_assignment(alias, class_expression)?
            } else {
                class_expression
            };
        let declaration = self.context.factory()?.create_node(
            self.source,
            NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                name: Some(name),
                exclamation_token: None,
                r#type: None,
                initializer: Some(initializer.node()),
            }),
            TransformFlags::NONE,
        )?;
        self.context
            .arena_mut()?
            .set_original_node(declaration, Some(original))?;
        let statement = self.create_variable_statement(vec![declaration], NodeFlags::LET)?;
        self.set_original_only(statement, original)?;
        self.context.factory()?.set_text_range_from_source_range(
            statement,
            location.source,
            location.range,
        )?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .set_comment_range(declaration_comment_range);
        Ok(vec![statement])
    }

    fn transform_class_members(
        &mut self,
        class: TransformNode,
        members: Option<NodeArrayId>,
        class_name: &str,
        serialization_context: MetadataSerializationContext,
        constructor_handoff: Option<ConstructorHandoff>,
    ) -> Result<TransformedClassMembers, TransformError> {
        let current_members = self.array_nodes(members)?;
        let original_class = self.context.arena().get_original_node(class);
        let original_members = match &self.context.arena().node(original_class)?.data {
            NodeData::ClassDeclaration(data) => self.array_nodes(data.members)?,
            NodeData::ClassExpression(data) => self.array_nodes(data.members)?,
            _ => Vec::new(),
        };
        let LegacyClassMemberPlan {
            emitted_members,
            decorations,
            constructor_parameters,
            contains_private_decorator_expression,
        } = self.prepare_class_member_plan(
            members,
            &current_members,
            &original_members,
            serialization_context,
            constructor_handoff,
        )?;

        let mut instance = Vec::new();
        let mut static_ = Vec::new();
        for prepared in decorations {
            let statement = self.create_member_decoration_statement(
                class_name,
                prepared.current_member,
                prepared.owner,
                prepared.decorators,
                prepared.metadata,
            )?;
            if self.has_static_modifier(prepared.current_member)? {
                static_.push(statement);
            } else {
                instance.push(statement);
            }
        }
        instance.extend(static_);

        Ok(TransformedClassMembers {
            emitted_members,
            decoration_statements: instance,
            constructor_parameters,
            contains_private_decorator_expression,
        })
    }

    /// Reproduce transformTypeScript's per-class-element sequencing. For each
    /// source owner, decorator metadata is serialized before that same member's
    /// computed name is prepared. This matters when both operations allocate
    /// generated bindings: tsc assigns serializer bindings first.
    ///
    /// Computed-name preparation is intentionally independent of decoration
    /// ownership. Both halves of an accessor pair pass through
    /// `visitPropertyNameOfClassElement`, even though only one half owns the
    /// eventual `__decorate` statement.
    fn prepare_class_member_plan(
        &mut self,
        members: Option<NodeArrayId>,
        current_members: &[TransformNode],
        original_members: &[TransformNode],
        serialization_context: MetadataSerializationContext,
        constructor_handoff: Option<ConstructorHandoff>,
    ) -> Result<LegacyClassMemberPlan, TransformError> {
        let mut current_members_by_original = BTreeMap::new();
        for (index, member) in current_members.iter().enumerate() {
            let original = self.context.arena().get_original_node(*member);
            current_members_by_original.insert(original.node(), index);
        }

        let mut metadata_by_original = BTreeMap::new();
        let mut prepared_current_members = BTreeMap::new();
        for original_member in original_members {
            let current_member = current_members_by_original
                .get(&original_member.node())
                .map(|index| current_members[*index]);
            // transformTypeScript visits a member's modifiers before it
            // injects metadata. Visiting the decorator expressions here lets
            // nested decorated class expressions allocate their computed-name
            // storage before serializer temporaries for this element.
            let member_decorators = current_member
                .map(|member| {
                    self.visit_member_decorator_expressions(
                        member,
                        ClassElementPreparationMode::Runtime,
                    )
                })
                .transpose()?;

            if self.emit_decorator_metadata
                && self.class_member_or_child_is_legacy_decorated(*original_member)?
            {
                if let Some(owner) = self.metadata_decorator_owner(*original_member)? {
                    let owner_member = owner.member();
                    if !current_members_by_original.contains_key(&owner_member.node()) {
                        return Err(TransformError::MissingTransformHandoff {
                            producer: "transformTypeScript",
                            consumer: "transformLegacyDecorators",
                            node: owner_member,
                            handoff: "decorated class-element anchor",
                        });
                    }
                    let metadata = self.member_metadata(owner_member, serialization_context)?;
                    metadata_by_original.insert(owner_member.node(), metadata);
                }
            }

            if let Some(current_member) = current_member {
                let prepared = self.prepare_and_visit_class_element(
                    current_member,
                    member_decorators.unwrap_or_default(),
                    ClassElementPreparationMode::Runtime,
                )?;
                prepared_current_members.insert(current_member.node(), prepared);
            }
        }

        // TypeScript can synthesize class elements whose original is not a
        // direct member of the source class. They cannot own source metadata,
        // but retain the previous computed-name preparation contract.
        for current_member in current_members {
            if !prepared_current_members.contains_key(&current_member.node()) {
                let member_decorators = self.visit_member_decorator_expressions(
                    *current_member,
                    ClassElementPreparationMode::Runtime,
                )?;
                let prepared = self.prepare_and_visit_class_element(
                    *current_member,
                    member_decorators,
                    ClassElementPreparationMode::Runtime,
                )?;
                prepared_current_members.insert(current_member.node(), prepared);
            }
        }

        let emitted_members = if let Some(members) = members {
            let mut emitted = Vec::with_capacity(current_members.len());
            for current_member in current_members {
                let prepared = prepared_current_members.get(&current_member.node()).ok_or(
                    TransformError::MissingTransformHandoff {
                        producer: "transformTypeScript",
                        consumer: "transformLegacyDecorators",
                        node: *current_member,
                        handoff: "visited class-element transaction",
                    },
                )?;
                debug_assert_eq!(prepared.runtime_member, *current_member);
                if let Some(member) = prepared.emitted_member {
                    emitted.push(member);
                }
            }
            let original = self.array(members);
            let updated = self
                .context
                .factory()?
                .update_node_array(original, emitted)?;
            let emitted = Some(updated.array());
            self.arrays.insert(members, emitted);
            emitted
        } else {
            None
        };

        // transformLegacyDecorators observes the class after the computed-name
        // rewrite above. Its accessor owner can therefore differ from the
        // parse-tree owner that received metadata (notably for `[-1]`).
        let constructor_parameters = if let Some(handoff) = constructor_handoff {
            debug_assert_eq!(
                self.context
                    .arena()
                    .get_original_node(handoff.current_with_body),
                handoff.original_with_body,
            );
            prepared_current_members
                .get(&handoff.current_with_body.node())
                .ok_or(TransformError::MissingTransformHandoff {
                    producer: "transformTypeScript",
                    consumer: "transformLegacyDecorators",
                    node: handoff.current_with_body,
                    handoff: "visited constructor parameter decorators",
                })?
                .decorators
                .parameters
                .clone()
        } else {
            Vec::new()
        };

        let mut decorations = Vec::new();
        let mut contains_private_decorator_expression = false;
        for current_member in current_members {
            let Some(runtime_owner) = self.runtime_decorator_owner(
                current_members,
                *current_member,
                &prepared_current_members,
                &metadata_by_original,
            )?
            else {
                continue;
            };
            let prepared = prepared_current_members.get(&current_member.node()).ok_or(
                TransformError::MissingTransformHandoff {
                    producer: "transformTypeScript",
                    consumer: "transformLegacyDecorators",
                    node: *current_member,
                    handoff: "visited member decorator expressions",
                },
            )?;
            let parameters = if let Some(parameter_member) = runtime_owner.parameter_member {
                prepared_current_members
                    .get(&parameter_member.node())
                    .ok_or(TransformError::MissingTransformHandoff {
                        producer: "transformTypeScript",
                        consumer: "transformLegacyDecorators",
                        node: parameter_member,
                        handoff: "visited parameter decorator expressions",
                    })?
                    .decorators
                    .parameters
                    .clone()
            } else {
                Vec::new()
            };
            let decorators = PreparedClassElementDecorators {
                member: prepared.decorators.member.clone(),
                parameters,
            };
            contains_private_decorator_expression |=
                decorators.contains_private_identifier_in_expression();
            if !runtime_owner.admits_owner {
                continue;
            }
            let original = self.context.arena().get_original_node(*current_member);
            let metadata = metadata_by_original
                .remove(&original.node())
                .unwrap_or_default();
            decorations.push(PreparedMemberDecoration {
                owner: runtime_owner.owner,
                current_member: *current_member,
                decorators,
                metadata,
            });
        }

        Ok(LegacyClassMemberPlan {
            emitted_members,
            decorations,
            constructor_parameters,
            contains_private_decorator_expression,
        })
    }

    /// Complete one transformTypeScript class-element transaction before the
    /// next source element can allocate a generated binding. This preserves
    /// tsc's ordering across metadata serialization, the member's computed
    /// name, and nested class expressions in its initializer or body.
    fn prepare_and_visit_class_element(
        &mut self,
        runtime_member: TransformNode,
        member_decorators: Vec<PreparedDecoratorExpression>,
        mode: ClassElementPreparationMode,
    ) -> Result<PreparedClassElement, TransformError> {
        self.prepare_decorated_computed_name(runtime_member)?;
        let owns_function_scope = self.class_element_has_body(runtime_member)?;
        if owns_function_scope {
            // Parameter decorator expressions execute in the same transform
            // lexical environment as parameter initializers and the body.
            // The direct member decorator and computed member name have
            // already been visited in the parent frame above.
            self.enter_function_lexical_environment();
            let inserted = self
                .preentered_function_scopes
                .insert(runtime_member.node());
            debug_assert!(inserted);
        }
        let parameters = self.class_element_parameters(runtime_member)?;
        let parameter_list = match self.visit_class_element_parameter_list(runtime_member, mode) {
            Ok(parameter_list) => parameter_list,
            Err(error) => {
                self.abandon_preentered_function_scope(runtime_member);
                return Err(error);
            }
        };
        let PreparedParameterList {
            emitted: emitted_parameters,
            decorators: parameter_decorators,
        } = parameter_list;
        if let Some(parameters) = parameters {
            self.arrays.insert(parameters, emitted_parameters);
        }
        let emitted_member = match self.visit(runtime_member.node()) {
            Ok(member) => member.map(|node| self.node(node)),
            Err(error) => {
                self.abandon_preentered_function_scope(runtime_member);
                return Err(error);
            }
        };
        debug_assert!(
            !owns_function_scope
                || !self
                    .preentered_function_scopes
                    .contains(&runtime_member.node()),
            "function-like member visit must consume its pre-entered lexical frame",
        );
        Ok(PreparedClassElement {
            runtime_member,
            emitted_member,
            decorators: PreparedClassElementDecorators {
                member: member_decorators,
                parameters: parameter_decorators,
            },
        })
    }

    fn visit_member_decorator_expressions(
        &mut self,
        member: TransformNode,
        mode: ClassElementPreparationMode,
    ) -> Result<Vec<PreparedDecoratorExpression>, TransformError> {
        let modifiers = match &self.context.arena().node(member)?.data {
            NodeData::PropertyDeclaration(data) => data.modifiers,
            NodeData::MethodDeclaration(data) => data.modifiers,
            NodeData::GetAccessor(data) => data.modifiers,
            NodeData::SetAccessor(data) => data.modifiers,
            NodeData::Constructor(data) => data.modifiers,
            _ => None,
        };
        let runtime_role = match mode {
            ClassElementPreparationMode::Runtime => PreparedDecoratorRuntimeRole::Direct {
                admits_owner: self.member_direct_decorator_admits_owner(member)?,
            },
            ClassElementPreparationMode::VisitationOnly => {
                PreparedDecoratorRuntimeRole::VisitationOnly
            }
        };
        self.visit_decorator_expressions(modifiers, runtime_role)
    }

    fn visit_class_element_parameter_list(
        &mut self,
        member: TransformNode,
        mode: ClassElementPreparationMode,
    ) -> Result<PreparedParameterList, TransformError> {
        let parameters = self.class_element_parameters(member)?;
        let parameter_nodes = self.array_nodes(parameters)?;
        let mut emitted = Vec::with_capacity(parameter_nodes.len());
        let mut decorators = Vec::new();
        let mut runtime_index = 0usize;
        for parameter in parameter_nodes {
            // transformTypeScript removes every explicit `this` parameter
            // from the current tree. Keep recovery/synthetic trees on that
            // same boundary: none of the parameter's modifier, binding-name,
            // or initializer surface is producer-visited.
            if self.is_this_parameter(parameter) {
                self.nodes.insert(parameter.node(), None);
                continue;
            }
            let modifiers = match &self.context.arena().node(parameter)?.data {
                NodeData::Parameter(data) => data.modifiers,
                _ => None,
            };
            let runtime_role = match mode {
                ClassElementPreparationMode::Runtime => PreparedDecoratorRuntimeRole::Parameter {
                    runtime_index,
                    admits_owner: self
                        .class_element_parameter_decorator_admits_owner(member, parameter)?,
                },
                ClassElementPreparationMode::VisitationOnly => {
                    PreparedDecoratorRuntimeRole::VisitationOnly
                }
            };
            decorators.extend(self.visit_decorator_expressions(modifiers, runtime_role)?);
            if let Some(parameter) = self.visit(parameter.node())? {
                emitted.push(self.node(parameter));
            }
            runtime_index += 1;
        }
        let emitted = match parameters {
            Some(parameters) => {
                let parameters = self.array(parameters);
                Some(
                    self.context
                        .factory()?
                        .update_node_array(parameters, emitted)?
                        .array(),
                )
            }
            None => None,
        };
        Ok(PreparedParameterList {
            emitted,
            decorators,
        })
    }

    fn class_element_parameters(
        &self,
        member: TransformNode,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        Ok(match &self.context.arena().node(member)?.data {
            NodeData::Constructor(data) => data.parameters,
            NodeData::MethodDeclaration(data) => data.parameters,
            NodeData::GetAccessor(data) => data.parameters,
            NodeData::SetAccessor(data) => data.parameters,
            _ => None,
        })
    }

    fn visit_decorator_expressions(
        &mut self,
        modifiers: Option<NodeArrayId>,
        runtime_role: PreparedDecoratorRuntimeRole,
    ) -> Result<Vec<PreparedDecoratorExpression>, TransformError> {
        let mut expressions = Vec::new();
        for modifier in self.array_nodes(modifiers)? {
            let contains_private_identifier_in_expression = self
                .context
                .arena()
                .transform_flags(modifier)
                .contains(TransformFlags::CONTAINS_PRIVATE_IDENTIFIER_IN_EXPRESSION);
            let NodeData::Decorator(data) = &self.context.arena().node(modifier)?.data else {
                continue;
            };
            let expression = data
                .expression
                .and_then(|expression| self.context.arena().node_ref(self.source, expression))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Decorator,
                    field: "expression",
                })?;
            // Static-block placement consumes transformTypeScript's handoff
            // fact. Visiting a nested decorated class can erase the inner
            // decorator expression, so recomputing from the post-legacy node
            // would lose a private access that was present at aggregation.
            let expression = self
                .visit(expression.node())?
                .map(|expression| self.node(expression))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Decorator,
                    field: "expression",
                })?;
            expressions.push(PreparedDecoratorExpression {
                visited: expression,
                runtime_role,
                contains_private_identifier_in_expression,
            });
        }
        Ok(expressions)
    }

    /// Establish the shared identity for decorated computed names before any
    /// consumer rewrites class elements. This is the semantic boundary used by
    /// tsc's TypeScript transform: legacy decoration and class-field lowering
    /// must refer to one cache rather than independently evaluating the key.
    fn prepare_class_expression_members(
        &mut self,
        members: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let current_members = self.array_nodes(members)?;
        let mut emitted_members = Vec::with_capacity(current_members.len());
        for member in current_members {
            let member_decorators = self.visit_member_decorator_expressions(
                member,
                ClassElementPreparationMode::VisitationOnly,
            )?;
            let prepared = self.prepare_and_visit_class_element(
                member,
                member_decorators,
                ClassElementPreparationMode::VisitationOnly,
            )?;
            debug_assert_eq!(prepared.runtime_member, member);
            if let Some(member) = prepared.emitted_member {
                emitted_members.push(member);
            }
        }
        if let Some(members) = members {
            let original = self.array(members);
            let updated = self
                .context
                .factory()?
                .update_node_array(original, emitted_members)?;
            let updated = Some(updated.array());
            self.arrays.insert(members, updated);
            Ok(updated)
        } else {
            Ok(None)
        }
    }

    fn prepare_decorated_computed_name(
        &mut self,
        member: TransformNode,
    ) -> Result<(), TransformError> {
        // transformTypeScript's `visitPropertyNameOfClassElement` keys this
        // cache from a decorator on the member itself. This rule is shared by
        // class declarations and class expressions.
        let plan = self.decorator_computed_name_plan(member)?;
        let original_member = self.context.arena().get_original_node(member);
        // The direct-decorator fact belongs to the source member, while
        // inlineability belongs to the transformTypeScript result (for
        // example, `"x" as string` has already become `"x"`).
        let record = self.context.arena().node(member)?.clone();
        let name = match record.data {
            NodeData::PropertyDeclaration(data) => data.name,
            NodeData::MethodDeclaration(data) => data.name,
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            _ => return Ok(()),
        };
        let Some(name) = name.and_then(|name| self.context.arena().node_ref(self.source, name))
        else {
            return Ok(());
        };
        let NodeData::ComputedPropertyName(data) = self.context.arena().node(name)?.data.clone()
        else {
            return Ok(());
        };
        let expression = data
            .expression
            .and_then(|expression| self.context.arena().node_ref(self.source, expression))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ComputedPropertyName,
                field: "expression",
            })?;
        // visitPropertyNameOfClassElement first visits the expression and only
        // then decides whether to allocate the outer generated name. Nested
        // decorated class expressions can allocate their own binding during
        // that visit and must therefore precede this member's binding.
        let expression = self
            .visit(expression.node())?
            .map(|expression| self.node(expression))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ComputedPropertyName,
                field: "expression",
            })?;
        if !plan.caches_at_class_evaluation() {
            return Ok(());
        }
        let inner = self.skip_partially_emitted_expressions(expression)?;
        if self.is_simple_inlineable_expression(inner)? {
            return Ok(());
        }
        if !self.computed_names.contains_key(&original_member.node()) {
            let generated = self.allocate_temp_name(
                LegacyGeneratedBindingKind::DecoratedComputedName,
                LegacyTempBindingStorage::Hoisted,
            )?;
            self.computed_names
                .insert(original_member.node(), generated);
        }
        Ok(())
    }

    fn create_member_decoration_statement(
        &mut self,
        class_name: &str,
        member: TransformNode,
        owner: DecoratorOwner,
        prepared_decorators: PreparedClassElementDecorators,
        metadata: Vec<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let owner_member = owner.member();
        let record = self.context.arena().node(owner_member)?.clone();
        let is_property = matches!(&record.data, NodeData::PropertyDeclaration(_));

        // transformLegacyDecorators consumes the expressions visited by the
        // element transaction. Reopening either modifier tree here would
        // revive erased syntax or skip nested legacy-decorator work.
        let current_record = self.context.arena().node(member)?.clone();
        let modifiers = match &current_record.data {
            NodeData::PropertyDeclaration(data) => data.modifiers,
            NodeData::MethodDeclaration(data) => data.modifiers,
            NodeData::GetAccessor(data) => data.modifiers,
            NodeData::SetAccessor(data) => data.modifiers,
            _ => unreachable!("decorator owner is a supported class element"),
        };
        let mut decorators = prepared_decorators
            .member
            .into_iter()
            .map(|decorator| decorator.visited)
            .collect::<Vec<_>>();
        for parameter in prepared_decorators.parameters {
            let PreparedDecoratorRuntimeRole::Parameter { runtime_index, .. } =
                parameter.runtime_role
            else {
                debug_assert!(
                    false,
                    "member parameter handoff must carry a parameter role"
                );
                continue;
            };
            decorators.push(self.create_param(runtime_index, parameter.visited)?);
        }
        debug_assert!(!decorators.is_empty());
        decorators.extend(metadata);
        let target = if self.has_modifier(modifiers, SyntaxKind::StaticKeyword)? {
            self.create_identifier(class_name)?
        } else {
            let class = self.create_identifier(class_name)?;
            self.create_property_access(class, "prototype")?
        };
        let member_record = self.context.arena().node(member)?;
        let name = match &member_record.data {
            NodeData::PropertyDeclaration(data) => data.name,
            NodeData::MethodDeclaration(data) => data.name,
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            _ => None,
        }
        .ok_or(TransformError::RequiredChildRemoved {
            parent: record.kind,
            field: "name",
        })?;
        let computed_name_plan = self.decorator_computed_name_plan(member)?;
        let member_name = self.property_name_expression(member, name, computed_name_plan)?;
        let descriptor =
            if is_property && !self.has_modifier(modifiers, SyntaxKind::AccessorKeyword)? {
                self.create_void_zero()?
            } else {
                self.create_null()?
            };
        let call =
            self.create_decorate_call(decorators, target, Some(member_name), Some(descriptor))?;
        let statement = self.create_expression_statement(call)?;
        self.context
            .arena_mut()?
            .metadata_mut(statement)
            .add_flags(EmitFlags::NO_COMMENTS);
        Ok(statement)
    }

    fn materialize_class_decorators(
        &mut self,
        prepared: PreparedClassDecoration,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let mut decorators = prepared
            .direct
            .into_iter()
            .map(|decorator| decorator.visited)
            .collect::<Vec<_>>();
        for parameter in prepared.constructor_parameters {
            let PreparedDecoratorRuntimeRole::Parameter { runtime_index, .. } =
                parameter.runtime_role
            else {
                debug_assert!(false, "constructor handoff must carry a parameter role");
                continue;
            };
            decorators.push(self.create_param(runtime_index, parameter.visited)?);
        }
        if let Some(metadata) = prepared.metadata {
            decorators.push(metadata);
        }
        Ok(decorators)
    }

    fn member_metadata(
        &mut self,
        member: TransformNode,
        serialization_context: MetadataSerializationContext,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let original = self.context.arena().get_original_node(member);
        let record = self.context.arena().node(original)?.clone();
        let mut metadata = Vec::new();
        match record.data {
            NodeData::PropertyDeclaration(data) => {
                let value = self.serialize_type(
                    data.r#type,
                    MetadataFallback::Object,
                    serialization_context,
                )?;
                metadata.push(self.create_metadata("design:type", value)?);
            }
            NodeData::MethodDeclaration(data) => {
                let value = self.create_identifier("Function")?;
                metadata.push(self.create_metadata("design:type", value)?);
                let parameters = self
                    .serialize_parameter_types_from_array(data.parameters, serialization_context)?;
                let value = self.create_array_literal(parameters, false)?;
                metadata.push(self.create_metadata("design:paramtypes", value)?);
                let value = if data.r#type.is_none()
                    && self.has_modifier(data.modifiers, SyntaxKind::AsyncKeyword)?
                {
                    self.create_identifier("Promise")?
                } else {
                    self.serialize_type(
                        data.r#type,
                        MetadataFallback::VoidZero,
                        serialization_context,
                    )?
                };
                metadata.push(self.create_metadata("design:returntype", value)?);
            }
            NodeData::GetAccessor(data) => {
                let group = self.metadata_accessor_group(original)?;
                let accessor_type = self.metadata_accessor_type(group)?;
                let parameters = if let Some(setter) = group.set_accessor {
                    self.accessor_parameters(setter)?
                } else {
                    data.parameters
                };
                let value = self.serialize_type(
                    accessor_type,
                    MetadataFallback::Object,
                    serialization_context,
                )?;
                metadata.push(self.create_metadata("design:type", value)?);
                let parameters =
                    self.serialize_parameter_types_from_array(parameters, serialization_context)?;
                let value = self.create_array_literal(parameters, false)?;
                metadata.push(self.create_metadata("design:paramtypes", value)?);
            }
            NodeData::SetAccessor(data) => {
                let group = self.metadata_accessor_group(original)?;
                let accessor_type = self.metadata_accessor_type(group)?;
                let value = self.serialize_type(
                    accessor_type,
                    MetadataFallback::Object,
                    serialization_context,
                )?;
                metadata.push(self.create_metadata("design:type", value)?);
                let parameters = self
                    .serialize_parameter_types_from_array(data.parameters, serialization_context)?;
                let value = self.create_array_literal(parameters, false)?;
                metadata.push(self.create_metadata("design:paramtypes", value)?);
            }
            _ => {}
        }
        Ok(metadata)
    }

    fn metadata_accessor_type(
        &self,
        group: MetadataAccessorGroup,
    ) -> Result<Option<NodeId>, TransformError> {
        if let Some(setter) = group.set_accessor {
            let parameters = self.accessor_parameters(setter)?;
            if let Some(r#type) = self.setter_value_parameter_type(parameters)? {
                return Ok(Some(r#type));
            }
        }
        let Some(getter) = group.get_accessor else {
            return Ok(None);
        };
        Ok(match &self.context.arena().node(getter)?.data {
            NodeData::GetAccessor(data) => data.r#type,
            _ => None,
        })
    }

    fn accessor_parameters(
        &self,
        accessor: TransformNode,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        Ok(match &self.context.arena().node(accessor)?.data {
            NodeData::GetAccessor(data) => data.parameters,
            NodeData::SetAccessor(data) => data.parameters,
            _ => None,
        })
    }

    fn metadata_accessor_group(
        &self,
        accessor: TransformNode,
    ) -> Result<MetadataAccessorGroup, TransformError> {
        let record = self.context.arena().node(accessor)?;
        let (accessor_name, accessor_modifiers) = match &record.data {
            NodeData::GetAccessor(data) => (data.name, data.modifiers),
            NodeData::SetAccessor(data) => (data.name, data.modifiers),
            _ => unreachable!("metadata accessor group requires an accessor"),
        };
        let accessor_is_static =
            self.has_modifier(accessor_modifiers, SyntaxKind::StaticKeyword)?;

        if matches!(
            accessor_name
                .map(|name| self.property_name_identity(self.node(name)))
                .transpose()?
                .flatten(),
            Some(AccessorPropertyNameIdentity::Dynamic)
        ) {
            return Ok(MetadataAccessorGroup {
                first_accessor: accessor,
                second_accessor: None,
                get_accessor: matches!(&record.data, NodeData::GetAccessor(_)).then_some(accessor),
                set_accessor: matches!(&record.data, NodeData::SetAccessor(_)).then_some(accessor),
            });
        }

        let Some(parent) = record.parent else {
            return Ok(MetadataAccessorGroup {
                first_accessor: accessor,
                second_accessor: None,
                get_accessor: matches!(&record.data, NodeData::GetAccessor(_)).then_some(accessor),
                set_accessor: matches!(&record.data, NodeData::SetAccessor(_)).then_some(accessor),
            });
        };
        let members = match &self.context.arena().node(self.node(parent))?.data {
            NodeData::ClassDeclaration(data) => data.members,
            NodeData::ClassExpression(data) => data.members,
            _ => None,
        };

        let mut first_accessor = None;
        let mut second_accessor = None;
        let mut get_accessor = None;
        let mut set_accessor = None;
        for candidate in self.array_nodes(members)? {
            let (modifiers, name, is_getter) = match &self.context.arena().node(candidate)?.data {
                NodeData::GetAccessor(data) => (data.modifiers, data.name, true),
                NodeData::SetAccessor(data) => (data.modifiers, data.name, false),
                _ => continue,
            };
            if self.has_modifier(modifiers, SyntaxKind::StaticKeyword)? == accessor_is_static
                && self.property_names_equal(accessor_name, name)?
            {
                if first_accessor.is_none() {
                    first_accessor = Some(candidate);
                } else if second_accessor.is_none() {
                    second_accessor = Some(candidate);
                }
                if is_getter && get_accessor.is_none() {
                    get_accessor = Some(candidate);
                } else if !is_getter && set_accessor.is_none() {
                    set_accessor = Some(candidate);
                }
            }
        }
        Ok(MetadataAccessorGroup {
            first_accessor: first_accessor.unwrap_or(accessor),
            second_accessor,
            get_accessor,
            set_accessor,
        })
    }

    fn metadata_accessor_owner(
        &self,
        accessor: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let group = self.metadata_accessor_group(accessor)?;
        for candidate in [Some(group.first_accessor), group.second_accessor]
            .into_iter()
            .flatten()
        {
            let modifiers = match &self.context.arena().node(candidate)?.data {
                NodeData::GetAccessor(data) => data.modifiers,
                NodeData::SetAccessor(data) => data.modifiers,
                _ => None,
            };
            if !self.decorator_expressions(modifiers)?.is_empty() {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    fn runtime_accessor_group(
        &self,
        current_members: &[TransformNode],
        accessor: TransformNode,
    ) -> Result<RuntimeAccessorGroup, TransformError> {
        let record = self.context.arena().node(accessor)?;
        let accessor_modifiers = match &record.data {
            NodeData::GetAccessor(data) => data.modifiers,
            NodeData::SetAccessor(data) => data.modifiers,
            _ => unreachable!("runtime accessor group requires an accessor"),
        };
        let accessor_is_static =
            self.has_modifier(accessor_modifiers, SyntaxKind::StaticKeyword)?;
        if matches!(
            self.runtime_property_name_identity(accessor)?,
            Some(AccessorPropertyNameIdentity::Dynamic)
        ) {
            return Ok(RuntimeAccessorGroup {
                first_accessor: accessor,
                second_accessor: None,
                get_accessor: matches!(&record.data, NodeData::GetAccessor(_)).then_some(accessor),
                set_accessor: matches!(&record.data, NodeData::SetAccessor(_)).then_some(accessor),
            });
        }

        let mut first_accessor = None;
        let mut second_accessor = None;
        let mut get_accessor = None;
        let mut set_accessor = None;
        for candidate in current_members {
            let (modifiers, is_getter) = match &self.context.arena().node(*candidate)?.data {
                NodeData::GetAccessor(data) => (data.modifiers, true),
                NodeData::SetAccessor(data) => (data.modifiers, false),
                _ => continue,
            };
            if self.has_modifier(modifiers, SyntaxKind::StaticKeyword)? == accessor_is_static
                && self.runtime_property_names_equal(accessor, *candidate)?
            {
                if first_accessor.is_none() {
                    first_accessor = Some(*candidate);
                } else if second_accessor.is_none() {
                    second_accessor = Some(*candidate);
                }
                if is_getter && get_accessor.is_none() {
                    get_accessor = Some(*candidate);
                } else if !is_getter && set_accessor.is_none() {
                    set_accessor = Some(*candidate);
                }
            }
        }
        Ok(RuntimeAccessorGroup {
            first_accessor: first_accessor.unwrap_or(accessor),
            second_accessor,
            get_accessor,
            set_accessor,
        })
    }

    fn runtime_property_name_identity(
        &self,
        member: TransformNode,
    ) -> Result<Option<AccessorPropertyNameIdentity>, TransformError> {
        let original = self.context.arena().get_original_node(member);
        if self.computed_names.contains_key(&original.node()) {
            return Ok(Some(AccessorPropertyNameIdentity::Dynamic));
        }
        let name = match &self.context.arena().node(member)?.data {
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            _ => None,
        };
        name.map(|name| self.property_name_identity(self.node(name)))
            .transpose()
            .map(Option::flatten)
    }

    fn runtime_property_names_equal(
        &self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<bool, TransformError> {
        let left = self.runtime_property_name_identity(left)?;
        let right = self.runtime_property_name_identity(right)?;
        Ok(matches!(
            (left, right),
            (
                Some(AccessorPropertyNameIdentity::Static(left)),
                Some(AccessorPropertyNameIdentity::Static(right)),
            ) if left == right
        ))
    }

    fn runtime_accessor_has_decorators(
        &self,
        accessor: TransformNode,
        prepared_current_members: &BTreeMap<NodeId, PreparedClassElement>,
        metadata_by_original: &BTreeMap<NodeId, Vec<TransformNode>>,
    ) -> Result<bool, TransformError> {
        let prepared = prepared_current_members.get(&accessor.node()).ok_or(
            TransformError::MissingTransformHandoff {
                producer: "transformTypeScript",
                consumer: "transformLegacyDecorators",
                node: accessor,
                handoff: "visited accessor decorator expressions",
            },
        )?;
        let original = self.context.arena().get_original_node(accessor);
        Ok(!prepared.decorators.member.is_empty()
            || metadata_by_original.contains_key(&original.node()))
    }

    fn runtime_accessor_owner(
        &self,
        group: RuntimeAccessorGroup,
        prepared_current_members: &BTreeMap<NodeId, PreparedClassElement>,
        metadata_by_original: &BTreeMap<NodeId, Vec<TransformNode>>,
    ) -> Result<Option<TransformNode>, TransformError> {
        for candidate in [Some(group.first_accessor), group.second_accessor]
            .into_iter()
            .flatten()
        {
            if self.runtime_accessor_has_decorators(
                candidate,
                prepared_current_members,
                metadata_by_original,
            )? {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }

    fn runtime_decorator_owner(
        &self,
        current_members: &[TransformNode],
        member: TransformNode,
        prepared_current_members: &BTreeMap<NodeId, PreparedClassElement>,
        metadata_by_original: &BTreeMap<NodeId, Vec<TransformNode>>,
    ) -> Result<Option<RuntimeDecoratorOwner>, TransformError> {
        let original = self.context.arena().get_original_node(member);
        let prepared = prepared_current_members.get(&member.node()).ok_or(
            TransformError::MissingTransformHandoff {
                producer: "transformTypeScript",
                consumer: "transformLegacyDecorators",
                node: member,
                handoff: "visited class-element decorator expressions",
            },
        )?;
        Ok(match &self.context.arena().node(member)?.data {
            NodeData::PropertyDeclaration(_) => {
                let decorated = !prepared.decorators.member.is_empty()
                    || metadata_by_original.contains_key(&original.node());
                decorated.then_some(RuntimeDecoratorOwner {
                    owner: DecoratorOwner::Property(original),
                    parameter_member: None,
                    admits_owner: prepared.decorators.admits_owner(),
                })
            }
            NodeData::MethodDeclaration(_) => {
                let decorated = !prepared.decorators.member.is_empty()
                    || !prepared.decorators.parameters.is_empty()
                    || metadata_by_original.contains_key(&original.node());
                decorated.then_some(RuntimeDecoratorOwner {
                    owner: DecoratorOwner::MethodWithBody(original),
                    parameter_member: Some(member),
                    admits_owner: prepared.decorators.admits_owner(),
                })
            }
            NodeData::GetAccessor(_) | NodeData::SetAccessor(_) => {
                let group = self.runtime_accessor_group(current_members, member)?;
                let owner = self.runtime_accessor_owner(
                    group,
                    prepared_current_members,
                    metadata_by_original,
                )?;
                if owner != Some(member) {
                    None
                } else {
                    debug_assert!(group.get_accessor.is_some() || group.set_accessor.is_some());
                    Some(RuntimeDecoratorOwner {
                        owner: DecoratorOwner::AccessorWithBody(original),
                        parameter_member: group.set_accessor,
                        // Admission belongs to the aggregation owner. A setter
                        // parameter decorator is appended to a getter-owned
                        // aggregate, but cannot make a private getter runtime
                        // admissible on its own.
                        admits_owner: prepared.decorators.admits_owner(),
                    })
                }
            }
            _ => None,
        })
    }

    fn setter_value_parameter_type(
        &self,
        parameters: Option<NodeArrayId>,
    ) -> Result<Option<NodeId>, TransformError> {
        let parameters = self.array_nodes(parameters)?;
        let Some(mut parameter) = parameters.first().copied() else {
            return Ok(None);
        };
        if parameters.len() == 2 && self.is_this_parameter(parameter) {
            parameter = parameters[1];
        }
        Ok(match &self.context.arena().node(parameter)?.data {
            NodeData::Parameter(data) => data.r#type,
            _ => None,
        })
    }

    fn property_names_equal(
        &self,
        left: Option<NodeId>,
        right: Option<NodeId>,
    ) -> Result<bool, TransformError> {
        let (Some(left), Some(right)) = (left, right) else {
            return Ok(false);
        };
        let left = self.property_name_identity(self.node(left))?;
        let right = self.property_name_identity(self.node(right))?;
        Ok(matches!(
            (left, right),
            (
                Some(AccessorPropertyNameIdentity::Static(left)),
                Some(AccessorPropertyNameIdentity::Static(right)),
            ) if left == right
        ))
    }

    fn property_name_identity(
        &self,
        name: TransformNode,
    ) -> Result<Option<AccessorPropertyNameIdentity>, TransformError> {
        Ok(match &self.context.arena().node(name)?.data {
            NodeData::Identifier(data) => Some(AccessorPropertyNameIdentity::Static(
                data.escaped_text.clone(),
            )),
            NodeData::PrivateIdentifier(data) => Some(AccessorPropertyNameIdentity::Static(
                data.escaped_text.clone(),
            )),
            NodeData::StringLiteral(data) => Some(AccessorPropertyNameIdentity::Static(
                tsc_syntax::escape_leading_underscores(&data.text),
            )),
            NodeData::NumericLiteral(data) => Some(AccessorPropertyNameIdentity::Static(
                tsc_syntax::escape_leading_underscores(&data.text),
            )),
            NodeData::BigIntLiteral(data) => Some(AccessorPropertyNameIdentity::Static(
                tsc_syntax::escape_leading_underscores(&data.text),
            )),
            NodeData::NoSubstitutionTemplateLiteral(data) => {
                Some(AccessorPropertyNameIdentity::Static(
                    tsc_syntax::escape_leading_underscores(&data.text),
                ))
            }
            NodeData::ComputedPropertyName(data) => {
                let Some(expression) = data
                    .expression
                    .and_then(|expression| self.context.arena().node_ref(self.source, expression))
                else {
                    return Ok(None);
                };
                Some(self.computed_property_name_identity(expression)?)
            }
            _ => None,
        })
    }

    fn computed_property_name_identity(
        &self,
        expression: TransformNode,
    ) -> Result<AccessorPropertyNameIdentity, TransformError> {
        Ok(match &self.context.arena().node(expression)?.data {
            NodeData::StringLiteral(data) => AccessorPropertyNameIdentity::Static(
                tsc_syntax::escape_leading_underscores(&data.text),
            ),
            NodeData::NumericLiteral(data) => AccessorPropertyNameIdentity::Static(
                tsc_syntax::escape_leading_underscores(&data.text),
            ),
            NodeData::NoSubstitutionTemplateLiteral(data) => AccessorPropertyNameIdentity::Static(
                tsc_syntax::escape_leading_underscores(&data.text),
            ),
            NodeData::PrefixUnaryExpression(data)
                if matches!(
                    data.operator,
                    SyntaxKind::PlusToken | SyntaxKind::MinusToken
                ) =>
            {
                let Some(operand) = data
                    .operand
                    .and_then(|operand| self.context.arena().node_ref(self.source, operand))
                else {
                    return Ok(AccessorPropertyNameIdentity::Dynamic);
                };
                let NodeData::NumericLiteral(operand) = &self.context.arena().node(operand)?.data
                else {
                    return Ok(AccessorPropertyNameIdentity::Dynamic);
                };
                let text = if data.operator == SyntaxKind::MinusToken {
                    format!("-{}", operand.text)
                } else {
                    operand.text.clone()
                };
                AccessorPropertyNameIdentity::Static(tsc_syntax::escape_leading_underscores(&text))
            }
            _ => AccessorPropertyNameIdentity::Dynamic,
        })
    }

    fn serialize_parameter_types(
        &mut self,
        original_constructor: TransformNode,
        serialization_context: MetadataSerializationContext,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let parameters = match &self.context.arena().node(original_constructor)?.data {
            NodeData::Constructor(data) => data.parameters,
            _ => None,
        };
        self.serialize_parameter_types_from_array(parameters, serialization_context)
    }

    /// tsc-port: shouldAddParamTypesMetadata @6.0.3
    /// tsc-hash: 63e67fe79b1beb9f0fa2ec0f6f6059d3cfa10ee1e8eaff0cb50bfcee9fc7028c
    /// tsc-span: _tsc.js:94708-94719
    /// tsc-port: serializeParameterTypesOfNode @6.0.3
    /// tsc-hash: e5db9892b8e6f775010ce8a63d06bfd2176efbe56de3bd63f3cd9c1582d8d098
    /// tsc-span: _tsc.js:98124-98142
    /// tsc-port: createMetadataHelper @6.0.3
    /// tsc-hash: c651bd38047268e531374ba55fca896a765e6db5bf3aa9e237f25632456fb679
    /// tsc-span: _tsc.js:25551-25562
    fn create_constructor_parameter_metadata(
        &mut self,
        plan: ConstructorMetadataPlan,
    ) -> Result<TransformNode, TransformError> {
        let parameter_types = self.serialize_parameter_types(
            plan.original_constructor_with_body,
            plan.serialization_context,
        )?;
        let value = self.create_array_literal(parameter_types, false)?;
        self.create_metadata("design:paramtypes", value)
    }

    fn serialize_parameter_types_from_array(
        &mut self,
        parameters: Option<NodeArrayId>,
        serialization_context: MetadataSerializationContext,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let parameters = self.array_nodes(parameters)?;
        let mut serialized = Vec::with_capacity(parameters.len());
        for (index, parameter) in parameters.into_iter().enumerate() {
            if index == 0 && self.is_this_parameter(parameter) {
                continue;
            }
            serialized.push(self.serialize_parameter_type(parameter, serialization_context)?);
        }
        Ok(serialized)
    }

    fn serialize_parameter_type(
        &mut self,
        parameter: TransformNode,
        serialization_context: MetadataSerializationContext,
    ) -> Result<TransformNode, TransformError> {
        let original = self.context.arena().get_original_node(parameter);
        let r#type = match &self.context.arena().node(original)?.data {
            NodeData::Parameter(data) if data.dot_dot_dot_token.is_some() => {
                self.rest_parameter_element_type(data.r#type)?
            }
            NodeData::Parameter(data) => data.r#type,
            _ => None,
        };
        self.serialize_type(r#type, MetadataFallback::Object, serialization_context)
    }

    fn is_this_parameter(&self, parameter: TransformNode) -> bool {
        let original = self.context.arena().get_original_node(parameter);
        let Ok(record) = self.context.arena().node(original) else {
            return false;
        };
        let NodeData::Parameter(data) = &record.data else {
            return false;
        };
        data.name.is_some_and(|name| {
            self.context.arena().node(self.node(name)).is_ok_and(
                |name| matches!(&name.data, NodeData::Identifier(data) if data.text == "this"),
            )
        })
    }

    fn rest_parameter_element_type(
        &self,
        r#type: Option<NodeId>,
    ) -> Result<Option<NodeId>, TransformError> {
        let Some(r#type) = r#type else {
            return Ok(None);
        };
        Ok(match &self.context.arena().node(self.node(r#type))?.data {
            NodeData::ArrayType(data) => data.element_type,
            NodeData::TypeReference(data) => data.type_arguments.and_then(|arguments| {
                self.context
                    .arena()
                    .node_array_ref(self.source, arguments)
                    .and_then(|arguments| self.context.arena().node_array(arguments).ok())
                    .and_then(|arguments| {
                        (arguments.nodes.len() == 1).then_some(arguments.nodes[0])
                    })
            }),
            _ => None,
        })
    }

    /// tsc-port: serializeTypeNode @6.0.3
    /// tsc-hash: dcacf9f0f369cd137334d19de5369beae8d497b91141b060ced299bda8c51810
    /// tsc-span: _tsc.js:98161-98330
    fn serialize_type(
        &mut self,
        r#type: Option<NodeId>,
        fallback: MetadataFallback,
        serialization_context: MetadataSerializationContext,
    ) -> Result<TransformNode, TransformError> {
        let Some(r#type) = r#type else {
            return self.metadata_fallback(fallback);
        };
        let node = self.node(r#type);
        let record = self.context.arena().node(node)?.clone();
        match record.kind {
            SyntaxKind::StringKeyword => self.create_identifier("String"),
            SyntaxKind::NumberKeyword => self.create_identifier("Number"),
            SyntaxKind::BooleanKeyword => self.create_identifier("Boolean"),
            SyntaxKind::BigIntKeyword => self.create_identifier("BigInt"),
            SyntaxKind::SymbolKeyword => self.create_identifier("Symbol"),
            SyntaxKind::ObjectKeyword | SyntaxKind::AnyKeyword | SyntaxKind::UnknownKeyword => {
                self.create_identifier("Object")
            }
            SyntaxKind::VoidKeyword | SyntaxKind::UndefinedKeyword | SyntaxKind::NeverKeyword => {
                self.create_void_zero()
            }
            SyntaxKind::FunctionType | SyntaxKind::ConstructorType => {
                self.create_identifier("Function")
            }
            SyntaxKind::ArrayType | SyntaxKind::TupleType => self.create_identifier("Array"),
            SyntaxKind::TypePredicate => match record.data {
                NodeData::TypePredicate(data) if data.asserts_modifier.is_some() => {
                    self.create_void_zero()
                }
                NodeData::TypePredicate(_) => self.create_identifier("Boolean"),
                _ => self.metadata_fallback(fallback),
            },
            SyntaxKind::TemplateLiteralType => self.create_identifier("String"),
            SyntaxKind::LiteralType => match record.data {
                NodeData::LiteralType(data) => self.serialize_literal_type(data.literal),
                _ => self.metadata_fallback(fallback),
            },
            SyntaxKind::ParenthesizedType => match record.data {
                NodeData::ParenthesizedType(data) => {
                    self.serialize_type(data.r#type, fallback, serialization_context)
                }
                _ => self.metadata_fallback(fallback),
            },
            SyntaxKind::TypeReference => match record.data {
                NodeData::TypeReference(data) => self.serialize_entity_name_type(
                    node,
                    data.type_name,
                    fallback,
                    serialization_context,
                ),
                _ => self.metadata_fallback(fallback),
            },
            SyntaxKind::IntersectionType => match record.data {
                NodeData::IntersectionType(data) => {
                    self.serialize_union_or_intersection(data.types, true, serialization_context)
                }
                _ => self.metadata_fallback(fallback),
            },
            SyntaxKind::UnionType => match record.data {
                NodeData::UnionType(data) => {
                    self.serialize_union_or_intersection(data.types, false, serialization_context)
                }
                _ => self.metadata_fallback(fallback),
            },
            SyntaxKind::ConditionalType => match record.data {
                NodeData::ConditionalType(data) => self.serialize_type_constituents(
                    [data.true_type, data.false_type].into_iter().flatten(),
                    false,
                    serialization_context,
                ),
                _ => self.metadata_fallback(fallback),
            },
            SyntaxKind::TypeOperator => match record.data {
                NodeData::TypeOperator(data) if data.operator == SyntaxKind::ReadonlyKeyword => {
                    self.serialize_type(data.r#type, fallback, serialization_context)
                }
                _ => self.create_identifier("Object"),
            },
            SyntaxKind::JSDocNullableType => match record.data {
                NodeData::JSDocNullableType(data) => {
                    self.serialize_type(data.r#type, fallback, serialization_context)
                }
                _ => self.metadata_fallback(fallback),
            },
            SyntaxKind::JSDocNonNullableType => match record.data {
                NodeData::JSDocNonNullableType(data) => {
                    self.serialize_type(data.r#type, fallback, serialization_context)
                }
                _ => self.metadata_fallback(fallback),
            },
            SyntaxKind::JSDocOptionalType => match record.data {
                NodeData::JSDocOptionalType(data) => {
                    self.serialize_type(data.r#type, fallback, serialization_context)
                }
                _ => self.metadata_fallback(fallback),
            },
            _ => self.metadata_fallback(fallback),
        }
    }

    fn serialize_literal_type(
        &mut self,
        literal: Option<NodeId>,
    ) -> Result<TransformNode, TransformError> {
        let Some(literal) = literal else {
            return self.create_identifier("Object");
        };
        let literal = self.node(literal);
        match self.context.arena().node(literal)?.data.clone() {
            NodeData::StringLiteral(_) | NodeData::NoSubstitutionTemplateLiteral(_) => {
                self.create_identifier("String")
            }
            NodeData::NumericLiteral(_) => self.create_identifier("Number"),
            NodeData::BigIntLiteral(_) => self.create_identifier("BigInt"),
            NodeData::Token => match self.context.arena().node(literal)?.kind {
                SyntaxKind::TrueKeyword | SyntaxKind::FalseKeyword => {
                    self.create_identifier("Boolean")
                }
                SyntaxKind::NullKeyword => self.create_void_zero(),
                _ => self.create_identifier("Object"),
            },
            NodeData::PrefixUnaryExpression(data) => {
                let Some(operand) = data.operand else {
                    return self.create_identifier("Object");
                };
                match self.context.arena().node(self.node(operand))?.kind {
                    SyntaxKind::NumericLiteral => self.create_identifier("Number"),
                    SyntaxKind::BigIntLiteral => self.create_identifier("BigInt"),
                    _ => self.create_identifier("Object"),
                }
            }
            _ => self.create_identifier("Object"),
        }
    }

    fn serialize_union_or_intersection(
        &mut self,
        types: Option<NodeArrayId>,
        is_intersection: bool,
        serialization_context: MetadataSerializationContext,
    ) -> Result<TransformNode, TransformError> {
        let types = self
            .array_nodes(types)?
            .into_iter()
            .map(TransformNode::node);
        self.serialize_type_constituents(types, is_intersection, serialization_context)
    }

    fn serialize_type_constituents(
        &mut self,
        types: impl IntoIterator<Item = NodeId>,
        is_intersection: bool,
        serialization_context: MetadataSerializationContext,
    ) -> Result<TransformNode, TransformError> {
        let mut serialized: Option<(TransformNode, String)> = None;
        for r#type in types {
            let r#type = self.skip_type_parentheses(r#type)?;
            let kind = self.context.arena().node(self.node(r#type))?.kind;
            if kind == SyntaxKind::NeverKeyword {
                if is_intersection {
                    return self.create_void_zero();
                }
                continue;
            }
            if kind == SyntaxKind::UnknownKeyword {
                if !is_intersection {
                    return self.create_identifier("Object");
                }
                continue;
            }
            if kind == SyntaxKind::AnyKeyword {
                return self.create_identifier("Object");
            }
            if !self.strict_null_checks && self.is_null_or_undefined_type(r#type)? {
                continue;
            }
            let node = self.serialize_type(
                Some(r#type),
                MetadataFallback::Object,
                serialization_context,
            )?;
            let key = self
                .serialized_type_key(node)?
                .unwrap_or_else(|| "other".to_owned());
            if key == "id:Object" {
                return Ok(node);
            }
            if let Some((_, previous)) = &serialized {
                if *previous != key {
                    return self.create_identifier("Object");
                }
            } else {
                serialized = Some((node, key));
            }
        }
        if let Some((node, _)) = serialized {
            Ok(node)
        } else {
            self.create_void_zero()
        }
    }

    fn skip_type_parentheses(&self, mut r#type: NodeId) -> Result<NodeId, TransformError> {
        loop {
            let NodeData::ParenthesizedType(data) =
                &self.context.arena().node(self.node(r#type))?.data
            else {
                return Ok(r#type);
            };
            let Some(inner) = data.r#type else {
                return Ok(r#type);
            };
            r#type = inner;
        }
    }

    fn is_null_or_undefined_type(&self, r#type: NodeId) -> Result<bool, TransformError> {
        let record = self.context.arena().node(self.node(r#type))?;
        if record.kind == SyntaxKind::UndefinedKeyword {
            return Ok(true);
        }
        let NodeData::LiteralType(data) = &record.data else {
            return Ok(false);
        };
        Ok(data.literal.is_some_and(|literal| {
            self.context
                .arena()
                .node(self.node(literal))
                .is_ok_and(|literal| literal.kind == SyntaxKind::NullKeyword)
        }))
    }

    fn serialized_type_key(&self, node: TransformNode) -> Result<Option<String>, TransformError> {
        Ok(match &self.context.arena().node(node)?.data {
            NodeData::Identifier(data) => Some(format!("id:{}", data.text)),
            NodeData::VoidExpression(data) => data
                .expression
                .and_then(|expression| self.context.arena().node_ref(self.source, expression))
                .and_then(|expression| self.context.arena().node(expression).ok())
                .and_then(|expression| {
                    matches!(&expression.data, NodeData::NumericLiteral(data) if data.text == "0")
                        .then_some("void:0".to_owned())
                }),
            NodeData::PropertyAccessExpression(data) => {
                let expression = data
                    .expression
                    .and_then(|expression| self.context.arena().node_ref(self.source, expression));
                let name = data
                    .name
                    .and_then(|name| self.context.arena().node_ref(self.source, name));
                match (expression, name) {
                    (Some(expression), Some(name)) => {
                        let left = self.serialized_type_key(expression)?;
                        let right = self.serialized_type_key(name)?;
                        left.zip(right)
                            .map(|(left, right)| format!("{left}.{right}"))
                    }
                    _ => None,
                }
            }
            _ => None,
        })
    }

    /// tsc-port: serializeTypeReferenceNode @6.0.3
    /// tsc-hash: 8c542ad686faae965916b6b92788bca6ec6fd36d4d190e6111d3777832bc9ad3
    /// tsc-span: _tsc.js:98331-98412
    fn serialize_entity_name_type(
        &mut self,
        type_reference: TransformNode,
        name: Option<NodeId>,
        fallback: MetadataFallback,
        serialization_context: MetadataSerializationContext,
    ) -> Result<TransformNode, TransformError> {
        let Some(name) = name else {
            return self.metadata_fallback(fallback);
        };
        let node = self.node(name);
        let kind =
            if let Some(resolver_node) = self.context.arena().parse_tree_resolver_node(node)? {
                self.resolver.get_type_reference_serialization_kind(
                    resolver_node,
                    serialization_context.resolver_location,
                )?
            } else {
                EmitTypeReferenceSerializationKind::Unknown
            };
        match kind {
            EmitTypeReferenceSerializationKind::Unknown => {
                // A conditional type serializes both branches before deciding
                // whether their runtime representations agree. Creating the
                // Unknown fallback here would hoist a temporary even though a
                // differing sibling immediately collapses the result to
                // `Object`. tsc deliberately short-circuits every type
                // reference nested in a direct true/false branch.
                if self.type_reference_is_in_conditional_branch(type_reference)? {
                    self.create_identifier("Object")
                } else {
                    self.serialize_unknown_entity_name_type(node, serialization_context)
                }
            }
            EmitTypeReferenceSerializationKind::TypeWithConstructSignatureAndValue => {
                self.entity_name_expression(node, serialization_context)
            }
            EmitTypeReferenceSerializationKind::VoidNullableOrNeverType => self.create_void_zero(),
            EmitTypeReferenceSerializationKind::NumberLikeType => self.create_identifier("Number"),
            EmitTypeReferenceSerializationKind::BigIntLikeType => self.create_identifier("BigInt"),
            EmitTypeReferenceSerializationKind::StringLikeType => self.create_identifier("String"),
            EmitTypeReferenceSerializationKind::BooleanType => self.create_identifier("Boolean"),
            EmitTypeReferenceSerializationKind::ArrayLikeType => self.create_identifier("Array"),
            EmitTypeReferenceSerializationKind::ESSymbolType => self.create_identifier("Symbol"),
            EmitTypeReferenceSerializationKind::Promise => self.create_identifier("Promise"),
            EmitTypeReferenceSerializationKind::TypeWithCallSignature => {
                self.create_identifier("Function")
            }
            EmitTypeReferenceSerializationKind::ObjectType => self.create_identifier("Object"),
        }
    }

    fn type_reference_is_in_conditional_branch(
        &self,
        type_reference: TransformNode,
    ) -> Result<bool, TransformError> {
        let mut current = type_reference;
        while let Some(parent) = self.context.arena().node(current)?.parent {
            let parent = self.node(parent);
            if let NodeData::ConditionalType(data) = &self.context.arena().node(parent)?.data {
                if data.true_type == Some(current.node()) || data.false_type == Some(current.node())
                {
                    return Ok(true);
                }
            }
            current = parent;
        }
        Ok(false)
    }

    fn serialize_unknown_entity_name_type(
        &mut self,
        name: TransformNode,
        serialization_context: MetadataSerializationContext,
    ) -> Result<TransformNode, TransformError> {
        let (guard, value) = self.checked_entity_name_parts(name, serialization_context)?;
        let checked = self.create_binary(guard, SyntaxKind::AmpersandAmpersandToken, value)?;
        let temp_name = self.allocate_temp_name(
            LegacyGeneratedBindingKind::MetadataTemp,
            LegacyTempBindingStorage::Hoisted,
        )?;
        let temp = self.create_generated_identifier(&temp_name)?;
        let assignment = self.create_assignment(temp, checked)?;
        let assignment = self.create_parenthesized(assignment)?;
        let type_of = self.create_typeof(assignment)?;
        let function = self.create_string_literal("function")?;
        let condition =
            self.create_binary(type_of, SyntaxKind::EqualsEqualsEqualsToken, function)?;
        let when_true = self.create_generated_identifier(&temp_name)?;
        let when_false = self.create_identifier("Object")?;
        self.create_conditional(condition, when_true, when_false)
    }

    fn checked_entity_name_parts(
        &mut self,
        name: TransformNode,
        serialization_context: MetadataSerializationContext,
    ) -> Result<(TransformNode, TransformNode), TransformError> {
        match self.context.arena().node(name)?.data.clone() {
            NodeData::Identifier(_) => {
                let left = self.entity_identifier_expression(name, serialization_context)?;
                let type_of = self.create_typeof(left)?;
                let undefined = self.create_string_literal("undefined")?;
                let guard = self.create_binary(
                    type_of,
                    SyntaxKind::ExclamationEqualsEqualsToken,
                    undefined,
                )?;
                let value = self.entity_identifier_expression(name, serialization_context)?;
                Ok((guard, value))
            }
            NodeData::QualifiedName(data) => {
                let left = data
                    .left
                    .and_then(|left| self.context.arena().node_ref(self.source, left))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::QualifiedName,
                        field: "left",
                    })?;
                let right = data
                    .right
                    .and_then(|right| self.context.arena().node_ref(self.source, right))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::QualifiedName,
                        field: "right",
                    })?;
                let right = self.identifier_text(right.node())?.to_owned();
                if matches!(
                    self.context.arena().node(left)?.data,
                    NodeData::Identifier(_)
                ) {
                    let (guard, left_value) =
                        self.checked_entity_name_parts(left, serialization_context)?;
                    let value = self.create_property_access(left_value, &right)?;
                    return Ok((guard, value));
                }
                let (left_guard, left_value) =
                    self.checked_entity_name_parts(left, serialization_context)?;
                let temp_name = self.allocate_temp_name(
                    LegacyGeneratedBindingKind::MetadataTemp,
                    LegacyTempBindingStorage::Hoisted,
                )?;
                let temp = self.create_generated_identifier(&temp_name)?;
                let assignment = self.create_assignment(temp, left_value)?;
                let void_zero = self.create_void_zero()?;
                let defined = self.create_binary(
                    assignment,
                    SyntaxKind::ExclamationEqualsEqualsToken,
                    void_zero,
                )?;
                let guard =
                    self.create_binary(left_guard, SyntaxKind::AmpersandAmpersandToken, defined)?;
                let temp = self.create_generated_identifier(&temp_name)?;
                let value = self.create_property_access(temp, &right)?;
                Ok((guard, value))
            }
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::TypeReference,
                field: "type name",
            }),
        }
    }

    fn entity_name_expression(
        &mut self,
        node: TransformNode,
        serialization_context: MetadataSerializationContext,
    ) -> Result<TransformNode, TransformError> {
        match self.context.arena().node(node)?.data.clone() {
            NodeData::Identifier(_) => {
                self.entity_identifier_expression(node, serialization_context)
            }
            NodeData::QualifiedName(data) => {
                let left = data
                    .left
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::QualifiedName,
                        field: "left",
                    })?;
                let right = data
                    .right
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::QualifiedName,
                        field: "right",
                    })?;
                let right = self.identifier_text(right.node())?.to_owned();
                let left = self.entity_name_expression(left, serialization_context)?;
                let expression = self.create_property_access(left, &right)?;
                self.set_original_and_range(expression, node)
            }
            _ => self.create_identifier("Object"),
        }
    }

    fn entity_identifier_expression(
        &mut self,
        node: TransformNode,
        serialization_context: MetadataSerializationContext,
    ) -> Result<TransformNode, TransformError> {
        let text = self.identifier_text(node.node())?.to_owned();
        let expression = self.create_identifier(&text)?;
        let expression = self.set_original_and_range(expression, node)?;
        let resolver_node = self.resolver_node(node)?;
        let declaration = self
            .resolver
            .get_referenced_import_declaration_at_location(
                resolver_node,
                serialization_context.resolver_location,
            )?;
        if let Some(declaration) = declaration {
            let current_source = self.context.arena().source(self.source)?.program_source();
            if current_source == Some(declaration.source()) {
                let declaration = self
                    .context
                    .arena()
                    .node_ref(self.source, declaration.node())
                    .ok_or_else(|| TransformError::UnknownNode(self.node(declaration.node())))?;
                self.context
                    .arena_mut()?
                    .metadata_mut(expression)
                    .set_referenced_import_declaration(declaration);
            }
        }
        Ok(expression)
    }

    fn metadata_fallback(
        &mut self,
        fallback: MetadataFallback,
    ) -> Result<TransformNode, TransformError> {
        match fallback {
            MetadataFallback::Object => self.create_identifier("Object"),
            MetadataFallback::VoidZero => self.create_void_zero(),
        }
    }

    /// tsc-port: getFirstConstructorWithBody @6.0.3
    /// tsc-hash: 9a7337f235fb939299cfc0513bfd74f5a61039c196919e9dde7af622e2557370
    /// tsc-span: _tsc.js:16674-16676
    fn first_constructor(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<Option<TransformNode>, TransformError> {
        for member in self.array_nodes(members)? {
            if matches!(
                &self.context.arena().node(member)?.data,
                NodeData::Constructor(data) if data.body.is_some()
            ) {
                return Ok(Some(member));
            }
        }
        Ok(None)
    }

    fn constructor_handoff(
        &self,
        current_members: Option<NodeArrayId>,
        original_members: Option<NodeArrayId>,
    ) -> Result<Option<ConstructorHandoff>, TransformError> {
        let Some(original_with_body) = self.first_constructor(original_members)? else {
            return Ok(None);
        };
        for current_with_body in self.array_nodes(current_members)? {
            if matches!(
                &self.context.arena().node(current_with_body)?.data,
                NodeData::Constructor(data) if data.body.is_some()
            ) && self.context.arena().get_original_node(current_with_body) == original_with_body
            {
                return Ok(Some(ConstructorHandoff {
                    original_with_body,
                    current_with_body,
                }));
            }
        }
        Err(TransformError::MissingTransformHandoff {
            producer: "transformTypeScript",
            consumer: "transformLegacyDecorators",
            node: original_with_body,
            handoff: "current constructor with body",
        })
    }

    fn class_has_member_decoration(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for member in self.array_nodes(members)? {
            if self.class_member_or_child_is_legacy_decorated(member)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn class_has_legacy_decorator_syntax(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for member in self.array_nodes(members)? {
            let (modifiers, parameters) = match &self.context.arena().node(member)?.data {
                NodeData::PropertyDeclaration(data) => (data.modifiers, None),
                NodeData::MethodDeclaration(data) => (data.modifiers, data.parameters),
                NodeData::GetAccessor(data) => (data.modifiers, data.parameters),
                NodeData::SetAccessor(data) => (data.modifiers, data.parameters),
                NodeData::Constructor(data) => (data.modifiers, data.parameters),
                _ => (None, None),
            };
            if !self.decorator_expressions(modifiers)?.is_empty() {
                return Ok(true);
            }
            for parameter in self.array_nodes(parameters)? {
                if self.is_this_parameter(parameter) {
                    continue;
                }
                let modifiers = match &self.context.arena().node(parameter)?.data {
                    NodeData::Parameter(data) => data.modifiers,
                    _ => None,
                };
                if !self.decorator_expressions(modifiers)?.is_empty() {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn class_element_has_body(&self, member: TransformNode) -> Result<bool, TransformError> {
        Ok(match &self.context.arena().node(member)?.data {
            NodeData::Constructor(data) => data.body.is_some(),
            NodeData::MethodDeclaration(data) => data.body.is_some(),
            NodeData::GetAccessor(data) => data.body.is_some(),
            NodeData::SetAccessor(data) => data.body.is_some(),
            _ => false,
        })
    }

    fn member_direct_decorator_admits_owner(
        &self,
        member: TransformNode,
    ) -> Result<bool, TransformError> {
        // Runtime admission observes transformTypeScript's current node. Its
        // recovery path may synthesize an accessor body that was absent from
        // the parse tree. Metadata/source admission deliberately remains on
        // the original node in the predicates below.
        let (modifiers, name, eligible) = match &self.context.arena().node(member)?.data {
            NodeData::PropertyDeclaration(data) => (data.modifiers, data.name, true),
            NodeData::MethodDeclaration(data) => (data.modifiers, data.name, data.body.is_some()),
            NodeData::GetAccessor(data) => (data.modifiers, data.name, data.body.is_some()),
            NodeData::SetAccessor(data) => (data.modifiers, data.name, data.body.is_some()),
            _ => (None, None, false),
        };
        Ok(eligible
            && !self.name_is_private_identifier(name)?
            && !self.decorator_expressions(modifiers)?.is_empty())
    }

    fn class_element_parameter_decorator_admits_owner(
        &self,
        member: TransformNode,
        parameter: TransformNode,
    ) -> Result<bool, TransformError> {
        let owner_has_runtime_body = match &self.context.arena().node(member)?.data {
            NodeData::Constructor(data) => data.body.is_some(),
            NodeData::MethodDeclaration(data) => data.body.is_some(),
            NodeData::SetAccessor(data) => data.body.is_some(),
            _ => false,
        };
        let NodeData::Parameter(data) = &self.context.arena().node(parameter)?.data else {
            return Ok(false);
        };
        Ok(owner_has_runtime_body
            && !self.is_this_parameter(parameter)
            && !self.name_is_private_identifier(data.name)?)
    }

    /// Admission is tsc's `nodeOrChildIsDecorated` contract, which is narrower
    /// than the later `getAllDecoratorsOf*` aggregation. In particular, a
    /// decorator directly on a private member cannot admit that member, while
    /// a runtime parameter decorator can; once admitted, aggregation retains
    /// both expressions for recovery emit.
    fn class_member_or_child_is_legacy_decorated(
        &self,
        member: TransformNode,
    ) -> Result<bool, TransformError> {
        let original = self.context.arena().get_original_node(member);
        let record = self.context.arena().node(original)?;
        let (modifiers, name, parameters, eligible) = match &record.data {
            NodeData::PropertyDeclaration(data) => (data.modifiers, data.name, None, true),
            NodeData::MethodDeclaration(data) => (
                data.modifiers,
                data.name,
                data.parameters,
                data.body.is_some(),
            ),
            NodeData::GetAccessor(data) => (data.modifiers, data.name, None, data.body.is_some()),
            NodeData::SetAccessor(data) => (
                data.modifiers,
                data.name,
                data.parameters,
                data.body.is_some(),
            ),
            _ => (None, None, None, false),
        };
        if !eligible {
            return Ok(false);
        }
        let direct = !self.name_is_private_identifier(name)?
            && !self.decorator_expressions(modifiers)?.is_empty();
        Ok(direct || self.class_element_parameter_list_is_legacy_decorated(parameters)?)
    }

    fn member_has_direct_decorator(&self, member: TransformNode) -> Result<bool, TransformError> {
        let original = self.context.arena().get_original_node(member);
        let modifiers = match &self.context.arena().node(original)?.data {
            NodeData::PropertyDeclaration(data) => data.modifiers,
            NodeData::MethodDeclaration(data) => data.modifiers,
            NodeData::GetAccessor(data) => data.modifiers,
            NodeData::SetAccessor(data) => data.modifiers,
            _ => None,
        };
        Ok(!self.decorator_expressions(modifiers)?.is_empty())
    }

    /// tsc-port: getAllDecoratorsOfClassElement/getAllDecoratorsOfMethod @6.0.3
    /// tsc-hash: 1e32c7d45db7aacd65944d5393ca3f632a42c037d03fd6f795874e7a240870f8
    /// tsc-span: _tsc.js:93146-93208
    fn metadata_decorator_owner(
        &self,
        member: TransformNode,
    ) -> Result<Option<DecoratorOwner>, TransformError> {
        let original = self.context.arena().get_original_node(member);
        let record = self.context.arena().node(original)?;
        match &record.data {
            NodeData::PropertyDeclaration(data) => {
                Ok((!self.decorator_expressions(data.modifiers)?.is_empty())
                    .then_some(DecoratorOwner::Property(original)))
            }
            NodeData::MethodDeclaration(data) => {
                if data.body.is_none() {
                    return Ok(None);
                }
                let decorated = !self.decorator_expressions(data.modifiers)?.is_empty()
                    || self.parameters_have_decorators(data.parameters)?;
                Ok(decorated.then_some(DecoratorOwner::MethodWithBody(original)))
            }
            NodeData::GetAccessor(data) => {
                if data.body.is_none() {
                    return Ok(None);
                }
                let Some(owner) = self.metadata_accessor_owner(original)? else {
                    return Ok(None);
                };
                Ok((owner == original).then_some(DecoratorOwner::AccessorWithBody(owner)))
            }
            NodeData::SetAccessor(data) => {
                if data.body.is_none() {
                    return Ok(None);
                }
                let Some(owner) = self.metadata_accessor_owner(original)? else {
                    return Ok(None);
                };
                Ok((owner == original).then_some(DecoratorOwner::AccessorWithBody(owner)))
            }
            _ => Ok(None),
        }
    }

    fn parameters_have_decorators(
        &self,
        parameters: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for parameter in self.array_nodes(parameters)? {
            if self.is_this_parameter(parameter) {
                continue;
            }
            let modifiers = match &self.context.arena().node(parameter)?.data {
                NodeData::Parameter(data) => data.modifiers,
                _ => None,
            };
            if !self.decorator_expressions(modifiers)?.is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn class_element_parameter_list_is_legacy_decorated(
        &self,
        parameters: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for parameter in self.array_nodes(parameters)? {
            let original = self.context.arena().get_original_node(parameter);
            let NodeData::Parameter(data) = &self.context.arena().node(original)?.data else {
                continue;
            };
            if self.decorator_expressions(data.modifiers)?.is_empty()
                || self.is_this_parameter(original)
                || self.name_is_private_identifier(data.name)?
            {
                continue;
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn constructor_parameter_list_is_legacy_decorated(
        &self,
        constructor: TransformNode,
    ) -> Result<bool, TransformError> {
        let parameters = match &self.context.arena().node(constructor)?.data {
            NodeData::Constructor(data) => data.parameters,
            _ => None,
        };
        for (index, parameter) in self.array_nodes(parameters)?.into_iter().enumerate() {
            let original = self.context.arena().get_original_node(parameter);
            let NodeData::Parameter(data) = &self.context.arena().node(original)?.data else {
                continue;
            };
            if self.decorator_expressions(data.modifiers)?.is_empty()
                || (index == 0 && self.is_this_parameter(original))
                || self.name_is_private_identifier(data.name)?
            {
                continue;
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn decorator_expressions(
        &self,
        modifiers: Option<NodeArrayId>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let mut decorators = Vec::new();
        for modifier in self.array_nodes(modifiers)? {
            let NodeData::Decorator(data) = &self.context.arena().node(modifier)?.data else {
                continue;
            };
            let expression = data
                .expression
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::Decorator,
                    field: "expression",
                })?;
            decorators.push(self.node(expression));
        }
        Ok(decorators)
    }

    fn name_is_private_identifier(&self, name: Option<NodeId>) -> Result<bool, TransformError> {
        let Some(name) = name else {
            return Ok(false);
        };
        Ok(self.context.arena().node(self.node(name))?.kind == SyntaxKind::PrivateIdentifier)
    }

    fn has_static_modifier(&self, member: TransformNode) -> Result<bool, TransformError> {
        let modifiers = match &self.context.arena().node(member)?.data {
            NodeData::PropertyDeclaration(data) => data.modifiers,
            NodeData::MethodDeclaration(data) => data.modifiers,
            NodeData::GetAccessor(data) => data.modifiers,
            NodeData::SetAccessor(data) => data.modifiers,
            _ => None,
        };
        self.has_modifier(modifiers, SyntaxKind::StaticKeyword)
    }

    fn strip_decorators(
        &mut self,
        modifiers: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        self.filter_modifiers(modifiers, |kind| kind != SyntaxKind::Decorator)
    }

    fn strip_class_declaration_modifiers(
        &mut self,
        modifiers: Option<NodeArrayId>,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        self.filter_modifiers(modifiers, |kind| {
            !matches!(
                kind,
                SyntaxKind::Decorator | SyntaxKind::ExportKeyword | SyntaxKind::DefaultKeyword
            )
        })
    }

    fn filter_modifiers(
        &mut self,
        modifiers: Option<NodeArrayId>,
        keep: impl Fn(SyntaxKind) -> bool,
    ) -> Result<Option<NodeArrayId>, TransformError> {
        let Some(modifiers) =
            modifiers.and_then(|id| self.context.arena().node_array_ref(self.source, id))
        else {
            return Ok(None);
        };
        let retained = self
            .context
            .arena()
            .node_array(modifiers)?
            .nodes
            .iter()
            .filter_map(|id| self.context.arena().node_ref(self.source, *id))
            .filter(|modifier| {
                self.context
                    .arena()
                    .node(*modifier)
                    .is_ok_and(|modifier| keep(modifier.kind))
            })
            .collect::<Vec<_>>();
        if retained.is_empty() {
            Ok(None)
        } else {
            Ok(Some(
                self.context
                    .factory()?
                    .update_node_array(modifiers, retained)?
                    .array(),
            ))
        }
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

    fn property_name_expression(
        &mut self,
        member: TransformNode,
        name: NodeId,
        plan: DecoratorComputedNamePlan,
    ) -> Result<TransformNode, TransformError> {
        let original_member = self.context.arena().get_original_node(member).node();
        if plan.caches_at_class_evaluation() {
            if let Some(generated) = self.computed_names.get(&original_member).cloned() {
                return self.create_generated_identifier(&generated);
            }
        }
        let name = self.node(name);
        match self.context.arena().node(name)?.data.clone() {
            NodeData::Identifier(data) => self.create_string_literal(&data.text),
            NodeData::StringLiteral(_)
            | NodeData::NumericLiteral(_)
            | NodeData::BigIntLiteral(_)
            | NodeData::NoSubstitutionTemplateLiteral(_) => {
                self.context.factory()?.clone_node(name)
            }
            NodeData::ComputedPropertyName(data) => {
                let expression = data
                    .expression
                    .and_then(|expression| self.context.arena().node_ref(self.source, expression))
                    .ok_or(TransformError::RequiredChildRemoved {
                        parent: SyntaxKind::ComputedPropertyName,
                        field: "expression",
                    })?;
                if !self.computed_name_helper_requires_generated_reference(member, plan)? {
                    Ok(expression)
                } else {
                    // A missing class-evaluation cache is observable tsc
                    // recovery behavior: the helper still references the
                    // generated identity, but no declaration or key assignment
                    // is synthesized retroactively.
                    let generated = self.allocate_temp_name(
                        LegacyGeneratedBindingKind::DecoratedComputedName,
                        LegacyTempBindingStorage::ReferenceOnly,
                    )?;
                    self.create_generated_identifier(&generated)
                }
            }
            NodeData::PrivateIdentifier(_) => self.create_identifier(""),
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::PropertyDeclaration,
                field: "property name",
            }),
        }
    }

    fn computed_name_helper_requires_generated_reference(
        &self,
        member: TransformNode,
        plan: DecoratorComputedNamePlan,
    ) -> Result<bool, TransformError> {
        if plan == DecoratorComputedNamePlan::AmbientExpression {
            return Ok(false);
        }
        let original = self.context.arena().get_original_node(member);
        let name = match &self.context.arena().node(original)?.data {
            NodeData::PropertyDeclaration(data) => data.name,
            NodeData::MethodDeclaration(data) => data.name,
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            _ => None,
        };
        let Some(name) = name.and_then(|name| self.context.arena().node_ref(self.source, name))
        else {
            return Ok(false);
        };
        let NodeData::ComputedPropertyName(data) = &self.context.arena().node(name)?.data else {
            return Ok(false);
        };
        let expression = data
            .expression
            .and_then(|expression| self.context.arena().node_ref(self.source, expression))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ComputedPropertyName,
                field: "expression",
            })?;
        Ok(!self.is_simple_inlineable_expression(expression)?)
    }

    fn decorator_computed_name_plan(
        &self,
        member: TransformNode,
    ) -> Result<DecoratorComputedNamePlan, TransformError> {
        let record = self.context.arena().node(member)?;
        let ambient = NodeFlags::from_bits(record.flags).contains(NodeFlags::AMBIENT);
        let modifiers = match &record.data {
            NodeData::PropertyDeclaration(data) => data.modifiers,
            NodeData::MethodDeclaration(data) => data.modifiers,
            NodeData::GetAccessor(data) => data.modifiers,
            NodeData::SetAccessor(data) => data.modifiers,
            _ => None,
        };
        if ambient || self.has_modifier(modifiers, SyntaxKind::DeclareKeyword)? {
            Ok(DecoratorComputedNamePlan::AmbientExpression)
        } else if self.member_has_direct_decorator(member)? {
            Ok(DecoratorComputedNamePlan::ClassEvaluationCacheCandidate)
        } else {
            Ok(DecoratorComputedNamePlan::HelperReferenceOnly)
        }
    }

    fn rewrite_computed_member_name(
        &mut self,
        member: TransformNode,
        name: Option<NodeId>,
    ) -> Result<Option<NodeId>, TransformError> {
        let original_member = self.context.arena().get_original_node(member).node();
        let Some(generated) = self.computed_names.get(&original_member).cloned() else {
            return Ok(name);
        };
        let Some(name) = name.and_then(|name| self.context.arena().node_ref(self.source, name))
        else {
            return Ok(name);
        };
        let NodeData::ComputedPropertyName(data) = self.context.arena().node(name)?.data.clone()
        else {
            return Ok(Some(name.node()));
        };
        let expression = data
            .expression
            .and_then(|expression| self.context.arena().node_ref(self.source, expression))
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ComputedPropertyName,
                field: "expression",
            })?;
        let target = self.create_generated_identifier(&generated)?;
        let assignment = self.create_assignment(target, expression)?;
        let computed = self.context.factory()?.create_node(
            self.source,
            NodeData::ComputedPropertyName(tsc_syntax::nodes::ComputedPropertyNameData {
                expression: Some(assignment.node()),
            }),
            TransformFlags::NONE,
        )?;
        self.context
            .arena_mut()?
            .metadata_mut(computed)
            .set_internal_flags(InternalEmitFlags::GENERATED_COMPUTED_PROPERTY_NAME);
        self.set_original_and_range(computed, name)?;
        Ok(Some(computed.node()))
    }

    fn class_alias_for_reference(
        &self,
        node: TransformNode,
    ) -> Result<Option<TargetBinding>, TransformError> {
        let Some(resolver_node) = self.context.arena().parse_tree_resolver_node(node)? else {
            return Ok(None);
        };
        let has_flag = match self.resolver.has_node_check_flag(
            resolver_node,
            NodeCheckFlags::CONSTRUCTOR_REFERENCE.bits() as u32,
        ) {
            Ok(has_flag) => has_flag,
            Err(EmitResolverError::UnknownNode { .. }) => false,
            Err(error) => return Err(error.into()),
        };
        if !has_flag {
            return Ok(None);
        }
        let declaration = match self
            .resolver
            .get_referenced_value_declaration(resolver_node)
        {
            Ok(declaration) => declaration,
            Err(EmitResolverError::UnknownNode { .. }) => None,
            Err(error) => return Err(error.into()),
        };
        Ok(
            declaration
                .and_then(|declaration| self.class_aliases.get(&declaration.node()).cloned()),
        )
    }

    fn class_has_static_property_or_block(
        &self,
        members: Option<NodeArrayId>,
    ) -> Result<bool, TransformError> {
        for member in self.array_nodes(members)? {
            match &self.context.arena().node(member)?.data {
                NodeData::ClassStaticBlockDeclaration(_) => return Ok(true),
                NodeData::PropertyDeclaration(data)
                    if self.has_modifier(data.modifiers, SyntaxKind::StaticKeyword)? =>
                {
                    return Ok(true);
                }
                _ => {}
            }
        }
        Ok(false)
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

    fn allocate_class_alias(&mut self, class_name: &str) -> Result<TargetBinding, TransformError> {
        let base = class_name.trim_end_matches('_');
        // The source-wide ordinal is an output-tree property, not a transform
        // visitation property. Give every alias identity the same optimistic
        // spelling and let the generated-binding finalizer assign C_1/C_2 in
        // declaration order. Distinct TargetBinding ids keep references
        // correlated while the spelling remains provisional.
        let candidate = format!("{base}_1");
        let binding = self.create_target_binding(
            LegacyGeneratedBindingKind::ClassAlias,
            candidate,
            Some(base),
        )?;
        self.lexical_binding_frames
            .last_mut()
            .expect("legacy decorator owns a lexical binding frame")
            .class_aliases
            .push(binding.clone());
        Ok(binding)
    }

    fn allocate_generated_class_name(&mut self, base: &str) -> String {
        let mut ordinal = 1usize;
        loop {
            let candidate = format!("{base}_{ordinal}");
            if self.used_names.insert(candidate.clone()) {
                return candidate;
            }
            ordinal += 1;
        }
    }

    fn allocate_temp_name(
        &mut self,
        kind: LegacyGeneratedBindingKind,
        storage: LegacyTempBindingStorage,
    ) -> Result<TargetBinding, TransformError> {
        let candidate = match kind {
            LegacyGeneratedBindingKind::MetadataTemp => self.generated_bindings.allocate_temp(),
            LegacyGeneratedBindingKind::DecoratedComputedName => {
                self.generated_bindings.allocate_temp_with_policy(true)
            }
            LegacyGeneratedBindingKind::ClassAlias => {
                unreachable!("class aliases use source-wide numbered allocation")
            }
        };
        let binding = self.create_target_binding(kind, candidate, None)?;
        if storage == LegacyTempBindingStorage::Hoisted {
            self.lexical_binding_frames
                .last_mut()
                .expect("legacy decorator owns a lexical binding frame")
                .producer_temps
                .push(binding.clone());
        }
        Ok(binding)
    }

    fn create_target_binding(
        &mut self,
        kind: LegacyGeneratedBindingKind,
        provisional_name: String,
        numbered_base: Option<&str>,
    ) -> Result<TargetBinding, TransformError> {
        match kind {
            LegacyGeneratedBindingKind::MetadataTemp => {
                debug_assert!(numbered_base.is_none());
                TargetBinding::allocate_planned(self.context, provisional_name)
            }
            LegacyGeneratedBindingKind::DecoratedComputedName => {
                debug_assert!(numbered_base.is_none());
                TargetBinding::allocate_planned_reserved_in_nested_scopes(
                    self.context,
                    provisional_name,
                )
            }
            LegacyGeneratedBindingKind::ClassAlias => TargetBinding::allocate_numbered(
                self.context,
                numbered_base
                    .expect("class alias owns a source-wide numbered base")
                    .to_owned(),
                provisional_name,
            ),
        }
    }

    fn prepend_hoisted_declarations(
        &mut self,
        root: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        debug_assert_eq!(self.lexical_binding_frames.len(), 1);
        debug_assert!(self.generated_binding_scope_stack.is_empty());
        debug_assert!(self.preentered_function_scopes.is_empty());
        let bindings = std::mem::take(&mut self.lexical_binding_frames[0]);
        let _ = self.generated_bindings.source_bindings();
        if bindings.is_empty() {
            return Ok(root);
        }
        let NodeData::SourceFile(mut data) = self.context.arena().node(root)?.data.clone() else {
            return Err(TransformError::RootKindExpected {
                actual: self.context.arena().node(root)?.kind,
            });
        };
        let hoisted = self.create_hoisted_variable_statements(&bindings)?;
        let mut statements = self.array_nodes(data.statements)?;
        let mut position = 0;
        while position < statements.len()
            && is_prologue_statement(self.context.arena(), statements[position])?
        {
            position += 1;
        }
        statements.splice(position..position, hoisted);
        data.statements = Some(
            self.context
                .factory()?
                .create_node_array(self.source, statements)?
                .array(),
        );
        let flags = self.context.arena().transform_flags(root);
        self.context
            .factory()?
            .update_node(root, NodeData::SourceFile(data), flags)
    }

    fn create_named_export(
        &mut self,
        name: &str,
        declaration_name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
        // tsc passes `factory.getDeclarationName(node)` to
        // `createExternalModuleExport`. Preserve that declaration identity:
        // collectExternalModuleInfo asks the resolver for the value
        // declaration behind this synthetic specifier, and the later module
        // substitution uses the resulting exported-binding relation.
        self.set_original_and_range(name, declaration_name)?;
        let specifier = self.context.factory()?.create_node(
            self.source,
            NodeData::ExportSpecifier(tsc_syntax::nodes::ExportSpecifierData {
                name: Some(name.node()),
                property_name: None,
                is_type_only: false,
            }),
            TransformFlags::NONE,
        )?;
        let elements = self
            .context
            .factory()?
            .create_node_array(self.source, vec![specifier])?;
        let clause = self.context.factory()?.create_node(
            self.source,
            NodeData::NamedExports(tsc_syntax::nodes::NamedExportsData {
                elements: Some(elements.array()),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ExportDeclaration(tsc_syntax::nodes::ExportDeclarationData {
                modifiers: None,
                is_type_only: false,
                export_clause: Some(clause.node()),
                module_specifier: None,
                attributes: None,
            }),
            TransformFlags::NONE,
        )
    }

    fn create_export_default(&mut self, name: &str) -> Result<TransformNode, TransformError> {
        let name = self.create_identifier(name)?;
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

    fn create_decorate_call(
        &mut self,
        decorators: Vec<TransformNode>,
        target: TransformNode,
        member_name: Option<TransformNode>,
        descriptor: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(EmitHelper::with_text(
            "typescript:decorate",
            false,
            DECORATE_HELPER_TEXT,
            Some(2),
            Vec::new(),
        ))?;
        let decorators = self.create_array_literal(decorators, true)?;
        let mut arguments = vec![decorators, target];
        if let Some(member_name) = member_name {
            arguments.push(member_name);
            if let Some(descriptor) = descriptor {
                arguments.push(descriptor);
            }
        }
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::Decorate)?;
        self.create_call(helper, arguments)
    }

    fn create_metadata(
        &mut self,
        key: &str,
        value: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(EmitHelper::with_text(
            "typescript:metadata",
            false,
            METADATA_HELPER_TEXT,
            Some(3),
            Vec::new(),
        ))?;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::Metadata)?;
        let key = self.create_string_literal(key)?;
        self.create_call(helper, vec![key, value])
    }

    fn create_param(
        &mut self,
        index: usize,
        decorator: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.request_emit_helper(EmitHelper::with_text(
            "typescript:param",
            false,
            PARAM_HELPER_TEXT,
            Some(4),
            Vec::new(),
        ))?;
        let helper = self
            .context
            .factory()?
            .create_unscoped_helper_identifier(self.source, EmitHelperName::Param)?;
        let index = self.create_numeric_literal(&index.to_string())?;
        self.create_call(helper, vec![index, decorator])
    }

    fn create_identifier(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::Identifier(tsc_syntax::nodes::IdentifierData {
                escaped_text: text.to_owned(),
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_generated_identifier(
        &mut self,
        binding: &TargetBinding,
    ) -> Result<TransformNode, TransformError> {
        let text = binding.printable_text(self.context).to_owned();
        let identifier = self.create_identifier(&text)?;
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

    fn create_numeric_literal(&mut self, text: &str) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::NumericLiteral(tsc_syntax::nodes::NumericLiteralData {
                text: text.to_owned(),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_null(&mut self) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_token(
            self.source,
            SyntaxKind::NullKeyword,
            TransformFlags::NONE,
        )
    }

    fn create_void_zero(&mut self) -> Result<TransformNode, TransformError> {
        let zero = self.create_numeric_literal("0")?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::VoidExpression(tsc_syntax::nodes::VoidExpressionData {
                expression: Some(zero.node()),
            }),
            TransformFlags::NONE,
        )
    }

    fn create_typeof(
        &mut self,
        expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context.factory()?.create_node(
            self.source,
            NodeData::TypeOfExpression(tsc_syntax::nodes::TypeOfExpressionData {
                expression: Some(expression.node()),
            }),
            TransformFlags::NONE,
        )
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

    fn create_assignment(
        &mut self,
        left: TransformNode,
        right: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let equals = self.context.factory()?.create_token(
            self.source,
            SyntaxKind::EqualsToken,
            TransformFlags::NONE,
        )?;
        self.context.factory()?.create_node(
            self.source,
            NodeData::BinaryExpression(tsc_syntax::nodes::BinaryExpressionData {
                left: Some(left.node()),
                operator_token: Some(equals.node()),
                right: Some(right.node()),
            }),
            TransformFlags::NONE,
        )
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
        self.context.factory()?.create_node(
            self.source,
            NodeData::ConditionalExpression(tsc_syntax::nodes::ConditionalExpressionData {
                condition: Some(condition.node()),
                question_token: Some(question.node()),
                when_true: Some(when_true.node()),
                colon_token: Some(colon.node()),
                when_false: Some(when_false.node()),
            }),
            TransformFlags::NONE,
        )
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

    fn create_array_literal(
        &mut self,
        elements: Vec<TransformNode>,
        multi_line: bool,
    ) -> Result<TransformNode, TransformError> {
        let elements = self
            .context
            .factory()?
            .create_node_array(self.source, elements)?;
        let array = self.context.factory()?.create_node(
            self.source,
            NodeData::ArrayLiteralExpression(tsc_syntax::nodes::ArrayLiteralExpressionData {
                elements: Some(elements.array()),
            }),
            TransformFlags::NONE,
        )?;
        self.context.factory()?.set_multi_line(array, multi_line)
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

    fn enter_function_lexical_environment(&mut self) {
        self.enter_lexical_environment(GeneratedBindingOwner::FunctionBody);
    }

    fn enter_static_lexical_environment(&mut self) {
        self.enter_lexical_environment(GeneratedBindingOwner::StaticEvaluation);
    }

    fn enter_lexical_environment(&mut self, owner: GeneratedBindingOwner) {
        let scope = self.generated_bindings.enter(owner);
        self.generated_binding_scope_stack.push(scope);
        self.lexical_binding_frames
            .push(LegacyLexicalBindingFrame::default());
    }

    fn abandon_preentered_function_scope(&mut self, function: TransformNode) {
        if self.preentered_function_scopes.remove(&function.node()) {
            let _ = self.exit_lexical_environment();
        }
    }

    fn exit_lexical_environment(&mut self) -> LegacyLexicalBindingFrame {
        let frame = self
            .lexical_binding_frames
            .pop()
            .expect("legacy decorator owns a lexical binding frame");
        debug_assert!(!self.lexical_binding_frames.is_empty());
        let (previous, completed) = self
            .generated_binding_scope_stack
            .pop()
            .expect("legacy decorator owns a generated-name lexical scope");
        let _ = self.generated_bindings.exit(previous, completed);
        frame
    }

    /// Visit the parse-time name/modifier surface in the parent environment,
    /// then enter the function environment for parameters and the body. This
    /// is the same boundary used by tsc's transform lexical environment: a
    /// computed method name is class-evaluation work, while a nested decorated
    /// class in a parameter initializer or body hoists into that function.
    fn update_class_static_block(
        &mut self,
        original: TransformNode,
        mut data: tsc_syntax::nodes::ClassStaticBlockDeclarationData,
    ) -> Result<NodeId, TransformError> {
        let body = data
            .body
            .take()
            .ok_or(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ClassStaticBlockDeclaration,
                field: "body",
            })?;
        let mut surface = NodeData::ClassStaticBlockDeclaration(data);
        try_visit_each_child(&mut surface, self)?;
        let NodeData::ClassStaticBlockDeclaration(mut data) = surface else {
            unreachable!("class-static-block surface retains its node kind");
        };
        self.enter_static_lexical_environment();
        let visited_body = self.visit(body);
        let bindings = self.exit_lexical_environment();
        let body = visited_body?;
        data.body = self.attach_function_hoists(body, bindings, false)?;
        let node_data = NodeData::ClassStaticBlockDeclaration(data);
        let flags = flags_after_update(self.context.arena(), original, &node_data)?;
        Ok(self
            .context
            .factory()?
            .update_node(original, node_data, flags)?
            .node())
    }

    fn update_function_like(
        &mut self,
        original: TransformNode,
        mut data: NodeData,
    ) -> Result<NodeId, TransformError> {
        let concise_arrow_body = match &data {
            NodeData::ArrowFunction(data) => data
                .body
                .and_then(|body| self.context.arena().node_ref(self.source, body))
                .is_some_and(|body| {
                    self.context
                        .arena()
                        .node(body)
                        .is_ok_and(|body| body.kind != SyntaxKind::Block)
                }),
            _ => false,
        };
        let (parameters, body) = match &mut data {
            NodeData::ArrowFunction(data) => (data.parameters.take(), data.body.take()),
            NodeData::Constructor(data) => (data.parameters.take(), data.body.take()),
            NodeData::FunctionDeclaration(data) => (data.parameters.take(), data.body.take()),
            NodeData::FunctionExpression(data) => (data.parameters.take(), data.body.take()),
            NodeData::GetAccessor(data) => (data.parameters.take(), data.body.take()),
            NodeData::MethodDeclaration(data) => (data.parameters.take(), data.body.take()),
            NodeData::SetAccessor(data) => (data.parameters.take(), data.body.take()),
            _ => {
                return Err(TransformError::RequiredChildRemoved {
                    parent: self.context.arena().node(original)?.kind,
                    field: "function-like declaration",
                });
            }
        };

        // Names, computed names, modifiers, type parameters, and return types
        // belong to the parent name-generation environment.
        try_visit_each_child(&mut data, self)?;

        let preentered = self.preentered_function_scopes.remove(&original.node());
        if !preentered {
            self.enter_function_lexical_environment();
        }
        let visited = (|| {
            let parameters = match parameters {
                Some(parameters) => self.visit_nodes(parameters)?,
                None => None,
            };
            let body = match body {
                Some(body) => self.visit(body)?,
                None => None,
            };
            Ok::<_, TransformError>((parameters, body))
        })();
        let bindings = self.exit_lexical_environment();
        let (parameters, body) = visited?;
        let body = self.attach_function_hoists(body, bindings, concise_arrow_body)?;

        match &mut data {
            NodeData::ArrowFunction(data) => {
                data.parameters = parameters;
                data.body = body;
            }
            NodeData::Constructor(data) => {
                data.parameters = parameters;
                data.body = body;
            }
            NodeData::FunctionDeclaration(data) => {
                data.parameters = parameters;
                data.body = body;
            }
            NodeData::FunctionExpression(data) => {
                data.parameters = parameters;
                data.body = body;
            }
            NodeData::GetAccessor(data) => {
                data.parameters = parameters;
                data.body = body;
            }
            NodeData::MethodDeclaration(data) => {
                data.parameters = parameters;
                data.body = body;
            }
            NodeData::SetAccessor(data) => {
                data.parameters = parameters;
                data.body = body;
            }
            _ => unreachable!("function-like data was validated above"),
        }
        let flags = flags_after_update(self.context.arena(), original, &data)?;
        Ok(self
            .context
            .factory()?
            .update_node(original, data, flags)?
            .node())
    }

    fn attach_function_hoists(
        &mut self,
        body: Option<NodeId>,
        bindings: LegacyLexicalBindingFrame,
        concise_arrow_body: bool,
    ) -> Result<Option<NodeId>, TransformError> {
        if bindings.is_empty() {
            return Ok(body);
        }
        let Some(body) = body.map(|body| self.node(body)) else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::FunctionDeclaration,
                field: "function body owning generated bindings",
            });
        };
        let body = if self.context.arena().node(body)?.kind == SyntaxKind::Block {
            self.prepend_bindings_to_block(body, bindings)?
        } else if concise_arrow_body {
            let return_statement = self.context.factory()?.create_node(
                self.source,
                NodeData::ReturnStatement(tsc_syntax::nodes::ReturnStatementData {
                    expression: Some(body.node()),
                }),
                TransformFlags::NONE,
            )?;
            let block = self.create_block(vec![return_statement], false)?;
            self.prepend_bindings_to_block(block, bindings)?
        } else {
            return Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::FunctionDeclaration,
                field: "block function body",
            });
        };
        Ok(Some(body.node()))
    }

    fn prepend_bindings_to_block(
        &mut self,
        block: TransformNode,
        bindings: LegacyLexicalBindingFrame,
    ) -> Result<TransformNode, TransformError> {
        let NodeData::Block(mut data) = self.context.arena().node(block)?.data.clone() else {
            return Err(TransformError::RequiredChildRemoved {
                parent: self.context.arena().node(block)?.kind,
                field: "function body block",
            });
        };
        let hoisted = self.create_hoisted_variable_statements(&bindings)?;
        let mut statements = self.array_nodes(data.statements)?;
        let mut position = 0;
        while position < statements.len()
            && is_prologue_statement(self.context.arena(), statements[position])?
        {
            position += 1;
        }
        statements.splice(position..position, hoisted);
        data.statements = Some(
            self.context
                .factory()?
                .create_node_array(self.source, statements)?
                .array(),
        );
        let flags = self.context.arena().transform_flags(block);
        self.context
            .factory()?
            .update_node(block, NodeData::Block(data), flags)
    }

    fn create_hoisted_variable_statement(
        &mut self,
        bindings: &[TargetBinding],
    ) -> Result<TransformNode, TransformError> {
        let mut declarations = Vec::with_capacity(bindings.len());
        for binding in bindings {
            let name = self.create_generated_identifier(binding)?;
            declarations.push(self.context.factory()?.create_node(
                self.source,
                NodeData::VariableDeclaration(tsc_syntax::nodes::VariableDeclarationData {
                    name: Some(name.node()),
                    exclamation_token: None,
                    r#type: None,
                    initializer: None,
                }),
                TransformFlags::NONE,
            )?);
        }
        let hoisted = self.create_variable_statement(declarations, NodeFlags::NONE)?;
        self.context
            .arena_mut()?
            .metadata_mut(hoisted)
            .add_flags(EmitFlags::CUSTOM_PROLOGUE);
        Ok(hoisted)
    }

    fn create_hoisted_variable_statements(
        &mut self,
        bindings: &LegacyLexicalBindingFrame,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let mut statements = Vec::with_capacity(2);
        // transformLegacyDecorators hoists class aliases in its own epoch;
        // transformTypeScript's metadata/computed-name temps follow in a
        // separate statement even though both epochs share one lexical owner.
        if !bindings.class_aliases.is_empty() {
            statements.push(self.create_hoisted_variable_statement(&bindings.class_aliases)?);
        }
        if !bindings.producer_temps.is_empty() {
            statements.push(self.create_hoisted_variable_statement(&bindings.producer_temps)?);
        }
        Ok(statements)
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

    fn set_original_only(
        &mut self,
        node: TransformNode,
        original: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.context
            .arena_mut()?
            .set_original_node(node, Some(original))?;
        Ok(node)
    }

    fn raw_comment_range(&self, node: TransformNode) -> Result<CommentRange, TransformError> {
        let source = self.context.arena().source(node.source())?.syntax();
        let record = self.context.arena().node(node)?;
        let range = SourceRange::from_raw(record.pos, record.end, source.positions())
            .map_err(|error| TransformError::InvalidSourceRange { node, error })?;
        Ok(CommentRange::new(node.source(), range))
    }

    /// Typed class-declaration branch of tsc's `moveRangePastModifiers`.
    ///
    /// tsc-port: moveRangePastModifiers @6.0.3
    /// tsc-span: _tsc.js:17311-17318
    fn move_range_past_modifiers(
        &self,
        declaration: TransformNode,
        modifiers: Option<NodeArrayId>,
    ) -> Result<RangePastModifiers, TransformError> {
        let declaration_record = self.context.arena().node(declaration)?.clone();
        let mut last_modifier_end = None;
        let mut last_decorator_end = None;
        for modifier in self.array_nodes(modifiers)? {
            let record = self.context.arena().node(modifier)?;
            last_modifier_end = Some(record.end);
            if record.kind == SyntaxKind::Decorator {
                last_decorator_end = Some(record.end);
            }
        }
        let start = last_modifier_end
            .filter(|end| *end != u32::MAX)
            .or_else(|| last_decorator_end.filter(|end| *end != u32::MAX))
            .unwrap_or(declaration_record.pos);
        let source = self.context.arena().source(declaration.source())?.syntax();
        let range = SourceRange::from_raw(start, declaration_record.end, source.positions())
            .map_err(|error| TransformError::InvalidSourceRange {
                node: declaration,
                error,
            })?;
        Ok(RangePastModifiers {
            source: declaration.source(),
            range,
        })
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

    fn identifier_text(&self, id: NodeId) -> Result<&str, TransformError> {
        match &self.context.arena().node(self.node(id))?.data {
            NodeData::Identifier(data) => Ok(&data.text),
            _ => Err(TransformError::RequiredChildRemoved {
                parent: SyntaxKind::ClassDeclaration,
                field: "identifier name",
            }),
        }
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

    const fn node(&self, id: NodeId) -> TransformNode {
        TransformNode::new(self.source, id)
    }

    const fn array(&self, id: NodeArrayId) -> TransformNodeArray {
        TransformNodeArray::new(self.source, id)
    }
}

impl NodeDataChildVisitor for LegacyDecoratorVisitor<'_, '_> {
    type Error = TransformError;

    fn node_kind(&self, id: NodeId) -> SyntaxKind {
        self.context
            .arena()
            .node(self.node(id))
            .expect("legacy-decorator child belongs to its transform source")
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
            if self.context.arena().node(self.node(node))?.kind == SyntaxKind::ClassDeclaration {
                visited.extend(self.visit_class_declaration(node)?);
            } else if let Some(node) = self.visit(node)? {
                visited.push(self.node(node));
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

#[derive(Clone, Copy)]
enum MetadataFallback {
    Object,
    VoidZero,
}
